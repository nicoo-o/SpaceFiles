// Menus : clic droit, barre d'actions, menu de la barre d'état, sessions, historique et leurs sous-menus.

use crate::*;
/// Menu clic droit de Windows pour la zone visée ; (x, y) en coordonnées écran
pub(crate) unsafe fn menu_windows(hwnd: HWND, cible: Cible, x: i32, y: i32) {
    let Some(f) = etat_de(hwnd) else { return };
    let dossier = f.app.dossier().map(str::to_string);
    let chemins = match cible {
        Cible::Ligne(_) => f.app.chemins_choisis(),
        Cible::FondListe => Vec::new(),
        c => match f.app.chemin_cible(c) {
            Some(ch) => vec![ch],
            None => return,
        },
    };
    let fond = if matches!(cible, Cible::Ligne(_) | Cible::FondListe) { dossier.as_deref() } else { None };
    if chemins.is_empty() && fond.is_none() {
        return;
    }
    let r = coquille::menu(hwnd, &chemins, fond, x, y, touche(VK_SHIFT));
    let Some(f) = etat_de(hwnd) else { return };
    match r {
        // Une relecture du dossier pendant le menu peut avoir déplacé la sélection : renommer l'élément voisin serait pire que ne rien faire
        Ok(coquille::Choix::Renommer) if matches!(cible, Cible::Ligne(_)) => {
            if f.app.chemins_choisis() == chemins {
                f.app.commencer_renommage()
            } else {
                f.app.annoncer("LA LISTE A CHANGE PENDANT LE MENU : RENOMMEZ A NOUVEAU (F2)", true)
            }
        }
        Err(e) => f.app.annoncer(operations::message_com(&e), true),
        _ => {}
    }
    let _ = InvalidateRect(hwnd, None, false);
}

/// Clic sur la barre d'état : menu de la sélection, au-dessus du clic (DIP)
pub(crate) unsafe fn menu_statut(hwnd: HWND, f: &mut Fenetre, x: f32, y: f32) {
    use menu::Autre;
    use Choix::{Autre as A, Commande as C, Separation as S};
    let rien = f.app.marques.is_empty();
    let mut choix = vec![C(Commande::ToutChoisir, false), C(Commande::InverserSelection, false)];
    if !rien {
        choix.extend([A("TOUT DESELECTIONNER".into(), Autre::Deselectionner), S, A("COPIER LES NOMS".into(), Autre::CopierNoms), C(Commande::CopierChemin, false), S, Choix::Sous("METTRE DANS UN GROUPE", sous_menu_groupes(&f.app))]);
    }
    choix.extend([S, C(Commande::TaillesDossiers, false)]);
    let m = ouvrir_menu(hwnd, f, choix, x, y);
    // Le menu s'ouvre vers le haut, au-dessus de la barre
    let haut = m.niveaux[0].hauteur();
    m.niveaux[0].y = (y - haut - 6.0).max(4.0);
    let _ = InvalidateRect(hwnd, None, false);
}

/// Mise en forme d'un menu : libellés, raccourcis, lignes grisées
pub(crate) fn lignes_menu(a: &App, choix: Vec<Choix>) -> Vec<menu::Ligne> {
    choix
        .into_iter()
        .map(|c| match c {
            Choix::Commande(c, coche) => {
                let texte = libelle_commande(c);
                let (libelle, raccourci) = texte.split_once('\t').unwrap_or((texte.as_str(), ""));
                menu::Ligne { libelle: libelle.to_string(), raccourci: raccourci.to_string(), coche, actif: commande_possible(a, c), genre: menu::Genre::Commande(c) }
            }
            Choix::Separation => menu::Ligne { libelle: String::new(), raccourci: String::new(), coche: false, actif: false, genre: menu::Genre::Separation },
            Choix::Sous(titre, v) => menu::Ligne { libelle: titre.to_string(), raccourci: String::new(), coche: false, actif: true, genre: menu::Genre::Sous(lignes_menu(a, v)) },
            Choix::PlusOptions => menu::Ligne { libelle: "PLUS D'OPTIONS".into(), raccourci: "MAJ+F10".into(), coche: false, actif: true, genre: menu::Genre::PlusOptions },
            Choix::Icones(v) => menu::Ligne { libelle: String::new(), raccourci: String::new(), coche: false, actif: true, genre: menu::Genre::Icones(v.into_iter().map(|(o, c)| (o, c, commande_possible(a, c))).collect()) },
            // « ● NOM » : ligne cochée (groupe qui contient déjà les éléments)
            Choix::Autre(libelle, x) => {
                let coche = libelle.starts_with("● ");
                let libelle = libelle.trim_start_matches("● ").to_string();
                menu::Ligne { libelle, raccourci: String::new(), coche, actif: true, genre: menu::Genre::Autre(x) }
            }
        })
        .collect()
}

