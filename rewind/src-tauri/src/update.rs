//! Mises à jour automatiques (« lanceur ») : Rewind vérifie sur GitHub Releases s'il existe une
//! version plus récente, la télécharge, vérifie sa signature (clé publique dans tauri.conf.json)
//! et relance l'installeur. Une mise à jour non signée par notre clé est toujours refusée.

use serde::Serialize;
use tauri::{AppHandle, Emitter};
use tauri_plugin_updater::UpdaterExt;

/// Adresse du fichier `latest.json` publié avec chaque version.
/// Hébergement et téléchargement exclusivement sur GitHub Releases.
pub const ENDPOINT: &str = "https://github.com/depre99ed/Rewind/releases/latest/download/latest.json";

#[derive(Clone, Serialize)]
pub struct UpdateInfo {
    pub configured: bool,
    pub current: String,
    pub available: Option<String>,
    pub notes: String,
}

fn endpoint() -> Option<tauri::Url> {
    let raw = std::env::var("REWIND_UPDATE_URL").ok().filter(|s| !s.is_empty()).unwrap_or_else(|| ENDPOINT.to_string());
    tauri::Url::parse(&raw).ok()
}

pub async fn check(app: &AppHandle) -> Result<UpdateInfo, String> {
    check_within(app, std::time::Duration::from_secs(20)).await
}

/// Recherche avec un délai maximal (au lancement : quelques secondes, pour ne pas retarder la fenêtre).
pub async fn check_within(app: &AppHandle, timeout: std::time::Duration) -> Result<UpdateInfo, String> {
    let current = app.package_info().version.to_string();
    let Some(url) = endpoint() else {
        return Ok(UpdateInfo { configured: false, current, available: None, notes: String::new() });
    };
    let updater = app.updater_builder().endpoints(vec![url]).map_err(|e| e.to_string())?
        .timeout(timeout).build().map_err(|e| e.to_string())?;
    match updater.check().await {
        Ok(Some(u)) => Ok(UpdateInfo { configured: true, current, available: Some(u.version.clone()), notes: u.body.clone().unwrap_or_default() }),
        Ok(None) => Ok(UpdateInfo { configured: true, current, available: None, notes: String::new() }),
        Err(e) => Err(format!("Recherche de mise à jour impossible : {e}")),
    }
}

/// Vérifie qu'un installeur est bien signé par NOTRE clé (celle dont la clé publique est dans
/// tauri.conf.json, embarquée dans Rewind). Même contrôle que celui du module de mise à jour.
pub fn verify(file: &std::path::Path, sig_b64: &str) -> Result<(), String> {
    use base64::Engine;
    let conf: serde_json::Value = serde_json::from_str(include_str!("../tauri.conf.json")).map_err(|e| e.to_string())?;
    let pub_b64 = conf["plugins"]["updater"]["pubkey"].as_str().ok_or("clé publique absente de tauri.conf.json")?;
    let b64 = base64::engine::general_purpose::STANDARD;
    let pk_text = String::from_utf8(b64.decode(pub_b64.trim()).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    let sig_text = String::from_utf8(b64.decode(sig_b64.trim()).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    let pk = minisign_verify::PublicKey::decode(&pk_text).map_err(|e| format!("clé publique : {e}"))?;
    let sig = minisign_verify::Signature::decode(&sig_text).map_err(|e| format!("signature : {e}"))?;
    let data = std::fs::read(file).map_err(|e| e.to_string())?;
    pk.verify(&data, &sig, true).map_err(|e| format!("signature refusée : {e}"))
}

/// Test : `rewind.exe --verify-update <installeur> <fichier .sig>` (+ contrôle qu'un fichier modifié est refusé).
pub fn self_test(file: &std::path::Path, sig_file: &std::path::Path) -> i32 {
    let sig = std::fs::read_to_string(sig_file).unwrap_or_default();
    let good = verify(file, &sig);
    println!("installeur d'origine : {}", match &good { Ok(()) => "signature valide".to_string(), Err(e) => format!("ÉCHEC {e}") });
    // Copie modifiée d'un octet : doit être refusée
    let tampered = std::env::temp_dir().join("rewind-verif-modifie.bin");
    let mut data = std::fs::read(file).unwrap_or_default();
    if let Some(b) = data.get_mut(1000) {
        *b ^= 0xff;
    }
    let _ = std::fs::write(&tampered, &data);
    let bad = verify(&tampered, &sig);
    let _ = std::fs::remove_file(&tampered);
    println!("installeur modifié : {}", match &bad { Ok(()) => "ACCEPTÉ (problème !)".to_string(), Err(e) => format!("refusé ({e})") });
    if good.is_ok() && bad.is_err() { 0 } else { 1 }
}

/// Télécharge, vérifie et installe la mise à jour, puis relance Rewind.
pub async fn install(app: &AppHandle) -> Result<(), String> {
    use std::sync::atomic::{AtomicBool, Ordering};
    static INSTALLING: AtomicBool = AtomicBool::new(false);
    if INSTALLING.compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire).is_err() {
        return Err("Une installation est déjà en cours".into());
    }
    struct Reset;
    impl Drop for Reset {
        fn drop(&mut self) { INSTALLING.store(false, Ordering::Release); }
    }
    let _reset = Reset;
    let url = endpoint().ok_or("Canal de mises à jour non configuré")?;
    let updater = app.updater_builder().endpoints(vec![url]).map_err(|e| e.to_string())?
        .timeout(std::time::Duration::from_secs(600)).build().map_err(|e| e.to_string())?;
    let Some(update) = updater.check().await.map_err(|e| e.to_string())? else {
        return Err("Aucune mise à jour disponible".into());
    };
    log::info!("mise à jour vers {}", update.version);
    let mut received = 0usize;
    let handle = app.clone();
    update
        .download_and_install(
            move |chunk, total| {
                received += chunk;
                if let Some(t) = total.filter(|t| *t > 0) {
                    let _ = handle.emit("edit-progress", serde_json::json!({ "op": "update", "value": received as f64 / t as f64 }));
                }
            },
            || log::info!("mise à jour téléchargée, installation"),
        )
        .await
        .map_err(|e| format!("Installation impossible : {e}"))?;
    app.restart();
}

/// Contrôle le vrai canal et la signature sans rien installer ni lancer l'enregistrement.
pub async fn probe(app: &AppHandle, download: bool) -> Result<serde_json::Value, String> {
    let info = check(app).await?;
    if !info.configured { return Err("Canal de mises à jour non configuré".into()); }
    let mut result = serde_json::json!({ "check": info, "signature_verified": false });
    if download {
        let url = endpoint().ok_or("Canal non configuré")?;
        // Ce comparateur est réservé au diagnostic : contrôler aussi la version déjà installée.
        let updater = app.updater_builder().endpoints(vec![url]).map_err(|e| e.to_string())?
            .timeout(std::time::Duration::from_secs(600))
            .version_comparator(|_, _| true).build().map_err(|e| e.to_string())?;
        let update = updater.check().await.map_err(|e| e.to_string())?.ok_or("Version publiée absente")?;
        let bytes = update.download(|_, _| {}, || {}).await.map_err(|e| e.to_string())?;
        result["signature_verified"] = true.into();
        result["downloaded_version"] = update.version.into();
        result["downloaded_bytes"] = bytes.len().into();
    }
    Ok(result)
}
