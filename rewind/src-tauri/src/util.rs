//! Petits outils Windows : dossiers, processus, écrans, fenêtre active, processus enfants.

use serde::Serialize;
use std::{
    os::windows::process::CommandExt,
    path::{Path, PathBuf},
    process::{Child, Command},
    time::{SystemTime, UNIX_EPOCH},
};
use windows_sys::Win32::{
    Foundation::{CloseHandle, HANDLE, HWND, LPARAM, RECT},
    Graphics::Gdi::{
        DEVMODEW, ENUM_CURRENT_SETTINGS, EnumDisplayMonitors, EnumDisplaySettingsW, GetMonitorInfoW, HDC, HMONITOR,
        MONITOR_DEFAULTTONEAREST, MONITORINFO, MONITORINFOEXW, MonitorFromWindow,
    },
    System::{
        Diagnostics::ToolHelp::{
            CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW,
            TH32CS_SNAPPROCESS,
        },
        JobObjects::{
            AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
            JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectExtendedLimitInformation,
            SetInformationJobObject,
        },
        Threading::{
            GetProcessTimes, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW,
        },
    },
    UI::WindowsAndMessaging::{
        GetForegroundWindow, GetWindowRect, GetWindowThreadProcessId, InternalGetWindowText,
    },
};

pub const CREATE_NO_WINDOW: u32 = 0x0800_0000;
const BELOW_NORMAL_PRIORITY_CLASS: u32 = 0x0000_4000;

/// Secondes depuis 1970 (avec décimales).
pub fn now() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or_default()
}

/// Fin d'un fichier texte (les `max` derniers octets), sans le lire en entier (journaux de FFmpeg).
pub fn read_tail(path: &Path, max: u64) -> String {
    use std::io::{Read, Seek, SeekFrom};
    let Ok(mut f) = std::fs::File::open(path) else { return String::new() };
    let len = f.metadata().map(|m| m.len()).unwrap_or(0);
    if len > max {
        let _ = f.seek(SeekFrom::Start(len - max));
    }
    let mut buf = Vec::new();
    let _ = f.read_to_end(&mut buf);
    String::from_utf8_lossy(&buf).into_owned()
}

/// « Il y a `d` » sans paniquer : juste après le démarrage de Windows, `Instant::now() - 60 s` panique
/// (l'horloge monotone part de zéro au démarrage ; Rewind se lance avec Windows). Dans ce cas : maintenant.
pub fn ago(d: std::time::Duration) -> std::time::Instant {
    let now = std::time::Instant::now();
    now.checked_sub(d).unwrap_or(now)
}

pub fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

/// Valeurs d'une clé du registre (`HKLM\…` ou `HKCU\…`), lues directement, au même format texte que
/// `reg query` (nombres en `0x…` hexadécimal). Remplace le lancement de reg.exe (2026-11-15) : un
/// programme qui lance reg.exe en cachette ressemble à un logiciel malveillant pour les antivirus.
pub fn reg_values(key: &str) -> std::collections::HashMap<String, String> {
    use windows_sys::Win32::System::Registry::*;
    let mut map = std::collections::HashMap::new();
    let (root, sub) = if let Some(s) = key.strip_prefix(r"HKLM\") {
        (HKEY_LOCAL_MACHINE, s)
    } else if let Some(s) = key.strip_prefix(r"HKCU\") {
        (HKEY_CURRENT_USER, s)
    } else {
        return map;
    };
    let sub_w = wide(sub);
    let mut hkey: HKEY = std::ptr::null_mut();
    unsafe {
        if RegOpenKeyExW(root, sub_w.as_ptr(), 0, KEY_READ | KEY_WOW64_64KEY, &mut hkey) != 0 {
            return map;
        }
        for index in 0u32.. {
            let mut name = [0u16; 512];
            let mut name_len = name.len() as u32;
            let mut kind = 0u32;
            let mut data = vec![0u8; 4096];
            let mut data_len = data.len() as u32;
            let rc = RegEnumValueW(hkey, index, name.as_mut_ptr(), &mut name_len, std::ptr::null(), &mut kind, data.as_mut_ptr(), &mut data_len);
            if rc == 234 {
                continue; // ERROR_MORE_DATA : valeur trop longue, ignorée (inutile au rapport)
            }
            if rc != 0 {
                break; // ERROR_NO_MORE_ITEMS
            }
            data.truncate(data_len as usize);
            let name = String::from_utf16_lossy(&name[..name_len as usize]);
            let text = match kind {
                REG_SZ | REG_EXPAND_SZ | REG_MULTI_SZ => {
                    let units: Vec<u16> = data.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
                    let s = String::from_utf16_lossy(&units);
                    s.trim_end_matches('\0').replace('\0', "\\0")
                }
                REG_DWORD if data.len() >= 4 => format!("0x{:x}", u32::from_le_bytes([data[0], data[1], data[2], data[3]])),
                REG_QWORD if data.len() >= 8 => format!("0x{:x}", u64::from_le_bytes(data[..8].try_into().unwrap_or([0; 8]))),
                _ => data.iter().map(|b| format!("{b:02X}")).collect(),
            };
            map.insert(name, text);
        }
        RegCloseKey(hkey);
    }
    map
}

