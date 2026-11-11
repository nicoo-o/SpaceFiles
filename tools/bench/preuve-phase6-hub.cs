// Preuve de bout en bout de la phase 6 (lot D) : le critere d'acceptation du masterplan, « liste les deux applications avec les bonnes
// versions, detecte une application arretee, signale le conflit Alt+Espace, change le theme partage et les deux applications suivent
// sans redemarrage », rejoue sur le Hub PUBLIE (dossier autonome, jamais `dotnet run`) et un FAUX profil.
//
// Ce que le harnais monte (tout sous %TEMP%\sf-phase6-hub, faux profil : APPDATA / LOCALAPPDATA / TEMP redirigees dans les processus
// lances, SPACEFILES_SANS_REGISTRE=1) :
//   - un resident SpaceFiles d'essai (copie de target\release\spacefiles.exe, SPACEFILES_PIPE : tube d'essai, manifeste dans le faux Eco) ;
//   - l'hote Eco REEL de SpaceNotch (SpaceNotch.Platform.Windows.Eco.EcoHost) dans CE processus, tube d'essai, version 1.2.3 ;
//   - une reservation RegisterHotKey de Ctrl+Maj+Espace (un choix de SpaceNotch que le Hub sonde en essai : option --raccourci-essai) ;
//   - le Hub publie, lance avec --eco/--plugins/--journaux du faux profil et --raccourci-essai, pilote par UI Automation (InvokePattern,
//     SelectionItemPattern, WindowPattern.Close) : AUCUNE entree souris ni clavier.
// Le Hub ouvre sa fenetre a l'ecran (c'est un Hub, pas SpaceFiles) ; aucune fenetre SpaceFiles n'est ouverte, le resident n'en a pas.
//
// LIMITES DITES : (1) le resident SpaceFiles relit shared.json (tampon d'une seconde) mais rien ne l'affiche : « SpaceFiles suit le
// theme » n'est PAS prouve ici (le test Rust `eco::partage::tests::octets_ecrits_par_le_hub` couvre la lecture de ces octets ; SpaceNotch,
// lui, est prouve par l'avis de l'hote reel). (2) L'hote Eco de SpaceNotch est celui de SpaceNotch, mais en processus de ce harnais : la
// vraie notch n'est pas lancee. (3) Rien n'est clique a la souris ; ni Narrateur ni DPI autres que celui de l'ecran.
//
// Usage (depuis le depot SpaceFiles, apres `cargo build --release` et `dotnet publish` du Hub) :
//   dotnet run -c Release tools\bench\preuve-phase6-hub.cs -- [<chemin de SpaceNotch.Hub.exe>]
#:property TargetFramework=net10.0-windows10.0.26100.0
#:property Nullable=enable
#:property ImplicitUsings=enable
#:property AllowUnsafeBlocks=true
#:property UseWPF=true
#:property PublishTrimmed=false
#:property PublishAot=false
#:property JsonSerializerIsReflectionEnabledByDefault=false
#:project ../../../dynamic iland windows/src/SpaceNotch.Platform.Windows/SpaceNotch.Platform.Windows.csproj

using System.Collections.Concurrent;
using System.Diagnostics;
using System.IO;
using System.Runtime.InteropServices;
using System.Text;
using System.Text.Json;
using System.Windows.Automation;
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

static HashSet<int> Pids(string nom) => Process.GetProcessesByName(nom).Select(p => p.Id).ToHashSet();

bool Attendre(Func<bool> condition, int ms)
{
    var t = Stopwatch.StartNew();
    while (t.ElapsedMilliseconds < ms)
    {
        try { if (condition()) { return true; } } catch (Exception e) when (e is ElementNotAvailableException or InvalidOperationException or System.ComponentModel.Win32Exception) { }
        Thread.Sleep(50);
    }
    try { return condition(); } catch (Exception e) when (e is ElementNotAvailableException or InvalidOperationException or System.ComponentModel.Win32Exception) { return false; }
}

