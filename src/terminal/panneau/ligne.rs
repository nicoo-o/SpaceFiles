//! T5 : la ligne de saisie du terminal. Tant que le shell attend une commande (invite affichée, aucun programme plein écran),
//! SpaceFiles garde lui-même la ligne en cours (`Saisie`) au lieu de tout envoyer au shell touche par touche : c'est ce qui
//! permet la suggestion grise tirée de l'historique de SpaceFiles et le rappel par ↑ / ↓. La ligne n'est envoyée qu'à Entrée.
//!
//! Règle de prudence : dès qu'une touche sort de ce que l'éditeur sait faire (Tab, F1-F12, Alt+lettre, Ctrl+R…), la ligne en
//! cours part au shell, la touche aussi, et la ligne **appartient au shell** jusqu'à la prochaine invite annoncée : aucune
//! suggestion, aucun enregistrement dans l'historique pour cette ligne. Mieux vaut perdre un confort qu'une frappe.
//!
//! Le cœur (`EtatLigne`) ne connaît ni le shell ni la fenêtre : il se teste sans écran. Les méthodes de `Panneau` en bas
//! l'appliquent au vrai panneau (envoi des octets, invite prête).

use super::{touche_vers_octets, Mods, Panneau};
use crate::terminal::completion::{completer, Candidat, Genre, Proposition};
use crate::terminal::historique::Historique;
use crate::terminal::melange::decider;
use crate::terminal::saisie::{Saisie, LONGUEUR_MAX};
use crate::terminal::session::Shell;

const VK_BACK: u32 = 0x08;
const VK_TAB: u32 = 0x09;
/// Rangées du menu de complétion visibles à la fois (le reste défile)
pub const MENU_RANGEES: usize = 8;
/// Maj, Ctrl, Alt, Verr. Maj et leurs variantes gauche / droite : les presser ne ferme pas le menu (Maj+Tab commence par Maj)
fn est_modificateur(vk: u32) -> bool {
    matches!(vk, 0x10 | 0x11 | 0x12 | 0x14 | 0xA0..=0xA5)
}
const VK_RETURN: u32 = 0x0D;
const VK_ESCAPE: u32 = 0x1B;
const VK_END: u32 = 0x23;
const VK_HOME: u32 = 0x24;
const VK_LEFT: u32 = 0x25;
const VK_UP: u32 = 0x26;
const VK_RIGHT: u32 = 0x27;
const VK_DOWN: u32 = 0x28;
const VK_DELETE: u32 = 0x2E;
const VK_A: u32 = 0x41;
const VK_C: u32 = 0x43;
const VK_E: u32 = 0x45;
const VK_U: u32 = 0x55;
const VK_W: u32 = 0x57;

/// Un rappel par ↑ / ↓ en cours
#[derive(Clone, Debug, PartialEq, Eq)]
struct Rappel {
    /// Rang de la ligne montrée parmi celles qui commencent par `filtre` (0 = la plus récente)
    rang: usize,
    /// Ce qui était tapé quand on a pressé ↑ la première fois : filtre du rappel, et retour de ↓ au bout
    filtre: String,
}

/// Ce que devient une touche ou un caractère pour la ligne
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Effet {
    /// La ligne n'en veut pas : le chemin habituel (octets bruts vers le shell) s'applique
    Passer,
    /// Comme `Passer`, mais la ligne en cours doit d'abord partir au shell et lui appartenir ensuite
    Deleguer,
    /// Réglé par la ligne (édition, rappel, suggestion acceptée) : rien à envoyer
    Pris,
    /// Entrée : la ligne est prise ; l'appelant l'envoie au shell
    Soumettre(String),
}

/// T7 : le menu de complétion ouvert par Tab (plusieurs candidats)
#[derive(Clone, Debug, PartialEq, Eq)]
struct Menu {
    /// La ligne avant le premier Tab : Échap la rétablit, et chaque candidat remplace le jeton à partir d'elle
    origine: String,
    proposition: Proposition,
    /// Le candidat inséré dans la ligne ; `None` tant que seul le début commun a été ajouté (ou, en liste automatique, tant
    /// que rien n'est choisi)
    rang: Option<usize>,
    /// La liste automatique (rafraîchie à chaque frappe, sans Tab) et non le menu ouvert par Tab
    auto: bool,
}

