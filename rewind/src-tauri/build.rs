use std::path::{Path, PathBuf};

fn main() {
    // Sans ça, Cargo garde en cache l'icône intégrée à l'exe quand on change de logo.
    for f in ["icons/icon.ico", "icons/32x32.png", "icons/128x128.png", "icons/128x128@2x.png"] {
        println!("cargo:rerun-if-changed={f}");
    }
    println!("cargo:rerun-if-changed=tauri.conf.json");

    // ONNX Runtime (détourage) arrive compilé avec DirectML et Direct3D 12, que Rewind n'utilise jamais (le
    // détourage tourne sur le processeur). Importés normalement, Rewind ne démarrait PAS DU TOUT sous Windows 10
    // 1809 à 1909 (DirectML absent ou trop ancien) : on ne les charge qu'au premier appel, c'est-à-dire jamais.
    // (audit Windows 10 du 2026-11-25)
    println!("cargo:rustc-link-arg=/DELAYLOAD:DirectML.dll");
    println!("cargo:rustc-link-arg=/DELAYLOAD:d3d12.dll");
    println!("cargo:rustc-link-lib=delayimp");

    copy_vc_runtime();
    tauri_build::build()
}

/// ONNX Runtime a aussi besoin du runtime C++ de Visual Studio (MSVCP140.dll). Un Windows 10 sans le
/// « redistribuable Visual C++ », ou avec une version trop ancienne, refusait de lancer Rewind (ou plantait au
/// premier détourage). Les DLL redistribuables de Visual Studio sont livrées à côté de rewind.exe
/// (déploiement local autorisé par Microsoft) : copiées ici dans `vcrt/`, puis installées par le bundle.
fn copy_vc_runtime() {
    const FILES: [&str; 4] = ["msvcp140.dll", "msvcp140_1.dll", "vcruntime140.dll", "vcruntime140_1.dll"];
    let out = Path::new("vcrt");
    let Some(src) = find_crt() else {
        if FILES.iter().all(|f| out.join(f).is_file()) {
            return;
        }
        panic!(
            "Runtime Visual C++ introuvable : installe les « Outils de génération C++ » de Visual Studio (dossier \
             VC\\Redist\\MSVC\\…\\x64\\Microsoft.VC*.CRT) ou copie {FILES:?} dans src-tauri/vcrt."
        );
    };
    let _ = std::fs::create_dir_all(out);
    for f in FILES {
        let from = src.join(f);
        let to = out.join(f);
        let same = std::fs::metadata(&from).ok().map(|m| m.len()) == std::fs::metadata(&to).ok().map(|m| m.len());
        if !same && let Err(e) = std::fs::copy(&from, &to) {
            panic!("copie de {} impossible : {e}", from.display());
        }
    }
}

/// Dossier `…\VC\Redist\MSVC\<version>\x64\Microsoft.VC*.CRT` le plus récent.
fn find_crt() -> Option<PathBuf> {
    let mut roots: Vec<PathBuf> = Vec::new();
    if let Ok(d) = std::env::var("VCToolsRedistDir") {
        roots.push(PathBuf::from(d));
    }
    for base in [r"C:\Program Files\Microsoft Visual Studio", r"C:\Program Files (x86)\Microsoft Visual Studio"] {
        // <base>\<année>\<édition>\VC\Redist\MSVC\<version>
        for year in dirs(Path::new(base)) {
            for edition in dirs(&year) {
                roots.extend(dirs(&edition.join(r"VC\Redist\MSVC")));
            }
        }
    }
    let mut found: Vec<PathBuf> = roots
        .iter()
        .flat_map(|r| dirs(&r.join("x64")))
        .filter(|d| d.file_name().is_some_and(|n| n.to_string_lossy().starts_with("Microsoft.VC") && n.to_string_lossy().ends_with(".CRT")))
        .filter(|d| d.join("msvcp140.dll").is_file() && d.join("msvcp140_1.dll").is_file())
        .collect();
    // Le plus récent : le numéro de version est dans le chemin (14.51.36231…)
    found.sort_by_key(|d| d.to_string_lossy().to_string());
    found.pop()
}

fn dirs(p: &Path) -> Vec<PathBuf> {
    std::fs::read_dir(p).map(|r| r.flatten().map(|e| e.path()).filter(|p| p.is_dir()).collect()).unwrap_or_default()
}
