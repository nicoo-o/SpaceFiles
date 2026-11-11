// Palette à étapes : ce que la fenêtre fait pour elle. Lancer la recherche et la lecture des dossiers en fond (jamais sur le fil
// d'interface), exécuter une commande d'élément par les opérations qui existent déjà, suivre la progression, annuler.

use crate::assistant::{self, CommandeEl, Execution, Resultat};
use crate::*;

/// Résultats de l'index pour la palette (LPARAM = Box<(requête, Vec<Trouve>)>)
pub(crate) const WM_PALETTE_RECHERCHE: u32 = WM_APP + 42;
/// Un dossier lu pour l'aperçu de la palette (LPARAM = Box<assistant::Lecture>)
pub(crate) const WM_PALETTE_LECTURE: u32 = WM_APP + 43;

/// Génération de la recherche d'index : chaque requête envoyée en fond prend un numéro, et une réponse dont le numéro est dépassé n'est
/// ni calculée (la requête suivante est déjà partie) ni postée. L'index ne s'interrompt pas en plein calcul, mais on ne l'empile plus.
static RECHERCHE_GEN: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// Poste une valeur à la fenêtre ; rend la boîte si la fenêtre n'existe plus
fn poster<T>(fenetre: isize, msg: u32, v: T) {
    let p = Box::into_raw(Box::new(v));
    // SAFETY: `p` est une boîte valide dont la fenêtre prend la propriété (elle la libère dans son gestionnaire) ; si l'envoi échoue
    // personne ne la recevra et on la reprend ici
    unsafe {
        if PostMessageW(HWND(fenetre as *mut _), msg, WPARAM(0), LPARAM(p as isize)).is_err() {
            drop(Box::from_raw(p));
        }
    }
}

/// Lance en fond ce que la palette vient de demander : la recherche dans l'index et la lecture des dossiers à montrer. Chaque chose
/// n'est demandée qu'une fois (`Palette::a_demander`) ; les réponses reviennent par `WM_PALETTE_RECHERCHE` et `WM_PALETTE_LECTURE`.
pub(crate) fn lectures_palette(hwnd: HWND, f: &mut Fenetre) {
    let Some(p) = f.palette.as_mut() else { return };
    let (recherche, a_lire) = p.a_demander();
    let fenetre = hwnd.0 as isize;
    if let Some(q) = recherche {
        use std::sync::atomic::Ordering::SeqCst;
        let generation = RECHERCHE_GEN.fetch_add(1, SeqCst) + 1;
        std::thread::spawn(move || {
            // Une requête plus récente est déjà partie (frappe suivante) : celle-ci ne sert plus à rien
            if RECHERCHE_GEN.load(SeqCst) != generation {
                return;
            }
            // L'index consulte ce test tous les 16 384 éléments : une frappe suivante abandonne la recherche en cours
            let depasse = || RECHERCHE_GEN.load(SeqCst) != generation;
            let reponse = index::chercher_avec(&q, 40, &depasse);
            if depasse() {
                return;
            }
            // `None` : l'index a échoué (en construction…). Ce n'est pas « aucun résultat » : la palette doit pouvoir redemander.
            let trouves: Option<Vec<palette::Trouve>> = reponse.ok().map(|r| {
                r.iter().map(|e| palette::Trouve { chemin: dossier::joindre(e.parent.as_deref().unwrap_or(""), &e.nom), dossier: e.est_conteneur(), icone: icones::pour(e) }).collect()
            });
            poster(fenetre, WM_PALETTE_RECHERCHE, (q, trouves));
        });
    }
    for chemin in a_lire {
        std::thread::spawn(move || poster(fenetre, WM_PALETTE_LECTURE, assistant::lire_dossier(&chemin)));
    }
}

/// Les résultats de l'index arrivent
pub(crate) fn recevoir_recherche(hwnd: HWND, f: &mut Fenetre, requete: String, trouves: Option<Vec<palette::Trouve>>) {
    match trouves {
        Some(t) => {
            if f.palette.as_mut().is_some_and(|p| p.recevoir_recherche(&requete, t)) {
                f.remplir_palette();
                lectures_palette(hwnd, f);
            }
        }
        // L'index a échoué : la requête sera redemandée à la prochaine frappe ou au prochain événement, pas tout de suite (boucle)
        None => {
            if let Some(p) = f.palette.as_mut() {
                p.recherche_echouee(&requete);
            }
        }
    }
}

/// Un dossier lu arrive. Une lecture qui n'était plus attendue (la palette a fini une opération depuis) est jetée.
pub(crate) fn recevoir_lecture(hwnd: HWND, f: &mut Fenetre, l: assistant::Lecture) {
    let Some(p) = f.palette.as_mut() else { return };
    if !p.recevoir_lecture(l) {
        return;
    }
    f.remplir_palette();
    lectures_palette(hwnd, f);
}

/// Une opération (copie, déplacement, corbeille, annulation) est en cours, en attente ou attend une réponse à la carte des conflits
fn occupe(f: &Fenetre) -> bool {
    f.apps().any(|a| a.operation.is_some() || a.conflits.is_some() || a.en_file() > 0)
}

