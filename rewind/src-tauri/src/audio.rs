//! Capture audio (WASAPI) envoyée à FFmpeg par des tuyaux nommés.
//!
//! Chaque source écrit un flux continu de PCM 48 kHz stéréo float, cadencé sur l'horloge du PC :
//! si l'application (ex. Discord) n'est pas lancée ou se tait, on envoie du silence. Ainsi FFmpeg
//! reçoit toujours un flux régulier et le son reste synchronisé avec l'image.

use crate::{settings::AudioSource, util};
use anyhow::{Context, Result};
use serde::Serialize;
use std::{
    collections::VecDeque,
    fs::File,
    io::Write,
    os::windows::io::FromRawHandle,
    ptr::{null, null_mut},
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU32, Ordering},
    },
    thread,
    time::{Duration, Instant},
};
use wasapi::{AudioCaptureClient, AudioClient, DeviceEnumerator, Direction, SampleType, StreamMode, WaveFormat};
use windows_sys::Win32::{
    Foundation::{CloseHandle, ERROR_PIPE_CONNECTED, ERROR_PIPE_LISTENING, GetLastError, HANDLE, INVALID_HANDLE_VALUE},
    Storage::FileSystem::PIPE_ACCESS_OUTBOUND,
    System::Pipes::{
        ConnectNamedPipe, CreateNamedPipeW, PIPE_NOWAIT, PIPE_READMODE_BYTE, PIPE_REJECT_REMOTE_CLIENTS,
        PIPE_TYPE_BYTE, PIPE_WAIT, SetNamedPipeHandleState,
    },
};

pub const RATE: u32 = 48_000;
const PACKET: Duration = Duration::from_millis(10);
const BYTES: usize = RATE as usize / 100 * 2 * 4; // 10 ms stéréo f32

#[derive(Clone, Debug, Serialize)]
pub struct Device {
    pub id: String,
    pub name: String,
    pub default: bool,
}

/// Liste des sorties (input = false) ou des micros (input = true).
pub fn devices(input: bool) -> Vec<Device> {
    let run = move || -> Result<Vec<Device>> {
        let _ = wasapi::initialize_mta().ok();
        let enumerator = DeviceEnumerator::new()?;
        let direction = if input { Direction::Capture } else { Direction::Render };
        let default = enumerator.get_default_device(direction).and_then(|d| d.get_id()).unwrap_or_default();
        let collection = enumerator.get_device_collection(direction)?;
        let mut list = Vec::new();
        for i in 0..collection.get_nbr_devices()? {
            if let Ok(device) = collection.get_device_at_index(i)
                && let (Ok(id), Ok(name)) = (device.get_id(), device.get_friendlyname())
            {
                list.push(Device { default: id == default, id, name });
            }
        }
        Ok(list)
    };
    // COM doit être initialisé dans un fil dédié
    thread::spawn(run).join().ok().and_then(Result::ok).unwrap_or_default()
}

/// Son du micro choisi dans Rewind (vide = micro par défaut de Windows) en PCM mono 16 bits à `rate` Hz,
/// par paquets d'environ 20 ms (commande vocale : 16 kHz ; voix off du Studio : 48 kHz). S'arrête quand
/// `sink` renvoie false ou que `stop` passe à vrai. Micro ouvert en mode partagé : l'enregistrement continue à côté.
pub fn mic_stream(device: &str, rate: u32, stop: &AtomicBool, mut sink: impl FnMut(&[u8]) -> bool) -> Result<()> {
    let _ = wasapi::initialize_mta().ok();
    let enumerator = DeviceEnumerator::new()?;
    let dev = if device.is_empty() { enumerator.get_default_device(Direction::Capture)? } else { enumerator.get_device(device)? };
    let mut client = dev.get_iaudioclient()?;
    // Windows convertit lui-même vers mono 16 bits à la bonne fréquence (autoconvert)
    let format = WaveFormat::new(16, 16, SampleType::Int, rate as usize, 1, None);
    let mode = StreamMode::PollingShared { autoconvert: true, buffer_duration_hns: 2_000_000 };
    client.initialize_client(&format, Direction::Capture, mode, None)?;
    let packets = client.get_audiocaptureclient()?;
    client.start_stream()?;
    let mut queue: VecDeque<u8> = VecDeque::with_capacity(16_000);
    let mut out = Vec::with_capacity(1280);
    let result = (|| -> Result<()> {
        while !stop.load(Ordering::Relaxed) {
            thread::sleep(Duration::from_millis(20));
            while packets.get_next_packet_size()?.unwrap_or_default() > 0 {
                packets.read_from_device_to_deque(&mut queue)?;
            }
            if queue.len() >= 640 {
                out.clear();
                out.extend(queue.drain(..queue.len() / 2 * 2));
                if !sink(&out) {
                    break;
                }
            }
        }
        Ok(())
    })();
    let _ = client.stop_stream();
    result
}

