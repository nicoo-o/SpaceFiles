// Preuve croisee de la phase 5, lot D (ADR-036 de SpaceNotch) : l'hote Eco REEL de SpaceNotch (C#, SpaceNotch.Platform.Windows) suit
// `shared.json` d'un FAUX profil et rapporte l'avis de theme / mouvement. AUCUNE fenetre visible, ni SpaceNotch ni le resident de
// l'auteur : l'hote tourne dans ce processus, avec un tube d'essai (Eco.spacenotch.p5p<pid>, jamais le vrai) et un dossier Eco sous
// %TEMP%. Personne n'ecrit encore ce fichier (le Hub est la phase 6) : c'est ce harnais qui l'ecrit, en ecriture atomique
// (temporaire puis renommage). Aucune entree souris. Code de sortie non nul au moindre echec.
//
// LIMITE DITE : la lecture cote Rust (spacefiles.exe : `eco::partage::mouvement_partage`, tampon d'une seconde, `animations_actives`)
// n'a pas de petit binaire de test, elle est donc prouvee par les tests unitaires de `cargo test` (src/eco/partage.rs, src/reglages.rs),
// PAS par ce harnais. Ce harnais ne prouve que le cote SpaceNotch (hote, surveillance, avis, resolution du theme).
//
// Usage (depuis le depot SpaceFiles) :
//   dotnet run -c Release tools\bench\preuve-phase5-partage.cs
#:property TargetFramework=net10.0-windows10.0.26100.0
#:property Nullable=enable
#:property ImplicitUsings=enable
#:property AllowUnsafeBlocks=true
#:property JsonSerializerIsReflectionEnabledByDefault=false
#:project ../../../dynamic iland windows/src/SpaceNotch.Platform.Windows/SpaceNotch.Platform.Windows.csproj

using System.Collections.Concurrent;
using System.Diagnostics;
using System.Text;
using SpaceNotch.Core.Eco;
using SpaceNotch.Platform.Windows.Eco;

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

static HashSet<int> PidsSpaceFiles() => Process.GetProcessesByName("spacefiles").Select(p => p.Id).ToHashSet();

bool Attendre(Func<bool> condition, int ms)
{
    var t = Stopwatch.StartNew();
    while (t.ElapsedMilliseconds < ms) { if (condition()) { return true; } Thread.Sleep(10); }
    return condition();
}

string local = Environment.GetEnvironmentVariable("LOCALAPPDATA")!;
string roaming = Environment.GetEnvironmentVariable("APPDATA")!;
string[] reels = [Path.Combine(local, "Eco"), Path.Combine(local, "SpaceFiles"), Path.Combine(roaming, "SpaceFiles"), Path.Combine(roaming, "SpaceNotch")];
string[] avant = reels.Select(Instantane).ToArray();
bool ecoAbsentAvant = !Directory.Exists(reels[0]);
HashSet<int> pidsAvant = PidsSpaceFiles();
Console.WriteLine($"== 0. Etat reel : empreintes prises ; %LOCALAPPDATA%\\Eco {(ecoAbsentAvant ? "absent" : "present")} ; processus spacefiles.exe deja la : [{string.Join(",", pidsAvant.Order())}]");

string racine = Path.Combine(Path.GetTempPath(), "sf-phase5-partage");
if (Directory.Exists(racine)) { Directory.Delete(racine, true); }
string eco = Path.Combine(racine, "faux-profil", "Local", "Eco");   // le « %LOCALAPPDATA%\Eco » du faux profil
Directory.CreateDirectory(eco);
string partage = EcoPaths.SharedFile(eco);
Ok(!Path.GetFullPath(eco).StartsWith(Path.GetFullPath(reels[0]).TrimEnd('\\') + "\\", StringComparison.OrdinalIgnoreCase) && !string.Equals(Path.GetFullPath(eco).TrimEnd('\\'), Path.GetFullPath(reels[0]).TrimEnd('\\'), StringComparison.OrdinalIgnoreCase),
    "le dossier Eco d'essai (sous %TEMP%) n'est ni le vrai %LOCALAPPDATA%\\Eco ni dessous");

