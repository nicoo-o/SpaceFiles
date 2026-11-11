// Commandes : exécution d'une commande (palette, barre, raccourcis), disponibilité et libellés, verbes du shell.

use crate::*;

mod etapes;
mod fichiers;
mod menus;
mod reglages_page;

pub(crate) use etapes::*;
pub(crate) use fichiers::*;
pub(crate) use menus::*;
pub(crate) use reglages_page::*;
/// Lance une autre fenêtre SpaceFiles avec ces arguments
pub(crate) fn lancer_soi(arguments: &str) {
    if let Ok(exe) = std::env::current_exe() {
        unsafe {
            ShellExecuteW(None, None, &HSTRING::from(exe.as_os_str()), &HSTRING::from(arguments), None, SW_SHOWNORMAL);
        }
    }
}

pub(crate) unsafe fn executer_palette(hwnd: HWND, f: &mut Fenetre) {
    let Some(p) = f.palette.take() else { return };
    let Some(it) = p.items.get(p.choix) else { return };
    // Pixel se plisse (chevrons) à l'instant où la commande part, pendant qu'il revient dans la barre d'état
    f.plisse_depuis = Some(std::time::Instant::now());
    match it.action.clone() {
        palette::Action::Commande(c) => executer(hwnd, f, c),
        palette::Action::Aller(emp) => f.app.naviguer(hwnd, emp),
        // « Créer un nouveau groupe… » : la palette reste ouverte et attend le nom
        palette::Action::NouveauGroupe => {
            let mut p = p;
            p.demander_nom_groupe();
            f.palette = Some(p);
            f.remplir_palette();
        }
        // Ligne d'information (« Tapez le nom ») : Entrée ne ferme pas la palette
        palette::Action::Rien => f.palette = Some(p),
        palette::Action::MettreDansGroupe(nom) => {
            let chemins = if let palette::Mode::Groupe(c) = &p.mode { c.clone() } else { Vec::new() };
            if let Some(nom) = groupes::creer(&nom) {
                let n = groupes::ajouter(&nom, &chemins);
                f.app.annoncer(format!("{nom} · {n} AJOUTE{}", if n > 1 { "S" } else { "" }), false);
                crate::app::toaster(crate::app::phrase_ajoute(n, &nom));
                sons::jouer(sons::Son::Tic);
            }
            rafraichir_groupes(hwnd, f);
        }
        palette::Action::EnregistrerSession(nom) => {
            let onglets: Vec<String> = f.emplacements().iter().filter_map(vers_session).collect();
            let n = onglets.len();
            let ligne = std::iter::once(nom.clone()).chain(onglets).collect::<Vec<_>>().join("|");
            reglages::modifier(|r| {
                r.sessions.retain(|x| x.split('|').next() != Some(nom.as_str()));
                r.sessions.insert(0, ligne);
            });
            f.app.annoncer(format!("SESSION « {nom} » · {n} ONGLET{}", if n > 1 { "S" } else { "" }), false);
            sons::jouer(sons::Son::Tic);
        }
        palette::Action::OuvrirSession(nom) => ouvrir_session(hwnd, f, &nom),
        palette::Action::AjouterRegle(ligne) => {
            reglages::modifier(|r| {
                if !r.regles.contains(&ligne) {
                    r.regles.push(ligne);
                }
            });
            f.app.annoncer("REGLE AJOUTEE · PALETTE › RANGER CE DOSSIER POUR APPLIQUER", false);
            sons::jouer(sons::Son::Tic);
        }
        palette::Action::ChoisirGlyphe(dossiers, rang) => {
            glyphes::choisir(&dossiers, rang);
            let n = dossiers.len();
            f.app.annoncer(match rang {
                Some(k) => format!("GLYPH {} · {n} DOSSIER{}", glyphes::MOTIFS[k].0, if n > 1 { "S" } else { "" }),
                None => "GLYPH ENLEVE".to_string(),
            }, false);
            sons::jouer(sons::Son::Tic);
        }
        palette::Action::EnregistrerNote(dossier, texte) => {
            reglages::noter_dossier(&dossier, &texte);
            let note = reglages::note_du_dossier(&dossier);
            for a in f.tous() {
                if a.emplacement == Emplacement::Dossier(dossier.clone()) {
                    a.note = note.clone();
                }
            }
            sons::jouer(sons::Son::Tic);
        }
        palette::Action::RenommerGroupe(ancien, nouveau) => {
            if let Some(n) = groupes::renommer(&ancien, &nouveau) {
                for a in f.tous() {
                    if a.emplacement == Emplacement::Groupe(ancien.clone()) {
                        a.emplacement = Emplacement::Groupe(n.clone());
                    }
                }
            }
            rafraichir_groupes(hwnd, f);
        }
        // Les actions d'étape (élément, commande, destination…) passent par `Palette::entrer`, jamais par ici
        _ => {}
    }
}

