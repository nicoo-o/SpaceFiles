// Preuve de la phase 5, lot A : `explorer.open {reuse:true}` (reutiliser une fenetre SpaceFiles ouverte) contre un resident
// SpaceFiles de TEST. AUCUNE fenetre visible : le resident est une copie de l'exe sous %TEMP% (faux profil, APPDATA /
// LOCALAPPDATA redirigees dans ce seul processus, SPACEFILES_SANS_REGISTRE=1, SPACEFILES_PIPE distinct, faux lanceur de
// fenetre) et la « fenetre SpaceFiles » est une fenetre MESSAGE-ONLY creee par ce harnais, de la classe donnee par
// SPACEFILES_CLASSE_CIBLE (variable lue par le resident seulement en mode essai). Elle repond 1, 0 ou ne repond pas, et note
// la charge WM_COPYDATA recue. Ne lance ni SpaceNotch ni le resident de l'auteur, ne touche pas a ses donnees (empreintes
// avant / apres), aucune entree souris.
//
// Usage (depuis le depot SpaceFiles, apres `cargo build --release`) :
//   dotnet run -c Release tools\bench\preuve-phase5-reuse.cs
#:property TargetFramework=net10.0
#:property Nullable=enable
#:property ImplicitUsings=enable
#:property AllowUnsafeBlocks=true

using System.Collections.Concurrent;
using System.Diagnostics;
using System.IO.Pipes;
using System.Runtime.InteropServices;
using System.Text;

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
Console.WriteLine($"== 0. Etat reel : empreintes prises (noms, tailles, dates) ; %LOCALAPPDATA%\\Eco {(ecoAbsentAvant ? "absent" : "present")} ; processus spacefiles.exe deja la : [{string.Join(",", pidsAvant.Order())}]");

string racine = Path.Combine(Path.GetTempPath(), "sf-phase5-reuse");
if (Directory.Exists(racine)) { Directory.Delete(racine, true); }
string profil = Path.Combine(racine, "faux-profil");
Directory.CreateDirectory(Path.Combine(profil, "Local", "SpaceFiles"));
Directory.CreateDirectory(Path.Combine(profil, "Roaming", "SpaceFiles"));
string exeSource = Path.GetFullPath(Path.Combine(Directory.GetCurrentDirectory(), "target", "release", "spacefiles.exe"));
if (!File.Exists(exeSource)) { throw new FileNotFoundException("lancer depuis le depot SpaceFiles apres cargo build --release", exeSource); }
string exeCopie = Path.Combine(racine, "spacefiles.exe");
File.Copy(exeSource, exeCopie, true);
string suffixe = "p5" + Environment.ProcessId;
string tube = "Eco.spacefiles." + suffixe;
string classe = "SfEssaiCible" + Environment.ProcessId;
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
        return lecteur.ReadToEnd().Split('\n', StringSplitOptions.RemoveEmptyEntries | StringSplitOptions.TrimEntries);
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

string Appeler(string ligne)
{
    using var c = new NamedPipeClientStream(".", tube, PipeDirection.InOut, PipeOptions.None);
    c.Connect(2000);
    c.Write(Encoding.UTF8.GetBytes(ligne + "\n"));
    c.Flush();
    var tampon = new List<byte>();
    int b;
    while ((b = c.ReadByte()) >= 0 && b != '\n') { tampon.Add((byte)b); }
    return Encoding.UTF8.GetString([.. tampon]);
}

static string Json(string s) => s.Replace("\\", "\\\\").Replace("\"", "\\\"");
string Ouvrir(string args) => Appeler("{\"v\":1,\"id\":\"p5\",\"kind\":\"call\",\"name\":\"explorer.open\",\"args\":" + args + "}");

