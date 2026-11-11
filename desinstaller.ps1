# Retire SpaceFiles de la machine, exactement ce que installer.ps1 et l'application (src/defaut.rs) ont ecrit, rien d'autre.
#   .\desinstaller.ps1 -Simuler          liste ce qui serait retire, ne modifie rien (lecture seule)
#   .\desinstaller.ps1                   liste, demande « O/N », puis retire
#   .\desinstaller.ps1 -Oui              retire sans demander
#   .\desinstaller.ps1 -SupprimerDonnees retire aussi les reglages, l'index et le journal (%APPDATA%\SpaceFiles,
#                                        %LOCALAPPDATA%\SpaceFiles) et les dossiers de l'ancien nom (NothingExplorer)
# Par defaut les donnees de l'utilisateur restent. Aucun droit administrateur : tout est dans HKCU et le profil.
# Les parametres de racines (memes noms que dans installer.ps1) servent aux essais : voir tools/install/essai-installation.ps1.
# Idempotent : relance sans rien a retirer, il le dit et sort avec le code 0. Code 1 si une suppression a echoue,
# code 2 si l'utilisateur refuse la confirmation.
param(
  [switch]$Simuler,
  [switch]$Oui,
  [switch]$SupprimerDonnees,
  [string]$RacineRegistre = "HKCU:\Software",
  [string]$DossierInstall = (Join-Path $env:LOCALAPPDATA "Programs\SpaceFiles"),
  [string]$MenuDemarrer = (Join-Path $env:APPDATA "Microsoft\Windows\Start Menu\Programs"),
  [string]$DossierEpingles = (Join-Path $env:APPDATA "Microsoft\Internet Explorer\Quick Launch\User Pinned\TaskBar"),
  [string]$DossierDonnees = $env:APPDATA,
  [string]$DossierDonneesLocales = $env:LOCALAPPDATA,
  [int]$DelaiFermeture = 5
)
$ErrorActionPreference = 'Stop'

