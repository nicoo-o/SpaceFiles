//! Panneau de terminal (PLAN-TERMINAL, lot T2b) : l'état d'un terminal affiché dans la fenêtre — une session ConPTY
//! (`session`), l'émulateur qui en lit la sortie (`vt`), la position de défilement, la sélection et la traduction du
//! clavier en octets pour le shell. Aucun dessin ni message de fenêtre ici : tout ce qui se décide sans écran se teste
//! sans écran (la fenêtre n'a plus qu'à appeler ces fonctions).
//!
//! Cycle de vie : le panneau est créé à l'ouverture et détruit à la fermeture (aucune session tant qu'il est fermé) ;
//! sa destruction ferme la pseudo-console et tue le shell et ses descendants (voir `session`).

use std::sync::Arc;

use super::session::{Session, Shell};
use super::souris::{self, Evenement};
use super::vt::{Cellule, Modes, Terminal, HISTORIQUE_DEFAUT};

// T5 : la ligne de saisie tenue par SpaceFiles tant que le shell attend une commande (suggestion, rappel)
pub mod ligne;

/// Dimensions plancher de la grille : en dessous, un shell n'affiche plus rien d'utile
pub const COLONNES_MIN: usize = 10;
pub const LIGNES_MIN: usize = 3;

/// Une position dans la grille, indépendante du défilement : `ligne` est la ligne absolue (écoulées + indice)
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Pos {
    pub ligne: u64,
    pub col: usize,
}

/// Modificateurs tenus pendant une touche
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Mods {
    pub ctrl: bool,
    pub alt: bool,
    pub maj: bool,
}

/// Défilement demandé par le clavier
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Defil {
    PageHaut,
    PageBas,
    Debut,
    Fin,
}

pub struct Panneau {
    session: Option<Session>,
    vt: Terminal,
    /// Lignes remontées dans l'historique (0 = on voit le bas, là où le shell écrit)
    decalage: usize,
    ancre: Option<Pos>,
    fin_selection: Option<Pos>,
    /// Pourquoi aucun shell n'a pu démarrer (affiché dans le panneau)
    erreur: Option<String>,
    /// Code de sortie du shell quand il s'est terminé
    fini: Option<u32>,
    /// Première moitié d'une paire de substituts UTF-16 reçue par WM_CHAR
    substitut: Option<u16>,
    dossier: String,
    /// Dossier où l'on sait que le shell se trouve : celui du lancement, puis le dernier annoncé par lui (OSC 9;9) ou
    /// demandé par `aller_dans`. Sert à ne réagir qu'aux vrais changements de dossier.
    dossier_connu: String,
    /// Annonces de dossier déjà comptées (`Terminal::annonces_dossier`) : une de plus = une invite vient de s'afficher
    annonces_vues: u64,
    /// Le shell attend une commande (invite affichée, rien envoyé depuis) : seul moment où l'on peut lui taper un `cd`
    invite_prete: bool,
    /// L'utilisateur a tapé quelque chose depuis la dernière invite : une ligne est peut-être commencée, on n'y touche pas
    frappe: bool,
    /// Des octets sont partis au shell depuis la dernière Entrée ou le dernier Ctrl+C (frappe d'avance pendant une commande,
    /// ligne déléguée) : à la prochaine invite, le shell tient déjà du texte que SpaceFiles ne connaît pas, la ligne de
    /// SpaceFiles ne doit pas prendre la main
    en_attente: bool,
    shell: Shell,
    /// Dernière case envoyée au programme pour un mouvement : un mouvement n'est renvoyé que s'il change de case
    derniere_case_souris: Option<(usize, usize)>,
    /// La ligne en cours de saisie, tenue ici tant que `saisie_prete()` ; envoyée au shell à Entrée (ou déléguée à lui)
    ligne: ligne::EtatLigne,
}

