// Rendu de la grille d'un terminal (T2b) : sort de rendu.rs pour garder celui-ci lisible.
// Le texte se dessine par « plages » de cellules aux attributs identiques (un DrawText par plage, pas par case) ; tout ce qui
// n'est pas de l'ASCII imprimable est posé case par case, car une police de secours n'a pas la chasse de la police mono et
// décalerait le reste de la ligne. La traduction des attributs en couleurs (`style_de`) est pure et testée sans fenêtre.

use super::*;
use crate::terminal::panneau::Panneau;
use crate::terminal::vt::{Attr, Couleur};

/// Ce que le dessin du panneau a besoin de savoir d'un terminal affiché.
pub struct VueTerminal<'a> {
    pub panneau: &'a Panneau,
    /// Le clavier va-t-il au terminal ? (curseur plein ; sinon, simple contour)
    pub focus: bool,
    /// Dossier de chaque onglet du panneau, dans l'ordre (T4a) : le dessin en fait les libellés selon la place
    pub onglets: Vec<String>,
    /// Rang de l'onglet dont `panneau` est l'état
    pub actif: usize,
    /// T4c : l'onglet JOURNAL est affiché (les opérations, de la plus ancienne à la plus récente) ; `None` = un shell est affiché
    pub journal: Option<&'a [crate::journal::Operation]>,
    /// T5 : la ligne en cours de saisie (tenue par SpaceFiles à l'invite), dessinée par-dessus la rangée du curseur du shell,
    /// avec sa suggestion grise et son propre curseur ; `None` = le curseur du shell suffit
    pub ligne: Option<crate::terminal::panneau::ligne::VueLigne>,
}

/// Taille du texte du terminal en DIP, avant « Taille du texte » (JetBrains Mono, décision D-T1)
const TAILLE_TERMINAL: f32 = 13.0;

const FG_DEFAUT: D2D1_COLOR_F = rgba(0xE6, 0xE6, 0xE6, 1.0);
const FG_GRAS: D2D1_COLOR_F = rgba(255, 255, 255, 1.0);
/// Fond d'une cellule vidéo inverse sans fond propre : le noir du panneau
const FOND_TERMINAL: D2D1_COLOR_F = VOILE;
const SELECTION: D2D1_COLOR_F = rgba(0xE8, 0xA6, 0x6B, 0.32);
const CURSEUR: D2D1_COLOR_F = rgba(255, 255, 255, 0.85);
/// La suggestion tirée de l'historique : un gris moyen, lisible sur le fond noir comme sur le fond clair
const SUGGESTION: D2D1_COLOR_F = rgba(0x80, 0x80, 0x80, 1.0);

/// Les 16 couleurs ANSI : lisibles sur fond noir (le « noir » est un gris sombre, sinon il disparaît)
const ANSI: [(u8, u8, u8); 16] = [
    (0x2B, 0x2B, 0x2B),
    (0xE5, 0x53, 0x4B),
    (0x7D, 0xBE, 0x6C),
    (0xE5, 0xC0, 0x7B),
    (0x5C, 0x9D, 0xED),
    (0xC6, 0x78, 0xDD),
    (0x56, 0xB6, 0xC2),
    (0xC8, 0xC8, 0xC8),
    (0x6B, 0x6B, 0x6B),
    (0xFF, 0x6B, 0x63),
    (0x98, 0xD9, 0x82),
    (0xF2, 0xD4, 0x8A),
    (0x7B, 0xB2, 0xFF),
    (0xDA, 0x8F, 0xF0),
    (0x73, 0xCF, 0xDB),
    (0xFF, 0xFF, 0xFF),
];

/// Texte par défaut en thème clair (19/11/2026, retour de l'auteur : la sortie de `ping` en E6E6E6 sur fond clair était
/// invisible). Aucune composante sous 0,1 à la fois : `theme::vers_clair` prendrait ces teintes pour un fond noir.
const FG_DEFAUT_CLAIR: D2D1_COLOR_F = rgba(0x24, 0x29, 0x2E, 1.0);
const FG_GRAS_CLAIR: D2D1_COLOR_F = rgba(0x1C, 0x1C, 0x1C, 1.0);

