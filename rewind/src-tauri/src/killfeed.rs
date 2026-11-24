//! Kill feed de Counter-Strike 2 : moment exact où chacun de TES kills apparaît à l'écran (2026-11-23).
//!
//! Dans CS2, tes kills s'affichent en haut à droite dans un cadre sombre à fin contour rouge vif ; ta mort a un
//! fond rouge sombre (avec un contour rouge lui aussi) : jamais comptée. Chaque nouvelle ligne arrive EN BAS de la
//! liste, d'un coup (aucune animation) ; la liste remonte quand la plus ancienne s'efface.
//!
//! Détection, image par image (moitié haute droite de l'image) :
//!  1. traits rouges fins et longs ; règle stricte (210,6,9) et règle « pâle » (contour à cheval sur deux lignes
//!     de pixels, ou mélangé au ciel), cramoisie seulement (les murs ocre ont plus de vert que de bleu) ;
//!  2. un cadre = deux traits voisins dont au moins un net, même largeur, intérieur ni rouge vif ni teinté de
//!     rouge (la mort), bords verticaux présents ;
//!  3. suivi d'une image à l'autre (largeur, position, empreinte du contenu). Une ligne ne fait que remonter ; une
//!     ligne déjà là au début du clip, découverte au-dessus d'une ligne suivie, ou dont le texte et le contour
//!     étaient déjà visibles un quart de seconde plus tôt, n'est pas un nouveau kill.
//!
//! Le Studio (killfx.ts) s'en sert ainsi : clip avec les repères de kills du jeu (GSI) → le jeu fait foi et chaque
//! repère est recalé sur l'apparition de sa ligne (mesuré sur 12 clips : 35 kills sur 35, aucun en trop, +0,1 à
//! +0,2 s après le repère) ; clip sans repères (anciens clips, Medal) → cette détection seule (contrôlée à l'œil
//! sur des clips manuels : quelques lignes déjà affichées étaient encore comptées avant la « preuve d'apparition »).

use crate::{ffmpeg, media, util};
use anyhow::{Result, bail};
use std::io::Read;
use std::path::Path;
use std::process::Stdio;

/// Version de l'algorithme (change = analyses en cache refaites)
const VERSION: u32 = 6;
const SIG: usize = 24;

#[derive(Clone, Debug)]
pub struct FeedBox {
    pub top: usize,
    pub bottom: usize,
    pub left: usize,
    pub right: usize,
    pub sig: [f32; SIG],
    /// Vraiment apparue maintenant (absente un quart de seconde plus tôt) ; calculé par `scan`
    pub fresh: bool,
}

/// Pixels « d'interface » d'une zone : texte et icônes blancs, contour rouge vif. Sert à savoir si une ligne
/// vient d'apparaître (rien de tout ça juste avant) ou était déjà là, mal vue (sur un ciel, un mur ocre).
fn hud_pixels(frame: &[u8], w: usize, b: &FeedBox) -> usize {
    let mut n = 0;
    for y in b.top..=b.bottom {
        for x in b.left..=b.right.min(w - 1) {
            let p = &frame[(y * w + x) * 3..(y * w + x) * 3 + 3];
            if p[0].min(p[1]).min(p[2]) >= 225 || red(p) {
                n += 1;
            }
        }
    }
    n
}

/// Rouge vif du contour (210,6,9). Règle stricte : un contour mélangé au ciel ou à un mur peut être manqué un
/// instant, mais jamais un mur ocre, une porte ou le fond de la mort n'est pris pour un contour (essai du
/// 2026-11-23 avec une règle plus large : 8 fausses détections de plus sur 12 clips).
#[inline]
fn red(p: &[u8]) -> bool {
    let (r, g, b) = (p[0] as i16, p[1] as i16, p[2] as i16);
    r >= 120 && g <= 75 && b <= 75 && r - g >= 80 && r - b >= 80
}

/// Rouge « pâle » : un contour à cheval sur deux lignes de pixels (110,34,39) ou mélangé au ciel (242,71,99).
/// Cramoisi seulement (le bleu ≥ le vert) : les murs ocre (120,44,31) n'y entrent pas. Ne suffit jamais seul :
/// un cadre doit avoir au moins un trait net (règle stricte).
#[inline]
fn pale(p: &[u8]) -> bool {
    let (r, g, b) = (p[0] as i16, p[1] as i16, p[2] as i16);
    r >= 90 && r - g.max(b) >= 45 && b >= g - 12
}

