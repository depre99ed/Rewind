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
        // « Son du PC » exclut l'application qui a déjà sa propre piste (ex. Discord),
        // sinon elle serait entendue deux fois dans le mixage.
        let exclude = sources.iter().find(|s| s.enabled && s.kind == "app").map(|s| s.app.clone());
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
            let excl = if source.kind == "system" && source.device.is_empty() { exclude.clone() } else { None };
            thread::Builder::new().name(format!("audio {}", source.label)).spawn(move || {
                if let Err(e) = pipe.run(&src, excl.as_deref(), &stop, &muted, &worker_level, &worker_active) {
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
        exclude: Option<&str>,
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
        let mut stream: Option<Capture> = None;
        let mut retry = Instant::now();
        let mut recheck = retry + Duration::from_secs(2);
        let mut deadline = Instant::now();
        let mut packet = vec![0u8; BYTES];
        let is_mic = source.kind == "mic";
        let gain = source.volume as f32 / 100.0;
        while !stop.load(Ordering::Relaxed) {
            let now = Instant::now();
            if now >= recheck {
                recheck = now + Duration::from_secs(2);
                if stream.as_ref().is_some_and(|c| !c.still_valid(source, exclude)) {
                    stream = None;
                }
            }
            if stream.is_none() && now >= retry {
                retry = now + Duration::from_secs(2);
                stream = Capture::open(source, exclude).unwrap_or_else(|e| {
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
        Ok(())
    }
}

struct Capture {
    client: AudioClient,
    packets: AudioCaptureClient,
    queue: VecDeque<u8>,
    key: String,
}

impl Capture {
    fn open(source: &AudioSource, exclude: Option<&str>) -> Result<Option<Self>> {
        let format = WaveFormat::new(32, 32, SampleType::Float, RATE as usize, 2, None);
        let excluded = exclude.and_then(util::root_process);
        let (mut client, key) = match source.kind.as_str() {
            "system" if excluded.is_some() => {
                let pid = excluded.unwrap_or_default();
                (AudioClient::new_application_loopback_client(pid, false)?, format!("x{pid}"))
            }
            "app" => {
                let Some(pid) = util::root_process(&source.app) else { return Ok(None) };
                (AudioClient::new_application_loopback_client(pid, true)?, pid.to_string())
            }
            kind => {
                let direction = if kind == "mic" { Direction::Capture } else { Direction::Render };
                let enumerator = DeviceEnumerator::new()?;
                let device = if source.device.is_empty() {
                    enumerator.get_default_device(direction)?
                } else {
                    enumerator.get_device(&source.device)?
                };
                (device.get_iaudioclient()?, device.get_id()?)
            }
        };
        let mode = StreamMode::PollingShared { autoconvert: true, buffer_duration_hns: 1_000_000 };
        client.initialize_client(&format, Direction::Capture, mode, None)?;
        let packets = client.get_audiocaptureclient()?;
        client.start_stream()?;
        Ok(Some(Self { client, packets, queue: VecDeque::with_capacity(BYTES * 8), key }))
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
