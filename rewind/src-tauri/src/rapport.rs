//! Rapport de diagnostic : tout ce qu'il faut pour comprendre un problème sans demander de captures.
//!
//! Le rapport réunit la version, le système, le matériel, les réglages, l'état du moteur, la
//! liaison CS2, les mises à jour, la bibliothèque, les plantages et gels, les événements d'erreur de
//! Windows, le journal de Rewind (erreurs, avertissements, erreurs de l'interface, fin du journal)
//! et les journaux de FFmpeg (y compris ceux de la session précédente).
//!
//! Vie privée : aucun mot de passe, aucune adresse de webhook Discord ; le dossier personnel est
//! remplacé par `%USERPROFILE%` ; le nom du PC est masqué.
//!
//! Attention : `panic = "abort"` en version publiée. Ce module ne doit jamais paniquer (pas de
//! `unwrap`, pas de découpage de texte hors des limites de caractères).

use crate::{audio, ffmpeg, settings::Settings, util};
use std::{
    fmt::Write as _,
    fs,
    io::{Read, Seek, SeekFrom},
    path::{Path, PathBuf},
    time::SystemTime,
};

/// Ce que `lib.rs` fournit (état vivant de l'appli).
pub struct Live {
    pub settings: Settings,
    pub status: String,
    pub status_error: String,
    pub cs2: String,
    pub update: String,
    pub library: String,
    pub webview: String,
}

pub struct Report {
    pub text: String,
    /// Nombre d'éléments à regarder (erreurs, plantages, gels…)
    pub problems: u32,
}

const MAX_READ: u64 = 3_000_000;

// ---------------------------------------------------------------------------
// Petits outils
// ---------------------------------------------------------------------------

/// Remplacement insensible à la casse (ASCII), sans jamais couper un caractère.
fn replace_ci(text: &str, needle: &str, with: &str) -> String {
    if needle.is_empty() {
        return text.to_string();
    }
    let lower = text.to_ascii_lowercase();
    let n = needle.to_ascii_lowercase();
    let mut out = String::with_capacity(text.len());
    let mut from = 0;
    while let Some(pos) = lower[from..].find(&n) {
        let at = from + pos;
        out.push_str(&text[from..at]);
        out.push_str(with);
        from = at + n.len();
    }
    out.push_str(&text[from..]);
    out
}

/// Retire ce qui est personnel ou secret.
pub fn sanitize(text: &str) -> String {
    let mut out = text.to_string();
    if let Ok(profile) = std::env::var("USERPROFILE")
        && profile.len() > 3
    {
        out = replace_ci(&out, &profile, "%USERPROFILE%");
        out = replace_ci(&out, &profile.replace('\\', "\\\\"), "%USERPROFILE%");
        out = replace_ci(&out, &profile.replace('\\', "/"), "%USERPROFILE%");
    }
    if let Ok(pc) = std::env::var("COMPUTERNAME")
        && pc.len() >= 4
    {
        out = replace_ci(&out, &pc, "<pc>");
    }
    // Adresses de webhooks Discord : on garde le début, jamais le secret.
    let marker = "/api/webhooks/";
    let mut result = String::with_capacity(out.len());
    let mut rest = out.as_str();
    while let Some(pos) = rest.find(marker) {
        let end = pos + marker.len();
        result.push_str(&rest[..end]);
        result.push_str("<masqué>");
        let tail = &rest[end..];
        let stop = tail.find(|c: char| c.is_whitespace() || matches!(c, '"' | '\'' | ')' | ',' | '<' | '>')).unwrap_or(tail.len());
        rest = &tail[stop..];
    }
    result.push_str(rest);
    result
}

/// Dernières lignes d'un fichier texte (lecture limitée aux derniers octets).
fn tail_lines(path: &Path, max_lines: usize) -> Vec<String> {
    let Ok(mut f) = fs::File::open(path) else { return Vec::new() };
    let len = f.metadata().map(|m| m.len()).unwrap_or(0);
    if len > MAX_READ {
        let _ = f.seek(SeekFrom::Start(len - MAX_READ));
    }
    let mut buf = Vec::new();
    let _ = f.read_to_end(&mut buf);
    let text = String::from_utf8_lossy(&buf);
    let mut lines: Vec<String> = text.lines().map(|l| l.to_string()).collect();
    if len > MAX_READ && !lines.is_empty() {
        lines.remove(0); // première ligne probablement coupée
    }
    let skip = lines.len().saturating_sub(max_lines);
    lines.split_off(skip)
}

