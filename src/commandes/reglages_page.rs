// Page des réglages et ses formulaires : ouverture, activation d'un réglage, stockages, soumission.

use crate::*;
/// Poids sonore (O4) : le survol d'un dossier fait un tic dont la hauteur dépend de sa taille
pub(crate) static SURVOL_NUMERO: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

pub(crate) fn poids_sonore(f: &Fenetre, i: usize) {
    let n = SURVOL_NUMERO.fetch_add(1, Ordering::Relaxed) + 1;
    if !reglages::POIDS_SONORE.load(Ordering::Relaxed) {
        return;
    }
    let Some(e) = f.app.entrees.get(i).filter(|e| matches!(e.genre, dossier::Genre::Dossier)) else { return };
    let Some(chemin) = f.app.chemin_de(e) else { return };
    let connue = e.taille;
    std::thread::spawn(move || {
        // Un court délai : traverser la liste en vitesse ne fait pas une rafale de tics
        std::thread::sleep(std::time::Duration::from_millis(70));
        if SURVOL_NUMERO.load(Ordering::Relaxed) != n {
            return;
        }
        let taille = if connue > 0 { Some(connue) } else { index::taille_dossier(&chemin) };
        if let Some(t) = taille.filter(|_| SURVOL_NUMERO.load(Ordering::Relaxed) == n) {
            sons::jouer_poids(t);
        }
    });
}

/// Palette de la note du dossier ouvert (O8)
pub(crate) fn ouvrir_note(f: &mut Fenetre) {
    if let Emplacement::Dossier(d) = f.app.emplacement.clone() {
        let texte = f.app.note.clone().unwrap_or_default();
        f.palette = Some(Palette::note(&d, &texte));
        f.remplir_palette();
    } else {
        f.app.annoncer("NOTE : OUVRIR UN DOSSIER", true);
    }
}

/// Ouvre la page des réglages dans l'onglet affiché
pub(crate) unsafe fn ouvrir_reglages(hwnd: HWND, f: &mut Fenetre) {
    if f.app.emplacement != Emplacement::Reglages {
        f.page = Page::nouvelle();
        f.app.naviguer(hwnd, Emplacement::Reglages);
    }
}

/// F1 : la page Raccourcis des réglages, qui liste toutes les touches (dont Espace, Ctrl+K, Tab)
pub(crate) unsafe fn ouvrir_aide(hwnd: HWND, f: &mut Fenetre) {
    ouvrir_reglages(hwnd, f);
    let lignes = f.lignes();
    if let Some(k) = crate::page::CATEGORIES.iter().position(|&c| c == "Raccourcis") {
        f.page.aller_categorie(&lignes, k);
    }
}

/// « Chercher un réglage » : la palette liste les réglages de toutes les pages (titre et détail), la requête les filtre
pub(crate) fn chercher_reglage(f: &mut Fenetre) {
    let lignes = f.lignes();
    let mut liste = Vec::new();
    for cat in 0..crate::page::CATEGORIES.len() {
        for k in crate::page::plage(&lignes, cat) {
            let l = &lignes[k];
            // Les lignes d'information (touches, données) ne se règlent pas : la page Raccourcis a sa commande d'aide
            if l.element.choisissable() && !l.libelle.is_empty() {
                liste.push((cat, k, l.libelle.clone(), l.detail.clone()));
            }
        }
    }
    f.palette = Some(Palette::reglage(liste));
    f.remplir_palette();
}

/// Ouvre la page `cat` des réglages et y choisit la ligne `rang`
pub(crate) unsafe fn aller_reglage(hwnd: HWND, f: &mut Fenetre, cat: usize, rang: usize) {
    ouvrir_reglages(hwnd, f);
    let lignes = f.lignes();
    f.page.aller_categorie(&lignes, cat);
    if lignes.get(rang).is_some_and(|l| l.element.choisissable()) && crate::page::plage(&lignes, cat).contains(&rang) {
        f.page.focus = rang;
    }
    let hauteur = hauteur_page(hwnd, f);
    f.page.montrer(&lignes, hauteur);
}

/// « + Ajouter un stockage » de Ce PC : la page Stockages des réglages, formulaire ouvert
pub(crate) unsafe fn formulaire_stockage(hwnd: HWND, f: &mut Fenetre) {
    ouvrir_reglages(hwnd, f);
    let lignes = f.lignes();
    if let Some(k) = crate::page::CATEGORIES.iter().position(|&c| c == "Stockages") {
        f.page.aller_categorie(&lignes, k);
    }
    f.page.formulaire = Some(Formulaire::nouveau());
}