/// Les 16 couleurs ANSI en thème clair : mêmes teintes que `ANSI`, assombries pour dépasser 4,5:1 sur le fond #F7F7F8
/// (le « blanc » devient un gris moyen, le « blanc vif » reste blanc pur : `vers_clair` en fait l'encre).
const ANSI_CLAIR: [(u8, u8, u8); 16] = [
    (0x24, 0x29, 0x2E),
    (0xCF, 0x22, 0x2E),
    (0x11, 0x63, 0x29),
    (0x7D, 0x4E, 0x00),
    (0x05, 0x50, 0xAE),
    (0x82, 0x50, 0xDF),
    (0x1B, 0x7C, 0x83),
    (0x6E, 0x77, 0x81),
    (0x57, 0x60, 0x6A),
    (0xA4, 0x0E, 0x26),
    (0x1A, 0x7F, 0x37),
    (0x63, 0x3C, 0x01),
    (0x09, 0x69, 0xDA),
    (0x6A, 0x3F, 0xC4),
    (0x14, 0x7C, 0x8C),
    (0xFF, 0xFF, 0xFF),
];

/// Comme `rvb_indexe`, avec la palette du thème : seules les 16 premières couleurs changent (cube et gris restent).
fn rvb_du_theme(i: u8, clair: bool) -> (u8, u8, u8) {
    if clair && i < 16 {
        ANSI_CLAIR[i as usize]
    } else {
        rvb_indexe(i)
    }
}

/// Couleur d'un indice de la palette 256 : 0-15 ANSI, 16-231 cube 6 × 6 × 6, 232-255 gris
pub(super) fn rvb_indexe(i: u8) -> (u8, u8, u8) {
    match i {
        0..=15 => ANSI[i as usize],
        16..=231 => {
            let k = i - 16;
            let niveau = |n: u8| if n == 0 { 0 } else { 55 + 40 * n };
            (niveau(k / 36), niveau((k / 6) % 6), niveau(k % 6))
        }
        _ => {
            let g = 8 + 10 * (i - 232);
            (g, g, g)
        }
    }
}

/// Ce qu'on dessine pour une cellule une fois les attributs résolus
#[derive(Clone, Copy)]
pub(super) struct Style {
    pub fg: D2D1_COLOR_F,
    /// `None` : le fond du panneau, rien à remplir
    pub bg: Option<D2D1_COLOR_F>,
    pub souligne: bool,
    pub barre: bool,
    pub cache: bool,
}

/// Traduit les attributs d'une cellule en couleurs. Le gras éclaircit (les 8 premières couleurs passent aux 8 suivantes) :
/// la police n'a qu'une graisse, on ne la simule jamais. L'inverse échange avant/arrière-plan ; le pâle baisse l'opacité du texte.
pub(super) fn style_de(a: &Attr) -> Style {
    style_pour(a, crate::theme::clair())
}

/// `style_de` avec le thème passé en paramètre (testable sans état global).
pub(super) fn style_pour(a: &Attr, clair: bool) -> Style {
    let gras = a.a(Attr::GRAS);
    let fg_propre = match a.fg {
        Couleur::Defaut => None,
        Couleur::Indexee(i) => {
            let i = if gras && i < 8 { i + 8 } else { i };
            let (r, g, b) = rvb_du_theme(i, clair);
            Some(rgba(r, g, b, 1.0))
        }
        Couleur::Rvb(r, g, b) => Some(rgba(r, g, b, 1.0)),
    };
    let bg_propre = match a.bg {
        Couleur::Defaut => None,
        Couleur::Indexee(i) => {
            let (r, g, b) = rvb_du_theme(i, clair);
            Some(rgba(r, g, b, 1.0))
        }
        Couleur::Rvb(r, g, b) => Some(rgba(r, g, b, 1.0)),
    };
    let (defaut, defaut_gras) = if clair { (FG_DEFAUT_CLAIR, FG_GRAS_CLAIR) } else { (FG_DEFAUT, FG_GRAS) };
    let fg_effectif = fg_propre.unwrap_or(if gras { defaut_gras } else { defaut });
    let (mut fg, bg) = if a.a(Attr::INVERSE) { (bg_propre.unwrap_or(FOND_TERMINAL), Some(fg_effectif)) } else { (fg_effectif, bg_propre) };
    if a.a(Attr::PALE) {
        fg.a *= 0.6;
    }
    Style { fg, bg, souligne: a.a(Attr::SOULIGNE), barre: a.a(Attr::BARRE), cache: a.a(Attr::CACHE) }
}

