// Preuve croisee de la phase 5, lots B et C : le VRAI SpaceFilesOpener de SpaceNotch (C#, projet SpaceNotch.Platform.Windows) appelle
// un resident SpaceFiles de TEST avec `reuse:true`. AUCUNE fenetre visible : le resident est une copie de l'exe sous %TEMP%
// (faux profil, APPDATA / LOCALAPPDATA redirigees dans ce seul processus, SPACEFILES_SANS_REGISTRE=1, SPACEFILES_PIPE distinct,
// faux lanceur de fenetre) ; la « fenetre SpaceFiles » est une fenetre MESSAGE-ONLY creee ici, de la classe donnee par
// SPACEFILES_CLASSE_CIBLE (lue par le resident seulement en mode essai). Ne lance ni SpaceNotch ni le resident de l'auteur, ne
// touche pas a ses donnees (empreintes avant / apres), aucune entree souris. Code de sortie non nul au moindre echec.
//
// Usage (depuis le depot SpaceFiles, apres `cargo build --release`) :
//   dotnet run -c Release tools\bench\preuve-phase5-notch.cs
#:property TargetFramework=net10.0-windows10.0.26100.0
#:property Nullable=enable
#:property ImplicitUsings=enable
#:property AllowUnsafeBlocks=true
#:property JsonSerializerIsReflectionEnabledByDefault=false
#:project ../../../dynamic iland windows/src/SpaceNotch.Platform.Windows/SpaceNotch.Platform.Windows.csproj
#:project ../../../dynamic iland windows/src/SpaceNotch.Features/SpaceNotch.Features.csproj

using System.Collections.Concurrent;
using System.Diagnostics;
using System.Runtime.InteropServices;
using System.Text;
using SpaceNotch.Core.Activities;
using SpaceNotch.Core.Eco;
using SpaceNotch.Core.Events;
using SpaceNotch.Features.Launcher;
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

static HashSet<int> PidsSpaceFiles() => Process.GetProcessesByName("spacefiles").Select(p => p.Id).ToHashSet();

string local = Environment.GetEnvironmentVariable("LOCALAPPDATA")!;
string roaming = Environment.GetEnvironmentVariable("APPDATA")!;
string[] reels = [Path.Combine(local, "Eco"), Path.Combine(local, "SpaceFiles"), Path.Combine(roaming, "SpaceFiles"), Path.Combine(roaming, "SpaceNotch")];
string[] avant = reels.Select(Instantane).ToArray();
bool ecoAbsentAvant = !Directory.Exists(reels[0]);
HashSet<int> pidsAvant = PidsSpaceFiles();
Console.WriteLine($"== 0. Etat reel : empreintes prises ; %LOCALAPPDATA%\\Eco {(ecoAbsentAvant ? "absent" : "present")} ; processus spacefiles.exe deja la : [{string.Join(",", pidsAvant.Order())}]");

string racine = Path.Combine(Path.GetTempPath(), "sf-phase5-notch");
if (Directory.Exists(racine)) { Directory.Delete(racine, true); }
string profil = Path.Combine(racine, "faux-profil");
Directory.CreateDirectory(Path.Combine(profil, "Local", "SpaceFiles"));
Directory.CreateDirectory(Path.Combine(profil, "Roaming", "SpaceFiles"));
string exeSource = Path.GetFullPath(Path.Combine(Directory.GetCurrentDirectory(), "target", "release", "spacefiles.exe"));
if (!File.Exists(exeSource)) { throw new FileNotFoundException("lancer depuis le depot SpaceFiles apres cargo build --release", exeSource); }
string exeCopie = Path.Combine(racine, "spacefiles.exe");
File.Copy(exeSource, exeCopie, true);
string suffixe = "p5n" + Environment.ProcessId;
string tube = "Eco.spacefiles." + suffixe;
string classe = "SfEssaiNotch" + Environment.ProcessId;
string fauxLanceur = Path.Combine(racine, "faux-lanceur.txt");
string dossier = Path.Combine(racine, "dossier d'essai");
Directory.CreateDirectory(dossier);
string fichier = Path.Combine(dossier, "note.txt");
File.WriteAllText(fichier, "x");