fn first_lines(path: &Path, max_lines: usize) -> Vec<String> {
    let Ok(mut f) = fs::File::open(path) else { return Vec::new() };
    let mut buf = vec![0u8; 200_000];
    let n = f.read(&mut buf).unwrap_or(0);
    buf.truncate(n);
    String::from_utf8_lossy(&buf).lines().take(max_lines).map(|l| l.to_string()).collect()
}

fn is_error(line: &str) -> bool {
    line.contains("[ERROR]")
}
fn is_warn(line: &str) -> bool {
    line.contains("[WARN]")
}

fn ffmpeg_error_line(line: &str) -> bool {
    let l = line.to_ascii_lowercase();
    ["error", "fail", "invalid", "unable", "cannot", "can't", "denied", "no such", "not found", "unknown", "overrun", "dropping", "too large", "timed out", "broken", "corrupt", "discontinuity"]
        .iter()
        .any(|k| l.contains(k))
}

fn human(bytes: u64) -> String {
    if bytes >= 1_000_000_000 {
        format!("{:.1} Go", bytes as f64 / 1e9)
    } else if bytes >= 1_000_000 {
        format!("{:.1} Mo", bytes as f64 / 1e6)
    } else {
        format!("{} Ko", (bytes / 1000).max(if bytes > 0 { 1 } else { 0 }))
    }
}

fn when(t: SystemTime) -> String {
    let dt: chrono::DateTime<chrono::Local> = t.into();
    dt.format("%Y-%m-%d %H:%M:%S").to_string()
}

fn section(out: &mut String, title: &str) {
    let _ = writeln!(out, "\n==================== {title} ====================");
}

// ---------------------------------------------------------------------------
// Système
// ---------------------------------------------------------------------------

fn memory_info() -> Option<(u64, u64)> {
    use windows_sys::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};
    unsafe {
        let mut m: MEMORYSTATUSEX = std::mem::zeroed();
        m.dwLength = std::mem::size_of::<MEMORYSTATUSEX>() as u32;
        (GlobalMemoryStatusEx(&mut m) != 0).then_some((m.ullTotalPhys, m.ullAvailPhys))
    }
}

fn windows_uptime_hours() -> f64 {
    use windows_sys::Win32::System::SystemInformation::GetTickCount64;
    unsafe { GetTickCount64() as f64 / 3_600_000.0 }
}

fn system_section(out: &mut String) {
    let cv = util::reg_values(r"HKLM\SOFTWARE\Microsoft\Windows NT\CurrentVersion");
    let get = |k: &str| cv.get(k).cloned().unwrap_or_default();
    let build: u32 = get("CurrentBuildNumber").parse().unwrap_or(0);
    // Windows 11 annonce encore « Windows 10 » dans ProductName : le numéro de build tranche.
    let name = if build >= 22000 { get("ProductName").replace("Windows 10", "Windows 11") } else { get("ProductName") };
    let _ = writeln!(out, "Windows : {} {} (build {}.{})", name, get("DisplayVersion"), get("CurrentBuildNumber"), cv.get("UBR").map(|u| u.trim_start_matches("0x")).map(|h| u32::from_str_radix(h, 16).unwrap_or(0)).unwrap_or(0));
    let cpu = util::reg_values(r"HKLM\HARDWARE\DESCRIPTION\System\CentralProcessor\0");
    let cores = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(0);
    let _ = writeln!(out, "Processeur : {} ({} threads)", cpu.get("ProcessorNameString").map(|s| s.trim().to_string()).unwrap_or_default(), cores);
    if let Some((total, free)) = memory_info() {
        let _ = writeln!(out, "Mémoire : {} au total, {} libres", human(total), human(free));
    }
    let _ = writeln!(out, "Windows allumé depuis : {:.1} h", windows_uptime_hours());
    // Cartes graphiques et pilotes (les versions de pilote expliquent beaucoup de pannes de capture)
    // (les 8 premières cartes, lues directement dans le registre : instantané)
    let mut seen = 0;
    for i in 0..8 {
        let v = util::reg_values(&format!(r"HKLM\SYSTEM\CurrentControlSet\Control\Class\{{4d36e968-e325-11ce-bfc1-08002be10318}}\{i:04}"));
        if let Some(desc) = v.get("DriverDesc") {
            let _ = writeln!(out, "Carte graphique : {} · pilote {} ({})", desc, v.get("DriverVersion").cloned().unwrap_or_default(), v.get("DriverDate").cloned().unwrap_or_default());
            seen += 1;
        }
    }
    if seen == 0 {
        let _ = writeln!(out, "Carte graphique : non lisible");
    }
}

