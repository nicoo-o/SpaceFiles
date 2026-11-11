// Procédure de la fenêtre (WndProc) : un gros match sur les messages Win32.

use crate::*;
pub(crate) extern "system" fn procedure(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    unsafe {
        match msg {
            WM_NCCREATE => {
                let cs = &*(lparam.0 as *const CREATESTRUCTW);
                SetWindowLongPtrW(hwnd, GWLP_USERDATA, cs.lpCreateParams as isize);
                arbre::lier(hwnd);
                if let Some(f) = etat_de(hwnd) {
                    f.dpi = dpi_de(hwnd) as f32;
                }
                DefWindowProcW(hwnd, msg, wparam, lparam)
            }

            // Toute la fenêtre devient zone client : pas de cadre Windows, mais on garde
            // les styles normaux pour conserver l'ancrage (Snap), l'ombre et les animations.
            WM_NCCALCSIZE if wparam.0 != 0 => {
                if IsZoomed(hwnd).as_bool() {
                    // Agrandie, Windows déborde de l'écran de l'épaisseur du cadre : on la retire
                    let p = &mut *(lparam.0 as *mut NCCALCSIZE_PARAMS);
                    let dpi = GetDpiForWindow(hwnd);
                    let bord = GetSystemMetricsForDpi(SM_CXFRAME, dpi) + GetSystemMetricsForDpi(SM_CXPADDEDBORDER, dpi);
                    p.rgrc[0].left += bord;
                    p.rgrc[0].top += bord;
                    p.rgrc[0].right -= bord;
                    p.rgrc[0].bottom -= bord;
                }
                LRESULT(0)
            }

            WM_NCHITTEST => {
                let Some(f) = etat_de(hwnd) else { return DefWindowProcW(hwnd, msg, wparam, lparam) };
                let mut pt = POINT { x: x_lparam(lparam), y: y_lparam(lparam) };
                let _ = ScreenToClient(hwnd, &mut pt);
                let (l, h) = taille_client(hwnd);
                let bord = (BORD_REDIM * f.dpi / 96.0) as i32;
                if !IsZoomed(hwnd).as_bool() {
                    let gauche = pt.x < bord;
                    let droite = pt.x >= l as i32 - bord;
                    let haut = pt.y < bord;
                    let bas = pt.y >= h as i32 - bord;
                    let zone = match (gauche, droite, haut, bas) {
                        (true, _, true, _) => HTTOPLEFT,
                        (_, true, true, _) => HTTOPRIGHT,
                        (true, _, _, true) => HTBOTTOMLEFT,
                        (_, true, _, true) => HTBOTTOMRIGHT,
                        (true, _, _, _) => HTLEFT,
                        (_, true, _, _) => HTRIGHT,
                        (_, _, true, _) => HTTOP,
                        (_, _, _, true) => HTBOTTOM,
                        _ => HTNOWHERE,
                    };
                    if zone != HTNOWHERE {
                        return LRESULT(zone as isize);
                    }
                }
                if f.dip(pt.y) < HAUTEUR_TITRE && cible_sous(hwnd, f, pt.x, pt.y) == Cible::Rien {
                    return LRESULT(HTCAPTION as isize);
                }
                LRESULT(HTCLIENT as isize)
            }

            // Résultat d'une lecture : pour l'onglet qui l'a demandée (les disques, pour tous)
            WM_LECTURE => {
                let l = Box::from_raw(lparam.0 as *mut Lecture);
                // Fin d'une lecture de dossier : moment où `shared.json` est contrôlé (au plus une fois par seconde)
                // pour le mouvement réduit et le thème, avant que la lecture ne démarre l'apparition en points
                rafraichir_apparence(hwnd);
                if let Some(f) = etat_de(hwnd) {
                    if l.volet {
                        if let Ok(v) = &l.resultat {
                            for a in f.tous() {
                                a.disques = v.clone();
                            }
                            // Cloud détecté : registre et quelques dossiers, rapide
                            f.detectes = stockages::cloud_detecte(v);
                            f.rafraichir_lieux();
                            f.mesurer_etats(hwnd);
                        }
                    } else if let Some(a) = f.tous().find(|a| a.generation() == l.generation) {
                        a.recevoir(hwnd, *l);
                    }
                    animer(hwnd, f);
                }
                LRESULT(0)
            }

            // Lot 14b : états mesurés des lignes CLOUD et SERVEURS (le fil de mesure a posté son résultat)
            etats::WM_ETATS => {
                let m = Box::from_raw(lparam.0 as *mut Vec<(String, etats::Etat)>);
                if let Some(f) = etat_de(hwnd) {
                    f.recevoir_etats(*m);
                    let _ = InvalidateRect(hwnd, None, false);
                }
                LRESULT(0)
            }

            index::WM_INDEX => {
                if let Some(f) = etat_de(hwnd) {
                    for a in f.tous().filter(|a| matches!(a.emplacement, Emplacement::Globale { .. })) {
                        a.charger(hwnd);
                    }
                    let _ = InvalidateRect(hwnd, None, false);
                }
                LRESULT(0)
            }

            WM_GIT => {
                let b = Box::from_raw(lparam.0 as *mut (u64, Option<Git>));
                if let Some(f) = etat_de(hwnd) {
                    let (n, g) = *b;
                    if let Some(a) = f.tous().find(|a| a.possede_git(n)) {
                        a.recevoir_git(n, g);
                    }
                    let _ = InvalidateRect(hwnd, None, false);
                }
                LRESULT(0)
            }

            WM_FOND => {
                let m = Box::from_raw(lparam.0 as *mut MessageFond);
                if let Some(f) = etat_de(hwnd) {
                    if let Some(a) = f.tous().find(|a| a.possede_fond(m.numero)) {
                        a.recevoir_fond(*m);
                        a.ranger_telechargements(hwnd);
                        a.nettoyer_telechargements(hwnd);
                    }
                    let _ = InvalidateRect(hwnd, None, false);
                }
                LRESULT(0)
            }

            WM_OUTIL => {
                let r = Box::from_raw(lparam.0 as *mut Fini);
                if let Some(f) = etat_de(hwnd) {
                    if let Some(t) = &r.copier {
                        operations::copier_texte(hwnd, t);
                    }
                    if let Some(c) = &r.lancer {
                        systeme::lancer(hwnd, c);
                    }
                    f.app.annoncer(r.message.clone(), r.erreur);
                    sons::jouer(if r.erreur { sons::Son::Erreur } else { sons::Son::Fini });
                    let _ = InvalidateRect(hwnd, None, false);
                }
                LRESULT(0)
            }

            // La compression lancée par la palette a fini : message à part de WM_OUTIL, pour qu'une autre tâche (empreinte, OCR…) qui
            // finit pendant ce temps ne passe pas pour la sienne
            WM_PALETTE_TACHE => {
                // Pendant une modale l'appelant tient `&mut Fenetre` : la réponse est gardée et reposte à sa fin
                if retenir_palette(msg, lparam) {
                    return LRESULT(0);
                }
                let r = Box::from_raw(lparam.0 as *mut Fini);
                if let Some(f) = etat_de(hwnd) {
                    f.app.annoncer(r.message.clone(), r.erreur);
                    if let Some(res) = resume_tache(f, &r) {
                        if let Some(p) = f.palette.as_mut() {
                            p.finir(res);
                        }
                        f.remplir_palette();
                    }
                    sons::jouer(if r.erreur { sons::Son::Erreur } else { sons::Son::Fini });
                    let _ = InvalidateRect(hwnd, None, false);
                }
                LRESULT(0)
            }

            ocr::WM_OCR => {
                let r = ocr::recevoir(Box::from_raw(lparam.0 as *mut ocr::Resultat));
                if let Some(f) = etat_de(hwnd) {
                    texte_lu(hwnd, f, r);
                    let _ = InvalidateRect(hwnd, None, false);
                }
                LRESULT(0)
            }

            WM_VIGNETTE => {
                let b = Box::from_raw(lparam.0 as *mut (String, Option<Image>));
                if let Some(f) = etat_de(hwnd) {
                    let (c, img) = *b;
                    f.rendu.recevoir_vignette(c, img);
                    let _ = InvalidateRect(hwnd, None, false);
                }
                LRESULT(0)
            }

            WM_GALERIE => {
                let b = Box::from_raw(lparam.0 as *mut (u64, Option<Image>));
                if let Some(f) = etat_de(hwnd) {
                    let (n, img) = *b;
                    if let Some(g) = f.galerie.as_mut().filter(|g| g.numero == n) {
                        g.image = img;
                        g.charge = true;
                        let _ = InvalidateRect(hwnd, None, false);
                    }
                }
                LRESULT(0)
            }

            WM_CHANGEMENT => {
                if let Some(f) = etat_de(hwnd) {
                    for a in f.tous() {
                        a.changement(hwnd, wparam.0 as u64);
                    }
                }
                LRESULT(0)
            }

            // Le fil de l'abonnement au dossier Téléchargements a changé l'état des téléchargements (message posté, sans paramètre :
            // la lecture se fait dans `navigateur`, jamais de disque ici). Pixel et la grille suivent ; une marque NOUVEAU
            // changée repeint la liste, sinon seule la bande des pastilles.
            navigateur::WM_TELECHARGEMENT => {
                // Pendant une boucle modale (menu du Shell, glisser, boîte de choix) l'appelant tient `&mut Fenetre` : on ne le
                // reprend pas, le message est retenu et reposté à la fin de la modale (`GardeModale`)
                if retenir_telechargement() {
                    return LRESULT(0);
                }
                if let Some(f) = etat_de(hwnd) {
                    regler_activite(hwnd, f);
                    if navigateur::marques_modifiees() {
                        let _ = InvalidateRect(hwnd, None, false);
                    } else {
                        invalider_bande(hwnd, f);
                    }
                }
                LRESULT(0)
            }

            arbre::WM_ARBRE => {
                // SAFETY : la boîte vient de `arbre::lire_en_fond`, qui l'a donnée à ce message et ne la reprend plus
                let r = Box::from_raw(lparam.0 as *mut (String, Vec<(String, String)>));
                arbre::recevoir(r.0, r.1);
                let _ = InvalidateRect(hwnd, None, false);
                LRESULT(0)
            }

            WM_PALETTE_RECHERCHE => {
                if retenir_palette(msg, lparam) {
                    return LRESULT(0);
                }
                let b = Box::from_raw(lparam.0 as *mut (String, Option<Vec<palette::Trouve>>));
                if let Some(f) = etat_de(hwnd) {
                    let (requete, trouves) = *b;
                    recevoir_recherche(hwnd, f, requete, trouves);
                    let _ = InvalidateRect(hwnd, None, false);
                }
                LRESULT(0)
            }

            WM_PALETTE_LECTURE => {
                if retenir_palette(msg, lparam) {
                    return LRESULT(0);
                }
                let b = Box::from_raw(lparam.0 as *mut assistant::Lecture);
                if let Some(f) = etat_de(hwnd) {
                    recevoir_lecture(hwnd, f, *b);
                    let _ = InvalidateRect(hwnd, None, false);
                }
                LRESULT(0)
            }

            WM_PROGRES => {
                if let Some(f) = etat_de(hwnd) {
                    // Le numéro d'opération évite que la copie d'un onglet fasse avancer la barre d'un autre
                    for a in f.tous() {
                        if a.possede(lparam.0 as u64) {
                            a.progres(wparam.0);
                        }
                    }
                    // La palette à étapes montre la barre d'avancement de SON opération (numéro), pas celle d'un nettoyage ou d'un rangement
                    // de fond : toute la fenêtre est repeinte tant qu'elle attend
                    let numero = lparam.0 as u64;
                    let de_la_palette = f.apps().any(|a| a.est_palette(numero));
                    if let Some(p) = f.palette.as_mut().filter(|p| de_la_palette && p.etape == assistant::Etape::Progression) {
                        p.progression = (wparam.0 as f32 / 1000.0).clamp(0.0, 1.0);
                        let _ = InvalidateRect(hwnd, None, false);
                    } else {
                        // Sinon la progression ne change que les pastilles et la barre d'état
                        invalider_bande(hwnd, f);
                    }
                }
                LRESULT(0)
            }

            WM_OPERATION_FINIE => {
                let b = Box::from_raw(lparam.0 as *mut Bilan);
                if let Some(f) = etat_de(hwnd) {
                    // La palette qui attendait cette opération en montre le résultat (lu avant que le bilan ne soit consommé)
                    let resultat = resume_fin(f, &b);
                    let genre = b.genre;
                    // L'entrée d'historique que cette opération a notée en finissant : « Annuler » du résultat défera celle-là
                    let mut entree = None;
                    if let Some(a) = f.tous().find(|a| a.possede(b.numero)) {
                        a.fin_operation(hwnd, *b);
                        entree = a.palette_entree.take();
                    }
                    if let Some(r) = resultat {
                        let r = r.avec_entree(genre, entree);
                        if let Some(p) = f.palette.as_mut() {
                            p.finir(r);
                        }
                        f.remplir_palette();
                        lectures_palette(hwnd, f);
                    }
                    let _ = InvalidateRect(hwnd, None, false);
                }
                LRESULT(0)
            }

            WM_STOCKAGE => {
                let r = Box::from_raw(lparam.0 as *mut std::result::Result<String, String>);
                if let Some(f) = etat_de(hwnd) {
                    let en_cours = f.page.formulaire.as_ref().is_some_and(|fo| fo.en_cours);
                    if en_cours {
                        match *r {
                            Ok(adresse) => ajouter_stockage(f, adresse),
                            Err(e) => {
                                if let Some(fo) = &mut f.page.formulaire {
                                    fo.en_cours = false;
                                    fo.erreur = Some(e);
                                    fo.forcer = true;
                                    fo.focus = page::F_AJOUTER;
                                }
                                sons::jouer(sons::Son::Erreur);
                            }
                        }
                    }
                    let _ = InvalidateRect(hwnd, None, false);
                }
                LRESULT(0)
            }

            WM_APERCU => {
                let a = Box::from_raw(lparam.0 as *mut Apercu);
                if let Some(f) = etat_de(hwnd) {
                    if a.numero == f.apercu_numero {
                        f.apercu = Some(*a);
                        let _ = InvalidateRect(hwnd, None, false);
                    }
                }
                LRESULT(0)
            }

            WM_MOUSEMOVE => {
                if let Some(f) = etat_de(hwnd) {
                    if !f.suivi_souris {
                        let mut t = TRACKMOUSEEVENT {
                            cbSize: std::mem::size_of::<TRACKMOUSEEVENT>() as u32,
                            dwFlags: TME_LEAVE,
                            hwndTrack: hwnd,
                            dwHoverTime: 0,
                        };
                        let _ = TrackMouseEvent(&mut t);
                        f.suivi_souris = true;
                    }
                    let (x, y) = (f.dip(x_lparam(lparam)), f.dip(y_lparam(lparam)));
                    // Centre du reflet : le dernier WM_MOUSEMOVE, sans lissage. Pointeur retrouvé après une touche : le reflet reparaît
                    let pointeur_perdu = f.pointeur.is_none();
                    f.pointeur = Some((x, y));
                    // Les yeux de Pixel suivent le pointeur : repeinte de la bande seulement si le regard (arrondi) change
                    f.suivre_regard(hwnd);
                    if let Some(m) = f.menu.as_mut() {
                        let change = m.survoler(x, y);
                        if change || (pointeur_perdu && reflet::actif_maintenant()) {
                            let _ = InvalidateRect(hwnd, None, false);
                        } else {
                            invalider_reflet(hwnd, f);
                        }
                        return LRESULT(0);
                    }
                    // Palette ouverte : elle seule reçoit la souris (survol d'une ligne = ligne choisie), rien ne survole dessous.
                    // Tiroir Historique : même chose sous son rectangle. Un lasso en cours garde son capteur.
                    if f.app.lasso.is_none() {
                        let dest = destinataire(f, x, y);
                        if dest != DestSouris::Explorateur {
                            let change = if dest == DestSouris::Palette { survol_palette(f, x, y) } else { efface_survol(f) };
                            if change {
                                lectures_palette(hwnd, f);
                            }
                            if change || pointeur_perdu {
                                let _ = InvalidateRect(hwnd, None, false);
                                animer(hwnd, f);
                            }
                            return LRESULT(0);
                        }
                    }
                    // Bouton droit enfoncé sur des éléments choisis et souris qui part : glisser (menu au dépôt)
                    if let Some(d) = f.droit_depart {
                        let (x, y) = (x_lparam(lparam), y_lparam(lparam));
                        if wparam.0 & windows::Win32::System::SystemServices::MK_RBUTTON.0 as usize == 0 {
                            f.droit_depart = None;
                        } else if (x - d.x).abs() > GetSystemMetrics(SM_CXDRAG) || (y - d.y).abs() > GetSystemMetrics(SM_CYDRAG) {
                            f.droit_depart = None;
                            let chemins = f.app.chemins_choisis();
                            if !chemins.is_empty() {
                                let _ = coquille::glisser(hwnd, &chemins);
                            }
                            return LRESULT(0);
                        }
                    }
                    if f.app.lasso.is_some() {
                        let (x, y, _) = point_panneau(hwnd, f, x_lparam(lparam), y_lparam(lparam));
                        if f.app.etirer_lasso(x, y) {
                            let _ = InvalidateRect(hwnd, None, false);
                        }
                        return LRESULT(0);
                    }
                    let c = cible_sous(hwnd, f, x_lparam(lparam), y_lparam(lparam));
                    if c != f.app.survol {
                        // Le fondu et le ressort de survol sont suivis au dessin (`mouvement::Survols`), qui arme la minuterie
                        f.app.survol = c;
                        // Échap avait replié la pastille survolée : elle se redéploie au prochain survol
                        if !matches!(c, Cible::Pastille(_) | Cible::BoutonPastille(..)) {
                            f.pastille_masquee = false;
                        }
                        if let Cible::Ligne(i) = c {
                            poids_sonore(f, i);
                        }
                        let _ = InvalidateRect(hwnd, None, false);
                    } else if pointeur_perdu && reflet::actif_maintenant() {
                        let _ = InvalidateRect(hwnd, None, false);
                    } else {
                        // Même cible : seul le reflet bouge. Le rectangle de l'élément éclairé (ancien) et celui de l'élément
                        // éclairable (nouveau, y compris un bouton de la carte des conflits) décident seulement d'un WM_PAINT,
                        // l'image est redessinée en entier ; rien si aucun n'existe
                        invalider_reflet(hwnd, f);
                    }
                }
                LRESULT(0)
            }

            // Curseur : la main sur les lignes et les puces de la palette, la flèche sur sa carte ; ailleurs celui de la classe
            WM_SETCURSOR if (lparam.0 & 0xFFFF) as u32 == HTCLIENT => {
                if let Some(f) = etat_de(hwnd) {
                    if f.palette.is_some() {
                        let mut p = POINT::default();
                        let _ = GetCursorPos(&mut p);
                        let _ = ScreenToClient(hwnd, &mut p);
                        let cible = f.rendu.zones_palette.borrow().cible(f.dip(p.x), f.dip(p.y));
                        let curseur = LoadCursorW(None, if main_sur_palette(cible) { IDC_HAND } else { IDC_ARROW }).unwrap_or_default();
                        SetCursor(curseur);
                        return LRESULT(1);
                    }
                }
                DefWindowProcW(hwnd, msg, wparam, lparam)
            }

            WM_MOUSELEAVE => {
                if let Some(f) = etat_de(hwnd) {
                    f.suivi_souris = false;
                    f.pastille_masquee = false;
                    f.pointeur = None;
                    f.suivre_regard(hwnd);
                    f.survol_appui = false;
                    if f.app.survol != Cible::Rien {
                        f.app.survol = Cible::Rien;
                        let _ = InvalidateRect(hwnd, None, false);
                    } else {
                        invalider_reflet(hwnd, f);
                    }
                }
                LRESULT(0)
            }

            WM_LBUTTONDOWN => {
                let _ = SetFocus(hwnd);
                if let Some(f) = etat_de(hwnd) {
                    // Un clic déplace le focus : l'anneau clavier disparaît.
                    if f.focus_clavier {
                        f.focus_clavier = false;
                        let _ = InvalidateRect(hwnd, None, false);
                    }
                    // Appui : le chrome revient tout de suite à sa place, même avant le relâchement.
                    f.survol_appui = true;
                    // Menu ouvert : un clic dedans choisit, ailleurs il ferme (et ne fait rien d'autre)
                    let (x, y) = (f.dip(x_lparam(lparam)), f.dip(y_lparam(lparam)));
                    if let Some(m) = f.menu.as_mut() {
                        let action = m.cliquer(x, y);
                        f.avaler_clic = true;
                        agir_menu(hwnd, f, action);
                        return LRESULT(0);
                    }
                    // Carte des conflits : seuls ses boutons répondent
                    if f.tous().any(|a| a.conflits.is_some()) {
                        let (x, y) = (f.dip(x_lparam(lparam)), f.dip(y_lparam(lparam)));
                        let choix = f.rendu.zones_conflit.borrow().iter().find(|(r, _)| x >= r.left && x <= r.right && y >= r.top && y <= r.bottom).map(|z| z.1);
                        let maj = wparam.0 & 0x0004 != 0; // MK_SHIFT
                        if let (Some(c), Some(a)) = (choix, f.tous().find(|a| a.conflits.is_some())) {
                            choisir_conflit(hwnd, a, c, maj);
                        }
                        synchroniser_palette(f);
                        // Le relâchement ne doit rien faire sur ce qui était dessous (une pastille, par exemple)
                        f.avaler_clic = true;
                        animer(hwnd, f);
                        return LRESULT(0);
                    }
                    // Palette : un clic sur une puce change de catégorie, sur une ligne lance la commande, hors de la carte la ferme
                    if f.palette.is_some() {
                        let (x, y) = (f.dip(x_lparam(lparam)), f.dip(y_lparam(lparam)));
                        let cible = f.rendu.zones_palette.borrow().cible(x, y);
                        f.avaler_clic = true;
                        match cible {
                            rendu::CiblePalette::Puce(rang) => {
                                if f.palette.as_mut().filter(|p| !p.lance).is_some_and(|p| p.aller_etape(rang)) {
                                    f.remplir_palette();
                                    lectures_palette(hwnd, f);
                                }
                            }
                            rendu::CiblePalette::Ligne(i) => {
                                // Le rang vient de la dernière image ; la liste a pu changer depuis (des résultats arrivent). On retrouve la
                                // ligne vue par sa clé : si elle n'y est plus, le clic est perdu plutôt que d'agir sur une autre ligne.
                                let vue = f.rendu.zones_palette.borrow().cle_de(i).map(str::to_string);
                                if let Some(p) = f.palette.as_mut().filter(|p| !p.lance) {
                                    if let Some(k) = vue.and_then(|c| p.items.iter().position(|it| it.cle() == c)) {
                                        p.choix = k;
                                        lancer_palette(hwnd, f, false);
                                    }
                                }
                            }
                            rendu::CiblePalette::Carte => {}
                            rendu::CiblePalette::Dehors => {
                                if f.palette.as_ref().is_some_and(|p| !p.lance) {
                                    f.palette = None;
                                }
                            }
                        }
                        animer(hwnd, f);
                        return LRESULT(0);
                    }
                    // Tiroir Historique : « Annuler » défait l'action de son entrée, ailleurs dans le tiroir le clic ne traverse pas
                    if f.historique {
                        let (x, y) = (f.dip(x_lparam(lparam)), f.dip(y_lparam(lparam)));
                        let cible = f.rendu.zones_historique.borrow().cible(x, y);
                        match cible {
                            rendu::CibleHistorique::Annuler(id) => {
                                f.avaler_clic = true;
                                f.app.defaire_entree(hwnd, id);
                                let _ = InvalidateRect(hwnd, None, false);
                                animer(hwnd, f);
                                return LRESULT(0);
                            }
                            rendu::CibleHistorique::Tiroir => {
                                f.avaler_clic = true;
                                return LRESULT(0);
                            }
                            rendu::CibleHistorique::Dehors => {}
                        }
                    }
                    // Un clic ferme le coup d'œil (le renommage par lot se ferme avec Échap)
                    if f.galerie.take().is_some() || f.lot.is_some() {
                        f.avaler_clic = true;
                        let _ = InvalidateRect(hwnd, None, false);
                        return LRESULT(0);
                    }
                    let mut cible = cible_sous(hwnd, f, x_lparam(lparam), y_lparam(lparam));
                    // Un clic ailleurs abandonne le chemin en cours de saisie
                    if f.app.saisie_chemin.is_some() && cible != Cible::Chemin {
                        f.app.saisie_chemin = None;
                    }
                    // Clic dans l'autre panneau : il prend le focus, puis le clic s'y applique
                    if cible == Cible::AutrePanneau {
                        f.basculer_panneau();
                        cible = cible_sous(hwnd, f, x_lparam(lparam), y_lparam(lparam));
                    }
                    // Un clic ailleurs que sur la ligne en cours de renommage valide le nouveau nom
                    if let Some(ed) = &f.app.edition {
                        if cible == Cible::Ligne(ed.index) {
                            return LRESULT(0);
                        }
                        f.app.valider_renommage();
                    }
                    let ctrl = wparam.0 & 0x0008 != 0; // MK_CONTROL
                    let maj = wparam.0 & 0x0004 != 0; // MK_SHIFT
                    // En-tête de section : le chevron replie, le reste choisit toute la section
                    if let Cible::Section(s) = cible {
                        let (px, _, lp) = point_panneau(hwnd, f, x_lparam(lparam), y_lparam(lparam));
                        if px < colonnes(lp)[0].1 + 26.0 {
                            f.app.replier(s);
                        } else {
                            f.app.choisir_section(s, ctrl || maj);
                        }
                        sons::jouer(sons::Son::Tic);
                        let _ = InvalidateRect(hwnd, None, false);
                        return LRESULT(0);
                    }
                    // Lasso : depuis le vide de la liste, ou (vue liste) depuis n'importe quel endroit d'une ligne
                    // hors de son icône et de son nom, comme dans l'Explorateur (une liste pleine n'a pas de vide)
                    let (px, py, lp) = point_panneau(hwnd, f, x_lparam(lparam), y_lparam(lparam));
                    let hors_nom = |i: usize| {
                        f.app.affichage == Affichage::Liste
                            && f.app.entrees.get(i).is_some_and(|e| px > colonnes(lp)[0].1 + 28.0 + f.rendu.largeur_nom(&e.nom) + 16.0)
                    };
                    let lasso = match cible {
                        Cible::FondListe => f.app.affichage != Affichage::Colonnes,
                        Cible::Ligne(i) => hors_nom(i) && !ctrl && !maj,
                        _ => false,
                    };
                    // Case à cocher (à la place de l'icône au survol) : ajoute ou retire l'élément, sans Ctrl
                    if let Cible::Ligne(i) = cible {
                        let case = match f.app.affichage {
                            Affichage::Liste => {
                                let x0 = colonnes(lp)[0].1;
                                px >= x0 - 4.0 && px <= x0 + 24.0
                            }
                            Affichage::Grille => {
                                let (par, hr) = f.app.pas();
                                let x = grille_x0() + (i % par) as f32 * (f.app.cote_grille + app::GRILLE_ECART);
                                let y = LISTE_Y + (i / par) as f32 * hr - f.app.defilement;
                                px >= x && px <= x + 26.0 && py >= y + 6.0 && py <= y + 32.0
                            }
                            Affichage::Colonnes => false,
                        };
                        if case && !ctrl && !maj {
                            f.app.basculer(i);
                            let _ = InvalidateRect(hwnd, None, false);
                            return LRESULT(0);
                        }
                    }
                    // Clic sur une ligne déjà choisie : on garde la sélection, qu'on va peut-être glisser
                    let mut differe = None;
                    match cible {
                        Cible::Ligne(i) if ctrl => f.app.basculer(i),
                        Cible::Ligne(i) if maj => f.app.etendre(i),
                        Cible::Ligne(i) if f.app.marques.contains(&i) => {
                            f.app.selection = Some(i);
                            differe = Some(i);
                        }
                        Cible::Ligne(i) => f.app.choisir_seul(i),
                        Cible::FondListe if !ctrl && !maj => f.app.rien_choisir(),
                        _ => {}
                    }
                    let _ = InvalidateRect(hwnd, None, false);
                    if lasso {
                        // Clic simple dans le vide d'une ligne déjà choisie : elle seule reste choisie
                        if let Some(i) = differe {
                            f.app.choisir_seul(i);
                        }
                        f.app.commencer_lasso(px, py, ctrl || maj);
                        SetCapture(hwnd);
                        return LRESULT(0);
                    }

                    // Glisser : vers une autre application ou un dossier de la liste
                    if let Cible::Ligne(i) = cible {
                        if f.app.marques.contains(&i) {
                            let _ = UpdateWindow(hwnd);
                            let mut pt = POINT { x: x_lparam(lparam), y: y_lparam(lparam) };
                            let _ = ClientToScreen(hwnd, &mut pt);
                            if DragDetect(hwnd, pt).as_bool() {
                                let chemins = f.app.chemins_choisis();
                                if !chemins.is_empty() {
                                    let _ = coquille::glisser(hwnd, &chemins);
                                }
                            } else {
                                if let Some(i) = differe {
                                    f.app.choisir_seul(i);
                                }
                                // Réglage « ouvrir en un clic »
                                if !ctrl && !maj && reglages::actuels().un_clic {
                                    f.app.ouvrir(hwnd, i);
                                    animer(hwnd, f);
                                }
                            }
                            if let Some(f) = etat_de(hwnd) {
                                f.app.survol = Cible::Rien;
                            }
                            let _ = InvalidateRect(hwnd, None, false);
                        }
                    }
                }
                LRESULT(0)
            }

            WM_RBUTTONDOWN => {
                let _ = SetFocus(hwnd);
                if let Some(f) = etat_de(hwnd) {
                    if f.focus_clavier {
                        f.focus_clavier = false;
                        let _ = InvalidateRect(hwnd, None, false);
                    }
                    // Palette ouverte, ou tiroir Historique sous le pointeur : le clic droit ne traverse pas
                    if !souris_pour_explorateur(f, lparam) {
                        return LRESULT(0);
                    }
                    fermer_menu(f);
                    if f.app.edition.is_some() {
                        f.app.valider_renommage();
                    }
                    f.droit_depart = None;
                    match cible_sous(hwnd, f, x_lparam(lparam), y_lparam(lparam)) {
                        Cible::Ligne(i) if !f.app.marques.contains(&i) => f.app.choisir_seul(i),
                        Cible::Ligne(i) => f.app.selection = Some(i),
                        Cible::FondListe => f.app.rien_choisir(),
                        _ => {}
                    }
                    if matches!(cible_sous(hwnd, f, x_lparam(lparam), y_lparam(lparam)), Cible::Ligne(_)) && !f.app.sans_fichiers() {
                        f.droit_depart = Some(POINT { x: x_lparam(lparam), y: y_lparam(lparam) });
                    }
                    let _ = InvalidateRect(hwnd, None, false);
                    let _ = UpdateWindow(hwnd);
                }
                LRESULT(0)
            }

            // Clic droit relâché, Maj+F10 ou touche Menu
            WM_CONTEXTMENU => {
                let Some(f) = etat_de(hwnd) else { return LRESULT(0) };
                let (x, y) = (x_lparam(lparam), y_lparam(lparam));
                // Palette ouverte : pas de menu de l'explorateur, ni à la souris ni au clavier
                if f.palette.is_some() {
                    return LRESULT(0);
                }
                if !(x == -1 && y == -1) {
                    let mut c = POINT { x, y };
                    let _ = ScreenToClient(hwnd, &mut c);
                    if destinataire(f, f.dip(c.x), f.dip(c.y)) != DestSouris::Explorateur {
                        return LRESULT(0);
                    }
                }
                if x == -1 && y == -1 {
                    // Au clavier : sous la ligne qui a le focus
                    let (cible, ligne) = match f.app.selection {
                        Some(i) if f.app.marques.contains(&i) => (Cible::Ligne(i), f.app.rang_de(i) as f32 + 1.0),
                        _ => (Cible::FondListe, 0.0),
                    };
                    let echelle = f.dpi / 96.0;
                    let (lt, _) = taille_client(hwnd);
                    let decalage = if f.second.is_some() && f.focus_droite { largeur_panneau(largeur_liste(f.dip(lt as i32), f.apercu_visible), true) - largeur_volet() } else { 0.0 };
                    let mut pt = POINT {
                        x: ((largeur_volet() + 60.0 + decalage) * echelle) as i32,
                        y: ((LISTE_Y + ligne * hauteur_ligne() - f.app.defilement) * echelle) as i32,
                    };
                    let _ = ClientToScreen(hwnd, &mut pt);
                    menu_contextuel(hwnd, cible, pt.x, pt.y);
                } else {
                    let mut c = POINT { x, y };
                    let _ = ScreenToClient(hwnd, &mut c);
                    let cible = cible_sous(hwnd, f, c.x, c.y);
                    match cible {
                        Cible::Ligne(_)
                        | Cible::FondListe
                        | Cible::Favori(_)
                        | Cible::Epingle(_)
                        | Cible::Disque(_)
                        | Cible::Segment(_)
                        | Cible::Nuage(_)
                        | Cible::Serveur(_)
                        | Cible::Groupe(_)
                        | Cible::Arbre(_)
                        | Cible::Entete(_) => menu_contextuel(hwnd, cible, x, y),
                        _ => return DefWindowProcW(hwnd, msg, wparam, lparam),
                    }
                }
                LRESULT(0)
            }

            // Sous-menus du Shell (Ouvrir avec, Envoyer vers…) pendant que le menu clic droit est ouvert
            WM_INITMENUPOPUP | WM_DRAWITEM | WM_MEASUREITEM | WM_MENUCHAR => {
                coquille::message_menu(msg, wparam, lparam).unwrap_or_else(|| DefWindowProcW(hwnd, msg, wparam, lparam))
            }

            WM_LBUTTONDBLCLK => {
                if let Some(f) = etat_de(hwnd) {
                    if !souris_pour_explorateur(f, lparam) {
                        return LRESULT(0);
                    }
                    match cible_sous(hwnd, f, x_lparam(lparam), y_lparam(lparam)) {
                        Cible::Ligne(i) => f.app.ouvrir(hwnd, i),
                        // Double-clic dans le vide de la barre d'onglets : nouvel onglet
                        Cible::Rien if f.dip(y_lparam(lparam)) < HAUTEUR_TITRE => {}
                        _ => {}
                    }
                    let _ = InvalidateRect(hwnd, None, false);
                }
                LRESULT(0)
            }

            WM_LBUTTONUP => {
                if let Some(f) = etat_de(hwnd) {
                    f.survol_appui = false;
                    // Relâchement sur un élément chrome encore survolé : il remonte de 2 DIP au ressort (suivi au dessin)
                    if f.app.survol.est_chrome() {
                        let _ = InvalidateRect(hwnd, None, false);
                    }
                    if std::mem::take(&mut f.avaler_clic) {
                        return LRESULT(0);
                    }
                    if f.app.lasso.is_some() {
                        f.app.finir_lasso();
                        let _ = ReleaseCapture();
                        let _ = InvalidateRect(hwnd, None, false);
                        return LRESULT(0);
                    }
                    // Le relâchement d'un clic fait sur la palette ou le tiroir ne touche pas à ce qui est dessous
                    if !souris_pour_explorateur(f, lparam) {
                        return LRESULT(0);
                    }
                    match cible_sous(hwnd, f, x_lparam(lparam), y_lparam(lparam)) {
                        Cible::Reduire => {
                            let _ = ShowWindow(hwnd, SW_MINIMIZE);
                        }
                        Cible::Agrandir => {
                            let _ = ShowWindow(hwnd, if IsZoomed(hwnd).as_bool() { SW_RESTORE } else { SW_MAXIMIZE });
                        }
                        Cible::Fermer => {
                            let _ = PostMessageW(hwnd, WM_CLOSE, WPARAM(0), LPARAM(0));
                        }
                        Cible::Precedent => f.app.precedent(hwnd),
                        Cible::Suivant => f.app.suivant(hwnd),
                        Cible::Parent => f.app.parent(hwnd),
                        Cible::Segment(i) => {
                            if let Some((_, emp, _)) = f.app.zones.segments.get(i).cloned() {
                                f.app.naviguer(hwnd, emp);
                            }
                        }
                        Cible::Entete(col) => f.app.trier_par(col),
                        Cible::Favori(i) => f.app.ouvrir_favori(hwnd, i),
                        Cible::Epingle(i) => f.app.ouvrir_epingle(hwnd, i),
                        Cible::Disque(i) => f.app.ouvrir_disque(hwnd, i),
                        Cible::Onglet(k) => f.aller_onglet(k),
                        Cible::FermerOnglet(k) => f.fermer_onglet(hwnd, k),
                        Cible::NouvelOnglet => {
                            let emp = f.app.emplacement.clone();
                            f.nouvel_onglet(hwnd, emp);
                        }
                        Cible::Nuage(_) | Cible::Serveur(_) => {
                            let c = cible_sous(hwnd, f, x_lparam(lparam), y_lparam(lparam));
                            f.app.ouvrir_lieu(hwnd, c);
                        }
                        Cible::Reglages => ouvrir_reglages(hwnd, f),
                        Cible::ReglagesFermer => quitter_reglages(hwnd, f),
                        // Un clic sur le rail affiche la page de la catégorie (il ne fait plus défiler)
                        Cible::ReglagesNav(k) => {
                            let lignes = f.lignes();
                            f.page.aller_categorie(&lignes, k);
                        }
                        Cible::CePc => f.app.naviguer(hwnd, Emplacement::CePC),
                        Cible::RechercheHaut => executer(hwnd, f, Commande::RechercheGlobale),
                        Cible::EssentielHaut => executer(hwnd, f, Commande::Essentiel),
                        Cible::Terminal => executer(hwnd, f, Commande::Terminal),
                        Cible::TerminalFermer => f.terminal_visible = false,
                        Cible::TerminalExterne => crate::commandes::terminal_externe(f),
                        Cible::TerminalPanneau => {},
                        Cible::Groupe(k) => {
                            if let Some(g) = f.app.groupes.get(k).map(|g| g.nom.clone()) {
                                f.app.naviguer(hwnd, Emplacement::Groupe(g));
                            }
                        }
                        Cible::Pastille(i) => f.pastille_fixee = if f.pastille_fixee == Some(i) { None } else { Some(i) },
                        Cible::BoutonPastille(i, b) => f.clic_bouton_pastille(hwnd, i, b),
                        Cible::PlusPastilles => {}
                        Cible::Chemin => f.app.commencer_saisie_chemin(),
                        Cible::Section(_) => {}
                        Cible::TitreVolet(k) => reglages::modifier(|r| r.volet_replie ^= 1 << k),
                        Cible::Arbre(i) => clic_arbre(hwnd, f, i, f.dip(x_lparam(lparam))),
                        Cible::Jour(j) => f.app.choisir_jour(j),
                        Cible::Note => ouvrir_note(f),
                        Cible::Puce(k) => f.app.ajouter_jeton(crate::rendu::PUCES_FILTRE[k.min(crate::rendu::PUCES_FILTRE.len() - 1)]),
                        Cible::Corbeille => f.app.naviguer(hwnd, Emplacement::Corbeille),
                        Cible::Statut => {
                            menu_statut(hwnd, f, f.dip(x_lparam(lparam)), f.dip(y_lparam(lparam)));
                            return LRESULT(0);
                        }
                        Cible::Outil(o) => {
                            clic_outil(hwnd, f, o);
                            let _ = InvalidateRect(hwnd, None, false);
                            return LRESULT(0);
                        }
                        Cible::Densite(k) => {
                            let confort = k == 0;
                            let lignes = u32::from(confort);
                            if let Some(d) = f.app.dossier().map(str::to_string) {
                                reglages::memoriser_disposition(&d, reglages::COLONNES.load(Ordering::Relaxed), lignes);
                            } else {
                                reglages::modifier(|r| r.taille_lignes = lignes);
                            }
                            f.app.annoncer(if confort { "DENSITE CONFORT · 40 DIP" } else { "DENSITE COMPACT · 30 DIP" }, false);
                        }
                        Cible::Reglage(k) => {
                            f.page.focus = k;
                            // Contrôle segmenté : le clic choisit le segment visé, pas le suivant
                            let (largeur, _) = taille_client(hwnd);
                            let (cx, cy) = (f.dip(x_lparam(lparam)), f.dip(y_lparam(lparam)));
                            match segment_vise(f, f.dip(largeur as i32), k, cx, cy) {
                                Some(s) => choisir_segment(hwnd, f, k, s),
                                None => activer_reglage(hwnd, f, k, 0),
                            }
                        }
                        Cible::Champ(k) => {
                            if let Some(fo) = &mut f.page.formulaire {
                                match k {
                                    page::F_GENRE => fo.changer_sorte(1),
                                    page::F_AJOUTER => soumettre(hwnd, f),
                                    page::F_ANNULER => fermer_formulaire(f),
                                    _ => fo.focus = k,
                                }
                            }
                        }
                        Cible::Ligne(_) | Cible::FondListe | Cible::Rien | Cible::AutrePanneau => {}
                    }
                    let _ = InvalidateRect(hwnd, None, false);
                }
                LRESULT(0)
            }

            // Clic du milieu : ouvrir un dossier dans un nouvel onglet, fermer un onglet
            WM_MBUTTONUP => {
                if let Some(f) = etat_de(hwnd) {
                    if !souris_pour_explorateur(f, lparam) {
                        return LRESULT(0);
                    }
                    match cible_sous(hwnd, f, x_lparam(lparam), y_lparam(lparam)) {
                        Cible::Onglet(k) | Cible::FermerOnglet(k) => f.fermer_onglet(hwnd, k),
                        c @ (Cible::Ligne(_) | Cible::Favori(_) | Cible::Epingle(_) | Cible::Disque(_) | Cible::Segment(_) | Cible::Nuage(_) | Cible::Serveur(_) | Cible::Groupe(_) | Cible::CePc | Cible::Arbre(_)) => {
                            let emp = match c {
                                Cible::Groupe(k) => f.app.groupes.get(k).map(|g| Emplacement::Groupe(g.nom.clone())),
                                Cible::CePc => Some(Emplacement::CePC),
                                Cible::Segment(i) => f.app.zones.segments.get(i).map(|s| s.1.clone()),
                                Cible::Nuage(i) => f.app.nuages.get(i).map(|l| l.emplacement.clone()),
                                Cible::Serveur(i) => f.app.serveurs.get(i).map(|l| l.emplacement.clone()),
                                _ => f.app.chemin_cible(c).map(|ch| Emplacement::dossier(&ch)),
                            };
                            if let Some(emp) = emp {
                                f.nouvel_onglet(hwnd, emp);
                            }
                        }
                        _ => {}
                    }
                    let _ = InvalidateRect(hwnd, None, false);
                }
                LRESULT(0)
            }

            // Boutons latéraux de la souris : précédent / suivant
            WM_XBUTTONUP => {
                if let Some(f) = etat_de(hwnd) {
                    if !souris_pour_explorateur(f, lparam) {
                        return LRESULT(1);
                    }
                    if (wparam.0 >> 16) & 0xFFFF == 1 {
                        f.app.precedent(hwnd);
                    } else {
                        f.app.suivant(hwnd);
                    }
                }
                LRESULT(1)
            }

            WM_MOUSEWHEEL => {
                if let Some(f) = etat_de(hwnd) {
                    let crans = ((wparam.0 >> 16) & 0xFFFF) as i16 as f32 / 120.0;
                    let ctrl = wparam.0 & 0x0008 != 0; // MK_CONTROL
                    if let Some(l) = &mut f.lot {
                        l.defilement = (l.defilement as i64 - (crans * 3.0) as i64).clamp(0, l.noms.len().saturating_sub(1) as i64) as usize;
                    } else if let Some(p) = &mut f.palette {
                        p.deplacer(-(crans as i64));
                    } else if f.historique && {
                        let mut p = POINT { x: x_lparam(lparam), y: y_lparam(lparam) };
                        let _ = ScreenToClient(hwnd, &mut p);
                        f.rendu.zones_historique.borrow().cible(f.dip(p.x), f.dip(p.y)) != rendu::CibleHistorique::Dehors
                    } {
                        // Molette sur le tiroir Historique : il défile, la liste dessous reste en place
                        let (l, h) = taille_client(hwnd);
                        let zone = rendu::zone_tiroir(f.dip(l as i32), f.dip(h as i32));
                        let entrees = journal_historique::instantane();
                        let max = rendu::historique_defilement_max(entrees.len(), &zone, entrees.iter().any(|e| e.id == 0));
                        f.historique_defilement = (f.historique_defilement - crans * rendu::HIST_RANG).clamp(0.0, max);
                        let _ = InvalidateRect(hwnd, None, false);
                    } else if ctrl && f.app.affichage == Affichage::Grille {
                        // Ctrl+molette : taille des vignettes
                        f.app.zoomer(crans);
                    } else if ctrl && f.app.affichage == Affichage::Liste {
                        f.app.choisir_vue(hwnd, Affichage::Grille);
                    } else if reglages::ARBRE.load(Ordering::Relaxed) && {
                        let mut p = POINT { x: x_lparam(lparam), y: y_lparam(lparam) };
                        let _ = ScreenToClient(hwnd, &mut p);
                        f.dip(p.x) < largeur_volet() && f.dip(p.y) > HAUTEUR_TITRE
                    } {
                        defiler_volet(hwnd, f, -crans * 64.0);
                    } else if f.app.emplacement == Emplacement::Reglages {
                        let (lignes, hauteur) = (f.lignes(), hauteur_page(hwnd, f));
                        f.page.defiler(&lignes, -crans * 2.0 * page::LIGNE_H, hauteur);
                    } else {
                        f.app.defiler(-crans * 3.0 * hauteur_ligne());
                    }
                    animer(hwnd, f);
                }
                LRESULT(0)
            }

            WM_TIMER if wparam.0 == MINUTERIE_DEFILEMENT => minuteries::timer_defilement(hwnd),

            minuteries::WM_IMAGE_ANIMATION => minuteries::image_animation(hwnd),

            WM_TIMER if wparam.0 == MINUTERIE_ANNONCE => minuteries::timer_annonce(hwnd),

            WM_TIMER if wparam.0 == MINUTERIE_APERCU => minuteries::timer_apercu(hwnd),

            WM_TIMER if wparam.0 == MINUTERIE_RESSORT => minuteries::timer_ressort(hwnd),

            WM_TIMER if wparam.0 == MINUTERIE_SESSION => minuteries::timer_session(hwnd),

            WM_TIMER if wparam.0 == MINUTERIE_ACTIVITE => minuteries::timer_activite(hwnd),

            WM_TIMER if wparam.0 == MINUTERIE_CLIGNEMENT => minuteries::timer_clignement(hwnd),

            WM_TIMER if wparam.0 == MINUTERIE_PALETTE_CLIGNE => minuteries::timer_clignement_palette(hwnd),

            WM_TIMER if wparam.0 == MINUTERIE_PALETTE_LANCE => minuteries::timer_lancement_palette(hwnd),

            // Windows s'arrête : la session s'écrit tout de suite, sans attendre la minuterie
            WM_QUERYENDSESSION => {
                if let Some(f) = etat_de(hwnd) {
                    sauver_session(f);
                }
                LRESULT(1)
            }

            // Arrêt confirmé : la session est enregistrée proprement, le marqueur part (le processus peut être tué sans WM_DESTROY)
            WM_ENDSESSION => {
                if wparam.0 != 0 {
                    if let Some(f) = etat_de(hwnd) {
                        sauver_session(f);
                        lever_marqueur();
                    }
                }
                LRESULT(0)
            }

            // Mode essai : le relâchement d'un modificateur posté (voir `noter_modificateur_essai`)
            WM_KEYUP | WM_SYSKEYUP => {
                noter_modificateur_essai(wparam.0 as u16, false);
                DefWindowProcW(hwnd, msg, wparam, lparam)
            }

            WM_KEYDOWN | WM_SYSKEYDOWN => {
                let Some(f) = etat_de(hwnd) else { return LRESULT(0) };
                // Une touche enlève le reflet : la position du curseur ne dit rien d'un élément qui a le focus clavier
                // sauf les modificateurs seuls (Maj, Ctrl, Alt et leurs variantes gauche / droite : 0x10 à 0x12, 0xA0 à 0xA5), qui ne
                // touchent à aucun élément
                let modificateur = matches!(wparam.0 as u16, 0x10..=0x12 | 0xA0..=0xA5);
                let nouveau_focus = !modificateur && !f.focus_clavier;
                if nouveau_focus {
                    f.focus_clavier = true;
                }
                if !modificateur && f.pointeur.take().is_some() {
                    invalider_reflet(hwnd, f);
                    // Plus de pointeur : les yeux de Pixel reviennent droit devant
                    f.suivre_regard(hwnd);
                }
                if nouveau_focus {
                    let _ = InvalidateRect(hwnd, None, false);
                }
                noter_modificateur_essai(wparam.0 as u16, true);
                let vk = VIRTUAL_KEY(wparam.0 as u16);
                let alt = msg == WM_SYSKEYDOWN || touche(VK_MENU);
                let ctrl = touche(VK_CONTROL);
                let maj = touche(VK_SHIFT);

                // Fenêtres posées par-dessus : elles prennent toutes les touches
                let mut pris = false;
                if let Some(a) = f.tous().find(|a| a.conflits.is_some()) {
                    touche_conflit(hwnd, a, vk, maj);
                    pris = true;
                }
                if pris {
                    // Échap sur la carte abandonne l'opération : la palette qui l'attendait le dit au lieu de rester en progression
                    synchroniser_palette(f);
                }
                if !pris {
                    if let Some(m) = f.menu.as_mut() {
                        let action = m.touche(vk.0);
                        agir_menu(hwnd, f, action);
                        pris = true;
                    }
                }
                f.avaler_caractere = pris;
                if pris {
                    animer(hwnd, f);
                    return LRESULT(0);
                }
                if f.palette.is_some() {
                    touche_palette(hwnd, f, vk, ctrl, maj);
                    animer(hwnd, f);
                    return LRESULT(0);
                }
                if f.lot.is_some() {
                    touche_lot(hwnd, f, vk, ctrl, maj);
                    animer(hwnd, f);
                    return LRESULT(0);
                }
                if f.galerie.is_some() {
                    touche_galerie(hwnd, f, vk);
                    animer(hwnd, f);
                    return LRESULT(0);
                }
                // Chemin en cours de saisie : Entrée y va, Tab complète, Échap annule
                if f.app.saisie_chemin.is_some() {
                    match vk {
                        VK_ESCAPE => f.app.saisie_chemin = None,
                        VK_RETURN => {
                            if let Some(q) = f.app.valider_chemin(hwnd) {
                                index::demarrer();
                                f.palette = Some(Palette::avec_texte(&q));
                                f.remplir_palette();
                            }
                        }
                        VK_TAB => f.app.completer_chemin(maj),
                        _ => {
                            if let Some(ed) = &mut f.app.saisie_chemin {
                                editer(hwnd, ed, vk, ctrl, maj);
                            }
                        }
                    }
                    animer(hwnd, f);
                    return LRESULT(0);
                }
                // Panneau Historique (lot 15B) : Échap rentre le tiroir avant tout autre geste, sauf pendant une saisie ou une fenêtre modale
                // (déjà traitées plus haut)
                if vk == VK_ESCAPE && f.historique && !f.modale() && f.app.edition.is_none() && f.page.formulaire.is_none() {
                    f.historique = false;
                    let _ = InvalidateRect(hwnd, None, false);
                    animer(hwnd, f);
                    return LRESULT(0);
                }
                // Le panneau terminal est une couche de la liste : Échap le ferme avant les autres gestes.
                if vk == VK_ESCAPE
                    && f.terminal_visible
                    && f.app.emplacement != Emplacement::Reglages
                    && !f.modale()
                    && f.app.edition.is_none()
                    && f.page.formulaire.is_none()
                {
                    f.terminal_visible = false;
                    let _ = InvalidateRect(hwnd, None, false);
                    animer(hwnd, f);
                    return LRESULT(0);
                }
                if f.app.arbre_focus && !ctrl && !alt && f.app.edition.is_none() && touche_arbre(hwnd, f, vk) {
                    animer(hwnd, f);
                    return LRESULT(0);
                }
                // Raccourcis choisis dans les réglages : la nouvelle touche lance la commande, l'ancienne ne fait plus rien
                if f.app.edition.is_none() {
                    if let Some(combo) = combinaison(vk, ctrl, alt, maj) {
                        if let Some(c) = reglages::commande_de_touche(&combo).and_then(|n| palette::commande_nommee(&n)) {
                            executer(hwnd, f, c);
                            animer(hwnd, f);
                            return LRESULT(0);
                        }
                        let abandonnee = palette::COMMANDES.iter().any(|x| x.2 == combo && reglages::touche_de_commande(&format!("{:?}", x.0).to_uppercase()).is_some());
                        if abandonnee {
                            return LRESULT(0);
                        }
                    }
                }
                // Ctrl+L, Alt+D : taper un chemin (comme dans l'Explorateur)
                if (vk == VK_L && ctrl && !alt && !maj) || (vk == VK_D && alt && !ctrl) {
                    f.app.commencer_saisie_chemin();
                    animer(hwnd, f);
                    return LRESULT(0);
                }

                // Onglets, aperçu, sons : valables partout
                match vk {
                    _ if bascule_palette(vk, ctrl, alt, maj) => {
                        ouvrir_palette(f);
                        animer(hwnd, f);
                        return LRESULT(0);
                    }
                    VK_F6 if !maj => {
                        f.double_panneau(hwnd);
                        animer(hwnd, f);
                        return LRESULT(0);
                    }
                    // Dans les réglages, Tab change de catégorie (touche_page) : il ne bascule pas le double panneau
                    VK_TAB if !ctrl && f.second.is_some() && f.app.edition.is_none() && f.page.formulaire.is_none() && f.app.emplacement != Emplacement::Reglages => {
                        f.basculer_panneau();
                        animer(hwnd, f);
                        return LRESULT(0);
                    }
                    VK_B if ctrl && maj && !alt => {
                        basculer_arbre(f);
                        animer(hwnd, f);
                        return LRESULT(0);
                    }
                    VK_T if ctrl && maj && !alt => {
                        f.rouvrir_onglet(hwnd);
                        animer(hwnd, f);
                        return LRESULT(0);
                    }
                    // Ctrl+Alt+T est réservé au terminal (plus bas) : ne pas créer un onglet à sa place.
                    VK_T if ctrl && !alt => {
                        let emp = f.app.emplacement.clone();
                        f.nouvel_onglet(hwnd, emp);
                        animer(hwnd, f);
                        return LRESULT(0);
                    }
                    VK_W if ctrl => {
                        let k = f.rang;
                        f.fermer_onglet(hwnd, k);
                        animer(hwnd, f);
                        return LRESULT(0);
                    }
                    VK_TAB if ctrl => {
                        let n = f.nb_onglets();
                        let k = if maj { (f.rang + n - 1) % n } else { (f.rang + 1) % n };
                        f.aller_onglet(k);
                        animer(hwnd, f);
                        return LRESULT(0);
                    }
                    VK_P if alt => {
                        f.apercu_visible = !f.apercu_visible;
                        f.apercu_demande = None;
                        let _ = InvalidateRect(hwnd, None, false);
                        return LRESULT(0);
                    }
                    // Ctrl+Z : défaire la dernière action (aussi depuis les réglages)
                    VK_Z if ctrl && !alt && f.app.edition.is_none() && f.page.formulaire.is_none() => {
                        f.app.annuler_derniere(hwnd);
                        animer(hwnd, f);
                        return LRESULT(0);
                    }
                    VK_Y if ctrl && !alt && f.app.edition.is_none() && f.page.formulaire.is_none() => {
                        f.app.refaire(hwnd);
                        animer(hwnd, f);
                        return LRESULT(0);
                    }
                    VK_PAUSE => {
                        f.app.basculer_pause();
                        let _ = InvalidateRect(hwnd, None, false);
                        return LRESULT(0);
                    }
                    // Ctrl+, : réglages
                    VK_OEM_COMMA if ctrl => {
                        ouvrir_reglages(hwnd, f);
                        animer(hwnd, f);
                        return LRESULT(0);
                    }
                    VK_M if ctrl => {
                        let actifs = !sons::ACTIFS.load(Ordering::Relaxed);
                        reglages::modifier(|r| r.sons = actifs);
                        f.app.annoncer(if actifs { "SONS ACTIVES" } else { "SONS COUPES" }, false);
                        sons::jouer(sons::Son::Tic);
                        let _ = InvalidateRect(hwnd, None, false);
                        return LRESULT(0);
                    }
                    _ => {}
                }

                if f.app.emplacement == Emplacement::Reglages {
                    if touche_page(hwnd, f, vk, ctrl, maj, alt) {
                        animer(hwnd, f);
                        return LRESULT(0);
                    }
                    return if alt { DefWindowProcW(hwnd, msg, wparam, lparam) } else { LRESULT(0) };
                }

                // Raccourcis des commandes (E15–E18)
                let commande = match vk {
                    VK_RETURN if alt && !ctrl && f.app.edition.is_none() => Some(Commande::Proprietes),
                    VK_T if ctrl && alt => Some(Commande::Terminal),
                    VK_C if ctrl && alt => Some(Commande::VsCode),
                    VK_L if ctrl && alt => Some(Commande::CheminWsl),
                    VK_U if ctrl && alt => Some(Commande::CheminUnix),
                    VK_H if ctrl && alt => Some(Commande::Sha256),
                    VK_H if ctrl && maj => Some(Commande::Historique),
                    VK_F6 if ctrl && maj => Some(Commande::Comparer),
                    VK_F9 if !ctrl && !alt => Some(Commande::Essentiel),
                    VK_Z if ctrl && alt => Some(Commande::Zip),
                    VK_X if ctrl && alt => Some(Commande::Extraire),
                    VK_D if ctrl && !alt => Some(Commande::Epingler),
                    VK_F if ctrl && maj => Some(Commande::RechercheGlobale),
                    VK_1 if ctrl && maj => Some(Commande::VueListe),
                    VK_2 if ctrl && maj => Some(Commande::VueGrille),
                    VK_3 if ctrl && maj => Some(Commande::VueColonnes),
                    VK_4 if ctrl && maj => Some(Commande::VueTailles),
                    VK_1 if ctrl && !alt => Some(Commande::BaliseRouge),
                    VK_2 if ctrl && !alt => Some(Commande::BaliseBlanche),
                    VK_3 if ctrl && !alt => Some(Commande::BaliseGrise),
                    VK_0 if ctrl && !alt => Some(Commande::SansBalise),
                    // Ctrl+1/2/3 posent déjà les balises de couleur, Ctrl+Maj+1/2/3 changent la vue : les étiquettes (carrés) prennent Alt+1/2/3
                    VK_1 if alt && !ctrl && !maj => Some(Commande::Etiquette1),
                    VK_2 if alt && !ctrl && !maj => Some(Commande::Etiquette2),
                    VK_3 if alt && !ctrl && !maj => Some(Commande::Etiquette3),
                    VK_G if ctrl && !alt => Some(Commande::NouveauGroupe),
                    VK_A if ctrl && maj => Some(Commande::Administrateur),
                    _ => None,
                };
                if let Some(c) = commande.filter(|_| f.app.edition.is_none()) {
                    executer(hwnd, f, c);
                    animer(hwnd, f);
                    return LRESULT(0);
                }
                if f.app.edition.is_none() {
                    match vk {
                        // Maj+F5 (toujours) et F5 (double panneau seulement, comme la maquette) : copier vers l'autre panneau ;
                        // Maj+F6 : déplacer. En simple panneau F5 reste « actualiser » (plus bas).
                        VK_F5 if action_f5(f.second.is_some(), maj, ctrl, alt) == ActionF5::Copier => {
                            f.vers_autre(hwnd, false);
                            animer(hwnd, f);
                            return LRESULT(0);
                        }
                        VK_F6 if maj => {
                            f.vers_autre(hwnd, true);
                            animer(hwnd, f);
                            return LRESULT(0);
                        }
                        // Espace sur des dossiers : leur taille (comme Total Commander)
                        VK_SPACE if !ctrl && f.app.filtre.is_empty() && f.app.dossiers_choisis() && f.app.dossier().is_some() => {
                            f.app.calculer_tailles(hwnd);
                            animer(hwnd, f);
                            return LRESULT(0);
                        }
                        // Espace (sans filtre en cours) : coup d'œil
                        VK_SPACE if !ctrl && f.app.filtre.is_empty() => {
                            ouvrir_galerie(hwnd, f);
                            animer(hwnd, f);
                            return LRESULT(0);
                        }
                        // F2 sur plusieurs éléments : renommage par lot
                        VK_F2 if f.app.marques.len() > 1 => {
                            ouvrir_lot(f);
                            animer(hwnd, f);
                            return LRESULT(0);
                        }
                        _ => {}
                    }
                }

                let nb_pastilles = f.pastilles().len();
                let app = &mut f.app;
                let (par, _) = app.pas();
                let affichage = app.affichage;

                // Renommage en cours : les touches vont au champ de saisie
                if let Some(ed) = &mut app.edition {
                    match vk {
                        VK_RETURN => app.valider_renommage(),
                        VK_ESCAPE => app.annuler_renommage(),
                        VK_LEFT => ed.gauche(maj),
                        VK_RIGHT => ed.droite(maj),
                        VK_HOME => ed.aller(0, maj),
                        VK_END => ed.aller(ed.texte.len(), maj),
                        VK_BACK => ed.effacer_arriere(),
                        VK_DELETE => ed.effacer_avant(),
                        VK_A if ctrl => ed.tout(),
                        VK_C if ctrl => operations::copier_texte(hwnd, &ed.texte_choisi()),
                        VK_X if ctrl => {
                            let s = ed.couper();
                            operations::copier_texte(hwnd, &s);
                        }
                        VK_V if ctrl => {
                            if let Some(s) = operations::lire_texte(hwnd) {
                                ed.inserer(&s);
                            }
                        }
                        _ => return LRESULT(0),
                    }
                    let _ = InvalidateRect(hwnd, None, false);
                    return LRESULT(0);
                }

                match vk {
                    VK_LEFT if alt => app.precedent(hwnd),
                    VK_RIGHT if alt => app.suivant(hwnd),
                    VK_UP if alt => app.parent(hwnd),
                    // Retour arrière : efface le filtre lettre par lettre, puis remonte au dossier parent
                    VK_BACK => {
                        if !app.effacer_filtre() {
                            app.parent(hwnd);
                        }
                    }
                    VK_BROWSER_BACK => app.precedent(hwnd),
                    VK_BROWSER_FORWARD => app.suivant(hwnd),
                    // Grille : haut / bas changent de rangée, gauche / droite d'élément
                    VK_UP => app.deplacer_selection(-(par as i64), maj),
                    VK_DOWN => app.deplacer_selection(par as i64, maj),
                    VK_LEFT if affichage == Affichage::Grille => app.deplacer_selection(-1, maj),
                    VK_RIGHT if affichage == Affichage::Grille => app.deplacer_selection(1, maj),
                    // Colonnes : gauche remonte, droite entre dans le dossier choisi
                    VK_LEFT if affichage == Affichage::Colonnes => app.parent(hwnd),
                    VK_RIGHT if affichage == Affichage::Colonnes => {
                        if let Some(i) = app.selection.filter(|&i| app.entrees.get(i).is_some_and(|e| e.est_conteneur())) {
                            app.ouvrir(hwnd, i);
                        }
                    }
                    VK_PRIOR => app.deplacer_selection(-app.lignes_par_page(), maj),
                    VK_NEXT => app.deplacer_selection(app.lignes_par_page(), maj),
                    VK_HOME => app.deplacer_selection(i64::MIN / 2, maj),
                    VK_END => app.deplacer_selection(i64::MAX / 2, maj),
                    VK_RETURN => {
                        if let Some(i) = app.selection {
                            app.ouvrir(hwnd, i);
                        }
                    }
                    VK_F5 => app.charger(hwnd),
                    VK_F2 => app.commencer_renommage(),
                    VK_F3 | VK_F if (vk == VK_F3 || ctrl) && matches!(app.emplacement, Emplacement::Globale { .. }) => {
                        let motif = app.filtre.trim().to_string();
                        if !motif.is_empty() {
                            app.naviguer(hwnd, Emplacement::Globale { motif });
                        }
                    }
                    VK_F3 => app.rechercher(hwnd),
                    VK_F if ctrl => app.rechercher(hwnd),
                    VK_DELETE => app.supprimer(hwnd, maj),
                    VK_ESCAPE => {
                        // Une pastille ouverte (fixée par un clic, ou survolée) se referme d'abord ; l'annulation de l'opération
                        // ne vient qu'à l'Échap suivant
                        let fixee_valide = f.pastille_fixee.is_some_and(|i| i < nb_pastilles);
                        f.pastille_fixee = None;
                        if fixee_valide {
                        } else if matches!(app.survol, Cible::Pastille(_) | Cible::BoutonPastille(..)) && !f.pastille_masquee {
                            f.pastille_masquee = true;
                        } else if !app.vider_filtre() && !app.annuler_operation() {
                            app.rien_choisir();
                        }
                    }
                    // Ctrl+H : afficher ou masquer les fichiers cachés
                    VK_H if ctrl => {
                        app.caches = !app.caches;
                        app.charger(hwnd);
                    }
                    VK_A if ctrl => app.tout_choisir(),
                    VK_C if ctrl && maj => app.copier_chemin(hwnd),
                    VK_C if ctrl => app.copier(hwnd, false),
                    VK_X if ctrl => app.copier(hwnd, true),
                    VK_V if ctrl => app.coller(hwnd),
                    VK_N if ctrl && maj => app.nouveau_dossier(),
                    _ => return if alt { DefWindowProcW(hwnd, msg, wparam, lparam) } else { LRESULT(0) },
                }
                animer(hwnd, f);
                LRESULT(0)
            }

            // Saisie : dans le champ de renommage, sinon dans le filtre (la liste se réduit à chaque lettre)
            WM_CHAR => {
                if let Some(f) = etat_de(hwnd) {
                    if std::mem::take(&mut f.avaler_caractere) {
                        return LRESULT(0);
                    }
                    if let Some(c) = char::from_u32(wparam.0 as u32) {
                        if !c.is_control() && !(touche(VK_CONTROL) && !touche(VK_MENU)) {
                            if f.galerie.is_some() {
                                // Espace et lettres ne vont nulle part pendant le coup d'œil
                            } else if f.palette.is_some() {
                                if let Some(p) = f.palette.as_mut().filter(|p| !p.lance && p.accepte_saisie()) {
                                    p.saisie.inserer(c.encode_utf8(&mut [0; 4]));
                                    p.choix = 0;
                                    f.remplir_palette();
                                    lectures_palette(hwnd, f);
                                    // Les yeux de Pixel clignent 120 ms après la frappe
                                    minuteries::armer_clignement_palette(hwnd);
                                }
                                animer(hwnd, f);
                            } else if let Some(l) = &mut f.lot {
                                let k = l.focus;
                                l.champs[k].inserer(c.encode_utf8(&mut [0; 4]));
                                let _ = InvalidateRect(hwnd, None, false);
                            } else if f.app.emplacement == Emplacement::Reglages {
                                // Saisie dans le formulaire ; la page elle-même n'a pas de filtre
                                if let Some(fo) = &mut f.page.formulaire {
                                    if let Some(ed) = fo.champ() {
                                        ed.inserer(c.encode_utf8(&mut [0; 4]));
                                        fo.forcer = false;
                                    }
                                }
                                let _ = InvalidateRect(hwnd, None, false);
                            } else if let Some(ed) = &mut f.app.saisie_chemin {
                                ed.inserer(c.encode_utf8(&mut [0; 4]));
                                f.app.completion = None;
                                let _ = InvalidateRect(hwnd, None, false);
                            } else if let Some(ed) = &mut f.app.edition {
                                ed.inserer(c.encode_utf8(&mut [0; 4]));
                                let _ = InvalidateRect(hwnd, None, false);
                            } else if !(c == ' ' && f.app.filtre.is_empty()) {
                                f.app.taper_filtre(c);
                                animer(hwnd, f);
                            }
                        }
                    }
                }
                LRESULT(0)
            }

            WM_SIZE => {
                if let Some(f) = etat_de(hwnd) {
                    f.rendu.redimensionner((lparam.0 & 0xFFFF) as u32, ((lparam.0 >> 16) & 0xFFFF) as u32);
                    // Le responsive doit être juste avant le prochain message souris, même sans attente du dessin.
                    crate::theme::memoriser_largeur(f.dip((lparam.0 & 0xFFFF) as i32));
                    // Réduction ou rétablissement : posté (WM_SIZE peut arriver pendant un traitement qui tient `f`, comme
                    // ShowWindow(SW_MINIMIZE) du bouton Réduire) ; le message posté règle les minuteries
                    let _ = PostMessageW(hwnd, WM_APPARENCE, WPARAM(0), LPARAM(0));
                    let _ = InvalidateRect(hwnd, None, false);
                }
                LRESULT(0)
            }

            WM_DPICHANGED => {
                if let Some(f) = etat_de(hwnd) {
                    f.dpi = if dpi_force().is_some() { dpi_de(hwnd) } else { (wparam.0 & 0xFFFF) as u32 } as f32;
                    f.rendu.changer_dpi();
                    // Les coordonnées du pointeur sont en DIP d'une échelle qui change : le reflet attend le prochain WM_MOUSEMOVE
                    f.pointeur = None;
                    let r = &*(lparam.0 as *const RECT);
                    let _ = SetWindowPos(hwnd, None, r.left, r.top, r.right - r.left, r.bottom - r.top, SWP_NOZORDER | SWP_NOACTIVATE);
                }
                LRESULT(0)
            }

            // La fenêtre reprend le focus : l'avis de thème ou de mouvement de `shared.json` a pu changer pendant qu'elle dormait
            // (rien n'est dessiné ni invalidé si le thème effectif est le même)
            WM_ACTIVATE => {
                // Envoyé de façon synchrone (fermeture d'un dialogue modal, pendant un traitement de la fenêtre) : posté. Le message
                // posté relit l'activité et la réduction et règle les minuteries de Pixel (activation comme désactivation).
                let _ = PostMessageW(hwnd, WM_APPARENCE, WPARAM(0), LPARAM(0));
                DefWindowProcW(hwnd, msg, wparam, lparam)
            }

            WM_APPARENCE => {
                rafraichir_apparence(hwnd);
                // Activité, réduction, mouvement réduit (Windows, shared.json) : relus ici, hors de tout traitement qui tient `f`
                if let Some(f) = etat_de(hwnd) {
                    f.active = GetActiveWindow() == hwnd;
                    f.reduite = IsIconic(hwnd).as_bool();
                    // Mouvement réduit changé (Windows, shared.json) : le halo apparaît ou disparaît sans attendre la souris
                    let reflet_actif = reflet::actif_maintenant();
                    if reflet_actif != f.reflet_actif {
                        f.reflet_actif = reflet_actif;
                        let _ = InvalidateRect(hwnd, None, false);
                    }
                    regler_clignement(hwnd, f);
                    regler_activite(hwnd, f);
                    // Retour de la fenêtre au premier plan : les serveurs et clients cloud ont pu changer d'état
                    if f.active {
                        f.mesurer_etats(hwnd);
                    }
                }
                LRESULT(0)
            }

            // Thème Contraste élevé, thème clair ou sombre, effets d'animation activés ou coupés dans Windows : relus tout de
            // suite, sans attendre un réglage de l'app
            WM_SETTINGCHANGE => {
                let texte = reglages::relire_echelle_texte();
                if texte {
                    if let (Some(f), Ok(r)) = (etat_de(hwnd), Rendu::nouveau()) {
                        f.rendu = r;
                    }
                }
                if reglages::relire_contraste_windows() || texte {
                    let _ = InvalidateRect(hwnd, None, false);
                }
                rafraichir_apparence(hwnd);
                // Le mouvement réduit a pu changer : les minuteries de Pixel sont réévaluées par le message posté
                let _ = PostMessageW(hwnd, WM_APPARENCE, WPARAM(0), LPARAM(0));
                DefWindowProcW(hwnd, msg, wparam, lparam)
            }

            // Alt relâché seul : pas de « mode menu » (il n'y a pas de barre de menus et il avalerait les touches suivantes)
            WM_SYSCOMMAND if (wparam.0 & 0xFFF0) as u32 == SC_KEYMENU && lparam.0 == 0 => LRESULT(0),

            // Onglets envoyés par une autre fenêtre (RATTACHER) : chacun s'ouvre ici s'il est un dossier qui existe
            WM_COPYDATA => {
                if lparam.0 == 0 {
                    return LRESULT(0);
                }
                // SAFETY: lparam non nul vérifié ci-dessus ; pour WM_COPYDATA il pointe sur un COPYDATASTRUCT valide pendant le message
                let cd = &*(lparam.0 as *const windows::Win32::System::DataExchange::COPYDATASTRUCT);
                // Un chemin à ouvrir dans un nouvel onglet (`explorer.open {reuse}`) : 1 = acceptée, 0 = refusée
                if cd.dwData == eco::reutiliser::OUVRIR_ICI {
                    return LRESULT(recevoir_ouvrir_ici(hwnd, cd) as isize);
                }
                if cd.dwData != ONGLETS_COPIES || cd.lpData.is_null() {
                    return LRESULT(0);
                }
                // SAFETY: l'expéditeur garantit cbData octets lisibles pendant l'appel (contrat de WM_COPYDATA)
                let u = std::slice::from_raw_parts(cd.lpData as *const u16, cd.cbData as usize / 2);
                let Some(f) = etat_de(hwnd) else { return LRESULT(0) };
                for chemin in String::from_utf16_lossy(u).lines().filter(|c| std::path::Path::new(c).is_dir()).take(50) {
                    f.nouvel_onglet(hwnd, Emplacement::dossier(chemin));
                }
                let _ = InvalidateRect(hwnd, None, false);
                LRESULT(1)
            }

            // Fin de la consultation du disque de `recevoir_ouvrir_ici` : l'onglet s'ouvre ici, sur le fil d'interface
            WM_OUVRIR_ICI => {
                // Aucun pointeur dans ce message : un message forgé ne fait que vider la file plus tôt (voir `OUVERTURES`)
                let a_ouvrir = eco::reutiliser::vider(&OUVERTURES, hwnd.0 as isize);
                if let Some(f) = etat_de(hwnd) {
                    for o in a_ouvrir {
                        ouvrir_ici(hwnd, f, o);
                    }
                }
                LRESULT(0)
            }

            WM_GETOBJECT => match accessibilite::demande(hwnd, wparam, lparam) {
                Some(r) => {
                    // Premier lecteur d'écran : l'instantané n'existait pas, il se remplit tout de suite puis à chaque dessin
                    if let Some(f) = etat_de(hwnd) {
                        let noms: Vec<String> = f.pastilles().iter().map(|(p, _)| p.nom_accessible()).collect();
                        let lue = f.page_lue();
                        accessibilite::maj(hwnd, &f.app, &noms, lue.as_ref());
                    }
                    let _ = InvalidateRect(hwnd, None, false);
                    r
                }
                None => DefWindowProcW(hwnd, msg, wparam, lparam),
            },

            WM_ERASEBKGND => LRESULT(1),

            WM_PAINT => {
                let mut ps = PAINTSTRUCT::default();
                BeginPaint(hwnd, &mut ps);
                if let Some(f) = etat_de(hwnd) {
                    // L'élément montré par l'aperçu a changé : on le prépare (après un court délai)
                    if f.apercu_visible {
                        if let Some((chemin, entree)) = sujet_apercu(&f.app) {
                            if f.apercu_demande.as_deref() != Some(chemin.as_str()) {
                                f.apercu_demande = Some(chemin.clone());
                                f.apercu_attente = Some((chemin, entree));
                                SetTimer(hwnd, MINUTERIE_APERCU, 70, None);
                            }
                        }
                    }
                    // Progression d'une copie : bouton de la barre des tâches
                    let op = f.tous().find_map(|a| a.operation.as_ref().map(|o| glyph::Avancement { part: o.avancement }));
                    glyph::progression(hwnd, op);

                    montrer_dossier_arbre(hwnd, f);
                    // Capture sans souris (mode essai seulement) : `SPACEFILES_REFLET=x,y` pose le pointeur et la cible survolée
                    if let Some((ex, ey)) = reflet::pointeur_d_essai() {
                        let cible = cible_sous(hwnd, f, (ex * f.dpi / 96.0) as i32, (ey * f.dpi / 96.0) as i32);
                        f.app.survol = cible;
                        f.pointeur = Some((ex, ey));
                    }
                    // Pastilles d'opération et Pixel : calculés avant l'emprunt de la vue (et avant que la carte des conflits
                    // quitte son onglet, car Pixel en dépend)
                    let pixel = f.vue_pixel(hwnd);
                    let liste_pastilles: Vec<pastilles::Pastille> = f.pastilles().into_iter().map(|(p, _)| p).collect();
                    // Un rang fixé par un clic et devenu périmé (la liste a raccourci) est oublié
                    if f.pastille_fixee.is_some_and(|i| i >= liste_pastilles.len()) {
                        f.pastille_fixee = None;
                    }
                    let pastille_ouverte = f.pastille_ouverte(liste_pastilles.len());
                    let noms: Vec<String> = liste_pastilles.iter().map(|p| p.nom_accessible()).collect();
                    let lue = f.page_lue();
                    accessibilite::maj(hwnd, &f.app, &noms, lue.as_ref());
                    let (l, h) = taille_client(hwnd);
                    let dpi = f.dpi;
                    let titres = f.titres();
                    let lignes = if f.app.emplacement == Emplacement::Reglages { f.lignes() } else { Vec::new() };
                    // Colonnes Miller : le contenu de l'élément choisi est demandé quand il change
                    for a in std::iter::once(&mut f.app).chain(f.second.iter_mut()) {
                        if a.affichage == Affichage::Colonnes {
                            a.demander_miller_enfant(hwnd);
                        }
                    }
                    let n = f.app.entrees.len();
                    let position = f.app.selection.unwrap_or(0);
                    // Carte des conflits (de n'importe quel onglet) : sortie de son onglet le temps du dessin
                    let conflits = f.tous().enumerate().find_map(|(k, a)| a.conflits.take().map(|c| (k, c)));
                    // Lot 12C : fondus de survol et soulèvement du chrome, mis à jour une fois par image avant le dessin
                    let t_maj = mouvement::maintenant_ms();
                    let animer = reglages::animations_actives();
                    let soulevements = {
                        let mut m = f.rendu.mouvements.borrow_mut();
                        m.survols.maj(f.app.survol, f.survol_appui, t_maj, animer);
                        m.survols.soulevements(t_maj)
                    };
                    // La page des Réglages qu'on vient de quitter reste dessinée 160 ms (fondu de sortie)
                    let ouvert_ici = f.app.emplacement == Emplacement::Reglages;
                    let page_sortie_voulue = animer && f.second.is_none() && f.rendu.mouvements.borrow().page.sortie_voulue(ouvert_ici, t_maj);
                    let lignes_sortie = if page_sortie_voulue { f.lignes() } else { Vec::new() };
                    // Ouverture : la fenêtre part de l'état caché dès le premier dessin (sans quoi une image pleine précéderait le fondu)
                    let fenetre = if f.ouverture_attend && animer && f.fermeture.is_none() {
                        Some(mouvement::etat_ouverture(0))
                    } else {
                        mouvement::etat_fenetre(f.ouverture, f.fermeture, t_maj, animer)
                    };
                    let cascade = if !animer { None } else if f.ouverture_attend { Some(0) } else { f.ouverture.map(|d| t_maj.saturating_sub(d)).filter(|e| *e < mouvement::CASCADE_TOTAL_MS) };
                    // Panneau Historique : les entrées sont lues (et le fichier chargé) seulement tant que le tiroir est sorti ou rentre
                    let hist_visible = f.historique || f.rendu.mouvements.borrow().tiroir_visible(t_maj);
                    let hist_entrees = if hist_visible { journal_historique::instantane() } else { Vec::new() };
                    let hist_maintenant = if hist_visible { journal_historique::maintenant() } else { String::new() };
                    let mut vue = Vue {
                        historique: rendu::VueHistorique { ouvert: f.historique, entrees: &hist_entrees, defilement: f.historique_defilement, maintenant: hist_maintenant },
                        pointeur: f.pointeur,
                        soulevements: &soulevements,
                        fenetre,
                        cascade,
                        page_sortie: page_sortie_voulue.then(|| VuePage { lignes: &lignes_sortie, page: &f.page, focus_clavier: f.focus_clavier }),
                        onglets: &titres,
                        actif: f.rang,
                        apercu_visible: f.apercu_visible && f.app.emplacement != Emplacement::Reglages,
                        apercu: f.apercu.as_ref(),
                        terminal_visible: f.terminal_visible && f.app.emplacement != Emplacement::Reglages,
                        terminal_lignes: &f.terminal_lignes,
                        page: (f.app.emplacement == Emplacement::Reglages).then_some(VuePage { lignes: &lignes, page: &f.page, focus_clavier: f.focus_clavier }),
                        double: f.second.is_some(),
                        focus_droite: f.focus_droite,
                        focus_clavier: f.focus_clavier,
                        palette: f.palette.as_ref(),
                        lot: f.lot.as_ref(),
                        galerie: f.galerie.as_ref().map(|g| (g, position, n)),
                        conflits: conflits.as_ref().map(|c| &c.1),
                        menu: f.menu.as_ref(),
                        pastilles: &liste_pastilles,
                        pastille_ouverte,
                        pixel,
                    };
                    // Mesure de coût du reflet (mode essai : `SPACEFILES_MESURE_REFLET=n`) : n images sans reflet puis n avec, deux fois,
                    // après un échauffement ; le résultat va au journal. Une seule fois par fenêtre.
                    // Au premier dessin les zones (barre d'actions…) n'existent pas encore : la cible d'essai n'est trouvée qu'au suivant
                    let cible_trouvee = reflet::pointeur_d_essai().is_none() || f.app.survol != Cible::Rien;
                    if let (Some(n), false, true) = (reflet::mesure_demandee(), f.mesure_reflet_faite, cible_trouvee) {
                        f.mesure_reflet_faite = true;
                        let pointeur = vue.pointeur;
                        for _ in 0..5 {
                            let _ = f.rendu.dessiner(hwnd, l, h, dpi, &mut f.app, f.second.as_mut(), &vue);
                        }
                        let mut total_ms = [0.0f64; 2];
                        let mut eclaire = false;
                        for _ in 0..2 {
                            for (k, avec) in [false, true].into_iter().enumerate() {
                                vue.pointeur = if avec { pointeur } else { None };
                                let debut = std::time::Instant::now();
                                for _ in 0..n {
                                    let _ = f.rendu.dessiner(hwnd, l, h, dpi, &mut f.app, f.second.as_mut(), &vue);
                                }
                                total_ms[k] += debut.elapsed().as_secs_f64() * 1000.0;
                                eclaire |= avec && f.rendu.zone_reflet.get().is_some();
                            }
                        }
                        vue.pointeur = pointeur;
                        let par_image = |t: f64| t / (2 * n) as f64;
                        journal::ecrire(
                            journal::Niveau::Info,
                            "reflet",
                            &format!("MESURE n={n} sans={:.3} ms/image avec={:.3} ms/image ecart={:.3} ms zone_eclairee={eclaire}", par_image(total_ms[0]), par_image(total_ms[1]), par_image(total_ms[1]) - par_image(total_ms[0])),
                        );
                    }
                    // Mesure du défilement (mode essai : `SPACEFILES_MESURE_DEFILEMENT`) : dessin chronométré puis image suivante demandée
                    let mesure = mesure_defilement::demandee().filter(|d| mesure_defilement::avant_image(&mut f.app, *d));
                    let _ = f.rendu.dessiner(hwnd, l, h, dpi, &mut f.app, f.second.as_mut(), &vue);
                    if let Some(d) = mesure {
                        mesure_defilement::apres_image(hwnd, &mut f.app, d);
                    }
                    // Ouverture animée : l'horloge démarre après le premier dessin (la cible Direct2D vient d'être créée)
                    let maintenant = mouvement::maintenant_ms();
                    if std::mem::take(&mut f.ouverture_attend) && reglages::animations_actives() {
                        f.ouverture = Some(maintenant);
                        f.mesure_ouverture = mouvement::mesure_demandee().then_some((maintenant, 0));
                    }
                    if let Some((debut, images)) = f.mesure_ouverture.as_mut() {
                        *images += 1;
                        if f.ouverture.is_none() {
                            journal::ecrire(journal::Niveau::Info, "mouvement", &format!("OUVERTURE du premier au dernier dessin : {} ms, {} images", maintenant - *debut, images));
                            f.mesure_ouverture = None;
                        }
                    }
                    // Mesure de cadence (mode essai) : intervalle entre deux dessins d'une même animation
                    if mouvement::mesure_demandee() && f.mouvement_en_cours(maintenant) {
                        if let Some(bilan) = f.cadence.noter(mouvement::maintenant_us()) {
                            mouvement::journaliser_cadence(&bilan);
                        }
                    }
                    // Un glissement vient de commencer (ou continue) : la minuterie d'animation existante le porte jusqu'à sa fin
                    if (f.ouverture.is_some() || f.rendu.mouvements.borrow().en_cours(maintenant)) && !f.anime {
                        f.anime = true;
                        armer_animation(hwnd, f);
                    }
                    if let Some((k, c)) = conflits {
                        if let Some(a) = f.tous().nth(k) {
                            a.conflits = Some(c);
                        }
                    }
                    // Vignettes de la grille à préparer
                    let demandes: Vec<String> = f.rendu.a_demander.borrow_mut().drain(..).collect();
                    if !demandes.is_empty() {
                        // Les vignettes passent par des décodeurs du Shell : pas dans un dossier non fiable
                        let non_fiables = reglages::actuels().non_fiables;
                        let demandes: Vec<String> = demandes.into_iter().filter(|c| !reglages::est_sous(dossier::dossier_de(c), &non_fiables)).collect();
                        let oubliees = f.vignettes.get_or_insert_with(|| Vignettes::nouveau(hwnd)).demander(demandes);
                        f.rendu.oublier(&oubliees);
                    }
                    // La même minuterie efface l'annonce et les pastilles de fin, d'erreur et d'annulation
                    // Tous les onglets : une fin d'opération d'un onglet masqué arme aussi la minuterie qui l'efface
                    let temporaires = f.pastille_temporaire_presente();
                    if (f.app.annonce.is_some() || temporaires) && !f.annonce_armee {
                        f.annonce_armee = true;
                        SetTimer(hwnd, MINUTERIE_ANNONCE, 500, None);
                    }
                    surveiller_session(hwnd, f);
                }
                let _ = EndPaint(hwnd, &ps);
                LRESULT(0)
            }

            // Fermer en pleine copie tuerait le fil de Windows : fichier tronqué, file et Ctrl+Z perdus
            WM_CLOSE if etat_de(hwnd).is_some_and(|f| f.tous().any(|a| a.operation.is_some())) => {
                if let Some(f) = etat_de(hwnd) {
                    f.app.annoncer("OPERATION EN COURS · ECHAP POUR L'ANNULER AVANT DE FERMER", true);
                    let _ = InvalidateRect(hwnd, None, false);
                }
                LRESULT(0)
            }

            // Fermeture animée (lot 12A) : la garde ci-dessus a déjà refusé une fermeture en pleine opération. La sortie dure 240 ms
            // puis la minuterie d'animation détruit la fenêtre. Mouvement réduit, fenêtre réduite ou invisible : fermeture immédiate.
            WM_CLOSE
                if etat_de(hwnd).is_some_and(|f| {
                    f.fermeture.is_some() || (reglages::animations_actives() && !IsIconic(hwnd).as_bool() && IsWindowVisible(hwnd).as_bool())
                }) =>
            {
                if let Some(f) = etat_de(hwnd) {
                    if f.fermeture.is_none() {
                        f.fermeture = Some(mouvement::maintenant_ms());
                        animer(hwnd, f);
                    }
                }
                LRESULT(0)
            }

            WM_DESTROY => {
                let _ = RevokeDragDrop(hwnd);
                // L'abonnement au dossier Téléchargements est coupé avec la fenêtre : le fil s'arrête dès qu'il le voit (une lecture en
                // cours peut finir, son résultat est jeté par la génération)
                navigateur::arreter();
                // Session : onglets ouverts et dernier dossier, pour la prochaine ouverture
                if let Some(f) = etat_de(hwnd) {
                    fermer_formulaire(f);
                    sauver_session(f);
                    // Fenêtre détruite en pleine animation : le fil de synchronisation sur l'écran s'arrête
                    if let Some(arret) = f.metronome.take() {
                        arret.store(true, std::sync::atomic::Ordering::Release);
                    }
                    // Fermeture normale : le marqueur part avec la dernière fenêtre principale
                    lever_marqueur();
                }
                glyph::progression(hwnd, None);
                let p = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut Fenetre;
                if !p.is_null() {
                    SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
                    drop(Box::from_raw(p));
                }
                PostQuitMessage(0);
                LRESULT(0)
            }

            _ => DefWindowProcW(hwnd, msg, wparam, lparam),
        }
    }
}
