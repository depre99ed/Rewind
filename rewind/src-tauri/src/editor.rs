//! Montage : couper, fusionner, format vertical (TikTok / Shorts), compression pour Discord.
//! Les fichiers d'origine ne sont jamais modifiés : chaque opération crée un nouveau fichier.

use crate::{ffmpeg, util};
use anyhow::{Context, Result, bail};
use std::{
    io::{BufRead, BufReader},
    path::{Path, PathBuf},
    process::Stdio,
};

fn unique(path: PathBuf) -> PathBuf {
    if !path.exists() {
        return path;
    }
    let stem = path.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
    let ext = path.extension().map(|s| s.to_string_lossy().to_string()).unwrap_or_else(|| "mp4".into());
    for n in 2..1000 {
        let p = path.with_file_name(format!("{stem} ({n}).{ext}"));
        if !p.exists() {
            return p;
        }
    }
    path
}

pub fn output_next_to(source: &Path, suffix: &str) -> PathBuf {
    let stem = source.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_else(|| "Clip".into());
    unique(source.with_file_name(format!("{} {suffix}.mp4", util::safe_name(&stem))))
}

pub(crate) fn encoder_args() -> Vec<String> {
    let enc = ffmpeg::pick_encoder("auto");
    let mut a: Vec<String> = vec!["-c:v".into(), enc.clone()];
    if enc.contains("nvenc") {
        a.extend(["-preset", "p5", "-rc", "vbr", "-cq", "21", "-b:v", "0"].map(String::from));
    } else if enc.contains("amf") {
        a.extend(["-quality", "quality", "-rc", "cqp", "-qp_i", "21", "-qp_p", "23"].map(String::from));
    } else if enc.contains("qsv") {
        a.extend(["-global_quality", "22"].map(String::from));
    } else {
        a.extend(["-preset", "fast", "-crf", "21"].map(String::from));
    }
    a.extend(["-pix_fmt", "yuv420p", "-c:a", "aac", "-b:a", "192k", "-movflags", "+faststart"].map(String::from));
    a
}

/// Lance FFmpeg en suivant l'avancement (0..1) sur la durée attendue.
pub(crate) fn run(args: Vec<String>, out: &Path, total: f64, progress: &dyn Fn(f64)) -> Result<()> {
    let tools = ffmpeg::tools()?;
    let partial = out.with_extension("partial.mp4");
    let mut child = util::command(&tools.ffmpeg)
        .args(["-hide_banner", "-loglevel", "error", "-y", "-progress", "pipe:1", "-nostats"])
        .args(&args)
        .arg(&partial)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .context("Lancement de FFmpeg")?;
    let stderr = child.stderr.take();
    let err_thread = std::thread::spawn(move || {
        let mut s = String::new();
        if let Some(mut e) = stderr {
            use std::io::Read;
            let _ = e.read_to_string(&mut s);
        }
        s
    });
    if let Some(stdout) = child.stdout.take() {
        for line in BufReader::new(stdout).lines().map_while(Result::ok) {
            if let Some(v) = line.strip_prefix("out_time_us=")
                && let Ok(us) = v.parse::<f64>()
                && total > 0.0
            {
                progress((us / 1e6 / total).clamp(0.0, 1.0));
            }
        }
    }
    let status = child.wait()?;
    let err = err_thread.join().unwrap_or_default();
    if !status.success() {
        let _ = std::fs::remove_file(&partial);
        bail!("Montage impossible : {}", err.lines().last().unwrap_or("erreur inconnue"));
    }
    std::fs::rename(&partial, out)?;
    progress(1.0);
    Ok(())
}

/// Garde la partie [start, end] (secondes). `exact` = réencodage précis à l'image près.
pub fn trim(source: &Path, start: f64, end: f64, exact: bool, progress: &dyn Fn(f64)) -> Result<PathBuf> {
    if end - start < 0.2 {
        bail!("La partie gardée est trop courte.");
    }
    let out = output_next_to(source, "(coupé)");
    let mut args: Vec<String> = vec!["-ss".into(), format!("{start:.3}"), "-i".into(), source.to_string_lossy().into()];
    args.extend(["-t".into(), format!("{:.3}", end - start), "-map".into(), "0".into()]);
    if exact {
        args.extend(["-map".into(), "-0:d?".into()]);
        args.extend(encoder_args());
    } else {
        args.extend(["-c".into(), "copy".into(), "-avoid_negative_ts".into(), "make_zero".into(), "-movflags".into(), "+faststart".into()]);
    }
    run(args, &out, end - start, progress)?;
    Ok(out)
}

fn has_audio(path: &Path) -> bool {
    let Ok(tools) = ffmpeg::tools() else { return false };
    util::command(&tools.ffprobe)
        .args(["-v", "error", "-select_streams", "a", "-show_entries", "stream=index", "-of", "csv=p=0"])
        .arg(path)
        .output()
        .map(|o| !String::from_utf8_lossy(&o.stdout).trim().is_empty())
        .unwrap_or(false)
}

fn video_size(path: &Path) -> (u32, u32) {
    let Ok(tools) = ffmpeg::tools() else { return (1920, 1080) };
    let out = util::command(&tools.ffprobe)
        .args(["-v", "error", "-select_streams", "v:0", "-show_entries", "stream=width,height", "-of", "csv=p=0"])
        .arg(path)
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).to_string())
        .unwrap_or_default();
    let mut it = out.trim().split(',').filter_map(|x| x.parse::<u32>().ok());
    (it.next().unwrap_or(1920), it.next().unwrap_or(1080))
}

