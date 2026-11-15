//! Mémoire tampon en RAM (2026-11-15).
//!
//! Par défaut, FFmpeg écrit un segment d'une seconde par fichier dans le dossier tampon (sur le SSD).
//! En mode RAM, il écrit chaque segment dans un « tuyau nommé » Windows (`\\.\pipe\…`) : Rewind lit
//! le segment et le garde en mémoire, rien n'est écrit sur le disque. La liste des segments (petit
//! fichier CSV) et les journaux restent sur le disque.
//!
//! FFmpeg ouvre le segment n+1 juste après avoir fermé le n : chaque tuyau est donc créé à l'avance
//! (AHEAD segments d'avance). Testé le 2026-11-15 : segments matroska lisibles, liste correcte
//! (nom de base du segment), assemblage par « concat » identique.
//! Un clip réécrit les segments utiles dans son dossier de travail le temps de l'assemblage.

use anyhow::{Context, Result};
use parking_lot::Mutex;
use std::{
    collections::{HashMap, HashSet},
    sync::{
        Arc, LazyLock,
        atomic::{AtomicBool, Ordering},
    },
    thread,
};
use windows_sys::Win32::{
    Foundation::{CloseHandle, ERROR_PIPE_CONNECTED, GENERIC_WRITE, GetLastError, HANDLE, INVALID_HANDLE_VALUE},
    Storage::FileSystem::{CreateFileW, OPEN_EXISTING, PIPE_ACCESS_INBOUND, ReadFile},
    System::Pipes::{ConnectNamedPipe, CreateNamedPipeW, PIPE_READMODE_BYTE, PIPE_REJECT_REMOTE_CLIENTS, PIPE_TYPE_BYTE, PIPE_WAIT},
};

const AHEAD: u64 = 3;

/// Segments en mémoire, par nom de fichier (en minuscules), ex. « rw-1731…-2-v00000012.mkv ».
static STORE: LazyLock<Mutex<HashMap<String, Arc<Vec<u8>>>>> = LazyLock::new(|| Mutex::new(HashMap::new()));

pub fn get(name: &str) -> Option<Arc<Vec<u8>>> {
    STORE.lock().get(&name.to_lowercase()).cloned()
}

pub fn remove(name: &str) {
    STORE.lock().remove(&name.to_lowercase());
}

/// Retire tout ce qui n'est plus utilisé (segments sortis du tampon, fin de session coupée).
pub fn retain(keep: impl Fn(&str) -> bool) {
    STORE.lock().retain(|k, _| keep(k));
}

/// Mémoire du PC : (totale, disponible) en octets.
pub fn memory() -> Option<(u64, u64)> {
    use windows_sys::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};
    // SAFETY : structure initialisée avec sa taille
    unsafe {
        let mut m: MEMORYSTATUSEX = std::mem::zeroed();
        m.dwLength = std::mem::size_of::<MEMORYSTATUSEX>() as u32;
        (GlobalMemoryStatusEx(&mut m) != 0).then_some((m.ullTotalPhys, m.ullAvailPhys))
    }
}

struct Shared {
    /// Nom de base, ex. « rw-1731…-2-v » ; extension, ex. « mkv »
    prefix: String,
    ext: String,
    stop: AtomicBool,
    /// Tuyaux créés mais pas encore ouverts par FFmpeg
    pending: Mutex<HashSet<u64>>,
}

impl Shared {
    fn name(&self, i: u64) -> String {
        format!("{}{i:08}.{}", self.prefix, self.ext)
    }
    fn path(&self, i: u64) -> String {
        format!(r"\\.\pipe\{}", self.name(i))
    }
}

/// Réception des segments d'un FFmpeg (image ou son).
pub struct Sink {
    shared: Arc<Shared>,
}

struct Handle(HANDLE);
unsafe impl Send for Handle {}
impl Drop for Handle {
    fn drop(&mut self) {
        // SAFETY : poignée valide, fermée une seule fois
        unsafe { CloseHandle(self.0) };
    }
}

