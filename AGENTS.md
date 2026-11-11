# SpaceFiles (ex-Nothing Explorer)

Explorateur de fichiers pour Windows 11 au style Nothing : Rust (édition 2021), Win32 natif et
Direct2D/DirectWrite, sans WebView ni framework d'interface. Une seule fenêtre sans bordure, fond noir opaque
(plus d'acrylique depuis le lot 1 de l'alignement sur la maquette v6), police Doto. Lancé avec Windows en processus résident (`--resident`).

Format [AGENTS.md](https://agents.md), lu par tout agent de code. Réglages propres à Claude Code : `CLAUDE.md`.

## Terminer une tâche (règle n° 1)

Une tâche est **terminée** seulement quand tout ceci est vrai ; sinon elle est « en cours » et on dit ce qui manque :

1. Tout ce qui était demandé est fait : appelants, tests et fichiers que le changement casse compris. Pas de
   demi-fonction. Si c'est trop gros, découper par écrit en points qui se terminent seuls, puis faire le premier.
2. `cargo test` lancé et lu (un test non lancé n'est jamais « réussi » ; le nombre de tests ne baisse pas),
   `cargo build --release` lancé et lu. Un hook de fin de tour le refait et refuse de finir s'il échoue.
3. Un changement visible est vu à l'écran avec `/snapshot` (clavier seulement). Ce qui demande la souris n'est pas
   vérifié par l'agent : il va dans la liste de rodage F1 de `docs/ORDRE.md`.
4. Le rapport dit : ce qui a changé et pourquoi, les commandes lancées **avec leur résultat exact**, ce qui n'a pas
   été vu, les limites. Une vitesse s'annonce avec un chiffre mesuré, sinon « non mesurée ».

Une demande sans critère d'acceptation : en fixer un et le prouver. Une idée nouvelle en route va dans
`docs/IDEES.md`, pas dans le point en cours. L'ordre de travail est `docs/ORDRE.md` : « continue » = premier point
non terminé.

## Construire et tester

```bash
cargo check            # vérification rapide
cargo build --release  # binaire : target\release\spacefiles.exe (LTO, panic = abort)
cargo test             # tests unitaires (une vingtaine de secondes)
```

- `cargo build` (debug) est lent à l'exécution : ne rien mesurer dessus.
- Après `cargo build --release`, relancer `installer.ps1` (copie l'exe dans `%LOCALAPPDATA%\Programs\SpaceFiles`,
  recrée le raccourci et l'icône ; ferme aussi l'ancien résident `NothingExplorerResident`). Une instance qui tourne
  bloque la liaison (`LNK1104`) : la fermer avant.
- Données dans `%APPDATA%\SpaceFiles` et `%LOCALAPPDATA%\SpaceFiles` ; au premier lancement, copie depuis l'ancien
  dossier `NothingExplorer` (gardé intact, `src/migration.rs`). Lancer le binaire pour un essai avec `APPDATA` et
  `LOCALAPPDATA` redirigés vers un faux profil et `SPACEFILES_SANS_REGISTRE=1` (aucun accès au registre).
- Dépôt git local depuis le 01/11/2026, tag `pre-reboot` (état d'avant le reboot). Commit seulement à la demande
  de l'auteur. Depuis le 11/11/2026, une version publique existe sur GitHub (`nicoo-o/SpaceFiles`) : **ne jamais
  pousser `main` ni le tag**. `tools/publication/publier.sh "message"` fabrique la branche locale `public` (le
  contenu de `main` sans les captures de l'écran de l'auteur) ; seul `git push origin public:main` publie, avec
  l'accord de l'auteur. Pages d'accueil : `README.md` (anglais), `README.fr.md` ; titre généré par
  `node tools/readme/titre.mjs`. Le plan de travail est `PROJECT_REBOOT_MASTERPLAN.md` (`PLAN.md` et
  `docs/ORDRE.md` sont gelés) ; constats dans `AUDIT.md`. Les mods Windhawk
  (`windhawk/`) sont hors périmètre depuis le reboot : leur code n'est plus que dans le tag `pre-reboot`.
- Chaque fichier de `src/` commence par un commentaire en français qui dit son rôle : le lire avant d'éditer.
  `src/main.rs` ne garde que `main()` et les déclarations ; la fenêtre est dans `src/fenetre/` (dont `procedure.rs`, 1 863 lignes au 10/11), les commandes et menus dans `src/commandes/`, les minuteries dans `src/minuteries/`.

## Conventions

- **Tout est en français** : commentaires, documentation, titres, libellés affichés (entrées de menu en
  majuscules : `OUVRIR AVEC ▸`).
- Un commentaire dit **pourquoi** (le défaut corrigé, la mesure, la contrainte), pas ce que fait la ligne.
- Les mesures de `theme.rs` sont en DIP : passer par le facteur DPI de la fenêtre pour les pixels physiques.
- Win32 via le crate `windows` 0.58. Un `unsafe` reste minimal et porte un commentaire sur son invariant (durée de
  vie d'un handle, fil d'interface, taille d'un tampon).
- Les opérations sur les fichiers passent par `IFileOperation` (`operations.rs`) : une copie à la main perd
  l'annulation et les conflits.
- Une fonctionnalité qui écoute le système (dossier, veille, disques) se désabonne quand elle n'est plus affichée.
  Préférer un événement système à une minuterie.
- Pas de nouvelle dépendance sans la justifier par écrit, mesure avant / après à l'appui : taille de l'exe (3,70 Mo
  au 09/11, budget accepté par l'auteur le 10/11 ; 2,8 Mo au 24/10) et mémoire du résident (16 Mo de working set)
  sont des objectifs mesurés.

## Écosystème

SpaceFiles dialogue avec SpaceNotch et le Hub (autre dépôt : `..\dynamic iland windows`) par des tubes nommés
`Eco.<app>.<utilisateur>`, un manifeste et `shared.json` dans `%LOCALAPPDATA%\Eco`. Le contrat est
`docs/ecosystem/protocol.md`, le code `src/eco/`. Changer un message, le manifeste ou une clé partagée touche
**les deux dépôts** : mettre le contrat à jour d'abord et le dire à l'auteur avant de coder. Alt+Espace appartient à
SpaceNotch (décision D-01) ; ici, la palette s'ouvre par `Ctrl+K`. Vue d'ensemble : `..\SPACEHUB-VUE\SPACEHUB.md`.

## Pièges connus

- **Ne jamais lancer un test, une capture ou un script qui déplace la souris de l'utilisateur** (un hook le bloque).
  Les tests clavier sont permis.
- `#![windows_subsystem = "windows"]` : pas de console, un `println!` ne se voit nulle part. Déboguer par un fichier
  de journal (`%LOCALAPPDATA%\SpaceFiles\journal.txt`, écrit par `journal::ecrire`) ou la barre d'état.
- `panic = "abort"` ferme l'app sans rien afficher : pas de `unwrap()` sur une E/S réseau (SFTP, lecteur réseau),
  afficher l'erreur dans la barre d'état.
- Lire un fichier UTF-8 sans BOM avec `Get-Content` (PowerShell 5.1) donne du faux mojibake : utiliser l'outil Read
  ou `-Encoding UTF8`. Un `.ps1` avec accents doit avoir un BOM.
- `cargo` n'est pas toujours dans le PATH : `$env:Path += ";$env:USERPROFILE\.cargo\bin"`.
