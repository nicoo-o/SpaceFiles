<p align="right"><a href="README.md">English</a> · <b>Français</b></p>

<p align="center">
  <img src="docs/readme/titre.svg" alt="SpaceFiles, dessiné en points" width="854">
</p>

<p align="center">
  <b>Un explorateur de fichiers pour Windows 11, dessiné point par point.</b><br>
  Rust, Win32 natif, Direct2D. Pas de framework d'interface, pas de télémétrie. Le seul WebView est celui, facultatif,
  du coup d'œil sur les pages HTML (WebView2, scripts et réseau coupés).
</p>

<p align="center">
  <code>exe de 3,81 Mo</code> · <code>résident de 16 Mo</code> · <code>640 tests</code> · <code>1 WebView facultatif</code>
</p>

---

> **ÉTAT ▸ EN DÉVELOPPEMENT.** Aucune version publiée, aucun binaire à télécharger. Tout ce qui suit existe dans
> `main` aujourd'hui ; ce qui manque est dans [LIMITES CONNUES ▸](#limites-connues-). Un chiffre sur cette page est un
> chiffre mesuré.

## OUVRIR ▸

SpaceFiles, c'est une fenêtre noire sans bordure, une police en matrice de points (Doto) et des menus qui crient
poliment en majuscules. C'est l'explorateur de fichiers de la [suite Space](https://github.com/nicoo-o) : de petites
apps Windows natives et privées, qui travaillent ensemble.

Il s'appelait *Nothing Explorer* jusqu'en novembre 2026. L'ancien nom de protocole répond encore, comme un chat.

## AFFICHER ▸

| | |
|---|---|
| **Onglets** | <kbd>Ctrl</kbd>+<kbd>T</kbd> / <kbd>W</kbd> / <kbd>Tab</kbd>, session sauvée 2 s après chaque changement et rouverte après un plantage |
| **Double panneau** | <kbd>F6</kbd>, comparer et synchroniser avec <kbd>Ctrl</kbd>+<kbd>Maj</kbd>+<kbd>F6</kbd> |
| **Trois vues** | Liste, grille (<kbd>Ctrl</kbd>+molette), colonnes Miller |
| **Coup d'œil** | <kbd>Espace</kbd> sur un fichier (sur un dossier, il calcule la taille à la place), et un volet d'aperçu |
| **Thèmes** | Sombre, clair, auto ou système ; le mouvement réduit coupe toutes les animations |
| **Pixel** | Deux barres dans la barre d'état. Elles changent de couleur selon l'heure et ferment les yeux la nuit. Ce n'est pas une mascotte : c'est un voyant |

## TROUVER ▸

- **Taper pour filtrer**, <kbd>Ctrl</kbd>+<kbd>F</kbd> pour chercher sous le dossier courant.
- **Index maison** (<kbd>Ctrl</kbd>+<kbd>Maj</kbd>+<kbd>F</kbd>) : environ 4 min 30 et 110 Mo pour 2,5 millions d'éléments, parcourus à
  priorité basse. Filtres : `ext:` `taille>` `modif:` `dossier:` `dans:` `contenu:`.
- **Palette de commandes** : <kbd>Ctrl</kbd>+<kbd>K</kbd>. Chemin éditable : <kbd>Ctrl</kbd>+<kbd>L</kbd>.

## AGIR ▸

- Copier, déplacer et jeter passent par l'`IFileOperation` de Windows, donc **l'annulation marche** :
  <kbd>Ctrl</kbd>+<kbd>Z</kbd> / <kbd>Ctrl</kbd>+<kbd>Y</kbd>, sur 50 étapes.
- File d'attente avec pause et reprise, conflits côte à côte, une jauge de 16 barres qui finit par
  `ANNULER · CTRL Z`.
- Renommage par lot avec expressions régulières, balises (<kbd>Ctrl</kbd>+<kbd>1</kbd>/<kbd>2</kbd>/<kbd>3</kbd>), épingles, SHA-256,
  archives.
- Les téléchargements du navigateur sont suivis : les fichiers partiels `.crdownload` et `.part` ont leur pastille,
  les fichiers finis une marque `NOUVEAU`.
- <kbd>Win</kbd>+<kbd>E</kbd> ouvre SpaceFiles quand le petit processus résident tourne.

## MESURER ▸

Pas d'adjectifs. Des chiffres, avec leurs limites.

| Quoi | Mesuré | Remarque |
|---|---|---|
| Exécutable | 3,81 Mo (3 810 304 octets) | build release, LTO, `panic = abort`, 14 novembre 2026 |
| Processus résident | 16 Mo de working set | la part qui reste en mémoire pour <kbd>Win</kbd>+<kbd>E</kbd> |
| Dossier de 200 000 entrées | listé en 0,9 s, 195 Mo | une seule mesure, pas une moyenne |
| Index de 2,5 millions d'éléments | ~4 min 30, 110 Mo | parcours à priorité basse |
| Tests | 640 réussis, 0 échec, 7 ignorés | `cargo test`, 14 novembre 2026 |
| Dépendances directes | 4 crates (30 au total) | `windows`, `windows-core`, `regex`, `webview2-com` |

## BRANCHER ▸

SpaceFiles parle aux autres apps Space par des tubes nommés réservés à l'utilisateur courant
(`Eco.<app>.<utilisateur>`, lignes JSON), un manifeste et un fichier de réglages partagé. Aucun port en écoute, aucun
compte (la seule connexion sortante est un test d'accessibilité sur le port 445 d'un stockage réseau configuré).

- [**SpaceHub**](https://github.com/nicoo-o/SpaceHub) : le point de rencontre de la suite. Il liste les apps, leurs
  versions et leur santé, et porte les réglages partagés de thème et de mouvement que SpaceFiles suit.
- [**SpaceNotch**](https://github.com/nicoo-o/SpaceNotch) : la Dynamic Island pour Windows 11. Elle peut demander une
  recherche de fichiers à SpaceFiles, qui lui envoie l'avancement de ses opérations. <kbd>Alt</kbd>+<kbd>Espace</kbd>
  appartient à SpaceNotch ; c'est pour cela que la palette, ici, est sur <kbd>Ctrl</kbd>+<kbd>K</kbd>.
- Le contrat est écrit dans [`docs/ecosystem/protocol.md`](docs/ecosystem/protocol.md). Le Hub n'est jamais requis,
  et aucune app n'en lance une autre pour fonctionner.

## CONSTRUIRE ▸

Windows 11 et une chaîne Rust. Rien d'autre à installer.

```powershell
cargo build --release      # target\release\spacefiles.exe
cargo test                 # une demi-minute
./installer.ps1            # copie l'exe dans %LOCALAPPDATA%\Programs\SpaceFiles, raccourci et icône
```

Lire [`docs/installation.md`](docs/installation.md) avant de lancer l'installeur : on y trouve ce qui est écrit et
comment `desinstaller.ps1` le retire (les données sont gardées, sauf demande contraire).

## LIMITES CONNUES ▸

- **Pas de version publiée, pas de binaire signé, pas de mise à jour automatique.** À construire soi-même, ou attendre.
  Un exe téléchargé déclenche SmartScreen : comparer son SHA-256 (`tools/install/empreinte.ps1`). WebView2 est facultatif (aperçu HTML seulement).
- **En français seulement** : interface, commentaires et documentation.
- **Pas encore de capture sur cette page.** Les vraies montrent les dossiers de l'auteur ; des captures propres sont
  sur la liste.
- SFTP, WebDAV, sauvegarde NAS et MTP sont gelés derrière un réglage `experimental`, désactivé pour un profil neuf.
- Non couvert par les vérifications automatiques : tout ce qui demande une vraie souris (survol, menu clic droit,
  lasso), un second écran d'un autre DPI, le plein écran.
- La liste complète, sans adoucissement, est à la fin de [`CHANGELOG.md`](CHANGELOG.md).

## CE QUE SPACEFILES NE FERA PAS ▸

- **Aucun compte ni service distant** : pas de Drive, Dropbox ou iCloud, aucun fichier envoyé à un modèle. Le réseau
  reste fermé.
- **Aucune télémétrie, aucune recommandation personnalisée** : le volet « recommandés » de l'Explorateur Windows n'est
  pas un objectif.
- **Aucune mise à jour automatique en tâche de fond** : le binaire se remplace à la main, le jour où une version
  existera.
- **Aucun moteur d'archive maison** : `tar.exe` couvre zip, tar et 7z dans les deux sens ; la compression ne se
  réimplémente pas.
- **Aucune place de marché d'extensions, aucun compagnon mobile** : un seul auteur, un seul binaire.

Ces refus sont argumentés dans les notes internes du 13/11/2026.

## À PROPOS ▸

Écrit par une seule personne, avec beaucoup d'aide d'agents de code ; [`AGENTS.md`](AGENTS.md) est le règlement
qu'ils suivent, et c'est aussi une bonne visite du code pour un humain. Le titre en points ci-dessus est généré par
[`tools/readme/titre.mjs`](tools/readme/titre.mjs).

[MIT](LICENSE). Les polices de `assets/fonts` gardent leur propre licence.

<p align="center"><sub>6 ÉLÉMENTS · 1 WEBVIEW FACULTATIF · LU EN UN DÉFILEMENT</sub></p>
