//! Jeux lancés en administrateur : tâche planifiée qui démarre l'assistant d'entrées en administrateur
//! (`rewind.exe --input-helper`, voir inputs.rs) sans invite Windows à chaque fois.
//!
//! - Activation : UNE invite Windows (création de la tâche avec les droits les plus élevés) ;
//! - ensuite Rewind lance la tâche lui-même (`schtasks /Run`), toutes les 15 s si l'assistant n'est pas là ;
//! - désactivation : une invite Windows (suppression de la tâche).
//! La tâche n'a aucun déclencheur : elle ne démarre que quand Rewind la lance.

use crate::{inputs, util};
use anyhow::{Result, bail};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};

const TASK: &str = "Rewind\\Entrees administrateur";
static ENABLED: AtomicBool = AtomicBool::new(false);

fn schtasks() -> PathBuf {
    let root = std::env::var_os("SystemRoot").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("C:\\Windows"));
    root.join("System32").join("schtasks.exe")
}

/// La tâche existe-t-elle ? (lecture sans droits particuliers)
fn exists() -> bool {
    util::command(&schtasks()).args(["/Query", "/TN", TASK]).output().map(|o| o.status.success()).unwrap_or(false)
}

pub fn enabled() -> bool {
    ENABLED.load(Ordering::Relaxed)
}

/// Lance l'assistant (la tâche lui donne les droits administrateur, sans invite).
fn run() -> bool {
    util::command(&schtasks()).args(["/Run", "/TN", TASK]).output().map(|o| o.status.success()).unwrap_or(false)
}

/// Au démarrage : tâche présente → assistant lancé, puis relancé s'il disparaît.
pub fn watch() {
    if cfg!(debug_assertions) || std::env::var_os("REWIND_DATA_DIR").is_some() {
        return; // jamais depuis un test : la tâche vise l'exécutable installé
    }
    let _ = std::thread::Builder::new().name("entrees-admin-tache".into()).spawn(|| {
        ENABLED.store(exists(), Ordering::Relaxed);
        let mut warned = false;
        loop {
            if enabled() && !inputs::HELPER_ON.load(Ordering::Relaxed) && !run() && !warned {
                warned = true;
                log::warn!("entrées : impossible de lancer l'assistant administrateur (tâche « {TASK} »)");
            }
            if !enabled() && inputs::game_needs_helper() && !warned {
                warned = true;
                log::warn!("entrées : le jeu tourne en administrateur, Rewind non : ses touches sont invisibles (Réglages › Raccourcis › Jeux lancés en administrateur)");
            }
            std::thread::sleep(std::time::Duration::from_secs(15));
        }
    });
}

/// Lance `schtasks` en administrateur (invite Windows) et attend son résultat.
fn elevated_schtasks(args: &str) -> Result<()> {
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::System::Threading::{GetExitCodeProcess, WaitForSingleObject};
    use windows_sys::Win32::UI::Shell::{SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW, ShellExecuteExW};
    let wide = |s: &str| s.encode_utf16().chain(std::iter::once(0)).collect::<Vec<u16>>();
    let (verb, file, params) = (wide("runas"), wide(&schtasks().to_string_lossy()), wide(args));
    unsafe {
        let mut info: SHELLEXECUTEINFOW = std::mem::zeroed();
        info.cbSize = std::mem::size_of::<SHELLEXECUTEINFOW>() as u32;
        info.fMask = SEE_MASK_NOCLOSEPROCESS;
        info.lpVerb = verb.as_ptr();
        info.lpFile = file.as_ptr();
        info.lpParameters = params.as_ptr();
        info.nShow = 0; // SW_HIDE
        if ShellExecuteExW(&mut info) == 0 {
            bail!("autorisation refusée");
        }
        if info.hProcess.is_null() {
            bail!("lancement impossible");
        }
        WaitForSingleObject(info.hProcess, 60_000);
        let mut code = 1u32;
        GetExitCodeProcess(info.hProcess, &mut code);
        CloseHandle(info.hProcess);
        if code != 0 {
            bail!("schtasks a échoué (code {code})");
        }
    }
    Ok(())
}