fn from_wide(buf: &[u16]) -> String {
    let end = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    String::from_utf16_lossy(&buf[..end])
}

/// Données de Rewind : `%LOCALAPPDATA%\com.rewind.app` (séparé du dossier d'installation
/// `%LOCALAPPDATA%\Rewind`, pour qu'une mise à jour ou une désinstallation ne les mélange pas).
pub fn data_dir() -> PathBuf {
    // Tests unitaires (cargo test) : le dossier suit REWIND_DATA_DIR à chaque appel (un test qui le règle n'est
    // plus à la merci de l'ordre des tests) et, sans lui, un dossier temporaire : jamais les vraies données.
    #[cfg(test)]
    {
        let dir = std::env::var_os("REWIND_DATA_DIR").map(PathBuf::from).unwrap_or_else(|| std::env::temp_dir().join("rewind-tests"));
        let _ = std::fs::create_dir_all(&dir);
        dir
    }
    #[cfg(not(test))]
    {
        data_dir_installed()
    }
}

/// Dossier des données hors tests (calculé une fois).
#[cfg(not(test))]
fn data_dir_installed() -> PathBuf {
    static DIR: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();
    DIR.get_or_init(|| {
        // Tests : dossier séparé (ne jamais toucher au Rewind installé qui tourne)
        if let Some(dir) = std::env::var_os("REWIND_DATA_DIR") {
            let dir = PathBuf::from(dir);
            let _ = std::fs::create_dir_all(&dir);
            return dir;
        }
        let base = std::env::var_os("LOCALAPPDATA").map(PathBuf::from).unwrap_or_else(std::env::temp_dir);
        let dir = base.join("com.rewind.app");
        let _ = std::fs::create_dir_all(&dir);
        // Reprise des réglages et de la bibliothèque de la première version
        let old = base.join("Rewind");
        for f in ["settings.json", "library.json"] {
            if !dir.join(f).exists() && old.join(f).is_file() {
                let _ = std::fs::copy(old.join(f), dir.join(f));
            }
        }
        dir
    })
    .clone()
}

/// Dossier Vidéos de l'utilisateur.
pub fn videos_dir() -> PathBuf {
    std::env::var_os("USERPROFILE")
        .map(|p| PathBuf::from(p).join("Videos"))
        .unwrap_or_else(|| PathBuf::from("C:\\"))
}

// ---------------------------------------------------------------------------
// Priorités (2026-11-24, audit de performances)
// ---------------------------------------------------------------------------
// - Capture (FFmpeg image et son) : au-dessus de la normale. Décision de l'utilisateur : quand le PC
//   est à fond, l'enregistrement passe avant le jeu (avant : priorité basse, images perdues possibles).
// - Clips et captures d'écran : un peu basse (l'utilisateur attend le résultat, même en jeu).
// - Tout le reste (miniatures, montage, partage, analyse…) : un peu basse, et « inactive » quand un
//   jeu est au premier plan : ces tâches ne prennent que le processeur que le jeu laisse.

const NORMAL_PRIORITY_CLASS: u32 = 0x20;
const ABOVE_NORMAL_PRIORITY_CLASS: u32 = 0x8000;
const IDLE_PRIORITY_CLASS: u32 = 0x40;

static GAME_ACTIVE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Posé par le fil « jeux » (toutes les 500 ms) : un jeu est au premier plan.
pub fn set_game_active(on: bool) {
    GAME_ACTIVE.store(on, std::sync::atomic::Ordering::Relaxed);
}

/// Un jeu est-il au premier plan ? (tâches annexes : attendre ou passer en priorité minimale)
pub fn game_active() -> bool {
    GAME_ACTIVE.load(std::sync::atomic::Ordering::Relaxed)
}

