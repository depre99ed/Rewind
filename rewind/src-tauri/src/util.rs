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
        MONITORINFOEXW,
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
        GetForegroundWindow, GetWindowRect, GetWindowTextW, GetWindowThreadProcessId,
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

pub fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

fn from_wide(buf: &[u16]) -> String {
    let end = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    String::from_utf16_lossy(&buf[..end])
}

/// Données de Rewind : `%LOCALAPPDATA%\com.rewind.app` (séparé du dossier d'installation
/// `%LOCALAPPDATA%\Rewind`, pour qu'une mise à jour ou une désinstallation ne les mélange pas).
pub fn data_dir() -> PathBuf {
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

/// Commande sans fenêtre console, en priorité un peu basse pour ne pas gêner les jeux.
pub fn command(program: &Path) -> Command {
    let mut cmd = Command::new(program);
    cmd.creation_flags(CREATE_NO_WINDOW | BELOW_NORMAL_PRIORITY_CLASS);
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
        let n = GetWindowTextW(hwnd, buf.as_mut_ptr(), buf.len() as i32);
        let title = String::from_utf16_lossy(&buf[..n.max(0) as usize]);
        let mut rect: RECT = std::mem::zeroed();
        GetWindowRect(hwnd, &mut rect);
        let fullscreen = monitors().iter().any(|m| {
            rect.left <= m.x
                && rect.top <= m.y
                && rect.right >= m.x + m.width
                && rect.bottom >= m.y + m.height
        });
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
                    mode.dmDisplayFrequency
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
}
