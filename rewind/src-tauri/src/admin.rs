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
    let exe = std::env::current_exe()?;
    let esc = |s: &str| s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;");
    let xml = format!(
        r#"<?xml version="1.0" encoding="UTF-16"?>
<Task version="1.2" xmlns="http://schemas.microsoft.com/windows/2004/02/mit/task">
  <RegistrationInfo><Description>{}</Description></RegistrationInfo>
  <Principals><Principal id="Author"><LogonType>InteractiveToken</LogonType><RunLevel>HighestAvailable</RunLevel></Principal></Principals>
  <Settings>
    <MultipleInstancesPolicy>IgnoreNew</MultipleInstancesPolicy>
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
    let file = std::env::temp_dir().join("rewind-tache.xml");
    let mut bytes = vec![0xFF, 0xFE];
    for u in xml.encode_utf16() {
        bytes.extend_from_slice(&u.to_le_bytes());
    }
    std::fs::write(&file, bytes)?;
    let res = elevated_schtasks(&format!("/Create /TN \"{name}\" /XML \"{}\" /F", file.display()));
    let _ = std::fs::remove_file(&file);
    res
}

fn tray_flag() -> PathBuf {
    util::data_dir().join("demarrage-discret.txt")
}

/// Rewind relancé par la tâche après un démarrage de Windows : rester caché.
pub fn take_tray_flag() -> bool {
    let f = tray_flag();
    let on = f.is_file();
    if on {
        let _ = std::fs::remove_file(f);
    }
    on
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
    // Ce Rewind se ferme d'abord (une seule instance), puis la tâche lance le nouveau
    let cmd = format!("ping -n 3 127.0.0.1 >nul & \"{}\" /Run /TN \"{MAIN_TASK}\"", schtasks().display());
    util::command(std::path::Path::new("cmd.exe")).args(["/C", &cmd]).spawn()?;
    std::thread::spawn(|| {
        std::thread::sleep(std::time::Duration::from_millis(400));
        std::process::exit(0);
    });
    Ok(())
}

/// Réglages : ne plus lancer Rewind en administrateur (une invite). Effet au prochain lancement.
pub fn disable_main() -> Result<()> {
    elevated_schtasks(&format!("/Delete /TN \"{MAIN_TASK}\" /F"))?;
    log::info!("Rewind ne sera plus lancé en administrateur");
    Ok(())
}