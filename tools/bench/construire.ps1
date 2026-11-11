# Mesure la durée de `cargo build --release` : construction complète du crate, puis reconstruction sans changement.
# Utilise un dossier cible à part (par défaut %TEMP%\ne-bench-target) pour ne pas toucher à target\release, dont
# l'exe est tenu par un résident qui tourne (LNK1104) ; ne supprime rien dans target\.
param([int]$Essais = 3, [string]$Cible = (Join-Path $env:TEMP 'ne-bench-target'))
$ErrorActionPreference = 'Stop'
$env:Path += ";$env:USERPROFILE\.cargo\bin"
$env:CARGO_TARGET_DIR = $Cible
Set-Location (Join-Path $PSScriptRoot '..\..')
function Duree($bloc) { $sw = [Diagnostics.Stopwatch]::StartNew(); & $bloc | Out-Null; if ($LASTEXITCODE -ne 0) { throw 'cargo build a échoué' }; $sw.Stop(); [math]::Round($sw.Elapsed.TotalSeconds, 1) }
if (-not (Test-Path (Join-Path $Cible 'release\spacefiles.exe'))) {
    "Première construction (dépendances comprises) : {0} s" -f (Duree { cmd /c 'cargo build --release >nul 2>&1' })
}
$complets = @(); $vides = @()
for ($i = 1; $i -le $Essais; $i++) {
    cmd /c 'cargo clean -p spacefiles --release >nul 2>&1'
    $complets += Duree { cmd /c 'cargo build --release >nul 2>&1' }
    $vides += Duree { cmd /c 'cargo build --release >nul 2>&1' }
}
function Resume($v) { $t = $v | Sort-Object; "médiane {0} s (min {1}, max {2}) sur {3} essais : {4}" -f $t[[int][math]::Floor($t.Count / 2)], $t[0], $t[-1], $t.Count, ($v -join ' ') }
"Construction complète du crate (cargo clean -p spacefiles --release, dépendances en cache) : " + (Resume $complets)
"Reconstruction sans changement : " + (Resume $vides)
"Taille de l'exe : {0} octets" -f (Get-Item (Join-Path $Cible 'release\spacefiles.exe')).Length