// Preuve croisee de la phase 6, lot B (ADR-037 de SpaceNotch, section « Le Hub ecrit shared.json ») : l'ECRIVAIN du Hub
// (SpaceNotch.Platform.Windows.Eco.EcoSharedWriter, pas une ecriture a la main comme le harnais de la phase 5) produit
// `shared.json` d'un FAUX profil, et l'hote Eco REEL de SpaceNotch (en processus, tube d'essai Eco.spacenotch.p6p<pid>, jamais le
// vrai) rapporte l'avis en moins de 1 s, avec les bonnes valeurs. AUCUNE fenetre, ni SpaceNotch ni le resident de l'auteur ni le
// Hub : seul l'ecrivain sans interface est exerce. Aucune entree souris. Code de sortie non nul au moindre echec.
//
// Il verifie aussi les OCTETS exacts d'une fusion (accent, langue, champ inconnu conserves) : ces memes octets sont la fixture
// litterale du test Rust `eco::partage::tests::octets_ecrits_par_le_hub` (src/eco/partage.rs), qui les relit cote SpaceFiles.
// LIMITE DITE : la lecture cote Rust n'est prouvee que par `cargo test`, pas par ce harnais.
//
// Usage (depuis le depot SpaceFiles) :
//   dotnet run -c Release tools\bench\preuve-phase6-hub-apparence.cs
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
Console.WriteLine($"== 0. Etat reel : empreintes prises ; %LOCALAPPDATA%\\Eco {(ecoAbsentAvant ? "absent" : "present")} ; spacefiles.exe deja la : [{string.Join(",", pidsAvant.Order())}]");

string racine = Path.Combine(Path.GetTempPath(), "sf-phase6-hub-apparence");
if (Directory.Exists(racine)) { Directory.Delete(racine, true); }
string eco = Path.Combine(racine, "faux-profil", "Local", "Eco");   // le « %LOCALAPPDATA%\Eco » du faux profil ; absent au depart
string partage = EcoPaths.SharedFile(eco);
Ok(!Path.GetFullPath(eco).StartsWith(Path.GetFullPath(reels[0]).TrimEnd('\\') + "\\", StringComparison.OrdinalIgnoreCase) && !string.Equals(Path.GetFullPath(eco).TrimEnd('\\'), Path.GetFullPath(reels[0]).TrimEnd('\\'), StringComparison.OrdinalIgnoreCase),
    "le dossier Eco d'essai (sous %TEMP%) n'est ni le vrai %LOCALAPPDATA%\\Eco ni dessous");
Ok(!Directory.Exists(eco), "le dossier Eco d'essai n'existe pas encore");

var recus = new ConcurrentQueue<(EcoSharedOpinion Avis, long Ms)>();
var chrono = Stopwatch.StartNew();
var journal = new ConcurrentQueue<string>();

bool Ecrire(EcoSharedPatch patch, out EcoSharedWriteResult resultat)
    => EcoSharedWriter.TryWrite(eco, patch, out resultat, log: journal.Enqueue);

// Un patch vide sur un fichier absent n'ecrit rien et ne cree rien. Le dossier est cree ensuite a la main : l'hote en a besoin pour
// surveiller (le cas « dossier absent » de l'ecrivain est prouve par les tests C#, EcoSharedWriterTests).
Ok(Ecrire(new EcoSharedPatch(null, null), out var r0) && r0.Status == EcoSharedWriteStatus.Unchanged && !File.Exists(partage), "patch vide sur fichier absent : rien d'ecrit");
Directory.CreateDirectory(eco);

await using var hote = new EcoHost(
    new EcoHostOptions(eco, "Eco.spacenotch.p6p" + Environment.ProcessId, @"C:\SpaceNotch\SpaceNotch.exe", "1.0.0")
    {
        OnSharedChanged = a => recus.Enqueue((a, chrono.ElapsedMilliseconds)),
        Log = journal.Enqueue
    },
    new SinkVide());

