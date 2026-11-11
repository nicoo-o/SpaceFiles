# Mesure le lancement de `spacefiles.exe` (fenêtre normale) : de la création du processus à l'apparition de la fenêtre
# « SpaceFiles » (classe SpaceFiles) du processus lancé ici, attendue par sondage toutes les 5 ms (pas à l'œil).
# Cela mesure l'apparition de la fenêtre (ShowWindow), PAS la première image dessinée : le programme n'écrit pas ce moment.
# Faux profil (APPDATA / LOCALAPPDATA redirigés, SPACEFILES_SANS_REGISTRE=1) : la fenêtre lit et écrit ses réglages à chaque lancement.
# Dossier ouvert : un petit dossier de test (-Dossier), pour que la mesure soit celle du démarrage et pas celle d'une lecture.
# « Froid » = premier lancement d'une copie de l'exe qu'on vient de faire (un nouveau fichier par essai) ; le PC n'est pas
# redémarré : ce n'est pas un vrai démarrage à froid du disque. « Chaud » = relances de la même copie.
# Sans voler le focus : la fenêtre vit sur un bureau Windows à part (CreateDesktop), jamais devant l'utilisateur (SW_SHOWNOACTIVATE seul
# ne suffisait pas : le premier plan passait à SpaceFiles). Conséquence : DWM ne compose pas ce bureau ; la mesure est celle de la création
# et de l'affichage de la fenêtre, sans la composition. Le script note si le premier plan du bureau de l'utilisateur est devenu la fenêtre lancée.
# Ne ferme que le processus lancé ici (par identifiant). Aucune entrée souris ni clavier.
param([string]$Exe = (Join-Path $PSScriptRoot '..\..\target\release\spacefiles.exe'), [int]$Froid = 10, [int]$Chaud = 10, [string]$Dossier = '')
$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'commun.ps1')
$Exe = (Resolve-Path $Exe).Path
$racine = Join-Path $env:TEMP "ne-bench-lancement-$PID"
$null = Poser-FauxProfil $racine
Ouvrir-Bureau
if (-not $Dossier) { $Dossier = Join-Path $racine 'petit'; $null = Creer-DossierTest $Dossier 20 }

function Un-Lancement([string]$exe) {
    $avant = [NeLance]::PidAuPremierPlan()
    $sw = [Diagnostics.Stopwatch]::StartNew()
    $p = Lancer-SansFocus $exe "`"$Dossier`""
    $ms = $null; $vole = $false
    while ($sw.ElapsedMilliseconds -lt 10000) {
        # Le pid doit être le nôtre : une fenêtre SpaceFiles déjà ouverte par ailleurs ne compte pas (elle n'est pas sur ce bureau)
        if ([NeLance]::TrouverFenetre('SpaceFiles', [uint32]$p.Id) -ne 0) { $ms = $sw.ElapsedMilliseconds; break }
        Start-Sleep -Milliseconds 5
    }
    Start-Sleep -Milliseconds 300
    if ([NeLance]::PidAuPremierPlan() -eq $p.Id -and $avant -ne $p.Id) { $vole = $true }
    if (-not $p.HasExited) { Stop-Process -Id $p.Id -Force }
    Start-Sleep -Milliseconds 700
    [pscustomobject]@{ Ms = $ms; FocusPris = $vole }
}

$froids = @(); $vols = 0
for ($i = 1; $i -le $Froid; $i++) {
    $copie = Join-Path $racine "froid$i.exe"
    Copy-Item $Exe $copie
    $r = Un-Lancement $copie
    $froids += $r.Ms; if ($r.FocusPris) { $vols++ }
    "froid $i : $(if ($r.Ms -ne $null) { "$($r.Ms) ms" } else { 'fenêtre non apparue en 10 s' })"
}
$copie = Join-Path $racine 'chaud.exe'; Copy-Item $Exe $copie
$null = Un-Lancement $copie   # premier lancement de cette copie : non compté
$chauds = @()
for ($i = 1; $i -le $Chaud; $i++) {
    $r = Un-Lancement $copie
    $chauds += $r.Ms; if ($r.FocusPris) { $vols++ }
    "chaud $i : $(if ($r.Ms -ne $null) { "$($r.Ms) ms" } else { 'fenêtre non apparue en 10 s' })"
}
"froid : " + (Texte-Resume (Resume-Valeurs $froids) ' ms')
"chaud : " + (Texte-Resume (Resume-Valeurs $chauds) ' ms')
"lancements où la fenêtre lancée est passée au premier plan : $vols sur $($Froid + $Chaud)"
Fermer-Bureau
Remove-Item $racine -Recurse -Force -ErrorAction SilentlyContinue
