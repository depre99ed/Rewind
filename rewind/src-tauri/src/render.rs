//! Export du Studio v2 : piste principale + zoom animé + calques (texte, image, GIF, vidéo, flou) +
//! touches clavier / souris + plusieurs pistes son, en un seul passage FFmpeg.
//!
//! L'interface calcule tout ce qui est « temps » et « position » (studio/export.ts) : points clés déjà
//! échantillonnés, en temps du montage et en pixels de sortie. Ici on traduit en graphe FFmpeg.
//!
//! Pièges FFmpeg (testés le 2026-11-15 sur la version fournie) :
//! - une image dont la TAILLE change d'une image à l'autre (scale eval=frame) casse le filtre suivant
//!   (« Frame size changed without filter graph reinitialization ») : la taille et la rotation animées
//!   passent donc par `perspective` (sense=destination, eval=frame) dans un cadre carré fixe ;
//! - `perspective` n'a pas de variable de temps : temps = début du calque + n° d'image / i/s ;
//! - zoom animé et tremblement passent par `perspective` avec `enable` : seules les images concernées sont
//!   recalculées (avant : `zoompan` et crop + scale sur TOUTE la vidéo, même hors effet). Temps d'une image
//!   dans `perspective` = (in - 1) / i/s (vérifié le 2026-11-23 : `in` vaut 1 pour la première image) ;
//! - ralentis et accélérés : un seul décodage par morceau (setpts avec une courbe), au lieu d'un morceau
//!   (une lecture du fichier) par marche de vitesse, ce qui rendait l'export très lent.

use crate::editor::{self, Piece, LADDER, STUDIO_AUDIO_KBPS};
use crate::{ffmpeg, util};
use anyhow::{Result, bail};
use serde::Deserialize;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, Deserialize, Default)]
#[serde(default)]
pub struct ZoomKf {
    pub t: f64,
    pub z: f64,
    pub x: f64,
    pub y: f64,
}

/// Point clé d'un calque : centre (px), échelle relative à la taille max (≤ 1), rotation (degrés).
#[derive(Clone, Debug, Deserialize, Default)]
#[serde(default)]
pub struct LayerKf {
    pub t: f64,
    pub x: f64,
    pub y: f64,
    pub s: f64,
    pub r: f64,
}

#[derive(Clone, Debug, Deserialize, Default)]
#[serde(default)]
pub struct LayerSpec {
    /// image, gif, video, blur
    pub kind: String,
    pub path: String,
    pub at: f64,
    pub dur: f64,
    /// Taille du calque à son échelle maximale (pixels de sortie)
    pub w: f64,
    pub h: f64,
    pub kf: Vec<LayerKf>,
    pub opacity: f64,
    /// Opacité animée (temps du montage) : seulement si l'utilisateur l'a animée
    pub op_kf: Vec<(f64, f64)>,
    pub fade_in: f64,
    pub fade_out: f64,
    /// Fenêtres d'affichage en plus (machine à écrire) : [(début, fin)] en temps du montage
    pub windows: Vec<(f64, f64)>,
    // Vidéo
    pub src_start: f64,
    pub speed: f64,
    pub volume: f64,
    // Flou : « blur » ou « pixel », intensité
    pub mode: String,
    pub strength: f64,
}

#[derive(Clone, Debug, Deserialize, Default)]
#[serde(default)]
pub struct AudioSpec {
    pub path: String,
    pub at: f64,
    pub src_start: f64,
    pub dur: f64,
    pub volume: f64,
    /// Volume dans le temps (temps local de l'élément)
    pub kf: Vec<(f64, f64)>,
    pub fade_in: f64,
    pub fade_out: f64,
    pub duck: bool,
}

#[derive(Clone, Debug, Deserialize, Default)]
#[serde(default)]
pub struct KeySprite {
    pub key: u16,
    pub path: String,
}

#[derive(Clone, Debug, Deserialize, Default)]
#[serde(default)]
pub struct KeysSpec {
    pub base: String,
    pub sprites: Vec<KeySprite>,
    /// Coin haut gauche (pixels de sortie)
    pub x: f64,
    pub y: f64,
}

/// Effet appliqué à toute l'image (vidéo + calques, pas les touches) pendant [at, at + dur] :
/// « bw » noir et blanc, « invert » négatif, « glitch » couleurs décalées, « shake » tremblement.
/// Les mêmes formules sont dans l'aperçu (Preview.svelte, fxStyle).
#[derive(Clone, Debug, Deserialize, Default)]
#[serde(default)]
pub struct EffectSpec {
    pub kind: String,
    pub at: f64,
    pub dur: f64,
    /// 0..1
    pub strength: f64,
}

#[derive(Clone, Debug, Deserialize, Default)]
#[serde(default)]
pub struct RenderSpec {
    pub title: String,
    /// Taille de composition (paire) ; la sortie peut être réduite pour tenir une taille maximale
    pub w: u32,
    pub h: u32,
    /// fit, blur, crop : comment un clip remplit un cadre d'un autre format
    pub fit: String,
    /// Petit côté voulu en sortie (1080, 720)
    pub short: u32,
    pub fps: u32,
    pub max_mb: f64,
    pub total: f64,
    pub pieces: Vec<Piece>,
    pub zoom: Vec<ZoomKf>,
    pub layers: Vec<LayerSpec>,
    pub audio: Vec<AudioSpec>,
    pub keys: Option<KeysSpec>,
    pub effects: Vec<EffectSpec>,
    /// Fondu au noir (image et son) sur les dernières secondes ; 0 = aucun
    pub end_fade: f64,
}

/// Ordre fixe des effets (le même que l'aperçu) : couleurs d'abord, tremblement en dernier.
const FX_ORDER: [&str; 4] = ["bw", "invert", "glitch", "shake"];