/// Deux chemins désignent le même dossier (casse et barre finale ignorées : Windows ne les distingue pas)
pub(crate) fn meme_dossier(a: &str, b: &str) -> bool {
    a.trim_end_matches('\\').eq_ignore_ascii_case(b.trim_end_matches('\\'))
}

/// La commande qui change le dossier du shell, ou `None` si le chemin ne peut pas s'y glisser sans risque (caractère de
/// contrôle : une fin de ligne y ferait lancer autre chose). PowerShell : chemin littéral entre apostrophes, que l'on double
/// (les apostrophes typographiques comptent aussi comme apostrophes pour PowerShell).
fn commande_cd(shell: Shell, dossier: &str) -> Option<Vec<u8>> {
    if dossier.is_empty() || dossier.chars().any(char::is_control) {
        return None;
    }
    match shell {
        Shell::PowerShell => {
            let mut c = String::from("cd -LiteralPath '");
            for ch in dossier.chars() {
                if matches!(ch, '\'' | '\u{2018}' | '\u{2019}' | '\u{201A}' | '\u{201B}') {
                    c.push(ch);
                }
                c.push(ch);
            }
            c.push_str("'\r");
            Some(c.into_bytes())
        }
        // cmd n'annonce pas son dossier : jamais prêt, jamais appelé
        Shell::Cmd => None,
    }
}

impl Panneau {
    /// Ouvre un terminal dans `dossier`. Un shell qui ne démarre pas n'est pas une panique : le panneau s'ouvre quand
    /// même et montre l'erreur (`erreur()`). `notifier` doit seulement poster un message à la fenêtre.
    pub fn ouvrir(shell: Shell, dossier: &str, colonnes: usize, lignes: usize, notifier: Arc<dyn Fn() + Send + Sync>) -> Panneau {
        let colonnes = colonnes.max(COLONNES_MIN);
        let lignes = lignes.max(LIGNES_MIN);
        let mut p = Panneau {
            session: None,
            vt: Terminal::avec_historique(colonnes, lignes, HISTORIQUE_DEFAUT),
            decalage: 0,
            ancre: None,
            fin_selection: None,
            erreur: None,
            fini: None,
            substitut: None,
            dossier: dossier.to_string(),
            dossier_connu: dossier.to_string(),
            annonces_vues: 0,
            invite_prete: false,
            frappe: false,
            en_attente: false,
            shell,
            derniere_case_souris: None,
            ligne: ligne::EtatLigne::default(),
        };
        p.demarrer(colonnes, lignes, notifier);
        p
    }

    fn demarrer(&mut self, colonnes: usize, lignes: usize, notifier: Arc<dyn Fn() + Send + Sync>) {
        let dossier = if self.dossier.is_empty() { None } else { Some(self.dossier.as_str()) };
        match Session::lancer(&self.shell.ligne_de_commande(), dossier, colonnes as u16, lignes as u16, notifier) {
            Ok(s) => {
                self.session = Some(s);
                self.erreur = None;
                self.fini = None;
            }
            Err(e) => {
                self.session = None;
                self.erreur = Some(e);
            }
        }
    }

    /// Relance un shell dans le même dossier après la fin du précédent (la grille repart d'un écran propre)
    pub fn relancer(&mut self, notifier: Arc<dyn Fn() + Send + Sync>) {
        let (c, l) = (self.vt.colonnes(), self.vt.lignes());
        self.session = None; // ferme l'ancienne session avant d'en ouvrir une autre
        self.vt = Terminal::avec_historique(c, l, HISTORIQUE_DEFAUT);
        self.decalage = 0;
        self.effacer_selection();
        self.substitut = None;
        self.derniere_case_souris = None;
        self.dossier_connu = self.dossier.clone();
        self.annonces_vues = 0;
        self.invite_prete = false;
        self.frappe = false;
        self.en_attente = false;
        self.ligne = ligne::EtatLigne::default();
        self.demarrer(c, l, notifier);
    }