impl Sink {
    pub fn start(prefix: &str, ext: &str) -> Result<Self> {
        let shared = Arc::new(Shared { prefix: prefix.to_string(), ext: ext.to_string(), stop: AtomicBool::new(false), pending: Mutex::new(HashSet::new()) });
        for i in 0..AHEAD {
            serve(&shared, i)?;
        }
        Ok(Self { shared })
    }

    /// Modèle de nom à donner au muxer « segment » de FFmpeg.
    pub fn pattern(&self) -> String {
        format!(r"\\.\pipe\{}%08d.{}", self.shared.prefix, self.shared.ext)
    }
}

impl Drop for Sink {
    fn drop(&mut self) {
        self.shared.stop.store(true, Ordering::SeqCst);
        // Débloque les fils qui attendent encore FFmpeg : on s'y connecte nous-mêmes, puis on ferme
        let pending: Vec<u64> = self.shared.pending.lock().iter().copied().collect();
        for i in pending {
            let w = crate::util::wide(&self.shared.path(i));
            // SAFETY : chemin terminé par 0 ; poignée fermée tout de suite
            unsafe {
                let h = CreateFileW(w.as_ptr(), GENERIC_WRITE, 0, std::ptr::null(), OPEN_EXISTING, 0, std::ptr::null_mut());
                if h != INVALID_HANDLE_VALUE {
                    CloseHandle(h);
                }
            }
        }
    }
}

/// Crée le tuyau du segment `i` et un fil qui attend FFmpeg, lit tout le segment et le range.
fn serve(shared: &Arc<Shared>, i: u64) -> Result<()> {
    let w = crate::util::wide(&shared.path(i));
    // SAFETY : chemin terminé par 0 ; une seule instance, lecture seule, clients locaux seulement
    let h = unsafe {
        CreateNamedPipeW(w.as_ptr(), PIPE_ACCESS_INBOUND, PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT | PIPE_REJECT_REMOTE_CLIENTS, 1, 0, 1 << 20, 0, std::ptr::null())
    };
    if h == INVALID_HANDLE_VALUE {
        return Err(std::io::Error::last_os_error()).context("Création du tuyau de la mémoire tampon");
    }
    let handle = Handle(h);
    shared.pending.lock().insert(i);
    let shared = Arc::clone(shared);
    thread::Builder::new().name(format!("tampon RAM {}", shared.name(i))).spawn(move || {
        let handle = handle; // la poignée entière (Send), pas seulement son champ
        // SAFETY : poignée valide jusqu'à la fin du fil
        let ok = unsafe { ConnectNamedPipe(handle.0, std::ptr::null_mut()) != 0 || GetLastError() == ERROR_PIPE_CONNECTED };
        shared.pending.lock().remove(&i);
        if !ok || shared.stop.load(Ordering::SeqCst) {
            return;
        }
        // FFmpeg vient d'ouvrir ce segment : on prépare celui d'après-après
        if let Err(e) = serve(&shared, i + AHEAD) {
            log::warn!("tampon RAM : {e:#}");
        }
        let mut data: Vec<u8> = Vec::with_capacity(4 << 20);
        let mut buf = vec![0u8; 1 << 20];
        loop {
            let mut read = 0u32;
            // SAFETY : tampon et taille cohérents
            let ok = unsafe { ReadFile(handle.0, buf.as_mut_ptr(), buf.len() as u32, &mut read, std::ptr::null_mut()) != 0 };
            if read > 0 {
                data.extend_from_slice(&buf[..read as usize]);
            }
            if !ok || read == 0 {
                break; // FFmpeg a fermé le segment
            }
        }
        if !data.is_empty() && !shared.stop.load(Ordering::SeqCst) {
            data.shrink_to_fit();
            STORE.lock().insert(shared.name(i).to_lowercase(), Arc::new(data));
        }
    })?;
    Ok(())
}
