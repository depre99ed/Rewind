//! Présence Discord (« Rich Presence ») : comme Medal ou Spotify, le profil Discord montre ce que fait Rewind
//! (« Enregistre Counter-Strike 2 », « Monte une vidéo dans le Studio »…) avec un bouton « Télécharger Rewind ».
//!
//! Gratuit et local : Rewind parle à l'application Discord ouverte sur le PC par son canal
//! `\\.\pipe\discord-ipc-N` (protocole RPC de Discord : en-tête de 8 octets = code + longueur, puis du JSON).
//! Rien ne passe par un serveur de Rewind. Désactivable dans les Réglages (`discord_presence`).
//!
//! L'affichage « Joue à Rewind » et le logo viennent de l'application Discord « Rewind » créée sur le
//! portail des développeurs de Discord : son identifiant (public, ce n'est pas un secret) est `CLIENT_ID`.

use crate::{AppState, util};
use parking_lot::Mutex;
use serde_json::{Value, json};
use std::io::{Read, Write};
use std::time::{Duration, Instant};
use tauri::{AppHandle, Manager};

/// Identifiant de l'application Discord « Rewind » (vide = présence inactive). Test : REWIND_DISCORD_CLIENT_ID.
const CLIENT_ID: &str = "1571581651333414922";
const DOWNLOAD_URL: &str = "https://github.com/depre99ed/Rewind/releases/latest";

fn client_id() -> String {
    std::env::var("REWIND_DISCORD_CLIENT_ID").unwrap_or_else(|_| CLIENT_ID.to_string())
}

/// Jeu vu au premier plan (exécutable, processus, heure de lancement du processus)
static GAME: Mutex<Option<(String, u32, f64)>> = Mutex::new(None);
/// Ce que l'utilisateur regarde dans la fenêtre de Rewind ("library", "studio", "viewer", "friends", "cs2", "settings")
static UI: Mutex<String> = Mutex::new(String::new());

/// Appelé par la surveillance des jeux quand un jeu est au premier plan.
pub fn set_game(exe: &str, pid: u32) {
    let mut g = GAME.lock();
    if g.as_ref().is_some_and(|(_, p, _)| *p == pid) {
        return;
    }
    let since = util::process_start(pid).unwrap_or_else(util::now);
    *g = Some((exe.to_string(), pid, since));
}

/// Appelé par l'interface quand on change de page.
pub fn set_ui(context: &str) {
    *UI.lock() = context.chars().take(20).collect();
}

/// Le jeu vu en dernier tourne-t-il encore ? (même processus, même heure de lancement)
fn running_game() -> Option<(String, f64)> {
    let mut g = GAME.lock();
    let (exe, pid, since) = g.clone()?;
    let alive = util::process_start(pid).is_some_and(|s| (s - since).abs() < 2.0);
    if !alive {
        *g = None;
        return None;
    }
    Some((util::friendly_app(&exe), since))
}

/// Ce que Discord doit afficher maintenant (None = rien).
fn wanted(app: &AppHandle) -> Option<Value> {
    let state = app.state::<AppState>();
    let english = util::english(&state.settings.lock().language);
    let tr = |fr: &str, en: &str| if english { en.to_string() } else { fr.to_string() };
    let recording = state.recorder.try_lock_for(Duration::from_millis(200)).map(|r| r.status().recording).unwrap_or(true);
    let visible = app.get_webview_window("main").is_some_and(|w| w.is_visible().unwrap_or(false) && !w.is_minimized().unwrap_or(false));
    let ui = UI.lock().clone();
    let doing = match ui.as_str() {
        "studio" => tr("Monte une vidéo dans le Studio", "Editing a video in the Studio"),
        "viewer" => tr("Regarde un clip", "Watching a clip"),
        "friends" => tr("Partage des clips avec ses amis", "Sharing clips with friends"),
        "cs2" => tr("Règle ses clips Counter-Strike 2", "Setting up Counter-Strike 2 clips"),
        "settings" => tr("Règle Rewind", "Setting up Rewind"),
        _ => tr("Regarde ses clips", "Browsing clips"),
    };
    let mut activity = json!({
        "assets": { "large_image": "rewind", "large_text": "Rewind" },
        "buttons": [ { "label": tr("Télécharger Rewind", "Get Rewind"), "url": DOWNLOAD_URL } ],
    });
    match (running_game().filter(|_| recording), visible) {
        (Some((game, since)), _) => {
            activity["details"] = json!(tr(&format!("Enregistre {game}"), &format!("Recording {game}")));
            activity["state"] = json!(if visible { doing } else { tr("Prêt à clipper", "Ready to clip") });
            activity["timestamps"] = json!({ "start": since as i64 });
        }
        (None, true) => {
            activity["details"] = json!(doing);
        }
        (None, false) => return None,
    }
    Some(activity)
}

/// Connexion au canal local de Discord.
struct Ipc {
    pipe: std::fs::File,
    nonce: u64,
}

impl Ipc {
    fn connect(client_id: &str) -> Option<Ipc> {
        for n in 0..10 {
            let path = format!(r"\\.\pipe\discord-ipc-{n}");
            let Ok(pipe) = std::fs::OpenOptions::new().read(true).write(true).open(&path) else { continue };
            let mut ipc = Ipc { pipe, nonce: 0 };
            if ipc.send(0, &json!({ "v": 1, "client_id": client_id })).is_err() {
                continue;
            }
            match ipc.read() {
                Some((_, v)) if v["evt"] == "READY" => {
                    log::info!("Discord : présence reliée ({})", v["data"]["user"]["username"].as_str().map(|_| "compte connecté").unwrap_or("?"));
                    return Some(ipc);
                }
                Some((_, v)) => {
                    // Refus : { data: { message } } ou, à la fermeture, { code, message }
                    let why = v["data"]["message"].as_str().or(v["message"].as_str()).unwrap_or("?");
                    log::warn!("Discord : connexion refusée ({why})");
                    return None;
                }
                None => continue,
            }
        }
        None
    }