    pub fn dossier(&self) -> &str {
        &self.dossier
    }

    /// Dossier où le shell se trouve d'après ce que l'on sait (lancement, dernière annonce ou dernier `cd` envoyé)
    pub fn dossier_connu(&self) -> &str {
        &self.dossier_connu
    }

    pub fn erreur(&self) -> Option<&str> {
        self.erreur.as_deref()
    }

    /// Code de sortie du shell s'il est terminé
    pub fn fini(&self) -> Option<u32> {
        self.fini
    }

    /// Le shell tourne et peut recevoir des touches
    pub fn actif(&self) -> bool {
        self.session.is_some() && self.fini.is_none()
    }

    pub fn terminal(&self) -> &Terminal {
        &self.vt
    }

    /// Lignes d'historique au-dessus de la vue (0 = en bas) ; lu par les tests, le rendu passe par `ligne_visible`
    #[allow(dead_code)]
    pub fn decalage(&self) -> usize {
        self.decalage
    }

    /// Prend ce que le shell a écrit, le passe à l'émulateur, renvoie au shell les réponses que celui-ci attend
    /// (position du curseur, etc.) et note la fin du shell. Rend `true` si l'affichage a changé.
    pub fn pomper(&mut self) -> bool {
        let Some(session) = self.session.as_ref() else { return false };
        let octets = session.prendre_sortie();
        let mut change = false;
        if !octets.is_empty() {
            let avant = self.vt.lignes_ecoulees() + self.vt.historique_len() as u64;
            self.vt.ecrire(&octets);
            let apres = self.vt.lignes_ecoulees() + self.vt.historique_len() as u64;
            // Une vue remontée dans l'historique reste sur les mêmes lignes quand il en arrive de nouvelles
            if self.decalage > 0 {
                let nouvelles = apres.saturating_sub(avant) as usize;
                self.decalage = (self.decalage + nouvelles).min(self.vt.historique_len());
            }
            if self.vt.ecran_alterne() {
                self.decalage = 0;
            }
            let reponses = self.vt.prendre_reponses();
            if !reponses.is_empty() {
                session.ecrire(&reponses);
            }
            self.vt.prendre_cloches();
            // Une annonce de dossier de plus : le shell vient d'afficher son invite, il attend une commande
            let annonces = self.vt.annonces_dossier();
            if annonces != self.annonces_vues {
                self.annonces_vues = annonces;
                // Du texte déjà envoyé à l'avance (PSReadLine l'affiche à cette invite) : la ligne reste au shell
                self.invite_prete = !self.en_attente;
                self.frappe = self.en_attente;
            }
            change = true;
        }
        if self.fini.is_none() {
            if let Some(code) = session.code_de_sortie() {
                self.fini = Some(code);
                change = true;
            }
        }
        change
    }

    /// Envoie des octets au shell et revient en bas (taper fait toujours revoir ce qu'on tape)
    pub fn envoyer(&mut self, octets: &[u8]) {
        if octets.is_empty() || !self.actif() {
            return;
        }
        if let Some(s) = self.session.as_ref() {
            s.ecrire(octets);
        }
        self.decalage = 0;
        // Entrée : une commande part, l'invite reviendra avec une nouvelle annonce ; sinon une ligne se compose
        if octets.contains(&b'\r') {
            self.invite_prete = false;
        }
        self.frappe = true;
        // Ce qui suit la dernière Entrée ou le dernier Ctrl+C attend encore dans la ligne du shell
        self.en_attente = match octets.iter().rposition(|b| *b == b'\r' || *b == 3) {
            Some(i) => i + 1 < octets.len(),
            None => true,
        };
    }