fn processes_section(out: &mut String) {
    let all = util::processes();
    let me = std::process::id();
    let _ = writeln!(out, "Rewind : processus {me}");
    if let Some(start) = util::process_start(me) {
        let _ = writeln!(out, "  lancé depuis : {:.1} h", ((util::now() - start) / 3600.0).max(0.0));
    }
    if let Some(mem) = util::process_memory(me) {
        let _ = writeln!(out, "  mémoire : {}", human(mem));
    }
    if let Some((cpu, handles)) = util::process_cpu_handles(me) {
        let _ = writeln!(out, "  temps processeur total : {cpu:.0} s · handles ouverts : {handles}");
    }
    let ff: Vec<_> = all.iter().filter(|p| p.exe.eq_ignore_ascii_case("ffmpeg.exe")).collect();
    let _ = writeln!(out, "  FFmpeg en cours : {}{}", ff.len(), if ff.len() > 3 { "  ← anormal (restes ?)" } else { "" });
    let others = [
        "obs64.exe", "obs32.exe", "streamlabs obs.exe", "medal.exe", "outplayed.exe", "msiafterburner.exe", "rtss.exe",
        "nvcontainer.exe", "nvidia share.exe", "radeonsoftware.exe", "discord.exe", "steam.exe", "cs2.exe",
        "faceit.exe", "faceitclient.exe", "easyanticheat.exe", "vgc.exe", "vgtray.exe", "beservice.exe",
        "wallpaperengine32.exe", "wallpaper64.exe", "lghub.exe", "icue.exe", "razer synapse 3.exe",
    ];
    let mut found: Vec<String> = Vec::new();
    for o in others {
        if all.iter().any(|p| p.exe.eq_ignore_ascii_case(o)) {
            found.push(o.to_string());
        }
    }
    let _ = writeln!(out, "Logiciels qui peuvent interagir (en cours) : {}", if found.is_empty() { "aucun repéré".to_string() } else { found.join(", ") });
}

// ---------------------------------------------------------------------------
// Matériel (écrans, son, encodeurs)
// ---------------------------------------------------------------------------

fn hardware_section(out: &mut String) {
    for m in util::monitors() {
        let _ = writeln!(out, "Écran {} : {}×{} à {} Hz, position {},{}{}", m.name, m.width, m.height, m.refresh, m.x, m.y, if m.primary { " (principal)" } else { "" });
    }
    for (label, input) in [("Sortie audio", false), ("Micro", true)] {
        let list = audio::devices(input);
        if list.is_empty() {
            let _ = writeln!(out, "{label} : aucun appareil trouvé");
        }
        for d in list {
            let _ = writeln!(out, "{label} : {}{}", d.name, if d.default { " (par défaut)" } else { "" });
        }
    }
    match ffmpeg::tools() {
        Ok(t) => {
            let _ = writeln!(out, "FFmpeg : {}", t.ffmpeg.display());
            let version = util::command(&t.ffmpeg)
                .arg("-version")
                .output()
                .map(|o| String::from_utf8_lossy(&o.stdout).lines().next().unwrap_or("").to_string())
                .unwrap_or_default();
            let _ = writeln!(out, "  version : {version}");
        }
        Err(e) => {
            let _ = writeln!(out, "FFmpeg : INTROUVABLE ({e})");
        }
    }
    let _ = writeln!(out, "Encodeurs qui fonctionnent : {}", ffmpeg::working_encoders().join(", "));
}

// ---------------------------------------------------------------------------
// Plantages, gels, événements Windows
// ---------------------------------------------------------------------------

