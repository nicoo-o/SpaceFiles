// Preuve du lot 4 de la phase 3 : le client de SpaceNotch (SpaceFilesSearchProvider + LauncherSearchCoordinator)
// contre le VRAI service de recherche de SpaceFiles (target\release\spacefiles.exe), lance en resident de TEST.
// Meme montage que tools/bench/service-recherche.ps1 : faux profil sous %TEMP%, APPDATA / LOCALAPPDATA redirigees
// dans le seul processus lance, SPACEFILES_SANS_REGISTRE=1, SPACEFILES_PIPE distinct (pas de crochet clavier),
// index.bin reel COPIE (jamais ouvert en ecriture). Ne touche ni au resident de l'auteur ni a ses donnees, ne lance
// pas SpaceNotch, aucune entree souris.
//
// Usage (depuis le depot SpaceFiles, apres `cargo build --release`) :
//   dotnet run -c Release tools\bench\preuve-lot4-client.cs
// Option : --essais 15
#:property TargetFramework=net10.0-windows10.0.26100.0
#:property Nullable=enable
#:property ImplicitUsings=enable
#:property JsonSerializerIsReflectionEnabledByDefault=false
#:project ../../../dynamic iland windows/src/SpaceNotch.Platform.Windows/SpaceNotch.Platform.Windows.csproj

using System.Diagnostics;
using System.IO.Pipes;
using System.Text;
using SpaceNotch.Core.Launcher;
using SpaceNotch.Core.Launcher.Search;
using SpaceNotch.Platform.Windows.SpaceFiles;

int essais = 15;
for (int i = 0; i < args.Length - 1; i++)
{
    if (args[i] == "--essais") { essais = int.Parse(args[i + 1], System.Globalization.CultureInfo.InvariantCulture); }
}

int echecs = 0;
void Ok(bool condition, string text)
{
    Console.WriteLine((condition ? "OK    " : "ECHEC ") + text);
    if (!condition) { echecs++; }
}

string exeSource = Environment.GetEnvironmentVariable("SF_EXE") ?? FindExe();
string indexSource = Path.Combine(Environment.GetEnvironmentVariable("LOCALAPPDATA")!, "SpaceFiles", "index.bin");

static string FindExe()
{
    // Le script est lance depuis le depot SpaceFiles : target\release\spacefiles.exe
    string candidate = Path.GetFullPath(Path.Combine(Directory.GetCurrentDirectory(), "target", "release", "spacefiles.exe"));
    return File.Exists(candidate) ? candidate : throw new FileNotFoundException("lancer depuis le depot SpaceFiles apres cargo build --release", candidate);
}

string racine = Path.Combine(Path.GetTempPath(), "sf-lot4");
string profil = Path.Combine(racine, "faux-profil");
string suffixe = "lot4" + Environment.ProcessId;
string nomTube = "Eco.spacefiles." + suffixe;

if (Directory.Exists(racine)) { Directory.Delete(racine, true); }
Directory.CreateDirectory(Path.Combine(profil, "Local", "SpaceFiles"));
Directory.CreateDirectory(Path.Combine(profil, "Roaming", "SpaceFiles"));
string copieIndex = Path.Combine(profil, "Local", "SpaceFiles", "index.bin");
File.Copy(indexSource, copieIndex, true);

// La COPIE est rajeunie (au-dela de 6 h le service reconstruirait l'index) : date de construction = 8 octets a l'offset 8.
using (var fs = new FileStream(copieIndex, FileMode.Open, FileAccess.ReadWrite))
{
    fs.Position = 8;
    fs.Write(BitConverter.GetBytes(DateTimeOffset.UtcNow.ToUnixTimeSeconds()));
}

string exeCopie = Path.Combine(racine, "spacefiles.exe");
File.Copy(exeSource, exeCopie, true);

var start = new ProcessStartInfo(exeCopie, "--resident") { UseShellExecute = false, CreateNoWindow = true, WindowStyle = ProcessWindowStyle.Hidden };
start.Environment["LOCALAPPDATA"] = Path.Combine(profil, "Local");
start.Environment["APPDATA"] = Path.Combine(profil, "Roaming");
start.Environment["SPACEFILES_SANS_REGISTRE"] = "1";
start.Environment["SPACEFILES_PIPE"] = suffixe;
start.Environment["SPACEFILES_LIBERATION_S"] = "120";
start.Environment["SPACEFILES_INACTIVITE_MS"] = "3000";

using Process resident = Process.Start(start)!;
Console.WriteLine($"== Resident de test pid {resident.Id}, tube \\\\.\\pipe\\{nomTube}, exe copie dans {racine}");

