// Fenêtre principale : l'état d'une fenêtre (struct Fenetre et ses méthodes), les messages privés et les petites aides
// de lecture de LPARAM, de touches et de taille. Le reste de la fenêtre est réparti dans les sous-modules.

use crate::*;

mod arbre;
mod clavier;
mod procedure;
mod session;
mod souris;

pub(crate) use arbre::*;
pub(crate) use clavier::*;
pub(crate) use procedure::*;
pub(crate) use session::*;
pub(crate) use souris::*;
pub(crate) const WM_MOUSELEAVE: u32 = 0x02A3;

/// Vérification d'un stockage ajouté terminée (LPARAM = Box<Result<String, String>>)
pub(crate) const WM_STOCKAGE: u32 = WM_APP + 6;

/// Un fil court a déposé le chemin reçu par `explorer.open {reuse}` dans `OUVERTURES` : la fenêtre l'ouvre en onglet.
/// Aucun paramètre (ni pointeur). Voir `recevoir_ouvrir_ici`.
pub(crate) const WM_OUVRIR_ICI: u32 = WM_APP + 31;

/// Boucles modales de Windows en cours (menu du Shell, glisser vers une autre application, boîte de choix) : leur boucle de
/// messages distribue pendant que l'appelant tient un `&mut Fenetre`. Un message qui reprendrait cet état (`WM_TELECHARGEMENT`)
/// ne doit pas agir tout de suite : il attend la fin de la modale.
static MODALES: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
/// Un `WM_TELECHARGEMENT` est arrivé pendant une modale : il est reposté une seule fois, à sa fin
static TELECHARGEMENT_EN_ATTENTE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Posée autour d'un appel modal ; la détruire à sa sortie reposte le message de téléchargement s'il a été retenu.
/// Fil d'interface seulement (les compteurs sont atomiques, mais le repost vise la fenêtre de ce fil).
pub(crate) struct GardeModale(HWND);

impl GardeModale {
    pub(crate) fn nouvelle(hwnd: HWND) -> Self {
        MODALES.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        GardeModale(hwnd)
    }
}

impl Drop for GardeModale {
    fn drop(&mut self) {
        if MODALES.fetch_sub(1, std::sync::atomic::Ordering::SeqCst) == 1 {
            if TELECHARGEMENT_EN_ATTENTE.swap(false, std::sync::atomic::Ordering::SeqCst) {
                // SAFETY: PostMessageW est sans effet de bord sur nos données ; `self.0` est la fenêtre qui a ouvert la modale
                // (échec silencieux si elle n'existe plus)
                unsafe {
                    let _ = PostMessageW(self.0, navigateur::WM_TELECHARGEMENT, WPARAM(0), LPARAM(0));
                }
            }
            // Les réponses de fond de la palette retenues pendant la modale reviennent maintenant
            let retenus = std::mem::take(&mut *PALETTE_RETENUS.lock().unwrap_or_else(|e| e.into_inner()));
            for (msg, p) in retenus {
                // SAFETY: `p` est le pointeur d'une boîte que la palette a donnée au message ; si l'envoi échoue personne ne la recevra
                // et `liberer_palette` la reprend avec son vrai type
                unsafe {
                    if PostMessageW(self.0, msg, WPARAM(0), LPARAM(p)).is_err() {
                        liberer_palette(msg, p);
                    }
                }
            }
        }
    }
}

/// Réponses de fond de la palette (recherche, lecture d'un dossier, fin de compression) arrivées pendant une modale : (message, boîte)
static PALETTE_RETENUS: std::sync::Mutex<Vec<(u32, isize)>> = std::sync::Mutex::new(Vec::new());

/// Pendant une modale l'appelant tient `&mut Fenetre` : une réponse de la palette ne le reprend pas, elle est gardée (avec sa boîte) et
/// reposte à la fin de la modale. Vrai = ne rien faire maintenant ; la propriété de la boîte passe à la liste.
pub(crate) fn retenir_palette(msg: u32, lparam: LPARAM) -> bool {
    if MODALES.load(std::sync::atomic::Ordering::SeqCst) > 0 {
        PALETTE_RETENUS.lock().unwrap_or_else(|e| e.into_inner()).push((msg, lparam.0));
        true
    } else {
        false
    }
}

/// Rend une boîte retenue qu'on n'a pas pu reposter, selon le message qui la portait
unsafe fn liberer_palette(msg: u32, p: isize) {
    match msg {
        WM_PALETTE_RECHERCHE => drop(Box::from_raw(p as *mut (String, Option<Vec<palette::Trouve>>))),
        WM_PALETTE_LECTURE => drop(Box::from_raw(p as *mut assistant::Lecture)),
        WM_PALETTE_TACHE => drop(Box::from_raw(p as *mut Fini)),
        _ => {}
    }
}

/// Une modale tourne-t-elle ? Si oui, le message est retenu (et reposté à la fin) ; vrai = ne rien faire maintenant.
pub(crate) fn retenir_telechargement() -> bool {
    if MODALES.load(std::sync::atomic::Ordering::SeqCst) > 0 {
        TELECHARGEMENT_EN_ATTENTE.store(true, std::sync::atomic::Ordering::SeqCst);
        true
    } else {
        false
    }
}