impl Menu {
    /// La ligne que donne le choix du candidat `rang` : une commande de l'historique remplace toute la ligne, les autres
    /// candidats remplacent le jeton
    fn ligne_du(&self, rang: usize) -> String {
        let c = &self.proposition.candidats[rang];
        if c.genre == Genre::Historique {
            c.brut.clone()
        } else {
            remplacer_jeton(&self.origine, self.proposition.debut, &self.proposition.texte_de(c))
        }
    }
}

/// Lignes d'historique au plus dans la liste automatique
const AUTO_HISTORIQUE: usize = 3;
/// Candidats au plus dans la liste automatique (historique compris) : la liste défile par fenêtres de `MENU_RANGEES`
const AUTO_MAX: usize = 24;
/// Caractères tapés au moins (espaces de tête exclus) avant que la liste automatique apparaisse
const AUTO_MIN: usize = 2;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct EtatLigne {
    saisie: Saisie,
    rappel: Option<Rappel>,
    completion: Option<Menu>,
}

impl EtatLigne {
    pub fn saisie(&self) -> &Saisie {
        &self.saisie
    }

    pub fn vide(&self) -> bool {
        self.saisie.vide()
    }

    fn oublier_rappel(&mut self) {
        self.rappel = None;
    }

    /// La fin que la suggestion grise ajouterait à la ligne ; rien pendant un rappel ni dans un menu de Tab, ni quand la liste
    /// automatique a un candidat choisi (la ligne montre alors ce choix), ni curseur au milieu du texte. La liste automatique
    /// sans choix la laisse : la grise est sa première ligne d'historique.
    pub fn suggestion(&self, hist: &Historique, dossier: &str) -> Option<String> {
        let menu_muet = self.completion.as_ref().is_some_and(|m| !m.auto || m.rang.is_some());
        if self.rappel.is_some() || menu_muet || self.saisie.vide() || !self.saisie.curseur_a_la_fin() {
            return None;
        }
        hist.suggerer(&self.saisie.texte(), dossier).map(str::to_string)
    }

    fn accepter_suggestion(&mut self, hist: &Historique, dossier: &str) {
        if let Some(fin) = self.suggestion(hist, dossier) {
            self.saisie.inserer(&fin);
        }
    }

    /// ↑ : la commande précédente qui commence par ce qui était tapé
    /// `false` si rien n'a été trouvé (historique vide ou épuisé)
    fn rappeler_plus_ancien(&mut self, hist: &Historique) -> bool {
        let courant = self.saisie.texte();
        let (mut rang, filtre) = match &self.rappel {
            Some(r) => (r.rang + 1, r.filtre.clone()),
            None => (0, courant.clone()),
        };
        // Au premier ↑, la ligne identique à ce qui est tapé ne compte pas (elle ne changerait rien à l'écran)
        let trouvee = loop {
            match hist.rappeler(rang, &filtre) {
                Some(l) if self.rappel.is_none() && l == courant => rang += 1,
                autre => break autre.map(str::to_string),
            }
        };
        match trouvee {
            Some(l) => {
                self.saisie.remplacer(&l);
                self.rappel = Some(Rappel { rang, filtre });
                true
            }
            None => false,
        }
    }

    /// ↓ : une commande plus récente, puis le brouillon d'avant le premier ↑
    fn rappeler_plus_recent(&mut self, hist: &Historique) {
        let Some(r) = self.rappel.clone() else { return };
        if r.rang > 0 {
            if let Some(l) = hist.rappeler(r.rang - 1, &r.filtre).map(str::to_string) {
                self.saisie.remplacer(&l);
                self.rappel = Some(Rappel { rang: r.rang - 1, filtre: r.filtre });
                return;
            }
        }
        self.saisie.remplacer(&r.filtre);
        self.rappel = None;
    }