// ---------------------------------------------------------------------------------------------- etat reel avant
string local = Environment.GetEnvironmentVariable("LOCALAPPDATA")!;
string roaming = Environment.GetEnvironmentVariable("APPDATA")!;
string[] reels = [Path.Combine(local, "Eco"), Path.Combine(local, "SpaceFiles"), Path.Combine(roaming, "SpaceFiles"), Path.Combine(roaming, "SpaceNotch"), Path.Combine(local, "SpaceNotch")];
string[] avant = reels.Select(Instantane).ToArray();
bool ecoAbsentAvant = !Directory.Exists(reels[0]);
HashSet<int> sfAvant = Pids("spacefiles");
HashSet<int> hubAvant = Pids("SpaceNotch.Hub");
HashSet<int> notchAvant = Pids("SpaceNotch.App");
Console.WriteLine($"== 0. Etat reel : empreintes prises ; %LOCALAPPDATA%\\Eco {(ecoAbsentAvant ? "absent" : "present")} ; spacefiles.exe [{string.Join(",", sfAvant.Order())}] ; SpaceNotch.Hub.exe [{string.Join(",", hubAvant.Order())}] ; SpaceNotch.App.exe [{string.Join(",", notchAvant.Order())}]");

string hubExe = Path.GetFullPath(args.Length > 0 ? args[0] : Path.Combine(Directory.GetCurrentDirectory(), "..", "dynamic iland windows", "artifacts", "hub", "win-x64", "SpaceNotch.Hub.exe"));
Ok(File.Exists(hubExe) && File.Exists(Path.Combine(Path.GetDirectoryName(hubExe)!, "resources.pri")), "le Hub PUBLIE existe, resources.pri a cote : " + hubExe);

string racine = Path.Combine(Path.GetTempPath(), "sf-phase6-hub");
if (Directory.Exists(racine)) { Directory.Delete(racine, true); }
string profil = Path.Combine(racine, "faux-profil");
string fauxLocal = Path.Combine(profil, "Local");
string fauxRoaming = Path.Combine(profil, "Roaming");
string fauxTemp = Path.Combine(profil, "Temp");
string eco = Path.Combine(fauxLocal, "Eco");
string plugins = Path.Combine(fauxLocal, "SpaceNotch", "plugins");
foreach (string d in new[] { Path.Combine(fauxLocal, "SpaceFiles"), Path.Combine(fauxRoaming, "SpaceFiles"), fauxTemp, plugins, Path.Combine(fauxLocal, "SpaceNotch", "logs") }) { Directory.CreateDirectory(d); }
Ok(!Path.GetFullPath(eco).StartsWith(Path.GetFullPath(reels[0]).TrimEnd('\\') + "\\", StringComparison.OrdinalIgnoreCase) && !string.Equals(Path.GetFullPath(eco).TrimEnd('\\'), Path.GetFullPath(reels[0]).TrimEnd('\\'), StringComparison.OrdinalIgnoreCase),
    "le dossier Eco d'essai (sous %TEMP%) n'est ni le vrai %LOCALAPPDATA%\\Eco ni dessous");

string exeSource = Path.GetFullPath(Path.Combine(Directory.GetCurrentDirectory(), "target", "release", "spacefiles.exe"));
if (!File.Exists(exeSource)) { throw new FileNotFoundException("lancer depuis le depot SpaceFiles apres cargo build --release", exeSource); }
string exeCopie = Path.Combine(racine, "spacefiles.exe");
File.Copy(exeSource, exeCopie, true);
string suffixe = "p6h" + Environment.ProcessId;
string partage = EcoPaths.SharedFile(eco);

