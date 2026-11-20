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
    let fin = zone_essentiel_haut(l).left - 12.0;
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
        // Lot 6 (D-6) : le « + » est à `ONGLET_ECART` du dernier onglet, comme deux onglets entre eux, et centré dans la barre
        // comme les icônes de droite (il était calé sur le haut de l'onglet, 1 DIP plus haut). Cible : 32 × 32, au-delà de 24 × 24.
        v.push((Cible::NouvelOnglet, rect(x, (HAUTEUR_TITRE - BOUTON_HAUT) / 2.0, BOUTON_HAUT, BOUTON_HAUT)));
    }
    v
}

/// Deux icônes de la barre haute (Essentiel, Réglages), avant les contrôles système. La loupe en a été retirée à la demande de
/// l'auteur (17/11/2026) : taper dans le champ suffit pour chercher ; la recherche dans tout le PC reste sur Ctrl+Maj+F, la
/// palette et le menu.
pub fn x_icones_haut(l: f32) -> f32 {
    l - 3.0 * BOUTON_LARGEUR - MARGE_HAUT - 2.0 * BOUTON_HAUT - ECART_HAUT
}

pub fn zone_icone_haut(l: f32, rang: usize) -> D2D_RECT_F {
    rect(x_icones_haut(l) + rang as f32 * (BOUTON_HAUT + ECART_HAUT), (HAUTEUR_TITRE - BOUTON_HAUT) / 2.0, BOUTON_HAUT, BOUTON_HAUT)
}

pub fn zone_essentiel_haut(l: f32) -> D2D_RECT_F {
    zone_icone_haut(l, 0)
}

pub fn zone_reglages_haut(l: f32) -> D2D_RECT_F {
    zone_icone_haut(l, 1)
}

/// Bouton « >_ » à droite du chemin, avant les autres éléments de droite
pub const TERMINAL_LARGEUR: f32 = 32.0;
pub(super) const TERMINAL_RESERVE: f32 = 40.0;

/// Lot 6 (D-6) : même gabarit que les boutons Précédent / Suivant / Parent de la même rangée (`boutons_nav` : 4 DIP sous le haut
/// de la rangée, hauteur `nav_h() - 8`), au lieu de 32 DIP posés 3 DIP sous le haut.
pub fn zone_terminal(l: f32) -> D2D_RECT_F {
    rect(l - MARGE_LISTE - TERMINAL_LARGEUR, nav_y() + 4.0, TERMINAL_LARGEUR, nav_h() - 8.0)
}

/// Panneau terminal : plancher de 196 DIP, mais 42 % de la fenêtre (au plus 460) pour qu'une session ait de la place ;
/// au-dessus du pied, sur toute la largeur de la liste (l'aperçu reste à droite).
pub const TERMINAL_PANNEAU_H: f32 = 196.0;
const TERMINAL_PANNEAU_H_MAX: f32 = 460.0;
/// Hauteur de l'en-tête (titre, bouton externe, croix) ; la grille commence dessous.
pub const TERMINAL_ENTETE_H: f32 = 40.0;
const TERMINAL_MARGE: f32 = 12.0;

pub fn terminal_panneau_hauteur(h: f32) -> f32 {
    (h * 0.42).clamp(TERMINAL_PANNEAU_H, TERMINAL_PANNEAU_H_MAX)
}

pub fn zone_terminal_panneau(l: f32, h: f32) -> D2D_RECT_F {
    let bas = h - statut_h();
    let haut = (bas - terminal_panneau_hauteur(h)).max(HAUTEUR_TITRE + 1.0);
    rect(largeur_volet() + 1.0, haut, (l - largeur_volet() - 1.0).max(0.0), (bas - haut).max(0.0))
}

pub fn zone_terminal_fermer(l: f32, h: f32) -> D2D_RECT_F {
    let p = zone_terminal_panneau(l, h);
    rect(p.right - 34.0, p.top + 8.0, 24.0, 24.0)
}

/// Bouton « OUVRIR DANS WINDOWS TERMINAL » : dans l'en-tête, à gauche de la croix (plus de pied de panneau).
pub fn zone_terminal_action(l: f32, h: f32) -> D2D_RECT_F {
    let p = zone_terminal_panneau(l, h);
    let f = zone_terminal_fermer(l, h);
    rect((f.left - 8.0 - 236.0).max(p.left + 120.0), p.top + 6.0, 236.0, 28.0)
}