pub struct Pipe {
    pub name: String,
    pub label: String,
    /// "system", "mic" ou "app"
    pub kind: String,
    pub level: Arc<AtomicU32>,
    pub active: Arc<AtomicBool>,
}

pub struct Inputs {
    pub pipes: Vec<Pipe>,
    stop: Arc<AtomicBool>,
}

impl Inputs {
    pub fn start(sources: &[AudioSource], muted: Arc<AtomicBool>) -> Result<Self> {
        let mut inputs = Self { pipes: Vec::new(), stop: Arc::new(AtomicBool::new(false)) };
        let stamp = (util::now() * 1000.0) as u64;
        // « Son du PC » exclut ce qui a déjà sa propre piste (Discord, le jeu…),
        // sinon ce serait entendu deux fois dans le mixage.
        let split = Split {
            apps: sources.iter().filter(|s| s.enabled && s.kind == "app").map(|s| s.app.clone()).collect(),
            game_track: sources.iter().any(|s| s.enabled && s.kind == "game"),
            game_live: Arc::new(AtomicU32::new(0)),
        };
        for (index, source) in sources.iter().filter(|s| s.enabled).take(8).enumerate() {
            let name = format!(r"\\.\pipe\rewind-audio-{stamp}-{index}");
            let path = util::wide(&name);
            let handle = unsafe {
                CreateNamedPipeW(
                    path.as_ptr(),
                    PIPE_ACCESS_OUTBOUND,
                    PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_NOWAIT | PIPE_REJECT_REMOTE_CLIENTS,
                    1,
                    (BYTES * 8) as u32,
                    0,
                    0,
                    null(),
                )
            };
            if handle == INVALID_HANDLE_VALUE {
                return Err(std::io::Error::last_os_error()).context("Création de la liaison audio");
            }
            let pipe = PipeHandle(handle as usize);
            let stop = Arc::clone(&inputs.stop);
            let muted = Arc::clone(&muted);
            let level = Arc::new(AtomicU32::new(0));
            let active = Arc::new(AtomicBool::new(false));
            let (worker_level, worker_active) = (Arc::clone(&level), Arc::clone(&active));
            let src = source.clone();
            let split = split.clone();
            thread::Builder::new().name(format!("audio {}", source.label)).spawn(move || {
                if let Err(e) = pipe.run(&src, &split, &stop, &muted, &worker_level, &worker_active) {
                    log::warn!("audio {} : {e:#}", src.label);
                }
            })?;
            inputs.pipes.push(Pipe { name, label: source.label.clone(), kind: source.kind.clone(), level, active });
        }
        Ok(inputs)
    }
}

