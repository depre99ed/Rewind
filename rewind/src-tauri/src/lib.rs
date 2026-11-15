//! Rewind — application de clips instantanés pour Windows.

mod admin;
mod amis;
mod medal;
mod ram;
mod audio;
mod cs2;
mod discord;
mod raccourcis;
mod rapport;
mod editor;
mod ffmpeg;
mod inputs;
mod library;
mod media;
mod recorder;
mod render;
mod settings;
mod share;
mod speech;
mod studio;
mod toast;
mod update;
mod util;
mod voice;
mod watchdog;

use library::{Clip, Library};
use parking_lot::Mutex;
use recorder::{ExportRequest, Recorder};
use serde::Serialize;
use settings::Settings;
use std::{
    path::PathBuf,
    sync::{Arc, mpsc},
    time::Duration,
};
use tauri::{
    AppHandle, Emitter, Manager, State, WindowEvent,
    menu::{Menu, MenuItem, PredefinedMenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
};
use tauri_plugin_autostart::ManagerExt as _;

pub struct AppState {
    recorder: Arc<Mutex<Recorder>>,
    library: Arc<Mutex<Library>>,
    settings: Arc<Mutex<Settings>>,
    cs2_tx: Mutex<Option<mpsc::Sender<settings::Cs2Settings>>>,
    cs2_live: Mutex<Option<Arc<Mutex<cs2::Cs2Live>>>>,
    /// Clip sur mesure en cours (1er appui = début)
    custom: Mutex<Option<Hold>>,
    /// Session complète en cours
    session: Mutex<Option<Hold>>,
    /// Dernier état connu du moteur : renvoyé si le moteur est occupé (l'interface n'attend jamais)
    last_status: Mutex<recorder::Status>,
    /// Mise à jour trouvée au lancement, montrée dans la fenêtre « update »
    pending_update: Mutex<Option<update::UpdateInfo>>,
    /// La fenêtre de mise à jour est ouverte au lancement (« Plus tard » ouvre alors Rewind) ou
    /// pendant l'utilisation (« Plus tard » la ferme simplement)
    update_at_launch: std::sync::atomic::AtomicBool,
}

/// Enregistrement « long » en cours : heure de début et repères accumulés (kills CS2, repères).
#[derive(Clone, Default)]
struct Hold {
    start: f64,
    game: String,
    markers: Vec<(f64, String, String)>,
}

type Res<T> = Result<T, String>;

/// Toute erreur renvoyée à l'interface passe par ici : on la garde aussi dans le journal,
/// pour que le rapport de diagnostic contienne ce que l'utilisateur a vu.
fn err<E: std::fmt::Display>(e: E) -> String {
    let text = format!("{e:#}");
    log::warn!("commande en échec : {text}");
    text
}

#[derive(Clone, Serialize)]
struct Progress {
    op: String,
    value: f64,
}

// ---------------------------------------------------------------------------
// Son de confirmation : synthétisé (aucun fichier externe), quatre styles au choix (Réglages › Notifications)
// ---------------------------------------------------------------------------

/// Petit générateur de bruit (reproductible : le son est le même à chaque fois).
struct Noise(u32);
impl Noise {
    fn next(&mut self) -> f64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 17;
        self.0 ^= self.0 << 5;
        self.0 as f64 / u32::MAX as f64 * 2.0 - 1.0
    }
}

/// Échantillons (48 kHz, -1..1) du son `kind`.
fn synth(kind: &str) -> Vec<f64> {
    use std::f64::consts::PI;
    let rate = 48_000.0;
    let mut n = Noise(0x2545_F491);
    let mut out: Vec<f64>;
    match kind {
        // Déclic d'obturateur : deux claquements secs (bruit filtré + petit choc grave), très courts
        "shutter" => {
            out = vec![0.0; (rate * 0.16) as usize];
            let (mut lp, mut prev) = (0.0, 0.0);
            for (i, o) in out.iter_mut().enumerate() {
                let t = i as f64 / rate;
                let click = |t0: f64, a: f64| if t >= t0 { a * (-(t - t0) * 110.0).exp() } else { 0.0 };
                let thump = |t0: f64, a: f64| if t >= t0 { a * (2.0 * PI * 140.0 * (t - t0)).sin() * (-(t - t0) * 70.0).exp() } else { 0.0 };
                let raw = n.next();
                // passe-haut puis passe-bas : un « tic » mat, pas un souffle
                let hp = raw - prev;
                prev = raw;
                lp += (hp - lp) * 0.45;
                *o = lp * (click(0.0, 1.0) + click(0.058, 0.7)) * 0.9 + thump(0.0, 0.35) + thump(0.058, 0.25);
            }
        }
        // Carillon doux : deux notes de cloche (partiels inharmoniques), la 2e un peu plus haute
        "chime" => {
            out = vec![0.0; (rate * 0.75) as usize];
            for (t0, f, a) in [(0.0, 880.0, 0.7), (0.085, 1318.5, 0.6)] {
                for (i, o) in out.iter_mut().enumerate() {
                    let t = i as f64 / rate - t0;
                    if t < 0.0 {
                        continue;
                    }
                    let att = (t / 0.004).min(1.0);
                    let v = (2.0 * PI * f * t).sin() * (-t * 7.0).exp()
                        + 0.28 * (2.0 * PI * f * 2.76 * t).sin() * (-t * 16.0).exp()
                        + 0.12 * (2.0 * PI * f * 5.40 * t).sin() * (-t * 28.0).exp();
                    *o += a * att * v;
                }
            }
        }
        // Rembobinage : bande qui accélère (glissando qui monte, frottement), puis arrêt net
        "rewind" => {
            out = vec![0.0; (rate * 0.30) as usize];
            let (mut phase, mut lp) = (0.0, 0.0);
            for (i, o) in out.iter_mut().enumerate() {
                let t = i as f64 / rate;
                phase += 2.0 * PI * 220.0 * (t * 7.5).exp() / rate;
                let scrub = 0.65 + 0.35 * (2.0 * PI * 38.0 * t).sin();
                let tone = phase.sin() * 0.6 + (phase * 2.0).sin() * 0.2;
                lp += (n.next() - lp) * 0.25;
                let env = (t / 0.012).min(1.0) * ((0.30 - t) / 0.05).clamp(0.0, 1.0);
                *o = (tone * 0.75 + lp * 0.35) * scrub * env;
            }
        }
        // Ancien son (deux notes pures), gardé pour qui le préfère
        _ => {
            out = vec![0.0; (rate * 0.32) as usize];
            for (i, o) in out.iter_mut().enumerate() {
                let t = i as f64 / rate;
                let f = if t < 0.09 { 1046.5 } else { 1568.0 };
                let local = if t < 0.09 { t } else { t - 0.09 };
                let env = (local * 400.0).min(1.0) * (-local * 14.0).exp();
                *o = ((2.0 * PI * f * t).sin() * 0.8 + (2.0 * PI * f * 2.0 * t).sin() * 0.2) * env;
            }
        }
    }
    // Même volume crête pour les quatre, fondu de 3 ms à la fin (pas de clic)
    let peak = out.iter().fold(0.0f64, |m, v| m.max(v.abs())).max(1e-9);
    let tail = (rate * 0.003) as usize;
    let len = out.len();
    for (i, v) in out.iter_mut().enumerate() {
        *v /= peak;
        if i + tail > len {
            *v *= (len - i) as f64 / tail as f64;
        }
    }
    out
}

fn custom_sound() -> PathBuf {
    util::data_dir().join("son-perso.wav")
}

/// Échantillons du son importé (format écrit par `sound_import` : 16 bits mono 48 kHz), -1..1.
fn custom_samples() -> Vec<f64> {
    let bytes = std::fs::read(custom_sound()).unwrap_or_default();
    let data = bytes.windows(4).position(|w| w == b"data").map(|p| p + 8).unwrap_or(44).min(bytes.len());
    let mut out: Vec<f64> = bytes[data..].chunks_exact(2).map(|c| i16::from_le_bytes([c[0], c[1]]) as f64 / i16::MAX as f64).collect();
    let peak = out.iter().fold(0.0f64, |m, v| m.max(v.abs())).max(1e-9);
    for v in out.iter_mut() {
        *v /= peak;
    }
    out
}

fn clip_sound(kind: &str, volume: u32) -> PathBuf {
    let path = util::data_dir().join(format!("son-{kind}-{volume}.wav"));
    if path.is_file() {
        return path;
    }
    let rate = 48_000u32;
    let amp = (volume.min(100) as f64 / 100.0) * 0.5;
    let raw = if kind == "custom" { custom_samples() } else { synth(kind) };
    if raw.is_empty() {
        return clip_sound("shutter", volume);
    }
    let samples: Vec<i16> = raw.into_iter().map(|v| (v * amp * i16::MAX as f64) as i16).collect();
    let mut wav: Vec<u8> = Vec::new();
    let data_len = (samples.len() * 2) as u32;
    wav.extend(b"RIFF");
    wav.extend((36 + data_len).to_le_bytes());
    wav.extend(b"WAVEfmt ");
    wav.extend(16u32.to_le_bytes());
    wav.extend(1u16.to_le_bytes());
    wav.extend(1u16.to_le_bytes());
    wav.extend(rate.to_le_bytes());
    wav.extend((rate * 2).to_le_bytes());
    wav.extend(2u16.to_le_bytes());
    wav.extend(16u16.to_le_bytes());
    wav.extend(b"data");
    wav.extend(data_len.to_le_bytes());
    for s in samples {
        wav.extend(s.to_le_bytes());
    }
    let _ = std::fs::write(&path, wav);
    path
}

fn play_file(path: &std::path::Path) {
    use windows_sys::Win32::Media::Audio::{PlaySoundW, SND_ASYNC, SND_FILENAME, SND_NODEFAULT};
    let w = util::wide(&path.to_string_lossy());
    unsafe {
        PlaySoundW(w.as_ptr(), std::ptr::null_mut(), SND_ASYNC | SND_FILENAME | SND_NODEFAULT);
    }
}

fn play_sound(settings: &Settings) {
    if !settings.sound_on_clip || settings.sound_volume == 0 {
        return;
    }
    play_file(&clip_sound(&settings.clip_sound, settings.sound_volume));
}

/// Réglages : importer son propre son de clip (n'importe quel fichier son : converti, 3 s au plus).
#[tauri::command(async)]
fn sound_import(app: AppHandle, state: State<AppState>, path: String) -> Res<String> {
    let tools = ffmpeg::tools().map_err(err)?;
    let tmp = util::data_dir().join("son-perso.tmp.wav");
    let out = util::command(&tools.ffmpeg)
        .args(["-hide_banner", "-loglevel", "error", "-y", "-i"])
        .arg(&path)
        .args(["-vn", "-af", "silenceremove=start_periods=1:start_threshold=-50dB", "-t", "3", "-ac", "1", "-ar", "48000", "-c:a", "pcm_s16le"])
        .arg(&tmp)
        .output()
        .map_err(err)?;
    if !out.status.success() || std::fs::metadata(&tmp).map(|m| m.len() < 2000).unwrap_or(true) {
        let _ = std::fs::remove_file(&tmp);
        return Err(err("ce fichier n'a pas de son lisible"));
    }
    std::fs::rename(&tmp, custom_sound()).map_err(err)?;
    // Anciennes versions du son importé (une par volume) : à refaire
    if let Ok(entries) = std::fs::read_dir(util::data_dir()) {
        for e in entries.flatten() {
            if e.file_name().to_string_lossy().starts_with("son-custom-") {
                let _ = std::fs::remove_file(e.path());
            }
        }
    }
    let name = std::path::Path::new(&path).file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
    let mut settings = state.settings.lock().clone();
    settings.clip_sound = "custom".into();
    settings.clip_sound_name = name.clone();
    let _ = settings.save();
    *state.settings.lock() = settings.clone();
    let _ = app.emit("settings-changed", settings);
    play_file(&clip_sound("custom", state.settings.lock().sound_volume.max(20)));
    Ok(name)
}

/// Réglages : écouter un son de clip avant de le choisir.
#[tauri::command(async)]
fn sound_preview(state: State<AppState>, kind: String) {
    let volume = state.settings.lock().sound_volume.max(20);
    play_file(&clip_sound(&kind, volume));
}

// ---------------------------------------------------------------------------
// Création des clips
// ---------------------------------------------------------------------------

/// Sauvegarde la période [start, end] (heures absolues). S'exécute dans un fil séparé.
fn save_clip(app: AppHandle, start: f64, end: f64, title: Option<String>, game: Option<String>, kind: &str, markers: Vec<(f64, String, String)>) {
    let kind = kind.to_string();
    std::thread::spawn(move || {
        save_clip_blocking(&app, start, end, title, game, &kind, markers);
    });
}

/// Même chose, en attendant le résultat (renvoie le clip créé).
fn save_clip_blocking(app: &AppHandle, start: f64, end: f64, title: Option<String>, game: Option<String>, kind: &str, markers: Vec<(f64, String, String)>) -> Option<Clip> {
    let kind = kind.to_string();
    {
        let state = app.state::<AppState>();
        let recorder = Arc::clone(&state.recorder);
        // Attendre que l'image et le son qui contiennent « end » soient écrits
        recorder::wait_written(&recorder, end, Duration::from_millis(4000));
        let settings = state.settings.lock().clone();
        let result = (|| -> anyhow::Result<Clip> {
            let (prepared, game) = {
                let r = recorder.lock();
                let game = game.clone().unwrap_or_else(|| r.main_app(start, end));
                (r.prepare_export(start, end)?, game)
            };
            let title = title.clone().unwrap_or_else(|| game.clone());
            let folder = recorder::clip_folder(&settings, &game);
            let first = prepared.first_start;
            let clip = recorder::export(prepared, ExportRequest { start, end, title, game, kind: kind.clone(), markers: markers.clone(), folder })?;
            if settings.input_overlay {
                inputs::save_for_clip(&clip.id, first, first + clip.duration);
            }
            Ok(clip)
        })();
        match result {
            Ok(clip) => {
                play_sound(&settings);
                toast::show(app, &settings, toast_for(&clip, "saved"));
                let _ = state.library.lock().add(clip.clone());
                log::info!("clip enregistré : {}", clip.path);
                let _ = app.emit("clip-saved", clip.clone());
                Some(clip)
            }
            Err(e) => {
                toast::show(app, &settings, toast::Toast { state: "failed".into(), title: tr(&settings, "Clip impossible", "Clip failed"), subtitle: format!("{e:#}"), ..Default::default() });
                log::warn!("clip impossible : {e:#}");
                let _ = app.emit("clip-failed", format!("{e:#}"));
                None
            }
        }
    }
}

/// Contenu de la notification en jeu pour un clip.
fn toast_for(clip: &Clip, state: &str) -> toast::Toast {
    let order = ["ace", "4k", "3k", "streak", "clutch", "collateral", "onetap", "knife", "zeus", "2k", "headshot"];
    let mut badges: Vec<String> = Vec::new();
    for k in order {
        if let Some(m) = clip.markers.iter().find(|m| m.kind == k) {
            let label = if k == "streak" { m.label.replace(" kills", "") } else { cs2::label(k).to_string() };
            if !badges.contains(&label) {
                badges.push(label);
            }
        }
    }
    badges.truncate(3);
    toast::Toast {
        state: state.into(),
        title: clip.title.clone(),
        subtitle: if clip.kind == "screenshot" { "Capture d'écran".into() } else { format!("{} · {:.0} s", clip.game, clip.duration) },
        game: clip.game.clone(),
        thumb: clip.thumb.clone(),
        badges,
        seconds: clip.duration,
        keys: Vec::new(),
        lang: String::new(),
    }
}

/// Texte de l'interface côté moteur (notifications, menu) dans la langue choisie.
fn tr(s: &Settings, fr: &str, en: &str) -> String {
    if util::english(&s.language) { en.to_string() } else { fr.to_string() }
}