/// Termine la palette sur ce résultat
fn finir(f: &mut Fenetre, r: Resultat) {
    if let Some(p) = f.palette.as_mut() {
        p.finir(r);
    }
    f.remplir_palette();
}

/// Entrée sur la dernière étape : la commande part, par les opérations de l'application (conflits, historique et annulation compris)
pub(crate) unsafe fn executer_etape(hwnd: HWND, f: &mut Fenetre, ex: Execution) {
    let chemins = ex.sujet.chemins.clone();
    let Some(premier) = chemins.first().cloned() else { return };
    let dossier_sujet = ex.sujet.genre == assistant::GenreSujet::Dossier;
    // Pixel se plisse à l'instant où la commande part, comme pour une commande générale
    f.plisse_depuis = Some(std::time::Instant::now());
    match ex.commande {
        CommandeEl::Ouvrir => {
            f.palette = None;
            if dossier_sujet {
                f.app.naviguer(hwnd, Emplacement::dossier(&premier));
            } else {
                systeme::lancer(hwnd, &premier);
            }
        }
        CommandeEl::OuvrirOnglet => {
            f.palette = None;
            f.nouvel_onglet(hwnd, Emplacement::dossier(&premier));
        }
        CommandeEl::Partager | CommandeEl::Proprietes => {
            f.palette = None;
            let verbe = if ex.commande == CommandeEl::Partager { "Windows.ModernShare" } else { "properties" };
            if let Err(e) = coquille::invoquer(hwnd, &chemins, verbe) {
                f.app.annoncer(operations::message_com(&e), true);
            }
        }
        CommandeEl::AjouterGroupe => {
            f.palette = Some(Palette::groupe(chemins));
            f.remplir_palette();
        }
        CommandeEl::CopierChemin => {
            operations::copier_texte(hwnd, &chemins.join("\r\n"));
            finir(f, Resultat::ok(if chemins.len() > 1 { "Chemins copiés" } else { "Chemin copié" }, false));
        }
        CommandeEl::CopierPressePapiers | CommandeEl::CouperPressePapiers => {
            let couper = ex.commande == CommandeEl::CouperPressePapiers;
            if operations::copier_fichiers(hwnd, &chemins, couper) {
                f.app.coupes = if couper { chemins.iter().cloned().collect() } else { Default::default() };
                let message = match (couper, chemins.len() > 1) {
                    (true, false) => "Coupé : collez-le avec Ctrl+V dans le dossier voulu",
                    (true, true) => "Coupés : collez-les avec Ctrl+V dans le dossier voulu",
                    (false, false) => "Copié : collez-le avec Ctrl+V dans le dossier voulu",
                    (false, true) => "Copiés : collez-les avec Ctrl+V dans le dossier voulu",
                };
                finir(f, Resultat::ok(message, false));
            } else {
                finir(f, Resultat::erreur("Presse-papiers indisponible"));
            }
        }
        CommandeEl::CopierVers | CommandeEl::DeplacerVers | CommandeEl::Corbeille | CommandeEl::CompresserVers => {
            if occupe(f) {
                finir(f, Resultat::erreur("Une autre opération est en cours : réessayez dans un instant"));
                return;
            }
            let dest = ex.dossier.clone().unwrap_or_default();
            // Copier ou déplacer teste les noms pris, déplace et crée sur le fil d'interface : sur le réseau il attendrait. Jamais proposé
            // en destination, mais refusé aussi si un chemin tapé ou mémorisé y mène.
            let sur_le_reseau = matches!(ex.commande, CommandeEl::CopierVers | CommandeEl::DeplacerVers) && assistant::destination_distante(&dest);
            if sur_le_reseau {
                finir(f, Resultat::erreur("Destination réseau : la palette n'y copie pas (la fenêtre attendrait le réseau). Glissez-déposez les éléments."));
                return;
            }
            let verbe = match ex.commande {
                CommandeEl::CopierVers => "Copie en cours…",
                CommandeEl::DeplacerVers => "Déplacement en cours…",
                CommandeEl::CompresserVers => "Compression en cours…",
                _ => "Mise à la corbeille…",
            };
            // Cette opération-là est celle de la palette : sa progression et sa fin sont reconnues par son numéro, pas par « il y en a une »
            f.app.armer_palette();
            match ex.commande {
                CommandeEl::CopierVers => f.app.deposer(hwnd, chemins, dest, false),
                CommandeEl::DeplacerVers => f.app.deposer(hwnd, chemins, dest, true),
                CommandeEl::CompresserVers => f.app.compresser_vers(hwnd, chemins, dest, dossier_sujet),
                _ => f.app.supprimer_chemins(hwnd, chemins),
            }
            f.app.desarmer_palette();
            if let Some(p) = f.palette.as_mut() {
                p.en_cours(verbe, ex.commande == CommandeEl::CompresserVers, false);
            }
            // Conflit « Ignorer » par défaut, tous les noms pris : « rien à faire », aucune opération ne part et rien n'arrivera. La
            // palette le dit tout de suite au lieu de rester sur « Copie en cours… »
            if ex.commande != CommandeEl::CompresserVers {
                synchroniser_palette(f);
            }
            f.remplir_palette();
        }
        CommandeEl::Renommer => {
            let nom = ex.nom.clone().unwrap_or_default();
            if assistant::destination_distante(&premier) {
                finir(f, Resultat::erreur("Dossier réseau : renommez-le depuis la liste (la fenêtre attendrait le réseau)"));
                return;
            }
            match f.app.renommer_chemin(hwnd, &premier, &nom) {
                Ok(id) => {
                    // « Autre action » vise le nouveau nom, pas l'ancien chemin qui n'existe plus
                    let nouveau = dossier::joindre(dossier::dossier_de(&premier), nom.trim());
                    if let Some(p) = f.palette.as_mut() {
                        p.sujet_renomme(nouveau);
                    }
                    finir(f, Resultat::ok_avec_entree(format!("Renommé en {}", nom.trim()), id));
                }
                Err(e) => finir(f, Resultat::erreur(e)),
            }
        }
        CommandeEl::NouveauDossier => {
            let nom = ex.nom.clone().unwrap_or_default();
            if assistant::destination_distante(&premier) {
                finir(f, Resultat::erreur("Dossier réseau : créez-le depuis la liste (la fenêtre attendrait le réseau)"));
                return;
            }
            match f.app.creer_dossier_nomme(hwnd, &premier, &nom) {
                Ok(id) => finir(f, Resultat::ok_avec_entree(format!("Dossier « {} » créé", nom.trim()), id)),
                Err(e) => finir(f, Resultat::erreur(e)),
            }
        }
    }
    sons::jouer(sons::Son::Tic);
}