/// Tremblement : part de l'image gardée (1 - 2f) et décalage (temps du montage `t`).
/// Mêmes nombres dans l'aperçu : f = 0,03 × intensité, sinus à 7 Hz (x) et 11 Hz (y).
pub fn shake_crop(w: u32, h: u32, strength: f64, var: &str) -> (u32, u32, String, String) {
    let f = 0.03 * strength.clamp(0.0, 1.0);
    let cw = ((w as f64 * (1.0 - 2.0 * f)) / 2.0).floor() as u32 * 2;
    let ch = ((h as f64 * (1.0 - 2.0 * f)) / 2.0).floor() as u32 * 2;
    let (mx, my) = ((w - cw) as f64 / 2.0, (h - ch) as f64 / 2.0);
    (cw, ch, format!("{}*(1+sin(2*PI*7*{var}))", n(mx)), format!("{}*(1+sin(2*PI*11*{var}+1))", n(my)))
}

/// Filtres des effets, ajoutés à la suite de l'image `cur` ; renvoie la nouvelle étiquette.
fn effect_filters(g: &mut String, effects: &[EffectSpec], w: u32, h: u32, fps: u32, mut cur: String) -> String {
    let mut list: Vec<&EffectSpec> = effects.iter().filter(|e| e.dur > 0.01 && FX_ORDER.contains(&e.kind.as_str())).collect();
    list.sort_by(|a, b| {
        let ia = FX_ORDER.iter().position(|k| *k == a.kind).unwrap_or(9);
        let ib = FX_ORDER.iter().position(|k| *k == b.kind).unwrap_or(9);
        ia.cmp(&ib).then(a.at.total_cmp(&b.at))
    });
    for (i, e) in list.iter().enumerate() {
        let en = format!("between(t\\,{}\\,{})", n(e.at), n(e.at + e.dur));
        let s = e.strength.clamp(0.0, 1.0);
        let out = format!("FX{i}");
        match e.kind.as_str() {
            "bw" => g.push_str(&format!("[{cur}]hue=s={}:enable='{en}'[{out}];", n(1.0 - s))),
            "invert" => g.push_str(&format!("[{cur}]negate=enable='{en}'[{out}];")),
            "glitch" => {
                let a = ((0.02 * s * w as f64).round() as i64).max(1);
                g.push_str(&format!("[{cur}]rgbashift=rh={}:bh={}:edge=smear:enable='{en}'[{out}];", -a, a));
            }
            _ => {
                // Tremblement : la partie gardée qui bouge, agrandie au cadre, calculée seulement pendant l'effet
                let (cw, ch, x, y) = shake_crop(w, h, s, &frame_time(fps));
                g.push_str(&format!("[{cur}]{}[{out}];", warp(&x, &y, &cw.to_string(), &ch.to_string(), "linear", &en)));
            }
        }
        cur = out;
    }
    cur
}

/// Temps (s) de l'image en cours dans `perspective` (pas de variable t ; flux à i/s constant depuis 0).
fn frame_time(fps: u32) -> String {
    format!("((in-1)/{fps})")
}

/// Recadrage animé (x, y, largeur, hauteur de la partie gardée) agrandi au cadre, seulement si `en` :
/// `perspective` fait crop + scale en un filtre, multi-cœur, et ne travaille pas hors de `en`.
fn warp(x: &str, y: &str, cw: &str, ch: &str, interp: &str, en: &str) -> String {
    let (x1, y2) = (format!("{x}+{cw}"), format!("{y}+{ch}"));
    format!(
        "perspective=x0='{x}':y0='{y}':x1='{x1}':y1='{y}':x2='{x}':y2='{y2}':x3='{x1}':y3='{y2}':sense=source:interpolation={interp}:eval=frame:enable='{en}'"
    )
}

/// Instants où une courbe de zoom (t, z) dépasse 1 : fenêtres `enable` (fusionnées).
fn zoom_windows(points: &[(f64, f64)]) -> String {
    let mut wins: Vec<(f64, f64)> = Vec::new();
    for w in points.windows(2) {
        let ((a, za), (b, zb)) = (w[0], w[1]);
        if za.max(zb) <= 1.001 {
            continue;
        }
        match wins.last_mut() {
            Some(last) if a <= last.1 + 1e-3 => last.1 = last.1.max(b),
            _ => wins.push((a, b)),
        }
    }
    if let Some(&(t, z)) = points.last()
        && z > 1.001
    {
        wins.push((t, 1e6));
    }
    if wins.is_empty() {
        return "0".into();
    }
    wins.iter().map(|(a, b)| format!("between(t\\,{}\\,{})", n(*a), n(*b))).collect::<Vec<_>>().join("+")
}

/// Vitesse variable d'un morceau : temps de sortie en fonction du temps source `T` (depuis le début du morceau).
fn speed_curve(p: &Piece) -> String {
    let mut pts: Vec<(f64, f64)> = Vec::new();
    let mut acc = 0.0;
    for &(a, b, v) in &p.segs {
        pts.push((a - p.start, acc));
        acc += (b - a).max(0.0) / v.clamp(0.25, 4.0);
        pts.push((b - p.start, acc));
    }
    piecewise(&pts, "T")
}

const AFMT: &str = "aresample=48000,aformat=sample_fmts=fltp:sample_rates=48000:channel_layouts=stereo";

fn n(v: f64) -> String {
    let s = format!("{v:.4}");
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s.is_empty() || s == "-" || s == "-0" { "0".into() } else { s.into() }
}

/// Courbe linéaire par morceaux (points triés) en expression FFmpeg de la variable `var` ;
/// avant le 1er point / après le dernier : constante.
///
/// Forme en ARBRE (`if(lt(var, milieu), gauche, droite)`, profondeur log2 du nombre de points) : FFmpeg refuse
/// une somme de plus de ~90 segments (« Error when evaluating the expression », vérifié le 2026-11-24), ce
/// qu'atteignent vite une courbe de vitesse, beaucoup de zooms punch ou une image animée.
pub fn piecewise(points: &[(f64, f64)], var: &str) -> String {
    // Points d'instants distincts (un saut : on garde le dernier point du même instant)
    let mut pts: Vec<(f64, f64)> = Vec::with_capacity(points.len());
    for &p in points {
        match pts.last_mut() {
            Some(last) if p.0 - last.0 < 1e-6 => *last = p,
            _ => pts.push(p),
        }
    }
    match pts.len() {
        0 => return "0".into(),
        1 => return n(pts[0].1),
        _ => {}
    }
    if pts.iter().all(|p| (p.1 - pts[0].1).abs() < 1e-6) {
        return n(pts[0].1);
    }
    let (t0, v0) = pts[0];
    let (tn, vn) = pts[pts.len() - 1];
    format!("if(lt({var}\\,{})\\,{}\\,if(gte({var}\\,{})\\,{}\\,{}))", n(t0), n(v0), n(tn), n(vn), seg_tree(&pts, var))
}