fn clip_last(app: &AppHandle, seconds: u32) {
    let now = util::now();
    let _ = app.emit("clip-started", seconds);
    {
        let state = app.state::<AppState>();
        let settings = state.settings.lock().clone();
        let game = state.recorder.lock().main_app(now - seconds as f64, now);
        toast::show(app, &settings, toast::Toast {
            state: "saving".into(),
            title: if util::english(&settings.language) { format!("Last {seconds} seconds") } else { format!("{seconds} dernières secondes") },
            subtitle: game.clone(),
            game,
            seconds: seconds as f64,
            ..Default::default()
        });
    }
    save_clip(app.clone(), now - seconds as f64, now, None, None, "manual", vec![(now, "manual".into(), "Raccourci".into())]);
}

fn screenshot(app: &AppHandle) {
    let app = app.clone();
    std::thread::spawn(move || {
        let state = app.state::<AppState>();
        let settings = state.settings.lock().clone();
        let game = state.recorder.lock().main_app(util::now() - 2.0, util::now());
        let folder = recorder::clip_folder(&settings, &game).join("Captures");
        match recorder::screenshot(&state.recorder, &folder, &game) {
            Ok(clip) => {
                play_sound(&settings);
                toast::show(&app, &settings, toast_for(&clip, "shot"));
                let _ = state.library.lock().add(clip.clone());
                let _ = app.emit("clip-saved", clip);
            }
            Err(e) => {
                let _ = app.emit("clip-failed", format!("{e:#}"));
            }
        }
    });
}

// ---------------------------------------------------------------------------
// Clip sur mesure et session complète (appui = début, nouvel appui = fin)
// ---------------------------------------------------------------------------

/// Le tampon garde tout depuis le début du clip sur mesure ou de la session la plus ancienne.
fn update_hold(app: &AppHandle) {
    let state = app.state::<AppState>();
    let a = state.custom.lock().as_ref().map(|h| h.start);
    let b = state.session.lock().as_ref().map(|h| h.start);
    let from = match (a, b) {
        (Some(x), Some(y)) => Some(x.min(y)),
        (x, y) => x.or(y),
    };
    state.recorder.lock().set_hold(from);
}

/// Ajoute un repère au clip sur mesure et à la session en cours (kills CS2, touche « repère »).
fn add_hold_marker(app: &AppHandle, time: f64, kind: &str, label: &str) -> bool {
    let state = app.state::<AppState>();
    let mut added = false;
    for hold in [&state.custom, &state.session] {
        if let Some(h) = hold.lock().as_mut() {
            h.markers.push((time, kind.to_string(), label.to_string()));
            added = true;
        }
    }
    added
}

fn fmt_len(secs: f64) -> String {
    let s = secs.max(0.0).round() as u64;
    if s >= 3600 {
        format!("{} h {:02} min", s / 3600, (s % 3600) / 60)
    } else if s >= 60 {
        format!("{} min {:02} s", s / 60, s % 60)
    } else {
        format!("{s} s")
    }
}

/// Sauvegarde un enregistrement long dans un fil séparé, puis relâche le tampon.
fn finish_hold(app: &AppHandle, hold: Hold, title: String, kind: &'static str) {
    let app = app.clone();
    std::thread::spawn(move || {
        let end = util::now();
        let mut markers = hold.markers.clone();
        markers.retain(|m| m.0 >= hold.start - 1.0);
        markers.sort_by(|a, b| a.0.total_cmp(&b.0));
        let game = (!hold.game.is_empty()).then(|| hold.game.clone());
        save_clip_blocking(&app, hold.start, end, Some(title), game, kind, markers);
        update_hold(&app);
        let _ = app.emit("hold-changed", ());
    });
}

/// Durée courte et lisible pour un raccourci : « 30 s », « 2 min », « 1 min 30 ».
fn fmt_short(secs: u32) -> String {
    match (secs / 60, secs % 60) {
        (0, s) => format!("{s} s"),
        (m, 0) => format!("{m} min"),
        (m, s) => format!("{m} min {s:02}"),
    }
}

/// Bandeau d'accueil (6 s) quand un jeu se lance : le jeu est reconnu, voici les raccourcis.
fn welcome(app: &AppHandle, settings: &Settings, game: &str) {
    let h = &settings.hotkeys;
    let mut keys: Vec<(String, String)> = Vec::new();
    for (k, label) in [
        (&h.clip, format!("Clip {}", fmt_short(settings.clip_seconds))),
        (&h.clip_long, format!("Clip {}", fmt_short(settings.clip_long_seconds))),
        (&h.clip_custom, tr(settings, "Clip sur mesure", "Custom clip")),
        (&h.session, tr(settings, "Session complète", "Full session")),
    ] {
        if !k.trim().is_empty() {
            keys.push((k.clone(), label));
        }
    }
    let paused = app.state::<AppState>().recorder.lock().paused;
    toast::show(app, settings, toast::Toast {
        state: "welcome".into(),
        title: game.to_string(),
        subtitle: if paused {
            format!("{} · {} {}", tr(settings, "Enregistrement en pause", "Recording paused"), h.toggle_recording, tr(settings, "pour reprendre", "to resume"))
        } else {
            tr(settings, "Rewind enregistre · tes raccourcis", "Rewind is recording · your hotkeys")
        },
        game: game.to_string(),
        keys,
        ..Default::default()
    });
}

fn toggle_custom(app: &AppHandle) {
    let state = app.state::<AppState>();
    let settings = state.settings.lock().clone();
    let now = util::now();
    let running = state.custom.lock().take();
    match running {
        None => {
            let game = state.recorder.lock().main_app(now - 2.0, now);
            *state.custom.lock() = Some(Hold { start: now, game: game.clone(), markers: vec![(now, "manual".into(), "Début".into())] });
            update_hold(app);
            play_sound(&settings);
            toast::show(app, &settings, toast::Toast {
                state: "marked".into(),
                title: tr(&settings, "Clip sur mesure lancé", "Custom clip started"),
                subtitle: format!("{} {} {}", tr(&settings, "Rappuie sur", "Press"), settings.hotkeys.clip_custom, tr(&settings, "pour le terminer", "again to finish")),
                game,
                ..Default::default()
            });
            let _ = app.emit("hold-changed", ());
        }
        Some(h) => {
            let len = now - h.start;
            toast::show(app, &settings, toast::Toast {
                state: "saving".into(),
                title: format!("{} · {}", tr(&settings, "Clip sur mesure", "Custom clip"), fmt_len(len)),
                subtitle: h.game.clone(),
                game: h.game.clone(),
                seconds: len,
                ..Default::default()
            });
            let _ = app.emit("clip-started", len.round() as u32);
            let title = if h.game.is_empty() { tr(&settings, "Clip sur mesure", "Custom clip") } else { h.game.clone() };
            finish_hold(app, h, title, "manual");
        }
    }
}

fn toggle_session(app: &AppHandle) {
    let state = app.state::<AppState>();
    let settings = state.settings.lock().clone();
    let now = util::now();
    let running = state.session.lock().take();
    match running {
        None => {
            let game = state.recorder.lock().main_app(now - 2.0, now);
            *state.session.lock() = Some(Hold { start: now, game: game.clone(), markers: Vec::new() });
            update_hold(app);
            play_sound(&settings);
            toast::show(app, &settings, toast::Toast {
                state: "marked".into(),
                title: tr(&settings, "Session complète en cours", "Full session recording"),
                subtitle: format!("{} · {} {}", tr(&settings, "Tout est gardé", "Everything is kept"), settings.hotkeys.session, tr(&settings, "pour terminer", "to finish")),
                game,
                ..Default::default()
            });
            let _ = app.emit("hold-changed", ());
        }
        Some(h) => {
            let len = now - h.start;
            if len < 3.0 {
                // Double appui par erreur : on annule simplement
                update_hold(app);
                let _ = app.emit("hold-changed", ());
                return;
            }
            let kills = h.markers.iter().filter(|m| cs2::is_kill(&m.1)).count();
            toast::show(app, &settings, toast::Toast {
                state: "saving".into(),
                title: format!("{} {}", tr(&settings, "Session de", "Session of"), fmt_len(len)),
                subtitle: if kills > 0 { format!("{} · {kills} {}", h.game, tr(&settings, "kills repérés", "kills marked")) } else { h.game.clone() },
                game: h.game.clone(),
                seconds: len,
                ..Default::default()
            });
            let _ = app.emit("clip-started", len.round() as u32);
            let date = chrono::Local::now().format("%d-%m %Hh%M");
            let title = if h.game.is_empty() { format!("Session {date}") } else { format!("Session {} {date}", h.game) };
            finish_hold(app, h, title, "session");
        }
    }
}

fn bookmark(app: &AppHandle) {
    let state = app.state::<AppState>();
    let settings = state.settings.lock().clone();
    let now = util::now();
    if add_hold_marker(app, now, "manual", "Repère") {
        play_sound(&settings);
        toast::show(app, &settings, toast::Toast { state: "marked".into(), title: tr(&settings, "Repère ajouté", "Marker added"), subtitle: tr(&settings, "Il apparaîtra sur la frise du clip", "It will show on the clip timeline"), ..Default::default() });
    } else {
        toast::show(app, &settings, toast::Toast {
            state: "marked".into(),
            title: tr(&settings, "Aucune session en cours", "No session running"),
            subtitle: format!("{} ({}) {} ({})", tr(&settings, "Lance une session", "Start a session"), settings.hotkeys.session, tr(&settings, "ou un clip sur mesure", "or a custom clip"), settings.hotkeys.clip_custom),
            ..Default::default()
        });
    }
}

fn toggle_recording(app: &AppHandle) {
    let state = app.state::<AppState>();
    let paused = {
        let mut r = state.recorder.lock();
        let p = !r.paused;
        r.set_paused(p);
        p
    };
    {
        let mut s = state.settings.lock();
        s.recording_enabled = !paused;
        let _ = s.save();
    }
    let _ = app.emit("settings-changed", state.settings.lock().clone());
}

// ---------------------------------------------------------------------------
// Raccourcis clavier globaux
// ---------------------------------------------------------------------------

/// Exécute `f` hors du fil de l'interface (raccourcis, menu de l'icône).
fn off_ui(app: &AppHandle, f: impl FnOnce(&AppHandle) + Send + 'static) {
    let app = app.clone();
    std::thread::spawn(move || f(&app));
}

/// Raccourcis lus par le moteur d'entrées (Raw Input, inputs.rs) : rien n'est réservé auprès de
/// Windows, la touche n'est jamais avalée (elle reste saisissable dans les Réglages), une touche seule
/// (F8) ou un bouton de souris marche, et Ctrl/Maj tenus en plus (s'accroupir, marcher) n'empêchent rien.
fn register_hotkeys(_app: &AppHandle, settings: &Settings) -> Vec<String> {
    let h = &settings.hotkeys;
    inputs::set_bindings(&[
        (h.clip.clone(), "clip"),
        (h.clip_long.clone(), "clip_long"),
        (h.clip_custom.clone(), "clip_custom"),
        (h.session.clone(), "session"),
        (h.bookmark.clone(), "bookmark"),
        (h.screenshot.clone(), "screenshot"),
        (h.toggle_recording.clone(), "toggle"),
    ])
}

/// Action d'un raccourci (appelée depuis le fil des entrées : le travail part dans un fil à lui).
fn run_hotkey(app: &AppHandle, action: &'static str) {
    off_ui(app, move |app| {
        let s = app.state::<AppState>().settings.lock().clone();
        match action {
            "clip" => clip_last(app, s.clip_seconds),
            "clip_long" => clip_last(app, s.clip_long_seconds),
            "clip_custom" => toggle_custom(app),
            "session" => toggle_session(app),
            "bookmark" => bookmark(app),
            "screenshot" => screenshot(app),
            "toggle" => toggle_recording(app),
            _ => {}
        }
    });
}

// ---------------------------------------------------------------------------
// Commandes appelées par l'interface
// ---------------------------------------------------------------------------

#[tauri::command(async)]
fn get_settings(state: State<AppState>) -> Settings {
    state.settings.lock().clone()
}

/// Micro écouté par la commande vocale : celui choisi dans les sources audio de Rewind.
fn voice_mic(settings: &Settings) -> String {
    settings.audio_sources.iter().find(|s| s.kind == "mic").map(|s| s.device.clone()).unwrap_or_default()
}

/// Dernière fois que « Rewind clip » a été entendu (bouton « Tester » des Réglages).
#[tauri::command(async)]
fn voice_last() -> Option<voice::Heard> {
    voice::last_heard()
}

/// Pendant la saisie d'un raccourci dans les Réglages : les raccourcis ne se déclenchent pas.
#[tauri::command(async)]
fn hotkeys_pause(on: bool) {
    inputs::pause_hotkeys(on);
}

/// Nom des touches de l'overlay dans la disposition du clavier (Z Q S D sur un AZERTY).
#[tauri::command(async)]
fn key_labels() -> std::collections::HashMap<u16, String> {
    inputs::key_labels()
}

#[derive(Serialize)]
struct SaveResult {
    hotkey_errors: Vec<String>,
    cs2_files: Vec<String>,
}

#[tauri::command(async)]
fn save_settings(app: AppHandle, state: State<AppState>, mut settings: Settings) -> Res<SaveResult> {
    settings.sanitize();
    settings.save().map_err(err)?;
    let old = std::mem::replace(&mut *state.settings.lock(), settings.clone());
    state.recorder.lock().apply_settings(settings.clone());
    let hotkey_errors = register_hotkeys(&app, &settings);
    if old.input_overlay != settings.input_overlay {
        inputs::set_overlay(settings.input_overlay);
    }
    voice::set_mic(&voice_mic(&settings));
    voice::set_enabled(settings.voice_command);
    if old.launch_at_startup != settings.launch_at_startup {
        let al = app.autolaunch();
        let _ = if settings.launch_at_startup { al.enable() } else { al.disable() };
    }
    let mut cs2_files = Vec::new();
    if settings.cs2.enabled {
        cs2_files = cs2::install_config(settings.cs2.port, &cs2::token());
    }
    if let Some(tx) = state.cs2_tx.lock().as_ref() {
        let _ = tx.send(settings.cs2.effective());
    }
    if old.clip_folder != settings.clip_folder {
        let mut lib = state.library.lock();
        lib.scan(&PathBuf::from(&settings.clip_folder));
        lib.sort();
        let _ = lib.save();
    }
    let _ = app.emit("settings-changed", settings);
    Ok(SaveResult { hotkey_errors, cs2_files })
}

#[tauri::command(async)]
fn get_status(state: State<AppState>) -> recorder::Status {
    // Interrogé plusieurs fois par seconde : si le moteur est occupé (démarrage de FFmpeg…),
    // on renvoie le dernier état connu au lieu d'empiler des attentes.
    let fresh = state.recorder.try_lock_for(Duration::from_millis(150)).map(|r| r.status());
    let mut st = match fresh {
        Some(s) => {
            *state.last_status.lock() = s.clone();
            s
        }
        None => state.last_status.lock().clone(),
    };
    st.custom_since = state.custom.lock().as_ref().map(|h| h.start).unwrap_or(0.0);
    st.session_since = state.session.lock().as_ref().map(|h| h.start).unwrap_or(0.0);
    st.voice = voice::STATE.load(std::sync::atomic::Ordering::Relaxed);
    st
}

/// Test de la signature des mises à jour : `rewind.exe --verify-update <installeur> <.sig>`.
pub fn verify_update_test(file: PathBuf, sig: PathBuf) -> i32 {
    update::self_test(&file, &sig)
}

/// Diagnostic du canal réel, sans fenêtre, capture ni accès aux données personnelles.
pub fn probe_update(out: PathBuf, download: bool) {
    let mut context = tauri::generate_context!();
    context.config_mut().app.windows.clear();
    // Les tests debug peuvent simuler un ancien client ; jamais dans l'application distribuée.
    #[cfg(debug_assertions)]
    if let Ok(version) = std::env::var("REWIND_TEST_CURRENT_VERSION") {
        context.package_info_mut().version = version.parse().expect("version de test invalide");
    }
    tauri::Builder::default()
        .plugin(tauri_plugin_updater::Builder::new().build())
        .setup(move |app| {
            let handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                let result = update::probe(&handle, download).await;
                let code = if result.is_ok() { 0 } else { 1 };
                let report = match result {
                    Ok(value) => value,
                    Err(error) => serde_json::json!({ "error": error }),
                };
                let written = std::fs::write(&out, serde_json::to_vec_pretty(&report).unwrap());
                handle.exit(if written.is_ok() { code } else { 2 });
            });
            Ok(())
        })
        .run(context)
        .expect("diagnostic du canal de mises à jour");
}

