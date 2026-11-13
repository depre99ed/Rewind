# Rewind

**Clips instantanés pour Windows.** Rewind enregistre l'écran en continu et garde les dernières secondes
quand tu appuies sur un raccourci. Sur Counter-Strike 2, il clippe tout seul tes meilleurs rounds.

[Télécharger pour Windows](https://github.com/depre99ed/Rewind/releases/latest) · Windows 11 et Windows 10 récent, 64 bits · Français et anglais · [English version](README.en.md)

![La bibliothèque de Rewind : dernier clip à la une, clips rangés par jour](docs/bibliotheque.png)

## Ce que fait Rewind

**Enregistrer sans y penser**
- Un raccourci garde les 30 dernières secondes, un autre les 2 dernières minutes. Les deux durées se règlent.
- Clip sur mesure : un appui pour le début, un appui pour la fin.
- Session complète : toute la partie dans un seul fichier, avec les kills repérés sur la frise.
- Son sur pistes séparées : son du PC, micro, Discord. Chaque application peut avoir sa propre piste.

**Counter-Strike 2**
- Clips automatiques par l'API officielle de Valve (Game State Integration). Aucune injection, aucune lecture de la mémoire du jeu.
- Un seul clip par round, déclenché par ce que tu choisis : nombre de kills, one tap, headshot, couteau, Zeus, collateral, ace.
- Deux profils, Legit et HvH, avec chacun leurs réglages.

![Réglages des clips automatiques de Counter-Strike 2](docs/counter-strike-2.png)

**Monter et partager**
- Studio : frise avec vignettes et forme d'onde, découpe, transitions, musique de fond, volume par piste.
- Formats prêts : 16:9, 9:16 avec fond flou ou recadré, fichiers de 10 ou 20 Mo pour Discord.
- « Copier pour Discord » compresse le clip à la bonne taille et le met dans le presse-papiers : il suffit de coller.

![Le Studio : frise, aperçu et réglages du projet](docs/studio.png)

## Installer

1. Télécharge `Rewind_<version>_x64-setup.exe` depuis la [dernière version](https://github.com/depre99ed/Rewind/releases/latest).
2. Lance-le. L'installation se fait pour ton compte Windows, sans droits administrateur.
3. Windows peut afficher « Windows a protégé votre ordinateur », car l'installeur n'a pas encore de certificat d'éditeur.
   Clique sur **Informations complémentaires** puis **Exécuter quand même**.

Rewind se met ensuite à jour lui-même. Il cherche une nouvelle version au démarrage puis toutes les 4 heures.
Il installe seulement quand tu cliques sur « Installer et relancer ». Chaque mise à jour est signée : une version
qui n'est pas signée par la clé du projet est refusée.

## Raccourcis par défaut

Tous se changent dans Réglages → Raccourcis.

| Raccourci | Action |
|---|---|
| `Alt+F10` | Garder les 30 dernières secondes |
| `Alt+Shift+F10` | Garder les 2 dernières minutes |
| `Alt+F11` | Clip sur mesure : début, puis fin |
| `Alt+F7` | Session complète : début, puis fin |
| `Alt+F8` | Ajouter un repère à la session en cours |
| `Alt+F12` | Capture d'écran |
| `Alt+F9` | Mettre l'enregistrement en pause ou le reprendre |

## Configuration

- **Carte graphique** : NVIDIA (NVENC), AMD (AMF) ou Intel (Quick Sync). Sans encodeur matériel, Rewind passe en 720p à 30 i/s sur le processeur.
- **Par défaut** : 1080p à 60 images par seconde. Sur un écran à 100 Hz, la cadence est calée à 50 i/s pour garder une image régulière.
- **Capture** : Windows Graphics Capture, la même technique que l'outil Capture d'écran de Windows. Rien n'est injecté dans les jeux, donc aucun risque avec les anti-triche.

## Confidentialité

Tes clips restent sur ton PC, dans `Vidéos\Rewind\<jeu>\`. Les réglages et le journal sont dans `%LOCALAPPDATA%\com.rewind.app\`.
Rewind n'a ni compte ni statistiques d'usage. Il ne se connecte à Internet que dans deux cas :
- pour chercher une mise à jour, sur cette page GitHub ;
- pour envoyer un clip à un webhook Discord, si tu en as configuré un.

## Compiler depuis les sources

L'application est dans [`rewind/`](rewind/) : moteur en Rust (Tauri 2), interface en Svelte 5, enregistrement par FFmpeg.
Pour la compilation, les tests et l'architecture, lis [`rewind/README.md`](rewind/README.md).
La publication d'une version est décrite dans [`PUBLICATION.md`](PUBLICATION.md).

## Licence

FFmpeg est fourni avec Rewind sous sa propre licence ([`rewind/src-tauri/bin/FFMPEG-LICENSE.txt`](rewind/src-tauri/bin/FFMPEG-LICENSE.txt)).
La licence du code de Rewind n'est pas encore choisie.
