//! Import des clips de Medal déjà présents sur le PC (2026-11-15).
//!
//! - Dossier de Medal : celui noté dans sa base (`<dossier>\Databases\medal-*.db`, clé « ClipFolder »),
//!   sinon `Vidéos\Medal`. La base peut citer un ancien compte Windows : on retombe alors sur le dossier
//!   par défaut. Elle est seulement LUE (jamais modifiée), octet par octet, sans moteur SQLite.
//! - Clips : `<dossier>\Clips\<jeu>\*.mp4`. Jeu = nom du sous-dossier ; date exacte = fin du nom de fichier
//!   (`MedalTVCounterStrike220261012020926704` = 2026-10-12 02:09:26.704), sinon date du fichier.
//!   Les infos par clip de la base sont dans un format privé de Medal : on ne s'en sert pas.
//! - Import = COPIE (choix de l'utilisateur) vers le dossier des clips de Rewind, nommée comme un clip
//!   Rewind (« Counter-Strike 2 2026-10-12 02h09-26.mp4 »), datée comme l'original. Les fichiers déjà copiés
//!   sont notés (`medal-importes.json`) : un 2e import ne copie que les nouveaux.
//! - Réglages de Medal proposés à l'accueil : raccourci du clip, durée, images/s, définition, débit.

use crate::util;
use anyhow::{Result, bail};
use serde::Serialize;
use std::collections::HashSet;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, Serialize, Default)]
pub struct MedalInfo {
    pub found: bool,
    pub folder: String,
    pub clips: u32,
    pub size_mb: f64,
    /// Déjà copiés dans Rewind
    pub imported: u32,
    /// Réglages lus dans Medal (absents si illisibles)
    pub hotkey: Option<String>,
    pub clip_seconds: Option<u32>,
    pub fps: Option<u32>,
    pub resolution: Option<u32>,
    pub bitrate: Option<u32>,
}

#[derive(Clone, Debug, Serialize, Default)]
pub struct ImportResult {
    pub copied: u32,
    pub skipped: u32,
    pub failed: u32,
}

fn imported_file() -> PathBuf {
    util::data_dir().join("medal-importes.json")
}

fn imported_set() -> HashSet<String> {
    std::fs::read_to_string(imported_file())
        .ok()
        .and_then(|t| serde_json::from_str::<Vec<String>>(&t).ok())
        .unwrap_or_default()
        .into_iter()
        .map(|s| s.to_lowercase())
        .collect()
}

/// Base de Medal (la plus récente) dans un dossier Medal.
fn database(folder: &Path) -> Option<Vec<u8>> {
    let dir = folder.join("Databases");
    let mut dbs: Vec<_> = std::fs::read_dir(dir).ok()?.flatten().filter(|e| e.file_name().to_string_lossy().ends_with(".db")).collect();
    dbs.sort_by_key(|e| e.metadata().and_then(|m| m.modified()).ok());
    std::fs::read(dbs.last()?.path()).ok()
}

/// Valeurs encodées par Medal : chaîne courte = octet (longueur × 16 + 7) puis texte ; longue = 0xC7/0xC8,
/// longueur, texte ; nombre écrit en texte = (longueur × 16 + 3) puis chiffres.
fn token(b: &[u8], i: usize) -> Option<(String, usize)> {
    let tag = *b.get(i)?;
    let (start, len) = match tag {
        0xC7 | 0xC8 => (i + 2, *b.get(i + 1)? as usize),
        t if t & 0x0F == 7 || t & 0x0F == 3 => (i + 1, (t >> 4) as usize),
        _ => return None,
    };
    let s = b.get(start..start + len)?;
    if len == 0 || s.iter().any(|c| *c < 0x20) {
        return None;
    }
    Some((String::from_utf8_lossy(s).to_string(), start + len))
}

fn find(b: &[u8], what: &[u8], from: usize) -> Option<usize> {
    b.get(from..)?.windows(what.len()).position(|w| w == what).map(|p| p + from)
}

/// Valeur qui suit une clé de la table des réglages (la valeur est rangée juste après la clé).
fn value_after(b: &[u8], key: &[u8]) -> Option<String> {
    let mut from = 0;
    while let Some(p) = find(b, key, from) {
        if let Some((v, _)) = token(b, p + key.len()) {
            return Some(v);
        }
        from = p + 1;
    }
    None
}

/// Nom de touche Medal → format des Réglages de Rewind.
fn key_name(k: &str) -> Option<String> {
    let k = k.trim();
    let named = match k.to_ascii_lowercase().as_str() {
        "leftalt" | "rightalt" | "alt" | "lmenu" => "Alt",
        "leftcontrol" | "rightcontrol" | "control" | "ctrl" | "leftctrl" => "Ctrl",
        "leftshift" | "rightshift" | "shift" => "Shift",
        "mouse4" | "xbutton1" | "mousex1" => "Mouse4",
        "mouse5" | "xbutton2" | "mousex2" => "Mouse5",
        _ => "",
    };
    if !named.is_empty() {
        return Some(named.to_string());
    }
    let upper = k.to_ascii_uppercase();
    let fkey = upper.strip_prefix('F').and_then(|n| n.parse::<u8>().ok()).is_some_and(|n| (1..=24).contains(&n));
    if fkey || (upper.len() == 1 && upper.chars().all(|c| c.is_ascii_alphanumeric())) {
        return Some(upper);
    }
    None
}

