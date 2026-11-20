// Page ESPACE (E1) : entrée, synchronisation de l'arbre avec l'emplacement affiché, touches, sortie.
// L'arbre (et le tableau des cumuls, ~22 Mo) n'existe que tant que la page est affichée : `synchroniser_espace` le
// construit à l'arrivée et le libère dès qu'un autre emplacement est montré, quel que soit le chemin pris pour partir.

use crate::*;

/// Ouvre la page ESPACE dans l'onglet affiché
pub(crate) unsafe fn ouvrir_espace(hwnd: HWND, f: &mut Fenetre) {
    if f.app.emplacement != Emplacement::Espace {
        f.espace = espace::Etat::nouveau();
        f.app.naviguer(hwnd, Emplacement::Espace);
    }
    // WM_INDEX quand une reconstruction en cours installe un index plus récent
    index::abonner(hwnd);
    synchroniser_espace(hwnd, f);
}

/// Ouvre la page ESPACE directement sur NETTOYAGE (carte « récupérable » de Ce PC, E9). Le rang vient de `espace::PAGES`.
pub(crate) unsafe fn ouvrir_nettoyage(hwnd: HWND, f: &mut Fenetre) {
    ouvrir_espace(hwnd, f);
    if let Some(k) = espace::PAGES.iter().position(|p| *p == "Nettoyage") {
        f.espace.choisir_page(k);
        lancer_nettoyage(hwnd, f);
    }
}

/// Met l'état de la page d'accord avec l'emplacement : construit l'arbre depuis l'index enregistré (sans jamais
/// parcourir les disques), ou le libère quand la page n'est plus affichée.
pub(crate) unsafe fn synchroniser_espace(hwnd: HWND, f: &mut Fenetre) {
    if f.app.emplacement != Emplacement::Espace {
        if f.espace.arbre.is_some() {
            f.espace = espace::Etat::nouveau();
        }
        return;
    }
    // Un index installé est toujours complet (une reconstruction n'installe le sien qu'à la fin) : on l'affiche
    // même pendant qu'une reconstruction tourne, et on rebâtit l'arbre quand elle en installe un nouveau.
    match index::actuel() {
        Some(i) => {
            if !f.espace.arbre.as_ref().is_some_and(|a| a.est_sur(&i)) {
                // La page NETTOYAGE ne dépend pas de l'index : un index qui s'installe ne doit ni la quitter, ni
                // perdre sa mesure, ni en lancer une seconde (le fil de la première court peut-être encore).
                let ancien = std::mem::replace(&mut f.espace, espace::Etat::nouveau());
                f.espace.reprendre_nettoyage(ancien);
                f.espace.arbre = Some(espace::analyse::Arbre::nouveau(i));
                // La carte est la vue de base (choix de l'auteur, 18/11/2026) ; l'arbre reste à un clic ou à `V`.
                // Elle a besoin de l'arbre, d'où cet appel après l'avoir posé.
                f.espace.choisir_vue(espace::Vue::Carte);
            }
        }
        None => {
            if index::enregistre_existe() {
                index::charger_enregistre(hwnd);
            }
        }
    }
}

/// Lance la mesure du nettoyage en fil de fond dès qu'on arrive sur la page NETTOYAGE (une seule fois par affichage)
pub(crate) fn lancer_nettoyage(hwnd: HWND, f: &mut Fenetre) {
    // Une suppression court encore (la page a été quittée puis rouverte) : la mesure repartira à sa fin, une mesure
    // faite pendant l'effacement montrerait des éléments en train de disparaître
    if espace::nettoyage::suppression_en_cours() {
        return;
    }
    if f.espace.nettoyage_a_lancer() && !espace::nettoyage::lancer(hwnd) {
        f.espace.nettoyage_en_cours = false;
    }
}

/// Hauteur visible de la liste (DIP), pour les touches Page haut / Page bas et le défilement
pub(crate) fn hauteur_espace(hwnd: HWND, f: &Fenetre) -> f32 {
    let mut r = RECT::default();
    unsafe {
        let _ = GetClientRect(hwnd, &mut r);
    }
    let h = (r.bottom - r.top) as f32 / (f.dpi / 96.0);
    (h - rendu::ESPACE_LISTE_Y - statut_h()).max(espace::LIGNE_H)
}

/// Pas d'une ligne de l'arbre (DIP) selon la taille de ligne choisie
pub(crate) fn pas_espace() -> f32 {
    if reglages::lignes_confort() {
        espace::LIGNE_H
    } else {
        espace::LIGNE_H_COMPACTE
    }
}