/// Erreurs d'application enregistrées par Windows pour Rewind ou FFmpeg (14 derniers jours).
fn windows_events() -> Vec<String> {
    let query = "*[System[(EventID=1000 or EventID=1002) and TimeCreated[timediff(@SystemTime) <= 1209600000]]]";
    let Ok(output) = util::command(Path::new("wevtutil"))
        .args(["qe", "Application", "/c:80", "/rd:true", "/f:text"])
        .arg(format!("/q:{query}"))
        .output()
    else {
        return Vec::new();
    };
    let text = String::from_utf8_lossy(&output.stdout).to_string();
    text.split("Event[")
        .filter(|b| {
            let l = b.to_ascii_lowercase();
            l.contains("rewind") || l.contains("ffmpeg")
        })
        .take(6)
        .map(|b| b.lines().filter(|l| !l.trim().is_empty()).take(22).collect::<Vec<_>>().join("\n"))
        .collect()
}

struct Crashes {
    reports: Vec<PathBuf>,
    dumps: Vec<PathBuf>,
}

fn list_crashes() -> Crashes {
    let mut reports = Vec::new();
    let mut dumps = Vec::new();
    if let Ok(rd) = fs::read_dir(util::data_dir()) {
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().to_string();
            if name.starts_with("plantage-") && name.ends_with(".txt") {
                reports.push(e.path());
            } else if name.starts_with("gel-") && name.ends_with(".dmp") {
                dumps.push(e.path());
            }
        }
    }
    reports.sort();
    dumps.sort();
    Crashes { reports, dumps }
}

// ---------------------------------------------------------------------------
// FFmpeg
// ---------------------------------------------------------------------------

fn ffmpeg_section(out: &mut String, problems: &mut u32) {
    // Session précédente (archivée au démarrage, avant que le tampon soit vidé)
    let old = util::data_dir().join("ffmpeg-precedent.log");
    if old.is_file() {
        let _ = writeln!(out, "--- Session précédente de FFmpeg (archivée au démarrage) ---");
        let lines = tail_lines(&old, 120);
        for l in &lines {
            let _ = writeln!(out, "{l}");
        }
        *problems += lines.iter().filter(|l| ffmpeg_error_line(l)).count().min(20) as u32;
    }
    let root = util::data_dir().join("buffer");
    let mut logs: Vec<(SystemTime, PathBuf)> = Vec::new();
    if let Ok(rd) = fs::read_dir(&root) {
        for e in rd.flatten() {
            let p = e.path().join("ffmpeg.log");
            if let Ok(m) = fs::metadata(&p).and_then(|m| m.modified()) {
                logs.push((m, p));
            }
        }
    }
    logs.sort_by(|a, b| b.0.cmp(&a.0));
    if logs.is_empty() {
        let _ = writeln!(out, "--- Session en cours : aucun journal FFmpeg (moteur arrêté ou pas encore lancé) ---");
    }
    for (t, p) in logs.iter().take(3) {
        let name = p.parent().and_then(|d| d.file_name()).map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        let size = fs::metadata(p).map(|m| m.len()).unwrap_or(0);
        let _ = writeln!(out, "--- Session en cours {name} (modifié {}, {}) ---", when(*t), human(size));
        let all = tail_lines(p, 4000);
        let errs: Vec<&String> = all.iter().filter(|l| ffmpeg_error_line(l)).collect();
        let _ = writeln!(out, "Lignes d'erreur ou d'avertissement : {}", errs.len());
        *problems += errs.len().min(20) as u32;
        for l in errs.iter().rev().take(40).rev() {
            let _ = writeln!(out, "  ! {l}");
        }
        let _ = writeln!(out, "Dernières lignes :");
        let skip = all.len().saturating_sub(25);
        for l in &all[skip..] {
            let _ = writeln!(out, "  {l}");
        }
    }
}

// ---------------------------------------------------------------------------
// Rapport complet
// ---------------------------------------------------------------------------

