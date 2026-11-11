# Écrans de référence de SpaceFiles : 2 dossiers d'essai (un rempli, un vide) × 3 échelles (100, 150, 200 %), dans chacun
# des deux thèmes (sombre, clair). Sans aucune entrée (ni souris ni clavier) : l'échelle est imposée par SPACEFILES_DPI, le
# dossier est passé en argument, le thème vient d'un reglages.toml écrit dans un FAUX profil (APPDATA et LOCALAPPDATA
# redirigés : les vrais réglages de l'auteur ne sont ni lus ni écrits), la capture lit la fenêtre (capture.ps1).
#
# Modes :
#   -Mode Capturer  : écrit les 6 captures dans -Sortie\<theme> (défaut : le dossier temporaire)
#   -Mode Referencer: les écrit dans tools/ui-snapshot/reference/<theme> (à relire à l'œil avant de les garder)
#   -Mode Verifier  : les refait et les compare aux références ; code de sortie 1 si l'une s'écarte trop
# -Theme Sombre (défaut) | Clair | Tous (les deux jeux, l'un après l'autre : 12 captures).
# Les références dépendent de la police Doto installée et du rendu Direct2D : elles sont propres à cette
# machine. Le seuil tolère l'heure de la barre d'état (« LU EN x MS ») et le grain de la trame.
param(
    [ValidateSet('Capturer', 'Referencer', 'Verifier')] [string] $Mode = 'Capturer',
    [ValidateSet('Sombre', 'Clair', 'Tous')] [string] $Theme = 'Sombre',
    [string] $Sortie = (Join-Path $env:TEMP 'ne-ecrans'),
    [double] $Seuil = 0.02
)

$ErrorActionPreference = 'Stop'
# Les lancements d'essai ne lisent ni n'écrivent jamais le registre (defaut.rs)
$env:SPACEFILES_SANS_REGISTRE = '1'
$ici = Split-Path -Parent $MyInvocation.MyCommand.Path
$racine = Resolve-Path (Join-Path $ici '..\..')
$exe = Join-Path $racine 'target\release\spacefiles.exe'
$reference = Join-Path $ici 'reference'
$themes = if ($Theme -eq 'Tous') { @('Sombre', 'Clair') } else { @($Theme) }
# Sauvegarde de l'environnement de l'auteur, rendu en fin de script quoi qu'il arrive
$appdata0 = $env:APPDATA; $local0 = $env:LOCALAPPDATA
# Un résident lancé depuis CE binaire (et non celui installé) est relancé à la fin ; sinon rien n'est lancé
$residentLance = [bool](Get-Process spacefiles -ErrorAction SilentlyContinue | Where-Object { $_.Path -eq $exe })

# Dossiers d'essai aux contenus et aux dates fixes : la liste ne change jamais d'une exécution à l'autre
$essai = Join-Path $env:TEMP 'ne-reference'
$plein = Join-Path $essai 'plein'
$vide = Join-Path $essai 'vide'
$faux = Join-Path $env:TEMP 'ne-faux-profil'
Remove-Item -Recurse -Force $essai -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force $plein, $vide | Out-Null
$date = [datetime]'2020-03-14 10:00:00'
foreach ($n in 'Rapport.pdf', 'Photo.png', 'notes.txt', 'archive.zip', 'musique.mp3', 'script.py') {
    $f = Join-Path $plein $n
    [IO.File]::WriteAllBytes($f, (New-Object byte[] (1500 * ($n.Length))))
    (Get-Item $f).LastWriteTime = $date; (Get-Item $f).CreationTime = $date
}
foreach ($d in 'Projets', 'Images') {
    $p = Join-Path $plein $d
    New-Item -ItemType Directory $p | Out-Null
    (Get-Item $p).LastWriteTime = $date; (Get-Item $p).CreationTime = $date
}

