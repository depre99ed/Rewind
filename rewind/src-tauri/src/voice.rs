//! Commande vocale « Rewind clip » (hors ligne, désactivée par défaut).
//!
//! Utilise la reconnaissance vocale intégrée à Windows (System.Speech, aucun service en ligne)
//! dans un petit processus PowerShell à part, avec une grammaire d'une seule phrase : rien
//! d'autre n'est reconnu ni enregistré. Le processus est arrêté avec Rewind (objet « job »).
//!
//! Depuis le 2026-11-15, Rewind envoie lui-même au script le son du micro CHOISI dans ses réglages
//! (PCM 16 kHz mono par l'entrée standard). Avant, le script écoutait le micro par défaut de Windows,
//! qui pouvait être un autre appareil (micro virtuel Voicemod, webcam…) : la phrase n'était jamais entendue.

use crate::util;
use parking_lot::Mutex;
use serde::Serialize;
use std::io::{BufRead, BufReader, Write};
use std::process::{Child, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::time::{Duration, Instant};

const SCRIPT: &str = include_str!("voice.ps1");

/// 0 = coupée, 1 = démarrage, 2 = à l'écoute, 3 = indisponible (aucun moteur), 4 = erreur
pub static STATE: AtomicU8 = AtomicU8::new(0);
static WANTED: AtomicBool = AtomicBool::new(false);
static STARTED: AtomicBool = AtomicBool::new(false);
/// Micro choisi dans Rewind (identifiant Windows ; vide = micro par défaut)
static MIC: Mutex<String> = Mutex::new(String::new());
/// Vrai quand le micro a changé : on relance l'écoute
static RESTART: AtomicBool = AtomicBool::new(false);
static LAST: Mutex<Option<Heard>> = Mutex::new(None);

/// Dernière fois que la phrase a été entendue (bouton « Tester » des Réglages, et réglage du seuil).
#[derive(Clone, Serialize)]
pub struct Heard {
    pub at: f64,
    pub confidence: f32,
    pub accepted: bool,
}

pub fn last_heard() -> Option<Heard> {
    LAST.lock().clone()
}

fn script_path() -> std::path::PathBuf {
    let p = util::data_dir().join("commande-vocale.ps1");
    // Avec BOM : PowerShell 5.1 lit sinon le fichier en ANSI (accents des commentaires abîmés)
    let content = format!("\u{feff}{SCRIPT}");
    if std::fs::read_to_string(&p).ok().as_deref() != Some(content.as_str()) {
        let _ = std::fs::write(&p, content);
    }
    p
}

fn spawn(extra: &[&str], stdin: bool) -> std::io::Result<Child> {
    let mut cmd = util::command(std::path::Path::new("powershell.exe"));
    cmd.args(["-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-File"])
        .arg(script_path())
        .args(extra)
        .stdin(if stdin { Stdio::piped() } else { Stdio::null() })
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    cmd.spawn()
}

/// Micro à écouter (appelé au démarrage et à chaque enregistrement des réglages).
pub fn set_mic(device: &str) {
    let mut m = MIC.lock();
    if *m != device {
        *m = device.to_string();
        RESTART.store(true, Ordering::SeqCst);
    }
}

/// Lance la surveillance (une seule fois) ; `on_clip` est appelé à chaque « Rewind clip ».
pub fn start(on_clip: impl Fn() + Send + 'static) {
    if STARTED.swap(true, Ordering::SeqCst) {
        return;
    }
    std::thread::Builder::new()
        .name("voix".into())
        .spawn(move || {
            let mut failures = 0u32;
            loop {
                if !WANTED.load(Ordering::SeqCst) {
                    STATE.store(0, Ordering::SeqCst);
                    std::thread::sleep(Duration::from_millis(700));
                    continue;
                }
                STATE.store(1, Ordering::SeqCst);
                RESTART.store(false, Ordering::SeqCst);
                let mut child = match spawn(&["-Stdin", "-Verbose"], true) {
                    Ok(c) => c,
                    Err(e) => {
                        log::warn!("commande vocale : lancement impossible ({e})");
                        STATE.store(4, Ordering::SeqCst);
                        std::thread::sleep(Duration::from_secs(30));
                        continue;
                    }
                };
                let job = util::Job::new();
                if let Some(j) = &job {
                    j.adopt(&child);
                }
                // Le son du micro choisi part dans l'entrée standard du script
                let feed_stop = Arc::new(AtomicBool::new(false));
                if let Some(mut stdin) = child.stdin.take() {
                    let mic = MIC.lock().clone();
                    let stop = Arc::clone(&feed_stop);
                    std::thread::Builder::new()
                        .name("voix-micro".into())
                        .spawn(move || {
                            if let Err(e) = crate::audio::mic_stream(&mic, 16_000, &stop, |pcm| stdin.write_all(pcm).is_ok()) {
                                log::warn!("commande vocale : micro illisible ({e:#})");
                            }
                        })
                        .ok();
                }
                // Lecture des lignes dans un fil : on peut couper à tout moment depuis celui-ci
                let (tx, rx) = std::sync::mpsc::channel::<String>();
                if let Some(out) = child.stdout.take() {
                    std::thread::spawn(move || {
                        for line in BufReader::new(out).lines().map_while(Result::ok) {
                            if tx.send(line).is_err() {
                                break;
                            }
                        }
                    });
                }
                let mut last_clip = Instant::now() - Duration::from_secs(60);
                let mut last_heard_log = Instant::now() - Duration::from_secs(60);
                let mut fatal = false;
                loop {
                    if !WANTED.load(Ordering::SeqCst) || RESTART.load(Ordering::SeqCst) {
                        break;
                    }
                    match rx.recv_timeout(Duration::from_millis(500)) {
                        Ok(line) => {
                            if let Some(lang) = line.strip_prefix("READY ") {
                                STATE.store(2, Ordering::SeqCst);
                                failures = 0;
                                let mic = MIC.lock().clone();
                                log::info!("commande vocale à l'écoute (moteur {lang}, micro {})", if mic.is_empty() { "par défaut" } else { "choisi dans Rewind" });
                            } else if let Some(rest) = line.strip_prefix("CLIP ") {
                                let conf = rest.split_whitespace().next().and_then(|c| c.replace(',', ".").parse().ok()).unwrap_or(0.0);
                                *LAST.lock() = Some(Heard { at: util::now(), confidence: conf, accepted: true });
                                // Un seul clip vocal par 20 s : un clip garde déjà les 30 dernières secondes, et
                                // juste après une vraie commande on parle fort (faux déclenchements vus le 2026-11-15)
                                if last_clip.elapsed() > Duration::from_secs(20) {
                                    last_clip = Instant::now();
                                    log::info!("commande vocale : {line}");
                                    on_clip();
                                } else {
                                    log::info!("commande vocale ignorée (un clip vocal il y a moins de 20 s) : {line}");
                                }
                            } else if let Some(rest) = line.strip_prefix("HEARD ") {
                                // Phrase proche mais sous le seuil : gardée pour le bouton « Tester » et le réglage du seuil
                                let conf = rest.split_whitespace().next().and_then(|c| c.replace(',', ".").parse().ok()).unwrap_or(0.0);
                                *LAST.lock() = Some(Heard { at: util::now(), confidence: conf, accepted: false });
                                if last_heard_log.elapsed() > Duration::from_secs(10) {
                                    last_heard_log = Instant::now();
                                    log::info!("commande vocale : entendue sous le seuil ({rest})");
                                }
                            } else if let Some(msg) = line.strip_prefix("ERR ") {
                                log::warn!("commande vocale : {msg}");
                                STATE.store(if msg.contains("aucun moteur") { 3 } else { 4 }, Ordering::SeqCst);
                                fatal = msg.contains("aucun moteur");
                            }
                        }
                        Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                            if let Ok(Some(_)) = child.try_wait() {
                                break;
                            }
                        }
                        Err(_) => break,
                    }
                }
                feed_stop.store(true, Ordering::SeqCst);
                let _ = child.kill();
                let _ = child.wait();
                drop(job);
                if RESTART.load(Ordering::SeqCst) {
                    continue; // micro changé : relance immédiate
                }
                if WANTED.load(Ordering::SeqCst) {
                    // Arrêt inattendu : on relance, de plus en plus lentement
                    failures += 1;
                    if STATE.load(Ordering::SeqCst) == 2 {
                        STATE.store(4, Ordering::SeqCst);
                    }
                    let wait = if fatal { 300 } else { (5 * failures as u64).min(120) };
                    std::thread::sleep(Duration::from_secs(wait));
                }
            }
        })
        .ok();
}