pub(crate) struct Fenetre {
    pub(crate) rendu: Rendu,
    /// Onglet affiché
    pub(crate) app: App,
    /// Autres onglets, dans l'ordre d'affichage (sans l'onglet affiché)
    pub(crate) fond: Vec<App>,
    /// Position de l'onglet affiché dans la barre
    pub(crate) rang: usize,
    pub(crate) dpi: f32,
    pub(crate) suivi_souris: bool,
    pub(crate) anime: bool,
    pub(crate) apercu_visible: bool,
    pub(crate) apercu: Option<Apercu>,
    /// Chemin dont l'aperçu a été demandé, et numéro de la demande
    pub(crate) apercu_demande: Option<String>,
    pub(crate) apercu_numero: u64,
    pub(crate) apercu_attente: Option<(String, Option<Entree>)>,
    /// Panneau terminal intégré ouvert (bouton `>_` du chemin ou Ctrl+Alt+T)
    pub(crate) terminal_visible: bool,
    /// Dernières opérations chargées à l'ouverture du panneau (contenu réel du journal)
    pub(crate) terminal_lignes: Vec<crate::journal::Operation>,
    pub(crate) par_defaut: bool,
    /// Page des réglages (commune à tous les onglets)
    pub(crate) page: Page,
    /// Cloud détecté (OneDrive, Google Drive…), relu avec les disques
    pub(crate) detectes: Vec<Lieu>,
    /// États mesurés des lieux du volet (lot 14b), la mesure en cours et l'heure de la dernière
    pub(crate) etats: crate::etats::Cache,
    pub(crate) etats_en_cours: bool,
    pub(crate) etats_mesure: Option<std::time::Instant>,
    /// Résident Win+E activé
    pub(crate) win_e: bool,
    /// Double panneau (F6) : l'autre panneau ; `app` est toujours celui qui a le focus
    pub(crate) second: Option<App>,
    /// Le panneau qui a le focus est celui de droite
    pub(crate) focus_droite: bool,
    /// Une touche a été utilisée depuis le dernier clic : le focus clavier se voit (anneau de 2 px).
    pub(crate) focus_clavier: bool,
    pub(crate) palette: Option<Palette>,
    /// Panneau Historique (lot 15B, Ctrl+Maj+H) : tiroir à droite ouvert ; il glisse (mouvement.rs) et ne prend pas le focus
    pub(crate) historique: bool,
    /// Défilement du tiroir (DIP depuis le haut de la liste) ; borné au dessin
    pub(crate) historique_defilement: f32,
    pub(crate) lot: Option<Lot>,
    pub(crate) galerie: Option<Galerie>,
    pub(crate) galerie_numero: u64,
    /// La touche vient d'être prise par la carte des conflits : son caractère (WM_CHAR) ne va pas au filtre
    pub(crate) avaler_caractere: bool,
    /// Menu Nothing ouvert (barre d'actions, clic droit)
    pub(crate) menu: Option<menu::Menu>,
    /// Le bouton enfoncé a fermé un menu : son relâchement ne fait rien d'autre
    pub(crate) avaler_clic: bool,
    /// Applications du sous-menu OUVRIR AVEC du dernier clic droit
    pub(crate) applications: Vec<coquille::Application>,
    /// Dépôt avec le bouton droit en attente du menu : (sources, dossier)
    pub(crate) depot_droit: Option<(Vec<String>, String)>,
    /// Bouton droit enfoncé sur un élément choisi : point de départ d'un éventuel glisser (px)
    pub(crate) droit_depart: Option<POINT>,
    /// Fils de préparation des vignettes de la grille (créés au premier besoin)
    pub(crate) vignettes: Option<Vignettes>,
    /// Minuterie d'effacement de l'annonce déjà lancée : la relancer à chaque rendu la repousserait sans fin
    pub(crate) annonce_armee: bool,
    /// Emplacements des derniers onglets fermés (10 au plus), le plus récent en dernier : Ctrl+Maj+T les rouvre
    pub(crate) fermes: Vec<Emplacement>,
    /// Dernière signature des onglets vue au rendu (`vue_session`) : un écart arme la minuterie d'écriture de la session
    pub(crate) session_vue: Option<(Vec<Option<String>>, usize)>,
    /// Minuterie d'écriture de la session lancée : un changement de plus pendant l'attente ne la relance pas
    pub(crate) session_armee: bool,
    /// La fenêtre est active (WM_ACTIVATE) et n'est pas réduite (WM_SIZE) : conditions du clignement de Pixel
    pub(crate) active: bool,
    pub(crate) reduite: bool,
    /// Minuterie de clignement (9 s) armée ; elle lance un clignement de 630 ms (`Yeux::cligner`)
    pub(crate) clignement_arme: bool,
    /// Regard voulu des yeux de Pixel (DIP, quantifié), calculé depuis `pointeur` : un écart avec la valeur précédente est la seule
    /// raison de repeindre la bande sur un `WM_MOUSEMOVE`
    pub(crate) regard_voulu: (f32, f32),
    /// Heure (`mouvement::maintenant_ms`) de la dernière image peinte pendant que seuls les yeux de Pixel bougent
    pub(crate) image_yeux: u64,
    /// Pixel est dans la palette (et non dans la barre d'état), et depuis quand il a changé de place (glissement de 0,5 s)
    pub(crate) pixel_dans_palette: bool,
    pub(crate) relais_depuis: Option<std::time::Instant>,
    /// Une commande de la palette vient d'être lancée : Pixel se plisse (chevrons) un instant
    pub(crate) plisse_depuis: Option<std::time::Instant>,
    /// Pastille déployée par un clic (le survol déploie aussi, sans la fixer), et Échap l'a repliée pendant le survol
    pub(crate) pastille_fixee: Option<usize>,
    pub(crate) pastille_masquee: bool,
    /// Minuterie d'activité (100 ms) armée par `regler_activite`
    pub(crate) activite_armee: bool,
    /// Position du dernier `WM_MOUSEMOVE` (DIP, fenêtre) : centre du reflet. `None` : souris sortie, ou une touche a été pressée
    /// (un élément qui a le focus clavier n'est pas éclairé : la position du curseur ne dit rien)
    pub(crate) pointeur: Option<(f32, f32)>,
    /// Le bouton de la souris est enfoncé : le chrome survolé retombe à plat en 60 ms (`.chrome:active`). Le fondu et le ressort
    /// de survol sont suivis au dessin (`mouvement::Survols`).
    pub(crate) survol_appui: bool,
    /// La mesure de coût du reflet (`SPACEFILES_MESURE_REFLET`) est faite : elle ne tourne qu'une fois
    pub(crate) mesure_reflet_faite: bool,
    /// Dernière valeur de `reflet::actif_maintenant()` vue par `WM_APPARENCE` : un changement redessine toute la fenêtre
    pub(crate) reflet_actif: bool,
    /// Ouverture de la fenêtre (lot 12A) : demandée au lancement, l'horloge (`mouvement::maintenant_ms`) ne démarre qu'après le
    /// premier dessin (sinon la préparation de la cible Direct2D mangerait l'animation). `fermeture` : début de la sortie.
    pub(crate) ouverture_attend: bool,
    pub(crate) ouverture: Option<u64>,
    pub(crate) fermeture: Option<u64>,
    /// Mesure de l'ouverture (mode essai) : heure du premier dessin animé et images dessinées depuis
    pub(crate) mesure_ouverture: Option<(u64, u32)>,
    /// Mesure de la régularité des images (mode essai `SPACEFILES_MESURE_MOUVEMENT=1`)
    pub(crate) cadence: mouvement::Cadence,
    /// Fil de synchronisation sur l'écran (`DwmFlush`) tant qu'une animation tourne ; `true` dedans le fait sortir
    pub(crate) metronome: Option<std::sync::Arc<std::sync::atomic::AtomicBool>>,
}

/// Durée du glissement de Pixel entre la barre d'état et la palette, et du pli au lancement d'une commande
pub(crate) const RELAIS_MS: u128 = 500;

