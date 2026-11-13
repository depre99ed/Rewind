//! Localisation de FFmpeg et choix de l'encodeur vidéo.

use crate::util;
use anyhow::{Context, Result, bail};
use std::{
    path::{Path, PathBuf},
    process::{Output, Stdio},
    sync::OnceLock,
};

#[derive(Clone, Debug)]
pub struct Tools {
    pub ffmpeg: PathBuf,
    pub ffprobe: PathBuf,
}

static TOOLS: OnceLock<Option<Tools>> = OnceLock::new();
static ENCODERS: OnceLock<Vec<String>> = OnceLock::new();

/// Cherche ffmpeg.exe : à côté de Rewind, dans `bin/`, dans REWIND_FFMPEG_DIR, puis dans le PATH.
pub fn tools() -> Result<Tools> {
    TOOLS
        .get_or_init(find)
        .clone()
        .context("FFmpeg est introuvable. Place ffmpeg.exe et ffprobe.exe dans le dossier bin de Rewind.")
}

fn find() -> Option<Tools> {
    let mut candidates: Vec<PathBuf> = Vec::new();
    if let Ok(dir) = std::env::var("REWIND_FFMPEG_DIR") {
        candidates.push(PathBuf::from(dir));
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            candidates.push(dir.join("bin"));
            candidates.push(dir.to_path_buf());
            candidates.push(dir.join("resources").join("bin"));
            // En développement : target/debug → src-tauri/bin
            candidates.push(dir.join("..").join("..").join("bin"));
        }
    }
    for dir in candidates {
        let ffmpeg = dir.join("ffmpeg.exe");
        let ffprobe = dir.join("ffprobe.exe");
        if ffmpeg.is_file() && ffprobe.is_file() {
            return Some(Tools { ffmpeg, ffprobe });
        }
    }
    // Dernier recours : le PATH
    let output = util::command(Path::new("where")).arg("ffmpeg").output().ok()?;
    let first = String::from_utf8_lossy(&output.stdout).lines().next()?.trim().to_string();
    let ffmpeg = PathBuf::from(first);
    let ffprobe = ffmpeg.with_file_name("ffprobe.exe");
    (ffmpeg.is_file() && ffprobe.is_file()).then_some(Tools { ffmpeg, ffprobe })
}

pub fn checked(output: Output, what: &str) -> Result<Output> {
    if output.status.success() {
        return Ok(output);
    }
    let err = String::from_utf8_lossy(&output.stderr);
    let tail: Vec<&str> = err.lines().filter(|l| !l.trim().is_empty()).rev().take(4).collect();
    bail!("{what} : {}", tail.into_iter().rev().collect::<Vec<_>>().join(" | "));
}

/// Encodeurs matériels qui fonctionnent vraiment sur ce PC (testés une fois).
pub fn working_encoders() -> Vec<String> {
    ENCODERS
        .get_or_init(|| {
            let Ok(tools) = tools() else { return vec!["libx264".into()] };
            let candidates = [
                "h264_nvenc", "hevc_nvenc", "av1_nvenc", "h264_amf", "hevc_amf", "av1_amf", "h264_qsv",
                "hevc_qsv",
            ];
            let mut found: Vec<String> = std::thread::scope(|scope| {
                let handles: Vec<_> = candidates
                    .iter()
                    .map(|enc| {
                        let ffmpeg = tools.ffmpeg.clone();
                        scope.spawn(move || {
                            let ok = util::command(&ffmpeg)
                                .args([
                                    "-hide_banner", "-loglevel", "error", "-f", "lavfi", "-i",
                                    "color=c=black:s=640x360:r=30", "-frames:v", "3", "-c:v", enc,
                                    "-f", "null", "-",
                                ])
                                .stdin(Stdio::null())
                                .stdout(Stdio::null())
                                .stderr(Stdio::null())
                                .status()
                                .map(|s| s.success())
                                .unwrap_or(false);
                            ok.then(|| enc.to_string())
                        })
                    })
                    .collect();
                handles.into_iter().filter_map(|h| h.join().ok().flatten()).collect()
            });
            found.push("libx264".into());
            found
        })
        .clone()
}

