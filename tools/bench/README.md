# Mesures de SpaceFiles (tools/bench)

Tous les scripts mesurent l'exe release (`target\release\spacefiles.exe`), dans un faux profil (`APPDATA`/`LOCALAPPDATA`
redirigés, `SPACEFILES_SANS_REGISTRE=1`), sans souris ni vol de focus, et ne ferment que les processus qu'ils ont lancés.
Lancer : `powershell -NoProfile -ExecutionPolicy Bypass -File tools\bench\<script>.ps1 [options]`. `commun.ps1` est la bibliothèque commune.

- `construire.ps1` : durée de `cargo build --release` (complète, sans changement) dans un dossier cible à part, taille de l'exe.
- `resident.ps1 -Essais 5 -Repos 60` : résident au repos (mémoire privée, working set, CPU, changements de contexte, fils, handles).
- `lancement.ps1 -Froid 10 -Chaud 10` : création du processus jusqu'à l'apparition de la fenêtre (pas la première image).
- `fenetre.ps1` : mémoire de la fenêtre sur 1 000 / 10 000 / 100 000 éléments, et défilement (crochet `SPACEFILES_MESURE_DEFILEMENT`).
- `resident-soak.ps1 -Duree 60` : test de durée du résident (CSV, pente en Mo/h, handles) avec activité par le tube.