impl Fenetre {
    /// Convertit des pixels physiques en DIP
    pub(crate) fn dip(&self, px: i32) -> f32 {
        px as f32 * 96.0 / self.dpi
    }

    /// Un mouvement du lot 12A n'a pas fini : ouverture, fermeture ou glissement (pastille, vue, onglet). La minuterie
    /// d'animation reste armée pendant ce temps et s'arrête d'elle-même ensuite.
    pub(crate) fn mouvement_en_cours(&self, maintenant: u64) -> bool {
        self.mouvement_hors_yeux(maintenant) || self.rendu.mouvements.borrow().yeux.en_cours(maintenant)
    }

    /// Un mouvement autre que les yeux de Pixel n'a pas fini (ouverture, fermeture, glissements, fondus)
    pub(crate) fn mouvement_hors_yeux(&self, maintenant: u64) -> bool {
        self.ouverture_attend
            || self.fermeture.is_some()
            || self.ouverture.is_some_and(|d| maintenant < d + mouvement::OUVERTURE_MS.max(mouvement::CASCADE_TOTAL_MS))
            || self.rendu.mouvements.borrow().en_cours_hors_yeux(maintenant)
    }

    pub(crate) fn nb_onglets(&self) -> usize {
        self.fond.len() + 1
    }

    pub(crate) fn titres(&self) -> Vec<String> {
        let mut v: Vec<String> = self.fond.iter().map(App::titre).collect();
        v.insert(self.rang, self.app.titre());
        v
    }

    /// Tous les onglets, affiché compris
    pub(crate) fn tous(&mut self) -> impl Iterator<Item = &mut App> {
        std::iter::once(&mut self.app).chain(self.fond.iter_mut()).chain(self.second.iter_mut())
    }

    /// Pastilles de tous les onglets, dans l'ordre d'affichage, avec l'onglet de chacune (rang dans `tous()` ; `usize::MAX` pour
    /// l'annulation, commune à tous les onglets comme l'historique)
    pub(crate) fn pastilles(&self) -> Vec<(pastilles::Pastille, usize)> {
        let mut v: Vec<(pastilles::Pastille, usize)> = std::iter::once(&self.app)
            .chain(self.fond.iter())
            .chain(self.second.iter())
            .enumerate()
            .flat_map(|(k, a)| a.pastilles_propres().into_iter().map(move |p| (p, k)))
            .collect();
        // Téléchargements du navigateur : communs à tous les onglets (un seul abonnement pour le processus), comme l'annulation
        v.extend(navigateur::pastilles().into_iter().map(|p| (p, usize::MAX)));
        if let Some(l) = app::annulation_proposee() {
            v.push((pastilles::Pastille::annulation(l), usize::MAX));
        }
        // Cartes d'information (toasts) : communes à tous les onglets
        v.extend(app::toasts_vivants().into_iter().map(|p| (p, usize::MAX)));
        v.sort_by_key(|(p, _)| pastilles::rang_ordre(p));
        v
    }

    /// Tous les onglets en lecture seule (même ordre que `tous`)
    pub(crate) fn apps(&self) -> impl Iterator<Item = &App> {
        std::iter::once(&self.app).chain(self.fond.iter()).chain(self.second.iter())
    }

    /// Une fenêtre modale cache les pastilles : palette, renommage par lot, coup d'œil, menu, carte des conflits
    pub(crate) fn modale(&self) -> bool {
        self.palette.is_some() || self.lot.is_some() || self.galerie.is_some() || self.menu.is_some() || self.apps().any(|a| a.conflits.is_some())
    }

    /// Une pastille de fin, d'erreur ou d'annulation attend d'être effacée (tous les onglets, pas seulement l'affiché)
    pub(crate) fn pastille_temporaire_presente(&self) -> bool {
        self.apps().any(|a| a.fin_op.is_some()) || app::annulation_proposee().is_some() || navigateur::temporaires() || !app::toasts_vivants().is_empty()
    }

    /// Pastille déployée : celle qui est survolée (sauf si Échap l'a repliée), sinon celle qu'un clic a fixée
    pub(crate) fn pastille_ouverte(&self, nombre: usize) -> Option<usize> {
        let survolee = match self.app.survol {
            Cible::Pastille(i) | Cible::BoutonPastille(i, _) if !self.pastille_masquee => Some(i),
            _ => None,
        };
        survolee.or(self.pastille_fixee).filter(|&i| i < nombre)
    }

