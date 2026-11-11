// Preuve croisee du lot B de la phase 4 : le serveur Eco de SpaceNotch (C#) et le client / l'emetteur de SpaceFiles
// (Rust) l'un contre l'autre. Ne lance JAMAIS SpaceNotch : le serveur C# tourne dans CE processus, sous un nom de tube
// d'essai (Eco.spacenotch.lotb<pid>) ; le resident SpaceFiles est un resident de TEST (exe copie sous %TEMP%, faux
// profil, APPDATA / LOCALAPPDATA redirigees dans ce seul processus, SPACEFILES_SANS_REGISTRE=1, SPACEFILES_PIPE distinct,
// faux lanceur de fenetre : aucune fenetre). Ne lit ni n'ecrit les vraies donnees (la phase 0 le prouve en comparant
// avant / apres), ne touche pas au resident de l'auteur, aucune entree souris. Pas de search.query ici : il demanderait
// l'index reel (prouve au lot 4 de la phase 3).
//
// Usage (depuis le depot SpaceFiles, apres `cargo build --release`) :
//   dotnet run -c Release tools\bench\preuve-lot-b-notch.cs
#:property TargetFramework=net10.0-windows10.0.26100.0
#:property Nullable=enable
#:property ImplicitUsings=enable
#:property JsonSerializerIsReflectionEnabledByDefault=false
#:project ../../../dynamic iland windows/src/SpaceNotch.Platform.Windows/SpaceNotch.Platform.Windows.csproj

using System.Collections.Concurrent;
using System.Diagnostics;
using System.Text;
using SpaceNotch.Core.Eco;
using SpaceNotch.Platform.Windows.Eco;
using SpaceNotch.Platform.Windows.SpaceFiles;

int echecs = 0;
void Ok(bool condition, string text)
{
    Console.WriteLine((condition ? "OK    " : "ECHEC ") + text);
    if (!condition) { echecs++; }
}

static string Instantane(string dossier)
{
    // Empreinte du contenu d'un dossier reel : noms, tailles, dates d'ecriture. Lecture de metadonnees seulement.
    if (!Directory.Exists(dossier)) { return "(absent)"; }
    var sb = new StringBuilder();
    foreach (string f in Directory.EnumerateFiles(dossier, "*", SearchOption.AllDirectories).OrderBy(x => x, StringComparer.OrdinalIgnoreCase))
    {
        var i = new FileInfo(f);
        sb.Append(f).Append('|').Append(i.Length).Append('|').Append(i.LastWriteTimeUtc.Ticks).Append('\n');
    }
    return sb.ToString();
}

string local = Environment.GetEnvironmentVariable("LOCALAPPDATA")!;
string roaming = Environment.GetEnvironmentVariable("APPDATA")!;
string[] reels = [Path.Combine(local, "Eco"), Path.Combine(local, "SpaceFiles"), Path.Combine(roaming, "SpaceFiles"), Path.Combine(roaming, "SpaceNotch")];
string[] avant = reels.Select(Instantane).ToArray();
Console.WriteLine("== 0. Donnees reelles : empreinte prise (noms, tailles, dates), comparee a la fin");

string racine = Path.Combine(Path.GetTempPath(), "sf-lot-b");
if (Directory.Exists(racine)) { Directory.Delete(racine, true); }
string profil = Path.Combine(racine, "faux-profil");
Directory.CreateDirectory(Path.Combine(profil, "Local", "SpaceFiles"));
Directory.CreateDirectory(Path.Combine(profil, "Roaming", "SpaceFiles"));
string ecoRacine = Path.Combine(profil, "Local", "Eco");

string suffixeNotch = "lotb" + Environment.ProcessId;
string tubeNotch = "Eco.spacenotch." + suffixeNotch;

var sink = new Collecteur();
await using var hote = new EcoHost(
    new EcoHostOptions(ecoRacine, tubeNotch, Environment.ProcessPath!, "0.0.0-essai") { Log = m => Console.WriteLine("   [serveur C#] " + m) },
    sink);
