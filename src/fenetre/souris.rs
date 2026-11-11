// Souris dans la fenêtre : cible sous le pointeur, géométrie de la page, et glisser-déposer (zones, dossier à ressort, menu de dépôt).

use crate::*;
/// Le point (DIP, coordonnées du panneau de largeur `l`) est dans la zone des fichiers : sous l'en-tête, au-dessus de la barre
/// d'état, de la fin du volet au bord droit du panneau. Les marges gauche et droite en font partie.
pub(crate) fn zone_fichiers(l: f32, h: f32, x: f32, y: f32) -> bool {
    x > largeur_volet() && x < l && y >= LISTE_Y && y < h - STATUT_H
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
    let avait = f.app.survol != Cible::Rien;
    f.app.survol = Cible::Rien;
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
        if dedans(&zone_recherche_haut(lt), x, y) {
            return Cible::RechercheHaut;
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
    if f.terminal_visible && app.emplacement != Emplacement::Reglages {
        let panneau = crate::rendu::zone_terminal_panneau(ll, h);
        if dedans(&panneau, x, y) {
            if dedans(&crate::rendu::zone_terminal_fermer(ll, h), x, y) {
                return Cible::TerminalFermer;
            }
            if dedans(&crate::rendu::zone_terminal_action(ll, h), x, y) {
                return Cible::TerminalExterne;
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
    if let Some(z) = app.zones.jours.iter().find(|z| dedans(&z.0, x, y)) {
        return Cible::Jour(z.1);
    }
    if y >= h - STATUT_H && x > largeur_volet() && x < lt && app.emplacement != Emplacement::Reglages {
        return Cible::Statut;
    }
    // Vide de la barre du chemin (après les segments)
    if y >= NAV_Y && y < NAV_Y + NAV_H && x >= chemin_x() - 10.0 && x < l - MARGE_LISTE && app.emplacement != Emplacement::Reglages {
        return Cible::Chemin;
    }
    // Toute la zone des fichiers répond, marge droite comprise : c'est le vide où l'on clique pour le menu du fond, le lasso, le dépôt
    let dans_liste = zone_fichiers(l, h, x, y);
    match app.affichage {
        Affichage::Grille if dans_liste => return case_grille(app, x, y).map_or(Cible::FondListe, Cible::Ligne),
        Affichage::Colonnes if dans_liste => {
            let (cx, cw) = colonnes_miller(l)[1];
            let i = ((y - LISTE_Y + app.defilement) / hauteur_ligne()) as usize;
            return if x >= cx && x < cx + cw && i < app.entrees.len() { Cible::Ligne(i) } else { Cible::FondListe };
        }
        Affichage::Grille | Affichage::Colonnes => return Cible::Rien,
        Affichage::Liste => {}
    }
    if x > largeur_volet() && x < l && y >= ENTETE_Y && y < ENTETE_Y + ENTETE_H {
        for (col, cx, w) in colonnes(l) {
            if x >= cx && x < cx + w {
                return Cible::Entete(col);
            }
        }
    }
    if dans_liste {
        // Les lignes s'arrêtent avant la marge droite ; le reste (marges gauche et droite, dessous la dernière ligne) est le fond
        let r = ((y - LISTE_Y + app.defilement) / hauteur_ligne()) as usize;
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
    if x <= page::gauche() - 6.0 || x >= page::droite(l) + 6.0 || y < page::PAGE_Y || y >= h - STATUT_H {
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
    (f.dip(hp as i32) - STATUT_H - page::PAGE_Y).max(0.0)
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