/// Test de la commande vocale : `rewind.exe --voice <dossier de .wav>`.
pub fn voice_test(dir: PathBuf) -> i32 {
    init_logging();
    voice::self_test(&dir)
}

/// Pause asynchrone (sans bloquer les fils de l'interface).
async fn tokio_sleep(secs: u64) {
    let _ = tauri::async_runtime::spawn_blocking(move || std::thread::sleep(Duration::from_secs(secs))).await;
}

/// Mises à jour : recherche (sans rien installer).
#[tauri::command]
async fn check_update(app: AppHandle) -> Res<update::UpdateInfo> {
    update::check(&app).await
}

/// Mises à jour : télécharge, vérifie la signature, installe et relance Rewind.
#[tauri::command]
async fn install_update(app: AppHandle) -> Res<()> {
    update::install(&app).await
}

/// Touches clavier/souris gardées avec un clip (vide si l'option était coupée).
#[tauri::command(async)]
fn clip_inputs(id: String) -> Vec<inputs::InputEvent> {
    inputs::for_clip(&id)
}

/// Boutons de l'interface : clip sur mesure / session complète / repère (comme les raccourcis).
#[tauri::command(async)]
fn hold_action(app: AppHandle, action: String) {
    match action.as_str() {
        "custom" => toggle_custom(&app),
        "session" => toggle_session(&app),
        "bookmark" => bookmark(&app),
        _ => {}
    }
}

#[tauri::command(async)]
fn list_clips(state: State<AppState>) -> Vec<Clip> {
    state.library.lock().clips.clone()
}

#[tauri::command(async)]
fn clip_now(app: AppHandle, state: State<AppState>, seconds: Option<u32>) {
    let s = seconds.unwrap_or_else(|| state.settings.lock().clip_seconds);
    clip_last(&app, s);
}

#[tauri::command(async)]
fn take_screenshot(app: AppHandle) {
    screenshot(&app);
}

#[tauri::command(async)]
fn set_recording(app: AppHandle, state: State<AppState>, enabled: bool) {
    let paused = state.recorder.lock().paused;
    if paused == enabled {
        toggle_recording(&app);
    }
}

#[tauri::command(async)]
fn set_mic_muted(state: State<AppState>, muted: bool) {
    state.recorder.lock().mic_muted.store(muted, std::sync::atomic::Ordering::Relaxed);
}

#[tauri::command(async)]
fn update_clip(state: State<AppState>, id: String, title: Option<String>, favorite: Option<bool>, tags: Option<Vec<String>>) -> Res<Clip> {
    let mut lib = state.library.lock();
    if let Some(t) = title {
        let t = t.trim();
        if !t.is_empty() {
            lib.rename(&id, t).map_err(err)?;
        }
    }
    let clip = lib.get_mut(&id).ok_or("Clip introuvable")?;
    if let Some(f) = favorite {
        clip.favorite = f;
    }
    if let Some(t) = tags {
        clip.tags = t;
    }
    let c = clip.clone();
    lib.save().map_err(err)?;
    Ok(c)
}

#[tauri::command(async)]
fn delete_clips(state: State<AppState>, ids: Vec<String>) -> Res<()> {
    let mut lib = state.library.lock();
    for id in ids {
        lib.remove(&id).map_err(err)?;
    }
    Ok(())
}

fn clip_path(state: &State<AppState>, id: &str) -> Res<PathBuf> {
    state.library.lock().get(id).map(|c| PathBuf::from(&c.path)).ok_or_else(|| "Clip introuvable".to_string())
}

#[tauri::command(async)]
fn reveal_clip(state: State<AppState>, id: String) -> Res<()> {
    share::reveal(&clip_path(&state, &id)?).map_err(err)
}

#[tauri::command(async)]
fn open_external(state: State<AppState>, id: String) -> Res<()> {
    share::open_default(&clip_path(&state, &id)?).map_err(err)
}

#[tauri::command(async)]
fn open_folder(path: String) -> Res<()> {
    let p = PathBuf::from(path);
    std::fs::create_dir_all(&p).ok();
    share::open_default(&p).map_err(err)
}

#[tauri::command(async)]
fn copy_clip(state: State<AppState>, id: String) -> Res<()> {
    share::copy_file_to_clipboard(&clip_path(&state, &id)?).map_err(err)
}

#[derive(Serialize)]
struct DiscordCopy {
    compressed: bool,
    size_mb: f64,
    height: u32,
    max_mb: u32,
}

/// « Copier pour Discord » : compresse si besoin sous la taille choisie, puis met le fichier
/// dans le presse-papiers (Ctrl+V dans Discord).
#[tauri::command]
async fn copy_for_discord(app: AppHandle, id: String) -> Res<DiscordCopy> {
    let (path, max_mb, is_shot) = {
        let state = app.state::<AppState>();
        let lib = state.library.lock();
        let clip = lib.get(&id).ok_or("Clip introuvable")?;
        (PathBuf::from(&clip.path), state.settings.lock().discord_max_mb, clip.kind == "screenshot")
    };
    tauri::async_runtime::spawn_blocking(move || -> anyhow::Result<DiscordCopy> {
        let file = if is_shot {
            editor::DiscordFile { path: path.clone(), height: 0, compressed: false }
        } else {
            let dir = util::data_dir().join("discord");
            // On ne garde que les dernières versions compressées
            if let Ok(entries) = std::fs::read_dir(&dir) {
                let mut files: Vec<_> = entries.flatten().filter_map(|e| Some((e.metadata().ok()?.modified().ok()?, e.path()))).collect();
                files.sort_by(|a, b| b.0.cmp(&a.0));
                for (_, p) in files.into_iter().skip(8) {
                    let _ = std::fs::remove_file(p);
                }
            }
            editor::prepare_for_discord(&path, max_mb as f64, &dir, &progress_fn(&app, "compress"))?
        };
        share::copy_file_to_clipboard(&file.path)?;
        let size_mb = std::fs::metadata(&file.path).map(|m| m.len()).unwrap_or(0) as f64 / 1_048_576.0;
        Ok(DiscordCopy { compressed: file.compressed, size_mb, height: file.height, max_mb })
    })
    .await
    .map_err(err)?
    .map_err(err)
}

fn add_edit_result(app: &AppHandle, source: &Clip, out: PathBuf, suffix: &str) -> anyhow::Result<Clip> {
    let id = library::new_id();
    let thumb = library::thumbs_dir().join(format!("{id}.jpg"));
    let duration = ffmpeg::duration(&out).unwrap_or(0.0);
    let _ = ffmpeg::thumbnail(&out, (duration * 0.4).min(3.0), &thumb);
    let clip = Clip {
        id,
        path: out.to_string_lossy().to_string(),
        thumb: if thumb.is_file() { thumb.to_string_lossy().to_string() } else { String::new() },
        title: format!("{} {suffix}", source.title),
        game: source.game.clone(),
        created: util::now(),
        duration,
        size: std::fs::metadata(&out).map(|m| m.len()).unwrap_or(0),
        kind: "edit".into(),
        markers: Vec::new(),
        favorite: false,
        tags: source.tags.clone(),
    };
    app.state::<AppState>().library.lock().add(clip.clone())?;
    let _ = app.emit("clip-saved", clip.clone());
    Ok(clip)
}

fn progress_fn(app: &AppHandle, op: &str) -> impl Fn(f64) + use<> {
    let app = app.clone();
    let op = op.to_string();
    move |v| {
        let _ = app.emit("edit-progress", Progress { op: op.clone(), value: v });
    }
}

#[tauri::command]
async fn edit_trim(app: AppHandle, id: String, start: f64, end: f64, exact: bool) -> Res<Clip> {
    let source = app.state::<AppState>().library.lock().get(&id).cloned().ok_or("Clip introuvable")?;
    tauri::async_runtime::spawn_blocking(move || {
        let out = editor::trim(&PathBuf::from(&source.path), start, end, exact, &progress_fn(&app, "trim"))?;
        add_edit_result(&app, &source, out, "(coupé)")
    })
    .await
    .map_err(err)?
    .map_err(err)
}

#[tauri::command]
async fn edit_merge(app: AppHandle, ids: Vec<String>) -> Res<Clip> {
    let clips: Vec<Clip> = {
        let state = app.state::<AppState>();
        let lib = state.library.lock();
        ids.iter().filter_map(|id| lib.get(id).cloned()).collect()
    };
    if clips.len() < 2 {
        return Err("Choisis au moins deux clips.".into());
    }
    tauri::async_runtime::spawn_blocking(move || {
        let paths: Vec<PathBuf> = clips.iter().map(|c| PathBuf::from(&c.path)).collect();
        let out = editor::merge(&paths, &progress_fn(&app, "merge"))?;
        add_edit_result(&app, &clips[0], out, "(fusion)")
    })
    .await
    .map_err(err)?
    .map_err(err)
}

#[tauri::command]
async fn edit_vertical(app: AppHandle, id: String, mode: String, start: f64, end: f64) -> Res<Clip> {
    let source = app.state::<AppState>().library.lock().get(&id).cloned().ok_or("Clip introuvable")?;
    tauri::async_runtime::spawn_blocking(move || {
        let out = editor::vertical(&PathBuf::from(&source.path), &mode, start, end, &progress_fn(&app, "vertical"))?;
        add_edit_result(&app, &source, out, "(vertical)")
    })
    .await
    .map_err(err)?
    .map_err(err)
}

#[tauri::command]
async fn media_info(path: String) -> Res<media::MediaInfo> {
    tauri::async_runtime::spawn_blocking(move || media::info(std::path::Path::new(&path))).await.map_err(err)?.map_err(err)
}

#[tauri::command]
async fn media_filmstrip(path: String) -> Res<media::Filmstrip> {
    tauri::async_runtime::spawn_blocking(move || media::filmstrip(std::path::Path::new(&path))).await.map_err(err)?.map_err(err)
}

#[tauri::command]
async fn media_waveform(path: String) -> Res<media::Waveform> {
    tauri::async_runtime::spawn_blocking(move || media::waveform(std::path::Path::new(&path))).await.map_err(err)?.map_err(err)
}

/// Pistes son séparées d'un clip, en fichiers à part (aperçu du Studio).
#[tauri::command]
async fn media_stems(path: String) -> Res<Vec<String>> {
    tauri::async_runtime::spawn_blocking(move || media::stems(std::path::Path::new(&path))).await.map_err(err)?.map_err(err)
}

#[tauri::command]
async fn studio_export(app: AppHandle, pieces: Vec<editor::Piece>, options: editor::StudioOptions, game: String) -> Res<Clip> {
    tauri::async_runtime::spawn_blocking(move || -> anyhow::Result<Clip> {
        let settings = app.state::<AppState>().settings.lock().clone();
        let folder = recorder::clip_folder(&settings, if game.is_empty() { "Montages" } else { &game });
        let out = editor::studio_export(&pieces, &options, &folder, &progress_fn(&app, "studio"))?;
        let source = Clip { title: options.title.clone(), game: game.clone(), ..Default::default() };
        let mut clip = add_edit_result(&app, &source, out, "")?;
        clip.title = options.title.clone();
        let state = app.state::<AppState>();
        if let Some(c) = state.library.lock().get_mut(&clip.id) {
            c.title = options.title.clone();
        }
        let _ = state.library.lock().save();
        Ok(clip)
    })
    .await
    .map_err(err)?
    .map_err(err)
}

/// Export du Studio v2 (calques, zoom, plusieurs pistes son…) : voir render.rs.
#[tauri::command]
async fn studio_render(app: AppHandle, spec: render::RenderSpec, game: String) -> Res<Clip> {
    tauri::async_runtime::spawn_blocking(move || -> anyhow::Result<Clip> {
        let settings = app.state::<AppState>().settings.lock().clone();
        let folder = recorder::clip_folder(&settings, if game.is_empty() { "Montages" } else { &game });
        let out = render::render(&spec, &folder, &progress_fn(&app, "studio"))?;
        let source = Clip { title: spec.title.clone(), game: game.clone(), ..Default::default() };
        let mut clip = add_edit_result(&app, &source, out, "")?;
        clip.title = spec.title.clone();
        let state = app.state::<AppState>();
        if let Some(c) = state.library.lock().get_mut(&clip.id) {
            c.title = spec.title.clone();
        }
        let _ = state.library.lock().save();
        log::info!("montage exporté : {}", clip.path);
        Ok(clip)
    })
    .await
    .map_err(err)?
    .map_err(err)
}

#[tauri::command(async)]
fn projects_list() -> Vec<studio::Meta> {
    studio::list()
}

#[tauri::command(async)]
fn project_load(id: String) -> Res<String> {
    studio::load(&id).map_err(err)
}

#[tauri::command(async)]
fn project_save(id: String, json: String) -> Res<()> {
    studio::save(&id, &json).map_err(err)
}

#[tauri::command(async)]
fn project_delete(id: String) -> Res<()> {
    studio::delete(&id).map_err(err)
}

/// Image dessinée par l'interface (texte, touches) → fichier PNG pour l'export.
#[tauri::command(async)]
fn studio_png(b64: String) -> Res<String> {
    studio::save_png(&b64).map_err(err)
}

/// Voix off : enregistre le micro choisi dans Rewind.
#[tauri::command(async)]
fn voiceover_start(state: State<AppState>) -> Res<()> {
    let mic = voice_mic(&state.settings.lock());
    studio::voiceover_start(mic).map_err(err)
}

#[tauri::command(async)]
fn voiceover_stop() -> Res<studio::VoiceOver> {
    studio::voiceover_stop().map_err(err)
}

#[derive(Serialize)]
struct ShareResult {
    url: String,
    compressed: bool,
}

// ---------------------------------------------------------------------------
// Partage de clips entre amis (amis.rs, guichet : rewind/partage)
// ---------------------------------------------------------------------------

/// Page affichée dans Rewind (présence Discord : « Monte une vidéo dans le Studio »…).
#[tauri::command(async)]
fn discord_context(view: String) {
    discord::set_ui(&view);
    amis::set_focus(view == "friends");
}

#[tauri::command(async)]
fn friends_view() -> amis::View {
    amis::snapshot()
}

#[tauri::command(async)]
fn friends_enable(app: AppHandle, name: String) -> Res<amis::View> {
    amis::enable(&app, &name).map_err(err)
}

#[tauri::command(async)]
fn friends_disable(app: AppHandle) {
    amis::disable(&app);
}

#[tauri::command(async)]
fn friends_rename(app: AppHandle, name: String) -> Res<()> {
    amis::rename(&app, &name).map_err(err)
}

#[tauri::command(async)]
fn friends_add(app: AppHandle, code: String) -> Res<String> {
    amis::add(&app, &code).map_err(err)
}

#[tauri::command(async)]
fn friends_answer(app: AppHandle, user: String, accept: bool) -> Res<()> {
    amis::answer(&app, &user, accept).map_err(err)
}

#[tauri::command(async)]
fn friends_remove(app: AppHandle, user: String, block: bool) -> Res<()> {
    amis::remove(&app, &user, block).map_err(err)
}

#[tauri::command(async)]
fn friends_refresh() {
    amis::wake();
}

#[tauri::command(async)]
fn friends_delete_identity(app: AppHandle) -> Res<()> {
    amis::delete_identity(&app).map_err(err)
}

#[tauri::command(async)]
fn friends_recovery_key() -> Res<String> {
    amis::recovery_key().map_err(err)
}

#[tauri::command(async)]
fn friends_new_recovery(app: AppHandle) -> Res<String> {
    amis::new_recovery(&app).map_err(err)
}

#[tauri::command(async)]
fn friends_save_recovery(path: String) -> Res<()> {
    amis::save_recovery(std::path::Path::new(&path)).map_err(err)
}

