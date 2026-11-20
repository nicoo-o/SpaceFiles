# E4 : liste blanche de nettoyage (proposition générique, à relire par l'auteur)

Statut : **proposée puis révisée le 19/11/2026, VALIDÉE par l'auteur le 19/11/2026** (liste du § 2, seuil de 7 jours, suppression directe des caches de shaders). Le lot E4a (mesure et affichage en lecture seule) est écrit ; le code de suppression
(E4b) vient ensuite, et n'écrira que des chemins issus de cette liste. Aucun modèle n'est ajouté ensuite sans son accord (`docs/PLAN-ACTION-ESPACE-TERMINAL.md` § 4).

SpaceFiles est distribué à d'autres utilisateurs : la liste décrit donc des **modèles de catégorie** valables sur tout
Windows 10 ou 11, jamais des chemins ou des noms de programmes propres à une machine. Les mesures de l'auteur
(19/11/2026, lecture seule, rien supprimé) ne sont que des **exemples**.

## 1. Forme d'une catégorie

Chaque catégorie est un modèle de quatre éléments, écrit dans le code (aucun nom de programme) :

1. **Racine** : un dossier lu par l'API Windows du profil (`SHGetKnownFolderPath`, `GetTempPath`), jamais une chaîne
   écrite en dur ni un simple `%VARIABLE%` non vérifié.
2. **Règle** : âge minimal, motif de nom de fichier, ou « tout le contenu ».
3. **Mode** : Corbeille, suppression directe ou définitif (Corbeille Windows).
4. **Texte de risque** affiché à l'utilisateur avant confirmation.

Une catégorie n'est **affichée que si** sa racine existe et dépasse une taille minimale. Sur un PC sans carte NVIDIA,
la ligne NVIDIA n'apparaît pas.

## 2. Modèles proposés (sans droits administrateur)

| # | Catégorie | Racine (sous le profil de l'utilisateur) | Règle | Mode | Présence | Exemple mesuré (PC de l'auteur) |
|---|---|---|---|---|---|---|
| 1 | Fichiers temporaires | `%LOCALAPPDATA%\Temp` (= `GetTempPath` s'il y pointe) | éléments dont le **fichier le plus récent** a plus de 7 jours | Corbeille | tout PC | 3 104 Mo, dont 1 545 Mo de plus de 7 jours |
| 2 | Cache de shaders DirectX | `%LOCALAPPDATA%\D3DSCache` | tout le contenu | Directe | Windows 10 / 11 | 26 Mo |
| 3 | Cache de shaders NVIDIA | `%LOCALAPPDATA%\NVIDIA\DXCache`, `...\GLCache` | tout le contenu | Directe | si carte NVIDIA | 26 558 Mo + 6 Mo |
| 4 | Cache de shaders AMD | `%LOCALAPPDATA%\AMD\DxCache`, `...\GLCache` | tout le contenu | Directe | si carte AMD | absent |
| 5 | Cache de shaders Intel | `%LOCALAPPDATA%\Intel\ShaderCache` | tout le contenu | Directe | si carte Intel | non mesuré |
| 6 | Rapports de plantage | `%LOCALAPPDATA%\CrashDumps` | tous les `.dmp`, **sauf les 2 plus récents** | Corbeille | si Windows y écrit | 346 Mo, 10 fichiers |
| 7 | Corbeille Windows | Corbeille de tous les lecteurs (`SHEmptyRecycleBin`) | tout | **Définitif** | tout PC | 44 Mo, 22 éléments |

Retirée de la proposition précédente : les vignettes (`thumbcache_*.db`) : 163 Mo seulement pour des vignettes à refaire,
fichiers verrouillés par l'Explorateur. Pas de nom de programme (LEDKeeper2, etc.) dans aucune règle.

## 3. Ce que chaque ligne coûte à l'utilisateur (texte affiché avant confirmation)

| # | Sûr à supprimer ? | Effet sur la performance |
|---|---|---|
| 1 | Oui ; un fichier verrouillé est ignoré. Réserve : un programme tiers qui rangerait des données durables dans `Temp`. | Aucun. |
| 2 à 5 | Oui, le pilote ou Windows recrée le cache. Jamais pendant qu'un jeu ou une application 3D tourne (fichiers verrouillés, ignorés). | **Temporaire** : saccades et chargements plus longs au premier lancement de chaque jeu, le temps de reconstruire. |
| 6 | Oui pour Windows. Efface la trace d'un plantage, pas sa cause. | Aucun. |
| 7 | **Non récupérable.** Jamais cochée par défaut ; contenu listé avant confirmation. | Aucun. |

Un cache de shaders très gros (plusieurs Go) est anormal : il est signalé à l'utilisateur comme tel.

## 4. Règles de sécurité

1. Seuls les modèles du § 2 sont touchés. Un chemin qui n'est pas **un descendant exact** d'une racine de catégorie
   (après résolution complète du chemin) est refusé ; test unitaire par catégorie.
2. Jamais : `C:\Windows`, `Program Files`, `Prefetch`, `System Volume Information`. Jonctions et liens symboliques :
   **non suivis**.
3. Une racine qui n'est pas sous le profil de l'utilisateur (par exemple `%TEMP%` redirigé vers la racine de `D:\`, ou un
   dossier partagé) **désactive** la catégorie : aucun chemin de repli.
4. **Rien n'est coché par défaut.** Confirmation avec la liste exacte, la taille et le mode (« Corbeille », « direct »,
   « définitif »).
5. Fichier verrouillé, refusé ou protégé par une stratégie d'entreprise : ignoré, compté « en cours d'usage », l'opération
   continue sans erreur bloquante.
6. Résultat journalisé (éléments, octets libérés, échecs) dans l'historique des opérations.
7. Les tests de suppression tournent **uniquement** sur un faux profil dans un dossier temporaire de test : faux profil
   avec et sans carte NVIDIA, `%TEMP%` redirigé hors profil, dossier vide, lien symbolique vers un dossier hors liste,
   fichier verrouillé. Aucune vraie donnée n'est touchée pendant le développement.

## 5. Hors E4 (volontairement)

- Caches de navigateurs, points de restauration, compression NTFS : E15.
- Caches d'outils de développement (cargo, npm, pip…), `target`, `node_modules`, WSL / Docker : E13.
- `C:\Windows\Temp`, Windows Update, hibernation, composants Windows : E10 (administrateur).
- Téléchargements, Bureau, Documents : jamais proposés.

## 6. Ce que l'auteur doit trancher

1. Valider, retirer ou modifier chaque ligne du § 2 (surtout 3 à 5 pour leur effet temporaire sur les jeux, 6 et 7).
2. Seuil d'âge de `%TEMP%` : 7 jours (proposé) ou plus.
3. Caches de shaders : suppression **directe** (proposé : une Corbeille de 26 Go n'a pas de sens) ou Corbeille.
4. Ordre des lots : E4a (modèles, détection, mesure, page NETTOYAGE en lecture seule), E4b (suppression avec confirmation),
   E4c (analyse automatique au lancement de la fenêtre) ; chacun se termine seul.

## 7. État (19/11/2026)

E4a (mesure) et E4b (suppression avec confirmation, trois modes, revérification juste avant chaque effacement, journal) sont faites ; E4c (analyse automatique au lancement) reste à faire. Les modes réellement appliqués : caches de shaders en **direct**, fichiers temporaires et rapports de plantage en **Corbeille**, Corbeille Windows en **définitif**. Détails et limites : `docs/PLAN-ACTION-ESPACE-TERMINAL.md` § E4.