/// Onglets du panneau (T4a) : à droite de l'icône `>_`, jusqu'au bouton « Windows Terminal », puis le bouton « + ».
pub const TERMINAL_ONGLET_MIN: f32 = 92.0;
pub const TERMINAL_ONGLET_MAX: f32 = 150.0;
const TERMINAL_ONGLET_ECART: f32 = 4.0;
const TERMINAL_PLUS: f32 = 24.0;

/// Onglet « JOURNAL » (T4c) : le journal des opérations, à droite de la bande des shells, avant le bouton « Windows Terminal »
const TERMINAL_JOURNAL_LARGEUR: f32 = 88.0;
/// Sous cette largeur, l'onglet JOURNAL n'a plus la place de son libellé : il disparaît (largeur 0) plutôt que d'écraser les
/// onglets des shells dans une fenêtre très étroite
const TERMINAL_JOURNAL_MIN: f32 = 56.0;

/// Largeur de l'onglet JOURNAL : 88 DIP, moins ce que la bande réclame pour un onglet shell minimal et le « + » ; 0 s'il ne
/// reste pas `TERMINAL_JOURNAL_MIN`
pub fn terminal_journal_largeur(l: f32, h: f32) -> f32 {
    let p = zone_terminal_panneau(l, h);
    let total = zone_terminal_action(l, h).left - 8.0 - (p.left + 50.0);
    let w = (total - (TERMINAL_ONGLET_MIN + TERMINAL_ONGLET_ECART + TERMINAL_PLUS + 8.0)).min(TERMINAL_JOURNAL_LARGEUR);
    if w >= TERMINAL_JOURNAL_MIN { w } else { 0.0 }
}

/// Zone de l'onglet JOURNAL (largeur 0 quand la fenêtre est trop étroite : ni dessiné ni cliquable)
pub fn zone_terminal_journal(l: f32, h: f32) -> D2D_RECT_F {
    let p = zone_terminal_panneau(l, h);
    let action = zone_terminal_action(l, h);
    let w = terminal_journal_largeur(l, h);
    rect(action.left - 8.0 - w, p.top + 8.0, w, 24.0)
}

/// Début et fin (x) de la bande où tiennent les onglets et le bouton « + » : elle s'arrête avant l'onglet JOURNAL, ou avant le
/// bouton « Windows Terminal » quand le journal n'a pas la place
fn terminal_bande_onglets(l: f32, h: f32) -> (f32, f32) {
    let p = zone_terminal_panneau(l, h);
    let fin = if terminal_journal_largeur(l, h) > 0.0 { zone_terminal_journal(l, h).left } else { zone_terminal_action(l, h).left };
    (p.left + 50.0, (fin - 8.0).max(p.left + 50.0))
}

/// Largeur de chacun des `n` onglets : ce que la bande laisse, sans dépasser `TERMINAL_ONGLET_MAX` (peut passer sous le
/// minimum si la fenêtre est très étroite : `terminal_peut_ajouter` refuse alors un onglet de plus)
pub fn terminal_onglet_largeur(l: f32, h: f32, n: usize) -> f32 {
    let n = n.max(1) as f32;
    let (debut, fin) = terminal_bande_onglets(l, h);
    let libre = fin - debut - TERMINAL_PLUS - TERMINAL_ONGLET_ECART * n;
    (libre / n).clamp(0.0, TERMINAL_ONGLET_MAX)
}

/// Un onglet de plus tient-il dans l'en-tête, avec sa largeur minimale, sans dépasser le maximum d'onglets ?
pub fn terminal_peut_ajouter(l: f32, h: f32, n: usize) -> bool {
    n < crate::terminal::onglets::ONGLETS_MAX && terminal_onglet_largeur(l, h, n + 1) >= TERMINAL_ONGLET_MIN
}

/// Zone de l'onglet de rang `i` quand il y en a `n`
pub fn zone_terminal_onglet(l: f32, h: f32, i: usize, n: usize) -> D2D_RECT_F {
    let p = zone_terminal_panneau(l, h);
    let (debut, _) = terminal_bande_onglets(l, h);
    let w = terminal_onglet_largeur(l, h, n);
    rect(debut + i as f32 * (w + TERMINAL_ONGLET_ECART), p.top + 8.0, w, 24.0)
}