try
{
    Console.WriteLine("== (a) l'hote demarre sans shared.json : aucun avis");
    Ok(await hote.StartAsync(), "l'hote demarre (tube d'essai ouvert)");
    Thread.Sleep(500);
    Ok(recus.Count == 1 && recus.TryPeek(out var initial) && initial.Avis == EcoSharedOpinion.None, "seul rappel : l'etat initial « aucun avis »");
    recus.Clear();

    // ------------------------------------------------------------ (b) l'ecrivain cree le fichier : light + reduced
    Console.WriteLine("== (b) l'ECRIVAIN du Hub ecrit theme light + mouvement reduced");
    long t0 = chrono.ElapsedMilliseconds;
    Ok(Ecrire(new EcoSharedPatch(EcoTheme.Light, EcoMotion.Reduced), out var r1) && r1.Status == EcoSharedWriteStatus.Written, "ecriture reussie (Written)");
    Ok(Attendre(() => recus.Count >= 1, 1000), "l'hote recoit un avis en moins de 1 s");
    var premier = recus.TryPeek(out var p0) ? p0 : default;
    long delai = premier.Ms - t0;
    Console.WriteLine($"   delai ecriture -> avis : {delai} ms");
    Ok(delai < 1000, $"delai {delai} ms < 1000 ms");
    Ok(premier.Avis == new EcoSharedOpinion(EcoTheme.Light, EcoMotion.Reduced), "avis = theme light + mouvement reduced");
    Ok(EcoAppearance.ResolveLight(null, premier.Avis, systemLight: false) && premier.Avis.ReducesMotion, "la notch en ferait : clair + animations coupees");
    Thread.Sleep(600);
    Ok(recus.Count == 1, $"rapporte UNE seule fois (compte : {recus.Count})");

    // ------------------------------------------------------------ (c) un seul champ change : l'autre est conserve
    Console.WriteLine("== (c) l'ecrivain ne change que le theme : le mouvement reste reduced");
    Ok(Ecrire(new EcoSharedPatch(EcoTheme.Dark, null), out var r2) && r2.Status == EcoSharedWriteStatus.Written, "ecriture reussie");
    Ok(Attendre(() => recus.Count >= 2, 1000), "un second avis arrive en moins de 1 s");
    Ok(recus.ToArray()[^1].Avis == new EcoSharedOpinion(EcoTheme.Dark, EcoMotion.Reduced), "avis = dark + reduced");

    // ------------------------------------------------------------ (d) meme choix : aucune reecriture, donc aucun evenement
    Console.WriteLine("== (d) le meme choix une seconde fois : rien n'est reecrit, l'hote n'est pas reveille");
    Thread.Sleep(500);
    int avantMeme = recus.Count;
    Ok(Ecrire(new EcoSharedPatch(EcoTheme.Dark, EcoMotion.Reduced), out var r3) && r3.Status == EcoSharedWriteStatus.Unchanged, "resultat Unchanged");
    Thread.Sleep(700);
    Ok(recus.Count == avantMeme, $"aucun avis de plus (compte : {recus.Count})");

    // ------------------------------------------------------------ (e) system + full
    Console.WriteLine("== (e) system + full");
    Ok(Ecrire(new EcoSharedPatch(EcoTheme.System, EcoMotion.Full), out var r4) && r4.Status == EcoSharedWriteStatus.Written, "ecriture reussie");
    Ok(Attendre(() => recus.Count >= avantMeme + 1, 1000), "un avis arrive");
    Ok(recus.ToArray()[^1].Avis == new EcoSharedOpinion(EcoTheme.System, EcoMotion.Full), "avis = system + full");

    // ------------------------------------------------------------ (f) fichier abime : pas d'ecrasement sans « Reinitialiser »
    Console.WriteLine("== (f) fichier abime : l'ecrivain refuse sans reset, puis reinitialise sur demande");
    File.WriteAllText(partage, "{ ceci n'est pas du json");
    Attendre(() => recus.ToArray()[^1].Avis == EcoSharedOpinion.None, 1000);
    byte[] abime = File.ReadAllBytes(partage);
    Ok(!Ecrire(new EcoSharedPatch(EcoTheme.Light, null), out var r5) && r5.Status == EcoSharedWriteStatus.NeedsReset, "sans reset : refuse (NeedsReset)");
    Ok(abime.SequenceEqual(File.ReadAllBytes(partage)), "le fichier abime est intact");
    Ok(EcoSharedWriter.TryWrite(eco, new EcoSharedPatch(EcoTheme.Light, null), out var r6, reset: true) && r6.Status == EcoSharedWriteStatus.Written, "avec reset : ecrit");
    Ok(Attendre(() => recus.ToArray()[^1].Avis == new EcoSharedOpinion(EcoTheme.Light, null), 1000), "avis = light, aucun avis de mouvement");

    // ------------------------------------------------------------ (f2) lu par SpaceNotch, rejete par SpaceFiles : jamais ecrit
    Console.WriteLine("== (f2) fichier hors du sous-ensemble commun (cle repetee) : refuse sans reset");
    File.WriteAllText(partage, "{\"theme\":\"light\",\"x\":1,\"theme\":\"light\"}");
    Attendre(() => recus.ToArray()[^1].Avis.Theme == EcoTheme.Light, 1000);
    byte[] repete = File.ReadAllBytes(partage);
    Ok(!Ecrire(new EcoSharedPatch(EcoTheme.Light, null), out var r5b) && r5b.Status == EcoSharedWriteStatus.Incompatible && r5b.Cause == "DuplicateKey", "refuse (Incompatible / DuplicateKey), meme pour un choix que le fichier dit deja");
    Ok(repete.SequenceEqual(File.ReadAllBytes(partage)), "le fichier est intact");

    // ------------------------------------------------------------ (g) octets exacts d'une fusion (fixture du test Rust)
    Console.WriteLine("== (g) octets exacts d'une fusion : accent, langue et champ inconnu conserves");
    File.WriteAllText(partage, "{\"accent\":\"#00ff7f\",\"language\":\"en-US\",\"future\":{\"a\":[1,2.50]},\"theme\":\"dark\"}");
    Ok(Ecrire(new EcoSharedPatch(EcoTheme.Light, EcoMotion.Reduced), out var r7) && r7.Status == EcoSharedWriteStatus.Written, "fusion ecrite");
    string octets = Encoding.UTF8.GetString(File.ReadAllBytes(partage));
    string attendu = "{\n  \"accent\": \"#00ff7f\",\n  \"language\": \"en-US\",\n  \"future\": {\"a\":[1,2.50]},\n  \"theme\": \"light\",\n  \"motion\": \"reduced\"\n}";
    Console.WriteLine("   octets ecrits :");
    foreach (string ligne in octets.Split('\n')) { Console.WriteLine("   | " + ligne); }
    Ok(octets == attendu, "les octets ecrits sont EXACTEMENT la fixture litterale du test Rust octets_ecrits_par_le_hub");
    Ok(!File.ReadAllBytes(partage).AsSpan().StartsWith(new byte[] { 0xEF, 0xBB, 0xBF }), "UTF-8 sans BOM");
    Ok(Directory.EnumerateFiles(eco).Count() == 1, "aucun fichier temporaire laisse");
    Ok(Attendre(() => recus.ToArray()[^1].Avis == new EcoSharedOpinion(EcoTheme.Light, EcoMotion.Reduced), 1000), "l'hote voit light + reduced apres la fusion");

    Ok(!journal.Any(m => m.Contains(eco, StringComparison.OrdinalIgnoreCase) || m.Contains("light", StringComparison.Ordinal) || m.Contains("reduced", StringComparison.Ordinal)), "le journal ne contient ni chemin ni contenu");

    var pidsPendant = PidsSpaceFiles();
    pidsPendant.ExceptWith(pidsAvant);
    Ok(pidsPendant.Count == 0, $"aucun spacefiles.exe ajoute (nouveaux : [{string.Join(",", pidsPendant.Order())}])");
}
finally
{
    try { await hote.StopAsync(); } catch (Exception) { }
}

Console.WriteLine("== (h) etat reel");
string[] apres = reels.Select(Instantane).ToArray();
for (int i = 0; i < reels.Length; i++) { Ok(avant[i] == apres[i], "donnees reelles inchangees : " + reels[i]); }
Ok(ecoAbsentAvant == !Directory.Exists(reels[0]), "%LOCALAPPDATA%\\Eco : meme etat qu'avant (" + (ecoAbsentAvant ? "absent" : "present") + ")");
Ok(PidsSpaceFiles().SetEquals(pidsAvant), "liste des processus spacefiles.exe identique a celle d'avant");
Console.WriteLine("   (la lecture cote Rust n'est PAS prouvee ici : voir cargo test, eco::partage::tests::octets_ecrits_par_le_hub)");

Console.WriteLine(echecs == 0 ? "== TOUT EST OK" : $"== {echecs} ECHEC(S)");
return echecs == 0 ? 0 : 1;

sealed class SinkVide : IEcoEventSink
{
    public void OnProgress(EcoProgressUpdate update) { }
}
