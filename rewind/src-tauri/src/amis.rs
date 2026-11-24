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
/// … et plus souvent quand la page Amis est affichée (une relève ≈ 1 Ko)
const POLL_FOCUS: Duration = Duration::from_secs(30);

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

/// Ancienne clé aléatoire (avant la 0.4.0), ou clé de secours d'un PC dont l'identifiant est illisible.
fn key_path() -> PathBuf {
    dir().join("cle.bin")
}

/// Clé de secours (aléatoire, chiffrée par Windows) : seule sa partie publique est au guichet.
fn recovery_path() -> PathBuf {
    dir().join("secours.bin")
}

fn read_sealed(path: &Path) -> Option<SigningKey> {
    let raw = std::fs::read(path).ok()?;
    let seed: [u8; 32] = dpapi(&raw, false).ok()?.try_into().ok()?;
    Some(SigningKey::from_bytes(&seed))
}

/// Compte ou clé modifiés : le fil de relève refait sa vérification tout de suite.
static ACCOUNT_DIRTY: AtomicBool = AtomicBool::new(true);

fn write_sealed(path: &Path, key: &SigningKey) -> Result<()> {
    std::fs::write(path, dpapi(&key.to_bytes(), true)?)?;
    ACCOUNT_DIRTY.store(true, Ordering::SeqCst);
    Ok(())
}

fn random_key() -> Result<SigningKey> {
    let mut seed = [0u8; 32];
    getrandom::getrandom(&mut seed).map_err(|e| anyhow!("hasard indisponible : {e}"))?;
    Ok(SigningKey::from_bytes(&seed))
}

fn pubkey_b64(key: &SigningKey) -> String {
    b64(key.verifying_key().as_bytes())
}

/// Identifiant matériel de ce PC : identifiant de Windows (MachineGuid) + identifiant de la carte mère
/// (SMBIOS, s'il est valable). Lu comme le font beaucoup de logiciels (Steam, navigateurs…), sans WMI ni
/// programme externe. Ne quitte jamais le PC : il sert seulement à fabriquer la clé de ce Rewind.
fn hardware_id() -> Option<String> {
    let guid = util::reg_values(r"HKLM\SOFTWARE\Microsoft\Cryptography").get("MachineGuid").map(|g| g.trim().to_ascii_lowercase()).filter(|g| g.len() >= 32)?;
    let board = smbios_uuid().map(|u| hex(&u)).unwrap_or_default();
    Some(format!("{guid}|{board}"))
}

/// UUID du système lu dans la table SMBIOS du micrologiciel (structure de type 1).
fn smbios_uuid() -> Option<[u8; 16]> {
    use windows_sys::Win32::System::SystemInformation::GetSystemFirmwareTable;
    const RSMB: u32 = u32::from_be_bytes(*b"RSMB");
    let size = unsafe { GetSystemFirmwareTable(RSMB, 0, std::ptr::null_mut(), 0) };
    if !(8..=1 << 20).contains(&size) {
        return None;
    }
    let mut buf = vec![0u8; size as usize];
    if unsafe { GetSystemFirmwareTable(RSMB, 0, buf.as_mut_ptr() as _, size) } != size {
        return None;
    }
    // En-tête RawSMBIOSData (8 octets), puis les structures : type, longueur, poignée, champs, chaînes (fin : 0 0)
    let len = u32::from_le_bytes(buf[4..8].try_into().ok()?) as usize;
    let table = buf.get(8..8 + len.min(buf.len() - 8))?;
    let mut i = 0;
    while i + 4 <= table.len() {
        let (kind, flen) = (table[i], table[i + 1] as usize);
        if flen < 4 || i + flen > table.len() {
            return None;
        }
        if kind == 1 && flen >= 0x18 {
            let uuid: [u8; 16] = table[i + 8..i + 24].try_into().ok()?;
            // Valeurs bidon fréquentes : tout à 0 ou tout à FF, ou la valeur d'exemple des fabricants
            let fake = uuid.iter().all(|&b| b == 0) || uuid.iter().all(|&b| b == 0xff) || hex(&uuid) == "03000200040005000006000700080009";
            return (!fake).then_some(uuid);
        }
        if kind == 127 {
            return None;
        }
        // Chaînes : jusqu'au double zéro
        let mut j = i + flen;
        while j + 1 < table.len() && !(table[j] == 0 && table[j + 1] == 0) {
            j += 1;
        }
        i = j + 2;
    }
    None
}