/// Colle plusieurs clips bout à bout (tailles différentes acceptées).
pub fn merge(sources: &[PathBuf], progress: &dyn Fn(f64)) -> Result<PathBuf> {
    if sources.len() < 2 {
        bail!("Choisis au moins deux clips.");
    }
    let (w, h) = video_size(&sources[0]);
    let first = &sources[0];
    let out = output_next_to(first, "(fusion)");
    let mut args: Vec<String> = Vec::new();
    let mut graph = String::new();
    let mut total = 0.0;
    for (i, s) in sources.iter().enumerate() {
        let d = ffmpeg::duration(s).unwrap_or(0.0);
        total += d;
        args.extend(["-i".into(), s.to_string_lossy().into()]);
        graph.push_str(&format!(
            "[{i}:v]scale={w}:{h}:force_original_aspect_ratio=decrease,pad={w}:{h}:(ow-iw)/2:(oh-ih)/2,setsar=1,fps=60,format=yuv420p[v{i}];"
        ));
        if has_audio(s) {
            graph.push_str(&format!("[{i}:a:0]aresample=48000,aformat=channel_layouts=stereo[a{i}];"));
        } else {
            graph.push_str(&format!("aevalsrc=0|0:s=48000:d={d:.3}[a{i}];"));
        }
    }
    for i in 0..sources.len() {
        graph.push_str(&format!("[v{i}][a{i}]"));
    }
    graph.push_str(&format!("concat=n={}:v=1:a=1[v][a]", sources.len()));
    args.extend(["-filter_complex".into(), graph, "-map".into(), "[v]".into(), "-map".into(), "[a]".into()]);
    args.extend(encoder_args());
    run(args, &out, total, progress)?;
    Ok(out)
}

/// Format vertical 1080×1920. mode = "crop" (recadrage centré) ou "blur" (fond flou).
pub fn vertical(source: &Path, mode: &str, start: f64, end: f64, progress: &dyn Fn(f64)) -> Result<PathBuf> {
    let out = output_next_to(source, "(vertical)");
    let filter = if mode == "crop" {
        "[0:v]crop=ih*9/16:ih,scale=1080:1920,setsar=1[v]".to_string()
    } else {
        "[0:v]split[a][b];[a]scale=1080:1920:force_original_aspect_ratio=increase,crop=1080:1920,boxblur=24:2[bg];[b]scale=1080:-2[fg];[bg][fg]overlay=(W-w)/2:(H-h)/2,setsar=1[v]".to_string()
    };
    let total = ffmpeg::duration(source).unwrap_or(0.0);
    let (s, e) = if end > start && end > 0.0 { (start, end) } else { (0.0, total) };
    let mut args: Vec<String> = vec!["-ss".into(), format!("{s:.3}"), "-i".into(), source.to_string_lossy().into(), "-t".into(), format!("{:.3}", e - s)];
    args.extend(["-filter_complex".into(), filter, "-map".into(), "[v]".into(), "-map".into(), "0:a:0?".into()]);
    args.extend(encoder_args());
    run(args, &out, e - s, progress)?;
    Ok(out)
}

/// Résultat de la préparation pour Discord.
pub struct DiscordFile {
    pub path: PathBuf,
    pub height: u32,
    pub compressed: bool,
}

/// Prépare un fichier qui tient sous `max_mb` Mo : tel quel s'il est déjà assez léger, sinon
/// réencodé en 1080p, 720p ou 480p selon ce qui garde une qualité correcte. Si même le 480p
/// serait trop dégradé, renvoie une erreur claire avec la durée maximale conseillée.
pub fn prepare_for_discord(source: &Path, max_mb: f64, out_dir: &Path, progress: &dyn Fn(f64)) -> Result<DiscordFile> {
    let size_mb = std::fs::metadata(source).map(|m| m.len()).unwrap_or(0) as f64 / 1_048_576.0;
    let (_, h) = video_size(source);
    if size_mb <= max_mb * 0.98 {
        return Ok(DiscordFile { path: source.to_path_buf(), height: h, compressed: false });
    }
    let dur = ffmpeg::duration(source).context("Durée inconnue")?.max(1.0);
    let audio_kbps = 96.0;
    let budget = |factor: f64| (max_mb * 8192.0 * factor) / dur - audio_kbps;
    // Débits minimaux pour une image correcte (H.264, 50-60 i/s)
    let ladder: [(u32, f64); 3] = [(1080, 4500.0), (720, 2000.0), (480, 850.0)];
    let video_kbps = budget(0.92);
    let Some(&(height, _)) = ladder.iter().find(|(lh, min)| *lh <= h.max(480) && video_kbps >= *min) else {
        let max_secs = (max_mb * 8192.0 * 0.92) / (850.0 + audio_kbps);
        bail!(
            "Ce clip dure {:.0} s : trop long pour tenir dans {:.0} Mo avec une image correcte. Rogne-le à {:.0} s maximum dans le Studio.",
            dur, max_mb, max_secs.floor()
        );
    };
    std::fs::create_dir_all(out_dir)?;
    let stem = source.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_else(|| "clip".into());
    let out = out_dir.join(format!("{}.mp4", util::safe_name(&stem)));
    let enc = ffmpeg::pick_encoder("auto");
    // Deux essais : si le premier dépasse (rare), on retente avec 15 % de débit en moins
    for factor in [0.92, 0.80] {
        let v = budget(factor).max(300.0);
        let mut args: Vec<String> = vec!["-i".into(), source.to_string_lossy().into(), "-map".into(), "0:v:0".into(), "-map".into(), "0:a:0?".into()];
        args.extend(["-vf".into(), format!("scale=-2:{height}:flags=lanczos"), "-c:v".into(), enc.clone()]);
        if enc.contains("nvenc") {
            args.extend(["-preset", "p6", "-tune", "hq", "-rc", "cbr", "-multipass", "fullres"].map(String::from));
        } else if enc == "libx264" {
            args.extend(["-preset", "medium"].map(String::from));
        }
        args.extend([
            "-b:v".into(), format!("{v:.0}k"), "-maxrate".into(), format!("{v:.0}k"), "-bufsize".into(), format!("{:.0}k", v * 2.0),
            "-pix_fmt".into(), "yuv420p".into(), "-c:a".into(), "aac".into(), "-b:a".into(), format!("{audio_kbps:.0}k"),
            "-movflags".into(), "+faststart".into(),
        ]);
        let _ = std::fs::remove_file(&out);
        run(args, &out, dur, progress)?;
        let got = std::fs::metadata(&out).map(|m| m.len()).unwrap_or(u64::MAX) as f64 / 1_048_576.0;
        if got <= max_mb {
            return Ok(DiscordFile { path: out, height, compressed: true });
        }
    }
    bail!("Impossible de passer sous {max_mb:.0} Mo : rogne le clip dans le Studio.")
}

