# Installe (ou met a jour) SpaceFiles pour l'utilisateur courant, sans droits admin :
#   %LOCALAPPDATA%\Programs\SpaceFiles\spacefiles.exe + icone
#   reprend l'installation d'avant le renommage (Nothing Explorer) sans rien perdre ; l'ancien dossier
#   Programs\NothingExplorer n'est pas supprime
#   raccourci du menu Demarrer (a epingler a la barre des taches)
#   lancement avec Windows : resident --resident (cle Run de HKCU, la meme que le reglage Win+E)
# A relancer apres chaque `cargo build --release`.
# Sans parametre : installation reelle, comme toujours. Les parametres ne servent qu'aux essais
# (tools/install/essai-installation.ps1) : des racines factices pour ne jamais toucher au vrai HKCU ni au vrai
# menu Demarrer. -SansLancement : ni arret ni lancement du resident, ni epingle de barre des taches, ni SHChangeNotify.
param(
  [string]$RacineRegistre = "HKCU:\Software",
  [string]$DossierInstall = (Join-Path $env:LOCALAPPDATA "Programs\SpaceFiles"),
  [string]$MenuDemarrer = (Join-Path $env:APPDATA "Microsoft\Windows\Start Menu\Programs"),
  [string]$DossierEpingles = (Join-Path $env:APPDATA "Microsoft\Internet Explorer\Quick Launch\User Pinned\TaskBar"),
  [switch]$SansLancement
)
$ErrorActionPreference = 'Stop'
$projet = Split-Path $PSCommandPath
$source = Join-Path $projet "target\release\spacefiles.exe"
$dest = $DossierInstall
$exe = Join-Path $dest "spacefiles.exe"
$destAncien = Join-Path (Split-Path $dest) "NothingExplorer"
$exeAncien = Join-Path $destAncien "nothing-explorer.exe"
$icoSource = Join-Path $projet "assets\spacefiles.ico"
# Nom d'icone qui change avec son contenu : le cache d'icones de Windows (cle = chemin) se met a jour
$version = (Get-FileHash $icoSource -Algorithm MD5).Hash.Substring(0, 8).ToLower()
$ico = Join-Path $dest "spacefiles-$version.ico"

Add-Type -Namespace NE -Name W -MemberDefinition @'
[DllImport("user32.dll", CharSet = CharSet.Unicode)] public static extern IntPtr FindWindowW(string c, string t);
[DllImport("user32.dll")] public static extern bool PostMessageW(IntPtr h, uint m, IntPtr w, IntPtr l);
[DllImport("kernel32.dll", CharSet = CharSet.Unicode, SetLastError = true)] public static extern IntPtr BeginUpdateResourceW(string f, bool effacer);
[DllImport("kernel32.dll", SetLastError = true)] public static extern bool UpdateResourceW(IntPtr h, IntPtr type, IntPtr nom, ushort langue, byte[] d, uint n);
[DllImport("kernel32.dll", SetLastError = true)] public static extern bool EndUpdateResourceW(IntPtr h, bool abandon);
[DllImport("shell32.dll")] public static extern void SHChangeNotify(int e, uint f, IntPtr a, IntPtr b);
// Icone dans les ressources de l'exe (RT_ICON + RT_GROUP_ICON) : barre des taches, Alt+Tab, epingle
public static void Icone(string exe, byte[] ico) {
  int n = BitConverter.ToUInt16(ico, 4);
  var groupe = new byte[6 + 14 * n];
  Array.Copy(ico, groupe, 6);
  IntPtr h = BeginUpdateResourceW(exe, false);
  if (h == IntPtr.Zero) throw new System.ComponentModel.Win32Exception();
  for (int i = 0; i < n; i++) {
    int e = 6 + 16 * i, taille = BitConverter.ToInt32(ico, e + 8), debut = BitConverter.ToInt32(ico, e + 12);
    var image = new byte[taille]; Array.Copy(ico, debut, image, 0, taille);
    UpdateResourceW(h, (IntPtr)3, (IntPtr)(i + 1), 0, image, (uint)taille);
    Array.Copy(ico, e, groupe, 6 + 14 * i, 12);
    BitConverter.GetBytes((ushort)(i + 1)).CopyTo(groupe, 6 + 14 * i + 12);
  }
  UpdateResourceW(h, (IntPtr)14, (IntPtr)1, 0, groupe, (uint)groupe.Length);
  if (!EndUpdateResourceW(h, false)) throw new System.ComponentModel.Win32Exception();
}
'@

