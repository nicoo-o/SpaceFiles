// État de l'explorateur : emplacement, contenu, historique, sélection, défilement, opérations

use std::collections::{HashSet, VecDeque};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::Graphics::Direct2D::Common::D2D_RECT_F;
use windows::Win32::UI::WindowsAndMessaging::{PostMessageW, WM_APP};

use crate::dossier::{self, Colonne, Emplacement, Entree, Genre, Regroupement};
use crate::edition::Edition;
use crate::lot::Lot;
use crate::outils::{self, Balise, Git};
use crate::icones::Icone;
use crate::historique::{self, Action};
use crate::operations::{self, Bilan, Controle};
use crate::stockages::Lieu;
use crate::surveillance::{self, Surveillant};
use crate::theme::*;
use crate::rendu::grille_x0;

/// Message envoyé par le fil de lecture quand un dossier est lu (LPARAM = Box<Lecture>)
pub const WM_LECTURE: u32 = WM_APP + 1;

/// État Git du dossier lu (LPARAM = Box<(numéro, Option<Git>)>)
pub const WM_GIT: u32 = WM_APP + 7;
/// Tâche de fond finie : empreinte, archive… (LPARAM = Box<Fini>)
pub const WM_OUTIL: u32 = WM_APP + 8;
/// Lecture d'appoint : colonnes Miller, taille d'un dossier (LPARAM = Box<MessageFond>)
pub const WM_FOND: u32 = WM_APP + 9;

/// Affichage du contenu (E15)
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Affichage {
    Liste,
    /// Miniatures en grille
    Grille,
    /// Colonnes Miller : parent, dossier ouvert, contenu de l'élément choisi
    Colonnes,
}

/// Côté des vignettes de la grille (Ctrl+molette), minimum de la maquette v6
pub const GRILLE_MIN: f32 = 116.0;
pub const GRILLE_MAX: f32 = 320.0;
/// Hauteur réservée au nom sous une vignette
pub const GRILLE_NOM: f32 = 34.0;
pub const GRILLE_ECART: f32 = 8.0;

/// Résultat d'une tâche de fond, affiché dans la barre d'état
pub struct Fini {
    pub message: String,
    pub erreur: bool,
    /// Texte à mettre dans le presse-papiers (empreinte)
    pub copier: Option<String>,
    /// Fichier à ouvrir (extrait d'une archive)
    pub lancer: Option<String>,
}

/// Lance une tâche dans un fil séparé ; son résultat revient par WM_OUTIL
pub fn tache(hwnd: HWND, f: impl FnOnce() -> Fini + Send + 'static) {
    let fenetre = hwnd.0 as isize;
    std::thread::spawn(move || {
        let r = Box::new(f());
        unsafe {
            let p = Box::into_raw(r);
            if PostMessageW(HWND(fenetre as *mut _), WM_OUTIL, WPARAM(0), LPARAM(p as isize)).is_err() {
                drop(Box::from_raw(p));
            }
        }
    });
}

/// Fin de la tâche de fond lancée par la palette à étapes (compression) : message à part de `WM_OUTIL`, pour qu'une autre tâche
/// (empreinte, OCR…) qui finit pendant ce temps ne passe pas pour la sienne (LPARAM = Box<Fini>)
pub const WM_PALETTE_TACHE: u32 = WM_APP + 44;

/// Comme `tache`, pour la palette : le résultat revient par `WM_PALETTE_TACHE`
pub fn tache_palette(hwnd: HWND, f: impl FnOnce() -> Fini + Send + 'static) {
    let fenetre = hwnd.0 as isize;
    std::thread::spawn(move || {
        let r = Box::new(f());
        // SAFETY: la boîte est donnée au message ; si l'envoi échoue (fenêtre détruite) personne ne la recevra et on la reprend ici
        unsafe {
            let p = Box::into_raw(r);
            if PostMessageW(HWND(fenetre as *mut _), WM_PALETTE_TACHE, WPARAM(0), LPARAM(p as isize)).is_err() {
                drop(Box::from_raw(p));
            }
        }
    });
}

pub enum Fond {
    MillerParent(Vec<Entree>),
    /// Chemin de l'élément choisi, son contenu
    MillerEnfant(String, Vec<Entree>),
    /// Nom d'un dossier, taille de tout ce qu'il contient
    Taille(String, u64),
    /// Propriétés lues (chemin en minuscules), et vrai pour le dernier lot
    Metas(Vec<(String, Arc<dossier::Meta>)>, bool),
    /// Badges du dossier Téléchargements (nom en minuscules)
    Badges(std::collections::HashMap<String, crate::telechargements::Badge>),
    /// Éléments cloud en attente de synchronisation (nom en minuscules)
    Synchro(HashSet<String>),
}

pub struct MessageFond {
    pub numero: u64,
    pub quoi: Fond,
}

fn poster<T>(fenetre: isize, msg: u32, v: T) -> bool {
    unsafe {
        let p = Box::into_raw(Box::new(v));
        if PostMessageW(HWND(fenetre as *mut _), msg, WPARAM(0), LPARAM(p as isize)).is_err() {
            drop(Box::from_raw(p));
            return false;
        }
    }
    true
}

/// Calcul des tailles en cours : un calcul plus récent arrête les autres
static TAILLES: AtomicU64 = AtomicU64::new(0);
/// Lecture des propriétés en cours : une lecture plus récente arrête les autres
static METAS: AtomicU64 = AtomicU64::new(0);
/// Surveillance de la synchronisation cloud : un seul suivi par dossier affiché
static SYNCHRO: AtomicU64 = AtomicU64::new(0);

/// Durée d'affichage d'une annonce dans la barre d'état
pub const DUREE_ANNONCE: Duration = Duration::from_secs(4);
/// Apparition en points du contenu d'un dossier ouvert
/// Part de l'écart comblée par image à 60 images par seconde (la sensation d'origine du défilement lissé)
const LISSAGE_PAR_IMAGE_60: f32 = 0.28;

/// Part de l'écart à combler après `dt` secondes : même courbe que 0,28 par image à 60 Hz, à toute cadence. Le lissage par
/// image allait 4 fois plus vite à 240 Hz (écran de l'auteur) depuis que les images suivent l'écran : le cran de molette
/// se faisait en ~60 ms au lieu de ~230 ms, ressenti comme des sauts.
pub fn part_lissage(dt: f32) -> f32 {
    1.0 - (1.0 - LISSAGE_PAR_IMAGE_60).powf(dt.clamp(0.0, 0.05) * 60.0)
}

const DUREE_APPARITION: Duration = Duration::from_millis(420);

/// Numéros uniques pour toute la fenêtre : chaque onglet reconnaît ainsi ses lectures et ses surveillances
fn numero() -> u64 {
    static COMPTEUR: AtomicU64 = AtomicU64::new(1);
    COMPTEUR.fetch_add(1, Ordering::Relaxed)
}

pub struct Lecture {
    pub generation: u64,
    /// Lecture des disques pour le volet, et non du contenu affiché
    pub volet: bool,
    /// Relecture du même dossier (après un changement) : on garde défilement et sélection
    pub actualisation: bool,
    pub resultat: Result<Vec<Entree>, String>,
    pub duree_ms: f64,
}

/// Lit un emplacement dans un fil séparé et poste le résultat à la fenêtre
fn lancer_lecture(hwnd: HWND, generation: u64, volet: bool, actualisation: bool, emplacement: Emplacement, caches: bool, tri: (Colonne, bool, bool)) {
    let fenetre = hwnd.0 as isize;
    std::thread::spawn(move || {
        let debut = Instant::now();
        let mut resultat = dossier::lire(&emplacement, caches);
        if let Ok(v) = &mut resultat {
            dossier::trier(v, tri.0, tri.1, tri.2);
        }
        let l = Box::new(Lecture { generation, volet, actualisation, resultat, duree_ms: debut.elapsed().as_secs_f64() * 1000.0 });
        unsafe {
            let p = Box::into_raw(l);
            if PostMessageW(HWND(fenetre as *mut _), WM_LECTURE, WPARAM(0), LPARAM(p as isize)).is_err() {
                drop(Box::from_raw(p));
            }
        }
    });
}

/// Raccourci du volet de gauche
#[derive(Clone)]
pub struct Favori {
    pub nom: String,
    pub emplacement: Emplacement,
    pub icone: Icone,
}

/// Élément de l'interface sous la souris
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Cible {
    Rien,
    Reduire,
    Agrandir,
    Fermer,
    Precedent,
    Suivant,
    Parent,
    Segment(usize),
    Entete(Colonne),
    Ligne(usize),
    /// Zone vide de la liste, sous la dernière ligne
    FondListe,
    Favori(usize),
    /// Section ÉPINGLÉS du volet (Ctrl+D)
    Epingle(usize),
    Disque(usize),
    Onglet(usize),
    FermerOnglet(usize),
    NouvelOnglet,
    /// Loupe de la barre haute
    RechercheHaut,
    /// Mode Essentiel de la barre haute
    EssentielHaut,
    /// Section CLOUD du volet
    Nuage(usize),
    /// Section SERVEURS du volet
    Serveur(usize),
    /// Section GROUPES du volet
    Groupe(usize),
    /// Entrée « CE PC » en tête du volet
    CePc,
    /// Titre d'une section du volet (clic : replier / déplier) : rang dans rendu::SECTIONS_VOLET
    TitreVolet(u8),
    /// Ligne de l'arbre de dossiers du volet (rang dans `App::lignes_arbre`)
    Arbre(usize),
    /// Jour de la chronologie en points (systeme::numero_jour) : clic = ne montrer que ce jour
    Jour(u32),
    /// Note du dossier (clic : la modifier)
    Note,
    /// Corbeille en points de la vue Ce PC (clic : l'ouvrir)
    Corbeille,
    /// Bouton « réglages » en haut à droite
    Reglages,
    /// Croix du panneau Réglages
    ReglagesFermer,
    /// Catégorie du rail des Réglages (rang dans page::CATEGORIES)
    ReglagesNav(usize),
    /// Ligne de la page des réglages
    Reglage(usize),
    /// Élément du formulaire « ajouter un stockage »
    Champ(usize),
    /// Reste de la barre d'état (clic : menu de la sélection)
    Statut,
    /// En-tête d'une section du regroupement (clic : replier, ou tout choisir)
    Section(usize),
    /// Vide de la barre du chemin (clic : saisir un chemin)
    Chemin,
    /// Bouton « >_ » à droite du chemin (ouvrir / fermer le panneau terminal)
    Terminal,
    /// Croix du panneau terminal
    TerminalFermer,
    /// « Ouvrir dans Windows Terminal » dans le panneau terminal
    TerminalExterne,
    /// Corps du panneau terminal : un clic ne doit pas atteindre la liste dessous
    TerminalPanneau,
    /// L'autre panneau (double panneau, F6) : un clic lui donne le focus
    AutrePanneau,
    /// Bouton de la barre d'actions rapides
    Outil(crate::barre::Outil),
    /// Densité de la liste : 0 compact, 1 confort
    Densite(usize),
    /// Puce du champ de filtre (`ext:`, `taille>`, `modif:`, Échap) : un clic ajoute son jeton au filtre
    Puce(usize),
    /// Pastille d'opération au-dessus de la barre d'état (rang dans la liste des pastilles) : le survol la déploie, le clic la fixe
    Pastille(usize),
    /// Bouton d'une pastille déployée
    BoutonPastille(usize, crate::pastilles::Bouton),
    /// Pastille « +N » des pastilles qui ne tiennent pas
    PlusPastilles,
}

impl Cible {
    /// Élément portant `.chrome` dans la maquette : son survol se traduit de 2 px vers le haut.
    /// Les lignes de liste, le fil d'Ariane, les réglages et les pastilles ont leur propre mouvement.
    pub fn est_chrome(self) -> bool {
        matches!(
            self,
            Cible::Precedent
                | Cible::Suivant
                | Cible::Parent
                | Cible::Onglet(_)
                | Cible::NouvelOnglet
                | Cible::RechercheHaut
                | Cible::EssentielHaut
                | Cible::CePc
                | Cible::Favori(_)
                | Cible::Epingle(_)
                | Cible::Disque(_)
                | Cible::Nuage(_)
                | Cible::Serveur(_)
                | Cible::Groupe(_)
                | Cible::Arbre(_)
                | Cible::Reglages
                | Cible::Terminal
                | Cible::TerminalFermer
                | Cible::TerminalExterne
                | Cible::Outil(_)
        )
    }
}

/// Mode Essentiel (O2, F9) : seuls les éléments des 7 derniers jours et les épinglés restent
pub static ESSENTIEL: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Zones cliquables calculées au dessin (en DIP)
#[derive(Default)]
pub struct Zones {
    /// Segments du fil d'Ariane : zone, emplacement vise, texte
    pub segments: Vec<(D2D_RECT_F, Emplacement, String)>,
    /// Boutons de la barre d'actions rapides
    pub barre: Vec<crate::barre::Place>,
    /// Deux zones de densité, calculées avec la barre
    pub densite: [Option<D2D_RECT_F>; 2],
    /// Chronologie en points : zone de chaque jour, le jour, nombre d'éléments ce jour-là
    pub jours: Vec<(D2D_RECT_F, u32, usize)>,
    /// Pastille de la note du dossier, dans la barre du chemin
    pub note: Option<D2D_RECT_F>,
    /// Corbeille en points, dans la vue Ce PC
    pub corbeille: Option<D2D_RECT_F>,
    /// Puces du champ de filtre, quand il est assez ouvert pour les montrer
    pub puces: Vec<D2D_RECT_F>,
}

/// Opération de fichiers en cours (copie, déplacement, suppression)
pub struct Operation {
    /// Avancement de 0 à 1
    pub avancement: f32,
    pub controle: Controle,
    pub numero: u64,
    /// « COPIE · 3 ÉLÉMENTS » : le nom de la pastille
    pub nom_court: String,
    /// Dossier de destination (copie, déplacement) : la puce de la pastille déployée
    pub destination: Option<String>,
    /// Début, pour la phase de la grille 3 × 3 et du glissement de Pixel
    pub debut: Instant,
    trace: Trace,
    /// À sortir de la corbeille une fois l'opération finie (Ctrl+Z d'un remplacement)
    apres: Vec<(String, String)>,
}

/// Où va l'opération finie dans l'historique
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Trace {
    Aucune,
    /// Action de l'utilisateur : Ctrl+Z la défera
    Noter,
    /// Ctrl+Z / Ctrl+Y en cours : l'inverse va dans cette pile, avec le libellé d'origine et l'identifiant de son entrée du journal
    Empiler(historique::Pile, &'static str, u64),
}

/// Fin d'une opération, gardée pour la pastille « TERMINÉ » (5 s) ou d'erreur (10 s, ou jusqu'à sa fermeture)
pub struct FinOp {
    pub libelle: String,
    pub destination: Option<String>,
    /// Message de l'échec ; aucun : l'opération a réussi
    pub erreur: Option<String>,
    pub quand: Instant,
}

/// Dernière action qu'on peut défaire, pour la pastille « ANNULER » (10 s). Commune à tous les onglets comme l'historique :
/// Ctrl+Z défait la même action avant et après la disparition de la pastille.
static ANNULABLE: std::sync::Mutex<Option<(String, Instant)>> = std::sync::Mutex::new(None);

/// Une action vient d'entrer dans l'historique : la pastille « SUPPRIMÉ · 1 ÉLÉMENT · ANNULER » apparaît
pub fn proposer_annulation(libelle: String) {
    *ANNULABLE.lock().unwrap_or_else(|e| e.into_inner()) = Some((libelle, Instant::now()));
}

/// Libellé de la pastille d'annulation et son âge, si elle n'a pas expiré
pub fn annulation_proposee() -> Option<String> {
    ANNULABLE.lock().unwrap_or_else(|e| e.into_inner()).as_ref().filter(|(_, t)| !crate::pastilles::expiree(crate::pastilles::Genre::Annulation, t.elapsed().as_millis())).map(|(l, _)| l.clone())
}

/// L'action a été défaite (Ctrl+Z ou bouton) : la pastille n'a plus d'objet
pub fn oublier_annulation() {
    *ANNULABLE.lock().unwrap_or_else(|e| e.into_inner()) = None;
}

/// Cartes d'information (lot 13, `toast()` de la maquette) : un court retour d'action, 4,5 s. Communes à tous les onglets comme
/// l'annulation proposée. Le retour de la barre d'état (`annoncer`) reste ; la carte le double d'une phrase lisible.
static TOASTS: std::sync::Mutex<crate::pastilles::Toasts> = std::sync::Mutex::new(crate::pastilles::Toasts::nouveau());

/// « Élément ajouté à Projets » / « 3 éléments ajoutés à Projets » (`toast(n+' ajouté à '+g)` de la maquette)
pub fn phrase_ajoute(n: usize, groupe: &str) -> String {
    if n > 1 {
        format!("{n} éléments ajoutés à {groupe}")
    } else {
        format!("Élément ajouté à {groupe}")
    }
}

/// Montre une carte d'information
pub fn toaster(texte: impl Into<String>) {
    TOASTS.lock().unwrap_or_else(|e| e.into_inner()).ajouter(texte.into(), Instant::now());
}

/// Cartes d'information encore vivantes, sous forme de pastilles
pub fn toasts_vivants() -> Vec<crate::pastilles::Pastille> {
    TOASTS.lock().unwrap_or_else(|e| e.into_inner()).pastilles(Instant::now())
}

/// Retire les cartes expirées ; vrai s'il en reste
pub fn purger_toasts() -> bool {
    TOASTS.lock().unwrap_or_else(|e| e.into_inner()).purger(Instant::now())
}

/// Le ✕ d'une carte d'information
pub fn fermer_toast(cle: u64) {
    TOASTS.lock().unwrap_or_else(|e| e.into_inner()).fermer(cle);
}

/// Opération en file d'attente : elle démarre quand la précédente est finie
struct Attente {
    genre: operations::Genre,
    sources: Vec<String>,
    dest: Option<String>,
    trace: Trace,
    apres: Vec<(String, String)>,
}

/// Sélection au lasso : rectangle tiré à la souris depuis le vide de la liste.
/// x en coordonnées du panneau, y dans le contenu défilé (le départ suit le défilement).
/// Comparaison des deux panneaux : ce qui distingue un élément de son homonyme dans l'autre panneau
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ecart {
    SeulIci,
    PlusRecent,
    PlusAncien,
    Taille,
    Pareil,
}

impl Ecart {
    pub fn libelle(self) -> &'static str {
        match self {
            Ecart::SeulIci => "SEULEMENT ICI",
            Ecart::PlusRecent => "PLUS RECENT",
            Ecart::PlusAncien => "PLUS ANCIEN",
            Ecart::Taille => "TAILLE DIFFERENTE",
            Ecart::Pareil => "IDENTIQUE",
        }
    }

    /// Libellé court, quand la place manque (double panneau)
    pub fn court(self) -> &'static str {
        match self {
            Ecart::SeulIci => "SEUL",
            Ecart::PlusRecent => "RECENT",
            Ecart::PlusAncien => "ANCIEN",
            Ecart::Taille => "TAILLE",
            Ecart::Pareil => "",
        }
    }

    /// À copier vers l'autre panneau pour les synchroniser
    pub fn a_copier(self) -> bool {
        matches!(self, Ecart::SeulIci | Ecart::PlusRecent)
    }
}

/// Ligne affichée de la liste regroupée : en-tête de section, ou élément
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Rang {
    Section(usize),
    Element(usize),
}

/// Section du regroupement : titre, nombre d'éléments (repliée : ses éléments sont cachés)
pub struct Section {
    pub titre: String,
    pub nombre: usize,
    pub replie: bool,
}