    /// Ce que Pixel doit dire maintenant (état, phase du mouvement, heure, relais vers la palette). Déclenche le glissement
    /// quand la palette s'ouvre ou se ferme. Mouvement réduit : pose fixe, bascule immédiate.
    /// Invariant (unsafe) : fil d'interface, `hwnd` est la fenêtre principale vivante ; aucune référence n'est gardée après l'appel.
    pub(crate) unsafe fn vue_pixel(&mut self, hwnd: HWND) -> rendu::VuePixel {
        // Pause, reprise, fin d'opération : la minuterie d'activité suit (armée seulement si quelque chose bouge)
        regler_activite(hwnd, self);
        let anime = reglages::animations_actives();
        let dans = self.palette.is_some();
        if dans != self.pixel_dans_palette {
            self.pixel_dans_palette = dans;
            self.relais_depuis = Some(std::time::Instant::now());
            // La minuterie d'animation existante porte le glissement (elle s'arrête d'elle-même quand il est fini)
            animer(hwnd, self);
        }
        let cible = if dans { 1.0 } else { 0.0 };
        let place = match self.relais_depuis {
            Some(t) if anime && t.elapsed().as_millis() < RELAIS_MS => {
                let x = t.elapsed().as_millis() as f32 / RELAIS_MS as f32;
                // Ressort : arrivée rapide qui ralentit, comme le défilement
                let e = 1.0 - (1.0 - x).powi(3);
                if dans { e } else { 1.0 - e }
            }
            _ => {
                self.relais_depuis = None;
                cible
            }
        };
        if self.plisse_depuis.is_some_and(|t| t.elapsed().as_millis() >= RELAIS_MS) || !anime && self.plisse_depuis.is_some() {
            self.plisse_depuis = None;
        }
        let plisse = self.plisse_depuis.is_some();
        // En pause rien ne bouge : Pixel ne glisse plus
        // Un téléchargement du navigateur qui avance compte aussi (seuls ceux dont la taille bouge ; pas ceux en pause)
        let activite = self.apps().any(|a| a.en_activite() && !a.en_pause()) || navigateur::avance();
        let conflit = self.apps().any(|a| a.conflits.is_some());
        let erreur = self.apps().any(|a| a.en_erreur());
        let reussite = plisse || self.apps().any(|a| a.reussite_recente()) || navigateur::reussite_recente(pixel::REUSSITE_MS);
        // SAFETY: simple lecture de l'heure locale
        let heure = windows::Win32::System::SystemInformation::GetLocalTime().wHour as u32;
        let periode = pixel::periode_effective(reglages::choix_pixel(), heure);
        let etat = pixel::etat_pixel(activite, conflit, erreur, reussite, periode == pixel::Periode::Nuit);
        let debut = self.apps().filter_map(|a| a.operation.as_ref().map(|o| o.debut)).chain(navigateur::debut()).min();
        let ecoule = debut.map_or(0, |d| d.elapsed().as_millis());
        // Lot 13 : regard vers le pointeur, nuit, fondu de couleur et clignement viennent de `Yeux` (lu à chaque image, aucune
        // minuterie propre : le regard est repeint par le `WM_MOUSEMOVE`, le clignement par la minuterie de 9 s)
        let regard_voulu = self.calculer_regard_voulu(hwnd);
        self.regard_voulu = regard_voulu;
        let t = mouvement::maintenant_ms();
        // Lot 15A : une commande vient d'être lancée depuis la palette (`.pe.hp`) : barres de 5 DIP, couleur d'accent
        let lance = self.palette.as_ref().is_some_and(|p| p.lance);
        let teinte = if lance { pixel::Teinte::Ambre } else { pixel::teinte(etat, periode) };
        let yeux = {
            let mut m = self.rendu.mouvements.borrow_mut();
            m.yeux.contente(lance, t, anime);
            m.yeux.maj(regard_voulu, etat == pixel::EtatPixel::Nuit, teinte, t, anime)
        };
        let phase = match etat {
            // Mouvement réduit : pose fixe, décalée d'un cran vers la droite pour rester distincte du repos
            pixel::EtatPixel::Activite if anime => pixel::phase_glissement(ecoule),
            pixel::EtatPixel::Activite => 0.25,
            // Le clignement ne joue pas quand la nuit ferme déjà les yeux (`animation:none` de la maquette)
            pixel::EtatPixel::Repos if anime => yeux.clignement * (1.0 - yeux.aplatie),
            _ => 0.0,
        };
        rendu::VuePixel { etat, periode, phase, place, grille: anime.then(|| (ecoule % 1200) as f32 / 1200.0), yeux }
    }

    /// Regard que les yeux de Pixel doivent viser : vers le pointeur quand il est dans la fenêtre, la palette fermée, les animations
    /// et les yeux autorisés ; sinon droit devant. Arrondi au demi-DIP (`pixel::regard_pointeur`).
    /// Invariant (unsafe) : fil d'interface, `hwnd` est la fenêtre principale vivante (lecture de sa taille).
    pub(crate) unsafe fn calculer_regard_voulu(&self, hwnd: HWND) -> (f32, f32) {
        // Lot 15A : dans la palette, Pixel regarde vers la ligne choisie (`palEyes`, de -4 à +4 DIP vers le bas selon son rang), pas vers
        // le pointeur ; le glissement de 350 ms est celui du regard. Mouvement réduit : posé tout de suite (le regard reste).
        if let Some(p) = &self.palette {
            // Pendant le lancement les yeux se centrent (`palRun` : `palEyes(0,'hp')`)
            return if reglages::yeux_pixel_actifs() && !p.lance { (0.0, pixel::regard(p.choix, p.items.len())) } else { (0.0, 0.0) };
        }
        let actif = reglages::animations_actives() && reglages::yeux_pixel_actifs();
        if !actif {
            return (0.0, 0.0);
        }
        let (_, h) = taille_client(hwnd);
        pixel::regard_pointeur(self.pointeur, rendu::pixel_dans_statut(self.dip(h as i32)), true)
    }

    /// Le pointeur a bougé, est sorti ou a été retiré par une touche : si le regard voulu change, la bande de la barre d'état est
    /// repeinte (le dessin lance le glissement de 350 ms). Un déplacement qui ne change pas le regard arrondi ne coûte rien.
    /// Invariant (unsafe) : fil d'interface, `hwnd` est la fenêtre principale vivante ; aucune référence n'est gardée après l'appel.
    pub(crate) unsafe fn suivre_regard(&mut self, hwnd: HWND) {
        let voulu = self.calculer_regard_voulu(hwnd);
        if voulu != self.regard_voulu {
            self.regard_voulu = voulu;
            invalider_bande(hwnd, self);
        }
    }

    /// Un clic sur un bouton d'une pastille : `rang` est le rang dans `pastilles()`
    /// Invariant (unsafe) : fil d'interface, `hwnd` est la fenêtre principale vivante ; aucune référence n'est gardée après l'appel.
    pub(crate) unsafe fn clic_bouton_pastille(&mut self, hwnd: HWND, rang: usize, bouton: pastilles::Bouton) {
        // Une modale cache les pastilles : aucun clic ne les traverse (le dessin précédent a pu les montrer avant son ouverture)
        if self.modale() {
            return;
        }
        let liste = self.pastilles();
        let Some((p, onglet)) = liste.get(rang) else { return };
        // Le rang vient du dessin précédent : la pastille trouvée doit être celle qui a été dessinée (même clé) et porter ce bouton
        let dessinee = self.rendu.zones_pastilles.borrow().places.iter().find(|pl| pl.index == rang).map(|pl| pl.cle);
        if dessinee != Some(p.cle) || !pastilles::bouton_autorise(p, bouton) {
            return;
        }
        let destination = p.destination.clone();
        let onglet = *onglet;
        // Pastille d'un téléchargement du navigateur : ni onglet ni opération, la clé dit laquelle
        if navigateur::est_cle(p.cle) {
            match bouton {
                pastilles::Bouton::Afficher => {
                    // Le dossier s'ouvre sur le fichier choisi (même chemin que « montrer » de la palette) ; la pastille n'est
                    // fermée que si le fichier a vraiment été montré, sinon elle reste (le fichier a pu disparaître)
                    if let Some(chemin) = navigateur::chemin_fini(p.cle) {
                        if self.app.montrer(hwnd, &chemin) {
                            navigateur::fermer(p.cle);
                        }
                    }
                }
                pastilles::Bouton::Fermer => navigateur::fermer(p.cle),
                // Pas d'autre bouton sur ces pastilles (`bouton_autorise` l'a déjà refusé)
                _ => {}
            }
            self.pastille_fixee = None;
            return;
        }
        // Carte d'information : ni onglet ni opération, la clé dit laquelle (le ✕ est son seul bouton)
        if p.genre == pastilles::Genre::Info {
            if bouton == pastilles::Bouton::Fermer {
                app::fermer_toast(p.cle);
            }
            self.pastille_fixee = None;
            return;
        }
        match bouton {
            pastilles::Bouton::Pause => {
                if let Some(a) = self.tous().nth(onglet) {
                    a.basculer_pause();
                }
            }
            pastilles::Bouton::Annuler => {
                if let Some(a) = self.tous().nth(onglet) {
                    a.annuler_operation();
                }
            }
            pastilles::Bouton::Destination => {
                if let Some(d) = destination {
                    self.app.naviguer(hwnd, Emplacement::dossier(&d));
                }
            }
            pastilles::Bouton::Fermer => {
                if let Some(a) = self.tous().nth(onglet) {
                    a.fin_op = None;
                }
            }
            pastilles::Bouton::AnnulerAction => {
                self.app.annuler_derniere(hwnd);
                app::oublier_annulation();
            }
            // Réservé aux téléchargements du navigateur, traités plus haut
            pastilles::Bouton::Afficher => {}
        }
        self.pastille_fixee = None;
    }