    /// Une touche enfoncée. `deleguable` : le chemin habituel saurait en faire quelque chose (`touche_vers_octets`).
    pub fn touche(&mut self, vk: u32, m: Mods, hist: &Historique, dossier: &str, deleguable: bool) -> Effet {
        let sortie_libre = if deleguable { Effet::Deleguer } else { Effet::Passer };
        // Alt seul ou AltGr (Ctrl+Alt) : un caractère d'AltGr arrive par WM_CHAR, une touche Alt+x est pour le shell
        if m.alt {
            return sortie_libre;
        }
        // T7 : Tab complète (Ctrl+Tab reste au shell) ; le menu ouvert prend Échap et les flèches, tout autre geste le ferme
        if vk == VK_TAB && !m.ctrl {
            self.oublier_rappel();
            // Liste automatique sans choix : Tab fait son travail habituel (début commun, puis menu), la liste s'efface
            if self.completion.as_ref().is_some_and(|m| m.auto && m.rang.is_none()) {
                self.completion = None;
            }
            self.tabulation(m.maj, dossier);
            return Effet::Pris;
        }
        if self.completion.is_some() && !m.ctrl && !m.maj {
            match vk {
                VK_ESCAPE => {
                    if let Some(menu) = self.completion.take() {
                        self.saisie.remplacer(&menu.origine);
                    }
                    return Effet::Pris;
                }
                // Liste automatique, un candidat choisi : Entrée le valide (la ligne reste, la liste se ferme) sans exécuter ;
                // une seconde Entrée exécute. Sans choix, ou après Tab (la ligne complétée s'exécute comme toujours), Entrée
                // soumet la ligne.
                VK_RETURN if self.completion.as_ref().is_some_and(|m| m.auto && m.rang.is_some()) => {
                    self.completion = None;
                    return Effet::Pris;
                }
                VK_DOWN => {
                    self.cycler(1);
                    return Effet::Pris;
                }
                VK_UP => {
                    self.cycler(-1);
                    return Effet::Pris;
                }
                _ => {}
            }
        }
        if !est_modificateur(vk) {
            self.completion = None;
        }
        if !matches!(vk, VK_UP | VK_DOWN) {
            self.oublier_rappel();
        }
        let s = &mut self.saisie;
        match (vk, m.ctrl, m.maj) {
            (VK_RETURN, false, _) => return Effet::Soumettre(s.prendre()),
            (VK_BACK, false, _) => {
                s.retour_arriere();
            }
            (VK_BACK, true, false) | (VK_W, true, false) => {
                s.supprimer_mot_gauche();
            }
            (VK_DELETE, false, false) => {
                s.supprimer();
            }
            (VK_DELETE, true, false) => {
                s.supprimer_mot_droite();
            }
            (VK_LEFT, false, false) => {
                s.gauche();
            }
            (VK_LEFT, true, false) => {
                s.mot_gauche();
            }
            (VK_RIGHT, false, false) => {
                if s.curseur_a_la_fin() {
                    self.accepter_suggestion(hist, dossier);
                } else {
                    s.droite();
                }
            }
            (VK_RIGHT, true, false) => {
                s.mot_droite();
            }
            (VK_HOME, false, false) | (VK_A, true, false) => {
                s.debut();
            }
            (VK_END, false, false) | (VK_E, true, false) => {
                if s.curseur_a_la_fin() {
                    self.accepter_suggestion(hist, dossier);
                } else {
                    s.fin();
                }
            }
            (VK_U, true, false) => {
                s.supprimer_jusqu_au_debut();
            }
            // Rien dans l'historique de SpaceFiles : la flèche est pour le shell (PSReadLine a tout l'historique de l'auteur)
            (VK_UP, false, false) => {
                if !self.rappeler_plus_ancien(hist) && self.rappel.is_none() {
                    return sortie_libre;
                }
            }
            (VK_DOWN, false, false) => self.rappeler_plus_recent(hist),
            // Échap vide la ligne ; vide déjà, rien ne part (le shell n'a rien reçu, il reste « à l'invite »)
            (VK_ESCAPE, false, false) => {
                s.prendre();
            }
            // Ctrl+C : une ligne commencée s'abandonne ; sinon c'est l'interruption du shell (WM_CHAR 0x03)
            (VK_C, true, false) => {
                if s.vide() {
                    return Effet::Passer;
                }
                s.prendre();
            }
            _ => return sortie_libre,
        }
        // Effacer des lettres change aussi ce que la liste automatique propose
        if matches!((vk, m.ctrl), (VK_BACK, _) | (VK_DELETE, _) | (VK_W, true) | (VK_U, true)) {
            self.rafraichir(hist, dossier);
        }
        Effet::Pris
    }

