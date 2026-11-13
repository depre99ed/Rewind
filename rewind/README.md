# Rewind

Clips instantanés pour Windows, légers et ouverts. Rewind filme en continu (jeux, Discord, bureau)
et garde les dernières secondes quand tu appuies sur un raccourci. En Rust (Tauri 2) + Svelte 5.

*Instant clips for Windows, lightweight and open. Rewind records continuously (games, Discord, desktop)
and keeps the last seconds when you press a hotkey. Interface in French and English.*

## Ce que fait Rewind

- **Capture sans injection** : Windows Graphics Capture + encodeur de la carte graphique (NVENC, AMF,
  QuickSync ; x264 en secours). Cadence calée sur l'écran (100 Hz → 50 i/s) pour une image régulière.
- **Son sur pistes séparées** : son du PC, micro, Discord (chaque appli peut avoir sa piste), mixage en plus.
  Anti-bruit du micro en option.
- **Raccourcis** : clip court, clip long, clip sur mesure (début / fin), session complète (un fichier,
  kills repérés), repère, capture d'écran, pause. Bandeau des raccourcis au lancement d'un jeu.
- **Counter-Strike 2** : clips automatiques par l'API officielle de Valve (Game State Integration),
  un clip par round (seuil réglable), séquences en deathmatch / HvH, one tap, collateral, ace…
- **Studio** : frise avec vignettes et forme d'onde, glisser-déposer, aimant, vitesse, transitions,
  musique de fond (fondus, atténuation automatique), volume par piste, touches clavier/souris incrustées,
  modèles TikTok / YouTube / Discord 10–20 Mo.
- **Partage** : « Copier pour Discord » compresse sous la taille voulue puis met le fichier dans le presse-papiers.
- **Options** : touches clavier/souris à l'écran (touches de jeu seulement, jeu au premier plan seulement),
  commande vocale « Rewind clip » (hors ligne, reconnaissance vocale de Windows).
- **Robustesse** : chien de garde, reprise en ~2 s après une panne de l'enregistreur, clips recollés à cheval
  sur un redémarrage, pause si le disque est presque plein, réglages et bibliothèque réparés s'ils sont abîmés.
- **Mises à jour signées** (GitHub Releases) : une version non signée par la clé du projet est refusée.

Rien n'est envoyé sur Internet (sauf si tu utilises un webhook Discord ou la recherche de mise à jour).

## Compiler

Prérequis : Rust stable, Node 20+, et `ffmpeg.exe` + `ffprobe.exe` (build récente avec `gfxcapture`)
dans `src-tauri/bin/` (non versionnés).

```bash
npm install
npm run tauri dev
```

Installeur signé : `npm run release` (clé existante requise dans
`%USERPROFILE%\.tauri\rewind-updater.key`, génération de `latest.json`, copie sur le Bureau).
Publication complète : `npm run release -- -Version 0.2.2 -Publish -Notes "Corrections"`.
Voir [le guide de publication](../PUBLICATION.md) pour les prérequis et les vérifications.

## Tests

Tests unitaires : `cargo test` dans `src-tauri` (CS2, Studio, réglages, recollage, touches…).

Tests de bout en bout (mettre `REWIND_DATA_DIR` vers un dossier de test, jamais vers les vraies données) :

| Commande | Vérifie |
|---|---|
| `rewind --stress <minutes> <dossier>` | endurance : clips en continu et en rafale, FFmpeg tué régulièrement, mémoire, CPU, handles |
| `rewind --stitch <dossier> [son]` | clip à cheval sur une panne (`REWIND_DOUBLE=1` : deux pannes de suite) |
| `rewind --hold <dossier>` | session complète plus longue que le tampon, avec repères |
| `rewind --studio <clipA> <clipB> <dossier> [musique]` | exports du Studio (durées image ET son, 480p, taille max, touches) |
| `rewind --media <fichier>` | pistes, vignettes, forme d'onde |
| `rewind --keys` | crochets clavier réels (Maj / Ctrl simulés) |
| `rewind --voice <dossier de .wav>` | commande vocale (oui-* doit déclencher, non-* jamais) |
| `rewind --verify-update <setup.exe> <.sig>` | signature des mises à jour |
| `rewind --replay-cs2 <journal.jsonl>` | rejoue une partie CS2 enregistrée |

Traductions : `node tools/check-i18n.mjs` vérifie que chaque phrase de l'interface a sa version anglaise.

## Licence

À confirmer par l'auteur (le `package.json` indique MIT). FFmpeg est distribué à côté de Rewind sous sa propre
licence (`src-tauri/bin/FFMPEG-LICENSE.txt`).