    /// Retire les pastilles périmées ; vrai s'il en reste une qui doit disparaître seule (la minuterie d'annonce continue)
    pub(crate) fn pastilles_temporaires(&mut self) -> bool {
        let mut reste = false;
        for a in self.tous() {
            reste |= a.expirer_pastilles();
        }
        reste |= navigateur::expirer();
        reste |= app::purger_toasts();
        reste || app::annulation_proposee().is_some()
    }

    /// Ouvre un onglet juste après l'onglet affiché
    pub(crate) unsafe fn nouvel_onglet(&mut self, hwnd: HWND, emp: Emplacement) {
        let mut neuf = App::nouveau(emp);
        neuf.disques = self.app.disques.clone();
        neuf.nuages = self.app.nuages.clone();
        neuf.serveurs = self.app.serveurs.clone();
        neuf.etats = self.etats.clone();
        neuf.epingles = epingles_volet();
        let ancien = std::mem::replace(&mut self.app, neuf);
        self.fond.insert(self.rang, ancien);
        self.rang += 1;
        self.app.charger(hwnd);
    }

    /// Affiche l'onglet `k` (position dans la barre)
    pub(crate) fn aller_onglet(&mut self, k: usize) {
        if k >= self.nb_onglets() || k == self.rang {
            return;
        }
        let j = if k < self.rang { k } else { k - 1 };
        std::mem::swap(&mut self.app, &mut self.fond[j]);
        // L'ancien onglet affiché reprend sa place dans la barre
        let ancien = self.fond.remove(j);
        let place = if k < self.rang { self.rang - 1 } else { self.rang };
        self.fond.insert(place, ancien);
        self.rang = k;
        self.apercu_demande = None;
        // Colonnes et lignes sont propres au dossier affiché : celles du nouvel onglet
        reglages::afficher_dossier(self.app.dossier());
    }

    /// Stockages ajoutés (dans l'ordre des réglages)
    pub(crate) fn ajoutes(&self) -> Vec<Lieu> {
        let r = reglages::actuels();
        let (cloud, serveurs) = stockages::lieux_ajoutes(&r.stockages, r.experimental);
        let mut v: Vec<Lieu> = cloud.into_iter().chain(serveurs).collect();
        v.sort_by_key(|l| l.ajoute);
        v
    }

    /// La page des réglages telle que la lit un lecteur d'écran (None hors des réglages)
    pub(crate) fn page_lue(&self) -> Option<accessibilite::PageLue> {
        if self.app.emplacement != Emplacement::Reglages {
            return None;
        }
        let lignes = self.lignes();
        let (cartes, focus) = page::noms_accessibles(&lignes, self.page.categorie, self.page.focus);
        Some(accessibilite::PageLue { categorie: page::CATEGORIES[self.page.categorie].to_string(), cartes, focus })
    }

    /// Lignes de la page des réglages
    pub(crate) fn lignes(&self) -> Vec<page::Ligne> {
        page::lignes(&reglages::actuels(), &self.detectes, &self.ajoutes(), self.par_defaut, self.win_e)
    }

    /// Volet de tous les onglets : cloud détecté + stockages ajoutés
    pub(crate) fn rafraichir_lieux(&mut self) {
        let r = reglages::actuels();
        let (cloud, mut serveurs) = stockages::lieux_ajoutes(&r.stockages, r.experimental);
        // NAS connu du client Zima, s'il n'a pas déjà été ajouté à la main
        if let Some(z) = stockages::zima_client() {
            if !serveurs.iter().any(|s| s.detail.eq_ignore_ascii_case(z.detail.rsplit(' ').next().unwrap_or(""))) {
                serveurs.insert(0, z);
            }
        }
        let nuages: Vec<Lieu> = self.detectes.iter().cloned().chain(cloud).collect();
        let epingles = epingles_volet();
        let etats = self.etats.clone();
        for a in self.tous() {
            a.nuages = nuages.clone();
            a.serveurs = serveurs.clone();
            a.epingles = epingles.clone();
            a.etats = etats.clone();
        }
    }

    /// Lot 14b : mesure l'état des lignes CLOUD et SERVEURS dans un fil (jamais au dessin). Fenêtre cachée ou réduite : rien.
    /// Une mesure à la fois, au plus toutes les 30 s, sauf s'il y a un lieu jamais mesuré.
    pub(crate) unsafe fn mesurer_etats(&mut self, hwnd: HWND) {
        if self.etats_en_cours || self.reduite || !IsWindowVisible(hwnd).as_bool() {
            return;
        }
        let d = etats::demandes(&self.app.nuages, &self.app.serveurs);
        if d.is_empty() {
            return;
        }
        let nouveau = d.iter().any(|x| self.etats.lire(&x.cle).is_none());
        if !nouveau && self.etats_mesure.is_some_and(|t| t.elapsed() < etats::ECART) {
            return;
        }
        self.etats_en_cours = true;
        self.etats_mesure = Some(std::time::Instant::now());
        etats::lancer(hwnd, d);
    }

    /// Résultat d'une mesure : le cache est remplacé et recopié dans tous les onglets
    pub(crate) fn recevoir_etats(&mut self, mesures: Vec<(String, etats::Etat)>) {
        self.etats_en_cours = false;
        self.etats.remplacer(mesures);
        let etats = self.etats.clone();
        for a in self.tous() {
            a.etats = etats.clone();
        }
    }

    /// Double panneau : le focus passe à l'autre panneau (Tab, clic)
    pub(crate) fn basculer_panneau(&mut self) {
        if let Some(o) = &mut self.second {
            std::mem::swap(&mut self.app, o);
            self.focus_droite = !self.focus_droite;
            self.apercu_demande = None;
        }
    }

