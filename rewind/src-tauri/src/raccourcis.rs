//! Raccourcis Windows de Rewind : après un changement de logo, l'icône des raccourcis déjà créés
//! (Bureau, menu Démarrer, épinglés à la barre des tâches) se met à jour (2026-11-16).
//!
//! Windows garde l'icône d'un raccourci en cache tant qu'il pointe vers le même fichier, même après un
//! redémarrage. Remède (celui de Chrome) : chaque raccourci QUI POINTE VERS REWIND reçoit comme icône un
//! fichier .ico dont le nom change avec le logo, puis Windows est prévenu raccourci par raccourci
//! (SHCNE_UPDATEITEM ; SHCNE_ASSOCCHANGED seul ne rafraîchit pas la barre des tâches).
//! Fait une seule fois par logo ; les autres raccourcis ne sont ouverts qu'en lecture.

use crate::util;
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use windows::Win32::Storage::FileSystem::WIN32_FIND_DATAW;
use windows::Win32::System::Com::{CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, CoCreateInstance, CoInitializeEx, CoUninitialize, IPersistFile, STGM_READ, STGM_READWRITE};
use windows::Win32::UI::Shell::{IShellLinkW, ShellLink};
use windows::core::{HSTRING, Interface, PCWSTR};

/// Icône de l'application telle qu'elle est compilée (même fichier que l'installeur)
const ICON: &[u8] = include_bytes!("../icons/icon.ico");

fn icon_tag() -> String {
    Sha256::digest(ICON)[..5].iter().map(|b| format!("{b:02x}")).collect()
}

/// Dossiers où Windows range les raccourcis de l'utilisateur.
fn shortcut_dirs() -> Vec<PathBuf> {
    use windows_sys::Win32::UI::Shell::{FOLDERID_Desktop, FOLDERID_Programs, FOLDERID_PublicDesktop, FOLDERID_StartMenu, SHGetKnownFolderPath};
    let mut dirs = Vec::new();
    for id in [&FOLDERID_Desktop, &FOLDERID_PublicDesktop, &FOLDERID_Programs, &FOLDERID_StartMenu] {
        let mut p: windows_sys::core::PWSTR = std::ptr::null_mut();
        unsafe {
            if SHGetKnownFolderPath(id, 0, std::ptr::null_mut(), &mut p) == 0 && !p.is_null() {
                let len = (0..).take_while(|&i| *p.add(i) != 0).count();
                dirs.push(PathBuf::from(String::from_utf16_lossy(std::slice::from_raw_parts(p, len))));
            }
            windows_sys::Win32::System::Com::CoTaskMemFree(p as _);
        }
    }
    if let Ok(appdata) = std::env::var("APPDATA") {
        let pinned = PathBuf::from(appdata).join(r"Microsoft\Internet Explorer\Quick Launch");
        dirs.push(pinned.join(r"User Pinned\TaskBar"));
        dirs.push(pinned.join(r"User Pinned\StartMenu"));
        dirs.push(pinned);
    }
    dirs
}

/// Raccourcis (.lnk) d'un dossier et de ses sous-dossiers directs (« Rewind » dans le menu Démarrer).
fn lnk_files(dir: &Path, depth: u32, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for e in entries.flatten() {
        let p = e.path();
        if p.is_dir() {
            if depth > 0 {
                lnk_files(&p, depth - 1, out);
            }
        } else if p.extension().is_some_and(|x| x.eq_ignore_ascii_case("lnk")) {
            out.push(p);
        }
    }
}

fn same_file(a: &Path, b: &Path) -> bool {
    let norm = |p: &Path| std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf()).to_string_lossy().to_lowercase();
    norm(a) == norm(b)
}

/// Nouvelle icône sur un raccourci s'il pointe vers Rewind. true = modifié.
/// Lecture seule pour connaître la cible (les raccourcis des autres programmes ne sont jamais ouverts en
/// écriture : avant la 0.4.4, ceux du Bureau public donnaient « accès refusé ») ; écriture seulement pour Rewind.
fn retarget_icon(lnk: &Path, exe: &Path, icon: &Path) -> windows::core::Result<bool> {
    unsafe {
        let link: IShellLinkW = CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER)?;
        let file: IPersistFile = link.cast()?;
        file.Load(&HSTRING::from(lnk.as_os_str()), STGM_READ)?;
        let mut target = [0u16; 1024];
        let mut data = WIN32_FIND_DATAW::default();
        link.GetPath(&mut target, &mut data, 0)?;
        let len = target.iter().position(|&c| c == 0).unwrap_or(target.len());
        let target = PathBuf::from(String::from_utf16_lossy(&target[..len]));
        if target.as_os_str().is_empty() || !same_file(&target, exe) {
            return Ok(false);
        }
        let link: IShellLinkW = CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER)?;
        let file: IPersistFile = link.cast()?;
        file.Load(&HSTRING::from(lnk.as_os_str()), STGM_READWRITE)?;
        link.SetIconLocation(&HSTRING::from(icon.as_os_str()), 0)?;
        file.Save(PCWSTR::null(), true)?;
        Ok(true)
    }
}

