// Minuteries de la fenêtre (animation, annonce, aperçu, ressort) et ce qu'elles déclenchent.

use crate::*;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

/// Minuterie de l'animation (défilement, apparition en points) : seulement si le fil de synchronisation sur l'écran n'a pas pu partir
pub(crate) const MINUTERIE_DEFILEMENT: usize = 1;

/// Message posté à la fenêtre par le fil de synchronisation à chaque rafraîchissement de l'écran, tant qu'une animation tourne
pub(crate) const WM_IMAGE_ANIMATION: u32 = WM_APP + 40;

/// Un message `WM_IMAGE_ANIMATION` attend déjà dans la file : le fil n'en poste pas un second (une image lente ne s'empile pas)
static IMAGE_EN_ATTENTE: AtomicBool = AtomicBool::new(false);

/// Minuterie qui efface l'annonce de la barre d'état quand elle expire
pub(crate) const MINUTERIE_ANNONCE: usize = 2;

/// Petit délai avant de préparer l'aperçu : parcourir la liste aux flèches reste fluide
pub(crate) const MINUTERIE_APERCU: usize = 3;

/// Dossier « à ressort » : survolé pendant un glisser, il s'ouvre après ce délai (ms)
pub(crate) const MINUTERIE_RESSORT: usize = 4;

/// Écriture de la session : à usage unique, posée au premier changement d'onglets et détruite quand elle sonne
pub(crate) const MINUTERIE_SESSION: usize = 5;

/// Libération de l'index du service de recherche (fenêtre du résident) : à usage unique, réarmée à chaque requête
/// (SetTimer sur le même identifiant repart de zéro) et détruite quand elle sonne
pub(crate) const MINUTERIE_LIBERATION: usize = 6;

/// Activité : 100 ms, armée SEULEMENT pendant qu'une opération tourne (glissement de Pixel, grille 3 × 3 des pastilles), détruite
/// quand la dernière finit. N'invalide que la bande de la barre d'état et des pastilles. Jamais armée au repos.
pub(crate) const MINUTERIE_ACTIVITE: usize = 7;

/// Clignement de Pixel : 9 s, armée seulement quand la fenêtre est visible, active et les animations autorisées ; elle
/// déclenche un clignement de 630 ms (`pixel::phase_clignement`), joué par l'animation d'écran existante, sans seconde minuterie.
/// Le numéro 9, ancienne minuterie de fin de clignement, n'est plus utilisé.
pub(crate) const MINUTERIE_CLIGNEMENT: usize = 8;

/// Clignement des yeux de Pixel dans la palette, 120 ms après la dernière frappe (`palBlink`) : à usage unique, réarmée à chaque frappe
/// (SetTimer sur le même identifiant repart de zéro, comme `clearTimeout`) et détruite quand elle sonne. Jamais armée au repos.
pub(crate) const MINUTERIE_PALETTE_CLIGNE: usize = 10;

/// Lancement d'une commande de la palette, 160 ms après Entrée (`palRun`) : les yeux de Pixel sont contents pendant ce délai.
/// À usage unique, détruite quand elle sonne.
pub(crate) const MINUTERIE_PALETTE_LANCE: usize = 11;

/// Une frappe vient d'avoir lieu dans la palette : les yeux cligneront dans 120 ms (mouvement réduit ou yeux coupés : jamais)
pub(crate) unsafe fn armer_clignement_palette(hwnd: HWND) {
    if reglages::animations_actives() && reglages::yeux_pixel_actifs() {
        SetTimer(hwnd, MINUTERIE_PALETTE_CLIGNE, pixel::CLIGNEMENT_PALETTE_DELAI_MS, None);
    }
}

/// Minuterie du clignement de la palette : détruite d'abord, puis les yeux clignent (280 ms) sur l'animation d'écran
pub(crate) unsafe fn timer_clignement_palette(hwnd: HWND) -> LRESULT {
    let _ = KillTimer(hwnd, MINUTERIE_PALETTE_CLIGNE);
    if let Some(f) = etat_de(hwnd) {
        if f.palette.is_some() && reglages::animations_actives() && reglages::yeux_pixel_actifs() {
            f.rendu.mouvements.borrow_mut().yeux.cligner_palette(mouvement::maintenant_ms());
            animer(hwnd, f);
        }
    }
    LRESULT(0)
}

/// Minuterie du lancement : détruite d'abord, puis la commande choisie part et la palette se referme
pub(crate) unsafe fn timer_lancement_palette(hwnd: HWND) -> LRESULT {
    let _ = KillTimer(hwnd, MINUTERIE_PALETTE_LANCE);
    if let Some(f) = etat_de(hwnd) {
        if f.palette.as_ref().is_some_and(|p| p.lance) {
            executer_palette(hwnd, f);
            animer(hwnd, f);
        }
    }
    LRESULT(0)
}