/// Remplace `target` par `replacement` d'un seul coup (ReplaceFileW) et garde l'ancien `target` sous
/// le nom `backup`. En cas d'échec, les fichiers restent comme avant.
pub fn replace_file(target: &Path, replacement: &Path, backup: &Path) -> std::io::Result<()> {
    use windows_sys::Win32::Storage::FileSystem::ReplaceFileW;
    let (t, r, b) = (wide(&target.to_string_lossy()), wide(&replacement.to_string_lossy()), wide(&backup.to_string_lossy()));
    let ok = unsafe { ReplaceFileW(t.as_ptr(), r.as_ptr(), b.as_ptr(), 0, std::ptr::null(), std::ptr::null()) };
    if ok == 0 { Err(std::io::Error::last_os_error()) } else { Ok(()) }
}

/// `Command::output` avec un délai maximal (audit 0.4.12) : un FFmpeg/FFprobe bloqué (fichier
/// abîmé, disque réseau figé) ne doit plus immobiliser pour toujours la file des clips ou le Studio.
/// Au-delà du délai, le processus est arrêté et une erreur « TimedOut » est renvoyée.
pub trait OutputTimeout {
    fn output_timeout(&mut self, max: std::time::Duration) -> std::io::Result<std::process::Output>;
}

impl OutputTimeout for std::process::Command {
    fn output_timeout(&mut self, max: std::time::Duration) -> std::io::Result<std::process::Output> {
        use std::io::Read;
        let mut child = self.stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::piped()).spawn()?;
        let (mut so, mut se) = (child.stdout.take(), child.stderr.take());
        let t1 = std::thread::spawn(move || {
            let mut b = Vec::new();
            if let Some(s) = so.as_mut() {
                let _ = s.read_to_end(&mut b);
            }
            b
        });
        let t2 = std::thread::spawn(move || {
            let mut b = Vec::new();
            if let Some(s) = se.as_mut() {
                let _ = s.read_to_end(&mut b);
            }
            b
        });
        let deadline = std::time::Instant::now() + max;
        let status = loop {
            if let Some(st) = child.try_wait()? {
                break st;
            }
            if std::time::Instant::now() >= deadline {
                let _ = child.kill();
                let _ = child.wait();
                let _ = (t1.join(), t2.join());
                return Err(std::io::Error::new(
                    std::io::ErrorKind::TimedOut,
                    format!("le traitement ne répond plus après {} s : arrêté", max.as_secs()),
                ));
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        };
        Ok(std::process::Output { status, stdout: t1.join().unwrap_or_default(), stderr: t2.join().unwrap_or_default() })
    }
}

/// Dernier jeu vu au premier plan (processus), gardé quand on passe sur le bureau : la piste « Jeu »
/// continue de capter son son tant qu'il tourne (2026-11-24).
static GAME_PID: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

pub fn set_game_pid(pid: u32) {
    GAME_PID.store(pid, std::sync::atomic::Ordering::Relaxed);
}

/// Processus du dernier jeu, s'il tourne encore (`procs` : liste des processus déjà lue).
pub fn game_pid(procs: &[ProcessInfo]) -> Option<u32> {
    let mut pid = GAME_PID.load(std::sync::atomic::Ordering::Relaxed);
    // Essai de la piste « Jeu » sans jeu : REWIND_TEST_GAME_PID désigne le processus qui joue le rôle du jeu
    if pid == 0 {
        pid = std::env::var("REWIND_TEST_GAME_PID").ok().and_then(|v| v.parse().ok()).unwrap_or(0);
    }
    (pid != 0 && procs.iter().any(|p| p.pid == pid)).then_some(pid)
}

thread_local! {
    static URGENT: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
}

/// Tant que ce garde vit, les commandes lancées par ce fil gardent la priorité « un peu basse »
/// même en jeu (clip demandé par l'utilisateur : sa miniature ne doit pas retarder l'annonce).
pub struct Urgent;
impl Urgent {
    pub fn begin() -> Self {
        URGENT.with(|u| u.set(u.get() + 1));
        Urgent
    }
}
impl Drop for Urgent {
    fn drop(&mut self) {
        URGENT.with(|u| u.set(u.get().saturating_sub(1)));
    }
}

/// Commande sans fenêtre console, en priorité un peu basse pour ne pas gêner les jeux
/// (priorité minimale si un jeu est au premier plan, sauf travail urgent : voir `Urgent`).
pub fn command(program: &Path) -> Command {
    let mut cmd = Command::new(program);
    let urgent = URGENT.with(|u| u.get() > 0);
    let class = if game_active() && !urgent { IDLE_PRIORITY_CLASS } else { BELOW_NORMAL_PRIORITY_CLASS };
    cmd.creation_flags(CREATE_NO_WINDOW | class);
    cmd
}

