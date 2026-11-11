# Essai de l'installeur et du desinstalleur sur un FAUX environnement : jamais le vrai HKCU, ni le vrai menu Demarrer, ni
# le vrai dossier d'installation. Tout vit dans HKCU:\Software\SpaceFilesTest-<id> et dans un dossier temporaire dont le nom
# contient un espace et un accent. Aucune fenetre n'est ouverte, aucune entree souris, aucun processus de l'auteur touche.
#
#   .\tools\install\essai-installation.ps1
#
# Methode : on construit un etat « etranger » (valeur Run d'une autre application, verbes shell d'autres programmes, un
# verbe qui porte l'ancien nom mais lance un autre exe, un spacefiles.exe ailleurs et vivant, des fichiers utilisateur...),
# on le photographie (registre + fichiers), on installe, on ajoute a la main les cles de defaut.rs::activer, on lance
# le desinstalleur (-Simuler, puis pour de vrai, puis une seconde fois, puis avec -SupprimerDonnees) et on exige que
# l'etat final soit IDENTIQUE a la photographie de depart : tout ce qui est a nous a disparu, rien d'etranger n'a bouge.
# Sortie : « OK » ou la liste des ecarts. Code 0 si OK, 1 sinon.
$ErrorActionPreference = 'Stop'
$racine = Split-Path (Split-Path $PSScriptRoot)
$installeur = Join-Path $racine "installer.ps1"
$desinstalleur = Join-Path $racine "desinstaller.ps1"
$ecarts = New-Object System.Collections.ArrayList
$comptes = @{ Retire = 0; Etranger = 0; Verifs = 0 }
$aNettoyer = New-Object System.Collections.ArrayList   # environnements crees
$processusEssai = New-Object System.Collections.ArrayList   # numeros des processus de l'essai (seuls ceux-la sont arretes)

function Verifier([bool]$condition, [string]$message) {
  $comptes.Verifs++
  if (-not $condition) { [void]$ecarts.Add($message); Write-Host "  ECART : $message" }
}

function Lancer([string]$script, [string[]]$arguments) {
  $sortie = & powershell.exe -NoProfile -NonInteractive -ExecutionPolicy Bypass -File $script @arguments 2>&1 | Out-String
  [pscustomobject]@{ Code = $LASTEXITCODE; Sortie = $sortie }
}

function Nouveau-Lnk([string]$chemin, [string]$cible) {
  $s = (New-Object -ComObject WScript.Shell).CreateShortcut($chemin)
  $s.TargetPath = $cible; $s.Save()
}

function Cle([string]$chemin) { if (-not (Test-Path -LiteralPath $chemin)) { New-Item -Path $chemin -Force | Out-Null } }
function Valeur([string]$cle, [string]$nom, [string]$valeur) { Cle $cle; Set-ItemProperty -LiteralPath $cle -Name $nom -Value $valeur }
function Defaut([string]$cle, [string]$valeur) {
  Cle $cle
  $k = [Microsoft.Win32.Registry]::CurrentUser.OpenSubKey($cle.Substring(6), $true)
  try { $k.SetValue("", $valeur) } finally { $k.Close() }
}
function Lire-Defaut([string]$cle) {
  $k = Get-Item -LiteralPath $cle -ErrorAction SilentlyContinue
  if ($null -eq $k) { return $null }
  try { $k.GetValue("", $null) } finally { $k.Close() }
}

# Photographie du registre (toutes les cles et valeurs sous la racine) et des fichiers, en lignes triees
function Photo-Registre($e) {
  $cles = @(Get-Item -LiteralPath $e.Reg) + @(Get-ChildItem -LiteralPath $e.Reg -Recurse)
  $lignes = foreach ($k in $cles) {
    $noms = @($k.GetValueNames())
    if ($noms.Count -eq 0) { "REG $($k.Name) |" }
    foreach ($n in $noms) { "REG $($k.Name) | [$n] ($($k.GetValueKind($n))) = $($k.GetValue($n, $null, 'DoNotExpandEnvironmentNames'))" }
  }
  @($lignes | Sort-Object)
}
function Photo-Fichiers($e) {
  $lignes = foreach ($f in Get-ChildItem -LiteralPath $e.Base -Recurse -Force) {
    $rel = $f.FullName.Substring($e.Base.Length + 1)
    if ($f.PSIsContainer) { "FIC $rel | dossier" } else { "FIC $rel | $($f.Length) octets | $((Get-FileHash -LiteralPath $f.FullName -Algorithm SHA256).Hash)" }
  }
  @($lignes | Sort-Object)
}
function Photo($e) { @(Photo-Registre $e) + @(Photo-Fichiers $e) }