/// Réencode pour tenir sous `max_mb` Mo (limite d'envoi de Discord).
pub fn compress(source: &Path, max_mb: f64, progress: &dyn Fn(f64)) -> Result<PathBuf> {
    let dur = ffmpeg::duration(source).context("Durée inconnue")?.max(1.0);
    let audio_kbps = 96.0;
    let video_kbps = ((max_mb * 8192.0 * 0.92) / dur - audio_kbps).max(250.0);
    let height = if video_kbps < 1500.0 { 720 } else { 1080 };
    let out = output_next_to(source, "(discord)");
    let enc = ffmpeg::pick_encoder("auto");
    let mut args: Vec<String> = vec!["-i".into(), source.to_string_lossy().into(), "-map".into(), "0:v:0".into(), "-map".into(), "0:a:0?".into()];
    args.extend(["-vf".into(), format!("scale=-2:'min({height},ih)'"), "-c:v".into(), enc.clone()]);
    if enc.contains("nvenc") {
        args.extend(["-preset", "p6", "-rc", "cbr"].map(String::from));
    }
    args.extend([
        "-b:v".into(), format!("{video_kbps:.0}k"), "-maxrate".into(), format!("{video_kbps:.0}k"),
        "-bufsize".into(), format!("{:.0}k", video_kbps * 2.0), "-pix_fmt".into(), "yuv420p".into(),
        "-c:a".into(), "aac".into(), "-b:a".into(), format!("{audio_kbps:.0}k"), "-movflags".into(), "+faststart".into(),
    ]);
    run(args, &out, dur, progress)?;
    Ok(out)
}

// ---------------------------------------------------------------------------
// Studio : montage façon CapCut (morceaux, vitesse, volumes par piste, transitions,
// musique de fond, modèles d'export avec taille maximale)
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, serde::Deserialize)]
#[serde(default)]
pub struct Piece {
    pub path: String,
    pub start: f64,
    pub end: f64,
    /// 0.25 à 4
    pub speed: f64,
    /// Volume général du morceau (1 = 100 %, jusqu'à 2)
    pub volume: f64,
    /// Volume de chaque piste séparée du clip (son du PC, micro, Discord…), dans l'ordre du fichier
    /// après la piste de mixage. Vide : la piste de mixage est utilisée telle quelle.
    pub track_volumes: Vec<f64>,
    /// Transition vers le morceau suivant : none, fade, black, white, zoom, slide, wipe
    pub transition: String,
    pub transition_dur: f64,
    /// Identifiant du clip d'origine (pour retrouver ses touches clavier / souris)
    pub clip_id: String,
    /// Arrêt sur image : durée tenue (l'image est celle de `start`) ; 0 = morceau normal
    pub freeze: f64,
    /// Réglages couleur [luminosité, contraste, saturation] (1 = neutre, mêmes formules que les filtres CSS)
    pub color: Vec<f64>,
    /// Vitesse variable (ralentis et accélérés des kills) : [(début, fin, vitesse)] en temps source, à la
    /// suite, de `start` à `end`. Vide : `speed` partout. Un seul passage de décodage par morceau à l'export.
    pub segs: Vec<(f64, f64, f64)>,
}

impl Default for Piece {
    fn default() -> Self {
        Self {
            freeze: 0.0,
            color: Vec::new(),
            clip_id: String::new(),
            path: String::new(),
            start: 0.0,
            end: 0.0,
            speed: 1.0,
            volume: 1.0,
            track_volumes: Vec::new(),
            transition: "none".into(),
            transition_dur: 0.5,
            segs: Vec::new(),
        }
    }
}

/// Musique de fond (bouclée si elle est plus courte que le montage).
#[derive(Clone, Debug, serde::Deserialize, Default)]
#[serde(default)]
pub struct Music {
    pub path: String,
    pub volume: f64,
    /// Début dans le fichier de musique (secondes)
    pub offset: f64,
    /// Position de départ dans le montage (secondes)
    pub at: f64,
    pub fade_in: f64,
    pub fade_out: f64,
    /// Baisse automatiquement la musique quand le jeu ou la voix parlent fort
    pub duck: bool,
}

#[derive(Clone, Debug, serde::Deserialize)]
#[serde(default)]
pub struct StudioOptions {
    pub title: String,
    /// "16:9", "9:16-blur", "9:16-crop"
    pub format: String,
    /// 1080 ou 720 (720/480 possibles automatiquement avec une taille maximale)
    pub height: u32,
    /// Images par seconde ; 0 = comme la source (60 au plus)
    pub fps: u32,
    /// Taille maximale du fichier en Mo (modèles Discord) ; 0 = pas de limite
    pub max_mb: f64,
    pub music: Option<Music>,
    /// Incruste les touches clavier / souris enregistrées avec les clips
    pub keys: bool,
    /// Langue des noms de touches incrustés ("fr" : Maj, Espace ; "en" : Shift, Space)
    pub lang: String,
}

