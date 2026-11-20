// Souris dans la fenêtre : cible sous le pointeur, géométrie de la page, et glisser-déposer (zones, dossier à ressort, menu de dépôt).

use crate::*;
/// Le point (DIP, coordonnées du panneau de largeur `l`) est dans la zone des fichiers : sous l'en-tête, au-dessus de la barre
/// d'état, de la fin du volet au bord droit du panneau. Les marges gauche et droite en font partie.
pub(crate) fn zone_fichiers(l: f32, h: f32, x: f32, y: f32) -> bool {
    x > largeur_volet() && x < l && y >= liste_y() && y < h - statut_h()
}

/// Abscisse (DIP) où s'arrête une ligne de la vue liste : sa hauteur de fond va jusqu'ici, au-delà c'est la marge droite vide
pub(crate) fn bord_droit_ligne(l: f32) -> f32 {
    l - MARGE_LISTE_DROITE + 6.0
}

/// Le point est sur la bande des lignes de la vue liste (hors marges gauche et droite)
pub(crate) fn dans_ligne(l: f32, x: f32) -> bool {
    x > largeur_volet() + MARGE_LISTE - 6.0 && x < bord_droit_ligne(l)
}

/// Qui reçoit la souris à cet instant
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum DestSouris {
    /// L'explorateur : survol, clics, lasso, glisser, menus
    Explorateur,
    /// La palette de commandes, seule : rien de ce qui est dessous ne réagit
    Palette,
    /// Le tiroir Historique, sur sa seule zone
    Tiroir,
}

/// La palette ouverte prend toute la souris ; le tiroir Historique la prend seulement sous son rectangle
pub(crate) fn destinataire_souris(palette_ouverte: bool, tiroir_ouvert: bool, dans_tiroir: bool) -> DestSouris {
    if palette_ouverte {
        DestSouris::Palette
    } else if tiroir_ouvert && dans_tiroir {
        DestSouris::Tiroir
    } else {
        DestSouris::Explorateur
    }
}

/// Destinataire de la souris au point (x, y) en DIP de la fenêtre
pub(crate) fn destinataire(f: &Fenetre, x: f32, y: f32) -> DestSouris {
    let dans_tiroir = f.historique && f.rendu.zones_historique.borrow().cible(x, y) != rendu::CibleHistorique::Dehors;
    destinataire_souris(f.palette.is_some(), f.historique, dans_tiroir)
}

/// Le message souris `lparam` (coordonnées client) revient-il à l'explorateur ? Faux pendant la palette, et sous le tiroir.
pub(crate) fn souris_pour_explorateur(f: &Fenetre, lparam: LPARAM) -> bool {
    destinataire(f, f.dip(x_lparam(lparam)), f.dip(y_lparam(lparam))) == DestSouris::Explorateur
}

/// La souris est sur la palette : la ligne survolée devient la ligne choisie. Vrai s'il faut redessiner (choix changé, ou le
/// survol de l'explorateur dessous vient d'être effacé).
pub(crate) fn survol_palette(f: &mut Fenetre, x: f32, y: f32) -> bool {
    let cible = f.rendu.zones_palette.borrow().cible(x, y);
    let mut change = false;
    if let (rendu::CiblePalette::Ligne(i), Some(p)) = (cible, f.palette.as_mut()) {
        change = p.survoler(i);
    }
    change | efface_survol(f)
}

/// Plus rien de l'explorateur n'est survolé. Vrai si quelque chose l'était.
pub(crate) fn efface_survol(f: &mut Fenetre) -> bool {
    let avait = f.app.survol != Cible::Rien || f.app.survol_etiquette.is_some();
    f.app.survol = Cible::Rien;
    f.app.survol_etiquette = None;
    avait
}

/// Curseur voulu sur la palette : la main sur une ligne ou une puce, la flèche ailleurs
pub(crate) fn main_sur_palette(cible: rendu::CiblePalette) -> bool {
    matches!(cible, rendu::CiblePalette::Ligne(_) | rendu::CiblePalette::Puce(_))
}