/// Ouvre un menu Nothing en (x, y), en DIP dans la fenêtre
pub(crate) unsafe fn ouvrir_menu(hwnd: HWND, f: &mut Fenetre, choix: Vec<Choix>, x: f32, y: f32) -> &mut menu::Menu {
    let lignes = lignes_menu(&f.app, choix);
    let largeur = menu::largeur_sous(&lignes).max(240.0);
    let (l, h) = taille_client(hwnd);
    sons::jouer(sons::Son::Tic);
    f.menu.insert(menu::Menu::nouveau(lignes, x, y, largeur, (f.dip(l as i32), f.dip(h as i32))))
}

pub(crate) fn fermer_menu(f: &mut Fenetre) {
    f.menu = None;
    f.app.menu_barre = None;
}

/// Ce qu'un clic ou une touche a demandé au menu
pub(crate) unsafe fn agir_menu(hwnd: HWND, f: &mut Fenetre, action: menu::Action) {
    match action {
        menu::Action::Rien => {}
        menu::Action::Fermer => fermer_menu(f),
        menu::Action::Commande(c) => {
            fermer_menu(f);
            executer(hwnd, f, c);
        }
        menu::Action::Autre(x) => {
            fermer_menu(f);
            let _ = InvalidateRect(hwnd, None, false);
            let _ = UpdateWindow(hwnd);
            action_autre(hwnd, f, x);
        }
        menu::Action::PlusOptions => {
            let cible = f.menu.as_ref().and_then(|m| m.cible);
            fermer_menu(f);
            let _ = InvalidateRect(hwnd, None, false);
            let _ = UpdateWindow(hwnd);
            if let Some((c, x, y)) = cible {
                menu_windows(hwnd, c, x, y);
            }
        }
    }
    let _ = InvalidateRect(hwnd, None, false);
}

