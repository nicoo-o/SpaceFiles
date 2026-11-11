# Décompose le working set d'un processus (le Hub lancé pour l'essai) : image (DLL / exe), mappé, privé ; pour les images, par module.
# Lecture seule (VirtualQueryEx + QueryWorkingSetEx), rien n'est installé ni modifié dans le processus. Aucune entrée souris.
# Sert au lot D de la phase 6 (E-4) : savoir ce que contiennent les ~140 Mo (WinUI / Windows App SDK natif, runtime .NET, code managé).
# Limite : le tas managé ne se distingue pas du reste de la mémoire privée (il faudrait dotnet-counters ou un GC.GetTotalMemory interne).
# Usage : powershell -NoProfile -File tools\bench\decomposer-hub.ps1 -ProcessId <pid>
param([Parameter(Mandatory = $true)][int]$ProcessId, [int]$Haut = 18)
Add-Type -TypeDefinition @"
using System; using System.Collections.Generic; using System.Runtime.InteropServices;
public static class Decomp {
    [StructLayout(LayoutKind.Sequential)] public struct MBI { public IntPtr BaseAddress, AllocationBase; public uint AllocationProtect; public IntPtr RegionSize; public uint State, Protect, Type; }
    [StructLayout(LayoutKind.Sequential)] public struct WSEX { public IntPtr VirtualAddress; public UIntPtr Flags; }
    [DllImport("kernel32.dll")] static extern IntPtr OpenProcess(uint access, bool inherit, int pid);
    [DllImport("kernel32.dll")] static extern bool CloseHandle(IntPtr h);
    [DllImport("kernel32.dll")] static extern UIntPtr VirtualQueryEx(IntPtr h, IntPtr addr, out MBI mbi, UIntPtr len);
    [DllImport("psapi.dll")] static extern bool QueryWorkingSetEx(IntPtr h, [In, Out] WSEX[] info, int size);
    // Pour chaque région validée : (type, base d'allocation, pages résidentes, pages résidentes privées, pages validées)
    public static List<long[]> Regions(int pid) {
        var res = new List<long[]>();
        IntPtr h = OpenProcess(0x0410, false, pid);
        if (h == IntPtr.Zero) throw new Exception("OpenProcess a échoué");
        try {
            long addr = 0; MBI mbi;
            while (VirtualQueryEx(h, (IntPtr)addr, out mbi, (UIntPtr)Marshal.SizeOf(typeof(MBI))) != UIntPtr.Zero) {
                long size = mbi.RegionSize.ToInt64();
                if (mbi.State == 0x1000) {
                    int pages = (int)(size / 4096);
                    long resident = 0, priv = 0;
                    for (int start = 0; start < pages; start += 4096) {
                        int n = Math.Min(4096, pages - start);
                        var info = new WSEX[n];
                        for (int i = 0; i < n; i++) info[i].VirtualAddress = (IntPtr)(addr + (long)(start + i) * 4096);
                        if (!QueryWorkingSetEx(h, info, n * Marshal.SizeOf(typeof(WSEX)))) continue;
                        for (int i = 0; i < n; i++) {
                            ulong f = (ulong)info[i].Flags;
                            if ((f & 1) != 0) { resident++; if (((f >> 1) & 7) <= 1) priv++; }
                        }
                    }
                    res.Add(new long[] { mbi.Type, mbi.AllocationBase.ToInt64(), size, resident * 4096, priv * 4096 });
                }
                addr += size;
            }
        } finally { CloseHandle(h); }
        return res;
    }
}
"@
$proc = Get-Process -Id $ProcessId
$mods = @{}
foreach ($m in $proc.Modules) { $mods[$m.BaseAddress.ToInt64()] = $m.ModuleName }
$regions = [Decomp]::Regions($ProcessId)
$types = @{ 0x1000000 = 'Image (DLL, exe)'; 0x40000 = 'Mappé (fichiers, sections)'; 0x20000 = 'Privé (tas natif, tas managé, piles)' }
"Working set du processus {0} ({1}) : {2:N1} Mio (WorkingSet64) ; QueryWorkingSetEx : {3:N1} Mio résidents, dont {4:N1} Mio non partagés" -f $ProcessId, $proc.ProcessName, ($proc.WorkingSet64 / 1MB), (($regions | ForEach-Object { $_[3] } | Measure-Object -Sum).Sum / 1MB), (($regions | ForEach-Object { $_[4] } | Measure-Object -Sum).Sum / 1MB)
foreach ($t in $types.Keys | Sort-Object) {
    $r = $regions | Where-Object { $_[0] -eq $t }
    "{0,-42} résident {1,7:N1} Mio   non partagé {2,7:N1} Mio   validé {3,7:N1} Mio" -f $types[$t], (($r | ForEach-Object { $_[3] } | Measure-Object -Sum).Sum / 1MB), (($r | ForEach-Object { $_[4] } | Measure-Object -Sum).Sum / 1MB), (($r | ForEach-Object { $_[2] } | Measure-Object -Sum).Sum / 1MB)
}
"-- Images, par module (résident / non partagé, Mio) --"
$parModule = @{}
foreach ($r in $regions | Where-Object { $_[0] -eq 0x1000000 }) {
    $nom = if ($mods.ContainsKey($r[1])) { $mods[$r[1]] } else { ('(image à 0x{0:X})' -f $r[1]) }
    if (-not $parModule.ContainsKey($nom)) { $parModule[$nom] = @(0L, 0L) }
    $parModule[$nom][0] += $r[3]; $parModule[$nom][1] += $r[4]
}
$parModule.GetEnumerator() | Sort-Object { $_.Value[0] } -Descending | Select-Object -First $Haut | ForEach-Object { "{0,-55} {1,7:N1} / {2,7:N1}" -f $_.Key, ($_.Value[0] / 1MB), ($_.Value[1] / 1MB) }
$reste = ($parModule.GetEnumerator() | Sort-Object { $_.Value[0] } -Descending | Select-Object -Skip $Haut | ForEach-Object { $_.Value[0] } | Measure-Object -Sum).Sum
"{0,-55} {1,7:N1}" -f "(autres modules : $([math]::Max(0, $parModule.Count - $Haut)))", ($reste / 1MB)
