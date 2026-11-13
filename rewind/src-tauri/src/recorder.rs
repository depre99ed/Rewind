//! Moteur d'enregistrement « replay buffer ».
//!
//! FFmpeg capture l'écran par le GPU (Windows Graphics Capture, sans injection), encode en continu
//! (NVENC / AMF / QuickSync) et écrit des segments d'environ 1 seconde dans un dossier tampon.
//! Les horodatages sont ceux de l'horloge du PC (`-use_wallclock_as_timestamps` + `-copyts`) :
//! chaque segment sait donc exactement à quelle heure il a été filmé, ce qui permet de faire
//! correspondre un kill CS2 ou un appui de touche à la bonne image.
//! Un clip = recopie (sans réencodage) des segments couvrant la période demandée.

use crate::{
    audio,
    ffmpeg,
    library::{self, Clip, Marker},
    settings::Settings,
    util,
};
use anyhow::{Context, Result, bail};
use parking_lot::Mutex;
use serde::Serialize;
use std::{
    collections::VecDeque,
    fs,
    io::{BufRead, BufReader, Read, Seek, SeekFrom},
    path::{Path, PathBuf},
    process::{Child, Stdio},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

#[derive(Clone, Debug)]
pub struct Segment {
    pub path: PathBuf,
    pub start: f64,
    pub end: f64,
    pub bytes: u64,
    pub session: u64,
}

struct Capture {
    /// FFmpeg image (capture GPU → encodeur → segments vidéo)
    child: Child,
    /// FFmpeg son (tuyaux WASAPI → AAC → segments audio), absent s'il n'y a aucune source
    audio_child: Option<Child>,
    _job: Option<util::Job>,
    audio: audio::Inputs,
    dir: PathBuf,
    vcsv: u64,
    acsv: u64,
    session: u64,
    started: Instant,
    /// Dernière arrivée d'un segment image / son (chien de garde)
    last_video: Instant,
    last_audio: Instant,
    /// Configuration des écrans au démarrage de la capture
    monitors: String,
    tracks: Vec<String>,
    encoder: String,
    width: u32,
    height: u32,
    fps: u32,
}

fn stop_child(child: &mut Child) {
    // Arrêt propre : « q » sur l'entrée de FFmpeg, puis arrêt forcé si besoin
    if let Some(stdin) = child.stdin.as_mut() {
        use std::io::Write;
        let _ = stdin.write_all(b"q");
        let _ = stdin.flush();
    }
    let deadline = Instant::now() + Duration::from_millis(1500);
    while Instant::now() < deadline {
        if let Ok(Some(_)) = child.try_wait() {
            return;
        }
        std::thread::sleep(Duration::from_millis(30));
    }
    let _ = child.kill();
    let _ = child.wait();
}

impl Drop for Capture {
    fn drop(&mut self) {
        stop_child(&mut self.child);
        if let Some(a) = self.audio_child.as_mut() {
            stop_child(a);
        }
    }
}

/// Cadence de sortie : un diviseur exact de la fréquence de l'écran, le plus proche du choix.
/// Ex. écran 100 Hz + 60 demandés → 50 i/s (une image sur deux, parfaitement régulier) ;
/// 144 Hz → 72 ; 60 Hz → 60. Capturer 60 sur un écran 100 Hz donnait des images irrégulières.
pub fn pick_fps(wanted: u32, refresh: u32) -> u32 {
    if !(24..=360).contains(&refresh) {
        return wanted;
    }
    let mut best = refresh;
    let mut best_diff = i64::MAX;
    for k in 1..=10 {
        if refresh % k != 0 {
            continue;
        }
        let f = refresh / k;
        if f < 24 {
            break;
        }
        let diff = (f as i64 - wanted as i64).abs();
        if diff < best_diff || (diff == best_diff && f > best) {
            best = f;
            best_diff = diff;
        }
    }
    best
}

#[derive(Clone, Debug, Serialize, Default)]
pub struct AudioLevel {
    pub label: String,
    pub level: f32,
    pub active: bool,
}

#[derive(Clone, Debug, Serialize, Default)]
pub struct Status {
    pub recording: bool,
    pub starting: bool,
    pub buffered: f64,
    pub buffer_target: f64,
    pub encoder: String,
    pub width: u32,
    pub height: u32,
    pub fps: u32,
    pub buffer_mb: f64,
    pub error: String,
    pub audio: Vec<AudioLevel>,
    pub app: String,
    /// Heure de début du clip sur mesure en cours (0 = aucun)
    pub custom_since: f64,
    /// Heure de début de la session complète en cours (0 = aucune)
    pub session_since: f64,
    /// Commande vocale : 0 coupée, 1 démarrage, 2 à l'écoute, 3 indisponible, 4 erreur
    pub voice: u8,
}

pub struct Recorder {
    capture: Option<Capture>,
    /// Segments vidéo (1 s) et audio (2 s), horodatés à l'heure du PC
    pub segments: VecDeque<Segment>,
    pub audio_segments: VecDeque<Segment>,
    settings: Settings,
    pub error: String,
    session_counter: u64,
    next_retry: Instant,
    failures: u32,
    /// (heure, application au premier plan) — sert à nommer les clips
    pub apps: VecDeque<(f64, String)>,
    pub mic_muted: Arc<AtomicBool>,
    pub paused: bool,
    /// Heure du dernier passage (détecte une sortie de veille ou un gel)
    last_tick: f64,
    last_checks: Instant,
    last_disk: Instant,
    /// Enregistrement suspendu faute de place
    pub disk_low: bool,
    /// Réglages de chaque session (pour recoller les morceaux d'avant/après un redémarrage)
    sessions: VecDeque<SessionInfo>,
    /// Clip sur mesure ou session complète en cours : rien n'est effacé après cette heure
    hold_from: Option<f64>,
}

/// Une session = un lancement de FFmpeg. Deux sessions de même `key` peuvent être recollées.
#[derive(Clone, Debug)]
struct SessionInfo {
    id: u64,
    key: String,
    tracks: Vec<String>,
}

/// En dessous : pause ; au-dessus : reprise (Go)
const DISK_PAUSE_GB: f64 = 1.5;
const DISK_RESUME_GB: f64 = 3.0;

fn buffer_root() -> PathBuf {
    util::data_dir().join("buffer")
}

/// Nettoie les restes d'une session précédente (à appeler avant de démarrer le moteur).
pub fn clean_buffer() {
    // Les journaux FFmpeg de la session précédente servent au diagnostic : on les garde avant de vider.
    crate::rapport::archive_previous_ffmpeg_logs(&buffer_root());
    let _ = fs::remove_dir_all(buffer_root());
    let _ = fs::create_dir_all(buffer_root());
}

impl Recorder {
    /// Ne touche pas au disque : le tampon se vide avec `clean_buffer`, seulement dans l'instance
    /// principale (un 2e lancement effaçait le tampon de Rewind déjà ouvert, 2026-11-13).
    pub fn new(settings: Settings) -> Self {
        Self {
            capture: None,
            segments: VecDeque::new(),
            audio_segments: VecDeque::new(),
            paused: !settings.recording_enabled,
            settings,
            error: String::new(),
            session_counter: 0,
            next_retry: Instant::now(),
            failures: 0,
            apps: VecDeque::new(),
            mic_muted: Arc::new(AtomicBool::new(false)),
            last_tick: 0.0,
            last_checks: Instant::now(),
            last_disk: Instant::now() - Duration::from_secs(60),
            disk_low: false,
            sessions: VecDeque::new(),
            hold_from: None,
        }
    }

    /// Garde tout à partir de `from` (clip sur mesure, session complète) ; `None` = tampon normal.
    pub fn set_hold(&mut self, from: Option<f64>) {
        self.hold_from = from;
    }

    /// Redémarre la capture tout de suite (problème détecté), sans compter un échec.
    fn restart(&mut self, why: &str) {
        log::warn!("redémarrage de la capture : {why}");
        self.stop_capture();
        self.next_retry = Instant::now() + Duration::from_millis(500);
    }

    /// Vérifications de santé, quelques fois par seconde.
    fn health(&mut self, now: f64) {
        // 1) Sortie de veille / gel du PC : l'horloge a sauté
        if self.last_tick > 0.0 && now - self.last_tick > 8.0 && self.capture.is_some() {
            self.restart(&format!("pause de {:.0} s (veille ou gel)", now - self.last_tick));
        }
        self.last_tick = now;
        // 2) Espace disque (toutes les 20 s)
        if self.last_disk.elapsed() >= Duration::from_secs(20) {
            self.last_disk = Instant::now();
            let free = [buffer_root(), PathBuf::from(&self.settings.clip_folder)]
                .iter()
                .filter_map(|p| util::free_space(p))
                .min()
                .map(|b| b as f64 / 1e9);
            if let Some(gb) = free {
                if !self.disk_low && gb < DISK_PAUSE_GB {
                    self.disk_low = true;
                    self.error = format!("Disque presque plein ({gb:.1} Go libres) : enregistrement en pause. Libère de la place, il reprendra tout seul.");
                    log::warn!("{}", self.error);
                    self.stop_capture();
                } else if self.disk_low && gb > DISK_RESUME_GB {
                    self.disk_low = false;
                    self.error.clear();
                    log::info!("espace disque revenu ({gb:.1} Go) : reprise");
                    self.next_retry = Instant::now();
                }
            }
        }
        // 3) Écrans et chien de garde (toutes les 2 s)
        if self.last_checks.elapsed() < Duration::from_secs(2) {
            return;
        }
        self.last_checks = Instant::now();
        let mut why: Option<String> = None;
        if let Some(c) = &self.capture {
            if util::monitors_signature() != c.monitors {
                why = Some("configuration des écrans modifiée".into());
            } else if c.started.elapsed() > Duration::from_secs(15) && c.last_video.elapsed() > Duration::from_secs(8) {
                why = Some(format!("image figée depuis {} s", c.last_video.elapsed().as_secs()));
            } else if c.audio_child.is_some()
                && c.started.elapsed() > Duration::from_secs(15)
                && c.last_audio.elapsed() > Duration::from_secs(12)
            {
                why = Some(format!("son figé depuis {} s", c.last_audio.elapsed().as_secs()));
            }
        }
        if let Some(w) = why {
            self.restart(&w);
        }
    }

    /// Durée gardée : réglage utilisateur, avec une marge pour les clips CS2 « fin de round ».
    fn retention(&self) -> f64 {
        let mut r = self.settings.buffer_seconds.max(self.settings.clip_long_seconds) as f64;
        if self.settings.cs2.enabled && self.settings.cs2.timing == "round" {
            r = r.max(200.0);
        }
        r + 3.0
    }

    fn video_key(s: &Settings) -> String {
        let (fps, res, br) = s.effective_video();
        format!(
            "{}|{}|{}|{}|{}|{}|{}|{}|{:?}|{}",
            s.capture_mode, s.monitor, s.window_exe, fps, res, br, s.encoder, s.show_cursor,
            s.audio_sources, s.separate_tracks
        ) + &format!("|{}|{}", s.audio_offset_ms, s.mic_denoise)
    }

    pub fn apply_settings(&mut self, settings: Settings) {
        let changed = Self::video_key(&settings) != Self::video_key(&self.settings);
        self.paused = !settings.recording_enabled;
        self.settings = settings;
        if changed {
            self.stop_capture();
            self.failures = 0;
            self.next_retry = Instant::now();
        }
    }

    pub fn set_paused(&mut self, paused: bool) {
        self.paused = paused;
        if paused {
            self.stop_capture();
        } else {
            self.failures = 0;
            self.next_retry = Instant::now();
        }
    }

    fn stop_capture(&mut self) {
        if let Some(mut c) = self.capture.take() {
            Self::read_lists(&mut c, &mut self.segments, &mut self.audio_segments);
        }
    }

    fn read_lists(c: &mut Capture, video: &mut VecDeque<Segment>, audio: &mut VecDeque<Segment>) {
        read_list(&c.dir, "video.csv", &mut c.vcsv, c.session, video);
        read_list(&c.dir, "audio.csv", &mut c.acsv, c.session, audio);
    }

    /// Appelé ~4 fois par seconde par le fil de surveillance.
    pub fn tick(&mut self) {
        let now = util::now();
        // Application au premier plan (une fois par seconde suffit)
        if self.apps.back().is_none_or(|(t, _)| now - t >= 1.0)
            && let Some(fg) = util::foreground()
            && !fg.exe.eq_ignore_ascii_case("rewind.exe")
        {
            // Rewind lui-même ne compte pas : on garde l'application vue juste avant
            self.apps.push_back((now, fg.exe));
        }
        let keep = self.retention() + 60.0;
        while self.apps.front().is_some_and(|(t, _)| now - t > keep) {
            self.apps.pop_front();
        }

        if self.paused {
            self.prune(now);
            self.last_tick = now;
            return;
        }
        self.health(now);
        if self.disk_low {
            self.prune(now);
            return;
        }
        // FFmpeg a-t-il quitté ?
        let mut crashed = false;
        if let Some(c) = self.capture.as_mut() {
            let video_exit = c.child.try_wait().ok().flatten().map(|s| ("image", s, "ffmpeg.log"));
            let audio_exit = c.audio_child.as_mut().and_then(|a| a.try_wait().ok().flatten()).map(|s| ("son", s, "ffmpeg-audio.log"));
            if let Some((which, status, logname)) = video_exit.or(audio_exit) {
                let log = fs::read_to_string(c.dir.join(logname)).unwrap_or_default();
                let last: Vec<&str> = log.lines().filter(|l| !l.trim().is_empty()).rev().take(3).collect();
                self.error = format!(
                    "L'enregistreur ({which}) s'est arrêté ({status}). {}",
                    last.into_iter().rev().collect::<Vec<_>>().join(" ")
                );
                log::warn!("{}", self.error);
                crashed = true;
            }
        }
        if crashed {
            self.stop_capture();
            self.failures += 1;
            // 1re panne : on relance tout de suite (chaque seconde perdue est un trou dans les clips) ;
            // si ça recommence, on espace les essais pour ne pas boucler
            self.next_retry = Instant::now()
                + if self.failures <= 2 { Duration::from_millis(200) } else { Duration::from_secs((2 * self.failures as u64).min(30)) };
        }
        if self.capture.is_none() && Instant::now() >= self.next_retry {
            match self.start_capture() {
                Ok(c) => {
                    self.capture = Some(c);
                }
                Err(e) => {
                    self.error = format!("{e:#}");
                    log::warn!("démarrage capture : {}", self.error);
                    self.failures += 1;
                    self.next_retry = Instant::now() + Duration::from_secs((3 * self.failures as u64).min(30));
                }
            }
        }
        if let Some(c) = self.capture.as_ref()
            && self.sessions.back().is_none_or(|s| s.id != c.session)
        {
            self.sessions.push_back(SessionInfo {
                id: c.session,
                key: format!("{}|{}x{}|{}|{}", c.encoder, c.width, c.height, c.fps, c.tracks.join(",")),
                tracks: c.tracks.clone(),
            });
        }
        if let Some(c) = self.capture.as_mut() {
            let before = self.segments.len();
            let before_audio = self.audio_segments.len();
            Self::read_lists(c, &mut self.segments, &mut self.audio_segments);
            if self.audio_segments.len() > before_audio {
                c.last_audio = Instant::now();
            }
            if self.segments.len() > before {
                c.last_video = Instant::now();
                self.error.clear();
                if c.started.elapsed() > Duration::from_secs(20) {
                    self.failures = 0;
                }
            }
        }
        self.prune(now);
    }

    fn prune(&mut self, now: f64) {
        let mut limit = now - self.retention();
        if let Some(h) = self.hold_from {
            limit = limit.min(h - 5.0);
        }
        while let Some(seg) = self.segments.front() {
            if seg.end >= limit {
                break;
            }
            let _ = fs::remove_file(&seg.path);
            self.segments.pop_front();
        }
        while let Some(seg) = self.audio_segments.front() {
            if seg.end >= limit {
                break;
            }
            let _ = fs::remove_file(&seg.path);
            self.audio_segments.pop_front();
        }
        // Fiches des sessions qui n'ont plus aucun segment
        let active_session = self.capture.as_ref().map(|c| c.session);
        let segments = &self.segments;
        self.sessions.retain(|s| Some(s.id) == active_session || segments.iter().any(|g| g.session == s.id));
        // Dossiers de sessions terminées et vides
        if let Ok(entries) = fs::read_dir(buffer_root()) {
            let active = self.capture.as_ref().map(|c| c.dir.clone());
            for e in entries.flatten() {
                let p = e.path();
                if Some(&p) == active.as_ref() || !p.is_dir() {
                    continue;
                }
                // Seulement les dossiers de session « s… » : jamais un export en cours d'assemblage
                // (bug du 2026-11-13 : le clip « 2K · Headshot » a perdu son dossier de travail).
                let name = p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
                if !name.starts_with('s') || name.starts_with("shot-") {
                    let old = e
                        .metadata()
                        .and_then(|m| m.modified())
                        .ok()
                        .and_then(|t| t.elapsed().ok())
                        .is_some_and(|age| age.as_secs() > 600);
                    if old {
                        let _ = fs::remove_dir_all(&p); // reste d'un export interrompu
                    }
                    continue;
                }
                let used = self.segments.iter().chain(self.audio_segments.iter()).any(|s| s.path.starts_with(&p));
                if !used {
                    let _ = fs::remove_dir_all(&p);
                }
            }
        }
    }

    fn monitor(&self) -> Option<util::Monitor> {
        let monitors = util::monitors();
        monitors
            .iter()
            .find(|m| !self.settings.monitor.is_empty() && m.name == self.settings.monitor)
            .or_else(|| monitors.iter().find(|m| m.primary))
            .or(monitors.first())
            .cloned()
    }

    /// Taille de sortie : la résolution choisie s'applique au petit côté (marche aussi pour un écran vertical).
    fn output_size(&self, res: u32) -> (u32, u32) {
        let (mw, mh) = self
            .monitor()
            .map(|m| (m.width.max(2) as u32, m.height.max(2) as u32))
            .unwrap_or((1920, 1080));
        let short = mw.min(mh);
        let scale = if res == 0 || res >= short { 1.0 } else { res as f64 / short as f64 };
        let even = |v: f64| ((v / 2.0).round() as u32 * 2).max(2);
        (even(mw as f64 * scale), even(mh as f64 * scale))
    }

    fn start_capture(&mut self) -> Result<Capture> {
        let tools = ffmpeg::tools()?;
        let s = self.settings.clone();
        let (mut fps, mut res, mut bitrate) = s.effective_video();
        let encoder = ffmpeg::pick_encoder(&s.encoder);
        if encoder == "libx264" && s.encoder != "libx264" {
            // Pas d'encodeur matériel : on protège le processeur (petit PC)
            fps = fps.min(30);
            res = if res == 0 { 720 } else { res.min(720) };
            bitrate = bitrate.min(6);
            log::warn!("aucun encodeur matériel : 720p30 sur le processeur");
        }
        let (width, height) = self.output_size(res);
        // On capture à la fréquence réelle de l'écran, puis on garde une cadence qui la divise
        // exactement (sinon les images sont irrégulières : saccades mesurées le 2026-11-13).
        let refresh = self.monitor().map(|m| m.refresh).unwrap_or(0);
        let capture_fps = if (24..=360).contains(&refresh) { refresh } else { fps };
        let wanted = fps;
        fps = pick_fps(fps, refresh);
        if fps != wanted {
            log::info!("{wanted} i/s demandés, écran à {refresh} Hz : {fps} i/s (cadence régulière)");
        }

        // Source vidéo
        let target = if s.capture_mode == "window" && !s.window_exe.trim().is_empty() {
            let exe = s.window_exe.trim();
            if !util::process_running(exe) {
                bail!("La fenêtre de {exe} n'est pas ouverte : en attente…");
            }
            let pattern: String = exe
                .chars()
                .map(|c| if c.is_ascii_alphanumeric() { c.to_string() } else { format!("\\{c}") })
                .collect();
            format!("window_exe=(?i){pattern}")
        } else {
            let m = self.monitor().context("Aucun écran trouvé")?;
            format!("hmonitor={}", m.handle)
        };
        let source = format!(
            "gfxcapture={target}:max_framerate={capture_fps}:capture_cursor={}:width={width}:height={height}:resize_mode=scale_aspect",
            u8::from(s.show_cursor)
        );

        self.session_counter += 1;
        let session = self.session_counter;
        let dir = buffer_root().join(format!("s{}-{session}", (util::now() * 1000.0) as u64));
        fs::create_dir_all(&dir)?;

        let audio = audio::Inputs::start(&s.audio_sources, Arc::clone(&self.mic_muted))?;
        let n = audio.pipes.len();
        let job = util::Job::new();

        // 1) Image seule : rien ne la fait attendre, donc des horodatages réguliers
        let mut vargs: Vec<String> = Vec::new();
        let mut push = |items: &[&str]| vargs.extend(items.iter().map(|x| x.to_string()));
        push(&["-hide_banner", "-loglevel", "warning", "-nostats", "-y", "-copyts"]);
        push(&["-use_wallclock_as_timestamps", "1", "-f", "lavfi", "-i", &source]);
        let mut vf = format!("fps={fps}");
        if let Some(f) = ffmpeg::upload_filter(&encoder) {
            vf.push(',');
            vf.push_str(f);
        }
        push(&["-vf", &vf, "-map", "0:v"]);
        vargs.extend(ffmpeg::video_args(&encoder, fps, bitrate));
        let mut push = |items: &[&str]| vargs.extend(items.iter().map(|x| x.to_string()));
        push(&[
            "-fps_mode", "passthrough", "-f", "segment", "-segment_format", "matroska", "-segment_time", "1",
            "-reset_timestamps", "1", "-segment_list", "video.csv", "-segment_list_type", "csv",
            "-segment_list_size", "0", "v%08d.mkv",
        ]);
        fs::write(dir.join("command.txt"), vargs.join(" ")).ok();
        let child = util::command(&tools.ffmpeg)
            .current_dir(&dir)
            .args(&vargs)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::from(fs::File::create(dir.join("ffmpeg.log"))?))
            .spawn()
            .context("Lancement de FFmpeg (image)")?;
        if let Some(j) = &job {
            j.adopt(&child);
        }

        // 2) Son : mixage + une piste par source, segments de 2 s calés sur l'horloge
        let mut tracks: Vec<String> = Vec::new();
        let mut audio_child = None;
        if n > 0 {
            let mut aargs: Vec<String> = Vec::new();
            let mut push = |items: &[&str]| aargs.extend(items.iter().map(|x| x.to_string()));
            push(&["-hide_banner", "-loglevel", "warning", "-nostats", "-y", "-copyts"]);
            for p in &audio.pipes {
                push(&[
                    "-use_wallclock_as_timestamps", "1", "-probesize", "32", "-analyzeduration", "0", "-f", "f32le",
                    "-ar", "48000", "-ac", "2", "-i", &p.name,
                ]);
            }
            // Compensation fixe de 110 ms, mesurée le 2026-11-13 (vidéo flash + bip jouée par Chrome et
            // par ffplay : le son arrivait 74-78 ms après l'image, en plus des ~35 ms déjà compensés).
            // Plus le réglage fin de l'utilisateur.
            let shift = -0.110 + s.audio_offset_ms as f64 / 1000.0;
            let fix = format!("asetpts=PTS{:+.4}/TB,aresample=async=1000", shift);
            // Micro : anti-bruit optionnel (coupe le grave parasite, puis débruitage FFT léger)
            let chain = |i: usize| -> String {
                if s.mic_denoise && audio.pipes[i].kind == "mic" {
                    format!("{fix},highpass=f=80,afftdn=nr=14:nf=-35:tn=1")
                } else {
                    fix.clone()
                }
            };
            let mut graph: Vec<String> = Vec::new();
            let mut maps: Vec<String> = Vec::new();
            if n == 1 {
                graph.push(format!("[0:a]{}[a0]", chain(0)));
                maps.push("[a0]".into());
                tracks.push(audio.pipes[0].label.clone());
            } else {
                let separate = s.separate_tracks;
                let mut mix_inputs = String::new();
                for i in 0..n {
                    if separate {
                        graph.push(format!("[{i}:a]{},asplit=2[m{i}][s{i}]", chain(i)));
                    } else {
                        graph.push(format!("[{i}:a]{}[m{i}]", chain(i)));
                    }
                    mix_inputs.push_str(&format!("[m{i}]"));
                }
                graph.push(format!("{mix_inputs}amix=inputs={n}:normalize=0:dropout_transition=0[mix]"));
                maps.push("[mix]".into());
                tracks.push("Mixage".into());
                if separate {
                    for (i, p) in audio.pipes.iter().enumerate() {
                        maps.push(format!("[s{i}]"));
                        tracks.push(p.label.clone());
                    }
                }
            }
            aargs.push("-filter_complex".into());
            aargs.push(graph.join(";"));
            for m in &maps {
                aargs.push("-map".into());
                aargs.push(m.clone());
            }
            let mut push = |items: &[&str]| aargs.extend(items.iter().map(|x| x.to_string()));
            push(&[
                "-c:a", "aac", "-b:a", "160k", "-ar", "48000", "-f", "segment", "-segment_format", "matroska",
                "-segment_time", "2", "-segment_atclocktime", "1", "-reset_timestamps", "1", "-segment_list",
                "audio.csv", "-segment_list_type", "csv", "-segment_list_size", "0", "a%08d.mka",
            ]);
            fs::write(dir.join("command-audio.txt"), aargs.join(" ")).ok();
            let a = util::command(&tools.ffmpeg)
                .current_dir(&dir)
                .args(&aargs)
                .stdin(Stdio::piped())
                .stdout(Stdio::null())
                .stderr(Stdio::from(fs::File::create(dir.join("ffmpeg-audio.log"))?))
                .spawn()
                .context("Lancement de FFmpeg (son)")?;
            if let Some(j) = &job {
                j.adopt(&a);
            }
            audio_child = Some(a);
        }
        log::info!("capture démarrée : {encoder} {width}x{height}@{fps} {n} pistes audio");
        Ok(Capture {
            child,
            audio_child,
            _job: job,
            audio,
            dir,
            vcsv: 0,
            acsv: 0,
            session,
            started: Instant::now(),
            last_video: Instant::now(),
            last_audio: Instant::now(),
            monitors: util::monitors_signature(),
            tracks,
            encoder,
            width,
            height,
            fps,
        })
    }

    pub fn status(&self) -> Status {
        let now = util::now();
        let current = self.segments.back().map(|s| s.session);
        let first = self.segments.iter().find(|s| Some(s.session) == current).map(|s| s.start);
        let last = self.segments.back().map(|s| s.end);
        let buffered = match (first, last) {
            (Some(a), Some(b)) => (b - a).min(self.settings.buffer_seconds as f64).max(0.0),
            _ => 0.0,
        };
        let mut st = Status {
            recording: self.capture.is_some() && !self.paused,
            starting: self.capture.is_some() && self.segments.iter().all(|s| Some(s.session) != self.capture.as_ref().map(|c| c.session)),
            buffered,
            buffer_target: self.settings.buffer_seconds as f64,
            buffer_mb: self.segments.iter().chain(self.audio_segments.iter()).map(|s| s.bytes).sum::<u64>() as f64 / 1_048_576.0,
            error: if self.paused { String::new() } else { self.error.clone() },
            app: self.apps.back().map(|(_, a)| util::friendly_app(a)).unwrap_or_default(),
            ..Default::default()
        };
        if let Some(c) = &self.capture {
            st.encoder = c.encoder.clone();
            st.width = c.width;
            st.height = c.height;
            st.fps = c.fps;
            st.audio = c
                .audio
                .pipes
                .iter()
                .map(|p| AudioLevel {
                    label: p.label.clone(),
                    level: f32::from_bits(p.level.load(Ordering::Relaxed)),
                    active: p.active.load(Ordering::Relaxed),
                })
                .collect();
        }
        let _ = now;
        st
    }

    /// Tests d'endurance : PID des FFmpeg en cours (image, son).
    pub fn ffmpeg_pids(&self) -> Vec<u32> {
        let mut v = Vec::new();
        if let Some(c) = &self.capture {
            v.push(c.child.id());
            if let Some(a) = &c.audio_child {
                v.push(a.id());
            }
        }
        v
    }

    /// Tests d'endurance : simule une panne (tue le FFmpeg image ou son).
    pub fn inject_fault(&mut self, audio: bool) -> bool {
        let Some(c) = self.capture.as_mut() else { return false };
        if audio {
            c.audio_child.as_mut().map(|a| a.kill().is_ok()).unwrap_or(false)
        } else {
            c.child.kill().is_ok()
        }
    }

    /// Application la plus présente à l'écran entre deux instants.
    pub fn main_app(&self, start: f64, end: f64) -> String {
        let mut counts: Vec<(String, u32)> = Vec::new();
        for (t, app) in &self.apps {
            if *t >= start - 1.0 && *t <= end + 1.0 {
                match counts.iter_mut().find(|(a, _)| a == app) {
                    Some((_, n)) => *n += 1,
                    None => counts.push((app.clone(), 1)),
                }
            }
        }
        counts.sort_by(|a, b| b.1.cmp(&a.1));
        counts.first().map(|(a, _)| util::friendly_app(a)).unwrap_or_else(|| "Bureau".into())
    }

    /// Vrai quand l'image ET le son jusqu'à `end` sont écrits sur le disque.
    /// Le son est découpé en morceaux de 2 s : sans cette attente, la fin du clip était muette
    /// (jusqu'à 1 s sans son, vu sur de vrais clips CS2 et par le test d'endurance du 2026-11-13).
    pub fn written_until(&self, end: f64) -> bool {
        let video = self.segments.back().is_some_and(|s| s.end >= end - 0.25);
        let audio = match &self.capture {
            Some(c) if c.audio_child.is_some() => {
                self.audio_segments.back().is_some_and(|a| a.session == c.session && a.end >= end - 0.15)
            }
            _ => true,
        };
        video && audio
    }

    pub fn track_labels(&self) -> Vec<String> {
        self.capture.as_ref().map(|c| c.tracks.clone()).unwrap_or_default()
    }

    /// Prépare un export : choisit les segments et les « fige » (lien physique) pour que le
    /// nettoyage du tampon ne les efface pas pendant la copie.
    ///
    /// Si l'enregistreur a redémarré pendant la période (panne, sortie de veille…), les morceaux
    /// d'avant et d'après sont recollés, chacun avec son propre calage du son (bug trouvé par le
    /// test d'endurance du 2026-11-13 : un clip juste après une panne ne faisait que 3 s).
    pub fn prepare_export(&self, start: f64, end: f64) -> Result<PreparedExport> {
        let Some(latest) = self.segments.back() else {
            bail!("Le tampon est encore vide : l'enregistrement vient de démarrer.");
        };
        // Sessions recollables : la plus récente et celles d'avant qui ont exactement les mêmes réglages
        let latest_info = self.sessions.iter().find(|s| s.id == latest.session);
        let mut allowed: Vec<u64> = vec![latest.session];
        if let Some(info) = latest_info {
            for s in self.sessions.iter().rev().skip_while(|s| s.id != info.id).skip(1) {
                if s.key != info.key {
                    break;
                }
                allowed.push(s.id);
            }
        }
        let chosen: Vec<&Segment> = self
            .segments
            .iter()
            .filter(|s| allowed.contains(&s.session) && s.end > start && s.start < end)
            .collect();
        if chosen.is_empty() {
            bail!("Cette période n'est plus dans le tampon.");
        }
        let work = buffer_root().join(format!("export-{}", library::new_id()));
        fs::create_dir_all(&work)?;
        // Liens physiques : le nettoyage du tampon ne peut pas effacer les fichiers pendant la copie
        let mut counter = 0usize;
        let mut freeze = |list: &[&Segment], ext: &str, prefix: &str| -> Result<(Vec<PathBuf>, Vec<f64>)> {
            let mut files = Vec::new();
            for seg in list {
                let dst = work.join(format!("{prefix}{counter:06}.{ext}"));
                counter += 1;
                if fs::hard_link(&seg.path, &dst).is_err() {
                    fs::copy(&seg.path, &dst)?;
                }
                files.push(dst);
            }
            // Durée exacte de chaque segment = écart avec le suivant (l'horloge fait foi)
            let durations = list
                .iter()
                .enumerate()
                .map(|(i, s)| list.get(i + 1).map(|n| n.start - s.start).unwrap_or(s.end - s.start).max(0.001))
                .collect();
            Ok((files, durations))
        };
        let mut parts = Vec::new();
        let mut i = 0;
        while i < chosen.len() {
            let session = chosen[i].session;
            let mut j = i;
            while j < chosen.len() && chosen[j].session == session {
                j += 1;
            }
            let group = &chosen[i..j];
            let v0 = group[0].start;
            let vend = group[group.len() - 1].end.min(end);
            let audio_group: Vec<&Segment> = self
                .audio_segments
                .iter()
                .filter(|a| a.session == session && a.end > v0 && a.start < vend)
                .collect();
            let (files, durations) = freeze(group, "mkv", "v")?;
            let (audio_files, audio_durations) = freeze(&audio_group, "mka", "a")?;
            parts.push(Part {
                files,
                durations,
                audio_files,
                audio_durations,
                audio_start: audio_group.first().map(|a| a.start).unwrap_or(v0),
                audio_end: audio_group.last().map(|a| a.end).unwrap_or(vend),
                first_start: v0,
                last_end: group[group.len() - 1].end,
            });
            i = j;
        }
        if parts.len() > 1 {
            log::info!("export recollé à partir de {} sessions", parts.len());
        }
        let tracks = latest_info.map(|s| s.tracks.clone()).unwrap_or_else(|| self.track_labels());
        Ok(PreparedExport {
            work,
            first_start: parts[0].first_start,
            last_end: parts[parts.len() - 1].last_end,
            parts,
            tracks,
        })
    }
}

