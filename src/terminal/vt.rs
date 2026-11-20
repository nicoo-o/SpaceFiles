//! Émulateur de terminal (PLAN-TERMINAL, lot T1) : transforme le flux d'octets d'un shell (séquences VT, texte
//! UTF-8) en une grille de cellules, un curseur, un historique de défilement et quelques métadonnées (titre,
//! dossier courant, marques d'invite). Aucune dépendance à Win32 : tout est testable avec des octets.
//!
//! Le découpage en séquences est confié à `vte` (machine d'états de Paul Williams, sans allocation) ; ce fichier
//! ne contient que le sens donné à chaque séquence. ConPTY ne relaie pas le flux brut du shell : il re-synthétise
//! l'écran (positionnements absolus, effacements). Les tests s'appuient donc sur des flux réels enregistrés en T0
//! (`docs/terminal/flux-t0/`), pas sur des séquences imaginées.
//!
//! Garanties (l'app est compilée en `panic = "abort"`, une panique ferme tout sans message) :
//! - aucun `unwrap`, aucun indice non vérifié sur une entrée venue du shell ; les invariants (grille de
//!   `lignes × colonnes`, curseur dedans, région de défilement valide) sont rétablis après chaque séquence ;
//! - mémoire bornée : historique plafonné (5 000 lignes par défaut, lignes rognées de leurs blancs de fin),
//!   taille de grille plafonnée, file de réponses plafonnée, titre et dossier tronqués ;
//! - séquences coupées en deux lectures, UTF-8 coupé au milieu d'un caractère : sans effet visible (test par octet).
//!
//! Limites assumées (rien de cela ne sert à PowerShell, cmd ni aux outils courants lancés sous ConPTY) :
//! caractères combinants et sélecteurs de variation ignorés (le caractère de base est affiché seul) ; pas de
//! jeux de caractères DEC (`ESC ( 0`) ; pas de reflux du texte au changement de largeur (les lignes sont rognées ou
//! complétées) ; pas de DECCOLM, de mode ligne seule ni de défilement par colonnes ; sixel et DCS ignorés.

// Consommé par la session (T2) et le rendu (T3) ; retirer cette ligne quand ils existeront.
#![allow(dead_code)]

use std::collections::VecDeque;

use vte::{Params, Parser, Perform};

/// Lignes d'historique gardées par défaut (hors écran visible).
pub const HISTORIQUE_DEFAUT: usize = 5_000;
/// Plafonds de taille de la grille : un `ESC[8;…t` ou une taille de fenêtre aberrante ne doit pas allouer sans fin.
pub const COLONNES_MAX: usize = 1_000;
pub const LIGNES_MAX: usize = 500;
const TITRE_MAX: usize = 256;
const DOSSIER_MAX: usize = 1_024;
/// Réponses en attente d'envoi au shell (position du curseur, attributs de l'appareil) : un flux qui en réclame
/// sans fin ne doit pas faire grossir la file.
const REPONSES_MAX: usize = 4_096;
const MARQUES_MAX: usize = 256;

// ───────────────────────── types publics ─────────────────────────

/// Couleur d'une cellule : palette 256 (0 à 15 = couleurs du thème, 16 à 255 = cube et gris fixes) ou RVB.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Couleur {
    #[default]
    Defaut,
    Indexee(u8),
    Rvb(u8, u8, u8),
}

/// Attributs d'affichage d'une cellule. `inverse` n'est pas appliqué ici : c'est au rendu d'échanger fg et bg.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Attr {
    pub fg: Couleur,
    pub bg: Couleur,
    pub drapeaux: u8,
}

impl Attr {
    pub const GRAS: u8 = 1;
    pub const PALE: u8 = 2;
    pub const ITALIQUE: u8 = 4;
    pub const SOULIGNE: u8 = 8;
    pub const INVERSE: u8 = 16;
    pub const BARRE: u8 = 32;
    pub const CACHE: u8 = 64;

    pub fn a(&self, drapeau: u8) -> bool {
        self.drapeaux & drapeau != 0
    }

    fn regler(&mut self, drapeau: u8, actif: bool) {
        if actif {
            self.drapeaux |= drapeau;
        } else {
            self.drapeaux &= !drapeau;
        }
    }
}

/// Une case de la grille. `largeur` : 1 = normale, 2 = moitié gauche d'un caractère large (CJK, emoji),
/// 0 = moitié droite de ce caractère (`c` y vaut `' '`, le rendu ne la dessine pas).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Cellule {
    pub c: char,
    pub attr: Attr,
    pub largeur: u8,
}

impl Cellule {
    pub const VIDE: Cellule = Cellule {
        c: ' ',
        attr: Attr { fg: Couleur::Defaut, bg: Couleur::Defaut, drapeaux: 0 },
        largeur: 1,
    };

    /// Case blanche portant seulement la couleur de fond courante (effacement « BCE » des terminaux modernes).
    fn vide(bg: Couleur) -> Cellule {
        Cellule { attr: Attr { fg: Couleur::Defaut, bg, drapeaux: 0 }, ..Cellule::VIDE }
    }
}

impl Default for Cellule {
    fn default() -> Self {
        Cellule::VIDE
    }
}

/// Repère posé par le shell (OSC 133) : `A` début d'invite, `B` fin d'invite (début de saisie), `C` début de la
/// sortie de la commande, `D` fin de commande (avec son code de sortie). `ligne` est absolue : elle ne change
/// pas quand l'écran défile, `0` étant la toute première ligne jamais écrite.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Marque {
    pub genre: char,
    pub ligne: u64,
    pub code: Option<i32>,
}

/// Modes que l'application dans le terminal peut changer et que la session ou le rendu doivent connaître.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Modes {
    /// DECTCEM `?25` : curseur affiché.
    pub curseur_visible: bool,
    /// DECAWM `?7` : retour automatique à la ligne en bout de ligne.
    pub retour_auto: bool,
    /// DECCKM `?1` : les flèches s'envoient sous la forme `ESC O A` et non `ESC [ A`.
    pub touches_curseur_appli: bool,
    /// `?2004` : le collage doit être encadré par `ESC[200~` … `ESC[201~`.
    pub collage_encadre: bool,
    /// `?1004` : l'application veut être prévenue quand la fenêtre gagne ou perd le focus.
    pub focus: bool,
    /// DECOM `?6` : les positions du curseur sont relatives à la région de défilement.
    pub origine: bool,
    /// IRM `4` : un caractère écrit pousse la suite de la ligne vers la droite au lieu de la recouvrir.
    pub insertion: bool,
    /// `?1000`, `?1002`, `?1003`, `?1006` : suivi de la souris demandé (utilisé plus tard par le panneau).
    pub souris_appui: bool,
    pub souris_glisse: bool,
    pub souris_mouvement: bool,
    pub souris_sgr: bool,
}

