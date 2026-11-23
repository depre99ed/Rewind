//! Bibliothèque : index des clips et captures (`%LOCALAPPDATA%\Rewind\library.json`).

use crate::{ffmpeg, util};
use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Marker {
    /// Position dans le clip (secondes)
    pub time: f64,
    /// kill, headshot, onetap, ace, manual…
    pub kind: String,
    pub label: String,
}

impl Default for Marker {
    fn default() -> Self {
        Self { time: 0.0, kind: "manual".into(), label: String::new() }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Default)]
#[serde(default)]
pub struct Clip {
    pub id: String,
    pub path: String,
    pub thumb: String,
    pub title: String,
    pub game: String,
    pub created: f64,
    pub duration: f64,
    pub size: u64,
    /// manual, auto, edit, screenshot
    pub kind: String,
    pub markers: Vec<Marker>,
    pub favorite: bool,
    pub tags: Vec<String>,
}

pub struct Library {
    pub clips: Vec<Clip>,
}

fn index_path() -> PathBuf {
    util::data_dir().join("library.json")
}

pub fn thumbs_dir() -> PathBuf {
    let dir = util::data_dir().join("thumbs");
    let _ = std::fs::create_dir_all(&dir);
    dir
}

pub fn new_id() -> String {
    use std::sync::atomic::{AtomicU32, Ordering};
    static COUNTER: AtomicU32 = AtomicU32::new(0);
    format!("{:x}{:03x}", (util::now() * 1000.0) as u64, COUNTER.fetch_add(1, Ordering::Relaxed) % 4096)
}

/// `c` est-il le clip `path` qui a changé de dossier ? Son ancien fichier n'existe plus, même nom, même taille.
fn moved_here(c: &Clip, path: &Path, size: u64) -> bool {
    let same_name = Path::new(&c.path)
        .file_name()
        .zip(path.file_name())
        .is_some_and(|(a, b)| a.to_string_lossy().eq_ignore_ascii_case(&b.to_string_lossy()));
    same_name && (c.size == 0 || size == 0 || c.size == size) && !Path::new(&c.path).is_file()
}

/// Le disque (ou le partage réseau) qui contient ce chemin est-il là ?
fn drive_available(path: &Path) -> bool {
    use std::path::Component;
    let mut root = PathBuf::new();
    for c in path.components() {
        match c {
            Component::Prefix(_) | Component::RootDir => root.push(c.as_os_str()),
            _ => break,
        }
    }
    root.as_os_str().is_empty() || root.exists()
}

/// Nom de collection propre : espaces réduits, 40 caractères au plus (vide = refusé).
pub fn collection_name(name: &str) -> String {
    name.split_whitespace().collect::<Vec<_>>().join(" ").chars().take(40).collect()
}

impl Library {
    /// Toutes les collections existantes (orthographe d'origine), triées.
    pub fn collections(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        for t in self.clips.iter().flat_map(|c| c.tags.iter()) {
            if !out.iter().any(|o| o.eq_ignore_ascii_case(t)) {
                out.push(t.clone());
            }
        }
        out.sort_by_key(|a| a.to_lowercase());
        out
    }

    /// Ajoute (ou retire) plusieurs clips d'une collection en une fois. « Edit » et « edit » sont la même
    /// collection : l'orthographe déjà utilisée est gardée. Renvoie les clips modifiés (à enregistrer avec `save`).
    pub fn set_collection(&mut self, ids: &[String], name: &str, add: bool) -> Result<Vec<Clip>> {
        let wanted = collection_name(name);
        if wanted.is_empty() {
            bail!("Nom de collection vide");
        }
        let name = self.collections().into_iter().find(|c| c.eq_ignore_ascii_case(&wanted)).unwrap_or(wanted);
        let mut changed = Vec::new();
        for id in ids {
            let Some(c) = self.get_mut(id) else { continue };
            let has = c.tags.iter().any(|t| t.eq_ignore_ascii_case(&name));
            if add && !has {
                c.tags.push(name.clone());
            } else if !add && has {
                c.tags.retain(|t| !t.eq_ignore_ascii_case(&name));
            } else {
                continue;
            }
            changed.push(c.clone());
        }
        Ok(changed)
    }

