# Fonctions communes des mesures de la phase 8 (à charger par `. $PSScriptRoot\commun.ps1`).
# - faux profil : APPDATA / LOCALAPPDATA redirigés dans CE processus PowerShell (et ses enfants) seulement ;
# - lancement sans voler le focus : CreateProcess avec SW_SHOWNOACTIVATE (la première fenêtre montrée par le processus
#   prend cette valeur) ; ne ferme jamais qu'un processus dont on connaît l'identifiant ;
# - médiane / p95 / min / max ; dossiers de test de N fichiers.
# Aucune entrée souris ni clavier.

Add-Type -TypeDefinition @"
using System; using System.Collections.Generic; using System.Runtime.InteropServices; using System.Text;
public static class NeLance {
    [StructLayout(LayoutKind.Sequential, CharSet = CharSet.Unicode)]
    struct STARTUPINFO { public int cb; public string lpReserved, lpDesktop, lpTitle; public int dwX, dwY, dwXSize, dwYSize, dwXCountChars, dwYCountChars, dwFillAttribute, dwFlags; public short wShowWindow, cbReserved2; public IntPtr lpReserved2, hStdInput, hStdOutput, hStdError; }
    [StructLayout(LayoutKind.Sequential)]
    struct PROCESS_INFORMATION { public IntPtr hProcess, hThread; public int dwProcessId, dwThreadId; }
    delegate bool EnumProc(IntPtr h, IntPtr l);
    [DllImport("kernel32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
    static extern bool CreateProcessW(string app, StringBuilder cmd, IntPtr pa, IntPtr ta, bool inherit, uint flags, IntPtr env, string dir, ref STARTUPINFO si, out PROCESS_INFORMATION pi);
    [DllImport("kernel32.dll")] static extern bool CloseHandle(IntPtr h);
    [DllImport("user32.dll", CharSet = CharSet.Unicode, SetLastError = true)] static extern IntPtr CreateDesktopW(string name, IntPtr dev, IntPtr mode, int flags, uint access, IntPtr sa);
    [DllImport("user32.dll")] static extern bool CloseDesktop(IntPtr d);
    [DllImport("user32.dll")] static extern bool EnumDesktopWindows(IntPtr d, EnumProc p, IntPtr l);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] static extern int GetClassNameW(IntPtr h, StringBuilder s, int n);
    [DllImport("user32.dll")] static extern IntPtr GetForegroundWindow();
    [DllImport("user32.dll")] static extern bool IsWindowVisible(IntPtr h);
    [DllImport("user32.dll")] static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
    static IntPtr bureau = IntPtr.Zero;
    // Bureau Windows à part : les fenêtres lancées dessus ne sont jamais montrées à l'utilisateur et ne prennent jamais son focus
    public static void OuvrirBureau(string nom) {
        bureau = CreateDesktopW(nom, IntPtr.Zero, IntPtr.Zero, 0, 0x10000000, IntPtr.Zero);
        if (bureau == IntPtr.Zero) throw new System.ComponentModel.Win32Exception(Marshal.GetLastWin32Error());
    }
    public static void FermerBureau() { if (bureau != IntPtr.Zero) { CloseDesktop(bureau); bureau = IntPtr.Zero; } }
    // showWindow : 4 = SW_SHOWNOACTIVATE, 0 = SW_HIDE ; sur le bureau à part si OuvrirBureau a été appelé
    public static int Lancer(string exe, string arguments, short showWindow, string nomBureau) {
        var si = new STARTUPINFO(); si.cb = Marshal.SizeOf(si); si.dwFlags = 1; si.wShowWindow = showWindow;
        if (bureau != IntPtr.Zero) si.lpDesktop = "WinSta0\\" + nomBureau;
        PROCESS_INFORMATION pi;
        var cmd = new StringBuilder("\"" + exe + "\" " + arguments);
        if (!CreateProcessW(null, cmd, IntPtr.Zero, IntPtr.Zero, false, 0, IntPtr.Zero, null, ref si, out pi)) throw new System.ComponentModel.Win32Exception(Marshal.GetLastWin32Error());
        CloseHandle(pi.hThread); CloseHandle(pi.hProcess);
        return pi.dwProcessId;
    }
    // Fenêtre visible de classe `classe` appartenant au processus `pid`, sur le bureau à part (ou le bureau courant) ; 0 si aucune
    public static long TrouverFenetre(string classe, uint pid) {
        long trouvee = 0;
        EnumProc p = delegate (IntPtr h, IntPtr l) {
            var s = new StringBuilder(64); GetClassNameW(h, s, 64);
            uint q; GetWindowThreadProcessId(h, out q);
            if (q == pid && s.ToString() == classe && IsWindowVisible(h)) { trouvee = h.ToInt64(); return false; }
            return true;
        };
        if (bureau != IntPtr.Zero) EnumDesktopWindows(bureau, p, IntPtr.Zero);
        return trouvee;
    }
    public static uint PidAuPremierPlan() { uint p = 0; GetWindowThreadProcessId(GetForegroundWindow(), out p); return p; }
}
"@

$script:NomBureau = $null
# Les fenêtres lancées ensuite par Lancer-SansFocus vivent sur un bureau Windows à part (nom unique), jamais devant l'utilisateur.
# Raison : SW_SHOWNOACTIVATE ne suffit pas, SpaceFiles prend le premier plan malgré lui (constaté : 20 lancements sur 20).
function Ouvrir-Bureau { $script:NomBureau = "nebench$PID"; [NeLance]::OuvrirBureau($script:NomBureau) }
function Fermer-Bureau { [NeLance]::FermerBureau(); $script:NomBureau = $null }
function Lancer-SansFocus([string]$Exe, [string]$Arguments = '', [switch]$Cache) {
    $id = [NeLance]::Lancer($Exe, $Arguments, $(if ($Cache) { 0 } else { 4 }), $script:NomBureau)
    Get-Process -Id $id
}
function Resume-Valeurs($valeurs) {
    $t = @($valeurs | ? { $_ -ne $null } | Sort-Object)
    if (-not $t.Count) { return $null }
    $n = $t.Count
    $med = if ($n % 2) { $t[[int][math]::Floor($n / 2)] } else { ($t[$n / 2 - 1] + $t[$n / 2]) / 2 }
    $p95 = $t[[int][math]::Max(0, [math]::Ceiling($n * 0.95) - 1)]
    [pscustomobject]@{ N = $n; Mediane = $med; P95 = $p95; Min = $t[0]; Max = $t[-1] }
}
function Texte-Resume($r, [string]$unite = '') {
    if (-not $r) { return 'aucune valeur' }
    "n={0} médiane={1:N2}{5} p95={2:N2}{5} min={3:N2}{5} max={4:N2}{5}" -f $r.N, $r.Mediane, $r.P95, $r.Min, $r.Max, $unite
}

# Faux profil sous %TEMP% : APPDATA / LOCALAPPDATA redirigés, aucun accès au registre de SpaceFiles
function Poser-FauxProfil([string]$Racine) {
    $local = Join-Path $Racine 'Local'; $roaming = Join-Path $Racine 'Roaming'
    New-Item -ItemType Directory -Force (Join-Path $local 'SpaceFiles'), (Join-Path $roaming 'SpaceFiles') | Out-Null
    $env:LOCALAPPDATA = $local; $env:APPDATA = $roaming; $env:SPACEFILES_SANS_REGISTRE = '1'
    [pscustomobject]@{ Local = $local; Roaming = $roaming; Journal = (Join-Path $local 'SpaceFiles\journal.txt') }
}

# Dossier de N fichiers vides : noms variés, 1 % de sous-dossiers, quelques noms Unicode (1 sur 50) et longs (1 sur 200)
function Creer-DossierTest([string]$Chemin, [int]$N) {
    if (Test-Path $Chemin) { Remove-Item $Chemin -Recurse -Force }
    New-Item -ItemType Directory -Force $Chemin | Out-Null
    $ext = 'txt', 'md', 'png', 'jpg', 'pdf', 'rs', 'zip', 'mp3', 'docx', 'log'
    $unicode = 'écrit-été', 'ñandú-ü', '日本語のファイル', 'Ünïcödé-ΑΒΓ', 'файл-тест', 'emoji-café'
    for ($i = 1; $i -le $N; $i++) {
        if ($i % 100 -eq 0) { [void][IO.Directory]::CreateDirectory((Join-Path $Chemin ('dossier-{0:D6}' -f $i))); continue }
        $nom = if ($i % 200 -eq 7) { ('nom-long-{0:D6}-' -f $i) + ('x' * 150) }
               elseif ($i % 50 -eq 3) { '{0}-{1:D6}' -f $unicode[$i % $unicode.Count], $i }
               else { 'fichier-{0:D6}' -f $i }
        $f = [IO.File]::Create((Join-Path $Chemin ($nom + '.' + $ext[$i % $ext.Count]))); $f.Dispose()
    }
    (Get-ChildItem $Chemin -Force | Measure-Object).Count
}

# Attend (au plus $delai ms) une ligne du journal du faux profil qui correspond au motif ; la renvoie ou $null
function Attendre-Journal([string]$Journal, [string]$Motif, [int]$Delai = 120000) {
    $sw = [Diagnostics.Stopwatch]::StartNew()
    while ($sw.ElapsedMilliseconds -lt $Delai) {
        if (Test-Path $Journal) {
            $l = Get-Content $Journal -Encoding UTF8 | ? { $_ -match $Motif } | Select-Object -Last 1
            if ($l) { return $l }
        }
        Start-Sleep -Milliseconds 500
    }
    $null
}

# Photographie des processus spacefiles / SpaceNotch : identifiant, ligne de commande (preuve que ceux de l'auteur ne bougent pas)
function Etat-Processus {
    Get-CimInstance Win32_Process | ? { $_.Name -match '^(spacefiles|SpaceNotch)' } | Sort-Object ProcessId |
        % { '{0} {1} démarré {2} : {3}' -f $_.ProcessId, $_.Name, $_.CreationDate.ToString('s'), $_.CommandLine }
}