impl Default for Modes {
    fn default() -> Self {
        Modes {
            curseur_visible: true,
            retour_auto: true,
            touches_curseur_appli: false,
            collage_encadre: false,
            focus: false,
            origine: false,
            insertion: false,
            souris_appui: false,
            souris_glisse: false,
            souris_mouvement: false,
            souris_sgr: false,
        }
    }
}

// ───────────────────────── terminal ─────────────────────────

/// Le terminal : un analyseur `vte` et l'état qu'il met à jour.
pub struct Terminal {
    parser: Parser,
    etat: Etat,
    /// Début d'un caractère UTF-8 coupé à la fin du dernier lot (3 octets au plus).
    queue: Vec<u8>,
}

/// Nombre d'octets en fin de lot qui forment le début d'un caractère UTF-8 pas encore complet (0 à 3).
fn queue_utf8_incomplete(b: &[u8]) -> usize {
    for k in 1..=3usize.min(b.len()) {
        let o = b[b.len() - k];
        if (0x80..=0xBF).contains(&o) {
            continue;
        }
        let besoin = match o {
            0xC2..=0xDF => 2,
            0xE0..=0xEF => 3,
            0xF0..=0xF4 => 4,
            _ => return 0,
        };
        return if besoin > k { k } else { 0 };
    }
    0
}

impl Terminal {
    pub fn new(colonnes: usize, lignes: usize) -> Terminal {
        Terminal::avec_historique(colonnes, lignes, HISTORIQUE_DEFAUT)
    }

    pub fn avec_historique(colonnes: usize, lignes: usize, historique_max: usize) -> Terminal {
        Terminal { parser: Parser::new(), etat: Etat::new(colonnes, lignes, historique_max), queue: Vec::new() }
    }

    /// Fait avancer l'émulateur avec des octets venus du shell. N'importe quel découpage donne le même résultat.
    pub fn ecrire(&mut self, octets: &[u8]) {
        if octets.is_empty() {
            return;
        }
        // On ne confie jamais à `vte` un caractère UTF-8 coupé en fin de lot : sa reprise (0.15.0,
        // `advance_partial_utf8`) perd un octet quand l'accent coupé est suivi d'un autre caractère incomplet.
        // La queue incomplète (3 octets au plus) attend le lot suivant.
        let tampon;
        let mut a_traiter = octets;
        if !self.queue.is_empty() {
            let mut t = std::mem::take(&mut self.queue);
            t.extend_from_slice(octets);
            tampon = t;
            a_traiter = &tampon;
        }
        let coupe = queue_utf8_incomplete(a_traiter);
        let (utile, reste) = a_traiter.split_at(a_traiter.len() - coupe);
        let reste = reste.to_vec();
        self.parser.advance(&mut self.etat, utile);
        self.queue = reste;
        self.etat.generation = self.etat.generation.wrapping_add(1);
    }

    /// Change la taille de la grille. Hauteur : les lignes qui sortent par le haut passent dans l'historique et
    /// un agrandissement les en fait revenir, rien n'est perdu ; largeur : les lignes sont rognées ou complétées.
    pub fn redimensionner(&mut self, colonnes: usize, lignes: usize) {
        self.etat.redimensionner(colonnes, lignes);
        self.etat.generation = self.etat.generation.wrapping_add(1);
    }

    pub fn colonnes(&self) -> usize {
        self.etat.cols
    }

    pub fn lignes(&self) -> usize {
        self.etat.lignes
    }

    /// Compteur qui change à chaque `ecrire` ou `redimensionner` : le rendu ne redessine que s'il a bougé.
    pub fn generation(&self) -> u64 {
        self.etat.generation
    }

    pub fn cellule(&self, colonne: usize, ligne: usize) -> Option<&Cellule> {
        self.etat.ecran.get(ligne)?.get(colonne)
    }

    pub fn ligne(&self, ligne: usize) -> Option<&[Cellule]> {
        self.etat.ecran.get(ligne).map(|l| l.as_slice())
    }

    /// Texte d'une ligne de l'écran, sans les blancs de fin ni les moitiés droites de caractères larges.
    pub fn texte_ligne(&self, ligne: usize) -> String {
        self.ligne(ligne).map(texte_de).unwrap_or_default()
    }

    /// Texte de tout l'écran, une ligne par `\n`, sans les lignes vides de fin.
    pub fn texte_ecran(&self) -> String {
        let mut lignes: Vec<String> = (0..self.etat.lignes).map(|l| self.texte_ligne(l)).collect();
        while lignes.last().is_some_and(|l| l.is_empty()) {
            lignes.pop();
        }
        lignes.join("\n")
    }

    /// Nombre de lignes d'historique (hors écran), de la plus ancienne (indice 0) à la plus récente.
    pub fn historique_len(&self) -> usize {
        self.etat.historique.len()
    }

    /// Une ligne d'historique. Elle peut être plus courte que la grille (blancs de fin retirés) ou plus étroite
    /// (grille élargie depuis) : le rendu complète par des blancs.
    pub fn ligne_historique(&self, indice: usize) -> Option<&[Cellule]> {
        self.etat.historique.get(indice).map(|l| l.as_slice())
    }

    pub fn texte_historique(&self, indice: usize) -> Option<String> {
        self.ligne_historique(indice).map(texte_de)
    }

    /// Lignes sorties de l'historique depuis le début (plafond atteint) : `ligne absolue = écoulées + indice`.
    pub fn lignes_ecoulees(&self) -> u64 {
        self.etat.lignes_ecoulees
    }

    /// Position du curseur (colonne, ligne), 0 en haut à gauche.
    pub fn curseur(&self) -> (usize, usize) {
        (self.etat.col, self.etat.lig)
    }

    pub fn curseur_visible(&self) -> bool {
        self.etat.modes.curseur_visible
    }

    /// Forme demandée par DECSCUSR : 0 ou 1 bloc clignotant, 2 bloc, 3 ou 4 souligné, 5 ou 6 barre.
    pub fn forme_curseur(&self) -> u8 {
        self.etat.forme_curseur
    }

    pub fn modes(&self) -> Modes {
        self.etat.modes
    }

