// Palette à étapes (lot 15C) : la logique pure de « élément › commande › destination ». Qui est l'élément, quelles commandes
// s'appliquent à lui, dans quel ordre proposer les destinations, un nom est-il libre, que dit la phrase d'effet. Rien ici ne
// touche à la fenêtre ni au disque sauf `lire_dossier`, qui ne tourne jamais sur le fil d'interface (voir `commandes::assistant`).

use crate::icones::Icone;
use std::collections::HashSet;

/// Étape de la palette. Les trois premières sont les puces du fil d'Ariane ; `Progression` et `Resultat` les gardent toutes allumées.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Etape {
    Element,
    Commande,
    Parametre,
    Progression,
    Resultat,
}

impl Etape {
    /// Rang de la puce animée (`mouvement::AnimPalette`) : 0, 1, 2 ; au-delà, aucune puce n'est « courante »
    pub fn rang(self) -> usize {
        match self {
            Etape::Element => 0,
            Etape::Commande => 1,
            Etape::Parametre => 2,
            Etape::Progression | Etape::Resultat => 3,
        }
    }
}

/// Ce que l'élément choisi est : décide des commandes qui s'appliquent
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GenreSujet {
    Fichier,
    Dossier,
    Plusieurs,
}

/// L'élément sur lequel on travaille : un fichier, un dossier, plusieurs éléments, ou le dossier ouvert quand rien n'est choisi
#[derive(Clone, PartialEq, Debug)]
pub struct Sujet {
    pub chemins: Vec<String>,
    pub genre: GenreSujet,
    /// Le dossier ouvert pris faute de sélection : on ne le déplace pas, on ne le renomme pas, on ne le jette pas (il est sous nos pieds)
    pub ouvert: bool,
    pub icone: Icone,
}

/// Nom d'un chemin : son dernier segment (« C:\ » pour une racine)
pub fn nom_de(chemin: &str) -> String {
    let t = chemin.trim_end_matches('\\');
    if t.len() <= 2 && t.ends_with(':') {
        return format!("{t}\\");
    }
    t.rsplit('\\').next().unwrap_or(t).to_string()
}

impl Sujet {
    pub fn un(chemin: String, dossier: bool, icone: Icone) -> Self {
        Self { chemins: vec![chemin], genre: if dossier { GenreSujet::Dossier } else { GenreSujet::Fichier }, ouvert: false, icone }
    }

    pub fn plusieurs(chemins: Vec<String>) -> Self {
        Self { chemins, genre: GenreSujet::Plusieurs, ouvert: false, icone: Icone::Fichier }
    }

    /// Le dossier ouvert (aucune sélection)
    pub fn dossier_ouvert(chemin: String) -> Self {
        Self { chemins: vec![chemin], genre: GenreSujet::Dossier, ouvert: true, icone: Icone::Dossier }
    }

    /// « rapport.pdf », « 3 éléments »
    pub fn libelle(&self) -> String {
        match self.genre {
            GenreSujet::Plusieurs => format!("{} éléments", self.chemins.len()),
            _ => self.chemins.first().map(|c| nom_de(c)).unwrap_or_default(),
        }
    }

    /// Noms (derniers segments) de ce qu'on va copier, déplacer ou créer
    pub fn noms(&self) -> Vec<String> {
        self.chemins.iter().map(|c| nom_de(c)).collect()
    }

    /// Dossier qui contient le premier élément (là où un renommage se joue)
    pub fn parent(&self) -> Option<String> {
        self.chemins.first().map(|c| crate::dossier::dossier_de(c).to_string())
    }

    /// Tous les éléments sont dans le même dossier (sans tenir compte de la casse ni de la barre finale)
    pub fn meme_parent(&self) -> bool {
        let cle = |c: &String| crate::dossier::dossier_de(c.trim_end_matches('\\')).trim_end_matches('\\').to_lowercase();
        let mut it = self.chemins.iter().map(cle);
        it.next().is_none_or(|p| it.all(|q| q == p))
    }
}

/// Ce qu'une commande demande avant de partir
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Param {
    /// Part tout de suite
    Aucun,
    Destination,
    Nom,
    Confirmation,
}

impl Param {
    /// Ce que la ligne de commande annonce (colonne de détail, étape 2)
    pub fn annonce(self) -> &'static str {
        match self {
            Param::Aucun => "TOUT DE SUITE",
            Param::Destination => "DESTINATION ›",
            Param::Nom => "NOM ›",
            Param::Confirmation => "CONFIRMATION ›",
        }
    }
}

/// Commandes qui s'appliquent à un élément choisi dans la palette. Chacune passe par une opération qui existe déjà (`IFileOperation`,
/// renommage de l'historique, `archive`, presse-papiers, verbes du shell) ; aucune n'est neuve.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CommandeEl {
    Ouvrir,
    OuvrirOnglet,
    CopierVers,
    DeplacerVers,
    CompresserVers,
    Renommer,
    NouveauDossier,
    Corbeille,
    CopierPressePapiers,
    CouperPressePapiers,
    CopierChemin,
    Partager,
    Proprietes,
    AjouterGroupe,
}

pub struct DefCommande {
    pub commande: CommandeEl,
    pub libelle: &'static str,
    pub raccourci: &'static str,
    pub param: Param,
    pub mots: &'static str,
    pub icone: Icone,
}

