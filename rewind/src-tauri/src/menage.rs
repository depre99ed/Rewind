//! Ménage des fichiers temporaires de Rewind (2026-11-24, audit de performances).
//!
//! Décision de l'utilisateur : nettoyer automatiquement les fichiers de TRAVAIL de Rewind qui
//! grossissaient sans limite, JAMAIS ses clips, projets, réglages ni la bibliothèque.
//! Tourne 3 minutes après le démarrage, puis toutes les 12 heures (Rewind reste ouvert des semaines).
//!
//! Ce qui est nettoyé (tout est dans le dossier de données de Rewind) :
//! - `discord/` : copies compressées pour Discord (déjà collées), plus de 3 jours ;
//! - `cache/graphe-*.txt` : graphes d'export du Studio gardés pour diagnostic, plus de 2 jours ;
//! - `*.partial.mp4` abandonnés dans le dossier des clips (`studio::clean_stale_outputs`).

use std::{
    path::Path,
    time::{Duration, SystemTime},
};

fn older_than(path: &Path, age: Duration) -> bool {
    std::fs::metadata(path)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| SystemTime::now().duration_since(t).ok())
        .is_some_and(|a| a > age)
}

/// Supprime, dans `dir` (sans descendre dans les sous-dossiers), les fichiers dont le nom passe `matches`
/// et sont plus vieux que `age`. Renvoie (nombre, octets).
fn sweep(dir: &Path, age: Duration, matches: impl Fn(&str) -> bool) -> (u32, u64) {
    let (mut n, mut bytes) = (0u32, 0u64);
    let Ok(entries) = std::fs::read_dir(dir) else { return (0, 0) };
    for e in entries.flatten() {
        let p = e.path();
        if !p.is_file() {
            continue;
        }
        let name = p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        if !matches(&name) || !older_than(&p, age) {
            continue;
        }
        let size = e.metadata().map(|m| m.len()).unwrap_or(0);
        if std::fs::remove_file(&p).is_ok() {
            n += 1;
            bytes += size;
        }
    }
    (n, bytes)
}

/// Un passage de ménage (aussi appelé par les tests).
pub fn run_once(data: &Path) -> (u32, u64) {
    let day = Duration::from_secs(86_400);
    let mut total = (0u32, 0u64);
    let mut add = |r: (u32, u64)| {
        total.0 += r.0;
        total.1 += r.1;
    };
    add(sweep(&data.join("discord"), 3 * day, |n| n.to_ascii_lowercase().ends_with(".mp4")));
    add(sweep(&data.join("cache"), 2 * day, |n| n.starts_with("graphe-") && n.ends_with(".txt")));
    total
}

pub fn start() {
    let _ = std::thread::Builder::new().name("menage".into()).spawn(|| {
        std::thread::sleep(Duration::from_secs(180));
        loop {
            let (n, bytes) = run_once(&crate::util::data_dir());
            // Exports interrompus (`*.partial.mp4` de plus de 10 min, jamais dans « Reçus »)
            crate::studio::clean_stale_outputs();
            if n > 0 {
                log::info!("ménage : {n} fichier(s) temporaire(s) supprimé(s) ({:.0} Mo)", bytes as f64 / 1_048_576.0);
            }
            std::thread::sleep(Duration::from_secs(12 * 3600));
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn menage_ne_touche_que_les_temporaires_anciens() {
        let dir = std::env::temp_dir().join(format!("rewind-menage-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("discord")).unwrap();
        std::fs::create_dir_all(dir.join("cache")).unwrap();
        let old = SystemTime::now() - Duration::from_secs(5 * 86_400);
        let mk = |rel: &str, aged: bool| {
            let p = dir.join(rel);
            std::fs::write(&p, b"x").unwrap();
            if aged {
                let f = std::fs::File::options().write(true).open(&p).unwrap();
                f.set_modified(old).unwrap();
            }
            p
        };
        let a = mk("discord/vieux.mp4", true);
        let b = mk("discord/recent.mp4", false);
        let c = mk("cache/graphe-1.txt", true);
        let d = mk("cache/autre.txt", true);
        let e = mk("library.json", true);
        let (n, _) = run_once(&dir);
        assert_eq!(n, 2);
        assert!(!a.exists() && !c.exists());
        assert!(b.exists() && d.exists() && e.exists());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