pub struct Lasso {
    pub x0: f32,
    pub y0: f32,
    pub x1: f32,
    pub y1: f32,
    /// Sélection gardée (Ctrl ou Maj au départ)
    base: HashSet<usize>,
    /// La souris a assez bougé : le rectangle est tiré
    pub actif: bool,
}

pub struct App {
    pub emplacement: Emplacement,
    pub entrees: Vec<Entree>,
    historique: Vec<Emplacement>,
    position: usize,
    pub defilement: f32,
    pub defilement_cible: f32,
    /// Instant de la dernière image du lissage : le pas dépend du temps écoulé, pas du nombre d'images
    dernier_lissage: Option<Instant>,
    /// Ligne qui a le focus du clavier
    pub selection: Option<usize>,
    /// Lignes choisies (sélection multiple)
    pub marques: HashSet<usize>,
    pub lasso: Option<Lasso>,
    /// Regrouper l'affichage (vue liste) : mode, sections repliées (par titre), lignes affichées
    pub regroupement: Regroupement,
    pub replies: HashSet<String>,
    pub rangs: Vec<Rang>,
    pub sections: Vec<Section>,
    /// Ligne affichée de chaque élément (vide sans regroupement)
    rang_de: Vec<usize>,
    /// Comparaison avec l'autre panneau (nom en minuscules → écart), jusqu'au prochain changement de dossier
    pub comparaison: Option<std::collections::HashMap<String, Ecart>>,
    /// Téléchargements : badges des fichiers (nom en minuscules)
    pub badges: std::collections::HashMap<String, crate::telechargements::Badge>,
    /// Éléments cloud pas encore synchronisés (point rouge), noms en minuscules
    pub pas_synchro: HashSet<String>,
    /// Chronologie : jour choisi (seuls ses éléments sont montrés)
    pub jour: Option<u32>,
    /// Note épinglée au dossier ouvert (O8)
    pub note: Option<String>,
    /// Corbeille vidée (O7) : départ de la dissolution et points allumés avant
    pub vidage: Option<(std::time::Instant, usize)>,
    /// Téléchargements : fichiers à mettre à la corbeille (réglages de nettoyage), au prochain passage
    nettoyage: Vec<String>,
    /// Téléchargements : (nom, dossier d'arrivée) que les règles de rangement désignent, au prochain passage
    a_ranger: Vec<(String, String)>,
    /// Chemins déjà confiés au rangement automatique : un conflit refusé ne revient pas à chaque actualisation
    ranges: std::collections::HashSet<String>,
    /// Chemin tapé à la place du fil d'Ariane (clic dans son vide, Ctrl+L, Alt+D)
    pub saisie_chemin: Option<Edition>,
    /// Tab : texte d'origine, rang de la proposition, texte proposé
    pub completion: Option<(String, usize, String)>,
    /// Point de départ d'une sélection avec Maj
    ancre: usize,
    pub survol: Cible,
    /// Bouton de la barre dont le menu est ouvert (dessiné enfoncé, sans bulle)
    pub menu_barre: Option<crate::barre::Outil>,
    /// Colonne, croissant, fichiers et dossiers mélangés
    pub tri: (Colonne, bool, bool),
    generation: u64,
    pub chargement: bool,
    pub erreur: Option<String>,
    pub duree_ms: f64,
    pub caches: bool,
    pub a_reselectionner: Vec<String>,
    /// Après la création d'un dossier : le renommer dès qu'il apparaît
    renommer_apres: Option<String>,
    pub zones: Zones,
    /// Hauteur visible de la liste, en DIP (mise à jour au dessin)
    pub hauteur_liste: f32,
    pub favoris: Vec<Favori>,
    pub disques: Vec<Entree>,
    /// Cloud détecté et stockages ajoutés (volet)
    pub nuages: Vec<Lieu>,
    pub serveurs: Vec<Lieu>,
    /// États mesurés des lignes CLOUD et SERVEURS (lot 14b), par `Lieu.detail`
    pub etats: crate::etats::Cache,
    /// Groupes créés par l'utilisateur (volet)
    pub groupes: Vec<crate::groupes::Groupe>,
    /// Arbre du volet : le clavier y est (Ctrl+Maj+B), dossier sous le curseur (clé), défilement du volet en DIP
    pub arbre_focus: bool,
    pub arbre_curseur: Option<String>,
    pub volet_defilement: f32,
    /// Dernier dossier vers lequel l'arbre a défilé : il ne défile qu'au changement de dossier
    pub arbre_vu: String,
    /// Dossier affiché avant la navigation en cours, pour retenir où l'on en était (V17)
    vu: Option<String>,
    pub edition: Option<Edition>,
    /// Chemins coupés (Ctrl+X), affichés en pâle jusqu'au collage
    pub coupes: HashSet<String>,
    pub operation: Option<Operation>,
    /// Palette à étapes : la prochaine opération qui part dans cet onglet est la sienne (posé juste avant l'appel, repris par
    /// `lancer_elements`, effacé si rien ne part). Une opération de fond (nettoyage, rangement, images) ne le prend jamais.
    pub palette_arme: bool,
    /// Numéro de l'opération lancée par la palette (progression et fin ne comptent que pour elle)
    pub palette_numero: Option<u64>,
    /// Entrée d'historique que l'opération de la palette a notée en finissant (son « Annuler » défait celle-là et pas une autre)
    pub palette_entree: Option<u64>,
    /// Dernière opération finie de cet onglet : pastille « TERMINÉ » ou d'erreur
    pub fin_op: Option<FinOp>,
    file: VecDeque<Attente>,
    /// Copie ou déplacement en attente de réponses (carte des conflits)
    pub conflits: Option<crate::conflit::Conflits>,
    /// Message temporaire dans la barre d'état : texte, erreur ?, heure
    pub annonce: Option<(String, bool, Instant)>,
    surveillant: Option<Surveillant>,
    jeton_surveillance: u64,
    /// Zone visee pendant un glisser-déposer (FondListe : le dossier ouvert)
    pub depot: Cible,
    /// Filtre tapé au clavier : seules les entrées qui y correspondent sont affichées
    pub filtre: String,
    /// Champ de filtre ouvert sans texte (Ctrl+F sur un filtre vide, comme `openSearch` de la maquette) : il ne se ferme qu'à Échap
    pub filtre_ouvert: bool,
    /// Contenu complet lu, avant filtre
    pub tout: Vec<Entree>,
    /// Début de l'apparition en points du contenu (dossier qui vient d'être ouvert)
    pub apparition: Option<Instant>,
    /// Dossiers épinglés (volet)
    pub epingles: Vec<Favori>,
    pub affichage: Affichage,
    pub cote_grille: f32,
    /// Vignettes par rangée, calculé au dessin
    pub par_rangee: usize,
    /// Vue « tailles » : taille de chaque dossier calculée en arrière-plan, barres de points
    pub tailles: bool,
    /// Tailles déjà calculées (chemin en minuscules → octets) : gardées pendant les actualisations
    tailles_connues: std::collections::HashMap<String, u64>,
    pub calcul_tailles: usize,
    pub git: Option<Git>,
    /// Le dossier ouvert est marqué non fiable (V11) : mis à jour après chaque lecture
    pub non_fiable: bool,
    numero_git: u64,
    /// Colonnes Miller : contenu du dossier parent et de l'élément choisi
    pub miller_parent: Vec<Entree>,
    pub miller_enfant: Vec<Entree>,
    pub miller_demande: Option<String>,
    numero_fond: u64,
}

impl App {
    pub fn nouveau(depart: Emplacement) -> Self {
        let mut a = Self::sans_favoris(depart);
        a.charger_favoris();
        a
    }

    /// Les favoris du volet viennent des dossiers connus du Shell (~90 ms mesurés au premier appel). Au démarrage de la fenêtre
    /// la lecture se fait dans un fil (`systeme::precharger_favoris`) pendant que la fenêtre se crée, et `charger_favoris` ne
    /// s'appelle qu'à la fin, juste avant l'affichage : voir `main`.
    pub fn sans_favoris(depart: Emplacement) -> Self {
        Self {
            emplacement: depart.clone(),
            entrees: Vec::new(),
            historique: vec![depart],
            position: 0,
            defilement: 0.0,
            defilement_cible: 0.0,
            dernier_lissage: None,
            selection: None,
            marques: HashSet::new(),
            ancre: 0,
            survol: Cible::Rien,
            menu_barre: None,
            tri: (Colonne::Nom, true, false),
            generation: 0,
            chargement: false,
            erreur: None,
            duree_ms: 0.0,
            caches: crate::reglages::actuels().caches,
            a_reselectionner: Vec::new(),
            renommer_apres: None,
            zones: Zones::default(),
            hauteur_liste: 400.0,
            favoris: Vec::new(),
            disques: Vec::new(),
            nuages: Vec::new(),
            serveurs: Vec::new(),
            etats: Default::default(),
            groupes: crate::groupes::liste(),
            arbre_focus: false,
            arbre_curseur: None,
            volet_defilement: 0.0,
            arbre_vu: String::new(),
            vu: None,
            edition: None,
            coupes: HashSet::new(),
            operation: None,
            palette_arme: false,
            palette_numero: None,
            palette_entree: None,
            fin_op: None,
            file: VecDeque::new(),
            conflits: None,
            annonce: None,
            surveillant: None,
            jeton_surveillance: 0,
            depot: Cible::Rien,
            lasso: None,
            regroupement: Regroupement::Aucun,
            replies: HashSet::new(),
            rangs: Vec::new(),
            sections: Vec::new(),
            rang_de: Vec::new(),
            comparaison: None,
            badges: std::collections::HashMap::new(),
            pas_synchro: HashSet::new(),
            jour: None,
            note: None,
            vidage: None,
            nettoyage: Vec::new(),
            a_ranger: Vec::new(),
            ranges: Default::default(),
            saisie_chemin: None,
            completion: None,
            filtre: String::new(),
            // Mode essai (captures sans clavier) : champ de filtre ouvert dès le lancement, comme après Ctrl+F sur un filtre vide
            filtre_ouvert: ouvert_par_essai(),
            tout: Vec::new(),
            apparition: None,
            epingles: Vec::new(),
            affichage: [Affichage::Liste, Affichage::Grille, Affichage::Colonnes][(crate::reglages::actuels().vue_defaut as usize).min(2)],
            cote_grille: GRILLE_MIN,
            par_rangee: 1,
            tailles: false,
            tailles_connues: std::collections::HashMap::new(),
            calcul_tailles: 0,
            git: None,
            non_fiable: false,
            numero_git: 0,
            miller_parent: Vec::new(),
            miller_enfant: Vec::new(),
            miller_demande: None,
            numero_fond: numero(),
        }
    }

    /// Remplit les favoris du volet (attend la fin de la lecture des dossiers connus si elle tourne encore)
    pub fn charger_favoris(&mut self) {
        self.favoris = crate::systeme::favoris().into_iter().map(|(nom, emplacement, icone)| Favori { nom, emplacement, icone }).collect();
    }

    pub fn generation(&self) -> u64 {
        self.generation
    }

    /// Libellé de l'onglet
    pub fn titre(&self) -> String {
        self.emplacement.titre()
    }

    /// Ce PC, la liste des partages d'un serveur, les réglages : rien à copier, renommer ou supprimer
    pub fn sans_fichiers(&self) -> bool {
        matches!(self.emplacement, Emplacement::CePC | Emplacement::Serveur(_) | Emplacement::Reglages | Emplacement::Archive { .. } | Emplacement::Sftp { .. })
    }

    /// Chemin du dossier ouvert (aucun pour Ce PC)
    pub fn dossier(&self) -> Option<&str> {
        match &self.emplacement {
            Emplacement::Dossier(c) => Some(c),
            _ => None,
        }
    }

    pub fn chemin_de(&self, e: &Entree) -> Option<String> {
        // Corbeille : l'élément réel est le fichier `$R` (son chemin d'origine ne mène plus à rien)
        if self.emplacement == Emplacement::Panneau {
            return crate::panneau::analyse(&e.nom);
        }
        if self.emplacement == Emplacement::Corbeille {
            return crate::corbeille::reel(&dossier::joindre(e.parent.as_deref()?, &e.nom));
        }
        if let Genre::Disque { racine, .. } = &e.genre {
            return Some(racine.clone());
        }
        if let Some(p) = &e.parent {
            return Some(dossier::joindre(p, &e.nom));
        }
        let c = self.dossier()?;
        Some(if c.ends_with('\\') { format!("{c}{}", e.nom) } else { format!("{c}\\{}", e.nom) })
    }

    /// Lance la lecture de l'emplacement courant dans un fil séparé, et sa surveillance
    pub fn charger(&mut self, hwnd: HWND) {
        // Les entrées et la sélection sont encore celles du dossier quitté
        if self.vu.as_deref().is_some_and(|v| self.dossier() != Some(v)) {
            let nom = self.choisis().first().and_then(|&i| self.entrees.get(i)).map(|e| e.nom.clone()).unwrap_or_default();
            if let Some(v) = self.vu.take() {
                noter_reprise(&v, &nom);
            }
        }
        self.saisie_chemin = None;
        self.comparaison = None;
        self.badges.clear();
        self.pas_synchro.clear();
        self.jour = None;
        self.note = match &self.emplacement {
            Emplacement::Dossier(c) => crate::reglages::note_du_dossier(c),
            _ => None,
        };
        self.generation = numero();
        self.chargement = true;
        self.edition = None;
        self.filtre.clear();
        self.filtre_ouvert = ouvert_par_essai();
        if let Emplacement::Dossier(c) = &self.emplacement {
            self.tri = crate::reglages::tri_du_dossier(c);
            // Un dossier qu'on vient d'ouvrir reprend sa vue et ses colonnes ; une actualisation n'y touche pas
            if self.vu.as_deref() != Some(c.as_str()) {
                crate::reglages::afficher_dossier(Some(c));
                self.affichage = [Affichage::Liste, Affichage::Grille, Affichage::Colonnes][crate::reglages::vue_du_dossier(c) as usize];
            }
        }
        let regroupement = match &self.emplacement {
            Emplacement::Dossier(c) => crate::reglages::regroupement_du_dossier(c),
            // Récents : les plus récents d'abord, par jour
            Emplacement::Recents => {
                self.tri = (Colonne::Date, false, true);
                Regroupement::Date
            }
            // Corbeille : les derniers supprimés d'abord, par jour
            Emplacement::Corbeille => {
                self.tri = (Colonne::Date, false, true);
                Regroupement::Date
            }
            // Doublons : un lot par section, le plus de place à gagner d'abord
            Emplacement::Doublons(_) => {
                self.tri = (Colonne::Date, false, false);
                Regroupement::Doublon
            }
            _ => Regroupement::Aucun,
        };
        if regroupement != self.regroupement || !self.replies.is_empty() {
            self.regroupement = regroupement;
            self.replies.clear();
        }
        lancer_lecture(hwnd, self.generation, false, false, self.emplacement.clone(), self.caches, self.tri);
        if self.emplacement == Emplacement::CePC {
            crate::corbeille::actualiser(hwnd);
        }
        self.git = None;
        self.miller_demande = None;
        self.miller_enfant.clear();
        if let Emplacement::Dossier(c) = &self.emplacement {
            noter_recent(c);
            self.vu = Some(c.clone());
        }
        if self.affichage == Affichage::Colonnes {
            self.charger_miller_parent(hwnd);
        }
        self.jeton_surveillance = numero();
        self.surveillant = self.dossier().and_then(|c| surveillance::surveiller(hwnd, c, self.jeton_surveillance));
    }

    /// Relit le même dossier sans perdre la position (le dossier a changé)
    pub fn actualiser(&mut self, hwnd: HWND) {
        self.generation = numero();
        lancer_lecture(hwnd, self.generation, false, true, self.emplacement.clone(), self.caches, self.tri);
    }

    /// Le dossier surveillé a changé
    pub fn changement(&mut self, hwnd: HWND, jeton: u64) {
        if jeton == self.jeton_surveillance && self.edition.is_none() {
            self.actualiser(hwnd);
        }
    }

    /// Lit les disques pour le volet (un lecteur réseau peut être lent : jamais dans le fil de l'interface)
    pub fn charger_disques(&self, hwnd: HWND) {
        lancer_lecture(hwnd, 0, true, false, Emplacement::CePC, false, (Colonne::Nom, true, false));
    }

    /// Reçoit le résultat d'une lecture. Ignore les lectures dépassées.
    pub fn recevoir(&mut self, hwnd: HWND, l: Lecture) {
        if l.volet {
            if let Ok(v) = l.resultat {
                self.disques = v;
            }
            return;
        }
        if l.generation != self.generation {
            return;
        }
        if let (Emplacement::CePC, Ok(v)) = (&self.emplacement, &l.resultat) {
            self.disques = v.clone();
        }
        self.chargement = false;
        self.duree_ms = l.duree_ms;

        // Ce qu'il faut resélectionner : les mêmes noms lors d'une actualisation
        let (noms, focus) = if l.actualisation {
            let noms: Vec<String> = self.marques.iter().filter_map(|&i| self.entrees.get(i)).map(|e| e.nom.clone()).collect();
            let focus = self.selection.and_then(|i| self.entrees.get(i)).map(|e| e.nom.clone());
            (noms, focus)
        } else {
            let noms = std::mem::take(&mut self.a_reselectionner);
            let focus = noms.first().cloned();
            (noms, focus)
        };

        match l.resultat {
            Ok(v) => {
                self.tout = v;
                self.erreur = None;
            }
            Err(e) => {
                self.tout.clear();
                self.erreur = Some(e);
            }
        }
        self.rebatir();
        if !l.actualisation && crate::reglages::animations_actives() {
            self.apparition = Some(Instant::now());
        }
        let trouver = |entrees: &[Entree], nom: &str| entrees.iter().position(|e| e.nom.eq_ignore_ascii_case(nom));
        self.marques = noms.iter().filter_map(|n| trouver(&self.entrees, n)).collect();
        self.selection = focus.and_then(|n| trouver(&self.entrees, &n)).or_else(|| self.marques.iter().min().copied());

        if !l.actualisation {
            self.defilement = 0.0;
            self.defilement_cible = 0.0;
            if self.selection.is_none() && !self.entrees.is_empty() {
                self.selection = Some(0);
                self.marques.insert(0);
            }
            self.montrer_selection();
            self.defilement = self.defilement_cible;
        } else {
            if let Some(i) = self.selection {
                if self.marques.is_empty() {
                    self.marques.insert(i);
                }
            }
            self.defilement_cible = self.defilement_cible.clamp(0.0, self.defilement_max());
            self.defilement = self.defilement.clamp(0.0, self.defilement_max());
        }
        self.ancre = self.selection.unwrap_or(0);

        self.apres_lecture(hwnd);

        // Nouveau dossier créé : on passe tout de suite en renommage
        if let Some(nom) = self.renommer_apres.take() {
            if let Some(i) = trouver(&self.entrees, &nom) {
                self.choisir_seul(i);
                self.montrer_selection();
                self.commencer_renommage();
            } else {
                self.renommer_apres = Some(nom);
            }
        }
    }

    pub fn naviguer(&mut self, hwnd: HWND, emp: Emplacement) {
        if emp == self.emplacement {
            return;
        }
        // Dernier filet : liens, épingles, récents, sessions et onglets reçus d'une autre fenêtre passent tous ici
        if let Some(g) = emp.experimentale() {
            if !crate::reglages::fonction_experimentale_permise(crate::reglages::experimental(), g) {
                self.annoncer(crate::reglages::MESSAGE_EXPERIMENTAL, true);
                return;
            }
        }
        self.historique.truncate(self.position + 1);
        self.historique.push(emp.clone());
        self.position = self.historique.len() - 1;
        self.emplacement = emp;
        self.charger(hwnd);
    }

