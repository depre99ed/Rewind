//! Clavier et souris lus par « Raw Input » : raccourcis globaux et touches à l'écran.
//!
//! Pourquoi Raw Input (2026-11-15) :
//! - les crochets bas niveau (WH_KEYBOARD_LL / WH_MOUSE_LL) étaient retirés EN SILENCE par Windows
//!   pendant les jeux (un crochet trop lent à répondre est supprimé sans prévenir ; le crochet souris
//!   passait chaque mouvement d'une souris 1000-8000 Hz) : les fichiers de touches ne contenaient qu'un clic ;
//! - RegisterHotKey avalait la touche (impossible de la saisir dans les Réglages) et F8 seul ne
//!   déclenchait rien chez l'utilisateur.
//! Raw Input reçoit une COPIE de chaque appui, même en arrière-plan : rien n'est bloqué, aucune latence
//! n'est ajoutée au jeu, et l'inscription ne peut pas être retirée (on la vérifie quand même toutes les 3 s).
//!
//! Vie privée : pour l'overlay, seules les touches de jeu d'une liste fixe sont notées (ZQSD/WASD,
//! espace, Maj, Ctrl, quelques lettres d'action, 1-5, boutons de la souris), seulement quand un jeu est
//! au premier plan, et tout reste en mémoire : rien n'est écrit sur le disque sauf avec un clip.
//! Les raccourcis ne gardent rien : on compare l'appui à la liste des raccourcis et on l'oublie.

use crate::util;
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use std::cell::RefCell;
use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicBool, AtomicIsize, AtomicU32, Ordering};
use std::sync::OnceLock;
use windows_sys::Win32::{
    Foundation::{HWND, LPARAM, LRESULT, WPARAM},
    System::{LibraryLoader::GetModuleHandleW, Threading::GetCurrentThreadId},
    UI::{
        Input::{
            GetRawInputData, GetRegisteredRawInputDevices, RegisterRawInputDevices, HRAWINPUT, RAWINPUT, RAWINPUTDEVICE, RAWINPUTHEADER,
            RIDEV_INPUTSINK, RID_INPUT, RIM_TYPEKEYBOARD, RIM_TYPEMOUSE,
            KeyboardAndMouse::{GetAsyncKeyState, GetKeyboardLayout, MapVirtualKeyExW, MAPVK_VK_TO_VSC_EX, MAPVK_VSC_TO_VK_EX},
        },
        WindowsAndMessaging::{
            CreateWindowExW, DefWindowProcW, DispatchMessageW, GetForegroundWindow, GetMessageW, GetWindowThreadProcessId, PostThreadMessageW,
            RegisterClassW, SetTimer, TranslateMessage, HWND_MESSAGE, MSG, RI_KEY_BREAK, RI_KEY_E0, RI_MOUSE_BUTTON_4_DOWN, RI_MOUSE_BUTTON_4_UP,
            RI_MOUSE_BUTTON_5_DOWN, RI_MOUSE_BUTTON_5_UP, RI_MOUSE_LEFT_BUTTON_DOWN, RI_MOUSE_LEFT_BUTTON_UP, RI_MOUSE_MIDDLE_BUTTON_DOWN,
            RI_MOUSE_MIDDLE_BUTTON_UP, RI_MOUSE_RIGHT_BUTTON_DOWN, RI_MOUSE_RIGHT_BUTTON_UP, WM_INPUT, WM_QUIT, WM_TIMER, WNDCLASSW,
        },
    },
};

// ---------------------------------------------------------------------------
// Touches à l'écran (overlay)
// ---------------------------------------------------------------------------

/// Un appui ou un relâchement. `key` = code de touche virtuelle de la disposition QWERTY américaine
/// (position physique : sur un clavier AZERTY, la touche Z est notée 0x57 « W ») ; 1 = clic gauche,
/// 2 = clic droit, 4 = molette, 5 / 6 = boutons latéraux.
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq)]
pub struct InputEvent {
    pub t: f64,
    pub key: u16,
    pub down: bool,
}

/// Touches notées : (code QWERTY, code de position physique « scan code », nom affiché par défaut).
pub const KEYS: &[(u16, u16, &str)] = &[
    (0x57, 0x11, "W"), (0x41, 0x1E, "A"), (0x53, 0x1F, "S"), (0x44, 0x20, "D"), (0x51, 0x10, "Q"), (0x45, 0x12, "E"),
    (0x52, 0x13, "R"), (0x46, 0x21, "F"), (0x47, 0x22, "G"), (0x43, 0x2E, "C"), (0x56, 0x2F, "V"), (0x20, 0x39, "Espace"),
    (0xA0, 0x2A, "Maj"), (0xA0, 0x36, "Maj"), (0xA2, 0x1D, "Ctrl"), (0xA2, 0xE01D, "Ctrl"), (0x09, 0x0F, "Tab"),
    (0x31, 0x02, "1"), (0x32, 0x03, "2"), (0x33, 0x04, "3"), (0x34, 0x05, "4"), (0x35, 0x06, "5"),
    // Superposition « complète » (2026-11-15)
    (0x54, 0x14, "T"), (0x5A, 0x2C, "Z"), (0x58, 0x2D, "X"), (0x42, 0x30, "B"), (0xA4, 0x38, "Alt"),
    (0x01, 0, "Clic G"), (0x02, 0, "Clic D"), (0x04, 0, "Molette"), (0x05, 0, "Souris 4"), (0x06, 0, "Souris 5"),
];

static EVENTS: Mutex<VecDeque<InputEvent>> = Mutex::new(VecDeque::new());
static PRESSED: Mutex<[bool; 256]> = Mutex::new([false; 256]);
/// Processus du jeu au premier plan (0 = aucun), tenu à jour par le fil de surveillance.
static GAME_PID: AtomicU32 = AtomicU32::new(0);
/// Tests : on note les touches même sans jeu au premier plan.
static FORCE_GAME: AtomicBool = AtomicBool::new(false);
/// Disposition du clavier du jeu (pour afficher Z Q S D sur un AZERTY).
static GAME_LAYOUT: AtomicIsize = AtomicIsize::new(0);
static OVERLAY: AtomicBool = AtomicBool::new(false);
const MAX_EVENTS: usize = 60_000;

/// Le fil de surveillance indique le jeu au premier plan (0 = aucun).
pub fn set_game(pid: u32) {
    GAME_PID.store(pid, Ordering::Relaxed);
}