/// Côté de la croix de fermeture d'un onglet, et marge qui la sépare du bord droit de l'onglet
const TERMINAL_CROIX: f32 = 18.0;
const TERMINAL_CROIX_MARGE: f32 = 3.0;

/// Croix de fermeture de l'onglet de rang `i` : un carré dans l'onglet, contre son bord droit (19/11/2026, demande de l'auteur :
/// fermer un onglet sans passer par le clic du milieu, que rien n'annonce)
pub fn zone_terminal_onglet_croix(l: f32, h: f32, i: usize, n: usize) -> D2D_RECT_F {
    let z = zone_terminal_onglet(l, h, i, n);
    let c = TERMINAL_CROIX.min((z.right - z.left).max(0.0));
    rect(z.right - TERMINAL_CROIX_MARGE - c, z.top + (z.bottom - z.top - c) / 2.0, c, c)
}

/// Largeur que le libellé d'un onglet peut occuper : l'onglet moins ses marges et la croix
pub fn terminal_onglet_place_libelle(z: &D2D_RECT_F) -> f32 {
    ((z.right - z.left) - 10.0 - TERMINAL_CROIX - TERMINAL_CROIX_MARGE - 4.0).max(0.0)
}

/// Bouton « + » (nouvel onglet), juste après le dernier des `n` onglets
pub fn zone_terminal_plus(l: f32, h: f32, n: usize) -> D2D_RECT_F {
    let p = zone_terminal_panneau(l, h);
    let dernier = zone_terminal_onglet(l, h, n.saturating_sub(1), n.max(1));
    rect(dernier.right + TERMINAL_ONGLET_ECART, p.top + 8.0, TERMINAL_PLUS, 24.0)
}

