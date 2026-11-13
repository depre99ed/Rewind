//! Touches clavier / souris pour l'option « touches à l'écran » (désactivée par défaut).
//!
//! Crochets « bas niveau » de Windows (aucune injection dans le jeu). Pour la vie privée :
//! seules les touches de jeu d'une liste fixe sont notées (ZQSD/WASD, espace, Maj, Ctrl, quelques
//! lettres d'action, 1-5, boutons de la souris), seulement quand un jeu est au premier plan, et tout
//! reste en mémoire : rien n'est écrit sur le disque sauf avec un clip, si l'option est active.

use crate::util;
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use windows_sys::Win32::{
    Foundation::{LPARAM, LRESULT, WPARAM},
    System::{LibraryLoader::GetModuleHandleW, Threading::GetCurrentThreadId},
    UI::WindowsAndMessaging::{
        CallNextHookEx, DispatchMessageW, GetMessageW, KBDLLHOOKSTRUCT, MSG, MSLLHOOKSTRUCT, PostThreadMessageW,
        SetWindowsHookExW, TranslateMessage, UnhookWindowsHookEx, WH_KEYBOARD_LL, WH_MOUSE_LL, WM_KEYDOWN, WM_KEYUP,
        WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MBUTTONDOWN, WM_MBUTTONUP, WM_QUIT, WM_RBUTTONDOWN, WM_RBUTTONUP,
        WM_SYSKEYDOWN, WM_SYSKEYUP, WM_XBUTTONDOWN, WM_XBUTTONUP,
    },
};

/// Un appui ou un relâchement. `key` = code de touche virtuelle Windows (1 = clic gauche, 2 = clic droit…).
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq)]
pub struct InputEvent {
    pub t: f64,
    pub key: u16,
    pub down: bool,
}

/// Touches notées (code virtuel → nom affiché). Disposition indépendante du clavier (AZERTY : Z = W).
pub const KEYS: &[(u16, &str)] = &[
    (0x57, "W"), (0x41, "A"), (0x53, "S"), (0x44, "D"), (0x51, "Q"), (0x45, "E"), (0x52, "R"), (0x46, "F"),
    (0x47, "G"), (0x43, "C"), (0x56, "V"), (0x20, "Espace"), (0xA0, "Maj"), (0xA2, "Ctrl"), (0x09, "Tab"),
    (0x31, "1"), (0x32, "2"), (0x33, "3"), (0x34, "4"), (0x35, "5"),
    (0x01, "Clic G"), (0x02, "Clic D"), (0x04, "Molette"), (0x05, "Souris 4"), (0x06, "Souris 5"),
];

static EVENTS: Mutex<VecDeque<InputEvent>> = Mutex::new(VecDeque::new());
static PRESSED: Mutex<[bool; 256]> = Mutex::new([false; 256]);
/// Mis à jour par le fil de surveillance : un jeu est-il au premier plan ?
pub static GAME_FOCUSED: AtomicBool = AtomicBool::new(false);
static RUNNING: AtomicBool = AtomicBool::new(false);
static THREAD: AtomicU32 = AtomicU32::new(0);
const MAX_EVENTS: usize = 60_000;

fn allowed(vk: u16) -> bool {
    KEYS.iter().any(|(k, _)| *k == vk)
}

fn record(vk: u16, down: bool) {
    if !GAME_FOCUSED.load(Ordering::Relaxed) || !allowed(vk) {
        return;
    }
    {
        // Les répétitions automatiques (touche maintenue) ne sont pas des appuis
        let mut p = PRESSED.lock();
        if p[vk as usize] == down {
            return;
        }
        p[vk as usize] = down;
    }
    let mut ev = EVENTS.lock();
    ev.push_back(InputEvent { t: util::now(), key: vk, down });
    while ev.len() > MAX_EVENTS {
        ev.pop_front();
    }
}

unsafe extern "system" fn keyboard(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code >= 0 {
        let info = unsafe { &*(lparam as *const KBDLLHOOKSTRUCT) };
        let vk = match info.vkCode as u16 {
            0xA1 | 0x10 => 0xA0, // Maj droite / générique → Maj
            0xA3 | 0x11 => 0xA2, // Ctrl droit / générique → Ctrl
            v => v,
        };
        match wparam as u32 {
            WM_KEYDOWN | WM_SYSKEYDOWN => record(vk, true),
            WM_KEYUP | WM_SYSKEYUP => record(vk, false),
            _ => {}
        }
    }
    unsafe { CallNextHookEx(std::ptr::null_mut(), code, wparam, lparam) }
}

unsafe extern "system" fn mouse(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code >= 0 {
        let msg = wparam as u32;
        let x = || {
            let info = unsafe { &*(lparam as *const MSLLHOOKSTRUCT) };
            if (info.mouseData >> 16) & 0xFFFF == 1 { 0x05 } else { 0x06 }
        };
        match msg {
            WM_LBUTTONDOWN => record(0x01, true),
            WM_LBUTTONUP => record(0x01, false),
            WM_RBUTTONDOWN => record(0x02, true),
            WM_RBUTTONUP => record(0x02, false),
            WM_MBUTTONDOWN => record(0x04, true),
            WM_MBUTTONUP => record(0x04, false),
            WM_XBUTTONDOWN => record(x(), true),
            WM_XBUTTONUP => record(x(), false),
            _ => {}
        }
    }
    unsafe { CallNextHookEx(std::ptr::null_mut(), code, wparam, lparam) }
}

