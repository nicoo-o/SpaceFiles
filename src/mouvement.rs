// Mouvement de la maquette v6 (lots 12A à 12C) : états de l'ouverture et de la fermeture de la fenêtre, glissements (pastille de
// sélection, indicateur de vue, liseré de l'onglet actif), entrées de la liste (FLIP, cascade d'ouverture), fondus de survol,
// reflet, interrupteurs, page des Réglages. Tout est pur et prend l'heure en paramètre (millisecondes depuis le lancement) :
// aucune minuterie ici. La fenêtre arme la minuterie d'animation existante tant qu'un mouvement est en cours et la détruit à la
// fin ; le mouvement réduit donne l'état final tout de suite.

use crate::app::Cible;
use crate::dossier::{Emplacement, Entree};
use crate::pastilles::{Disposition, Pastille, Place};
use crate::pixel::Teinte;
use crate::reflet::Plan;
use crate::theme::{ease, ease_in, ease_in_css, ease_palette_fermeture, ease_standard, ressort};
use std::collections::{HashMap, HashSet};
use std::sync::Mutex;
use windows::Win32::Graphics::Direct2D::Common::D2D_RECT_F;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::OnceLock;
use std::time::Instant;

/// Durées de la maquette : `openWin` 560 ms, fermeture plus courte, `.pill` 300 ms, `.views .vi` 450 ms, liseré d'onglet 300 ms
pub const OUVERTURE_MS: u64 = 560;
pub const FERMETURE_MS: u64 = 240;
pub const PASTILLE_MS: u64 = 300;
pub const VUE_MS: u64 = 450;
pub const ONGLET_MS: u64 = 300;

/// `openWin` : translateY 48 px, échelle .88, rayon 36 px au départ ; 20 px à l'arrivée
const DECALAGE_DEPART: f32 = 48.0;
const ECHELLE_DEPART: f32 = 0.88;
const RAYON_DEPART: f32 = 36.0;
const RAYON_ARRIVEE: f32 = 20.0;

/// Heure en millisecondes depuis le premier appel (horloge monotone) ; l'origine ne compte pas, seuls les écarts servent
pub fn maintenant_ms() -> u64 {
    origine().elapsed().as_millis() as u64
}

fn origine() -> &'static Instant {
    static ORIGINE: OnceLock<Instant> = OnceLock::new();
    ORIGINE.get_or_init(Instant::now)
}

/// Mesure de l'ouverture (mode essai seulement : `SPACEFILES_SANS_REGISTRE=1` et `SPACEFILES_MESURE_MOUVEMENT=1`) : la durée
/// réelle, du premier au dernier dessin, et le nombre d'images vont au journal. Lue une fois.
pub fn mesure_demandee() -> bool {
    static VALEUR: OnceLock<bool> = OnceLock::new();
    *VALEUR.get_or_init(|| crate::defaut::sans_registre() && std::env::var("SPACEFILES_MESURE_MOUVEMENT").is_ok_and(|v| v.trim() == "1"))
}

/// Où en est la fenêtre : décalage vers le bas (DIP), échelle (origine bas-centre), opacité du contenu, rayon des coins (DIP)
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EtatFenetre {
    pub decalage: f32,
    pub echelle: f32,
    pub opacite: f32,
    pub rayon: f32,
}

impl EtatFenetre {
    /// État de repos : rien ne bouge
    pub const NORMAL: EtatFenetre = EtatFenetre { decalage: 0.0, echelle: 1.0, opacite: 1.0, rayon: RAYON_ARRIVEE };

    /// `p` = 0 : fenêtre cachée (départ de l'ouverture, arrivée de la fermeture) ; 1 : état normal. Dépasse 1 avec le ressort.
    fn depuis(p: f32) -> EtatFenetre {
        EtatFenetre {
            decalage: DECALAGE_DEPART * (1.0 - p),
            echelle: ECHELLE_DEPART + (1.0 - ECHELLE_DEPART) * p,
            opacite: p.clamp(0.0, 1.0),
            rayon: RAYON_DEPART + (RAYON_ARRIVEE - RAYON_DEPART) * p,
        }
    }
}

/// Ouverture, `ecoule` ms après son début : ressort de la maquette, avec dépassement
pub fn etat_ouverture(ecoule: u64) -> EtatFenetre {
    if ecoule >= OUVERTURE_MS {
        return EtatFenetre::NORMAL;
    }
    EtatFenetre::depuis(ressort(ecoule as f32 / OUVERTURE_MS as f32))
}

/// Fermeture, `ecoule` ms après son début : sens inverse, sans dépassement ; `replay` de la maquette, 240 ms en
/// `cubic-bezier(.4, 0, 1, 1)` (part doucement, finit vite)
pub fn etat_fermeture(ecoule: u64) -> EtatFenetre {
    let x = (ecoule as f32 / FERMETURE_MS as f32).clamp(0.0, 1.0);
    EtatFenetre::depuis(1.0 - ease_in(x))
}

/// État à dessiner : `None` = fenêtre normale. `animer` faux (mouvement réduit) : toujours `None`, ouverture et fermeture
/// instantanées. La fermeture l'emporte sur l'ouverture.
pub fn etat_fenetre(ouverture: Option<u64>, fermeture: Option<u64>, maintenant: u64, animer: bool) -> Option<EtatFenetre> {
    if !animer {
        return None;
    }
    if let Some(debut) = fermeture {
        return Some(etat_fermeture(maintenant.saturating_sub(debut)));
    }
    let ecoule = maintenant.saturating_sub(ouverture?);
    (ecoule < OUVERTURE_MS).then(|| etat_ouverture(ecoule))
}

/// La fermeture a fini de se jouer : la fenêtre peut se détruire
pub fn fermeture_finie(debut: u64, maintenant: u64) -> bool {
    maintenant.saturating_sub(debut) >= FERMETURE_MS
}

/// Valeur qui glisse d'une cible à l'autre avec le ressort de la maquette. Une nouvelle cible repart de la valeur du moment
/// (pas de saut si on change d'avis en route) ; la première cible, un saut demandé ou le mouvement réduit posent la valeur d'un coup.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Glisse {
    de: f32,
    vers: f32,
    debut: u64,
    duree: u64,
    pose: bool,
    courbe: Courbe,
}

/// Courbe d'un mouvement : le ressort de la maquette (`--spring`) ou `ease` (la courbe d'une `transition` sans courbe)
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Courbe {
    Ressort,
    Ease,
}

impl Courbe {
    pub fn appliquer(self, x: f32) -> f32 {
        match self {
            Courbe::Ressort => ressort(x),
            Courbe::Ease => ease(x),
        }
    }
}

impl Glisse {
    pub const fn nouveau() -> Glisse {
        Glisse::avec(Courbe::Ressort)
    }

    pub const fn avec(courbe: Courbe) -> Glisse {
        Glisse { de: 0.0, vers: 0.0, debut: 0, duree: 0, pose: false, courbe }
    }

    /// Valeur à l'heure `t` : la cible une fois la durée écoulée
    pub fn valeur(&self, t: u64) -> f32 {
        if !self.pose || self.duree == 0 || t >= self.debut + self.duree {
            return self.vers;
        }
        let x = t.saturating_sub(self.debut) as f32 / self.duree as f32;
        self.de + (self.vers - self.de) * self.courbe.appliquer(x)
    }

    /// La cible visée (la valeur finale du glissement)
    pub fn cible(&self) -> f32 {
        self.vers
    }

    /// Le glissement n'a pas fini de se jouer
    pub fn en_cours(&self, t: u64) -> bool {
        self.pose && t < self.debut + self.duree
    }

    /// Vise `cible`. `sauter` : poser la valeur sans glisser (première pose, ligne précédente hors vue, mouvement réduit).
    pub fn viser(&mut self, cible: f32, t: u64, duree: u64, sauter: bool) {
        let courbe = self.courbe;
        if !self.pose || sauter || duree == 0 {
            *self = Glisse { de: cible, vers: cible, debut: t, duree: 0, pose: true, courbe };
        } else if (cible - self.vers).abs() > 1e-4 {
            *self = Glisse { de: self.valeur(t), vers: cible, debut: t, duree, pose: true, courbe };
        }
    }

    /// Oublie la cible : la prochaine sera posée sans glisser
    pub fn oublier(&mut self) {
        *self = Glisse::avec(self.courbe);
    }
}

/// Une ligne de la liste est-elle (au moins en partie) dans la zone visible ? `y` = haut de la ligne dans le contenu.
pub fn ligne_visible(y: f32, hauteur_ligne: f32, defilement: f32, hauteur_liste: f32) -> bool {
    y + hauteur_ligne > defilement && y < defilement + hauteur_liste
}

/// La pastille de sélection saute-t-elle au lieu de glisser ? Oui en mouvement réduit, ou si la ligne précédente ou la nouvelle
/// n'est pas dans la zone visible (défilement, changement de dossier) : elle ne court pas derrière un défilement.
pub fn pastille_saute(precedente_y: f32, cible_y: f32, hauteur_ligne: f32, defilement: f32, hauteur_liste: f32, animer: bool) -> bool {
    !animer || !ligne_visible(precedente_y, hauteur_ligne, defilement, hauteur_liste) || !ligne_visible(cible_y, hauteur_ligne, defilement, hauteur_liste)
}

// ── Lot 12B : les entrées glissent quand l'ordre ou la disposition change (FLIP de la maquette, `flip`) ──

/// `flip` de la maquette : 440 ms ressort pour une entrée déjà présente, 340 ms pour une entrée nouvelle (opacité 0 → 1, échelle .9 → 1)
pub const FLIP_MS: u64 = 440;
pub const FLIP_NOUVEAU_MS: u64 = 340;
/// Au-delà, mise à jour directe : le coût d'un glissement ne doit pas dépendre de la taille du dossier
pub const FLIP_MAX: usize = 400;
/// Un dernier dessin plus ancien que ça (fenêtre cachée, résident) ne sert plus de point de départ
const FLIP_PEREMPTION_MS: u64 = 500;
const FLIP_ECHELLE_DEPART: f32 = 0.9;

/// Compteur des changements d'ordre ou de disposition demandés (tri, filtre, vue, regroupement) : chaque panneau compare ce qu'il
/// a vu à la dernière valeur. Global parce que l'état de l'application n'est pas visible du rendu autrement que par `&App`.
static FLIP_GENERATION: AtomicU32 = AtomicU32::new(0);

/// À appeler au moment où l'ordre ou la disposition des entrées change : le prochain dessin fera glisser les entrées
pub fn demander_flip() {
    FLIP_GENERATION.fetch_add(1, Ordering::Relaxed);
}

pub fn generation_flip() -> u32 {
    FLIP_GENERATION.load(Ordering::Relaxed)
}

/// Écart du dessin d'une entrée par rapport à sa place finale : translation (DIP), échelle autour de son centre, opacité
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Decalage {
    pub dx: f32,
    pub dy: f32,
    pub echelle: f32,
    pub opacite: f32,
}

impl Decalage {
    pub const AUCUN: Decalage = Decalage { dx: 0.0, dy: 0.0, echelle: 1.0, opacite: 1.0 };

    pub fn est_aucun(&self) -> bool {
        self.dx.abs() < 0.01 && self.dy.abs() < 0.01 && (self.echelle - 1.0).abs() < 1e-3 && (self.opacite - 1.0).abs() < 1e-3
    }
}

#[derive(Clone, Copy, Debug)]
struct Deplacement {
    /// Écart de départ : ancienne position affichée − nouvelle position
    dx: f32,
    dy: f32,
    debut: u64,
    nouveau: bool,
}

impl Deplacement {
    fn duree(&self) -> u64 {
        if self.nouveau { FLIP_NOUVEAU_MS } else { FLIP_MS }
    }

    fn decalage(&self, t: u64) -> Decalage {
        if t >= self.debut + self.duree() {
            return Decalage::AUCUN;
        }
        let r = ressort(t.saturating_sub(self.debut) as f32 / self.duree() as f32);
        if self.nouveau {
            Decalage { dx: 0.0, dy: 0.0, echelle: FLIP_ECHELLE_DEPART + (1.0 - FLIP_ECHELLE_DEPART) * r, opacite: r.clamp(0.0, 1.0) }
        } else {
            Decalage { dx: self.dx * (1.0 - r), dy: self.dy * (1.0 - r), echelle: 1.0, opacite: 1.0 }
        }
    }
}

/// Les glissements d'entrées d'un panneau. Chaque dessin donne les places finales des entrées visibles (clé, position écran) ; si un
/// changement a été demandé depuis le dernier dessin, l'écart avec les places **affichées** au dessin précédent (décalage en cours
/// compris : pas de saut si un changement arrive en route) démarre un glissement. Les structures ne contiennent que les entrées
/// visibles (plafond `FLIP_MAX`) et se vident quand les durées sont écoulées.
pub struct Flip {
    /// Places affichées au dernier dessin
    derniere: HashMap<String, (f32, f32)>,
    en_cours: HashMap<String, Deplacement>,
    emplacement: Option<Emplacement>,
    forme: u32,
    generation: u32,
    t_derniere: u64,
    fin: u64,
    /// (premier dessin avec décalage, dernier, nombre d'images) de l'animation qui tourne
    suivi: Option<(u64, u64, u32)>,
    /// Mesure de la dernière animation finie : durée du premier au dernier dessin avec décalage (ms) et nombre d'images
    pub mesure: Option<(u64, u32)>,
    /// Lot 12D : une entrée vient de disparaître, les suivantes doivent glisser au prochain dessin (comme un changement d'ordre)
    force: bool,
}

impl Flip {
    pub fn nouveau() -> Flip {
        Flip { derniere: HashMap::new(), en_cours: HashMap::new(), emplacement: None, forme: 0, generation: 0, t_derniere: 0, fin: 0, suivi: None, mesure: None, force: false }
    }

    /// Place affichée de l'entrée `cle` au dernier dessin (décalage en cours compris)
    pub fn place_affichee(&self, cle: &str) -> Option<(f32, f32)> {
        self.derniere.get(cle).copied()
    }

    /// Le dernier dessin montrait ce dossier (sinon ses places ne disent rien de celui-ci)
    pub fn meme_dossier(&self, emplacement: &Emplacement) -> bool {
        self.emplacement.as_ref() == Some(emplacement)
    }

    /// Une entrée vient de disparaître : au prochain dessin les suivantes glissent vers leur nouvelle place
    pub fn forcer_glissement(&mut self) {
        self.force = true;
    }

    /// Un glissement n'a pas fini : la minuterie d'animation doit tourner
    pub fn en_cours(&self, t: u64) -> bool {
        !self.en_cours.is_empty() && t < self.fin
    }

    /// Nombre d'entrées en mouvement (tests : vérifie le vidage)
    #[cfg(test)]
    pub fn nombre(&self) -> usize {
        self.en_cours.len()
    }

    /// Un dessin : `entrees` = (clé, place finale) des entrées visibles. `forme` = hauteur de ligne en DIP (bits) : un changement
    /// de densité dans le même dossier est un changement de disposition. `generation` = `generation_flip()`. Rend, dans l'ordre de
    /// `entrees`, le décalage à appliquer à chacune. Un autre dossier, le mouvement réduit ou plus de `FLIP_MAX` entrées : mises à
    /// jour directes, aucun décalage.
    pub fn image(&mut self, emplacement: &Emplacement, forme: u32, generation: u32, entrees: &[(String, (f32, f32))], t: u64, animer: bool) -> Vec<Decalage> {
        let mut demande = generation != self.generation || std::mem::take(&mut self.force);
        self.generation = generation;
        let meme_dossier = self.emplacement.as_ref() == Some(emplacement);

        if meme_dossier && self.forme != forme {
            demande = true;
        }
        if !meme_dossier {
            self.emplacement = Some(emplacement.clone());
        }
        self.forme = forme;
        let perime = t.saturating_sub(self.t_derniere) > FLIP_PEREMPTION_MS;
        self.t_derniere = t;

        if !animer || entrees.len() > FLIP_MAX || !meme_dossier {
            self.en_cours.clear();
            self.fin = 0;
            self.suivi = None;
            self.derniere.clear();
            self.derniere.extend(entrees.iter().map(|(k, p)| (k.clone(), *p)));
            return vec![Decalage::AUCUN; entrees.len()];
        }

        if demande && !perime && !self.derniere.is_empty() {
            let mut neuf = HashMap::new();
            for (k, p) in entrees {
                match self.derniere.get(k) {
                    Some(&(ox, oy)) => {
                        let (dx, dy) = (ox - p.0, oy - p.1);
                        if dx.abs() >= 0.5 || dy.abs() >= 0.5 {
                            neuf.insert(k.clone(), Deplacement { dx, dy, debut: t, nouveau: false });
                        }
                    }
                    None => {
                        neuf.insert(k.clone(), Deplacement { dx: 0.0, dy: 0.0, debut: t, nouveau: true });
                    }
                }
            }
            self.fin = neuf.values().map(|m| m.debut + m.duree()).max().unwrap_or(0);
            self.suivi = (!neuf.is_empty()).then_some((t, t, 0));
            self.en_cours = neuf;
        }

        let mut sortie = Vec::with_capacity(entrees.len());
        let mut prochaine = HashMap::with_capacity(entrees.len());
        for (k, p) in entrees {
            let d = self.en_cours.get(k).map_or(Decalage::AUCUN, |m| m.decalage(t));
            prochaine.insert(k.clone(), (p.0 + d.dx, p.1 + d.dy));
            sortie.push(d);
        }
        self.derniere = prochaine;
        if let Some(s) = self.suivi.as_mut() {
            if sortie.iter().any(|d| !d.est_aucun()) {
                s.1 = t;
                s.2 += 1;
            }
        }
        // Vidage : ce qui a fini de se jouer ne reste pas en mémoire
        self.en_cours.retain(|_, m| t < m.debut + m.duree());
        if self.en_cours.is_empty() {
            self.fin = 0;
            if let Some((premier, dernier, images)) = self.suivi.take() {
                self.mesure = Some((dernier - premier, images));
            }
        }
        sortie
    }
}

// ── Lot 12C : finition du mouvement, comme les `transition` et `animate()` de la maquette ──

/// `.row` : fond de survol 120 ms. `.chrome` : fond et soulèvement 160 ms, 60 ms à l'appui (`:active`)
pub const SURVOL_LIGNE_MS: u64 = 120;
pub const SURVOL_CHROME_MS: u64 = 160;
pub const APPUI_MS: u64 = 60;
/// `.rv:after` : le reflet apparaît et disparaît en 180 ms
pub const REFLET_MS: u64 = 180;
/// `.cb` : opacité 150 ms, échelle .6 → 1 en 300 ms au ressort
pub const CASE_FONDU_MS: u64 = 150;
pub const CASE_ECHELLE_MS: u64 = 300;
const CASE_ECHELLE_DEPART: f32 = 0.6;
/// `[data-tip]` : l'infobulle paraît après 500 ms de survol, 240 ms au ressort, depuis 5 px plus haut
pub const BULLE_DELAI_MS: u64 = 500;
pub const BULLE_MS: u64 = 240;
const BULLE_DECALAGE: f32 = -5.0;
/// Entrée des lignes à l'ouverture (`dissolve` : 460 ms, 26 ms par ligne sur 16 lignes, 28 px). Le décalage est plafonné à
/// 140 ms pour que la dernière ligne ait fini avant 600 ms.
pub const CASCADE_PAS_MS: u64 = 26;
pub const CASCADE_DELAI_MAX_MS: u64 = 140;
pub const CASCADE_MS: u64 = 460;
pub const CASCADE_TOTAL_MS: u64 = CASCADE_DELAI_MAX_MS + CASCADE_MS;
pub const CASCADE_LIGNES: usize = 16;
const CASCADE_DECALAGE: f32 = 28.0;
/// `.sw` : fond 200 ms, bille 300 ms au ressort ; rail des Réglages : barre d'accent 200 ms
pub const INTERRUPTEUR_FOND_MS: u64 = 200;
pub const INTERRUPTEUR_BILLE_MS: u64 = 300;
pub const RAIL_MS: u64 = 200;
/// `openSet` 380 ms au ressort (échelle .97 → 1), `closeSet` 160 ms linéaire (échelle 1 → .98)
pub const PAGE_OUVERTURE_MS: u64 = 380;
pub const PAGE_FERMETURE_MS: u64 = 160;
/// Second panneau (F6) : translateX 30 px et fondu, 420 ms au ressort
pub const SECOND_PANNEAU_MS: u64 = 420;
pub const SECOND_PANNEAU_DECALAGE: f32 = 30.0;
/// Éléments qui apparaissent en glissant (menu contextuel 260 ms, renommage par lot et formulaire 420 ms, palette 480 ms) :
/// leur numéro dans `Mouvements::apparus`
pub const APPARU_MENU: usize = 0;
pub const APPARU_LOT: usize = 1;
pub const APPARU_FORMULAIRE: usize = 2;
pub const APPARU_PALETTE: usize = 3;
pub const APPARU_MENU_MS: u64 = 260;
pub const APPARU_CARTE_MS: u64 = 420;
pub const APPARU_PALETTE_MS: u64 = 480;

/// Durée du fond de survol d'un élément : 160 ms pour le `.chrome`, 120 ms pour le reste
pub fn duree_survol(cible: Cible) -> u64 {
    if cible.est_chrome() { SURVOL_CHROME_MS } else { SURVOL_LIGNE_MS }
}