/// Quitte la page des réglages (Échap) : retour au dossier précédent
pub(crate) unsafe fn quitter_reglages(hwnd: HWND, f: &mut Fenetre) {
    if f.app.peut_reculer() {
        f.app.precedent(hwnd);
    } else {
        let emp = depart_reglage(&reglages::actuels());
        f.app.naviguer(hwnd, emp);
    }
}

pub(crate) fn fermer_formulaire(f: &mut Fenetre) {
    if let Some(mut fo) = f.page.formulaire.take() {
        fo.oublier_mot_de_passe();
    }
}

/// Change le réglage de la ligne `k` (sens : 0 = Entrée / clic, -1 / +1 = flèches)
pub(crate) unsafe fn activer_reglage(hwnd: HWND, f: &mut Fenetre, k: usize, sens: i32) {
    let lignes = f.lignes();
    let Some(ligne) = lignes.get(k) else { return };
    let element = ligne.element;
    let pas = if sens < 0 { -1 } else { 1 };
    let suivant = |n: usize, i: usize| ((i as i32 + pas).rem_euclid(n as i32)) as usize;
    let mut annonce: Option<String> = None;
    match element {
        Element::Section(_) | Element::Version | Element::Info => return,
        // Page Raccourcis : la ligne attend la nouvelle touche (voir `capturer_touche`)
        Element::Raccourci(i) => {
            f.page.capture = Some(i);
            f.app.annoncer("APPUYEZ SUR LA NOUVELLE TOUCHE · ECHAP ANNULE · SUPPR RETABLIT L'ORIGINE", false);
            return;
        }
        // Réglages à contrôle segmenté : Entrée / Espace passe au segment suivant (en boucle), les flèches vont au voisin
        // sans boucler (l'ordre des segments à l'écran n'est pas celui des valeurs enregistrées)
        Element::SectionVolet(_) | Element::Theme | Element::TailleLignes | Element::VueDefaut | Element::Mouvement | Element::PixelHeure => {
            let page::Valeur::Segments(options, actif) = &ligne.valeur else { return };
            let (n, a) = (options.len(), *actif);
            let s = if sens == 0 { (a + 1) % n } else if sens < 0 { a.saturating_sub(1) } else { (a + 1).min(n - 1) };
            appliquer_segment(element, s);
        }
        Element::Epingle(i) => {
            if let Some(c) = reglages::actuels().epingles.get(i).cloned() {
                f.app.naviguer(hwnd, Emplacement::dossier(&c));
            }
            return;
        }
        Element::ConflitDefaut => reglages::modifier(|r| r.conflit_defaut = suivant(5, r.conflit_defaut as usize) as u32),
        Element::Extensions => reglages::modifier(|r| r.extensions = !r.extensions),
        Element::Contraste => reglages::modifier(|r| r.contraste = !r.contraste),
        Element::Trame => reglages::modifier(|r| r.trame = !r.trame),
        Element::RefletSurvol => reglages::modifier(|r| r.reflet = !r.reflet),
        Element::YeuxPixel => reglages::modifier(|r| r.yeux_pixel = !r.yeux_pixel),
        Element::DisqueIndexe(i) => {
            if let Some(d) = index::disques().get(i) {
                let x = reglages::forme_exclu(d);
                let remis = reglages::actuels().exclus.contains(&x);
                reglages::modifier(|r| {
                    if remis {
                        r.exclus.retain(|y| y != &x);
                    } else {
                        r.exclus.push(x.clone());
                    }
                });
                // Remis dans la recherche : ses fichiers n'y sont qu'après une nouvelle lecture des disques
                if remis {
                    index::reconstruire();
                    annonce = Some("DISQUE REMIS · INDEX EN RECONSTRUCTION".into());
                }
            }
        }
        Element::Exclu(i) => {
            reglages::modifier(|r| {
                if i < r.exclus.len() {
                    r.exclus.remove(i);
                }
            });
            index::reconstruire();
            annonce = Some("DOSSIER REMIS DANS LA RECHERCHE · INDEX EN RECONSTRUCTION".into());
        }
        Element::Reconstruire => {
            index::reconstruire();
            annonce = Some("RECONSTRUCTION DE L'INDEX EN ARRIERE-PLAN".into());
        }
        Element::RetenirRecents => reglages::modifier(|r| r.retenir_recents = !r.retenir_recents),
        Element::EffacerRecents => {
            reglages::modifier(|r| r.recents.clear());
            annonce = Some("DOSSIERS RECENTS OUBLIES".into());
        }
        Element::EffacerIndex => {
            index::effacer();
            annonce = Some("INDEX DE RECHERCHE SUPPRIME".into());
        }
        Element::Detecte(i) => {
            if let Some(emp) = f.detectes.get(i).map(|l| l.emplacement.clone()) {
                f.app.naviguer(hwnd, emp);
            }
            return;
        }
        Element::Ajoute(i) => {
            if let Some(emp) = f.ajoutes().into_iter().find(|l| l.ajoute == Some(i)).map(|l| l.emplacement) {
                f.app.naviguer(hwnd, emp);
            }
            return;
        }
        Element::Ajouter => {
            f.page.formulaire = Some(Formulaire::nouveau());
        }
        Element::Depart => reglages::modifier(|r| {
            let i = Depart::TOUS.iter().position(|&d| d == r.depart).unwrap_or(0);
            r.depart = Depart::TOUS[suivant(Depart::TOUS.len(), i)];
        }),
        Element::LimiteReseau => reglages::modifier(|r| {
            let d = &reglages::LIMITES_RESEAU;
            let i = d.iter().position(|&x| x == r.limite_reseau_mo).unwrap_or(0);
            r.limite_reseau_mo = d[suivant(d.len(), i)];
        }),
        Element::NettoyageJours => reglages::modifier(|r| {
            let d = &reglages::JOURS_NETTOYAGE;
            let i = d.iter().position(|&x| x == r.nettoyage_jours).unwrap_or(0);
            r.nettoyage_jours = d[suivant(d.len(), i)];
        }),
        Element::NettoyerCopies => reglages::modifier(|r| r.nettoyer_copies = !r.nettoyer_copies),
        Element::Arbre => reglages::modifier(|r| r.arbre = !r.arbre),
        Element::NettoyerArchives => reglages::modifier(|r| r.nettoyer_archives = !r.nettoyer_archives),
        Element::TelechargementsNavigateur => reglages::modifier(|r| r.telechargements_navigateur = !r.telechargements_navigateur),
        Element::RangerAuto => reglages::modifier(|r| r.ranger_auto = !r.ranger_auto),
        Element::CopieVerifiee => reglages::modifier(|r| r.copie_verifiee = !r.copie_verifiee),
        Element::Regle(i) => {
            reglages::modifier(|r| {
                if i < r.regles.len() {
                    r.regles.remove(i);
                }
            });
            annonce = Some("REGLE RETIREE".into());
        }
        Element::Ressort => reglages::modifier(|r| {
            let d = &reglages::DELAIS_RESSORT;
            let i = d.iter().position(|&x| x == r.ressort_ms).unwrap_or(2);
            r.ressort_ms = d[suivant(d.len(), i)];
        }),
        Element::RestaurerOnglets => reglages::modifier(|r| r.restaurer_onglets = !r.restaurer_onglets),
        Element::Caches => reglages::modifier(|r| r.caches = !r.caches),
        // Pas un réglage enregistré : la même bascule que F9, qui annonce et rebâtit les listes
        Element::Essentiel => {
            super::executer(hwnd, f, Commande::Essentiel);
            return;
        }
        Element::DatesRelatives => reglages::modifier(|r| r.dates_relatives = !r.dates_relatives),
        Element::EtatCloud => reglages::modifier(|r| r.etat_cloud = !r.etat_cloud),
        Element::Apercu => reglages::modifier(|r| r.apercu = !r.apercu),
        Element::ApercuSysteme => reglages::modifier(|r| r.apercu_systeme = !r.apercu_systeme),
        Element::UnClic => reglages::modifier(|r| r.un_clic = !r.un_clic),
        Element::Animations => reglages::modifier(|r| r.animations = !r.animations),
        Element::Sons => reglages::modifier(|r| r.sons = !r.sons),
        Element::PoidsSonore => reglages::modifier(|r| r.poids_sonore = !r.poids_sonore),
        Element::VoletDense => reglages::modifier(|r| r.volet_dense = !r.volet_dense),
        Element::Profil => crate::commandes::executer(hwnd, f, crate::palette::Commande::BasculerProfil),
        Element::Experimental => {
            reglages::modifier(|r| r.experimental = !r.experimental);
            // Le volet montre ou cache les serveurs SFTP / WebDAV tout de suite
            f.rafraichir_lieux();
        }
        Element::ParDefaut => {
            if f.par_defaut {
                defaut::desactiver();
            } else {
                defaut::activer();
            }
            f.par_defaut = defaut::est_actif();
            annonce = Some(
                if f.par_defaut { "SPACEFILES OUVRE MAINTENANT LES DOSSIERS" } else { "L'EXPLORATEUR DE WINDOWS OUVRE DE NOUVEAU LES DOSSIERS" }.into(),
            );
        }
        Element::WinE => {
            if f.win_e {
                defaut::desactiver_win_e();
            } else if !defaut::activer_win_e() {
                f.app.annoncer("CLE DE DEMARRAGE NON ECRITE", true);
            }
            f.win_e = defaut::win_e_actif();
            annonce = Some(if f.win_e { "WIN+E OUVRE MAINTENANT SPACEFILES" } else { "WIN+E RENDU A WINDOWS, RESIDENT ARRETE" }.into());
        }
        Element::Fichier => {
            // Écrit le fichier s'il n'existe pas encore, puis l'ouvre dans l'éditeur de texte
            reglages::modifier(|_| {});
            if let Some(c) = reglages::chemin_fichier() {
                systeme::lancer(hwnd, &c.to_string_lossy());
            }
        }
        Element::Exporter => match systeme::choisir(hwnd, true) {
            Some(d) => {
                let dossier = std::path::Path::new(&d);
                annonce = Some(match reglages::exporter(dossier).and_then(|c| crate::marquage::exporter(dossier).map(|_| c)) {
                    Ok(c) => format!("RÉGLAGES ET MARQUES EXPORTÉS · {}", c.display().to_string().to_uppercase()),
                    Err(e) => e,
                })
            }
            None => annonce = Some("EXPORT ANNULE".into()),
        },
        Element::Importer => match systeme::choisir(hwnd, false) {
            Some(c) => {
                let fichier = std::path::Path::new(&c);
                // Les marques (groupes, balises, étiquettes) sont dans un fichier voisin du TOML, écrit par le même export
                let marques = fichier.with_file_name(crate::marquage::FICHIER_EXPORT);
                annonce = Some(match reglages::importer(fichier).and_then(|()| crate::marquage::importer(&marques)) {
                    Ok(avec_marques) => {
                        f.rafraichir_lieux();
                        if avec_marques { "RÉGLAGES ET MARQUES IMPORTÉS" } else { "RÉGLAGES IMPORTÉS (PAS DE FICHIER DE MARQUES À CÔTÉ)" }.into()
                    }
                    Err(e) => e,
                })
            }
            None => annonce = Some("IMPORT ANNULE".into()),
        },
        Element::Reinitialiser => {
            reglages::reinitialiser();
            f.rafraichir_lieux();
            annonce = Some("RÉGLAGES REMIS PAR DÉFAUT (STOCKAGES CONSERVÉS)".into());
        }
    }
    if let Some(a) = annonce {
        f.app.annoncer(a, false);
    }
    terminer_changement(hwnd);
}