var psi = new ProcessStartInfo(exeCopie, "--resident") { UseShellExecute = false, CreateNoWindow = true, WindowStyle = ProcessWindowStyle.Hidden };
psi.Environment["LOCALAPPDATA"] = Path.Combine(profil, "Local");
psi.Environment["APPDATA"] = Path.Combine(profil, "Roaming");
psi.Environment["SPACEFILES_SANS_REGISTRE"] = "1";
psi.Environment["SPACEFILES_PIPE"] = suffixe;
psi.Environment["SPACEFILES_FAUX_LANCEUR"] = fauxLanceur;
psi.Environment["SPACEFILES_CLASSE_CIBLE"] = classe;
psi.Environment["SPACEFILES_LIBERATION_S"] = "120";
using Process resident = Process.Start(psi)!;

string[] LireLignes()
{
    try
    {
        using var fs = new FileStream(fauxLanceur, FileMode.Open, FileAccess.Read, FileShare.ReadWrite | FileShare.Delete);
        using var lecteur = new StreamReader(fs);
        return lecteur.ReadToEnd().Split((char)10, StringSplitOptions.RemoveEmptyEntries | StringSplitOptions.TrimEntries);
    }
    catch (IOException) { return []; }
    catch (UnauthorizedAccessException) { return []; }
}

bool Attendre(Func<bool> condition, int ms)
{
    var t = Stopwatch.StartNew();
    while (t.ElapsedMilliseconds < ms) { if (condition()) { return true; } Thread.Sleep(15); }
    return condition();
}