/// Élément de l'interface sous ce point (coordonnées client, pixels)
pub(crate) unsafe fn cible_sous(hwnd: HWND, f: &Fenetre, px: i32, py: i32) -> Cible {
    let (lp, hp) = taille_client(hwnd);
    let (x, y, lt, h) = (f.dip(px), f.dip(py), f.dip(lp as i32), f.dip(hp as i32));
    let app = &f.app;

    if y < HAUTEUR_TITRE {
        if x >= lt - 3.0 * BOUTON_LARGEUR {
            return match ((lt - x) / BOUTON_LARGEUR) as i32 {
                0 => Cible::Fermer,
                1 => Cible::Agrandir,
                _ => Cible::Reduire,
            };
        }
        if dedans(&zone_essentiel_haut(lt), x, y) {
            return Cible::EssentielHaut;
        }
        if dedans(&zone_reglages_haut(lt), x, y) {
            return Cible::Reglages;
        }
        let n = f.nb_onglets();
        for (quoi, r) in zones_onglets(lt, n) {
            if dedans(&r, x, y) {
                if let Cible::Onglet(k) = quoi {
                    if n > 1 && x >= r.right - 24.0 {
                        return Cible::FermerOnglet(k);
                    }
                }
                return quoi;
            }
        }
        return Cible::Rien;
    }
    // Panneau Réglages : il couvre le volet, le chemin et la liste sous la barre de titre
    if app.emplacement == Emplacement::Reglages {
        return cible_page(f, lt, h, x, y);
    }
    // Page ESPACE (E1) : couvre aussi le volet ; elle a sa croix, son rail et ses lignes, rien dessous ne doit réagir
    if app.emplacement == Emplacement::Espace {
        return cible_espace(f, lt, h, x, y);
    }
    // Pastilles d'opération (coordonnées de la fenêtre) : par-dessus la liste, jamais sur le volet
    if !f.modale() {
        let d = f.rendu.zones_pastilles.borrow();
        match d.sous(x, y) {
            Some((i, Some(b))) => return Cible::BoutonPastille(i, b),
            Some((i, None)) => return Cible::Pastille(i),
            None if d.plus.is_some_and(|(_, z)| z.contient(x, y)) => return Cible::PlusPastilles,
            None => {}
        }
    }
    for (quoi, r) in zones_volet(app) {
        if dedans(&r, x, y) {
            return quoi;
        }
    }
    // La liste s'arrête au volet d'aperçu
    let ll = largeur_liste(lt, f.apercu_visible);
    if x <= largeur_volet() || x >= ll {
        return Cible::Rien;
    }
    // Panneau terminal : il est au-dessus de la liste, mais jamais du pied
    if f.terminal_affiche() {
        let panneau = crate::rendu::zone_terminal_panneau(ll, h);
        if dedans(&panneau, x, y) {
            if dedans(&crate::rendu::zone_terminal_fermer(ll, h), x, y) {
                return Cible::TerminalFermer;
            }
            if dedans(&crate::rendu::zone_terminal_action(ll, h), x, y) {
                return Cible::TerminalExterne;
            }
            // T4a : onglets, puis « + » (seulement tant qu'un onglet de plus tient : sans lui, le bouton n'est pas dessiné).
            // Même seuil que `dessiner_terminal` : un panneau trop petit n'est pas dessiné, ses boutons ne se cliquent pas
            if panneau.bottom - panneau.top < 40.0 || panneau.right - panneau.left < 80.0 {
                return Cible::TerminalPanneau;
            }
            if crate::rendu::terminal_journal_largeur(ll, h) > 0.0 && dedans(&crate::rendu::zone_terminal_journal(ll, h), x, y) {
                return Cible::TerminalJournal;
            }
            let n = f.terminaux.len();
            for i in 0..n {
                // La croix est dans l'onglet : elle passe avant lui
                if dedans(&crate::rendu::zone_terminal_onglet_croix(ll, h, i, n), x, y) {
                    return Cible::TerminalOngletFermer(i);
                }
                if dedans(&crate::rendu::zone_terminal_onglet(ll, h, i, n), x, y) {
                    return Cible::TerminalOnglet(i);
                }
            }
            if crate::rendu::terminal_peut_ajouter(ll, h, n) && dedans(&crate::rendu::zone_terminal_plus(ll, h, n), x, y) {
                return Cible::TerminalNouveau;
            }
            return Cible::TerminalPanneau;
        }
    }
    // Double panneau : coordonnées du panneau qui a le focus ; l'autre panneau n'est qu'une cible de clic
    let l = largeur_panneau(ll, f.second.is_some());
    let mut x = x;
    if f.second.is_some() {
        let a_droite = x >= l;
        if a_droite != f.focus_droite {
            return Cible::AutrePanneau;
        }
        if a_droite {
            x -= l - largeur_volet();
        }
    }
    for (quoi, r) in boutons_nav() {
        if dedans(&r, x, y) {
            return quoi;
        }
    }
    if app.emplacement != Emplacement::Reglages && dedans(&zone_terminal(l), x, y) {
        return Cible::Terminal;
    }
    for (k, r) in app.zones.densite.iter().enumerate() {
        if r.is_some_and(|z| dedans(&z, x, y)) {
            return Cible::Densite(k);
        }
    }
    if let Some(p) = app.zones.barre.iter().find(|p| dedans(&p.zone, x, y)) {
        return Cible::Outil(p.outil);
    }
    for (i, (r, _, _)) in app.zones.segments.iter().enumerate() {
        if dedans(r, x, y) {
            return Cible::Segment(i);
        }
    }
    if let Some(k) = app.zones.puces.iter().position(|r| dedans(r, x, y)) {
        return Cible::Puce(k);
    }
    if app.zones.note.is_some_and(|r| dedans(&r, x, y)) {
        return Cible::Note;
    }
    if app.zones.corbeille.is_some_and(|r| dedans(&r, x, y)) {
        return Cible::Corbeille;
    }
    if let Some((_, q)) = app.zones.stockages.iter().find(|(r, _)| dedans(r, x, y)) {
        return *q;
    }
    if let Some(z) = app.zones.jours.iter().find(|z| dedans(&z.0, x, y)) {
        return Cible::Jour(z.1);
    }
    if y >= h - statut_h() && x > largeur_volet() && x < lt && app.emplacement != Emplacement::Reglages {
        return Cible::Statut;
    }
    // Vide de la barre du chemin (après les segments)
    if (nav_y()..nav_y() + nav_h()).contains(&y) && x >= chemin_x() - 10.0 && x < l - MARGE_LISTE && app.emplacement != Emplacement::Reglages {
        return Cible::Chemin;
    }
    // Toute la zone des fichiers répond, marge droite comprise : c'est le vide où l'on clique pour le menu du fond, le lasso, le dépôt
    let dans_liste = zone_fichiers(l, h, x, y);
    match app.affichage {
        Affichage::Grille if dans_liste => return case_grille(app, x, y).map_or(Cible::FondListe, Cible::Ligne),
        Affichage::Colonnes if dans_liste => {
            let (cx, cw) = colonnes_miller(l)[1];
            let i = ((y - liste_y() + app.defilement) / hauteur_ligne()) as usize;
            return if x >= cx && x < cx + cw && i < app.entrees.len() { Cible::Ligne(i) } else { Cible::FondListe };
        }
        Affichage::Grille | Affichage::Colonnes => return Cible::Rien,
        Affichage::Liste => {}
    }
    if x > largeur_volet() && x < l && (entete_y()..entete_y() + entete_h()).contains(&y) {
        for (col, cx, w) in colonnes(l) {
            if x >= cx && x < cx + w {
                return Cible::Entete(col);
            }
        }
    }
    if dans_liste {
        // Les lignes s'arrêtent avant la marge droite ; le reste (marges gauche et droite, dessous la dernière ligne) est le fond
        let r = ((y - liste_y() + app.defilement) / hauteur_ligne()) as usize;
        if dans_ligne(l, x) && r < app.nb_rangs() {
            return match app.rang(r) {
                app::Rang::Section(s) => Cible::Section(s),
                app::Rang::Element(i) => Cible::Ligne(i),
            };
        }
        return Cible::FondListe;
    }
    Cible::Rien
}

