# Calcule l'empreinte SHA-256 de l'exécutable release, à joindre à chaque version publiée :
# elle permet de vérifier qu'un exe téléchargé est bien celui qui a été compilé (le binaire n'est pas signé).
param([string]$Exe = (Join-Path (Split-Path $PSScriptRoot -Parent | Split-Path -Parent) 'target/release/spacefiles.exe'))
if (-not (Test-Path $Exe)) { Write-Error "Introuvable : $Exe (lancer d'abord cargo build --release)"; exit 1 }
$h = Get-FileHash -Algorithm SHA256 -LiteralPath $Exe
$octets = (Get-Item -LiteralPath $Exe).Length
"{0}  spacefiles.exe  ({1:N0} octets)" -f $h.Hash.ToLower(), $octets