try
{
    for (int k = 0; k < 100 && !EcoClient.PipeExists(tube); k++) { Thread.Sleep(50); }
    Ok(EcoClient.PipeExists(tube), $"tube du resident de test (pid {resident.Id}) present");

    // Le vrai SpaceFilesOpener et le vrai EcoClient ; seuls le verifieur (exe = la copie sous %TEMP%) et le droit de premier
    // plan (un espion : aucun appel reel a AllowSetForegroundWindow) sont ceux de l'essai.
    var grant = new EspionPremierPlan();
    var ouvreur = new SpaceFilesOpener(new EcoClient(tube, new PipeServerVerifier(() => exeCopie, null)), grant, m => Console.WriteLine("   [ouvreur] " + m));

    // ---------------------------------------------------------------- (a) sans fenetre : repli sur le lancement
    Console.WriteLine("== (a) sans fenetre SpaceFiles : le faux lanceur recoit --ouvrir");
    SpaceFilesOpenResult r = await ouvreur.OpenInSpaceFilesAsync(dossier, false, CancellationToken.None);
    Ok(r == SpaceFilesOpenResult.Opened, $"SpaceFilesOpener : {r} (attendu Opened)");
    Ok(Attendre(() => LireLignes().Length >= 1, 4000), "le faux lanceur recoit une ligne");
    string[] l = LireLignes();
    Ok(l.Length == 1 && l[0].StartsWith("--ouvrir ", StringComparison.Ordinal) && l[0].Contains("dossier d'essai", StringComparison.Ordinal), "ligne recue : --ouvrir \"<dossier>\" (" + (l.Length > 0 ? l[0] : "rien") + ")");
    Ok(grant.Pids.SequenceEqual([(uint)resident.Id]), "AllowSetForegroundWindow vise le pid verifie du resident, et lui seul");

    // ---------------------------------------------------------------- fenetre d'essai message-only
    using var fenetre = new FenetreEssai(classe);
    Ok(fenetre.Prete, "fenetre message-only de classe " + classe + " creee dans ce processus");

    // ---------------------------------------------------------------- (b) fenetre qui repond 1, reveal vrai
    Console.WriteLine("== (b) fenetre qui repond 1, SpaceFilesOpener(reveal:true) : la fenetre recoit OUVRIR_ICI, le faux lanceur ne recoit RIEN");
    fenetre.Mode = 0;
    int lignesAvant = LireLignes().Length;
    r = await ouvreur.OpenInSpaceFilesAsync(fichier, true, CancellationToken.None);
    Ok(r == SpaceFilesOpenResult.Opened, $"SpaceFilesOpener : {r} (attendu Opened)");
    Ok(Attendre(() => fenetre.Recus.Count >= 1, 4000), "la fenetre recoit un WM_COPYDATA");
    var recu = fenetre.Recus.TryPeek(out var r0) ? r0 : default;
    Console.WriteLine($"   dwData = 0x{recu.DwData:X}, charge = {recu.Texte.Replace("\n", "\\n")}");
    Ok(recu.DwData == 0x4E450002, "dwData = OUVRIR_ICI (0x4E450002)");
    string[] pa = recu.Texte.Split((char)10, 3);
    Ok(pa.Length == 3 && pa[0] == "1" && ulong.TryParse(pa[1], out _) && pa[2] == fichier, "charge = \"1\\n<tick>\\n<chemin>\" : drapeau reveler = 1 et bon chemin");
    Ok(recu.AgeMs >= 0 && recu.AgeMs <= 1500 && recu.Acceptee, $"age {recu.AgeMs} ms : acceptee par la regle d'age");
    Thread.Sleep(700);
    Ok(LireLignes().Length == lignesAvant, "le faux lanceur n'a rien recu (aucune fenetre lancee)");

    // ---------------------------------------------------------------- (c) reveal faux : drapeau 0
    Console.WriteLine("== (c) fenetre qui repond 1, SpaceFilesOpener(reveal:false) : drapeau reveler = 0");
    int recusAvant = fenetre.Recus.Count;
    r = await ouvreur.OpenInSpaceFilesAsync(dossier, false, CancellationToken.None);
    Ok(r == SpaceFilesOpenResult.Opened, $"SpaceFilesOpener : {r} (attendu Opened)");
    Ok(Attendre(() => fenetre.Recus.Count >= recusAvant + 1, 4000), "la fenetre recoit un second WM_COPYDATA");
    var deux = fenetre.Recus.ToArray()[^1];
    string[] pc = deux.Texte.Split((char)10, 3);
    Ok(deux.DwData == 0x4E450002 && pc.Length == 3 && pc[0] == "0" && pc[2] == dossier, "charge = \"0\\n<tick>\\n<dossier>\" : drapeau reveler = 0 et bon chemin");
    Thread.Sleep(500);
    Ok(LireLignes().Length == lignesAvant, "le faux lanceur n'a toujours rien recu");

    // ---------------------------------------------------------------- (d) fenetre qui repond 0 : repli
    Console.WriteLine("== (d) fenetre qui repond 0 : repli sur le lancement, --ouvrir");
    fenetre.Mode = 1;
    recusAvant = fenetre.Recus.Count;
    r = await ouvreur.OpenInSpaceFilesAsync(dossier, false, CancellationToken.None);
    Ok(r == SpaceFilesOpenResult.Opened, $"SpaceFilesOpener : {r} (attendu Opened)");
    Ok(Attendre(() => LireLignes().Length >= lignesAvant + 1, 4000), "le faux lanceur recoit la ligne apres le refus");
    Ok(fenetre.Recus.Count == recusAvant + 1, "la fenetre a bien ete essayee d'abord");
    l = LireLignes();
    Ok(l[^1].StartsWith("--ouvrir ", StringComparison.Ordinal), "ligne recue : " + l[^1]);

    // ---------------------------------------------------------------- (e) chemin invalide : rien ne part
    Console.WriteLine("== (e) chemin relatif : InvalidPath, rien ne part");
    lignesAvant = LireLignes().Length;
    recusAvant = fenetre.Recus.Count;
    r = await ouvreur.OpenInSpaceFilesAsync("relatif\\chemin", false, CancellationToken.None);
    Ok(r == SpaceFilesOpenResult.InvalidPath, $"SpaceFilesOpener : {r} (attendu InvalidPath)");
    Thread.Sleep(300);
    Ok(LireLignes().Length == lignesAvant && fenetre.Recus.Count == recusAvant, "ni lancement ni charge");
    Ok(!resident.HasExited, "le resident est toujours vivant");

    // ---------------------------------------------------------------- (g) lot C : la methode de Feature « Ouvrir SpaceFiles »
    Console.WriteLine("== (g) lot C : LauncherFeature.OpenSpaceFilesHomeAsync avec un dossier personnel FACTICE, vers le resident d'essai");
    fenetre.Mode = 0;
    string fauxHome = Path.Combine(racine, "faux-home");
    Directory.CreateDirectory(fauxHome);
    var replis = new List<string>();
    lignesAvant = LireLignes().Length;
    recusAvant = fenetre.Recus.Count;
    await using (var feature = new LauncherFeature(
        new ActivityManager(), new EventBus(), spaceFiles: ouvreur,
        homeFolder: () => fauxHome,
        locationFallback: p => { replis.Add(p); return true; }))   // jamais l'Explorateur reel
    {
        bool traite = await feature.OpenSpaceFilesHomeAsync();
        Ok(traite, "OpenSpaceFilesHomeAsync rend vrai");
    }
    Ok(Attendre(() => fenetre.Recus.Count >= recusAvant + 1, 4000), "la fenetre recoit un WM_COPYDATA par la methode de Feature");
    var home = fenetre.Recus.ToArray()[^1];
    string[] ph = home.Texte.Split((char)10, 3);
    Ok(home.DwData == 0x4E450002 && ph.Length == 3 && ph[0] == "0" && ph[2] == fauxHome, "charge = \"0\\n<tick>\\n<faux dossier personnel>\" : sans reveler, bon chemin");
    Ok(home.Acceptee, $"age {home.AgeMs} ms : acceptee par la regle d'age");
    Ok(replis.Count == 0, "aucun repli sur l'Explorateur (ouverture accordee)");
    Thread.Sleep(500);
    Ok(LireLignes().Length == lignesAvant, "le faux lanceur n'a rien recu : SpaceNotch ne lance pas spacefiles.exe");

    // ---------------------------------------------------------------- processus
    var pidsPendant = PidsSpaceFiles();
    pidsPendant.ExceptWith(pidsAvant);
    Ok(pidsPendant.SetEquals([resident.Id]), $"aucun processus spacefiles.exe ajoute a part le resident de test (nouveaux : [{string.Join(",", pidsPendant.Order())}])");
}
finally
{
    if (!resident.HasExited) { resident.Kill(); }
    resident.WaitForExit(5000);
}