/// Le plateau (Z10) est un groupe ordinaire de ce nom : zone de passage où l'on pose des éléments de plusieurs dossiers
pub(crate) const PLATEAU: &str = "PLATEAU";

/// Ouvre ou ferme le panneau terminal. Son contenu vient du journal des opérations : aucune commande
/// n'est simulée. L'ouverture dans Windows Terminal reste une action séparée du panneau.
pub(crate) fn basculer_terminal(f: &mut Fenetre) {
    if f.terminal_visible {
        f.terminal_visible = false;
        return;
    }
    if f.app.dossier_ici().is_some() {
        f.terminal_lignes = journal::dernieres_operations(5);
        f.terminal_visible = true;
    } else {
        f.app.annoncer("PAS DE DOSSIER ICI", true);
    }
}

/// « Ouvrir dans Windows Terminal » depuis le panneau : ouvre le vrai terminal dans le dossier courant.
pub(crate) fn terminal_externe(f: &mut Fenetre) {
    match f.app.dossier_ici() {
        Some(d) => {
            outils::terminal(&d);
            f.app.annoncer("TERMINAL OUVERT ICI", false);
            crate::app::toaster("Terminal ouvert ici");
        }
        None => f.app.annoncer("PAS DE DOSSIER ICI", true),
    }
}