/// Cadres de kill feed (contour rouge fin) dans une image RGB24 `w`×`h` ; `s` = hauteur de la vidéo / 1080.
pub fn boxes(frame: &[u8], w: usize, h: usize, s: f64) -> Vec<FeedBox> {
    let strict: Vec<bool> = frame.chunks_exact(3).map(red).collect();
    let mask: Vec<bool> = frame.chunks_exact(3).zip(&strict).map(|(p, &s)| s || pale(p)).collect();
    let at = |x: usize, y: usize| mask[y * w + x];
    let rows: Vec<usize> = (0..h).map(|y| mask[y * w..(y + 1) * w].iter().filter(|&&m| m).count()).collect();
    let strict_rows: Vec<usize> = (0..h).map(|y| strict[y * w..(y + 1) * w].iter().filter(|&&m| m).count()).collect();
    let lmin = (120.0 * s) as usize;
    let thick = (5.0 * s).max(5.0) as usize;
    // Traits : suites de lignes « longues », gardées si fines ; étendue horizontale du plus long segment
    let mut lines: Vec<(usize, usize, usize, usize, bool)> = Vec::new();
    let mut y = 0;
    while y < h {
        if rows[y] < lmin {
            y += 1;
            continue;
        }
        let y0 = y;
        while y < h && rows[y] >= lmin {
            y += 1;
        }
        let y1 = y - 1;
        if y1 - y0 + 1 > thick {
            continue;
        }
        let yy = (y0..=y1).max_by_key(|&r| rows[r]).unwrap_or(y0);
        let (mut best, mut start, mut prev) = ((0usize, 0usize), usize::MAX, 0usize);
        for x in (0..w).filter(|&x| at(x, yy)) {
            if start == usize::MAX {
                start = x;
            } else if x - prev > 3 {
                if prev - start > best.1 - best.0 {
                    best = (start, prev);
                }
                start = x;
            }
            prev = x;
        }
        if start != usize::MAX && prev - start > best.1 - best.0 {
            best = (start, prev);
        }
        let net = (y0..=y1).any(|r| strict_rows[r] >= lmin);
        lines.push((y0, y1, best.0, best.1, net));
    }
    let mut out = Vec::new();
    let edge = ((4.0 * s) as usize).max(2);
    for pair in lines.windows(2) {
        // le bas d'un cadre est le trait fin SUIVANT (aucun autre trait entre les deux)
        let ((a1, b1, l1, r1, n1), (a2, b2, l2, r2, n2)) = (pair[0], pair[1]);
        if !n1 && !n2 {
            continue; // deux traits pâles : pas assez sûr
        }
        let gap = a2.saturating_sub(b1);
        if (gap as f64) < 16.0 * s || (gap as f64) > 50.0 * s {
            continue;
        }
        if (l1 as f64 - l2 as f64).abs() > 6.0 * s || (r1 as f64 - r2 as f64).abs() > 6.0 * s {
            continue;
        }
        let (l, r) = (l1.max(l2), r1.min(r2));
        if r <= l + 20 {
            continue;
        }
        // intérieur sombre (pas un fond rouge plein : la mort du joueur)
        let inner_rows = (b1 + 1)..a2;
        let inner_red: usize = inner_rows.clone().map(|y| (l..=r).filter(|&x| at(x, y)).count()).sum();
        if inner_red as f64 > 0.2 * ((r - l + 1) * inner_rows.len()) as f64 {
            continue;
        }
        // bords verticaux rouges à gauche et à droite sur la hauteur du cadre
        let (lo, hi) = (l1.min(l2), r1.max(r2));
        let side = |x0: usize, x1: usize| inner_rows.clone().filter(|&y| (x0..=x1.min(w - 1)).any(|x| at(x, y))).count() as f64 / inner_rows.len().max(1) as f64;
        if side(lo.saturating_sub(edge), lo + edge) < 0.6 || side(hi.saturating_sub(edge), hi + edge) < 0.6 {
            continue;
        }
        // intérieur teinté de rouge = ta mort (fond rouge sombre, avec contour rouge lui aussi) : jamais un kill
        let (x0, x1) = (l + 3, r.saturating_sub(2));
        let (y0, y1) = (b1 + 2, a2.saturating_sub(1));
        if x1 > x0 && y1 > y0 {
            let (mut sr, mut sg, mut sb, mut n) = (0f64, 0f64, 0f64, 0f64);
            for y in y0..y1 {
                for x in (x0..x1).step_by(2) {
                    let p = &frame[(y * w + x) * 3..(y * w + x) * 3 + 3];
                    sr += p[0] as f64;
                    sg += p[1] as f64;
                    sb += p[2] as f64;
                    n += 1.0;
                }
            }
            let (mr, mg, mb) = (sr / n, sg / n, sb / n);
            if mr > mg + 25.0 && mr > mb + 25.0 {
                continue;
            }
        }
        // empreinte : luminosité de l'intérieur en 24 colonnes
        let mut sig = [0f32; SIG];
        if x1 > x0 + SIG && y1 > y0 {
            let bw = (x1 - x0) as f32 / SIG as f32;
            for (i, v) in sig.iter_mut().enumerate() {
                let (c0, c1) = (x0 + (i as f32 * bw) as usize, (x0 + ((i + 1) as f32 * bw) as usize).max(x0 + (i as f32 * bw) as usize + 1));
                let mut sum = 0f32;
                let mut n = 0f32;
                for y in y0..y1 {
                    for x in c0..c1 {
                        let p = &frame[(y * w + x) * 3..(y * w + x) * 3 + 3];
                        sum += (p[0] as f32 + p[1] as f32 + p[2] as f32) / 3.0;
                        n += 1.0;
                    }
                }
                *v = if n > 0.0 { sum / n } else { 0.0 };
            }
        }
        out.push(FeedBox { top: a1, bottom: b2, left: l, right: r, sig, fresh: true });
    }
    out
}

