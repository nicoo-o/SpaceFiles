# Protocole inter-applications, version 1 (brouillon à relire)

Source : `PROJECT_REBOOT_MASTERPLAN.md` §3.1, 3.2, 3.5 et 6. Ce document **spécifie**. Côté SpaceFiles (phase 4, lot A,
bibliothèque `src/eco/`) sont implémentés et testés : `hello`, `search.query`, `explorer.open`, l'émission de `progress`,
le serveur de tube à plusieurs instances, le client borné à 300 ms, le manifeste et la lecture de `shared.json`.
Côté SpaceNotch (phase 4, lot B, 02/11/2026, ADR-033 de SpaceNotch) sont implémentés et testés : son tube
`Eco.spacenotch.<utilisateur>` (4 instances), son `hello` (capacité `progress`), son manifeste `spacenotch.json`, la
réception de `progress` (affichée dans le canal de la notch), le client générique et l'appel `explorer.open`.
Preuve croisée : `tools/bench/preuve-lot-b-notch.cs`. Non vu à l'écran : focus de la fenêtre ouverte, affichage dans la notch.
Tout ce qui n'est pas dans le plan est marqué **[à décider]** ; chaque ajout du lot A est marqué **à relire par l'auteur**.

**Décision de l'auteur (01/11/2026, lot A)** : l'identifiant de l'application et le nom du tube sont **`spacefiles`**
(`Eco.spacefiles.<utilisateur>`, `spacefiles.json`), et non `explorer` comme dans le brouillon ; l'appel garde son nom
`explorer.open`. Tout `explorer` dans les exemples ci-dessous veut dire `spacefiles`.

## Principes

- Aucune application n'en démarre une autre pour fonctionner. Tout appel entre applications est facultatif et borné à
  **300 ms**, puis l'appelant se dégrade (il continue sans réponse).
- Un plantage reste dans son processus. Pas de mémoire partagée.
- Le Hub (application à part, lancée à la demande) ne fait que lire les manifestes et écrire `shared.json`. S'il est
  fermé, rien ne se dégrade.

## Transport