/// Actions des menus qui ne sont pas des commandes de palette
pub(crate) unsafe fn action_autre(hwnd: HWND, f: &mut Fenetre, x: menu::Autre) {
    let chemins = if f.app.sans_fichiers() { Vec::new() } else { f.app.chemins_choisis() };
    let r = match x {
        menu::Autre::Verbe(v) => return verbe_shell(hwnd, f, v),
        menu::Autre::Application(k) => match f.applications.get(k) {
            Some(a) if !chemins.is_empty() => coquille::ouvrir_avec(hwnd, &chemins, a),
            _ => Ok(()),
        },
        menu::Autre::ChoisirApplication => match chemins.first() {
            Some(c) => coquille::choisir_application(hwnd, c),
            None => {
                f.app.annoncer("CHOISISSEZ D'ABORD UN FICHIER", true);
                Ok(())
            }
        },
        menu::Autre::AutrePanneau => {
            if let (Some(ch), Some(o)) = (f.app.selection.and_then(|i| f.app.chemin_cible(Cible::Ligne(i))), f.second.as_mut()) {
                o.naviguer(hwnd, Emplacement::dossier(&ch));
            }
            Ok(())
        }
        menu::Autre::VersAutre(deplacer) => {
            f.vers_autre(hwnd, deplacer);
            Ok(())
        }
        menu::Autre::BasculerGroupe(k) => {
            if let Some(g) = f.app.groupes.get(k).map(|g| g.nom.clone()) {
                if groupes::contient_tous(&g, &chemins) {
                    groupes::retirer(&g, &chemins);
                    f.app.annoncer(format!("RETIRE DE {g}"), false);
                    crate::app::toaster(format!("Retiré de {g}"));
                } else {
                    let n = groupes::ajouter(&g, &chemins);
                    f.app.annoncer(format!("{g} · {n} AJOUTE{}", if n > 1 { "S" } else { "" }), false);
                    crate::app::toaster(crate::app::phrase_ajoute(n, &g));
                }
                rafraichir_groupes(hwnd, f);
            }
            Ok(())
        }
        menu::Autre::TelechargerSftp => {
            if reglages::fonction_experimentale_permise(reglages::experimental(), reglages::Experimentale::Sftp) {
                f.app.telecharger_sftp(hwnd);
            } else {
                f.app.annoncer(reglages::MESSAGE_EXPERIMENTAL, true);
            }
            Ok(())
        }
        menu::Autre::AllerCopie => {
            let copie = f.app.selection.and_then(|i| f.app.entrees.get(i)).and_then(|e| f.app.badges.get(&e.nom.to_lowercase()).cloned());
            if let Some(telechargements::Badge::Copie(c)) = copie {
                f.app.montrer(hwnd, &c);
            }
            Ok(())
        }
        menu::Autre::OuvrirSession(k) => {
            if let Some(nom) = reglages::actuels().sessions.get(k).and_then(|x| x.split('|').next().map(str::to_string)) {
                ouvrir_session(hwnd, f, &nom);
            }
            Ok(())
        }
        menu::Autre::SupprimerSession(k) => {
            reglages::modifier(|r| {
                if k < r.sessions.len() {
                    r.sessions.remove(k);
                }
            });
            f.app.annoncer("SESSION SUPPRIMEE", false);
            Ok(())
        }
        menu::Autre::Regrouper(k) => {
            if f.app.affichage != Affichage::Liste {
                f.app.changer_affichage(hwnd, Affichage::Liste);
            }
            f.app.regrouper(hwnd, dossier::Regroupement::TOUS[k as usize]);
            Ok(())
        }
        menu::Autre::BasculerColonne(bit) => {
            changer_colonnes(f, |c| c ^ bit);
            // Ces deux colonnes ne sont lues que si elles sont montrées : relire ce qui a déjà été lu
            if bit & (reglages::COL_ORIGINE | reglages::COL_SIGNE) != 0 {
                f.app.oublier_metas();
            }
            f.app.lancer_metas(hwnd);
            Ok(())
        }
        menu::Autre::DateMontree(k) => {
            changer_colonnes(f, |c| (c & !0x300) | (k << 8));
            // La date triée change aussi
            let (col, croissant, melange) = f.app.tri;
            if col == dossier::Colonne::Date {
                f.app.tri = (col, !croissant, melange);
                f.app.trier_par(dossier::Colonne::Date);
            }
            Ok(())
        }
        menu::Autre::DeposerIci(k) => {
            if let Some((sources, dest)) = f.depot_droit.take() {
                match k {
                    0 | 1 => {
                        f.app.deposer(hwnd, sources, dest, k == 1);
                    }
                    2 => match coquille::raccourcis(&sources, &dest) {
                        Ok(n) => f.app.annoncer(format!("{n} RACCOURCI{} CRÉÉ{}", if n > 1 { "S" } else { "" }, if n > 1 { "S" } else { "" }), false),
                        Err(e) => f.app.annoncer(operations::message_com(&e), true),
                    },
                    _ => {}
                }
            }
            Ok(())
        }
        menu::Autre::Deselectionner => {
            f.app.rien_choisir();
            Ok(())
        }
        menu::Autre::CopierNoms => {
            let noms: Vec<String> = f.app.choisis().into_iter().filter_map(|i| f.app.entrees.get(i)).map(|e| e.nom.clone()).collect();
            if !noms.is_empty() {
                operations::copier_texte(hwnd, &noms.join("\r\n"));
                f.app.annoncer(format!("{} NOM{} COPIE{}", noms.len(), if noms.len() > 1 { "S" } else { "" }, if noms.len() > 1 { "S" } else { "" }), false);
                crate::app::toaster(if noms.len() > 1 { format!("{} noms copiés", noms.len()) } else { "Nom copié".to_string() });
            }
            Ok(())
        }
        menu::Autre::NouveauGroupe => {
            demander_groupe(f);
            Ok(())
        }
        menu::Autre::RetirerDuGroupe => {
            if let Emplacement::Groupe(g) = f.app.emplacement.clone() {
                groupes::retirer(&g, &chemins);
                f.app.annoncer(format!("RETIRE DE {g} (LES FICHIERS RESTENT)"), false);
                rafraichir_groupes(hwnd, f);
            }
            Ok(())
        }
        menu::Autre::OuvrirGroupe(k) => {
            if let Some(g) = f.app.groupes.get(k).map(|g| g.nom.clone()) {
                f.app.naviguer(hwnd, Emplacement::Groupe(g));
            }
            Ok(())
        }
        menu::Autre::OngletGroupe(k) => {
            if let Some(g) = f.app.groupes.get(k).map(|g| g.nom.clone()) {
                f.nouvel_onglet(hwnd, Emplacement::Groupe(g));
            }
            Ok(())
        }
        menu::Autre::RenommerGroupe(k) => {
            if let Some(g) = f.app.groupes.get(k).map(|g| g.nom.clone()) {
                f.palette = Some(Palette::renommer_groupe(&g));
                f.remplir_palette();
            }
            Ok(())
        }
        menu::Autre::CouleurGroupe(k, c) => {
            if let Some(g) = f.app.groupes.get(k).map(|g| g.nom.clone()) {
                groupes::colorer(&g, c);
                rafraichir_groupes(hwnd, f);
            }
            Ok(())
        }
        menu::Autre::SupprimerGroupe(k) => {
            if let Some(g) = f.app.groupes.get(k).map(|g| g.nom.clone()) {
                groupes::supprimer(&g);
                f.app.annoncer(format!("GROUPE {g} SUPPRIME (LES FICHIERS RESTENT)"), false);
                if f.app.emplacement == Emplacement::Groupe(g) {
                    f.app.parent(hwnd);
                }
                rafraichir_groupes(hwnd, f);
            }
            Ok(())
        }
    };
    if let Err(e) = r {
        f.app.annoncer(operations::message_com(&e), true);
    }
}

