//! Partage : envoi sur un salon Discord (webhook), copie du fichier dans le presse-papiers
//! (Ctrl+V dans Discord l'envoie), ouverture dans l'Explorateur.

use crate::util;
use anyhow::{Result, bail};
use std::{io::Read, path::Path};

/// Limite d'envoi des webhooks Discord (serveur sans boost).
pub const DISCORD_LIMIT_MB: f64 = 10.0;

pub fn send_webhook(url: &str, file: &Path, message: &str) -> Result<String> {
    if !(url.starts_with("https://discord.com/api/webhooks/")
        || url.starts_with("https://discordapp.com/api/webhooks/")
        || url.starts_with("https://ptb.discord.com/api/webhooks/")
        || url.starts_with("https://canary.discord.com/api/webhooks/"))
    {
        bail!("Ce lien n'est pas un webhook Discord.");
    }
    let mut data = Vec::new();
    std::fs::File::open(file)?.read_to_end(&mut data)?;
    let name = file.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| "clip.mp4".into());
    let ascii: String = name.chars().map(|c| if c.is_ascii() && c != '"' { c } else { '_' }).collect();
    let mime = if name.to_lowercase().ends_with(".png") { "image/png" } else { "video/mp4" };
    let boundary = format!("----rewind{}", (util::now() * 1000.0) as u64);
    let payload = serde_json::json!({ "content": message, "username": "Rewind" }).to_string();
    let mut body: Vec<u8> = Vec::with_capacity(data.len() + 1024);
    body.extend(format!("--{boundary}\r\nContent-Disposition: form-data; name=\"payload_json\"\r\nContent-Type: application/json\r\n\r\n{payload}\r\n").as_bytes());
    body.extend(format!("--{boundary}\r\nContent-Disposition: form-data; name=\"files[0]\"; filename=\"{ascii}\"\r\nContent-Type: {mime}\r\n\r\n").as_bytes());
    body.extend(&data);
    body.extend(format!("\r\n--{boundary}--\r\n").as_bytes());
    let response = ureq::post(&format!("{url}?wait=true"))
        .set("Content-Type", &format!("multipart/form-data; boundary={boundary}"))
        .timeout(std::time::Duration::from_secs(300))
        .send_bytes(&body);
    match response {
        Ok(r) => {
            let json: serde_json::Value = r.into_json().unwrap_or_default();
            Ok(json["attachments"][0]["url"].as_str().unwrap_or("").to_string())
        }
        Err(ureq::Error::Status(413, _)) => bail!("Fichier trop lourd pour Discord : compresse-le d'abord."),
        Err(ureq::Error::Status(code, r)) => bail!("Discord a refusé l'envoi ({code}) : {}", r.into_string().unwrap_or_default()),
        Err(e) => bail!("Envoi impossible : {e}"),
    }
}

/// Met le fichier dans le presse-papiers comme un copier depuis l'Explorateur.
pub fn copy_file_to_clipboard(path: &Path) -> Result<()> {
    use windows_sys::Win32::{
        Foundation::GlobalFree,
        System::{
            DataExchange::{CloseClipboard, EmptyClipboard, OpenClipboard, SetClipboardData},
            Memory::{GHND, GlobalAlloc, GlobalLock, GlobalUnlock},
        },
    };
    const CF_HDROP: u32 = 15;
    #[repr(C)]
    struct DropFiles {
        p_files: u32,
        x: i32,
        y: i32,
        f_nc: i32,
        f_wide: i32,
    }
    let wide: Vec<u16> = path.to_string_lossy().encode_utf16().chain([0, 0]).collect();
    let header = std::mem::size_of::<DropFiles>();
    let size = header + wide.len() * 2;
    unsafe {
        let mem = GlobalAlloc(GHND, size);
        if mem.is_null() {
            bail!("Mémoire insuffisante");
        }
        let ptr = GlobalLock(mem) as *mut u8;
        let df = DropFiles { p_files: header as u32, x: 0, y: 0, f_nc: 0, f_wide: 1 };
        std::ptr::copy_nonoverlapping(&df as *const DropFiles as *const u8, ptr, header);
        std::ptr::copy_nonoverlapping(wide.as_ptr() as *const u8, ptr.add(header), wide.len() * 2);
        GlobalUnlock(mem);
        let mut opened = false;
        for _ in 0..10 {
            if OpenClipboard(std::ptr::null_mut()) != 0 {
                opened = true;
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(30));
        }
        if !opened {
            GlobalFree(mem);
            bail!("Presse-papiers occupé, réessaie.");
        }
        EmptyClipboard();
        if SetClipboardData(CF_HDROP, mem as _).is_null() {
            CloseClipboard();
            GlobalFree(mem);
            bail!("Copie impossible");
        }
        CloseClipboard();
    }
    Ok(())
}

pub fn reveal(path: &Path) -> Result<()> {
    util::command(Path::new("explorer.exe")).arg(format!("/select,{}", path.to_string_lossy())).spawn()?;
    Ok(())
}

pub fn open_default(path: &Path) -> Result<()> {
    util::command(Path::new("cmd")).args(["/C", "start", ""]).arg(path).spawn()?;
    Ok(())
}