/// Active ou coupe la prise des touches pour l'overlay (les raccourcis, eux, marchent toujours).
pub fn set_overlay(on: bool) {
    let was = OVERLAY.swap(on, Ordering::SeqCst);
    if was && !on {
        EVENTS.lock().clear();
        *PRESSED.lock() = [false; 256];
    }
    if on != was {
        log::info!("touches à l'écran : {}", if on { "enregistrement actif" } else { "arrêt" });
    }
}

/// Le jeu est-il VRAIMENT au premier plan à cet instant (pas il y a 500 ms) ?
fn game_in_front() -> bool {
    if FORCE_GAME.load(Ordering::Relaxed) {
        return true;
    }
    let game = GAME_PID.load(Ordering::Relaxed);
    if game == 0 {
        return false;
    }
    let mut pid = 0u32;
    unsafe {
        let hwnd = GetForegroundWindow();
        if hwnd.is_null() {
            return false;
        }
        let thread = GetWindowThreadProcessId(hwnd, &mut pid);
        if pid == game {
            GAME_LAYOUT.store(GetKeyboardLayout(thread) as isize, Ordering::Relaxed);
        }
    }
    pid == game
}

fn record(key: u16, down: bool) {
    if !OVERLAY.load(Ordering::Relaxed) || !game_in_front() {
        return;
    }
    {
        // Les répétitions automatiques (touche maintenue) ne sont pas des appuis
        let mut p = PRESSED.lock();
        if p[key as usize] == down {
            return;
        }
        p[key as usize] = down;
    }
    let mut ev = EVENTS.lock();
    ev.push_back(InputEvent { t: util::now(), key, down });
    while ev.len() > MAX_EVENTS {
        ev.pop_front();
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

/// Garde les touches d'un clip (heures converties en position dans le clip). `spans` : morceaux
/// d'enregistrement recollés dans le clip (heure de début, heure de fin), dans l'ordre. Une touche
/// tombée dans un trou entre deux morceaux (capture relancée) est écartée, les suivantes sont recalées
/// comme les repères (audit 0.4.12 : elles étaient en retard après le premier trou).
pub fn save_for_clip(id: &str, spans: &[(f64, f64)]) {
    let (Some(first), Some(last)) = (spans.first(), spans.last()) else { return };
    let mut list: Vec<InputEvent> = Vec::new();
    for e in between(first.0, last.1) {
        let mut offset = 0.0;
        for (a, b) in spans {
            if e.t >= *a && e.t <= *b {
                list.push(InputEvent { t: offset + (e.t - a).max(0.0), ..e });
                break;
            }
            offset += (b - a).max(0.0);
        }
    }
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

/// Nom de chaque touche de l'overlay dans la disposition du clavier (Z au lieu de W sur un AZERTY).
/// Seules les lettres changent ; le reste garde son nom par défaut (traduit par l'interface).
pub fn key_labels() -> HashMap<u16, String> {
    let mut layout = GAME_LAYOUT.load(Ordering::Relaxed);
    if layout == 0 {
        layout = unsafe { GetKeyboardLayout(0) } as isize;
    }
    let mut out = HashMap::new();
    for (qwerty, sc, _) in KEYS {
        if !(0x41..=0x5A).contains(qwerty) {
            continue;
        }
        let vk = unsafe { MapVirtualKeyExW(*sc as u32, MAPVK_VSC_TO_VK_EX, layout as _) };
        if (0x41..=0x5A).contains(&vk) {
            out.insert(*qwerty, char::from_u32(vk).unwrap_or('?').to_string());
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Raccourcis
// ---------------------------------------------------------------------------

const CTRL: u8 = 1;
const SHIFT: u8 = 2;
const ALT: u8 = 4;
const WIN: u8 = 8;

/// Touche principale d'un raccourci : position physique du clavier (scan code, 0xE0xx pour les
/// touches étendues) ou bouton de souris (3 = molette, 4 et 5 = boutons latéraux).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Main {
    Key(u16),
    Mouse(u8),
}

#[derive(Clone, Debug)]
struct Binding {
    mods: u8,
    main: Main,
    action: &'static str,
    text: String,
}

static BINDINGS: Mutex<Vec<Binding>> = Mutex::new(Vec::new());
/// Change à chaque nouvelle liste de raccourcis (le secours en jeu recalcule alors ses touches)
static BINDINGS_GEN: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
static HANDLER: OnceLock<Box<dyn Fn(&'static str) + Send + Sync>> = OnceLock::new();
/// Pendant la saisie d'un raccourci dans les Réglages : rien ne se déclenche (heure de fin).
static PAUSED_UNTIL: Mutex<f64> = Mutex::new(0.0);
static LAST_FIRE: Mutex<Vec<(&'static str, f64)>> = Mutex::new(Vec::new());

/// Noms des touches (format des Réglages : « Ctrl+Alt+F10 », « Mouse4 ») → position physique.
fn key_code(name: &str) -> Option<Main> {
    let n = name.trim();
    let lower = n.to_ascii_lowercase();
    match lower.as_str() {
        "mouse4" | "mousex1" | "xbutton1" => return Some(Main::Mouse(4)),
        "mouse5" | "mousex2" | "xbutton2" => return Some(Main::Mouse(5)),
        "mousemiddle" | "mouse3" | "middleclick" => return Some(Main::Mouse(3)),
        _ => {}
    }
    // F1-F24
    if let Some(num) = lower.strip_prefix('f').and_then(|x| x.parse::<u16>().ok()) {
        let sc = match num {
            1..=10 => 0x3A + num,
            11 => 0x57,
            12 => 0x58,
            13..=23 => 0x64 + (num - 13),
            24 => 0x76,
            _ => return None,
        };
        return Some(Main::Key(sc));
    }
    const LETTERS: [(char, u16); 26] = [
        ('A', 0x1E), ('B', 0x30), ('C', 0x2E), ('D', 0x20), ('E', 0x12), ('F', 0x21), ('G', 0x22), ('H', 0x23), ('I', 0x17),
        ('J', 0x24), ('K', 0x25), ('L', 0x26), ('M', 0x32), ('N', 0x31), ('O', 0x18), ('P', 0x19), ('Q', 0x10), ('R', 0x13),
        ('S', 0x1F), ('T', 0x14), ('U', 0x16), ('V', 0x2F), ('W', 0x11), ('X', 0x2D), ('Y', 0x15), ('Z', 0x2C),
    ];
    if n.chars().count() == 1 {
        let c = n.chars().next()?.to_ascii_uppercase();
        if let Some((_, sc)) = LETTERS.iter().find(|(l, _)| *l == c) {
            return Some(Main::Key(*sc));
        }
        if let Some(d) = c.to_digit(10) {
            return Some(Main::Key(if d == 0 { 0x0B } else { 0x01 + d as u16 }));
        }
    }
    let sc = match lower.as_str() {
        "space" => 0x39,
        "enter" | "return" => 0x1C,
        "tab" => 0x0F,
        "escape" | "esc" => 0x01,
        "backspace" => 0x0E,
        "capslock" => 0x3A,
        "insert" => 0xE052,
        "delete" => 0xE053,
        "home" => 0xE047,
        "end" => 0xE04F,
        "pageup" => 0xE049,
        "pagedown" => 0xE051,
        "up" | "arrowup" => 0xE048,
        "down" | "arrowdown" => 0xE050,
        "left" | "arrowleft" => 0xE04B,
        "right" | "arrowright" => 0xE04D,
        "printscreen" => 0xE037,
        "pause" => 0xE11D,
        "scrolllock" => 0x46,
        "numlock" => 0x45,
        "num0" | "numpad0" => 0x52,
        "num1" | "numpad1" => 0x4F,
        "num2" | "numpad2" => 0x50,
        "num3" | "numpad3" => 0x51,
        "num4" | "numpad4" => 0x4B,
        "num5" | "numpad5" => 0x4C,
        "num6" | "numpad6" => 0x4D,
        "num7" | "numpad7" => 0x47,
        "num8" | "numpad8" => 0x48,
        "num9" | "numpad9" => 0x49,
        "numadd" | "numpadadd" => 0x4E,
        "numsubtract" | "numpadsubtract" => 0x4A,
        "nummultiply" | "numpadmultiply" => 0x37,
        "numdivide" | "numpaddivide" => 0xE035,
        "numdecimal" | "numpaddecimal" => 0x53,
        "numenter" | "numpadenter" => 0xE01C,
        "backquote" => 0x29,
        "minus" => 0x0C,
        "equal" => 0x0D,
        "bracketleft" => 0x1A,
        "bracketright" => 0x1B,
        "backslash" => 0x2B,
        "semicolon" => 0x27,
        "quote" => 0x28,
        "comma" => 0x33,
        "period" => 0x34,
        "slash" => 0x35,
        "intlbackslash" => 0x56,
        _ => {
            // « KeyA », « Digit1 » (anciens formats)
            if let Some(l) = n.strip_prefix("Key") {
                return key_code(l);
            }
            if let Some(d) = n.strip_prefix("Digit") {
                return key_code(d);
            }
            return None;
        }
    };
    Some(Main::Key(sc))
}

/// « Ctrl+Shift+F10 » → (modificateurs, touche). None si illisible.
pub fn parse(text: &str) -> Option<(u8, Main)> {
    let mut mods = 0u8;
    let mut main = None;
    for part in text.split('+').map(str::trim).filter(|p| !p.is_empty()) {
        match part.to_ascii_lowercase().as_str() {
            "ctrl" | "control" | "commandorcontrol" | "cmdorctrl" => mods |= CTRL,
            "shift" | "maj" => mods |= SHIFT,
            "alt" | "option" => mods |= ALT,
            "win" | "super" | "meta" | "cmd" | "command" => mods |= WIN,
            _ => {
                if main.is_some() {
                    return None; // deux touches principales
                }
                main = Some(key_code(part)?);
            }
        }
    }
    main.map(|m| (mods, m))
}

/// Remplace la liste des raccourcis. Renvoie les erreurs (raccourci illisible ou en double).
pub fn set_bindings(list: &[(String, &'static str)]) -> Vec<String> {
    let mut out: Vec<Binding> = Vec::new();
    let mut errors = Vec::new();
    for (text, action) in list {
        if text.trim().is_empty() {
            continue;
        }
        match parse(text) {
            Some((mods, main)) => {
                if let Some(other) = out.iter().find(|b| b.mods == mods && b.main == main) {
                    errors.push(format!("{text} : déjà utilisé ({})", other.action));
                    continue;
                }
                out.push(Binding { mods, main, action, text: text.clone() });
            }
            None => errors.push(format!("{text} : touche inconnue")),
        }
    }
    *BINDINGS.lock() = out;
    BINDINGS_GEN.fetch_add(1, Ordering::Relaxed);
    errors
}

/// Ce qu'il faut faire quand un raccourci est pressé (appelé dans le fil des entrées : rendre la main vite).
pub fn on_hotkey(f: impl Fn(&'static str) + Send + Sync + 'static) {
    let _ = HANDLER.set(Box::new(f));
}

/// Pendant la saisie d'un raccourci dans les Réglages (sécurité : se rouvre seul après 60 s).
pub fn pause_hotkeys(on: bool) {
    *PAUSED_UNTIL.lock() = if on { util::now() + 60.0 } else { 0.0 };
}

fn current_mods() -> u8 {
    let down = |vk: i32| unsafe { GetAsyncKeyState(vk) } < 0;
    let mut m = 0;
    if down(0x11) {
        m |= CTRL;
    }
    if down(0x10) {
        m |= SHIFT;
    }
    if down(0x12) {
        m |= ALT;
    }
    if down(0x5B) || down(0x5C) {
        m |= WIN;
    }
    m
}

/// Choix du raccourci : même touche, et ses modificateurs tous tenus. Des touches EN PLUS
/// (Ctrl pour s'accroupir, Maj pour marcher) n'empêchent pas « F8 » ; si « F8 » et « Alt+F8 » existent,
/// Alt+F8 l'emporte quand Alt est tenu (le plus précis gagne).
fn pick(bindings: &[Binding], main: Main, mods: u8) -> Option<(&'static str, String)> {
    bindings
        .iter()
        .filter(|b| b.main == main && b.mods & !mods == 0)
        .max_by_key(|b| b.mods.count_ones())
        .map(|b| (b.action, b.text.clone()))
}

fn press(main: Main) {
    let now = util::now();
    if now < *PAUSED_UNTIL.lock() {
        return;
    }
    let Some((action, text)) = pick(&BINDINGS.lock(), main, current_mods()) else { return };
    {
        // Anti-rebond : un appui = une action
        let mut last = LAST_FIRE.lock();
        if last.iter().any(|(a, t)| *a == action && now - t < 0.3) {
            return;
        }
        last.retain(|(a, _)| *a != action);
        last.push((action, now));
    }
    log::info!("raccourci {text} → {action}");
    if let Some(h) = HANDLER.get() {
        h(action);
    }
}

// ---------------------------------------------------------------------------
// Fil des entrées (fenêtre invisible + Raw Input)
// ---------------------------------------------------------------------------

#[derive(Default)]
struct ThreadState {
    /// Touches tenues (scan code → heure du dernier appui) pour reconnaître les répétitions automatiques
    held: HashMap<u16, f64>,
    warned: bool,
}

thread_local! {
    static ST: RefCell<ThreadState> = RefCell::new(ThreadState::default());
}

static RUNNING: AtomicBool = AtomicBool::new(false);
static THREAD: AtomicU32 = AtomicU32::new(0);
/// Vrai dès que la fenêtre reçoit des entrées (rapport de diagnostic).
pub static ALIVE: AtomicBool = AtomicBool::new(false);
/// Appuis reçus par Raw Input pendant qu'un jeu est au premier plan (diagnostic).
static RAW_IN_GAME: AtomicU32 = AtomicU32::new(0);

fn devices(hwnd: HWND) -> [RAWINPUTDEVICE; 2] {
    [
        RAWINPUTDEVICE { usUsagePage: 0x01, usUsage: 0x06, dwFlags: RIDEV_INPUTSINK, hwndTarget: hwnd }, // clavier
        RAWINPUTDEVICE { usUsagePage: 0x01, usUsage: 0x02, dwFlags: RIDEV_INPUTSINK, hwndTarget: hwnd }, // souris
    ]
}

fn register(hwnd: HWND) -> bool {
    let list = devices(hwnd);
    unsafe { RegisterRawInputDevices(list.as_ptr(), list.len() as u32, std::mem::size_of::<RAWINPUTDEVICE>() as u32) != 0 }
}

/// L'inscription est faite pour tout le processus : si une autre partie de l'appli (la fenêtre
/// principale) s'inscrit après nous, on la reprend.
fn still_registered(hwnd: HWND) -> bool {
    let mut list: [RAWINPUTDEVICE; 8] = unsafe { std::mem::zeroed() };
    let mut n = list.len() as u32;
    let got = unsafe { GetRegisteredRawInputDevices(list.as_mut_ptr(), &mut n, std::mem::size_of::<RAWINPUTDEVICE>() as u32) };
    if got == u32::MAX {
        return false;
    }
    let ok = |usage: u16| list[..got as usize].iter().any(|d| d.usUsagePage == 1 && d.usUsage == usage && d.hwndTarget == hwnd && d.dwFlags & RIDEV_INPUTSINK != 0);
    ok(0x06) && ok(0x02)
}

fn on_keyboard(make: u16, flags: u32, vkey: u16) {
    if vkey == 0xFF {
        return; // fausses touches (Maj fantôme des touches étendues)
    }
    if forward([1, make as u8, (make >> 8) as u8, flags as u8, (flags >> 8) as u8, vkey as u8, (vkey >> 8) as u8]) {
        return;
    }
    let up = flags & RI_KEY_BREAK != 0;
    let mut sc = make;
    if sc == 0 {
        sc = unsafe { MapVirtualKeyExW(vkey as u32, MAPVK_VK_TO_VSC_EX, std::ptr::null_mut()) } as u16 & 0xFF;
    }
    let code = match vkey {
        0x13 => 0xE11D,                   // Pause
        0x2C => 0xE037,                   // Impr. écran
        0x90 => 0x45,                     // Verr. num
        _ if flags & RI_KEY_E0 != 0 => 0xE000 | (sc & 0xFF),
        _ => sc & 0xFF,
    };
    let now = util::now();
    let repeat = ST.with(|s| {
        let mut s = s.borrow_mut();
        if up {
            s.held.remove(&code);
            false
        } else {
            // Une répétition arrive toutes les ~33 ms tant que la touche est tenue ; si la touche
            // était notée tenue depuis longtemps sans répétition, son relâchement a été manqué.
            let rep = s.held.get(&code).is_some_and(|t| now - t < 0.75);
            s.held.insert(code, now);
            rep
        }
    });
    if let Some((qwerty, _, _)) = KEYS.iter().find(|(_, s, _)| *s != 0 && *s == code) {
        record(*qwerty, !up);
    }
    if !up && !repeat && GAME_PID.load(Ordering::Relaxed) != 0 && game_in_front() {
        RAW_IN_GAME.fetch_add(1, Ordering::Relaxed);
    }
    if !up && !repeat {
        press(Main::Key(code));
    }
}

fn on_mouse(buttons: u32) {
    if buttons == 0 {
        return; // simple mouvement : rien à faire
    }
    if forward([2, buttons as u8, (buttons >> 8) as u8, 0, 0, 0, 0]) {
        return;
    }
    let pairs: [(u32, u32, u16, Option<u8>); 5] = [
        (RI_MOUSE_LEFT_BUTTON_DOWN, RI_MOUSE_LEFT_BUTTON_UP, 0x01, None),
        (RI_MOUSE_RIGHT_BUTTON_DOWN, RI_MOUSE_RIGHT_BUTTON_UP, 0x02, None),
        (RI_MOUSE_MIDDLE_BUTTON_DOWN, RI_MOUSE_MIDDLE_BUTTON_UP, 0x04, Some(3)),
        (RI_MOUSE_BUTTON_4_DOWN, RI_MOUSE_BUTTON_4_UP, 0x05, Some(4)),
        (RI_MOUSE_BUTTON_5_DOWN, RI_MOUSE_BUTTON_5_UP, 0x06, Some(5)),
    ];
    for (down, up, key, hotkey) in pairs {
        if buttons & down != 0 {
            record(key, true);
            if let Some(b) = hotkey {
                press(Main::Mouse(b));
            }
        }
        if buttons & up != 0 {
            record(key, false);
        }
    }
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        WM_INPUT => {
            let mut raw: RAWINPUT = unsafe { std::mem::zeroed() };
            let mut size = std::mem::size_of::<RAWINPUT>() as u32;
            let got = unsafe {
                GetRawInputData(lparam as HRAWINPUT, RID_INPUT, &mut raw as *mut RAWINPUT as *mut _, &mut size, std::mem::size_of::<RAWINPUTHEADER>() as u32)
            };
            if got != u32::MAX && got > 0 {
                ALIVE.store(true, Ordering::Relaxed);
                if raw.header.dwType == RIM_TYPEKEYBOARD {
                    let k = unsafe { raw.data.keyboard };
                    if WATCH.load(Ordering::Relaxed) {
                        watch_note(raw.header.hDevice, &k);
                    }
                    on_keyboard(k.MakeCode, k.Flags as u32, k.VKey);
                } else if raw.header.dwType == RIM_TYPEMOUSE {
                    let b = unsafe { raw.data.mouse.Anonymous.Anonymous.usButtonFlags } as u32;
                    on_mouse(b);
                }
            }
            // Obligatoire : Windows libère les données d'entrée dans DefWindowProc
            unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
        }
        WM_TIMER => {
            ST.with(|s| {
                let mut s = s.borrow_mut();
                if !still_registered(hwnd) {
                    let ok = register(hwnd);
                    if !s.warned {
                        s.warned = true;
                        log::warn!("entrées : inscription reprise ({})", if ok { "ok" } else { "échec" });
                    }
                }
                // Touches notées tenues depuis plus de 30 s sans répétition : relâchement manqué
                let now = util::now();
                s.held.retain(|_, t| now - *t < 30.0);
            });
            0
        }
        _ => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
    }
}

/// Démarre le fil des entrées (une seule fois) : raccourcis toujours, overlay selon l'option.
pub fn start() {
    if RUNNING.swap(true, Ordering::SeqCst) {
        return;
    }
    let spawned = std::thread::Builder::new().name("entrees".into()).spawn(|| unsafe {
        THREAD.store(GetCurrentThreadId(), Ordering::SeqCst);
        let module = GetModuleHandleW(std::ptr::null());
        let class: Vec<u16> = "RewindEntrees\0".encode_utf16().collect();
        let wc = WNDCLASSW { lpfnWndProc: Some(wndproc), hInstance: module, lpszClassName: class.as_ptr(), ..std::mem::zeroed() };
        RegisterClassW(&wc);
        let hwnd = CreateWindowExW(0, class.as_ptr(), std::ptr::null(), 0, 0, 0, 0, 0, HWND_MESSAGE, std::ptr::null_mut(), module, std::ptr::null());
        if hwnd.is_null() {
            log::warn!("entrées : fenêtre impossible, raccourcis et touches à l'écran indisponibles");
            RUNNING.store(false, Ordering::SeqCst);
            return;
        }
        if register(hwnd) {
            log::info!("entrées : raccourcis et touches prêts (Raw Input)");
        } else {
            log::warn!("entrées : inscription Raw Input refusée par Windows");
        }
        SetTimer(hwnd, 1, 3000, None);
        let mut msg: MSG = std::mem::zeroed();
        while GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) > 0 {
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
        RUNNING.store(false, Ordering::SeqCst);
    });
    if spawned.is_err() {
        RUNNING.store(false, Ordering::SeqCst);
        return;
    }
    let _ = std::thread::Builder::new().name("entrees-jeu".into()).spawn(poll_loop);
}

// ---------------------------------------------------------------------------
// Jeux lancés en administrateur (2026-11-15)
// ---------------------------------------------------------------------------
// Windows (UIPI) cache les touches d'une fenêtre « administrateur » à tout programme qui ne l'est pas :
// Raw Input, crochets et GetAsyncKeyState restent muets. Quand Steam (donc CS2) tourne en administrateur,
// F8 ne marchait qu'en dehors du jeu. Solution : un petit assistant `rewind.exe --input-helper`, lancé en
// administrateur par une tâche planifiée (une seule invite Windows, à l'activation), lit les touches avec
// le même moteur et les envoie au Rewind principal par une connexion locale (127.0.0.1 + jeton).
// Le principal ne garde ses envois que si la fenêtre du premier plan est « administrateur » (pas de doublon).

/// Mode assistant : les touches partent vers le Rewind principal au lieu d'être traitées ici.
static FORWARD: Mutex<Option<std::net::TcpStream>> = Mutex::new(None);
/// Rewind principal : un assistant est connecté.
pub static HELPER_ON: AtomicBool = AtomicBool::new(false);
/// Premier plan « administrateur » (cache : pid → élevé).
static FG_ELEVATED: Mutex<(u32, bool)> = Mutex::new((0, false));

fn forward(msg: [u8; 7]) -> bool {
    use std::io::Write;
    let mut f = FORWARD.lock();
    let Some(s) = f.as_mut() else { return false };
    if s.write_all(&msg).is_err() {
        // Rewind principal fermé : l'assistant s'arrête
        std::process::exit(0);
    }
    true
}

/// La fenêtre au premier plan appartient-elle à un programme lancé en administrateur ?
pub fn foreground_elevated() -> bool {
    let mut pid = 0u32;
    unsafe {
        let hwnd = GetForegroundWindow();
        if hwnd.is_null() {
            return false;
        }
        GetWindowThreadProcessId(hwnd, &mut pid);
    }
    let mut c = FG_ELEVATED.lock();
    if c.0 != pid {
        *c = (pid, util::process_elevated(pid).unwrap_or(false));
    }
    c.1
}

/// Le jeu suivi tourne-t-il en administrateur alors que Rewind non ? (touches invisibles sans l'assistant)
pub fn game_needs_helper() -> bool {
    let game = GAME_PID.load(Ordering::Relaxed);
    game != 0 && util::process_elevated(game) == Some(true) && util::process_elevated(0) == Some(false)
}

fn helper_file() -> std::path::PathBuf {
    // Les tests ne touchent pas au dossier de données (il est mémorisé au premier appel : le test du
    // rapport en choisit un autre)
    if cfg!(test) {
        return std::env::temp_dir().join(format!("rewind-test-entrees-{}.json", std::process::id()));
    }
    util::data_dir().join("entrees-admin.json")
}

#[derive(Serialize, Deserialize)]
struct HelperInfo {
    port: u16,
    token: String,
}

/// Rewind principal : attend l'assistant sur 127.0.0.1 (port libre + jeton écrits dans le dossier de données).
pub fn serve_helper() {
    let Ok(listener) = std::net::TcpListener::bind("127.0.0.1:0") else { return };
    let Ok(addr) = listener.local_addr() else { return };
    let token = format!("{:016x}{:016x}", rand_u64(), rand_u64());
    let info = HelperInfo { port: addr.port(), token: token.clone() };
    if std::fs::write(helper_file(), serde_json::to_string(&info).unwrap_or_default()).is_err() {
        return;
    }
    let _ = std::thread::Builder::new().name("entrees-admin".into()).spawn(move || {
        for stream in listener.incoming().flatten() {
            let token = token.clone();
            let _ = std::thread::Builder::new().name("entrees-admin-lien".into()).spawn(move || helper_link(stream, &token));
        }
    });
}

fn rand_u64() -> u64 {
    use std::hash::{BuildHasher, Hasher};
    let mut h = std::collections::hash_map::RandomState::new().build_hasher();
    h.write_u64((util::now() * 1e9) as u64);
    h.finish()
}

fn helper_link(mut s: std::net::TcpStream, token: &str) {
    use std::io::Read;
    let mut got = vec![0u8; token.len()];
    if s.read_exact(&mut got).is_err() || got != token.as_bytes() {
        return; // pas notre assistant
    }
    HELPER_ON.store(true, Ordering::SeqCst);
    log::info!("entrées : assistant administrateur connecté (touches des jeux lancés en administrateur)");
    let mut msg = [0u8; 7];
    while s.read_exact(&mut msg).is_ok() {
        // Seulement quand une fenêtre administrateur est devant : sinon Rewind reçoit déjà tout lui-même
        if !cfg!(test) && !foreground_elevated() {
            continue;
        }
        let w = |i: usize| msg[i] as u16 | (msg[i + 1] as u16) << 8;
        match msg[0] {
            1 => on_keyboard(w(1), w(3) as u32, w(5)),
            2 => on_mouse(w(1) as u32),
            _ => {}
        }
    }
    HELPER_ON.store(false, Ordering::SeqCst);
    log::warn!("entrées : assistant administrateur déconnecté");
}

/// Assistant (`rewind.exe --input-helper`, lancé en administrateur) : se connecte au Rewind principal,
/// lit le clavier et la souris et lui envoie tout ; s'arrête quand Rewind se ferme.
pub fn helper_main() -> i32 {
    use std::io::{Read, Write};
    // Rewind vient peut-être de démarrer : on attend son adresse jusqu'à 30 s
    let mut stream = None;
    for _ in 0..60 {
        if let Some(info) = std::fs::read_to_string(helper_file()).ok().and_then(|t| serde_json::from_str::<HelperInfo>(&t).ok())
            && let Ok(mut s) = std::net::TcpStream::connect(("127.0.0.1", info.port))
            && s.write_all(info.token.as_bytes()).is_ok()
        {
            let _ = s.set_nodelay(true);
            stream = Some(s);
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(500));
    }
    let Some(s) = stream else { return 1 };
    let Ok(watch) = s.try_clone() else { return 1 };
    *FORWARD.lock() = Some(s);
    start();
    // Bloque jusqu'à la fermeture de la connexion par Rewind
    let mut watch = watch;
    let mut buf = [0u8; 16];
    while matches!(watch.read(&mut buf), Ok(n) if n > 0) {}
    0
}

/// Touche Windows (VK) d'une position physique, pour lire son état réel.
fn vk_of(main: Main, layout: isize) -> i32 {
    match main {
        Main::Mouse(3) => 0x04,
        Main::Mouse(4) => 0x05,
        Main::Mouse(5) => 0x06,
        Main::Mouse(_) => 0,
        Main::Key(0xE11D) => 0x13,
        Main::Key(0xE037) => 0x2C,
        Main::Key(0x45) => 0x90,
        Main::Key(sc) => unsafe { MapVirtualKeyExW(sc as u32, MAPVK_VSC_TO_VK_EX, layout as _) as i32 },
    }
}

/// Secours pendant le jeu : certains jeux (CS2) empêchent Windows de transmettre les touches aux autres
/// programmes (Raw Input muet : F8 sans effet en partie, touches à l'écran vides, alors que tout marche
/// sur le bureau). Quand un jeu est au premier plan, on lit donc l'état réel des touches toutes les 8 ms
/// (GetAsyncKeyState). Les doublons avec Raw Input sont filtrés (anti-rebond des raccourcis, état tenu).
fn poll_loop() {
    // 2026-11-24 : 125 tours par seconde en jeu. Les codes des touches (MapVirtualKeyExW) et la liste
    // des raccourcis ne sont plus recalculés à chaque tour, et plus aucune allocation par tour :
    // seulement quand la disposition du clavier ou les raccourcis changent.
    let mut hot_prev: HashMap<i32, bool> = HashMap::new();
    let mut key_prev: HashMap<u16, bool> = HashMap::new();
    let mut hot_vks: Vec<(Main, i32)> = Vec::new();
    let mut key_vks: Vec<(u16, i32)> = Vec::new();
    let mut state: Vec<(u16, bool)> = Vec::with_capacity(KEYS.len());
    let mut cached: Option<(isize, u64)> = None;
    let mut noted = false;
    // Diagnostic : après 2 min de jeu au premier plan, une ligne dit si des touches ont été vues
    // (lues directement / reçues par Raw Input) : le journal dira où ça bloque chez l'utilisateur.
    let (mut front_ms, mut seen_poll, mut reported) = (0u64, 0u32, false);
    loop {
        let active = RUNNING.load(Ordering::Relaxed) && game_in_front();
        std::thread::sleep(std::time::Duration::from_millis(if active { 8 } else { 250 }));
        if active && !reported {
            front_ms += 8;
            if front_ms >= 120_000 {
                reported = true;
                log::info!(
                    "entrées pendant le jeu : {seen_poll} appuis lus directement, {} reçus par Raw Input",
                    RAW_IN_GAME.load(Ordering::Relaxed)
                );
            }
        }
        if !active {
            hot_prev.clear();
            key_prev.clear();
            continue;
        }
        let layout = GAME_LAYOUT.load(Ordering::Relaxed);
        let generation = BINDINGS_GEN.load(Ordering::Relaxed);
        if cached != Some((layout, generation)) {
            cached = Some((layout, generation));
            hot_vks = BINDINGS.lock().iter().map(|b| (b.main, vk_of(b.main, layout))).collect();
            key_vks = KEYS.iter().map(|(qwerty, sc, _)| (*qwerty, if *sc == 0 { *qwerty as i32 } else { vk_of(Main::Key(*sc), layout) })).collect();
        }
        let down = |vk: i32| vk != 0 && unsafe { GetAsyncKeyState(vk) } < 0;
        // Raccourcis : un passage « relâché → appuyé » = un appui
        for &(main, vk) in &hot_vks {
            let now_down = down(vk);
            let was = hot_prev.insert(vk, now_down).unwrap_or(true);
            if now_down && !was {
                seen_poll += 1;
                if !noted {
                    noted = true;
                    log::info!("entrées : touches lues directement pendant le jeu (secours actif)");
                }
                press(main);
            }
        }
        // Touches à l'écran (Maj gauche / droite = une seule touche)
        if OVERLAY.load(Ordering::Relaxed) {
            state.clear();
            for &(k, vk) in &key_vks {
                let d = down(vk);
                match state.iter_mut().find(|e| e.0 == k) {
                    Some(e) => e.1 |= d,
                    None => state.push((k, d)),
                }
            }
            for &(k, d) in &state {
                let before = key_prev.insert(k, d);
                if before.is_some() && before != Some(d) && d {
                    seen_poll += 1;
                }
                if before != Some(d) {
                    record(k, d);
                }
            }
        }
    }
}

/// Diagnostic : `rewind.exe --watch-keys <secondes>` écrit chaque touche reçue avec l'appareil qui
/// l'envoie (pour trouver une touche « fantôme » tenue en permanence, ex. Ctrl bloqué par un logiciel).
pub fn watch(seconds: u64) -> i32 {
    WATCH.store(true, Ordering::SeqCst);
    start();
    std::thread::sleep(std::time::Duration::from_secs(seconds));
    WATCH.store(false, Ordering::SeqCst);
    let seen = WATCHED.lock().clone();
    for (dev, list) in seen {
        println!("appareil : {dev}");
        for l in list.iter().take(40) {
            println!("   {l}");
        }
        if list.len() > 40 {
            println!("   … {} de plus", list.len() - 40);
        }
    }
    println!("état actuel : Ctrl={} Maj={} Alt={}", unsafe { GetAsyncKeyState(0x11) } < 0, unsafe { GetAsyncKeyState(0x10) } < 0, unsafe { GetAsyncKeyState(0x12) } < 0);
    stop();
    0
}

static WATCH: AtomicBool = AtomicBool::new(false);
static WATCHED: Mutex<Vec<(String, Vec<String>)>> = Mutex::new(Vec::new());

fn device_name(dev: windows_sys::Win32::Foundation::HANDLE) -> String {
    use windows_sys::Win32::UI::Input::{GetRawInputDeviceInfoW, RIDI_DEVICENAME};
    if dev.is_null() {
        return "aucun (touche simulée par un logiciel)".into();
    }
    let mut buf = [0u16; 512];
    let mut n = buf.len() as u32;
    let got = unsafe { GetRawInputDeviceInfoW(dev, RIDI_DEVICENAME, buf.as_mut_ptr() as *mut _, &mut n) };
    if got == u32::MAX || got == 0 {
        return format!("{dev:?}");
    }
    String::from_utf16_lossy(&buf[..got as usize]).trim_end_matches('\0').to_string()
}

fn watch_note(dev: windows_sys::Win32::Foundation::HANDLE, k: &windows_sys::Win32::UI::Input::RAWKEYBOARD) {
    let name = device_name(dev);
    let line = format!(
        "{:.3} vk=0x{:02X} scan=0x{:02X} {}{} extra=0x{:X}",
        util::now() % 100.0,
        k.VKey,
        k.MakeCode,
        if k.Flags as u32 & RI_KEY_BREAK != 0 { "relâchée" } else { "appuyée" },
        if k.Flags as u32 & RI_KEY_E0 != 0 { " (E0)" } else { "" },
        k.ExtraInformation
    );
    let mut w = WATCHED.lock();
    match w.iter_mut().find(|(d, _)| *d == name) {
        Some((_, l)) => l.push(line),
        None => w.push((name, vec![line])),
    }
}

/// Arrêt du fil (tests).
fn stop() {
    let id = THREAD.load(Ordering::SeqCst);
    if id != 0 {
        unsafe {
            PostThreadMessageW(id, WM_QUIT, 0, 0);
        }
    }
}

/// Test réel : `rewind.exe --keys`. Simule Maj, Ctrl et F14 (touche que personne n'utilise : ne
/// déclenche rien dans les autres applis ni dans le Rewind installé).
pub fn self_test() -> i32 {
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_KEYUP, SendInput};
    FORCE_GAME.store(true, Ordering::Relaxed);
    set_overlay(true);
    let fired: &'static Mutex<Vec<&'static str>> = Box::leak(Box::new(Mutex::new(Vec::new())));
    on_hotkey(move |a| fired.lock().push(a));
    let errors = set_bindings(&[("F14".into(), "clip"), ("Shift+F14".into(), "bookmark"), ("Mouse4".into(), "session")]);
    println!("raccourcis : {}", if errors.is_empty() { "ok".to_string() } else { format!("{errors:?}") });
    start();
    std::thread::sleep(std::time::Duration::from_millis(500));
    let t0 = util::now();
    let key = |vk: u16, up: bool| INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 { ki: KEYBDINPUT { wVk: vk, wScan: 0, dwFlags: if up { KEYEVENTF_KEYUP } else { 0 }, time: 0, dwExtraInfo: 0 } },
    };
    let send = |vk: u16, up: bool| unsafe {
        let i = [key(vk, up)];
        SendInput(1, i.as_ptr(), std::mem::size_of::<INPUT>() as i32);
        std::thread::sleep(std::time::Duration::from_millis(60));
    };
    // 1) overlay : Maj puis Ctrl
    for (vk, up) in [(0xA0, false), (0xA0, true), (0xA2, false), (0xA2, true)] {
        send(vk, up);
    }
    // 2) F14 seul → clip ; 3) Ctrl tenu + F14 → clip quand même ; 4) Maj + F14 → repère (plus précis)
    send(0x7D, false);
    send(0x7D, true);
    std::thread::sleep(std::time::Duration::from_millis(350));
    send(0xA2, false);
    send(0x7D, false);
    send(0x7D, true);
    send(0xA2, true);
    std::thread::sleep(std::time::Duration::from_millis(350));
    send(0xA0, false);
    send(0x7D, false);
    send(0x7D, true);
    send(0xA0, true);
    std::thread::sleep(std::time::Duration::from_millis(300));
    let got = between(t0, util::now());
    let actions = fired.lock().clone();
    stop();
    FORCE_GAME.store(false, Ordering::Relaxed);
    println!("touches notées : {got:?}");
    println!("raccourcis déclenchés : {actions:?}");
    let overlay_ok = got.len() >= 4 && got[0].key == 0xA0 && got[0].down && !got[1].down && got[2].key == 0xA2;
    let hotkeys_ok = actions == vec!["clip", "clip", "bookmark"];
    println!("{}", if overlay_ok { "OK : touches à l'écran" } else { "ÉCHEC : touches à l'écran" });
    println!("{}", if hotkeys_ok { "OK : raccourcis (seul, avec Ctrl en plus, le plus précis gagne)" } else { "ÉCHEC : raccourcis" });
    if overlay_ok && hotkeys_ok { 0 } else { 1 }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Les tests qui changent la liste des raccourcis passent l'un après l'autre
    static BINDINGS_TEST: Mutex<()> = Mutex::new(());

    #[test]
    fn assistant_administrateur_relaie_les_raccourcis() {
        use std::io::Write;
        let _seul = BINDINGS_TEST.lock();
        static GOT: Mutex<Vec<&'static str>> = Mutex::new(Vec::new());
        on_hotkey(|a| GOT.lock().push(a));
        assert!(set_bindings(&[("F15".to_string(), "clip")]).is_empty());
        serve_helper();
        let info: HelperInfo = serde_json::from_str(&std::fs::read_to_string(helper_file()).unwrap()).unwrap();
        // Mauvais jeton : ignoré
        let mut bad = std::net::TcpStream::connect(("127.0.0.1", info.port)).unwrap();
        bad.write_all(&b"x".repeat(info.token.len())).unwrap();
        bad.write_all(&[1, 0x66, 0, 0, 0, 0x7E, 0]).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(200));
        assert!(GOT.lock().is_empty(), "un inconnu ne déclenche rien");
        // Assistant : F15 (position 0x66) appuyé puis relâché
        let mut s = std::net::TcpStream::connect(("127.0.0.1", info.port)).unwrap();
        s.write_all(info.token.as_bytes()).unwrap();
        s.write_all(&[1, 0x66, 0, 0, 0, 0x7E, 0]).unwrap();
        s.write_all(&[1, 0x66, 0, 1, 0, 0x7E, 0]).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(300));
        assert_eq!(*GOT.lock(), vec!["clip"]);
        assert!(HELPER_ON.load(Ordering::SeqCst));
        drop(s);
        std::thread::sleep(std::time::Duration::from_millis(200));
        assert!(!HELPER_ON.load(Ordering::SeqCst), "assistant parti");
    }

    #[test]
    fn etat_au_debut_et_repetitions() {
        FORCE_GAME.store(true, Ordering::Relaxed);
        OVERLAY.store(true, Ordering::Relaxed);
        EVENTS.lock().clear();
        *PRESSED.lock() = [false; 256];
        record(0x57, true); // W enfoncé avant le clip
        record(0x57, true); // répétition automatique : ignorée
        let a = util::now() + 0.001;
        std::thread::sleep(std::time::Duration::from_millis(5));
        record(0x01, true);
        record(0x01, false);
        let list = between(a, util::now() + 1.0);
        assert_eq!(list.len(), 3, "{list:?}");
        assert_eq!((list[0].key, list[0].down), (0x57, true));
        assert_eq!(EVENTS.lock().len(), 3);
        FORCE_GAME.store(false, Ordering::Relaxed);
        GAME_PID.store(0, Ordering::Relaxed);
        record(0x41, true); // pas de jeu au premier plan : rien
        assert_eq!(EVENTS.lock().len(), 3);
    }

    #[test]
    fn lecture_des_raccourcis() {
        assert_eq!(parse("F8"), Some((0, Main::Key(0x42))));
        assert_eq!(parse("Alt+F10"), Some((ALT, Main::Key(0x44))));
        assert_eq!(parse("Alt+Shift+F10"), Some((ALT | SHIFT, Main::Key(0x44))));
        assert_eq!(parse("Ctrl+K"), Some((CTRL, Main::Key(0x25))));
        assert_eq!(parse("Mouse4"), Some((0, Main::Mouse(4))));
        assert_eq!(parse("Alt+Mouse5"), Some((ALT, Main::Mouse(5))));
        assert_eq!(parse("Ctrl+Num5"), Some((CTRL, Main::Key(0x4C))));
        assert_eq!(parse("F12"), Some((0, Main::Key(0x58))));
        assert_eq!(parse("F13"), Some((0, Main::Key(0x64))));
        assert_eq!(parse("Pause"), Some((0, Main::Key(0xE11D))));
        assert_eq!(parse("Alt+1"), Some((ALT, Main::Key(0x02))));
        assert_eq!(parse("Ctrl+Backquote"), Some((CTRL, Main::Key(0x29))));
        assert_eq!(parse("Ctrl+Truc"), None);
        assert_eq!(parse("F8+F9"), None);
    }

    #[test]
    fn choix_du_raccourci() {
        let b = |t: &str, a: &'static str| {
            let (mods, main) = parse(t).unwrap();
            Binding { mods, main, action: a, text: t.into() }
        };
        let list = vec![b("F8", "clip"), b("Alt+F8", "bookmark"), b("Alt+Shift+F10", "clip_long"), b("Mouse4", "session")];
        let f8 = parse("F8").unwrap().1;
        assert_eq!(pick(&list, f8, 0).map(|x| x.0), Some("clip"));
        assert_eq!(pick(&list, f8, CTRL).map(|x| x.0), Some("clip"), "accroupi : F8 marche quand même");
        assert_eq!(pick(&list, f8, SHIFT | CTRL).map(|x| x.0), Some("clip"));
        assert_eq!(pick(&list, f8, ALT).map(|x| x.0), Some("bookmark"), "le plus précis gagne");
        let f10 = parse("F10").unwrap().1;
        assert_eq!(pick(&list, f10, ALT).map(|x| x.0), None, "Alt+F10 seul n'est pas Alt+Maj+F10");
        assert_eq!(pick(&list, f10, ALT | SHIFT).map(|x| x.0), Some("clip_long"));
        assert_eq!(pick(&list, Main::Mouse(4), 0).map(|x| x.0), Some("session"));
        assert_eq!(pick(&list, Main::Mouse(5), 0).map(|x| x.0), None);
    }

    #[test]
    fn doublons_et_inconnus() {
        let _seul = BINDINGS_TEST.lock();
        let errors = set_bindings(&[("F8".into(), "clip"), ("F8".into(), "bookmark"), ("Ctrl+Truc".into(), "session")]);
        assert_eq!(errors.len(), 2, "{errors:?}");
        set_bindings(&[]);
    }
}
