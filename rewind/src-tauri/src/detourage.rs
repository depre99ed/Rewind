//! Détourage : retirer le fond d'une image (2026-11-24).
//!
//! Modèle IS-Net « général » (projet DIS, licence Apache 2.0 ; fichier publié par rembg), téléchargé une seule
//! fois (178 Mo, empreinte SHA-256 vérifiée) dans `data_dir/outils`, puis exécuté sur le processeur avec
//! ONNX Runtime (lié dans Rewind, rien à installer). Tout reste sur le PC : l'image n'est envoyée nulle part.
//!
//! Même préparation que rembg : image ramenée à 1024 × 1024, valeurs divisées par le maximum puis − 0,5 ;
//! masque de sortie normalisé (min → 0, max → 1). L'interface remet le masque à la taille de l'image.

use crate::util;
use anyhow::{Context, Result, anyhow, bail};
use base64::Engine;
use ort::session::Session;
use ort::value::Tensor;
use parking_lot::Mutex;
use sha2::{Digest, Sha256};
use std::io::{Read, Write};
use std::path::PathBuf;
use std::sync::{Once, OnceLock};
use std::time::{Duration, Instant};

const URL: &str = "https://github.com/danielgatis/rembg/releases/download/v0.0.0/isnet-general-use.onnx";
const SHA256: &str = "60920e99c45464f2ba57bee2ad08c919a52bbf852739e96947fbb4358c0d964a";
const BYTES: u64 = 178_648_008;
/// Côté de l'image donnée au modèle
pub const SIDE: usize = 1024;

fn model_path() -> PathBuf {
    util::data_dir().join("outils").join("detourage-isnet-general.onnx")
}

/// Le modèle est-il déjà sur le PC ?
pub fn ready() -> bool {
    std::fs::metadata(model_path()).is_ok_and(|m| m.len() == BYTES)
}

/// Télécharge le modèle si besoin (avancement 0..1), vérifie son empreinte, renvoie son chemin.
pub fn model(progress: &dyn Fn(f64)) -> Result<PathBuf> {
    let path = model_path();
    if ready() {
        return Ok(path);
    }
    std::fs::create_dir_all(path.parent().unwrap_or(std::path::Path::new(".")))?;
    let part = path.with_extension("part");
    let resp = ureq::get(URL).timeout(std::time::Duration::from_secs(600)).call().context("Téléchargement de l'outil de détourage")?;
    let mut reader = resp.into_reader();
    let mut file = std::fs::File::create(&part)?;
    let mut hash = Sha256::new();
    let mut buf = vec![0u8; 1 << 16];
    let mut done = 0u64;
    loop {
        let n = reader.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hash.update(&buf[..n]);
        file.write_all(&buf[..n])?;
        done += n as u64;
        progress(done as f64 / BYTES as f64);
    }
    file.flush()?;
    drop(file);
    let got: String = hash.finalize().iter().map(|b| format!("{b:02x}")).collect();
    if got != SHA256 || done != BYTES {
        let _ = std::fs::remove_file(&part);
        bail!("L'outil de détourage téléchargé est abîmé (empreinte différente) : réessaie.");
    }
    std::fs::rename(&part, &path)?;
    Ok(path)
}

/// Session ONNX gardée en mémoire (178 Mo de modèle + tampons) seulement tant qu'elle sert : libérée après
/// `IDLE` sans appel, recréée au besoin (quelques secondes) au détourage suivant.
struct Loaded {
    session: Session,
    last_use: Instant,
}

static SESSION: Mutex<Option<Loaded>> = Mutex::new(None);
/// L'environnement ONNX Runtime ne peut être créé qu'une fois par processus : il reste, lui (quelques ko).
static ENV: OnceLock<ort::environment::Environment> = OnceLock::new();
static WATCHER: Once = Once::new();
const IDLE: Duration = Duration::from_secs(120);

fn create_session() -> Result<Session> {
    let path = model(&|_| {})?;
    if ENV.get().is_none() {
        let env = ort::init().with_name("rewind").build().map_err(|e| anyhow!("ONNX Runtime : {e}"))?;
        let _ = ENV.set(env);
    }
    let env = ENV.get().context("ONNX Runtime")?;
    // La moitié des cœurs (2 à 6) : le détourage ne doit pas écraser le reste de l'appli, et les fils
    // inactifs ne tournent pas à vide (« spinning » coupé)
    let threads = (std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4) / 2).clamp(2, 6);
    Session::builder(env)
        .map_err(|e| anyhow!("{e}"))?
        .with_intra_threads(threads)
        .map_err(|e| anyhow!("{e}"))?
        .with_intra_op_spinning(false)
        .map_err(|e| anyhow!("{e}"))?
        .with_inter_op_spinning(false)
        .map_err(|e| anyhow!("{e}"))?
        .commit_from_file(&path)
        .map_err(|e| anyhow!("Modèle de détourage illisible : {e}"))
}

