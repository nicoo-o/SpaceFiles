# Capture la fenêtre de SpaceFiles en PNG, sans injecter aucune entrée (ni souris ni clavier).
#
# PrintWindow demande à la fenêtre de se dessiner dans son propre bitmap : le curseur de l'utilisateur
# ne bouge pas, et la fenêtre n'a pas besoin d'être au premier plan (PW_RENDERFULLCONTENT pour le contenu
# Direct2D). Une capture ne prouve que ce qu'elle montre : si l'état voulu n'est pas à
# l'écran, le dire plutôt que conclure.
#
# Usage : ./tools/ui-snapshot/capture.ps1 -OutDir <dossier> -Name <nom> [-Count 4] [-IntervalMs 700]
param(
    [Parameter(Mandatory = $true)] [string] $OutDir,
    [string] $Name = 'capture',
    [int] $Count = 1,
    [int] $IntervalMs = 700,
    [string] $Title = 'SpaceFiles'
)

$ErrorActionPreference = 'Stop'

Add-Type -ReferencedAssemblies System.Drawing -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
public static class NeCapture {
    [StructLayout(LayoutKind.Sequential)] public struct RECT { public int Left, Top, Right, Bottom; }
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] public static extern IntPtr FindWindowW(string cls, string title);
    [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr hwnd, out RECT rect);
    [DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
    [DllImport("user32.dll")] public static extern bool PrintWindow(IntPtr hwnd, IntPtr hdc, uint flags);
}
'@ -ErrorAction Stop

Add-Type -AssemblyName System.Drawing

# Sans cela PowerShell est « non conscient du DPI » : GetWindowRect rend des dimensions réduites par le
# facteur d'échelle et la capture est rognée à droite et en bas.
[void][NeCapture]::SetProcessDPIAware()

# Classe de fenêtre « SpaceFiles » (main.rs) ; le titre sert de repli.
# [NullString]::Value : PowerShell convertit un $null en chaîne vide pour un paramètre string, et
# FindWindowW chercherait alors un titre vide (rien trouvé).
$hwnd = [NeCapture]::FindWindowW('SpaceFiles', [NullString]::Value)
if ($hwnd -eq [IntPtr]::Zero) {
    $hwnd = [NeCapture]::FindWindowW([NullString]::Value, $Title)
}
if ($hwnd -eq [IntPtr]::Zero) {
    throw "Fenêtre introuvable : lancer SpaceFiles d'abord (titre « $Title »)."
}

New-Item -ItemType Directory -Force -Path $OutDir | Out-Null

for ($i = 1; $i -le $Count; $i++) {
    $rect = New-Object NeCapture+RECT
    [void][NeCapture]::GetWindowRect($hwnd, [ref]$rect)
    $w = $rect.Right - $rect.Left
    $h = $rect.Bottom - $rect.Top

    $bmp = New-Object System.Drawing.Bitmap $w, $h
    $g = [System.Drawing.Graphics]::FromImage($bmp)
    $hdc = $g.GetHdc()
    # 2 = PW_RENDERFULLCONTENT : inclut le contenu composé (Direct2D).
    [void][NeCapture]::PrintWindow($hwnd, $hdc, 2)
    $g.ReleaseHdc($hdc)

    $file = Join-Path $OutDir ('{0}-{1:00}.png' -f $Name, $i)
    $bmp.Save($file, [System.Drawing.Imaging.ImageFormat]::Png)
    $g.Dispose()
    $bmp.Dispose()
    Write-Output $file

    if ($i -lt $Count) { Start-Sleep -Milliseconds $IntervalMs }
}