    /// Dossier que le shell vient d'annoncer (OSC 9;9) et qui n'est pas celui qu'on lui connaissait : un `cd` tapé dans le
    /// terminal. `None` tant qu'il reste là où l'on pense qu'il est. Le dossier annoncé devient le dossier connu.
    pub fn dossier_change_par_le_shell(&mut self) -> Option<String> {
        let annonce = self.vt.dossier()?;
        if meme_dossier(annonce, &self.dossier_connu) {
            return None;
        }
        let annonce = annonce.to_string();
        self.dossier_connu = annonce.clone();
        Some(annonce)
    }

    /// Un `cd` peut être tapé au shell sans écraser le travail de l'utilisateur : une invite vide est affichée (annoncée par
    /// le shell, rien tapé depuis) et aucun programme ne tient l'écran. Faux pour un shell qui n'annonce pas son dossier.
    pub fn peut_changer_de_dossier(&self) -> bool {
        self.actif() && self.invite_prete && !self.frappe && !self.vt.ecran_alterne() && self.ligne.vide()
    }

    /// Tape un `cd` vers `dossier` au shell (liste → terminal). `false`, sans rien envoyer, si ce n'est pas le moment
    /// (`peut_changer_de_dossier`), si le dossier est déjà le bon ou s'il contient un caractère de contrôle.
    pub fn aller_dans(&mut self, dossier: &str) -> bool {
        if !self.peut_changer_de_dossier() || meme_dossier(dossier, &self.dossier_connu) {
            return false;
        }
        let Some(octets) = commande_cd(self.shell, dossier) else { return false };
        if let Some(s) = self.session.as_ref() {
            s.ecrire(&octets);
        }
        self.decalage = 0;
        self.dossier_connu = dossier.to_string();
        self.invite_prete = false;
        true
    }

    /// Une unité UTF-16 reçue par WM_CHAR → octets UTF-8 (rien tant qu'une paire de substituts est incomplète)
    pub fn octets_de_unite(&mut self, u: u16) -> Vec<u8> {
        let c = match (self.substitut.take(), u) {
            (_, 0xD800..=0xDBFF) => {
                self.substitut = Some(u);
                return Vec::new();
            }
            (Some(haut), 0xDC00..=0xDFFF) => {
                let v = 0x10000 + (((haut as u32) - 0xD800) << 10) + ((u as u32) - 0xDC00);
                char::from_u32(v)
            }
            (_, 0xDC00..=0xDFFF) => None, // moitié basse orpheline
            (_, _) => char::from_u32(u as u32),
        };
        match c {
            Some(c) => {
                let mut tampon = [0u8; 4];
                c.encode_utf8(&mut tampon).as_bytes().to_vec()
            }
            None => Vec::new(),
        }
    }

    /// Texte collé → octets pour le shell : fins de ligne en `\r`, caractères de contrôle retirés (un ESC collé ne doit
    /// pas pouvoir fermer le collage encadré ni lancer une séquence), encadré par `ESC[200~` / `ESC[201~` si le shell l'a demandé.
    pub fn octets_de_collage(&self, texte: &str) -> Vec<u8> {
        let mut nettoye = String::with_capacity(texte.len());
        let mut precedent_cr = false;
        for c in texte.chars() {
            match c {
                '\r' => {
                    nettoye.push('\r');
                    precedent_cr = true;
                    continue;
                }
                '\n' => {
                    if !precedent_cr {
                        nettoye.push('\r');
                    }
                }
                '\t' => nettoye.push('\t'),
                c if c.is_control() => {}
                c => nettoye.push(c),
            }
            precedent_cr = false;
        }
        let mut sortie = Vec::with_capacity(nettoye.len() + 12);
        let encadre = self.vt.modes().collage_encadre;
        if encadre {
            sortie.extend_from_slice(b"\x1b[200~");
        }
        sortie.extend_from_slice(nettoye.as_bytes());
        if encadre {
            sortie.extend_from_slice(b"\x1b[201~");
        }
        sortie
    }