Ok(await hote.StartAsync(), "serveur Eco de SpaceNotch (en processus, tube d'essai) ouvert");
Ok(File.Exists(Path.Combine(ecoRacine, "apps", "spacenotch.json")), "manifeste ecrit dans le faux dossier Eco (pas le vrai)");

// ------------------------------------------------------------------ 1. Rust -> C# : emetteur de SpaceFiles
Console.WriteLine("== 1. Emetteur Rust (src/eco/progression.rs) contre le serveur C# : 1000 ms de mises a jour toutes les 5 ms, puis Done");
(int code, string sortie) = Cargo("test emettre_vers_serveur_externe -- --ignored --nocapture", suffixeNotch, 1000, 0);
Console.WriteLine(Indenter(sortie));
Ok(code == 0, "cargo test (ignore) : code de sortie 0");
Ok(sortie.Contains("HELLO_OK", StringComparison.Ordinal), "le client Rust recoit le hello du serveur C# (app=spacenotch, protocole 1)");
var recus = sink.Snapshot();
Console.WriteLine($"   evenements recus par le serveur C# : {recus.Count} ; compteurs : connexions {hote.Server.Connections}, acceptes {hote.Server.EventsAccepted}, rejetes {hote.Server.Rejected}");
Ok(recus.Count >= 1 && recus[^1].Update.State == EcoProgressState.Done, "le dernier evenement recu est 'done'");
Ok(recus.Count >= 2 && recus[0].Update.State == EcoProgressState.Running, "le premier est 'running'");
if (recus.Count > 0)
{
    var running = recus.Where(r => r.Update.State == EcoProgressState.Running).ToList();
    double secondes = Math.Max(0.001, (recus[^1].At - recus[0].At).TotalSeconds);
    Console.WriteLine($"   {running.Count} 'running' recus sur {secondes:N2} s ({running.Count / secondes:N1}/s) ; l'emetteur Rust en produisait ~200/s");
    Ok(running.Count / Math.Max(1.0, secondes) <= 12, "debit coalesce cote emetteur Rust (<= 12/s recus)");
    Ok(recus.All(r => r.Update.Op.Length > 0 && r.Update.Done <= r.Update.Total || r.Update.Total == 0), "done <= total sur tous les evenements");
    bool croissant = true;
    for (int i = 1; i < running.Count; i++) { croissant &= running[i].Update.Done >= running[i - 1].Update.Done; }
    Ok(croissant, "les 'done' recus ne reculent jamais (ordre conserve)");
}

// ------------------------------------------------------------------ 2. Rust -> C# : 1000 hello
Console.WriteLine("== 2. 1000 hello du client Rust (nom de tube a casse variee pour contourner son cache de 60 s)");
long avantConnexions = hote.Server.Connections;
(code, sortie) = Cargo("test emettre_vers_serveur_externe -- --ignored --nocapture", suffixeNotch, 200, 1000);
Console.WriteLine(Indenter(sortie));
Ok(code == 0, "cargo test (ignore, 1000 hello) : code 0");
Console.WriteLine($"   connexions servies pendant ce passage : {hote.Server.Connections - avantConnexions}");
Ok(hote.Server.Connections - avantConnexions >= 1000, ">= 1000 connexions servies sans plantage ni fuite de blocage");

// ------------------------------------------------------------------ 3. C# -> Rust : resident de test
Console.WriteLine("== 3. Client C# (EcoClient / SpaceFilesOpener) contre un resident SpaceFiles de TEST");
string exeSource = Path.GetFullPath(Path.Combine(Directory.GetCurrentDirectory(), "target", "release", "spacefiles.exe"));
if (!File.Exists(exeSource)) { throw new FileNotFoundException("lancer depuis le depot SpaceFiles apres cargo build --release", exeSource); }
string exeCopie = Path.Combine(racine, "spacefiles.exe");
File.Copy(exeSource, exeCopie, true);
string suffixeSf = "lotb" + Environment.ProcessId;
string tubeSf = "Eco.spacefiles." + suffixeSf;
string fauxLanceur = Path.Combine(racine, "faux-lanceur.txt");