/// Clé de ce PC : toujours la même sur ce PC (réinstaller Rewind ou effacer ses données redonne le même
/// compte), différente sur un autre PC. Dérivée de l'identifiant matériel, jamais enregistrée.
fn device_key() -> Option<SigningKey> {
    static KEY: OnceLock<Option<[u8; 32]>> = OnceLock::new();
    let seed = KEY.get_or_init(|| {
        #[allow(unused_mut)]
        let mut id = hardware_id()?;
        // Essais (versions de développement) : un dossier de données = un « PC » différent
        #[cfg(debug_assertions)]
        if let Ok(d) = std::env::var("REWIND_DATA_DIR") {
            id.push('|');
            id.push_str(&d.to_lowercase());
        }
        let mut h = Sha256::new();
        h.update(b"Rewind : identite du PC, v1\n");
        h.update(id.as_bytes());
        Some(h.finalize().into())
    });
    seed.map(|s| SigningKey::from_bytes(&s))
}

/// Clé qui signe les demandes : l'ancienne clé tant qu'elle n'est pas remplacée, sinon celle du PC.
fn load_key() -> Option<SigningKey> {
    read_sealed(&key_path()).or_else(device_key)
}

fn load_or_create_key() -> Result<SigningKey> {
    if let Some(k) = load_key() {
        return Ok(k);
    }
    // Identifiant du PC illisible (très rare) : clé aléatoire gardée sur le disque, comme avant
    let k = random_key()?;
    write_sealed(&key_path(), &k)?;
    Ok(k)
}

fn load_recovery() -> Option<SigningKey> {
    read_sealed(&recovery_path())
}

/// La clé de secours gardée sur ce PC est-elle celle du compte ?
fn recovery_here(acc: Option<&Account>) -> bool {
    match (acc, load_recovery()) {
        (Some(a), Some(k)) => !a.recovery.is_empty() && a.recovery == pubkey_b64(&k),
        _ => false,
    }
}

/// Passage de l'ancienne clé aléatoire à la clé du PC (une fois, au premier lancement de la 0.4.0).
/// L'ancienne clé devient la clé de secours : une clé « RWK- » déjà notée reste valable.
fn migrate_to_device_key() {
    // Refus définitif du guichet (ce PC a déjà un autre compte) : on garde l'ancienne clé, sans réessayer
    static GAVE_UP: AtomicBool = AtomicBool::new(false);
    if GAVE_UP.load(Ordering::SeqCst) || !key_path().exists() {
        return;
    }
    let (Some(old), Some(device), Some(acc)) = (read_sealed(&key_path()), device_key(), load_account()) else { return };
    if old.verifying_key() == device.verifying_key() {
        let _ = std::fs::remove_file(key_path());
        return;
    }
    let time = now_ms();
    let proof = b64(&device.sign(format!("rekey\n{}\n{time}", acc.id).as_bytes()).to_bytes());
    match call("POST", "/v1/me/key", Some(json!({ "pubkey": pubkey_b64(&device), "time": time, "proof": proof }))) {
        Ok(r) => {
            let recovery = r["recovery"].as_str().unwrap_or_default().to_string();
            if recovery == pubkey_b64(&old) && write_sealed(&recovery_path(), &old).is_err() {
                return; // on garde l'ancienne clé tant que la clé de secours n'est pas écrite
            }
            let _ = std::fs::remove_file(key_path());
            save_account(&Account { recovery, ..acc });
            log::info!("partage : identité rattachée à ce PC");
        }
        Err(e) => {
            log::warn!("partage : rattachement à ce PC impossible pour l'instant ({e})");
            // Guichet injoignable : nouvel essai à la prochaine relève
            if e.to_string() != "ami:offline" {
                GAVE_UP.store(true, Ordering::SeqCst);
            }
        }
    }
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
    /// Partie publique de la clé de secours (vide = pas encore créée)
    pub recovery: String,
}