fn notify_item(path: &Path) {
    use windows_sys::Win32::UI::Shell::{SHCNE_UPDATEITEM, SHCNF_PATHW, SHChangeNotify};
    let w = util::wide(&path.to_string_lossy());
    unsafe { SHChangeNotify(SHCNE_UPDATEITEM as i32, SHCNF_PATHW, w.as_ptr() as _, std::ptr::null()) };
}

/// À lancer au démarrage : ne fait rien si le logo n'a pas changé depuis la dernière fois.
pub fn refresh_icons_once() {
    let _ = std::thread::Builder::new().name("raccourcis".into()).spawn(|| {
        std::thread::sleep(std::time::Duration::from_secs(4));
        let tag = icon_tag();
        let flag = util::data_dir().join("icone.txt");
        if std::fs::read_to_string(&flag).is_ok_and(|t| t.trim() == tag) {
            return;
        }
        let Ok(exe) = std::env::current_exe() else { return };
        let icon = util::data_dir().join(format!("icone-{tag}.ico"));
        if std::fs::write(&icon, ICON).is_err() {
            return;
        }
        // Anciennes icônes : supprimées (les raccourcis vont pointer vers la nouvelle)
        if let Ok(entries) = std::fs::read_dir(util::data_dir()) {
            for e in entries.flatten() {
                let name = e.file_name().to_string_lossy().to_string();
                if name.starts_with("icone-") && name.ends_with(".ico") && e.path() != icon {
                    let _ = std::fs::remove_file(e.path());
                }
            }
        }
        let mut files = Vec::new();
        for d in shortcut_dirs() {
            lnk_files(&d, 1, &mut files);
        }
        let mut changed = 0;
        unsafe {
            let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        }
        for lnk in &files {
            match retarget_icon(lnk, &exe, &icon) {
                Ok(true) => {
                    notify_item(lnk);
                    changed += 1;
                }
                Ok(false) => {}
                Err(e) => log::warn!("raccourci {} : icône non changée ({e})", lnk.display()),
            }
        }
        unsafe { CoUninitialize() };
        notify_item(&exe);
        unsafe {
            use windows_sys::Win32::UI::Shell::{SHCNE_ASSOCCHANGED, SHCNF_IDLIST, SHChangeNotify};
            SHChangeNotify(SHCNE_ASSOCCHANGED as i32, SHCNF_IDLIST, std::ptr::null(), std::ptr::null());
        }
        log::info!("logo : {changed} raccourci(s) de Rewind mis à jour");
        let _ = std::fs::write(&flag, tag);
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Vrai raccourci Windows : celui qui pointe vers le programme reçoit la nouvelle icône, un autre n'est pas touché.
    #[test]
    fn icone_des_raccourcis() {
        let dir = std::env::temp_dir().join(format!("rewind-lnk-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let exe = std::env::current_exe().unwrap();
        let other = PathBuf::from(r"C:\Windows\notepad.exe");
        let icon = dir.join("icone-test.ico");
        std::fs::write(&icon, ICON).unwrap();
        unsafe {
            let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
            for (name, target) in [("Rewind.lnk", &exe), ("Autre.lnk", &other)] {
                let link: IShellLinkW = CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER).unwrap();
                link.SetPath(&HSTRING::from(target.as_os_str())).unwrap();
                let file: IPersistFile = link.cast().unwrap();
                file.Save(&HSTRING::from(dir.join(name).as_os_str()), true).unwrap();
            }
        }
        // Raccourci d'un autre programme en lecture seule (comme ceux du Bureau public) : lu sans erreur, pas touché
        let autre = dir.join("Autre.lnk");
        let mut perms = std::fs::metadata(&autre).unwrap().permissions();
        perms.set_readonly(true);
        std::fs::set_permissions(&autre, perms.clone()).unwrap();
        assert!(retarget_icon(&dir.join("Rewind.lnk"), &exe, &icon).unwrap());
        assert!(!retarget_icon(&autre, &exe, &icon).unwrap(), "lecture seule : aucune erreur");
        #[allow(clippy::permissions_set_readonly_false)]
        perms.set_readonly(false);
        std::fs::set_permissions(&autre, perms).unwrap();
        unsafe {
            let read = |name: &str| {
                let link: IShellLinkW = CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER).unwrap();
                let file: IPersistFile = link.cast().unwrap();
                file.Load(&HSTRING::from(dir.join(name).as_os_str()), STGM_READWRITE).unwrap();
                let mut buf = [0u16; 1024];
                let mut idx = 0;
                link.GetIconLocation(&mut buf, &mut idx).unwrap();
                String::from_utf16_lossy(&buf[..buf.iter().position(|&c| c == 0).unwrap_or(0)])
            };
            assert_eq!(read("Rewind.lnk").to_lowercase(), icon.to_string_lossy().to_lowercase());
            assert_eq!(read("Autre.lnk"), "");
            CoUninitialize();
        }
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(icon_tag().len(), 10);
    }
}