    pub fn ecran_alterne(&self) -> bool {
        self.etat.principal.is_some()
    }

    pub fn titre(&self) -> &str {
        &self.etat.titre
    }

    /// Dossier courant annoncé par le shell (OSC 9;9), `None` tant qu'il n'a rien dit.
    pub fn dossier(&self) -> Option<&str> {
        self.etat.dossier.as_deref()
    }

    /// Combien de fois le shell a annoncé son dossier : monte à chaque invite affichée, même sans changement de dossier.
    pub fn annonces_dossier(&self) -> u64 {
        self.etat.annonces_dossier
    }

    pub fn marques(&self) -> &[Marque] {
        &self.etat.marques
    }

    /// Octets que le shell attend en retour (réponse à `ESC[6n`, à `ESC[c`…) : à écrire dans le tube d'entrée.
    pub fn prendre_reponses(&mut self) -> Vec<u8> {
        std::mem::take(&mut self.etat.reponses)
    }

    /// Nombre de BEL reçus depuis le dernier appel (le panneau décide : flash, son ou rien).
    pub fn prendre_cloches(&mut self) -> u32 {
        std::mem::take(&mut self.etat.cloches)
    }
}

fn texte_de(ligne: &[Cellule]) -> String {
    let mut s: String = ligne.iter().filter(|c| c.largeur != 0).map(|c| c.c).collect();
    let fin = s.trim_end_matches(' ').len();
    s.truncate(fin);
    s
}

// ───────────────────────── état interne ─────────────────────────

/// Curseur mémorisé par ESC 7 / CSI s (et par le mode 1049).
#[derive(Clone, Copy)]
struct CurseurSauve {
    col: usize,
    lig: usize,
    attr: Attr,
    attente: bool,
    origine: bool,
}

struct Etat {
    cols: usize,
    lignes: usize,
    /// Écran visible : toujours `lignes` rangées de `cols` cellules.
    ecran: Vec<Vec<Cellule>>,
    /// Écran principal mis de côté pendant que l'écran alterné (éditeur, `less`…) est affiché. `Some` = on est
    /// sur l'écran alterné, qui n'a pas d'historique.
    principal: Option<Vec<Vec<Cellule>>>,
    historique: VecDeque<Vec<Cellule>>,
    historique_max: usize,
    lignes_ecoulees: u64,
    col: usize,
    lig: usize,
    /// Le dernier caractère a rempli la dernière colonne : le suivant passera à la ligne (comportement des VT100).
    attente: bool,
    attr: Attr,
    /// Région de défilement (haut et bas inclus).
    haut: usize,
    bas: usize,
    sauve: Option<CurseurSauve>,
    sauve_alterne: Option<CurseurSauve>,
    tabs: Vec<bool>,
    modes: Modes,
    forme_curseur: u8,
    titre: String,
    dossier: Option<String>,
    /// Nombre d'annonces de dossier reçues (une par invite) : un dossier inchangé compte quand même, c'est le signe qu'une invite vient de s'afficher
    annonces_dossier: u64,
    marques: Vec<Marque>,
    reponses: Vec<u8>,
    cloches: u32,
    generation: u64,
}

fn ligne_vide(cols: usize, bg: Couleur) -> Vec<Cellule> {
    vec![Cellule::vide(bg); cols]
}

fn tabs_par_defaut(cols: usize) -> Vec<bool> {
    (0..cols).map(|c| c % 8 == 0 && c > 0).collect()
}

/// Rogne ou complète une ligne ; une moitié gauche de caractère large coupée par le bord redevient un blanc.
fn ajuster_largeur(ligne: &mut Vec<Cellule>, cols: usize) {
    ligne.resize(cols, Cellule::VIDE);
    if let Some(derniere) = ligne.last_mut() {
        if derniere.largeur == 2 {
            *derniere = Cellule::VIDE;
        }
    }
}

fn ajuster_hauteur_simple(ecran: &mut Vec<Vec<Cellule>>, lignes: usize, cols: usize) {
    ecran.truncate(lignes);
    while ecran.len() < lignes {
        ecran.push(ligne_vide(cols, Couleur::Defaut));
    }
}

fn ligne_blanche(ligne: &[Cellule]) -> bool {
    ligne.iter().all(|c| c.c == ' ')
}

impl Etat {
    fn new(cols: usize, lignes: usize, historique_max: usize) -> Etat {
        let cols = cols.clamp(1, COLONNES_MAX);
        let lignes = lignes.clamp(1, LIGNES_MAX);
        Etat {
            cols,
            lignes,
            ecran: (0..lignes).map(|_| ligne_vide(cols, Couleur::Defaut)).collect(),
            principal: None,
            historique: VecDeque::new(),
            historique_max,
            lignes_ecoulees: 0,
            col: 0,
            lig: 0,
            attente: false,
            attr: Attr::default(),
            haut: 0,
            bas: lignes - 1,
            sauve: None,
            sauve_alterne: None,
            tabs: tabs_par_defaut(cols),
            modes: Modes::default(),
            forme_curseur: 0,
            titre: String::new(),
            dossier: None,
            annonces_dossier: 0,
            marques: Vec::new(),
            reponses: Vec::new(),
            cloches: 0,
            generation: 0,
        }
    }

    fn alterne(&self) -> bool {
        self.principal.is_some()
    }

    fn vide_ligne(&self) -> Vec<Cellule> {
        ligne_vide(self.cols, self.attr.bg)
    }

    fn cellule_mut(&mut self, lig: usize, col: usize) -> Option<&mut Cellule> {
        self.ecran.get_mut(lig)?.get_mut(col)
    }

    fn ligne_absolue(&self) -> u64 {
        self.lignes_ecoulees + self.historique.len() as u64 + self.lig as u64
    }

    /// Rétablit les invariants après une séquence : curseur dans la grille, région de défilement valide.
    fn fixer_curseur(&mut self) {
        self.col = self.col.min(self.cols - 1);
        self.lig = self.lig.min(self.lignes - 1);
        if self.haut > self.bas || self.bas >= self.lignes {
            self.haut = 0;
            self.bas = self.lignes - 1;
        }
    }

    // ── défilement ──

    fn vers_historique(&mut self, mut ligne: Vec<Cellule>) {
        if self.historique_max == 0 {
            self.lignes_ecoulees += 1;
            return;
        }
        // Les blancs de fin ne servent à rien : sans cela 5 000 lignes de 200 colonnes pèsent 16 Mo.
        while ligne.last().is_some_and(|c| *c == Cellule::VIDE) {
            ligne.pop();
        }
        ligne.shrink_to_fit();
        self.historique.push_back(ligne);
        while self.historique.len() > self.historique_max {
            self.historique.pop_front();
            self.lignes_ecoulees += 1;
        }
    }

