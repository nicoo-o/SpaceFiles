# Décisions de l'auteur et suite des travaux (10/11/2026)

État de départ : lots 14 à 17 faits, commités (`3443f5f` lot 16, `beb281b` lot 17) et installés ; 594 tests ; exe 3,70 Mo ; démarrage chaud 281 ms. Liste de rodage à la souris : `docs/ui/rodage-f1-2026-11-09.md`.

## Décisions prises le 10/11/2026
| Sujet | Décision |
|---|---|
| Ordre | 1. Corriger les 3 écarts visuels du lot 17 ; 2. lot 15D (25 idées) ; le rodage F1 se fait en parallèle par l'auteur |
| Étiquettes | Garder `Alt+1/2/3` ; **faire la bulle d'info « Étiquette N »** maintenant (nouvelle zone cliquable dans le hit-test, à vérifier à la souris) |
| Budget exe | Accepter 3,7 Mo (pas de `opt-level = "s"` : coût en fluidité 240 Hz) |
| « Raccourci vers… » | **À faire** : opération `.lnk` dans la palette (passer par l'API shell, annulable) |
| File d'attente | **À faire** : la palette met en file les commandes au lieu de refuser pendant une opération |
| Hooks d'essai | **À retirer** : `SPACEFILES_FILTRE_OUVERT`, `SPACEFILES_PALETTE_OUVERTE`, modificateurs postés. Conséquence : les captures sans souris perdent ces raccourcis ; prévoir une autre façon (touches postées normales) ou garder `SPACEFILES_DPI` |
| Ce PC | **À faire** : sections Cloud et Serveurs comme la maquette |

## Décisions déjà actées avant
- Idées 13 (icônes de type colorées) et 14 (touches en pastilles) : abandonnées, hors direction artistique.
- Idée 19 (langage naturel) : reportée à un lot IA (modèle, clé, confidentialité à décider).
- Alt+Espace : appartient à SpaceNotch (D-01), `Ctrl+K` ouvre la palette.
- Stockage des alias, recettes, recherches et espaces : fichier de données utilisateur à côté des groupes (défaut du brief 15D).

## Plan de travail (chaque lot : commit, build release, `installer.ps1`, liste à tester)
1. **Lot 18 : correctifs du lot 17** : aperçu Espace en carte flottante (`dessiner_galerie`), statut tronqué du panneau de droite, titre de palette coupé sur le trait d'union (`dessiner_apercu_palette`) ; bulle d'étiquette ; retrait des hooks d'essai (adapter `lot16/17-capture.ps1`).
2. **Lot 19 : Ce PC et palette** : sections Cloud et Serveurs ; « Raccourci vers… » (.lnk) ; file d'attente des opérations de la palette.
3. **Lot 15D : centre de commandes** (brief `docs/prompts/2026-11-09-lot-15d-centre-commandes.md`), points A à E.
4. **Lot IA (plus tard)** : langage naturel.

## Décisions complémentaires du 10/11/2026 (questions posées après coup)
| Sujet | Décision |
|---|---|
| Œil de nuit | Blanc |
| Mouvement réduit, changement de dossier | Coupe nette, sans fondu |
| `taille>` | Base 1024 |
| `modif:` | Les deux : 4 chiffres = année, petit nombre = derniers jours |
| Stockage alias, recettes, recherches, espaces | Fichiers à côté des groupes (`%APPDATA%\SpaceFiles`), lecture tolérante |
| Groupes comme destination | Non, pas maintenant |
| Compresser | Ajouter une **destination** au choix de l'archive (zip reste le format ; pas de 7z, pas de dépendance) |
| Étiquettes dans la grille | Bas gauche de la vignette (comme livré) |
| Alt+Espace | Non : Ctrl+K seulement (SpaceNotch garde Alt+Espace, D-01) |
| ZimaOS, Google Drive, Dropbox, iCloud | Vérifiés par l'auteur au rodage F1 sur ses vrais comptes (à ajouter à `docs/ui/rodage-f1-2026-11-09.md`) |
| Comparaison au pixel avec la maquette | Après le lot 15D, une seule recette finale |
| Budgets (démarrage froid, défilement) | Remesurés au lot 18 avec l'exe final, chiffres avant/après |

## Plan mis à jour
- **Lot 18** : 3 écarts visuels + bulle d'étiquette + retrait des hooks d'essai + remesure des budgets + `taille>` 1024 et `modif:` année/jours (vérifier l'existant) + œil de nuit blanc + coupe nette en mouvement réduit.
- **Lot 19** : Ce PC (Cloud, Serveurs) + « Raccourci vers… » (.lnk) + file d'attente des opérations de la palette + compresser avec destination.
- **Lot 15D** puis **recette finale** (comparaison à la maquette) puis lot IA.

## Encore ouvert
- Dossier ouvert en tête des récents de destination ; note ToolHelp à vérifier dans `PROJECT_REBOOT_MASTERPLAN.md`.
