# Mesure le résident `spacefiles.exe --resident` au repos : octets privés, working set, CPU, changements de contexte (réveils),
# fils et handles. Plusieurs lancements (-Essais), chacun avec sa propre copie du processus, dans un faux profil.
# Le résident de test tourne en mode essai (SPACEFILES_PIPE : verrou, classe de fenêtre et tube à part, pas de crochet clavier),
# sans quoi il se heurterait au verrou du vrai résident ou poserait un second crochet Win+E. Le processus lancé ici est le seul
# arrêté, par son identifiant. Si un résident de l'auteur tourne déjà, il n'est ni touché ni mesuré ici : -Existant le mesure
# (lecture seule : ni lancement ni arrêt).
# CPU : TotalProcessorTime lu au début et à la fin de la fenêtre de repos (-Repos secondes, après 5 s de mise en route).
# Réveils : changements de contexte des fils (compteur brut WMI Win32_PerfRawData_PerfProc_Thread), même fenêtre, seulement pour\n# les fils présents aux deux relevés (un fil qui disparaît emporte son compteur : la somme brute peut baisser).
param([string]$Exe = (Join-Path $PSScriptRoot '..\..\target\release\spacefiles.exe'), [int]$Repos = 60, [int]$Essais = 1, [switch]$Existant)
$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'commun.ps1')

function Delta-Contextes($a, $b) { $s = 0.0; foreach ($k in $a.Keys) { if ($b.ContainsKey($k)) { $s += $b[$k] - $a[$k] } }; $s }
function Photo($procId) {
    $p = Get-Process -Id $procId
    $c = (Get-CimInstance Win32_PerfRawData_PerfProc_Thread -Filter "IDProcess=$procId" | Measure-Object ContextSwitchesPersec -Sum).Sum
    [pscustomobject]@{ Prive = $p.PrivateMemorySize64; WS = $p.WorkingSet64; Cpu = $p.TotalProcessorTime.TotalMilliseconds; Contextes = $c; Fils = $p.Threads.Count; Handles = $p.HandleCount }
}

if ($Existant) {
    $p = Get-CimInstance Win32_Process -Filter "Name='spacefiles.exe'" | ? CommandLine -match '--resident' | Select-Object -First 1
    if (-not $p) { throw 'Aucun résident en cours.' }
    $a = Photo $p.ProcessId; Start-Sleep -Seconds $Repos; $b = Photo $p.ProcessId
    "pid {0} (existant, lecture seule) : privé {1:N2} Mo, WS {2:N2} Mo, CPU {3:N0} ms en {4} s, {5:N0} changements de contexte (fils communs), {6} fils, {7} handles" -f $p.ProcessId, ($b.Prive / 1MB), ($b.WS / 1MB), ($b.Cpu - $a.Cpu), $Repos, (Delta-Contextes $a.Contextes $b.Contextes), $b.Fils, $b.Handles
    return
}

$Exe = (Resolve-Path $Exe).Path
$racine = Join-Path $env:TEMP "ne-bench-resident-$PID"
$null = Poser-FauxProfil $racine
$lignes = @()
for ($i = 1; $i -le $Essais; $i++) {
    $env:SPACEFILES_PIPE = "bench$PID-$i"
    $r = Lancer-SansFocus $Exe '--resident' -Cache
    Remove-Item Env:\SPACEFILES_PIPE
    try {
        Start-Sleep -Seconds 5
        $a = Photo $r.Id
        Start-Sleep -Seconds $Repos
        $b = Photo $r.Id
        $l = [pscustomobject]@{ Essai = $i; Pid = $r.Id; PriveMo = [math]::Round($b.Prive / 1MB, 2); WsMo = [math]::Round($b.WS / 1MB, 2); CpuMs = [math]::Round($b.Cpu - $a.Cpu, 1); CpuPct = [math]::Round(($b.Cpu - $a.Cpu) / ($Repos * 1000) * 100, 4); Contextes = (Delta-Contextes $a.Contextes $b.Contextes); ContextesParS = [math]::Round((Delta-Contextes $a.Contextes $b.Contextes) / $Repos, 3); Fils = $b.Fils; Handles = $b.Handles }
        $l | Format-List | Out-String -Width 200 | Write-Host
        $lignes += $l
    } finally {
        if (-not $r.HasExited) { Stop-Process -Id $r.Id -Force }
    }
    Start-Sleep -Seconds 2
}
$lignes | Format-Table -AutoSize | Out-String -Width 200
foreach ($c in 'PriveMo', 'WsMo', 'CpuMs', 'CpuPct', 'Contextes', 'Fils', 'Handles') { "{0,-10} : {1}" -f $c, (Texte-Resume (Resume-Valeurs ($lignes | % { $_.$c }))) }
Remove-Item $racine -Recurse -Force -ErrorAction SilentlyContinue