/// Menu des sessions : les ouvrir, enregistrer les onglets, en supprimer
pub(crate) unsafe fn menu_sessions(hwnd: HWND, f: &mut Fenetre) {
    use menu::Autre;
    let sessions = reglages::actuels().sessions;
    let mut choix: Vec<Choix> = sessions
        .iter()
        .enumerate()
        .map(|(k, x)| Choix::Autre(format!("OUVRIR {} · {} ONGLET{}", x.split('|').next().unwrap_or(""), x.split('|').count() - 1, if x.split('|').count() > 2 { "S" } else { "" }), Autre::OuvrirSession(k)))
        .collect();
    if !choix.is_empty() {
        choix.push(Choix::Separation);
    }
    choix.push(Choix::Commande(Commande::EnregistrerSession, false));
    if !sessions.is_empty() {
        let sup = sessions.iter().enumerate().map(|(k, x)| Choix::Autre(x.split('|').next().unwrap_or("").to_string(), Autre::SupprimerSession(k))).collect();
        choix.push(Choix::Sous("SUPPRIMER UNE SESSION", sup));
    }
    ouvrir_menu(hwnd, f, choix, largeur_volet() + 60.0, LISTE_Y);
    let _ = InvalidateRect(hwnd, None, false);
}

/// Ctrl+Maj+H : le tiroir Historique (lot 15B, `toggleHist` de la maquette) sort ou rentre. Le journal est lu à la première sortie
/// (jamais au démarrage). Remplace l'ancien menu « ANNULER … » : chaque entrée du tiroir porte son propre bouton « Annuler ».
pub(crate) unsafe fn basculer_historique(hwnd: HWND, f: &mut Fenetre) {
    f.historique = !f.historique;
    f.historique_defilement = 0.0;
    if f.historique {
        let _ = journal_historique::instantane();
    }
    let _ = InvalidateRect(hwnd, None, false);
}

/// Sous-menu REGROUPER PAR : aucun, date, type, taille, nom, groupe (celui en cours coché)
pub(crate) fn sous_menu_regrouper(a: &App) -> Vec<Choix> {
    dossier::Regroupement::TOUS
        .iter()
        .enumerate()
        .map(|(k, g)| Choix::Autre(format!("{}{}", if a.regroupement == *g { "● " } else { "" }, g.libelle()), menu::Autre::Regrouper(k as u8)))
        .collect()
}