/// Exécute une commande (palette ou raccourci)
pub(crate) unsafe fn executer(hwnd: HWND, f: &mut Fenetre, c: Commande) {
    // Sauvegarde NAS et MTP : fermés tant que `experimental` est faux, même par un raccourci choisi dans les réglages
    if !palette::commande_permise(reglages::experimental(), c) {
        f.app.annoncer(reglages::MESSAGE_EXPERIMENTAL, true);
        return;
    }
    match c {
        Commande::NouvelOnglet => {
            let emp = f.app.emplacement.clone();
            f.nouvel_onglet(hwnd, emp);
        }
        Commande::FermerOnglet => {
            let k = f.rang;
            f.fermer_onglet(hwnd, k);
        }
        Commande::RouvrirOnglet => f.rouvrir_onglet(hwnd),
        Commande::Reglages => ouvrir_reglages(hwnd, f),
        Commande::Terminal => basculer_terminal(f),
        Commande::VsCode => {
            // Le fichier choisi s'il y en a un, sinon le dossier
            let cible = f.app.selection.and_then(|i| f.app.entrees.get(i)).filter(|e| !e.est_conteneur() && f.app.marques.len() == 1).and_then(|e| f.app.chemin_de(e));
            match cible.or_else(|| f.app.dossier_ici()) {
                Some(d) if outils::vscode(&d) => {
                    f.app.annoncer("OUVERT DANS VS CODE", false);
                    crate::app::toaster("Ouvert dans VS Code");
                }
                Some(_) => f.app.annoncer("VS CODE INTROUVABLE (COMMANDE « code »)", true),
                None => f.app.annoncer("PAS DE DOSSIER ICI", true),
            }
        }
        Commande::CheminWsl => f.app.copier_chemin_format(hwnd, true),
        Commande::CheminUnix => f.app.copier_chemin_format(hwnd, false),
        Commande::CopierChemin => f.app.copier_chemin(hwnd),
        Commande::Sha256 => f.app.empreinte(hwnd),
        Commande::Debloquer => f.app.debloquer(),
        Commande::ImageJpg => f.app.traiter_images(hwnd, images::Traitement::Jpg),
        Commande::ImagePng => f.app.traiter_images(hwnd, images::Traitement::Png),
        Commande::ImageReduire => f.app.traiter_images(hwnd, images::Traitement::Reduire(1920)),
        Commande::ImageSansMeta => f.app.traiter_images(hwnd, images::Traitement::SansMeta),
        Commande::Zip => f.app.compresser(hwnd, "zip"),
        Commande::SeptZip => f.app.compresser(hwnd, "7z"),
        Commande::Extraire => f.app.extraire(hwnd),
        Commande::RenommerLot => ouvrir_lot(f),
        Commande::Epingler => f.epingler(),
        Commande::BaliseRouge => f.app.baliser(Some(Balise::Rouge)),
        Commande::BaliseBlanche => f.app.baliser(Some(Balise::Blanc)),
        Commande::BaliseGrise => f.app.baliser(Some(Balise::Gris)),
        Commande::SansBalise => f.app.baliser(None),
        Commande::Etiquette1 => f.app.etiqueter(1),
        Commande::Etiquette2 => f.app.etiqueter(2),
        Commande::Etiquette3 => f.app.etiqueter(3),
        Commande::VueListe => {
            f.app.choisir_vue(hwnd, Affichage::Liste);
            if f.app.tailles {
                f.app.basculer_tailles(hwnd);
            }
        }
        Commande::VueGrille => f.app.choisir_vue(hwnd, Affichage::Grille),
        Commande::VueColonnes => f.app.choisir_vue(hwnd, Affichage::Colonnes),
        Commande::VueTailles => {
            // Les tailles se lisent dans la liste
            if !f.app.tailles {
                f.app.changer_affichage(hwnd, Affichage::Liste);
            }
            f.app.basculer_tailles(hwnd);
        }
        Commande::Couper => f.app.copier(hwnd, true),
        Commande::Copier => f.app.copier(hwnd, false),
        Commande::Coller => f.app.coller(hwnd),
        Commande::Renommer => {
            if f.app.choisis().len() > 1 {
                ouvrir_lot(f);
            } else {
                f.app.commencer_renommage();
            }
        }
        Commande::Supprimer => f.app.supprimer(hwnd, false),
        Commande::Ouvrir => {
            if let Some(i) = f.app.selection {
                f.app.ouvrir(hwnd, i);
            }
        }
        Commande::OuvrirOnglet => {
            if let Some(ch) = f.app.selection.and_then(|i| f.app.chemin_cible(Cible::Ligne(i))) {
                f.nouvel_onglet(hwnd, Emplacement::dossier(&ch));
            }
        }
        Commande::Melanger => f.app.basculer_melange(),
        Commande::Historique => basculer_historique(hwnd, f),
        Commande::Comparer => comparer_panneaux(hwnd, f, false),
        Commande::Synchroniser => comparer_panneaux(hwnd, f, true),
        Commande::EnregistrerSession => {
            let n = f.emplacements().iter().filter_map(vers_session).count();
            if n == 0 {
                f.app.annoncer("AUCUN ONGLET A ENREGISTRER (DOSSIERS SEULEMENT)", true);
            } else {
                f.palette = Some(Palette::session(n));
                f.remplir_palette();
            }
        }
        Commande::Sessions => menu_sessions(hwnd, f),
        Commande::Note => ouvrir_note(f),
        Commande::RegleRangement => {
            f.palette = Some(Palette::regle());
            f.remplir_palette();
        }
        Commande::Ranger => f.app.ranger_ici(hwnd),
        Commande::Ejecter => {
            // Le disque du dossier ouvert s'il est amovible, sinon le premier amovible du volet
            let racines = f.app.disques.iter().filter_map(|d| match &d.genre {
                dossier::Genre::Disque { racine, .. } => Some(racine.clone()),
                _ => None,
            });
            let ici = f.app.dossier().map(str::to_string).filter(|d| operations::amovible(d));
            let Some(racine) = ici.or_else(|| racines.into_iter().find(|r| operations::amovible(r))) else {
                f.app.annoncer("AUCUN DISQUE AMOVIBLE A EJECTER", true);
                return;
            };
            let Some(lettre) = outils::lettre_lecteur(&racine) else { return };
            // Nos propres onglets tiennent le disque (surveillance du dossier) : ils le quittent d'abord
            let accueil = std::env::var("USERPROFILE").unwrap_or_else(|_| "C:\\".into());
            for a in f.tous() {
                if a.dossier().and_then(outils::lettre_lecteur) == Some(lettre) {
                    a.naviguer(hwnd, Emplacement::dossier(&accueil));
                }
            }
            app::tache(hwnd, move || match outils::ejecter(lettre) {
                Ok(()) => app::Fini { message: format!("{lettre}: EJECTE : TU PEUX LE RETIRER"), erreur: false, copier: None, lancer: None },
                Err(m) => app::Fini { message: m, erreur: true, copier: None, lancer: None },
            });
        }
        Commande::NonFiable => {
            let Emplacement::Dossier(d) = f.app.emplacement.clone() else {
                f.app.annoncer("NON FIABLE : OUVRIR UN DOSSIER", true);
                return;
            };
            let exact = reglages::actuels().non_fiables.iter().any(|x| x.eq_ignore_ascii_case(&d));
            if !exact && reglages::non_fiable(&d) {
                f.app.annoncer("DEJA NON FIABLE : UN DOSSIER PARENT L'EST", false);
                return;
            }
            reglages::modifier(|r| {
                if exact {
                    r.non_fiables.retain(|x| !x.eq_ignore_ascii_case(&d));
                } else {
                    r.non_fiables.push(d.clone());
                }
            });
            // L'aperçu déjà affiché est refait (ou remplacé par l'avis) sans attendre un changement de sélection
            f.apercu_demande = None;
            f.app.actualiser(hwnd);
            f.app.annoncer(if exact { "DOSSIER REDEVENU FIABLE" } else { "DOSSIER NON FIABLE : PAS D'APERCU, DE VIGNETTES, DE PROPRIETES NI DE GIT" }, false);
        }
        Commande::Arbre => basculer_arbre(f),
        Commande::Essentiel => {
            let oui = !app::ESSENTIEL.load(Ordering::Relaxed);
            app::ESSENTIEL.store(oui, Ordering::Relaxed);
            for a in f.tous() {
                a.rebatir_garder();
            }
            f.app.annoncer(if oui { "MODE ESSENTIEL · 7 DERNIERS JOURS ET EPINGLES · F9 POUR TOUT REVOIR" } else { "MODE ESSENTIEL COUPE" }, false);
            sons::jouer(sons::Son::Tic);
        }
        Commande::GlyphDossier => {
            let mut dossiers: Vec<String> = f.app.chemins_choisis().into_iter().filter(|c| std::path::Path::new(c).is_dir()).collect();
            if dossiers.is_empty() {
                dossiers.extend(f.app.dossier().map(str::to_string));
            }
            if dossiers.is_empty() {
                f.app.annoncer("GLYPH : CHOISIR UN DOSSIER", true);
            } else {
                f.palette = Some(Palette::glyphe(dossiers));
                f.remplir_palette();
            }
        }
        Commande::ViderCorbeille => vider_corbeille(hwnd, f),
        Commande::LignesDossier => {
            let Some(d) = f.app.dossier().map(str::to_string) else {
                f.app.annoncer("TAILLE DES LIGNES : OUVRIR UN DOSSIER", true);
                return;
            };
            let k = if reglages::lignes_confort() { 0 } else { 1 };
            reglages::memoriser_disposition(&d, reglages::COLONNES.load(Ordering::Relaxed), k);
            f.app.annoncer(&format!("LIGNES {} DANS CE DOSSIER", reglages::TAILLES_LIGNES[k as usize]), false);
        }
        Commande::DossierParDefaut => {
            if let Some(d) = f.app.dossier().map(str::to_string) {
                reglages::oublier_disposition(&d);
                f.app.annoncer("CE DOSSIER REPREND LES COLONNES ET LES LIGNES PAR DEFAUT", false);
            }
        }
        Commande::VoirPanneau => f.app.naviguer(hwnd, Emplacement::Panneau),
        Commande::AjouterPlateau => {
            let chemins = if f.app.sans_fichiers() { Vec::new() } else { f.app.chemins_choisis() };
            if chemins.is_empty() {
                f.app.annoncer("CHOISISSEZ D'ABORD DES ELEMENTS", true);
            } else if let Some(nom) = groupes::creer(PLATEAU) {
                let n = groupes::ajouter(&nom, &chemins);
                f.rafraichir_lieux();
                f.app.annoncer(format!("{n} AJOUTE{} AU PLATEAU · {} EN TOUT", if n > 1 { "S" } else { "" }, groupes::membres(&nom).len()), false);
            }
        }
        Commande::ComparerTextes => comparer_textes(hwnd, f),
        Commande::ArracherOnglet => match (&f.app.emplacement, f.nb_onglets() > 1) {
            (Emplacement::Dossier(c), true) => {
                lancer_soi(&format!("\"{}\"", c.trim_end_matches('\\')));
                let k = f.rang;
                f.fermer_onglet(hwnd, k);
            }
            (_, false) => f.app.annoncer("IL N'Y A QU'UN ONGLET", true),
            _ => f.app.annoncer("SEUL UN DOSSIER SE DETACHE", true),
        },
        Commande::RattacherFenetre => {
            let dossiers: Vec<String> = f.emplacements().into_iter().filter_map(|e| if let Emplacement::Dossier(c) = e { Some(c) } else { None }).collect();
            if dossiers.is_empty() {
                f.app.annoncer("AUCUN DOSSIER A ENVOYER", true);
            } else if envoyer_onglets(hwnd, &dossiers) {
                let _ = PostMessageW(hwnd, WM_CLOSE, WPARAM(0), LPARAM(0));
            } else {
                f.app.annoncer("AUCUNE AUTRE FENETRE SPACEFILES", true);
            }
        }
        Commande::SauvegardeDestination => {
            if let Some(d) = systeme::choisir(hwnd, true) {
                reglages::modifier(|r| r.sauvegarde_dest = d.clone());
                f.app.annoncer(format!("DESTINATION DE SAUVEGARDE : {}", d.to_uppercase()), false);
            }
        }
        Commande::SauvegardeAjouter => match &f.app.emplacement {
            Emplacement::Dossier(c) => {
                let c = c.clone();
                reglages::modifier(|r| {
                    if !r.sauvegarde.contains(&c) {
                        r.sauvegarde.push(c.clone());
                    }
                });
                f.app.annoncer(format!("{} DOSSIER(S) DANS LA LISTE DE SAUVEGARDE", reglages::actuels().sauvegarde.len()), false);
            }
            _ => f.app.annoncer("OUVREZ UN DOSSIER POUR L'AJOUTER", true),
        },
        Commande::SauvegardeVider => {
            reglages::modifier(|r| r.sauvegarde.clear());
            f.app.annoncer("LISTE DE SAUVEGARDE VIDEE", false);
        }
        Commande::LireTexteImage => lire_texte_image(hwnd, f),
        Commande::ListerTelephones => match mtp::appareils() {
            Ok(n) if n.is_empty() => f.app.annoncer("AUCUN TELEPHONE OU APPAREIL MTP BRANCHE", false),
            Ok(n) => f.app.annoncer(format!("APPAREILS : {}", n.join(" · ").to_uppercase()), false),
            Err(e) => f.app.annoncer(e, true),
        },
        Commande::SauvegardeLancer => f.app.sauvegarder(hwnd),
        Commande::ModeCapture => {
            let actif = !reglages::MODE_CAPTURE.fetch_xor(true, std::sync::atomic::Ordering::Relaxed);
            f.app.annoncer(if actif { "MODE CAPTURE : NOMS CHOISIS MASQUES" } else { "MODE CAPTURE ARRETE" }, false);
        }
        Commande::EpinglerAccesRapide => {
            let dossiers = reglages::actuels().epingles;
            let n = dossiers.iter().filter(|d| systeme::verbe_dossier(d, "pintohome")).count();
            f.app.annoncer(format!("{n} DOSSIER(S) EPINGLE(S) A L'ACCES RAPIDE DE WINDOWS"), n == 0 && !dossiers.is_empty());
        }
        Commande::OuvrirJournalOperations => match journal::fichier_operations().filter(|p| p.exists()) {
            Some(p) => systeme::lancer(hwnd, &p.to_string_lossy()),
            None => f.app.annoncer("AUCUNE OPERATION JOURNALISEE POUR L'INSTANT", false),
        },
        Commande::VoirPlateau =>f.app.naviguer(hwnd, Emplacement::Groupe(PLATEAU.into())),
        Commande::DeposerPlateau => f.app.deposer_plateau(hwnd, false),
        Commande::DeplacerPlateau => f.app.deposer_plateau(hwnd, true),
        Commande::ViderPlateau => {
            groupes::supprimer(PLATEAU);
            f.rafraichir_lieux();
            f.app.annoncer("PLATEAU VIDE (LES FICHIERS NE SONT PAS TOUCHES)", false);
        }
        Commande::VoirCorbeille => f.app.naviguer(hwnd, Emplacement::Corbeille),
        Commande::Restaurer => {
            if f.app.emplacement != Emplacement::Corbeille {
                f.app.annoncer("RESTAURER : OUVRIR D'ABORD LA CORBEILLE", true);
                return;
            }
            let origines: Vec<String> = f.app.choisis().into_iter().filter_map(|i| f.app.entrees.get(i)).filter_map(|e| Some(dossier::joindre(e.parent.as_deref()?, &e.nom))).collect();
            if origines.is_empty() {
                f.app.annoncer("CHOISISSEZ CE QU'IL FAUT RESTAURER", true);
                return;
            }
            let (mut faits, mut erreur) = (0, None);
            for o in &origines {
                match corbeille::restaurer(o) {
                    Ok(()) => faits += 1,
                    Err(m) => erreur = Some(m),
                }
            }
            f.app.actualiser(hwnd);
            let s = if faits > 1 { "S" } else { "" };
            f.app.annoncer(erreur.map_or(format!("{faits} ELEMENT{s} RESTAURE{s}"), |m| format!("{faits} RESTAURE · {m}")), faits == 0);
        }
        Commande::PoidsSonore => {
            reglages::modifier(|r| r.poids_sonore = !r.poids_sonore);
            let oui = reglages::actuels().poids_sonore;
            f.app.annoncer(if oui { "POIDS SONORE : UN TIC AU SURVOL DES DOSSIERS" } else { "POIDS SONORE COUPE" }, false);
        }
        Commande::PartageProche => partage_proche(hwnd, f),
        Commande::Chronologie => {
            reglages::modifier(|r| r.chronologie = !r.chronologie);
            let oui = reglages::actuels().chronologie;
            for a in f.tous() {
                if !oui && a.jour.is_some() {
                    a.jour = None;
                    a.rebatir_garder();
                }
            }
            f.app.annoncer(if oui { "CHRONOLOGIE EN POINTS · CLIC SUR UN JOUR POUR LE FILTRER" } else { "CHRONOLOGIE MASQUEE" }, false);
        }
        Commande::Doublons => {
            if let Some(d) = f.app.dossier().map(str::to_string) {
                f.app.naviguer(hwnd, Emplacement::Doublons(d));
            }
        }
        Commande::Regrouper(k) => {
            if f.app.affichage != Affichage::Liste {
                f.app.changer_affichage(hwnd, Affichage::Liste);
            }
            f.app.regrouper(hwnd, dossier::Regroupement::TOUS[k as usize]);
        }
        Commande::NouveauGroupe => demander_groupe(f),
        Commande::TaillesDossiers => f.app.calculer_tailles(hwnd),
        Commande::Administrateur => {
            // Une nouvelle fenêtre élevée sur le même dossier (Windows demande la confirmation)
            let dossier = f.app.dossier().map(str::to_string).unwrap_or_default();
            if let Ok(exe) = std::env::current_exe() {
                let exe = HSTRING::from(exe.as_os_str());
                let args = HSTRING::from(format!("\"{dossier}\""));
                ShellExecuteW(hwnd, w!("runas"), &exe, &args, windows::core::PCWSTR::null(), SW_SHOWNORMAL);
            }
        }
        Commande::OuvrirAvec => action_autre(hwnd, f, menu::Autre::ChoisirApplication),
        Commande::Partager => verbe_shell(hwnd, f, "Windows.ModernShare"),
        Commande::Proprietes => verbe_shell(hwnd, f, "properties"),
        Commande::NouveauFichier => f.app.nouveau_fichier(),
        Commande::TrierNom => f.app.trier(dossier::Colonne::Nom, f.app.tri.1),
        Commande::TrierDate => f.app.trier(dossier::Colonne::Date, f.app.tri.1),
        Commande::TrierType => f.app.trier(dossier::Colonne::Type, f.app.tri.1),
        Commande::TrierTaille => f.app.trier(dossier::Colonne::Taille, f.app.tri.1),
        Commande::TrierCroissant => f.app.trier(f.app.tri.0, true),
        Commande::TrierDecroissant => f.app.trier(f.app.tri.0, false),
        Commande::ToutChoisir => f.app.tout_choisir(),
        Commande::InverserSelection => f.app.inverser_selection(),
        Commande::Palette => ouvrir_palette(f),
        Commande::DoublePanneau => f.double_panneau(hwnd),
        Commande::CopierVersAutre => f.vers_autre(hwnd, false),
        Commande::Galerie => ouvrir_galerie(hwnd, f),
        Commande::RechercheGlobale => recherche_globale(hwnd, f),
        Commande::ReconstruireIndex => {
            index::reconstruire();
            f.app.annoncer("RECONSTRUCTION DE L'INDEX EN ARRIERE-PLAN", false);
        }
        Commande::ExclureRecherche => {
            if let Some(d) = f.app.dossier().map(str::to_string) {
                let x = reglages::forme_exclu(&d);
                reglages::modifier(|r| {
                    if !r.exclus.contains(&x) {
                        r.exclus.push(x.clone());
                    }
                });
                f.app.annoncer(format!("{} EXCLU DE LA RECHERCHE · REGLAGES › RECHERCHE POUR LE REMETTRE", dossier::nom_affiche(&d).to_uppercase()), false);
            }
        }
        Commande::Caches => {
            f.app.caches = !f.app.caches;
            f.app.charger(hwnd);
        }
        Commande::Apercu => {
            f.apercu_visible = !f.apercu_visible;
            f.apercu_demande = None;
        }
        Commande::NouveauDossier => f.app.nouveau_dossier(),
        Commande::Actualiser => f.app.charger(hwnd),
        Commande::Annuler => f.app.annuler_derniere(hwnd),
        Commande::Refaire => f.app.refaire(hwnd),
        Commande::Sons => {
            let actifs = !sons::ACTIFS.load(Ordering::Relaxed);
            reglages::modifier(|r| r.sons = actifs);
            f.app.annoncer(if actifs { "SONS ACTIVES" } else { "SONS COUPES" }, false);
        }
    }
    sons::jouer(sons::Son::Tic);
}