    /// Renomme une collection sur tous ses clips (fusionne si le nouveau nom existe déjà).
    pub fn rename_collection(&mut self, old: &str, new: &str) -> Result<Vec<Clip>> {
        let new = collection_name(new);
        if new.is_empty() {
            bail!("Nom de collection vide");
        }
        let new = self.collections().into_iter().find(|c| c.eq_ignore_ascii_case(&new) && !c.eq_ignore_ascii_case(old)).unwrap_or(new);
        let mut changed = Vec::new();
        for c in self.clips.iter_mut() {
            if !c.tags.iter().any(|t| t.eq_ignore_ascii_case(old)) {
                continue;
            }
            c.tags.retain(|t| !t.eq_ignore_ascii_case(old) && !t.eq_ignore_ascii_case(&new));
            c.tags.push(new.clone());
            changed.push(c.clone());
        }
        Ok(changed)
    }

    /// Supprime une collection (les clips restent, seule l'étiquette part).
    pub fn delete_collection(&mut self, name: &str) -> Vec<Clip> {
        let mut changed = Vec::new();
        for c in self.clips.iter_mut() {
            if c.tags.iter().any(|t| t.eq_ignore_ascii_case(name)) {
                c.tags.retain(|t| !t.eq_ignore_ascii_case(name));
                changed.push(c.clone());
            }
        }
        changed
    }

    pub fn load(clip_folder: &Path) -> Self {
        let text = std::fs::read_to_string(index_path()).ok();
        let clips: Vec<Clip> = match text.as_deref().map(serde_json::from_str::<Vec<Clip>>) {
            Some(Ok(c)) => c,
            Some(Err(_)) => {
                // Index abîmé : on le garde de côté et on reconstruit à partir du dossier des clips
                let _ = std::fs::rename(index_path(), index_path().with_extension(format!("abime-{}.json", chrono::Local::now().format("%Y%m%d-%H%M%S"))));
                log::warn!("index de la bibliothèque illisible : reconstruit depuis le dossier");
                Vec::new()
            }
            None => Vec::new(),
        };
        let mut lib = Self { clips };
        // D'abord retrouver les clips déplacés (dossier changé, fichiers copiés ailleurs) : ils gardent leur
        // identifiant, leurs repères de kills et leurs touches. Ensuite seulement on oublie les fichiers disparus.
        lib.scan(clip_folder);
        // Un disque absent (clé, disque externe pas encore monté) ne doit pas effacer ses clips de l'index :
        // ils perdraient leurs collections, favoris et repères quand le disque revient.
        lib.clips.retain(|c| Path::new(&c.path).is_file() || !drive_available(Path::new(&c.path)));
        lib.sort();
        let _ = lib.save();
        lib
    }