function Comparer([string]$titre, $attendu, $obtenu) {
  $diff = @(Compare-Object -ReferenceObject @($attendu) -DifferenceObject @($obtenu))
  Verifier ($diff.Count -eq 0) "$titre : $($diff.Count) ligne(s) different(s) :`n    $(($diff | ForEach-Object { "$($_.SideIndicator) $($_.InputObject)" }) -join "`n    ")"
}

# Environnement factice : un dossier temporaire (espace + accent) et une racine de registre propre a l'essai
function Nouvel-Environnement {
  $id = [guid]::NewGuid().ToString('N').Substring(0, 8)
  $base = Join-Path $env:TEMP "SpaceFiles essai é $id"
  $e = @{
    Id = $id; Base = $base; Reg = "HKCU:\Software\SpaceFilesTest-$id"
    Programs = Join-Path $base "Local\Programs"; Install = Join-Path $base "Local\Programs\SpaceFiles"
    Menu = Join-Path $base "Roaming\Menu Démarrer"; Epingles = Join-Path $base "Roaming\Epingles"
    Roaming = Join-Path $base "Roaming"; Local = Join-Path $base "Local"; Ailleurs = Join-Path $base "Ailleurs"
  }
  $e.Exe = Join-Path $e.Install "spacefiles.exe"
  foreach ($d in $e.Programs, $e.Menu, $e.Epingles, $e.Ailleurs) { New-Item -ItemType Directory -Force $d | Out-Null }
  # Cles qui existent deja sur toute machine : l'installeur ne les cree pas, le desinstalleur ne les retire jamais
  Cle "$($e.Reg)\Microsoft\Windows\CurrentVersion\Run"
  foreach ($c in "Folder", "Directory", "Drive") { Cle "$($e.Reg)\Classes\$c\shell" }
  [void]$aNettoyer.Add($e)
  $e
}

# Un exe inoffensif (ping) qui sert de « spacefiles.exe » : un exe tenu par un processus, sans fenetre
$ping = Join-Path $env:SystemRoot "System32\PING.EXE"

function Lancer-Ping([string]$exe) {
  $p = Start-Process $exe -ArgumentList "-n", "900", "127.0.0.1" -WindowStyle Hidden -PassThru
  [void]$processusEssai.Add($p.Id)
  $p.Id
}

# L'etranger : tout ce qui appartient a d'autres programmes ou a l'utilisateur et ne doit jamais bouger
function Poser-Etranger($e) {
  $r = $e.Reg
  Valeur "$r\Microsoft\Windows\CurrentVersion\Run" "AutreAppli" '"C:\Autre\autre.exe" /silencieux'
  Defaut "$r\Classes\Folder\shell" "none"
  # Directory\shell n'a pas de valeur par defaut a l'origine ; Drive en a une autre
  Defaut "$r\Classes\Drive\shell" "ExplorateurX"
  Defaut "$r\Classes\Folder\shell\AutreProgramme" "Ouvrir avec Autre"
  Defaut "$r\Classes\Folder\shell\AutreProgramme\command" '"C:\Autre\autre.exe" "%1"'
  Defaut "$r\Classes\Directory\shell\AutreProgramme" "Ouvrir avec Autre"
  Defaut "$r\Classes\Directory\shell\AutreProgramme\command" '"C:\Autre\autre.exe" "%1"'
  # Un verbe qui porte l'ancien nom mais lance un autre exe : jamais touche (regle de defaut.rs)
  Defaut "$r\Classes\Directory\shell\NothingExplorer" "Autre chose"
  Defaut "$r\Classes\Directory\shell\NothingExplorer\command" '"C:\Autre\pas-nous.exe" "%1"'
  Defaut "$r\Classes\monprotocole" "URL:Mon protocole"
  Valeur "$r\Classes\monprotocole" "URL Protocol" ""
  Defaut "$r\Classes\monprotocole\shell\open\command" '"C:\Autre\autre.exe" "%1"'
  # Fichiers : un exe « spacefiles.exe » ailleurs que dans le dossier d'installation, des raccourcis d'autres programmes,
  # des fichiers utilisateur
  Copy-Item $ping (Join-Path $e.Ailleurs "spacefiles.exe")
  Copy-Item $ping (Join-Path $e.Ailleurs "autre.exe")
  Nouveau-Lnk (Join-Path $e.Menu "Autre.lnk") (Join-Path $e.Ailleurs "autre.exe")
  Nouveau-Lnk (Join-Path $e.Epingles "Autre.lnk") (Join-Path $e.Ailleurs "autre.exe")
  New-Item -ItemType Directory -Force (Join-Path $e.Roaming "Autre") | Out-Null
  Set-Content -LiteralPath (Join-Path $e.Roaming "Autre\fichier utilisateur é.txt") -Value "à moi" -Encoding UTF8
}

# Ce que fait defaut.rs::activer (lignes 145-169), a l'identique, pour une classe et un exe donnes
function Activer-Comme-Rust($e, [string]$classe, [string]$exe) {
  $base = "$($e.Reg)\Classes\$classe\shell"
  Defaut "$base\SpaceFiles" "Ouvrir dans SpaceFiles"
  Defaut "$base\SpaceFiles\command" "`"$exe`" `"%1`""
  $avant = Lire-Defaut $base
  if ($null -ne $avant -and $avant -ne "SpaceFiles") { Valeur $base "SpaceFilesAncienne" $avant }
  Defaut $base "SpaceFiles"
}