/// Ce que fait une touche pressée pendant la capture d'une touche (calcul pur : c'est ce que testent les tests)
#[derive(Debug, PartialEq, Eq)]
enum Capture {
    /// Touche de modification seule : on attend la suivante
    Attendre,
    /// Échap : la touche reste telle quelle
    Abandonner,
    /// Suppr : la touche d'origine revient
    Retablir,
    /// Pas une combinaison valide (lettre seule, touche inconnue)
    Invalide,
    /// Déjà prise par l'occupant nommé
    Occupee(&'static str),
    /// Enregistrer cette combinaison
    Enregistrer(String),
}

fn decider_capture(vk: VIRTUAL_KEY, ctrl: bool, maj: bool, alt: bool, i: usize, actuels: &[String]) -> Capture {
    if matches!(vk, VK_CONTROL | VK_SHIFT | VK_MENU | VK_LCONTROL | VK_RCONTROL | VK_LSHIFT | VK_RSHIFT | VK_LMENU | VK_RMENU | VK_LWIN | VK_RWIN) {
        return Capture::Attendre;
    }
    let nue = !(ctrl || alt || maj);
    if vk == VK_ESCAPE && nue {
        return Capture::Abandonner;
    }
    if vk == VK_DELETE && nue {
        return Capture::Retablir;
    }
    let Some(combo) = combinaison(vk, ctrl, alt, maj) else { return Capture::Invalide };
    match palette::occupant(&combo, i, actuels) {
        Some(autre) => Capture::Occupee(autre),
        None => Capture::Enregistrer(combo),
    }
}

/// Page Raccourcis, ligne en attente d'une touche : ce que fait la touche pressée. Échap abandonne, Suppr rétablit la touche
/// d'origine, une combinaison valide est enregistrée si personne d'autre ne la tient (sinon on reste en attente, avec
/// le nom de l'occupant dans la barre d'état). Les touches de modification seules attendent la suivante.
pub(crate) unsafe fn capturer_touche(f: &mut Fenetre, vk: VIRTUAL_KEY, ctrl: bool, maj: bool, alt: bool) {
    let Some(i) = f.page.capture else { return };
    match decider_capture(vk, ctrl, maj, alt, i, &reglages::actuels().raccourcis) {
        Capture::Attendre => {}
        Capture::Abandonner => {
            f.page.capture = None;
            f.app.annoncer("TOUCHE INCHANGEE", false);
        }
        Capture::Retablir => {
            reglages::modifier(|r| r.raccourcis = palette::raccourcis_avec(&r.raccourcis, i, None));
            f.page.capture = None;
            f.app.annoncer("TOUCHE D'ORIGINE RETABLIE", false);
        }
        Capture::Invalide => f.app.annoncer("AJOUTEZ CTRL OU ALT, OU CHOISISSEZ UNE TOUCHE F", true),
        Capture::Occupee(autre) => {
            let combo = combinaison(vk, ctrl, alt, maj).unwrap_or_default();
            f.app.annoncer(format!("{} EST DEJA A : {autre}", page::touche_lisible(&combo).to_uppercase()), true);
        }
        Capture::Enregistrer(combo) => {
            reglages::modifier(|r| r.raccourcis = palette::raccourcis_avec(&r.raccourcis, i, Some(&combo)));
            f.page.capture = None;
            f.app.annoncer(format!("{} : {}", palette::COMMANDES[i].1, page::touche_lisible(&combo).to_uppercase()), false);
        }
    }
}

/// Ce que tout changement de réglage déclenche ensuite
unsafe fn terminer_changement(hwnd: HWND) {
    // Le suivi des téléchargements du navigateur a pu être coupé ou remis (interrupteur, import, remise à zéro) : l'abonnement
    // au dossier suit tout de suite ; coupé, plus aucun fil, pastille ni marque (idempotent)
    navigateur::regler(hwnd);
    // Un réglage (thème, animations, import, remise à zéro) a pu changer l'apparence : DWM, images et fenêtre suivent
    rafraichir_apparence(hwnd);
    sons::jouer(sons::Son::Tic);
}

/// Écrit la valeur du segment `s` d'un réglage à contrôle segmenté (rang dans l'ordre affiché, voir `page::lignes`)
pub(crate) fn appliquer_segment(element: Element, s: usize) {
    if matches!(element, Element::Theme | Element::TailleLignes | Element::VueDefaut | Element::Mouvement | Element::PixelHeure | Element::SectionVolet(_)) {
        reglages::modifier(|r| segment_dans(r, element, s));
    }
}

/// Le calcul de `appliquer_segment` sur des réglages donnés (sans toucher au fichier : c'est ce que testent les tests)
fn segment_dans(r: &mut reglages::Reglages, element: Element, s: usize) {
    match element {
        Element::Theme => r.theme = page::THEMES_SEGMENTS[s.min(page::THEMES_SEGMENTS.len() - 1)],
        // Rang 0 = Confort : `taille_lignes` vaut 1 pour Confort
        Element::TailleLignes => r.taille_lignes = u32::from(s == 0),
        Element::VueDefaut => r.vue_defaut = s.min(2) as u32,
        // Rang 0 = Complet, 1 = Réduit
        Element::Mouvement => r.mouvement_reduit = s == 1,
        Element::PixelHeure => r.pixel = crate::pixel::ChoixPixel::depuis_rang(s),
        Element::SectionVolet(k) => {
            let b = 1u32 << k;
            r.volet_replie = (r.volet_replie & !b) | if s == 1 { b } else { 0 };
            r.volet_masque = (r.volet_masque & !b) | if s == 2 { b } else { 0 };
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pixel::ChoixPixel;

    fn rang(c: palette::Commande) -> usize {
        palette::COMMANDES.iter().position(|x| x.0 == c).unwrap()
    }

    #[test]
    fn capture_de_touche_avec_modificateurs_simules() {
        use windows::Win32::UI::Input::KeyboardAndMouse::{VK_C, VK_E, VK_F2, VK_F8, VK_L, VK_Y};
        let renommer = rang(palette::Commande::Renommer);
        // Modificateurs seuls : on attend ; Échap abandonne ; Suppr rétablit
        assert_eq!(decider_capture(VK_CONTROL, true, false, false, renommer, &[]), Capture::Attendre);
        assert_eq!(decider_capture(VK_ESCAPE, false, false, false, renommer, &[]), Capture::Abandonner);
        assert_eq!(decider_capture(VK_DELETE, false, false, false, renommer, &[]), Capture::Retablir);
        // Les modificateurs s'écrivent dans l'ordre Ctrl, Alt, Maj
        assert_eq!(decider_capture(VK_Y, true, true, false, renommer, &[]), Capture::Enregistrer("CTRL+MAJ+Y".into()));
        assert_eq!(decider_capture(VK_Y, true, true, true, renommer, &[]), Capture::Enregistrer("CTRL+ALT+MAJ+Y".into()));
        // Une lettre seule ou avec Maj seul : refusée ; une touche F nue : acceptée
        assert_eq!(decider_capture(VK_Y, false, false, false, renommer, &[]), Capture::Invalide);
        assert_eq!(decider_capture(VK_Y, false, true, false, renommer, &[]), Capture::Invalide);
        assert_eq!(decider_capture(VK_F8, false, false, false, renommer, &[]), Capture::Enregistrer("F8".into()));
        // Touche déjà prise par une autre commande
        let copier = rang(palette::Commande::Copier);
        assert!(matches!(decider_capture(VK_C, true, false, false, renommer, &[]), Capture::Occupee(_)));
        assert_eq!(decider_capture(VK_E, false, false, false, copier, &[]), Capture::Invalide);
        // Sa propre touche reste libre pour elle-même ; une touche quittée est libre pour une autre
        assert_eq!(decider_capture(VK_F2, false, false, false, renommer, &[]), Capture::Enregistrer("F2".into()));
        let l = vec!["CTRL+MAJ+Y=Renommer".to_string()];
        assert_eq!(decider_capture(VK_F2, false, false, false, copier, &l), Capture::Enregistrer("F2".into()));
        assert!(matches!(decider_capture(VK_Y, true, true, false, copier, &l), Capture::Occupee(_)));
        // Les anciennes touches fixes sont des commandes de la table : Ctrl+L appartient à « saisir un chemin »
        assert_eq!(decider_capture(VK_L, true, false, false, renommer, &[]), Capture::Occupee("SAISIR UN CHEMIN"));
    }

    #[test]
    fn segment_mouvement() {
        let mut r = reglages::Reglages::default();
        segment_dans(&mut r, Element::Mouvement, 1);
        assert!(r.mouvement_reduit);
        segment_dans(&mut r, Element::Mouvement, 0);
        assert!(!r.mouvement_reduit);
    }

    #[test]
    fn segment_pixel_selon_l_heure() {
        let mut r = reglages::Reglages::default();
        for (s, attendu) in [(1, ChoixPixel::Jour), (2, ChoixPixel::Soir), (3, ChoixPixel::Nuit), (0, ChoixPixel::Auto), (9, ChoixPixel::Nuit)] {
            segment_dans(&mut r, Element::PixelHeure, s);
            assert_eq!(r.pixel, attendu, "segment {s}");
        }
    }

    /// Les anciens segments se comportent comme avant la refonte en fonction pure
    #[test]
    fn segments_existants_inchanges() {
        let mut r = reglages::Reglages::default();
        segment_dans(&mut r, Element::TailleLignes, 1);
        assert_eq!(r.taille_lignes, 0);
        segment_dans(&mut r, Element::SectionVolet(2), 2);
        assert_eq!((r.volet_masque, r.volet_replie), (4, 0));
        segment_dans(&mut r, Element::SectionVolet(2), 1);
        assert_eq!((r.volet_masque, r.volet_replie), (0, 4));
        segment_dans(&mut r, Element::VueDefaut, 7);
        assert_eq!(r.vue_defaut, 2);
    }
}

/// Clic sur un segment : ce segment est choisi (et non le suivant)
pub(crate) unsafe fn choisir_segment(hwnd: HWND, f: &mut Fenetre, k: usize, s: usize) {
    let Some(element) = f.lignes().get(k).map(|l| l.element) else { return };
    appliquer_segment(element, s);
    terminer_changement(hwnd);
}

/// Retire un stockage ajouté. Le mot de passe mémorisé est oublié si plus aucun stockage n'utilise ce serveur.
pub(crate) fn retirer_stockage(f: &mut Fenetre, i: usize) {
    let r = reglages::actuels();
    let Some(s) = r.stockages.get(i).cloned() else { return };
    reglages::modifier(|r| {
        r.stockages.remove(i);
    });
    let hote = |s: &Stockage| stockages::hote(&stockages::chemin(s.sorte, &s.adresse)).map(|h| h.to_lowercase());
    if let Some(h) = hote(&s) {
        if !reglages::actuels().stockages.iter().any(|t| hote(t).as_deref() == Some(h.as_str())) {
            stockages::oublier(&h);
        }
    }
    f.rafraichir_lieux();
    f.app.annoncer(format!("{} RETIRE", s.nom.to_uppercase()), false);
    sons::jouer(sons::Son::Corbeille);
}

/// Ajoute le stockage du formulaire (adresse vérifiée, ou forcée)
pub(crate) fn ajouter_stockage(f: &mut Fenetre, adresse: String) {
    let Some(fo) = &f.page.formulaire else { return };
    let sorte = fo.sorte;
    let utilisateur = if fo.champ_utile(3) { fo.champs[2].valeur().trim().to_string() } else { String::new() };
    let mut nom = fo.champs[0].valeur().trim().to_string();
    if nom.is_empty() {
        let c = stockages::chemin(sorte, &adresse);
        nom = match stockages::hote(&c) {
            Some(h) if !c.trim_start_matches('\\').contains('\\') => dossier::nom_hote(&h),
            _ => c.trim_end_matches('\\').rsplit('\\').next().unwrap_or(&c).to_string(),
        };
    }
    let nom_affiche = nom.to_uppercase();
    reglages::modifier(|r| r.stockages.push(Stockage { nom, sorte, adresse, utilisateur }));
    fermer_formulaire(f);
    f.rafraichir_lieux();
    // La nouvelle ligne devient la ligne choisie
    let lignes = f.lignes();
    let n = reglages::actuels().stockages.len();
    if let Some(k) = lignes.iter().position(|l| l.element == Element::Ajoute(n - 1)) {
        f.page.focus = k;
    }
    f.app.annoncer(format!("{nom_affiche} AJOUTE AU VOLET"), false);
    sons::jouer(sons::Son::Fini);
}

/// AJOUTER : vérifie le stockage dans un fil séparé (réseau : plusieurs secondes possibles)
pub(crate) unsafe fn soumettre(hwnd: HWND, f: &mut Fenetre) {
    let Some(fo) = &mut f.page.formulaire else { return };
    if fo.en_cours {
        return;
    }
    let adresse = fo.champs[1].valeur().trim().to_string();
    if fo.forcer {
        if adresse.is_empty() {
            fo.erreur = Some("ADRESSE MANQUANTE".into());
            return;
        }
        ajouter_stockage(f, adresse);
        return;
    }
    let sorte = fo.sorte;
    let utile = fo.champ_utile(3);
    let utilisateur = if utile { fo.champs[2].valeur().trim().to_string() } else { String::new() };
    let mut mot_de_passe: Vec<u16> = if utile { fo.champs[3].texte.clone() } else { Vec::new() };
    fo.en_cours = true;
    fo.erreur = None;
    let fenetre = hwnd.0 as isize;
    std::thread::spawn(move || {
        let r = Box::new(stockages::verifier(sorte, &adresse, &utilisateur, &mot_de_passe));
        mot_de_passe.iter_mut().for_each(|c| *c = 0);
        let p = Box::into_raw(r);
        if PostMessageW(HWND(fenetre as *mut _), WM_STOCKAGE, WPARAM(0), LPARAM(p as isize)).is_err() {
            drop(Box::from_raw(p));
        }
    });
}