/// Sous-menu GROUPES : chaque groupe (coché s'il contient déjà les éléments choisis), puis « nouveau groupe »
pub(crate) fn sous_menu_groupes(a: &App) -> Vec<Choix> {
    use menu::Autre;
    let chemins = a.chemins_choisis();
    let mut sous: Vec<Choix> = a
        .groupes
        .iter()
        .enumerate()
        .map(|(k, g)| {
            let coche = if groupes::contient_tous(&g.nom, &chemins) { "● " } else { "" };
            Choix::Autre(format!("{coche}{}", g.nom), Autre::BasculerGroupe(k))
        })
        .collect();
    if !sous.is_empty() {
        sous.push(Choix::Separation);
    }
    sous.push(Choix::Autre("NOUVEAU GROUPE…".into(), Autre::NouveauGroupe));
    sous
}

/// Familles de fichiers qui ont leurs propres actions dans le clic droit
pub(crate) fn famille(ext: &str) -> &'static str {
    match ext.to_ascii_lowercase().as_str() {
        "jpg" | "jpeg" | "png" | "bmp" | "gif" | "webp" | "tif" | "tiff" | "heic" | "jfif" => "image",
        "exe" | "msi" | "bat" | "cmd" | "ps1" | "com" => "programme",
        "pdf" | "txt" | "doc" | "docx" | "rtf" | "odt" | "xls" | "xlsx" | "ppt" | "pptx" => "document",
        _ => "",
    }
}

