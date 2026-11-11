# SpaceFiles : options de signature du code et de mise à jour (document de décision)

Phase 8, lot D. Rédigé le 04/11/2026. **Rien n'est acheté, créé, installé ni appliqué** : ce document expose des options
sourcées et des recommandations que l'auteur accepte ou refuse. Les décisions proposées (D-13 et suivantes) ne sont **pas**
écrites dans le registre du masterplan.

Convention : « consulté le 04/11/2026 » vaut pour toutes les sources de la section 3. Les prix, seuils et politiques sont
marqués **[à revérifier]**. « Non établi » = non trouvé, contradictoire ou non lu ; aucun chiffre n'est donné de mémoire.
Hypothèse de travail (non vérifiée) : l'auteur réside en Europe (fuseau de la machine : Europe/Bruxelles), donc hors États-Unis
et Canada, ce qui compte pour l'éligibilité au service de signature de Microsoft. À confirmer par l'auteur.

## 1. Résumé en 10 lignes

1. **À décider** : (D-13) SpaceFiles sera-t-il distribué à des tiers un jour, ou reste-t-il un outil personnel ? Tout le reste en dépend.
2. Aujourd'hui l'exe n'est pas signé ; sur la machine de l'auteur cela ne pose problème que si Smart App Control est **activé** (mode « Activé ») : en mode « Évaluation » il ne bloquerait rien selon un résumé de recherche relayant Microsoft (S2 décrit seulement le mode : il évalue l'adéquation de l'appareil et se coupe seul pour certains usages ; l'absence de blocage n'est pas écrite dans les pages lues).
3. Pour un tiers : un exe non signé téléchargé donne l'écran SmartScreen « Windows a protégé votre ordinateur » (contournable) et, avec Smart App Control activé, un blocage sans contournement par application (S1, S2, S3).
4. Le certificat EV n'apporte plus de réputation immédiate auprès de SmartScreen (S3) : ne pas payer un EV pour cela.
5. Le service géré de Microsoft (Artifact Signing, ex-Trusted Signing) coûte 9,99 USD/mois **[à revérifier]** mais, pour un **particulier**, il est réservé aux États-Unis et au Canada (S4) ; en Europe il faudrait une **organisation** (entité légale, validation de 1 à 20 jours ouvrés).
6. Recommandation signature : **ne rien acheter maintenant** ; si publication, choisir entre un certificat OV classique (matériel ou HSM obligatoire, prix annuel à revérifier) et Artifact Signing via une entité légale. Le Store (MSIX) est le seul chemin sans certificat à acheter, mais il impose un empaquetage qui menace Win+E, la clé `Run` et les verbes de dossier (section 4.1).
7. Recommandation mise à jour : **manuelle (comme aujourd'hui) + le Hub affiche version et lien**, sans mise à jour automatique (cohérent avec D-09). Une vérification de version dans l'application n'est à envisager qu'après décision sur l'hébergement et ajoute le premier code réseau HTTP de l'exe.
8. Le dépôt n'a **pas de `LICENSE`**, pas de ressource de version (`VERSIONINFO`) ni de `build.rs`, et aucun remote git : une première publication demande ces trois choses (section 5).
9. **Coûte de l'argent ou demande un compte** : certificat OV (achat), Artifact Signing (compte Azure + abonnement + validation d'identité), Store (compte Partner Center, frais d'inscription à revérifier), hébergement GitHub (compte), winget (compte GitHub pour la demande de fusion).
10. Gratuit et décidable seul : numéro de version, ressource de version, `LICENSE`, script d'archive et somme de contrôle, préparation d'un manifeste winget (non soumis).

## 2. Faits sur le dépôt (lus, pas devinés)

| Fait | Source dans le dépôt |
|---|---|
| Exe non signé ; `Cargo.toml` version `0.1.0` ; profil release : `lto = true`, `codegen-units = 1`, `panic = "abort"`, `strip = true` | `Cargo.toml` |
| Pas de `build.rs`, pas de crate de ressources (`winres`, `embed-resource` : aucune occurrence dans `src/`) : **pas de `VERSIONINFO`** ni d'icône ou manifeste embarqués d'après le code ; l'exe lui-même n'a pas été inspecté | absence de `build.rs`, `Cargo.toml` |
| Pas de fichier `LICENSE` ; pas de champ `license` dans `Cargo.toml` | racine du dépôt |
| Pas de remote git, pas de push | `AGENTS.md` (« Dépôt git local ») |
| Mise à jour actuelle : relancer `installer.ps1` après `cargo build --release` ; il ferme le résident, copie l'exe et l'icône dans `%LOCALAPPDATA%\Programs\SpaceFiles`, écrit la valeur `Run\SpaceFilesWinE`, enregistre les protocoles `spacefiles://` et `nothingexplorer://`, crée le raccourci | `docs/installation.md`, `installer.ps1` |
| `installer.ps1` copie un exe compilé localement : il n'ajoute pas de marque du Web ; aucune occurrence de « Zone », « Unblock » ou de hachage d'exe dans le script | `installer.ps1` |
| Crochet clavier bas niveau `WH_KEYBOARD_LL` (Win+E uniquement), installé par le résident | `src/raccourci.rs` (l. 201), `CHANGELOG.md` |
| Clé `Run` écrite par l'application (`Software\Microsoft\Windows\CurrentVersion\Run`, valeur `SpaceFilesWinE`, commande `"<exe>" --resident`) ; verbes de dossier écrits par l'application (réglage « Explorateur par défaut ») | `src/defaut.rs` |
| Taille de l'exe : 3 848 704 octets (3,85 Mo) contre 2,8 Mo visé ; résident à 3,45 Mo privés (≤ 10 Mo visé) | `CHANGELOG.md`, `docs/prompts/2026-11-03-phase-8-lot-a-mesures-rapport.md` |
| Aucune pile HTTP dans `src/` (recherche de `winhttp`, `wininet`, `reqwest`, `URLDownload` : 0 résultat) ; le protocole Eco n'ouvre « pas de port réseau, pas d'HTTP local » ; SFTP passe par un processus externe (`src/sftp.rs`, `Command::new`) | `src/`, `docs/ecosystem/protocol.md` l. 27 |
| `AGENTS.md` ne contient **pas** de règle écrite « pas de réseau sans demande » ; l'esprit existe (rejet d'HTTP local, masterplan ligne 164) mais rien n'interdit un appel sortant. Toute vérification de version serait donc une **nouvelle décision**, à prendre explicitement | `AGENTS.md`, `PROJECT_REBOOT_MASTERPLAN.md` |
| Vérification de signature existante dans l'app : `WinVerifyTrust` sans réseau, pour lire le signataire d'un programme (fonction `outils.rs`) | `src/outils.rs` l. 388 |
| Le manifeste Eco d'une application contient déjà un champ `version` que le Hub lit ; le Hub « ne fait que lire les manifestes et écrire `shared.json` » | `docs/ecosystem/manifest.schema.json`, `docs/ecosystem/protocol.md` l. 21 |
| D-09 : « Updates : per-app now; Hub only lists » (statut : proposition) | `PROJECT_REBOOT_MASTERPLAN.md` l. 363 |
| Masterplan Part 10 : « SpaceNotch is unsigned; Smart App Control blocks it [S] » ; SpaceNotch est empaqueté avec identité (`Package.appxmanifest`), non signé, avec mise à jour automatique depuis GitHub « every 6 h with consent » (description de l'architecture visée, à ne pas prendre pour un fait vérifié ici) | `PROJECT_REBOOT_MASTERPLAN.md` l. 100, 347, 410 |
| Licences des dépendances : lues dans les `Cargo.toml` du registre local de `cargo` (sans réseau, sans outil installé) ; voir section 5 | `Cargo.lock`, `~/.cargo/registry/src` |

## 3. Sources (consultées le 04/11/2026)

| Id | Page | URL |
|---|---|---|
| S1 | Smart App Control, FAQ (support Microsoft) | https://support.microsoft.com/windows/smart-app-control-faq-285ea03d-fa88-4d56-882e-6698afdb7003 |
| S2 | Smart App Control, vue d'ensemble pour développeurs (Microsoft Learn, mis à jour 06/10/2026) | https://learn.microsoft.com/en-us/windows/apps/develop/smart-app-control/overview |
| S3 | Réputation SmartScreen pour les développeurs (Microsoft Learn, mis à jour 17/08/2026) | https://learn.microsoft.com/windows/apps/package-and-deploy/smartscreen-reputation |
| S4 | Artifact Signing, démarrage rapide : éligibilité, validation d'identité, SKU (Microsoft Learn, mis à jour 08/10/2026) | https://learn.microsoft.com/azure/artifact-signing/quickstart |
| S5 | Artifact Signing, présentation | https://learn.microsoft.com/azure/artifact-signing/overview |
| S6 | Artifact Signing, tarifs | https://azure.microsoft.com/pricing/details/artifact-signing/ |
| S7 | Signer pour Smart App Control (Microsoft Learn, 28/09/2026) | https://learn.microsoft.com/en-us/windows/apps/develop/smart-app-control/code-signing-for-smart-app-control |
| S8 | Application Control for Windows (Smart App Control, mode évaluation) | https://learn.microsoft.com/windows/security/application-security/application-control/app-control-for-business/appcontrol |
| S9 | Microsoft Store, prise en main (EXE/MSI, MSIX, signature, frais) | https://learn.microsoft.com/windows/apps/publish/get-started |
| S10 | Soumission d'une application EXE/MSI au Store | https://learn.microsoft.com/windows/apps/publish/publish-your-app/msi/create-app-submission |
| S11 | winget : soumettre un manifeste, validation | https://learn.microsoft.com/windows/package-manager/package/repository |
| S12 | winget : politiques du dépôt | https://learn.microsoft.com/windows/package-manager/package/windows-package-manager-policies |
| S13 | MSIX : comportement des applications de bureau empaquetées (registre, AppData) | https://learn.microsoft.com/windows/msix/desktop/desktop-to-uwp-behind-the-scenes |
| S14 | MSIX : mise à jour automatique avec App Installer | https://learn.microsoft.com/windows/msix/app-installer/auto-update-and-repair--overview |
| S15 | Velopack, documentation | https://docs.velopack.io/ |
| S16 | Velopack, crate Rust (résultat de recherche, licence MIT, `rust-version 1.75`) | https://docs.rs/crate/velopack/1.1.1/source/Cargo.toml |
| S17 | Tarifs et durées de certificats OV/EV : **pages de revendeurs seulement** (SSL Dragon, Sectigo via recherche, non lues en entier) | https://www.ssldragon.com/ssl-certificates/code-signing/ ; https://www.sectigo.com/code-signing |
| S18 | Fil communautaire : Artifact Signing refusé à un utilisateur en France (non officiel, assisté par IA) | https://learn.microsoft.com/en-us/answers/questions/5810735/cant-create-a-new-trusted-signing-individual-ident |
| S19 | Fils communautaires : faux positifs d'antivirus sur des exe Rust | https://users.rust-lang.org/t/why-my-windows-defender-think-my-rust-file-is-trojan/111832 |

Les pages S4, S5, S7, S9 à S14 sont officielles et lues en entier ou en grande partie. S6 n'affiche pas les montants dans la page
récupérée (seules la structure Basic/Premium et les quotas 5 000 / 100 000 signatures). Le montant 9,99 USD/mois vient de S3
(« Starts at $9.99/month ») et d'un résumé de recherche de la page de tarifs : **[à revérifier]**. S17 à S19 ne sont pas des
sources officielles ; leurs chiffres sont indicatifs.

## 4. Tableaux par question

### 4.1 Signature du code

**Ce qui se passe aujourd'hui avec un `spacefiles.exe` non signé**

| Mécanisme | Effet établi | Source / réserve |
|---|---|---|
| SmartScreen (réputation) | Pour un fichier non signé : « Windows protected your PC », l'utilisateur doit choisir « Run anyway » ; une stratégie d'entreprise peut interdire de continuer. La réputation d'un fichier non signé repart de zéro **à chaque nouvelle version** | S3 |
| Smart App Control, mode **Évaluation** | Ne bloquerait rien d'après un résumé de recherche relayant Microsoft (« won't block anything ») ; ni S1 ni S2 ne l'écrivent : **non confirmé par une page lue**. Il se coupe tout seul si Windows juge l'usage incompatible (développeurs) ; sur postes gérés par une entreprise il se coupe en 48 h sauf activation par l'utilisateur | S2, S8 |
| Smart App Control, mode **Activé** | Un exe non signé est bloqué sauf prédiction positive du service cloud ; un exe signé valide s'exécute même sans réputation. **Aucun contournement par application** (désactiver complètement ou signer). Les contrôles de signature s'appliquent à **tous** les exécutables, pas seulement ceux téléchargés | S1, S2, S3 |
| Smart App Control, disponibilité | « Recent Windows updates allow Smart App Control to be enabled without requiring a clean installation » (S1) ; d'autres sources disent encore installation propre requise : **contradictoire, non établi** pour la build 26300 de l'auteur | S1 ; sources anciennes non lues |
| Marque du Web (Mark of the Web) | Les pages lues parlent de fichiers « téléchargés » pour SmartScreen mais **ne décrivent pas le rôle exact de la marque du Web** : non établi. Le comportement de `installer.ps1` (copie locale, aucune marque ajoutée) est un fait du dépôt ; en déduire « SmartScreen ne s'applique pas » n'est **pas** établi par les sources lues |
| Antivirus tiers | Les exe Rust neufs sont régulièrement signalés à tort par des antivirus à apprentissage automatique (S19, fils communautaires) ; un crochet clavier global est un motif classique de logiciel espion, **mais aucune source lue ne relie `WH_KEYBOARD_LL` à un faux positif précis** : hypothèse prudente, non établie |