/// Crée la tâche (une invite Windows) puis lance l'assistant (gardé en secours, plus proposé).
#[allow(dead_code)]
pub fn enable() -> Result<()> {
    if cfg!(debug_assertions) {
        bail!("indisponible dans une version de développement");
    }
    let exe = std::env::current_exe()?;
    // Tâche décrite en XML : pas d'arrêt sur batterie, pas de limite de durée, une seule copie à la fois
    let xml = format!(
        r#"<?xml version="1.0" encoding="UTF-16"?>
<Task version="1.2" xmlns="http://schemas.microsoft.com/windows/2004/02/mit/task">
  <RegistrationInfo><Description>Rewind : touches et raccourcis dans les jeux lancés en administrateur</Description></RegistrationInfo>
  <Principals><Principal id="Author"><LogonType>InteractiveToken</LogonType><RunLevel>HighestAvailable</RunLevel></Principal></Principals>
  <Settings>
    <MultipleInstancesPolicy>IgnoreNew</MultipleInstancesPolicy>
    <DisallowStartIfOnBatteries>false</DisallowStartIfOnBatteries>
    <StopIfGoingOnBatteries>false</StopIfGoingOnBatteries>
    <ExecutionTimeLimit>PT0S</ExecutionTimeLimit>
    <AllowStartOnDemand>true</AllowStartOnDemand>
    <Priority>5</Priority>
  </Settings>
  <Actions Context="Author"><Exec><Command>{}</Command><Arguments>--input-helper</Arguments></Exec></Actions>
</Task>"#,
        exe.to_string_lossy().replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
    );
    let file = std::env::temp_dir().join("rewind-tache-entrees.xml");
    // schtasks lit le XML en UTF-16 (avec marque d'ordre des octets)
    let mut bytes = vec![0xFF, 0xFE];
    for u in xml.encode_utf16() {
        bytes.extend_from_slice(&u.to_le_bytes());
    }
    std::fs::write(&file, bytes)?;
    let res = elevated_schtasks(&format!("/Create /TN \"{TASK}\" /XML \"{}\" /F", file.display()));
    let _ = std::fs::remove_file(&file);
    res?;
    ENABLED.store(true, Ordering::Relaxed);
    log::info!("entrées : assistant administrateur autorisé (tâche « {TASK} »)");
    if !run() {
        bail!("tâche créée, mais lancement impossible");
    }
    Ok(())
}

/// Supprime la tâche (une invite Windows). L'assistant en cours s'arrête avec Rewind.
#[allow(dead_code)]
pub fn disable() -> Result<()> {
    elevated_schtasks(&format!("/Delete /TN \"{TASK}\" /F"))?;
    ENABLED.store(false, Ordering::Relaxed);
    log::info!("entrées : assistant administrateur retiré");
    Ok(())
}

// ---------------------------------------------------------------------------
// Rewind entier en administrateur (2026-11-15, choix de l'utilisateur) : le plus simple et le plus sûr
// pour voir les touches des jeux lancés en administrateur (Steam en admin → CS2 en admin).
// Une tâche « Rewind\Rewind administrateur » (droits les plus élevés) est créée UNE fois avec l'accord
// de Windows ; ensuite chaque lancement normal (raccourci, démarrage de Windows) passe par elle, sans
// nouvelle demande. Refus : Rewind marche normalement, la demande ne revient pas (Réglages › Raccourcis).
// ---------------------------------------------------------------------------

const MAIN_TASK: &str = "Rewind\\Rewind administrateur";

fn task_exists(name: &str) -> bool {
    util::command(&schtasks()).args(["/Query", "/TN", name]).output().map(|o| o.status.success()).unwrap_or(false)
}

fn task_run(name: &str) -> bool {
    util::command(&schtasks()).args(["/Run", "/TN", name]).output().map(|o| o.status.success()).unwrap_or(false)
}

pub fn main_enabled() -> bool {
    task_exists(MAIN_TASK)
}

/// Crée une tâche « droits les plus élevés » qui lance cet exécutable (une invite Windows).
fn create_task(name: &str, args: &str, what: &str) -> Result<()> {
    let file = write_task_xml(args, what)?;
    let res = elevated_schtasks(&format!("/Create /TN \"{name}\" /XML \"{}\" /F", file.display()));
    let _ = std::fs::remove_file(&file);
    res
}

