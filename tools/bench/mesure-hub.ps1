# Mesure E-4 du Hub (SpaceNotch.Hub.exe publié) : mémoire de tout l'arbre de processus, délai d'ouverture, fermeture = 0.
# Phase 6, lot D. Aucune entrée souris ni clavier : la fenêtre est pilotée par UI Automation (InvokePattern, WindowPattern.Close).
#
# Chaque lancement est un processus neuf, sur un FAUX profil : APPDATA, LOCALAPPDATA, TEMP et TMP du Hub pointent vers un dossier
# d'essai (le Hub ne lit ni n'écrit rien dans les vrais dossiers de l'auteur) ; --eco / --plugins / --journaux visent aussi ce
# dossier. Le Hub s'ouvre quand même à l'écran (c'est ce qu'on mesure), sans prendre la souris.
#
# Relevés par lancement (somme sur le processus et ses descendants) :
#   t+2 s après l'apparition de la fenêtre, après les cinq pages, après 20 « Actualiser », après 60 s de repos (depuis la fin des
#   20 actualisations). Trois métriques : Working Set (WorkingSet64), Private Working Set (compteur « Working Set - Private »,
#   WMI Win32_PerfFormattedData_PerfProc_Process.WorkingSetPrivate) et Private Bytes (PrivateMemorySize64).
# Délai d'ouverture : de Process.Start à l'apparition de la fenêtre « Hub » (UIA, non hors écran), sondée toutes les 5 ms.
# Fermeture : WindowPattern.Close, puis délai jusqu'à la disparition du processus, processus résiduels, fichiers laissés.
#
# Usage : powershell -NoProfile -File tools\bench\mesure-hub.ps1 -Exe <SpaceNotch.Hub.exe> -Etiquette avant -Lancements 7 -Sortie <fichier.json>
param(
    [Parameter(Mandatory = $true)][string]$Exe,
    [string]$Etiquette = 'essai',
    [int]$Lancements = 7,
    [int]$RepoSecondes = 60,
    [string]$Sortie = ''
)
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName UIAutomationClient
Add-Type -AssemblyName UIAutomationTypes
$Exe = (Resolve-Path $Exe).Path
$AE = [Windows.Automation.AutomationElement]
$racineUia = $AE::RootElement

function Arbre([int]$racinePid) {
    # Le processus et tous ses descendants (un WinUI peut lancer des enfants).
    $tous = Get-CimInstance Win32_Process -Property ProcessId, ParentProcessId
    $ids = New-Object 'System.Collections.Generic.HashSet[int]'
    [void]$ids.Add($racinePid)
    do {
        $avant = $ids.Count
        foreach ($p in $tous) { if ($ids.Contains([int]$p.ParentProcessId)) { [void]$ids.Add([int]$p.ProcessId) } }
    } while ($ids.Count -gt $avant)
    return @($ids)
}

function Releve([int]$racinePid) {
    $ids = Arbre $racinePid
    $ws = 0L; $pb = 0L; $pws = 0L
    foreach ($id in $ids) {
        $proc = Get-Process -Id $id -ErrorAction SilentlyContinue
        if ($proc) { $ws += $proc.WorkingSet64; $pb += $proc.PrivateMemorySize64 }
        $perf = Get-CimInstance Win32_PerfFormattedData_PerfProc_Process -Filter "IDProcess=$id" -ErrorAction SilentlyContinue
        if ($perf) { $pws += [int64]$perf.WorkingSetPrivate }
    }
    return [pscustomobject]@{ Processus = $ids.Count; WS = [math]::Round($ws / 1MB, 1); PWS = [math]::Round($pws / 1MB, 1); PB = [math]::Round($pb / 1MB, 1) }
}

function Bouton($fenetre, [string]$id) {
    $c = New-Object Windows.Automation.PropertyCondition($AE::AutomationIdProperty, $id)
    return $fenetre.FindFirst([Windows.Automation.TreeScope]::Descendants, $c)
}