/// Table des commandes d'élément, dans l'ordre de l'étape 2 (les plus courantes d'abord)
pub const TABLE: &[DefCommande] = &[
    DefCommande { commande: CommandeEl::Ouvrir, libelle: "OUVRIR", raccourci: "ENTRÉE", param: Param::Aucun, mots: "open lancer", icone: Icone::Fichier },
    DefCommande { commande: CommandeEl::CopierVers, libelle: "COPIER VERS…", raccourci: "", param: Param::Destination, mots: "copy dupliquer destination", icone: Icone::Dossier },
    DefCommande { commande: CommandeEl::DeplacerVers, libelle: "DÉPLACER VERS…", raccourci: "", param: Param::Destination, mots: "move couper deplacer destination", icone: Icone::Dossier },
    DefCommande { commande: CommandeEl::Renommer, libelle: "RENOMMER…", raccourci: "F2", param: Param::Nom, mots: "rename nom", icone: Icone::Texte },
    DefCommande { commande: CommandeEl::Corbeille, libelle: "METTRE À LA CORBEILLE", raccourci: "SUPPR", param: Param::Confirmation, mots: "supprimer effacer delete corbeille", icone: Icone::Fichier },
    DefCommande { commande: CommandeEl::CompresserVers, libelle: "COMPRESSER VERS… (ZIP)", raccourci: "", param: Param::Destination, mots: "archive zipper zip destination", icone: Icone::Archive },
    DefCommande { commande: CommandeEl::NouveauDossier, libelle: "NOUVEAU DOSSIER DEDANS…", raccourci: "", param: Param::Nom, mots: "creer dossier dedans", icone: Icone::Dossier },
    DefCommande { commande: CommandeEl::OuvrirOnglet, libelle: "OUVRIR DANS UN NOUVEL ONGLET", raccourci: "CLIC MILIEU", param: Param::Aucun, mots: "tab onglet", icone: Icone::Dossier },
    DefCommande { commande: CommandeEl::CopierChemin, libelle: "COPIER LE CHEMIN", raccourci: "CTRL+MAJ+C", param: Param::Aucun, mots: "path adresse", icone: Icone::Texte },
    DefCommande { commande: CommandeEl::CopierPressePapiers, libelle: "COPIER (PRESSE-PAPIERS)", raccourci: "CTRL+C", param: Param::Aucun, mots: "copy presse papiers coller ensuite", icone: Icone::Fichier },
    DefCommande { commande: CommandeEl::CouperPressePapiers, libelle: "COUPER (PRESSE-PAPIERS)", raccourci: "CTRL+X", param: Param::Aucun, mots: "cut presse papiers coller ensuite", icone: Icone::Fichier },
    DefCommande { commande: CommandeEl::AjouterGroupe, libelle: "AJOUTER À UN GROUPE…", raccourci: "CTRL+G", param: Param::Aucun, mots: "groupe tag etiquette collection", icone: Icone::Dossier },
    DefCommande { commande: CommandeEl::Partager, libelle: "PARTAGER", raccourci: "", param: Param::Aucun, mots: "share envoyer", icone: Icone::Nuage },
    DefCommande { commande: CommandeEl::Proprietes, libelle: "PROPRIÉTÉS", raccourci: "ALT+ENTREE", param: Param::Aucun, mots: "infos details properties", icone: Icone::Reglages },
];

pub fn def(c: CommandeEl) -> &'static DefCommande {
    TABLE.iter().find(|d| d.commande == c).unwrap_or(&TABLE[0])
}

/// Libellé sans les points de suspension (« Copier vers »), pour la puce du fil d'Ariane
pub fn libelle_court(c: CommandeEl) -> String {
    let l = def(c).libelle.trim_end_matches(" (ZIP)").trim_end_matches('…').trim();
    // Capitales pour les listes, minuscules pour la puce : « Copier vers »
    let mut it = l.chars();
    it.next().map_or_else(String::new, |p| p.to_string() + &it.as_str().to_lowercase())
}

/// Pourquoi la commande ne s'applique pas à cet élément (`None` : elle s'applique)
pub fn indisponible(c: CommandeEl, s: &Sujet) -> Option<&'static str> {
    use CommandeEl::*;
    let plusieurs = s.genre == GenreSujet::Plusieurs;
    match c {
        Ouvrir | OuvrirOnglet | Renommer | NouveauDossier if plusieurs => Some("UN SEUL ÉLÉMENT"),
        OuvrirOnglet | NouveauDossier if s.genre == GenreSujet::Fichier => Some("UN DOSSIER SEULEMENT"),
        Renommer | DeplacerVers | Corbeille | CouperPressePapiers if s.ouvert => Some("DOSSIER OUVERT : OUVREZ SON PARENT"),
        // Le zip se fait depuis un seul dossier source (les noms y sont relatifs) : des éléments de dossiers différents ne tiennent pas
        CompresserVers if plusieurs && !s.meme_parent() => Some("ÉLÉMENTS DE DOSSIERS DIFFÉRENTS"),
        _ => None,
    }
}

/// Une commande prête à partir : l'élément, le paramètre (dossier ou nom) déjà choisi
#[derive(Clone, PartialEq, Debug)]
pub struct Execution {
    pub commande: CommandeEl,
    pub sujet: Sujet,
    pub dossier: Option<String>,
    pub nom: Option<String>,
}

/// Ce que la palette dit une fois la commande finie
#[derive(Clone, PartialEq, Debug)]
pub struct Resultat {
    pub message: String,
    pub erreur: bool,
    /// Une entrée de l'historique peut la défaire : le bouton « Annuler » existe
    pub annulable: bool,
    /// L'entrée d'historique que cette commande a notée : « Annuler » défait celle-là (`App::defaire_entree`) et aucune autre
    pub entree: Option<u64>,
}

impl Resultat {
    pub fn ok(message: impl Into<String>, annulable: bool) -> Self {
        Self { message: message.into(), erreur: false, annulable, entree: None }
    }

    pub fn erreur(message: impl Into<String>) -> Self {
        Self { message: message.into(), erreur: true, annulable: false, entree: None }
    }