    fn defiler_haut(&mut self, n: usize) {
        if self.haut > self.bas || self.bas >= self.ecran.len() {
            return;
        }
        let n = n.min(self.bas - self.haut + 1);
        for _ in 0..n {
            let ligne = self.ecran.remove(self.haut);
            let vide = self.vide_ligne();
            self.ecran.insert(self.bas, vide);
            // Seules les lignes qui quittent le haut de l'écran principal entier font l'historique.
            if self.haut == 0 && !self.alterne() {
                self.vers_historique(ligne);
            }
        }
    }

    fn defiler_bas(&mut self, n: usize) {
        if self.haut > self.bas || self.bas >= self.ecran.len() {
            return;
        }
        let n = n.min(self.bas - self.haut + 1);
        for _ in 0..n {
            self.ecran.remove(self.bas);
            let vide = self.vide_ligne();
            self.ecran.insert(self.haut, vide);
        }
    }

    /// Descend d'une ligne ; en bas de la région, c'est la région qui défile.
    fn index(&mut self) {
        if self.lig == self.bas {
            self.defiler_haut(1);
        } else if self.lig + 1 < self.lignes {
            self.lig += 1;
        }
    }

    fn index_inverse(&mut self) {
        if self.lig == self.haut {
            self.defiler_bas(1);
        } else if self.lig > 0 {
            self.lig -= 1;
        }
    }

    // ── écriture de texte ──

    fn ecrire_char(&mut self, c: char) {
        let w = largeur_char(c);
        if w == 0 {
            return;
        }
        if self.attente {
            if self.modes.retour_auto {
                self.col = 0;
                self.index();
            }
            self.attente = false;
        }
        if w == 2 && self.col + 1 >= self.cols {
            if self.cols < 2 {
                return;
            }
            if self.modes.retour_auto {
                // La moitié droite ne tient pas : la dernière case reste vide, le caractère passe à la ligne.
                let (l, k, bg) = (self.lig, self.col, self.attr.bg);
                if let Some(ligne) = self.ecran.get_mut(l) {
                    // La case peut être la moitié droite d'un caractère large : sa moitié gauche redevient un blanc.
                    liberer(ligne, k);
                    if let Some(x) = ligne.get_mut(k) {
                        *x = Cellule::vide(bg);
                    }
                }
                self.col = 0;
                self.index();
            } else {
                self.col = self.cols - 2;
            }
        }
        if self.modes.insertion {
            self.inserer_cellules(w);
        }
        self.poser(c, w);
    }

    fn poser(&mut self, c: char, w: usize) {
        let (l, k, attr, cols) = (self.lig, self.col, self.attr, self.cols);
        if let Some(ligne) = self.ecran.get_mut(l) {
            // Recouvrir une moitié de caractère large : l'autre moitié devient un blanc.
            liberer(ligne, k);
            if w == 2 {
                liberer(ligne, k + 1);
            }
            if let Some(x) = ligne.get_mut(k) {
                *x = Cellule { c, attr, largeur: w as u8 };
            }
            if w == 2 {
                if let Some(x) = ligne.get_mut(k + 1) {
                    *x = Cellule { c: ' ', attr, largeur: 0 };
                }
            }
        }
        let suivant = k + w;
        if suivant >= cols {
            self.col = cols - 1;
            self.attente = self.modes.retour_auto;
        } else {
            self.col = suivant;
        }
    }

    fn inserer_cellules(&mut self, n: usize) {
        let (l, k, cols, bg) = (self.lig, self.col, self.cols, self.attr.bg);
        if let Some(ligne) = self.ecran.get_mut(l) {
            let n = n.min(cols.saturating_sub(k));
            if k <= ligne.len() {
                ligne.splice(k..k, std::iter::repeat(Cellule::vide(bg)).take(n));
            }
            ligne.truncate(cols);
        }
        self.nettoyer(l);
    }

    // ── effacement ──

    /// Efface `[de, a)` sur une ligne (a exclu, borné à la largeur).
    fn effacer_plage(&mut self, l: usize, de: usize, a: usize) {
        let bg = self.attr.bg;
        if let Some(ligne) = self.ecran.get_mut(l) {
            let a = a.min(ligne.len());
            for x in ligne.iter_mut().take(a).skip(de) {
                *x = Cellule::vide(bg);
            }
        }
        self.nettoyer(l);
    }

    fn effacer_lignes(&mut self, de: usize, a: usize) {
        for l in de..a.min(self.lignes) {
            self.effacer_plage(l, 0, self.cols);
        }
    }

    /// Supprime les moitiés orphelines de caractères larges d'une ligne (après un effacement ou un décalage).
    fn nettoyer(&mut self, l: usize) {
        let Some(ligne) = self.ecran.get_mut(l) else { return };
        for i in 0..ligne.len() {
            let orpheline = match ligne[i].largeur {
                2 => ligne.get(i + 1).map_or(true, |s| s.largeur != 0),
                0 => i == 0 || ligne[i - 1].largeur != 2,
                _ => false,
            };
            if orpheline {
                ligne[i] = Cellule::VIDE;
            }
        }
    }

    // ── curseur ──

    fn aller(&mut self, l: usize, c: usize) {
        self.lig = if self.modes.origine { (self.haut + l).min(self.bas) } else { l.min(self.lignes - 1) };
        self.col = c.min(self.cols - 1);
        self.attente = false;
    }

    fn haut_curseur(&mut self, n: usize) {
        let min = if self.lig >= self.haut { self.haut } else { 0 };
        self.lig = self.lig.saturating_sub(n).max(min);
        self.attente = false;
    }

    fn bas_curseur(&mut self, n: usize) {
        let max = if self.lig <= self.bas { self.bas } else { self.lignes - 1 };
        self.lig = (self.lig + n).min(max);
        self.attente = false;
    }

    fn tab_suivante(&mut self, n: usize) {
        for _ in 0..n {
            let mut c = self.col + 1;
            while c < self.cols - 1 && !self.tabs.get(c).copied().unwrap_or(false) {
                c += 1;
            }
            self.col = c.min(self.cols - 1);
        }
        self.attente = false;
    }

    fn tab_precedente(&mut self, n: usize) {
        for _ in 0..n {
            let mut c = self.col.saturating_sub(1);
            while c > 0 && !self.tabs.get(c).copied().unwrap_or(false) {
                c -= 1;
            }
            self.col = c;
        }
        self.attente = false;
    }

