//! Commande vocale « Rewind clip » (hors ligne, désactivée par défaut).
//!
//! Utilise la reconnaissance vocale intégrée à Windows (System.Speech, aucun service en ligne)
//! dans un petit processus PowerShell à part, avec une grammaire d'une seule phrase : rien
//! d'autre n'est reconnu ni enregistré. Le processus est arrêté avec Rewind (objet « job »).

use crate::util;
use std::io::{BufRead, BufReader};
use std::process::{Child, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::time::{Duration, Instant};

const SCRIPT: &str = include_str!("voice.ps1");

/// 0 = coupée, 1 = démarrage, 2 = à l'écoute, 3 = indisponible (aucun moteur), 4 = erreur
pub static STATE: AtomicU8 = AtomicU8::new(0);
static WANTED: AtomicBool = AtomicBool::new(false);
static STARTED: AtomicBool = AtomicBool::new(false);

fn script_path() -> std::path::PathBuf {
    let p = util::data_dir().join("commande-vocale.ps1");
    // Avec BOM : PowerShell 5.1 lit sinon le fichier en ANSI (accents des commentaires abîmés)
    let content = format!("\u{feff}{SCRIPT}");
    if std::fs::read_to_string(&p).ok().as_deref() != Some(content.as_str()) {
        let _ = std::fs::write(&p, content);
    }
    p
}

fn spawn(extra: &[&str]) -> std::io::Result<Child> {
    let mut cmd = util::command(std::path::Path::new("powershell.exe"));
    cmd.args(["-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-File"])
        .arg(script_path())
        .args(extra)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    cmd.spawn()
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
                let mut child = match spawn(&[]) {
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
                let mut fatal = false;
                loop {
                    if !WANTED.load(Ordering::SeqCst) {
                        break;
                    }
                    match rx.recv_timeout(Duration::from_millis(500)) {
                        Ok(line) => {
                            if let Some(lang) = line.strip_prefix("READY ") {
                                STATE.store(2, Ordering::SeqCst);
                                failures = 0;
                                log::info!("commande vocale à l'écoute (moteur {lang})");
                            } else if line.starts_with("CLIP ") {
                                // Anti-rebond : une seule phrase = un seul clip
                                if last_clip.elapsed() > Duration::from_secs(4) {
                                    last_clip = Instant::now();
                                    log::info!("commande vocale : {line}");
                                    on_clip();
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
                let _ = child.kill();
                let _ = child.wait();
                drop(job);
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
pub fn self_test(dir: &std::path::Path) -> i32 {
    let mut files: Vec<_> = std::fs::read_dir(dir).map(|d| d.flatten().map(|e| e.path()).collect()).unwrap_or_default();
    files.retain(|p| p.extension().is_some_and(|e| e == "wav"));
    files.sort();
    let (mut ok, mut ko) = (0, 0);
    for f in files {
        let name = f.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
        let wav = f.to_string_lossy().to_string();
        let out = spawn(&["-Wav", &wav]).and_then(|c| c.wait_with_output());
        let text = out.map(|o| String::from_utf8_lossy(&o.stdout).to_string()).unwrap_or_default();
        let clipped = text.lines().any(|l| l.starts_with("CLIP "));
        let expected = name.starts_with("oui");
        let good = clipped == expected;
        if good { ok += 1 } else { ko += 1 }
        println!("{} {name} : {}", if good { "OK   " } else { "RATÉ " }, text.lines().filter(|l| !l.starts_with("READY")).collect::<Vec<_>>().join(" | "));
    }
    println!("{ok} bons, {ko} ratés");
    if ko == 0 { 0 } else { 1 }
}
