# Notes de version de SpaceFiles

SpaceFiles n'a pas de version publiée ; cette liste décrit l'état de `main` depuis le reboot du 01/11/2026 (tag `pre-reboot`, `503bb77`). Le numéro dans `Cargo.toml` est `0.1.0`, inchangé. Installer et désinstaller : `docs/installation.md`.

Chaque ligne se termine par ses sources entre parenthèses : hash de commit court ou chemin d'un rapport du dépôt. Les lots qui touchent SpaceNotch ou son Hub (autre dépôt) ne sont comptés ici que pour la part côté SpaceFiles.

## [Non publié]

### Ajouté

- Le produit s'appelle SpaceFiles (ancien nom : Nothing Explorer) : exe `spacefiles.exe`, données dans `%APPDATA%\SpaceFiles` et `%LOCALAPPDATA%\SpaceFiles`, classes et libellés renommés, protocole `spacefiles://` avec l'alias `nothingexplorer://` conservé (37bfb06).
- Au premier lancement, les données de l'ancien dossier `NothingExplorer` sont copiées vers `SpaceFiles` ; l'ancien dossier n'est jamais modifié (70b7527).
- Un journal unique, `journal.txt`, écrit par `journal::ecrire` (niveau, module, message) (886643e).
- Le résident répond à des recherches de fichiers par un tube nommé `Eco.spacefiles.<utilisateur>` (`hello`, `search.query`) ; l'index est chargé à la demande et libéré après 5 minutes (2296875).
- Protocole Eco v1 : bibliothèque `src/eco/` avec serveur de tube à 4 instances, client borné à 300 ms avec vérification du serveur, `explorer.open` lancé hors du fil du crochet clavier, émission de `progress`, manifeste et lecture de `shared.json` (1911dbe, `docs/ecosystem/protocol.md`).
- `explorer.open` accepte `reuse:true` : le résident confie le chemin à une fenêtre SpaceFiles déjà ouverte, sinon il lance une fenêtre (d3803a4).
- Un avis de mouvement partagé dans `shared.json` (`motion:"reduced"`) est lu par SpaceFiles (a2778d4).
- Jetons de couleur `ACCENT` (ambre `#E8A66B`) et `ERREUR` (`#F0836B`) à la place du rouge Nothing ; `GROUPE_ROUGE` est gardé pour les groupes (19f0ce4).
- Thème clair, choix `auto / sombre / clair / systeme` (réglage local, puis `shared.json`, puis Windows, puis sombre), page APPARENCE, mouvement réduit qui coupe toutes les animations (7120c41).
- Raccourcis dessinés en touches, Doto jamais sous 14 DIP, audit de contraste `tools/design/audit-contraste.ps1` (457b096).
- Pixel (deux barres, couleur selon l'heure, yeux fermés la nuit) dans la barre d'état, pastilles d'opération (jauge de 16 barres, fin, erreur, « ANNULER · CTRL Z »), `CTRL+,` de retour dans le volet (871c258).
- Téléchargements du navigateur : fichiers partiels `.crdownload` et `.part`, pastille indéterminée, « TÉLÉCHARGÉ » avec AFFICHER, marque « NOUVEAU », réglage `telechargements_navigateur` (19bc308).
- Reflet (halo et lueur de trait) sous le curseur sur les éléments cliquables d'au plus 480×160 DIP (c0f327a).
- `desinstaller.ps1` : retire ce que l'installeur et l'application ont écrit, restaure l'explorateur par défaut sauvegardé, conserve les données sauf `-SupprimerDonnees`, option `-Simuler` (3ac49a4, `docs/installation.md`).
- `installer.ps1` accepte des racines factices et `-SansLancement` ; `tools/install/essai-installation.ps1` rejoue installation et désinstallation sur un faux registre (3ac49a4, `docs/prompts/2026-11-04-phase-8-lot-b-desinstalleur-rapport.md`).
- Scripts de mesure `tools/bench` (résident, fenêtre, démarrage, défilement, test d'une heure) et crochet d'essai `SPACEFILES_MESURE_DEFILEMENT` (e8fa36f, `docs/prompts/2026-11-03-phase-8-lot-a-mesures-rapport.md`).

### Modifié

- `main.rs` est découpé en modules `fenetre/`, `commandes/` et `minuteries/`, sans changement de comportement (eade85e).
- La session (onglets, onglet actif) est sauvée en continu, 2 s après un changement ; une fermeture anormale est détectée et les onglets sont rouverts avec un message dans la barre d'état (160685b, `docs/audit/phase-2-lot-c-session-2026-11-01.md`).
- SFTP, WebDAV, sauvegarde NAS et MTP sont gelés derrière le réglage `experimental`, faux pour un profil neuf ; une configuration SFTP ou WebDAV existante le met à vrai (de3ad39, `docs/audit/phase-2-lot-b-experimental-2026-11-01.md`).
- Le crochet clavier du résident ne garde que Win+E (edfa086).
- L'annulation d'une copie interrompt maintenant un fichier unique en cours et une annulation avant le premier octet est signalée comme annulée (1570152, `docs/audit/matrice-destructive-2026-11-01.md`).
- Un renommage dont le nom finit par un point ou un espace est refusé (« NOM INVALIDE ») (1570152, `docs/audit/matrice-destructive-2026-11-01.md`).
- Le dossier de 200 000 entrées est listé en 0,9 s avec 195 Mo, une seule mesure (4d22fec, `docs/audit/phase-2-lot-d-200k-2026-11-01.md`).

### Retiré

- Fond vivant, veille, mods Windhawk, `nothing-essential/` et ce qui s'y rattache, ainsi que 9 features du crate `windows` ; le code est dans le tag `pre-reboot` (2ffef18).
- La recherche rapide Alt+Espace de SpaceFiles (`--rapide`, `Palette::rapide`, réglage « RECHERCHE RAPIDE ») : la touche appartient à SpaceNotch (edfa086, `docs/audit/phase-3-lot-2-retrait-alt-espace-2026-11-01.md`).

### Corrigé

- Défauts D1, D2, D4 et D5 de la matrice des opérations destructives : annulation non signalée, annulation sans effet dans un seul gros fichier, mise à la corbeille non annoncée et restauration impossible pour un chemin de plus de 260 caractères (1570152, `docs/audit/matrice-destructive-2026-11-01.md`).
- 8 `unwrap` de production remplacés par `if let` ou `let else` (56b5ace, `docs/audit/unwrap-2026-11-01.md`).
- Win+E : l'état bloqué quand la touche Windows est relâchée avant E est corrigé (edfa086).
- Une requête `contenu:` avec une majuscule spéciale provoquait une panique du service de recherche (2296875).
- R24 : un client connecté et muet bloquait les autres clients du tube jusqu'à 10 s (1911dbe, `AUDIT.md`).
- R21 et R22 : commentaire orphelin replacé, message de lecture OCR en cours reformulé, place libérée à l'arrivée du résultat (de3ad39, `docs/audit/phase-2-lot-b-experimental-2026-11-01.md`).

### Interne

- Tests et outils sans effet visible : banc de la matrice destructive de 27 tests, jetons de design `design/tokens.json`, spécification du protocole, harnais de preuve croisée avec SpaceNotch, rapports des lots ; `cargo test` passe de 61 réussis à la phase 0 à 355 réussis, 0 échec, 7 ignorés au lot A de la phase 8 (941c09b, 8cfa062, 3463956, e8fa36f, `PLAN.md`).
- Côté Hub de SpaceNotch (autre dépôt), les briefs, rapports et mesures sont archivés ici (c3bbd6a, 0f7d4bf, d1432b5, 417f265).

## Limites connues

Reprises de `AUDIT.md`, des rapports de lots et de `PLAN.md`, sans adoucissement.

### Budgets mesurés dépassés (phase 8, lot A)

- Démarrage « processus → fenêtre visible » : 514 ms à chaud (médiane, n = 10) et 920 ms à froid (n = 10) contre 400 ms ; mesure sur un bureau Windows non composé, première image non mesurée (e8fa36f, `docs/prompts/2026-11-03-phase-8-lot-a-mesures-rapport.md`).
- Mémoire privée de la fenêtre à 10 000 éléments : 82,13 Mo contre 80 Mo, dont environ 76 Mo de coût fixe dès 20 éléments (e8fa36f, `docs/prompts/2026-11-03-phase-8-lot-a-mesures-rapport.md`).
- Taille de l'exe : 3 848 704 octets (3,85 Mo) contre 2,8 Mo d'objectif historique (e8fa36f, `docs/prompts/2026-11-03-phase-8-lot-a-mesures-rapport.md`).
- Résident : la mémoire privée de repos passe de 3,44 à 3,89 Mo en deux marches après deux rafales de recherche, pente de 0,554 Mo/h au test d'une heure (seuil de la consigne : 0,5) ; un seul essai, fuite non exclue (e8fa36f, `docs/prompts/2026-11-03-phase-8-lot-a-mesures-rapport.md`).
- Le « 60 fps perçu » n'est pas mesuré : le coût de dessin est de 2,192 ms médiane et 2,521 ms au p95 pour 10 000 éléments en liste, sans composition ni vsync (e8fa36f, `docs/prompts/2026-11-03-phase-8-lot-a-mesures-rapport.md`).
- Latence de requête du service de recherche : 0,6 à 16 ms au p95 sauf requêtes rares (`zzzqxj` 109 ms) ; la cible de 30 ms n'est pas tenue pour celles-ci (2296875, `PLAN.md`).

### Constats ouverts de AUDIT.md

- R23 : `reglages.toml` écrit avec un BOM UTF-8 (PowerShell `-Encoding UTF8`) : la première clé n'est pas lue (`AUDIT.md`).
- R25 : le nom du tube dépend de `USERNAME` ; un autre processus du même utilisateur et de même intégrité peut usurper le tube (`AUDIT.md`).
- R26 : un index de plus de 6 h est reconstruit dans le résident à la première requête, plusieurs minutes et de la mémoire en plus des environ 140 Mo (`AUDIT.md`).
- R27 : latence de requête rare de 100 à 130 ms au p95 sur 2,75 M éléments (`AUDIT.md`).
- R28 : Win+E appelle `ShellExecuteW` dans la procédure de la fenêtre cachée du crochet ; un lancement lent retarde les touches suivantes (`AUDIT.md`).
- R29 et R30 (SpaceNotch) : quatre clients lents occupent les 4 instances du tube jusqu'à la coupure à 30 s ; `op:1` et `op:"1"` désignent la même opération (`AUDIT.md`).
- R31 et R32 (SpaceNotch) : flash de thème au démarrage de la notch ; le suivi de `shared.json` s'arrête si le dossier `Eco` est supprimé puis recréé (`AUDIT.md`).
- R33 (moyenne) : `ONGLETS_COPIES` sans borne de taille à la réception, `is_dir` sur le fil d'interface ; une charge vers un hôte mort fige la fenêtre (`AUDIT.md`).
- R34 : « mouvement réduit » ne coupe que l'apparition en points d'une liste ; le défilement adouci et les autres animations l'ignorent (`AUDIT.md`).
- R35 : la fenêtre répond « 1 » avant d'avoir ouvert l'onglet demandé par `ouvrir_ici` ; une fenêtre fermée dans l'intervalle perd la demande sans repli (`AUDIT.md`).
- R36 : réentrance des messages `WM_APP` dans les boucles modales (menu du Shell, glisser-déposer) (`AUDIT.md`).
- R37 (moyenne, Hub) : working set au repos 142,5 Mio, soit 153 Mo décimaux au pire, contre « ≤ 150 Mo » ; décision D-08 en attente (`AUDIT.md`).
- R38 : SpaceFiles n'écrit presque jamais une ligne de niveau erreur ; la carte Santé du Hub dit « aucune erreur » dans presque tous les cas (`AUDIT.md`).
- R39 à R41 (Hub et SpaceNotch) : la sonde d'Alt+Espace ne voit pas un crochet bas niveau ; un lancement du Hub sur 28 est resté vivant 10 s ; le lecteur JSON Rust (profondeur 8) et le lecteur C# (16) divergent (`AUDIT.md`).
- R42 : SpaceFiles n'a aucun argument qui ouvre ses réglages ; le Hub n'affiche pas de bouton « Réglages » pour lui (`AUDIT.md`).
- R43 (moyenne) : les captures claires de `ecrans.ps1 -Theme Clair` sortent en aplat sombre vide (code 1), aussi avec le binaire du commit précédent ; cause non élucidée, le thème clair n'est pas revu à l'écran (`AUDIT.md`).
- R44 : la marque « NOUVEAU » n'est dessinée qu'en liste et en détails, pas en grille ni en colonnes (`AUDIT.md`).
- R45 : la surveillance des téléchargements est par processus de fenêtre, pas dans le résident ; pas d'accès clavier à AFFICHER ni ✕ des pastilles (`AUDIT.md`).
- R46 (moyenne) : une boucle modale distribue `WM_PROGRES` et `WM_CHANGEMENT` pendant qu'un `&mut Fenetre` est vivant : aliasing, comportement indéfini ; antérieur à la phase 7 (`AUDIT.md`).
- R47 : pastilles sans événement UIA, boutons non exposés, « X sur Y » et vitesse non affichés, libellés « DEPLACEMENT » et « DÉPLACEMENT » non harmonisés (`AUDIT.md`).
- R48 : zones de reflet plus petites que les zones de clic ; plusieurs éléments exclus du reflet (`AUDIT.md`).
- R49 : accent partagé de `shared.json` non honoré ; couleurs de groupes ambre et vert sous 3,0:1 sur le fond clair (`AUDIT.md`).
- R50 : états de Pixel autres que le repos, relais vers la palette et clignement de 9 s jamais vus à l'écran (`AUDIT.md`).
- R51 : chaque mouvement sur un élément éclairé redessine toute la scène (3,4 à 4,2 ms mesurés) (`AUDIT.md`).

### Jamais vu sous la souris

- Rien n'a été vu sous la souris : le rodage F1 (liste, glisser-déposer, lasso, palette, réglages, menus, deux fenêtres, deuxième écran à autre DPI) n'est pas fait (`docs/ORDRE.md`).
- Phase 2, suspendu par l'auteur : copie interrompue ou réseau coupé sur un vrai partage, accessibilité et Narrateur, deuxième écran et DPI mixtes, défilement à 60 fps sur 200 000 éléments non mesuré (`PLAN.md`).
- Phase 2, boîte « Fichier en cours d'utilisation » de Windows (D3) : sa visibilité au premier plan dans l'application réelle n'est pas vérifiée (`docs/audit/matrice-destructive-2026-11-01.md`).
- Phase 2, scénarios de la matrice non couverts : déplacement entre volumes interrompu, coupure de partage réseau, disque cible plein (`docs/audit/matrice-destructive-2026-11-01.md`).
- Phases 3 à 5 : Alt+Espace avec SpaceNotch lancé, Win+E réel, focus de la fenêtre ouverte par `explorer.open`, restauration d'une fenêtre réduite, thème partagé appliqué, contraste élevé réel (`PLAN.md`).
- Phase 7 : survol, clic, déploiement des pastilles, AFFICHER, reflet en direct, une opération réelle avec pastilles, un vrai Chrome, Edge ou Firefox, menus du Shell en clair (`PLAN.md`).
- Phase 8 : lancement de `installer.ps1` et `desinstaller.ps1` sur la vraie machine, épingle de la barre des tâches, vrai résident arrêté, entrée « Applications installées » (inexistante) (`docs/prompts/2026-11-04-phase-8-lot-b-desinstalleur-rapport.md`).

## Critères de la première version utilisable

Liste de `PROJECT_REBOOT_MASTERPLAN.md`, Part 11 (« First genuinely usable release »), état au 04/11/2026.

- Critères 7.A de l'explorateur : **non tenu**. Résident à 3,45 Mo privés (tenu), mais démarrage à froid 920 ms contre 400 ms, liste de 10 000 éléments à 82,13 Mo contre 80 Mo, 200 000 éléments à 60 fps non mesuré, test d'8 heures non fait (un test d'une heure), liste de 40 tâches à la souris non faite, Narrateur et DPI mixtes non vérifiés (`docs/prompts/2026-11-03-phase-8-lot-a-mesures-rapport.md`).
- Alt+Espace s'ouvre à chaud en 100 ms au plus : **non vérifié ici**. C'est un critère de SpaceNotch ; SpaceFiles n'a plus de recherche rapide, et la mesure touche → visible n'a jamais été faite (`docs/audit/phase-3-e1-mesures-2026-11-01.md`).
- Progression et ouverture de dossier entre l'explorateur et SpaceNotch, avec repli : **tenu en code, non vu à l'écran**. Preuve croisée par harnais côté Rust et C#, aucune vue réelle de la progression dans la notch (3463956, `docs/audit/phase-4-lot-c-preuve-croisee-2026-11-02.md`).
- Thème partagé : **tenu en partie**. SpaceFiles lit le thème et le mouvement de `shared.json`, mais l'accent partagé n'est pas honoré (R49) et rien n'a été vu à l'écran (a2778d4, `docs/audit/phase-5-lot-d-reglages-partages-2026-11-02.md`).
- Le Hub v1 liste et diagnose : **tenu en code côté SpaceNotch, non vérifié ici**. Cinq pages et mesures E-4 faites, mais barrière mémoire en dépassement selon la métrique (R37) et aucun clic réel (417f265, `docs/audit/phase-6-lot-d-hub-memoire-publication-2026-11-02.md`).
- Une identité cohérente en sombre et en clair : **non vérifié**. Le thème clair n'est pas revu à l'écran (R43) ; l'audit de contraste donne 38 paires, 0 sous 4,5:1 (7120c41, 457b096, `AUDIT.md`).
- Zéro bug connu de perte de données : **tenu au sens littéral, non vérifié pour trois scénarios**. Aucun constat ouvert de `AUDIT.md` ne signale une perte de données et D1 à D5 sont corrigés, mais déplacement interrompu entre volumes, coupure réseau et disque plein ne sont pas couverts, et R46 est un comportement indéfini possible (1570152, `docs/audit/matrice-destructive-2026-11-01.md`).

## Mise à jour

- Aujourd'hui, mettre à jour consiste à relancer `installer.ps1` après `cargo build --release` (`docs/installation.md`).
- Il n'y a pas d'auto-mise à jour ni de réglage « mises à jour » (`AUDIT.md`).
- La stratégie de mise à jour reste une décision de l'auteur, lot D de la phase 8 (`docs/prompts/2026-11-04-phase-8-lot-c-notes-de-version.md`).