// ---------------------------------------------------------------------------------------------- la reservation du raccourci d'essai
// Ctrl+Maj+Espace : un choix de SpaceNotch (cle ctrl-shift-space) que le Hub sonde quand on le lui designe. Reserve sur un fil a lui
// (RegisterHotKey(NULL) est lie au fil), libere sur ordre. Si la machine le tient deja, la preuve du conflit n'est pas possible.
const uint ModCtrlShift = 0x0002 | 0x0004 | 0x4000;
const uint VkSpace = 0x20;
var reserveOk = new ManualResetEventSlim(false);
var libere = new ManualResetEventSlim(false);
bool Reserver() => HotkeyApi.Register(ModCtrlShift, VkSpace);
object verrou = new();
Thread? fil = null;
bool reserve = false;
void Reservation(bool voulue)
{
    lock (verrou)
    {
        if (voulue && fil is null)
        {
            reserveOk.Reset();
            libere.Reset();
            fil = new Thread(() =>
            {
                reserve = Reserver();
                reserveOk.Set();
                if (reserve) { libere.Wait(); HotkeyApi.Unregister(); reserve = false; }
            }) { IsBackground = true, Name = "reservation-raccourci" };
            fil.Start();
            reserveOk.Wait();
        }
        else if (!voulue && fil is not null)
        {
            libere.Set();
            fil.Join(2000);
            fil = null;
        }
    }
}

var recus = new ConcurrentQueue<(EcoSharedOpinion Avis, long Ms)>();
var chrono = Stopwatch.StartNew();
var journalHote = new ConcurrentQueue<string>();

ProcessStartInfo Psi(string exe, string arguments, bool redirigerTemp)
{
    var psi = new ProcessStartInfo(exe, arguments) { UseShellExecute = false, CreateNoWindow = true };
    psi.Environment["LOCALAPPDATA"] = fauxLocal;
    psi.Environment["APPDATA"] = fauxRoaming;
    // Le resident d'essai exige que le faux profil soit SOUS le TEMP qu'il voit (regle de `choisir_dossier_eco`) : TEMP n'est donc redirige
    // que pour le Hub (pour verifier qu'il ne laisse aucun fichier ailleurs que dans le faux profil).
    if (redirigerTemp)
    {
        psi.Environment["TEMP"] = fauxTemp;
        psi.Environment["TMP"] = fauxTemp;
    }

    psi.Environment["SPACEFILES_SANS_REGISTRE"] = "1";
    return psi;
}

var psiResident = Psi(exeCopie, "--resident", redirigerTemp: false);
psiResident.WindowStyle = ProcessWindowStyle.Hidden;
psiResident.Environment["SPACEFILES_PIPE"] = suffixe;
psiResident.Environment["SPACEFILES_LIBERATION_S"] = "120";
using Process resident = Process.Start(psiResident)!;

await using var hote = new EcoHost(
    new EcoHostOptions(eco, "Eco.spacenotch." + suffixe, Environment.ProcessPath!, "1.2.3")
    {
        OnSharedChanged = a => recus.Enqueue((a, chrono.ElapsedMilliseconds)),
        Log = journalHote.Enqueue
    },
    new SinkVide());

Process? hub = null;
AutomationElement? fenetre = null;

AutomationElement? Trouver(string idAutomation)
    => fenetre?.FindFirst(TreeScope.Descendants, new PropertyCondition(AutomationElement.AutomationIdProperty, idAutomation));

List<string> Textes()
{
    // Tous les noms accessibles de la fenetre : c'est ce que lit un lecteur d'ecran.
    var liste = new List<string>();
    foreach (AutomationElement e in fenetre!.FindAll(TreeScope.Descendants, Condition.TrueCondition))
    {
        string n = e.Current.Name;
        if (!string.IsNullOrEmpty(n)) { liste.Add(n); }
    }
    return liste;
}

bool Contient(string morceau) => Textes().Any(t => t.Contains(morceau, StringComparison.OrdinalIgnoreCase));
void Invoquer(string id) => ((InvokePattern)Trouver(id)!.GetCurrentPattern(InvokePattern.Pattern)).Invoke();
void Choisir(string nom)
{
    AutomationElement e = fenetre!.FindFirst(TreeScope.Descendants, new AndCondition(
        new PropertyCondition(AutomationElement.ControlTypeProperty, ControlType.RadioButton),
        new PropertyCondition(AutomationElement.NameProperty, nom)))!;
    ((SelectionItemPattern)e.GetCurrentPattern(SelectionItemPattern.Pattern)).Select();
}
void AfficherTextes(string titre)
{
    Console.WriteLine("   [" + titre + "] " + string.Join(" | ", Textes().Where(t => t.Length > 2)));
}