    /// Réussite qui a noté cette entrée d'historique : « Annuler » existe
    pub fn ok_avec_entree(message: impl Into<String>, entree: u64) -> Self {
        Self { message: message.into(), erreur: false, annulable: true, entree: Some(entree) }
    }

    /// Relie le résultat d'une opération à l'entrée d'historique qu'elle a vraiment notée. Sans entrée (suppression définitive,
    /// rien de fait), « Annuler » n'existe pas : il défairait une autre action. Une corbeille sans entrée n'est pas allée à la corbeille.
    pub fn avec_entree(mut self, genre: crate::operations::Genre, entree: Option<u64>) -> Self {
        if self.erreur {
            return self;
        }
        match entree {
            Some(id) if self.annulable => self.entree = Some(id),
            Some(_) => {}
            None => {
                if self.annulable && genre == crate::operations::Genre::Corbeille {
                    self.message = "Supprimé définitivement : cet élément n'a pas pu aller à la corbeille".to_string();
                }
                self.annulable = false;
            }
        }
        self
    }
}

/// « 1 élément » / « 3 éléments »
fn elements(n: usize) -> String {
    if n > 1 {
        format!("{n} éléments")
    } else {
        "1 élément".to_string()
    }
}

/// Message de fin d'une copie, d'un déplacement ou d'une mise à la corbeille. `annulation` : c'était un « Annuler » de la palette.
pub fn message_fin(genre: crate::operations::Genre, nombre: usize, dossier: &str, annule: bool, erreur: Option<&str>, annulation: bool) -> Resultat {
    use crate::operations::Genre;
    if let Some(e) = erreur {
        return Resultat::erreur(e.to_string());
    }
    if annule {
        return Resultat::erreur("Interrompu : ce qui était déjà fait reste en place");
    }
    if annulation {
        return Resultat::ok("Annulé", false);
    }
    let dest = nom_de(dossier);
    match genre {
        Genre::Copier => Resultat::ok(if nombre > 1 { format!("{} copiés dans {dest}", elements(nombre)) } else { format!("Copié dans {dest}") }, true),
        Genre::Deplacer => Resultat::ok(if nombre > 1 { format!("{} déplacés dans {dest}", elements(nombre)) } else { format!("Déplacé dans {dest}") }, true),
        Genre::Corbeille => Resultat::ok(if nombre > 1 { format!("{} mis à la corbeille", elements(nombre)) } else { "Mis à la corbeille".to_string() }, true),
        Genre::SupprimerDefinitivement => Resultat::ok("Supprimé", false),
    }
}

/// Phrase d'effet de l'aperçu : ce qui va arriver, au futur
pub fn phrase_effet(c: CommandeEl, s: &Sujet, dossier: &str) -> String {
    let (nom, pluriel) = (s.libelle(), s.genre == GenreSujet::Plusieurs);
    let ou = if dossier.is_empty() { String::new() } else { format!(" dans {}", nom_de(dossier)) };
    match c {
        CommandeEl::CopierVers => if pluriel { format!("{nom} seront copiés{ou}") } else { format!("{nom} sera copié{ou}") },
        CommandeEl::DeplacerVers => if pluriel { format!("{nom} seront déplacés{ou}") } else { format!("{nom} sera déplacé{ou}") },
        CommandeEl::CompresserVers => if pluriel { format!("{nom} seront compressés en .zip{ou}") } else { format!("{nom} sera compressé en .zip{ou}") },
        CommandeEl::Corbeille => phrase_corbeille(s, s.chemins.iter().any(|c| !disque_fixe(c))),
        _ => String::new(),
    }
}

/// Phrase d'effet de la corbeille. `definitif` : un des éléments n'est pas sur un disque fixe (réseau, amovible), où Windows supprime
/// sans passer par la corbeille : rien à annuler après, et la phrase ne le promet pas.
pub fn phrase_corbeille(s: &Sujet, definitif: bool) -> String {
    let (nom, pluriel) = (s.libelle(), s.genre == GenreSujet::Plusieurs);
    if definitif {
        return if pluriel { format!("{nom} seront supprimés définitivement : ce disque n'a pas de corbeille, Annuler ne sera pas possible.") } else { format!("{nom} sera supprimé définitivement : ce disque n'a pas de corbeille, Annuler ne sera pas possible.") };
    }
    if pluriel {
        format!("{nom} iront à la corbeille. Annuler restera possible après.")
    } else {
        format!("{nom} ira à la corbeille. Annuler restera possible après.")
    }
}

/// Emplacements où la sélection de la liste n'est pas faite de vrais fichiers : la corbeille et le panneau donnent les fichiers `$R…`
/// (ou des noms d'analyse), que la palette déplacerait ou jetterait au lieu de l'élément que l'on croit viser. Pas de sélection ici.
pub fn emplacement_sans_selection(e: &crate::dossier::Emplacement) -> bool {
    matches!(e, crate::dossier::Emplacement::Corbeille | crate::dossier::Emplacement::Panneau)
}

/// Un chemin qui peut bloquer le fil d'interface (partage réseau `\\hôte\…`, lecteur réseau monté) : la palette n'y copie ni n'y
/// déplace, et ne le propose pas en destination (les tests d'existence des noms pris, `MoveFileW`, `create_dir` y attendent le réseau).
pub fn destination_distante(chemin: &str) -> bool {
    if chemin.starts_with("\\\\") {
        return true;
    }
    let b = chemin.as_bytes();
    if b.len() < 2 || !b[0].is_ascii_alphabetic() || b[1] != b':' {
        return false;
    }
    let racine: Vec<u16> = format!("{}:\\", b[0] as char).encode_utf16().chain(std::iter::once(0)).collect();
    // SAFETY: `racine` est une chaîne UTF-16 terminée par 0 qui vit pendant l'appel
    unsafe { windows::Win32::Storage::FileSystem::GetDriveTypeW(windows::core::PCWSTR(racine.as_ptr())) == 4 }
}

