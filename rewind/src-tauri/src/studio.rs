//! Studio : projets de montage enregistrés, images envoyées par l'interface (textes, touches) et voix off.
//!
//! Projets : un fichier JSON par projet dans `<données>/projects`. Le moteur ne lit que quelques champs
//! (nom, date, vignette, durée) pour la liste : le reste appartient à l'interface (studio/model.ts).

use crate::util;
use anyhow::{Context, Result, bail};
use base64::Engine;
use parking_lot::Mutex;
use serde::Serialize;
use std::io::{Seek, SeekFrom, Write};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

fn dir() -> PathBuf {
    let d = util::data_dir().join("projects");
    let _ = std::fs::create_dir_all(&d);
    d
}

thread_local! {
    /// Tests : dossier de projets à part (sans toucher au dossier de données, partagé par tous les tests)
    static TEST_DIR: std::cell::RefCell<Option<PathBuf>> = const { std::cell::RefCell::new(None) };
}

fn projects_dir() -> PathBuf {
    TEST_DIR.with(|d| d.borrow().clone()).unwrap_or_else(dir)
}

/// Identifiant sûr (lettres et chiffres seulement : jamais de chemin dans un nom de fichier).
fn check_id(id: &str) -> Result<()> {
    if id.is_empty() || id.len() > 64 || !id.chars().all(|c| c.is_ascii_alphanumeric()) {
        bail!("identifiant de projet invalide");
    }
    Ok(())
}

#[derive(Serialize)]
pub struct Meta {
    pub id: String,
    pub name: String,
    pub updated: f64,
    pub duration: f64,
    pub thumb: String,
    pub pieces: usize,
}