/// Description XML de la tâche (UTF-16), écrite dans un fichier temporaire.
///
/// `Parallel` (et non `IgnoreNew`, jusqu'à la 0.3.5) : si un Rewind lancé par la tâche tourne déjà,
/// Windows IGNORAIT un nouveau lancement ; le raccourci ne faisait alors plus rien, fenêtre fermée
/// (bug du 2026-11-15). Le nouveau Rewind démarre, voit l'ancien et lui passe la main (une seule instance).
fn write_task_xml(args: &str, what: &str) -> Result<PathBuf> {
    let exe = std::env::current_exe()?;
    let esc = |s: &str| s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;");
    let xml = format!(
        r#"<?xml version="1.0" encoding="UTF-16"?>
<Task version="1.2" xmlns="http://schemas.microsoft.com/windows/2004/02/mit/task">
  <RegistrationInfo><Description>{}</Description></RegistrationInfo>
  <Principals><Principal id="Author"><LogonType>InteractiveToken</LogonType><RunLevel>HighestAvailable</RunLevel></Principal></Principals>
  <Settings>
    <MultipleInstancesPolicy>Parallel</MultipleInstancesPolicy>
    <DisallowStartIfOnBatteries>false</DisallowStartIfOnBatteries>
    <StopIfGoingOnBatteries>false</StopIfGoingOnBatteries>
    <ExecutionTimeLimit>PT0S</ExecutionTimeLimit>
    <AllowStartOnDemand>true</AllowStartOnDemand>
    <Priority>5</Priority>
  </Settings>
  <Actions Context="Author"><Exec><Command>{}</Command><Arguments>{}</Arguments></Exec></Actions>
</Task>"#,
        esc(what),
        esc(&exe.to_string_lossy()),
        esc(args)
    );
    let file = std::env::temp_dir().join(format!("rewind-tache-{}.xml", std::process::id()));
    let mut bytes = vec![0xFF, 0xFE];
    for u in xml.encode_utf16() {
        bytes.extend_from_slice(&u.to_le_bytes());
    }
    std::fs::write(&file, bytes)?;
    Ok(file)
}

/// Rewind administrateur : une tâche créée avant la 0.3.6 (règle `IgnoreNew`) est réécrite avec la
/// nouvelle règle. Déjà administrateur : aucune invite Windows.
pub fn repair_main_task() {
    if cfg!(debug_assertions) || std::env::var_os("REWIND_DATA_DIR").is_some() || util::process_elevated(0) != Some(true) {
        return;
    }
    let _ = std::thread::Builder::new().name("tache-admin".into()).spawn(|| {
        let Ok(out) = util::command(&schtasks()).args(["/Query", "/TN", MAIN_TASK, "/XML"]).output() else { return };
        let xml = String::from_utf8_lossy(&out.stdout).to_lowercase();
        // Seulement la tâche de CET exécutable (une autre copie de Rewind ne la détourne pas)
        let exe = std::env::current_exe().map(|p| p.to_string_lossy().to_lowercase()).unwrap_or_default();
        if !out.status.success() || !xml.contains("ignorenew") || exe.is_empty() || !xml.contains(&exe) {
            return;
        }
        let res = write_task_xml("", "Rewind : clips instantanés, lancé en administrateur pour voir les touches des jeux").and_then(|file| {
            let out = util::command(&schtasks())
                .args(["/Create", "/TN", MAIN_TASK, "/XML"])
                .arg(&file)
                .arg("/F")
                .output();
            let _ = std::fs::remove_file(&file);
            match out {
                Ok(o) if o.status.success() => Ok(()),
                Ok(o) => bail!("{}", String::from_utf8_lossy(&o.stderr).trim()),
                Err(e) => Err(e.into()),
            }
        });
        match res {
            Ok(()) => log::info!("tâche « {MAIN_TASK} » réparée : un nouveau lancement n'est plus ignoré"),
            Err(e) => log::warn!("tâche « {MAIN_TASK} » : réparation impossible ({e})"),
        }
    });
}

fn tray_flag() -> PathBuf {
    util::data_dir().join("demarrage-discret.txt")
}

/// Rewind relancé par la tâche après un démarrage de Windows : rester caché.
/// Le drapeau ne vaut que 2 minutes : un drapeau resté là (lancement ignoré, plantage) cachait la
/// fenêtre d'un lancement normal beaucoup plus tard.
pub fn take_tray_flag() -> bool {
    let f = tray_flag();
    let fresh = std::fs::metadata(&f)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.elapsed().ok())
        .is_some_and(|age| age.as_secs() < 120);
    let _ = std::fs::remove_file(f);
    fresh
}

// ---------------------------------------------------------------------------
// Un Rewind tourne déjà (fenêtre fermée, enregistrement en arrière-plan) : le nouveau lancement
// (raccourci, menu Démarrer) lui demande d'ouvrir sa fenêtre puis s'arrête.
//
// Avant la 0.3.6, le raccourci ne faisait rien quand Rewind tournait en administrateur : le lancement
// passait par la tâche, que Windows ignorait (`IgnoreNew`), et le message de l'extension « une seule
// instance » d'un programme normal vers un programme administrateur est bloqué par Windows (UIPI).
// Il fallait tuer Rewind dans le gestionnaire des tâches.
// ---------------------------------------------------------------------------