/// Active ou coupe l'enregistrement des touches (fil dédié avec sa boucle de messages).
pub fn set_enabled(on: bool) {
    if on {
        if RUNNING.swap(true, Ordering::SeqCst) {
            return;
        }
        let spawned = std::thread::Builder::new().name("touches".into()).spawn(|| unsafe {
            THREAD.store(GetCurrentThreadId(), Ordering::SeqCst);
            let module = GetModuleHandleW(std::ptr::null());
            let kb = SetWindowsHookExW(WH_KEYBOARD_LL, Some(keyboard), module, 0);
            let ms = SetWindowsHookExW(WH_MOUSE_LL, Some(mouse), module, 0);
            if kb.is_null() || ms.is_null() {
                log::warn!("touches à l'écran : crochets refusés par Windows");
            } else {
                log::info!("touches à l'écran : enregistrement actif");
            }
            let mut msg: MSG = std::mem::zeroed();
            while GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) > 0 {
                TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
            if !kb.is_null() {
                UnhookWindowsHookEx(kb);
            }
            if !ms.is_null() {
                UnhookWindowsHookEx(ms);
            }
            RUNNING.store(false, Ordering::SeqCst);
            log::info!("touches à l'écran : arrêt");
        });
        if spawned.is_err() {
            RUNNING.store(false, Ordering::SeqCst);
        }
    } else if RUNNING.load(Ordering::SeqCst) {
        let id = THREAD.load(Ordering::SeqCst);
        if id != 0 {
            unsafe {
                PostThreadMessageW(id, WM_QUIT, 0, 0);
            }
        }
        EVENTS.lock().clear();
        *PRESSED.lock() = [false; 256];
    }
}

/// Événements entre deux heures, avec l'état des touches déjà enfoncées au début.
pub fn between(a: f64, b: f64) -> Vec<InputEvent> {
    let ev = EVENTS.lock();
    let mut held = [false; 256];
    let mut out = Vec::new();
    for e in ev.iter() {
        if e.t < a {
            held[e.key as usize] = e.down;
        } else if e.t <= b {
            out.push(*e);
        }
    }
    let mut first: Vec<InputEvent> = (0..256u16).filter(|k| held[*k as usize]).map(|k| InputEvent { t: a, key: k, down: true }).collect();
    first.extend(out);
    first
}

fn dir() -> std::path::PathBuf {
    let d = util::data_dir().join("inputs");
    let _ = std::fs::create_dir_all(&d);
    d
}

/// Garde les touches d'un clip (heures converties en position dans le clip).
pub fn save_for_clip(id: &str, clip_start: f64, clip_end: f64) {
    let list: Vec<InputEvent> = between(clip_start, clip_end)
        .into_iter()
        .map(|e| InputEvent { t: (e.t - clip_start).max(0.0), ..e })
        .collect();
    if list.is_empty() {
        return;
    }
    if let Ok(text) = serde_json::to_string(&list) {
        let _ = std::fs::write(dir().join(format!("{id}.json")), text);
    }
}

/// Écrit directement une liste de touches pour un clip (tests).
pub fn save_events(id: &str, list: &[InputEvent]) {
    if let Ok(text) = serde_json::to_string(list) {
        let _ = std::fs::write(dir().join(format!("{id}.json")), text);
    }
}

/// Touches enregistrées avec un clip (vide si l'option était coupée).
pub fn for_clip(id: &str) -> Vec<InputEvent> {
    std::fs::read_to_string(dir().join(format!("{id}.json")))
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default()
}

/// Test réel des crochets : `rewind.exe --keys`. Simule Maj et Ctrl (ne tapent rien nulle part).
pub fn self_test() -> i32 {
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_KEYUP, SendInput};
    GAME_FOCUSED.store(true, Ordering::Relaxed);
    set_enabled(true);
    std::thread::sleep(std::time::Duration::from_millis(400));
    let t0 = util::now();
    let key = |vk: u16, up: bool| INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 { ki: KEYBDINPUT { wVk: vk, wScan: 0, dwFlags: if up { KEYEVENTF_KEYUP } else { 0 }, time: 0, dwExtraInfo: 0 } },
    };
    for (vk, up) in [(0xA0, false), (0xA0, true), (0xA2, false), (0xA2, true)] {
        let i = [key(vk, up)];
        unsafe {
            SendInput(1, i.as_ptr(), std::mem::size_of::<INPUT>() as i32);
        }
        std::thread::sleep(std::time::Duration::from_millis(60));
    }
    std::thread::sleep(std::time::Duration::from_millis(200));
    let got = between(t0, util::now());
    set_enabled(false);
    GAME_FOCUSED.store(false, Ordering::Relaxed);
    println!("événements reçus : {got:?}");
    let ok = got.len() == 4 && got[0].key == 0xA0 && got[0].down && !got[1].down && got[2].key == 0xA2;
    println!("{}", if ok { "OK : crochets clavier fonctionnels" } else { "ÉCHEC" });
    if ok { 0 } else { 1 }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn etat_au_debut_et_repetitions() {
        GAME_FOCUSED.store(true, Ordering::Relaxed);
        EVENTS.lock().clear();
        *PRESSED.lock() = [false; 256];
        record(0x57, true); // W enfoncé avant le clip
        record(0x57, true); // répétition automatique : ignorée
        let a = util::now() + 0.001;
        std::thread::sleep(std::time::Duration::from_millis(5));
        record(0x01, true);
        record(0x01, false);
        record(0x4B, true); // K : hors liste, jamais noté
        let list = between(a, util::now() + 1.0);
        assert_eq!(list.len(), 3, "{list:?}");
        assert_eq!((list[0].key, list[0].down), (0x57, true));
        assert_eq!(EVENTS.lock().len(), 3);
        GAME_FOCUSED.store(false, Ordering::Relaxed);
        record(0x41, true); // pas de jeu au premier plan : rien
        assert_eq!(EVENTS.lock().len(), 3);
    }
}