    fn send(&mut self, op: u32, payload: &Value) -> std::io::Result<()> {
        let body = serde_json::to_vec(payload)?;
        let mut frame = Vec::with_capacity(8 + body.len());
        frame.extend_from_slice(&op.to_le_bytes());
        frame.extend_from_slice(&(body.len() as u32).to_le_bytes());
        frame.extend_from_slice(&body);
        self.pipe.write_all(&frame)?;
        self.pipe.flush()
    }

    fn read(&mut self) -> Option<(u32, Value)> {
        let mut head = [0u8; 8];
        self.pipe.read_exact(&mut head).ok()?;
        let op = u32::from_le_bytes(head[..4].try_into().ok()?);
        let len = u32::from_le_bytes(head[4..].try_into().ok()?) as usize;
        if len > 1 << 20 {
            return None;
        }
        let mut body = vec![0u8; len];
        self.pipe.read_exact(&mut body).ok()?;
        Some((op, serde_json::from_slice(&body).unwrap_or(Value::Null)))
    }

    /// Change l'activité affichée (None = effacer). false = canal perdu (Discord fermé).
    fn set(&mut self, activity: Option<&Value>) -> bool {
        self.nonce += 1;
        let msg = json!({ "cmd": "SET_ACTIVITY", "args": { "pid": std::process::id(), "activity": activity }, "nonce": self.nonce.to_string() });
        if self.send(1, &msg).is_err() {
            return false;
        }
        match self.read() {
            Some((2, _)) | None => false, // fermeture ou canal coupé
            Some((_, v)) => {
                if v["evt"] == "ERROR" {
                    log::warn!("Discord : activité refusée ({})", v["data"]["message"].as_str().unwrap_or("?"));
                }
                true
            }
        }
    }
}

/// Fil de la présence : vérifie toutes les 5 s ce qu'il faut afficher ; Discord fermé = nouvel essai plus tard.
pub fn start(app: AppHandle) {
    let _ = std::thread::Builder::new().name("discord".into()).spawn(move || {
        let mut ipc: Option<Ipc> = None;
        let mut shown: Option<Value> = None;
        let mut next_try = Instant::now();
        loop {
            std::thread::sleep(Duration::from_secs(5));
            let id = client_id();
            let enabled = !id.is_empty() && app.state::<AppState>().settings.lock().discord_presence;
            let want = if enabled { wanted(&app) } else { None };
            if want.is_none() && ipc.is_none() {
                shown = None;
                continue;
            }
            if ipc.is_none() {
                if Instant::now() < next_try {
                    continue;
                }
                ipc = Ipc::connect(&id);
                shown = None;
                if ipc.is_none() {
                    next_try = Instant::now() + Duration::from_secs(60); // Discord fermé : on réessaie dans 1 min
                    continue;
                }
            }
            if want == shown {
                continue;
            }
            let ok = ipc.as_mut().is_some_and(|c| c.set(want.as_ref()));
            if ok {
                shown = want;
                // Présence effacée et coupée : on rend la main à Discord
                if shown.is_none() && !enabled {
                    ipc = None;
                }
            } else {
                ipc = None;
                shown = None;
                next_try = Instant::now() + Duration::from_secs(15);
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Avec l'application Discord ouverte : un identifiant d'application inconnu est refusé vite et
    /// proprement ; avec REWIND_DISCORD_CLIENT_ID (vrai identifiant), l'activité est affichée 15 s puis effacée.
    /// `cargo test --lib discord_reel -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn discord_reel() {
        let t = Instant::now();
        let bogus = Ipc::connect("1");
        assert!(bogus.is_none(), "un faux identifiant ne doit pas être accepté");
        assert!(t.elapsed() < Duration::from_secs(5), "refus rapide");
        let Ok(id) = std::env::var("REWIND_DISCORD_CLIENT_ID") else { return };
        let mut ipc = Ipc::connect(&id).expect("Discord ouvert et identifiant valide");
        let activity = json!({
            "details": "Enregistre Counter-Strike 2", "state": "Test de Rewind",
            "timestamps": { "start": (util::now() - 754.0) as i64 },
            "assets": { "large_image": "rewind", "large_text": "Rewind" },
            "buttons": [ { "label": "Télécharger Rewind", "url": DOWNLOAD_URL } ],
        });
        assert!(ipc.set(Some(&activity)), "activité acceptée");
        println!("Regarde ton profil Discord pendant 40 s…");
        std::thread::sleep(Duration::from_secs(40));
        assert!(ipc.set(None), "activité effacée");
    }

    #[test]
    fn trame_discord() {
        // En-tête : code puis longueur, en petit-boutiste
        let body = br#"{"v":1}"#;
        let mut frame = Vec::new();
        frame.extend_from_slice(&0u32.to_le_bytes());
        frame.extend_from_slice(&(body.len() as u32).to_le_bytes());
        frame.extend_from_slice(body);
        assert_eq!(&frame[..8], &[0, 0, 0, 0, 7, 0, 0, 0]);
    }
}