    pub fn precedent(&mut self, hwnd: HWND) {
        if self.position > 0 {
            // En reculant vers le dossier parent, on resélectionne celui d'où l'on vient
            self.a_reselectionner = self.emplacement.nom().map(str::to_string).into_iter().collect();
            self.position -= 1;
            self.emplacement = self.historique[self.position].clone();
            self.charger(hwnd);
        }
    }

    pub fn suivant(&mut self, hwnd: HWND) {
        if self.position + 1 < self.historique.len() {
            self.position += 1;
            self.emplacement = self.historique[self.position].clone();
            self.charger(hwnd);
        }
    }

    pub fn parent(&mut self, hwnd: HWND) {
        if let Some(p) = self.emplacement.parent() {
            self.a_reselectionner = match &self.emplacement {
                Emplacement::Dossier(c) if c.len() <= 3 => Vec::new(),
                e => e.nom().map(str::to_string).into_iter().collect(),
            };
            self.naviguer(hwnd, p);
        }
    }

    pub fn peut_reculer(&self) -> bool {
        self.position > 0
    }
    pub fn peut_avancer(&self) -> bool {
        self.position + 1 < self.historique.len()
    }

    /// Ouvre l'entrée : entre dans un dossier ou un disque, lance un fichier
    pub fn ouvrir(&mut self, hwnd: HWND, i: usize) {
        self.oublier_nouveau(i);
        let Some(e) = self.entrees.get(i) else { return };
        if e.est_conteneur() {
            let emp = self.emplacement.enfant(e);
            self.naviguer(hwnd, emp);
        } else if let Emplacement::Archive { fichier, interne } = &self.emplacement {
            // Fichier dans une archive : extrait dans un dossier temporaire, puis ouvert
            let (fichier, membre) = (fichier.clone(), if interne.is_empty() { e.nom.clone() } else { format!("{interne}/{}", e.nom) });
            self.annoncer(format!("EXTRACTION DE {}…", e.nom.to_uppercase()), false);
            tache(hwnd, move || {
                let dest = crate::archive::dossier_temporaire(&fichier);
                let bruts = crate::archive::bruts(&fichier, &[membre.clone()]);
                match crate::archive::extraire(&fichier, &bruts, &dest) {
                    Ok(()) => Fini { message: "OUVERT DEPUIS L'ARCHIVE (COPIE TEMPORAIRE)".into(), erreur: false, copier: None, lancer: Some(dossier::joindre(&dest, &membre.replace('/', "\\"))) },
                    Err(e) => Fini { message: e, erreur: true, copier: None, lancer: None },
                }
            });
        } else if let Emplacement::Sftp { hote, chemin } = &self.emplacement {
            // Fichier distant : téléchargé dans un dossier temporaire, puis ouvert
            let (hote, distant, nom) = (hote.clone(), crate::sftp::joindre(chemin, &e.nom), e.nom.clone());
            self.annoncer(format!("TELECHARGEMENT DE {}…", nom.to_uppercase()), false);
            tache(hwnd, move || {
                let dest = crate::sftp::dossier_temporaire(&hote);
                let _ = std::fs::create_dir_all(&dest);
                match crate::sftp::telecharger(&hote, &[distant], &dest) {
                    Ok(()) => Fini { message: "OUVERT DEPUIS LE SERVEUR (COPIE TEMPORAIRE)".into(), erreur: false, copier: None, lancer: Some(dossier::joindre(&dest, &nom)) },
                    Err(e) => Fini { message: e, erreur: true, copier: None, lancer: None },
                }
            });
        } else if let Some(chemin) = self.chemin_de(e) {
            if crate::archive::est_archive(&e.ext) {
                self.naviguer(hwnd, Emplacement::Archive { fichier: chemin, interne: String::new() });
            } else {
                crate::systeme::lancer(hwnd, &chemin);
            }
        }
    }

    pub fn trier_par(&mut self, col: Colonne) {
        // Les entrées glisseront vers leur nouvelle place (lot 12B)
        crate::mouvement::demander_flip();
        self.tri = if self.tri.0 == col { (col, !self.tri.1, self.tri.2) } else { (col, col == Colonne::Nom || col == Colonne::Type, self.tri.2) };
        if let Emplacement::Dossier(c) = &self.emplacement {
            crate::reglages::memoriser_tri(c, self.tri);
        }
        let focus = self.selection.and_then(|i| self.entrees.get(i)).map(|e| e.nom.clone());
        let noms: HashSet<String> = self.marques.iter().filter_map(|&i| self.entrees.get(i)).map(|e| e.nom.clone()).collect();
        dossier::trier(&mut self.tout, self.tri.0, self.tri.1, self.tri.2);
        self.rebatir();
        self.selection = focus.and_then(|n| self.entrees.iter().position(|e| e.nom == n));
        self.marques = self.entrees.iter().enumerate().filter(|(_, e)| noms.contains(&e.nom)).map(|(i, _)| i).collect();
        self.edition = None;
        self.montrer_selection();
    }

    /// Éléments par rangée et hauteur d'une rangée (liste : 1 par ligne ; grille : une rangée de vignettes)
    pub fn pas(&self) -> (usize, f32) {
        match self.affichage {
            Affichage::Grille => (self.par_rangee.max(1), self.cote_grille + GRILLE_NOM + GRILLE_ECART),
            _ => (1, hauteur_ligne()),
        }
    }

    fn defilement_max(&self) -> f32 {
        let (par, h) = self.pas();
        defilement_maximal(self.nb_rangs().div_ceil(par), h, self.hauteur_liste)
    }

    // ── Regrouper l'affichage ──

    /// Lignes affichées : en-têtes de section compris
    pub fn nb_rangs(&self) -> usize {
        if self.rangs.is_empty() {
            self.entrees.len()
        } else {
            self.rangs.len()
        }
    }

    /// Ce qu'il y a à la ligne affichée r
    pub fn rang(&self, r: usize) -> Rang {
        self.rangs.get(r).copied().unwrap_or(Rang::Element(r))
    }

    /// Ligne affichée de l'élément i
    pub fn rang_de(&self, i: usize) -> usize {
        self.rang_de.get(i).copied().unwrap_or(i)
    }

    /// Rien à montrer (ni élément, ni section repliée)
    pub fn vide(&self) -> bool {
        self.entrees.is_empty() && self.rangs.is_empty()
    }

    /// Éléments affichés : le filtre, puis (vue liste regroupée) rangés par section ;
    /// les éléments des sections repliées sont retirés, comme par un filtre
    fn rebatir(&mut self) {
        let v = self.filtrees();
        self.rangs.clear();
        self.sections.clear();
        self.rang_de.clear();
        let possible = matches!(self.emplacement, Emplacement::Dossier(_) | Emplacement::Recherche { .. } | Emplacement::Groupe(_) | Emplacement::Recents | Emplacement::Archive { .. } | Emplacement::Serveur(_) | Emplacement::Sftp { .. } | Emplacement::Doublons(_));
        if self.regroupement == Regroupement::Aucun || self.affichage != Affichage::Liste || v.is_empty() || !possible {
            self.entrees = v;
            return;
        }
        let chemin = self.dossier().unwrap_or("").to_string();
        let cles: Vec<(u32, String)> = v.iter().map(|e| dossier::section(e, self.regroupement, &chemin)).collect();
        let mut ordre: Vec<usize> = (0..v.len()).collect();
        // Date : les plus récentes d'abord, sauf tri par date croissante
        let inverse = self.regroupement == Regroupement::Date && self.tri.0 == Colonne::Date && self.tri.1;
        ordre.sort_by(|&a, &b| if inverse { cles[b].cmp(&cles[a]) } else { cles[a].cmp(&cles[b]) });
        let mut entrees = Vec::with_capacity(v.len());
        let mut courante: Option<usize> = None;
        for &k in &ordre {
            if courante.map_or(true, |c| cles[c] != cles[k]) {
                courante = Some(k);
                let titre = cles[k].1.clone();
                let replie = self.replies.contains(&titre);
                self.sections.push(Section { titre, nombre: 0, replie });
                self.rangs.push(Rang::Section(self.sections.len() - 1));
            }
            let s = self.sections.last_mut().unwrap();
            s.nombre += 1;
            if !s.replie {
                self.rang_de.push(self.rangs.len());
                self.rangs.push(Rang::Element(entrees.len()));
                entrees.push(v[k].clone());
            }
        }
        self.entrees = entrees;
    }

    /// Compare ce panneau à l'autre : présence, date (à 2 secondes près), taille
    pub fn comparer_avec(&self, autre: &App) -> std::collections::HashMap<String, Ecart> {
        let autres: std::collections::HashMap<String, &Entree> = autre.tout.iter().map(|e| (e.nom.to_lowercase(), e)).collect();
        self.tout
            .iter()
            .map(|e| {
                let k = e.nom.to_lowercase();
                let ecart = match autres.get(&k) {
                    None => Ecart::SeulIci,
                    Some(o) if e.est_conteneur() || o.est_conteneur() => Ecart::Pareil,
                    Some(o) => {
                        let dt = e.modif as i64 - o.modif as i64;
                        if dt > 20_000_000 {
                            Ecart::PlusRecent
                        } else if dt < -20_000_000 {
                            Ecart::PlusAncien
                        } else if e.taille != o.taille {
                            Ecart::Taille
                        } else {
                            Ecart::Pareil
                        }
                    }
                };
                (k, ecart)
            })
            .collect()
    }

    /// Choisit ce qui manque dans l'autre panneau ou y est plus ancien ; renvoie leur nombre
    pub fn choisir_ecarts(&mut self) -> usize {
        let Some(cmp) = &self.comparaison else { return 0 };
        let v: Vec<usize> = self.entrees.iter().enumerate().filter(|(_, e)| cmp.get(&e.nom.to_lowercase()).is_some_and(|x| x.a_copier())).map(|(i, _)| i).collect();
        self.marques = v.iter().copied().collect();
        self.selection = v.first().copied();
        if let Some(&i) = v.first() {
            self.ancre = i;
        }
        self.montrer_selection();
        v.len()
    }

    /// Rebâtit en gardant l'élément en cours et la sélection
    pub fn rebatir_garder(&mut self) {
        let focus = self.selection.and_then(|i| self.entrees.get(i)).map(|e| e.nom.clone());
        let noms: HashSet<String> = self.marques.iter().filter_map(|&i| self.entrees.get(i)).map(|e| e.nom.clone()).collect();
        self.rebatir();
        self.selection = focus.and_then(|n| self.entrees.iter().position(|e| e.nom == n));
        self.marques = self.entrees.iter().enumerate().filter(|(_, e)| noms.contains(&e.nom)).map(|(i, _)| i).collect();
        self.edition = None;
        self.defilement_cible = self.defilement_cible.clamp(0.0, self.defilement_max());
        self.montrer_selection();
    }

    /// Choisit un regroupement pour ce dossier (retenu)
    pub fn regrouper(&mut self, hwnd: HWND, g: Regroupement) {
        crate::mouvement::demander_flip();
        self.regroupement = g;
        self.replies.clear();
        // La quarantaine a besoin de l'origine et de la signature, que les propriétés déjà lues n'ont pas forcément
        if g == Regroupement::Origine {
            self.oublier_metas();
            self.lancer_metas(hwnd);
        }
        if let Emplacement::Dossier(c) = &self.emplacement {
            crate::reglages::memoriser_regroupement(c, g);
        }
        self.rebatir_garder();
        self.annoncer(if g == Regroupement::Aucun { "SANS REGROUPEMENT".to_string() } else { format!("REGROUPE PAR {}", g.libelle()) }, false);
    }

    /// Replie ou déplie la section s
    pub fn replier(&mut self, s: usize) {
        let Some(titre) = self.sections.get(s).map(|x| x.titre.clone()) else { return };
        if !self.replies.remove(&titre) {
            self.replies.insert(titre);
        }
        self.rebatir_garder();
    }

    /// Clic sur l'en-tête d'une section : tous ses éléments sont choisis (ajoutés avec Ctrl ou Maj)
    pub fn choisir_section(&mut self, s: usize, ajouter: bool) {
        let Some(debut) = self.rangs.iter().position(|r| *r == Rang::Section(s)) else { return };
        let elements: Vec<usize> = self.rangs[debut + 1..].iter().map_while(|r| if let Rang::Element(i) = r { Some(*i) } else { None }).collect();
        if !ajouter {
            self.marques.clear();
        }
        self.marques.extend(elements.iter().copied());
        if let Some(&i) = elements.first() {
            self.selection = Some(i);
            self.ancre = i;
        }
    }

    /// Le fil d'Ariane devient un champ avec le chemin complet, tout sélectionné
    pub fn commencer_saisie_chemin(&mut self) {
        let t = match &self.emplacement {
            Emplacement::Dossier(c) | Emplacement::Recherche { racine: c, .. } => c.clone(),
            Emplacement::Serveur(h) => format!("\\\\{h}"),
            Emplacement::Archive { fichier, .. } => fichier.clone(),
            Emplacement::Sftp { hote, chemin } => crate::sftp::adresse(hote, chemin),
            Emplacement::Reglages => return,
            _ => String::new(),
        };
        let mut ed = Edition::champ(&t);
        ed.tout();
        self.saisie_chemin = Some(ed);
        self.completion = None;
        self.filtre.clear();
        self.filtre_ouvert = false;
    }

    /// Tab (Maj+Tab : à l'envers) : complète le dernier morceau avec les dossiers qui commencent pareil
    pub fn completer_chemin(&mut self, arriere: bool) {
        let Some(ed) = &self.saisie_chemin else { return };
        let texte = ed.valeur();
        let (base, suite) = match &self.completion {
            Some((b, k, propose)) if *propose == texte => (b.clone(), Some(*k)),
            _ => (texte.clone(), None),
        };
        let Some(i) = base.rfind('\\') else { return };
        let (dossier, prefixe) = (&base[..=i], base[i + 1..].to_lowercase());
        let mut noms: Vec<String> = std::fs::read_dir(dossier)
            .map(|it| it.flatten().filter(|e| e.file_type().is_ok_and(|t| t.is_dir())).map(|e| e.file_name().to_string_lossy().to_string()).filter(|n| n.to_lowercase().starts_with(&prefixe)).collect())
            .unwrap_or_default();
        if noms.is_empty() {
            return;
        }
        noms.sort_by_key(|n| n.to_lowercase());
        let n = noms.len();
        let k = match suite {
            Some(k) if arriere => (k + n - 1) % n,
            Some(k) => (k + 1) % n,
            None if arriere => n - 1,
            None => 0,
        };
        let propose = format!("{dossier}{}", noms[k]);
        self.saisie_chemin = Some(Edition::champ(&propose));
        self.completion = Some((base, k, propose));
    }

    /// Entrée : ouvre le chemin tapé (dossier, fichier montré dans son dossier, archive, serveur)
    /// Omnibar : un texte qui n'a pas l'air d'un chemin n'est pas une erreur, il est rendu à l'appelant pour la recherche
    pub fn valider_chemin(&mut self, hwnd: HWND) -> Option<String> {
        let Some(ed) = &self.saisie_chemin else { return None };
        let brut = ed.valeur();
        // SFTP et WebDAV sont gelés tant que le réglage `experimental` est faux : refusés ici, avec la raison
        if let Some(g) = crate::reglages::genre_adresse(&brut) {
            if !crate::reglages::fonction_experimentale_permise(crate::reglages::experimental(), g) {
                self.saisie_chemin = None;
                self.annoncer(crate::reglages::MESSAGE_EXPERIMENTAL, true);
                return None;
            }
        }
        // SFTP : les / restent des / (chemin du serveur)
        if let Some((hote, chemin)) = crate::sftp::decouper(brut.trim().trim_matches('"')) {
            self.saisie_chemin = None;
            self.naviguer(hwnd, Emplacement::Sftp { hote, chemin });
            return None;
        }
        let mut t = brut.trim().trim_matches('"').replace('/', "\\");
        // %VARIABLE% et ~ : comme dans l'Explorateur
        while let Some(a) = t.find('%') {
            let Some(b) = t[a + 1..].find('%').map(|b| a + 1 + b) else { break };
            let v = std::env::var(&t[a + 1..b]).unwrap_or_default();
            if v.is_empty() {
                break;
            }
            t.replace_range(a..=b, &v);
        }
        if let Some(reste) = t.strip_prefix('~') {
            t = format!("{}{reste}", std::env::var("USERPROFILE").unwrap_or_default());
        }
        let p = std::path::Path::new(&t);
        let emp = if t.is_empty() || t.eq_ignore_ascii_case("ce pc") {
            Some(Emplacement::CePC)
        } else if t.starts_with("\\\\") || t.to_ascii_lowercase().starts_with("sftp://") || p.is_dir() {
            Some(Emplacement::dossier(&t))
        } else if p.is_file() {
            let ext = t.rsplit_once('.').map(|(_, e)| e.to_uppercase()).unwrap_or_default();
            if crate::archive::est_archive(&ext) {
                Some(Emplacement::Archive { fichier: t.clone(), interne: String::new() })
            } else {
                self.saisie_chemin = None;
                self.montrer(hwnd, &t);
                return None;
            }
        } else {
            None
        };
        match emp {
            Some(e) => {
                self.saisie_chemin = None;
                self.naviguer(hwnd, e);
            }
            // Des mots sans « : » ni séparateur de dossier : une recherche, pas un chemin
            None if !t.contains(['\\', ':']) => {
                self.saisie_chemin = None;
                return Some(brut.trim().to_string());
            }
            None => self.annoncer(format!("CHEMIN INTROUVABLE : {}", t.to_uppercase()), true),
        }
        None
    }

    /// Début d'un lasso au point (x, y) du panneau
    pub fn commencer_lasso(&mut self, x: f32, y: f32, garder: bool) {
        let y = y - LISTE_Y + self.defilement;
        let base = if garder { self.marques.clone() } else { HashSet::new() };
        self.lasso = Some(Lasso { x0: x, y0: y, x1: x, y1: y, base, actif: false });
    }

    /// La souris bouge pendant un lasso : le rectangle suit, la liste défile près des bords,
    /// les éléments touchés sont choisis. Renvoie vrai s'il faut redessiner.
    pub fn etirer_lasso(&mut self, x: f32, y: f32) -> bool {
        let bas = LISTE_Y + self.hauteur_liste;
        if y > bas - 8.0 {
            self.defiler((y - bas + 8.0).min(40.0));
        } else if y < LISTE_Y + 8.0 {
            self.defiler(-(LISTE_Y + 8.0 - y).min(40.0));
        }
        self.defilement = self.defilement_cible;
        let (par, hr) = self.pas();
        let grille = self.affichage == Affichage::Grille;
        let rangs = if grille { Vec::new() } else { self.rangs.clone() };
        let (cote, n) = (self.cote_grille, if grille { self.entrees.len() } else { self.nb_rangs() });
        let Some(l) = self.lasso.as_mut() else { return false };
        l.x1 = x;
        l.y1 = y - LISTE_Y + self.defilement;
        if !l.actif && (l.x1 - l.x0).abs() < 5.0 && (l.y1 - l.y0).abs() < 5.0 {
            return false;
        }
        l.actif = true;
        let (gauche, droite) = (l.x0.min(l.x1), l.x0.max(l.x1));
        let (haut, bas) = (l.y0.min(l.y1).max(0.0), l.y0.max(l.y1).max(0.0));
        let mut choisis = l.base.clone();
        let mut dernier = None;
        let (r0, r1) = ((haut / hr) as usize, (bas / hr) as usize);
        for rang in r0..=r1 {
            for col in 0..par {
                let i = rang * par + col;
                if i >= n {
                    break;
                }
                // Liste regroupée : les en-têtes de section ne se choisissent pas
                let i = match rangs.get(i) {
                    Some(Rang::Element(e)) => *e,
                    Some(Rang::Section(_)) => continue,
                    None => i,
                };
                let touche = if grille {
                    let cx = grille_x0() + col as f32 * (cote + GRILLE_ECART);
                    let cy = rang as f32 * hr;
                    cx < droite && cx + cote > gauche && cy < bas && cy + cote + GRILLE_NOM > haut
                } else {
                    true
                };
                if touche {
                    choisis.insert(i);
                    dernier = Some(i);
                }
            }
        }
        self.marques = choisis;
        if dernier.is_some() {
            self.selection = dernier;
        }
        self.oublier_nouveaux_choisis();
        true
    }

