# Plan : le vrai terminal de SpaceFiles (écrit le 17/11/2026)

Statut : **plan validé avec l'auteur et revérifié sur internet le 17/11/2026 (voir § 2 bis) ; rien n'est codé.** Démarrage
seulement sur son accord, après les lots 7 et 8 du [plan UX](../ui/plan-action-ux-2026-11-16.md), **une étape à la fois**
(règle n° 1 d'`AGENTS.md`). Chaque étape se termine seule : build, tests et rapport avec les chiffres mesurés.

## 1. Ce que veut l'auteur (réponses du 17/11/2026)

| Sujet | Décision |
|---|---|
| Shell | **Un shell « SpaceFiles » mixte** : une ligne de saisie dessinée par SpaceFiles, qui envoie chaque commande à un **vrai** PowerShell ou à un **vrai** cmd. Détection automatique + préfixe qui force le choix (`cmd:` / `ps:`) + badge CMD / PS. Aucun nouveau langage de commande. |
| Forme | **Panneau en bas, redimensionnable, avec onglets** (remplace le panneau « journal » de 196 DIP ; ouverture inchangée : Ctrl+Alt+T et bouton « >_ »). Le journal des opérations devient un onglet à part. |
| Programmes plein écran | **Supportés dans le panneau** (vim, nano, htop, ssh, python interactif, winget avec barre de progression). |
| Aide à la saisie | (1) **suggestion grise** tirée de l'historique, (2) **complétion Tab** (commandes, options, chemins) avec menu, (3) **explication** d'une commande ou d'une option. **Pas d'IA / langage naturel** : reporté, comme l'idée 19 du centre de commandes. |
| Liens avec l'explorateur | Les quatre : dossier courant synchronisé · fichiers sélectionnés en paramètre (touche + glisser-déposer) · sortie cliquable · historique par dossier + commandes épinglées. |
| Technique | **ConPTY via le crate `windows` déjà présent** pour la session. **Révisé le 17/11 après vérification : analyseur = crate `vte` (machine d'états seule, utilisée par Alacritty) + grille, défilement et écran alternatif écrits par nous.** Ni `alacritty_terminal` ni `portable-pty`. Surcoût de `vte` en exe à mesurer à T0. |
| Aide à la saisie (confirmé 17/11) | **Ligne de saisie SpaceFiles** pour tout (suggestion grise, Tab, explication), identique en cmd et PowerShell. Raison vérifiée sur le PC de l'auteur : PowerShell 7 absent, PowerShell 5.1 avec PSReadLine 2.0 (sans prédictions), Clink absent ; l'aide native du shell n'existe donc pas ici. Sources de complétion **maison** (D-T3 tranchée) ; un PowerShell caché (`TabExpansion2`) reste une option à rouvrir après T7 seulement si les mesures le justifient. |
| Mixte (confirmé 17/11) | Détection automatique + préfixe + badge, **gardé** ; les cas ambigus tombent sur le dernier shell utilisé ; aucune devinette de plus. Plus ambitieux que Warp (qui gère mal cmd) : la table de règles est le risque, elle est testée à T6. |
| Plancher Windows (décidé 17/11) | **Windows 11 22H2 (build 22621) et après** pour les marqueurs fiables (mode passthrough de ConPTY). Repli sur un Windows plus ancien : dossier suivi en lisant nous-mêmes les `cd`, fin de commande détectée à l'invite ; blocs de commandes dégradés. Le PC de l'auteur est en 10.0.26300 : le chemin complet y est disponible. Pas de `conpty.dll` embarquée. |
| Priorité | Chantier à part, **après les lots 7 et 8** du plan UX. |

## 2. Ce que l'on sait (sources)

- ConPTY (Windows 10 version 1809 minimum) : l'application crée deux tuyaux synchrones, appelle `CreatePseudoConsole`, lance le
  shell avec l'attribut `PROC_THREAD_ATTRIBUTE_PSEUDOCONSOLE`, puis lit du texte UTF-8 mêlé de séquences VT et y écrit les touches.
  Microsoft recommande **un fil par tuyau** (risque d'interblocage) et de libérer les poignées après `CreateProcess` ; fermer la
  session termine aussi les processus enfants.
  [Creating a Pseudoconsole session](https://learn.microsoft.com/windows/console/creating-a-pseudoconsole-session),
  [CreatePseudoConsole](https://learn.microsoft.com/windows/console/createpseudoconsole),
  [Pseudoconsoles](https://learn.microsoft.com/windows/console/pseudoconsoles).
- Ce que font les autres : `portable-pty` (WezTerm) lance le shell, `alacritty_terminal` / `vte` analysent la grille,
  `conpty-oxide` ne fait que la session. Écartés au profit de ConPTY direct + `vte` (poids de l'exe, API instable d'`alacritty_terminal`,
  voir § 2 bis).
  [conpty-oxide](https://docs.rs/crate/conpty-oxide/0.1.2), [rust-pty](https://docs.rs/crate/rust-pty/0.2.0/source/src/windows.rs),
  [portable-pty](https://openapps.pro/packages/portable-pty).
- Aide à la saisie, modèles : [Warp, complétions](https://docs.warp.dev/terminal/command-completions/completions) (Tab, specs de
  centaines de commandes) et [autosuggestions](https://docs.warp.dev/terminal/command-completions/autosuggestions) (historique) ;
  [PSReadLine, prédictions](https://learn.microsoft.com/en-us/powershell/scripting/learn/shell/using-predictors?view=powershell-7.4)
  (vue en ligne ou en liste, plugin `CompletionPredictor` à partir de PowerShell 7.2) ;
  [carapace-bin](https://carapace-sh.github.io/carapace-bin/print.html) (plus de 500 commandes, PowerShell, cmd expérimental via Clink).
  Fig n'existe plus seul (sa technique sert à Amazon Q).

## 2 bis. Revérification sur internet (17/11/2026) : ce qui tient, ce qui change

| Point du plan | Verdict | Détail et source |
|---|---|---|
| ConPTY comme socle | **Tient** | API documentée, Windows 10 1809+ ; c'est ce qu'utilisent Windows Terminal, WezTerm, Alacritty. [Creating a Pseudoconsole session](https://learn.microsoft.com/windows/console/creating-a-pseudoconsole-session). |
| Le flux ConPTY est « le flux du programme » | **Faux : à corriger** | ConPTY **re-synthétise** l'écran : on ne reçoit pas les octets bruts de l'enfant, l'ordre de certains OSC n'est pas garanti, les DCS ne passent pas, le redimensionnement et le défilement posent des problèmes connus (WezTerm a un « mode quirk »). Conséquence : T1/T2 testent sur de **vrais flux ConPTY enregistrés**, pas sur des flux théoriques. |
| Marqueurs OSC 133 / 9;9 (blocs, dossier, fin de commande) | **Faisable, avec condition** | Windows Terminal ≥ 1.18 les comprend, mais ConPTY ne les transmet de façon fiable qu'avec le **mode passthrough** (`PSEUDOCONSOLE_PASSTHROUGH_MODE`, Windows 11 22H2+). **Dernière vérification (17/11) sur la valeur du drapeau : non confirmée, et un conflit apparaît.** Des copies tierces de `conpty.h` et le crate [portable-pty](https://docs.rs/crate/portable-pty/latest/source/src/win/psuedocon.rs) donnent `PSEUDOCONSOLE_PASSTHROUGH_MODE = 0x8`, mais le [`winconpty.h` actuel de microsoft/terminal](https://github.com/microsoft/terminal/blob/main/src/winconpty/winconpty.h) définit `0x08` comme `PSEUDOCONSOLE_GLYPH_WIDTH_GRAPHEMES` (0x10 = WCSWIDTH, 0x18 = CONSOLE, 0x20 = AMBIGUOUS_IS_WIDE) et ne cite pas le passthrough. Les drapeaux de `conpty.dll` (livrée avec Windows Terminal) et ceux du `kernel32` de Windows ne sont donc peut-être pas les mêmes. Constat local : le crate `windows` 0.58 (et même `windows-sys` 0.61) ne définit que `PSEUDOCONSOLE_INHERIT_CURSOR`, la constante serait donc posée par nous. **Règle : on ne s'y fie pas avant le test de T0** (créer la session avec le drapeau, vérifier si le flux contient bien les OSC) ; en cas d'échec de création, retomber sur les drapeaux par défaut, comme le fait [rmux-pty](https://docs.rs/crate/rmux-pty/0.5.0/source/src/backend/windows/pty.rs). Pas de SDK Windows Kits installé sur le PC pour lire `consoleapi.h`. Sources : [microsoft/terminal#1173](https://github.com/microsoft/terminal/issues/1173), [#1985](https://github.com/microsoft/terminal/issues/1985). → plancher Windows 11 22H2 + repli (§ 1). |
| Analyseur VT « maison » | **Changé** | `vte` ne fait que la machine d'états (Paul Williams), est éprouvé et léger (version 0.15.0, licence Apache-2.0 ou MIT, vérifiée sur [crates.io](https://crates.io/crates/vte) et son [Cargo.toml](https://github.com/alacritty/vte/blob/master/Cargo.toml) : dépendances obligatoires `arrayvec` et `memchr` sans fonctions par défaut ; `log`, `bitflags`, `cursor-icon` seulement avec la fonction `ansi`, que l'on n'active pas) ; la grille reste à écrire de toute façon. `alacritty_terminal` : API instable et peu documentée pour un intégrateur, trop lourd. `portable-pty` : pas de passthrough. [vte](https://docs.rs/vte), [alacritty_terminal](https://docs.rs/alacritty_terminal). |
| Ligne de saisie maison | **Tient** | Même approche que Warp (intégration par shell, blocs). Les solutions natives n'existent pas ici : PSReadLine 2.0 (PowerShell 5.1 de l'auteur) n'a pas de prédictions, elles demandent PSReadLine 2.2+ et PowerShell 7.2+ ; Clink ne concerne que cmd. [PSReadLine predictors](https://learn.microsoft.com/powershell/scripting/learn/shell/using-predictors), [Clink](https://chrisant996.github.io/clink/clink.html). |
| Complétion Tab fidèle | **Coût réel** | La complétion exacte de PowerShell (`CompleteInput` / `TabExpansion2`) exige un processus ou un runspace PowerShell ; latence et mémoire **non mesurées**. D'où : sources maison d'abord (chemins, `PATH`, alias, base embarquée). [CommandCompletion.CompleteInput](https://learn.microsoft.com/dotnet/api/system.management.automation.commandcompletion.completeinput). |
| Shell mixte | **Plus ambitieux que l'existant** | Warp gère PowerShell, WSL, Git Bash, mais cmd est mal ou pas couvert ; aucun terminal grand public ne détecte cmd/PowerShell automatiquement. Faisable avec une table de règles et un préfixe de secours, mais à tester sur des lignes réelles. [Warp](https://docs.warp.dev/). |
| Poids et mémoire | **Inconnus** | Aucune source ne les donne pour notre cas : mesure à T0 (exe avant / après avec `vte`, `Win32_System_Console`). |

Sur le PC de l'auteur (mesuré le 17/11/2026) : Windows 11 10.0.26300, `wt.exe` présent, `wsl.exe` présent, `pwsh` absent,
PowerShell 5.1 avec PSReadLine 2.0.0, `clink` absent.

## 3. Architecture (trois couches)

1. **Session** (`src/terminal/session.rs`) : une session ConPTY par shell (PowerShell, cmd), créée à l'ouverture d'un onglet et
   détruite à sa fermeture. Deux fils (lecture, écriture) ; la sortie arrive à la fenêtre par un message Win32 posté (jamais de
   verrou tenu pendant le dessin). Aucune session tant que le panneau n'a jamais été ouvert : le résident ne paie rien.
2. **Émulateur** (`src/terminal/vt.rs`, **sans Win32, entièrement testable**) : le crate `vte` découpe le flux en séquences ; notre
   code (trait `Perform`) les applique (couleurs SGR, curseur, effacements, défilement, titre, écran alternatif, caractères larges)
   sur une **grille** de cellules + historique de défilement borné. Hors périmètre au début : sixel, images, ligatures.
3. **Ligne de saisie SpaceFiles** (`src/terminal/saisie.rs`) : éditeur de la commande en cours, dessiné par `rendu.rs`. Elle porte
   la suggestion grise, le menu Tab, l'explication, l'historique. Quand le programme demande l'**écran alternatif** (DECSET 1049),
   elle se retire et le panneau passe en **mode brut** (les touches vont telles quelles au programme) ; elle revient à la sortie.

Pour savoir où commence et finit une commande (blocs, code de sortie, dossier courant) : séquences d'intégration que l'on fait
émettre par le shell (fonction `prompt` de PowerShell, variable `PROMPT` de cmd), de type OSC 133 / OSC 9;9. La session est créée
avec le **mode passthrough** quand Windows ≥ 22621 (§ 2 bis) ; sinon (ou si le test de T0 montre que les séquences se perdent),
**repli sans marqueurs** : dossier courant lu dans la commande `cd` que l'on interprète nous-mêmes, fin de commande reconnue à
l'invite. Le code ne dépend jamais des marqueurs pour fonctionner : ils ajoutent la précision (blocs, code de sortie).

**Mixte.** Deux sessions persistantes à la demande (une PowerShell, une cmd), chacune garde son état (variables, historique du
shell). La détection est une table de règles testée (`dir /b`, `copy`, `%VAR%` → cmd ; `Get-…`, `|`, `$x`, `-Param` → PowerShell ;
ambigu → dernier shell utilisé). Le dossier courant est **partagé** : changer de shell réémet un `cd` silencieux.

## 4. Étapes (chacune se termine seule)

Test automatique = commande lancée par l'agent. Tout ce qui demande la souris va dans la liste de rodage F1.

| # | Étape | Acceptation |
|---|---|---|
| T0 | **FAIT le 17/11/2026** ([T0-mesures.md](T0-mesures.md) : `vte` = +10 752 octets d'exe ; les drapeaux 0/0x8/0x10/0x18 donnent des flux identiques, passthrough non distinguable ; OSC 133 et 9;9 arrivent sans réglage pour cmd et PowerShell ; décision : continuer). **Prototype jetable de la session** : feature `Win32_System_Console`, lancer `cmd /c echo ok`, lire la sortie, sans interface. Trois mesures et vérifications : (a) **exe avant / après** avec `vte` ; (b) valeur **effective** du drapeau passthrough, trouvée par l'essai (0x8 contesté, voir § 2 bis : 0x8 = largeur des glyphes dans le `winconpty.h` actuel) ; (c) enregistrer dans un fichier le flux brut reçu pour `cmd` et `powershell` (invite, couleurs, `cd`) **avec et sans** passthrough, et dire si les OSC 133 / 9;9 arrivent. | Un test d'intégration lit `ok` ; les trois chiffres / constats sont écrits ; décision de continuer, de changer de plancher Windows ou de s'arrêter. |
| T1 | **FAIT le 18/11/2026** (65 tests, 878 au total ; détails et limites dans `docs/PLAN-ACTION-ESPACE-TERMINAL.md` § T1 ; bogue de `vte` 0.15.0 contourné dans `Terminal::ecrire`). **Grille sur `vte`** (`vt.rs`). | Tests unitaires sur les flux enregistrés à T0 et des flux réels (couleurs, curseur, défilement, écran alternatif, accents, emoji larges, séquences coupées au milieu) ; aucune panique sur flux aléatoire (`panic = "abort"`). |
| T2 | **Panneau** : rendu de la grille, redimensionnement (`ResizePseudoConsole`), défilement, copie, Ctrl+C, fermeture propre. | Capture lue (voir « Capture » ci-dessous) ; test : redimensionner ne perd aucune ligne ; fermer l'onglet ne laisse aucun `powershell.exe` ni `conhost.exe` orphelin (vérifié par liste de processus). |
| T3 | **Programmes plein écran** : écran alternatif, curseur, touches fléchées et de fonction. | `python` interactif, `winget`, `more` et un programme plein écran de test fonctionnent au clavier ; la souris dans vim est notée pour l'auteur. |
| T4 | **Onglets + dossier synchronisé** ; le journal des opérations devient un onglet. | Naviguer dans la liste change le dossier du terminal ; `cd` dans le terminal change la liste ; le journal garde ses données. |
| T5 | **FAIT en code le 20/11/2026 (1057 tests, non vu à l'écran ; détail dans `docs/PLAN-ACTION-ESPACE-TERMINAL.md`).** **Ligne de saisie + suggestion grise** depuis l'historique SpaceFiles (aussi par dossier). | Tests sur l'éditeur (curseur, mots, collage) et sur le classement ; l'historique n'enregistre pas les lignes qui contiennent `password`, `token`, `secret` (à confirmer, voir D-T7). |
| T6 | **FAIT en code le 20/11/2026 (1109 tests ; détail et limites dans `docs/PLAN-ACTION-ESPACE-TERMINAL.md`).** **Mixte** : détection, préfixes, badge, dossier partagé. | Table de règles testée (au moins 40 lignes réelles des deux shells) ; erreur de détection récupérable d'un geste (préfixe forcé). |
| T7 | **FAIT en code le 20/11/2026 (1133 tests attendus, exe +68 Ko ; détail, bilan `TabExpansion2` et limites dans `docs/PLAN-ACTION-ESPACE-TERMINAL.md`).** **Complétion Tab** (menu sous la ligne), sources **maison**. | Chemins, commandes (`PATH`, alias, cmdlets connues) ; options : base embarquée des commandes courantes, **poids mesuré** avant / après. Bilan écrit : ce qui manque par rapport à `TabExpansion2` ; un PowerShell caché n'est rouvert que sur ce bilan. **T7bis (20/11/2026, 1145 tests)** : la même liste s'affiche aussi **sans Tab**, dès la 2e lettre, mise à jour à chaque frappe (historique d'abord, ↓/↑ puis Entrée) ; détail dans `docs/PLAN-ACTION-ESPACE-TERMINAL.md`. |
| T8 | **Explication** d'une commande ou d'une option, hors ligne. | Source : base maison des commandes courantes + `Get-Help` / `/?` / `--help` à la demande ; en français quand on l'a ; jamais d'envoi de la commande sur le réseau. |
| T9 | **Liens explorateur** : fichiers sélectionnés en paramètre (touche + glisser-déposer), sortie cliquable (chemins, `fichier:ligne`), commandes épinglées par dossier. | Chemins entre guillemets corrects avec espaces et accents ; Ctrl+clic ouvre dans SpaceFiles (souris : notée pour l'auteur, clavier possible en parallèle). |
| T10 | **Couleurs du thème** (idée 7, retenue le 19/11/2026) : les 16 couleurs ANSI projetées sur les jetons Nothing (blanc, orange d'accent), thème clair compris. | Un test calcule le contraste de chacune des 16 couleurs sur le fond sombre et sur le fond clair (seuil écrit avant le code) ; capture lue dans les deux thèmes (au-delà de deux images : `regard`). |
| T11 | **Profil Simple / Expert** (idée 8) : le terminal n'existe qu'en profil Expert (le profil existe depuis le commit `220296a`). | En Simple : Ctrl+Alt+T et le bouton « >_ » sont absents et aucune session n'est créée ; passer d'Expert à Simple ferme le terminal après avertissement s'il reste un programme lancé. |
| T12 | **Blocs de commandes** (idée 1, comme Warp) : chaque commande et sa sortie forment un bloc repliable et copiable, avec code de sortie et durée. Socle des étapes T15, T16 et T19. | Marqueurs OSC 133 lus sur des flux **réels** enregistrés de cmd et de PowerShell (T0 : ils arrivent sans réglage) ; tests sur ces flux ; repli dégradé (fin de commande détectée à l'invite) sous Windows 11 22H2 ; poids de l'exe mesuré avant / après. |
| T13 | **Palette Ctrl+K reliée** (idée 4) : préfixe `>` pour exécuter dans le terminal, et « Exécuter dans le terminal » dans le menu d'un fichier, `{fichier}` remplacé par la sélection (D-T9). | Chemins entre guillemets corrects avec espaces et accents ; collage multi-lignes : confirmation (§ 5) ; la commande part vers le shell détecté à T6. |
| T14 | **Commandes épinglées paramétrables** (idée 5) : par exemple `ffmpeg -i {fichier} …` sur la sélection, ré-exécutable d'un geste ; prolonge les commandes épinglées de T9. | Tests de substitution (`{fichier}`, `{dossier}`, espaces, accents, guillemets, sélection multiple) ; stockage par dossier. |
| T15 | **Explication des erreurs fréquentes** en français (idée 6) : « accès refusé », « chemin introuvable », « fichier utilisé par un autre processus » avec renvoi vers l'outil Restart Manager déjà dans l'app. Dépend de T12 (code de sortie) et de T8. | Table maison testée sur de vraies sorties d'erreur de cmd et de PowerShell ; texte jamais envoyé sur le réseau. |
| T16 | **Résultats cliquables comme des fichiers** (idée 2) : les lignes de `dir`, `ls` et `Get-ChildItem` se comportent comme dans la liste (sélection, clic droit, aperçu). Dépend de T9 et de T12. | Analyseur testé sur des sorties réelles (dates et colonnes selon la langue du système) ; une ligne non reconnue reste du texte simple ; souris : notée pour l'auteur. |
| T17 | **Aperçu avant l'irréversible** (idée 3) : avant `del *.tmp` ou `Remove-Item -Recurse`, la liste des fichiers touchés s'affiche dans le volet d'aperçu, avec la Corbeille proposée (D-T5 : « avertir d'abord »). Motifs simples seulement. | Liste exacte sur un jeu d'essai dans un dossier temporaire ; la commande n'est jamais exécutée par l'aperçu ; un motif trop complexe ne donne pas d'aperçu et le dit. |
| T18 | **Session restaurée** (idée 10) : onglets, dossiers et historique reviennent au redémarrage, pas les processus. | Redémarrage : mêmes onglets et mêmes dossiers, aucune commande rejouée ; l'historique respecte D-T7 (lignes de secrets exclues) ; données dans `%APPDATA%SpaceFiles`. |
| T19 | **Notification SpaceNotch** (idée 9) : une commande longue qui se termine déclenche une notification par les tubes `Eco.*`. Touche **les deux dépôts** : `docs/ecosystem/protocol.md` d'abord, accord de l'auteur avant de coder. Dépend de T12. | Contrat écrit et validé ; seuil de durée choisi (D-T10) ; message reçu par SpaceNotch sur un essai réel ; la ligne de commande ne part pas dans le message (confidentialité, à confirmer). |

**Capture.** Le panneau ne s'ouvre pas sans entrée ; pour `/snapshot`, prévoir en T2 un argument de lancement (ex. `--terminal`,
réservé aux essais) plutôt qu'un clic simulé.

## 5. Budgets et garde-fous

- **Poids de l'exe.** L'exe est à 4 472 832 o, déjà au-dessus du budget de 3,70 Mo : chaque étape **mesure avant / après** et le
  dit ; l'auteur décide si le dépassement est accepté (comme pour WebView2). Jamais de dépendance sans justification écrite.
- **Mémoire.** Le résident ne crée aucune session tant que le panneau est fermé. Le coût d'un `powershell.exe` fils est dans un autre
  processus : **à mesurer** (non connu), pas à supposer.
- **Sécurité.** Le shell tourne avec les droits de l'utilisateur, sans élévation. Collage de plusieurs lignes : demander
  confirmation. Aucun appel réseau ; aucune ligne de commande dans le journal technique (`journal.txt`).
- **Robustesse** (`panic = "abort"`) : pas de `unwrap()` sur les tuyaux ni l'analyseur ; un flux illisible affiche l'erreur dans la
  barre d'état et ferme l'onglet, pas l'application.
- **Accessibilité.** Le texte de la grille doit être lisible par UI Automation (lecteur d'écran) : à étudier en T2, sinon dit
  « non couvert ».
- **Souris.** Sélection, molette, poignée de hauteur, clic sur les liens, souris dans vim : écrits dans `docs/ui/rodage-f1-2026-11-09.md`.

## 6. Idées pour en faire une fonctionnalité phare (**retenues par l'auteur le 19/11/2026** : elles forment les étapes T10 à T19)

Classées par rapport gain / effort. Correspondance : idée 7 → T10 ; idée 8 → T11 ; idée 1 → T12 ; idée 4 → T13 ; idée 5 → T14 ; idée 6 → T15 ; idée 2 → T16 ; idée 3 → T17 ; idée 10 → T18 ; idée 9 → T19. L'ordre des étapes suit les dépendances (T12 est le socle de T15, T16 et T19), pas l'ordre des idées. Elles passent **après T9** ; chacune garde la règle n° 1 (une étape à la fois, qui se termine seule). Rien n'est codé.

1. **Blocs de commandes** (comme Warp) : chaque commande + sa sortie forme un bloc repliable, copiable, avec code de sortie et durée.
   Demande les marqueurs d'intégration (voir § 3).
2. **Résultats cliquables comme des fichiers** : la sortie de `dir` / `ls` / `Get-ChildItem` montre des lignes qui se comportent comme
   dans la liste (sélection, clic droit, aperçu). C'est ce que seul un terminal **dans** un explorateur peut offrir.
3. **Aperçu avant l'irréversible** : avant `del *.tmp` ou `Remove-Item -Recurse`, afficher dans le volet d'aperçu la liste des fichiers
   touchés (motifs simples seulement) et proposer la Corbeille plutôt que la suppression définitive.
4. **Palette Ctrl+K reliée** : préfixe `>` pour exécuter une commande dans le terminal, et « Exécuter dans le terminal » dans le menu
   d'un fichier (avec `{fichier}` remplacé par la sélection).
5. **Commandes épinglées paramétrables** par dossier (`ffmpeg -i {fichier} …` sur la sélection), ré-exécutables d'un geste.
6. **Explication des erreurs fréquentes** en français (table maison : « accès refusé », « chemin introuvable », « fichier utilisé par
   un autre processus » avec renvoi vers l'outil Restart Manager déjà dans l'app).
7. **Couleurs du thème** : les 16 couleurs ANSI projetées sur les jetons Nothing (blanc, orange d'accent), thème clair compris.
8. **Profil Simple / Expert** : terminal visible dans Expert seulement (le profil existe depuis le commit `220296a`).
9. **Écosystème** : une commande longue qui se termine déclenche une notification SpaceNotch par les tubes `Eco.*` (touche les deux
   dépôts : **contrat d'abord**, `docs/ecosystem/protocol.md`).
10. **Session restaurée** : onglets, dossiers et historique reviennent au redémarrage (pas les processus).

## 7. Questions encore ouvertes (à trancher avant l'étape concernée)

- **D-T1** Police de la sortie : Doto n'est pas faite pour une grille de code ; proposition : police à chasse fixe du système
  (Cascadia Mono, sinon Consolas) pour la sortie, Doto pour les repères (onglets, badge).
- **D-T2** Détection ambiguë (`cd`, `echo`, `type` existent dans les deux) : dernier shell utilisé, sans deviner davantage ?
- ~~**D-T3** Complétion~~ **Tranchée le 17/11** : sources maison ; `TabExpansion2` seulement à rouvrir après le bilan de T7. **Rouverte et retranchée le 20/11/2026 (réponses de l'auteur)** : (1) PowerShell : « la table la plus complète possible » → s'appuyer sur le vrai moteur (`TabExpansion2` et `Get-Command` dans un PowerShell caché), qui couvre cmdlets, modules, paramètres, variables, alias et fonctions du profil ; la table maison reste en secours hors ligne ; **coût à mesurer avant de livrer** (second processus, latence par frappe, mémoire du résident) ; (2) **étiquette de portée** dans la liste (« installe pour tout le PC » / « agit dans ce dossier ») pour les commandes connues, sans blocage ni confirmation ; (3) cmd : ajouter les **commandes internes** (dir, copy, del, set, ren, type…) avec description ; (4) cas à régler avant la suite : paramètres et variables (`-Path`, `$env:`, `%VAR%`), dossier réseau / WSL / administrateur, dossier du terminal différent de celui de la liste. Pas fait en code à cette date.
- ~~**D-T8** Plancher Windows~~ **Tranchée le 17/11** : Windows 11 22H2+ avec repli par lecture (§ 1).
- **D-T4** WSL (bash) : plus tard, hors de ce plan, ou jamais ?
- **D-T5** Commandes dangereuses (§ 6, idée 3, devenue T17) : avertir seulement, ou aussi proposer la Corbeille ?
- **D-T6** IA / langage naturel : reportée ; à rouvrir seulement avec un choix de modèle, de clé et de confidentialité.
- **D-T7** Historique : exclure automatiquement les lignes qui ressemblent à des secrets, et où le stocker (`%LOCALAPPDATA%\SpaceFiles`, non itinérant : choix de T5) ?
- **D-T9** (T13) « Exécuter dans le terminal » depuis le menu d'un fichier : la commande part-elle tout de suite ou est-elle seulement placée sur la ligne de saisie, l'auteur validant par Entrée ?
- **D-T10** (T19) Seuil de durée au-delà duquel une commande terminée notifie SpaceNotch (proposition : 30 secondes, réglable) ; et la notification dit-elle seulement « terminé » ou aussi le code de sortie ?

## 8. Limites

- T0 fait le 17/11/2026 (surcoût de `vte` mesuré : +10,5 Ko) ; T1 fait le 18/11/2026 (exe inchangé tant que `vt` n'est pas appelé : surcoût réel à mesurer à T2 ; pas de reflow, combinants ignorés, table de largeur approximative) ; T2 faite en code le 18/11/2026 (non commitée : session ConPTY + panneau, exe +80 896 octets, 920 tests ; état et points restants dans `docs/terminal/T2-etat.md` ; aucune session n'est lancée tant que le panneau est fermé ; preuve « pas de processus orphelin » non faite) ; T3 faite en code le 19/11/2026 (936 tests, exe 4 661 760 octets, relecture `rust-reviewer` corrigée mais non refaite ; souris vers les programmes plein écran, `more` / `python` / `winget` au clavier dans un vrai shell ; souris dans vim à vérifier par l’auteur) ; T4 à T9 non commencées ; mémoire d'une session et temps de démarrage encore **inconnus**.
- T4a faite en code le 19/11/2026 (non commitée : onglets multiples, 1001 tests, exe 4 775 424 octets ; détail, preuves et limites dans `docs/PLAN-ACTION-ESPACE-TERMINAL.md`). T4b faite en code le 19/11/2026 (non commitée : dossier synchronisé liste ↔ terminal par invite PowerShell OSC 9;9, 1010 tests, exe 4 781 056 octets ; liste → shell non vu dans la vraie fenêtre). T4c faite en code le 19/11/2026 (non commitée : journal des opérations en onglet JOURNAL, Ctrl+Maj+J, 1011 tests, exe 4 789 248 octets ; T4a et T4b sont commitées et validées). T4 est terminée.
- Vérifié sur internet (§ 2 bis) : le principe de ConPTY, la nécessité du passthrough pour les OSC, les limites de `vte` et
  d'`alacritty_terminal`, l'absence d'aide native sur le PowerShell 5.1 de l'auteur. **Non vérifié** : la valeur numérique officielle
  du drapeau passthrough (0x8 selon des copies tierces, mais `winconpty.h` de microsoft/terminal l'utilise pour autre chose),
  le comportement exact des OSC 133 sur le build 26300 (test à T0), le coût d'un PowerShell caché.
- La grille, le redimensionnement et l'écran alternatif restent le poste de travail le plus long (T1 à T3), même avec `vte` ;
  la souris (sélection, vim) ne se teste pas ici.
- Sur un Windows antérieur à 11 22H2, les blocs de commandes sont dégradés (choix assumé).