/// Le reflet bouge dans son élément (ou en sort) : demande un WM_PAINT limité au rectangle de l'élément éclairé au dernier dessin
/// (même coupé depuis : le halo doit partir) et à celui de l'élément éclairable (`zone_candidate`, pointeur dedans ou non, seulement
/// si le reflet est actif). Ce rectangle décide seulement s'il y a un repaint : l'image est redessinée en entier à chaque fois
/// (`Rendu::dessiner` n'a pas de dessin partiel). Aucun repaint si rien n'est éclairé ni éclairable : une souris qui bouge sur
/// un élément non éclairable ne coûte rien.
/// Invariant (unsafe) : fil d'interface, `hwnd` est la fenêtre principale vivante ; `InvalidateRect` copie le RECT, aucune
/// référence n'est gardée après l'appel. Un message synchrone n'est jamais envoyé d'ici (pas de `&mut Fenetre` repris).
pub(crate) unsafe fn invalider_reflet(hwnd: HWND, f: &Fenetre) {
    let candidate = if reflet::actif_maintenant() { f.rendu.zone_candidate.get() } else { None };
    let echelle = f.dpi / 96.0;
    for z in [f.rendu.zone_reflet.get(), candidate].into_iter().flatten() {
        // Un pixel de marge de chaque côté : le trait de la lueur est centré sur le bord
        let r = RECT { left: (z.left * echelle).floor() as i32 - 1, top: (z.top * echelle).floor() as i32 - 1, right: (z.right * echelle).ceil() as i32 + 1, bottom: (z.bottom * echelle).ceil() as i32 + 1 };
        let _ = InvalidateRect(hwnd, Some(&r), false);
    }
}