// ── Nom (renommer, nouveau dossier) ──

/// État d'un nom saisi, vérifié en direct
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EtatNom {
    Libre,
    Vide,
    Invalide,
    /// Un élément porte déjà ce nom
    Existe,
    /// C'est le nom d'avant
    Identique,
    /// Le dossier n'a pas (encore) pu être lu : on ne sait pas, l'opération dira si le nom est pris
    NonVerifie,
}

/// `existants` : noms du dossier en minuscules (`None` : dossier pas lu). `ancien` : le nom d'avant (renommage).
pub fn etat_nom(nom: &str, ancien: Option<&str>, existants: Option<&HashSet<String>>) -> EtatNom {
    let n = nom.trim();
    if n.is_empty() {
        return EtatNom::Vide;
    }
    // Le nom nettoyé (espaces de fin retirés) : c'est lui qui part à l'opération, un espace tapé après « a » ne le rend pas invalide
    if !crate::operations::nom_valide(n) {
        return EtatNom::Invalide;
    }
    if ancien == Some(n) {
        return EtatNom::Identique;
    }
    let Some(ex) = existants else { return EtatNom::NonVerifie };
    // Changer seulement la casse (a.txt → A.txt) n'est pas un conflit : c'est le même élément
    let meme = ancien.is_some_and(|a| a.to_lowercase() == n.to_lowercase());
    if ex.contains(&n.to_lowercase()) && !meme {
        EtatNom::Existe
    } else {
        EtatNom::Libre
    }
}

impl EtatNom {
    pub fn raison(self) -> Option<&'static str> {
        match self {
            EtatNom::Libre | EtatNom::NonVerifie => None,
            EtatNom::Vide => Some("TAPEZ UN NOM"),
            EtatNom::Invalide => Some("NOM INVALIDE (INTERDITS : \\ / : * ? \" < > |)"),
            EtatNom::Existe => Some("UN ÉLÉMENT PORTE DÉJÀ CE NOM"),
            EtatNom::Identique => Some("C'EST DÉJÀ SON NOM"),
        }
    }
}

// ── Destinations ──

/// Ce que l'étape « destination » sait des dossiers connus
pub struct Lieux<'a> {
    pub autre_panneau: Option<&'a str>,
    pub recents: &'a [String],
    pub epingles: &'a [String],
    /// Lieux du volet : (nom, chemin)
    pub volet: &'a [(String, String)],
    pub courant: Option<&'a str>,
}

#[derive(Clone, PartialEq, Debug)]
pub struct Destination {
    pub chemin: String,
    pub libelle: String,
    pub detail: String,
    pub groupe: &'static str,
    pub icone: Icone,
}

/// Dossier d'un chemin, sans barre finale (comparaison sans casse)
fn cle(c: &str) -> String {
    c.trim_end_matches('\\').to_lowercase()
}

/// Un dossier peut-il être une destination ? Pas une recherche enregistrée (`?motif`), pas un chemin réseau d'expérimental, pas vide
fn est_dossier_reel(c: &str) -> bool {
    let b = c.as_bytes();
    !c.is_empty() && !c.starts_with('?') && (c.starts_with("\\\\") || (b.len() >= 2 && b[0].is_ascii_alphabetic() && b[1] == b':'))
}

/// Les destinations dans l'ordre voulu : l'autre panneau, récents, favoris (épinglés puis lieux du volet), recherche (ce qui est tapé),
/// puis le dossier courant et son parent. Un dossier n'apparaît qu'une fois ; `requete` filtre (`score`), `trouves` sont les dossiers
/// rapportés par la recherche asynchrone.
pub fn destinations(l: &Lieux, requete: &str, trouves: &[String]) -> Vec<Destination> {
    let q = requete.trim();
    let mut v: Vec<Destination> = Vec::new();
    let mut vus: HashSet<String> = HashSet::new();
    let mut ajouter = |chemin: &str, libelle: String, detail: String, groupe: &'static str, icone: Icone, filtrer: bool| {
        if !est_dossier_reel(chemin) || !vus.insert(cle(chemin)) {
            return;
        }
        if filtrer && !q.is_empty() && crate::palette::score(&libelle, q).is_none() && !chemin.to_lowercase().contains(&q.to_lowercase()) {
            vus.remove(&cle(chemin));
            return;
        }
        v.push(Destination { chemin: chemin.to_string(), libelle, detail, groupe, icone });
    };
    if let Some(a) = l.autre_panneau {
        ajouter(a, format!("AUTRE PANNEAU · {}", crate::dossier::nom_affiche(a).to_uppercase()), a.to_string(), "AUTRE PANNEAU", Icone::Disque, true);
    }
    for r in l.recents {
        ajouter(r, crate::dossier::nom_affiche(r).to_uppercase(), format!("RÉCENT · {r}"), "RÉCENTS", Icone::Horloge, true);
    }
    for e in l.epingles {
        ajouter(e, crate::dossier::nom_affiche(e).to_uppercase(), format!("ÉPINGLÉ · {e}"), "FAVORIS", Icone::Dossier, true);
    }
    for (nom, chemin) in l.volet {
        ajouter(chemin, nom.to_uppercase(), chemin.clone(), "FAVORIS", Icone::Dossier, true);
    }
    // Recherche : seulement ce qui est tapé, jamais l'index vide
    if !q.is_empty() {
        for t in trouves {
            ajouter(t, crate::dossier::nom_affiche(t).to_uppercase(), t.clone(), "RECHERCHE", Icone::Dossier, false);
        }
    }
    if let Some(c) = l.courant {
        ajouter(c, format!("{} · CE DOSSIER", crate::dossier::nom_affiche(c).to_uppercase()), c.to_string(), "ICI", Icone::Dossier, true);
        let p = crate::dossier::dossier_de(c.trim_end_matches('\\'));
        if !p.eq_ignore_ascii_case(c.trim_end_matches('\\')) {
            ajouter(p, format!("{} · PARENT", crate::dossier::nom_affiche(p).to_uppercase()), p.to_string(), "ICI", Icone::Dossier, true);
        }
    }
    v
}

