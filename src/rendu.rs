// Rendu Direct2D + DirectWrite de la fenêtre

use windows::core::{Interface, Result, HSTRING};
use windows::Win32::Foundation::{D2DERR_RECREATE_TARGET, HWND};
use windows::Win32::Graphics::Direct2D::Common::*;
use windows::Win32::Graphics::Direct2D::*;
use windows::Win32::Graphics::DirectWrite::*;
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM;

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};

use windows::Foundation::Numerics::Matrix3x2;

use crate::apercu::{Apercu, Galerie, Image, Teinte};
use crate::app::{Affichage, App, Cible, GRILLE_ECART, GRILLE_NOM};
use crate::dossier::Entree;
use crate::lot::Lot;
use crate::conflit::{Choix, Conflits};
use crate::menu::{self, Menu};
use crate::outils::{self, Balise};
use crate::palette::{self, Palette};
use crate::pastilles::{self, Place, Bouton, Disposition, Genre as GenrePastille, Pastille, Tete};
use crate::pixel::{self, EtatPixel, Periode};
use crate::reflet;
use crate::dossier::{Colonne, Emplacement, Genre, Nuage};
use crate::page::{self, Element, Ligne, Page, Valeur};
use crate::icones::{self, Icone, TOUTES};
use crate::systeme;
use crate::mouvement::{self, EtatFenetre, Mouvements};
use crate::barre::Outil;
use crate::theme::*;

/// Ce qui n'appartient pas à l'onglet affiché : la barre d'onglets, l'aperçu, les réglages
pub struct Vue<'a> {
    pub onglets: &'a [String],
    pub actif: usize,
    pub apercu_visible: bool,
    pub apercu: Option<&'a Apercu>,
    /// Panneau terminal intégré et ses dernières opérations (journal réel)
    pub terminal_visible: bool,
    pub terminal_lignes: &'a [crate::journal::Operation],
    /// Page des réglages (quand l'onglet affiché est sur Réglages)
    pub page: Option<VuePage<'a>>,
    /// Double panneau (F6) : le panneau qui a le focus est-il celui de droite ?
    pub double: bool,
    pub focus_droite: bool,
    /// Une touche a été utilisée depuis le dernier clic : le focus clavier se voit (anneau de 2 px).
    pub focus_clavier: bool,
    pub palette: Option<&'a Palette>,
    pub lot: Option<&'a Lot>,
    /// Coup d'œil : l'élément, sa position et le nombre d'éléments
    pub galerie: Option<(&'a Galerie, usize, usize)>,
    /// Copie en attente de réponses (noms déjà pris)
    pub conflits: Option<&'a Conflits>,
    /// Menu Nothing ouvert (barre d'actions, clic droit)
    pub menu: Option<&'a Menu>,
    /// Pastilles d'opération (de tous les onglets, dans l'ordre d'affichage) et celle qui est déployée
    pub pastilles: &'a [Pastille],
    pub pastille_ouverte: Option<usize>,
    pub pixel: VuePixel,
    /// Position du pointeur (DIP, fenêtre) pour le reflet ; `None` : souris absente ou touche pressée, aucun reflet
    pub pointeur: Option<(f32, f32)>,
    /// Soulèvement (DIP, -2 au plus, le ressort dépasse un peu) des éléments de chrome survolés ou qui viennent de l'être
    pub soulevements: &'a [(Cible, f32)],
    /// Ouverture ou fermeture de la fenêtre en cours (lot 12A) ; `None` : fenêtre normale
    pub fenetre: Option<EtatFenetre>,
    /// Cascade des lignes à l'ouverture (lot 12C) : ms écoulées depuis le début de l'ouverture ; `None` : terminée ou absente
    pub cascade: Option<u64>,
    /// Page des Réglages qu'on vient de quitter, dessinée encore 160 ms par-dessus le contenu qui apparaît (lot 12C)
    pub page_sortie: Option<VuePage<'a>>,
    /// Panneau Historique (lot 15B) : tiroir ouvert voulu, entrées à montrer (vides quand il est rentré pour de bon), défilement
    pub historique: VueHistorique<'a>,
}

/// Ce que le tiroir Historique montre
pub struct VueHistorique<'a> {
    pub ouvert: bool,
    /// Du plus récent au plus ancien
    pub entrees: &'a [crate::journal_historique::Entree],
    pub defilement: f32,
    /// « AAAA-MM-JJ HH:MM » : de quoi écrire « Hier » ou l'heure
    pub maintenant: String,
}

/// Tiroir Historique (`.hist` de la maquette) : 340 DIP de large, du haut de la zone de contenu au pied (`top:54px; bottom:40px`),
/// marge de 16, entrées de 51 DIP (9 + 33 + 9, filet de 1), bouton « Annuler » de 64 × 26
pub const TIROIR_L: f32 = 340.0;
const HIST_PAD: f32 = 16.0;
pub const HIST_RANG: f32 = 51.0;
const HIST_ENTETE: f32 = 48.0;
const HIST_NOTE_H: f32 = 28.0;
const HIST_BOUTON_L: f32 = 64.0;
const HIST_BOUTON_H: f32 = 26.0;
const HIST_TEMPS_L: f32 = 40.0;

/// Le tiroir sorti (sans décalage) dans une fenêtre de `l` × `h` DIP
pub fn zone_tiroir(l: f32, h: f32) -> D2D_RECT_F {
    rect(l - TIROIR_L, HAUTEUR_TITRE + 1.0, TIROIR_L, (h - STATUT_H - HAUTEUR_TITRE - 1.0).max(0.0))
}

/// Le bas de la liste : au-dessus de la marge, et de la note « session » quand elle est là
fn hist_bas_liste(zone: &D2D_RECT_F, avec_note: bool) -> f32 {
    zone.bottom - HIST_PAD - if avec_note { HIST_NOTE_H } else { 0.0 }
}

/// Défilement le plus bas du tiroir pour `n` entrées (DIP) ; 0 quand tout tient
pub fn historique_defilement_max(n: usize, zone: &D2D_RECT_F, avec_note: bool) -> f32 {
    let visible = hist_bas_liste(zone, avec_note) - (zone.top + HIST_ENTETE);
    (n as f32 * HIST_RANG - visible).max(0.0)
}

/// Où tombe un clic dans le tiroir
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CibleHistorique {
    /// Le bouton « Annuler » de l'entrée d'identifiant `id`
    Annuler(u64),
    /// Ailleurs dans le tiroir
    Tiroir,
    /// Hors du tiroir (ou tiroir rentré)
    Dehors,
}

/// Zones de clic du tiroir au dernier dessin, décalage du glissement compris
#[derive(Clone, Default)]
pub struct ZonesHistorique {
    tiroir: Option<D2D_RECT_F>,
    boutons: Vec<(D2D_RECT_F, u64)>,
}

impl ZonesHistorique {
    pub fn cible(&self, x: f32, y: f32) -> CibleHistorique {
        if let Some((_, id)) = self.boutons.iter().find(|(r, _)| dedans(r, x, y)) {
            return CibleHistorique::Annuler(*id);
        }
        match self.tiroir {
            Some(r) if dedans(&r, x, y) => CibleHistorique::Tiroir,
            _ => CibleHistorique::Dehors,
        }
    }
}

/// Mélange de deux couleurs (déjà passées par le thème) : `k` = 0 donne `a`, 1 donne `b`
fn melange_couleurs(a: &D2D1_COLOR_F, b: &D2D1_COLOR_F, k: f32) -> D2D1_COLOR_F {
    D2D1_COLOR_F { r: mouvement::melanger(a.r, b.r, k), g: mouvement::melanger(a.g, b.g, k), b: mouvement::melanger(a.b, b.b, k), a: mouvement::melanger(a.a, b.a, k) }
}

/// Traduit un rectangle vers le haut pour le survol du chrome, sans toucher à sa zone de clic.
fn decaler(r: D2D_RECT_F, dy: f32) -> D2D_RECT_F {
    D2D_RECT_F { left: r.left, top: r.top + dy, right: r.right, bottom: r.bottom + dy }
}

/// Décalage applicable à un élément précis : seul le chrome survolé se soulève.
fn decalage_chrome(_app: &App, vue: &Vue, cible: Cible) -> f32 {
    vue.soulevements.iter().find(|(c, _)| *c == cible).map_or(0.0, |(_, d)| *d)
}

/// Ce que Pixel doit dire à cet instant (calculé par la fenêtre : état, phase du mouvement, heure, relais vers la palette)
#[derive(Clone, Copy, Debug)]
pub struct VuePixel {
    pub etat: EtatPixel,
    pub periode: Periode,
    /// Phase du mouvement (voir `pixel::forme`)
    pub phase: f32,
    /// 0 : dans la barre d'état ; 1 : à gauche du champ de la palette ; entre les deux : en vol
    pub place: f32,
    /// Phase de la grille 3 × 3 des pastilles (0 à 1, un tour en 1,2 s comme `.hyp`) ; `None` : mouvement réduit, grille allumée et immobile
    pub grille: Option<f32>,
    /// Lot 13 : regard vers le pointeur, fermeture de la nuit, fondu de couleur et clignement (voir `mouvement::Yeux`)
    pub yeux: mouvement::VueYeux,
}

/// Couleur d'une teinte de Pixel (jetons de `theme.rs`)
fn couleur_teinte(t: pixel::Teinte) -> D2D1_COLOR_F {
    match t {
        pixel::Teinte::Cyan => PIXEL_JOUR,
        pixel::Teinte::Ambre => ACCENT,
        pixel::Teinte::Doux => BLANC_DOUX,
        pixel::Teinte::Faible => BLANC_FAIBLE,
        pixel::Teinte::Erreur => ERREUR,
    }
}

/// Palette des commandes (maquette `.pal`) : champ de 64 DIP, puis jauge et puces (4 + 44), corps de 330 au plus, pied de 46 ; aperçu de 270 de large
const PALETTE_CHAMP_H: f32 = 64.0;
const PALETTE_ENTETE: f32 = 112.0;
const PALETTE_PIED: f32 = 46.0;
const PALETTE_APERCU_L: f32 = 270.0;

/// Où tombe un clic dans la palette : une puce du fil d'Ariane (rang de l'étape où revenir), une ligne (rang dans `Palette::items`),
/// la carte ailleurs, ou hors de la carte
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum CiblePalette {
    Puce(usize),
    Ligne(usize),
    Carte,
    Dehors,
}

/// Zones de clic de la palette au dernier dessin
#[derive(Clone, Default)]
pub struct ZonesPalette {
    carte: Option<D2D_RECT_F>,
    puces: Vec<(usize, D2D_RECT_F)>,
    lignes: Vec<(usize, D2D_RECT_F)>,
    /// Clé (`Item::cle`) de chaque ligne dessinée, par son rang : un clic retrouve la ligne qu'on a vue même si la liste a changé depuis
    cles: Vec<(usize, String)>,
}

impl ZonesPalette {
    /// Clé de la ligne de rang `k` au dernier dessin
    pub fn cle_de(&self, k: usize) -> Option<&str> {
        self.cles.iter().find(|(r, _)| *r == k).map(|(_, c)| c.as_str())
    }

    pub fn cible(&self, x: f32, y: f32) -> CiblePalette {
        if let Some((c, _)) = self.puces.iter().find(|(_, r)| dedans(r, x, y)) {
            return CiblePalette::Puce(*c);
        }
        if let Some((i, _)) = self.lignes.iter().find(|(_, r)| dedans(r, x, y)) {
            return CiblePalette::Ligne(*i);
        }
        // Pas encore dessinée : un clic ne la ferme pas
        match self.carte {
            Some(r) if !dedans(&r, x, y) => CiblePalette::Dehors,
            _ => CiblePalette::Carte,
        }
    }
}

/// Place réservée à Pixel dans la barre d'état avant le texte : sa boîte (26 DIP) et l'espace
pub const PIXEL_DECALAGE: f32 = 36.0;

/// Bord gauche de la grille (coordonnées du panneau)
pub fn grille_x0() -> f32 {
    largeur_volet() + MARGE_LISTE + 6.0
}

/// Vignette de la grille sous ce point (coordonnées du panneau)
pub fn case_grille(app: &App, x: f32, y: f32) -> Option<usize> {
    let (par, hr) = app.pas();
    let pas_x = app.cote_grille + GRILLE_ECART;
    if x < grille_x0() || y < LISTE_Y {
        return None;
    }
    let col = ((x - grille_x0()) / pas_x) as usize;
    let dans_x = (x - grille_x0()) % pas_x;
    let rang = ((y - LISTE_Y + app.defilement) / hr) as usize;
    let dans_y = (y - LISTE_Y + app.defilement) % hr;
    if col >= par || dans_x > app.cote_grille || dans_y > app.cote_grille + GRILLE_NOM {
        return None;
    }
    let i = rang * par + col;
    (i < app.entrees.len()).then_some(i)
}

pub struct VuePage<'a> {
    pub lignes: &'a [Ligne],
    pub page: &'a Page,
    pub focus_clavier: bool,
}

/// Onglets puis bouton « + », dans la barre de titre, juste après le nom
pub fn zones_onglets(l: f32, n: usize) -> Vec<(Cible, D2D_RECT_F)> {
    let fin = zone_recherche_haut(l).left - 12.0;
    let n = n.max(1);
    let dispo = (fin - ONGLETS_X - BOUTON_HAUT - 8.0).max(0.0);
    let ecart = ONGLET_ECART * n.saturating_sub(1) as f32;
    let largeur = ((dispo - ecart) / n as f32).clamp(ONGLET_MIN, ONGLET_MAX);
    let y = (HAUTEUR_TITRE - 34.0) / 2.0;
    let mut v: Vec<(Cible, D2D_RECT_F)> = (0..n).map(|k| (Cible::Onglet(k), rect(ONGLETS_X + k as f32 * (largeur + ONGLET_ECART), y, largeur, 34.0))).collect();
    let x = ONGLETS_X + n as f32 * (largeur + ONGLET_ECART);
    if x + BOUTON_HAUT < fin {
        v.push((Cible::NouvelOnglet, rect(x, y, BOUTON_HAUT, BOUTON_HAUT)));
    }
    v
}

/// Trois icônes de la barre haute, avant les contrôles système
pub fn x_icones_haut(l: f32) -> f32 {
    l - 3.0 * BOUTON_LARGEUR - MARGE_HAUT - 3.0 * BOUTON_HAUT - 2.0 * ECART_HAUT
}

pub fn zone_icone_haut(l: f32, rang: usize) -> D2D_RECT_F {
    rect(x_icones_haut(l) + rang as f32 * (BOUTON_HAUT + ECART_HAUT), (HAUTEUR_TITRE - BOUTON_HAUT) / 2.0, BOUTON_HAUT, BOUTON_HAUT)
}

pub fn zone_recherche_haut(l: f32) -> D2D_RECT_F {
    zone_icone_haut(l, 0)
}

pub fn zone_essentiel_haut(l: f32) -> D2D_RECT_F {
    zone_icone_haut(l, 1)
}

pub fn zone_reglages_haut(l: f32) -> D2D_RECT_F {
    zone_icone_haut(l, 2)
}

/// Bouton « >_ » à droite du chemin, avant les autres éléments de droite
pub const TERMINAL_LARGEUR: f32 = 32.0;
const TERMINAL_RESERVE: f32 = 40.0;

pub fn zone_terminal(l: f32) -> D2D_RECT_F {
    rect(l - MARGE_LISTE - TERMINAL_LARGEUR, NAV_Y + 3.0, TERMINAL_LARGEUR, BOUTON_HAUT)
}

/// Panneau terminal : 196 DIP au-dessus du pied, sur toute la largeur de la liste (l'aperçu reste à droite).
pub const TERMINAL_PANNEAU_H: f32 = 196.0;

pub fn zone_terminal_panneau(l: f32, h: f32) -> D2D_RECT_F {
    let bas = h - STATUT_H;
    let haut = (bas - TERMINAL_PANNEAU_H).max(HAUTEUR_TITRE + 1.0);
    rect(largeur_volet() + 1.0, haut, (l - largeur_volet() - 1.0).max(0.0), (bas - haut).max(0.0))
}

pub fn zone_terminal_fermer(l: f32, h: f32) -> D2D_RECT_F {
    let p = zone_terminal_panneau(l, h);
    rect(p.right - 34.0, p.top + 8.0, 24.0, 24.0)
}

pub fn zone_terminal_action(l: f32, h: f32) -> D2D_RECT_F {
    let p = zone_terminal_panneau(l, h);
    rect((p.right - 20.0 - 236.0).max(p.left + 20.0), p.bottom - 40.0, 236.0, 28.0)
}

/// Carte du disque (O6) et Corbeille (O7) : blocs posés sous les disques de Ce PC
const CARTE_BLOC_L: f32 = 360.0;
const CARTE_BLOC_H: f32 = 176.0;

/// Coin haut gauche du bloc `k` (disques, puis la Corbeille), `y0` : sous la dernière ligne
fn bloc_carte(l: f32, y0: f32, k: usize) -> (f32, f32) {
    let x_debut = largeur_volet() + MARGE_LISTE + 4.0;
    let par_rangee = (((l - x_debut - MARGE_LISTE) / CARTE_BLOC_L).floor() as usize).max(1);
    (x_debut + (k % par_rangee) as f32 * CARTE_BLOC_L, y0 + (k / par_rangee) as f32 * CARTE_BLOC_H)
}

/// Disques de Ce PC qui ont une carte
fn disques_carte(app: &App) -> impl Iterator<Item = (&str, u64, u64)> {
    app.entrees.iter().filter_map(|e| match &e.genre {
        Genre::Disque { racine, libre, total } if *total > 0 => Some((racine.trim_end_matches('\\'), *libre, *total)),
        _ => None,
    })
}

fn haut_carte(app: &App) -> f32 {
    LISTE_Y + app.nb_rangs() as f32 * hauteur_ligne() - app.defilement + 26.0
}

/// Centre de Pixel dans la barre d'état (coordonnées de la fenêtre, DIP)
pub fn pixel_dans_statut(h: f32) -> (f32, f32) {
    (largeur_volet() + MARGE_LISTE + pixel::LARGEUR / 2.0, h - STATUT_H / 2.0)
}

/// Centre de Pixel dans la palette : à gauche du champ de saisie, là où était son point d'accent
pub fn pixel_dans_palette(l: f32) -> (f32, f32) {
    let w = (l - 120.0).clamp(320.0, 780.0);
    ((l - w) / 2.0 + 30.0, HAUTEUR_TITRE + 40.0 + 32.0)
}

// ── Touches de clavier (phase 7 lot C1) ──
// Un raccourci s'affiche en petits cadres, une touche par cadre (maquette `.k` : cadre 1 px, rayon 6, texte 11 DIP en police
// de texte, jamais en Doto sous 14). La chasse de JetBrains Mono est fixe (0,6 em) : la largeur d'un cadre se calcule sans
// mesure, ce qui laisse `menu.rs` dimensionner ses cartes sans accès au rendu.
/// Marge horizontale d'un segment du chemin (`.crumb`, 2 / 8) et espace de part et d'autre du séparateur (`.path`, gap 8)
const CRUMB_MARGE: f32 = 8.0;
const CRUMB_ECART: f32 = 8.0;
const TOUCHE_TAILLE: f32 = 11.0;
const TOUCHE_H: f32 = 18.0;
const TOUCHE_MARGE: f32 = 6.0;
const TOUCHE_ESPACE: f32 = 4.0;
const TOUCHE_RAYON: f32 = 6.0;

// ── Champ de filtre (`.spill`, lot 14) ──
/// Puces proposées dans le champ (`data-t` de la maquette) : un clic ajoute le jeton au filtre
pub const PUCES_FILTRE: [&str; 3] = ["ext:", "taille>", "modif:"];
/// Place de la loupe avant le texte, marge à droite après la touche Échap
const FILTRE_GAUCHE: f32 = 30.0;
const FILTRE_DROITE: f32 = 12.0;
/// `.chip` : marge de 8 de chaque côté du texte, 20 de haut, 6 entre deux puces
const PUCE_MARGE: f32 = 8.0;
const PUCE_H: f32 = 20.0;
const PUCE_ECART: f32 = 6.0;
/// Place du point de couleur devant le nom d'un groupe dans le chemin
const GROUPE_POINT: f32 = 16.0;

/// Mesure du champ de filtre à pleine ouverture
struct MesureFiltre {
    plein: f32,
    /// Largeur réservée au texte tapé
    texte: f32,
    /// Les puces tiennent-elles ?
    puces: bool,
}

/// Les touches d'un raccourci : `"CTRL+MAJ+P"` → `["CTRL", "MAJ", "P"]`. Un « + » sans touche avant lui est la touche « + »
/// (`"CTRL++"` → `["CTRL", "+"]`). Une touche peut contenir des espaces (`"ALT+FLECHE GAUCHE"`).
pub fn decouper_touches(raccourci: &str) -> Vec<String> {
    let mut touches = Vec::new();
    let mut courante = String::new();
    for ch in raccourci.chars() {
        if ch != '+' {
            courante.push(ch);
        } else if courante.trim().is_empty() {
            touches.push("+".to_string());
            courante.clear();
        } else {
            touches.push(courante.trim().to_string());
            courante.clear();
        }
    }
    if !courante.trim().is_empty() {
        touches.push(courante.trim().to_string());
    }
    touches
}

fn touche_hauteur() -> f32 {
    // Les lignes ont une hauteur fixe en DIP : le cadre suit le texte agrandi de Windows, mais pas au-delà
    (TOUCHE_H * crate::reglages::echelle_texte()).min(24.0)
}

fn largeur_touche(texte: &str) -> f32 {
    (texte.chars().count() as f32 * 0.6 * TOUCHE_TAILLE * crate::reglages::echelle_texte() + 2.0 * TOUCHE_MARGE).max(touche_hauteur())
}

/// Largeur d'un raccourci dessiné en touches (DIP) ; 0 pour une chaîne vide
pub fn largeur_touches(raccourci: &str) -> f32 {
    let touches = decouper_touches(raccourci);
    touches.iter().map(|t| largeur_touche(t)).sum::<f32>() + TOUCHE_ESPACE * touches.len().saturating_sub(1) as f32
}

/// Un morceau d'un texte d'aide : texte simple, ou raccourci à dessiner en touches
#[derive(Debug, PartialEq, Eq)]
pub enum Morceau<'a> {
    Texte(&'a str),
    Touches(&'a str),
}

/// Début et fin (octets) du mot sans la ponctuation qui l'entoure : `(CTRL+R) :` → `CTRL+R`. Une virgule après un « + » est la
/// touche « , » (`CTRL+,`), pas de la ponctuation.
fn noyau(mot: &str) -> (usize, usize) {
    let debut = mot.len() - mot.trim_start_matches('(').len();
    let mut fin = mot.len();
    while fin > debut {
        let reste = &mot[..fin];
        let Some(dernier) = reste.chars().next_back() else { break };
        let ponctuation = matches!(dernier, ')' | ':' | '.' | ';') || (dernier == ',' && !reste[..fin - 1].ends_with('+'));
        if !ponctuation {
            break;
        }
        fin -= dernier.len_utf8();
    }
    (debut, fin)
}

/// Ce mot est-il un raccourci ? Une combinaison (`CTRL+F`, `MAJ+CLIC`) ou une touche seule des aides (`ENTREE`, `ECHAP`, `F2`).
/// Liste fermée : seuls ces noms de touches, jamais un mot quelconque d'un message.
fn est_raccourci(mot: &str) -> bool {
    if let Some((tete, reste)) = mot.split_once('+') {
        return matches!(tete, "CTRL" | "ALT" | "MAJ" | "SHIFT" | "WIN") && !reste.is_empty();
    }
    let fonction = mot.strip_prefix('F').and_then(|n| n.parse::<u8>().ok()).is_some_and(|n| (1..=12).contains(&n)) && mot[1..].chars().all(|c| c.is_ascii_digit());
    fonction || matches!(mot, "ENTREE" | "ENTRÉE" | "ECHAP" | "ÉCHAP" | "TAB" | "ESPACE" | "SUPPR")
}

/// Coupe un texte d'aide (« ENTREE OUVRE · CTRL+F : SOUS-DOSSIERS ») en texte simple et raccourcis. Les espaces restent dans
/// le texte simple : concaténés, les morceaux redonnent le texte d'entrée.
pub fn segmenter_aide(texte: &str) -> Vec<Morceau<'_>> {
    let mut morceaux = Vec::new();
    let mut simple = 0;
    let mut mot: Option<usize> = None;
    for (i, ch) in texte.char_indices().chain(std::iter::once((texte.len(), ' '))) {
        if !ch.is_whitespace() {
            mot.get_or_insert(i);
            continue;
        }
        let Some(d) = mot.take() else { continue };
        let (a, b) = noyau(&texte[d..i]);
        if est_raccourci(&texte[d + a..d + b]) {
            if simple < d + a {
                morceaux.push(Morceau::Texte(&texte[simple..d + a]));
            }
            morceaux.push(Morceau::Touches(&texte[d + a..d + b]));
            simple = d + b;
        }
    }
    if simple < texte.len() {
        morceaux.push(Morceau::Texte(&texte[simple..]));
    }
    morceaux
}

/// Où se pose un bloc « texte et touches » dans sa zone
#[derive(Clone, Copy, PartialEq, Eq)]
enum Pose {
    Gauche,
    Droite,
    Centre,
}

struct Formats {
    code: IDWriteTextFormat,
    titre: IDWriteTextFormat,
    /// Nom de l'élément dans l'aperçu : Segoe UI 18, graisse moyenne (maquette v6)
    apercu_nom: IDWriteTextFormat,
    chemin: IDWriteTextFormat,
    petit: IDWriteTextFormat,
    petit_droite: IDWriteTextFormat,
    /// Texte d'une touche de clavier (Segoe, centré dans son cadre)
    touche: IDWriteTextFormat,
    petit_centre: IDWriteTextFormat,
    liste: IDWriteTextFormat,
    liste_droite: IDWriteTextFormat,
    /// Texte courant du volet et des pastilles : Segoe 13,5
    volet: IDWriteTextFormat,
    /// Noms des lieux, groupes, disques et épingles du volet (`.it .d` de la maquette) : Doto 14 gras, espacement 1 px (voir `texte_espace`)
    volet_nom: IDWriteTextFormat,
    /// Valeurs et étiquettes du volet (Doto, comme `petit` mais plus grand)
    valeur: IDWriteTextFormat,
    /// Texte courant à petite taille : détails, chemins, noms d'onglets (mono)
    petit_mono: IDWriteTextFormat,
    /// Pourcentage des pastilles, aligné à droite (Segoe)
    volet_droite: IDWriteTextFormat,
    /// Texte des boutons de pastille (mono, centré)
    mono_centre: IDWriteTextFormat,
    /// Titre d'une page de réglages (maquette `.pg h2`) : Segoe 22, graisse moyenne
    h2: IDWriteTextFormat,
    /// Titre d'une carte de réglage (`.scard .tt`) : Segoe 14, graisse moyenne
    carte_titre: IDWriteTextFormat,
    /// Description d'une carte (`.scard .ds`) : Segoe 12, à la ligne, en haut de sa zone (deux lignes au plus, voir `deux_lignes`)
    carte_desc: IDWriteTextFormat,
    /// Texte de l'état vide de la liste (`.empty`) : Segoe 14, centré
    vide: IDWriteTextFormat,
    /// Titre de l'aperçu de la palette (`.pv .vt`) : Doto 20 gras, à la ligne, en haut de sa zone
    apercu_titre: IDWriteTextFormat,
    /// Grand « + » de l'aperçu « Nouveau dossier » (`.big`) : Doto 34 gras, centré
    apercu_grand: IDWriteTextFormat,
}

/// Ressources liées à la cible de rendu (recréées si le pilote graphique la perd)
struct Cible2D {
    /// Thème clair au moment de la création : icônes et trame sont des images peintes dans ce thème, la cible est recréée
    /// au dessin dès que `theme::clair()` diffère
    clair: bool,
    rt: ID2D1HwndRenderTarget,
    pinceau: ID2D1SolidColorBrush,
    trame: ID2D1BitmapBrush,
    /// Icônes à points pré-dessinées (dans l'ordre de `icones::TOUTES`) : avec accent rouge, et toutes blanches
    icones: Vec<ID2D1Bitmap>,
    icones_blanches: Vec<ID2D1Bitmap>,
    /// Miniature de l'aperçu, convertie une fois (numéro de l'aperçu, image)
    miniature: RefCell<Option<(u64, ID2D1Bitmap)>>,
    /// Calque pour l'apparition en fondu des lignes
    couche: ID2D1Layer,
    /// Vignettes de la grille déjà converties (chemin → image)
    vignettes: RefCell<HashMap<String, ID2D1Bitmap>>,
    /// Image du coup d'œil (numéro, image)
    galerie: RefCell<Option<(u64, ID2D1Bitmap)>>,
    /// Pinceau radial du reflet, créé une fois avec la cible : blanc (encre en clair) de 100 % au centre à 0 % au rayon. Chaque
    /// reflet ne change que son centre, son rayon et son opacité (jamais de pinceau créé par image)
    reflet: ID2D1RadialGradientBrush,
    /// Dégradé de la pastille png (135°, bleu puis ambre à 55 %) dans le carré unité : chaque dessin n'en change que la
    /// transformation (échelle, position), jamais de pinceau créé par image
    degrade_png: ID2D1LinearGradientBrush,
}

/// Élément que le reflet éclaire : sa zone (DIP, fenêtre), le rayon de coin de son fond, s'il est désactivé, et un rognage éventuel
struct CibleReflet {
    zone: D2D_RECT_F,
    coin: f32,
    desactive: bool,
    clip: Option<D2D_RECT_F>,
}

const COTE_ICONE: f32 = 22.0;

/// Dessine une icône une fois pour toutes dans une petite image
unsafe fn creer_icone(rt: &ID2D1HwndRenderTarget, icone: Icone, blanche: bool) -> Result<ID2D1Bitmap> {
    let cible = rt.CreateCompatibleRenderTarget(
        Some(&D2D_SIZE_F { width: COTE_ICONE, height: COTE_ICONE }),
        None,
        None,
        D2D1_COMPATIBLE_RENDER_TARGET_OPTIONS_NONE,
    )?;
    cible.BeginDraw();
    cible.Clear(Some(&rgba(0, 0, 0, 0.0)));
    // Images créées une fois : elles prennent la couleur du thème du moment, et la cible est recréée quand il change
    let blanc = cible.CreateSolidColorBrush(&crate::theme::teinte_fixe(&BLANC), None)?;
    let rouge = cible.CreateSolidColorBrush(&crate::theme::teinte_fixe(if blanche { &BLANC } else { &ACCENT }), None)?;
    let motif = icone.motif();
    let colonnes = motif.iter().map(|l| l.len()).max().unwrap_or(1) as f32;
    let lignes = motif.len() as f32;
    let pas = 2.8;
    let ox = (COTE_ICONE - (colonnes - 1.0) * pas) / 2.0;
    let oy = (COTE_ICONE - (lignes - 1.0) * pas) / 2.0;
    for (j, ligne) in motif.iter().enumerate() {
        for (i, c) in ligne.bytes().enumerate() {
            let pinceau = match c {
                b'#' => &blanc,
                b'r' => &rouge,
                _ => continue,
            };
            cible.FillEllipse(&ellipse(ox + i as f32 * pas, oy + j as f32 * pas, 1.05), pinceau);
        }
    }
    cible.EndDraw(None, None)?;
    cible.GetBitmap()
}

/// Volet de gauche : titres de section (texte, y) et zones (favoris, cloud, serveurs, disques).
/// Une section vide (pas de cloud, pas de serveur) n'apparaît pas.
/// Sections du volet, dans l'ordre (les bits des réglages volet_replie / volet_masque suivent ce rang)
pub const SECTIONS_VOLET: [&str; 6] = ["FAVORIS", "ÉPINGLES", "GROUPES", "CLOUD", "SERVEURS", "DISQUES"];

/// Volet : titres (texte, y, repliée ?, nombre d'éléments) et zones cliquables
pub fn disposition_volet(app: &App) -> (Vec<(&'static str, f32, bool, usize)>, Vec<(Cible, D2D_RECT_F)>) {
    let (mut titres, mut v) = (Vec::new(), Vec::new());
    // Mode Essentiel : le volet est replié, il n'a ni titre ni zone
    if largeur_volet() == 0.0 {
        return (titres, v);
    }
    // Le défilement du volet n'existe que pour l'arbre : sans lui, le volet reste en place
    let mut y = VOLET_Y - if crate::reglages::ARBRE.load(std::sync::atomic::Ordering::Relaxed) { app.volet_defilement } else { 0.0 };
    let replie = crate::reglages::VOLET_REPLIE.load(std::sync::atomic::Ordering::Relaxed);
    let masque = crate::reglages::VOLET_MASQUE.load(std::sync::atomic::Ordering::Relaxed);
    let arbre_n = if crate::reglages::ARBRE.load(std::sync::atomic::Ordering::Relaxed) { app.lignes_arbre().len() } else { 0 };
    let (x, w) = (VOLET_PAD, largeur_volet() - 2.0 * VOLET_PAD);
    // Entrée fixe de la maquette : Ce PC, avant les sections
    v.push((Cible::CePc, rect(x, y, w, volet_ligne() - 2.0)));
    y += volet_ligne() + 8.0;
    let sections: [(&'static str, usize, fn(usize) -> Cible, f32); 7] = [
        ("FAVORIS", app.favoris.len(), Cible::Favori, volet_ligne()),
        ("ÉPINGLES", app.epingles.len(), Cible::Epingle, volet_ligne()),
        ("GROUPES", app.groupes.len(), Cible::Groupe, volet_ligne()),
        ("CLOUD", app.nuages.len(), Cible::Nuage, volet_ligne()),
        ("SERVEURS", app.serveurs.len(), Cible::Serveur, volet_ligne()),
        ("DISQUES", app.disques.len(), Cible::Disque, VOLET_DISQUE),
        ("ARBORESCENCE", arbre_n, Cible::Arbre, volet_ligne()),
    ];
    let essentiel = crate::app::ESSENTIEL.load(std::sync::atomic::Ordering::Relaxed);
    for (k, (titre, n, cible, pas)) in sections.into_iter().enumerate() {
        if (n == 0 && titre != "FAVORIS" && titre != "DISQUES") || masque & (1 << k) != 0 || (essentiel && titre != "ÉPINGLES") {
            continue;
        }
        let repliee = replie & (1 << k) != 0;
        titres.push((titre, y, repliee, n));
        v.push((Cible::TitreVolet(k as u8), rect(x, y, w, VOLET_ENTETE)));
        y += VOLET_ENTETE;
        if repliee {
            y += VOLET_ESPACE;
            continue;
        }
        for i in 0..n {
            let marge = if pas == VOLET_DISQUE { 4.0 } else { 2.0 };
            v.push((cible(i), rect(x, y + i as f32 * pas, w, pas - marge)));
        }
        y += n as f32 * pas + VOLET_ESPACE;
    }
    (titres, v)
}

/// Nom affiché d'un élément : sans l'extension quand le réglage les cache (dossiers, archives
/// et noms commençant par un point exceptés)
/// Côté d'un carré d'étiquette et écart entre deux carrés, en DIP (`.sq i{width:5px;height:5px}`, `.sq{gap:2px}`)
const CARRE_ETIQUETTE: f32 = 5.0;
const ECART_ETIQUETTE: f32 = 2.0;

/// Largeur de `n` carrés d'étiquette
fn largeur_etiquette(n: u8) -> f32 {
    n as f32 * CARRE_ETIQUETTE + (n as f32 - 1.0).max(0.0) * ECART_ETIQUETTE
}

fn nom_vu(e: &Entree) -> &[u16] {
    let n = &e.nom_w[..e.nom_w.len() - 1];
    if crate::reglages::EXTENSIONS.load(std::sync::atomic::Ordering::Relaxed) || e.est_conteneur() {
        return n;
    }
    match n.iter().rposition(|&c| c == b'.' as u16) {
        Some(p) if p > 0 => &n[..p],
        _ => n,
    }
}

/// Place réservée à l'étiquette de la chronologie, à gauche des points
const CHRONO_GAUCHE: f32 = 150.0;

/// Chronologie : un point par jour jusqu'à aujourd'hui (autant que la largeur permet, 120 jours au plus)
fn placer_jours(app: &App, l: f32, h: f32) -> Vec<(D2D_RECT_F, u32, usize)> {
    let x0 = largeur_volet() + MARGE_LISTE + CHRONO_GAUCHE;
    let x1 = l - MARGE_LISTE - 10.0;
    const PAS: f32 = 11.0;
    let n = (((x1 - x0) / PAS).floor() as i64).clamp(0, 120) as u32;
    if n < 7 {
        return Vec::new();
    }
    let aujourdhui = systeme::numero_jour(systeme::maintenant());
    let mut comptes = HashMap::new();
    for e in &app.tout {
        *comptes.entry(systeme::numero_jour(e.date_colonne())).or_insert(0usize) += 1;
    }
    let y = h - STATUT_H - CHRONO_H;
    (0..n)
        .map(|k| {
            let j = aujourdhui - (n - 1 - k);
            (rect(x1 - (n - k) as f32 * PAS, y, PAS, CHRONO_H), j, comptes.get(&j).copied().unwrap_or(0))
        })
        .collect()
}

pub fn zones_volet(app: &App) -> Vec<(Cible, D2D_RECT_F)> {
    disposition_volet(app).1
}

pub struct Rendu {
    d2d: ID2D1Factory,
    dwrite: IDWriteFactory,
    f: Formats,
    cible: Option<Cible2D>,
    /// Vignettes reçues, pas encore converties (None : le fichier n'en a pas)
    images: RefCell<HashMap<String, Option<Image>>>,
    /// Vignettes demandées (pour ne pas les redemander)
    demandees: RefCell<HashSet<String>>,
    /// Vignettes à demander après le dessin
    pub a_demander: RefCell<Vec<String>>,
    /// Boutons de la carte des conflits, pour les clics
    pub zones_conflit: RefCell<Vec<(D2D_RECT_F, Choix)>>,
    /// Pastilles posées au dernier dessin : le survol et les clics les lisent (en DIP, coordonnées de la fenêtre)
    pub zones_pastilles: RefCell<Disposition>,
    /// Zone éclairée par le reflet au dernier dessin (DIP, fenêtre) : `None` si aucun reflet. La souris l'invalide seule quand
    /// elle bouge dessus, et n'invalide rien quand elle bouge ailleurs
    pub zone_reflet: Cell<Option<D2D_RECT_F>>,
    /// Élément éclairable au dernier dessin, pointeur dedans ou non : invalidé aussi par la souris (voir `dessiner_reflet`)
    pub zone_candidate: Cell<Option<D2D_RECT_F>>,
    /// Style de trait à bouts ronds
    trait_rond: ID2D1StrokeStyle,
    /// Glissements en cours (pastille de sélection, indicateur de vue, liseré d'onglet), lus et mis à jour au dessin
    pub mouvements: RefCell<Mouvements>,
    /// Heure (`mouvement::maintenant_ms`) prise au début de l'image : tout l'image utilise la même
    t_image: Cell<u64>,
    /// Rang du panneau de liste en cours de dessin (0 gauche, 1 droite), pour suivre une pastille par panneau
    panneau: Cell<usize>,
    /// Rang (0 gauche, 1 droite) du panneau dont le contenu se dessine : il choisit la pastille et le glissement d'entrées à suivre
    panneau_en_cours: Cell<usize>,
    /// Cascade d'ouverture (lot 12C) : ms écoulées depuis le début de l'ouverture pour l'image en cours ; `None` : pas de cascade
    cascade: Cell<Option<u64>>,
    /// Lot 12D : opacité du contenu du volet et de l'aperçu pour l'image en cours (`.side, .prev`, 250 ms)
    fondu_volets: Cell<(f32, f32)>,
    /// Lot 14 : (part de la largeur pleine, opacité) du champ de filtre de chaque panneau pour l'image en cours (`.spill`)
    pilules: Cell<[(f32, f32); 2]>,
    /// Lot 12D : la palette telle qu'elle était au dernier dessin, de quoi finir sa fermeture quand l'application l'a déjà fermée
    derniere_palette: RefCell<Option<Palette>>,
    /// Lot 15A : zones de clic de la palette au dernier dessin, et les premiers éléments du dossier montré par l'aperçu « Aller à »
    pub zones_palette: RefCell<ZonesPalette>,
    /// Lot 15B : zones de clic du tiroir Historique au dernier dessin
    pub zones_historique: RefCell<ZonesHistorique>,
}

/// Clé d'une entrée pour le glissement (lot 12B) : son dossier et son nom, unique même dans des résultats de recherche
pub(crate) fn cle_flip(e: &Entree) -> String {
    match &e.parent {
        Some(p) => format!("{p}\\{}", e.nom),
        None => e.nom.clone(),
    }
}

/// Une place de pastille déplacée de (`dx`, `dy`) : toutes ses zones suivent (sortie d'une carte, posée là où elle était affichée)
fn translater_place(p: &Place, dx: f32, dy: f32) -> Place {
    let t = |z: pastilles::Zone| pastilles::Zone { x: z.x + dx, y: z.y + dy, ..z };
    let mut q = p.clone();
    q.zone = t(p.zone);
    q.tete = t(p.tete);
    q.zone_libelle = t(p.zone_libelle);
    q.jauge = p.jauge.map(t);
    q.pourcent = p.pourcent.map(t);
    q.etiquette = p.etiquette.as_ref().map(|(s, z)| (s.clone(), t(*z)));
    q.message = p.message.as_ref().map(|(s, z)| (s.clone(), t(*z)));
    q.destination = p.destination.as_ref().map(|(s, z)| (s.clone(), t(*z)));
    q.touches = p.touches.map(t);
    q.boutons = p.boutons.iter().map(|(b, z, s)| (*b, t(*z), s.clone())).collect();
    q
}

/// `local` appliquée d'abord, puis `base` (matrices Direct2D en vecteurs lignes) : le glissement d'une entrée se pose par-dessus la
/// transformation déjà en place (double panneau, ouverture de la fenêtre)
fn composer(local: &Matrix3x2, base: &Matrix3x2) -> Matrix3x2 {
    Matrix3x2 {
        M11: local.M11 * base.M11 + local.M12 * base.M21,
        M12: local.M11 * base.M12 + local.M12 * base.M22,
        M21: local.M21 * base.M11 + local.M22 * base.M21,
        M22: local.M21 * base.M12 + local.M22 * base.M22,
        M31: local.M31 * base.M11 + local.M32 * base.M21 + base.M31,
        M32: local.M31 * base.M12 + local.M32 * base.M22 + base.M32,
    }
}

fn format_famille(dw: &IDWriteFactory, famille: &str, taille: f32, graisse: DWRITE_FONT_WEIGHT, align: DWRITE_TEXT_ALIGNMENT) -> Result<IDWriteTextFormat> {
    let taille = taille * crate::reglages::echelle_texte();
    unsafe {
        let f = dw.CreateTextFormat(
            &HSTRING::from(famille),
            None,
            graisse,
            DWRITE_FONT_STYLE_NORMAL,
            DWRITE_FONT_STRETCH_NORMAL,
            taille,
            &HSTRING::from("fr-fr"),
        )?;
        f.SetParagraphAlignment(DWRITE_PARAGRAPH_ALIGNMENT_CENTER)?;
        f.SetTextAlignment(align)?;
        f.SetWordWrapping(DWRITE_WORD_WRAPPING_NO_WRAP)?;
        // Texte trop long : coupé avec « … »
        let signe = dw.CreateEllipsisTrimmingSign(&f)?;
        let coupe = DWRITE_TRIMMING { granularity: DWRITE_TRIMMING_GRANULARITY_CHARACTER, delimiter: 0, delimiterCount: 0 };
        f.SetTrimming(&coupe, &signe)?;
        Ok(f)
    }
}

/// Titres et marques : Doto
fn format_texte(dw: &IDWriteFactory, taille: f32, graisse: DWRITE_FONT_WEIGHT, align: DWRITE_TEXT_ALIGNMENT) -> Result<IDWriteTextFormat> {
    format_famille(dw, POLICE, taille, graisse, align)
}

/// Interface (libellés, colonnes, états, valeurs) : Segoe UI Variable Text, comme la maquette v6 ; repli sur Segoe UI
fn format_sans(dw: &IDWriteFactory, taille: f32, graisse: DWRITE_FONT_WEIGHT, align: DWRITE_TEXT_ALIGNMENT) -> Result<IDWriteTextFormat> {
    for famille in ["Segoe UI Variable Text", "Segoe UI"] {
        if let Ok(f) = format_famille(dw, famille, taille, graisse, align) {
            return Ok(f);
        }
    }
    format_famille(dw, "Segoe UI", taille, graisse, align)
}

thread_local! {
    /// JetBrains Mono embarquée, chargée une fois (None : le chargement a échoué, on retombe sur Cascadia Mono)
    static COLLECTION_MONO: RefCell<Option<Option<IDWriteFontCollection>>> = const { RefCell::new(None) };
}

/// Fichier de la police mono, embarqué dans l'exe : rien n'est installé sur la machine
static JETBRAINS_MONO: &[u8] = include_bytes!("../assets/fonts/JetBrainsMono-Regular.ttf");

/// Collection privée qui contient JetBrains Mono (chargeur de polices en mémoire de DirectWrite 5, Windows 10 1809+)
fn collection_mono(dw: &IDWriteFactory) -> Option<IDWriteFontCollection> {
    COLLECTION_MONO.with(|c| {
        c.borrow_mut()
            .get_or_insert_with(|| unsafe {
                let f5: IDWriteFactory5 = dw.cast().ok()?;
                let chargeur = f5.CreateInMemoryFontFileLoader().ok()?;
                dw.RegisterFontFileLoader(&chargeur).ok()?;
                // SAFETY: JETBRAINS_MONO est 'static, donc vivant tant que le chargeur existe (il n'est jamais retiré)
                let fichier = chargeur.CreateInMemoryFontFileReference(dw, JETBRAINS_MONO.as_ptr() as *const _, JETBRAINS_MONO.len() as u32, None).ok()?;
                let b = f5.CreateFontSetBuilder().ok()?;
                b.AddFontFile(&fichier).ok()?;
                let jeu = b.CreateFontSet().ok()?;
                let col: IDWriteFontCollection1 = f5.CreateFontCollectionFromFontSet(&jeu).ok()?;
                col.cast().ok()
            })
            .clone()
    })
}

/// Format en police mono (noms, chemins, dates, texte courant) : une seule graisse, donc jamais de gras simulé
fn format_mono(dw: &IDWriteFactory, taille: f32, align: DWRITE_TEXT_ALIGNMENT) -> Result<IDWriteTextFormat> {
    let taille = taille * crate::reglages::echelle_texte();
    unsafe {
        let col = collection_mono(dw);
        let nom = if col.is_some() { POLICE_TEXTE } else { POLICE_TEXTE_SECOURS };
        let f = dw.CreateTextFormat(&HSTRING::from(nom), col.as_ref(), DWRITE_FONT_WEIGHT_NORMAL, DWRITE_FONT_STYLE_NORMAL, DWRITE_FONT_STRETCH_NORMAL, taille, &HSTRING::from("fr-fr"))?;
        f.SetParagraphAlignment(DWRITE_PARAGRAPH_ALIGNMENT_CENTER)?;
        f.SetTextAlignment(align)?;
        f.SetWordWrapping(DWRITE_WORD_WRAPPING_NO_WRAP)?;
        let signe = dw.CreateEllipsisTrimmingSign(&f)?;
        let coupe = DWRITE_TRIMMING { granularity: DWRITE_TRIMMING_GRANULARITY_CHARACTER, delimiter: 0, delimiterCount: 0 };
        f.SetTrimming(&coupe, &signe)?;
        Ok(f)
    }
}

pub fn rect(x: f32, y: f32, l: f32, h: f32) -> D2D_RECT_F {
    D2D_RECT_F { left: x, top: y, right: x + l, bottom: y + h }
}

fn ellipse(x: f32, y: f32, r: f32) -> D2D1_ELLIPSE {
    D2D1_ELLIPSE { point: D2D_POINT_2F { x, y }, radiusX: r, radiusY: r }
}

pub fn dedans(r: &D2D_RECT_F, x: f32, y: f32) -> bool {
    x >= r.left && x < r.right && y >= r.top && y < r.bottom
}

/// Rectangles des trois boutons de navigation
pub fn boutons_nav() -> [(Cible, D2D_RECT_F); 3] {
    [
        (Cible::Precedent, rect(nav_x(), NAV_Y + 4.0, NAV_BOUTON, NAV_H - 8.0)),
        (Cible::Suivant, rect(nav_x() + NAV_BOUTON, NAV_Y + 4.0, NAV_BOUTON, NAV_H - 8.0)),
        (Cible::Parent, rect(nav_x() + 2.0 * NAV_BOUTON, NAV_Y + 4.0, NAV_BOUTON, NAV_H - 8.0)),
    ]
}

impl Rendu {
    pub fn nouveau() -> Result<Self> {
        unsafe {
            let d2d: ID2D1Factory = D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, None)?;
            let dw: IDWriteFactory = DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED)?;
            let g = DWRITE_TEXT_ALIGNMENT_LEADING;
            let d = DWRITE_TEXT_ALIGNMENT_TRAILING;
            // Aperçu des fichiers texte : police à chasse fixe lisible, lignes non coupées, en haut à gauche
            let code = dw.CreateTextFormat(
                &HSTRING::from(if collection_mono(&dw).is_some() { POLICE_TEXTE } else { POLICE_TEXTE_SECOURS }),
                collection_mono(&dw).as_ref(),
                DWRITE_FONT_WEIGHT_NORMAL,
                DWRITE_FONT_STYLE_NORMAL,
                DWRITE_FONT_STRETCH_NORMAL,
                11.0 * crate::reglages::echelle_texte(),
                &HSTRING::from("fr-fr"),
            )?;
            code.SetWordWrapping(DWRITE_WORD_WRAPPING_NO_WRAP)?;
            let f = Formats {
                code,
                titre: format_texte(&dw, 18.0, DWRITE_FONT_WEIGHT_BOLD, g)?,
                apercu_nom: format_sans(&dw, 18.0, DWRITE_FONT_WEIGHT_MEDIUM, g)?,
                chemin: format_texte(&dw, 22.0, DWRITE_FONT_WEIGHT_BOLD, g)?,
                // Libellés de l'interface : sans empattement, comme la maquette v6 (`--lab`, `.cols`, `.status`)
                petit: format_sans(&dw, 12.0, DWRITE_FONT_WEIGHT_NORMAL, g)?,
                petit_droite: format_sans(&dw, 12.0, DWRITE_FONT_WEIGHT_NORMAL, d)?,
                // Les touches de la maquette v6 sont en Segoe, pas en mono (lot 5)
                touche: format_sans(&dw, TOUCHE_TAILLE, DWRITE_FONT_WEIGHT_NORMAL, DWRITE_TEXT_ALIGNMENT_CENTER)?,
                petit_centre: format_sans(&dw, 12.0, DWRITE_FONT_WEIGHT_NORMAL, DWRITE_TEXT_ALIGNMENT_CENTER)?,
                liste: format_sans(&dw, 14.0, DWRITE_FONT_WEIGHT_NORMAL, g)?,
                liste_droite: format_sans(&dw, 13.0, DWRITE_FONT_WEIGHT_NORMAL, d)?,
                volet: format_sans(&dw, 13.5, DWRITE_FONT_WEIGHT_NORMAL, g)?,
                volet_nom: format_texte(&dw, 14.0, DWRITE_FONT_WEIGHT_BOLD, g)?,
                valeur: format_sans(&dw, 13.0, DWRITE_FONT_WEIGHT_NORMAL, g)?,
                petit_mono: format_mono(&dw, 12.0, g)?,
                volet_droite: format_sans(&dw, 12.0, DWRITE_FONT_WEIGHT_NORMAL, d)?,
                mono_centre: format_mono(&dw, 12.0, DWRITE_TEXT_ALIGNMENT_CENTER)?,
                h2: format_sans(&dw, 22.0, DWRITE_FONT_WEIGHT_MEDIUM, g)?,
                carte_titre: format_sans(&dw, 14.0, DWRITE_FONT_WEIGHT_MEDIUM, g)?,
                carte_desc: {
                    let d = format_sans(&dw, 12.0, DWRITE_FONT_WEIGHT_NORMAL, g)?;
                    d.SetWordWrapping(DWRITE_WORD_WRAPPING_WRAP)?;
                    d.SetParagraphAlignment(DWRITE_PARAGRAPH_ALIGNMENT_NEAR)?;
                    d
                },
                vide: format_sans(&dw, 14.0, DWRITE_FONT_WEIGHT_NORMAL, DWRITE_TEXT_ALIGNMENT_CENTER)?,
                apercu_titre: {
                    let t = format_texte(&dw, 20.0, DWRITE_FONT_WEIGHT_BOLD, g)?;
                    t.SetWordWrapping(DWRITE_WORD_WRAPPING_WRAP)?;
                    t.SetParagraphAlignment(DWRITE_PARAGRAPH_ALIGNMENT_NEAR)?;
                    t
                },
                apercu_grand: format_texte(&dw, 34.0, DWRITE_FONT_WEIGHT_BOLD, DWRITE_TEXT_ALIGNMENT_CENTER)?,
            };
            // Traits de Pixel et des signes des pastilles : bouts ronds
            let trait_rond = d2d.CreateStrokeStyle(
                &D2D1_STROKE_STYLE_PROPERTIES { startCap: D2D1_CAP_STYLE_ROUND, endCap: D2D1_CAP_STYLE_ROUND, dashCap: D2D1_CAP_STYLE_ROUND, lineJoin: D2D1_LINE_JOIN_ROUND, miterLimit: 1.0, dashStyle: D2D1_DASH_STYLE_SOLID, dashOffset: 0.0 },
                None,
            )?;
            Ok(Self {
                d2d,
                dwrite: dw,
                f,
                cible: None,
                images: RefCell::new(HashMap::new()),
                demandees: RefCell::new(HashSet::new()),
                a_demander: RefCell::new(Vec::new()),
                zones_conflit: RefCell::new(Vec::new()),
                zones_pastilles: RefCell::new(Disposition::default()),
                zone_reflet: Cell::new(None),
                zone_candidate: Cell::new(None),
                trait_rond,
                mouvements: RefCell::new(Mouvements::nouveau()),
                t_image: Cell::new(0),
                panneau: Cell::new(0),
                panneau_en_cours: Cell::new(0),
                cascade: Cell::new(None),
                fondu_volets: Cell::new((1.0, 1.0)),
                pilules: Cell::new([(0.0, 0.0); 2]),
                derniere_palette: RefCell::new(None),
                zones_palette: RefCell::new(ZonesPalette::default()),
                zones_historique: RefCell::new(ZonesHistorique::default()),
            })
        }
    }

    fn creer_cible(&self, hwnd: HWND, largeur: u32, hauteur: u32, dpi: f32) -> Result<Cible2D> {
        unsafe {
            let props = D2D1_RENDER_TARGET_PROPERTIES {
                pixelFormat: D2D1_PIXEL_FORMAT { format: DXGI_FORMAT_B8G8R8A8_UNORM, alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED },
                ..Default::default()
            };
            let props_hwnd = D2D1_HWND_RENDER_TARGET_PROPERTIES {
                hwnd,
                pixelSize: D2D_SIZE_U { width: largeur.max(1), height: hauteur.max(1) },
                presentOptions: D2D1_PRESENT_OPTIONS_NONE,
            };
            let rt = self.d2d.CreateHwndRenderTarget(&props, &props_hwnd)?;
            rt.SetDpi(dpi, dpi);
            rt.SetTextAntialiasMode(D2D1_TEXT_ANTIALIAS_MODE_GRAYSCALE);
            let pinceau = rt.CreateSolidColorBrush(&BLANC, None)?;

            // Motif d'un seul point, répété en mosaïque : la trame coûte un seul remplissage
            let motif = rt.CreateCompatibleRenderTarget(
                Some(&D2D_SIZE_F { width: PAS_TRAME, height: PAS_TRAME }),
                None,
                None,
                D2D1_COMPATIBLE_RENDER_TARGET_OPTIONS_NONE,
            )?;
            motif.BeginDraw();
            motif.Clear(Some(&rgba(0, 0, 0, 0.0)));
            let p = motif.CreateSolidColorBrush(&crate::theme::teinte_fixe(&POINT_TRAME), None)?;
            motif.FillEllipse(&ellipse(PAS_TRAME / 2.0, PAS_TRAME / 2.0, 1.0), &p);
            motif.EndDraw(None, None)?;
            let bitmap = motif.GetBitmap()?;
            let trame = rt.CreateBitmapBrush(
                &bitmap,
                Some(&D2D1_BITMAP_BRUSH_PROPERTIES {
                    extendModeX: D2D1_EXTEND_MODE_WRAP,
                    extendModeY: D2D1_EXTEND_MODE_WRAP,
                    interpolationMode: D2D1_BITMAP_INTERPOLATION_MODE_NEAREST_NEIGHBOR,
                }),
                None,
            )?;
            let mut icones = Vec::new();
            let mut icones_blanches = Vec::new();
            for i in TOUTES {
                icones.push(creer_icone(&rt, i, false)?);
                icones_blanches.push(creer_icone(&rt, i, true)?);
            }
            let couche = rt.CreateLayer(None)?;
            let encre = crate::theme::teinte_fixe(&BLANC);
            let paliers = [
                D2D1_GRADIENT_STOP { position: 0.0, color: D2D1_COLOR_F { a: 1.0, ..encre } },
                D2D1_GRADIENT_STOP { position: 1.0, color: D2D1_COLOR_F { a: 0.0, ..encre } },
            ];
            let degrade = rt.CreateGradientStopCollection(&paliers, D2D1_GAMMA_2_2, D2D1_EXTEND_MODE_CLAMP)?;
            let proprietes = D2D1_RADIAL_GRADIENT_BRUSH_PROPERTIES {
                center: D2D_POINT_2F { x: 0.0, y: 0.0 },
                gradientOriginOffset: D2D_POINT_2F { x: 0.0, y: 0.0 },
                radiusX: 1.0,
                radiusY: 1.0,
            };
            let reflet = rt.CreateRadialGradientBrush(&proprietes, None, &degrade)?;
            let png = icones::Pastille::Png;
            let rgb = |c: [u8; 3]| rgba(c[0], c[1], c[2], 1.0);
            let paliers_png = [
                D2D1_GRADIENT_STOP { position: 0.0, color: rgb(png.fond()) },
                D2D1_GRADIENT_STOP { position: 0.55, color: rgb(png.fond()) },
                D2D1_GRADIENT_STOP { position: 0.55, color: rgb(png.accent()) },
                D2D1_GRADIENT_STOP { position: 1.0, color: rgb(png.accent()) },
            ];
            let degrade_png_paliers = rt.CreateGradientStopCollection(&paliers_png, D2D1_GAMMA_2_2, D2D1_EXTEND_MODE_CLAMP)?;
            let degrade_png = rt.CreateLinearGradientBrush(
                &D2D1_LINEAR_GRADIENT_BRUSH_PROPERTIES { startPoint: D2D_POINT_2F { x: 0.0, y: 0.0 }, endPoint: D2D_POINT_2F { x: 1.0, y: 1.0 } },
                None,
                &degrade_png_paliers,
            )?;
            Ok(Cible2D {
                clair: crate::theme::clair(),
                rt,
                pinceau,
                trame,
                icones,
                icones_blanches,
                miniature: RefCell::new(None),
                couche,
                vignettes: RefCell::new(HashMap::new()),
                galerie: RefCell::new(None),
                reflet,
                degrade_png,
            })
        }
    }

    pub fn redimensionner(&mut self, largeur: u32, hauteur: u32) {
        if let Some(c) = &self.cible {
            unsafe {
                if c.rt.Resize(&D2D_SIZE_U { width: largeur.max(1), height: hauteur.max(1) }).is_err() {
                    self.cible = None;
                }
            }
        }
    }

    /// Nouvel écran ou nouvelle échelle : tout est recréé pour garder des icônes nettes
    pub fn changer_dpi(&mut self) {
        self.cible = None;
    }

    unsafe fn icone(&self, c: &Cible2D, icone: Icone, cx: f32, cy: f32, blanche: bool, opacite: f32) {
        let liste = if blanche { &c.icones_blanches } else { &c.icones };
        let r = rect(cx - COTE_ICONE / 2.0, cy - COTE_ICONE / 2.0, COTE_ICONE, COTE_ICONE);
        c.rt.DrawBitmap(&liste[icone.index()], Some(&r), opacite, D2D1_BITMAP_INTERPOLATION_MODE_LINEAR, None);
    }

    /// Miniature colorée d'un type de fichier (maquette `TH`) : carré arrondi de `cote` DIP centré en (cx, cy). Les couleurs sont
    /// celles de la maquette dans les deux thèmes (ce sont des couleurs de contenu, pas d'interface). `opacite` < 1 : fichier masqué
    unsafe fn pastille(&self, c: &Cible2D, p: icones::Pastille, cx: f32, cy: f32, cote: f32, rayon: f32, opacite: f32) {
        let rt = &c.rt;
        let r = rect(cx - cote / 2.0, cy - cote / 2.0, cote, cote);
        let forme = |r: D2D_RECT_F, rayon: f32| D2D1_ROUNDED_RECT { rect: r, radiusX: rayon, radiusY: rayon };
        let col = |rgb: [u8; 3]| rgba(rgb[0], rgb[1], rgb[2], opacite);
        let b = icones::PASTILLE_BORD;
        match p {
            icones::Pastille::Png => {
                c.degrade_png.SetOpacity(opacite);
                c.degrade_png.SetTransform(&Matrix3x2 { M11: cote, M12: 0.0, M21: 0.0, M22: cote, M31: r.left, M32: r.top });
                rt.FillRoundedRectangle(&forme(r, rayon), &c.degrade_png);
            }
            icones::Pastille::Exe => {
                // Bord intérieur : l'anneau est le fond clair, le centre reprend le fond sombre
                c.pinceau.SetColor(&col(p.accent()));
                rt.FillRoundedRectangle(&forme(r, rayon), &c.pinceau);
                c.pinceau.SetColor(&col(p.fond()));
                rt.FillRectangle(&rect(r.left + b, r.top + b, cote - 2.0 * b, cote - 2.0 * b), &c.pinceau);
            }
            icones::Pastille::Pdf | icones::Pastille::Zip => {
                c.pinceau.SetColor(&col(p.fond()));
                rt.FillRoundedRectangle(&forme(r, rayon), &c.pinceau);
                // Bandeau haut : la même forme arrondie, rognée à ses `b` premiers DIP
                rt.PushAxisAlignedClip(&rect(r.left, r.top, cote, b), D2D1_ANTIALIAS_MODE_ALIASED);
                c.pinceau.SetColor(&col(p.accent()));
                rt.FillRoundedRectangle(&forme(r, rayon), &c.pinceau);
                rt.PopAxisAlignedClip();
            }
        }
    }

    /// Largeur d'un texte dans un format donné (DIP)
    /// Largeur d'un nom dans la liste (DIP) : au-delà, la ligne est du « vide » où un lasso peut partir
    pub fn largeur_nom(&self, nom: &str) -> f32 {
        self.mesurer(nom, &self.f.liste)
    }

    /// Texte avec l'espacement de 1 DIP entre les lettres de la maquette (`.d`, `letter-spacing: 1px`). Rend `None` si DirectWrite
    /// ne peut pas construire la mise en page ; l'espacement seul est facultatif (sans lui, le texte reste lisible).
    fn layout_espace(&self, texte: &[u16], format: &IDWriteTextFormat, l: f32, h: f32) -> Option<IDWriteTextLayout> {
        unsafe {
            let layout = self.dwrite.CreateTextLayout(texte, format, l, h).ok()?;
            if let Ok(l1) = layout.cast::<IDWriteTextLayout1>() {
                let _ = l1.SetCharacterSpacing(0.0, 1.0, 0.0, DWRITE_TEXT_RANGE { startPosition: 0, length: texte.len() as u32 });
            }
            Some(layout)
        }
    }

    /// Largeur d'un texte Doto avec l'espacement de 1 DIP (DIP)
    fn mesurer_espace(&self, texte: &str, format: &IDWriteTextFormat) -> f32 {
        let w: Vec<u16> = texte.encode_utf16().collect();
        let Some(layout) = self.layout_espace(&w, format, 10_000.0, 100.0) else { return 0.0 };
        let mut m = DWRITE_TEXT_METRICS::default();
        // SAFETY: `m` vit pendant l'appel
        if unsafe { layout.GetMetrics(&mut m) }.is_ok() { m.widthIncludingTrailingWhitespace } else { 0.0 }
    }

    /// Comme `texte`, avec l'espacement de 1 DIP entre les lettres. Trop long pour `zone` : coupé par « … » comme les autres formats
    unsafe fn texte_espace(&self, c: &Cible2D, format: &IDWriteTextFormat, texte: &str, zone: D2D_RECT_F) {
        if zone.right - zone.left < 4.0 {
            return;
        }
        let w: Vec<u16> = texte.encode_utf16().collect();
        match self.layout_espace(&w, format, zone.right - zone.left, zone.bottom - zone.top) {
            Some(layout) => c.rt.DrawTextLayout(D2D_POINT_2F { x: zone.left, y: zone.top }, &layout, &c.pinceau, D2D1_DRAW_TEXT_OPTIONS_CLIP),
            None => self.texte(c, format, texte, zone),
        }
    }

    /// Largeur du nom affiché d'une entrée (extension masquée comprise)
    fn mesurer_nom(&self, e: &Entree, format: &IDWriteTextFormat) -> f32 {
        self.mesurer(&String::from_utf16_lossy(nom_vu(e)), format)
    }

    /// Carrés d'étiquette (maquette `.sq i` : 5 × 5 DIP pleins, 2 DIP d'écart, couleur `--i2`) : `n` carrés à partir de `x`, centrés sur `cy`
    unsafe fn carres_etiquette(&self, c: &Cible2D, x: f32, cy: f32, n: u8) {
        self.couleur(c, &BLANC_DOUX);
        for k in 0..n {
            c.rt.FillRectangle(&rect(x + k as f32 * (CARRE_ETIQUETTE + ECART_ETIQUETTE), (cy - CARRE_ETIQUETTE / 2.0).round(), CARRE_ETIQUETTE, CARRE_ETIQUETTE), &c.pinceau);
        }
    }

    fn mesurer(&self, texte: &str, format: &IDWriteTextFormat) -> f32 {
        let w: Vec<u16> = texte.encode_utf16().collect();
        unsafe {
            let Ok(layout) = self.dwrite.CreateTextLayout(&w, format, 10_000.0, 100.0) else { return 0.0 };
            let mut m = DWRITE_TEXT_METRICS::default();
            if layout.GetMetrics(&mut m).is_ok() {
                m.widthIncludingTrailingWhitespace
            } else {
                0.0
            }
        }
    }

    /// Dessine toute la fenêtre. `largeur`/`hauteur` en pixels physiques.
    pub fn dessiner(&mut self, hwnd: HWND, largeur: u32, hauteur: u32, dpi: f32, app: &mut App, mut autre: Option<&mut App>, vue: &Vue) -> Result<()> {
        if self.cible.as_ref().is_some_and(|c| c.clair != crate::theme::clair()) {
            self.cible = None;
        }
        if self.cible.is_none() {
            self.cible = Some(self.creer_cible(hwnd, largeur, hauteur, dpi)?);
            // Nouvelles images : les vignettes déjà converties sont à redemander
            self.demandees.borrow_mut().clear();
        }
        let l = largeur as f32 * 96.0 / dpi;
        let h = hauteur as f32 * 96.0 / dpi;
        // Le responsive est calculé à chaque image : 240 px de volet et pas d'aperçu à 1100 px ou moins.
        memoriser_largeur(l);
        // Lot 12D : le volet et l'aperçu glissent (`.body`, 500 ms au ressort) ; les largeurs affichées sont posées avant tout calcul
        // de disposition, les zones de clic et les colonnes les relisent
        {
            let v = self.mouvements.borrow_mut().volets(largeur_volet_cible(), largeur_apercu_cible(l, vue.apercu_visible), mouvement::maintenant_ms(), crate::reglages::animations_actives());
            crate::theme::poser_largeurs_affichees(v.glisse.then_some(v.volet), v.glisse.then_some(v.apercu));
            self.fondu_volets.set((v.fondu_volet, v.fondu_apercu));
        }
        let ll = largeur_liste(l, vue.apercu_visible);
        let lp = largeur_panneau(ll, autre.is_some());
        for (rang, a) in std::iter::once(&mut *app).chain(autre.as_deref_mut()).enumerate() {
            a.hauteur_liste = (h - LISTE_Y - STATUT_H - if a.avec_chronologie() { CHRONO_H } else { 0.0 }).max(0.0);
            a.zones.jours = if a.avec_chronologie() { placer_jours(a, lp, h) } else { Vec::new() };
            // Lot 14 : le champ de filtre s'ouvre (largeur 450 ms au ressort, opacité 200 ms) ; sa part est posée avant la mesure du chemin
            let pilule = self.mouvements.borrow_mut().pilule(rang, a.champ_filtre_visible(), mouvement::maintenant_ms(), crate::reglages::animations_actives());
            let mut pilules = self.pilules.get();
            pilules[rang.min(1)] = pilule;
            self.pilules.set(pilules);
            // Fil d'Ariane mesuré avant le dessin (les zones servent aussi aux clics) ; filtre et branche prennent la droite
            let place = self.place_droite(a, lp, rang);
            a.zones.puces = match self.zone_filtre(a, lp, rang) {
                Some((z, m, _)) if a.champ_filtre_visible() => self.placer_puces(z, &m).into_iter().filter(|p| p.right <= z.right - 4.0).collect(),
                _ => Vec::new(),
            };
            a.zones.segments = self.placer_segments(a, lp - place);
            let wn = self.largeur_note(a, lp);
            a.zones.note = (wn > 0.0 && a.saisie_chemin.is_none()).then(|| rect(lp - MARGE_LISTE - TERMINAL_RESERVE - (place - TERMINAL_RESERVE - wn - 8.0) - wn, NAV_Y + 9.0, wn, NAV_H - 18.0));
            a.zones.barre = if a.emplacement == Emplacement::Reglages {
                Vec::new()
            } else {
                crate::barre::placer(largeur_volet() + MARGE_LISTE - 6.0, lp - MARGE_LISTE + 6.0, &|t| self.mesurer(t, &self.f.petit))
            };
            a.zones.densite = crate::barre::zones_densite(&a.zones.barre);
            a.zones.corbeille = None;
            if a.emplacement == Emplacement::CePC {
                crate::index::preparer_carte(hwnd);
                let (bx, by) = bloc_carte(lp, haut_carte(a), disques_carte(a).count());
                a.zones.corbeille = crate::corbeille::etat().map(|_| rect(bx - 6.0, by - 4.0, CARTE_BLOC_L - 20.0, CARTE_BLOC_H - 30.0));
            }
            let pas = a.cote_grille + GRILLE_ECART;
            a.par_rangee = (((lp - MARGE_LISTE_DROITE - grille_x0() + GRILLE_ECART) / pas).floor() as usize).max(1);
        }

        let c = self.cible.as_ref().unwrap();
        let res = unsafe {
            c.rt.BeginDraw();
            self.dessiner_tout(c, app, autre.as_deref(), l, h, vue);
            c.rt.EndDraw(None, None)
        };
        if let Err(e) = res {
            // Cible perdue (changement de carte graphique, veille…) : on la recrée au prochain dessin
            if e.code() == D2DERR_RECREATE_TARGET {
                self.cible = None;
                return Ok(());
            }
            return Err(e);
        }
        Ok(())
    }

    /// Place les segments du chemin (maquette `.path`) : chaque segment est un bouton `.crumb` de marge 2 / 8 en Doto 22 gras, séparés
    /// par « › » avec 8 DIP de chaque côté. Si tout ne tient pas, les premiers sont masqués ; un dernier segment trop long est tronqué
    /// (« … ») à la place disponible, pour ne jamais déborder sur les boutons de droite.
    fn placer_segments(&self, app: &App, l: f32) -> Vec<(D2D_RECT_F, Emplacement, String)> {
        let segs = app.emplacement.segments();
        let sep = self.mesurer_espace("›", &self.f.chemin) + 2.0 * CRUMB_ECART;
        let mut largeurs: Vec<f32> = segs.iter().map(|(t, _)| self.mesurer_espace(t, &self.f.chemin) + 2.0 * CRUMB_MARGE).collect();
        // Groupe ouvert : le dernier segment porte un point de la couleur du groupe (maquette `.gc`)
        if let (Emplacement::Groupe(_), Some(dernier)) = (&app.emplacement, largeurs.last_mut()) {
            *dernier += GROUPE_POINT;
        }
        let dispo = l - chemin_x() - 20.0;
        let mut debut = 0;
        let total = |lg: &[f32], d: usize| lg[d..].iter().sum::<f32>() + sep * (lg.len() - d - 1) as f32;
        while debut + 1 < segs.len() && total(&largeurs, debut) > dispo {
            debut += 1;
        }
        if let Some(&dernier) = largeurs.last() {
            let reste = (dispo - (total(&largeurs, debut) - dernier)).max(60.0);
            let n = largeurs.len();
            largeurs[n - 1] = dernier.min(reste);
        }
        let mut x = chemin_x() - CRUMB_MARGE;
        let mut v = Vec::new();
        for (i, (texte, emp)) in segs.into_iter().enumerate().skip(debut) {
            v.push((rect(x, NAV_Y, largeurs[i], NAV_H), emp, texte));
            x += largeurs[i] + sep;
        }
        v
    }

    /// Texte (Doto `petit`) qui tient dans `largeur` : trop long, il perd ses derniers mots et finit par « … ». Jamais un mot coupé
    /// au milieu, sauf un premier mot qui ne tient pas seul (le formatage DirectWrite le coupe alors avec son propre « … »).
    fn tronquer_aux_mots(&self, texte: &str, largeur: f32) -> String {
        if self.mesurer(texte, &self.f.petit) <= largeur {
            return texte.to_string();
        }
        let mut mots: Vec<&str> = texte.split(' ').collect();
        while mots.len() > 1 {
            mots.pop();
            let essai = format!("{}…", mots.join(" ").trim_end_matches([',', ';', ':', ' ']));
            if self.mesurer(&essai, &self.f.petit) <= largeur {
                return essai;
            }
        }
        texte.to_string()
    }

    unsafe fn texte(&self, c: &Cible2D, format: &IDWriteTextFormat, texte: &str, zone: D2D_RECT_F) {
        // Colonne masquée (fenêtre étroite) : rien à dessiner
        if zone.right - zone.left < 4.0 {
            return;
        }
        let utf16: Vec<u16> = texte.encode_utf16().collect();
        c.rt.DrawText(&utf16, format, &zone, &c.pinceau, D2D1_DRAW_TEXT_OPTIONS_CLIP, DWRITE_MEASURING_MODE_NATURAL);
    }

    /// Une touche dessinée à `x` (bord gauche), centrée sur `cy` ; rend le bord droit du cadre.
    unsafe fn touche_unique(&self, c: &Cible2D, texte: &str, x: f32, cy: f32, actif: bool) -> f32 {
        let h = touche_hauteur();
        let w = largeur_touche(texte);
        // Demi-pixel : le trait de 1 px tombe entre deux pixels, sans flou
        let cadre = D2D1_ROUNDED_RECT { rect: rect(x + 0.5, cy - h / 2.0 + 0.5, w - 1.0, h - 1.0), radiusX: TOUCHE_RAYON, radiusY: TOUCHE_RAYON };
        self.couleur(c, &BLANC_TRES_FAIBLE);
        c.rt.DrawRoundedRectangle(&cadre, &c.pinceau, 1.0, None);
        self.couleur(c, if actif { &BLANC_FAIBLE } else { &BLANC_TRES_FAIBLE });
        self.texte(c, &self.f.touche, texte, rect(x, cy - h / 2.0, w, h));
        x + w
    }

    /// Dessine un raccourci en touches, la première à `x` (bord gauche), centrées sur `cy` ; rend le bord droit du bloc.
    /// `actif` faux : texte de la touche éteint (ligne grisée).
    unsafe fn touches(&self, c: &Cible2D, raccourci: &str, x: f32, cy: f32, actif: bool) -> f32 {
        let mut x = x;
        for (k, t) in decouper_touches(raccourci).iter().enumerate() {
            if k > 0 {
                x += TOUCHE_ESPACE;
            }
            x = self.touche_unique(c, t, x, cy, actif);
        }
        x
    }

    /// Raccourci d'une ligne (menu, palette) : touches alignées à droite, en `x_droit`. Pas assez de place entre `x_min` et lui :
    /// rien n'est dessiné, le libellé de la commande garde la priorité.
    unsafe fn touches_droite(&self, c: &Cible2D, raccourci: &str, x_droit: f32, cy: f32, x_min: f32, actif: bool) {
        let w = largeur_touches(raccourci);
        if w > 0.0 && x_droit - w >= x_min {
            self.touches(c, raccourci, x_droit - w, cy, actif);
        }
    }

    /// Largeur d'un texte d'aide dont les raccourcis sont dessinés en touches
    fn largeur_aide(&self, morceaux: &[Morceau]) -> f32 {
        morceaux
            .iter()
            .map(|m| match m {
                Morceau::Texte(t) => self.mesurer(t, &self.f.petit),
                Morceau::Touches(k) => largeur_touches(k),
            })
            .sum()
    }

    /// Trois gestes du pied de la maquette v6, en Segoe 11 : glyphe puis libellé.
    const GROUPES_PIED: [(&'static str, &'static str); 3] = [("↵", "OUVRIR"), ("ESPACE", "APERCU"), ("CTRL+,", "RÉGLAGES")];
    const PIED_ECART: f32 = 14.0;
    const PIED_TOUCHE_LABEL: f32 = 6.0;

    /// État d'opération à droite du pied (pause, file) ; rien : les trois gestes restent affichés
    fn texte_operation(app: &App) -> Option<String> {
        let etat = app.operation.as_ref().map(|op| {
            let pause = if crate::operations::attend_reseau(op.numero) {
                "RESEAU COUPE · REPRISE DES SON RETOUR"
            } else if app.en_pause() {
                "EN PAUSE"
            } else if crate::operations::LIMITEE.load(std::sync::atomic::Ordering::Relaxed) {
                "VITESSE LIMITEE"
            } else {
                ""
            };
            let file = if app.en_file() > 0 { format!("+{} EN FILE", app.en_file()) } else { String::new() };
            [pause.to_string(), file].into_iter().filter(|t| !t.is_empty()).collect::<Vec<_>>().join(" · ")
        });
        etat.filter(|t| !t.is_empty())
    }

    fn largeur_pied(&self) -> f32 {
        let groupes: f32 = Self::GROUPES_PIED
            .iter()
            .map(|(touche, libelle)| largeur_touche(touche) + Self::PIED_TOUCHE_LABEL + self.mesurer(libelle, &self.f.petit))
            .sum();
        groupes + Self::PIED_ECART * (Self::GROUPES_PIED.len() - 1) as f32
    }

    /// Pied de la maquette : les trois gestes dessinés en touches, pas la phrase « ENTREE OUVRIR · … ».
    /// Chaque geste est une seule touche (`CTRL+,` ne se coupe pas en deux cadres).
    unsafe fn pied(&self, c: &Cible2D, droite: f32, ys: f32, couleur: &D2D1_COLOR_F) {
        let mut x = droite - self.largeur_pied();
        let cy = ys + STATUT_H / 2.0;
        for (touche, libelle) in Self::GROUPES_PIED {
            x = self.touche_unique(c, touche, x, cy, true) + Self::PIED_TOUCHE_LABEL;
            let w = self.mesurer(libelle, &self.f.petit);
            self.couleur(c, couleur);
            self.texte(c, &self.f.petit, libelle, rect(x, ys, w + 2.0, STATUT_H));
            x += w + Self::PIED_ECART;
        }
    }

    /// Texte d'aide (Doto) dont les raccourcis (`CTRL+F`, `ENTREE`, `F2`) sont dessinés en touches, dans `zone`. Trop long : le
    /// texte est coupé par « … » puis la suite est omise, jamais une touche à moitié.
    unsafe fn aide(&self, c: &Cible2D, texte: &str, zone: D2D_RECT_F, pose: Pose, couleur: &D2D1_COLOR_F) {
        let morceaux = segmenter_aide(texte);
        self.couleur(c, couleur);
        if morceaux.iter().all(|m| matches!(m, Morceau::Texte(_))) {
            let format = match pose {
                Pose::Gauche => &self.f.petit,
                Pose::Droite => &self.f.petit_droite,
                Pose::Centre => &self.f.petit_centre,
            };
            return self.texte(c, format, texte, zone);
        }
        let place = zone.right - zone.left;
        let total = self.largeur_aide(&morceaux);
        let mut x = match pose {
            Pose::Droite if total <= place => zone.right - total,
            Pose::Centre if total <= place => zone.left + (place - total) / 2.0,
            _ => zone.left,
        };
        let cy = (zone.top + zone.bottom) / 2.0;
        for m in &morceaux {
            match m {
                Morceau::Texte(t) => {
                    let w = self.mesurer(t, &self.f.petit);
                    self.couleur(c, couleur);
                    self.texte(c, &self.f.petit, t, rect(x, zone.top, (w + 2.0).min(zone.right - x), zone.bottom - zone.top));
                    x += w;
                }
                Morceau::Touches(k) => {
                    if x + largeur_touches(k) > zone.right + 0.5 {
                        break;
                    }
                    x = self.touches(c, k, x, cy, true);
                }
            }
            if x >= zone.right {
                break;
            }
        }
    }

    /// Texte de l'aperçu avec ses couleurs (code coloré, Markdown mis en forme)
    unsafe fn texte_teinte(&self, c: &Cible2D, texte: &str, teintes: &[(u32, u32, Teinte)], zone: D2D_RECT_F) {
        let u: Vec<u16> = texte.encode_utf16().collect();
        let Ok(mise) = self.dwrite.CreateTextLayout(&u, &self.f.code, zone.right - zone.left, zone.bottom - zone.top) else { return };
        let teinte = |t: Teinte| match t {
            Teinte::MotCle | Teinte::Lien => ACCENT,
            Teinte::Chaine | Teinte::Titre | Teinte::Gras => BLANC,
            Teinte::Commentaire | Teinte::Discret => BLANC_DOUX,
            Teinte::Nombre => TEINTE_NOMBRE,
        };
        let mut pinceaux: Vec<(Teinte, ID2D1SolidColorBrush)> = Vec::new();
        for &(debut, longueur, t) in teintes {
            let r = DWRITE_TEXT_RANGE { startPosition: debut, length: longueur };
            if !pinceaux.iter().any(|(x, _)| *x == t) {
                if let Ok(p) = c.rt.CreateSolidColorBrush(&crate::theme::contraste(&teinte(t)), None) {
                    pinceaux.push((t, p));
                }
            }
            if let Some((_, p)) = pinceaux.iter().find(|(x, _)| *x == t) {
                let _ = mise.SetDrawingEffect(p, r);
            }
            if matches!(t, Teinte::Titre | Teinte::Gras) {
                let _ = mise.SetFontWeight(DWRITE_FONT_WEIGHT_BOLD, r);
            }
            if t == Teinte::Titre {
                let _ = mise.SetFontSize(14.0, r);
            }
        }
        c.rt.DrawTextLayout(D2D_POINT_2F { x: zone.left, y: zone.top }, &mise, &c.pinceau, D2D1_DRAW_TEXT_OPTIONS_CLIP);
    }

    unsafe fn texte_w(&self, c: &Cible2D, format: &IDWriteTextFormat, texte: &[u16], zone: D2D_RECT_F) {
        c.rt.DrawText(texte, format, &zone, &c.pinceau, D2D1_DRAW_TEXT_OPTIONS_CLIP, DWRITE_MEASURING_MODE_NATURAL);
    }

    /// Triangle plein pointe en haut, centré en (cx, cy) : le `▲` de `.st.wr`, sans dépendre d'une police
    unsafe fn triangle_plein(&self, c: &Cible2D, cx: f32, cy: f32, demi: f32) {
        let Ok(g) = self.d2d.CreatePathGeometry() else { return };
        let Ok(s) = g.Open() else { return };
        s.BeginFigure(D2D_POINT_2F { x: cx, y: cy - demi }, D2D1_FIGURE_BEGIN_FILLED);
        s.AddLine(D2D_POINT_2F { x: cx + demi, y: cy + demi });
        s.AddLine(D2D_POINT_2F { x: cx - demi, y: cy + demi });
        s.EndFigure(D2D1_FIGURE_END_CLOSED);
        if s.Close().is_ok() {
            c.rt.FillGeometry(&g, &c.pinceau, None);
        }
    }

    /// Lot 14 (`.hl`) : souligne en accent (2 DIP, à 3 DIP de la ligne de base) les lettres de `nom` que `filtre` fait correspondre,
    /// accents et casse ignorés. `nom`, `format` et `zone` sont ceux du texte qui vient d'être dessiné : les positions viennent de
    /// la mise en page DirectWrite elle-même (alignement et approche des lettres compris).
    unsafe fn souligner(&self, c: &Cible2D, format: &IDWriteTextFormat, nom: &[u16], filtre: &str, zone: D2D_RECT_F) {
        if filtre.trim().is_empty() || zone.right - zone.left < 4.0 {
            return;
        }
        let texte = String::from_utf16_lossy(nom);
        let plages = crate::dossier::plages_correspondance(&texte, filtre);
        self.souligner_plages(c, format, nom, &plages, zone);
    }

    /// Souligne en accent les `plages` (début, fin exclue, en lettres) de `nom`, dessiné avec `format` dans `zone` (filtre des dossiers
    /// et requête de la palette)
    unsafe fn souligner_plages(&self, c: &Cible2D, format: &IDWriteTextFormat, nom: &[u16], plages: &[(usize, usize)], zone: D2D_RECT_F) {
        if plages.is_empty() || zone.right - zone.left < 4.0 {
            return;
        }
        let texte = String::from_utf16_lossy(nom);
        let Ok(layout) = self.dwrite.CreateTextLayout(nom, format, zone.right - zone.left, zone.bottom - zone.top) else { return };
        // Position UTF-16 de chaque lettre : les plages comptent des lettres
        let mut debuts = Vec::with_capacity(texte.chars().count() + 1);
        let mut u = 0usize;
        for ch in texte.chars() {
            debuts.push(u);
            u += ch.len_utf16();
        }
        debuts.push(u);
        let mut lignes = [DWRITE_LINE_METRICS::default()];
        let mut n = 0u32;
        let _ = layout.GetLineMetrics(Some(&mut lignes), &mut n);
        let base = lignes[0].baseline;
        self.couleur(c, &ACCENT);
        c.rt.PushAxisAlignedClip(&zone, D2D1_ANTIALIAS_MODE_ALIASED);
        for &(a, b) in plages {
            let (Some(&pa), Some(&pb)) = (debuts.get(a), debuts.get(b.saturating_sub(1))) else { continue };
            let (mut xa, mut ya, mut xb, mut yb) = (0.0f32, 0.0f32, 0.0f32, 0.0f32);
            let mut m = DWRITE_HIT_TEST_METRICS::default();
            let _ = layout.HitTestTextPosition(pa as u32, false, &mut xa, &mut ya, &mut m);
            let _ = layout.HitTestTextPosition(pb as u32, true, &mut xb, &mut yb, &mut m);
            if xb > xa {
                c.rt.FillRectangle(&rect(zone.left + xa, zone.top + ya + base + 3.0, xb - xa, 2.0), &c.pinceau);
            }
        }
        c.rt.PopAxisAlignedClip();
    }

    /// Dessine un motif de points centré en (cx, cy), avec un pas donné
    /// Ligne de l'arbre du volet : chevron en points, icône, nom ; le curseur du clavier est cerclé de rouge
    unsafe fn ligne_arbre(&self, c: &Cible2D, app: &App, l: &crate::arbre::Ligne, r: D2D_RECT_F, survole: bool, fondu: f32, depose: bool) {
        let courant = app.dossier().map(crate::arbre::cle).unwrap_or_default();
        let cl = crate::arbre::cle(&l.chemin);
        let actif = cl == courant;
        let curseur = app.arbre_focus && app.arbre_curseur.as_deref().unwrap_or(&courant) == cl;
        let forme = D2D1_ROUNDED_RECT { rect: r, radiusX: 7.0, radiusY: 7.0 };
        if actif || fondu > 0.0 {
            if actif { self.couleur(c, &SURVOL_FORT) } else { self.couleur_k(c, &SURVOL, fondu) }
            c.rt.FillRoundedRectangle(&forme, &c.pinceau);
        }
        if curseur {
            self.anneau(c, r, 7.0);
        }
        if depose {
            self.couleur(c, &ACCENT);
            c.rt.DrawRoundedRectangle(&forme, &c.pinceau, 1.5, None);
        }
        let x = r.left + l.profondeur as f32 * crate::arbre::RETRAIT;
        let y = (r.top + r.bottom) / 2.0;
        if l.enfants != Some(false) {
            self.couleur(c, &BLANC_DOUX);
            let motif: &[(i32, i32)] = if l.ouvert { &[(-1, -1), (0, 0), (1, -1)] } else { &[(-1, -1), (0, 0), (-1, 1)] };
            self.points(c, x + 9.0, y, 3.2, 1.2, motif);
        }
        self.icone(c, Icone::Dossier, x + 27.0, y, false, if actif || survole { 1.0 } else { 0.7 });
        self.couleur(c, if actif || survole { &BLANC } else { &BLANC_DOUX });
        self.texte(c, &self.f.volet, &l.nom, rect(x + 42.0, r.top, (r.right - x - 56.0).max(10.0), r.bottom - r.top));
        if actif {
            self.couleur(c, &ACCENT);
            c.rt.FillEllipse(&ellipse(r.right - 10.0, y, 2.5), &c.pinceau);
        }
    }

    unsafe fn points(&self, c: &Cible2D, cx: f32, cy: f32, pas: f32, r: f32, motif: &[(i32, i32)]) {
        for &(i, j) in motif {
            c.rt.FillEllipse(&ellipse(cx + i as f32 * pas, cy + j as f32 * pas, r), &c.pinceau);
        }
    }

    unsafe fn couleur(&self, c: &Cible2D, col: &D2D1_COLOR_F) {
        // Point de passage unique : thème clair, contraste renforcé, Contraste élevé de Windows (identité sinon)
        c.pinceau.SetColor(&crate::theme::contraste(col));
    }

    /// Intensité (0 à 1) du fond de survol de `cible` à cette image : fondu de 120 ms (lignes) ou 160 ms (chrome)
    fn survol_k(&self, cible: Cible) -> f32 {
        self.mouvements.borrow().survols.fond(cible, self.t_image.get())
    }

    /// Comme `couleur`, avec l'opacité du jeton multipliée par `k` (fondu d'un fond de survol)
    unsafe fn couleur_k(&self, c: &Cible2D, col: &D2D1_COLOR_F, k: f32) {
        self.couleur(c, &D2D1_COLOR_F { a: col.a * k.clamp(0.0, 1.0), ..*col });
    }

    /// Dessine avec un décalage (translation, échelle autour de `centre`, opacité) : calque d'opacité seulement si elle varie
    unsafe fn avec_decalage(&self, c: &Cible2D, d: &mouvement::Decalage, centre: (f32, f32), bornes: D2D_RECT_F, dessiner: impl FnOnce()) {
        let couche = if d.opacite < 0.999 { c.rt.CreateLayer(None).ok() } else { None };
        let garde = self.flip_pousser(c, d, centre, bornes, &couche);
        dessiner();
        self.flip_retirer(c, garde);
    }

    /// Décalage d'un élément qui apparaît (`quoi` : `mouvement::APPARU_*`) : translation verticale de `dy0` DIP et échelle de `echelle0`
    /// au départ, opacité 0 ; `AUCUN` au repos, absent, ou en mouvement réduit.
    fn decalage_apparition(&self, quoi: usize, present: bool, duree: u64, dy0: f32, echelle0: f32) -> mouvement::Decalage {
        let p = self.mouvements.borrow_mut().apparition(quoi, present, duree, self.t_image.get(), crate::reglages::animations_actives());
        p.map_or(mouvement::Decalage::AUCUN, |p| mouvement::Decalage { dx: 0.0, dy: dy0 * (1.0 - p), echelle: echelle0 + (1.0 - echelle0) * p, opacite: p.clamp(0.0, 1.0) })
    }

    /// Comme `couleur`, pour un jeton dont la valeur claire ne se déduit pas de la valeur sombre.
    unsafe fn couleur_ou(&self, c: &Cible2D, col: &D2D1_COLOR_F, clair: &D2D1_COLOR_F) {
        c.pinceau.SetColor(&crate::theme::contraste_ou(col, clair));
    }

    /// Anneau de focus clavier : 2 px, décalé de 2 px (maquette `:focus-visible`).
    unsafe fn anneau(&self, c: &Cible2D, r: D2D_RECT_F, rayon: f32) {
        let dehors = D2D_RECT_F { left: r.left - 2.0, top: r.top - 2.0, right: r.right + 2.0, bottom: r.bottom + 2.0 };
        c.pinceau.SetColor(&crate::theme::contraste_ou(&ANNEAU, &ANNEAU_CLAIR));
        c.rt.DrawRoundedRectangle(&D2D1_ROUNDED_RECT { rect: dehors, radiusX: rayon + 2.0, radiusY: rayon + 2.0 }, &c.pinceau, 2.0, None);
    }

    /// Anneau de focus d'une capsule de liste ou de grille, tracé à l'intérieur de la capsule pour que le clip de la liste ne
    /// le rogne pas en première ni en dernière ligne
    unsafe fn anneau_dans(&self, c: &Cible2D, capsule: D2D_RECT_F, rayon: f32) {
        let t = crate::theme::trait_anneau_dans([capsule.left, capsule.top, capsule.right - capsule.left, capsule.bottom - capsule.top]);
        c.pinceau.SetColor(&crate::theme::contraste_ou(&ANNEAU, &ANNEAU_CLAIR));
        c.rt.DrawRoundedRectangle(&D2D1_ROUNDED_RECT { rect: rect(t[0], t[1], t[2], t[3]), radiusX: rayon, radiusY: rayon }, &c.pinceau, crate::theme::ANNEAU_EPAISSEUR, None);
    }

    /// Champ de renommage : fond noir, contour blanc, sélection rouge, curseur
    unsafe fn champ(&self, c: &Cible2D, ed: &crate::edition::Edition, zone: D2D_RECT_F) {
        let rt = &c.rt;
        let cadre = D2D_RECT_F { left: zone.left - 6.0, top: zone.top + 3.0, right: zone.right, bottom: zone.bottom - 3.0 };
        let arrondi = D2D1_ROUNDED_RECT { rect: cadre, radiusX: 5.0, radiusY: 5.0 };
        self.couleur(c, &CHAMP_FOND);
        rt.FillRoundedRectangle(&arrondi, &c.pinceau);
        self.couleur(c, &BLANC);
        rt.DrawRoundedRectangle(&arrondi, &c.pinceau, 1.0, None);

        let Ok(layout) = self.dwrite.CreateTextLayout(&ed.texte, &self.f.liste, 10_000.0, zone.bottom - zone.top) else { return };
        let position = |p: usize| {
            let (mut x, mut y) = (0.0f32, 0.0f32);
            let mut m = DWRITE_HIT_TEST_METRICS::default();
            let _ = layout.HitTestTextPosition(p as u32, false, &mut x, &mut y, &mut m);
            x
        };
        // Décalage horizontal pour que le curseur reste visible dans un nom très long
        let largeur = zone.right - zone.left - 4.0;
        let xc = position(ed.curseur);
        let decalage = (xc - largeur).max(0.0);
        rt.PushAxisAlignedClip(&cadre, D2D1_ANTIALIAS_MODE_ALIASED);
        let (a, b) = ed.selection();
        if a != b {
            self.couleur(c, &ACCENT_VISE);
            let (xa, xb) = (position(a), position(b));
            rt.FillRectangle(&rect(zone.left + xa - decalage, zone.top + 6.0, xb - xa, zone.bottom - zone.top - 12.0), &c.pinceau);
        }
        self.couleur(c, &BLANC);
        rt.DrawTextLayout(D2D_POINT_2F { x: zone.left - decalage, y: zone.top }, &layout, &c.pinceau, D2D1_DRAW_TEXT_OPTIONS_NONE);
        self.couleur(c, &ACCENT);
        rt.FillRectangle(&rect(zone.left + xc - decalage, zone.top + 6.0, 2.0, zone.bottom - zone.top - 12.0), &c.pinceau);
        rt.PopAxisAlignedClip();
    }

    /// Mesure du champ de filtre (`.spill`) à pleine ouverture : largeur totale, largeur du texte, et si les puces tiennent.
    /// Loupe 30, texte, puces `ext:` `taille>` `modif:` (`.chip` : texte Segoe 12, 8 de marge de chaque côté, bord 1), touche Échap.
    /// Jamais plus de 60 % du panneau ; trop étroit, les puces disparaissent avant que le texte soit rogné.
    fn mesure_filtre(&self, app: &App, l: f32) -> MesureFiltre {
        let f = &self.f;
        let puces: f32 = PUCES_FILTRE.iter().map(|p| self.mesurer(p, &f.petit) + 2.0 * PUCE_MARGE + 2.0 + PUCE_ECART).sum();
        let suite = puces + largeur_touche("Échap") + FILTRE_DROITE;
        let souhaite = (self.mesurer(&app.filtre, &f.liste) + 24.0).clamp(96.0, 220.0);
        let cap = (l * 0.6).max(140.0);
        let besoin = FILTRE_GAUCHE + souhaite + suite;
        if besoin <= cap {
            MesureFiltre { plein: besoin, texte: souhaite, puces: true }
        } else if cap - FILTRE_GAUCHE - suite >= 70.0 {
            MesureFiltre { plein: cap, texte: cap - FILTRE_GAUCHE - suite, puces: true }
        } else {
            let texte = (cap - FILTRE_GAUCHE - FILTRE_DROITE).clamp(40.0, souhaite);
            MesureFiltre { plein: FILTRE_GAUCHE + texte + FILTRE_DROITE, texte, puces: false }
        }
    }

    /// Zone du champ de filtre du panneau `rang` à cette image (largeur animée, `.spill`) et sa mesure ; `None` fermé
    fn zone_filtre(&self, app: &App, l: f32, rang: usize) -> Option<(D2D_RECT_F, MesureFiltre, f32)> {
        let (k, opacite) = self.pilules.get()[rang.min(1)];
        if k < 0.004 || opacite < 0.004 {
            return None;
        }
        let m = self.mesure_filtre(app, l);
        let w = m.plein * k;
        let droite = l - MARGE_LISTE - TERMINAL_RESERVE;
        Some((rect(droite - w, NAV_Y + 5.0, w, NAV_H - 10.0), m, opacite))
    }

    /// Les trois puces dans la zone du champ (rangées de la gauche vers la droite, après le texte)
    fn placer_puces(&self, z: D2D_RECT_F, m: &MesureFiltre) -> Vec<D2D_RECT_F> {
        if !m.puces {
            return Vec::new();
        }
        let mut x = z.left + FILTRE_GAUCHE + m.texte + PUCE_ECART;
        let cy = (z.top + z.bottom) / 2.0;
        PUCES_FILTRE
            .iter()
            .map(|p| {
                let w = self.mesurer(p, &self.f.petit) + 2.0 * PUCE_MARGE + 2.0;
                let r = rect(x, cy - PUCE_H / 2.0, w, PUCE_H);
                x += w + PUCE_ECART;
                r
            })
            .collect()
    }

    /// Largeur affichée du champ de filtre du panneau `rang` (0 fermé), à droite du chemin
    fn largeur_filtre(&self, app: &App, l: f32, rang: usize) -> f32 {
        self.zone_filtre(app, l, rang).map_or(0.0, |(z, _, _)| z.right - z.left)
    }

    /// Largeur de la pastille de branche Git (0 hors d'un dépôt)
    fn largeur_branche(&self, app: &App) -> f32 {
        match app.git.as_ref().map(|g| g.branche.as_str()).filter(|b| !b.is_empty()) {
            Some(b) => (self.mesurer(&b.to_uppercase(), &self.f.petit) + 40.0).min(220.0),
            None => 0.0,
        }
    }

    /// Largeur de la pastille de la note du dossier (0 sans note), `l` : largeur du panneau
    fn largeur_note(&self, app: &App, l: f32) -> f32 {
        match app.note.as_deref() {
            Some(n) => (self.mesurer(&n.to_uppercase(), &self.f.petit) + 40.0).min(360.0).min(l * 0.35),
            None => 0.0,
        }
    }

    /// Place prise à droite du fil d'Ariane : filtre, branche et note
    fn place_droite(&self, app: &App, l: f32, rang: usize) -> f32 {
        let wf = self.largeur_filtre(app, l, rang);
        let filtre = if wf > 0.0 { wf + 16.0 } else { 0.0 };
        let branche = self.largeur_branche(app);
        let note = self.largeur_note(app, l);
        let terminal = if app.emplacement == Emplacement::Reglages { 0.0 } else { TERMINAL_RESERVE };
        terminal + filtre + if branche > 0.0 { branche + 8.0 } else { 0.0 } + if note > 0.0 { note + 8.0 } else { 0.0 }
    }

    /// Vignette reçue du fil de préparation
    pub fn recevoir_vignette(&self, chemin: String, image: Option<Image>) {
        let mut m = self.images.borrow_mut();
        // Trop d'images en attente (dossier quitté) : on les oublie, elles seront redemandées si besoin
        if m.values().filter(|v| v.is_some()).count() > 200 {
            let cles: Vec<String> = m.iter().filter(|(_, v)| v.is_some()).map(|(k, _)| k.clone()).collect();
            let mut d = self.demandees.borrow_mut();
            for k in cles {
                m.remove(&k);
                d.remove(&k);
            }
        }
        m.insert(chemin, image);
    }

    /// Demandes abandonnées par le fil de préparation : à redemander
    pub fn oublier(&self, chemins: &[String]) {
        let mut d = self.demandees.borrow_mut();
        for c in chemins {
            d.remove(c);
        }
    }

    unsafe fn bitmap(&self, c: &Cible2D, img: &Image) -> Option<ID2D1Bitmap> {
        let props = D2D1_BITMAP_PROPERTIES {
            pixelFormat: D2D1_PIXEL_FORMAT { format: DXGI_FORMAT_B8G8R8A8_UNORM, alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED },
            dpiX: 96.0,
            dpiY: 96.0,
        };
        c.rt.CreateBitmap(D2D_SIZE_U { width: img.largeur, height: img.hauteur }, Some(img.pixels.as_ptr() as *const _), img.largeur * 4, &props).ok()
    }

    /// Vignette d'un fichier pour la grille : convertie si elle est arrivée, demandée sinon
    unsafe fn vignette(&self, c: &Cible2D, chemin: &str) -> Option<ID2D1Bitmap> {
        if let Some(b) = c.vignettes.borrow().get(chemin) {
            return Some(b.clone());
        }
        let mut images = self.images.borrow_mut();
        match images.get(chemin) {
            Some(Some(_)) => {
                let img = images.remove(chemin).flatten()?;
                let b = self.bitmap(c, &img)?;
                let mut v = c.vignettes.borrow_mut();
                if v.len() > 500 {
                    v.clear();
                    self.demandees.borrow_mut().clear();
                }
                v.insert(chemin.to_string(), b.clone());
                Some(b)
            }
            Some(None) => None,
            None => {
                if self.demandees.borrow_mut().insert(chemin.to_string()) {
                    self.a_demander.borrow_mut().push(chemin.to_string());
                }
                None
            }
        }
    }

    /// Icône en points agrandie (grille, coup d'œil) : `pas` = écart entre deux points
    unsafe fn icone_grande(&self, c: &Cible2D, icone: Icone, cx: f32, cy: f32, pas: f32, blanche: bool, opacite: f32) {
        self.motif_points(c, icone.motif(), cx, cy, pas, blanche, opacite);
    }

    /// Motif de points centré en (cx, cy) : `#` blanc, `r` rouge (blanc si `blanche`)
    unsafe fn motif_points(&self, c: &Cible2D, motif: &[&str], cx: f32, cy: f32, pas: f32, blanche: bool, opacite: f32) {
        let colonnes = motif.iter().map(|l| l.len()).max().unwrap_or(1) as f32;
        let (ox, oy) = (cx - (colonnes - 1.0) * pas / 2.0, cy - (motif.len() as f32 - 1.0) * pas / 2.0);
        for (j, ligne) in motif.iter().enumerate() {
            for (i, ch) in ligne.bytes().enumerate() {
                let col = match ch {
                    b'#' => BLANC,
                    b'r' if !blanche => ACCENT,
                    b'r' => BLANC,
                    _ => continue,
                };
                self.couleur(c, &D2D1_COLOR_F { a: col.a * opacite, ..col });
                c.rt.FillEllipse(&ellipse(ox + i as f32 * pas, oy + j as f32 * pas, pas * 0.36), &c.pinceau);
            }
        }
    }

    /// Case à cocher en points : anneau, et point plein quand l'élément est choisi
    unsafe fn case_a_cocher(&self, c: &Cible2D, x: f32, y: f32, coche: bool, sur_accent: bool) {
        self.couleur(c, if sur_accent || coche { &BLANC } else { &BLANC_DOUX });
        c.rt.DrawEllipse(&ellipse(x, y, 7.0), &c.pinceau, 1.4, None);
        if coche {
            self.couleur(c, if sur_accent { &BLANC } else { &ACCENT });
            c.rt.FillEllipse(&ellipse(x, y, 3.6), &c.pinceau);
        }
    }

    /// Couleur d'un groupe (groupes::COULEURS)
    pub fn couleur_groupe(c: char) -> D2D1_COLOR_F {
        match c {
            'b' => BLANC,
            'g' => rgba(140, 140, 140, 1.0),
            'a' => rgba(0xE8, 0xA3, 0x17, 1.0),
            'v' => rgba(0x7B, 0xD8, 0x8F, 1.0),
            'u' => rgba(0x6A, 0xA9, 0xFF, 1.0),
            'p' => rgba(0xB5, 0x8C, 0xFF, 1.0),
            _ => GROUPE_ROUGE,
        }
    }

    /// Couleur d'une balise
    pub(crate) fn couleur_balise(b: Balise) -> D2D1_COLOR_F {
        match b {
            Balise::Rouge => GROUPE_ROUGE,
            Balise::Blanc => BLANC,
            Balise::Gris => rgba(140, 140, 140, 1.0),
        }
    }

    /// Barre d'onglets dans la barre de titre
    unsafe fn dessiner_onglets(&self, c: &Cible2D, app: &App, l: f32, vue: &Vue) {
        let rt = &c.rt;
        let n = vue.onglets.len();
        // Fond et liseré de l'onglet actif (lot 12A) : ils glissent d'un onglet à l'autre en 300 ms, ressort
        let zones = zones_onglets(l, n);
        {
            let onglets: Vec<D2D_RECT_F> = zones.iter().filter(|(q, _)| matches!(q, Cible::Onglet(_))).map(|(_, r)| *r).collect();
            let mut m = self.mouvements.borrow_mut();
            let t = self.t_image.get();
            let saute = !crate::reglages::animations_actives() || m.nb_onglets != n;
            m.nb_onglets = n;
            m.onglet.viser(vue.actif as f32, t, mouvement::ONGLET_MS, saute);
            let bornes: Vec<(f32, f32)> = onglets.iter().map(|z| (z.left, z.right)).collect();
            if let (Some((gauche, droite)), Some(z)) = (mouvement::interpoler(&bornes, m.onglet.valeur(t)), onglets.get(vue.actif)) {
                let z = decaler(*z, decalage_chrome(app, vue, Cible::Onglet(vue.actif)));
                let r = D2D_RECT_F { left: gauche, right: droite, ..z };
                self.couleur_ou(c, &SELECTION, &SELECTION_CLAIR);
                rt.FillRoundedRectangle(&D2D1_ROUNDED_RECT { rect: r, radiusX: 12.0, radiusY: 12.0 }, &c.pinceau);
                // Liseré supérieur de base, pas de contour complet autour de l'onglet actif
                self.couleur(c, &BLANC_TRES_FAIBLE);
                rt.FillRectangle(&rect(r.left + 8.0, r.top, r.right - r.left - 16.0, 1.0), &c.pinceau);
            }
        }
        for (quoi, r) in zones {
            let r = decaler(r, decalage_chrome(app, vue, quoi));
            let survole = app.survol == quoi;
            let arrondi = D2D1_ROUNDED_RECT { rect: r, radiusX: 12.0, radiusY: 12.0 };
            match quoi {
                Cible::Onglet(k) => {
                    let actif = k == vue.actif;
                    let fermer_survole = app.survol == Cible::FermerOnglet(k);
                    // L'onglet actif a son fond et son liseré glissants, posés avant la boucle
                    let fondu = self.survol_k(quoi).max(self.survol_k(Cible::FermerOnglet(k)));
                    if !actif && fondu > 0.0 {
                        self.couleur_k(c, &SURVOL, fondu);
                        rt.FillRoundedRectangle(&arrondi, &c.pinceau);
                    }
                    self.couleur(c, if actif { &ACCENT } else { &BLANC_TRES_FAIBLE });
                    rt.FillEllipse(&ellipse(r.left + 12.0, (r.top + r.bottom) / 2.0, 2.5), &c.pinceau);
                    let avec_croix = n > 1 && (actif || survole || fermer_survole);
                    let droite = if avec_croix { 26.0 } else { 10.0 };
                    self.couleur(c, if actif || survole { &BLANC } else { &BLANC_DOUX });
                    self.texte(c, &self.f.volet, &vue.onglets[k], rect(r.left + 22.0, r.top, r.right - r.left - 22.0 - droite, r.bottom - r.top));
                    if avec_croix {
                        self.couleur(c, if fermer_survole { &BLANC } else { &BLANC_DOUX });
                        self.points(c, r.right - 13.0, (r.top + r.bottom) / 2.0, 2.6, 1.1, &[(0, 0), (-1, -1), (1, -1), (-1, 1), (1, 1)]);
                    }
                    // Le document actif porte l'anneau quand rien dans la liste n'a le focus clavier.
                    if actif && vue.focus_clavier && app.selection.is_none() && !app.arbre_focus && app.emplacement != Emplacement::Reglages {
                        self.anneau(c, r, 12.0);
                    }
                }
                _ => {
                    let fondu = self.survol_k(quoi);
                    if fondu > 0.0 {
                        self.couleur_k(c, &SURVOL, fondu);
                        rt.FillRoundedRectangle(&arrondi, &c.pinceau);
                    }
                    self.couleur(c, if survole { &BLANC } else { &BLANC_DOUX });
                    self.points(c, (r.left + r.right) / 2.0, (r.top + r.bottom) / 2.0, 3.0, 1.2, &[(0, 0), (-1, 0), (1, 0), (0, -1), (0, 1)]);
                }
            }
        }
    }

    /// Volet d'aperçu, à droite de la liste
    unsafe fn dessiner_apercu(&self, c: &Cible2D, x0: f32, l: f32, h: f32, apercu: Option<&Apercu>) {
        let rt = &c.rt;
        let f = &self.f;
        self.couleur(c, &LIGNE);
        rt.FillRectangle(&rect(x0, HAUTEUR_TITRE + 1.0, 1.0, h - HAUTEUR_TITRE - 1.0), &c.pinceau);
        self.couleur(c, &PANNEAU);
        rt.FillRectangle(&rect(x0, HAUTEUR_TITRE + 1.0, l - x0, h - HAUTEUR_TITRE - 1.0), &c.pinceau);
        // Padding 20 / 22 de la maquette v6 : 20 en haut, 22 à gauche et à droite
        let (gauche, largeur) = (x0 + 22.0, l - x0 - 44.0);
        let haut = HAUTEUR_TITRE + 1.0 + 20.0;
        self.couleur(c, &BLANC_FAIBLE);
        self.texte(c, &f.petit, "APERÇU", rect(gauche, haut, largeur, 18.0));
        let Some(a) = apercu else {
            self.couleur(c, &BLANC_TRES_FAIBLE);
            self.texte(c, &f.petit, "…", rect(gauche, haut + 40.0, largeur, 30.0));
            return;
        };

        // Image : la miniature de Windows, sinon la grande icône en points
        let mut y = haut + 29.0;
        let haut_max = 150.0f32;
        let mut dessinee = false;
        if let Some(img) = &a.image {
            let mut cache = c.miniature.borrow_mut();
            if cache.as_ref().map_or(true, |(n, _)| *n != a.numero) {
                let props = D2D1_BITMAP_PROPERTIES {
                    pixelFormat: D2D1_PIXEL_FORMAT { format: DXGI_FORMAT_B8G8R8A8_UNORM, alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED },
                    dpiX: 96.0,
                    dpiY: 96.0,
                };
                *cache = rt
                    .CreateBitmap(D2D_SIZE_U { width: img.largeur, height: img.hauteur }, Some(img.pixels.as_ptr() as *const _), img.largeur * 4, &props)
                    .ok()
                    .map(|b| (a.numero, b));
            }
            if let Some((_, b)) = cache.as_ref() {
                let echelle = (largeur / img.largeur as f32).min(haut_max / img.hauteur as f32).min(2.0);
                let (iw, ih) = (img.largeur as f32 * echelle, img.hauteur as f32 * echelle);
                let zone = rect(gauche + (largeur - iw) / 2.0, y, iw, ih);
                rt.DrawBitmap(b, Some(&zone), 1.0, D2D1_BITMAP_INTERPOLATION_MODE_LINEAR, None);
                self.couleur(c, &LIGNE);
                rt.DrawRectangle(&zone, &c.pinceau, 1.0, None);
                y += ih + 18.0;
                dessinee = true;
            }
        }
        if !dessinee {
            // Carte de l'aperçu (maquette `.glyph`) : un carré sombre bordé, l'icône en points au centre
            let carte = D2D1_ROUNDED_RECT { rect: rect(gauche, y, largeur, 150.0), radiusX: 20.0, radiusY: 20.0 };
            self.couleur_ou(c, &CARTE, &CARTE_CLAIR);
            rt.FillRoundedRectangle(&carte, &c.pinceau);
            self.couleur(c, &LIGNE);
            rt.DrawRoundedRectangle(&carte, &c.pinceau, 1.0, None);
            // Icône agrandie : chaque point de la matrice 7×7 devient un gros point
            let motif = a.icone.motif();
            let pas = 13.0;
            let cote = 7.0 * pas;
            let (ox, oy) = (gauche + (largeur - cote) / 2.0 + pas / 2.0, y + 75.0 - cote / 2.0 + pas / 2.0);
            for (j, ligne) in motif.iter().enumerate() {
                for (i, ch) in ligne.bytes().enumerate() {
                    let col = match ch {
                        b'#' => &BLANC,
                        b'r' => &ACCENT,
                        _ => &BLANC_TRES_FAIBLE,
                    };
                    self.couleur(c, col);
                    let r = if ch == b'.' { 1.3 } else { 5.2 };
                    rt.FillEllipse(&ellipse(ox + i as f32 * pas, oy + j as f32 * pas, r), &c.pinceau);
                }
            }
            y += 170.0;
        }

        // Nom (Segoe 18) puis les informations : libellé 12, valeur 14, comme la maquette
        self.couleur(c, &BLANC);
        self.texte(c, &f.apercu_nom, &a.nom, rect(gauche, y, largeur, 28.0));
        y += 40.0;
        for (libelle, valeur) in &a.infos {
            if y > h - 60.0 {
                break;
            }
            self.couleur(c, &BLANC_FAIBLE);
            self.texte(c, &f.petit, libelle, rect(gauche, y, largeur, 18.0));
            self.couleur(c, &BLANC);
            self.texte(c, &f.liste, valeur, rect(gauche, y + 16.0, largeur, 20.0));
            y += 44.0;
        }

        // Début du fichier texte
        if let Some(t) = &a.texte {
            let zone = rect(gauche - 6.0, y + 4.0, largeur + 12.0, (h - y - 24.0).max(0.0));
            if zone.bottom - zone.top > 40.0 {
                self.couleur(c, &CHAMP_FOND);
                rt.FillRoundedRectangle(&D2D1_ROUNDED_RECT { rect: zone, radiusX: 6.0, radiusY: 6.0 }, &c.pinceau);
                self.couleur(c, &BLANC_DOUX);
                let interieur = rect(zone.left + 10.0, zone.top + 8.0, zone.right - zone.left - 20.0, zone.bottom - zone.top - 16.0);
                rt.PushAxisAlignedClip(&interieur, D2D1_ANTIALIAS_MODE_ALIASED);
                self.texte_teinte(c, t, &a.teintes, interieur);
                rt.PopAxisAlignedClip();
            }
        }
    }

    /// Panneau terminal intégré (196 px) : journal réel des opérations, fermeture, et ouverture du vrai Windows Terminal.
    unsafe fn dessiner_terminal(&self, c: &Cible2D, app: &App, l: f32, h: f32, lignes: &[crate::journal::Operation], vue: &Vue) {
        let rt = &c.rt;
        let p = zone_terminal_panneau(l, h);
        if p.bottom - p.top < 40.0 || p.right - p.left < 80.0 {
            return;
        }
        self.couleur(c, &PANNEAU);
        rt.FillRectangle(&p, &c.pinceau);
        self.couleur(c, &LIGNE);
        rt.DrawRectangle(&p, &c.pinceau, 1.0, None);

        // En-tête : invite `>_`, titre et journal des opérations
        self.couleur(c, &ACCENT);
        self.points(c, p.left + 22.0, p.top + 21.0, 3.0, 1.15, &[(0, -2), (1, -1), (2, 0), (1, 1), (0, 2)]);
        rt.FillRectangle(&rect(p.left + 30.0, p.top + 24.0, 10.0, 1.5), &c.pinceau);
        self.couleur(c, &BLANC);
        self.texte(c, &self.f.petit, "TERMINAL", rect(p.left + 50.0, p.top + 10.0, 96.0, 24.0));
        self.couleur(c, &BLANC_FAIBLE);
        self.texte(c, &self.f.petit, "JOURNAL DES OPERATIONS", rect(p.left + 146.0, p.top + 10.0, p.right - p.left - 200.0, 24.0));

        // Fermeture : croix
        let fermer = decaler(zone_terminal_fermer(l, h), decalage_chrome(app, vue, Cible::TerminalFermer));
        let survole_fermer = app.survol == Cible::TerminalFermer;
        let fondu_fermer = self.survol_k(Cible::TerminalFermer);
        if fondu_fermer > 0.0 {
            self.couleur_k(c, &SURVOL, fondu_fermer);
            rt.FillRoundedRectangle(&D2D1_ROUNDED_RECT { rect: fermer, radiusX: 6.0, radiusY: 6.0 }, &c.pinceau);
        }
        self.couleur(c, if survole_fermer { &BLANC } else { &BLANC_DOUX });
        self.points(c, (fermer.left + fermer.right) / 2.0, (fermer.top + fermer.bottom) / 2.0, 3.4, 1.1, &[(0, 0), (-1, -1), (1, -1), (-1, 1), (1, 1)]);

        self.couleur(c, &LIGNE);
        rt.FillRectangle(&rect(p.left, p.top + 42.0, p.right - p.left, 1.0), &c.pinceau);

        // Corps : les cinq dernières opérations, de la plus ancienne à la plus récente
        let mut y0 = p.top + 46.0;
        if lignes.is_empty() {
            self.couleur(c, &BLANC_FAIBLE);
            self.texte(c, &self.f.petit, "AUCUNE OPERATION RECENTE", rect(p.left + 24.0, y0, p.right - p.left - 48.0, 20.0));
            self.couleur(c, &BLANC_TRES_FAIBLE);
            self.texte(c, &self.f.petit, "COPIE, DEPLACEMENT, CORBEILLE ET RENOMMAGE S'INSCRIVENT ICI.", rect(p.left + 24.0, y0 + 22.0, p.right - p.left - 48.0, 20.0));
        } else {
            let place = ((p.bottom - 46.0 - y0) / 20.0).floor().max(0.0) as usize;
            for op in lignes.iter().rev().take(place).rev() {
                let heure = op.heure();
                self.couleur(c, &BLANC_TRES_FAIBLE);
                self.texte(c, &self.f.petit_mono, &heure, rect(p.left + 24.0, y0, 44.0, 20.0));
                self.couleur(c, &ACCENT);
                self.texte(c, &self.f.petit_mono, &op.action.to_uppercase(), rect(p.left + 72.0, y0, 86.0, 20.0));
                self.couleur(c, &BLANC_DOUX);
                self.texte(c, &self.f.petit_mono, &op.detail, rect(p.left + 164.0, y0, (p.right - p.left - 188.0).max(20.0), 20.0));
                y0 += 20.0;
            }
        }

        // Bas du panneau : Échap ferme, et l'action séparée Windows Terminal
        self.couleur(c, &LIGNE);
        rt.FillRectangle(&rect(p.left, p.bottom - 44.0, p.right - p.left, 1.0), &c.pinceau);
        self.aide(c, "ECHAP FERMER", rect(p.left + 24.0, p.bottom - 38.0, 180.0, 28.0), Pose::Gauche, &BLANC_FAIBLE);
        let action = decaler(zone_terminal_action(l, h), decalage_chrome(app, vue, Cible::TerminalExterne));
        let possible = app.dossier_ici().is_some();
        let survole_action = app.survol == Cible::TerminalExterne;
        if survole_action && possible {
            self.couleur(c, &SURVOL);
            rt.FillRoundedRectangle(&D2D1_ROUNDED_RECT { rect: action, radiusX: 7.0, radiusY: 7.0 }, &c.pinceau);
        }
        self.couleur(c, if possible { &LIGNE } else { &BLANC_TRES_FAIBLE });
        rt.DrawRoundedRectangle(&D2D1_ROUNDED_RECT { rect: action, radiusX: 7.0, radiusY: 7.0 }, &c.pinceau, 1.0, None);
        self.couleur(c, if !possible { &BLANC_TRES_FAIBLE } else if survole_action { &BLANC } else { &BLANC_DOUX });
        self.texte(c, &self.f.petit, "OUVRIR DANS WINDOWS TERMINAL", rect(action.left + 10.0, action.top, action.right - action.left - 20.0, action.bottom - action.top));
    }

    unsafe fn dessiner_tout(&self, c: &Cible2D, app: &App, autre: Option<&App>, l: f32, h: f32, vue: &Vue) {
        let rt = &c.rt;
        let f = &self.f;
        // Fond client noir opaque (l'acrylique a été retiré au lot 1) : le clear ne sert qu'à repartir d'un tampon propre
        rt.Clear(Some(&rgba(0, 0, 0, 0.0)));
        self.couleur(c, &VOILE);
        rt.FillRectangle(&rect(0.0, 0.0, l, h), &c.pinceau);
        let t_image = mouvement::maintenant_ms();
        self.t_image.set(t_image);
        self.cascade.set(vue.cascade);
        {
            // Lot 12C : numéro d'image (les entrées non revues à la fin sont retirées) et mouvement de la page des Réglages
            let mut m = self.mouvements.borrow_mut();
            m.debut_image();
            m.page.maj(vue.page.is_some() && !vue.double, t_image, crate::reglages::animations_actives());
            if vue.page.is_none() && vue.page_sortie.is_none() {
                m.oublier_page();
            }
        }
        self.panneau.set(0);
        // Ouverture / fermeture (lot 12A) : tout le contenu est dessiné décalé, réduit autour du bas-centre, dans un calque
        // à coins arrondis et à opacité propre ; le VOILE déjà posé reste visible autour (pas de flash noir en thème clair)
        let calque = vue.fenetre.and_then(|e| self.ouvrir_calque_fenetre(c, e, l, h));
        if crate::app::ESSENTIEL.load(std::sync::atomic::Ordering::Relaxed) {
            // Mode Essentiel : trame deux fois plus serrée, comme l'écran essentiel des téléphones Nothing
            c.trame.SetTransform(&windows::Foundation::Numerics::Matrix3x2 { M11: 0.5, M12: 0.0, M21: 0.0, M22: 0.5, M31: 0.0, M32: 0.0 });
            self.trame_plein(c, &rect(0.0, 0.0, l, h));
            c.trame.SetTransform(&windows::Foundation::Numerics::Matrix3x2::identity());
        } else {
            self.trame_plein(c, &rect(0.0, 0.0, l, h));
        }
        // ── Liseré supérieur de 1 px (maquette `.win:before`) : 10 % → 90 %, blanc 55 % sous 70 % d'opacité ──
        let marge_lisere = l * 0.1;
        if marge_lisere > 0.0 && l > 2.0 * marge_lisere {
            let paliers = [
                D2D1_GRADIENT_STOP { position: 0.0, color: rgba(255, 255, 255, 0.0) },
                D2D1_GRADIENT_STOP { position: 0.5, color: crate::theme::contraste(&LISERE_HAUT) },
                D2D1_GRADIENT_STOP { position: 1.0, color: rgba(255, 255, 255, 0.0) },
            ];
            if let Ok(degrade) = rt.CreateGradientStopCollection(&paliers, D2D1_GAMMA_2_2, D2D1_EXTEND_MODE_CLAMP) {
                let proprietes = D2D1_LINEAR_GRADIENT_BRUSH_PROPERTIES {
                    startPoint: D2D_POINT_2F { x: marge_lisere, y: 0.5 },
                    endPoint: D2D_POINT_2F { x: l - marge_lisere, y: 0.5 },
                };
                if let Ok(lisere) = rt.CreateLinearGradientBrush(&proprietes, None, &degrade) {
                    lisere.SetOpacity(0.7);
                    rt.FillRectangle(&rect(marge_lisere, 0.0, l - 2.0 * marge_lisere, 1.0), &lisere);
                }
            }
        }
        // ── Barre de titre : marque en 3 × 3 points, puis les onglets ──
        let (lx, ly) = (22.0, HAUTEUR_TITRE / 2.0 - 9.5);
        for j in 0..3 {
            for i in 0..3 {
                self.couleur(c, if (i, j) == (2, 1) { &ACCENT } else { &BLANC_DOUX });
                rt.FillRectangle(&rect(lx + i as f32 * 7.0, ly + j as f32 * 7.0, 5.0, 5.0), &c.pinceau);
            }
        }
        self.couleur(c, &BLANC);            self.texte(c, &f.titre, "SPACEFILES", rect(48.0, 0.0, largeur_volet() - 56.0, HAUTEUR_TITRE));
        self.dessiner_onglets(c, app, l, vue);

        // ── Loupe, Essentiel et Réglages, à droite avant les contrôles système ──
        for (quoi, r) in [(Cible::RechercheHaut, zone_recherche_haut(l)), (Cible::EssentielHaut, zone_essentiel_haut(l)), (Cible::Reglages, zone_reglages_haut(l))] {
            let r = decaler(r, decalage_chrome(app, vue, quoi));
            let survole = app.survol == quoi;
            let actif = match quoi {
                Cible::Reglages => app.emplacement == Emplacement::Reglages,
                Cible::EssentielHaut => crate::app::ESSENTIEL.load(std::sync::atomic::Ordering::Relaxed),
                _ => false,
            };
            let k = self.survol_k(quoi);
            if actif || k > 0.0 {
                if actif { self.couleur(c, &SURVOL_FORT) } else { self.couleur_k(c, &SURVOL, k) }
                rt.FillRoundedRectangle(&D2D1_ROUNDED_RECT { rect: r, radiusX: 8.0, radiusY: 8.0 }, &c.pinceau);
            }
            self.couleur(c, if actif { &ACCENT } else if survole { &BLANC } else { &BLANC_DOUX });
            let (cx, cy) = ((r.left + r.right) / 2.0, (r.top + r.bottom) / 2.0);
            match quoi {
                Cible::RechercheHaut => self.points(c, cx, cy - 1.0, 2.6, 1.0, &[(0, -2), (1, -2), (-1, -1), (2, -1), (-1, 0), (2, 0), (0, 1), (1, 1), (2, 2), (3, 3)]),
                Cible::EssentielHaut => self.points(c, cx, cy, 2.4, 1.0, &[(0, -3), (0, -2), (-1, -1), (0, -1), (1, -1), (0, 0), (0, 1), (0, 2), (0, 3), (-2, 0), (-1, 0), (1, 0), (2, 0)]),
                _ => self.icone(c, Icone::Reglages, cx, cy, false, if survole || actif { 1.0 } else { 0.65 }),
            }
        }

        let boutons = [(Cible::Reduire, 3), (Cible::Agrandir, 2), (Cible::Fermer, 1)];
        for (quoi, rang) in boutons {
            let cx = l - BOUTON_LARGEUR * rang as f32 + BOUTON_LARGEUR / 2.0;
            let cy = HAUTEUR_TITRE / 2.0;
            let actif = app.survol == quoi;
            self.couleur(c, if actif { &BLANC } else { &BLANC_DOUX });
            let motif: &[(i32, i32)] = match quoi {
                Cible::Reduire => &[(-1, 0), (0, 0), (1, 0)],
                Cible::Agrandir => &[(-1, -1), (0, -1), (1, -1), (-1, 0), (1, 0), (-1, 1), (0, 1), (1, 1)],
                _ => &[(0, 0), (-1, -1), (1, -1), (-1, 1), (1, 1)],
            };
            self.points(c, cx, cy, 5.0, 1.6, motif);
        }
        self.couleur(c, &LIGNE);
        rt.FillRectangle(&rect(0.0, HAUTEUR_TITRE, l, 1.0), &c.pinceau);

        // ── Volet gauche : favoris et disques (replié en mode Essentiel) ──
        if largeur_volet() > 0.5 {
        rt.FillRectangle(&rect(largeur_volet(), HAUTEUR_TITRE + 1.0, 1.0, h - HAUTEUR_TITRE - 1.0), &c.pinceau);
        // Lot 12D : le contenu du volet se fond en 250 ms pendant que sa colonne glisse (`.side`)
        let garde_volet = self.flip_pousser(c, &mouvement::Decalage { dx: 0.0, dy: 0.0, echelle: 1.0, opacite: self.fondu_volets.get().0 }, (0.0, 0.0), rect(0.0, HAUTEUR_TITRE, largeur_volet() + 1.0, h - HAUTEUR_TITRE), &self.couche_opacite(c, self.fondu_volets.get().0));
        // Le volet s'arrête au-dessus du bouton des réglages (beaucoup de stockages : la fin est coupée)
        let bas_volet = h - 46.0;
        rt.PushAxisAlignedClip(&rect(0.0, HAUTEUR_TITRE + 1.0, largeur_volet(), bas_volet - HAUTEUR_TITRE - 1.0), D2D1_ANTIALIAS_MODE_ALIASED);
        let (titres, zones) = disposition_volet(app);
        self.couleur(c, &BLANC_DOUX);
        for (titre, y, repliee, n) in titres {
            self.texte(c, &f.petit, titre, rect(VOLET_PAD + 12.0, y, largeur_volet() - VOLET_PAD - 42.0, VOLET_ENTETE));
            // Section repliée : son nombre d'éléments et « + » à droite ; au survol, « − » pour replier
            let survole = matches!(app.survol, Cible::TitreVolet(_)) && zones.iter().any(|(q, r)| *q == app.survol && (r.top - y).abs() < 0.5);
            if repliee || survole {
                let t = if repliee { format!("{n}  +") } else { "−".to_string() };
                self.texte(c, &f.petit_droite, &t, rect(VOLET_PAD, y, largeur_volet() - 2.0 * VOLET_PAD - 12.0, VOLET_ENTETE));
            }
        }
        let lignes_arbre = if crate::reglages::ARBRE.load(std::sync::atomic::Ordering::Relaxed) { app.lignes_arbre() } else { Vec::new() };
        for (quoi, r) in zones {
            let r = decaler(r, decalage_chrome(app, vue, quoi));
            if quoi == Cible::CePc {
                let actif = app.emplacement == Emplacement::CePC;
                let survole = app.survol == quoi;
                let fondu = self.survol_k(quoi);
                if actif || fondu > 0.0 {
                    if actif { self.couleur_ou(c, &SELECTION, &SELECTION_CLAIR); } else { self.couleur_k(c, &SURVOL, fondu); }
                    rt.FillRoundedRectangle(&D2D1_ROUNDED_RECT { rect: r, radiusX: 10.0, radiusY: 10.0 }, &c.pinceau);
                    if actif {
                        self.couleur(c, &ACCENT_BORD);
                        rt.DrawRoundedRectangle(&D2D1_ROUNDED_RECT { rect: r, radiusX: 10.0, radiusY: 10.0 }, &c.pinceau, 1.0, None);
                    }
                }
                self.couleur(c, if actif || survole { &BLANC } else { &BLANC_DOUX });
                self.icone(c, Icone::Bureau, r.left + 22.0, (r.top + r.bottom) / 2.0, false, if actif || survole { 1.0 } else { 0.72 });
                self.couleur(c, &BLANC);
                self.texte_espace(c, &f.volet_nom, "CE PC", rect(r.left + 46.0, r.top, r.right - r.left - 56.0, r.bottom - r.top));
                continue;
            }
            if let Cible::Arbre(i) = quoi {
                if let Some(l) = lignes_arbre.get(i) {
                    self.ligne_arbre(c, app, l, r, app.survol == quoi, self.survol_k(quoi), app.depot == quoi);
                }
                continue;
            }
            let actif = app.actif(quoi);
            let (nom, icone) = match quoi {
                Cible::Favori(i) => (app.favoris[i].nom.as_str(), app.favoris[i].icone),
                Cible::Epingle(i) => (app.epingles[i].nom.as_str(), app.epingles[i].icone),
                Cible::Nuage(i) => (app.nuages[i].nom.as_str(), app.nuages[i].icone),
                Cible::Serveur(i) => (app.serveurs[i].nom.as_str(), app.serveurs[i].icone),
                Cible::Disque(i) => (app.disques[i].nom.as_str(), Icone::Disque),
                Cible::Groupe(i) => (app.groupes[i].nom.as_str(), Icone::Dossier),
                _ => continue,
            };
            let survole = app.survol == quoi;
            let fondu = self.survol_k(quoi);
            if actif || fondu > 0.0 {
                if actif { self.couleur_ou(c, &SELECTION, &SELECTION_CLAIR); } else { self.couleur_k(c, &SURVOL, fondu); }
                rt.FillRoundedRectangle(&D2D1_ROUNDED_RECT { rect: r, radiusX: 10.0, radiusY: 10.0 }, &c.pinceau);
                if actif {
                    self.couleur(c, &ACCENT_BORD);
                    rt.DrawRoundedRectangle(&D2D1_ROUNDED_RECT { rect: r, radiusX: 10.0, radiusY: 10.0 }, &c.pinceau, 1.0, None);
                }
            }
            if app.depot == quoi {
                self.couleur(c, &ACCENT);
                rt.DrawRoundedRectangle(&D2D1_ROUNDED_RECT { rect: r, radiusX: 7.0, radiusY: 7.0 }, &c.pinceau, 1.5, None);
            }
            if let Cible::Disque(i) = quoi {
                // Carte du disque (maquette v6) : contour, nom, jauge et place libre en toutes lettres
                let carte = D2D1_ROUNDED_RECT { rect: r, radiusX: 14.0, radiusY: 14.0 };
                self.couleur(c, &CHAMP_FOND);
                rt.FillRoundedRectangle(&carte, &c.pinceau);
                self.couleur(c, if actif { &ACCENT_BORD } else { &LIGNE });
                rt.DrawRoundedRectangle(&carte, &c.pinceau, 1.0, None);
                self.icone(c, Icone::Disque, r.left + 22.0, r.top + 24.0, false, 0.85);
                self.couleur(c, &BLANC);
                self.texte_espace(c, &f.volet_nom, &nom.to_uppercase(), rect(r.left + 44.0, r.top + 6.0, r.right - r.left - 56.0, 30.0));
                if let Genre::Disque { libre, total, .. } = app.disques[i].genre {
                    let rempli = if total > 0 { 1.0 - libre as f64 / total as f64 } else { 0.0 };
                    let (mx, my, mw) = (r.left + 14.0, r.top + 44.0, r.right - r.left - 28.0);
                    self.couleur(c, &BLANC_TRES_FAIBLE);
                    rt.FillRoundedRectangle(&D2D1_ROUNDED_RECT { rect: rect(mx, my, mw, 4.0), radiusX: 2.0, radiusY: 2.0 }, &c.pinceau);
                    if rempli > 0.0 {
                        self.couleur(c, if rempli > 0.9 { &ERREUR } else { &BLANC_DOUX });
                        rt.FillRoundedRectangle(&D2D1_ROUNDED_RECT { rect: rect(mx, my, (mw as f64 * rempli) as f32, 4.0), radiusX: 2.0, radiusY: 2.0 }, &c.pinceau);
                    }
                    // Maquette v6 : « 181 Go libres sur 476 Go » en casse normale ; au-delà de 90 %, le mot « Presque plein · 28 Go libres »
                    // en couleur d'erreur avec un `▲` (`.st.wr` : triangle de 9 px, 6 d'écart) : la couleur ne porte jamais seule l'état
                    let x_texte = r.left + 14.0;
                    if rempli > 0.9 {
                        let place = r.right - r.left - 28.0 - 15.0;
                        let detail = crate::dossier::etiquette_disque(true, false, libre, &|t| self.mesurer(t, &f.petit) <= place);
                        self.couleur(c, &ERREUR);
                        self.triangle_plein(c, x_texte + 4.5, r.top + 59.0, 4.5);
                        self.texte(c, &f.petit, &detail, rect(x_texte + 15.0, r.top + 50.0, place, 18.0));
                    } else {
                        self.couleur(c, &BLANC_FAIBLE);
                        self.texte(c, &f.petit, &crate::dossier::libres_sur(libre, total), rect(x_texte, r.top + 50.0, r.right - r.left - 28.0, 18.0));
                    }
                }
                continue;
            }
            let ligne_h = r.bottom - r.top;
            if let Cible::Groupe(i) = quoi {
                // Groupe : un point de 8 px de sa couleur, et son nombre d'éléments
                let g = &app.groupes[i];
                self.couleur(c, &Self::couleur_groupe(g.couleur));
                rt.FillEllipse(&ellipse(r.left + 22.0, r.top + ligne_h / 2.0, 4.0), &c.pinceau);
                if !actif {
                    self.couleur(c, &BLANC_TRES_FAIBLE);
                    self.texte(c, &f.petit_droite, &g.nombre.to_string(), rect(r.left, r.top, r.right - r.left - 12.0, ligne_h));
                }
            } else {
                self.icone(c, icone, r.left + 22.0, r.top + ligne_h / 2.0, false, if actif || survole { 1.0 } else { 0.7 });
            }
            // Lot 14b (`.st`) : mot d'état mesuré à droite des lignes CLOUD et SERVEURS, en casse normale (Segoe 12), précédé d'un `✓` ou
            // d'un point ; rien tant que l'état n'est pas mesuré. Le nom se tronque pour lui laisser la place.
            let cle_etat = match quoi {
                Cible::Nuage(i) => Some(app.nuages[i].detail.as_str()),
                Cible::Serveur(i) => Some(app.serveurs[i].detail.as_str()),
                _ => None,
            };
            let mut droite_nom = r.right - 12.0;
            if let Some(e) = cle_etat.and_then(|k| app.etats.lire(k)) {
                let m = crate::etats::mot(e);
                let largeur = self.mesurer(m.texte, &f.petit_droite).ceil() + 2.0;
                let x_mot = r.right - 12.0 - largeur;
                let (cx, cy) = (x_mot - 10.0, r.top + ligne_h / 2.0);
                self.couleur(c, match m.ton {
                    crate::etats::Ton::Accent => &ACCENT,
                    crate::etats::Ton::Neutre => &BLANC_FAIBLE,
                    crate::etats::Ton::Erreur => &ERREUR,
                });
                match m.marque {
                    crate::etats::Marque::Point => rt.FillEllipse(&ellipse(cx, cy, 3.0), &c.pinceau),
                    crate::etats::Marque::Coche => {
                        let (a, b, d) = (D2D_POINT_2F { x: cx - 3.5, y: cy + 0.5 }, D2D_POINT_2F { x: cx - 1.0, y: cy + 3.0 }, D2D_POINT_2F { x: cx + 3.5, y: cy - 3.0 });
                        rt.DrawLine(a, b, &c.pinceau, 1.5, Some(&self.trait_rond));
                        rt.DrawLine(b, d, &c.pinceau, 1.5, Some(&self.trait_rond));
                    }
                }
                self.texte(c, &f.petit_droite, m.texte, rect(x_mot, r.top, largeur + 1.0, ligne_h));
                droite_nom = cx - 3.5 - 8.0;
            }
            // `.it .d` : le nom est en `--i1` quelle que soit la ligne, seule l'icône reste atténuée
            self.couleur(c, &BLANC);
            let fin_nom = if cle_etat.is_some() { droite_nom } else { r.right - 12.0 };
            self.texte_espace(c, &f.volet_nom, &nom.to_uppercase(), rect(r.left + 46.0, r.top, fin_nom - (r.left + 46.0), ligne_h));
        }

        rt.PopAxisAlignedClip();
        self.flip_retirer(c, garde_volet);

        }

        // Volet d'aperçu (pour le panneau qui a le focus) : la suite se limite à la largeur restante
        let largeur_totale = l;
        let ll = largeur_liste(l, vue.apercu_visible);
        if crate::theme::largeur_apercu(l, vue.apercu_visible) > 0.5 {
            // L'aperçu s'arrête au-dessus du pied : celui-ci court sur toute la largeur de la fenêtre. Pendant son glissement le
            // contenu garde sa largeur finale, rogné par la colonne, et se fond en 250 ms (`.prev`)
            let col = rect(ll, HAUTEUR_TITRE + 1.0, largeur_totale - ll, h - STATUT_H - HAUTEUR_TITRE - 1.0);
            rt.PushAxisAlignedClip(&col, D2D1_ANTIALIAS_MODE_ALIASED);
            let garde_apercu = self.flip_pousser(c, &mouvement::Decalage { dx: 0.0, dy: 0.0, echelle: 1.0, opacite: self.fondu_volets.get().1 }, (0.0, 0.0), col, &self.couche_opacite(c, self.fondu_volets.get().1));
            let x_plein = if crate::theme::largeur_apercu(l, vue.apercu_visible) < crate::theme::LARGEUR_APERCU { largeur_totale - crate::theme::LARGEUR_APERCU } else { ll };
            self.dessiner_apercu(c, x_plein, largeur_totale, h - STATUT_H, vue.apercu);
            self.flip_retirer(c, garde_apercu);
            rt.PopAxisAlignedClip();
            if x_plein != ll {
                self.couleur(c, &LIGNE);
                rt.FillRectangle(&rect(ll, HAUTEUR_TITRE + 1.0, 1.0, h - HAUTEUR_TITRE - 1.0), &c.pinceau);
            }
        }
        match autre {
            None => self.dessiner_contenu(c, app, ll, h, vue, true, true),
            Some(o) => {
                // Double panneau : chacun est dessiné comme une fenêtre plus étroite, celui de droite décalé
                let lp = largeur_panneau(ll, true);
                let moitie = lp - largeur_volet();
                // Lot 12C : à l'ouverture (F6) le panneau de droite arrive de 30 DIP à droite en fondu, 420 ms au ressort
                let arrivee = self.mouvements.borrow_mut().second_panneau(true, t_image, crate::reglages::animations_actives());
                let (gauche, droite) = if vue.focus_droite { (o, app) } else { (app, o) };
                rt.PushAxisAlignedClip(&rect(largeur_volet() + 1.0, HAUTEUR_TITRE + 1.0, moitie - 1.0, h), D2D1_ANTIALIAS_MODE_ALIASED);
                self.dessiner_contenu(c, gauche, lp, h, vue, !vue.focus_droite, true);
                rt.PopAxisAlignedClip();
                self.couleur(c, &LIGNE);
                rt.FillRectangle(&rect(lp, HAUTEUR_TITRE + 1.0, 1.0, h - HAUTEUR_TITRE - 1.0), &c.pinceau);
                rt.SetTransform(&Matrix3x2::translation(moitie, 0.0));
                let d = mouvement::Decalage { dx: mouvement::SECOND_PANNEAU_DECALAGE * (1.0 - arrivee), dy: 0.0, echelle: 1.0, opacite: arrivee.clamp(0.0, 1.0) };
                let zone = rect(largeur_volet() + 1.0, HAUTEUR_TITRE + 1.0, moitie - 1.0, h);
                self.avec_decalage(c, &d, ((zone.left + zone.right) / 2.0, (zone.top + zone.bottom) / 2.0), rect(zone.left - 40.0, zone.top, zone.right - zone.left + 80.0, zone.bottom - zone.top), || {
                    rt.PushAxisAlignedClip(&zone, D2D1_ANTIALIAS_MODE_ALIASED);
                    self.dessiner_contenu(c, droite, lp, h, vue, vue.focus_droite, false);
                    rt.PopAxisAlignedClip();
                });
                rt.SetTransform(&Matrix3x2::identity());
            }
        }
        if autre.is_none() {
            let mut m = self.mouvements.borrow_mut();
            m.second_panneau(false, t_image, true);
        }
        // Lot 12C : la page des Réglages qu'on vient de quitter s'efface encore 160 ms (opacité 1 → 0, échelle 1 → .98)
        if let (Some(sortie), None) = (&vue.page_sortie, autre) {
            let etat = self.mouvements.borrow().page.etat(t_image);
            if let Some((false, e)) = etat {
                let d = mouvement::Decalage { dx: 0.0, dy: 0.0, echelle: e.echelle, opacite: e.opacite };
                self.avec_decalage(c, &d, (ll / 2.0, (HAUTEUR_TITRE + h - STATUT_H) / 2.0), rect(0.0, HAUTEUR_TITRE, ll, h - HAUTEUR_TITRE), || self.dessiner_page(c, app, ll, h, sortie, true));
            }
        }
        // ── Pied de la fenêtre : séparateur et gestes sur toute la largeur, aperçu compris ──
        let ys = h - STATUT_H;
        self.couleur(c, &LIGNE);
        rt.FillRectangle(&rect(0.0, ys, largeur_totale, 1.0), &c.pinceau);
        match Self::texte_operation(app) {
            Some(t) => {
                let zone = rect(largeur_totale - 400.0, ys, 400.0 - 20.0, STATUT_H);
                self.aide(c, &t, zone, Pose::Droite, &BLANC_FAIBLE);
            }
            None => self.pied(c, largeur_totale - 20.0, ys, &BLANC_FAIBLE),
        }
        // Lot 15B : tiroir Historique, sous les pastilles (la carte « Action annulée » reste lisible) et sous les fenêtres modales
        {
            let avance = self.mouvements.borrow_mut().tiroir(vue.historique.ouvert, t_image, crate::reglages::animations_actives());
            if avance > 0.001 {
                self.dessiner_historique(c, &vue.historique, avance, l, h, vue.pointeur);
            } else {
                *self.zones_historique.borrow_mut() = ZonesHistorique::default();
            }
        }
        // Pastilles d'opération, posées sur le bord haut de la barre d'état ; les zones restent pour le survol et les clics
        // Lot 12D : chaque carte se déploie au ressort (`.card .more`), entre et sort comme dans la maquette (`addOp`, `dropOp`)
        let animer = crate::reglages::animations_actives();
        let deploiements = {
            let mut m = self.mouvements.borrow_mut();
            m.cartes.debut_image();
            m.cartes.deploiements(vue.pastilles, vue.pastille_ouverte, t_image, animer)
        };
        let ks: Vec<f32> = deploiements.iter().map(|d| d.0).collect();
        let disposition = pastilles::mettre_en_page_anime(l, largeur_volet(), h - STATUT_H - 8.0, vue.pastilles, &ks, largeur_touches("CTRL+Z"), self.mesurer("100 %", &self.f.volet).max(self.mesurer("PAUSE", &self.f.volet)).ceil(), &|t| self.mesurer(t, &self.f.volet));
        let decalages_cartes = self.mouvements.borrow_mut().cartes.image(&disposition, vue.pastilles, t_image, animer);
        self.dessiner_pastilles(c, &disposition, vue, app.survol, &decalages_cartes, &deploiements);
        {
            let mut m = self.mouvements.borrow_mut();
            m.cartes.fin_image();
        }
        *self.zones_pastilles.borrow_mut() = disposition;
        // Panneau terminal : au-dessus de la liste et des pastilles, jamais au-dessus du pied
        if vue.terminal_visible {
            self.dessiner_terminal(c, app, ll, h, vue.terminal_lignes, vue);
        }
        // Pixel dans la barre d'état : sous les fenêtres modales ; en vol ou dans la palette, il passe par-dessus (voir plus bas)
        if vue.pixel.place <= 0.0 {
            self.dessiner_pixel(c, &vue.pixel, l, h);
        }
        // Lot 12C : renommage par lot (`translateY(14) scale(.96)`, 420 ms), palette (descend de 10 DIP, 480 ms) et menu contextuel
        // (`scale(.94) translateY(-4)`, 260 ms) apparaissent en glissant ; leur sortie n'est pas animée (voir l'inventaire)
        let plein = rect(0.0, 0.0, largeur_totale, h);
        let d_lot = self.decalage_apparition(mouvement::APPARU_LOT, vue.lot.is_some(), mouvement::APPARU_CARTE_MS, 14.0, 0.96);
        if let Some(lot) = vue.lot {
            self.avec_decalage(c, &d_lot, (largeur_totale / 2.0, h / 2.0), plein, || self.dessiner_lot(c, lot, ll, h));
        }
        // Lot 12D : le coup d'œil se déploie depuis la ligne choisie (l. 504, 460 ms au ressort) ; son contenu paraît ensuite en 180 ms
        // et il se replie sur la ligne en 240 ms (l. 497)
        let animer_cadres = crate::reglages::animations_actives();
        let cible_galerie = [0.0, HAUTEUR_TITRE + 1.0, largeur_totale, h - HAUTEUR_TITRE - 1.0];
        let source_galerie = Self::rect_ligne_choisie(app, ll, vue.galerie.map_or(0, |g| g.1), cible_galerie);
        let etat_galerie = self.mouvements.borrow_mut().morph_galerie.maj(vue.galerie.is_some(), source_galerie, cible_galerie, t_image, animer_cadres);
        if etat_galerie.visible {
            if let Some(cadre) = etat_galerie.cadre {
                self.carte(c, rect(cadre[0], cadre[1], cadre[2], cadre[3]));
            } else if let Some(g) = vue.galerie {
                self.carte(c, rect(cible_galerie[0], cible_galerie[1], cible_galerie[2], cible_galerie[3]));
                let d = mouvement::Decalage { dx: 0.0, dy: 0.0, echelle: 1.0, opacite: etat_galerie.contenu };
                self.avec_decalage(c, &d, (largeur_totale / 2.0, h / 2.0), plein, || self.dessiner_galerie(c, g, largeur_totale, h));
            }
        }
        // La palette se révèle par le haut (`clip-path: inset(0 0 100% 0)` vers `inset(0)`) en tombant de 10 DIP, 480 ms au ressort, et se
        // referme à l'envers en 260 ms (l. 638-645) ; le voile derrière paraît et disparaît d'un coup
        let d_palette = self.decalage_apparition(mouvement::APPARU_PALETTE, vue.palette.is_some(), mouvement::APPARU_PALETTE_MS, -10.0, 1.0);
        let fermeture_palette = self.mouvements.borrow_mut().palette_fermee(vue.palette.is_some(), t_image, animer_cadres);
        if vue.palette.is_none() {
            // Zones de la palette fermée : un clic ne doit pas utiliser la géométrie d'une ancienne ouverture
            *self.zones_palette.borrow_mut() = ZonesPalette::default();
        }
        if let Some(p) = vue.palette {
            *self.derniere_palette.borrow_mut() = Some(p.clone());
            self.couleur(c, &rgba(0, 0, 0, 0.45));
            rt.FillRectangle(&rect(0.0, HAUTEUR_TITRE + 1.0, largeur_totale, h - HAUTEUR_TITRE - 1.0), &c.pinceau);
            let zone = Self::rect_palette(p, largeur_totale, h);
            let decalage = mouvement::Decalage { opacite: 1.0, ..d_palette };
            self.avec_decalage(c, &decalage, (largeur_totale / 2.0, h / 2.0), plein, || {
                self.avec_revelation(c, zone, d_palette.opacite, || self.dessiner_palette(c, p, largeur_totale, h));
            });
        } else if let Some((revele, dy)) = fermeture_palette {
            let derniere = self.derniere_palette.borrow();
            if let Some(p) = derniere.as_ref() {
                let zone = Self::rect_palette(p, largeur_totale, h);
                let decalage = mouvement::Decalage { dx: 0.0, dy, echelle: 1.0, opacite: 1.0 };
                self.avec_decalage(c, &decalage, (largeur_totale / 2.0, h / 2.0), plein, || {
                    self.avec_revelation(c, zone, revele, || self.dessiner_palette(c, p, largeur_totale, h));
                });
            }
        } else if self.derniere_palette.borrow().is_some() {
            *self.derniere_palette.borrow_mut() = None;
        }
        if vue.pixel.place > 0.0 {
            self.dessiner_pixel(c, &vue.pixel, l, h);
        }
        self.zones_conflit.borrow_mut().clear();
        // Lot 12D : la carte des conflits part de la première carte d'opération (l. 582) ; contenu caché pendant le dépliage
        let cible_conflit = Self::rect_conflit(largeur_totale, h);
        let source_conflit = match self.zones_pastilles.borrow().places.first() {
            Some(p) => [p.zone.x, p.zone.y, p.zone.l, p.zone.h],
            None => [largeur_totale / 2.0 - 100.0, h - STATUT_H - 8.0 - pastilles::HAUTEUR, 200.0, pastilles::HAUTEUR],
        };
        let etat_conflit = self.mouvements.borrow_mut().morph_conflit.maj(vue.conflits.is_some(), source_conflit, cible_conflit, t_image, animer_cadres);
        if etat_conflit.visible {
            if let Some(cadre) = etat_conflit.cadre {
                self.couleur(c, &rgba(0, 0, 0, 0.6 * etat_conflit.voile));
                rt.FillRectangle(&rect(0.0, HAUTEUR_TITRE + 1.0, largeur_totale, h - HAUTEUR_TITRE - 1.0), &c.pinceau);
                self.carte(c, rect(cadre[0], cadre[1], cadre[2], cadre[3]));
            } else if let Some(cf) = vue.conflits {
                if etat_conflit.contenu < 0.999 {
                    self.dessiner_conflits(c, cf, largeur_totale, h, true, false);
                    let d = mouvement::Decalage { dx: 0.0, dy: 0.0, echelle: 1.0, opacite: etat_conflit.contenu };
                    self.avec_decalage(c, &d, (largeur_totale / 2.0, h / 2.0), plein, || self.dessiner_conflits(c, cf, largeur_totale, h, false, true));
                } else {
                    self.dessiner_conflits(c, cf, largeur_totale, h, true, true);
                }
            }
        }
        let d_menu = self.decalage_apparition(mouvement::APPARU_MENU, vue.menu.is_some(), mouvement::APPARU_MENU_MS, -4.0, 0.94);
        if let Some(m) = vue.menu {
            let centre = m.niveaux.first().map_or((0.0, 0.0), |n| {
                let k = n.cadre();
                ((k.left + k.right) / 2.0, (k.top + k.bottom) / 2.0)
            });
            self.avec_decalage(c, &d_menu, centre, plein, || self.dessiner_menu(c, m));
        }
        // Reflet : une seule passe, par-dessus tout ce qui est déjà tracé
        self.dessiner_reflet(c, app, l, h, vue);
        if calque.is_some() {
            rt.PopLayer();
            rt.SetTransform(&Matrix3x2::identity());
        }
        self.mouvements.borrow_mut().fin_image(t_image);
    }

    /// Pose la transformation et le calque de l'ouverture / fermeture ; `None` si le calque n'a pas pu être créé (rien à dépiler,
    /// la fenêtre se dessine alors normalement). Invariant (unsafe) : entre `BeginDraw` et `EndDraw`, fil d'interface ; le masque
    /// géométrique est prêté à D2D le temps de l'appel puis relâché ici.
    unsafe fn ouvrir_calque_fenetre(&self, c: &Cible2D, e: EtatFenetre, l: f32, h: f32) -> Option<ID2D1Layer> {
        let rt = &c.rt;
        let calque = rt.CreateLayer(None).ok()?;
        let masque: ID2D1Geometry = self.d2d.CreateRoundedRectangleGeometry(&D2D1_ROUNDED_RECT { rect: rect(0.0, 0.0, l, h), radiusX: e.rayon, radiusY: e.rayon }).ok()?.cast().ok()?;
        // Échelle autour du bas-centre (`transform-origin: 50% 100%`), puis translation vers le bas
        rt.SetTransform(&Matrix3x2 { M11: e.echelle, M12: 0.0, M21: 0.0, M22: e.echelle, M31: l / 2.0 * (1.0 - e.echelle), M32: h * (1.0 - e.echelle) + e.decalage });
        let params = D2D1_LAYER_PARAMETERS {
            contentBounds: D2D_RECT_F { left: f32::MIN, top: f32::MIN, right: f32::MAX, bottom: f32::MAX },
            geometricMask: std::mem::ManuallyDrop::new(Some(masque)),
            maskAntialiasMode: D2D1_ANTIALIAS_MODE_PER_PRIMITIVE,
            maskTransform: Matrix3x2::identity(),
            opacity: e.opacite,
            ..Default::default()
        };
        rt.PushLayer(&params, &calque);
        // Le masque a été copié par D2D (compté) : on relâche notre référence
        drop(std::mem::ManuallyDrop::into_inner(params.geometricMask));
        Some(calque)
    }

    /// Élément éclairé par le reflet et rayon de coin de son fond : celui de `survol`, ou, sous une fenêtre modale, l'élément de la
    /// modale sous le pointeur (menu, boutons de la carte des conflits). Les rectangles sont ceux que le dessin et les clics
    /// utilisent déjà (zones du volet, de la barre, des pastilles, des onglets, du menu) ; seuls `disposition_volet` (déjà
    /// calculée à chaque image) et les positions de la page des réglages sont relus, et seulement pour l'élément survolé.
    fn cible_du_reflet(&self, app: &App, l: f32, h: f32, vue: &Vue) -> Option<CibleReflet> {
        let libre = |zone: D2D_RECT_F, coin: f32| Some(CibleReflet { zone, coin, desactive: false, clip: None });
        // Menu ouvert : l'élément allumé du niveau le plus profond (`survol` vient de la souris, une touche efface le pointeur).
        // Indépendant du pointeur : la zone sert aussi de candidate à l'invalidation quand le pointeur y rentre sans que `survol` change.
        if let Some(m) = vue.menu {
            let n = m.niveaux.iter().rev().find(|n| n.survol.is_some() || n.icone.is_some())?;
            if let Some(j) = n.icone {
                let k = n.lignes.iter().position(|l| matches!(l.genre, menu::Genre::Icones(_)))?;
                let menu::Genre::Icones(v) = &n.lignes[k].genre else { return None };
                return Some(CibleReflet { zone: n.case_icone(k, j), coin: 8.0, desactive: !v.get(j)?.2, clip: None });
            }
            return libre(n.zone(n.survol?), 6.0);
        }
        if vue.conflits.is_some() {
            let (px, py) = vue.pointeur?;
            return self.zones_conflit.borrow().iter().find(|(r, _)| reflet::contient(r, px, py)).and_then(|(r, _)| libre(*r, 8.0));
        }
        if vue.palette.is_some() || vue.lot.is_some() || vue.galerie.is_some() {
            return None;
        }
        let ll = largeur_liste(l, vue.apercu_visible);
        let lp = largeur_panneau(ll, vue.double);
        // Double panneau : le panneau de droite est dessiné décalé, comme `dessiner_tout` le fait par une translation
        let decalage = if vue.double && vue.focus_droite { lp - largeur_volet() } else { 0.0 };
        let decale = |r: D2D_RECT_F| D2D_RECT_F { left: r.left + decalage, right: r.right + decalage, ..r };
        let zone_pastille = |z: pastilles::Zone| rect(z.x, z.y, z.l, z.h);
        match app.survol {
            Cible::Onglet(k) | Cible::FermerOnglet(k) => zones_onglets(l, vue.onglets.len()).into_iter().find(|(q, _)| *q == Cible::Onglet(k)).and_then(|(_, r)| libre(r, 8.0)),
            Cible::NouvelOnglet => zones_onglets(l, vue.onglets.len()).into_iter().find(|(q, _)| *q == Cible::NouvelOnglet).and_then(|(_, r)| libre(r, 8.0)),
            Cible::CePc | Cible::Favori(_) | Cible::Epingle(_) | Cible::Disque(_) | Cible::Nuage(_) | Cible::Serveur(_) | Cible::Groupe(_) | Cible::Arbre(_) => {
                // Le volet est rogné au-dessus du bouton des réglages : le reflet l'est aussi
                let bas = h - 52.0;
                let r = disposition_volet(app).1.into_iter().find(|(q, _)| *q == app.survol)?.1;
                Some(CibleReflet { zone: r, coin: 7.0, desactive: false, clip: Some(rect(0.0, HAUTEUR_TITRE + 1.0, largeur_volet(), bas - HAUTEUR_TITRE - 1.0)) })
            }
            Cible::Reglages => libre(zone_reglages_haut(l), 8.0),
            Cible::ReglagesFermer => libre(page::zone_fermer(l), 8.0),
            Cible::ReglagesNav(k) => page::zones_nav().get(k).and_then(|(_, r)| libre(*r, REGLAGE_SEG_RAYON)),
            Cible::RechercheHaut => libre(zone_recherche_haut(l), 8.0),
            Cible::EssentielHaut => libre(zone_essentiel_haut(l), 8.0),
            Cible::Terminal => libre(zone_terminal(l), 8.0),
            Cible::TerminalFermer => libre(zone_terminal_fermer(ll, h), 8.0),
            Cible::TerminalExterne => Some(CibleReflet { zone: zone_terminal_action(ll, h), coin: 8.0, desactive: app.dossier_ici().is_none(), clip: None }),
            Cible::Precedent | Cible::Suivant | Cible::Parent => {
                let possible = match app.survol {
                    Cible::Precedent => app.peut_reculer(),
                    Cible::Suivant => app.peut_avancer(),
                    _ => app.emplacement.parent().is_some(),
                };
                let r = boutons_nav().into_iter().find(|(q, _)| *q == app.survol)?.1;
                Some(CibleReflet { zone: decale(r), coin: 8.0, desactive: !possible, clip: None })
            }
            Cible::Outil(o) => {
                let p = app.zones.barre.iter().find(|p| p.outil == o)?;
                Some(CibleReflet { zone: decale(p.zone), coin: o.rayon_coin(), desactive: !app.outil_possible(o), clip: None })
            }
            Cible::Note => libre(decale(app.zones.note?), 11.0),
            Cible::Puce(k) => libre(decale(*app.zones.puces.get(k)?), 8.0),
            Cible::Corbeille => libre(decale(app.zones.corbeille?), 10.0),
            Cible::Reglage(k) => {
                let vp = vue.page.as_ref()?;
                let (y0, hl) = *page::positions(vp.lignes, vp.page.categorie).get(k)?;
                if hl == 0.0 {
                    return None;
                }
                let (haut, bas) = (page::PAGE_Y, h - STATUT_H);
                let y = haut + y0 - vp.page.defilement;
                let clip = rect(REGLAGES_NAV + 2.0, haut, lp - REGLAGES_NAV - 2.0, bas - haut);
                Some(CibleReflet { zone: page::rect_carte(y, lp), coin: REGLAGE_CARTE_RAYON, desactive: false, clip: Some(clip) })
            }
            Cible::Champ(k) => {
                // Seuls le sélecteur de genre et les deux boutons sont des boutons ; les champs de saisie ne sont pas éclairés
                vue.page.as_ref()?.page.formulaire.as_ref()?;
                let carte = page::carte_formulaire(lp, h);
                let z = page::zones_formulaire(carte).into_iter().find(|(q, _)| *q == k)?.1;
                if k == page::F_GENRE {
                    libre(decale(rect(z.left + 150.0, z.top + 2.0, z.right - z.left - 150.0, z.bottom - z.top - 4.0)), 7.0)
                } else if k >= page::F_AJOUTER {
                    libre(decale(z), 19.0)
                } else {
                    None
                }
            }
            Cible::Pastille(i) => {
                let d = self.zones_pastilles.borrow();
                libre(zone_pastille(d.places.iter().find(|p| p.index == i)?.zone), pastilles::RAYON)
            }
            Cible::BoutonPastille(i, b) => {
                let d = self.zones_pastilles.borrow();
                let z = d.places.iter().find(|p| p.index == i)?.boutons.iter().find(|(q, _, _)| *q == b).map(|(_, z, _)| *z)?;
                libre(zone_pastille(z), if b == Bouton::Destination { 8.0 } else { 10.0 })
            }
            Cible::PlusPastilles => libre(zone_pastille(self.zones_pastilles.borrow().plus?.1), pastilles::RAYON),
            _ => None,
        }
    }

    /// Reflet qui suit le curseur : halo de fond puis lueur du contour de l'élément survolé, avec le pinceau radial partagé.
    /// Rien si le reflet est coupé (mouvement réduit, Contraste élevé), si l'élément est désactivé ou trop grand, ou si le pointeur
    /// est inconnu (voir `reflet.rs`). `zone_reflet` garde ce qui a été éclairé pour l'invalidation de la souris.
    /// Invariant (unsafe) : appelée entre `BeginDraw` et `EndDraw` de `c.rt`, sur le fil d'interface ; le pinceau `c.reflet` vit
    /// avec la cible et n'est modifié (centre, rayon, opacité) que d'ici, jamais recréé.
    unsafe fn dessiner_reflet(&self, c: &Cible2D, app: &App, l: f32, h: f32, vue: &Vue) {
        let actif = reflet::actif_maintenant();
        let cible = if actif { self.cible_du_reflet(app, l, h, vue) } else { None };
        // Candidate : l'élément éclairable même si le pointeur n'est pas (encore) dedans ; la souris l'invalide aussi, sinon le halo
        // n'apparaîtrait pas quand le pointeur y entre sans que la cible change. Carte des conflits : la bande de ses boutons.
        let candidate = if actif && vue.menu.is_none() && vue.conflits.is_some() {
            self.zones_conflit.borrow().iter().map(|(r, _)| *r).reduce(|a, b| D2D_RECT_F { left: a.left.min(b.left), top: a.top.min(b.top), right: a.right.max(b.right), bottom: a.bottom.max(b.bottom) })
        } else {
            cible.as_ref().filter(|z| reflet::eligible(&z.zone, z.desactive)).map(|z| z.zone)
        };
        self.zone_candidate.set(candidate);
        let plan = cible.and_then(|z| reflet::planifier(z.zone, z.coin, z.desactive, vue.pointeur, true).map(|p| (p, z.clip)));
        self.zone_reflet.set(plan.as_ref().map(|(p, _)| p.zone));
        // Lot 12C (`.rv:after { transition: opacity .18s }`) : le reflet apparaît et disparaît en 180 ms ; celui qu'on vient de quitter
        // reste à sa dernière place le temps de s'éteindre
        let a_dessiner = self.mouvements.borrow_mut().reflets.maj(plan, self.t_image.get(), crate::reglages::animations_actives());
        let rt = &c.rt;
        let pinceau = &c.reflet;
        for (p, clip, k) in a_dessiner {
            if let Some(r) = &clip {
                rt.PushAxisAlignedClip(r, D2D1_ANTIALIAS_MODE_ALIASED);
            }
            pinceau.SetCenter(D2D_POINT_2F { x: p.centre.0, y: p.centre.1 });
            pinceau.SetGradientOriginOffset(D2D_POINT_2F { x: 0.0, y: 0.0 });
            // Calque 1 : halo dans le fond, découpé au rectangle arrondi de l'élément (le remplissage a cette forme)
            pinceau.SetRadiusX(p.rayon_halo);
            pinceau.SetRadiusY(p.rayon_halo);
            pinceau.SetOpacity(crate::theme::sans_renfort(&REFLET_FOND).a * k);
            rt.FillRoundedRectangle(&D2D1_ROUNDED_RECT { rect: p.zone, radiusX: p.rayon_coin, radiusY: p.rayon_coin }, pinceau);
            // Calque 2 : lueur sur le contour (1 px, tracé au milieu du pixel comme les autres traits). Garde : curseur à 90 DIP ou plus
            // du trait, la lueur serait invisible (jamais le cas d'un élément éligible, 160 DIP de haut au plus, mais le plan le dit)
            if p.alpha_trait_maximal() > 0.0 {
                pinceau.SetRadiusX(p.rayon_lueur);
                pinceau.SetRadiusY(p.rayon_lueur);
                pinceau.SetOpacity(crate::theme::sans_renfort(&REFLET_TRAIT).a * k);
                let z = p.zone;
                let coin = (p.rayon_coin - 0.5).max(0.0);
                let contour = D2D1_ROUNDED_RECT { rect: rect(z.left + 0.5, z.top + 0.5, z.right - z.left - 1.0, z.bottom - z.top - 1.0), radiusX: coin, radiusY: coin };
                rt.DrawRoundedRectangle(&contour, pinceau, 1.0, None);
            }
            if clip.is_some() {
                rt.PopAxisAlignedClip();
            }
        }
    }

    /// En-tête d'une section du regroupement : chevron, titre, nombre, pointillés jusqu'au bout
    #[allow(clippy::too_many_arguments)]
    unsafe fn dessiner_section(&self, c: &Cible2D, app: &App, s: usize, x0: f32, y: f32, largeur: f32, x_nom: f32) {
        let rt = &c.rt;
        let Some(sec) = app.sections.get(s) else { return };
        let survol = app.survol == Cible::Section(s);
        let fondu = self.survol_k(Cible::Section(s));
        if fondu > 0.0 {
            self.couleur_k(c, &SURVOL, fondu);
            rt.FillRoundedRectangle(&D2D1_ROUNDED_RECT { rect: rect(x0, y + 1.0, largeur, hauteur_ligne() - 2.0), radiusX: 7.0, radiusY: 7.0 }, &c.pinceau);
        }
        let cy = y + hauteur_ligne() / 2.0;
        // Chevron : vers le bas ouvert, vers la droite replié
        self.couleur(c, if survol { &ACCENT } else { &BLANC_DOUX });
        let motif: &[(i32, i32)] = if sec.replie { &[(-1, -2), (0, -1), (1, 0), (0, 1), (-1, 2)] } else { &[(-2, -1), (-1, 0), (0, 1), (1, 0), (2, -1)] };
        self.points(c, x_nom + 10.0, cy, 2.2, 0.9, motif);
        let titre = sec.titre.as_str();
        self.couleur(c, &BLANC);
        let wt = self.mesurer(titre, &self.f.petit);
        self.texte(c, &self.f.petit, titre, rect(x_nom + 28.0, y, wt + 4.0, hauteur_ligne()));
        let nombre = crate::systeme::nombre(sec.nombre);
        let wn = self.mesurer(&nombre, &self.f.petit);
        self.couleur(c, &ACCENT);
        self.texte(c, &self.f.petit, &nombre, rect(x_nom + 28.0 + wt + 12.0, y, wn + 4.0, hauteur_ligne()));
        // Pointillés jusqu'au bout de la ligne
        self.couleur(c, &BLANC_TRES_FAIBLE);
        let mut x = x_nom + 28.0 + wt + 12.0 + wn + 14.0;
        while x < x0 + largeur - 10.0 {
            rt.FillEllipse(&ellipse(x, cy, 0.9), &c.pinceau);
            x += 6.0;
        }
        if survol {
            self.couleur(c, &BLANC_DOUX);
            let aide = if sec.replie { "CLIC : TOUT CHOISIR · CHEVRON : DEPLIER" } else { "CLIC : TOUT CHOISIR · CHEVRON : REPLIER" };
            let wa = self.mesurer(aide, &self.f.petit) + 20.0;
            self.couleur(c, &rgba(16, 16, 16, 0.95));
            rt.FillRectangle(&rect(x0 + largeur - wa - 8.0, y + 6.0, wa, hauteur_ligne() - 12.0), &c.pinceau);
            self.couleur(c, &BLANC_DOUX);
            self.texte(c, &self.f.petit_droite, aide, rect(x0, y, largeur - 18.0, hauteur_ligne()));
        }
    }

    /// Menu Nothing : carte en verre sombre, lignes en Doto, point rouge pour l'état en cours,
    /// séparateurs en pointillés, sous-menus à droite
    unsafe fn dessiner_menu(&self, c: &Cible2D, m: &Menu) {
        let rt = &c.rt;
        let f = &self.f;
        for (i, n) in m.niveaux.iter().enumerate() {
            let cadre = n.cadre();
            // Ombre légère, puis la carte
            self.couleur(c, &rgba(0, 0, 0, 0.35));
            rt.FillRoundedRectangle(&D2D1_ROUNDED_RECT { rect: rect(cadre.left + 2.0, cadre.top + 4.0, cadre.right - cadre.left, cadre.bottom - cadre.top), radiusX: 10.0, radiusY: 10.0 }, &c.pinceau);
            let arrondi = D2D1_ROUNDED_RECT { rect: cadre, radiusX: 10.0, radiusY: 10.0 };
            self.couleur(c, &rgba(16, 16, 16, 0.98));
            rt.FillRoundedRectangle(&arrondi, &c.pinceau);
            self.trame_arrondie(c, &arrondi);
            self.couleur(c, &BLANC_TRES_FAIBLE);
            rt.DrawRoundedRectangle(&arrondi, &c.pinceau, 1.0, None);
            // Ligne du parent qui a ouvert ce sous-menu : reste allumée
            let ouvert = m.niveaux.get(i + 1).map(|s| s.origine);
            for (k, l) in n.lignes.iter().enumerate() {
                let z = n.zone(k);
                if let menu::Genre::Separation = l.genre {
                    self.couleur(c, &BLANC_TRES_FAIBLE);
                    let mut x = z.left + 8.0;
                    let y = (z.top + z.bottom) / 2.0;
                    while x < z.right - 8.0 {
                        rt.FillEllipse(&ellipse(x, y, 0.9), &c.pinceau);
                        x += 5.0;
                    }
                    continue;
                }
                // Rangée d'icônes : points comme la barre d'actions, nom de l'icône survolée dessous
                if let menu::Genre::Icones(v) = &l.genre {
                    for (j, (o, _, possible)) in v.iter().enumerate() {
                        let r = n.case_icone(k, j);
                        let survolee = n.icone == Some(j) && *possible;
                        if survolee {
                            self.couleur(c, &SURVOL_FORT);
                            rt.FillRoundedRectangle(&D2D1_ROUNDED_RECT { rect: r, radiusX: 8.0, radiusY: 8.0 }, &c.pinceau);
                        }
                        let couleur = if !possible { &BLANC_TRES_FAIBLE } else if survolee { &BLANC } else { &BLANC_DOUX };
                        self.couleur(c, couleur);
                        self.points(c, (r.left + r.right) / 2.0, r.top + 16.0, 2.5, 0.95, o.motif());
                        // Libellé court sous l'icône (« COPIER · CTRL+C » → « COPIER »)
                        let nom = o.bulle().split(" · ").next().unwrap_or("");
                        self.couleur(c, if !possible { &BLANC_TRES_FAIBLE } else if survolee { &BLANC } else { &BLANC_DOUX });
                        self.texte(c, &f.petit_centre, nom, rect(r.left - 4.0, r.top + 30.0, r.right - r.left + 8.0, 18.0));
                    }
                    continue;
                }
                let allume = n.survol == Some(k) || ouvert == Some(k);
                if allume {
                    self.couleur(c, &SURVOL);
                    rt.FillRoundedRectangle(&D2D1_ROUNDED_RECT { rect: z, radiusX: 6.0, radiusY: 6.0 }, &c.pinceau);
                }
                if l.coche {
                    self.couleur(c, &ACCENT);
                    rt.FillEllipse(&ellipse(z.left + 13.0, (z.top + z.bottom) / 2.0, 3.0), &c.pinceau);
                }
                let plus = matches!(l.genre, menu::Genre::PlusOptions);
                self.couleur(c, if !l.actif { &BLANC_TRES_FAIBLE } else if plus { &BLANC_DOUX } else { &BLANC });
                self.texte(c, &f.petit, &l.libelle, rect(z.left + 28.0, z.top + 6.0, z.right - z.left - 40.0, z.bottom - z.top - 6.0));
                if !l.raccourci.is_empty() {
                    // Le libellé garde sa place : le raccourci n'est dessiné que s'il tient à droite de lui
                    let x_min = z.left + 28.0 + self.mesurer(&l.libelle, &f.petit) + 12.0;
                    // Centre du libellé (sa zone commence 6 DIP plus bas que la ligne), pour que touches et texte s'alignent
                    self.touches_droite(c, &l.raccourci, z.right - 12.0, z.top + 6.0 + (z.bottom - z.top - 6.0) / 2.0, x_min, l.actif);
                }
                if let menu::Genre::Sous(_) = l.genre {
                    self.couleur(c, if allume { &ACCENT } else { &BLANC_DOUX });
                    self.points(c, z.right - 14.0, (z.top + z.bottom) / 2.0, 2.6, 1.0, &[(-1, -1), (0, 0), (-1, 1)]);
                }
            }
        }
    }

    /// Navigation, chemin, liste (ou grille, colonnes, page des réglages) et barre d'état d'un panneau
    /// Barre d'actions rapides : icônes en points, libellés courts, vue en cours en rouge
    unsafe fn dessiner_barre(&self, c: &Cible2D, app: &App, vue: &Vue, double: bool) {
        let rt = &c.rt;
        // Les trois vues forment un seul groupe arrondi (maquette `.tgrp`), pas trois boutons séparés
        let vues: Vec<&crate::barre::Place> = app.zones.barre.iter().filter(|p| matches!(p.outil, Outil::VueListe | Outil::VueGrille | Outil::VueColonnes | Outil::VueTailles | Outil::Double)).collect();
        if let (Some(premier), Some(dernier)) = (vues.first(), vues.last()) {
            let zone = rect(premier.zone.left - 4.0, premier.zone.top - 4.0, dernier.zone.right - premier.zone.left + 8.0, premier.zone.bottom - premier.zone.top + 8.0);
            self.couleur(c, &PANNEAU);
            rt.FillRoundedRectangle(&D2D1_ROUNDED_RECT { rect: zone, radiusX: 18.0, radiusY: 18.0 }, &c.pinceau);
            self.couleur(c, &LIGNE);
            rt.DrawRoundedRectangle(&D2D1_ROUNDED_RECT { rect: zone, radiusX: 18.0, radiusY: 18.0 }, &c.pinceau, 1.0, None);
        }
        if let [Some(confort), Some(compact)] = app.zones.densite {
            let cadre = rect(confort.left - 3.0, confort.top - 4.0, compact.right - confort.left + 6.0, compact.bottom - confort.top + 8.0);
            self.couleur(c, &PANNEAU);
            rt.FillRoundedRectangle(&D2D1_ROUNDED_RECT { rect: cadre, radiusX: 18.0, radiusY: 18.0 }, &c.pinceau);
            self.couleur(c, &LIGNE);
            rt.DrawRoundedRectangle(&D2D1_ROUNDED_RECT { rect: cadre, radiusX: 18.0, radiusY: 18.0 }, &c.pinceau, 1.0, None);
            for (k, r) in [confort, compact].into_iter().enumerate() {
                let r = decaler(r, decalage_chrome(app, vue, Cible::Densite(k)));
                let actif = (k == 0 && crate::reglages::lignes_confort()) || (k == 1 && !crate::reglages::lignes_confort());
                let survole = app.survol == Cible::Densite(k);
                let fondu = self.survol_k(Cible::Densite(k));
                if actif || fondu > 0.0 {
                    if actif { self.couleur(c, &SURVOL_FORT) } else { self.couleur_k(c, &SURVOL, fondu) }
                    rt.FillRoundedRectangle(&D2D1_ROUNDED_RECT { rect: r, radiusX: 15.0, radiusY: 15.0 }, &c.pinceau);
                }
                self.couleur(c, if actif || survole { &BLANC } else { &BLANC_DOUX });
                self.texte(c, &self.f.petit_centre, if k == 0 { "CONFORT" } else { "COMPACT" }, r);
            }
        }
        // Indicateur de vue (lot 12A) : le fond de l'outil actif glisse entre les trois cases (liste, grille, colonnes)
        let glisse = self.fond_vue_glissant(c, app, vue);
        for p in &app.zones.barre {
            let o = p.outil;
            let r = decaler(p.zone, decalage_chrome(app, vue, Cible::Outil(o)));
            let possible = app.outil_possible(o);
            let allume = app.outil_allume(o) || (o == Outil::Double && double);
            let survole = possible && (app.survol == Cible::Outil(o) || app.menu_barre == Some(o));
            // Fondu du fond de survol (160 ms) ; un menu de la barre ouvert garde l'outil allumé
            let fondu = if !possible { 0.0 } else if app.menu_barre == Some(o) { 1.0 } else { self.survol_k(Cible::Outil(o)) };
            if o == Outil::Nouveau {
                // Pilule « + NOUVEAU », contour discret, comme la maquette
                let arrondi = D2D1_ROUNDED_RECT { rect: r, radiusX: o.rayon_coin(), radiusY: o.rayon_coin() };
                self.couleur(c, &D2D1_COLOR_F { a: mouvement::melanger(SURVOL.a, SURVOL_FORT.a, fondu), ..SURVOL });
                rt.FillRoundedRectangle(&arrondi, &c.pinceau);
                self.couleur(c, &BLANC_TRES_FAIBLE);
                rt.DrawRoundedRectangle(&arrondi, &c.pinceau, 1.0, None);
                self.couleur(c, if survole { &BLANC } else { &BLANC_DOUX });
                self.points(c, r.left + 18.0, (r.top + r.bottom) / 2.0, 5.0, 1.6, o.motif());
                self.couleur(c, &BLANC);
                self.texte(c, &self.f.petit, "NOUVEAU", rect(r.left + 32.0, r.top, r.right - r.left - 48.0, r.bottom - r.top));
                self.couleur(c, &BLANC_DOUX);
                self.points(c, r.right - 10.0, (r.top + r.bottom) / 2.0, 2.0, 0.8, &[(-1, 0), (0, 1), (1, 0)]);
                continue;
            }
            if p.separe {
                self.couleur(c, &LIGNE);
                rt.FillRectangle(&rect(r.left - 9.0, r.top + 9.0, 1.0, r.bottom - r.top - 18.0), &c.pinceau);
            }
            let arrondi = D2D1_ROUNDED_RECT { rect: r, radiusX: o.rayon_coin(), radiusY: o.rayon_coin() };
            // Un outil de vue allumé n'a pas de fond propre quand le fond glissant le porte
            let fond_allume = allume && !(glisse && Self::OUTILS_VUE.contains(&o));
            if fondu > 0.0 || fond_allume {
                let alpha = if fond_allume { mouvement::melanger(SURVOL.a, SURVOL_FORT.a, fondu) } else { SURVOL_FORT.a * fondu };
                self.couleur(c, &D2D1_COLOR_F { a: alpha, ..SURVOL_FORT });
                rt.FillRoundedRectangle(&arrondi, &c.pinceau);
            }
            let couleur = if !possible {
                &BLANC_TRES_FAIBLE
            } else if allume {
                &ACCENT
            } else if survole {
                &BLANC
            } else {
                &BLANC_DOUX
            };
            let cy = (r.top + r.bottom) / 2.0;
            let menu = o.a_menu() && o != Outil::Plus;
            let cx = if menu { r.left + 15.0 } else { (r.left + r.right) / 2.0 };
            self.couleur(c, couleur);
            if o == Outil::Plus {
                self.points(c, cx, cy, 5.0, 1.6, o.motif());
            } else {
                self.points(c, cx, cy, 2.5, 0.95, o.motif());
            }
            if menu {
                if p.texte {
                    self.couleur(c, if !possible { &BLANC_TRES_FAIBLE } else if survole { &BLANC } else { &BLANC_DOUX });
                    self.texte(c, &self.f.petit, o.libelle(), rect(r.left + 29.0, r.top, r.right - r.left - 29.0 - 14.0, r.bottom - r.top));
                }
                self.couleur(c, if survole { &BLANC } else { &BLANC_DOUX });
                self.points(c, r.right - 9.0, cy, 2.0, 0.8, &[(-1, 0), (0, 1), (1, 0)]);
            }
        }
    }

    const OUTILS_VUE: [Outil; 3] = [Outil::VueListe, Outil::VueGrille, Outil::VueColonnes];

    /// Dessine le fond de l'outil de vue actif à sa position glissante (450 ms, ressort). Faux : pas de fond glissant (une des
    /// trois cases manque, ou la vue active n'en fait pas partie), les outils se dessinent comme avant.
    unsafe fn fond_vue_glissant(&self, c: &Cible2D, app: &App, vue: &Vue) -> bool {
        let zones: Vec<D2D_RECT_F> = Self::OUTILS_VUE.iter().filter_map(|o| app.zones.barre.iter().find(|p| p.outil == *o).map(|p| p.zone)).collect();
        let actif = Self::OUTILS_VUE.iter().position(|o| app.outil_allume(*o));
        let mut m = self.mouvements.borrow_mut();
        let (Some(k), true) = (actif, zones.len() == Self::OUTILS_VUE.len()) else {
            m.vue.oublier();
            return false;
        };
        let t = self.t_image.get();
        m.vue.viser(k as f32, t, mouvement::VUE_MS, !crate::reglages::animations_actives());
        let bornes: Vec<(f32, f32)> = zones.iter().map(|z| (z.left, z.right)).collect();
        let Some((gauche, droite)) = mouvement::interpoler(&bornes, m.vue.valeur(t)) else { return false };
        let dy = decalage_chrome(app, vue, Cible::Outil(Self::OUTILS_VUE[k]));
        let z = decaler(zones[k], dy);
        self.couleur(c, &SURVOL);
        c.rt.FillRoundedRectangle(&D2D1_ROUNDED_RECT { rect: D2D_RECT_F { left: gauche, right: droite, ..z }, radiusX: 10.0, radiusY: 10.0 }, &c.pinceau);
        true
    }

    /// Bulle sous le bouton survolé de la barre : nom et raccourci
    unsafe fn dessiner_bulle(&self, c: &Cible2D, app: &App, l: f32) {
        let Cible::Outil(o) = app.survol else { return };
        if app.menu_barre.is_some() {
            return;
        }
        let Some(p) = app.zones.barre.iter().find(|p| p.outil == o) else { return };
        // Lot 12C (`[data-tip]`) : l'infobulle paraît après 500 ms de survol, 240 ms au ressort depuis 5 DIP plus haut
        let Some((opacite, dy)) = self.mouvements.borrow().survols.bulle(self.t_image.get()) else { return };
        let texte = o.bulle();
        let w = self.largeur_aide(&segmenter_aide(texte)) + 24.0;
        let x = ((p.zone.left + p.zone.right) / 2.0 - w / 2.0).clamp(largeur_volet() + 8.0, (l - w - 8.0).max(largeur_volet() + 8.0));
        let r = rect(x, p.zone.bottom + 6.0, w, 26.0);
        let arrondi = D2D1_ROUNDED_RECT { rect: r, radiusX: 8.0, radiusY: 8.0 };
        let d = mouvement::Decalage { dx: 0.0, dy, echelle: 1.0, opacite };
        self.avec_decalage(c, &d, ((r.left + r.right) / 2.0, (r.top + r.bottom) / 2.0), rect(r.left - 12.0, r.top - 12.0, w + 24.0, 50.0), || {
            self.couleur(c, &CHAMP_FOND);
            c.rt.FillRoundedRectangle(&arrondi, &c.pinceau);
            self.couleur(c, &BLANC_TRES_FAIBLE);
            c.rt.DrawRoundedRectangle(&arrondi, &c.pinceau, 1.0, None);
            self.aide(c, texte, rect(r.left + 12.0, r.top, w - 18.0, 26.0), Pose::Gauche, if app.outil_possible(o) { &BLANC } else { &BLANC_FAIBLE });
        });
    }

    unsafe fn dessiner_contenu(&self, c: &Cible2D, app: &App, l: f32, h: f32, vue: &Vue, focus: bool, premier: bool) {
        let rt = &c.rt;
        let f = &self.f;
        let rang = self.panneau.get().min(1);
        self.panneau.set(rang + 1);
        self.panneau_en_cours.set(rang);
        // Double panneau : un trait rouge en haut du panneau qui a le focus
        if vue.double && focus {
            self.couleur(c, &ACCENT);
            rt.FillRectangle(&rect(largeur_volet() + 1.0, HAUTEUR_TITRE + 1.0, l - largeur_volet() - 1.0, 2.0), &c.pinceau);
        }

        // ── Navigation : précédent, suivant, parent ──
        for (quoi, r) in boutons_nav() {
            let r = decaler(r, decalage_chrome(app, vue, quoi));
            let possible = match quoi {
                Cible::Precedent => app.peut_reculer(),
                Cible::Suivant => app.peut_avancer(),
                _ => app.emplacement.parent().is_some(),
            };
            let fondu = if possible { self.survol_k(quoi) } else { 0.0 };
            if fondu > 0.0 {
                self.couleur_k(c, &SURVOL, fondu);
                rt.FillRoundedRectangle(&D2D1_ROUNDED_RECT { rect: r, radiusX: 8.0, radiusY: 8.0 }, &c.pinceau);
            }
            self.couleur(c, if !possible { &BLANC_TRES_FAIBLE } else if app.survol == quoi { &BLANC } else { &BLANC_DOUX });
            let motif: &[(i32, i32)] = match quoi {
                Cible::Precedent => &[(1, -2), (0, -1), (-1, 0), (0, 1), (1, 2)],
                Cible::Suivant => &[(-1, -2), (0, -1), (1, 0), (0, 1), (-1, 2)],
                _ => &[(-2, 1), (-1, 0), (0, -1), (1, 0), (2, 1)],
            };
            self.points(c, (r.left + r.right) / 2.0, (r.top + r.bottom) / 2.0, 3.5, 1.4, motif);
        }

        // ── Fil d'Ariane (ou le chemin en cours de saisie) ──
        if let Some(ed) = &app.saisie_chemin {
            let r = rect(chemin_x() - 10.0, NAV_Y + 3.0, (l - MARGE_LISTE - chemin_x() + 10.0).max(80.0), NAV_H - 6.0);
            let arrondi = D2D1_ROUNDED_RECT { rect: r, radiusX: 8.0, radiusY: 8.0 };
            self.couleur(c, &CHAMP_FOND);
            rt.FillRoundedRectangle(&arrondi, &c.pinceau);
            self.couleur(c, &ACCENT);
            rt.DrawRoundedRectangle(&arrondi, &c.pinceau, 1.2, None);
            let aide = "TAB : COMPLETER · ENTREE : ALLER · ECHAP";
            let wa = self.largeur_aide(&segmenter_aide(aide)) + 16.0;
            let place_aide = r.right - r.left > wa + 260.0;
            if place_aide {
                self.aide(c, aide, rect(r.left, r.top, r.right - r.left - 12.0, r.bottom - r.top), Pose::Droite, &BLANC_TRES_FAIBLE);
            }
            self.champ(c, ed, rect(r.left + 10.0, r.top, r.right - r.left - 20.0 - if place_aide { wa } else { 0.0 }, r.bottom - r.top));
        } else if app.survol == Cible::Chemin {
            // Vide du chemin survolé : il se clique pour taper un chemin
            let fin = app.zones.segments.last().map_or(chemin_x(), |s| s.0.right);
            self.couleur(c, &SURVOL);
            rt.FillRoundedRectangle(&D2D1_ROUNDED_RECT { rect: rect(chemin_x() - 10.0, NAV_Y + 3.0, (l - MARGE_LISTE - chemin_x() + 10.0).max(80.0), NAV_H - 6.0), radiusX: 8.0, radiusY: 8.0 }, &c.pinceau);
            self.couleur(c, &BLANC_TRES_FAIBLE);
            self.texte(c, &f.petit, "CLIC : TAPER UN CHEMIN", rect(fin + 24.0, NAV_Y, (l - MARGE_LISTE - fin - 30.0).max(0.0), NAV_H));
        }
        let n = if app.saisie_chemin.is_some() { 0 } else { app.zones.segments.len() };
        for (i, (r, _, texte)) in app.zones.segments.iter().enumerate().take(n) {
            let dernier = i + 1 == n;
            let survole = app.survol == Cible::Segment(i) || app.depot == Cible::Segment(i);
            // `.crumb:hover` : fond `--hov`, rayon 10 ; dernier segment `--i1`, parents `--i2`
            if survole {
                self.couleur(c, &SURVOL);
                rt.FillRoundedRectangle(&D2D1_ROUNDED_RECT { rect: rect(r.left, r.top + 3.0, r.right - r.left, NAV_H - 6.0), radiusX: 10.0, radiusY: 10.0 }, &c.pinceau);
            }
            // Groupe ouvert (lot 14, `.gc`) : un point de la couleur choisie pour le groupe devant son nom
            let point = match &app.emplacement {
                Emplacement::Groupe(nom) if dernier => Some(Self::couleur_groupe(app.groupes.iter().find(|g| g.nom == *nom).map_or('r', |g| g.couleur))),
                _ => None,
            };
            if let Some(col) = point {
                self.couleur(c, &col);
                rt.FillEllipse(&ellipse(r.left + CRUMB_MARGE + 4.0, (r.top + r.bottom) / 2.0, 4.0), &c.pinceau);
            }
            let decal = if point.is_some() { GROUPE_POINT } else { 0.0 };
            self.couleur(c, if dernier || survole { &BLANC } else { &BLANC_DOUX });
            // +2 DIP : la mesure et le dessin arrondissent chacun de leur côté, sans marge « ÉTÉ CAFÉ » finirait en « ÉTÉ CAF… »
            self.texte_espace(c, &f.chemin, texte, rect(r.left + CRUMB_MARGE + decal, r.top, r.right - r.left - 2.0 * CRUMB_MARGE + 2.0 - decal, NAV_H));
            if !dernier {
                self.couleur(c, &BLANC_TRES_FAIBLE);
                self.texte_espace(c, &f.chemin, "›", rect(r.right + CRUMB_ECART, r.top, 40.0, NAV_H));
            }
        }

        // ── Filtre tapé au clavier, à droite du chemin ──
        // Lot 14 (`.spill`) : le champ s'ouvre par sa largeur (le contenu reste en place, rogné par le bord qui avance) et par son opacité ;
        // ouvert à vide il montre les puces `ext:` `taille>` `modif:` et la touche Échap
        if let Some((r, m, opacite)) = self.zone_filtre(app, l, rang) {
            let d = mouvement::Decalage { dx: 0.0, dy: 0.0, echelle: 1.0, opacite };
            self.avec_decalage(c, &d, ((r.left + r.right) / 2.0, (r.top + r.bottom) / 2.0), rect(r.left - 4.0, r.top - 4.0, r.right - r.left + 8.0, r.bottom - r.top + 8.0), || {
                let arrondi = D2D1_ROUNDED_RECT { rect: r, radiusX: 8.0, radiusY: 8.0 };
                self.couleur(c, &CHAMP_FOND);
                rt.FillRoundedRectangle(&arrondi, &c.pinceau);
                // Bord rouge dès qu'il y a un filtre ; ouvert à vide, le bord discret `--s2` de la maquette
                self.couleur(c, if app.filtre.is_empty() { &BLANC_TRES_FAIBLE } else { &ACCENT });
                rt.DrawRoundedRectangle(&arrondi, &c.pinceau, 1.2, None);
                rt.PushAxisAlignedClip(&r, D2D1_ANTIALIAS_MODE_ALIASED);
                // Loupe en points
                self.points(c, r.left + 15.0, (r.top + r.bottom) / 2.0 - 1.5, 2.2, 0.9, &[(0, -1), (1, -1), (-1, 0), (2, 0), (0, 1), (1, 1), (2, 2), (3, 3)]);
                let zone_texte = rect(r.left + FILTRE_GAUCHE, r.top, m.texte, r.bottom - r.top);
                if app.filtre.is_empty() {
                    self.couleur(c, &BLANC_FAIBLE);
                    self.texte(c, &f.liste, "Rechercher ici", zone_texte);
                } else {
                    self.couleur(c, &BLANC);
                    self.texte(c, &f.liste, &app.filtre, zone_texte);
                }
                let tw = self.mesurer(&app.filtre, &f.liste).min(m.texte - 4.0);
                self.couleur(c, &ACCENT);
                rt.FillRectangle(&rect(r.left + FILTRE_GAUCHE + 1.0 + tw, r.top + 8.0, 2.0, r.bottom - r.top - 16.0), &c.pinceau);
                let puces = self.placer_puces(r, &m);
                for (i, (p, jeton)) in puces.iter().zip(PUCES_FILTRE).enumerate() {
                    let fondu = self.survol_k(Cible::Puce(i));
                    if fondu > 0.0 {
                        self.couleur_k(c, &SURVOL, fondu);
                        rt.FillRoundedRectangle(&D2D1_ROUNDED_RECT { rect: *p, radiusX: 8.0, radiusY: 8.0 }, &c.pinceau);
                    }
                    self.couleur(c, &BLANC_TRES_FAIBLE);
                    rt.DrawRoundedRectangle(&D2D1_ROUNDED_RECT { rect: rect(p.left + 0.5, p.top + 0.5, p.right - p.left - 1.0, p.bottom - p.top - 1.0), radiusX: 8.0, radiusY: 8.0 }, &c.pinceau, 1.0, None);
                    self.couleur(c, &BLANC_DOUX);
                    self.texte(c, &f.petit_centre, jeton, *p);
                }
                if let Some(dernier) = puces.last() {
                    self.touche_unique(c, "Échap", dernier.right + PUCE_ECART, (r.top + r.bottom) / 2.0, false);
                }
                rt.PopAxisAlignedClip();
            });
        }

        // ── Branche Git, à droite du chemin (avant le filtre) ──
        let wb = self.largeur_branche(app);
        if wb > 0.0 {
            let wf = self.largeur_filtre(app, l, rang);
            let place_filtre = if wf > 0.0 { wf + 16.0 } else { 0.0 };
            let r = rect(l - MARGE_LISTE - TERMINAL_RESERVE - place_filtre - wb, NAV_Y + 9.0, wb, NAV_H - 18.0);
            let arrondi = D2D1_ROUNDED_RECT { rect: r, radiusX: 11.0, radiusY: 11.0 };
            self.couleur(c, &BLANC_TRES_FAIBLE);
            rt.DrawRoundedRectangle(&arrondi, &c.pinceau, 1.0, None);
            let modifie = app.git.as_ref().is_some_and(|g| g.etats.values().any(|&e| e != '?'));
            self.couleur(c, if modifie { &ACCENT } else { &BLANC_DOUX });
            // Embranchement en points
            self.points(c, r.left + 14.0, (r.top + r.bottom) / 2.0, 2.4, 0.95, &[(-1, -2), (-1, -1), (-1, 0), (-1, 1), (-1, 2), (1, -2), (1, -1), (0, 0)]);
            self.couleur(c, &BLANC_DOUX);
            let b = app.git.as_ref().map(|g| g.branche.to_uppercase()).unwrap_or_default();
            self.texte(c, &f.petit, &b, rect(r.left + 26.0, r.top, r.right - r.left - 32.0, r.bottom - r.top));
        }

        // ── Terminal : bouton « >_ » à droite du chemin ──
        if app.emplacement != Emplacement::Reglages {
            let r = decaler(zone_terminal(l), decalage_chrome(app, vue, Cible::Terminal));
            let possible = app.dossier_ici().is_some();
            let survole = app.survol == Cible::Terminal;
            let ouvert = vue.terminal_visible;
            if (survole && possible) || ouvert {
                self.couleur(c, &SURVOL);
                rt.FillRoundedRectangle(&D2D1_ROUNDED_RECT { rect: r, radiusX: 8.0, radiusY: 8.0 }, &c.pinceau);
            }
            self.couleur(c, if ouvert { &ACCENT_BORD } else { &LIGNE });
            rt.DrawRoundedRectangle(&D2D1_ROUNDED_RECT { rect: r, radiusX: 8.0, radiusY: 8.0 }, &c.pinceau, 1.0, None);
            self.couleur(c, if !possible { &BLANC_TRES_FAIBLE } else if survole || ouvert { &BLANC } else { &BLANC_DOUX });
            self.texte(c, &f.petit_mono, ">_", rect(r.left, r.top, r.right - r.left, r.bottom - r.top));
        }

        // ── Note du dossier (O8), à gauche de la branche : point rouge et texte ──
        if let (Some(r), Some(n)) = (app.zones.note, app.note.as_deref()) {
            let arrondi = D2D1_ROUNDED_RECT { rect: r, radiusX: 11.0, radiusY: 11.0 };
            self.couleur(c, if app.survol == Cible::Note { &SURVOL } else { &CHAMP_FOND });
            rt.FillRoundedRectangle(&arrondi, &c.pinceau);
            self.couleur(c, &ACCENT);
            rt.FillEllipse(&ellipse(r.left + 14.0, (r.top + r.bottom) / 2.0, 3.0), &c.pinceau);
            self.couleur(c, &BLANC);
            self.texte(c, &f.petit, &n.to_uppercase(), rect(r.left + 26.0, r.top, r.right - r.left - 34.0, r.bottom - r.top));
        }

        if let Some(p) = vue.page.as_ref().filter(|_| focus && app.emplacement == Emplacement::Reglages) {
            // Lot 12C (`openSet`) : la page entre en fondu, de l'échelle .97 à 1, 380 ms au ressort (panneau unique seulement)
            let etat = if vue.double { None } else { self.mouvements.borrow().page.etat(self.t_image.get()) };
            match etat {
                Some((true, e)) => {
                    let d = mouvement::Decalage { dx: 0.0, dy: 0.0, echelle: e.echelle, opacite: e.opacite };
                    self.avec_decalage(c, &d, (l / 2.0, (HAUTEUR_TITRE + h - STATUT_H) / 2.0), rect(0.0, HAUTEUR_TITRE, l, h - HAUTEUR_TITRE), || self.dessiner_page(c, app, l, h, p, premier));
                }
                _ => self.dessiner_page(c, app, l, h, p, premier),
            }
            return;
        }

        self.dessiner_barre(c, app, vue, vue.double);

        match app.affichage {
            Affichage::Liste => self.dessiner_liste(c, app, l, !vue.double || focus, vue.focus_clavier),
            Affichage::Grille => self.dessiner_grille(c, app, l, vue.focus_clavier && focus),
            Affichage::Colonnes => self.dessiner_miller(c, app, l),
        }
        if !app.zones.jours.is_empty() {
            self.dessiner_chronologie(c, app, l, h);
        }
        self.dessiner_bulle(c, app, l);

        // Lasso : voile rouge léger et contour en points blancs
        if let Some(la) = app.lasso.as_ref().filter(|la| la.actif) {
            let haut_liste = LISTE_Y;
            let bas_liste = LISTE_Y + app.hauteur_liste;
            let y0 = (la.y0.min(la.y1) - app.defilement + LISTE_Y).clamp(haut_liste, bas_liste);
            let y1 = (la.y0.max(la.y1) - app.defilement + LISTE_Y).clamp(haut_liste, bas_liste);
            let (x0, x1) = (la.x0.min(la.x1).max(largeur_volet() + 2.0), la.x0.max(la.x1).min(l - 2.0));
            if x1 > x0 && y1 > y0 {
                self.couleur(c, &D2D1_COLOR_F { a: 0.10, ..ACCENT });
                rt.FillRectangle(&D2D_RECT_F { left: x0, top: y0, right: x1, bottom: y1 }, &c.pinceau);
                // Contour blanc : il reste visible sur les lignes choisies (rouges)
                self.couleur(c, &BLANC);
                let mut x = x0;
                while x <= x1 {
                    rt.FillEllipse(&ellipse(x, y0, 1.1), &c.pinceau);
                    rt.FillEllipse(&ellipse(x, y1, 1.1), &c.pinceau);
                    x += 6.0;
                }
                let mut y = y0;
                while y <= y1 {
                    rt.FillEllipse(&ellipse(x0, y, 1.1), &c.pinceau);
                    rt.FillEllipse(&ellipse(x1, y, 1.1), &c.pinceau);
                    y += 6.0;
                }
            }
        }

        // Dépôt dans le dossier ouvert : contour en pointillés rouges autour de la liste
        if app.depot == Cible::FondListe {
            self.couleur(c, &ACCENT);
            let (gauche, droite) = (largeur_volet() + MARGE_LISTE - 8.0, l - MARGE_LISTE + 4.0);
            let (haut, bas) = (LISTE_Y - 2.0, LISTE_Y + app.hauteur_liste - 4.0);
            let mut x = gauche;
            while x <= droite {
                rt.FillEllipse(&ellipse(x, haut, 1.3), &c.pinceau);
                rt.FillEllipse(&ellipse(x, bas, 1.3), &c.pinceau);
                x += 8.0;
            }
            let mut y = haut;
            while y <= bas {
                rt.FillEllipse(&ellipse(gauche, y, 1.3), &c.pinceau);
                rt.FillEllipse(&ellipse(droite, y, 1.3), &c.pinceau);
                y += 8.0;
            }
        }

        // Message au centre : lecture, erreur ou dossier vide
        let recherche = matches!(app.emplacement, Emplacement::Recherche { .. });
        let globale = matches!(app.emplacement, Emplacement::Globale { .. });
        let message = if let Some(err) = &app.erreur {
            Some((err.as_str(), &ERREUR))
        } else if app.vide() && app.chargement && globale {
            Some(("RECHERCHE DANS L'INDEX…", &BLANC_DOUX))
        } else if app.vide() && globale {
            Some(("AUCUN RESULTAT · FILTRES : EXT:PDF  TAILLE>10MO  MODIF:7  DOSSIER:  DANS:C:\\  CONTENU:TEXTE", &BLANC_DOUX))
        } else if app.vide() && app.chargement {
            Some((if recherche { "RECHERCHE DANS LES SOUS-DOSSIERS…" } else { "LECTURE…" }, &BLANC_DOUX))
        } else if app.vide() && app.filtre.is_empty() && matches!(app.emplacement, Emplacement::Serveur(_)) {
            Some(("AUCUN DOSSIER PARTAGE VISIBLE SUR CE SERVEUR", &BLANC_DOUX))
        } else if app.vide() && app.filtre.is_empty() && recherche {
            Some(("AUCUN RESULTAT", &BLANC_DOUX))
        } else {
            None
        };
        if let Some((m, col)) = message {
            self.aide(c, m, rect(largeur_volet(), LISTE_Y + 40.0, l - largeur_volet(), 40.0), Pose::Centre, col);
        } else if app.vide() {
            // État vide de la maquette (`.empty`) : dossier vide, ou filtre sans résultat. Les yeux ouverts au-dessus d'une phrase,
            // centrés dans la liste (liste, grille et colonnes : la zone est celle de la liste dans les trois vues)
            // Colonnes : le dossier ouvert est la colonne du milieu, c'est elle qui est vide (pas la largeur entière de la liste)
            let (gauche, largeur) = if app.affichage == Affichage::Colonnes { colonnes_miller(l)[1] } else { (largeur_volet(), l - largeur_volet()) };
            self.dessiner_etat_vide(c, &pixel::texte_vide(&app.filtre), vue.pixel.periode, rect(gauche, LISTE_Y, largeur, app.hauteur_liste));
        }

        // Barre de défilement fine
        let (par, hr) = app.pas();
        let total = hauteur_defilable(app.nb_rangs().div_ceil(par), hr);
        if total > app.hauteur_liste && app.hauteur_liste > 0.0 {
            let hb = (app.hauteur_liste * app.hauteur_liste / total).max(24.0);
            let yb = LISTE_Y + (app.hauteur_liste - hb) * (app.defilement / (total - app.hauteur_liste)).clamp(0.0, 1.0);
            self.couleur(c, &BLANC_TRES_FAIBLE);
            rt.FillRoundedRectangle(&D2D1_ROUNDED_RECT { rect: rect(l - 9.0, yb, 3.0, hb), radiusX: 1.5, radiusY: 1.5 }, &c.pinceau);
        }

        // ── Barre d'état : le séparateur et les gestes sont dessinés une seule fois, sur toute la fenêtre ──
        let ys = h - STATUT_H;
        // À gauche : annonce récente (blanche, ou rouge si erreur), sinon le résumé du dossier.
        // À droite, le pied réserve sa place ; l'état d'opération le remplace s'il y en a un.
        let texte_operation = Self::texte_operation(app);
        // Double panneau : le panneau de gauche s'arrête au milieu de la fenêtre, le pied (touches à droite) n'est pas sous lui ;
        // sans cette exception son résumé (« 8 ÉLÉMENTS · 1 DOSSIER · LU EN 0,1 MS ») était coupé en « 8 ÉLÉME… » (lot 17)
        let place_droite = match &texte_operation {
            _ if vue.double && premier => 0.0,
            Some(t) => self.largeur_aide(&segmenter_aide(t)) + MARGE_LISTE + 40.0,
            None => self.largeur_pied() + MARGE_LISTE + 30.0,
        };
        let decalage = if premier { PIXEL_DECALAGE } else { 0.0 };
        let zone_gauche = rect(largeur_volet() + MARGE_LISTE + decalage, ys, (l - largeur_volet() - MARGE_LISTE - decalage - place_droite).max(0.0), STATUT_H);
        match app.annonce.as_ref().filter(|a| a.2.elapsed() < crate::app::DUREE_ANNONCE) {
            Some((texte, erreur, _)) => {
                self.couleur(c, if *erreur { &ERREUR } else { &BLANC });
                rt.FillEllipse(&ellipse(zone_gauche.left + 3.0, ys + STATUT_H / 2.0, 2.5), &c.pinceau);
                self.aide(c, texte, rect(zone_gauche.left + 12.0, ys, zone_gauche.right - zone_gauche.left - 12.0, STATUT_H), Pose::Gauche, if *erreur { &ERREUR } else { &BLANC });
            }
            None if !app.filtre.is_empty() => {
                let texte = format!("{} TROUVES · ENTREE OUVRE · CTRL+F : SOUS-DOSSIERS · ECHAP : EFFACER", systeme::nombre(app.entrees.len()));
                self.aide(c, &texte, zone_gauche, Pose::Gauche, &BLANC_DOUX);
            }
            None if globale => {
                let texte = format!("{} RESULTATS EN {} MS · {}", systeme::nombre(app.entrees.len()), systeme::nombre(app.duree_ms as usize), crate::index::etat());
                self.aide(c, &texte, zone_gauche, Pose::Gauche, &BLANC_DOUX);
            }
            None if matches!(app.emplacement, Emplacement::Sftp { .. }) => {
                let texte = format!("SFTP · {} ÉLÉMENTS · LECTURE SEULE · ENTRÉE OUVRE UNE COPIE · CLIC DROIT : TELECHARGER", systeme::nombre(app.entrees.len()));
                self.aide(c, &texte, zone_gauche, Pose::Gauche, &BLANC_DOUX);
            }
            None if matches!(app.emplacement, Emplacement::Archive { .. }) => {
                let texte = format!("ARCHIVE · {} ÉLÉMENTS · LECTURE SEULE · ENTRÉE OUVRE · CTRL+ALT+X : EXTRAIRE", systeme::nombre(app.entrees.len()));
                self.aide(c, &texte, zone_gauche, Pose::Gauche, &BLANC_DOUX);
            }
            None if matches!(app.emplacement, Emplacement::Doublons(_)) => {
                let texte = if app.chargement {
                    "RECHERCHE DES DOUBLONS · TAILLES PUIS EMPREINTES…".to_string()
                } else if app.tout.is_empty() {
                    "AUCUN DOUBLON".to_string()
                } else {
                    format!("DOUBLONS · {} FICHIERS · {} · GARDER UN EXEMPLAIRE PAR LOT", systeme::nombre(app.tout.len()), crate::doublons::resume(&app.tout))
                };
                self.aide(c, &texte, zone_gauche, Pose::Gauche, &BLANC_DOUX);
            }
            None if app.calcul_tailles > 0 => {
                let texte = format!("CALCUL DES TAILLES · {} DOSSIERS RESTANTS", systeme::nombre(app.calcul_tailles));
                self.aide(c, &texte, zone_gauche, Pose::Gauche, &BLANC_DOUX);
            }
            None if app.marques.len() > 1 => {
                let texte = format!("{} SELECTIONNES · {}", systeme::nombre(app.marques.len()), systeme::taille(app.taille_choisie()));
                self.aide(c, &texte, zone_gauche, Pose::Gauche, &BLANC);
            }
            None if recherche => {
                let texte = format!(
                    "{} RESULTATS EN {} MS{}",
                    systeme::nombre(app.entrees.len()),
                    systeme::nombre(app.duree_ms as usize),
                    if app.entrees.len() >= 20_000 { " · LIMITE ATTEINTE" } else { "" }
                );
                self.aide(c, &texte, zone_gauche, Pose::Gauche, &BLANC_DOUX);
            }
            None => {
                let dossiers = app.entrees.iter().filter(|e| e.est_conteneur()).count();
                let resume = format!(
                    "{} {} · {} {} · LU EN {} MS{}{}",
                    systeme::nombre(app.entrees.len()),
                    if app.entrees.len() > 1 { "ÉLÉMENTS" } else { "ÉLÉMENT" },
                    systeme::nombre(dossiers),
                    if dossiers > 1 { "DOSSIERS" } else { "DOSSIER" },
                    format!("{:.1}", app.duree_ms).replace('.', ","),
                    if app.caches { " · FICHIERS CACHES VISIBLES" } else { "" },
                    if app.non_fiable { " · DOSSIER NON FIABLE" } else { "" }
                );
                self.aide(c, &resume, zone_gauche, Pose::Gauche, &BLANC_DOUX);
            }
        }

        // Barre d'état survolée : elle est cliquable (menu de la sélection)
        if app.survol == Cible::Statut {
            self.couleur(c, &SURVOL);
            rt.FillRectangle(&rect(largeur_volet() + 1.0, ys + 1.0, l - largeur_volet() - 1.0, STATUT_H - 1.0), &c.pinceau);
        }
    }

    /// Vue liste : en-têtes de colonnes et lignes visibles
    /// Chronologie en points (O3) : un point par jour, plus gros les jours chargés ; le jour choisi en rouge
    unsafe fn dessiner_chronologie(&self, c: &Cible2D, app: &App, l: f32, h: f32) {
        let rt = &c.rt;
        let y = h - STATUT_H - CHRONO_H;
        self.couleur(c, &LIGNE);
        rt.FillRectangle(&rect(largeur_volet() + MARGE_LISTE, y, l - largeur_volet() - 2.0 * MARGE_LISTE, 1.0), &c.pinceau);
        let cy = y + CHRONO_H / 2.0;
        // À gauche : le jour survolé, sinon le jour choisi, sinon la période
        let survole = match app.survol {
            Cible::Jour(j) => app.zones.jours.iter().find(|z| z.1 == j).map(|z| (z.1, z.2)),
            _ => None,
        };
        let etiquette = match (survole, app.jour) {
            (Some((j, n)), _) => format!("{} · {n}", systeme::jour_court(j)),
            (None, Some(j)) => format!("{} · FILTRE", systeme::jour_court(j)),
            (None, None) => format!("{} JOURS", app.zones.jours.len()),
        };
        self.couleur(c, if app.jour.is_some() { &BLANC } else { &BLANC_DOUX });
        self.texte(c, &self.f.petit, &etiquette, rect(largeur_volet() + MARGE_LISTE, y, CHRONO_GAUCHE - 8.0, CHRONO_H));
        let max = app.zones.jours.iter().map(|z| z.2).max().unwrap_or(1).max(1) as f32;
        for &(r, j, n) in &app.zones.jours {
            let cx = (r.left + r.right) / 2.0;
            let choisi = app.jour == Some(j);
            if systeme::debut_de_mois(j) {
                self.couleur(c, &BLANC_TRES_FAIBLE);
                rt.FillRectangle(&rect(cx - 0.5, y + 4.0, 1.0, 4.0), &c.pinceau);
            }
            let rayon = if n == 0 { 1.0 } else { 1.6 + 2.6 * ((1.0 + n as f32).ln() / (1.0 + max).ln()) };
            let couleur = if choisi || app.survol == Cible::Jour(j) { &ACCENT } else if n == 0 { &BLANC_TRES_FAIBLE } else { &BLANC_DOUX };
            self.couleur(c, couleur);
            rt.FillEllipse(&ellipse(cx, cy, rayon), &c.pinceau);
        }
    }

    unsafe fn dessiner_liste(&self, c: &Cible2D, app: &App, l: f32, actif: bool, focus_clavier: bool) {
        let rt = &c.rt;
        let f = &self.f;
        let taille_max = if app.tailles { app.taille_max() } else { 0 };
        let surlignage = app.texte_surlignage();
        // ── En-têtes de colonnes ──
        let cols = colonnes(l);
        for (col, x, w) in cols {
            if w <= 0.0 {
                continue;
            }
            let actif = app.tri.0 == col;
            let survole = app.survol == Cible::Entete(col);
            self.couleur(c, if actif || survole { &BLANC } else { &BLANC_FAIBLE });
            let nom = match (col, &app.emplacement) {
                (Colonne::Nom, _) => "NOM",
                (Colonne::Date, Emplacement::CePC) => "REMPLISSAGE",
                (Colonne::Date, _) if app.tailles => "TAILLE RELATIVE",
                (Colonne::Date, _) => match crate::reglages::date_montree() {
                    1 => "MODIFIÉ",
                    2 => "AJOUTÉ",
                    _ => "DATE",
                },
                (Colonne::Type, _) => "TYPE",
                (Colonne::Taille, Emplacement::CePC) => "LIBRE",
                (Colonne::Taille, _) => "TAILLE",
                (Colonne::Dimensions, _) => "DIMENSIONS",
                (Colonne::Duree, _) => "DUREE",
                (Colonne::Auteur, _) => "AUTEUR",
                (Colonne::Origine, _) => "ORIGINE",
                (Colonne::Signature, _) => "SIGNE PAR",
            };
            let droite = col == Colonne::Taille;
            let zone = if col == Colonne::Nom { rect(x + 28.0, ENTETE_Y, w - 36.0, ENTETE_H) } else { rect(x, ENTETE_Y, w - 8.0, ENTETE_H) };
            self.texte(c, if droite { &f.petit_droite } else { &f.petit }, nom, zone);
            if actif {
                // Point rouge : plein = croissant, anneau = décroissant
                let tw = self.mesurer(nom, &f.petit);
                let px = if droite { zone.right - tw - 10.0 } else { zone.left + tw + 9.0 };
                self.couleur(c, &ACCENT);
                let p = ellipse(px, ENTETE_Y + ENTETE_H / 2.0, 3.0);
                if app.tri.1 {
                    rt.FillEllipse(&p, &c.pinceau);
                } else {
                    rt.DrawEllipse(&p, &c.pinceau, 1.3, None);
                }
            }
        }
        self.couleur(c, &LIGNE);
        rt.FillRectangle(&rect(largeur_volet() + MARGE_LISTE, ENTETE_Y + ENTETE_H, l - largeur_volet() - MARGE_LISTE - MARGE_LISTE_DROITE, 1.0), &c.pinceau);

        // ── Liste (seules les lignes visibles sont dessinées) ──
        let zone_liste = rect(largeur_volet() + 1.0, LISTE_Y, l - largeur_volet() - 1.0, app.hauteur_liste);
        rt.PushAxisAlignedClip(&zone_liste, D2D1_ANTIALIAS_MODE_ALIASED);
        let premiere = (app.defilement / hauteur_ligne()).floor().max(0.0) as usize;
        let nb = (app.hauteur_liste / hauteur_ligne()).ceil() as usize + 2;
        let x0 = largeur_volet() + MARGE_LISTE - 6.0;
        // Le fond de ligne va jusqu'à la marge droite de la liste ; seules les colonnes s'arrêtent avant la bande vide
        let largeur_fond = (l - x0 - MARGE_LISTE_DROITE + 6.0).max(0.0);
        // Noms marqués NOUVEAU dans ce dossier (lus une fois par dessin) et largeur du mot avec son point
        // Lot 12B : tri, vue, filtre ou regroupement changés, les lignes glissent de leur ancienne place à la nouvelle (la place
        // finale reste celle du clic, de la sélection et du défilement)
        let derniere = (premiere + nb).min(app.nb_rangs());
        let mut places = Vec::new();
        let mut cases = Vec::new();
        for r in premiere..derniere {
            if let crate::app::Rang::Element(i) = app.rang(r) {
                places.push((&app.entrees[i], (x0, LISTE_Y + r as f32 * hauteur_ligne() - app.defilement)));
                cases.push(r - premiere);
            }
        }
        let mut decalages = vec![mouvement::Decalage::AUCUN; derniere.saturating_sub(premiere)];
        for (case, d) in cases.iter().zip(self.decalages_flip(app, &places, 0, largeur_fond, hauteur_ligne())) {
            decalages[*case] = d;
        }
        // Lot 12C : à l'ouverture de la fenêtre chaque ligne entre à son tour (26 ms par ligne, 460 ms au ressort), par-dessus le
        // glissement éventuel d'un tri
        if let (Some(ecoule), true) = (self.cascade.get(), crate::reglages::animations_actives()) {
            for (k, d) in decalages.iter_mut().enumerate() {
                *d = mouvement::combiner(*d, mouvement::decalage_cascade(k, ecoule));
            }
        }
        let couche_flip = self.couche_flip(c, &decalages);
        // Pastille de sélection (lot 12A) : la barre d'accent glisse de l'ancienne ligne à la nouvelle. Sa position est en
        // coordonnées du contenu, donc un défilement ne la déplace pas ; elle saute si l'ancienne ligne n'est plus visible.
        let pastille = self.pastille_glissante(app);
        let nouveaux = app.dossier().map(crate::navigateur::nouveaux_dans).unwrap_or_default();
        let largeur_nouveau = if nouveaux.is_empty() { 0.0 } else { self.mesurer("NOUVEAU", &f.petit) + 10.0 };
        for r in premiere..(premiere + nb).min(app.nb_rangs()) {
            let y = LISTE_Y + r as f32 * hauteur_ligne() - app.defilement;
            let i = match app.rang(r) {
                crate::app::Rang::Element(i) => i,
                crate::app::Rang::Section(s) => {
                    self.dessiner_section(c, app, s, x0, y, largeur_fond, cols[0].1);
                    continue;
                }
            };
            let e = &app.entrees[i];
            let garde = self.flip_pousser(c, &decalages[r - premiere], (x0 + largeur_fond / 2.0, y + hauteur_ligne() / 2.0), rect(0.0, y - 8.0, l, hauteur_ligne() + 16.0), &couche_flip);

            // Apparition : la ligne est d'abord une rangée de points, qui se dissout en laissant place au contenu
            let a = app.apparition_ligne(i, premiere);
            if a < 1.0 {
                let longueur = (e.nom.chars().count().min(40) as f32 * 8.0 + 120.0).min(largeur_fond - 40.0);
                let mut x = cols[0].1 + 10.0;
                let mut k = 0;
                while x < cols[0].1 + longueur {
                    // Les points s'éteignent un peu au hasard (dissolution), le premier est rouge
                    let seuil = ((k * 37 + i * 11) % 17) as f32 / 17.0;
                    if a < seuil * 0.9 + 0.1 {
                        let col = if k == 0 { ACCENT } else { BLANC };
                        self.couleur(c, &D2D1_COLOR_F { a: (1.0 - a) * if k == 0 { 0.9 } else { 0.35 }, ..col });
                        rt.FillEllipse(&ellipse(x, y + hauteur_ligne() / 2.0, 1.6), &c.pinceau);
                    }
                    x += 9.0;
                    k += 1;
                }
                if a <= 0.0 {
                    self.flip_retirer(c, garde);
                    continue;
                }
                let params = D2D1_LAYER_PARAMETERS {
                    contentBounds: rect(0.0, y, l, hauteur_ligne()),
                    maskAntialiasMode: D2D1_ANTIALIAS_MODE_PER_PRIMITIVE,
                    maskTransform: windows::Foundation::Numerics::Matrix3x2::identity(),
                    opacity: a,
                    ..Default::default()
                };
                rt.PushLayer(&params, &c.couche);
            }

            let fond = rect(x0, y + 1.0, largeur_fond, hauteur_ligne() - 2.0);
            let choisie = app.marques.contains(&i);
            let arrondi = D2D1_ROUNDED_RECT { rect: fond, radiusX: 12.0, radiusY: 12.0 };
            if choisie {
                // Sélection de la maquette v6 : fond neutre et trait d'accent à gauche ; panneau sans focus : estompé
                if actif { self.couleur_ou(c, &SELECTION, &SELECTION_CLAIR); } else { self.couleur(c, &SELECTION_TERNE); }
                rt.FillRoundedRectangle(&arrondi, &c.pinceau);
                // La ligne glissante a sa barre dessinée après la boucle, à sa position animée
                if pastille.map_or(true, |(p, _)| p != i) {
                    self.couleur(c, if actif { &ACCENT } else { &BLANC_FAIBLE });
                    rt.FillRoundedRectangle(&D2D1_ROUNDED_RECT { rect: rect(x0 + 2.0, y + 12.0, 3.0, (hauteur_ligne() - 24.0).max(6.0)), radiusX: 1.5, radiusY: 1.5 }, &c.pinceau);
                }
            } else {
                // Fond de survol : fondu de 120 ms à l'entrée et à la sortie (`.row { transition: background-color .12s }`)
                let k = if actif { self.survol_k(Cible::Ligne(i)) } else { 0.0 };
                if k > 0.0 {
                    self.couleur_k(c, &SURVOL_FORT, k);
                    rt.FillRoundedRectangle(&arrondi, &c.pinceau);
                }
            }
            // Dossier visé par un glisser-déposer
            if app.depot == Cible::Ligne(i) {
                self.couleur(c, &ACCENT);
                rt.DrawRoundedRectangle(&arrondi, &c.pinceau, 1.5, None);
            }
            // Ligne qui a le focus sans être choisie (Ctrl+clic) : simple contour
            if app.selection == Some(i) && !choisie {
                self.couleur(c, &BLANC_TRES_FAIBLE);
                rt.DrawRoundedRectangle(&arrondi, &c.pinceau, 1.0, None);
            }
            let coupe = !app.coupes.is_empty() && app.chemin_de(e).is_some_and(|p| app.coupes.contains(&p));
            let attenue = (e.cache || coupe) && !choisie;
            let principal = if attenue { &BLANC_FAIBLE } else { &BLANC };
            let secondaire = if choisie { &BLANC } else if attenue { &BLANC_TRES_FAIBLE } else { &BLANC_DOUX };

            // Icône à points du type de fichier
            let (cx, cy) = (cols[0].1 + 10.0, y + hauteur_ligne() / 2.0);
            let opacite = if attenue { 0.35 } else if e.est_conteneur() || choisie { 1.0 } else { 0.8 };
            // Survol (ou sélection multiple) : l'icône devient une case à cocher, comme dans Files
            // Lot 12C : la case apparaît en 150 ms (échelle .6 → 1 au ressort en 300 ms) pendant que l'icône s'efface (`.ic`, 150 ms)
            let voulue = app.survol == Cible::Ligne(i) || (choisie && app.marques.len() > 1);
            let (opacite_case, echelle_case) = {
                let mut m = self.mouvements.borrow_mut();
                let image = m.image;
                let cle = ((self.panneau_en_cours.get().min(1) as u64) << 32) | i as u64;
                m.cases.etat(cle, voulue, image, self.t_image.get(), crate::reglages::animations_actives())
            };
            if opacite_case > 0.0 {
                let d = mouvement::Decalage { dx: 0.0, dy: 0.0, echelle: echelle_case, opacite: opacite_case };
                self.avec_decalage(c, &d, (cx, cy), rect(cx - 16.0, cy - 16.0, 32.0, 32.0), || self.case_a_cocher(c, cx, cy, choisie, choisie));
            }
            let part_icone = 1.0 - opacite_case;
            if part_icone > 0.003 {
                if let Some(m) = Self::glyphe_de(app, e) {
                    self.motif_points(c, m, cx, cy, 2.8, choisie, opacite * part_icone);
                } else if let Some(p) = icones::pastille_de(e) {
                    // `.thumb` 22 px, rayon 5 ; légèrement décalée pour que la balise de couleur garde sa place à gauche
                    self.pastille(c, p, cx + 1.0, cy, 22.0, 5.0, if attenue { 0.4 } else { 1.0 } * part_icone);
                } else {
                    self.icone(c, icones::pour(e), cx, cy, choisie, opacite * part_icone);
                }
            }
            // Balise de couleur : un point devant l'icône
            if let Some(b) = app.chemin_de(e).and_then(|p| outils::balise(&p)) {
                self.couleur(c, &Self::couleur_balise(b));
                rt.FillEllipse(&ellipse(cx - 13.5, cy, 2.6), &c.pinceau);
            }

            let etat_cloud = if crate::reglages::ETAT_CLOUD.load(std::sync::atomic::Ordering::Relaxed) { e.nuage } else { Nuage::Aucun };
            let place_nuage = if etat_cloud == Nuage::Aucun { 0.0 } else { 20.0 };
            let git = app.etat_git(e);
            let place_git = if git.is_some() { 18.0 } else { 0.0 };
            let zone_nom = rect(cols[0].1 + 28.0, y, cols[0].2 - 36.0 - place_nuage - place_git, hauteur_ligne());
            // État Git : M (modifié) et A (ajouté) en rouge, ? (nouveau, non suivi) en pâle
            if let Some(g) = git {
                self.couleur(c, if choisie { &BLANC } else if g == '?' { &BLANC_DOUX } else { &ACCENT });
                self.texte(c, &f.petit, &g.to_string(), rect(zone_nom.right + 4.0, y, 14.0, hauteur_ligne()));
            }
            let principal = if git.is_some_and(|g| g != '?') && !choisie && !attenue { &ACCENT } else { principal };
            if etat_cloud != Nuage::Aucun {
                // Anneau : en ligne seulement · point : sur ce PC · point dans un anneau : toujours gardé.
                // En rouge pendant la synchronisation (envoi ou réception en cours)
                let p = ellipse(cols[0].1 + cols[0].2 - 18.0, cy, 4.0);
                let en_cours = app.pas_synchro.contains(&e.nom.to_lowercase());
                self.couleur(c, if en_cours { &ACCENT } else if choisie { &BLANC } else if etat_cloud == Nuage::EnLigne { &BLANC_DOUX } else { &BLANC });
                if etat_cloud != Nuage::Local {
                    rt.DrawEllipse(&p, &c.pinceau, 1.2, None);
                }
                if etat_cloud != Nuage::EnLigne {
                    rt.FillEllipse(&ellipse(p.point.x, cy, if etat_cloud == Nuage::Epingle { 1.8 } else { 2.6 }), &c.pinceau);
                }
            }
            match app.edition.as_ref().filter(|ed| ed.index == i) {
                Some(ed) => self.champ(c, ed, zone_nom),
                None => {
                    self.couleur(c, principal);
                    let nom = nom_vu(e);
                    let etiquette = if crate::etiquettes::existe_une() { app.chemin_de(e).and_then(|p| crate::etiquettes::de(&p)) } else { None };
                    let marque = app.badges.get(&e.nom.to_lowercase()).map(|b| b.libelle()).or_else(|| {
                        app.comparaison.as_ref().and_then(|m| m.get(&e.nom.to_lowercase())).filter(|x| **x != crate::app::Ecart::Pareil).map(|x| x.court())
                    });
                    // Marque NOUVEAU (téléchargement du navigateur arrivé pendant que SpaceFiles tourne) : sa place est réservée, le nom
                    // se tronque avant qu'elle soit coupée
                    let nouveau = !nouveaux.is_empty() && nouveaux.contains(&e.nom.to_lowercase());
                    let reserve = marque.map_or(0.0, |t| self.mesurer(t, &f.petit) + 28.0) + if nouveau { largeur_nouveau + 10.0 } else { 0.0 };
                    let zone_texte = if reserve > 0.0 && zone_nom.right - zone_nom.left > reserve + 48.0 { rect(zone_nom.left, y, zone_nom.right - zone_nom.left - reserve, hauteur_ligne()) } else { zone_nom };
                    // Mode capture (Z12) : le nom des éléments choisis est masqué, pour partager l'écran
                    let masque: Vec<u16>;
                    let nom = if choisie && crate::reglages::MODE_CAPTURE.load(std::sync::atomic::Ordering::Relaxed) {
                        masque = vec![0x2588; nom.len().min(24)];
                        &masque[..]
                    } else {
                        nom
                    };
                    self.texte_w(c, &f.liste, nom, zone_texte);
                    self.souligner(c, &f.liste, nom, &surlignage, zone_texte);
                    // Groupes de l'élément : pastilles « • NOM » après le nom (hors de la vue du groupe lui-même)
                    let groupes: Vec<String> = app.chemin_de(e).map(|p| crate::groupes::de(&p)).unwrap_or_default();
                    let ouvert = if let Emplacement::Groupe(n) = &app.emplacement { Some(n.as_str()) } else { None };
                    let mut xg = (zone_nom.left + self.mesurer(&e.nom, &f.liste) + 14.0).min(zone_texte.right + 8.0);
                    // Étiquette (lot 16, `.sq`) : 1 à 3 carrés juste après le nom, avant les pastilles de groupes
                    if let Some(n) = etiquette {
                        let x_carres = zone_nom.left + self.mesurer_nom(e, &f.liste) + 8.0;
                        if x_carres + largeur_etiquette(n) <= zone_texte.right {
                            self.carres_etiquette(c, x_carres, cy, n);
                            xg = xg.max(x_carres + largeur_etiquette(n) + 6.0);
                        }
                    }
                    for g in groupes.iter().filter(|g| Some(g.as_str()) != ouvert).take(3) {
                        let wg = self.mesurer(g, &f.petit) + 22.0;
                        if xg + wg > zone_nom.right {
                            break;
                        }
                        let pastille = rect(xg, y + 10.0, wg, hauteur_ligne() - 20.0);
                        let fond_pastille = if choisie { rgba(0, 0, 0, 0.18) } else { SURVOL };
                        self.couleur(c, &fond_pastille);
                        rt.FillRoundedRectangle(&D2D1_ROUNDED_RECT { rect: pastille, radiusX: 9.0, radiusY: 9.0 }, &c.pinceau);
                        self.couleur(c, &if choisie { BLANC } else { Self::couleur_groupe(app.groupes.iter().find(|x| x.nom == *g).map_or('r', |x| x.couleur)) });
                        rt.FillEllipse(&ellipse(xg + 9.0, y + hauteur_ligne() / 2.0, 2.6), &c.pinceau);
                        self.couleur(c, if choisie { &BLANC } else { &BLANC_DOUX });
                        self.texte(c, &f.petit, g, rect(xg + 16.0, y, wg - 18.0, hauteur_ligne()));
                        xg += wg + 6.0;
                    }
                    // Téléchargements : DEJA INSTALLE, COPIE SUR LE PC
                    if let Some(b) = app.badges.get(&e.nom.to_lowercase()) {
                        let t = b.libelle();
                        let wg = self.mesurer(t, &f.petit) + 16.0;
                        if xg + wg <= zone_nom.right {
                            let pastille = rect(xg, y + 9.0, wg, hauteur_ligne() - 18.0);
                            let arrondi = D2D1_ROUNDED_RECT { rect: pastille, radiusX: 9.0, radiusY: 9.0 };
                            self.couleur(c, if choisie { &BLANC } else { &ACCENT });
                            rt.DrawRoundedRectangle(&arrondi, &c.pinceau, 1.0, None);
                            self.texte(c, &f.petit, t, rect(xg + 8.0, y, wg - 10.0, hauteur_ligne()));
                            xg += wg + 6.0;
                        }
                    }
                    // Comparaison des panneaux : l'écart de l'élément, en pastille
                    if let Some(ecart) = app.comparaison.as_ref().and_then(|m| m.get(&e.nom.to_lowercase())).filter(|x| **x != crate::app::Ecart::Pareil) {
                        let mut t = ecart.libelle();
                        let mut wg = self.mesurer(t, &f.petit) + 16.0;
                        if xg + wg > zone_nom.right {
                            t = ecart.court();
                            wg = self.mesurer(t, &f.petit) + 16.0;
                        }
                        if xg + wg <= zone_nom.right {
                            let pastille = rect(xg, y + 9.0, wg, hauteur_ligne() - 18.0);
                            let arrondi = D2D1_ROUNDED_RECT { rect: pastille, radiusX: 9.0, radiusY: 9.0 };
                            let fort = ecart.a_copier();
                            self.couleur(c, if choisie { &BLANC } else if fort { &ACCENT } else { &BLANC_DOUX });
                            rt.DrawRoundedRectangle(&arrondi, &c.pinceau, 1.0, None);
                            self.texte(c, &f.petit, t, rect(xg + 8.0, y, wg - 10.0, hauteur_ligne()));
                            xg += wg + 6.0;
                        }
                    }
                    // NOUVEAU : un point et le mot entier (jamais la couleur seule), après les badges, jamais par-dessus
                    if nouveau && xg + largeur_nouveau <= zone_nom.right {
                        self.couleur(c, if choisie { &BLANC } else { &ACCENT });
                        rt.FillEllipse(&ellipse(xg + 3.0, y + hauteur_ligne() / 2.0, 2.6), &c.pinceau);
                        self.texte(c, &f.petit, "NOUVEAU", rect(xg + 10.0, y, largeur_nouveau - 10.0 + 2.0, hauteur_ligne()));
                        xg += largeur_nouveau + 6.0;
                    }
                    // Groupe ouvert : le dossier de chaque élément, en pâle, après le nom
                    if let (Some(p), Emplacement::Groupe(_) | Emplacement::Recents | Emplacement::Corbeille | Emplacement::Doublons(_)) = (&e.parent, &app.emplacement) {
                        let w = xg - zone_nom.left - 2.0;
                        if w < zone_nom.right - zone_nom.left - 40.0 {
                            self.couleur(c, if choisie { &BLANC } else { &BLANC_DOUX });
                            self.texte(c, &f.petit_mono, &format!("· {p}"), rect(zone_nom.left + w, y, zone_nom.right - zone_nom.left - w, hauteur_ligne()));
                        }
                    }
                    // Résultat de recherche : son dossier, en pâle, après le nom
                    if let (Some(p), Emplacement::Recherche { racine, .. }) = (&e.parent, &app.emplacement) {
                        let relatif = p.get(racine.trim_end_matches('\\').len()..).unwrap_or("").trim_start_matches('\\');
                        let ou = if relatif.is_empty() { "·".to_string() } else { format!("· {relatif}") };
                        let w = self.mesurer(&e.nom, &f.liste) + 12.0;
                        if w < zone_nom.right - zone_nom.left - 40.0 {
                            self.couleur(c, if choisie { &BLANC } else { &BLANC_DOUX });
                            self.texte(c, &f.petit_mono, &ou, rect(zone_nom.left + w, y, zone_nom.right - zone_nom.left - w, hauteur_ligne()));
                        }
                    }
                }
            }

            self.couleur(c, secondaire);
            match &e.genre {
                Genre::Disque { libre, total, racine } => {
                    // Remplissage du disque en 12 points, rouges si presque plein
                    let rempli = if *total > 0 { 1.0 - *libre as f64 / *total as f64 } else { 0.0 };
                    let n = (rempli * 12.0).round() as i32;
                    for k in 0..12 {
                        let plein = k < n;
                        let col = if !plein { &BLANC_TRES_FAIBLE } else if rempli > 0.9 && !choisie { &ERREUR } else { secondaire };
                        self.couleur(c, col);
                        rt.FillEllipse(&ellipse(cols[1].1 + 4.0 + k as f32 * 9.0, cy, 2.6), &c.pinceau);
                    }
                    self.couleur(c, secondaire);
                    // Disque presque plein : le mot accompagne la couleur (une couleur ne porte jamais seule un état)
                    let presque_plein = rempli > 0.9;
                    if presque_plein && !choisie {
                        self.couleur(c, &ERREUR);
                    }
                    let etiquette = crate::dossier::etiquette_disque(presque_plein, racine.starts_with("\\\\"), *libre, &|t| self.mesurer(t, &f.petit) <= cols[2].2 - 8.0);
                    self.texte(c, &f.petit, &etiquette, rect(cols[2].1, y, cols[2].2 - 8.0, hauteur_ligne()));
                    self.couleur(c, secondaire);
                    self.texte(c, &f.liste_droite, &systeme::taille(*libre), rect(cols[3].1, y, cols[3].2 - 8.0, hauteur_ligne()));
                }
                genre => {
                    if app.tailles && cols[1].2 > 0.0 {
                        // Vue « tailles » : barre de 16 points proportionnelle au plus gros élément
                        let max = taille_max.max(1) as f64;
                        let n = ((e.taille as f64 / max) * 16.0).ceil() as i32;
                        for k in 0..16 {
                            let col = if k >= n { &BLANC_TRES_FAIBLE } else if choisie { &BLANC } else if k >= 12 { &ACCENT } else { &BLANC_DOUX };
                            self.couleur(c, col);
                            rt.FillEllipse(&ellipse(cols[1].1 + 4.0 + k as f32 * 9.0, cy, 2.6), &c.pinceau);
                        }
                        self.couleur(c, secondaire);
                    } else if cols[1].2 > 0.0 {
                        self.texte(c, &f.liste, &systeme::date_liste(e.date_colonne()), rect(cols[1].1, y, cols[1].2 - 8.0, hauteur_ligne()));
                    }
                    if cols[2].2 > 0.0 {
                        self.texte(c, &f.liste, icones::nom_type(e), rect(cols[2].1, y, cols[2].2 - 8.0, hauteur_ligne()));
                    }
                    // Colonnes de propriétés (lues en arrière-plan)
                    if let Some(m) = &e.meta {
                        for (k, t) in [(4, m.dimensions()), (5, m.duree_texte()), (6, m.auteur.clone()), (7, m.origine.clone()), (8, m.signature.clone())] {
                            if cols[k].2 > 0.0 && !t.is_empty() {
                                self.texte(c, &f.liste, &t, rect(cols[k].1, y, cols[k].2 - 10.0, hauteur_ligne()));
                            }
                        }
                    }
                    if cols[3].2 > 0.0 && (matches!(genre, Genre::Fichier) || e.taille > 0) {
                        self.texte(c, &f.liste_droite, &systeme::taille(e.taille), rect(cols[3].1, y, cols[3].2 - 8.0, hauteur_ligne()));
                    }
                }
            }
            if focus_clavier && actif && app.selection == Some(i) {
                self.anneau_dans(c, fond, 12.0);
            }
            if a < 1.0 {
                rt.PopLayer();
            }
            self.flip_retirer(c, garde);
        }
        // Lot 12D : les lignes supprimées finissent leur sortie sous les suivantes (opacité 1 → 0, échelle 1 → .92, 220 ms)
        for (ligne, r, d) in &self.lignes_sortantes(0) {
            let y = r[1] - app.defilement;
            let garde = self.flip_pousser(c, d, (x0 + largeur_fond / 2.0, y + hauteur_ligne() / 2.0), rect(0.0, y - 8.0, l, hauteur_ligne() + 16.0), &self.couche_opacite(c, d.opacite));
            self.dessiner_ligne_sortante(c, app, ligne, &cols, x0, y, largeur_fond, actif);
            self.flip_retirer(c, garde);
        }
        if let Some((choisie, haut)) = pastille {
            self.couleur(c, if actif { &ACCENT } else { &BLANC_FAIBLE });
            let y = LISTE_Y + haut - app.defilement;
            // La barre suit la ligne choisie, glissement d'entrée compris (elle est posée à la place finale puis décalée comme elle)
            let d = app.rang_de(choisie).checked_sub(premiere).and_then(|k| decalages.get(k)).copied().unwrap_or(mouvement::Decalage::AUCUN);
            let garde = self.flip_pousser(c, &d, (x0 + largeur_fond / 2.0, y + hauteur_ligne() / 2.0), rect(0.0, y - 8.0, l, hauteur_ligne() + 16.0), &couche_flip);
            rt.FillRoundedRectangle(&D2D1_ROUNDED_RECT { rect: rect(x0 + 2.0, y + 12.0, 3.0, (hauteur_ligne() - 24.0).max(6.0)), radiusX: 1.5, radiusY: 1.5 }, &c.pinceau);
            self.flip_retirer(c, garde);
        }
        if app.emplacement == Emplacement::CePC {
            self.dessiner_carte(c, app, l, haut_carte(app));
        }
        rt.PopAxisAlignedClip();

    }

    /// Les lignes en train de sortir dans la vue `vue` (0 liste, 1 grille, 2 colonnes) du panneau en cours, avec leur décalage du moment
    fn lignes_sortantes(&self, vue: u8) -> Vec<(mouvement::LigneSortante, [f32; 4], mouvement::Decalage)> {
        let m = self.mouvements.borrow();
        m.fantomes_lignes[self.panneau_en_cours.get().min(1)].iter().filter(|g| g.contenu.vue == vue).filter_map(|g| g.decalage(self.t_image.get()).map(|d| (g.contenu.clone(), g.rect, d))).collect()
    }

    /// Une ligne qui sort (lot 12D) : fond de sélection, icône, nom, date, type et taille, comme elle était dessinée. Sa mémoire est
    /// celle de l'entrée au moment de la suppression ; les marques (Git, groupes, badges) n'y sont pas.
    unsafe fn dessiner_ligne_sortante(&self, c: &Cible2D, app: &App, ligne: &mouvement::LigneSortante, cols: &[(Colonne, f32, f32); 9], x0: f32, y: f32, largeur_fond: f32, actif: bool) {
        let rt = &c.rt;
        let f = &self.f;
        let (e, choisie, h) = (&ligne.entree, ligne.choisie, hauteur_ligne());
        if choisie {
            if actif { self.couleur_ou(c, &SELECTION, &SELECTION_CLAIR); } else { self.couleur(c, &SELECTION_TERNE); }
            rt.FillRoundedRectangle(&D2D1_ROUNDED_RECT { rect: rect(x0, y + 1.0, largeur_fond, h - 2.0), radiusX: 12.0, radiusY: 12.0 }, &c.pinceau);
            self.couleur(c, if actif { &ACCENT } else { &BLANC_FAIBLE });
            rt.FillRoundedRectangle(&D2D1_ROUNDED_RECT { rect: rect(x0 + 2.0, y + 12.0, 3.0, (h - 24.0).max(6.0)), radiusX: 1.5, radiusY: 1.5 }, &c.pinceau);
        }
        let (cx, cy) = (cols[0].1 + 10.0, y + h / 2.0);
        if let Some(m) = Self::glyphe_de(app, e) {
            self.motif_points(c, m, cx, cy, 2.8, choisie, 1.0);
        } else if let Some(p) = icones::pastille_de(e) {
            self.pastille(c, p, cx + 1.0, cy, 22.0, 5.0, 1.0);
        } else {
            self.icone(c, icones::pour(e), cx, cy, choisie, 1.0);
        }
        self.couleur(c, &BLANC);
        self.texte_w(c, &f.liste, nom_vu(e), rect(cols[0].1 + 28.0, y, cols[0].2 - 36.0, h));
        self.couleur(c, if choisie { &BLANC } else { &BLANC_DOUX });
        if cols[1].2 > 0.0 {
            self.texte(c, &f.liste, &systeme::date_liste(e.date_colonne()), rect(cols[1].1, y, cols[1].2 - 8.0, h));
        }
        if cols[2].2 > 0.0 {
            self.texte(c, &f.liste, icones::nom_type(e), rect(cols[2].1, y, cols[2].2 - 8.0, h));
        }
        if cols[3].2 > 0.0 && (matches!(e.genre, Genre::Fichier) || e.taille > 0) {
            self.texte(c, &f.liste_droite, &systeme::taille(e.taille), rect(cols[3].1, y, cols[3].2 - 8.0, h));
        }
    }

    /// Décalages des entrées visibles du panneau en cours (lot 12B). `places` = entrée et place **finale** à l'écran ; un décalage par
    /// place, dans l'ordre. Le clic, la sélection et le défilement ne les voient pas : ils gardent la place finale.
    fn decalages_flip(&self, app: &App, places: &[(&Entree, (f32, f32))], vue: u8, largeur: f32, hauteur: f32) -> Vec<mouvement::Decalage> {
        let k = self.panneau_en_cours.get().min(1);
        let entrees: Vec<(String, (f32, f32))> = places.iter().map(|(e, p)| (cle_flip(e), *p)).collect();
        let mut m = self.mouvements.borrow_mut();
        // Lot 12D : une ligne dont la suppression était annoncée vient de quitter la liste : elle finit sa sortie en fantôme et les
        // suivantes glissent (le `flip` de la maquette, l. 565)
        m.lignes_retirees(k, &app.emplacement, &entrees, vue, largeur, hauteur, app.defilement, self.t_image.get(), crate::reglages::animations_actives());
        let d = m.flips[k].image(&app.emplacement, hauteur_ligne().to_bits(), mouvement::generation_flip(), &entrees, self.t_image.get(), crate::reglages::animations_actives());
        if let Some((ms, images)) = m.flips[k].mesure.take() {
            if mouvement::mesure_demandee() {
                crate::journal::ecrire(crate::journal::Niveau::Info, "mouvement", &format!("FLIP du premier au dernier dessin avec décalage : {ms} ms, {images} images"));
            }
        }
        d
    }

    /// Calque pour un dessin d'opacité `opacite` ; `None` si elle est pleine (pas de calque à payer au repos)
    unsafe fn couche_opacite(&self, c: &Cible2D, opacite: f32) -> Option<ID2D1Layer> {
        (opacite < 0.999).then(|| c.rt.CreateLayer(None).ok()).flatten()
    }

    /// Calque d'opacité pour les entrées qui apparaissent ; `None` si aucune n'en a besoin
    unsafe fn couche_flip(&self, c: &Cible2D, decalages: &[mouvement::Decalage]) -> Option<ID2D1Layer> {
        decalages.iter().any(|d| d.opacite < 0.999).then(|| c.rt.CreateLayer(None).ok()).flatten()
    }

    /// Pose le décalage d'une entrée sur la transformation du dessin (translation, échelle autour de `centre`) et, si elle apparaît,
    /// un calque d'opacité. Rend de quoi tout défaire avec `flip_retirer` ; `None` quand l'entrée est à sa place.
    unsafe fn flip_pousser(&self, c: &Cible2D, d: &mouvement::Decalage, centre: (f32, f32), bornes: D2D_RECT_F, couche: &Option<ID2D1Layer>) -> Option<(Matrix3x2, bool)> {
        if d.est_aucun() {
            return None;
        }
        let rt = &c.rt;
        let mut base = Matrix3x2::identity();
        rt.GetTransform(&mut base);
        let s = d.echelle;
        let local = Matrix3x2 { M11: s, M12: 0.0, M21: 0.0, M22: s, M31: centre.0 * (1.0 - s) + d.dx, M32: centre.1 * (1.0 - s) + d.dy };
        rt.SetTransform(&composer(&local, &base));
        let mut calque = false;
        if d.opacite < 0.999 {
            if let Some(l) = couche {
                let params = D2D1_LAYER_PARAMETERS { contentBounds: bornes, maskAntialiasMode: D2D1_ANTIALIAS_MODE_PER_PRIMITIVE, maskTransform: Matrix3x2::identity(), opacity: d.opacite.clamp(0.0, 1.0), ..Default::default() };
                rt.PushLayer(&params, l);
                calque = true;
            }
        }
        Some((base, calque))
    }

    unsafe fn flip_retirer(&self, c: &Cible2D, garde: Option<(Matrix3x2, bool)>) {
        if let Some((base, calque)) = garde {
            if calque {
                c.rt.PopLayer();
            }
            c.rt.SetTransform(&base);
        }
    }

    /// Met à jour le glissement de la pastille du panneau en cours et rend (élément choisi, haut de la pastille dans le contenu).
    /// `None` : pas de pastille glissante (plusieurs éléments choisis, aucun, ou ligne absente) ; les barres sont alors posées
    /// sur leur ligne.
    fn pastille_glissante(&self, app: &App) -> Option<(usize, f32)> {
        let k = self.panneau_en_cours.get().min(1);
        let mut m = self.mouvements.borrow_mut();
        let cle = (app.emplacement.clone(), app.entrees.len());
        // Les entrées glissent (lot 12B) : la ligne choisie porte son propre décalage, la pastille la suit en le reprenant
        // au dessin ; elle ne glisse pas en plus, sinon elle serait dédoublée
        let flip = m.flips[k].en_cours(self.t_image.get());
        let changee = flip || m.cle_pastille[k].as_ref() != Some(&cle);
        m.cle_pastille[k] = Some(cle);
        let i = app.selection.filter(|i| app.marques.len() == 1 && app.marques.contains(i) && *i < app.entrees.len());
        let Some(i) = i else {
            m.pastille[k].oublier();
            return None;
        };
        let (t, animer) = (self.t_image.get(), crate::reglages::animations_actives());
        let cible = app.rang_de(i) as f32 * hauteur_ligne();
        let g = &mut m.pastille[k];
        let saute = changee || mouvement::pastille_saute(g.valeur(t), cible, hauteur_ligne(), app.defilement, app.hauteur_liste, animer);
        g.viser(cible, t, mouvement::PASTILLE_MS, saute);
        Some((i, g.valeur(t)))
    }

    /// Corbeille Glyph (O7) : 100 points, allumés selon la place qu'elle occupe ; vidée, ils se dissolvent
    unsafe fn dessiner_corbeille(&self, c: &Cible2D, app: &App, (bx, by): (f32, f32)) {
        const PAS: f32 = 12.0;
        let Some(e) = crate::corbeille::etat() else { return };
        let f = &self.f;
        let survol = app.survol == Cible::Corbeille;
        if survol {
            if let Some(z) = app.zones.corbeille {
                self.couleur(c, &SURVOL);
                c.rt.FillRoundedRectangle(&D2D1_ROUNDED_RECT { rect: z, radiusX: 10.0, radiusY: 10.0 }, &c.pinceau);
            }
        }
        self.couleur(c, if survol { &BLANC } else { &BLANC_DOUX });
        self.texte(c, &f.petit, "CORBEILLE", rect(bx, by, CARTE_BLOC_L - 20.0, 18.0));
        let allumes = crate::corbeille::points(&e);
        // Dissolution : les points allumés avant le vidage s'éteignent un à un, dans le désordre
        let vidage = app.vidage.map(|(t, avant)| (avant, (t.elapsed().as_secs_f32() / 1.2).min(1.0)));
        let (gx, gy) = (bx + 6.0, by + 32.0);
        for n in 0..100usize {
            let (px, py) = (gx + (n % 10) as f32 * PAS, gy + (n / 10) as f32 * PAS);
            let (allume, alpha) = match vidage {
                Some((avant, a)) if n < avant => {
                    let seuil = ((n * 37 + 11) % 100) as f32 / 100.0;
                    (a < seuil, 1.0 - a * 0.6)
                }
                _ => (n < allumes, 1.0),
            };
            if allume {
                let dernier = vidage.map_or(n + 1 == allumes, |(avant, _)| n + 1 == avant);
                let col = if dernier { ACCENT } else { BLANC };
                self.couleur(c, &D2D1_COLOR_F { a: col.a * alpha, ..col });
                c.rt.FillEllipse(&ellipse(px, py, 4.2), &c.pinceau);
            } else {
                self.couleur(c, &BLANC_TRES_FAIBLE);
                c.rt.DrawEllipse(&ellipse(px, py, 3.6), &c.pinceau, 1.0, None);
            }
        }
        let lx = gx + 10.0 * PAS + 8.0;
        let lignes = [
            if e.elements == 0 { "VIDE".to_string() } else { format!("{} · {} ELEMENT{}", systeme::taille(e.octets), e.elements, if e.elements > 1 { "S" } else { "" }) },
            if e.capacite > 0 { format!("RESERVE {}", systeme::taille(e.capacite)) } else { String::new() },
            "CLIC : OUVRIR".to_string(),
            "PALETTE : VIDER".to_string(),
        ];
        for (k, t) in lignes.iter().filter(|t| !t.is_empty()).enumerate() {
            self.couleur(c, if k == 0 { &BLANC } else { &BLANC_DOUX });
            self.texte(c, &f.petit, t, rect(lx + 8.0, by + 26.0 + k as f32 * 16.0, CARTE_BLOC_L - (lx - bx) - 18.0, 18.0));
        }
    }

    /// Glyph choisi pour ce dossier (O1)
    fn glyphe_de(app: &App, e: &Entree) -> Option<&'static [&'static str]> {
        if !matches!(e.genre, Genre::Dossier) {
            return None;
        }
        app.chemin_de(e).and_then(|p| crate::glyphes::de(&p))
    }

    /// Carte du disque (O6) : sous les disques de Ce PC, un carré de 10 × 10 points par disque,
    /// chaque point = 1 % du disque, teinté selon la famille de fichiers ; anneaux = place libre
    unsafe fn dessiner_carte(&self, c: &Cible2D, app: &App, l: f32, y0: f32) {
        const TEINTES: [D2D1_COLOR_F; 7] = [ACCENT, BLANC, D2D1_COLOR_F { a: 0.5, ..ACCENT }, BLANC_DOUX, rgba(255, 255, 255, 0.32), rgba(255, 255, 255, 0.2), rgba(255, 255, 255, 0.1)];
        const PAS: f32 = 12.0;
        const BLOC_L: f32 = CARTE_BLOC_L;
        let f = &self.f;
        let carte = crate::index::carte_prete();
        if carte.is_none() {
            self.couleur(c, &BLANC_DOUX);
            self.texte(c, &f.petit, "CARTE DU DISQUE : LECTURE DE L'INDEX…", rect(largeur_volet() + MARGE_LISTE + 4.0, y0 - 20.0, 400.0, 20.0));
        }
        let nb = disques_carte(app).count();
        self.dessiner_corbeille(c, app, bloc_carte(l, y0, nb));
        let Some(carte) = carte else { return };
        for (k, (racine, libre, total)) in disques_carte(app).enumerate() {
            let (bx, by) = bloc_carte(l, y0, k);
            let familles = carte.iter().find(|d| d.0.eq_ignore_ascii_case(racine)).map(|d| d.1);
            self.couleur(c, &BLANC_DOUX);
            let titre = if familles.is_some() { format!("{racine} · CARTE") } else { format!("{racine} · NON INDEXE") };
            self.texte(c, &f.petit, &titre, rect(bx, by, BLOC_L - 20.0, 18.0));
            // Parts : familles, reste occupé non indexé (système, dossiers exclus), libre
            let occupe = total.saturating_sub(libre);
            let mut octets = [0u64; 8];
            if let Some(fa) = familles {
                octets[..6].copy_from_slice(&fa);
            }
            let indexe: u64 = octets[..6].iter().sum();
            if indexe > occupe {
                // L'index compte des fichiers creux ou compressés à leur taille apparente : on ramène à l'occupé
                for o in octets[..6].iter_mut() {
                    *o = (*o as f64 * occupe as f64 / indexe as f64) as u64;
                }
            }
            octets[6] = occupe.saturating_sub(octets[..6].iter().sum());
            octets[7] = libre.min(total);
            // 100 points, au plus fort reste
            let exact: Vec<f64> = octets.iter().map(|&o| o as f64 * 100.0 / total as f64).collect();
            let mut points: Vec<usize> = exact.iter().map(|e| e.floor() as usize).collect();
            let mut restes: Vec<usize> = (0..8).collect();
            restes.sort_by(|&a, &b| (exact[b] - exact[b].floor()).total_cmp(&(exact[a] - exact[a].floor())));
            for &i in restes.iter().cycle().take(100usize.saturating_sub(points.iter().sum())) {
                points[i] += 1;
            }
            let (gx, gy) = (bx + 6.0, by + 32.0);
            let mut n = 0usize;
            for (fam, &p) in points.iter().enumerate() {
                for _ in 0..p {
                    if n >= 100 {
                        break;
                    }
                    let (px, py) = (gx + (n % 10) as f32 * PAS, gy + (n / 10) as f32 * PAS);
                    if fam == 7 {
                        self.couleur(c, &BLANC_TRES_FAIBLE);
                        c.rt.DrawEllipse(&ellipse(px, py, 3.6), &c.pinceau, 1.0, None);
                    } else {
                        self.couleur(c, &TEINTES[fam]);
                        c.rt.FillEllipse(&ellipse(px, py, 4.2), &c.pinceau);
                    }
                    n += 1;
                }
            }
            // Légende : familles présentes, en pourcentage du disque
            let lx = gx + 10.0 * PAS + 8.0;
            let mut ly = by + 26.0;
            for fam in 0..8 {
                if points[fam] == 0 && !(fam < 6 && octets[fam] > 0) {
                    continue;
                }
                let nom = match fam {
                    6 => "SYSTEME, NON INDEXE",
                    7 => "LIBRE",
                    _ => crate::index::FAMILLES_CARTE[fam],
                };
                if fam == 7 {
                    self.couleur(c, &BLANC_TRES_FAIBLE);
                    c.rt.DrawEllipse(&ellipse(lx + 4.0, ly + 8.0, 3.6), &c.pinceau, 1.0, None);
                } else {
                    self.couleur(c, &TEINTES[fam]);
                    c.rt.FillEllipse(&ellipse(lx + 4.0, ly + 8.0, 4.2), &c.pinceau);
                }
                self.couleur(c, &BLANC_DOUX);
                // La taille reste entière à droite ; c'est le nom qui cède de la place, mot par mot (14 DIP : « SYSTEME, NON INDEXE »
                // et sa taille ne tiennent plus ensemble dans la carte)
                let place = BLOC_L - (lx - bx) - 26.0;
                let taille = systeme::taille(octets[fam]);
                let nom = self.tronquer_aux_mots(nom, place - self.mesurer(&taille, &f.petit) - 8.0);
                self.texte(c, &f.petit, &nom, rect(lx + 16.0, ly, place, 18.0));
                self.texte(c, &f.petit_droite, &taille, rect(lx + 16.0, ly, place, 18.0));
                ly += 16.0;
            }
        }
    }

    /// Vue grille : vignettes de Windows (photos, vidéos, PDF…), sinon la grande icône en points
    unsafe fn dessiner_grille(&self, c: &Cible2D, app: &App, l: f32, focus_clavier: bool) {
        let rt = &c.rt;
        let f = &self.f;
        let cote = app.cote_grille;
        let surlignage = app.texte_surlignage();
        let (par, hr) = app.pas();
        self.couleur(c, &LIGNE);
        rt.FillRectangle(&rect(largeur_volet() + MARGE_LISTE, ENTETE_Y + ENTETE_H, l - largeur_volet() - MARGE_LISTE - MARGE_LISTE_DROITE, 1.0), &c.pinceau);
        rt.PushAxisAlignedClip(&rect(largeur_volet() + 1.0, LISTE_Y, l - largeur_volet() - 1.0, app.hauteur_liste), D2D1_ANTIALIAS_MODE_ALIASED);
        let premiere = (app.defilement / hr).floor().max(0.0) as usize;
        let rangees = (app.hauteur_liste / hr).ceil() as usize + 1;
        let (debut, fin) = (premiere * par, ((premiere + rangees + 1) * par).min(app.entrees.len()));
        let place = |i: usize| (grille_x0() + (i % par) as f32 * (cote + GRILLE_ECART), LISTE_Y + (i / par) as f32 * hr - app.defilement);
        // Lot 12B : les vignettes glissent de leur ancienne case à la nouvelle quand l'ordre, la vue ou le filtre change
        let places: Vec<(&Entree, (f32, f32))> = (debut..fin).map(|i| (&app.entrees[i], place(i))).collect();
        let decalages = self.decalages_flip(app, &places, 1, cote, cote + GRILLE_NOM);
        let couche_flip = self.couche_flip(c, &decalages);
        for i in debut..fin {
            let e = &app.entrees[i];
            let (x, y) = place(i);
            let garde = self.flip_pousser(c, &decalages[i - debut], (x + cote / 2.0, y + (cote + GRILLE_NOM) / 2.0), rect(x - 8.0, y - 8.0, cote + 16.0, cote + GRILLE_NOM + 16.0), &couche_flip);
            let fond = rect(x, y, cote, cote + GRILLE_NOM);
            let arrondi = D2D1_ROUNDED_RECT { rect: fond, radiusX: 12.0, radiusY: 12.0 };
            let choisie = app.marques.contains(&i);
            if choisie {
                self.couleur_ou(c, &SELECTION, &SELECTION_CLAIR);
                rt.FillRoundedRectangle(&arrondi, &c.pinceau);
                self.couleur(c, &BORDURE_SELECTION_GRILLE);
                // Trait de 1 DIP rentré d'une demi-épaisseur : sur la première rangée, centré sur le bord il était rogné par le clip
                rt.DrawRoundedRectangle(&D2D1_ROUNDED_RECT { rect: rect(x + 0.5, y + 0.5, cote - 1.0, cote + GRILLE_NOM - 1.0), radiusX: 11.5, radiusY: 11.5 }, &c.pinceau, 1.0, None);
            } else if app.survol == Cible::Ligne(i) {
                self.couleur(c, &SURVOL);
                rt.FillRoundedRectangle(&arrondi, &c.pinceau);
            }
            if app.depot == Cible::Ligne(i) {
                self.couleur(c, &ACCENT);
                rt.DrawRoundedRectangle(&arrondi, &c.pinceau, 1.5, None);
            }
            if app.selection == Some(i) && !choisie {
                self.couleur(c, &BLANC_TRES_FAIBLE);
                rt.DrawRoundedRectangle(&arrondi, &c.pinceau, 1.0, None);
            }
            let chemin = app.chemin_de(e);
            let image = match (&chemin, &e.genre) {
                (Some(ch), Genre::Fichier) if e.nuage != Nuage::EnLigne => self.vignette(c, ch),
                _ => None,
            };
            let carre = rect(x + 6.0, y + 6.0, cote - 12.0, cote - 12.0);
            match image {
                Some(b) => {
                    let t = b.GetSize();
                    let k = ((carre.right - carre.left) / t.width).min((carre.bottom - carre.top) / t.height);
                    let (iw, ih) = (t.width * k, t.height * k);
                    let zone = rect(carre.left + (cote - iw) / 2.0, carre.top + (carre.bottom - carre.top - ih) / 2.0, iw, ih);
                    rt.DrawBitmap(&b, Some(&zone), 1.0, D2D1_BITMAP_INTERPOLATION_MODE_LINEAR, None);
                    self.couleur(c, &LIGNE);
                    rt.DrawRectangle(&zone, &c.pinceau, 1.0, None);
                }
                None => {
                    // Icône de 40 DIP, indépendante du zoom de la vignette
                    let pas = 6.6;
                    match Self::glyphe_de(app, e) {
                        Some(m) => self.motif_points(c, m, x + cote / 2.0, y + cote / 2.0, pas, choisie, if e.cache { 0.4 } else { 1.0 }),
                        None => match icones::pastille_de(e) {
                            // 40 DIP, rayon 10 (brief du lot 11 ; la maquette dit 9)
                            Some(p) => self.pastille(c, p, x + cote / 2.0, y + cote / 2.0, 40.0, 10.0, if e.cache { 0.4 } else { 1.0 }),
                            None => self.icone_grande(c, icones::pour(e), x + cote / 2.0, y + cote / 2.0, pas, choisie, if e.cache { 0.4 } else { 1.0 }),
                        },
                    }
                }
            }
            if let Some(b) = chemin.as_deref().and_then(outils::balise) {
                self.couleur(c, &Self::couleur_balise(b));
                rt.FillEllipse(&ellipse(x + 6.0, y + 12.0, 3.2), &c.pinceau);
            }
            if let Some(g) = app.etat_git(e) {
                self.couleur(c, if g == '?' { &BLANC_DOUX } else { &ACCENT });
                self.texte(c, &f.petit, &g.to_string(), rect(x + cote - 14.0, y + 3.0, 14.0, 18.0));
            }
            // Étiquette (lot 16) : les carrés au bas de la vignette, à gauche (le nom centré dessous garde toute sa place)
            if let Some(n) = chemin.as_deref().filter(|_| crate::etiquettes::existe_une()).and_then(crate::etiquettes::de) {
                self.carres_etiquette(c, x + 8.0, y + cote - 10.0, n);
            }
            if app.survol == Cible::Ligne(i) || (choisie && app.marques.len() > 1) {
                // Disque sous la case : noir translucide en sombre, surface claire en clair (pas un voile de fond)
                c.pinceau.SetColor(&crate::theme::contraste_ou(&DISQUE_CASE, &DISQUE_CASE_CLAIR));
                rt.FillEllipse(&ellipse(x + 13.0, y + 19.0, 10.0), &c.pinceau);
                self.case_a_cocher(c, x + 13.0, y + 19.0, choisie, false);
            }
            self.couleur(c, if choisie || !e.cache { &BLANC } else { &BLANC_FAIBLE });
            self.texte_w(c, &f.petit_centre, nom_vu(e), rect(x + 4.0, y + cote, cote - 8.0, GRILLE_NOM - 4.0));
            self.souligner(c, &f.petit_centre, nom_vu(e), &surlignage, rect(x + 4.0, y + cote, cote - 8.0, GRILLE_NOM - 4.0));
            if focus_clavier && app.selection == Some(i) {
                self.anneau_dans(c, fond, 12.0);
            }
            self.flip_retirer(c, garde);
        }
        // Lot 12D : la vignette supprimée finit sa sortie (opacité 1 → 0, échelle 1 → .92, 220 ms)
        for (ligne, r, d) in &self.lignes_sortantes(1) {
            let (x, y) = (r[0], r[1] - app.defilement);
            let garde = self.flip_pousser(c, d, (x + r[2] / 2.0, y + r[3] / 2.0), rect(x - 8.0, y - 8.0, r[2] + 16.0, r[3] + 16.0), &self.couche_opacite(c, d.opacite));
            let e = &ligne.entree;
            if ligne.choisie {
                self.couleur_ou(c, &SELECTION, &SELECTION_CLAIR);
                rt.FillRoundedRectangle(&D2D1_ROUNDED_RECT { rect: rect(x, y, r[2], r[3]), radiusX: 12.0, radiusY: 12.0 }, &c.pinceau);
            }
            let (cx, cy) = (x + r[2] / 2.0, y + r[2] / 2.0);
            match Self::glyphe_de(app, e) {
                Some(m) => self.motif_points(c, m, cx, cy, 6.6, ligne.choisie, 1.0),
                None => match icones::pastille_de(e) {
                    Some(p) => self.pastille(c, p, cx, cy, 40.0, 10.0, 1.0),
                    None => self.icone_grande(c, icones::pour(e), cx, cy, 6.6, ligne.choisie, 1.0),
                },
            }
            self.couleur(c, &BLANC);
            self.texte_w(c, &f.petit_centre, nom_vu(e), rect(x + 4.0, y + r[2], r[2] - 8.0, GRILLE_NOM - 4.0));
            self.flip_retirer(c, garde);
        }
        rt.PopAxisAlignedClip();
    }

    /// Une colonne de la vue Miller : icône et nom, flèche en points pour les dossiers
    unsafe fn colonne_miller(&self, c: &Cible2D, entrees: &[Entree], (x, w): (f32, f32), decalage: f32, hauteur: f32, choisie: impl Fn(usize) -> (bool, bool), survol: Option<usize>, balise: impl Fn(&Entree) -> Option<outils::Balise>, etiquette: impl Fn(&Entree) -> Option<u8>, decalages: &[mouvement::Decalage], surlignage: &str) {
        let rt = &c.rt;
        let (premiere, nb) = Self::etendue_miller(decalage, hauteur);
        let couche_flip = self.couche_flip(c, decalages);
        for i in premiere..(premiere + nb).min(entrees.len()) {
            let e = &entrees[i];
            let y = LISTE_Y + i as f32 * hauteur_ligne() - decalage;
            // Lot 12B : seule la colonne du dossier ouvert reçoit des décalages (les deux autres : tranche vide)
            let garde = match decalages.get(i - premiere) {
                Some(d) => self.flip_pousser(c, d, (x + w / 2.0, y + hauteur_ligne() / 2.0), rect(x - 8.0, y - 8.0, w + 16.0, hauteur_ligne() + 16.0), &couche_flip),
                None => None,
            };
            let fond = rect(x, y + 1.0, w, hauteur_ligne() - 2.0);
            let arrondi = D2D1_ROUNDED_RECT { rect: fond, radiusX: 7.0, radiusY: 7.0 };
            let (marque, focus) = choisie(i);
            if marque {
                self.couleur(c, &ACCENT_SELECTION);
                rt.FillRoundedRectangle(&arrondi, &c.pinceau);
                self.couleur(c, &ACCENT);
                rt.FillEllipse(&ellipse(x + 5.0, y + hauteur_ligne() / 2.0, 2.2), &c.pinceau);
            } else if focus {
                self.couleur(c, &SURVOL_FORT);
                rt.FillRoundedRectangle(&arrondi, &c.pinceau);
            } else if survol == Some(i) {
                self.couleur(c, &SURVOL);
                rt.FillRoundedRectangle(&arrondi, &c.pinceau);
            }
            let cy = y + hauteur_ligne() / 2.0;
            self.icone(c, icones::pour(e), x + 16.0, cy, marque, if e.cache { 0.4 } else { 1.0 });
            if let Some(b) = balise(e) {
                self.couleur(c, &Self::couleur_balise(b));
                rt.FillEllipse(&ellipse(x + 3.5, cy, 2.6), &c.pinceau);
            }
            let fleche = if e.est_conteneur() { 16.0 } else { 0.0 };
            self.couleur(c, if marque { &BLANC } else if e.cache { &BLANC_FAIBLE } else if focus { &BLANC } else { &BLANC_DOUX });
            self.texte_w(c, &self.f.liste, nom_vu(e), rect(x + 34.0, y, w - 40.0 - fleche, hauteur_ligne()));
            self.souligner(c, &self.f.liste, nom_vu(e), surlignage, rect(x + 34.0, y, w - 40.0 - fleche, hauteur_ligne()));
            if let Some(n) = etiquette(e) {
                let x_carres = x + 34.0 + self.mesurer_nom(e, &self.f.liste) + 8.0;
                if x_carres + largeur_etiquette(n) <= x + w - 8.0 - fleche {
                    self.carres_etiquette(c, x_carres, cy, n);
                }
            }
            if fleche > 0.0 {
                self.couleur(c, if marque { &BLANC } else { &BLANC_TRES_FAIBLE });
                self.points(c, x + w - 10.0, cy, 2.6, 1.0, &[(-1, -1), (0, 0), (-1, 1)]);
            }
            self.flip_retirer(c, garde);
        }
    }

    /// Première ligne visible et nombre de lignes dessinées d'une colonne de la vue Miller
    fn etendue_miller(decalage: f32, hauteur: f32) -> (usize, usize) {
        ((decalage / hauteur_ligne()).floor().max(0.0) as usize, (hauteur / hauteur_ligne()).ceil() as usize + 2)
    }

    /// Vue colonnes (Miller) : dossier parent, dossier ouvert, contenu de l'élément choisi
    unsafe fn dessiner_miller(&self, c: &Cible2D, app: &App, l: f32) {
        let rt = &c.rt;
        let f = &self.f;
        let cols = colonnes_miller(l);
        let choisi = app.selection.and_then(|i| app.entrees.get(i));
        let titres = [
            app.emplacement.parent().map(|p| p.titre()).unwrap_or_default(),
            app.emplacement.titre(),
            choisi.map(|e| e.nom.to_uppercase()).unwrap_or_default(),
        ];
        for (k, (x, w)) in cols.iter().enumerate() {
            self.couleur(c, if k == 1 { &BLANC } else { &BLANC_DOUX });
            self.texte(c, &f.petit, &titres[k], rect(x + 8.0, ENTETE_Y, w - 16.0, ENTETE_H));
            if k > 0 {
                self.couleur(c, &LIGNE);
                rt.FillRectangle(&rect(x - 4.0, ENTETE_Y, 1.0, LISTE_Y - ENTETE_Y + app.hauteur_liste), &c.pinceau);
            }
        }
        self.couleur(c, &LIGNE);
        rt.FillRectangle(&rect(largeur_volet() + MARGE_LISTE, ENTETE_Y + ENTETE_H, l - largeur_volet() - MARGE_LISTE - MARGE_LISTE_DROITE, 1.0), &c.pinceau);
        rt.PushAxisAlignedClip(&rect(largeur_volet() + 1.0, LISTE_Y, l - largeur_volet() - 1.0, app.hauteur_liste), D2D1_ANTIALIAS_MODE_ALIASED);

        // Parent : le dossier ouvert y est marqué, et ramené dans la zone visible
        let courant = app.emplacement.nom().map(str::to_lowercase);
        let i_courant = app.miller_parent.iter().position(|e| Some(e.nom.to_lowercase()) == courant);
        let max0 = (app.miller_parent.len() as f32 * hauteur_ligne() - app.hauteur_liste).max(0.0);
        let dec0 = i_courant.map_or(0.0, |i| ((i as f32 + 0.5) * hauteur_ligne() - app.hauteur_liste / 2.0).clamp(0.0, max0));
        self.colonne_miller(c, &app.miller_parent, cols[0], dec0, app.hauteur_liste, |i| (false, Some(i) == i_courant), None, |_| None, |_| None, &[], "");

        // Dossier ouvert : sélection et défilement de l'onglet
        let survol = match app.survol {
            Cible::Ligne(i) => Some(i),
            _ => None,
        };
        // Lot 12B : les lignes du dossier ouvert glissent comme celles de la liste (les colonnes voisines, non)
        let (premiere, nb) = Self::etendue_miller(app.defilement, app.hauteur_liste);
        let fin = (premiere + nb).min(app.entrees.len());
        let places: Vec<(&Entree, (f32, f32))> = (premiere..fin).map(|i| (&app.entrees[i], (cols[1].0, LISTE_Y + i as f32 * hauteur_ligne() - app.defilement))).collect();
        let decalages = self.decalages_flip(app, &places, 2, cols[1].1, hauteur_ligne());
        self.colonne_miller(c, &app.entrees, cols[1], app.defilement, app.hauteur_liste, |i| (app.marques.contains(&i), app.selection == Some(i)), survol, |e| app.chemin_de(e).and_then(|p| outils::balise(&p)), |e| if crate::etiquettes::existe_une() { app.chemin_de(e).and_then(|p| crate::etiquettes::de(&p)) } else { None }, &decalages, &app.texte_surlignage());
        // Lot 12D : la ligne supprimée finit sa sortie dans la colonne du dossier ouvert
        for (ligne, r, d) in &self.lignes_sortantes(2) {
            let (x, y) = (r[0], r[1] - app.defilement);
            let garde = self.flip_pousser(c, d, (x + r[2] / 2.0, y + r[3] / 2.0), rect(x - 8.0, y - 8.0, r[2] + 16.0, r[3] + 16.0), &self.couche_opacite(c, d.opacite));
            if ligne.choisie {
                self.couleur(c, &ACCENT_SELECTION);
                rt.FillRoundedRectangle(&D2D1_ROUNDED_RECT { rect: rect(x, y + 1.0, r[2], r[3] - 2.0), radiusX: 7.0, radiusY: 7.0 }, &c.pinceau);
            }
            self.icone(c, icones::pour(&ligne.entree), x + 16.0, y + r[3] / 2.0, ligne.choisie, 1.0);
            self.couleur(c, if ligne.choisie { &BLANC } else { &BLANC_DOUX });
            self.texte_w(c, &f.liste, nom_vu(&ligne.entree), rect(x + 34.0, y, r[2] - 40.0, r[3]));
            self.flip_retirer(c, garde);
        }

        // Élément choisi : son contenu, ou ses informations si c'est un fichier
        let (x, w) = cols[2];
        match choisi {
            Some(e) if e.est_conteneur() => {
                if app.miller_enfant.is_empty() {
                    self.couleur(c, &BLANC_TRES_FAIBLE);
                    self.texte(c, &f.petit, "VIDE", rect(x + 10.0, LISTE_Y + 6.0, w - 20.0, 24.0));
                }
                self.colonne_miller(c, &app.miller_enfant, cols[2], 0.0, app.hauteur_liste, |_| (false, false), None, |_| None, |_| None, &[], "");
            }
            Some(e) => {
                self.icone_grande(c, icones::pour(e), x + w / 2.0, LISTE_Y + 70.0, 13.0, false, 1.0);
                let mut y = LISTE_Y + 150.0;
                self.couleur(c, &BLANC);
                self.texte_w(c, &f.liste, nom_vu(e), rect(x + 10.0, y, w - 20.0, 24.0));
                y += 34.0;
                for (lib, val) in [("TYPE", icones::nom_type(e).to_string()), ("TAILLE", systeme::taille(e.taille)), ("MODIFIÉ LE", systeme::date(e.modif))] {
                    self.couleur(c, &BLANC_DOUX);
                    self.texte(c, &f.petit, lib, rect(x + 10.0, y, w - 20.0, 18.0));
                    self.couleur(c, &BLANC_DOUX);
                    self.texte(c, &f.valeur, &val, rect(x + 10.0, y + 15.0, w - 20.0, 20.0));
                    y += 42.0;
                }
            }
            None => {}
        }
        rt.PopAxisAlignedClip();
    }

    /// Trame de points sur un rectangle. Réglage « Trame de points » coupé : rien n'est peint, le fond déjà posé dessous
    /// reste un aplat. Tous les remplissages au pinceau trame passent par ici ou par `trame_arrondie`.
    unsafe fn trame_plein(&self, c: &Cible2D, r: &D2D_RECT_F) {
        if crate::reglages::trame_active() {
            c.rt.FillRectangle(r, &c.trame);
        }
    }

    unsafe fn trame_arrondie(&self, c: &Cible2D, r: &D2D1_ROUNDED_RECT) {
        if crate::reglages::trame_active() {
            c.rt.FillRoundedRectangle(r, &c.trame);
        }
    }

    /// Carte posée par-dessus la fenêtre : fond, trame, contour
    unsafe fn carte(&self, c: &Cible2D, r: D2D_RECT_F) {
        let arrondi = D2D1_ROUNDED_RECT { rect: r, radiusX: 14.0, radiusY: 14.0 };
        self.couleur(c, &CHAMP_FOND);
        c.rt.FillRoundedRectangle(&arrondi, &c.pinceau);
        self.trame_arrondie(c, &arrondi);
        self.couleur(c, &BLANC_TRES_FAIBLE);
        c.rt.DrawRoundedRectangle(&arrondi, &c.pinceau, 1.0, None);
    }

    /// Pixel, les deux yeux : une seule instance à l'écran. Dans la barre d'état (`place` 0), à gauche du champ de la palette
    /// (`place` 1) ou en vol entre les deux. Forme et couleur viennent de `pixel.rs` ; les jetons de `theme.rs` passent par `couleur`.
    unsafe fn dessiner_pixel(&self, c: &Cible2D, v: &VuePixel, l: f32, h: f32) {
        // Réglage « Yeux de Pixel » coupé : rien n'est peint, la place de la boîte reste réservée (la barre d'état ne bouge pas)
        if !crate::reglages::yeux_pixel_actifs() {
            return;
        }
        let (x0, y0) = pixel_dans_statut(h);
        let (x1, y1) = pixel_dans_palette(l);
        let t = v.place.clamp(0.0, 1.0);
        let (cx, cy) = (x0 + (x1 - x0) * t, y0 + (y1 - y0) * t);
        // Regard (`--lx`, `--ly` : le pointeur dans la barre d'état, la ligne choisie dans la palette) ; la nuit écrase les barres
        // (`scaleY(.14)`), les yeux contents de la palette les ramènent à 5 DIP
        let forme = pixel::forme_affichee(v.etat, v.phase, v.yeux.aplatie).contente(v.yeux.contents).decalee(v.yeux.dx, v.yeux.dy);
        // Fondu de couleur de 200 ms entre l'ancienne teinte et la nouvelle (`transition: background-color .2s`)
        let (avant, apres, k) = (couleur_teinte(v.yeux.teinte_avant), couleur_teinte(v.yeux.teinte), v.yeux.fondu);
        let couleur = D2D1_COLOR_F { r: avant.r + (apres.r - avant.r) * k, g: avant.g + (apres.g - avant.g) * k, b: avant.b + (apres.b - avant.b) * k, a: avant.a + (apres.a - avant.a) * k };
        self.couleur(c, &couleur);
        for e in &forme.elements {
            match *e {
                pixel::Element::Barre { cx: bx, cy: by, l: bl, h: bh, rayon } => {
                    let r = rect(cx + bx - bl / 2.0, cy + by - bh / 2.0, bl, bh);
                    c.rt.FillRoundedRectangle(&D2D1_ROUNDED_RECT { rect: r, radiusX: rayon, radiusY: rayon }, &c.pinceau);
                }
                pixel::Element::Trait { x1, y1, x2, y2 } => {
                    c.rt.DrawLine(D2D_POINT_2F { x: cx + x1, y: cy + y1 }, D2D_POINT_2F { x: cx + x2, y: cy + y2 }, &c.pinceau, forme.epaisseur, Some(&self.trait_rond));
                }
            }
        }
    }

    /// État vide de la liste (`.empty` de la maquette) : les yeux ouverts (`eyeSvg('open')`, 26 × 14, deux barres de 5 × 12 à rayon 1,5
    /// en x = 4 et 17) puis, 14 DIP plus bas, la phrase en Segoe 14 couleur `--i2`. Le bloc est centré dans `zone`. La couleur suit la
    /// période de Pixel (`--px`) ; la nuit écrase les yeux à 14 % comme ceux de la barre d'état.
    unsafe fn dessiner_etat_vide(&self, c: &Cible2D, phrase: &str, periode: Periode, zone: D2D_RECT_F) {
        const YEUX_H: f32 = 14.0;
        const ECART: f32 = 14.0;
        const TEXTE_H: f32 = 20.0;
        let bloc = YEUX_H + ECART + TEXTE_H;
        let cx = (zone.left + zone.right) / 2.0;
        let haut = ((zone.top + zone.bottom) / 2.0 - bloc / 2.0).max(zone.top);
        let (teinte, echelle) = pixel::yeux_vides(periode);
        self.couleur(c, &couleur_teinte(teinte));
        for x in [4.0f32, 17.0] {
            let hauteur = 12.0 * echelle;
            let r = rect(cx - 13.0 + x, haut + YEUX_H / 2.0 - hauteur / 2.0, 5.0, hauteur);
            let rayon = 1.5f32.min(hauteur / 2.0);
            c.rt.FillRoundedRectangle(&D2D1_ROUNDED_RECT { rect: r, radiusX: rayon, radiusY: rayon }, &c.pinceau);
        }
        self.couleur(c, &BLANC_DOUX);
        self.texte(c, &self.f.vide, phrase, rect(zone.left + 24.0, haut + YEUX_H + ECART, (zone.right - zone.left - 48.0).max(0.0), TEXTE_H));
    }

    /// Un trait de bouts ronds (signes des pastilles : coche, croix, flèche)
    unsafe fn trait_signe(&self, c: &Cible2D, (x1, y1): (f32, f32), (x2, y2): (f32, f32), epaisseur: f32) {
        c.rt.DrawLine(D2D_POINT_2F { x: x1, y: y1 }, D2D_POINT_2F { x: x2, y: y2 }, &c.pinceau, epaisseur, Some(&self.trait_rond));
    }

    /// Les pastilles au-dessus de la barre d'état. `grille` : phase de la grille 3 × 3 (0 à 1) ; `None` en mouvement réduit,
    /// la grille est alors allumée et immobile.
    /// Les cartes d'opération au-dessus de la barre d'état, avec leur entrée, leurs glissements et leurs sorties (lot 12D).
    /// `decalages` : un par place ; `deploiements` : (largeur, opacité du `.more`) par pastille de `vue.pastilles`.
    unsafe fn dessiner_pastilles(&self, c: &Cible2D, d: &Disposition, vue: &Vue, survol: Cible, decalages: &[mouvement::Decalage], deploiements: &[(f32, f32)]) {
        let rt = &c.rt;
        let f = &self.f;
        let (t, animer) = (self.t_image.get(), crate::reglages::animations_actives());
        for (pl, dec) in d.places.iter().zip(decalages) {
            let Some(p) = vue.pastilles.get(pl.index) else { continue };
            let more = deploiements.get(pl.index).map_or(1.0, |m| m.1);
            let pleins = ((p.avancement * pastilles::BARRES as f32).floor() as usize).min(pastilles::BARRES);
            let barres = self.mouvements.borrow_mut().cartes.jauge(p, pleins, t, animer);
            let z = pl.zone;
            let garde = self.flip_pousser(c, dec, (z.x + z.l / 2.0, z.y + z.h / 2.0), rect(z.x - 24.0, z.y - 40.0, pl.pleine + 48.0, z.h + 80.0), &self.couche_opacite(c, dec.opacite));
            self.dessiner_une_pastille(c, pl, p, vue.pixel.grille, survol, more, &barres);
            self.flip_retirer(c, garde);
        }
        // Cartes qui viennent de disparaître : elles finissent leur sortie (240 ms, `ease-in`) à la place qu'elles occupaient
        let fantomes: Vec<(Place, Pastille, [f32; 4], mouvement::Decalage)> = {
            let m = self.mouvements.borrow();
            m.cartes.fantomes.iter().filter_map(|g| g.decalage(t).map(|dec| (g.contenu.0.clone(), g.contenu.1.clone(), g.rect, dec))).collect()
        };
        for (pl, p, r, dec) in &fantomes {
            // Le fantôme est posé là où la carte était affichée (le glissement de départ compris)
            let place = translater_place(pl, r[0] - pl.zone.x, r[1] - pl.zone.y);
            let pleins = ((p.avancement * pastilles::BARRES as f32).floor() as usize).min(pastilles::BARRES);
            let mut barres = [0.0f32; 16];
            for (k, b) in barres.iter_mut().enumerate() {
                *b = f32::from(k < pleins);
            }
            let garde = self.flip_pousser(c, dec, (r[0] + r[2] / 2.0, r[1] + r[3] / 2.0), rect(r[0] - 24.0, r[1] - 40.0, place.pleine + 48.0, r[3] + 80.0), &self.couche_opacite(c, dec.opacite));
            self.dessiner_une_pastille(c, &place, p, vue.pixel.grille, Cible::Rien, 1.0, &barres);
            self.flip_retirer(c, garde);
        }
        // Pastilles qui ne tiennent pas : « +N »
        if let Some((n, z)) = d.plus {
            self.couleur(c, &CHAMP_FOND);
            rt.FillRoundedRectangle(&D2D1_ROUNDED_RECT { rect: rect(z.x, z.y, z.l, z.h), radiusX: pastilles::RAYON, radiusY: pastilles::RAYON }, &c.pinceau);
            self.couleur(c, &LIGNE);
            rt.DrawRoundedRectangle(&D2D1_ROUNDED_RECT { rect: rect(z.x + 0.5, z.y + 0.5, z.l - 1.0, z.h - 1.0), radiusX: pastilles::RAYON, radiusY: pastilles::RAYON }, &c.pinceau, 1.0, None);
            self.couleur(c, &BLANC_DOUX);
            self.texte(c, &f.mono_centre, &format!("+{n}"), rect(z.x, z.y, z.l, z.h));
        }
        // Infobulle de la puce de destination : le chemin complet
        if let Cible::BoutonPastille(i, Bouton::Destination) = survol {
            if let (Some(pl), Some(chemin)) = (d.places.iter().find(|p| p.index == i), vue.pastilles.get(i).and_then(|p| p.destination.as_deref())) {
                let w = (self.mesurer(chemin, &f.petit_mono) + 20.0).min(720.0);
                let r = rect(pl.zone.x, pl.zone.y - 30.0, w, 24.0);
                let arrondi = D2D1_ROUNDED_RECT { rect: r, radiusX: 8.0, radiusY: 8.0 };
                self.couleur(c, &CHAMP_FOND);
                rt.FillRoundedRectangle(&arrondi, &c.pinceau);
                self.couleur(c, &BLANC_TRES_FAIBLE);
                rt.DrawRoundedRectangle(&arrondi, &c.pinceau, 1.0, None);
                self.couleur(c, &BLANC);
                self.texte(c, &f.petit_mono, chemin, rect(r.left + 10.0, r.top, w - 14.0, 24.0));
            }
        }
    }

    /// Une carte d'opération : fond, tête, nom, jauge, mot de droite, puis le `.more` (message, puce, boutons) rogné et fondu.
    /// `barres` : opacité d'allumage des 16 barres de la jauge.
    unsafe fn dessiner_une_pastille(&self, c: &Cible2D, pl: &Place, p: &Pastille, grille: Option<f32>, survol: Cible, more: f32, barres: &[f32; 16]) {
        let rt = &c.rt;
        let f = &self.f;
        {
            let z = pl.zone;
            let attente = p.genre == GenrePastille::Attente;
            let corps = D2D1_ROUNDED_RECT { rect: rect(z.x, z.y, z.l, z.h), radiusX: pastilles::RAYON, radiusY: pastilles::RAYON };
            self.couleur(c, &CHAMP_FOND);
            rt.FillRoundedRectangle(&corps, &c.pinceau);
            // Trait : discret ; en échec, la couleur d'erreur (avec le mot ERREUR et la croix : jamais seule)
            let trait_ = if p.genre == GenrePastille::Erreur { D2D1_COLOR_F { a: 0.55, ..ERREUR } } else if attente { LIGNE } else { BLANC_TRES_FAIBLE };
            self.couleur(c, &trait_);
            rt.DrawRoundedRectangle(&D2D1_ROUNDED_RECT { rect: rect(z.x + 0.5, z.y + 0.5, z.l - 1.0, z.h - 1.0), radiusX: pastilles::RAYON, radiusY: pastilles::RAYON }, &c.pinceau, 1.0, None);
            // Liseré clair en haut (maquette : inset 0 1px 0)
            self.couleur(c, &SURVOL_FORT);
            rt.FillRectangle(&rect(z.x + pastilles::RAYON, z.y + 1.0, (z.l - 2.0 * pastilles::RAYON).max(0.0), 1.0), &c.pinceau);
            // Déploiement en cours : l'intérieur est celui de la carte déployée, rogné à la partie déjà révélée
            let rogne = pl.pleine > z.l + 0.5;
            if rogne {
                rt.PushAxisAlignedClip(&rect(z.x, z.y, z.l, z.h), D2D1_ANTIALIAS_MODE_ALIASED);
            }

            // Tête : disque de 24 DIP et son signe
            let (tx, ty) = (pl.tete.x + pastilles::TETE / 2.0, pl.tete.y + pastilles::TETE / 2.0);
            if !attente {
                self.couleur(c, &SURVOL_FORT);
                rt.FillEllipse(&ellipse(tx, ty, pastilles::TETE / 2.0), &c.pinceau);
            }
            match p.tete {
                Tete::Grille => {
                    // 9 points de 4 DIP, pas de 6 ; l'opacité tourne en diagonale, un tour en 1,2 s, décalage de 0,12 s par diagonale (maquette : .hyp)
                    for ligne in 0..3 {
                        for col in 0..3 {
                            let alpha = match (grille, p.en_pause) {
                                (Some(t), false) => {
                                    let phase = (t - (ligne + col) as f32 * 0.1).rem_euclid(1.0);
                                    if phase < 0.4 { 0.25 + 0.75 * phase / 0.4 } else { 1.0 - 0.75 * (phase - 0.4) / 0.6 }
                                }
                                // Pause ou mouvement réduit : allumée, immobile
                                (_, true) => 0.5,
                                (None, false) => 1.0,
                            };
                            self.couleur(c, &D2D1_COLOR_F { a: alpha, ..ACCENT });
                            let (px, py) = (tx + (col as f32 - 1.0) * 6.0, ty + (ligne as f32 - 1.0) * 6.0);
                            rt.FillRoundedRectangle(&D2D1_ROUNDED_RECT { rect: rect(px - 2.0, py - 2.0, 4.0, 4.0), radiusX: 0.8, radiusY: 0.8 }, &c.pinceau);
                        }
                    }
                }
                Tete::Coche => {
                    self.couleur(c, &BLANC_DOUX);
                    self.trait_signe(c, (tx - 4.5, ty + 0.5), (tx - 1.5, ty + 3.5), 1.8);
                    self.trait_signe(c, (tx - 1.5, ty + 3.5), (tx + 4.5, ty - 3.5), 1.8);
                }
                Tete::Croix => {
                    self.couleur(c, &ERREUR);
                    self.trait_signe(c, (tx - 3.5, ty - 3.5), (tx + 3.5, ty + 3.5), 1.8);
                    self.trait_signe(c, (tx - 3.5, ty + 3.5), (tx + 3.5, ty - 3.5), 1.8);
                }
                Tete::Vide => {
                    self.couleur(c, &BLANC_FAIBLE);
                    rt.DrawEllipse(&ellipse(tx, ty, 5.0), &c.pinceau, 1.2, None);
                }
                Tete::CroixGrise => {
                    // Annulé : la même croix que l'erreur, mais grise (on ne sait pas si c'est un échec)
                    self.couleur(c, &BLANC_DOUX);
                    self.trait_signe(c, (tx - 3.5, ty - 3.5), (tx + 3.5, ty + 3.5), 1.8);
                    self.trait_signe(c, (tx - 3.5, ty + 3.5), (tx + 3.5, ty - 3.5), 1.8);
                }
                Tete::Retour => {
                    // Flèche de retour (↶ simplifiée) : l'action peut être défaite
                    self.couleur(c, &BLANC_DOUX);
                    self.trait_signe(c, (tx - 5.0, ty), (tx + 4.0, ty), 1.8);
                    self.trait_signe(c, (tx - 2.0, ty - 3.0), (tx - 5.0, ty), 1.8);
                    self.trait_signe(c, (tx - 2.0, ty + 3.0), (tx - 5.0, ty), 1.8);
                }
            }

            // Nom court
            self.couleur(c, if attente { &BLANC_DOUX } else { &BLANC });
            // Le rectangle de découpe dépasse de 3 DIP (l'écart de 10 DIP avant le mot de droite le permet) : à largeur exacte, la
            // découpe d'Direct2D mangeait le dernier point du « o » de « lent.iso »
            self.texte(c, &f.volet, &pl.libelle, rect(pl.zone_libelle.x, pl.zone_libelle.y, pl.zone_libelle.l + 3.0, pl.zone_libelle.h));

            // Jauge de 16 barres et pourcentage
            if let (Some(j), Some(pc)) = (pl.jauge, pl.pourcent) {
                for k in 0..pastilles::BARRES {
                    // `.dp i` : le fond d'une barre passe de l'éteint à l'allumé en 200 ms
                    let m = barres[k];
                    self.couleur(c, &D2D1_COLOR_F { r: BLANC_TRES_FAIBLE.r + (ACCENT.r - BLANC_TRES_FAIBLE.r) * m, g: BLANC_TRES_FAIBLE.g + (ACCENT.g - BLANC_TRES_FAIBLE.g) * m, b: BLANC_TRES_FAIBLE.b + (ACCENT.b - BLANC_TRES_FAIBLE.b) * m, a: BLANC_TRES_FAIBLE.a + (ACCENT.a - BLANC_TRES_FAIBLE.a) * m });
                    let x = j.x + k as f32 * (pastilles::BARRE_L + pastilles::BARRE_ECART);
                    rt.FillRoundedRectangle(&D2D1_ROUNDED_RECT { rect: rect(x, j.y, pastilles::BARRE_L, j.h), radiusX: 1.0, radiusY: 1.0 }, &c.pinceau);
                }
                if p.en_pause {
                    self.couleur(c, &BLANC_DOUX);
                    self.texte(c, &f.volet_droite, "PAUSE", rect(pc.x, pc.y, pc.l, pc.h));
                } else {
                    self.couleur(c, &ACCENT);
                    self.texte(c, &f.volet_droite, &format!("{} %", (p.avancement * 100.0).round() as i32), rect(pc.x, pc.y, pc.l, pc.h));
                }
            }
            // Mot de droite et message d'erreur
            if let Some((t, ez)) = &pl.etiquette {
                self.couleur(c, if p.genre == GenrePastille::Erreur { &ERREUR } else if attente { &BLANC_FAIBLE } else { &BLANC_DOUX });
                self.texte(c, &f.petit_mono, t, rect(ez.x, ez.y, ez.l + 2.0, ez.h));
            }
            if let Some(tz) = pl.touches {
                self.touches(c, "CTRL+Z", tz.x, tz.y + tz.h / 2.0, true);
            }

            // `.more` : message, puce de destination et boutons se révèlent avec la largeur de la carte et apparaissent en 200 ms
            let couche_more = self.couche_opacite(c, more);
            let garde_more = self.flip_pousser(c, &mouvement::Decalage { dx: 0.0, dy: 0.0, echelle: 1.0, opacite: more }, (z.x, z.y), rect(z.x - 4.0, z.y - 4.0, pl.pleine + 8.0, z.h + 8.0), &couche_more);
            if let Some((t, mz)) = &pl.message {
                self.couleur(c, &BLANC_DOUX);
                self.texte(c, &f.petit_mono, t, rect(mz.x, mz.y, mz.l + 2.0, mz.h));
            }

            // Boutons de la pastille déployée
            for (b, bz, texte) in &pl.boutons {
                let survole = survol == Cible::BoutonPastille(pl.index, *b);
                let forme = D2D1_ROUNDED_RECT { rect: rect(bz.x, bz.y, bz.l, bz.h), radiusX: if *b == Bouton::Destination { 8.0 } else { 10.0 }, radiusY: if *b == Bouton::Destination { 8.0 } else { 10.0 } };
                self.couleur(c, if survole { &SURVOL_FORT } else { &SURVOL });
                rt.FillRoundedRectangle(&forme, &c.pinceau);
                self.couleur(c, &BLANC_TRES_FAIBLE);
                rt.DrawRoundedRectangle(&D2D1_ROUNDED_RECT { rect: rect(bz.x + 0.5, bz.y + 0.5, bz.l - 1.0, bz.h - 1.0), ..forme }, &c.pinceau, 1.0, None);
                let (bx, by) = (bz.x + bz.l / 2.0, bz.y + bz.h / 2.0);
                match b {
                    Bouton::Destination => {
                        if let Some((nom, _)) = &pl.destination {
                            self.couleur(c, if survole { &BLANC } else { &BLANC_DOUX });
                            self.texte(c, &f.mono_centre, nom, rect(bz.x, bz.y, bz.l, bz.h));
                        }
                    }
                    Bouton::Annuler | Bouton::Fermer => {
                        self.couleur(c, if survole { &BLANC } else { &BLANC_DOUX });
                        self.trait_signe(c, (bx - 3.5, by - 3.5), (bx + 3.5, by + 3.5), 1.5);
                        self.trait_signe(c, (bx - 3.5, by + 3.5), (bx + 3.5, by - 3.5), 1.5);
                    }
                    Bouton::Pause | Bouton::AnnulerAction | Bouton::Afficher => {
                        self.couleur(c, &BLANC);
                        self.texte(c, &f.mono_centre, texte, rect(bz.x, bz.y, bz.l, bz.h));
                    }
                }
            }
            self.flip_retirer(c, garde_more);
            if rogne {
                rt.PopAxisAlignedClip();
            }
        }
    }

    /// Rectangle de la carte de la palette pour une fenêtre `l` × `h`. Les commandes ont la carte de la maquette (champ 64, jauge, puces
    /// 44, corps 330 au plus, pied 46) ; les modes qui demandent un nom gardent la carte simple d'avant, haute de ses lignes.
    fn rect_palette(p: &Palette, l: f32, h: f32) -> D2D_RECT_F {
        let w = (l - 120.0).clamp(320.0, 780.0);
        if p.est_assistant() {
            let corps = Self::corps_palette(h);
            return rect((l - w) / 2.0, HAUTEUR_TITRE + 40.0, w, PALETTE_ENTETE + corps + PALETTE_PIED);
        }
        const RANG: f32 = 36.0;
        let visibles = ((h - HAUTEUR_TITRE - 220.0) / RANG).floor().clamp(3.0, 12.0) as usize;
        rect((l - w) / 2.0, HAUTEUR_TITRE + 40.0, w, 70.0 + p.items.len().min(visibles).max(1) as f32 * RANG + 40.0)
    }

    /// Hauteur du corps (liste et aperçu) : 330 DIP (`.pbd`), moins si la fenêtre est basse
    fn corps_palette(h: f32) -> f32 {
        (h - HAUTEUR_TITRE - 40.0 - PALETTE_ENTETE - PALETTE_PIED - 24.0).clamp(120.0, 330.0)
    }

    /// Dessine dans une zone révélée par le haut sur la fraction `fraction` de sa hauteur (`clip-path: inset(0 0 x% 0 round …)`) ;
    /// entière (≥ 1), le dessin est direct, sans calque.
    unsafe fn avec_revelation(&self, c: &Cible2D, zone: D2D_RECT_F, fraction: f32, dessiner: impl FnOnce()) {
        let fraction = fraction.clamp(0.0, 1.0);
        if fraction >= 0.999 {
            dessiner();
            return;
        }
        let rt = &c.rt;
        let haut = (zone.bottom - zone.top) * fraction;
        let (Ok(calque), Ok(masque)) = (rt.CreateLayer(None), self.d2d.CreateRoundedRectangleGeometry(&D2D1_ROUNDED_RECT { rect: rect(zone.left, zone.top, zone.right - zone.left, haut.max(0.0)), radiusX: 14.0, radiusY: 14.0 })) else {
            dessiner();
            return;
        };
        let Ok(masque) = masque.cast::<ID2D1Geometry>() else {
            dessiner();
            return;
        };
        let params = D2D1_LAYER_PARAMETERS {
            contentBounds: D2D_RECT_F { left: f32::MIN, top: f32::MIN, right: f32::MAX, bottom: f32::MAX },
            geometricMask: std::mem::ManuallyDrop::new(Some(masque)),
            maskAntialiasMode: D2D1_ANTIALIAS_MODE_PER_PRIMITIVE,
            maskTransform: Matrix3x2::identity(),
            opacity: 1.0,
            ..Default::default()
        };
        rt.PushLayer(&params, &calque);
        // Le masque a été copié par D2D (compté) : on relâche notre référence
        drop(std::mem::ManuallyDrop::into_inner(params.geometricMask));
        dessiner();
        rt.PopLayer();
    }

    /// Palette de commandes (Ctrl+K ou Ctrl+Maj+P) : le voile est posé par l'appelant, hors de la révélation
    unsafe fn dessiner_palette(&self, c: &Cible2D, p: &Palette, l: f32, h: f32) {
        let z = Self::rect_palette(p, l, h);
        *self.zones_palette.borrow_mut() = ZonesPalette { carte: Some(z), ..Default::default() };
        if p.est_assistant() {
            self.dessiner_palette_commandes(c, p, z);
        } else {
            self.dessiner_palette_simple(c, p, z, h);
        }
    }

    /// Palette à étapes, comme la maquette : champ et yeux, jauge `.pm2`, fil d'Ariane (puces `.pcat`), liste à pastille `.ps` et
    /// aperçu `.pv`, pied `.ft`. Tout ce qui bouge vient de `mouvement::AnimPalette`, réglé ici à chaque image.
    unsafe fn dessiner_palette_commandes(&self, c: &Cible2D, p: &Palette, z: D2D_RECT_F) {
        let rt = &c.rt;
        let f = &self.f;
        let (x, y, w, hc) = (z.left, z.top, z.right - z.left, z.bottom - z.top);
        let t = self.t_image.get();
        let animer = crate::reglages::animations_actives();
        self.carte(c, z);

        // ── Champ ── Pixel (dessiné par la fenêtre, une seule instance) se pose à gauche : `pixel_dans_palette`
        let champ = rect(x + 46.0, y + 12.0, w - 66.0 - 64.0, 40.0);
        if p.saisie.texte.is_empty() {
            self.couleur(c, &BLANC_TRES_FAIBLE);
            self.texte(c, &f.petit, &p.invite(), rect(champ.left + 10.0, champ.top, champ.right - champ.left - 20.0, 40.0));
            self.couleur(c, &ACCENT);
            rt.FillRectangle(&rect(champ.left + 6.0, champ.top + 10.0, 2.0, 20.0), &c.pinceau);
        } else {
            self.champ(c, &p.saisie, rect(champ.left + 6.0, champ.top, champ.right - champ.left - 6.0, 40.0));
        }
        self.touches_droite(c, "ECHAP", x + w - 22.0, y + 32.0, champ.right, true);

        // ── Mise en page de la liste, et réglage des glissements pour cette image ──
        let lignes = p.lignes();
        let geo = palette::geometrie(&lignes);
        let total = palette::hauteur_contenu(&lignes);
        let corps = z.bottom - z.top - PALETTE_ENTETE - PALETTE_PIED;
        let avec_apercu = w >= 560.0;
        let largeur_liste = if avec_apercu { w - PALETTE_APERCU_L } else { w };
        let (lx, ly) = (x, y + PALETTE_ENTETE);
        let cle = p.cle_liste();
        if p.cle_defilement.get() != cle {
            p.cle_defilement.set(cle);
            p.defilement.set(0.0);
        }
        let rang_choisi = lignes.iter().position(|l| *l == palette::LignePal::Ligne(p.choix));
        let pastille = rang_choisi.and_then(|r| geo.get(r).copied());
        if let Some((haut, hauteur)) = pastille {
            p.defilement.set(palette::defilement_visible(p.defilement.get(), haut, hauteur, corps, total));
        }
        let defil = p.defilement.get();
        // Éléments dont une part est dans la fenêtre de la liste : seules leurs puces sont suivies
        let visibles: Vec<usize> = lignes
            .iter()
            .zip(&geo)
            .filter_map(|(l, &(haut, hauteur))| match l {
                palette::LignePal::Ligne(k) if haut + hauteur > defil && haut < defil + corps => Some(*k),
                _ => None,
            })
            .collect();
        let affiches = visibles.first().copied().unwrap_or(0)..visibles.last().map_or(0, |k| k + 1);
        let requete = p.requete();
        {
            let mut m = self.mouvements.borrow_mut();
            m.palette.maj(
                &mouvement::EntreePalette {
                    lettres: requete.chars().count(),
                    categorie: p.etape.rang(),
                    cle_liste: cle,
                    pastille,
                    choix: p.choix,
                    affiches,
                    cle_commande: p.cle_commande(),
                },
                t,
                animer,
            );
        }
        let anim = self.mouvements.borrow();
        let anim = &anim.palette;

        // ── Jauge `.pm2` : 40 traits, ceux sous `q.len() * 2` s'allument ──
        let ja_x = x + 22.0;
        let ja_l = w - 44.0;
        let trait_l = ((ja_l - 3.0 * (mouvement::JAUGE_TRAITS - 1) as f32) / mouvement::JAUGE_TRAITS as f32).max(1.0);
        for i in 0..mouvement::JAUGE_TRAITS {
            let (k, echelle) = anim.jauge(i, t);
            let col = melange_couleurs(&LIGNE, &ACCENT, k);
            self.couleur(c, &col);
            let hh = 2.0 * echelle;
            let r = rect(ja_x + i as f32 * (trait_l + 3.0), y + PALETTE_CHAMP_H - hh / 2.0, trait_l, hh);
            rt.FillRoundedRectangle(&D2D1_ROUNDED_RECT { rect: r, radiusX: 1.0, radiusY: 1.0 }, &c.pinceau);
        }

        // ── Fil d'Ariane : une puce par étape posée, l'étape courante en accent qui entre en 200 ms (`.pcat`) ──
        let mut cx = x + 18.0;
        let cy = y + PALETTE_CHAMP_H + 4.0 + 12.0;
        for (n, (libelle, rang, courante)) in p.fil().iter().enumerate() {
            if n > 0 {
                self.couleur(c, &BLANC_FAIBLE);
                self.texte(c, &f.petit, "›", rect(cx, cy, 12.0, 28.0));
                cx += 16.0;
            }
            let wl = self.mesurer(libelle, &f.petit).min(200.0);
            let wp = 12.0 + wl + 12.0 + 2.0;
            if cx + wp > x + w - 18.0 {
                break;
            }
            let r = rect(cx, cy, wp, 28.0);
            let on = if *courante { anim.categorie(*rang, t) } else { 0.0 };
            let arrondi = D2D1_ROUNDED_RECT { rect: D2D_RECT_F { left: r.left + 0.5, top: r.top + 0.5, right: r.right - 0.5, bottom: r.bottom - 0.5 }, radiusX: 14.0, radiusY: 14.0 };
            if *courante && on > 0.001 {
                self.couleur_k(c, &ACCENT, 0.10 * on);
                rt.FillRoundedRectangle(&arrondi, &c.pinceau);
            }
            // Une étape posée a le bord de l'accent à demi et le texte blanc ; la courante passe de gris à accent
            let bord = if *courante { melange_couleurs(&LIGNE, &D2D1_COLOR_F { a: 0.6, ..ACCENT }, on) } else { D2D1_COLOR_F { a: 0.35, ..ACCENT } };
            self.couleur(c, &bord);
            rt.DrawRoundedRectangle(&arrondi, &c.pinceau, 1.0, None);
            self.couleur(c, &if *courante { melange_couleurs(&BLANC_DOUX, &ACCENT, on) } else { BLANC });
            self.texte(c, &f.petit, libelle, rect(cx + 13.0, cy, wl + 2.0, 28.0));
            // Les puces posées se cliquent : retour à cette étape ; sur « Élément », la recherche d'un autre élément rouvre
            if !*courante && p.etape != crate::assistant::Etape::Progression {
                self.zones_palette.borrow_mut().puces.push((*rang, r));
            }
            cx += wp + 6.0;
        }

        // ── Liste ──
        let zone_liste = rect(lx, ly, largeur_liste, corps);
        rt.PushAxisAlignedClip(&zone_liste, D2D1_ANTIALIAS_MODE_ALIASED);
        let (p_haut, p_hauteur) = anim.pastille(t);
        if rang_choisi.is_some() {
            let r = rect(lx + 10.0, ly + p_haut - defil, largeur_liste - 20.0, p_hauteur);
            self.couleur(c, &LIGNE);
            rt.FillRoundedRectangle(&D2D1_ROUNDED_RECT { rect: r, radiusX: 14.0, radiusY: 14.0 }, &c.pinceau);
            self.couleur(c, &ACCENT);
            rt.FillRoundedRectangle(&D2D1_ROUNDED_RECT { rect: rect(r.left, r.top + 13.0, 3.0, (p_hauteur - 26.0).max(0.0)), radiusX: 1.5, radiusY: 1.5 }, &c.pinceau);
        }
        if p.items.is_empty() {
            self.couleur(c, &BLANC_FAIBLE);
            self.texte_espace(c, &f.petit, "AUCUN RÉSULTAT", rect(lx + 26.0, ly + palette::LISTE_HAUT, largeur_liste - 52.0, palette::TITRE_H));
        }
        for (ligne, &(haut, hauteur)) in lignes.iter().zip(&geo) {
            let yr = ly + haut - defil;
            if yr + hauteur <= ly || yr >= ly + corps {
                continue;
            }
            match *ligne {
                palette::LignePal::Titre(titre) => {
                    // `.gh` : 12 DIP en capitales espacées, couleur i3
                    self.couleur(c, &BLANC_FAIBLE);
                    self.texte_espace(c, &f.petit, titre, rect(lx + 26.0, yr + 3.0, largeur_liste - 52.0, hauteur));
                }
                palette::LignePal::Ligne(k) => {
                    let it = &p.items[k];
                    let ligne_x = lx + 10.0;
                    let ligne_l = largeur_liste - 20.0;
                    let cur = anim.puce(k, t);
                    // Puce d'icône `.pt` : 30 × 30, rayon 10, bord et couleur passent à l'accent sur la ligne courante
                    let puce = rect(ligne_x + 16.0, yr + (hauteur - 30.0) / 2.0, 30.0, 30.0);
                    self.couleur(c, &melange_couleurs(&LIGNE, &D2D1_COLOR_F { a: 0.6, ..ACCENT }, cur));
                    rt.DrawRoundedRectangle(&D2D1_ROUNDED_RECT { rect: D2D_RECT_F { left: puce.left + 0.5, top: puce.top + 0.5, right: puce.right - 0.5, bottom: puce.bottom - 0.5 }, radiusX: 10.0, radiusY: 10.0 }, &c.pinceau, 1.0, None);
                    self.icone(c, it.icone, puce.left + 15.0, puce.top + 15.0, false, if it.grise { 0.35 } else { 0.7 + 0.3 * cur });
                    // Libellé (surligné par la requête, `hlq`), détail, raccourci
                    let wr = largeur_touches(it.raccourci) + 20.0;
                    let x_texte = puce.right + 14.0;
                    let wl = (self.mesurer(&it.libelle, &f.liste) + 16.0).min((ligne_x + ligne_l - 14.0 - x_texte - wr) * 0.8).max(0.0);
                    self.couleur(c, if it.grise { &BLANC_FAIBLE } else { &BLANC });
                    let zone_libelle = rect(x_texte, yr, wl, hauteur);
                    self.texte(c, &f.liste, &it.libelle, zone_libelle);
                    let nom: Vec<u16> = it.libelle.encode_utf16().collect();
                    self.souligner_plages(c, &f.liste, &nom, &palette::plages_palette(&it.libelle, &requete), zone_libelle);
                    if !it.detail.is_empty() {
                        self.couleur(c, &BLANC_FAIBLE);
                        self.texte(c, &f.petit_mono, &it.detail, rect(x_texte + wl + 4.0, yr, (ligne_x + ligne_l - 14.0 - wr - x_texte - wl - 4.0).max(0.0), hauteur));
                    }
                    self.touches_droite(c, it.raccourci, ligne_x + ligne_l - 14.0, yr + hauteur / 2.0, x_texte + wl, true);
                    let visible = rect(ligne_x, yr.max(ly), ligne_l, (yr + hauteur).min(ly + corps) - yr.max(ly));
                    {
                        let mut z = self.zones_palette.borrow_mut();
                        z.lignes.push((k, visible));
                        z.cles.push((k, it.cle()));
                    }
                }
            }
        }
        rt.PopAxisAlignedClip();

        // ── Aperçu `.pv` ──
        if avec_apercu {
            let ax = x + w - PALETTE_APERCU_L;
            self.couleur(c, &PANNEAU);
            rt.FillRectangle(&rect(ax, ly, PALETTE_APERCU_L, corps), &c.pinceau);
            self.couleur(c, &LIGNE);
            rt.FillRectangle(&rect(ax, ly, 1.0, corps), &c.pinceau);
            if let Some(a) = p.apercu() {
                self.dessiner_apercu_palette(c, &a, rect(ax + 1.0, ly, PALETTE_APERCU_L - 1.0, corps), anim.vin(t));
            }
        }

        // ── Pied `.ft` ──
        let py = y + hc - PALETTE_PIED;
        self.couleur(c, &LIGNE);
        rt.FillRectangle(&rect(x, py, w, 1.0), &c.pinceau);
        let (compte, gestes) = p.pied();
        self.couleur(c, &BLANC_DOUX);
        let larg_gestes: f32 = gestes.iter().map(|(k, lib)| largeur_touche(k) + 6.0 + self.mesurer(lib, &f.petit)).sum::<f32>() + 16.0 * (gestes.len() - 1) as f32;
        self.texte_espace(c, &f.volet_nom, &compte, rect(x + 22.0, py, (w - 44.0 - larg_gestes - 16.0).max(0.0), PALETTE_PIED));
        let mut gx = x + w - 22.0 - larg_gestes;
        for (k, lib) in gestes {
            gx = self.touche_unique(c, k, gx, py + PALETTE_PIED / 2.0, true) + 6.0;
            let lw = self.mesurer(lib, &f.petit);
            self.couleur(c, &BLANC_FAIBLE);
            self.texte(c, &f.petit, lib, rect(gx, py, lw + 2.0, PALETTE_PIED));
            gx += lw + 16.0;
        }
    }

    /// Colonne de droite de la palette : titre en Doto, description, puis le dessin de la commande (apparition `vin` : 350 ms au ressort)
    unsafe fn dessiner_apercu_palette(&self, c: &Cible2D, a: &palette::ApercuPalette, zone: D2D_RECT_F, vin: f32) {
        let f = &self.f;
        let rt = &c.rt;
        let (x, y) = (zone.left + 20.0, zone.top + 18.0);
        let l = zone.right - zone.left - 40.0;
        self.couleur(c, &BLANC);
        // Deux lignes de titre au plus (Doto 20, interligne 1,15) : une troisième serait coupée par « … »
        self.texte_espace(c, &f.apercu_titre, &a.titre, rect(x, y, l, 56.0));
        self.couleur(c, &BLANC_DOUX);
        self.texte(c, &f.carte_desc, &a.description, rect(x, y + 56.0 + 10.0, l, 54.0));
        let haut_art = y + 56.0 + 10.0 + 54.0 + 14.0;
        let art = rect(x, haut_art, l, (zone.bottom - 18.0 - haut_art).max(0.0));
        if a.art == palette::Art::Aucun || art.bottom - art.top < 40.0 {
            return;
        }
        let cadre = D2D1_ROUNDED_RECT { rect: art, radiusX: 16.0, radiusY: 16.0 };
        self.couleur(c, &VOILE);
        rt.FillRoundedRectangle(&cadre, &c.pinceau);
        self.trame_arrondie(c, &cadre);
        self.couleur(c, &LIGNE);
        rt.DrawRoundedRectangle(&D2D1_ROUNDED_RECT { rect: D2D_RECT_F { left: art.left + 0.5, top: art.top + 0.5, right: art.right - 0.5, bottom: art.bottom - 0.5 }, radiusX: 16.0, radiusY: 16.0 }, &c.pinceau, 1.0, None);
        let dedans = D2D_RECT_F { left: art.left + 14.0, top: art.top + 14.0, right: art.right - 14.0, bottom: art.bottom - 14.0 };
        // `vin` : le dessin monte de 8 DIP en se fondant, au ressort ; coupé en mouvement réduit (la valeur est alors 1)
        let d = mouvement::Decalage { dx: 0.0, dy: mouvement::VIN_DECALAGE * (1.0 - vin), echelle: 1.0, opacite: vin.clamp(0.0, 1.0) };
        rt.PushAxisAlignedClip(&art, D2D1_ANTIALIAS_MODE_ALIASED);
        self.avec_decalage(c, &d, ((art.left + art.right) / 2.0, (art.top + art.bottom) / 2.0), art, || self.dessiner_art(c, &a.art, dedans));
        rt.PopAxisAlignedClip();
    }

    /// Une barre `.b` de l'aperçu : 8 DIP de haut, rayon 4, accent ou gris
    unsafe fn barre_art(&self, c: &Cible2D, r: D2D_RECT_F, accent: bool) {
        self.couleur(c, if accent { &ACCENT } else { &BLANC_TRES_FAIBLE });
        let rayon = ((r.bottom - r.top) / 2.0).min(4.0);
        c.rt.FillRoundedRectangle(&D2D1_ROUNDED_RECT { rect: r, radiusX: rayon, radiusY: rayon }, &c.pinceau);
    }

    /// Ligne `.frow` : icône, texte, valeur à droite (couleur i3)
    unsafe fn ligne_art(&self, c: &Cible2D, icone: Icone, texte: &str, droite: &str, x: f32, y: f32, l: f32, couleur: &D2D1_COLOR_F) {
        self.icone(c, icone, x + 8.0, y + 11.0, false, 0.8);
        let wd = if droite.is_empty() { 0.0 } else { self.mesurer(droite, &self.f.petit) + 8.0 };
        self.couleur(c, couleur);
        self.texte(c, &self.f.petit, texte, rect(x + 24.0, y, (l - 24.0 - wd).max(0.0), 22.0));
        if wd > 0.0 {
            self.couleur(c, &BLANC_FAIBLE);
            self.texte(c, &self.f.petit_droite, droite, rect(x + l - wd, y, wd, 22.0));
        }
    }

    /// Points de progression `.dp` : `pourcent` % allumés en accent
    unsafe fn points_art(&self, c: &Cible2D, x: f32, y: f32, l: f32, pourcent: f32) {
        const N: usize = 24;
        let pas = l / N as f32;
        for i in 0..N {
            self.couleur(c, if (i as f32 + 0.5) / N as f32 * 100.0 <= pourcent { &ACCENT } else { &BLANC_TRES_FAIBLE });
            c.rt.FillEllipse(&ellipse(x + pas * (i as f32 + 0.5), y, 1.5), &c.pinceau);
        }
    }

    /// Colonne `.colm` : fond s1, rayon 8, quatre barres de 6 DIP (celle de `accent` en accent) ; `fondu` : opacité de la colonne
    unsafe fn colonne_art(&self, c: &Cible2D, r: D2D_RECT_F, barres: usize, accent: Option<usize>, fondu: f32) {
        self.couleur_k(c, &LIGNE, fondu);
        c.rt.FillRoundedRectangle(&D2D1_ROUNDED_RECT { rect: r, radiusX: 8.0, radiusY: 8.0 }, &c.pinceau);
        for j in 0..barres {
            let b = rect(r.left + 6.0, r.top + 8.0 + j as f32 * 11.0, (r.right - r.left - 12.0).max(0.0), 6.0);
            if b.right > b.left && b.bottom <= r.bottom {
                self.barre_art(c, b, accent == Some(j));
            }
        }
    }

    /// Le dessin de l'aperçu (`.art`) selon la commande, dans `d`
    unsafe fn dessiner_art(&self, c: &Cible2D, art: &palette::Art, d: D2D_RECT_F) {
        use palette::Art;
        let (l, h) = (d.right - d.left, d.bottom - d.top);
        let centre_v = |contenu: f32| d.top + ((h - contenu) / 2.0).max(0.0);
        match art {
            Art::Aucun => {}
            Art::Liste => {
                let y0 = centre_v(5.0 * 8.0 + 4.0 * 7.0);
                for (i, pct) in [70.0f32, 52.0, 84.0, 60.0, 76.0].into_iter().enumerate() {
                    self.barre_art(c, rect(d.left, y0 + i as f32 * 15.0, l * pct / 100.0, 8.0), i == 1);
                }
            }
            Art::Grille => {
                let par = (((l + 8.0) / 52.0).floor() as usize).max(1);
                let rangs = 6usize.div_ceil(par);
                let y0 = centre_v(rangs as f32 * 36.0 + (rangs - 1) as f32 * 8.0);
                for i in 0..6usize {
                    let (col, rang) = (i % par, i / par);
                    // Dernière ligne centrée, comme `justify-content: center`
                    let dans = if rang == rangs - 1 { 6 - rang * par } else { par };
                    let x0 = d.left + (l - (dans as f32 * 44.0 + (dans - 1) as f32 * 8.0)) / 2.0;
                    self.couleur(c, &BLANC_TRES_FAIBLE);
                    c.rt.FillRoundedRectangle(&D2D1_ROUNDED_RECT { rect: rect(x0 + col as f32 * 52.0, y0 + rang as f32 * 44.0, 44.0, 36.0), radiusX: 8.0, radiusY: 8.0 }, &c.pinceau);
                }
            }
            Art::Colonnes => {
                let lc = (l - 16.0) / 3.0;
                for k in 0..3usize {
                    self.colonne_art(c, rect(d.left + k as f32 * (lc + 8.0), d.top, lc, h), 4, (k == 1).then_some(1), 1.0);
                }
            }
            Art::Essentiel => {
                let lm = l - 2.0 * 22.0 - 16.0;
                self.colonne_art(c, rect(d.left, d.top, 22.0, h), 0, None, 0.35);
                self.colonne_art(c, rect(d.left + 30.0, d.top, lm, h), 3, None, 1.0);
                self.colonne_art(c, rect(d.right - 22.0, d.top, 22.0, h), 0, None, 0.35);
            }
            Art::NouveauDossier => {
                let y0 = centre_v(34.0 + 7.0 + 22.0);
                self.couleur(c, &ACCENT);
                self.texte(c, &self.f.apercu_grand, "+", rect(d.left, y0, l, 34.0));
                let texte = "Nouveau dossier";
                let tw = self.mesurer(texte, &self.f.petit) + 24.0;
                let x0 = d.left + (l - tw) / 2.0;
                self.ligne_art(c, Icone::Dossier, texte, "", x0, y0 + 41.0, tw, &BLANC_DOUX);
            }
            Art::Copier => {
                let y0 = centre_v(22.0 + 4.0 + 8.0);
                self.ligne_art(c, Icone::Fichier, "sélection", "→ Documents", d.left, y0, l, &BLANC_DOUX);
                self.points_art(c, d.left, y0 + 26.0 + 4.0, l, 62.0);
            }
            Art::Supprimer => {
                let y0 = centre_v(22.0 + 4.0 + 8.0);
                self.ligne_art(c, Icone::Fichier, "1 élément supprimé", "Annuler", d.left, y0, l, &BLANC_DOUX);
                self.points_art(c, d.left, y0 + 26.0 + 4.0, l, 78.0);
            }
            Art::Recherche => {
                let y0 = centre_v(22.0 + 7.0 + 22.0);
                self.ligne_art(c, Icone::Disque, "contrat ext:pdf", "", d.left, y0, l, &BLANC_DOUX);
                let mut x0 = d.left + 2.0;
                for puce in ["ext:", "taille>", "modif:"] {
                    let w = self.mesurer(puce, &self.f.petit) + 18.0;
                    if x0 + w > d.right {
                        break;
                    }
                    let r = rect(x0, y0 + 29.0, w, 22.0);
                    self.couleur(c, &BLANC_TRES_FAIBLE);
                    c.rt.DrawRoundedRectangle(&D2D1_ROUNDED_RECT { rect: D2D_RECT_F { left: r.left + 0.5, top: r.top + 0.5, right: r.right - 0.5, bottom: r.bottom - 0.5 }, radiusX: 11.0, radiusY: 11.0 }, &c.pinceau, 1.0, None);
                    self.couleur(c, &BLANC_DOUX);
                    self.texte(c, &self.f.petit_centre, puce, r);
                    x0 += w + 6.0;
                }
            }
            Art::Reglages => {
                let y0 = centre_v(3.0 * 22.0 + 2.0 * 7.0);
                for (i, nom) in ["Apparence", "Stockages", "Raccourcis"].into_iter().enumerate() {
                    let yy = y0 + i as f32 * 29.0;
                    if i == 0 {
                        self.couleur(c, &ACCENT);
                        c.rt.FillRoundedRectangle(&D2D1_ROUNDED_RECT { rect: rect(d.left, yy + 5.0, 3.0, 12.0), radiusX: 1.5, radiusY: 1.5 }, &c.pinceau);
                    }
                    self.couleur(c, if i == 0 { &BLANC } else { &BLANC_DOUX });
                    self.texte(c, &self.f.petit, nom, rect(d.left + 11.0, yy, l - 11.0, 22.0));
                }
            }
            Art::Contenu(contenu) => self.dessiner_apercu_contenu(c, contenu, d),
            Art::Nom { avant, apres, raison } => {
                let y0 = centre_v(14.0 + 22.0 + 14.0 + 22.0 + 6.0 + 36.0);
                self.couleur(c, &BLANC_FAIBLE);
                self.texte(c, &self.f.petit, "AVANT", rect(d.left, y0, l, 14.0));
                self.ligne_art(c, Icone::Fichier, avant, "", d.left, y0 + 14.0, l, &BLANC_DOUX);
                self.couleur(c, &BLANC_FAIBLE);
                self.texte(c, &self.f.petit, "APRÈS", rect(d.left, y0 + 14.0 + 22.0 + 6.0, l, 14.0));
                let apres = if apres.is_empty() { "…" } else { apres.as_str() };
                self.ligne_art(c, Icone::Fichier, apres, "", d.left, y0 + 14.0 + 22.0 + 6.0 + 14.0, l, if raison.is_some() { &ERREUR } else { &ACCENT });
                if let Some(r) = raison {
                    self.couleur(c, &ERREUR);
                    self.texte(c, &self.f.carte_desc, r, rect(d.left, y0 + 14.0 + 22.0 + 6.0 + 14.0 + 26.0, l, 36.0));
                }
            }
            Art::Phrase { texte, avertissement } => self.encadre_phrase(c, texte, *avertissement, rect(d.left, d.top, l, h.min(96.0))),
            Art::Progression(pct) => {
                let y0 = centre_v(22.0 + 10.0 + 8.0);
                self.couleur(c, &BLANC_DOUX);
                self.texte(c, &self.f.petit, &format!("{} %", (pct.clamp(0.0, 1.0) * 100.0).round() as u32), rect(d.left, y0, l, 22.0));
                self.points_art(c, d.left, y0 + 22.0 + 10.0, l, pct.clamp(0.0, 1.0) * 100.0);
            }
        }
    }

    /// Phrase d'effet dans un cadre : accent à demi pour ce qui va arriver, rouge pour un avertissement
    unsafe fn encadre_phrase(&self, c: &Cible2D, texte: &str, avertissement: bool, zone: D2D_RECT_F) {
        let cadre = D2D1_ROUNDED_RECT { rect: D2D_RECT_F { left: zone.left + 0.5, top: zone.top + 0.5, right: zone.right - 0.5, bottom: zone.bottom - 0.5 }, radiusX: 10.0, radiusY: 10.0 };
        self.couleur(c, if avertissement { &ERREUR } else { &ACCENT_BORD });
        c.rt.DrawRoundedRectangle(&cadre, &c.pinceau, 1.0, None);
        self.couleur(c, if avertissement { &ERREUR } else { &BLANC_DOUX });
        self.texte(c, &self.f.carte_desc, texte, rect(zone.left + 10.0, zone.top + 8.0, (zone.right - zone.left - 20.0).max(0.0), (zone.bottom - zone.top - 12.0).max(0.0)));
    }

    /// Le contenu d'un dossier : ses premiers éléments, puis ce qui va arriver (« + » ou « ! » s'il existe déjà), puis la phrase d'effet
    unsafe fn dessiner_apercu_contenu(&self, c: &Cible2D, contenu: &palette::Contenu, d: D2D_RECT_F) {
        let (l, h) = (d.right - d.left, d.bottom - d.top);
        let phrase_h = if contenu.effet.is_empty() { 0.0 } else { 58.0f32.min(h * 0.45) };
        let lignes = (((h - phrase_h - 6.0) / 26.0).floor() as usize).max(1);
        let nouveaux = contenu.nouveaux.len().min(2).min(lignes.saturating_sub(1));
        let elements = lignes - nouveaux;
        let mut y = d.top;
        if contenu.en_cours {
            self.couleur(c, &BLANC_FAIBLE);
            self.texte(c, &self.f.petit, "Lecture du dossier…", rect(d.left, y, l, 22.0));
            y += 26.0;
        } else if !contenu.lu {
            self.couleur(c, &BLANC_FAIBLE);
            self.texte(c, &self.f.petit, "Contenu non lu (réseau ou amovible)", rect(d.left, y, l, 22.0));
            y += 26.0;
        } else if contenu.elements.is_empty() {
            self.couleur(c, &BLANC_FAIBLE);
            self.texte(c, &self.f.petit, "Dossier vide", rect(d.left, y, l, 22.0));
            y += 26.0;
        }
        for (nom, taille, dossier) in contenu.elements.iter().take(elements) {
            self.ligne_art(c, if *dossier { Icone::Dossier } else { Icone::Fichier }, nom, taille, d.left, y, l, &BLANC_DOUX);
            y += 26.0;
        }
        for (nom, existe) in contenu.nouveaux.iter().take(nouveaux) {
            self.ligne_art(c, Icone::Fichier, nom, if *existe { "existe déjà" } else { "nouveau" }, d.left, y, l, if *existe { &ERREUR } else { &ACCENT });
            y += 26.0;
        }
        if phrase_h > 0.0 {
            self.encadre_phrase(c, &contenu.effet, contenu.conflit, rect(d.left, d.bottom - phrase_h, l, phrase_h));
        }
    }

    /// Palette des modes qui demandent un nom (groupe, session, note, glyphe, règle, renommage) : la carte simple d'avant la maquette
    unsafe fn dessiner_palette_simple(&self, c: &Cible2D, p: &Palette, z: D2D_RECT_F, h: f32) {
        let rt = &c.rt;
        let f = &self.f;
        const RANG: f32 = 36.0;
        let visibles = ((h - HAUTEUR_TITRE - 220.0) / RANG).floor().clamp(3.0, 12.0) as usize;
        let (x, y, w, hc) = (z.left, z.top, z.right - z.left, z.bottom - z.top);
        self.carte(c, rect(x, y, w, hc));
        let n = p.items.len().min(visibles);
        // Pixel (dessiné par la fenêtre, une seule instance) se pose à gauche du champ : `pixel_dans_palette`
        let champ = rect(x + 46.0, y + 12.0, w - 66.0, 40.0);
        if p.saisie.texte.is_empty() {
            self.couleur(c, &BLANC_TRES_FAIBLE);
            self.texte(c, &f.petit, &p.invite(), rect(champ.left + 10.0, champ.top, champ.right - champ.left - 20.0, 40.0));
            self.couleur(c, &ACCENT);
            rt.FillRectangle(&rect(champ.left + 6.0, champ.top + 10.0, 2.0, 20.0), &c.pinceau);
        } else {
            self.champ(c, &p.saisie, rect(champ.left + 6.0, champ.top, champ.right - champ.left - 6.0, 40.0));
        }
        self.couleur(c, &LIGNE);
        rt.FillRectangle(&rect(x + 16.0, y + 62.0, w - 32.0, 1.0), &c.pinceau);
        let debut = (p.choix + 1).saturating_sub(n);
        if p.items.is_empty() {
            self.couleur(c, &BLANC_DOUX);
            self.texte(c, &f.petit, "AUCUN RESULTAT", rect(x + 24.0, y + 70.0, w - 48.0, RANG));
        }
        for (k, it) in p.items.iter().enumerate().skip(debut).take(n) {
            let yr = y + 68.0 + (k - debut) as f32 * RANG;
            let fond = rect(x + 10.0, yr, w - 20.0, RANG - 2.0);
            let choisi = k == p.choix;
            if choisi {
                self.couleur(c, &SURVOL_FORT);
                rt.FillRoundedRectangle(&D2D1_ROUNDED_RECT { rect: fond, radiusX: 8.0, radiusY: 8.0 }, &c.pinceau);
                self.couleur(c, &ACCENT);
                rt.FillRectangle(&rect(fond.left, fond.top + 9.0, 2.0, RANG - 20.0), &c.pinceau);
            }
            self.icone(c, it.icone, x + 32.0, yr + RANG / 2.0, false, if choisi { 1.0 } else { 0.7 });
            let wr = largeur_touches(it.raccourci) + 20.0;
            let wl = (self.mesurer(&it.libelle, &f.liste) + 16.0).min((w - 90.0 - wr) * 0.7);
            self.couleur(c, if choisi { &BLANC } else { &BLANC_DOUX });
            self.texte(c, &f.liste, &it.libelle, rect(x + 52.0, yr, wl, RANG));
            self.couleur(c, &BLANC_DOUX);
            self.texte(c, &f.petit_mono, &it.detail, rect(x + 56.0 + wl, yr, (w - 90.0 - wr - wl).max(0.0), RANG));
            // Touches (une par cadre) ; si la place manque entre le libellé et le bord, le libellé garde la priorité
            self.touches_droite(c, it.raccourci, x + w - 20.0, yr + RANG / 2.0, x + 56.0 + wl, true);
        }
        let aide = format!("{} RESULTATS · ↑↓ CHOISIR · ENTREE EXECUTER · ECHAP FERMER", p.items.len());
        self.aide(c, &aide, rect(x + 24.0, y + hc - 34.0, w - 48.0, 24.0), Pose::Gauche, &BLANC_TRES_FAIBLE);
    }

    /// Renommage par lot : motif, chercher / remplacer, aperçu avant / après
    unsafe fn dessiner_lot(&self, c: &Cible2D, lot: &Lot, l: f32, h: f32) {
        let rt = &c.rt;
        let f = &self.f;
        self.couleur(c, &rgba(0, 0, 0, 0.55));
        rt.FillRectangle(&rect(largeur_volet() + 1.0, HAUTEUR_TITRE + 1.0, l - largeur_volet() - 1.0, h - HAUTEUR_TITRE - 1.0), &c.pinceau);
        let w = (l - largeur_volet() - 60.0).clamp(420.0, 920.0);
        let x = largeur_volet() + (l - largeur_volet() - w) / 2.0;
        let y = HAUTEUR_TITRE + 30.0;
        let hc = (h - y - 30.0).max(360.0);
        self.carte(c, rect(x, y, w, hc));
        self.couleur(c, &ACCENT);
        rt.FillEllipse(&ellipse(x + 28.0, y + 34.0, 4.0), &c.pinceau);
        self.couleur(c, &BLANC);
        self.texte(c, &f.titre, &format!("RENOMMER PAR LOT · {} ELEMENTS", lot.noms.len()), rect(x + 42.0, y + 18.0, w - 60.0, 32.0));
        let lw = 150.0;
        for k in 0..3 {
            let yc = y + 64.0 + k as f32 * 46.0;
            let focus = lot.focus == k;
            self.couleur(c, if focus { &BLANC } else { &BLANC_DOUX });
            self.texte(c, &f.petit, crate::lot::LIBELLES[k], rect(x + 24.0, yc, lw, 40.0));
            let champ = rect(x + 24.0 + lw, yc + 2.0, w - 48.0 - lw, 36.0);
            if focus {
                self.champ(c, &lot.champs[k], rect(champ.left + 10.0, champ.top, champ.right - champ.left - 10.0, 36.0));
            } else {
                self.couleur(c, &BLANC_TRES_FAIBLE);
                rt.DrawRoundedRectangle(&D2D1_ROUNDED_RECT { rect: champ, radiusX: 5.0, radiusY: 5.0 }, &c.pinceau, 1.0, None);
                self.couleur(c, &BLANC_DOUX);
                self.texte_w(c, &f.liste, &lot.champs[k].texte, rect(champ.left + 10.0, champ.top, champ.right - champ.left - 20.0, 36.0));
            }
        }
        let yi = y + 64.0 + 3.0 * 46.0;
        self.aide(c, &format!("REGEX (CTRL+R) : {}", if lot.regex { "OUI · $1 DANS REMPLACER" } else { "NON" }), rect(x + 24.0, yi, w - 48.0, 20.0), Pose::Gauche, if lot.regex { &ACCENT } else { &BLANC_DOUX });
        self.couleur(c, &BLANC_TRES_FAIBLE);
        self.texte(c, &f.petit, "JETONS DU MOTIF : {nom} {ext} {n} {nn} {nnn} (COMPTEUR) {date} (PRISE DE VUE EXIF, SINON MODIFICATION)", rect(x + 24.0, yi + 20.0, w - 48.0, 20.0));

        // Aperçu avant / après
        let ya = yi + 52.0;
        self.couleur(c, &LIGNE);
        rt.FillRectangle(&rect(x + 16.0, ya - 6.0, w - 32.0, 1.0), &c.pinceau);
        let apercu = lot.apercu();
        let rang = 26.0;
        let place = ((y + hc - 70.0 - ya) / rang).floor().max(1.0) as usize;
        let moitie = (w - 48.0) / 2.0;
        for (k, ancien) in lot.noms.iter().enumerate().skip(lot.defilement).take(place) {
            let yr = ya + (k - lot.defilement) as f32 * rang;
            self.couleur(c, &BLANC_DOUX);
            self.texte(c, &f.liste, ancien, rect(x + 24.0, yr, moitie - 24.0, rang));
            if let Ok(v) = &apercu {
                let change = v[k] != *ancien;
                self.couleur(c, if change { &ACCENT } else { &BLANC_TRES_FAIBLE });
                self.points(c, x + 24.0 + moitie - 12.0, yr + rang / 2.0, 2.6, 1.0, &[(-1, -1), (0, 0), (-1, 1)]);
                self.couleur(c, if change { &BLANC } else { &BLANC_DOUX });
                self.texte(c, &f.liste, &v[k], rect(x + 24.0 + moitie, yr, moitie, rang));
            }
        }
        if lot.noms.len() > place {
            self.couleur(c, &BLANC_TRES_FAIBLE);
            self.texte(c, &f.petit_droite, &format!("{}–{} / {}", lot.defilement + 1, (lot.defilement + place).min(lot.noms.len()), lot.noms.len()), rect(x + 24.0, y + hc - 64.0, w - 48.0, 20.0));
        }
        let yb = y + hc - 40.0;
        match &apercu {
            Err(e) => {
                self.couleur(c, &ACCENT);
                rt.FillEllipse(&ellipse(x + 27.0, yb + 12.0, 2.5), &c.pinceau);
                self.texte(c, &f.petit, e, rect(x + 36.0, yb, w - 60.0, 24.0));
            }
            Ok(v) => {
                let n = v.iter().zip(&lot.noms).filter(|(a, b)| a != b).count();
                self.aide(c, &format!("{n} A RENOMMER · ENTREE : VALIDER · TAB : CHAMP SUIVANT · PAGE ↑↓ : DEFILER · ECHAP : ANNULER"), rect(x + 24.0, yb, w - 48.0, 24.0), Pose::Gauche, &BLANC_DOUX);
            }
        }
    }

    /// Conflits de noms : le fichier qui arrive et celui qui est déjà là, côte à côte
    /// Rectangle (x, y, largeur, hauteur) de la carte des conflits pour une fenêtre `l` × `h`
    fn rect_conflit(l: f32, h: f32) -> [f32; 4] {
        let w = (l - 60.0).clamp(480.0, 860.0);
        let hc = 500.0f32.min(h - HAUTEUR_TITRE - 30.0).max(420.0);
        [(l - w) / 2.0, HAUTEUR_TITRE + ((h - HAUTEUR_TITRE - hc) / 2.0).max(10.0), w, hc]
    }

    /// Rectangle de la ligne choisie `i` (là d'où le coup d'œil se déploie), ramené dans la zone visible de la liste ; à défaut le
    /// haut de la liste. `cible` sert de repli si la liste n'a pas de hauteur.
    fn rect_ligne_choisie(app: &App, ll: f32, i: usize, cible: [f32; 4]) -> [f32; 4] {
        let x = largeur_volet() + MARGE_LISTE - 6.0;
        let largeur = (ll - x - MARGE_LISTE_DROITE + 6.0).max(40.0);
        if app.hauteur_liste <= 0.0 || i >= app.entrees.len() {
            return [x, LISTE_Y, largeur, hauteur_ligne().max(cible[3].min(30.0))];
        }
        let y = LISTE_Y + app.rang_de(i) as f32 * hauteur_ligne() - app.defilement;
        [x, y.clamp(LISTE_Y, LISTE_Y + (app.hauteur_liste - hauteur_ligne()).max(0.0)), largeur, hauteur_ligne()]
    }

    /// Carte des conflits. `cadre` : voile et fond de la carte ; `contenu` : titre, fiches, boutons (lot 12D : le contenu paraît après
    /// le dépliage du cadre).
    unsafe fn dessiner_conflits(&self, c: &Cible2D, cf: &Conflits, l: f32, h: f32, cadre: bool, contenu: bool) {
        let rt = &c.rt;
        let f = &self.f;
        let Some(x) = cf.liste.get(cf.rang) else { return };
        let [x0, y0, w, hc] = Self::rect_conflit(l, h);
        if cadre {
            self.couleur(c, &rgba(0, 0, 0, 0.6));
            rt.FillRectangle(&rect(0.0, HAUTEUR_TITRE + 1.0, l, h - HAUTEUR_TITRE - 1.0), &c.pinceau);
            self.carte(c, rect(x0, y0, w, hc));
        }
        if !contenu {
            return;
        }

        // Titre : quel conflit, quelle opération, vers où
        self.couleur(c, &ACCENT);
        rt.FillEllipse(&ellipse(x0 + 28.0, y0 + 34.0, 4.0), &c.pinceau);
        self.couleur(c, &BLANC);
        let n = cf.liste.len();
        let titre = if n > 1 { format!("{} NOMS DEJA PRIS · {} / {}", n, cf.rang + 1, n) } else { "NOM DEJA PRIS".to_string() };
        self.texte(c, &f.titre, &titre, rect(x0 + 42.0, y0 + 18.0, w - 60.0, 32.0));
        self.couleur(c, &BLANC_DOUX);
        let libres = if cf.libres.is_empty() { String::new() } else { format!(" · {} AUTRES SANS CONFLIT", systeme::nombre(cf.libres.len())) };
        self.texte(c, &f.petit, &format!("{} VERS {}{}", cf.genre.libelle(), cf.dest.to_uppercase(), libres), rect(x0 + 42.0, y0 + 50.0, w - 66.0, 20.0));

        // Les deux fiches
        let yf = y0 + 84.0;
        let lf = (w - 48.0 - 40.0) / 2.0;
        // 2 s d'écart toléré : les disques FAT arrondissent les dates
        let plus_recent = |a: &Entree, b: &Entree| a.modif > b.modif + 20_000_000;
        let arrive = if cf.genre == crate::operations::Genre::Deplacer { "ARRIVE (DEPLACEMENT)" } else { "ARRIVE (COPIE)" };
        for (k, (fiche, autre, titre)) in [(&x.source, &x.cible, arrive), (&x.cible, &x.source, "DEJA LA")].into_iter().enumerate() {
            let fx = x0 + 24.0 + k as f32 * (lf + 40.0);
            self.couleur(c, &BLANC_TRES_FAIBLE);
            rt.DrawRoundedRectangle(&D2D1_ROUNDED_RECT { rect: rect(fx, yf, lf, 236.0), radiusX: 10.0, radiusY: 10.0 }, &c.pinceau, 1.0, None);
            self.couleur(c, if k == 0 { &ACCENT } else { &BLANC_DOUX });
            self.texte(c, &f.petit, titre, rect(fx + 14.0, yf + 8.0, lf - 28.0, 20.0));
            // Miniature (fichiers), sinon grande icône en points
            let zone = rect(fx + 14.0, yf + 32.0, lf - 28.0, 120.0);
            let image = if fiche.entree.est_conteneur() { None } else { self.vignette(c, &fiche.chemin) };
            match image {
                Some(b) => {
                    let t = b.GetSize();
                    let e = ((zone.right - zone.left) / t.width).min((zone.bottom - zone.top) / t.height).min(1.0);
                    let (iw, ih) = (t.width * e, t.height * e);
                    let r = rect(zone.left + (zone.right - zone.left - iw) / 2.0, zone.top + (zone.bottom - zone.top - ih) / 2.0, iw, ih);
                    rt.DrawBitmap(&b, Some(&r), 1.0, D2D1_BITMAP_INTERPOLATION_MODE_LINEAR, None);
                }
                None => self.icone_grande(c, icones::pour(&fiche.entree), (zone.left + zone.right) / 2.0, (zone.top + zone.bottom) / 2.0, 12.0, false, 1.0),
            }
            self.couleur(c, &BLANC);
            self.texte(c, &f.liste, &fiche.entree.nom, rect(fx + 14.0, yf + 158.0, lf - 28.0, 24.0));
            let taille = if fiche.entree.est_conteneur() { "DOSSIER".to_string() } else { systeme::taille(fiche.entree.taille) };
            self.couleur(c, &BLANC_DOUX);
            self.texte(c, &f.petit, &format!("{taille} · {}", systeme::date(fiche.entree.modif)), rect(fx + 14.0, yf + 184.0, lf - 28.0, 20.0));
            // Ce qui les distingue, en rouge
            let mut marques = Vec::new();
            if plus_recent(&fiche.entree, &autre.entree) {
                marques.push("PLUS RECENT");
            }
            if !fiche.entree.est_conteneur() && fiche.entree.taille > autre.entree.taille {
                marques.push("PLUS GRAND");
            }
            if !marques.is_empty() {
                self.couleur(c, &ACCENT);
                rt.FillEllipse(&ellipse(fx + 18.0, yf + 218.0, 2.5), &c.pinceau);
                self.texte(c, &f.petit, &marques.join(" · "), rect(fx + 26.0, yf + 208.0, lf - 40.0, 20.0));
            }
        }
        // Flèche en points entre les deux fiches
        self.couleur(c, &ACCENT);
        self.points(c, x0 + 24.0 + lf + 20.0, yf + 92.0, 4.0, 1.4, &[(-2, 0), (-1, 0), (0, 0), (1, 0), (0, -1), (0, 1), (-1, -2), (-1, 2)]);
        let meme = x.source.entree.taille == x.cible.entree.taille && !plus_recent(&x.source.entree, &x.cible.entree) && !plus_recent(&x.cible.entree, &x.source.entree);
        if meme && !x.source.entree.est_conteneur() {
            self.couleur(c, &BLANC_DOUX);
            self.texte(c, &f.petit_centre, "MEME TAILLE ET MEME DATE : PROBABLEMENT IDENTIQUES", rect(x0 + 24.0, yf + 244.0, w - 48.0, 20.0));
        }

        // Boutons : touche en rouge, libellé, conséquence
        let yb = yf + 274.0;
        let lb = (w - 48.0 - 3.0 * 10.0) / 4.0;
        let mut zones = self.zones_conflit.borrow_mut();
        for (k, choix) in Choix::TOUS.into_iter().enumerate() {
            let r = rect(x0 + 24.0 + k as f32 * (lb + 10.0), yb, lb, 64.0);
            let arrondi = D2D1_ROUNDED_RECT { rect: r, radiusX: 8.0, radiusY: 8.0 };
            self.couleur(c, &SURVOL);
            rt.FillRoundedRectangle(&arrondi, &c.pinceau);
            let actif = x.choix == Some(choix);
            self.couleur(c, if actif { &ACCENT } else { &BLANC_TRES_FAIBLE });
            rt.DrawRoundedRectangle(&arrondi, &c.pinceau, if actif { 1.5 } else { 1.0 }, None);
            self.couleur(c, &ACCENT);
            self.texte(c, &f.liste, &choix.touche().to_string(), rect(r.left + 12.0, r.top + 8.0, 20.0, 24.0));
            self.couleur(c, &BLANC);
            self.texte(c, &f.petit, choix.libelle(), rect(r.left + 32.0, r.top + 11.0, lb - 40.0, 20.0));
            self.couleur(c, &BLANC_DOUX);
            self.texte(c, &f.petit, choix.detail(), rect(r.left + 12.0, r.top + 36.0, lb - 20.0, 20.0));
            zones.push((r, choix));
        }

        // Un point par conflit : répondu (blanc), affiché (rouge), à faire (estompé)
        let yp = yb + 84.0;
        let visibles = n.min(60);
        for k in 0..visibles {
            let couleur = if k == cf.rang { &ACCENT } else if cf.liste[k].choix.is_some() { &BLANC } else { &BLANC_TRES_FAIBLE };
            self.couleur(c, couleur);
            rt.FillEllipse(&ellipse(x0 + 28.0 + k as f32 * 9.0, yp, 2.5), &c.pinceau);
        }
        if n > visibles {
            self.couleur(c, &BLANC_DOUX);
            self.texte(c, &f.petit, &format!("+{}", n - visibles), rect(x0 + 28.0 + visibles as f32 * 9.0, yp - 10.0, 60.0, 20.0));
        }
        let restants = cf.liste.iter().filter(|x| x.choix.is_none()).count();
        let aide = if n > 1 {
            format!("LETTRE : CE FICHIER · MAJ+LETTRE OU MAJ+CLIC : LES {restants} SANS REPONSE · ← → : AUTRE CONFLIT · ECHAP : TOUT ANNULER")
        } else {
            "R G N I OU CLIC : CHOISIR · ECHAP : ANNULER".to_string()
        };
        self.aide(c, &aide, rect(x0 + 24.0, y0 + hc - 34.0, w - 48.0, 22.0), Pose::Gauche, &BLANC_TRES_FAIBLE);
    }

    /// Texte raccourci par « … » pour tenir dans `largeur` (recherche du plus long préfixe qui tient)
    fn ecourter(&self, texte: &str, largeur: f32, format: &IDWriteTextFormat) -> String {
        if self.mesurer(texte, format) <= largeur {
            return texte.to_string();
        }
        let mut lettres: Vec<char> = texte.chars().collect();
        while !lettres.is_empty() {
            lettres.pop();
            let essai: String = lettres.iter().collect::<String>().trim_end().to_string() + "…";
            if self.mesurer(&essai, format) <= largeur {
                return essai;
            }
        }
        "…".to_string()
    }

    /// Panneau Historique (`.hist` de la maquette, lot 15B) : tiroir à droite, fond carte, bord gauche et coins gauches de 20, titre et
    /// touche, une ligne par action (heure, libellé, détail, « Annuler » ou ✓). `avance` : 0 rentré, 1 sorti (dépasse un peu au ressort).
    unsafe fn dessiner_historique(&self, c: &Cible2D, vh: &VueHistorique, avance: f32, l: f32, h: f32, pointeur: Option<(f32, f32)>) {
        let rt = &c.rt;
        let f = &self.f;
        let z = zone_tiroir(l, h);
        let hz = z.bottom - z.top;
        let dx = (1.0 - avance) * TIROIR_L * 1.05;
        let avec_note = vh.entrees.iter().any(|e| e.id == 0);
        let (y_liste, y_bas) = (z.top + HIST_ENTETE, hist_bas_liste(&z, avec_note));
        let defil = vh.defilement.clamp(0.0, historique_defilement_max(vh.entrees.len(), &z, avec_note));
        let x_fin = z.right - HIST_PAD;
        let x_texte = z.left + HIST_PAD + HIST_TEMPS_L + 10.0;
        let l_texte = (x_fin - 10.0 - HIST_BOUTON_L - x_texte).max(20.0);
        let mut boutons: Vec<(D2D_RECT_F, u64)> = Vec::new();
        let d = mouvement::Decalage { dx, dy: 0.0, echelle: 1.0, opacite: 1.0 };
        self.avec_decalage(c, &d, (l / 2.0, h / 2.0), rect(0.0, 0.0, l, h), || {
            // Fond : un rectangle arrondi dont le bord droit est hors fenêtre (seuls les coins gauches se voient), puis le bord gauche
            // de 1 px (`border-left`) et le liseré intérieur (`inset 1px 0 0 var(--rim)`)
            let fond = D2D1_ROUNDED_RECT { rect: rect(z.left, z.top, TIROIR_L + 20.0, hz), radiusX: 20.0, radiusY: 20.0 };
            self.couleur_ou(c, &CARTE, &CARTE_CLAIR);
            rt.FillRoundedRectangle(&fond, &c.pinceau);
            self.trame_arrondie(c, &fond);
            rt.PushAxisAlignedClip(&rect(z.left - 1.0, z.top - 1.0, 22.0, hz + 2.0), D2D1_ANTIALIAS_MODE_ALIASED);
            self.couleur(c, &BLANC_TRES_FAIBLE);
            rt.DrawRoundedRectangle(&D2D1_ROUNDED_RECT { rect: rect(z.left + 0.5, z.top + 0.5, TIROIR_L + 19.0, hz - 1.0), radiusX: 19.5, radiusY: 19.5 }, &c.pinceau, 1.0, None);
            rt.PopAxisAlignedClip();
            self.couleur(c, &LIGNE);
            rt.FillRectangle(&rect(z.left + 1.0, z.top + 20.0, 1.0, (hz - 40.0).max(0.0)), &c.pinceau);

            // Titre `h4` : HISTORIQUE (12, espacé, --i3) et la touche
            self.couleur(c, &BLANC_FAIBLE);
            self.texte_espace(c, &f.petit, "HISTORIQUE", rect(z.left + HIST_PAD, z.top + HIST_PAD, 160.0, 20.0));
            let kw = largeur_touches("CTRL+MAJ+H");
            self.touches(c, "CTRL+MAJ+H", x_fin - kw, z.top + HIST_PAD + 10.0, true);

            if vh.entrees.is_empty() {
                self.couleur(c, &BLANC_FAIBLE);
                self.texte(c, &f.petit, "Aucune action pour l'instant", rect(z.left + HIST_PAD, y_liste, TIROIR_L - 2.0 * HIST_PAD, 28.0));
            }
            rt.PushAxisAlignedClip(&rect(z.left + 1.0, y_liste, TIROIR_L - 1.0, (y_bas - y_liste).max(0.0)), D2D1_ANTIALIAS_MODE_ALIASED);
            for (k, e) in vh.entrees.iter().enumerate() {
                let y = y_liste + k as f32 * HIST_RANG - defil;
                if y + HIST_RANG < y_liste || y > y_bas {
                    continue;
                }
                // Filet du bas (`border-bottom: 1px solid var(--s1)`)
                self.couleur(c, &LIGNE);
                rt.FillRectangle(&rect(z.left + HIST_PAD, y + HIST_RANG - 1.0, TIROIR_L - 2.0 * HIST_PAD, 1.0), &c.pinceau);
                let ligne = HIST_RANG - 1.0;
                self.couleur(c, &BLANC_FAIBLE);
                self.texte(c, &f.petit_mono, &crate::journal_historique::heure_affichee(&e.quand, &vh.maintenant), rect(z.left + HIST_PAD, y, HIST_TEMPS_L, ligne));
                self.couleur(c, &BLANC);
                self.texte(c, &f.petit, &self.ecourter(&e.libelle_affiche(), l_texte, &f.petit), rect(x_texte, y + 6.0, l_texte, 20.0));
                self.couleur(c, &BLANC_FAIBLE);
                self.texte(c, &f.petit, &self.ecourter(&e.detail, l_texte, &f.petit), rect(x_texte, y + 25.0, l_texte, 18.0));
                if e.etat == crate::journal_historique::Etat::Annulable {
                    let r = rect(x_fin - HIST_BOUTON_L, y + (ligne - HIST_BOUTON_H) / 2.0, HIST_BOUTON_L, HIST_BOUTON_H);
                    let dessus = pointeur.is_some_and(|(px, py)| dedans(&r, px - dx, py)) && r.bottom > y_liste && r.top < y_bas;
                    let arrondi = D2D1_ROUNDED_RECT { rect: r, radiusX: 8.0, radiusY: 8.0 };
                    self.couleur(c, if dessus { &SURVOL_FORT } else { &SURVOL });
                    rt.FillRoundedRectangle(&arrondi, &c.pinceau);
                    self.couleur(c, if dessus { &ACCENT_BORD } else { &BLANC_TRES_FAIBLE });
                    rt.DrawRoundedRectangle(&arrondi, &c.pinceau, 1.0, None);
                    self.couleur(c, &BLANC);
                    self.texte(c, &f.petit_centre, "Annuler", r);
                    // Seule la part visible du bouton répond aux clics
                    let visible = rect(r.left, r.top.max(y_liste), HIST_BOUTON_L, (r.bottom.min(y_bas) - r.top.max(y_liste)).max(0.0));
                    boutons.push((visible, e.id));
                } else {
                    self.couleur(c, &BLANC_FAIBLE);
                    self.texte(c, &f.petit_droite, "✓", rect(x_fin - HIST_BOUTON_L, y, HIST_BOUTON_L, ligne));
                }
            }
            rt.PopAxisAlignedClip();
            if avec_note {
                self.couleur(c, &BLANC_FAIBLE);
                self.texte(c, &f.petit, "Annulation possible seulement pendant la session", rect(z.left + HIST_PAD, y_bas + 4.0, TIROIR_L - 2.0 * HIST_PAD, 20.0));
            }
        });
        let decale = |r: D2D_RECT_F| rect(r.left + dx, r.top, r.right - r.left, r.bottom - r.top);
        *self.zones_historique.borrow_mut() = ZonesHistorique { tiroir: Some(decale(z)), boutons: boutons.into_iter().map(|(r, id)| (decale(r), id)).collect() };
    }

    /// Coup d'œil : l'élément choisi en grand (galerie photo avec ← →)
    unsafe fn dessiner_galerie(&self, c: &Cible2D, (g, k, n): (&Galerie, usize, usize), l: f32, h: f32) {
        let rt = &c.rt;
        let f = &self.f;
        self.couleur(c, &rgba(0, 0, 0, 0.97));
        rt.FillRectangle(&rect(0.0, HAUTEUR_TITRE + 1.0, l, h - HAUTEUR_TITRE - 1.0), &c.pinceau);
        self.trame_plein(c, &rect(0.0, HAUTEUR_TITRE + 1.0, l, h - HAUTEUR_TITRE - 1.0));
        let zone = rect(40.0, HAUTEUR_TITRE + 24.0, l - 80.0, h - HAUTEUR_TITRE - 120.0);
        let mut dessinee = false;
        if let Some(img) = &g.image {
            let mut cache = c.galerie.borrow_mut();
            if cache.as_ref().map_or(true, |(num, _)| *num != g.numero) {
                *cache = self.bitmap(c, img).map(|b| (g.numero, b));
            }
            if let Some((_, b)) = cache.as_ref() {
                let k = ((zone.right - zone.left) / img.largeur as f32).min((zone.bottom - zone.top) / img.hauteur as f32).min(3.0);
                let (iw, ih) = (img.largeur as f32 * k, img.hauteur as f32 * k);
                let r = rect(zone.left + (zone.right - zone.left - iw) / 2.0, zone.top + (zone.bottom - zone.top - ih) / 2.0, iw, ih);
                rt.DrawBitmap(b, Some(&r), 1.0, D2D1_BITMAP_INTERPOLATION_MODE_LINEAR, None);
                dessinee = true;
            }
        }
        if !dessinee {
            let (cx, cy) = ((zone.left + zone.right) / 2.0, (zone.top + zone.bottom) / 2.0);
            if g.charge {
                self.icone_grande(c, g.icone, cx, cy - 20.0, 26.0, false, 1.0);
                self.couleur(c, &BLANC_DOUX);
                self.texte(c, &f.petit_centre, "PAS D'APERCU POUR CE FICHIER", rect(zone.left, cy + 110.0, zone.right - zone.left, 24.0));
            } else {
                self.couleur(c, &BLANC_DOUX);
                self.texte(c, &f.petit_centre, "…", rect(zone.left, cy - 12.0, zone.right - zone.left, 24.0));
            }
        }
        let yb = h - 84.0;
        self.couleur(c, &BLANC);
        self.texte(c, &f.petit_centre, &g.nom.to_uppercase(), rect(80.0, yb, l - 160.0, 26.0));
        self.couleur(c, &BLANC_DOUX);
        self.texte(c, &f.petit_centre, &g.detail, rect(80.0, yb + 24.0, l - 160.0, 22.0));
        self.aide(c, "← → : PRECEDENT / SUIVANT · ESPACE / ECHAP : FERMER · ENTREE : OUVRIR", rect(24.0, h - 34.0, l - 200.0, 24.0), Pose::Gauche, &BLANC_TRES_FAIBLE);
        // Position en points : où l'on est dans le dossier
        self.couleur(c, &BLANC_DOUX);
        self.texte(c, &f.petit_droite, &format!("{} / {}", k + 1, n), rect(l - 160.0, h - 34.0, 136.0, 24.0));
    }

    /// Texte de description coupé à deux lignes de `largeur` (avec « … » si besoin) et son nombre de lignes (1 ou 2).
    /// DirectWrite ne coupe que dans la largeur : la coupure en hauteur se fait ici, sur la mesure d'une mise en page.
    unsafe fn deux_lignes(&self, texte: &str, largeur: f32) -> (String, usize) {
        let mesure = |t: &str| -> Option<(usize, usize)> {
            let w: Vec<u16> = t.encode_utf16().collect();
            let layout = self.dwrite.CreateTextLayout(&w, &self.f.carte_desc, largeur.max(20.0), 1000.0).ok()?;
            let mut m = DWRITE_TEXT_METRICS::default();
            layout.GetMetrics(&mut m).ok()?;
            let n = m.lineCount as usize;
            if n <= 2 {
                return Some((n, w.len()));
            }
            let mut lignes = vec![DWRITE_LINE_METRICS::default(); n];
            let mut vues = 0u32;
            layout.GetLineMetrics(Some(&mut lignes), &mut vues).ok()?;
            Some((n, lignes.iter().take(2).map(|x| x.length as usize).sum()))
        };
        let Some((n, coupe)) = mesure(texte) else { return (texte.to_string(), 1) };
        if n <= 2 {
            return (texte.to_string(), n.max(1));
        }
        let w: Vec<u16> = texte.encode_utf16().collect();
        let mut base: Vec<char> = String::from_utf16_lossy(&w[..coupe.min(w.len())]).trim_end().chars().collect();
        while !base.is_empty() {
            let essai = format!("{}…", base.iter().collect::<String>().trim_end());
            if mesure(&essai).is_some_and(|(n, _)| n <= 2) {
                return (essai, 2);
            }
            base.pop();
        }
        ("…".to_string(), 1)
    }

    /// Page des réglages, à la place des colonnes et de la liste : rail des catégories à gauche, puis la catégorie
    /// choisie (titre h2 et cartes) à droite (maquette v6 `.set`)
    unsafe fn dessiner_page(&self, c: &Cible2D, app: &App, l: f32, h: f32, vp: &VuePage, premier: bool) {
        let rt = &c.rt;
        let f = &self.f;
        let (gauche, droite) = (page::gauche(), page::droite(l));
        let (haut, bas) = (page::PAGE_Y, h - STATUT_H);
        let p = vp.page;
        let formulaire = p.formulaire.as_ref();
        // Panneau modal : il couvre le chemin, la barre d'actions et le volet, sous la barre de titre
        self.couleur(c, &VOILE);
        rt.FillRectangle(&rect(0.0, HAUTEUR_TITRE, l, bas - HAUTEUR_TITRE), &c.pinceau);
        self.couleur(c, &LIGNE);
        rt.FillRectangle(&rect(REGLAGES_NAV, HAUTEUR_TITRE, 1.0, bas - HAUTEUR_TITRE), &c.pinceau);
        self.couleur(c, &BLANC);
        self.texte(c, &f.carte_titre, "Réglages", rect(page::NAV_X + 12.0, HAUTEUR_TITRE + 14.0, REGLAGES_NAV - 48.0, 30.0));
        for (k, r) in page::zones_nav() {
            let actif = p.categorie == k;
            let survole = app.survol == Cible::ReglagesNav(k);
            let fondu = self.survol_k(Cible::ReglagesNav(k));
            if actif || fondu > 0.0 {
                if actif { self.couleur_ou(c, &SELECTION, &SELECTION_CLAIR); } else { self.couleur_k(c, &SURVOL, fondu); }
                rt.FillRoundedRectangle(&D2D1_ROUNDED_RECT { rect: r, radiusX: REGLAGE_SEG_RAYON, radiusY: REGLAGE_SEG_RAYON }, &c.pinceau);
            }
            // Barre d'accent de 3 x 16 (maquette `.ni .b3`, opacité 200 ms) : celle de la catégorie quittée s'éteint pendant que
            // la nouvelle s'allume
            let barre = self.mouvements.borrow_mut().barre_rail(k, actif, self.t_image.get(), crate::reglages::animations_actives());
            if barre > 0.003 {
                self.couleur_k(c, &ACCENT, barre);
                rt.FillRoundedRectangle(&D2D1_ROUNDED_RECT { rect: rect(r.left + 6.0, (r.top + r.bottom) / 2.0 - 8.0, 3.0, 16.0), radiusX: 1.5, radiusY: 1.5 }, &c.pinceau);
            }
            self.couleur(c, if actif || survole { &BLANC } else { &BLANC_DOUX });
            self.texte(c, &f.volet, page::CATEGORIES[k], rect(r.left + 20.0, r.top, r.right - r.left - 24.0, r.bottom - r.top));
        }
        let fermer = page::zone_fermer(l);
        let fondu_fermer = self.survol_k(Cible::ReglagesFermer);
        if fondu_fermer > 0.0 {
            self.couleur_k(c, &SURVOL, fondu_fermer);
            rt.FillRoundedRectangle(&D2D1_ROUNDED_RECT { rect: fermer, radiusX: 8.0, radiusY: 8.0 }, &c.pinceau);
        }
        self.couleur(c, if app.survol == Cible::ReglagesFermer { &BLANC } else { &BLANC_DOUX });
        let (cx, cy) = ((fermer.left + fermer.right) / 2.0, (fermer.top + fermer.bottom) / 2.0);
        self.trait_signe(c, (cx - 4.0, cy - 4.0), (cx + 4.0, cy + 4.0), 1.4);
        self.trait_signe(c, (cx - 4.0, cy + 4.0), (cx + 4.0, cy - 4.0), 1.4);
        rt.PushAxisAlignedClip(&rect(REGLAGES_NAV + 2.0, haut, l - REGLAGES_NAV - 2.0, bas - haut), D2D1_ANTIALIAS_MODE_ALIASED);
        let positions = page::positions(vp.lignes, p.categorie);
        for (k, (ligne, (y0, hl))) in vp.lignes.iter().zip(positions).enumerate() {
            if hl == 0.0 {
                continue;
            }
            let y = haut + y0 - p.defilement;
            if y + hl < haut || y > bas {
                continue;
            }
            if let Element::Section(titre) = ligne.element {
                self.couleur(c, &BLANC);
                self.texte(c, &f.h2, titre, rect(gauche, y + REGLAGES_PAD_Y, droite - gauche - 48.0, 32.0));
                continue;
            }
            // Ligne d'information (raccourcis) : libellé à gauche, touche à droite, un trait dessous
            if ligne.element == Element::Info && ligne.detail.is_empty() {
                self.couleur(c, &BLANC);
                self.texte(c, &f.liste, &ligne.libelle, rect(gauche + 16.0, y, (droite - gauche) * 0.6, hl));
                if let Valeur::Texte(t) = &ligne.valeur {
                    self.couleur(c, &BLANC_DOUX);
                    self.texte(c, &f.petit_droite, t, rect(droite - 16.0 - 240.0, y, 240.0, hl));
                }
                self.couleur(c, &LIGNE);
                rt.FillRectangle(&rect(gauche, y + hl - 1.0, droite - gauche, 1.0), &c.pinceau);
                continue;
            }
            let carte = page::rect_carte(y, l);
            let arrondi = D2D1_ROUNDED_RECT { rect: carte, radiusX: REGLAGE_CARTE_RAYON, radiusY: REGLAGE_CARTE_RAYON };
            let choisie = p.focus == k && formulaire.is_none();
            self.couleur_ou(c, &CARTE, &CARTE_CLAIR);
            rt.FillRoundedRectangle(&arrondi, &c.pinceau);
            let fondu = self.survol_k(Cible::Reglage(k));
            if fondu > 0.0 {
                self.couleur_k(c, &SURVOL, fondu);
                rt.FillRoundedRectangle(&arrondi, &c.pinceau);
            }
            // Trait de 1 px, tracé au demi-pixel pour rester net
            let bord = D2D1_ROUNDED_RECT { rect: rect(carte.left + 0.5, carte.top + 0.5, carte.right - carte.left - 1.0, carte.bottom - carte.top - 1.0), radiusX: REGLAGE_CARTE_RAYON, radiusY: REGLAGE_CARTE_RAYON };
            self.couleur(c, if choisie { &ACCENT_BORD } else { &LIGNE });
            rt.DrawRoundedRectangle(&bord, &c.pinceau, 1.0, None);

            // Tuile de 36 : l'icône du réglage (celle du lieu pour un stockage)
            let cy = (carte.top + carte.bottom) / 2.0;
            let tuile = rect(carte.left + 16.0, cy - REGLAGE_TUILE / 2.0, REGLAGE_TUILE, REGLAGE_TUILE);
            self.couleur_ou(c, &SELECTION, &SELECTION_CLAIR);
            rt.FillRoundedRectangle(&D2D1_ROUNDED_RECT { rect: tuile, radiusX: REGLAGE_TUILE_RAYON, radiusY: REGLAGE_TUILE_RAYON }, &c.pinceau);
            let icone = match ligne.element {
                Element::Ajoute(i) => app.nuages.iter().chain(&app.serveurs).find(|l| l.ajoute == Some(i)).map_or(Icone::Serveur, |l| l.icone),
                e => e.icone(),
            };
            self.icone(c, icone, tuile.left + REGLAGE_TUILE / 2.0, cy, false, 0.9);

            // Contrôle à droite (padding 16) : sa zone fixe aussi la largeur du texte
            let bord_droit = carte.right - 16.0;
            let mut gauche_controle = bord_droit;
            let mut anneau_controle: Option<D2D_RECT_F> = None;
            match &ligne.valeur {
                Valeur::Rien => {}
                Valeur::Interrupteur(oui) => {
                    let piste = rect(bord_droit - REGLAGE_INTER_L, cy - REGLAGE_INTER_H / 2.0, REGLAGE_INTER_L, REGLAGE_INTER_H);
                    gauche_controle = piste.left;
                    // Lot 12C (`.sw`) : le fond change de couleur en 200 ms, la bille glisse en 300 ms au ressort
                    let (position, fondu) = self.mouvements.borrow_mut().interrupteur(k, *oui, self.t_image.get(), crate::reglages::animations_actives());
                    let (inactif, actif) = (crate::theme::contraste(&BLANC_TRES_FAIBLE), crate::theme::contraste(&ACCENT));
                    c.pinceau.SetColor(&melange_couleurs(&inactif, &actif, fondu));
                    rt.FillRoundedRectangle(&D2D1_ROUNDED_RECT { rect: piste, radiusX: REGLAGE_INTER_H / 2.0, radiusY: REGLAGE_INTER_H / 2.0 }, &c.pinceau);
                    // Bille de 16 à 4 du bord, décalée de 16 quand l'interrupteur est actif ; elle prend la couleur du fond
                    let bx = piste.left + 4.0 + REGLAGE_BILLE / 2.0 + 16.0 * position;
                    let (bille_off, bille_on) = (crate::theme::contraste(&BLANC), crate::theme::contraste(&VOILE));
                    c.pinceau.SetColor(&melange_couleurs(&bille_off, &bille_on, fondu));
                    rt.FillEllipse(&ellipse(bx, cy, REGLAGE_BILLE / 2.0), &c.pinceau);
                }
                Valeur::Segments(options, actif) => {
                    let zones = page::zones_segments(carte, options);
                    let (premier_seg, dernier) = (zones[0], zones[zones.len() - 1]);
                    gauche_controle = premier_seg.left;
                    let cadre = rect(premier_seg.left, premier_seg.top, dernier.right - premier_seg.left, premier_seg.bottom - premier_seg.top);
                    for (s, z) in zones.iter().enumerate() {
                        if s == *actif {
                            self.couleur_ou(c, &SELECTION, &SELECTION_CLAIR);
                            rt.FillRoundedRectangle(&D2D1_ROUNDED_RECT { rect: rect(z.left + 2.0, z.top + 2.0, z.right - z.left - 4.0, z.bottom - z.top - 4.0), radiusX: REGLAGE_SEG_RAYON - 2.0, radiusY: REGLAGE_SEG_RAYON - 2.0 }, &c.pinceau);
                        }
                        self.couleur(c, if s == *actif { &BLANC } else { &BLANC_DOUX });
                        self.texte(c, &f.petit_centre, &options[s], *z);
                    }
                    self.couleur(c, &BLANC_TRES_FAIBLE);
                    rt.DrawRoundedRectangle(&D2D1_ROUNDED_RECT { rect: rect(cadre.left + 0.5, cadre.top + 0.5, cadre.right - cadre.left - 1.0, cadre.bottom - cadre.top - 1.0), radiusX: REGLAGE_SEG_RAYON, radiusY: REGLAGE_SEG_RAYON }, &c.pinceau, 1.0, None);
                }
                Valeur::Choix(t) => {
                    let texte = format!("‹  {t}  ›");
                    let w = self.mesurer(&texte, &f.petit_droite) + 4.0;
                    gauche_controle = bord_droit - w;
                    self.couleur(c, if choisie { &BLANC } else { &BLANC_DOUX });
                    self.texte(c, &f.petit_droite, &texte, rect(gauche_controle, carte.top, w, carte.bottom - carte.top));
                }
                Valeur::Texte(t) => {
                    let w = self.mesurer(t, &f.petit_droite) + 4.0;
                    gauche_controle = bord_droit - w;
                    self.couleur(c, &BLANC_DOUX);
                    self.texte(c, &f.petit_droite, t, rect(gauche_controle, carte.top, w, carte.bottom - carte.top));
                }
                Valeur::Bouton(t) => {
                    let tw = self.mesurer(t, &f.petit) + 28.0;
                    let b = rect(bord_droit - tw, cy - 14.0, tw, 28.0);
                    gauche_controle = b.left;
                    anneau_controle = Some(b);
                    let rb = D2D1_ROUNDED_RECT { rect: b, radiusX: 14.0, radiusY: 14.0 };
                    let rouge = ligne.element == Element::Ajouter || (choisie && ligne.element == Element::Reinitialiser);
                    self.couleur(c, if rouge { &ACCENT } else if choisie { &BLANC } else { &BLANC_DOUX });
                    if rouge && choisie {
                        rt.FillRoundedRectangle(&rb, &c.pinceau);
                    } else {
                        rt.DrawRoundedRectangle(&rb, &c.pinceau, 1.2, None);
                    }
                    self.couleur(c, if rouge && choisie { &ENCRE_SUR_ACCENT } else if choisie || rouge { &BLANC } else { &BLANC_DOUX });
                    self.texte(c, &f.petit_centre, t, b);
                }
            }

            // Titre (14, graisse moyenne) et description (12, deux lignes au plus), centrés dans la hauteur de la carte
            let tx = tuile.right + 16.0;
            let w = (gauche_controle - 16.0 - tx).max(40.0);
            let (desc, n) = self.deux_lignes(&ligne.detail, w);
            let bloc = 20.0 + if desc.is_empty() { 0.0 } else { 2.0 + 16.0 * n as f32 };
            let haut_bloc = cy - bloc / 2.0;
            self.couleur(c, &BLANC);
            self.texte(c, &f.carte_titre, &ligne.libelle, rect(tx, haut_bloc, w, 20.0));
            if !desc.is_empty() {
                self.couleur(c, &BLANC_FAIBLE);
                self.texte(c, &f.carte_desc, &desc, rect(tx, haut_bloc + 22.0, w, 16.0 * n as f32));
            }

            if choisie && vp.focus_clavier {
                match anneau_controle {
                    Some(b) => self.anneau(c, b, 14.0),
                    None => self.anneau(c, carte, REGLAGE_CARTE_RAYON),
                }
            }
        }
        rt.PopAxisAlignedClip();

        // Barre de défilement
        let total = page::hauteur_totale(vp.lignes, p.categorie);
        let visible = bas - haut;
        if total > visible {
            let hb = (visible * visible / total).max(24.0);
            let yb = haut + (visible - hb) * (p.defilement / (total - visible)).clamp(0.0, 1.0);
            self.couleur(c, &BLANC_TRES_FAIBLE);
            rt.FillRoundedRectangle(&D2D1_ROUNDED_RECT { rect: rect(l - 9.0, yb, 3.0, hb), radiusX: 1.5, radiusY: 1.5 }, &c.pinceau);
        }

        // Barre d'état : annonce ou aide clavier
        let ys = h - STATUT_H;
        self.couleur(c, &LIGNE);
        rt.FillRectangle(&rect(0.0, ys, l, 1.0), &c.pinceau);
        // Pixel occupe le bord gauche de la barre d'état (il est dessiné par la fenêtre, une seule fois)
        let decalage = if premier { PIXEL_DECALAGE } else { 0.0 };
        let zone = rect(gauche + decalage, ys, droite - gauche - decalage, STATUT_H);
        match app.annonce.as_ref().filter(|a| a.2.elapsed() < crate::app::DUREE_ANNONCE) {
            Some((texte, erreur, _)) => {
                self.couleur(c, if *erreur { &ERREUR } else { &BLANC });
                rt.FillEllipse(&ellipse(zone.left + 3.0, ys + STATUT_H / 2.0, 2.5), &c.pinceau);
                self.aide(c, texte, rect(zone.left + 12.0, ys, zone.right - zone.left - 12.0, STATUT_H), Pose::Gauche, if *erreur { &ERREUR } else { &BLANC });
            }
            None => {
                let aide = if formulaire.is_some() {
                    "TAB / ↑↓ : CHAMP SUIVANT · ← → : GENRE · ENTREE : AJOUTER · ECHAP : ANNULER"
                } else {
                    "↑↓ : CHOISIR · ENTREE / ESPACE : CHANGER · ← → : VALEUR · TAB : CATÉGORIE · SUPPR : RETIRER · ECHAP : RETOUR"
                };
                self.aide(c, aide, zone, Pose::Gauche, &BLANC_DOUX);
            }
        }

        // Formulaire « ajouter un stockage » : `translateY(14) scale(.96)`, 420 ms
        let d_form = self.decalage_apparition(mouvement::APPARU_FORMULAIRE, formulaire.is_some(), mouvement::APPARU_CARTE_MS, 14.0, 0.96);
        if let Some(fo) = formulaire {
            let carte = page::carte_formulaire(l, h);
            self.avec_decalage(c, &d_form, ((carte.left + carte.right) / 2.0, (carte.top + carte.bottom) / 2.0), rect(0.0, 0.0, l, h), || self.dessiner_formulaire(c, app, l, h, fo));
        }
    }

    /// Carte « ajouter un stockage », par-dessus la page assombrie
    unsafe fn dessiner_formulaire(&self, c: &Cible2D, app: &App, l: f32, h: f32, fo: &page::Formulaire) {
        let rt = &c.rt;
        let f = &self.f;
        self.couleur(c, &rgba(0, 0, 0, 0.55));
        rt.FillRectangle(&rect(0.0, HAUTEUR_TITRE + 1.0, l, h - HAUTEUR_TITRE - 1.0), &c.pinceau);
        let carte = page::carte_formulaire(l, h);
        let arrondi = D2D1_ROUNDED_RECT { rect: carte, radiusX: 14.0, radiusY: 14.0 };
        self.couleur(c, &CHAMP_FOND);
        rt.FillRoundedRectangle(&arrondi, &c.pinceau);
        self.trame_arrondie(c, &arrondi);
        self.couleur(c, &BLANC_TRES_FAIBLE);
        rt.DrawRoundedRectangle(&arrondi, &c.pinceau, 1.0, None);
        self.couleur(c, &ACCENT);
        rt.FillEllipse(&ellipse(carte.left + 28.0, carte.top + 34.0, 4.0), &c.pinceau);
        self.couleur(c, &BLANC);
        self.texte(c, &f.titre, "Ajouter un stockage", rect(carte.left + 42.0, carte.top + 18.0, carte.right - carte.left - 60.0, 32.0));

        let libelle_w = 150.0;
        let mut zones = page::zones_formulaire(carte);
        let boutons = zones.split_off(5);
        for (k, z) in zones {
            let focus = fo.focus == k;
            let survole = app.survol == Cible::Champ(k);
            let utile = k == page::F_GENRE || fo.champ_utile(k);
            let titre = if k == page::F_GENRE { "Genre" } else { page::CHAMPS[k - 1] };
            self.couleur(c, if focus { &BLANC } else if utile { &BLANC_DOUX } else { &BLANC_TRES_FAIBLE });
            self.texte(c, &f.petit, titre, rect(z.left, z.top, libelle_w, z.bottom - z.top));
            let champ = rect(z.left + libelle_w, z.top + 2.0, z.right - z.left - libelle_w, z.bottom - z.top - 4.0);
            if k == page::F_GENRE {
                if focus || survole {
                    self.couleur(c, if focus { &SURVOL_FORT } else { &SURVOL });
                    rt.FillRoundedRectangle(&D2D1_ROUNDED_RECT { rect: champ, radiusX: 7.0, radiusY: 7.0 }, &c.pinceau);
                }
                self.couleur(c, &BLANC);
                self.texte(c, &f.liste, &format!("‹  {}  ›", fo.sorte.libelle()), rect(champ.left + 10.0, champ.top, champ.right - champ.left - 20.0, champ.bottom - champ.top));
                continue;
            }
            if !utile {
                self.couleur(c, &BLANC_TRES_FAIBLE);
                self.texte(c, &f.petit, "Inutile pour un dossier", rect(champ.left + 10.0, champ.top, champ.right - champ.left - 20.0, champ.bottom - champ.top));
                continue;
            }
            let ed = &fo.champs[k - 1];
            // Mot de passe : des étoiles à la place des caractères
            let masque;
            let ed = if k == 4 {
                masque = crate::edition::Edition { index: 0, original: String::new(), texte: vec!['*' as u16; ed.texte.len()], curseur: ed.curseur, ancre: ed.ancre };
                &masque
            } else {
                ed
            };
            if focus {
                self.champ(c, ed, rect(champ.left + 10.0, champ.top, champ.right - champ.left - 10.0, champ.bottom - champ.top));
            } else {
                let cadre = D2D1_ROUNDED_RECT { rect: champ, radiusX: 5.0, radiusY: 5.0 };
                self.couleur(c, if survole { &BLANC_DOUX } else { &BLANC_TRES_FAIBLE });
                rt.DrawRoundedRectangle(&cadre, &c.pinceau, 1.0, None);
                let zone_texte = rect(champ.left + 10.0, champ.top, champ.right - champ.left - 20.0, champ.bottom - champ.top);
                if ed.texte.is_empty() {
                    let exemple = match k {
                        1 => "Nom dans le volet (facultatif)",
                        2 => fo.sorte.exemple(),
                        3 => "Facultatif",
                        _ => "Facultatif · gardé par Windows",
                    };
                    self.couleur(c, &BLANC_TRES_FAIBLE);
                    self.texte(c, &f.petit, exemple, zone_texte);
                } else {
                    self.couleur(c, &BLANC_DOUX);
                    self.texte_w(c, &f.liste, &ed.texte, zone_texte);
                }
            }
        }

        // Aide selon le genre, puis l'erreur éventuelle
        let y = carte.top + 64.0 + 5.0 * page::FORM_LIGNE;
        let aide = match fo.sorte {
            crate::reglages::Sorte::Zima => "ADRESSE VIDE : ZIMAOS EST CHERCHE SUR LE RESEAU · A DISTANCE : L'ADRESSE ZIMA (ZEROTIER)",
            crate::reglages::Sorte::Smb => "NAS, AUTRE PC, BOX : SES DOSSIERS PARTAGES APPARAISSENT DANS SERVEURS",
            crate::reglages::Sorte::WebDav => "NEXTCLOUD, OWNCLOUD, NAS : PAR LE CLIENT WEBDAV DE WINDOWS",
            crate::reglages::Sorte::Sftp => "SERVEUR SSH : CLE SSH SEULEMENT (AGENT OU ~/.SSH), PARCOURU EN LECTURE",
            crate::reglages::Sorte::Dossier => "N'IMPORTE QUEL DOSSIER : GOOGLE DRIVE, LECTEUR RESEAU, CLE USB…",
        };
        let w = carte.right - carte.left - 48.0;
        self.couleur(c, &BLANC_DOUX);
        self.texte(c, &f.petit, aide, rect(carte.left + 24.0, y, w, 20.0));
        self.couleur(c, &BLANC_TRES_FAIBLE);
        self.texte(c, &f.petit, "LE MOT DE PASSE VA DANS LE GESTIONNAIRE D'IDENTIFICATION DE WINDOWS, JAMAIS DANS UN FICHIER", rect(carte.left + 24.0, y + 20.0, w, 20.0));
        if let Some(e) = &fo.erreur {
            self.couleur(c, &ERREUR);
            rt.FillEllipse(&ellipse(carte.left + 27.0, y + 54.0, 2.5), &c.pinceau);
            self.texte(c, &f.petit, e, rect(carte.left + 36.0, y + 44.0, w - 12.0, 20.0));
        }

        for (k, b) in boutons {
            let focus = fo.focus == k;
            let survole = app.survol == Cible::Champ(k);
            let rb = D2D1_ROUNDED_RECT { rect: b, radiusX: 19.0, radiusY: 19.0 };
            let ajouter = k == page::F_AJOUTER;
            let texte = if !ajouter {
                "Annuler"
            } else if fo.en_cours {
                "Vérification…"
            } else if fo.forcer {
                "Ajouter quand même"
            } else {
                "Ajouter"
            };
            if ajouter && (focus || survole) {
                self.couleur(c, &ACCENT);
                rt.FillRoundedRectangle(&rb, &c.pinceau);
            } else {
                self.couleur(c, if ajouter { &ACCENT } else if focus || survole { &BLANC } else { &BLANC_DOUX });
                rt.DrawRoundedRectangle(&rb, &c.pinceau, if focus { 2.0 } else { 1.2 }, None);
            }
            self.couleur(c, if ajouter && (focus || survole) { &ENCRE_SUR_ACCENT } else { &BLANC });
            self.texte(c, &f.petit_centre, texte, b);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Maquette `.sq` : carrés de 5 DIP, 2 DIP d'écart. Le dessin n'ajoute aucune zone cliquable (`Cible` inchangée) : la hauteur d'une
    /// ligne et la case de la grille ne dépendent pas de l'étiquette.
    #[test]
    fn carres_d_etiquette_de_la_maquette() {
        assert_eq!([largeur_etiquette(1), largeur_etiquette(2), largeur_etiquette(3)], [5.0, 12.0, 19.0]);
        assert_eq!(largeur_etiquette(0), 0.0);
    }

    fn touches(r: &str) -> Vec<String> {
        decouper_touches(r)
    }

    /// Une touche par cadre : les cas du brief (combinaison, touche seule, virgule, nom à espace, vide, « + » comme touche)
    #[test]
    fn raccourci_decoupe_en_touches() {
        assert_eq!(touches("CTRL+MAJ+P"), ["CTRL", "MAJ", "P"]);
        assert_eq!(touches("F2"), ["F2"]);
        assert_eq!(touches("CTRL+,"), ["CTRL", ","]);
        assert_eq!(touches("ALT+FLECHE GAUCHE"), ["ALT", "FLECHE GAUCHE"]);
        assert!(touches("").is_empty());
        assert_eq!(touches("CTRL++"), ["CTRL", "+"]);
        assert_eq!(touches("+"), ["+"]);
        assert_eq!(touches("MAJ+F10"), ["MAJ", "F10"]);
    }

    /// Largeur : nulle sans touche, une touche large comme son texte, un écart de 4 DIP entre cadres
    #[test]
    fn largeur_des_touches() {
        assert_eq!(largeur_touches(""), 0.0);
        let (a, b) = (largeur_touches("CTRL"), largeur_touches("P"));
        assert!(a > b && b >= touche_hauteur(), "une touche plus longue est plus large, jamais plus étroite que haute");
        let somme = largeur_touches("CTRL+P");
        assert!((somme - (a + b + TOUCHE_ESPACE)).abs() < 0.001, "{somme} ≠ {a} + {b} + écart");
    }

    fn morceaux(t: &str) -> Vec<(bool, &str)> {
        segmenter_aide(t).into_iter().map(|m| match m {
            Morceau::Texte(x) => (false, x),
            Morceau::Touches(x) => (true, x),
        }).collect()
    }

    /// Les aides inline : seuls les raccourcis passent en touches, le reste est rendu tel quel (espaces compris)
    #[test]
    fn aide_decoupee_en_texte_et_touches() {
        assert_eq!(morceaux("ENTREE OUVRE · CTRL+F : SOUS-DOSSIERS"), [(true, "ENTREE"), (false, " OUVRE · "), (true, "CTRL+F"), (false, " : SOUS-DOSSIERS")]);
        assert_eq!(morceaux("REGEX (CTRL+R) : OUI"), [(false, "REGEX ("), (true, "CTRL+R"), (false, ") : OUI")]);
        assert_eq!(morceaux("COUPER · CTRL+X"), [(false, "COUPER · "), (true, "CTRL+X")]);
        assert_eq!(morceaux("RENOMMER · F2"), [(false, "RENOMMER · "), (true, "F2")]);
        assert_eq!(morceaux("CTRL+,"), [(true, "CTRL+,")]);
        assert_eq!(morceaux("TAB, ECHAP."), [(true, "TAB"), (false, ", "), (true, "ECHAP"), (false, ".")]);
        // Rien qui ressemble à une touche : un seul morceau de texte
        assert_eq!(morceaux("DOSSIER VIDE · F13 · FICHIER"), [(false, "DOSSIER VIDE · F13 · FICHIER")]);
        assert_eq!(morceaux("TABLE ESPACES ENTREES"), [(false, "TABLE ESPACES ENTREES")]);
        assert!(morceaux("").is_empty());
        // Concaténés, les morceaux redonnent le texte d'entrée
        let t = "MAJ+LETTRE OU MAJ+CLIC : LES 3 SANS REPONSE · ← → : AUTRE · ECHAP : ANNULER";
        assert_eq!(morceaux(t).iter().map(|m| m.1).collect::<String>(), t);
    }

    /// Le tiroir Historique : 340 DIP contre le bord droit, du haut de la zone de contenu au pied (`top:54px; bottom:40px` de la maquette)
    #[test]
    fn tiroir_historique_geometrie() {
        let z = zone_tiroir(1440.0, 900.0);
        assert_eq!((z.left, z.right), (1440.0 - 340.0, 1440.0));
        assert_eq!((z.top, z.bottom), (HAUTEUR_TITRE + 1.0, 900.0 - STATUT_H));
        // Quelques entrées tiennent : pas de défilement ; beaucoup : le défilement va jusqu'au bas de la dernière
        assert_eq!(historique_defilement_max(4, &z, false), 0.0);
        let max = historique_defilement_max(200, &z, true);
        assert!(max > 0.0);
        let visible = hist_bas_liste(&z, true) - (z.top + HIST_ENTETE);
        assert!((max + visible - 200.0 * HIST_RANG).abs() < 0.01);
        // La note « session » réduit la place de la liste
        assert!(historique_defilement_max(30, &z, true) > historique_defilement_max(30, &z, false));
    }

    #[test]
    fn clic_dans_le_tiroir_historique() {
        let mut zones = ZonesHistorique::default();
        // Pas encore dessiné (ou rentré) : tout est dehors
        assert_eq!(zones.cible(1400.0, 300.0), CibleHistorique::Dehors);
        let tiroir = zone_tiroir(1440.0, 900.0);
        zones.tiroir = Some(tiroir);
        zones.boutons = vec![(rect(1360.0, 100.0, 64.0, 26.0), 7), (rect(1360.0, 151.0, 64.0, 26.0), 9)];
        assert_eq!(zones.cible(1380.0, 110.0), CibleHistorique::Annuler(7));
        assert_eq!(zones.cible(1380.0, 160.0), CibleHistorique::Annuler(9));
        assert_eq!(zones.cible(1380.0, 140.0), CibleHistorique::Tiroir);
        assert_eq!(zones.cible(500.0, 300.0), CibleHistorique::Dehors);
    }
}
