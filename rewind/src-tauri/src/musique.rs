//! Musique par lien pour le Studio (2026-11-23) : coller un lien TikTok, YouTube ou SoundCloud et récupérer le son.
//!
//! - TikTok : le SON D'ORIGINE de la vidéo (la musique utilisée, sans les voix ni les effets ajoutés par-dessus),
//!   lu dans les données de la page (`music.playUrl`). Si la page ne le donne pas : le son de la vidéo, en le disant.
//! - YouTube, SoundCloud : l'outil libre yt-dlp (téléchargé une fois depuis GitHub, empreinte SHA-256 vérifiée
//!   avec la liste publiée par le projet), converti en MP3 avec le FFmpeg de Rewind.
//!
//! Les fichiers vont dans le dossier de données (« musique ») et ne sont jamais envoyés ailleurs.

use crate::{ffmpeg, util};
use anyhow::{Context, Result, bail};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

const UA: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0 Safari/537.36";
const YTDLP_URL: &str = "https://github.com/yt-dlp/yt-dlp/releases/latest/download/yt-dlp.exe";
const YTDLP_SUMS: &str = "https://github.com/yt-dlp/yt-dlp/releases/latest/download/SHA2-256SUMS";

#[derive(Clone, Debug, Serialize)]
pub struct LinkAudio {
    pub path: String,
    pub name: String,
    /// Précision à montrer (ex. « son complet de la vidéo »), vide sinon
    pub note: String,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Site {
    TikTok,
    YouTube,
    SoundCloud,
}

/// Site reconnu d'après le lien (None = lien non pris en charge).
pub fn site(url: &str) -> Option<Site> {
    let u = url.trim().to_ascii_lowercase();
    if !(u.starts_with("https://") || u.starts_with("http://")) {
        return None;
    }
    let host = u.split("://").nth(1)?.split(['/', '?', '#']).next()?.trim_start_matches("www.").trim_start_matches("m.").to_string();
    match host.as_str() {
        "tiktok.com" | "vm.tiktok.com" | "vt.tiktok.com" => Some(Site::TikTok),
        "youtube.com" | "youtu.be" | "music.youtube.com" => Some(Site::YouTube),
        "soundcloud.com" | "on.soundcloud.com" => Some(Site::SoundCloud),
        _ => None,
    }
}

fn out_dir() -> PathBuf {
    let d = util::data_dir().join("musique");
    let _ = std::fs::create_dir_all(&d);
    d
}

fn agent() -> ureq::Agent {
    ureq::AgentBuilder::new().timeout(Duration::from_secs(60)).redirects(8).user_agent(UA).build()
}

/// Récupère le son d'un lien. Long (téléchargement) : à lancer hors du fil de l'interface.
pub fn import(url: &str) -> Result<LinkAudio> {
    match site(url) {
        Some(Site::TikTok) => match tiktok_sound(url) {
            Ok(a) => Ok(a),
            Err(e) => {
                log::warn!("TikTok : son d'origine indisponible ({e:#}), son de la vidéo à la place");
                let mut a = ytdlp_audio(url)?;
                a.note = "Son complet de la vidéo : la musique seule n'était pas disponible.".into();
                Ok(a)
            }
        },
        Some(_) => ytdlp_audio(url),
        None => bail!("Lien non pris en charge : colle un lien TikTok, YouTube ou SoundCloud."),
    }
}

/// TikTok : données de la page → `music` (titre, auteur, fichier du son d'origine).
fn tiktok_sound(url: &str) -> Result<LinkAudio> {
    let ag = agent();
    let html = ag.get(url.trim()).set("Accept-Language", "fr-FR,fr;q=0.9,en;q=0.8").call()?.into_string()?;
    let json = between(&html, r#"<script id="__UNIVERSAL_DATA_FOR_REHYDRATION__" type="application/json">"#, "</script>").context("données de la page introuvables")?;
    let v: serde_json::Value = serde_json::from_str(json)?;
    let item = &v["__DEFAULT_SCOPE__"]["webapp.video-detail"]["itemInfo"]["itemStruct"];
    let music = &item["music"];
    let play = music["playUrl"].as_str().filter(|s| s.starts_with("http")).context("pas de son d'origine dans la page")?;
    let title = music["title"].as_str().unwrap_or("Son TikTok").trim().to_string();
    let author = music["authorName"].as_str().unwrap_or("").trim().to_string();
    let name = if author.is_empty() { title.clone() } else { format!("{title} - {author}") };
    let id = music["id"].as_str().map(str::to_string).or_else(|| music["id"].as_u64().map(|n| n.to_string())).unwrap_or_else(|| item["id"].as_str().unwrap_or("tiktok").to_string());
    let path = out_dir().join(format!("{} [{}].mp3", util::safe_name(&name).chars().take(80).collect::<String>(), util::safe_name(&id)));
    if !path.is_file() {
        let resp = ag.get(play).set("Referer", "https://www.tiktok.com/").call()?;
        let mut bytes = Vec::new();
        resp.into_reader().take(60 << 20).read_to_end(&mut bytes)?;
        if bytes.len() < 1024 {
            bail!("son vide");
        }
        let tmp = path.with_extension("part");
        std::fs::write(&tmp, &bytes)?;
        std::fs::rename(&tmp, &path)?;
    }
    // Son « original » = la bande-son créée par l'auteur de la vidéo (souvent sa propre voix) : on le précise
    let note = if music["original"].as_bool() == Some(true) && title.to_lowercase().contains("original") {
        "Son original de la vidéo (pas une musique ajoutée) : il peut contenir des voix.".into()
    } else {
        String::new()
    };
    log::info!("TikTok : son d'origine récupéré ({})", path.display());
    Ok(LinkAudio { path: path.to_string_lossy().to_string(), name, note })
}

fn between<'a>(s: &'a str, a: &str, b: &str) -> Option<&'a str> {
    let i = s.find(a)? + a.len();
    let j = s[i..].find(b)? + i;
    Some(&s[i..j])
}