/// Point client (px) en coordonnées du panneau qui a le focus (DIP), et largeur de ce panneau
pub(crate) unsafe fn point_panneau(hwnd: HWND, f: &Fenetre, px: i32, py: i32) -> (f32, f32, f32) {
    let (lp, _) = taille_client(hwnd);
    let ll = largeur_liste(f.dip(lp as i32), f.apercu_visible);
    let l = largeur_panneau(ll, f.second.is_some());
    let mut x = f.dip(px);
    if f.second.is_some() && f.focus_droite {
        x -= l - largeur_volet();
    }
    (x, f.dip(py), l)
}

/// Ligne de la page des réglages, ou élément du formulaire, sous ce point (DIP)
pub(crate) fn cible_page(f: &Fenetre, l: f32, h: f32, x: f32, y: f32) -> Cible {
    if f.page.formulaire.is_some() {
        let carte = page::carte_formulaire(l, h);
        return page::zones_formulaire(carte).into_iter().find(|(_, r)| dedans(r, x, y)).map_or(Cible::Rien, |(k, _)| Cible::Champ(k));
    }
    if dedans(&page::zone_fermer(l), x, y) {
        return Cible::ReglagesFermer;
    }
    if let Some((k, _)) = page::zones_nav().into_iter().find(|(_, r)| dedans(r, x, y)) {
        return Cible::ReglagesNav(k);
    }
    if x <= page::gauche() - 6.0 || x >= page::droite(l) + 6.0 || y < page::PAGE_Y || y >= h - statut_h() {
        return Cible::Rien;
    }
    let lignes = f.lignes();
    let yy = y - page::PAGE_Y + f.page.defilement;
    // La marge de 10 au-dessus de la carte n'est pas la carte
    page::positions(&lignes, f.page.categorie)
        .iter()
        .position(|&(a, b)| b > 0.0 && yy >= a + REGLAGE_CARTE_ECART && yy < a + b)
        .filter(|&k| lignes[k].element.choisissable())
        .map_or(Cible::Rien, Cible::Reglage)
}

/// Élément de la page ESPACE sous ce point (DIP) : croix, page du rail ou ligne de l'arbre
pub(crate) fn cible_espace(f: &Fenetre, l: f32, h: f32, x: f32, y: f32) -> Cible {
    if dedans(&page::zone_fermer(l), x, y) {
        return Cible::EspaceFermer;
    }
    if let Some(k) = (0..espace::PAGES.len()).find(|&k| dedans(&rendu::rect(page::NAV_X, page::NAV_Y + k as f32 * page::NAV_H, page::NAV_W, page::NAV_H - 2.0), x, y)) {
        return Cible::EspaceNav(k);
    }
    let e = &f.espace;
    if e.page == espace::PAGE_ANALYSE && e.arbre.is_some() {
        if let Some(k) = (0..espace::VUES.len()).find(|&k| dedans(&rendu::espace_segment_vue(l, k), x, y)) {
            return Cible::EspaceVue(k);
        }
    }
    if e.page == espace::PAGE_NETTOYAGE {
        return cible_nettoyage(e, l, h, x, y);
    }
    if e.page != espace::PAGE_ANALYSE || x < page::gauche() - 8.0 || x > page::droite(l) + 8.0 || y >= h - statut_h() {
        return Cible::Rien;
    }
    if e.vue == espace::Vue::Carte {
        // Fil d'Ariane : toutes les pastilles sauf la dernière (l'endroit où l'on est)
        let miettes = rendu::espace_miettes(e, l);
        if let Some((_, m)) = miettes.iter().take(miettes.len().saturating_sub(1)).find(|(r, _)| dedans(r, x, y)) {
            return Cible::EspaceChemin(m.niveau);
        }
        return e.cellule_sous(rendu::espace_zone_carte(l, h), x, y).map_or(Cible::Rien, Cible::EspaceCellule);
    }
    e.ligne_sous(y - rendu::ESPACE_LISTE_Y, pas_espace()).map_or(Cible::Rien, Cible::EspaceLigne)
}