/// FFmpeg de capture (image et son) : l'enregistrement passe avant tout. `cpu_encoder` : encodage
/// par le processeur (x264, secours) → priorité normale seulement, pour ne pas écraser le jeu.
pub fn command_capture(program: &Path, cpu_encoder: bool) -> Command {
    let mut cmd = Command::new(program);
    cmd.creation_flags(CREATE_NO_WINDOW | if cpu_encoder { NORMAL_PRIORITY_CLASS } else { ABOVE_NORMAL_PRIORITY_CLASS });
    cmd
}

/// Nom de fichier sans caractères interdits par Windows.
pub fn safe_name(text: &str) -> String {
    let cleaned: String = text
        .chars()
        .map(|c| match c {
            '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*' => '_',
            c if c.is_control() => '_',
            c => c,
        })
        .collect();
    let trimmed = cleaned.trim().trim_end_matches('.').to_string();
    if trimmed.is_empty() { "Clip".into() } else { trimmed.chars().take(120).collect() }
}

// ---------------------------------------------------------------------------
// Objet « job » : tue FFmpeg automatiquement si Rewind se ferme ou plante.
// ---------------------------------------------------------------------------

pub struct Job(HANDLE);
unsafe impl Send for Job {}
unsafe impl Sync for Job {}

impl Job {
    pub fn new() -> Option<Self> {
        unsafe {
            let job = CreateJobObjectW(std::ptr::null(), std::ptr::null());
            if job.is_null() {
                return None;
            }
            let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = std::mem::zeroed();
            info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            SetInformationJobObject(
                job,
                JobObjectExtendedLimitInformation,
                &info as *const _ as *const _,
                std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            );
            Some(Self(job))
        }
    }

    pub fn adopt(&self, child: &Child) {
        use std::os::windows::io::AsRawHandle;
        unsafe {
            AssignProcessToJobObject(self.0, child.as_raw_handle() as HANDLE);
        }
    }
}

impl Drop for Job {
    fn drop(&mut self) {
        unsafe {
            CloseHandle(self.0);
        }
    }
}

// ---------------------------------------------------------------------------
// Processus
// ---------------------------------------------------------------------------

#[derive(Clone, Debug)]
pub struct ProcessInfo {
    pub pid: u32,
    pub parent: u32,
    pub exe: String,
}

pub fn processes() -> Vec<ProcessInfo> {
    let mut list = Vec::new();
    unsafe {
        let snap = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
        if snap.is_null() || snap as isize == -1 {
            return list;
        }
        let mut entry: PROCESSENTRY32W = std::mem::zeroed();
        entry.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;
        if Process32FirstW(snap, &mut entry) != 0 {
            loop {
                list.push(ProcessInfo {
                    pid: entry.th32ProcessID,
                    parent: entry.th32ParentProcessID,
                    exe: from_wide(&entry.szExeFile),
                });
                if Process32NextW(snap, &mut entry) == 0 {
                    break;
                }
            }
        }
        CloseHandle(snap);
    }
    list
}

/// PID « racine » d'une application (ex. le Discord.exe principal, dont les autres sont les enfants).
pub fn root_process(exe: &str) -> Option<u32> {
    let exe = exe.to_ascii_lowercase();
    let exe = if exe.ends_with(".exe") { exe } else { format!("{exe}.exe") };
    let all = processes();
    let matching: Vec<&ProcessInfo> =
        all.iter().filter(|p| p.exe.to_ascii_lowercase() == exe).collect();
    let pids: Vec<u32> = matching.iter().map(|p| p.pid).collect();
    matching
        .iter()
        .find(|p| !pids.contains(&p.parent))
        .or(matching.first())
        .map(|p| p.pid)
}

pub fn process_running(exe: &str) -> bool {
    root_process(exe).is_some()
}

/// Processus lancé « en tant qu'administrateur » ? (None : impossible à savoir). 0 = Rewind lui-même.
/// Un jeu élevé au premier plan rend ses touches invisibles pour un Rewind qui ne l'est pas (UIPI).
pub fn process_elevated(pid: u32) -> Option<bool> {
    use windows_sys::Win32::Security::{GetTokenInformation, TOKEN_ELEVATION, TOKEN_QUERY, TokenElevation};
    use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};
    unsafe {
        let own = pid == 0;
        let handle = if own { GetCurrentProcess() } else { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
        if handle.is_null() {
            return None;
        }
        let mut token: HANDLE = std::ptr::null_mut();
        let opened = OpenProcessToken(handle, TOKEN_QUERY, &mut token) != 0;
        if !own {
            CloseHandle(handle);
        }
        if !opened {
            return None;
        }
        let mut e: TOKEN_ELEVATION = std::mem::zeroed();
        let mut len = 0u32;
        let ok = GetTokenInformation(token, TokenElevation, &mut e as *mut _ as *mut _, std::mem::size_of::<TOKEN_ELEVATION>() as u32, &mut len);
        CloseHandle(token);
        if ok == 0 { None } else { Some(e.TokenIsElevated != 0) }
    }
}