/// Arbre de recherche sur les segments de `pts` (au moins 2 points).
fn seg_tree(pts: &[(f64, f64)], var: &str) -> String {
    if pts.len() == 2 {
        let ((a, va), (b, vb)) = (pts[0], pts[1]);
        if (vb - va).abs() < 1e-9 {
            return n(va);
        }
        return format!("({}+{}*({var}-{})/{})", n(va), n(vb - va), n(a), n(b - a));
    }
    let m = pts.len() / 2;
    format!("if(lt({var}\\,{})\\,{}\\,{})", n(pts[m].0), seg_tree(&pts[..=m], var), seg_tree(&pts[m..], var))
}

/// Cadre d'un morceau vers la taille de composition (fit : bandes noires ; blur : fond flou ; crop : recadré).
fn frame_filter(fit: &str, w: u32, h: u32, i: usize) -> String {
    match fit {
        "crop" => format!("scale={w}:{h}:force_original_aspect_ratio=increase:flags=lanczos,crop={w}:{h}"),
        "blur" => format!(
            "split[b{i}][f{i}];[b{i}]scale={w}:{h}:force_original_aspect_ratio=increase,crop={w}:{h},boxblur=24:2[bg{i}];[f{i}]scale={w}:{h}:force_original_aspect_ratio=decrease:flags=lanczos[fg{i}];[bg{i}][fg{i}]overlay=(W-w)/2:(H-h)/2"
        ),
        _ => format!("scale={w}:{h}:force_original_aspect_ratio=decrease:flags=lanczos,pad={w}:{h}:(ow-iw)/2:(oh-ih)/2"),
    }
}

/// Réglages couleur, mêmes formules que les filtres CSS brightness / contrast / saturate de l'aperçu.
fn color_filter(c: &[f64]) -> Option<String> {
    if c.len() < 3 {
        return None;
    }
    let (b, k, s) = (c[0].clamp(0.0, 3.0), c[1].clamp(0.0, 3.0), c[2].clamp(0.0, 3.0));
    if (b - 1.0).abs() < 0.005 && (k - 1.0).abs() < 0.005 && (s - 1.0).abs() < 0.005 {
        return None;
    }
    let mut f = vec!["format=rgb24".to_string()];
    if (b - 1.0).abs() >= 0.005 {
        f.push(format!("lutrgb=r='clip(val*{b:.4}\\,0\\,255)':g='clip(val*{b:.4}\\,0\\,255)':b='clip(val*{b:.4}\\,0\\,255)'"));
    }
    if (k - 1.0).abs() >= 0.005 {
        f.push(format!("lutrgb=r='clip((val-127.5)*{k:.4}+127.5\\,0\\,255)':g='clip((val-127.5)*{k:.4}+127.5\\,0\\,255)':b='clip((val-127.5)*{k:.4}+127.5\\,0\\,255)'"));
    }
    if (s - 1.0).abs() >= 0.005 {
        // Matrice de saturation du W3C (filtre CSS saturate)
        f.push(format!(
            "colorchannelmixer=rr={:.4}:rg={:.4}:rb={:.4}:gr={:.4}:gg={:.4}:gb={:.4}:br={:.4}:bg={:.4}:bb={:.4}",
            0.213 + 0.787 * s, 0.715 - 0.715 * s, 0.072 - 0.072 * s,
            0.213 - 0.213 * s, 0.715 + 0.285 * s, 0.072 - 0.072 * s,
            0.213 - 0.213 * s, 0.715 - 0.715 * s, 0.072 + 0.928 * s
        ));
    }
    f.push("format=yuv420p".into());
    Some(f.join(","))
}

/// Points clés d'un calque → expressions (temps du montage) : x, y, échelle relative, rotation (radians).
fn layer_exprs(l: &LayerSpec, var: &str) -> (String, String, String, String) {
    let kf: Vec<&LayerKf> = l.kf.iter().collect();
    let pts = |f: &dyn Fn(&LayerKf) -> f64| kf.iter().map(|k| (k.t, f(k))).collect::<Vec<_>>();
    if kf.is_empty() {
        return ("0".into(), "0".into(), "1".into(), "0".into());
    }
    (
        piecewise(&pts(&|k| k.x), var),
        piecewise(&pts(&|k| k.y), var),
        piecewise(&pts(&|k| k.s.clamp(0.01, 1.0)), var),
        piecewise(&pts(&|k| k.r.to_radians()), var),
    )
}

fn animated(l: &LayerSpec) -> (bool, bool) {
    let first = l.kf.first();
    let scale_or_rot = l.kf.iter().any(|k| first.is_some_and(|f| (k.s - f.s).abs() > 1e-4 || (k.r - f.r).abs() > 1e-3)) || first.is_some_and(|f| f.r.abs() > 1e-3 || f.s < 0.999);
    let moves = l.kf.iter().any(|k| first.is_some_and(|f| (k.x - f.x).abs() > 0.01 || (k.y - f.y).abs() > 0.01));
    (scale_or_rot, moves)
}

/// Fenêtre d'affichage d'un calque (enable) : sa durée, ou ses fenêtres propres (machine à écrire).
fn enable_of(l: &LayerSpec) -> String {
    if !l.windows.is_empty() {
        return l.windows.iter().map(|(a, b)| format!("between(t\\,{}\\,{})", n(*a), n(*b))).collect::<Vec<_>>().join("+");
    }
    format!("between(t\\,{}\\,{})", n(l.at), n(l.at + l.dur))
}

