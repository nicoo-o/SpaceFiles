# T0 : prototype ConPTY et trois mesures (17/11/2026)

Étape T0 de [PLAN-TERMINAL.md](PLAN-TERMINAL.md). Prototype jetable, **compilé seulement pour les tests** : `src/terminal/essai_t0.rs`
(session ConPTY : deux tuyaux, `CreatePseudoConsole`, liste d'attributs `PSEUDOCONSOLE`, `CreateProcessW`, fil de lecture, fermeture).
PC de l'auteur : Windows 11 10.0.26300, session élevée, PowerShell 5.1 (PSReadLine 2.0.0), `pwsh` absent.

## Critère d'acceptation

`cargo test cmd_echo_ok` : **réussi**. Un `cmd.exe /q` interactif reçoit `echo ok` et le flux ConPTY contient `ok`
(394 octets reçus, séquences d'initialisation `ESC[?9001h`, `ESC[?1004h`, effacement, titre, puis le texte).

## Constats à retenir pour T2 (la vraie session)

1. **`STARTF_USESTDHANDLES` est obligatoire dans les tests.** Sans lui, un fils lancé depuis un processus qui possède une console
   (`cargo test`, un terminal) écrit dans la console du **parent** et pas dans la pseudo-console : le premier essai ne rendait pas `ok`.
   Sans effet pour SpaceFiles (sous-système « windows », pas de console), mais à garder dans le code partagé.
2. **Un fils qui meurt aussitôt ne perd pas sa sortie.** `cmd.exe /c echo ok`, fermeture de la pseudo-console avec **0 ms** puis
   **1 000 ms** d'attente après la fin du fils : `ok` présent dans les deux cas (90 octets). Une première hypothèse « la sortie se perd »,
   notée dans le code pendant la mise au point, était un effet du défaut n° 1 et est **retirée**.
3. Le fil de lecture doit tourner avant la fermeture : `ClosePseudoConsole` attend que la sortie se vide.

## Mesure (a) : poids de `vte`

`vte` 0.15 ajouté à `Cargo.toml` et **réellement utilisé** depuis `main` (un `Perform` qui compte caractères, CSI et OSC, appelé depuis
un argument de ligne de commande, pour que l'édition de liens ne l'élimine pas), `cargo build --release` :

| | Taille de `spacefiles.exe` |
|---|---|
| Avant | **4 484 096 octets** |
| Après | **4 494 848 octets** |
| Écart | **+10 752 octets (+10,5 Ko)** |

Mesure unique, avec un usage minimal de `vte` (analyseur seul, sans la grille que nous écrirons). `Win32_System_Console` ne change pas
la taille de façon mesurable ici (la fonctionnalité n'ajoute que des déclarations). `Cargo.toml`, `Cargo.lock` et `main.rs` ont été
**remis dans leur état d'avant** : `vte` n'est pas dans le projet avant T1. Mémoire du résident : non mesurée (aucune session lancée).

## Mesure (b) : drapeau passthrough effectif

`CreatePseudoConsole` testé avec les drapeaux **0x0, 0x8, 0x10, 0x18**, pour `cmd` et `powershell` : **les quatre créations sont
acceptées**, et les flux reçus sont **identiques octet pour octet** (`cmp` : 0x0 = 0x8 = 0x10 = 0x18, pour les deux shells).

Conclusion honnête : **aucun de ces drapeaux n'a d'effet observable sur ce build**, donc la valeur du drapeau passthrough n'est **pas
établie** (soit il est ignoré, soit les valeurs candidates ne l'activent pas, soit le mode est déjà celui par défaut). On n'a donc pas
besoin de lui pour la suite, voir (c), et T2 créera la pseudo-console avec le drapeau 0 (comportement connu et mesuré).

## Mesure (c) : OSC 133 et OSC 9;9

Flux bruts enregistrés dans [flux-t0/](flux-t0/) (`.bin` brut, `.txt` lisible). Invite configurée pour émettre `ESC]133;A ESC\` puis
`ESC]9;9;<dossier> ESC\` ; commandes : `cd \`, puis `echo bonjour` (cmd) ou `Write-Host 'bonjour' -ForegroundColor Green` (PowerShell).

| Shell | Octets | OSC 133 | OSC 9;9 | Séquences CSI | Accepté avec les 4 drapeaux |
|---|---|---|---|---|---|
| cmd | 589 | **3** | **3** | 19 | oui |
| powershell 5.1 | 823 | **3** | **3** | 59 | oui |

**Les deux séquences arrivent intactes, y compris sans passthrough (drapeau 0)**, une par invite (3 invites : au lancement, après
`cd \`, après la commande). Le dossier courant arrive donc par `OSC 9;9` : `C:\Users\utilisateur\Desktop\projet perso code\nothing-explorer`
puis `C:\`. PowerShell envoie aussi les couleurs (`ESC[93m`, `ESC[38;5;10m` pour le vert de `Write-Host`) et son colorage de la ligne de
saisie par PSReadLine ; cmd n'en a pas.

À savoir pour T1 : le flux contient `ESC[?9001h` (mode de saisie Win32) et `ESC[?1004h` (suivi du focus) à l'ouverture, `ESC[?9001l` et
`ESC[?1004l` à la fermeture ; la grille doit les ignorer proprement. Les noms accentués (`réservés`, titre « Administrateur ») sont de
l'UTF-8 : le décodage doit être fait comme tel, pas en page de codes.

## Décision

**Continuer** (T1), sans changer le plancher Windows : le principe marche sur ce build, `vte` coûte ~10,5 Ko, les OSC 133 / 9;9 arrivent
sans réglage particulier. Le repli par lecture (sans OSC) reste prévu par le plan pour les Windows plus anciens, mais n'est pas nécessaire
ici.

## Non vérifié

- **Une seule machine, un seul build (26300)** : le comportement sur Windows 11 22H2 à 24H2 et sur Windows 10 n'est pas mesuré ; le plancher
  22621 du plan reste une hypothèse.
- Les drapeaux ne changent rien ici, mais je n'ai pas lu quelle valeur Windows attribue à chacun : le passthrough n'est donc pas « testé
  et trouvé inutile », il est « non distinguable sur ce flux ».
- Les flux enregistrés viennent d'une session **élevée** ; une session normale peut différer (titre, invite).
- Redimensionnement, entrée au clavier (touches spéciales, mode Win32 `?9001`), écran alternatif (`vim`, `less`), très gros débit : pas
  testés, ils relèvent de T1 à T3.
- Mémoire et temps de démarrage d'une session : non mesurés.
- `vte` a été utilisé mais son résultat (comptes) n'a pas été relu sur ces flux ; seul le poids a été mesuré.