/// Entrée de la ligne `rang` (0 = première visible) `ecoule` ms après l'ouverture : invisible pendant son délai, puis fondu et
/// translation de 28 DIP au ressort sur 460 ms. Au-delà de 16 lignes, aucune animation.
pub fn decalage_cascade(rang: usize, ecoule: u64) -> Decalage {
    if rang >= CASCADE_LIGNES {
        return Decalage::AUCUN;
    }
    let delai = (rang as u64 * CASCADE_PAS_MS).min(CASCADE_DELAI_MAX_MS);
    if ecoule >= delai + CASCADE_MS {
        return Decalage::AUCUN;
    }
    let p = if ecoule < delai { 0.0 } else { ressort((ecoule - delai) as f32 / CASCADE_MS as f32) };
    Decalage { dx: CASCADE_DECALAGE * (1.0 - p), dy: 0.0, echelle: 1.0, opacite: p.clamp(0.0, 1.0) }
}

/// Deux décalages appliqués l'un après l'autre (cascade d'ouverture et glissement d'un tri)
pub fn combiner(a: Decalage, b: Decalage) -> Decalage {
    Decalage { dx: a.dx + b.dx, dy: a.dy + b.dy, echelle: a.echelle * b.echelle, opacite: a.opacite * b.opacite }
}

/// Mélange de deux valeurs : `k` = 0 donne `a`, 1 donne `b` (fond et bille d'un interrupteur)
pub fn melanger(a: f32, b: f32, k: f32) -> f32 {
    a + (b - a) * k
}

#[derive(Clone, Copy, Debug)]
struct EntreeSurvol {
    cible: Cible,
    /// Fond de survol : 0 → 1, `ease`
    fond: Glisse,
    /// Soulèvement du chrome : 0 → 1 (fois −2 DIP), ressort
    soulevement: Glisse,
}

/// Fondus de survol (`.row`, `.chrome`) : l'élément survolé et ceux qui viennent de l'être, chacun avec son fond (`ease`) et son
/// soulèvement (ressort). Mis à jour une fois par image avec la cible survolée ; une entrée finie et à zéro est retirée.
pub struct Survols {
    courant: Cible,
    depuis: u64,
    appui: bool,
    animer: bool,
    entrees: Vec<EntreeSurvol>,
}

impl Survols {
    pub fn nouveau() -> Survols {
        Survols { courant: Cible::Rien, depuis: 0, appui: false, animer: true, entrees: Vec::new() }
    }

    /// Une image : `cible` est ce qui est survolé, `appui` le bouton de la souris est enfoncé. Mouvement réduit (`animer` faux) :
    /// aucune entrée, l'état est celui de la cible courante tout de suite.
    pub fn maj(&mut self, cible: Cible, appui: bool, t: u64, animer: bool) {
        if cible != self.courant {
            self.courant = cible;
            self.depuis = t;
        }
        self.appui = appui;
        self.animer = animer;
        if !animer {
            self.entrees.clear();
            return;
        }
        for e in self.entrees.iter_mut() {
            let ici = e.cible == cible;
            let duree = if ici && appui { APPUI_MS } else { duree_survol(e.cible) };
            e.fond.viser(f32::from(ici), t, duree, false);
            e.soulevement.viser(f32::from(ici && e.cible.est_chrome() && !appui), t, duree, false);
        }
        if cible != Cible::Rien && !self.entrees.iter().any(|e| e.cible == cible) {
            let duree = duree_survol(cible);
            let mut e = EntreeSurvol { cible, fond: Glisse::avec(Courbe::Ease), soulevement: Glisse::nouveau() };
            e.fond.viser(0.0, t, 0, true);
            e.soulevement.viser(0.0, t, 0, true);
            e.fond.viser(1.0, t, duree, false);
            if cible.est_chrome() && !appui {
                e.soulevement.viser(1.0, t, duree, false);
            }
            self.entrees.push(e);
        }
        self.entrees.retain(|e| e.fond.en_cours(t) || e.soulevement.en_cours(t) || e.fond.valeur(t).abs() > 1e-3 || e.soulevement.valeur(t).abs() > 1e-3);
    }

    /// Intensité du fond de survol de `cible` (0 à 1)
    pub fn fond(&self, cible: Cible, t: u64) -> f32 {
        if !self.animer {
            return f32::from(cible == self.courant && cible != Cible::Rien);
        }
        self.entrees.iter().find(|e| e.cible == cible).map_or(0.0, |e| e.fond.valeur(t).clamp(0.0, 1.0))
    }

    /// Décalage vertical (DIP, 0 ou négatif) du chrome `cible` ; le ressort dépasse un peu avant de se poser
    pub fn soulevement(&self, cible: Cible, t: u64) -> f32 {
        if !self.animer {
            return if cible == self.courant && cible.est_chrome() && !self.appui { crate::theme::SURVOL_DECALAGE } else { 0.0 };
        }
        self.entrees.iter().find(|e| e.cible == cible).map_or(0.0, |e| crate::theme::SURVOL_DECALAGE * e.soulevement.valeur(t))
    }

    /// Tous les soulèvements non nuls, pour le dessin
    pub fn soulevements(&self, t: u64) -> Vec<(Cible, f32)> {
        let source: Vec<Cible> = if self.animer { self.entrees.iter().map(|e| e.cible).collect() } else { vec![self.courant] };
        source.into_iter().map(|c| (c, self.soulevement(c, t))).filter(|(_, d)| d.abs() > 1e-3).collect()
    }

    pub fn en_cours(&self, t: u64) -> bool {
        self.entrees.iter().any(|e| e.fond.en_cours(t) || e.soulevement.en_cours(t)) || (self.animer && self.bulle_possible() && t < self.depuis + BULLE_DELAI_MS + BULLE_MS)
    }

    fn bulle_possible(&self) -> bool {
        matches!(self.courant, Cible::Outil(_))
    }

    /// Infobulle de la barre : (opacité, décalage vertical en DIP), ou `None` tant que le délai de 500 ms n'est pas écoulé.
    /// Mouvement réduit : tout de suite, sans mouvement (`.rm [data-tip]:before { animation: none }`).
    pub fn bulle(&self, t: u64) -> Option<(f32, f32)> {
        if !self.bulle_possible() {
            return None;
        }
        if !self.animer {
            return Some((1.0, 0.0));
        }
        let ecoule = t.saturating_sub(self.depuis);
        if ecoule < BULLE_DELAI_MS {
            return None;
        }
        let p = ressort((ecoule - BULLE_DELAI_MS) as f32 / BULLE_MS as f32);
        Some((p.clamp(0.0, 1.0), BULLE_DECALAGE * (1.0 - p)))
    }
}

/// Reflet qui suit le curseur, avec ses fondus : le reflet courant monte à 1 en 180 ms, ceux qu'on vient de quitter descendent à
/// 0 à leur dernière place (le centre ne bouge plus) avant de disparaître.
pub struct Reflets {
    vus: Vec<(Plan, Option<D2D_RECT_F>, Glisse)>,
}

impl Reflets {
    pub fn nouveau() -> Reflets {
        Reflets { vus: Vec::new() }
    }

    /// Une image : le reflet voulu (plan et rognage), ou `None`. Rend ce qu'il faut dessiner, avec l'opacité de chacun (0 à 1).
    pub fn maj(&mut self, courant: Option<(Plan, Option<D2D_RECT_F>)>, t: u64, animer: bool) -> Vec<(Plan, Option<D2D_RECT_F>, f32)> {
        if !animer {
            self.vus.clear();
            return courant.map(|(p, c)| vec![(p, c, 1.0)]).unwrap_or_default();
        }
        for v in self.vus.iter_mut() {
            let ici = courant.as_ref().is_some_and(|(p, _)| p.zone == v.0.zone);
            if ici {
                if let Some((p, c)) = courant {
                    v.0 = p;
                    v.1 = c;
                }
            }
            v.2.viser(f32::from(ici), t, REFLET_MS, false);
        }
        if let Some((p, c)) = courant {
            if !self.vus.iter().any(|v| v.0.zone == p.zone) {
                let mut g = Glisse::avec(Courbe::Ease);
                g.viser(0.0, t, 0, true);
                g.viser(1.0, t, REFLET_MS, false);
                self.vus.push((p, c, g));
            }
        }
        self.vus.retain(|v| v.2.en_cours(t) || v.2.valeur(t) > 1e-3);
        self.vus.iter().map(|(p, c, g)| (*p, *c, g.valeur(t).clamp(0.0, 1.0))).collect()
    }

    pub fn en_cours(&self, t: u64) -> bool {
        self.vus.iter().any(|v| v.2.en_cours(t))
    }

    #[cfg(test)]
    pub fn nombre(&self) -> usize {
        self.vus.len()
    }
}

#[derive(Clone, Copy, Debug)]
struct EntreeCase {
    echelle: Glisse,
    opacite: Glisse,
    vu: u32,
}

/// Case à cocher d'une ligne (`.cb`) : opacité 150 ms, échelle .6 → 1 en 300 ms au ressort. L'icône (`.ic`) fait l'inverse.
/// Une entrée n'existe que pendant que la case est voulue ou qu'elle s'efface ; une ligne qui n'est plus dessinée l'est retirée.
pub struct Cases {
    entrees: HashMap<u64, EntreeCase>,
}

impl Cases {
    pub fn nouveau() -> Cases {
        Cases { entrees: HashMap::new() }
    }

    /// (opacité de la case, échelle) de la ligne `cle` à cette image. `voulue` : survolée, ou choisie parmi plusieurs.
    pub fn etat(&mut self, cle: u64, voulue: bool, image: u32, t: u64, animer: bool) -> (f32, f32) {
        if !animer {
            self.entrees.clear();
            return if voulue { (1.0, 1.0) } else { (0.0, CASE_ECHELLE_DEPART) };
        }
        let cible = f32::from(voulue);
        let e = match self.entrees.get_mut(&cle) {
            Some(e) => e,
            None if !voulue => return (0.0, CASE_ECHELLE_DEPART),
            None => {
                let mut echelle = Glisse::nouveau();
                let mut opacite = Glisse::avec(Courbe::Ease);
                echelle.viser(0.0, t, 0, true);
                opacite.viser(0.0, t, 0, true);
                self.entrees.entry(cle).or_insert(EntreeCase { echelle, opacite, vu: image })
            }
        };
        e.vu = image;
        e.echelle.viser(cible, t, CASE_ECHELLE_MS, false);
        e.opacite.viser(cible, t, CASE_FONDU_MS, false);
        (e.opacite.valeur(t).clamp(0.0, 1.0), CASE_ECHELLE_DEPART + (1.0 - CASE_ECHELLE_DEPART) * e.echelle.valeur(t))
    }

    /// Fin d'image : retire les lignes plus dessinées et celles revenues à l'état caché
    pub fn fin_image(&mut self, image: u32, t: u64) {
        self.entrees.retain(|_, e| e.vu == image && (e.echelle.en_cours(t) || e.opacite.en_cours(t) || e.echelle.cible() > 0.0));
    }

    pub fn en_cours(&self, t: u64) -> bool {
        self.entrees.values().any(|e| e.echelle.en_cours(t) || e.opacite.en_cours(t))
    }

    #[cfg(test)]
    pub fn nombre(&self) -> usize {
        self.entrees.len()
    }
}

/// Ouverture et fermeture de la page des Réglages (`openSet` / `closeSet`)
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EtatPage {
    pub opacite: f32,
    pub echelle: f32,
}

/// Ouverture, `ecoule` ms après son début : fondu et échelle .97 → 1 au ressort, 380 ms
pub fn etat_page_ouverture(ecoule: u64) -> Option<EtatPage> {
    if ecoule >= PAGE_OUVERTURE_MS {
        return None;
    }
    let p = ressort(ecoule as f32 / PAGE_OUVERTURE_MS as f32);
    Some(EtatPage { opacite: p.clamp(0.0, 1.0), echelle: 0.97 + 0.03 * p })
}

/// Fermeture : opacité 1 → 0 et échelle 1 → .98, 160 ms, linéaire
pub fn etat_page_fermeture(ecoule: u64) -> Option<EtatPage> {
    if ecoule >= PAGE_FERMETURE_MS {
        return None;
    }
    let x = ecoule as f32 / PAGE_FERMETURE_MS as f32;
    Some(EtatPage { opacite: 1.0 - x, echelle: 1.0 - 0.02 * x })
}

/// Où en est la page des Réglages : ouverte ou non au dernier dessin, et depuis quand elle a changé
pub struct PageAnim {
    ouverte: bool,
    debut: Option<u64>,
}

impl PageAnim {
    pub fn nouveau() -> PageAnim {
        PageAnim { ouverte: false, debut: None }
    }

    /// Une image : la page est affichée (ou non). Un changement démarre le mouvement ; mouvement réduit : aucun.
    pub fn maj(&mut self, ouverte: bool, t: u64, animer: bool) {
        if ouverte != self.ouverte {
            self.ouverte = ouverte;
            self.debut = animer.then_some(t);
        }
        if !animer {
            self.debut = None;
        }
    }

    /// La page vient d'être quittée ou se ferme encore : la fenêtre doit fournir ses lignes pour la dessiner pendant 160 ms
    pub fn sortie_voulue(&self, ouverte_maintenant: bool, t: u64) -> bool {
        !ouverte_maintenant && (self.ouverte || self.etat(t).is_some_and(|(ouvrait, _)| !ouvrait))
    }

    /// (page en ouverture ?, état) pendant le mouvement ; `None` au repos
    pub fn etat(&self, t: u64) -> Option<(bool, EtatPage)> {
        let debut = self.debut?;
        let ecoule = t.saturating_sub(debut);
        if self.ouverte {
            etat_page_ouverture(ecoule).map(|e| (true, e))
        } else {
            etat_page_fermeture(ecoule).map(|e| (false, e))
        }
    }

    pub fn en_cours(&self, t: u64) -> bool {
        self.etat(t).is_some()
    }
}

/// Mesure de la régularité des images (mode essai `SPACEFILES_MESURE_MOUVEMENT=1`) : intervalles entre deux dessins d'une même
/// animation, en millisecondes. Une pause de plus de 150 ms termine la série.
pub struct Cadence {
    dernier: Option<u64>,
    intervalles: Vec<f32>,
}

/// Bilan d'une série d'images : nombre d'intervalles, moyenne, écart-type et maximum (ms)
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BilanCadence {
    pub intervalles: usize,
    pub moyenne: f32,
    pub ecart_type: f32,
    pub maximum: f32,
}

const CADENCE_PAUSE_MS: f32 = 150.0;

pub fn bilan_cadence(intervalles: &[f32]) -> Option<BilanCadence> {
    if intervalles.is_empty() {
        return None;
    }
    let n = intervalles.len() as f32;
    let moyenne = intervalles.iter().sum::<f32>() / n;
    let variance = intervalles.iter().map(|x| (x - moyenne).powi(2)).sum::<f32>() / n;
    Some(BilanCadence { intervalles: intervalles.len(), moyenne, ecart_type: variance.sqrt(), maximum: intervalles.iter().cloned().fold(0.0, f32::max) })
}

impl Cadence {
    pub fn nouveau() -> Cadence {
        Cadence { dernier: None, intervalles: Vec::new() }
    }

    /// Un dessin à l'heure `us` (microsecondes). Rend le bilan de la série précédente si une pause l'a terminée.
    pub fn noter(&mut self, us: u64) -> Option<BilanCadence> {
        let mut fini = None;
        if let Some(avant) = self.dernier {
            let ms = us.saturating_sub(avant) as f32 / 1000.0;
            if ms > CADENCE_PAUSE_MS {
                fini = self.vider();
            } else {
                self.intervalles.push(ms);
            }
        }
        self.dernier = Some(us);
        fini
    }

    /// Termine la série en cours (la minuterie d'animation s'arrête)
    pub fn vider(&mut self) -> Option<BilanCadence> {
        self.dernier = None;
        let bilan = bilan_cadence(&self.intervalles);
        self.intervalles.clear();
        bilan
    }
}

/// Écrit le bilan d'une série d'images au journal (mode essai seulement)
pub fn journaliser_cadence(b: &BilanCadence) {
    crate::journal::ecrire(
        crate::journal::Niveau::Info,
        "mouvement",
        &format!("CADENCE {} intervalles : moyenne {:.2} ms, ecart-type {:.2} ms, maximum {:.2} ms", b.intervalles, b.moyenne, b.ecart_type, b.maximum),
    );
}

/// Heure en microsecondes (même origine que `maintenant_ms`), pour la mesure de cadence
pub fn maintenant_us() -> u64 {
    origine().elapsed().as_micros() as u64
}

// ── Lot 12D : sorties « fantômes », cartes d'opération, volets, aperçu qui se déploie ──

/// Une sortie se joue sur un fantôme : l'élément n'existe plus dans la liste, on en garde de quoi le redessiner. Au plus
/// `FANTOMES_MAX` à la fois, chacun jusqu'à la fin de sa durée puis supprimé ; aucune minuterie au repos (`en_cours`).
pub const FANTOMES_MAX: usize = 24;
/// Suppression d'une ligne (l. 565) : opacité 1 → 0 et échelle 1 → .92, 220 ms `ease-in`
pub const SORTIE_LIGNE_MS: u64 = 220;
/// `dropOp` (l. 472) : translateY 20 + échelle .92 + opacité 0, 240 ms `ease-in`
pub const SORTIE_CARTE_MS: u64 = 240;
/// `addOp` (l. 451) : translateY 30 + échelle .9, 460 ms au ressort
pub const CARTE_ENTREE_MS: u64 = 460;
/// `animateOthers` (l. 454) : les cartes voisines glissent en 420 ms au ressort
pub const CARTE_VOISINES_MS: u64 = 420;
const CARTE_ENTREE_DY: f32 = 30.0;
const CARTE_ENTREE_ECHELLE: f32 = 0.9;
/// `.card .more` (l. 120) : largeur 400 ms au ressort, opacité 200 ms
pub const DEPLOIEMENT_MS: u64 = 400;
pub const DEPLOIEMENT_FONDU_MS: u64 = 200;
/// `.dp i` (l. 118) : fond d'une barre de la jauge 200 ms
pub const BARRE_JAUGE_MS: u64 = 200;
/// `.body` (l. 157) : colonnes du volet et de l'aperçu 500 ms au ressort ; `.side, .prev` (l. 158) : opacité 250 ms
pub const VOLET_MS: u64 = 500;
pub const VOLET_FONDU_MS: u64 = 250;
/// `.spill` (lot 14) : largeur 450 ms au ressort, opacité 200 ms
pub const PILULE_MS: u64 = 450;
pub const PILULE_FONDU_MS: u64 = 200;
/// Aperçu flottant et carte de conflit (l. 497, 504, 582) : le cadre part du rectangle d'origine en 460 ms au ressort, le contenu
/// (`.fp .in`, l. 127) paraît ensuite en 180 ms ; à la fermeture le cadre revient en 240 ms en `cubic-bezier(.4, 0, .2, 1)`
pub const MORPH_OUVERTURE_MS: u64 = 460;
pub const MORPH_CONTENU_MS: u64 = 180;
pub const MORPH_FERMETURE_MS: u64 = 240;
/// `.hist` (l. 253) : le tiroir de l'Historique glisse en `translateX` 500 ms au ressort, dans les deux sens
pub const TIROIR_MS: u64 = 500;
/// Fermeture de la palette (l. 645) : révélation inversée par `clip-path` et translateY(-10), 260 ms en `cubic-bezier(.5, 0, .8, .4)`
pub const PALETTE_FERMETURE_MS: u64 = 260;
pub const PALETTE_DECALAGE: f32 = -10.0;
/// Une suppression annoncée mais jamais vue disparaître (refusée, annulée) est oubliée après ce délai
const ATTENTE_SORTIE_MS: u64 = 60_000;
/// Une suppression déjà jouée par un panneau reste disponible un instant pour l'autre panneau (même dossier ouvert deux fois)
const SORTIE_PARTAGEE_MS: u64 = 1000;

/// Comment un fantôme s'efface : durée, translation verticale (DIP) et échelle d'arrivée
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Sortie {
    pub duree: u64,
    pub dy: f32,
    pub echelle: f32,
}

pub const SORTIE_LIGNE: Sortie = Sortie { duree: SORTIE_LIGNE_MS, dy: 0.0, echelle: 0.92 };
pub const SORTIE_CARTE: Sortie = Sortie { duree: SORTIE_CARTE_MS, dy: 20.0, echelle: 0.92 };

/// Un élément en train de sortir : son contenu mémorisé, le rectangle qu'il occupait (x, y, largeur, hauteur) et son départ
#[derive(Clone, Debug)]
pub struct Fantome<T> {
    pub contenu: T,
    pub rect: [f32; 4],
    debut: u64,
    sortie: Sortie,
}

impl<T> Fantome<T> {
    /// Où en est la sortie à l'heure `t` ; `None` une fois finie. Opacité 1 → 0, échelle et translation vers leur valeur d'arrivée,
    /// `ease-in` du CSS.
    pub fn decalage(&self, t: u64) -> Option<Decalage> {
        let ecoule = t.saturating_sub(self.debut);
        if ecoule >= self.sortie.duree {
            return None;
        }
        let k = ease_in_css(ecoule as f32 / self.sortie.duree as f32);
        Some(Decalage { dx: 0.0, dy: self.sortie.dy * k, echelle: 1.0 + (self.sortie.echelle - 1.0) * k, opacite: 1.0 - k })
    }
}

/// La liste bornée des fantômes d'un panneau ou d'une zone
pub struct Fantomes<T> {
    liste: Vec<Fantome<T>>,
}

impl<T> Fantomes<T> {
    pub fn nouveau() -> Fantomes<T> {
        Fantomes { liste: Vec::new() }
    }

    /// Lance une sortie. Refusée (`false`, l'élément disparaît d'un coup) en mouvement réduit ou si la liste est pleine.
    pub fn lancer(&mut self, contenu: T, rect: [f32; 4], t: u64, sortie: Sortie, animer: bool) -> bool {
        if !animer || self.liste.len() >= FANTOMES_MAX {
            return false;
        }
        self.liste.push(Fantome { contenu, rect, debut: t, sortie });
        true
    }