/// Une requête vient d'arriver : l'échéance repart pour toute la durée d'inactivité
pub(crate) unsafe fn armer_liberation(hwnd: HWND) {
    // Le message est consommé : le service peut en poster un autre à la prochaine requête
    service_recherche::activite_traitee();
    SetTimer(hwnd, MINUTERIE_LIBERATION, service_recherche::delai_liberation_ms().min(u32::MAX as u64) as u32, None);
}

/// Minuterie libération : détruite d'abord ; elle ne repart que si l'échéance n'est pas atteinte ou si l'index était occupé
pub(crate) unsafe fn timer_liberation(hwnd: HWND) -> LRESULT {
    let _ = KillTimer(hwnd, MINUTERIE_LIBERATION);
    if let Some(reste) = service_recherche::au_declenchement() {
        SetTimer(hwnd, MINUTERIE_LIBERATION, reste.min(u32::MAX as u64) as u32, None);
    }
    LRESULT(0)
}

/// Compare les onglets à ce qu'on a vu au dernier rendu ; un écart arme la minuterie d'écriture (une seule à la fois)
pub(crate) unsafe fn surveiller_session(hwnd: HWND, f: &mut Fenetre) {
    let vue = vue_session(f);
    if f.session_vue.as_ref() != Some(&vue) {
        f.session_vue = Some(vue);
        if !f.session_armee {
            f.session_armee = true;
            SetTimer(hwnd, MINUTERIE_SESSION, ATTENTE_SESSION_MS, None);
        }
    }
}

/// Minuterie session : le calme est venu, la session s'écrit (si elle a changé) et la minuterie disparaît
pub(crate) unsafe fn timer_session(hwnd: HWND) -> LRESULT {
    let _ = KillTimer(hwnd, MINUTERIE_SESSION);
    if let Some(f) = etat_de(hwnd) {
        f.session_armee = false;
        sauver_session(f);
    }
    LRESULT(0)
}

/// Démarre la minuterie d'animation si elle ne tourne pas déjà
pub(crate) unsafe fn animer(hwnd: HWND, f: &mut Fenetre) {
    // Mouvement réduit : le défilement et les effets en cours sont réglés tout de suite, sans minuterie ni image intermédiaire
    if !reglages::animations_actives() {
        for a in f.tous() {
            a.animer();
        }
        let _ = InvalidateRect(hwnd, None, false);
        return;
    }
    if !f.anime {
        f.anime = true;
        armer_animation(hwnd, f);
    }
    let _ = InvalidateRect(hwnd, None, false);
}

/// Lance le cadencement des images sur le rafraîchissement de l'écran, le temps d'une animation. Une `WM_TIMER` de 8 ms est arrondie
/// par Windows et bat contre l'écran (images à 15, 20, 31 ms : écart-type de 4 à 5 ms mesuré au lot 12C, y compris avec une horloge
/// de 1 ms). Un petit fil attend donc `DwmFlush` (retour à chaque passe de composition du bureau) et poste `WM_IMAGE_ANIMATION` à la
/// fenêtre, qui dessine l'image aussitôt. Le fil ne vit que pendant l'animation : `arreter_animation` le prévient et il sort au réveil
/// suivant ; au repos, ni fil ni minuterie. Si le fil ne peut pas partir, repli sur la minuterie de 8 ms.
/// Invariant (unsafe) : fil d'interface, `hwnd` est la fenêtre principale vivante ; le fil n'emporte que le numéro du handle (copie
/// de `isize`) et `PostMessageW` sur un handle détruit échoue sans effet.
pub(crate) unsafe fn armer_animation(hwnd: HWND, f: &mut Fenetre) {
    if f.metronome.is_some() {
        return;
    }
    let mesure = mouvement::mesure_demandee();
    let arret = Arc::new(AtomicBool::new(false));
    let copie = arret.clone();
    let cible = hwnd.0 as isize;
    let lance = std::thread::Builder::new().name("synchro-ecran".into()).spawn(move || {
        let mut ticks = 0u32;
        let depart = std::time::Instant::now();
        while !copie.load(Ordering::Acquire) {
            // SAFETY: `DwmFlush` n'a aucun paramètre ; il bloque jusqu'à la prochaine passe de composition. Une erreur (composition
            // coupée) ne doit pas faire tourner le fil à vide : il dort alors un pas de 8 ms.
            if unsafe { DwmFlush() }.is_err() {
                std::thread::sleep(std::time::Duration::from_millis(8));
            }
            if copie.load(Ordering::Acquire) {
                break;
            }
            ticks += 1;
            if mesure && ticks == 60 {
                journaliser_periode_ecran(depart.elapsed().as_secs_f32() * 1000.0 / 60.0);
            }
            if !IMAGE_EN_ATTENTE.swap(true, Ordering::AcqRel) {
                // SAFETY: handle copié ; si la fenêtre n'existe plus, l'appel échoue et on lève le drapeau
                if unsafe { PostMessageW(HWND(cible as *mut _), WM_IMAGE_ANIMATION, WPARAM(0), LPARAM(0)) }.is_err() {
                    IMAGE_EN_ATTENTE.store(false, Ordering::Release);
                }
            }
        }
    });
    match lance {
        Ok(_) => f.metronome = Some(arret),
        Err(_) => {
            SetTimer(hwnd, MINUTERIE_DEFILEMENT, 8, None);
        }
    }
}