/// Élément de la page NETTOYAGE sous ce point (DIP) : bouton du bas, case ou ligne de la liste. Les lignes sont celles que le
/// dessin montre (`rendu::net_plage`) et la zone de la liste s'arrête au-dessus du bas de page, comme le dessin.
pub(crate) fn cible_nettoyage(e: &espace::Etat, l: f32, h: f32, x: f32, y: f32) -> Cible {
    let boutons: &[usize] = match &e.net_etape {
        espace::EtapeNet::Liste if e.nettoyage.as_ref().is_some_and(|c| !c.is_empty()) => &[espace::NET_SUPPRIMER],
        espace::EtapeNet::Confirmation(_) => &[espace::NET_CONFIRMER, espace::NET_ANNULER],
        _ => &[],
    };
    if let Some(&k) = boutons.iter().find(|&&k| dedans(&rendu::net_bouton(l, h, k), x, y)) {
        return Cible::NettoyageBouton(k);
    }
    // Seule la liste elle-même se vise : pas la confirmation (aucune ligne à choisir), pas la mesure en cours
    let (espace::EtapeNet::Liste, Some(cats)) = (&e.net_etape, e.nettoyage.as_ref()) else { return Cible::Rien };
    let (debut, fin) = rendu::net_plage(cats.len(), e.net_focus, e.net_debut, h);
    (debut..fin)
        .find_map(|k| {
            let rang = k - debut;
            if dedans(&rendu::net_zone_case(rang), x, y) {
                Some(Cible::NettoyageCase(k))
            } else if dedans(&rendu::net_rect_ligne(l, rang), x, y) {
                Some(Cible::NettoyageLigne(k))
            } else {
                None
            }
        })
        .unwrap_or(Cible::Rien)
}

/// Segment visé dans la carte `k` (point en DIP de la fenêtre), pour un réglage à contrôle segmenté
pub(crate) fn segment_vise(f: &Fenetre, l: f32, k: usize, x: f32, y: f32) -> Option<usize> {
    let lignes = f.lignes();
    let page::Valeur::Segments(options, _) = &lignes.get(k)?.valeur else { return None };
    let (y0, _) = *page::positions(&lignes, f.page.categorie).get(k)?;
    let carte = page::rect_carte(page::PAGE_Y + y0 - f.page.defilement, l);
    page::segment_sous(carte, options, x, y)
}

/// Hauteur visible de la page des réglages (DIP)
pub(crate) unsafe fn hauteur_page(hwnd: HWND, f: &Fenetre) -> f32 {
    let (_, hp) = taille_client(hwnd);
    (f.dip(hp as i32) - statut_h() - page::PAGE_Y).max(0.0)
}

/// Zone visée par un glisser-déposer au point client (px)
pub(crate) unsafe fn zone_depot(hwnd: HWND, f: &Fenetre, x: i32, y: i32) -> Cible {
    match cible_sous(hwnd, f, x, y) {
        Cible::Ligne(i) if f.app.entrees[i].est_conteneur() => Cible::Ligne(i),
        c @ (Cible::Favori(_) | Cible::Disque(_) | Cible::Segment(_) | Cible::Nuage(_) | Cible::Serveur(_) | Cible::Arbre(_)) => c,
        Cible::Onglet(k) | Cible::FermerOnglet(k) => Cible::Onglet(k),
        Cible::Epingle(i) => Cible::Epingle(i),
        Cible::Groupe(k) => Cible::Groupe(k),
        Cible::CePc => Cible::CePc,
        Cible::AutrePanneau => Cible::AutrePanneau,
        _ => Cible::FondListe,
    }
}