/// Heure de démarrage d'un processus (secondes depuis 1970).
pub fn process_start(pid: u32) -> Option<f64> {
    use windows_sys::Win32::Foundation::FILETIME;
    unsafe {
        let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if handle.is_null() {
            return None;
        }
        let (mut c, mut e, mut k, mut u): (FILETIME, FILETIME, FILETIME, FILETIME) =
            (std::mem::zeroed(), std::mem::zeroed(), std::mem::zeroed(), std::mem::zeroed());
        let ok = GetProcessTimes(handle, &mut c, &mut e, &mut k, &mut u);
        CloseHandle(handle);
        if ok == 0 {
            return None;
        }
        let ticks = ((c.dwHighDateTime as u64) << 32) | c.dwLowDateTime as u64;
        // FILETIME : centaines de ns depuis 1601
        Some(ticks as f64 / 1e7 - 11_644_473_600.0)
    }
}

fn exe_of_pid(pid: u32) -> Option<String> {
    unsafe {
        let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if handle.is_null() {
            return None;
        }
        let mut buf = [0u16; 1024];
        let mut len = buf.len() as u32;
        let ok = QueryFullProcessImageNameW(handle, 0, buf.as_mut_ptr(), &mut len);
        CloseHandle(handle);
        if ok == 0 {
            return None;
        }
        let path = String::from_utf16_lossy(&buf[..len as usize]);
        Path::new(&path).file_name().map(|n| n.to_string_lossy().to_string())
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct Foreground {
    pub exe: String,
    pub title: String,
    pub fullscreen: bool,
    pub pid: u32,
}

/// Interface en anglais ? Réglage « language » : "en", "fr" ou "auto" (langue d'affichage de Windows).
pub fn english(language: &str) -> bool {
    match language {
        "en" => true,
        "fr" => false,
        _ => {
            let id = unsafe { windows_sys::Win32::Globalization::GetUserDefaultUILanguage() };
            id & 0x3ff != 0x0c // 0x0c = français (toutes variantes)
        }
    }
}

/// Jeux connus (nom de l'exécutable en minuscules).
const KNOWN_GAMES: &[&str] = &[
    "cs2.exe", "fortniteclient-win64-shipping.exe", "valorant-win64-shipping.exe", "league of legends.exe",
    "rocketleague.exe", "r5apex.exe", "r5apex_dx12.exe", "overwatch.exe", "javaw.exe", "minecraft.windows.exe",
    "robloxplayerbeta.exe", "gta5.exe", "gta5_enhanced.exe", "rainbowsix.exe", "tslgame.exe",
    "marvel-win64-shipping.exe", "dota2.exe", "deadlock.exe", "cod.exe", "cod24-cod.exe", "eldenring.exe",
    "helldivers2.exe", "thefinals.exe", "eft.exe", "rust.exe", "witcher3.exe", "cyberpunk2077.exe",
];

/// Applications plein écran qui ne sont PAS des jeux (navigateurs, lecteurs, bureau…).
const NOT_GAMES: &[&str] = &[
    "explorer.exe", "rewind.exe", "chrome.exe", "msedge.exe", "firefox.exe", "opera.exe", "opera_gx.exe", "brave.exe",
    "vlc.exe", "mpc-hc64.exe", "mpc-be64.exe", "potplayermini64.exe", "wmplayer.exe", "discord.exe",
    "steamwebhelper.exe", "steam.exe", "obs64.exe", "code.exe", "applicationframehost.exe", "lockapp.exe",
    "searchhost.exe", "shellexperiencehost.exe", "startmenuexperiencehost.exe", "textinputhost.exe",
    "photos.exe", "spotify.exe", "powerpnt.exe", "acrord32.exe", "windowsterminal.exe", "devenv.exe",
    "claude.exe", "taskmgr.exe", "mstsc.exe", "netflix.exe", "epicgameslauncher.exe", "riotclientux.exe",
    "battle.net.exe", "eadesktop.exe", "upc.exe",
];

/// Jeu connu, ou appli plein écran qui n'est ni un navigateur, ni un lecteur vidéo, ni le bureau.
pub fn is_game(exe: &str, fullscreen: bool) -> bool {
    let lower = exe.to_ascii_lowercase();
    KNOWN_GAMES.contains(&lower.as_str()) || (fullscreen && !NOT_GAMES.contains(&lower.as_str()))
}

/// Application au premier plan (pour savoir quel jeu/appli est filmé).
pub fn foreground() -> Option<Foreground> {
    unsafe {
        let hwnd: HWND = GetForegroundWindow();
        if hwnd.is_null() {
            return None;
        }
        let mut pid = 0u32;
        GetWindowThreadProcessId(hwnd, &mut pid);
        let exe = exe_of_pid(pid)?;
        let mut buf = [0u16; 512];
        // JAMAIS GetWindowTextW ici : pour une fenêtre de Rewind, il envoie un message au fil de
        // l'interface et attend sa réponse. Appelé sous le verrou du moteur pendant que l'interface
        // attend ce verrou, il figeait Rewind pour toujours (gel du 2026-11-13, vu dans une image
        // mémoire). InternalGetWindowText lit le titre directement, sans message.
        let n = InternalGetWindowText(hwnd, buf.as_mut_ptr(), buf.len() as i32);
        let title = String::from_utf16_lossy(&buf[..n.max(0) as usize]);
        let mut rect: RECT = std::mem::zeroed();
        GetWindowRect(hwnd, &mut rect);
        // Plein écran = la fenêtre couvre tout son écran. Seulement l'écran de la fenêtre : appelé
        // plusieurs fois par seconde, l'ancienne version relisait tous les écrans et leur fréquence
        // (EnumDisplaySettingsW, parfois lent selon le pilote) à chaque appel (2026-11-24).
        let mon = MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST);
        let mut info: MONITORINFO = std::mem::zeroed();
        info.cbSize = std::mem::size_of::<MONITORINFO>() as u32;
        let fullscreen = !mon.is_null() && GetMonitorInfoW(mon, &mut info) != 0 && {
            let m = info.rcMonitor;
            rect.left <= m.left && rect.top <= m.top && rect.right >= m.right && rect.bottom >= m.bottom
        };
        Some(Foreground { exe, title, fullscreen, pid })
    }
}

// ---------------------------------------------------------------------------
// Écrans
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, Serialize)]
pub struct Monitor {
    pub index: usize,
    pub handle: u64,
    pub name: String,
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
    pub primary: bool,
    /// Fréquence de rafraîchissement (Hz)
    pub refresh: u32,
}

