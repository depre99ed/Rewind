//! Veille de l'interface : si la fenêtre ne répond plus pendant 10 s, on le note dans le journal
//! (avec l'état des verrous du moteur) et on écrit une petite image mémoire (`gel-*.dmp`, quelques Mo)
//! qui montre où chaque fil attend. C'est ce qui a permis de trouver le gel du 2026-11-13.

use crate::{AppState, util};
use std::{
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};
use tauri::{AppHandle, Manager};

const PERIOD: Duration = Duration::from_secs(2);
const FROZEN_AFTER: Duration = Duration::from_secs(10);
const KEEP_DUMPS: usize = 3;

pub fn start(app: AppHandle) {
    let answered = Arc::new(AtomicU64::new(0));
    let _ = std::thread::Builder::new().name("veille-interface".into()).spawn(move || {
        let mut sent = 0u64;
        let mut asked_at = Instant::now();
        let mut reported = false;
        let mut last_loop = Instant::now();
        loop {
            std::thread::sleep(PERIOD);
            // Sortie de veille du PC : le temps écoulé ne veut rien dire, on repart de zéro
            if last_loop.elapsed() > PERIOD * 3 {
                asked_at = Instant::now();
            }
            last_loop = Instant::now();
            if answered.load(Ordering::Acquire) >= sent {
                if reported {
                    log::warn!("interface de nouveau réactive après {:.0} s", asked_at.elapsed().as_secs_f64());
                    reported = false;
                }
                sent += 1;
                asked_at = Instant::now();
                let a = Arc::clone(&answered);
                let n = sent;
                if app.run_on_main_thread(move || a.store(n, Ordering::Release)).is_err() {
                    return; // l'application se ferme
                }
            } else if !reported && asked_at.elapsed() >= FROZEN_AFTER {
                reported = true;
                let state = app.state::<AppState>();
                let busy = |free: bool| if free { "libre" } else { "OCCUPÉ" };
                log::warn!(
                    "interface bloquée depuis {:.0} s (verrous : moteur {}, réglages {}, bibliothèque {}) : image mémoire écrite pour le diagnostic",
                    asked_at.elapsed().as_secs_f64(),
                    busy(state.recorder.try_lock().is_some()),
                    busy(state.settings.try_lock().is_some()),
                    busy(state.library.try_lock().is_some()),
                );
                write_dump();
            }
        }
    });
}

/// Image mémoire légère du processus (piles de tous les fils), gardée en 3 exemplaires au plus.
fn write_dump() {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::System::{
        Diagnostics::Debug::{MiniDumpNormal, MiniDumpWithThreadInfo, MiniDumpWriteDump},
        Threading::{GetCurrentProcess, GetCurrentProcessId},
    };
    let dir = util::data_dir();
    let path = dir.join(format!("gel-{}.dmp", chrono::Local::now().format("%Y%m%d-%H%M%S")));
    let Ok(file) = std::fs::File::create(&path) else { return };
    let ok = unsafe {
        MiniDumpWriteDump(
            GetCurrentProcess(),
            GetCurrentProcessId(),
            file.as_raw_handle() as _,
            MiniDumpNormal | MiniDumpWithThreadInfo,
            std::ptr::null(),
            std::ptr::null(),
            std::ptr::null(),
        )
    };
    drop(file);
    if ok == 0 {
        let _ = std::fs::remove_file(&path);
        log::warn!("image mémoire impossible");
        return;
    }
    log::warn!("image mémoire : {}", path.display());
    // Ménage : on garde les plus récentes
    if let Ok(entries) = std::fs::read_dir(&dir) {
        let mut dumps: Vec<_> = entries
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.file_name().is_some_and(|n| n.to_string_lossy().starts_with("gel-")) && p.extension().is_some_and(|x| x == "dmp"))
            .collect();
        dumps.sort();
        while dumps.len() > KEEP_DUMPS {
            let _ = std::fs::remove_file(dumps.remove(0));
        }
    }
}