Process Demarrer()
{
    var s = new ProcessStartInfo(exeCopie, "--resident") { UseShellExecute = false, CreateNoWindow = true, WindowStyle = ProcessWindowStyle.Hidden };
    s.Environment["LOCALAPPDATA"] = Path.Combine(profil, "Local");
    s.Environment["APPDATA"] = Path.Combine(profil, "Roaming");
    s.Environment["SPACEFILES_SANS_REGISTRE"] = "1";
    s.Environment["SPACEFILES_PIPE"] = suffixeSf;
    s.Environment["SPACEFILES_FAUX_LANCEUR"] = fauxLanceur;
    s.Environment["SPACEFILES_LIBERATION_S"] = "120";
    s.Environment["SPACEFILES_INACTIVITE_MS"] = "3000";
    return Process.Start(s)!;
}

using Process resident = Demarrer();
try
{
    for (int k = 0; k < 100 && !EcoClient.PipeExists(tubeSf); k++) { Thread.Sleep(50); }
    Ok(EcoClient.PipeExists(tubeSf), $"tube du resident de test (pid {resident.Id}) present");

    var verifieur = new PipeServerVerifier(() => exeCopie, null);
    var client = new EcoClient(tubeSf, verifieur);
    var grant = new EspionPremierPlan();
    var ouvreur = new SpaceFilesOpener(client, grant, m => Console.WriteLine("   [ouvreur] " + m));

    string dossier = Path.Combine(racine, "dossier d'essai");
    Directory.CreateDirectory(dossier);
    string fichier = Path.Combine(dossier, "note.txt");
    File.WriteAllText(fichier, "x");

    var sw = Stopwatch.StartNew();
    SpaceFilesOpenResult r = await ouvreur.OpenInSpaceFilesAsync(dossier, false, CancellationToken.None);
    Console.WriteLine($"   ouvrir un dossier : {r} en {sw.ElapsedMilliseconds} ms ; permission de premier plan donnee au pid {string.Join(",", grant.Pids)}");
    Ok(r == SpaceFilesOpenResult.Opened, "explorer.open (dossier) accepte par le vrai resident");
    Ok(grant.Pids.SequenceEqual([(uint)resident.Id]), "AllowSetForegroundWindow vise le pid verifie du resident (et lui seul)");

    Thread.Sleep(600);
    r = await ouvreur.OpenInSpaceFilesAsync(fichier, true, CancellationToken.None);
    Ok(r == SpaceFilesOpenResult.Opened, "explorer.open (reveal d'un fichier) accepte");

    // Lecture en partage complet : le resident ecrit dans ce fichier pendant qu'on le lit.
    static string[] LireLignes(string fichier)
    {
        try
        {
            using var fs = new FileStream(fichier, FileMode.Open, FileAccess.Read, FileShare.ReadWrite | FileShare.Delete);
            using var lecteur = new StreamReader(fs);
            return lecteur.ReadToEnd().Split((char)10, StringSplitOptions.RemoveEmptyEntries | StringSplitOptions.TrimEntries);
        }
        catch (IOException)
        {
            return [];
        }
    }

    for (int k = 0; k < 100 && LireLignes(fauxLanceur).Length < 2; k++) { Thread.Sleep(50); }
    string[] lignes = LireLignes(fauxLanceur);
    Console.WriteLine("   lignes recues par le faux lanceur (aucune fenetre reelle) : " + string.Join(" || ", lignes));
    Ok(lignes.Length == 2 && lignes[0].StartsWith("--ouvrir ", StringComparison.Ordinal) && lignes[1].StartsWith("--montrer ", StringComparison.Ordinal),
        "le resident a decide d'ouvrir (--ouvrir) puis de montrer (--montrer), avec les chemins demandes");

    r = await ouvreur.OpenInSpaceFilesAsync("relatif\\chemin", false, CancellationToken.None);
    Ok(r == SpaceFilesOpenResult.InvalidPath, "chemin relatif refuse avant tout envoi");

    // Verifieur qui n'est pas le bon exe : le client doit refuser sans rien envoyer.
    var mefiant = new SpaceFilesOpener(new EcoClient(tubeSf, new PipeServerVerifier(() => Environment.ProcessPath, null)), new EspionPremierPlan());
    r = await mefiant.OpenInSpaceFilesAsync(dossier, false, CancellationToken.None);
    Ok(r == SpaceFilesOpenResult.Untrusted, "serveur dont l'exe n'est pas celui du manifeste : Untrusted, rien envoye");

    // ---------------------------------------------------------------- 4. 1000 hello C# -> Rust
    Console.WriteLine("== 4. 1000 hello du client C# (une connexion courte chacun, verification du serveur comprise)");
    var durees = new List<double>();
    int echecsHello = 0;
    for (int i = 0; i < 1000; i++)
    {
        var c = new EcoClient(tubeSf, verifieur);
        var t = Stopwatch.StartNew();
        var (outcome, connexion) = await c.ConnectAsync(500, CancellationToken.None);
        if (connexion is null) { echecsHello++; continue; }
        await using (connexion)
        {
            var (o2, _, _) = await c.HelloAsync(connexion, CancellationToken.None);
            if (o2 != EcoOutcome.Ok) { echecsHello++; }
        }
        durees.Add(t.Elapsed.TotalMilliseconds);
    }
    durees.Sort();
    Console.WriteLine($"   echecs {echecsHello}/1000 ; p50 {durees[durees.Count / 2]:N2} ms, p95 {durees[(int)(durees.Count * 0.95)]:N2} ms, max {durees[^1]:N2} ms");
    Ok(echecsHello == 0, "1000 hello C# -> Rust sans echec");
    Ok(!resident.HasExited, "le resident de test est toujours vivant");

    // ---------------------------------------------------------------- 5. coupures
    Console.WriteLine("== 5. Coupures");
    // 5a. le serveur C# disparait pendant que Rust emet : Rust ne plante pas.
    string tubeAEteindre = "lotb-eteint" + Environment.ProcessId;
    var sink2 = new Collecteur();
    var hote2 = new EcoHost(new EcoHostOptions(Path.Combine(racine, "Eco2"), "Eco.spacenotch." + tubeAEteindre, Environment.ProcessPath!, "0.0.0-essai"), sink2);
    await hote2.StartAsync();
    Task<(int, string)> rust = Task.Run(() => Cargo("test emettre_vers_serveur_externe -- --ignored --nocapture", "lotb-eteint" + Environment.ProcessId, 4000, 0));
    await Task.Delay(1500);
    await hote2.DisposeAsync();
    (int code2, string sortie2) = await rust;
    Console.WriteLine($"   serveur C# arrete a ~1,5 s d'une emission de 4 s : {sink2.Snapshot().Count} evenements recus avant ; code de sortie de l'emetteur Rust {code2}");
    Ok(code2 == 0, "l'emetteur Rust termine proprement quand le serveur disparait (pas de panique, pas de blocage)");

    // 5b. le resident est tue pendant qu'un client C# attend.
    var attente = ouvreur.OpenInSpaceFilesAsync(dossier, false, CancellationToken.None);
    resident.Kill();
    sw.Restart();
    SpaceFilesOpenResult apres = await ouvreur.OpenInSpaceFilesAsync(dossier, false, CancellationToken.None);
    _ = await attente;
    Console.WriteLine($"   apres kill du resident de test : {apres} en {sw.ElapsedMilliseconds} ms");
    Ok(apres != SpaceFilesOpenResult.Opened && sw.ElapsedMilliseconds < 700, "resident en train de mourir : jamais Opened, jamais d'exception, dans le budget (l'appelant se replie sur l'Explorateur)");
    resident.WaitForExit(5000);
    sw.Restart();
    SpaceFilesOpenResult mort = await ouvreur.OpenInSpaceFilesAsync(dossier, false, CancellationToken.None);
    Console.WriteLine($"   resident termine : {mort} en {sw.ElapsedMilliseconds} ms");
    Ok(mort == SpaceFilesOpenResult.Absent && sw.ElapsedMilliseconds < 200, "resident termine : Absent tout de suite");
}
finally
{
    if (!resident.HasExited) { resident.Kill(); }
}

