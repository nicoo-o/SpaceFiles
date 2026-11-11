# Preuve de la bibliotheque Eco (phase 4, lot A) : lance un SECOND resident de test (faux profil sous %TEMP%, tube a part,
# pas de crochet clavier : voir `essai()` dans src/raccourci.rs) et l'interroge par le tube. Complete service-recherche.ps1
# (qui reste la preuve du lot 3, inchangee dans ses assertions sauf deux, voir le rapport).
# Ne touche ni au resident de l'auteur ni a ses donnees : APPDATA / LOCALAPPDATA ne sont redirigees que dans ce processus
# PowerShell (et le resident qu'il lance) ; index.bin reel est seulement COPIE (lecture seule). Aucune entree souris.
# PAR DEFAUT AUCUNE FENETRE N'EST OUVERTE SUR LE BUREAU : `explorer.open` est prouve avec un FAUX LANCEUR du resident de test
# (variable SPACEFILES_FAUX_LANCEUR, lue seulement en mode essai) qui note la ligne d'arguments dans un fichier sans rien afficher.
# `-AvecFenetres` ajoute UN essai reel (une fenetre de l'exe de TEST, copie sous %TEMP%, fermee par WM_CLOSE envoye a CETTE fenetre
# seulement, reperee par EnumWindows + pid) : elle apparait quelques secondes sur le bureau. Non lance par defaut.
# Usage : powershell -NoProfile -ExecutionPolicy Bypass -File tools\bench\eco-lot-a.ps1 [-Mille 1000] [-AvecFenetres]
param(
    [string]$Exe = (Join-Path $PSScriptRoot '..\..\target\release\spacefiles.exe'),
    [string]$IndexSource = (Join-Path $env:LOCALAPPDATA 'SpaceFiles\index.bin'),
    [int]$Mille = 1000,
    [switch]$AvecFenetres
)
$ErrorActionPreference = 'Stop'
$racine = Join-Path $env:TEMP 'sf-eco-lota'
$profil = Join-Path $racine 'faux-profil'
$suffixe = 'lota' + $PID
$nomTube = "Eco.spacefiles.$suffixe"
$nomNotch = "Eco.spacenotch.$suffixe"
$lanceurFaux = Join-Path $racine 'faux-lanceur.txt'
$reelEco = Join-Path $env:LOCALAPPDATA 'Eco'
$eco0 = if (Test-Path $reelEco) { (Get-Item $reelEco).LastWriteTimeUtc.Ticks } else { 'absent' }
Remove-Item $racine -Recurse -Force -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force (Join-Path $profil 'Local\SpaceFiles'), (Join-Path $profil 'Local\Eco'), (Join-Path $profil 'Roaming\SpaceFiles'), (Join-Path $profil 'Dossier de test') | Out-Null
Copy-Item $IndexSource (Join-Path $profil 'Local\SpaceFiles\index.bin') -Force
# La COPIE est rajeunie (voir service-recherche.ps1) : sinon le service reconstruirait l'index au-dela de 6 h
$fs = [IO.File]::Open((Join-Path $profil 'Local\SpaceFiles\index.bin'), 'Open', 'ReadWrite'); $fs.Position = 8
$maintenant = [BitConverter]::GetBytes([int64][DateTimeOffset]::UtcNow.ToUnixTimeSeconds()); $fs.Write($maintenant, 0, 8); $fs.Dispose()
$exeCopie = Join-Path $racine 'spacefiles.exe'
Copy-Item (Resolve-Path $Exe).Path $exeCopie -Force
# shared.json avec BOM UTF-8 : le resident doit le lire (AUDIT R23)
$partage = Join-Path $profil 'Local\Eco\shared.json'
[IO.File]::WriteAllText($partage, '{"theme":"light","accent":"#112233","motion":"reduced","language":"en-US","shortcuts":{"a.b":"Ctrl+K"},"futur":{"x":[1]}}', (New-Object Text.UTF8Encoding($true)))