pub fn set_enabled(on: bool) {
    WANTED.store(on, Ordering::SeqCst);
}

/// Test : `rewind.exe --voice <dossier de .wav>` — chaque fichier « oui-* » doit déclencher, « non-* » jamais.
/// Les fichiers passent par l'entrée standard, comme le micro dans l'appli (WAV 16 kHz mono 16 bits).
pub fn self_test(dir: &std::path::Path) -> i32 {
    let mut files: Vec<_> = std::fs::read_dir(dir).map(|d| d.flatten().map(|e| e.path()).collect()).unwrap_or_default();
    files.retain(|p| p.extension().is_some_and(|e| e == "wav"));
    files.sort();
    let (mut ok, mut ko) = (0, 0);
    for f in files {
        let name = f.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
        let bytes = std::fs::read(&f).unwrap_or_default();
        // En-tête WAV : on cherche le bloc « data »
        let data = bytes.windows(4).position(|w| w == b"data").map(|i| &bytes[(i + 8).min(bytes.len())..]).unwrap_or(&[]);
        // Réglage du seuil : REWIND_VOICE_ARGS="-MinConfidence 0.6 -DictationWeight 0.3"
        let tuning = std::env::var("REWIND_VOICE_ARGS").unwrap_or_default();
        let mut args: Vec<&str> = vec!["-Stdin", "-Verbose"];
        args.extend(tuning.split_whitespace());
        let out = spawn(&args, true).and_then(|mut c| {
            if let Some(mut stdin) = c.stdin.take() {
                let pcm = data.to_vec();
                std::thread::spawn(move || {
                    // Au rythme réel (comme un micro), puis 1,5 s de silence
                    for chunk in pcm.chunks(640) {
                        if stdin.write_all(chunk).is_err() {
                            return;
                        }
                        std::thread::sleep(Duration::from_millis(20));
                    }
                    let _ = stdin.write_all(&vec![0u8; 48_000]);
                });
            }
            let started = Instant::now();
            let mut text = String::new();
            if let Some(out) = c.stdout.take() {
                let (tx, rx) = std::sync::mpsc::channel::<String>();
                std::thread::spawn(move || {
                    for line in BufReader::new(out).lines().map_while(Result::ok) {
                        if tx.send(line).is_err() {
                            break;
                        }
                    }
                });
                let limit = Duration::from_secs_f64(data.len() as f64 / 32_000.0 + 4.0);
                while started.elapsed() < limit {
                    if let Ok(line) = rx.recv_timeout(Duration::from_millis(200)) {
                        text.push_str(&line);
                        text.push('\n');
                        if line.starts_with("CLIP ") {
                            break;
                        }
                    }
                }
            }
            let _ = c.kill();
            let _ = c.wait();
            Ok(text)
        });
        let text = out.unwrap_or_default();
        let clipped = text.lines().any(|l| l.starts_with("CLIP "));
        let expected = name.starts_with("oui");
        let good = clipped == expected;
        if good { ok += 1 } else { ko += 1 }
        println!("{} {name} : {}", if good { "OK   " } else { "RATÉ " }, text.lines().filter(|l| !l.starts_with("READY")).collect::<Vec<_>>().join(" | "));
    }
    println!("{ok} bons, {ko} ratés");
    if ko == 0 { 0 } else { 1 }
}