#[tauri::command(async)]
fn friends_restore(app: AppHandle, key: String) -> Res<amis::View> {
    amis::restore(&app, &key).map_err(err)
}

#[tauri::command(async)]
fn friends_set_avatar(app: AppHandle, path: String) -> Res<()> {
    amis::set_avatar(&app, std::path::Path::new(&path)).map_err(err)
}

#[tauri::command(async)]
fn friends_remove_avatar(app: AppHandle) -> Res<()> {
    amis::remove_avatar(&app).map_err(err)
}

#[tauri::command(async)]
fn friends_avatar(id: String, version: u64) -> Res<String> {
    amis::avatar(&id, version).map(|p| p.to_string_lossy().to_string()).map_err(err)
}

#[tauri::command(async)]
fn friends_show_online(app: AppHandle, show: bool) -> Res<()> {
    amis::set_show_online(&app, show).map_err(err)
}

#[tauri::command(async)]
fn share_send(app: AppHandle, id: String, to: Vec<String>) -> Res<()> {
    amis::send(&app, &id, to).map_err(err)
}

#[tauri::command(async)]
fn share_cancel(app: AppHandle, key: String) -> Res<()> {
    amis::cancel(&app, &key).map_err(err)
}

#[tauri::command(async)]
fn share_dismiss(app: AppHandle, id: String) -> Res<()> {
    amis::dismiss(&app, &id).map_err(err)
}

#[tauri::command(async)]
fn share_report(id: String, reason: String) -> Res<()> {
    amis::report(&id, &reason).map_err(err)
}

#[tauri::command(async)]
fn share_download(app: AppHandle, id: String) -> Res<()> {
    amis::download_now(&app, &id).map_err(err)
}

#[tauri::command(async)]
fn share_thumb(id: String) -> Res<String> {
    amis::thumb(&id).map(|p| p.to_string_lossy().to_string()).map_err(err)
}

#[tauri::command]
async fn share_discord(app: AppHandle, id: String, webhook: usize, message: String) -> Res<ShareResult> {
    let (source, url) = {
        let state = app.state::<AppState>();
        let source = state.library.lock().get(&id).cloned().ok_or("Clip introuvable")?;
        let url = state.settings.lock().webhooks.get(webhook).map(|w| w.url.clone()).ok_or("Ajoute d'abord un webhook Discord dans les réglages.")?;
        (source, url)
    };
    tauri::async_runtime::spawn_blocking(move || -> anyhow::Result<ShareResult> {
        let mut path = PathBuf::from(&source.path);
        let mut compressed = false;
        let size_mb = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0) as f64 / 1_048_576.0;
        let cleanup;
        if size_mb > share::DISCORD_LIMIT_MB - 0.3 && source.kind != "screenshot" {
            let out = editor::compress(&path, share::DISCORD_LIMIT_MB - 0.6, &progress_fn(&app, "compress"))?;
            compressed = true;
            cleanup = Some(out.clone());
            path = out;
        } else {
            cleanup = None;
        }
        let _ = app.emit("edit-progress", Progress { op: "upload".into(), value: 0.5 });
        let result = share::send_webhook(&url, &path, &message);
        if let Some(p) = cleanup {
            let _ = std::fs::remove_file(p);
        }
        let _ = app.emit("edit-progress", Progress { op: "upload".into(), value: 1.0 });
        Ok(ShareResult { url: result?, compressed })
    })
    .await
    .map_err(err)?
    .map_err(err)
}

#[derive(Serialize)]
struct Hardware {
    monitors: Vec<util::Monitor>,
    outputs: Vec<audio::Device>,
    inputs: Vec<audio::Device>,
    encoders: Vec<String>,
    apps: Vec<String>,
    ffmpeg: String,
    /// Mémoire vive du PC (Go), pour le choix « tampon en RAM »
    ram_gb: f64,
}

#[tauri::command]
async fn get_hardware() -> Hardware {
    tauri::async_runtime::spawn_blocking(|| {
        let mut apps: Vec<String> = util::processes()
            .into_iter()
            .map(|p| p.exe)
            .filter(|e| {
                let l = e.to_ascii_lowercase();
                l.ends_with(".exe")
                    && !matches!(
                        l.as_str(),
                        "svchost.exe" | "conhost.exe" | "csrss.exe" | "dwm.exe" | "lsass.exe" | "services.exe"
                            | "smss.exe" | "wininit.exe" | "winlogon.exe" | "fontdrvhost.exe" | "runtimebroker.exe"
                            | "rewind.exe" | "ffmpeg.exe" | "dllhost.exe" | "sihost.exe" | "ctfmon.exe"
                            | "taskhostw.exe" | "searchhost.exe" | "wmiprvse.exe" | "audiodg.exe"
                    )
            })
            .collect();
        apps.sort_by_key(|a| a.to_ascii_lowercase());
        apps.dedup_by(|a, b| a.eq_ignore_ascii_case(b));
        Hardware {
            monitors: util::monitors(),
            outputs: audio::devices(false),
            inputs: audio::devices(true),
            encoders: ffmpeg::working_encoders(),
            apps,
            ffmpeg: ffmpeg::tools().map(|t| t.ffmpeg.to_string_lossy().to_string()).unwrap_or_default(),
            ram_gb: ram::memory().map(|m| m.0 as f64 / 1_073_741_824.0).unwrap_or(0.0),
        }
    })
    .await
    .unwrap_or(Hardware { monitors: vec![], outputs: vec![], inputs: vec![], encoders: vec![], apps: vec![], ffmpeg: String::new(), ram_gb: 0.0 })
}

#[derive(Serialize)]
struct Cs2Info {
    installs: Vec<String>,
    configured: Vec<String>,
    /// CS2 est lancé
    running: bool,
    /// CS2 a été lancé avant que le fichier de liaison soit (ré)écrit : il faut le relancer
    needs_restart: bool,
    live: cs2::Cs2Live,
    /// secondes depuis le dernier message (-1 = jamais)
    since_last: f64,
}

#[tauri::command(async)]
fn cs2_info(state: State<AppState>) -> Cs2Info {
    let dirs = cs2::cs2_cfg_dirs();
    let installs: Vec<String> = dirs.iter().map(|p| p.to_string_lossy().to_string()).collect();
    let files: Vec<PathBuf> = dirs.iter().map(|p| p.join("gamestate_integration_rewind.cfg")).filter(|p| p.is_file()).collect();
    let configured = files.iter().map(|p| p.to_string_lossy().to_string()).collect();
    let newest_cfg = files
        .iter()
        .filter_map(|p| std::fs::metadata(p).and_then(|m| m.modified()).ok())
        .filter_map(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs_f64())
        .fold(0.0, f64::max);
    let cs2_pid = util::root_process("cs2.exe");
    let started = cs2_pid.and_then(util::process_start);
    let live = state.cs2_live.lock().as_ref().map(|l| l.lock().clone()).unwrap_or_default();
    let since_last = if live.last_message > 0.0 { util::now() - live.last_message } else { -1.0 };
    let needs_restart = started.is_some_and(|t| newest_cfg > 0.0 && t < newest_cfg - 1.0) && since_last < 0.0;
    Cs2Info { installs, configured, running: cs2_pid.is_some(), needs_restart, live, since_last }
}

#[tauri::command(async)]
fn cs2_install(state: State<AppState>) -> Vec<String> {
    let port = state.settings.lock().cs2.port;
    cs2::install_config(port, &cs2::token())
}

#[tauri::command(async)]
fn engine_log() -> String {
    recorder::last_log()
}

/// Erreurs vues par l'interface (JavaScript, commandes en échec, messages d'erreur affichés) :
/// elles vont dans le journal pour apparaître dans le rapport de diagnostic.
#[tauri::command(async)]
fn log_front(level: String, message: String) {
    static COUNT: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    if COUNT.fetch_add(1, std::sync::atomic::Ordering::Relaxed) > 400 {
        return; // garde-fou : une boucle d'erreurs ne doit pas remplir le journal
    }
    let text: String = message.chars().take(900).collect();
    if level == "error" {
        log::error!("[interface] {text}");
    } else {
        log::warn!("[interface] {text}");
    }
}

#[derive(Serialize)]
struct DiagnosticOut {
    text: String,
    problems: u32,
    /// Chemin du fichier si le rapport a été enregistré
    path: Option<String>,
    copied: bool,
}

/// Rapport de diagnostic complet. `save` : l'enregistre aussi dans un fichier et l'affiche dans l'Explorateur.
#[tauri::command(async)]
async fn diagnostic_report(app: AppHandle, state: State<'_, AppState>, copy: bool, save: bool) -> Res<DiagnosticOut> {
    let settings = state.settings.lock().clone();
    let status = state
        .recorder
        .try_lock_for(Duration::from_millis(300))
        .map(|r| r.status())
        .unwrap_or_else(|| state.last_status.lock().clone());
    let status_text = serde_json::to_string_pretty(&status).unwrap_or_default();
    let cs2_text = serde_json::to_string_pretty(&cs2_info(state.clone())).unwrap_or_default();
    let library = {
        let lib = state.library.lock();
        let total: u64 = lib.clips.iter().map(|c| c.size).sum();
        let missing = lib.clips.iter().filter(|c| !std::path::Path::new(&c.path).is_file()).count();
        let empty = lib.clips.iter().filter(|c| c.size == 0).count();
        let mut kinds: std::collections::BTreeMap<String, usize> = std::collections::BTreeMap::new();
        for c in &lib.clips {
            *kinds.entry(c.kind.clone()).or_default() += 1;
        }
        format!(
            "{} éléments, {:.1} Go ; fichiers introuvables : {missing} ; fichiers vides : {empty} ; par type : {kinds:?}",
            lib.clips.len(),
            total as f64 / 1e9
        )
    };
    let update = {
        let pending = state.pending_update.lock().as_ref().map(|u| u.available.clone());
        let live = match update::check_within(&app, Duration::from_secs(4)).await {
            Ok(u) => format!("canal joignable ; version installée {} ; {}", u.current, u.available.map(|v| format!("version {v} disponible")).unwrap_or_else(|| "à jour".into())),
            Err(e) => format!("canal NON joignable ou refusé : {e}"),
        };
        format!(
            "Version installée : {}\nRecherche automatique : {}\nCanal : {}\nTest du canal à l'instant : {live}\nMise à jour proposée au lancement : {pending:?}",
            app.package_info().version,
            settings.auto_update,
            update::ENDPOINT
        )
    };
    let live = rapport::Live {
        settings,
        status: status_text,
        status_error: status.error.clone(),
        cs2: cs2_text,
        update,
        library,
        webview: tauri::webview_version().unwrap_or_else(|e| format!("illisible ({e})")),
    };
    let report = tauri::async_runtime::spawn_blocking(move || rapport::build(live)).await.map_err(err)?;
    let mut path = None;
    if save {
        let dir = util::data_dir();
        let file = dir.join(format!("rapport-diagnostic-{}.txt", chrono::Local::now().format("%Y%m%d-%H%M%S")));
        std::fs::write(&file, &report.text).map_err(err)?;
        // on garde les 5 derniers rapports
        if let Ok(rd) = std::fs::read_dir(&dir) {
            let mut old: Vec<PathBuf> = rd
                .flatten()
                .map(|e| e.path())
                .filter(|p| p.file_name().is_some_and(|n| n.to_string_lossy().starts_with("rapport-diagnostic-")))
                .collect();
            old.sort();
            for p in old.iter().take(old.len().saturating_sub(5)) {
                let _ = std::fs::remove_file(p);
            }
        }
        let _ = share::reveal(&file);
        path = Some(file.to_string_lossy().to_string());
    }
    let mut copied = false;
    if copy {
        share::copy_text_to_clipboard(&report.text).map_err(err)?;
        copied = true;
    }
    Ok(DiagnosticOut { text: report.text, problems: report.problems, path, copied })
}

/// Medal sur ce PC : dossier, nombre de clips, déjà importés, réglages.
#[tauri::command(async)]
fn medal_detect(folder: Option<String>) -> medal::MedalInfo {
    match folder {
        Some(f) => medal::detect_in(std::path::Path::new(&f)),
        None => medal::detect(),
    }
}

/// Copie les clips Medal pas encore importés dans le dossier des clips (progression : « edit-progress », op « medal »).
#[tauri::command(async)]
fn medal_import(app: AppHandle, state: State<AppState>, from: Option<String>) -> Res<medal::ImportResult> {
    let (folder, per_game) = {
        let s = state.settings.lock();
        (PathBuf::from(&s.clip_folder), s.folder_per_game)
    };
    let res = medal::import(&folder, per_game, from.as_deref().map(std::path::Path::new), &|v| {
        let _ = app.emit("edit-progress", Progress { op: "medal".into(), value: v });
    })
    .map_err(err)?;
    {
        let mut lib = state.library.lock();
        lib.scan(&folder);
        lib.sort();
        let _ = lib.save();
    }
    let _ = app.emit("library-changed", ());
    // Miniatures et durées des clips copiés, sans bloquer
    let handle = app.clone();
    std::thread::spawn(move || {
        let state = handle.state::<AppState>();
        // Copie : la bibliothèque n'est pas bloquée pendant le calcul des miniatures
        let mut copy = Library { clips: state.library.lock().clips.clone() };
        if copy.fill_missing() {
            let mut l = state.library.lock();
            for c in copy.clips {
                if let Some(x) = l.get_mut(&c.id) {
                    x.duration = c.duration;
                    x.thumb = c.thumb;
                }
            }
            let _ = l.save();
            drop(l);
            let _ = handle.emit("library-changed", ());
        }
    });
    Ok(res)
}

#[tauri::command(async)]
fn refresh_library(state: State<AppState>) -> Vec<Clip> {
    let folder = PathBuf::from(&state.settings.lock().clip_folder);
    let mut lib = state.library.lock();
    lib.clips.retain(|c| std::path::Path::new(&c.path).is_file());
    lib.scan(&folder);
    lib.sort();
    let _ = lib.save();
    lib.clips.clone()
}

/// Fenêtre de mise à jour : ce qui a été trouvé au lancement.
#[tauri::command(async)]
fn pending_update(state: State<AppState>) -> Option<update::UpdateInfo> {
    state.pending_update.lock().clone()
}

/// Fenêtre de mise à jour : « Plus tard » → on ouvre Rewind normalement.
#[tauri::command(async)]
fn update_later(app: AppHandle) {
    log::info!("mise à jour remise à plus tard");
    // Au lancement : la fenêtre principale d'abord (sans aucune fenêtre ouverte, l'appli pourrait
    // se fermer). Pendant l'utilisation : on ferme seulement la petite fenêtre, sans rien ouvrir.
    if app.state::<AppState>().update_at_launch.load(std::sync::atomic::Ordering::Relaxed) {
        show_main(&app);
    }
    if let Some(w) = app.get_webview_window("update") {
        let _ = w.close();
    }
}

/// Version déjà proposée dans la fenêtre (une seule proposition par version).
fn last_proposed_update() -> String {
    std::fs::read_to_string(util::data_dir().join("update-proposed.txt")).unwrap_or_default().trim().to_string()
}

fn mark_proposed_update(version: &str) {
    let _ = std::fs::write(util::data_dir().join("update-proposed.txt"), version);
}

/// L'utilisateur est occupé : jeu ou plein écran au premier plan, clip sur mesure ou session en cours.
fn user_busy(app: &AppHandle) -> bool {
    let state = app.state::<AppState>();
    util::foreground().is_some_and(|f| f.fullscreen || util::is_game(&f.exe, f.fullscreen))
        || state.custom.lock().is_some()
        || state.session.lock().is_some()
}

