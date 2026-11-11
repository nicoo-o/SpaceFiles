# Installer, mettre à jour, désinstaller SpaceFiles

Tout se passe dans le profil de l'utilisateur (`HKEY_CURRENT_USER`, `%LOCALAPPDATA%`, `%APPDATA%`) : aucun droit
d'administrateur. Scripts PowerShell 5.1, à lancer depuis la racine du dépôt.

## Installer

```powershell
cargo build --release
.\installer.ps1
```

L'installeur ferme le résident en cours, copie `spacefiles.exe` et l'icône dans `%LOCALAPPDATA%\Programs\SpaceFiles`,
écrit la valeur `SpaceFilesWinE` de la clé `Run` (lancement avec Windows, `--resident`), enregistre les protocoles
`spacefiles://` et `nothingexplorer://`, crée le raccourci `SpaceFiles.lnk` du menu Démarrer, réoriente l'épingle de la
barre des tâches si elle existe, puis relance le résident. Les verbes « Ouvrir dans SpaceFiles » des dossiers ne sont
écrits que par l'application (réglage « Explorateur par défaut »), jamais par l'installeur.

**Mise à jour = relancer l'installeur** après chaque `cargo build --release` : même commande, même résultat. Une
installation d'avant le renommage (Nothing Explorer) est reprise sans rien perdre (verbe, valeur de sauvegarde, valeur
`Run`, raccourci) ; l'ancien dossier `Programs\NothingExplorer` n'est pas supprimé.

## Désinstaller

```powershell
.\desinstaller.ps1 -Simuler            # liste ce qui serait retiré, ne modifie rien
.\desinstaller.ps1                     # liste, demande « O/N », puis retire
.\desinstaller.ps1 -Oui                # retire sans demander
.\desinstaller.ps1 -Oui -SupprimerDonnees
```

Le désinstalleur retire **exactement** ce que l'installeur et l'application ont écrit, et rien d'autre :

| Élément | Règle |
|---|---|
| Processus `spacefiles.exe` | seulement ceux dont le **chemin** est celui du dossier d'installation (ou de l'ancien) ; fermeture douce (`WM_CLOSE`), arrêt par numéro après 5 s ; jamais par nom, jamais un exe d'ailleurs |
| Valeur `Run\SpaceFilesWinE` | retirée (nom exact) |
| Valeur `Run\NothingExplorerWinE` | retirée seulement si elle lance `nothing-explorer.exe` |
| Verbes `Classes\{Folder,Directory,Drive}\shell\SpaceFiles` | retirés (nom exact) |
| Verbe `...\shell\NothingExplorer` | retiré seulement si sa commande lance `nothing-explorer.exe` |
| Valeur par défaut de `...\shell` | **restaurée** depuis `SpaceFilesAncienne` (ou `NothingExplorerAncienne`) ; sans sauvegarde, retirée ; seulement si elle vaut `SpaceFiles` / `NothingExplorer` |
| Valeurs `SpaceFilesAncienne`, `NothingExplorerAncienne` | retirées après la restauration |
| Protocoles `Classes\spacefiles`, `Classes\nothingexplorer` | retirés seulement si leur commande lance `spacefiles.exe` ou `nothing-explorer.exe` |
| Ancien essai Win+E (`Classes\CLSID\{52205fd8-...}`) | retiré seulement si sa commande contient `nothing-explorer` |
| `SpaceFiles.lnk`, `Nothing Explorer.lnk` (menu Démarrer) | retirés s'ils visent notre exe |
| Épingle de barre des tâches vers notre exe | retirée |
| Fichiers de `Programs\SpaceFiles` (exe, `spacefiles.old*.exe`, icônes) | retirés ; le dossier part s'il est vide ensuite, sinon il reste avec ce qui n'est pas à nous |

Jamais supprimées : les clés parentes (`shell`, `Classes`, `Run`) et tout ce qui appartient à un autre programme. Un
élément « laissé » parce qu'il n'est pas à nous est signalé (`LAISSÉ`).

### Ce qui est conservé

Les données de l'utilisateur restent : `%APPDATA%\SpaceFiles`, `%LOCALAPPDATA%\SpaceFiles` (réglages, index, journal),
les anciens dossiers `NothingExplorer` des mêmes endroits, et l'ancien dossier `Programs\NothingExplorer` (retiré
seulement s'il est vide). `-SupprimerDonnees` les retire, après les avoir listés avec leur taille.

### Sortie

`RETIRÉ` / `ABSENT` / `ÉCHEC` par ligne (chaque chemin est relu après le retrait). Code 0 : tout est retiré ou était
déjà absent (relancé sans rien à retirer, il écrit « Rien à retirer. »). Code 1 : une suppression a échoué (exe tenu
par un processus qui n'a pas pu être arrêté, par exemple). Code 2 : confirmation refusée ou impossible (session sans
terminal : utiliser `-Oui`).

## Essayer sans toucher à sa machine

Les deux scripts acceptent des racines factices (`-RacineRegistre`, `-DossierInstall`, `-MenuDemarrer`,
`-DossierEpingles` ; le désinstalleur ajoute `-DossierDonnees` et `-DossierDonneesLocales`). L'installeur a aussi
`-SansLancement` (ni arrêt ni lancement du résident, ni épingle, ni notification du Shell).

```powershell
.\tools\install\essai-installation.ps1   # faux registre HKCU:\Software\SpaceFilesTest-<id> + dossier temporaire
```

L'essai installe, ajoute à la main les clés de `defaut.rs::activer`, pose des éléments étrangers, désinstalle et exige
que l'état final soit identique à celui d'avant l'installation. Il affiche `OK` ou la liste des écarts (code 0 / 1).

## Non couvert

L'entrée dans « Applications installées » de Windows n'existe pas (aucune clé `Uninstall` n'est écrite). L'épingle de
barre des tâches et un vrai résident arrêté ne sont pas vérifiés par l'essai (voir le rapport du lot B).
