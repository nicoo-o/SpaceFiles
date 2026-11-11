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
        Element::DatesRelatives => reglages::modifier(|r| r.dates_relatives = !r.dates_relatives),
        Element::EtatCloud => reglages::modifier(|r| r.etat_cloud = !r.etat_cloud),
        Element::Apercu => reglages::modifier(|r| r.apercu = !r.apercu),
        Element::UnClic => reglages::modifier(|r| r.un_clic = !r.un_clic),
        Element::Animations => reglages::modifier(|r| r.animations = !r.animations),
        Element::Sons => reglages::modifier(|r| r.sons = !r.sons),
        Element::PoidsSonore => reglages::modifier(|r| r.poids_sonore = !r.poids_sonore),
        Element::VoletDense => reglages::modifier(|r| r.volet_dense = !r.volet_dense),
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
                annonce = Some(match reglages::exporter(std::path::Path::new(&d)) {
                    Ok(c) => format!("RÉGLAGES EXPORTÉS · {}", c.display().to_string().to_uppercase()),
                    Err(e) => e,
                })
            }
            None => annonce = Some("EXPORT ANNULE".into()),
        },
        Element::Importer => match systeme::choisir(hwnd, false) {
            Some(c) => {
                annonce = Some(match reglages::importer(std::path::Path::new(&c)) {
                    Ok(()) => {
                        f.rafraichir_lieux();
                        "RÉGLAGES IMPORTÉS".into()
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