try
{
    // ------------------------------------------------------------------------------------------ 1. les deux applications en marche
    Console.WriteLine("== 1. Resident SpaceFiles d'essai et hote Eco de SpaceNotch en marche");
    Ok(Attendre(() => EcoClient.PipeExists("Eco.spacefiles." + suffixe) && File.Exists(Path.Combine(eco, "apps", "spacefiles.json")), 8000), $"resident d'essai (pid {resident.Id}) : tube et manifeste presents dans le faux Eco");
    Ok(await hote.StartAsync(), "l'hote Eco de SpaceNotch demarre (tube d'essai)");
    Ok(Attendre(() => recus.Count >= 1, 2000), "l'hote a rapporte son etat initial (« aucun avis »)");
    recus.Clear();
    string versionSf = JsonDocument.Parse(File.ReadAllText(Path.Combine(eco, "apps", "spacefiles.json"))).RootElement.GetProperty("version").GetString()!;
    Console.WriteLine($"   versions des manifestes : SpaceFiles {versionSf}, SpaceNotch 1.2.3");

    Ok(Attendre(() => { Reservation(true); return reserve; }, 1000), "Ctrl+Maj+Espace reserve par ce harnais (sinon la preuve du conflit est impossible sur cette machine)");

    // ------------------------------------------------------------------------------------------ 2. le Hub publie
    Console.WriteLine("== 2. Le Hub publie est lance avec --eco du faux profil");
    var psiHub = Psi(hubExe, $"--eco \"{eco}\" --plugins \"{plugins}\" --journaux \"{fauxLocal}\" --raccourci-essai ctrl-shift-space", redirigerTemp: true);
    psiHub.WorkingDirectory = Path.GetDirectoryName(hubExe)!;
    var ouverture = Stopwatch.StartNew();
    hub = Process.Start(psiHub)!;
    Ok(Attendre(() =>
    {
        foreach (AutomationElement f in AutomationElement.RootElement.FindAll(TreeScope.Children, new PropertyCondition(AutomationElement.ProcessIdProperty, hub.Id)))
        {
            if (f.Current.Name == "Hub" && !f.Current.IsOffscreen) { fenetre = f; return true; }
        }
        return false;
    }, 20000), $"la fenetre « Hub » apparait (UIA), pid {hub.Id}, {ouverture.ElapsedMilliseconds} ms");
    if (fenetre is null) { throw new InvalidOperationException("fenetre du Hub absente : la suite n'a pas de sens"); }

    // ------------------------------------------------------------------------------------------ 3. page Applications
    Console.WriteLine("== 3. Page Applications : les deux applications, leurs versions, SpaceFiles « En marche »");
    Ok(Attendre(() => Contient("2 applications"), 8000), "etat de la page : « 2 applications »");
    Ok(Attendre(() => Textes().Any(t => t.Contains("SpaceFiles", StringComparison.Ordinal) && t.Contains(versionSf, StringComparison.Ordinal)), 5000), $"la ligne SpaceFiles dit sa version {versionSf}");
    Ok(Textes().Any(t => t.Contains("SpaceNotch", StringComparison.Ordinal) && t.Contains("1.2.3", StringComparison.Ordinal)), "la ligne SpaceNotch dit sa version 1.2.3");
    Ok(Textes().Any(t => t.Contains("SpaceFiles", StringComparison.Ordinal) && t.Contains("En marche", StringComparison.OrdinalIgnoreCase)), "SpaceFiles : « En marche »");
    Ok(Textes().Any(t => t.Contains("SpaceNotch", StringComparison.Ordinal) && t.Contains("En marche", StringComparison.OrdinalIgnoreCase)), "SpaceNotch : « En marche »");
    AfficherTextes("page Applications");

    // ------------------------------------------------------------------------------------------ 4. une application arretee
    Console.WriteLine("== 4. Le resident d'essai est arrete : apres « Actualiser », le Hub dit « Arretee »");
    resident.Kill();
    resident.WaitForExit(5000);
    Ok(resident.HasExited, "resident d'essai arrete (son pid seulement)");
    Ok(Attendre(() => !EcoClient.PipeExists("Eco.spacefiles." + suffixe), 3000), "son tube a disparu");
    Invoquer("RefreshButton");
    Ok(Attendre(() => Textes().Any(t => t.Contains("SpaceFiles", StringComparison.Ordinal) && t.Contains("Arrêtée", StringComparison.OrdinalIgnoreCase)), 8000), "SpaceFiles : « Arretee » apres Actualiser");
    Ok(Textes().Any(t => t.Contains("SpaceNotch", StringComparison.Ordinal) && t.Contains("En marche", StringComparison.OrdinalIgnoreCase)), "SpaceNotch reste « En marche »");
    AfficherTextes("page Applications apres l'arret");

    // ------------------------------------------------------------------------------------------ 5. raccourcis
    Console.WriteLine("== 5. Page Raccourcis : la combinaison d'essai (Ctrl+Maj+Espace) tenue par le harnais");
    Invoquer("RailShortcuts");
    Ok(Attendre(() => Contient("Réservé"), 8000), "SpaceNotch en marche + touche prise : « Réservé »");
    Ok(Attendre(() => Contient("aucun conflit"), 3000), "etat de la page : « Aucun conflit de raccourci »");
    AfficherTextes("raccourcis, reserve");

    Reservation(false);
    Ok(!reserve, "le harnais libere la combinaison");
    Invoquer("ShortcutsRefresh");
    Ok(Attendre(() => Contient("1 conflit"), 8000), "SpaceNotch en marche + touche LIBRE : « 1 conflit de raccourci »");
    Ok(Contient("le lanceur n'est pas réservé"), "la ligne explique : « le lanceur n'est pas reserve »");
    AfficherTextes("raccourcis, lanceur non reserve");

    Ok(Attendre(() => { Reservation(true); return reserve; }, 1000), "le harnais reserve de nouveau la combinaison");

    // ------------------------------------------------------------------------------------------ 6. apparence
    Console.WriteLine("== 6. Page Apparence : « Clair » et « Reduit » par UIA ; shared.json change ; l'hote Eco de SpaceNotch recoit l'avis");
    Ok(!File.Exists(partage), "shared.json n'existe pas avant le choix");
    Invoquer("RailAppearance");
    Ok(Attendre(() => fenetre!.FindFirst(TreeScope.Descendants, new PropertyCondition(AutomationElement.NameProperty, "Clair")) is not null, 8000), "le choix « Clair » est visible");
    recus.Clear();
    long t0 = chrono.ElapsedMilliseconds;
    Choisir("Clair");
    Ok(Attendre(() => File.Exists(partage) && File.ReadAllText(partage).Contains("\"light\"", StringComparison.Ordinal), 5000), "shared.json contient le theme « light »");
    Ok(Attendre(() => recus.Any(r => r.Avis.Theme == EcoTheme.Light), 5000), "l'hote Eco de SpaceNotch rapporte le theme clair");
    long delaiClair = recus.Where(r => r.Avis.Theme == EcoTheme.Light).Select(r => r.Ms).DefaultIfEmpty(0).Min() - t0;
    Console.WriteLine($"   clic UIA -> avis recu par l'hote : {delaiClair} ms");
    Choisir("Réduit");
    Ok(Attendre(() => File.Exists(partage) && File.ReadAllText(partage).Contains("\"reduced\"", StringComparison.Ordinal), 5000), "shared.json contient le mouvement « reduced »");
    Ok(Attendre(() => recus.ToArray().LastOrDefault().Avis == new EcoSharedOpinion(EcoTheme.Light, EcoMotion.Reduced), 5000), "l'hote Eco de SpaceNotch rapporte clair + reduit, sans redemarrage");
    var dernierAvis = recus.ToArray().LastOrDefault().Avis;
    Ok(EcoAppearance.ResolveLight(null, dernierAvis, systemLight: false) && dernierAvis.ReducesMotion, "la notch en ferait : clair (meme si Windows est sombre) et animations coupees");
    string contenu = File.Exists(partage) ? File.ReadAllText(partage) : "";
    Console.WriteLine("   shared.json : " + contenu.Replace("\n", " ").Replace("\r", ""));
    Ok(Directory.EnumerateFiles(eco).Count() == 1, "aucun fichier temporaire laisse dans le dossier Eco d'essai");
    Console.WriteLine("   (SpaceFiles : le resident d'essai est deja arrete ici ; sa relecture de ces octets est prouvee par cargo test, pas par ce harnais)");

    // ------------------------------------------------------------------------------------------ 7. SpaceNotch s'arrete aussi : combinaison prise par un autre
    Console.WriteLine("== 7. SpaceNotch arretee + touche tenue par un autre : conflit");
    await hote.StopAsync();
    Invoquer("RailShortcuts");
    Invoquer("ShortcutsRefresh");
    Ok(Attendre(() => Contient("1 conflit") && Contient("est pris par une autre application"), 8000), "SpaceNotch arretee + touche prise : conflit « pris par une autre application »");
    AfficherTextes("raccourcis, pris par un autre");

    // ------------------------------------------------------------------------------------------ 8. fermeture
    Console.WriteLine("== 8. Fermeture du Hub");
    var fermeture = Stopwatch.StartNew();
    ((WindowPattern)fenetre.GetCurrentPattern(WindowPattern.Pattern)).Close();
    bool parti = hub.WaitForExit(10000);
    Ok(parti, $"le processus du Hub a disparu apres la fermeture de la fenetre ({fermeture.ElapsedMilliseconds} ms)");
    Thread.Sleep(500);
    Ok(Pids("SpaceNotch.Hub").SetEquals(hubAvant), "aucun SpaceNotch.Hub.exe residuel");
}
catch (Exception e)
{
    Ok(false, "exception dans le harnais : " + e.GetType().Name + " " + e.Message);
}
finally
{
    Reservation(false);
    try { if (hub is { HasExited: false }) { hub.Kill(); } } catch (Exception) { }
    try { if (!resident.HasExited) { resident.Kill(); } } catch (Exception) { }
    try { await hote.StopAsync(); } catch (Exception) { }
}

