# Test de durée du résident : échantillonne la mémoire privée, le working set, les handles, les fils et le CPU d'un résident
# de test toutes les -Intervalle secondes pendant -Duree minutes, écrit un CSV et résume (début, fin, maximum, pente en Mo/h).
# Résident de test : propre copie de l'exe, faux profil, mode essai (SPACEFILES_PIPE : tube, verrou et classe à part, pas de
# crochet clavier), arrêté par son identifiant à la fin. Ne touche ni au résident de l'auteur ni à ses données : l'index de
# recherche est une COPIE de index.bin (la copie est rajeunie pour éviter une reconstruction).
# Activité sans souris : toutes les 5 minutes un appel `hello` par le tube ; toutes les -RechercheMin minutes une rafale de
# 5 recherches (charge l'index, ~124 Mo ; libéré après SPACEFILES_LIBERATION_S = 60 s). Les échantillons pris dans les
# 4 minutes qui suivent une rafale sont étiquetés « apres_recherche » ; les autres « repos » (la pente de repos n'utilise qu'eux).
# Chaque échantillon est pris AVANT l'activité de sa minute.
param(
    [string]$Exe = (Join-Path $PSScriptRoot '..\..\target\release\spacefiles.exe'),
    [int]$Duree = 60, [int]$Intervalle = 60, [int]$RechercheMin = 15,
    [string]$Csv = (Join-Path $env:TEMP 'ne-soak-resident.csv'),
    [string]$IndexSource = (Join-Path $env:LOCALAPPDATA 'SpaceFiles\index.bin'),
    [switch]$SansActivite
)
$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'commun.ps1')
$Exe = (Resolve-Path $Exe).Path
$racine = Join-Path $env:TEMP "ne-soak-$PID"
New-Item -ItemType Directory -Force $racine | Out-Null
$exeCopie = Join-Path $racine 'spacefiles.exe'; Copy-Item $Exe $exeCopie
$nomTube = "Eco.spacefiles.soak$PID"

if (-not $SansActivite) {
    # Lecture de l'index SOURCE (pas de PowerShell avant la redirection du profil : $env:LOCALAPPDATA est encore le vrai)
    $profil = Poser-FauxProfil $racine
    $copieIndex = Join-Path $profil.Local 'SpaceFiles\index.bin'
    Copy-Item $IndexSource $copieIndex -Force
    $fs = [IO.File]::Open($copieIndex, 'Open', 'ReadWrite'); $fs.Position = 8
    $t = [BitConverter]::GetBytes([int64][DateTimeOffset]::UtcNow.ToUnixTimeSeconds()); $fs.Write($t, 0, 8); $fs.Dispose()
} else { $null = Poser-FauxProfil $racine }

function Appeler([string]$ligne, [int]$delai = 8000) {
    $c = New-Object System.IO.Pipes.NamedPipeClientStream('.', $nomTube, [System.IO.Pipes.PipeDirection]::InOut, [System.IO.Pipes.PipeOptions]::Asynchronous)
    try {
        $c.Connect(5000)
        $o = [Text.Encoding]::UTF8.GetBytes($ligne + "`n"); $c.Write($o, 0, $o.Length); $c.Flush()
        $tampon = New-Object byte[] 16384; $recu = ''
        $sw = [Diagnostics.Stopwatch]::StartNew()
        while ($recu -notmatch "`n" -and $sw.ElapsedMilliseconds -lt $delai) {
            $lecture = $c.ReadAsync($tampon, 0, $tampon.Length)
            if (-not $lecture.Wait([Math]::Max(1, $delai - [int]$sw.ElapsedMilliseconds))) { break }
            if ($lecture.Result -le 0) { break }
            $recu += [Text.Encoding]::UTF8.GetString($tampon, 0, $lecture.Result)
        }
        $recu
    } catch { "ERREUR $($_.Exception.Message)" } finally { $c.Dispose() }
}
function Photo($procId) {
    $p = Get-Process -Id $procId
    $c = (Get-CimInstance Win32_PerfRawData_PerfProc_Thread -Filter "IDProcess=$procId" | Measure-Object ContextSwitchesPersec -Sum).Sum
    [pscustomobject]@{ Prive = $p.PrivateMemorySize64 / 1MB; WS = $p.WorkingSet64 / 1MB; Handles = $p.HandleCount; Fils = $p.Threads.Count; Cpu = $p.TotalProcessorTime.TotalMilliseconds }
}
function Pente($points) {
    # moindres carrés : points = @(@(minute, valeur), ...) -> Mo par heure
    $n = $points.Count; if ($n -lt 3) { return $null }
    $sx = ($points | % { $_[0] } | Measure-Object -Sum).Sum; $sy = ($points | % { $_[1] } | Measure-Object -Sum).Sum
    $sxy = ($points | % { $_[0] * $_[1] } | Measure-Object -Sum).Sum; $sxx = ($points | % { $_[0] * $_[0] } | Measure-Object -Sum).Sum
    $d = $n * $sxx - $sx * $sx; if ($d -eq 0) { return $null }
    60 * ($n * $sxy - $sx * $sy) / $d
}