    /// La liste automatique : après chaque frappe, jusqu'à `AUTO_HISTORIQUE` commandes de l'historique qui commencent par la
    /// ligne, puis les complétions de Tab du dernier mot (commandes, sous-commandes, options, fichiers). Rien avant
    /// `AUTO_MIN` caractères, curseur au milieu, ligne de plusieurs lignes ou rappel en cours. Aucune complétion de chemin
    /// depuis ou vers un partage réseau (`\\`) : lire un dossier distant à chaque lettre figerait la frappe.
    fn rafraichir(&mut self, hist: &Historique, dossier: &str) {
        self.completion = None;
        if self.rappel.is_some() || !self.saisie.curseur_a_la_fin() {
            return;
        }
        let origine = self.saisie.texte();
        if origine.contains('\n') || origine.trim_start().chars().count() < AUTO_MIN {
            return;
        }
        let mut candidats: Vec<Candidat> = hist
            .proposer(&origine, dossier, AUTO_HISTORIQUE)
            .into_iter()
            .map(|l| Candidat { brut: l.to_string(), nom: l.to_string(), genre: Genre::Historique, description: "historique" })
            .collect();
        let reseau = dossier.starts_with("\\\\") || origine.contains("\\\\");
        let mut proposition = Proposition {
            debut: origine.chars().count(),
            prefixe: origine.clone(),
            guillemet: false,
            chemins: false,
            candidats: Vec::new(),
        };
        if !reseau {
            let p = completer(&origine, dossier);
            // Un jeton vide (« git ») : tout proposer à chaque espace noierait la liste, Tab reste là pour ça
            if !p.prefixe.is_empty() {
                let deja: Vec<String> = candidats.iter().map(|c| c.brut.clone()).collect();
                let retenus: Vec<Candidat> = p
                    .candidats
                    .iter()
                    .filter(|c| {
                        let ligne = remplacer_jeton(&origine, p.debut, &p.texte_de(c));
                        ligne != origine && !deja.contains(&ligne)
                    })
                    .cloned()
                    .collect();
                proposition = Proposition { candidats: Vec::new(), ..p };
                candidats.extend(retenus);
            }
        }
        candidats.truncate(AUTO_MAX);
        if candidats.is_empty() {
            return;
        }
        proposition.candidats = candidats;
        self.completion = Some(Menu { origine, proposition, rang: None, auto: true });
    }

    /// Un caractère reçu par WM_CHAR : imprimable = inséré ; contrôle = pour le shell (Ctrl+lettre sans geste ici)
    pub fn caractere(&mut self, c: char) -> Effet {
        if c.is_control() {
            // Ctrl+C sur une ligne vide ne change rien à l'état du shell : il reçoit l'interruption et la ligne reste prête
            return if c == '\u{3}' && self.saisie.vide() { Effet::Passer } else { Effet::Deleguer };
        }
        self.oublier_rappel();
        self.completion = None;
        // Ligne pleine : jamais de caractère perdu en silence, la ligne passe au shell qui reçoit aussi ce caractère
        if self.saisie.inserer_caractere(c) { Effet::Pris } else { Effet::Deleguer }
    }

    /// Un collage : inséré tel quel s'il tient dans la ligne, sinon la ligne se délègue (jamais de troncature silencieuse)
    pub fn coller(&mut self, texte: &str) -> Effet {
        if self.saisie.longueur() + texte.chars().count() > LONGUEUR_MAX {
            return Effet::Deleguer;
        }
        self.oublier_rappel();
        self.completion = None;
        self.saisie.inserer(texte);
        Effet::Pris
    }