**Distinguer l'auteur d'un tiers.** Sur la machine de l'auteur : l'exe est compilé là, copié par le script ; si Smart App Control
est « Évaluation » ou « Désactivé » il ne se passe rien ; s'il est « Activé », le blocage s'applique **même sans marque du Web**
(S3 : « signature checks apply to all executable files »). Un tiers qui télécharge l'exe cumule SmartScreen et (s'il l'a activé) Smart App Control.
L'état réel de Smart App Control sur la machine de l'auteur n'a pas été lu par cet agent (Paramètres > Sécurité Windows > Contrôle des applications et du navigateur : à regarder par l'auteur).

**Options**

| Option | Coût | Démarches | SmartScreen | Smart App Control | Effet sur `installer.ps1` et `cargo build --release` | Dépendance à un tiers |
|---|---|---|---|---|---|---|
| (a) Rester non signé et documenter | 0 | Documenter dans `docs/installation.md` (étapes « Run anyway », état de Smart App Control, journal `CodeIntegrity` événements 3076 évaluation / 3077 application, source communautaire) | Avertissement à chaque version (S3) | Blocage si « Activé » (S1, S2) ; contournement = désactiver le composant (définitif tant qu'on ne réinstalle pas : « Once evaluation ends… » selon relais de recherche, non officiel) | Aucun | Aucune |
| (b) Certificat OV (ou EV) d'une autorité classique | OV : de l'ordre de 200 à 400 USD/an selon revendeurs **[à revérifier, non officiel]** ; EV plus cher, **sans avantage SmartScreen** (S3) | Achat, validation d'identité de l'organisation ou de l'individu ; clé sur matériel conforme (jeton USB, HSM ou KeyLocker/eSigner) depuis juin 2023 ; durée max 460 jours annoncée par plusieurs revendeurs depuis début 2026, un revendeur donne un autre calendrier **[à revérifier, contradictoire]** | Avertissement jusqu'à accumulation de réputation (« plusieurs semaines et des centaines d'installations propres », S3), puis réputation attachée au certificat si on garde la même identité | Passe si l'autorité est dans le Trusted Root Program (S2, S7) | Ajouter un pas `signtool sign … /tr <horodatage> /fd sha256` après la compilation ; le script copie alors un exe signé ; signer **avant** de calculer la somme de contrôle | Autorité, revendeur, matériel ; renouvellement annuel |
| (c) Artifact Signing (ex-Trusted Signing), service géré de Microsoft | 9,99 USD/mois (Basic, 5 000 signatures) ; Premium 100 000 signatures, prix non lu **[à revérifier]** ; facturation dès la création du compte, sans prorata (S6, S3) | Compte Azure + abonnement + ressource `Microsoft.CodeSigning` + validation d'identité dans le portail uniquement ; **particulier : États-Unis ou Canada seulement ; organisation : États-Unis, Canada, UE, Royaume-Uni, Australie, Nouvelle-Zélande, Japon, Corée du Sud, Singapour, Suisse, Norvège, Israël** (S4) ; validation organisation 1 à 20 jours ouvrés (S4) ; particulier : pièce d'identité par un partenaire (AU10TIX) et compte de facturation de type « Individual » (S4) | Même régime que (b) : réputation à accumuler, pas d'exemption (S3) | Voie « préférée » de Microsoft (S7) | Signature en ligne de commande via Azure CLI/PowerShell pour la gestion des ressources (S4) ; **la commande exacte de signature d'un exe (SignTool + bibliothèque) n'a pas été lue : non établi** ; la signature est « content-confidential » (le fichier ne quitte pas la machine, S5) ; horodatage externe pris en charge (S5) ; durée de validité des certificats : **non établie** (non trouvée dans S4/S5) | Microsoft (service en évolution : renommé, SKU Premium récent ; un fil communautaire de mars 2026 signale un refus pour un particulier en France, S18, non officiel) |
| (d) Certificat auto-signé local | 0 | `New-SelfSignedCertificate` + approbation locale | Même comportement que non signé (S3) | Smart App Control ne considère que les certificats de fournisseurs de confiance (S7) ; effet d'un certificat racine ajouté par l'utilisateur à son magasin : **non établi** (aucune page officielle trouvée) | Utile seulement pour tester la chaîne de signature (par exemple la lecture du signataire par `outils.rs`), pas pour contourner un blocage | Aucune |
| (e) Microsoft Store | Frais d'inscription : page S9 dit que le nouveau parcours « ne charge pas de frais d'inscription », mais cite aussi un paiement dans la même FAQ : **contradictoire, [à revérifier]** | Compte Partner Center (compte Microsoft, vérification d'identité), choix particulier ou société (un particulier doit publier hors activité commerciale, S9), réservation du nom, soumission, certification jusqu'à 3 jours ouvrés (S9) ; **MSIX** : le Store re-signe gratuitement, aucun certificat à acheter ; **EXE/MSI** : le Store **ne re-signe pas**, il faut déjà un certificat (S9) | Aucun avertissement pour une application installée par le Store (S3) | Les applications du Store passent (relais de recherche, S3 dit « never subject to SmartScreen » ; pour Smart App Control, non lu en page officielle) | **MSIX = refonte de l'installation** : voir tableau suivant | Microsoft (politiques du Store, certification, délais) |

**Si l'on empaquette en MSIX (options e et mise à jour par App Installer)** : effets lus dans S13.

| Aspect de SpaceFiles | Ce que dit la documentation | Conséquence pour ce dépôt |
|---|---|---|
| Écritures sous `HKCU` | Copiées à l'écriture vers un emplacement privé par application ; supprimées à la désinstallation | La clé `Run\SpaceFilesWinE` écrite par `defaut.rs` : **si Windows la lit au démarrage depuis cet emplacement privé n'est pas établi** ; le chemin prévu est une tâche de démarrage déclarée dans le manifeste (non lu dans cette recherche) |
| Verbes `Classes\{Folder,Directory,Drive}\shell\SpaceFiles` et protocoles | Même virtualisation de `HKCU` ; aucune page lue ne dit si l'Explorateur Windows voit ces clés | Risque que « Ouvrir dans SpaceFiles » et `spacefiles://` cessent de fonctionner ; protocoles à déclarer dans le manifeste (non lu) |
| Crochet Win+E | Application « full trust » (`mediumIL`) possible (S13) ; le crochet global `WH_KEYBOARD_LL` n'est pas traité par la page | **Non établi** ; à tester sur un paquet réel |
| Écritures `AppData` (`%APPDATA%\SpaceFiles`, `%LOCALAPPDATA%\SpaceFiles`) | Les nouveaux fichiers sont redirigés vers un emplacement privé par paquet (Windows 10 1903+), fusionné à la lecture ; supprimés à la désinstallation | **Les données utilisateur (index de 130 Mo, réglages) disparaîtraient à la désinstallation du paquet** ; le Hub et SpaceNotch lisant `%LOCALAPPDATA%\Eco` verraient-ils les fichiers : **non établi**, et c'est le cœur du protocole Eco |
| Installation | `C:\Program Files\WindowsApps\…`, fichiers en lecture seule | `installer.ps1` et `desinstaller.ps1` deviennent sans objet pour ce mode ; `tools/bench` et la détection du chemin d'installation à revoir |

**Effets de bord à relever**

- Crochet `WH_KEYBOARD_LL` et clé `Run` : motifs fréquents chez les logiciels malveillants ; une signature ne les masque pas mais ancre une identité de signataire (S3 : « ne pas signer d'applications potentiellement indésirables », sinon la réputation du certificat devient **négative**). En cas de faux positif : soumettre le fichier à Microsoft Defender (S11 cite le portail de soumission), et à chaque éditeur d'antivirus séparément.
- Horodatage : à la signature, utiliser un horodatage externe de confiance pour que la signature reste valide après l'expiration du certificat (Artifact Signing le permet, S5 ; la pratique est courante pour les certificats classiques mais **non lue** ici).
- Signer l'exe de 3,85 Mo : la signature s'ajoute au fichier ; l'augmentation de taille n'a **pas été mesurée** (non établi). `strip = true` et LTO ne gênent pas : signer **après** `cargo build --release`, jamais avant ; toute modification de l'exe après signature casse la signature (S3 : « Do not modify signed files »).
- Chaque version signée avec la **même identité** conserve la réputation de la précédente ; un changement d'identité la perd (S3).

**Contexte SpaceNotch (autre dépôt, aucune conclusion).** Si SpaceNotch est bloqué par Smart App Control et pas SpaceFiles (non signé aussi), l'utilisateur qui ne lance que SpaceFiles n'est pas gêné par le blocage de SpaceNotch **sur le plan de la signature** ; à l'inverse, un utilisateur de SpaceNotch seul n'est pas aidé par la signature de SpaceFiles : la réputation d'un signataire est attachée à une identité de certificat, donc signer les deux avec la même identité mutualise la réputation (S3 : « consistent signing identity »), alors que le blocage Smart App Control, lui, se juge **fichier par fichier** (S2). Que SpaceNotch soit réellement bloqué est une affirmation du masterplan marquée `[S]` : non revérifiée ici. Une application empaquetée MSIX (cas décrit pour SpaceNotch) suit les règles du Store ou de la signature du paquet (S9), pas celles d'un exe isolé.

**Recommandation 1 (signature)** : *D-14 : ne rien acheter tant que D-13 n'est pas tranché ; garder (a) et documenter.* Si D-13 = publier :
(c) avec une entité légale si l'auteur en a une ou en crée une pour d'autres raisons ; sinon (b) OV. EV : écarté (S3). MSIX/Store : écarté
tant que les effets sur `HKCU`, `AppData`, protocoles et Win+E ne sont pas testés sur un paquet réel.
*Ce qu'elle suppose* : l'auteur est en France (particulier non éligible à (c), S4) et publie à une audience restreinte ;
l'exe garde son crochet Win+E. *Ce qui la ferait changer* : Microsoft ouvre (c) aux particuliers français (à revérifier à la
décision) ; un test MSIX montre que `Run`, verbes et `AppData` fonctionnent ; l'auteur ne veut aucun paiement récurrent (alors
(a) seul, avec avertissement documenté).

### 4.2 Stratégie de mise à jour

Aujourd'hui : relancer `installer.ps1` (ferme le résident, copie, relance). Exe sans pile HTTP (voir section 2) ; pas de remote git.

| Option | Prérequis | Signature | Taille exe / mémoire résident | Réseau et vie privée | Si le résident tient l'exe | Retour arrière |
|---|---|---|---|---|---|---|
| (a) Manuelle (état actuel) | Aucun ; l'auteur compile | Sans objet | Aucune | Aucun réseau | `installer.ps1` ferme le résident puis copie ; une instance qui tourne bloque la liaison (`LNK1104`, `AGENTS.md`) | Recompiler un ancien commit ; pas de copie de l'ancien exe gardée par le script (non lu : à vérifier) |
| (b) winget | Compte GitHub, dépôt de manifestes `microsoft/winget-pkgs` (demande de fusion), **URL de l'exe stable, HTTPS, directe chez l'éditeur, sans redirection**, somme SHA-256 exacte, installeur silencieux, installation et désinstallation propres sans droits administrateur, pas de faux positif antivirus (S11, S12) ; la signature n'est **pas** listée comme exigence dans S11/S12 ; le comportement face à un installeur non signé : **non établi** ; la prise en charge d'un exe « portable » seul sans installeur : **non établie** dans les pages lues | Pas exigée dans les pages lues ; recommandée par S3 | Aucune dans l'exe | `winget upgrade` fait la vérification ; aucun réseau dans l'application ; l'URL d'hébergement voit les téléchargements | winget ne fait pas fermer le résident : **non établi** ; le programme d'installation devrait le gérer (c'est le rôle de `installer.ps1`, aujourd'hui un script de dépôt et non un installeur silencieux autonome) | Version précédente réinstallable par son numéro (comportement général de winget, non lu) |
| (c) Vérification de version dans l'application (JSON, avertissement seulement) | Un hébergement HTTPS statique (pas de remote git : où ? voir D-16) ; un client HTTP : **première pile HTTP de l'exe** (WinHTTP par la crate `windows` : taille **non mesurée**, à mesurer) | Facultative (le fichier ne contient qu'un numéro et un lien) ; signer le JSON ou fixer l'origine évite l'usurpation (non étudié) | Taille : non mesurée ; mémoire : faire la requête dans la fenêtre, pas dans le résident, à la demande | **Nouvel appel sortant ; vie privée : l'hébergeur voit l'IP et l'heure** ; à rendre opt-in (réglage), jamais au démarrage sans accord ; décision à écrire (aucune règle actuelle ne l'interdit ni ne l'autorise) | Aucun téléchargement : sans effet | Sans objet |
| (d) Mise à jour automatique dans l'application | (c) plus téléchargement, vérification de signature et de somme, remplacement | **Indispensable** pour une mise à jour automatique sûre (sinon exécution de code téléchargé) | Code de remplacement + HTTP : taille non mesurée, risque pour le budget d'exe déjà dépassé | Téléchargements en arrière-plan : consentement explicite requis | Un exe en cours d'exécution ne peut pas s'écraser lui-même ; il faut un processus auxiliaire ou renommer l'exe en cours (**général, non sourcé ici**) ; reproduire ce que fait `installer.ps1` (fermer le résident, copier, relancer) | Garder l'exe précédent à côté ; non existant aujourd'hui |
| (e) MSIX / App Installer | Empaquetage, **signature obligatoire du paquet** pour une distribution hors Store (S9), hébergement HTTPS d'un fichier `.appinstaller`, Windows 10 2004+ (S14) ; réglages `HoursBetweenUpdateChecks`, `UpdateBlocksActivation`, `ShowPrompt` (S14) | Obligatoire | Hors exe ; l'empaquetage ne change pas l'exe | Windows fait la vérification (réseau hors application) | Windows gère le remplacement du paquet ; comportement avec un résident qui tient le fichier : **non établi** | Pas de retour arrière documenté dans S14 : **non établi** ; effets sur `HKCU`/`AppData`/Win+E : tableau 4.1 |
| (f) Outils tiers | — | — | — | — | — | — |

**(f) une ligne par outil** (aucune étude détaillée) :

- **Velopack** : installation et mise à jour pour applications de bureau, SDK Rust (crate `velopack`, licence MIT, `rust-version 1.75` selon la version 1.1.1 lue ; version actuelle non confirmée), hébergement sur fichiers statiques ou GitHub Releases ; l'outil `vpk` est un outil en ligne de commande .NET (une source plus ancienne indique le SDK .NET 8, à revérifier) ; l'application doit intégrer le SDK, donc ajoute une dépendance Rust et du réseau ; empreinte mémoire, taille, comportement avec un résident, signature : **non établis** (S15, S16). C'est le seul outil tiers lu.
- **Squirrel** : non consulté dans ce lot : non établi.
- **WiX / MSI, NSIS, Inno Setup** (installeurs classiques) : non consultés : non établi.

**Hub et D-09 (« Per-app now; Hub only lists »).**

| Option | Ce que D-09 implique | Le Hub peut-il afficher version et lien sans mettre à jour ? |
|---|---|---|
| (a) manuelle | Compatible : chaque application se met à jour seule | Oui : le manifeste Eco contient déjà `version` (`manifest.schema.json`) ; le lien vers la page de publication serait un **champ optionnel à ajouter** (le schéma accepte les champs inconnus : `additionalProperties: true`, mais le protocole le dit « à décider ») |
| (b) winget | Compatible (winget est externe aux applications) | Oui : version via manifeste ; le Hub peut montrer la commande `winget upgrade` sans l'exécuter |
| (c) vérification | Compatible si chaque application fait sa propre vérification ; le Hub n'ajoute pas de réseau | Oui pour afficher « version installée » et « version disponible » **si** l'application écrit la seconde dans son manifeste ; non établi qu'il faille le faire ; sinon le Hub afficherait seulement la version installée |
| (d) automatique | Contraire à l'esprit de D-09 si le Hub orchestre ; compatible si chaque application se met à jour seule | Oui en lecture seule |
| (e) MSIX | Chaque paquet se met à jour par Windows ; le Hub ne fait que lister | Oui |

**Recommandation 2 (mise à jour)** : *D-15 : option (a) + le Hub affiche version et lien (sans mise à jour), conforme à D-09 ; reporter (c) à après D-13 et D-16.*
*Ce qu'elle suppose* : peu d'utilisateurs, auteur seul à compiler ; exe sans HTTP. *Ce qui la ferait changer* : plus de quelques dizaines d'utilisateurs
(alors (b) puis (c) opt-in) ; Store retenu en D-14 (alors (e)). (d) est déconseillée : elle exige signature, HTTP, mécanisme de
remplacement, et dépasse un budget d'exe déjà dépassé.

## 5. Chaîne de publication minimale (si l'auteur publie un jour)

**Numéro de version.** `Cargo.toml` dit `0.1.0` ; le changelog propose `0.2.0`, non appliqué. Le journal écrit déjà `version 0.1.0` (protocole, exemple).
La version vit dans `Cargo.toml` : l'exposer au Hub (champ `version` du manifeste Eco, déjà prévu : « celle du binaire »).

**Ressource de version dans l'exe.** Absente d'après le code (pas de `build.rs`, pas de crate de ressources). Ajouter un `build.rs` et une
ressource (propriétés du fichier : éditeur, description, version) demande soit une dépendance de compilation (à justifier dans `PLAN.md`, règle
du dépôt), soit un fichier `.rc` compilé par l'outil de ressources de l'environnement : **choix non étudié** (hors périmètre de ce lot, pas de `src/` ni de `Cargo.toml` touchés).

**Nom d'archive et somme de contrôle.** Proposition non appliquée : `SpaceFiles-<version>-win-x64.zip` contenant `spacefiles.exe`, l'icône, `installer.ps1`, `desinstaller.ps1`,
`LICENSE`, `CHANGELOG.md` ; somme `SHA-256` de l'archive dans un fichier `.sha256` ; pour winget, la somme de l'**exe** ou de l'archive selon le type d'installeur retenu (non établi pour un exe portable).

**Hébergement.** Aucun remote : la publication demande un compte chez un hébergeur de binaires (GitHub Releases est cité par S15 comme un hôte possible et correspond à la
description du masterplan pour SpaceNotch, ligne 100 ; autre choix non étudié). winget exige l'URL directe de l'éditeur (S11, S12).

**Licence.** Pas de `LICENSE` dans le dépôt. Licences des dépendances, lues dans les `Cargo.toml` du registre local (sans outil de licences, sans réseau ; **pas une analyse juridique**) :

| Crate (version) | Licence déclarée |
|---|---|
| `regex` 1.13.1, `regex-automata` 0.4.18, `regex-syntax` 0.8.11 | MIT OR Apache-2.0 |
| `aho-corasick` 1.1.5, `memchr` 2.8.3 | Unlicense OR MIT |
| `windows` 0.58.0, `windows-core` 0.58.0, `windows-implement` 0.58.0, `windows-interface` 0.58.0, `windows-result` 0.2.0, `windows-strings` 0.1.0, `windows-targets` 0.52.6 | MIT OR Apache-2.0 |
| `proc-macro2` 1.0.107, `quote` 1.0.47, `syn` 2.0.119 (outils de compilation) | MIT OR Apache-2.0 |
| `unicode-ident` 1.0.27 | (MIT OR Apache-2.0) AND Unicode-3.0 |
| `windows_x86_64_gnu` 0.52.6 | MIT OR Apache-2.0 |
| `windows_x86_64_msvc`, `windows_i686_*`, `windows_aarch64_*`, `windows_x86_64_gnullvm` | **non listés** (sources non présentes dans le registre local de cette machine pour la cible MSVC) |

Polices : `assets/fonts` contient `JetBrainsMono-Regular.ttf` et `OFL.txt` (SIL OFL 1.1). Le texte de l'interface utilise Doto (`AGENTS.md`) ; **la présence et la licence du fichier de police Doto ne sont pas établies** dans `assets/fonts`. Obligations de redistribution des licences (mentions MIT/Apache, OFL) : **non analysées**. Ce que couvre `LICENSE` pour le code de l'auteur est une décision de l'auteur (D-17), pas de l'agent.

**Ordre de travail proposé (5 à 8 étapes)**

| # | Étape | Décidable seul ? | Demande un compte ou un achat de l'auteur |
|---|---|---|---|
| 1 | Trancher D-13 (publier ou non) et D-17 (licence du code) | Oui | Non |
| 2 | Choisir la version (0.1.0 ou 0.2.0), la ressource de version et l'icône embarquée ; justifier toute dépendance de compilation dans `PLAN.md` | Oui (code à faire par une session d'implémentation) | Non |
| 3 | Écrire `LICENSE`, une liste de mentions de tiers, et vérifier la présence/licence de la police Doto | Oui pour l'écriture ; **choix de licence : auteur** | Non |
| 4 | Script d'archive reproductible (zip, SHA-256) dans `tools/`, essai sur faux profil comme `essai-installation.ps1` | Oui | Non |
| 5 | Décider de la signature (D-14) ; si OV : acheter le certificat et le matériel ; si Artifact Signing : créer le compte Azure, valider l'identité (ou l'entité) ; ajouter le pas de signature avant la somme de contrôle | Décision : auteur ; configuration : après achat | **Oui** (achat ou abonnement, pièce d'identité) |
| 6 | Choisir l'hébergement (D-16) et publier l'archive + `.sha256` | Choix : auteur | **Oui** (compte chez l'hébergeur) |
| 7 | (Facultatif) Manifeste winget préparé en local, `winget validate`, test en bac à sable ; soumission | Préparation : oui ; **soumission : non** | **Oui** (compte GitHub pour la demande de fusion) |
| 8 | Test sur une machine propre avec Smart App Control « Activé » ; soumettre un faux positif éventuel à Microsoft Defender ; noter le résultat dans les critères de la première version | Non (machine propre : auteur) | Non |

## 6. À revérifier avant décision

- **Prix** : Artifact Signing Basic 9,99 USD/mois, Premium inconnu, dépassement par signature inconnu (S3, S6) ; certificats OV/EV (S17, revendeurs, 200 à 685 USD/an selon fournisseurs) ; frais d'inscription Partner Center (S9 contradictoire).
- **Éligibilité** Artifact Signing pour un particulier hors États-Unis/Canada, et pour une micro-entreprise : S4 dit « organisation » avec « entité légale » ; si une entreprise individuelle française compte comme organisation : **non établi**.
- Durée de validité des certificats d'Artifact Signing, commande SignTool exacte, adresse du service d'horodatage : non établis.
- Calendrier exact de la durée maximale (460 jours) des certificats de signature de code : plusieurs dates données par les revendeurs.
- Comportement de Smart App Control : installation propre exigée ou non (S1 contre sources anciennes), autorisation d'un certificat racine local, état réel sur la machine de l'auteur.
- Rôle de la marque du Web dans SmartScreen et Smart App Control.
- winget : acceptation d'un exe non signé, d'un exe « portable » sans installeur, arrêt d'un résident pendant la mise à jour.
- MSIX : sort de la clé `Run`, des verbes et protocoles `HKCU`, du crochet Win+E, et des fichiers `AppData` partagés avec le Hub et SpaceNotch.
- Velopack : version actuelle, empreinte, signature, besoin du SDK .NET 8.
- Faux positifs d'antivirus : aucun exe signé ni non signé de SpaceFiles n'a été soumis à un scanner (aucun outil lancé ni fichier envoyé par ce lot).

## 7. Décisions proposées à l'auteur (à la suite de D-12 ; non écrites au masterplan)

| N° | Décision | Options | Recommandation | Suppose | Changerait si |
|---|---|---|---|---|---|
| D-13 | SpaceFiles est-il publié à des tiers ? | Non (outil personnel) / cercle restreint / public | À trancher par l'auteur ; sans publication, D-14 à D-17 tombent | — | — |
| D-14 | Signature du code | (a) non signé documenté / (b) OV / (c) Artifact Signing via entité / (e) Store MSIX | (a) tant que D-13 n'est pas « publier » ; ensuite (c) si entité, sinon (b) | Auteur en France ; Microsoft inchangé | Éligibilité des particuliers français ; test MSIX favorable |
| D-15 | Mise à jour | (a) manuelle + Hub liste / (b) winget / (c) vérification opt-in / (d) automatique / (e) MSIX | (a) + Hub liste (D-09) | Peu d'utilisateurs ; pas d'HTTP dans l'exe | Audience plus large ; Store retenu |
| D-16 | Hébergement des binaires et du fichier de version | Hébergeur de dépôts avec publications / autre hébergement statique / aucun | À choisir par l'auteur ; non étudié ici, sauf mention de GitHub Releases par S15 | Compte chez l'hébergeur | — |
| D-17 | Licence du code de SpaceFiles et mentions de tiers | À choisir (propriétaire, permissive, copyleft…) ; conséquences des dépendances non analysées | Décision de l'auteur ; aucun conseil juridique | — | Avis juridique |
| D-18 | Réseau sortant dans l'exe (nécessaire à (c), (d)) | Interdit / opt-in à la demande / permis | Si (c) un jour : opt-in, requête faite par la fenêtre, jamais par le résident, ni au démarrage ; à écrire dans `AGENTS.md` | D-15 passe à (c) | D-15 reste (a) : sans objet |