// ------------------------------------------------------------------ 6. cout au repos et sous rafale
Console.WriteLine("== 6. Cout du serveur C# : fils et memoire avant / apres 10 000 evenements (client C# factice)");
var moi = Process.GetCurrentProcess();
moi.Refresh();
long mem0 = moi.PrivateMemorySize64;
int fils0 = moi.Threads.Count;
{
    // Ecriture brute d'evenements sur une connexion ordinaire : un evenement n'a pas de reponse.
    using var flux = new System.IO.Pipes.NamedPipeClientStream(".", tubeNotch, System.IO.Pipes.PipeDirection.InOut, System.IO.Pipes.PipeOptions.Asynchronous);
    flux.Connect(1000);
    for (int i = 0; i < 10000; i++)
    {
        byte[] ligne = Encoding.UTF8.GetBytes("{\"v\":1,\"from\":\"spacefiles\",\"id\":\"e\",\"kind\":\"event\",\"name\":\"progress\",\"args\":{\"op\":\"1-1\",\"state\":\"running\",\"done\":" + i + ",\"total\":10000}}\n");
        await flux.WriteAsync(ligne);
    }
    await flux.FlushAsync();
}
await Task.Delay(1500);
moi.Refresh();
Console.WriteLine($"   memoire privee {mem0 / 1024 / 1024} Mo -> {moi.PrivateMemorySize64 / 1024 / 1024} Mo ; fils {fils0} -> {moi.Threads.Count} ; evenements acceptes cumules {hote.Server.EventsAccepted}, rejetes {hote.Server.Rejected}");
Ok(moi.Threads.Count <= fils0 + 4, "pas de fils qui s'accumulent");