impl Default for StudioOptions {
    fn default() -> Self {
        Self { title: "Montage".into(), format: "16:9".into(), height: 1080, fps: 0, max_mb: 0.0, music: None, keys: false, lang: "fr".into() }
    }
}

/// Intervalles d'appui de chaque touche, en temps du montage (vitesse et transitions comprises).
pub(crate) fn key_intervals(pieces: &[Piece]) -> std::collections::BTreeMap<u16, Vec<(f64, f64)>> {
    let trans = transition_durations(pieces);
    let mut out: std::collections::BTreeMap<u16, Vec<(f64, f64)>> = Default::default();
    let mut offset = 0.0;
    for (k, p) in pieces.iter().enumerate() {
        if k > 0 {
            offset -= trans[k];
        }
        if !p.clip_id.is_empty() {
            let mut events = crate::inputs::for_clip(&p.clip_id);
            events.sort_by(|a, b| a.t.total_cmp(&b.t));
            let mut down: std::collections::HashMap<u16, f64> = Default::default();
            let mut push = |key: u16, a: f64, b: f64| {
                let (a, b) = (a.max(p.start), b.min(p.end));
                if b > a {
                    let (oa, ob) = (offset + src_to_out(p, a), offset + src_to_out(p, b));
                    out.entry(key).or_default().push((oa, ob.max(oa + 0.05)));
                }
            };
            for e in &events {
                if e.down {
                    down.entry(e.key).or_insert(e.t);
                } else if let Some(t0) = down.remove(&e.key) {
                    push(e.key, t0, e.t);
                }
            }
            for (key, t0) in down {
                push(key, t0, p.end);
            }
        }
        offset += out_len(p);
    }
    out
}

/// Filtres FFmpeg qui dessinent le clavier et la souris (même disposition que l'appli).
fn keys_filters(pieces: &[Piece], w: u32, h: u32, english: bool) -> Option<String> {
    let intervals = key_intervals(pieces);
    if intervals.is_empty() {
        return None;
    }
    let font = ["C:/Windows/Fonts/segoeuib.ttf", "C:/Windows/Fonts/arialbd.ttf"].into_iter().find(|f| Path::new(f).is_file());
    let base = w.min(h) as f64;
    let u = (base * 0.042).round();
    let gap = (u * 0.13).round();
    let pad = (u * 0.33).round();
    let margin = (base * 0.03).round();
    let rows: [&[(u16, &str, f64)]; 3] = [
        &[(0x09, "Tab", 1.4), (0x51, "Q", 1.0), (0x57, "W", 1.0), (0x45, "E", 1.0), (0x52, "R", 1.0)],
        &[(0xA0, "Maj", 1.4), (0x41, "A", 1.0), (0x53, "S", 1.0), (0x44, "D", 1.0), (0x46, "F", 1.0)],
        &[(0xA2, "Ctrl", 1.4), (0x20, "Espace", 3.1), (0x43, "C", 1.0)],
    ];
    let row_w = |r: &[(u16, &str, f64)]| r.iter().map(|k| k.2 * u).sum::<f64>() + gap * (r.len() as f64 - 1.0);
    let board_w = rows.iter().map(|r| row_w(r)).fold(0.0, f64::max);
    let (mouse_w, mouse_h) = ((1.5 * u).round(), (2.25 * u).round());
    let panel_h = 3.0 * u + 2.0 * gap + 2.0 * pad;
    let panel_w = pad + board_w + 3.0 * gap + mouse_w + pad;
    let (x0, y0) = (margin, h as f64 - margin - panel_h);
    let enable = |key: u16| -> Option<String> {
        intervals.get(&key).map(|list| list.iter().map(|(a, b)| format!("between(t,{a:.3},{b:.3})")).collect::<Vec<_>>().join("+"))
    };
    let mut f: Vec<String> = Vec::new();
    let boxf = |x: f64, y: f64, bw: f64, bh: f64, color: &str, en: Option<&str>| {
        let mut s = format!("drawbox=x={x:.0}:y={y:.0}:w={bw:.0}:h={bh:.0}:color={color}:t=fill");
        if let Some(e) = en {
            s.push_str(&format!(":enable='{e}'"));
        }
        s
    };
    let textf = |label: &str, x: f64, y: f64, bw: f64, bh: f64, color: &str, size: f64, en: Option<&str>| -> Option<String> {
        let font = font?;
        let mut s = format!(
            "drawtext=fontfile='{}':text='{label}':fontsize={size:.0}:fontcolor={color}:x={x:.0}+({bw:.0}-tw)/2:y={y:.0}+({bh:.0}-th)/2",
            font.replace(':', "\\:")
        );
        if let Some(e) = en {
            s.push_str(&format!(":enable='{e}'"));
        }
        Some(s)
    };
    f.push(boxf(x0, y0, panel_w, panel_h, "black@0.42", None));
    for (r, row) in rows.iter().enumerate() {
        let mut x = x0 + pad;
        let y = y0 + pad + r as f64 * (u + gap);
        for (key, label, kw) in row.iter() {
            let label = match (*label, english) {
                ("Maj", true) => "Shift",
                ("Espace", true) => "Space",
                (l, _) => l,
            };
            let bw = kw * u;
            let size = if label.len() > 2 { u * 0.3 } else { u * 0.4 };
            f.push(boxf(x, y, bw, u, "white@0.14", None));
            f.extend(textf(label, x, y, bw, u, "white@0.9", size, None));
            if let Some(e) = enable(*key) {
                f.push(boxf(x, y, bw, u, "white@0.95", Some(&e)));
                f.extend(textf(label, x, y, bw, u, "0x0b0c0f", size, Some(&e)));
            }
            x += bw + gap;
        }
    }
    // Souris : corps, boutons gauche / droit, molette
    let mx = x0 + pad + board_w + 3.0 * gap;
    let my = y0 + panel_h - pad - mouse_h;
    let half = (mouse_w - gap) / 2.0;
    let bh = (mouse_h * 0.42).round();
    f.push(boxf(mx, my, mouse_w, mouse_h, "white@0.1", None));
    for (key, bx) in [(0x01u16, mx), (0x02u16, mx + half + gap)] {
        f.push(boxf(bx, my, half, bh, "white@0.16", None));
        if let Some(e) = enable(key) {
            f.push(boxf(bx, my, half, bh, "white@0.95", Some(&e)));
        }
    }
    if let Some(e) = enable(0x04) {
        f.push(boxf(mx + mouse_w / 2.0 - gap, my + gap, gap * 2.0, bh - 2.0 * gap, "0x0b0c0f", Some(&e)));
    }
    Some(f.join(","))
}

