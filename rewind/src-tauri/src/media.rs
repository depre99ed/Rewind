//! Infos sur les médias pour le Studio : pistes, miniatures défilantes (une image par tranche de
//! temps, assemblées en une seule bande), forme d'onde du son. Tout est mis en cache sur le disque.

use crate::util::OutputTimeout;
use crate::{ffmpeg, util};
use anyhow::{Context, Result, bail};
use serde::Serialize;
use std::{
    io::Read,
    path::{Path, PathBuf},
    process::Stdio,
};

#[derive(Clone, Debug, Serialize, Default)]
pub struct MediaInfo {
    pub duration: f64,
    pub width: u32,
    pub height: u32,
    pub fps: f64,
    /// Nom de chaque piste son, dans l'ordre du fichier (ex. « Mixage », « Son du PC », « Micro »)
    pub tracks: Vec<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct Filmstrip {
    /// Image unique contenant toutes les vignettes côte à côte
    pub sprite: String,
    pub frames: u32,
    /// Durée couverte par chaque vignette (secondes)
    pub interval: f64,
    pub frame_w: u32,
    pub frame_h: u32,
}

#[derive(Clone, Debug, Serialize)]
pub struct Waveform {
    /// Nombre de valeurs par seconde
    pub rate: f64,
    /// Niveau crête de chaque tranche (0..255)
    pub peaks: Vec<u8>,
}

const STRIP_H: u32 = 72;
const WAVE_RATE: f64 = 50.0;

pub(crate) fn cache_dir(kind: &str) -> PathBuf {
    let dir = util::data_dir().join("cache").join(kind);
    let _ = std::fs::create_dir_all(&dir);
    dir
}

/// Clé de cache : chemin + taille + date de modification (un fichier remplacé est recalculé).
pub(crate) fn cache_key(path: &Path) -> String {
    let meta = std::fs::metadata(path).ok();
    let size = meta.as_ref().map(|m| m.len()).unwrap_or(0);
    let mtime = meta
        .and_then(|m| m.modified().ok())
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let mut h: u64 = 0xcbf29ce484222325;
    for b in format!("{}|{size}|{mtime}", path.to_string_lossy().to_lowercase()).bytes() {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    format!("{h:016x}")
}

/// Garde le cache sous `max` fichiers (les plus anciens partent).
pub(crate) fn prune(dir: &Path, max: usize) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    let mut files: Vec<(std::time::SystemTime, PathBuf)> =
        entries.flatten().filter_map(|e| Some((e.metadata().ok()?.modified().ok()?, e.path()))).collect();
    if files.len() <= max {
        return;
    }
    files.sort_by(|a, b| a.0.cmp(&b.0));
    let extra = files.len() - max;
    for (_, p) in files.into_iter().take(extra) {
        let _ = std::fs::remove_file(p);
    }
}

pub fn info(path: &Path) -> Result<MediaInfo> {
    if !path.is_file() {
        bail!("Fichier introuvable : {}", path.display());
    }
    let tools = ffmpeg::tools()?;
    let out = util::command(&tools.ffprobe)
        .args([
            "-v", "error", "-show_entries",
            "format=duration:stream=codec_type,width,height,avg_frame_rate:stream_tags=title,name", "-of", "json",
        ])
        .arg(path)
        .stdin(Stdio::null())
        .output_timeout(std::time::Duration::from_secs(30))
        .context("ffprobe")?;
    let json: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap_or_default();
    let mut info = MediaInfo {
        duration: json["format"]["duration"].as_str().and_then(|s| s.parse().ok()).unwrap_or(0.0),
        ..Default::default()
    };
    for s in json["streams"].as_array().cloned().unwrap_or_default() {
        match s["codec_type"].as_str() {
            Some("video") if info.width == 0 => {
                info.width = s["width"].as_u64().unwrap_or(0) as u32;
                info.height = s["height"].as_u64().unwrap_or(0) as u32;
                let fr = s["avg_frame_rate"].as_str().unwrap_or("0/1");
                let mut it = fr.split('/').map(|x| x.parse::<f64>().unwrap_or(0.0));
                let (a, b) = (it.next().unwrap_or(0.0), it.next().unwrap_or(1.0));
                info.fps = if b > 0.0 { a / b } else { 0.0 };
            }
            Some("audio") => {
                let n = info.tracks.len() + 1;
                // MKV : « title » ; MP4 : « name » (c'est là que FFmpeg range le titre de la piste)
                let tag = |k: &str| s["tags"][k].as_str().filter(|t| !t.trim().is_empty()).map(str::to_string);
                let title = tag("title").or_else(|| tag("name"));
                info.tracks.push(title.unwrap_or_else(|| format!("Piste {n}")));
            }
            _ => {}
        }
    }
    Ok(info)
}

/// Bande de vignettes : environ une par seconde (90 au plus), hauteur 72 px.
/// Seules les images clés sont décodées : très rapide sur nos clips (une image clé par seconde).
pub fn filmstrip(path: &Path) -> Result<Filmstrip> {
    let info = info(path)?;
    if info.width == 0 {
        bail!("Pas d'image dans ce fichier");
    }
    let dur = info.duration.max(0.1);
    let frames = (dur.ceil() as u32).clamp(1, 90);
    let interval = dur / frames as f64;
    let frame_h = STRIP_H;
    let frame_w = (((info.width as f64 / info.height.max(1) as f64) * frame_h as f64 / 2.0).round() as u32 * 2).max(2);
    let dir = cache_dir("filmstrip");
    let out = dir.join(format!("{}.jpg", cache_key(path)));
    let strip = Filmstrip { sprite: out.to_string_lossy().to_string(), frames, interval, frame_w, frame_h };
    if out.is_file() {
        return Ok(strip);
    }
    let tools = ffmpeg::tools()?;
    let tmp = out.with_extension("tmp.jpg");
    // tpad : la dernière image est prolongée, sinon les dernières vignettes restaient noires
    let vf = format!(
        "tpad=stop_mode=clone:stop_duration={:.2},fps=1/{interval:.4},scale={frame_w}:{frame_h}:flags=bilinear,tile={frames}x1",
        interval * 3.0 + 1.0
    );
    let run = |skip: bool| {
        let mut cmd = util::command(&tools.ffmpeg);
        cmd.args(["-hide_banner", "-loglevel", "error", "-y"]);
        if skip {
            cmd.args(["-skip_frame", "nokey"]);
        }
        cmd.arg("-i").arg(path).args(["-an", "-vf", &vf, "-frames:v", "1", "-q:v", "5"]).arg(&tmp).stdin(Stdio::null()).output_timeout(std::time::Duration::from_secs(60))
    };
    // Images clés seules d'abord ; si le fichier en a trop peu, décodage complet
    let ok = run(true).map(|o| o.status.success()).unwrap_or(false) && tmp.is_file();
    if !ok {
        ffmpeg::checked(run(false)?, "Vignettes")?;
    }
    std::fs::rename(&tmp, &out)?;
    prune(&dir, 400);
    Ok(strip)
}

/// Pistes son séparées d'un clip (sans la piste 0 de mixage), chacune dans son propre fichier .m4a :
/// l'aperçu du Studio les joue à part pour qu'un réglage « Micro 0 % » s'entende tout de suite.
/// Copie directe du son (aucun ré-encodage) : quasi instantané, gardé en cache.
pub fn stems(path: &Path) -> Result<Vec<String>> {
    let info = info(path)?;
    if info.tracks.len() < 2 {
        return Ok(Vec::new());
    }
    let dir = cache_dir("stems");
    let key = cache_key(path);
    let tools = ffmpeg::tools()?;
    let mut out = Vec::new();
    for k in 1..info.tracks.len() {
        let file = dir.join(format!("{key}_{k}.m4a"));
        if !file.is_file() {
            let tmp = dir.join(format!("{key}_{k}.tmp.m4a"));
            let run = |copy: bool| {
                let mut cmd = util::command(&tools.ffmpeg);
                cmd.args(["-hide_banner", "-loglevel", "error", "-y", "-i"]).arg(path);
                cmd.args(["-vn", "-map", &format!("0:a:{k}")]);
                if copy {
                    cmd.args(["-c:a", "copy"]);
                } else {
                    cmd.args(["-c:a", "aac", "-b:a", "160k"]);
                }
                cmd.arg(&tmp).stdin(Stdio::null()).output_timeout(std::time::Duration::from_secs(300))
            };
            // Copie directe ; si le codec ne rentre pas dans un .m4a, ré-encodage en AAC
            let ok = run(true).map(|o| o.status.success()).unwrap_or(false) && tmp.is_file();
            if !ok {
                ffmpeg::checked(run(false)?, "Pistes son")?;
            }
            std::fs::rename(&tmp, &file)?;
        }
        out.push(file.to_string_lossy().to_string());
    }
    prune(&dir, 300);
    Ok(out)
}

/// Forme d'onde de la première piste son (50 valeurs par seconde).
pub fn waveform(path: &Path) -> Result<Waveform> {
    let dir = cache_dir("wave");
    let cached = dir.join(format!("{}.bin", cache_key(path)));
    if let Ok(peaks) = std::fs::read(&cached) {
        return Ok(Waveform { rate: WAVE_RATE, peaks });
    }
    let tools = ffmpeg::tools()?;
    let mut child = util::command(&tools.ffmpeg)
        .args(["-hide_banner", "-loglevel", "error", "-i"])
        .arg(path)
        .args(["-map", "0:a:0", "-ac", "1", "-ar", "8000", "-f", "s16le", "pipe:1"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .context("Forme d'onde")?;
    // Agrégé au fil de la lecture (audit 0.4.12) : avant, tout le son était gardé en mémoire avant le
    // calcul (~58 Mo par heure de vidéo). Même résultat, octet pour octet.
    let per = (8000.0 / WAVE_RATE) as usize;
    let block = per * 2;
    let to_byte = |peak: u16| ((peak as f64 / 32768.0).powf(0.6) * 255.0).round().min(255.0) as u8;
    let mut peaks: Vec<u8> = Vec::new();
    let mut pending: Vec<u8> = Vec::with_capacity(block + (1 << 16));
    if let Some(mut out) = child.stdout.take() {
        let mut buf = vec![0u8; 1 << 16];
        loop {
            let n = match out.read(&mut buf) {
                Ok(0) | Err(_) => break,
                Ok(n) => n,
            };
            pending.extend_from_slice(&buf[..n]);
            let whole = pending.len() / block * block;
            for chunk in pending[..whole].chunks(block) {
                // Racine : les sons faibles restent visibles
                peaks.push(to_byte(chunk.chunks_exact(2).map(|b| i16::from_le_bytes([b[0], b[1]]).unsigned_abs()).max().unwrap_or(0)));
            }
            pending.drain(..whole);
        }
    }
    let _ = child.wait();
    if !pending.is_empty() {
        peaks.push(to_byte(pending.chunks_exact(2).map(|b| i16::from_le_bytes([b[0], b[1]]).unsigned_abs()).max().unwrap_or(0)));
    }
    let _ = std::fs::write(&cached, &peaks);
    prune(&dir, 600);
    Ok(Waveform { rate: WAVE_RATE, peaks })
}
