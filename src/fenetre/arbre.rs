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

/// Fait défiler le volet (arbre) de `delta` DIP, sans dépasser le contenu
pub(crate) unsafe fn defiler_volet(hwnd: HWND, f: &mut Fenetre, delta: f32) {
    let (_, hp) = taille_client(hwnd);
    let bas = f.dip(hp as i32) - 52.0;
    let contenu = zones_volet(&f.app).iter().map(|z| z.1.bottom).fold(0.0, f32::max) + f.app.volet_defilement;
    let max = (contenu - bas).max(0.0);
    f.app.volet_defilement = (f.app.volet_defilement + delta).clamp(0.0, max);
}

/// Fait défiler le volet pour que la ligne `j` de l'arbre soit dans sa partie visible
pub(crate) unsafe fn montrer_ligne_arbre(hwnd: HWND, f: &mut Fenetre, j: usize) {
    let (_, hp) = taille_client(hwnd);
    let (haut, bas) = (HAUTEUR_TITRE + 4.0, f.dip(hp as i32) - 52.0);
    if let Some((_, r)) = zones_volet(&f.app).into_iter().find(|(q, _)| *q == Cible::Arbre(j)) {
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