/// Morceau continu d'un export (une seule session FFmpeg).
struct Part {
    files: Vec<PathBuf>,
    durations: Vec<f64>,
    audio_files: Vec<PathBuf>,
    audio_durations: Vec<f64>,
    audio_start: f64,
    /// Fin du dernier morceau de son écrit (plus tôt que l'image si le son a planté pendant le clip)
    audio_end: f64,
    first_start: f64,
    last_end: f64,
}

pub struct PreparedExport {
    work: PathBuf,
    parts: Vec<Part>,
    pub first_start: f64,
    pub last_end: f64,
    tracks: Vec<String>,
}

impl PreparedExport {
    /// Nombre de morceaux recollés (1 = pas de redémarrage pendant la période).
    pub fn parts(&self) -> usize {
        self.parts.len()
    }

    /// Nombre de pistes audio attendues dans le clip.
    pub fn track_count(&self) -> usize {
        self.tracks.len()
    }

    /// Position dans le clip d'une heure absolue (les trous entre sessions sont sautés).
    fn clip_time(&self, t: f64, end: f64) -> f64 {
        let mut offset = 0.0;
        for (i, p) in self.parts.iter().enumerate() {
            let last = i + 1 == self.parts.len();
            let p_end = if last { end.min(p.last_end) } else { p.last_end };
            if t < p.first_start {
                return offset;
            }
            if t <= p_end || last {
                return offset + (t - p.first_start).clamp(0.0, (p_end - p.first_start).max(0.0));
            }
            offset += (p_end - p.first_start).max(0.0);
        }
        offset
    }
}

