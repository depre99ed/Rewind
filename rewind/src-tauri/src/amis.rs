//! Partage de clips entre amis (2026-11-15).
//!
//! Tout se passe dans Rewind. Le « guichet » est un Worker Cloudflare (code : `rewind/partage`) qui sert
//! de boîte aux lettres : l'envoyeur dépose le clip, l'ami le relève, il est supprimé dès réception
//! (au plus tard 7 jours). Aucun compte : ce Rewind crée une paire de clés Ed25519 (clé privée
//! protégée par Windows, DPAPI) et signe chacune de ses demandes. Rien n'est envoyé tant que
//! l'utilisateur n'a pas activé le partage (réglage `share_enabled`).
//!
//! Fichiers (dans `<données>\amis\`) : `cle.bin` (clé privée chiffrée par Windows), `compte.json`
//! (identifiant, code ami, pseudo), `recus.json` (historique des clips reçus), `miniatures\`.

use crate::{AppState, ffmpeg, library, util};
use anyhow::{Context, Result, anyhow, bail};
use base64::Engine;
use ed25519_dalek::{Signer, SigningKey};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::collections::HashSet;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter, Manager};

const DEFAULT_URL: &str = "https://rewind-partage.rewind-clips.workers.dev";
/// Relève de la boîte quand Rewind est ouvert (et tout de suite sur demande de l'interface)
const POLL: Duration = Duration::from_secs(120);

fn base_url() -> String {
    std::env::var("REWIND_SHARE_URL").unwrap_or_else(|_| DEFAULT_URL.into()).trim_end_matches('/').to_string()
}

fn dir() -> PathBuf {
    let d = util::data_dir().join("amis");
    let _ = std::fs::create_dir_all(&d);
    d
}