function Poser-Donnees($e) {
  foreach ($d in (Join-Path $e.Roaming "SpaceFiles"), (Join-Path $e.Local "SpaceFiles"), (Join-Path $e.Roaming "NothingExplorer")) {
    New-Item -ItemType Directory -Force $d | Out-Null
    Set-Content -LiteralPath (Join-Path $d "reglages é.json") -Value "{}" -Encoding UTF8
  }
}

function Parametres($e, [string[]]$en_plus) {
  @("-RacineRegistre", $e.Reg, "-DossierInstall", $e.Install, "-MenuDemarrer", $e.Menu, "-DossierEpingles", $e.Epingles) + $en_plus
}
function Param-Des($e, [string[]]$en_plus) {
  (Parametres $e @()) + @("-DossierDonnees", $e.Roaming, "-DossierDonneesLocales", $e.Local, "-DelaiFermeture", "1") + $en_plus
}

function Lire-Processus-SpaceFiles {
  @(Get-CimInstance Win32_Process -Filter "Name='spacefiles.exe'" | ForEach-Object { "$($_.ProcessId) $($_.ExecutablePath)" })
}

$auteurAvant = @(Lire-Processus-SpaceFiles)
Write-Host "spacefiles.exe presents avant l'essai : $(if ($auteurAvant.Count) { $auteurAvant -join ' ; ' } else { 'aucun' })"