// ---------------------------------------------------------------------------------------------- etat reel apres
Console.WriteLine("== 9. Etat reel");
string[] apres = reels.Select(Instantane).ToArray();
for (int i = 0; i < reels.Length; i++) { Ok(avant[i] == apres[i], "donnees reelles inchangees : " + reels[i]); }
Ok(ecoAbsentAvant == !Directory.Exists(reels[0]), "%LOCALAPPDATA%\\Eco : meme etat qu'avant (" + (ecoAbsentAvant ? "absent" : "present") + ")");
Ok(Pids("spacefiles").SetEquals(sfAvant), "seuls les spacefiles.exe d'avant tournent (le resident de l'auteur)");
Ok(Pids("SpaceNotch.App").SetEquals(notchAvant), "aucun SpaceNotch.App.exe lance ni arrete");
Ok(Pids("SpaceNotch.Hub").SetEquals(hubAvant), "aucun SpaceNotch.Hub.exe residuel");
Console.WriteLine("   (non prouve ici : SpaceFiles affichant le theme, un clic reel, Narrateur, la vraie notch)");

Console.WriteLine(echecs == 0 ? "== TOUT EST OK" : $"== {echecs} ECHEC(S)");
return echecs == 0 ? 0 : 1;

static class HotkeyApi
{
    private const int Id = 0x7A51;

    public static bool Register(uint modifiers, uint vk) => RegisterHotKey(IntPtr.Zero, Id, modifiers, vk);

    public static void Unregister() => UnregisterHotKey(IntPtr.Zero, Id);

    [DllImport("user32.dll", SetLastError = true)]
    private static extern bool RegisterHotKey(IntPtr hWnd, int id, uint fsModifiers, uint vk);

    [DllImport("user32.dll", SetLastError = true)]
    private static extern bool UnregisterHotKey(IntPtr hWnd, int id);
}

sealed class SinkVide : IEcoEventSink
{
    public void OnProgress(EcoProgressUpdate update) { }
}