    /// Ajoute les vidéos/images du dossier qui ne sont pas encore dans l'index (ex. copiées à la main).
    pub fn scan(&mut self, folder: &Path) {
        let mut stack = vec![folder.to_path_buf()];
        let mut depth = 0;
        while let Some(dir) = stack.pop() {
            depth += 1;
            if depth > 200 {
                break;
            }
            let Ok(entries) = std::fs::read_dir(&dir) else { continue };
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    if path.file_name().is_some_and(|n| n != ".rewind-tmp") {
                        stack.push(path);
                    }
                    continue;
                }
                let ext = path.extension().map(|e| e.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
                let is_video = matches!(ext.as_str(), "mp4" | "mkv" | "mov" | "webm");
                let is_image = matches!(ext.as_str(), "png" | "jpg" | "jpeg");
                if !(is_video || is_image) || path.to_string_lossy().contains(".partial.") {
                    continue;
                }
                let p = path.to_string_lossy().to_string();
                if self.clips.iter().any(|c| c.path == p) {
                    continue;
                }
                let meta = entry.metadata().ok();
                // Fichier vide = nom réservé par un export en cours : il sera ajouté une fois fini
                if meta.as_ref().is_some_and(|m| m.len() == 0) {
                    continue;
                }
                // Clip déplacé : même nom de fichier (et même taille si connue) qu'un clip dont le fichier a disparu
                let size = meta.as_ref().map(|m| m.len()).unwrap_or(0);
                if let Some(old) = self.clips.iter_mut().find(|c| moved_here(c, &path, size)) {
                    log::info!("clip retrouvé après déplacement : {} -> {}", old.path, p);
                    old.path = p;
                    continue;
                }
                let created = meta
                    .as_ref()
                    .and_then(|m| m.modified().ok())
                    .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                    .map(|d| d.as_secs_f64())
                    .unwrap_or_else(util::now);
                let game = path
                    .parent()
                    .filter(|p| p != &folder)
                    .and_then(|p| p.file_name())
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_default();
                self.clips.push(Clip {
                    id: new_id(),
                    title: path.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default(),
                    path: p,
                    game,
                    created,
                    size: meta.map(|m| m.len()).unwrap_or(0),
                    // Durée lue tout de suite (sinon le clip s'affichait « 0:00 » jusqu'au prochain démarrage)
                    duration: if is_video { ffmpeg::duration(&path).unwrap_or(0.0) } else { 0.0 },
                    kind: if is_image { "screenshot".into() } else { "manual".into() },
                    ..Default::default()
                });
            }
        }
    }

    pub fn sort(&mut self) {
        self.clips.sort_by(|a, b| b.created.total_cmp(&a.created));
    }

    pub fn save(&self) -> Result<()> {
        let tmp = index_path().with_extension("json.tmp");
        std::fs::write(&tmp, serde_json::to_string(&self.clips)?)?;
        std::fs::rename(tmp, index_path())?;
        Ok(())
    }

    pub fn add(&mut self, clip: Clip) -> Result<()> {
        self.clips.retain(|c| c.id != clip.id);
        self.clips.push(clip);
        self.sort();
        self.save()
    }

    pub fn get(&self, id: &str) -> Option<&Clip> {
        self.clips.iter().find(|c| c.id == id)
    }

    pub fn get_mut(&mut self, id: &str) -> Option<&mut Clip> {
        self.clips.iter_mut().find(|c| c.id == id)
    }

    /// Envoie le fichier à la Corbeille (récupérable) et le retire de l'index.
    pub fn remove(&mut self, id: &str) -> Result<()> {
        let Some(clip) = self.get(id).cloned() else { bail!("Clip introuvable") };
        if Path::new(&clip.path).exists() {
            recycle(Path::new(&clip.path))?;
        }
        if !clip.thumb.is_empty() {
            let _ = std::fs::remove_file(&clip.thumb);
        }
        self.clips.retain(|c| c.id != id);
        self.save()
    }

    /// Renomme le fichier sur le disque en même temps que le titre.
    pub fn rename(&mut self, id: &str, title: &str) -> Result<()> {
        let clip = self.get_mut(id).ok_or_else(|| anyhow::anyhow!("Clip introuvable"))?;
        let old = PathBuf::from(&clip.path);
        let ext = old.extension().map(|e| e.to_string_lossy().to_string()).unwrap_or_else(|| "mp4".into());
        let mut new = old.with_file_name(format!("{}.{ext}", util::safe_name(title)));
        let mut n = 2;
        while new.exists() && new != old {
            new = old.with_file_name(format!("{} ({n}).{ext}", util::safe_name(title)));
            n += 1;
        }
        if new != old {
            std::fs::rename(&old, &new)?;
        }
        clip.path = new.to_string_lossy().to_string();
        clip.title = title.to_string();
        self.save()
    }

    /// Calcule durée et miniature des clips importés qui n'en ont pas.
    pub fn fill_missing(&mut self) -> bool {
        let mut changed = false;
        for clip in self.clips.iter_mut().take(400) {
            let path = PathBuf::from(&clip.path);
            if clip.kind == "screenshot" {
                if clip.thumb.is_empty() {
                    clip.thumb = clip.path.clone();
                    changed = true;
                }
                continue;
            }
            if clip.duration <= 0.0 {
                clip.duration = ffmpeg::duration(&path).unwrap_or(0.0);
                changed = true;
            }
            if clip.thumb.is_empty() || !Path::new(&clip.thumb).is_file() {
                let out = thumbs_dir().join(format!("{}.jpg", clip.id));
                if ffmpeg::thumbnail(&path, (clip.duration * 0.5).min(3.0), &out).is_ok() {
                    clip.thumb = out.to_string_lossy().to_string();
                    changed = true;
                }
            }
        }
        if changed {
            let _ = self.save();
        }
        changed
    }
}