    /// F6 : ouvre ou ferme le double panneau
    pub(crate) unsafe fn double_panneau(&mut self, hwnd: HWND) {
        if self.second.take().is_some() {
            self.focus_droite = false;
            self.app.annoncer("PANNEAU UNIQUE", false);
            return;
        }
        let mut o = App::nouveau(self.app.emplacement.clone());
        o.disques = self.app.disques.clone();
        o.nuages = self.app.nuages.clone();
        o.serveurs = self.app.serveurs.clone();
        o.etats = self.etats.clone();
        o.epingles = self.app.epingles.clone();
        o.charger(hwnd);
        self.second = Some(o);
        self.focus_droite = false;
        self.app.annoncer("DOUBLE PANNEAU · TAB : CHANGER DE PANNEAU · F5 / MAJ+F6 : COPIER / DEPLACER EN FACE", false);
    }

    /// F5 (double panneau), Maj+F5 / Maj+F6 : copie ou déplace la sélection dans le dossier de l'autre panneau
    pub(crate) unsafe fn vers_autre(&mut self, hwnd: HWND, deplacer: bool) {
        let Some(dest) = self.second.as_ref().and_then(|o| o.dossier().map(str::to_string)) else {
            self.app.annoncer("OUVREZ LE DOUBLE PANNEAU (F6) SUR UN DOSSIER", true);
            return;
        };
        let chemins = self.app.chemins_choisis();
        if chemins.is_empty() || self.app.sans_fichiers() {
            return;
        }
        // L'autre panneau resélectionne ce qui arrive
        self.app.deposer(hwnd, chemins, dest, deplacer);
    }

    /// Ctrl+D : épingle le dossier ouvert dans le volet (ou l'en retire)
    pub(crate) fn epingler(&mut self) {
        // Une recherche dans tout le PC s'épingle comme un dossier virtuel (« ?motif »)
        let Some(d) = (match &self.app.emplacement {
            Emplacement::Globale { motif } => Some(format!("?{motif}")),
            _ => self.app.dossier().map(str::to_string),
        }) else {
            self.app.annoncer("SEUL UN DOSSIER OU UNE RECHERCHE PEUT ETRE EPINGLE", true);
            return;
        };
        let deja = reglages::actuels().epingles.iter().any(|e| e.eq_ignore_ascii_case(&d));
        reglages::modifier(|r| {
            if deja {
                r.epingles.retain(|e| !e.eq_ignore_ascii_case(&d));
            } else {
                r.epingles.push(d.clone());
            }
        });
        self.rafraichir_lieux();
        self.app.annoncer(if deja { "RETIRE DES EPINGLES" } else if d.starts_with('?') { "RECHERCHE ENREGISTREE DANS LE VOLET" } else { "EPINGLE DANS LE VOLET" }, false);
        sons::jouer(sons::Son::Tic);
    }

    /// Recalcule les propositions de la palette
    pub(crate) fn remplir_palette(&mut self) {
        let r = reglages::actuels();
        let dossier_ouvert = self.app.dossier().map(str::to_string);
        // Les éléments du dossier ouvert : le chemin se compose sans rien lire (jamais `chemin_de`, qui touche la corbeille ou le disque)
        let fichiers = match &dossier_ouvert {
            Some(d) => self.app.entrees.iter().take(5000).map(|e| (dossier::joindre(d, &e.nom), e.est_conteneur(), icones::pour(e))).collect(),
            None => Vec::new(),
        };
        // Corbeille et panneau : la sélection y donne les fichiers `$R…`, pas les éléments que la liste montre. La palette ne la propose
        // pas (elle les déplacerait ou les jetterait sous un faux nom) ; les éléments restent atteignables en tapant leur nom.
        let selection = if self.app.sans_fichiers() || assistant::emplacement_sans_selection(&self.app.emplacement) {
            Vec::new()
        } else {
            self.app.choisis().into_iter().filter_map(|i| self.app.entrees.get(i)).filter_map(|e| Some((self.app.chemin_de(e)?, e.est_conteneur(), icones::pour(e)))).collect()
        };
        let volet = self.app.favoris.iter().chain(self.app.epingles.iter()).filter_map(|v| if let Emplacement::Dossier(c) = &v.emplacement { Some((v.nom.clone(), c.clone())) } else { None }).collect();
        let autre_panneau = self.second.as_ref().and_then(|o| o.dossier().map(str::to_string));
        let recents_elements = assistant::elements_recents(&r.reprises, &r.recents);
        if let Some(p) = &mut self.palette {
            p.remplir(&palette::Sources {
                epingles: &r.epingles,
                recents: &r.recents,
                recents_elements,
                fichiers,
                groupes: &self.app.groupes,
                sessions: &r.sessions,
                selection,
                dossier_ouvert,
                volet,
                autre_panneau,
            });
        }
    }

    /// Emplacements de tous les onglets, dans l'ordre de la barre
    pub(crate) fn emplacements(&self) -> Vec<Emplacement> {
        let mut v: Vec<Emplacement> = self.fond.iter().map(|a| a.emplacement.clone()).collect();
        v.insert(self.rang, self.app.emplacement.clone());
        v
    }

    /// Rouvre le dernier onglet fermé, à l'emplacement où il était
    pub(crate) unsafe fn rouvrir_onglet(&mut self, hwnd: HWND) {
        match self.fermes.pop() {
            Some(emp) => self.nouvel_onglet(hwnd, emp),
            None => self.app.annoncer("AUCUN ONGLET FERME A ROUVRIR", false),
        }
    }

    pub(crate) unsafe fn fermer_onglet(&mut self, hwnd: HWND, k: usize) {
        if self.fond.is_empty() {
            let _ = PostMessageW(hwnd, WM_CLOSE, WPARAM(0), LPARAM(0));
            return;
        }
        let parti = if k == self.rang { Some(&self.app) } else if k < self.nb_onglets() { self.fond.get(if k < self.rang { k } else { k - 1 }) } else { None };
        if let Some(e) = parti.map(|a| a.emplacement.clone()).filter(|e| *e != Emplacement::Reglages) {
            self.fermes.push(e);
            if self.fermes.len() > 10 {
                self.fermes.remove(0);
            }
        }
        if k == self.rang {
            self.app = if self.rang < self.fond.len() {
                self.fond.remove(self.rang)
            } else {
                self.rang -= 1;
                self.fond.remove(self.rang)
            };
            self.apercu_demande = None;
        } else if k < self.nb_onglets() {
            let j = if k < self.rang { k } else { k - 1 };
            self.fond.remove(j);
            if k < self.rang {
                self.rang -= 1;
            }
        }
    }
}