/// Windows donne la fréquence de l'écran arrondie vers le bas (59,94 Hz → 59 ; 143,98 → 143 ; 164,8 → 164) :
/// on la ramène à la fréquence nominale, sinon aucune cadence ne la divise et l'image devient irrégulière
/// (audit Windows 10 / AMD du 2026-11-25).
pub fn normalize_refresh(hz: u32) -> u32 {
    const NOMINAL: [u32; 18] = [24, 30, 48, 50, 60, 72, 75, 90, 100, 120, 144, 165, 170, 175, 180, 200, 240, 360];
    if NOMINAL.contains(&(hz + 1)) { hz + 1 } else { hz }
}

/// Numéro de build de Windows (19045 = Windows 10 22H2, 22000 et plus = Windows 11), 0 si illisible.
pub fn windows_build() -> u32 {
    static BUILD: std::sync::OnceLock<u32> = std::sync::OnceLock::new();
    *BUILD.get_or_init(|| {
        reg_values(r"HKLM\SOFTWARE\Microsoft\Windows NT\CurrentVersion")
            .get("CurrentBuildNumber")
            .and_then(|b| b.trim().parse().ok())
            .unwrap_or(0)
    })
}

pub fn monitors() -> Vec<Monitor> {
    unsafe extern "system" fn callback(
        monitor: HMONITOR,
        _: HDC,
        _: *mut RECT,
        data: LPARAM,
    ) -> i32 {
        unsafe {
            let list = &mut *(data as *mut Vec<Monitor>);
            let mut info: MONITORINFOEXW = std::mem::zeroed();
            info.monitorInfo.cbSize = std::mem::size_of::<MONITORINFOEXW>() as u32;
            if GetMonitorInfoW(monitor, &mut info as *mut _ as *mut _) != 0 {
                let r = info.monitorInfo.rcMonitor;
                let mut mode: DEVMODEW = std::mem::zeroed();
                mode.dmSize = std::mem::size_of::<DEVMODEW>() as u16;
                let refresh = if EnumDisplaySettingsW(info.szDevice.as_ptr(), ENUM_CURRENT_SETTINGS, &mut mode) != 0 {
                    normalize_refresh(mode.dmDisplayFrequency)
                } else {
                    0
                };
                list.push(Monitor {
                    refresh,
                    index: list.len(),
                    handle: monitor as u64,
                    name: from_wide(&info.szDevice),
                    x: r.left,
                    y: r.top,
                    width: r.right - r.left,
                    height: r.bottom - r.top,
                    primary: info.monitorInfo.dwFlags & 1 != 0,
                });
            }
            1
        }
    }
    let mut list: Vec<Monitor> = Vec::new();
    unsafe {
        EnumDisplayMonitors(
            std::ptr::null_mut(),
            std::ptr::null(),
            Some(callback),
            &mut list as *mut _ as LPARAM,
        );
    }
    list
}