// ------------------------------------------------------------------ (f) etat reel
Console.WriteLine("== (f) etat reel");
string[] apres = reels.Select(Instantane).ToArray();
for (int i = 0; i < reels.Length; i++) { Ok(avant[i] == apres[i], "donnees reelles inchangees : " + reels[i]); }
Ok(ecoAbsentAvant == !Directory.Exists(reels[0]), "%LOCALAPPDATA%\\Eco : meme etat qu'avant (" + (ecoAbsentAvant ? "absent" : "present") + ")");
Ok(PidsSpaceFiles().SetEquals(pidsAvant), "liste des processus spacefiles.exe identique a celle d'avant");

Console.WriteLine(echecs == 0 ? "== TOUT EST OK" : $"== {echecs} ECHEC(S)");
return echecs == 0 ? 0 : 1;

// ====================================================================== espion et fenetre d'essai
readonly record struct Charge(long DwData, string Texte, long AgeMs, bool Acceptee);

sealed class EspionPremierPlan : IForegroundGrant
{
    public List<uint> Pids { get; } = [];
    public bool Allow(uint processId) { Pids.Add(processId); return true; }
}

sealed class FenetreEssai : IDisposable
{
    delegate IntPtr ProcFenetre(IntPtr h, uint m, IntPtr w, IntPtr l);

    [StructLayout(LayoutKind.Sequential, CharSet = CharSet.Unicode)]
    struct WNDCLASSEX
    {
        public uint cbSize; public uint style; public IntPtr lpfnWndProc; public int cbClsExtra; public int cbWndExtra;
        public IntPtr hInstance; public IntPtr hIcon; public IntPtr hCursor; public IntPtr hbrBackground;
        public string? lpszMenuName; public string lpszClassName; public IntPtr hIconSm;
    }

    [StructLayout(LayoutKind.Sequential)]
    struct COPYDATA { public IntPtr dwData; public uint cbData; public IntPtr lpData; }

    [StructLayout(LayoutKind.Sequential)]
    struct MSG { public IntPtr hwnd; public uint message; public IntPtr wParam; public IntPtr lParam; public uint time; public int x; public int y; }