/// « Annuler » du résultat : l'entrée d'historique que la commande a notée est défaite, celle-là et pas « la dernière de la pile ». Une
/// copie ou un déplacement défaits tournent en tâche de fond : la palette montre l'avancement puis « Annulé ».
pub(crate) unsafe fn defaire_etape(hwnd: HWND, f: &mut Fenetre) {
    if occupe(f) {
        return;
    }
    // Pas d'entrée notée (renommage échoué, copie du chemin, suppression définitive) : rien à défaire, et surtout pas une autre action
    let Some(id) = f.palette.as_ref().and_then(|p| p.resultat.as_ref()).filter(|r| r.annulable).and_then(|r| r.entree) else { return };
    f.app.armer_palette();
    let fait = f.app.annuler_pour_palette(hwnd, id);
    f.app.desarmer_palette();
    if occupe(f) {
        if let Some(p) = f.palette.as_mut() {
            p.en_cours("Annulation en cours…", false, true);
        }
        f.remplir_palette();
    } else if fait {
        finir(f, Resultat::ok("Annulé", false));
    } else {
        let message = f.app.annonce.as_ref().map(|a| a.0.clone()).unwrap_or_else(|| "Rien à annuler".to_string());
        finir(f, Resultat::erreur(message));
    }
}

/// Après la carte des conflits (Échap, ou tout ignoré) : plus rien ne tourne alors que la palette attend. Elle montre ce qui s'est passé.
pub(crate) fn synchroniser_palette(f: &mut Fenetre) {
    let attend = f.palette.as_ref().is_some_and(|p| p.etape == assistant::Etape::Progression && p.attente.as_ref().is_some_and(|a| !a.tache));
    if attend && !occupe(f) {
        finir(f, Resultat::ok("Rien n'a été fait : tout a été ignoré ou abandonné", false));
    }
}

/// Une opération vient de finir : la palette en attente montre son résultat si c'est SON opération (numéro posé par `lancer_elements`).
/// Un nettoyage ou un rangement de fond, une conversion d'image qui finissent pendant ce temps ne lui disent rien. `Bilan` est lu
/// avant que l'application ne le consomme ; l'entrée d'historique notée à la fin est reliée ensuite (`Resultat::avec_entree`).
pub(crate) fn resume_fin(f: &Fenetre, b: &Bilan) -> Option<Resultat> {
    if !f.apps().any(|a| a.est_palette(b.numero)) {
        return None;
    }
    let p = f.palette.as_ref()?;
    let a = p.attente.as_ref().filter(|a| p.etape == assistant::Etape::Progression && !a.tache)?;
    Some(assistant::message_fin(b.genre, b.nombre, p.destination.as_deref().unwrap_or(""), b.annule, b.erreur.as_deref(), a.annulation))
}

/// Une tâche de fond (compression) a rendu son message
pub(crate) fn resume_tache(f: &Fenetre, r: &Fini) -> Option<Resultat> {
    let p = f.palette.as_ref()?;
    p.attente.as_ref().filter(|a| p.etape == assistant::Etape::Progression && a.tache)?;
    Some(if r.erreur { Resultat::erreur(r.message.clone()) } else { Resultat::ok(format!("Compressé dans {}", p.destination.as_deref().unwrap_or("le dossier")), false) })
}
