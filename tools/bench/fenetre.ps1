# Mesure la fenêtre de SpaceFiles sur des dossiers de N fichiers (faux profil, copie de l'exe, sans souris ni focus) :
#  1. mémoire : -EssaisMemoire lancements par taille, -Repos secondes de repos après l'ouverture, octets privés et working set ;
#  2. défilement : un lancement par taille et par vue (liste, grille) avec SPACEFILES_MESURE_DEFILEMENT=pas[,grille] ; la fenêtre
#     descend toute seule de haut en bas et écrit au journal du faux profil la ligne « MESURE ... » (images, temps de
#     Rendu::dessiner, intervalles entre images, temps de lecture du dossier). Le pas est de N/50 DIP par image.
# Les dossiers de test (fichiers vides, noms variés) sont créés une fois dans %TEMP%\ne-bench-dossiers et réutilisés.
# Ne ferme que les processus lancés ici (par identifiant). Aucune entrée souris ni clavier.
param(
    [string]$Exe = (Join-Path $PSScriptRoot '..\..\target\release\spacefiles.exe'),
    [int[]]$Tailles = @(1000, 10000, 100000),
    [int]$EssaisMemoire = 3,
    [int]$Repos = 30,
    [switch]$SansDefilement
)
$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'commun.ps1')
$Exe = (Resolve-Path $Exe).Path
$racine = Join-Path $env:TEMP "ne-bench-fenetre-$PID"
New-Item -ItemType Directory -Force $racine | Out-Null
# Bureau Windows à part : aucune fenêtre devant l'utilisateur, aucun focus pris (voir commun.ps1). DWM ne le compose pas.
Ouvrir-Bureau
$copie = Join-Path $racine 'spacefiles.exe'; Copy-Item $Exe $copie
$base = Join-Path $env:TEMP 'ne-bench-dossiers'
$dossiers = @{}
foreach ($n in $Tailles) {
    $d = Join-Path $base "d$n"
    $existant = if (Test-Path $d) { (Get-ChildItem $d -Force | Measure-Object).Count } else { -1 }
    if ($existant -ne $n) { "création du dossier de $n éléments..."; $existant = Creer-DossierTest $d $n }
    "dossier $d : $existant éléments"
    $dossiers[$n] = $d
}
function Nouveau-Profil([string]$nom) { Poser-FauxProfil (Join-Path $racine $nom) }
function Fermer($p) { if ($p -and -not $p.HasExited) { Stop-Process -Id $p.Id -Force } }

"== Mémoire (repos $Repos s après le lancement, $EssaisMemoire essais par taille)"
$memoire = @()
if ($true) {
    foreach ($n in $Tailles) {
        for ($i = 1; $i -le $EssaisMemoire; $i++) {
            $null = Nouveau-Profil "m$n-$i"
            $p = Lancer-SansFocus $copie "`"$($dossiers[$n])`""
            try {
                Start-Sleep -Seconds $Repos
                $p.Refresh()
                $l = [pscustomobject]@{ N = $n; Essai = $i; PriveMo = [math]::Round($p.PrivateMemorySize64 / 1MB, 2); WsMo = [math]::Round($p.WorkingSet64 / 1MB, 2); Fils = $p.Threads.Count; Handles = $p.HandleCount }
                $memoire += $l
                "N={0} essai {1} : privé {2} Mo, WS {3} Mo, {4} fils, {5} handles" -f $l.N, $l.Essai, $l.PriveMo, $l.WsMo, $l.Fils, $l.Handles
            } finally { Fermer $p }
            Start-Sleep -Seconds 1
        }
        "N=$n : privé " + (Texte-Resume (Resume-Valeurs ($memoire | ? N -eq $n | % PriveMo)) ' Mo') + " ; WS " + (Texte-Resume (Resume-Valeurs ($memoire | ? N -eq $n | % WsMo)) ' Mo')
    }
}

if (-not $SansDefilement) {
    "== Défilement (pas = N/50 DIP par image)"
    foreach ($n in $Tailles) {
        foreach ($vue in 'liste', 'grille') {
            $profil = Nouveau-Profil "d$n-$vue"
            $pas = [math]::Max(1, [int]($n / 50))
            $env:SPACEFILES_MESURE_DEFILEMENT = if ($vue -eq 'grille') { "$pas,grille" } else { "$pas" }
            $p = Lancer-SansFocus $copie "`"$($dossiers[$n])`""
            Remove-Item Env:\SPACEFILES_MESURE_DEFILEMENT
            try {
                $ligne = Attendre-Journal $profil.Journal 'defilement MESURE' 240000
                if ($ligne) { "N=$n $vue : $ligne" } else { "N=$n $vue : pas de ligne MESURE en 240 s (échec)" }
                Start-Sleep -Seconds 3; $p.Refresh()
                "   après le défilement : privé {0:N2} Mo, WS {1:N2} Mo" -f ($p.PrivateMemorySize64 / 1MB), ($p.WorkingSet64 / 1MB)
            } finally { Fermer $p }
            Start-Sleep -Seconds 1
        }
    }
}
Fermer-Bureau
Remove-Item $racine -Recurse -Force -ErrorAction SilentlyContinue