/// Propose la fenêtre de mise à jour pendant l'utilisation, une seule fois par version, au premier
/// moment calme : aucun jeu ni plein écran au premier plan, aucun clip sur mesure ni session en cours.
/// Sinon on réessaie toutes les 20 s pendant 6 h ; le bouton de la barre de titre reste disponible.
fn propose_update_when_idle(app: AppHandle, info: update::UpdateInfo) {
    use std::sync::atomic::{AtomicBool, Ordering};
    static WAITING: AtomicBool = AtomicBool::new(false);
    let Some(version) = info.available.clone() else { return };
    if last_proposed_update() == version || WAITING.swap(true, Ordering::SeqCst) {
        return;
    }
    tauri::async_runtime::spawn(async move {
        for _ in 0..(6 * 60 * 3) {
            tokio_sleep(20).await;
            let state = app.state::<AppState>();
            if !state.settings.lock().auto_update || last_proposed_update() == version {
                break;
            }
            if user_busy(&app) || app.get_webview_window("update").is_some() {
                continue;
            }
            *state.pending_update.lock() = Some(info.clone());
            state.update_at_launch.store(false, Ordering::Relaxed);
            mark_proposed_update(&version);
            log::info!("mise à jour proposée pendant l'utilisation : {version}");
            show_update_window(&app);
            break;
        }
        WAITING.store(false, Ordering::SeqCst);
    });
}

/// Petite fenêtre centrée, dans le style de l'appli. Renvoie false si elle n'a pas pu s'ouvrir.
fn show_update_window(app: &AppHandle) -> bool {
    if let Some(w) = app.get_webview_window("update") {
        let _ = w.show();
        let _ = w.set_focus();
        return true;
    }
    let built = tauri::WebviewWindowBuilder::new(app, "update", tauri::WebviewUrl::App("update".into()))
        .title("Rewind")
        .inner_size(460.0, 400.0)
        .resizable(false)
        .maximizable(false)
        .minimizable(false)
        .decorations(false)
        .shadow(true)
        .center()
        .background_color(tauri::window::Color(10, 10, 11, 255))
        .focused(true)
        .build();
    match built {
        Ok(_) => true,
        Err(e) => {
            log::warn!("fenêtre de mise à jour impossible : {e}");
            false
        }
    }
}

#[tauri::command(async)]
fn quit_app(app: AppHandle) {
    app.exit(0);
}

// ---------------------------------------------------------------------------
// Démarrage
// ---------------------------------------------------------------------------

/// Affiche la fenêtre ; la recrée si elle a été fermée (l'interface est détruite à la fermeture
/// pour libérer ~350 Mo de mémoire pendant que l'enregistrement continue).
fn show_main(app: &AppHandle) {
    let window = match app.get_webview_window("main") {
        Some(w) => Some(w),
        None => app
            .config()
            .app
            .windows
            .iter()
            .find(|w| w.label == "main")
            .and_then(|cfg| tauri::WebviewWindowBuilder::from_config(app, cfg).ok())
            .and_then(|b| b.build().map_err(|e| log::error!("fenêtre principale impossible à recréer : {e}")).ok()),
    };
    if let Some(w) = window {
        let _ = w.unminimize();
        let _ = w.show();
        let _ = w.set_focus();
    }
}

fn build_tray(app: &AppHandle) -> tauri::Result<()> {
    // Langue du menu : celle des réglages au démarrage (un changement de langue s'applique au prochain lancement)
    let s = app.state::<AppState>().settings.lock().clone();
    let open = MenuItem::with_id(app, "open", tr(&s, "Ouvrir Rewind", "Open Rewind"), true, None::<&str>)?;
    let clip = MenuItem::with_id(app, "clip", tr(&s, "Clipper maintenant", "Clip now"), true, None::<&str>)?;
    let shot = MenuItem::with_id(app, "shot", tr(&s, "Capture d'écran", "Screenshot"), true, None::<&str>)?;
    let toggle = MenuItem::with_id(app, "toggle", tr(&s, "Pause / reprise de l'enregistrement", "Pause / resume recording"), true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", tr(&s, "Quitter Rewind", "Quit Rewind"), true, None::<&str>)?;
    let sep = PredefinedMenuItem::separator(app)?;
    let sep2 = PredefinedMenuItem::separator(app)?;
    let menu = Menu::with_items(app, &[&open, &sep, &clip, &shot, &toggle, &sep2, &quit])?;
    let mut builder = TrayIconBuilder::with_id("main")
        .tooltip(tr(&s, "Rewind — enregistrement en cours", "Rewind — recording"))
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "open" => show_main(app),
            "clip" => off_ui(app, |app| {
                let s = app.state::<AppState>().settings.lock().clip_seconds;
                clip_last(app, s);
            }),
            "shot" => screenshot(app),
            "toggle" => off_ui(app, toggle_recording),
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = event {
                show_main(tray.app_handle());
            }
        });
    if let Some(icon) = app.default_window_icon() {
        builder = builder.icon(icon.clone());
    }
    builder.build(app)?;
    Ok(())
}

fn init_logging() {
    /// Journal tournant : au-delà de 5 Mo, l'ancien devient rewind.old.log (vérifié aussi en cours de route,
    /// pour une appli qui reste ouverte des semaines).
    struct Logger {
        path: PathBuf,
        file: Mutex<(Option<std::fs::File>, u32)>,
    }
    fn open_log(path: &std::path::Path) -> Option<std::fs::File> {
        if std::fs::metadata(path).map(|m| m.len() > 5_000_000).unwrap_or(false) {
            let _ = std::fs::rename(path, path.with_extension("old.log"));
        }
        std::fs::OpenOptions::new().create(true).append(true).open(path).ok()
    }
    impl log::Log for Logger {
        fn enabled(&self, m: &log::Metadata) -> bool {
            m.level() <= log::Level::Info
        }
        fn log(&self, r: &log::Record) {
            if !self.enabled(r.metadata()) {
                return;
            }
            let line = format!("{} [{}] {}\n", chrono::Local::now().format("%Y-%m-%d %H:%M:%S"), r.level(), r.args());
            let mut guard = self.file.lock();
            guard.1 += 1;
            if guard.1 % 500 == 0 {
                guard.0 = None; // fermé avant de renommer
                guard.0 = open_log(&self.path);
            }
            if let Some(f) = guard.0.as_mut() {
                use std::io::Write;
                let _ = f.write_all(line.as_bytes());
            }
        }
        fn flush(&self) {}
    }
    let path = util::data_dir().join("rewind.log");
    let file = open_log(&path);
    let logger: &'static Logger = Box::leak(Box::new(Logger { path, file: Mutex::new((file, 0)) }));
    let _ = log::set_logger(logger);
    log::set_max_level(log::LevelFilter::Info);
}

/// Autotest sans interface : `rewind.exe --selftest <secondes> <dossier>`.
/// Enregistre pendant N secondes, sauvegarde les 8 dernières, affiche le résultat et quitte.
pub fn selftest(seconds: u64, out: PathBuf) -> i32 {
    init_logging();
    let mut settings = Settings::load();
    settings.recording_enabled = true;
    if let Ok(enc) = std::env::var("REWIND_ENCODER") {
        settings.encoder = enc;
    }
    if std::env::var_os("REWIND_RAM").is_some() {
        settings.buffer_in_ram = true;
    }
    if let Some(fps) = std::env::var("REWIND_FPS").ok().and_then(|v| v.parse().ok()) {
        settings.fps = fps;
    }
    println!("encodeurs utilisables : {:?}", ffmpeg::working_encoders());
    for m in util::monitors() {
        println!("écran {} : {}x{} à {} Hz{}", m.name, m.width, m.height, m.refresh, if m.primary { " (principal)" } else { "" });
    }
    recorder::clean_buffer();
    let mut rec = Recorder::new(settings);
    let start = std::time::Instant::now();
    while start.elapsed() < Duration::from_secs(seconds) {
        rec.tick();
        std::thread::sleep(Duration::from_millis(250));
    }
    let st = rec.status();
    println!(
        "état : enregistre={} tampon={:.1}s {}x{}@{} {} {:.1} Mo erreur='{}'",
        st.recording, st.buffered, st.width, st.height, st.fps, st.encoder, st.buffer_mb, st.error
    );
    for a in &st.audio {
        println!("  audio {} : actif={} niveau={:.3}", a.label, a.active, a.level);
    }
    let now = util::now();
    let wait = std::time::Instant::now();
    while !rec.written_until(now) && wait.elapsed() < Duration::from_secs(4) {
        std::thread::sleep(Duration::from_millis(100));
        rec.tick();
    }
    let clip_len = (seconds as f64 - 4.0).clamp(3.0, 110.0);
    let result = rec.prepare_export(now - clip_len, now).and_then(|p| {
        recorder::export(
            p,
            ExportRequest {
                start: now - clip_len,
                end: now,
                title: "Autotest".into(),
                game: "Test".into(),
                kind: "manual".into(),
                markers: vec![(now - 1.0, "manual".into(), "Test".into())],
                folder: out,
            },
        )
    });
    let code = match result {
        Ok(c) => {
            println!("CLIP OK : {} ({:.2} s, {} octets)", c.path, c.duration, c.size);
            0
        }
        Err(e) => {
            println!("ÉCHEC : {e:#}");
            println!("{}", recorder::last_log());
            1
        }
    };
    drop(rec);
    code
}

/// Test sans interface du Studio : `rewind.exe --studio <clipA> <clipB> <dossier>`.
/// Test sans interface du Studio : `rewind.exe --studio <clipA> <clipB> <dossier> [musique]`.
/// Vérifie la durée exacte (vitesse + transitions), les formats, la musique et la taille maximale.
pub fn studio_test(a: String, b: String, out: PathBuf, music: Option<String>) -> i32 {
    let info_a = media::info(std::path::Path::new(&a)).unwrap_or_default();
    let dur_b = media::info(std::path::Path::new(&b)).map(|i| i.duration).unwrap_or(0.0);
    let tracks = info_a.tracks.len();
    if info_a.duration < 6.0 || dur_b < 7.0 {
        println!("ÉCHEC : il faut des clips d'au moins 6 s (A) et 7 s (B)");
        return 1;
    }
    // Pistes séparées : on coupe la 2e (souvent le micro) pour vérifier le mixage par piste
    let track_volumes: Vec<f64> = if tracks > 1 { (1..tracks).map(|k| if k == 2 { 0.0 } else { 1.0 }).collect() } else { vec![] };
    let mid = (info_a.duration - 3.0).min(5.0);
    let pieces = vec![
        editor::Piece { path: a.clone(), start: 0.5, end: 2.5, speed: 0.5, transition: "fade".into(), transition_dur: 0.5, ..Default::default() },
        editor::Piece { path: a, start: mid, end: mid + 3.0, speed: 1.0, volume: 1.5, track_volumes, transition: "zoom".into(), transition_dur: 0.8, ..Default::default() },
        editor::Piece { path: b, start: 1.0, end: 7.0, speed: 2.0, transition: "none".into(), ..Default::default() },
    ];
    let expected = editor::total_duration(&pieces); // 4 + 3 + 3 - 0,5 - 0,8 = 8,7 s
    let music = music.map(|path| editor::Music { path, volume: 0.6, offset: 3.0, at: 1.0, fade_in: 1.0, fade_out: 2.0, duck: true });
    let cases: Vec<(&str, editor::StudioOptions)> = vec![
        ("16:9", editor::StudioOptions { title: "Test studio 16x9".into(), music: music.clone(), ..Default::default() }),
        ("vertical", editor::StudioOptions { title: "Test studio vertical".into(), format: "9:16-blur".into(), fps: 60, ..Default::default() }),
        ("discord 10 Mo", editor::StudioOptions { title: "Test studio discord".into(), max_mb: 10.0, music, ..Default::default() }),
        // Petite taille : passe en 480p (852×480, dimensions paires) à 30 i/s
        ("2 Mo (480p)", editor::StudioOptions { title: "Test studio 480p".into(), max_mb: 2.0, ..Default::default() }),
    ];
    // Touches incrustées : W de 0,5 s à 1,5 s du clip A, clic gauche à 1,0 s, espace à 10,5 s
    let ev = |t: f64, key: u16, down: bool| inputs::InputEvent { t, key, down };
    inputs::save_events("test-touches", &[ev(0.5, 0x57, true), ev(1.5, 0x57, false), ev(1.0, 0x01, true), ev(1.2, 0x01, false), ev(mid + 0.5, 0x20, true), ev(mid + 0.9, 0x20, false)]);
    let mut keyed = pieces.clone();
    keyed[0].clip_id = "test-touches".into();
    keyed[1].clip_id = "test-touches".into();
    let mut cases = cases;
    cases.push(("touches", editor::StudioOptions { title: "Test studio touches".into(), keys: true, ..Default::default() }));
    let keyed_case = cases.len() - 1;
    let mut code = 0;
    for (n, (name, opts)) in cases.into_iter().enumerate() {
        let t0 = std::time::Instant::now();
        let list = if n == keyed_case { &keyed } else { &pieces };
        match editor::studio_export(list, &opts, &out, &|_| {}) {
            Ok(p) => {
                // Image ET son doivent durer ce qui est attendu (un « OK » sur la durée du fichier seule
                // avait caché une image arrêtée à 3,5 s)
                let streams = ffmpeg::streams(&p);
                let dur = |kind: &str| streams.iter().find(|s| s.kind == kind).map(|s| s.duration).unwrap_or(0.0);
                let (v, a) = (dur("video"), dur("audio"));
                let mb = std::fs::metadata(&p).map(|m| m.len()).unwrap_or(0) as f64 / 1_048_576.0;
                let ok = (v - expected).abs() < 0.15 && (a - expected).abs() < 0.15 && (opts.max_mb <= 0.0 || mb <= opts.max_mb);
                println!(
                    "{} {name} : image {v:.2} s, son {a:.2} s (attendu {expected:.2} s), {mb:.1} Mo, {:.1} s de calcul → {}",
                    if ok { "OK" } else { "ÉCHEC" }, t0.elapsed().as_secs_f64(), p.display()
                );
                if !ok {
                    code = 1;
                }
            }
            Err(e) => {
                println!("ÉCHEC {name} : {e:#}");
                code = 1;
            }
        }
    }
    code
}

/// Vérifie le contenu d'un clip de test : une image, le bon nombre de pistes son, son et image de même longueur.
fn check_clip(path: &std::path::Path, tracks: usize, wanted: f64) -> Result<f64, String> {
    let streams = ffmpeg::streams(path);
    let video: Vec<&ffmpeg::StreamInfo> = streams.iter().filter(|s| s.kind == "video").collect();
    let audio: Vec<&ffmpeg::StreamInfo> = streams.iter().filter(|s| s.kind == "audio").collect();
    if video.len() != 1 {
        return Err(format!("{} piste(s) image au lieu de 1", video.len()));
    }
    if audio.len() != tracks {
        return Err(format!("{} piste(s) son au lieu de {tracks}", audio.len()));
    }
    let v = video[0];
    if v.start.abs() > 0.25 {
        return Err(format!("l'image commence à {:.2} s au lieu de 0", v.start));
    }
    if v.duration < wanted * 0.6 {
        return Err(format!("clip trop court ({:.1} s pour {wanted:.0} s demandées)", v.duration));
    }
    for a in &audio {
        if a.start.abs() > 0.25 {
            return Err(format!("le son commence à {:.2} s au lieu de 0", a.start));
        }
        if (a.start + a.duration - v.start - v.duration).abs() > 0.4 {
            return Err(format!("son ({:.2} s) et image ({:.2} s) ne finissent pas ensemble", a.start + a.duration, v.start + v.duration));
        }
    }
    Ok(v.duration)
}

/// Test réel des touches à l'écran : `rewind.exe --keys`.
pub fn keys_test() -> i32 {
    init_logging();
    inputs::self_test()
}

