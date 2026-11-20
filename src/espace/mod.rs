// Page ESPACE (chantier ESPACE, E1) : où est passé l'espace disque, et plus tard comment le récupérer.
//
// `analyse` : l'arbre trié par taille, lu dans l'index enregistré (logique pure, testée sans fenêtre).
// Ce fichier : l'état de la page (rail à gauche, liste à droite, ligne choisie, défilement) et ses touches. Le dessin est
// dans `rendu.rs` (`dessiner_espace`), l'entrée et la sortie de la page dans `fenetre/`. Rien n'est lu sur le disque ici :
// jamais de parcours devant l'utilisateur, l'index enregistré est montré avec sa date.

pub mod analyse;
pub mod carte;
pub mod instantane;
pub mod nettoyage;

use analyse::{Arbre, Cellule};

/// Pages du rail, dans l'ordre. Seule la première existe en E1 ; les autres arrivent avec leurs étapes (E4 à E9).
pub const PAGES: [&str; 5] = ["Analyse", "Nettoyage", "Programmes", "Démarrage", "Mémoire"];
/// Titre en toutes lettres de chaque page : dit ce qu'elle fait (le rail garde des mots courts)
pub const TITRES: [&str; 5] = ["Ce qui prend de la place sur ce PC", "Libérer de la place", "Programmes installés", "Démarrage de Windows", "Mémoire utilisée"];
/// Rang de la page qui existe
pub const PAGE_ANALYSE: usize = 0;
/// Rang de la page NETTOYAGE (E4a : lecture seule)
pub const PAGE_NETTOYAGE: usize = 1;

/// Hauteur d'une ligne de l'arbre (DIP)
pub const LIGNE_H: f32 = 36.0;
pub const LIGNE_H_COMPACTE: f32 = 28.0;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Zone {
    Rail,
    Liste,
}

/// Façon de montrer l'espace : la carte en rectangles, l'arbre trié par taille, ou l'un des trois bilans (E3, lecture seule)
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Vue {
    Arbre,
    Carte,
    /// Bilan par type de fichier (images, vidéos, archives…)
    Types,
    /// Les plus gros fichiers de l'index
    Gros,
    /// Bilan par âge de la date de modification
    Age,
}

/// Noms des vues, dans l'ordre du contrôle segmenté (rang 0 = carte : c'est la vue de base, donc à gauche)
pub const VUES: [&str; 5] = ["Carte", "Arbre", "Types", "Gros fichiers", "Âge"];

impl Vue {
    /// Rang dans `VUES`
    pub fn rang(self) -> usize {
        match self {
            Vue::Carte => 0,
            Vue::Arbre => 1,
            Vue::Types => 2,
            Vue::Gros => 3,
            Vue::Age => 4,
        }
    }

    /// Vue du segment de rang `k` de `VUES`
    pub fn depuis_rang(k: usize) -> Option<Vue> {
        [Vue::Carte, Vue::Arbre, Vue::Types, Vue::Gros, Vue::Age].get(k).copied()
    }

    /// Une liste de bilan (types, gros fichiers, âge), par opposition à la carte et à l'arbre
    pub fn est_bilan(self) -> bool {
        matches!(self, Vue::Types | Vue::Gros | Vue::Age)
    }

    /// À qui appartient la ligne choisie et le défilement : la carte et l'arbre partagent les leurs (la carte suit l'arbre)
    fn proprietaire(self) -> usize {
        match self {
            Vue::Carte | Vue::Arbre => 0,
            Vue::Types => 1,
            Vue::Gros => 2,
            Vue::Age => 3,
        }
    }
}

/// Une pastille du fil d'Ariane de la carte : un dossier de la pile, cliquable pour y remonter
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Miette {
    pub libelle: String,
    /// Profondeur de pile à rejoindre en cliquant (0 : les disques)
    pub niveau: usize,
}

/// Nombre de lettres au-delà duquel le nom d'une pastille est coupé par « … »
const MIETTE_LETTRES: usize = 22;
/// Largeur estimée d'une pastille (DIP) : le dessin et la souris la calculent de la même façon, sans mesurer de texte
pub fn largeur_miette(libelle: &str) -> f32 {
    libelle.chars().count() as f32 * 7.6 + 22.0
}
/// Espace entre deux pastilles, où se dessine le séparateur (DIP)
pub const MIETTE_ECART: f32 = 20.0;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Touche {
    Haut,
    Bas,
    Droite,
    Gauche,
    Entree,
    Retour,
    Debut,
    Fin,
    PageHaut,
    PageBas,
    Tab,
    /// Passe de l'arbre à la carte, et inversement (touche V)
    Basculer,
    /// Ouvre le dossier choisi dans l'explorateur (touche O) ; dans l'arbre, Entrée fait la même chose
    Ouvrir,
    /// Choisit la vue de rang `k` de `VUES` (touches 1 à 5), seule voie au clavier vers les bilans
    Vue(usize),
    /// Coche ou décoche la catégorie choisie de la page NETTOYAGE (Espace)
    Cocher,
}

/// Où en est le nettoyage : la liste, la nouvelle mesure qui précède toute confirmation, la confirmation elle-même
/// (avec la liste exacte de ce qui sera effacé, mesurée à l'instant), ou la suppression en cours
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum EtapeNet {
    Liste,
    Preparation,
    Confirmation(Vec<nettoyage::Categorie>),
    Suppression,
}

/// Boutons du bas de la page NETTOYAGE (`Cible::NettoyageBouton`) : supprimer la sélection, confirmer, annuler
pub const NET_SUPPRIMER: usize = 0;
pub const NET_CONFIRMER: usize = 1;
pub const NET_ANNULER: usize = 2;

/// Nombre de rectangles dont Page haut / Page bas déplacent le choix dans la carte
const SAUT_CARTE: usize = 8;

/// Ce que la fenêtre doit faire après une touche
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Action {
    Rien,
    /// Ouvrir ce dossier dans l'explorateur
    Ouvrir(String),
    /// Quitter la page (retour d'un cran)
    Quitter,
    /// Effacer ces catégories (la confirmation vient d'être donnée) : la fenêtre lance le fil de suppression
    Effacer(Vec<nettoyage::Categorie>),
}

pub struct Etat {
    pub page: usize,
    pub zone: Zone,
    /// Rempli quand l'index est prêt ; libéré en quittant la page
    pub arbre: Option<Arbre>,
    /// Ligne choisie dans l'arbre
    pub focus: usize,
    /// Position de défilement voulue (DIP) : la molette et le clavier la règlent, les tests la lisent
    pub defilement: f32,
    /// Position dessinée : elle rejoint `defilement` en glissant, comme la liste de fichiers
    pub affiche: f32,
    dernier_lissage: Option<std::time::Instant>,
    pub vue: Vue,
    /// Carte : dossiers dans lesquels on est descendu, du premier au dernier (vide : les disques)
    pub carte_pile: Vec<u32>,
    /// Carte : les rectangles du dossier montré, du plus gros au plus petit
    pub carte: Vec<Cellule>,
    /// Carte : rectangle choisi
    pub carte_focus: usize,
    /// Carte : pour chaque rectangle, ses plus gros éléments (dessinés à l'intérieur d'un dossier assez grand). Calculés à
    /// chaque changement de dossier, jamais à l'image : le dessin ne fait que les lire.
    pub carte_apercus: Vec<Vec<Cellule>>,
    /// Vrai juste après une bascule vers l'arbre : la fenêtre, qui connaît la hauteur de la liste, y centre la ligne choisie
    pub a_centrer: bool,
    /// Dernière vue de base (carte ou arbre) montrée : on y revient en quittant un bilan, et c'est elle qui dit si l'autre
    /// doit se recaler sur sa position
    base: Vue,
    /// Ligne choisie, défilement voulu et défilement dessiné de chaque liste qui n'est pas affichée (`Vue::proprietaire`) :
    /// chaque liste retrouve sa place quand on y revient
    memo: [(usize, f32, f32); 4],
    /// Page NETTOYAGE : catégories mesurées (`None` tant que la mesure n'est pas revenue)
    pub nettoyage: Option<Vec<nettoyage::Categorie>>,
    /// Une mesure est lancée : on n'en lance pas une seconde
    pub nettoyage_en_cours: bool,
    /// Catégorie choisie dans la page NETTOYAGE
    pub net_focus: usize,
    /// Catégories cochées (par identifiant : une nouvelle mesure ne les perd pas). Rien n'est coché par défaut.
    pub net_cochees: Vec<&'static str>,
    pub net_etape: EtapeNet,
    /// Dernier résultat ou refus, dit sous la liste
    pub net_message: Option<String>,
    /// Première ligne montrée quand la molette a fait défiler la liste ; `None` : la liste suit la ligne choisie (clavier)
    pub net_debut: Option<usize>,
    /// Reste de molette inférieur à une ligne (pavé tactile : plusieurs petits crans font une ligne)
    net_cran: f32,
}

/// Parts minimales d'un dossier pour qu'on lui calcule un aperçu, et nombre d'éléments gardés
const APERCU_PART_MIN: f32 = 0.03;
const APERCU_ELEMENTS: usize = 12;
const APERCU_DOSSIERS_MAX: usize = 40;

/// Un pas de glissement de `de` vers `vers` pendant `dt` secondes (même courbe que la liste de fichiers)
fn lisser(de: f32, vers: f32, dt: f32) -> f32 {
    de + (vers - de) * crate::app::part_lissage(dt)
}

impl Etat {
    pub fn nouveau() -> Self {
        Self {
            page: PAGE_ANALYSE,
            zone: Zone::Liste,
            arbre: None,
            focus: 0,
            defilement: 0.0,
            affiche: 0.0,
            dernier_lissage: None,
            vue: Vue::Arbre,
            carte_pile: Vec::new(),
            carte: Vec::new(),
            carte_focus: 0,
            carte_apercus: Vec::new(),
            a_centrer: false,
            base: Vue::Arbre,
            memo: [(0, 0.0, 0.0); 4],
            nettoyage: None,
            nettoyage_en_cours: false,
            net_focus: 0,
            net_cochees: Vec::new(),
            net_etape: EtapeNet::Liste,
            net_message: None,
            net_debut: None,
            net_cran: 0.0,
        }
    }

    /// Reprend d'un ancien état tout ce qui appartient à la page NETTOYAGE : la page NETTOYAGE ne dépend pas de l'index,
    /// un index qui s'installe ne doit ni la quitter, ni perdre sa mesure ou ses coches, ni en lancer une seconde (le fil
    /// de la première court peut-être encore, ou une suppression est en cours).
    pub fn reprendre_nettoyage(&mut self, ancien: Etat) {
        self.page = ancien.page;
        self.nettoyage = ancien.nettoyage;
        self.nettoyage_en_cours = ancien.nettoyage_en_cours;
        self.net_focus = ancien.net_focus;
        self.net_cochees = ancien.net_cochees;
        self.net_etape = ancien.net_etape;
        self.net_message = ancien.net_message;
        self.net_debut = ancien.net_debut;
    }

