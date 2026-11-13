# Publier une mise à jour de Rewind

Le dépôt public est [depre99ed/Rewind](https://github.com/depre99ed/Rewind).
Le code, l'installeur et le manifeste de mise à jour sont hébergés sur GitHub.
Aucun serveur supplémentaire, compte utilisateur ni jeton GitHub dans l'application.

## Une commande depuis ce PC

Depuis le dossier `rewind`, après les modifications :

```powershell
npm run release -- -Version 0.2.2 -Publish -Notes "Description des changements"
```

Choisir un numéro supérieur à la dernière version publiée. Ne pas réutiliser un numéro.
Sans `-Publish`, la commande compile et copie l'installeur sur le Bureau sans publier.

La commande synchronise les versions, vérifie Svelte et les traductions, exécute les tests Rust,
compile l'installeur, le signe et teste le refus d'un fichier modifié. Elle exporte ensuite
les sources dans un checkout de publication séparé, publie une release contenant l'EXE,
sa signature et `latest.json`, puis télécharge la version publique avec le vrai plugin
de mise à jour et vérifie sa signature. Les fichiers sont chargés en brouillon avant publication.

Prérequis : `gh auth login` avec le compte propriétaire, Rust, Node, outils C++ Windows,
FFmpeg dans `rewind/src-tauri/bin`, et la clé privée existante dans
`%USERPROFILE%\.tauri\rewind-updater.key`. Conserver cette clé : les installations existantes
n'accepteront pas une clé différente. Le script transmet son chemin au compilateur sans l'afficher.

## Ce que reçoit l'utilisateur

Rewind cherche une version supérieure 40 secondes après le démarrage, puis toutes les quatre heures
si « Rechercher automatiquement » est activé. Une indication apparaît en haut de la fenêtre ;
elle ouvre directement Réglages → Démarrage. L'utilisateur choisit « Installer et relancer ».
L'installeur est téléchargé depuis GitHub et sa signature est vérifiée avant installation.
Un ordinateur hors ligne continue à fonctionner ; la recherche sera réessayée au prochain passage.

Canal : `https://github.com/depre99ed/Rewind/releases/latest/download/latest.json`.
Une simple modification du code ou un tag seul ne suffit pas : la release doit être publiée,
stable, et contenir les trois fichiers générés. La version locale 0.2.0 distribuée avant le branchement
GitHub nécessite une première installation manuelle de 0.2.1 (son canal était vide).

## Vérifier sans installer

```powershell
.\src-tauri\target\release\rewind.exe --check-update C:\chemin\rapport.json --download
```

Ce mode ne lance ni enregistrement ni interface, ne lit pas les réglages personnels et n'installe rien.
Il contrôle la comparaison normale des versions et télécharge la version publiée même si elle est
identique à la version courante, afin de tester la signature. Cela valide le canal et les octets reçus,
pas un remplacement réel de l'application sur une autre machine.

## Sources publiées

Le dépôt local historique contient des essais privés. Ne pas le pousser directement.
`tools/export-public.ps1` publie uniquement les sources de `rewind`, les README, `.gitignore`
et ce guide. Il exclut les réglages locaux des assistants et de l'éditeur ; captures, profils de navigateur,
enregistrements, journaux et historique de travail restent sur le PC. Les versions successives
conservent un historique public propre dans GitHub. Les checkouts sont dans `.release/` (ignoré).

Références : [Tauri updater](https://v2.tauri.app/plugin/updater/),
[liens vers les versions GitHub](https://docs.github.com/en/repositories/releasing-projects-on-github/linking-to-releases).