    /// Fin du lasso. Renvoie vrai si un rectangle a été tiré.
    pub fn finir_lasso(&mut self) -> bool {
        self.lasso.take().is_some_and(|l| l.actif)
    }

    pub fn defiler(&mut self, delta: f32) {
        self.defilement_cible = (self.defilement_cible + delta).clamp(0.0, self.defilement_max());
    }

    /// Fait avancer l'animation de défilement. Renvoie vrai tant qu'elle n'est pas finie.
    pub fn animer(&mut self) -> bool {
        // Mouvement réduit : plus d'apparition, de dissolution ni de défilement lissé ; la liste saute à sa position
        if !crate::reglages::animations_actives() {
            self.apparition = None;
            self.vidage = None;
            self.defilement_cible = self.defilement_cible.clamp(0.0, self.defilement_max());
            self.defilement = self.defilement_cible;
            return false;
        }
        let mut apparait = self.apparition.is_some_and(|t| t.elapsed() < DUREE_APPARITION);
        if !apparait {
            self.apparition = None;
        }
        if self.vidage.is_some_and(|v| v.0.elapsed().as_millis() > 1400) {
            self.vidage = None;
        }
        apparait |= self.vidage.is_some();
        self.defilement_cible = self.defilement_cible.clamp(0.0, self.defilement_max());
        let ecart = self.defilement_cible - self.defilement;
        if ecart.abs() < 0.5 {
            self.defilement = self.defilement_cible;
            self.dernier_lissage = None;
            return apparait;
        }
        // Première image d'une salve (ou longue pause) : un pas de 1/60 s, comme avant
        let maintenant = Instant::now();
        let dt = self.dernier_lissage.map_or(1.0 / 60.0, |t| maintenant.duration_since(t).as_secs_f32());
        self.dernier_lissage = Some(maintenant);
        self.defilement += ecart * part_lissage(if dt > 0.05 { 1.0 / 60.0 } else { dt });
        true
    }

    /// Comme `animer`, sans rien faire avancer : le défilement lissé, l'apparition ou le vidage ont-ils encore à bouger ?
    pub fn en_mouvement(&self) -> bool {
        if !crate::reglages::animations_actives() {
            return false;
        }
        self.apparition.is_some_and(|t| t.elapsed() < DUREE_APPARITION)
            || self.vidage.is_some_and(|v| v.0.elapsed().as_millis() <= 1400)
            || (self.defilement_cible.clamp(0.0, self.defilement_max()) - self.defilement).abs() >= 0.5
    }

    /// Avancement de l'apparition de la ligne `i` : 0 = encore en points, 1 = affichée
    pub fn apparition_ligne(&self, i: usize, premiere: usize) -> f32 {
        let Some(t) = self.apparition else { return 1.0 };
        // Les lignes visibles apparaissent l'une après l'autre, du haut vers le bas
        let rang = i.saturating_sub(premiere).min(24) as f32;
        let ms = t.elapsed().as_secs_f32() * 1000.0 - rang * 11.0;
        (ms / 140.0).clamp(0.0, 1.0)
    }

    /// Ramène la ligne sélectionnée dans la zone visible
    pub fn montrer_selection(&mut self) {
        let Some(i) = self.selection else { return };
        let (par, h) = self.pas();
        let haut = (self.rang_de(i) / par) as f32 * h;
        let bas = haut + h;
        if haut < self.defilement_cible {
            self.defilement_cible = haut;
        } else if bas > self.defilement_cible + self.hauteur_liste {
            self.defilement_cible = bas - self.hauteur_liste;
        }
        self.defilement_cible = self.defilement_cible.clamp(0.0, self.defilement_max());
    }

    // ── Sélection ──

    /// Un fichier choisi ou ouvert par l'utilisateur n'est plus « NOUVEAU » (marque des téléchargements du navigateur). La
    /// sélection automatique d'une lecture de dossier ne passe pas ici : elle ne compte pas comme un regard.
    fn oublier_nouveau(&self, i: usize) {
        if let (Some(d), Some(e)) = (self.dossier(), self.entrees.get(i)) {
            crate::navigateur::oublier(d, &e.nom);
        }
    }

    fn oublier_nouveaux_choisis(&self) {
        if crate::navigateur::a_des_nouveaux() {
            for &i in &self.marques {
                self.oublier_nouveau(i);
            }
        }
    }

    pub fn choisir_seul(&mut self, i: usize) {
        self.selection = Some(i);
        self.marques.clear();
        self.marques.insert(i);
        self.ancre = i;
        self.oublier_nouveau(i);
    }

    /// Ctrl+clic : ajoute ou retire une ligne
    pub fn basculer(&mut self, i: usize) {
        if !self.marques.remove(&i) {
            self.marques.insert(i);
        }
        self.selection = Some(i);
        self.ancre = i;
        self.oublier_nouveau(i);
    }

    /// Maj+clic ou Maj+flèche : choisit toutes les lignes entre l'ancre et celle-ci
    pub fn etendre(&mut self, i: usize) {
        let (a, b) = (self.ancre.min(i), self.ancre.max(i));
        self.marques = (a..=b).filter(|&k| k < self.entrees.len()).collect();
        self.selection = Some(i);
        self.oublier_nouveaux_choisis();
    }

    pub fn tout_choisir(&mut self) {
        self.marques = (0..self.entrees.len()).collect();
        self.oublier_nouveaux_choisis();
    }

    pub fn rien_choisir(&mut self) {
        self.marques.clear();
    }

    pub fn deplacer_selection(&mut self, delta: i64, etendre: bool) {
        if self.entrees.is_empty() {
            return;
        }
        let n = self.entrees.len() as i64;
        let i = match self.selection {
            Some(i) => (i as i64).saturating_add(delta).clamp(0, n - 1),
            None => 0,
        } as usize;
        if etendre {
            self.etendre(i);
        } else {
            self.choisir_seul(i);
        }
        self.montrer_selection();
    }

    pub fn lignes_par_page(&self) -> i64 {
        let (par, h) = self.pas();
        ((self.hauteur_liste / h) as i64 - 1).max(1) * par as i64
    }

    /// Indices choisis, dans l'ordre d'affichage
    pub fn choisis(&self) -> Vec<usize> {
        let mut v: Vec<usize> = self.marques.iter().copied().filter(|&i| i < self.entrees.len()).collect();
        v.sort_unstable();
        v
    }

    pub fn chemins_choisis(&self) -> Vec<String> {
        self.choisis().into_iter().filter_map(|i| self.chemin_de(&self.entrees[i])).collect()
    }

    /// Dossier désigné par une zone (ligne de dossier, favori, disque, segment du chemin)
    pub fn chemin_cible(&self, c: Cible) -> Option<String> {
        let dossier = |e: &Emplacement| match e {
            Emplacement::Dossier(c) => Some(c.clone()),
            _ => None,
        };
        match c {
            Cible::Ligne(i) => self.entrees.get(i).filter(|e| e.est_conteneur()).and_then(|e| self.chemin_de(e)),
            Cible::Favori(i) => self.favoris.get(i).and_then(|f| dossier(&f.emplacement)),
            Cible::Epingle(i) => self.epingles.get(i).and_then(|f| dossier(&f.emplacement)),
            Cible::Disque(i) => self.disques.get(i).and_then(|e| self.chemin_de(e)),
            Cible::Segment(i) => self.zones.segments.get(i).and_then(|s| dossier(&s.1)),
            Cible::Nuage(i) => self.nuages.get(i).and_then(|l| dossier(&l.emplacement)),
            Cible::Serveur(i) => self.serveurs.get(i).and_then(|l| dossier(&l.emplacement)),
            Cible::FondListe => self.dossier().map(str::to_string),
            Cible::Arbre(i) => self.lignes_arbre().into_iter().nth(i).map(|l| l.chemin),
            _ => None,
        }
    }

    /// Lignes de l'arbre du volet : les disques en racines, le dossier ouvert déplié
    pub fn lignes_arbre(&self) -> Vec<crate::arbre::Ligne> {
        let racines: Vec<(String, String)> = self
            .disques
            .iter()
            .filter_map(|d| match &d.genre {
                Genre::Disque { racine, .. } => Some((racine.trim_end_matches('\\').to_string(), racine.clone())),
                _ => None,
            })
            .collect();
        crate::arbre::aplatir(&racines, self.dossier().unwrap_or(""))
    }

    /// Fichiers déposés par glisser-déposer
    pub fn deposer(&mut self, hwnd: HWND, sources: Vec<String>, dest: String, deplacer: bool) {
        if self.dossier().is_some_and(|d| d.trim_end_matches('\\').eq_ignore_ascii_case(dest.trim_end_matches('\\'))) {
            self.a_reselectionner = sources.iter().filter_map(|s| s.trim_end_matches('\\').rsplit('\\').next()).map(str::to_string).collect();
        }
        let genre = if deplacer { operations::Genre::Deplacer } else { operations::Genre::Copier };
        self.lancer_operation(hwnd, genre, sources, Some(dest), Trace::Noter);
    }

    /// Taille totale des fichiers choisis
    pub fn taille_choisie(&self) -> u64 {
        self.choisis().into_iter().map(|i| &self.entrees[i]).filter(|e| matches!(e.genre, Genre::Fichier)).map(|e| e.taille).sum()
    }

    // ── Presse-papiers et opérations ──

    pub fn annoncer(&mut self, texte: impl Into<String>, erreur: bool) {
        self.annonce = Some((texte.into(), erreur, Instant::now()));
    }

    fn pluriel(n: usize, mot: &str) -> String {
        format!("{} {mot}{}", crate::systeme::nombre(n), if n > 1 { "S" } else { "" })
    }

    pub fn copier(&mut self, hwnd: HWND, couper: bool) {
        if self.sans_fichiers() {
            return;
        }
        let chemins = self.chemins_choisis();
        if chemins.is_empty() {
            return;
        }
        if operations::copier_fichiers(hwnd, &chemins, couper) {
            self.coupes = if couper { chemins.iter().cloned().collect() } else { HashSet::new() };
            let n = chemins.len();
            self.annoncer(format!("{} {}", Self::pluriel(n, "ÉLÉMENT"), if couper { "COUPE" } else { "COPIE" }) + if n > 1 { "S" } else { "" }, false);
        } else {
            self.annoncer("PRESSE-PAPIERS INDISPONIBLE", true);
        }
    }

    pub fn coller(&mut self, hwnd: HWND) {
        let Some(dest) = self.dossier().map(str::to_string) else { return };
        let Some((sources, couper)) = operations::lire_fichiers(hwnd) else {
            self.annoncer("RIEN A COLLER", true);
            return;
        };
        // Les éléments collés seront choisis quand ils apparaîtront
        self.a_reselectionner = sources.iter().filter_map(|s| s.rsplit('\\').next()).map(str::to_string).collect();
        let genre = if couper { operations::Genre::Deplacer } else { operations::Genre::Copier };
        self.lancer_operation(hwnd, genre, sources, Some(dest), Trace::Noter);
        if couper {
            operations::vider(hwnd);
            self.coupes.clear();
        }
    }

    /// Plateau (Z10) : copie ou déplace ce qu'il contient vers le dossier ouvert (conflits et Ctrl+Z comme tout collage)
    pub fn deposer_plateau(&mut self, hwnd: HWND, deplacer: bool) {
        let Some(dest) = self.dossier().map(str::to_string) else {
            self.annoncer("OUVREZ UN DOSSIER POUR Y DEPOSER LE PLATEAU", true);
            return;
        };
        let sources: Vec<String> = crate::groupes::membres("PLATEAU").into_iter().filter(|s| std::path::Path::new(s).exists()).collect();
        if sources.is_empty() {
            self.annoncer("LE PLATEAU EST VIDE", true);
            return;
        }
        self.a_reselectionner = sources.iter().filter_map(|s| s.rsplit('\\').next()).map(str::to_string).collect();
        let genre = if deplacer { operations::Genre::Deplacer } else { operations::Genre::Copier };
        self.lancer_operation(hwnd, genre, sources, Some(dest), Trace::Noter);
    }

    /// Z8 : copie les dossiers de la liste de sauvegarde dans « SAUVEGARDE AAAA-MM-JJ » sous le dossier de destination
    /// (le NAS). Ne part que sur la commande de l'utilisateur ; le dossier daté est neuf, donc sans carte de conflits.
    pub fn sauvegarder(&mut self, hwnd: HWND) {
        let r = crate::reglages::actuels();
        let sources: Vec<String> = r.sauvegarde.iter().filter(|s| std::path::Path::new(s).exists()).cloned().collect();
        if r.sauvegarde_dest.is_empty() || sources.is_empty() {
            self.annoncer("DEFINISSEZ LA DESTINATION ET AJOUTEZ DES DOSSIERS A SAUVEGARDER (PALETTE : SAUVEGARDE)", true);
            return;
        }
        // shortcut: un NAS éteint bloque ce test quelques secondes sur le fil d'interface, à passer en fil si gênant
        if !std::path::Path::new(&r.sauvegarde_dest).is_dir() {
            self.annoncer("DESTINATION INJOIGNABLE : LE NAS EST-IL ALLUME ?", true);
            return;
        }
        // SAFETY: simple lecture de l'heure locale
        let t = unsafe { windows::Win32::System::SystemInformation::GetLocalTime() };
        let dest = dossier::joindre(&r.sauvegarde_dest, &format!("SAUVEGARDE {:04}-{:02}-{:02}", t.wYear, t.wMonth, t.wDay));
        if std::fs::create_dir_all(&dest).is_err() {
            self.annoncer("DOSSIER DE SAUVEGARDE NON CRÉÉ SUR LA DESTINATION", true);
            return;
        }
        self.lancer_operation(hwnd, operations::Genre::Copier, sources, Some(dest), Trace::Noter);
    }

    pub fn supprimer(&mut self, hwnd: HWND, definitif: bool) {
        if self.sans_fichiers() {
            return;
        }
        let chemins = self.chemins_choisis();
        if chemins.is_empty() {
            return;
        }
        let genre = if definitif { operations::Genre::SupprimerDefinitivement } else { operations::Genre::Corbeille };
        // Lot 12D : les lignes qui vont partir sont mémorisées pour jouer leur sortie (maquette l. 565) quand la liste se rafraîchit
        let sorties: Vec<(String, crate::mouvement::LigneSortante)> = self
            .choisis()
            .into_iter()
            .filter_map(|i| self.entrees.get(i))
            .map(|e| (crate::rendu::cle_flip(e), crate::mouvement::LigneSortante { entree: e.clone(), choisie: true, vue: 0 }))
            .collect();
        crate::mouvement::annoncer_sorties(sorties);
        self.lancer_operation(hwnd, genre, chemins, None, Trace::Noter);
    }

    /// Palette à étapes : met ces chemins précis à la corbeille (annulable), sans passer par la sélection
    pub fn supprimer_chemins(&mut self, hwnd: HWND, chemins: Vec<String>) {
        self.lancer_operation(hwnd, operations::Genre::Corbeille, chemins, None, Trace::Noter);
    }

    /// Palette à étapes : renomme ce chemin comme le renommage dans la liste (historique, groupes). La pastille « Annuler » n'est pas
    /// proposée : la palette a son propre bouton. Rend l'identifiant de l'entrée d'historique, que le « Annuler » du résultat défait.
    pub fn renommer_chemin(&mut self, hwnd: HWND, chemin: &str, nouveau: &str) -> Result<u64, String> {
        let nouveau = nouveau.trim();
        if !operations::nom_valide(nouveau) {
            return Err("Nom invalide (interdits : \\ / : * ? \" < > |, noms réservés de Windows)".to_string());
        }
        let (d, ancien) = (dossier::dossier_de(chemin).to_string(), crate::assistant::nom_de(chemin));
        operations::renommer(&d, &ancien, nouveau).map_err(|e| e.to_lowercase())?;
        let id = historique::noter(Action::Renommage { dossier: d.clone(), ancien: ancien.clone(), nouveau: nouveau.to_string() });
        crate::groupes::suivre(&dossier::joindre(&d, &ancien), &dossier::joindre(&d, nouveau));
        if self.dossier().is_some_and(|o| o.eq_ignore_ascii_case(&d)) {
            self.a_reselectionner = vec![nouveau.to_string()];
            self.actualiser(hwnd);
        }
        Ok(id)
    }

    /// Palette à étapes : crée le dossier `nom` dans `dans` (Ctrl+Z le retire s'il est encore vide). `create_dir` refuse un nom pris,
    /// donc rien n'est jamais écrasé. Rend l'identifiant de l'entrée d'historique.
    pub fn creer_dossier_nomme(&mut self, hwnd: HWND, dans: &str, nom: &str) -> Result<u64, String> {
        let nom = nom.trim();
        if !operations::nom_valide(nom) {
            return Err("Nom invalide (interdits : \\ / : * ? \" < > |, noms réservés de Windows)".to_string());
        }
        let chemin = dossier::joindre(dans, nom);
        std::fs::create_dir(&chemin).map_err(|e| operations::message_io(&e).to_lowercase())?;
        let id = historique::noter(Action::NouveauDossier { chemin });
        if self.dossier().is_some_and(|o| o.eq_ignore_ascii_case(dans.trim_end_matches('\\'))) {
            self.actualiser(hwnd);
        }
        Ok(id)
    }

    /// Palette à étapes : compresse ces éléments (tous du même dossier) en .zip dans `dest`, en tâche de fond ; le message revient par `Fini`
    pub fn compresser_vers(&mut self, hwnd: HWND, chemins: Vec<String>, dest: String, dossier_seul: bool) {
        let Some(premier) = chemins.first() else { return };
        let source = dossier::dossier_de(premier).to_string();
        let noms: Vec<String> = chemins.iter().map(|c| crate::assistant::nom_de(c)).collect();
        let base = if noms.len() == 1 { crate::lot::decouper(&noms[0], dossier_seul).0.to_string() } else { dossier::nom_affiche(&source) };
        // Le choix du nom libre (`nom_libre` teste le disque) et l'écriture du zip se font dans le fil de fond, pas sur le fil d'interface
        tache_palette(hwnd, move || {
            let nom = crate::archive::nom_libre(&dest, &base, ".zip");
            match crate::archive::creer(&dossier::joindre(&dest, &nom), &source, &noms) {
                Ok(()) => Fini { message: format!("{} CRÉÉ", nom.to_uppercase()), erreur: false, copier: None, lancer: None },
                Err(e) => Fini { message: e, erreur: true, copier: None, lancer: None },
            }
        });
    }

    /// Palette à étapes, « Annuler » du résultat : défait l'entrée d'historique `id` (celle que l'opération de la palette a notée),
    /// jamais « la dernière de la pile », que d'autres actions ont pu recouvrir depuis. Vrai si elle l'a été (ou lancée en fond).
    pub fn annuler_pour_palette(&mut self, hwnd: HWND, id: u64) -> bool {
        self.defaire_entree(hwnd, id)
    }