if (-not $RacineRegistre.StartsWith('HKCU:\', [StringComparison]::OrdinalIgnoreCase)) {
  Write-Host "Racine de registre refusee : seule HKCU:\ est geree ($RacineRegistre)"; exit 1
}

$exe = Join-Path $DossierInstall "spacefiles.exe"
$dossierAncien = Join-Path (Split-Path $DossierInstall) "NothingExplorer"
$exeAncien = Join-Path $dossierAncien "nothing-explorer.exe"
$run = "$RacineRegistre\Microsoft\Windows\CurrentVersion\Run"
$classesDossier = "Folder", "Directory", "Drive"
$ancienWinE = "$RacineRegistre\Classes\CLSID\{52205fd8-5dfb-447d-801a-d0b52f2e83e1}"

# ── Outils ──

function Normaliser([string]$p) {
  try { [IO.Path]::GetFullPath($p).TrimEnd('\').ToLowerInvariant() } catch { $p.TrimEnd('\').ToLowerInvariant() }
}

# Valeur d'une cle ("" : la valeur par defaut) ; $null si la cle ou la valeur n'existe pas
function Lire-Valeur([string]$cle, [string]$nom) {
  $k = Get-Item -LiteralPath $cle -ErrorAction SilentlyContinue
  if ($null -eq $k) { return $null }
  try { return $k.GetValue($nom, $null) } finally { $k.Close() }
}

function Valeur-Existe([string]$cle, [string]$nom) {
  $k = Get-Item -LiteralPath $cle -ErrorAction SilentlyContinue
  if ($null -eq $k) { return $false }
  try { return [bool]($k.GetValueNames() -contains $nom) } finally { $k.Close() }
}

# Ouvre en ecriture la sous-cle de HKCU correspondant a un chemin « HKCU:\... » (le fournisseur de registre de PowerShell
# ne sait ni lire ni effacer proprement la valeur par defaut)
function Ouvrir-Ecriture([string]$cle) {
  [Microsoft.Win32.Registry]::CurrentUser.OpenSubKey($cle.Substring(6), $true)
}

function Supprimer-Valeur([string]$cle, [string]$nom) {
  $k = Ouvrir-Ecriture $cle
  if ($null -eq $k) { return }
  try { $k.DeleteValue($nom, $false) } finally { $k.Close() }
}

function Ecrire-Valeur([string]$cle, [string]$nom, [string]$valeur) {
  $k = Ouvrir-Ecriture $cle
  if ($null -eq $k) { throw "cle introuvable : $cle" }
  try { $k.SetValue($nom, $valeur) } finally { $k.Close() }
}

# Chemin de l'exe d'une ligne de commande (« "C:\a b\x.exe" "%1" » ou « C:\x.exe --resident »)
function Chemin-Exe([string]$commande) {
  if ([string]::IsNullOrWhiteSpace($commande)) { return $null }
  $c = $commande.Trim()
  if ($c.StartsWith('"')) {
    $f = $c.IndexOf('"', 1)
    if ($f -lt 0) { return $null }
    return $c.Substring(1, $f - 1)
  }
  $m = [regex]::Match($c, '^(.*?\.exe)', 'IgnoreCase')
  if ($m.Success) { return $m.Groups[1].Value }
  return $null
}

function Exe-Est([string]$commande, [string[]]$noms) {
  $p = Chemin-Exe $commande
  if (-not $p) { return $false }
  return [bool]($noms -contains [IO.Path]::GetFileName($p).ToLowerInvariant())
}
function Lance-Notre-Exe([string]$commande) { Exe-Est $commande @("spacefiles.exe", "nothing-explorer.exe") }
function Lance-Ancien-Exe([string]$commande) { Exe-Est $commande @("nothing-explorer.exe") }

# Processus dont le chemin est celui du dossier d'installation visé (ou de l'ancien dossier) : jamais par nom seul
function Trouver-Processus {
  $dirNeuf = Normaliser $DossierInstall
  $dirAncien = Normaliser $dossierAncien
  @(Get-CimInstance Win32_Process | Where-Object { $_.ExecutablePath } | Where-Object {
    $rep = Normaliser (Split-Path $_.ExecutablePath)
    $nom = $_.Name.ToLowerInvariant()
    (($rep -eq $dirNeuf) -and ($nom -like "spacefiles*.exe")) -or (($rep -eq $dirAncien) -and ($nom -eq "nothing-explorer.exe"))
  })
}

# Fermeture douce : WM_CLOSE a chaque fenetre de haut niveau du processus (identifie par son numero)
function Charger-Fermeture {
  if ('SF.Fermeture' -as [type]) { return }
  Add-Type -Namespace SF -Name Fermeture -MemberDefinition @'
public delegate bool Rappel(IntPtr h, IntPtr l);
[System.Runtime.InteropServices.DllImport("user32.dll")] static extern bool EnumWindows(Rappel p, IntPtr l);
[System.Runtime.InteropServices.DllImport("user32.dll")] static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
[System.Runtime.InteropServices.DllImport("user32.dll")] static extern bool PostMessageW(IntPtr h, uint m, IntPtr w, IntPtr l);
public static int Fermer(uint pid) {
  int n = 0;
  EnumWindows(delegate (IntPtr h, IntPtr l) {
    uint p; GetWindowThreadProcessId(h, out p);
    if (p == pid && PostMessageW(h, 0x10, IntPtr.Zero, IntPtr.Zero)) n++;
    return true;
  }, IntPtr.Zero);
  return n;
}
'@
}

function Arreter-Processus {
  $procs = Trouver-Processus
  if ($procs.Count -eq 0) { return }
  Charger-Fermeture
  foreach ($p in $procs) {
    $n = [SF.Fermeture]::Fermer([uint32]$p.ProcessId)
    Write-Host "  fermeture douce du processus $($p.ProcessId) ($n fenêtre(s) prévenue(s)) : $($p.ExecutablePath)"
  }
  $fin = (Get-Date).AddSeconds($DelaiFermeture)
  while ((Get-Date) -lt $fin -and (Trouver-Processus).Count -gt 0) { Start-Sleep -Milliseconds 200 }
  # On relit la liste : seul un processus qui a toujours le bon chemin est arrete, par son numero
  foreach ($p in (Trouver-Processus)) {
    Write-Host "  arrêt du processus $($p.ProcessId) (encore là après $DelaiFermeture s)"
    Stop-Process -Id $p.ProcessId -Force -ErrorAction SilentlyContinue
  }
  Start-Sleep -Milliseconds 300
}

# Un exe qui vient d'etre arrete peut rester tenu un instant : on reessaie avant de declarer l'echec
function Retirer-Chemin([string]$chemin, [bool]$arbre) {
  for ($i = 1; $i -le 4; $i++) {
    try {
      if ($arbre) { Remove-Item -LiteralPath $chemin -Recurse -Force -ErrorAction Stop } else { Remove-Item -LiteralPath $chemin -Force -ErrorAction Stop }
      return
    } catch {
      if ($i -eq 4) { Write-Host "  impossible de retirer $chemin : $($_.Exception.Message)"; return }
      Write-Host "  $chemin est tenu, nouvel essai ($i/3)"
      Start-Sleep -Seconds 1
    }
  }
}

function Cible-Raccourci([string]$lnk) {
  try { (New-Object -ComObject WScript.Shell).CreateShortcut($lnk).TargetPath } catch { $null }
}

function Resume-Dossier([string]$dossier) {
  $f = @(Get-ChildItem -LiteralPath $dossier -Recurse -File -Force -ErrorAction SilentlyContinue)
  $taille = ($f | Measure-Object Length -Sum).Sum
  "{0} fichier(s), {1:N0} Ko" -f $f.Count, ([double]$taille / 1KB)
}

# ── Plan : tout ce qui peut exister, avec son test (« est-ce encore la ? ») et son retrait ──

$items = New-Object System.Collections.ArrayList
$conserves = New-Object System.Collections.ArrayList
$laisses = New-Object System.Collections.ArrayList

# $d : donnees de l'element, passees aux deux blocs (pas de fermeture : les blocs lisent $args[0])
function Ajouter([string]$genre, [string]$libelle, $d, [scriptblock]$test, [scriptblock]$retirer) {
  $o = [pscustomobject]@{ Genre = $genre; Libelle = $libelle; D = $d; Test = $test; Retirer = $retirer; Present = $false }
  $o.Present = [bool](& $test $d)
  [void]$items.Add($o)
}

# 1. Processus residents lances depuis le dossier d'installation
$procs = Trouver-Processus
$libProc = if ($procs.Count -gt 0) { "processus " + (($procs | ForEach-Object { "$($_.ProcessId) ($($_.ExecutablePath))" }) -join ", ") } else { "processus spacefiles.exe du dossier $DossierInstall" }
Ajouter "processus" $libProc @{} { @(Trouver-Processus).Count -gt 0 } { Arreter-Processus }

# 2. Lancement avec Windows (cle Run)
Ajouter "registre" "valeur $run : SpaceFilesWinE" @{ Cle = $run; Nom = "SpaceFilesWinE" } {
  param($d) Valeur-Existe $d.Cle $d.Nom
} { param($d) Supprimer-Valeur $d.Cle $d.Nom }
# L'ancienne valeur n'est a nous que si elle lance nothing-explorer.exe (meme regle que defaut.rs)
$ancienneRun = Lire-Valeur $run "NothingExplorerWinE"
if ($null -ne $ancienneRun -and -not (Lance-Ancien-Exe $ancienneRun)) {
  [void]$laisses.Add("valeur $run : NothingExplorerWinE (ne lance pas nothing-explorer.exe : pas à nous)")
} else {
  Ajouter "registre" "valeur $run : NothingExplorerWinE" @{ Cle = $run; Nom = "NothingExplorerWinE" } {
    param($d) (Valeur-Existe $d.Cle $d.Nom) -and (Lance-Ancien-Exe (Lire-Valeur $d.Cle $d.Nom))
  } { param($d) Supprimer-Valeur $d.Cle $d.Nom }
}

# 3. Verbes « Ouvrir dans SpaceFiles » des dossiers, valeur par defaut de `shell` et sauvegardes
foreach ($c in $classesDossier) {
  $base = "$RacineRegistre\Classes\$c\shell"
  $ancienNous = Lance-Ancien-Exe (Lire-Valeur "$base\NothingExplorer\command" "")
  $d = @{ Base = $base; AncienNous = [bool]$ancienNous }
  # a) la valeur par defaut de `shell` : restauree depuis la sauvegarde, sinon retiree, seulement si c'est la notre
  Ajouter "registre" "valeur par défaut de $base (restaurée depuis la sauvegarde, sinon retirée)" $d {
    param($d)
    $v = Lire-Valeur $d.Base ""
    ($v -eq "SpaceFiles") -or ($d.AncienNous -and $v -eq "NothingExplorer")
  } {
    param($d)
    $avant = Lire-Valeur $d.Base "SpaceFilesAncienne"
    if ($null -eq $avant) { $avant = Lire-Valeur $d.Base "NothingExplorerAncienne" }
    if ($null -eq $avant) {
      Supprimer-Valeur $d.Base ""
      Write-Host "  valeur par défaut de $($d.Base) retirée (pas de sauvegarde : il n'y en avait pas)"
    } else {
      Ecrire-Valeur $d.Base "" $avant
      Write-Host "  valeur par défaut de $($d.Base) restaurée : « $avant »"
    }
  }
  # b) le verbe SpaceFiles (nom exact, comme `desactiver` de defaut.rs)
  Ajouter "registre" "clé $base\SpaceFiles" @{ Cle = "$base\SpaceFiles" } {
    param($d) Test-Path -LiteralPath $d.Cle
  } { param($d) Retirer-Chemin $d.Cle $true }
  # c) l'ancien verbe, seulement s'il lance nothing-explorer.exe
  $ancienCmd = Lire-Valeur "$base\NothingExplorer\command" ""
  if ((Test-Path -LiteralPath "$base\NothingExplorer") -and -not (Lance-Ancien-Exe $ancienCmd)) {
    [void]$laisses.Add("clé $base\NothingExplorer (ne lance pas nothing-explorer.exe : pas à nous)")
  } else {
    Ajouter "registre" "clé $base\NothingExplorer" @{ Cle = "$base\NothingExplorer" } {
      param($d) (Test-Path -LiteralPath $d.Cle) -and (Lance-Ancien-Exe (Lire-Valeur "$($d.Cle)\command" ""))
    } { param($d) Retirer-Chemin $d.Cle $true }
  }
  # d) les valeurs de sauvegarde (apres la restauration ci-dessus)
  foreach ($nom in "SpaceFilesAncienne", "NothingExplorerAncienne") {
    Ajouter "registre" "valeur $base : $nom" @{ Cle = $base; Nom = $nom } {
      param($d) Valeur-Existe $d.Cle $d.Nom
    } { param($d) Supprimer-Valeur $d.Cle $d.Nom }
  }
}

# 4. Protocoles spacefiles:// et nothingexplorer:// : a nous si la commande lance notre exe (ou, sans commande, si la
# valeur par defaut est celle de l'installeur)
function Protocole-Est-Notre([string]$p) {
  $cmd = Lire-Valeur "$p\shell\open\command" ""
  if (Lance-Notre-Exe $cmd) { return $true }
  return [bool]([string]::IsNullOrEmpty($cmd) -and (Lire-Valeur $p "") -eq "URL:SpaceFiles")
}
foreach ($proto in "spacefiles", "nothingexplorer") {
  $p = "$RacineRegistre\Classes\$proto"
  if ((Test-Path -LiteralPath $p) -and -not (Protocole-Est-Notre $p)) {
    [void]$laisses.Add("clé $p (sa commande ne lance pas SpaceFiles : pas à nous)")
  } else {
    Ajouter "registre" "clé $p (protocole)" @{ Cle = $p } {
      param($d) (Test-Path -LiteralPath $d.Cle) -and (Protocole-Est-Notre $d.Cle)
    } { param($d) Retirer-Chemin $d.Cle $true }
  }
}

# 5. Essai Win+E d'une version precedente (copie de « Explorateur de fichiers » dans HKCU), retire s'il est de nous
Ajouter "registre" "clé $ancienWinE (ancien essai Win+E)" @{ Cle = $ancienWinE } {
  param($d)
  (Test-Path -LiteralPath $d.Cle) -and ([string](Lire-Valeur "$($d.Cle)\shell\opennewwindow\command" "")).ToLowerInvariant().Contains("nothing-explorer")
} { param($d) Retirer-Chemin $d.Cle $true }

# 6. Raccourcis du menu Demarrer (seulement s'ils visent notre exe)
foreach ($nom in "SpaceFiles.lnk", "Nothing Explorer.lnk") {
  $lnk = Join-Path $MenuDemarrer $nom
  if (Test-Path -LiteralPath $lnk) {
    $cible = Cible-Raccourci $lnk
    if ($cible -and -not (Lance-Notre-Exe $cible)) { [void]$laisses.Add("raccourci $lnk (vise $cible : pas à nous)"); continue }
  }
  Ajouter "fichier" "raccourci $lnk" @{ Chemin = $lnk } {
    param($d) Test-Path -LiteralPath $d.Chemin
  } { param($d) Retirer-Chemin $d.Chemin $false }
}

# 7. Epingle eventuelle de la barre des taches (retargetee par l'installeur vers notre exe)
if (Test-Path -LiteralPath $DossierEpingles) {
  foreach ($f in @(Get-ChildItem -LiteralPath $DossierEpingles -Filter *.lnk -File -ErrorAction SilentlyContinue)) {
    $cible = Cible-Raccourci $f.FullName
    if ($cible -and ((Normaliser $cible) -eq (Normaliser $exe) -or (Normaliser $cible) -eq (Normaliser $exeAncien))) {
      Ajouter "fichier" "épingle de la barre des tâches $($f.FullName)" @{ Chemin = $f.FullName } {
        param($d) Test-Path -LiteralPath $d.Chemin
      } { param($d) Retirer-Chemin $d.Chemin $false }
    }
  }
}

# 8. Dossier d'installation : nos fichiers, puis le dossier s'il ne reste rien d'autre
if (Test-Path -LiteralPath $DossierInstall) {
  $etrangers = @()
  foreach ($f in @(Get-ChildItem -LiteralPath $DossierInstall -Force)) {
    $n = $f.Name.ToLowerInvariant()
    if (-not $f.PSIsContainer -and ($n -eq "spacefiles.exe" -or $n -like "spacefiles.old*.exe" -or $n -like "spacefiles*.ico" -or $n -like "nothing-explorer*.ico")) {
      Ajouter "fichier" "fichier $($f.FullName)" @{ Chemin = $f.FullName } {
        param($d) Test-Path -LiteralPath $d.Chemin
      } { param($d) Retirer-Chemin $d.Chemin $false }
    } else { $etrangers += $f.FullName }
  }
  if ($etrangers.Count -eq 0) {
    Ajouter "fichier" "dossier $DossierInstall (vide une fois nos fichiers retirés)" @{ Chemin = $DossierInstall } {
      param($d) Test-Path -LiteralPath $d.Chemin
    } { param($d) Retirer-Chemin $d.Chemin $false }
  } else {
    [void]$conserves.Add("dossier $DossierInstall : contient d'autres fichiers que les nôtres ($($etrangers -join ', ')), il reste")
  }
} else {
  Ajouter "fichier" "dossier $DossierInstall" @{ Chemin = $DossierInstall } { param($d) Test-Path -LiteralPath $d.Chemin } { param($d) Retirer-Chemin $d.Chemin $true }
}

# 9. Ancien dossier d'installation (Programs\NothingExplorer) : avec -SupprimerDonnees, ou s'il est vide
if (Test-Path -LiteralPath $dossierAncien) {
  $vide = @(Get-ChildItem -LiteralPath $dossierAncien -Force).Count -eq 0
  if ($SupprimerDonnees -or $vide) {
    Ajouter "fichier" "ancien dossier $dossierAncien ($(Resume-Dossier $dossierAncien))" @{ Chemin = $dossierAncien } {
      param($d) Test-Path -LiteralPath $d.Chemin
    } { param($d) Retirer-Chemin $d.Chemin $true }
  } else {
    [void]$conserves.Add("ancien dossier $dossierAncien ($(Resume-Dossier $dossierAncien)) : retiré seulement avec -SupprimerDonnees")
  }
}

# 10. Donnees de l'utilisateur : gardees sauf -SupprimerDonnees
foreach ($pere in $DossierDonnees, $DossierDonneesLocales) {
  foreach ($nom in "SpaceFiles", "NothingExplorer") {
    $dos = Join-Path $pere $nom
    if (-not (Test-Path -LiteralPath $dos)) { continue }
    if ($SupprimerDonnees) {
      Ajouter "données" "dossier de données $dos ($(Resume-Dossier $dos))" @{ Chemin = $dos } {
        param($d) Test-Path -LiteralPath $d.Chemin
      } { param($d) Retirer-Chemin $d.Chemin $true }
    } else {
      [void]$conserves.Add("données $dos ($(Resume-Dossier $dos)) : conservées, retirées seulement avec -SupprimerDonnees")
    }
  }
}

# ── Affichage du plan ──

$aRetirer = @($items | Where-Object { $_.Present })
$mode = if ($Simuler) { "SIMULATION : rien n'est modifié" } else { "DÉSINSTALLATION" }
Write-Host "SpaceFiles — $mode"
Write-Host "  dossier d'installation : $DossierInstall"
foreach ($l in $conserves) { Write-Host "CONSERVÉ : $l" }
foreach ($l in $laisses) { Write-Host "LAISSÉ   : $l" }

if ($aRetirer.Count -eq 0) {
  Write-Host "Rien à retirer."
  exit 0
}
foreach ($i in $aRetirer) {
  $verbe = if ($Simuler) { "SERAIT RETIRÉ" } else { "À RETIRER" }
  Write-Host ("{0} : {1}" -f $verbe, $i.Libelle)
}
if ($Simuler) { Write-Host "$($aRetirer.Count) élément(s) seraient retirés (simulation, rien n'a été modifié)."; exit 0 }

if (-not $Oui) {
  try { $rep = Read-Host "Retirer tout cela ? (O/N)" } catch { Write-Host "Pas de confirmation possible (session non interactive) : annulé. Utiliser -Oui."; exit 2 }
  if ($rep -notmatch '^(o|oui|y|yes)$') { Write-Host "Annulé, rien n'a été modifié."; exit 2 }
}

# ── Retrait, dans l'ordre du plan (processus d'abord : il tient l'exe) ──

foreach ($i in $aRetirer) {
  Write-Host "retrait : $($i.Libelle)"
  try { & $i.Retirer $i.D } catch { Write-Host "  erreur : $($_.Exception.Message)" }
}

# ── Rapport : chaque element est relu ──

$echecs = 0; $retires = 0; $absents = 0
foreach ($i in $items) {
  if (-not $i.Present) { $etat = "ABSENT"; $absents++ }
  elseif (& $i.Test $i.D) { $etat = "ÉCHEC"; $echecs++ }
  else { $etat = "RETIRÉ"; $retires++ }
  Write-Host ("{0,-7} : {1}" -f $etat, $i.Libelle)
}
Write-Host "Bilan : $retires retiré(s), $absents absent(s), $echecs échec(s)."
if ($echecs -gt 0) { exit 1 }
exit 0
