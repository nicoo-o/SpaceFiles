# Preuve du service de recherche du resident (phase 3, lot 3) : lance un SECOND resident de test (faux profil sous
# %TEMP%, tube a part, pas de crochet clavier : voir `essai()` dans src/raccourci.rs) et l'interroge par le tube.
# Ne touche ni au resident de l'auteur ni a ses donnees : APPDATA / LOCALAPPDATA ne sont redirigees que dans ce
# processus PowerShell (et le resident qu'il lance) ; index.bin reel est seulement COPIE (lecture seule).
# Aucune entree souris. Usage :
#   powershell -NoProfile -ExecutionPolicy Bypass -File tools\bench\service-recherche.ps1 [-Liberation 8] [-Essais 15]
param(
    [string]$Exe = (Join-Path $PSScriptRoot '..\..\target\release\spacefiles.exe'),
    [string]$IndexSource = (Join-Path $env:LOCALAPPDATA 'SpaceFiles\index.bin'),
    [int]$Liberation = 8,
    [int]$Essais = 15
)
$ErrorActionPreference = 'Stop'
$racine = Join-Path $env:TEMP 'sf-lot3'
$profil = Join-Path $racine 'faux-profil'
$suffixe = 'lot3' + $PID
$nomTube = "Eco.spacefiles.$suffixe"
Remove-Item $racine -Recurse -Force -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force (Join-Path $profil 'Local\SpaceFiles'), (Join-Path $profil 'Roaming\SpaceFiles') | Out-Null
Copy-Item $IndexSource (Join-Path $profil 'Local\SpaceFiles\index.bin') -Force
# La COPIE (jamais l'original) est rajeunie : au-dela de 6 h le service reconstruirait l'index (parcours des disques,
# plusieurs minutes) et la liberation attendrait la fin de cette construction. Date de construction = 8 octets a
# l'offset 8 (secondes Unix).
$copieIndex = Join-Path $profil 'Local\SpaceFiles\index.bin'
$fs = [IO.File]::Open($copieIndex, 'Open', 'ReadWrite'); $fs.Position = 8
$maintenant = [BitConverter]::GetBytes([int64][DateTimeOffset]::UtcNow.ToUnixTimeSeconds()); $fs.Write($maintenant, 0, 8); $fs.Dispose()
$exeCopie = Join-Path $racine 'spacefiles.exe'
Copy-Item (Resolve-Path $Exe).Path $exeCopie -Force