/// Partager, Propriétés : verbe du menu Windows sur les éléments choisis (Propriétés : le dossier s'il n'y en a pas)
pub(crate) unsafe fn verbe_shell(hwnd: HWND, f: &mut Fenetre, verbe: &str) {
    let mut chemins = if f.app.sans_fichiers() { Vec::new() } else { f.app.chemins_choisis() };
    if chemins.is_empty() && verbe == "properties" {
        chemins.extend(f.app.dossier().map(str::to_string));
    }
    if chemins.is_empty() {
        f.app.annoncer("CHOISISSEZ D'ABORD UN ELEMENT", true);
        return;
    }
    if let Err(e) = coquille::invoquer(hwnd, &chemins, verbe) {
        f.app.annoncer(operations::message_com(&e), true);
    }
}

/// Libellé « TEXTE\tRACCOURCI » d'une commande, pris dans la table de la palette
pub(crate) fn libelle_commande(c: Commande) -> String {
    if c == Commande::Palette {
        return "PALETTE DE COMMANDES\tCTRL+K".into();
    }
    match palette::COMMANDES.iter().find(|x| x.0 == c) {
        Some((_, l, r, _)) if !r.is_empty() => format!("{l}\t{r}"),
        Some((_, l, _, _)) => l.to_string(),
        None => format!("{c:?}").to_uppercase(),
    }
}