function Cliquer($fenetre, [string]$id) {
    $b = Bouton $fenetre $id
    if (-not $b) { throw "bouton $id introuvable" }
    $b.GetCurrentPattern([Windows.Automation.InvokePattern]::Pattern).Invoke()
}

$racineEssai = Join-Path ([IO.Path]::GetTempPath()) ('sf-mesure-hub-' + $Etiquette)
$resultats = @()
for ($n = 1; $n -le $Lancements; $n++) {
    if (Test-Path $racineEssai) { Remove-Item $racineEssai -Recurse -Force }
    $local = Join-Path $racineEssai 'Local'; $roaming = Join-Path $racineEssai 'Roaming'; $tmp = Join-Path $racineEssai 'Temp'
    $eco = Join-Path $local 'Eco'
    foreach ($d in @("$eco\apps", "$roaming", "$tmp", "$local\SpaceFiles", "$local\SpaceNotch\logs", "$racineEssai\plugins")) { New-Item -ItemType Directory -Force $d | Out-Null }
    # Deux manifestes, pas de tube : deux lignes « Arrêtée » (la liste réelle de l'auteur a deux lignes).
    $m1 = '{"id":"spacefiles","name":"SpaceFiles","version":"0.9.0","exe":"C:\\essai\\spacefiles.exe","pipe":"Eco.spacefiles.mesure","protocol":1,"capabilities":[]}'
    $m2 = '{"id":"spacenotch","name":"SpaceNotch","version":"1.2.3","exe":"C:\\essai\\SpaceNotch.App.exe","pipe":"Eco.spacenotch.mesure","protocol":1,"capabilities":[]}'
    [IO.File]::WriteAllText("$eco\apps\spacefiles.json", $m1)
    [IO.File]::WriteAllText("$eco\apps\spacenotch.json", $m2)

    $nomExe = [IO.Path]::GetFileNameWithoutExtension($Exe)
    $dejaLa = @(Get-Process -Name $nomExe -ErrorAction SilentlyContinue).Count

    $psi = New-Object Diagnostics.ProcessStartInfo
    $psi.FileName = $Exe
    $psi.UseShellExecute = $false
    $psi.WorkingDirectory = Split-Path $Exe
    $psi.Arguments = '--eco "{0}" --plugins "{1}\plugins" --journaux "{2}"' -f $eco, $racineEssai, $local
    $psi.EnvironmentVariables['LOCALAPPDATA'] = $local; $psi.EnvironmentVariables['APPDATA'] = $roaming
    $psi.EnvironmentVariables['TEMP'] = $tmp; $psi.EnvironmentVariables['TMP'] = $tmp
    $psi.EnvironmentVariables['SPACEFILES_SANS_REGISTRE'] = '1'
    $avantFichiers = @(Get-ChildItem $racineEssai -Recurse -Force | Where-Object { -not $_.PSIsContainer } | ForEach-Object { $_.FullName })

    $sw = [Diagnostics.Stopwatch]::StartNew()
    $proc = [Diagnostics.Process]::Start($psi)
    $cond = New-Object Windows.Automation.PropertyCondition($AE::ProcessIdProperty, $proc.Id)
    $fenetre = $null; $delai = $null
    while ($sw.ElapsedMilliseconds -lt 20000) {
        $f = $racineUia.FindFirst([Windows.Automation.TreeScope]::Children, $cond)
        if ($f -and $f.Current.Name -eq 'Hub' -and -not $f.Current.IsOffscreen) { $fenetre = $f; $delai = $sw.ElapsedMilliseconds; break }
        Start-Sleep -Milliseconds 5
    }
    if (-not $fenetre) { try { $proc.Kill() } catch {}; throw "lancement ${n} : fenêtre non apparue en 20 s" }

    # t+2 s après l'apparition de la fenêtre
    Start-Sleep -Milliseconds 2000
    $r2 = Releve $proc.Id

    # les cinq pages (l'ordre du rail), un temps pour que chaque page lise et dessine
    foreach ($id in @('RailAppearance', 'RailShortcuts', 'RailModules', 'RailHealth', 'RailApps')) {
        Cliquer $fenetre $id
        Start-Sleep -Milliseconds 1200
    }
    $rPages = Releve $proc.Id

    # 20 actualisations de la page Applications
    for ($i = 0; $i -lt 20; $i++) { Cliquer $fenetre 'RefreshButton'; Start-Sleep -Milliseconds 300 }
    Start-Sleep -Milliseconds 1000
    $rActu = Releve $proc.Id

    # repos
    Start-Sleep -Seconds $RepoSecondes
    $rRepos = Releve $proc.Id

    # fermeture : la fenêtre (WindowPattern.Close), jamais Kill
    $tFerme = [Diagnostics.Stopwatch]::StartNew()
    $fenetre.GetCurrentPattern([Windows.Automation.WindowPattern]::Pattern).Close()
    $parti = $proc.WaitForExit(10000)
    $delaiFermeture = if ($parti) { $tFerme.ElapsedMilliseconds } else { $null }
    if (-not $parti) { $proc.Kill() }
    Start-Sleep -Milliseconds 500
    $residuels = @(Get-Process -Name $nomExe -ErrorAction SilentlyContinue).Count - $dejaLa
    $laisses = @(Get-ChildItem $racineEssai -Recurse -Force -ErrorAction SilentlyContinue | Where-Object { -not $_.PSIsContainer -and ($avantFichiers -notcontains $_.FullName) } | ForEach-Object { $_.FullName.Substring($racineEssai.Length) })

    $res = [pscustomobject]@{
        Lancement = $n; DelaiOuvertureMs = $delai; DelaiFermetureMs = $delaiFermeture; ProcessusResiduels = $residuels
        T2 = $r2; Pages = $rPages; Actualisations = $rActu; Repos = $rRepos
        FichiersApresFermeture = $laisses
    }
    $resultats += $res
    "lancement {0} : ouverture {1} ms ; fermeture {2} ms ; résiduels {3} ; processus dans l'arbre {4} ; WS/PWS/PB (Mo) t+2 s {5}/{6}/{7}, pages {8}/{9}/{10}, 20 actu. {11}/{12}/{13}, repos {14}/{15}/{16}" -f `
        $n, $delai, $delaiFermeture, $residuels, $r2.Processus, $r2.WS, $r2.PWS, $r2.PB, $rPages.WS, $rPages.PWS, $rPages.PB, $rActu.WS, $rActu.PWS, $rActu.PB, $rRepos.WS, $rRepos.PWS, $rRepos.PB
}

function Stat($valeurs) {
    $t = @($valeurs | Sort-Object)
    $med = if ($t.Count % 2) { $t[[int][math]::Floor($t.Count / 2)] } else { ($t[$t.Count / 2 - 1] + $t[$t.Count / 2]) / 2 }
    "médiane {0} (min {1}, max {2}, n={3})" -f $med, $t[0], $t[-1], $t.Count
}
"== Étiquette $Etiquette : $Lancements lancements à froid, exe $Exe"
"Ouverture (ms) : " + (Stat ($resultats | ForEach-Object { $_.DelaiOuvertureMs }))
"Fermeture (ms) : " + (Stat ($resultats | ForEach-Object { $_.DelaiFermetureMs }))
foreach ($etape in @('T2', 'Pages', 'Actualisations', 'Repos')) {
    foreach ($metrique in @('WS', 'PWS', 'PB')) {
        "$etape $metrique (Mo) : " + (Stat ($resultats | ForEach-Object { $_.$etape.$metrique }))
    }
}
"Processus dans l'arbre (max) : " + (($resultats | ForEach-Object { $_.T2.Processus } | Measure-Object -Maximum).Maximum)
"Processus résiduels (somme) : " + (($resultats | Measure-Object -Property ProcessusResiduels -Sum).Sum)
"Fichiers créés par le Hub dans le faux profil (dernier lancement) : " + ($resultats[-1].FichiersApresFermeture -join ' ; ')
if ($Sortie) { $resultats | ConvertTo-Json -Depth 5 | Set-Content -Encoding UTF8 $Sortie }