pub(crate) fn atempo(speed: f64) -> String {
    // atempo accepte 0,5 à 2 : on enchaîne pour aller au-delà
    let mut parts = Vec::new();
    let mut s = speed;
    while s < 0.5 {
        parts.push("atempo=0.5".to_string());
        s /= 0.5;
    }
    while s > 2.0 {
        parts.push("atempo=2.0".to_string());
        s /= 2.0;
    }
    if (s - 1.0).abs() > 1e-6 {
        parts.push(format!("atempo={s:.5}"));
    }
    if parts.is_empty() { "anull".into() } else { parts.join(",") }
}

/// Cadence de sortie du Studio : la vraie cadence de la source (images comptées), ramenée à 60 i/s
/// au plus par un diviseur exact (100 → 50, 144 → 48, 120 → 60) pour garder un mouvement régulier.
pub(crate) fn frame_rate(path: &Path) -> f64 {
    let Ok(tools) = ffmpeg::tools() else { return 60.0 };
    let out = util::command(&tools.ffprobe)
        .args(["-v", "error", "-select_streams", "v:0", "-count_packets", "-show_entries", "stream=avg_frame_rate,nb_read_packets:format=duration", "-of", "json"])
        .arg(path)
        .output()
        .map(|o| o.stdout)
        .unwrap_or_default();
    let json: serde_json::Value = serde_json::from_slice(&out).unwrap_or_default();
    let num = |v: &serde_json::Value| v.as_str().and_then(|s| s.parse::<f64>().ok());
    let frames = json["streams"][0]["nb_read_packets"].as_str().and_then(|s| s.parse::<f64>().ok()).unwrap_or(0.0);
    let dur = num(&json["format"]["duration"]).unwrap_or(0.0);
    let avg = json["streams"][0]["avg_frame_rate"].as_str().and_then(|s| {
        let mut it = s.split('/').map(|x| x.parse::<f64>().unwrap_or(0.0));
        let (a, b) = (it.next()?, it.next().unwrap_or(1.0));
        (b > 0.0).then_some(a / b)
    });
    let real = if frames > 10.0 && dur > 0.2 { frames / dur } else { avg.unwrap_or(60.0) };
    studio_fps(real) as f64
}

/// Plus grande cadence ≤ 60 qui divise exactement la cadence source (arrondie).
pub fn studio_fps(real: f64) -> u32 {
    let src = real.round().max(1.0) as u32;
    if src <= 60 {
        return src.max(24);
    }
    (2..=10).map(|k| src / k).find(|&f| f <= 60 && f >= 24 && src % (src / f).max(1) == 0).unwrap_or(60)
}

/// Durée de sortie d'un morceau (après vitesse ; arrêt sur image : durée tenue).
pub fn out_len(p: &Piece) -> f64 {
    if p.freeze > 0.0 {
        return p.freeze.max(0.1);
    }
    if !p.segs.is_empty() {
        return p.segs.iter().map(|&(a, b, v)| (b - a).max(0.0) / v.clamp(0.25, 4.0)).sum::<f64>().max(0.05);
    }
    (p.end - p.start).max(0.05) / p.speed.clamp(0.25, 4.0)
}

/// Temps de sortie (depuis le début du morceau) d'un instant `s` du clip source.
pub fn src_to_out(p: &Piece, s: f64) -> f64 {
    if p.segs.is_empty() {
        return (s - p.start) / p.speed.clamp(0.25, 4.0);
    }
    let mut acc = 0.0;
    for &(a, b, v) in &p.segs {
        let v = v.clamp(0.25, 4.0);
        if s <= b {
            return acc + (s.max(a) - a) / v;
        }
        acc += (b - a).max(0.0) / v;
    }
    acc
}

/// Durée de chaque transition : `d[k]` = chevauchement entre le morceau k-1 et le morceau k
/// (`d[0]` = 0). Même calcul que l'interface (studio.svelte.ts) : 0,1 à 2 s, et jamais plus de
/// la moitié de chacun des deux morceaux.
pub fn transition_durations(pieces: &[Piece]) -> Vec<f64> {
    let lens: Vec<f64> = pieces.iter().map(out_len).collect();
    let mut d = vec![0.0; pieces.len()];
    for k in 1..pieces.len() {
        let p = &pieces[k - 1];
        if p.transition.is_empty() || p.transition == "none" {
            continue;
        }
        d[k] = p.transition_dur.clamp(0.1, 2.0).min(lens[k - 1] * 0.5).min(lens[k] * 0.5);
    }
    d
}