fn similar(a: &[f32; SIG], b: &[f32; SIG]) -> f32 {
    let ma = a.iter().sum::<f32>() / SIG as f32;
    let mb = b.iter().sum::<f32>() / SIG as f32;
    let (mut ab, mut aa, mut bb) = (0f32, 0f32, 0f32);
    for i in 0..SIG {
        let (x, y) = (a[i] - ma, b[i] - mb);
        ab += x * y;
        aa += x * x;
        bb += y * y;
    }
    let d = (aa * bb).sqrt();
    if d < 1e-6 { 1.0 } else { ab / d }
}

struct Track {
    #[allow(dead_code)] // gardé pour le diagnostic
    left: f64,
    right: f64,
    width: f64,
    y: f64,
    sig: [f32; SIG],
    first: f64,
    last: f64,
    hits: u32,
    initial: bool,
    counted: bool,
}

/// Suit les cadres d'une image à l'autre ; renvoie les moments d'apparition des nouveaux kills.
#[derive(Default)]
pub struct Tracker {
    tracks: Vec<Track>,
    start: Option<f64>,
    pub kills: Vec<f64>,
}

impl Tracker {
    pub fn push(&mut self, t: f64, found: &[FeedBox], s: f64) {
        let start = *self.start.get_or_insert(t);
        let existing = self.tracks.len();
        let mut used = vec![false; self.tracks.len()];
        for b in found {
            let (w, r, y) = ((b.right - b.left) as f64, b.right as f64, b.top as f64);
            let mut best: Option<usize> = None;
            let mut best_d = f64::MAX;
            for (k, tr) in self.tracks.iter().enumerate() {
                if used[k] {
                    continue;
                }
                // une ligne ne fait que remonter (les plus anciennes s'effacent en haut), jamais descendre :
                // une nouvelle ligne arrive en bas, elle ne peut pas être prise pour une ligne au-dessus
                let moved_ok = y - tr.y <= 8.0 * s && tr.y - y <= 160.0 * s;
                // ligne perdue depuis plus d'une demi-seconde : il faut qu'elle soit vraiment identique
                let lost = t - tr.last > 0.5;
                let (wt, st) = if lost { (3.0 * s, 0.9) } else { (6.0 * s, 0.6) };
                if moved_ok && (tr.width - w).abs() <= wt && (tr.right - r).abs() <= 10.0 * s && similar(&tr.sig, &b.sig) > st {
                    let d = (tr.y - y).abs();
                    if d < best_d {
                        best = Some(k);
                        best_d = d;
                    }
                }
            }
            if best.is_none() {
                // deuxième détection d'une ligne qui vient d'apparaître au même endroit : la même ligne
                best = self.tracks.iter().enumerate().position(|(k, tr)| !used[k] && !tr.initial && t - tr.first <= 0.3 && (tr.y - y).abs() <= 10.0 * s);
            }
            match best {
                Some(k) => {
                    used[k] = true;
                    let tr = &mut self.tracks[k];
                    tr.y = y;
                    tr.left = b.left as f64;
                    tr.right = r;
                    tr.width = w;
                    tr.last = t;
                    tr.hits += 1;
                    for i in 0..SIG {
                        tr.sig[i] = 0.8 * tr.sig[i] + 0.2 * b.sig[i];
                    }
                }
                None => {
                    // Une nouvelle ligne arrive toujours EN BAS du kill feed : une ligne découverte au-dessus d'une
                    // ligne déjà suivie (et visible) est ancienne (mal vue jusque-là, sur un ciel ou un mur) : pas un kill
                    let below = self.tracks.iter().take(existing).any(|tr| tr.last >= t - 0.1 && tr.y > y + 8.0 * s);
                    // Même place, même largeur qu'une ligne vue il y a peu : la même ligne, retrouvée
                    let same_slot = self.tracks.iter().take(existing).any(|tr| (tr.y - y).abs() <= 6.0 * s && (tr.width - w).abs() <= 4.0 * s && similar(&tr.sig, &b.sig) > 0.5);
                    let old = t - start < 0.05 || below || same_slot || !b.fresh;
                    self.tracks.push(Track { left: b.left as f64, right: r, width: w, y, sig: b.sig, first: t, last: t, hits: 1, initial: old, counted: false });
                    used.push(true);
                }
            }
        }
        for tr in self.tracks.iter_mut() {
            if !tr.counted && !tr.initial && tr.hits >= 3 {
                tr.counted = true;
                self.kills.push((tr.first * 1000.0).round() / 1000.0);
            }
        }
        self.kills.sort_by(f64::total_cmp);
        self.kills.dedup_by(|a, b| (*a - *b).abs() < 0.05);
        // lignes disparues gardées 8 s (durée d'affichage du kill feed) : une ligne qui remonte ou réapparaît n'est pas un nouveau kill
        self.tracks.retain(|tr| t - tr.last <= 8.0);
    }
}