    /// Faut-il lancer la mesure du nettoyage ? Vrai une seule fois, à l'arrivée sur la page NETTOYAGE : la fenêtre
    /// lance alors le fil de fond (l'état ne connaît pas la fenêtre). Jamais pendant une suppression : la mesure
    /// repartira à sa fin.
    pub fn nettoyage_a_lancer(&mut self) -> bool {
        if self.page == PAGE_NETTOYAGE
            && self.nettoyage.is_none()
            && !self.nettoyage_en_cours
            && self.net_etape != EtapeNet::Suppression
        {
            self.nettoyage_en_cours = true;
            return true;
        }
        false
    }

    /// Reçoit le résultat de la mesure. Après un Entrée sur une sélection (étape `Preparation`), c'est cette mesure toute
    /// fraîche, et non celle de l'ouverture de la page, qui fixe la liste montrée dans la confirmation.
    pub fn recevoir_nettoyage(&mut self, c: Vec<nettoyage::Categorie>) {
        self.net_focus = self.net_focus.min(c.len().saturating_sub(1));
        // Une catégorie qui n'a plus rien à proposer perd sa coche
        self.net_cochees.retain(|id| c.iter().any(|k| k.id == *id));
        self.nettoyage_en_cours = false;
        if self.net_etape == EtapeNet::Preparation {
            let choisies: Vec<nettoyage::Categorie> = c.iter().filter(|k| self.net_cochees.contains(&k.id)).cloned().collect();
            if choisies.is_empty() {
                self.net_etape = EtapeNet::Liste;
                self.net_message = Some("Plus rien à nettoyer dans cette sélection".to_string());
            } else {
                self.net_etape = EtapeNet::Confirmation(choisies);
            }
        }
        self.nettoyage = Some(c);
    }

    /// La suppression est finie : coches remises à zéro, résultat dit sous la liste, nouvelle mesure demandée
    pub fn finir_suppression(&mut self, message: String) {
        self.net_etape = EtapeNet::Liste;
        self.net_cochees.clear();
        self.net_message = Some(message);
        self.nettoyage = None;
        self.nettoyage_en_cours = false;
    }

    /// Échap sur la page NETTOYAGE : annule la confirmation ou la préparation en cours. Rend vrai si la touche est
    /// consommée (alors la page reste). Pendant la suppression elle quitte la page, comme sur la liste : l'effacement
    /// continue en fond et son bilan arrive quand même ; la page ne doit jamais emprisonner l'auteur.
    pub fn echap_nettoyage(&mut self) -> bool {
        if self.page != PAGE_NETTOYAGE {
            return false;
        }
        match self.net_etape {
            EtapeNet::Liste | EtapeNet::Suppression => false,
            EtapeNet::Preparation | EtapeNet::Confirmation(_) => {
                self.net_etape = EtapeNet::Liste;
                true
            }
        }
    }

    /// Touches de la page NETTOYAGE : se déplacer, cocher (Espace), demander la suppression (Entrée), confirmer
    /// (Entrée encore), annuler (Échap). Rien n'est supprimé sans la confirmation.
    fn touche_nettoyage(&mut self, t: Touche) -> Action {
        match self.net_etape.clone() {
            // Seule sortie permise : quitter la page (la suppression, elle, continue en fond)
            EtapeNet::Suppression => return if t == Touche::Retour { Action::Quitter } else { Action::Rien },
            EtapeNet::Preparation => {
                if t == Touche::Retour {
                    self.net_etape = EtapeNet::Liste;
                }
                return Action::Rien;
            }
            EtapeNet::Confirmation(v) => {
                match t {
                    Touche::Retour => self.net_etape = EtapeNet::Liste,
                    Touche::Entree => {
                        self.net_etape = EtapeNet::Suppression;
                        self.net_message = None;
                        return Action::Effacer(v);
                    }
                    _ => {}
                }
                return Action::Rien;
            }
            EtapeNet::Liste => {}
        }
        // Une touche rend la main au clavier : la liste suit de nouveau la ligne choisie (la molette l'avait peut-être déplacée)
        self.net_debut = None;
        let n = self.nettoyage.as_ref().map_or(0, |c| c.len());
        match t {
            Touche::Haut => self.net_focus = self.net_focus.saturating_sub(1),
            Touche::Bas => self.net_focus = (self.net_focus + 1).min(n.saturating_sub(1)),
            Touche::PageHaut | Touche::Debut => self.net_focus = 0,
            Touche::PageBas | Touche::Fin => self.net_focus = n.saturating_sub(1),
            Touche::Cocher => self.net_basculer(),
            Touche::Entree => {
                if self.nettoyage.is_none() {
                    return Action::Rien;
                }
                if self.net_cochees.is_empty() {
                    self.net_message = Some("Cochez d'abord une catégorie (Espace)".to_string());
                } else {
                    // La liste de la confirmation vient d'une mesure faite maintenant, pas de celle de l'ouverture
                    self.net_message = None;
                    self.net_etape = EtapeNet::Preparation;
                    self.nettoyage = None;
                }
            }
            Touche::Retour => return Action::Quitter,
            _ => {}
        }
        Action::Rien
    }

    /// Coche ou décoche la catégorie choisie
    fn net_basculer(&mut self) {
        let id = self.nettoyage.as_ref().and_then(|c| c.get(self.net_focus)).map(|k| k.id);
        if let Some(id) = id {
            match self.net_cochees.iter().position(|x| *x == id) {
                Some(i) => {
                    self.net_cochees.remove(i);
                }
                None => self.net_cochees.push(id),
            }
            self.net_message = None;
        }
    }

    /// Clic sur la ligne de rang `k` : la choisit, et la coche ou décoche si `cocher` (clic sur la case, double-clic).
    /// `debut` est la première ligne montrée à cet instant : la liste ne doit pas bouger sous le pointeur, alors que la
    /// règle « suivre la ligne choisie » la recalerait. Sans effet hors de la liste.
    pub fn net_cliquer(&mut self, k: usize, cocher: bool, debut: usize) {
        let n = self.nettoyage.as_ref().map_or(0, |c| c.len());
        if self.page != PAGE_NETTOYAGE || self.net_etape != EtapeNet::Liste || k >= n {
            return;
        }
        self.net_focus = k;
        self.net_debut = Some(debut);
        if cocher {
            self.net_basculer();
        }
    }

    /// Molette sur la liste (`delta` positif = vers le haut, fractionnaire sur un pavé tactile) : déplace le début de la
    /// liste sans toucher à la ligne choisie. `debut` et `visibles` : la première ligne montrée et le nombre de lignes entières.
    pub fn molette_nettoyage(&mut self, delta: f32, debut: usize, visibles: usize) {
        let n = self.nettoyage.as_ref().map_or(0, |c| c.len());
        if self.page != PAGE_NETTOYAGE || self.net_etape != EtapeNet::Liste || n == 0 {
            return;
        }
        let maxi = n.saturating_sub(visibles);
        self.net_cran -= delta * 2.0;
        let lignes = self.net_cran.trunc();
        self.net_cran -= lignes;
        let voulu = debut as f32 + lignes;
        let nouveau = voulu.clamp(0.0, maxi as f32) as usize;
        // Quand la molette pousse contre une butée, le reste ne s'accumule pas : sinon il faudrait « rembourser » avant de
        // repartir. Un petit cran vers l'intérieur depuis la butée, lui, s'accumule (pavé tactile).
        if voulu < 0.0 || voulu > maxi as f32 {
            self.net_cran = 0.0;
        }
        self.net_debut = Some(nouveau);
    }

    /// Clic sur un bouton du bas de page (`NET_SUPPRIMER` sur la liste ; `NET_CONFIRMER`, `NET_ANNULER` sur la confirmation) :
    /// la même chose que la touche correspondante, donc la même confirmation avant tout effacement
    pub fn net_bouton(&mut self, k: usize) -> Action {
        if self.page != PAGE_NETTOYAGE {
            return Action::Rien;
        }
        match (k, &self.net_etape) {
            (NET_SUPPRIMER, EtapeNet::Liste) | (NET_CONFIRMER, EtapeNet::Confirmation(_)) => self.touche_nettoyage(Touche::Entree),
            (NET_ANNULER, EtapeNet::Confirmation(_)) => self.touche_nettoyage(Touche::Retour),
            _ => Action::Rien,
        }
    }

    /// Les lignes de la liste affichée : celles de l'arbre, ou celles du bilan montré (vides tant qu'il n'est pas calculé)
    pub fn lignes(&self) -> &[analyse::Ligne] {
        let Some(arbre) = self.arbre.as_ref() else { return &[] };
        let bilans = arbre.bilans();
        match self.vue {
            Vue::Types => bilans.map_or(&[], |b| &b.types[..]),
            Vue::Gros => bilans.map_or(&[], |b| &b.gros[..]),
            Vue::Age => bilans.map_or(&[], |b| &b.ages[..]),
            Vue::Arbre | Vue::Carte => arbre.lignes(),
        }
    }