/// Réserve un nom de fichier libre (`base.ext`, `base (2).ext`…) en le créant vide, de façon atomique :
/// deux exports simultanés ne peuvent pas obtenir le même nom.
pub fn reserve_file(folder: &Path, base: &str, ext: &str) -> std::io::Result<PathBuf> {
    std::fs::create_dir_all(folder)?;
    let mut n = 1;
    loop {
        let name = if n == 1 { format!("{base}.{ext}") } else { format!("{base} ({n}).{ext}") };
        let path = folder.join(name);
        match std::fs::OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(_) => return Ok(path),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists && n < 1000 => n += 1,
            Err(e) => return Err(e),
        }
    }
}

/// Temps processeur consommé par un processus depuis son lancement (secondes) et nombre de handles ouverts.
/// Sert au test d'endurance : CPU moyen et détection de fuites.
pub fn process_cpu_handles(pid: u32) -> Option<(f64, u32)> {
    use windows_sys::Win32::{Foundation::FILETIME, System::Threading::GetProcessHandleCount};
    unsafe {
        let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if h.is_null() {
            return None;
        }
        let mut times: [FILETIME; 4] = std::mem::zeroed();
        let [c, e, k, u] = &mut times;
        let ok = GetProcessTimes(h, c, e, k, u);
        let mut handles = 0u32;
        GetProcessHandleCount(h, &mut handles);
        CloseHandle(h);
        if ok == 0 {
            return None;
        }
        let ft = |f: &FILETIME| ((f.dwHighDateTime as u64) << 32 | f.dwLowDateTime as u64) as f64 / 1e7;
        Some((ft(k) + ft(u), handles))
    }
}

/// Mémoire utilisée par un processus (octets, « working set »).
pub fn process_memory(pid: u32) -> Option<u64> {
    use windows_sys::Win32::System::ProcessStatus::{K32GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS};
    unsafe {
        let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if h.is_null() {
            return None;
        }
        let mut c: PROCESS_MEMORY_COUNTERS = std::mem::zeroed();
        c.cb = std::mem::size_of::<PROCESS_MEMORY_COUNTERS>() as u32;
        let ok = K32GetProcessMemoryInfo(h, &mut c, c.cb);
        CloseHandle(h);
        (ok != 0).then_some(c.WorkingSetSize as u64)
    }
}

/// Espace libre (octets) sur le disque qui contient `path`.
pub fn free_space(path: &Path) -> Option<u64> {
    use windows_sys::Win32::Storage::FileSystem::GetDiskFreeSpaceExW;
    let mut dir = path.to_path_buf();
    while !dir.exists() {
        dir = dir.parent()?.to_path_buf();
    }
    let w = wide(&dir.to_string_lossy());
    let mut free = 0u64;
    let ok = unsafe { GetDiskFreeSpaceExW(w.as_ptr(), &mut free, std::ptr::null_mut(), std::ptr::null_mut()) };
    (ok != 0).then_some(free)
}

/// Empreinte de la configuration des écrans (change si on branche un écran, change la résolution…).
pub fn monitors_signature() -> String {
    monitors()
        .iter()
        .map(|m| format!("{}:{}x{}@{}+{}+{}:{}", m.name, m.width, m.height, m.refresh, m.x, m.y, m.primary))
        .collect::<Vec<_>>()
        .join("|")
}