    /// Retire les sorties finies
    pub fn purger(&mut self, t: u64) {
        self.liste.retain(|f| t.saturating_sub(f.debut) < f.sortie.duree);
    }

    pub fn vider(&mut self) {
        self.liste.clear();
    }

    /// Une sortie n'a pas fini : la minuterie d'animation doit tourner
    pub fn en_cours(&self, t: u64) -> bool {
        self.liste.iter().any(|f| t.saturating_sub(f.debut) < f.sortie.duree)
    }

    pub fn iter(&self) -> impl Iterator<Item = &Fantome<T>> {
        self.liste.iter()
    }

    #[cfg(test)]
    pub fn len(&self) -> usize {
        self.liste.len()
    }
}

/// Une ligne qui va sortir : l'entrée telle qu'elle était affichée et si elle était choisie (une ligne supprimée l'est presque toujours)
#[derive(Clone, Debug)]
pub struct LigneSortante {
    pub entree: Entree,
    pub choisie: bool,
    /// Vue où elle sortait (0 liste, 1 grille, 2 colonnes) : seule cette vue la dessine
    pub vue: u8,
}

struct Attendue {
    cle: String,
    ligne: LigneSortante,
    depuis: u64,
    jouee: Option<u64>,
}

/// Suppressions demandées et pas encore vues à l'écran. Global comme `FLIP_GENERATION` : l'état de l'application ne voit pas le rendu.
static ATTENDUES: Mutex<Vec<Attendue>> = Mutex::new(Vec::new());

/// À appeler quand une suppression part : les lignes concernées (clé du glissement, ligne mémorisée). Au-delà de `FANTOMES_MAX`
/// lignes, aucune sortie n'est animée.
pub fn annoncer_sorties(lignes: Vec<(String, LigneSortante)>) {
    if lignes.is_empty() || lignes.len() > FANTOMES_MAX {
        return;
    }
    let t = maintenant_ms();
    if let Ok(mut v) = ATTENDUES.lock() {
        v.extend(lignes.into_iter().map(|(cle, ligne)| Attendue { cle, ligne, depuis: t, jouee: None }));
    }
}

/// Vide la liste des suppressions attendues (tests)
#[cfg(test)]
pub fn oublier_sorties() {
    if let Ok(mut v) = ATTENDUES.lock() {
        v.clear();
    }
}

/// Une carte d'opération telle qu'elle a été dessinée à l'image précédente
#[derive(Clone)]
struct CarteVue {
    place: Place,
    pastille: Pastille,
    /// Où elle a été dessinée (glissement compris)
    affichee: (f32, f32),
}

#[derive(Clone, Copy)]
struct EntreeDeploiement {
    largeur: Glisse,
    opacite: Glisse,
    vu: u32,
}

#[derive(Clone, Copy)]
struct EntreeJauge {
    barres: [Glisse; 16],
    vu: u32,
}

/// Identité d'une carte : son numéro d'opération, sinon (pastille sans numéro) une clé tirée du genre et du libellé
pub fn cle_carte(p: &Pastille) -> u64 {
    if p.cle != 0 {
        return p.cle;
    }
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    format!("{:?}", p.genre).hash(&mut h);
    p.libelle.hash(&mut h);
    h.finish() | (1 << 63)
}

/// Les cartes d'opération (pastilles) : entrée, voisines qui glissent, sortie en fantôme, déploiement du `.more`, jauge
pub struct Cartes {
    vues: HashMap<u64, CarteVue>,
    ordre: Vec<u64>,
    arrivees: HashMap<u64, u64>,
    deplacements: HashMap<u64, (f32, f32, u64)>,
    pub fantomes: Fantomes<(Place, Pastille)>,
    deploiements: HashMap<u64, EntreeDeploiement>,
    jauges: HashMap<u64, EntreeJauge>,
    initialise: bool,
    image: u32,
}

impl Cartes {
    pub fn nouveau() -> Cartes {
        Cartes {
            vues: HashMap::new(),
            ordre: Vec::new(),
            arrivees: HashMap::new(),
            deplacements: HashMap::new(),
            fantomes: Fantomes::nouveau(),
            deploiements: HashMap::new(),
            jauges: HashMap::new(),
            initialise: false,
            image: 0,
        }
    }

    pub fn en_cours(&self, t: u64) -> bool {
        self.arrivees.values().any(|d| t.saturating_sub(*d) < CARTE_ENTREE_MS)
            || self.deplacements.values().any(|(_, _, d)| t.saturating_sub(*d) < CARTE_VOISINES_MS)
            || self.fantomes.en_cours(t)
            || self.deploiements.values().any(|e| e.largeur.en_cours(t) || e.opacite.en_cours(t))
            || self.jauges.values().any(|e| e.barres.iter().any(|g| g.en_cours(t)))
    }

    /// Début d'image : numéro, pour retirer les déploiements et jauges des cartes disparues
    pub fn debut_image(&mut self) {
        self.image = self.image.wrapping_add(1);
    }

    pub fn fin_image(&mut self) {
        let n = self.image;
        self.deploiements.retain(|_, e| e.vu == n);
        self.jauges.retain(|_, e| e.vu == n);
    }

    /// Déploiement de chaque carte visible (`.card .more`) : (largeur 0 à 1 au ressort, opacité 0 à 1). `ouverte` : la carte survolée
    /// ou fixée ; les cartes déployées en permanence (erreur, annulation) le sont d'emblée. Une carte vue pour la première fois est
    /// posée à son état ; mouvement réduit : l'état tout de suite.
    pub fn deploiements(&mut self, pastilles: &[Pastille], ouverte: Option<usize>, t: u64, animer: bool) -> Vec<(f32, f32)> {
        let n = self.image;
        let mut sortie = Vec::with_capacity(pastilles.len());
        for (i, p) in pastilles.iter().enumerate() {
            let cible = f32::from(p.deployee_en_permanence() || ouverte == Some(i));
            if !animer {
                sortie.push((cible, cible));
                continue;
            }
            let e = self.deploiements.entry(cle_carte(p)).or_insert_with(|| {
                let (mut largeur, mut opacite) = (Glisse::nouveau(), Glisse::avec(Courbe::Ease));
                largeur.viser(cible, t, 0, true);
                opacite.viser(cible, t, 0, true);
                EntreeDeploiement { largeur, opacite, vu: n }
            });
            e.vu = n;
            e.largeur.viser(cible, t, DEPLOIEMENT_MS, false);
            e.opacite.viser(cible, t, DEPLOIEMENT_FONDU_MS, false);
            sortie.push((e.largeur.valeur(t).clamp(0.0, 1.0), e.opacite.valeur(t).clamp(0.0, 1.0)));
        }
        if !animer {
            self.deploiements.clear();
        }
        sortie
    }

    /// Opacité (0 à 1) des 16 barres de la jauge de la carte `p` : une barre qui s'allume passe de l'éteint à l'allumé en 200 ms
    pub fn jauge(&mut self, p: &Pastille, pleins: usize, t: u64, animer: bool) -> [f32; 16] {
        let mut sortie = [0.0f32; 16];
        if !animer {
            self.jauges.clear();
            for (k, s) in sortie.iter_mut().enumerate() {
                *s = f32::from(k < pleins);
            }
            return sortie;
        }
        let n = self.image;
        let e = self.jauges.entry(cle_carte(p)).or_insert_with(|| {
            let mut barres = [Glisse::avec(Courbe::Ease); 16];
            for (k, g) in barres.iter_mut().enumerate() {
                g.viser(f32::from(k < pleins), t, 0, true);
            }
            EntreeJauge { barres, vu: n }
        });
        e.vu = n;
        for (k, g) in e.barres.iter_mut().enumerate() {
            g.viser(f32::from(k < pleins), t, BARRE_JAUGE_MS, false);
            sortie[k] = g.valeur(t).clamp(0.0, 1.0);
        }
        sortie
    }

    /// Une image : les places finales des cartes (`d`, dans l'ordre de `pastilles`). Rend le décalage de chacune, dans l'ordre de
    /// `d.places` : entrée d'une carte nouvelle, glissement des voisines quand l'ensemble change. Une carte qui disparaît devient
    /// un fantôme (`fantomes`). La première image n'anime rien (cartes déjà là à l'ouverture) ; mouvement réduit : rien du tout.
    pub fn image(&mut self, d: &Disposition, pastilles: &[Pastille], t: u64, animer: bool) -> Vec<Decalage> {
        let cles: Vec<u64> = d.places.iter().map(|p| pastilles.get(p.index).map_or(0, cle_carte)).collect();
        let n = d.places.len();
        let premiere = !self.initialise;
        self.initialise = true;
        if !animer || premiere {
            self.arrivees.clear();
            self.deplacements.clear();
            self.fantomes.vider();
            self.enregistrer(d, pastilles, &cles, &vec![Decalage::AUCUN; n]);
            return vec![Decalage::AUCUN; n];
        }
        if cles != self.ordre {
            for (cle, v) in &self.vues {
                if !cles.contains(cle) {
                    let z = v.place.zone;
                    self.fantomes.lancer((v.place.clone(), v.pastille.clone()), [v.affichee.0, v.affichee.1, z.l, z.h], t, SORTIE_CARTE, true);
                }
            }
            for (place, cle) in d.places.iter().zip(&cles) {
                match self.vues.get(cle) {
                    None => {
                        self.arrivees.insert(*cle, t);
                    }
                    Some(v) => {
                        let (dx, dy) = (v.affichee.0 - place.zone.x, v.affichee.1 - place.zone.y);
                        if dx.abs() >= 0.5 || dy.abs() >= 0.5 {
                            self.deplacements.insert(*cle, (dx, dy, t));
                        }
                    }
                }
            }
        }
        let decalages: Vec<Decalage> = cles
            .iter()
            .map(|cle| {
                let mut dec = Decalage::AUCUN;
                if let Some(debut) = self.arrivees.get(cle) {
                    let e = t.saturating_sub(*debut);
                    if e < CARTE_ENTREE_MS {
                        let p = ressort(e as f32 / CARTE_ENTREE_MS as f32);
                        dec = combiner(dec, Decalage { dx: 0.0, dy: CARTE_ENTREE_DY * (1.0 - p), echelle: CARTE_ENTREE_ECHELLE + (1.0 - CARTE_ENTREE_ECHELLE) * p, opacite: p.clamp(0.0, 1.0) });
                    }
                }
                if let Some((dx, dy, debut)) = self.deplacements.get(cle) {
                    let e = t.saturating_sub(*debut);
                    if e < CARTE_VOISINES_MS {
                        let r = ressort(e as f32 / CARTE_VOISINES_MS as f32);
                        dec = combiner(dec, Decalage { dx: dx * (1.0 - r), dy: dy * (1.0 - r), echelle: 1.0, opacite: 1.0 });
                    }
                }
                dec
            })
            .collect();
        self.arrivees.retain(|_, d| t.saturating_sub(*d) < CARTE_ENTREE_MS);
        self.deplacements.retain(|_, (_, _, d)| t.saturating_sub(*d) < CARTE_VOISINES_MS);
        self.fantomes.purger(t);
        self.enregistrer(d, pastilles, &cles, &decalages);
        decalages
    }

    fn enregistrer(&mut self, d: &Disposition, pastilles: &[Pastille], cles: &[u64], decalages: &[Decalage]) {
        self.vues.clear();
        for ((place, cle), dec) in d.places.iter().zip(cles).zip(decalages) {
            if let Some(p) = pastilles.get(place.index) {
                self.vues.insert(*cle, CarteVue { place: place.clone(), pastille: p.clone(), affichee: (place.zone.x + dec.dx, place.zone.y + dec.dy) });
            }
        }
        self.ordre = cles.to_vec();
    }
}

/// Un cadre qui se déploie depuis un rectangle d'origine (aperçu flottant l. 497-504, carte de conflit l. 582) et s'y replie.
/// Rectangles en (x, y, largeur, hauteur). Ouverture : le cadre seul va de l'origine à la place finale en 460 ms au ressort pendant
/// que le contenu reste caché, puis le contenu paraît en 180 ms (`.fp.on .in`). Fermeture : le cadre revient à l'origine en 240 ms.
pub struct Morph {
    present: bool,
    debut: Option<u64>,
    ferme: Option<u64>,
    source: [f32; 4],
    cible: [f32; 4],
}

/// Ce que le dessin doit montrer d'un cadre qui se déploie
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EtatMorph {
    /// Cadre en mouvement (le contenu est caché) ; `None` : le cadre est à sa place, le contenu se montre selon `contenu`
    pub cadre: Option<[f32; 4]>,
    /// Opacité du contenu (0 à 1)
    pub contenu: f32,
    /// Opacité du voile derrière (0 à 1)
    pub voile: f32,
    /// Quelque chose est à dessiner (ouvert ou en train de se fermer)
    pub visible: bool,
}

fn melanger_rect(a: [f32; 4], b: [f32; 4], k: f32) -> [f32; 4] {
    [melanger(a[0], b[0], k), melanger(a[1], b[1], k), melanger(a[2], b[2], k).max(0.0), melanger(a[3], b[3], k).max(0.0)]
}

impl Morph {
    pub fn nouveau() -> Morph {
        Morph { present: false, debut: None, ferme: None, source: [0.0; 4], cible: [0.0; 4] }
    }

    /// Une image. `present` : le cadre est affiché. `source` : rectangle d'origine (relu seulement à l'apparition) ; `cible` : sa place.
    /// Mouvement réduit : apparition et disparition instantanées.
    pub fn maj(&mut self, present: bool, source: [f32; 4], cible: [f32; 4], t: u64, animer: bool) -> EtatMorph {
        if present && !self.present {
            self.source = source;
            self.debut = animer.then_some(t);
            self.ferme = None;
        } else if !present && self.present {
            self.ferme = animer.then_some(t);
            self.debut = None;
        }
        self.present = present;
        if present {
            self.cible = cible;
            let Some(debut) = self.debut else { return EtatMorph { cadre: None, contenu: 1.0, voile: 1.0, visible: true } };
            let e = t.saturating_sub(debut);
            if e < MORPH_OUVERTURE_MS {
                let p = ressort(e as f32 / MORPH_OUVERTURE_MS as f32);
                return EtatMorph { cadre: Some(melanger_rect(self.source, cible, p)), contenu: 0.0, voile: p.clamp(0.0, 1.0), visible: true };
            }
            if e >= MORPH_OUVERTURE_MS + MORPH_CONTENU_MS {
                self.debut = None;
                return EtatMorph { cadre: None, contenu: 1.0, voile: 1.0, visible: true };
            }
            return EtatMorph { cadre: None, contenu: ease((e - MORPH_OUVERTURE_MS) as f32 / MORPH_CONTENU_MS as f32), voile: 1.0, visible: true };
        }
        match self.ferme {
            Some(debut) if t.saturating_sub(debut) < MORPH_FERMETURE_MS => {
                let p = ease_standard(t.saturating_sub(debut) as f32 / MORPH_FERMETURE_MS as f32);
                EtatMorph { cadre: Some(melanger_rect(self.cible, self.source, p)), contenu: 0.0, voile: 1.0 - p, visible: true }
            }
            _ => {
                self.ferme = None;
                EtatMorph { cadre: None, contenu: 0.0, voile: 0.0, visible: false }
            }
        }
    }

    pub fn en_cours(&self, t: u64) -> bool {
        self.ferme.is_some_and(|d| t.saturating_sub(d) < MORPH_FERMETURE_MS)
            || (self.present && self.debut.is_some_and(|d| t.saturating_sub(d) < MORPH_OUVERTURE_MS + MORPH_CONTENU_MS))
    }
}

/// Yeux de Pixel (lot 13) : regard vers le pointeur `transform .35s` ressort, hauteur `.25s` (nuit), couleur `.2s`, clignement.
pub const YEUX_REGARD_MS: u64 = 350;
pub const YEUX_HAUTEUR_MS: u64 = 250;
pub const YEUX_COULEUR_MS: u64 = 200;
/// Intervalle minimal entre deux images quand seuls les yeux de Pixel bougent (30 images par seconde)
pub const IMAGE_YEUX_MS: u64 = 33;

/// Faut-il peindre l'image qui vient d'être signalée ? Toujours, sauf quand seuls les yeux bougent et que la dernière image date de
/// moins de `IMAGE_YEUX_MS`. `derniere` : heure de la dernière image peinte (mise à jour ici quand on peint).
pub fn peindre_cette_image(seuls_les_yeux: bool, derniere: &mut u64, t: u64) -> bool {
    if seuls_les_yeux && t.saturating_sub(*derniere) < IMAGE_YEUX_MS {
        return false;
    }
    *derniere = t;
    true
}

/// Ce que le rendu peint des yeux à cette image
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct VueYeux {
    /// Décalage du regard (DIP) : le ressort peut dépasser la cible de quelques dixièmes
    pub dx: f32,
    pub dy: f32,
    /// Fermeture par la nuit : 0 yeux ouverts, 1 `scaleY(.14)`
    pub aplatie: f32,
    /// Fermeture du clignement : 0 ouverts, 1 `scaleY(.1)`
    pub clignement: f32,
    /// Yeux contents de la palette (`.pe.hp`) : 0 normaux, 1 à 5 DIP de haut (`transition: height .25s`)
    pub contents: f32,
    /// Couleur de départ, couleur d'arrivée et avancement du fondu de l'une à l'autre
    pub teinte_avant: Teinte,
    pub teinte: Teinte,
    pub fondu: f32,
}

/// Les glissements des yeux de Pixel. Chaque cible nouvelle repart de la valeur du moment ; mouvement réduit : tout est posé d'un
/// coup (regard centré, clignement coupé, nuit et couleur immédiates). Rien ne tourne ici : l'état est lu à chaque image.
pub struct Yeux {
    regard_x: Glisse,
    regard_y: Glisse,
    nuit: Glisse,
    fondu: Glisse,
    /// Lot 15A : yeux contents de la palette (une commande part)
    contents: Glisse,
    avant: Teinte,
    teinte: Teinte,
    vu: bool,
    /// Début du clignement en cours (heure de l'horloge de `maintenant_ms`) et s'il est celui, court, de la palette (`peb`, 280 ms)
    clignement: Option<(u64, bool)>,
}

impl Yeux {
    pub fn nouveau() -> Yeux {
        Yeux {
            regard_x: Glisse::nouveau(),
            regard_y: Glisse::nouveau(),
            nuit: Glisse::avec(Courbe::Ease),
            fondu: Glisse::avec(Courbe::Ease),
            contents: Glisse::avec(Courbe::Ease),
            avant: Teinte::Cyan,
            teinte: Teinte::Cyan,
            vu: false,
            clignement: None,
        }
    }

    /// Durée du clignement en cours selon son genre
    fn duree_clignement(palette: bool) -> u64 {
        if palette { crate::pixel::CLIGNEMENT_PALETTE_MS } else { crate::pixel::CLIGNEMENT_MS }
    }

    /// Lance un clignement (la minuterie de 9 s sonne). Sans effet pendant un clignement déjà en cours.
    pub fn cligner(&mut self, t: u64) {
        if self.clignement.is_none_or(|(d, p)| t.saturating_sub(d) >= Self::duree_clignement(p)) {
            self.clignement = Some((t, false));
        }
    }

    /// Clignement court de la palette (`palBlink`, 120 ms après une frappe) : repart de zéro même si le précédent n'a pas fini
    /// (`classList.remove('bl')` puis `add`), sauf pendant un long clignement de la barre d'état
    pub fn cligner_palette(&mut self, t: u64) {
        if self.clignement.is_none_or(|(d, p)| p || t.saturating_sub(d) >= crate::pixel::CLIGNEMENT_MS) {
            self.clignement = Some((t, true));
        }
    }

    /// Yeux contents (la palette lance une commande) ou normaux : hauteur en 250 ms, mouvement réduit : tout de suite
    pub fn contente(&mut self, contents: bool, t: u64, animer: bool) {
        self.contents.viser(f32::from(contents), t, YEUX_HAUTEUR_MS, !animer);
    }

    /// État à l'heure `t` pour un regard visé, une nuit voulue ou non et une teinte donnés
    pub fn maj(&mut self, regard: (f32, f32), nuit: bool, teinte: Teinte, t: u64, animer: bool) -> VueYeux {
        self.regard_x.viser(regard.0, t, YEUX_REGARD_MS, !animer);
        self.regard_y.viser(regard.1, t, YEUX_REGARD_MS, !animer);
        self.nuit.viser(f32::from(nuit), t, YEUX_HAUTEUR_MS, !animer);
        if !self.vu {
            self.vu = true;
            self.avant = teinte;
            self.teinte = teinte;
            self.fondu.viser(1.0, t, 0, true);
        } else if teinte != self.teinte {
            self.avant = self.teinte;
            self.teinte = teinte;
            self.fondu = Glisse::avec(Courbe::Ease);
            self.fondu.viser(0.0, t, 0, true);
            self.fondu.viser(1.0, t, YEUX_COULEUR_MS, !animer);
        }
        let clignement = match self.clignement {
            Some((d, palette)) if animer && t.saturating_sub(d) < Self::duree_clignement(palette) => {
                if palette { crate::pixel::phase_clignement_palette(t.saturating_sub(d)) } else { crate::pixel::phase_clignement(t.saturating_sub(d)) }
            }
            _ => {
                self.clignement = None;
                0.0
            }
        };
        VueYeux {
            dx: self.regard_x.valeur(t),
            dy: self.regard_y.valeur(t),
            aplatie: self.nuit.valeur(t).clamp(0.0, 1.0),
            clignement,
            contents: self.contents.valeur(t).clamp(0.0, 1.0),
            teinte_avant: self.avant,
            teinte: self.teinte,
            fondu: self.fondu.valeur(t).clamp(0.0, 1.0),
        }
    }