    fn sauver_curseur(&self) -> CurseurSauve {
        CurseurSauve { col: self.col, lig: self.lig, attr: self.attr, attente: self.attente, origine: self.modes.origine }
    }

    fn appliquer_curseur(&mut self, s: CurseurSauve) {
        self.col = s.col;
        self.lig = s.lig;
        self.attr = s.attr;
        self.attente = s.attente;
        self.modes.origine = s.origine;
        self.fixer_curseur();
    }

    // ── écran alterné ──

    fn entrer_alterne(&mut self, sauver: bool) {
        if self.alterne() {
            return;
        }
        if sauver {
            self.sauve_alterne = Some(self.sauver_curseur());
        }
        let vide = (0..self.lignes).map(|_| ligne_vide(self.cols, Couleur::Defaut)).collect();
        self.principal = Some(std::mem::replace(&mut self.ecran, vide));
        self.haut = 0;
        self.bas = self.lignes - 1;
    }

    fn quitter_alterne(&mut self, restaurer: bool) {
        let Some(principal) = self.principal.take() else { return };
        self.ecran = principal;
        self.haut = 0;
        self.bas = self.lignes - 1;
        if restaurer {
            if let Some(s) = self.sauve_alterne.take() {
                self.appliquer_curseur(s);
            }
        }
        self.fixer_curseur();
    }

    // ── redimensionnement ──

    fn redimensionner(&mut self, cols: usize, lignes: usize) {
        let cols = cols.clamp(1, COLONNES_MAX);
        let lignes = lignes.clamp(1, LIGNES_MAX);
        if cols == self.cols && lignes == self.lignes {
            return;
        }
        for l in self.ecran.iter_mut() {
            ajuster_largeur(l, cols);
        }
        if let Some(p) = self.principal.as_mut() {
            for l in p.iter_mut() {
                ajuster_largeur(l, cols);
            }
        }
        let anciennes_lignes = self.lignes;
        self.cols = cols;
        self.lignes = lignes;
        if self.alterne() {
            ajuster_hauteur_simple(&mut self.ecran, lignes, cols);
            if let Some(p) = self.principal.as_mut() {
                ajuster_hauteur_simple(p, lignes, cols);
            }
        } else if lignes < anciennes_lignes {
            let exces = anciennes_lignes - lignes;
            // D'abord les lignes blanches sous le curseur (rien à perdre), ensuite le haut part dans l'historique.
            let mut retirees_bas = 0;
            while retirees_bas < exces
                && self.ecran.len() > self.lig + 1
                && self.ecran.last().is_some_and(|l| ligne_blanche(l))
            {
                self.ecran.pop();
                retirees_bas += 1;
            }
            for _ in 0..exces - retirees_bas {
                if self.ecran.is_empty() {
                    break;
                }
                let l = self.ecran.remove(0);
                self.vers_historique(l);
                self.lig = self.lig.saturating_sub(1);
            }
            ajuster_hauteur_simple(&mut self.ecran, lignes, cols);
        } else {
            for _ in 0..lignes - anciennes_lignes {
                match self.historique.pop_back() {
                    Some(mut l) => {
                        ajuster_largeur(&mut l, cols);
                        self.ecran.insert(0, l);
                        self.lig += 1;
                    }
                    None => self.ecran.push(ligne_vide(cols, Couleur::Defaut)),
                }
            }
        }
        let ancien = std::mem::take(&mut self.tabs);
        self.tabs = (0..cols).map(|c| ancien.get(c).copied().unwrap_or(c % 8 == 0 && c > 0)).collect();
        self.haut = 0;
        self.bas = lignes - 1;
        self.attente = false;
        self.fixer_curseur();
    }

    // ── réponses au shell ──

    fn repondre(&mut self, octets: &[u8]) {
        if self.reponses.len() + octets.len() <= REPONSES_MAX {
            self.reponses.extend_from_slice(octets);
        }
    }

    // ── réinitialisations ──

    fn reinitialisation_souple(&mut self) {
        self.attr = Attr::default();
        self.modes = Modes::default();
        self.haut = 0;
        self.bas = self.lignes - 1;
        self.sauve = None;
        self.attente = false;
    }

    fn reinitialisation_totale(&mut self) {
        self.quitter_alterne(false);
        self.reinitialisation_souple();
        self.effacer_lignes(0, self.lignes);
        self.col = 0;
        self.lig = 0;
        self.tabs = tabs_par_defaut(self.cols);
        self.forme_curseur = 0;
    }

    // ── séquences CSI ──

    fn sgr(&mut self, params: &Params) {
        let mut it = params.iter();
        let mut vu = false;
        while let Some(g) = it.next() {
            vu = true;
            let code = g.first().copied().unwrap_or(0);
            match code {
                0 => self.attr = Attr::default(),
                1 => self.attr.regler(Attr::GRAS, true),
                2 => self.attr.regler(Attr::PALE, true),
                3 => self.attr.regler(Attr::ITALIQUE, true),
                // 4:0 coupe le soulignement, 4:1 à 4:5 en choisissent un style : un seul style ici.
                4 => self.attr.regler(Attr::SOULIGNE, g.get(1).copied() != Some(0)),
                7 => self.attr.regler(Attr::INVERSE, true),
                8 => self.attr.regler(Attr::CACHE, true),
                9 => self.attr.regler(Attr::BARRE, true),
                21 => self.attr.regler(Attr::SOULIGNE, true),
                22 => {
                    self.attr.regler(Attr::GRAS, false);
                    self.attr.regler(Attr::PALE, false);
                }
                23 => self.attr.regler(Attr::ITALIQUE, false),
                24 => self.attr.regler(Attr::SOULIGNE, false),
                27 => self.attr.regler(Attr::INVERSE, false),
                28 => self.attr.regler(Attr::CACHE, false),
                29 => self.attr.regler(Attr::BARRE, false),
                30..=37 => self.attr.fg = Couleur::Indexee((code - 30) as u8),
                39 => self.attr.fg = Couleur::Defaut,
                40..=47 => self.attr.bg = Couleur::Indexee((code - 40) as u8),
                49 => self.attr.bg = Couleur::Defaut,
                90..=97 => self.attr.fg = Couleur::Indexee((code - 90 + 8) as u8),
                100..=107 => self.attr.bg = Couleur::Indexee((code - 100 + 8) as u8),
                38 | 48 => {
                    let couleur = if g.len() > 1 {
                        couleur_sous_parametres(g)
                    } else {
                        // Forme `38;5;n` ou `38;2;r;g;b` : les valeurs sont les paramètres suivants.
                        let genre = it.next().and_then(|x| x.first().copied());
                        match genre {
                            Some(5) => it.next().and_then(|x| x.first().copied()).and_then(couleur_indexee),
                            Some(2) => {
                                let r = it.next().and_then(|x| x.first().copied());
                                let v = it.next().and_then(|x| x.first().copied());
                                let b = it.next().and_then(|x| x.first().copied());
                                match (r, v, b) {
                                    (Some(r), Some(v), Some(b)) => Some(Couleur::Rvb(r.min(255) as u8, v.min(255) as u8, b.min(255) as u8)),
                                    _ => None,
                                }
                            }
                            _ => None,
                        }
                    };
                    if let Some(c) = couleur {
                        if code == 38 {
                            self.attr.fg = c;
                        } else {
                            self.attr.bg = c;
                        }
                    }
                }
                _ => {}
            }
        }
        if !vu {
            // `ESC[m` sans paramètre vaut `ESC[0m`.
            self.attr = Attr::default();
        }
    }