/// Format du texte du terminal : mono, sans retour à la ligne, en haut à gauche, sans points de suspension
/// (le trimming de `format_mono` est fait pour des noms, pas pour une grille).
pub(super) fn format_terminal(dw: &IDWriteFactory) -> Result<IDWriteTextFormat> {
    unsafe {
        let col = collection_mono(dw);
        let nom = if col.is_some() { POLICE_TEXTE } else { POLICE_TEXTE_SECOURS };
        let f = dw.CreateTextFormat(
            &HSTRING::from(nom),
            col.as_ref(),
            DWRITE_FONT_WEIGHT_NORMAL,
            DWRITE_FONT_STYLE_NORMAL,
            DWRITE_FONT_STRETCH_NORMAL,
            TAILLE_TERMINAL * crate::reglages::echelle_texte(),
            &HSTRING::from("fr-fr"),
        )?;
        f.SetWordWrapping(DWRITE_WORD_WRAPPING_NO_WRAP)?;
        Ok(f)
    }
}

/// Largeur et hauteur d'une cellule en DIP : la largeur est celle de 20 « M » divisée par 20 (une police mono a la même
/// avance partout ; mesurer vingt caractères évite l'arrondi d'un seul), la hauteur celle d'une ligne, arrondie au DIP.
pub(super) fn mesurer_cellule(dw: &IDWriteFactory, format: &IDWriteTextFormat) -> (f32, f32) {
    let m: Vec<u16> = "M".repeat(20).encode_utf16().collect();
    unsafe {
        let Ok(layout) = dw.CreateTextLayout(&m, format, 10_000.0, 1_000.0) else { return (8.0, 17.0) };
        let mut mesure = DWRITE_TEXT_METRICS::default();
        if layout.GetMetrics(&mut mesure).is_err() || mesure.width <= 0.0 || mesure.height <= 0.0 {
            return (8.0, 17.0);
        }
        (mesure.width / 20.0, mesure.height.round().max(1.0))
    }
}

impl Rendu {
    /// Taille d'une cellule du terminal en DIP (largeur, hauteur) : sert à compter les colonnes et les lignes d'une zone.
    pub fn cellule_terminal(&self) -> (f32, f32) {
        self.cellule_terminal
    }

