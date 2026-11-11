# Audit des contrastes des jetons de texte (WCAG) : lance le test `tableau_contrastes` de src/theme.rs, qui calcule chaque
# rapport à partir de la transformation de thème (sombre et clair), puis l'affiche en tableau.
#   - seuil 4,5:1 : encres, accent, erreur, encre sur accent, teinte « Nombre ». Une ligne ECHEC fait sortir ce script en code 1.
#   - seuil 3,0:1 : couleurs de groupes et de balises, à titre d'information (AVERT, jamais bloquant).
# Code de sortie : 0 tout est bon, 1 une paire de texte est sous 4,5:1, 2 le test n'a rien rendu (erreur de compilation...).
$ErrorActionPreference = 'Continue'
[Console]::OutputEncoding = [Text.Encoding]::UTF8
$env:Path += ";$env:USERPROFILE\.cargo\bin"
$racine = Resolve-Path (Join-Path $PSScriptRoot '..\..')
Push-Location $racine
try {
    $sortie = & cargo test --release tableau_contrastes -- --ignored --nocapture 2>$null
} finally {
    Pop-Location
}
$lignes = @($sortie | Where-Object { $_ -match '^[^;]+;(SOMBRE|CLAIR);[0-9.]+;[0-9.]+;(OK|ECHEC|AVERT)$' })
if ($lignes.Count -eq 0) {
    Write-Host 'AUDIT IMPOSSIBLE : le test tableau_contrastes n''a rien affiché (voir `cargo test --release tableau_contrastes`).'
    exit 2
}
$lignes_objets = foreach ($l in $lignes) {
    $c = $l -split ';'
    [pscustomobject]@{ Paire = $c[0]; Theme = $c[1]; Rapport = ('{0} : 1' -f $c[2]); Seuil = ('{0} : 1' -f $c[3]); Etat = $c[4] }
}
$lignes_objets | Format-Table -AutoSize | Out-String -Width 200 | Write-Host
$echecs = @($lignes_objets | Where-Object { $_.Etat -eq 'ECHEC' })
$avert = @($lignes_objets | Where-Object { $_.Etat -eq 'AVERT' })
Write-Host ('{0} paires, {1} sous le seuil de texte (4,5), {2} avertissements d''information (3,0).' -f $lignes_objets.Count, $echecs.Count, $avert.Count)
if ($echecs.Count) { Write-Host 'ECHEC : un jeton de texte est sous 4,5:1.'; exit 1 }
Write-Host 'OK : tous les jetons de texte sont à 4,5:1 ou plus.'
exit 0