/// Période de rafraîchissement du bureau (mode essai seulement), une fois par processus : la durée moyenne de 60 retours de `DwmFlush`
/// consécutifs, de quoi juger la cadence mesurée. (`DwmGetCompositionTimingInfo` répond « composition indisponible » sur cette machine.)
fn journaliser_periode_ecran(ms: f32) {
    static UNE_FOIS: std::sync::Once = std::sync::Once::new();
    UNE_FOIS.call_once(|| {
        journal::ecrire(journal::Niveau::Info, "mouvement", &format!("PERIODE_ECRAN {ms:.2} ms ({:.1} Hz) sur 60 retours de DwmFlush", 1000.0 / ms));
    });
}

/// Arrête le cadencement : le fil de synchronisation sort à son prochain réveil, la minuterie de repli est détruite
/// Invariant (unsafe) : fil d'interface, `hwnd` est la fenêtre principale vivante.
pub(crate) unsafe fn arreter_animation(hwnd: HWND, f: &mut Fenetre) {
    let _ = KillTimer(hwnd, MINUTERIE_DEFILEMENT);
    f.anime = false;
    if let Some(arret) = f.metronome.take() {
        arret.store(true, Ordering::Release);
    }
}

/// Ce que l'aperçu doit montrer : l'élément qui a le focus, sinon le dossier ouvert
pub(crate) fn sujet_apercu(app: &App) -> Option<(String, Option<Entree>)> {
    match app.selection.and_then(|i| app.entrees.get(i)) {
        Some(e) => app.chemin_de(e).map(|c| (c, Some(e.clone()))),
        None => app.dossier().map(|d| (d.to_string(), None)),
    }
}

/// Un rafraîchissement de l'écran, signalé par le fil de synchronisation : même travail que la minuterie de repli
pub(crate) unsafe fn image_animation(hwnd: HWND) -> LRESULT {
    IMAGE_EN_ATTENTE.store(false, Ordering::Release);
    // Message resté dans la file après l'arrêt de l'animation : rien à faire
    if let Some(f) = etat_de(hwnd).filter(|f| f.anime) {
        // Seuls les yeux de Pixel bougent (clignement toutes les 9 s, regard) : 30 images par seconde, pas la cadence de l'écran
        // (mesuré : un clignement à 237 Hz coûtait ~340 ms de processeur, soit 3 % d'un cœur)
        let t = mouvement::maintenant_ms();
        let seuls = f.apps().all(|a| !a.en_mouvement()) && !pixel_en_vol(f) && !f.mouvement_hors_yeux(t);
        if !mouvement::peindre_cette_image(seuls, &mut f.image_yeux, t) {
            return LRESULT(0);
        }
        return timer_defilement(hwnd);
    }
    LRESULT(0)
}

/// Pixel glisse vers la palette ou se plisse : ces mouvements-là veulent toute la cadence de l'écran
fn pixel_en_vol(f: &Fenetre) -> bool {
    f.relais_depuis.is_some_and(|t| t.elapsed().as_millis() < RELAIS_MS) || f.plisse_depuis.is_some_and(|t| t.elapsed().as_millis() < RELAIS_MS)
}