    /// La prochaine opération qui part dans cet onglet est celle de la palette (voir `palette_arme`)
    pub fn armer_palette(&mut self) {
        self.palette_arme = true;
    }

    /// Après l'appel qui devait lancer l'opération de la palette : si rien ne tourne ni n'attend (rien à faire, tout ignoré, annulation
    /// faite sur place), la marque ne doit pas rester pour l'opération de fond suivante.
    pub fn desarmer_palette(&mut self) {
        if self.operation.is_none() && self.conflits.is_none() {
            self.palette_arme = false;
        }
    }

    /// L'opération `numero` est-elle celle que la palette attend ?
    pub fn est_palette(&self, numero: u64) -> bool {
        self.palette_numero == Some(numero)
    }

    /// Démarre l'opération, ou la met en file d'attente si une autre tourne dans cet onglet
    /// Copie et déplacement : les noms déjà pris sont d'abord demandés (carte des conflits)
    fn lancer_operation(&mut self, hwnd: HWND, genre: operations::Genre, sources: Vec<String>, dest: Option<String>, trace: Trace) {
        self.lancer_puis(hwnd, genre, sources, dest, trace, Vec::new());
    }

    /// Pareil, puis sort `apres` de la corbeille une fois fini
    fn lancer_puis(&mut self, hwnd: HWND, genre: operations::Genre, sources: Vec<String>, dest: Option<String>, trace: Trace, apres: Vec<(String, String)>) {
        if self.operation.is_some() || self.conflits.is_some() {
            self.file.push_back(Attente { genre, sources, dest, trace, apres });
            self.annoncer(format!("{} EN FILE D'ATTENTE ({})", genre.libelle(), self.file.len()), false);
            return;
        }
        let Some(d) = dest.filter(|_| matches!(genre, operations::Genre::Copier | operations::Genre::Deplacer)) else {
            let elements = sources.into_iter().map(|s| operations::Element::simple(s, None)).collect();
            return self.lancer_elements(hwnd, genre, elements, Vec::new(), trace, apres);
        };
        let mut c = crate::conflit::preparer(genre, &sources, &d, trace);
        c.apres = apres;
        if c.liste.is_empty() {
            self.lancer_elements(hwnd, genre, c.libres, c.vides, trace, c.apres);
        } else if let Some(choix) = crate::conflit::Choix::par_defaut(crate::reglages::actuels().conflit_defaut) {
            // Réponse réglée d'avance : pas de carte, l'opération part avec ce choix pour tous les noms pris
            c.choisir(choix, true);
            let (vides, apres) = (c.vides.clone(), c.apres.clone());
            self.lancer_elements(hwnd, genre, c.elements(), vides, trace, apres);
        } else {
            crate::sons::jouer(crate::sons::Son::Tic);
            self.conflits = Some(c);
        }
    }

    /// Conflits réglés (ou opération sans conflit) : départ
    pub fn lancer_elements(&mut self, hwnd: HWND, genre: operations::Genre, elements: Vec<operations::Element>, vides: Vec<String>, trace: Trace, apres: Vec<(String, String)>) {
        if elements.is_empty() {
            self.palette_arme = false;
            self.annoncer(format!("{} : RIEN A FAIRE", genre.libelle()), false);
            return self.suivante(hwnd);
        }
        let controle = Controle::default();
        let n = numero();
        // L'opération de la palette porte sa marque ; une opération de fond lancée entre-temps (file) ne la reçoit pas
        if std::mem::take(&mut self.palette_arme) {
            self.palette_numero = Some(n);
            self.palette_entree = None;
        }
        let destination = elements.first().and_then(|e| e.dest.clone()).filter(|_| matches!(genre, operations::Genre::Copier | operations::Genre::Deplacer));
        let nom_court = crate::pastilles::nom_court(genre.libelle(), elements.len());
        self.fin_op = None;
        self.operation = Some(Operation { avancement: 0.0, controle: controle.clone(), numero: n, nom_court, destination, debut: Instant::now(), trace, apres });
        operations::lancer(hwnd, n, genre, elements, vides, controle);
        // Pixel glisse et la grille 3 × 3 tourne tant que l'opération dure : une minuterie de 100 ms, détruite à la fin
        // SAFETY: fil d'interface, `hwnd` est la fenêtre principale vivante ; SetTimer ne garde aucune référence
        unsafe { crate::minuteries::armer_activite(hwnd) };
    }

    /// Carte des conflits fermée par Échap : l'opération est abandonnée
    pub fn abandonner_conflits(&mut self, hwnd: HWND) {
        if let Some(c) = self.conflits.take() {
            self.annoncer(format!("{} ANNULEE", c.genre.libelle()), true);
            self.palette_arme = false;
        }
        self.suivante(hwnd);
    }

    /// Démarre l'opération suivante de la file
    fn suivante(&mut self, hwnd: HWND) {
        if self.operation.is_none() && self.conflits.is_none() {
            if let Some(a) = self.file.pop_front() {
                self.lancer_puis(hwnd, a.genre, a.sources, a.dest, a.trace, a.apres);
            }
        }
    }

    /// Opérations en attente derrière celle en cours
    pub fn en_file(&self) -> usize {
        self.file.len()
    }

    /// Pastilles de cet onglet : l'opération qui tourne, celles qui attendent derrière, la fin ou l'échec de la dernière
    pub fn pastilles_propres(&self) -> Vec<crate::pastilles::Pastille> {
        use crate::pastilles::{expiree, nom_court, Genre, Pastille};
        let mut v = Vec::new();
        if let Some(op) = &self.operation {
            v.push(Pastille::operation(op.nom_court.clone(), op.avancement, self.en_pause(), op.destination.clone()).avec_cle(op.numero));
        }
        for a in &self.file {
            v.push(Pastille::attente(nom_court(a.genre.libelle(), a.sources.len())));
        }
        if let Some(f) = &self.fin_op {
            match &f.erreur {
                Some(e) if !expiree(Genre::Erreur, f.quand.elapsed().as_millis()) => v.push(Pastille::erreur(f.libelle.clone(), e.clone())),
                None if !expiree(Genre::Fin, f.quand.elapsed().as_millis()) => v.push(Pastille::fin(f.libelle.clone(), f.destination.clone())),
                _ => {}
            }
        }
        v
    }

    /// Une opération tourne dans cet onglet (Pixel glisse, la grille tourne)
    pub fn en_activite(&self) -> bool {
        self.operation.is_some()
    }

    /// Retire la pastille de fin ou d'erreur qui a fait son temps ; vrai si une pastille temporaire reste à surveiller
    pub fn expirer_pastilles(&mut self) -> bool {
        use crate::pastilles::{expiree, Genre};
        if let Some(f) = &self.fin_op {
            let genre = if f.erreur.is_some() { Genre::Erreur } else { Genre::Fin };
            if expiree(genre, f.quand.elapsed().as_millis()) {
                self.fin_op = None;
            }
        }
        self.fin_op.is_some()
    }

    /// Pixel : la dernière opération de cet onglet s'est mal terminée et sa pastille est encore là
    pub fn en_erreur(&self) -> bool {
        self.fin_op.as_ref().is_some_and(|f| crate::pastilles::erreur_vivante(f.erreur.is_some(), f.quand.elapsed().as_millis()))
    }

    /// Pixel : une opération a réussi il y a moins de 1,5 s
    pub fn reussite_recente(&self) -> bool {
        self.fin_op.as_ref().is_some_and(|f| f.erreur.is_none() && f.quand.elapsed().as_millis() < crate::pixel::REUSSITE_MS)
    }

    /// Pause / reprise de l'opération en cours (touche Pause, bouton de la pastille)
    pub fn basculer_pause(&mut self) {
        let Some(op) = &self.operation else { return };
        let pause = !op.controle.pause.load(Ordering::Relaxed);
        op.controle.pause.store(pause, Ordering::Relaxed);
        self.annoncer(if pause { "EN PAUSE · PAUSE OU BOUTON REPRISE DE LA PASTILLE POUR REPRENDRE" } else { "REPRISE" }, false);
    }
}

impl App {
    pub fn en_pause(&self) -> bool {
        self.operation.as_ref().is_some_and(|o| o.controle.pause.load(Ordering::Relaxed))
    }

    pub fn progres(&mut self, millieme: usize) {
        if let Some(op) = &mut self.operation {
            op.avancement = millieme as f32 / 1000.0;
        }
    }

    /// Échap pendant une opération : on demande l'arrêt (et la file d'attente est vidée)
    pub fn annuler_operation(&mut self) -> bool {
        if let Some(op) = &self.operation {
            op.controle.annule.store(true, Ordering::Relaxed);
            op.controle.pause.store(false, Ordering::Relaxed);
            self.file.clear();
            return true;
        }
        false
    }

    pub fn fin_operation(&mut self, hwnd: HWND, b: Bilan) {
        let (trace, apres, nom_court, destination) = self.operation.take().map_or((Trace::Aucune, Vec::new(), String::new(), None), |o| (o.trace, o.apres, o.nom_court, o.destination));
        // C'est l'opération de la palette ? Alors l'entrée d'historique qu'elle note ici est celle que son « Annuler » défera
        let de_la_palette = self.palette_numero == Some(b.numero);
        if de_la_palette {
            self.palette_numero = None;
        }
        // Ce qui a été fait (même en partie) peut être défait avec Ctrl+Z (ou refait avec Ctrl+Y)
        let faits: Vec<(String, String)> = b.resultats.iter().filter_map(|(a, n)| n.clone().map(|n| (a.clone(), n))).collect();
        let remplaces = b.remplaces;
        let n_faits = faits.len();
        // Un déplacement (et son annulation, qui est un déplacement en sens inverse) emmène les étiquettes avec les fichiers ; une
        // copie non : c'est un autre fichier, qui n'a pas choisi ses carrés
        if b.genre == operations::Genre::Deplacer {
            crate::etiquettes::suivre_paires(&faits);
        }
        let action = match b.genre {
            _ if faits.is_empty() => None,
            operations::Genre::Copier => Some(Action::Copie { crees: faits.into_iter().map(|p| p.1).collect(), remplaces }),
            operations::Genre::Deplacer => Some(Action::Deplacement { paires: faits, remplaces }),
            operations::Genre::Corbeille if apres.is_empty() => Some(Action::Corbeille { paires: faits }),
            // Ctrl+Z d'un remplacement : les copies sont parties, les anciens reviennent ; l'inverse refait l'échange
            operations::Genre::Corbeille => Some(Action::Copie { crees: apres.iter().map(|p| p.0.clone()).collect(), remplaces: faits }),
            operations::Genre::SupprimerDefinitivement => None,
        };
        if !apres.is_empty() {
            let _ = historique::restaurer(&apres);
        }
        match (action, trace) {
            (Some(a), Trace::Noter) => {
                // Pastille « ANNULER » : seulement pour une action de l'utilisateur, pas pour un Ctrl+Z ou Ctrl+Y
                let verbe = match b.genre {
                    operations::Genre::Corbeille => "SUPPRIMÉ",
                    operations::Genre::Deplacer => "DÉPLACÉ",
                    _ => "COPIÉ",
                };
                let id = historique::noter(a);
                if de_la_palette {
                    self.palette_entree = Some(id);
                }
                if !b.annule && b.erreur.is_none() {
                    proposer_annulation(crate::pastilles::nom_court(verbe, n_faits));
                }
            }
            (Some(a), Trace::Empiler(p, libelle, id)) => historique::empiler(p, a, libelle, id),
            _ => {}
        }
        let sans_erreur = b.erreur.is_none();
        if let Some(e) = b.erreur {
            self.fin_op = Some(FinOp { libelle: nom_court, destination: None, erreur: Some(e.clone()), quand: Instant::now() });
            self.annoncer(e, true);
        } else if b.annule {
            self.annoncer(format!("{} ANNULEE", b.genre.libelle()), true);
        } else {
            self.fin_op = Some(FinOp { libelle: nom_court, destination, erreur: None, quand: Instant::now() });
            let fait = match b.genre {
                operations::Genre::Copier => "COPIE",
                operations::Genre::Deplacer => "DEPLACE",
                operations::Genre::Corbeille => "MIS A LA CORBEILLE",
                operations::Genre::SupprimerDefinitivement => "SUPPRIME",
            };
            let s = if b.nombre > 1 && fait != "MIS A LA CORBEILLE" { "S" } else { "" };
            self.annoncer(format!("{} {fait}{s}", Self::pluriel(b.nombre, "ÉLÉMENT")), false);
        }
        // Copie vérifiée (réglage, désactivé par défaut) : vers un NAS ou une clé, chaque fichier est comparé à son original
        if b.genre == operations::Genre::Copier && !b.annule && sans_erreur && crate::reglages::actuels().copie_verifiee {
            let paires: Vec<(String, String)> = b.resultats.iter().filter_map(|(a, n)| n.clone().map(|n| (a.clone(), n))).filter(|(_, n)| operations::support_externe(n)).collect();
            if !paires.is_empty() {
                self.annoncer("COPIE TERMINEE · VERIFICATION DES EMPREINTES…", false);
                tache(hwnd, move || {
                    let (bons, mauvais) = outils::verifier_copies(&paires);
                    if mauvais.is_empty() {
                        Fini { message: format!("COPIE VERIFIEE · {} IDENTIQUE{} A L'ORIGINAL", Self::pluriel(bons, "FICHIER"), if bons > 1 { "S" } else { "" }), erreur: false, copier: None, lancer: None }
                    } else {
                        let noms = mauvais.iter().take(3).cloned().collect::<Vec<_>>().join(", ").to_uppercase();
                        Fini { message: format!("ATTENTION : {} DIFFERENT OU MANQUE : {noms}", Self::pluriel(mauvais.len(), "FICHIER")), erreur: true, copier: None, lancer: None }
                    }
                });
            }
        }
        // Téléchargements : ce qui vient d'être copié ailleurs part à la corbeille (réglage, désactivé par défaut)
        if b.genre == operations::Genre::Copier && !b.annule && sans_erreur && crate::reglages::actuels().nettoyer_copies {
            let partis: Vec<String> = b
                .resultats
                .iter()
                .filter(|(a, n)| n.as_deref().is_some_and(|n| !crate::systeme::dossier_telechargements(dossier::dossier_de(n))) && crate::systeme::dossier_telechargements(dossier::dossier_de(a)))
                .filter(|(a, _)| std::path::Path::new(a).is_file())
                .map(|(a, _)| a.clone())
                .collect();
            if !partis.is_empty() {
                self.nettoyage = partis;
                self.nettoyer_telechargements(hwnd);
            }
        }
        // La surveillance actualise déjà, mais un dossier réseau peut ne rien signaler
        self.actualiser(hwnd);
        self.suivante(hwnd);
    }

    /// L'opération `numero` est-elle celle de cet onglet ?
    pub fn possede(&self, numero: u64) -> bool {
        self.operation.as_ref().is_some_and(|o| o.numero == numero)
    }

    /// Ctrl+Z : défait la dernière action sur les fichiers
    pub fn annuler_derniere(&mut self, hwnd: HWND) {
        self.defaire(hwnd, historique::Pile::Annuler);
    }

    /// Panneau Historique, bouton « Annuler » d'une entrée : défait cette action-là, les autres restent. Rend vrai si elle l'a été.
    pub fn defaire_entree(&mut self, hwnd: HWND, id: u64) -> bool {
        if !historique::remonter_id(historique::Pile::Annuler, id) {
            // Plus dans la pile (session finie, pile pleine, déjà défaite avec Ctrl+Z) : le bouton n'a plus lieu d'être
            crate::journal_historique::marquer(id, crate::journal_historique::Etat::Passe);
            self.annoncer("CETTE ACTION NE PEUT PLUS ETRE ANNULEE", true);
            return false;
        }
        let fait = self.defaire(hwnd, historique::Pile::Annuler);
        if fait {
            toaster("Action annulée");
        }
        fait
    }

    /// Ctrl+Y : refait ce que Ctrl+Z vient de défaire
    pub fn refaire(&mut self, hwnd: HWND) {
        self.defaire(hwnd, historique::Pile::Refaire);
    }

    /// Défait l'action en haut de `pile` ; son inverse va dans l'autre pile. Vrai si elle a été défaite (ou lancée, pour une copie
    /// ou un déplacement qui se jouent en tâche de fond).
    fn defaire(&mut self, hwnd: HWND, pile: historique::Pile) -> bool {
        let Some((a, libelle, id)) = historique::reprendre(pile) else {
            self.annoncer(if pile == historique::Pile::Annuler { "RIEN A ANNULER" } else { "RIEN A REFAIRE" }, false);
            return false;
        };
        let autre = pile.autre();
        let inverse = |a: Action| historique::empiler(autre, a, libelle, id);
        let trace = Trace::Empiler(autre, libelle, id);
        let r: Result<(), String> = match a {
            Action::RenommageLot { dossier, paires } => {
                let inv: Vec<(String, String)> = paires.iter().map(|(a, b)| (b.clone(), a.clone())).collect();
                let r = crate::lot::renommer(&dossier, &inv);
                if r.is_ok() {
                    inverse(Action::RenommageLot { dossier: dossier.clone(), paires: inv });
                }
                if self.dossier().is_some_and(|d| d.eq_ignore_ascii_case(&dossier)) {
                    self.actualiser(hwnd);
                }
                r
            }
            Action::Renommage { dossier, ancien, nouveau } => {
                let r = operations::renommer(&dossier, &nouveau, &ancien);
                if r.is_ok() {
                    inverse(Action::Renommage { dossier: dossier.clone(), ancien: nouveau, nouveau: ancien.clone() });
                    if self.dossier().is_some_and(|d| d.eq_ignore_ascii_case(&dossier)) {
                        self.a_reselectionner = vec![ancien];
                        self.actualiser(hwnd);
                    }
                }
                r
            }
            // Seulement s'il est encore vide : on ne jette jamais ce qu'on y a mis depuis
            Action::NouveauDossier { chemin } => match std::fs::remove_dir(&chemin) {
                Ok(()) => {
                    inverse(Action::DossierRetire { chemin });
                    Ok(())
                }
                Err(_) => Err("LE DOSSIER N'EST PLUS VIDE : LAISSE EN PLACE".to_string()),
            },
            Action::DossierRetire { chemin } => match std::fs::create_dir(&chemin) {
                Ok(()) => {
                    inverse(Action::NouveauDossier { chemin });
                    Ok(())
                }
                Err(_) => Err("IMPOSSIBLE DE RECREER LE DOSSIER".to_string()),
            },
            Action::Copie { crees, remplaces } => {
                self.lancer_puis(hwnd, operations::Genre::Corbeille, crees, None, trace, remplaces);
                Ok(())
            }
            Action::Deplacement { paires, remplaces } => {
                // Retour dans chaque dossier d'origine (une opération par dossier) ; un dossier vidé par
                // une fusion a été retiré : on le recrée
                let mut groupes: Vec<(String, Vec<String>)> = Vec::new();
                for (origine, actuel) in paires {
                    let parent = dossier::dossier_de(&origine).to_string();
                    match groupes.iter_mut().find(|g| g.0.eq_ignore_ascii_case(&parent)) {
                        Some(g) => g.1.push(actuel),
                        None => groupes.push((parent, vec![actuel])),
                    }
                }
                // Les éléments remplacés reviennent après le dernier retour
                let n = groupes.len();
                let mut remplaces = Some(remplaces);
                for (k, (dest, sources)) in groupes.into_iter().enumerate() {
                    let _ = std::fs::create_dir_all(&dest);
                    let apres = if k + 1 == n { remplaces.take().unwrap_or_default() } else { Vec::new() };
                    self.lancer_puis(hwnd, operations::Genre::Deplacer, sources, Some(dest), trace, apres);
                }
                Ok(())
            }
            Action::Corbeille { paires } => {
                let r = historique::restaurer(&paires).map(|_| ());
                if r.is_ok() {
                    // Remettre à la corbeille ce qui vient d'en sortir
                    inverse(Action::Copie { crees: paires.iter().map(|p| p.0.clone()).collect(), remplaces: Vec::new() });
                    self.a_reselectionner = paires.iter().filter_map(|p| p.0.rsplit('\\').next()).map(str::to_string).collect();
                    self.actualiser(hwnd);
                }
                r
            }
        };
        let fait = if pile == historique::Pile::Annuler { "ANNULE" } else { "RETABLI" };
        if pile == historique::Pile::Annuler {
            oublier_annulation();
        }
        // Panneau Historique : l'entrée passe à « Annulé · … » (Ctrl+Z, bouton) ou redevient annulable (Ctrl+Y) ; si l'inverse a échoué
        // l'action n'est plus dans aucune pile, son bouton disparaît
        use crate::journal_historique::{marquer, Etat};
        match r {
            Ok(()) => {
                marquer(id, if pile == historique::Pile::Annuler { Etat::Annule } else { Etat::Annulable });
                self.annoncer(format!("{fait} : {libelle}"), false);
                crate::sons::jouer(crate::sons::Son::Tic);
                true
            }
            Err(e) => {
                marquer(id, Etat::Passe);
                self.annoncer(e, true);
                crate::sons::jouer(crate::sons::Son::Erreur);
                false
            }
        }
    }