/// Construit les arguments FFmpeg. `out_short` : petit côté de sortie (réduction finale éventuelle).
pub fn render_args(spec: &RenderSpec, out_short: u32, fps: u32, kbps: Option<f64>, graph_file: &Path) -> Result<Vec<String>> {
    let (w, h) = (spec.w.max(2) / 2 * 2, spec.h.max(2) / 2 * 2);
    let total = spec.total.max(0.1);
    let mut args: Vec<String> = Vec::new();
    let mut g = String::new();
    let mut inputs = 0usize;
    let mut input = |args: &mut Vec<String>, pre: Vec<String>, path: &str| -> usize {
        args.extend(pre);
        args.extend(["-i".into(), path.to_string()]);
        inputs += 1;
        inputs - 1
    };

    // ---- Piste principale ----
    let pieces = &spec.pieces;
    let trans = editor::transition_durations(pieces);
    let mut main_len = 0.0;
    let (mut cur_v, mut cur_a) = (String::new(), String::new());
    for (i, p) in pieces.iter().enumerate() {
        let speed = p.speed.clamp(0.25, 4.0);
        let olen = editor::out_len(p);
        let info = ffmpeg::streams(Path::new(&p.path));
        // Clip déjà au format du cadre (16:9 dans un montage 16:9) : ni fond flou ni bandes à calculer
        let same_shape = info
            .iter()
            .find(|s| s.kind == "video" && s.width > 0 && s.height > 0)
            .is_some_and(|s| ((s.width as f64 * h as f64) / (s.height as f64 * w as f64) - 1.0).abs() < 0.01);
        let frame = frame_filter(if same_shape { "crop" } else { &spec.fit }, w, h, i);
        let color = color_filter(&p.color).map(|c| format!(",{c}")).unwrap_or_default();
        if p.freeze > 0.0 {
            let k = input(&mut args, vec!["-ss".into(), n(p.start.max(0.0)), "-t".into(), "0.5".into()], &p.path);
            g.push_str(&format!(
                "[{k}:v]trim=end_frame=1,setpts=PTS-STARTPTS,{frame}{color},fps={fps},format=yuv420p,setsar=1,tpad=stop_mode=clone:stop_duration={},trim=duration={},setpts=PTS-STARTPTS,settb=AVTB[v{i}];",
                n(olen + 0.5), n(olen)
            ));
            g.push_str(&format!("anullsrc=r=48000:cl=stereo,atrim=0:{}[a{i}];", n(olen)));
        } else {
            let len = (p.end - p.start).max(0.05);
            let k = input(&mut args, vec!["-ss".into(), n(p.start.max(0.0)), "-t".into(), n(len)], &p.path);
            // Vitesse : constante, ou courbe (ralentis / accélérés) en un seul passage
            let timing = if p.segs.is_empty() { format!("setpts=(PTS-STARTPTS)/{speed:.5}") } else { format!("setpts=PTS-STARTPTS,setpts='({})/TB'", speed_curve(p)) };
            g.push_str(&format!(
                "[{k}:v]{timing},{frame}{color},fps={fps},format=yuv420p,setsar=1,tpad=stop_mode=clone:stop_duration=1,trim=duration={},setpts=PTS-STARTPTS,settb=AVTB[v{i}];",
                n(olen)
            ));
            // Son : piste de mixage, ou pistes séparées avec leur volume
            let n_audio = info.iter().filter(|s| s.kind == "audio").count();
            let vol = p.volume.clamp(0.0, 2.0);
            // Son brut [raw] → vitesse (constante, ou morceau par morceau), volume, longueur exacte
            let tempo = |g: &mut String, raw: &str| {
                let end = format!("volume={vol:.3},apad,atrim=0:{},asetpts=PTS-STARTPTS[a{i}];", n(olen));
                if p.segs.is_empty() {
                    g.push_str(&format!("[{raw}]asetpts=PTS-STARTPTS,{},{end}", editor::atempo(speed)));
                    return;
                }
                let parts = p.segs.len();
                g.push_str(&format!("[{raw}]asetpts=PTS-STARTPTS,asplit={parts}"));
                for j in 0..parts {
                    g.push_str(&format!("[sp{i}x{j}]"));
                }
                g.push(';');
                let mut labels = String::new();
                for (j, &(a, b, v)) in p.segs.iter().enumerate() {
                    g.push_str(&format!(
                        "[sp{i}x{j}]atrim=start={}:end={},asetpts=PTS-STARTPTS,{}[sq{i}x{j}];",
                        n((a - p.start).max(0.0)), n((b - p.start).max(0.0)), editor::atempo(v.clamp(0.25, 4.0))
                    ));
                    labels.push_str(&format!("[sq{i}x{j}]"));
                }
                g.push_str(&format!("{labels}concat=n={parts}:v=0:a=1,{end}"));
            };
            let separate = n_audio > 1 && !p.track_volumes.is_empty() && n_audio > p.track_volumes.len();
            let used: Vec<(usize, f64)> = if separate {
                p.track_volumes.iter().enumerate().map(|(t, v)| (t + 1, v.clamp(0.0, 2.0))).filter(|(_, v)| *v > 0.001).collect()
            } else {
                Vec::new()
            };
            if n_audio == 0 || vol <= 0.001 || (separate && used.is_empty()) {
                g.push_str(&format!("anullsrc=r=48000:cl=stereo,atrim=0:{}[a{i}];", n(olen)));
            } else if separate {
                let mut labels = String::new();
                for (s, v) in &used {
                    g.push_str(&format!("[{k}:a:{s}]{AFMT},volume={v:.3}[t{i}x{s}];"));
                    labels.push_str(&format!("[t{i}x{s}]"));
                }
                if used.len() == 1 {
                    g.push_str(&format!("{labels}anull[raw{i}];"));
                } else {
                    g.push_str(&format!("{labels}amix=inputs={}:normalize=0:duration=longest[raw{i}];", used.len()));
                }
                tempo(&mut g, &format!("raw{i}"));
            } else {
                g.push_str(&format!("[{k}:a:0]{AFMT}[raw{i}];"));
                tempo(&mut g, &format!("raw{i}"));
            }
        }
        if i == 0 {
            cur_v = "v0".into();
            cur_a = "a0".into();
            main_len = olen;
        } else {
            let d = trans[i];
            let (nv, na) = (format!("xv{i}"), format!("xa{i}"));
            if d > 0.0 {
                g.push_str(&format!(
                    "[{cur_v}][v{i}]xfade=transition={}:duration={}:offset={}[{nv}];[{cur_a}][a{i}]acrossfade=d={}:c1=tri:c2=tri[{na}];",
                    editor::xfade_name(&pieces[i - 1].transition), n(d), n((main_len - d).max(0.0)), n(d)
                ));
                main_len += olen - d;
            } else {
                g.push_str(&format!("[{cur_v}][{cur_a}][v{i}][a{i}]concat=n=2:v=1:a=1[{nv}][{na}];"));
                main_len += olen;
            }
            cur_v = nv;
            cur_a = na;
        }
    }
    if pieces.is_empty() {
        g.push_str(&format!("color=c=black:s={w}x{h}:r={fps}:d={},format=yuv420p,settb=AVTB[vbase];anullsrc=r=48000:cl=stereo,atrim=0:{}[abase];", n(total), n(total)));
        cur_v = "vbase".into();
        cur_a = "abase".into();
    } else if total > main_len + 0.02 {
        // Des calques ou du son dépassent la piste principale : on prolonge en noir / silence
        g.push_str(&format!("[{cur_v}]tpad=stop_mode=add:stop_duration={}:color=black[vpad];[{cur_a}]apad,atrim=0:{}[apad];", n(total - main_len + 0.1), n(total)));
        cur_v = "vpad".into();
        cur_a = "apad".into();
    }

    // ---- Zoom animé de la vidéo principale ----
    if spec.zoom.iter().any(|k| k.z > 1.001) {
        let tt = frame_time(fps);
        let zp: Vec<(f64, f64)> = spec.zoom.iter().map(|k| (k.t, k.z.clamp(1.0, 5.0))).collect();
        let z = piecewise(&zp, &tt);
        let x = piecewise(&spec.zoom.iter().map(|k| (k.t, k.x.clamp(0.0, 1.0))).collect::<Vec<_>>(), &tt);
        let y = piecewise(&spec.zoom.iter().map(|k| (k.t, k.y.clamp(0.0, 1.0))).collect::<Vec<_>>(), &tt);
        let (cw, ch) = (format!("({w}/({z}))"), format!("({h}/({z}))"));
        let (x0, y0) = (format!("(({w}-{cw})*({x}))"), format!("(({h}-{ch})*({y}))"));
        g.push_str(&format!("[{cur_v}]{}[vzoom];", warp(&x0, &y0, &cw, &ch, "cubic", &zoom_windows(&zp))));
        cur_v = "vzoom".into();
    }

    // ---- Calques (du plus bas au plus haut) ----
    let mut extra_audio: Vec<String> = Vec::new();
    for (li, l) in spec.layers.iter().enumerate() {
        if l.dur <= 0.01 {
            continue;
        }
        let en = enable_of(l);
        let (x, y, s, r) = layer_exprs(l, "t");
        let fade = {
            let mut f = String::new();
            if l.fade_in > 0.01 {
                f.push_str(&format!(",fade=t=in:st={}:d={}:alpha=1", n(l.at), n(l.fade_in.min(l.dur))));
            }
            if l.fade_out > 0.01 {
                let d = l.fade_out.min(l.dur);
                f.push_str(&format!(",fade=t=out:st={}:d={}:alpha=1", n(l.at + l.dur - d), n(d)));
            }
            f
        };
        if l.kind == "blur" {
            // Zone floutée ou pixelisée : on découpe l'image du dessous, on la floute, on la remet
            let (bw, bh) = ((l.w.max(4.0) as u32) / 2 * 2, (l.h.max(4.0) as u32) / 2 * 2);
            let cx = format!("max(0\\,min(iw-{bw}\\,({x})-{bw}/2))");
            let cy = format!("max(0\\,min(ih-{bh}\\,({y})-{bh}/2))");
            let full = bw + 2 >= w && bh + 2 >= h && l.mode != "pixel" && l.opacity >= 0.999 && fade.is_empty();
            if full {
                // Toute l'image floutée : directement, seulement pendant l'effet
                let rr = (l.strength.clamp(1.0, 60.0) as u32).min(h / 2).max(1);
                g.push_str(&format!("[{cur_v}]boxblur={rr}:3:enable='{en}'[L{li}];"));
                cur_v = format!("L{li}");
                continue;
            }
            let blur = if l.mode == "pixel" {
                let cell = (l.strength.clamp(2.0, 60.0)) as u32;
                format!("scale=max(1\\,iw/{cell}):max(1\\,ih/{cell}):flags=area,scale={bw}:{bh}:flags=neighbor")
            } else {
                let rr = (l.strength.clamp(1.0, 60.0) as u32).min(bw.min(bh) / 2).max(1);
                format!("boxblur={rr}:3:enable='{en}'")
            };
            // Opacité et fondus de la zone (comme l'aperçu) : la copie floutée devient transparente
            let opacity = l.opacity.clamp(0.0, 1.0);
            let alpha = if opacity < 0.999 || !fade.is_empty() {
                let mix = if opacity < 0.999 { format!(",colorchannelmixer=aa={opacity:.4}") } else { String::new() };
                format!(",format=rgba{mix}{fade}")
            } else {
                String::new()
            };
            g.push_str(&format!(
                "[{cur_v}]split[bl{li}a][bl{li}b];[bl{li}b]crop={bw}:{bh}:x='{cx}':y='{cy}',{blur}{alpha}[bl{li}c];[bl{li}a][bl{li}c]overlay=x='{}':y='{}':enable='{en}'[L{li}];",
                cx.replace("iw", &w.to_string()).replace("ih", &h.to_string()),
                cy.replace("iw", &w.to_string()).replace("ih", &h.to_string())
            ));
            cur_v = format!("L{li}");
            continue;
        }
        if !Path::new(&l.path).is_file() {
            bail!("Fichier introuvable pour un calque : {}", l.path);
        }
        let (lw, lh) = ((l.w.max(2.0).round() as u32) / 2 * 2, (l.h.max(2.0).round() as u32) / 2 * 2);
        let k = match l.kind.as_str() {
            "video" => {
                let len = l.dur * l.speed.clamp(0.25, 4.0);
                input(&mut args, vec!["-ss".into(), n(l.src_start.max(0.0)), "-t".into(), n(len + 0.1)], &l.path)
            }
            "gif" => input(&mut args, vec!["-ignore_loop".into(), "0".into(), "-t".into(), n(l.dur + 0.2)], &l.path),
            _ => input(&mut args, vec!["-loop".into(), "1".into(), "-framerate".into(), fps.to_string(), "-t".into(), n(l.dur + 0.2)], &l.path),
        };
        let speed = if l.kind == "video" { l.speed.clamp(0.25, 4.0) } else { 1.0 };
        let (scale_rot, _) = animated(l);
        let mut chain = format!("[{k}:v]setpts=(PTS-STARTPTS)/{speed:.5}+{}/TB,fps={fps},format=rgba,scale={lw}:{lh}:flags=lanczos", n(l.at));
        let opacity = l.opacity.clamp(0.0, 1.0);
        if l.op_kf.len() > 1 {
            let e = piecewise(&l.op_kf, "T");
            chain.push_str(&format!(",geq=r='r(X\\,Y)':g='g(X\\,Y)':b='b(X\\,Y)':a='alpha(X\\,Y)*clip({e}\\,0\\,1)'"));
        } else if opacity < 0.999 {
            chain.push_str(&format!(",colorchannelmixer=aa={opacity:.4}"));
        }
        chain.push_str(&fade);
        let (ox, oy);
        if scale_rot {
            // Cadre carré fixe assez grand pour la rotation ; la taille et l'angle animés passent par perspective
            let side = ((lw as f64).hypot(lh as f64).ceil() as u32 + 4) / 2 * 2;
            let tt = format!("({}+in/{fps})", n(l.at));
            let (sx, rx) = (layer_exprs(l, &tt).2, layer_exprs(l, &tt).3);
            let (hw, hh, c) = (lw as f64 / 2.0, lh as f64 / 2.0, side as f64 / 2.0);
            let corner = |dx: f64, dy: f64, axis: char| -> String {
                // Point (dx, dy) × échelle, tourné de l'angle, autour du centre du cadre
                if axis == 'x' {
                    format!("{}+(({})*({}*cos({rx})-({})*sin({rx})))", n(c), sx, n(dx), n(dy))
                } else {
                    format!("{}+(({})*({}*sin({rx})+({})*cos({rx})))", n(c), sx, n(dx), n(dy))
                }
            };
            chain.push_str(&format!(
                ",format=yuva444p,pad={side}:{side}:(ow-iw)/2:(oh-ih)/2:color=black@0,perspective=x0='{}':y0='{}':x1='{}':y1='{}':x2='{}':y2='{}':x3='{}':y3='{}':sense=destination:eval=frame:interpolation=linear",
                corner(-hw, -hh, 'x'), corner(-hw, -hh, 'y'), corner(hw, -hh, 'x'), corner(hw, -hh, 'y'),
                corner(-hw, hh, 'x'), corner(-hw, hh, 'y'), corner(hw, hh, 'x'), corner(hw, hh, 'y')
            ));
            ox = format!("({x})-{}", side / 2);
            oy = format!("({y})-{}", side / 2);
        } else {
            ox = format!("({x})-{}", lw / 2);
            oy = format!("({y})-{}", lh / 2);
        }
        let _ = (s, r);
        g.push_str(&format!("{chain}[o{li}];[{cur_v}][o{li}]overlay=x='{ox}':y='{oy}':eof_action=pass:enable='{en}'[L{li}];"));
        cur_v = format!("L{li}");
        // Son d'une vidéo en incrustation
        if l.kind == "video" && l.volume > 0.001 && ffmpeg::streams(Path::new(&l.path)).iter().any(|s| s.kind == "audio") {
            let delay = (l.at * 1000.0).round() as u64;
            g.push_str(&format!(
                "[{k}:a:0]{AFMT},asetpts=PTS-STARTPTS,{},volume={:.3},atrim=0:{},adelay={delay}:all=1,apad,atrim=0:{}[pa{li}];",
                editor::atempo(speed), l.volume.clamp(0.0, 2.0), n(l.dur), n(total)
            ));
            extra_audio.push(format!("pa{li}"));
        }
    }

    // ---- Effets sur toute l'image (avant les touches, qui restent nettes et immobiles) ----
    cur_v = effect_filters(&mut g, &spec.effects, w, h, fps, cur_v);

    // ---- Touches clavier / souris ----
    if let Some(keys) = &spec.keys
        && Path::new(&keys.base).is_file()
    {
        let intervals = editor::key_intervals(pieces);
        if !intervals.is_empty() {
            let k = input(&mut args, vec!["-loop".into(), "1".into(), "-framerate".into(), fps.to_string(), "-t".into(), n(total)], &keys.base);
            g.push_str(&format!("[{k}:v]format=rgba[kb];[{cur_v}][kb]overlay=x={}:y={}:eof_action=pass[K];", n(keys.x.round()), n(keys.y.round())));
            cur_v = "K".into();
            for sp in &keys.sprites {
                let Some(list) = intervals.get(&sp.key) else { continue };
                if !Path::new(&sp.path).is_file() {
                    continue;
                }
                let en = list.iter().map(|(a, b)| format!("between(t\\,{}\\,{})", n(*a), n(*b))).collect::<Vec<_>>().join("+");
                let k = input(&mut args, vec!["-loop".into(), "1".into(), "-framerate".into(), fps.to_string(), "-t".into(), n(total)], &sp.path);
                g.push_str(&format!("[{k}:v]format=rgba[ks{}];[{cur_v}][ks{}]overlay=x={}:y={}:eof_action=pass:enable='{en}'[K{}];", sp.key, sp.key, n(keys.x.round()), n(keys.y.round()), sp.key));
                cur_v = format!("K{}", sp.key);
            }
        }
    }

    // ---- Sortie vidéo : taille finale ----
    let vertical = h > w;
    let final_scale = if out_short < w.min(h) {
        if vertical { format!(",scale={out_short}:-2:flags=lanczos") } else { format!(",scale=-2:{out_short}:flags=lanczos") }
    } else {
        String::new()
    };
    let end_fade = spec.end_fade.clamp(0.0, total);
    let vfade = if end_fade > 0.01 { format!(",fade=t=out:st={}:d={}", n(total - end_fade), n(end_fade)) } else { String::new() };
    g.push_str(&format!("[{cur_v}]trim=duration={}{vfade}{final_scale},format=yuv420p,setsar=1[vout];", n(total)));

    // ---- Son : principal + incrustations + éléments (musique, bruitages, voix off) ----
    let mut plain: Vec<String> = vec![cur_a.clone()];
    plain.extend(extra_audio);
    let mut ducked: Vec<String> = Vec::new();
    for (ai, a) in spec.audio.iter().enumerate() {
        if a.dur <= 0.01 || a.volume <= 0.0001 && a.kf.iter().all(|k| k.1 <= 0.0001) {
            continue;
        }
        if !Path::new(&a.path).is_file() {
            bail!("Son introuvable : {}", a.path);
        }
        let k = input(&mut args, vec!["-ss".into(), n(a.src_start.max(0.0)), "-t".into(), n(a.dur + 0.1)], &a.path);
        let vol = if a.kf.len() > 1 {
            format!("volume='{}':eval=frame", piecewise(&a.kf, "t"))
        } else {
            format!("volume={:.3}", a.kf.first().map(|k| k.1).unwrap_or(a.volume).clamp(0.0, 2.0))
        };
        let mut chain = format!("[{k}:a:0]asetpts=PTS-STARTPTS,{AFMT},atrim=0:{},{vol}", n(a.dur));
        if a.fade_in > 0.01 {
            chain.push_str(&format!(",afade=t=in:st=0:d={}", n(a.fade_in.min(a.dur / 2.0))));
        }
        if a.fade_out > 0.01 {
            let fo = a.fade_out.min(a.dur / 2.0);
            chain.push_str(&format!(",afade=t=out:st={}:d={}", n(a.dur - fo), n(fo)));
        }
        let delay = (a.at.max(0.0) * 1000.0).round() as u64;
        chain.push_str(&format!(",adelay={delay}:all=1,apad,atrim=0:{}[au{ai}];", n(total)));
        g.push_str(&chain);
        if a.duck { ducked.push(format!("au{ai}")) } else { plain.push(format!("au{ai}")) }
    }
    let limiter = "alimiter=limit=0.97:level=0:latency=1";
    let mix = |list: &[String], out: &str, g: &mut String| {
        if list.len() == 1 {
            g.push_str(&format!("[{}]anull[{out}];", list[0]));
        } else {
            let ins: String = list.iter().map(|l| format!("[{l}]")).collect();
            g.push_str(&format!("{ins}amix=inputs={}:duration=first:normalize=0[{out}];", list.len()));
        }
    };
    if ducked.is_empty() {
        mix(&plain, "amx", &mut g);
    } else {
        // La musique baisse quand le jeu ou la voix sont forts (le son principal sert de déclencheur)
        mix(&ducked, "mus", &mut g);
        g.push_str(&format!("[{}]asplit=2[amain][asc];[mus][asc]sidechaincompress=threshold=0.04:ratio=8:attack=30:release=400[musd];", plain[0]));
        let mut rest = vec!["amain".to_string()];
        rest.extend(plain.iter().skip(1).cloned());
        rest.push("musd".into());
        mix(&rest, "amx", &mut g);
    }
    let afade = if end_fade > 0.01 { format!(",afade=t=out:st={}:d={}", n(total - end_fade), n(end_fade)) } else { String::new() };
    g.push_str(&format!("[amx]{limiter},atrim=0:{}{afade}[aout]", n(total)));

    // Le graphe passe par un fichier (ligne de commande Windows limitée à 32 767 caractères)
    std::fs::write(graph_file, &g)?;
    args.extend(["-/filter_complex".into(), graph_file.to_string_lossy().to_string()]);
    args.extend(["-map".into(), "[vout]".into(), "-map".into(), "[aout]".into(), "-t".into(), n(total), "-r".into(), fps.to_string()]);
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
        None => args.extend(editor::encoder_args()),
    }
    args.extend(["-metadata".into(), format!("title={}", spec.title)]);
    Ok(args)
}