pub struct ExportRequest {
    #[allow(dead_code)]
    pub start: f64,
    pub end: f64,
    pub title: String,
    pub game: String,
    pub kind: String,
    /// Marqueurs en heure absolue (converties en position dans le clip)
    pub markers: Vec<(f64, String, String)>,
    pub folder: PathBuf,
}

/// Attend (au plus `max`) que la période jusqu'à `end` soit écrite, image et son.
/// Le fil de surveillance continue de faire tourner l'enregistreur pendant ce temps.
pub fn wait_written(recorder: &Mutex<Recorder>, end: f64, max: Duration) -> bool {
    let deadline = Instant::now() + max;
    loop {
        if recorder.lock().written_until(end) {
            return true;
        }
        if Instant::now() > deadline {
            log::warn!("le tampon n'a pas rattrapé l'heure demandée : clip fait avec ce qui est écrit");
            return false;
        }
        std::thread::sleep(Duration::from_millis(60));
    }
}

/// Lit les nouvelles lignes complètes d'une liste de segments (« fichier,début,fin »).
fn read_list(dir: &Path, list: &str, offset: &mut u64, session: u64, segments: &mut VecDeque<Segment>) {
    let Ok(mut file) = fs::File::open(dir.join(list)) else { return };
    if file.seek(SeekFrom::Start(*offset)).is_err() {
        return;
    }
    let mut text = String::new();
    if file.read_to_string(&mut text).is_err() {
        return;
    }
    let complete = match text.rfind('\n') {
        Some(i) => i + 1,
        None => return,
    };
    *offset += complete as u64;
    for line in text[..complete].lines() {
        let parts: Vec<&str> = line.trim().rsplitn(3, ',').collect();
        if parts.len() != 3 {
            continue;
        }
        let (Ok(end), Ok(mut start)) = (parts[0].parse::<f64>(), parts[1].parse::<f64>()) else { continue };
        // Le tout premier segment d'une session annonce parfois un début à 0 : on le reconstitue
        if start < 1.0e9 {
            start = segments.back().filter(|s| s.session == session).map(|s| s.end).unwrap_or(end - 1.0);
        }
        let file = dir.join(parts[2].trim_matches('"'));
        let bytes = fs::metadata(&file).map(|m| m.len()).unwrap_or(0);
        if bytes == 0 || end <= start {
            continue;
        }
        segments.push_back(Segment { path: file, start, end, bytes, session });
    }
}

