fn main() {
    // Sans ça, Cargo garde en cache l'icône intégrée à l'exe quand on change de logo.
    for f in ["icons/icon.ico", "icons/32x32.png", "icons/128x128.png", "icons/128x128@2x.png"] {
        println!("cargo:rerun-if-changed={f}");
    }
    println!("cargo:rerun-if-changed=tauri.conf.json");
    tauri_build::build()
}