pub(crate) unsafe fn etat_de<'a>(hwnd: HWND) -> Option<&'a mut Fenetre> {
    let p = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut Fenetre;
    p.as_mut()
}

pub(crate) fn x_lparam(l: LPARAM) -> i32 {
    (l.0 & 0xFFFF) as i16 as i32
}

pub(crate) fn y_lparam(l: LPARAM) -> i32 {
    ((l.0 >> 16) & 0xFFFF) as i16 as i32
}

pub(crate) unsafe fn taille_client(hwnd: HWND) -> (u32, u32) {
    let mut r = RECT::default();
    let _ = GetClientRect(hwnd, &mut r);
    ((r.right - r.left) as u32, (r.bottom - r.top) as u32)
}

/// Combinaison écrite comme dans la table des raccourcis (« CTRL+ALT+MAJ+D ») ; rien pour une lettre seule, qui sert à la frappe
pub(crate) fn combinaison(vk: VIRTUAL_KEY, ctrl: bool, alt: bool, maj: bool) -> Option<String> {
    let nom = match vk.0 {
        0x41..=0x5A | 0x30..=0x39 => (vk.0 as u8 as char).to_string(),
        0x70..=0x7B => format!("F{}", vk.0 - 0x6F),
        0x0D => "ENTREE".into(),
        0x2E => "SUPPR".into(),
        0x20 => "ESPACE".into(),
        0x09 => "TAB".into(),
        _ => return None,
    };
    if !(ctrl || alt || nom.starts_with('F') && nom.len() > 1) {
        return None;
    }
    Some([(ctrl, "CTRL+"), (alt, "ALT+"), (maj, "MAJ+")].iter().filter(|x| x.0).map(|x| x.1).collect::<String>() + &nom)
}

/// Ctrl+K ou Ctrl+Maj+P : ouvre la palette, et la ferme quand elle est ouverte
pub(crate) fn bascule_palette(vk: VIRTUAL_KEY, ctrl: bool, alt: bool, maj: bool) -> bool {
    combinaison(vk, ctrl, alt, maj).is_some_and(|c| crate::palette::TOUCHES_PALETTE.contains(&c.as_str()))
}

/// Ce que fait F5 selon le mode (lot 15B, `if(k==='F5'&&dual)` de la maquette)
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum ActionF5 {
    /// Copier la sélection dans le dossier de l'autre panneau
    Copier,
    /// Relire le dossier
    Actualiser,
}

/// F5 copie vers l'autre panneau en double panneau (Maj+F5 le fait toujours, et dit d'ouvrir le double panneau s'il manque) ;
/// avec Ctrl ou Alt, ou en simple panneau, il actualise comme avant.
pub(crate) fn action_f5(double: bool, maj: bool, ctrl: bool, alt: bool) -> ActionF5 {
    if maj || (double && !ctrl && !alt) {
        ActionF5::Copier
    } else {
        ActionF5::Actualiser
    }
}

/// Mode essai seulement (`SPACEFILES_SANS_REGISTRE=1`) : les modificateurs postés (WM_KEYDOWN / WM_KEYUP de Ctrl, Maj, Alt) comptent comme
/// pressés, car `GetKeyState` ne voit que le vrai clavier et une capture ne doit ni injecter de touche ni prendre le focus de l'auteur.
static MODIFICATEURS_ESSAI: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(0);

fn essai_actif() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(defaut::sans_registre)
}

/// Retient l'appui ou le relâchement d'un modificateur posté (sans effet hors du mode essai)
pub(crate) fn noter_modificateur_essai(vk: u16, enfonce: bool) {
    if !essai_actif() {
        return;
    }
    let bit = match vk {
        0x11 | 0xA2 | 0xA3 => 1,
        0x10 | 0xA0 | 0xA1 => 2,
        0x12 | 0xA4 | 0xA5 => 4,
        _ => return,
    };
    if enfonce {
        MODIFICATEURS_ESSAI.fetch_or(bit, std::sync::atomic::Ordering::Relaxed);
    } else {
        MODIFICATEURS_ESSAI.fetch_and(!bit, std::sync::atomic::Ordering::Relaxed);
    }
}

pub(crate) fn touche(vk: VIRTUAL_KEY) -> bool {
    if essai_actif() {
        let bit = match vk {
            VK_CONTROL => 1,
            VK_SHIFT => 2,
            VK_MENU => 4,
            _ => 0,
        };
        if bit != 0 && MODIFICATEURS_ESSAI.load(std::sync::atomic::Ordering::Relaxed) & bit != 0 {
            return true;
        }
    }
    unsafe { GetKeyState(vk.0 as i32) < 0 }
}

/// Mode sombre immersif, couleur de la barre et de la bordure selon le thème. La barre DWM est masquée par le client
/// (WS_SYSMENU retiré) ; la bordure 1 px est peinte par DWM, sans alpha : on lui donne la couleur composée `--s2` sur
/// le fond (#242424 en sombre, #D2D2D3 en clair). La couleur de barre reste définie pour les états transitoires.
pub(crate) unsafe fn appliquer_dwm(hwnd: HWND, clair: bool) {
    // SAFETY: fil d'interface, `hwnd` est la fenêtre principale vivante. Chaque DwmSetWindowAttribute lit exactement 4 octets
    // (i32 ou COLORREF u32) sur une variable locale qui vit pendant l'appel ; un échec est ignoré (attribut non géré).
    let sombre: i32 = if clair { 0 } else { 1 };
    let _ = DwmSetWindowAttribute(hwnd, DWMWA_USE_IMMERSIVE_DARK_MODE, &sombre as *const _ as *const c_void, 4);
    // COLORREF 0x00BBGGRR. La bordure reprend `--s2` composé sur le fond : blanc 14 % sur noir = #242424 ;
    // en clair, encre 16 % sur #F7F7F8 = #D2D2D3.
    let barre: u32 = if clair { 0x00F0_F0F0 } else { 0x0044_4444 };
    let _ = DwmSetWindowAttribute(hwnd, DWMWA_CAPTION_COLOR, &barre as *const _ as *const c_void, 4);
    let bordure: u32 = if clair { 0x00D2_D2_D3 } else { 0x0024_2424 };
    let _ = DwmSetWindowAttribute(hwnd, DWMWA_BORDER_COLOR, &bordure as *const _ as *const c_void, 4);
}