var recus = new ConcurrentQueue<(EcoSharedOpinion Avis, long Ms)>();
var chrono = Stopwatch.StartNew();
var journal = new ConcurrentQueue<string>();

void EcrireAtomique(string contenu)
{
    string tmp = partage + "." + Guid.NewGuid().ToString("N") + ".tmp";
    File.WriteAllBytes(tmp, Encoding.UTF8.GetBytes(contenu));
    File.Move(tmp, partage, overwrite: true);
}

await using var hote = new EcoHost(
    new EcoHostOptions(eco, "Eco.spacenotch.p5p" + Environment.ProcessId, @"C:\SpaceNotch\SpaceNotch.exe", "1.0.0")
    {
        OnSharedChanged = a => recus.Enqueue((a, chrono.ElapsedMilliseconds)),
        Log = journal.Enqueue
    },
    new SinkVide());

try
{
    // ------------------------------------------------------------ (a) sans shared.json : aucun avis
    Console.WriteLine("== (a) l'hote demarre sans shared.json : aucun avis");
    Ok(await hote.StartAsync(), "l'hote demarre (tube d'essai ouvert)");
    Thread.Sleep(500);
    // Le premier etat est rappele meme s'il est « aucun avis » (un nouvel observateur annonce son etat de depart) ; rien d'autre
    Ok(recus.Count == 1 && recus.TryPeek(out var initial) && initial.Avis == EcoSharedOpinion.None, "aucun avis rapporte (fichier absent) : le seul rappel est l'etat initial « aucun avis »");
    recus.Clear();

    // ------------------------------------------------------------ (b) ecriture atomique light + reduced
    Console.WriteLine("== (b) ecriture atomique de {\"theme\":\"light\",\"motion\":\"reduced\"}");
    long t0 = chrono.ElapsedMilliseconds;
    EcrireAtomique("{\"theme\":\"light\",\"motion\":\"reduced\"}");
    Ok(Attendre(() => recus.Count >= 1, 1000), "un avis arrive en moins de 1 s");
    var premier = recus.TryPeek(out var p0) ? p0 : default;
    long delai = premier.Ms - t0;
    Console.WriteLine($"   delai ecriture -> avis : {delai} ms");
    Ok(delai < 1000, $"delai {delai} ms < 1000 ms");
    Ok(premier.Avis == new EcoSharedOpinion(EcoTheme.Light, EcoMotion.Reduced), "avis = theme light + mouvement reduced");
    Thread.Sleep(600);
    Ok(recus.Count == 1, $"rapporte UNE seule fois (compte : {recus.Count})");
    // Ce que la notch en ferait : « Automatique » suit le partage meme si Windows est en sombre ; la reduction coupe les animations
    Ok(EcoAppearance.ResolveLight(null, premier.Avis, systemLight: false), "apparence « Automatique » + Windows sombre + partage light -> clair");
    Ok(!EcoAppearance.ResolveLight(false, premier.Avis, systemLight: false), "apparence « Sombre » explicite : reste sombre (surcharge locale)");
    Ok(premier.Avis.ReducesMotion, "mouvement reduit");

    // ------------------------------------------------------------ (c) reecrit {"motion":"full"}
    Console.WriteLine("== (c) reecriture de {\"motion\":\"full\"} : plus d'avis de theme, mouvement full");
    EcrireAtomique("{\"motion\":\"full\"}");
    Ok(Attendre(() => recus.Count >= 2, 1000), "un second avis arrive");
    var deuxieme = recus.ToArray()[^1];
    Ok(deuxieme.Avis == new EcoSharedOpinion(null, EcoMotion.Full), "avis = aucun theme + mouvement full");
    Ok(!deuxieme.Avis.ReducesMotion, "full n'allume rien de force : ne reduit pas");
    Ok(!EcoAppearance.ResolveLight(null, deuxieme.Avis, systemLight: false) && EcoAppearance.ResolveLight(null, deuxieme.Avis, systemLight: true), "plus d'avis de theme : « Automatique » suit Windows");
    Thread.Sleep(500);
    Ok(recus.Count == 2, $"toujours deux avis au total (compte : {recus.Count})");

    // ------------------------------------------------------------ (d) fichier abime
    Console.WriteLine("== (d) fichier abime : aucun avis");
    EcrireAtomique("{ ceci n'est pas du json");
    Ok(Attendre(() => recus.Count >= 3, 1000), "un troisieme avis arrive");
    Ok(recus.ToArray()[^1].Avis == EcoSharedOpinion.None, "avis = aucun (fichier abime)");

    // ------------------------------------------------------------ (e) suppression
    Console.WriteLine("== (e) un avis valide puis suppression du fichier : aucun avis");
    EcrireAtomique("{\"theme\":\"dark\",\"motion\":\"reduced\"}");
    Ok(Attendre(() => recus.Count >= 4, 1000), "un avis valide arrive");
    Ok(recus.ToArray()[^1].Avis == new EcoSharedOpinion(EcoTheme.Dark, EcoMotion.Reduced), "avis = dark + reduced");
    File.Delete(partage);
    Ok(Attendre(() => recus.Count >= 5, 1000), "un avis arrive apres la suppression");
    Ok(recus.ToArray()[^1].Avis == EcoSharedOpinion.None, "avis = aucun (fichier supprime)");
    Thread.Sleep(500);
    Ok(recus.Count == 5, $"cinq avis au total, chacun une fois (compte : {recus.Count})");

    // ------------------------------------------------------------ (f) arret : plus rien
    Console.WriteLine("== (f) apres l'arret de l'hote, ecrire ne declenche rien");
    await hote.StopAsync();
    EcrireAtomique("{\"theme\":\"light\"}");
    Thread.Sleep(700);
    Ok(recus.Count == 5, "aucun avis apres l'arret (observateur libere)");
    Ok(!journal.Any(m => m.Contains(eco, StringComparison.OrdinalIgnoreCase) || m.Contains("light", StringComparison.Ordinal) || m.Contains("reduced", StringComparison.Ordinal)), "le journal de l'hote ne contient ni chemin ni contenu");

    // ------------------------------------------------------------ processus
    var pidsPendant = PidsSpaceFiles();
    pidsPendant.ExceptWith(pidsAvant);
    Ok(pidsPendant.Count == 0, $"aucun spacefiles.exe ajoute (nouveaux : [{string.Join(",", pidsPendant.Order())}])");
}
finally
{
    try { await hote.StopAsync(); } catch (Exception) { }
}

// ------------------------------------------------------------------ etat reel
Console.WriteLine("== (g) etat reel");
string[] apres = reels.Select(Instantane).ToArray();
for (int i = 0; i < reels.Length; i++) { Ok(avant[i] == apres[i], "donnees reelles inchangees : " + reels[i]); }
Ok(ecoAbsentAvant == !Directory.Exists(reels[0]), "%LOCALAPPDATA%\\Eco : meme etat qu'avant (" + (ecoAbsentAvant ? "absent" : "present") + ")");
Ok(PidsSpaceFiles().SetEquals(pidsAvant), "liste des processus spacefiles.exe identique a celle d'avant");
Console.WriteLine("   (la lecture cote Rust n'est PAS prouvee ici : voir cargo test, src/eco/partage.rs)");

Console.WriteLine(echecs == 0 ? "== TOUT EST OK" : $"== {echecs} ECHEC(S)");
return echecs == 0 ? 0 : 1;

sealed class SinkVide : IEcoEventSink
{
    public void OnProgress(EcoProgressUpdate update) { }
}