/// Zone où la grille du terminal est dessinée : sous l'en-tête, avec une marge de chaque côté.
pub fn zone_terminal_grille(l: f32, h: f32) -> D2D_RECT_F {
    let p = zone_terminal_panneau(l, h);
    rect(p.left + TERMINAL_MARGE, p.top + TERMINAL_ENTETE_H + 4.0, (p.right - p.left - 2.0 * TERMINAL_MARGE).max(0.0), (p.bottom - p.top - TERMINAL_ENTETE_H - 4.0 - 8.0).max(0.0))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn largeur(r: &D2D_RECT_F) -> f32 {
        r.right - r.left
    }

    #[test]
    fn les_deux_icones_haut_sont_cote_a_cote_sans_chevaucher() {
        let l = 1200.0;
        let (a, b) = (zone_essentiel_haut(l), zone_reglages_haut(l));
        assert!(a.right < b.left, "icônes qui se chevauchent");
        assert_eq!(largeur(&a), BOUTON_HAUT);
        // Les icônes restent avant les contrôles système de la fenêtre
        assert!(b.right <= l - 3.0 * BOUTON_LARGEUR);
    }

    /// Lot 6 (D-6) : d'Essentiel à « fermer », cinq centres au même pas, multiple de 4 (la loupe a été retirée le 17/11/2026)
    #[test]
    fn les_icones_du_haut_et_les_controles_systeme_sont_au_meme_pas() {
        for l in [900.0f32, 1200.0, 1920.0] {
            let mut centres: Vec<f32> = (0..2).map(|k| (zone_icone_haut(l, k).left + zone_icone_haut(l, k).right) / 2.0).collect();
            // Même formule que le dessin des contrôles (réduire, agrandir, fermer : rangs 3, 2, 1)
            centres.extend([3, 2, 1].map(|rang| l - BOUTON_LARGEUR * rang as f32 + BOUTON_LARGEUR / 2.0));
            let pas: Vec<f32> = centres.windows(2).map(|w| w[1] - w[0]).collect();
            assert!(pas.iter().all(|p| (p - pas[0]).abs() < 0.01), "pas inégaux à l = {l} : {pas:?}");
            assert_eq!(pas[0] % 4.0, 0.0, "le pas {} n'est pas un multiple de 4", pas[0]);
        }
    }

    /// Lot 6 (D-6) : le « + » est à l'écart d'un onglet du dernier onglet, centré dans la barre, et sa cible dépasse 24 × 24
    #[test]
    fn le_plus_des_onglets_est_colle_a_l_onglet_et_assez_grand() {
        for n in [1usize, 2, 4] {
            let v = zones_onglets(1400.0, n);
            let dernier = v.iter().filter(|(q, _)| matches!(q, Cible::Onglet(_))).next_back().unwrap().1;
            let plus = v.iter().find(|(q, _)| *q == Cible::NouvelOnglet).expect("« + » absent").1;
            assert!((plus.left - dernier.right - ONGLET_ECART).abs() < 0.01, "{n} onglets : écart {} au lieu de {ONGLET_ECART}", plus.left - dernier.right);
            assert!(largeur(&plus) >= 24.0 && plus.bottom - plus.top >= 24.0, "cible {} × {}", largeur(&plus), plus.bottom - plus.top);
            assert!(((plus.top + plus.bottom) / 2.0 - HAUTEUR_TITRE / 2.0).abs() < 0.01, "« + » pas centré dans la barre de titre");
        }
    }

    /// Lot 6 (D-6) : le bouton terminal a le gabarit des boutons Précédent / Suivant / Parent de la même rangée
    #[test]
    fn le_bouton_terminal_a_le_gabarit_des_boutons_de_navigation() {
        let z = zone_terminal(1200.0);
        let (_, nav) = boutons_nav()[0];
        assert_eq!((z.top, z.bottom), (nav.top, nav.bottom));
        assert_eq!(largeur(&z), NAV_BOUTON);
        assert!(z.bottom - z.top >= 24.0);
    }

    #[test]
    fn les_onglets_ne_depassent_jamais_la_recherche() {
        for n in [1usize, 3, 8, 12] {
            let l = 1100.0;
            let fin = zone_essentiel_haut(l).left;
            for (_, z) in zones_onglets(l, n) {
                assert!(z.right <= fin, "{n} onglets : un cadre dépasse les icônes du haut");
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
        for z in [zone_terminal_fermer(l, h), zone_terminal_action(l, h), zone_terminal_grille(l, h)] {
            assert!(z.left >= p.left && z.right <= p.right + 0.01 && z.top >= p.top && z.bottom <= p.bottom + 0.01);
        }
    }

    #[test]
    fn le_panneau_terminal_grandit_avec_la_fenetre_dans_des_bornes() {
        let hauteur = |h: f32| { let p = zone_terminal_panneau(1200.0, h); p.bottom - p.top };
        // Petite fenêtre : le plancher (42 % de 400 = 168 < 196)
        assert_eq!(hauteur(400.0), TERMINAL_PANNEAU_H);
        assert!((hauteur(900.0) - 378.0).abs() < 0.01, "42 % de 900 attendu, obtenu {}", hauteur(900.0));
        assert_eq!(hauteur(2000.0), 460.0);
    }

    /// Le bouton externe et la croix sont dans l'en-tête, sans se recouvrir ni toucher la grille
    #[test]
    fn l_entete_du_terminal_est_libre_au_dessus_de_la_grille() {
        for (l, h) in [(1000.0f32, 700.0f32), (700.0, 400.0)] {
            let (a, f, g) = (zone_terminal_action(l, h), zone_terminal_fermer(l, h), zone_terminal_grille(l, h));
            assert!(a.right <= f.left + 0.01, "le bouton externe chevauche la croix à {l} × {h}");
            assert!(a.bottom <= g.top && f.bottom <= g.top, "l'en-tête mord sur la grille à {l} × {h}");
            assert!(g.bottom > g.top && g.right > g.left);
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

    #[test]
    fn les_onglets_du_terminal_tiennent_dans_l_en_tete_sans_se_chevaucher() {
        for (l, h) in [(1200.0, 800.0), (900.0, 600.0), (700.0, 500.0)] {
            let action = zone_terminal_action(l, h);
            for n in 1..=crate::terminal::onglets::ONGLETS_MAX {
                if n > 1 && !terminal_peut_ajouter(l, h, n - 1) {
                    break; // la fenêtre n'en accepte pas davantage : l'interface refuse l'ouverture
                }
                let zones: Vec<_> = (0..n).map(|i| zone_terminal_onglet(l, h, i, n)).collect();
                let plus = zone_terminal_plus(l, h, n);
                for (i, z) in zones.iter().enumerate() {
                    // Le premier onglet s'ouvre toujours, même dans une fenêtre si étroite qu'il passe sous le minimum ; dès
                    // le deuxième, `terminal_peut_ajouter` a garanti le minimum
                    let plancher = if n == 1 { 0.0 } else { TERMINAL_ONGLET_MIN - 0.01 };
                    assert!(z.right - z.left >= plancher && z.right - z.left <= TERMINAL_ONGLET_MAX + 0.01, "largeur {} (n={n}, fenêtre {l})", z.right - z.left);
                    if i > 0 {
                        assert!(zones[i - 1].right <= z.left, "onglets {} et {i} se chevauchent (n={n}, fenêtre {l})", i - 1);
                    }
                }
                assert!(zones[n - 1].right <= plus.left, "le « + » chevauche le dernier onglet (n={n}, fenêtre {l})");
                let journal = zone_terminal_journal(l, h);
                if journal.right - journal.left > 0.0 {
                    assert!(plus.right <= journal.left, "le « + » déborde sur l'onglet JOURNAL (n={n}, fenêtre {l}) : {} > {}", plus.right, journal.left);
                    assert!(journal.right <= action.left, "l'onglet JOURNAL déborde sur le bouton Windows Terminal (fenêtre {l})");
                    assert!(journal.right - journal.left >= 56.0, "onglet JOURNAL trop étroit (fenêtre {l})");
                } else {
                    // fenêtre trop étroite : le journal s'efface, les onglets gardent toute la bande jusqu'au bouton externe
                    assert!(plus.right <= action.left, "le « + » déborde sur le bouton Windows Terminal (n={n}, fenêtre {l})");
                }
                if l >= 900.0 {
                    assert!(journal.right - journal.left >= 56.0, "l'onglet JOURNAL doit rester visible à {l} DIP");
                }
            }
        }
    }

    #[test]
    fn la_croix_d_un_onglet_est_dans_l_onglet_et_laisse_place_au_libelle() {
        for (l, h) in [(1200.0, 800.0), (900.0, 600.0), (700.0, 500.0)] {
            for n in 1..=crate::terminal::onglets::ONGLETS_MAX {
                if n > 1 && !terminal_peut_ajouter(l, h, n - 1) {
                    break;
                }
                for i in 0..n {
                    let z = zone_terminal_onglet(l, h, i, n);
                    let c = zone_terminal_onglet_croix(l, h, i, n);
                    assert!(c.left >= z.left && c.right <= z.right && c.top >= z.top && c.bottom <= z.bottom, "croix hors de l'onglet {i} (n={n}, fenêtre {l})");
                    assert!(c.right - c.left >= 14.0, "croix trop petite pour être visée (n={n}, fenêtre {l})");
                    // le libellé (10 DIP de marge à gauche) s'arrête avant la croix
                    assert!(z.left + 10.0 + terminal_onglet_place_libelle(&z) <= c.left, "le libellé chevauche la croix (n={n}, fenêtre {l})");
                    if n > 1 {
                        assert!(terminal_onglet_place_libelle(&z) >= 40.0, "libellé de {} DIP seulement (n={n}, fenêtre {l})", terminal_onglet_place_libelle(&z));
                    }
                }
            }
        }
    }

    #[test]
    fn le_nombre_d_onglets_est_borne_par_la_place_et_par_le_maximum() {
        assert!(terminal_peut_ajouter(1400.0, 900.0, 1));
        assert!(!terminal_peut_ajouter(1400.0, 900.0, crate::terminal::onglets::ONGLETS_MAX), "jamais plus que le maximum");
        // une fenêtre étroite refuse avant le maximum (le premier onglet, lui, s'ouvre toujours : `nouvel_onglet_terminal`)
        let mut n = 1;
        while terminal_peut_ajouter(700.0, 500.0, n) {
            n += 1;
        }
        assert!(n < crate::terminal::onglets::ONGLETS_MAX, "à 700 DIP, huit onglets ne tiennent pas (n = {n})");
    }
}