/// Assemble le clip (sans réencodage), crée la miniature et renvoie la fiche du clip.
pub fn export(prepared: PreparedExport, req: ExportRequest) -> Result<Clip> {
    let reserved: std::cell::RefCell<Option<PathBuf>> = std::cell::RefCell::new(None);
    let result = (|| -> Result<Clip> {
        let tools = ffmpeg::tools()?;
        let quote = |p: &Path| p.to_string_lossy().replace('\'', "'\\''");
        let write_list = |name: &str, files: &[PathBuf], durations: &[f64]| -> Result<PathBuf> {
            let list = prepared.work.join(name);
            let mut text = String::new();
            for (f, d) in files.iter().zip(durations) {
                text.push_str(&format!("file '{}'\nduration {d:.6}\n", quote(f)));
            }
            fs::write(&list, text)?;
            Ok(list)
        };
        let clip_start = prepared.first_start;
        let clip_end = req.end.min(prepared.last_end);

        let stamp = chrono::Local::now().format("%Y-%m-%d %Hh%M-%S");
        // Nom réservé tout de suite : deux clips à la même seconde ne se marchent plus dessus
        // (« fichier utilisé par un autre processus », vu par le test d'endurance)
        let out = util::reserve_file(&req.folder, &util::safe_name(&format!("{} {stamp}", req.title)), "mp4")?;
        reserved.replace(Some(out.clone()));
        let partial = out.with_extension("partial.mp4");
        let single = prepared.parts.len() == 1;
        let add_final_meta = |cmd: &mut std::process::Command| {
            for (i, t) in prepared.tracks.iter().enumerate() {
                cmd.arg(format!("-metadata:s:a:{i}")).arg(format!("title={t}"));
                cmd.arg(format!("-disposition:a:{i}")).arg(if i == 0 { "default" } else { "0" });
            }
            cmd.args(["-metadata", &format!("title={}", req.title), "-movflags", "+faststart"]);
        };
        // Chaque morceau : image + son recalé, coupé à sa durée exacte
        let mut part_files = Vec::new();
        let mut total = 0.0;
        for (i, part) in prepared.parts.iter().enumerate() {
            let last = i + 1 == prepared.parts.len();
            let p_end = if last { req.end.min(part.last_end) } else { part.last_end };
            let duration = (p_end - part.first_start).max(if single { 0.5 } else { 0.05 });
            total += duration;
            let list = write_list(&format!("video{i}.txt"), &part.files, &part.durations)?;
            let mut cmd = util::command(&tools.ffmpeg);
            cmd.args(["-hide_banner", "-loglevel", "error", "-y", "-f", "concat", "-safe", "0", "-i"]).arg(&list);
            // Morceaux à recoller : le son est réencodé (rapide) pour être coupé pile au début et
            // complété jusqu'à la fin de l'image. Copié tel quel, il débordait de ~0,8 s avant l'image
            // et décalait le départ de la vidéo (test --stitch du 2026-11-13).
            // Son qui démarre après l'image (juste après une relance, le son repart ~0,5 s après) :
            // on le réencode aussi, précédé de silence, pour que son et image commencent ensemble
            // (test d'endurance de 3 h : 10 clips « son qui commence à 0,3-0,7 s »).
            // Son qui s'arrête avant l'image (son tué pendant le clip, test d'endurance d'1 h :
            // « son 15,28 s, image 15,86 s ») : même traitement, complété de silence jusqu'au bout.
            let late = part.audio_start - part.first_start;
            let early_end = p_end - part.audio_end;
            let encode_audio = !prepared.tracks.is_empty() && (!single || (!part.audio_files.is_empty() && (late > 0.05 || early_end > 0.05)));
            if !part.audio_files.is_empty() && early_end > 0.05 {
                log::info!("son fini {early_end:.2} s avant l'image : complété de silence");
            }
            if !part.audio_files.is_empty() {
                let alist = write_list(&format!("audio{i}.txt"), &part.audio_files, &part.audio_durations)?;
                // Aligne le son sur l'image grâce à l'heure de début de chacun
                let delta = part.first_start - part.audio_start;
                if delta >= 0.0 {
                    cmd.args(["-ss", &format!("{delta:.4}")]);
                } else if !encode_audio {
                    cmd.args(["-itsoffset", &format!("{:.4}", -delta)]);
                }
                cmd.args(["-f", "concat", "-safe", "0", "-i"]).arg(alist);
                cmd.args(["-map", "0:v", "-map", "1:a"]);
                if encode_audio {
                    let mut af = String::new();
                    if delta < 0.0 {
                        af.push_str(&format!("adelay={}:all=1,", (-delta * 1000.0).round() as u64));
                    }
                    af.push_str("apad");
                    cmd.args(["-af", &af]);
                }
            } else if encode_audio {
                // Morceau sans son (le son a lâché) : silence, pour que les morceaux se recollent
                cmd.args(["-f", "lavfi", "-i", "anullsrc=r=48000:cl=stereo", "-map", "0:v"]);
                for _ in &prepared.tracks {
                    cmd.args(["-map", "1:a"]);
                }
            } else {
                cmd.args(["-map", "0:v"]);
            }
            cmd.args(["-t", &format!("{duration:.3}")]);
            if encode_audio {
                cmd.args(["-c:v", "copy", "-c:a", "aac", "-b:a", "160k", "-ar", "48000"]);
            } else {
                cmd.args(["-c", "copy"]);
            }
            let target = if single { partial.clone() } else { prepared.work.join(format!("part{i}.mkv")) };
            if single {
                add_final_meta(&mut cmd);
            }
            cmd.arg(&target);
            let output = cmd.stdin(Stdio::null()).output()?;
            ffmpeg::checked(output, "Assemblage du clip")?;
            part_files.push((target, duration));
        }
        if !single {
            // Recollage des morceaux sans réencodage, chacun avec sa durée exacte
            let mut text = String::new();
            for (f, d) in &part_files {
                text.push_str(&format!("file '{}'\nduration {d:.6}\n", quote(f)));
            }
            let list = prepared.work.join("parts.txt");
            fs::write(&list, text)?;
            let mut cmd = util::command(&tools.ffmpeg);
            cmd.args(["-hide_banner", "-loglevel", "error", "-y", "-f", "concat", "-safe", "0", "-i"]).arg(&list);
            cmd.args(["-map", "0", "-c", "copy"]);
            add_final_meta(&mut cmd);
            cmd.arg(&partial);
            let output = cmd.stdin(Stdio::null()).output()?;
            ffmpeg::checked(output, "Recollage du clip")?;
        }
        fs::rename(&partial, &out)?;
        let duration = total;

        let id = library::new_id();
        let thumb = library::thumbs_dir().join(format!("{id}.jpg"));
        let real = ffmpeg::duration(&out).unwrap_or(duration);
        let thumb_at = req
            .markers
            .first()
            .map(|(t, _, _)| prepared.clip_time(*t, req.end).clamp(0.0, (real - 0.1).max(0.0)))
            .unwrap_or(real * 0.6);
        let _ = ffmpeg::thumbnail(&out, thumb_at, &thumb);
        let markers = req
            .markers
            .iter()
            .filter(|(t, _, _)| *t >= clip_start - 0.5 && *t <= clip_end + 0.5)
            .map(|(t, kind, label)| Marker {
                time: prepared.clip_time(*t, req.end).clamp(0.0, real),
                kind: kind.clone(),
                label: label.clone(),
            })
            .collect();
        Ok(Clip {
            id,
            path: out.to_string_lossy().to_string(),
            thumb: if thumb.is_file() { thumb.to_string_lossy().to_string() } else { String::new() },
            title: req.title.clone(),
            game: req.game.clone(),
            created: util::now(),
            duration: real,
            size: fs::metadata(&out).map(|m| m.len()).unwrap_or(0),
            kind: req.kind.clone(),
            markers,
            favorite: false,
            tags: Vec::new(),
        })
    })();
    if result.is_err() {
        // Pas de fichier vide ou à moitié écrit laissé dans le dossier des clips
        if let Some(out) = reserved.borrow().as_ref() {
            if fs::metadata(out).map(|m| m.len() == 0).unwrap_or(false) {
                let _ = fs::remove_file(out);
            }
            let _ = fs::remove_file(out.with_extension("partial.mp4"));
        }
    }
    if std::env::var_os("REWIND_KEEP_WORK").is_none() {
        let _ = fs::remove_dir_all(&prepared.work);
    }
    result
}