/// Raccourci et durée du clip principal de Medal (« clip;length=30 … inputs F8 »).
fn medal_hotkey(b: &[u8]) -> (Option<String>, Option<u32>) {
    let Some(p) = find(b, b"clip;length=", 0) else { return (None, None) };
    let digits: String = b[p + 12..].iter().take(4).take_while(|c| c.is_ascii_digit()).map(|c| *c as char).collect();
    let seconds = digits.parse().ok();
    let Some(q) = find(b, b"inputs", p).filter(|q| q - p < 120) else { return (None, seconds) };
    // Une ou plusieurs touches à la suite (éventuellement précédées d'un en-tête de liste)
    let mut keys = Vec::new();
    let mut i = q + 6;
    for _ in 0..6 {
        match token(b, i) {
            Some((k, next)) => match key_name(&k) {
                Some(n) => {
                    keys.push(n);
                    i = next;
                }
                None => break,
            },
            None if keys.is_empty() => i += 1,
            None => break,
        }
    }
    ((!keys.is_empty()).then(|| keys.join("+")), seconds)
}

/// Dossier de Medal sur ce PC (ou None).
fn medal_folder() -> Option<(PathBuf, Option<Vec<u8>>)> {
    let default = util::videos_dir().join("Medal");
    let db = database(&default);
    if let Some(b) = &db
        && let Some(f) = value_after(b, b"ClipFolder")
    {
        let p = PathBuf::from(f.replace("\\\\", "\\"));
        if p.join("Clips").is_dir() {
            return Some((p, db));
        }
    }
    default.join("Clips").is_dir().then_some((default, db))
}

fn clip_files(folder: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![folder.join("Clips")];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else { continue };
        for e in entries.flatten() {
            let p = e.path();
            if p.is_dir() {
                stack.push(p);
            } else if p.extension().is_some_and(|x| x.eq_ignore_ascii_case("mp4") || x.eq_ignore_ascii_case("mkv")) {
                out.push(p);
            }
        }
    }
    out.sort();
    out
}

pub fn detect() -> MedalInfo {
    let Some((folder, db)) = medal_folder() else { return MedalInfo::default() };
    let files = clip_files(&folder);
    let done = imported_set();
    let size: u64 = files.iter().filter_map(|f| f.metadata().ok()).map(|m| m.len()).sum();
    let mut info = MedalInfo {
        found: !files.is_empty(),
        folder: folder.to_string_lossy().to_string(),
        clips: files.len() as u32,
        size_mb: size as f64 / 1_048_576.0,
        imported: files.iter().filter(|f| done.contains(&f.to_string_lossy().to_lowercase())).count() as u32,
        ..Default::default()
    };
    if let Some(b) = db {
        let (hk, secs) = medal_hotkey(&b);
        info.hotkey = hk;
        info.clip_seconds = secs.filter(|s| (5..=600).contains(s));
        info.fps = value_after(&b, b"TargetFPS").and_then(|v| v.parse().ok()).filter(|v| (10..=240).contains(v));
        info.bitrate = value_after(&b, b"Bitrate").and_then(|v| v.parse().ok()).filter(|v| (1..=200).contains(v));
        // « Resolution » apparaît aussi ailleurs : on garde l'occurrence suivie de « height »
        let mut from = 0;
        while let Some(p) = find(&b, b"Resolution", from) {
            if let Some((v, _)) = find(&b, b"height", p).filter(|q| q - p < 40).and_then(|q| token(&b, q + 6))
                && let Ok(h) = v.parse::<u32>()
                && (360..=4320).contains(&h)
            {
                info.resolution = Some(h);
                break;
            }
            from = p + 1;
        }
    }
    info
}

/// Date d'un clip Medal d'après son nom (…AAAAMMJJhhmmssmmm), en secondes locales depuis 1970.
fn date_from_name(stem: &str) -> Option<chrono::NaiveDateTime> {
    let digits: String = stem.chars().rev().take_while(|c| c.is_ascii_digit()).collect::<Vec<_>>().into_iter().rev().collect();
    if digits.len() < 14 {
        return None;
    }
    let d = &digits[digits.len() - 17.min(digits.len())..];
    let d = if d.len() >= 17 { &d[..14] } else { &digits[digits.len() - 14..] };
    chrono::NaiveDateTime::parse_from_str(d, "%Y%m%d%H%M%S").ok()
}