    /// Un glissement ou un clignement n'a pas fini : la minuterie d'animation doit tourner
    pub fn en_cours(&self, t: u64) -> bool {
        self.regard_x.en_cours(t)
            || self.regard_y.en_cours(t)
            || self.nuit.en_cours(t)
            || self.fondu.en_cours(t)
            || self.contents.en_cours(t)
            || self.clignement.is_some_and(|(d, p)| t.saturating_sub(d) < Self::duree_clignement(p))
    }
}

// ── Lot 15A : palette de commandes (jauge, puces de catégorie, pastille de sélection, puces d'icône, aperçu) ──

/// Traits de la jauge `.pm2` : un trait s'allume (couleur 150 ms, `scaleY(2)` 250 ms au ressort) par demi-lettre tapée
pub const JAUGE_TRAITS: usize = 40;
pub const JAUGE_COULEUR_MS: u64 = 150;
pub const JAUGE_ECHELLE_MS: u64 = 250;
/// Pastille de sélection `.ps` : `top` 300 ms au ressort, `height` 200 ms
pub const PASTILLE_PALETTE_HAUT_MS: u64 = 300;
pub const PASTILLE_PALETTE_HAUTEUR_MS: u64 = 200;
/// Puce d'icône `.pt` et puce de catégorie `.pcat` : bord et couleur en 200 ms
pub const PUCE_MS: u64 = 200;
/// Aperçu `vin` : 350 ms au ressort, décalé de 8 DIP vers le bas au départ
pub const VIN_MS: u64 = 350;
pub const VIN_DECALAGE: f32 = 8.0;

/// Traits allumés de la jauge pour une requête de `lettres` caractères (`Math.min(40, q.length * 2)`)
pub fn jauge_allumes(lettres: usize) -> usize {
    (lettres * 2).min(JAUGE_TRAITS)
}

/// Ce que la palette montre à cette image, pour régler les glissements
pub struct EntreePalette {
    /// Longueur de la requête tapée
    pub lettres: usize,
    /// Rang de la catégorie active
    pub categorie: usize,
    /// Change quand la liste est reconstruite (autre requête ou autre catégorie) : la pastille saute au lieu de glisser (`palSel(pi, true)`)
    pub cle_liste: u64,
    /// Haut et hauteur de la ligne choisie, en coordonnées du contenu de la liste ; `None` : liste vide
    pub pastille: Option<(f32, f32)>,
    /// Rang de l'élément choisi et rangs des éléments affichés (les seuls dont la puce est suivie)
    pub choix: usize,
    pub affiches: std::ops::Range<usize>,
    /// Identité de la commande choisie : un changement rejoue l'apparition de l'aperçu
    pub cle_commande: u64,
}

/// Les glissements de la palette. Rien ne tourne ici : l'état est réglé à chaque image par `maj` et lu par les accesseurs. Mouvement
/// réduit : tout est posé d'un coup, dans son état final.
pub struct AnimPalette {
    jauge_couleur: [Glisse; JAUGE_TRAITS],
    jauge_echelle: [Glisse; JAUGE_TRAITS],
    categories: [Glisse; 5],
    haut: Glisse,
    hauteur: Glisse,
    puces: HashMap<usize, Glisse>,
    vin: Glisse,
    cle_liste: Option<u64>,
    cle_commande: Option<u64>,
    /// Réglée au moins une fois depuis la dernière remise à zéro (évite de reconstruire la structure à chaque image palette fermée)
    utilisee: bool,
}

impl AnimPalette {
    pub fn nouveau() -> AnimPalette {
        AnimPalette {
            jauge_couleur: [Glisse::avec(Courbe::Ease); JAUGE_TRAITS],
            jauge_echelle: [Glisse::nouveau(); JAUGE_TRAITS],
            categories: [Glisse::avec(Courbe::Ease); 5],
            haut: Glisse::nouveau(),
            hauteur: Glisse::avec(Courbe::Ease),
            puces: HashMap::new(),
            vin: Glisse::nouveau(),
            cle_liste: None,
            cle_commande: None,
            utilisee: false,
        }
    }

    /// Règle les cibles de cette image. La première image, une liste reconstruite (pastille) et le mouvement réduit posent sans glisser.
    pub fn maj(&mut self, e: &EntreePalette, t: u64, animer: bool) {
        self.utilisee = true;
        let reconstruite = self.cle_liste != Some(e.cle_liste);
        self.cle_liste = Some(e.cle_liste);
        let saute = !animer || reconstruite;
        let allumes = jauge_allumes(e.lettres);
        for i in 0..JAUGE_TRAITS {
            let on = f32::from(i < allumes);
            self.jauge_couleur[i].viser(on, t, JAUGE_COULEUR_MS, !animer);
            self.jauge_echelle[i].viser(on, t, JAUGE_ECHELLE_MS, !animer);
        }
        for (k, g) in self.categories.iter_mut().enumerate() {
            g.viser(f32::from(k == e.categorie), t, PUCE_MS, !animer);
        }
        if let Some((haut, hauteur)) = e.pastille {
            self.haut.viser(haut, t, PASTILLE_PALETTE_HAUT_MS, saute);
            self.hauteur.viser(hauteur, t, PASTILLE_PALETTE_HAUTEUR_MS, saute);
        }
        // Les puces des lignes qui ne sont plus affichées sont oubliées ; une ligne qui apparaît est posée sans glisser
        self.puces.retain(|k, _| e.affiches.contains(k));
        for i in e.affiches.clone() {
            self.puces.entry(i).or_insert_with(|| Glisse::avec(Courbe::Ease)).viser(f32::from(i == e.choix), t, PUCE_MS, saute);
        }
        if self.cle_commande != Some(e.cle_commande) {
            self.cle_commande = Some(e.cle_commande);
            self.vin = Glisse::nouveau();
            self.vin.viser(0.0, t, 0, true);
            self.vin.viser(1.0, t, VIN_MS, !animer);
        }
    }

    /// Trait `i` de la jauge : (fondu de couleur de 0 à 1, échelle verticale de 1 à 2, un peu plus avec le ressort)
    pub fn jauge(&self, i: usize, t: u64) -> (f32, f32) {
        let i = i.min(JAUGE_TRAITS - 1);
        (self.jauge_couleur[i].valeur(t).clamp(0.0, 1.0), 1.0 + self.jauge_echelle[i].valeur(t).max(0.0))
    }

    /// Part d'accent de la puce de catégorie `k` (0 à 1)
    pub fn categorie(&self, k: usize, t: u64) -> f32 {
        self.categories.get(k).map_or(0.0, |g| g.valeur(t).clamp(0.0, 1.0))
    }

    /// Haut et hauteur affichés de la pastille de sélection (coordonnées du contenu de la liste)
    pub fn pastille(&self, t: u64) -> (f32, f32) {
        (self.haut.valeur(t), self.hauteur.valeur(t).max(0.0))
    }

    /// Part d'accent de la puce d'icône de l'élément `i` (0 à 1) ; 0 si elle n'est pas suivie
    pub fn puce(&self, i: usize, t: u64) -> f32 {
        self.puces.get(&i).map_or(0.0, |g| g.valeur(t).clamp(0.0, 1.0))
    }

    /// Avancement de l'apparition de l'aperçu (`vin` : 0 caché, 1 en place, dépasse un peu avec le ressort)
    pub fn vin(&self, t: u64) -> f32 {
        self.vin.valeur(t)
    }

    pub fn en_cours(&self, t: u64) -> bool {
        self.jauge_couleur.iter().chain(&self.jauge_echelle).chain(&self.categories).any(|g| g.en_cours(t))
            || self.haut.en_cours(t)
            || self.hauteur.en_cours(t)
            || self.vin.en_cours(t)
            || self.puces.values().any(|g| g.en_cours(t))
    }
}

/// Largeurs affichées du volet et de l'aperçu, et fondu de leur contenu
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Volets {
    pub volet: f32,
    pub apercu: f32,
    pub fondu_volet: f32,
    pub fondu_apercu: f32,
    /// Une des largeurs est en train de glisser : le rendu pose les largeurs affichées
    pub glisse: bool,
}

/// Les glissements en cours dans le rendu. La pastille de sélection est suivie par panneau (gauche, droite) ; sa position est
/// en coordonnées du contenu (hors défilement) pour qu'un défilement ne la fasse pas courir derrière la ligne.
pub struct Mouvements {
    pub pastille: [Glisse; 2],
    /// Dossier et nombre d'éléments vus au dernier dessin de chaque panneau : un autre dossier ou un autre contenu = saut
    pub cle_pastille: [Option<(Emplacement, usize)>; 2],
    pub vue: Glisse,
    pub onglet: Glisse,
    /// Nombre d'onglets au dernier dessin : un onglet ajouté ou fermé change la barre, le liseré saute
    pub nb_onglets: usize,
    /// Lot 12B : glissement des entrées de chaque panneau
    pub flips: [Flip; 2],
    /// Lot 12C : fondus de survol, reflet, cases à cocher, interrupteurs, rail et page des Réglages, second panneau
    pub survols: Survols,
    pub reflets: Reflets,
    pub cases: Cases,
    interrupteurs: HashMap<usize, EntreeInterrupteur>,
    rail: Vec<Glisse>,
    pub page: PageAnim,
    /// 0 : second panneau absent ; 1 : en place (F6, 420 ms au ressort)
    pub second: Glisse,
    /// Apparition du menu, du renommage par lot, du formulaire et de la palette (0 : absent, 1 : en place)
    apparus: [Glisse; 4],
    /// Numéro de l'image en cours de dessin : une entrée qui n'est plus vue à la fin de l'image est retirée
    pub image: u32,
    /// Lot 12D : lignes en train de sortir (un jeu par panneau), cartes d'opération, largeurs du volet et de l'aperçu, cadres qui se
    /// déploient (coup d'œil, carte des conflits)
    pub fantomes_lignes: [Fantomes<LigneSortante>; 2],
    pub cartes: Cartes,
    volet: Glisse,
    apercu: Glisse,
    fondu_volet: Glisse,
    fondu_apercu: Glisse,
    pub morph_galerie: Morph,
    pub morph_conflit: Morph,
    palette_etait: bool,
    palette_ferme: Option<u64>,
    /// Lot 13 : yeux de Pixel (regard, nuit, couleur, clignement)
    pub yeux: Yeux,
    /// Lot 14 : pilule de filtre `.spill` de chaque panneau (largeur au ressort, opacité en fondu ; 0 fermée, 1 ouverte)
    pilule: [Glisse; 2],
    pilule_fondu: [Glisse; 2],
    /// Lot 15A : glissements de la palette (jauge, catégories, pastille, puces, aperçu)
    pub palette: AnimPalette,
    /// Lot 15B : tiroir de l'Historique (0 : rentré, 1 : sorti ; le ressort dépasse un peu)
    tiroir: Glisse,
}

#[derive(Clone, Copy, Debug)]
struct EntreeInterrupteur {
    bille: Glisse,
    fond: Glisse,
    vu: u32,
}

impl Mouvements {
    pub fn nouveau() -> Mouvements {
        Mouvements {
            pastille: [Glisse::nouveau(); 2],
            cle_pastille: [None, None],
            vue: Glisse::nouveau(),
            onglet: Glisse::nouveau(),
            nb_onglets: 0,
            flips: [Flip::nouveau(), Flip::nouveau()],
            survols: Survols::nouveau(),
            reflets: Reflets::nouveau(),
            cases: Cases::nouveau(),
            interrupteurs: HashMap::new(),
            rail: Vec::new(),
            page: PageAnim::nouveau(),
            second: Glisse::nouveau(),
            apparus: [Glisse::nouveau(); 4],
            image: 0,
            fantomes_lignes: [Fantomes::nouveau(), Fantomes::nouveau()],
            cartes: Cartes::nouveau(),
            volet: Glisse::nouveau(),
            apercu: Glisse::nouveau(),
            fondu_volet: Glisse::avec(Courbe::Ease),
            fondu_apercu: Glisse::avec(Courbe::Ease),
            morph_galerie: Morph::nouveau(),
            morph_conflit: Morph::nouveau(),
            palette_etait: false,
            palette_ferme: None,
            yeux: Yeux::nouveau(),
            pilule: [Glisse::nouveau(); 2],
            pilule_fondu: [Glisse::avec(Courbe::Ease); 2],
            palette: AnimPalette::nouveau(),
            tiroir: Glisse::nouveau(),
        }
    }

    /// Tiroir de l'Historique (`.hist`, `translateX(105%)` vers `none`) : avancement de 0 (rentré) à 1 (sorti), au ressort en 500 ms.
    /// La première image et le mouvement réduit posent la valeur sans glisser.
    pub fn tiroir(&mut self, ouvert: bool, t: u64, animer: bool) -> f32 {
        self.tiroir.viser(f32::from(ouvert), t, TIROIR_MS, !animer);
        self.tiroir.valeur(t)
    }

    /// Le tiroir est encore (en partie) à l'écran : sorti, ou en train de rentrer. Sert à garder ses entrées jusqu'au bout du glissement.
    pub fn tiroir_visible(&self, t: u64) -> bool {
        self.tiroir.cible() > 0.5 || self.tiroir.valeur(t) > 0.001 || self.tiroir.en_cours(t)
    }

    /// Pilule de filtre du panneau `k` (`.spill`) : (part de sa largeur pleine, opacité). La largeur suit le ressort de la maquette en
    /// 450 ms et peut dépasser 1 un instant (le dessin la borne à son espace) ; l'opacité se fond en 200 ms, dans les deux sens.
    /// Posée sans glisser à la première image et en mouvement réduit.
    pub fn pilule(&mut self, k: usize, ouverte: bool, t: u64, animer: bool) -> (f32, f32) {
        let k = k.min(1);
        let cible = f32::from(ouverte);
        self.pilule[k].viser(cible, t, PILULE_MS, !animer);
        self.pilule_fondu[k].viser(cible, t, PILULE_FONDU_MS, !animer);
        (self.pilule[k].valeur(t).max(0.0), self.pilule_fondu[k].valeur(t).clamp(0.0, 1.0))
    }

    /// Fermeture de la palette : `Some((fraction encore révélée de 0 à 1, translation verticale en DIP))` pendant les 260 ms qui suivent
    /// sa disparition, `None` sinon (ouverte, fermée depuis longtemps, ou mouvement réduit : elle disparaît d'un coup)
    pub fn palette_fermee(&mut self, presente: bool, t: u64, animer: bool) -> Option<(f32, f32)> {
        if self.palette_etait && !presente {
            self.palette_ferme = animer.then_some(t);
        }
        if presente || !animer {
            self.palette_ferme = None;
        }
        self.palette_etait = presente;
        // Palette fermée pour de bon (fermeture finie ou coupée) : les glissements repartent de zéro à la prochaine ouverture
        if !presente && self.palette_ferme.is_none() && self.palette.utilisee {
            self.palette = AnimPalette::nouveau();
        }
        let debut = self.palette_ferme?;
        let e = t.saturating_sub(debut);
        if e >= PALETTE_FERMETURE_MS {
            self.palette_ferme = None;
            return None;
        }
        let k = ease_palette_fermeture(e as f32 / PALETTE_FERMETURE_MS as f32);
        Some((1.0 - k, PALETTE_DECALAGE * k))
    }

    /// Largeurs du volet et de l'aperçu à cette image (`.body`, 500 ms au ressort) et fondu de leur contenu (250 ms). Les cibles
    /// sont les largeurs voulues ; la première image, et le mouvement réduit, posent les valeurs sans glisser.
    pub fn volets(&mut self, volet_cible: f32, apercu_cible: f32, t: u64, animer: bool) -> Volets {
        self.volet.viser(volet_cible, t, VOLET_MS, !animer);
        self.apercu.viser(apercu_cible, t, VOLET_MS, !animer);
        self.fondu_volet.viser(f32::from(volet_cible > 0.0), t, VOLET_FONDU_MS, !animer);
        self.fondu_apercu.viser(f32::from(apercu_cible > 0.0), t, VOLET_FONDU_MS, !animer);
        let glisse = self.volet.en_cours(t) || self.apercu.en_cours(t);
        Volets {
            volet: self.volet.valeur(t).max(0.0),
            apercu: self.apercu.valeur(t).max(0.0),
            fondu_volet: self.fondu_volet.valeur(t).clamp(0.0, 1.0),
            fondu_apercu: self.fondu_apercu.valeur(t).clamp(0.0, 1.0),
            glisse,
        }
    }

    /// Les lignes dont la suppression a été annoncée et qui viennent de quitter la liste du panneau `k` deviennent des fantômes.
    /// `entrees` : (clé, place finale) des entrées affichées à cette image ; `defilement` : décalage de la liste (le fantôme est gardé en coordonnées
    /// du contenu). Rend vrai si au moins une ligne sort : les suivantes glissent alors (`Flip::forcer_glissement`). Pas de
    /// fantôme au-delà de `FLIP_MAX` entrées affichées, ni en mouvement réduit.
    pub fn lignes_retirees(&mut self, k: usize, emplacement: &Emplacement, entrees: &[(String, (f32, f32))], vue: u8, largeur: f32, hauteur: f32, defilement: f32, t: u64, animer: bool) -> bool {
        let k = k.min(1);
        let meme = self.flips[k].meme_dossier(emplacement);
        if !meme {
            self.fantomes_lignes[k].vider();
        }
        self.fantomes_lignes[k].purger(t);
        if !animer || entrees.len() > FLIP_MAX || !meme {
            return false;
        }
        let Ok(mut attendues) = ATTENDUES.lock() else { return false };
        attendues.retain(|a| t.saturating_sub(a.depuis) < ATTENTE_SORTIE_MS && a.jouee.map_or(true, |j| t.saturating_sub(j) < SORTIE_PARTAGEE_MS));
        if attendues.is_empty() {
            return false;
        }
        let presentes: HashSet<&str> = entrees.iter().map(|(k, _)| k.as_str()).collect();
        let mut une = false;
        for a in attendues.iter_mut() {
            if presentes.contains(a.cle.as_str()) {
                continue;
            }
            if let Some((x, y)) = self.flips[k].place_affichee(&a.cle) {
                if self.fantomes_lignes[k].lancer(LigneSortante { vue, ..a.ligne.clone() }, [x, y + defilement, largeur, hauteur], t, SORTIE_LIGNE, true) {
                    une = true;
                }
                a.jouee.get_or_insert(t);
            }
        }
        if une {
            self.flips[k].forcer_glissement();
        }
        une
    }

    /// Au moins un glissement n'a pas fini : la minuterie d'animation doit tourner
    pub fn en_cours(&self, t: u64) -> bool {
        self.en_cours_hors_yeux(t) || self.yeux.en_cours(t)
    }

    /// Comme `en_cours`, sans les yeux de Pixel : quand seuls les yeux bougent, la fenêtre se contente de `IMAGE_YEUX_MS` entre deux
    /// images (un clignement ou un regard n'a pas besoin de la cadence de l'écran, et chaque image redessine toute la fenêtre)
    pub fn en_cours_hors_yeux(&self, t: u64) -> bool {
        self.pastille.iter().any(|g| g.en_cours(t))
            || self.vue.en_cours(t)
            || self.onglet.en_cours(t)
            || self.flips.iter().any(|f| f.en_cours(t))
            || self.survols.en_cours(t)
            || self.reflets.en_cours(t)
            || self.cases.en_cours(t)
            || self.interrupteurs.values().any(|e| e.bille.en_cours(t) || e.fond.en_cours(t))
            || self.rail.iter().any(|g| g.en_cours(t))
            || self.page.en_cours(t)
            || self.second.en_cours(t)
            || self.apparus.iter().any(|g| g.en_cours(t))
            || self.fantomes_lignes.iter().any(|f| f.en_cours(t))
            || self.cartes.en_cours(t)
            || self.volet.en_cours(t)
            || self.apercu.en_cours(t)
            || self.fondu_volet.en_cours(t)
            || self.fondu_apercu.en_cours(t)
            || self.pilule.iter().chain(&self.pilule_fondu).any(|g| g.en_cours(t))
            || self.morph_galerie.en_cours(t)
            || self.morph_conflit.en_cours(t)
            || self.palette_ferme.is_some_and(|d| t.saturating_sub(d) < PALETTE_FERMETURE_MS)
            || self.palette.en_cours(t)
            || self.tiroir.en_cours(t)
    }

    /// Avancement (0 à 1, le ressort dépasse un peu) de l'apparition de `quoi` tant qu'elle n'a pas fini ; `None` : absent, en place,
    /// ou mouvement réduit (en place tout de suite). Un élément absent est remis à zéro pour sa prochaine apparition.
    pub fn apparition(&mut self, quoi: usize, present: bool, duree: u64, t: u64, animer: bool) -> Option<f32> {
        let g = &mut self.apparus[quoi];
        if !present {
            g.viser(0.0, t, 0, true);
            return None;
        }
        g.viser(1.0, t, duree, !animer);
        g.en_cours(t).then(|| g.valeur(t))
    }

    /// Début d'une image : le numéro avance
    pub fn debut_image(&mut self) {
        self.image = self.image.wrapping_add(1);
    }

    /// Fin d'image : les cases et interrupteurs qui n'ont pas été dessinés sont oubliés (page fermée, ligne sortie de la vue)
    pub fn fin_image(&mut self, t: u64) {
        let image = self.image;
        self.cases.fin_image(image, t);
        self.interrupteurs.retain(|_, e| e.vu == image);
    }