/// Capture d'écran : la dernière image du segment le plus récent (instantané, marche en plein écran).
pub fn screenshot(recorder: &Mutex<Recorder>, folder: &Path, app: &str) -> Result<Clip> {
    let seg = {
        let r = recorder.lock();
        r.segments.back().cloned().context("Rien n'est encore enregistré.")?
    };
    let tools = ffmpeg::tools()?;
    let tmp = buffer_root().join(format!("shot-{}.mkv", library::new_id()));
    if fs::hard_link(&seg.path, &tmp).is_err() {
        fs::copy(&seg.path, &tmp)?;
    }
    fs::create_dir_all(folder)?;
    let stamp = chrono::Local::now().format("%Y-%m-%d %Hh%M-%S");
    let out = folder.join(format!("{} {stamp}.png", util::safe_name(app)));
    let output = util::command(&tools.ffmpeg)
        .args(["-hide_banner", "-loglevel", "error", "-y", "-sseof", "-0.05", "-i"])
        .arg(&tmp)
        .args(["-frames:v", "1", "-update", "1"])
        .arg(&out)
        .stdin(Stdio::null())
        .output();
    let _ = fs::remove_file(&tmp);
    ffmpeg::checked(output?, "Capture d'écran")?;
    Ok(Clip {
        id: library::new_id(),
        thumb: out.to_string_lossy().to_string(),
        path: out.to_string_lossy().to_string(),
        title: format!("Capture {app}"),
        game: app.to_string(),
        created: util::now(),
        size: fs::metadata(&out).map(|m| m.len()).unwrap_or(0),
        kind: "screenshot".into(),
        ..Default::default()
    })
}