/// Analyse un clip (moitié haute droite, toutes les images, 60 i/s au plus) ; résultat gardé en cache.
pub fn scan(path: &Path) -> Result<Vec<f64>> {
    let dir = media::cache_dir("killfeed");
    let cache = dir.join(format!("{}-v{VERSION}.json", media::cache_key(path)));
    if let Ok(text) = std::fs::read_to_string(&cache)
        && let Ok(v) = serde_json::from_str::<Vec<f64>>(&text)
    {
        return Ok(v);
    }
    let info = media::info(path)?;
    if info.width == 0 || info.height == 0 {
        bail!("Pas d'image dans ce fichier");
    }
    let (cw, ch) = ((info.width / 2) as usize, (info.height / 2) as usize);
    let fps = if info.fps > 1.0 { info.fps.min(60.0) } else { 30.0 };
    let tools = ffmpeg::tools()?;
    let vf = format!("fps={fps},crop={cw}:{ch}:{}:0", info.width as usize - cw);
    let mut child = util::command(&tools.ffmpeg)
        .args(["-hide_banner", "-loglevel", "error", "-i"])
        .arg(path)
        .args(["-an", "-vf", &vf, "-pix_fmt", "rgb24", "-f", "rawvideo", "-"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()?;
    let mut out = child.stdout.take().ok_or_else(|| anyhow::anyhow!("FFmpeg sans sortie"))?;
    let s = info.height as f64 / 1080.0;
    let mut buf = vec![0u8; cw * ch * 3];
    let mut tracker = Tracker::default();
    let mut i = 0u64;
    // images du dernier quart de seconde (preuve d'apparition)
    let back = ((fps * 0.25).round() as usize).max(2);
    let mut history: std::collections::VecDeque<Vec<u8>> = std::collections::VecDeque::with_capacity(back + 1);
    // Diagnostic : REWIND_KF_DEBUG="début-fin" affiche les cadres vus à chaque image dans cet intervalle
    let debug: Option<(f64, f64)> = std::env::var("REWIND_KF_DEBUG").ok().and_then(|v| {
        let (a, b) = v.split_once('-')?;
        Some((a.parse().ok()?, b.parse().ok()?))
    });
    while out.read_exact(&mut buf).is_ok() {
        let t = i as f64 / fps;
        let mut found = boxes(&buf, cw, ch, s);
        if history.len() >= back {
            let before = &history[0];
            for b in found.iter_mut() {
                let (now, was) = (hud_pixels(&buf, cw, b), hud_pixels(before, cw, b));
                b.fresh = (was as f64) < 0.45 * now as f64;
            }
        }
        if let Some((a, b)) = debug
            && t >= a
            && t <= b
        {
            let list: Vec<String> = found.iter().map(|f| format!("y{}-{} x{}-{} w{}", f.top, f.bottom, f.left, f.right, f.right - f.left)).collect();
            println!("{t:7.3} {} | kills {:?}", list.join("  "), tracker.kills);
        }
        tracker.push(t, &found, s);
        // Rotation des tampons : le plus ancien sort de l'historique et est réutilisé (aucune allocation par image)
        match history.len() >= back {
            true => {
                let mut old = history.pop_front().unwrap_or_default();
                old.clear();
                old.extend_from_slice(&buf);
                history.push_back(old);
            }
            false => history.push_back(buf.clone()),
        }
        i += 1;
    }
    let _ = child.wait();
    if i == 0 {
        bail!("Aucune image lue");
    }
    let kills = tracker.kills;
    let _ = std::fs::write(&cache, serde_json::to_string(&kills)?);
    media::prune(&dir, 600);
    log::info!("kill feed : {} kill(s) dans {} ({} images)", kills.len(), path.display(), i);
    Ok(kills)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Image synthétique : un cadre de kill (contour rouge fin) et un cadre de mort (fond rouge plein).
    fn frame(w: usize, h: usize, kill: Option<(usize, usize, usize)>, death: Option<(usize, usize)>) -> Vec<u8> {
        let mut f = vec![30u8; w * h * 3];
        let mut put = |x: usize, y: usize, c: [u8; 3]| f[(y * w + x) * 3..(y * w + x) * 3 + 3].copy_from_slice(&c);
        if let Some((top, l, r)) = kill {
            for x in l..=r {
                for dy in 0..2 {
                    put(x, top + dy, [210, 6, 9]);
                    put(x, top + 30 + dy, [200, 10, 12]);
                }
            }
            for y in top..top + 32 {
                for dx in 0..2 {
                    put(l + dx, y, [210, 6, 9]);
                    put(r - dx, y, [210, 6, 9]);
                }
            }
            // texte blanc au milieu (empreinte)
            for x in (l + 20..l + 120).step_by(3) {
                for y in top + 10..top + 22 {
                    put(x, y, [250, 250, 250]);
                }
            }
        }
        if let Some((top, l)) = death {
            // fond rouge sombre ET contour rouge vif (comme dans CS2)
            for y in top..top + 32 {
                for x in l..l + 400 {
                    let edge = y < top + 2 || y >= top + 30 || x < l + 2 || x >= l + 398;
                    put(x, y, if edge { [210, 6, 9] } else { [93, 17, 22] });
                }
            }
        }
        f
    }

    #[test]
    fn kill_detecte_mort_ignoree() {
        let (w, h) = (960, 540);
        assert_eq!(boxes(&frame(w, h, Some((140, 500, 940)), None), w, h, 1.0).len(), 1, "cadre de kill");
        assert!(boxes(&frame(w, h, None, Some((200, 500))), w, h, 1.0).is_empty(), "fond rouge de la mort : pas un kill");
        assert!(boxes(&frame(w, h, None, None), w, h, 1.0).is_empty());
    }

    #[test]
    fn suivi_sans_doublon() {
        let (w, h, s) = (960, 540, 1.0);
        let mut tr = Tracker::default();
        // déjà là au début du clip : ignoré ; puis nouveau kill à 1,00 s ; la ligne remonte à 2 s ; disparaît 0,5 s
        for i in 0..200 {
            let t = i as f64 / 50.0;
            let old = if t < 1.5 { Some((106, 498, 940)) } else { None };
            let new_top = if t < 2.0 { 140 } else { 106 };
            let visible_new = t >= 1.0 && !(2.4..2.9).contains(&t);
            let mut found = Vec::new();
            if let Some(o) = old {
                found.extend(boxes(&frame(w, h, Some(o), None), w, h, s));
            }
            if visible_new {
                let mut b = boxes(&frame(w, h, Some((new_top, 554, 940)), None), w, h, s);
                // contenu différent de l'ancienne ligne
                for v in b.iter_mut().flat_map(|b| b.sig.iter_mut()).step_by(2) {
                    *v += 60.0;
                }
                found.extend(b);
            }
            tr.push(t, &found, s);
        }
        assert_eq!(tr.kills, vec![1.0], "un seul kill, au bon moment");
    }
}
