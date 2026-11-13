# Rewind : documentation développeur

Guide pour compiler, tester et comprendre le code. Pour la présentation de l'application, voir le [README principal](../README.md).

## Architecture

| Partie | Fichier | Rôle |
|---|---|---|
| Capture et encodage | `src-tauri/src/recorder.rs` | FFmpeg `gfxcapture` (Windows Graphics Capture, sans injection), puis NVENC, AMF ou Quick Sync. Segments de 1 s horodatés à l'horloge du PC, assemblés sans réencodage à l'export |
| Audio | `src-tauri/src/audio.rs` | WASAPI : son du PC (sans l'application qui a sa propre piste), micro, une piste par application |
| Counter-Strike 2 | `src-tauri/src/cs2.rs` | Serveur Game State Integration sur `127.0.0.1:12762`, détection des moments, règles Legit et HvH |
| Bibliothèque | `src-tauri/src/library.rs` | Index, miniatures, Corbeille Windows (jamais de suppression définitive) |
| Montage | `src-tauri/src/editor.rs`, `media.rs` | Couper, fusionner, Studio, vertical 9:16, compression Discord |
| Partage | `src-tauri/src/share.rs` | Webhook Discord, copie du fichier pour le presse-papiers |
| Mises à jour | `src-tauri/src/update.rs` | Recherche et installation signées depuis GitHub Releases |
| Veille | `src-tauri/src/watchdog.rs` | Détecte une fenêtre figée, écrit une ligne de journal et une image mémoire `gel-*.dmp` |
| Interface | `src/lib/components/` | Svelte 5. Verre liquide dans `src/lib/glass.ts`, jetons de style dans `src/lib/styles.css` |

Deux règles à connaître :

- **Une commande Tauri sans `async` tourne sur le fil de l'interface.** Elles sont toutes déclarées `#[tauri::command(async)]` pour qu'aucune ne puisse figer la fenêtre en attendant un verrou.
- **Ne jamais appeler `GetWindowTextW` sur la fenêtre au premier plan.** Si c'est la fenêtre de Rewind, Windows attend le fil de l'interface. Le code utilise `InternalGetWindowText`.

## Compiler

Prérequis : Rust stable, Node 20 ou plus, les outils C++ de Visual Studio, et `ffmpeg.exe` et `ffprobe.exe`
(build récente avec `gfxcapture` et l'option `dup_frames`, par exemple « nightly » de gyan.dev) dans `src-tauri/bin/`. Ces deux fichiers ne sont pas versionnés.

```powershell
cd rewind
npm install
npm run tauri dev      # application avec rechargement
npm run dev            # interface seule dans un navigateur, moteur simulé (src/lib/mock.ts)
```

Installeur signé :

```powershell
npm run release        # compile, signe et copie Rewind-setup.exe sur le Bureau
```

La clé privée de signature doit exister dans `%USERPROFILE%\.tauri\rewind-updater.key`. Elle ne va jamais dans le dépôt.
Pour publier une version, voir [`PUBLICATION.md`](../PUBLICATION.md).

## Tests

Tests unitaires (CS2, Studio, réglages, recollage, touches) :

```powershell
cd src-tauri
cargo test --lib
```

Tests de bout en bout. **Toujours** mettre `REWIND_DATA_DIR` vers un dossier de test, jamais vers les vraies données.

| Commande | Vérifie |
|---|---|
| `rewind --selftest <secondes> <dossier>` | Capture et export sans interface |
| `rewind --stress <minutes> <dossier>` | Endurance : clips en continu et en rafale, FFmpeg tué régulièrement, mémoire, processeur, handles |
| `rewind --stitch <dossier> [son]` | Clip à cheval sur une panne (`REWIND_DOUBLE=1` : deux pannes de suite, `REWIND_CUT_END=<s>` : son coupé avant la fin) |
| `rewind --hold <dossier>` | Session complète plus longue que le tampon, avec repères |
| `rewind --studio <clipA> <clipB> <dossier> [musique]` | Exports du Studio : durées image et son, 480p, taille maximale, touches |
| `rewind --media <fichier>` | Pistes, vignettes, forme d'onde |
| `rewind --keys` | Crochets clavier réels |
| `rewind --voice <dossier de .wav>` | Commande vocale : les « oui » déclenchent, les « non » jamais |
| `rewind --replay-cs2 <journal.jsonl>` | Rejoue une partie CS2 enregistrée |
| `rewind --verify-update <setup.exe> <.sig>` | Signature d'une mise à jour, et refus d'un fichier modifié |
| `rewind --check-update <rapport.json> [--download]` | Contrôle le canal GitHub réel, télécharge et vérifie la signature sans rien installer |

Traductions : `node tools/check-i18n.mjs` vérifie que chaque phrase de l'interface a sa version anglaise.

Gel de la fenêtre : si Rewind écrit « interface bloquée » dans son journal, il a aussi enregistré une image mémoire
`gel-*.dmp` dans `%LOCALAPPDATA%\com.rewind.app\`. Elle se lit avec WinDbg : `cdb -z gel.dmp -c "~*k 25; q"`.

## Données

| Quoi | Où |
|---|---|
| Clips | `Vidéos\Rewind\<jeu>\` |
| Réglages, bibliothèque, journal | `%LOCALAPPDATA%\com.rewind.app\` |
| Tampon d'enregistrement | dossier `buffer` dans le dossier de données, vidé au démarrage de l'instance principale |
