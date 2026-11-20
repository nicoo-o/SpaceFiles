# Plan : la page ESPACE, nettoyeur et analyseur d'espace de SpaceFiles (écrit le 17/11/2026)

Statut : **plan écrit avec les réponses de l'auteur du 17/11/2026 et une recherche sur internet ; rien n'est codé.** Démarrage
seulement sur son accord, **une étape à la fois** (règle n° 1 d'`AGENTS.md`), chacune se terminant seule : build, tests, rapport
avec les chiffres mesurés. Ce chantier est indépendant du [chantier terminal](../terminal/PLAN-TERMINAL.md) ; l'ordre est
décidé (§ 8) : E0, puis le terminal, puis la suite d'ESPACE.

## 1. Ce que veut l'auteur (réponses du 17/11/2026)

| Sujet | Décision |
|---|---|
| Idée de départ | Un « nettoyeur de mémoire » qui nettoie le PC **automatiquement**, mais qui, comme WizTree, **montre ce qui prend de la place** et permet de **désinstaller**. « On y a déjà un peu travaillé au début de l'app » : voir § 3. |
| Périmètre | **Disque + RAM + démarrage** (le plus large des trois choix proposés). |
| Automatisme | **Analyse automatique, l'auteur valide tout** : SpaceFiles mesure en arrière-plan et propose (« 14 Go récupérables ») ; rien n'est supprimé sans son clic. |
| Visualisation | **Barres + carte en rectangles** : arbre de dossiers trié par taille avec barres de proportion, et en bascule la carte (treemap) aux couleurs Nothing. |
| Vitesse | **Sans administrateur d'abord ; un bouton « Analyse rapide (admin) »** lit le disque en quelques secondes (UAC à chaque fois). |
| Désinstallation | **Programmes + applis du Store + restes** : désinstalleur officiel, puis scan des restes (dossiers, registre) que l'auteur coche avant suppression (comme Geek Uninstaller). |
| Emplacement | **Une page « ESPACE » à part entière** (un nouvel `Emplacement`, comme Réglages ou Corbeille), ouverte par Ce PC (carte d'un disque), la palette et le volet. |
| Fonctions en plus (les quatre proposées) | (1) gros fichiers et âge des fichiers ; (2) caches et dossiers de développeur (`target`, `node_modules`…) ; (3) comparaison avec la dernière analyse ; (4) Windows : composants de mise à jour, hibernation, fichiers OneDrive en ligne seulement. Les doublons existent déjà : on les relie. |
| Suppression | **Corbeille d'abord, définitif si impossible** : tout passe par `IFileOperation` quand c'est possible ; les caches 100 % régénérables sont supprimés directement, avec résumé et annulation de 10 s quand elle existe. |

## 2. Ce que dit internet (recherche du 17/11/2026, sources en fin de document)

| Constat | Source | Ce que ça change pour nous |
|---|---|---|
| WizTree est rapide parce qu'il lit la table MFT de NTFS directement ; WinDirStat parcourt les dossiers normalement ; SpaceSniffer est « treemap d'abord », portable. Les chiffres de vitesse viennent des éditeurs : aucune mesure indépendante sur une même machine trouvée. | comparatifs tiers | Notre promesse de vitesse se **mesure sur le PC de l'auteur** (étape E0), pas sur les slogans. |
| TreeSize apprécié pour sa **hiérarchie familière** ; la carte seule est jugée moins lisible. | comparatif | Confirme le choix « barres + carte » : l'arbre est la vue de référence, la carte est la bascule. |
| TreeSize montre l'espace **par âge des fichiers** ; DiskRecon aussi (vue « File Dates »). | jam-software, blacksunsoftware | Fonction (1) : « 18 Go non ouverts depuis plus d'un an ». |
| **Comparer deux analyses** (ajoutés / supprimés / changés) existe chez OO DiskCommander ; la comparaison d'historique est réservée aux éditions payantes de TreeSize et FolderSizes. | oo-software, jam-software | Fonction (3) : un atout gratuit, donc différenciant. |
| TreeSize tient compte des **liens physiques (hardlinks)** et des flux alternatifs ; WinDirStat distingue taille logique et taille physique. | jam-software, neowin | À respecter : ne pas compter deux fois un même fichier ; afficher « taille sur disque ». |
| Geek Uninstaller : après le désinstalleur officiel, **scan des restes** (fichiers, dossiers, registre) proposé. Bulk Crap Uninstaller : désinstallation par lot. Un test d'un concurrent (jv16, à prendre avec prudence) compte beaucoup plus de restes après BCU qu'après Geek. | ghacks, majorgeeks, jv16 | Étape E6 prudente : **jamais de suppression de reste sans coche**, et export `.reg` avant toute clé supprimée. |
| **WinSxS : ne jamais supprimer à la main** ; passer par « Nettoyage de Windows Update » ou `DISM /StartComponentCleanup`. | geekrewind, helpdeskgeek | Étape E10 : on appelle l'outil de Windows, on ne touche pas au dossier. |
| Le cache de shaders DirectX est sans risque (reconstruit, saccade brève au prochain lancement d'un jeu). **Prefetch : vider est un mythe nuisible**, à laisser. `C:\Windows\Temp` : seulement les fichiers non utilisés. | geekrewind, thewindowsclub | Liste blanche des catégories (§ 5). **Prefetch n'y figure jamais.** |
| Storage Sense = automatique en arrière-plan ; Nettoyage de disque = manuel, à la demande. | geekrewind, windowscentral | Notre mode : analyse automatique, **action manuelle** (choix de l'auteur). |

**Non trouvé dans la recherche** (à ne pas affirmer) : comparatif de PC Manager, BleachBit et CCleaner ; comportement des
outils avec les fichiers OneDrive « en ligne seulement » ; tri des programmes par taille chez les désinstalleurs. De plus,
**l'affirmation « vider la RAM est surtout cosmétique, Windows la gère déjà »** vient de mes connaissances, pas de cette
recherche : elle est à revérifier avant l'étape E8 (voir § 8).

## 3. Ce qui existe déjà dans le code (repéré le 17/11/2026, à revérifier au moment de coder)

| Brique | Où | Réutilisation |
|---|---|---|
| Index de tous les fichiers des disques fixes, parcours en arrière-plan sans administrateur, `index.bin` (122,5 Mo sur le PC de l'auteur), surveillance par `ReadDirectoryChangesW` | `src/index.rs` | Source de l'analyse sans admin. **À vérifier en E0** : l'index garde-t-il la taille de chaque fichier ? (`taille_dossier(chemin)` existe, ce qui le laisse penser.) |
| Carte des disques de Ce PC (`preparer_carte`, `carte_prete`, six compteurs par disque) | `src/index.rs` | Point d'entrée visuel : la carte d'un disque mène à ESPACE. |
| Doublons : taille, empreinte rapide, SHA-256, lots rangés par « place à gagner » | `src/doublons.rs`, `Emplacement::Doublons` | Lien depuis ESPACE, pas de nouveau code. |
| Corbeille | `src/corbeille.rs`, `Emplacement::Corbeille` | « Vider la Corbeille » compté dans le récupérable. |
| Nettoyage des Téléchargements (`FOF_WANTNUKEWARNING`, R13) | `src/telechargements.rs` | Modèle de confirmation avant suppression définitive. |
| Opérations via `IFileOperation`, historique, annulation | `src/operations.rs`, `src/historique.rs` | Toute suppression passe ici. |
| Fonctions Windows déjà activées dans `Cargo.toml` | `windows` 0.58 | `Win32_System_Registry`, `Win32_System_Diagnostics_ToolHelp`, `Win32_Storage_CloudFilters`, `Win32_System_Memory` : déjà là. |
| Style de page (rail de catégories, lignes, segments) | `src/page.rs`, `src/pages.rs` | Modèle de la page ESPACE (rail à gauche, contenu à droite). |

**Fonctions Windows à ajouter** (à justifier par écrit et mesurer l'exe avant / après, règle d'`AGENTS.md`) : mémoire des
processus (`Win32_System_ProcessStatus`), contrôle du disque pour l'analyse rapide (`Win32_System_Ioctl`), applis du Store
(`Management_Deployment`). Taille de l'exe au 09/11 : 3,70 Mo (budget accepté par l'auteur le 10/11).

## 4. Le design (cohérent avec l'app existante)

### Principes (tirés du plan UX du 16/11 et de l'audit)

- **Noir opaque, Doto pour la structure** (titres, étiquettes, chiffres-clés), **Segoe pour le contenu** (noms de fichiers et
  de programmes), décision D-3 du lot 7.
- **Un seul accent : l'orange**, réservé à ce qui est actif ou **récupérable**. Tout le reste en gris (`BLANC`, `BLANC_DOUX`,
  `LIGNE`). Une carte en arc-en-ciel serait contraire au style Nothing.
- Rayons du jeton unique (8 pour les lignes, `RAYON_GRAND` pour les cartes), tailles en **multiples de 4**, barre orange
  active de 3 × 16 à gauche de la ligne choisie (celle du volet et du rail des Réglages), ombre douce sous les éléments
  flottants (lot 8).
- Icônes en **points** (matrice 7 × 7 de `icones.rs`), nouvelles icônes : ESPACE (disque en parts), NETTOYER, PROGRAMMES,
  DÉMARRAGE, MÉMOIRE.
- **Au clavier d'abord** : Haut / Bas, Droite / Gauche pour déplier, Entrée pour descendre, Retour pour remonter, Tab entre
  rail / liste / carte, Suppr pour supprimer. Aucune action destructive à la souris sans confirmation visible.
- Mouvement : transitions des listes dans les durées de `mouvement.rs` ; **Mouvement réduit** respecté ; **Contraste élevé**
  de Windows : carte en contours, pas de dégradés.
- Accessibilité : chaque ligne et chaque rectangle de la carte exposés à Narrateur (arbre UIA existant), avec « nom, taille,
  pourcentage ».

### Maquette de la page (1400 × 820, thème sombre)

```
┌ SPACEFILES ── onglets ───────────────────────────────────────────────  ⌕  ⚙  ─ □ ✕ ┐
│ ESPACE ▸ C:                                              ◔ ANALYSE · 3 s · 412 Go    │
├────────────┬─────────────────────────────────────────────────────────────────────────┤
│ ▌ANALYSE   │  RÉCUPÉRABLE   14,2 GO ───────────────────────────────  [ NETTOYER ▸ ] │
│  NETTOYAGE │  ┌───────────────┬──────────────┬──────────┬───────────┐                 │
│  PROGRAMMES│  │ Temporaires   │ Caches       │ Corbeille│ Dév.      │  (orange =      │
│  DÉMARRAGE │  │ 3,1 Go        │ 5,8 Go       │ 1,2 Go   │ 4,1 Go    │   récupérable)  │
│  MÉMOIRE   │  └───────────────┴──────────────┴──────────┴───────────┘                 │
│            │                                                                          │
│  C:  412 Go│  VUE   [ ARBRE ]  CARTE        TRI  TAILLE ▾     AGE  TOUS ▾             │
│  D:  1,2 To│  ───────────────────────────────────────────────────────────────────────│
│            │  ▌▾ Users                          ████████████████░░░░  186 Go   45 %   │
│  Types ▾   │      ▸ utilisateur                       ███████████████░░░░░  174 Go   42 %   │
│   Vidéo    │  ▸ Program Files                   ████░░░░░░░░░░░░░░░░   58 Go   14 %   │
│   Images   │  ▸ Windows                         ███░░░░░░░░░░░░░░░░░   31 Go    8 %   │
│   Code     │  ▸ ProgramData                     █░░░░░░░░░░░░░░░░░░░   12 Go    3 %   │
│            │                                                                          │
│  Depuis    │  Depuis la dernière analyse (lundi) :  Users\utilisateur\AppData  + 3,2 Go ▲   │
│  lundi     │                                                                          │
└────────────┴─────────────────────────────────────────────────────────────────────────┘
```

- **Rail à gauche** (comme les Réglages) : les cinq pages ; sous elles, les disques avec leur jauge et la ventilation par types.
- **Bandeau « RÉCUPÉRABLE »** en haut : un chiffre en Doto, la décomposition en rectangles gris dont seul le contour du survol
  passe à l'orange, le bouton NETTOYER (conventions des boutons de la barre d'outils, lot 4).
- **Arbre** : ligne de 36 DIP (28 en compact), nom en Segoe, barre de proportion en gris, taille et pourcentage en Doto.
  La ligne choisie : fond doux + barre orange.
- **Carte** : rectangles « squarified », niveaux de gris par profondeur, nom écrit dans le rectangle s'il tient, infobulle
  sinon ; le clic descend, Retour remonte ; la sélection reste synchronisée avec l'arbre.
- **Confirmation** : une carte flottante (ombre douce) listant les lignes exactes, la taille et « Corbeille » ou « définitif ».
- **Pixel** (la mascotte) : un seul clin d'œil pendant l'analyse, rien de plus.

### Pages du rail

| Page | Contenu |
|---|---|
| ANALYSE | Bandeau récupérable, arbre / carte, gros fichiers, âge, types, comparaison avec la dernière analyse. |
| NETTOYAGE | Catégories sûres avec cases et tailles ; modes de suppression (Corbeille, direct) affichés par ligne. |
| PROGRAMMES | Programmes installés et applis du Store : nom, éditeur, taille, date d'installation ; tri par taille ; Désinstaller ; puis liste des restes à cocher. |
| DÉMARRAGE | Programmes lancés avec Windows : source, éditeur, état ; Désactiver / Réactiver. |
| MÉMOIRE | Processus regroupés par programme, mémoire utilisée, Fermer (avec confirmation). |

## 5. Architecture

Un module `src/espace/` (un fichier par rôle, chacun commençant par son commentaire de rôle en français) :

| Fichier | Rôle |
|---|---|
| `analyse.rs` | Construit l'arbre **compact** (tableau de nœuds : nom interne, taille, indice du parent) dans un fil à priorité d'arrière-plan ; ne suit pas les jonctions ; compte une fois les liens physiques ; ignore les fichiers OneDrive en ligne seulement (taille sur disque nulle). |
| `carte.rs` | Calcul « squarified » des rectangles (fonction pure, testée) ; le dessin reste dans `rendu/`. |
| `categories.rs` | Types de fichiers (extensions), âge, gros fichiers. |
| `nettoyage.rs` | **Liste blanche** des catégories et de leurs chemins ; mesure, puis suppression par `IFileOperation`. |
| `programmes.rs` | Programmes (clés Uninstall de HKLM, WOW6432Node et HKCU) et applis du Store ; lancement du désinstalleur officiel. |
| `restes.rs` | Scan des restes après désinstallation ; export `.reg` avant toute clé supprimée. |
| `demarrage.rs` | Entrées Run, dossiers Démarrage, état d'activation. |
| `memoire.rs` | Processus et mémoire utilisée, fermeture. |
| `instantane.rs` | Petit fichier de l'analyse précédente (top des dossiers) pour la comparaison. |

- Un nouvel `Emplacement::Espace(page)` dans `dossier.rs`, géré comme Réglages (pas de liste de fichiers classique).
- **Mémoire du résident** : l'arbre complet n'existe que pendant que la page est ouverte ; il est **libéré à la fermeture** et
  l'analyse automatique ne garde que le résumé (quelques centaines d'octets). Le résident reste dans son budget de 16 Mo.
- **Analyse automatique** : au lancement de la fenêtre (pas du résident), si la dernière date de plus de 24 h ; priorité
  d'arrière-plan (processeur et disque), arrêt immédiat si l'auteur lance une copie ou ouvre un gros dossier.

### Garde-fous de suppression (non négociables)

1. **Liste blanche seulement** : une catégorie est un chemin connu (`%TEMP%`, cache de shaders, vignettes, caches des
   navigateurs…). Jamais « tout ce qui ressemble à un cache ».
2. **Jamais touchés** : `C:\Windows` (sauf par l'outil de Windows, § E10), `Program Files`, `Prefetch`, `System Volume
   Information`, les dossiers de données d'une app qui tourne. Un fichier verrouillé est ignoré et compté « en cours d'usage ».
3. **Restes** : aucune coche par défaut ; export `.reg` daté dans `%LOCALAPPDATA%\SpaceFiles\restes\` avant toute clé supprimée.
4. **Dossiers de développeur** (`target`, `node_modules`…) : proposés seulement s'ils sont **reconstructibles** (présence
   de `Cargo.toml` à côté de `target`, de `package.json` à côté de `node_modules`) et non modifiés depuis 30 jours
   (décision du 17/11, § 8).
5. **Confirmation** avec la liste exacte et la taille ; Corbeille d'abord ; annulation de 10 s par la pastille quand elle
   existe ; journal dans l'historique des opérations.
6. Test unitaire par règle : un chemin hors liste blanche est refusé.
7. **Ajouts du 17/11** : caches de navigateurs = caches seulement, **jamais l'historique ni les cookies** ; compactage WSL /
   Docker refusé si l'un tourne, avec sauvegarde conseillée (c'est une écriture sur le disque virtuel) ; points de
   restauration : jamais tous d'un coup, le plus récent reste ; désinstallation forcée et installation surveillée : mêmes
   règles que les restes (aucune coche par défaut, `.reg` exporté) ; compression NTFS : réversible, jamais sur `C:\Windows`
   ni sur un dossier en cours d'usage.

## 6. Étapes (chacune se termine seule ; la première décide de la suite)

| # | Étape | Critère de fin (preuve) |
|---|---|---|
| E0 | **Prototype de mesure**, sans interface : parcours de `C:` sans administrateur, arbre compact, console de test. | Chiffres sur le PC de l'auteur : durée, mémoire de l'arbre, nombre de nœuds ; l'index existant est-il réutilisable ? Décision écrite. **FAIT le 17/11/2026** : 2 842 618 éléments en 132 s sans admin, cumul 26 ms, 163,5 Mo, exact à l'octet sur `docs` ; l'index existant est réutilisé. Détail : [E0-mesures.md](E0-mesures.md). |
| E1 ✔ 18/11/2026 | Page ESPACE : rail, arbre trié avec barres, navigation au clavier et à la souris (croix, rail, lignes, flèche, double-clic), entrée depuis le volet de gauche (« Espace disque »), le menu AFFICHER, Ce PC et la palette ; titres explicites (« Ce qui prend de la place sur ce PC »). | Captures au clavier ; tests de tri et de pourcentage ; rien à la souris sans note de rodage. |
| E2 ✔ 18/11/2026 | Carte en rectangles et bascule arbre / carte (touche `V`, segments ARBRE / CARTE), descente et remontée, fil d'Ariane. | Tests de `carte.rs` (somme des aires, pas de chevauchement) ; captures au clavier ; 797 tests. Souris non vue. |
| E3 ✔ 18/11/2026 | Types, gros fichiers (100), âge (date de modification) : trois listes dans la page, touches `1` à `5`, lecture seule. | Tests de catégories, de tranches et de navigation ; captures au clavier ; 813 tests ; 227 ms pour 2,8 M de fichiers synthétiques. Souris non vue. |
| E4 | NETTOYAGE : catégories sûres (temp, shaders, vignettes, Corbeille, caches), bandeau « récupérable », confirmation, suppression, **analyse automatique**. | Test « chemin hors liste blanche refusé » ; suppression vue sur un faux profil ; annulation. |
| E5 | PROGRAMMES : programmes et applis du Store, taille, tri, désinstalleur officiel. | Liste comparée à « Applications installées » de Windows ; désinstallation d'un programme de test. |
| E6 | Restes après désinstallation, coches, export `.reg`. | Désinstallation d'un programme de test, restes listés, aucune coche par défaut, `.reg` relu. |
| E7 | DÉMARRAGE : liste, désactiver / réactiver. | Entrée de test désactivée puis réactivée, état relu dans le Gestionnaire de tâches. |
| E8 | MÉMOIRE : processus par programme, fermeture avec confirmation. | Chiffres comparés au Gestionnaire de tâches ; fermeture d'un processus de test. |
| E9 | Instantané et comparaison avec la dernière analyse ; carte « récupérable » dans Ce PC. | Deux analyses d'un dossier de test, écart attendu affiché. |
| E10 | Mode administrateur : analyse rapide du disque, composants de Windows Update (`DISM /StartComponentCleanup`), taille de l'hibernation, libération des fichiers OneDrive. | UAC accepté et refusé tous deux gérés ; chiffres avant / après lus. |
| E11 | **Recherche et filtres** dans l'analyse (jokers, taille, date, regex) + **export CSV** de l'arbre ou de la liste. Réutilise `index::analyser` / `chercher`. | Tests de filtres (taille, date, regex invalide refusée) ; CSV relu (UTF-8, séparateur, dates en UTC). Après E1. |
| E12 | **Ligne « espace non expliqué »** : écart entre l'espace pris sur le disque et la somme de l'arbre, ventilé en hibernation, fichier d'échange, restauration système, composants Windows quand c'est lisible. | Chiffres comparés à l'espace libre donné par Windows ; ce qui n'est pas lisible sans admin est dit « non mesuré ». Après E1. |
| E13 | **Développeur** : caches d'outils (cargo, npm, pip, Gradle…) dans NETTOYAGE ; **WSL et Docker** : tailles des `.vhdx`, `docker system prune` proposé, puis compactage (`wsl --shutdown` d'abord, avertissement et sauvegarde conseillée). | Un chemin par outil vérifié sur ce PC ; compactage essayé sur un disque de test ; aucune action si Docker ou WSL tourne. Caches après E4, compactage après E10 (admin). |
| E14 | **Désinstallation par lot**, **désinstallation forcée** (entrée cassée : retrait de l'entrée + scan des restes, export `.reg`) et **surveillance d'une installation** (instantané avant / après, restes proposés à la coche). | Lot de deux programmes de test ; programme à désinstalleur absent traité ; installation de test suivie puis retirée. Après E6 et E9. |
| E15 | **Points de restauration** (taille lue, réduction en gardant le plus récent), **caches de navigateurs** (caches seulement), **compression NTFS** d'un dossier froid (réversible). | Taille des clichés comparée à `vssadmin` ; cache vidé sur un profil de test sans toucher cookies ni historique ; dossier compressé puis décompressé, taille avant / après lue. Après E4 ; la restauration demande l'admin (E10). |
| E16 | **Alerte « disque presque plein »** dans SpaceNotch (`C: 92 % plein, 14 Go récupérables`). | **Contrat `docs/ecosystem/protocol.md` mis à jour d'abord et validé par l'auteur** (deux dépôts) ; message reçu côté SpaceNotch. Après E9. |

Après chaque étape : `cargo build --release`, `cargo test` (le nombre ne baisse pas), `/snapshot` pour ce qui est visible,
relecture `rust-reviewer` pour tout `unsafe` ou COM, liste de rodage mise à jour pour ce qui demande la souris.

## 7. Budgets

- Exe : chaque dépendance ou fonction Windows ajoutée est mesurée avant / après (budget actuel : 3,70 Mo).
- Résident : 16 Mo de mémoire de travail, arbre libéré à la fermeture de la page.
- Analyse sans admin : **objectif à fixer en E0** d'après la mesure réelle, pas un chiffre inventé ; analyse rapide admin :
  objectif en secondes à fixer en E10 de la même façon.
- Aucune analyse ne doit ralentir le défilement ni la copie en cours (priorité d'arrière-plan, test d'abandon).

## 8. Décisions du 17/11/2026 (second tour de questions) et ce qui reste ouvert

| # | Sujet | Décision de l'auteur |
|---|---|---|
| 1 | Ordre avec le terminal | **E0 d'abord** (prototype de mesure, court), puis le chantier terminal (T0), puis retour à ESPACE avec les chiffres d'E0. |
| 2 | E8, RAM | **Liste des gros consommateurs + Fermer** (avec confirmation). **Pas de bouton « libérer la RAM »**. |
| 3 | Analyse automatique | **Au lancement de la fenêtre si la dernière date de plus de 24 h**, priorité d'arrière-plan, arrêt immédiat si copie ou gros dossier. |
| 4 | Dossiers de développeur | Proposés seulement s'ils sont **reconstructibles** (`Cargo.toml` / `package.json` à côté) **et inactifs depuis 30 jours**. |
| 5 | Applis du Store | **Tout lister, avec un avertissement** sur les applis intégrées à Windows (carte d'avertissement avant désinstallation). *Écart avec ma proposition (ne pas proposer le système) : l'auteur a choisi de tout permettre ; l'avertissement est obligatoire.* |
| 6 | E7, DÉMARRAGE | **Liste + état + activer / désactiver.** Aucune mesure de coût de démarrage (Windows ne la donne pas simplement). |
| 7 | E10, administrateur | **Second processus élevé à la demande** (UAC à chaque fois) ; SpaceFiles et le résident restent non élevés. Conception détaillée à valider avec l'auteur avant E10, comme pour `Eco`. |
| 8 | E5, dernier usage d'un programme | **Seulement s'il existe** ; jamais de valeur devinée. |

**Encore ouvert** : rien qui bloque E0. À reposer avant l'étape concernée : l'affirmation sur la RAM reste à revérifier si on
affiche un jour un bouton de libération (non prévu) ; le contenu exact de l'avertissement du Store (point 5) ; le protocole
entre l'application et le processus élevé (point 7).

## 8 bis. Fonctions ajoutées le 17/11/2026 (deuxième recherche internet)

Choisies par l'auteur : **toutes celles proposées sauf la couleur de la carte par type de fichier** (écartée : elle
contredit la règle « un seul accent orange » ; **reprise le 18/11/2026** : l'auteur a demandé la couleur par type pour
la refonte UX de la carte, voir `PLAN-ACTION-ESPACE-TERMINAL.md` E2). Elles forment les étapes E11 à E16 du § 6.

| Fonction | Origine | Réserves |
|---|---|---|
| Recherche et filtres, export CSV | WizTree (jokers, opérateurs de taille et de date, regex ; export CSV réimportable) | Pas de comparaison de deux analyses intégrée chez WizTree : la nôtre (E9) est un vrai plus. |
| Espace non expliqué | Sources : hibernation (environ 75 % de la RAM, parfois bien plus), fichier d'échange, clichés de restauration, WinSxS en liens physiques | Tailles exactes parfois illisibles sans admin : le dire plutôt que deviner. |
| WSL et Docker | Blogs et forums : `.vhdx` qui ne rétrécit jamais seul, `Optimize-VHD` (Hyper-V, absent de Windows Home), repli `diskpart`, `wsl --manage` | **Aucune source officielle Microsoft ou Docker lue.** Les commandes exactes sont à vérifier avant E13. |
| Caches d'outils de dev, compression NTFS, alerte SpaceNotch | Idées de ma part, non cherchées | Chemins, gains et vitesses à mesurer. |
| Désinstallation par lot / forcée / surveillée | Revo (Pro : forcée, multiple, surveillance en direct), Geek (mode forcé), Bulk Crap Uninstaller (lot) | Comparatifs anciens et partiaux (un test donne l'avantage à Revo pour le registre, un autre à Geek pour le 64 bits). |
| Points de restauration, caches de navigateurs | BleachBit pour les caches ; clichés cités comme gros consommateurs cachés sans méthode sûre | Méthode de réduction des clichés à vérifier. |

**Correction sur l'âge des fichiers (E3)** : « non ouverts depuis un an » suppose que Windows note le dernier accès. Mesuré le
17/11 sur ce PC : `fsutil behavior query disablelastaccess` rend `2` (géré par le système, mises à jour **activées**). Mais
ce réglage varie d'un PC à l'autre et un antivirus ou l'indexation peut « toucher » un fichier. **L'âge affiché sera donc la
date de dernière modification par défaut**, le dernier accès n'étant montré qu'en second et étiqueté comme indicatif
(l'affirmation sur l'antivirus et l'indexation vient de mes connaissances, non vérifiée ici).

**Écarté** : surveillance de la santé des disques (SMART, températures) : hors sujet pour un nettoyeur, à garder pour un
autre chantier si l'auteur le veut.

## 9. Limites

- Recherche internet de surface : pages de comparatifs et d'éditeurs, certaines partisanes (FolderSizes, jv16). Aucune
  fonction n'a été essayée sur ce PC.
- Les noms de modules, de fonctions Windows et le contenu de l'index sont des **propositions et hypothèses** à revérifier
  dans le code au moment d'écrire chaque étape.
- Mesuré en E0 (17/11) : durée d'analyse et mémoire, voir [E0-mesures.md](E0-mesures.md). Taille de l'exe : non mesurée, à produire quand les fonctions Windows d'ESPACE seront ajoutées.

## Sources

- [WizTree vs WinDirStat (Tecnobits)](https://tecnobits.com/en/wiztree-vs-windirstat) · [Comparatif d'analyseurs (FolderSizes, éditeur concurrent)](https://www.foldersizes.com/compare) · [Revue WizTree (PCWorld)](https://www.pcworld.com/article/2036835/review-find-out-whats-taking-up-space-on-your-drive-in-seconds-with-wiztree.html) · [Alternatives à WinDirStat (theme404)](https://theme404.com/blog/best-windirstat-alternatives)
- [TreeSize, fonctions principales](https://jam-software.com/treesize/features.shtml) · [DiskRecon](https://www.blacksunsoftware.com/diskrecon/help/) · [OO DiskCommander, journal des changements](https://www.oo-software.com/en/oodiskcommander/changelog) · [Doublons (FileVoyager)](https://www.filevoyager.com/doc/the-duplicate-finder/)
- [Geek Uninstaller (ghacks)](https://www.ghacks.net/2016/06/22/geek-uninstaller-remove-windows-apps/) · [Bulk Crap Uninstaller (ghacks)](https://www.ghacks.net/2015/04/10/bulk-crap-uninstaller-removes-multiple-programs-silently-on-windows/) · [Restes après désinstallation (jv16, concurrent)](https://jv16powertools.com/blog/?p=236577)
- [Nettoyer WinSxS (geekrewind)](https://geekrewind.com/how-to-clean-up-the-winsxs-folder-in-windows-11) · [Nettoyage de disque Windows 11 (geekrewind)](https://geekrewind.com/how-to-use-disk-cleanup-in-windows-11/) · [12 conseils d'espace disque (Windows Central)](https://windowscentral.com/how-free-space-windows-11) · [Que supprimer du dossier Windows (The Windows Club)](https://thewindowsclub.com/what-can-i-delete-from-windows-folder)
- Deuxième recherche (17/11) : [Compacter les .vhdx de WSL2 et Docker (woshub)](https://woshub.com/shrink-wsl2-and-docker-desktop-vhdx-files-in-windows/) · [Idem (Scott Hanselman)](https://www.hanselman.com/blog/shrink-your-wsl2-virtual-disks-and-docker-images-and-reclaim-disk-space) · [Docker et WSL2 (Nick Janetakis)](https://nickjanetakis.com/blog/reclaiming-tons-of-diskspace-by-compacting-your-docker-desktop-wsl-2-vm) · [WizTree 4.32 (Neowin)](https://www.neowin.net/software/wiztree-432/) · [WizTree vs FolderSizes](https://www.foldersizes.com/features/wiztree) · [Revo contre Geek (ghacks)](https://www.ghacks.net/?p=63719) · [Désinstalleurs les plus complets (ghacks)](https://www.ghacks.net/?p=112635) · [Recommandations de nettoyage de Windows 11 (pureinfotech)](https://pureinfotech.com/free-up-space-cleanup-recommendations-windows-11/) · [Comparatif de nettoyeurs (kil0bit)](https://kil0bit.blogspot.com/2026/06/is-your-windows-pc-slow-3-best-free.html)