/// Doit rester égal à `identifier` dans tauri.conf.json (noms choisis par l'extension « une seule instance »).
/// Test seulement : `REWIND_APP_ID` à la compilation, avec `TAURI_CONFIG={"identifier":…}`, pour essayer
/// pendant que le vrai Rewind tourne.
const APP_ID: &str = match option_env!("REWIND_APP_ID") {
    Some(id) => id,
    None => "com.rewind.app",
};

/// Petite fenêtre invisible de l'extension « une seule instance » du Rewind déjà ouvert.
fn instance_window() -> Option<windows_sys::Win32::Foundation::HWND> {
    use windows_sys::Win32::UI::WindowsAndMessaging::FindWindowW;
    let class = util::wide(&format!("{APP_ID}-sic"));
    let name = util::wide(&format!("{APP_ID}-siw"));
    let hwnd = unsafe { FindWindowW(class.as_ptr(), name.as_ptr()) };
    (!hwnd.is_null()).then_some(hwnd)
}

/// Au lancement, avant tout : true si un Rewind tourne déjà (ce lancement doit alors s'arrêter).
/// Jamais deux Rewind en même temps (deux enregistreurs), même si le message ne passe pas.
pub fn wake_running(tray: bool) -> bool {
    use windows_sys::Win32::System::DataExchange::COPYDATASTRUCT;
    use windows_sys::Win32::System::Threading::GetCurrentProcessId;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        AllowSetForegroundWindow, GetWindowThreadProcessId, SMTO_ABORTIFHUNG, SendMessageTimeoutW, WM_COPYDATA,
    };
    let Some(hwnd) = instance_window() else { return false };
    let mut pid = 0u32;
    unsafe { GetWindowThreadProcessId(hwnd, &mut pid) };
    if pid == 0 || pid == unsafe { GetCurrentProcessId() } {
        return false;
    }
    if tray {
        return true; // démarrage de Windows alors que Rewind tourne : rien à faire
    }
    unsafe { AllowSetForegroundWindow(pid) };
    // Même format que l'extension (dossier courant puis arguments, séparés par un zéro)
    let mut data = std::env::current_dir().unwrap_or_default().to_string_lossy().into_owned();
    for arg in std::env::args_os() {
        data.push('\0');
        data.push_str(&arg.to_string_lossy());
    }
    let cds = COPYDATASTRUCT { dwData: 1543, cbData: data.len() as u32, lpData: data.as_ptr() as _ };
    let mut answer = 0usize;
    let sent = unsafe { SendMessageTimeoutW(hwnd, WM_COPYDATA, 0, &cds as *const _ as isize, SMTO_ABORTIFHUNG, 5000, &mut answer) };
    if sent != 0 && answer == 1 {
        return true;
    }
    // Refusé (Rewind administrateur d'avant la 0.3.6) ou fenêtre figée : la tâche lance un Rewind
    // administrateur, qui peut, lui, passer le message.
    if util::process_elevated(0) == Some(false) && main_enabled() {
        task_run(MAIN_TASK);
    }
    true
}

/// Rewind déjà ouvert : accepter le message « ouvre ta fenêtre » d'un lancement sans droits
/// administrateur (sinon Windows le bloque quand Rewind est administrateur).
pub fn accept_wake_messages() {
    use windows_sys::Win32::System::Threading::GetCurrentProcessId;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        ChangeWindowMessageFilterEx, GetWindowThreadProcessId, MSGFLT_ALLOW, WM_COPYDATA,
    };
    let Some(hwnd) = instance_window() else {
        log::warn!("une seule instance : fenêtre de réveil introuvable");
        return;
    };
    let mut pid = 0u32;
    unsafe { GetWindowThreadProcessId(hwnd, &mut pid) };
    if pid != unsafe { GetCurrentProcessId() } {
        return;
    }
    if unsafe { ChangeWindowMessageFilterEx(hwnd, WM_COPYDATA, MSGFLT_ALLOW, std::ptr::null_mut()) } == 0 {
        log::warn!("une seule instance : message de réveil non autorisé");
    }
}

