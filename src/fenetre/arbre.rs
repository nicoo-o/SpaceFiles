// Volet de l'arbre des dossiers : bascule, défilement, ligne visible et clic.

use crate::*;
/// Ctrl+Maj+B : le clavier entre dans l'arbre du volet (qu'on allume au besoin), ou en sort
pub(crate) fn basculer_arbre(f: &mut Fenetre) {
    if !reglages::ARBRE.load(Ordering::Relaxed) {
        reglages::modifier(|r| r.arbre = true);
    }
    f.app.arbre_focus = !f.app.arbre_focus;
    if f.app.arbre_focus {
        f.app.arbre_curseur = None;
    }
}

/// Fait défiler le milieu du volet (sections et arbre, sous Ce PC et au-dessus des disques) de `delta` DIP, sans dépasser le contenu
pub(crate) unsafe fn defiler_volet(_hwnd: HWND, f: &mut Fenetre, delta: f32) {
    let max = dispo_volet(&f.app).defilement_max;
    f.app.volet_defilement = (f.app.volet_defilement + delta).clamp(0.0, max);
}

/// Fait défiler le volet pour que la ligne `j` de l'arbre soit dans sa partie visible
pub(crate) unsafe fn montrer_ligne_arbre(hwnd: HWND, f: &mut Fenetre, j: usize) {
    let d = dispo_volet(&f.app);
    let (haut, bas) = d.region;
    if let Some((_, r)) = d.zones.into_iter().find(|(q, _)| *q == Cible::Arbre(j)) {
        if r.top < haut {
            defiler_volet(hwnd, f, r.top - haut);
        } else if r.bottom > bas {
            defiler_volet(hwnd, f, r.bottom - bas);
        }
    }
}

/// L'arbre défile jusqu'au dossier ouvert, une fois par dossier (la ligne n'existe qu'une fois ses parents lus : on réessaie au dessin suivant)
pub(crate) unsafe fn montrer_dossier_arbre(hwnd: HWND, f: &mut Fenetre) {
    if !reglages::ARBRE.load(Ordering::Relaxed) {
        return;
    }
    let Some(cle) = f.app.dossier().map(arbre::cle) else { return };
    if f.app.arbre_vu == cle {
        return;
    }
    if let Some(j) = f.app.lignes_arbre().iter().position(|l| arbre::cle(&l.chemin) == cle) {
        f.app.arbre_vu = cle;
        montrer_ligne_arbre(hwnd, f, j);
    }
}

/// Clic sur une ligne de l'arbre : le chevron déplie ou replie, le reste ouvre le dossier
pub(crate) unsafe fn clic_arbre(hwnd: HWND, f: &mut Fenetre, i: usize, x: f32) {
    let lignes = f.app.lignes_arbre();
    let Some(l) = lignes.get(i) else { return };
    f.app.arbre_curseur = Some(arbre::cle(&l.chemin));
    f.app.arbre_focus = false;
    let chevron = 8.0 + l.profondeur as f32 * arbre::RETRAIT + 20.0;
    if x < chevron && l.enfants != Some(false) {
        arbre::basculer(&l.chemin);
    } else {
        f.app.naviguer(hwnd, Emplacement::dossier(&l.chemin));
    }
}