    /// Prend ce qui reste dans la ligne (délégation au shell ou fermeture)
    fn vider(&mut self) -> String {
        self.oublier_rappel();
        self.completion = None;
        self.saisie.prendre()
    }

    /// T7 : Tab. Menu ouvert : candidat suivant (précédent avec Maj). Sinon on complète le dernier mot de la ligne : un seul
    /// candidat s'insère, plusieurs ajoutent leur début commun et ouvrent le menu. Refusé (sans effet) sur une ligne vide,
    /// curseur au milieu du texte ou ligne de plusieurs lignes : le jeton à compléter y serait ambigu.
    fn tabulation(&mut self, inverse: bool, dossier: &str) {
        if self.completion.is_some() {
            self.cycler(if inverse { -1 } else { 1 });
            return;
        }
        if self.saisie.vide() || !self.saisie.curseur_a_la_fin() {
            return;
        }
        let origine = self.saisie.texte();
        if origine.contains('\n') {
            return;
        }
        let proposition = completer(&origine, dossier);
        match proposition.candidats.len() {
            0 => {}
            1 => {
                let texte = proposition.texte_de(&proposition.candidats[0]);
                self.saisie.remplacer(&remplacer_jeton(&origine, proposition.debut, &texte));
            }
            _ => {
                let commun = proposition.prefixe_commun();
                let ouvrir_sur = commun.is_some();
                if let Some(c) = commun {
                    self.saisie.remplacer(&remplacer_jeton(&origine, proposition.debut, &c));
                }
                self.completion = Some(Menu { origine, proposition, rang: None, auto: false });
                if !ouvrir_sur {
                    self.cycler(if inverse { -1 } else { 1 });
                }
            }
        }
    }

    /// Passe au candidat suivant (`sens` = 1) ou précédent (-1) et l'écrit dans la ligne à la place du jeton
    fn cycler(&mut self, sens: isize) {
        let Some(menu) = self.completion.as_mut() else { return };
        let n = menu.proposition.candidats.len() as isize;
        if n == 0 {
            return;
        }
        let rang = match menu.rang {
            None if sens < 0 => n - 1,
            None => 0,
            Some(r) => (r as isize + sens).rem_euclid(n),
        } as usize;
        menu.rang = Some(rang);
        let ligne = menu.ligne_du(rang);
        self.saisie.remplacer(&ligne);
    }

    /// Le menu de complétion à dessiner, `None` s'il est fermé
    pub fn vue_menu(&self) -> Option<VueMenu> {
        let menu = self.completion.as_ref()?;
        let total = menu.proposition.candidats.len();
        let debut = match menu.rang {
            Some(r) if r >= MENU_RANGEES => r + 1 - MENU_RANGEES,
            _ => 0,
        };
        let lignes: Vec<LigneMenu> = menu
            .proposition
            .candidats
            .iter()
            .skip(debut)
            .take(MENU_RANGEES)
            .map(|c| LigneMenu { nom: c.nom.clone(), description: c.description, genre: c.genre })
            .collect();
        let selection = menu.rang.map(|r| r - debut);
        Some(VueMenu { lignes, selection, total, debut })
    }
}

/// La ligne `origine` dont le jeton qui commence au caractère `debut` est remplacé par `texte`
fn remplacer_jeton(origine: &str, debut: usize, texte: &str) -> String {
    let mut ligne: String = origine.chars().take(debut).collect();
    ligne.push_str(texte);
    ligne
}

/// Une rangée du menu de complétion
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LigneMenu {
    pub nom: String,
    pub description: &'static str,
    pub genre: Genre,
}

/// Le menu de complétion tel que le dessin le pose : au plus `MENU_RANGEES` rangées, la sélection les suit
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VueMenu {
    pub lignes: Vec<LigneMenu>,
    /// Rang de la rangée choisie parmi `lignes` ; `None` tant que seul le début commun a été inséré
    pub selection: Option<usize>,
    /// Nombre de candidats en tout (le menu en montre une fenêtre)
    pub total: usize,
    /// Rang du premier candidat montré
    pub debut: usize,
}