/// Nom lisible d'un exécutable connu.
pub fn friendly_app(exe: &str) -> String {
    let lower = exe.to_ascii_lowercase();
    let known = [
        ("cs2.exe", "Counter-Strike 2"),
        ("fortniteclient-win64-shipping.exe", "Fortnite"),
        ("valorant-win64-shipping.exe", "Valorant"),
        ("league of legends.exe", "League of Legends"),
        ("rocketleague.exe", "Rocket League"),
        ("r5apex.exe", "Apex Legends"),
        ("r5apex_dx12.exe", "Apex Legends"),
        ("overwatch.exe", "Overwatch 2"),
        ("javaw.exe", "Minecraft"),
        ("minecraft.windows.exe", "Minecraft"),
        ("robloxplayerbeta.exe", "Roblox"),
        ("gta5.exe", "GTA V"),
        ("gta5_enhanced.exe", "GTA V"),
        ("rainbowsix.exe", "Rainbow Six Siege"),
        ("tslgame.exe", "PUBG"),
        ("marvel-win64-shipping.exe", "Marvel Rivals"),
        ("dota2.exe", "Dota 2"),
        ("deadlock.exe", "Deadlock"),
        ("discord.exe", "Discord"),
        ("chrome.exe", "Chrome"),
        ("msedge.exe", "Edge"),
        ("firefox.exe", "Firefox"),
        ("opera.exe", "Opera"),
        ("brave.exe", "Brave"),
        ("spotify.exe", "Spotify"),
        ("explorer.exe", "Bureau"),
        // Menu Démarrer, recherche, clavier tactile, écran de verrouillage : c'est encore le bureau
        ("searchhost.exe", "Bureau"),
        ("startmenuexperiencehost.exe", "Bureau"),
        ("shellexperiencehost.exe", "Bureau"),
        ("textinputhost.exe", "Bureau"),
        ("lockapp.exe", "Bureau"),
        ("code.exe", "VS Code"),
        ("obs64.exe", "OBS"),
        ("steamwebhelper.exe", "Steam"),
        ("steam.exe", "Steam"),
        ("vlc.exe", "VLC"),
    ];
    for (k, v) in known {
        if lower == k {
            return v.to_string();
        }
    }
    let base = exe.trim_end_matches(".exe").trim_end_matches(".EXE");
    let mut chars = base.chars();
    match chars.next() {
        Some(f) => f.to_uppercase().collect::<String>() + chars.as_str(),
        None => "Bureau".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detection_des_jeux_pour_le_bandeau() {
        assert!(is_game("cs2.exe", false), "jeu connu, même en fenêtre");
        assert!(is_game("CS2.EXE", false), "majuscules");
        assert!(is_game("MonJeuIndie.exe", true), "appli inconnue en plein écran = jeu");
        assert!(!is_game("MonJeuIndie.exe", false), "appli inconnue en fenêtre : non");
        assert!(!is_game("chrome.exe", true), "vidéo plein écran dans le navigateur : non");
        assert!(!is_game("explorer.exe", true), "bureau : non");
        // Recherche / menu Démarrer : nommés « Bureau », pas « SearchHost »
        assert_eq!(friendly_app("SearchHost.exe"), "Bureau");
        assert_eq!(friendly_app("StartMenuExperienceHost.exe"), "Bureau");
        assert!(!is_game("rewind.exe", true), "Rewind lui-même : non");
    }

    #[test]
    fn noms_uniques_meme_en_parallele() {
        let dir = std::env::temp_dir().join(format!("rewind-test-noms-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let handles: Vec<_> = (0..8)
            .map(|_| {
                let d = dir.clone();
                std::thread::spawn(move || reserve_file(&d, "Clip 2026-11-13 12h00-00", "mp4").unwrap())
            })
            .collect();
        let mut names: Vec<PathBuf> = handles.into_iter().map(|h| h.join().unwrap()).collect();
        names.sort();
        names.dedup();
        assert_eq!(names.len(), 8, "chaque clip doit avoir son propre fichier");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// La lecture directe du registre donne les mêmes valeurs que `reg query` (qu'elle remplace).
    #[test]
    fn registre_lu_comme_reg_exe() {
        let key = r"HKLM\SOFTWARE\Microsoft\Windows NT\CurrentVersion";
        let native = reg_values(key);
        let out = command(Path::new("reg")).args(["query", key]).output().expect("reg.exe");
        let text = String::from_utf8_lossy(&out.stdout).to_string();
        for name in ["ProductName", "CurrentBuildNumber", "UBR", "DisplayVersion"] {
            let line = text.lines().find(|l| l.trim_start().starts_with(&format!("{name} "))).expect(name);
            let value = line.split_whitespace().skip(2).collect::<Vec<_>>().join(" ");
            assert_eq!(native.get(name).map(|v| v.trim().to_string()), Some(value), "{name}");
        }
        assert!(reg_values(r"HKLM\SOFTWARE\Cle\Qui\N'Existe\Pas").is_empty());
        assert!(reg_values(r"HKXX\SOFTWARE").is_empty());
    }
}