    fn mode_prive(&mut self, n: u16, actif: bool) {
        match n {
            1 => self.modes.touches_curseur_appli = actif,
            6 => {
                self.modes.origine = actif;
                self.aller(0, 0);
            }
            7 => self.modes.retour_auto = actif,
            25 => self.modes.curseur_visible = actif,
            47 | 1047 => {
                if actif {
                    self.entrer_alterne(false);
                } else {
                    self.quitter_alterne(false);
                }
            }
            1048 => {
                if actif {
                    self.sauve_alterne = Some(self.sauver_curseur());
                } else if let Some(s) = self.sauve_alterne {
                    self.appliquer_curseur(s);
                }
            }
            1049 => {
                if actif {
                    self.entrer_alterne(true);
                } else {
                    self.quitter_alterne(true);
                }
            }
            // Comme dans xterm, un seul niveau de suivi à la fois : en demander un remplace les deux autres
            1000 | 1002 | 1003 => {
                if actif {
                    self.modes.souris_appui = n == 1000;
                    self.modes.souris_glisse = n == 1002;
                    self.modes.souris_mouvement = n == 1003;
                } else {
                    match n {
                        1000 => self.modes.souris_appui = false,
                        1002 => self.modes.souris_glisse = false,
                        _ => self.modes.souris_mouvement = false,
                    }
                }
            }
            1006 => self.modes.souris_sgr = actif,
            1004 => self.modes.focus = actif,
            2004 => self.modes.collage_encadre = actif,
            // 9001 (entrée Win32 de ConPTY), 12 (clignotement), 3, 5… : sans effet sur la grille, ignorés.
            _ => {}
        }
    }

    fn csi_standard(&mut self, ps: &Ps, params: &Params, action: char) {
        let n = ps.nz(0, 1);
        match action {
            '@' => self.inserer_cellules(n),
            'A' => self.haut_curseur(n),
            'B' | 'e' => self.bas_curseur(n),
            'C' | 'a' => {
                self.col = (self.col + n).min(self.cols - 1);
                self.attente = false;
            }
            'D' => {
                self.col = self.col.saturating_sub(n);
                self.attente = false;
            }
            'E' => {
                self.bas_curseur(n);
                self.col = 0;
            }
            'F' => {
                self.haut_curseur(n);
                self.col = 0;
            }
            'G' | '`' => {
                self.col = (n - 1).min(self.cols - 1);
                self.attente = false;
            }
            'H' | 'f' => self.aller(ps.nz(0, 1) - 1, ps.nz(1, 1) - 1),
            'd' => {
                let c = self.col;
                self.aller(n - 1, c);
            }
            'I' => self.tab_suivante(n),
            'Z' => self.tab_precedente(n),
            'J' => {
                let (l, k) = (self.lig, self.col);
                match ps.nz(0, 0) {
                    0 => {
                        self.effacer_plage(l, k, self.cols);
                        self.effacer_lignes(l + 1, self.lignes);
                    }
                    1 => {
                        self.effacer_lignes(0, l);
                        self.effacer_plage(l, 0, k + 1);
                    }
                    2 => self.effacer_lignes(0, self.lignes),
                    3 => {
                        self.effacer_lignes(0, self.lignes);
                        self.lignes_ecoulees += self.historique.len() as u64;
                        self.historique.clear();
                    }
                    _ => {}
                }
                self.attente = false;
            }
            'K' => {
                let (l, k) = (self.lig, self.col);
                match ps.nz(0, 0) {
                    0 => self.effacer_plage(l, k, self.cols),
                    1 => self.effacer_plage(l, 0, k + 1),
                    2 => self.effacer_plage(l, 0, self.cols),
                    _ => {}
                }
            }
            'L' => {
                if self.lig >= self.haut && self.lig <= self.bas && self.bas < self.ecran.len() {
                    for _ in 0..n.min(self.bas - self.lig + 1) {
                        self.ecran.remove(self.bas);
                        let vide = self.vide_ligne();
                        self.ecran.insert(self.lig, vide);
                    }
                    self.col = 0;
                    self.attente = false;
                }
            }
            'M' => {
                if self.lig >= self.haut && self.lig <= self.bas && self.bas < self.ecran.len() {
                    for _ in 0..n.min(self.bas - self.lig + 1) {
                        self.ecran.remove(self.lig);
                        let vide = self.vide_ligne();
                        self.ecran.insert(self.bas, vide);
                    }
                    self.col = 0;
                    self.attente = false;
                }
            }
            'P' => {
                let (l, k, cols, bg) = (self.lig, self.col, self.cols, self.attr.bg);
                if let Some(ligne) = self.ecran.get_mut(l) {
                    let n = n.min(cols - k);
                    if k + n <= ligne.len() {
                        ligne.drain(k..k + n);
                    }
                    ligne.resize(cols, Cellule::vide(bg));
                }
                self.nettoyer(l);
            }
            'X' => {
                let (l, k) = (self.lig, self.col);
                self.effacer_plage(l, k, k + n);
            }
            'S' => self.defiler_haut(n),
            'T' => self.defiler_bas(n),
            'm' => self.sgr(params),
            'r' => {
                let haut = ps.nz(0, 1);
                let bas = ps.nz(1, self.lignes);
                if haut < bas && bas <= self.lignes {
                    self.haut = haut - 1;
                    self.bas = bas - 1;
                    self.aller(0, 0);
                }
            }
            's' => self.sauve = Some(self.sauver_curseur()),
            'u' => {
                if let Some(s) = self.sauve {
                    self.appliquer_curseur(s);
                }
            }
            'n' => match ps.brut(0) {
                Some(5) => self.repondre(b"\x1b[0n"),
                Some(6) => {
                    let l = if self.modes.origine { self.lig.saturating_sub(self.haut) } else { self.lig };
                    let r = format!("\x1b[{};{}R", l + 1, self.col + 1);
                    self.repondre(r.as_bytes());
                }
                _ => {}
            },
            'c' => {
                if ps.nz(0, 0) == 0 {
                    // Se présente comme un VT100 avec option vidéo avancée : le plus petit commun dénominateur.
                    self.repondre(b"\x1b[?1;2c");
                }
            }
            'g' => match ps.brut(0).unwrap_or(0) {
                0 => {
                    let k = self.col;
                    if let Some(t) = self.tabs.get_mut(k) {
                        *t = false;
                    }
                }
                3 => self.tabs.iter_mut().for_each(|t| *t = false),
                _ => {}
            },
            'h' | 'l' => {
                let actif = action == 'h';
                for i in 0..ps.n {
                    if ps.brut(i) == Some(4) {
                        self.modes.insertion = actif;
                    }
                }
            }
            _ => {}
        }
    }
}