/// Glisser en attente au-dessus d'un dossier (ou d'un onglet) : il s'ouvre, comme sur macOS
pub(crate) unsafe fn ouvrir_ressort(hwnd: HWND, f: &mut Fenetre, zone: Cible) {
    match zone {
        Cible::Ligne(i) => f.app.ouvrir(hwnd, i),
        Cible::Favori(i) => f.app.ouvrir_favori(hwnd, i),
        Cible::Epingle(i) => f.app.ouvrir_epingle(hwnd, i),
        Cible::Disque(i) => f.app.ouvrir_disque(hwnd, i),
        Cible::Nuage(_) | Cible::Serveur(_) => f.app.ouvrir_lieu(hwnd, zone),
        Cible::Arbre(i) => {
            if let Some(l) = f.app.lignes_arbre().into_iter().nth(i) {
                f.app.naviguer(hwnd, Emplacement::dossier(&l.chemin));
            }
        }
        Cible::Segment(i) => {
            if let Some((_, emp, _)) = f.app.zones.segments.get(i).cloned() {
                f.app.naviguer(hwnd, emp);
            }
        }
        Cible::Onglet(k) => f.aller_onglet(k),
        _ => return,
    }
    f.app.depot = Cible::Rien;
    sons::jouer(sons::Son::Tic);
    animer(hwnd, f);
}

pub(crate) unsafe fn depot_zone_ecran(hwnd: HWND, p: POINT) -> Option<(&'static mut Fenetre, Cible)> {
    let f = etat_de(hwnd)?;
    let mut c = p;
    let _ = ScreenToClient(hwnd, &mut c);
    let zone = zone_depot(hwnd, f, c.x, c.y);
    Some((f, zone))
}

pub(crate) unsafe fn depot_destination(hwnd: HWND, p: POINT) -> Option<String> {
    let (f, zone) = depot_zone_ecran(hwnd, p)?;
    if zone == Cible::AutrePanneau {
        return f.second.as_ref().and_then(|o| o.dossier().map(str::to_string));
    }
    if let Cible::Groupe(k) = zone {
        return f.app.groupes.get(k).map(|g| format!("{DEPOT_GROUPE}{}", g.nom));
    }
    f.app.chemin_cible(zone)
}

pub(crate) unsafe fn depot_survol(hwnd: HWND, p: Option<POINT>) {
    let zone = p.and_then(|p| depot_zone_ecran(hwnd, p)).map_or(Cible::Rien, |(_, z)| z);
    if let Some(f) = etat_de(hwnd) {
        if f.app.depot != zone {
            f.app.depot = zone;
            // Le compte à rebours repart à chaque changement de cible
            let delai = reglages::actuels().ressort_ms;
            match zone {
                Cible::Ligne(_) | Cible::Favori(_) | Cible::Epingle(_) | Cible::Disque(_) | Cible::Segment(_) | Cible::Onglet(_) | Cible::Nuage(_) | Cible::Serveur(_) | Cible::Arbre(_)
                    if delai > 0 =>
                {
                    SetTimer(hwnd, MINUTERIE_RESSORT, delai, None);
                }
                _ => {
                    let _ = KillTimer(hwnd, MINUTERIE_RESSORT);
                }
            }
            let _ = InvalidateRect(hwnd, None, false);
        }
    }
}

/// Destination de dépôt qui n'est pas un dossier : un groupe du volet (préfixe impossible dans un chemin)
pub(crate) const DEPOT_GROUPE: &str = "\u{1}GROUPE:";

pub(crate) unsafe fn depot_deposer(hwnd: HWND, sources: Vec<String>, dest: String, deplacer: bool) {
    if let (Some(f), Some(nom)) = (etat_de(hwnd), dest.strip_prefix(DEPOT_GROUPE)) {
        let n = groupes::ajouter(nom, &sources);
        f.app.depot = Cible::Rien;
        f.app.annoncer(format!("{nom} · {n} AJOUTE{}", if n > 1 { "S" } else { "" }), false);
        sons::jouer(sons::Son::Tic);
        rafraichir_groupes(hwnd, f);
        return;
    }
    if let Some(f) = etat_de(hwnd) {
        f.app.deposer(hwnd, sources, dest, deplacer);
        let _ = InvalidateRect(hwnd, None, false);
    }
}

