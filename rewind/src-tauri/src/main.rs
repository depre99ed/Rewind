// Pas de console en version finale
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.get(1).map(String::as_str) == Some("--check-update") {
        let Some(out) = args.get(2) else { std::process::exit(2) };
        rewind_lib::probe_update(out.into(), args.iter().any(|a| a == "--download"));
        return;
    }
    if args.get(1).map(String::as_str) == Some("--selftest") {
        let seconds = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(10);
        let out = args.get(3).map(std::path::PathBuf::from).unwrap_or_else(std::env::temp_dir);
        std::process::exit(rewind_lib::selftest(seconds, out));
    }
    if args.get(1).map(String::as_str) == Some("--discord") {
        let file = std::path::PathBuf::from(args.get(2).cloned().unwrap_or_default());
        let mb = args.get(3).and_then(|s| s.parse().ok()).unwrap_or(20.0);
        let out = args.get(4).map(std::path::PathBuf::from).unwrap_or_else(std::env::temp_dir);
        std::process::exit(rewind_lib::discord_test(file, mb, out));
    }
    if args.get(1).map(String::as_str) == Some("--studio") {
        let a = args.get(2).cloned().unwrap_or_default();
        let b = args.get(3).cloned().unwrap_or_default();
        let out = args.get(4).map(std::path::PathBuf::from).unwrap_or_else(std::env::temp_dir);
        std::process::exit(rewind_lib::studio_test(a, b, out, args.get(5).cloned()));
    }
    if args.get(1).map(String::as_str) == Some("--verify-update") {
        let file = args.get(2).map(std::path::PathBuf::from).unwrap_or_default();
        let sig = args.get(3).map(std::path::PathBuf::from).unwrap_or_default();
        std::process::exit(rewind_lib::verify_update_test(file, sig));
    }
    if args.get(1).map(String::as_str) == Some("--voice") {
        let dir = args.get(2).map(std::path::PathBuf::from).unwrap_or_else(std::env::temp_dir);
        std::process::exit(rewind_lib::voice_test(dir));
    }
    if args.get(1).map(String::as_str) == Some("--toast-demo") {
        std::process::exit(rewind_lib::toast_demo(args.get(2).cloned().unwrap_or_default()));
    }
    if args.get(1).map(String::as_str) == Some("--keys") {
        std::process::exit(rewind_lib::keys_test());
    }
    if args.get(1).map(String::as_str) == Some("--render") {
        let spec = std::path::PathBuf::from(args.get(2).cloned().unwrap_or_default());
        let out = args.get(3).map(std::path::PathBuf::from).unwrap_or_else(std::env::temp_dir);
        std::process::exit(rewind_lib::render_test(spec, out));
    }
    if args.get(1).map(String::as_str) == Some("--detourage") {
        let a = |i: usize| std::path::PathBuf::from(args.get(i).cloned().unwrap_or_default());
        std::process::exit(rewind_lib::detourage_test(a(2), a(3)));
    }
    if args.get(1).map(String::as_str) == Some("--sounds") {
        // Écrit les sons de clip en .wav (pour les écouter hors de l'appli)
        let dir = std::path::PathBuf::from(args.get(2).cloned().unwrap_or_default());
        std::process::exit(rewind_lib::sounds_test(dir));
    }
    if args.get(1).map(String::as_str) == Some("--medal-import") {
        // Test : copie les clips Medal vers un dossier (REWIND_DATA_DIR vers un dossier de test)
        let dest = std::path::PathBuf::from(args.get(2).cloned().unwrap_or_default());
        std::process::exit(rewind_lib::medal_import_test(dest));
    }
    if args.get(1).map(String::as_str) == Some("--medal-detect") {
        // Diagnostic : ce que Rewind trouve de Medal sur ce PC
        std::process::exit(rewind_lib::medal_detect_test());
    }
    if args.get(1).map(String::as_str) == Some("--input-helper") {
        // Assistant lancé en administrateur : relaie clavier et souris des jeux lancés en administrateur
        std::process::exit(rewind_lib::input_helper());
    }
    if args.get(1).map(String::as_str) == Some("--watch-keys") {
        let seconds = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(5);
        std::process::exit(rewind_lib::watch_keys(seconds));
    }
    if args.get(1).map(String::as_str) == Some("--hold") {
        let out = args.get(2).map(std::path::PathBuf::from).unwrap_or_else(std::env::temp_dir);
        std::process::exit(rewind_lib::hold_test(out));
    }
    if args.get(1).map(String::as_str) == Some("--raccourcir") {
        let path = args.get(2).cloned().unwrap_or_default();
        std::process::exit(rewind_lib::shorten_test(path));
    }
    if args.get(1).map(String::as_str) == Some("--musique") {
        let url = args.get(2).cloned().unwrap_or_default();
        std::process::exit(rewind_lib::music_test(url));
    }
    if args.get(1).map(String::as_str) == Some("--killfeed") {
        let path = args.get(2).cloned().unwrap_or_default();
        std::process::exit(rewind_lib::killfeed_test(path));
    }
    if args.get(1).map(String::as_str) == Some("--media") {
        let path = args.get(2).cloned().unwrap_or_default();
        std::process::exit(rewind_lib::media_test(path));
    }
    if args.get(1).map(String::as_str) == Some("--stitch") {
        let out = args.get(2).map(std::path::PathBuf::from).unwrap_or_else(std::env::temp_dir);
        let audio = args.get(3).map(String::as_str) == Some("son");
        std::process::exit(rewind_lib::stitch_test(out, audio));
    }
    if args.get(1).map(String::as_str) == Some("--stress") {
        let minutes = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(10);
        let out = args.get(3).map(std::path::PathBuf::from).unwrap_or_else(std::env::temp_dir);
        std::process::exit(rewind_lib::stress(minutes, out));
    }
    if args.get(1).map(String::as_str) == Some("--replay-cs2") {
        let file = std::path::PathBuf::from(args.get(2).cloned().unwrap_or_default());
        std::process::exit(rewind_lib::replay_cs2(file));
    }
    // Un Rewind tourne déjà (fenêtre fermée, enregistrement en fond) : il ouvre sa fenêtre et on s'arrête.
    if rewind_lib::wake_running_instance() {
        return;
    }
    // Rewind tourne en administrateur (sinon Windows lui cache les touches des jeux lancés en admin) :
    // relancé par la tâche planifiée, ou première demande à Windows. true = l'instance élevée prend le relais.
    if rewind_lib::start_elevated() {
        return;
    }
    rewind_lib::run()
}