/// Durée totale du montage (les transitions font se chevaucher les morceaux).
pub fn total_duration(pieces: &[Piece]) -> f64 {
    let d = transition_durations(pieces);
    pieces.iter().map(out_len).sum::<f64>() - d.iter().sum::<f64>()
}

pub(crate) fn xfade_name(t: &str) -> &'static str {
    match t {
        "black" => "fadeblack",
        "white" => "fadewhite",
        "zoom" => "zoomin",
        "slide" => "slideleft",
        "wipe" => "wipeleft",
        _ => "fade",
    }
}

/// Débits minimaux pour une image correcte (H.264) selon la hauteur.
pub(crate) const LADDER: [(u32, f64); 3] = [(1080, 4500.0), (720, 2000.0), (480, 850.0)];
pub(crate) const STUDIO_AUDIO_KBPS: f64 = 128.0;

/// Pour une taille maximale : (hauteur, i/s, débit vidéo en kb/s) ou une erreur claire.
pub fn size_plan(total: f64, max_mb: f64, max_height: u32, fps: u32) -> Result<(u32, u32, f64)> {
    let video_kbps = (max_mb * 8192.0 * 0.92) / total.max(0.5) - STUDIO_AUDIO_KBPS;
    let Some(&(height, _)) = LADDER.iter().find(|(h, min)| *h <= max_height.max(480) && video_kbps >= *min) else {
        let max_secs = (max_mb * 8192.0 * 0.92) / (LADDER[2].1 + STUDIO_AUDIO_KBPS);
        bail!(
            "Le montage dure {:.0} s : trop long pour tenir dans {:.0} Mo avec une image correcte. Raccourcis-le à {:.0} s maximum.",
            total, max_mb, max_secs.floor()
        );
    };
    // Petit budget : 30 i/s donnent une image plus nette que 60 i/s floues
    let fps = if video_kbps < 3500.0 { fps.min(30) } else { fps };
    Ok((height, fps, video_kbps))
}

/// Exporte le montage dans un nouveau fichier (les originaux ne sont jamais modifiés).
pub fn studio_export(pieces: &[Piece], opts: &StudioOptions, folder: &Path, progress: &dyn Fn(f64)) -> Result<PathBuf> {
    if pieces.is_empty() {
        bail!("Le montage est vide.");
    }
    // Chaque morceau doit rester dans son clip : un morceau vide bloquerait toute la suite de l'image
    // (vu le 2026-11-13 : vidéo arrêtée à la 1re transition, le son continuait)
    let mut fixed: Vec<Piece> = Vec::with_capacity(pieces.len());
    for p in pieces {
        if !Path::new(&p.path).is_file() {
            bail!("Clip introuvable : {} (déplacé ou supprimé ?)", p.path);
        }
        let dur = ffmpeg::duration(Path::new(&p.path)).unwrap_or(f64::MAX);
        let mut q = p.clone();
        q.start = q.start.clamp(0.0, (dur - 0.1).max(0.0));
        q.end = q.end.min(dur);
        if q.end - q.start < 0.1 {
            bail!("Un morceau est en dehors de son clip ({:.1} s pour un clip de {dur:.1} s).", p.start);
        }
        fixed.push(q);
    }
    let pieces = &fixed[..];
    let total = total_duration(pieces);
    let src_fps = frame_rate(Path::new(&pieces[0].path)) as u32;
    let mut fps = if opts.fps > 0 { opts.fps.clamp(24, 60) } else { src_fps };
    let mut height = if opts.height >= 1080 { 1080 } else { 720 };
    let mut target_kbps: Option<f64> = None;
    if opts.max_mb > 0.0 {
        let (h, f, kbps) = size_plan(total, opts.max_mb, height, fps)?;
        height = h;
        fps = f;
        target_kbps = Some(kbps);
    }
    std::fs::create_dir_all(folder)?;
    let stamp = chrono::Local::now().format("%Y-%m-%d %Hh%M-%S");
    let out = util::reserve_file(folder, &util::safe_name(&format!("{} {stamp}", opts.title)), "mp4")?;
    // Deux essais avec une taille maximale : si le premier dépasse (rare), 15 % de débit en moins
    let factors: &[f64] = if target_kbps.is_some() { &[1.0, 0.85] } else { &[1.0] };
    let mut last_err = None;
    for &factor in factors {
        let args = studio_args(pieces, opts, height, fps, target_kbps.map(|k| k * factor), total);
        let result = run(args.clone(), &out, total, progress);
        // Fichier du graphe de filtres : plus utile une fois FFmpeg terminé
        if let Some(i) = args.iter().position(|a| a == "-/filter_complex") {
            let _ = std::fs::remove_file(&args[i + 1]);
        }
        match result {
            Ok(()) => {
                let mb = std::fs::metadata(&out).map(|m| m.len()).unwrap_or(0) as f64 / 1_048_576.0;
                if opts.max_mb <= 0.0 || mb <= opts.max_mb {
                    return Ok(out);
                }
                log::warn!("montage de {mb:.1} Mo pour {} Mo max : nouvel essai", opts.max_mb);
                last_err = Some(anyhow::anyhow!("Impossible de passer sous {:.0} Mo : raccourcis le montage.", opts.max_mb));
            }
            Err(e) => {
                last_err = Some(e);
                break;
            }
        }
    }
    let _ = std::fs::remove_file(&out);
    Err(last_err.unwrap_or_else(|| anyhow::anyhow!("Export impossible")))
}