/// Dépôt avec le bouton droit : menu « copier ici / déplacer ici / raccourcis ici » au point de dépôt
pub(crate) unsafe fn depot_menu(hwnd: HWND, sources: Vec<String>, dest: String, p: POINT) {
    if dest.starts_with(DEPOT_GROUPE) {
        return depot_deposer(hwnd, sources, dest, false);
    }
    let Some(f) = etat_de(hwnd) else { return };
    use menu::Autre;
    use Choix::{Autre as A, Separation as S};
    let nom = dest.trim_end_matches('\\').rsplit('\\').next().unwrap_or(&dest).to_uppercase();
    let n = sources.len();
    let quoi = if n > 1 { format!("{n} ÉLÉMENTS") } else { String::new() };
    let choix = vec![
        A(format!("COPIER {quoi} ICI").replace("  ", " "), Autre::DeposerIci(0)),
        A(format!("DEPLACER {quoi} ICI").replace("  ", " "), Autre::DeposerIci(1)),
        A(if n > 1 { "CRÉER DES RACCOURCIS ICI".into() } else { "CRÉER UN RACCOURCI ICI".into() }, Autre::DeposerIci(2)),
        S,
        A("ANNULER".into(), Autre::DeposerIci(3)),
    ];
    f.depot_droit = Some((sources, dest));
    let mut c = p;
    let _ = ScreenToClient(hwnd, &mut c);
    let _ = SetForegroundWindow(hwnd);
    let (x, y) = (f.dip(c.x), f.dip(c.y));
    ouvrir_menu(hwnd, f, choix, x, y);
    f.app.annoncer(format!("DEPOSER DANS {nom} : CHOISISSEZ"), false);
    let _ = InvalidateRect(hwnd, None, false);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zone_fichiers_exclut_le_volet_l_en_tete_et_la_barre_d_etat_mais_pas_le_haut_de_la_liste() {
        let (l, h) = (1000.0f32, 700.0f32);
        let v = largeur_volet();
        // Bords exacts : x strictement entre le volet et `l`, y de liste_y() inclus à la barre d'état exclue
        assert!(!zone_fichiers(l, h, v, liste_y() + 10.0));
        assert!(zone_fichiers(l, h, v + 0.5, liste_y()));
        assert!(!zone_fichiers(l, h, l, liste_y() + 10.0));
        assert!(zone_fichiers(l, h, l - 0.5, h - statut_h() - 0.5));
        assert!(!zone_fichiers(l, h, 500.0, liste_y() - 0.5));
        assert!(!zone_fichiers(l, h, 500.0, h - statut_h()));
    }

    /// Une page NETTOYAGE mesurée avec `n` catégories
    fn page_nettoyage(n: usize) -> espace::Etat {
        const IDS: [&str; 8] = ["a", "b", "c", "d", "e", "f", "g", "h"];
        let mut e = espace::Etat::nouveau();
        e.page = espace::PAGE_NETTOYAGE;
        assert!(e.nettoyage_a_lancer());
        let cats = (0..n)
            .map(|k| espace::nettoyage::Categorie {
                id: IDS[k],
                libelle: "Essai",
                risque: "",
                mode: espace::nettoyage::Mode::Direct,
                octets: 10 - k as u64,
                fichiers: 1,
                elements: 1,
                illisibles: 0,
                anormal: false,
                chemins: Vec::new(),
            })
            .collect();
        e.recevoir_nettoyage(cats);
        e
    }

    #[test]
    fn la_souris_vise_la_case_la_ligne_et_le_bouton_de_nettoyage_la_ou_le_dessin_les_pose() {
        let (l, h) = (1000.0f32, 700.0f32);
        let e = page_nettoyage(3);
        // Milieu de la case, puis du texte, de chaque ligne : ce que `dessiner_espace_nettoyage` trace aux mêmes rectangles
        for rang in 0..3 {
            let (case, ligne) = (rendu::net_zone_case(rang), rendu::net_rect_ligne(l, rang));
            assert_eq!(cible_nettoyage(&e, l, h, (case.left + case.right) / 2.0, (case.top + case.bottom) / 2.0), Cible::NettoyageCase(rang));
            assert_eq!(cible_nettoyage(&e, l, h, (ligne.left + ligne.right) / 2.0, (ligne.top + ligne.bottom) / 2.0), Cible::NettoyageLigne(rang));
        }
        // Entre deux lignes (les 4 DIP d'écart) et sous la dernière : rien
        let ligne = rendu::net_rect_ligne(l, 0);
        assert_eq!(cible_nettoyage(&e, l, h, (ligne.left + ligne.right) / 2.0, ligne.bottom + 2.0), Cible::Rien);
        let dernier = rendu::net_rect_ligne(l, 2);
        assert_eq!(cible_nettoyage(&e, l, h, (dernier.left + dernier.right) / 2.0, dernier.bottom + 30.0), Cible::Rien);
        // Le bouton SUPPRIMER est dans le bas de page ; CONFIRMER et ANNULER n'existent pas sur la liste
        let b = rendu::net_bouton(l, h, espace::NET_SUPPRIMER);
        assert_eq!(cible_nettoyage(&e, l, h, (b.left + b.right) / 2.0, (b.top + b.bottom) / 2.0), Cible::NettoyageBouton(espace::NET_SUPPRIMER));
        // (ANNULER se recouvre avec SUPPRIMER sur la liste : le test vise CONFIRMER, qui est à côté)
        let c = rendu::net_bouton(l, h, espace::NET_CONFIRMER);
        assert_eq!(cible_nettoyage(&e, l, h, (c.left + c.right) / 2.0, (c.top + c.bottom) / 2.0), Cible::Rien);
    }

    #[test]
    fn la_souris_de_nettoyage_suit_le_defilement_de_la_molette() {
        let (l, h) = (1000.0f32, 300.0f32);
        let mut e = page_nettoyage(8);
        let visibles = rendu::net_visibles(h);
        assert!(visibles < 8, "la fenêtre de l'essai doit être trop basse pour huit lignes");
        // Au début, la première ligne est la rangée 0 ; après la molette, c'est la catégorie `debut` qui y est
        let r = rendu::net_rect_ligne(l, 0);
        let (x, y) = ((r.left + r.right) / 2.0, (r.top + r.bottom) / 2.0);
        assert_eq!(cible_nettoyage(&e, l, h, x, y), Cible::NettoyageLigne(0));
        e.molette_nettoyage(-1.0, 0, visibles);
        assert_eq!(cible_nettoyage(&e, l, h, x, y), Cible::NettoyageLigne(2));
        // Pendant la confirmation, aucune ligne n'est visable : seuls les deux boutons répondent
        e.net_etape = espace::EtapeNet::Confirmation(Vec::new());
        assert_eq!(cible_nettoyage(&e, l, h, x, y), Cible::Rien);
        let b = rendu::net_bouton(l, h, espace::NET_CONFIRMER);
        assert_eq!(cible_nettoyage(&e, l, h, (b.left + b.right) / 2.0, (b.top + b.bottom) / 2.0), Cible::NettoyageBouton(espace::NET_CONFIRMER));
    }

    #[test]
    fn dans_ligne_commence_6_dip_avant_la_marge_gauche_et_finit_au_bord_droit() {
        let l = 900.0f32;
        let debut = largeur_volet() + MARGE_LISTE - 6.0;
        assert!(!dans_ligne(l, debut));
        assert!(dans_ligne(l, debut + 0.5));
        assert!(!dans_ligne(l, bord_droit_ligne(l)));
        assert!(dans_ligne(l, bord_droit_ligne(l) - 0.5));
        // Le bord droit suit la largeur du panneau, d'un décalage constant
        assert_eq!(bord_droit_ligne(l + 100.0) - bord_droit_ligne(l), 100.0);
    }

    #[test]
    fn le_tiroir_ferme_ne_prend_jamais_la_souris_et_la_palette_la_prend_toujours() {
        for tiroir in [false, true] {
            for dans in [false, true] {
                assert_eq!(destinataire_souris(true, tiroir, dans), DestSouris::Palette);
            }
        }
        for dans in [false, true] {
            assert_eq!(destinataire_souris(false, false, dans), DestSouris::Explorateur);
        }
    }
}
