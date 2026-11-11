# Latence de requête de l'index Windows Search (SystemIndex), mêmes termes que le banc de l'index SpaceFiles
# (phase 3, E-1). Lecture seule : une requête SQL via OLE DB (Search.CollatorDSO), rien n'est écrit nulle part,
# aucune donnée de SpaceFiles ou de SpaceNotch n'est touchée.
#
# Chaque requête est jouée $Essais fois APRÈS un essai de chauffe écarté (donné à part, « premier appel »).
# Le temps compté va de l'exécution à la lecture de toutes les lignes (au plus $Max), pour être comparable à
# `chercher(motif, 5000)` du banc SpaceFiles qui rend le vecteur complet. Percentiles : rang le plus proche
# (p50 = élément ceil(0,50 n), p95 = élément ceil(0,95 n) du tri croissant).
#
# Usage : powershell -NoProfile -ExecutionPolicy Bypass -File tools\bench\windows-search.ps1 [-Essais 20] [-Max 5000]
param(
    [int]$Essais = 20,
    [int]$Max = 5000
)

$ErrorActionPreference = 'Stop'

function Percentile([double[]]$tri, [double]$p) {
    $rang = [int][math]::Ceiling($p * $tri.Count)
    if ($rang -lt 1) { $rang = 1 }
    return $tri[$rang - 1]
}

function Echapper([string]$t) { return $t.Replace("'", "''") }

# Requêtes : (libellé, SQL). LIKE '%x%' = même sémantique que SpaceFiles (sous-chaîne du nom, sans casse).
# La variante CONTAINS (préfixe de mot, indexée) est mesurée à part : sémantique différente, donc marquée.
$requetes = @(
    @{ Nom = 'a';         Genre = 'sous-chaîne'; Sql = "SELECT TOP $Max System.ItemPathDisplay FROM SystemIndex WHERE System.FileName LIKE '%a%'" },
    @{ Nom = 'windows';   Genre = 'sous-chaîne'; Sql = "SELECT TOP $Max System.ItemPathDisplay FROM SystemIndex WHERE System.FileName LIKE '%windows%'" },
    @{ Nom = 'ext:pdf';   Genre = 'extension';   Sql = "SELECT TOP $Max System.ItemPathDisplay FROM SystemIndex WHERE System.FileExtension = '.pdf'" },
    @{ Nom = 'readme.md'; Genre = 'sous-chaîne'; Sql = "SELECT TOP $Max System.ItemPathDisplay FROM SystemIndex WHERE System.FileName LIKE '%readme.md%'" },
    @{ Nom = 'note';      Genre = 'sous-chaîne'; Sql = "SELECT TOP $Max System.ItemPathDisplay FROM SystemIndex WHERE System.FileName LIKE '%note%'" },
    @{ Nom = 'zzzqxj';    Genre = 'sous-chaîne'; Sql = "SELECT TOP $Max System.ItemPathDisplay FROM SystemIndex WHERE System.FileName LIKE '%zzzqxj%'" },
    @{ Nom = 'a';         Genre = 'préfixe de mot (CONTAINS)'; Sql = "SELECT TOP $Max System.ItemPathDisplay FROM SystemIndex WHERE CONTAINS(System.FileName, '""a*""')" },
    @{ Nom = 'windows';   Genre = 'préfixe de mot (CONTAINS)'; Sql = "SELECT TOP $Max System.ItemPathDisplay FROM SystemIndex WHERE CONTAINS(System.FileName, '""windows*""')" },
    @{ Nom = 'note';      Genre = 'préfixe de mot (CONTAINS)'; Sql = "SELECT TOP $Max System.ItemPathDisplay FROM SystemIndex WHERE CONTAINS(System.FileName, '""note*""')" },
    @{ Nom = 'zzzqxj';    Genre = 'préfixe de mot (CONTAINS)'; Sql = "SELECT TOP $Max System.ItemPathDisplay FROM SystemIndex WHERE CONTAINS(System.FileName, '""zzzqxj*""')" }
)

$service = Get-Service WSearch
Write-Output ("SERVICE WSearch : {0}" -f $service.Status)

$cn = New-Object -ComObject ADODB.Connection
$cn.Open("Provider=Search.CollatorDSO;Extended Properties='Application=Windows';")

function Jouer([string]$sql) {
    $t = [System.Diagnostics.Stopwatch]::StartNew()
    $rs = New-Object -ComObject ADODB.Recordset
    $rs.Open($sql, $cn)
    $n = 0
    $premier = $null
    while (-not $rs.EOF) {
        if ($n -eq 0) { $premier = $t.Elapsed.TotalMilliseconds }
        $n++
        $rs.MoveNext()
    }
    $rs.Close()
    $t.Stop()
    return @{ Ms = $t.Elapsed.TotalMilliseconds; N = $n; Premier = $premier }
}

foreach ($q in $requetes) {
    $chauffe = Jouer $q.Sql
    $ms = New-Object System.Collections.Generic.List[double]
    $premiers = New-Object System.Collections.Generic.List[double]
    $n = 0
    for ($i = 0; $i -lt $Essais; $i++) {
        $r = Jouer $q.Sql
        $ms.Add($r.Ms)
        if ($null -ne $r.Premier) { $premiers.Add($r.Premier) }
        $n = $r.N
    }
    $tri = [double[]]($ms | Sort-Object)
    $triP = if ($premiers.Count -gt 0) { [double[]]($premiers | Sort-Object) } else { $null }
    $pp = if ($triP) { '{0:N1}' -f (Percentile $triP 0.5) } else { 'n/a' }
    Write-Output ('REQUETE {0,-10} {1,-26} resultats={2,-5} premier_appel={3,8:N1} ms | essais={4} p50={5,8:N1} p95={6,8:N1} max={7,8:N1} ms | 1re_ligne_p50={8} ms' -f `
        $q.Nom, $q.Genre, $n, $chauffe.Ms, $Essais, (Percentile $tri 0.5), (Percentile $tri 0.95), $tri[$tri.Count - 1], $pp)
}

$cn.Close()
