# E0 : mesures du parcours et du cumul des tailles (17/11/2026)

Étape E0 de [PLAN-ESPACE.md](PLAN-ESPACE.md) : prototype de mesure, sans interface. PC de l'auteur, **sans droits d'administrateur**.

## Ce qui a été fait

- `Index::tailles_cumulees()` ([src/index.rs](../../src/index.rs)) : taille de chaque dossier = somme de ce qu'il contient, en **un seul
  passage à rebours** (les nœuds sont rangés en largeur, un parent précède ses enfants : pas de récursion ni de pile).
- `Index::trouver(chemin)` : nœud d'un chemin, pour les essais.
- Tests : `cumul_des_tailles` (unitaire, lancé et réussi) ; `mesure_espace_e0` (ignoré par défaut, la mesure ci-dessous).

Commande de la mesure : `cargo test --release mesure_espace_e0 -- --ignored --nocapture` (l'outil condense la sortie de `cargo` :
pour lire les lignes `MESURE`, lancer le binaire `target\release\deps\spacefiles-*.exe mesure_espace_e0 --ignored --nocapture`).

## Chiffres mesurés

| Mesure | Résultat |
|---|---|
| Parcours complet des disques fixes (`construire()`, `FindFirstFileExW`, sans admin) | **132 451 ms** (2 min 12 s), un seul disque fixe (C:) |
| Éléments | **2 842 618** (452 369 dossiers, 2 390 249 fichiers) |
| Cumul des tailles par dossier (passage à rebours) | **26 ms** (9 µs pour 1 000 éléments) |
| Mémoire (estimée par capacité) | nœuds 86,7 Mo + noms 55,1 Mo + cumuls 21,7 Mo = **163,5 Mo** |
| `index.bin` sur disque (déjà existant) | 122,5 Mo |

Plus gros dossiers de `C:\` (tailles logiques, sans admin) : `Users` 398,5 Go, `Program Files (x86)` 218,3 Go, `Program Files` 89,2 Go,
`Riot Games` 85,4 Go, `Games` 33,6 Go, `ProgramData` 32,7 Go, `Windows` 31,5 Go, `Autodesk` 30,5 Go, `_acestream_cache_` 10,5 Go.

## Contrôle de justesse

Le cumul de `docs` donne **54 014 155 octets** ; `Get-ChildItem -Recurse -Force -File | Measure-Object Length -Sum` (PowerShell) donne
**683 fichiers, 54 014 155 octets** : identique à l'octet près.

## Espace non expliqué (déjà visible, pour E12)

`C:` : l'index compte **949,9 Go**, Windows annonce **1 001,2 Go** occupés : **51,2 Go (5 %) non expliqués**. Causes probables, non
vérifiées une à une : dossiers exclus par `ignore()` (`WinSxS`, `Installer`, `servicing`, corbeille, `System Volume Information`),
fichiers système sans droits (`pagefile.sys`, `hiberfil.sys`, `swapfile.sys`), tailles logiques et non tailles sur disque, liens
physiques comptés plusieurs fois, dossiers illisibles sans admin.

## Décision

1. **Réutiliser l'index existant** : il garde déjà taille et date de modification par fichier, et le cumul ne coûte que 26 ms et
   21,7 Mo (alloués seulement pendant que la page ESPACE est ouverte). Pas de second parcours à écrire.
2. **Le parcours dure 2 min 12 s** sur ce PC : acceptable en arrière-plan (l'index se reconstruit déjà toutes les 6 h), pas
   acceptable comme attente affichée. L'analyse automatique à l'ouverture (§ 8) doit lire l'index enregistré, jamais relancer un
   parcours devant l'utilisateur ; la page affiche la date de l'index.
3. **Réserve**: tailles logiques seulement ; pour « espace occupé réel » (taille sur disque, liens physiques) et pour les 5 % non
   expliqués, il faudra un second passage ciblé (E12). Le budget mémoire du résident (16 Mo) n'est pas touché : l'index n'est chargé
   que pour une fenêtre, et le cumul n'existe que page ouverte.

## Non vérifié

- La durée du parcours est une mesure unique (une seule exécution, disque en usage normal) ; elle n'est pas répétée.
- Aucun second disque fixe sur ce PC : le temps d'un disque réseau ou externe n'est pas mesuré.
- Mémoire : estimation par capacité des vecteurs, pas le working set mesuré du processus.