$env:SPACEFILES_PIPE = "soak$PID"
$env:SPACEFILES_LIBERATION_S = '60'
$r = Lancer-SansFocus $exeCopie '--resident' -Cache
Remove-Item Env:\SPACEFILES_PIPE, Env:\SPACEFILES_LIBERATION_S
# Le nom du tube vient de l'application, de l'utilisateur et du suffixe d'essai : on le retrouve par le préfixe
"minute;heure;etat;prive_mo;ws_mo;handles;fils;cpu_ms;activite" | Set-Content $Csv -Encoding UTF8
$lignes = @(); $derniereRafale = $null
try {
    Start-Sleep -Seconds 10
    $debut = Get-Date
    $nbEchantillons = [int][math]::Floor($Duree * 60 / $Intervalle) + 1
    for ($k = 0; $k -lt $nbEchantillons; $k++) {
        $attendu = $debut.AddSeconds($k * $Intervalle)
        $reste = ($attendu - (Get-Date)).TotalSeconds
        if ($reste -gt 0) { Start-Sleep -Milliseconds ([int]($reste * 1000)) }
        $minute = [math]::Round(((Get-Date) - $debut).TotalMinutes, 2)
        $s = Photo $r.Id
        $etat = if ($derniereRafale -ne $null -and $minute - $derniereRafale -lt 4) { 'apres_recherche' } else { 'repos' }
        $activite = 'aucune'
        if (-not $SansActivite) {
            # `hello` toutes les 5 minutes seulement : une requête réarme la minuterie de libération de l'index (60 s ici)
            if (($k * $Intervalle) % 300 -eq 0) {
                $hello = Appeler '{"v":1,"id":"h","from":"soak","kind":"call","name":"hello"}'
                $activite = if ($hello -match '"app":"spacefiles"') { 'hello ok' } else { "hello ECHEC : $hello" }
            }
            if ($RechercheMin -gt 0 -and $k -gt 0 -and ($k * $Intervalle) % ($RechercheMin * 60) -eq 0) {
                $reponse = ''
                for ($j = 0; $j -lt 40; $j++) {
                    $reponse = Appeler ('{"v":1,"id":"s' + $j + '","from":"soak","kind":"call","name":"search.query","args":{"q":"windows"}}')
                    if ($reponse -notmatch 'loading') { break }
                    Start-Sleep -Milliseconds 500
                }
                1..4 | % { $null = Appeler ('{"v":1,"id":"t' + $_ + '","from":"soak","kind":"call","name":"search.query","args":{"q":"readme"}}') }
                $derniereRafale = $minute
                $activite += $(if ($reponse -match '"name"') { ' + rafale de recherches ok' } else { ' + rafale de recherches SANS RESULTAT' })
            }
        }
        $ligne = [pscustomobject]@{ Minute = $minute; Etat = $etat; Prive = $s.Prive; WS = $s.WS; Handles = $s.Handles; Fils = $s.Fils; Cpu = $s.Cpu }
        $lignes += $ligne
        ('{0};{1};{2};{3:N3};{4:N3};{5};{6};{7:N0};{8}' -f $minute, (Get-Date).ToString('HH:mm:ss'), $etat, $s.Prive, $s.WS, $s.Handles, $s.Fils, $s.Cpu, $activite) | Add-Content $Csv -Encoding UTF8
    }
    "CSV : $Csv ($($lignes.Count) échantillons sur $Duree min, intervalle $Intervalle s)"
    $premier = $lignes[0]; $dernier = $lignes[-1]
    "début : privé {0:N2} Mo, WS {1:N2} Mo, {2} handles, {3} fils" -f $premier.Prive, $premier.WS, $premier.Handles, $premier.Fils
    "fin   : privé {0:N2} Mo, WS {1:N2} Mo, {2} handles, {3} fils" -f $dernier.Prive, $dernier.WS, $dernier.Handles, $dernier.Fils
    $max = $lignes | Sort-Object Prive -Descending | Select-Object -First 1
    "maximum : privé {0:N2} Mo à la minute {1} ({2})" -f $max.Prive, $max.Minute, $max.Etat
    $pointsTous = @($lignes | % { , @($_.Minute, $_.Prive) })
    $pointsRepos = @($lignes | ? Etat -eq 'repos' | % { , @($_.Minute, $_.Prive) })
    "pente (tous les échantillons) : {0:N3} Mo/h" -f (Pente $pointsTous)
    "pente (échantillons au repos : {1}) : {0:N3} Mo/h" -f (Pente $pointsRepos), $pointsRepos.Count
    "handles : min {0}, max {1}, début {2}, fin {3}" -f ($lignes | Measure-Object Handles -Minimum).Minimum, ($lignes | Measure-Object Handles -Maximum).Maximum, $premier.Handles, $dernier.Handles
    "CPU consommé sur la durée : {0:N0} ms (résolution 15,6 ms)" -f ($dernier.Cpu - $premier.Cpu)
} finally {
    if (-not $r.HasExited) { Stop-Process -Id $r.Id -Force }
    Remove-Item $racine -Recurse -Force -ErrorAction SilentlyContinue
}