# 1. Fermer le resident (il garde l'exe ouvert), celui-ci et celui d'avant le renommage (autre classe de fenetre) ;
# les fenetres ouvertes peuvent rester
# ([NullString] : $null deviendrait "" et FindWindow chercherait une fenetre sans titre)
if (-not $SansLancement) {  # (essai : on ne ferme jamais le vrai resident de l'auteur)
$fermes = $false
foreach ($classe in "SpaceFilesResident", "NothingExplorerResident") {
  $r = [NE.W]::FindWindowW($classe, [NullString]::Value)
  if ($r -ne [IntPtr]::Zero) { [NE.W]::PostMessageW($r, 0x10, [IntPtr]::Zero, [IntPtr]::Zero) | Out-Null; $fermes = $true }
}
if ($fermes) { Start-Sleep -Milliseconds 800 }
# Un resident qui ne s'est pas ferme garderait son crochet clavier (et l'ancienne version) : on l'arrete
Get-CimInstance Win32_Process | Where-Object { ($_.Name -eq "spacefiles.exe" -or $_.Name -eq "nothing-explorer.exe") -and $_.CommandLine -like "*--resident*" } | ForEach-Object {
  "arret force du resident $($_.ProcessId)"; Stop-Process -Id $_.ProcessId -Force -ErrorAction SilentlyContinue
}
Start-Sleep -Milliseconds 300
}

# 2. Copier (un exe encore utilise par une fenetre se renomme mais ne s'ecrase pas)
New-Item -ItemType Directory -Force $dest | Out-Null
Get-ChildItem $dest -Filter "spacefiles.old*.exe" | ForEach-Object { try { Remove-Item $_.FullName -Force } catch {} }
if (Test-Path $exe) {
  try { Remove-Item $exe -Force } catch { Rename-Item $exe ("spacefiles.old" + (Get-Date -Format HHmmss) + ".exe") }
}
Copy-Item $source $exe
[NE.W]::Icone($exe, [IO.File]::ReadAllBytes($icoSource))
Get-ChildItem (Join-Path $dest "*") -Include "nothing-explorer*.ico", "spacefiles*.ico" -File | Where-Object { $_.FullName -ne $ico } | ForEach-Object { try { Remove-Item $_.FullName -Force } catch {} }
Copy-Item $icoSource $ico -Force