fn studio_args(pieces: &[Piece], opts: &StudioOptions, height: u32, fps: u32, kbps: Option<f64>, total: f64) -> Vec<String> {
    let vertical = opts.format.starts_with("9:16");
    // Dimensions paires obligatoires en H.264 (480p : 852 et non 853)
    let long = (height * 16 / 9) / 2 * 2;
    let (w, h) = if vertical { (height, long) } else { (long, height) };
    let trans = transition_durations(pieces);
    let mut args: Vec<String> = Vec::new();
    let mut graph = String::new();
    const AFMT: &str = "aresample=48000,aformat=sample_fmts=fltp:sample_rates=48000:channel_layouts=stereo";

    for (i, p) in pieces.iter().enumerate() {
        let speed = p.speed.clamp(0.25, 4.0);
        let len = (p.end - p.start).max(0.05);
        let olen = out_len(p);
        args.extend(["-ss".into(), format!("{:.3}", p.start.max(0.0)), "-t".into(), format!("{len:.3}"), "-i".into(), p.path.clone()]);
        let frame = match opts.format.as_str() {
            "9:16-crop" => format!("crop=ih*9/16:ih,scale={w}:{h}:flags=lanczos"),
            "9:16-blur" => format!(
                "split[b{i}][f{i}];[b{i}]scale={w}:{h}:force_original_aspect_ratio=increase,crop={w}:{h},boxblur=24:2[bg{i}];[f{i}]scale={w}:-2:flags=lanczos[fg{i}];[bg{i}][fg{i}]overlay=(W-w)/2:(H-h)/2"
            ),
            _ => format!("scale={w}:{h}:force_original_aspect_ratio=decrease:flags=lanczos,pad={w}:{h}:(ow-iw)/2:(oh-ih)/2"),
        };
        // Durée exacte (complétée par la dernière image si besoin) : indispensable pour caler les transitions
        graph.push_str(&format!(
            "[{i}:v]setpts=(PTS-STARTPTS)/{speed:.5},{frame},fps={fps},format=yuv420p,setsar=1,tpad=stop_mode=clone:stop_duration=1,trim=duration={olen:.4},setpts=PTS-STARTPTS,settb=AVTB[v{i}];"
        ));
        // Son : piste de mixage, ou pistes séparées avec leur volume
        let n_audio = ffmpeg::streams(Path::new(&p.path)).iter().filter(|s| s.kind == "audio").count();
        let vol = p.volume.clamp(0.0, 2.0);
        let tail = format!("asetpts=PTS-STARTPTS,{},volume={vol:.3},apad,atrim=0:{olen:.4},asetpts=PTS-STARTPTS[a{i}];", atempo(speed));
        let separate = n_audio > 1 && !p.track_volumes.is_empty() && n_audio > p.track_volumes.len();
        if n_audio == 0 || vol <= 0.001 {
            graph.push_str(&format!("anullsrc=r=48000:cl=stereo,atrim=0:{olen:.4}[a{i}];"));
        } else if separate {
            let used: Vec<(usize, f64)> = p
                .track_volumes
                .iter()
                .enumerate()
                .map(|(k, v)| (k + 1, v.clamp(0.0, 2.0)))
                .filter(|(_, v)| *v > 0.001)
                .collect();
            if used.is_empty() {
                graph.push_str(&format!("anullsrc=r=48000:cl=stereo,atrim=0:{olen:.4}[a{i}];"));
            } else {
                let mut labels = String::new();
                for (s, v) in &used {
                    graph.push_str(&format!("[{i}:a:{s}]{AFMT},volume={v:.3}[t{i}x{s}];"));
                    labels.push_str(&format!("[t{i}x{s}]"));
                }
                if used.len() == 1 {
                    graph.push_str(&format!("{labels}{tail}"));
                } else {
                    graph.push_str(&format!("{labels}amix=inputs={}:normalize=0:duration=longest,{tail}", used.len()));
                }
            }
        } else {
            graph.push_str(&format!("[{i}:a:0]{AFMT},{tail}"));
        }
    }

    // Enchaînement : fondu croisé (xfade + acrossfade) ou collage simple (concat)
    let mut cur_v = "v0".to_string();
    let mut cur_a = "a0".to_string();
    let mut cur_len = out_len(&pieces[0]);
    for k in 1..pieces.len() {
        let d = trans[k];
        let (nv, na) = (format!("xv{k}"), format!("xa{k}"));
        if d > 0.0 {
            graph.push_str(&format!(
                "[{cur_v}][v{k}]xfade=transition={}:duration={d:.4}:offset={:.4}[{nv}];[{cur_a}][a{k}]acrossfade=d={d:.4}:c1=tri:c2=tri[{na}];",
                xfade_name(&pieces[k - 1].transition),
                (cur_len - d).max(0.0)
            ));
            cur_len += out_len(&pieces[k]) - d;
        } else {
            graph.push_str(&format!("[{cur_v}][{cur_a}][v{k}][a{k}]concat=n=2:v=1:a=1[{nv}][{na}];"));
            cur_len += out_len(&pieces[k]);
        }
        cur_v = nv;
        cur_a = na;
    }

    // Musique de fond
    let limiter = "alimiter=limit=0.97:level=0:latency=1";
    let music = opts.music.as_ref().filter(|m| !m.path.is_empty() && Path::new(&m.path).is_file() && m.volume > 0.001);
    let mlen = music.map(|m| (total - m.at.max(0.0)).max(0.0)).unwrap_or(0.0);
    if let (Some(m), true) = (music, mlen > 0.2) {
        let idx = pieces.len();
        let offset = m.offset.max(0.0);
        args.extend(["-stream_loop".into(), "-1".into(), "-t".into(), format!("{:.3}", offset + mlen + 1.0), "-i".into(), m.path.clone()]);
        let mut chain = format!("[{idx}:a]atrim=start={offset:.3},asetpts=PTS-STARTPTS,{AFMT},volume={:.3}", m.volume.clamp(0.0, 2.0));
        if m.fade_in > 0.01 {
            chain.push_str(&format!(",afade=t=in:st=0:d={:.3}", m.fade_in.min(mlen / 2.0)));
        }
        if m.fade_out > 0.01 {
            let fo = m.fade_out.min(mlen / 2.0);
            chain.push_str(&format!(",afade=t=out:st={:.3}:d={fo:.3}", mlen - fo));
        }
        let delay = (m.at.max(0.0) * 1000.0).round() as u64;
        chain.push_str(&format!(",atrim=0:{mlen:.4},adelay={delay}:all=1,apad,atrim=0:{total:.4}[mus];"));
        graph.push_str(&chain);
        if m.duck {
            graph.push_str(&format!(
                "[{cur_a}]asplit=2[amain][asc];[mus][asc]sidechaincompress=threshold=0.04:ratio=8:attack=30:release=400[musd];[amain][musd]amix=inputs=2:duration=first:normalize=0,{limiter}[aout]"
            ));
        } else {
            graph.push_str(&format!("[{cur_a}][mus]amix=inputs=2:duration=first:normalize=0,{limiter}[aout]"));
        }
    } else {
        graph.push_str(&format!("[{cur_a}]{limiter}[aout]"));
    }

    // Touches clavier / souris incrustées (option)
    if opts.keys
        && let Some(kf) = keys_filters(pieces, w, h, opts.lang == "en")
    {
        graph.push_str(&format!(";[{cur_v}]{kf}[vkeys]"));
        cur_v = "vkeys".into();
    }

    // Le graphe passe par un fichier : avec beaucoup de morceaux ou de touches, il dépasserait
    // la longueur maximale d'une ligne de commande Windows (32 767 caractères)
    let graph_file = util::data_dir().join("cache").join(format!("graphe-{}.txt", crate::library::new_id()));
    let _ = std::fs::create_dir_all(graph_file.parent().unwrap_or(Path::new(".")));
    if std::fs::write(&graph_file, &graph).is_ok() {
        args.extend(["-/filter_complex".into(), graph_file.to_string_lossy().to_string()]);
    } else {
        args.extend(["-filter_complex".into(), graph]);
    }
    args.extend(["-map".into(), format!("[{cur_v}]"), "-map".into(), "[aout]".into()]);
    args.extend(["-t".into(), format!("{total:.4}")]);
    match kbps {
        Some(v) => {
            let enc = ffmpeg::pick_encoder("auto");
            args.extend(["-c:v".into(), enc.clone()]);
            if enc.contains("nvenc") {
                args.extend(["-preset", "p6", "-tune", "hq", "-rc", "cbr", "-multipass", "fullres"].map(String::from));
            } else if enc == "libx264" {
                args.extend(["-preset", "medium"].map(String::from));
            }
            args.extend([
                "-b:v".into(), format!("{v:.0}k"), "-maxrate".into(), format!("{v:.0}k"), "-bufsize".into(), format!("{:.0}k", v * 2.0),
                "-pix_fmt".into(), "yuv420p".into(), "-c:a".into(), "aac".into(), "-b:a".into(), format!("{STUDIO_AUDIO_KBPS:.0}k"),
                "-movflags".into(), "+faststart".into(),
            ]);
        }
        None => args.extend(encoder_args()),
    }
    args.extend(["-metadata".into(), format!("title={}", opts.title)]);
    args
}