Tube nommé (named pipe), un par application : `\\.\pipe\Eco.<app>.<utilisateur>`, accessible au seul utilisateur courant
(`CurrentUserOnly`). Pas de port réseau, pas d'HTTP local. `WM_COPYDATA` reste interne à l'Explorateur (transfert
d'onglets). Le schéma d'URL (`spacefiles://`, ancien `nothingexplorer://` gardé en alias une version) sert aux liens
profonds à sens unique (« ouvre ceci »).

## Trame

Une ligne JSON UTF-8 terminée par `\n`, sans retour à la ligne à l'intérieur.

```json
{"v":1,"id":"42","from":"spacenotch","kind":"call","name":"explorer.open","args":{"path":"C:\\Users"}}
```

| Champ | Type | Sens |
|---|---|---|
| `v` | entier | version du protocole (1) |
| `id` | chaîne | identifiant choisi par l'appelant ; une réponse le reprend |
| `from` | chaîne | identifiant de l'application émettrice (`spacefiles`, `spacenotch`, `hub`…) |
| `kind` | `call` \| `reply` \| `event` | appel, réponse, événement sans réponse |
| `name` | chaîne | nom de l'appel ou de l'événement, `domaine.verbe` |
| `args` | objet | arguments (`call`, `event`) |
| `result` / `error` | objet / `{code,message}` | contenu d'une `reply` (l'un ou l'autre) |

Bornes : une ligne fait **4 096 octets au plus** (UTF-8, `\n` et `\r` finaux exclus), **32 Ko** (32 768 octets) pour
`explorer.open` seulement (les chemins sont longs). Une ligne plus longue est refusée avec `error.code = "trop_long"` puis
la connexion est fermée. Le serveur ne connaît le nom de l'appel qu'une fois la ligne complète : il lit donc jusqu'à
32 Ko, puis refuse `trop_long` tout appel autre que `explorer.open` qui dépasse 4 096 octets, et tout appel au-delà de
32 Ko (**à relire par l'auteur**). Un champ inconnu est ignoré ; un `name` inconnu reçoit `error.code = "inconnu"`. Un `v`
supérieur à celui qu'on parle reçoit `error.code = "version"`.

**Versionnage** (testé, **à relire par l'auteur**) : version reçue / version parlée (nous parlons la 1).

| Reçu | Réponse |
|---|---|
| `v` absent | traité comme 1 |
| `v` = 1 (ou 1.0) | normal |
| `v` > 1 | `error.code = "version"`, même si le `name` est inconnu (la version prime) |
| `v` = 0, négatif, fractionnaire, ou pas un nombre | `invalide` |
| champ inconnu (n'importe où) | ignoré |
| `name` inconnu | `inconnu` |
| `kind` autre que `call` (côté serveur de SpaceFiles : un appel est attendu) | `invalide` |
| un `hello` reçu par un client annonçant `protocol` > 1, ou sans `protocol` | le client se dégrade (`Protocole`) : il n'envoie rien d'autre à cette application |

## Appels et événements de la version 1

| Nom | Sens | Contenu |
|---|---|---|
| `hello` | appel, toutes les applications | réponse : `{app, version, protocol, capabilities[], settingsSchema?}` |
| `search.query` | appel vers le résident SpaceFiles | `{q, limit?}` : cherche dans l'index de fichiers (voir plus bas) |
| `explorer.open` | appel vers le résident SpaceFiles | `{path, newTab?, reveal?, reuse?}` : ouvre un dossier, dans une fenêtre déjà ouverte si `reuse` (voir plus bas) |
| `progress` | événement de SpaceFiles vers SpaceNotch | `{op, state, done, total}` : avancement d'une opération de fichiers |

Capacités annoncées par le `hello` de SpaceFiles (implémentées et testées) : `search.query`, `explorer.open`. `progress`
est un événement que SpaceFiles *émet* : il n'est pas une capacité de SpaceFiles ; SpaceNotch dira dans son propre
`hello` s'il le reçoit : le `hello` de SpaceNotch annonce `["progress"]` (lot B). Les capacités du Hub restent **[à décider]**.

## Découverte

Chaque application écrit son manifeste `%LOCALAPPDATA%\Eco\apps\<id>.json` au démarrage (schéma :
`manifest.schema.json`, ici même). Un manifeste dont le processus a disparu est affiché « arrêté », pas supprimé.

**Manifeste de SpaceFiles** (implémenté, **à relire par l'auteur**) : le résident écrit `%LOCALAPPDATA%\Eco\apps\spacefiles.json`
à son démarrage, **une fois le tube créé** (s'il n'a pas pu le créer, il n'annonce rien). Champs : `id:"spacefiles"`,
`name:"SpaceFiles"`, `version` (celle du binaire), `exe` (chemin complet de l'exe), `pipe` (`Eco.spacefiles.<utilisateur>`, sans le
préfixe `\\.\pipe\`), `protocol:1`, `capabilities:["search.query","explorer.open"]`, `settingsLink:"spacefiles://settings"`.
Écriture atomique (temporaire à nom unique + `sync_all` + renommage) ; jamais supprimé à l'arrêt. Mode essai
(`SPACEFILES_PIPE`) : écrit sous le `LOCALAPPDATA` du processus, donc dans le faux profil ; si ce `LOCALAPPDATA` est le vrai
dossier de l'utilisateur, rien n'est écrit. Mode portable (`portable.txt` à côté de l'exe) : rien n'est écrit (**[à décider]**).
Le chemin `exe` sert aux clients : voir « Règle côté client ». Chaque application doit donc écrire son manifeste *avant*
d'ouvrir son tube. SpaceNotch le fait (lot B) ; SpaceFiles écrit le sien *après* avoir créé le tube (voir plus haut), écart
sans conséquence tant qu'un client se dégrade proprement sur un manifeste absent.

## Réglages partagés

`%LOCALAPPDATA%\Eco\shared.json` ne contient que : thème (`dark`, `light`, `system`), accent, mouvement (`full`,
`reduced`), langue, registre des raccourcis (identifiant de commande → touche). Chaque application a des valeurs par
défaut si le fichier manque. Écriture atomique (fichier temporaire puis renommage), comme `reglages.rs`.

**Format et lecture côté SpaceFiles** (implémenté, **à relire par l'auteur** ; les noms de champs sont ma proposition) :

```json
{"theme":"dark","accent":"#D71921","motion":"full","language":"fr","shortcuts":{"explorer.nouvelOnglet":"Ctrl+T"}}
```

- Valeurs par défaut (fichier absent, vide, abîmé, trop gros > 256 Ko, ou champ invalide, champ par champ) : `dark`,
  `#D71921`, `full`, `fr`, aucun raccourci. Jamais d'erreur ni de panique.
- **Le BOM UTF-8 est toléré** (AUDIT R23), ainsi que l'UTF-16 avec BOM (PowerShell 5.1).
- Un champ **inconnu** (écrit par une version plus récente) est conservé tel quel à la réécriture ; un champ connu à valeur
  invalide est remplacé par son défaut à la réécriture.
- Écriture : temporaire à nom unique + `sync_all` + renommage ; deux écrivains en même temps laissent toujours un fichier
  complet (testé). Le code d'écriture existe dans `src/eco/partage.rs` mais n'est compilé qu'en test (SpaceFiles n'écrit
  pas `shared.json` avant les phases 6 et 7 : c'est le Hub qui l'écrit).
- **Phase 6, lot B : le Hub est le premier écrivain** (ADR-037 de SpaceNotch, section « Le Hub écrit shared.json »). Page
  « Apparence » : seuls `theme` et `motion` sont écrits (`system`/`dark`/`light`, `full`/`reduced`) ; tout le reste (accent,
  langue, raccourcis, champs inconnus) est conservé tel quel. La forme écrite est du JSON indenté de deux espaces, UTF-8 sans
  BOM, `\n`. Le Hub n'écrit jamais un fichier que SpaceFiles rejette : le fichier courant et le résultat doivent être dans le
  sous-ensemble commun aux deux lecteurs (au plus 8 conteneurs imbriqués, 1 024 clés par objet, aucune clé répétée dans un objet,
  nombres de 32 caractères au plus et finis, pas de substitut `\u` isolé, 256 Ko) ; sinon rien n'est écrit et « Réinitialiser »
  est proposé. Un fichier abîmé n'est jamais écrasé sans ce clic (il écrit `{}`). Aucune réécriture si le choix n'a pas changé.
  Dernier écrivain gagne, sans verrou. Test Rust des bornes : `hors_sous_ensemble_commun_rejete_par_spacefiles`.
  Le test `octets_ecrits_par_le_hub` (`src/eco/partage.rs`) relit les octets exacts de l'écrivain.
- **Phase 6, lot C : le Hub lit, sans rien écrire, ce que SpaceFiles laisse sur la machine** (ADR-037 de SpaceNotch, section
  « Raccourcis, Modules, Santé »). Ce qu'il en lit : le journal `%LOCALAPPDATA%\SpaceFiles\journal.txt` (la fin seulement, 64 Ko ;
  une erreur est une ligne `AAAA-MM-JJ HH:MM:SS ERREUR module message` de `journal::ecrire`, ou l'en-tête d'un plantage
  `AAAA-MM-JJ HH:MM:SS · version …` avec la ligne qui suit), la valeur `SpaceFilesWinE` de `HKCU\...\Run` et la fenêtre invisible de
  classe `SpaceFilesResident` (ligne « Win+E »). Ces trois formats sont donc un contrat de fait : en changer un oblige à changer le
  Hub. Manque signalé : le seul appel de `journal::ecrire` à niveau `Erreur` fixe est dans `systeme.rs` (échec de ShellExecute) ;
  `eco/mod.rs` et `migration.rs` passent un niveau variable. La page Santé dira donc « aucune erreur » pour SpaceFiles dans
  presque tous les cas. Rien n'a été ajouté au journal pour ce lot.
- **Phase 4 : lecture une seule fois, au démarrage du résident** (mise en cache, une ligne de journal d'état).
- **Phase 5, lot D : le thème et le mouvement sont honorés, à chaud** (ADR-036 de SpaceNotch, 02/11/2026, **à relire par
  l'auteur**) :
  - **Précédence** : réglage local explicite > partagé > défaut. Pour le mouvement, la réduction la plus restrictive gagne
    (OU) : `reduced` éteint les animations ; `full` n'en allume jamais de force (ni contre Windows, ni contre un réglage
    local qui les éteint). En clair : `reduced` partagé OU « Effets d'animation » coupés dans Windows OU réglage local coupé.
  - **Absence d'avis** : fichier absent, abîmé, trop gros, OU champ absent / invalide d'un fichier présent = **aucun avis** :
    on garde le comportement d'avant. Les valeurs par défaut (`dark`, `full`…) ne sont jamais appliquées comme un avis
    (`avis_theme` / `avis_mouvement` en Rust, `ThemeOpinion` / `MotionOpinion` en C#). **Les valeurs sont sensibles à la
    casse** : `"Reduced"` ou `"LIGHT"` = aucun avis.
  - **Échec d'entrée-sortie ≠ fichier abîmé** (côté SpaceFiles, `Etat::Illisible`) : un fichier verrouillé ou une ouverture
    refusée après 4 essais ne fige pas « aucun avis ». Le tampon ne mémorise pas la signature, **garde le dernier avis connu**
    et relit au contrôle suivant (toujours au plus une fois par seconde). Seul « absent » ou un contenu mauvais donnent
    « aucun avis ».
  - **Bornes de lecture** : 256 Ko au plus ; **1 024 clés au plus par objet** de `shared.json` (256 pour les trames du tube,
    inchangé) ; 1 024 raccourcis au plus (aligné sur la borne de clés). Au-delà : fichier abîmé = aucun avis.
  - **Relecture sans fil ni minuterie** (SpaceFiles) : chaque fenêtre, processus séparé, relit le fichier **à la fin d'une
    lecture de dossier, au plus une fois par seconde**, avec un tampon (date de modification + taille). Pas de scrutation.
    Un fichier absent coûte un `metadata` par seconde. Pas de lecture en mode portable (testé avant tout accès au disque) ni
    sans `LOCALAPPDATA`. SpaceNotch, lui, observe le dossier `Eco` (`FileSystemWatcher`, regroupement 200 ms).
  - **Ce que `reduced` coupe aujourd'hui dans SpaceFiles** : seulement l'apparition en points d'un dossier (seul appelant :
    `app.rs`, une fois par navigation). Rien d'autre n'est encore relié.
  - **SpaceNotch honore thème et mouvement ; SpaceFiles honore `motion` et, depuis le lot B de la phase 7, `theme`** (thème clair,
    réglage local prioritaire) ; `accent` n'est toujours pas honoré. Langue et raccourcis partagés : hors lot.
  - **Limites connues** : (1) même date + même taille = changement invisible (granularité FAT/exFAT de 2 s) ; (2) `LOCALAPPDATA`
    sur un partage réseau : le contrôle (`metadata`, au plus une fois par seconde) peut bloquer le fil d'interface, il n'y a
    pas de fil de travail dans ce lot ; (3) un changement n'est vu qu'à la prochaine lecture de dossier.
  - Personne n'écrit encore le fichier (le Hub est la phase 6) : tout est prouvé par des fichiers écrits à la main
    (`tools/bench/preuve-phase5-partage.cs` pour SpaceNotch ; les tests de `src/eco/partage.rs` pour la lecture Rust).

## Journal

Convention proposée le 01/11/2026, à confirmer par l'auteur (implémentée par `journal::ecrire` dans `src/journal.rs`) :

- Fichier : `%LOCALAPPDATA%\<Application>\journal.txt` (celui de l'Explorateur : `SpaceFiles`), UTF-8, fins de ligne CRLF.
- Une trace = une ligne : `AAAA-MM-JJ HH:MM:SS NIVEAU module message`, heure locale. `NIVEAU` vaut `INFO`, `AVERT` ou `ERREUR`.
  `module` est un mot sans espace (`main`, `systeme`…). Les sauts de ligne du message sont remplacés par un espace.
- Plafond de 256 Ko : au-delà, le fichier passe en `journal.ancien.txt` et un nouveau repart (même fonction `ajouter` pour tout).
- Exemple : `2026-11-01 13:26:35 INFO main demarrage version 0.1.0 avec ["C:\\Windows"]`.
- Modules de la bibliothèque Eco (**à relire par l'auteur**) : `eco` (démarrage : manifeste écrit, `shared.json` lu ou abîmé),
  `eco-serveur` (tube : démarré avec N instances, arrêté, client inactif déconnecté, trame trop longue), `eco-client`
  (émission de `progress` : disponible / indisponible, une ligne à chaque changement), `service_recherche` (index : chargement,
  libération, requête lente). **États seulement : jamais le contenu d'un appel, un texte cherché, un chemin ni un nom de
  fichier.**
- Le crochet de panic garde son format propre (en-tête horodaté, fil, message sur plusieurs lignes) dans le même fichier.
- Pas de trace dans les chemins chauds (défilement, rendu, minuteries) : chaque appel ouvre et ferme le fichier.

## Service de recherche du résident : `search.query` (implémenté, **à relire par l'auteur**)

Le résident (`spacefiles.exe --resident`, jamais une fenêtre normale) sert le tube
`\\.\pipe\Eco.spacefiles.<utilisateur>` (nom d'utilisateur Windows, tout caractère hors `A-Za-z0-9._-` remplacé par `_`).
**Plusieurs clients à la fois** (lot A, **à relire par l'auteur** ; l'ancienne limite « un seul client à la fois » est
levée) : **4 instances** du tube attendent en même temps, chacune servie par son propre fil (4 fils au repos, tous bloqués
en attente de connexion ; le lot 3 en avait 1). Un client lent ou muet n'occupe qu'une instance. Au-delà de 4 clients
simultanés, un cinquième trouve le tube occupé (`ERROR_PIPE_BUSY`) et attend qu'une instance se libère, dans son budget de
300 ms. Des appels courts : se connecter, appeler, se déconnecter. Un client connecté
qui n'envoie rien pendant **10 s** est déconnecté (`SPACEFILES_INACTIVITE_MS`, essais seulement). Un client qui se connecte
puis ferme aussitôt n'arrête jamais le service. La variable
d'environnement `SPACEFILES_PIPE` remplace `<utilisateur>` : **essais seulement**, absente en usage normal (elle donne aussi
un résident de test sans crochet clavier, voir `tools/bench/service-recherche.ps1`).

**Accès.** Le tube est créé avec une DACL explicite `D:P(A;;GA;;;<SID de l'utilisateur>)` (la DACL par défaut d'un tube
donne la lecture à Everyone et à l'accès anonyme), `PIPE_REJECT_REMOTE_CLIENTS` et `FILE_FLAG_FIRST_PIPE_INSTANCE` pour
la **première** instance (un processus qui tient déjà le nom l'emporte : le service ne démarre pas, il le journalise ; les
trois autres instances rejoignent le nom que nous tenons déjà). La DACL est suivie de l'étiquette d'intégrité moyenne
`S:(ML;;NWNR;;;ME)` : un processus d'intégrité basse (bac à sable) ne peut ni lire ni écrire le tube. Un masque d'accès plus
étroit (`0x12019b`, sans le droit de créer une instance) a été essayé et **abandonné** : la DACL du tube existant s'applique
aussi au résident, qui ne pouvait plus créer ses instances 2 à 4 (essai : 1 instance sur 4, clients refusés).

**Limite connue : le nom du tube.** `<utilisateur>` est la variable `USERNAME`, pas le SID. Deux comptes dont les noms
deviennent identiques après nettoyage, ou une variable modifiée, donnent le même nom ; la DACL (SID) reste, elle, exacte.
Le journal du résident le rappelle au démarrage du service.

**Règle côté client (appliquée par `eco::client` de SpaceFiles, lot A, et par `EcoClient` de SpaceNotch, lot B).** La DACL
protège le *serveur* (qui peut se connecter), pas le *client* : un autre compte local peut créer avant le résident un tube
du même nom et recevoir les requêtes. Avant d'envoyer une requête, le client : (1) ouvre le tube avec
`SECURITY_SQOS_PRESENT | SECURITY_IDENTIFICATION` (le serveur ne peut alors qu'identifier le client, jamais agir en son
nom) ; (2) lit `GetNamedPipeServerProcessId`, ouvre ce processus, lit le SID de son jeton et vérifie qu'il est **égal au
SID du client**, que son **intégrité est moyenne ou plus**, qu'il n'est **pas dans un AppContainer**, et (**à relire par
l'auteur**) que son **exécutable est celui du manifeste** de l'application (`exe`) ; sinon il se dégrade (comme sans réponse,
résultat `NonFiable`) sans rien envoyer. Un manifeste absent ou sans `exe` donne aussi `NonFiable`. **Limite de cette vérification** : le manifeste est un fichier que
n'importe quel processus du même utilisateur peut modifier ; elle protège d'un **imposteur accidentel** (un autre programme
qui prend le nom du tube, une ancienne version), **pas** d'un processus malveillant du même compte. Les chemins d'exe sont
comparés sans tenir compte de la casse (Unicode compris) puis d'après leurs chemins finaux (une jonction ou un nom 8.3
vers le même fichier est accepté). Le manifeste lu est gardé en mémoire et n'est relu que si sa date ou sa taille change
(lecture bornée à 64 Ko). Pour un tube injecté en
essai (`SPACEFILES_PIPE`), le chemin de l'exe n'est pas exigé. Budget total **300 ms** (connexion, vérification, envoi,
réponse) ; la réponse à `hello` est gardée 60 s. Résultat typé : `Ok(réponse)`, `Absent` (pas de tube : immédiat), `Delai`,
`NonFiable`, `Protocole` (réponse illisible ou autre protocole), `Erreur{code}`. Jamais d'appel depuis le fil d'interface.

**Trame.** L'enveloppe de la section « Trame ». Une ligne de **4 096 octets au plus** (saut de ligne exclu) en requête
comme en réponse (32 Ko pour la requête `explorer.open`) : une réponse trop grosse contient moins de résultats
(`truncated:true`), jamais une ligne coupée. `id` (chaîne de 64 octets au plus, recopiée dans la réponse), `v` (1) et
`kind` (`call`) sont facultatifs en entrée.

**`hello`** → `result: {"app":"spacefiles","version":"0.1.0","protocol":1,"capabilities":["search.query","explorer.open"]}`.

**`search.query`** : `args: {"q": "<texte>", "limit": 30}`.

| Champ | Règle |
|---|---|
| `q` | obligatoire, chaîne. Moins de 3 caractères (espaces de bord retirés) : réponse vide, sans erreur. Mêmes filtres que la palette (`ext:pdf`, `taille>10mo`, `dans:`…) sauf `contenu:` (lirait des fichiers de l'utilisateur) : erreur `non_supporte`. |
| `limit` | facultatif, entier de 1 à 30, 30 par défaut. Hors bornes ou fractionnaire : erreur `hors_bornes`. |

Réponse : `result: {"results":[{"name":"Windows","path":"C:\\Windows","dir":"C:\\"}],"truncated":false}`.
`path` est le chemin complet, `dir` le dossier qui le contient. Les chemins sont échappés en JSON (guillemets, antislash,
caractères de contrôle ; les accents et emojis restent en UTF-8). `truncated:true` : il y avait d'autres résultats que
ceux renvoyés (au-delà de `limit` ou retirés pour tenir en 4 096 octets). L'ordre est celui de l'index (dossiers proches
de la racine d'abord), sans classement par pertinence.

**`loading`.** L'index n'est pas gardé en mémoire au repos. La première `search.query` lance son chargement en
arrière-plan (index.bin lu, ou construit s'il manque ; reconstruit s'il a plus de 6 h) et la réponse est tout de suite
`result: {"results":[],"loading":true}`. L'appelant réessaie plus tard ou se dégrade à ses 300 ms. Mesuré sur l'index de
l'auteur (2 749 160 éléments) : résultats réels environ 0,1 à 0,2 s après la première requête.

**Libération.** 5 minutes sans `search.query` : l'index est libéré (mémoire privée du résident revenue à ses ~4 Mo) et la
surveillance des dossiers s'arrête ; la requête suivante repasse par `loading`. Une construction en cours (index absent ou
périmé) retarde la libération jusqu'à sa fin. Minuterie à usage unique du résident, réarmée à chaque requête. Variable
`SPACEFILES_LIBERATION_S` (1 à 86 400 s) : essais seulement.

**Annulation.** Une `search.query` suivie d'une autre ligne **quelconque** (n'importe quel appel, pas seulement une autre recherche) déjà envoyée sur la même connexion est abandonnée **sans réponse** (le
balayage de l'index consulte le tube tous les 16 384 éléments) ; seule la dernière reçoit sa réponse. Un client qui
envoie une requête à la fois n'est jamais concerné.

## `explorer.open` (implémenté, **à relire par l'auteur**)

`args: {"path": "<chemin>", "newTab": false, "reveal": false, "reuse": false}`. `newTab` est lu (un type faux est `invalide`)
mais **ignoré**. Sans `reuse` (ou `reuse:false`), le résident ouvre une **nouvelle fenêtre**, comme avant. Avec `reuse:true`,
il essaie d'abord de confier le chemin à une fenêtre SpaceFiles **déjà ouverte** (voir « Réutiliser une fenêtre » plus bas).

| Champ | Règle |
|---|---|
| `path` | obligatoire, chaîne. Chemin complet local (`C:\…`) ou UNC (`\\hôte`, `\\hôte\partage\…`). Refusé `invalide` : relatif, nom de programme (`calc.exe`), tout schéma (`https://…`, `ms-settings:`), flux de données (`C:\a:b`), espace de noms de périphériques (`\\.\`, `\\?\`), hôte fait de points (`\\.`, `\\./pipe/x`), nom de périphérique réservé (`CON`, `PRN`, `AUX`, `NUL`, `COM1` à `COM9`, `LPT1` à `LPT9`, avec ou sans extension, à n'importe quel niveau du chemin), caractère de contrôle (NUL compris), caractère de direction du texte (bidi), séparateurs U+2028 et U+2029, espaces de largeur nulle (U+200B à U+200D), U+FEFF, caractère interdit par Windows (`"<>|*?`). `hors_bornes` à partir de **32 000 caractères**. |
| `reveal` | facultatif, booléen. Vrai : la fenêtre s'ouvre sur le **dossier parent** avec l'élément choisi (c'est `--montrer`, déjà connu de la fenêtre) — pour un fichier comme pour un dossier. |
| `newTab` | facultatif, booléen, ignoré (voir ci-dessus) |
| `reuse` | facultatif, booléen, faux par défaut (`null` = absent). Un autre type est `invalide`, comme `newTab` et `reveal`. Vrai : essayer une fenêtre ouverte avant d'en lancer une nouvelle. |

**Ce qui est exécuté.** Le chemin n'est **jamais exécuté** et le résident ne touche **jamais au disque** pour lui : le fil du
tube valide le **texte** (les `/` sont d'abord remplacés par des `\`), applique les limites de débit et met la demande dans une
file bornée (8). Un fil de lancement du résident, distinct du fil du crochet clavier et de la fenêtre cachée, lance une nouvelle
fenêtre de `spacefiles.exe` avec **toujours** un drapeau dédié : `--ouvrir "<chemin>"` (ou `--montrer "<chemin>"` quand `reveal`
est vrai), antislash finaux doublés pour la ligne de commande. C'est la **nouvelle fenêtre** qui décide, dans son propre
processus et en une seule consultation du disque : un dossier est ouvert ; un fichier, un chemin disparu ou `reveal` donnent le
dossier parent avec l'élément choisi ; `\\hôte` et la racine d'un lecteur (`C:\`) s'ouvrent tels quels. Jamais l'Explorateur de
Windows, jamais l'exécution du fichier. Un lancement qui échoue est journalisé (état seulement, sans chemin). Il se fait avec
`ShellExecuteExW` (`SEE_MASK_NOCLOSEPROCESS`) : **une fenêtre lancée par le résident n'a pas, de droit, le premier plan**.
Deux relais : SpaceNotch appelle `AllowSetForegroundWindow` avec le pid vérifié du résident avant `explorer.open`, puis le
résident appelle `AllowSetForegroundWindow` avec le pid du processus qu'il vient de lancer (journal : état « accorde » ou
« refuse »). **Non vu à l'écran** : point de rodage F1.

**Réutiliser une fenêtre (`reuse:true`, implémenté, à relire par l'auteur, phase 5 lot A).** La réponse est la même, `{"opened":true}`,
dès que la demande est en file : on ne dit pas si une fenêtre est réutilisée. Mêmes validations de chemin, même seau de débit
(une demande `reuse` consomme une place comme une autre). Le **fil de lancement** du résident (jamais le fil du tube ni le crochet
clavier) traite la demande ainsi :
1. **Choix des fenêtres.** Parmi les fenêtres de premier niveau de classe `SpaceFiles`, dans l'ordre Z (la plus haute d'abord),
   celles qui sont **visibles**, d'un **autre processus** que le résident, **dont l'exécutable est le même que celui du résident**
   (`QueryFullProcessImageNameW` des deux côtés, comparaison insensible à la casse), au plus **3**. La fenêtre cachée du résident a sa
   propre classe (`SpaceFilesResident`) et son pid : elle n'est jamais prise. Une fenêtre dont l'envoi a **expiré** est **sautée
   pendant 10 s** (le résident ne reste pas 2 s sur chaque demande devant une fenêtre figée).
2. **Droit de premier plan.** Avant chaque envoi, le résident appelle `AllowSetForegroundWindow(pid de la fenêtre cible)` (le droit
   lui a été donné par SpaceNotch avant l'appel ; un refus de Windows n'est pas une erreur, l'état est journalisé sans chemin).
3. **Message.** `WM_COPYDATA` avec `dwData = OUVRIR_ICI = 0x4E450002` (voisin de `0x4E450001`, les onglets copiés), charge **UTF-16**
   sans zéro final : `<r>\n<tick>\n<chemin>` (les `\n` sont de vrais sauts de ligne U+000A). `<r>` vaut `1` si `reveal` est vrai,
   `0` sinon ; `<tick>` est l'instant d'envoi en entier décimal (`GetTickCount64`, millisecondes, commun à toute la session) ;
   `<chemin>` est le chemin déjà validé et normalisé. Envoi par `SendMessageTimeoutW` (`SMTO_ABORTIFHUNG`, **2 000 ms** au plus).
   Réponse `1` = acceptée ; tout le reste (`0`, délai, fenêtre figée, aucune fenêtre) = **repli** : la demande est lancée comme
   sans `reuse` (`--ouvrir` / `--montrer`). Seul un **échec immédiat pour accès refusé** (`ERROR_ACCESS_DENIED`, fenêtre élevée) fait
   essayer la fenêtre suivante ; un délai ou une réponse `0` ne passent pas à la suivante.
4. **Réception (dans la fenêtre).** N'importe quel processus du même utilisateur peut écrire à une fenêtre : la fenêtre ne fait
   pas confiance à l'expéditeur. Elle refuse (`0`) toute autre marque, toute charge de plus de **64 Ko**, de longueur impaire (UTF-16
   tronqué), avec un surrogate isolé, sans les deux `\n`, dont le premier champ n'est pas `0` ou `1`, dont l'instant n'est pas un
   entier décimal, ou dont le chemin est refusé par les règles de `path` ci-dessus. Elle refuse aussi toute charge **de plus de
   1 500 ms** à la réception (le résident a renoncé à 2 000 ms et s'est replié : l'ouvrir aussi donnerait deux ouvertures), et
   quand **4 consultations du disque** sont déjà en vol. Sinon elle se restaure si elle est réduite et se met au premier plan, puis
   résout le chemin avec la même règle que `--ouvrir` / `--montrer` (une seule consultation du disque, **dans un fil court** : un
   partage réseau mort ne fige pas la fenêtre). Le fil dépose le résultat dans une file protégée du processus et poste à la fenêtre
   `WM_OUVRIR_ICI` **sans paramètre** ; sur son fil d'interface la fenêtre vide la file pour son handle, ferme ce qui visait l'onglet
   affiché (menu, palette, renommage par lot, renommage en cours, galerie) et ouvre un **nouvel onglet** sur le dossier résolu avec
   l'élément choisi.

Limites connues. (a) La réponse `1` veut dire « acceptée et en cours », pas « l'onglet est ouvert » : l'onglet apparaît juste
après la réponse. Une fenêtre fermée pendant ce court intervalle perd la demande, sans repli. (b) Une fenêtre qui met plus de 2 s à
répondre est tenue pour absente : le résident se replie, et si l'ancienne traite le message plus tard, l'âge de la charge
(> 1 500 ms) lui fait répondre `0` sans rien ouvrir. Reste un intervalle étroit, entre 1 500 ms et 2 000 ms, où la fenêtre a refusé
sans que le résident ait encore renoncé : il se replie alors sur le `0`, un seul lancement. (c) Avec plusieurs fenêtres, seule la
plus haute est essayée sauf accès refusé (jusqu'à 3). (d) Le premier plan réel après `SetForegroundWindow` n'est pas garanti
(voir le droit ci-dessus). (e) Essais seulement : `SPACEFILES_CLASSE_CIBLE` (nom de classe), lue **seulement si `SPACEFILES_PIPE`
est posée**, remplace la recherche par celle d'une fenêtre **message-only** de cette classe, sans exiger de visibilité ni
d'exécutable identique (le pid du résident reste exclu) ; en usage normal elle est ignorée.

**Ce que « ouvert » veut dire.** Réponse `result: {"opened":true}` dès que la demande est dans la file du résident. La fenêtre
n'est pas attendue, et rien ne dit qu'elle s'affichera (chemin introuvable, session verrouillée…).

**Limite de débit.** Pour tous les clients ensemble : au plus **5 ouvertures par seconde glissante**, une **rafale de 5** puis une
ouverture de plus **toutes les 2 s** (seau), **20 par minute** au plus, et 8 demandes en attente de lancement ; au-delà
`error.code = "trop_vite"` (réessayer plus tard). Les appels invalides et les demandes refusées ne consomment rien.

**Progress (événement, implémenté, à relire par l'auteur).** SpaceFiles envoie à SpaceNotch, au tube
`\\.\pipe\Eco.spacenotch.<utilisateur>` (créé par SpaceNotch, lot B), un événement `kind:"event"`, `name:"progress"`,
`args: {"op":"<pid>-<n>","state":"running|paused|done|error|cancelled","done":N,"total":N}` : `op` identifie l'opération
(copie, déplacement, corbeille, suppression d'une fenêtre), `done`/`total` sont les unités du Shell (en pratique des
octets ; pour un état final sans chiffres connus, les derniers vus, `done = total` pour `done`). Aucune réponse n'est attendue.
- Un seul fil d'émission, alimenté par une file bornée (64, un seul `running` en attente par opération) : ni le fil d'interface ni
  un fil d'opération ne fait d'entrée-sortie de tube. File pleine : on jette d'abord un `running`, **jamais un état final**.
- **Cible en panne** : après un `hello` sans réponse (`Delai`), une réponse d'un autre protocole ou un serveur non fiable, la cible
  est tenue pour indisponible **5 s** : rien n'est envoyé, aucune connexion n'est tentée (pas de réessai en boucle). Un état final
  qui attendait est réessayé **une fois** à l'échéance, puis abandonné.
- **Fenêtre fermée en pleine copie** : une fenêtre qu'on ferme (ou qui plante) pendant une opération n'émet **aucun état final**.
  SpaceNotch doit faire expirer les opérations sans nouvelles (par exemple après quelques secondes sans `running`).
  Fait au lot B : 30 s sans nouvelle, la carte passe à « Interrompue » ; au plus 4 opérations suivies, 40 événements par
  seconde et 8 créations par seconde au total, et un état non final arrivant après un état final (10 s) est ignoré.
  `op` est reçu comme chaîne ou comme entier.
- **Coalescence** : au plus 10 événements par seconde et par opération pour `running` ; un changement d'état (`paused`, reprise) et
  l'état final (`done`, `error` ou `cancelled`) sont toujours émis.
- **Absent = aucun coût** : sans tube `Eco.spacenotch.*`, aucun fil n'est lancé et aucun appel de tube n'est fait (présence
  vérifiée par `WaitNamedPipe` de 1 ms, qui n'ouvre rien, au plus une fois par seconde et seulement quand une opération émet).
- Avant le premier événement, l'émetteur fait un `hello` (gardé 60 s) pour s'assurer que le serveur est bien SpaceNotch et parle le
  protocole 1 ; il applique la règle côté client (exe du manifeste de SpaceNotch exigé). Sinon : rien n'est émis.
- L'opération de fichiers ne dépend jamais de l'émission : un échec est jeté, sans trace à l'écran. Le fil s'arrête après 30 s
  sans événement.

## Erreurs

**Erreurs** : `error: {"code","message"}`, la connexion reste utilisable sauf `trop_long`.

| `code` | Cas |
|---|---|
| `invalide` | JSON invalide, imbrication au-delà de 8 niveaux, clé en double dans un objet, type de champ faux (`reuse` compris), `id` de plus de 64 octets, `kind` autre que `call` |
| `champ_manquant` | `name`, `args`, `args.q` ou `args.path` absent |
| `hors_bornes` | `limit` hors de 1 à 30 ; `path` de 32 000 caractères ou plus |
| `inconnu` | `name` inconnu |
| `trop_vite` | `explorer.open` : plus de 5 ouvertures dans la dernière seconde (ou file pleine) |
| `interne` | le résident ne peut pas traiter la demande maintenant (fenêtre cachée absente) |
| `version` | `v` supérieur à 1 |
| `non_supporte` | `contenu:` dans `q` |
| `index` | index illisible |
| `index_efface` | « Supprimer l'index » a été demandé : le service ne le recrée pas de lui-même (le prochain usage d'une fenêtre de l'Explorateur le reconstruit, le service le reprend alors) |
| `trop_long` | ligne de plus de 4 096 octets (32 Ko pour `explorer.open`) : la réponse est envoyée, puis la connexion est fermée (au plus 2 s après, ou dès que le client la ferme) |

**Journal.** Le journal ne reçoit jamais le texte cherché ni un chemin de résultat : seulement les états (service démarré
ou arrêté, premier « loading » d'un chargement, libération, client inactif déconnecté) et, pour les seules requêtes de plus
de 100 ms, la durée et le nombre de résultats.