try {
  # ═════ Scenario A : installation complete, activation « comme Rust », desinstallation ═════
  Write-Host "`n== Scenario A : installation, activer(), exe tenu par un processus, desinstallation =="
  $e = Nouvel-Environnement
  Poser-Etranger $e
  $s0 = Photo $e

  $i = Lancer $installeur (Parametres $e @("-SansLancement"))
  Verifier ($i.Code -eq 0) "installeur : code $($i.Code) au lieu de 0`n$($i.Sortie)"
  $i = Lancer $installeur (Parametres $e @("-SansLancement"))   # mise a jour = relancer l'installeur
  Verifier ($i.Code -eq 0) "installeur (2e passage) : code $($i.Code)`n$($i.Sortie)"
  Verifier (Test-Path -LiteralPath $e.Exe) "l'installeur n'a pas copie l'exe"
  Verifier (Test-Path -LiteralPath (Join-Path $e.Menu "SpaceFiles.lnk")) "l'installeur n'a pas cree le raccourci"
  Verifier ((Get-ItemProperty -LiteralPath "$($e.Reg)\Microsoft\Windows\CurrentVersion\Run").SpaceFilesWinE -eq "`"$($e.Exe)`" --resident") "valeur Run SpaceFilesWinE incorrecte apres l'installation"
  Verifier ((Lire-Defaut "$($e.Reg)\Classes\spacefiles\shell\open\command") -eq "`"$($e.Exe)`" `"%1`"") "protocole spacefiles incorrect"
  Verifier (Test-Path -LiteralPath "$($e.Reg)\Classes\nothingexplorer\shell\open\command") "protocole nothingexplorer absent apres l'installation"
  Verifier (-not (Test-Path -LiteralPath "$($e.Reg)\Classes\Folder\shell\SpaceFiles")) "l'installeur a cree un verbe de dossier sans que l'utilisateur l'ait active"

  # activer() : Directory et Drive avec l'exe installe, Folder avec un exe ailleurs (build de developpement).
  # (Directory porte aussi un verbe « NothingExplorer » d'un autre programme : l'installeur ne met a jour un verbe existant
  # que dans la branche « pas d'ancien verbe » et ne le suit donc pas ; la mise a jour n'est testee que sur Folder.)
  Activer-Comme-Rust $e "Folder" (Join-Path $e.Ailleurs "spacefiles.exe")
  Activer-Comme-Rust $e "Directory" $e.Exe
  Activer-Comme-Rust $e "Drive" $e.Exe
  # Nouvelle mise a jour : l'installeur doit pointer les verbes existants vers l'exe installe
  $i = Lancer $installeur (Parametres $e @("-SansLancement"))
  Verifier ($i.Code -eq 0) "installeur (3e passage) : code $($i.Code)"
  Verifier ((Lire-Defaut "$($e.Reg)\Classes\Folder\shell\SpaceFiles\command") -eq "`"$($e.Exe)`" `"%1`"") "mise a jour : le verbe Folder ne vise pas l'exe installe"
  Poser-Donnees $e

  # L'exe est tenu : un processus lance depuis le dossier d'installation, un autre depuis « Ailleurs »
  Copy-Item $ping $e.Exe -Force
  $pidInstalle = Lancer-Ping $e.Exe
  $pidAilleurs = Lancer-Ping (Join-Path $e.Ailleurs "spacefiles.exe")
  Start-Sleep -Milliseconds 400
  $s1 = Photo $e
  Verifier (@(Compare-Object $s0 $s1).Count -gt 0) "l'installation n'a rien change dans l'etat photographie (essai sans valeur)"

  # -Simuler : etat identique avant et apres, processus vivants
  $r = Lancer $desinstalleur (Param-Des $e @("-Simuler"))
  Write-Host $r.Sortie
  Verifier ($r.Code -eq 0) "-Simuler : code $($r.Code)"
  Verifier ($r.Sortie -match 'SERAIT RETIR') "-Simuler n'a rien liste"
  Comparer "-Simuler change l'etat" $s1 (Photo $e)
  Verifier ($null -ne (Get-Process -Id $pidInstalle -ErrorAction SilentlyContinue)) "-Simuler a arrete le processus installe"

  # Desinstallation pour de vrai, sans -SupprimerDonnees
  $r = Lancer $desinstalleur (Param-Des $e @("-Oui"))
  Write-Host $r.Sortie
  Verifier ($r.Code -eq 0) "desinstallation : code $($r.Code)"
  Verifier ($null -eq (Get-Process -Id $pidInstalle -ErrorAction SilentlyContinue)) "le processus lance depuis le dossier d'installation tourne encore"
  Verifier ($null -ne (Get-Process -Id $pidAilleurs -ErrorAction SilentlyContinue)) "le spacefiles.exe d'ailleurs a ete arrete (ne doit jamais l'etre)"
  Verifier ($r.Sortie -match ', 0 \S+chec\(s\)') "un element est en echec dans le rapport"
  $s2 = Photo $e
  $donnees = "FIC Roaming\SpaceFiles", "FIC Local\SpaceFiles", "FIC Roaming\NothingExplorer"
  $horsDonnees = @($s2 | Where-Object { $l = $_; -not ($donnees | Where-Object { $l.StartsWith($_) }) })
  $avecDonnees = @($s2 | Where-Object { $l = $_; $donnees | Where-Object { $l.StartsWith($_) } })
  $donneesAvant = @($s1 | Where-Object { $l = $_; $donnees | Where-Object { $l.StartsWith($_) } })
  Comparer "apres desinstallation (hors donnees) : l'etat doit etre celui d'avant l'installation" $s0 $horsDonnees
  Comparer "les donnees de l'utilisateur doivent rester intactes" $donneesAvant $avecDonnees
  Verifier ($avecDonnees.Count -gt 0) "les donnees ont disparu sans -SupprimerDonnees"
  $retiresA = @($s1 | Where-Object { $s0 -notcontains $_ -and ($donneesAvant -notcontains $_) })
  $comptes.Retire = $retiresA.Count
  $comptes.Etranger = @($s0 | Where-Object { $s2 -contains $_ }).Count
  Verifier ($comptes.Etranger -eq $s0.Count) "des lignes etrangeres ont disparu"
  # Le verbe de la classe Folder avait « none » comme valeur d'avant : restauree ; Directory : aucune valeur ; Drive : « ExplorateurX »
  Verifier ((Lire-Defaut "$($e.Reg)\Classes\Folder\shell") -eq "none") "Folder\shell : valeur par defaut d'origine non restauree"
  Verifier ($null -eq (Lire-Defaut "$($e.Reg)\Classes\Directory\shell")) "Directory\shell : une valeur par defaut est restee alors qu'il n'y en avait pas"
  Verifier ((Lire-Defaut "$($e.Reg)\Classes\Drive\shell") -eq "ExplorateurX") "Drive\shell : valeur par defaut d'origine non restauree"

  # Deuxieme passage : « rien a retirer », code 0, etat inchange
  $r = Lancer $desinstalleur (Param-Des $e @("-Oui"))
  Write-Host $r.Sortie
  Verifier ($r.Code -eq 0) "2e desinstallation : code $($r.Code)"
  Verifier ($r.Sortie -match '(?i)rien\s+\S+\s+retirer') "2e desinstallation : « rien a retirer » absent"
  Comparer "2e desinstallation change l'etat" $s2 (Photo $e)

  # -SupprimerDonnees : les donnees, et seulement elles
  $r = Lancer $desinstalleur (Param-Des $e @("-Oui", "-SupprimerDonnees"))
  Write-Host $r.Sortie
  Verifier ($r.Code -eq 0) "-SupprimerDonnees : code $($r.Code)"
  Comparer "apres -SupprimerDonnees : l'etat complet doit etre celui d'avant l'installation" $s0 (Photo $e)
  Verifier ($null -ne (Get-Process -Id $pidAilleurs -ErrorAction SilentlyContinue)) "le spacefiles.exe d'ailleurs a ete arrete par -SupprimerDonnees"
  $r = Lancer $desinstalleur (Param-Des $e @("-Oui", "-SupprimerDonnees"))
  Verifier ($r.Code -eq 0 -and $r.Sortie -match '(?i)rien\s+\S+\s+retirer') "3e passage apres -SupprimerDonnees : pas « rien a retirer »"

  # ═════ Scenario B : anciens noms, protocole nothingexplorer absent, dossier d'installation absent ═════
  Write-Host "`n== Scenario B : anciens noms (NothingExplorer), dossier d'installation absent, protocole absent =="
  $b = Nouvel-Environnement
  Poser-Etranger $b
  # Raccourci « SpaceFiles.lnk » d'un autre programme : ne doit pas etre retire
  Nouveau-Lnk (Join-Path $b.Menu "SpaceFiles.lnk") (Join-Path $b.Ailleurs "autre.exe")
  $sb0 = Photo $b
  $ancienExe = Join-Path $b.Programs "NothingExplorer\nothing-explorer.exe"
  New-Item -ItemType Directory -Force (Split-Path $ancienExe) | Out-Null
  Copy-Item $ping $ancienExe
  $r = $b.Reg
  Valeur "$r\Microsoft\Windows\CurrentVersion\Run" "NothingExplorerWinE" "`"$ancienExe`" --resident"
  foreach ($c in "Folder", "Drive") {
    Defaut "$r\Classes\$c\shell\NothingExplorer" "Ouvrir dans Nothing Explorer"
    Defaut "$r\Classes\$c\shell\NothingExplorer\command" "`"$ancienExe`" `"%1`""
  }
  # Folder avait « none » avant : sauvegardee ; Drive n'avait rien... mais ici il avait « ExplorateurX » : sauvegarde aussi
  Valeur "$r\Classes\Folder\shell" "NothingExplorerAncienne" "none"
  Defaut "$r\Classes\Folder\shell" "NothingExplorer"
  Valeur "$r\Classes\Drive\shell" "NothingExplorerAncienne" "ExplorateurX"
  Defaut "$r\Classes\Drive\shell" "NothingExplorer"
  Defaut "$r\Classes\spacefiles" "URL:SpaceFiles"
  Valeur "$r\Classes\spacefiles" "URL Protocol" ""
  Defaut "$r\Classes\spacefiles\shell\open\command" "`"$($b.Exe)`" `"%1`""
  # (nothingexplorer:// volontairement absent)
  Nouveau-Lnk (Join-Path $b.Menu "Nothing Explorer.lnk") $ancienExe
  Poser-Donnees $b
  Verifier (-not (Test-Path -LiteralPath $b.Install)) "scenario B : le dossier d'installation devait etre absent"
  $r1 = Lancer $desinstalleur (Param-Des $b @("-Oui"))
  Write-Host $r1.Sortie
  Verifier ($r1.Code -eq 0) "B : code $($r1.Code)"
  Verifier ((Lire-Defaut "$($b.Reg)\Classes\Folder\shell") -eq "none") "B : Folder\shell non restaure depuis NothingExplorerAncienne"
  Verifier ((Lire-Defaut "$($b.Reg)\Classes\Drive\shell") -eq "ExplorateurX") "B : Drive\shell non restaure depuis NothingExplorerAncienne"
  Verifier ($null -ne (Lire-Defaut "$($b.Reg)\Classes\Directory\shell\NothingExplorer\command")) "B : le verbe NothingExplorer d'un autre programme a ete retire"
  Verifier ($r1.Sortie -match 'LAISS') "B : les elements etrangers laisses ne sont pas signales"
  # Sans -SupprimerDonnees : restent l'ancien dossier d'installation et les donnees
  $apres = Photo $b
  Verifier (Test-Path -LiteralPath $ancienExe) "B : l'ancien dossier Programs\NothingExplorer a ete retire sans -SupprimerDonnees"
  Verifier (Test-Path -LiteralPath (Join-Path $b.Roaming "NothingExplorer")) "B : les anciennes donnees ont ete retirees sans -SupprimerDonnees"
  Verifier (-not (Test-Path -LiteralPath (Join-Path $b.Menu "Nothing Explorer.lnk"))) "B : « Nothing Explorer.lnk » est reste"
  Verifier (Test-Path -LiteralPath (Join-Path $b.Menu "SpaceFiles.lnk")) "B : le SpaceFiles.lnk d'un autre programme a ete retire"
  Verifier (@(Compare-Object (Photo-Registre $b) @($sb0 | Where-Object { $_ -like 'REG *' })).Count -eq 0) "B : le registre n'est pas revenu a l'etat etranger"
  $r1 = Lancer $desinstalleur (Param-Des $b @("-Oui", "-SupprimerDonnees"))
  Verifier ($r1.Code -eq 0) "B (-SupprimerDonnees) : code $($r1.Code)"
  Comparer "B apres -SupprimerDonnees : l'etat complet doit etre l'etat etranger de depart" $sb0 (Photo $b)

  # ═════ Scenario C : dossier d'installation avec un fichier qui n'est pas a nous ═════
  Write-Host "`n== Scenario C : le dossier d'installation contient un fichier etranger =="
  $c = Nouvel-Environnement
  New-Item -ItemType Directory -Force $c.Install | Out-Null
  Copy-Item $ping $c.Exe
  Set-Content -LiteralPath (Join-Path $c.Install "mes notes é.txt") -Value "notes" -Encoding UTF8
  Set-Content -LiteralPath (Join-Path $c.Install "spacefiles-abcd1234.ico") -Value "ico" -Encoding UTF8
  $pidC = Lancer-Ping $c.Exe
  Start-Sleep -Milliseconds 400
  $r2 = Lancer $desinstalleur (Param-Des $c @("-Oui"))
  Write-Host $r2.Sortie
  Verifier ($r2.Code -eq 0) "C : code $($r2.Code)"
  Verifier ($null -eq (Get-Process -Id $pidC -ErrorAction SilentlyContinue)) "C : le processus du dossier d'installation tourne encore"
  Verifier (-not (Test-Path -LiteralPath $c.Exe)) "C : l'exe est reste"
  Verifier (Test-Path -LiteralPath (Join-Path $c.Install "mes notes é.txt")) "C : le fichier etranger a ete retire"
  Verifier ($r2.Sortie -match 'CONSERV') "C : le dossier conserve n'est pas signale"

  # ═════ Scenario D : confirmation refusee (session non interactive, sans -Oui) ═════
  Write-Host "`n== Scenario D : sans -Oui ni terminal, rien n'est retire =="
  $d = Nouvel-Environnement
  Poser-Etranger $d
  [void](Lancer $installeur (Parametres $d @("-SansLancement")))
  $sd = Photo $d
  $r3 = Lancer $desinstalleur (Param-Des $d @())
  Write-Host $r3.Sortie
  Verifier ($r3.Code -eq 2) "D : code $($r3.Code) au lieu de 2 (annule faute de confirmation)"
  Comparer "D : une desinstallation annulee change l'etat" $sd (Photo $d)
}
catch {
  [void]$ecarts.Add("exception : $($_.Exception.Message) ($($_.InvocationInfo.ScriptLineNumber))")
  Write-Host "EXCEPTION : $($_.Exception.Message) (ligne $($_.InvocationInfo.ScriptLineNumber))"
}
finally {
  # Seuls les processus de l'essai (par numero) et les dossiers / cles de l'essai sont retires
  foreach ($p in $processusEssai) { Stop-Process -Id $p -Force -ErrorAction SilentlyContinue }
  Start-Sleep -Milliseconds 400
  foreach ($e in $aNettoyer) {
    if ($e.Reg -like 'HKCU:\Software\SpaceFilesTest-*' -and (Test-Path -LiteralPath $e.Reg)) { Remove-Item -LiteralPath $e.Reg -Recurse -Force }
    if ($e.Base -like "$env:TEMP\SpaceFiles essai *" -and (Test-Path -LiteralPath $e.Base)) { Remove-Item -LiteralPath $e.Base -Recurse -Force -ErrorAction SilentlyContinue }
  }
}

$auteurApres = @(Lire-Processus-SpaceFiles)
Write-Host "spacefiles.exe presents apres l'essai : $(if ($auteurApres.Count) { $auteurApres -join ' ; ' } else { 'aucun' })"
Verifier (@(Compare-Object $auteurAvant $auteurApres).Count -eq 0) "la liste des spacefiles.exe a change entre le debut et la fin de l'essai"
$restes = @(Get-ChildItem HKCU:\Software | Where-Object { $_.PSChildName -like 'SpaceFilesTest-*' })
Verifier ($restes.Count -eq 0) "des cles de test restent dans HKCU : $($restes.Name -join ', ')"

if ($ecarts.Count -eq 0) {
  Write-Host "`nOK : $($comptes.Verifs) verifications, $($comptes.Retire) lignes d'etat (registre + fichiers) retirees par le desinstalleur, $($comptes.Etranger) lignes etrangeres inchangees (scenario A)."
  exit 0
}
Write-Host "`nECARTS ($($ecarts.Count)) :"
$ecarts | ForEach-Object { Write-Host " - $_" }
exit 1