Add-Type -TypeDefinition @'
using System; using System.Collections.Generic; using System.Diagnostics; using System.IO; using System.IO.Pipes;
using System.Runtime.InteropServices; using System.Text; using System.Threading;
public static class Eco {
  static string LireLigne(Stream s) {
    var tampon = new List<byte>(); var bloc = new byte[4096];
    while (true) {
      int k = tampon.IndexOf(10);
      if (k >= 0) return Encoding.UTF8.GetString(tampon.GetRange(0, k).ToArray());
      int n = s.Read(bloc, 0, bloc.Length);
      if (n <= 0) return tampon.Count == 0 ? null : Encoding.UTF8.GetString(tampon.ToArray());
      for (int i = 0; i < n; i++) tampon.Add(bloc[i]);
    }
  }
  // Connexion courte : connecter, une ligne, une reponse, fermer
  public static string Appeler(string tube, string ligne, int delaiMs) {
    using (var c = new NamedPipeClientStream(".", tube, PipeDirection.InOut, PipeOptions.None)) {
      c.Connect(delaiMs);
      var o = Encoding.UTF8.GetBytes(ligne + "\n"); c.Write(o, 0, o.Length); c.Flush();
      return LireLigne(c);
    }
  }
  // n appels `hello` en connexion courte : durees en ms
  public static double[] Mille(string tube, int n) {
    var d = new double[n]; var ligne = "{\"v\":1,\"id\":\"m\",\"from\":\"essai\",\"kind\":\"call\",\"name\":\"hello\"}";
    for (int i = 0; i < n; i++) {
      var sw = Stopwatch.StartNew();
      var r = Appeler(tube, ligne, 5000);
      sw.Stop();
      if (r == null || r.IndexOf("\"app\":\"spacefiles\"") < 0) throw new Exception("hello " + i + " : " + r);
      d[i] = sw.Elapsed.TotalMilliseconds;
    }
    return d;
  }
  // Une ligne par fil, tous en meme temps : "ms|reponse" (ou "ERREUR|...")
  public static string[] Concurrence(string tube, string[] lignes) {
    var res = new string[lignes.Length]; var fils = new List<Thread>(); var depart = new ManualResetEvent(false);
    for (int i = 0; i < lignes.Length; i++) {
      int k = i;
      var t = new Thread(() => {
        depart.WaitOne(); var sw = Stopwatch.StartNew();
        try { var r = Appeler(tube, lignes[k], 8000); res[k] = sw.ElapsedMilliseconds + "|" + r; }
        catch (Exception e) { res[k] = "ERREUR|" + e.GetType().Name; }
      });
      t.Start(); fils.Add(t);
    }
    depart.Set(); foreach (var t in fils) t.Join();
    return res;
  }
  // n lignes envoyees l'une apres l'autre sur UNE connexion : les reponses
  public static string[] Rafale(string tube, string ligne, int n) {
    var res = new string[n];
    using (var c = new NamedPipeClientStream(".", tube, PipeDirection.InOut, PipeOptions.None)) {
      c.Connect(5000);
      var o = Encoding.UTF8.GetBytes(ligne + "\n");
      for (int i = 0; i < n; i++) { c.Write(o, 0, o.Length); c.Flush(); res[i] = LireLigne(c); }
    }
    return res;
  }
  // Une ligne trop longue : envoie sans lire la suite, rend la reponse puis dit si la connexion a ete fermee
  public static string TropLong(string tube, string ligne) {
    using (var c = new NamedPipeClientStream(".", tube, PipeDirection.InOut, PipeOptions.None)) {
      c.Connect(5000);
      var o = Encoding.UTF8.GetBytes(ligne + "\n"); c.Write(o, 0, o.Length); c.Flush();
      var r = LireLigne(c);
      var suite = LireLigne(c);
      return r + "|ferme=" + (suite == null);
    }
  }
}
public static class Fen {
  delegate bool Rappel(IntPtr h, IntPtr l);
  [DllImport("user32.dll")] static extern bool EnumWindows(Rappel f, IntPtr l);
  [DllImport("user32.dll", CharSet=CharSet.Unicode)] static extern int GetClassName(IntPtr h, StringBuilder s, int n);
  [DllImport("user32.dll")] static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
  [DllImport("user32.dll")] static extern bool IsWindowVisible(IntPtr h);
  [DllImport("user32.dll")] public static extern bool PostMessage(IntPtr h, uint m, IntPtr w, IntPtr l);
  [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern IntPtr FindWindowEx(IntPtr p, IntPtr a, string c, string t);
  // Fenetres de classe donnee appartenant a l'un de ces processus
  public static List<IntPtr> Liste(string classe, uint[] pids) {
    var r = new List<IntPtr>(); var ens = new HashSet<uint>(pids);
    EnumWindows((h, l) => {
      var s = new StringBuilder(256); GetClassName(h, s, 256);
      uint pid; GetWindowThreadProcessId(h, out pid);
      if (s.ToString() == classe && ens.Contains(pid)) r.Add(h);
      return true;
    }, IntPtr.Zero);
    return r;
  }
  public static uint Pid(IntPtr h) { uint p; GetWindowThreadProcessId(h, out p); return p; }
}
'@

function Pct($v, $p) { $t = $v | Sort-Object; $t[[int][Math]::Max(0, [Math]::Ceiling($t.Count * $p) - 1)] }
function Ok($cond, $texte) { if ($cond) { "OK   $texte" } else { "ECHEC $texte"; $script:echecs++ } }
$script:echecs = 0
function Prive($procId) { (Get-Process -Id $procId).PrivateMemorySize64 / 1MB }
function Tube-Present { [IO.Directory]::GetFiles('\\.\pipe\') -contains "\\.\pipe\$nomTube" }
function Demarrer-Resident([bool]$fauxLanceur = $true) {
    $ancien = @{ LOCALAPPDATA = $env:LOCALAPPDATA; APPDATA = $env:APPDATA }
    $env:LOCALAPPDATA = Join-Path $profil 'Local'; $env:APPDATA = Join-Path $profil 'Roaming'
    $env:SPACEFILES_SANS_REGISTRE = '1'; $env:SPACEFILES_PIPE = $suffixe; $env:SPACEFILES_INACTIVITE_MS = '3000'
    if ($fauxLanceur) { $env:SPACEFILES_FAUX_LANCEUR = $lanceurFaux }
    $p = Start-Process -FilePath $exeCopie -ArgumentList '--resident' -PassThru -WindowStyle Hidden
    $env:LOCALAPPDATA = $ancien.LOCALAPPDATA; $env:APPDATA = $ancien.APPDATA
    Remove-Item Env:\SPACEFILES_PIPE, Env:\SPACEFILES_SANS_REGISTRE, Env:\SPACEFILES_INACTIVITE_MS, Env:\SPACEFILES_FAUX_LANCEUR -ErrorAction SilentlyContinue
    $p
}
function Pids-De-Test([int]$residentId) {
    @(Get-Process -ErrorAction SilentlyContinue | Where-Object { $_.Path -eq $exeCopie -and $_.Id -ne $residentId } | ForEach-Object { [uint32]$_.Id })
}
function Fermer-Fenetres-De-Test([int]$residentId) {
    $pids = Pids-De-Test $residentId
    if ($pids.Count -eq 0) { return 0 }
    $n = 0
    foreach ($h in [Fen]::Liste('SpaceFiles', [uint32[]]$pids)) { [void][Fen]::PostMessage($h, 0x10, [IntPtr]::Zero, [IntPtr]::Zero); $n++ }
    $fin = [Diagnostics.Stopwatch]::StartNew()
    while ($fin.ElapsedMilliseconds -lt 8000 -and (Pids-De-Test $residentId).Count -gt 0) { Start-Sleep -Milliseconds 100 }
    # Reste : processus de TEST (copie sous %TEMP%) dont la fenetre n'a pas repondu
    foreach ($p in (Pids-De-Test $residentId)) { Stop-Process -Id $p -Force -ErrorAction SilentlyContinue }
    $n
}

# Faux SpaceNotch : un tube que personne ne doit connecter tant qu'aucune operation de fichiers n'emet
$faux = New-Object System.IO.Pipes.NamedPipeServerStream($nomNotch, [System.IO.Pipes.PipeDirection]::InOut, 2, [System.IO.Pipes.PipeTransmissionMode]::Byte, [System.IO.Pipes.PipeOptions]::Asynchronous)
$attenteFaux = $faux.WaitForConnectionAsync()

$resident = Demarrer-Resident
$idRes = $resident.Id
try {
    Start-Sleep -Seconds 3
    "== Resident de test pid $idRes, tube \\.\pipe\$nomTube"
    Ok (Tube-Present) 'le tube du resident existe'
    $threads0 = (Get-Process -Id $idRes).Threads.Count
    $prive0 = Prive $idRes
    "Memoire privee au repos, avant toute requete : {0:N1} Mo ({1} fils)" -f $prive0, $threads0

    "== 1. hello"
    $r = [Eco]::Appeler($nomTube, '{"v":1,"id":"h1","from":"essai","kind":"call","name":"hello"}', 3000)
    $r
    Ok ($r -match '"app":"spacefiles"' -and $r -match '"protocol":1' -and $r -match '"search.query"' -and $r -match '"explorer.open"') 'hello : protocol 1, capacites search.query et explorer.open'
    $r = [Eco]::Appeler($nomTube, '{"v":2,"id":"h2","from":"essai","kind":"call","name":"hello"}', 3000)
    Ok ($r -match '"code":"version"') 'v:2 -> erreur version'
    $r = [Eco]::Appeler($nomTube, '{"v":1,"id":"h3","from":"essai","kind":"call","name":"zzz.inconnu"}', 3000)
    Ok ($r -match '"code":"inconnu"') 'name inconnu -> erreur inconnu'

    Ok ((Get-Process -Id $idRes).Threads.Count -eq $threads0) "apres 3 appels, aucun fil de plus : $((Get-Process -Id $idRes).Threads.Count) fils (au repos : $threads0 ; aucun fil d'emission progress, aucune operation de fichiers)"

    "== 2. Concurrence : 8 clients (4 search.query, 3 hello, 1 muet qui reste connecte)"
    # Chargement de l'index d'abord (la premiere requete repond loading) : les 4 recherches mesurees sont des recherches reelles
    for ($k = 0; $k -lt 400; $k++) {
        $r = [Eco]::Appeler($nomTube, '{"v":1,"id":"w","from":"essai","kind":"call","name":"search.query","args":{"q":"windows"}}', 3000)
        if ($r -notmatch 'loading') { break }; Start-Sleep -Milliseconds 25
    }
    Ok ($r -match '"name"') 'index charge pour la mesure'
    $muet = New-Object System.IO.Pipes.NamedPipeClientStream('.', $nomTube, [System.IO.Pipes.PipeDirection]::InOut, [System.IO.Pipes.PipeOptions]::Asynchronous)
    $muet.Connect(3000)
    $swMuet = [Diagnostics.Stopwatch]::StartNew()
    $buf = New-Object byte[] 16; $lectureMuet = $muet.ReadAsync($buf, 0, 16)
    $lignes = @(1..4 | ForEach-Object { '{"v":1,"id":"s' + $_ + '","from":"essai","kind":"call","name":"search.query","args":{"q":"windows"}}' }) + @(1..3 | ForEach-Object { '{"v":1,"id":"c' + $_ + '","from":"essai","kind":"call","name":"hello"}' })
    $sw = [Diagnostics.Stopwatch]::StartNew()
    $res = [Eco]::Concurrence($nomTube, [string[]]$lignes)
    $total = $sw.ElapsedMilliseconds
    $res | ForEach-Object { $_.Substring(0, [Math]::Min(90, $_.Length)) }
    $servis = @($res | Where-Object { $_ -notmatch '^ERREUR' -and $_ -match '"kind":"reply"' }).Count
    "7 clients non muets servis : $servis / 7 en $total ms pendant que le muet est connecte ($($swMuet.ElapsedMilliseconds) ms de connexion)"
    Ok ($servis -eq 7 -and $total -lt 2500 -and -not $lectureMuet.IsCompleted) 'les 7 clients non muets sont servis, le muet est encore connecte'
    $coupe = $lectureMuet.Wait(7000)
    "muet coupe apres $($swMuet.ElapsedMilliseconds) ms (delai d'essai : 3000 ms)"
    Ok ($coupe -and $swMuet.ElapsedMilliseconds -ge 2500 -and $swMuet.ElapsedMilliseconds -lt 7000) 'le muet est coupe apres le delai d essai'
    $muet.Dispose()

    "== 3. explorer.open (lancement de fenetre REMPLACE par un faux lanceur : aucune fenetre n'est ouverte)"
    function Lignes-Lanceur { if (Test-Path $lanceurFaux) { @(Get-Content $lanceurFaux -Encoding UTF8) } else { @() } }
    function Attendre-Lignes([int]$n) {
        $t = [Diagnostics.Stopwatch]::StartNew()
        while ($t.ElapsedMilliseconds -lt 5000 -and @(Lignes-Lanceur).Count -lt $n) { Start-Sleep -Milliseconds 50 }
        Lignes-Lanceur
    }
    $r = [Eco]::Appeler($nomTube, '{"v":1,"id":"p0","from":"essai","kind":"call","name":"explorer.open","args":{}}', 3000)
    Ok ($r -match '"code":"champ_manquant"') 'sans path -> champ_manquant'
    $dossierBrut = Join-Path $profil 'Dossier de test'
    $dossierTest = $dossierBrut.Replace('\', '\\')
    $r = [Eco]::Appeler($nomTube, '{"v":1,"id":"o1","from":"essai","kind":"call","name":"explorer.open","args":{"path":"' + $dossierTest + '"}}', 3000)
    $r
    Ok ($r -match '"opened":true') 'dossier du faux profil -> {opened:true}'
    $l = @(Attendre-Lignes 1)
    Ok ($l.Count -eq 1 -and $l[0] -ceq "--ouvrir `"$dossierBrut`"") "le lanceur a recu exactement --ouvrir <dossier> (le chemin n'est jamais passe nu : $($l[0]))"
    $r = [Eco]::Appeler($nomTube, '{"v":1,"id":"o2","from":"essai","kind":"call","name":"explorer.open","args":{"path":"' + $dossierTest + '","reveal":true}}', 3000)
    $l = @(Attendre-Lignes 2)
    Ok ($r -match '"opened":true' -and $l.Count -eq 2 -and $l[1] -ceq "--montrer `"$dossierBrut`"") 'reveal:true -> --montrer <chemin>'
    $r = [Eco]::Appeler($nomTube, '{"v":1,"id":"o3","from":"essai","kind":"call","name":"explorer.open","args":{"path":"C:/Windows/notepad.exe"}}', 3000)
    $l = @(Attendre-Lignes 3)
    Ok ($r -match '"opened":true' -and $l.Count -eq 3 -and $l[2] -ceq '--ouvrir "C:\Windows\notepad.exe"') 'un fichier (et des / normalises en \) : toujours --ouvrir <chemin>, jamais lance par le resident'
    $r = [Eco]::Appeler($nomTube, '{"v":1,"id":"o4","from":"essai","kind":"call","name":"explorer.open","args":{"path":"C:\\"}}', 3000)
    $l = @(Attendre-Lignes 4)
    Ok ($r -match '"opened":true' -and $l.Count -eq 4 -and $l[3] -ceq '--ouvrir "C:\\"') 'racine C:\ : antislash final double dans l''argument'
    Ok ((Pids-De-Test $idRes).Count -eq 0) 'aucun processus SpaceFiles de test lance (le faux lanceur n''affiche rien)'
    Ok (-not ((Get-Content $lanceurFaux -Raw) -match 'explorer\.exe')) 'jamais explorer.exe'
    # Erreurs : aucune ne doit atteindre le lanceur
    $invalides = [ordered]@{
        'chemin relatif'  = '{"v":1,"id":"e1","from":"essai","kind":"call","name":"explorer.open","args":{"path":"dossier\\sous"}}'
        'calc.exe'        = '{"v":1,"id":"e2","from":"essai","kind":"call","name":"explorer.open","args":{"path":"calc.exe"}}'
        'https://x'       = '{"v":1,"id":"e3","from":"essai","kind":"call","name":"explorer.open","args":{"path":"https://x"}}'
        'NUL dans le chemin' = '{"v":1,"id":"e4","from":"essai","kind":"call","name":"explorer.open","args":{"path":"C:\\a\u0000b"}}'
        'bidi'            = '{"v":1,"id":"e5","from":"essai","kind":"call","name":"explorer.open","args":{"path":"C:\\a\u202eb"}}'
        'flux C:\a:b'     = '{"v":1,"id":"e6","from":"essai","kind":"call","name":"explorer.open","args":{"path":"C:\\a:b"}}'
        'peripherique'    = '{"v":1,"id":"e7","from":"essai","kind":"call","name":"explorer.open","args":{"path":"\\\\.\\pipe\\x"}}'
        'reveal non booleen' = '{"v":1,"id":"e8","from":"essai","kind":"call","name":"explorer.open","args":{"path":"C:\\","reveal":"oui"}}'
        'peripherique en /' = '{"v":1,"id":"e12","from":"essai","kind":"call","name":"explorer.open","args":{"path":"\\\\./pipe/x"}}'
        'espace de noms \\?\' = '{"v":1,"id":"e13","from":"essai","kind":"call","name":"explorer.open","args":{"path":"\\\\?\\C:\\x"}}'
        'C:\NUL'          = '{"v":1,"id":"e14","from":"essai","kind":"call","name":"explorer.open","args":{"path":"C:\\NUL"}}'
        'C:\x\COM1.txt'   = '{"v":1,"id":"e15","from":"essai","kind":"call","name":"explorer.open","args":{"path":"C:\\x\\COM1.txt"}}'
        'U+2028'          = '{"v":1,"id":"e16","from":"essai","kind":"call","name":"explorer.open","args":{"path":"C:\\a\u2028b"}}'
        'U+200B'          = '{"v":1,"id":"e17","from":"essai","kind":"call","name":"explorer.open","args":{"path":"C:\\a\u200bb"}}'
    }
    foreach ($k in $invalides.Keys) {
        $r = [Eco]::Appeler($nomTube, $invalides[$k], 3000)
        Ok ($r -match '"error":\{"code":"(invalide|hors_bornes)"') "$k -> erreur (invalide/hors_bornes)"
    }
    $long = '{"v":1,"id":"e9","from":"essai","kind":"call","name":"explorer.open","args":{"path":"C:\\' + ('d' * 32100) + '"}}'
    $r = [Eco]::Appeler($nomTube, $long, 3000)
    Ok ($r -match '"code":"hors_bornes"') "chemin de 32 100 caracteres (ligne de $($long.Length) octets) -> hors_bornes"
    $tl = '{"v":1,"id":"e10","from":"essai","kind":"call","name":"explorer.open","args":{"path":"C:\\' + ('d' * 33000) + '"}}'
    $r = [Eco]::TropLong($nomTube, $tl)
    Ok ($r -match '"code":"trop_long"' -and $r -match 'ferme=True') "chemin de 33 000 caracteres (ligne de $($tl.Length) octets > 32 Ko) -> trop_long puis connexion fermee"
    $r = [Eco]::TropLong($nomTube, '{"v":1,"id":"e11","from":"essai","kind":"call","name":"search.query","args":{"q":"' + ('a' * 5000) + '"}}')
    Ok ($r -match '"code":"trop_long"' -and $r -match 'ferme=True') 'search.query de 5 000 octets -> trop_long puis connexion fermee (la borne de 32 Ko est propre a explorer.open)'
    Start-Sleep -Milliseconds 500
    Ok (@(Lignes-Lanceur).Count -eq 4) 'aucun appel invalide n''a atteint le lanceur (4 lignes seulement, celles des appels valides)'
    # Limite de debit : le seau de rafale est plein apres 10 s de calme (5 ouvertures), puis une toutes les 2 s
    Start-Sleep -Seconds 10
    $avantRafale = @(Lignes-Lanceur).Count
    $swR = [Diagnostics.Stopwatch]::StartNew()
    $rafale = [Eco]::Rafale($nomTube, '{"v":1,"id":"r","from":"essai","kind":"call","name":"explorer.open","args":{"path":"' + $dossierTest + '"}}', 20)
    "20 appels en $($swR.ElapsedMilliseconds) ms"
    $acceptes = @($rafale | Where-Object { $_ -match '"opened":true' }).Count
    $refuses = @($rafale | Where-Object { $_ -match '"code":"trop_vite"' }).Count
    "acceptes : $acceptes, trop_vite : $refuses"
    Ok ($acceptes -eq 5 -and ($acceptes + $refuses) -eq 20) '20 appels en rafale apres 10 s de calme : exactement 5 acceptes, le reste trop_vite'
    $l = @(Attendre-Lignes ($avantRafale + 5))
    Ok ($l.Count -eq $avantRafale + 5) "le lanceur a ete appele 5 fois, pas plus ($($l.Count - $avantRafale))"
    $r = [Eco]::Appeler($nomTube, '{"v":1,"id":"r2","from":"essai","kind":"call","name":"explorer.open","args":{"path":"' + $dossierTest + '"}}', 3000)
    Ok ($r -match '"code":"trop_vite"') 'seau vide : l''appel suivant est refuse'
    Start-Sleep -Milliseconds 2200
    $r = [Eco]::Appeler($nomTube, '{"v":1,"id":"r3","from":"essai","kind":"call","name":"explorer.open","args":{"path":"' + $dossierTest + '"}}', 3000)
    Ok ($r -match '"opened":true') 'une ouverture revient 2 s plus tard (recharge du seau)'
    Ok ((Pids-De-Test $idRes).Count -eq 0) 'toujours aucune fenetre de test ouverte'

    "== 4. progress : le resident n'emet rien tant qu'aucune operation de fichiers ne tourne"
    Ok (-not $attenteFaux.IsCompleted) 'le faux SpaceNotch n''a recu aucune connexion du resident (pas d''operation : pas d''appel)'
    'Les fils sont comptes avant le chargement de l''index (section 1) : le chargement en ajoute pour la recherche, pas pour progress.'
    '(la copie reelle emettant running puis done est prouvee par le test Rust copie_reelle_via_le_suivi, pas par ce script : une copie demande la fenetre, donc la souris)'

    "== 5. $Mille appels hello de suite, connexion courte (client .NET)"
    [void][Eco]::Mille($nomTube, 50)
    $sw = [Diagnostics.Stopwatch]::StartNew()
    $d = [Eco]::Mille($nomTube, $Mille)
    $sw.Stop()
    "{0} appels : total {1:N0} ms ; p50 {2:N2} ms ; p95 {3:N2} ms ; max {4:N2} ms" -f $Mille, $sw.ElapsedMilliseconds, (Pct $d 0.5), (Pct $d 0.95), ($d | Measure-Object -Maximum).Maximum
    Ok ($sw.ElapsedMilliseconds -lt 1000) "$Mille appels en moins d'1 s (cible du masterplan ; mesure ci-dessus)"

    "== 6. Manifeste et shared.json"
    $manifeste = Join-Path $profil 'Local\Eco\apps\spacefiles.json'
    Ok (Test-Path $manifeste) 'manifeste ecrit dans le dossier Eco du FAUX profil'
    $m = Get-Content $manifeste -Raw -Encoding UTF8 | ConvertFrom-Json
    $m | ConvertTo-Json -Compress
    Ok ($m.id -eq 'spacefiles' -and $m.name -eq 'SpaceFiles' -and $m.protocol -eq 1 -and $m.pipe -eq $nomTube -and $m.exe -eq $exeCopie -and $m.version -and $m.settingsLink -eq 'spacefiles://settings' -and ($m.capabilities -join ',') -eq 'search.query,explorer.open') 'manifeste conforme (id, name, version, exe, pipe, protocol, capabilities, settingsLink)'
    Ok ($m.id -cmatch '^[a-z][a-z0-9-]*$') 'id conforme au motif du schema'
    $reelApres = if (Test-Path $reelEco) { (Get-Item $reelEco).LastWriteTimeUtc.Ticks } else { 'absent' }
    Ok ("$reelApres" -eq "$eco0") 'le dossier Eco REEL n''a pas ete touche (existence et horodatage)'
    $journal = Join-Path $profil 'Local\SpaceFiles\journal.txt'
    $texteJ = Get-Content $journal -Raw -Encoding UTF8
    Ok ($texteJ -match 'eco shared\.json lu : theme light, accent #112233, mouvement reduced, langue en-US, avis de theme oui, 1 raccourci\(s\), 1 champ\(s\) inconnu\(s\) conserve\(s\)') 'shared.json avec BOM lu par le resident (journal : une ligne d''etat)'

    if ($AvecFenetres) {
        "== 3b. UN essai reel d'ouverture de fenetre (-AvecFenetres) : une fenetre de test va apparaitre quelques secondes"
        Stop-Process -Id $idRes -Force; [void]$resident.WaitForExit(5000)
        $resident = Demarrer-Resident $false; $idRes = $resident.Id
        Start-Sleep -Seconds 3
        $r = [Eco]::Appeler($nomTube, '{"v":1,"id":"w1",r"from":"essai","kind":"call","name":"explorer.open","args":{"path":"' + $dossierTest + '"}}', 3000)
        Ok ($r -match '"opened":true') 'resident SANS faux lanceur : {opened:true}'
        $fin = [Diagnostics.Stopwatch]::StartNew(); $vues = @()
        while ($fin.ElapsedMilliseconds -lt 15000) {
            $pids = Pids-De-Test $idRes
            if ($pids.Count -gt 0) { $vues = @([Fen]::Liste('SpaceFiles', [uint32[]]$pids)); if ($vues.Count -ge 1) { break } }
            Start-Sleep -Milliseconds 100
        }
        "fenetre SpaceFiles du processus de test apparue apres $($fin.ElapsedMilliseconds) ms : $($vues.Count) fenetre(s)"
        Ok ($vues.Count -ge 1) 'une fenetre SpaceFiles du processus de test est apparue (EnumWindows + pid)'
        $fermees = Fermer-Fenetres-De-Test $idRes
        Ok ((Pids-De-Test $idRes).Count -eq 0) "fenetre de test fermee par WM_CLOSE ($fermees envoyee(s), a cette fenetre seulement)"
    } else {
        '(3b ignore : aucun essai reel de fenetre sans -AvecFenetres)'
    }

    "== 7. Tuer le resident de test, puis le relancer"
    Stop-Process -Id $idRes -Force
    [void]$resident.WaitForExit(5000)
    Ok (-not (Tube-Present)) 'le tube a disparu avec le resident'
    Ok (Test-Path $manifeste) 'le manifeste n''est pas supprime a l''arret (le Hub l''affichera arrete)'
    $sw = [Diagnostics.Stopwatch]::StartNew(); $erreur = $null
    try { [void][Eco]::Appeler($nomTube, '{"v":1,"id":"x","kind":"call","name":"hello"}', 300) } catch { $erreur = $_.Exception.GetType().Name }
    "appel .NET apres l'arret : $erreur apres $($sw.ElapsedMilliseconds) ms (le client Rust rend Absent tout de suite : test tube_absent_rend_absent_vite)"
    Ok ($erreur -and $sw.ElapsedMilliseconds -lt 1500) 'un client borne a 300 ms n''attend pas apres la mort du resident'
    # Fichier abime pour le redemarrage : valeurs par defaut sans erreur
    [IO.File]::WriteAllBytes($partage, [byte[]](0x7B, 0x22, 0x74, 0x68, 0x00, 0xFF))
    $resident = Demarrer-Resident
    $idRes = $resident.Id
    Start-Sleep -Seconds 3
    $r = [Eco]::Appeler($nomTube, '{"v":1,"id":"h9","from":"essai","kind":"call","name":"hello"}', 3000)
    Ok ($r -match '"id":"h9"' -and $r -match '"app":"spacefiles"') 'apres redemarrage le service revient'
    $texteJ = Get-Content $journal -Raw -Encoding UTF8
    Ok ($texteJ -match 'eco shared\.json abime : valeurs par defaut') 'shared.json abime : valeurs par defaut, une ligne de journal, le resident demarre'

    "== 8. Memoire et fils au repos"
    Start-Sleep -Seconds 2
    $prive1 = Prive $idRes; $threads1 = (Get-Process -Id $idRes).Threads.Count
    "Resident de test redemarre, au repos : {0:N1} Mo prives, {1} fils (premier demarrage : {2:N1} Mo, {3} fils)" -f $prive1, $threads1, $prive0, $threads0
    "Memoire privee au repos (premier demarrage) : {0:N1} Mo ; fils : {1}" -f $prive0, $threads0

    "== 9. Arret propre"
    $h = [Fen]::FindWindowEx([IntPtr]::Zero, [IntPtr]::Zero, 'SpaceFilesResidentEssai', [NullString]::Value)
    $envoye = $false
    while ($h -ne [IntPtr]::Zero) {
        if ([Fen]::Pid($h) -eq $idRes) { [void][Fen]::PostMessage($h, 0x10, [IntPtr]::Zero, [IntPtr]::Zero); $envoye = $true; break }
        $h = [Fen]::FindWindowEx([IntPtr]::Zero, $h, 'SpaceFilesResidentEssai', [NullString]::Value)
    }
    Ok $envoye 'WM_CLOSE envoye a la fenetre du resident de test'
    Ok ($resident.WaitForExit(6000)) 'le resident de test se termine tout seul (aucun fil orphelin ne le retient)'
    Ok (-not (Tube-Present)) 'le tube a disparu'
    "-- journal du faux profil (resident : modules eco, eco-serveur, service_recherche) :"
    $lignesJ = Get-Content $journal -Encoding UTF8 | Where-Object { $_ -match ' (eco|eco-serveur|eco-client|service_recherche) ' }
    $lignesJ | Select-Object -Last 14
    Ok (-not ($lignesJ -match 'windows|Dossier de test|calc|https|C:\\|sf-eco|zzz')) 'le journal du resident ne contient ni texte cherche, ni chemin, ni nom d appel'
    Ok ($texteJ -match 'eco-serveur service demarre : 4 instances') 'journal : service demarre avec 4 instances'
    Ok ((Get-Content $journal -Raw -Encoding UTF8) -match 'eco-serveur service arrete') 'journal : service arrete'
} finally {
    if (-not $resident.HasExited) { Stop-Process -Id $idRes -Force }
    foreach ($p in (Pids-De-Test $idRes)) { Stop-Process -Id $p -Force -ErrorAction SilentlyContinue }
    $faux.Dispose()
}
"== Bilan : $script:echecs echec(s)"
