# Banc de latence des requêtes d'index + coût mémoire du chargement de index.bin (phase 3, E-1).
# Travaille dans une COPIE du projet sous %TEMP% (src/ du dépôt n'est jamais modifié) : y ajoute le test #[ignore]
# `bench_requetes_e1` (tools/bench/index-bench-e1-test.rs.txt), puis le lance avec LOCALAPPDATA / APPDATA redirigés
# vers un faux profil sous %TEMP% (le processus lancé seulement) et SPACEFILES_SANS_REGISTRE=1. Le faux profil
# reçoit une COPIE de index.bin lue dans le vrai dossier (lecture seule : le vrai fichier n'est ni modifié ni ouvert
# en écriture). Ne touche ni au résident de l'auteur ni à ses réglages.
#
# Usage : powershell -NoProfile -ExecutionPolicy Bypass -File tools\bench\index-bench-e1.ps1
param([string]$IndexSource = (Join-Path $env:LOCALAPPDATA 'SpaceFiles\index.bin'))
$ErrorActionPreference = 'Stop'
$env:Path += ";$env:USERPROFILE\.cargo\bin"

$depot = (Resolve-Path (Join-Path $PSScriptRoot '..\..')).Path
$racine = Join-Path $env:TEMP 'ne-bench-e1'
$copie = Join-Path $racine 'projet'
$profil = Join-Path $racine 'faux-profil'

New-Item -ItemType Directory -Force $copie, (Join-Path $profil 'Local\SpaceFiles'), (Join-Path $profil 'Roaming\SpaceFiles') | Out-Null
foreach ($f in 'Cargo.toml', 'Cargo.lock', 'build.rs') { if (Test-Path (Join-Path $depot $f)) { Copy-Item (Join-Path $depot $f) $copie -Force } }
foreach ($d in 'src', 'assets', 'resources') { if (Test-Path (Join-Path $depot $d)) { Copy-Item (Join-Path $depot $d) $copie -Recurse -Force } }
Copy-Item $IndexSource (Join-Path $profil 'Local\SpaceFiles\index.bin') -Force

# Le test est ajouté avant l'accolade finale de `mod tests` (dernière ligne non vide de src/index.rs).
$idx = Join-Path $copie 'src\index.rs'
$texte = [IO.File]::ReadAllText($idx, [Text.Encoding]::UTF8).TrimEnd()
if (-not $texte.EndsWith('}')) { throw 'src/index.rs ne finit pas par }' }
$test = [IO.File]::ReadAllText((Join-Path $PSScriptRoot 'index-bench-e1-test.rs.txt'), [Text.Encoding]::UTF8)
[IO.File]::WriteAllText($idx, $texte.Substring(0, $texte.Length - 1) + $test + "}`r`n", (New-Object Text.UTF8Encoding($false)))

# Construction SANS redirection (cargo et l'éditeur de liens ont besoin du vrai profil), puis exécution du binaire
# de test avec la redirection : seul ce processus voit le faux profil.
$env:CARGO_TARGET_DIR = Join-Path $env:TEMP 'ne-bench-target'
Push-Location $copie
try {
    $ErrorActionPreference = 'Continue'  # cargo écrit sa progression sur stderr
    $sortie = & cargo test --release --no-run 2>&1 | ForEach-Object { "$_" }
    $ErrorActionPreference = 'Stop'
    $sortie | Select-Object -Last 5
    $ligne = $sortie | Where-Object { $_ -match 'Executable unittests.*\((.+\.exe)\)' } | Select-Object -First 1
    if (-not $ligne -or $ligne -notmatch '\((.+\.exe)\)') { throw "binaire de test introuvable" }
    $exe = $Matches[1]
    if (-not [IO.Path]::IsPathRooted($exe)) { $exe = Join-Path $copie $exe }
} finally { Pop-Location }

$env:LOCALAPPDATA = Join-Path $profil 'Local'
$env:APPDATA = Join-Path $profil 'Roaming'
$env:SPACEFILES_SANS_REGISTRE = '1'
& $exe bench_requetes_e1 --ignored --nocapture --test-threads=1 | Out-Host