/// Ce que le dessin pose pour la ligne : tout est déjà découpé à la largeur de la rangée
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VueLigne {
    /// Colonne de la première case de la ligne (juste après l'invite)
    pub col: usize,
    /// Les caractères visibles, une case chacun (les fins de ligne collées sont des « ↵ »)
    pub visibles: Vec<char>,
    /// Rang du curseur parmi les cases de la ligne (au plus `visibles.len()`)
    pub curseur: usize,
    /// Suggestion grise, posée juste après `visibles` (déjà tronquée à la place qui reste)
    pub suggestion: Vec<char>,
    /// T6 : la syntaxe que la ligne tapée aura à Entrée (badge CMD / PS à droite de la rangée)
    pub shell: Shell,
    /// T7 : le menu de complétion ouvert par Tab, posé sous la rangée (ou au-dessus faute de place)
    pub menu: Option<VueMenu>,
}

impl VueLigne {
    /// Le mot du badge
    pub fn badge(&self) -> &'static str {
        match self.shell {
            Shell::Cmd => "CMD",
            Shell::PowerShell => "PS",
        }
    }

    /// Le caractère sous le curseur : celui de la ligne, ou la première lettre de la suggestion
    pub fn sous_curseur(&self) -> Option<char> {
        match self.visibles.get(self.curseur) {
            Some(&c) => Some(c),
            None if self.curseur >= self.visibles.len() => self.suggestion.first().copied(),
            None => None,
        }
    }
}

/// La vue de la ligne pour une rangée de `colonnes` cases dont l'invite occupe `col_invite` ; `None` pour une ligne vide
/// (le curseur du shell suffit alors) ou sans place. La fenêtre garde le curseur visible quand la ligne dépasse la rangée.
pub fn composer_vue(saisie: &Saisie, suggestion: Option<&str>, col_invite: usize, colonnes: usize) -> Option<VueLigne> {
    if saisie.vide() {
        return None;
    }
    let largeur = colonnes.saturating_sub(col_invite);
    if largeur == 0 {
        return None;
    }
    let (debut, curseur) = saisie.fenetre(largeur);
    let visibles: Vec<char> = saisie.texte_affiche().into_iter().skip(debut).take(largeur).collect();
    let place = largeur.saturating_sub(visibles.len());
    let suggestion: Vec<char> = match suggestion {
        Some(s) if saisie.curseur_a_la_fin() => s.chars().filter(|c| !c.is_control()).take(place).collect(),
        _ => Vec::new(),
    };
    // `vue_ligne` y met la syntaxe reconnue ; ici le défaut, pour que les tests de découpe ne dépendent pas de la décision
    Some(VueLigne { col: col_invite, visibles, curseur, suggestion, shell: Shell::PowerShell, menu: None })
}

impl Panneau {
    /// La ligne de SpaceFiles est celle qui reçoit la frappe : le shell attend une commande et aucun programme n'a l'écran
    pub fn saisie_prete(&self) -> bool {
        self.actif() && self.invite_prete && !self.vt.ecran_alterne()
    }

    /// Envoie au shell ce qui reste dans la ligne, qui lui appartient ensuite jusqu'à la prochaine invite
    fn deleguer_au_shell(&mut self) {
        let reste = self.ligne.vider();
        if !reste.is_empty() {
            let octets = self.octets_de_collage(&reste);
            self.envoyer(&octets);
        }
        self.invite_prete = false;
    }

    /// Une touche enfoncée alors que le terminal a le clavier. `None` : le chemin habituel continue (la ligne a été envoyée
    /// au shell si besoin). `Some(None)` : réglée par la ligne. `Some(Some(ligne))` : Entrée, la ligne est partie au shell ;
    /// l'appelant l'enregistre dans l'historique.
    pub fn touche_ligne(&mut self, vk: u32, m: Mods, hist: &Historique) -> Option<Option<String>> {
        if !self.saisie_prete() {
            return None;
        }
        let deleguable = touche_vers_octets(vk, m, &self.vt.modes()).is_some();
        let dossier = self.dossier_connu.clone();
        let effet = self.ligne.touche(vk, m, hist, &dossier, deleguable);
        self.appliquer(effet)
    }