    pub fn copier_chemin(&mut self, hwnd: HWND) {
        let chemins = self.chemins_choisis();
        let texte = if chemins.is_empty() { self.dossier().unwrap_or_default().to_string() } else { chemins.join("\r\n") };
        if !texte.is_empty() {
            operations::copier_texte(hwnd, &texte);
            self.annoncer("CHEMIN COPIE", false);
            toaster("Chemin copié");
        }
    }

    pub fn nouveau_dossier(&mut self) {
        let Some(d) = self.dossier().map(str::to_string) else { return };
        match operations::nouveau_dossier(&d) {
            Ok(nom) => {
                historique::noter(Action::NouveauDossier { chemin: dossier::joindre(&d, &nom) });
                self.renommer_apres = Some(nom);
            }
            Err(e) => self.annoncer(e, true),
        }
    }

    /// Nouveau document texte, aussitôt en renommage (Ctrl+Z : à la corbeille)
    pub fn nouveau_fichier(&mut self) {
        let Some(d) = self.dossier().map(str::to_string) else { return };
        match operations::nouveau_fichier(&d) {
            Ok(nom) => {
                historique::noter(Action::Copie { crees: vec![dossier::joindre(&d, &nom)], remplaces: Vec::new() });
                self.renommer_apres = Some(nom);
            }
            Err(e) => self.annoncer(e, true),
        }
    }

    /// Tri choisi dans le menu « Trier » (colonne et sens)
    pub fn trier(&mut self, col: Colonne, croissant: bool) {
        // trier_par inverse le sens quand la colonne ne change pas : on part de l'état opposé
        self.tri = (col, !croissant, self.tri.2);
        self.trier_par(col);
        self.tri = (col, croissant, self.tri.2);
    }

    /// Fichiers et dossiers mélangés, ou dossiers d'abord
    pub fn basculer_melange(&mut self) {
        self.tri = (self.tri.0, !self.tri.1, !self.tri.2);
        self.trier_par(self.tri.0);
        self.annoncer(if self.tri.2 { "FICHIERS ET DOSSIERS MELANGES" } else { "DOSSIERS D'ABORD" }, false);
    }

    pub fn inverser_selection(&mut self) {
        self.marques = (0..self.entrees.len()).filter(|i| !self.marques.contains(i)).collect();
    }

    /// Bouton de la barre d'actions utilisable ici ?
    pub fn outil_possible(&self, o: crate::barre::Outil) -> bool {
        use crate::barre::Outil;
        // Appelé à chaque image : un seul élément choisi suffit à décider (pas de liste de chemins)
        let choix = !self.sans_fichiers() && self.marques.iter().find_map(|&i| self.entrees.get(i)).is_some_and(|e| self.chemin_de(e).is_some());
        let dossier = self.dossier().is_some();
        match o {
            Outil::Nouveau => dossier,
            Outil::Couper | Outil::Copier | Outil::Supprimer | Outil::Partager => choix,
            Outil::Renommer => choix && dossier,
            Outil::Coller => dossier && operations::fichiers_a_coller(),
            Outil::Proprietes => choix || dossier,
            _ => true,
        }
    }

    /// Bouton de vue allumé (vue en cours)
    pub fn outil_allume(&self, o: crate::barre::Outil) -> bool {
        use crate::barre::Outil;
        match o {
            Outil::VueListe => self.affichage == Affichage::Liste && !self.tailles,
            Outil::VueGrille => self.affichage == Affichage::Grille,
            Outil::VueColonnes => self.affichage == Affichage::Colonnes,
            Outil::VueTailles => self.tailles,
            _ => false,
        }
    }

    // ── Renommage ──

    pub fn commencer_renommage(&mut self) {
        if self.sans_fichiers() {
            return;
        }
        if let Some(i) = self.selection {
            let e = &self.entrees[i];
            self.edition = Some(Edition::nouveau(i, &e.nom, e.est_conteneur()));
            self.choisir_seul(i);
        }
    }

    pub fn annuler_renommage(&mut self) {
        self.edition = None;
    }

    pub fn valider_renommage(&mut self) {
        let Some(ed) = self.edition.take() else { return };
        let nouveau = ed.valeur().trim().to_string();
        if nouveau == ed.original {
            return;
        }
        if !operations::nom_valide(&nouveau) {
            self.annoncer("NOM INVALIDE (INTERDITS : \\ / : * ? \" < > |)", true);
            return;
        }
        let Some(d) = self.dossier().map(str::to_string) else { return };
        match operations::renommer(&d, &ed.original, &nouveau) {
            Ok(()) => {
                historique::noter(Action::Renommage { dossier: d.clone(), ancien: ed.original.clone(), nouveau: nouveau.clone() });
                proposer_annulation(crate::pastilles::nom_court("RENOMMÉ", 1));
                // Les groupes suivent le nouveau nom
                crate::groupes::suivre(&dossier::joindre(&d, &ed.original), &dossier::joindre(&d, &nouveau));
                // Le nouveau nom est resélectionné à l'actualisation
                if let Some(e) = self.entrees.get_mut(ed.index) {
                    e.nom = nouveau.clone();
                    e.nom_w = nouveau.encode_utf16().chain(std::iter::once(0)).collect();
                }
                self.annoncer("RENOMME", false);
                toaster(format!("Renommé en « {nouveau} »"));
            }
            Err(e) => self.annoncer(e, true),
        }
    }

    // ── Volet ──

    /// Entrée du volet qui correspond au dossier ouvert : un favori exact, sinon le stockage
    /// le plus précis qui le contient (OneDrive, un partage…), sinon son disque
    pub fn actif(&self, quoi: Cible) -> bool {
        if let Emplacement::Groupe(n) = &self.emplacement {
            return matches!(quoi, Cible::Groupe(i) if self.groupes.get(i).is_some_and(|g| &g.nom == n));
        }
        if self.favoris.iter().any(|f| f.emplacement == self.emplacement) {
            return matches!(quoi, Cible::Favori(i) if self.favoris[i].emplacement == self.emplacement);
        }
        if self.epingles.iter().any(|f| f.emplacement == self.emplacement) {
            return matches!(quoi, Cible::Epingle(i) if self.epingles[i].emplacement == self.emplacement);
        }
        let ouvert = match &self.emplacement {
            Emplacement::Dossier(c) | Emplacement::Recherche { racine: c, .. } => c.clone(),
            Emplacement::Serveur(h) => format!("\\\\{h}"),
            _ => return false,
        };
        // Le dossier ouvert est-il `racine` ou un de ses sous-dossiers ?
        let sous = |racine: &str| {
            let (c, r) = (ouvert.to_uppercase(), racine.trim_end_matches('\\').to_uppercase());
            c == r || c.starts_with(&format!("{r}\\"))
        };
        let lieux = self.nuages.iter().enumerate().map(|(i, l)| (Cible::Nuage(i), l)).chain(self.serveurs.iter().enumerate().map(|(i, l)| (Cible::Serveur(i), l)));
        let meilleur = lieux.filter(|(_, l)| sous(&l.detail)).max_by_key(|(_, l)| l.detail.len()).map(|(c, _)| c);
        match quoi {
            Cible::Nuage(_) | Cible::Serveur(_) => meilleur == Some(quoi),
            Cible::Disque(i) if meilleur.is_none() => match self.disques.get(i).map(|d| &d.genre) {
                Some(Genre::Disque { racine, .. }) => sous(racine),
                _ => false,
            },
            _ => false,
        }
    }

    /// Ouvre un stockage du volet (cloud ou serveur)
    pub fn ouvrir_lieu(&mut self, hwnd: HWND, quoi: Cible) {
        let lieu = match quoi {
            Cible::Nuage(i) => self.nuages.get(i),
            Cible::Serveur(i) => self.serveurs.get(i),
            _ => None,
        };
        if let Some(emp) = lieu.map(|l| l.emplacement.clone()) {
            self.naviguer(hwnd, emp);
        }
    }

    pub fn ouvrir_epingle(&mut self, hwnd: HWND, i: usize) {
        if let Some(f) = self.epingles.get(i) {
            let emp = f.emplacement.clone();
            self.naviguer(hwnd, emp);
        }
    }

    pub fn ouvrir_favori(&mut self, hwnd: HWND, i: usize) {
        if let Some(f) = self.favoris.get(i) {
            let emp = f.emplacement.clone();
            self.naviguer(hwnd, emp);
        }
    }

    pub fn ouvrir_disque(&mut self, hwnd: HWND, i: usize) {
        if let Some(Genre::Disque { racine, .. }) = self.disques.get(i).map(|d| &d.genre) {
            let emp = Emplacement::dossier(racine);
            self.naviguer(hwnd, emp);
        }
    }

    // ── Filtre et recherche ──

    fn filtrees(&self) -> Vec<Entree> {
        let essentiel = self.essentiel_ici();
        if self.filtre.is_empty() && self.jour.is_none() && !essentiel {
            return self.tout.clone();
        }
        let analyse = dossier::analyser_filtre(&self.filtre);
        // Mode Essentiel : ce qui a bougé ces 7 derniers jours, et les épinglés
        let epingles: Vec<String> = if essentiel { crate::reglages::actuels().epingles.iter().map(|c| c.trim_end_matches('\\').to_lowercase()).collect() } else { Vec::new() };
        let maintenant = crate::systeme::maintenant();
        let limite = maintenant.saturating_sub(7 * 24 * 3600 * 10_000_000);
        self.tout
            .iter()
            .filter(|e| analyse.est_vide() || analyse.accepte(e, maintenant))
            .filter(|e| analyse.etiquette().map_or(true, |n| self.chemin_de(e).and_then(|c| crate::etiquettes::de(&c)) == Some(n)))
            .filter(|e| self.jour.map_or(true, |j| crate::systeme::numero_jour(e.date_colonne()) == j))
            .filter(|e| !essentiel || e.date_colonne() >= limite || self.chemin_de(e).is_some_and(|c| epingles.contains(&c.trim_end_matches('\\').to_lowercase())))
            .cloned()
            .collect()
    }

    /// Le mode Essentiel s'applique-t-il ici ? (dossiers, groupes, résultats)
    pub fn essentiel_ici(&self) -> bool {
        ESSENTIEL.load(Ordering::Relaxed) && matches!(self.emplacement, Emplacement::Dossier(_) | Emplacement::Groupe(_) | Emplacement::Globale { .. })
    }

    /// Chronologie : ne montrer que les éléments de ce jour (de nouveau : tout remontrer)
    pub fn choisir_jour(&mut self, j: u32) {
        self.jour = if self.jour == Some(j) { None } else { Some(j) };
        self.appliquer_filtre();
        let t = match self.jour {
            Some(j) => format!("{} · {} SEULEMENT · CLIC DE NOUVEAU POUR TOUT REVOIR", crate::systeme::jour_court(j), Self::pluriel(self.entrees.len(), "ÉLÉMENT")),
            None => "TOUS LES JOURS".to_string(),
        };
        self.annoncer(t, false);
    }

    /// La chronologie a-t-elle sa place ici ? (dossiers et résultats, en liste ou en grille)
    pub fn avec_chronologie(&self) -> bool {
        crate::reglages::CHRONOLOGIE.load(Ordering::Relaxed)
            && self.affichage != Affichage::Colonnes
            && !matches!(self.emplacement, Emplacement::CePC | Emplacement::Reglages | Emplacement::Serveur(_) | Emplacement::Doublons(_))
    }

    /// Le filtre a changé : la liste se réduit tout de suite, la première entrée est choisie
    fn appliquer_filtre(&mut self) {
        crate::mouvement::demander_flip();
        self.edition = None;
        self.rebatir();
        self.marques.clear();
        self.selection = None;
        if !self.entrees.is_empty() {
            self.choisir_seul(0);
        }
        self.defilement_cible = 0.0;
    }

    pub fn taper_filtre(&mut self, c: char) {
        self.filtre.push(c);
        self.appliquer_filtre();
    }

    /// Retour arrière dans le filtre. Faux s'il était déjà vide.
    pub fn effacer_filtre(&mut self) -> bool {
        if self.filtre.pop().is_none() {
            return false;
        }
        self.appliquer_filtre();
        true
    }

    /// Échap : retire le filtre et ferme le champ. Faux s'il n'y avait ni filtre ni champ ouvert.
    pub fn vider_filtre(&mut self) -> bool {
        if self.filtre.is_empty() {
            return std::mem::take(&mut self.filtre_ouvert);
        }
        let focus = self.selection.and_then(|i| self.entrees.get(i)).map(|e| e.nom.clone());
        self.filtre.clear();
        self.filtre_ouvert = false;
        self.appliquer_filtre();
        // On reste sur l'élément qu'on avait trouvé
        if let Some(i) = focus.and_then(|n| self.entrees.iter().position(|e| e.nom == n)) {
            self.choisir_seul(i);
            self.montrer_selection();
        }
        true
    }

    /// Le champ de filtre est-il montré ? Dès qu'il y a du texte, ou ouvert à vide par Ctrl+F.
    pub fn champ_filtre_visible(&self) -> bool {
        !self.filtre.is_empty() || self.filtre_ouvert
    }

    /// Clic sur une puce du champ : son jeton est ajouté au filtre (`data-t` de la maquette), séparé par une espace
    pub fn ajouter_jeton(&mut self, jeton: &str) {
        if !self.filtre.is_empty() && !self.filtre.ends_with(' ') {
            self.filtre.push(' ');
        }
        self.filtre.push_str(jeton);
        self.appliquer_filtre();
    }

    /// Ce que le surlignage doit chercher dans les noms : le filtre tapé, et le motif d'une recherche dans les sous-dossiers
    pub fn texte_surlignage(&self) -> String {
        match &self.emplacement {
            Emplacement::Recherche { motif, .. } => format!("{motif} {}", self.filtre),
            _ => self.filtre.clone(),
        }
    }

    /// Ctrl+F : sans filtre, ouvre le champ (puces `ext:` `taille>` `modif:`, comme `openSearch` de la maquette) ; avec un filtre,
    /// le cherche dans tous les sous-dossiers
    pub fn rechercher(&mut self, hwnd: HWND) {
        if self.filtre.trim().is_empty() && self.emplacement != Emplacement::Reglages {
            self.filtre_ouvert = true;
            self.annoncer("TAPEZ CE QUE VOUS CHERCHEZ · EXT:PDF TAILLE>10MO MODIF:2026 · CTRL+F DE NOUVEAU POUR CHERCHER DANS LES SOUS-DOSSIERS", false);
            return;
        }
        let racine = match &self.emplacement {
            Emplacement::Dossier(c) => c.clone(),
            Emplacement::Recherche { racine, .. } => racine.clone(),
            _ => {
                self.annoncer("OUVREZ D'ABORD UN DOSSIER OU UN DISQUE", true);
                return;
            }
        };
        let motif = self.filtre.trim().to_string();
        if motif.is_empty() {
            self.annoncer("TAPEZ CE QUE VOUS CHERCHEZ, PUIS CTRL+F", false);
            return;
        }
        self.naviguer(hwnd, Emplacement::Recherche { racine, motif });
    }

    // ── Vues (E15) ──

    /// Vue choisie par l'utilisateur : retenue pour ce dossier (V8), contrairement aux bascules faites par le programme
    pub fn choisir_vue(&mut self, hwnd: HWND, a: Affichage) {
        if let Emplacement::Dossier(c) = &self.emplacement {
            crate::reglages::memoriser_vue(c, a as u32);
        }
        self.changer_affichage(hwnd, a);
    }

    pub fn changer_affichage(&mut self, hwnd: HWND, a: Affichage) {
        if self.affichage == a {
            return;
        }
        crate::mouvement::demander_flip();
        self.affichage = a;
        self.defilement_cible = 0.0;
        self.defilement = 0.0;
        // Le regroupement n'existe qu'en vue liste
        if self.regroupement != Regroupement::Aucun {
            self.rebatir_garder();
        }
        if a == Affichage::Colonnes {
            self.charger_miller_parent(hwnd);
            self.miller_demande = None;
        }
        self.montrer_selection();
        self.annoncer(
            match a {
                Affichage::Liste => "VUE LISTE",
                Affichage::Grille => "VUE GRILLE · CTRL+MOLETTE : TAILLE DES VIGNETTES",
                Affichage::Colonnes => "VUE COLONNES · ← → : REMONTER / ENTRER",
            },
            false,
        );
    }

    /// Ctrl+molette : vignettes plus grandes ou plus petites
    pub fn zoomer(&mut self, crans: f32) {
        self.cote_grille = (self.cote_grille * 1.12f32.powf(crans)).clamp(GRILLE_MIN, GRILLE_MAX);
        self.montrer_selection();
    }

    pub fn basculer_tailles(&mut self, hwnd: HWND) {
        self.tailles = !self.tailles;
        if self.tailles {
            self.annoncer("TAILLES DES DOSSIERS · CALCUL EN ARRIERE-PLAN", false);
            self.lancer_tailles(hwnd);
        } else {
            TAILLES.fetch_add(1, Ordering::SeqCst);
            self.calcul_tailles = 0;
            for e in self.entrees.iter_mut().chain(self.tout.iter_mut()) {
                if matches!(e.genre, Genre::Dossier) {
                    e.taille = 0;
                }
            }
        }
    }

    pub fn oublier_metas(&mut self) {
        for e in self.entrees.iter_mut().chain(self.tout.iter_mut()) {
            e.meta = None;
        }
    }