/// Encodeur choisi : réglage de l'utilisateur s'il marche, sinon le meilleur H.264 disponible.
pub fn pick_encoder(wanted: &str) -> String {
    let list = working_encoders();
    if wanted != "auto" && list.iter().any(|e| e == wanted) {
        return wanted.to_string();
    }
    for pref in ["h264_nvenc", "h264_amf", "h264_qsv"] {
        if list.iter().any(|e| e == pref) {
            return pref.to_string();
        }
    }
    "libx264".into()
}

/// Options d'encodage pour l'enregistrement continu.
pub fn video_args(encoder: &str, fps: u32, bitrate: u32) -> Vec<String> {
    let b = format!("{bitrate}M");
    let max = format!("{}M", bitrate * 3 / 2);
    let buf = format!("{}M", bitrate * 2);
    let g = fps.to_string();
    let mut a: Vec<String> = vec!["-c:v".into(), encoder.into()];
    let push = |a: &mut Vec<String>, items: &[&str]| a.extend(items.iter().map(|s| s.to_string()));
    if encoder.contains("nvenc") {
        push(&mut a, &["-preset", "p4", "-tune", "hq", "-rc", "vbr", "-cq", "23", "-bf", "0", "-no-scenecut", "1"]);
    } else if encoder.contains("amf") {
        push(&mut a, &["-usage", "transcoding", "-quality", "balanced", "-rc", "vbr_peak", "-bf", "0"]);
    } else if encoder.contains("qsv") {
        push(&mut a, &["-preset", "faster", "-look_ahead", "0", "-bf", "0"]);
    } else {
        push(&mut a, &["-preset", "veryfast", "-tune", "zerolatency", "-threads", "4"]);
    }
    push(&mut a, &["-b:v", &b, "-maxrate", &max, "-bufsize", &buf, "-g", &g]);
    a
}

/// Filtre à ajouter après la capture GPU selon l'encodeur.
pub fn upload_filter(encoder: &str) -> Option<&'static str> {
    if encoder.contains("nvenc") || encoder.contains("amf") {
        None // ces encodeurs lisent directement les textures D3D11 (zéro copie)
    } else if encoder.contains("qsv") {
        Some("hwmap=derive_device=qsv,format=qsv")
    } else {
        Some("hwdownload,format=bgra,format=yuv420p")
    }
}

/// Durée d'un fichier vidéo en secondes.
pub fn duration(path: &Path) -> Option<f64> {
    let tools = tools().ok()?;
    let out = util::command(&tools.ffprobe)
        .args(["-v", "error", "-show_entries", "format=duration", "-of", "default=nw=1:nk=1"])
        .arg(path)
        .stdin(Stdio::null())
        .output()
        .ok()?;
    String::from_utf8_lossy(&out.stdout).trim().parse().ok()
}

/// Une piste d'un fichier (contrôles automatiques).
#[derive(Clone, Debug)]
pub struct StreamInfo {
    /// « video » ou « audio »
    pub kind: String,
    pub start: f64,
    pub duration: f64,
}

/// Pistes d'un fichier, avec leur début et leur durée. Sert aux contrôles automatiques.
pub fn streams(path: &Path) -> Vec<StreamInfo> {
    let Ok(tools) = tools() else { return Vec::new() };
    let Ok(out) = util::command(&tools.ffprobe)
        .args(["-v", "error", "-show_entries", "stream=codec_type,start_time,duration", "-of", "json"])
        .arg(path)
        .stdin(Stdio::null())
        .output()
    else {
        return Vec::new();
    };
    let json: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap_or_default();
    let num = |v: &serde_json::Value| v.as_str().and_then(|s| s.parse::<f64>().ok()).unwrap_or(0.0);
    json["streams"]
        .as_array()
        .map(|a| {
            a.iter()
                .map(|s| StreamInfo {
                    kind: s["codec_type"].as_str().unwrap_or_default().to_string(),
                    start: num(&s["start_time"]),
                    duration: num(&s["duration"]),
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Miniature JPEG prise à `at` secondes.
pub fn thumbnail(video: &Path, at: f64, out: &Path) -> Result<()> {
    let tools = tools()?;
    let output = util::command(&tools.ffmpeg)
        .args(["-hide_banner", "-loglevel", "error", "-y", "-ss", &format!("{at:.2}"), "-i"])
        .arg(video)
        .args(["-frames:v", "1", "-vf", "scale=480:-2,format=yuvj420p", "-q:v", "4"])
        .arg(out)
        .stdin(Stdio::null())
        .output()?;
    checked(output, "Miniature")?;
    Ok(())
}