/// Minuterie defilement : corps déplacé tel quel de la branche WM_TIMER de la procédure
pub(crate) unsafe fn timer_defilement(hwnd: HWND) -> LRESULT {
    if let Some(f) = etat_de(hwnd) {
        // Le glissement de Pixel vers la palette (0,5 s), le pli d'une commande lancée et le ressort de survol du chrome
        // tournent sur cette même minuterie
        let pixel_en_vol = pixel_en_vol(f);
        let maintenant = mouvement::maintenant_ms();
        // Ouverture finie : plus d'état à dessiner. Fermeture finie : la fenêtre se détruit (ne plus toucher à `f` ensuite).
        // La cascade des lignes (600 ms au plus) finit après la fenêtre (560 ms) : l'ouverture dure jusque-là
        if f.ouverture.is_some_and(|d| maintenant >= d + mouvement::OUVERTURE_MS.max(mouvement::CASCADE_TOTAL_MS)) {
            f.ouverture = None;
        }
        if f.fermeture.is_some_and(|d| mouvement::fermeture_finie(d, maintenant)) || (f.fermeture.is_some() && !reglages::animations_actives()) {
            arreter_animation(hwnd, f);
            let _ = DestroyWindow(hwnd);
            return LRESULT(0);
        }
        if !f.app.animer() && !pixel_en_vol && !f.mouvement_en_cours(maintenant) {
            arreter_animation(hwnd, f);
            if let Some(bilan) = f.cadence.vider() {
                mouvement::journaliser_cadence(&bilan);
            }
        }
        let _ = InvalidateRect(hwnd, None, false);
    }
    LRESULT(0)
}

/// Minuterie annonce : corps déplacé tel quel de la branche WM_TIMER de la procédure
pub(crate) unsafe fn timer_annonce(hwnd: HWND) -> LRESULT {
    if let Some(f) = etat_de(hwnd) {
        // Pastilles de fin (5 s), d'erreur et d'annulation (10 s) : elles s'effacent par cette même minuterie, tant qu'il en reste
        let temporaires = f.pastilles_temporaires();
        if f.app.annonce.as_ref().map_or(true, |(_, _, t)| t.elapsed() >= app::DUREE_ANNONCE) {
            f.app.annonce = None;
        }
        if f.app.annonce.is_none() && !temporaires {
            f.annonce_armee = false;
            let _ = KillTimer(hwnd, MINUTERIE_ANNONCE);
        }
        let _ = InvalidateRect(hwnd, None, false);
    }
    LRESULT(0)
}

/// Minuterie apercu : corps déplacé tel quel de la branche WM_TIMER de la procédure
pub(crate) unsafe fn timer_apercu(hwnd: HWND) -> LRESULT {
    let _ = KillTimer(hwnd, MINUTERIE_APERCU);
    if let Some(f) = etat_de(hwnd) {
        if let Some((chemin, entree)) = f.apercu_attente.take() {
            f.apercu_numero += 1;
            apercu::charger(hwnd, f.apercu_numero, chemin, entree);
        }
    }
    LRESULT(0)
}

/// Minuterie ressort : corps déplacé tel quel de la branche WM_TIMER de la procédure
pub(crate) unsafe fn timer_ressort(hwnd: HWND) -> LRESULT {
    let _ = KillTimer(hwnd, MINUTERIE_RESSORT);
    if let Some(f) = etat_de(hwnd) {
        let zone = f.app.depot;
        ouvrir_ressort(hwnd, f, zone);
    }
    LRESULT(0)
}

/// Repeint la bande de la barre d'état (Pixel) et des pastilles posées au dernier dessin, plus 34 DIP au-dessus (infobulle de
/// destination). Si les pastilles à montrer ne sont pas celles du dernier dessin (une vient d'apparaître), tout est repeint.
/// Invariant (unsafe) : fil d'interface, `hwnd` est la fenêtre principale vivante ; aucune référence n'est gardée après l'appel.
pub(crate) unsafe fn invalider_bande(hwnd: HWND, f: &Fenetre) {
    let (l, h) = taille_client(hwnd);
    let posees = f.rendu.zones_pastilles.borrow();
    let attendues = f.pastilles().len();
    let posees_n = posees.places.len() + usize::from(posees.plus.is_some());
    // Les pastilles posées au dernier dessin sont « 3 au plus » plus le « +N » : on compare au nombre qui doit l'être
    let attendu_n = attendues.min(pastilles::MAX_VISIBLES) + usize::from(attendues > pastilles::MAX_VISIBLES);
    if posees_n != attendu_n {
        let _ = InvalidateRect(hwnd, None, false);
        return;
    }
    let echelle = f.dpi / 96.0;
    let mut haut = f.dip(h as i32) - STATUT_H;
    if let Some(e) = posees.enveloppe() {
        haut = haut.min(e.y - 34.0);
    }
    let r = RECT { left: 0, top: (haut.max(0.0) * echelle).floor() as i32, right: l as i32, bottom: h as i32 };
    let _ = InvalidateRect(hwnd, Some(&r), false);
}

