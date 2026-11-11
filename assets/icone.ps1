param([string]$Sortie)
# Icône de SpaceFiles : dossier en matrice de points blancs, point rouge au centre, sur carré noir arrondi
Add-Type -AssemblyName System.Drawing
$motif = @("###....", "#..####", "#.....#", "#..r..#", "#.....#", "#######")
$tailles = 16, 20, 24, 32, 40, 48, 64, 128, 256
$images = @()
foreach ($t in $tailles) {
  $b = New-Object Drawing.Bitmap $t, $t
  $g = [Drawing.Graphics]::FromImage($b)
  $g.SmoothingMode = 'AntiAlias'
  $g.Clear([Drawing.Color]::Transparent)
  # Fond : carré arrondi noir
  $r = [Math]::Max(2, $t * 0.22)
  $p = New-Object Drawing.Drawing2D.GraphicsPath
  $w = $t - 1
  $p.AddArc(0, 0, 2*$r, 2*$r, 180, 90); $p.AddArc($w-2*$r, 0, 2*$r, 2*$r, 270, 90)
  $p.AddArc($w-2*$r, $w-2*$r, 2*$r, 2*$r, 0, 90); $p.AddArc(0, $w-2*$r, 2*$r, 2*$r, 90, 90); $p.CloseFigure()
  # Sans fond : les points occupent toute l'icône
  $pas = $t * 0.96 / 7
  $x0 = ($t - 7 * $pas) / 2; $y0 = ($t - 6 * $pas) / 2 + $pas * 0.05
  $d = [Math]::Max(1.6, $pas * 0.72)
  $blanc = New-Object Drawing.SolidBrush ([Drawing.Color]::FromArgb(255, 240, 240, 240))
  $rouge = New-Object Drawing.SolidBrush ([Drawing.Color]::FromArgb(255, 215, 25, 33))
  for ($y = 0; $y -lt 6; $y++) { for ($x = 0; $x -lt 7; $x++) {
    $c = $motif[$y][$x]; if ($c -eq '.') { continue }
    $pinceau = if ($c -eq 'r') { $rouge } else { $blanc }
    $cx = $x0 + ($x + 0.5) * $pas; $cy = $y0 + ($y + 0.5) * $pas
    $g.FillEllipse($pinceau, [single]($cx - $d/2), [single]($cy - $d/2), [single]$d, [single]$d)
  } }
  $g.Dispose()
  $m = New-Object IO.MemoryStream; $b.Save($m, [Drawing.Imaging.ImageFormat]::Png); $b.Dispose()
  $images += ,@($t, $m.ToArray())
}
# Fichier ICO : en-tête, répertoire, images PNG
$f = New-Object IO.MemoryStream; $e = New-Object IO.BinaryWriter $f
$e.Write([uint16]0); $e.Write([uint16]1); $e.Write([uint16]$images.Count)
$decalage = 6 + 16 * $images.Count
foreach ($i in $images) {
  $t = $i[0]; $n = $i[1].Length; $c = if ($t -ge 256) { 0 } else { $t }
  $e.Write([byte]$c); $e.Write([byte]$c); $e.Write([byte]0); $e.Write([byte]0)
  $e.Write([uint16]1); $e.Write([uint16]32); $e.Write([uint32]$n); $e.Write([uint32]$decalage); $decalage += $n
}
foreach ($i in $images) { $e.Write($i[1]) }
$e.Flush(); [IO.File]::WriteAllBytes($Sortie, $f.ToArray())
"icone : $Sortie ($($f.Length) octets)"
# Aperçu agrandi
$apercu = [Drawing.Image]::FromStream((New-Object IO.MemoryStream (,$images[-1][1])))
$apercu.Save([IO.Path]::ChangeExtension($Sortie, ".png"))