#[cfg(test)]
mod tests {
    use super::*;

    fn piece(start: f64, end: f64, speed: f64, transition: &str, dur: f64) -> Piece {
        Piece { path: String::new(), start, end, speed, transition: transition.into(), transition_dur: dur, ..Default::default() }
    }

    #[test]
    fn durees_avec_transitions() {
        // 10 s + 4 s (2× plus rapide : 8 s de source) + 6 s, fondu de 1 s puis collage simple
        let p = vec![piece(0.0, 10.0, 1.0, "fade", 1.0), piece(2.0, 10.0, 2.0, "none", 0.5), piece(0.0, 6.0, 1.0, "none", 0.5)];
        let d = transition_durations(&p);
        assert_eq!(d, vec![0.0, 1.0, 0.0]);
        assert!((total_duration(&p) - 19.0).abs() < 1e-9);
    }

    #[test]
    fn transition_bornee_par_les_morceaux() {
        // Le 2e morceau ne dure que 0,6 s : la transition ne peut pas dépasser 0,3 s
        let p = vec![piece(0.0, 10.0, 1.0, "zoom", 2.0), piece(0.0, 0.6, 1.0, "none", 0.5)];
        assert!((transition_durations(&p)[1] - 0.3).abs() < 1e-9);
    }

    #[test]
    fn cadence_studio_reguliere() {
        assert_eq!(studio_fps(100.0), 50);
        assert_eq!(studio_fps(144.0), 48);
        assert_eq!(studio_fps(120.0), 60);
        assert_eq!(studio_fps(165.0), 55);
        assert_eq!(studio_fps(59.94), 60);
        assert_eq!(studio_fps(50.0), 50);
        assert_eq!(studio_fps(30.0), 30);
    }

    #[test]
    fn plan_discord() {
        // 30 s dans 20 Mo : 1080p ; 60 s dans 10 Mo : 480p à 30 i/s ; 5 min dans 10 Mo : refus clair
        assert_eq!(size_plan(30.0, 20.0, 1080, 60).unwrap().0, 1080);
        let (h, f, _) = size_plan(60.0, 10.0, 1080, 60).unwrap();
        assert_eq!((h, f), (480, 30));
        assert!(size_plan(300.0, 10.0, 1080, 60).is_err());
    }
}