    /// Colonnes dimensions / durée / auteur montrées : lit ces propriétés pour les fichiers du dossier
    pub fn lancer_metas(&mut self, hwnd: HWND) {
        let travail = METAS.fetch_add(1, Ordering::SeqCst) + 1;
        let complet = self.regroupement == Regroupement::Origine;
        // Les propriétés passent par des modules du Shell (du code) : pas dans un dossier non fiable
        if self.non_fiable {
            return;
        }
        if !(crate::reglages::colonnes_meta() || complet) || self.affichage != Affichage::Liste {
            return;
        }
        let dossier = self.dossier().unwrap_or("").to_string();
        let chemins: Vec<String> = self
            .tout
            .iter()
            .filter(|e| matches!(e.genre, Genre::Fichier) && e.meta.is_none() && !matches!(e.nuage, dossier::Nuage::EnLigne))
            .take(5000)
            .map(|e| dossier::joindre(e.parent.as_deref().unwrap_or(&dossier), &e.nom))
            .filter(|c| c.len() > 3)
            .collect();
        if chemins.is_empty() {
            return;
        }
        let (fenetre, n) = (hwnd.0 as isize, self.numero_fond);
        std::thread::spawn(move || unsafe {
            let _ = windows::Win32::System::Com::CoInitializeEx(None, windows::Win32::System::Com::COINIT_APARTMENTTHREADED);
            let mut lot = Vec::new();
            let total = chemins.len();
            for (k, c) in chemins.into_iter().enumerate() {
                if METAS.load(Ordering::Relaxed) != travail {
                    return;
                }
                let m = crate::apercu::lire_meta(&c, complet);
                lot.push((c.to_lowercase(), Arc::new(m)));
                let fin = k + 1 == total;
                // Par lots de 40 : la liste se remplit au fil de la lecture
                if lot.len() >= 40 || fin {
                    if !poster(fenetre, WM_FOND, MessageFond { numero: n, quoi: Fond::Metas(std::mem::take(&mut lot), fin) }) {
                        return;
                    }
                }
            }
        });
    }

    /// Déplace ces (nom, dossier d'arrivée) depuis `dossier`, une opération par dossier d'arrivée (conflits et Ctrl+Z comme tout déplacement)
    fn ranger(&mut self, hwnd: HWND, dossier: &str, v: Vec<(String, String)>) {
        let mut par_dest: Vec<(String, Vec<String>)> = Vec::new();
        let mut refuses = 0;
        for (nom, dest) in v {
            // Le dossier d'arrivée est créé s'il manque, mais son dernier niveau seulement : une faute de frappe plus haut ne crée rien
            if !std::path::Path::new(&dest).is_dir() && std::fs::create_dir(&dest).is_err() {
                refuses += 1;
                continue;
            }
            let source = dossier::joindre(dossier, &nom);
            match par_dest.iter_mut().find(|(d, _)| *d == dest) {
                Some((_, s)) => s.push(source),
                None => par_dest.push((dest, vec![source])),
            }
        }
        let n: usize = par_dest.iter().map(|(_, s)| s.len()).sum();
        for (dest, sources) in par_dest {
            self.lancer_operation(hwnd, operations::Genre::Deplacer, sources, Some(dest), Trace::Noter);
        }
        if refuses > 0 {
            self.annoncer(format!("{} RANGE{} · {refuses} DOSSIER(S) D'ARRIVEE INTROUVABLE(S)", Self::pluriel(n, "ÉLÉMENT"), if n > 1 { "S" } else { "" }), true);
        } else if n > 0 {
            self.annoncer(format!("RANGEMENT · {} (CTRL+Z POUR ANNULER)", Self::pluriel(n, "ÉLÉMENT")), false);
        }
    }

    /// Commande « Ranger ce dossier » : applique les règles aux fichiers du dossier ouvert
    pub fn ranger_ici(&mut self, hwnd: HWND) {
        let Emplacement::Dossier(d) = self.emplacement.clone() else {
            self.annoncer("RANGER : OUVRIR UN DOSSIER", true);
            return;
        };
        let r = crate::reglages::actuels();
        if r.regles.is_empty() {
            self.annoncer("AUCUNE REGLE : PALETTE › REGLE DE RANGEMENT…", true);
            return;
        }
        let fichiers: Vec<(String, u64)> = self.tout.iter().filter(|e| matches!(e.genre, Genre::Fichier)).map(|e| (e.nom.clone(), e.modif)).collect();
        let v = crate::rangement::choisir(&r.regles, &fichiers, crate::systeme::maintenant(), &d);
        if v.is_empty() {
            self.annoncer("AUCUNE REGLE NE S'APPLIQUE ICI", false);
        } else {
            self.ranger(hwnd, &d, v);
        }
    }

    /// Rangement automatique décidé à la réception des badges (réglage, désactivé par défaut)
    pub fn ranger_telechargements(&mut self, hwnd: HWND) {
        let v = std::mem::take(&mut self.a_ranger);
        let Emplacement::Dossier(d) = self.emplacement.clone() else { return };
        // Pas pendant une autre opération (le conflit ou la file se mêleraient) : le prochain passage reprendra
        if v.is_empty() || self.operation.is_some() || !self.file.is_empty() || self.conflits.is_some() {
            return;
        }
        let v: Vec<(String, String)> = v.into_iter().filter(|(nom, _)| self.ranges.insert(dossier::joindre(&d, nom).to_lowercase())).collect();
        self.ranger(hwnd, &d, v);
    }

    /// Nettoyage automatique des Téléchargements décidé à la réception des badges : vers la corbeille
    pub fn nettoyer_telechargements(&mut self, hwnd: HWND) {
        let v = std::mem::take(&mut self.nettoyage);
        if v.is_empty() {
            return;
        }
        let n = v.len();
        self.lancer_operation(hwnd, operations::Genre::Corbeille, v, None, Trace::Noter);
        self.annoncer(format!("NETTOYAGE DES TELECHARGEMENTS · {} A LA CORBEILLE (CTRL+Z POUR ANNULER)", Self::pluriel(n, "ÉLÉMENT")), false);
    }

    /// Dossier Téléchargements : badges DEJA INSTALLE et COPIE SUR LE PC, lus en fond
    fn lancer_badges(&mut self, hwnd: HWND) {
        let Some(d) = self.dossier().map(str::to_string) else { return };
        if !crate::systeme::dossier_telechargements(&d) {
            return;
        }
        let fichiers: Vec<(String, u64)> = self.tout.iter().filter(|e| matches!(e.genre, Genre::Fichier)).map(|e| (e.nom.clone(), e.taille)).collect();
        let (fenetre, n) = (hwnd.0 as isize, self.numero_fond);
        std::thread::spawn(move || {
            let b = crate::telechargements::badges(&d, &fichiers);
            poster(fenetre, WM_FOND, MessageFond { numero: n, quoi: Fond::Badges(b) });
        });
    }

    /// Dossier avec des fichiers cloud : ceux qui attendent la synchronisation, revus toutes les 3 s
    /// tant qu'il en reste (10 minutes au plus ; la surveillance du dossier relance aussi la lecture)
    fn lancer_synchro(&mut self, hwnd: HWND) {
        let travail = SYNCHRO.fetch_add(1, Ordering::SeqCst) + 1;
        let Some(d) = self.dossier().map(str::to_string) else { return };
        let noms: Vec<String> = self.tout.iter().filter(|e| e.nuage != dossier::Nuage::Aucun).map(|e| e.nom.clone()).collect();
        if noms.is_empty() || !crate::reglages::ETAT_CLOUD.load(Ordering::Relaxed) {
            return;
        }
        let (fenetre, n) = (hwnd.0 as isize, self.numero_fond);
        std::thread::spawn(move || {
            let mut avant: Option<HashSet<String>> = None;
            for _ in 0..200 {
                if SYNCHRO.load(Ordering::Relaxed) != travail {
                    return;
                }
                let v = crate::synchro::en_attente(&d, &noms);
                let fini = v.is_empty();
                if avant.as_ref() != Some(&v) {
                    avant = Some(v.clone());
                    if !poster(fenetre, WM_FOND, MessageFond { numero: n, quoi: Fond::Synchro(v) }) {
                        return;
                    }
                }
                if fini {
                    return;
                }
                std::thread::sleep(std::time::Duration::from_secs(3));
            }
        });
    }

    /// Après chaque lecture : état Git, tailles, colonne parente
    fn apres_lecture(&mut self, hwnd: HWND) {
        self.non_fiable = self.dossier().is_some_and(crate::reglages::non_fiable);
        self.lancer_metas(hwnd);
        self.lancer_synchro(hwnd);
        self.lancer_badges(hwnd);
        // Recherche globale sur un index incomplet : relancée toute seule quand il est prêt
        if matches!(self.emplacement, Emplacement::Globale { .. }) && !crate::index::pret() {
            crate::index::abonner(hwnd);
        }
        let fenetre = hwnd.0 as isize;
        // Un dépôt non fiable n'est pas interrogé : git lit sa configuration, qui peut lancer des commandes
        if self.non_fiable {
            self.git = None;
        } else if let Some(d) = self.dossier().map(str::to_string) {
            self.numero_git = numero();
            let n = self.numero_git;
            std::thread::spawn(move || {
                let g = outils::etat(&d);
                poster(fenetre, WM_GIT, (n, g));
            });
        }
        if self.tailles {
            self.lancer_tailles(hwnd);
        } else {
            self.remettre_tailles();
        }
    }

    pub fn recevoir_git(&mut self, numero: u64, g: Option<Git>) -> bool {
        if numero != self.numero_git {
            return false;
        }
        self.git = g;
        true
    }

    /// État Git d'un élément du dossier ouvert : 'M', 'A' ou '?'
    pub fn etat_git(&self, e: &Entree) -> Option<char> {
        self.git.as_ref().filter(|g| !g.etats.is_empty())?.etats.get(&e.nom.to_lowercase()).copied()
    }

    /// Espace sur des dossiers (ou la palette) : leur taille, calculée en arrière-plan, dans la colonne TAILLE.
    /// Sans dossier choisi : tous les dossiers ouverts.
    pub fn calculer_tailles(&mut self, hwnd: HWND) {
        let Some(d) = self.dossier().map(str::to_string) else {
            self.annoncer("OUVREZ D'ABORD UN DOSSIER", true);
            return;
        };
        let choisis: Vec<String> = self.choisis().into_iter().filter_map(|i| self.entrees.get(i)).filter(|e| matches!(e.genre, Genre::Dossier)).map(|e| e.nom.clone()).collect();
        let noms: Vec<String> = if choisis.is_empty() { self.tout.iter().filter(|e| matches!(e.genre, Genre::Dossier) && !e.lien).map(|e| e.nom.clone()).collect() } else { choisis };
        // Recalcul demandé : les tailles connues de ces dossiers sont oubliées
        for n in &noms {
            self.tailles_connues.remove(&dossier::joindre(&d, n).to_lowercase());
        }
        self.annoncer(format!("TAILLE DE {} · CALCUL EN ARRIERE-PLAN", Self::pluriel(noms.len(), "DOSSIER")), false);
        self.lancer_tailles_de(hwnd, d, noms);
    }

    /// Les dossiers choisis sont-ils tous des dossiers (Espace calcule alors leur taille) ?
    pub fn dossiers_choisis(&self) -> bool {
        let c = self.choisis();
        !c.is_empty() && c.iter().all(|&i| self.entrees.get(i).is_some_and(|e| matches!(e.genre, Genre::Dossier)))
    }

    /// Remet les tailles déjà calculées (après une relecture du dossier)
    fn remettre_tailles(&mut self) {
        let Some(d) = self.dossier().map(str::to_string) else { return };
        if self.tailles_connues.is_empty() {
            return;
        }
        for e in self.entrees.iter_mut().chain(self.tout.iter_mut()) {
            if matches!(e.genre, Genre::Dossier) {
                if let Some(&t) = self.tailles_connues.get(&dossier::joindre(&d, &e.nom).to_lowercase()) {
                    e.taille = t;
                }
            }
        }
    }

    fn lancer_tailles(&mut self, hwnd: HWND) {
        let Some(d) = self.dossier().map(str::to_string) else { return };
        // Tailles déjà connues : remises tout de suite
        for e in self.entrees.iter_mut().chain(self.tout.iter_mut()) {
            if matches!(e.genre, Genre::Dossier) {
                if let Some(&t) = self.tailles_connues.get(&dossier::joindre(&d, &e.nom).to_lowercase()) {
                    e.taille = t;
                }
            }
        }
        let a_calculer: Vec<String> = self
            .tout
            .iter()
            .filter(|e| matches!(e.genre, Genre::Dossier) && !e.lien && !self.tailles_connues.contains_key(&dossier::joindre(&d, &e.nom).to_lowercase()))
            .map(|e| e.nom.clone())
            .collect();
        self.lancer_tailles_de(hwnd, d, a_calculer);
    }

    fn lancer_tailles_de(&mut self, hwnd: HWND, d: String, a_calculer: Vec<String>) {
        self.calcul_tailles = a_calculer.len();
        if a_calculer.is_empty() {
            return;
        }
        let travail = TAILLES.fetch_add(1, Ordering::SeqCst) + 1;
        let (fenetre, n) = (hwnd.0 as isize, self.numero_fond);
        std::thread::spawn(move || {
            for nom in a_calculer {
                let continuer = || TAILLES.load(Ordering::Relaxed) == travail;
                if !continuer() {
                    return;
                }
                let t = dossier::taille_totale(&dossier::joindre(&d, &nom), &continuer);
                if !continuer() || !poster(fenetre, WM_FOND, MessageFond { numero: n, quoi: Fond::Taille(dossier::joindre(&d, &nom), t) }) {
                    return;
                }
            }
        });
    }

    fn charger_miller_parent(&mut self, hwnd: HWND) {
        self.miller_parent.clear();
        let Some(p) = self.emplacement.parent() else { return };
        let (fenetre, n, caches) = (hwnd.0 as isize, self.numero_fond, self.caches);
        std::thread::spawn(move || {
            let mut v = dossier::lire(&p, caches).unwrap_or_default();
            dossier::trier(&mut v, Colonne::Nom, true, false);
            poster(fenetre, WM_FOND, MessageFond { numero: n, quoi: Fond::MillerParent(v) });
        });
    }

    /// Colonnes Miller : le contenu de l'élément choisi s'affiche à droite (demandé au dessin)
    pub fn demander_miller_enfant(&mut self, hwnd: HWND) {
        let sujet = self.selection.and_then(|i| self.entrees.get(i)).filter(|e| e.est_conteneur()).map(|e| self.emplacement.enfant(e));
        let cle = sujet.as_ref().map(|e| format!("{e:?}"));
        if cle == self.miller_demande {
            return;
        }
        self.miller_demande = cle.clone();
        self.miller_enfant.clear();
        let Some(emp) = sujet else { return };
        let (fenetre, n, caches, cle) = (hwnd.0 as isize, self.numero_fond, self.caches, cle.unwrap_or_default());
        std::thread::spawn(move || {
            let mut v = dossier::lire(&emp, caches).unwrap_or_default();
            dossier::trier(&mut v, Colonne::Nom, true, false);
            poster(fenetre, WM_FOND, MessageFond { numero: n, quoi: Fond::MillerEnfant(cle, v) });
        });
    }

    pub fn possede_fond(&self, numero: u64) -> bool {
        numero == self.numero_fond
    }

    pub fn possede_git(&self, numero: u64) -> bool {
        numero == self.numero_git
    }

    pub fn recevoir_fond(&mut self, m: MessageFond) -> bool {
        if m.numero != self.numero_fond {
            return false;
        }
        match m.quoi {
            Fond::MillerParent(v) => self.miller_parent = v,
            Fond::MillerEnfant(cle, v) => {
                if self.miller_demande.as_deref() == Some(cle.as_str()) {
                    self.miller_enfant = v;
                }
            }
            Fond::Metas(v, fin) => {
                let dossier = self.dossier().unwrap_or("").to_string();
                let carte: std::collections::HashMap<String, Arc<dossier::Meta>> = v.into_iter().collect();
                for e in self.entrees.iter_mut().chain(self.tout.iter_mut()) {
                    if e.meta.is_none() && matches!(e.genre, Genre::Fichier) {
                        let c = dossier::joindre(e.parent.as_deref().unwrap_or(&dossier), &e.nom).to_lowercase();
                        if let Some(m) = carte.get(&c) {
                            e.meta = Some(m.clone());
                        }
                    }
                }
                // Quarantaine : les sections se font une fois tout lu (sinon les lignes sauteraient d'une section à l'autre)
                if fin && self.regroupement == Regroupement::Origine {
                    self.rebatir_garder();
                }
                // Tri sur une de ces colonnes : refait une fois tout lu
                if fin && matches!(self.tri.0, Colonne::Dimensions | Colonne::Duree | Colonne::Auteur | Colonne::Origine | Colonne::Signature) {
                    dossier::trier(&mut self.tout, self.tri.0, self.tri.1, self.tri.2);
                    self.rebatir_garder();
                }
            }
            Fond::Synchro(v) => self.pas_synchro = v,
            Fond::Badges(b) => {
                self.badges = b;
                let Some(d) = self.dossier().map(str::to_string) else { return true };
                let r = crate::reglages::actuels();
                let limite = crate::systeme::maintenant().saturating_sub(r.nettoyage_jours as u64 * 864_000_000_000);
                self.nettoyage = self
                    .tout
                    .iter()
                    .filter(|e| matches!(e.genre, Genre::Fichier))
                    .filter(|e| {
                        (r.nettoyage_jours > 0 && e.ajout > 0 && e.ajout < limite)
                            || (r.nettoyer_copies && matches!(self.badges.get(&e.nom.to_lowercase()), Some(crate::telechargements::Badge::Copie(_))))
                    })
                    .map(|e| dossier::joindre(&d, &e.nom))
                    .collect();
                if r.ranger_auto {
                    let fichiers: Vec<(String, u64)> = self
                        .tout
                        .iter()
                        .filter(|e| matches!(e.genre, Genre::Fichier))
                        .filter(|e| !self.nettoyage.contains(&dossier::joindre(&d, &e.nom)))
                        .map(|e| (e.nom.clone(), e.modif))
                        .collect();
                    self.a_ranger = crate::rangement::choisir(&r.regles, &fichiers, crate::systeme::maintenant(), &d);
                }
            }
            Fond::Taille(chemin, t) => {
                self.tailles_connues.insert(chemin.to_lowercase(), t);
                self.calcul_tailles = self.calcul_tailles.saturating_sub(1);
                let Some(d) = self.dossier().map(str::to_string) else { return true };
                for e in self.entrees.iter_mut().chain(self.tout.iter_mut()) {
                    if matches!(e.genre, Genre::Dossier) && dossier::joindre(&d, &e.nom).eq_ignore_ascii_case(&chemin) {
                        e.taille = t;
                    }
                }
            }
        }
        true
    }

    /// Plus grande taille affichée (barres de la vue « tailles »)
    pub fn taille_max(&self) -> u64 {
        self.entrees.iter().map(|e| e.taille).max().unwrap_or(0)
    }

    // ── Productivité (E16) et développeur (E18) ──

    /// Dossier « ici » pour un terminal ou VS Code : le dossier ouvert, celui d'une archive ou d'un résultat
    pub fn dossier_ici(&self) -> Option<String> {
        match &self.emplacement {
            Emplacement::Dossier(c) | Emplacement::Recherche { racine: c, .. } => Some(c.clone()),
            Emplacement::Archive { fichier, .. } => Some(dossier::dossier_de(fichier).to_string()),
            Emplacement::Globale { .. } | Emplacement::Groupe(_) | Emplacement::Recents | Emplacement::Doublons(_) => self.selection.and_then(|i| self.entrees.get(i)).and_then(|e| e.parent.as_deref().map(str::to_string)),
            _ => None,
        }
    }

    /// Copie le chemin (choisis, sinon le dossier) au format WSL ou Unix
    pub fn copier_chemin_format(&mut self, hwnd: HWND, wsl: bool) {
        let mut chemins = self.chemins_choisis();
        if chemins.is_empty() {
            chemins.extend(self.dossier_ici());
        }
        if chemins.is_empty() {
            return;
        }
        let texte: Vec<String> = chemins.iter().map(|c| if wsl { outils::chemin_wsl(c) } else { outils::chemin_unix(c) }).collect();
        operations::copier_texte(hwnd, &texte.join("\n"));
        self.annoncer(format!("COPIE : {}", texte[0]), false);
    }