fn account_path() -> PathBuf {
    dir().join("compte.json")
}

fn load_account() -> Option<Account> {
    serde_json::from_str::<Account>(&std::fs::read_to_string(account_path()).ok()?).ok().filter(|a| !a.id.is_empty())
}

fn save_account(a: &Account) {
    let _ = std::fs::write(account_path(), serde_json::to_string_pretty(a).unwrap_or_default());
    ACCOUNT_DIRTY.store(true, Ordering::SeqCst);
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
    /// Étape d'un envoi : "compress" (allègement de la vidéo) ou "" (envoi)
    pub stage: String,
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
    /// La clé de secours du compte est gardée sur ce PC (sinon : en créer une nouvelle)
    pub recovery_here: bool,
    /// Relève en cours
    pub syncing: bool,
}

static VIEW: OnceLock<Mutex<View>> = OnceLock::new();
static WAKE: AtomicBool = AtomicBool::new(false);
static CANCELLED: Mutex<Vec<String>> = Mutex::new(Vec::new());

fn view() -> &'static Mutex<View> {
    VIEW.get_or_init(|| {
        let me = load_account();
        Mutex::new(View { received: load_received(), recovery_here: recovery_here(me.as_ref()), ready: me.is_some() && load_key().is_some(), me, ..Default::default() })
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

static FOCUS: AtomicBool = AtomicBool::new(false);

/// La page Amis est affichée (relève toutes les 30 s tant que la fenêtre est visible).
pub fn set_focus(on: bool) {
    FOCUS.store(on, Ordering::SeqCst);
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

/// Inscrit cette clé au guichet (la même clé, donc le même PC, redonne toujours le même compte).
/// La clé de secours proposée n'est gardée que si le compte n'en a pas encore.
fn register_key(key: &SigningKey, name: &str, recovery: Option<&SigningKey>) -> Result<Account> {
    let pubkey = pubkey_b64(key);
    let time = now_ms();
    let proof = b64(&key.sign(format!("register\n{pubkey}\n{time}").as_bytes()).to_bytes());
    Ok(agent()
        .post(&format!("{}/v1/register", base_url()))
        .set("content-type", "application/json")
        .send_json(json!({ "pubkey": pubkey, "name": name, "time": time, "proof": proof, "recovery": recovery.map(pubkey_b64) }))
        .map_err(http_error)?
        .into_json::<Account>()?)
}

/// Clé de secours locale : gardée si c'est celle du compte, effacée si le compte en a une autre.
fn keep_recovery(acc: &Account, proposed: &SigningKey) {
    if acc.recovery == pubkey_b64(proposed) {
        if load_recovery().map(|k| k.verifying_key()) != Some(proposed.verifying_key()) {
            let _ = write_sealed(&recovery_path(), proposed);
        }
    } else if load_recovery().is_some_and(|k| pubkey_b64(&k) != acc.recovery) {
        let _ = std::fs::remove_file(recovery_path());
    }
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
    v.recovery_here = recovery_here(v.me.as_ref());
}

/// Active le partage : crée l'identité (une fois) et obtient le code ami.
pub fn enable(app: &AppHandle, name: &str) -> Result<View> {
    let name = clean_name(name)?;
    let key = load_or_create_key()?;
    let proposed = match load_recovery() {
        Some(k) => k,
        None => random_key()?,
    };
    // Ancienne clé encore là (compte d'avant la 0.4.0) : elle deviendra la clé de secours au passage à la
    // clé du PC, on n'en propose donc pas d'autre
    let legacy = key_path().exists() && device_key().is_some();
    let resp = register_key(&key, &name, (!legacy).then_some(&proposed))?;
    if !legacy {
        keep_recovery(&resp, &proposed);
    }
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
    let _ = std::fs::remove_file(recovery_path());
    let _ = std::fs::remove_file(account_path());
    ACCOUNT_DIRTY.store(true, Ordering::SeqCst);
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

fn recovery_text(key: &SigningKey) -> String {
    let seed = key.to_bytes();
    let mut data = seed.to_vec();
    data.extend_from_slice(&Sha256::digest(seed)[..3]);
    let raw = base32(&data);
    let groups: Vec<String> = raw.as_bytes().chunks(4).map(|c| String::from_utf8_lossy(c).to_string()).collect();
    format!("RWK-{}", groups.join("-"))
}

/// Texte de la clé de secours du compte (gardée sur ce PC).
pub fn recovery_key() -> Result<String> {
    let acc = load_account().ok_or_else(|| coded("no_identity"))?;
    let key = load_recovery().filter(|k| pubkey_b64(k) == acc.recovery).ok_or_else(|| coded("recovery_missing"))?;
    Ok(recovery_text(&key))
}

/// Nouvelle clé de secours (l'ancienne ne marche plus) ; renvoie son texte.
pub fn new_recovery(app: &AppHandle) -> Result<String> {
    let acc = load_account().ok_or_else(|| coded("no_identity"))?;
    let key = random_key()?;
    call("POST", "/v1/me/recovery", Some(json!({ "recovery": pubkey_b64(&key) })))?;
    write_sealed(&recovery_path(), &key)?;
    let acc = Account { recovery: pubkey_b64(&key), ..acc };
    save_account(&acc);
    {
        let mut v = view().lock();
        v.me = Some(acc);
        v.recovery_here = true;
    }
    changed(app);
    log::info!("partage : nouvelle clé de secours");
    Ok(recovery_text(&key))
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

/// Restaure une identité depuis sa clé de secours : le compte est rattaché à ce PC
/// (s'il y avait déjà un compte sur ce PC, il est remplacé).
pub fn restore(app: &AppHandle, text: &str) -> Result<View> {
    let recovery = SigningKey::from_bytes(&parse_recovery(text)?);
    let (device, from_hardware) = match device_key() {
        Some(k) => (k, true),
        None => (load_or_create_key()?, false),
    };
    if recovery.verifying_key() == device.verifying_key() {
        return Err(coded("recovery_invalid"));
    }
    let (rpub, dpub, time) = (pubkey_b64(&recovery), pubkey_b64(&device), now_ms());
    let acc = agent()
        .post(&format!("{}/v1/recover", base_url()))
        .set("content-type", "application/json")
        .send_json(json!({
            "recovery": rpub, "pubkey": dpub, "time": time,
            "proof_recovery": b64(&recovery.sign(format!("recover\n{dpub}\n{time}").as_bytes()).to_bytes()),
            "proof_device": b64(&device.sign(format!("device\n{rpub}\n{time}").as_bytes()).to_bytes()),
        }))
        .map_err(http_error)?
        .into_json::<Account>()?;
    write_sealed(&recovery_path(), &recovery)?;
    if from_hardware {
        let _ = std::fs::remove_file(key_path());
    }
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
    migrate_to_device_key();
    view().lock().syncing = true;
    changed(app);
    let result = sync();
    view().lock().syncing = false;
    match result {
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
        v.recovery_here = recovery_here(v.me.as_ref());
    }
    for item in &new_items {
        let _ = app.emit("friends-new-clip", json!({ "from": item.sender_name, "title": item.title }));
    }
    changed(app);
}

/// Fil de relève : toutes les 2 minutes quand le partage est activé (30 s quand la page Amis est
/// affichée), et tout de suite après une action.
pub fn start(app: AppHandle) {
    let _ = std::thread::Builder::new().name("amis".into()).spawn(move || {
        // Clips allégés d'un envoi interrompu (Rewind fermé pendant l'envoi)
        let _ = std::fs::remove_dir_all(dir().join("envoi"));
        let mut last = crate::util::ago(POLL);
        // 2026-11-24 : le compte et la clé (lecture de 2 fichiers + déchiffrement) étaient relus
        // 2 fois par seconde, même partage coupé. Maintenant : au réveil, ou toutes les 10 s.
        let mut ready = false;
        let mut checked: Option<Instant> = None;
        let mut pending_download = false;
        loop {
            let (enabled, auto) = {
                let state = app.state::<AppState>();
                let s = state.settings.lock();
                (s.share_enabled, s.share_auto_download)
            };
            let wake = WAKE.swap(false, Ordering::SeqCst);
            let dirty = ACCOUNT_DIRTY.swap(false, Ordering::SeqCst);
            if wake || dirty || checked.is_none_or(|t| t.elapsed() >= Duration::from_secs(10)) {
                ready = load_account().is_some() && load_key().is_some();
                checked = Some(Instant::now());
            }
            {
                let mut v = view().lock();
                v.enabled = enabled;
                v.ready = ready;
            }
            if enabled && ready {
                let watching = FOCUS.load(Ordering::SeqCst)
                    && app.get_webview_window("main").is_some_and(|w| w.is_visible().unwrap_or(false) && !w.is_minimized().unwrap_or(false));
                let every = if watching { POLL_FOCUS } else { POLL };
                if wake || last.elapsed() >= every {
                    last = Instant::now();
                    sync_now(&app);
                    pending_download = auto;
                }
                // Réception automatique : jamais pendant qu'un jeu est au premier plan (téléchargement,
                // empreinte, miniature) ; elle reprend dès que le jeu n'est plus devant.
                if pending_download && !crate::util::game_active() {
                    pending_download = false;
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

/// Débit vidéo visé pour l'envoi : 65 % du débit moyen d'origine. NVENC en VBR multipasse reste souvent en
/// dessous sur les clips de jeu : mesuré sur un vrai clip CS2 1080p50 à 5,6 Mb/s, fichier 35 à 45 % plus
/// léger selon le passage, pour une image quasi identique (SSIM 0,994). Jamais sous un plancher qui dépend de la définition
/// (2,5 Mb/s en 1080p). None = ne pas réencoder (déjà léger, ou codec déjà efficace comme HEVC/AV1).
fn share_bitrate(codec: &str, width: u32, height: u32, video_bps: f64) -> Option<u64> {
    if !matches!(codec, "h264" | "mpeg4") || video_bps <= 0.0 {
        return None;
    }
    let pixels = (width.max(1) * height.max(1)) as f64;
    let floor = (pixels / (1920.0 * 1080.0)).clamp(0.25, 4.0) * 2_500_000.0;
    let target = (video_bps * 0.65).max(floor);
    (target < video_bps * 0.92).then_some(target as u64)
}

/// Allège le clip avant l'envoi (carte graphique si possible, son copié tel quel).
/// Renvoie le fichier allégé, ou None si l'original est gardé (déjà léger, ou réencodage raté).
fn compress_for_share(path: &Path, duration: f64, size: u64, out: &Path, cancelled: impl Fn() -> bool, mut progress: impl FnMut(f64)) -> Option<PathBuf> {
    let tools = ffmpeg::tools().ok()?;
    let probe = util::command(&tools.ffprobe)
        .args(["-v", "error", "-select_streams", "v:0", "-show_entries", "stream=codec_name,width,height,bit_rate", "-of", "json"])
        .arg(path)
        .stdin(std::process::Stdio::null())
        .output()
        .ok()?;
    let info: Value = serde_json::from_slice(&probe.stdout).ok()?;
    let v = &info["streams"][0];
    let (codec, width, height) = (v["codec_name"].as_str().unwrap_or(""), v["width"].as_u64().unwrap_or(0) as u32, v["height"].as_u64().unwrap_or(0) as u32);
    // Débit vidéo : celui du flux, sinon celui du fichier moins ~200 kb/s de son
    let video_bps = v["bit_rate"].as_str().and_then(|b| b.parse::<f64>().ok()).unwrap_or_else(|| (size as f64 * 8.0 / duration.max(0.1)) - 200_000.0);
    let target = share_bitrate(codec, width, height, video_bps)?;
    let encoder = ffmpeg::pick_encoder("auto");
    let (b, max, buf) = (target.to_string(), (target * 3 / 2).to_string(), (target * 2).to_string());
    let mut args: Vec<String> = ["-hide_banner", "-loglevel", "error", "-nostats", "-progress", "pipe:1", "-y", "-i"].iter().map(|s| s.to_string()).collect();
    args.push(path.to_string_lossy().to_string());
    args.extend(["-map", "0:v:0", "-map", "0:a?", "-c:v", &encoder].iter().map(|s| s.to_string()));
    let quality: &[&str] = if encoder.contains("nvenc") {
        &["-preset", "p5", "-tune", "hq", "-rc", "vbr", "-multipass", "qres"]
    } else if encoder.contains("amf") {
        &["-usage", "transcoding", "-quality", "quality", "-rc", "vbr_peak"]
    } else if encoder.contains("qsv") {
        &["-preset", "medium"]
    } else {
        &["-preset", "fast"]
    };
    args.extend(quality.iter().map(|s| s.to_string()));
    args.extend(["-b:v", &b, "-maxrate", &max, "-bufsize", &buf, "-pix_fmt", "yuv420p", "-c:a", "copy", "-movflags", "+faststart"].iter().map(|s| s.to_string()));
    args.push(out.to_string_lossy().to_string());
    let mut child = util::command(&tools.ffmpeg).args(&args).stdin(std::process::Stdio::null()).stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::piped()).spawn().ok()?;
    // Progression : lignes « out_time_us=… » de ffmpeg
    if let Some(stdout) = child.stdout.take() {
        use std::io::BufRead;
        for line in std::io::BufReader::new(stdout).lines().map_while(|l| l.ok()) {
            if cancelled() {
                let _ = child.kill();
                break;
            }
            if let Some(us) = line.strip_prefix("out_time_us=").and_then(|x| x.trim().parse::<f64>().ok()) {
                progress((us / 1e6 / duration.max(0.1)).clamp(0.0, 1.0));
            }
        }
    }
    let status = child.wait().ok()?;
    let new_size = std::fs::metadata(out).map(|m| m.len()).unwrap_or(0);
    let ok = status.success() && new_size > 0 && new_size < size * 9 / 10 && ffmpeg::duration(out).is_some_and(|d| (d - duration).abs() < 1.0);
    if !ok {
        if !cancelled() {
            let mut err = String::new();
            if let Some(mut e) = child.stderr.take() {
                let _ = e.read_to_string(&mut err);
            }
            log::warn!("partage : allègement impossible avec {encoder}, envoi de l'original ({})", err.lines().last().unwrap_or("taille non réduite"));
        }
        let _ = std::fs::remove_file(out);
        return None;
    }
    log::info!("partage : clip allégé avec {encoder} ({} → {} Mo)", size >> 20, new_size >> 20);
    Some(out.to_path_buf())
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
    let original = PathBuf::from(&clip.path);
    let original_size = std::fs::metadata(&original).map(|m| m.len()).map_err(|_| coded("file_missing"))?;
    let limits = view().lock().limits.clone();
    // Trop gros même une fois allégé
    if original_size > limits.max_clip * 2 {
        return Err(coded("clip_too_big"));
    }
    let names: Vec<String> = {
        let v = view().lock();
        to.iter().map(|id| v.friends.iter().find(|f| &f.id == id).map(|f| f.name.clone()).unwrap_or_default()).collect()
    };
    let key = format!("local-{}-{}", clip.id, now_ms());
    let mut t = Transfer { key: key.clone(), kind: "up".into(), title: clip.title.clone(), peer: names.join(", "), state: "running".into(), stage: "compress".into(), ..Default::default() };
    set_transfer(app, t.clone());
    let app = app.clone();
    let light_path = dir().join("envoi").join(format!("{key}.mp4"));
    std::thread::Builder::new().name("amis-envoi".into()).spawn(move || {
        let result = (|| -> Result<String> {
            // 0) Allègement de la vidéo (25 % de la barre) : 30 à 40 % de moins à envoyer et à stocker
            let _ = std::fs::create_dir_all(dir().join("envoi"));
            let mut last = Instant::now();
            let light = compress_for_share(&original, clip.duration, original_size, &light_path, || is_cancelled(&key, ""), |p| {
                if last.elapsed() > Duration::from_millis(250) {
                    last = Instant::now();
                    t.progress = 0.25 * p;
                    set_transfer(&app, t.clone());
                }
            });
            if is_cancelled(&t.key, "") {
                bail!("ami:cancelled");
            }
            let path = light.unwrap_or_else(|| original.clone());
            let size = std::fs::metadata(&path)?.len();
            if size > limits.max_clip {
                bail!("ami:clip_too_big");
            }
            t.stage.clear();
            t.progress = 0.25;
            set_transfer(&app, t.clone());
            // 1) Empreinte (5 % de la barre)
            let sha = file_sha256(&path, |done| {
                if last.elapsed() > Duration::from_millis(250) {
                    last = Instant::now();
                    t.progress = 0.25 + 0.05 * done as f64 / size.max(1) as f64;
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
                            t2.progress = 0.3 + 0.65 * (base + c2.load(Ordering::Relaxed)) as f64 / size.max(1) as f64;
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
        let _ = std::fs::remove_file(&light_path);
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
    fn cle_du_pc_stable() {
        // Lue deux fois de suite (et sans cache) : même identifiant, donc même compte
        let a = hardware_id().expect("identifiant de Windows lisible");
        let b = hardware_id().unwrap();
        assert_eq!(a, b);
        let (guid, board) = a.split_once('|').unwrap();
        assert!(guid.len() >= 32, "MachineGuid : {guid}");
        println!("carte mère lue : {}", if board.is_empty() { "non (MachineGuid seul)" } else { "oui" });
        assert_eq!(device_key().unwrap().verifying_key(), device_key().unwrap().verifying_key());
    }

    #[test]
    fn allegement_avant_envoi() {
        // Clip Rewind typique : 1080p à 5,6 Mb/s → visé 3,64 Mb/s
        let t = share_bitrate("h264", 1920, 1080, 5.6e6).unwrap();
        assert!((3_600_000..3_700_000).contains(&t), "{t}");
        // Déjà très léger : on garde l'original (plancher 1080p : 2,5 Mb/s)
        assert!(share_bitrate("h264", 1920, 1080, 2.6e6).is_none());
        // 1440p à 5 Mb/s : réduit, mais pas sous le plancher de 1440p (~4,4 Mb/s)
        let t = share_bitrate("h264", 2560, 1440, 5e6).unwrap();
        assert!((4_400_000..4_500_000).contains(&t), "{t}");
        // HEVC / AV1 : déjà efficaces, on ne réencode pas
        assert!(share_bitrate("hevc", 1920, 1080, 40e6).is_none());
        assert!(share_bitrate("av1", 1920, 1080, 40e6).is_none());
    }

    #[test]
    fn pseudo_nettoye() {
        assert_eq!(clean_name("  Lucas\n ").unwrap(), "Lucas");
        assert!(clean_name("   ").is_err());
        assert_eq!(clean_name(&"x".repeat(40)).unwrap().len(), 24);
    }
}