try
{
    Ok(Attendre(() => { try { using var c = new NamedPipeClientStream(".", tube); c.Connect(100); return true; } catch { return false; } }, 5000),
        $"tube du resident de test (pid {resident.Id}) present");
    string cDossier = Json(dossier);
    string cFichier = Json(fichier);

    // ---------------------------------------------------------------- (b) reuse:true SANS fenetre (avant de creer la fenetre d'essai)
    Console.WriteLine("== (b) reuse:true sans fenetre : repli sur le lancement");
    string rep = Ouvrir($"{{\"path\":\"{cDossier}\",\"reuse\":true}}");
    Ok(rep.Contains("\"opened\":true", StringComparison.Ordinal), "reponse {opened:true} (la reponse ne dit pas si une fenetre est reutilisee)");
    Ok(Attendre(() => LireLignes().Length >= 1, 4000), "le faux lanceur recoit une ligne");
    string[] l = LireLignes();
    Ok(l.Length == 1 && l[0].StartsWith("--ouvrir ", StringComparison.Ordinal) && l[0].Contains("dossier d'essai", StringComparison.Ordinal), "ligne recue : --ouvrir \"<dossier>\" (" + (l.Length > 0 ? l[0] : "rien") + ")");

    // ---------------------------------------------------------------- fenetre d'essai message-only
    using var fenetre = new FenetreEssai(classe);
    Ok(fenetre.Prete, "fenetre message-only de classe " + classe + " creee dans ce processus");

    // ---------------------------------------------------------------- (a) reuse:true + fenetre qui repond 1
    Console.WriteLine("== (a) reuse:true + fenetre qui repond 1 : la charge arrive, le faux lanceur ne recoit RIEN");
    fenetre.Mode = 0;
    int lignesAvant = LireLignes().Length;
    rep = Ouvrir($"{{\"path\":\"{cFichier}\",\"reveal\":true,\"reuse\":true}}");
    Ok(rep.Contains("\"opened\":true", StringComparison.Ordinal), "reponse {opened:true}");
    Ok(Attendre(() => fenetre.Recus.Count >= 1, 4000), "la fenetre recoit un WM_COPYDATA");
    var recu = fenetre.Recus.TryPeek(out var r0) ? r0 : default;
    Console.WriteLine($"   dwData = 0x{recu.DwData:X}, charge = {recu.Texte.Replace("\n", "\\n")}");
    Ok(recu.DwData == 0x4E450002, "dwData = OUVRIR_ICI (0x4E450002)");
    string[] pa = recu.Texte.Split('\n', 3);
    Ok(pa.Length == 3 && pa[0] == "1" && ulong.TryParse(pa[1], out _) && pa[2] == fichier, "charge = \"1\\n<tick>\\n<chemin>\" (reveler vrai, instant d'envoi, chemin normalise)");
    Ok(recu.AgeMs >= 0 && recu.AgeMs <= 1500 && recu.Acceptee, $"age de la charge a la reception {recu.AgeMs} ms (<= 1500) : acceptee par la regle d'age");
    Thread.Sleep(700);
    Ok(LireLignes().Length == lignesAvant, "le faux lanceur n'a rien recu (aucune fenetre lancee)");

    // ---------------------------------------------------------------- (c) fenetre qui repond 0
    Console.WriteLine("== (c) fenetre qui repond 0 : repli");
    fenetre.Mode = 1;
    int recusAvant = fenetre.Recus.Count;
    rep = Ouvrir($"{{\"path\":\"{cDossier}\",\"reuse\":true}}");
    Ok(rep.Contains("\"opened\":true", StringComparison.Ordinal), "reponse {opened:true}");
    Ok(Attendre(() => LireLignes().Length >= lignesAvant + 1, 4000), "le faux lanceur recoit la ligne apres le refus");
    fenetre.Recus.TryPeek(out _);
    var toutes = fenetre.Recus.ToArray();
    Ok(toutes.Length == recusAvant + 1 && toutes[^1].Texte.Split('\n', 3) is ["0", _, var cc] && cc == dossier, "la fenetre a bien ete essayee d'abord (charge \"0\\n<tick>\\n<dossier>\")");
    l = LireLignes();
    Ok(l[^1].StartsWith("--ouvrir ", StringComparison.Ordinal), "ligne recue : " + l[^1]);

    // ---------------------------------------------------------------- (d) fenetre qui ne repond pas
    Console.WriteLine("== (d) fenetre figee 4 s AVANT de lire le message : repli borne, le message abandonne n'est jamais livre (un seul lancement)");
    fenetre.Mode = 0;
    lignesAvant = LireLignes().Length;
    recusAvant = fenetre.Recus.Count;
    int acceptees0 = fenetre.Acceptees;
    fenetre.Bloquer(4000);
    Ok(Attendre(() => !fenetre.Libre, 2000), "la fenetre est bloquee (4 s) sans lire sa file de messages");
    var sw = Stopwatch.StartNew();
    long debutD = Environment.TickCount64;
    rep = Ouvrir($"{{\"path\":\"{cDossier}\",\"reuse\":true}}");
    long reponseMs = sw.ElapsedMilliseconds;
    Ok(rep.Contains("\"opened\":true", StringComparison.Ordinal) && reponseMs < 500, $"reponse {{opened:true}} tout de suite ({reponseMs} ms), sans attendre la fenetre");
    bool replie = Attendre(() => LireLignes().Length >= lignesAvant + 1, 6000);
    long repliMs = sw.ElapsedMilliseconds;
    Console.WriteLine($"   repli recu apres {repliMs} ms (delai du resident : 2 000 ms) ; la fenetre est bloquee 4 000 ms");
    Ok(replie && repliMs >= 1500 && repliMs < 3500, "repli dans un delai borne : entre 1,5 s et 3,5 s (SendMessageTimeout 2 s)");
    Ok(fenetre.Recus.Count == recusAvant, "la fenetre n'a pas encore lu la charge au moment du repli");
    Ok(!resident.HasExited, "le resident est toujours vivant");
    Ok(Attendre(() => fenetre.Libre, 6000), "la fenetre se debloque");
    Thread.Sleep(500);
    Ok(fenetre.Recus.Count == recusAvant && fenetre.Acceptees == acceptees0, "le message dont l'envoi a expire n'est JAMAIS livre ensuite (Windows l'abandonne) : rien ne s'ouvre cote fenetre");
    Ok(LireLignes().Length == lignesAvant + 1, "ouvertures au total pour cette demande : 1 (le lancement du repli, pas de doublon)");

    // ---------------------------------------------------------------- (d1) la fenetre dont l'envoi a expire est sautee 10 s
    Console.WriteLine("== (d1) fenetre dont l'envoi vient d'expirer : sautee pendant 10 s (repli immediat, la fenetre n'est pas interrogee)");
    lignesAvant = LireLignes().Length;
    recusAvant = fenetre.Recus.Count;
    sw.Restart();
    rep = Ouvrir($"{{\"path\":\"{cDossier}\",\"reuse\":true}}");
    Ok(Attendre(() => LireLignes().Length >= lignesAvant + 1, 1500), $"repli sans attendre les 2 s ({sw.ElapsedMilliseconds} ms)");
    Thread.Sleep(300);
    Ok(fenetre.Recus.Count == recusAvant, "la fenetre n'a pas ete interrogee (oubliee 10 s)");
    long reste = 12_500 - (Environment.TickCount64 - debutD);
    Console.WriteLine($"   attente de {Math.Max(0, reste)} ms pour que l'oubli (10 s) expire");
    if (reste > 0) { Thread.Sleep((int)reste); }

    // ---------------------------------------------------------------- (d2) fenetre lente : la charge arrive entre 1,5 s et 2 s
    Console.WriteLine("== (d2) fenetre bloquee 1,8 s : la charge est livree TARD (age > 1,5 s) mais avant le renoncement du resident : la fenetre repond 0, n'ouvre rien, un seul lancement");
    lignesAvant = LireLignes().Length;
    recusAvant = fenetre.Recus.Count;
    acceptees0 = fenetre.Acceptees;
    fenetre.Bloquer(1800);
    Ok(Attendre(() => !fenetre.Libre, 2000), "la fenetre est bloquee (1,8 s)");
    sw.Restart();
    rep = Ouvrir($"{{\"path\":\"{cDossier}\",\"reuse\":true}}");
    Ok(rep.Contains("\"opened\":true", StringComparison.Ordinal), "reponse {opened:true}");
    Ok(Attendre(() => LireLignes().Length >= lignesAvant + 1, 6000), "le faux lanceur recoit la ligne du repli");
    Console.WriteLine($"   repli recu apres {sw.ElapsedMilliseconds} ms");
    Ok(Attendre(() => fenetre.Recus.Count == recusAvant + 1, 3000), "la fenetre a lu la charge (en retard)");
    var lente = fenetre.Recus.ToArray()[^1];
    Console.WriteLine($"   charge tardive : age {lente.AgeMs} ms, decision {(lente.Acceptee ? "acceptee" : "refusee (0)")}");
    Ok(lente.AgeMs > 1500 && lente.AgeMs < 2000 && !lente.Acceptee, "age entre 1 500 et 2 000 ms : refusee par la regle d'age (reponse 0)");
    Ok(fenetre.Acceptees == acceptees0, "aucune ouverture cote fenetre");
    Thread.Sleep(300);
    Ok(LireLignes().Length == lignesAvant + 1, "ouvertures au total pour cette demande : 1 (le lancement du repli, pas de doublon)");
    Ok(!resident.HasExited, "le resident est toujours vivant");
    Attendre(() => fenetre.Libre, 3000);

    // ---------------------------------------------------------------- (e) reuse:false avec fenetre
    Console.WriteLine("== (e) reuse:false avec fenetre : le faux lanceur recoit la ligne, la fenetre ne recoit rien");
    fenetre.Mode = 0;
    lignesAvant = LireLignes().Length;
    recusAvant = fenetre.Recus.Count;
    rep = Ouvrir($"{{\"path\":\"{cDossier}\",\"reuse\":false}}");
    Ok(rep.Contains("\"opened\":true", StringComparison.Ordinal), "reponse {opened:true}");
    Ok(Attendre(() => LireLignes().Length >= lignesAvant + 1, 4000), "le faux lanceur recoit la ligne");
    Thread.Sleep(500);
    Ok(fenetre.Recus.Count == recusAvant, "la fenetre n'a rien recu");

    // ---------------------------------------------------------------- (f) reuse de mauvais type
    Console.WriteLine("== (f) types faux");
    lignesAvant = LireLignes().Length;
    recusAvant = fenetre.Recus.Count;
    foreach (string faux in new[] { "\"oui\"", "1", "[]" })
    {
        rep = Ouvrir($"{{\"path\":\"{cDossier}\",\"reuse\":{faux}}}");
        Ok(rep.Contains("\"code\":\"invalide\"", StringComparison.Ordinal), $"reuse:{faux} -> invalide");
    }
    Thread.Sleep(400);
    Ok(LireLignes().Length == lignesAvant && fenetre.Recus.Count == recusAvant, "aucune ouverture, aucune charge pour une demande invalide");
    Ok(!resident.HasExited, "le resident est toujours vivant");

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

// ------------------------------------------------------------------ (g) etat reel
Console.WriteLine("== (g) etat reel");
string[] apres = reels.Select(Instantane).ToArray();
for (int i = 0; i < reels.Length; i++) { Ok(avant[i] == apres[i], "donnees reelles inchangees : " + reels[i]); }
Ok(ecoAbsentAvant == !Directory.Exists(reels[0]), "%LOCALAPPDATA%\\Eco : meme etat qu'avant (" + (ecoAbsentAvant ? "absent" : "present") + ")");
Ok(PidsSpaceFiles().SetEquals(pidsAvant), "liste des processus spacefiles.exe identique a celle d'avant");

Console.WriteLine(echecs == 0 ? "== TOUT EST OK" : $"== {echecs} ECHEC(S)");
return echecs == 0 ? 0 : 1;

// ====================================================================== fenetre message-only d'essai
readonly record struct Charge(long DwData, string Texte, long AgeMs, bool Acceptee);

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