/// Une opération vient de démarrer : arme la minuterie d'activité si elle ne tourne pas déjà. Mouvement réduit : pas de
/// minuterie, la grille est fixe et Pixel immobile ; seules les progressions (WM_PROGRES) repeignent.
/// Invariant (unsafe) : fil d'interface, `hwnd` est la fenêtre principale vivante ; aucune référence n'est gardée après l'appel.
pub(crate) unsafe fn armer_activite(hwnd: HWND) {
    if reglages::animations_actives() {
        SetTimer(hwnd, MINUTERIE_ACTIVITE, 100, None);
    }
}

/// Quelque chose bouge-t-il (opération en cours et pas en pause, ou téléchargement du navigateur qui avance) et la fenêtre est-elle visible, non réduite, animée ?
fn activite_voulue(hwnd: HWND, f: &Fenetre) -> bool {
    // SAFETY: IsWindowVisible lit l'état de `hwnd`, fenêtre principale vivante (fil d'interface)
    let visible = unsafe { IsWindowVisible(hwnd).as_bool() };
    // Un téléchargement du navigateur qui avance fait tourner la grille et glisser Pixel comme une opération ; à sa fin ou en
    // pause (`avance` est faux) la minuterie se désarme au prochain réglage
    visible && !f.reduite && reglages::animations_actives() && (f.apps().any(|a| a.en_activite() && !a.en_pause()) || navigateur::avance())
}

/// Arme ou détruit la minuterie d'activité selon `activite_voulue` : pas en pause, pas réduite, pas cachée, animations permises
/// Invariant (unsafe) : fil d'interface, `hwnd` est la fenêtre principale vivante ; aucune référence n'est gardée après l'appel.
pub(crate) unsafe fn regler_activite(hwnd: HWND, f: &mut Fenetre) {
    let veut = activite_voulue(hwnd, f);
    if veut && !f.activite_armee {
        f.activite_armee = true;
        SetTimer(hwnd, MINUTERIE_ACTIVITE, 100, None);
    } else if !veut && f.activite_armee {
        f.activite_armee = false;
        let _ = KillTimer(hwnd, MINUTERIE_ACTIVITE);
    }
}

/// Minuterie activité : repeint la bande ; elle se détruit dès que rien ne bouge plus
/// Invariant (unsafe) : fil d'interface, `hwnd` est la fenêtre principale vivante ; aucune référence n'est gardée après l'appel.
pub(crate) unsafe fn timer_activite(hwnd: HWND) -> LRESULT {
    if let Some(f) = etat_de(hwnd) {
        if !activite_voulue(hwnd, f) {
            f.activite_armee = false;
            let _ = KillTimer(hwnd, MINUTERIE_ACTIVITE);
        }
        invalider_bande(hwnd, f);
    } else {
        let _ = KillTimer(hwnd, MINUTERIE_ACTIVITE);
    }
    LRESULT(0)
}

/// Règle le clignement : armé seulement si la fenêtre est visible, non réduite, active et les animations autorisées
/// Invariant (unsafe) : fil d'interface, `hwnd` est la fenêtre principale vivante ; aucune référence n'est gardée après l'appel.
pub(crate) unsafe fn regler_clignement(hwnd: HWND, f: &mut Fenetre) {
    let veut = f.active && !f.reduite && IsWindowVisible(hwnd).as_bool() && reglages::animations_actives();
    if veut && !f.clignement_arme {
        f.clignement_arme = true;
        SetTimer(hwnd, MINUTERIE_CLIGNEMENT, pixel::CLIGNEMENT_PERIODE_MS, None);
    } else if !veut && f.clignement_arme {
        f.clignement_arme = false;
        let _ = KillTimer(hwnd, MINUTERIE_CLIGNEMENT);
        invalider_bande(hwnd, f);
    }
}

/// Minuterie clignement (9 s) : les yeux se ferment puis se rouvrent (630 ms) sur l'animation d'écran, qui s'arrête d'elle-même
/// avec le dernier mouvement. Mouvement réduit : rien (la minuterie n'est de toute façon pas armée).
/// Invariant (unsafe) : fil d'interface, `hwnd` est la fenêtre principale vivante ; aucune référence n'est gardée après l'appel.
pub(crate) unsafe fn timer_clignement(hwnd: HWND) -> LRESULT {
    if let Some(f) = etat_de(hwnd) {
        if reglages::animations_actives() && reglages::yeux_pixel_actifs() {
            f.rendu.mouvements.borrow_mut().yeux.cligner(mouvement::maintenant_ms());
            animer(hwnd, f);
        }
    }
    LRESULT(0)
}