/// Test de l'export du Studio v2 : `rewind.exe --render <spec.json> <dossier>` (REWIND_DATA_DIR vers un dossier de test).
pub fn render_test(spec: PathBuf, out: PathBuf) -> i32 {
    init_logging();
    let text = match std::fs::read_to_string(&spec) {
        Ok(t) => t,
        Err(e) => {
            println!("spec illisible : {e}");
            return 2;
        }
    };
    let spec: render::RenderSpec = match serde_json::from_str(&text) {
        Ok(s) => s,
        Err(e) => {
            println!("spec invalide : {e}");
            return 2;
        }
    };
    // Pistes séparées pour l'aperçu (media_stems) : vérifiées sur les mêmes fichiers si demandé
    if std::env::var_os("REWIND_TEST_STEMS").is_some() {
        for p in &spec.pieces {
            let t = std::time::Instant::now();
            match media::stems(std::path::Path::new(&p.path)) {
                Ok(list) => println!("pistes séparées ({} ms) : {list:?}", t.elapsed().as_millis()),
                Err(e) => println!("ÉCHEC pistes séparées : {e:#}"),
            }
        }
    }
    let started = std::time::Instant::now();
    match render::render(&spec, &out, &|p| {
        if (p * 100.0) as u32 % 25 == 0 {
            println!("{:.0} %", p * 100.0);
        }
    }) {
        Ok(path) => {
            println!("OK : {} en {:.1} s", path.display(), started.elapsed().as_secs_f64());
            0
        }
        Err(e) => {
            println!("ÉCHEC : {e:#}");
            1
        }
    }
}

/// Test : `rewind.exe --sounds <dossier>` écrit les quatre sons de clip.
pub fn sounds_test(dir: PathBuf) -> i32 {
    let _ = std::fs::create_dir_all(&dir);
    for (k, name) in [("shutter", "1-declic"), ("chime", "2-carillon"), ("rewind", "3-rembobinage"), ("ding", "4-ancien")] {
        let _ = std::fs::copy(clip_sound(k, 80), dir.join(format!("son-{name}.wav")));
    }
    0
}

/// Test : `rewind.exe --medal-import <dossier>`.
pub fn medal_import_test(dest: PathBuf) -> i32 {
    let t = std::time::Instant::now();
    match medal::import(&dest, true, None, &|_| {}) {
        Ok(r) => { println!("{} copiés, {} déjà là, {} en échec ({:.1} s)", r.copied, r.skipped, r.failed, t.elapsed().as_secs_f64()); 0 }
        Err(e) => { println!("ÉCHEC : {e:#}"); 1 }
    }
}

/// Diagnostic : `rewind.exe --medal-detect`.
pub fn medal_detect_test() -> i32 {
    println!("{}", serde_json::to_string_pretty(&medal::detect()).unwrap_or_default());
    0
}

/// Au lancement : passe la main à un Rewind administrateur si possible (voir admin.rs).
pub fn start_elevated() -> bool {
    admin::start_elevated(std::env::args().any(|a| a == "--tray"))
}

/// Au lancement, avant tout : un Rewind tourne déjà → il ouvre sa fenêtre, celui-ci s'arrête.
/// (Sauf s'il est en train de se fermer pour laisser la place à celui-ci : on attend d'abord sa fin.)
pub fn wake_running_instance() -> bool {
    admin::wait_previous();
    admin::wake_running(std::env::args().any(|a| a == "--tray"))
}

/// Assistant d'entrées lancé en administrateur (`rewind.exe --input-helper`, voir admin.rs).
pub fn input_helper() -> i32 {
    inputs::helper_main()
}

#[derive(serde::Serialize)]
struct AdminInput {
    /// Le jeu suivi tourne en administrateur et Rewind non : ses touches sont invisibles sans l'assistant
    game_admin: bool,
    /// Assistant connecté
    helper: bool,
    /// Autorisé (tâche planifiée créée)
    enabled: bool,
    /// Rewind lui-même lancé en administrateur (rien à faire)
    rewind_admin: bool,
}

#[tauri::command(async)]
fn admin_input_status() -> AdminInput {
    AdminInput {
        game_admin: inputs::game_needs_helper(),
        helper: inputs::HELPER_ON.load(std::sync::atomic::Ordering::Relaxed),
        enabled: admin::main_enabled(),
        rewind_admin: util::process_elevated(0) == Some(true),
    }
}

/// Active (une invite Windows) ou retire l'assistant administrateur.
#[tauri::command(async)]
fn admin_input_set(on: bool) -> Res<()> {
    if on { admin::enable_main() } else { admin::disable_main() }.map_err(err)
}

/// Diagnostic des touches reçues et de l'appareil qui les envoie : `rewind.exe --watch-keys <secondes>`.
pub fn watch_keys(seconds: u64) -> i32 {
    inputs::watch(seconds)
}

/// Test de la session complète : `rewind.exe --hold <dossier>` (avec REWIND_DATA_DIR vers un dossier de test).
/// Tampon réduit à 13 s, session tenue 40 s avec un FFmpeg tué au milieu : le fichier doit faire ~40 s
/// (sans la session il ferait au plus 13 s) et les repères doivent être aux bons endroits.
pub fn hold_test(out: PathBuf) -> i32 {
    init_logging();
    let _ = std::fs::create_dir_all(&out);
    let mut settings = Settings::load();
    settings.recording_enabled = true;
    settings.cs2.enabled = false;
    settings.clip_seconds = 5;
    settings.clip_long_seconds = 5;
    settings.buffer_seconds = 10;
    settings.buffer_in_ram = std::env::var_os("REWIND_RAM").is_some();
    recorder::clean_buffer();
    let rec = Arc::new(Mutex::new(Recorder::new(settings)));
    let run = |secs: f64| {
        let s = std::time::Instant::now();
        while s.elapsed().as_secs_f64() < secs {
            rec.lock().tick();
            std::thread::sleep(Duration::from_millis(100));
        }
    };
    run(6.0);
    let start = util::now();
    rec.lock().set_hold(Some(start));
    let mut markers = vec![(start, "manual".to_string(), "Début".to_string())];
    run(15.0);
    markers.push((util::now(), "headshot".into(), "Headshot (test)".into()));
    let _ = rec.lock().inject_fault(false); // panne au milieu de la session
    run(25.0);
    markers.push((util::now() - 2.0, "kill".into(), "Kill (test)".into()));
    let end = util::now();
    let rec2 = Arc::clone(&rec);
    let ticker = std::thread::spawn(move || {
        let s = std::time::Instant::now();
        while s.elapsed() < Duration::from_secs(5) {
            rec2.lock().tick();
            std::thread::sleep(Duration::from_millis(100));
        }
    });
    recorder::wait_written(&rec, end, Duration::from_secs(4));
    let _ = ticker.join();
    let prepared = match rec.lock().prepare_export(start, end) {
        Ok(p) => p,
        Err(e) => {
            println!("ÉCHEC : {e:#}");
            return 1;
        }
    };
    let (parts, tracks) = (prepared.parts(), prepared.track_count());
    rec.lock().set_hold(None);
    match recorder::export(prepared, ExportRequest { start, end, title: "Session test".into(), game: "Test".into(), kind: "session".into(), markers, folder: out }) {
        Ok(c) => {
            println!("clip : {:.1} s attendus ~{:.1} s (sans session : 13 s max), {parts} morceau(x)", c.duration, end - start);
            for m in &c.markers {
                println!("  repère {:>6.2} s : {}", m.time, m.label);
            }
            let check = check_clip(std::path::Path::new(&c.path), tracks, end - start);
            match check {
                Ok(_) if c.duration > (end - start) - 6.0 && c.markers.len() == 3 => {
                    println!("OK : {}", c.path);
                    0
                }
                Ok(_) => {
                    println!("ÉCHEC : durée ou repères incorrects");
                    1
                }
                Err(e) => {
                    println!("ÉCHEC contrôle : {e}");
                    1
                }
            }
        }
        Err(e) => {
            println!("ÉCHEC export : {e:#}");
            1
        }
    }
}

/// Test des infos du Studio : `rewind.exe --media <fichier>` (pistes, vignettes, forme d'onde, temps de calcul).
pub fn media_test(path: String) -> i32 {
    let p = std::path::Path::new(&path);
    let t0 = std::time::Instant::now();
    let info = match media::info(p) {
        Ok(i) => i,
        Err(e) => {
            println!("ÉCHEC infos : {e:#}");
            return 1;
        }
    };
    println!("infos : {:.2} s, {}x{} à {:.0} i/s, pistes {:?} ({} ms)", info.duration, info.width, info.height, info.fps, info.tracks, t0.elapsed().as_millis());
    let t1 = std::time::Instant::now();
    let strip = media::filmstrip(p);
    match &strip {
        Ok(s) => println!("vignettes : {} × {}x{} toutes les {:.2} s → {} ({} ms)", s.frames, s.frame_w, s.frame_h, s.interval, s.sprite, t1.elapsed().as_millis()),
        Err(e) => println!("ÉCHEC vignettes : {e:#}"),
    }
    let t2 = std::time::Instant::now();
    let wave = media::waveform(p);
    match &wave {
        Ok(w) => println!(
            "forme d'onde : {} valeurs ({:.1} s), crête {} ({} ms)",
            w.peaks.len(), w.peaks.len() as f64 / w.rate, w.peaks.iter().max().unwrap_or(&0), t2.elapsed().as_millis()
        ),
        Err(e) => println!("ÉCHEC forme d'onde : {e:#}"),
    }
    let t3 = std::time::Instant::now();
    let _ = media::filmstrip(p);
    let _ = media::waveform(p);
    println!("deuxième appel (cache) : {} ms", t3.elapsed().as_millis());
    if strip.is_ok() && wave.is_ok() { 0 } else { 1 }
}

/// Test rapide du recollage : `rewind.exe --stitch <dossier>` (avec REWIND_DATA_DIR vers un dossier de test).
/// Enregistre 14 s, tue FFmpeg image, attend la reprise + 6 s, fait un clip de 15 s à cheval sur la panne
/// et contrôle le résultat (pistes, débuts à 0, son et image qui finissent ensemble).
pub fn stitch_test(out: PathBuf, audio_fault: bool) -> i32 {
    init_logging();
    let _ = std::fs::create_dir_all(&out);
    let mut settings = Settings::load();
    settings.recording_enabled = true;
    recorder::clean_buffer();
    let rec = Arc::new(Mutex::new(Recorder::new(settings)));
    let t0 = std::time::Instant::now();
    let run = |secs: f64| {
        let s = std::time::Instant::now();
        while s.elapsed().as_secs_f64() < secs {
            rec.lock().tick();
            std::thread::sleep(Duration::from_millis(100));
        }
    };
    run(14.0);
    if std::env::var_os("REWIND_CUT_END").is_some() {
        // Son tué pile au moment du clip (test d'endurance d'1 h : son fini 0,58 s avant l'image).
        // Le clip ne doit pas être recollé (une seule session d'image) et son + image doivent finir ensemble.
        // REWIND_CUT_END=0.6 : le son est tué 0,6 s AVANT la fin du clip (la fin du son n'existe pas)
        let ahead: f64 = std::env::var("REWIND_CUT_END").ok().and_then(|v| v.parse().ok()).unwrap_or(0.0);
        let mut now = util::now();
        let _ = rec.lock().inject_fault(true);
        if ahead > 0.0 {
            std::thread::sleep(Duration::from_secs_f64(ahead));
            now = util::now();
        }
        println!("son tué {ahead:.1} s avant la fin du clip");
        let rec2 = Arc::clone(&rec);
        let ticker = std::thread::spawn(move || {
            let s = std::time::Instant::now();
            while s.elapsed() < Duration::from_secs(5) {
                rec2.lock().tick();
                std::thread::sleep(Duration::from_millis(100));
            }
        });
        let written = recorder::wait_written(&rec, now, Duration::from_millis(4000));
        let _ = ticker.join();
        let prepared = match rec.lock().prepare_export(now - 12.0, now) {
            Ok(p) => p,
            Err(e) => {
                println!("ÉCHEC préparation : {e:#}");
                return 1;
            }
        };
        let tracks = prepared.track_count();
        println!("tampon à jour : {written} ; morceaux : {}", prepared.parts());
        let req = ExportRequest { start: now - 12.0, end: now, title: "Son coupé".into(), game: "Test".into(), kind: "manual".into(), markers: vec![], folder: out };
        return match recorder::export(prepared, req) {
            Ok(c) => {
                for s in ffmpeg::streams(std::path::Path::new(&c.path)) {
                    println!("  {} : début {:.3} s, durée {:.3} s", s.kind, s.start, s.duration);
                }
                match check_clip(std::path::Path::new(&c.path), tracks, 12.0) {
                    Ok(d) => {
                        println!("OK : son et image finissent ensemble ({d:.2} s) : {}", c.path);
                        0
                    }
                    Err(e) => {
                        println!("ÉCHEC contrôle : {e} ({})", c.path);
                        1
                    }
                }
            }
            Err(e) => {
                println!("ÉCHEC export : {e:#}");
                1
            }
        };
    }
    let session = rec.lock().segments.back().map(|s| s.session).unwrap_or(0);
    if !rec.lock().inject_fault(audio_fault) {
        println!("ÉCHEC : rien à couper (enregistrement pas démarré ?)");
        return 1;
    }
    println!("panne simulée à {:.1} s ({})", t0.elapsed().as_secs_f64(), if audio_fault { "son" } else { "image" });
    let s = std::time::Instant::now();
    while rec.lock().segments.back().map(|g| g.session).unwrap_or(0) == session && s.elapsed() < Duration::from_secs(20) {
        rec.lock().tick();
        std::thread::sleep(Duration::from_millis(100));
    }
    println!("reprise en {:.1} s", s.elapsed().as_secs_f64());
    if std::env::var_os("REWIND_DOUBLE").is_some() {
        // Deuxième panne (son) juste après la reprise : un morceau très court au milieu
        run(1.5);
        let session2 = rec.lock().segments.back().map(|s| s.session).unwrap_or(0);
        let _ = rec.lock().inject_fault(true);
        println!("deuxième panne (son)");
        let s2 = std::time::Instant::now();
        while rec.lock().segments.back().map(|g| g.session).unwrap_or(0) == session2 && s2.elapsed() < Duration::from_secs(20) {
            rec.lock().tick();
            std::thread::sleep(Duration::from_millis(100));
        }
        println!("reprise en {:.1} s", s2.elapsed().as_secs_f64());
        run(3.0);
    } else {
        run(6.0);
    }
    let now = util::now();
    let rec2 = Arc::clone(&rec);
    let ticker = std::thread::spawn(move || {
        let s = std::time::Instant::now();
        while s.elapsed() < Duration::from_secs(5) {
            rec2.lock().tick();
            std::thread::sleep(Duration::from_millis(100));
        }
    });
    let written = recorder::wait_written(&rec, now, Duration::from_millis(4000));
    let _ = ticker.join();
    // REWIND_LATE : clip qui commence pile à la relance (le son y arrive après l'image)
    let late_mode = std::env::var_os("REWIND_LATE").is_some();
    let start = if late_mode {
        rec.lock().segments.iter().find(|g| g.session != session).map(|g| g.start).unwrap_or(now - 5.0)
    } else {
        now - 15.0
    };
    let prepared = match rec.lock().prepare_export(start, now) {
        Ok(p) => p,
        Err(e) => {
            println!("ÉCHEC préparation : {e:#}");
            return 1;
        }
    };
    let (parts, tracks) = (prepared.parts(), prepared.track_count());
    println!("tampon à jour : {written} ; morceaux : {parts} ; pistes son : {tracks}");
    let res = recorder::export(prepared, ExportRequest { start, end: now, title: "Recollage".into(), game: "Test".into(), kind: "manual".into(), markers: vec![(now - 3.0, "manual".into(), "repère".into())], folder: out });
    match res {
        Ok(c) => {
            for s in ffmpeg::streams(std::path::Path::new(&c.path)) {
                println!("  {} : début {:.3} s, durée {:.3} s", s.kind, s.start, s.duration);
            }
            println!("repère à {:.2} s (attendu ≈ {:.2} s)", c.markers.first().map(|m| m.time).unwrap_or(-1.0), c.duration - 3.0);
            let wanted = if late_mode { now - start } else if std::env::var_os("REWIND_DOUBLE").is_some() { 7.5 } else { 15.0 };
            match check_clip(std::path::Path::new(&c.path), tracks, wanted) {
                Ok(d) if late_mode => {
                    println!("OK : clip depuis la relance de {d:.2} s, son et image alignés : {}", c.path);
                    0
                }
                Ok(d) if parts > 1 => {
                    println!("OK : clip recollé de {d:.2} s : {}", c.path);
                    0
                }
                Ok(_) => {
                    println!("ÉCHEC : le clip n'a pas été recollé ({parts} morceau)");
                    1
                }
                Err(e) => {
                    println!("ÉCHEC contrôle : {e} ({})", c.path);
                    1
                }
            }
        }
        Err(e) => {
            println!("ÉCHEC export : {e:#}");
            1
        }
    }
}

