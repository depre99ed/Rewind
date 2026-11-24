//! Moteur d'enregistrement « replay buffer ».
//!
//! FFmpeg capture l'écran par le GPU (Windows Graphics Capture, sans injection), encode en continu
//! (NVENC / AMF / QuickSync) et écrit des segments d'environ 1 seconde dans un dossier tampon.
//! Les horodatages sont ceux de l'horloge du PC (`-use_wallclock_as_timestamps` + `-copyts`) :
//! chaque segment sait donc exactement à quelle heure il a été filmé, ce qui permet de faire
//! correspondre un kill CS2 ou un appui de touche à la bonne image.
//! Un clip = recopie (sans réencodage) des segments couvrant la période demandée.

use crate::util::OutputTimeout;
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
    /// Segment gardé en RAM (`ram::get(nom du fichier)`), pas sur le disque
    pub ram: bool,
}

impl Segment {
    fn ram_name(&self) -> String {
        self.path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default()
    }

    /// Copie le segment vers `dst` (lien physique si possible, sinon copie ; depuis la RAM sinon).
    pub fn freeze(&self, dst: &Path) -> Result<()> {
        if self.ram {
            let data = crate::ram::get(&self.ram_name()).context("segment sorti de la mémoire")?;
            fs::write(dst, &*data)?;
        } else if fs::hard_link(&self.path, dst).is_err() {
            fs::copy(&self.path, dst)?;
        }
        Ok(())
    }

    fn discard(&self) {
        if self.ram {
            crate::ram::remove(&self.ram_name());
        } else {
            let _ = fs::remove_file(&self.path);
        }
    }
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
    /// Au moins un segment image reçu (une panne avant = l'encodeur n'a jamais marché)
    got_video: bool,
    /// Mode RAM : réception des segments image et son (arrêtée après FFmpeg, voir Drop)
    ram: Option<(crate::ram::Sink, Option<crate::ram::Sink>)>,
    /// Mode RAM : nombre de passages où le segment annoncé n'était pas encore reçu
    ram_wait: u32,
    width: u32,
    height: u32,
    fps: u32,
}