    /// Change la taille de la grille (et de la pseudo-console). Sans effet si elle ne change pas.
    pub fn redimensionner(&mut self, colonnes: usize, lignes: usize) {
        let colonnes = colonnes.clamp(COLONNES_MIN, 1_000);
        let lignes = lignes.clamp(LIGNES_MIN, 500);
        if colonnes == self.vt.colonnes() && lignes == self.vt.lignes() {
            return;
        }
        if let Some(s) = self.session.as_mut() {
            s.redimensionner(colonnes as u16, lignes as u16);
        }
        self.vt.redimensionner(colonnes, lignes);
        self.decalage = self.decalage.min(self.vt.historique_len());
        self.effacer_selection();
    }

    // ---------------------------------------------------------------- souris vers le programme (T3)

    /// Le programme a demandé la souris (vim `set mouse=a`, htop, mc…) : clic et glissement lui reviennent, la sélection
    /// de texte passe à Maj
    pub fn souris_capturee(&self) -> bool {
        self.actif() && souris::suivi_demande(&self.vt.modes())
    }

    /// Envoie un appui, un relâchement ou un mouvement au programme. `col` et `rangee` partent de 0 dans la grille visible.
    /// Un mouvement qui reste dans la même case n'est pas renvoyé. Rend `true` si quelque chose est parti.
    pub fn envoyer_souris(&mut self, e: Evenement, col: usize, rangee: usize, ctrl: bool, alt: bool) -> bool {
        if !self.actif() {
            return false;
        }
        let col = col.min(self.vt.colonnes().saturating_sub(1));
        let rangee = rangee.min(self.vt.lignes().saturating_sub(1));
        // Vue remontée dans l'historique : les rangées visibles ne sont pas celles de l'écran du programme. Un appui
        // ramène la vue en bas (comme une frappe) ; les autres événements attendent
        if self.decalage > 0 {
            if matches!(e, Evenement::Appui(_)) {
                self.decalage = 0;
                self.effacer_selection();
            } else {
                return false;
            }
        }
        if matches!(e, Evenement::Mouvement(_)) {
            if self.derniere_case_souris == Some((col, rangee)) {
                return false;
            }
        }
        let Some(octets) = souris::encoder(&self.vt.modes(), e, col, rangee, ctrl, alt) else { return false };
        // Même case mémorisée pour un appui : le glissement qui suit ne renvoie pas la case de l'appui
        self.derniere_case_souris = Some((col, rangee));
        if let Some(s) = self.session.as_ref() {
            s.ecrire(&octets);
        }
        true
    }

    /// Oublie la case mémorisée (fin de glissement, souris sortie du panneau)
    pub fn oublier_case_souris(&mut self) {
        self.derniere_case_souris = None;
    }

    /// La molette sur un programme plein écran. `crans` > 0 = vers le haut. Si le programme suit la souris, il reçoit les
    /// crans comme boutons 64/65 ; sinon, sur l'écran alterné (less, more, man), des flèches ; sur l'écran principal rien
    /// n'est envoyé et le panneau défile dans son historique. Rend `true` si la molette a été consommée.
    pub fn molette_vers_programme(&mut self, crans: isize, col: usize, rangee: usize, ctrl: bool, alt: bool) -> bool {
        if !self.actif() {
            return false;
        }
        // Une fraction de cran (pavé tactile) n'envoie rien encore, mais appartient quand même au programme : la molette ne
        // doit pas, en attendant le cran entier, faire défiler l'historique dessous
        if crans == 0 {
            return souris::suivi_demande(&self.vt.modes()) || self.vt.ecran_alterne();
        }
        if souris::suivi_demande(&self.vt.modes()) {
            let col = col.min(self.vt.colonnes().saturating_sub(1));
            let rangee = rangee.min(self.vt.lignes().saturating_sub(1));
            let mut tout = Vec::new();
            for _ in 0..crans.unsigned_abs().min(20) {
                if let Some(o) = souris::encoder(&self.vt.modes(), Evenement::Molette(crans > 0), col, rangee, ctrl, alt) {
                    tout.extend_from_slice(&o);
                }
            }
            if let Some(s) = self.session.as_ref() {
                s.ecrire(&tout);
            }
            return true;
        }
        if self.vt.ecran_alterne() {
            let fleches = souris::fleches_de_molette(&self.vt.modes(), crans);
            if let Some(s) = self.session.as_ref() {
                s.ecrire(&fleches);
            }
            return true;
        }
        false
    }

