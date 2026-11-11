// Menus au style Nothing, dessinés dans la fenêtre : boutons de la barre d'actions et clic droit.
// Police Doto, point rouge pour l'état en cours, séparateurs en pointillés, sous-menus.
// Le menu complet de Windows reste à un clic (« PLUS D'OPTIONS », ou Maj+clic droit).

use windows::Win32::Graphics::Direct2D::Common::D2D_RECT_F;

use crate::app::Cible;
use crate::palette::Commande;
use crate::rendu::{dedans, rect};

/// Contenu d'un menu, avant mise en forme
pub enum Choix {
    /// Commande, cochée (état en cours)
    Commande(Commande, bool),
    Separation,
    Sous(&'static str, Vec<Choix>),
    /// Le menu clic droit complet de Windows
    PlusOptions,
    /// Action hors palette : verbe Windows, application de OUVRIR AVEC
    Autre(String, Autre),
    /// Rangée d'icônes en haut du menu (couper, copier, renommer…) : bouton de la barre et commande lancée
    Icones(Vec<(crate::barre::Outil, Commande)>),
}

/// Actions propres aux menus (pas de commande de palette)
#[derive(Clone, Copy)]
pub enum Autre {
    /// Verbe du Shell sur les éléments choisis (« runas », « setdesktopwallpaper », « print »…)
    Verbe(&'static str),
    /// k-ième application proposée pour ouvrir le fichier
    Application(usize),
    /// Boîte « Ouvrir avec » de Windows
    ChoisirApplication,
    /// Ouvre le dossier choisi dans l'autre panneau
    AutrePanneau,
    /// Copie (false) ou déplace (true) la sélection dans le dossier de l'autre panneau
    VersAutre(bool),
    /// Barre d'état : tout désélectionner, copier les noms des éléments choisis
    Deselectionner,
    CopierNoms,
    /// Met les éléments choisis dans le k-ième groupe, ou les en retire s'ils y sont tous
    BasculerGroupe(usize),
    /// Demande un nom (palette) et met les éléments choisis dans ce nouveau groupe
    NouveauGroupe,
    /// Retire les éléments choisis du groupe ouvert
    RetirerDuGroupe,
    /// Groupe k du volet : ouvrir, ouvrir dans un onglet, renommer, couleur, supprimer
    OuvrirGroupe(usize),
    OngletGroupe(usize),
    RenommerGroupe(usize),
    CouleurGroupe(usize, char),
    SupprimerGroupe(usize),
    /// Après un glisser avec le bouton droit : 0 copier, 1 déplacer, 2 raccourcis, 3 annuler
    DeposerIci(u8),
    /// En-tête de la liste : montrer ou cacher une colonne (bit COL_*), date montrée (0, 1, 2)
    BasculerColonne(u32),
    /// Regrouper l'affichage : rang dans Regroupement::TOUS
    Regrouper(u8),
    /// Sessions d'onglets : ouvrir, supprimer (rang dans les réglages)
    OuvrirSession(usize),
    /// Téléchargements : ouvrir le dossier de la copie trouvée ailleurs
    AllerCopie,
    /// SFTP : télécharger les éléments choisis
    TelechargerSftp,
    SupprimerSession(usize),
    DateMontree(u32),
}

pub enum Genre {
    Commande(Commande),
    Separation,
    Sous(Vec<Ligne>),
    PlusOptions,
    Autre(Autre),
    /// Bouton, commande, possible ?
    Icones(Vec<(crate::barre::Outil, Commande, bool)>),
}

pub struct Ligne {
    pub libelle: String,
    pub raccourci: String,
    pub coche: bool,
    pub actif: bool,
    pub genre: Genre,
}

impl Ligne {
    pub fn choisissable(&self) -> bool {
        self.actif && !matches!(self.genre, Genre::Separation | Genre::Icones(_))
    }

    fn hauteur(&self) -> f32 {
        match self.genre {
            Genre::Separation => HAUTEUR_SEPARATION,
            Genre::Icones(_) => HAUTEUR_ICONES,
            _ => HAUTEUR,
        }
    }
}

pub const MARGE: f32 = 6.0;
pub const HAUTEUR: f32 = 30.0;
pub const HAUTEUR_SEPARATION: f32 = 11.0;
/// Rangée d'icônes : les points, et le nom de l'icône survolée dessous
pub const HAUTEUR_ICONES: f32 = 54.0;

pub struct Niveau {
    pub lignes: Vec<Ligne>,
    pub x: f32,
    pub y: f32,
    pub largeur: f32,
    pub survol: Option<usize>,
    /// Sous-menu : ligne du parent qui l'a ouvert
    pub origine: usize,
    /// Icône survolée dans la rangée d'icônes
    pub icone: Option<usize>,
}

impl Niveau {
    pub fn hauteur(&self) -> f32 {
        2.0 * MARGE + self.lignes.iter().map(Ligne::hauteur).sum::<f32>()
    }

    pub fn cadre(&self) -> D2D_RECT_F {
        rect(self.x, self.y, self.largeur, self.hauteur())
    }