    /// SHA-256 du fichier qui a le focus, calculé en arrière-plan puis copié
    /// Retire la marque « téléchargé d'Internet » des fichiers choisis (Windows n'avertit plus à l'ouverture)
    pub fn debloquer(&mut self) {
        let chemins: Vec<String> = self.choisis().into_iter().map(|i| &self.entrees[i]).filter(|e| matches!(e.genre, Genre::Fichier) && e.nuage == dossier::Nuage::Aucun).filter_map(|e| self.chemin_de(e)).collect();
        if chemins.is_empty() {
            self.annoncer("CHOISISSEZ DES FICHIERS", true);
            return;
        }
        let marques: Vec<&String> = chemins.iter().filter(|c| outils::origine_internet(c).is_some()).collect();
        if marques.is_empty() {
            self.annoncer("RIEN A DEBLOQUER : AUCUN FICHIER MARQUE INTERNET", false);
            return;
        }
        let faits = marques.iter().filter(|c| outils::debloquer(c).is_ok()).count();
        let echecs = marques.len() - faits;
        if echecs > 0 {
            self.annoncer(format!("{faits} DEBLOQUE{} · {echecs} REFUSE{} (DROITS ?)", if faits > 1 { "S" } else { "" }, if echecs > 1 { "S" } else { "" }), true);
        } else {
            self.annoncer(format!("{faits} FICHIER{} DEBLOQUE{}", if faits > 1 { "S" } else { "" }, if faits > 1 { "S" } else { "" }), false);
        }
    }

    pub fn empreinte(&mut self, hwnd: HWND) {
        let Some(e) = self.selection.and_then(|i| self.entrees.get(i)).filter(|e| matches!(e.genre, Genre::Fichier)) else {
            self.annoncer("CHOISISSEZ UN FICHIER", true);
            return;
        };
        let Some(chemin) = self.chemin_de(e) else { return };
        let nom = e.nom.to_uppercase();
        self.annoncer(format!("SHA-256 DE {nom}…"), false);
        tache(hwnd, move || match outils::empreinte(&chemin) {
            Ok(h) => Fini { message: format!("SHA-256 COPIE · {nom} · {}…", &h[..16].to_uppercase()), erreur: false, copier: Some(h), lancer: None },
            Err(e) => Fini { message: e, erreur: true, copier: None, lancer: None },
        });
    }

    /// Compresse les éléments choisis en .zip ou .7z (à côté)
    /// Actions en lot sur les images choisies : chacune donne un nouveau fichier à côté
    pub fn traiter_images(&mut self, hwnd: HWND, t: crate::images::Traitement) {
        let chemins: Vec<String> = self.choisis().into_iter().map(|i| &self.entrees[i]).filter(|e| crate::images::est_image(&e.ext)).filter_map(|e| self.chemin_de(e)).collect();
        if chemins.is_empty() {
            self.annoncer("CHOISISSEZ DES IMAGES (JPG, PNG, HEIC…)", true);
            return;
        }
        let total = chemins.len();
        self.annoncer(format!("{} · {total} IMAGE{}…", t.libelle(), if total > 1 { "S" } else { "" }), false);
        tache(hwnd, move || {
            let (n, erreur) = crate::images::traiter_tout(&chemins, t);
            match erreur {
                Some(e) if n == 0 => Fini { message: e, erreur: true, copier: None, lancer: None },
                Some(e) => Fini { message: format!("{n} IMAGE{} SUR {total} · {e}", if n > 1 { "S" } else { "" }), erreur: true, copier: None, lancer: None },
                None => Fini { message: format!("{n} IMAGE{} CREEE{}", if n > 1 { "S" } else { "" }, if n > 1 { "S" } else { "" }), erreur: false, copier: None, lancer: None },
            }
        });
    }

    /// SFTP : télécharge les éléments choisis dans le dossier Téléchargements
    pub fn telecharger_sftp(&mut self, hwnd: HWND) {
        let Emplacement::Sftp { hote, chemin } = &self.emplacement else { return };
        let distants: Vec<String> = self.choisis().into_iter().map(|i| crate::sftp::joindre(chemin, &self.entrees[i].nom)).collect();
        let Some(dest) = crate::systeme::chemin_telechargements() else { return };
        if distants.is_empty() {
            return;
        }
        let (hote, n) = (hote.clone(), distants.len());
        self.annoncer(format!("TELECHARGEMENT DE {}…", Self::pluriel(n, "ÉLÉMENT")), false);
        tache(hwnd, move || match crate::sftp::telecharger(&hote, &distants, &dest) {
            Ok(()) => Fini { message: format!("{} DANS TELECHARGEMENTS", Self::pluriel(n, "ÉLÉMENT")), erreur: false, copier: None, lancer: None },
            Err(e) => Fini { message: e, erreur: true, copier: None, lancer: None },
        });
    }

    pub fn compresser(&mut self, hwnd: HWND, ext: &str) {
        let Some(d) = self.dossier().map(str::to_string) else {
            self.annoncer("OUVREZ D'ABORD UN DOSSIER", true);
            return;
        };
        let noms: Vec<String> = self.choisis().into_iter().map(|i| self.entrees[i].nom.clone()).collect();
        if noms.is_empty() {
            self.annoncer("CHOISISSEZ CE QU'IL FAUT COMPRESSER", true);
            return;
        }
        let base = if noms.len() == 1 { crate::lot::decouper(&noms[0], self.entrees[self.choisis()[0]].est_conteneur()).0.to_string() } else { dossier::nom_affiche(&d) };
        let nom = crate::archive::nom_libre(&d, &base, &format!(".{ext}"));
        self.annoncer(format!("COMPRESSION EN {}…", nom.to_uppercase()), false);
        tache(hwnd, move || match crate::archive::creer(&dossier::joindre(&d, &nom), &d, &noms) {
            Ok(()) => Fini { message: format!("{} CRÉÉ", nom.to_uppercase()), erreur: false, copier: None, lancer: None },
            Err(e) => Fini { message: e, erreur: true, copier: None, lancer: None },
        });
    }

    /// Extrait l'archive choisie (dans un dossier à son nom), ou les éléments choisis dans l'archive ouverte
    pub fn extraire(&mut self, hwnd: HWND) {
        if let Emplacement::Archive { fichier, interne } = &self.emplacement {
            let (fichier, d) = (fichier.clone(), dossier::dossier_de(fichier).to_string());
            let choisis: Vec<String> =
                self.choisis().into_iter().map(|i| if interne.is_empty() { self.entrees[i].nom.clone() } else { format!("{interne}/{}", self.entrees[i].nom) }).collect();
            let base = crate::archive::sans_extension(fichier.rsplit('\\').next().unwrap_or(&fichier));
            let nom = crate::archive::nom_libre(&d, &base, "");
            self.annoncer(format!("EXTRACTION VERS {}…", nom.to_uppercase()), false);
            tache(hwnd, move || {
                let bruts = crate::archive::bruts(&fichier, &choisis);
                match crate::archive::extraire(&fichier, &bruts, &dossier::joindre(&d, &nom)) {
                    Ok(()) => Fini { message: format!("EXTRAIT DANS {}", nom.to_uppercase()), erreur: false, copier: None, lancer: None },
                    Err(e) => Fini { message: e, erreur: true, copier: None, lancer: None },
                }
            });
            return;
        }
        let archives: Vec<String> = self.choisis().into_iter().map(|i| &self.entrees[i]).filter(|e| crate::archive::est_archive(&e.ext)).filter_map(|e| self.chemin_de(e)).collect();
        if archives.is_empty() {
            self.annoncer("CHOISISSEZ UNE ARCHIVE (ZIP, 7Z, RAR…)", true);
            return;
        }
        self.annoncer(format!("EXTRACTION DE {} ARCHIVE(S)…", archives.len()), false);
        tache(hwnd, move || {
            let mut faits = Vec::new();
            for a in &archives {
                let d = dossier::dossier_de(a).to_string();
                let nom = crate::archive::nom_libre(&d, &crate::archive::sans_extension(a.rsplit('\\').next().unwrap_or(a)), "");
                if let Err(e) = crate::archive::extraire(a, &[], &dossier::joindre(&d, &nom)) {
                    return Fini { message: e, erreur: true, copier: None, lancer: None };
                }
                faits.push(nom.to_uppercase());
            }
            let nettoyer = crate::reglages::actuels().nettoyer_archives;
            let jetees: Vec<String> = archives.iter().filter(|a| nettoyer && crate::systeme::dossier_telechargements(dossier::dossier_de(a))).cloned().collect();
            let jete = !jetees.is_empty() && crate::telechargements::a_la_corbeille(&jetees);
            Fini { message: format!("EXTRAIT DANS {}{}", faits.join(", "), if jete { " · ARCHIVE A LA CORBEILLE" } else { "" }), erreur: false, copier: None, lancer: None }
        });
    }

    /// Ctrl+1/2/3/0 : balise de couleur des éléments choisis
    pub fn baliser(&mut self, b: Option<Balise>) {
        let chemins = self.chemins_choisis();
        if chemins.is_empty() {
            return;
        }
        outils::baliser(&chemins, b);
        let n = chemins.len();
        self.annoncer(
            match b {
                Some(Balise::Rouge) => format!("BALISE ROUGE · {n}"),
                Some(Balise::Blanc) => format!("BALISE BLANCHE · {n}"),
                Some(Balise::Gris) => format!("BALISE GRISE · {n}"),
                None => format!("BALISE RETIREE · {n}"),
            },
            false,
        );
    }

    /// Alt+1/2/3 : étiquette (1 à 3 carrés après le nom) des éléments choisis ; la même touche la retire si tous l'ont déjà
    pub fn etiqueter(&mut self, n: u8) {
        let chemins = self.chemins_choisis();
        if chemins.is_empty() {
            return;
        }
        let pose = crate::etiquettes::basculer(&chemins, n);
        let nb = chemins.len();
        self.annoncer(if pose { format!("ETIQUETTE {n} · {nb}") } else { format!("ETIQUETTE {n} RETIREE · {nb}") }, false);
    }

    /// Fichier trouvé ailleurs (palette, index) : son dossier s'ouvre et il est choisi
    /// Vrai si le fichier est choisi (dossier déjà ouvert) ou si l'ouverture de son dossier est lancée (il sera choisi à la
    /// lecture) ; faux si le dossier est ouvert mais le fichier n'y est pas (disparu, filtré)
    pub fn montrer(&mut self, hwnd: HWND, chemin: &str) -> bool {
        let d = dossier::dossier_de(chemin).to_string();
        self.a_reselectionner = vec![chemin.rsplit('\\').next().unwrap_or(chemin).to_string()];
        let emp = Emplacement::dossier(&d);
        if emp == self.emplacement {
            let noms = std::mem::take(&mut self.a_reselectionner);
            if let Some(i) = self.entrees.iter().position(|e| noms.iter().any(|n| n.eq_ignore_ascii_case(&e.nom))) {
                self.choisir_seul(i);
                self.montrer_selection();
                return true;
            }
            false
        } else {
            self.naviguer(hwnd, emp.clone());
            // `naviguer` refuse parfois (fonction expérimentale) : le dossier est alors resté l'ancien
            self.emplacement == emp
        }
    }

    /// F2 avec plusieurs éléments choisis : formulaire de renommage par lot
    pub fn commencer_lot(&mut self) -> Option<Lot> {
        let d = self.dossier()?.to_string();
        let choisis = self.choisis();
        if choisis.is_empty() {
            return None;
        }
        let noms: Vec<String> = choisis.iter().map(|&i| self.entrees[i].nom.clone()).collect();
        // {date} : la prise de vue des photos (EXIF), sinon la date de modification
        let dates = choisis
            .iter()
            .map(|&i| {
                let e = &self.entrees[i];
                let photo = matches!(e.ext.as_str(), "JPG" | "JPEG" | "HEIC" | "HEIF" | "TIF" | "TIFF" | "PNG" | "DNG" | "CR2" | "NEF" | "ARW");
                photo.then(|| crate::lot::date_prise(&dossier::joindre(&d, &e.nom))).flatten().unwrap_or(e.modif)
            })
            .collect();
        Some(Lot::nouveau(d, noms, dates))
    }

    /// Applique le renommage par lot
    pub fn appliquer_lot(&mut self, hwnd: HWND, lot: &Lot) -> Result<usize, String> {
        let nouveaux = lot.apercu()?;
        let paires: Vec<(String, String)> = lot.noms.iter().cloned().zip(nouveaux.iter().cloned()).filter(|(a, b)| a != b).collect();
        if paires.is_empty() {
            return Ok(0);
        }
        crate::lot::renommer(&lot.dossier, &paires)?;
        historique::noter(Action::RenommageLot { dossier: lot.dossier.clone(), paires: paires.clone() });
        proposer_annulation(crate::pastilles::nom_court("RENOMMÉ", paires.len()));
        self.marques.clear();
        self.selection = None;
        self.a_reselectionner = nouveaux;
        let n = paires.len();
        // Relecture complète (pas une actualisation) : les nouveaux noms sont choisis
        self.generation = numero();
        lancer_lecture(hwnd, self.generation, false, false, self.emplacement.clone(), self.caches, self.tri);
        Ok(n)
    }
}

/// « Reprendre là où j'en étais » : les 5 derniers dossiers quittés avec leur sélection (« dossier\tnom »)
fn noter_reprise(dossier: &str, nom: &str) {
    if !crate::reglages::actuels().retenir_recents {
        return;
    }
    crate::reglages::modifier(|r| {
        r.reprises.retain(|x| x.split('\t').next().is_some_and(|d| !d.eq_ignore_ascii_case(dossier)));
        r.reprises.insert(0, format!("{dossier}\t{nom}"));
        r.reprises.truncate(5);
    });
}

/// Dossiers récents (palette) : le plus récent d'abord, 20 au plus
fn noter_recent(c: &str) {
    let r = crate::reglages::actuels();
    if !r.retenir_recents || r.recents.first().is_some_and(|x| x.eq_ignore_ascii_case(c)) {
        return;
    }
    crate::reglages::modifier(|r| {
        r.recents.retain(|x| !x.eq_ignore_ascii_case(c));
        r.recents.insert(0, c.to_string());
        r.recents.truncate(20);
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Part du cran comblée après `ms` millisecondes, images toutes les `periode_ms`
    fn comble_apres(periode_ms: f32, ms: f32) -> f32 {
        let (mut pos, mut t) = (0.0f32, 0.0f32);
        while t + periode_ms <= ms + 1e-3 {
            pos += (100.0 - pos) * part_lissage(periode_ms / 1000.0);
            t += periode_ms;
        }
        pos / 100.0
    }

    #[test]
    fn lissage_du_defilement_ne_depend_pas_de_la_cadence() {
        // 0,28 par image à 60 Hz : inchangé
        assert!((part_lissage(1.0 / 60.0) - 0.28).abs() < 1e-5);
        // Même avancement au même instant à 60, 144 et 240 Hz (l'ancien pas par image : 0,28 à 240 Hz = 99 % en 58 ms)
        for ms in [50.0, 100.0, 200.0] {
            let ref60 = comble_apres(1000.0 / 60.0, ms);
            for hz in [144.0, 240.0] {
                let v = comble_apres(1000.0 / hz, ms);
                // Les images tombent entre deux instants : tolérance d'une image à 60 Hz
                assert!((v - ref60).abs() < 0.12, "{hz} Hz à {ms} ms : {v} contre {ref60}");
            }
        }
        // Une image très longue (fenêtre gelée) ne dépasse pas la cible
        assert!(part_lissage(5.0) < 1.0 && part_lissage(0.0) == 0.0);
    }

    #[test]
    fn seuls_les_controles_du_chrome_se_soulevent() {
        assert!(Cible::Onglet(0).est_chrome());
        assert!(Cible::NouvelOnglet.est_chrome());
        assert!(Cible::RechercheHaut.est_chrome());
        assert!(Cible::CePc.est_chrome());
        assert!(Cible::Outil(crate::barre::Outil::Nouveau).est_chrome());
        assert!(Cible::Terminal.est_chrome());
        assert!(!Cible::Densite(0).est_chrome());
        assert!(!Cible::Ligne(0).est_chrome());
        assert!(!Cible::Reglage(0).est_chrome());
        assert!(!Cible::FermerOnglet(0).est_chrome());
    }

    #[test]
    fn phrase_du_toast_ajoute_au_singulier_et_au_pluriel() {
        assert_eq!(phrase_ajoute(1, "Projets"), "Élément ajouté à Projets");
        assert_eq!(phrase_ajoute(3, "Projets"), "3 éléments ajoutés à Projets");
    }

    #[test]
    fn puces_du_filtre_ajoutent_leur_jeton_et_echap_ferme_le_champ() {
        let mut app = App::nouveau(Emplacement::CePC);
        assert!(!app.champ_filtre_visible());
        // Une puce sur un filtre vide : le jeton seul ; sur un filtre en cours : séparé par une espace
        app.ajouter_jeton("ext:");
        assert_eq!(app.filtre, "ext:");
        assert!(app.champ_filtre_visible());
        app.taper_filtre('p');
        app.ajouter_jeton("taille>");
        assert_eq!(app.filtre, "ext:p taille>");
        app.ajouter_jeton("modif:");
        assert_eq!(app.filtre, "ext:p taille> modif:");
        // Échap vide le filtre et ferme le champ ; un second Échap n'a plus rien à faire
        assert!(app.vider_filtre());
        assert!(app.filtre.is_empty() && !app.champ_filtre_visible());
        assert!(!app.vider_filtre());
        // Champ ouvert à vide (Ctrl+F) : visible, et le premier Échap le ferme seulement
        app.filtre_ouvert = true;
        assert!(app.champ_filtre_visible());
        assert!(app.vider_filtre());
        assert!(!app.champ_filtre_visible());
        assert!(!app.vider_filtre());
    }

    #[test]
    fn la_marque_de_la_palette_ne_reste_pas_sur_une_operation_de_fond() {
        let mut app = App::nouveau(Emplacement::CePC);
        // Rien ne part (aucun élément) : la marque est effacée, l'opération de fond suivante ne passera pas pour celle de la palette
        app.armer_palette();
        app.lancer_elements(HWND::default(), operations::Genre::Copier, Vec::new(), Vec::new(), Trace::Aucune, Vec::new());
        assert!(!app.palette_arme && app.palette_numero.is_none());
        // Appel qui n'a rien lancé ni mis en attente : `desarmer_palette` efface aussi
        app.armer_palette();
        app.desarmer_palette();
        assert!(!app.palette_arme);
        // Seul le numéro posé à la marque est celui de la palette
        app.palette_numero = Some(42);
        assert!(app.est_palette(42) && !app.est_palette(43));
    }

    #[test]
    fn surlignage_cherche_le_motif_de_la_recherche_et_le_filtre() {
        let mut app = App::nouveau(Emplacement::CePC);
        app.filtre = "plan".into();
        assert_eq!(app.texte_surlignage(), "plan");
        app.emplacement = Emplacement::Recherche { racine: r"C:\".into(), motif: "contrat".into() };
        assert_eq!(app.texte_surlignage().trim(), "contrat plan");
    }
}

/// Mode essai seulement (`SPACEFILES_SANS_REGISTRE=1` et `SPACEFILES_FILTRE_OUVERT=1`) : le champ de filtre est ouvert à vide, pour les
/// captures faites sans clavier (Ctrl+F ne peut pas être posté : il lit l'état réel des touches). Faux en usage normal.
fn ouvert_par_essai() -> bool {
    crate::defaut::sans_registre() && std::env::var("SPACEFILES_FILTRE_OUVERT").is_ok_and(|v| v.trim() == "1")
}