    // ---------------------------------------------------------------- défilement

    /// Remonte (`delta` > 0) ou redescend (`delta` < 0) de `delta` lignes ; rend `true` si la vue a bougé
    pub fn defiler(&mut self, delta: isize) -> bool {
        if self.vt.ecran_alterne() {
            return false;
        }
        let max = self.vt.historique_len();
        let nouveau = (self.decalage as isize + delta).clamp(0, max as isize) as usize;
        let change = nouveau != self.decalage;
        self.decalage = nouveau;
        change
    }

    pub fn defiler_clavier(&mut self, d: Defil) -> bool {
        let page = self.vt.lignes().saturating_sub(1).max(1) as isize;
        match d {
            Defil::PageHaut => self.defiler(page),
            Defil::PageBas => self.defiler(-page),
            Defil::Debut => self.defiler(isize::MAX / 2),
            Defil::Fin => self.defiler(isize::MIN / 2),
        }
    }

    /// La ligne de la grille affichée à la rangée `i` (0 = haut du panneau), compte tenu du défilement.
    /// Elle peut être plus courte que la grille : le rendu complète par des blancs.
    pub fn ligne_visible(&self, i: usize) -> Option<&[Cellule]> {
        let h = self.vt.historique_len();
        let decalage = if self.vt.ecran_alterne() { 0 } else { self.decalage.min(h) };
        let v = h - decalage + i;
        if v < h {
            self.vt.ligne_historique(v)
        } else {
            self.vt.ligne(v - h)
        }
    }

    /// Ligne absolue de la rangée visible `i`
    pub fn ligne_absolue(&self, i: usize) -> u64 {
        let h = self.vt.historique_len();
        let decalage = if self.vt.ecran_alterne() { 0 } else { self.decalage.min(h) };
        self.vt.lignes_ecoulees() + (h - decalage + i) as u64
    }

    /// Le curseur est-il dans la zone visible ? Rend sa rangée si oui (il disparaît quand on regarde l'historique)
    pub fn rangee_du_curseur(&self) -> Option<usize> {
        if self.decalage > 0 && !self.vt.ecran_alterne() {
            return None;
        }
        let (_, l) = self.vt.curseur();
        (l < self.vt.lignes()).then_some(l)
    }

    // ---------------------------------------------------------------- sélection

    pub fn effacer_selection(&mut self) {
        self.ancre = None;
        self.fin_selection = None;
    }

    pub fn a_selection(&self) -> bool {
        matches!((self.ancre, self.fin_selection), (Some(a), Some(f)) if a != f)
    }

    /// Début d'une sélection à la rangée visible `i`, colonne `col`
    pub fn commencer_selection(&mut self, i: usize, col: usize) {
        let p = Pos { ligne: self.ligne_absolue(i), col: col.min(self.vt.colonnes()) };
        self.ancre = Some(p);
        self.fin_selection = Some(p);
    }

    pub fn etendre_selection(&mut self, i: usize, col: usize) {
        if self.ancre.is_some() {
            self.fin_selection = Some(Pos { ligne: self.ligne_absolue(i), col: col.min(self.vt.colonnes()) });
        }
    }

    /// La sélection ordonnée du début vers la fin
    fn bornes(&self) -> Option<(Pos, Pos)> {
        let (a, f) = (self.ancre?, self.fin_selection?);
        (a != f).then(|| if a <= f { (a, f) } else { (f, a) })
    }