    /// Zone de la ligne k
    pub fn zone(&self, k: usize) -> D2D_RECT_F {
        let mut y = self.y + MARGE;
        for l in &self.lignes[..k] {
            y += l.hauteur();
        }
        rect(self.x + MARGE, y, self.largeur - 2.0 * MARGE, self.lignes[k].hauteur())
    }

    /// Case de la j-ième icône d'une rangée d'icônes (ligne k)
    pub fn case_icone(&self, k: usize, j: usize) -> D2D_RECT_F {
        let z = self.zone(k);
        let n = match &self.lignes[k].genre {
            Genre::Icones(v) => v.len().max(1),
            _ => 1,
        };
        let w = (z.right - z.left) / n as f32;
        rect(z.left + j as f32 * w + 2.0, z.top + 2.0, w - 4.0, z.bottom - z.top - 4.0)
    }

    /// Icône sous ce x dans la rangée d'icônes k
    fn icone_sous(&self, k: usize, x: f32, y: f32) -> Option<usize> {
        let Genre::Icones(v) = &self.lignes[k].genre else { return None };
        (0..v.len()).find(|&j| dedans(&self.case_icone(k, j), x, y))
    }

    /// Ligne choisissable suivante (pas = ±1), en boucle
    fn suivante(&self, pas: isize) -> Option<usize> {
        let n = self.lignes.len() as isize;
        let mut k = self.survol.map_or(if pas > 0 { -1 } else { n }, |s| s as isize);
        for _ in 0..n {
            k = (k + pas).rem_euclid(n);
            if self.lignes[k as usize].choisissable() {
                return Some(k as usize);
            }
        }
        None
    }
}

/// Menu ouvert : niveau 0 et sous-menus ouverts
pub struct Menu {
    pub niveaux: Vec<Niveau>,
    /// Bouton de la barre qui l'a ouvert (reste allumé)
    pub outil: Option<crate::barre::Outil>,
    /// Clic droit : zone visée et point à l'écran, pour « PLUS D'OPTIONS »
    pub cible: Option<(Cible, i32, i32)>,
    /// Place disponible (fenêtre, en DIP) : les menus restent dedans
    pub limite: (f32, f32),
}

/// Ce que demande un clic ou une touche
pub enum Action {
    Rien,
    Fermer,
    Commande(Commande),
    PlusOptions,
    Autre(Autre),
}

impl Menu {
    pub fn nouveau(lignes: Vec<Ligne>, x: f32, y: f32, largeur: f32, limite: (f32, f32)) -> Menu {
        let mut n = Niveau { lignes, x, y, largeur, survol: None, origine: 0, icone: None };
        placer(&mut n, limite, None);
        Menu { niveaux: vec![n], outil: None, cible: None, limite }
    }

    /// (niveau, ligne) sous ce point
    pub fn sous(&self, x: f32, y: f32) -> Option<(usize, usize)> {
        for (i, n) in self.niveaux.iter().enumerate().rev() {
            if dedans(&n.cadre(), x, y) {
                return (0..n.lignes.len()).find(|&k| dedans(&n.zone(k), x, y)).map(|k| (i, k));
            }
        }
        None
    }

    pub fn dans(&self, x: f32, y: f32) -> bool {
        self.niveaux.iter().any(|n| dedans(&n.cadre(), x, y))
    }

    /// Survol : la ligne s'allume, un sous-menu s'ouvre
    pub fn survoler(&mut self, x: f32, y: f32) -> bool {
        let Some((i, k)) = self.sous(x, y) else { return false };
        // Rangée d'icônes : seule l'icône sous la souris s'allume
        if matches!(self.niveaux[i].lignes[k].genre, Genre::Icones(_)) {
            let j = self.niveaux[i].icone_sous(k, x, y);
            let n = &mut self.niveaux[i];
            let change = n.icone != j || n.survol.is_some();
            n.icone = j;
            n.survol = None;
            self.reduire(i + 1);
            return change;
        }
        let ancienne_icone = self.niveaux[i].icone.take();
        let avant = self.niveaux[i].survol;
        let choisissable = self.niveaux[i].lignes[k].choisissable();
        self.niveaux[i].survol = choisissable.then_some(k);
        if choisissable && matches!(self.niveaux[i].lignes[k].genre, Genre::Sous(_)) {
            if self.niveaux.len() <= i + 1 || avant != Some(k) {
                self.ouvrir_sous(i, k);
            }
        } else if choisissable {
            self.reduire(i + 1);
        }
        avant != self.niveaux[i].survol || ancienne_icone.is_some()
    }

    fn ouvrir_sous(&mut self, i: usize, k: usize) {
        self.reduire(i + 1);
        let parent = &mut self.niveaux[i];
        let zone = parent.zone(k);
        let largeur_parent = parent.cadre();
        let Genre::Sous(lignes) = &mut parent.lignes[k].genre else { return };
        let lignes = std::mem::take(lignes);
        let largeur = largeur_sous(&lignes);
        let mut n = Niveau { lignes, x: largeur_parent.right - 4.0, y: zone.top - MARGE, largeur, survol: None, origine: k, icone: None };
        placer(&mut n, self.limite, Some(largeur_parent));
        self.niveaux.push(n);
    }