// ── Lecture d'un dossier (fil de fond) ──

/// Ce qu'on sait d'un dossier lu pour l'aperçu et la vérification des noms
#[derive(Clone, Debug, PartialEq)]
pub struct Lecture {
    pub chemin: String,
    /// Faux : partage réseau, lecteur amovible ou optique… jamais lu (l'aperçu le dit)
    pub lu: bool,
    /// Premiers éléments (nom, taille ou vide, dossier ?), dossiers d'abord
    pub apercu: Vec<(String, String, bool)>,
    /// Sous-dossiers visibles, par nom
    pub sous_dossiers: Vec<String>,
    /// Tous les noms (minuscules, cachés compris) : de quoi détecter un conflit
    pub noms: HashSet<String>,
    pub total: usize,
    pub libre: Option<u64>,
}

impl Lecture {
    pub fn non_lue(chemin: &str) -> Self {
        Self { chemin: chemin.to_string(), lu: false, apercu: Vec::new(), sous_dossiers: Vec::new(), noms: HashSet::new(), total: 0, libre: None }
    }
}

/// Le chemin est-il sur un disque fixe ? Un lecteur réseau monté, optique ou amovible vide peut bloquer plusieurs secondes.
pub fn disque_fixe(chemin: &str) -> bool {
    let b = chemin.as_bytes();
    if b.len() < 2 || !b[0].is_ascii_alphabetic() || b[1] != b':' {
        return false;
    }
    let racine: Vec<u16> = format!("{}:\\", b[0] as char).encode_utf16().chain(std::iter::once(0)).collect();
    // SAFETY: `racine` est une chaîne UTF-16 terminée par 0 qui vit pendant l'appel
    unsafe { windows::Win32::Storage::FileSystem::GetDriveTypeW(windows::core::PCWSTR(racine.as_ptr())) == 3 }
}

/// Lit un dossier pour l'aperçu de la palette. BLOQUANT : à appeler dans un fil de fond seulement. Hors disque fixe : `non_lue`.
pub fn lire_dossier(chemin: &str) -> Lecture {
    if !disque_fixe(chemin) {
        return Lecture::non_lue(chemin);
    }
    let Ok(lecture) = std::fs::read_dir(chemin) else { return Lecture::non_lue(chemin) };
    let mut l = Lecture { lu: true, ..Lecture::non_lue(chemin) };
    let mut tous: Vec<(String, String, bool)> = Vec::new();
    for e in lecture.flatten().take(20_000) {
        let nom = e.file_name().to_string_lossy().to_string();
        l.noms.insert(nom.to_lowercase());
        l.total += 1;
        let Ok(m) = e.metadata() else { continue };
        {
            use std::os::windows::fs::MetadataExt;
            // Cachés et système ($Recycle.Bin, System Volume Information…) : ni dans l'aperçu ni dans la navigation
            if m.file_attributes() & 0x6 != 0 {
                continue;
            }
        }
        if m.is_dir() {
            l.sous_dossiers.push(nom.clone());
        }
        tous.push((nom, if m.is_dir() { String::new() } else { crate::systeme::taille(m.len()) }, m.is_dir()));
    }
    tous.sort_by(|a, b| b.2.cmp(&a.2).then_with(|| a.0.to_lowercase().cmp(&b.0.to_lowercase())));
    tous.truncate(6);
    l.apercu = tous;
    l.sous_dossiers.sort_by_key(|n| n.to_lowercase());
    l.sous_dossiers.truncate(500);
    let (mut libre, mut total) = (0u64, 0u64);
    // SAFETY: `HSTRING` terminée par 0 qui vit pendant l'appel ; les deux sorties sont des variables locales
    let ok = unsafe { windows::Win32::Storage::FileSystem::GetDiskFreeSpaceExW(&windows::core::HSTRING::from(chemin), Some(&mut libre), Some(&mut total), None).is_ok() };
    l.libre = ok.then_some(libre);
    l
}

/// Éléments récents de l'étape 1 : les dernières sélections laissées dans chaque dossier (`reprises`, « dossier\tnom »), puis les
/// dossiers visités (`recents`). Que du réel : `sonde` dit si le chemin existe et si c'est un dossier (`None` : absent, on l'omet).
/// Jamais de « reprendre » ni de commande ici. Un doublon n'est gardé qu'une fois ; 12 au plus.
pub fn elements_recents_avec(reprises: &[String], recents: &[String], sonde: &dyn Fn(&str) -> Option<bool>) -> Vec<(String, bool, Icone)> {
    let mut v: Vec<(String, bool, Icone)> = Vec::new();
    let mut vus: HashSet<String> = HashSet::new();
    let chemins = reprises
        .iter()
        .map(|x| match x.split_once('\t') {
            Some((d, nom)) if !nom.is_empty() => crate::dossier::joindre(d, nom),
            Some((d, _)) => d.to_string(),
            None => x.clone(),
        })
        .chain(recents.iter().filter(|r| est_dossier_reel(r)).cloned());
    for c in chemins {
        if v.len() >= 12 || !vus.insert(cle(&c)) {
            continue;
        }
        let Some(dossier) = sonde(&c) else { continue };
        let icone = if dossier { Icone::Dossier } else { crate::icones::pour_extension(&nom_de(&c).rsplit_once('.').map(|x| x.1.to_uppercase()).unwrap_or_default()) };
        v.push((c, dossier, icone));
    }
    v
}