/// Liste des projets, le plus récent d'abord.
pub fn list() -> Vec<Meta> {
    let mut out = Vec::new();
    let Ok(entries) = std::fs::read_dir(projects_dir()) else { return out };
    for e in entries.flatten() {
        let path = e.path();
        if path.extension().is_none_or(|x| x != "json") {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(&path) else { continue };
        let Ok(v) = serde_json::from_str::<serde_json::Value>(&text) else { continue };
        let pieces = v["pieces"].as_array().cloned().unwrap_or_default();
        let duration: f64 = pieces
            .iter()
            .map(|p| {
                if p["kind"] == "freeze" {
                    p["hold"].as_f64().unwrap_or(0.0)
                } else {
                    (p["end"].as_f64().unwrap_or(0.0) - p["start"].as_f64().unwrap_or(0.0)).max(0.0) / p["speed"].as_f64().unwrap_or(1.0).clamp(0.25, 4.0)
                }
            })
            .sum();
        out.push(Meta {
            id: v["id"].as_str().unwrap_or_default().to_string(),
            name: v["name"].as_str().unwrap_or("Montage").to_string(),
            updated: v["updated"].as_f64().unwrap_or(0.0),
            duration,
            thumb: pieces.first().and_then(|p| p["thumb"].as_str()).unwrap_or_default().to_string(),
            pieces: pieces.len(),
        });
    }
    out.retain(|m| !m.id.is_empty());
    out.sort_by(|a, b| b.updated.total_cmp(&a.updated));
    out
}

pub fn load(id: &str) -> Result<String> {
    check_id(id)?;
    std::fs::read_to_string(projects_dir().join(format!("{id}.json"))).context("projet introuvable")
}

/// Écriture sûre : fichier temporaire puis renommage (jamais de projet à moitié écrit).
pub fn save(id: &str, json: &str) -> Result<()> {
    check_id(id)?;
    serde_json::from_str::<serde_json::Value>(json).context("projet illisible")?;
    let path = projects_dir().join(format!("{id}.json"));
    let tmp = projects_dir().join(format!("{id}.json.tmp"));
    std::fs::write(&tmp, json)?;
    std::fs::rename(&tmp, &path)?;
    Ok(())
}

pub fn delete(id: &str) -> Result<()> {
    check_id(id)?;
    let path = projects_dir().join(format!("{id}.json"));
    if path.is_file() {
        std::fs::remove_file(path)?;
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Images de l'interface (textes, touches) pour l'export
// ---------------------------------------------------------------------------

pub fn cache_dir() -> PathBuf {
    let d = util::data_dir().join("cache").join("studio");
    let _ = std::fs::create_dir_all(&d);
    d
}

/// Ménage du cache : images de plus d'un jour.
pub fn clean_cache() {
    if let Ok(entries) = std::fs::read_dir(cache_dir()) {
        for e in entries.flatten() {
            let old = e.metadata().and_then(|m| m.modified()).ok().and_then(|t| t.elapsed().ok()).is_some_and(|a| a.as_secs() > 86_400);
            if old {
                let _ = std::fs::remove_file(e.path());
            }
        }
    }
}

/// PNG envoyé en base64 → fichier du cache (même contenu = même fichier).
pub fn save_png(b64: &str) -> Result<String> {
    let bytes = base64::engine::general_purpose::STANDARD.decode(b64.trim()).context("image illisible")?;
    if bytes.len() < 8 || &bytes[1..4] != b"PNG" {
        bail!("ce n'est pas une image PNG");
    }
    // Empreinte simple (FNV-1a) : identifie l'image sans dépendance en plus
    let mut h: u64 = 0xcbf29ce484222325;
    for b in &bytes {
        h ^= *b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    let path = cache_dir().join(format!("{h:016x}.png"));
    if !path.is_file() {
        std::fs::write(&path, &bytes)?;
    }
    Ok(path.to_string_lossy().to_string())
}

// ---------------------------------------------------------------------------
// Voix off : le micro choisi dans Rewind, en WAV 48 kHz mono
// ---------------------------------------------------------------------------

struct Recording {
    stop: Arc<AtomicBool>,
    thread: std::thread::JoinHandle<Result<(PathBuf, f64)>>,
}

static RECORDING: Mutex<Option<Recording>> = Mutex::new(None);

pub fn voiceover_start(device: String) -> Result<()> {
    let mut slot = RECORDING.lock();
    if slot.is_some() {
        bail!("un enregistrement est déjà en cours");
    }
    let folder = util::data_dir().join("voix-off");
    std::fs::create_dir_all(&folder)?;
    let stamp = chrono::Local::now().format("%Y-%m-%d %Hh%M-%S");
    let path = folder.join(format!("Voix off {stamp}.wav"));
    let stop = Arc::new(AtomicBool::new(false));
    let stop2 = Arc::clone(&stop);
    let thread = std::thread::Builder::new().name("voix-off".into()).spawn(move || -> Result<(PathBuf, f64)> {
        const RATE: u32 = 48_000;
        let mut file = std::fs::File::create(&path)?;
        write_wav_header(&mut file, RATE, 0)?;
        let mut bytes: u64 = 0;
        let mut io_err = None;
        crate::audio::mic_stream(&device, RATE, &stop2, |pcm| match file.write_all(pcm) {
            Ok(()) => {
                bytes += pcm.len() as u64;
                true
            }
            Err(e) => {
                io_err = Some(e);
                false
            }
        })?;
        if let Some(e) = io_err {
            return Err(e.into());
        }
        file.seek(SeekFrom::Start(0))?;
        write_wav_header(&mut file, RATE, bytes as u32)?;
        Ok((path, bytes as f64 / (RATE as f64 * 2.0)))
    })?;
    *slot = Some(Recording { stop, thread });
    log::info!("voix off : enregistrement");
    Ok(())
}

#[derive(Serialize)]
pub struct VoiceOver {
    pub path: String,
    pub duration: f64,
}

pub fn voiceover_stop() -> Result<VoiceOver> {
    let rec = RECORDING.lock().take().context("aucun enregistrement en cours")?;
    rec.stop.store(true, Ordering::SeqCst);
    let (path, duration) = rec.thread.join().map_err(|_| anyhow::anyhow!("enregistrement interrompu"))??;
    if duration < 0.2 {
        let _ = std::fs::remove_file(&path);
        bail!("enregistrement trop court");
    }
    log::info!("voix off : {duration:.1} s");
    Ok(VoiceOver { path: path.to_string_lossy().to_string(), duration })
}

fn write_wav_header(f: &mut std::fs::File, rate: u32, data: u32) -> std::io::Result<()> {
    let mut h = Vec::with_capacity(44);
    h.extend_from_slice(b"RIFF");
    h.extend_from_slice(&(36 + data).to_le_bytes());
    h.extend_from_slice(b"WAVEfmt ");
    h.extend_from_slice(&16u32.to_le_bytes());
    h.extend_from_slice(&1u16.to_le_bytes()); // PCM
    h.extend_from_slice(&1u16.to_le_bytes()); // mono
    h.extend_from_slice(&rate.to_le_bytes());
    h.extend_from_slice(&(rate * 2).to_le_bytes());
    h.extend_from_slice(&2u16.to_le_bytes());
    h.extend_from_slice(&16u16.to_le_bytes());
    h.extend_from_slice(b"data");
    h.extend_from_slice(&data.to_le_bytes());
    f.write_all(&h)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn projets_enregistres_et_listes() {
        let dir = std::env::temp_dir().join(format!("rewind-projets-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        TEST_DIR.with(|d| *d.borrow_mut() = Some(dir.clone()));
        let json = r#"{"id":"abc123","name":"Mon montage","updated":10,"pieces":[{"kind":"clip","start":2,"end":8,"speed":2,"thumb":"t.jpg"},{"kind":"freeze","hold":1.5}]}"#;
        save("abc123", json).unwrap();
        assert!(save("../x", json).is_err(), "pas de chemin dans un identifiant");
        assert!(save("abc124", "pas du json").is_err());
        let l = list();
        assert_eq!(l.len(), 1);
        assert_eq!(l[0].name, "Mon montage");
        assert!((l[0].duration - 4.5).abs() < 1e-9, "{}", l[0].duration);
        assert_eq!(load("abc123").unwrap(), json);
        delete("abc123").unwrap();
        assert!(list().is_empty());
        let _ = std::fs::remove_dir_all(dir);
    }
}