/// Contenu du clic droit : sur des éléments choisis, ou dans le vide de la liste.
/// Prépare aussi la liste des applications de OUVRIR AVEC.
pub(crate) unsafe fn choix_clic_droit(f: &mut Fenetre, sur_elements: bool) -> Vec<Choix> {
    use Choix::{Autre as A, Commande as C, Separation as S};
    use menu::Autre;
    f.applications.clear();
    let a = &f.app;
    let double = f.second.is_some();
    if sur_elements {
        let plusieurs = a.choisis().len() > 1;
        let e = a.selection.and_then(|i| a.entrees.get(i));
        let conteneur = e.is_some_and(|e| e.est_conteneur());
        let ext = e.filter(|e| !e.est_conteneur()).map_or(String::new(), |e| e.ext.clone());
        let archive = !ext.is_empty() && archive::est_archive(&ext);
        use barre::Outil as O;
        let renommer = if plusieurs { Commande::RenommerLot } else { Commande::Renommer };
        let mut v = vec![
            Choix::Icones(vec![(O::Couper, Commande::Couper), (O::Copier, Commande::Copier), (O::Renommer, renommer), (O::Partager, Commande::Partager), (O::Supprimer, Commande::Supprimer)]),
            S,
            C(Commande::Ouvrir, false),
        ];
        if conteneur {
            v.push(C(Commande::OuvrirOnglet, false));
            if double && !plusieurs {
                v.push(A("OUVRIR DANS L'AUTRE PANNEAU".into(), Autre::AutrePanneau));
            }
        } else if e.is_some() {
            let applications = if ext.is_empty() { Vec::new() } else { coquille::applications(&ext) };
            let mut sous: Vec<Choix> = applications.iter().enumerate().map(|(k, x)| A(x.nom.to_uppercase(), Autre::Application(k))).collect();
            if !sous.is_empty() {
                sous.push(S);
            }
            sous.push(A("CHOISIR UNE AUTRE APPLICATION…".into(), Autre::ChoisirApplication));
            f.applications = applications;
            v.push(Choix::Sous("OUVRIR AVEC", sous));
        }
        if matches!(a.emplacement, Emplacement::Sftp { .. }) && reglages::fonction_experimentale_permise(reglages::experimental(), reglages::Experimentale::Sftp) {
            v.push(A("TELECHARGER DANS TELECHARGEMENTS".into(), Autre::TelechargerSftp));
        }
        if let Some(crate::telechargements::Badge::Copie(_)) = e.and_then(|e| a.badges.get(&e.nom.to_lowercase())).filter(|_| !plusieurs) {
            v.push(A("ALLER A LA COPIE".into(), Autre::AllerCopie));
        }
        if famille(&ext) == "image" || plusieurs && a.choisis().iter().any(|&i| images::est_image(&a.entrees[i].ext)) {
            v.push(Choix::Sous("IMAGES", vec![C(Commande::ImageJpg, false), C(Commande::ImagePng, false), C(Commande::ImageReduire, false), C(Commande::ImageSansMeta, false)]));
        }
        match famille(&ext) {
            "image" if !plusieurs => v.extend([A("DEFINIR COMME FOND D'ECRAN".into(), Autre::Verbe("setdesktopwallpaper")), A("MODIFIER".into(), Autre::Verbe("edit"))]),
            "image" | "document" => v.push(A("IMPRIMER".into(), Autre::Verbe("print"))),
            "programme" => v.push(A("EXECUTER EN TANT QU'ADMINISTRATEUR".into(), Autre::Verbe("runas"))),
            _ => {}
        }
        if double {
            v.extend([S, A("COPIER VERS L'AUTRE PANNEAU".into(), Autre::VersAutre(false)), A("DEPLACER VERS L'AUTRE PANNEAU".into(), Autre::VersAutre(true))]);
        }
        v.extend([S, A("CRÉER UN RACCOURCI".into(), Autre::Verbe("link"))]);
        v.extend([S, Choix::Sous("COPIER LE CHEMIN", vec![C(Commande::CopierChemin, false), C(Commande::CheminUnix, false), C(Commande::CheminWsl, false)])]);
        v.push(Choix::Sous("BALISES", vec![C(Commande::BaliseRouge, false), C(Commande::BaliseBlanche, false), C(Commande::BaliseGrise, false), S, C(Commande::SansBalise, false)]));
        v.push(Choix::Sous("ÉTIQUETTES", vec![C(Commande::Etiquette1, false), C(Commande::Etiquette2, false), C(Commande::Etiquette3, false)]));
        v.push(Choix::Sous("GROUPES", sous_menu_groupes(a)));
        if matches!(a.emplacement, Emplacement::Groupe(_)) {
            v.push(A("RETIRER DU GROUPE".into(), Autre::RetirerDuGroupe));
        }
        v.push(Choix::Sous("COMPRESSER", vec![C(Commande::Zip, false), C(Commande::SeptZip, false)]));
        if archive {
            v.push(C(Commande::Extraire, false));
        }
        if !conteneur && !plusieurs {
            v.push(C(Commande::Sha256, false));
        }
        if !conteneur {
            v.push(C(Commande::Debloquer, false));
        }
        v.extend([S, C(Commande::Proprietes, false), S, Choix::PlusOptions]);
        v
    } else {
        let f = &*f;
        vec![
            Choix::Sous("NOUVEAU", vec![C(Commande::NouveauDossier, false), C(Commande::NouveauFichier, false)]),
            C(Commande::Coller, false),
            C(Commande::Annuler, false),
            C(Commande::Historique, false),
            S,
            Choix::Sous("TRIER PAR", choix_barre(f, barre::Outil::Trier)),
            Choix::Sous("REGROUPER PAR", sous_menu_regrouper(&f.app)),
            Choix::Sous("AFFICHAGE", choix_barre(f, barre::Outil::Afficher)),
            C(Commande::ToutChoisir, false),
            C(Commande::Actualiser, false),
            S,
            C(Commande::Terminal, false),
            C(Commande::VsCode, false),
            C(Commande::CopierChemin, false),
            C(Commande::Epingler, f.app.dossier().is_some_and(|d| reglages::actuels().epingles.iter().any(|e| e.eq_ignore_ascii_case(d)))),
            C(Commande::DoublePanneau, double),
            S,
            C(Commande::Proprietes, false),
            S,
            Choix::PlusOptions,
        ]
    }
}