    /// Position de la bille (0 à 1, dépasse un peu avec le ressort) et du fond (0 à 1) de l'interrupteur `k`. Un interrupteur vu
    /// pour la première fois est posé à son état sans bouger ; mouvement réduit : l'état tout de suite.
    pub fn interrupteur(&mut self, k: usize, actif: bool, t: u64, animer: bool) -> (f32, f32) {
        let cible = f32::from(actif);
        if !animer {
            self.interrupteurs.clear();
            return (cible, cible);
        }
        let image = self.image;
        let e = self.interrupteurs.entry(k).or_insert_with(|| {
            let (mut bille, mut fond) = (Glisse::nouveau(), Glisse::avec(Courbe::Ease));
            bille.viser(cible, t, 0, true);
            fond.viser(cible, t, 0, true);
            EntreeInterrupteur { bille, fond, vu: image }
        });
        e.vu = image;
        e.bille.viser(cible, t, INTERRUPTEUR_BILLE_MS, false);
        e.fond.viser(cible, t, INTERRUPTEUR_FOND_MS, false);
        (e.bille.valeur(t), e.fond.valeur(t).clamp(0.0, 1.0))
    }

    /// Opacité (0 à 1) de la barre d'accent de la catégorie `k` du rail des Réglages : 200 ms
    pub fn barre_rail(&mut self, k: usize, active: bool, t: u64, animer: bool) -> f32 {
        if self.rail.len() <= k {
            self.rail.resize(k + 1, Glisse::avec(Courbe::Ease));
        }
        self.rail[k].viser(f32::from(active), t, RAIL_MS, !animer);
        self.rail[k].valeur(t).clamp(0.0, 1.0)
    }

    /// La page des Réglages n'est pas dessinée : ses états (interrupteurs, rail) repartent de zéro à la prochaine ouverture
    pub fn oublier_page(&mut self) {
        self.interrupteurs.clear();
        self.rail.clear();
    }

    /// Le second panneau (F6) : 0 quand il n'existe pas, puis monte à 1 en 420 ms
    pub fn second_panneau(&mut self, present: bool, t: u64, animer: bool) -> f32 {
        if present {
            self.second.viser(1.0, t, SECOND_PANNEAU_MS, !animer);
        } else {
            self.second.viser(0.0, t, 0, true);
        }
        self.second.valeur(t)
    }
}