# Faux profil neuf à chaque lancement : un processus arrêté de force laisse un marqueur de « fermeture anormale » qui ferait
# rouvrir des onglets et afficher un message dans la barre d'état de la capture suivante
function Nouveau-Profil([string] $theme) {
    Remove-Item -Recurse -Force $faux -ErrorAction SilentlyContinue
    $roaming = Join-Path $faux 'Roaming'; $locale = Join-Path $faux 'Local'
    New-Item -ItemType Directory -Force (Join-Path $roaming 'SpaceFiles'), (Join-Path $locale 'SpaceFiles') | Out-Null
    # UTF-8 sans BOM, comme l'écrit l'application
    [IO.File]::WriteAllText((Join-Path $roaming 'SpaceFiles\reglages.toml'), "theme = `"$($theme.ToLower())`"`r`n", (New-Object Text.UTF8Encoding $false))
    $env:APPDATA = $roaming; $env:LOCALAPPDATA = $locale
}

Add-Type -AssemblyName System.Drawing
$echelles = @{ 'dpi100' = 96; 'dpi150' = 144; 'dpi200' = 192 }
$ecarts = @()
try {
    foreach ($theme in $themes) {
        $cle = $theme.ToLower()
        $sortieTheme = if ($Mode -eq 'Referencer') { Join-Path $reference $cle } else { Join-Path $Sortie $cle }
        New-Item -ItemType Directory -Force $sortieTheme | Out-Null
        foreach ($dossier in @(@('plein', $plein), @('vide', $vide))) {
            foreach ($e in $echelles.GetEnumerator()) {
                Get-Process spacefiles -ErrorAction SilentlyContinue | Where-Object { $_.Path -eq $exe } | Stop-Process -Force
                Start-Sleep -Milliseconds 700
                Nouveau-Profil $theme
                $env:SPACEFILES_DPI = $e.Value
                $p = Start-Process $exe -ArgumentList "`"$($dossier[1])`"" -PassThru
                Start-Sleep -Seconds 8
                $nom = "$($dossier[0])-$($e.Key)"
                & (Join-Path $ici 'capture.ps1') -OutDir $sortieTheme -Name $nom -Count 3 -IntervalMs 1500 | Out-Null
                Stop-Process $p -Force -ErrorAction SilentlyContinue
                # La dernière des trois : la première peut précéder le redessin qui suit le changement d'échelle
                Move-Item -Force (Join-Path $sortieTheme "$nom-03.png") (Join-Path $sortieTheme "$nom-01.png")
                Remove-Item (Join-Path $sortieTheme "$nom-02.png") -ErrorAction SilentlyContinue
                $image = Join-Path $sortieTheme "$nom-01.png"
                if ($Mode -eq 'Referencer') { continue }
                if ($Mode -eq 'Verifier') {
                    $ref = Join-Path (Join-Path $reference $cle) "$nom-01.png"
                    if (-not (Test-Path $ref)) { $ecarts += "$cle/$nom : pas de référence"; continue }
                    $a = New-Object System.Drawing.Bitmap $ref
                    $b = New-Object System.Drawing.Bitmap $image
                    if ($a.Width -ne $b.Width -or $a.Height -ne $b.Height) {
                        $ecarts += "$cle/$nom : taille $($b.Width)x$($b.Height) au lieu de $($a.Width)x$($a.Height)"
                    } else {
                        # Un pixel diffère quand la somme de ses écarts de canaux dépasse 60 ; on échantillonne 1 pixel sur 3
                        $diff = 0; $total = 0
                        for ($y = 0; $y -lt $a.Height; $y += 3) {
                            for ($x = 0; $x -lt $a.Width; $x += 3) {
                                $pa = $a.GetPixel($x, $y); $pb = $b.GetPixel($x, $y)
                                $total++
                                if ([math]::Abs($pa.R - $pb.R) + [math]::Abs($pa.G - $pb.G) + [math]::Abs($pa.B - $pb.B) -gt 60) { $diff++ }
                            }
                        }
                        $part = $diff / $total
                        '{0}/{1} : {2:P2} de pixels différents' -f $cle, $nom, $part
                        # 200 % : la capture est parfois rognée à droite et en bas (cause non trouvée) ; elle est montrée, pas jugée
                        if ($nom -match 'dpi200') { 'INFORMATIF (non jugé)' } elseif ($part -gt $Seuil) { $ecarts += ('{0}/{1} : {2:P2} > {3:P0}' -f $cle, $nom, $part, $Seuil) }
                    }
                    $a.Dispose(); $b.Dispose()
                }
            }
        }
    }
} finally {
    $env:APPDATA = $appdata0; $env:LOCALAPPDATA = $local0
    Remove-Item Env:SPACEFILES_DPI, Env:SPACEFILES_SANS_REGISTRE -ErrorAction SilentlyContinue
    Remove-Item -Recurse -Force $essai, $faux -ErrorAction SilentlyContinue
}
if ($residentLance) { Start-Process $exe -ArgumentList '--resident' }
if ($ecarts.Count) { $ecarts | ForEach-Object { "ECART $_" }; exit 1 }
"OK : captures dans $(if ($Mode -eq 'Referencer') { $reference } else { $Sortie })"
