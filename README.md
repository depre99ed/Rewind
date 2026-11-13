# Rewind

Appli Windows de clips instantanés, écrite en Rust (Tauri 2 + Svelte) : elle filme l'écran en continu
et garde les dernières minutes. Un raccourci sauve les dernières secondes (un fou rire sur Discord, une
action en jeu…). Sur Counter-Strike 2, les kills, one-taps, ace, clutchs sont clippés tout seuls.

- `rewind/` : l'application
- [Télécharger la dernière version](https://github.com/depre99ed/Rewind/releases/latest)
- [Publier une mise à jour](PUBLICATION.md)

## Utiliser

Installer : [GitHub Releases](https://github.com/depre99ed/Rewind/releases/latest), ou `Rewind-setup.exe` sur le Bureau après `npm run release`.
(ou lancer directement `rewind/src-tauri/target/release/rewind.exe`, avec `ffmpeg.exe` et `ffprobe.exe` dans un dossier `bin` à côté).

| Raccourci (modifiable) | Action |
|---|---|
| Alt+F10 | Garder les 30 dernières secondes |
| Alt+Shift+F10 | Garder les 2 dernières minutes |
| Alt+F12 | Capture d'écran |
| Alt+F9 | Pause / reprise |

Clips rangés dans `Vidéos\Rewind\<jeu>\`. Réglages, index et journal : `%LOCALAPPDATA%\com.rewind.app\`.

Mises à jour : recherche au démarrage puis toutes les quatre heures ; installation signée depuis
GitHub après un clic sur « Installer et relancer ». La version locale 0.2.0 au canal vide doit
être remplacée manuellement une première fois par une version connectée à GitHub.

## Comment ça marche

| Partie | Fichier | Rôle |
|---|---|---|
| Capture + encodage | `src-tauri/src/recorder.rs` | FFmpeg `gfxcapture` (Windows Graphics Capture, sans injection) → NVENC/AMF/QuickSync → segments de 1 s horodatés à l'horloge du PC |
| Audio | `src-tauri/src/audio.rs` | WASAPI : son du PC (sans l'appli qui a sa piste), micro, une piste par appli (Discord…) |
| Counter-Strike 2 | `src-tauri/src/cs2.rs` | Serveur Game State Integration (API officielle Valve) sur 127.0.0.1:12762 + détection |
| Bibliothèque | `src-tauri/src/library.rs` | Index, miniatures, Corbeille Windows (jamais de suppression définitive) |
| Montage | `src-tauri/src/editor.rs` | Couper, fusionner, vertical 9:16, compression Discord |
| Partage | `src-tauri/src/share.rs` | Webhook Discord, copie du fichier pour Ctrl+V |
| Interface | `src/lib/components/` | Svelte 5 |

## Développer

```powershell
cd rewind
npm install
npm run tauri dev        # appli avec rechargement
npm run release          # installeur + copie sur le Bureau (Rewind-setup.exe)
npm run dev              # interface seule dans un navigateur (moteur simulé, src/lib/mock.ts)
cd src-tauri; cargo test --lib   # tests de la détection CS2
.\target\debug\rewind.exe --selftest 15 <dossier>   # autotest du moteur sans interface
python ..\tools\mesure_synchro.py <clip.mp4> bin\ffmpeg.exe   # mesure du décalage son/image
```

FFmpeg : build « nightly » de gyan.dev (2026-11-11, option `dup_frames` de `gfxcapture` nécessaire),
dans `src-tauri/bin/` (non versionné, 2 fichiers).