/// Test d'endurance : `rewind.exe --stress <minutes> <dossier>` (avec REWIND_DATA_DIR vers un dossier de test).
/// Comme l'appli : enregistrement continu, clips assemblés dans un autre fil pendant que la capture continue.
/// Un clip toutes les 20 s, une rafale de 3 clips simultanés toutes les 2 min, FFmpeg tué
/// (image toutes les 3 min, son toutes les 5 min). Chaque clip est contrôlé (pistes, longueurs).
/// Relevés toutes les 30 s : mémoire, processeur, handles, tampon. Rapport : stress.csv + stress-resume.txt.
pub fn stress(minutes: u64, out: PathBuf) -> i32 {
    init_logging();
    let _ = std::fs::create_dir_all(&out);
    let keep = out.join("clips-recolles");
    let mut settings = Settings::load();
    settings.recording_enabled = true;
    settings.buffer_in_ram = std::env::var_os("REWIND_RAM").is_some();
    recorder::clean_buffer();
    let rec = Arc::new(Mutex::new(Recorder::new(settings)));
    let start = std::time::Instant::now();
    let total = Duration::from_secs(minutes * 60);
    let mut csv = String::from(
        "minute;rewind_mo;ffmpeg_mo;rewind_cpu_pct;ffmpeg_cpu_pct;rewind_handles;segments;fichiers_tampon;tampon_mo;clips_ok;clips_ko;recolles;pannes;reprise_max_s\n",
    );
    let (mut ok, mut ko, mut faults, mut stitched) = (0u32, 0u32, 0u32, 0u32);
    let mut worst_recovery = 0.0f64;
    let mut last_clip = std::time::Instant::now();
    let mut last_burst = std::time::Instant::now();
    let mut last_sample = std::time::Instant::now() - Duration::from_secs(60);
    let mut last_video_fault = std::time::Instant::now();
    let mut last_audio_fault = std::time::Instant::now();
    let mut waiting_recovery: Option<(std::time::Instant, usize)> = None;
    let mut max_rewind = 0u64;
    let mut max_ffmpeg = 0u64;
    let mut max_handles = 0u32;
    let mut first_handles = 0u32;
    let mut max_buffer_mb = 0.0f64;
    let mut failures: Vec<String> = Vec::new();
    let mut slowest_export = 0.0f64;
    // CPU : (heure, temps CPU Rewind, temps CPU FFmpeg cumulé des processus vus)
    let cores = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1) as f64;
    let mut cpu_prev: Option<(std::time::Instant, f64, f64)> = None;
    let mut ffmpeg_cpu_done = 0.0f64; // temps CPU des FFmpeg déjà terminés
    let mut ffmpeg_seen: std::collections::HashMap<u32, f64> = std::collections::HashMap::new();
    let (tx, rx) = std::sync::mpsc::channel::<(f64, Result<(f64, usize), String>, f64)>();
    let mut in_flight = 0usize;
    let launch = |rec: &Arc<Mutex<Recorder>>, tx: &std::sync::mpsc::Sender<(f64, Result<(f64, usize), String>, f64)>, at: f64, keep: PathBuf, out: PathBuf| {
        let now = util::now();
        let tx = tx.clone();
        let rec = Arc::clone(rec);
        std::thread::spawn(move || {
            let t0 = std::time::Instant::now();
            // Comme l'appli : on attend que l'image et le son soient écrits, puis on assemble
            recorder::wait_written(&rec, now, Duration::from_millis(4000));
            let prepared = rec.lock().prepare_export(now - 15.0, now);
            let res = prepared.map_err(|e| format!("{e:#}")).and_then(|p| {
                let parts = p.parts();
                let tracks = p.track_count();
                recorder::export(p, ExportRequest { start: now - 15.0, end: now, title: "Stress".into(), game: "Test".into(), kind: "manual".into(), markers: vec![], folder: out.join("clips") })
                    .map_err(|e| format!("{e:#}"))
                    .and_then(|c| {
                        // Clip à cheval sur une panne : il manque forcément les secondes sans enregistrement
                        let checked = check_clip(std::path::Path::new(&c.path), tracks, if parts > 1 { 7.5 } else { 15.0 });
                        // On garde quelques clips recollés pour vérifier à l'œil ; les autres sont effacés
                        if parts > 1 && checked.is_ok() && std::fs::read_dir(&keep).map(|d| d.count()).unwrap_or(0) < 4 {
                            let _ = std::fs::create_dir_all(&keep);
                            let _ = std::fs::rename(&c.path, keep.join(std::path::Path::new(&c.path).file_name().unwrap_or_default()));
                        } else {
                            let _ = std::fs::remove_file(&c.path);
                        }
                        let _ = std::fs::remove_file(&c.thumb);
                        checked.map(|d| (d, parts))
                    })
            });
            let _ = tx.send((at, res, t0.elapsed().as_secs_f64()));
        });
    };
    while start.elapsed() < total || in_flight > 0 {
        if start.elapsed() > total + Duration::from_secs(120) {
            failures.push(format!("{in_flight} export(s) jamais terminé(s)"));
            ko += in_flight as u32;
            break;
        }
        rec.lock().tick();
        // Résultats des exports
        while let Ok((at, res, secs)) = rx.try_recv() {
            in_flight -= 1;
            slowest_export = slowest_export.max(secs);
            match res {
                Ok((_, parts)) => {
                    ok += 1;
                    if parts > 1 {
                        stitched += 1;
                    }
                }
                Err(e) => {
                    ko += 1;
                    failures.push(format!("{at:.0} s : {e}"));
                }
            }
        }
        let running = start.elapsed() < total;
        // Reprise après panne : quand de nouveaux segments arrivent
        if let Some((t, n)) = waiting_recovery {
            let r = rec.lock();
            let cur = r.segments.back().map(|s| s.session as usize).unwrap_or(0);
            if cur > n && r.status().recording {
                let secs = t.elapsed().as_secs_f64();
                worst_recovery = worst_recovery.max(secs);
                log::info!("stress : reprise en {secs:.1} s");
                waiting_recovery = None;
            }
        }
        // Clip toutes les 20 s, rafale de 3 clips simultanés toutes les 2 min
        let at = start.elapsed().as_secs_f64();
        if running && last_clip.elapsed() >= Duration::from_secs(20) && at > 25.0 && waiting_recovery.is_none() {
            last_clip = std::time::Instant::now();
            let n = if last_burst.elapsed() >= Duration::from_secs(120) {
                last_burst = std::time::Instant::now();
                3
            } else {
                1
            };
            for _ in 0..n {
                launch(&rec, &tx, at, keep.clone(), out.clone());
                in_flight += 1;
            }
        }
        // Pannes simulées
        if running && last_video_fault.elapsed() >= Duration::from_secs(180) && waiting_recovery.is_none() {
            last_video_fault = std::time::Instant::now();
            let mut r = rec.lock();
            let session = r.segments.back().map(|s| s.session as usize).unwrap_or(0);
            if r.inject_fault(false) {
                faults += 1;
                log::info!("stress : FFmpeg image tué");
                waiting_recovery = Some((std::time::Instant::now(), session));
            }
        }
        if running && last_audio_fault.elapsed() >= Duration::from_secs(300) && waiting_recovery.is_none() {
            last_audio_fault = std::time::Instant::now();
            let mut r = rec.lock();
            let session = r.segments.back().map(|s| s.session as usize).unwrap_or(0);
            if r.inject_fault(true) {
                faults += 1;
                log::info!("stress : FFmpeg son tué");
                waiting_recovery = Some((std::time::Instant::now(), session));
            }
        }
        // Relevés
        if last_sample.elapsed() >= Duration::from_secs(30) {
            last_sample = std::time::Instant::now();
            let r = rec.lock();
            let me = util::process_memory(std::process::id()).unwrap_or(0);
            let pids = r.ffmpeg_pids();
            let ff: u64 = pids.iter().filter_map(|p| util::process_memory(*p)).sum();
            let files = std::fs::read_dir(util::data_dir().join("buffer"))
                .map(|d| d.flatten().map(|e| std::fs::read_dir(e.path()).map(|x| x.count()).unwrap_or(0)).sum::<usize>())
                .unwrap_or(0);
            let st = r.status();
            let segs = r.segments.len();
            drop(r);
            // Processeur : écart de temps CPU depuis le relevé précédent
            let (my_cpu, handles) = util::process_cpu_handles(std::process::id()).unwrap_or((0.0, 0));
            let alive: Vec<u32> = pids.clone();
            ffmpeg_seen.retain(|pid, last| {
                if alive.contains(pid) {
                    true
                } else {
                    ffmpeg_cpu_done += *last;
                    false
                }
            });
            for pid in &pids {
                if let Some((t, _)) = util::process_cpu_handles(*pid) {
                    ffmpeg_seen.insert(*pid, t);
                }
            }
            let ff_cpu = ffmpeg_cpu_done + ffmpeg_seen.values().sum::<f64>();
            let now_i = std::time::Instant::now();
            let (my_pct, ff_pct) = match cpu_prev {
                Some((t, a, b)) => {
                    let wall = now_i.duration_since(t).as_secs_f64().max(0.001) * cores;
                    (((my_cpu - a) / wall * 100.0).max(0.0), ((ff_cpu - b) / wall * 100.0).max(0.0))
                }
                None => (0.0, 0.0),
            };
            cpu_prev = Some((now_i, my_cpu, ff_cpu));
            if first_handles == 0 && start.elapsed() > Duration::from_secs(60) {
                first_handles = handles;
            }
            max_rewind = max_rewind.max(me);
            max_ffmpeg = max_ffmpeg.max(ff);
            max_handles = max_handles.max(handles);
            max_buffer_mb = max_buffer_mb.max(st.buffer_mb);
            csv.push_str(&format!(
                "{:.1};{};{};{my_pct:.1};{ff_pct:.1};{handles};{segs};{files};{:.0};{ok};{ko};{stitched};{faults};{worst_recovery:.1}\n",
                start.elapsed().as_secs_f64() / 60.0, me / 1_048_576, ff / 1_048_576, st.buffer_mb
            ));
            let _ = std::fs::write(out.join("stress.csv"), &csv);
        }
        std::thread::sleep(Duration::from_millis(250));
    }
    // Fichiers restés dans le tampon après la fin (fuite de fichiers ?)
    let leftovers = std::fs::read_dir(util::data_dir().join("buffer"))
        .map(|d| d.flatten().filter(|e| e.file_name().to_string_lossy().starts_with("export-")).count())
        .unwrap_or(0);
    if leftovers > 0 {
        failures.push(format!("{leftovers} dossier(s) d'export non nettoyé(s)"));
    }
    let summary = format!(
        "Durée : {minutes} min\nClips réussis (contrôlés) : {ok}\nClips ratés : {ko}\nDont clips recollés après panne : {stitched}\nPannes simulées : {faults}\nReprise la plus lente : {worst_recovery:.1} s\nExport le plus lent : {slowest_export:.1} s\nMémoire max Rewind : {} Mo\nMémoire max FFmpeg : {} Mo\nHandles Rewind : {first_handles} à 1 min, {max_handles} au maximum\nTampon max : {max_buffer_mb:.0} Mo\nÉchecs :\n{}\n",
        max_rewind / 1_048_576, max_ffmpeg / 1_048_576, failures.join("\n")
    );
    let _ = std::fs::write(out.join("stress-resume.txt"), &summary);
    println!("{summary}");
    drop(rec);
    if ko == 0 { 0 } else { 1 }
}

/// Rejoue une partie CS2 enregistrée (journal .jsonl) : `rewind.exe --replay-cs2 <fichier>`.
/// Affiche les clips que Rewind aurait créés, avec leurs heures.
pub fn replay_cs2(file: PathBuf) -> i32 {
    let text = std::fs::read_to_string(&file).unwrap_or_default();
    let (tx, rx) = mpsc::channel();
    let (etx, _erx) = mpsc::channel();
    let settings = if std::env::var_os("REWIND_DATA_DIR").is_some() { Settings::load().cs2.effective() } else { settings::Cs2Settings::default() };
    let mut det = cs2::Cs2::new(settings, String::new(), tx, etx);
    let mut last_t: Option<f64> = None;
    let mut n = 0;
    for line in text.lines() {
        let Ok(v) = serde_json::from_str::<serde_json::Value>(line) else { continue };
        let Some(t) = v["t"].as_f64() else { continue };
        if let Some(mut lt) = last_t {
            while lt + 0.2 < t {
                lt += 0.2;
                det.tick_at(lt);
            }
        }
        det.handle_at(&v["body"].to_string(), t);
        last_t = Some(t);
        n += 1;
    }
    if let Some(mut lt) = last_t {
        for _ in 0..400 {
            lt += 0.2;
            det.tick_at(lt);
        }
    }
    println!("{n} messages rejoués");
    for c in rx.try_iter() {
        let at = chrono::DateTime::from_timestamp(c.start as i64, 0).map(|d| d.with_timezone(&chrono::Local).format("%H:%M:%S").to_string()).unwrap_or_default();
        let kinds: Vec<String> = c.markers.iter().map(|m| m.1.clone()).collect();
        println!("CLIP {at}  {:5.1} s  « {} »  {:?}", c.end - c.start, c.title, kinds);
    }
    0
}

/// Test sans interface : `rewind.exe --discord <fichier> <Mo> <dossier>`.
pub fn discord_test(file: PathBuf, max_mb: f64, out: PathBuf) -> i32 {
    match editor::prepare_for_discord(&file, max_mb, &out, &|_| {}) {
        Ok(f) => {
            let mb = std::fs::metadata(&f.path).map(|m| m.len()).unwrap_or(0) as f64 / 1_048_576.0;
            println!("OK : {} ({mb:.1} Mo, {}p, compressé : {})", f.path.display(), f.height, f.compressed);
            0
        }
        Err(e) => {
            println!("REFUS : {e:#}");
            1
        }
    }
}