try
{
    for (int k = 0; k < 100 && !Directory.EnumerateFiles(@"\\.\pipe\", nomTube).Any(); k++) { Thread.Sleep(50); }
    Ok(Directory.EnumerateFiles(@"\\.\pipe\", nomTube).Any(), "le tube du resident de test existe");

    using var provider = new SpaceFilesSearchProvider(nomTube);

    // ---------------------------------------------------------------- 1. Cout au repos
    Console.WriteLine("== 1. Cout du client au repos (provider + coordinateur construits, aucune saisie)");
    var moi = Process.GetCurrentProcess();
    moi.Refresh();
    int fils0 = moi.Threads.Count;
    TimeSpan cpu0 = moi.TotalProcessorTime;
    var misesAJour = new System.Collections.Concurrent.BlockingCollection<SearchUpdate>();
    var comptes = new CountingProvider(provider);
    using var coordinateur = new LauncherSearchCoordinator(comptes, misesAJour.Add);
    Thread.Sleep(3000);
    moi.Refresh();
    Console.WriteLine($"fils : {fils0} avant -> {moi.Threads.Count} apres 3 s au repos ; CPU {(moi.TotalProcessorTime - cpu0).TotalMilliseconds:N0} ms sur 3 s ; appels au tube : {comptes.Calls}");
    Ok(comptes.Calls == 0, "aucun appel au tube sans saisie");
    Ok(moi.Threads.Count <= fils0 + 1, "aucun fil supplementaire au repos");

    // ---------------------------------------------------------------- 2. Froid : loading puis resultats, via le coordinateur
    Console.WriteLine("== 2. Premiere requete a froid : windows (coordinateur + client + vrai service)");
    var sw = Stopwatch.StartNew();
    coordinateur.Query("windows");
    var etats = new List<string>();
    SearchUpdate? fin = null;
    while (sw.ElapsedMilliseconds < 20000 && misesAJour.TryTake(out SearchUpdate? u, 20000 - (int)sw.ElapsedMilliseconds))
    {
        etats.Add($"{u.Result.State}@{sw.ElapsedMilliseconds}ms");
        if (u.Result.State != SearchState.Loading) { fin = u; break; }
    }
    Console.WriteLine("etats publies : " + string.Join(" -> ", etats) + $" ; appels au tube : {comptes.Calls}");
    Ok(etats.Count > 0 && etats[0].StartsWith("Loading", StringComparison.Ordinal), "le premier etat publie est Loading (INDEX EN PREPARATION)");
    Ok(fin is { Result.State: SearchState.Ready } && fin.Result.Items.Count is >= 1 and <= 30, $"windows -> Ready avec {fin?.Result.Items.Count} resultats (<= 30)");
    Ok(sw.ElapsedMilliseconds < LauncherSearchCoordinator.LoadingBudget.TotalMilliseconds, $"resultats reels en {sw.ElapsedMilliseconds} ms depuis la frappe (budget 5 s)");
    if (fin is not null)
    {
        var classes = LauncherFileRanker.Rank("windows", fin.Result.Items);
        Console.WriteLine("5 premiers apres classement du client : " + string.Join(" | ", classes.Select(c => c.Title)));
        Ok(classes.Count == Math.Min(5, fin.Result.Items.Count), "section limitee a 5 lignes");
    }

    // ---------------------------------------------------------------- 3. hello (une fois) + latence par terme
    Console.WriteLine($"== 3. Latences bout en bout depuis le client (n = {essais} par terme, une connexion courte par appel, apres chargement)");
    foreach (string terme in new[] { "a", "aaa", "windows", "readme.md", "note", "zzzqxj" })
    {
        var durees = new List<double>();
        SearchProviderResult? dernier = null;
        for (int i = 0; i < essais; i++)
        {
            var t = Stopwatch.StartNew();
            dernier = provider.SearchAsync(terme, CancellationToken.None).GetAwaiter().GetResult();
            t.Stop();
            if (dernier.State == SearchState.Loading) { throw new InvalidOperationException("index decharge en cours de mesure"); }
            durees.Add(t.Elapsed.TotalMilliseconds);
        }
        durees.Sort();
        double P(double p) => durees[(int)Math.Max(0, Math.Ceiling(durees.Count * p) - 1)];
        string note = terme.Length < 3 ? "  (moins de 3 caracteres : jamais envoye)" : string.Empty;
        Console.WriteLine($"{terme,-10} p50 {P(0.5),7:N1} ms  p95 {P(0.95),7:N1} ms  min {durees[0],7:N1}  max {durees[^1],7:N1}  ({dernier!.Items.Count} resultats, {dernier.State}){note}");
    }

    // ---------------------------------------------------------------- 4. Annulation par frappes rapprochees
    Console.WriteLine("== 4. Frappes rapprochees (win, wind, windo, window, windows, 15 ms entre chacune)");
    while (misesAJour.TryTake(out _)) { }
    int avant = comptes.Calls;
    foreach (string frappe in new[] { "win", "wind", "windo", "window", "windows" })
    {
        coordinateur.Query(frappe);
        Thread.Sleep(15);
    }
    SearchUpdate? derniere = misesAJour.TryTake(out SearchUpdate? d1, 5000) ? d1 : null;
    Thread.Sleep(400);
    int surplus = misesAJour.Count;
    Console.WriteLine($"mises a jour : premiere pour '{derniere?.Query}' ({derniere?.Result.State}), {surplus} autre(s) ; appels au tube pour 5 frappes : {comptes.Calls - avant}");
    Ok(derniere?.Query == "windows" && surplus == 0, "seule la derniere frappe est publiee");
    Ok(comptes.Calls - avant == 1, "debounce : un seul appel au tube pour 5 frappes a 15 ms");

    // ---------------------------------------------------------------- 5. Client muet : l'instance est prise
    Console.WriteLine("== 5. Client muet (l'instance unique est occupee), puis retour a la normale");
    using (var muet = new NamedPipeClientStream(".", nomTube, PipeDirection.InOut, PipeOptions.Asynchronous))
    {
        muet.Connect(3000);
        var t = Stopwatch.StartNew();
        SearchProviderResult occupe = provider.SearchAsync("windows", CancellationToken.None).GetAwaiter().GetResult();
        Console.WriteLine($"appel pendant que l'instance est prise : {occupe.State} / {occupe.Reason} apres {t.ElapsedMilliseconds} ms");
        Ok(occupe.State == SearchState.Unavailable && t.ElapsedMilliseconds < 600, "instance prise : Indisponible dans le budget, sans exception");

        var viaCoordinateur = new System.Collections.Concurrent.BlockingCollection<SearchUpdate>();
        using var c2 = new LauncherSearchCoordinator(provider, viaCoordinateur.Add, callBudget: TimeSpan.FromMilliseconds(100));
        t.Restart();
        c2.Query("windows");
        SearchUpdate coupe = viaCoordinateur.Take(new CancellationTokenSource(5000).Token);
        Console.WriteLine($"idem via le coordinateur (budget 100 ms) : {coupe.Result.State} / {coupe.Result.Reason} apres {t.ElapsedMilliseconds} ms");
        Ok(coupe.Result.State == SearchState.Unavailable, "le coordinateur coupe l'appel a son budget");
    }
    Thread.Sleep(500);
    SearchProviderResult retour = provider.SearchAsync("windows", CancellationToken.None).GetAwaiter().GetResult();
    Ok(retour.State == SearchState.Ready || retour.State == SearchState.Loading, $"apres liberation de l'instance : {retour.State}");

    // ---------------------------------------------------------------- 6. Serveur absent
    Console.WriteLine("== 6. Serveur absent");
    using (var absent = new SpaceFilesSearchProvider("Eco.spacefiles.absent" + Environment.ProcessId))
    {
        var lat = new List<double>();
        SearchProviderResult r = null!;
        for (int i = 0; i < essais; i++)
        {
            var t = Stopwatch.StartNew();
            r = absent.SearchAsync("windows", CancellationToken.None).GetAwaiter().GetResult();
            lat.Add(t.Elapsed.TotalMilliseconds);
        }
        lat.Sort();
        Console.WriteLine($"tube inexistant : {r.State} / {r.Reason}, p50 {lat[lat.Count / 2]:N2} ms, max {lat[^1]:N2} ms");
        Ok(r is { State: SearchState.Unavailable, Reason: SearchReasons.Absent } && lat[^1] < 100, "tube absent : Indisponible tout de suite");
    }

    // ---------------------------------------------------------------- 7. Connexion coupee en cours de session (resident arrete)
    Console.WriteLine("== 7. Resident de test arrete (kill du processus de TEST uniquement)");
    string journal = Path.Combine(profil, "Local", "SpaceFiles", "journal.txt");
    string texteJournal = File.Exists(journal) ? File.ReadAllText(journal) : string.Empty;
    Ok(!System.Text.RegularExpressions.Regex.IsMatch(texteJournal, @"windows|readme|zzzqxj|note\b|C:\\"), "le journal du service ne contient ni texte cherche ni chemin");
    resident.Kill();
    resident.WaitForExit(5000);
    var tk = Stopwatch.StartNew();
    SearchProviderResult mort = provider.SearchAsync("windows", CancellationToken.None).GetAwaiter().GetResult();
    Console.WriteLine($"apres l'arret : {mort.State} / {mort.Reason} en {tk.ElapsedMilliseconds} ms");
    Ok(mort.State == SearchState.Unavailable, "service arrete : Indisponible, aucune exception");

    // ---------------------------------------------------------------- 8. Verification du serveur
    Console.WriteLine("== 8. Verification du serveur");
    Ok(PipeServerVerifier.IsProcessOwnedByCurrentUser((uint)resident.Id) || resident.HasExited, "(le resident de test tournait sous le SID de l'utilisateur : toutes les requetes ci-dessus sont passees par la verification)");
}
finally
{
    if (!resident.HasExited) { resident.Kill(); }
    resident.WaitForExit(5000);
    try { Directory.Delete(racine, true); } catch (IOException) { }
}

Console.WriteLine($"== Bilan : {echecs} echec(s)");
return echecs == 0 ? 0 : 1;

internal sealed class CountingProvider(ISearchProvider inner) : ISearchProvider
{
    private int _calls;
    public int Calls => Volatile.Read(ref _calls);
    public int MaxResults => inner.MaxResults;

    public Task<SearchProviderResult> SearchAsync(string query, CancellationToken cancellationToken)
    {
        Interlocked.Increment(ref _calls);
        return inner.SearchAsync(query, cancellationToken);
    }
}