/// Quitte la page (Échap) : retour au dossier précédent, ou à Ce PC si la page est la première de l'onglet
pub(crate) unsafe fn quitter_espace(hwnd: HWND, f: &mut Fenetre) {
    if f.app.peut_reculer() {
        f.app.precedent(hwnd);
    } else {
        f.app.naviguer(hwnd, Emplacement::CePC);
    }
}

/// Après une bascule vers l'arbre : centre la ligne choisie dans la liste (l'état ne connaît pas la hauteur affichée)
pub(crate) fn centrer_espace(hwnd: HWND, f: &mut Fenetre) {
    if f.espace.a_centrer && f.espace.vue == espace::Vue::Arbre {
        let (pas, hauteur) = (pas_espace(), hauteur_espace(hwnd, f));
        f.espace.centrer_ligne(pas, hauteur);
    }
}

/// Une touche de la page : exécute ce que l'état demande
pub(crate) unsafe fn touche_espace(hwnd: HWND, f: &mut Fenetre, t: espace::Touche) {
    let (pas, hauteur) = (pas_espace(), hauteur_espace(hwnd, f));
    let action = f.espace.touche(t, pas, hauteur);
    centrer_espace(hwnd, f);
    appliquer_action_espace(hwnd, f, action);
}

/// Hauteur de la fenêtre (DIP), pour la plage de lignes de la page NETTOYAGE
fn hauteur_fenetre(hwnd: HWND, f: &Fenetre) -> f32 {
    let mut r = RECT::default();
    unsafe {
        let _ = GetClientRect(hwnd, &mut r);
    }
    (r.bottom - r.top) as f32 / (f.dpi / 96.0)
}

/// Première ligne que la page NETTOYAGE montre à cet instant (la souris vise ces lignes-là)
fn debut_nettoyage(hwnd: HWND, f: &Fenetre) -> usize {
    let n = f.espace.nettoyage.as_ref().map_or(0, |c| c.len());
    rendu::net_plage(n, f.espace.net_focus, f.espace.net_debut, hauteur_fenetre(hwnd, f)).0
}

/// Clic sur la page NETTOYAGE : une ligne la choisit, sa case la coche ; `cocher` force la coche (double-clic sur la ligne)
pub(crate) fn cliquer_nettoyage(hwnd: HWND, f: &mut Fenetre, k: usize, case: bool, cocher: bool) {
    let debut = debut_nettoyage(hwnd, f);
    f.espace.net_cliquer(k, case || cocher, debut);
}

/// Clic sur un bouton du bas de la page NETTOYAGE : la même chose que la touche correspondante
pub(crate) unsafe fn bouton_nettoyage(hwnd: HWND, f: &mut Fenetre, k: usize) {
    let action = f.espace.net_bouton(k);
    appliquer_action_espace(hwnd, f, action);
}

/// Exécute ce que l'état de la page demande après une touche ou un clic
unsafe fn appliquer_action_espace(hwnd: HWND, f: &mut Fenetre, action: espace::Action) {
    match action {
        espace::Action::Rien => {}
        espace::Action::Ouvrir(chemin) => {
            f.app.naviguer(hwnd, Emplacement::Dossier(chemin));
        }
        espace::Action::Quitter => quitter_espace(hwnd, f),
        espace::Action::Effacer(demandes) => {
            // Un seul fil à la fois ; s'il ne peut pas partir, on le dit plutôt que de laisser la page figée
            if !espace::nettoyage::lancer_suppression(hwnd, demandes) {
                f.espace.finir_suppression("La suppression n'a pas pu démarrer".to_string());
            }
        }
    }
    lancer_nettoyage(hwnd, f);
}

/// Molette : règle la position voulue de `delta` crans (positif = vers le haut, fractionnaire sur un pavé tactile), sans
/// toucher à la ligne choisie ; le dessin la rejoint en glissant (`Etat::animer`), comme la liste de fichiers
pub(crate) fn molette_espace(hwnd: HWND, f: &mut Fenetre, delta: f32) {
    if f.espace.page == espace::PAGE_NETTOYAGE {
        let debut = debut_nettoyage(hwnd, f);
        let visibles = rendu::net_visibles(hauteur_fenetre(hwnd, f));
        f.espace.molette_nettoyage(delta, debut, visibles);
        return;
    }
    // La carte tient dans la page : rien à faire défiler
    if f.espace.vue == espace::Vue::Carte {
        return;
    }
    let (pas, hauteur) = (pas_espace(), hauteur_espace(hwnd, f));
    let n = f.espace.lignes().len();
    let maxi = (n as f32 * pas - hauteur).max(0.0);
    f.espace.defilement = (f.espace.defilement - delta * pas * 3.0).clamp(0.0, maxi);
}
