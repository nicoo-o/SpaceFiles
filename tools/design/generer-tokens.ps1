# Génère les jetons de design depuis design/tokens.json : Tokens.generated.xaml (SpaceNotch, Hub) et tokens.rs (Explorateur).
# Sortie dans -Sortie (défaut design/genere) ; -Mode Verifier compare avec le contenu déjà écrit et sort en code 1 s'il diffère.
# Le Rust généré n'est branché nulle part dans src/ : l'application des jetons à l'Explorateur est la phase 7.
param(
    [ValidateSet('Ecrire', 'Verifier')] [string] $Mode = 'Ecrire',
    [string] $Sortie = (Join-Path $PSScriptRoot '..\..\design\genere')
)
$ErrorActionPreference = 'Stop'
$t = Get-Content -Raw -Encoding UTF8 (Join-Path $PSScriptRoot '..\..\design\tokens.json') | ConvertFrom-Json
$inv = [Globalization.CultureInfo]::InvariantCulture
function Pascal($s) { (($s -split '-') | ForEach-Object { $_.Substring(0, 1).ToUpper() + $_.Substring(1) }) -join '' }
function Snake($s) { ($s -replace '-', '_').ToUpper() }
function Nombre($v) { ([double]$v).ToString($inv) }
function Flottant($v) { $s = Nombre $v; if ($s.Contains('.')) { $s } else { "$s.0" } }
function Membres($o) { $o.PSObject.Properties }

$x = New-Object Text.StringBuilder
$r = New-Object Text.StringBuilder
[void]$x.AppendLine('<!-- Généré par tools/design/generer-tokens.ps1 depuis design/tokens.json : ne pas modifier à la main. -->')
[void]$x.AppendLine('<ResourceDictionary xmlns="http://schemas.microsoft.com/winfx/2006/xaml/presentation" xmlns:x="http://schemas.microsoft.com/winfx/2006/xaml">')
[void]$r.AppendLine('// Généré par tools/design/generer-tokens.ps1 depuis design/tokens.json : ne pas modifier à la main.')
[void]$r.AppendLine('// Couleurs en 0xAARRGGBB, longueurs en DIP, durées en ms.')
[void]$r.AppendLine('#![allow(dead_code)]')
# Couleurs : le thème sombre est le dictionnaire par défaut (Default) et garde les noms COULEUR_* ; le clair est dans
# le dictionnaire Light et prend COULEUR_CLAIR_* côté Rust.
[void]$x.AppendLine('  <ResourceDictionary.ThemeDictionaries>')
foreach ($theme in @(@('dark', 'Default', 'COULEUR_'), @('light', 'Light', 'COULEUR_CLAIR_'))) {
    [void]$x.AppendLine("    <ResourceDictionary x:Key=`"$($theme[1])`">")
    foreach ($m in Membres $t.color.($theme[0])) {
        [void]$x.AppendLine("      <Color x:Key=`"EcoColor$(Pascal $m.Name)`">$($m.Value)</Color>")
        [void]$r.AppendLine("pub const $($theme[2])$(Snake $m.Name): u32 = 0x$($m.Value.Substring(1));")
    }
    [void]$x.AppendLine('    </ResourceDictionary>')
}
[void]$x.AppendLine('  </ResourceDictionary.ThemeDictionaries>')
foreach ($groupe in @(@('radius', 'Radius', 'RAYON'), @('spacing', 'Spacing', 'ESPACE'))) {
    foreach ($m in Membres $t.($groupe[0])) {
        [void]$x.AppendLine("  <x:Double x:Key=`"Eco$($groupe[1])$(Pascal $m.Name)`">$(Nombre $m.Value)</x:Double>")
        [void]$r.AppendLine("pub const $($groupe[2])_$(Snake $m.Name): f32 = $(Flottant $m.Value);")
    }
}
foreach ($m in Membres $t.type.family) {
    [void]$x.AppendLine("  <FontFamily x:Key=`"EcoFont$(Pascal $m.Name)`">$($m.Value)</FontFamily>")
    [void]$r.AppendLine("pub const POLICE_$(Snake $m.Name): &str = `"$($m.Value)`";")
}
foreach ($m in Membres $t.type.size) {
    [void]$x.AppendLine("  <x:Double x:Key=`"EcoFontSize$(Pascal $m.Name)`">$(Nombre $m.Value)</x:Double>")
    [void]$r.AppendLine("pub const TAILLE_$(Snake $m.Name): f32 = $(Flottant $m.Value);")
}
foreach ($m in Membres $t.motion.'fade-ms') {
    [void]$x.AppendLine("  <x:Double x:Key=`"EcoFadeMs$(Pascal $m.Name)`">$(Nombre $m.Value)</x:Double>")
    [void]$r.AppendLine("pub const FONDU_MS_$(Snake $m.Name): u32 = $(Nombre $m.Value);")
}
foreach ($m in Membres $t.motion.spring) {
    [void]$x.AppendLine("  <x:Double x:Key=`"EcoSpring$(Pascal $m.Name)Response`">$(Nombre $m.Value.response)</x:Double>")
    [void]$x.AppendLine("  <x:Double x:Key=`"EcoSpring$(Pascal $m.Name)Damping`">$(Nombre $m.Value.damping)</x:Double>")
    [void]$r.AppendLine("pub const RESSORT_$(Snake $m.Name): (f32, f32) = ($(Flottant $m.Value.response), $(Flottant $m.Value.damping));")
}
[void]$x.AppendLine('</ResourceDictionary>')

$fichiers = @{ 'Tokens.generated.xaml' = $x.ToString(); 'tokens.rs' = $r.ToString() }
New-Item -ItemType Directory -Force $Sortie | Out-Null
$ecarts = 0
foreach ($f in $fichiers.GetEnumerator()) {
    $p = Join-Path $Sortie $f.Key
    $texte = $f.Value -replace "`r`n", "`n"
    if ($Mode -eq 'Verifier') {
        if (-not (Test-Path $p) -or ((Get-Content -Raw -Encoding UTF8 $p) -replace "`r`n", "`n") -ne $texte) { "ECART $($f.Key)"; $ecarts++ }
    } else {
        [IO.File]::WriteAllText($p, $texte, (New-Object Text.UTF8Encoding($false)))
    }
}
if ($ecarts) { exit 1 }
if ($Mode -eq 'Verifier') { 'OK : sortie identique aux jetons' } else { "Écrit dans $Sortie" }