pub fn build(live: Live) -> Report {
    let mut problems = 0u32;
    let mut body = String::new();
    let data = util::data_dir();

    // --- Journal de Rewind
    let log = data.join("rewind.log");
    let old_log = data.join("rewind.old.log");
    let log_lines = tail_lines(&log, 6000);
    let old_lines = tail_lines(&old_log, 4000);
    let errors: Vec<&String> = old_lines.iter().chain(log_lines.iter()).filter(|l| is_error(l)).collect();
    let warns: Vec<&String> = old_lines.iter().chain(log_lines.iter()).filter(|l| is_warn(l)).collect();
    let front: Vec<&String> = old_lines.iter().chain(log_lines.iter()).filter(|l| l.contains("[interface]")).collect();
    let crashes = list_crashes();
    let events = windows_events();
    let low_disk = util::free_space(&data).map(|f| f < 5_000_000_000).unwrap_or(false);

    // --- Résumé (en tête : c'est ce qu'on lit en premier)
    let mut summary: Vec<String> = Vec::new();
    if !live.status_error.is_empty() {
        summary.push(format!("Le moteur signale une erreur : {}", live.status_error));
        problems += 1;
    }
    if !crashes.reports.is_empty() {
        summary.push(format!("{} rapport(s) de plantage (voir « Plantages et gels »).", crashes.reports.len()));
        problems += crashes.reports.len() as u32;
    }
    if !crashes.dumps.is_empty() {
        summary.push(format!("{} gel(s) de l'interface détecté(s) par la veille.", crashes.dumps.len()));
        problems += crashes.dumps.len() as u32;
    }
    if !events.is_empty() {
        summary.push(format!("{} erreur(s) d'application enregistrée(s) par Windows (Rewind ou FFmpeg, 14 jours).", events.len()));
        problems += events.len() as u32;
    }
    if !errors.is_empty() {
        summary.push(format!("{} erreur(s) dans le journal.", errors.len()));
        problems += errors.len().min(50) as u32;
    }
    if !warns.is_empty() {
        summary.push(format!("{} avertissement(s) dans le journal.", warns.len()));
        problems += warns.len().min(50) as u32;
    }
    if !front.is_empty() {
        summary.push(format!("{} message(s) d'erreur de l'interface.", front.len()));
    }
    if low_disk {
        summary.push("Moins de 5 Go libres sur le disque de Rewind.".into());
        problems += 1;
    }
    if ffmpeg::tools().is_err() {
        summary.push("FFmpeg est introuvable.".into());
        problems += 1;
    }

    // --- Système et matériel
    section(&mut body, "SYSTÈME");
    system_section(&mut body);
    let _ = writeln!(body, "WebView2 (affichage de l'interface) : {}", live.webview);
    let _ = writeln!(body, "Dossier de données : {}", data.display());
    let _ = writeln!(body, "Disque des données, espace libre : {}", util::free_space(&data).map(human).unwrap_or_else(|| "?".into()));
    let _ = writeln!(body, "Disque des clips, espace libre : {}", util::free_space(Path::new(&live.settings.clip_folder)).map(human).unwrap_or_else(|| "?".into()));

    section(&mut body, "MATÉRIEL");
    hardware_section(&mut body);

    section(&mut body, "PROCESSUS");
    processes_section(&mut body);

    section(&mut body, "ÉTAT DE REWIND");
    let _ = writeln!(body, "{}", live.status);
    // Moteur de touches (Raw Input) : raccourcis et touches à l'écran
    let alive = crate::inputs::ALIVE.load(std::sync::atomic::Ordering::Relaxed);
    let _ = writeln!(body, "Moteur de touches : {}", if alive { "reçoit le clavier et la souris" } else { "aucune touche reçue depuis le démarrage" });
    let _ = writeln!(
        body,
        "Commande vocale : {}",
        match crate::voice::STATE.load(std::sync::atomic::Ordering::Relaxed) {
            0 => "coupée".to_string(),
            1 => "démarrage".to_string(),
            2 => "à l'écoute".to_string(),
            3 => "indisponible (aucun moteur de reconnaissance)".to_string(),
            _ => "erreur".to_string(),
        }
    );
    if let Some(h) = crate::voice::last_heard() {
        let _ = writeln!(body, "Dernière phrase entendue : confiance {:.2}, {}", h.confidence, if h.accepted { "clip lancé" } else { "sous le seuil" });
    }

    section(&mut body, "RÉGLAGES (sans les webhooks)");
    match serde_json::to_value(&live.settings) {
        Ok(mut v) => {
            if let Some(w) = v.get_mut("webhooks") {
                let n = w.as_array().map(|a| a.len()).unwrap_or(0);
                *w = serde_json::json!(format!("{n} webhook(s) configuré(s), adresses masquées"));
            }
            let _ = writeln!(body, "{}", serde_json::to_string_pretty(&v).unwrap_or_default());
        }
        Err(e) => {
            let _ = writeln!(body, "Réglages illisibles : {e}");
        }
    }

    section(&mut body, "COUNTER-STRIKE 2");
    let _ = writeln!(body, "{}", live.cs2);

    section(&mut body, "MISES À JOUR");
    let _ = writeln!(body, "{}", live.update);

    section(&mut body, "BIBLIOTHÈQUE");
    let _ = writeln!(body, "{}", live.library);

    // --- Plantages et gels
    section(&mut body, "PLANTAGES ET GELS");
    if crashes.reports.is_empty() && crashes.dumps.is_empty() && events.is_empty() {
        let _ = writeln!(body, "Aucun plantage, aucun gel, aucune erreur d'application Windows (14 jours).");
    }
    for r in crashes.reports.iter().rev().take(3) {
        let _ = writeln!(body, "--- {} ---", r.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default());
        for l in first_lines(r, 70) {
            let _ = writeln!(body, "{l}");
        }
    }
    for d in crashes.dumps.iter().rev() {
        let size = fs::metadata(d).map(|m| m.len()).unwrap_or(0);
        let t = fs::metadata(d).and_then(|m| m.modified()).map(when).unwrap_or_default();
        let _ = writeln!(body, "Gel : {} ({}, {})", d.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default(), t, human(size));
    }
    for e in &events {
        let _ = writeln!(body, "--- Événement Windows ---\n{e}");
    }

    // --- Journal
    section(&mut body, "ERREURS DU JOURNAL");
    if errors.is_empty() {
        let _ = writeln!(body, "Aucune erreur.");
    }
    for l in errors.iter().rev().take(120).rev() {
        let _ = writeln!(body, "{l}");
    }
    section(&mut body, "AVERTISSEMENTS DU JOURNAL");
    if warns.is_empty() {
        let _ = writeln!(body, "Aucun avertissement.");
    }
    for l in warns.iter().rev().take(150).rev() {
        let _ = writeln!(body, "{l}");
    }
    section(&mut body, "ERREURS DE L'INTERFACE");
    if front.is_empty() {
        let _ = writeln!(body, "Aucune.");
    }
    for l in front.iter().rev().take(80).rev() {
        let _ = writeln!(body, "{l}");
    }

    section(&mut body, "FFMPEG");
    ffmpeg_section(&mut body, &mut problems);

    section(&mut body, "JOURNAL, DERNIÈRES LIGNES (toutes sortes)");
    let skip = log_lines.len().saturating_sub(250);
    for l in &log_lines[skip..] {
        let _ = writeln!(body, "{l}");
    }

    section(&mut body, "FICHIERS DU DOSSIER DE DONNÉES");
    if let Ok(rd) = fs::read_dir(&data) {
        let mut files: Vec<(String, u64, String)> = rd
            .flatten()
            .filter_map(|e| {
                let m = e.metadata().ok()?;
                let name = e.file_name().to_string_lossy().to_string();
                Some((name + if m.is_dir() { "/" } else { "" }, if m.is_dir() { 0 } else { m.len() }, m.modified().map(when).unwrap_or_default()))
            })
            .collect();
        files.sort();
        for (n, s, t) in files {
            let _ = writeln!(body, "{n:<32} {:>10}  {t}", if s > 0 { human(s) } else { String::new() });
        }
    }

    // --- Assemblage : en-tête + résumé en premier
    let mut text = String::new();
    let _ = writeln!(text, "REWIND · RAPPORT DE DIAGNOSTIC");
    let _ = writeln!(text, "Version : {} ({})", env!("CARGO_PKG_VERSION"), if cfg!(debug_assertions) { "développement" } else { "publique" });
    let _ = writeln!(text, "Créé le : {} (UTC{})", chrono::Local::now().format("%Y-%m-%d %H:%M:%S"), chrono::Local::now().format("%:z"));
    let _ = writeln!(text, "Ce rapport ne contient ni mot de passe ni adresse de webhook ; le dossier personnel est remplacé par %USERPROFILE%.");
    section(&mut text, "RÉSUMÉ");
    if summary.is_empty() {
        let _ = writeln!(text, "Rien d'anormal repéré.");
    }
    for s in &summary {
        let _ = writeln!(text, "- {s}");
    }
    if let Some(last) = errors.last() {
        let _ = writeln!(text, "Dernière erreur : {last}");
    }
    text.push_str(&body);

    Report { text: sanitize(&text), problems }
}

