// Page des réglages (Ctrl+, ou bouton en bas du volet) : une catégorie à la fois, en cartes (maquette v6), au clavier
// ou à la souris, et le formulaire « ajouter un stockage ». Le dessin est dans rendu.rs, les effets dans commandes/reglages_page.rs.
//
// Le modèle reste une liste plate de lignes (`lignes()`), rangée par catégorie : chaque catégorie commence par un
// `Element::Section` portant son nom. La page affichée (`Page::categorie`) ne montre que les lignes de sa section ;
// `focus` reste un indice dans la liste complète, donc `activer_reglage(k)` n'a pas changé.

use std::ops::Range;

use windows::Win32::Graphics::Direct2D::Common::D2D_RECT_F;

use crate::edition::Edition;
use crate::icones::Icone;
use crate::reglages::{Reglages, Sorte};
use crate::rendu::rect;
use crate::stockages::Lieu;
use crate::theme::*;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Element {
    Section(&'static str),
    Detecte(usize),
    Ajoute(usize),
    Ajouter,
    Depart,
    RestaurerOnglets,
    ParDefaut,
    WinE,
    Caches,
    DatesRelatives,
    EtatCloud,
    Apercu,
    UnClic,
    Ressort,
    NettoyageJours,
    LimiteReseau,
    NettoyerCopies,
    Arbre,
    NettoyerArchives,
    /// Téléchargements du navigateur : pastille et marque NOUVEAU
    TelechargementsNavigateur,
    RangerAuto,
    CopieVerifiee,
    ConflitDefaut,
    /// Règle de rangement (rang dans les réglages)
    Regle(usize),
    Animations,
    Sons,
    Fichier,
    Exporter,
    Importer,
    Reinitialiser,
    Version,
    /// Section du volet (rang dans rendu::SECTIONS_VOLET) : visible, repliée, masquée
    SectionVolet(u8),
    /// Dossier épinglé (rang dans les réglages)
    Epingle(usize),
    /// Thème : auto, sombre, clair ou celui de Windows
    Theme,
    TailleLignes,
    Extensions,
    Contraste,
    /// Mouvement complet ou réduit (lot 10B)
    Mouvement,
    /// Trame de points derrière la fenêtre
    Trame,
    /// Reflet qui suit le curseur
    RefletSurvol,
    /// Les deux yeux de Pixel dans la barre d'état
    YeuxPixel,
    /// Pixel selon l'heure : Auto, Jour, Soir, Nuit
    PixelHeure,
    VoletDense,
    VueDefaut,
    /// Disque fixe (rang dans index::disques) : dans la recherche ou non
    DisqueIndexe(usize),
    /// Dossier exclu de la recherche (rang dans les réglages)
    Exclu(usize),
    Reconstruire,
    RetenirRecents,
    EffacerRecents,
    EffacerIndex,
    /// Ligne d'information (raccourcis, dossier des données) : rien à choisir
    Info,
    PoidsSonore,
    /// Interrupteur des fonctions expérimentales (SFTP, WebDAV, sauvegarde NAS, MTP)
    Experimental,
}

impl Element {
    pub fn choisissable(self) -> bool {
        !matches!(self, Element::Section(_) | Element::Version | Element::Info)
    }

    /// Icône de la tuile de la carte (glyphes existants de `icones.rs`, aucun nouveau motif)
    pub fn icone(self) -> Icone {
        match self {
            Element::Section(_) | Element::Theme | Element::Reconstruire | Element::Reinitialiser => Icone::Reglages,
            Element::Detecte(_) | Element::EtatCloud => Icone::Nuage,
            Element::Ajoute(_) | Element::LimiteReseau => Icone::Serveur,
            Element::Ajouter
            | Element::ParDefaut
            | Element::Arbre
            | Element::Epingle(_)
            | Element::Ressort
            | Element::RangerAuto
            | Element::Regle(_)
            | Element::Exclu(_) => Icone::Dossier,
            Element::Depart => Icone::Maison,
            Element::RestaurerOnglets | Element::DatesRelatives | Element::NettoyageJours | Element::RetenirRecents | Element::EffacerRecents => Icone::Horloge,
            Element::WinE => Icone::Executable,
            Element::TailleLignes | Element::VoletDense | Element::VueDefaut => Icone::Tableur,
            Element::Contraste | Element::Apercu | Element::Trame | Element::RefletSurvol => Icone::Image,
            Element::Mouvement => Icone::Video,
            Element::YeuxPixel => Icone::Code,
            Element::PixelHeure => Icone::Horloge,
            Element::SectionVolet(_) | Element::Fichier | Element::Info => Icone::Texte,
            Element::Extensions | Element::Caches | Element::NettoyerCopies => Icone::Fichier,
            Element::UnClic => Icone::Raccourci,
            Element::ConflitDefaut => Icone::Document,
            Element::CopieVerifiee | Element::DisqueIndexe(_) | Element::EffacerIndex => Icone::Disque,
            Element::Experimental | Element::Version => Icone::Code,
            Element::TelechargementsNavigateur | Element::Importer => Icone::Telechargement,
            Element::NettoyerArchives | Element::Exporter => Icone::Archive,
            Element::Animations => Icone::Video,
            Element::Sons | Element::PoidsSonore => Icone::Musique,
        }
    }
}

/// Valeur affichée à droite d'une carte
pub enum Valeur {
    Rien,
    Interrupteur(bool),
    /// Choix de plus de trois valeurs : « ‹ valeur › », changé par les flèches
    Choix(String),
    /// Choix de deux à quatre valeurs, toutes visibles : options et rang de l'option active
    Segments(Vec<String>, usize),
    Bouton(&'static str),
    /// Texte à droite, sans flèches (touches d'un raccourci)
    Texte(String),
}

pub struct Ligne {
    pub element: Element,
    pub libelle: String,
    pub detail: String,
    pub valeur: Valeur,
}

fn oui(b: bool) -> Valeur {
    Valeur::Interrupteur(b)
}

/// Mots que `casse_phrase` laisse en capitales (sigles, formats)
const SIGLES: [&str; 14] = ["PDF", "SFTP", "NAS", "SHA-256", "OCR", "ZIP", "RAR", "7Z", "SMB", "DIP", "TOML", "GIT", "MTP", "PC"];

/// Mots de la palette (sans accents) dont `casse_phrase` remet les accents
const ACCENTS: [(&str, &str); 14] = [
    ("entree", "entrée"),
    ("proprietes", "propriétés"),
    ("operations", "opérations"),
    ("selectionner", "sélectionner"),
    ("selection", "sélection"),
    ("etre", "être"),
    ("apercu", "aperçu"),
    ("derniere", "dernière"),
    ("caches", "cachés"),
    ("ferme", "fermé"),
    ("annule", "annulé"),
    ("a", "à"),
    ("regles", "règles"),
    ("defaut", "défaut"),
];

/// « DOSSIER OU LECTEUR » devient « Dossier ou lecteur » : les libellés du reste de l'application sont en capitales,
/// la page des réglages est en casse normale (maquette v6). Les accents absents des sources ne reviennent pas.
pub fn casse_phrase(t: &str) -> String {
    let mots: Vec<String> = t
        .split(' ')
        .map(|m| {
            let coeur = m.trim_matches(|c: char| !c.is_alphanumeric() && c != '-');
            if SIGLES.contains(&coeur) {
                return m.to_string();
            }
            let bas = m.to_lowercase();
            // Les libellés de la palette n'ont pas d'accents : ceux des mots qui reviennent dans les raccourcis sont remis
            let (avant, mot) = bas.split_once('\'').map_or(("", bas.as_str()), |(a, b)| (a, b));
            let coeur = mot.trim_matches(|c: char| !c.is_alphanumeric());
            if let Some((_, bon)) = ACCENTS.iter().find(|(sans, _)| *sans == coeur) {
                let accentue = mot.replace(coeur, bon);
                return if avant.is_empty() { accentue } else { format!("{avant}'{accentue}") };
            }
            match bas.as_str() {
                x if x.contains("spacefiles") => x.replace("spacefiles", "SpaceFiles"),
                x if x.contains("zimaos") => x.replace("zimaos", "ZimaOS"),
                x if x.contains("webdav") => x.replace("webdav", "WebDAV"),
                x if x.contains("windows") => x.replace("windows", "Windows"),
                x => x.to_string(),
            }
        })
        .collect();
    let mut s = mots.join(" ").replace("vs code", "VS Code");
    if let Some((i, c)) = s.char_indices().find(|(_, c)| c.is_alphabetic()) {
        let maj: String = c.to_uppercase().collect();
        s.replace_range(i..i + c.len_utf8(), &maj);
    }
    s
}

/// « CTRL+MAJ+T » devient « Ctrl+Maj+T » : une touche d'une lettre ou d'un signe reste telle quelle
pub fn touche_lisible(t: &str) -> String {
    t.split('+')
        .map(|p| {
            let n = p.chars().count();
            if n <= 1 || p.chars().any(|c| c.is_ascii_digit()) {
                return p.to_string();
            }
            let mut c = p.chars();
            let t = c.next().map_or_else(String::new, |p1| p1.to_uppercase().collect::<String>() + &c.as_str().to_lowercase());
            if t == "Entree" { "Entrée".to_string() } else { t }
        })
        .collect::<Vec<_>>()
        .join("+")
}

/// Ordre des quatre segments du thème (le même que la maquette : Auto, Système, Sombre, Clair)
pub const THEMES_SEGMENTS: [ChoixTheme; 4] = [ChoixTheme::Auto, ChoixTheme::Systeme, ChoixTheme::Sombre, ChoixTheme::Clair];

fn segments(options: &[&str], actif: usize) -> Valeur {
    Valeur::Segments(options.iter().map(|s| s.to_string()).collect(), actif)
}

/// Lignes de la page, dans l'ordre d'affichage : une section par catégorie du rail, dans l'ordre de `CATEGORIES`
pub fn lignes(r: &Reglages, detectes: &[Lieu], ajoutes: &[Lieu], par_defaut: bool, win_e: bool) -> Vec<Ligne> {
    let mut v = Vec::new();
    let mut l = |element, libelle: &str, detail: &str, valeur| v.push(Ligne { element, libelle: libelle.into(), detail: detail.into(), valeur });

    l(Element::Section(CATEGORIES[0]), "", "", Valeur::Rien);
    let theme = THEMES_SEGMENTS.iter().position(|&t| t == r.theme).unwrap_or(0);
    l(Element::Theme, "Thème", "Auto suit l'avis partagé par les autres applications, sinon sombre. Système suit Windows.", segments(&THEMES_SEGMENTS.map(ChoixTheme::libelle), theme));
    // Rang 0 = Confort (40 DIP) : `taille_lignes` vaut 1 pour Confort dans le fichier de réglages
    l(Element::TailleLignes, "Densité", "Hauteur des lignes de la liste : confort 40 DIP, compact 30 DIP.", segments(&["Confort", "Compact"], usize::from(r.taille_lignes == 0)));
    l(Element::Contraste, "Contraste renforcé", "Textes, traits et survols plus francs ; suivi automatiquement sous le thème Contraste élevé de Windows.", oui(r.contraste));
    l(Element::Mouvement, "Mouvement", "Réduit coupe les ressorts, le reflet et les dissolutions.", segments(&["Complet", "Réduit"], usize::from(r.mouvement_reduit)));
    l(Element::Trame, "Trame de points", "Le motif discret derrière la fenêtre.", oui(r.trame));
    l(Element::RefletSurvol, "Reflet au survol", "Une lumière douce suit le curseur sur ce qui se clique.", oui(r.reflet));
    l(Element::YeuxPixel, "Yeux de Pixel", "Les deux yeux dans la barre d'état.", oui(r.yeux_pixel));
    l(
        Element::PixelHeure,
        "Pixel selon l'heure",
        "Cyan le jour, ambre le soir, yeux fermés la nuit.",
        segments(&crate::pixel::ChoixPixel::TOUS.map(crate::pixel::ChoixPixel::libelle), r.pixel.rang()),
    );

    l(Element::Section(CATEGORIES[1]), "", "", Valeur::Rien);
    for (i, d) in detectes.iter().enumerate() {
        l(Element::Detecte(i), &d.nom, &format!("Détecté · {}", d.detail), Valeur::Bouton("Ouvrir"));
    }
    for (i, a) in ajoutes.iter().enumerate() {
        let sorte = a.ajoute.and_then(|k| r.stockages.get(k)).map_or(String::new(), |s| casse_phrase(s.sorte.libelle()));
        l(Element::Ajoute(a.ajoute.unwrap_or(i)), &a.nom, &format!("{sorte} · {} · Suppr pour retirer", a.detail), Valeur::Bouton("Ouvrir"));
    }
    let genres = if r.experimental { "Serveur ZimaOS, partage Windows, WebDAV / Nextcloud, SFTP, dossier ou lecteur" } else { "Serveur ZimaOS, partage Windows, dossier ou lecteur" };
    l(Element::Ajouter, "Ajouter un stockage", genres, Valeur::Bouton("+ Ajouter"));

    l(Element::Section(CATEGORIES[2]), "", "", Valeur::Rien);
    l(Element::Depart, "Dossier d'ouverture", "Quand SpaceFiles s'ouvre sans dossier précis.", Valeur::Choix(r.depart.libelle().into()));
    l(Element::RestaurerOnglets, "Restaurer les onglets", "Rouvre les onglets de la dernière fois.", oui(r.restaurer_onglets));
    l(Element::ParDefaut, "Explorateur par défaut", "Les dossiers ouverts par Windows et les autres applications arrivent ici.", oui(par_defaut));
    l(Element::WinE, "Win+E ouvre SpaceFiles", "Petit résident lancé avec Windows qui intercepte Win+E, et garde l'ouverture instantanée.", oui(win_e));

    l(Element::Section(CATEGORIES[3]), "", "", Valeur::Rien);
    l(Element::VoletDense, "Densité du volet", "Lignes plus serrées dans le volet de gauche (favoris, arbre, disques).", oui(r.volet_dense));
    l(Element::Arbre, "Arborescence dans le volet", "Arbre de dossiers sous les disques. Ctrl+Maj+B : aller dans l'arbre, flèches et Entrée.", oui(r.arbre));
    for k in 1..crate::rendu::SECTIONS_VOLET.len() as u8 {
        let b = 1u32 << k;
        let etat = if r.volet_masque & b != 0 { 2 } else if r.volet_replie & b != 0 { 1 } else { 0 };
        l(
            Element::SectionVolet(k),
            &format!("Section {}", crate::rendu::SECTIONS_VOLET[k as usize].to_lowercase()),
            "Un clic sur son titre dans le volet la replie ou la déplie.",
            segments(&["Visible", "Repliée", "Masquée"], etat),
        );
    }
    for (i, c) in r.epingles.iter().enumerate() {
        l(Element::Epingle(i), &crate::dossier::nom_affiche(c), &format!("Épinglé · {c} · Suppr pour retirer · Maj+↑↓ pour déplacer"), Valeur::Bouton("Ouvrir"));
    }

    l(Element::Section(CATEGORIES[4]), "", "", Valeur::Rien);
    l(Element::Extensions, "Extensions des fichiers", "« photo.jpg » plutôt que « photo » (le type reste dans sa colonne).", oui(r.extensions));
    l(Element::VueDefaut, "Vue des nouveaux onglets", "Liste, grille de miniatures ou colonnes.", segments(&crate::reglages::VUES, (r.vue_defaut as usize).min(2)));
    l(Element::Caches, "Fichiers cachés", "Affichés dans les nouveaux onglets (Ctrl+H pour l'onglet ouvert).", oui(r.caches));
    l(Element::DatesRelatives, "Dates relatives", "« Il y a 5 min », « Hier 14:02 » au lieu de la date complète.", oui(r.dates_relatives));
    l(Element::EtatCloud, "État des fichiers cloud", "Point plein : sur ce PC · anneau : en ligne seulement · double : toujours gardé.", oui(r.etat_cloud));
    l(Element::Apercu, "Aperçu à l'ouverture", "Ouvert par défaut pour un nouveau profil (Alt+P à tout moment).", oui(r.apercu));

    l(Element::Section(CATEGORIES[5]), "", "", Valeur::Rien);
    l(Element::UnClic, "Ouvrir en un clic", "Un seul clic ouvre dossiers et fichiers.", oui(r.un_clic));
    let ressort = if r.ressort_ms == 0 { "Jamais".to_string() } else { format!("{},{} s", r.ressort_ms / 1000, r.ressort_ms % 1000 / 100) };
    l(Element::Ressort, "Dossiers à ressort", "Pendant un glisser, un dossier survolé s'ouvre après ce délai.", Valeur::Choix(ressort));
    l(
        Element::ConflitDefaut,
        "Nom déjà pris",
        "Quand une copie ou un déplacement rencontre un fichier de même nom : demander (carte) ou répondre d'avance.",
        Valeur::Choix(crate::reglages::CONFLITS_DEFAUT[(r.conflit_defaut as usize).min(4)].into()),
    );
    l(Element::CopieVerifiee, "Copie vérifiée", "Vers un NAS, un partage ou une clé : compare chaque fichier à l'original (SHA-256) après la copie.", oui(r.copie_verifiee));
    let limite = if r.limite_reseau_mo == 0 { "Sans limite".to_string() } else { format!("{} Mo/s", r.limite_reseau_mo) };
    l(Element::LimiteReseau, "Vitesse des copies réseau", "Copies vers ou depuis un NAS : laisse de la bande passante au reste.", Valeur::Choix(limite));
    l(Element::Experimental, "Fonctions expérimentales", "SFTP, WebDAV, sauvegarde NAS, MTP : peu testés.", oui(r.experimental));

    l(Element::Section(CATEGORIES[6]), "", "", Valeur::Rien);
    l(
        Element::TelechargementsNavigateur,
        "Suivre les téléchargements du navigateur",
        "Pastille pendant le téléchargement et marque Nouveau : seul le dossier Téléchargements est regardé, rien n'est demandé au navigateur.",
        oui(r.telechargements_navigateur),
    );
    let jours = if r.nettoyage_jours == 0 { "Jamais".to_string() } else { format!("{} jours", r.nettoyage_jours) };
    l(Element::NettoyageJours, "Nettoyage des anciens fichiers", "À l'ouverture du dossier, ce qui est arrivé il y a plus longtemps part à la corbeille.", Valeur::Choix(jours));
    l(Element::NettoyerCopies, "Retirer ce qui est copié ailleurs", "Un fichier copié ou déjà présent ailleurs sur le PC (badge Copie) part à la corbeille.", oui(r.nettoyer_copies));
    l(Element::NettoyerArchives, "Retirer les archives extraites", "ZIP, RAR, 7Z : après extraction, l'archive part à la corbeille.", oui(r.nettoyer_archives));
    l(Element::RangerAuto, "Ranger à l'arrivée", "Ce qui arrive ici va dans le dossier de sa règle (Ctrl+Z annule). Ajouter : palette › Règle de rangement.", oui(r.ranger_auto));
    for (i, x) in r.regles.iter().enumerate() {
        let Some(regle) = crate::rangement::Regle::analyser(x) else { continue };
        l(Element::Regle(i), &regle.libelle(), "Règle de rangement · Entrée ou Suppr pour la retirer", Valeur::Bouton("Retirer"));
    }

    l(Element::Section(CATEGORIES[7]), "", "", Valeur::Rien);
    for (i, d) in crate::index::disques().iter().enumerate() {
        let exclu = r.exclus.contains(&crate::reglages::forme_exclu(d));
        l(Element::DisqueIndexe(i), &format!("Disque {}", d.trim_end_matches('\\')), "Ses fichiers apparaissent dans la recherche (palette).", oui(!exclu));
    }
    for (i, x) in r.exclus.iter().enumerate().filter(|(_, x)| x.trim_end_matches('\\').contains('\\')) {
        l(Element::Exclu(i), x, "Dossier exclu de la recherche · Entrée ou Suppr pour le remettre", Valeur::Bouton("Remettre"));
    }
    l(Element::Reconstruire, "Reconstruire l'index", "Pour exclure un dossier : l'ouvrir, puis palette › Exclure.", Valeur::Bouton("Reconstruire"));

    l(Element::Section(CATEGORIES[8]), "", "", Valeur::Rien);
    l(Element::RetenirRecents, "Retenir les dossiers ouverts", "Proposés par la palette.", oui(r.retenir_recents));
    let n = r.recents.len();
    l(Element::EffacerRecents, "Oublier les dossiers récents", &format!("{n} dossier{} retenu{}", if n > 1 { "s" } else { "" }, if n > 1 { "s" } else { "" }), Valeur::Bouton("Oublier"));
    l(Element::EffacerIndex, "Supprimer l'index de recherche", "Noms des fichiers, gardés sur ce PC · refait à la recherche suivante.", Valeur::Bouton("Supprimer"));

    l(Element::Section(CATEGORIES[9]), "", "", Valeur::Rien);
    // Une commande dont la touche a été changée (réglage `raccourcis`) montre sa nouvelle touche
    let effectives: Vec<(String, &str)> = crate::palette::COMMANDES.iter().filter(|c| !c.2.is_empty() && !c.2.starts_with("CLIC")).map(|c| (crate::reglages::touche_de_commande(&format!("{:?}", c.0).to_uppercase()).unwrap_or_else(|| c.2.to_string()), c.1)).collect();
    let commandes = effectives.iter().map(|(t, q)| (t.as_str(), *q));
    let autres = [("WIN+E", "OUVRIR SPACEFILES"), ("CTRL+L", "SAISIR UN CHEMIN"), ("CTRL+K", "PALETTE DE COMMANDES"), ("CTRL+MAJ+P", "PALETTE DE COMMANDES"), ("TAB", "AUTRE PANNEAU (DOUBLE PANNEAU)")];
    // Une touche peut servir deux commandes selon le mode (F5 : actualiser en simple panneau, copier en double) : on déduplique la paire
    let mut vus = Vec::new();
    for (touche, quoi) in autres.into_iter().chain(commandes) {
        if !vus.contains(&(touche, quoi)) {
            vus.push((touche, quoi));
            l(Element::Info, &casse_phrase(quoi), "", Valeur::Texte(touche_lisible(touche)));
        }
    }

    l(Element::Section(CATEGORIES[10]), "", "", Valeur::Rien);
    l(Element::Animations, "Apparition en points", "Les lignes se dessinent en se dissolvant depuis des points.", oui(r.animations));
    l(Element::Sons, "Sons", "Fin de copie, corbeille, erreur (Ctrl+M).", oui(r.sons));
    l(Element::PoidsSonore, "Poids sonore", "Au survol d'un dossier, un tic : aigu s'il est léger, grave s'il est lourd.", oui(r.poids_sonore));

    l(Element::Section(CATEGORIES[11]), "", "", Valeur::Rien);
    l(Element::Fichier, "Fichier de réglages", "reglages.toml : lisible, à sauvegarder ou copier sur un autre PC.", Valeur::Bouton("Ouvrir"));
    l(Element::Exporter, "Exporter les réglages", "Copie reglages-spacefiles.toml dans un dossier à choisir (sans mots de passe : ils restent dans Windows).", Valeur::Bouton("Exporter"));
    l(Element::Importer, "Importer des réglages", "Remplace les réglages par ceux d'un fichier .toml ; les stockages importés seront à reconnecter.", Valeur::Bouton("Importer"));
    l(Element::Reinitialiser, "Réinitialiser", "Remet tous les réglages par défaut (les stockages restent).", Valeur::Bouton("Réinitialiser"));
    let donnees = crate::reglages::dossier_donnees(false).map(|p| p.display().to_string()).unwrap_or_default();
    let portable = crate::reglages::dossier_portable().is_some();
    l(Element::Info, "Données (réglages, index)", &donnees, Valeur::Texte(if portable { "Mode portable".into() } else { String::new() }));
    l(Element::Version, &format!("SpaceFiles {}", env!("CARGO_PKG_VERSION")), "Rust · Win32 · Direct2D", Valeur::Rien);
    v
}

// ── Géométrie de la page (DIP) ──

/// Titre de la catégorie (h2 de 22) avec la marge haute de 24 : première ligne de chaque page
pub const TITRE_H: f32 = REGLAGES_PAD_Y + 32.0;
/// Hauteur d'une ligne de carte : la carte et la marge de 10 qui la précède
pub const LIGNE_H: f32 = REGLAGE_CARTE_H + REGLAGE_CARTE_ECART;
/// Ligne d'information (raccourcis) : un seul texte, plus serrée
pub const INFO_H: f32 = 36.0;
/// Haut de la page (sous la barre de titre) : la marge haute de 24 fait partie du contenu qui défile
pub const PAGE_Y: f32 = HAUTEUR_TITRE + 1.0;

/// Bords gauche et droit des cartes (padding de 32)
pub fn gauche() -> f32 {
    REGLAGES_NAV + REGLAGES_PAD_X
}
pub fn droite(l: f32) -> f32 {
    l - REGLAGES_PAD_X
}

/// Rectangle de la carte d'une ligne dont le haut (avant défilement et PAGE_Y) est `y` : la marge de 10 est au-dessus
pub fn rect_carte(y: f32, l: f32) -> D2D_RECT_F {
    rect(gauche(), y + REGLAGE_CARTE_ECART, (droite(l) - gauche()).max(0.0), REGLAGE_CARTE_H)
}

/// Largeur d'un segment : celle de la plus longue option (estimée, la même pour dessiner et pour viser au clic)
pub fn largeur_segment(options: &[String]) -> f32 {
    options.iter().map(|o| o.chars().count() as f32 * 6.8 + 28.0).fold(64.0, f32::max)
}

/// Zones des segments, collés à droite de la carte (padding 16), de gauche à droite
pub fn zones_segments(carte: D2D_RECT_F, options: &[String]) -> Vec<D2D_RECT_F> {
    let w = largeur_segment(options);
    let x0 = carte.right - 16.0 - w * options.len() as f32;
    let y0 = (carte.top + carte.bottom - REGLAGE_SEG_H) / 2.0;
    (0..options.len()).map(|k| rect(x0 + k as f32 * w, y0, w, REGLAGE_SEG_H)).collect()
}

/// Segment visé par le point (x, y) de la carte, s'il y en a un
pub fn segment_sous(carte: D2D_RECT_F, options: &[String], x: f32, y: f32) -> Option<usize> {
    zones_segments(carte, options).iter().position(|z| crate::rendu::dedans(z, x, y))
}

/// Rail de navigation du panneau Réglages
pub const NAV_Y: f32 = HAUTEUR_TITRE + 56.0;
pub const NAV_H: f32 = 36.0;
pub const NAV_X: f32 = 10.0;
pub const NAV_W: f32 = REGLAGES_NAV - 2.0 * NAV_X;

/// Catégories du rail, dans l'ordre ; chacune est aussi le nom de sa `Element::Section` dans `lignes()`
pub const CATEGORIES: [&str; 12] = [
    "Apparence",
    "Stockages",
    "Démarrage",
    "Volet",
    "Affichage",
    "Comportement",
    "Téléchargements",
    "Recherche",
    "Confidentialité",
    "Raccourcis",
    "Animations et sons",
    "À propos",
];

/// Zones du rail des Réglages, dans l'ordre des catégories
pub fn zones_nav() -> Vec<(usize, D2D_RECT_F)> {
    (0..CATEGORIES.len()).map(|k| (k, rect(NAV_X, NAV_Y + k as f32 * NAV_H, NAV_W, NAV_H - 2.0))).collect()
}

/// Croix de fermeture du panneau Réglages, en haut à droite du contenu
pub fn zone_fermer(l: f32) -> D2D_RECT_F {
    rect(l - REGLAGES_MARGE - 30.0, HAUTEUR_TITRE + 14.0, 30.0, 30.0)
}

/// Indices des lignes de la catégorie `cat` : ceux qui suivent sa section, jusqu'à la section suivante
pub fn plage(lignes: &[Ligne], cat: usize) -> Range<usize> {
    let Some(nom) = CATEGORIES.get(cat) else { return 0..0 };
    let Some(debut) = lignes.iter().position(|l| matches!(l.element, Element::Section(t) if t == *nom)) else { return 0..0 };
    let fin = lignes[debut + 1..].iter().position(|l| matches!(l.element, Element::Section(_))).map_or(lignes.len(), |p| debut + 1 + p);
    debut + 1..fin
}

/// Haut et hauteur de chaque ligne de la liste complète, avant défilement. Seule la page `cat` occupe de la place : son titre
/// puis ses lignes ; toutes les autres valent (0, 0) et ne se dessinent ni ne se visent.
pub fn positions(lignes: &[Ligne], cat: usize) -> Vec<(f32, f32)> {
    let p = plage(lignes, cat);
    let mut v = vec![(0.0, 0.0); lignes.len()];
    if p.is_empty() {
        return v;
    }
    v[p.start - 1] = (0.0, TITRE_H);
    // Des lignes d'information sans carte (Raccourcis) : un peu d'air sous le titre, les cartes ont leur marge haute
    let mut y = TITRE_H + if lignes[p.start].element == Element::Info && lignes[p.start].detail.is_empty() { 8.0 } else { 0.0 };
    for k in p {
        let h = if lignes[k].element == Element::Info && lignes[k].detail.is_empty() { INFO_H } else { LIGNE_H };
        v[k] = (y, h);
        y += h;
    }
    v
}

/// Hauteur totale du contenu de la page `cat` (marge basse de 24 comprise)
pub fn hauteur_totale(lignes: &[Ligne], cat: usize) -> f32 {
    let p = plage(lignes, cat);
    p.last().map_or(TITRE_H, |k| {
        let (y, h) = positions(lignes, cat)[k];
        y + h
    }) + REGLAGES_PAD_Y
}

/// Nom lu par un lecteur d'écran pour chaque ligne de la page `cat` (« titre, valeur »), dans l'ordre affiché, et rang de
/// la ligne `focus` dans cette liste
pub fn noms_accessibles(lignes: &[Ligne], cat: usize, focus: usize) -> (Vec<String>, Option<usize>) {
    let p = plage(lignes, cat);
    let rang = p.contains(&focus).then(|| focus - p.start);
    let noms = p
        .map(|k| {
            let l = &lignes[k];
            let valeur = match &l.valeur {
                Valeur::Rien => String::new(),
                Valeur::Interrupteur(b) => (if *b { "activé" } else { "désactivé" }).to_string(),
                Valeur::Choix(t) | Valeur::Texte(t) => t.clone(),
                Valeur::Segments(o, a) => o.get(*a).cloned().unwrap_or_default(),
                Valeur::Bouton(t) => (*t).to_string(),
            };
            if valeur.is_empty() { l.libelle.clone() } else { format!("{}, {valeur}", l.libelle) }
        })
        .collect();
    (noms, rang)
}

pub struct Page {
    /// Ligne choisie (indice dans les lignes)
    pub focus: usize,
    pub defilement: f32,
    pub formulaire: Option<Formulaire>,
    /// Catégorie affichée (rang dans `CATEGORIES`)
    pub categorie: usize,
}

impl Page {
    /// Ouverture : la page Apparence, première carte choisie (la ligne 0 est la section, la 1 le thème)
    pub fn nouvelle() -> Self {
        Self { focus: 1, defilement: 0.0, formulaire: None, categorie: 0 }
    }

    /// Y a-t-il une carte à choisir dans la page affichée ? (Raccourcis : seulement des lignes d'information)
    pub fn a_des_choix(&self, lignes: &[Ligne]) -> bool {
        plage(lignes, self.categorie).any(|k| lignes[k].element.choisissable())
    }

    /// Carte choisissable suivante (sens +1) ou précédente (-1), sans sortir de la page affichée
    pub fn deplacer(&mut self, lignes: &[Ligne], sens: i32) {
        let p = plage(lignes, self.categorie);
        let mut i = self.focus as i32;
        loop {
            i += sens;
            if i < p.start as i32 || i >= p.end as i32 {
                return;
            }
            if lignes[i as usize].element.choisissable() {
                self.focus = i as usize;
                return;
            }
        }
    }

    /// Première (`debut`) ou dernière carte choisissable de la page affichée
    pub fn extreme(&mut self, lignes: &[Ligne], debut: bool) {
        let p = plage(lignes, self.categorie);
        let mut it = p.filter(|&k| lignes[k].element.choisissable());
        if let Some(k) = if debut { it.next() } else { it.last() } {
            self.focus = k;
        }
    }

    /// Ramène le focus dans la page affichée, sur une carte choisissable (après une suppression, un import)
    pub fn reajuster(&mut self, lignes: &[Ligne]) {
        let p = plage(lignes, self.categorie);
        if p.is_empty() {
            return;
        }
        self.focus = self.focus.clamp(p.start, p.end - 1);
        if !lignes[self.focus].element.choisissable() {
            let ancien = self.focus;
            self.deplacer(lignes, 1);
            if self.focus == ancien || !lignes[self.focus].element.choisissable() {
                self.focus = ancien;
                self.deplacer(lignes, -1);
            }
        }
    }

    /// Garde la carte choisie visible ; la première carte ramène le titre de la page
    pub fn montrer(&mut self, lignes: &[Ligne], hauteur: f32) {
        let pos = positions(lignes, self.categorie);
        let Some(&(y, h)) = pos.get(self.focus) else { return };
        if h == 0.0 {
            return;
        }
        let premiere = plage(lignes, self.categorie).find(|&k| lignes[k].element.choisissable()) == Some(self.focus);
        let haut = if premiere { 0.0 } else { y };
        if haut < self.defilement {
            self.defilement = haut;
        } else if y + h > self.defilement + hauteur {
            self.defilement = y + h - hauteur;
        }
        self.defiler(lignes, 0.0, hauteur);
    }

    pub fn defiler(&mut self, lignes: &[Ligne], delta: f32, hauteur: f32) {
        let total = hauteur_totale(lignes, self.categorie);
        self.defilement = (self.defilement + delta).clamp(0.0, (total - hauteur).max(0.0));
    }

    /// Affiche la catégorie `k` (bornée) : en haut de page, première carte choisie
    pub fn aller_categorie(&mut self, lignes: &[Ligne], k: usize) {
        self.categorie = k.min(CATEGORIES.len() - 1);
        self.defilement = 0.0;
        let p = plage(lignes, self.categorie);
        // Sans carte à choisir (Raccourcis), le focus reste sur la section : rien n'est surligné
        self.focus = p.clone().find(|&i| lignes[i].element.choisissable()).unwrap_or(p.start.saturating_sub(1));
    }

    /// Catégorie suivante (sens +1) ou précédente (-1) du rail, en boucle
    pub fn changer_categorie(&mut self, lignes: &[Ligne], sens: i32) {
        let n = CATEGORIES.len() as i32;
        self.aller_categorie(lignes, (self.categorie as i32 + sens).rem_euclid(n) as usize);
    }
}

// ── Formulaire « ajouter un stockage » ──

pub const CHAMPS: [&str; 4] = ["Nom", "Adresse", "Utilisateur", "Mot de passe"];
/// Éléments du formulaire : 0 genre, 1–4 champs, 5 ajouter, 6 annuler
pub const F_GENRE: usize = 0;
pub const F_AJOUTER: usize = 5;
pub const F_ANNULER: usize = 6;

pub struct Formulaire {
    pub sorte: Sorte,
    pub champs: [Edition; 4],
    pub focus: usize,
    pub erreur: Option<String>,
    /// Vérification réseau en cours
    pub en_cours: bool,
    /// Après un échec : un nouvel appui ajoute sans vérifier
    pub forcer: bool,
}

impl Formulaire {
    pub fn nouveau() -> Self {
        let vide = || Edition::nouveau(0, "", true);
        Self { sorte: Sorte::Zima, champs: [vide(), vide(), vide(), vide()], focus: 1, erreur: None, en_cours: false, forcer: false }
    }

    pub fn champ(&mut self) -> Option<&mut Edition> {
        match self.focus {
            1..=4 => Some(&mut self.champs[self.focus - 1]),
            _ => None,
        }
    }

    pub fn changer_sorte(&mut self, sens: i32) {
        let genres = Sorte::proposees(crate::reglages::experimental());
        let n = genres.len() as i32;
        let i = genres.iter().position(|&s| s == self.sorte).unwrap_or(0) as i32;
        self.sorte = genres[((i + sens).rem_euclid(n)) as usize];
        self.erreur = None;
        self.forcer = false;
    }

    pub fn deplacer(&mut self, sens: i32) {
        self.focus = (self.focus as i32 + sens).clamp(0, F_ANNULER as i32) as usize;
    }

    /// Les champs utilisateur / mot de passe ne servent pas pour un dossier
    pub fn champ_utile(&self, k: usize) -> bool {
        !(self.sorte == Sorte::Dossier && k >= 3) && !(self.sorte == Sorte::Sftp && k == 4)
    }

    /// Efface le mot de passe de la mémoire
    pub fn oublier_mot_de_passe(&mut self) {
        self.champs[3].texte.iter_mut().for_each(|c| *c = 0);
        self.champs[3] = Edition::nouveau(0, "", true);
    }
}

/// Carte du formulaire et zones cliquables, dans la zone de la page (x0, largeur l, hauteur h de la fenêtre)
pub fn carte_formulaire(l: f32, h: f32) -> D2D_RECT_F {
    let dispo = (l - REGLAGES_NAV - 2.0 * REGLAGES_MARGE).max(200.0);
    let w = dispo.min(620.0);
    let hauteur = 470.0f32.min(h - PAGE_Y - 20.0);
    rect(REGLAGES_NAV + (l - REGLAGES_NAV - w) / 2.0, PAGE_Y + 10.0, w, hauteur)
}

pub const FORM_LIGNE: f32 = 50.0;

pub fn zones_formulaire(carte: D2D_RECT_F) -> Vec<(usize, D2D_RECT_F)> {
    let (x, w) = (carte.left + 24.0, carte.right - carte.left - 48.0);
    let y0 = carte.top + 64.0;
    let mut v: Vec<(usize, D2D_RECT_F)> = (0..5).map(|k| (k, rect(x, y0 + k as f32 * FORM_LIGNE, w, FORM_LIGNE - 8.0))).collect();
    let yb = carte.bottom - 58.0;
    v.push((F_AJOUTER, rect(carte.right - 24.0 - 330.0, yb, 210.0, 38.0)));
    v.push((F_ANNULER, rect(carte.right - 24.0 - 110.0, yb, 110.0, 38.0)));
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Alt+Espace n'est plus une fonction de SpaceFiles : ni réglage, ni ligne d'aide dans la page
    #[test]
    fn page_sans_recherche_rapide() {
        for win_e in [false, true] {
            let v = lignes(&Reglages::default(), &[], &[], false, win_e);
            assert!(!v.is_empty());
            for l in &v {
                let tout = format!("{} {}", l.libelle, l.detail).to_uppercase();
                assert!(!tout.contains("RECHERCHE RAPIDE") && !tout.contains("ALT+ESPACE"), "ligne restante : {tout}");
                if let Valeur::Texte(t) = &l.valeur {
                    assert!(!t.to_uppercase().contains("ALT+ESPACE"), "touche restante : {t}");
                }
            }
            // La ligne Win+E, elle, reste
            assert!(v.iter().any(|l| matches!(l.element, Element::WinE)));
        }
    }

    // ── Catégories, pages et sélection (lot 10A) ──

    /// Réglages et lieux qui font apparaître toutes les variantes d'éléments (épingle, règle, exclu, stockage, détecté)
    fn lignes_completes() -> Vec<Ligne> {
        let mut r = Reglages::default();
        r.epingles = vec!["C:\\Dossier".into()];
        r.regles = vec!["pdf|D:\\Factures".into()];
        r.exclus = vec!["C:\\Perso\\Prive".into()];
        r.experimental = true;
        let lieu = |nom: &str, ajoute| Lieu { nom: nom.into(), emplacement: crate::dossier::Emplacement::CePC, icone: Icone::Nuage, ajoute, detail: "C:\\x".into() };
        let detectes = [lieu("OneDrive", None)];
        let ajoutes = [lieu("NAS", Some(0))];
        r.stockages = vec![crate::reglages::Stockage { nom: "NAS".into(), sorte: Sorte::Smb, adresse: "NAS".into(), utilisateur: String::new() }];
        lignes(&r, &detectes, &ajoutes, false, false)
    }

    /// Catégorie où le brief range chaque élément (répartition du lot 10A)
    fn categorie_attendue(e: Element) -> usize {
        use Element::*;
        match e {
            Theme | TailleLignes | Contraste | Mouvement | Trame | RefletSurvol | YeuxPixel | PixelHeure => 0,
            Detecte(_) | Ajoute(_) | Ajouter => 1,
            Depart | RestaurerOnglets | ParDefaut | WinE => 2,
            VoletDense | Arbre | SectionVolet(_) | Epingle(_) => 3,
            Extensions | VueDefaut | Caches | DatesRelatives | EtatCloud | Apercu => 4,
            UnClic | Ressort | ConflitDefaut | CopieVerifiee | LimiteReseau | Experimental => 5,
            TelechargementsNavigateur | NettoyageJours | NettoyerCopies | NettoyerArchives | RangerAuto | Regle(_) => 6,
            DisqueIndexe(_) | Exclu(_) | Reconstruire => 7,
            RetenirRecents | EffacerRecents | EffacerIndex => 8,
            Animations | Sons | PoidsSonore => 10,
            Fichier | Exporter | Importer | Reinitialiser | Version => 11,
            Info => usize::MAX,
            Section(_) => unreachable!(),
        }
    }

    /// Chaque réglage est dans une seule page, la bonne, et les 12 pages existent
    #[test]
    fn chaque_element_est_dans_une_seule_page() {
        let v = lignes_completes();
        assert_eq!(CATEGORIES.len(), 12);
        for cat in 0..CATEGORIES.len() {
            assert!(!plage(&v, cat).is_empty(), "la page {} existe et n'est pas vide", CATEGORIES[cat]);
        }
        for (k, l) in v.iter().enumerate() {
            if matches!(l.element, Element::Section(_)) {
                continue;
            }
            let pages: Vec<usize> = (0..CATEGORIES.len()).filter(|&c| plage(&v, c).contains(&k)).collect();
            assert_eq!(pages.len(), 1, "{:?} doit être dans exactement une page", l.element);
            if l.element == Element::Info {
                // Raccourcis (touches) ou données (À propos)
                assert!(pages[0] == 9 || pages[0] == 11, "ligne d'information dans la page {}", pages[0]);
            } else {
                assert_eq!(pages[0], categorie_attendue(l.element), "{:?}", l.element);
            }
        }
        // Les éléments variables du jeu d'essai sont bien présents
        for e in [Element::Epingle(0), Element::Regle(0), Element::Exclu(0), Element::Ajoute(0), Element::Detecte(0), Element::SectionVolet(1)] {
            assert!(v.iter().any(|l| l.element == e), "{e:?} absent du jeu d'essai");
        }
    }

    /// La table des catégories : chaque entrée a sa section du même nom, dans l'ordre ; aucune section n'est orpheline
    #[test]
    fn categories_sans_renvoi_vers_une_autre_section() {
        let v = lignes_completes();
        let sections: Vec<&str> = v.iter().filter_map(|l| if let Element::Section(t) = l.element { Some(t) } else { None }).collect();
        assert_eq!(sections, CATEGORIES, "une section par catégorie, du même nom et dans le même ordre");
        for (i, a) in CATEGORIES.iter().enumerate() {
            assert!(!CATEGORIES[i + 1..].contains(a), "{a} en double");
        }
        // Un libellé de rail est en casse normale : majuscule initiale, jamais tout en capitales
        for c in CATEGORIES {
            assert!(c.chars().next().is_some_and(char::is_uppercase) && c.chars().any(char::is_lowercase), "{c}");
        }
    }

    /// Les cartes sont en casse normale : aucun libellé ni aucune description en capitales
    #[test]
    fn textes_en_casse_normale() {
        for l in lignes_completes().iter().filter(|l| !matches!(l.element, Element::Section(_) | Element::Regle(_))) {
            for t in [&l.libelle, &l.detail] {
                let lettres = t.chars().filter(|c| c.is_alphabetic()).count();
                assert!(lettres < 4 || t.chars().any(char::is_lowercase), "texte en capitales : {t:?} ({:?})", l.element);
            }
        }
    }

    /// Positions : seule la page affichée occupe de la place, titre d'abord, cartes ensuite
    #[test]
    fn positions_de_la_page_affichee() {
        let v = lignes_completes();
        for cat in 0..CATEGORIES.len() {
            let pos = positions(&v, cat);
            let p = plage(&v, cat);
            assert_eq!(pos[p.start - 1], (0.0, TITRE_H));
            for (k, &(y, h)) in pos.iter().enumerate() {
                if p.contains(&k) {
                    assert!(h > 0.0 && y >= TITRE_H);
                } else if k != p.start - 1 {
                    assert_eq!(h, 0.0, "la ligne {k} n'est pas dans la page {cat}");
                }
            }
            assert!(hauteur_totale(&v, cat) > TITRE_H);
        }
    }

    /// Clavier sans interface : ↑↓ restent dans la page, Début / Fin, Tab et Ctrl+PgDn changent de catégorie en boucle
    #[test]
    fn selection_et_changement_de_page() {
        let v = lignes_completes();
        let mut p = Page::nouvelle();
        assert_eq!((p.categorie, v[p.focus].element), (0, Element::Theme));
        // Borne haute : ↑ sur la première carte ne bouge pas, et ne passe pas à la page précédente
        p.deplacer(&v, -1);
        assert_eq!(v[p.focus].element, Element::Theme);
        p.deplacer(&v, 1);
        p.deplacer(&v, 1);
        assert_eq!(v[p.focus].element, Element::Contraste);
        // Les cinq cartes du lot 10B suivent Contraste, dans l'ordre
        for e in [Element::Mouvement, Element::Trame, Element::RefletSurvol, Element::YeuxPixel, Element::PixelHeure] {
            p.deplacer(&v, 1);
            assert_eq!(v[p.focus].element, e);
        }
        // Borne basse : la dernière carte d'Apparence, ↓ ne sort pas vers Stockages
        p.deplacer(&v, 1);
        assert_eq!(v[p.focus].element, Element::PixelHeure);
        p.extreme(&v, true);
        assert_eq!(v[p.focus].element, Element::Theme);
        p.extreme(&v, false);
        assert_eq!(v[p.focus].element, Element::PixelHeure);
        // Changement de page : la première carte choisissable, en haut
        p.defilement = 120.0;
        p.changer_categorie(&v, 1);
        assert_eq!((p.categorie, p.defilement), (1, 0.0));
        assert_eq!(v[p.focus].element, Element::Detecte(0));
        // En boucle, dans les deux sens
        p.aller_categorie(&v, 11);
        p.changer_categorie(&v, 1);
        assert_eq!(p.categorie, 0);
        p.changer_categorie(&v, -1);
        assert_eq!(p.categorie, 11);
        // Une catégorie hors table est ramenée à la dernière
        p.aller_categorie(&v, 99);
        assert_eq!(p.categorie, 11);
        // Raccourcis : que des lignes d'information, rien à choisir, mais la page existe et défile
        p.aller_categorie(&v, 9);
        assert!(!p.a_des_choix(&v));
        assert!(matches!(v[p.focus].element, Element::Section(_)));
        p.deplacer(&v, 1);
        assert!(matches!(v[p.focus].element, Element::Section(_)));
        p.defiler(&v, 5000.0, 300.0);
        assert!(p.defilement > 0.0 && p.defilement <= hauteur_totale(&v, 9) - 300.0 + 0.01);
        // Après une suppression le focus revient sur une carte de la page
        p.aller_categorie(&v, 3);
        p.focus = plage(&v, 3).end + 5;
        p.reajuster(&v);
        assert!(plage(&v, 3).contains(&p.focus) && v[p.focus].element.choisissable());
    }

    /// Lot 10B : valeurs, textes de la maquette et lecture d'écran des cinq nouvelles cartes
    #[test]
    fn cartes_de_la_maquette() {
        let mut r = Reglages::default();
        r.mouvement_reduit = true;
        r.trame = false;
        r.pixel = crate::pixel::ChoixPixel::Soir;
        let v = lignes(&r, &[], &[], false, false);
        let ligne = |e: Element| v.iter().find(|l| l.element == e).unwrap_or_else(|| panic!("{e:?} absent"));
        assert!(matches!(&ligne(Element::Mouvement).valeur, Valeur::Segments(o, 1) if o == &["Complet", "Réduit"]));
        assert_eq!(ligne(Element::Mouvement).detail, "Réduit coupe les ressorts, le reflet et les dissolutions.");
        assert!(matches!(ligne(Element::Trame).valeur, Valeur::Interrupteur(false)));
        assert!(matches!(ligne(Element::RefletSurvol).valeur, Valeur::Interrupteur(true)));
        assert!(matches!(ligne(Element::YeuxPixel).valeur, Valeur::Interrupteur(true)));
        assert!(matches!(&ligne(Element::PixelHeure).valeur, Valeur::Segments(o, 2) if o == &["Auto", "Jour", "Soir", "Nuit"]));
        let (noms, _) = noms_accessibles(&v, 0, 1);
        assert!(noms.contains(&"Mouvement, Réduit".to_string()) && noms.contains(&"Pixel selon l'heure, Soir".to_string()), "{noms:?}");
        assert!(noms.contains(&"Trame de points, désactivé".to_string()));
    }

    /// La carte choisie reste visible, la première ramène le titre de la page
    #[test]
    fn carte_choisie_visible() {
        let v = lignes_completes();
        let mut p = Page::nouvelle();
        p.aller_categorie(&v, 5);
        p.extreme(&v, false);
        p.montrer(&v, 200.0);
        let (y, h) = positions(&v, 5)[p.focus];
        assert!(p.defilement + 200.0 >= y + h && p.defilement <= y);
        p.extreme(&v, true);
        p.montrer(&v, 200.0);
        assert_eq!(p.defilement, 0.0, "la première carte montre le titre");
    }

    /// Clic sur un segment : les zones sont dans la carte, côte à côte, et le point visé donne le bon rang
    #[test]
    fn segments_vises_au_clic() {
        let options: Vec<String> = ["Auto", "Système", "Sombre", "Clair"].iter().map(|s| s.to_string()).collect();
        let carte = rect_carte(100.0, 1440.0);
        let zones = zones_segments(carte, &options);
        assert_eq!(zones.len(), 4);
        assert!((zones[3].right - (carte.right - 16.0)).abs() < 0.01);
        for (k, z) in zones.iter().enumerate() {
            assert!(z.left >= carte.left && z.bottom <= carte.bottom && z.top >= carte.top);
            let (cx, cy) = ((z.left + z.right) / 2.0, (z.top + z.bottom) / 2.0);
            assert_eq!(segment_sous(carte, &options, cx, cy), Some(k));
        }
        assert_eq!(segment_sous(carte, &options, carte.left + 4.0, (carte.top + carte.bottom) / 2.0), None);
    }

    #[test]
    fn casses_et_touches() {
        assert_eq!(casse_phrase("DOSSIER OU LECTEUR"), "Dossier ou lecteur");
        assert_eq!(casse_phrase("PARTAGE WINDOWS (SMB)"), "Partage Windows (SMB)");
        assert_eq!(casse_phrase("OUVRIR SPACEFILES"), "Ouvrir SpaceFiles");
        assert_eq!(casse_phrase("COPIER VERS LE NAS"), "Copier vers le NAS");
        assert_eq!(touche_lisible("CTRL+MAJ+T"), "Ctrl+Maj+T");
        assert_eq!(touche_lisible("F6"), "F6");
        assert_eq!(touche_lisible("CTRL+,"), "Ctrl+,");
        assert_eq!(touche_lisible("ESPACE"), "Espace");
        assert_eq!(touche_lisible("ALT+ENTREE"), "Alt+Entrée");
        assert_eq!(casse_phrase("AJOUTER A UN GROUPE…"), "Ajouter à un groupe…");
        assert_eq!(casse_phrase("TOUT SELECTIONNER"), "Tout sélectionner");
        assert_eq!(casse_phrase("REFAIRE CE QUI VIENT D'ETRE ANNULE"), "Refaire ce qui vient d'être annulé");
        assert_eq!(casse_phrase("VOLET D'APERCU"), "Volet d'aperçu");
    }

    /// Lecteur d'écran : « titre, valeur » par carte, et le rang de la carte choisie dans la page affichée
    #[test]
    fn noms_pour_le_lecteur_d_ecran() {
        let v = lignes_completes();
        let k = v.iter().position(|l| l.element == Element::Theme).unwrap();
        let (noms, rang) = noms_accessibles(&v, 0, k);
        assert_eq!(rang, Some(0));
        assert_eq!(noms[0], "Thème, Auto");
        assert_eq!(noms.len(), plage(&v, 0).len());
        let k = v.iter().position(|l| l.element == Element::Contraste).unwrap();
        assert_eq!(noms_accessibles(&v, 0, k).0[2], "Contraste renforcé, désactivé");
        // Un focus d'une autre page ne désigne aucune carte de celle-ci
        assert_eq!(noms_accessibles(&v, 1, k).1, None);
    }
}