impl Drop for Inputs {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

/// Partage du son du PC entre les pistes (2026-11-24 : piste « Jeu » séparée de la musique, du navigateur…).
#[derive(Clone)]
struct Split {
    /// Applications qui ont leur propre piste (ex. Discord.exe)
    apps: Vec<String>,
    /// Une piste « Jeu » est active
    game_track: bool,
    /// Jeu dont la piste « Jeu » reçoit vraiment le son (0 = aucun) : « Son du PC » ne l'enlève
    /// qu'à cette condition, pour ne jamais perdre le son du jeu si sa capture échoue.
    game_live: Arc<AtomicU32>,
}

impl Split {
    /// Ancien fonctionnement de « Son du PC » : toute la sortie, sauf la première application à piste propre.
    fn first_app(&self) -> Option<&str> {
        self.apps.first().map(String::as_str)
    }
}

struct PipeHandle(usize);

impl Drop for PipeHandle {
    fn drop(&mut self) {
        if self.0 != 0 {
            unsafe { CloseHandle(self.0 as HANDLE) };
        }
    }
}

impl PipeHandle {
    fn run(
        mut self,
        source: &AudioSource,
        split: &Split,
        stop: &AtomicBool,
        muted: &AtomicBool,
        level: &AtomicU32,
        active: &AtomicBool,
    ) -> Result<()> {
        let handle = self.0 as HANDLE;
        // Attendre que FFmpeg ouvre le tuyau
        loop {
            if stop.load(Ordering::Relaxed) {
                return Ok(());
            }
            let ok = unsafe { ConnectNamedPipe(handle, null_mut()) };
            let code = unsafe { GetLastError() };
            if ok != 0 || code == ERROR_PIPE_CONNECTED {
                break;
            }
            if code != ERROR_PIPE_LISTENING {
                return Err(std::io::Error::from_raw_os_error(code as i32).into());
            }
            thread::sleep(Duration::from_millis(5));
        }
        let mode = PIPE_WAIT;
        unsafe { SetNamedPipeHandleState(handle, &mode, null(), null()) };
        let mut output = unsafe { File::from_raw_handle(handle as _) };
        self.0 = 0;
        let _ = wasapi::initialize_mta().ok();
        let mut stream: Option<Stream> = None;
        let mut retry = Instant::now();
        let mut recheck = retry + Duration::from_secs(2);
        let mut deadline = Instant::now();
        let mut packet = vec![0u8; BYTES];
        let is_mic = source.kind == "mic";
        let is_game = source.kind == "game";
        // Piste « Jeu » : le son du jeu a-t-il vraiment été reçu depuis l'ouverture ?
        let mut heard = false;
        let gain = source.volume as f32 / 100.0;
        while !stop.load(Ordering::Relaxed) {
            let now = Instant::now();
            if now >= recheck {
                recheck = now + Duration::from_secs(2);
                if stream.as_mut().is_some_and(|c| !c.still_valid(source, split)) {
                    stream = None;
                }
            }
            if stream.is_none() && now >= retry {
                retry = now + Duration::from_secs(2);
                heard = false;
                stream = Stream::open(source, split).unwrap_or_else(|e| {
                    log::debug!("source {} indisponible : {e:#}", source.label);
                    None
                });
            }
            active.store(stream.is_some(), Ordering::Relaxed);
            packet.fill(0);
            if let Some(c) = stream.as_mut()
                && c.read(&mut packet).is_err()
            {
                stream = None;
                packet.fill(0);
            }
            if is_game {
                if !heard && stream.is_some() && packet.iter().any(|b| *b != 0) {
                    heard = true;
                }
                let pid = match &stream {
                    Some(Stream::One(c)) if heard => c.pid,
                    _ => 0,
                };
                split.game_live.store(pid, Ordering::Relaxed);
            }
            let g = if is_mic && muted.load(Ordering::Relaxed) { 0.0 } else { gain };
            let mut peak = 0.0f32;
            for chunk in packet.chunks_exact_mut(4) {
                let v = f32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]) * g;
                let v = if v.is_finite() { v.clamp(-1.0, 1.0) } else { 0.0 };
                peak = peak.max(v.abs());
                chunk.copy_from_slice(&v.to_le_bytes());
            }
            let old = f32::from_bits(level.load(Ordering::Relaxed));
            level.store((peak.max(old * 0.85)).to_bits(), Ordering::Relaxed);
            if let Err(e) = output.write_all(&packet) {
                // 109/232/233 : FFmpeg a fermé le tuyau (arrêt normal)
                if matches!(e.raw_os_error(), Some(109 | 232 | 233)) {
                    return Ok(());
                }
                return Err(e.into());
            }
            deadline += PACKET;
            if deadline + Duration::from_millis(200) < now {
                deadline = now;
            }
            thread::sleep(deadline.saturating_duration_since(Instant::now()));
        }
        if is_game {
            split.game_live.store(0, Ordering::Relaxed);
        }
        Ok(())
    }
}

/// Ce qu'une piste lit : une seule capture, ou (« Son du PC » quand le jeu a sa piste) la somme des
/// applications qui font du son, sauf le jeu et les applications à piste propre. Windows ne sait
/// exclure qu'UN processus par capture : impossible d'enlever à la fois le jeu et Discord autrement.
enum Stream {
    One(Capture),
    Others(Others),
}