/// `elements_recents_avec` sur le disque : seulement les disques fixes (un partage réseau ou un lecteur amovible peut bloquer le fil
/// d'interface plusieurs secondes), et seulement ce qui existe encore.
pub fn elements_recents(reprises: &[String], recents: &[String]) -> Vec<(String, bool, Icone)> {
    elements_recents_avec(reprises, recents, &|c| if disque_fixe(c) { std::fs::metadata(c).ok().map(|m| m.is_dir()) } else { None })
}

/// Éléments du sujet dont le nom existe déjà dans la destination (comparaison sans casse)
pub fn conflits_de(noms: &[String], existants: &HashSet<String>) -> Vec<String> {
    noms.iter().filter(|n| existants.contains(&n.to_lowercase())).cloned().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fichier(c: &str) -> Sujet {
        Sujet::un(c.to_string(), false, Icone::Pdf)
    }

    #[test]
    fn noms_et_libelles_du_sujet() {
        assert_eq!(nom_de(r"C:\Docs\rapport.pdf"), "rapport.pdf");
        assert_eq!(nom_de(r"C:\Docs\"), "Docs");
        assert_eq!(nom_de(r"C:\"), r"C:\");
        assert_eq!(fichier(r"C:\a\b.txt").libelle(), "b.txt");
        let s = Sujet::plusieurs(vec![r"C:\a".into(), r"C:\b".into(), r"C:\c".into()]);
        assert_eq!(s.libelle(), "3 éléments");
        assert_eq!(s.noms(), ["a", "b", "c"]);
        assert_eq!(fichier(r"C:\a\b.txt").parent().as_deref(), Some(r"C:\a"));
    }

    #[test]
    fn commandes_disponibles_selon_le_type_d_element() {
        let f = fichier(r"C:\a\b.txt");
        let d = Sujet::un(r"C:\a".into(), true, Icone::Dossier);
        let p = Sujet::plusieurs(vec![r"C:\a".into(), r"C:\b".into()]);
        let o = Sujet::dossier_ouvert(r"C:\a".into());
        // Un fichier : pas d'onglet ni de dossier dedans
        assert_eq!(indisponible(CommandeEl::OuvrirOnglet, &f), Some("UN DOSSIER SEULEMENT"));
        assert_eq!(indisponible(CommandeEl::NouveauDossier, &f), Some("UN DOSSIER SEULEMENT"));
        assert_eq!(indisponible(CommandeEl::Renommer, &f), None);
        assert_eq!(indisponible(CommandeEl::CopierVers, &f), None);
        // Un dossier : tout s'applique
        for d_ in TABLE {
            assert_eq!(indisponible(d_.commande, &d), None, "{:?}", d_.commande);
        }
        // Plusieurs : renommer, ouvrir, nouveau dossier demandent un seul élément ; copier, déplacer, corbeille, zip restent
        for c in [CommandeEl::Renommer, CommandeEl::Ouvrir, CommandeEl::NouveauDossier, CommandeEl::OuvrirOnglet] {
            assert_eq!(indisponible(c, &p), Some("UN SEUL ÉLÉMENT"), "{c:?}");
        }
        for c in [CommandeEl::CopierVers, CommandeEl::DeplacerVers, CommandeEl::Corbeille, CommandeEl::CompresserVers, CommandeEl::CopierChemin] {
            assert_eq!(indisponible(c, &p), None, "{c:?}");
        }
        // Le dossier ouvert ne se déplace, ni ne se renomme, ni ne se jette ; on peut le copier
        for c in [CommandeEl::Renommer, CommandeEl::DeplacerVers, CommandeEl::Corbeille] {
            assert!(indisponible(c, &o).is_some(), "{c:?}");
        }
        assert_eq!(indisponible(CommandeEl::CopierVers, &o), None);
        assert_eq!(indisponible(CommandeEl::NouveauDossier, &o), None);
    }

    #[test]
    fn chaque_commande_dit_ce_qu_elle_demande() {
        assert_eq!(def(CommandeEl::CopierVers).param, Param::Destination);
        assert_eq!(def(CommandeEl::DeplacerVers).param, Param::Destination);
        assert_eq!(def(CommandeEl::CompresserVers).param, Param::Destination);
        assert_eq!(def(CommandeEl::Renommer).param, Param::Nom);
        assert_eq!(def(CommandeEl::NouveauDossier).param, Param::Nom);
        assert_eq!(def(CommandeEl::Corbeille).param, Param::Confirmation);
        assert_eq!(def(CommandeEl::Ouvrir).param, Param::Aucun);
        assert_eq!(def(CommandeEl::Renommer).raccourci, "F2");
        assert_eq!(Param::Destination.annonce(), "DESTINATION ›");
        assert_eq!(libelle_court(CommandeEl::CopierVers), "Copier vers");
        assert_eq!(libelle_court(CommandeEl::CompresserVers), "Compresser vers");
        // Chaque variante a sa ligne dans la table, une seule fois
        for d in TABLE {
            assert_eq!(TABLE.iter().filter(|x| x.commande == d.commande).count(), 1, "{:?}", d.commande);
        }
    }

    #[test]
    fn destinations_dans_l_ordre_voulu_sans_doublon() {
        let recents = vec![r"C:\Users\moi\Documents".to_string(), r"D:\Archives".to_string()];
        let epingles = vec![r"C:\Users\moi\Bureau".to_string(), r"C:\Users\moi\Documents".to_string(), "?contrat".to_string()];
        let volet = vec![("Téléchargements".to_string(), r"C:\Users\moi\Downloads".to_string())];
        let l = Lieux { autre_panneau: Some(r"E:\Panneau"), recents: &recents, epingles: &epingles, volet: &volet, courant: Some(r"C:\Users\moi\Projets") };
        let v = destinations(&l, "", &[]);
        let ordre: Vec<(&str, &str)> = v.iter().map(|d| (d.groupe, d.chemin.as_str())).collect();
        assert_eq!(
            ordre,
            [
                ("AUTRE PANNEAU", r"E:\Panneau"),
                ("RÉCENTS", r"C:\Users\moi\Documents"),
                ("RÉCENTS", r"D:\Archives"),
                ("FAVORIS", r"C:\Users\moi\Bureau"),
                ("FAVORIS", r"C:\Users\moi\Downloads"),
                ("ICI", r"C:\Users\moi\Projets"),
                ("ICI", r"C:\Users\moi"),
            ],
            "autre panneau, récents, favoris, dossier courant, parent ; Documents n'est pas répété ; la recherche enregistrée n'est pas un dossier"
        );
        // Pas d'autre panneau (simple panneau) : la liste commence aux récents
        let sans = Lieux { autre_panneau: None, ..l };
        assert_eq!(destinations(&sans, "", &[])[0].groupe, "RÉCENTS");
    }

    #[test]
    fn destinations_tapees_filtrent_et_ajoutent_la_recherche_avant_le_dossier_courant() {
        let recents = vec![r"C:\Users\moi\Documents".to_string()];
        let l = Lieux { autre_panneau: None, recents: &recents, epingles: &[], volet: &[], courant: Some(r"C:\Users\moi\Projets") };
        let trouves = vec![r"D:\Travail\Documentation".to_string(), r"C:\Users\moi\Documents".to_string()];
        let v = destinations(&l, "doc", &trouves);
        let ordre: Vec<(&str, &str)> = v.iter().map(|d| (d.groupe, d.chemin.as_str())).collect();
        assert_eq!(ordre, [("RÉCENTS", r"C:\Users\moi\Documents"), ("RECHERCHE", r"D:\Travail\Documentation")], "Projets et son parent ne contiennent pas « doc » ; Documents vient des récents, pas de la recherche");
        // Rien de tapé : la recherche ne montre rien même si des résultats traînent
        assert!(destinations(&l, "", &trouves).iter().all(|d| d.groupe != "RECHERCHE"));
    }

    #[test]
    fn nom_libre_existant_invalide_identique() {
        let ex: HashSet<String> = ["a.txt".to_string(), "photos".to_string()].into();
        assert_eq!(etat_nom("", None, Some(&ex)), EtatNom::Vide);
        assert_eq!(etat_nom("  ", None, Some(&ex)), EtatNom::Vide);
        assert_eq!(etat_nom("a?b", None, Some(&ex)), EtatNom::Invalide);
        assert_eq!(etat_nom("fin.", None, Some(&ex)), EtatNom::Invalide);
        assert_eq!(etat_nom("nouveau", None, Some(&ex)), EtatNom::Libre);
        assert_eq!(etat_nom("Photos", None, Some(&ex)), EtatNom::Existe, "sans tenir compte de la casse");
        assert_eq!(etat_nom("a.txt", Some("a.txt"), Some(&ex)), EtatNom::Identique);
        assert_eq!(etat_nom("A.txt", Some("a.txt"), Some(&ex)), EtatNom::Libre, "ne changer que la casse est permis");
        assert_eq!(etat_nom("photos", Some("a.txt"), Some(&ex)), EtatNom::Existe);
        assert_eq!(etat_nom("x", None, None), EtatNom::NonVerifie, "dossier pas lu : l'opération tranchera");
        assert!(EtatNom::Existe.raison().is_some() && EtatNom::Libre.raison().is_none() && EtatNom::NonVerifie.raison().is_none());
    }

    #[test]
    fn conflits_et_phrases_d_effet() {
        let ex: HashSet<String> = ["rapport.pdf".to_string()].into();
        assert_eq!(conflits_de(&["Rapport.PDF".into(), "autre.txt".into()], &ex), ["Rapport.PDF"]);
        assert!(conflits_de(&["autre.txt".into()], &ex).is_empty());
        let f = fichier(r"C:\a\rapport.pdf");
        assert_eq!(phrase_effet(CommandeEl::CopierVers, &f, r"C:\Docs"), "rapport.pdf sera copié dans Docs");
        assert_eq!(phrase_effet(CommandeEl::DeplacerVers, &f, r"C:\Docs"), "rapport.pdf sera déplacé dans Docs");
        let p = Sujet::plusieurs(vec!["a".into(), "b".into()]);
        assert_eq!(phrase_effet(CommandeEl::CopierVers, &p, r"C:\Docs"), "2 éléments seront copiés dans Docs");
        assert!(phrase_effet(CommandeEl::Corbeille, &f, "").starts_with("rapport.pdf ira à la corbeille"));
    }

    #[test]
    fn messages_de_fin_copie_deplacement_corbeille_annulation_erreur() {
        use crate::operations::Genre;
        let r = message_fin(Genre::Copier, 1, r"C:\Docs", false, None, false);
        assert_eq!((r.message.as_str(), r.annulable, r.erreur), ("Copié dans Docs", true, false));
        assert_eq!(message_fin(Genre::Copier, 3, r"C:\Docs", false, None, false).message, "3 éléments copiés dans Docs");
        assert_eq!(message_fin(Genre::Deplacer, 1, r"C:\Docs", false, None, false).message, "Déplacé dans Docs");
        assert_eq!(message_fin(Genre::Corbeille, 1, "", false, None, false).message, "Mis à la corbeille");
        // Un « Annuler » de la palette : « Annulé », et on ne peut pas annuler l'annulation ici (Ctrl+Y refait)
        let a = message_fin(Genre::Corbeille, 1, "", false, None, true);
        assert_eq!((a.message.as_str(), a.annulable), ("Annulé", false));
        let e = message_fin(Genre::Copier, 1, r"C:\Docs", false, Some("DISQUE PLEIN"), false);
        assert!(e.erreur && !e.annulable && e.message == "DISQUE PLEIN");
        assert!(message_fin(Genre::Copier, 1, r"C:\Docs", true, None, false).erreur);
    }

    #[test]
    fn nom_nettoye_reserve_et_pluriels() {
        let ex: HashSet<String> = HashSet::new();
        assert_eq!(etat_nom("rapport ", None, Some(&ex)), EtatNom::Libre, "l'espace de fin est retiré avant la validation");
        assert_eq!(etat_nom("con", None, Some(&ex)), EtatNom::Invalide);
        assert_eq!(etat_nom("NUL.txt", None, Some(&ex)), EtatNom::Invalide);
        assert_eq!(etat_nom("fin.", None, Some(&ex)), EtatNom::Invalide);
        let p = Sujet::plusieurs(vec![r"C:\a\x".into(), r"C:\a\y".into(), r"C:\a\z".into()]);
        assert_eq!(phrase_effet(CommandeEl::CompresserVers, &p, r"C:\Docs"), "3 éléments seront compressés en .zip dans Docs");
        assert_eq!(phrase_effet(CommandeEl::CompresserVers, &fichier(r"C:\a\b.txt"), ""), "b.txt sera compressé en .zip");
        assert_eq!(def(CommandeEl::Ouvrir).raccourci, "ENTRÉE");
    }

    #[test]
    fn corbeille_definitive_ne_promet_pas_annuler_et_compresser_exige_un_parent() {
        let f = fichier(r"\\nas\partage\a.txt");
        assert!(phrase_corbeille(&f, true).contains("définitivement") && !phrase_corbeille(&f, true).contains("restera possible"));
        assert!(phrase_corbeille(&f, false).contains("Annuler restera possible"));
        let p = Sujet::plusieurs(vec![r"C:\a\x".into(), r"C:\b\y".into()]);
        assert_eq!(indisponible(CommandeEl::CompresserVers, &p), Some("ÉLÉMENTS DE DOSSIERS DIFFÉRENTS"));
        let q = Sujet::plusieurs(vec![r"C:\a\x".into(), r"c:\A\y".into()]);
        assert_eq!(indisponible(CommandeEl::CompresserVers, &q), None);
        assert!(destination_distante(r"\\nas\partage"));
        assert!(!destination_distante("relatif"));
    }

    #[test]
    fn corbeille_et_panneau_ne_donnent_pas_de_selection() {
        use crate::dossier::Emplacement;
        assert!(emplacement_sans_selection(&Emplacement::Corbeille));
        assert!(emplacement_sans_selection(&Emplacement::Panneau));
        assert!(!emplacement_sans_selection(&Emplacement::dossier(r"C:\Docs")));
    }

    #[test]
    fn resultat_relie_a_l_entree_d_historique_ou_pas_annulable() {
        use crate::operations::Genre;
        let r = message_fin(Genre::Copier, 1, r"C:\Docs", false, None, false).avec_entree(Genre::Copier, Some(12));
        assert_eq!((r.annulable, r.entree), (true, Some(12)));
        let sans = message_fin(Genre::Corbeille, 1, "", false, None, false).avec_entree(Genre::Corbeille, None);
        assert!(!sans.annulable && sans.entree.is_none() && sans.message.contains("définitivement"));
        let sans = message_fin(Genre::Copier, 1, r"C:\Docs", false, None, false).avec_entree(Genre::Copier, None);
        assert!(!sans.annulable);
        let e = Resultat::erreur("x").avec_entree(Genre::Copier, Some(1));
        assert!(e.entree.is_none() && e.erreur);
    }

    #[test]
    fn etapes_et_rangs_des_puces() {
        assert_eq!([Etape::Element.rang(), Etape::Commande.rang(), Etape::Parametre.rang()], [0, 1, 2]);
        assert_eq!(Etape::Progression.rang(), 3);
        assert_eq!(Etape::Resultat.rang(), 3);
    }

    #[test]
    fn lecture_d_un_dossier_local_et_refus_du_reseau() {
        let dir = std::env::temp_dir().join(format!("sf-lot15c-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("zeta")).unwrap();
        std::fs::create_dir_all(dir.join("Alpha")).unwrap();
        for n in ["b.txt", "A.txt"] {
            std::fs::write(dir.join(n), vec![0u8; 2048]).unwrap();
        }
        let l = lire_dossier(dir.to_str().unwrap());
        // Le dossier temporaire peut être sur un disque non « fixe » (RAM, réseau) : le test ne l'exige que s'il l'est
        if disque_fixe(dir.to_str().unwrap()) {
            assert!(l.lu && l.total == 4);
            let noms: Vec<&str> = l.apercu.iter().map(|x| x.0.as_str()).collect();
            assert_eq!(noms, ["Alpha", "zeta", "A.txt", "b.txt"], "dossiers d'abord, puis par nom sans tenir compte de la casse");
            assert_eq!(l.sous_dossiers, ["Alpha", "zeta"]);
            assert!(l.noms.contains("a.txt") && l.noms.contains("alpha"), "noms en minuscules pour les conflits");
            assert_eq!(l.apercu[2].1, "2,0 KO");
            assert!(l.libre.is_some());
        }
        let _ = std::fs::remove_dir_all(&dir);
        // Partage réseau, chemin sans lecteur, dossier absent : « non lu », jamais d'attente
        for c in [r"\\serveur\partage", "", "relatif", dir.to_str().unwrap()] {
            assert!(!lire_dossier(c).lu, "{c}");
        }
        assert!(!disque_fixe(r"\\serveur\partage"));
    }
}