/// Relit ce qui peut avoir changé hors de l'application : mouvement réduit (Windows, `shared.json`) et thème (réglage
/// local, avis partagé, Windows). Appelée après un réglage, à un message de changement système, à l'activation de la
/// fenêtre et à la fin d'une lecture de dossier ; la relecture du fichier partagé reste limitée à une par seconde.
/// Rien n'est invalidé si le thème effectif n'a pas changé.
///
/// Ne touche jamais à `Fenetre` (pas de `&mut` refait) : la cible Direct2D compare elle-même le thème de sa création à
/// `theme::clair()` au dessin et se recrée (icônes et trame), donc un thème qui change par un autre chemin que cette
/// fonction est rattrapé au dessin suivant.
pub(crate) unsafe fn rafraichir_apparence(hwnd: HWND) {
    // SAFETY: fil d'interface ; seuls des atomiques, le registre, `shared.json` et des appels DWM / uxtheme sur `hwnd` sont
    // touchés, aucune référence vers l'état de la fenêtre n'est prise (l'appelant peut en tenir une).
    reglages::rafraichir_mouvement();
    theme::recalculer();
    if theme::a_appliquer() {
        let clair = theme::clair();
        appliquer_dwm(hwnd, clair);
        coquille::menus_theme(clair);
        let _ = InvalidateRect(hwnd, None, false);
    }
}

/// Demande à la fenêtre de rafraîchir son apparence au tour de boucle suivant (ex. `WM_ACTIVATE`, envoyé de façon synchrone
/// pendant que l'appelant peut tenir l'état : jamais de travail dans le message d'origine)
pub(crate) const WM_APPARENCE: u32 = WM_APP + 32;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn la_palette_prend_toute_la_souris_et_le_tiroir_sa_zone() {
        // Palette ouverte : tout va à la palette, dans le tiroir ou non
        assert_eq!(destinataire_souris(true, false, false), DestSouris::Palette);
        assert_eq!(destinataire_souris(true, true, true), DestSouris::Palette);
        // Tiroir ouvert : la souris est au tiroir sur son rectangle seulement
        assert_eq!(destinataire_souris(false, true, true), DestSouris::Tiroir);
        assert_eq!(destinataire_souris(false, true, false), DestSouris::Explorateur);
        // Tiroir fermé : un point « dans » son ancien rectangle n'y change rien
        assert_eq!(destinataire_souris(false, false, true), DestSouris::Explorateur);
        assert_eq!(destinataire_souris(false, false, false), DestSouris::Explorateur);
    }

    #[test]
    fn curseur_main_sur_lignes_et_puces_de_la_palette_seulement() {
        use crate::rendu::CiblePalette::*;
        assert!(main_sur_palette(Ligne(3)));
        assert!(main_sur_palette(Puce(0)));
        assert!(!main_sur_palette(Carte));
        assert!(!main_sur_palette(Dehors));
    }

    #[test]
    fn marge_droite_et_bas_de_liste_sont_du_fond_cliquable() {
        let (l, h) = (1200.0f32, 800.0f32);
        let milieu = LISTE_Y + 100.0;
        // Les lignes s'arrêtent avant la marge droite, qui reste dans la zone des fichiers (menu du fond, lasso, dépôt)
        let bord = bord_droit_ligne(l);
        assert!(dans_ligne(l, bord - 1.0));
        assert!(!dans_ligne(l, bord + 1.0));
        assert!(zone_fichiers(l, h, bord + 1.0, milieu));
        assert!(zone_fichiers(l, h, l - 2.0, milieu));
        // La marge est d'au moins 40 DIP entre les lignes et le bord (la maquette : 16)
        assert!(l - bord >= MARGE_LISTE_DROITE - 6.0 - 0.01);
        // Hors de la zone : sous l'en-tête, au-dessus de la barre d'état, volet à gauche, aperçu à droite
        assert!(!zone_fichiers(l, h, 100.0, milieu));
        assert!(!zone_fichiers(l, h, l + 1.0, milieu));
        assert!(!zone_fichiers(l, h, 600.0, LISTE_Y - 1.0));
        assert!(!zone_fichiers(l, h, 600.0, h - STATUT_H));
    }

    #[test]
    fn ctrl_k_et_ctrl_maj_p_basculent_la_palette() {
        assert!(bascule_palette(VK_K, true, false, false));
        assert!(bascule_palette(VK_P, true, false, true));
        // Ni la lettre seule, ni Ctrl+Maj+K, ni Alt+K, ni Ctrl+P (impression) ni Alt+Espace (lanceur de SpaceNotch)
        assert!(!bascule_palette(VK_K, false, false, false));
        assert!(!bascule_palette(VK_K, true, false, true));
        assert!(!bascule_palette(VK_K, true, true, false));
        assert!(!bascule_palette(VK_P, true, false, false));
        assert!(!bascule_palette(VK_SPACE, false, true, false));
    }

    #[test]
    fn ctrl_k_ne_sert_a_aucune_autre_commande_de_la_table() {
        assert_eq!(combinaison(VK_K, true, false, false).as_deref(), Some("CTRL+K"));
        assert!(palette::COMMANDES.iter().all(|c| !palette::TOUCHES_PALETTE.contains(&c.2)), "une commande de la table prend la touche de la palette");
    }

    #[test]
    fn f5_copie_en_double_panneau_et_actualise_en_simple() {
        // Double panneau : F5 copie vers l'autre panneau
        assert_eq!(action_f5(true, false, false, false), ActionF5::Copier);
        // Simple panneau : F5 relit le dossier
        assert_eq!(action_f5(false, false, false, false), ActionF5::Actualiser);
        // Maj+F5 copie toujours (et dit d'ouvrir le double panneau s'il manque)
        assert_eq!(action_f5(false, true, false, false), ActionF5::Copier);
        assert_eq!(action_f5(true, true, false, false), ActionF5::Copier);
        // Ctrl+F5 et Alt+F5 gardent leur sens d'avant
        assert_eq!(action_f5(true, false, true, false), ActionF5::Actualiser);
        assert_eq!(action_f5(true, false, false, true), ActionF5::Actualiser);
    }

    #[test]
    fn f5_est_dans_la_table_des_raccourcis_pour_les_deux_modes() {
        let noms: Vec<&str> = palette::COMMANDES.iter().filter(|c| c.2 == "F5").map(|c| c.1).collect();
        assert!(noms.contains(&"ACTUALISER"), "{noms:?}");
        assert!(noms.iter().any(|n| n.starts_with("COPIER VERS L'AUTRE PANNEAU")), "{noms:?}");
        assert!(palette::COMMANDES.iter().any(|c| c.0 == palette::Commande::CopierVersAutre), "F5 en double panneau reste une commande de la table");
    }
}