await hote.DisposeAsync();
Ok(!EcoClient.PipeExists(tubeNotch), "apres l'arret, le tube de SpaceNotch n'existe plus");

// ------------------------------------------------------------------ 0 bis. donnees reelles
string[] apres0 = reels.Select(Instantane).ToArray();
for (int i = 0; i < reels.Length; i++)
{
    Ok(avant[i] == apres0[i], "donnees reelles inchangees : " + reels[i]);
}

Console.WriteLine(echecs == 0 ? "== TOUT EST OK" : $"== {echecs} ECHEC(S)");
return echecs == 0 ? 0 : 1;

static string Indenter(string s) => string.Join("\n", s.Split('\n').Select(l => "   | " + l.TrimEnd()));

static (int Code, string Sortie) Cargo(string arguments, string cible, int dureeMs, int hellos)
{
    var s = new ProcessStartInfo("cargo", arguments) { UseShellExecute = false, CreateNoWindow = true, RedirectStandardOutput = true, RedirectStandardError = true };
    s.Environment["PATH"] = Environment.GetEnvironmentVariable("PATH") + ";" + Path.Combine(Environment.GetEnvironmentVariable("USERPROFILE")!, ".cargo", "bin");
    s.Environment["SPACEFILES_ECO_CIBLE_ESSAI"] = "Eco.spacenotch." + cible.Replace("Eco.spacenotch.", "", StringComparison.Ordinal);
    s.Environment["SPACEFILES_ECO_DUREE_MS"] = dureeMs.ToString(System.Globalization.CultureInfo.InvariantCulture);
    s.Environment["SPACEFILES_ECO_HELLOS"] = hellos.ToString(System.Globalization.CultureInfo.InvariantCulture);
    using Process p = Process.Start(s)!;
    string sortie = p.StandardOutput.ReadToEnd();
    string erreur = p.StandardError.ReadToEnd();
    p.WaitForExit();
    var lignes = (sortie + erreur).Split('\n').Where(l => l.Contains("HELLO") || l.Contains("EMIS") || l.Contains("test result") || l.Contains("panicked") || l.Contains("error")).ToArray();
    return (p.ExitCode, string.Join("\n", lignes));
}

sealed class Collecteur : IEcoEventSink
{
    private readonly ConcurrentQueue<(DateTime At, EcoProgressUpdate Update)> _items = new();
    public void OnProgress(EcoProgressUpdate update) => _items.Enqueue((DateTime.UtcNow, update));
    public List<(DateTime At, EcoProgressUpdate Update)> Snapshot() => [.. _items];
}

sealed class EspionPremierPlan : IForegroundGrant
{
    public List<uint> Pids { get; } = [];
    public bool Allow(uint processId) { Pids.Add(processId); return true; }
}