/// Copie les clips Medal pas encore importés vers `dest` (+ sous-dossier par jeu si demandé).
pub fn import(dest: &Path, per_game: bool, progress: &dyn Fn(f64)) -> Result<ImportResult> {
    let Some((folder, _)) = medal_folder() else { bail!("Medal introuvable sur ce PC") };
    let files = clip_files(&folder);
    let mut done = imported_set();
    let mut res = ImportResult::default();
    let n = files.len().max(1) as f64;
    for (i, src) in files.iter().enumerate() {
        progress(i as f64 / n);
        let key = src.to_string_lossy().to_lowercase();
        if done.contains(&key) {
            res.skipped += 1;
            continue;
        }
        let game = src
            .parent()
            .filter(|p| p.file_name().is_some_and(|n| !n.eq_ignore_ascii_case("Clips")))
            .and_then(|p| p.file_name())
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "Medal".into());
        let stem = src.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
        let modified = src.metadata().and_then(|m| m.modified()).ok();
        let when = date_from_name(&stem).or_else(|| {
            modified.map(|t| chrono::DateTime::<chrono::Local>::from(t).naive_local())
        });
        let stamp = when.map(|d| d.format("%Y-%m-%d %Hh%M-%S").to_string()).unwrap_or(stem.clone());
        let dir = if per_game { dest.join(util::safe_name(&game)) } else { dest.to_path_buf() };
        let ext = src.extension().map(|e| e.to_string_lossy().to_string()).unwrap_or_else(|| "mp4".into());
        let _ = std::fs::create_dir_all(&dir);
        let mut out = dir.join(format!("{} {stamp}.{ext}", util::safe_name(&game)));
        let mut k = 2;
        while out.exists() {
            out = dir.join(format!("{} {stamp} ({k}).{ext}", util::safe_name(&game)));
            k += 1;
        }
        // Copie dans un fichier temporaire puis renommage : jamais de clip à moitié copié dans la bibliothèque
        let tmp = out.with_extension(format!("partial.{ext}"));
        match std::fs::copy(src, &tmp).and_then(|_| std::fs::rename(&tmp, &out)) {
            Ok(()) => {
                // Même date que l'original (la bibliothèque trie par date du fichier)
                let date = when
                    .and_then(|d| d.and_local_timezone(chrono::Local).single())
                    .map(std::time::SystemTime::from)
                    .or(modified);
                if let (Some(t), Ok(f)) = (date, std::fs::File::options().write(true).open(&out)) {
                    let _ = f.set_modified(t);
                }
                done.insert(key);
                res.copied += 1;
            }
            Err(e) => {
                let _ = std::fs::remove_file(&tmp);
                log::warn!("import Medal : {} non copié ({e})", src.display());
                res.failed += 1;
            }
        }
        // Liste à jour après chaque fichier (un import interrompu reprend où il en était)
        let list: Vec<&String> = done.iter().collect();
        let _ = std::fs::write(imported_file(), serde_json::to_string(&list).unwrap_or_default());
    }
    progress(1.0);
    log::info!("import Medal : {} copiés, {} déjà là, {} en échec", res.copied, res.skipped, res.failed);
    Ok(res)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lecture_des_reglages_medal() {
        // Extraits réels de la base Medal (table key_values)
        let mut b = Vec::new();
        b.extend(b"Hotkeys\xdb\x01K\xccBgaction\xc7\x0eclip;length=30gdevice\x87keyboardGtype\xb7short_pressginputs'F8\xccH");
        b.extend(b"TargetFPS#60");
        b.extend(b"Bitrate#10");
        b.extend(b"Resolution\xcc\x17WwidthC1920gheightC1080");
        b.extend(b"ClipFolder\xc8\x10D:\\\\Clips\\\\Medal");
        assert_eq!(medal_hotkey(&b), (Some("F8".into()), Some(30)));
        assert_eq!(value_after(&b, b"TargetFPS").as_deref(), Some("60"));
        assert_eq!(value_after(&b, b"Bitrate").as_deref(), Some("10"));
        assert_eq!(value_after(&b, b"ClipFolder").as_deref(), Some("D:\\\\Clips\\\\Medal"));
        let p = find(&b, b"Resolution", 0).unwrap();
        let q = find(&b, b"height", p).unwrap();
        assert_eq!(token(&b, q + 6).map(|t| t.0).as_deref(), Some("1080"));
    }

    #[test]
    fn date_dans_le_nom_du_clip() {
        let d = date_from_name("MedalTVCounterStrike220261012020926704").unwrap();
        assert_eq!(d.format("%Y-%m-%d %H:%M:%S").to_string(), "2026-10-12 02:09:26");
        assert!(date_from_name("clip sans date").is_none());
        assert_eq!(key_name("LeftAlt").as_deref(), Some("Alt"));
        assert_eq!(key_name("f10").as_deref(), Some("F10"));
        assert_eq!(key_name("Bidule"), None);
    }
}