/// `38:5:n` / `48:5:n` et `38:2:[espace:]r:g:b` (sous-paramètres séparés par `:`).
fn couleur_sous_parametres(g: &[u16]) -> Option<Couleur> {
    match g.get(1).copied() {
        Some(5) => g.get(2).copied().and_then(couleur_indexee),
        Some(2) => {
            // Avec identifiant d'espace de couleur (`38:2::r:g:b` ou `38:2:1:r:g:b`) il y a un champ de plus.
            let debut = if g.len() >= 6 { 3 } else { 2 };
            let r = g.get(debut).copied()?;
            let v = g.get(debut + 1).copied()?;
            let b = g.get(debut + 2).copied()?;
            Some(Couleur::Rvb(r.min(255) as u8, v.min(255) as u8, b.min(255) as u8))
        }
        _ => None,
    }
}

fn couleur_indexee(n: u16) -> Option<Couleur> {
    u8::try_from(n).ok().map(Couleur::Indexee)
}

/// Paramètres numériques d'une séquence CSI, 32 au plus (limite de `vte`), sans allocation.
struct Ps {
    v: [u16; 32],
    n: usize,
}

impl Ps {
    fn de(params: &Params) -> Ps {
        let mut v = [0u16; 32];
        let mut n = 0;
        for g in params.iter() {
            if let Some(case) = v.get_mut(n) {
                *case = g.first().copied().unwrap_or(0);
                n += 1;
            }
        }
        Ps { v, n }
    }

    fn brut(&self, i: usize) -> Option<u16> {
        if i < self.n {
            self.v.get(i).copied()
        } else {
            None
        }
    }

    /// Paramètre `i`, ou `defaut` s'il manque ou vaut 0 (convention des séquences ECMA-48).
    fn nz(&self, i: usize, defaut: usize) -> usize {
        match self.brut(i) {
            Some(x) if x > 0 => x as usize,
            _ => defaut,
        }
    }
}

/// Recouvrir la case `i` d'un caractère large coupe ce caractère : l'autre moitié devient un blanc.
fn liberer(ligne: &mut [Cellule], i: usize) {
    match ligne.get(i).map(|x| x.largeur) {
        Some(0) => {
            if let Some(p) = i.checked_sub(1).and_then(|p| ligne.get_mut(p)) {
                *p = Cellule::VIDE;
            }
        }
        Some(2) => {
            if let Some(s) = ligne.get_mut(i + 1) {
                *s = Cellule::VIDE;
            }
        }
        _ => {}
    }
}

impl Perform for Etat {
    fn print(&mut self, c: char) {
        self.ecrire_char(c);
    }

    fn execute(&mut self, octet: u8) {
        match octet {
            0x07 => self.cloches = self.cloches.saturating_add(1),
            0x08 => {
                self.col = self.col.saturating_sub(1);
                self.attente = false;
            }
            0x09 => self.tab_suivante(1),
            0x0A..=0x0C => {
                self.attente = false;
                self.index();
            }
            0x0D => {
                self.col = 0;
                self.attente = false;
            }
            _ => {}
        }
        self.fixer_curseur();
    }

    fn csi_dispatch(&mut self, params: &Params, intermediaires: &[u8], ignore: bool, action: char) {
        if ignore {
            return;
        }
        let ps = Ps::de(params);
        match (intermediaires, action) {
            ([], _) => self.csi_standard(&ps, params, action),
            ([b'?'], 'h') | ([b'?'], 'l') => {
                for i in 0..ps.n {
                    if let Some(m) = ps.brut(i) {
                        self.mode_prive(m, action == 'h');
                    }
                }
            }
            // Effacements « sélectifs » : on n'a pas d'attribut de protection, ils valent les effacements normaux.
            ([b'?'], 'J') | ([b'?'], 'K') => self.csi_standard(&ps, params, action),
            ([b' '], 'q') => self.forme_curseur = ps.brut(0).unwrap_or(0).min(6) as u8,
            ([b'!'], 'p') => self.reinitialisation_souple(),
            _ => {}
        }
        self.fixer_curseur();
    }

    fn esc_dispatch(&mut self, intermediaires: &[u8], ignore: bool, octet: u8) {
        if ignore {
            return;
        }
        match (intermediaires, octet) {
            ([], b'7') => self.sauve = Some(self.sauver_curseur()),
            ([], b'8') => {
                if let Some(s) = self.sauve {
                    self.appliquer_curseur(s);
                }
            }
            ([], b'D') => {
                self.attente = false;
                self.index();
            }
            ([], b'E') => {
                self.col = 0;
                self.attente = false;
                self.index();
            }
            ([], b'M') => {
                self.attente = false;
                self.index_inverse();
            }
            ([], b'H') => {
                let k = self.col;
                if let Some(t) = self.tabs.get_mut(k) {
                    *t = true;
                }
            }
            ([], b'c') => self.reinitialisation_totale(),
            _ => {}
        }
        self.fixer_curseur();
    }