/// Commande grisée dans un menu de la barre (même règle que les boutons)
pub(crate) fn commande_possible(a: &App, c: Commande) -> bool {
    use barre::Outil;
    match c {
        Commande::NouveauDossier | Commande::NouveauFichier => a.outil_possible(Outil::Nouveau),
        Commande::Couper => a.outil_possible(Outil::Couper),
        Commande::Renommer => a.outil_possible(Outil::Renommer),
        Commande::Partager => a.outil_possible(Outil::Partager),
        Commande::Proprietes => a.outil_possible(Outil::Proprietes),
        Commande::Epingler => a.dossier().is_some() || matches!(a.emplacement, Emplacement::Globale { .. }),
        Commande::Coller => a.outil_possible(Outil::Coller),
        Commande::Supprimer => a.outil_possible(Outil::Supprimer),
        Commande::Copier => a.outil_possible(Outil::Copier),
        c => palette::commande_permise(reglages::experimental(), c),
    }
}

/// Clic sur un bouton de la barre d'actions : commande directe, ou menu sous le bouton
pub(crate) unsafe fn clic_outil(hwnd: HWND, f: &mut Fenetre, o: barre::Outil) {
    if !f.app.outil_possible(o) {
        return;
    }
    if let Some(c) = o.commande() {
        executer(hwnd, f, c);
        return;
    }
    let Some(p) = f.app.zones.barre.iter().find(|p| p.outil == o).copied() else { return };
    // Sous le bouton (panneau de droite : décalé)
    let (lt, _) = taille_client(hwnd);
    let decalage = if f.second.is_some() && f.focus_droite { largeur_panneau(largeur_liste(f.dip(lt as i32), f.apercu_visible), true) - largeur_volet() } else { 0.0 };
    let choix = choix_barre(f, o);
    let m = ouvrir_menu(hwnd, f, choix, p.zone.left + decalage, p.zone.bottom + 4.0);
    m.outil = Some(o);
    f.app.menu_barre = Some(o);
    let _ = InvalidateRect(hwnd, None, false);
}

/// Ctrl+Maj+F : le filtre tapé est cherché dans tout le PC (index)
pub(crate) unsafe fn recherche_globale(hwnd: HWND, f: &mut Fenetre) {
    let motif = f.app.filtre.trim().to_string();
    if motif.is_empty() {
        index::demarrer();
        f.app.annoncer("TAPEZ CE QUE VOUS CHERCHEZ, PUIS CTRL+MAJ+F · FILTRES : EXT:PDF TAILLE>10MO MODIF:7 DOSSIER: CONTENU:…", false);
        return;
    }
    f.app.naviguer(hwnd, Emplacement::Globale { motif });
}
