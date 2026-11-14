// Décisions pures de la procédure de la fenêtre, sorties de `procedure.rs` (audit du 14/11/2026, découpage) : bords de
// redimensionnement, crans de molette, pages et défilement. Aucun appel Win32 : testable sans fenêtre.

use windows::Win32::UI::WindowsAndMessaging::*;

/// Code de test de position (`WM_NCHITTEST`) d'un point selon les bords qu'il touche ; `HTNOWHERE` à l'intérieur.
/// Les coins l'emportent sur les côtés, le haut et le bas sur rien d'autre.
pub(super) fn zone_de_bord(gauche: bool, droite: bool, haut: bool, bas: bool) -> u32 {
    match (gauche, droite, haut, bas) {
        (true, _, true, _) => HTTOPLEFT,
        (_, true, true, _) => HTTOPRIGHT,
        (true, _, _, true) => HTBOTTOMLEFT,
        (_, true, _, true) => HTBOTTOMRIGHT,
        (true, _, _, _) => HTLEFT,
        (_, true, _, _) => HTRIGHT,
        (_, _, true, _) => HTTOP,
        (_, _, _, true) => HTBOTTOM,
        _ => HTNOWHERE,
    }
}

/// Crans de molette signés (haut = positif) lus dans le `wParam` de `WM_MOUSEWHEEL` : 120 unités par cran
pub(super) fn crans_molette(wparam: usize) -> f32 {
    ((wparam >> 16) & 0xFFFF) as i16 as f32 / 120.0
}

/// La touche Ctrl est-elle tenue (`MK_CONTROL`) dans le `wParam` d'un message de souris
pub(super) fn ctrl_tenue(wparam: usize) -> bool {
    wparam & 0x0008 != 0
}

/// Page suivante du coup d'œil pour une molette : un cran vers le haut recule d'une page, vers le bas avance ;
/// la page reste dans `0..n`. `n` à 0 ne plante pas (page 0).
pub(super) fn page_apres_molette(page: u32, n: u32, crans: f32) -> u32 {
    if n == 0 || crans == 0.0 {
        return page.min(n.saturating_sub(1));
    }
    let sens: i64 = if crans > 0.0 { -1 } else { 1 };
    (page as i64 + sens).clamp(0, n as i64 - 1) as u32
}

/// Défilement d'un lot de renommage : 3 lignes par cran, borné au dernier nom
pub(super) fn defilement_lot(actuel: usize, crans: f32, noms: usize) -> usize {
    (actuel as i64 - (crans * 3.0) as i64).clamp(0, noms.saturating_sub(1) as i64) as usize
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn les_coins_l_emportent_sur_les_cotes() {
        assert_eq!(zone_de_bord(true, false, true, false), HTTOPLEFT);
        assert_eq!(zone_de_bord(false, true, true, false), HTTOPRIGHT);
        assert_eq!(zone_de_bord(true, false, false, true), HTBOTTOMLEFT);
        assert_eq!(zone_de_bord(false, true, false, true), HTBOTTOMRIGHT);
    }

    #[test]
    fn un_seul_bord_donne_le_cote_et_aucun_donne_rien() {
        assert_eq!(zone_de_bord(true, false, false, false), HTLEFT);
        assert_eq!(zone_de_bord(false, true, false, false), HTRIGHT);
        assert_eq!(zone_de_bord(false, false, true, false), HTTOP);
        assert_eq!(zone_de_bord(false, false, false, true), HTBOTTOM);
        assert_eq!(zone_de_bord(false, false, false, false), HTNOWHERE);
    }

    #[test]
    fn les_crans_de_molette_ont_le_bon_signe() {
        assert_eq!(crans_molette(120 << 16), 1.0);
        assert_eq!(crans_molette(0xFF88 << 16), -1.0); // -120 sur 16 bits
        assert_eq!(crans_molette(0), 0.0);
        // Les modificateurs dans les bits bas ne changent pas le cran
        assert_eq!(crans_molette((120 << 16) | 0x0008), 1.0);
    }

    #[test]
    fn la_touche_ctrl_se_lit_dans_les_bits_bas() {
        assert!(ctrl_tenue(0x0008));
        assert!(ctrl_tenue(0x0008 | 0x0004));
        assert!(!ctrl_tenue(0x0004));
        assert!(!ctrl_tenue(120 << 16));
    }

    #[test]
    fn la_page_du_coup_d_oeil_reste_dans_les_bornes() {
        assert_eq!(page_apres_molette(3, 10, 1.0), 2);
        assert_eq!(page_apres_molette(3, 10, -1.0), 4);
        assert_eq!(page_apres_molette(0, 10, 1.0), 0);
        assert_eq!(page_apres_molette(9, 10, -1.0), 9);
        assert_eq!(page_apres_molette(5, 10, 0.0), 5);
        assert_eq!(page_apres_molette(0, 0, 1.0), 0);
        assert_eq!(page_apres_molette(4, 0, -1.0), 0);
    }

    #[test]
    fn le_defilement_du_lot_est_borne() {
        assert_eq!(defilement_lot(5, 1.0, 20), 2);
        assert_eq!(defilement_lot(5, -1.0, 20), 8);
        assert_eq!(defilement_lot(1, 1.0, 20), 0);
        assert_eq!(defilement_lot(18, -2.0, 20), 19);
        assert_eq!(defilement_lot(0, -1.0, 0), 0);
    }
}