/// Petit fil, démarré une seule fois : libère la session inutilisée depuis `IDLE`.
fn start_watcher() {
    WATCHER.call_once(|| {
        let _ = std::thread::Builder::new().name("detourage-idle".into()).spawn(|| {
            loop {
                std::thread::sleep(Duration::from_secs(30));
                let mut slot = SESSION.lock();
                if slot.as_ref().is_some_and(|l| l.last_use.elapsed() >= IDLE) {
                    *slot = None;
                    log::info!("détourage : modèle libéré de la mémoire (inutilisé depuis 2 min)");
                }
            }
        });
    });
}

/// Masque (SIDE × SIDE, 0..255 ; 255 = sujet gardé) d'une image RGBA SIDE × SIDE.
pub fn mask_rgba(rgba: &[u8]) -> Result<Vec<u8>> {
    if rgba.len() != SIDE * SIDE * 4 {
        bail!("image de {} octets, {} attendus", rgba.len(), SIDE * SIDE * 4);
    }
    // Valeurs / maximum, − 0,5 (rembg : normalize(mean 0,5, std 1)), en NCHW
    let max = rgba.chunks_exact(4).flat_map(|p| [p[0], p[1], p[2]]).max().unwrap_or(255).max(1) as f32;
    let plane = SIDE * SIDE;
    let mut data = vec![0f32; 3 * plane];
    for (i, p) in rgba.chunks_exact(4).enumerate() {
        for c in 0..3 {
            data[c * plane + i] = p[c] as f32 / max - 0.5;
        }
    }
    let input = Tensor::from_array(([1usize, 3, SIDE, SIDE], data)).map_err(|e| anyhow!("{e}"))?;
    start_watcher();
    let mut slot = SESSION.lock();
    if slot.is_none() {
        *slot = Some(Loaded { session: create_session()?, last_use: Instant::now() });
    }
    let loaded = slot.as_mut().context("session")?;
    let name = loaded.session.inputs()[0].name().to_string();
    let mask = {
        let outputs = loaded.session.run(ort::inputs![name => input]).map_err(|e| anyhow!("Détourage : {e}"))?;
        let (_, pred) = outputs[0].try_extract_tensor::<f32>().map_err(|e| anyhow!("{e}"))?;
        let pred = &pred[..plane.min(pred.len())];
        let (mi, ma) = pred.iter().fold((f32::MAX, f32::MIN), |(a, b), v| (a.min(*v), b.max(*v)));
        let span = (ma - mi).max(1e-6);
        pred.iter().map(|v| (((v - mi) / span) * 255.0).round().clamp(0.0, 255.0) as u8).collect::<Vec<u8>>()
    };
    loaded.last_use = Instant::now();
    Ok(mask)
}

/// PNG (base64) SIDE × SIDE → masque en base64 (SIDE × SIDE octets).
pub fn mask_png(b64: &str) -> Result<String> {
    let bytes = base64::engine::general_purpose::STANDARD.decode(b64.trim()).context("image illisible")?;
    let img = tauri::image::Image::from_bytes(&bytes).context("image illisible")?;
    if img.width() as usize != SIDE || img.height() as usize != SIDE {
        bail!("image de {}×{}, {SIDE}×{SIDE} attendu", img.width(), img.height());
    }
    let m = mask_rgba(img.rgba())?;
    Ok(base64::engine::general_purpose::STANDARD.encode(m))
}

/// Image détourée (PNG base64) rangée pour de bon dans `data_dir/images` (le cache du Studio est vidé).
pub fn save(b64: &str, name: &str) -> Result<String> {
    let bytes = base64::engine::general_purpose::STANDARD.decode(b64.trim()).context("image illisible")?;
    if bytes.len() < 8 || &bytes[1..4] != b"PNG" {
        bail!("ce n'est pas une image PNG");
    }
    let dir = util::data_dir().join("images");
    std::fs::create_dir_all(&dir)?;
    let h: String = Sha256::digest(&bytes).iter().take(5).map(|b| format!("{b:02x}")).collect();
    let path = dir.join(format!("{} {h}.png", util::safe_name(name)));
    std::fs::write(&path, bytes)?;
    Ok(path.to_string_lossy().to_string())
}