    /// Colonnes `[debut, fin)` sélectionnées sur la rangée visible `i`, s'il y en a
    pub fn selection_sur_rangee(&self, i: usize) -> Option<(usize, usize)> {
        let (d, f) = self.bornes()?;
        let abs = self.ligne_absolue(i);
        if abs < d.ligne || abs > f.ligne {
            return None;
        }
        let colonnes = self.vt.colonnes();
        let debut = if abs == d.ligne { d.col } else { 0 };
        let fin = if abs == f.ligne { f.col } else { colonnes };
        (debut < fin).then_some((debut, fin))
    }

    /// Le texte sélectionné : une ligne par `\n`, sans les blancs de fin de ligne
    pub fn texte_selection(&self) -> Option<String> {
        let (d, f) = self.bornes()?;
        let ecoulees = self.vt.lignes_ecoulees();
        let h = self.vt.historique_len();
        let colonnes = self.vt.colonnes();
        let mut sortie = String::new();
        for abs in d.ligne..=f.ligne {
            let Some(idx) = abs.checked_sub(ecoulees) else { continue }; // sortie de l'historique depuis
            let idx = idx as usize;
            let cellules: Option<&[Cellule]> = if idx < h { self.vt.ligne_historique(idx) } else { self.vt.ligne(idx - h) };
            let debut = if abs == d.ligne { d.col } else { 0 };
            let fin = if abs == f.ligne { f.col } else { colonnes };
            let mut ligne = String::new();
            if let Some(cellules) = cellules {
                for c in cellules.iter().take(fin).skip(debut) {
                    if c.largeur != 0 {
                        ligne.push(c.c);
                    }
                }
            }
            sortie.push_str(ligne.trim_end());
            if abs != f.ligne {
                sortie.push('\n');
            }
        }
        (!sortie.trim().is_empty()).then_some(sortie)
    }

    pub fn modes(&self) -> Modes {
        self.vt.modes()
    }
}

/// Défilement demandé par une combinaison de touches (Maj+PgHaut/PgBas, Ctrl+Maj+Début/Fin)
pub fn defilement_de_touche(vk: u32, m: Mods) -> Option<Defil> {
    if m.alt || !m.maj {
        return None;
    }
    match (vk, m.ctrl) {
        (VK_PRIOR, false) => Some(Defil::PageHaut),
        (VK_NEXT, false) => Some(Defil::PageBas),
        (VK_HOME, true) => Some(Defil::Debut),
        (VK_END, true) => Some(Defil::Fin),
        _ => None,
    }
}

const VK_BACK: u32 = 0x08;
const VK_TAB: u32 = 0x09;
const VK_RETURN: u32 = 0x0D;
const VK_ESCAPE: u32 = 0x1B;
const VK_PRIOR: u32 = 0x21;
const VK_NEXT: u32 = 0x22;
const VK_END: u32 = 0x23;
const VK_HOME: u32 = 0x24;
const VK_LEFT: u32 = 0x25;
const VK_UP: u32 = 0x26;
const VK_RIGHT: u32 = 0x27;
const VK_DOWN: u32 = 0x28;
const VK_INSERT: u32 = 0x2D;
const VK_DELETE: u32 = 0x2E;
const VK_F1: u32 = 0x70;
const VK_F12: u32 = 0x7B;

