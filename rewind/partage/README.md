# Rewind : guichet du partage entre amis

Petit service Cloudflare (Worker + base D1 + stockage R2) qui sert de **boîte aux lettres** au partage de
clips entre amis. Rewind (`src-tauri/src/amis.rs`) y dépose un clip, le Rewind de l'ami le relève ; le
fichier est supprimé dès réception, au plus tard après 7 jours.

- **Aucun compte** : chaque Rewind a une paire de clés Ed25519 et signe ses demandes
  (`MÉTHODE \n chemin \n heure \n empreinte SHA-256 du corps`, en-têtes `x-rewind-*`).
  Depuis la 0.4.0, cette clé est **dérivée de l'identifiant du PC** (MachineGuid de Windows + UUID SMBIOS de la
  carte mère, jamais envoyés) : réinstaller Rewind ou effacer ses données redonne le même compte.
- **Aucun secret** : le Worker écrit dans R2 par sa liaison ; il n'y a ni clé d'accès ni mot de passe.
- **Stocké** : pseudo, code ami, liens d'amitié, liste des envois ; les vidéos et miniatures le temps du trajet.
- **Limites** (`LIMITS` dans `src/index.ts`) : 500 Mo par clip, 2 Go envoyés par jour et par personne,
  200 amis, morceaux de 50 Mo, 20 inscriptions par jour et par adresse IP.

- **Profils** : photo (JPEG carré 256 px préparé par Rewind, `avatars/<id>` dans R2, visible des amis et des
  demandes en cours seulement), présence « en ligne » (amis seulement, désactivable ; écrite toutes les 5 min).
- **Règles du stockage** : `clips/` et `thumbs/` supprimés après 7 jours ; les photos (`avatars/`) restent.
- **Clé de secours** : une seconde clé, aléatoire (`RWK-…` côté Rewind, partie publique dans `users.recovery`).
  `POST /v1/recover` (preuve de la clé de secours + preuve de la clé du PC) rattache le compte à un nouveau PC ;
  l'éventuel compte que ce PC avait déjà est supprimé. `POST /v1/me/recovery` la remplace.
- **Passage à la clé du PC** (comptes d'avant la 0.4.0) : `POST /v1/me/key`, signé par l'ancienne clé avec la preuve
  de la nouvelle ; l'ancienne clé devient la clé de secours, donc une `RWK-` déjà notée reste valable.

## Base : création et migrations

`schema.sql` crée une base neuve complète. Sur une base existante, **une fois chacune**, dans l'ordre :
`migrations/0002_profils.sql`, `migrations/0003_cle_pc.sql`. En ligne : appliquées le 2026-11-15.

## Commandes (PowerShell : `npx.cmd`, pas `npx`)

```powershell
cd rewind\partage
npx.cmd --yes wrangler@4.162.0 deploy                                        # mettre en ligne
npx.cmd --yes wrangler@4.162.0 d1 execute rewind-amis --remote --file schema.sql   # tables (sans risque, IF NOT EXISTS)
npx.cmd --yes wrangler@4.162.0 tail                                          # journal en direct
```

## Tests

```powershell
npx.cmd --yes wrangler@4.162.0 d1 execute rewind-amis --local --file schema.sql
npx.cmd --yes wrangler@4.162.0 dev --local --port 8787     # simulation locale (dossier .wrangler, jamais publié)
node test/essai.mjs http://127.0.0.1:8787                   # guichet seul : 2 faux utilisateurs, refus, blocage…
node test/essai.mjs https://rewind-partage.rewind-clips.workers.dev   # même test en ligne (supprime ses faux utilisateurs)
```

`test/appli.mjs` et `test/ecran.mjs` pilotent un Rewind d'essai (WebView2 avec
`--remote-debugging-port=9333`, `REWIND_SHARE_URL=http://127.0.0.1:8787`) face à un faux ami.

## Lire les signalements d'abus

```powershell
npx.cmd --yes wrangler@4.162.0 d1 execute rewind-amis --remote --command "SELECT * FROM reports ORDER BY created DESC LIMIT 20"
```