    /// Dessine la grille dans `zone` : fonds, sélection, texte, décorations, puis curseur. Tout est rogné à la zone.
    ///
    /// # Safety
    /// Fil d'interface seulement, entre `BeginDraw` et `EndDraw` : `c` doit être la cible de rendu valide de la fenêtre.
    pub(super) unsafe fn dessiner_grille_terminal(&self, c: &Cible2D, zone: D2D_RECT_F, v: &VueTerminal) {
        let (cw, ch) = self.cellule_terminal;
        if zone.right - zone.left < cw || zone.bottom - zone.top < ch {
            return;
        }
        let rt = &c.rt;
        rt.PushAxisAlignedClip(&zone, D2D1_ANTIALIAS_MODE_ALIASED);
        let p = v.panneau;
        let vt = p.terminal();
        let rangees = (((zone.bottom - zone.top) / ch).floor() as usize).min(vt.lignes());
        // Bord gauche de la colonne `k`, calé sur le pixel : deux fonds voisins ne laissent pas de filet clair entre eux
        let x_de = |k: usize| zone.left + (k as f32 * cw).round();
        let y_de = |i: usize| zone.top + i as f32 * ch;

        for i in 0..rangees {
            let Some(ligne) = p.ligne_visible(i) else { continue };
            let y = y_de(i);
            let colonnes = ligne.len().min(vt.colonnes());
            // 1. Fonds (un rectangle par plage de même fond)
            let mut k = 0;
            while k < colonnes {
                let st = style_de(&ligne[k].attr);
                let mut fin = k + 1;
                while fin < colonnes && style_de(&ligne[fin].attr).bg.map(|b| (b.r, b.g, b.b)) == st.bg.map(|b| (b.r, b.g, b.b)) {
                    fin += 1;
                }
                if let Some(bg) = st.bg {
                    self.couleur(c, &bg);
                    rt.FillRectangle(&rect(x_de(k), y, x_de(fin) - x_de(k), ch), &c.pinceau);
                }
                k = fin;
            }
            // 2. Sélection
            if let Some((debut, fin)) = p.selection_sur_rangee(i) {
                let fin = fin.min(vt.colonnes());
                if fin > debut {
                    self.couleur(c, &SELECTION);
                    rt.FillRectangle(&rect(x_de(debut), y, x_de(fin) - x_de(debut), ch), &c.pinceau);
                }
            }
            // 3. Texte, par plages de même attribut ; l'ASCII s'enchaîne, le reste se pose case par case
            let mut k = 0;
            while k < colonnes {
                let cel = ligne[k];
                if cel.largeur == 0 || (cel.c == ' ' && !cel.attr.a(Attr::SOULIGNE) && !cel.attr.a(Attr::BARRE)) {
                    k += 1;
                    continue;
                }
                let st = style_de(&cel.attr);
                let ascii = cel.c.is_ascii_graphic() || cel.c == ' ';
                if !ascii {
                    let w = if cel.largeur >= 2 { 2 } else { 1 };
                    self.glyphes_terminal(c, &st, &cel.c.to_string(), x_de(k), y, x_de(k + w) - x_de(k));
                    k += w.max(1);
                    continue;
                }
                let mut fin = k + 1;
                let mut texte = String::new();
                texte.push(cel.c);
                while fin < colonnes && ligne[fin].attr == cel.attr && (ligne[fin].c.is_ascii_graphic() || ligne[fin].c == ' ') && ligne[fin].largeur == 1 {
                    texte.push(ligne[fin].c);
                    fin += 1;
                }
                self.glyphes_terminal(c, &st, &texte, x_de(k), y, x_de(fin) - x_de(k));
                k = fin;
            }
        }

        // 3 bis. T5 : la ligne en cours de saisie, sur la rangée du curseur du shell (qui n'a rien reçu), puis sa suggestion grise
        if let (Some(l), Some(rangee)) = (&v.ligne, p.rangee_du_curseur()) {
            if rangee < rangees {
                let y = y_de(rangee);
                // Le texte par défaut du thème (E6E6E6 serait invisible sur le fond clair, `vers_clair` ne le touche pas)
                let fg = if crate::theme::clair() { FG_DEFAUT_CLAIR } else { FG_DEFAUT };
                let normal = Style { fg, bg: None, souligne: false, barre: false, cache: false };
                // Fond propre sous la ligne et sa suggestion : une invite à droite (oh-my-posh…) ne se superpose pas au texte tapé
                let fin_col = (l.col + l.visibles.len() + l.suggestion.len()).min(vt.colonnes());
                self.couleur(c, &FOND_TERMINAL);
                rt.FillRectangle(&rect(x_de(l.col), y, x_de(fin_col) - x_de(l.col), ch), &c.pinceau);
                let gris = Style { fg: SUGGESTION, ..normal };
                for (suite, st) in [(&l.visibles, &normal), (&l.suggestion, &gris)] {
                    let debut = if std::ptr::eq(suite, &l.visibles) { l.col } else { l.col + l.visibles.len() };
                    if suite.iter().all(|ch| ch.is_ascii_graphic() || *ch == ' ') {
                        let texte: String = suite.iter().collect();
                        self.glyphes_terminal(c, st, &texte, x_de(debut), y, x_de(debut + suite.len()) - x_de(debut));
                    } else {
                        // Hors ASCII : case par case, comme la grille (une police de secours décalerait la suite)
                        for (k, ch) in suite.iter().enumerate() {
                            self.glyphes_terminal(c, st, &ch.to_string(), x_de(debut + k), y, x_de(debut + k + 1) - x_de(debut + k));
                        }
                    }
                }
                // T6 : badge CMD / PS tout à droite de la rangée, seulement s'il reste une case libre entre la ligne et lui
                let badge = l.badge();
                let col_badge = vt.colonnes().saturating_sub(badge.len());
                if fin_col + 1 < col_badge {
                    self.couleur(c, &FOND_TERMINAL);
                    rt.FillRectangle(&rect(x_de(col_badge), y, x_de(vt.colonnes()) - x_de(col_badge), ch), &c.pinceau);
                    self.glyphes_terminal(c, &gris, badge, x_de(col_badge), y, x_de(vt.colonnes()) - x_de(col_badge));
                }
            }
        }

        // 4. Curseur (visible seulement tant qu'on regarde l'écran courant) ; avec une ligne de saisie, c'est le sien
        if let (Some(rangee), true) = (p.rangee_du_curseur(), vt.curseur_visible() && p.actif()) {
            let col = match &v.ligne {
                Some(l) => l.col + l.curseur,
                None => vt.curseur().0,
            };
            if rangee < rangees && col < vt.colonnes() {
                let (x, y) = (x_de(col), y_de(rangee));
                let (x2, forme) = (x_de(col + 1), vt.forme_curseur());
                if !v.focus {
                    self.couleur(c, &CURSEUR);
                    rt.DrawRectangle(&rect(x + 0.5, y + 0.5, x2 - x - 1.0, ch - 1.0), &c.pinceau, 1.0, None);
                } else {
                    self.couleur(c, &CURSEUR);
                    match forme {
                        3 | 4 => rt.FillRectangle(&rect(x, y + ch - 2.0, x2 - x, 2.0), &c.pinceau),
                        5 | 6 => rt.FillRectangle(&rect(x, y, 2.0, ch), &c.pinceau),
                        _ => {
                            rt.FillRectangle(&rect(x, y, x2 - x, ch), &c.pinceau);
                            // Le caractère sous le bloc reste lisible : redessiné en noir
                            let sous = Style { fg: FOND_TERMINAL, bg: None, souligne: false, barre: false, cache: false };
                            if let Some(l) = &v.ligne {
                                if let Some(ch) = l.sous_curseur().filter(|ch| *ch != ' ') {
                                    self.glyphes_terminal(c, &sous, &ch.to_string(), x, y, x2 - x);
                                }
                            } else if let Some(cel) = p.ligne_visible(rangee).and_then(|l| l.get(col)) {
                                if cel.c != ' ' && cel.largeur != 0 {
                                    self.glyphes_terminal(c, &sous, &cel.c.to_string(), x, y, x2 - x);
                                }
                            }
                        }
                    }
                }
            }
        }
        // 5. T7 : le menu de complétion ouvert par Tab, sous la rangée du curseur (au-dessus faute de place), par-dessus la grille
        if let (Some(l), Some(rangee)) = (&v.ligne, p.rangee_du_curseur()) {
            if let Some(menu) = &l.menu {
                self.dessiner_menu_completion(c, menu, l.col, rangee, rangees, vt.colonnes(), &x_de, &y_de);
            }
        }
        rt.PopAxisAlignedClip();
    }