Add-Type -TypeDefinition @'
using System; using System.Runtime.InteropServices;
public static class Fen {
  [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern IntPtr FindWindowEx(IntPtr p, IntPtr a, string c, string t);
  [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
  [DllImport("user32.dll")] public static extern bool PostMessage(IntPtr h, uint m, IntPtr w, IntPtr l);
}
'@
function Poster-Resident($procId, [uint32]$message) {
    $h = [IntPtr]::Zero
    while (($h = [Fen]::FindWindowEx([IntPtr]::Zero, $h, 'SpaceFilesResidentEssai', [NullString]::Value)) -ne [IntPtr]::Zero) {
        [uint32]$p = 0; [void][Fen]::GetWindowThreadProcessId($h, [ref]$p)
        if ($p -eq $procId) { [void][Fen]::PostMessage($h, $message, [IntPtr]::Zero, [IntPtr]::Zero); return $true }
    }
    return $false
}

$script:un = New-Object byte[] 16384
$script:recu = New-Object System.Collections.Generic.List[byte]
$script:lecture = $null
function Connecter() {
    $script:lecture = $null; $script:recu.Clear()
    $c =New-Object System.IO.Pipes.NamedPipeClientStream('.', $nomTube, [System.IO.Pipes.PipeDirection]::InOut, [System.IO.Pipes.PipeOptions]::Asynchronous)
    # Asynchrone : sur un handle synchrone, une lecture en attente bloquerait toute ecriture
    $c.Connect(3000)
    $c
}
# Envoie une ligne, rend la reponse (ou $null apres $delai ms)
function Appeler($c, [string]$ligne, [int]$delai = 5000) {
    $o = [Text.Encoding]::UTF8.GetBytes($ligne + "`n")
    $c.Write($o, 0, $o.Length); $c.Flush()
    Lire-Ligne $c $delai
}
function Lire-Ligne($c, [int]$delai = 5000) {
    $fin = [Diagnostics.Stopwatch]::StartNew()
    while ($fin.ElapsedMilliseconds -lt $delai) {
        # Une ligne complete deja recue (plusieurs reponses dans un meme bloc) est rendue sans relire
        $k = $script:recu.IndexOf(10)
        if ($k -ge 0) {
            $ligne = [Text.Encoding]::UTF8.GetString($script:recu.GetRange(0, $k).ToArray())
            $script:recu.RemoveRange(0, $k + 1)
            return $ligne
        }
        # Une lecture qui expire reste en attente et sera reprise : abandonnee, elle avalerait des octets
        if ($null -eq $script:lecture) { $script:lecture = $c.ReadAsync($script:un, 0, $script:un.Length) }
        if (-not $script:lecture.Wait([Math]::Max(1, $delai - [int]$fin.ElapsedMilliseconds))) { return $null }
        $n = $script:lecture.Result
        $script:lecture = $null
        if ($n -le 0) { return $null }
        $script:recu.AddRange([byte[]]($script:un[0..($n - 1)]))
    }
    $null
}function Req($id, $q, $limit = $null) {
    $a = '{"q":"' + $q + '"' + $(if ($limit -ne $null) { ',"limit":' + $limit } else { '' }) + '}'
    '{"v":1,"id":"' + $id + '","from":"essai","kind":"call","name":"search.query","args":' + $a + '}'
}
function Prive($procId) { (Get-Process -Id $procId).PrivateMemorySize64 / 1MB }
function Pct($v, $p) { $t = $v | Sort-Object; $t[[int][Math]::Max(0, [Math]::Ceiling($t.Count * $p) - 1)] }
function Ok($cond, $texte) { if ($cond) { "OK   $texte" } else { "ECHEC $texte"; $script:echecs++ } }
$script:echecs = 0

# Redirection limitee a ce processus et a son enfant
$ancien = @{ LOCALAPPDATA = $env:LOCALAPPDATA; APPDATA = $env:APPDATA }
$env:LOCALAPPDATA = Join-Path $profil 'Local'; $env:APPDATA = Join-Path $profil 'Roaming'
$env:SPACEFILES_SANS_REGISTRE = '1'; $env:SPACEFILES_PIPE = $suffixe; $env:SPACEFILES_LIBERATION_S = "$Liberation"; $env:SPACEFILES_INACTIVITE_MS = '3000'
$resident = Start-Process -FilePath $exeCopie -ArgumentList '--resident' -PassThru -WindowStyle Hidden
$env:LOCALAPPDATA = $ancien.LOCALAPPDATA; $env:APPDATA = $ancien.APPDATA
Remove-Item Env:\SPACEFILES_PIPE, Env:\SPACEFILES_LIBERATION_S, Env:\SPACEFILES_SANS_REGISTRE, Env:\SPACEFILES_INACTIVITE_MS
$idRes = $resident.Id
try {
    Start-Sleep -Seconds 3
    "== Resident de test pid $idRes, tube \\.\pipe\$nomTube, liberation apres $Liberation s"
    $threads0 = (Get-Process -Id $idRes).Threads.Count
    $prive0 = Prive $idRes
    "Memoire privee au repos, avant toute requete : {0:N1} Mo ({1} fils)" -f $prive0, $threads0

    "== 1. Protocole"
    $c = Connecter
    $r = Appeler $c '{"v":1,"id":"h1","from":"essai","kind":"call","name":"hello"}'
    $r
    Ok ($r -match '"app":"spacefiles"' -and $r -match '"protocol":1' -and $r -match 'search.query') 'hello'
    $r = Appeler $c '{ceci nest pas du json'
    $r
    Ok ($r -match '"code":"invalide"') 'trame invalide -> erreur'
    $r = Appeler $c '{"v":1,"id":"h2","kind":"call","name":"hello"}'
    Ok ($r -match '"id":"h2"' -and $r -match '"app":"spacefiles"') 'le tube reste vivant apres une trame invalide'
    # Lot A de la phase 4 : explorer.open est servi maintenant ; le nom inconnu de ce test est un autre (voir eco-lot-a.ps1 pour explorer.open)
    $r = Appeler $c '{"v":1,"id":"u1","kind":"call","name":"zzz.inconnu","args":{}}'
    $r
    Ok ($r -match '"code":"inconnu"') 'appel inconnu -> erreur nommee'
    $r = Appeler $c '{"v":1,"id":"m1","kind":"call","name":"search.query","args":{}}'
    Ok ($r -match '"code":"champ_manquant"') 'champ q manquant -> erreur'
    $r = Appeler $c (Req 'l0' 'windows' 0)
    Ok ($r -match '"code":"hors_bornes"') 'limit 0 -> erreur'
    $r = Appeler $c (Req 'l31' 'windows' 31)
    Ok ($r -match '"code":"hors_bornes"') 'limit 31 -> erreur'
    $r = Appeler $c (Req 'c1' 'abc contenu:secret')
    Ok ($r -match '"code":"non_supporte"') 'contenu: refuse'
    $r = Appeler $c (Req 'court' 'ab')
    $r
    Ok ($r -match '"results":\[\]' -and $r -notmatch 'loading') 'q de 2 caracteres -> reponse vide'
    $c.Dispose()

    # Trame de 5000 caracteres : rejetee, connexion fermee par le service
    $c = Connecter
    $r = Appeler $c (Req 'big' ('a' * 5000))
    $r
    Ok ($r -match '"code":"trop_long"') 'requete de 5000 caracteres rejetee'
    $c.Dispose()
    Start-Sleep -Milliseconds 300
    $c = Connecter
    $r = Appeler $c '{"v":1,"id":"h3","kind":"call","name":"hello"}'
    Ok ($r -match '"id":"h3"') 'le service sert encore apres la trame trop longue'

    "== 1b. Clients qui partent aussitot, client muet"
    $c.Dispose()   # l'instance est unique : le client de la section 1 doit la rendre
    # 60 connexions fermees tout de suite : l'ancien service s'arretait apres 50 echecs de connexion
    for ($k = 0; $k -lt 60; $k++) { $x = Connecter; $x.Dispose() }
    $c = Connecter
    $r = Appeler $c '{"v":1,"id":"h5","kind":"call","name":"hello"}'
    Ok ($r -match '"id":"h5"') 'apres 60 clients connectes puis fermes aussitot, un client normal est servi'
    $c.Dispose()
    # Client muet (delai d'inactivite de l'essai : 3 s) : le suivant attend puis est servi
    $muet = Connecter
    $sw = [Diagnostics.Stopwatch]::StartNew()
    $suivant = New-Object System.IO.Pipes.NamedPipeClientStream('.', $nomTube, [System.IO.Pipes.PipeDirection]::InOut, [System.IO.Pipes.PipeOptions]::Asynchronous)
    $suivant.Connect(8000)
    $script:lecture = $null; $script:recu.Clear()
    $r = Appeler $suivant '{"v":1,"id":"h6","kind":"call","name":"hello"}' 3000
    "le client suivant a ete servi apres $($sw.ElapsedMilliseconds) ms"
    Ok ($r -match '"id":"h6"' -and $sw.ElapsedMilliseconds -lt 1500) 'lot A : plusieurs instances, un client muet ne bloque plus le suivant (avant : servi apres ~3 s)'
    $suivant.Dispose(); $muet.Dispose()
    Start-Sleep -Milliseconds 300
    $c = Connecter
    $r = Appeler $c '{"v":1,"id":"h3b","kind":"call","name":"hello"}'
    Ok ($r -match '"id":"h3b"') 'le service sert encore'
    $c.Dispose()

    $c = Connecter
    "== 2. Premiere requete a froid (chargement a la demande)"
    $sw = [Diagnostics.Stopwatch]::StartNew()
    $r = Appeler $c (Req 'f1' 'windows')
    "premiere reponse en {0} ms : {1}" -f $sw.ElapsedMilliseconds, $r.Substring(0, [Math]::Min(120, $r.Length))
    Ok ($r -match '"loading":true') 'premiere reponse = loading:true (non bloquante)'
    $premiereMs = $sw.ElapsedMilliseconds
    $nChargement = 1
    while ($true) {
        $r = Appeler $c (Req "f$nChargement" 'windows')
        if ($r -notmatch '"loading":true') { break }
        $nChargement++
        Start-Sleep -Milliseconds 20
        if ($sw.ElapsedMilliseconds -gt 60000) { throw 'index jamais charge' }
    }
    $froidMs = $sw.ElapsedMilliseconds
    "froid : premiere reponse (loading) en $premiereMs ms ; resultats reels apres $froidMs ms depuis la premiere requete ($nChargement reponses loading)"
    $nom = ([regex]::Matches($r, '"name"')).Count
    $r.Substring(0, [Math]::Min(300, $r.Length))
    Ok ($nom -ge 1 -and $nom -le 30 -and $r.Length -le 4096) "windows -> $nom resultats (<= 30), ligne de $($r.Length) octets"
    $priveCharge = Prive $idRes
    "Memoire privee apres chargement : {0:N1} Mo ({1} fils)" -f $priveCharge, (Get-Process -Id $idRes).Threads.Count

    "== 3. Latences a travers le tube (n = $Essais par terme, connexion unique, apres chargement)"
    foreach ($terme in 'a', 'aaa', 'windows', 'readme.md', 'note', 'zzzqxj') {
        $d = @()
        for ($i = 0; $i -lt $Essais; $i++) {
            $sw = [Diagnostics.Stopwatch]::StartNew()
            $r = Appeler $c (Req "t$i" $terme)
            $sw.Stop()
            if ($r -match 'loading') { throw 'index decharge en cours de mesure' }
            $d += $sw.Elapsed.TotalMilliseconds
        }
        $n = ([regex]::Matches($r, '"name"')).Count
        "{0,-10} p50 {1,7:N1} ms  p95 {2,7:N1} ms  min {3,7:N1}  max {4,7:N1}  ({5} resultats)" -f $terme, (Pct $d 0.5), (Pct $d 0.95), ($d | Measure-Object -Minimum).Minimum, ($d | Measure-Object -Maximum).Maximum, $n
    }

    "== 4. Annulation : 6 requetes lentes (zzzqxj) envoyees d'un coup sur la meme connexion"
    $lot = (1..6 | ForEach-Object { Req "z$_" 'zzzqxj' }) -join "`n"
    $o = [Text.Encoding]::UTF8.GetBytes($lot + "`n"); $c.Write($o, 0, $o.Length); $c.Flush()
    $ids = @()
    while ($true) { $l = Lire-Ligne $c 1500; if ($null -eq $l) { break }; $ids += ([regex]::Match($l, '"id":"([^"]*)"').Groups[1].Value) }
    "reponses recues pour : $($ids -join ', ')"
    Ok ($ids -contains 'z6' -and $ids.Count -lt 6) 'la derniere est repondue, les precedentes sont abandonnees'
    $r = Appeler $c '{"v":1,"id":"h4","kind":"call","name":"hello"}'
    Ok ($r -match '"id":"h4"') 'connexion intacte apres les abandons'
    $c.Dispose()

    "== 5. Acces"
    try {
        # Get-Acl ouvre le tube : il peut tomber pendant que l instance unique est occupee, on reessaie
        # (Get-Acl passe par le fournisseur FileSystem, qui ne voit pas toujours le tube : on lit la DACL sur le flux client)
        $acl = $null; $ca = Connecter
        $acl = $ca.GetAccessControl().GetSecurityDescriptorSddlForm('All'); $ca.Dispose()
        if (-not $acl) { throw 'Get-Acl impossible' }
        "SDDL du tube : $acl"
        $moi = [Security.Principal.WindowsIdentity]::GetCurrent().User.Value
        Ok ($acl -match [regex]::Escape($moi) -and $acl -notmatch ';WD\)' -and $acl -notmatch ';AN\)') 'DACL : uniquement l utilisateur courant (ni Everyone ni anonyme)'
    } catch { "SDDL non lisible par Get-Acl : $($_.Exception.Message)" }
    try {
        $faux = New-Object System.IO.Pipes.NamedPipeServerStream($nomTube, [System.IO.Pipes.PipeDirection]::InOut, 4)
        "ECHEC un second serveur a pu creer le meme nom de tube"; $script:echecs++; $faux.Dispose()
    } catch { "OK   un second serveur ne peut pas prendre le nom du tube ($($_.Exception.GetType().Name))" }

    "== 6. Liberation apres $Liberation s d'inactivite"
    $attente = $Liberation + 6
    $sw = [Diagnostics.Stopwatch]::StartNew()
    Start-Sleep -Seconds $attente
    $priveLibre = Prive $idRes
    "Memoire privee apres {0} s sans requete : {1:N1} Mo ({2} fils)" -f $attente, $priveLibre, (Get-Process -Id $idRes).Threads.Count
    "Resume memoire (Mo prives) : repos {0:N1} | charge {1:N1} | libere {2:N1} | ecart libere-repos {3:+0.0;-0.0}" -f $prive0, $priveCharge, $priveLibre, ($priveLibre - $prive0)
    Ok ((Get-Process -Id $idRes).Threads.Count -le $threads0) "fils de surveillance termines apres liberation (fils : $threads0 au repos)"
    Ok ([Math]::Abs($priveLibre - $prive0) -le 5)'prive revenu a +-5 Mo de la valeur de depart'
    # L'index doit se recharger a la requete suivante
    $c = Connecter
    $r = Appeler $c (Req 'r1' 'windows')
    Ok ($r -match '"loading":true') 'apres liberation, la requete suivante relance le chargement'
    $c.Dispose()

    "== 6b. Supprimer l'index (message WM_APP+24 envoye au resident de test)"
    $c = Connecter
    for ($k = 0; $k -lt 400; $k++) { $r = Appeler $c (Req "e$k" 'windows'); if ($r -notmatch 'loading') { break }; Start-Sleep -Milliseconds 25 }
    Ok ($r -match '"name"') 'index recharge'
    $c.Dispose()
    $indexFichier = Join-Path $profil 'Local\SpaceFiles\index.bin'
    Ok (Poster-Resident $idRes 0x8018) 'message d effacement poste'
    Start-Sleep -Milliseconds 500
    $c = Connecter
    $r = Appeler $c (Req 'e-apres' 'windows')
    $r
    Ok ($r -match '"code":"index_efface"') 'apres effacement le service ne sert plus et ne recree pas l index'
    $c.Dispose()
    Start-Sleep -Seconds 2
    Ok (-not (Test-Path $indexFichier)) 'index.bin n est pas reecrit'
    "Memoire privee apres effacement : {0:N1} Mo" -f (Prive $idRes)

    "== 7. Arret propre"
    $fermee = Poster-Resident $idRes 0x10
    Ok $fermee 'WM_CLOSE envoye a la fenetre du resident de test'
    $sortie = $resident.WaitForExit(5000)
    Ok $sortie 'le resident de test se termine tout seul'
    Ok (-not (Test-Path "\\.\pipe\$nomTube")) 'le tube a disparu'
    $journal = Join-Path $profil 'Local\SpaceFiles\journal.txt'
    "-- journal du faux profil :"
    Get-Content $journal -Encoding UTF8 | Select-Object -Last 14
    $texte = Get-Content $journal -Raw -Encoding UTF8
    Ok ($texte -notmatch 'windows|readme|zzzqxj|note\b|C:\\') 'le journal ne contient ni texte cherche ni chemin'
    Ok ($texte -match 'service arrete') 'journal : service arrete'
} finally {
    if (-not $resident.HasExited) { Stop-Process -Id $idRes -Force }
    Remove-Item Env:\SPACEFILES_PIPE -ErrorAction SilentlyContinue
}
"== Bilan : $script:echecs echec(s)"
