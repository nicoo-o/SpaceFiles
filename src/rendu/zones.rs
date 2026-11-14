// Géométrie pure de la fenêtre : grille, onglets, icônes de la barre haute, bouton et panneau du terminal. Sortie de rendu.rs
// (audit du 14/11/2026, découpage) : aucune dépendance à Direct2D hors le type de rectangle, donc testable sans fenêtre.

use super::*;

/// Place réservée à Pixel dans la barre d'état avant le texte : sa boîte (26 DIP) et l'espace
pub const PIXEL_DECALAGE: f32 = 36.0;

/// Bord gauche de la grille (coordonnées du panneau)
pub fn grille_x0() -> f32 {
    largeur_volet() + MARGE_LISTE + 6.0
}

/// Vignette de la grille sous ce point (coordonnées du panneau)
pub fn case_grille(app: &App, x: f32, y: f32) -> Option<usize> {
    let (par, hr) = app.pas();
    let pas_x = app.cote_grille + GRILLE_ECART;
    if x < grille_x0() || y < liste_y() {
        return None;
    }
    let col = ((x - grille_x0()) / pas_x) as usize;
    let dans_x = (x - grille_x0()) % pas_x;
    let rang = ((y - liste_y() + app.defilement) / hr) as usize;
    let dans_y = (y - liste_y() + app.defilement) % hr;
    if col >= par || dans_x > app.cote_grille || dans_y > app.cote_grille + GRILLE_NOM {
        return None;
    }
    let i = rang * par + col;
    (i < app.entrees.len()).then_some(i)
}

/// Largeur d'un onglet quand même le plancher ne tient plus : le titre y est tronqué, la croix reste visible
const ONGLET_MIN_ABSOLU: f32 = 44.0;

/// Onglets puis bouton « + », dans la barre de titre, juste après le nom
pub fn zones_onglets(l: f32, n: usize) -> Vec<(Cible, D2D_RECT_F)> {
    let fin = zone_recherche_haut(l).left - 12.0;
    let n = n.max(1);
    let dispo = (fin - ONGLETS_X - BOUTON_HAUT - 8.0).max(0.0);
    let ecart = ONGLET_ECART * n.saturating_sub(1) as f32;
    // Le plancher de 130 DIP cède quand la place manque (jusqu'à 44) : sinon 8 onglets dans 1 100 DIP passaient sous
    // l'icône de recherche (trouvé par le test des zones, audit du 14/11/2026)
    let brute = (dispo - ecart) / n as f32;
    let largeur = if brute >= ONGLET_MIN { brute.min(ONGLET_MAX) } else { brute.max(ONGLET_MIN_ABSOLU) };
    let y = (HAUTEUR_TITRE - 34.0) / 2.0;
    let mut v: Vec<(Cible, D2D_RECT_F)> = (0..n).map(|k| (Cible::Onglet(k), rect(ONGLETS_X + k as f32 * (largeur + ONGLET_ECART), y, largeur, 34.0))).collect();
    let x = ONGLETS_X + n as f32 * (largeur + ONGLET_ECART);
    if x + BOUTON_HAUT < fin {
        v.push((Cible::NouvelOnglet, rect(x, y, BOUTON_HAUT, BOUTON_HAUT)));
    }
    v
}

/// Trois icônes de la barre haute, avant les contrôles système
pub fn x_icones_haut(l: f32) -> f32 {
    l - 3.0 * BOUTON_LARGEUR - MARGE_HAUT - 3.0 * BOUTON_HAUT - 2.0 * ECART_HAUT
}

pub fn zone_icone_haut(l: f32, rang: usize) -> D2D_RECT_F {
    rect(x_icones_haut(l) + rang as f32 * (BOUTON_HAUT + ECART_HAUT), (HAUTEUR_TITRE - BOUTON_HAUT) / 2.0, BOUTON_HAUT, BOUTON_HAUT)
}

pub fn zone_recherche_haut(l: f32) -> D2D_RECT_F {
    zone_icone_haut(l, 0)
}

pub fn zone_essentiel_haut(l: f32) -> D2D_RECT_F {
    zone_icone_haut(l, 1)
}

pub fn zone_reglages_haut(l: f32) -> D2D_RECT_F {
    zone_icone_haut(l, 2)
}