    /// Le menu de complétion : un cadre, une rangée par candidat (nom, puis description en gris), la rangée choisie en
    /// surbrillance. Posé sous la rangée du curseur s'il y a la place, sinon au-dessus ; rien si aucun des deux ne convient.
    ///
    /// # Safety
    /// Mêmes conditions que `dessiner_grille_terminal` (appelée seulement d'elle).
    #[allow(clippy::too_many_arguments)]
    unsafe fn dessiner_menu_completion(
        &self,
        c: &Cible2D,
        menu: &crate::terminal::panneau::ligne::VueMenu,
        col: usize,
        rangee: usize,
        rangees: usize,
        colonnes: usize,
        x_de: &dyn Fn(usize) -> f32,
        y_de: &dyn Fn(usize) -> f32,
    ) {
        use crate::terminal::completion::Genre;
        let n = menu.lignes.len();
        let place = colonnes.saturating_sub(col);
        if n == 0 || place < 12 {
            return;
        }
        let premiere = if rangee + 1 + n <= rangees {
            rangee + 1
        } else if rangee >= n {
            rangee - n
        } else {
            return;
        };
        let nom_de = |l: &crate::terminal::panneau::ligne::LigneMenu| -> String {
            if l.genre == Genre::Dossier { format!("{}\\", l.nom) } else { l.nom.clone() }
        };
        let largeur_nom = menu.lignes.iter().map(|l| nom_de(l).chars().count()).max().unwrap_or(0).min(40);
        let largeur_desc = menu.lignes.iter().map(|l| l.description.chars().count()).max().unwrap_or(0);
        let interieur = largeur_nom + if largeur_desc > 0 { 2 + largeur_desc } else { 0 };
        // Une case de marge de chaque côté
        let largeur = (interieur + 2).min(place).min(60);
        let (rt, ch) = (&c.rt, self.cellule_terminal.1);
        let (x0, y0) = (x_de(col), y_de(premiere));
        let (w_px, h_px) = (x_de(col + largeur) - x0, ch * n as f32);
        self.couleur(c, &FOND_TERMINAL);
        rt.FillRectangle(&rect(x0, y0, w_px, h_px), &c.pinceau);
        let fg = if crate::theme::clair() { FG_DEFAUT_CLAIR } else { FG_DEFAUT };
        let normal = Style { fg, bg: None, souligne: false, barre: false, cache: false };
        let gris = Style { fg: SUGGESTION, ..normal };
        let tronquer = |t: &str, max: usize| -> String {
            if t.chars().count() <= max {
                t.to_string()
            } else {
                t.chars().take(max.saturating_sub(1)).chain(std::iter::once('…')).collect()
            }
        };
        for (k, l) in menu.lignes.iter().enumerate() {
            let y = y_de(premiere + k);
            if menu.selection == Some(k) {
                self.couleur(c, &SELECTION);
                rt.FillRectangle(&rect(x0, y, w_px, ch), &c.pinceau);
            }
            let dispo = largeur.saturating_sub(2);
            let nom = tronquer(&nom_de(l), largeur_nom.min(dispo));
            self.glyphes_terminal(c, &normal, &nom, x_de(col + 1), y, x_de(col + 1 + nom.chars().count()) - x_de(col + 1));
            let reste = dispo.saturating_sub(largeur_nom + 2);
            if reste > 3 && !l.description.is_empty() {
                let desc = tronquer(l.description, reste);
                let cd = col + 1 + largeur_nom + 2;
                self.glyphes_terminal(c, &gris, &desc, x_de(cd), y, x_de(cd + desc.chars().count()) - x_de(cd));
            }
        }
        // Un trait fin autour, pour détacher le menu de la grille
        self.couleur(c, &SUGGESTION);
        rt.DrawRectangle(&rect(x0 + 0.5, y0 + 0.5, w_px - 1.0, h_px - 1.0), &c.pinceau, 1.0, None);
    }