/// Clic droit, Maj+F10, touche Menu : menu Nothing sur la liste ; menu de Windows ailleurs ou avec Maj.
/// (x, y) en coordonnées écran
pub(crate) unsafe fn menu_contextuel(hwnd: HWND, cible: Cible, x: i32, y: i32) {
    let Some(f) = etat_de(hwnd) else { return };
    let liste = matches!(cible, Cible::Ligne(_) | Cible::FondListe);
    if let Cible::Groupe(k) = cible {
        use menu::Autre;
        use Choix::{Autre as A, Separation as S};
        let couleurs = groupes::COULEURS.iter().map(|&(c, nom)| A(nom.to_string(), Autre::CouleurGroupe(k, c))).collect();
        let choix = vec![
            A("OUVRIR".into(), Autre::OuvrirGroupe(k)),
            A("OUVRIR DANS UN NOUVEL ONGLET".into(), Autre::OngletGroupe(k)),
            S,
            A("RENOMMER…".into(), Autre::RenommerGroupe(k)),
            Choix::Sous("COULEUR", couleurs),
            S,
            A("SUPPRIMER LE GROUPE".into(), Autre::SupprimerGroupe(k)),
        ];
        let mut c = POINT { x, y };
        let _ = ScreenToClient(hwnd, &mut c);
        let (cx, cy) = (f.dip(c.x), f.dip(c.y));
        ouvrir_menu(hwnd, f, choix, cx + 2.0, cy + 2.0);
        let _ = InvalidateRect(hwnd, None, false);
        return;
    }
    // En-tête de la liste : colonnes montrées et date affichée
    if let Cible::Entete(_) = cible {
        use menu::Autre;
        use reglages::{colonne_cachee, colonne_montree, date_montree, COL_AUTEUR, COL_DATE, COL_DIMENSIONS, COL_DUREE, COL_ORIGINE, COL_SIGNE, COL_TAILLE, COL_TYPE};
        use Choix::{Autre as A, Separation as S};
        let coche = |oui: bool, t: &str| if oui { format!("● {t}") } else { t.to_string() };
        let dates = [(0, "LA PLUS RÉCENTE (MODIFIÉ OU ARRIVÉ)"), (1, "DATE DE MODIFICATION"), (2, "DATE DE CRÉATION / D'ARRIVÉE")]
            .into_iter()
            .map(|(k, t)| A(coche(date_montree() == k, t), Autre::DateMontree(k)))
            .collect();
        let choix = vec![
            A(coche(!colonne_cachee(COL_DATE), "DATE"), Autre::BasculerColonne(COL_DATE)),
            A(coche(!colonne_cachee(COL_TYPE), "TYPE"), Autre::BasculerColonne(COL_TYPE)),
            A(coche(!colonne_cachee(COL_TAILLE), "TAILLE"), Autre::BasculerColonne(COL_TAILLE)),
            S,
            A(coche(colonne_montree(COL_DIMENSIONS), "DIMENSIONS (IMAGES, VIDEOS)"), Autre::BasculerColonne(COL_DIMENSIONS)),
            A(coche(colonne_montree(COL_DUREE), "DUREE (AUDIO, VIDEOS)"), Autre::BasculerColonne(COL_DUREE)),
            A(coche(colonne_montree(COL_AUTEUR), "AUTEUR / ARTISTE"), Autre::BasculerColonne(COL_AUTEUR)),
            A(coche(colonne_montree(COL_ORIGINE), "ORIGINE (INTERNET)"), Autre::BasculerColonne(COL_ORIGINE)),
            A(coche(colonne_montree(COL_SIGNE), "SIGNE PAR (PROGRAMMES)"), Autre::BasculerColonne(COL_SIGNE)),
            S,
            Choix::Sous("DATE MONTREE", dates),
            Choix::Sous("TRIER PAR", choix_barre(f, barre::Outil::Trier)),
            Choix::Sous("REGROUPER PAR", sous_menu_regrouper(&f.app)),
        ];
        let mut c = POINT { x, y };
        let _ = ScreenToClient(hwnd, &mut c);
        let (cx, cy) = (f.dip(c.x), f.dip(c.y));
        ouvrir_menu(hwnd, f, choix, cx + 2.0, cy + 2.0);
        let _ = InvalidateRect(hwnd, None, false);
        return;
    }
    // Groupe ouvert : pas de dossier, mais le menu Nothing des éléments reste utile
    let groupe = matches!(f.app.emplacement, Emplacement::Groupe(_) | Emplacement::Recents | Emplacement::Corbeille | Emplacement::Doublons(_));
    if !liste || touche(VK_SHIFT) || (f.app.dossier().is_none() && !groupe) {
        return menu_windows(hwnd, cible, x, y);
    }
    let choix = choix_clic_droit(f, matches!(cible, Cible::Ligne(_)));
    let mut c = POINT { x, y };
    let _ = ScreenToClient(hwnd, &mut c);
    let (cx, cy) = (f.dip(c.x), f.dip(c.y));
    let m = ouvrir_menu(hwnd, f, choix, cx + 2.0, cy + 2.0);
    m.cible = Some((cible, x, y));
    let _ = InvalidateRect(hwnd, None, false);
}