/// Lit les dernières lignes du journal FFmpeg actif (diagnostic).
pub fn last_log() -> String {
    let mut newest: Option<(std::time::SystemTime, PathBuf)> = None;
    if let Ok(entries) = fs::read_dir(buffer_root()) {
        for e in entries.flatten() {
            let p = e.path().join("ffmpeg.log");
            if let Ok(m) = fs::metadata(&p).and_then(|m| m.modified())
                && newest.as_ref().is_none_or(|(t, _)| m > *t)
            {
                newest = Some((m, p));
            }
        }
    }
    newest
        .and_then(|(_, p)| fs::File::open(p).ok())
        .map(|f| {
            let lines: Vec<String> = BufReader::new(f).lines().map_while(Result::ok).collect();
            lines.iter().rev().take(30).rev().cloned().collect::<Vec<_>>().join("\n")
        })
        .unwrap_or_default()
}

pub fn clip_folder(settings: &Settings, game: &str) -> PathBuf {
    let base = PathBuf::from(&settings.clip_folder);
    if settings.folder_per_game && !game.is_empty() {
        base.join(util::safe_name(game))
    } else {
        base
    }
}


#[cfg(test)]
mod tests {
    use super::*;

    fn part(first_start: f64, last_end: f64) -> Part {
        Part { files: vec![], durations: vec![], audio_files: vec![], audio_durations: vec![], audio_start: first_start, audio_end: last_end, first_start, last_end }
    }