/// Corbeille Windows (annulable), jamais de suppression définitive.
pub fn recycle(path: &Path) -> Result<()> {
    use windows_sys::Win32::UI::Shell::{
        FO_DELETE, FOF_ALLOWUNDO, FOF_NOCONFIRMATION, FOF_NOERRORUI, FOF_SILENT, SHFILEOPSTRUCTW, SHFileOperationW,
    };
    let mut from: Vec<u16> = path.as_os_str().encode_wide_vec();
    from.push(0);
    from.push(0);
    let mut op: SHFILEOPSTRUCTW = unsafe { std::mem::zeroed() };
    op.wFunc = FO_DELETE;
    op.pFrom = from.as_ptr();
    op.fFlags = (FOF_ALLOWUNDO | FOF_NOCONFIRMATION | FOF_NOERRORUI | FOF_SILENT) as u16;
    let r = unsafe { SHFileOperationW(&mut op) };
    if r != 0 {
        bail!("Impossible d'envoyer le fichier à la Corbeille (code {r})");
    }
    Ok(())
}

trait WideVec {
    fn encode_wide_vec(&self) -> Vec<u16>;
}

impl WideVec for std::ffi::OsStr {
    fn encode_wide_vec(&self) -> Vec<u16> {
        use std::os::windows::ffi::OsStrExt;
        self.encode_wide().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn collections_fiables() {
        let clip = |id: &str, tags: &[&str]| Clip { id: id.into(), tags: tags.iter().map(|t| t.to_string()).collect(), ..Default::default() };
        let mut lib = Library { clips: vec![clip("a", &["Edit"]), clip("b", &[]), clip("c", &[])] };
        let ids: Vec<String> = ["a", "b", "c", "absent"].iter().map(|s| s.to_string()).collect();
        // « edit » rejoint « Edit », le clip absent est ignoré sans bloquer les autres
        let changed = lib.set_collection(&ids, "  edit ", true).unwrap();
        assert_eq!(changed.len(), 2);
        assert!(lib.clips.iter().all(|c| c.tags == vec!["Edit".to_string()]));
        assert_eq!(lib.collections(), vec!["Edit".to_string()]);
        assert!(lib.set_collection(&ids, "   ", true).is_err());
        lib.set_collection(&["b".into()], "EDIT", false).unwrap();
        assert!(lib.clips[1].tags.is_empty());
        lib.rename_collection("edit", "Montage  CS2").unwrap();
        assert_eq!(lib.collections(), vec!["Montage CS2".to_string()]);
        assert_eq!(lib.delete_collection("montage cs2").len(), 2);
        assert!(lib.collections().is_empty());
        assert_eq!(collection_name(&"x".repeat(60)).chars().count(), 40);
        assert!(drive_available(Path::new(r"C:\Windows\nope.mp4")));
        if !Path::new(r"Q:\").exists() {
            assert!(!drive_available(Path::new(r"Q:\clips\a.mp4")), "disque absent");
        }
    }

    #[test]
    fn clip_deplace_garde_ses_reperes() {
        let root = std::env::temp_dir().join(format!("rewind-test-lib-{}", new_id()));
        let (a, b) = (root.join("ancien").join("Counter-Strike 2"), root.join("nouveau").join("Counter-Strike 2"));
        std::fs::create_dir_all(&a).unwrap();
        std::fs::create_dir_all(&b).unwrap();
        let old = a.join("CS2 3K · 100 % HS 2026-11-15 01h48-36.mp4");
        std::fs::write(&old, b"0123456789").unwrap();
        let clip = Clip {
            id: "abc".into(), path: old.to_string_lossy().to_string(), title: "CS2 3K".into(), size: 10, kind: "auto".into(),
            markers: vec![Marker { time: 5.1, kind: "headshot".into(), label: "Headshot".into() }], ..Default::default()
        };
        let mut lib = Library { clips: vec![clip] };
        // Le fichier change de dossier (comme un déplacement vers un autre disque)
        let new = b.join("CS2 3K · 100 % HS 2026-11-15 01h48-36.mp4");
        std::fs::rename(&old, &new).unwrap();
        lib.scan(&root.join("nouveau"));
        assert_eq!(lib.clips.len(), 1, "pas de doublon");
        assert_eq!(lib.clips[0].id, "abc", "même identifiant");
        assert_eq!(lib.clips[0].path, new.to_string_lossy());
        assert_eq!(lib.clips[0].markers.len(), 1, "repères gardés");
        // Un autre fichier de même nom mais de taille différente n'est pas confondu
        assert!(!moved_here(&lib.clips[0], &new, 99));
        let _ = std::fs::remove_dir_all(&root);
    }
}