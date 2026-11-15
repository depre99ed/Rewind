//! Import des clips de Medal déjà présents sur le PC (2026-11-15).
//!
//! - Où chercher (tout est additionné, doublons retirés par nom de fichier) :
//!   1. les dossiers notés dans les bases de Medal : `%APPDATA%\Medal\medal-*.db` (+ leur `-wal`, où sont
//!      les changements récents) et `<dossier>\Databases\*.db`, clé « ClipFolder » ;
//!   2. `Vidéos\Medal` (vrai dossier Vidéos de Windows, même déplacé sur D: ou dans OneDrive) ;
//!   3. `X:\Medal`, `X:\Videos\Medal`, `X:\Clips\Medal` sur chaque disque ;
//!   4. les dossiers choisis à la main dans les Réglages (`medal-dossiers.json`).
//!   Un chemin de la base qui cite un ancien compte Windows est ramené au compte actuel.
//!   Les bases sont seulement LUES (jamais modifiées), octet par octet, sans moteur SQLite.
//! - Clips : `<dossier>\Clips\<jeu>\*.mp4` (ou les vidéos du dossier choisi à la main). Jeu = sous-dossier ;
//!   date exacte = fin du nom (`MedalTVCounterStrike220261012020926704` = 2026-10-12 02:09:26.704).
//! - Import = COPIE vers le dossier des clips de Rewind, nommée comme un clip Rewind, datée comme l'original.
//!   Les clips déjà copiés sont notés (`medal-importes.json`, comparés par NOM de fichier : un clip
//!   déplacé par Medal dans un autre dossier n'est pas recopié).
//! - Réglages de Medal proposés à l'accueil : raccourci du clip, durée, images/s, définition, débit.