/// Avant que le tampon soit vidé au démarrage, garde la fin des journaux FFmpeg de la session précédente.
/// Sans cela, une panne de FFmpeg survenue juste avant la fermeture serait perdue.
pub fn archive_previous_ffmpeg_logs(buffer_root: &Path) {
    let mut logs: Vec<(SystemTime, PathBuf)> = Vec::new();
    if let Ok(rd) = fs::read_dir(buffer_root) {
        for e in rd.flatten() {
            let p = e.path().join("ffmpeg.log");
            if let Ok(m) = fs::metadata(&p).and_then(|m| m.modified()) {
                logs.push((m, p));
            }
        }
    }
    if logs.is_empty() {
        return;
    }
    logs.sort_by(|a, b| b.0.cmp(&a.0));
    let mut text = String::new();
    for (t, p) in logs.iter().take(3) {
        let name = p.parent().and_then(|d| d.file_name()).map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        let _ = writeln!(text, "# {name} (dernière écriture {})", when(*t));
        let lines = tail_lines(p, 400);
        let keep = lines.len().min(120);
        let errs: Vec<&String> = lines.iter().filter(|l| ffmpeg_error_line(l)).collect();
        for l in errs.iter().rev().take(40).rev() {
            let _ = writeln!(text, "! {l}");
        }
        let _ = writeln!(text, "--- fin du journal ---");
        for l in &lines[lines.len() - keep.min(25)..] {
            let _ = writeln!(text, "{l}");
        }
    }
    let _ = fs::write(util::data_dir().join("ffmpeg-precedent.log"), text);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Fabrique un dossier de données avec des erreurs, un plantage, un journal FFmpeg, et vérifie le rapport.
    /// Lancer : REWIND_DATA_DIR=<dossier vide> cargo test rapport -- --nocapture
    #[test]
    fn rapport_complet_et_masque_les_secrets() {
        let dir = std::env::temp_dir().join(format!("rewind-rapport-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("buffer").join("s1")).unwrap();
        // SAFETY: test mono-fil sur cette variable, avant tout appel à data_dir()
        unsafe { std::env::set_var("REWIND_DATA_DIR", &dir) };
        let profile = std::env::var("USERPROFILE").unwrap_or_default();
        fs::write(
            dir.join("rewind.log"),
            format!(
                "2026-11-14 10:00:00 [INFO] Rewind démarre\n2026-11-14 10:00:05 [WARN] commande en échec : Discord a refusé l'envoi (401) https://discord.com/api/webhooks/123456/SECRETTOKEN-abc\n2026-11-14 10:00:09 [ERROR] [interface] TypeError: x is undefined ({profile}\\Videos\\clip.mp4)\n"
            ),
        )
        .unwrap();
        fs::write(dir.join("plantage-20261114-100000.txt"), "Rewind 0.2.3 a planté\nFil : engine\n").unwrap();
        fs::write(dir.join("gel-20261114-100100.dmp"), vec![0u8; 2048]).unwrap();
        fs::write(dir.join("buffer").join("s1").join("ffmpeg.log"), "[error] Impossible d'ouvrir le périphérique\nframe=1\n").unwrap();
        archive_previous_ffmpeg_logs(&dir.join("buffer"));
        let report = build(Live {
            settings: Settings::default(),
            status: "{\"recording\": false}".into(),
            status_error: "FFmpeg s'est arrêté".into(),
            cs2: "{}".into(),
            update: "canal joignable".into(),
            library: "0 éléments".into(),
            webview: "x".into(),
        });
        println!("{}", report.text);
        let t = &report.text;
        assert!(t.contains("RÉSUMÉ"));
        assert!(t.contains("TypeError: x is undefined"), "erreur de l'interface absente");
        assert!(t.contains("Discord a refusé"), "avertissement absent");
        assert!(t.contains("plantage-20261114-100000.txt"), "plantage absent");
        assert!(t.contains("gel-20261114-100100.dmp"), "gel absent");
        assert!(t.contains("Impossible d'ouvrir le périphérique"), "journal FFmpeg absent");
        assert!(!t.contains("SECRETTOKEN"), "le secret du webhook a fuité");
        if profile.len() > 3 {
            assert!(!t.to_ascii_lowercase().contains(&profile.to_ascii_lowercase()), "le dossier personnel a fuité");
        }
        assert!(report.problems >= 5, "problèmes comptés : {}", report.problems);
        let _ = fs::remove_dir_all(&dir);
    }
}