impl Stream {
    fn open(source: &AudioSource, split: &Split) -> Result<Option<Self>> {
        match source.kind.as_str() {
            "game" => {
                let procs = util::processes();
                let Some(pid) = util::game_pid(&procs) else { return Ok(None) };
                let c = Capture::loopback(pid, format!("g{pid}"))?;
                log::info!("piste Jeu : {}#{pid}", procs.iter().find(|p| p.pid == pid).map_or("?", |p| p.exe.as_str()));
                Ok(Some(Self::One(c)))
            }
            "system" if source.device.is_empty() && split.game_track => {
                let game = split.game_live.load(Ordering::Relaxed);
                if game != 0 {
                    let mut o = Others { game, parts: Vec::new(), last: Vec::new(), scratch: vec![0u8; BYTES] };
                    o.refresh(&split.apps);
                    return Ok(Some(Self::Others(o)));
                }
                Ok(Capture::open(source, split.first_app())?.map(Self::One))
            }
            _ => Ok(Capture::open(source, if source.kind == "system" && source.device.is_empty() { split.first_app() } else { None })?.map(Self::One)),
        }
    }

    fn still_valid(&mut self, source: &AudioSource, split: &Split) -> bool {
        let game = split.game_live.load(Ordering::Relaxed);
        match self {
            Self::One(c) if source.kind == "game" => {
                let procs = util::processes();
                util::game_pid(&procs) == Some(c.pid)
            }
            // Le jeu vient d'avoir sa piste : on passe à « tout sauf le jeu »
            Self::One(_) if source.kind == "system" && source.device.is_empty() && split.game_track && game != 0 => false,
            Self::One(c) => c.still_valid(source, if source.kind == "system" && source.device.is_empty() { split.first_app() } else { None }),
            Self::Others(o) => {
                if game == 0 || game != o.game {
                    return false;
                }
                o.refresh(&split.apps);
                true
            }
        }
    }

    fn read(&mut self, out: &mut [u8]) -> Result<()> {
        match self {
            Self::One(c) => c.read(out),
            Self::Others(o) => o.read(out),
        }
    }
}

/// Somme des applications qui font du son, sauf le jeu et les applications à piste propre.
struct Others {
    game: u32,
    parts: Vec<Capture>,
    /// Dernière liste d'applications captées (journal quand elle change)
    last: Vec<u32>,
    scratch: Vec<u8>,
}

impl Others {
    /// Met à jour la liste des applications captées (toutes les 2 s) sans couper les autres.
    fn refresh(&mut self, apps: &[String]) {
        let procs = util::processes();
        let mut excluded = vec![self.game];
        for a in apps {
            if let Some(pid) = root_of(&procs, a) {
                excluded.push(pid);
            }
        }
        let sessions = session_pids();
        let want = pick_others(&procs, &sessions, &excluded);
        if want != self.last {
            let name = |pid: &u32| procs.iter().find(|p| p.pid == *pid).map_or_else(|| pid.to_string(), |p| format!("{}#{pid}", p.exe));
            log::info!(
                "son du PC sans le jeu ({}) : {} (sessions : {})",
                name(&self.game),
                want.iter().map(name).collect::<Vec<_>>().join(", "),
                sessions.iter().map(name).collect::<Vec<_>>().join(", ")
            );
            self.last = want.clone();
        }
        self.parts.retain(|c| want.contains(&c.pid));
        for pid in want {
            if self.parts.len() >= 24 {
                break;
            }
            if self.parts.iter().any(|c| c.pid == pid) {
                continue;
            }
            match Capture::loopback(pid, pid.to_string()) {
                Ok(c) => self.parts.push(c),
                Err(e) => log::debug!("son de l'application {pid} indisponible : {e:#}"),
            }
        }
    }

    fn read(&mut self, out: &mut [u8]) -> Result<()> {
        let mut i = 0;
        while i < self.parts.len() {
            self.scratch.fill(0);
            if self.parts[i].read(&mut self.scratch).is_err() {
                self.parts.swap_remove(i);
                continue;
            }
            for (o, s) in out.chunks_exact_mut(4).zip(self.scratch.chunks_exact(4)) {
                let v = f32::from_le_bytes([o[0], o[1], o[2], o[3]]) + f32::from_le_bytes([s[0], s[1], s[2], s[3]]);
                o.copy_from_slice(&v.to_le_bytes());
            }
            i += 1;
        }
        Ok(())
    }
}