/// Bouton « >_ » à droite du chemin, avant les autres éléments de droite
pub const TERMINAL_LARGEUR: f32 = 32.0;
pub(super) const TERMINAL_RESERVE: f32 = 40.0;

pub fn zone_terminal(l: f32) -> D2D_RECT_F {
    rect(l - MARGE_LISTE - TERMINAL_LARGEUR, nav_y() + 3.0, TERMINAL_LARGEUR, BOUTON_HAUT)
}

/// Panneau terminal : 196 DIP au-dessus du pied, sur toute la largeur de la liste (l'aperçu reste à droite).
pub const TERMINAL_PANNEAU_H: f32 = 196.0;

pub fn zone_terminal_panneau(l: f32, h: f32) -> D2D_RECT_F {
    let bas = h - statut_h();
    let haut = (bas - TERMINAL_PANNEAU_H).max(HAUTEUR_TITRE + 1.0);
    rect(largeur_volet() + 1.0, haut, (l - largeur_volet() - 1.0).max(0.0), (bas - haut).max(0.0))
}

pub fn zone_terminal_fermer(l: f32, h: f32) -> D2D_RECT_F {
    let p = zone_terminal_panneau(l, h);
    rect(p.right - 34.0, p.top + 8.0, 24.0, 24.0)
}

pub fn zone_terminal_action(l: f32, h: f32) -> D2D_RECT_F {
    let p = zone_terminal_panneau(l, h);
    rect((p.right - 20.0 - 236.0).max(p.left + 20.0), p.bottom - 40.0, 236.0, 28.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn largeur(r: &D2D_RECT_F) -> f32 {
        r.right - r.left
    }

    #[test]
    fn les_trois_icones_haut_sont_cote_a_cote_sans_chevaucher() {
        let l = 1200.0;
        let (a, b, c) = (zone_recherche_haut(l), zone_essentiel_haut(l), zone_reglages_haut(l));
        assert!(a.right < b.left && b.right < c.left, "icônes qui se chevauchent");
        assert_eq!(largeur(&a), BOUTON_HAUT);
        // Les icônes restent avant les contrôles système de la fenêtre
        assert!(c.right <= l - 3.0 * BOUTON_LARGEUR);
    }

    #[test]
    fn les_onglets_ne_depassent_jamais_la_recherche() {
        for n in [1usize, 3, 8, 12] {
            let l = 1100.0;
            let fin = zone_recherche_haut(l).left;
            for (_, z) in zones_onglets(l, n) {
                assert!(z.right <= fin, "{n} onglets : un cadre dépasse la recherche");
            }
        }
    }

    #[test]
    fn zones_onglets_garde_au_moins_un_onglet_pour_zero() {
        let v = zones_onglets(1000.0, 0);
        assert!(matches!(v.first(), Some((Cible::Onglet(0), _))));
    }

    #[test]
    fn le_bouton_terminal_reste_dans_la_fenetre_et_hors_du_champ() {
        let l = 900.0;
        let z = zone_terminal(l);
        assert!(z.right <= l - MARGE_LISTE + 0.01);
        assert_eq!(largeur(&z), TERMINAL_LARGEUR);
        // Un champ réduit de la réserve ne rejoint pas le bouton (correctif du texte « ECHAP » superposé)
        assert!(l - MARGE_LISTE - TERMINAL_RESERVE <= z.left);
    }

    #[test]
    fn le_panneau_terminal_tient_au_dessus_du_pied() {
        let (l, h) = (1000.0, 700.0);
        let p = zone_terminal_panneau(l, h);
        assert!(p.bottom <= h - statut_h() + 0.01);
        assert!(p.top >= HAUTEUR_TITRE);
        for z in [zone_terminal_fermer(l, h), zone_terminal_action(l, h)] {
            assert!(z.left >= p.left && z.right <= p.right + 0.01 && z.top >= p.top && z.bottom <= p.bottom + 0.01);
        }
    }

    #[test]
    fn le_panneau_terminal_ne_devient_pas_negatif_dans_une_petite_fenetre() {
        let p = zone_terminal_panneau(300.0, 80.0);
        assert!(p.right >= p.left && p.bottom >= p.top);
    }

    #[test]
    fn le_bord_gauche_de_la_grille_suit_le_volet() {
        assert_eq!(grille_x0(), largeur_volet() + MARGE_LISTE + 6.0);
    }
}
