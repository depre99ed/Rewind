//! Notification en jeu : petite fenêtre transparente, toujours devant, qui ne prend jamais
//! le focus (le jeu garde la main) et que les clics traversent.

use crate::{settings::Settings, util};
use serde::Serialize;
use std::sync::atomic::{AtomicU64, Ordering};
use tauri::{AppHandle, Emitter, Manager, WebviewUrl, WebviewWindowBuilder};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    GWL_EXSTYLE, GetWindowLongPtrW, HWND_TOPMOST, SW_HIDE, SW_SHOWNOACTIVATE, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE,
    SetWindowLongPtrW, SetWindowPos, ShowWindow, WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TRANSPARENT,
};

const W: f64 = 400.0;
const H: f64 = 104.0;
static GENERATION: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Serialize, Default)]
pub struct Toast {
    /// "saving", "saved", "failed", "shot"
    pub state: String,
    pub title: String,
    pub subtitle: String,
    pub game: String,
    pub thumb: String,
    pub badges: Vec<String>,
    pub seconds: f64,
    /// Bandeau d'accueil : (raccourci, action)
    pub keys: Vec<(String, String)>,
    /// Langue de l'interface ("fr" / "en"), remplie par `show`
    pub lang: String,
}

/// Hauteur de la fenêtre selon le contenu (le bandeau d'accueil liste les raccourcis).
fn height_for(t: &Toast) -> f64 {
    if t.state == "welcome" { 98.0 + 27.0 * t.keys.len() as f64 + 12.0 } else { H }
}

fn hwnd(w: &tauri::WebviewWindow) -> Option<*mut std::ffi::c_void> {
    w.hwnd().ok().map(|h| h.0 as *mut std::ffi::c_void)
}

/// Crée la fenêtre (cachée) au démarrage.
pub fn create(app: &AppHandle) {
    if app.get_webview_window("toast").is_some() {
        return;
    }
    let built = WebviewWindowBuilder::new(app, "toast", WebviewUrl::App("toast".into()))
        .title("Rewind — notification")
        .inner_size(W, H)
        .decorations(false)
        .transparent(true)
        .shadow(false)
        .always_on_top(true)
        .skip_taskbar(true)
        .resizable(false)
        .focused(false)
        .visible(false)
        .build();
    match built {
        Ok(w) => {
            if let Some(h) = hwnd(&w) {
                unsafe {
                    let ex = GetWindowLongPtrW(h, GWL_EXSTYLE);
                    SetWindowLongPtrW(
                        h,
                        GWL_EXSTYLE,
                        ex | (WS_EX_NOACTIVATE | WS_EX_TRANSPARENT | WS_EX_TOOLWINDOW | WS_EX_LAYERED) as isize,
                    );
                }
            }
        }
        Err(e) => log::warn!("notification en jeu indisponible : {e}"),
    }
}

/// Place la fenêtre dans le coin choisi de l'écran filmé.
fn place(w: &tauri::WebviewWindow, settings: &Settings, height: f64) {
    let monitors = util::monitors();
    let Some(m) = monitors
        .iter()
        .find(|m| !settings.monitor.is_empty() && m.name == settings.monitor)
        .or_else(|| monitors.iter().find(|m| m.primary))
    else {
        return;
    };
    let scale = w.scale_factor().unwrap_or(1.0);
    let (ww, hh) = ((W * scale) as i32, (height * scale) as i32);
    let margin = (24.0 * scale) as i32;
    let (x, y) = match settings.notify_position.as_str() {
        "top-left" => (m.x + margin, m.y + margin),
        "top-right" => (m.x + m.width - ww - margin, m.y + margin),
        "top-center" => (m.x + (m.width - ww) / 2, m.y + margin),
        "bottom-left" => (m.x + margin, m.y + m.height - hh - margin * 3),
        "bottom-right" => (m.x + m.width - ww - margin, m.y + m.height - hh - margin * 3),
        // Milieu gauche : zone libre dans la plupart des jeux (CS2 : sous le radar, au-dessus du chat)
        _ => (m.x + margin, m.y + (m.height - hh) / 2),
    };
    let _ = w.set_position(tauri::PhysicalPosition::new(x, y));
}

/// Affiche (ou met à jour) la notification sans voler le focus du jeu.
pub fn show(app: &AppHandle, settings: &Settings, mut toast: Toast) {
    if !settings.notify_overlay {
        return;
    }
    toast.lang = if util::english(&settings.language) { "en".into() } else { "fr".into() };
    let Some(w) = app.get_webview_window("toast") else { return };
    let height = height_for(&toast);
    let _ = w.set_size(tauri::LogicalSize::new(W, height));
    place(&w, settings, height);
    let duration = match toast.state.as_str() {
        "saving" => 9000,
        "welcome" => 6000,
        _ => 3600,
    };
    let _ = app.emit_to("toast", "toast", toast);
    if let Some(h) = hwnd(&w) {
        unsafe {
            ShowWindow(h, SW_SHOWNOACTIVATE);
            SetWindowPos(h, HWND_TOPMOST, 0, 0, 0, 0, SWP_NOACTIVATE | SWP_NOMOVE | SWP_NOSIZE);
        }
    }
    let generation = GENERATION.fetch_add(1, Ordering::SeqCst) + 1;
    let app = app.clone();
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(duration));
        if GENERATION.load(Ordering::SeqCst) != generation {
            return; // une notification plus récente a pris la place
        }
        let _ = app.emit_to("toast", "toast-hide", ());
        std::thread::sleep(std::time::Duration::from_millis(420));
        if GENERATION.load(Ordering::SeqCst) == generation
            && let Some(w) = app.get_webview_window("toast")
            && let Some(h) = hwnd(&w)
        {
            unsafe {
                ShowWindow(h, SW_HIDE);
            }
        }
    });
}