fn b64(bytes: &[u8]) -> String {
    base64::engine::general_purpose::STANDARD.encode(bytes)
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn now_ms() -> u64 {
    (util::now() * 1000.0) as u64
}

/// Erreur lisible par l'interface : « ami:<code> » (traduite côté interface).
fn coded(code: &str) -> anyhow::Error {
    anyhow!("ami:{code}")
}

// ---------------------------------------------------------------------------
// Identité : clé privée chiffrée par Windows (seul ce compte Windows peut la relire)
// ---------------------------------------------------------------------------

fn dpapi(data: &[u8], protect: bool) -> Result<Vec<u8>> {
    use windows_sys::Win32::Foundation::LocalFree;
    use windows_sys::Win32::Security::Cryptography::{CRYPT_INTEGER_BLOB, CryptProtectData, CryptUnprotectData};
    unsafe {
        let input = CRYPT_INTEGER_BLOB { cbData: data.len() as u32, pbData: data.as_ptr() as *mut u8 };
        let mut out = CRYPT_INTEGER_BLOB { cbData: 0, pbData: std::ptr::null_mut() };
        let ok = if protect {
            CryptProtectData(&input, std::ptr::null(), std::ptr::null(), std::ptr::null(), std::ptr::null(), 0, &mut out)
        } else {
            CryptUnprotectData(&input, std::ptr::null_mut(), std::ptr::null(), std::ptr::null(), std::ptr::null(), 0, &mut out)
        };
        if ok == 0 || out.pbData.is_null() {
            bail!("clé illisible (protection Windows)");
        }
        let v = std::slice::from_raw_parts(out.pbData, out.cbData as usize).to_vec();
        LocalFree(out.pbData as _);
        Ok(v)
    }
}

fn key_path() -> PathBuf {
    dir().join("cle.bin")
}

fn load_key() -> Option<SigningKey> {
    let raw = std::fs::read(key_path()).ok()?;
    let seed: [u8; 32] = dpapi(&raw, false).ok()?.try_into().ok()?;
    Some(SigningKey::from_bytes(&seed))
}

fn load_or_create_key() -> Result<SigningKey> {
    if let Some(k) = load_key() {
        return Ok(k);
    }
    let mut seed = [0u8; 32];
    getrandom::getrandom(&mut seed).map_err(|e| anyhow!("hasard indisponible : {e}"))?;
    std::fs::write(key_path(), dpapi(&seed, true)?)?;
    Ok(SigningKey::from_bytes(&seed))
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Account {
    pub id: String,
    pub code: String,
    pub name: String,
    /// Version de ma photo de profil (0 = aucune)
    pub avatar: u64,
    /// Montrer à mes amis quand Rewind est ouvert
    pub show_online: bool,
}

fn account_path() -> PathBuf {
    dir().join("compte.json")
}

fn load_account() -> Option<Account> {
    serde_json::from_str::<Account>(&std::fs::read_to_string(account_path()).ok()?).ok().filter(|a| !a.id.is_empty())
}

fn save_account(a: &Account) {
    let _ = std::fs::write(account_path(), serde_json::to_string_pretty(a).unwrap_or_default());
}

// ---------------------------------------------------------------------------
// Demandes signées
// ---------------------------------------------------------------------------

fn agent() -> &'static ureq::Agent {
    static AGENT: OnceLock<ureq::Agent> = OnceLock::new();
    AGENT.get_or_init(|| {
        ureq::AgentBuilder::new()
            .timeout_connect(Duration::from_secs(10))
            .timeout_read(Duration::from_secs(90))
            .timeout_write(Duration::from_secs(600))
            .user_agent(concat!("Rewind/", env!("CARGO_PKG_VERSION")))
            .build()
    })
}

fn signed(method: &str, path: &str, body_hash: &str) -> Result<ureq::Request> {
    let key = load_key().ok_or_else(|| coded("no_identity"))?;
    let acc = load_account().ok_or_else(|| coded("no_identity"))?;
    let time = now_ms();
    let message = format!("{method}\n{path}\n{time}\n{body_hash}");
    let sig = b64(&key.sign(message.as_bytes()).to_bytes());
    Ok(agent()
        .request(method, &format!("{}{path}", base_url()))
        .set("x-rewind-id", &acc.id)
        .set("x-rewind-time", &time.to_string())
        .set("x-rewind-body", body_hash)
        .set("x-rewind-sig", &sig))
}

/// Erreur du guichet (code JSON) ou réseau → « ami:<code> ».
fn http_error(e: ureq::Error) -> anyhow::Error {
    match e {
        ureq::Error::Status(status, resp) => {
            let code = resp.into_json::<Value>().ok().and_then(|v| v["error"].as_str().map(String::from)).unwrap_or_else(|| format!("http_{status}"));
            coded(&code)
        }
        ureq::Error::Transport(t) => {
            log::warn!("partage : guichet injoignable ({t})");
            coded("offline")
        }
    }
}

fn call(method: &str, path: &str, body: Option<Value>) -> Result<Value> {
    let bytes = body.map(|b| serde_json::to_vec(&b).unwrap_or_default()).unwrap_or_default();
    let req = signed(method, path, &hex(&Sha256::digest(&bytes)))?;
    let resp = if bytes.is_empty() && (method == "GET" || method == "DELETE") {
        req.call()
    } else {
        req.set("content-type", "application/json").send_bytes(&bytes)
    }
    .map_err(http_error)?;
    resp.into_json::<Value>().map_err(|e| anyhow!("réponse illisible : {e}"))
}

// ---------------------------------------------------------------------------
// État montré à l'interface
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Person {
    pub id: String,
    pub name: String,
    pub since: f64,
    /// Version de sa photo (0 = aucune)
    pub avatar: u64,
    /// Rewind ouvert en ce moment (amis seulement, s'il l'accepte)
    pub online: bool,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Incoming {
    pub id: String,
    pub sender: String,
    pub sender_name: String,
    pub title: String,
    pub game: String,
    pub duration: f64,
    pub size: u64,
    pub sha256: String,
    pub markers: Vec<library::Marker>,
    pub has_thumb: bool,
    pub created: f64,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct SentTo {
    pub id: String,
    pub name: String,
    pub state: String,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Sent {
    pub id: String,
    pub title: String,
    pub game: String,
    pub size: u64,
    pub state: String,
    pub created: f64,
    pub to: Vec<SentTo>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Limits {
    pub max_clip: u64,
    pub daily_bytes: u64,
    pub used_today: u64,
    pub part: u64,
    pub keep_days: u32,
}

impl Default for Limits {
    fn default() -> Self {
        Self { max_clip: 500 << 20, daily_bytes: 2048 << 20, used_today: 0, part: 50 << 20, keep_days: 7 }
    }
}

/// Envoi ou réception en cours (ou terminé avec une erreur).
#[derive(Clone, Debug, Default, Serialize)]
pub struct Transfer {
    pub key: String,
    pub share_id: String,
    /// "up" (envoi) ou "down" (réception)
    pub kind: String,
    pub title: String,
    pub peer: String,
    pub progress: f64,
    /// "running", "error", "done"
    pub state: String,
    pub error: String,
}

/// Clip déjà reçu (historique local, la boîte du guichet est vidée après réception).
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Received {
    pub share_id: String,
    pub clip_id: String,
    pub from_id: String,
    pub from_name: String,
    pub title: String,
    pub game: String,
    pub at: f64,
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct View {
    /// Partage activé dans les réglages
    pub enabled: bool,
    /// Identité créée (code ami obtenu)
    pub ready: bool,
    /// Dernière relève réussie
    pub online: bool,
    pub error: String,
    pub synced_at: f64,
    pub me: Option<Account>,
    pub friends: Vec<Person>,
    pub incoming: Vec<Person>,
    pub outgoing: Vec<Person>,
    pub blocked: Vec<Person>,
    pub inbox: Vec<Incoming>,
    pub sent: Vec<Sent>,
    pub limits: Limits,
    pub transfers: Vec<Transfer>,
    pub received: Vec<Received>,
}

static VIEW: OnceLock<Mutex<View>> = OnceLock::new();
static WAKE: AtomicBool = AtomicBool::new(false);
static CANCELLED: Mutex<Vec<String>> = Mutex::new(Vec::new());

fn view() -> &'static Mutex<View> {
    VIEW.get_or_init(|| {
        Mutex::new(View { received: load_received(), me: load_account(), ready: load_account().is_some() && load_key().is_some(), ..Default::default() })
    })
}

pub fn snapshot() -> View {
    view().lock().clone()
}

fn changed(app: &AppHandle) {
    let _ = app.emit("friends-changed", ());
}

/// Relève tout de suite (après une action de l'utilisateur).
pub fn wake() {
    WAKE.store(true, Ordering::SeqCst);
}

fn received_path() -> PathBuf {
    dir().join("recus.json")
}

fn load_received() -> Vec<Received> {
    std::fs::read_to_string(received_path()).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default()
}

fn save_received(list: &[Received]) {
    let tmp = received_path().with_extension("json.tmp");
    if std::fs::write(&tmp, serde_json::to_string(list).unwrap_or_default()).is_ok() {
        let _ = std::fs::rename(tmp, received_path());
    }
}

fn set_transfer(app: &AppHandle, t: Transfer) {
    {
        let mut v = view().lock();
        v.transfers.retain(|x| x.key != t.key);
        v.transfers.insert(0, t);
        v.transfers.truncate(30);
    }
    changed(app);
}

// ---------------------------------------------------------------------------
// Actions de l'utilisateur
// ---------------------------------------------------------------------------

fn clean_name(name: &str) -> Result<String> {
    let n: String = name.chars().filter(|c| !c.is_control()).collect::<String>().trim().chars().take(24).collect();
    if n.is_empty() {
        return Err(coded("name_invalid"));
    }
    Ok(n)
}

/// Inscrit cette clé au guichet (la même clé redonne toujours le même compte).
fn register_key(key: &SigningKey, name: &str) -> Result<Account> {
    let pubkey = b64(key.verifying_key().as_bytes());
    let time = now_ms();
    let proof = b64(&key.sign(format!("register\n{pubkey}\n{time}").as_bytes()).to_bytes());
    Ok(agent()
        .post(&format!("{}/v1/register", base_url()))
        .set("content-type", "application/json")
        .send_json(json!({ "pubkey": pubkey, "name": name, "time": time, "proof": proof }))
        .map_err(http_error)?
        .into_json::<Account>()?)
}

fn turn_on(app: &AppHandle) {
    {
        let state = app.state::<AppState>();
        let mut s = state.settings.lock();
        s.share_enabled = true;
        let _ = s.save();
    }
    let mut v = view().lock();
    v.enabled = true;
    v.ready = true;
    v.me = load_account();
}

/// Active le partage : crée l'identité (une fois) et obtient le code ami.
pub fn enable(app: &AppHandle, name: &str) -> Result<View> {
    let name = clean_name(name)?;
    let key = load_or_create_key()?;
    let resp = register_key(&key, &name)?;
    save_account(&resp);
    // Même clé déjà inscrite avec un autre pseudo : on garde celui qui vient d'être choisi
    if resp.name != name {
        let _ = call("POST", "/v1/me", Some(json!({ "name": name })));
        save_account(&Account { name: name.clone(), ..resp.clone() });
    }
    turn_on(app);
    log::info!("partage entre amis activé");
    sync_now(app);
    Ok(snapshot())
}

pub fn disable(app: &AppHandle) {
    {
        let state = app.state::<AppState>();
        let mut s = state.settings.lock();
        s.share_enabled = false;
        let _ = s.save();
    }
    let mut v = view().lock();
    v.enabled = false;
    v.online = false;
    drop(v);
    changed(app);
}

pub fn rename(app: &AppHandle, name: &str) -> Result<()> {
    let name = clean_name(name)?;
    call("POST", "/v1/me", Some(json!({ "name": name })))?;
    if let Some(mut a) = load_account() {
        a.name = name;
        save_account(&a);
        view().lock().me = Some(a);
    }
    changed(app);
    Ok(())
}

/// Ajoute un ami par son code ; renvoie "pending" (demande envoyée) ou "friends" (déjà demandé par lui).
pub fn add(app: &AppHandle, code: &str) -> Result<String> {
    let r = call("POST", "/v1/friends/request", Some(json!({ "code": code.trim() })))?;
    sync_now(app);
    Ok(r["state"].as_str().unwrap_or("pending").to_string())
}

pub fn answer(app: &AppHandle, user: &str, accept: bool) -> Result<()> {
    call("POST", "/v1/friends/answer", Some(json!({ "user": user, "accept": accept })))?;
    sync_now(app);
    Ok(())
}

/// Retirer un ami / annuler une demande / débloquer (block = false), ou bloquer (block = true).
pub fn remove(app: &AppHandle, user: &str, block: bool) -> Result<()> {
    call("POST", "/v1/friends/remove", Some(json!({ "user": user, "block": block })))?;
    sync_now(app);
    Ok(())
}

pub fn dismiss(app: &AppHandle, share_id: &str) -> Result<()> {
    call("POST", &format!("/v1/shares/{share_id}/done"), Some(json!({ "state": "dismissed" })))?;
    sync_now(app);
    Ok(())
}

pub fn report(share_id: &str, reason: &str) -> Result<()> {
    call("POST", &format!("/v1/shares/{share_id}/report"), Some(json!({ "reason": reason })))?;
    Ok(())
}

/// Annule un envoi (en cours ou pas encore relevé par tous).
pub fn cancel(app: &AppHandle, key_or_share: &str) -> Result<()> {
    {
        let mut c = CANCELLED.lock();
        c.push(key_or_share.to_string());
        let extra = c.len().saturating_sub(50);
        c.drain(..extra);
    }
    let share_id = {
        let v = view().lock();
        v.transfers.iter().find(|t| t.key == key_or_share).map(|t| t.share_id.clone()).unwrap_or_else(|| key_or_share.to_string())
    };
    if !share_id.is_empty() && !share_id.starts_with("local-") {
        call("DELETE", &format!("/v1/shares/{share_id}"), None)?;
    }
    sync_now(app);
    Ok(())
}

/// « Supprimer mon identité » : tout est effacé chez Cloudflare, puis la clé et le code ici.
pub fn delete_identity(app: &AppHandle) -> Result<()> {
    if load_account().is_some() {
        match call("DELETE", "/v1/me", None) {
            Ok(_) => {}
            Err(e) if e.to_string() == "ami:unknown_user" => {}
            Err(e) => return Err(e),
        }
    }
    let _ = std::fs::remove_file(key_path());
    let _ = std::fs::remove_file(account_path());
    disable(app);
    {
        let mut v = view().lock();
        let received = std::mem::take(&mut v.received);
        *v = View { received, ..Default::default() };
    }
    log::info!("partage : identité supprimée");
    changed(app);
    Ok(())
}

/// Miniature d'un clip en attente (gardée dans le cache).
pub fn thumb(share_id: &str) -> Result<PathBuf> {
    let folder = dir().join("miniatures");
    let _ = std::fs::create_dir_all(&folder);
    let path = folder.join(format!("{share_id}.jpg"));
    if path.is_file() {
        return Ok(path);
    }
    let resp = signed("GET", &format!("/v1/shares/{share_id}/thumb"), &hex(&Sha256::digest([])))?.call().map_err(http_error)?;
    let mut bytes = Vec::new();
    resp.into_reader().take(1 << 20).read_to_end(&mut bytes)?;
    std::fs::write(&path, bytes)?;
    Ok(path)
}

// ---------------------------------------------------------------------------
// Clé de secours : retrouver son code et ses amis après une réinstallation (ou sur un autre PC)
// Format : « RWK- » + base32 (graine de 32 octets + 3 octets d'empreinte pour repérer une faute de frappe),
// par groupes de 4 caractères.
// ---------------------------------------------------------------------------

const B32: &[u8; 32] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ234567";

fn base32(data: &[u8]) -> String {
    let mut out = String::new();
    let (mut buf, mut bits) = (0u32, 0u32);
    for &b in data {
        buf = (buf << 8) | b as u32;
        bits += 8;
        while bits >= 5 {
            bits -= 5;
            out.push(B32[((buf >> bits) & 31) as usize] as char);
        }
    }
    if bits > 0 {
        out.push(B32[((buf << (5 - bits)) & 31) as usize] as char);
    }
    out
}

fn unbase32(text: &str) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    let (mut buf, mut bits) = (0u32, 0u32);
    for c in text.chars() {
        let v = B32.iter().position(|&x| x as char == c)? as u32;
        buf = (buf << 5) | v;
        bits += 5;
        if bits >= 8 {
            bits -= 8;
            out.push((buf >> bits) as u8);
        }
    }
    Some(out)
}

/// Texte de la clé de secours de cette identité.
pub fn recovery_key() -> Result<String> {
    let key = load_key().ok_or_else(|| coded("no_identity"))?;
    let seed = key.to_bytes();
    let mut data = seed.to_vec();
    data.extend_from_slice(&Sha256::digest(seed)[..3]);
    let raw = base32(&data);
    let groups: Vec<String> = raw.as_bytes().chunks(4).map(|c| String::from_utf8_lossy(c).to_string()).collect();
    Ok(format!("RWK-{}", groups.join("-")))
}

/// Lit une clé de secours tapée ou collée (espaces, tirets, minuscules acceptés).
fn parse_recovery(text: &str) -> Result<[u8; 32]> {
    let clean: String = text.trim().to_uppercase().trim_start_matches("RWK").chars().filter(|c| c.is_ascii_alphanumeric()).collect();
    let data = unbase32(&clean).ok_or_else(|| coded("recovery_invalid"))?;
    if data.len() < 35 {
        return Err(coded("recovery_invalid"));
    }
    let seed: [u8; 32] = data[..32].try_into().map_err(|_| coded("recovery_invalid"))?;
    if Sha256::digest(seed)[..3] != data[32..35] {
        return Err(coded("recovery_invalid"));
    }
    Ok(seed)
}

/// Enregistre la clé de secours dans un fichier texte choisi par l'utilisateur.
pub fn save_recovery(path: &Path) -> Result<()> {
    let key = recovery_key()?;
    let acc = load_account().unwrap_or_default();
    let text = format!(
        "Rewind : clé de secours du partage entre amis\r\nCompte : {} ({})\r\n\r\n{key}\r\n\r\nGarde-la en lieu sûr : elle permet de retrouver ton code ami et tes amis après une réinstallation,\r\nmais aussi de se faire passer pour toi. Ne la donne à personne.\r\n",
        acc.name, acc.code
    );
    std::fs::write(path, text)?;
    Ok(())
}

/// Restaure une identité depuis sa clé de secours (remplace celle de ce PC).
pub fn restore(app: &AppHandle, text: &str) -> Result<View> {
    let seed = parse_recovery(text)?;
    let key = SigningKey::from_bytes(&seed);
    let acc = register_key(&key, "Rewind")?;
    std::fs::write(key_path(), dpapi(&seed, true)?)?;
    save_account(&acc);
    turn_on(app);
    log::info!("partage : identité restaurée depuis la clé de secours");
    sync_now(app);
    Ok(snapshot())
}

// ---------------------------------------------------------------------------
// Photo de profil et présence
// ---------------------------------------------------------------------------

/// Photo choisie par l'utilisateur → carré de 256 px en JPEG (recadrée au centre) → envoyée au guichet.
pub fn set_avatar(app: &AppHandle, image: &Path) -> Result<()> {
    let tools = ffmpeg::tools()?;
    let out = dir().join("ma-photo.jpg");
    let status = util::command(&tools.ffmpeg)
        .args(["-hide_banner", "-loglevel", "error", "-y", "-i"])
        .arg(image)
        .args(["-frames:v", "1", "-vf", "crop='min(iw,ih)':'min(iw,ih)',scale=256:256,format=yuvj420p", "-q:v", "5"])
        .arg(&out)
        .stdin(std::process::Stdio::null())
        .output()?;
    if !status.status.success() || !out.is_file() {
        return Err(coded("avatar_unreadable"));
    }
    let bytes = std::fs::read(&out)?;
    signed("PUT", "/v1/me/avatar", &hex(&Sha256::digest(&bytes)))?.set("content-type", "image/jpeg").send_bytes(&bytes).map_err(http_error)?;
    sync_now(app);
    Ok(())
}

pub fn remove_avatar(app: &AppHandle) -> Result<()> {
    call("DELETE", "/v1/me/avatar", None)?;
    sync_now(app);
    Ok(())
}

/// Photo d'un ami (ou la mienne), gardée en cache par version.
pub fn avatar(user_id: &str, version: u64) -> Result<PathBuf> {
    if version == 0 || !user_id.chars().all(|c| c.is_ascii_hexdigit() || c == '-') {
        return Err(coded("not_found"));
    }
    let folder = dir().join("photos");
    let _ = std::fs::create_dir_all(&folder);
    let path = folder.join(format!("{user_id}-{version}.jpg"));
    if path.is_file() {
        return Ok(path);
    }
    let resp = signed("GET", &format!("/v1/users/{user_id}/avatar"), &hex(&Sha256::digest([])))?.call().map_err(http_error)?;
    let mut bytes = Vec::new();
    resp.into_reader().take(512 * 1024).read_to_end(&mut bytes)?;
    // Anciennes versions de cette photo : supprimées
    if let Ok(entries) = std::fs::read_dir(&folder) {
        for e in entries.flatten() {
            if e.file_name().to_string_lossy().starts_with(&format!("{user_id}-")) {
                let _ = std::fs::remove_file(e.path());
            }
        }
    }
    std::fs::write(&path, bytes)?;
    Ok(path)
}

/// « Montrer quand je suis en ligne » (vu par mes amis seulement).
pub fn set_show_online(app: &AppHandle, show: bool) -> Result<()> {
    call("POST", "/v1/me", Some(json!({ "show_online": show })))?;
    sync_now(app);
    Ok(())
}

// ---------------------------------------------------------------------------
// Relève
// ---------------------------------------------------------------------------

#[derive(Deserialize, Default)]
#[serde(default)]
struct SyncResp {
    me: Account,
    friends: Vec<Person>,
    incoming: Vec<Person>,
    outgoing: Vec<Person>,
    blocked: Vec<Person>,
    inbox: Vec<Incoming>,
    sent: Vec<Sent>,
    limits: Limits,
}

fn sync() -> Result<SyncResp> {
    let v = call("GET", "/v1/sync", None)?;
    Ok(serde_json::from_value(v)?)
}

/// Relève tout de suite, dans ce fil (actions de l'utilisateur) ; les erreurs restent dans l'état.
fn sync_now(app: &AppHandle) {
    match sync() {
        Ok(r) => apply(app, r),
        Err(e) => {
            let mut v = view().lock();
            v.online = false;
            v.error = e.to_string();
            drop(v);
            changed(app);
        }
    }
}

fn apply(app: &AppHandle, r: SyncResp) {
    let new_items: Vec<Incoming>;
    {
        let mut v = view().lock();
        let known: HashSet<String> = v.inbox.iter().map(|i| i.id.clone()).collect();
        new_items = r.inbox.iter().filter(|i| !known.contains(&i.id)).cloned().collect();
        if !r.me.id.is_empty() {
            let me = r.me.clone();
            if load_account().as_ref() != Some(&me) {
                save_account(&me);
            }
            v.me = Some(me);
        }
        v.friends = r.friends;
        v.incoming = r.incoming;
        v.outgoing = r.outgoing;
        v.blocked = r.blocked;
        v.inbox = r.inbox;
        v.sent = r.sent;
        v.limits = r.limits;
        v.online = true;
        v.error.clear();
        v.synced_at = util::now();
    }
    for item in &new_items {
        let _ = app.emit("friends-new-clip", json!({ "from": item.sender_name, "title": item.title }));
    }
    changed(app);
}

/// Fil de relève : toutes les 2 minutes quand le partage est activé, et tout de suite après une action.
pub fn start(app: AppHandle) {
    let _ = std::thread::Builder::new().name("amis".into()).spawn(move || {
        let mut last = Instant::now() - POLL;
        loop {
            let (enabled, auto) = {
                let state = app.state::<AppState>();
                let s = state.settings.lock();
                (s.share_enabled, s.share_auto_download)
            };
            let ready = load_account().is_some() && load_key().is_some();
            {
                let mut v = view().lock();
                v.enabled = enabled;
                v.ready = ready;
            }
            let wake = WAKE.swap(false, Ordering::SeqCst);
            if enabled && ready && (wake || last.elapsed() >= POLL) {
                last = Instant::now();
                sync_now(&app);
                if auto {
                    start_next_download(&app);
                }
            }
            std::thread::sleep(Duration::from_millis(500));
        }
    });
}

// ---------------------------------------------------------------------------
// Envoi
// ---------------------------------------------------------------------------

fn file_sha256(path: &Path, mut progress: impl FnMut(u64)) -> Result<String> {
    let mut f = std::fs::File::open(path)?;
    let mut h = Sha256::new();
    let mut buf = vec![0u8; 1 << 20];
    let mut done = 0u64;
    loop {
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
        done += n as u64;
        progress(done);
    }
    Ok(hex(&h.finalize()))
}

/// Lecteur qui compte ce qui part (barre de progression pendant l'envoi d'un morceau).
struct Counting<'a> {
    data: &'a [u8],
    pos: usize,
    sent: Arc<AtomicU64>,
}

impl Read for Counting<'_> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let n = buf.len().min(self.data.len() - self.pos).min(256 * 1024);
        buf[..n].copy_from_slice(&self.data[self.pos..self.pos + n]);
        self.pos += n;
        self.sent.fetch_add(n as u64, Ordering::Relaxed);
        Ok(n)
    }
}

fn is_cancelled(key: &str, share_id: &str) -> bool {
    let c = CANCELLED.lock();
    c.iter().any(|x| x == key || (!share_id.is_empty() && x == share_id))
}

/// Envoie un clip de la bibliothèque à des amis (en arrière-plan).
pub fn send(app: &AppHandle, clip_id: &str, to: Vec<String>) -> Result<()> {
    let clip = app.state::<AppState>().library.lock().get(clip_id).cloned().ok_or_else(|| coded("clip_unknown"))?;
    if clip.kind == "screenshot" {
        return Err(coded("not_a_video"));
    }
    if to.is_empty() {
        return Err(coded("recipients_invalid"));
    }
    let path = PathBuf::from(&clip.path);
    let size = std::fs::metadata(&path).map(|m| m.len()).map_err(|_| coded("file_missing"))?;
    let limits = view().lock().limits.clone();
    if size > limits.max_clip {
        return Err(coded("clip_too_big"));
    }
    let names: Vec<String> = {
        let v = view().lock();
        to.iter().map(|id| v.friends.iter().find(|f| &f.id == id).map(|f| f.name.clone()).unwrap_or_default()).collect()
    };
    let key = format!("local-{}-{}", clip.id, now_ms());
    let mut t = Transfer { key: key.clone(), kind: "up".into(), title: clip.title.clone(), peer: names.join(", "), state: "running".into(), ..Default::default() };
    set_transfer(app, t.clone());
    let app = app.clone();
    std::thread::Builder::new().name("amis-envoi".into()).spawn(move || {
        let result = (|| -> Result<String> {
            // 1) Empreinte (10 % de la barre)
            let mut last = Instant::now();
            let sha = file_sha256(&path, |done| {
                if last.elapsed() > Duration::from_millis(250) {
                    last = Instant::now();
                    t.progress = 0.1 * done as f64 / size.max(1) as f64;
                    set_transfer(&app, t.clone());
                }
            })?;
            // 2) Déclaration au guichet (amitié, quotas)
            let r = call(
                "POST",
                "/v1/shares",
                Some(json!({
                    "title": clip.title, "game": clip.game, "duration": clip.duration, "size": size, "sha256": sha,
                    "to": to, "markers": clip.markers,
                })),
            )?;
            let share_id = r["id"].as_str().ok_or_else(|| anyhow!("réponse sans identifiant"))?.to_string();
            let part = r["part_size"].as_u64().unwrap_or(50 << 20) as usize;
            let parts = r["parts"].as_u64().unwrap_or(1) as usize;
            t.share_id = share_id.clone();
            set_transfer(&app, t.clone());
            // 3) Morceaux (3 essais chacun)
            let mut file = std::fs::File::open(&path)?;
            let mut sent_before = 0u64;
            for n in 1..=parts {
                if is_cancelled(&t.key, &share_id) {
                    bail!("ami:cancelled");
                }
                let len = if n < parts { part } else { size as usize - part * (parts - 1) };
                let mut chunk = vec![0u8; len];
                file.read_exact(&mut chunk)?;
                let mut attempt = 0;
                loop {
                    attempt += 1;
                    let counter = Arc::new(AtomicU64::new(0));
                    let done = Arc::new(AtomicBool::new(false));
                    // Progression pendant l'envoi du morceau
                    let (a2, c2, d2, mut t2, base) = (app.clone(), Arc::clone(&counter), Arc::clone(&done), t.clone(), sent_before);
                    let watcher = std::thread::spawn(move || {
                        while !d2.load(Ordering::Relaxed) {
                            std::thread::sleep(Duration::from_millis(300));
                            t2.progress = 0.1 + 0.85 * (base + c2.load(Ordering::Relaxed)) as f64 / size.max(1) as f64;
                            set_transfer(&a2, t2.clone());
                        }
                    });
                    let path_url = format!("/v1/shares/{share_id}/parts/{n}");
                    let req = signed("PUT", &path_url, &hex(&Sha256::digest(&chunk)))?
                        .set("content-type", "application/octet-stream")
                        .set("content-length", &len.to_string());
                    let res = req.send(Counting { data: &chunk, pos: 0, sent: Arc::clone(&counter) });
                    done.store(true, Ordering::Relaxed);
                    let _ = watcher.join();
                    match res {
                        Ok(_) => break,
                        Err(e) if attempt < 3 && matches!(e, ureq::Error::Transport(_)) => {
                            log::warn!("partage : morceau {n} à renvoyer ({e})");
                            std::thread::sleep(Duration::from_secs(2 * attempt as u64));
                        }
                        Err(e) => return Err(http_error(e)),
                    }
                }
                sent_before += len as u64;
            }
            // 4) Miniature (facultative)
            if let Ok(bytes) = std::fs::read(&clip.thumb)
                && !bytes.is_empty()
                && bytes.len() <= 512 * 1024
            {
                let _ = signed("PUT", &format!("/v1/shares/{share_id}/thumb"), &hex(&Sha256::digest(&bytes)))
                    .map(|r| r.set("content-type", "image/jpeg").send_bytes(&bytes));
            }
            // 5) Fin : le clip apparaît chez les amis
            call("POST", &format!("/v1/shares/{share_id}/complete"), None)?;
            Ok(share_id)
        })();
        match result {
            Ok(id) => {
                log::info!("partage : clip « {} » envoyé ({id})", t.title);
                t.progress = 1.0;
                t.state = "done".into();
            }
            Err(e) => {
                let msg = e.to_string();
                if msg != "ami:cancelled" {
                    log::warn!("partage : envoi raté ({msg:#})");
                }
                t.state = if msg == "ami:cancelled" { "done".into() } else { "error".into() };
                t.error = msg;
                if !t.share_id.is_empty() {
                    let _ = call("DELETE", &format!("/v1/shares/{}", t.share_id), None);
                }
            }
        }
        set_transfer(&app, t);
        wake();
    })?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Réception
// ---------------------------------------------------------------------------

static DOWNLOADING: AtomicBool = AtomicBool::new(false);

/// Lance la réception du prochain clip en attente (un à la fois).
fn start_next_download(app: &AppHandle) {
    if DOWNLOADING.load(Ordering::SeqCst) {
        return;
    }
    let next = {
        let v = view().lock();
        let done: HashSet<&str> = v.received.iter().map(|r| r.share_id.as_str()).collect();
        // Une erreur de réception n'est pas réessayée en boucle : seulement au clic
        let failed: HashSet<&str> = v.transfers.iter().filter(|t| t.kind == "down" && t.state == "error").map(|t| t.share_id.as_str()).collect();
        v.inbox.iter().find(|i| !done.contains(i.id.as_str()) && !failed.contains(i.id.as_str())).cloned()
    };
    if let Some(item) = next {
        download(app, item);
    }
}

/// Réception au clic (aussi après une erreur).
pub fn download_now(app: &AppHandle, share_id: &str) -> Result<()> {
    let item = view().lock().inbox.iter().find(|i| i.id == share_id).cloned().ok_or_else(|| coded("share_unknown"))?;
    if DOWNLOADING.load(Ordering::SeqCst) {
        return Err(coded("busy"));
    }
    download(app, item);
    Ok(())
}

fn download(app: &AppHandle, item: Incoming) {
    if DOWNLOADING.swap(true, Ordering::SeqCst) {
        return;
    }
    let app = app.clone();
    let spawned = std::thread::Builder::new().name("amis-reception".into()).spawn(move || {
        let mut t = Transfer {
            key: format!("down-{}", item.id),
            share_id: item.id.clone(),
            kind: "down".into(),
            title: item.title.clone(),
            peer: item.sender_name.clone(),
            state: "running".into(),
            ..Default::default()
        };
        set_transfer(&app, t.clone());
        let result = receive(&app, &item, &mut t);
        match result {
            Ok(()) => {
                t.state = "done".into();
                t.progress = 1.0;
                log::info!("partage : clip « {} » reçu de {}", item.title, item.sender_name);
            }
            Err(e) => {
                log::warn!("partage : réception ratée ({e:#})");
                t.state = "error".into();
                t.error = e.to_string();
            }
        }
        set_transfer(&app, t);
        DOWNLOADING.store(false, Ordering::SeqCst);
        wake();
    });
    if spawned.is_err() {
        DOWNLOADING.store(false, Ordering::SeqCst);
    }
}

fn receive(app: &AppHandle, item: &Incoming, t: &mut Transfer) -> Result<()> {
    let clip_folder = PathBuf::from(&app.state::<AppState>().settings.lock().clip_folder);
    let folder = clip_folder.join("Reçus").join(util::safe_name(&item.sender_name));
    std::fs::create_dir_all(&folder)?;
    // Fichier temporaire « .partial. » : la bibliothèque l'ignore ; il permet de reprendre
    let partial = folder.join(format!("{}.partial.mp4", item.id));
    let have = std::fs::metadata(&partial).map(|m| m.len()).unwrap_or(0);
    let have = if have > item.size { 0 } else { have };
    if have < item.size {
        let path = format!("/v1/shares/{}/file", item.id);
        let mut req = signed("GET", &path, &hex(&Sha256::digest([])))?;
        if have > 0 {
            req = req.set("range", &format!("bytes={have}-"));
        }
        let resp = req.call().map_err(http_error)?;
        let resume = have > 0 && resp.status() == 206;
        let mut out = std::fs::OpenOptions::new().create(true).write(true).append(resume).truncate(!resume).open(&partial)?;
        let mut done = if resume { have } else { 0 };
        let mut reader = resp.into_reader();
        let mut buf = vec![0u8; 1 << 20];
        let mut last = Instant::now();
        loop {
            let n = reader.read(&mut buf)?;
            if n == 0 {
                break;
            }
            out.write_all(&buf[..n])?;
            done += n as u64;
            if done > item.size {
                bail!("ami:size_mismatch");
            }
            if last.elapsed() > Duration::from_millis(300) {
                last = Instant::now();
                t.progress = 0.95 * done as f64 / item.size.max(1) as f64;
                set_transfer(app, t.clone());
            }
        }
        out.flush()?;
    }
    // Vérifications : taille, empreinte, vraie vidéo
    let size = std::fs::metadata(&partial)?.len();
    if size != item.size {
        bail!("ami:size_mismatch");
    }
    if !file_sha256(&partial, |_| {})?.eq_ignore_ascii_case(&item.sha256) {
        let _ = std::fs::remove_file(&partial);
        bail!("ami:hash_mismatch");
    }
    let duration = ffmpeg::duration(&partial).filter(|d| *d > 0.0).ok_or_else(|| {
        let _ = std::fs::remove_file(&partial);
        coded("not_a_video")
    })?;
    // Rangé dans la bibliothèque : Vidéos\Rewind\Reçus\<ami>\<titre>.mp4
    let date = chrono::DateTime::from_timestamp_millis(item.created as i64).map(|d| d.with_timezone(&chrono::Local).format("%Y-%m-%d %Hh%M").to_string()).unwrap_or_default();
    let target = util::reserve_file(&folder, &util::safe_name(&format!("{} {date}", item.title)), "mp4")?;
    let _ = std::fs::remove_file(&target);
    std::fs::rename(&partial, &target).context("rangement du clip reçu")?;
    let id = library::new_id();
    let thumb = library::thumbs_dir().join(format!("{id}.jpg"));
    let _ = ffmpeg::thumbnail(&target, (duration * 0.4).min(3.0), &thumb);
    let clip = library::Clip {
        id: id.clone(),
        path: target.to_string_lossy().to_string(),
        thumb: if thumb.is_file() { thumb.to_string_lossy().to_string() } else { String::new() },
        title: item.title.clone(),
        game: item.game.clone(),
        created: util::now(),
        duration,
        size,
        kind: "received".into(),
        markers: item.markers.clone(),
        favorite: false,
        tags: Vec::new(),
    };
    app.state::<AppState>().library.lock().add(clip)?;
    let _ = app.emit("library-changed", ());
    {
        let mut v = view().lock();
        v.received.insert(
            0,
            Received {
                share_id: item.id.clone(),
                clip_id: id,
                from_id: item.sender.clone(),
                from_name: item.sender_name.clone(),
                title: item.title.clone(),
                game: item.game.clone(),
                at: util::now(),
            },
        );
        v.received.truncate(500);
        save_received(&v.received);
    }
    // Prévenir le guichet : le fichier est supprimé dès que tous l'ont reçu
    if let Err(e) = call("POST", &format!("/v1/shares/{}/done", item.id), Some(json!({ "state": "received" }))) {
        log::warn!("partage : confirmation de réception impossible ({e})");
    }
    let _ = std::fs::remove_file(dir().join("miniatures").join(format!("{}.jpg", item.id)));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cle_protegee_par_windows() {
        let secret = [7u8; 32];
        let sealed = dpapi(&secret, true).unwrap();
        assert_ne!(sealed, secret.to_vec());
        assert_eq!(dpapi(&sealed, false).unwrap(), secret.to_vec());
    }

    #[test]
    fn signature_lisible_par_le_guichet() {
        // Même message que le guichet : MÉTHODE \n chemin \n heure \n empreinte
        let key = SigningKey::from_bytes(&[3u8; 32]);
        let msg = format!("GET\n/v1/sync\n1\n{}", hex(&Sha256::digest([])));
        let sig = key.sign(msg.as_bytes());
        assert!(key.verifying_key().verify_strict(msg.as_bytes(), &sig).is_ok());
        assert_eq!(hex(&Sha256::digest([])), "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855");
    }

    #[test]
    fn cle_de_secours_aller_retour() {
        let seed = [42u8; 32];
        let mut data = seed.to_vec();
        data.extend_from_slice(&Sha256::digest(seed)[..3]);
        let text = format!("RWK-{}", base32(&data));
        assert_eq!(parse_recovery(&text).unwrap(), seed);
        // Tapée en minuscules, avec des espaces : acceptée
        assert_eq!(parse_recovery(&text.to_lowercase().replace('-', " ")).unwrap(), seed);
        // Une faute de frappe : refusée (empreinte)
        let mut bad: Vec<char> = text.chars().collect();
        let i = bad.len() - 6;
        bad[i] = if bad[i] == 'A' { 'B' } else { 'A' };
        assert!(parse_recovery(&bad.into_iter().collect::<String>()).is_err());
        assert!(parse_recovery("RWK-ABCD").is_err());
    }

    #[test]
    fn pseudo_nettoye() {
        assert_eq!(clean_name("  Lucas\n ").unwrap(), "Lucas");
        assert!(clean_name("   ").is_err());
        assert_eq!(clean_name(&"x".repeat(40)).unwrap().len(), 24);
    }
}