/// Au lancement (avant tout) : true si un Rewind administrateur prend le relais (celui-ci s'arrête).
pub fn start_elevated(tray: bool) -> bool {
    if cfg!(debug_assertions) || std::env::var_os("REWIND_DATA_DIR").is_some() || util::process_elevated(0) != Some(false) {
        return false;
    }
    if main_enabled() {
        if tray {
            let _ = std::fs::write(tray_flag(), "1");
        }
        if task_run(MAIN_TASK) {
            return true;
        }
        log::warn!("lancement en administrateur impossible : Rewind démarre sans");
        return false;
    }
    // Première fois seulement, et jamais au démarrage de Windows (pas d'invite surprise)
    let asked = util::data_dir().join("admin-demande.txt");
    if tray || asked.is_file() {
        return false;
    }
    let _ = std::fs::create_dir_all(util::data_dir());
    let _ = std::fs::write(&asked, "1");
    match create_task(MAIN_TASK, "", "Rewind : clips instantanés, lancé en administrateur pour voir les touches des jeux") {
        Ok(()) => task_run(MAIN_TASK),
        Err(_) => false,
    }
}

/// Réglages : « Lancer Rewind en administrateur » (une invite), puis Rewind se relance.
pub fn enable_main() -> Result<()> {
    if cfg!(debug_assertions) {
        bail!("indisponible dans une version de développement");
    }
    create_task(MAIN_TASK, "", "Rewind : clips instantanés, lancé en administrateur pour voir les touches des jeux")?;
    log::info!("Rewind sera lancé en administrateur (tâche « {MAIN_TASK} ») : relance");
    // Le nouveau Rewind (administrateur) attend que celui-ci soit fermé (`wait_previous`), puis démarre.
    // Avant la 0.3.7 : `cmd /C ping -n 3 127.0.0.1 >nul & schtasks /Run…`, une temporisation que les
    // antivirus connaissent bien chez les logiciels malveillants.
    std::fs::write(relaunch_flag(), std::process::id().to_string())?;
    if !task_run(MAIN_TASK) {
        let _ = std::fs::remove_file(relaunch_flag());
        bail!("tâche créée, mais lancement impossible");
    }
    std::thread::spawn(|| {
        std::thread::sleep(std::time::Duration::from_millis(400));
        std::process::exit(0);
    });
    Ok(())
}

fn relaunch_flag() -> PathBuf {
    util::data_dir().join("relance-admin.txt")
}

/// Au lancement, avant tout : si un Rewind vient de se fermer pour laisser la place à celui-ci
/// (passage en administrateur), on attend qu'il soit vraiment parti (10 s au plus).
pub fn wait_previous() {
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::System::Threading::{OpenProcess, PROCESS_SYNCHRONIZE, WaitForSingleObject};
    let f = relaunch_flag();
    let Ok(text) = std::fs::read_to_string(&f) else { return };
    let fresh = std::fs::metadata(&f).and_then(|m| m.modified()).ok().and_then(|t| t.elapsed().ok()).is_some_and(|a| a.as_secs() < 30);
    let _ = std::fs::remove_file(&f);
    let Ok(pid) = text.trim().parse::<u32>() else { return };
    if !fresh || pid == std::process::id() {
        return;
    }
    unsafe {
        let h = OpenProcess(PROCESS_SYNCHRONIZE, 0, pid);
        if !h.is_null() {
            WaitForSingleObject(h, 10_000);
            CloseHandle(h);
        }
    }
}

/// Réglages : ne plus lancer Rewind en administrateur (une invite). Effet au prochain lancement.
pub fn disable_main() -> Result<()> {
    elevated_schtasks(&format!("/Delete /TN \"{MAIN_TASK}\" /F"))?;
    log::info!("Rewind ne sera plus lancé en administrateur");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Passage en administrateur : le nouveau Rewind attend la fin de l'ancien (sans cmd ni ping).
    #[test]
    fn relance_attend_la_fin_de_l_ancien() {
        // Un petit programme qui dure ~1,5 s joue le rôle de l'ancien Rewind
        let mut old = std::process::Command::new(std::env::var("SystemRoot").unwrap_or(r"C:\Windows".into()) + r"\System32\PING.EXE")
            .args(["-n", "3", "127.0.0.1"])
            .stdout(std::process::Stdio::null())
            .spawn()
            .expect("programme de test");
        let _ = std::fs::create_dir_all(util::data_dir());
        std::fs::write(relaunch_flag(), old.id().to_string()).unwrap();
        let t = std::time::Instant::now();
        wait_previous();
        assert!(t.elapsed().as_secs_f64() > 1.0, "doit attendre la fin de l'ancien");
        assert!(old.try_wait().unwrap().is_some(), "l'ancien est bien terminé");
        assert!(!relaunch_flag().exists(), "le mot est effacé");
        // Sans mot : aucune attente
        let t = std::time::Instant::now();
        wait_previous();
        assert!(t.elapsed().as_millis() < 200);
    }
}