fn stop_child(child: &mut Child) {
    // Déjà arrêté (stop_capture) : rien à faire
    if let Ok(Some(_)) = child.try_wait() {
        return;
    }
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

/// Arrête l'image et le son en même temps : « q » aux deux, une seule attente commune (1,5 s).
fn stop_children(video: &mut Child, audio: Option<&mut Child>) {
    use std::io::Write;
    let mut all: Vec<&mut Child> = vec![video];
    if let Some(a) = audio {
        all.push(a);
    }
    for c in all.iter_mut() {
        if let Some(stdin) = c.stdin.as_mut() {
            let _ = stdin.write_all(b"q");
            let _ = stdin.flush();
        }
    }
    let deadline = Instant::now() + Duration::from_millis(1500);
    while Instant::now() < deadline {
        if all.iter_mut().all(|c| matches!(c.try_wait(), Ok(Some(_)))) {
            return;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    for c in all.iter_mut() {
        if !matches!(c.try_wait(), Ok(Some(_))) {
            let _ = c.kill();
            let _ = c.wait();
        }
    }
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
    /// Repli sur le processeur sans l'avoir choisi : qualité bridée à 720p30 pour protéger le jeu
    pub limited: bool,
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
    /// Repli d'encodage : 0 = carte graphique sans copie, 1 = carte graphique avec copie des images,
    /// 2 = processeur. Monte quand l'encodeur plante deux fois avant la 1re image (AMD RX 7600,
    /// 2026-11-18 : AMF « SubmitInput() failed with error 18 » sur les textures de la capture).
    fallback: u8,
    early_crashes: u32,
    /// Dernier grand ménage (mémoire RAM non listée, dossiers de session) : toutes les 5 s suffisent
    last_sweep: Instant,
    /// Secondes passées par la boucle du moteur à attendre son verrou depuis le dernier passage
    lock_wait: f64,
    /// Réglages d'image changés pendant une session complète ou un clip sur mesure : la capture ne
    /// redémarre qu'à la fin (sinon le clip ne contiendrait que la partie d'après, audit 0.4.12)
    restart_after_hold: bool,
}

/// Une session = un lancement de FFmpeg. Deux sessions de même `key` peuvent être recollées.
#[derive(Clone, Debug)]
struct SessionInfo {
    id: u64,
    key: String,
    tracks: Vec<String>,
}

/// En dessous : pause ; au-dessus : reprise (Go)
/// Fil d'écriture du tampon RAM vers le disque : segments écrits (nom, fichier), écriture en cours
#[derive(Default)]
struct Spill {
    done: Vec<(String, PathBuf)>,
    busy: bool,
}
static SPILL: Mutex<Spill> = Mutex::new(Spill { done: Vec::new(), busy: false });

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
            last_disk: util::ago(Duration::from_secs(60)),
            disk_low: false,
            sessions: VecDeque::new(),
            hold_from: None,
            fallback: 0,
            early_crashes: 0,
            last_sweep: Instant::now(),
            lock_wait: 0.0,
            restart_after_hold: false,
        }
    }

    /// Temps d'attente du verrou par la boucle du moteur (voir `health`).
    pub fn note_lock_wait(&mut self, waited: Duration) {
        self.lock_wait += waited.as_secs_f64();
    }

    /// Garde tout à partir de `from` (clip sur mesure, session complète) ; `None` = tampon normal.
    pub fn set_hold(&mut self, from: Option<f64>) {
        self.hold_from = from;
        if from.is_none() && std::mem::take(&mut self.restart_after_hold) {
            log::info!("fin de l'enregistrement long : nouveaux réglages d'image appliqués");
            self.stop_capture();
        }
    }

    /// Redémarre la capture tout de suite (problème détecté), sans compter un échec.
    fn restart(&mut self, why: &str) {
        log::warn!("redémarrage de la capture : {why}");
        self.stop_capture();
        self.next_retry = Instant::now() + Duration::from_millis(500);
    }

    /// Vérifications de santé, quelques fois par seconde.
    fn health(&mut self, now: f64) {
        // 1) Sortie de veille / gel du PC : l'horloge a sauté. Le temps passé à attendre le verrou du
        // moteur (un export qui le tenait) n'en fait pas partie : ce n'est pas un gel (audit 0.4.12).
        let gap = now - self.last_tick - std::mem::take(&mut self.lock_wait);
        if self.last_tick > 0.0 && gap > 8.0 && self.capture.is_some() {
            self.restart(&format!("pause de {gap:.0} s (veille ou gel)"));
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
        ) + &format!("|{}|{}|{}", s.audio_offset_ms, s.mic_denoise, s.buffer_in_ram)
    }

    pub fn apply_settings(&mut self, settings: Settings) {
        let changed = Self::video_key(&settings) != Self::video_key(&self.settings);
        self.paused = !settings.recording_enabled;
        self.settings = settings;
        if changed && self.hold_from.is_some() && self.capture.is_some() {
            log::info!("réglages d'image changés pendant un enregistrement long : appliqués à la fin");
            self.restart_after_hold = true;
        } else if changed {
            self.stop_capture();
            self.failures = 0;
            self.fallback = 0;
            self.early_crashes = 0;
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
            // 2026-11-24 : on arrête d'abord les deux FFmpeg ENSEMBLE (avant : l'un après l'autre,
            // jusqu'à 3 s sous le verrou), puis on lit les listes : le dernier segment, écrit par
            // FFmpeg en quittant, est maintenant gardé (avant, il était lu trop tôt et perdu).
            stop_children(&mut c.child, c.audio_child.as_mut());
            Self::read_lists(&mut c, &mut self.segments, &mut self.audio_segments);
            // Mode RAM : le fil de réception range le dernier segment juste après la fin de FFmpeg
            if c.ram.is_some() {
                for _ in 0..6 {
                    if c.ram_wait == 0 {
                        break;
                    }
                    std::thread::sleep(Duration::from_millis(50));
                    Self::read_lists(&mut c, &mut self.segments, &mut self.audio_segments);
                }
            }
        }
    }

    fn read_lists(c: &mut Capture, video: &mut VecDeque<Segment>, audio: &mut VecDeque<Segment>) {
        let ram = c.ram.is_some();
        read_list(&c.dir, "video.csv", &mut c.vcsv, c.session, video, ram, &mut c.ram_wait);
        read_list(&c.dir, "audio.csv", &mut c.acsv, c.session, audio, ram, &mut c.ram_wait);
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
        let mut early = false;
        if let Some(c) = self.capture.as_mut() {
            let video_exit = c.child.try_wait().ok().flatten().map(|s| ("image", s, "ffmpeg.log"));
            let audio_exit = c.audio_child.as_mut().and_then(|a| a.try_wait().ok().flatten()).map(|s| ("son", s, "ffmpeg-audio.log"));
            if let Some((which, status, logname)) = video_exit.or(audio_exit) {
                let log = util::read_tail(&c.dir.join(logname), 8 << 10);
                let last: Vec<&str> = log.lines().filter(|l| !l.trim().is_empty()).rev().take(3).collect();
                self.error = format!(
                    "L'enregistreur ({which}) s'est arrêté ({status}). {}",
                    last.into_iter().rev().collect::<Vec<_>>().join(" ")
                );
                log::warn!("{}", self.error);
                crashed = true;
                early = !c.got_video && c.encoder != "libx264";
            }
        }
        if crashed {
            self.stop_capture();
            self.failures += 1;
            self.early_crashes = if early { self.early_crashes + 1 } else { 0 };
            // 1re panne : on relance tout de suite (chaque seconde perdue est un trou dans les clips) ;
            // si ça recommence, on espace les essais pour ne pas boucler
            self.next_retry = Instant::now()
                + if self.failures <= 2 { Duration::from_millis(200) } else { Duration::from_secs((2 * self.failures as u64).min(30)) };
            if self.early_crashes >= 2 && self.fallback < 2 {
                self.fallback += 1;
                self.early_crashes = 0;
                self.failures = 0;
                self.next_retry = Instant::now();
                log::warn!(
                    "l'encodeur plante avant la première image : repli sur {}",
                    if self.fallback == 1 { "la carte graphique avec copie des images" } else { "le processeur (720p30)" }
                );
            }
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
                c.got_video = true;
                self.early_crashes = 0;
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
            seg.discard();
            self.segments.pop_front();
        }
        while let Some(seg) = self.audio_segments.front() {
            if seg.end >= limit {
                break;
            }
            seg.discard();
            self.audio_segments.pop_front();
        }
        // Le reste parcourt tous les segments et le dossier tampon : toutes les 5 s, pas 4 fois par seconde
        // (2026-11-24 : des centaines d'allocations par tick pour rien).
        if self.last_sweep.elapsed() < Duration::from_secs(5) {
            return;
        }
        self.last_sweep = Instant::now();
        self.spill_ram(now);
        // RAM : segments reçus mais jamais listés (fin de session coupée), ou d'une ancienne session
        {
            let used: std::collections::HashSet<String> =
                self.segments.iter().chain(self.audio_segments.iter()).filter(|s| s.ram).map(|s| s.ram_name().to_lowercase()).collect();
            let live = self.capture.as_ref().filter(|c| c.ram.is_some()).map(|c| format!("rw-{}-", c.session));
            crate::ram::retain(|k| used.contains(k) || live.as_ref().is_some_and(|p| k.starts_with(p.as_str())));
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

    /// Mode RAM + clip sur mesure / session complète : tout est gardé depuis `hold_from`. Au-delà d'un
    /// plafond (un quart de la RAM du PC, 4 Go au plus), les segments plus vieux que le tampon normal
    /// partent sur le disque, dans le dossier de la session (2026-11-24 : avant, la mémoire montait sans
    /// limite, ~4,5 Go par heure à 10 Mb/s). Le clip est identique : il lit le fichier au lieu de la RAM.
    ///
    /// 2026-11-24 (audit 0.4.12) : le plafond vaut aussi pour le tampon ORDINAIRE (30 min à 80 Mb/s =
    /// ~18 Go de RAM), et l'écriture se fait sur un fil à part, hors du verrou du moteur : un disque
    /// lent ne retarde plus l'enregistrement. Les plus vieux segments partent d'abord ; un segment ne
    /// quitte la RAM qu'une fois son fichier écrit (un export en cours le lit toujours quelque part).
    fn spill_ram(&mut self, _now: f64) {
        // 1) Segments déjà écrits par le fil : ils sont lus sur le disque à partir de maintenant
        let done = std::mem::take(&mut SPILL.lock().done);
        for (name, path) in done {
            let seg = self.segments.iter_mut().chain(self.audio_segments.iter_mut()).find(|s| s.ram && s.ram_name() == name);
            match seg {
                Some(s) => {
                    s.ram = false;
                    crate::ram::remove(&name);
                }
                // Sorti du tampon pendant l'écriture : le fichier ne sert plus
                None => {
                    let _ = fs::remove_file(&path);
                }
            }
        }
        let mut cap = crate::ram::memory().map(|m| m.0 / 4).unwrap_or(2 << 30).min(4 << 30);
        // Test : plafond réduit pour vérifier le passage sur le disque (`--hold`)
        if let Some(mb) = std::env::var("REWIND_TEST_RAM_CAP_MB").ok().and_then(|v| v.parse::<u64>().ok()) {
            cap = mb << 20;
        }
        let total = crate::ram::total_bytes();
        if total <= cap || SPILL.lock().busy {
            return;
        }
        // 2) Les plus anciens segments (image et son) jusqu'à revenir à 85 % du plafond
        let target = cap / 100 * 85;
        let mut list: Vec<&Segment> = self.segments.iter().chain(self.audio_segments.iter()).filter(|s| s.ram).collect();
        list.sort_by(|a, b| a.start.total_cmp(&b.start));
        let mut batch: Vec<(String, PathBuf, Arc<Vec<u8>>)> = Vec::new();
        let mut freed = 0u64;
        for seg in list {
            if total - freed <= target {
                break;
            }
            let name = seg.ram_name();
            let Some(data) = crate::ram::get(&name) else { continue };
            freed += data.len() as u64;
            batch.push((name, seg.path.clone(), data));
        }
        if batch.is_empty() {
            return;
        }
        SPILL.lock().busy = true;
        let n = batch.len();
        let held = self.hold_from.is_some();
        let _ = std::thread::Builder::new().name("tampon vers disque".into()).spawn(move || {
            let mut ok = Vec::new();
            let mut failed = 0u32;
            for (name, path, data) in batch {
                match fs::write(&path, &*data) {
                    Ok(()) => ok.push((name, path)),
                    Err(_) => failed += 1,
                }
            }
            let written = ok.len();
            let mut s = SPILL.lock();
            s.done.extend(ok);
            s.busy = false;
            drop(s);
            log::info!(
                "tampon RAM plein{} : {written}/{n} segment(s) anciens écrits sur le disque{}",
                if held { " (session gardée)" } else { "" },
                if failed > 0 { format!(", {failed} échec(s) : gardés en RAM") } else { String::new() }
            );
        });
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
        let encoder = if self.fallback >= 2 { "libx264".to_string() } else { ffmpeg::pick_encoder(&s.encoder) };
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

        // Mode RAM : FFmpeg écrit chaque segment dans un tuyau, Rewind le garde en mémoire
        let stamp = (util::now() * 1000.0) as u64;
        let ram_video = if s.buffer_in_ram { Some(crate::ram::Sink::start(&format!("rw-{session}-{stamp}-v"), "mkv")?) } else { None };
        let video_out = ram_video.as_ref().map(|r| r.pattern()).unwrap_or_else(|| "v%08d.mkv".into());

        let audio = audio::Inputs::start(&s.audio_sources, Arc::clone(&self.mic_muted))?;
        let n = audio.pipes.len();
        let job = util::Job::new();

        // 1) Image seule : rien ne la fait attendre, donc des horodatages réguliers
        let mut vargs: Vec<String> = Vec::new();
        let mut push = |items: &[&str]| vargs.extend(items.iter().map(|x| x.to_string()));
        push(&["-hide_banner", "-loglevel", "warning", "-nostats", "-y", "-copyts"]);
        push(&["-use_wallclock_as_timestamps", "1", "-f", "lavfi", "-i", &source]);
        let mut vf = format!("fps={fps}");
        // Test du repli : REWIND_TEST_HW_FAIL=1 fait échouer la capture « sans copie » dès le départ, =2 aussi avec copie
        let fail_below = std::env::var("REWIND_TEST_HW_FAIL").ok().and_then(|v| v.parse::<u8>().ok()).unwrap_or(0);
        if self.fallback < fail_below && encoder != "libx264" {
            vf.push_str(",panne_simulee_rewind");
        }
        let upload = if self.fallback == 1 && encoder != "libx264" { Some(ffmpeg::COPY_FILTER) } else { ffmpeg::upload_filter(&encoder) };
        if let Some(f) = upload {
            vf.push(',');
            vf.push_str(f);
        }
        push(&["-vf", &vf, "-map", "0:v"]);
        vargs.extend(ffmpeg::video_args(&encoder, fps, bitrate));
        let mut push = |items: &[&str]| vargs.extend(items.iter().map(|x| x.to_string()));
        push(&[
            "-fps_mode", "passthrough", "-f", "segment", "-segment_format", "matroska", "-segment_time", "1",
            "-reset_timestamps", "1", "-segment_list", "video.csv", "-segment_list_type", "csv",
            "-segment_list_size", "0", &video_out,
        ]);
        fs::write(dir.join("command.txt"), vargs.join(" ")).ok();
        let child = util::command_capture(&tools.ffmpeg, encoder == "libx264")
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
        let mut ram_audio: Option<crate::ram::Sink> = None;
        if n > 0 {
            if s.buffer_in_ram {
                ram_audio = Some(crate::ram::Sink::start(&format!("rw-{session}-{stamp}-a"), "mka")?);
            }
            let audio_out = ram_audio.as_ref().map(|r| r.pattern()).unwrap_or_else(|| "a%08d.mka".into());
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
                "audio.csv", "-segment_list_type", "csv", "-segment_list_size", "0", &audio_out,
            ]);
            fs::write(dir.join("command-audio.txt"), aargs.join(" ")).ok();
            let a = util::command_capture(&tools.ffmpeg, false)
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
        log::info!(
            "capture démarrée : {encoder}{} {width}x{height}@{fps} {n} pistes audio, tampon {}",
            if upload == Some(ffmpeg::COPY_FILTER) && encoder != "libx264" { " (images copiées)" } else { "" },
            if ram_video.is_some() { "en RAM" } else { "sur le disque" }
        );
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
            got_video: false,
            ram: ram_video.map(|v| (v, ram_audio)),
            ram_wait: 0,
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
            st.limited = c.encoder == "libx264" && self.settings.encoder != "libx264";
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
        let name = counts.first().map(|(a, _)| util::friendly_app(a)).unwrap_or_else(|| "Bureau".into());
        // Nom du clip et de son dossier dans la langue de l'appli (« Bureau » restait en anglais)
        if name == "Bureau" && util::english(&self.settings.language) { "Desktop".into() } else { name }
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
        self.prepare_export_inner(start, end, false)
    }

    /// Comme `prepare_export`, mais en mode RAM les segments ne sont PAS écrits ici (sous le verrou du
    /// moteur) : `export()` les écrit ensuite, verrou relâché (2026-11-24 : un long clip en RAM pouvait
    /// bloquer le moteur plus de 8 s et déclencher un faux redémarrage de la capture).
    pub fn prepare_export_deferred(&self, start: f64, end: f64) -> Result<PreparedExport> {
        self.prepare_export_inner(start, end, true)
    }

    fn prepare_export_inner(&self, start: f64, end: f64, defer_ram: bool) -> Result<PreparedExport> {
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
        // Une partie de la période vient d'une capture aux réglages différents (résolution, cadence,
        // pistes) : impossible de la recoller sans réencoder. Le clip le signale (audit 0.4.12).
        let truncated = self.segments.iter().any(|s| !allowed.contains(&s.session) && s.end > start && s.start < end);
        if truncated {
            log::warn!("clip partiel : une partie de la période a été enregistrée avec d'autres réglages");
        }
        let work = buffer_root().join(format!("export-{}", library::new_id()));
        fs::create_dir_all(&work)?;
        // Liens physiques : le nettoyage du tampon ne peut pas effacer les fichiers pendant la copie
        let mut counter = 0usize;
        let mut pending: Vec<Pending> = Vec::new();
        let mut freeze = |list: &[&Segment], ext: &str, prefix: &str| -> Result<(Vec<PathBuf>, Vec<f64>)> {
            let mut files = Vec::new();
            for seg in list {
                let dst = work.join(format!("{prefix}{counter:06}.{ext}"));
                counter += 1;
                if defer_ram && seg.ram {
                    // On garde une référence aux données : elles restent valables même si le tampon les retire
                    let data = crate::ram::get(&seg.ram_name()).context("segment sorti de la mémoire")?;
                    pending.push(Pending::Ram(data, dst.clone()));
                } else if defer_ram && !seg.ram && fs::hard_link(&seg.path, &dst).is_err() {
                    // Pas de lien physique possible (disque FAT/exFAT…) : la copie se fait hors du verrou du
                    // moteur (audit 0.4.12). Le fichier est ouvert tout de suite : il reste lisible même si le
                    // tampon le supprime entre-temps.
                    let file = fs::File::open(&seg.path).context("segment du tampon introuvable")?;
                    pending.push(Pending::File(file, dst.clone()));
                } else if !defer_ram {
                    seg.freeze(&dst)?;
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
            pending,
            truncated,
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
    /// Segments à écrire dans `work` avant l'assemblage, hors du verrou (`prepare_export_deferred`)
    pending: Vec<Pending>,
    /// Le début de la période manque (capture aux réglages différents)
    pub truncated: bool,
}

/// Copie de segment différée : données en RAM, ou fichier du tampon déjà ouvert.
enum Pending {
    Ram(Arc<Vec<u8>>, PathBuf),
    File(fs::File, PathBuf),
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

    /// Morceaux recollés (début, fin) en heure absolue, comme `clip_time` les compte.
    pub fn spans(&self, end: f64) -> Vec<(f64, f64)> {
        let n = self.parts.len();
        self.parts
            .iter()
            .enumerate()
            .map(|(i, p)| (p.first_start, if i + 1 == n { end.min(p.last_end) } else { p.last_end }))
            .collect()
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
fn read_list(dir: &Path, list: &str, offset: &mut u64, session: u64, segments: &mut VecDeque<Segment>, ram: bool, ram_wait: &mut u32) {
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
    let base = *offset;
    *offset += complete as u64;
    let mut pos = 0usize;
    for line in text[..complete].split_inclusive('\n') {
        let line_start = pos;
        pos += line.len();
        let parts: Vec<&str> = line.trim().rsplitn(3, ',').collect();
        if parts.len() != 3 {
            continue;
        }
        let (Ok(end), Ok(mut start)) = (parts[0].parse::<f64>(), parts[1].parse::<f64>()) else { continue };
        // Le tout premier segment d'une session annonce parfois un début à 0 : on le reconstitue
        if start < 1.0e9 {
            start = segments.back().filter(|s| s.session == session).map(|s| s.end).unwrap_or(end - 1.0);
        }
        let name = parts[2].trim_matches('"');
        // Le nom peut être un chemin complet de tuyau : on ne garde que le nom du segment
        let name = name.rsplit(['\\', '/']).next().unwrap_or(name);
        let file = dir.join(name);
        let bytes = if ram {
            match crate::ram::get(name) {
                Some(d) => d.len() as u64,
                // Annoncé avant d'être entièrement reçu : on réessaie au prochain passage (2 s au plus)
                None if *ram_wait < 8 => {
                    *ram_wait += 1;
                    *offset = base + line_start as u64;
                    return;
                }
                None => 0,
            }
        } else {
            fs::metadata(&file).map(|m| m.len()).unwrap_or(0)
        };
        *ram_wait = 0;
        if bytes == 0 || end <= start {
            continue;
        }
        segments.push_back(Segment { path: file, start, end, bytes, session, ram });
    }
}

/// Assemble le clip (sans réencodage), crée la miniature et renvoie la fiche du clip.
pub fn export(mut prepared: PreparedExport, req: ExportRequest) -> Result<Clip> {
    let _urgent = util::Urgent::begin(); // clip demandé : priorité normale-basse même en jeu
    // Segments RAM gardés de côté par `prepare_export_deferred` : écrits maintenant, hors verrou
    for p in prepared.pending.drain(..) {
        let r = match p {
            Pending::Ram(data, dst) => fs::write(&dst, &*data),
            Pending::File(mut src, dst) => fs::File::create(&dst).and_then(|mut f| std::io::copy(&mut src, &mut f)).map(|_| ()),
        };
        if let Err(e) = r {
            let _ = fs::remove_dir_all(&prepared.work);
            return Err(e).context("copie du tampon");
        }
    }
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
            let output = cmd.stdin(Stdio::null()).output_timeout(std::time::Duration::from_secs(1200))?;
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
            let output = cmd.stdin(Stdio::null()).output_timeout(std::time::Duration::from_secs(1200))?;
            ffmpeg::checked(output, "Recollage du clip")?;
        }
        fs::rename(&partial, &out)?;
        let duration = total;

        let id = library::new_id();
        let thumb = library::thumbs_dir().join(format!("{id}.jpg"));
        // Contrôle du fichier (durée réelle) et miniature EN MÊME TEMPS (audit 0.4.12 : ils se suivaient
        // et retardaient l'annonce du clip). La miniature garde sa place dans la notification.
        let thumb_at = req
            .markers
            .first()
            .map(|(t, _, _)| prepared.clip_time(*t, req.end).clamp(0.0, (duration - 0.1).max(0.0)))
            .unwrap_or(duration * 0.6);
        let real = std::thread::scope(|sc| {
            let probe = sc.spawn(|| ffmpeg::duration(&out));
            let _ = ffmpeg::thumbnail(&out, thumb_at, &thumb);
            probe.join().ok().flatten()
        })
        .unwrap_or(duration);
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
    let _urgent = util::Urgent::begin();
    let seg = {
        let r = recorder.lock();
        r.segments.back().cloned().context("Rien n'est encore enregistré.")?
    };
    let tools = ffmpeg::tools()?;
    let tmp = buffer_root().join(format!("shot-{}.mkv", library::new_id()));
    seg.freeze(&tmp)?;
    fs::create_dir_all(folder)?;
    let stamp = chrono::Local::now().format("%Y-%m-%d %Hh%M-%S");
    let out = folder.join(format!("{} {stamp}.png", util::safe_name(app)));
    let output = util::command(&tools.ffmpeg)
        .args(["-hide_banner", "-loglevel", "error", "-y", "-sseof", "-0.05", "-i"])
        .arg(&tmp)
        .args(["-frames:v", "1", "-update", "1"])
        .arg(&out)
        .stdin(Stdio::null())
        .output_timeout(std::time::Duration::from_secs(60));
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

    /// Clip pris sur le bureau : « Desktop » en anglais, « Bureau » en français ; un jeu garde son nom.
    #[test]
    fn nom_du_bureau_selon_la_langue() {
        let mut r = Recorder::new(Settings { language: "en".into(), ..Settings::default() });
        r.apps.push_back((100.0, "explorer.exe".into()));
        assert_eq!(r.main_app(99.0, 101.0), "Desktop");
        assert_eq!(r.main_app(500.0, 510.0), "Desktop", "aucune appli vue");
        r.settings.language = "fr".into();
        assert_eq!(r.main_app(99.0, 101.0), "Bureau");
        r.apps.push_back((100.5, "cs2.exe".into()));
        r.apps.push_back((100.8, "cs2.exe".into()));
        assert_eq!(r.main_app(99.0, 101.0), "Counter-Strike 2");
    }

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
        let p = PreparedExport { work: PathBuf::new(), parts: vec![part(100.0, 105.0), part(109.0, 120.0)], first_start: 100.0, last_end: 120.0, tracks: vec![], pending: vec![], truncated: false };
        assert!((p.clip_time(102.0, 118.0) - 2.0).abs() < 1e-9);
        assert!((p.clip_time(107.0, 118.0) - 5.0).abs() < 1e-9, "un repère dans le trou va au début du morceau suivant");
        assert!((p.clip_time(110.0, 118.0) - 6.0).abs() < 1e-9);
        assert!((p.clip_time(119.0, 118.0) - 14.0).abs() < 1e-9, "borné à la fin du clip");
        assert!((p.clip_time(90.0, 118.0) - 0.0).abs() < 1e-9);
    }
}