/// Les touches qui ne produisent pas de WM_CHAR utile (ou dont le WM_CHAR est ambigu) → octets pour le shell.
/// `None` : touche non gérée ici (une lettre, un chiffre... arrivent par WM_CHAR). Entrée, Tab, Échap et Retour arrière
/// sont traduits ici, et la fenêtre avale alors leur WM_CHAR, pour ne les envoyer qu'une fois.
pub fn touche_vers_octets(vk: u32, m: Mods, modes: &Modes) -> Option<Vec<u8>> {
    // Paramètre de modificateur des séquences CSI : 1 + Maj(1) + Alt(2) + Ctrl(4)
    let param = 1 + u32::from(m.maj) + 2 * u32::from(m.alt) + 4 * u32::from(m.ctrl);
    let csi_lettre = |lettre: char| -> Vec<u8> {
        if param == 1 {
            if modes.touches_curseur_appli {
                format!("\x1bO{lettre}").into_bytes()
            } else {
                format!("\x1b[{lettre}").into_bytes()
            }
        } else {
            format!("\x1b[1;{param}{lettre}").into_bytes()
        }
    };
    let csi_tilde = |n: u32| -> Vec<u8> {
        if param == 1 {
            format!("\x1b[{n}~").into_bytes()
        } else {
            format!("\x1b[{n};{param}~").into_bytes()
        }
    };
    let octets = match vk {
        VK_UP => csi_lettre('A'),
        VK_DOWN => csi_lettre('B'),
        VK_RIGHT => csi_lettre('C'),
        VK_LEFT => csi_lettre('D'),
        VK_HOME => csi_lettre('H'),
        VK_END => csi_lettre('F'),
        VK_PRIOR => csi_tilde(5),
        VK_NEXT => csi_tilde(6),
        VK_INSERT => csi_tilde(2),
        VK_DELETE => csi_tilde(3),
        VK_F1..=VK_F12 => {
            let n = vk - VK_F1 + 1;
            match n {
                1..=4 => {
                    let lettre = ['P', 'Q', 'R', 'S'][(n - 1) as usize];
                    if param == 1 {
                        format!("\x1bO{lettre}").into_bytes()
                    } else {
                        format!("\x1b[1;{param}{lettre}").into_bytes()
                    }
                }
                _ => {
                    let code = match n {
                        5 => 15,
                        6 => 17,
                        7 => 18,
                        8 => 19,
                        9 => 20,
                        10 => 21,
                        11 => 23,
                        _ => 24,
                    };
                    csi_tilde(code)
                }
            }
        }
        VK_RETURN => {
            if m.ctrl {
                vec![b'\n']
            } else if m.alt {
                vec![0x1b, b'\r']
            } else {
                vec![b'\r']
            }
        }
        VK_TAB => {
            if m.maj {
                b"\x1b[Z".to_vec()
            } else if m.alt {
                vec![0x1b, b'\t']
            } else {
                vec![b'\t']
            }
        }
        VK_ESCAPE => vec![0x1b],
        VK_BACK => {
            // 0x7f est le Retour arrière des shells modernes ; Ctrl+Retour arrière efface un mot (0x08)
            let base = if m.ctrl { 0x08 } else { 0x7f };
            if m.alt {
                vec![0x1b, base]
            } else {
                vec![base]
            }
        }
        // Alt+lettre ou chiffre : ESC puis le caractère (le WM_SYSCHAR ne passe pas par WM_CHAR)
        0x30..=0x39 | 0x41..=0x5A if m.alt && !m.ctrl => {
            let c = vk as u8;
            let c = if (0x41..=0x5A).contains(&vk) && !m.maj { c.to_ascii_lowercase() } else { c };
            vec![0x1b, c]
        }
        _ => return None,
    };
    Some(octets)
}

/// Taille de la grille (colonnes, lignes) que contient une zone de `largeur` × `hauteur` pixels pour des cellules de
/// `cw` × `ch` pixels ; jamais sous le plancher (`COLONNES_MIN` × `LIGNES_MIN`) ni au-dessus des plafonds de l'émulateur.
pub fn dimensions_grille(largeur: f32, hauteur: f32, cw: f32, ch: f32) -> (usize, usize) {
    if cw <= 0.0 || ch <= 0.0 {
        return (COLONNES_MIN, LIGNES_MIN);
    }
    let c = (largeur / cw).floor().max(0.0) as usize;
    let l = (hauteur / ch).floor().max(0.0) as usize;
    (c.clamp(COLONNES_MIN, 1_000), l.clamp(LIGNES_MIN, 500))
}

#[cfg(test)]
mod tests;