/// yt-dlp prêt à l'emploi : téléchargé une fois (puis rafraîchi tous les 30 jours), empreinte vérifiée.
pub fn ytdlp() -> Result<PathBuf> {
    let dir = util::data_dir().join("outils");
    std::fs::create_dir_all(&dir)?;
    let exe = dir.join("yt-dlp.exe");
    let fresh = std::fs::metadata(&exe).and_then(|m| m.modified()).ok().and_then(|t| t.elapsed().ok()).is_some_and(|age| age < Duration::from_secs(30 * 86_400));
    if exe.is_file() && fresh {
        return Ok(exe);
    }
    let ag = agent();
    let fetch = || -> Result<()> {
        let sums = ag.get(YTDLP_SUMS).call()?.into_string()?;
        let want = sums
            .lines()
            .find_map(|l| {
                let mut it = l.split_whitespace();
                let (h, f) = (it.next()?, it.next()?);
                (f.trim_start_matches('*') == "yt-dlp.exe").then(|| h.to_ascii_lowercase())
            })
            .context("empreinte de yt-dlp.exe introuvable")?;
        let mut bytes = Vec::new();
        ag.get(YTDLP_URL).call()?.into_reader().take(80 << 20).read_to_end(&mut bytes)?;
        let got: String = Sha256::digest(&bytes).iter().map(|b| format!("{b:02x}")).collect();
        if got != want {
            bail!("empreinte de yt-dlp.exe incorrecte : téléchargement refusé");
        }
        let tmp = exe.with_extension("part");
        std::fs::write(&tmp, &bytes)?;
        std::fs::rename(&tmp, &exe)?;
        log::info!("yt-dlp téléchargé et vérifié ({} Mo)", bytes.len() / 1_000_000);
        Ok(())
    };
    match fetch() {
        Ok(()) => Ok(exe),
        // hors ligne : l'ancienne version fait l'affaire
        Err(e) if exe.is_file() => {
            log::warn!("yt-dlp : mise à jour impossible ({e:#}), ancienne version gardée");
            Ok(exe)
        }
        Err(e) => Err(e.context("Téléchargement de l'outil yt-dlp impossible")),
    }
}

/// Son d'une vidéo YouTube, SoundCloud (ou TikTok en secours) en MP3, via yt-dlp + FFmpeg de Rewind.
fn ytdlp_audio(url: &str) -> Result<LinkAudio> {
    let exe = ytdlp()?;
    let tools = ffmpeg::tools()?;
    let ffdir = tools.ffmpeg.parent().map(Path::to_path_buf).unwrap_or_default();
    let dir = out_dir();
    let out = util::command(&exe)
        .args(["--no-playlist", "--no-warnings", "--no-progress", "--restrict-filenames", "-f", "bestaudio/best", "-x", "--audio-format", "mp3", "--audio-quality", "0", "--ffmpeg-location"])
        .arg(&ffdir)
        .arg("-o")
        .arg(dir.join("%(title).80s [%(id)s].%(ext)s"))
        .args(["--print", "after_move:%(filepath)s\t%(title)s\t%(uploader)s"])
        .arg(url.trim())
        .stdin(Stdio::null())
        .output()
        .context("Lancement de yt-dlp")?;
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr);
        let last = err.lines().rev().find(|l| !l.trim().is_empty()).unwrap_or("erreur inconnue");
        bail!("Son introuvable : {}", last.trim_start_matches("ERROR: "));
    }
    let line = String::from_utf8_lossy(&out.stdout).lines().rev().find(|l| l.contains('\t')).unwrap_or_default().to_string();
    let mut it = line.split('\t');
    let path = it.next().unwrap_or_default().trim().to_string();
    let title = it.next().unwrap_or("Musique").trim().to_string();
    let uploader = it.next().unwrap_or("").trim().to_string();
    if path.is_empty() || !Path::new(&path).is_file() {
        bail!("Le son n'a pas été enregistré");
    }
    let name = if uploader.is_empty() || uploader == "NA" { title } else { format!("{title} - {uploader}") };
    Ok(LinkAudio { path, name, note: String::new() })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sites_reconnus() {
        assert_eq!(site("https://www.tiktok.com/@a/video/123"), Some(Site::TikTok));
        assert_eq!(site("https://vm.tiktok.com/ZMabc/"), Some(Site::TikTok));
        assert_eq!(site("https://youtu.be/abc"), Some(Site::YouTube));
        assert_eq!(site("https://m.youtube.com/watch?v=abc"), Some(Site::YouTube));
        assert_eq!(site("https://soundcloud.com/x/y"), Some(Site::SoundCloud));
        assert_eq!(site("https://evil.example/tiktok.com"), None);
        assert_eq!(site("file:///C:/a.mp3"), None);
        assert_eq!(between("a<x>b</x>c", "<x>", "</x>"), Some("b"));
    }

    /// Réseau : `REWIND_DATA_DIR=<dossier> cargo test --lib musique_reelle -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn musique_reelle() {
        let a = import("https://www.tiktok.com/@scout2015/video/6718335390845095173").expect("TikTok");
        println!("TikTok : {} ({}) {}", a.name, a.path, a.note);
        assert!(Path::new(&a.path).is_file());
    }
}