/// Exporte le montage dans un nouveau fichier (les originaux ne sont jamais modifiés).
pub fn render(spec: &RenderSpec, folder: &Path, progress: &dyn Fn(f64)) -> Result<PathBuf> {
    if spec.pieces.is_empty() && spec.layers.is_empty() {
        bail!("Le montage est vide.");
    }
    // Chaque morceau doit rester dans son clip (un morceau vide bloquerait toute la suite de l'image)
    let mut spec = spec.clone();
    for p in &mut spec.pieces {
        if !Path::new(&p.path).is_file() {
            bail!("Clip introuvable : {} (déplacé ou supprimé ?)", p.path);
        }
        let dur = ffmpeg::duration(Path::new(&p.path)).unwrap_or(f64::MAX);
        p.start = p.start.clamp(0.0, (dur - 0.1).max(0.0));
        p.end = p.end.min(dur);
        if p.freeze <= 0.0 && p.end - p.start < 0.05 {
            bail!("Un morceau est en dehors de son clip ({:.1} s pour un clip de {dur:.1} s).", p.start);
        }
    }
    crate::studio::clean_cache();
    let total = spec.total.max(0.1);
    let src_fps = spec.pieces.first().map(|p| editor::frame_rate(Path::new(&p.path)) as u32).unwrap_or(60);
    let mut fps = if spec.fps > 0 { spec.fps.clamp(24, 60) } else { src_fps };
    let mut short = if spec.short >= 1080 { 1080 } else { 720 }.min(spec.w.min(spec.h).max(2));
    let mut target_kbps: Option<f64> = None;
    if spec.max_mb > 0.0 {
        let (sh, f, kbps) = editor::size_plan(total, spec.max_mb, short, fps)?;
        short = sh;
        fps = f;
        target_kbps = Some(kbps);
    }
    let _ = LADDER;
    std::fs::create_dir_all(folder)?;
    let stamp = chrono::Local::now().format("%Y-%m-%d %Hh%M-%S");
    let out = util::reserve_file(folder, &util::safe_name(&format!("{} {stamp}", spec.title)), "mp4")?;
    let graph = util::data_dir().join("cache").join(format!("graphe-{}.txt", crate::library::new_id()));
    let _ = std::fs::create_dir_all(graph.parent().unwrap_or(Path::new(".")));
    let factors: &[f64] = if target_kbps.is_some() { &[1.0, 0.85] } else { &[1.0] };
    let mut last_err = None;
    for &factor in factors {
        let args = render_args(&spec, short, fps, target_kbps.map(|k| k * factor), &graph)?;
        let result = editor::run(args, &out, total, progress);
        match result {
            Ok(()) => {
                let mb = std::fs::metadata(&out).map(|m| m.len()).unwrap_or(0) as f64 / 1_048_576.0;
                if spec.max_mb <= 0.0 || mb <= spec.max_mb {
                    let _ = std::fs::remove_file(&graph);
                    return Ok(out);
                }
                log::warn!("montage de {mb:.1} Mo pour {} Mo max : nouvel essai", spec.max_mb);
                last_err = Some(anyhow::anyhow!("Impossible de passer sous {:.0} Mo : raccourcis le montage.", spec.max_mb));
            }
            Err(e) => {
                // Le graphe est gardé pour comprendre l'erreur (rapport de diagnostic)
                log::warn!("export du Studio : {e:#} (graphe : {})", graph.display());
                last_err = Some(e);
                break;
            }
        }
    }
    let _ = std::fs::remove_file(&out);
    Err(last_err.unwrap_or_else(|| anyhow::anyhow!("Export impossible")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn courbe_par_morceaux() {
        assert_eq!(piecewise(&[], "t"), "0");
        assert_eq!(piecewise(&[(1.0, 0.5)], "t"), "0.5");
        assert_eq!(piecewise(&[(0.0, 2.0), (3.0, 2.0)], "t"), "2", "constante : pas d'expression");
        let e = piecewise(&[(0.0, 0.0), (2.0, 1.0)], "t");
        assert_eq!(e, "if(lt(t\\,0)\\,0\\,if(gte(t\\,2)\\,1\\,(0+1*(t-0)/2)))");
        // Beaucoup de points : profondeur en log2 (FFmpeg refuse les longues sommes)
        let many: Vec<(f64, f64)> = (0..400).map(|i| (i as f64 * 0.01, (i % 7) as f64)).collect();
        let e = piecewise(&many, "t");
        let depth = e.chars().fold((0i32, 0i32), |(d, m), c| match c { '(' => (d + 1, m.max(d + 1)), ')' => (d - 1, m), _ => (d, m) }).1;
        assert!(depth < 40, "profondeur {depth}");
    }

    #[test]
    fn vitesse_variable_et_zoom() {
        // Ralenti au milieu : la courbe suit les marches, avec un trou (marche trop courte, ignorée)
        let p = Piece { start: 10.0, end: 14.0, segs: vec![(10.0, 11.0, 1.0), (11.0, 12.0, 0.5), (12.05, 14.0, 2.0)], ..Default::default() };
        assert!((editor::out_len(&p) - (1.0 + 2.0 + 0.975)).abs() < 1e-9);
        assert!((editor::src_to_out(&p, 11.5) - 2.0).abs() < 1e-9);
        let c = speed_curve(&p);
        assert!(c.contains("(1+2*(T-1)/1)"), "{c}");
        assert!(c.contains("if(gte(T\\,4)\\,3.975"), "{c}");
        // Zoom : calculé seulement là où il dépasse 1
        assert_eq!(zoom_windows(&[(0.0, 1.0), (1.0, 1.0), (1.5, 1.3), (2.0, 1.0), (3.0, 1.0)]), "between(t\\,1\\,2)");
        assert_eq!(zoom_windows(&[(0.0, 1.0), (5.0, 1.0)]), "0");
    }

    #[test]
    fn couleur_comme_le_css() {
        assert!(color_filter(&[1.0, 1.0, 1.0]).is_none());
        let f = color_filter(&[1.2, 1.0, 0.0]).unwrap();
        assert!(f.contains("val*1.2000"));
        // Saturation 0 : niveaux de gris (chaque ligne de la matrice = 0.213, 0.715, 0.072)
        assert!(f.contains("rr=0.2130:rg=0.7150:rb=0.0720"), "{f}");
    }

    #[test]
    fn effets_sur_toute_l_image() {
        let fx = |kind: &str, at: f64| EffectSpec { kind: kind.into(), at, dur: 1.0, strength: 1.0 };
        let mut g = String::new();
        // Ordre fixe (couleur avant tremblement), quel que soit l'ordre reçu ; type inconnu ignoré
        let out = effect_filters(&mut g, &[fx("shake", 0.0), fx("bw", 2.0), fx("inconnu", 0.0), fx("glitch", 1.0)], 1920, 1080, 60, "L0".into());
        assert_eq!(out, "FX2");
        assert!(g.starts_with("[L0]hue=s=0:enable='between(t\\,2\\,3)'[FX0];"), "{g}");
        assert!(g.contains("[FX0]rgbashift=rh=-38:bh=38:edge=smear"), "{g}");
        assert!(g.contains("[FX1]perspective=x0='58*(1+sin(2*PI*7*((in-1)/60)))'"), "{g}");
        assert!(g.contains(":enable='between(t\\,0\\,1)'[FX2];"), "{g}");
        // Tremblement : cadre gardé pair, décalage max = la marge
        let (cw, ch, x, _) = shake_crop(1920, 1080, 1.0, "t");
        assert_eq!((cw, ch), (1804, 1014));
        assert_eq!(x, "58*(1+sin(2*PI*7*t))");
        // Aucun effet : rien n'est ajouté
        let mut g2 = String::new();
        assert_eq!(effect_filters(&mut g2, &[], 1920, 1080, 60, "L0".into()), "L0");
        assert!(g2.is_empty());
    }
}