use crate::util;
use anyhow::{Result, bail};
use serde::Serialize;
use std::collections::HashSet;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, Serialize, Default)]
pub struct MedalInfo {
    pub found: bool,
    /// Medal est (ou a été) installé, même sans clip trouvé
    pub installed: bool,
    pub folder: String,
    /// Tous les dossiers où des clips ont été trouvés
    pub folders: Vec<String>,
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

fn extra_file() -> PathBuf {
    util::data_dir().join("medal-dossiers.json")
}

fn read_list(path: &Path) -> Vec<String> {
    std::fs::read_to_string(path).ok().and_then(|t| serde_json::from_str::<Vec<String>>(&t).ok()).unwrap_or_default()
}

/// Clé d'un clip : son nom de fichier (les noms Medal contiennent la date à la milliseconde).
fn clip_key(p: &str) -> String {
    p.rsplit(['\\', '/']).next().unwrap_or(p).to_lowercase()
}

fn imported_set() -> HashSet<String> {
    read_list(&imported_file()).iter().map(|s| clip_key(s)).collect()
}

/// Dossier choisi à la main : retenu pour les prochaines détections.
fn remember_folder(folder: &Path) {
    let mut list = read_list(&extra_file());
    let s = folder.to_string_lossy().to_string();
    if !list.iter().any(|x| x.eq_ignore_ascii_case(&s)) {
        list.push(s);
        let _ = std::fs::write(extra_file(), serde_json::to_string(&list).unwrap_or_default());
    }
}

/// Bases de Medal (fichier + journal `-wal` devant : les valeurs récentes sont trouvées en premier),
/// de la plus récente à la plus ancienne.
fn databases(folders: &[PathBuf]) -> Vec<Vec<u8>> {
    let mut files: Vec<(std::time::SystemTime, PathBuf)> = Vec::new();
    let mut dirs: Vec<PathBuf> = folders.iter().map(|f| f.join("Databases")).collect();
    if let Some(app) = std::env::var_os("APPDATA") {
        dirs.insert(0, PathBuf::from(app).join("Medal"));
    }
    for dir in dirs {
        let Ok(entries) = std::fs::read_dir(&dir) else { continue };
        for e in entries.flatten() {
            let name = e.file_name().to_string_lossy().to_lowercase();
            if name.starts_with("medal") && name.ends_with(".db") {
                let p = e.path();
                // Date = la plus récente du fichier et de son journal
                let wal = PathBuf::from(format!("{}-wal", p.display()));
                let t = [&p, &wal].iter().filter_map(|f| f.metadata().and_then(|m| m.modified()).ok()).max();
                if let Some(t) = t {
                    files.push((t, p));
                }
            }
        }
    }
    files.sort_by(|a, b| b.0.cmp(&a.0));
    files.dedup_by(|a, b| a.1 == b.1);
    files
        .into_iter()
        .filter_map(|(_, p)| {
            let mut b = std::fs::read(PathBuf::from(format!("{}-wal", p.display()))).unwrap_or_default();
            b.extend(std::fs::read(&p).ok()?);
            Some(b)
        })
        .collect()
}

/// Valeurs encodées par Medal : chaîne courte = octet (longueur × 16 + 7 ou 8) puis texte ; longue =
/// 0xC7/0xC8, longueur, texte ; nombre écrit en texte = (longueur × 16 + 3) puis chiffres.
fn token(b: &[u8], i: usize) -> Option<(String, usize)> {
    let tag = *b.get(i)?;
    let (start, len) = match tag {
        0xC7 | 0xC8 => (i + 2, *b.get(i + 1)? as usize),
        t if matches!(t & 0x0F, 3 | 7 | 8) && t < 0xC0 => (i + 1, (t >> 4) as usize),
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
    values_after(b, key).into_iter().next()
}

fn values_after(b: &[u8], key: &[u8]) -> Vec<String> {
    let mut out = Vec::new();
    let mut from = 0;
    while let Some(p) = find(b, key, from) {
        if let Some((v, _)) = token(b, p + key.len()) {
            out.push(v);
        }
        from = p + 1;
    }
    out
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

/// Vrai dossier Vidéos de Windows (il peut être déplacé sur un autre disque ou dans OneDrive).
fn shell_videos() -> Option<PathBuf> {
    use windows_sys::Win32::System::Registry::{HKEY_CURRENT_USER, RRF_RT_REG_EXPAND_SZ, RRF_RT_REG_SZ, RegGetValueW};
    let wide = |s: &str| s.encode_utf16().chain(Some(0)).collect::<Vec<u16>>();
    let key = wide("Software\\Microsoft\\Windows\\CurrentVersion\\Explorer\\User Shell Folders");
    let name = wide("My Video");
    let mut buf = vec![0u16; 1024];
    let mut size = (buf.len() * 2) as u32;
    // SAFETY : tampon et taille cohérents ; RegGetValueW développe %USERPROFILE%
    let rc = unsafe {
        RegGetValueW(HKEY_CURRENT_USER, key.as_ptr(), name.as_ptr(), RRF_RT_REG_SZ | RRF_RT_REG_EXPAND_SZ, std::ptr::null_mut(), buf.as_mut_ptr().cast(), &mut size)
    };
    if rc != 0 {
        return None;
    }
    let len = buf.iter().position(|c| *c == 0).unwrap_or(0);
    let s = String::from_utf16_lossy(&buf[..len]);
    (!s.is_empty()).then(|| PathBuf::from(s))
}

/// Chemin lu dans la base : `\\` doublés, et peut-être un ancien compte Windows (`C:\Users\ancien\…`).
fn fix_path(raw: &str) -> Vec<PathBuf> {
    let p = raw.replace("\\\\", "\\").trim_end_matches('\\').to_string();
    if p.len() < 3 || !p.chars().nth(1).is_some_and(|c| c == ':') {
        return Vec::new();
    }
    let mut out = vec![PathBuf::from(&p)];
    let lower = p.to_lowercase();
    if let (Some(i), Some(home)) = (lower.find(":\\users\\"), std::env::var_os("USERPROFILE")) {
        let rest = &p[i + 8..];
        if let Some(j) = rest.find('\\') {
            out.push(PathBuf::from(home).join(&rest[j + 1..]));
        }
    }
    out
}

/// Disques internes et USB (C:, D:, …) ; pas les lecteurs réseau ni CD (lents, ou boîte « insérez un disque »).
fn drives() -> Vec<PathBuf> {
    use windows_sys::Win32::Storage::FileSystem::{GetDriveTypeW, GetLogicalDrives};
    // SAFETY : simple lecture du masque des lecteurs
    let mask = unsafe { GetLogicalDrives() };
    (0..26u8)
        .filter(|i| mask & (1 << i) != 0)
        .map(|i| format!("{}:\\", (b'A' + i) as char))
        .filter(|root| {
            let w: Vec<u16> = root.encode_utf16().chain(Some(0)).collect();
            // SAFETY : chaîne terminée par 0 ; 2 = amovible, 3 = disque fixe
            matches!(unsafe { GetDriveTypeW(w.as_ptr()) }, 2 | 3)
        })
        .map(PathBuf::from)
        .collect()
}

fn is_video(p: &Path) -> bool {
    p.extension().is_some_and(|x| ["mp4", "mkv", "mov"].iter().any(|e| x.eq_ignore_ascii_case(e)))
}

/// Vidéos d'un dossier Medal : `Clips\…` s'il existe, sinon le dossier lui-même (choisi à la main).
fn clip_files(folder: &Path) -> Vec<PathBuf> {
    let clips = folder.join("Clips");
    let root = if clips.is_dir() { clips } else { folder.to_path_buf() };
    let mut out = Vec::new();
    let mut stack = vec![(root, 0)];
    while let Some((dir, depth)) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else { continue };
        for e in entries.flatten() {
            let p = e.path();
            if p.is_dir() {
                // Pas les dossiers techniques de Medal (miniatures, montages en cours…)
                let n = e.file_name().to_string_lossy().to_lowercase();
                if depth < 3 && !["temp", "thumbnails", "databases", "edits", "reels", "screenshots"].contains(&n.as_str()) {
                    stack.push((p, depth + 1));
                }
            } else if is_video(&p) && !p.to_string_lossy().contains(".partial.") {
                out.push(p);
            }
        }
    }
    out.sort();
    out
}

/// Tous les dossiers Medal possibles sur ce PC (existants, sans doublon), et les bases lues.
fn medal_folders() -> (Vec<PathBuf>, Vec<Vec<u8>>) {
    let mut cand: Vec<PathBuf> = Vec::new();
    for v in [shell_videos(), Some(util::videos_dir())].into_iter().flatten() {
        cand.push(v.join("Medal"));
    }
    for d in drives() {
        for sub in ["Medal", "Videos\\Medal", "Clips\\Medal", "Vidéos\\Medal"] {
            cand.push(d.join(sub));
        }
    }
    // Bases : celle d'AppData, puis celles rangées dans les dossiers déjà connus
    let dbs = databases(&cand.iter().filter(|p| p.is_dir()).cloned().collect::<Vec<_>>());
    let mut from_db = Vec::new();
    for b in &dbs {
        for v in values_after(b, b"ClipFolder") {
            from_db.extend(fix_path(&v));
        }
    }
    let manual: Vec<PathBuf> = read_list(&extra_file()).into_iter().map(PathBuf::from).collect();
    let mut seen = HashSet::new();
    let folders = from_db
        .into_iter()
        .chain(cand)
        .chain(manual)
        .filter(|p| p.is_dir())
        .filter(|p| seen.insert(p.to_string_lossy().to_lowercase().trim_end_matches('\\').to_string()))
        .collect();
    (folders, dbs)
}

/// Clips trouvés dans tous les dossiers (un même nom de fichier n'est compté qu'une fois).
fn all_clips(folders: &[PathBuf]) -> (Vec<PathBuf>, Vec<String>) {
    let mut seen = HashSet::new();
    let mut files = Vec::new();
    let mut used = Vec::new();
    for f in folders {
        let list: Vec<PathBuf> = clip_files(f).into_iter().filter(|p| seen.insert(clip_key(&p.to_string_lossy()))).collect();
        if !list.is_empty() {
            used.push(f.to_string_lossy().to_string());
            files.extend(list);
        }
    }
    (files, used)
}

fn installed() -> bool {
    let local = std::env::var_os("LOCALAPPDATA").map(|p| PathBuf::from(p).join("Medal").join("Medal.exe"));
    let roaming = std::env::var_os("APPDATA").map(|p| PathBuf::from(p).join("Medal"));
    local.is_some_and(|p| p.is_file()) || roaming.is_some_and(|p| p.is_dir())
}

fn summary(files: &[PathBuf], used: Vec<String>) -> MedalInfo {
    let done = imported_set();
    let size: u64 = files.iter().filter_map(|f| f.metadata().ok()).map(|m| m.len()).sum();
    MedalInfo {
        found: !files.is_empty(),
        installed: installed(),
        folder: used.first().cloned().unwrap_or_default(),
        folders: used,
        clips: files.len() as u32,
        size_mb: size as f64 / 1_048_576.0,
        imported: files.iter().filter(|f| done.contains(&clip_key(&f.to_string_lossy()))).count() as u32,
        ..Default::default()
    }
}

pub fn detect() -> MedalInfo {
    let (folders, dbs) = medal_folders();
    let (files, used) = all_clips(&folders);
    let mut info = summary(&files, used);
    // Réglages : la base la plus récente qui en contient
    for b in &dbs {
        let (hk, secs) = medal_hotkey(b);
        info.hotkey = info.hotkey.take().or(hk);
        info.clip_seconds = info.clip_seconds.or(secs.filter(|s| (5..=600).contains(s)));
        info.fps = info.fps.or(value_after(b, b"TargetFPS").and_then(|v| v.parse().ok()).filter(|v| (10..=240).contains(v)));
        info.bitrate = info.bitrate.or(value_after(b, b"Bitrate").and_then(|v| v.parse().ok()).filter(|v| (1..=200).contains(v)));
        // « Resolution » apparaît aussi ailleurs : on garde l'occurrence suivie de « height »
        let mut from = 0;
        while info.resolution.is_none()
            && let Some(p) = find(b, b"Resolution", from)
        {
            if let Some((v, _)) = find(b, b"height", p).filter(|q| q - p < 40).and_then(|q| token(b, q + 6))
                && let Ok(h) = v.parse::<u32>()
                && (360..=4320).contains(&h)
            {
                info.resolution = Some(h);
            }
            from = p + 1;
        }
        if info.hotkey.is_some() && info.fps.is_some() && info.resolution.is_some() {
            break;
        }
    }
    info
}

/// Un dossier choisi à la main (Medal ou n'importe quel dossier de vidéos).
pub fn detect_in(folder: &Path) -> MedalInfo {
    let files = clip_files(folder);
    let used = if files.is_empty() { Vec::new() } else { vec![folder.to_string_lossy().to_string()] };
    summary(&files, used)
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

/// Copie les clips pas encore importés vers `dest` (+ sous-dossier par jeu si demandé).
/// `from` = un dossier choisi à la main (retenu pour la suite) ; sinon tous les dossiers Medal trouvés.
pub fn import(dest: &Path, per_game: bool, from: Option<&Path>, progress: &dyn Fn(f64)) -> Result<ImportResult> {
    let files = match from {
        Some(f) => {
            // Le dossier des clips de Rewind lui-même : ce serait des doublons
            let lower = |p: &Path| p.to_string_lossy().to_lowercase().trim_end_matches('\\').to_string();
            if lower(f).starts_with(&lower(dest)) || lower(dest).starts_with(&lower(f)) {
                bail!("c'est le dossier des clips de Rewind");
            }
            let files = clip_files(f);
            if !files.is_empty() {
                remember_folder(f);
            }
            files
        }
        None => all_clips(&medal_folders().0).0,
    };
    if files.is_empty() {
        bail!("aucun clip trouvé");
    }
    let mut list = read_list(&imported_file());
    let mut done: HashSet<String> = list.iter().map(|s| clip_key(s)).collect();
    let mut res = ImportResult::default();
    let n = files.len().max(1) as f64;
    for (i, src) in files.iter().enumerate() {
        progress(i as f64 / n);
        let key = clip_key(&src.to_string_lossy());
        if done.contains(&key) {
            res.skipped += 1;
            continue;
        }
        let game = src
            .parent()
            .filter(|p| p.file_name().is_some_and(|n| !n.eq_ignore_ascii_case("Clips")) && from.is_none_or(|f| *p != f))
            .and_then(|p| p.file_name())
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "Medal".into());
        let stem = src.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
        let modified = src.metadata().and_then(|m| m.modified()).ok();
        let when = date_from_name(&stem).or_else(|| modified.map(|t| chrono::DateTime::<chrono::Local>::from(t).naive_local()));
        let stamp = when.map(|d| d.format("%Y-%m-%d %Hh%M-%S").to_string()).unwrap_or(stem.clone());
        let dir = if per_game { dest.join(util::safe_name(&game)) } else { dest.to_path_buf() };
        let ext = src.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_else(|| "mp4".into());
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
                let date = when.and_then(|d| d.and_local_timezone(chrono::Local).single()).map(std::time::SystemTime::from).or(modified);
                if let (Some(t), Ok(f)) = (date, std::fs::File::options().write(true).open(&out)) {
                    let _ = f.set_modified(t);
                }
                done.insert(key);
                list.push(src.to_string_lossy().to_string());
                res.copied += 1;
            }
            Err(e) => {
                let _ = std::fs::remove_file(&tmp);
                log::warn!("import Medal : {} non copié ({e})", src.display());
                res.failed += 1;
            }
        }
        // Liste à jour après chaque fichier (un import interrompu reprend où il en était)
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
        // Base récente (%APPDATA%\Medal) : chaîne courte en « longueur × 16 + 8 »
        b.extend(b"ClipFolder\x98C:\\\\Medal%");
        assert_eq!(medal_hotkey(&b), (Some("F8".into()), Some(30)));
        assert_eq!(value_after(&b, b"TargetFPS").as_deref(), Some("60"));
        assert_eq!(value_after(&b, b"Bitrate").as_deref(), Some("10"));
        assert_eq!(values_after(&b, b"ClipFolder"), vec!["D:\\\\Clips\\\\Medal".to_string(), "C:\\\\Medal".to_string()]);
        assert_eq!(fix_path("C:\\\\Medal"), vec![PathBuf::from("C:\\Medal")]);
        let old = fix_path("C:\\\\Users\\\\depressed\\\\Videos\\\\Medal");
        assert_eq!(old.len(), 2);
        assert!(old[1].ends_with("Videos\\Medal") && !old[1].to_string_lossy().contains("depressed"));
        let p = find(&b, b"Resolution", 0).unwrap();
        let q = find(&b, b"height", p).unwrap();
        assert_eq!(token(&b, q + 6).map(|t| t.0).as_deref(), Some("1080"));
        assert_eq!(clip_key("C:\\Medal\\Clips\\CS2\\MedalTVA.mp4"), "medaltva.mp4");
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