/// Bornes (gauche, droite) d'un fond qui glisse entre des cases alignées : `pos` = 0 sur la première, 1 sur la deuxième, etc.
/// Entre deux cases c'est une interpolation ; hors des cases (dépassement du ressort) le prolongement du dernier écart.
pub fn interpoler(cases: &[(f32, f32)], pos: f32) -> Option<(f32, f32)> {
    let n = cases.len();
    if n == 0 {
        return None;
    }
    if n == 1 {
        return Some(cases[0]);
    }
    let k = (pos.floor() as isize).clamp(0, n as isize - 2) as usize;
    let u = pos - k as f32;
    let (a, b) = (cases[k], cases[k + 1]);
    Some((a.0 + (b.0 - a.0) * u, a.1 + (b.1 - a.1) * u))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ressort_bornes_et_depassement() {
        assert_eq!(ressort(0.0), 0.0);
        assert_eq!(ressort(1.0), 1.0);
        let milieu = ressort(0.5);
        assert!(milieu > 1.0, "le ressort dépasse 1 à mi-course : {milieu}");
        // Le dépassement culmine entre 0,4 et 0,6 de la durée
        let (pic, _) = (0..=100).map(|k| (k as f32 / 100.0, ressort(k as f32 / 100.0))).fold((0.0, 0.0), |a, b| if b.1 > a.1 { b } else { a });
        assert!((0.4..=0.6).contains(&pic), "pic du dépassement à {pic}");
        assert!(ressort(0.99) < ressort(pic));
        // Monotone en montée jusqu'au pic, puis redescend vers 1
        let mut avant = 0.0;
        for k in 0..=(pic * 100.0) as i32 {
            let v = ressort(k as f32 / 100.0);
            assert!(v >= avant - 1e-6);
            avant = v;
        }
    }

    #[test]
    fn ouverture_part_de_l_etat_cache_et_arrive_au_repos() {
        let d = etat_ouverture(0);
        assert_eq!((d.decalage, d.echelle, d.opacite, d.rayon), (48.0, 0.88, 0.0, 36.0));
        assert_eq!(etat_ouverture(OUVERTURE_MS), EtatFenetre::NORMAL);
        assert_eq!(etat_ouverture(OUVERTURE_MS + 1000), EtatFenetre::NORMAL);
        let m = etat_ouverture(OUVERTURE_MS / 2);
        assert!(m.decalage < 0.0 && m.echelle > 1.0, "le ressort dépasse l'état normal : {m:?}");
        assert_eq!(m.opacite, 1.0);
    }

    #[test]
    fn fermeture_sans_depassement() {
        assert_eq!(etat_fermeture(0), EtatFenetre::NORMAL);
        let f = etat_fermeture(FERMETURE_MS);
        assert_eq!((f.decalage, f.echelle, f.opacite, f.rayon), (48.0, 0.88, 0.0, 36.0));
        let mut avant = etat_fermeture(0);
        for ms in (0..=FERMETURE_MS).step_by(11) {
            let e = etat_fermeture(ms);
            assert!(e.decalage >= avant.decalage && e.decalage <= 48.0 && e.echelle <= avant.echelle && e.echelle >= 0.88 && e.opacite <= avant.opacite);
            avant = e;
        }
        assert!(!fermeture_finie(1000, 1000 + FERMETURE_MS - 1));
        assert!(fermeture_finie(1000, 1000 + FERMETURE_MS));
    }

    #[test]
    fn mouvement_reduit_donne_l_etat_final_tout_de_suite() {
        assert_eq!(etat_fenetre(Some(0), None, 10, false), None);
        assert_eq!(etat_fenetre(None, Some(0), 10, false), None);
        assert!(etat_fenetre(Some(0), None, 10, true).is_some());
        assert_eq!(etat_fenetre(Some(0), None, OUVERTURE_MS, true), None, "ouverture finie : plus d'état à dessiner");
        assert_eq!(etat_fenetre(None, None, 5, true), None);
        // La fermeture l'emporte sur une ouverture qui n'aurait pas fini
        assert_eq!(etat_fenetre(Some(0), Some(100), 100, true), Some(etat_fermeture(0)));
    }

    #[test]
    fn glisse_depart_milieu_fin() {
        let mut g = Glisse::nouveau();
        g.viser(10.0, 0, PASTILLE_MS, false);
        assert_eq!(g.valeur(0), 10.0, "première cible posée sans glisser");
        assert!(!g.en_cours(0));
        g.viser(70.0, 1000, PASTILLE_MS, false);
        assert!(g.en_cours(1000));
        assert_eq!(g.valeur(1000), 10.0);
        let milieu = g.valeur(1000 + PASTILLE_MS / 2);
        assert!(milieu > 10.0 && milieu < 90.0, "en route : {milieu}");
        assert!(g.valeur(1000 + PASTILLE_MS - 1) > 60.0);
        assert_eq!(g.valeur(1000 + PASTILLE_MS), 70.0);
        assert!(!g.en_cours(1000 + PASTILLE_MS));
    }

    #[test]
    fn glisse_repart_de_la_valeur_du_moment() {
        let mut g = Glisse::nouveau();
        g.viser(0.0, 0, PASTILLE_MS, false);
        g.viser(100.0, 100, PASTILLE_MS, false);
        let en_route = g.valeur(100 + PASTILLE_MS / 4);
        g.viser(0.0, 100 + PASTILLE_MS / 4, PASTILLE_MS, false);
        assert!((g.valeur(100 + PASTILLE_MS / 4) - en_route).abs() < 1e-3, "pas de saut quand on change d'avis");
        // Même cible : rien ne redémarre
        let avant = g;
        g.viser(0.0, 100 + PASTILLE_MS / 4 + 50, PASTILLE_MS, false);
        assert_eq!(g, avant);
    }

    #[test]
    fn glisse_saute_quand_on_le_demande() {
        let mut g = Glisse::nouveau();
        g.viser(0.0, 0, PASTILLE_MS, false);
        g.viser(300.0, 10, PASTILLE_MS, true);
        assert_eq!(g.valeur(10), 300.0);
        assert!(!g.en_cours(10), "un saut ne laisse aucun mouvement");
        g.oublier();
        g.viser(5.0, 20, PASTILLE_MS, false);
        assert!(!g.en_cours(20));
        assert_eq!(g.valeur(20), 5.0);
    }

    #[test]
    fn mouvement_reduit_pose_la_cible_sans_glisser() {
        // Le caller passe sauter = !animations_actives()
        let mut g = Glisse::nouveau();
        g.viser(0.0, 0, VUE_MS, true);
        g.viser(2.0, 5, VUE_MS, true);
        assert_eq!(g.valeur(5), 2.0);
        assert!(!g.en_cours(5));
    }

    #[test]
    fn pastille_saute_si_la_ligne_precedente_est_hors_vue() {
        let (h, defilement, liste) = (30.0, 300.0, 600.0);
        assert!(ligne_visible(300.0, h, defilement, liste));
        assert!(ligne_visible(270.5, h, defilement, liste), "ligne coupée en haut");
        assert!(!ligne_visible(270.0, h, defilement, liste), "ligne juste au-dessus");
        assert!(ligne_visible(899.0, h, defilement, liste));
        assert!(!ligne_visible(900.0, h, defilement, liste));
        // Précédente visible : on glisse. Précédente hors vue : le caller passe sauter = vrai, la pastille est déjà à la cible.
        let mut g = Glisse::nouveau();
        g.viser(330.0, 0, PASTILLE_MS, false);
        let precedente = g.valeur(10);
        g.viser(360.0, 10, PASTILLE_MS, pastille_saute(precedente, 360.0, h, defilement, liste, true));
        assert!(g.en_cours(10));
        // Défilement ou changement de dossier : l'ancienne position est hors vue, la pastille saute
        let lointaine = 3000.0;
        assert!(pastille_saute(g.valeur(500), lointaine, h, defilement, liste, true));
        g.viser(lointaine, 500, PASTILLE_MS, pastille_saute(g.valeur(500), lointaine, h, defilement, liste, true));
        assert_eq!(g.valeur(500), lointaine);
        assert!(!g.en_cours(500));
        // Mouvement réduit : toujours un saut
        assert!(pastille_saute(330.0, 360.0, h, defilement, liste, false));
    }

    #[test]
    fn indicateur_de_vue_pour_chaque_paire_de_cases() {
        let cases = [(0.0, 40.0), (44.0, 84.0), (88.0, 128.0)];
        for de in 0..3 {
            for vers in 0..3 {
                let mut g = Glisse::nouveau();
                g.viser(de as f32, 0, VUE_MS, false);
                g.viser(vers as f32, 100, VUE_MS, false);
                let (g0, d0) = interpoler(&cases, g.valeur(100)).unwrap();
                assert_eq!((g0, d0), cases[de], "départ sur la case {de}");
                let (g1, d1) = interpoler(&cases, g.valeur(100 + VUE_MS)).unwrap();
                assert_eq!((g1, d1), cases[vers], "arrivée sur la case {vers}");
                if de != vers {
                    assert!(g.en_cours(100 + VUE_MS / 2));
                    let (gm, _) = interpoler(&cases, g.valeur(100 + VUE_MS / 2)).unwrap();
                    let (bas, haut) = (cases[de].0.min(cases[vers].0), cases[de].0.max(cases[vers].0));
                    // À mi-course le ressort a dépassé la cible d'au plus 25 % de la distance
                    let marge = (haut - bas) * 0.25;
                    assert!(gm > bas - marge && gm < haut + marge, "{de} -> {vers} : milieu {gm}");
                    assert!(gm != cases[de].0, "{de} -> {vers} : la case a bougé");
                } else {
                    assert!(!g.en_cours(100));
                }
            }
        }
        assert_eq!(interpoler(&[], 0.5), None);
        assert_eq!(interpoler(&cases[..1], 3.0), Some(cases[0]));
        assert_eq!(interpoler(&cases, 0.5), Some((22.0, 62.0)));
    }

    fn dossier_a() -> Emplacement {
        Emplacement::dossier("C:\\essai")
    }

    /// Trois entrées en liste : a, b, c à 0, 30, 60
    fn trois(ordre: [&str; 3]) -> Vec<(String, (f32, f32))> {
        ordre.iter().enumerate().map(|(i, k)| (k.to_string(), (10.0, i as f32 * 30.0))).collect()
    }

    #[test]
    fn flip_decalage_ancienne_vers_nouvelle_position() {
        let mut f = Flip::nouveau();
        let g = 0;
        // Premier dessin : rien à animer
        assert!(f.image(&dossier_a(), 30, g, &trois(["a", "b", "c"]), 1000, true).iter().all(|d| d.est_aucun()));
        // Tri demandé : a passe de 0 à 60, b et c remontent d'un cran
        let d = f.image(&dossier_a(), 30, g + 1, &trois(["b", "c", "a"]), 1100, true);
        assert_eq!((d[0].dx, d[0].dy), (0.0, 30.0), "b était 30 plus bas");
        assert_eq!((d[1].dx, d[1].dy), (0.0, 30.0), "c était 30 plus bas");
        assert_eq!((d[2].dx, d[2].dy), (0.0, -60.0), "a était 60 plus haut");
        assert!(d.iter().all(|x| x.echelle == 1.0 && x.opacite == 1.0));
        assert_eq!(f.nombre(), 3);
        assert!(f.en_cours(1100));
    }

    #[test]
    fn flip_entree_nouvelle_et_entree_disparue() {
        let mut f = Flip::nouveau();
        f.image(&dossier_a(), 30, 0, &trois(["a", "b", "c"]), 1000, true);
        // Filtre : c disparaît (aucune animation, elle n'est plus dessinée), d est nouvelle, a reste en place
        let apres: Vec<(String, (f32, f32))> = vec![("a".into(), (10.0, 0.0)), ("d".into(), (10.0, 30.0))];
        let d = f.image(&dossier_a(), 30, 1, &apres, 1100, true);
        assert!(d[0].est_aucun(), "a n'a pas bougé");
        assert_eq!((d[1].opacite, d[1].echelle), (0.0, 0.9), "d naît invisible et plus petite");
        assert_eq!(f.nombre(), 1, "c disparue n'est pas suivie");
        // Fin de l'apparition : opaque, échelle 1
        let fin = f.image(&dossier_a(), 30, 1, &apres, 1100 + FLIP_NOUVEAU_MS, true);
        assert!(fin[1].est_aucun());
    }

    #[test]
    fn flip_interpolation_depart_milieu_fin() {
        let mut f = Flip::nouveau();
        f.image(&dossier_a(), 30, 0, &trois(["a", "b", "c"]), 0, true);
        let arrivee = trois(["b", "a", "c"]);
        let depart = f.image(&dossier_a(), 30, 1, &arrivee, 100, true);
        assert_eq!(depart[0].dy, 30.0, "t = 0 : à l'ancienne place");
        let milieu = f.image(&dossier_a(), 30, 1, &arrivee, 100 + FLIP_MS / 2, true);
        // Le ressort dépasse la cible à mi-course : l'écart a changé de signe ou presque
        assert!(milieu[0].dy < 30.0 && milieu[0].dy > -30.0, "en route : {}", milieu[0].dy);
        assert!(milieu[0].dy != 0.0);
        let presque = f.image(&dossier_a(), 30, 1, &arrivee, 100 + FLIP_MS - 1, true);
        assert!(presque[0].dy.abs() < 3.0, "presque arrivée : {}", presque[0].dy);
        let fin = f.image(&dossier_a(), 30, 1, &arrivee, 100 + FLIP_MS, true);
        assert!(fin.iter().all(|d| d.est_aucun()), "t = durée : à la place finale");
    }

    #[test]
    fn flip_repart_de_la_position_affichee() {
        let mut f = Flip::nouveau();
        f.image(&dossier_a(), 30, 0, &trois(["a", "b", "c"]), 0, true);
        let apres = trois(["b", "a", "c"]);
        f.image(&dossier_a(), 30, 1, &apres, 100, true);
        let t = 100 + FLIP_MS / 4;
        let en_route = f.image(&dossier_a(), 30, 1, &apres, t, true)[0];
        // Nouveau changement en route : retour à l'ordre d'origine. Le point de départ de b est sa place AFFICHÉE (30 + son écart en cours).
        let avant = trois(["a", "b", "c"]);
        let d = f.image(&dossier_a(), 30, 2, &avant, t, true);
        let affichee_b = 0.0 + en_route.dy; // b est affichée à sa place finale (0) + écart en cours
        let attendue = affichee_b - 30.0; // nouvelle place finale de b : 30
        assert!((d[1].dy - attendue).abs() < 1e-3, "b repart de {affichee_b}, écart {} attendu {attendue}", d[1].dy);
        assert!(en_route.dy != 0.0, "le test ne vaut que si b était en route");
    }

    #[test]
    fn flip_plafond_de_400_entrees() {
        let beaucoup = |n: usize, decale: f32| -> Vec<(String, (f32, f32))> { (0..n).map(|i| (format!("e{i}"), (0.0, i as f32 * 30.0 + decale))).collect() };
        let mut f = Flip::nouveau();
        f.image(&dossier_a(), 30, 0, &beaucoup(FLIP_MAX, 0.0), 0, true);
        let d = f.image(&dossier_a(), 30, 1, &beaucoup(FLIP_MAX, 5.0), 100, true);
        assert!(d.iter().any(|x| !x.est_aucun()), "400 entrées : animées");
        let mut g = Flip::nouveau();
        g.image(&dossier_a(), 30, 0, &beaucoup(FLIP_MAX + 1, 0.0), 0, true);
        let d = g.image(&dossier_a(), 30, 1, &beaucoup(FLIP_MAX + 1, 5.0), 100, true);
        assert!(d.iter().all(|x| x.est_aucun()), "401 entrées : mise à jour directe");
        assert_eq!(g.nombre(), 0);
        assert!(!g.en_cours(100));
    }

    #[test]
    fn flip_mouvement_reduit_aucun_decalage() {
        let mut f = Flip::nouveau();
        f.image(&dossier_a(), 30, 0, &trois(["a", "b", "c"]), 0, false);
        let d = f.image(&dossier_a(), 30, 1, &trois(["c", "b", "a"]), 100, false);
        assert!(d.iter().all(|x| x.est_aucun()));
        assert_eq!(f.nombre(), 0);
        assert!(!f.en_cours(100));
        // Un mouvement qui tournait s'arrête net quand le réglage change
        let mut g = Flip::nouveau();
        g.image(&dossier_a(), 30, 0, &trois(["a", "b", "c"]), 0, true);
        g.image(&dossier_a(), 30, 1, &trois(["c", "b", "a"]), 100, true);
        assert!(g.en_cours(150));
        g.image(&dossier_a(), 30, 1, &trois(["c", "b", "a"]), 200, false);
        assert!(!g.en_cours(200) && g.nombre() == 0);
    }

    #[test]
    fn flip_se_vide_a_la_fin_et_mesure_la_duree() {
        let mut f = Flip::nouveau();
        f.image(&dossier_a(), 30, 0, &trois(["a", "b", "c"]), 0, true);
        let apres = trois(["c", "b", "a"]);
        for ms in (100..100 + FLIP_MS).step_by(16) {
            f.image(&dossier_a(), 30, 1, &apres, ms, true);
        }
        assert!(f.en_cours(100 + FLIP_MS - 1));
        assert!(!f.en_cours(100 + FLIP_MS), "la minuterie peut s'arrêter dès la durée écoulée");
        f.image(&dossier_a(), 30, 1, &apres, 100 + FLIP_MS, true);
        assert_eq!(f.nombre(), 0, "structure vidée");
        let (duree, images) = f.mesure.expect("mesure posée à la fin");
        // Les derniers dessins ont un écart sous 0,01 DIP : ils ne comptent pas comme « avec décalage »
        assert!(duree <= FLIP_MS && duree >= 300 && images >= 15, "{duree} ms, {images} images");
        // Sans nouveau changement demandé, rien ne redémarre
        let d = f.image(&dossier_a(), 30, 1, &trois(["a", "b", "c"]), 5000, true);
        assert!(d.iter().all(|x| x.est_aucun()));
    }

    #[test]
    fn flip_ignore_autre_dossier_dessin_perime_et_defilement() {
        let mut f = Flip::nouveau();
        f.image(&dossier_a(), 30, 0, &trois(["a", "b", "c"]), 0, true);
        // Ouverture d'un autre dossier, même avec un changement demandé : pas de glissement (le fondu existant reste)
        let ailleurs = Emplacement::dossier("C:\\autre");
        let d = f.image(&ailleurs, 30, 1, &trois(["c", "b", "a"]), 100, true);
        assert!(d.iter().all(|x| x.est_aucun()));
        // Sans demande (défilement, actualisation du système de fichiers) : entrées déplacées, aucun glissement
        let d = f.image(&ailleurs, 30, 1, &trois(["a", "b", "c"]), 200, true);
        assert!(d.iter().all(|x| x.est_aucun()));
        // Dessin précédent trop ancien (fenêtre cachée) : la demande ne sert à rien
        let d = f.image(&ailleurs, 30, 2, &trois(["c", "b", "a"]), 200 + FLIP_PEREMPTION_MS + 1, true);
        assert!(d.iter().all(|x| x.est_aucun()));
    }

    #[test]
    fn flip_changement_de_densite_dans_le_meme_dossier() {
        let mut f = Flip::nouveau();
        f.image(&dossier_a(), 40, 0, &trois(["a", "b", "c"]), 0, true);
        let compact: Vec<(String, (f32, f32))> = ["a", "b", "c"].iter().enumerate().map(|(i, k)| (k.to_string(), (10.0, i as f32 * 20.0))).collect();
        let d = f.image(&dossier_a(), 30, 0, &compact, 100, true);
        assert_eq!(d[2].dy, 20.0, "c était à 60 (30 × 2), elle va à 40");
    }

    #[test]
    fn flip_la_demande_est_un_compteur() {
        let avant = generation_flip();
        demander_flip();
        assert_eq!(generation_flip(), avant + 1);
    }

    // ── Lot 12C ──

    #[test]
    fn cascade_26_ms_par_ligne_plafonnee_a_600_ms() {
        // Avant son tour, la ligne est invisible (fill: backwards) et décalée de 28 DIP
        let d = decalage_cascade(3, 50);
        assert_eq!((d.dx, d.opacite), (28.0, 0.0), "ligne 3 : délai 78 ms");
        assert!(decalage_cascade(0, 30).opacite > 0.0);
        assert!(decalage_cascade(3, 78 + 30).opacite > 0.0);
        // Le délai s'arrête à 140 ms : les lignes 6 à 15 démarrent ensemble
        for rang in 6..CASCADE_LIGNES {
            assert_eq!(decalage_cascade(rang, 139).opacite, 0.0, "ligne {rang}");
            assert!(decalage_cascade(rang, 150).opacite > 0.0, "ligne {rang}");
        }
        // Plus rien à dessiner au bout de 600 ms, pour toutes les lignes
        for rang in 0..40 {
            assert!(decalage_cascade(rang, CASCADE_TOTAL_MS).est_aucun(), "ligne {rang}");
        }
        assert_eq!(CASCADE_TOTAL_MS, 600);
        // Seules les 16 premières lignes entrent à tour de rôle
        assert!(decalage_cascade(CASCADE_LIGNES, 0).est_aucun());
        // La ligne 0 arrive au repos après 460 ms
        assert!(decalage_cascade(0, CASCADE_MS).est_aucun());
    }

    #[test]
    fn cascade_se_combine_avec_le_glissement_d_un_tri() {
        let tri = Decalage { dx: 0.0, dy: 30.0, echelle: 0.9, opacite: 0.5 };
        let c = combiner(tri, Decalage { dx: 28.0, dy: 0.0, echelle: 1.0, opacite: 0.5 });
        assert_eq!((c.dx, c.dy, c.opacite), (28.0, 30.0, 0.25));
        assert!((c.echelle - 0.9).abs() < 1e-6);
        assert_eq!(combiner(Decalage::AUCUN, Decalage::AUCUN), Decalage::AUCUN);
    }

    fn ligne(i: usize) -> Cible {
        Cible::Ligne(i)
    }

    #[test]
    fn fond_de_survol_d_une_ligne_120_ms_entree_et_sortie() {
        let mut s = Survols::nouveau();
        s.maj(ligne(2), false, 1000, true);
        assert_eq!(s.fond(ligne(2), 1000), 0.0, "le fondu part de zéro");
        let milieu = s.fond(ligne(2), 1060);
        assert!(milieu > 0.2 && milieu < 1.0, "en route à 60 ms : {milieu}");
        assert_eq!(s.fond(ligne(2), 1000 + SURVOL_LIGNE_MS), 1.0);
        assert!(s.en_cours(1100));
        assert!(!s.en_cours(1000 + SURVOL_LIGNE_MS));
        // Le pointeur quitte la ligne : elle s'éteint en 120 ms, sans saut
        s.maj(Cible::Rien, false, 2000, true);
        let sortie = s.fond(ligne(2), 2060);
        assert!(sortie > 0.0 && sortie < 1.0, "elle s'éteint : {sortie}");
        assert!(s.en_cours(2060));
        assert_eq!(s.fond(ligne(2), 2000 + SURVOL_LIGNE_MS), 0.0);
        s.maj(Cible::Rien, false, 2000 + SURVOL_LIGNE_MS, true);
        assert!(!s.en_cours(2000 + SURVOL_LIGNE_MS), "plus aucun mouvement");
        assert!(s.soulevements(2200).is_empty());
    }

    #[test]
    fn fond_du_chrome_160_ms_et_passage_d_un_bouton_a_l_autre() {
        let mut s = Survols::nouveau();
        s.maj(Cible::Precedent, false, 0, true);
        assert!(s.fond(Cible::Precedent, 100) < 1.0);
        assert_eq!(s.fond(Cible::Precedent, SURVOL_CHROME_MS), 1.0);
        // Passage d'un bouton à l'autre : le premier s'éteint pendant que le second s'allume
        s.maj(Cible::Suivant, false, 500, true);
        let (a, b) = (s.fond(Cible::Precedent, 580), s.fond(Cible::Suivant, 580));
        assert!(a > 0.0 && a < 1.0 && b > 0.0 && b < 1.0, "{a} {b}");
        assert_eq!(duree_survol(Cible::Precedent), 160);
        assert_eq!(duree_survol(ligne(0)), 120);
    }

    #[test]
    fn bouton_de_barre_se_souleve_de_2_dip_au_ressort_et_retombe_a_l_appui() {
        let mut s = Survols::nouveau();
        s.maj(Cible::NouvelOnglet, false, 0, true);
        assert_eq!(s.soulevement(Cible::NouvelOnglet, 0), 0.0);
        // Le ressort dépasse la cible (-2) à mi-course
        assert!(s.soulevement(Cible::NouvelOnglet, SURVOL_CHROME_MS / 2) < -2.0, "dépassement : {}", s.soulevement(Cible::NouvelOnglet, 80));
        assert_eq!(s.soulevement(Cible::NouvelOnglet, SURVOL_CHROME_MS), -2.0);
        // Appui : retour à plat en 60 ms
        s.maj(Cible::NouvelOnglet, true, 1000, true);
        assert!(s.en_cours(1030));
        assert_eq!(s.soulevement(Cible::NouvelOnglet, 1000 + APPUI_MS), 0.0);
        s.maj(Cible::NouvelOnglet, true, 1000 + APPUI_MS, true);
        assert!(!s.en_cours(1000 + APPUI_MS), "à plat pendant l'appui, plus rien ne bouge");
        // Relâchement sur le même bouton : il remonte en 160 ms
        s.maj(Cible::NouvelOnglet, false, 2000, true);
        assert!(s.soulevement(Cible::NouvelOnglet, 2000 + SURVOL_CHROME_MS / 2) < -1.0);
        assert_eq!(s.soulevement(Cible::NouvelOnglet, 2000 + SURVOL_CHROME_MS), -2.0);
        // Une ligne de liste ne se soulève jamais
        s.maj(ligne(1), false, 3000, true);
        assert_eq!(s.soulevement(ligne(1), 3200), 0.0);
        // Le bouton quitté retombe au ressort puis disparaît
        let retombe = s.soulevement(Cible::NouvelOnglet, 3000 + SURVOL_CHROME_MS / 2);
        assert!(retombe > -2.0 && retombe < 1.0, "il retombe en ressort (dépasse un peu le plat) : {retombe}");
        s.maj(ligne(1), false, 3000 + SURVOL_CHROME_MS + 130, true);
        assert!(s.soulevements(3500).is_empty());
    }

    #[test]
    fn survol_en_mouvement_reduit_est_instantane_et_ne_garde_rien() {
        let mut s = Survols::nouveau();
        s.maj(Cible::Precedent, false, 0, false);
        assert_eq!(s.fond(Cible::Precedent, 0), 1.0, "fond tout de suite");
        assert_eq!(s.soulevement(Cible::Precedent, 0), -2.0, "état de survol posé, sans ressort");
        assert!(!s.en_cours(0));
        s.maj(ligne(4), false, 10, false);
        assert_eq!(s.fond(Cible::Precedent, 10), 0.0, "plus survolé : éteint tout de suite");
        assert_eq!(s.fond(ligne(4), 10), 1.0);
        assert!(!s.en_cours(10));
        assert!(s.soulevements(10).is_empty());
        assert_eq!(s.bulle(10), None, "pas d'infobulle sur une ligne");
    }

    #[test]
    fn infobulle_500_ms_de_delai_puis_240_ms() {
        let mut s = Survols::nouveau();
        s.maj(Cible::Outil(crate::barre::Outil::Nouveau), false, 1000, true);
        assert_eq!(s.bulle(1000), None);
        assert_eq!(s.bulle(1000 + BULLE_DELAI_MS - 1), None);
        let (o, dy) = s.bulle(1000 + BULLE_DELAI_MS + 10).expect("elle paraît");
        assert!(o > 0.0 && o < 1.0 && dy < 0.0, "début : {o} {dy}");
        assert!(s.en_cours(1000 + BULLE_DELAI_MS + 10));
        let (o, dy) = s.bulle(1000 + BULLE_DELAI_MS + BULLE_MS).unwrap();
        assert_eq!((o, dy), (1.0, 0.0));
        assert!(!s.en_cours(1000 + BULLE_DELAI_MS + BULLE_MS), "plus de minuterie une fois posée");
        // Mouvement réduit : tout de suite, sans mouvement
        let mut r = Survols::nouveau();
        r.maj(Cible::Outil(crate::barre::Outil::Nouveau), false, 1000, false);
        assert_eq!(r.bulle(1000), Some((1.0, 0.0)));
        assert!(!r.en_cours(1000));
    }

    #[test]
    fn reflet_180_ms_a_l_entree_et_a_la_sortie() {
        use crate::reflet::planifier;
        use crate::rendu::rect;
        let plan = |x: f32| planifier(rect(100.0, 100.0, 120.0, 36.0), 8.0, false, Some((x, 110.0)), true).unwrap();
        let mut r = Reflets::nouveau();
        let v = r.maj(Some((plan(150.0), None)), 0, true);
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].2, 0.0, "il part de zéro");
        let v = r.maj(Some((plan(160.0), None)), 90, true);
        assert!(v[0].2 > 0.2 && v[0].2 < 1.0, "à mi-course : {}", v[0].2);
        assert_eq!(v[0].0.centre.0, 160.0, "il suit le curseur");
        let v = r.maj(Some((plan(170.0), None)), REFLET_MS, true);
        assert_eq!(v[0].2, 1.0);
        assert!(!r.en_cours(REFLET_MS));
        // Le curseur sort : le reflet reste à sa dernière place et s'éteint
        let v = r.maj(None, 1000, true);
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].0.centre.0, 170.0);
        let v = r.maj(None, 1090, true);
        assert!(v[0].2 > 0.0 && v[0].2 < 1.0);
        assert!(r.en_cours(1090));
        let v = r.maj(None, 1000 + REFLET_MS, true);
        assert!(v.is_empty() && r.nombre() == 0, "retiré à la fin");
        assert!(!r.en_cours(1000 + REFLET_MS));
    }

    #[test]
    fn reflet_en_mouvement_reduit_sans_fondu() {
        use crate::reflet::planifier;
        use crate::rendu::rect;
        let p = planifier(rect(0.0, 0.0, 100.0, 30.0), 8.0, false, Some((10.0, 10.0)), true).unwrap();
        let mut r = Reflets::nouveau();
        assert_eq!(r.maj(Some((p, None)), 0, false)[0].2, 1.0);
        assert!(r.maj(None, 1, false).is_empty());
        assert_eq!(r.nombre(), 0);
    }

    #[test]
    fn case_a_cocher_150_ms_de_fondu_300_ms_d_echelle() {
        let mut c = Cases::nouveau();
        assert_eq!(c.etat(7, false, 1, 0, true), (0.0, 0.6), "cachée : rien à garder");
        assert_eq!(c.nombre(), 0);
        let (o, e) = c.etat(7, true, 1, 100, true);
        assert_eq!((o, e), (0.0, 0.6));
        let (o, e) = c.etat(7, true, 1, 100 + CASE_FONDU_MS, true);
        assert_eq!(o, 1.0, "opaque après 150 ms");
        assert!(e > 0.6 && e < 1.5, "l'échelle est encore en route : {e}");
        let (_, e) = c.etat(7, true, 1, 100 + CASE_ECHELLE_MS, true);
        assert_eq!(e, 1.0);
        c.fin_image(1, 100 + CASE_ECHELLE_MS);
        assert!(!c.en_cours(100 + CASE_ECHELLE_MS));
        // Le pointeur part : elle s'efface, puis l'entrée est retirée
        let (o, _) = c.etat(7, false, 2, 1000, true);
        assert_eq!(o, 1.0);
        let (o, _) = c.etat(7, false, 2, 1075, true);
        assert!(o > 0.0 && o < 1.0);
        c.fin_image(2, 1075);
        assert!(c.en_cours(1075));
        c.etat(7, false, 3, 1000 + CASE_ECHELLE_MS, true);
        c.fin_image(3, 1000 + CASE_ECHELLE_MS);
        assert_eq!(c.nombre(), 0);
    }

    #[test]
    fn case_a_cocher_oubliee_quand_la_ligne_n_est_plus_dessinee_et_reduit_instantane() {
        let mut c = Cases::nouveau();
        c.etat(1, true, 1, 0, true);
        c.etat(2, true, 1, 0, true);
        c.fin_image(1, 0);
        assert_eq!(c.nombre(), 2);
        c.etat(1, true, 2, 10, true);
        c.fin_image(2, 10);
        assert_eq!(c.nombre(), 1, "la ligne 2 est sortie de la vue");
        let mut r = Cases::nouveau();
        assert_eq!(r.etat(1, true, 1, 0, false), (1.0, 1.0));
        assert_eq!(r.etat(1, false, 1, 0, false), (0.0, 0.6));
        assert_eq!(r.nombre(), 0);
        assert!(!r.en_cours(0));
    }

    #[test]
    fn interrupteur_bille_300_ms_ressort_fond_200_ms() {
        let mut m = Mouvements::nouveau();
        // Première vue : posé à son état, rien ne bouge
        assert_eq!(m.interrupteur(3, false, 0, true), (0.0, 0.0));
        assert!(!m.en_cours(0));
        // Bascule : le fond a fini à 200 ms, la bille à 300 ms
        m.interrupteur(3, true, 1000, true);
        assert!(m.en_cours(1100));
        let (bille, fond) = m.interrupteur(3, true, 1100, true);
        assert!(bille > 0.2 && fond > 0.2 && fond < 1.0, "en route : {bille} {fond}");
        assert_eq!(m.interrupteur(3, true, 1000 + INTERRUPTEUR_FOND_MS, true).1, 1.0);
        assert!(m.en_cours(1000 + INTERRUPTEUR_FOND_MS), "la bille tourne encore");
        let (bille, fond) = m.interrupteur(3, true, 1000 + INTERRUPTEUR_BILLE_MS, true);
        assert_eq!((bille, fond), (1.0, 1.0));
        assert!(!m.en_cours(1000 + INTERRUPTEUR_BILLE_MS));
        // Le ressort dépasse la position finale avant de se poser
        m.interrupteur(3, false, 2000, true);
        let milieu = m.interrupteur(3, false, 2000 + INTERRUPTEUR_BILLE_MS / 2, true).0;
        assert!(milieu < 0.0, "dépassement vers l'autre côté : {milieu}");
    }

    #[test]
    fn interrupteur_instantane_en_mouvement_reduit_et_oublie_hors_page() {
        let mut m = Mouvements::nouveau();
        assert_eq!(m.interrupteur(1, true, 0, false), (1.0, 1.0));
        assert_eq!(m.interrupteur(1, false, 5, false), (0.0, 0.0));
        assert!(!m.en_cours(5));
        // Une page non dessinée : les interrupteurs ne gardent rien
        let mut m = Mouvements::nouveau();
        m.debut_image();
        m.interrupteur(1, false, 0, true);
        m.fin_image(0);
        m.debut_image();
        m.fin_image(10);
        m.debut_image();
        // Réapparu à l'état inverse : posé, pas de mouvement venu d'une ancienne page
        assert_eq!(m.interrupteur(1, true, 1000, true), (1.0, 1.0));
        assert!(!m.en_cours(1000));
    }

    #[test]
    fn barre_du_rail_200_ms_et_posee_a_la_premiere_vue() {
        let mut m = Mouvements::nouveau();
        assert_eq!(m.barre_rail(0, true, 0, true), 1.0);
        assert_eq!(m.barre_rail(1, false, 0, true), 0.0);
        assert!(!m.en_cours(0));
        // Changement de catégorie : l'ancienne s'éteint, la nouvelle s'allume
        m.barre_rail(0, false, 1000, true);
        m.barre_rail(1, true, 1000, true);
        let (a, b) = (m.barre_rail(0, false, 1100, true), m.barre_rail(1, true, 1100, true));
        assert!(a > 0.0 && a < 1.0 && b > 0.0 && b < 1.0, "{a} {b}");
        assert!(m.en_cours(1100));
        assert_eq!((m.barre_rail(0, false, 1000 + RAIL_MS, true), m.barre_rail(1, true, 1000 + RAIL_MS, true)), (0.0, 1.0));
        assert!(!m.en_cours(1000 + RAIL_MS));
        // Mouvement réduit : posée tout de suite
        m.barre_rail(1, false, 3000, false);
        assert_eq!(m.barre_rail(1, false, 3000, false), 0.0);
    }

    #[test]
    fn page_des_reglages_ouverture_380_ms_fermeture_160_ms() {
        let o = etat_page_ouverture(0).unwrap();
        assert_eq!((o.opacite, o.echelle), (0.0, 0.97));
        assert!(etat_page_ouverture(PAGE_OUVERTURE_MS).is_none());
        let f = etat_page_fermeture(0).unwrap();
        assert_eq!((f.opacite, f.echelle), (1.0, 1.0));
        let f = etat_page_fermeture(PAGE_FERMETURE_MS / 2).unwrap();
        assert!((f.opacite - 0.5).abs() < 1e-6 && (f.echelle - 0.99).abs() < 1e-6, "linéaire : {f:?}");
        assert!(etat_page_fermeture(PAGE_FERMETURE_MS).is_none());

        let mut p = PageAnim::nouveau();
        p.maj(false, 0, true);
        assert!(!p.en_cours(0));
        p.maj(true, 1000, true);
        assert!(matches!(p.etat(1100), Some((true, _))));
        assert!(!p.en_cours(1000 + PAGE_OUVERTURE_MS), "plus de minuterie une fois ouverte");
        // Fermée : la fenêtre doit encore fournir la page pendant 160 ms
        assert!(p.sortie_voulue(false, 5000), "page quittée, pas encore vue fermée");
        p.maj(false, 5000, true);
        assert!(p.sortie_voulue(false, 5100));
        assert!(matches!(p.etat(5100), Some((false, _))));
        assert!(!p.sortie_voulue(false, 5000 + PAGE_FERMETURE_MS));
        assert!(!p.en_cours(5000 + PAGE_FERMETURE_MS));
        assert!(!p.sortie_voulue(true, 0), "page affichée : pas de sortie");
    }

    #[test]
    fn page_en_mouvement_reduit_sans_fondu() {
        let mut p = PageAnim::nouveau();
        p.maj(true, 0, false);
        assert!(p.etat(10).is_none());
        p.maj(false, 100, false);
        assert!(p.etat(110).is_none() && !p.en_cours(110));
    }

    #[test]
    fn second_panneau_arrive_en_420_ms() {
        let mut m = Mouvements::nouveau();
        assert_eq!(m.second_panneau(false, 0, true), 0.0);
        let debut = m.second_panneau(true, 1000, true);
        assert_eq!(debut, 0.0, "il part de zéro");
        assert!(m.en_cours(1100));
        let milieu = m.second_panneau(true, 1210, true);
        assert!(milieu > 0.3, "{milieu}");
        assert_eq!(m.second_panneau(true, 1000 + SECOND_PANNEAU_MS, true), 1.0);
        assert!(!m.en_cours(1000 + SECOND_PANNEAU_MS));
        // Fermé puis rouvert : il repart de zéro ; mouvement réduit : en place tout de suite
        m.second_panneau(false, 3000, true);
        assert_eq!(m.second_panneau(true, 3001, true), 0.0);
        let mut r = Mouvements::nouveau();
        r.second_panneau(false, 0, false);
        assert_eq!(r.second_panneau(true, 10, false), 1.0);
        assert!(!r.en_cours(10));
    }

    #[test]
    fn rien_ne_tourne_une_fois_tous_les_mouvements_finis() {
        let mut m = Mouvements::nouveau();
        m.survols.maj(Cible::Precedent, false, 0, true);
        m.debut_image();
        let image = m.image;
        m.cases.etat(1, true, image, 0, true);
        m.interrupteur(0, false, 0, true);
        m.interrupteur(0, true, 0, true);
        m.barre_rail(0, true, 0, true);
        m.barre_rail(0, false, 0, true);
        m.second_panneau(false, 0, true);
        m.second_panneau(true, 0, true);
        m.page.maj(true, 0, true);
        assert!(m.en_cours(10));
        let fin = 2000;
        m.survols.maj(Cible::Precedent, false, fin, true);
        m.cases.etat(1, true, image, fin, true);
        m.reflets.maj(None, fin, true);
        assert!(!m.en_cours(fin), "aucune minuterie nécessaire au repos");
    }

    #[test]
    fn cadence_moyenne_ecart_type_maximum() {
        assert_eq!(bilan_cadence(&[]), None);
        let b = bilan_cadence(&[16.0, 16.0, 16.0, 16.0]).unwrap();
        assert_eq!((b.moyenne, b.ecart_type, b.maximum, b.intervalles), (16.0, 0.0, 16.0, 4));
        let b = bilan_cadence(&[10.0, 20.0]).unwrap();
        assert_eq!((b.moyenne, b.ecart_type, b.maximum), (15.0, 5.0, 20.0));
        // Une série : les dessins se suivent, une pause de plus de 150 ms la termine
        let mut c = Cadence::nouveau();
        assert_eq!(c.noter(0), None);
        assert_eq!(c.noter(16_000), None);
        assert_eq!(c.noter(32_000), None);
        let fini = c.noter(500_000).expect("la pause termine la série");
        assert_eq!(fini.intervalles, 2);
        assert_eq!(fini.moyenne, 16.0);
        assert_eq!(c.vider(), None, "la nouvelle série n'a pas encore d'intervalle");
    }

    #[test]
    fn plus_rien_ne_bouge_une_fois_les_durees_ecoulees() {
        let mut m = Mouvements::nouveau();
        assert!(!m.en_cours(0), "au repos, aucune minuterie nécessaire");
        m.pastille[0].viser(0.0, 0, PASTILLE_MS, false);
        m.vue.viser(0.0, 0, VUE_MS, false);
        m.onglet.viser(0.0, 0, ONGLET_MS, false);
        assert!(!m.en_cours(0));
        m.pastille[0].viser(30.0, 1000, PASTILLE_MS, false);
        m.vue.viser(1.0, 1000, VUE_MS, false);
        m.onglet.viser(1.0, 1000, ONGLET_MS, false);
        assert!(m.en_cours(1000));
        assert!(m.en_cours(1000 + VUE_MS - 1), "la vue (450 ms) est la dernière à finir");
        assert!(!m.en_cours(1000 + VUE_MS));
        // L'ouverture et la fermeture suivent la même règle
        assert!(etat_fenetre(Some(0), None, OUVERTURE_MS - 1, true).is_some());
        assert!(etat_fenetre(Some(0), None, OUVERTURE_MS, true).is_none());
    }

    // ── Lot 12D ──

    fn entree_test(nom: &str) -> Entree {
        Entree {
            nom: nom.to_string(),
            nom_w: nom.encode_utf16().chain(std::iter::once(0)).collect(),
            genre: crate::dossier::Genre::Fichier,
            taille: 1234,
            modif: 0,
            ajout: 0,
            ext: String::new(),
            cache: false,
            parent: None,
            lien: false,
            nuage: crate::dossier::Nuage::Aucun,
            meta: None,
        }
    }

    fn dispo_cartes(libelles: &[&str]) -> (Disposition, Vec<Pastille>) {
        let p: Vec<Pastille> = libelles.iter().enumerate().map(|(i, l)| Pastille::operation(l.to_string(), 0.4, false, None).avec_cle(i as u64 + 1)).collect();
        let d = crate::pastilles::mettre_en_page_anime(1600.0, 220.0, 700.0, &p, &vec![0.0; p.len()], 60.0, 40.0, &|t: &str| t.chars().count() as f32 * 8.0);
        (d, p)
    }

    #[test]
    fn fantome_joue_puis_supprime() {
        let mut f: Fantomes<u32> = Fantomes::nouveau();
        assert!(!f.en_cours(0), "au repos, aucune minuterie");
        assert!(f.lancer(7, [0.0, 0.0, 10.0, 10.0], 1000, SORTIE_LIGNE, true));
        assert!(f.en_cours(1000) && f.en_cours(1000 + SORTIE_LIGNE_MS - 1));
        let debut = f.iter().next().unwrap().decalage(1000).unwrap();
        assert_eq!((debut.opacite, debut.echelle, debut.dy), (1.0, 1.0, 0.0), "départ : la ligne est entière");
        let milieu = f.iter().next().unwrap().decalage(1000 + SORTIE_LIGNE_MS / 2).unwrap();
        assert!(milieu.opacite < 1.0 && milieu.opacite > 0.0 && milieu.echelle < 1.0 && milieu.echelle > 0.92, "à mi-course : {milieu:?}");
        // Fin : plus de décalage, le fantôme est supprimé par `purger`
        assert!(f.iter().next().unwrap().decalage(1000 + SORTIE_LIGNE_MS).is_none());
        assert!(!f.en_cours(1000 + SORTIE_LIGNE_MS));
        assert_eq!(f.len(), 1, "encore en mémoire tant qu'on ne purge pas");
        f.purger(1000 + SORTIE_LIGNE_MS);
        assert_eq!(f.len(), 0, "le fantôme fini est supprimé");
    }

    #[test]
    fn fantomes_plafond_et_mouvement_reduit() {
        let mut f: Fantomes<usize> = Fantomes::nouveau();
        for i in 0..FANTOMES_MAX {
            assert!(f.lancer(i, [0.0; 4], 0, SORTIE_LIGNE, true));
        }
        assert!(!f.lancer(99, [0.0; 4], 0, SORTIE_LIGNE, true), "au-delà du plafond, la ligne disparaît d'un coup");
        assert_eq!(f.len(), FANTOMES_MAX);
        // Mouvement réduit : aucune sortie jouée
        let mut g: Fantomes<usize> = Fantomes::nouveau();
        assert!(!g.lancer(1, [0.0; 4], 0, SORTIE_LIGNE, false));
        assert_eq!((g.len(), g.en_cours(0)), (0, false));
    }

    #[test]
    fn durees_et_valeurs_des_sorties() {
        assert_eq!((SORTIE_LIGNE.duree, SORTIE_CARTE.duree), (220, 240));
        let mut f: Fantomes<()> = Fantomes::nouveau();
        f.lancer((), [0.0; 4], 0, SORTIE_CARTE, true);
        let g = f.iter().next().unwrap();
        assert!(g.decalage(SORTIE_CARTE_MS - 1).is_some() && g.decalage(SORTIE_CARTE_MS).is_none());
        let fin = g.decalage(SORTIE_CARTE_MS - 1).unwrap();
        assert!(fin.opacite < 0.02 && (fin.echelle - 0.92).abs() < 0.01 && (fin.dy - 20.0).abs() < 0.5, "la carte finit à 20 DIP plus bas, .92, transparente : {fin:?}");
        // `ease-in` : part doucement (plus de la moitié de l'opacité reste à mi-durée)
        let mi = g.decalage(SORTIE_CARTE_MS / 2).unwrap();
        assert!(mi.opacite > 0.5, "ease-in part doucement : {mi:?}");
    }

    #[test]
    fn carte_nouvelle_entre_les_voisines_glissent_la_retiree_sort() {
        let mut c = Cartes::nouveau();
        let (d1, p1) = dispo_cartes(&["COPIE A", "COPIE B"]);
        // Première image : ce qui est déjà là ne s'anime pas
        assert!(c.image(&d1, &p1, 1000, true).iter().all(|d| d.est_aucun()));
        // Une troisième carte arrive : elle entre (translateY 30, échelle .9, transparente), les deux autres se recentrent en glissant
        let (d2, p2) = dispo_cartes(&["COPIE A", "COPIE B", "COPIE C"]);
        let dec = c.image(&d2, &p2, 1100, true);
        assert_eq!(dec.len(), 3);
        assert!(dec[0].dx.abs() > 0.5 && dec[1].dx.abs() > 0.5, "les voisines glissent : {dec:?}");
        assert_eq!((dec[2].dy, dec[2].echelle, dec[2].opacite), (30.0, 0.9, 0.0), "l'arrivante part de 30 DIP plus bas");
        assert!(c.en_cours(1100) && c.en_cours(1100 + CARTE_VOISINES_MS - 1));
        // Fin des durées : plus rien ne bouge
        let fin = c.image(&d2, &p2, 1100 + CARTE_ENTREE_MS, true);
        assert!(fin.iter().all(|d| d.est_aucun()));
        assert!(!c.en_cours(1100 + CARTE_ENTREE_MS));
        // La carte B disparaît : elle devient un fantôme qui joue 240 ms puis s'efface ; C prend sa place en glissant
        let (d3, _) = dispo_cartes(&["COPIE A", "COPIE C"]);
        let mut p3: Vec<Pastille> = vec![p2[0].clone(), p2[2].clone()];
        // Les places de d3 portent les clés 1 et 2 : on aligne les clés sur celles des pastilles gardées
        p3[1] = p3[1].clone().avec_cle(3);
        let mut d3 = d3;
        d3.places[1].cle = 3;
        let apres = c.image(&d3, &p3, 2000, true);
        assert_eq!(c.fantomes.len(), 1);
        assert!(c.fantomes.en_cours(2000) && !c.fantomes.en_cours(2000 + SORTIE_CARTE_MS));
        assert_eq!(apres.len(), 2);
        c.image(&d3, &p3, 2000 + CARTE_VOISINES_MS, true);
        assert_eq!(c.fantomes.len(), 0, "le fantôme fini est purgé");
        assert!(!c.en_cours(2000 + CARTE_VOISINES_MS));
    }

    #[test]
    fn cartes_mouvement_reduit_rien_ne_bouge() {
        let mut c = Cartes::nouveau();
        let (d1, p1) = dispo_cartes(&["COPIE A"]);
        c.image(&d1, &p1, 0, false);
        let (d2, p2) = dispo_cartes(&["COPIE A", "COPIE B"]);
        assert!(c.image(&d2, &p2, 10, false).iter().all(|d| d.est_aucun()));
        assert!(!c.en_cours(10));
        let (d3, p3) = dispo_cartes(&[]);
        c.image(&d3, &p3, 20, false);
        assert_eq!(c.fantomes.len(), 0, "aucune sortie en mouvement réduit");
        // Déploiement et jauge : l'état tout de suite
        assert_eq!(c.deploiements(&p2, Some(1), 30, false), vec![(0.0, 0.0), (1.0, 1.0)]);
        assert_eq!(c.jauge(&p2[0], 6, 30, false)[5..7], [1.0, 0.0]);
        assert!(!c.en_cours(30));
    }

    #[test]
    fn deploiement_et_jauge_au_rythme_de_la_maquette() {
        let mut c = Cartes::nouveau();
        let (_, p) = dispo_cartes(&["COPIE A"]);
        c.debut_image();
        assert_eq!(c.deploiements(&p, None, 0, true), vec![(0.0, 0.0)], "repliée à la première image");
        c.debut_image();
        let v = c.deploiements(&p, Some(0), 100, true);
        assert!(v[0].0 < 0.2 && v[0].1 < 0.2, "le `.more` commence à peine : {v:?}");
        assert!(c.en_cours(100));
        c.debut_image();
        let fin = c.deploiements(&p, Some(0), 100 + DEPLOIEMENT_MS, true);
        assert_eq!(fin[0], (1.0, 1.0));
        assert!(!c.en_cours(100 + DEPLOIEMENT_MS));
        // Jauge : une barre qui s'allume prend 200 ms
        let j0 = c.jauge(&p[0], 3, 1000, true);
        assert_eq!((j0[2], j0[3]), (1.0, 0.0));
        assert_eq!(c.jauge(&p[0], 4, 1050, true)[3], 0.0, "la barre part de l'éteint");
        let j1 = c.jauge(&p[0], 4, 1100, true);
        assert!(j1[3] > 0.0 && j1[3] < 1.0, "la barre s'allume progressivement : {}", j1[3]);
        assert!(c.en_cours(1100));
        assert_eq!(c.jauge(&p[0], 4, 1050 + BARRE_JAUGE_MS, true)[3], 1.0);
        c.fin_image();
    }

    #[test]
    fn cadre_qui_se_deploie_puis_se_replie() {
        let mut m = Morph::nouveau();
        let (source, cible) = ([10.0, 20.0, 100.0, 30.0], [200.0, 100.0, 540.0, 280.0]);
        assert!(!m.maj(false, source, cible, 0, true).visible);
        assert!(!m.en_cours(0));
        let e = m.maj(true, source, cible, 1000, true);
        assert_eq!(e.cadre, Some(source), "le cadre part de la ligne");
        assert_eq!(e.contenu, 0.0, "le contenu attend la fin du dépliage");
        assert!(m.en_cours(1000));
        let mi = m.maj(true, source, cible, 1000 + MORPH_OUVERTURE_MS / 2, true);
        assert!(mi.cadre.unwrap()[2] > 100.0 && mi.contenu == 0.0);
        // Le contenu paraît pendant 180 ms
        let c = m.maj(true, source, cible, 1000 + MORPH_OUVERTURE_MS + MORPH_CONTENU_MS / 2, true);
        assert!(c.cadre.is_none() && c.contenu > 0.0 && c.contenu < 1.0, "{c:?}");
        let fin = m.maj(true, source, cible, 1000 + MORPH_OUVERTURE_MS + MORPH_CONTENU_MS, true);
        assert_eq!((fin.cadre, fin.contenu), (None, 1.0));
        assert!(!m.en_cours(1000 + MORPH_OUVERTURE_MS + MORPH_CONTENU_MS));
        // Fermeture : le cadre revient en 240 ms
        let f0 = m.maj(false, source, cible, 3000, true);
        assert!(f0.visible && f0.cadre.is_some());
        assert!(m.en_cours(3000));
        let f1 = m.maj(false, source, cible, 3000 + MORPH_FERMETURE_MS, true);
        assert!(!f1.visible && f1.cadre.is_none());
        assert!(!m.en_cours(3000 + MORPH_FERMETURE_MS));
    }

    #[test]
    fn cadre_en_mouvement_reduit_instantane() {
        let mut m = Morph::nouveau();
        let (source, cible) = ([0.0; 4], [1.0, 2.0, 3.0, 4.0]);
        let e = m.maj(true, source, cible, 0, false);
        assert_eq!((e.cadre, e.contenu, e.voile, e.visible), (None, 1.0, 1.0, true));
        assert!(!m.en_cours(0));
        let f = m.maj(false, source, cible, 10, false);
        assert!(!f.visible, "se ferme d'un coup");
    }


    #[test]
    fn pilule_de_filtre_s_ouvre_en_450_ms_et_se_fond_en_200() {
        let mut m = Mouvements::nouveau();
        // Fermée : largeur 0 et invisible, rien ne bouge
        assert_eq!(m.pilule(0, false, 0, true), (0.0, 0.0));
        assert!(!m.en_cours(0));
        // Ctrl+F : à l'instant du clic la largeur est encore 0, puis elle grandit au ressort
        assert_eq!(m.pilule(0, true, 1000, true), (0.0, 0.0));
        assert!(m.en_cours(1000));
        let (l, o) = m.pilule(0, true, 1100, true);
        assert!(l > 0.0 && l < 1.5 && o > 0.0 && o < 1.0, "{l} {o}");
        // L'opacité a fini à 200 ms, la largeur à 450 ms
        let (_, o) = m.pilule(0, true, 1000 + PILULE_FONDU_MS, true);
        assert_eq!(o, 1.0);
        assert!(m.en_cours(1000 + PILULE_FONDU_MS));
        assert_eq!(m.pilule(0, true, 1000 + PILULE_MS, true), (1.0, 1.0));
        assert!(!m.en_cours(1000 + PILULE_MS));
        // Échap : sens inverse, même durée ; jamais de largeur négative malgré le ressort
        let (l, _) = m.pilule(0, false, 2000 + 100, true);
        assert!((0.0..=1.5).contains(&l));
        assert_eq!(m.pilule(0, false, 2000 + PILULE_MS, true), (0.0, 0.0));
        // Le second panneau a sa propre pilule
        assert_eq!(m.pilule(1, false, 3000, true), (0.0, 0.0));
    }

    #[test]
    fn pilule_de_filtre_sans_animation_en_mouvement_reduit() {
        let mut m = Mouvements::nouveau();
        assert_eq!(m.pilule(0, false, 0, false), (0.0, 0.0));
        assert_eq!(m.pilule(0, true, 100, false), (1.0, 1.0), "ouverte d'un coup");
        assert!(!m.en_cours(100));
        assert_eq!(m.pilule(0, false, 200, false), (0.0, 0.0), "fermée d'un coup");
        assert!(!m.en_cours(200));
    }

    #[test]
    fn tiroir_de_l_historique_glisse_en_500_ms_au_ressort() {
        let mut m = Mouvements::nouveau();
        // Fermé d'emblée : rien ne bouge, rien n'est visible
        assert_eq!(m.tiroir(false, 0, true), 0.0);
        assert!(!m.en_cours(0) && !m.tiroir_visible(0));
        // Ctrl+Maj+H : parti de 0, il sort en 500 ms avec un léger dépassement
        assert_eq!(m.tiroir(true, 1000, true), 0.0);
        assert!(m.en_cours(1000) && m.tiroir_visible(1000));
        let milieu = m.tiroir(true, 1250, true);
        assert!(milieu > 0.0, "{milieu}");
        assert!((0..=TIROIR_MS).any(|e| m.tiroir(true, 1000 + e, true) > 1.0), "le ressort dépasse avant de se poser");
        assert_eq!(m.tiroir(true, 1000 + TIROIR_MS, true), 1.0);
        assert!(!m.en_cours(1000 + TIROIR_MS) && m.tiroir_visible(1000 + TIROIR_MS));
        // Il rentre en sens inverse : encore visible pendant le glissement, plus ensuite
        m.tiroir(false, 2000, true);
        assert!(m.en_cours(2100) && m.tiroir_visible(2100));
        assert_eq!(m.tiroir(false, 2000 + TIROIR_MS, true), 0.0);
        assert!(!m.en_cours(2000 + TIROIR_MS) && !m.tiroir_visible(2000 + TIROIR_MS));
    }

    #[test]
    fn tiroir_de_l_historique_pose_en_mouvement_reduit() {
        let mut m = Mouvements::nouveau();
        assert_eq!(m.tiroir(false, 0, false), 0.0);
        assert_eq!(m.tiroir(true, 100, false), 1.0, "sorti d'un coup");
        assert!(!m.en_cours(100));
        assert_eq!(m.tiroir(false, 200, false), 0.0, "rentré d'un coup");
        assert!(!m.en_cours(200) && !m.tiroir_visible(200));
    }

    #[test]
    fn volet_et_apercu_glissent_en_500_ms_et_se_fondent_en_250() {
        let mut m = Mouvements::nouveau();
        // Première image : posés à leur largeur, sans glisser
        let v = m.volets(268.0, 288.0, 0, true);
        assert_eq!((v.volet, v.apercu, v.fondu_volet, v.glisse), (268.0, 288.0, 1.0, false));
        // F9 : le volet se replie en 500 ms, son contenu se fond en 250 ms
        let a = m.volets(0.0, 288.0, 1000, true);
        assert!(a.glisse && a.volet > 200.0);
        let debut = m.volets(0.0, 288.0, 1080, true);
        assert!(debut.glisse && debut.volet > 0.0 && debut.volet < 268.0 && debut.fondu_volet > 0.0 && debut.fondu_volet < 1.0, "{debut:?}");
        let mi = m.volets(0.0, 288.0, 1000 + VOLET_FONDU_MS, true);
        assert!(mi.fondu_volet < 0.01 && mi.glisse, "le contenu a fini de se fondre à 250 ms : {mi:?}");
        assert!(m.en_cours(1000 + VOLET_MS - 1));
        let fin = m.volets(0.0, 288.0, 1000 + VOLET_MS, true);
        assert_eq!((fin.volet, fin.glisse), (0.0, false));
        assert!(!m.en_cours(1000 + VOLET_MS));
        // Mouvement réduit : la largeur change d'un coup
        let r = m.volets(268.0, 0.0, 5000, false);
        assert_eq!((r.volet, r.apercu, r.glisse), (268.0, 0.0, false));
        assert!(!m.en_cours(5000));
    }

    #[test]
    fn ligne_supprimee_devient_fantome_et_les_suivantes_glissent() {
        // Un seul test touche la liste globale des suppressions attendues (les tests tournent en parallèle)
        oublier_sorties();
        let mut m = Mouvements::nouveau();
        let e = dossier_a();
        let visibles = |noms: &[&str]| -> Vec<(String, (f32, f32))> { noms.iter().enumerate().map(|(i, k)| (k.to_string(), (10.0, i as f32 * 30.0))).collect() };
        let avant = visibles(&["a", "b", "c"]);
        assert!(!m.lignes_retirees(0, &e, &avant, 0, 600.0, 30.0, 0.0, 1000, true));
        m.flips[0].image(&e, 30, 0, &avant, 1000, true);
        // Sans suppression annoncée, une ligne qui part (filtre, défilement) ne fait pas de fantôme
        let apres = visibles(&["a", "c"]);
        assert!(!m.lignes_retirees(0, &e, &apres, 0, 600.0, 30.0, 0.0, 1100, true));
        assert_eq!(m.fantomes_lignes[0].len(), 0);
        // Suppression annoncée de « c » (encore à l'écran à 60), puis disparue
        annoncer_sorties(vec![("c".into(), LigneSortante { entree: entree_test("c"), choisie: true, vue: 0 })]);
        m.flips[0].image(&e, 30, 0, &avant, 1200, true);
        // Mouvement réduit : aucun fantôme
        assert!(!m.lignes_retirees(0, &e, &visibles(&["a", "b"]), 0, 600.0, 30.0, 0.0, 1300, false));
        assert_eq!(m.fantomes_lignes[0].len(), 0);
        // Mouvement complet : le fantôme part de la place où « c » était
        assert!(m.lignes_retirees(0, &e, &visibles(&["a", "b"]), 0, 600.0, 30.0, 50.0, 1300, true));
        assert_eq!(m.fantomes_lignes[0].len(), 1);
        let g = m.fantomes_lignes[0].iter().next().unwrap();
        assert_eq!(g.rect, [10.0, 60.0 + 50.0, 600.0, 30.0], "place affichée + défilement");
        assert!(m.fantomes_lignes[0].en_cours(1300 + SORTIE_LIGNE_MS - 1));
        // Les lignes qui suivent glissent : un glissement est forcé même sans changement d'ordre
        let suite: Vec<(String, (f32, f32))> = vec![("a".into(), (10.0, 0.0)), ("b".into(), (10.0, 0.0))];
        let d = m.flips[0].image(&e, 30, 0, &suite, 1300, true);
        assert!(d[1].dy > 0.0, "« b » remonte depuis sa place d'avant : {d:?}");
        // Plus de suppression à jouer pour cette clé : un second passage ne recrée rien
        assert!(!m.lignes_retirees(0, &e, &suite, 0, 600.0, 30.0, 0.0, 1400, true));
        assert_eq!(m.fantomes_lignes[0].len(), 1);
        // Après 220 ms le fantôme est purgé
        m.lignes_retirees(0, &e, &suite, 0, 600.0, 30.0, 0.0, 1300 + SORTIE_LIGNE_MS + 5, true);
        assert_eq!(m.fantomes_lignes[0].len(), 0);
        oublier_sorties();
    }

    #[test]
    fn plafond_de_400_entrees_pas_de_fantome() {
        let mut m = Mouvements::nouveau();
        let e = dossier_a();
        let beaucoup: Vec<(String, (f32, f32))> = (0..=FLIP_MAX).map(|i| (format!("f{i}"), (0.0, i as f32 * 30.0))).collect();
        m.flips[0].image(&e, 30, 0, &beaucoup[..1], 0, true);
        assert!(!m.lignes_retirees(0, &e, &beaucoup, 0, 600.0, 30.0, 0.0, 10, true), "plus de 400 entrées visibles : mise à jour directe");
        assert_eq!(m.fantomes_lignes[0].len(), 0);
    }

    #[test]
    fn mise_en_page_anime_extremes_identiques_et_milieu_interpole() {
        let m = |t: &str| t.chars().count() as f32 * 8.0;
        let p = vec![Pastille::operation("COPIE · 3 ÉLÉMENTS".into(), 0.4, false, Some(r"C:\Users\n\Documents".into())).avec_cle(1)];
        let ferme = crate::pastilles::mettre_en_page(1600.0, 220.0, 700.0, &p, None, 60.0, 40.0, &m);
        let ouvert = crate::pastilles::mettre_en_page(1600.0, 220.0, 700.0, &p, Some(0), 60.0, 40.0, &m);
        assert_eq!(crate::pastilles::mettre_en_page_anime(1600.0, 220.0, 700.0, &p, &[0.0], 60.0, 40.0, &m), ferme);
        assert_eq!(crate::pastilles::mettre_en_page_anime(1600.0, 220.0, 700.0, &p, &[1.0], 60.0, 40.0, &m), ouvert);
        let milieu = crate::pastilles::mettre_en_page_anime(1600.0, 220.0, 700.0, &p, &[0.5], 60.0, 40.0, &m);
        let (l0, l1, lm) = (ferme.places[0].zone.l, ouvert.places[0].zone.l, milieu.places[0].zone.l);
        assert!(l1 > l0 && lm > l0 && lm < l1, "la largeur est interpolée : {l0} < {lm} < {l1}");
        assert_eq!(milieu.places[0].pleine, l1, "l'intérieur est celui de la carte déployée, rogné à la zone");
        assert!(!milieu.places[0].boutons.is_empty());
    }

    #[test]
    fn cle_de_carte_stable_et_distincte() {
        let a = Pastille::operation("COPIE · 3 ÉLÉMENTS".into(), 0.1, false, None);
        let b = Pastille::operation("COPIE · 3 ÉLÉMENTS".into(), 0.9, false, None);
        assert_eq!(cle_carte(&a), cle_carte(&b), "l'avancement ne change pas l'identité");
        assert_ne!(cle_carte(&a), cle_carte(&Pastille::operation("DEPLACEMENT".into(), 0.1, false, None)));
        assert_eq!(cle_carte(&a.clone().avec_cle(5)), 5);
    }

    #[test]
    fn palette_se_referme_en_260_ms_et_pas_en_mouvement_reduit() {
        let mut m = Mouvements::nouveau();
        assert_eq!(m.palette_fermee(true, 0, true), None, "ouverte : rien à finir");
        assert_eq!(m.palette_fermee(false, 1000, true), Some((1.0, 0.0)), "départ : entière, à sa place");
        let (revele, dy) = m.palette_fermee(false, 1130, true).unwrap();
        assert!(revele > 0.0 && revele < 1.0 && dy < 0.0 && dy > -10.0, "à mi-course : {revele} {dy}");
        assert!(m.en_cours(1130));
        assert_eq!(m.palette_fermee(false, 1000 + PALETTE_FERMETURE_MS, true), None);
        assert!(!m.en_cours(1000 + PALETTE_FERMETURE_MS), "plus de minuterie une fois fermée");
        // Mouvement réduit : elle disparaît d'un coup
        m.palette_fermee(true, 2000, false);
        assert_eq!(m.palette_fermee(false, 2010, false), None);
        assert!(!m.en_cours(2010));
    }

    #[test]
    fn courbes_des_sorties_bornees_et_croissantes() {
        for f in [ease_in_css, ease_standard, ease_palette_fermeture] {
            assert_eq!((f(0.0), f(1.0)), (0.0, 1.0));
            let mut avant = 0.0;
            for k in 0..=100 {
                let v = f(k as f32 / 100.0);
                assert!(v >= avant - 1e-5 && (0.0..=1.0 + 1e-5).contains(&v));
                avant = v;
            }
        }
        // `ease-in` part plus doucement que `ease`
        assert!(ease_in_css(0.3) < ease(0.3));
    }
    // ── Lot 13 : yeux de Pixel ──

    #[test]
    fn yeux_regard_glisse_en_350_ms_et_s_arrete() {
        let mut y = Yeux::nouveau();
        let v0 = y.maj((0.0, 0.0), false, Teinte::Cyan, 1000, true);
        assert_eq!((v0.dx, v0.dy, v0.aplatie, v0.clignement), (0.0, 0.0, 0.0, 0.0));
        assert!(!y.en_cours(1000), "premier état posé sans glisser");
        // Le pointeur passe à gauche : cible -3, le regard glisse
        y.maj((-3.0, 0.0), false, Teinte::Cyan, 1100, true);
        assert!(y.en_cours(1100));
        let milieu = y.maj((-3.0, 0.0), false, Teinte::Cyan, 1275, true);
        assert!(milieu.dx < 0.0 && milieu.dx != -3.0, "à mi-course : {}", milieu.dx);
        // Une fois la durée écoulée : sur la cible et plus rien en cours
        let fin = y.maj((-3.0, 0.0), false, Teinte::Cyan, 1100 + YEUX_REGARD_MS, true);
        assert_eq!(fin.dx, -3.0);
        assert!(!y.en_cours(1100 + YEUX_REGARD_MS), "aucune animation en cours une fois finie");
        // Puis à droite : repart de la valeur du moment
        y.maj((3.0, 0.0), false, Teinte::Cyan, 2000, true);
        assert!(y.en_cours(2000));
        assert_eq!(y.maj((3.0, 0.0), false, Teinte::Cyan, 2000 + YEUX_REGARD_MS, true).dx, 3.0);
    }

    #[test]
    fn yeux_mouvement_reduit_tout_est_pose_d_un_coup() {
        let mut y = Yeux::nouveau();
        y.maj((0.0, 0.0), false, Teinte::Cyan, 0, false);
        y.cligner(10);
        let v = y.maj((-3.0, 4.0), true, Teinte::Ambre, 20, false);
        assert_eq!((v.dx, v.dy), (-3.0, 4.0));
        assert_eq!(v.aplatie, 1.0, "nuit immédiate");
        assert_eq!(v.clignement, 0.0, "clignement coupé");
        assert_eq!((v.teinte, v.fondu), (Teinte::Ambre, 1.0), "couleur immédiate");
        assert!(!y.en_cours(20));
    }

    #[test]
    fn yeux_clignement_dure_630_ms_puis_plus_rien() {
        let mut y = Yeux::nouveau();
        y.maj((0.0, 0.0), false, Teinte::Cyan, 0, true);
        y.cligner(5000);
        assert!(y.en_cours(5000));
        assert_eq!(y.maj((0.0, 0.0), false, Teinte::Cyan, 5000, true).clignement, 0.0, "début : ouverts");
        assert!((y.maj((0.0, 0.0), false, Teinte::Cyan, 5000 + crate::pixel::CLIGNEMENT_FERME_MS, true).clignement - 1.0).abs() < 1e-3);
        let apres = 5000 + crate::pixel::CLIGNEMENT_MS;
        assert!(y.en_cours(apres - 1));
        assert_eq!(y.maj((0.0, 0.0), false, Teinte::Cyan, apres, true).clignement, 0.0);
        assert!(!y.en_cours(apres), "aucune animation en cours une fois finie");
        // Un second clignement pendant le premier ne le relance pas
        y.cligner(9000);
        y.cligner(9100);
        assert_eq!(y.maj((0.0, 0.0), false, Teinte::Cyan, 9100, true).clignement, crate::pixel::phase_clignement(100));
    }

    #[test]
    fn yeux_nuit_en_250_ms_et_couleur_en_200_ms() {
        let mut y = Yeux::nouveau();
        y.maj((0.0, 0.0), false, Teinte::Cyan, 0, true);
        let v = y.maj((0.0, 0.0), true, Teinte::Faible, 1000, true);
        assert_eq!((v.aplatie, v.teinte_avant, v.teinte, v.fondu), (0.0, Teinte::Cyan, Teinte::Faible, 0.0));
        let milieu = y.maj((0.0, 0.0), true, Teinte::Faible, 1100, true);
        assert!(milieu.aplatie > 0.0 && milieu.aplatie < 1.0 && milieu.fondu > 0.0 && milieu.fondu < 1.0);
        // Couleur finie à 200 ms, hauteur à 250 ms
        assert!(y.en_cours(1210) && y.maj((0.0, 0.0), true, Teinte::Faible, 1210, true).fondu == 1.0);
        let fin = y.maj((0.0, 0.0), true, Teinte::Faible, 1000 + YEUX_HAUTEUR_MS, true);
        assert_eq!(fin.aplatie, 1.0);
        assert!(!y.en_cours(1000 + YEUX_HAUTEUR_MS));
    }

    #[test]
    fn yeux_comptent_dans_les_mouvements_en_cours() {
        let mut m = Mouvements::nouveau();
        m.yeux.maj((0.0, 0.0), false, Teinte::Cyan, 0, true);
        assert!(!m.en_cours(10));
        m.yeux.maj((3.0, 0.0), false, Teinte::Cyan, 100, true);
        assert!(m.en_cours(150), "le regard fait tourner la minuterie d'animation");
        assert!(!m.en_cours(100 + YEUX_REGARD_MS), "et elle s'arrête");
    }

    #[test]
    fn yeux_seuls_peignent_au_plus_30_images_par_seconde() {
        let mut derniere = 0u64;
        let mut peintes = 0;
        // Un signal d'écran toutes les 4 ms (237 Hz) pendant un clignement de 630 ms
        for t in (1000..1000 + crate::pixel::CLIGNEMENT_MS).step_by(4) {
            if peindre_cette_image(true, &mut derniere, t) {
                peintes += 1;
            }
        }
        assert!(peintes <= 21, "{peintes} images pour 630 ms");
        assert!(peintes >= 15, "{peintes} images : il en faut assez pour que le clignement se voie");
        // Une autre animation en cours : toutes les images passent
        let mut d = 0u64;
        assert!((0..100u64).all(|k| peindre_cette_image(false, &mut d, 2000 + k)));
        // Le mouvement des yeux seuls ne rend pas `en_cours_hors_yeux` vrai
        let mut m = Mouvements::nouveau();
        m.yeux.maj((0.0, 0.0), false, Teinte::Cyan, 0, true);
        m.yeux.cligner(100);
        assert!(m.en_cours(150) && !m.en_cours_hors_yeux(150));
    }

    // ── Lot 15A : palette ──

    fn entree_palette(lettres: usize, categorie: usize, cle_liste: u64, pastille: Option<(f32, f32)>, choix: usize, cle_commande: u64) -> EntreePalette {
        EntreePalette { lettres, categorie, cle_liste, pastille, choix, affiches: 0..8, cle_commande }
    }

    #[test]
    fn jauge_allume_deux_traits_par_lettre_dans_la_limite_de_40() {
        assert_eq!(jauge_allumes(0), 0);
        assert_eq!(jauge_allumes(1), 2);
        assert_eq!(jauge_allumes(7), 14);
        assert_eq!(jauge_allumes(20), 40);
        assert_eq!(jauge_allumes(500), 40, "jamais plus que les 40 traits");
    }

    #[test]
    fn jauge_s_allume_en_150_ms_et_s_agrandit_en_250_ms() {
        let mut a = AnimPalette::nouveau();
        a.maj(&entree_palette(0, 0, 1, None, 0, 1), 1000, true);
        assert_eq!(a.jauge(0, 1000), (0.0, 1.0), "éteinte : couleur de repos, échelle 1");
        a.maj(&entree_palette(3, 0, 2, None, 0, 1), 2000, true);
        // 6 traits allumés : les 6 premiers bougent, le septième non
        assert_eq!(a.jauge(0, 2000), (0.0, 1.0), "au départ du glissement");
        let (k, e) = a.jauge(5, 2075);
        assert!(k > 0.0 && k < 1.0 && e > 1.0 && e < 2.0, "à mi-couleur : {k} {e}");
        assert_eq!(a.jauge(5, 2000 + JAUGE_COULEUR_MS).0, 1.0, "couleur finie à 150 ms");
        assert!(a.en_cours(2000 + JAUGE_COULEUR_MS), "l'échelle (ressort, 250 ms) continue");
        let (k, e) = a.jauge(5, 2000 + JAUGE_ECHELLE_MS);
        assert_eq!((k, e), (1.0, 2.0), "scaleY(2) à l'arrivée");
        assert!(!a.en_cours(2000 + JAUGE_ECHELLE_MS));
        assert_eq!(a.jauge(6, 2000 + JAUGE_ECHELLE_MS), (0.0, 1.0), "le septième trait reste éteint");
    }

    #[test]
    fn palette_en_mouvement_reduit_pose_l_etat_final_tout_de_suite() {
        let mut a = AnimPalette::nouveau();
        a.maj(&entree_palette(4, 2, 1, Some((34.0, 46.0)), 1, 7), 500, false);
        assert_eq!(a.jauge(7, 500), (1.0, 2.0));
        assert_eq!(a.jauge(8, 500), (0.0, 1.0));
        assert_eq!(a.categorie(2, 500), 1.0);
        assert_eq!(a.categorie(0, 500), 0.0);
        assert_eq!(a.pastille(500), (34.0, 46.0));
        assert_eq!(a.puce(1, 500), 1.0);
        assert_eq!(a.puce(0, 500), 0.0);
        assert_eq!(a.vin(500), 1.0, "l'aperçu apparaît tout fait");
        assert!(!a.en_cours(500));
        // Changements suivants : toujours d'un coup
        a.maj(&entree_palette(1, 3, 2, Some((126.0, 46.0)), 3, 8), 600, false);
        assert_eq!((a.jauge(7, 600), a.categorie(3, 600), a.pastille(600), a.vin(600)), ((0.0, 1.0), 1.0, (126.0, 46.0), 1.0));
        assert!(!a.en_cours(600));
    }

    #[test]
    fn pastille_de_la_palette_glisse_sauf_liste_reconstruite() {
        let mut a = AnimPalette::nouveau();
        a.maj(&entree_palette(0, 0, 1, Some((34.0, 46.0)), 0, 1), 1000, true);
        assert_eq!(a.pastille(1000), (34.0, 46.0), "première image : posée sans glisser");
        // Flèche bas, même liste : top 300 ms au ressort, hauteur 200 ms
        a.maj(&entree_palette(0, 0, 1, Some((80.0, 46.0)), 1, 2), 2000, true);
        let (haut, _) = a.pastille(2100);
        assert!(haut > 34.0 && haut < 80.0 + 12.0, "en route : {haut}");
        assert_eq!(a.pastille(2000 + PASTILLE_PALETTE_HAUT_MS).0, 80.0);
        assert!(a.en_cours(2100) && !a.en_cours(2000 + VIN_MS), "ressort de 300 ms, apparition de 350 ms");
        // Autre requête ou autre catégorie (clé de liste différente) : saute (`palSel(pi, true)`), comme `instant`
        a.maj(&entree_palette(1, 0, 2, Some((172.0, 46.0)), 3, 3), 3000, true);
        assert_eq!(a.pastille(3000), (172.0, 46.0));
        assert!(!a.en_cours(3000 + VIN_MS), "le vin et le reste ont fini");
    }

    #[test]
    fn puces_de_categorie_et_d_icone_passent_a_l_accent_en_200_ms() {
        let mut a = AnimPalette::nouveau();
        a.maj(&entree_palette(0, 0, 1, Some((34.0, 46.0)), 0, 1), 1000, true);
        assert_eq!((a.categorie(0, 1000), a.categorie(1, 1000)), (1.0, 0.0));
        a.maj(&entree_palette(0, 1, 1, Some((80.0, 46.0)), 1, 2), 2000, true);
        let (sortante, entrante) = (a.categorie(0, 2100), a.categorie(1, 2100));
        assert!(sortante > 0.0 && sortante < 1.0 && entrante > 0.0 && entrante < 1.0, "{sortante} {entrante}");
        assert_eq!((a.categorie(0, 2000 + PUCE_MS), a.categorie(1, 2000 + PUCE_MS)), (0.0, 1.0));
        // Puce d'icône : l'ancienne ligne s'éteint, la nouvelle s'allume, en 200 ms
        assert!(a.puce(0, 2100) > 0.0 && a.puce(0, 2100) < 1.0 && a.puce(1, 2100) > 0.0 && a.puce(1, 2100) < 1.0);
        assert_eq!((a.puce(0, 2000 + PUCE_MS), a.puce(1, 2000 + PUCE_MS)), (0.0, 1.0));
        // Une ligne qui n'est plus affichée est oubliée
        let mut e = entree_palette(0, 1, 1, Some((80.0, 46.0)), 1, 2);
        e.affiches = 1..3;
        a.maj(&e, 3000, true);
        assert_eq!(a.puce(0, 3000), 0.0);
    }

    #[test]
    fn apercu_vin_rejoue_a_chaque_changement_de_commande() {
        let mut a = AnimPalette::nouveau();
        a.maj(&entree_palette(0, 0, 1, None, 0, 10), 1000, true);
        assert_eq!(a.vin(1000), 0.0, "il part de zéro à l'ouverture");
        assert!(a.vin(1175) > 0.3, "le ressort monte vite");
        assert_eq!(a.vin(1000 + VIN_MS), 1.0);
        // Même commande : rien ne rejoue
        a.maj(&entree_palette(0, 0, 1, None, 0, 10), 5000, true);
        assert_eq!(a.vin(5000), 1.0);
        // Autre commande : repart de zéro
        a.maj(&entree_palette(0, 0, 1, None, 1, 11), 6000, true);
        assert_eq!(a.vin(6000), 0.0);
        assert!(a.en_cours(6100) && !a.en_cours(6000 + VIN_MS));
    }

    #[test]
    fn glissements_de_la_palette_repartent_de_zero_a_la_fermeture() {
        let mut m = Mouvements::nouveau();
        m.palette_fermee(true, 0, true);
        m.palette.maj(&entree_palette(5, 1, 1, Some((34.0, 46.0)), 0, 1), 10, true);
        m.palette.maj(&entree_palette(6, 1, 1, Some((34.0, 46.0)), 0, 1), 20, true);
        assert!(m.en_cours_hors_yeux(30), "la jauge fait tourner la minuterie d'animation");
        // Fermée en mouvement réduit : aussitôt remise à zéro
        m.palette_fermee(false, 100, false);
        assert!(!m.en_cours(100), "palette fermée : plus rien ne tourne");
        // Rouverte : la première image pose tout sans glisser
        m.palette_fermee(true, 5000, true);
        m.palette.maj(&entree_palette(4, 0, 9, Some((34.0, 46.0)), 0, 2), 5000, true);
        assert_eq!(m.palette.jauge(2, 5000), (1.0, 2.0));
        assert_eq!(m.palette.pastille(5000), (34.0, 46.0));
    }

    #[test]
    fn yeux_contents_en_250_ms_et_clignement_de_la_palette_en_280_ms() {
        let mut y = Yeux::nouveau();
        y.contente(false, 0, true);
        y.maj((0.0, 0.0), false, Teinte::Cyan, 0, true);
        y.contente(true, 1000, true);
        assert_eq!(y.maj((0.0, 0.0), false, Teinte::Ambre, 1000, true).contents, 0.0);
        let milieu = y.maj((0.0, 0.0), false, Teinte::Ambre, 1125, true).contents;
        assert!(milieu > 0.0 && milieu < 1.0, "{milieu}");
        assert_eq!(y.maj((0.0, 0.0), false, Teinte::Ambre, 1000 + YEUX_HAUTEUR_MS, true).contents, 1.0);
        // Mouvement réduit : contents tout de suite
        let mut r = Yeux::nouveau();
        r.contente(true, 0, false);
        assert_eq!(r.maj((0.0, 0.0), false, Teinte::Ambre, 0, false).contents, 1.0);
        // Clignement court : creux à 140 ms, fini à 280 ms ; le suivant repart de zéro même en cours
        let mut c = Yeux::nouveau();
        c.maj((0.0, 0.0), false, Teinte::Cyan, 0, true);
        c.cligner_palette(2000);
        assert!(c.en_cours(2100));
        assert!((c.maj((0.0, 0.0), false, Teinte::Cyan, 2140, true).clignement - 1.0).abs() < 1e-3);
        c.cligner_palette(2200);
        assert!(c.maj((0.0, 0.0), false, Teinte::Cyan, 2200, true).clignement < 0.01, "nouvelle frappe : on repart");
        assert!(c.en_cours(2200 + 279) && !c.en_cours(2200 + crate::pixel::CLIGNEMENT_PALETTE_MS));
        assert_eq!(c.maj((0.0, 0.0), false, Teinte::Cyan, 2200 + 300, true).clignement, 0.0);
        // Mouvement réduit : jamais de clignement
        let mut n = Yeux::nouveau();
        n.cligner_palette(100);
        assert_eq!(n.maj((0.0, 0.0), false, Teinte::Cyan, 150, false).clignement, 0.0);
    }
}