# 3. Lancement avec Windows (resident) et Win+E : la valeur d'avant le renommage est remplacee, jamais gardee en double
$run = "$RacineRegistre\Microsoft\Windows\CurrentVersion\Run"
Set-ItemProperty $run -Name "SpaceFilesWinE" -Value "`"$exe`" --resident"
Remove-ItemProperty $run -Name "NothingExplorerWinE" -ErrorAction SilentlyContinue

# 4. Explorateur par defaut (E8) : si actif, les commandes suivent le nouvel emplacement. Meme regle que
# defaut.rs (migrer_anciennes_entrees) : le nouveau verbe et la sauvegarde sont ecrits AVANT que l'ancien soit retire,
# et un ancien verbe qui ne lance pas nothing-explorer.exe (autre programme) n'est jamais touche.
foreach ($c in "Folder", "Directory", "Drive") {
  $base = "$RacineRegistre\Classes\$c\shell"
  $ancienCmd = "$base\NothingExplorer\command"
  if (Test-Path $ancienCmd) {
    $commande = (Get-Item $ancienCmd).GetValue("")
    if ($commande -match '(^|[\\/"])nothing-explorer\.exe') {
      # Valeur de sauvegarde (le retour exact de l'explorateur de Windows) : copiee d'abord
      $sauvAncienne = (Get-ItemProperty $base -Name "NothingExplorerAncienne" -ErrorAction SilentlyContinue).NothingExplorerAncienne
      $sauvNouvelle = (Get-ItemProperty $base -Name "SpaceFilesAncienne" -ErrorAction SilentlyContinue).SpaceFilesAncienne
      if ($null -ne $sauvAncienne -and $null -eq $sauvNouvelle) { Set-ItemProperty $base -Name "SpaceFilesAncienne" -Value $sauvAncienne }
      New-Item "$base\SpaceFiles\command" -Force | Out-Null
      Set-Item "$base\SpaceFiles" -Value "Ouvrir dans SpaceFiles"
      Set-Item "$base\SpaceFiles\command" -Value "`"$exe`" `"%1`""
      if ((Get-Item $base).GetValue("") -eq "NothingExplorer") { Set-Item $base -Value "SpaceFiles" }
      Remove-Item "$base\NothingExplorer" -Recurse -Force
      Remove-ItemProperty $base -Name "NothingExplorerAncienne" -ErrorAction SilentlyContinue
    }
  } elseif (Test-Path "$base\SpaceFiles\command") {
    Set-Item "$base\SpaceFiles\command" -Value "`"$exe`" `"%1`""
  }
}

# 4b. Adresses spacefiles://open?path=... , search?q=... , group?name=... (navigation seulement) ;
# nothingexplorer:// reste enregistre comme alias (des liens existants l'utilisent), vers le nouvel exe
foreach ($proto in "spacefiles", "nothingexplorer") {
  $p = "$RacineRegistre\Classes\$proto"
  New-Item "$p\shell\open\command" -Force | Out-Null
  Set-ItemProperty $p -Name "(default)" -Value "URL:SpaceFiles"
  Set-ItemProperty $p -Name "URL Protocol" -Value ""
  Set-Item "$p\shell\open\command" -Value "`"$exe`" `"%1`""
}

# 5. Raccourci du menu Demarrer, avec l'icone en points
$menu = $MenuDemarrer
$lnk = Join-Path $menu "SpaceFiles.lnk"
$lnkAncien = Join-Path $menu "Nothing Explorer.lnk"
$s = (New-Object -ComObject WScript.Shell).CreateShortcut($lnk)
$s.TargetPath = $exe; $s.WorkingDirectory = $dest; $s.IconLocation = "$ico,0"
$s.Description = "Explorateur de fichiers au style Nothing"; $s.Save()
if (Test-Path $lnkAncien) { Remove-Item $lnkAncien -Force }
# Epingle eventuelle de la barre des taches : retargetee vers le nouvel exe (elle visait l'ancien ou deja le nouveau), meme icone
if (-not $SansLancement) {
Get-ChildItem $DossierEpingles -Filter *.lnk -ErrorAction SilentlyContinue | ForEach-Object {
  $p = (New-Object -ComObject WScript.Shell).CreateShortcut($_.FullName)
  if ($p.TargetPath -ieq $exe -or $p.TargetPath -ieq $exeAncien) {
    $p.TargetPath = $exe; $p.WorkingDirectory = $dest; $p.IconLocation = "$ico,0"; $p.Save()
  }
}
[NE.W]::SHChangeNotify(0x08000000, 0, [IntPtr]::Zero, [IntPtr]::Zero)  # SHCNE_ASSOCCHANGED : rafraichit les icones
}

# 6. Resident relance depuis l'exe installe
if (-not $SansLancement) { Start-Process $exe -ArgumentList "--resident" }
"installe : $exe"
if (Test-Path $destAncien) { "l'ancien dossier $destAncien n'est pas supprime : il peut l'etre a la main une fois SpaceFiles verifie" }