    fn osc_dispatch(&mut self, params: &[&[u8]], _termine_par_bel: bool) {
        let Some(code) = params.first() else { return };
        let reste = params.get(1..).unwrap_or(&[]);
        match *code {
            b"0" | b"2" => self.titre = joindre(reste, TITRE_MAX),
            // ConEmu / Windows Terminal : le shell annonce son dossier courant (cmd et PowerShell le font via ConPTY).
            b"9" => {
                if reste.first().is_some_and(|p| *p == b"9") {
                    let chemin = joindre(reste.get(1..).unwrap_or(&[]), DOSSIER_MAX);
                    let chemin = chemin.trim_matches('"');
                    if !chemin.is_empty() {
                        self.dossier = Some(chemin.to_string());
                        self.annonces_dossier += 1;
                    }
                }
            }
            // Intégration du shell (FinalTerm / VS Code / Windows Terminal) : repères d'invite et de commande.
            b"133" => {
                let genre = reste.first().and_then(|p| p.first()).copied().map(char::from);
                if let Some(genre @ ('A' | 'B' | 'C' | 'D')) = genre {
                    if !self.alterne() {
                        let code = reste
                            .get(1)
                            .and_then(|p| std::str::from_utf8(p).ok())
                            .and_then(|s| s.parse::<i32>().ok());
                        let ligne = self.ligne_absolue();
                        self.marques.push(Marque { genre, ligne, code: if genre == 'D' { code } else { None } });
                        if self.marques.len() > MARQUES_MAX {
                            let trop = self.marques.len() - MARQUES_MAX;
                            self.marques.drain(..trop);
                        }
                    }
                }
            }
            _ => {}
        }
    }
}

fn joindre(params: &[&[u8]], max: usize) -> String {
    let mut octets = Vec::new();
    for (i, p) in params.iter().enumerate() {
        if i > 0 {
            octets.push(b';');
        }
        octets.extend_from_slice(p);
    }
    String::from_utf8_lossy(&octets).chars().filter(|c| !c.is_control()).take(max).collect()
}

// ───────────────────────── largeur des caractères ─────────────────────────

/// Nombre de colonnes d'un caractère : 0 (combinant, sans largeur), 1 ou 2 (CJK, emoji). Table maison, tirée de
/// l'annexe UAX #11 (largeur « Wide » et « Fullwidth ») et des emoji à présentation large : pas de crate pour
/// quelques plages (le budget de taille de l'exe est mesuré).
pub fn largeur_char(c: char) -> usize {
    let u = c as u32;
    if u < 0x20 || (0x7F..0xA0).contains(&u) {
        return 0;
    }
    if u < 0x300 {
        return 1;
    }
    if ZERO.iter().any(|&(a, b)| u >= a && u <= b) {
        return 0;
    }
    if LARGES.iter().any(|&(a, b)| u >= a && u <= b) {
        return 2;
    }
    1
}

/// Combinants, joiner et sélecteurs : rattachés au caractère précédent, donc sans colonne propre.
const ZERO: &[(u32, u32)] = &[
    (0x0300, 0x036F),
    (0x0483, 0x0489),
    (0x0591, 0x05BD),
    (0x05BF, 0x05BF),
    (0x05C1, 0x05C2),
    (0x05C4, 0x05C5),
    (0x05C7, 0x05C7),
    (0x0610, 0x061A),
    (0x064B, 0x065F),
    (0x0670, 0x0670),
    (0x06D6, 0x06DC),
    (0x06DF, 0x06E4),
    (0x06E7, 0x06E8),
    (0x06EA, 0x06ED),
    (0x0E31, 0x0E31),
    (0x0E34, 0x0E3A),
    (0x0E47, 0x0E4E),
    (0x1AB0, 0x1AFF),
    (0x1DC0, 0x1DFF),
    (0x200B, 0x200F),
    (0x202A, 0x202E),
    (0x2060, 0x2064),
    (0x20D0, 0x20FF),
    (0x3099, 0x309A),
    (0xFE00, 0xFE0F),
    (0xFE20, 0xFE2F),
    (0xFEFF, 0xFEFF),
    (0x1F3FB, 0x1F3FF),
    (0xE0100, 0xE01EF),
];

/// Caractères sur deux colonnes (hangul, CJK, formes pleine largeur, emoji à présentation large).
const LARGES: &[(u32, u32)] = &[
    (0x1100, 0x115F),
    (0x231A, 0x231B),
    (0x2329, 0x232A),
    (0x23E9, 0x23EC),
    (0x23F0, 0x23F0),
    (0x23F3, 0x23F3),
    (0x25FD, 0x25FE),
    (0x2614, 0x2615),
    (0x2648, 0x2653),
    (0x267F, 0x267F),
    (0x2693, 0x2693),
    (0x26A1, 0x26A1),
    (0x26AA, 0x26AB),
    (0x26BD, 0x26BE),
    (0x26C4, 0x26C5),
    (0x26CE, 0x26CE),
    (0x26D4, 0x26D4),
    (0x26EA, 0x26EA),
    (0x26F2, 0x26F3),
    (0x26F5, 0x26F5),
    (0x26FA, 0x26FA),
    (0x26FD, 0x26FD),
    (0x2705, 0x2705),
    (0x270A, 0x270B),
    (0x2728, 0x2728),
    (0x274C, 0x274C),
    (0x274E, 0x274E),
    (0x2753, 0x2755),
    (0x2757, 0x2757),
    (0x2795, 0x2797),
    (0x27B0, 0x27B0),
    (0x27BF, 0x27BF),
    (0x2B1B, 0x2B1C),
    (0x2B50, 0x2B50),
    (0x2B55, 0x2B55),
    (0x2E80, 0x303E),
    (0x3041, 0x33FF),
    (0x3400, 0x4DBF),
    (0x4E00, 0x9FFF),
    (0xA000, 0xA4CF),
    (0xA960, 0xA97F),
    (0xAC00, 0xD7A3),
    (0xF900, 0xFAFF),
    (0xFE10, 0xFE19),
    (0xFE30, 0xFE6F),
    (0xFF01, 0xFF60),
    (0xFFE0, 0xFFE6),
    (0x1F004, 0x1F004),
    (0x1F0CF, 0x1F0CF),
    (0x1F18E, 0x1F18E),
    (0x1F191, 0x1F19A),
    (0x1F200, 0x1F2FF),
    (0x1F300, 0x1F64F),
    (0x1F680, 0x1F6FF),
    (0x1F900, 0x1F9FF),
    (0x1FA70, 0x1FAFF),
    (0x20000, 0x3FFFD),
];

#[cfg(test)]
mod tests;