    /// Un caractère (WM_CHAR) : rend les octets que l'appelant doit envoyer au shell avec `envoyer`. Vide si la ligne l'a pris
    /// (ou si une paire de substituts attend sa seconde moitié). Si la ligne devait partir au shell d'abord, elle est déjà
    /// partie : l'ordre des octets reste celui de la frappe.
    pub fn caractere_ligne(&mut self, unite: u16, hist: &Historique) -> Vec<u8> {
        let octets = self.octets_de_unite(unite);
        if octets.is_empty() || !self.saisie_prete() {
            return octets;
        }
        let Some(c) = String::from_utf8_lossy(&octets).chars().next() else { return octets };
        match self.ligne.caractere(c) {
            Effet::Pris => {
                self.decalage = 0;
                let dossier = self.dossier_connu.clone();
                self.ligne.rafraichir(hist, &dossier);
                Vec::new()
            }
            effet => {
                if self.appliquer(effet).is_some() {
                    return Vec::new();
                }
                octets
            }
        }
    }

    /// Un collage : `true` s'il est pris par la ligne ; sinon l'appelant le colle au shell (la ligne lui a été envoyée avant)
    pub fn coller_ligne(&mut self, texte: &str, hist: &Historique) -> bool {
        if !self.saisie_prete() {
            return false;
        }
        let effet = self.ligne.coller(texte);
        let pris = effet == Effet::Pris;
        let pris_ou_avale = self.appliquer(effet).is_some();
        if pris {
            let dossier = self.dossier_connu.clone();
            self.ligne.rafraichir(hist, &dossier);
        }
        pris_ou_avale
    }

    /// La ligne tenue compte plusieurs lignes (collage) et le shell n'a pas demandé le collage encadré : la lui envoyer
    /// exécuterait les premières lignes sans Entrée de l'auteur
    fn multiligne_sans_encadrement(&self) -> bool {
        !self.vt.modes().collage_encadre && self.ligne.saisie().texte().contains('\n')
    }

    fn appliquer(&mut self, effet: Effet) -> Option<Option<String>> {
        match effet {
            Effet::Passer => None,
            // Une ligne multiligne sans collage encadré ne se délègue pas : la touche est avalée (Entrée, Échap et Ctrl+C restent)
            Effet::Deleguer if self.multiligne_sans_encadrement() => {
                self.decalage = 0;
                Some(None)
            }
            Effet::Deleguer => {
                self.deleguer_au_shell();
                None
            }
            Effet::Pris => {
                // Taper fait toujours revoir ce qu'on tape
                self.decalage = 0;
                Some(None)
            }
            Effet::Soumettre(ligne) => {
                // T6 : ce qui part au shell peut différer de ce qui est tapé (cmd, `cmd:`, `ps:`) ; l'historique garde la frappe
                let envoi = decider(&ligne).envoi;
                let mut octets = if envoi.is_empty() { Vec::new() } else { self.octets_de_collage(&envoi) };
                octets.push(b'\r');
                self.envoyer(&octets);
                Some(Some(ligne))
            }
        }
    }

    /// La ligne à dessiner (texte, curseur, suggestion), ou `None` si le curseur du shell suffit
    pub fn vue_ligne(&self, hist: &Historique) -> Option<VueLigne> {
        if !self.saisie_prete() || self.rangee_du_curseur().is_none() {
            return None;
        }
        let suggestion = self.ligne.suggestion(hist, &self.dossier_connu);
        let mut vue = composer_vue(self.ligne.saisie(), suggestion.as_deref(), self.vt.curseur().0, self.vt.colonnes())?;
        vue.shell = decider(&self.ligne.saisie().texte()).shell;
        vue.menu = self.ligne.vue_menu();
        Some(vue)
    }
}

#[cfg(test)]
mod tests;