    /// Calcule le bilan de la vue montrée s'il ne l'est pas encore. Un seul passage sur l'index, gardé ensuite.
    fn preparer_bilan(&mut self) {
        let maintenant = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs());
        if let Some(arbre) = self.arbre.as_mut() {
            arbre.preparer_bilans(maintenant);
        }
    }

    /// Range la ligne choisie et le défilement de la liste qu'on quitte, reprend ceux de la liste qu'on montre
    fn echanger_memo(&mut self, de: Vue, vers: Vue) {
        let (a, b) = (de.proprietaire(), vers.proprietaire());
        if a == b {
            return;
        }
        self.memo[a] = (self.focus, self.defilement, self.affiche);
        (self.focus, self.defilement, self.affiche) = self.memo[b];
    }

    /// Dossier que la carte montre (`None` : les disques)
    pub fn carte_parent(&self) -> Option<u32> {
        self.carte_pile.last().copied()
    }

    /// Profondeur sous laquelle on ne remonte pas : 1 quand il n'y a qu'un disque (la carte s'ouvre dedans), sinon 0
    fn pile_min(&self) -> usize {
        let Some(arbre) = self.arbre.as_ref() else { return 0 };
        let disques = arbre.cellules(None);
        usize::from(disques.len() == 1 && disques[0].dossier)
    }

    /// Refait les rectangles du dossier montré
    fn actualiser_carte(&mut self) {
        self.carte = self.arbre.as_ref().map_or_else(Vec::new, |a| a.cellules(self.carte_parent()));
        self.carte_focus = self.carte_focus.min(self.carte.len().saturating_sub(1));
        self.carte_apercus = match self.arbre.as_ref() {
            Some(a) => self
                .carte
                .iter()
                .enumerate()
                .map(|(i, c)| match c.id {
                    Some(id) if c.dossier && c.part >= APERCU_PART_MIN && i < APERCU_DOSSIERS_MAX => a.apercu(id, APERCU_ELEMENTS),
                    _ => Vec::new(),
                })
                .collect(),
            None => Vec::new(),
        };
    }

    /// Le fil d'Ariane : « Tous les disques » (sauf s'il n'y a qu'un disque) puis un dossier par niveau de la pile. Trop long
    /// pour `largeur` DIP : les premiers se replient en une pastille « … » qui remonte d'un cran en deçà du premier montré.
    pub fn miettes(&self, largeur: f32) -> Vec<Miette> {
        let Some(arbre) = self.arbre.as_ref() else { return Vec::new() };
        let mut toutes = Vec::new();
        if self.pile_min() == 0 {
            toutes.push(Miette { libelle: "Tous les disques".into(), niveau: 0 });
        }
        for (i, &id) in self.carte_pile.iter().enumerate() {
            let nom = arbre.libelle_de(id, i == 0);
            let libelle = if nom.chars().count() > MIETTE_LETTRES { format!("{}…", nom.chars().take(MIETTE_LETTRES - 1).collect::<String>()) } else { nom };
            toutes.push(Miette { libelle, niveau: i + 1 });
        }
        // On garde les derniers tant qu'ils tiennent ; le dernier (l'endroit où l'on est) reste toujours
        let reserve = largeur_miette("…") + MIETTE_ECART;
        let mut utilise = 0.0;
        let mut premier = toutes.len();
        for (i, m) in toutes.iter().enumerate().rev() {
            let w = largeur_miette(&m.libelle) + if premier == toutes.len() { 0.0 } else { MIETTE_ECART };
            let a_cacher = i > 0;
            if premier < toutes.len() && utilise + w + if a_cacher { reserve } else { 0.0 } > largeur {
                break;
            }
            utilise += w;
            premier = i;
        }
        if premier == 0 {
            return toutes;
        }
        let niveau = toutes[premier - 1].niveau;
        let mut v = vec![Miette { libelle: "…".into(), niveau }];
        v.extend(toutes.into_iter().skip(premier));
        v
    }

    /// Remonte au niveau `n` de la pile (0 : les disques) en un clic sur le fil d'Ariane ; faux si `n` n'est pas au-dessus
    /// du niveau actuel. Le rectangle du dossier qu'on quitte redevient le choisi.
    pub fn aller_niveau(&mut self, n: usize) -> bool {
        self.zone = Zone::Liste;
        if n < self.pile_min() || n >= self.carte_pile.len() {
            return false;
        }
        let quitte = self.carte_pile[n];
        self.carte_pile.truncate(n);
        self.actualiser_carte();
        self.carte_focus = self.carte.iter().position(|c| c.id == Some(quitte)).unwrap_or(0);
        true
    }

    /// Montre la vue `v`. Les deux vues restent au même endroit d'une bascule à l'autre : vers l'arbre, les dossiers qui
    /// mènent à l'élément choisi de la carte s'ouvrent et sa ligne devient la choisie ; vers la carte, elle montre le
    /// dossier de la ligne choisie de l'arbre, cette ligne en rectangle choisi. Les dossiers ouverts de l'arbre ne se
    /// referment jamais. La carte ne repart des disques (ou du disque unique) qu'à sa toute première ouverture.
    pub fn choisir_vue(&mut self, v: Vue) {
        self.zone = Zone::Liste;
        if v == self.vue {
            return;
        }
        let avant = self.vue;
        self.echanger_memo(avant, v);
        self.vue = v;
        if v.est_bilan() {
            // La carte et l'arbre ne bougent pas pendant qu'on regarde un bilan ; `base` dit où revenir
            if !avant.est_bilan() {
                self.base = avant;
            }
            self.a_centrer = false;
            self.preparer_bilan();
            return;
        }
        // Venant d'un bilan, la vue de base d'où l'on était parti compte comme la vue quittée : la retrouver telle quelle
        let provenance = if avant.est_bilan() { self.base } else { avant };
        self.base = v;
        if provenance == v {
            return;
        }
        if v == Vue::Arbre {
            self.arbre_vers_la_carte_position();
            return;
        }
        if self.carte_pile.is_empty() && self.carte.is_empty() && self.focus == 0 {
            // Première ouverture, sans rien choisi dans l'arbre
            self.carte_focus = 0;
            self.actualiser_carte();
            if self.pile_min() == 1 {
                self.descendre(0);
            }
        } else {
            self.carte_vers_la_ligne_choisie();
        }
    }

    /// Arbre : ouvre le chemin de l'élément choisi dans la carte (à défaut, du dossier qu'elle montre) et le choisit
    fn arbre_vers_la_carte_position(&mut self) {
        let cible = self.carte.get(self.carte_focus).and_then(|c| c.id).or_else(|| self.carte_parent());
        let Some(arbre) = self.arbre.as_mut() else { return };
        let Some(id) = cible else { return };
        if let Some(rang) = arbre.reveler(id) {
            self.focus = rang;
            self.a_centrer = true;
        }
    }

    /// Carte : montre le dossier de la ligne choisie de l'arbre, avec cette ligne pour rectangle choisi
    fn carte_vers_la_ligne_choisie(&mut self) {
        let Some(arbre) = self.arbre.as_ref() else { return };
        let Some(ligne) = arbre.lignes().get(self.focus) else {
            self.actualiser_carte();
            return;
        };
        let id = ligne.id;
        let mut pile = arbre.ancetres_de(id);
        // Un disque unique : la carte reste dans le disque, jamais au-dessus
        let min = self.pile_min();
        let mut choisi = Some(id);
        if pile.len() < min {
            pile = vec![id];
            choisi = None;
        }
        self.carte_pile = pile;
        self.actualiser_carte();
        self.carte_focus = choisi.and_then(|c| self.carte.iter().position(|x| x.id == Some(c))).unwrap_or(0);
    }

    /// Centre la ligne choisie de l'arbre dans `hauteur` DIP de liste, lignes de `pas` (juste après une bascule)
    pub fn centrer_ligne(&mut self, pas: f32, hauteur: f32) {
        self.a_centrer = false;
        let n = self.lignes().len();
        let milieu = self.focus as f32 * pas - (hauteur - pas) / 2.0;
        self.defilement = milieu.clamp(0.0, (n as f32 * pas - hauteur).max(0.0));
    }

    /// L'autre vue de base (touche V) ; depuis un bilan, la vue de base qu'on avait quittée
    pub fn basculer(&mut self) {
        let v = match self.vue {
            Vue::Arbre => Vue::Carte,
            Vue::Carte => Vue::Arbre,
            Vue::Types | Vue::Gros | Vue::Age => self.base,
        };
        self.choisir_vue(v);
    }

    /// Descend dans le dossier du rectangle `i` ; faux si ce n'est pas un dossier ou s'il n'a rien à montrer
    pub fn descendre(&mut self, i: usize) -> bool {
        let Some(id) = self.carte.get(i).filter(|c| c.dossier).and_then(|c| c.id) else { return false };
        if !self.arbre.as_ref().is_some_and(|a| !a.cellules(Some(id)).is_empty()) {
            return false;
        }
        self.carte_pile.push(id);
        self.carte_focus = 0;
        self.actualiser_carte();
        true
    }

    /// Remonte d'un dossier ; faux au plus haut. Le rectangle du dossier qu'on quitte redevient le choisi.
    pub fn remonter(&mut self) -> bool {
        if self.carte_pile.len() <= self.pile_min() {
            return false;
        }
        let quitte = self.carte_pile.pop();
        self.actualiser_carte();
        self.carte_focus = self.carte.iter().position(|c| c.id == quitte).unwrap_or(0);
        true
    }

    /// Clic sur le rectangle `i` : il devient le choisi
    pub fn cliquer_cellule(&mut self, i: usize) {
        self.zone = Zone::Liste;
        if i < self.carte.len() {
            self.carte_focus = i;
        }
    }

    /// Les rectangles de la carte dans la zone `z` (même disposition pour le dessin et pour la souris)
    pub fn cases(&self, z: carte::Rect) -> Vec<carte::Rect> {
        let poids: Vec<u64> = self.carte.iter().map(|c| c.taille).collect();
        carte::disposer(&poids, z.x, z.y, z.l, z.h)
    }

    /// Rectangle de la carte sous le point `(x, y)` (DIP de la fenêtre), la carte occupant `z`
    pub fn cellule_sous(&self, z: carte::Rect, x: f32, y: f32) -> Option<usize> {
        if !z.contient(x, y) {
            return None;
        }
        self.cases(z).iter().position(|r| r.contient(x, y))
    }

    /// Chemin du dossier que la carte montre (`None` au niveau des disques) ; le fil d'Ariane le remplace à l'écran
    #[cfg(test)]
    pub fn carte_chemin(&self) -> Option<String> {
        let id = self.carte_parent()?;
        self.arbre.as_ref().map(|a| a.chemin_de(id))
    }

    fn touche_carte(&mut self, t: Touche) -> Action {
        let n = self.carte.len();
        match t {
            Touche::Droite | Touche::Bas => self.carte_focus = (self.carte_focus + 1).min(n.saturating_sub(1)),
            Touche::Gauche | Touche::Haut => self.carte_focus = self.carte_focus.saturating_sub(1),
            Touche::PageBas => self.carte_focus = (self.carte_focus + SAUT_CARTE).min(n.saturating_sub(1)),
            Touche::PageHaut => self.carte_focus = self.carte_focus.saturating_sub(SAUT_CARTE),
            Touche::Debut => self.carte_focus = 0,
            Touche::Fin => self.carte_focus = n.saturating_sub(1),
            Touche::Entree => {
                self.descendre(self.carte_focus);
            }
            Touche::Ouvrir => {
                if let (Some(c), Some(a)) = (self.carte.get(self.carte_focus), self.arbre.as_ref()) {
                    if let (true, Some(id)) = (c.dossier, c.id) {
                        return Action::Ouvrir(a.chemin_de(id));
                    }
                }
            }
            Touche::Retour => {
                if !self.remonter() {
                    return Action::Quitter;
                }
            }
            Touche::Tab | Touche::Basculer | Touche::Vue(_) | Touche::Cocher => {}
        }
        Action::Rien
    }

    /// Le glissement a-t-il encore à bouger ? Sans avancer : la cadence des images (30 ou 60 par seconde) en dépend.
    pub fn en_mouvement(&self) -> bool {
        crate::reglages::animations_actives() && (self.defilement - self.affiche).abs() >= 0.5
    }

    /// Fait avancer le glissement du défilement. Vrai tant qu'il n'a pas fini (la minuterie d'images doit continuer).
    pub fn animer(&mut self) -> bool {
        // Mouvement réduit : la liste saute à sa position, sans image intermédiaire
        if !crate::reglages::animations_actives() {
            self.affiche = self.defilement;
            self.dernier_lissage = None;
            return false;
        }
        let ecart = self.defilement - self.affiche;
        if ecart.abs() < 0.5 {
            self.affiche = self.defilement;
            self.dernier_lissage = None;
            return false;
        }
        // Même courbe que la liste de fichiers (`app::part_lissage`) ; première image d'une salve : un pas de 1/60 s
        let maintenant = std::time::Instant::now();
        let dt = self.dernier_lissage.map_or(1.0 / 60.0, |t| maintenant.duration_since(t).as_secs_f32());
        self.dernier_lissage = Some(maintenant);
        self.affiche = lisser(self.affiche, self.defilement, if dt > 0.05 { 1.0 / 60.0 } else { dt });
        true
    }

    /// Garde la ligne choisie visible dans `hauteur` DIP de liste, lignes de `pas`
    pub fn montrer(&mut self, pas: f32, hauteur: f32) {
        let n = self.lignes().len();
        let (haut, bas) = (self.focus as f32 * pas, (self.focus as f32 + 1.0) * pas);
        if haut < self.defilement {
            self.defilement = haut;
        } else if bas > self.defilement + hauteur {
            self.defilement = bas - hauteur;
        }
        self.defilement = self.defilement.clamp(0.0, (n as f32 * pas - hauteur).max(0.0));
    }

    /// Une touche du clavier. `pas` : hauteur d'une ligne ; `hauteur` : hauteur visible de la liste (DIP).
    pub fn touche(&mut self, t: Touche, pas: f32, hauteur: f32) -> Action {
        if t == Touche::Tab {
            self.zone = if self.zone == Zone::Rail { Zone::Liste } else { Zone::Rail };
            return Action::Rien;
        }
        if t == Touche::Basculer {
            if self.page == PAGE_ANALYSE && self.arbre.is_some() {
                self.basculer();
            }
            return Action::Rien;
        }
        if let Touche::Vue(k) = t {
            if self.page == PAGE_ANALYSE && self.arbre.is_some() {
                if let Some(v) = Vue::depuis_rang(k) {
                    self.zone = Zone::Liste;
                    self.choisir_vue(v);
                }
            }
            return Action::Rien;
        }
        if self.zone == Zone::Rail {
            return self.touche_rail(t);
        }
        if self.page == PAGE_NETTOYAGE {
            return self.touche_nettoyage(t);
        }
        if self.vue == Vue::Carte && self.arbre.is_some() {
            return self.touche_carte(t);
        }
        if self.vue.est_bilan() && self.arbre.is_some() {
            return self.touche_bilan(t, pas, hauteur);
        }
        let Some(arbre) = self.arbre.as_mut() else {
            return if t == Touche::Retour { Action::Quitter } else { Action::Rien };
        };
        let n = arbre.lignes().len();
        if n == 0 {
            return if t == Touche::Retour { Action::Quitter } else { Action::Rien };
        }
        let page = ((hauteur / pas) as usize).max(1);
        let mut action = Action::Rien;
        match t {
            Touche::Haut => self.focus = self.focus.saturating_sub(1),
            Touche::Bas => self.focus = (self.focus + 1).min(n - 1),
            Touche::PageHaut => self.focus = self.focus.saturating_sub(page),
            Touche::PageBas => self.focus = (self.focus + page).min(n - 1),
            Touche::Debut => self.focus = 0,
            Touche::Fin => self.focus = n - 1,
            Touche::Droite => {
                if !arbre.ouvrir(self.focus) {
                    // Déjà ouvert : on entre dans le premier enfant
                    if arbre.lignes().get(self.focus).is_some_and(|l| l.ouvert) && self.focus + 1 < n {
                        self.focus += 1;
                    }
                }
            }
            Touche::Gauche => {
                if !arbre.fermer(self.focus) {
                    if let Some(p) = arbre.parent_de(self.focus) {
                        self.focus = p;
                    }
                }
            }
            Touche::Entree | Touche::Ouvrir => {
                if arbre.lignes().get(self.focus).is_some_and(|l| l.dossier) {
                    if let Some(c) = arbre.chemin(self.focus) {
                        action = Action::Ouvrir(c);
                    }
                }
            }
            Touche::Retour => match arbre.parent_de(self.focus) {
                Some(p) => self.focus = p,
                None => action = Action::Quitter,
            },
            Touche::Tab | Touche::Basculer | Touche::Vue(_) | Touche::Cocher => {}
        }
        // Une fermeture ou une ouverture change la liste : la ligne choisie reste dans ses bornes
        let n = self.arbre.as_ref().map_or(0, |a| a.lignes().len());
        self.focus = self.focus.min(n.saturating_sub(1));
        self.montrer(pas, hauteur);
        action
    }

    /// Dossier que la ligne `i` de la liste ouvre dans l'explorateur : celui d'un dossier de l'arbre, ou le dossier qui
    /// contient un gros fichier. Rien pour un fichier de l'arbre ni pour un groupe (un type, une tranche d'âge : ce sont des
    /// totaux, pas des emplacements).
    pub fn chemin_a_ouvrir(&self, i: usize) -> Option<String> {
        let arbre = self.arbre.as_ref()?;
        let ligne = self.lignes().get(i)?;
        if self.vue.est_bilan() {
            return ligne.fichier.and_then(|id| arbre.dossier_du_fichier(id));
        }
        if ligne.dossier { arbre.chemin(i) } else { None }
    }

    /// Touches d'un bilan : on se déplace dans la liste ; Entrée sur un gros fichier ouvre son dossier.
    fn touche_bilan(&mut self, t: Touche, pas: f32, hauteur: f32) -> Action {
        let n = self.lignes().len();
        if n == 0 {
            return if t == Touche::Retour { Action::Quitter } else { Action::Rien };
        }
        let page = ((hauteur / pas) as usize).max(1);
        let mut action = Action::Rien;
        match t {
            Touche::Haut => self.focus = self.focus.saturating_sub(1),
            Touche::Bas => self.focus = (self.focus + 1).min(n - 1),
            Touche::PageHaut => self.focus = self.focus.saturating_sub(page),
            Touche::PageBas => self.focus = (self.focus + page).min(n - 1),
            Touche::Debut => self.focus = 0,
            Touche::Fin => self.focus = n - 1,
            Touche::Entree | Touche::Ouvrir => {
                if let Some(chemin) = self.chemin_a_ouvrir(self.focus) {
                    action = Action::Ouvrir(chemin);
                }
            }
            Touche::Retour => action = Action::Quitter,
            Touche::Droite | Touche::Gauche | Touche::Tab | Touche::Basculer | Touche::Vue(_) | Touche::Cocher => {}
        }
        self.montrer(pas, hauteur);
        action
    }

    /// Ligne de la liste (arbre ou bilan) sous un point : `dy` est la distance sous le haut de la liste (DIP, défilement non compris)
    pub fn ligne_sous(&self, dy: f32, pas: f32) -> Option<usize> {
        let n = self.lignes().len();
        if dy < 0.0 {
            return None;
        }
        let i = ((dy + self.affiche) / pas) as usize;
        (i < n).then_some(i)
    }

    /// Clic sur la ligne `i` : elle devient la ligne choisie ; sur la flèche d'un dossier, elle l'ouvre ou le ferme
    pub fn cliquer_ligne(&mut self, i: usize, sur_fleche: bool) {
        self.zone = Zone::Liste;
        if i >= self.lignes().len() {
            return;
        }
        self.focus = i;
        // Un bilan n'a ni flèche ni dossier à ouvrir : le clic ne fait que choisir la ligne
        if self.vue.est_bilan() {
            return;
        }
        let Some(arbre) = self.arbre.as_mut() else { return };
        if sur_fleche && arbre.lignes()[i].dossier && !arbre.ouvrir(i) {
            arbre.fermer(i);
        }
        let n = arbre.lignes().len();
        self.focus = self.focus.min(n.saturating_sub(1));
    }

    /// Clic sur une page du rail
    pub fn choisir_page(&mut self, k: usize) {
        if k < PAGES.len() {
            self.page = k;
            self.zone = Zone::Rail;
        }
    }

    fn touche_rail(&mut self, t: Touche) -> Action {
        let n = PAGES.len();
        match t {
            Touche::Haut => self.page = (self.page + n - 1) % n,
            Touche::Bas => self.page = (self.page + 1) % n,
            Touche::Debut => self.page = 0,
            Touche::Fin => self.page = n - 1,
            Touche::Droite | Touche::Entree => self.zone = Zone::Liste,
            Touche::Retour => return Action::Quitter,
            _ => {}
        }
        Action::Rien
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::index::{Index, DOSSIER_RACINE};
    use std::sync::Arc;

    fn etat() -> Etat {
        let mut i = Index::default();
        let c = i.ajouter("C:", DOSSIER_RACINE, true, 0, 0);
        let a = i.ajouter("A", c, true, 0, 0);
        i.ajouter("B", c, true, 0, 0);
        i.ajouter("f.txt", a, false, 10, 0);
        let mut e = Etat::nouveau();
        e.arbre = Some(Arbre::nouveau(Arc::new(i)));
        e
    }

    fn noms(e: &Etat) -> Vec<String> {
        e.arbre.as_ref().unwrap().lignes().iter().map(|l| l.nom.clone()).collect()
    }

    #[test]
    fn fleches_droite_gauche_ouvrent_et_ferment() {
        let mut e = etat();
        assert_eq!(e.touche(Touche::Droite, 36.0, 360.0), Action::Rien);
        assert_eq!(noms(&e), vec!["C:\\", "A", "B"]);
        // Déjà ouvert : Droite entre dans le premier enfant
        e.touche(Touche::Droite, 36.0, 360.0);
        assert_eq!(e.focus, 1);
        e.touche(Touche::Droite, 36.0, 360.0);
        assert_eq!(noms(&e), vec!["C:\\", "A", "f.txt", "B"]);
        // Gauche sur un fichier : remonte à son dossier, puis le ferme, puis remonte au disque
        e.touche(Touche::Bas, 36.0, 360.0);
        assert_eq!(e.focus, 2);
        e.touche(Touche::Gauche, 36.0, 360.0);
        assert_eq!(e.focus, 1);
        e.touche(Touche::Gauche, 36.0, 360.0);
        assert_eq!(noms(&e), vec!["C:\\", "A", "B"]);
        e.touche(Touche::Gauche, 36.0, 360.0);
        assert_eq!(e.focus, 0);
    }

    #[test]
    fn la_souris_choisit_ouvre_et_ferme() {
        let mut e = etat();
        e.defilement = 0.0;
        // Une seule ligne (le disque fermé) : 10 sous le haut la vise, 40 est déjà sous elle
        assert_eq!(e.ligne_sous(10.0, 36.0), Some(0));
        assert_eq!(e.ligne_sous(40.0, 36.0), None);
        assert_eq!(e.ligne_sous(-1.0, 36.0), None);
        assert_eq!(e.ligne_sous(36.0 * 50.0, 36.0), None);
        // Clic ailleurs que sur la flèche : on choisit seulement
        e.cliquer_ligne(0, false);
        assert_eq!(e.focus, 0);
        assert_eq!(noms(&e), vec!["C:\\"]);
        // Clic sur la flèche : ouvre, puis referme
        e.cliquer_ligne(0, true);
        assert_eq!(noms(&e), vec!["C:\\", "A", "B"]);
        e.cliquer_ligne(0, true);
        assert_eq!(noms(&e), vec!["C:\\"]);
        // Une ligne hors bornes ne change rien
        e.cliquer_ligne(9, true);
        assert_eq!(e.focus, 0);
        // Le rail : la page change et la zone devient le rail
        e.choisir_page(2);
        assert_eq!((e.page, e.zone), (2, Zone::Rail));
        e.choisir_page(99);
        assert_eq!(e.page, 2);
    }

    #[test]
    fn haut_bas_restent_dans_la_liste() {
        let mut e = etat();
        e.touche(Touche::Droite, 36.0, 360.0);
        e.touche(Touche::Haut, 36.0, 360.0);
        assert_eq!(e.focus, 0);
        e.touche(Touche::Fin, 36.0, 360.0);
        assert_eq!(e.focus, 2);
        e.touche(Touche::Bas, 36.0, 360.0);
        assert_eq!(e.focus, 2);
        e.touche(Touche::Debut, 36.0, 360.0);
        assert_eq!(e.focus, 0);
    }

    #[test]
    fn entree_ouvre_un_dossier_pas_un_fichier() {
        let mut e = etat();
        assert_eq!(e.touche(Touche::Entree, 36.0, 360.0), Action::Ouvrir("C:\\".into()));
        e.touche(Touche::Droite, 36.0, 360.0);
        e.touche(Touche::Bas, 36.0, 360.0);
        assert_eq!(e.touche(Touche::Entree, 36.0, 360.0), Action::Ouvrir("C:\\A".into()));
        e.touche(Touche::Droite, 36.0, 360.0);
        e.touche(Touche::Bas, 36.0, 360.0);
        assert_eq!(noms(&e)[e.focus], "f.txt");
        assert_eq!(e.touche(Touche::Entree, 36.0, 360.0), Action::Rien);
    }

    #[test]
    fn retour_remonte_puis_quitte() {
        let mut e = etat();
        e.touche(Touche::Droite, 36.0, 360.0);
        e.touche(Touche::Bas, 36.0, 360.0);
        assert_eq!(e.touche(Touche::Retour, 36.0, 360.0), Action::Rien);
        assert_eq!(e.focus, 0);
        assert_eq!(e.touche(Touche::Retour, 36.0, 360.0), Action::Quitter);
    }

    #[test]
    fn tab_passe_du_rail_a_la_liste_et_le_rail_change_de_page() {
        let mut e = etat();
        e.touche(Touche::Tab, 36.0, 360.0);
        assert_eq!(e.zone, Zone::Rail);
        e.touche(Touche::Bas, 36.0, 360.0);
        assert_eq!(e.page, 1);
        e.touche(Touche::Haut, 36.0, 360.0);
        e.touche(Touche::Haut, 36.0, 360.0);
        assert_eq!(e.page, PAGES.len() - 1, "le rail boucle");
        e.touche(Touche::Entree, 36.0, 360.0);
        assert_eq!(e.zone, Zone::Liste);
        e.touche(Touche::Tab, 36.0, 360.0);
        assert_eq!(e.touche(Touche::Retour, 36.0, 360.0), Action::Quitter);
    }

    #[test]
    fn la_ligne_choisie_reste_visible() {
        let mut i = Index::default();
        let c = i.ajouter("C:", DOSSIER_RACINE, true, 0, 0);
        for k in 0..30 {
            i.ajouter(&format!("f{k:02}"), c, false, 100 - k, 0);
        }
        let mut e = Etat::nouveau();
        e.arbre = Some(Arbre::nouveau(Arc::new(i)));
        e.touche(Touche::Droite, 36.0, 144.0); // 4 lignes visibles
        e.touche(Touche::PageBas, 36.0, 144.0);
        assert_eq!(e.focus, 4);
        assert!(e.defilement > 0.0);
        let (haut, bas) = (e.focus as f32 * 36.0, (e.focus as f32 + 1.0) * 36.0);
        assert!(haut >= e.defilement && bas <= e.defilement + 144.0);
        e.touche(Touche::Fin, 36.0, 144.0);
        assert_eq!(e.focus, 30);
        assert_eq!(e.defilement, 31.0 * 36.0 - 144.0, "jamais au-delà de la dernière ligne");
        e.touche(Touche::Debut, 36.0, 144.0);
        assert_eq!(e.defilement, 0.0);
    }

    #[test]
    fn le_dessin_rejoint_la_position_voulue_sans_la_depasser() {
        // 60 images par seconde : la distance restante baisse à chaque image, sans jamais dépasser la cible
        let (mut x, mut images) = (0.0_f32, 0);
        while (300.0 - x).abs() >= 0.5 {
            let suivant = lisser(x, 300.0, 1.0 / 60.0);
            assert!(suivant > x && suivant < 300.0, "il approche sans dépasser : {x} -> {suivant}");
            x = suivant;
            images += 1;
            assert!(images < 600, "le glissement doit finir en moins de 10 s");
        }
        assert!(images > 5, "il glisse, il ne saute pas : {images} images");
        // Au repos (écart nul), `animer` rend la main à la minuterie
        let mut e = Etat::nouveau();
        assert!(!e.animer());
    }

    /// C: ─ A (15) ─ un.txt 10, S (5) ─ deux.txt 5 ; B (7) ─ trois.txt 7 ; et, si `deux_disques`, D: ─ x.bin 3
    fn etat_carte(deux_disques: bool) -> Etat {
        let mut i = Index::default();
        let c = i.ajouter("C:", DOSSIER_RACINE, true, 0, 0);
        let b = i.ajouter("B", c, true, 0, 0);
        let a = i.ajouter("A", c, true, 0, 0);
        i.ajouter("trois.txt", b, false, 7, 0);
        i.ajouter("un.txt", a, false, 10, 0);
        let s = i.ajouter("S", a, true, 0, 0);
        i.ajouter("deux.txt", s, false, 5, 0);
        if deux_disques {
            let d = i.ajouter("D:", DOSSIER_RACINE, true, 0, 0);
            i.ajouter("x.bin", d, false, 3, 0);
        }
        let mut e = Etat::nouveau();
        e.arbre = Some(Arbre::nouveau(Arc::new(i)));
        e
    }

    fn noms_carte(e: &Etat) -> Vec<&str> {
        e.carte.iter().map(|c| c.nom.as_str()).collect()
    }

    #[test]
    fn la_carte_descend_remonte_et_quitte_en_haut() {
        let mut e = etat_carte(false);
        assert_eq!(e.vue, Vue::Arbre);
        e.touche(Touche::Basculer, 36.0, 360.0);
        // Un seul disque : la carte s'ouvre dedans
        assert_eq!((e.vue, e.carte_pile.len()), (Vue::Carte, 1));
        assert_eq!(noms_carte(&e), vec!["A", "B"]);
        e.touche(Touche::Bas, 36.0, 360.0);
        assert_eq!(e.carte_focus, 1);
        e.touche(Touche::Bas, 36.0, 360.0);
        assert_eq!(e.carte_focus, 1, "pas au-delà du dernier");
        e.touche(Touche::Debut, 36.0, 360.0);
        assert_eq!(e.touche(Touche::Ouvrir, 36.0, 360.0), Action::Ouvrir("C:\\A".into()));
        // Entrée descend dans A : un.txt (fichier) puis S
        e.touche(Touche::Entree, 36.0, 360.0);
        assert_eq!(noms_carte(&e), vec!["un.txt", "S"]);
        assert_eq!(e.carte_chemin(), Some("C:\\A".into()));
        // Entrée sur un fichier ne fait rien ; Ouvrir non plus
        assert_eq!(e.touche(Touche::Entree, 36.0, 360.0), Action::Rien);
        assert_eq!(e.touche(Touche::Ouvrir, 36.0, 360.0), Action::Rien);
        assert_eq!(e.carte_pile.len(), 2);
        e.touche(Touche::Droite, 36.0, 360.0);
        e.touche(Touche::Entree, 36.0, 360.0);
        assert_eq!(noms_carte(&e), vec!["deux.txt"]);
        // Retour remonte, en re-choisissant le dossier qu'on quitte
        assert_eq!(e.touche(Touche::Retour, 36.0, 360.0), Action::Rien);
        assert_eq!((noms_carte(&e), e.carte_focus), (vec!["un.txt", "S"], 1));
        assert_eq!(e.touche(Touche::Retour, 36.0, 360.0), Action::Rien);
        assert_eq!((noms_carte(&e), e.carte_focus), (vec!["A", "B"], 0));
        // Au plus haut (le disque unique), Retour quitte la page
        assert_eq!(e.touche(Touche::Retour, 36.0, 360.0), Action::Quitter);
    }

    #[test]
    fn la_carte_avec_deux_disques_commence_aux_disques() {
        let mut e = etat_carte(true);
        e.choisir_vue(Vue::Carte);
        assert_eq!((e.carte_pile.len(), noms_carte(&e)), (0, vec!["C:\\", "D:\\"]));
        assert_eq!(e.carte_chemin(), None);
        e.touche(Touche::Entree, 36.0, 360.0);
        assert_eq!(noms_carte(&e), vec!["A", "B"]);
        assert_eq!(e.touche(Touche::Retour, 36.0, 360.0), Action::Rien);
        assert_eq!(e.carte_pile.len(), 0);
        assert_eq!(e.touche(Touche::Retour, 36.0, 360.0), Action::Quitter);
    }

    #[test]
    fn basculer_garde_l_arbre_intact_et_la_souris_vise_les_rectangles() {
        let mut e = etat_carte(true);
        e.touche(Touche::Droite, 36.0, 360.0);
        let avant = noms(&e);
        e.touche(Touche::Basculer, 36.0, 360.0);
        e.touche(Touche::Basculer, 36.0, 360.0);
        assert_eq!((e.vue, noms(&e)), (Vue::Arbre, avant));
        // Dans l'arbre, les touches agissent sur l'arbre
        e.touche(Touche::Bas, 36.0, 360.0);
        assert_eq!(e.focus, 1);
        // Souris : un clic choisit le rectangle, hors carte rien n'est visé
        e.choisir_vue(Vue::Carte);
        assert_eq!(noms_carte(&e), vec!["A", "B"], "la carte s'ouvre dans le dossier de la ligne choisie de l'arbre");
        assert!(e.aller_niveau(0));
        let zone = carte::Rect { x: 100.0, y: 50.0, l: 400.0, h: 200.0 };
        let cases = e.cases(zone);
        assert_eq!(cases.len(), e.carte.len());
        let r = cases[1];
        assert_eq!(e.cellule_sous(zone, r.x + r.l / 2.0, r.y + r.h / 2.0), Some(1));
        assert_eq!(e.cellule_sous(zone, 10.0, 10.0), None);
        e.cliquer_cellule(1);
        assert_eq!(e.carte_focus, 1);
        e.cliquer_cellule(99);
        assert_eq!(e.carte_focus, 1, "hors bornes : rien ne change");
        assert!(e.descendre(1), "double-clic sur D: : il contient x.bin");
        assert_eq!(noms_carte(&e), vec!["x.bin"]);
        assert!(!e.descendre(0), "un fichier n'a rien dessous");
    }

    #[test]
    fn les_deux_vues_restent_au_meme_endroit_quand_on_bascule() {
        let mut e = etat_carte(true);
        // Carte : on descend dans C: puis A, sur le rectangle S (dossier)
        e.choisir_vue(Vue::Carte);
        e.touche(Touche::Entree, 36.0, 360.0);
        e.touche(Touche::Entree, 36.0, 360.0);
        e.touche(Touche::Bas, 36.0, 360.0);
        let (pile, focus_carte, rectangles) = (e.carte_pile.clone(), e.carte_focus, e.carte.len());
        assert_eq!((pile.len(), noms_carte(&e)), (2, vec!["un.txt", "S"]));
        // Vers l'arbre : C: et A s'ouvrent, S est la ligne choisie et sera centrée
        e.choisir_vue(Vue::Arbre);
        assert_eq!(noms(&e), vec!["C:\\", "A", "un.txt", "S", "B", "D:\\"]);
        assert_eq!(e.focus, 3);
        assert!(e.a_centrer);
        e.centrer_ligne(36.0, 72.0);
        assert!(!e.a_centrer);
        assert_eq!(e.defilement, 90.0, "ligne 3 au milieu de 72 DIP : 3 × 36 − (72 − 36) / 2");
        // Retour à la carte : même dossier, même rectangle choisi
        e.choisir_vue(Vue::Carte);
        assert_eq!((e.carte_pile.clone(), e.carte_focus, e.carte.len()), (pile, focus_carte, rectangles));
        assert_eq!(e.carte_apercus.len(), e.carte.len());
        // On se déplace dans l'arbre : la carte suit (B, dans C:)
        e.choisir_vue(Vue::Arbre);
        e.touche(Touche::Bas, 36.0, 360.0);
        assert_eq!(e.focus, 4);
        e.choisir_vue(Vue::Carte);
        assert_eq!((noms_carte(&e), e.carte_focus), (vec!["A", "B"], 1));
        // Un disque choisi dans l'arbre : la carte montre les disques, ce disque choisi
        e.choisir_vue(Vue::Arbre);
        e.touche(Touche::Fin, 36.0, 360.0);
        e.choisir_vue(Vue::Carte);
        assert_eq!((noms_carte(&e), e.carte_focus), (vec!["C:\\", "D:\\"], 1));
        // Un seul disque : la carte laissée dans un sous-dossier y reste aussi
        let mut u = etat_carte(false);
        u.choisir_vue(Vue::Carte);
        u.touche(Touche::Entree, 36.0, 360.0);
        let dedans = u.carte_pile.clone();
        u.choisir_vue(Vue::Arbre);
        u.choisir_vue(Vue::Carte);
        assert_eq!(u.carte_pile, dedans);
    }

    #[test]
    fn le_segment_met_la_carte_a_gauche() {
        assert_eq!(VUES, ["Carte", "Arbre", "Types", "Gros fichiers", "Âge"]);
        for (k, v) in [Vue::Carte, Vue::Arbre, Vue::Types, Vue::Gros, Vue::Age].into_iter().enumerate() {
            assert_eq!(v.rang(), k);
            assert_eq!(Vue::depuis_rang(k), Some(v));
        }
        assert_eq!(Vue::depuis_rang(5), None);
    }

    /// FILETIME d'une date unix (secondes)
    fn filetime(unix: u64) -> u64 {
        (unix + 11_644_473_600) * 10_000_000
    }

    const JOUR: u64 = 86_400;
    const NOW: u64 = 1_800_000_000;

    /// C: ─ docs ─ rapport.pdf 40 (hier), photo.jpg 300 (il y a 100 j) ; film.mkv 5000 (il y a 3 ans) ; vieux.zip 70 (date inconnue)
    fn etat_bilans() -> Etat {
        let mut i = Index::default();
        let c = i.ajouter("C:", DOSSIER_RACINE, true, 0, 0);
        let d = i.ajouter("docs", c, true, 0, 0);
        i.ajouter("rapport.pdf", d, false, 40, filetime(NOW - JOUR));
        i.ajouter("photo.jpg", d, false, 300, filetime(NOW - 100 * JOUR));
        i.ajouter("film.mkv", c, false, 5000, filetime(NOW - 3 * 365 * JOUR));
        i.ajouter("vieux.zip", c, false, 70, 0);
        let mut e = Etat::nouveau();
        e.arbre = Some(Arbre::nouveau(Arc::new(i)));
        e.arbre.as_mut().unwrap().preparer_bilans(NOW);
        e
    }

    #[test]
    fn les_tranches_d_age_ont_les_bonnes_bornes() {
        use analyse::tranche_de;
        assert_eq!(tranche_de(filetime(NOW), NOW), 0, "modifié maintenant");
        assert_eq!(tranche_de(filetime(NOW - 29 * JOUR), NOW), 0);
        assert_eq!(tranche_de(filetime(NOW - 30 * JOUR), NOW), 1, "30 jours : plus d'un mois");
        assert_eq!(tranche_de(filetime(NOW - 181 * JOUR), NOW), 1);
        assert_eq!(tranche_de(filetime(NOW - 182 * JOUR), NOW), 2);
        assert_eq!(tranche_de(filetime(NOW - 365 * JOUR), NOW), 3);
        assert_eq!(tranche_de(filetime(NOW - 730 * JOUR), NOW), 4, "2 ans et plus");
        assert_eq!(tranche_de(filetime(NOW + 5 * JOUR), NOW), 0, "une date dans le futur ne plante pas");
        assert_eq!(tranche_de(0, NOW), 5, "date inconnue");
    }

    #[test]
    fn les_types_regroupent_par_genre_du_plus_lourd_au_plus_leger() {
        let e = etat_bilans();
        let b = e.arbre.as_ref().unwrap().bilans().unwrap();
        let types: Vec<(&str, u64)> = b.types.iter().map(|l| (l.nom.as_str(), l.taille)).collect();
        assert_eq!(types.len(), 4, "seuls les genres présents : {types:?}");
        assert_eq!(types[0].1, 5000, "la vidéo est la plus lourde");
        assert!(types.windows(2).all(|w| w[0].1 >= w[1].1), "ordre décroissant : {types:?}");
        assert_eq!(b.types.iter().map(|l| l.taille).sum::<u64>(), 5410, "tout fichier est compté une fois");
        assert!(b.types.iter().all(|l| l.pastille.is_some() && l.fichier.is_none()));
        assert_eq!(b.types[0].detail, "1 fichier");
    }

    #[test]
    fn les_gros_fichiers_sont_triés_et_disent_leur_dossier() {
        let e = etat_bilans();
        let b = e.arbre.as_ref().unwrap().bilans().unwrap();
        let noms: Vec<&str> = b.gros.iter().map(|l| l.nom.as_str()).collect();
        assert_eq!(noms, ["film.mkv", "photo.jpg", "vieux.zip", "rapport.pdf"]);
        assert_eq!(b.gros[1].detail, "C:\\docs", "le dossier qui contient le fichier");
        assert!(b.gros.iter().all(|l| l.fichier.is_some() && !l.dossier));
    }

    #[test]
    fn les_gros_fichiers_s_arretent_a_cent() {
        let mut i = Index::default();
        let c = i.ajouter("C:", DOSSIER_RACINE, true, 0, 0);
        for k in 1..=250u64 {
            i.ajouter(&format!("f{k}.bin"), c, false, k, 0);
        }
        let mut a = Arbre::nouveau(Arc::new(i));
        a.preparer_bilans(NOW);
        let gros = &a.bilans().unwrap().gros;
        assert_eq!(gros.len(), analyse::GROS_MAX);
        assert_eq!(gros[0].taille, 250);
        assert_eq!(gros.last().unwrap().taille, 151, "les 100 plus gros : 250 à 151");
    }

    #[test]
    fn l_age_range_les_fichiers_du_plus_recent_au_plus_ancien() {
        let e = etat_bilans();
        let b = e.arbre.as_ref().unwrap().bilans().unwrap();
        let ages: Vec<(&str, u64)> = b.ages.iter().map(|l| (l.nom.as_str(), l.taille)).collect();
        assert_eq!(ages, [("Moins d'un mois", 40), ("1 à 6 mois", 300), ("Plus de 2 ans", 5000), ("Date inconnue", 70)]);
        assert!(b.ages.iter().all(|l| l.pastille.is_none()));
    }

    /// Mesure E3 (lancer avec `--release --ignored --nocapture`) : coût des trois bilans sur un index de 2,8 millions de
    /// fichiers (l'ordre de grandeur de l'index réel de l'auteur), au premier passage dans un bilan
    #[test]
    #[ignore]
    fn mesure_bilans_sur_2_8_millions_de_fichiers() {
        let mut i = Index::default();
        let c = i.ajouter("C:", DOSSIER_RACINE, true, 0, 0);
        let exts = ["rs", "png", "mkv", "txt", "zip", "dll", "js", "pdf", "bin", "log"];
        let dossiers: Vec<u32> = (0..2000).map(|k| i.ajouter(&format!("dossier{k}"), c, true, 0, 0)).collect();
        let mut graine = 12345u64;
        for k in 0..2_800_000u64 {
            graine = graine.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            let taille = (graine >> 40) % 5_000_000;
            let modif = filetime(NOW - (graine >> 20) % (900 * JOUR));
            i.ajouter(&format!("fichier{k}.{}", exts[(graine >> 33) as usize % exts.len()]), dossiers[(k % 2000) as usize], false, taille, modif);
        }
        let mut a = Arbre::nouveau(Arc::new(i));
        let t = std::time::Instant::now();
        a.preparer_bilans(NOW);
        let ms = t.elapsed().as_millis();
        let b = a.bilans().unwrap();
        println!("MESURE E3 : bilans de 2,8 M de fichiers en {ms} ms ({} types, {} gros, {} âges)", b.types.len(), b.gros.len(), b.ages.len());
    }

    #[test]
    fn un_index_sans_fichier_donne_des_bilans_vides() {
        let mut i = Index::default();
        i.ajouter("C:", DOSSIER_RACINE, true, 0, 0);
        let mut a = Arbre::nouveau(Arc::new(i));
        a.preparer_bilans(NOW);
        let b = a.bilans().unwrap();
        assert!(b.types.is_empty() && b.gros.is_empty() && b.ages.is_empty());
    }

    #[test]
    fn un_bilan_se_navigue_et_chaque_liste_garde_sa_place() {
        let mut e = etat_bilans();
        e.choisir_vue(Vue::Arbre);
        e.touche(Touche::Bas, 36.0, 360.0);
        let focus_arbre = e.focus;
        e.choisir_vue(Vue::Gros);
        assert_eq!(e.focus, 0, "une liste neuve commence en haut");
        assert_eq!(e.lignes().len(), 4);
        e.touche(Touche::Bas, 36.0, 360.0);
        e.touche(Touche::Bas, 36.0, 360.0);
        assert_eq!(e.focus, 2);
        e.touche(Touche::Fin, 36.0, 360.0);
        assert_eq!(e.focus, 3);
        e.touche(Touche::Bas, 36.0, 360.0);
        assert_eq!(e.focus, 3, "ne dépasse pas la dernière ligne");
        e.choisir_vue(Vue::Types);
        assert_eq!(e.focus, 0);
        e.choisir_vue(Vue::Gros);
        assert_eq!(e.focus, 3, "les gros fichiers retrouvent leur ligne");
        e.choisir_vue(Vue::Arbre);
        assert_eq!(e.focus, focus_arbre, "l'arbre retrouve la sienne, intacte");
    }

    #[test]
    fn les_touches_1_a_5_choisissent_la_vue() {
        let mut e = etat_bilans();
        e.choisir_vue(Vue::Carte);
        for (k, v) in [Vue::Carte, Vue::Arbre, Vue::Types, Vue::Gros, Vue::Age].into_iter().enumerate() {
            assert_eq!(e.touche(Touche::Vue(k), 36.0, 360.0), Action::Rien);
            assert_eq!(e.vue, v);
        }
        e.touche(Touche::Vue(9), 36.0, 360.0);
        assert_eq!(e.vue, Vue::Age, "un rang inconnu ne change rien");
    }

    #[test]
    fn quitter_un_bilan_revient_a_la_vue_de_base() {
        let mut e = etat_bilans();
        e.choisir_vue(Vue::Carte);
        e.choisir_vue(Vue::Age);
        e.basculer();
        assert_eq!(e.vue, Vue::Carte, "la bascule rend la vue de base");
        e.choisir_vue(Vue::Arbre);
        e.choisir_vue(Vue::Types);
        e.basculer();
        assert_eq!(e.vue, Vue::Arbre);
    }

    #[test]
    fn entree_sur_un_gros_fichier_ouvre_son_dossier_pas_sur_un_groupe() {
        let mut e = etat_bilans();
        e.choisir_vue(Vue::Gros);
        e.touche(Touche::Bas, 36.0, 360.0); // photo.jpg, dans C:\docs
        assert_eq!(e.touche(Touche::Entree, 36.0, 360.0), Action::Ouvrir("C:\\docs".into()));
        assert_eq!(e.chemin_a_ouvrir(1), Some("C:\\docs".into()), "le double-clic fait pareil");
        e.choisir_vue(Vue::Types);
        assert_eq!(e.touche(Touche::Entree, 36.0, 360.0), Action::Rien);
        assert_eq!(e.chemin_a_ouvrir(0), None);
        e.choisir_vue(Vue::Age);
        assert_eq!(e.touche(Touche::Entree, 36.0, 360.0), Action::Rien);
        assert_eq!(e.touche(Touche::Retour, 36.0, 360.0), Action::Quitter);
    }

    #[test]
    fn le_genre_se_lit_a_l_extension_sans_la_casse() {
        use analyse::Genre;
        assert_eq!(Genre::de("photo.PNG", false), Genre::Image);
        assert_eq!(Genre::de("film.mkv", false), Genre::Video);
        assert_eq!(Genre::de("main.rs", false), Genre::Code);
        assert_eq!(Genre::de("sauvegarde.zip", false), Genre::Archive);
        assert_eq!(Genre::de("setup.exe", false), Genre::Programme);
        assert_eq!(Genre::de("notes.txt", false), Genre::Document);
        assert_eq!(Genre::de("inconnu.zzz", false), Genre::Autre);
        assert_eq!(Genre::de("sans_extension", false), Genre::Autre);
        assert_eq!(Genre::de("dossier.png", true), Genre::Dossier, "un dossier reste un dossier, quel que soit son nom");
        assert_eq!(Genre::de("a.TarGzXyzAbc", false), Genre::Autre, "une extension de plus de 8 octets n'est pas un genre connu");
        assert_eq!(Genre::de("é.PNG", false), Genre::Image);
    }

    #[test]
    fn le_fil_d_ariane_suit_la_pile_et_aller_niveau_remonte() {
        // Deux disques : « Tous les disques » est la première pastille
        let mut e = etat_carte(true);
        e.choisir_vue(Vue::Carte);
        let m = e.miettes(2000.0);
        assert_eq!(m.iter().map(|x| (x.libelle.as_str(), x.niveau)).collect::<Vec<_>>(), vec![("Tous les disques", 0)]);
        e.touche(Touche::Entree, 36.0, 360.0); // dans C:
        e.touche(Touche::Entree, 36.0, 360.0); // dans A
        let m = e.miettes(2000.0);
        assert_eq!(m.iter().map(|x| x.niveau).collect::<Vec<_>>(), vec![0, 1, 2]);
        assert_eq!(m.last().unwrap().libelle, "A");
        // Un clic sur la pastille du disque remonte, en re-choisissant A
        assert!(e.aller_niveau(1));
        assert_eq!((e.carte_pile.len(), noms_carte(&e), e.carte_focus), (1, vec!["A", "B"], 0));
        // Le niveau actuel et au-delà ne font rien ; « Tous les disques » (0) remonte aux disques
        assert!(!e.aller_niveau(1));
        assert!(!e.aller_niveau(7));
        assert!(e.aller_niveau(0));
        assert_eq!(noms_carte(&e), vec!["C:\\", "D:\\"]);
        // Un seul disque : pas de « Tous les disques », et le niveau 0 est refusé (la carte ne peut pas être plus haute)
        let mut u = etat_carte(false);
        u.choisir_vue(Vue::Carte);
        u.touche(Touche::Entree, 36.0, 360.0);
        assert_eq!(u.miettes(2000.0).iter().map(|x| x.niveau).collect::<Vec<_>>(), vec![1, 2]);
        assert!(!u.aller_niveau(0));
    }

    #[test]
    fn un_fil_d_ariane_trop_long_replie_les_premiers_en_points() {
        let mut i = Index::default();
        let mut parent = i.ajouter("C:", DOSSIER_RACINE, true, 0, 0);
        for k in 0..6 {
            parent = i.ajouter(&format!("niveau-{k}"), parent, true, 0, 0);
        }
        i.ajouter("f.txt", parent, false, 10, 0);
        let mut e = Etat::nouveau();
        e.arbre = Some(Arbre::nouveau(Arc::new(i)));
        e.choisir_vue(Vue::Carte);
        for _ in 0..6 {
            e.touche(Touche::Entree, 36.0, 360.0);
        }
        assert_eq!(e.carte_pile.len(), 7);
        let large = e.miettes(5000.0);
        assert_eq!(large.len(), 7, "tout tient : aucune pastille repliée");
        let etroit = e.miettes(300.0);
        assert!(etroit.len() < 7 && etroit[0].libelle == "…", "les premiers se replient : {etroit:?}");
        assert_eq!(etroit.last().unwrap().niveau, 7, "l'endroit où l'on est reste toujours montré");
        // « … » remonte juste au-dessus du premier niveau montré
        assert_eq!(etroit[0].niveau + 1, etroit[1].niveau);
    }

    #[test]
    fn l_apercu_d_un_dossier_garde_les_plus_gros_elements() {
        let e = etat_carte(false);
        let a = e.arbre.as_ref().unwrap();
        let c = a.cellules(None)[0].id.unwrap();
        let dossiers = a.apercu(c, 1);
        assert_eq!(dossiers.len(), 1);
        assert_eq!(dossiers[0].nom, "A", "A (15) pèse plus que B (7)");
        assert!(a.apercu(c, 9).iter().all(|x| x.id.is_some()), "jamais le regroupement « autres éléments »");
    }

    #[test]
    fn sans_index_basculer_ne_fait_rien() {
        let mut e = Etat::nouveau();
        e.touche(Touche::Basculer, 36.0, 360.0);
        assert_eq!(e.vue, Vue::Arbre);
    }

    #[test]
    fn sans_index_seul_retour_agit() {
        let mut e = Etat::nouveau();
        assert_eq!(e.touche(Touche::Bas, 36.0, 360.0), Action::Rien);
        assert_eq!(e.touche(Touche::Retour, 36.0, 360.0), Action::Quitter);
    }

    // ───── Page NETTOYAGE : cocher, confirmer, annuler (E4b) ─────

    fn cat(id: &'static str, octets: u64) -> nettoyage::Categorie {
        nettoyage::Categorie {
            id,
            libelle: "Essai",
            risque: "",
            mode: nettoyage::Mode::Direct,
            octets,
            fichiers: 1,
            elements: 1,
            illisibles: 0,
            anormal: false,
            chemins: Vec::new(),
        }
    }

    /// Une page NETTOYAGE dont la mesure est revenue avec deux catégories
    fn page_nettoyage() -> Etat {
        let mut e = Etat::nouveau();
        e.page = PAGE_NETTOYAGE;
        assert!(e.nettoyage_a_lancer());
        e.recevoir_nettoyage(vec![cat("d3d", 5), cat("temp", 3)]);
        e
    }

    fn t(e: &mut Etat, k: Touche) -> Action {
        e.touche(k, 36.0, 360.0)
    }

    #[test]
    fn rien_n_est_coche_par_defaut_et_entree_sans_coche_ne_supprime_rien() {
        let mut e = page_nettoyage();
        assert!(e.net_cochees.is_empty());
        assert_eq!(t(&mut e, Touche::Entree), Action::Rien);
        assert_eq!(e.net_etape, EtapeNet::Liste);
        assert!(e.net_message.is_some() && e.nettoyage.is_some(), "pas de nouvelle mesure pour rien");
    }

    #[test]
    fn espace_coche_et_decoche_la_categorie_choisie() {
        let mut e = page_nettoyage();
        t(&mut e, Touche::Cocher);
        assert_eq!(e.net_cochees, vec!["d3d"]);
        t(&mut e, Touche::Bas);
        t(&mut e, Touche::Cocher);
        assert_eq!(e.net_cochees, vec!["d3d", "temp"]);
        t(&mut e, Touche::Cocher);
        assert_eq!(e.net_cochees, vec!["d3d"]);
    }

    #[test]
    fn entree_remesure_puis_demande_confirmation_avec_la_liste_fraiche() {
        let mut e = page_nettoyage();
        t(&mut e, Touche::Cocher);
        assert_eq!(t(&mut e, Touche::Entree), Action::Rien);
        // Préparation : la mesure de l'ouverture est jetée, une nouvelle part ; aucune touche ne supprime rien
        assert_eq!(e.net_etape, EtapeNet::Preparation);
        assert!(e.nettoyage.is_none() && e.nettoyage_a_lancer());
        assert_eq!(t(&mut e, Touche::Entree), Action::Rien);
        // La mesure revient (la catégorie a grossi) : la confirmation montre CETTE mesure, pas l'ancienne
        e.recevoir_nettoyage(vec![cat("d3d", 9), cat("temp", 3)]);
        match &e.net_etape {
            EtapeNet::Confirmation(v) => assert_eq!((v.len(), v[0].octets), (1, 9)),
            autre => panic!("{autre:?}"),
        }
    }

    /// Une page NETTOYAGE avec `n` catégories (la souris et la molette ont besoin d'une liste plus longue que la fenêtre)
    fn page_nettoyage_n(n: usize) -> Etat {
        const IDS: [&str; 6] = ["a", "b", "c", "d", "e", "f"];
        let mut e = Etat::nouveau();
        e.page = PAGE_NETTOYAGE;
        assert!(e.nettoyage_a_lancer());
        e.recevoir_nettoyage((0..n).map(|k| cat(IDS[k], 10 - k as u64)).collect());
        e
    }

    #[test]
    fn un_clic_sur_une_ligne_la_choisit_sans_la_cocher_et_sa_case_la_coche() {
        let mut e = page_nettoyage();
        e.net_cliquer(1, false, 0);
        assert_eq!((e.net_focus, e.net_cochees.is_empty()), (1, true));
        // La case coche, puis décoche au clic suivant
        e.net_cliquer(0, true, 0);
        assert_eq!((e.net_focus, e.net_cochees.clone()), (0, vec!["d3d"]));
        e.net_cliquer(0, true, 0);
        assert!(e.net_cochees.is_empty());
        // Une ligne qui n'existe pas ne change rien
        e.net_cliquer(7, true, 0);
        assert_eq!((e.net_focus, e.net_cochees.is_empty()), (0, true));
    }

    #[test]
    fn un_clic_fige_le_debut_de_la_liste_et_une_touche_la_fait_suivre_le_choix() {
        let mut e = page_nettoyage_n(6);
        e.net_cliquer(4, false, 2);
        assert_eq!((e.net_focus, e.net_debut), (4, Some(2)));
        // Une touche rend la main au clavier : la liste suit de nouveau la ligne choisie
        t(&mut e, Touche::Bas);
        assert_eq!((e.net_focus, e.net_debut), (5, None));
    }

    #[test]
    fn la_molette_deplace_le_debut_sans_toucher_au_choix_et_s_arrete_aux_butees() {
        let mut e = page_nettoyage_n(6);
        e.net_focus = 3;
        // Quatre lignes visibles sur six : le début va de 0 à 2 ; un cran (1,0) vers le bas descend de deux lignes
        e.molette_nettoyage(-1.0, 0, 4);
        assert_eq!((e.net_debut, e.net_focus), (Some(2), 3));
        // Butée basse : pas plus loin, et le reste ne s'accumule pas
        e.molette_nettoyage(-1.0, 2, 4);
        assert_eq!(e.net_debut, Some(2));
        e.molette_nettoyage(1.0, 2, 4);
        assert_eq!(e.net_debut, Some(0));
        // Pavé tactile : deux petits crans font une ligne
        e.molette_nettoyage(-0.25, 0, 4);
        assert_eq!(e.net_debut, Some(0));
        e.molette_nettoyage(-0.25, 0, 4);
        assert_eq!(e.net_debut, Some(1));
    }

    #[test]
    fn la_molette_et_les_clics_sont_ignores_pendant_la_confirmation() {
        let mut e = page_nettoyage();
        t(&mut e, Touche::Cocher);
        t(&mut e, Touche::Entree);
        e.nettoyage_a_lancer();
        e.recevoir_nettoyage(vec![cat("d3d", 5), cat("temp", 3)]);
        assert!(matches!(e.net_etape, EtapeNet::Confirmation(_)));
        e.molette_nettoyage(-1.0, 0, 1);
        e.net_cliquer(1, true, 0);
        assert_eq!((e.net_debut, e.net_focus, e.net_cochees.clone()), (None, 0, vec!["d3d"]));
    }

    #[test]
    fn les_boutons_font_ce_que_les_touches_font_avec_la_meme_confirmation() {
        let mut e = page_nettoyage();
        // SUPPRIMER sans coche : le même refus que la touche Entrée, rien ne part
        assert_eq!(e.net_bouton(NET_SUPPRIMER), Action::Rien);
        assert!(e.net_message.is_some() && e.net_etape == EtapeNet::Liste);
        // CONFIRMER et ANNULER n'existent pas sur la liste : ils ne font rien
        assert_eq!(e.net_bouton(NET_CONFIRMER), Action::Rien);
        assert_eq!(e.net_bouton(NET_ANNULER), Action::Rien);
        assert_eq!(e.net_etape, EtapeNet::Liste);
        // Avec une coche, SUPPRIMER prépare la confirmation (jamais l'effacement directement)
        e.net_cliquer(0, true, 0);
        assert_eq!(e.net_bouton(NET_SUPPRIMER), Action::Rien);
        assert_eq!(e.net_etape, EtapeNet::Preparation);
        e.nettoyage_a_lancer();
        e.recevoir_nettoyage(vec![cat("d3d", 5), cat("temp", 3)]);
        // ANNULER revient à la liste, la coche est gardée
        assert_eq!(e.net_bouton(NET_ANNULER), Action::Rien);
        assert_eq!((e.net_etape.clone(), e.net_cochees.clone()), (EtapeNet::Liste, vec!["d3d"]));
        // Seul CONFIRMER, dans la confirmation, efface la sélection exacte
        e.net_bouton(NET_SUPPRIMER);
        e.nettoyage_a_lancer();
        e.recevoir_nettoyage(vec![cat("d3d", 5), cat("temp", 3)]);
        let Action::Effacer(v) = e.net_bouton(NET_CONFIRMER) else { panic!("CONFIRMER doit effacer") };
        assert_eq!(v.iter().map(|k| k.id).collect::<Vec<_>>(), vec!["d3d"]);
        assert_eq!(e.net_etape, EtapeNet::Suppression);
        // Pendant la suppression, plus aucun bouton ne répond
        assert_eq!(e.net_bouton(NET_CONFIRMER), Action::Rien);
        assert_eq!(e.net_bouton(NET_SUPPRIMER), Action::Rien);
    }

    #[test]
    fn entree_en_confirmation_efface_et_echap_annule() {
        let mut e = page_nettoyage();
        t(&mut e, Touche::Cocher);
        t(&mut e, Touche::Entree);
        e.nettoyage_a_lancer();
        e.recevoir_nettoyage(vec![cat("d3d", 5), cat("temp", 3)]);
        // Échap annule : la page reste, rien n'est effacé, la coche est gardée
        assert!(e.echap_nettoyage());
        assert_eq!(e.net_etape, EtapeNet::Liste);
        assert_eq!(e.net_cochees, vec!["d3d"]);
        assert!(!e.echap_nettoyage(), "sans confirmation, Échap quitte la page");
        // Seconde tentative : Entrée dans la confirmation lance l'effacement de la sélection exacte
        t(&mut e, Touche::Entree);
        e.nettoyage_a_lancer();
        e.recevoir_nettoyage(vec![cat("d3d", 5), cat("temp", 3)]);
        // Une touche étrangère (flèche, Espace) ne confirme jamais
        assert_eq!(t(&mut e, Touche::Bas), Action::Rien);
        assert_eq!(t(&mut e, Touche::Cocher), Action::Rien);
        let Action::Effacer(v) = t(&mut e, Touche::Entree) else { panic!("Entrée doit confirmer") };
        assert_eq!(v.iter().map(|k| k.id).collect::<Vec<_>>(), vec!["d3d"]);
        assert_eq!(e.net_etape, EtapeNet::Suppression);
        // Pendant la suppression : rien ne se déclenche, aucune mesure ne part ; seule la sortie de la page reste ouverte
        // (Retour et Échap la quittent : la suppression continue en fond, la page ne doit pas emprisonner l'auteur)
        assert_eq!(t(&mut e, Touche::Entree), Action::Rien);
        assert_eq!(t(&mut e, Touche::Bas), Action::Rien);
        assert_eq!(t(&mut e, Touche::Retour), Action::Quitter);
        assert!(!e.echap_nettoyage() && e.net_etape == EtapeNet::Suppression);
        e.nettoyage = None;
        assert!(!e.nettoyage_a_lancer());
        // Fin : coches remises à zéro, résultat dit, nouvelle mesure demandée
        e.finir_suppression("Nettoyage terminé".to_string());
        assert_eq!((e.net_etape.clone(), e.net_cochees.len()), (EtapeNet::Liste, 0));
        assert_eq!(e.net_message.as_deref(), Some("Nettoyage terminé"));
        assert!(e.nettoyage_a_lancer());
    }

    #[test]
    fn une_selection_devenue_vide_apres_la_remesure_n_ouvre_pas_de_confirmation() {
        let mut e = page_nettoyage();
        t(&mut e, Touche::Cocher);
        t(&mut e, Touche::Entree);
        e.nettoyage_a_lancer();
        // Entre-temps, le cache a été vidé par un autre programme : plus rien à proposer
        e.recevoir_nettoyage(vec![cat("temp", 3)]);
        assert_eq!(e.net_etape, EtapeNet::Liste);
        assert!(e.net_cochees.is_empty());
        assert_eq!(e.net_message.as_deref(), Some("Plus rien à nettoyer dans cette sélection"));
    }

    #[test]
    fn les_coches_survivent_a_un_changement_d_index() {
        let mut e = page_nettoyage();
        t(&mut e, Touche::Cocher);
        let mut nouveau = Etat::nouveau();
        nouveau.reprendre_nettoyage(e);
        assert_eq!((nouveau.page, nouveau.net_cochees.clone()), (PAGE_NETTOYAGE, vec!["d3d"]));
        assert!(nouveau.nettoyage.is_some());
    }
}