/// Processus qui ont une session son sur la sortie par défaut (Chrome, Spotify, Discord, le jeu…).
fn session_pids() -> Vec<u32> {
    let run = || -> Result<Vec<u32>> {
        let device = DeviceEnumerator::new()?.get_default_device(Direction::Render)?;
        let list = device.get_iaudiosessionmanager()?.get_audiosessionenumerator()?;
        let mut pids = Vec::new();
        for i in 0..list.get_count()? {
            if let Ok(pid) = list.get_session(i).and_then(|s| s.get_process_id())
                && pid != 0
                && !pids.contains(&pid)
            {
                pids.push(pid);
            }
        }
        Ok(pids)
    };
    run().unwrap_or_default()
}

/// Premier processus de l'arbre d'une application (comme `util::root_process`, sur une liste déjà lue).
fn root_of(procs: &[util::ProcessInfo], exe: &str) -> Option<u32> {
    let exe = exe.to_ascii_lowercase();
    let exe = if exe.ends_with(".exe") { exe } else { format!("{exe}.exe") };
    let matching: Vec<&util::ProcessInfo> = procs.iter().filter(|p| p.exe.to_ascii_lowercase() == exe).collect();
    matching.iter().find(|p| !matching.iter().any(|q| q.pid == p.parent)).or(matching.first()).map(|p| p.pid)
}

/// Parents d'un processus (au plus 64, sans boucle).
fn ancestors(procs: &[util::ProcessInfo], pid: u32) -> Vec<u32> {
    let mut out = Vec::new();
    let mut cur = pid;
    while out.len() < 64 {
        let Some(p) = procs.iter().find(|p| p.pid == cur) else { break };
        if p.parent == 0 || p.parent == cur || out.contains(&p.parent) || p.parent == pid {
            break;
        }
        out.push(p.parent);
        cur = p.parent;
    }
    out
}

/// Applications à capter pour « tout sauf le jeu et les pistes propres » : chaque processus qui a une
/// session son, sauf ceux dans l'arbre d'un exclu, et sauf ceux dont l'arbre CONTIENT un exclu
/// (Steam a une session et lance le jeu : capter son arbre doublerait le jeu). Un processus dont un
/// parent est déjà capté est couvert par l'arbre de ce parent.
fn pick_others(procs: &[util::ProcessInfo], sessions: &[u32], excluded: &[u32]) -> Vec<u32> {
    let candidates: Vec<u32> = sessions
        .iter()
        .copied()
        .filter(|s| procs.iter().any(|p| p.pid == *s))
        .filter(|s| !excluded.contains(s) && !ancestors(procs, *s).iter().any(|a| excluded.contains(a)))
        .filter(|s| !excluded.iter().any(|e| ancestors(procs, *e).contains(s)))
        .collect();
    candidates.iter().copied().filter(|c| !ancestors(procs, *c).iter().any(|a| candidates.contains(a))).collect()
}

struct Capture {
    client: AudioClient,
    packets: AudioCaptureClient,
    queue: VecDeque<u8>,
    key: String,
    /// Processus capté (capture d'application), 0 pour un périphérique
    pid: u32,
}

impl Capture {
    /// Son d'un processus et de ses enfants seulement.
    fn loopback(pid: u32, key: String) -> Result<Self> {
        let client = AudioClient::new_application_loopback_client(pid, true)?;
        Self::start(client, key, pid)
    }

    fn start(mut client: AudioClient, key: String, pid: u32) -> Result<Self> {
        let format = WaveFormat::new(32, 32, SampleType::Float, RATE as usize, 2, None);
        let mode = StreamMode::PollingShared { autoconvert: true, buffer_duration_hns: 1_000_000 };
        client.initialize_client(&format, Direction::Capture, mode, None)?;
        let packets = client.get_audiocaptureclient()?;
        client.start_stream()?;
        Ok(Self { client, packets, queue: VecDeque::with_capacity(BYTES * 8), key, pid })
    }