    /// Une plage de texte et ses décorations (souligné, barré) ; une plage « cachée » ne dessine rien.
    ///
    /// # Safety
    /// Mêmes conditions que `dessiner_grille_terminal` (appelée seulement d'elle).
    unsafe fn glyphes_terminal(&self, c: &Cible2D, st: &Style, texte: &str, x: f32, y: f32, largeur: f32) {
        let (_, ch) = self.cellule_terminal;
        if st.cache || largeur <= 0.0 {
            return;
        }
        self.couleur(c, &st.fg);
        if !texte.chars().all(|t| t == ' ') {
            let utf16: Vec<u16> = texte.encode_utf16().collect();
            c.rt.DrawText(&utf16, &self.f.terminal, &rect(x, y, largeur + self.cellule_terminal.0, ch), &c.pinceau, D2D1_DRAW_TEXT_OPTIONS_NONE, DWRITE_MEASURING_MODE_NATURAL);
        }
        if st.souligne {
            c.rt.FillRectangle(&rect(x, y + ch - 2.0, largeur, 1.0), &c.pinceau);
        }
        if st.barre {
            c.rt.FillRectangle(&rect(x, y + (ch / 2.0).floor(), largeur, 1.0), &c.pinceau);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Les tests sombres ne dépendent pas du thème global (un autre test pourrait le changer)
    fn style_de(a: &Attr) -> Style {
        style_pour(a, false)
    }

    fn rvb(c: &D2D1_COLOR_F) -> (u8, u8, u8) {
        ((c.r * 255.0).round() as u8, (c.g * 255.0).round() as u8, (c.b * 255.0).round() as u8)
    }

    #[test]
    fn la_palette_256_a_ses_trois_zones() {
        assert_eq!(rvb_indexe(1), (0xE5, 0x53, 0x4B));
        assert_eq!(rvb_indexe(15), (255, 255, 255));
        // Cube : 16 = noir, 21 = bleu pur (niveau 5 sur la dernière composante), 231 = blanc
        assert_eq!(rvb_indexe(16), (0, 0, 0));
        assert_eq!(rvb_indexe(21), (0, 0, 255));
        assert_eq!(rvb_indexe(196), (255, 0, 0));
        assert_eq!(rvb_indexe(231), (255, 255, 255));
        // Gris : 232 = 8, 255 = 238
        assert_eq!(rvb_indexe(232), (8, 8, 8));
        assert_eq!(rvb_indexe(255), (238, 238, 238));
    }

    #[test]
    fn les_attributs_par_defaut_donnent_le_texte_clair_sans_fond() {
        let s = style_de(&Attr::default());
        assert_eq!(rvb(&s.fg), (0xE6, 0xE6, 0xE6));
        assert!(s.bg.is_none() && !s.souligne && !s.barre && !s.cache);
    }

    #[test]
    fn le_gras_eclaircit_les_huit_premieres_couleurs_seulement() {
        let attr = |i| Attr { fg: Couleur::Indexee(i), bg: Couleur::Defaut, drapeaux: Attr::GRAS };
        assert_eq!(rvb(&style_de(&attr(1)).fg), rvb_indexe(9), "rouge gras = rouge vif");
        assert_eq!(rvb(&style_de(&attr(9)).fg), rvb_indexe(9), "déjà vif : inchangé");
        assert_eq!(rvb(&style_de(&attr(100)).fg), rvb_indexe(100), "au-delà de 8 : inchangé");
        let defaut = Attr { drapeaux: Attr::GRAS, ..Attr::default() };
        assert_eq!(rvb(&style_de(&defaut).fg), (255, 255, 255), "texte par défaut en gras : blanc");
    }

    #[test]
    fn l_inverse_echange_les_couleurs_meme_par_defaut() {
        let s = style_de(&Attr { drapeaux: Attr::INVERSE, ..Attr::default() });
        assert_eq!(rvb(&s.bg.expect("fond")), (0xE6, 0xE6, 0xE6), "le fond prend le texte par défaut");
        assert_eq!(rvb(&s.fg), (0, 0, 0), "le texte prend le noir du panneau");
        let s = style_de(&Attr { fg: Couleur::Rvb(10, 20, 30), bg: Couleur::Rvb(200, 100, 50), drapeaux: Attr::INVERSE });
        assert_eq!((rvb(&s.fg), rvb(&s.bg.unwrap())), ((200, 100, 50), (10, 20, 30)));
    }

    /// Rapport de contraste WCAG entre deux couleurs opaques
    fn contraste_wcag(a: (u8, u8, u8), b: (u8, u8, u8)) -> f32 {
        let lum = |(r, g, b): (u8, u8, u8)| {
            let l = |v: u8| {
                let v = v as f32 / 255.0;
                if v <= 0.03928 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) }
            };
            0.2126 * l(r) + 0.7152 * l(g) + 0.0722 * l(b)
        };
        let (x, y) = (lum(a), lum(b));
        (x.max(y) + 0.05) / (x.min(y) + 0.05)
    }