    #[test]
    fn cadence_reguliere() {
        assert_eq!(pick_fps(60, 100), 50);
        assert_eq!(pick_fps(60, 144), 72);
        assert_eq!(pick_fps(60, 60), 60);
        assert_eq!(pick_fps(60, 0), 60);
        assert_eq!(pick_fps(30, 165), 33);
    }

    #[test]
    fn reperes_dans_un_clip_recolle() {
        // Morceau 1 : 100 → 105 ; panne ; morceau 2 : 109 → 120, clip demandé jusqu'à 118
        let p = PreparedExport { work: PathBuf::new(), parts: vec![part(100.0, 105.0), part(109.0, 120.0)], first_start: 100.0, last_end: 120.0, tracks: vec![] };
        assert!((p.clip_time(102.0, 118.0) - 2.0).abs() < 1e-9);
        assert!((p.clip_time(107.0, 118.0) - 5.0).abs() < 1e-9, "un repère dans le trou va au début du morceau suivant");
        assert!((p.clip_time(110.0, 118.0) - 6.0).abs() < 1e-9);
        assert!((p.clip_time(119.0, 118.0) - 14.0).abs() < 1e-9, "borné à la fin du clip");
        assert!((p.clip_time(90.0, 118.0) - 0.0).abs() < 1e-9);
    }
}