    [DllImport("user32.dll", CharSet = CharSet.Unicode, SetLastError = true)] static extern ushort RegisterClassExW(ref WNDCLASSEX c);
    [DllImport("user32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
    static extern IntPtr CreateWindowExW(uint ex, string classe, string titre, uint style, int x, int y, int w, int h, IntPtr parent, IntPtr menu, IntPtr inst, IntPtr param);
    [DllImport("user32.dll")] static extern IntPtr DefWindowProcW(IntPtr h, uint m, IntPtr w, IntPtr l);
    [DllImport("user32.dll")] static extern int GetMessageW(out MSG m, IntPtr h, uint min, uint max);
    [DllImport("user32.dll")] static extern bool TranslateMessage(ref MSG m);
    [DllImport("user32.dll")] static extern IntPtr DispatchMessageW(ref MSG m);
    [DllImport("user32.dll")] static extern bool PostMessageW(IntPtr h, uint m, IntPtr w, IntPtr l);
    [DllImport("kernel32.dll", CharSet = CharSet.Unicode)] static extern IntPtr GetModuleHandleW(string? nom);
    [DllImport("kernel32.dll")] static extern ulong GetTickCount64();

    static readonly IntPtr HWND_MESSAGE = new(-3);
    const uint WM_COPYDATA = 0x4A;
    const uint WM_CLOSE = 0x10;
    const uint WM_BLOQUER = 0x0401;
    const long AGE_MAX_MS = 1500;   // meme regle que eco::reutiliser::AGE_MAX_MS

    readonly ProcFenetre _proc;       // garde le delegue vivant tant que la fenetre existe
    readonly Thread _fil;
    readonly ManualResetEventSlim _prete = new(false);
    IntPtr _hwnd;
    volatile int _occupe;

    /// <summary>0 = applique la regle d'age puis repond 1 si acceptee, 1 = repond toujours 0</summary>
    public volatile int Mode;
    int _acceptees;
    public int Acceptees => _acceptees;
    /// <summary>Bloque le fil de la fenetre 4 s (message prive), sans qu'elle lise sa file : un message envoye entre-temps arrive tard</summary>
    public void Bloquer(int ms) => PostMessageW(_hwnd, WM_BLOQUER, new IntPtr(ms), IntPtr.Zero);
    public ConcurrentQueue<Charge> Recus { get; } = new();
    public bool Prete => _hwnd != IntPtr.Zero;
    public bool Libre => _occupe == 0;

    public FenetreEssai(string classe)
    {
        _proc = Traiter;
        _fil = new Thread(() =>
        {
            IntPtr inst = GetModuleHandleW(null);
            var c = new WNDCLASSEX { cbSize = (uint)Marshal.SizeOf<WNDCLASSEX>(), lpfnWndProc = Marshal.GetFunctionPointerForDelegate(_proc), hInstance = inst, lpszClassName = classe };
            RegisterClassExW(ref c);
            _hwnd = CreateWindowExW(0, classe, "essai", 0, 0, 0, 0, 0, HWND_MESSAGE, IntPtr.Zero, inst, IntPtr.Zero);
            _prete.Set();
            if (_hwnd == IntPtr.Zero) { return; }
            while (GetMessageW(out MSG m, IntPtr.Zero, 0, 0) > 0) { TranslateMessage(ref m); DispatchMessageW(ref m); }
        }) { IsBackground = true, Name = "fenetre-essai" };
        _fil.Start();
        _prete.Wait(5000);
    }

    IntPtr Traiter(IntPtr h, uint m, IntPtr w, IntPtr l)
    {
        if (m == WM_COPYDATA)
        {
            var cd = Marshal.PtrToStructure<COPYDATA>(l);
            var octets = new byte[cd.cbData];
            if (cd.cbData > 0) { Marshal.Copy(cd.lpData, octets, 0, octets.Length); }
            string texte = Encoding.Unicode.GetString(octets);
            string[] p = texte.Split('\n', 3);
            long age = p.Length == 3 && ulong.TryParse(p[1], out ulong tick) ? (long)(GetTickCount64() - tick) : -1;
            bool acceptee = Mode == 0 && age >= 0 && age <= AGE_MAX_MS;
            if (acceptee) { Interlocked.Increment(ref _acceptees); }
            Recus.Enqueue(new Charge(cd.dwData.ToInt64(), texte, age, acceptee));
            return acceptee ? new IntPtr(1) : IntPtr.Zero;
        }
        if (m == WM_BLOQUER)
        {
            _occupe = 1;
            Thread.Sleep((int)w);
            _occupe = 0;
            return IntPtr.Zero;
        }
        return DefWindowProcW(h, m, w, l);
    }

    public void Dispose()
    {
        // Le fil est en arriere-plan : il disparait avec le processus ; la fenetre message-only n'a rien de visible
        if (_hwnd != IntPtr.Zero) { PostMessageW(_hwnd, WM_CLOSE, IntPtr.Zero, IntPtr.Zero); }
    }
}