    /// Thème clair : le texte par défaut est foncé et lisible sur le fond clair (le défaut vu par l'auteur : E6E6E6 sur #F7F7F8)
    #[test]
    fn le_theme_clair_donne_un_texte_par_defaut_fonce_et_lisible() {
        let fond_clair = (247, 247, 248);
        let s = style_pour(&Attr::default(), true);
        assert!(contraste_wcag(rvb(&s.fg), fond_clair) >= 7.0, "texte par défaut : {:?}", rvb(&s.fg));
        let gras = style_pour(&Attr { drapeaux: Attr::GRAS, ..Attr::default() }, true);
        assert!(contraste_wcag(rvb(&gras.fg), fond_clair) >= 7.0, "gras par défaut : {:?}", rvb(&gras.fg));
        // En sombre, rien ne change
        assert_eq!(rvb(&style_pour(&Attr::default(), false).fg), (0xE6, 0xE6, 0xE6));
    }

    /// Thème clair : les couleurs ANSI 1 à 14 restent lisibles (4,5:1 au moins) sur le fond clair, gras compris
    #[test]
    fn les_couleurs_ansi_du_theme_clair_sont_lisibles() {
        let fond_clair = (247, 247, 248);
        for i in 0..15u8 {
            let c = rvb_du_theme(i, true);
            let r = contraste_wcag(c, fond_clair);
            // Le « blanc » ANSI 7 est un gris atténué : 3:1 suffit (texte secondaire, souvent un fond de cellule)
            let seuil = if i == 7 { 3.0 } else { 4.5 };
            assert!(r >= seuil, "ANSI {i} = {c:?} : {r:.2}:1 sur le fond clair");
        }
        // Aucune teinte du thème clair ne tombe dans la règle « presque noir » de vers_clair (elle en ferait le fond clair)
        for (i, c) in ANSI_CLAIR.iter().enumerate().take(15) {
            assert!(!(c.0 < 26 && c.1 < 26 && c.2 < 26), "ANSI clair {i} serait pris pour un fond noir");
        }
        assert!(FG_DEFAUT_CLAIR.r > 0.1 && FG_GRAS_CLAIR.r > 0.1);
        // Au-delà de 16, le cube et les gris sont communs aux deux thèmes
        assert_eq!(rvb_du_theme(196, true), rvb_indexe(196));
    }

    #[test]
    fn pale_baisse_l_opacite_et_les_decorations_passent() {
        let s = style_de(&Attr { drapeaux: Attr::PALE | Attr::SOULIGNE | Attr::BARRE | Attr::CACHE, ..Attr::default() });
        assert!((s.fg.a - 0.6).abs() < 1e-6);
        assert!(s.souligne && s.barre && s.cache);
    }

    /// La cellule mesurée avec la vraie police est plausible : plus haute que large, sans être démesurée
    #[test]
    fn la_cellule_mesuree_est_plausible() {
        use windows::Win32::Graphics::DirectWrite::{DWriteCreateFactory, DWRITE_FACTORY_TYPE_SHARED};
        let dw: IDWriteFactory = unsafe { DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED) }.expect("DirectWrite");
        let f = format_terminal(&dw).expect("format");
        let (cw, ch) = mesurer_cellule(&dw, &f);
        assert!((5.0..14.0).contains(&cw), "largeur de cellule {cw}");
        assert!((10.0..28.0).contains(&ch) && ch > cw, "hauteur de cellule {ch}");
    }
}