/// Contenu du menu d'un bouton de la barre (coché : état en cours)
pub(crate) fn choix_barre(f: &Fenetre, o: barre::Outil) -> Vec<Choix> {
    use barre::Outil;
    use dossier::Colonne;
    use Choix::{Commande as C, Separation as S};
    let a = &f.app;
    let possible = |o: Outil| a.outil_possible(o);
    match o {
        Outil::Nouveau => vec![C(Commande::NouveauDossier, possible(Outil::Nouveau)), C(Commande::NouveauFichier, possible(Outil::Nouveau)), S, C(Commande::NouvelOnglet, false)],
        Outil::Trier => vec![
            C(Commande::TrierNom, a.tri.0 == Colonne::Nom),
            C(Commande::TrierDate, a.tri.0 == Colonne::Date),
            C(Commande::TrierType, a.tri.0 == Colonne::Type),
            C(Commande::TrierTaille, a.tri.0 == Colonne::Taille),
            S,
            C(Commande::TrierCroissant, a.tri.1),
            C(Commande::TrierDecroissant, !a.tri.1),
            S,
            C(Commande::Melanger, a.tri.2),
        ],
        Outil::Afficher => vec![
            C(Commande::VueListe, a.outil_allume(Outil::VueListe)),
            C(Commande::VueGrille, a.outil_allume(Outil::VueGrille)),
            C(Commande::VueColonnes, a.outil_allume(Outil::VueColonnes)),
            C(Commande::VueTailles, a.outil_allume(Outil::VueTailles)),
            S,
            C(Commande::DoublePanneau, f.second.is_some()),
            C(Commande::Apercu, f.apercu_visible),
            C(Commande::Caches, a.caches),
            S,
            C(Commande::Galerie, false),
        ],
        Outil::Outils => vec![
            C(Commande::Palette, false),
            C(Commande::RechercheGlobale, false),
            S,
            C(Commande::Terminal, false),
            C(Commande::VsCode, false),
            C(Commande::CheminWsl, false),
            C(Commande::CheminUnix, false),
            S,
            C(Commande::Zip, false),
            C(Commande::SeptZip, false),
            C(Commande::Extraire, false),
            C(Commande::Sha256, false),
            C(Commande::Debloquer, false),
            S,
            C(Commande::RenommerLot, false),
            Choix::Sous("BALISES", vec![C(Commande::BaliseRouge, false), C(Commande::BaliseBlanche, false), C(Commande::BaliseGrise, false), S, C(Commande::SansBalise, false)]),
            Choix::Sous("ÉTIQUETTES", vec![C(Commande::Etiquette1, false), C(Commande::Etiquette2, false), C(Commande::Etiquette3, false)]),
            C(Commande::Epingler, f.app.dossier().is_some_and(|d| reglages::actuels().epingles.iter().any(|e| e.eq_ignore_ascii_case(d)))),
            S,
            C(Commande::ReconstruireIndex, false),
        ],
        _ => vec![
            C(Commande::ToutChoisir, false),
            C(Commande::InverserSelection, false),
            S,
            C(Commande::Couper, false),
            C(Commande::Copier, false),
            C(Commande::Coller, false),
            C(Commande::Renommer, false),
            C(Commande::Partager, false),
            C(Commande::Supprimer, false),
            C(Commande::Proprietes, false),
            C(Commande::CopierChemin, false),
            S,
            Choix::Sous("AFFICHER", vec![
                C(Commande::VueListe, a.outil_allume(Outil::VueListe)),
                C(Commande::VueGrille, a.outil_allume(Outil::VueGrille)),
                C(Commande::VueColonnes, a.outil_allume(Outil::VueColonnes)),
                C(Commande::VueTailles, a.outil_allume(Outil::VueTailles)),
                C(Commande::DoublePanneau, f.second.is_some()),
                C(Commande::Apercu, f.apercu_visible),
                C(Commande::Caches, a.caches),
                C(Commande::Galerie, false),
            ]),
            Choix::Sous("OUTILS", vec![
                C(Commande::Palette, false),
                C(Commande::RechercheGlobale, false),
                C(Commande::Terminal, false),
                C(Commande::VsCode, false),
                C(Commande::Zip, false),
                C(Commande::Extraire, false),
            ]),
            S,
            C(Commande::Annuler, false),
            C(Commande::Refaire, false),
            C(Commande::Actualiser, false),
            S,
            C(Commande::Sons, false),
            C(Commande::Reglages, false),
        ],
    }
}