    /// Ferme le dernier sous-menu (ses lignes reviennent dans la ligne qui l'a ouvert)
    fn fermer_sous(&mut self) {
        if self.niveaux.len() < 2 {
            return;
        }
        let Some(n) = self.niveaux.pop() else { return };
        let Some(parent) = self.niveaux.last_mut() else { return };
        if let Some(Genre::Sous(v)) = parent.lignes.get_mut(n.origine).map(|l| &mut l.genre) {
            *v = n.lignes;
        }
    }

    /// Ne garde que les `garder` premiers niveaux
    fn reduire(&mut self, garder: usize) {
        while self.niveaux.len() > garder.max(1) {
            self.fermer_sous();
        }
    }

    pub fn cliquer(&mut self, x: f32, y: f32) -> Action {
        match self.sous(x, y) {
            Some((i, k)) if matches!(self.niveaux[i].lignes[k].genre, Genre::Icones(_)) => {
                let j = self.niveaux[i].icone_sous(k, x, y);
                match (&self.niveaux[i].lignes[k].genre, j) {
                    (Genre::Icones(v), Some(j)) if v[j].2 => Action::Commande(v[j].1),
                    _ => Action::Rien,
                }
            }
            Some((i, k)) => self.activer(i, k),
            None if self.dans(x, y) => Action::Rien,
            None => Action::Fermer,
        }
    }

    fn activer(&mut self, i: usize, k: usize) -> Action {
        let l = &self.niveaux[i].lignes[k];
        if !l.choisissable() {
            return Action::Rien;
        }
        match &l.genre {
            Genre::Commande(c) => Action::Commande(*c),
            Genre::PlusOptions => Action::PlusOptions,
            Genre::Autre(a) => Action::Autre(*a),
            Genre::Sous(_) => {
                self.niveaux[i].survol = Some(k);
                if self.niveaux.len() <= i + 1 {
                    self.ouvrir_sous(i, k);
                }
                if let Some(s) = self.niveaux.get_mut(i + 1) {
                    s.survol = s.suivante(1);
                }
                Action::Rien
            }
            Genre::Separation | Genre::Icones(_) => Action::Rien,
        }
    }

    /// Clavier : ↑↓ choisir, → ou Entrée ouvrir, ← ou Échap revenir
    pub fn touche(&mut self, vk: u16) -> Action {
        const ECHAP: u16 = 0x1B;
        const ENTREE: u16 = 0x0D;
        const ESPACE: u16 = 0x20;
        const GAUCHE: u16 = 0x25;
        const HAUT: u16 = 0x26;
        const DROITE: u16 = 0x27;
        const BAS: u16 = 0x28;
        let i = self.niveaux.len() - 1;
        match vk {
            HAUT | BAS => {
                let n = &mut self.niveaux[i];
                n.survol = n.suivante(if vk == HAUT { -1 } else { 1 });
                Action::Rien
            }
            ENTREE | ESPACE | DROITE => match self.niveaux[i].survol {
                Some(k) if vk != DROITE || matches!(self.niveaux[i].lignes[k].genre, Genre::Sous(_)) => self.activer(i, k),
                _ => Action::Rien,
            },
            GAUCHE if i > 0 => {
                self.fermer_sous();
                Action::Rien
            }
            ECHAP if i > 0 => {
                self.fermer_sous();
                Action::Rien
            }
            ECHAP => Action::Fermer,
            _ => Action::Rien,
        }
    }
}

/// Largeur d'un sous-menu (sans mesure de texte : Doto est à chasse presque fixe, 8,4 DIP par lettre à 14 DIP, mesuré, plus une marge). Le raccourci est
/// dessiné en touches : sa largeur vient de `rendu::largeur_touches`, plus un écart avec le libellé.
pub fn largeur_sous(lignes: &[Ligne]) -> f32 {
    const LETTRE: f32 = 9.0;
    let echelle = crate::reglages::echelle_texte();
    let largeur = lignes
        .iter()
        .map(|l| l.libelle.chars().count() as f32 * LETTRE * echelle + if l.raccourci.is_empty() { 0.0 } else { crate::rendu::largeur_touches(&l.raccourci) + 16.0 })
        .fold(100.0, f32::max);
    (largeur + 74.0).clamp(180.0, 560.0)
}

/// Garde le menu dans la fenêtre ; un sous-menu qui déborde à droite passe à gauche de son parent
fn placer(n: &mut Niveau, (l, h): (f32, f32), parent: Option<D2D_RECT_F>) {
    if n.x + n.largeur > l - 4.0 {
        n.x = match parent {
            Some(p) => p.left - n.largeur + 4.0,
            None => l - 4.0 - n.largeur,
        };
    }
    n.x = n.x.max(4.0);
    let hauteur = n.hauteur();
    if n.y + hauteur > h - 4.0 {
        n.y = (h - 4.0 - hauteur).max(4.0);
    }
}