/// En cas de plantage, écrit un rapport lisible (où, quoi, pile d'appels) dans le dossier de Rewind.
fn install_crash_report() {
    std::panic::set_hook(Box::new(|info| {
        let thread = std::thread::current().name().unwrap_or("?").to_string();
        let bt = std::backtrace::Backtrace::force_capture();
        let text = format!(
            "Rewind {} a planté le {}\nFil : {thread}\n{info}\n\n{bt}\n",
            env!("CARGO_PKG_VERSION"),
            chrono::Local::now().format("%Y-%m-%d %H:%M:%S")
        );
        log::error!("PLANTAGE ({thread}) : {info}");
        let _ = std::fs::write(util::data_dir().join(format!("plantage-{}.txt", chrono::Local::now().format("%Y%m%d-%H%M%S"))), text);
    }));
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    init_logging();
    install_crash_report();
    let settings = Settings::load();
    log::info!("Rewind démarre");
    let recorder = Arc::new(Mutex::new(Recorder::new(settings.clone())));
    let library = Arc::new(Mutex::new(Library::load(&PathBuf::from(&settings.clip_folder))));
    let state = AppState {
        recorder: Arc::clone(&recorder),
        library: Arc::clone(&library),
        settings: Arc::new(Mutex::new(settings.clone())),
        cs2_tx: Mutex::new(None),
        cs2_live: Mutex::new(None),
        custom: Mutex::new(None),
        session: Mutex::new(None),
        last_status: Mutex::new(recorder::Status::default()),
        pending_update: Mutex::new(None),
        update_at_launch: std::sync::atomic::AtomicBool::new(false),
    };
    // --tray : démarrage de Windows ; relancé en administrateur par la tâche, le drapeau passe par un fichier
    let from_autostart = std::env::args().any(|a| a == "--tray") || admin::take_tray_flag();
    #[allow(unused_mut)]
    let mut context = tauri::generate_context!();
    // Tests (debug seulement) : se faire passer pour une version plus ancienne face au vrai canal GitHub
    #[cfg(debug_assertions)]
    if let Ok(v) = std::env::var("REWIND_TEST_CURRENT_VERSION") {
        context.package_info_mut().version = v.parse().expect("version de test invalide");
    }

    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, args, _cwd| {
            // Démarrage de Windows alors que Rewind tourne déjà : on ne montre rien
            if args.iter().any(|a| a == "--tray") {
                return;
            }
            log::info!("nouveau lancement : la fenêtre s'ouvre");
            show_main(app)
        }))
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_autostart::init(tauri_plugin_autostart::MacosLauncher::LaunchAgent, Some(vec!["--tray"])))
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
        .manage(state)
        .setup(move |app| {
            let handle = app.handle().clone();
            // Ici seulement : un 2e lancement s'arrête avant (extension « une seule instance »)
            recorder::clean_buffer();
            // Raccourci cliqué pendant que Rewind tourne (même en administrateur) : la fenêtre s'ouvre
            // (debug : REWIND_TEST_NO_WAKE_FILTER sert de témoin dans le test, comportement d'avant la 0.3.6)
            if !(cfg!(debug_assertions) && std::env::var_os("REWIND_TEST_NO_WAKE_FILTER").is_some()) {
                admin::accept_wake_messages();
            }
            admin::repair_main_task();
            build_tray(&handle)?;
            toast::create(&handle);
            // Moteur d'entrées (Raw Input) : raccourcis toujours, touches à l'écran selon l'option
            let hk = handle.clone();
            inputs::on_hotkey(move |action| run_hotkey(&hk, action));
            let errors = register_hotkeys(&handle, &settings);
            if !errors.is_empty() {
                log::warn!("raccourcis : {errors:?}");
            }
            inputs::set_overlay(settings.input_overlay);
            inputs::start();
            // Jeux lancés en administrateur : l'assistant (s'il a été autorisé) relaie leurs touches
            inputs::serve_helper();
            admin::watch();
            // Partage entre amis : relève de la boîte (seulement si l'utilisateur l'a activé)
            amis::start(handle.clone());
            // Présence Discord (comme Medal ou Spotify), par le canal local de l'application Discord
            discord::start(handle.clone());
            // Nouveau logo : icône des raccourcis déjà créés (Bureau, Démarrer, barre des tâches)
            raccourcis::refresh_icons_once();
            // Démarrage automatique avec Windows : jamais depuis un test ni une version de développement
            // (un essai avait écrit le chemin du dossier de compilation dans le registre de l'utilisateur).
            let real_install = !cfg!(debug_assertions) && std::env::var_os("REWIND_DATA_DIR").is_none();
            if !settings.first_run_done {
                // Premier lancement : on applique le choix par défaut une seule fois
                if settings.launch_at_startup && real_install {
                    let _ = handle.autolaunch().enable();
                }
                let state = handle.state::<AppState>();
                let mut s = state.settings.lock();
                s.first_run_done = true;
                let _ = s.save();
            } else if settings.launch_at_startup && real_install {
                // À chaque lancement : le démarrage automatique doit viser CET exécutable
                // (après une mise à jour ou un déplacement, l'ancien chemin ne marcherait plus).
                if let Err(e) = handle.autolaunch().enable() {
                    log::warn!("démarrage automatique : {e}");
                }
            }
            // Lancement : s'il existe une mise à jour, une petite fenêtre la propose (mettre à jour ou plus
            // tard). Sinon Rewind s'ouvre normalement, ou reste réduit dans la zone de notification si
            // « Démarrage réduit » est actif ou si Windows l'a lancé. L'enregistrement démarre dans tous
            // les cas, sans attendre.
            {
                let minimized = from_autostart || settings.start_minimized;
                let h = handle.clone();
                let check = settings.auto_update;
                tauri::async_runtime::spawn(async move {
                    let found = if check {
                        update::check_within(&h, Duration::from_secs(4)).await.ok().filter(|i| i.available.is_some())
                    } else {
                        None
                    };
                    let Some(info) = found else {
                        if !minimized {
                            show_main(&h);
                        }
                        return;
                    };
                    let state = h.state::<AppState>();
                    // Lancement réduit : « Plus tard » ferme seulement la petite fenêtre, sans ouvrir Rewind
                    state.update_at_launch.store(!minimized, std::sync::atomic::Ordering::Relaxed);
                    if minimized && user_busy(&h) {
                        // Un jeu est au premier plan : on attend un moment calme
                        log::info!("mise à jour trouvée au lancement réduit, proposée plus tard : {:?}", info.available);
                        propose_update_when_idle(h.clone(), info);
                        return;
                    }
                    log::info!("mise à jour proposée au lancement : {:?}", info.available);
                    if let Some(v) = &info.available {
                        mark_proposed_update(v);
                    }
                    *state.pending_update.lock() = Some(info);
                    if !show_update_window(&h) && !minimized {
                        show_main(&h);
                    }
                });
            }

            // Veille de l'interface (journal + image mémoire si la fenêtre se fige)
            watchdog::start(handle.clone());
            // Test de la veille (debug seulement) : fige l'interface N s, 5 s après le démarrage
            #[cfg(debug_assertions)]
            if let Some(secs) = std::env::var("REWIND_TEST_FREEZE_UI").ok().and_then(|s| s.parse::<u64>().ok()) {
                let h = handle.clone();
                std::thread::spawn(move || {
                    std::thread::sleep(Duration::from_secs(5));
                    let _ = h.run_on_main_thread(move || std::thread::sleep(Duration::from_secs(secs)));
                });
            }

            // Surveillance du moteur
            let rec = Arc::clone(&recorder);
            std::thread::Builder::new().name("engine".into()).spawn(move || loop {
                rec.lock().tick();
                std::thread::sleep(Duration::from_millis(250));
            })?;

            // Recherche au démarrage puis toutes les 4 h, même en arrière-plan.
            {
                let hu = handle.clone();
                tauri::async_runtime::spawn(async move {
                    tokio_sleep(40).await;
                    loop {
                        let enabled = hu.state::<AppState>().settings.lock().auto_update;
                        if enabled {
                            match update::check(&hu).await {
                                Ok(info) => {
                                    if info.available.is_some() {
                                        log::info!("mise à jour disponible : {:?}", info.available);
                                    }
                                    let _ = hu.emit("update-available", info.clone());
                                    if info.available.is_some() {
                                        propose_update_when_idle(hu.clone(), info);
                                    }
                                }
                                Err(e) => log::warn!("{e}"),
                            }
                        }
                        tokio_sleep(4 * 60 * 60).await;
                    }
                });
            }

            // Commande vocale « Rewind clip » (option) : même effet que le raccourci du clip court
            let hv = handle.clone();
            voice::start(move || {
                let s = hv.state::<AppState>().settings.lock().clone();
                if hv.state::<AppState>().recorder.lock().status().recording {
                    clip_last(&hv, s.clip_seconds);
                }
            });
            voice::set_mic(&voice_mic(&settings));
            voice::set_enabled(settings.voice_command);

            // Lancement d'un jeu : bandeau de 6 s avec les raccourcis (une fois par lancement)
            let h5 = handle.clone();
            std::thread::Builder::new().name("jeux".into()).spawn(move || {
                let mut announced: Vec<(u32, f64)> = Vec::new();
                let mut was_game = false;
                loop {
                    std::thread::sleep(Duration::from_millis(500));
                    let fg = util::foreground();
                    let game = fg.as_ref().is_some_and(|f| util::is_game(&f.exe, f.fullscreen));
                    // Les touches ne sont notées que quand un jeu est devant (jamais du texte ailleurs)
                    inputs::set_game(if game { fg.as_ref().map_or(0, |f| f.pid) } else { 0 });
                    // Présence Discord : dernier jeu vu au premier plan
                    if game && let Some(f) = fg.as_ref() {
                        discord::set_game(&f.exe, f.pid);
                    }
                    // Retour au jeu : l'interface met en pause le clip qu'on regardait
                    if game && !was_game {
                        let _ = h5.emit("game-front", ());
                    }
                    was_game = game;
                    let Some(fg) = fg else { continue };
                    if !game {
                        continue;
                    }
                    let start = util::process_start(fg.pid).unwrap_or(0.0);
                    if announced.iter().any(|a| a.0 == fg.pid && (a.1 - start).abs() < 1.0) {
                        continue;
                    }
                    announced.push((fg.pid, start));
                    if announced.len() > 64 {
                        announced.remove(0);
                    }
                    let settings = h5.state::<AppState>().settings.lock().clone();
                    if !settings.welcome_banner || !settings.notify_overlay {
                        continue;
                    }
                    // Laisse le jeu s'afficher (chargement, passage en plein écran)
                    std::thread::sleep(Duration::from_secs(3));
                    log::info!("jeu détecté : {} (plein écran : {})", fg.exe, fg.fullscreen);
                    welcome(&h5, &settings, &util::friendly_app(&fg.exe));
                }
            })?;

            // Miniatures / durées des clips importés
            let lib = Arc::clone(&library);
            let h2 = handle.clone();
            std::thread::spawn(move || {
                let changed = {
                    // copie pour ne pas bloquer la bibliothèque pendant le calcul
                    let mut copy = Library { clips: lib.lock().clips.clone() };
                    let changed = copy.fill_missing();
                    if changed {
                        let mut l = lib.lock();
                        for c in copy.clips {
                            if let Some(x) = l.get_mut(&c.id) {
                                x.duration = c.duration;
                                x.thumb = c.thumb;
                            }
                        }
                        let _ = l.save();
                    }
                    changed
                };
                if changed {
                    let _ = h2.emit("library-changed", ());
                }
            });

            // Counter-Strike 2
            let (clip_tx, clip_rx) = mpsc::channel::<cs2::AutoClip>();
            let (ev_tx, ev_rx) = mpsc::channel::<cs2::Cs2Event>();
            let (set_tx, set_rx) = mpsc::channel::<settings::Cs2Settings>();
            *handle.state::<AppState>().cs2_tx.lock() = Some(set_tx);
            if settings.cs2.enabled {
                let files = cs2::install_config(settings.cs2.port, &cs2::token());
                log::info!("GSI CS2 installé dans {} dossier(s)", files.len());
            }
            let detector = cs2::Cs2::new(settings.cs2.effective(), cs2::token(), clip_tx, ev_tx);
            *handle.state::<AppState>().cs2_live.lock() = Some(Arc::clone(&detector.live));
            // Diagnostic : on garde le dernier message reçu de CS2 dans un fichier
            let live = Arc::clone(&detector.live);
            std::thread::spawn(move || {
                let mut last = 0u64;
                loop {
                    std::thread::sleep(Duration::from_secs(5));
                    let (n, raw) = {
                        let l = live.lock();
                        (l.messages, l.last_raw.clone())
                    };
                    if n != last && !raw.is_empty() {
                        last = n;
                        let _ = std::fs::write(util::data_dir().join("cs2-dernier-message.json"), raw);
                    }
                }
            });
            cs2::serve(settings.cs2.port, detector, set_rx);
            let h3 = handle.clone();
            std::thread::spawn(move || {
                for ev in ev_rx {
                    // Kills pendant une session complète ou un clip sur mesure : repères dans le fichier
                    if let cs2::Cs2Event::Moment(m) = &ev
                        && cs2::is_kill(&m.kind)
                    {
                        let mut label = cs2::label(&m.kind).to_string();
                        if !m.weapon.is_empty() {
                            label.push_str(&format!(" · {}", m.weapon.to_uppercase()));
                        }
                        add_hold_marker(&h3, m.time, &m.kind, &label);
                    }
                    let _ = h3.emit("cs2-event", ev);
                }
            });
            let h4 = handle.clone();
            std::thread::spawn(move || {
                // Filet de sécurité : si deux clips automatiques se chevauchent, on garde le plus complet
                // (le moins complet part à la Corbeille, ou n'est pas créé).
                let mut recent: Vec<(f64, f64, usize, String)> = Vec::new();
                for clip in clip_rx {
                    // Attendre la fin de la période (post-roll) avant d'assembler
                    let wait = clip.end - util::now();
                    if wait > 0.0 {
                        std::thread::sleep(Duration::from_secs_f64(wait.min(30.0)));
                    }
                    let kills = clip.markers.iter().filter(|m| cs2::is_kill(&m.1)).count();
                    if let Some(pos) = recent.iter().position(|r| clip.start < r.1 - 0.5 && clip.end > r.0 + 0.5) {
                        let old = recent[pos].clone();
                        let better = kills > old.2 || (kills == old.2 && clip.end - clip.start > old.1 - old.0 + 0.5);
                        if !better {
                            log::info!("clip auto ignoré (déjà couvert) : {}", clip.title);
                            continue;
                        }
                        log::info!("clip auto « {} » remplace un clip moins complet", clip.title);
                        if let Some(c) = save_clip_blocking(&h4, clip.start, clip.end, Some(format!("CS2 {}", clip.title)), Some("Counter-Strike 2".into()), "auto", clip.markers) {
                            let _ = h4.state::<AppState>().library.lock().remove(&old.3);
                            let _ = h4.emit("library-changed", ());
                            recent[pos] = (clip.start, clip.end, kills, c.id);
                        }
                        continue;
                    }
                    log::info!("clip auto CS2 : {}", clip.title);
                    if let Some(c) = save_clip_blocking(&h4, clip.start, clip.end, Some(format!("CS2 {}", clip.title)), Some("Counter-Strike 2".into()), "auto", clip.markers) {
                        recent.push((clip.start, clip.end, kills, c.id));
                        if recent.len() > 12 {
                            recent.remove(0);
                        }
                    }
                }
            });
            Ok(())
        })
        .on_window_event(|window, event| {
            // Fermer la fenêtre la détruit (mémoire libérée) ; l'appli reste dans la barre des tâches
            if let WindowEvent::Destroyed = event
                && window.label() == "main"
            {
                log::info!("fenêtre fermée, l'enregistrement continue");
            }
        })
        .invoke_handler(tauri::generate_handler![
            get_settings,
            save_settings,
            hotkeys_pause,
            key_labels,
            voice_last,
            get_status,
            hold_action,
            clip_inputs,
            check_update,
            install_update,
            list_clips,
            clip_now,
            take_screenshot,
            set_recording,
            set_mic_muted,
            update_clip,
            delete_clips,
            reveal_clip,
            open_external,
            open_folder,
            copy_clip,
            copy_for_discord,
            studio_export,
            studio_render,
            projects_list,
            project_load,
            project_save,
            project_delete,
            studio_png,
            voiceover_start,
            voiceover_stop,
            media_info,
            media_filmstrip,
            media_waveform,
            media_stems,
            admin_input_status,
            medal_detect,
            medal_import,
            sound_preview,
            sound_import,
            admin_input_set,
            edit_trim,
            edit_merge,
            edit_vertical,
            share_discord,
            friends_view,
            discord_context,
            friends_enable,
            friends_disable,
            friends_rename,
            friends_add,
            friends_answer,
            friends_remove,
            friends_refresh,
            friends_delete_identity,
            friends_recovery_key,
            friends_new_recovery,
            friends_save_recovery,
            friends_restore,
            friends_set_avatar,
            friends_remove_avatar,
            friends_avatar,
            friends_show_online,
            share_send,
            share_cancel,
            share_dismiss,
            share_report,
            share_download,
            share_thumb,
            get_hardware,
            cs2_info,
            cs2_install,
            engine_log,
            diagnostic_report,
            log_front,
            refresh_library,
            quit_app,
            pending_update,
            update_later,
        ])
        .build(context)
        .expect("Rewind n'a pas pu démarrer")
        .run(|app, event| {
            // Plus de fenêtre ouverte : on reste en arrière-plan si « Fermer = réduire » est actif.
            // Un vrai « Quitter » (menu de l'icône) passe un code et n'est pas bloqué.
            if let tauri::RunEvent::ExitRequested { api, code: None, .. } = event
                && app.state::<AppState>().settings.lock().close_to_tray
            {
                api.prevent_exit();
            }
        });
}