    fn open(source: &AudioSource, exclude: Option<&str>) -> Result<Option<Self>> {
        let excluded = exclude.and_then(util::root_process);
        let (client, key, pid) = match source.kind.as_str() {
            "system" if excluded.is_some() => {
                let pid = excluded.unwrap_or_default();
                (AudioClient::new_application_loopback_client(pid, false)?, format!("x{pid}"), 0)
            }
            "app" => {
                let Some(pid) = util::root_process(&source.app) else { return Ok(None) };
                (AudioClient::new_application_loopback_client(pid, true)?, pid.to_string(), pid)
            }
            kind => {
                let direction = if kind == "mic" { Direction::Capture } else { Direction::Render };
                let enumerator = DeviceEnumerator::new()?;
                let device = if source.device.is_empty() {
                    enumerator.get_default_device(direction)?
                } else {
                    enumerator.get_device(&source.device)?
                };
                (device.get_iaudioclient()?, device.get_id()?, 0)
            }
        };
        Ok(Some(Self::start(client, key, pid)?))
    }

    fn still_valid(&self, source: &AudioSource, exclude: Option<&str>) -> bool {
        if source.kind == "system" && exclude.is_some() {
            let now = exclude.and_then(util::root_process).map(|p| format!("x{p}"));
            let was_excluding = self.key.starts_with('x');
            match now {
                Some(k) => return k == self.key,
                None if was_excluding => return false,
                None => {}
            }
        }
        match source.kind.as_str() {
            "app" => util::root_process(&source.app).is_some_and(|pid| pid.to_string() == self.key),
            kind if source.device.is_empty() => {
                let direction = if kind == "mic" { Direction::Capture } else { Direction::Render };
                DeviceEnumerator::new()
                    .and_then(|e| e.get_default_device(direction))
                    .and_then(|d| d.get_id())
                    .is_ok_and(|id| id == self.key)
            }
            _ => true,
        }
    }

    fn read(&mut self, out: &mut [u8]) -> Result<()> {
        while self.packets.get_next_packet_size()?.unwrap_or_default() > 0 {
            self.packets.read_from_device_to_deque(&mut self.queue)?;
        }
        // Si on a trop de retard (horloges différentes), on jette l'excédent
        // File courte (latence 10-30 ms) : au-delà, on jette le plus ancien pour rester calé sur l'image
        if self.queue.len() > BYTES * 3 {
            let excess = (self.queue.len() - BYTES) / 8 * 8;
            self.queue.drain(..excess);
        }
        let n = self.queue.len().min(out.len()) / 8 * 8;
        for (b, s) in out.iter_mut().zip(self.queue.drain(..n)) {
            *b = s;
        }
        Ok(())
    }
}

impl Drop for Capture {
    fn drop(&mut self) {
        let _ = self.client.stop_stream();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(pid: u32, parent: u32, exe: &str) -> util::ProcessInfo {
        util::ProcessInfo { pid, parent, exe: exe.into() }
    }

    /// « Son du PC » quand le jeu a sa piste : jamais le jeu ni Discord, jamais deux fois la même appli.
    #[test]
    fn autres_sons_sans_le_jeu_ni_discord() {
        let procs = vec![
            p(4, 1, "explorer.exe"),
            p(10, 4, "steam.exe"),
            p(11, 10, "steamwebhelper.exe"),
            p(20, 10, "cs2.exe"),
            p(30, 4, "chrome.exe"),
            p(31, 30, "chrome.exe"),
            p(40, 4, "Discord.exe"),
            p(41, 40, "Discord.exe"),
            p(50, 4, "Spotify.exe"),
        ];
        let discord = root_of(&procs, "discord").unwrap();
        assert_eq!(discord, 40);
        // sessions : Steam lui-même, son navigateur, le jeu, Chrome (parent ET enfant), Discord, Spotify
        let got = pick_others(&procs, &[10, 11, 20, 30, 31, 41, 50], &[20, discord]);
        // Steam a lancé le jeu : son arbre est écarté (le jeu serait doublé), mais son navigateur reste
        assert!(!got.contains(&10) && !got.contains(&20), "{got:?}");
        assert!(got.contains(&11), "{got:?}");
        // Chrome une seule fois (l'arbre du parent couvre l'enfant)
        assert!(got.contains(&30) && !got.contains(&31), "{got:?}");
        assert!(!got.contains(&41), "Discord a sa piste : {got:?}");
        assert!(got.contains(&50), "{got:?}");
    }
}
