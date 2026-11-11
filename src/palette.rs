// Palette de commandes (Ctrl+K ou Ctrl+Maj+P), à étapes depuis le lot 15C : élément › commande › destination (ou nom, ou confirmation),
// puis la progression et le résultat, tout dans la palette. La logique pure des étapes est dans `assistant.rs` ; ce fichier tient la
// table des commandes générales (« Partout »), la machine d'étapes (`Palette::entrer`, `reculer`) et ce que chaque étape propose et montre.

use crate::dossier::Emplacement;
use crate::edition::Edition;
use crate::icones::Icone;
use crate::assistant::{self, CommandeEl, Etape, Execution, GenreSujet, Lecture, Param, Resultat, Sujet};
use std::collections::HashMap;
use std::hash::{Hash, Hasher};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Commande {
    NouvelOnglet,
    FermerOnglet,
    RouvrirOnglet,
    Reglages,
    Terminal,
    VsCode,
    CheminWsl,
    CheminUnix,
    CopierChemin,
    Sha256,
    Debloquer,
    ImageJpg,
    ImagePng,
    ImageReduire,
    ImageSansMeta,
    Zip,
    SeptZip,
    Extraire,
    RenommerLot,
    Epingler,
    BaliseRouge,
    BaliseBlanche,
    BaliseGrise,
    SansBalise,
    Etiquette1,
    Etiquette2,
    Etiquette3,
    VueListe,
    VueGrille,
    VueColonnes,
    VueTailles,
    DoublePanneau,
    /// F5 en double panneau (comme Maj+F5) : copie la sélection dans le dossier de l'autre panneau
    CopierVersAutre,
    Galerie,
    RechercheGlobale,
    ReconstruireIndex,
    ExclureRecherche,
    Caches,
    Apercu,
    NouveauDossier,
    Actualiser,
    Annuler,
    Refaire,
    Sons,
    Couper,
    Copier,
    Coller,
    Renommer,
    Supprimer,
    Partager,
    Proprietes,
    NouveauFichier,
    TrierNom,
    TrierDate,
    TrierType,
    TrierTaille,
    TrierCroissant,
    TrierDecroissant,
    ToutChoisir,
    InverserSelection,
    Palette,
    Ouvrir,
    OuvrirOnglet,
    OuvrirAvec,
    Melanger,
    NouveauGroupe,
    TaillesDossiers,
    Administrateur,
    /// Regrouper l'affichage : rang dans Regroupement::TOUS
    Regrouper(u8),
    Historique,
    EnregistrerSession,
    Comparer,
    Synchroniser,
    Sessions,
    Doublons,
    Chronologie,
    Note,
    RegleRangement,
    NonFiable,
    Ranger,
    Ejecter,
    VoirCorbeille,
    VoirPanneau,
    ArracherOnglet,
    RattacherFenetre,
    ComparerTextes,
    AjouterPlateau,
    VoirPlateau,
    ModeCapture,
    SauvegardeDestination,
    SauvegardeAjouter,
    SauvegardeVider,
    SauvegardeLancer,
    LireTexteImage,
    ListerTelephones,
    OuvrirJournalOperations,
    EpinglerAccesRapide,
    DeposerPlateau,
    DeplacerPlateau,
    ViderPlateau,
    LignesDossier,
    DossierParDefaut,
    Restaurer,
    Essentiel,
    Arbre,
    GlyphDossier,
    ViderCorbeille,
    PoidsSonore,
    PartageProche,
}

/// Fonction expérimentale (gelée par le réglage `experimental`) qu'une commande utilise : la sauvegarde vers un NAS
/// (ses quatre commandes) et la liste des appareils MTP. `Synchroniser` (comparer deux panneaux) n'en fait pas partie.
pub fn commande_experimentale(c: Commande) -> Option<crate::reglages::Experimentale> {
    use crate::reglages::Experimentale;
    match c {
        Commande::SauvegardeDestination | Commande::SauvegardeAjouter | Commande::SauvegardeVider | Commande::SauvegardeLancer => Some(Experimentale::Sauvegarde),
        Commande::ListerTelephones => Some(Experimentale::Mtp),
        _ => None,
    }
}

/// Une commande est-elle atteignable (palette, menus, raccourcis) avec ce réglage ?
pub fn commande_permise(experimental: bool, c: Commande) -> bool {
    commande_experimentale(c).is_none_or(|g| crate::reglages::fonction_experimentale_permise(experimental, g))
}

/// Commande, libellé, raccourci, mots qui la retrouvent aussi
/// Commande portant ce nom de variante (sans tenir compte de la casse), pour la table de raccourcis des réglages
pub fn commande_nommee(nom: &str) -> Option<Commande> {
    COMMANDES.iter().map(|c| c.0).find(|c| format!("{c:?}").eq_ignore_ascii_case(nom))
}

/// Touches qui ouvrent et ferment la palette (écrites comme `fenetre::combinaison`). Alt+Espace n'en fait pas partie : il appartient
/// au lanceur de SpaceNotch (décision D-01 du 01/11) et le retirer d'ici était voulu.
pub const TOUCHES_PALETTE: [&str; 2] = ["CTRL+K", "CTRL+MAJ+P"];

pub const COMMANDES: &[(Commande, &str, &str, &str)] = &[
    (Commande::Ouvrir, "OUVRIR", "ENTREE", "open lancer"),
    (Commande::OuvrirOnglet, "OUVRIR DANS UN NOUVEL ONGLET", "CLIC MILIEU", "tab"),
    (Commande::OuvrirAvec, "OUVRIR AVEC…", "", "programme application choisir"),
    (Commande::Couper, "COUPER", "CTRL+X", "cut deplacer"),
    (Commande::Copier, "COPIER", "CTRL+C", "copy dupliquer"),
    (Commande::Coller, "COLLER", "CTRL+V", "paste"),
    (Commande::Renommer, "RENOMMER", "F2", "rename nom"),
    (Commande::Supprimer, "SUPPRIMER (CORBEILLE)", "SUPPR", "effacer delete corbeille"),
    (Commande::Partager, "PARTAGER", "", "share envoyer"),
    (Commande::Proprietes, "PROPRIETES", "ALT+ENTREE", "infos details properties"),
    (Commande::NouveauFichier, "NOUVEAU FICHIER TEXTE", "", "creer txt document"),
    (Commande::TrierNom, "TRIER PAR NOM", "", "ordre tri alphabetique"),
    (Commande::TrierDate, "TRIER PAR DATE", "", "ordre tri recent modification"),
    (Commande::TrierType, "TRIER PAR TYPE", "", "ordre tri extension"),
    (Commande::TrierTaille, "TRIER PAR TAILLE", "", "ordre tri poids"),
    (Commande::TrierCroissant, "ORDRE CROISSANT", "", "tri ascendant"),
    (Commande::TrierDecroissant, "ORDRE DECROISSANT", "", "tri descendant inverse"),
    (Commande::TaillesDossiers, "CALCULER LA TAILLE DES DOSSIERS", "ESPACE · DOSSIER", "poids taille dossier calcul"),
    (Commande::Administrateur, "ROUVRIR CE DOSSIER EN ADMINISTRATEUR", "CTRL+MAJ+A", "admin droits eleve acces refuse"),
    (Commande::NouveauGroupe, "AJOUTER A UN GROUPE…", "CTRL+G", "groupe tag etiquette creer collection"),
    (Commande::Melanger, "FICHIERS ET DOSSIERS MELANGES", "", "tri dossiers d'abord melanger"),
    (Commande::Comparer, "COMPARER LES DEUX PANNEAUX", "CTRL+MAJ+F6", "differences double panneau comparer dossiers"),
    (Commande::Synchroniser, "SYNCHRONISER VERS L'AUTRE PANNEAU", "", "copier manquants plus recents miroir sauvegarde"),
    (Commande::EnregistrerSession, "ENREGISTRER LES ONGLETS EN SESSION…", "", "session projet onglets sauver nommer"),
    (Commande::Sessions, "SESSIONS D'ONGLETS", "", "session projet onglets ouvrir"),
    (Commande::Doublons, "CHERCHER LES DOUBLONS ICI", "", "doublons identiques copies place gagner nettoyer"),
    (Commande::Chronologie, "CHRONOLOGIE EN POINTS", "", "jours dates calendrier frise timeline"),
    (Commande::Note, "NOTE DU DOSSIER…", "", "note memo post-it commentaire epingler rappel"),
    (Commande::RegleRangement, "REGLE DE RANGEMENT… (PDF > DOSSIER)", "", "ranger rangement regle trier telechargements automatique hazel"),
    (Commande::LignesDossier, "DENSITE DES LIGNES DE CE DOSSIER : COMPACT / CONFORT", "", "compacte confort hauteur lignes dossier"),
    (Commande::DossierParDefaut, "CE DOSSIER : COLONNES ET LIGNES PAR DEFAUT", "", "oublier reinitialiser colonnes lignes dossier"),
    (Commande::AjouterPlateau, "AJOUTER LA SELECTION AU PLATEAU", "", "plateau tampon temporaire depot panier etagere"),
    (Commande::SauvegardeDestination, "SAUVEGARDE : CHOISIR LE DOSSIER DE DESTINATION (NAS)", "", "sauvegarde backup nas zimaos destination choisir"),
    (Commande::SauvegardeAjouter, "SAUVEGARDE : AJOUTER LE DOSSIER OUVERT A LA LISTE", "", "sauvegarde backup nas ajouter dossier liste"),
    (Commande::SauvegardeVider, "SAUVEGARDE : VIDER LA LISTE DES DOSSIERS", "", "sauvegarde backup vider liste retirer"),
    (Commande::SauvegardeLancer, "SAUVEGARDE : LANCER LA COPIE VERS LE NAS", "", "sauvegarde backup nas lancer copier un clic"),
    (Commande::LireTexteImage, "LIRE LE TEXTE DE L'IMAGE OU DU PDF CHOISI (OCR)", "", "ocr texte image scan pdf reconnaissance lire"),
    (Commande::ListerTelephones, "TELEPHONES ET APPAREILS MTP BRANCHES", "", "telephone mtp android appareil portable branche usb"),
    (Commande::ModeCapture, "MODE CAPTURE : MASQUER OU MONTRER LES NOMS CHOISIS", "", "capture partage ecran flou masquer noms confidentiel"),
    (Commande::EpinglerAccesRapide, "EPINGLER MES DOSSIERS DANS L'ACCES RAPIDE DE WINDOWS", "", "acces rapide epingler dossiers boites ouvrir enregistrer dialogues"),
    (Commande::OuvrirJournalOperations, "OUVRIR LE JOURNAL DES OPERATIONS (CSV)", "", "journal csv operations historique copies suppressions traces"),
    (Commande::VoirPlateau, "VOIR LE PLATEAU", "", "plateau tampon temporaire depot panier etagere"),
    (Commande::DeposerPlateau, "COPIER LE PLATEAU ICI", "", "plateau deposer copier tampon panier"),
    (Commande::DeplacerPlateau, "DEPLACER LE PLATEAU ICI", "", "plateau deposer deplacer tampon panier"),
    (Commande::ViderPlateau, "VIDER LE PLATEAU", "", "plateau effacer retirer tampon panier"),
    (Commande::ComparerTextes, "COMPARER LES DEUX FICHIERS TEXTE CHOISIS", "", "comparer diff differences texte fichiers lignes"),
    (Commande::ArracherOnglet, "ARRACHER L'ONGLET DANS UNE NOUVELLE FENETRE", "", "onglet detacher fenetre separer arracher"),
    (Commande::RattacherFenetre, "RATTACHER CETTE FENETRE A UNE AUTRE", "", "onglets fusionner rattacher fenetre regrouper"),
    (Commande::VoirPanneau, "VOIR LE PANNEAU DE CONFIGURATION ICI", "", "panneau configuration controlpanel parametres reglages windows"),
    (Commande::VoirCorbeille, "VOIR LA CORBEILLE ICI", "", "corbeille supprimes restaurer poubelle shell recyclebin"),
    (Commande::Restaurer, "RESTAURER LA SELECTION (CORBEILLE)", "", "corbeille remettre annuler suppression"),
    (Commande::Ejecter, "EJECTER LE DISQUE AMOVIBLE", "", "usb cle disque externe retirer demonter eject"),
    (Commande::NonFiable,"DOSSIER NON FIABLE : MARQUER / DEMARQUER", "", "securite confiance aperçu apercu vignettes git telecharge suspect"),
    (Commande::Ranger, "RANGER CE DOSSIER SELON LES REGLES", "", "ranger rangement regles telechargements classer"),
    (Commande::Essentiel, "MODE ESSENTIEL", "F9", "essentiel recents epingles focus calme minimal"),
    (Commande::Arbre, "ARBORESCENCE : ALLER DANS L'ARBRE DU VOLET", "CTRL+MAJ+B", "arbre dossiers volet navigation"),
    (Commande::GlyphDossier, "GLYPH DU DOSSIER…", "", "glyph motif points icone dossier personnaliser"),
    (Commande::ViderCorbeille, "VIDER LA CORBEILLE", "", "corbeille vider poubelle nettoyer"),
    (Commande::PoidsSonore, "POIDS SONORE", "", "son tic survol taille dossier poids"),
    (Commande::PartageProche, "PARTAGE A PROXIMITE", "", "telephone android nothing phone link partager envoyer proche"),
    (Commande::Historique, "HISTORIQUE DES OPERATIONS", "CTRL+MAJ+H", "annuler defaire refaire journal copies suppressions"),
    (Commande::Regrouper(1), "REGROUPER PAR DATE", "", "sections aujourd'hui hier semaine mois grouper"),
    (Commande::Regrouper(2), "REGROUPER PAR TYPE", "", "sections extension grouper"),
    (Commande::Regrouper(3), "REGROUPER PAR TAILLE", "", "sections poids grouper"),
    (Commande::Regrouper(4), "REGROUPER PAR NOM", "", "sections lettre alphabet grouper"),
    (Commande::Regrouper(5), "REGROUPER PAR GROUPE", "", "sections tags balises grouper"),
    (Commande::Regrouper(6), "REGROUPER PAR ORIGINE (INTERNET, NON SIGNE)", "", "quarantaine securite internet telecharge signature verifier virus"),
    (Commande::Regrouper(0), "NE PLUS REGROUPER", "", "sections aucun regroupement grouper"),
    (Commande::ToutChoisir, "TOUT SELECTIONNER", "CTRL+A", "selection tout"),
    (Commande::InverserSelection, "INVERSER LA SELECTION", "", "selection inverse"),
    (Commande::RechercheGlobale, "CHERCHER DANS TOUT LE PC", "CTRL+MAJ+F", "index recherche globale everything"),
    (Commande::Terminal, "TERMINAL · JOURNAL ET OUVERTURE EXTERNE", "CTRL+ALT+T", "powershell console cmd wt panneau journal"),
    (Commande::VsCode, "OUVRIR DANS VS CODE", "CTRL+ALT+C", "code editeur visual studio"),
    (Commande::CheminWsl, "COPIER LE CHEMIN WSL (/MNT/C/…)", "CTRL+ALT+L", "linux bash path"),
    (Commande::CheminUnix, "COPIER LE CHEMIN UNIX (C:/…)", "CTRL+ALT+U", "slash path"),
    (Commande::CopierChemin, "COPIER LE CHEMIN", "CTRL+MAJ+C", "path"),
    (Commande::ImageJpg, "CONVERTIR EN JPG", "", "image photo heic webp png convertir jpeg"),
    (Commande::ImagePng, "CONVERTIR EN PNG", "", "image photo convertir"),
    (Commande::ImageReduire, "REDUIRE A 1920 PX", "", "image photo redimensionner taille plus petite compresser"),
    (Commande::ImageSansMeta, "RETIRER LES METADONNEES", "", "image photo exif gps lieu vie privee nettoyer"),
    (Commande::Sha256, "EMPREINTE SHA-256 (COPIEE)", "CTRL+ALT+H", "hash somme controle checksum"),
    (Commande::Debloquer, "DEBLOQUER (RETIRER L'ORIGINE INTERNET)", "", "debloquer unblock zone identifier internet telecharge securite"),
    (Commande::Zip, "COMPRESSER EN ZIP", "CTRL+ALT+Z", "archive zipper"),
    (Commande::SeptZip, "COMPRESSER EN 7Z", "", "archive 7zip"),
    (Commande::Extraire, "EXTRAIRE ICI", "CTRL+ALT+X", "decompresser dezipper archive unzip"),
    (Commande::RenommerLot, "RENOMMER PAR LOT", "F2 · PLUSIEURS", "renommage compteur regex exif"),
    (Commande::Epingler, "EPINGLER / DESEPINGLER CE DOSSIER OU CETTE RECHERCHE", "CTRL+D", "favori pin recherche enregistree sauvegarder"),
    (Commande::BaliseRouge, "BALISE ROUGE", "CTRL+1", "tag couleur etiquette"),
    (Commande::BaliseBlanche, "BALISE BLANCHE", "CTRL+2", "tag couleur etiquette"),
    (Commande::BaliseGrise, "BALISE GRISE", "CTRL+3", "tag couleur etiquette"),
    (Commande::SansBalise, "RETIRER LA BALISE", "CTRL+0", "tag couleur etiquette"),
    (Commande::Etiquette1, "ÉTIQUETTE 1 (UN CARRÉ)", "ALT+1", "etiquette balise carre marque pose retire"),
    (Commande::Etiquette2, "ÉTIQUETTE 2 (DEUX CARRÉS)", "ALT+2", "etiquette balise carres marque pose retire"),
    (Commande::Etiquette3, "ÉTIQUETTE 3 (TROIS CARRÉS)", "ALT+3", "etiquette balise carres marque pose retire"),
    (Commande::VueListe, "VUE LISTE", "CTRL+MAJ+1", "affichage details"),
    (Commande::VueGrille, "VUE GRILLE (MINIATURES)", "CTRL+MAJ+2", "affichage icones vignettes"),
    (Commande::VueColonnes, "VUE COLONNES (MILLER)", "CTRL+MAJ+3", "affichage finder"),
    (Commande::VueTailles, "TAILLES DES DOSSIERS", "CTRL+MAJ+4", "espace disque poids"),
    (Commande::DoublePanneau, "DOUBLE PANNEAU", "F6", "deux volets commander"),
    (Commande::CopierVersAutre, "COPIER VERS L'AUTRE PANNEAU (DOUBLE PANNEAU)", "F5", "face copier deux volets commander"),
    (Commande::Galerie, "COUP D'OEIL · GALERIE PHOTO", "ESPACE", "apercu photo diaporama"),
    (Commande::ReconstruireIndex, "RECONSTRUIRE L'INDEX DE RECHERCHE", "", "index"),
    (Commande::ExclureRecherche, "EXCLURE CE DOSSIER DE LA RECHERCHE", "", "index ignorer exclure cacher"),
    (Commande::NouvelOnglet, "NOUVEL ONGLET", "CTRL+T", "tab"),
    (Commande::FermerOnglet, "FERMER L'ONGLET", "CTRL+W", "tab"),
    (Commande::RouvrirOnglet, "ROUVRIR L'ONGLET FERME", "CTRL+MAJ+T", "tab annuler restaurer"),
    (Commande::NouveauDossier, "NOUVEAU DOSSIER", "CTRL+MAJ+N", "creer"),
    (Commande::Caches, "AFFICHER / MASQUER LES FICHIERS CACHES", "CTRL+H", "caches systeme"),
    (Commande::Apercu, "VOLET D'APERCU", "ALT+P", "preview"),
    (Commande::Actualiser, "ACTUALISER", "F5", "recharger"),
    (Commande::Annuler, "ANNULER LA DERNIERE ACTION", "CTRL+Z", "defaire undo"),
    (Commande::Refaire, "REFAIRE CE QUI VIENT D'ETRE ANNULE", "CTRL+Y", "retablir redo"),
    (Commande::Sons, "ACTIVER / COUPER LES SONS", "CTRL+M", "muet"),
    (Commande::Reglages, "RÉGLAGES", "CTRL+,", "reglages options parametres"),
];

/// Commandes que l'étape « Commande » reprend pour un élément choisi : elles ne sont plus dans « Partout », où elles agiraient sur la
/// sélection sans qu'on voie sur quoi. Les autres (images, balises, SHA, vues, tri, réglages…) restent des commandes générales.
pub fn commande_pour_element(c: Commande) -> bool {
    commande_el_de(c).is_some()
}

/// La commande d'élément (étapes) qui reprend une commande générale de l'ancienne palette, quand il y en a une
pub fn commande_el_de(c: Commande) -> Option<CommandeEl> {
    Some(match c {
        Commande::Ouvrir => CommandeEl::Ouvrir,
        Commande::OuvrirOnglet => CommandeEl::OuvrirOnglet,
        Commande::Couper => CommandeEl::CouperPressePapiers,
        Commande::Copier => CommandeEl::CopierPressePapiers,
        Commande::Renommer => CommandeEl::Renommer,
        Commande::Supprimer => CommandeEl::Corbeille,
        Commande::Partager => CommandeEl::Partager,
        Commande::Proprietes => CommandeEl::Proprietes,
        Commande::CopierChemin => CommandeEl::CopierChemin,
        Commande::Zip => CommandeEl::CompresserVers,
        Commande::NouveauDossier => CommandeEl::NouveauDossier,
        // « Copier vers l'autre panneau » : « Copier vers… » met l'autre panneau en tête de ses destinations
        Commande::CopierVersAutre => CommandeEl::CopierVers,
        Commande::NouveauGroupe => CommandeEl::AjouterGroupe,
        _ => return None,
    })
}

/// Icône en points d'une commande générale : le jeu d'icônes de l'application n'a pas un symbole par commande, quelques-unes ont le
/// leur, les autres prennent celui de leur genre (affichage, réglages, fichier)
fn icone_commande(c: Commande) -> Icone {
    match c {
        Commande::VueListe => Icone::Texte,
        Commande::VueGrille => Icone::Image,
        Commande::VueColonnes | Commande::VueTailles => Icone::Tableur,
        Commande::Essentiel => Icone::Maison,
        Commande::Historique | Commande::RouvrirOnglet | Commande::Chronologie => Icone::Horloge,
        Commande::SeptZip | Commande::Extraire => Icone::Archive,
        Commande::ImageJpg | Commande::ImagePng | Commande::ImageReduire | Commande::ImageSansMeta => Icone::Image,
        Commande::Terminal | Commande::VsCode => Icone::Code,
        Commande::RechercheGlobale | Commande::ReconstruireIndex => Icone::Disque,
        Commande::TrierNom | Commande::TrierDate | Commande::TrierType | Commande::TrierTaille | Commande::TrierCroissant | Commande::TrierDecroissant => Icone::Bureau,
        Commande::Reglages | Commande::Sons | Commande::PoidsSonore | Commande::Palette | Commande::Administrateur | Commande::ModeCapture => Icone::Reglages,
        _ => Icone::Fichier,
    }
}

#[derive(Clone, Debug)]
pub enum Action {
    Commande(Commande),
    Aller(Emplacement),
    /// Groupes : créer (ou compléter) avec les éléments retenus, renommer
    MettreDansGroupe(String),
    RenommerGroupe(String, String),
    /// Sessions nommées : enregistrer les onglets sous ce nom, rouvrir une session
    EnregistrerSession(String),
    OuvrirSession(String),
    /// Note de dossier : dossier, texte (vide : supprimer)
    EnregistrerNote(String, String),
    /// Glyph de dossier : ces dossiers, le motif (None : l'enlever)
    ChoisirGlyphe(Vec<String>, Option<usize>),
    /// Règle de rangement à retenir (forme des réglages)
    AjouterRegle(String),
    // ── Palette à étapes : ces actions changent d'étape ou partent par `Suite`, elles ne ferment pas la palette ──
    /// Étape 1 : l'élément sur lequel on travaille
    ChoisirSujet(Sujet),
    /// Étape 2 : la commande
    ChoisirCommande(CommandeEl),
    /// Étape 1, requête tapée : une commande d'élément trouvée par son nom, sur cet élément (saute l'étape 2)
    CommandeSur(Sujet, CommandeEl),
    /// « Ajouter à un groupe… » : demander le nom d'un nouveau groupe dans la palette
    NouveauGroupe,
    /// Étape 3, destination : entrer dans ce dossier (Ctrl+Entrée : le valider tout de suite)
    EntrerDans(String),
    /// Étape 3, destination : « ici »
    ValiderIci(String),
    /// Étape 3, nom : le nom tapé
    ValiderNom(String),
    /// Étape 3, confirmation
    Confirmer,
    /// Revenir à l'étape de la commande
    Retour,
    /// Résultat : « Annuler »
    Defaire,
    /// Résultat : « Autre action sur… »
    AutreAction,
    /// Ligne d'information : Entrée ne fait rien
    Rien,
}

/// Ce que la palette demande
#[derive(Clone, Debug, PartialEq)]
pub enum Mode {
    /// La palette à étapes : élément › commande › destination
    Commandes,
    /// Nom du groupe où mettre ces éléments (nouveau ou existant)
    Groupe(Vec<String>),
    /// Nouveau nom du groupe
    Renommer(String),
    /// Nom de la session où ranger ces onglets
    Session(usize),
    /// Texte de la note de ce dossier
    Note(String),
    /// Motif Glyph de ces dossiers
    Glyphe(Vec<String>),
    /// Règle de rangement : « types > dossier »
    Regle,
}

#[derive(Clone)]
pub struct Item {
    pub libelle: String,
    pub detail: String,
    pub raccourci: &'static str,
    pub icone: Icone,
    pub action: Action,
    /// Titre de groupe (`.gh`) sous lequel l'élément se range
    pub groupe: &'static str,
    /// Ne s'applique pas ici : montré en retrait, avec sa raison dans `detail`, Entrée ne fait rien
    pub grise: bool,
}

impl Item {
    /// Identité de la ligne (ce qu'elle fait, sur quel chemin) : elle suit la ligne quand d'autres s'insèrent avant elle. Un rang ne le
    /// fait pas : Ctrl+Entrée visait alors un autre dossier que celui qu'on avait choisi.
    pub fn cle(&self) -> String {
        format!("{:?}|{}", self.action, self.libelle)
    }

    fn nouveau(libelle: impl Into<String>, detail: impl Into<String>, icone: Icone, action: Action, groupe: &'static str) -> Self {
        Self { libelle: libelle.into(), detail: detail.into(), raccourci: "", icone, action, groupe, grise: false }
    }
}

/// Fichier ou dossier rapporté par la recherche asynchrone (index) : de quoi en faire un élément ou une destination
#[derive(Clone, Debug, PartialEq)]
pub struct Trouve {
    pub chemin: String,
    pub dossier: bool,
    pub icone: Icone,
}

/// Ce que la palette fait après Entrée
#[derive(Clone, Debug, PartialEq)]
pub enum Suite {
    /// Elle a changé d'étape (ou rien) : la liste se refait
    Rien,
    /// Une commande générale ou un lieu : lancé par `executer_palette` (après le clin d'œil des yeux de Pixel)
    Lancer,
    /// La commande, l'élément et le paramètre sont prêts
    Executer(Execution),
    /// « Annuler » du résultat
    Defaire,
}

/// Ce que la palette attend pendant la progression
#[derive(Clone, Debug, PartialEq)]
pub struct Attente {
    /// « Copie en cours… »
    pub verbe: String,
    /// La tâche de fond (compression) rendra un message, pas un bilan d'opération
    pub tache: bool,
    /// C'est le « Annuler » de la palette qui tourne
    pub annulation: bool,
}

#[derive(Clone)]
pub struct Palette {
    pub mode: Mode,
    pub saisie: Edition,
    pub choix: usize,
    /// Ce que la liste montre à cette étape
    pub items: Vec<Item>,
    /// Une commande vient d'être lancée : les yeux de Pixel sont contents le temps de `LANCEMENT_MS`, la palette ne répond plus
    pub lance: bool,
    /// Décalage de la liste (DIP), tenu par le dessin : `scrollIntoView({block: 'nearest'})`
    pub defilement: std::cell::Cell<f32>,
    /// Clé de la liste au dernier réglage du décalage : une requête ou une étape différente le ramène à zéro
    pub cle_defilement: std::cell::Cell<u64>,
    // ── Palette à étapes ──
    pub etape: Etape,
    pub sujet: Option<Sujet>,
    pub commande: Option<CommandeEl>,
    /// Destination : le dossier dans lequel on a navigué (Entrée entre, Ctrl+Entrée valide)
    pub dossier_nav: Option<String>,
    /// Destination validée (nom de la puce du fil d'Ariane)
    pub destination: Option<String>,
    /// Résultats de la recherche asynchrone, et la requête pour laquelle ils valent
    pub trouves: Vec<Trouve>,
    pub trouves_pour: String,
    /// Dernière requête envoyée à l'index (une requête n'est lancée qu'une fois)
    pub recherche_lancee: String,
    /// Dossiers lus par le fil de fond (clé : chemin en minuscules) ; `None` : lecture demandée, pas encore revenue
    pub lectures: HashMap<String, Option<Lecture>>,
    pub attente: Option<Attente>,
    /// Avancement de l'opération en cours (0 à 1)
    pub progression: f32,
    pub resultat: Option<Resultat>,
    /// L'élément n'est plus où le sujet le dit (déplacé, mis à la corbeille, renommage défait) : « Autre action » repart de l'étape 1
    pub sujet_perime: bool,
    /// `cle_liste` au dernier `remplir` : la ligne choisie n'est retrouvée par sa clé que si la liste est la même (même requête, même étape)
    derniere_liste: u64,
    /// Mode « groupe » : « Créer un nouveau groupe… » a été choisi, le champ attend le nom du nouveau groupe
    pub nom_groupe_demande: bool,
}

/// Délai entre Entrée et le lancement : les yeux de Pixel passent contents (`palRun`, 160 ms)
pub const LANCEMENT_MS: u32 = 160;

/// Mode essai seulement (`SPACEFILES_SANS_REGISTRE=1` et `SPACEFILES_PALETTE_OUVERTE=1`) : la palette est ouverte au lancement, pour les
/// captures faites sans clavier (Ctrl+Maj+P ne peut pas être posté : il lit l'état réel des touches). Faux en usage normal.
pub fn ouverte_par_essai() -> bool {
    crate::defaut::sans_registre() && std::env::var("SPACEFILES_PALETTE_OUVERTE").is_ok_and(|v| v.trim() == "1")
}

/// Une ligne de la liste : titre de groupe (`.gh`) ou élément (rang dans `Palette::items`)
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LignePal {
    Titre(&'static str),
    Ligne(usize),
}

/// Hauteurs de la liste (`.pr` 46 DIP, `.gh` 28 DIP, remplissage de `.pl` : 6 en haut, 8 en bas)
pub const RANG_H: f32 = 46.0;
pub const TITRE_H: f32 = 28.0;
pub const LISTE_HAUT: f32 = 6.0;
pub const LISTE_BAS: f32 = 8.0;

/// Haut et hauteur de chaque ligne dans le contenu de la liste
pub fn geometrie(lignes: &[LignePal]) -> Vec<(f32, f32)> {
    let mut y = LISTE_HAUT;
    lignes
        .iter()
        .map(|l| {
            let h = if matches!(l, LignePal::Titre(_)) { TITRE_H } else { RANG_H };
            let r = (y, h);
            y += h;
            r
        })
        .collect()
}

/// Hauteur totale du contenu de la liste
pub fn hauteur_contenu(lignes: &[LignePal]) -> f32 {
    geometrie(lignes).last().map_or(LISTE_HAUT + LISTE_BAS, |(y, h)| y + h + LISTE_BAS)
}

/// Décalage de la liste après `scrollIntoView({block: 'nearest'})` de la ligne (`haut`, `hauteur`) : inchangé si elle est visible,
/// sinon le moins possible pour la montrer. Borné au contenu.
pub fn defilement_visible(actuel: f32, haut: f32, hauteur: f32, visible: f32, total: f32) -> f32 {
    let mut d = actuel;
    if haut < d {
        d = haut;
    } else if haut + hauteur > d + visible {
        d = haut + hauteur - visible;
    }
    d.clamp(0.0, (total - visible).max(0.0))
}

/// Plages (début, fin exclue, en lettres) de `texte` que `requete` fait correspondre (`hlq` de la maquette) : la sous-chaîne si elle
/// est là, accents et casse ignorés, espaces de la requête ignorés ; sinon les lettres de la requête dans l'ordre (sous-séquence),
/// chacune fondue avec sa voisine quand elles se touchent ; vide si les lettres ne s'y trouvent pas toutes.
pub fn plages_palette(texte: &str, requete: &str) -> Vec<(usize, usize)> {
    let n: Vec<char> = crate::dossier::normaliser(requete).chars().filter(|c| !c.is_whitespace()).collect();
    if n.is_empty() {
        return Vec::new();
    }
    let z: Vec<char> = crate::dossier::normaliser(texte).chars().collect();
    if n.len() <= z.len() {
        if let Some(i) = (0..=z.len() - n.len()).find(|&i| z[i..i + n.len()] == n[..]) {
            return vec![(i, i + n.len())];
        }
    }
    let mut marque = vec![false; z.len()];
    let mut j = 0;
    for (i, &c) in z.iter().enumerate() {
        if j < n.len() && c == n[j] {
            marque[i] = true;
            j += 1;
        }
    }
    if j < n.len() {
        return Vec::new();
    }
    let mut plages: Vec<(usize, usize)> = Vec::new();
    for (i, _) in marque.iter().enumerate().filter(|(_, &m)| m) {
        match plages.last_mut() {
            Some(p) if p.1 == i => p.1 = i + 1,
            _ => plages.push((i, i + 1)),
        }
    }
    plages
}

/// Ce que la colonne de droite (`.pv`) montre pour la ligne choisie
#[derive(Clone, Debug, PartialEq)]
pub struct ApercuPalette {
    pub titre: String,
    pub description: String,
    pub art: Art,
}

/// Contenu d'un dossier pour l'aperçu : « aller à », ou destination d'une copie
#[derive(Clone, Debug, PartialEq)]
pub struct Contenu {
    /// Premiers éléments du dossier (nom, taille ou vide, dossier ?)
    pub elements: Vec<(String, String, bool)>,
    /// Ce qui va arriver (nom, existe déjà ?) : ligne verte « + » ou rouge « ! »
    pub nouveaux: Vec<(String, bool)>,
    /// Lecture revenue (faux : en cours, ou partage réseau « non lu »)
    pub lu: bool,
    pub en_cours: bool,
    /// Phrase d'effet, et si elle avertit d'un conflit (rouge)
    pub effet: String,
    pub conflit: bool,
}

/// Le dessin de l'aperçu (`.art`)
#[derive(Clone, Debug, PartialEq)]
pub enum Art {
    /// Pas de dessin : seulement le titre et la description
    Aucun,
    Liste,
    Grille,
    Colonnes,
    Essentiel,
    NouveauDossier,
    Copier,
    Supprimer,
    Recherche,
    Reglages,
    /// Un dossier : ses premiers éléments, la place libre se lit dans la description
    Contenu(Box<Contenu>),
    /// Renommer, nouveau dossier : avant, après, et ce qui ne va pas (rouge)
    Nom { avant: String, apres: String, raison: Option<&'static str> },
    /// Une phrase d'effet (vert) ou un avertissement (rouge)
    Phrase { texte: String, avertissement: bool },
    /// Opération en cours : avancement de 0 à 1
    Progression(f32),
}

/// Aperçu d'une commande générale ou d'un lieu : titre, description, dessin. Les commandes sans dessin propre montrent leur raccourci
/// ou leurs mots-clés.
pub fn apercu_de(it: &Item) -> ApercuPalette {
    let (description, art) = match &it.action {
        Action::Commande(c) => {
            let art = match c {
                Commande::VueListe => Art::Liste,
                Commande::VueGrille => Art::Grille,
                Commande::VueColonnes => Art::Colonnes,
                Commande::Essentiel => Art::Essentiel,
                Commande::NouveauDossier => Art::NouveauDossier,
                Commande::Copier => Art::Copier,
                Commande::Supprimer => Art::Supprimer,
                Commande::RechercheGlobale => Art::Recherche,
                Commande::Reglages => Art::Reglages,
                _ => Art::Aucun,
            };
            let description = match c {
                Commande::VueListe => "Une ligne par fichier".to_string(),
                Commande::VueGrille => "Grandes vignettes".to_string(),
                Commande::VueColonnes => "Colonnes de Miller".to_string(),
                Commande::Essentiel => "Replie le volet latéral et l'aperçu".to_string(),
                Commande::NouveauDossier => "Crée un dossier dans ce dossier".to_string(),
                Commande::Copier => "Copie la sélection, à coller où l'on veut".to_string(),
                Commande::Supprimer => "Envoie à la corbeille, avec Annuler".to_string(),
                Commande::RechercheGlobale => "Cherche dans tout le PC par l'index".to_string(),
                Commande::Reglages => "Apparence, stockages, raccourcis".to_string(),
                _ => mots_de(*c, it.raccourci),
            };
            (description, art)
        }
        _ => (it.detail.clone(), Art::Aucun),
    };
    ApercuPalette { titre: it.libelle.clone(), description, art }
}

/// Description d'une commande sans texte propre : son raccourci, sinon ses mots-clés
fn mots_de(c: Commande, raccourci: &str) -> String {
    if !raccourci.is_empty() {
        return format!("Raccourci : {raccourci}");
    }
    COMMANDES.iter().find(|x| x.0 == c).map_or_else(String::new, |x| format!("Mots-clés : {}", x.3.split_whitespace().collect::<Vec<_>>().join(", ")))
}

/// Ce que la palette peut proposer en plus des commandes
pub struct Sources<'a> {
    pub epingles: &'a [String],
    pub recents: &'a [String],
    /// « dossier\tnom » : de quoi reprendre là où l'on en était
    pub recents_elements: Vec<(String, bool, Icone)>,
    /// Éléments du dossier ouvert : chemin, dossier ?, icône
    pub fichiers: Vec<(String, bool, Icone)>,
    pub groupes: &'a [crate::groupes::Groupe],
    /// « NOM|onglet|onglet… »
    pub sessions: &'a [String],
    /// La sélection de l'onglet : chemin, dossier ?, icône
    pub selection: Vec<(String, bool, Icone)>,
    pub dossier_ouvert: Option<String>,
    /// Lieux du volet : (nom, chemin)
    pub volet: Vec<(String, String)>,
    /// Dossier de l'autre panneau (double panneau)
    pub autre_panneau: Option<String>,
}

/// Éléments gardés au plus (hors résultats de l'index) : la table de commandes entière tient, un dossier énorme ne gonfle pas la liste
const MAX_ELEMENTS: usize = 300;

/// Éléments du dossier ouvert montrés sans requête : la liste reste courte, la requête atteint les autres
const DOSSIER_SANS_REQUETE: usize = 12;

/// Éléments récents montrés sans requête
const RECENTS_SANS_REQUETE: usize = 6;

/// Lieux (dossiers) gardés en mémoire pour l'aperçu : au-delà, on repart de zéro
const LECTURES_MAX: usize = 64;

/// Les lettres de la requête apparaissent-elles dans l'ordre ? Score : début de mot et lettres contiguës valent plus.
pub fn score(texte: &str, requete: &str) -> Option<i32> {
    if requete.is_empty() {
        return Some(0);
    }
    let t: Vec<char> = texte.to_lowercase().chars().collect();
    let r: Vec<char> = requete.to_lowercase().chars().filter(|c| !c.is_whitespace()).collect();
    if let Some(i) = texte.to_lowercase().find(&requete.to_lowercase()) {
        // Sous-chaîne exacte : le meilleur cas, surtout au début
        return Some(1000 - i as i32 * 3 - t.len() as i32);
    }
    let (mut j, mut s, mut precedent) = (0usize, 0i32, usize::MAX);
    for (i, &c) in t.iter().enumerate() {
        if j < r.len() && c == r[j] {
            s += if i == 0 || !t[i - 1].is_alphanumeric() { 12 } else if precedent != usize::MAX && i == precedent + 1 { 8 } else { 1 };
            precedent = i;
            j += 1;
        }
    }
    (j == r.len()).then_some(s - t.len() as i32 / 4)
}

fn nom_court(chemin: &str) -> String {
    if chemin.starts_with('?') {
        return format!("« {} »", &chemin[1..]).to_uppercase();
    }
    chemin.trim_end_matches('\\').rsplit('\\').next().unwrap_or(chemin).to_uppercase()
}

fn pluriel_s(n: usize) -> &'static str {
    if n > 1 {
        "S"
    } else {
        ""
    }
}

/// Clé de la table des lectures : chemin en minuscules, sans barre finale
fn cle_lecture(chemin: &str) -> String {
    chemin.trim_end_matches('\\').to_lowercase()
}

impl Palette {
    pub fn nouvelle() -> Self {
        Self {
            mode: Mode::Commandes,
            saisie: Edition::champ(""),
            choix: 0,
            items: Vec::new(),
            lance: false,
            defilement: std::cell::Cell::new(0.0),
            cle_defilement: std::cell::Cell::new(0),
            etape: Etape::Element,
            sujet: None,
            commande: None,
            dossier_nav: None,
            destination: None,
            trouves: Vec::new(),
            trouves_pour: String::new(),
            recherche_lancee: String::new(),
            lectures: HashMap::new(),
            attente: None,
            progression: 0.0,
            resultat: None,
            sujet_perime: false,
            derniere_liste: 0,
            nom_groupe_demande: false,
        }
    }

    /// Palette déjà remplie : l'omnibar y envoie ce qui n'est pas un chemin
    pub fn avec_texte(texte: &str) -> Self {
        Self { saisie: Edition::champ(texte), ..Self::nouvelle() }
    }

    /// Palette qui demande un nom de groupe pour ces éléments
    pub fn groupe(chemins: Vec<String>) -> Self {
        Self { mode: Mode::Groupe(chemins), ..Self::nouvelle() }
    }

    /// Palette qui demande le nom d'une session pour ces `n` onglets
    pub fn session(n: usize) -> Self {
        Self { mode: Mode::Session(n), ..Self::nouvelle() }
    }

    pub fn glyphe(dossiers: Vec<String>) -> Self {
        Self { mode: Mode::Glyphe(dossiers), ..Self::nouvelle() }
    }

    pub fn regle() -> Self {
        Self { mode: Mode::Regle, ..Self::nouvelle() }
    }

    pub fn note(dossier: &str, texte: &str) -> Self {
        Self { mode: Mode::Note(dossier.to_string()), saisie: Edition::champ(texte), ..Self::nouvelle() }
    }

    pub fn renommer_groupe(nom: &str) -> Self {
        Self { mode: Mode::Renommer(nom.to_string()), saisie: Edition::champ(nom), ..Self::nouvelle() }
    }

    /// « Créer un nouveau groupe… » : le champ se vide et attend le nom
    pub fn demander_nom_groupe(&mut self) {
        self.nom_groupe_demande = true;
        self.saisie = Edition::champ("");
        self.choix = 0;
    }

    /// C'est la palette à étapes (les autres modes demandent seulement un nom ou un choix dans leur liste)
    pub fn est_assistant(&self) -> bool {
        self.mode == Mode::Commandes
    }

    /// Ctrl+Z et la ligne « Annuler » n'existent que sur un résultat qui a une entrée d'historique à défaire : sans cela (renommage
    /// échoué, copie du chemin, compression) ils défairaient la dernière action de la pile globale, qui n'est pas celle-ci.
    pub fn peut_defaire(&self) -> bool {
        self.est_assistant() && self.etape == Etape::Resultat && self.resultat.as_ref().is_some_and(|r| r.annulable)
    }

    /// Pied de la carte : le compte de l'étape et les gestes (touche, libellé) qui y valent. ← revient et → / ↵ avance à chaque étape où
    /// il y a une étape avant ou après ; Retour arrière fait comme ←.
    pub fn pied(&self) -> (String, Vec<(&'static str, &'static str)>) {
        let param = self.commande.map(|c| assistant::def(c).param);
        match self.etape {
            Etape::Element => (format!("1 · ÉLÉMENT · {}", self.items.len()), vec![("↑↓", "choisir"), ("→ / ↵", "suivant"), ("ÉCHAP", "fermer")]),
            Etape::Commande => ("2 · COMMANDE".to_string(), vec![("↑↓", "choisir"), ("← / ⌫", "retour"), ("→ / ↵", "suivant")]),
            Etape::Parametre => match param {
                Some(Param::Destination) => ("3 · DESTINATION".to_string(), vec![("← / ⌫", "retour"), ("→ / ↵", "entrer"), ("CTRL ↵", "ici")]),
                Some(Param::Nom) => ("3 · NOM".to_string(), vec![("⌫", "retour"), ("↵", "valider")]),
                _ => ("3 · CONFIRMATION".to_string(), vec![("← / ⌫", "retour"), ("→ / ↵", "confirmer")]),
            },
            Etape::Progression => ("EN COURS".to_string(), vec![("ÉCHAP", "fermer")]),
            Etape::Resultat => ("TERMINÉ".to_string(), vec![("← / ⌫", "retour"), ("→ / ↵", "choisir"), ("CTRL Z", "annuler"), ("ÉCHAP", "fermer")]),
        }
    }

    /// La saisie est ouverte : pas pendant la progression ni sur le résultat
    pub fn accepte_saisie(&self) -> bool {
        !self.est_assistant() || !matches!(self.etape, Etape::Progression | Etape::Resultat)
    }

    /// Texte affiché dans le champ vide
    pub fn invite(&self) -> String {
        match &self.mode {
            Mode::Commandes => match self.etape {
                Etape::Element => "UN FICHIER, UN DOSSIER OU UNE COMMANDE… (3 LETTRES : TOUT LE PC)".into(),
                Etape::Commande => format!("COMMANDE POUR {}", self.sujet.as_ref().map(|s| s.libelle()).unwrap_or_default().to_uppercase()),
                Etape::Parametre => match self.commande.map(|c| assistant::def(c).param) {
                    Some(Param::Destination) => "DOSSIER DE DESTINATION · TAPEZ POUR CHERCHER".into(),
                    Some(Param::Nom) => "NOM".into(),
                    _ => "ENTRÉE : CONFIRMER · RETOUR ARRIÈRE : REVENIR".into(),
                },
                Etape::Progression => "OPÉRATION EN COURS…".into(),
                Etape::Resultat => "TERMINÉ · ÉCHAP : FERMER".into(),
            },
            Mode::Groupe(c) if self.nom_groupe_demande => format!("NOM DU NOUVEAU GROUPE ({} ELEMENT{})", c.len(), if c.len() > 1 { "S" } else { "" }),
            Mode::Groupe(c) => format!("NOM DU GROUPE ({} ELEMENT{}) · NOUVEAU OU EXISTANT", c.len(), if c.len() > 1 { "S" } else { "" }),
            Mode::Renommer(n) => format!("NOUVEAU NOM POUR {n}"),
            Mode::Note(_) => "NOTE DU DOSSIER (VIDE : SUPPRIMER)".into(),
            Mode::Regle => "TYPES > DOSSIER COMPLET · EX : PDF DOCX > D:\\DOCUMENTS\\FACTURES".into(),
            Mode::Glyphe(d) => format!("GLYPH POUR {} DOSSIER{}", d.len(), if d.len() > 1 { "S" } else { "" }),
            Mode::Session(n) => format!("NOM DE LA SESSION ({n} ONGLET{}) · NOUVELLE OU A REMPLACER", if *n > 1 { "S" } else { "" }),
        }
    }

    /// Propositions des modes « groupe » : créer, ou choisir un groupe existant
    fn remplir_groupe(&mut self, s: &Sources) -> bool {
        let q = self.requete().trim().to_uppercase();
        let item = |libelle: String, detail: String, action: Action| Item::nouveau(libelle, detail, Icone::Dossier, action, "ALLER À");
        let mut v = Vec::new();
        match &self.mode {
            Mode::Commandes => return false,
            Mode::Groupe(_) if self.nom_groupe_demande => {
                // Le nom du nouveau groupe se tape ici ; un nom déjà pris ajoute simplement au groupe qui existe
                if q.is_empty() {
                    let mut it = item("TAPEZ LE NOM DU NOUVEAU GROUPE".into(), "ENTRÉE : CRÉER ET AJOUTER".into(), Action::Rien);
                    it.grise = true;
                    v.push(it);
                } else {
                    let existe = s.groupes.iter().any(|g| g.nom == q);
                    let libelle = if existe { format!("AJOUTER AU GROUPE « {q} »") } else { format!("CRÉER LE GROUPE « {q} »") };
                    let detail = if existe { "EXISTE DÉJÀ" } else { "NOUVEAU" };
                    v.push(item(libelle, detail.to_string(), Action::MettreDansGroupe(q.clone())));
                }
            }
            Mode::Groupe(_) => {
                // « Créer un nouveau groupe… » toujours en tête ; une fois un nom tapé, la création directe le remplace
                if q.is_empty() {
                    v.push(item("CRÉER UN NOUVEAU GROUPE…".into(), "VOUS TAPEZ SON NOM ENSUITE".into(), Action::NouveauGroupe));
                } else if !s.groupes.iter().any(|g| g.nom == q) {
                    v.push(item(format!("CREER LE GROUPE « {q} »"), "NOUVEAU".into(), Action::MettreDansGroupe(q.clone())));
                }
                let mut existants: Vec<(i32, &crate::groupes::Groupe)> = s.groupes.iter().filter_map(|g| score(&g.nom, &q).map(|sc| (sc, g))).collect();
                existants.sort_by(|a, b| b.0.cmp(&a.0));
                for (_, g) in existants {
                    v.push(item(g.nom.clone(), format!("GROUPE · {} ELEMENT{}", g.nombre, if g.nombre > 1 { "S" } else { "" }), Action::MettreDansGroupe(g.nom.clone())));
                }
            }
            Mode::Session(_) => {
                if !q.is_empty() && !s.sessions.iter().any(|x| x.split('|').next() == Some(q.as_str())) {
                    v.push(item(format!("ENREGISTRER LA SESSION « {q} »"), "NOUVELLE".into(), Action::EnregistrerSession(q.clone())));
                }
                for x in s.sessions {
                    let nom = x.split('|').next().unwrap_or("");
                    if score(nom, &q).is_some() {
                        v.push(item(format!("REMPLACER « {nom} »"), format!("SESSION · {} ONGLET{}", x.split('|').count() - 1, if x.split('|').count() > 2 { "S" } else { "" }), Action::EnregistrerSession(nom.to_string())));
                    }
                }
            }
            Mode::Glyphe(dossiers) => {
                for (k, (nom, _)) in crate::glyphes::MOTIFS.iter().enumerate() {
                    if score(nom, &q).is_some() || q.is_empty() {
                        v.push(item(nom.to_string(), "GLYPH".into(), Action::ChoisirGlyphe(dossiers.clone(), Some(k))));
                    }
                }
                v.push(item("AUCUN GLYPH".into(), "ICONE NORMALE".into(), Action::ChoisirGlyphe(dossiers.clone(), None)));
            }
            Mode::Note(dossier) => {
                let t = self.requete().trim().to_string();
                let libelle = if t.is_empty() { "SUPPRIMER LA NOTE".to_string() } else { format!("NOTER « {} »", t.to_uppercase()) };
                v.push(item(libelle, "NOTE DU DOSSIER".into(), Action::EnregistrerNote(dossier.clone(), t)));
            }
            Mode::Regle => {
                if let Some(r) = crate::rangement::Regle::analyser(&self.requete()) {
                    v.push(item(format!("AJOUTER : {}", r.libelle()), "REGLE DE RANGEMENT".into(), Action::AjouterRegle(r.ecrire())));
                }
            }
            Mode::Renommer(ancien) => {
                if !q.is_empty() && &q != ancien {
                    v.push(item(format!("RENOMMER EN « {q} »"), ancien.clone(), Action::RenommerGroupe(ancien.clone(), q)));
                }
            }
        }
        self.items = v;
        self.choix = self.choix.min(self.items.len().saturating_sub(1));
        true
    }

    pub fn requete(&self) -> String {
        self.saisie.valeur()
    }

    /// Recalcule la liste de l'étape courante pour la requête tapée
    pub fn remplir(&mut self, s: &Sources) {
        let cle_avant = self.items.get(self.choix).map(Item::cle);
        if self.remplir_groupe(s) {
            return;
        }
        self.items = match self.etape {
            Etape::Element => self.items_element(s),
            Etape::Commande => self.items_commande(),
            Etape::Parametre => self.items_parametre(s),
            Etape::Progression => {
                let verbe = self.attente.as_ref().map_or_else(|| "OPÉRATION EN COURS…".to_string(), |a| a.verbe.to_uppercase());
                let mut it = Item::nouveau(verbe, "ÉCHAP : FERMER, ELLE CONTINUE", Icone::Horloge, Action::Rien, "EN COURS");
                it.grise = true;
                vec![it]
            }
            Etape::Resultat => self.items_resultat(),
        };
        // Même liste (même requête, même étape) refaite parce que des résultats arrivent : la ligne choisie est retrouvée par sa clé, pas
        // par son rang, sinon des lignes insérées avant elle changeraient en silence ce que Ctrl+Entrée valide
        let liste = self.cle_liste();
        let meme_liste = liste == self.derniere_liste;
        self.derniere_liste = liste;
        let retrouvee = cle_avant.filter(|_| meme_liste).and_then(|c| self.items.iter().position(|i| i.cle() == c));
        if let Some(k) = retrouvee {
            self.choix = k;
            return;
        }
        self.choix = self.choix.min(self.items.len().saturating_sub(1));
        // Sur le résultat, Entrée ne doit jamais défaire : « Annuler » est en tête de liste mais la ligne choisie est « Autre action »
        // (un double Entrée renommait puis annulait)
        if self.etape == Etape::Resultat {
            if let Some(k) = self.items.iter().position(|i| matches!(i.action, Action::AutreAction)) {
                self.choix = k;
            }
        }
        // Un résultat ou une commande grisée ne se choisit pas par défaut : la première ligne utile en tête
        if self.items.get(self.choix).is_some_and(|i| i.grise) {
            if let Some(k) = self.items.iter().position(|i| !i.grise) {
                self.choix = k;
            }
        }
    }

    // ── Étape 1 : l'élément ──

    fn items_element(&self, s: &Sources) -> Vec<Item> {
        let requete = self.requete();
        let q = requete.trim();
        let mut v: Vec<Item> = Vec::new();
        let mut vus: std::collections::HashSet<String> = std::collections::HashSet::new();
        let sujet_item = |sujet: Sujet, detail: String, groupe: &'static str, v: &mut Vec<Item>| {
            let libelle = sujet.libelle();
            if !q.is_empty() && score(&libelle, q).is_none() {
                return;
            }
            let icone = sujet.icone;
            v.push(Item::nouveau(libelle, detail, icone, Action::ChoisirSujet(sujet), groupe));
        };
        // La sélection d'abord (préchoisie) ; sans sélection, le dossier ouvert
        match s.selection.len() {
            0 => {
                if let Some(d) = &s.dossier_ouvert {
                    sujet_item(Sujet::dossier_ouvert(d.clone()), format!("DOSSIER OUVERT · {d}"), "DOSSIER OUVERT", &mut v);
                }
            }
            1 => {
                let (c, dossier, icone) = &s.selection[0];
                vus.insert(c.to_lowercase());
                sujet_item(Sujet::un(c.clone(), *dossier, *icone), format!("SÉLECTION · {}", crate::dossier::dossier_de(c)), "SÉLECTION", &mut v);
            }
            n => {
                let chemins: Vec<String> = s.selection.iter().map(|x| x.0.clone()).collect();
                vus.extend(chemins.iter().map(|c| c.to_lowercase()));
                sujet_item(Sujet::plusieurs(chemins), format!("SÉLECTION · {n} ÉLÉMENTS"), "SÉLECTION", &mut v);
            }
        }
        // Récents : de vrais fichiers et dossiers ouverts récemment (jamais une commande ni un « reprendre »)
        let mut recents: Vec<(i32, &(String, bool, Icone))> = s
            .recents_elements
            .iter()
            .filter(|r| !vus.contains(&r.0.to_lowercase()))
            .filter_map(|r| score(&assistant::nom_de(&r.0), q).or_else(|| (!q.is_empty() && r.0.to_lowercase().contains(&q.to_lowercase())).then_some(-300)).map(|sc| (sc, r)))
            .collect();
        if q.is_empty() {
            recents.truncate(RECENTS_SANS_REQUETE);
        } else {
            recents.sort_by(|a, b| b.0.cmp(&a.0));
            recents.truncate(8);
        }
        for (_, (c, d, i)) in recents {
            vus.insert(c.to_lowercase());
            let detail = if *d { format!("RÉCENT · {c}") } else { format!("RÉCENT · {}", crate::dossier::dossier_de(c)) };
            v.push(Item::nouveau(assistant::nom_de(c), detail, *i, Action::ChoisirSujet(Sujet::un(c.clone(), *d, *i)), "RÉCENTS"));
        }
        // Les éléments du dossier ouvert : les premiers sans requête, les meilleurs avec
        let mut dossier: Vec<(i32, &(String, bool, Icone))> = s.fichiers.iter().filter(|f| !vus.contains(&f.0.to_lowercase())).filter_map(|f| score(&assistant::nom_de(&f.0), q).map(|sc| (sc, f))).collect();
        if q.is_empty() {
            dossier.truncate(DOSSIER_SANS_REQUETE);
        } else {
            dossier.sort_by(|a, b| b.0.cmp(&a.0));
            dossier.truncate(60);
        }
        for (_, (c, d, i)) in dossier {
            vus.insert(c.to_lowercase());
            v.push(Item::nouveau(assistant::nom_de(c), if *d { "DOSSIER ICI" } else { "ICI" }, *i, Action::ChoisirSujet(Sujet::un(c.clone(), *d, *i)), "DANS CE DOSSIER"));
        }
        // Tout le PC : ce que l'index a rapporté pour cette requête (asynchrone)
        if q.chars().count() >= 3 && self.trouves_pour == q {
            for t in self.trouves.iter().filter(|t| vus.insert(t.chemin.to_lowercase())).take(8) {
                v.push(Item::nouveau(assistant::nom_de(&t.chemin), format!("PC · {}", crate::dossier::dossier_de(&t.chemin)), t.icone, Action::ChoisirSujet(Sujet::un(t.chemin.clone(), t.dossier, t.icone)), "RÉSULTATS"));
            }
        }
        // Une commande d'élément tapée par son nom (« renommer », « corbeille »…) : proposée sur la sélection (ou le dossier ouvert)
        if !q.is_empty() {
            let cible = match s.selection.len() {
                0 => s.dossier_ouvert.clone().map(Sujet::dossier_ouvert),
                1 => Some(Sujet::un(s.selection[0].0.clone(), s.selection[0].1, s.selection[0].2)),
                _ => Some(Sujet::plusieurs(s.selection.iter().map(|x| x.0.clone()).collect())),
            };
            if let Some(sujet) = cible {
                let mut trouvees: Vec<(i32, &assistant::DefCommande)> =
                    assistant::TABLE.iter().filter(|d| assistant::indisponible(d.commande, &sujet).is_none()).filter_map(|d| score(d.libelle, q).or_else(|| score(d.mots, q).map(|x| x - 200)).map(|sc| (sc, d))).collect();
                trouvees.sort_by(|a, b| b.0.cmp(&a.0));
                for (_, d) in trouvees.into_iter().take(6) {
                    let mut it = Item::nouveau(d.libelle, format!("SUR {}", sujet.libelle().to_uppercase()), d.icone, Action::CommandeSur(sujet.clone(), d.commande), "SUR L'ÉLÉMENT");
                    it.raccourci = d.raccourci;
                    v.push(it);
                }
            }
        }
        v.extend(self.items_partout(s, q));
        v
    }

    /// « Partout » : ce qui ne s'applique à aucun élément (vues, tri, réglages…), les lieux, les groupes, les sessions. Choisir une de
    /// ces lignes la lance tout de suite, comme avant les étapes.
    fn items_partout(&self, s: &Sources, q: &str) -> Vec<Item> {
        let mut v: Vec<(i32, Item)> = Vec::new();
        // « groupe:rouge+bleu,vert » : les éléments de ROUGE et BLEU à la fois, ou de VERT
        if let Some(expr) = q.to_lowercase().strip_prefix("groupe:").map(|e| e.trim().to_uppercase()).filter(|e| !e.is_empty()) {
            v.push((1000, Item::nouveau(format!("GROUPES : {}", expr.replace('+', " ET ").replace(',', " OU ")), "+ : ET · , : OU", Icone::Dossier, Action::Aller(Emplacement::Groupe(expr)), "PARTOUT")));
        }
        for g in s.groupes {
            if let Some(sc) = score(&g.nom, q) {
                v.push((sc + 30, Item::nouveau(g.nom.clone(), format!("GROUPE · {} ELEMENT{}", g.nombre, pluriel_s(g.nombre)), Icone::Dossier, Action::Aller(Emplacement::Groupe(g.nom.clone())), "PARTOUT")));
            }
        }
        for x in s.sessions {
            let nom = x.split('|').next().unwrap_or("");
            if let Some(sc) = score(nom, q).or_else(|| score("session", q).map(|x| x - 100)) {
                let n = x.split('|').count() - 1;
                v.push((sc + 30, Item::nouveau(nom.to_string(), format!("SESSION · {n} ONGLET{}", pluriel_s(n)), Icone::Dossier, Action::OuvrirSession(nom.to_string()), "PARTOUT")));
            }
        }
        let experimental = crate::reglages::experimental();
        for &(c, libelle, raccourci, mots) in COMMANDES.iter().filter(|(c, ..)| commande_permise(experimental, *c) && !commande_pour_element(*c)) {
            let sc = score(libelle, q).or_else(|| score(mots, q).map(|x| x - 200));
            if let Some(sc) = sc {
                let mut it = Item::nouveau(libelle, "", icone_commande(c), Action::Commande(c), "PARTOUT");
                it.raccourci = raccourci;
                v.push((sc + 50, it));
            }
        }
        // Les dossiers récents ne sont plus ici : ce sont de vrais éléments, section « RÉCENTS » de l'étape 1 (`recents_elements`)
        for (liste, quoi, bonus) in [(s.epingles, "EPINGLE", 40)] {
            for c in liste.iter().filter(|c| crate::reglages::adresse_permise(experimental, c)) {
                let dans_chemin = !q.is_empty() && c.to_lowercase().contains(&q.to_lowercase());
                if let Some(sc) = score(&nom_court(c), q).or_else(|| dans_chemin.then_some(-300)) {
                    // Pas de doublon : un dossier épinglé n'est pas répété dans les récents
                    if v.iter().any(|(_, it)| matches!(&it.action, Action::Aller(Emplacement::Dossier(d)) if d.eq_ignore_ascii_case(c))) {
                        continue;
                    }
                    v.push((sc + bonus - if q.is_empty() { 0 } else { 20 }, Item::nouveau(nom_court(c), format!("{quoi} · {c}"), Icone::Dossier, Action::Aller(Emplacement::dossier(c)), "PARTOUT")));
                }
            }
        }
        // Le meilleur score en tête
        v.sort_by(|a, b| b.0.cmp(&a.0));
        v.truncate(MAX_ELEMENTS);
        v.into_iter().map(|x| x.1).collect()
    }

    // ── Étape 2 : la commande ──

    fn items_commande(&self) -> Vec<Item> {
        let Some(sujet) = &self.sujet else { return Vec::new() };
        let requete = self.requete();
        let q = requete.trim();
        let mut v: Vec<(i32, Item)> = Vec::new();
        for d in assistant::TABLE {
            let Some(sc) = score(d.libelle, q).or_else(|| score(d.mots, q).map(|x| x - 200)) else { continue };
            let raison = assistant::indisponible(d.commande, sujet);
            let mut it = Item::nouveau(d.libelle, raison.unwrap_or(d.param.annonce()), d.icone, Action::ChoisirCommande(d.commande), if raison.is_some() { "NE S'APPLIQUE PAS ICI" } else { "COMMANDES" });
            it.raccourci = d.raccourci;
            it.grise = raison.is_some();
            v.push((sc, it));
        }
        // Les commandes qui s'appliquent d'abord, dans l'ordre de la table sans requête, par score avec une requête
        let ordre = |it: &Item| it.grise;
        if q.is_empty() {
            v.sort_by_key(|x| ordre(&x.1));
        } else {
            v.sort_by(|a, b| ordre(&a.1).cmp(&ordre(&b.1)).then(b.0.cmp(&a.0)));
        }
        v.into_iter().map(|x| x.1).collect()
    }

    // ── Étape 3 : le paramètre ──

    fn items_parametre(&self, s: &Sources) -> Vec<Item> {
        let (Some(sujet), Some(c)) = (&self.sujet, self.commande) else { return Vec::new() };
        match assistant::def(c).param {
            Param::Destination => self.items_destination(s, c),
            Param::Nom => self.items_nom(sujet, c),
            Param::Confirmation => vec![
                Item::nouveau(format!("{} · {}", assistant::def(c).libelle, sujet.libelle().to_uppercase()), "ENTRÉE : CONFIRMER", assistant::def(c).icone, Action::Confirmer, "CONFIRMATION"),
                Item::nouveau("NON, REVENIR", "ÉCHAP FERME LA PALETTE", Icone::Horloge, Action::Retour, "CONFIRMATION"),
            ],
            Param::Aucun => Vec::new(),
        }
    }

    fn verbe_ici(c: CommandeEl) -> &'static str {
        match c {
            CommandeEl::DeplacerVers => "DÉPLACER ICI",
            CommandeEl::CompresserVers => "COMPRESSER ICI",
            _ => "COPIER ICI",
        }
    }

    fn items_destination(&self, s: &Sources, c: CommandeEl) -> Vec<Item> {
        let requete = self.requete();
        let q = requete.trim();
        let dossiers_trouves: Vec<String> = if q.chars().count() >= 3 && self.trouves_pour == q { self.trouves.iter().filter(|t| t.dossier).take(12).map(|t| t.chemin.clone()).collect() } else { Vec::new() };
        if let Some(nav) = &self.dossier_nav {
            // Navigation : « ici » en tête, le parent, puis les sous-dossiers (lus par le fil de fond)
            let mut v = vec![Item::nouveau(format!("{} · {}", Self::verbe_ici(c), assistant::nom_de(nav).to_uppercase()), "CTRL+ENTRÉE", assistant::def(c).icone, Action::ValiderIci(nav.clone()), "ICI")];
            let parent = crate::dossier::dossier_de(nav.trim_end_matches('\\')).to_string();
            if !parent.eq_ignore_ascii_case(nav.trim_end_matches('\\')) && (q.is_empty() || score(&assistant::nom_de(&parent), q).is_some()) {
                v.push(Item::nouveau(format!("{} · PARENT", assistant::nom_de(&parent).to_uppercase()), parent.clone(), Icone::Dossier, Action::EntrerDans(parent), "ICI"));
            }
            match self.lecture(nav) {
                Some(l) if l.lu => {
                    for n in l.sous_dossiers.iter().filter(|n| score(n, q).is_some()) {
                        v.push(Item::nouveau(n.clone(), "ENTRÉE : ENTRER", Icone::Dossier, Action::EntrerDans(crate::dossier::joindre(nav, n)), "SOUS-DOSSIERS"));
                    }
                }
                Some(_) => {
                    let mut it = Item::nouveau("SOUS-DOSSIERS NON LUS", "DISQUE RÉSEAU OU AMOVIBLE · CTRL+ENTRÉE POUR VALIDER ICI", Icone::Disque, Action::Rien, "SOUS-DOSSIERS");
                    it.grise = true;
                    v.push(it);
                }
                None => {
                    let mut it = Item::nouveau("LECTURE DU DOSSIER…", "", Icone::Horloge, Action::Rien, "SOUS-DOSSIERS");
                    it.grise = true;
                    v.push(it);
                }
            }
            return v;
        }
        let courant = s.dossier_ouvert.as_deref();
        let lieux = assistant::Lieux { autre_panneau: s.autre_panneau.as_deref(), recents: s.recents, epingles: s.epingles, volet: &s.volet, courant };
        assistant::destinations(&lieux, q, &dossiers_trouves)
            .into_iter()
            // Pas de destination réseau : copier ou déplacer y teste les noms pris sur le fil d'interface, qui attendrait le réseau
            .filter(|d| !assistant::destination_distante(&d.chemin))
            .map(|d| Item::nouveau(d.libelle, d.detail, d.icone, Action::EntrerDans(d.chemin), d.groupe))
            .collect()
    }

    /// Dossier où le nom saisi sera créé ou pris : le parent de l'élément (renommer), l'élément lui-même (nouveau dossier dedans)
    pub fn dossier_du_nom(&self) -> Option<String> {
        let s = self.sujet.as_ref()?;
        match self.commande? {
            CommandeEl::NouveauDossier => s.chemins.first().cloned(),
            _ => s.parent(),
        }
    }

    /// État du nom saisi pour renommer ou créer, vérifié sur la lecture du dossier (jamais sur le disque, ici)
    pub fn etat_du_nom(&self) -> assistant::EtatNom {
        let nom = self.requete();
        let ancien = (self.commande == Some(CommandeEl::Renommer)).then(|| self.sujet.as_ref().map(|s| s.libelle())).flatten();
        let existants = self.dossier_du_nom().and_then(|d| self.lecture(&d).filter(|l| l.lu).map(|l| &l.noms));
        assistant::etat_nom(&nom, ancien.as_deref(), existants)
    }

    fn items_nom(&self, sujet: &Sujet, c: CommandeEl) -> Vec<Item> {
        let nom = self.requete().trim().to_string();
        let etat = self.etat_du_nom();
        let (verbe, icone) = if c == CommandeEl::Renommer { ("RENOMMER EN", Icone::Texte) } else { ("CRÉER", Icone::Dossier) };
        let detail = if c == CommandeEl::Renommer { sujet.libelle() } else { format!("DANS {}", sujet.libelle().to_uppercase()) };
        match etat.raison() {
            Some(r) => {
                let mut it = Item::nouveau(if nom.is_empty() { verbe.to_string() } else { format!("{verbe} « {nom} »") }, r, icone, Action::Rien, "NOM");
                it.grise = true;
                vec![it]
            }
            None => {
                let suite = if etat == assistant::EtatNom::NonVerifie { format!("{detail} · DOSSIER NON LU : L'OPÉRATION VÉRIFIERA") } else { detail };
                vec![Item::nouveau(format!("{verbe} « {nom} »"), suite, icone, Action::ValiderNom(nom), "NOM")]
            }
        }
    }

    // ── Résultat ──

    fn items_resultat(&self) -> Vec<Item> {
        let Some(r) = &self.resultat else { return Vec::new() };
        let nom = self.sujet.as_ref().map(|s| s.libelle()).unwrap_or_default();
        let mut v = Vec::new();
        if r.annulable {
            v.push(Item::nouveau("ANNULER", "CTRL+Z", Icone::Horloge, Action::Defaire, "ET ENSUITE"));
        }
        let libelle = if self.sujet_perime { "AUTRE ACTION".to_string() } else { format!("AUTRE ACTION SUR {}", nom.to_uppercase()) };
        v.push(Item::nouveau(libelle, "ENTRÉE", Icone::Fichier, Action::AutreAction, "ET ENSUITE"));
        v
    }

    // ── Lectures de dossiers et recherche asynchrone ──

    /// Lecture prête d'un dossier (`None` : demandée mais pas revenue, ou jamais demandée)
    pub fn lecture(&self, chemin: &str) -> Option<&Lecture> {
        self.lectures.get(&cle_lecture(chemin)).and_then(|l| l.as_ref())
    }

    /// Le fil de fond a lu ce dossier. Gardée seulement si la lecture était attendue (entrée `None`) : une lecture partie avant la fin
    /// d'une opération arrive après `finir` (qui vide la table) avec l'état d'avant, et ne doit pas repeupler le cache. Vrai si gardée.
    pub fn recevoir_lecture(&mut self, l: Lecture) -> bool {
        let cle = cle_lecture(&l.chemin);
        if !matches!(self.lectures.get(&cle), Some(None)) {
            return false;
        }
        self.lectures.insert(cle, Some(l));
        true
    }

    /// Les résultats de l'index arrivent : gardés seulement s'ils valent pour ce qui est tapé maintenant
    pub fn recevoir_recherche(&mut self, requete: &str, trouves: Vec<Trouve>) -> bool {
        if self.requete().trim() != requete {
            return false;
        }
        self.trouves = trouves;
        self.trouves_pour = requete.to_string();
        true
    }

    /// La recherche de cette requête a échoué (index en construction…) : elle pourra être redemandée. Sans cela, `recherche_lancee`
    /// la tenait pour faite et la palette montrait « rien trouvé » jusqu'à la prochaine frappe.
    pub fn recherche_echouee(&mut self, requete: &str) {
        if self.recherche_lancee == requete {
            self.recherche_lancee.clear();
        }
    }

    /// Dossier que la ligne choisie désigne (de quoi montrer son contenu à droite)
    fn chemin_survole(&self) -> Option<String> {
        let it = self.items.get(self.choix)?;
        match (&it.action, self.etape) {
            (Action::EntrerDans(c) | Action::ValiderIci(c), Etape::Parametre) => Some(c.clone()),
            (Action::ChoisirSujet(s), Etape::Element) if s.genre == GenreSujet::Dossier => s.chemins.first().cloned(),
            (Action::Aller(Emplacement::Dossier(d)), Etape::Element) => Some(d.clone()),
            _ => None,
        }
    }

    /// Ce que le fil d'interface doit lancer en fond maintenant : (requête à chercher dans l'index, dossiers à lire). Chaque chose n'est
    /// demandée qu'une fois ; l'appelant lance les fils (`commandes::assistant`).
    pub fn a_demander(&mut self) -> (Option<String>, Vec<String>) {
        if !self.est_assistant() {
            return (None, Vec::new());
        }
        let requete = self.requete();
        let q = requete.trim().to_string();
        let cherche = matches!(self.etape, Etape::Element) || (self.etape == Etape::Parametre && self.commande.map(|c| assistant::def(c).param) == Some(Param::Destination) && self.dossier_nav.is_none());
        let recherche = (cherche && q.chars().count() >= 3 && self.recherche_lancee != q).then(|| {
            self.recherche_lancee = q.clone();
            q.clone()
        });
        let mut voulus: Vec<String> = Vec::new();
        match self.etape {
            Etape::Element | Etape::Parametre => {
                voulus.extend(self.chemin_survole());
                if let Some(n) = &self.dossier_nav {
                    voulus.push(n.clone());
                }
                if self.etape == Etape::Parametre && self.commande.map(|c| assistant::def(c).param) == Some(Param::Nom) {
                    voulus.extend(self.dossier_du_nom());
                }
            }
            _ => {}
        }
        if self.lectures.len() > LECTURES_MAX {
            self.lectures.clear();
        }
        let mut a_lire = Vec::new();
        for c in voulus {
            if let std::collections::hash_map::Entry::Vacant(e) = self.lectures.entry(cle_lecture(&c)) {
                e.insert(None);
                a_lire.push(c);
            }
        }
        (recherche, a_lire)
    }

    // ── Transitions ──

    /// Rien n'a plus de sens pour l'étape quittée : saisie vide, retour en haut de la liste
    fn repartir(&mut self, etape: Etape) {
        self.etape = etape;
        self.saisie = Edition::champ("");
        self.choix = 0;
    }

    /// Entrée (Ctrl+Entrée : valide le dossier visé comme « ici »)
    pub fn entrer(&mut self, ctrl: bool) -> Suite {
        let Some(it) = self.items.get(self.choix).cloned() else { return Suite::Rien };
        if it.grise {
            return Suite::Rien;
        }
        let pret = |p: &Self, dossier: Option<String>, nom: Option<String>| Some(Execution { commande: p.commande?, sujet: p.sujet.clone()?, dossier, nom });
        match it.action {
            Action::ChoisirSujet(s) => {
                self.sujet = Some(s);
                self.sujet_perime = false;
                self.commande = None;
                self.repartir(Etape::Commande);
                Suite::Rien
            }
            Action::ChoisirCommande(c) => self.choisir_commande(c),
            Action::CommandeSur(sujet, c) => {
                self.sujet = Some(sujet);
                self.sujet_perime = false;
                self.choisir_commande(c)
            }
            Action::EntrerDans(d) if ctrl => self.valider_dossier(d),
            Action::EntrerDans(d) => {
                self.dossier_nav = Some(d);
                self.repartir(Etape::Parametre);
                Suite::Rien
            }
            Action::ValiderIci(d) => self.valider_dossier(d),
            Action::ValiderNom(n) => pret(self, None, Some(n)).map_or(Suite::Rien, Suite::Executer),
            Action::Confirmer => pret(self, None, None).map_or(Suite::Rien, Suite::Executer),
            Action::Retour => {
                self.repartir(Etape::Commande);
                Suite::Rien
            }
            Action::Defaire => Suite::Defaire,
            Action::AutreAction => {
                self.resultat = None;
                self.attente = None;
                self.destination = None;
                self.commande = None;
                self.dossier_nav = None;
                self.vers_commande();
                Suite::Rien
            }
            Action::Rien => Suite::Rien,
            _ => Suite::Lancer,
        }
    }

    /// La commande est choisie : elle part tout de suite si elle ne demande rien, sinon l'étape de son paramètre s'ouvre
    fn choisir_commande(&mut self, c: CommandeEl) -> Suite {
        self.commande = Some(c);
        self.dossier_nav = None;
        match assistant::def(c).param {
            Param::Aucun => match self.sujet.clone() {
                Some(sujet) => Suite::Executer(Execution { commande: c, sujet, dossier: None, nom: None }),
                None => Suite::Rien,
            },
            param => {
                self.repartir(Etape::Parametre);
                // Renommer : le nom d'avant est dans le champ, son radical choisi comme dans la liste
                if param == Param::Nom && c == CommandeEl::Renommer {
                    if let Some(s) = &self.sujet {
                        self.saisie = Edition::nouveau(0, &s.libelle(), s.genre == GenreSujet::Dossier);
                    }
                }
                Suite::Rien
            }
        }
    }

    fn valider_dossier(&mut self, d: String) -> Suite {
        self.destination = Some(assistant::nom_de(&d));
        match (self.commande, self.sujet.clone()) {
            (Some(commande), Some(sujet)) => Suite::Executer(Execution { commande, sujet, dossier: Some(d), nom: None }),
            _ => Suite::Rien,
        }
    }

    /// Retour arrière sur un champ vide : l'étape d'avant (sur la puce « élément », la recherche d'un autre élément rouvre). Vrai si elle a changé.
    pub fn reculer(&mut self) -> bool {
        match self.etape {
            Etape::Element | Etape::Progression => false,
            Etape::Commande => {
                self.sujet = None;
                self.repartir(Etape::Element);
                true
            }
            Etape::Parametre | Etape::Resultat => {
                self.resultat = None;
                self.commande = None;
                self.dossier_nav = None;
                self.vers_commande();
                true
            }
        }
    }

    /// Retour à l'étape de la commande ; si l'élément n'est plus là où le sujet le dit (déplacé, mis à la corbeille…), à l'étape de
    /// l'élément, sujet oublié : sinon la corbeille viserait l'ancien chemin
    fn vers_commande(&mut self) {
        if std::mem::take(&mut self.sujet_perime) {
            self.sujet = None;
            self.repartir(Etape::Element);
        } else {
            self.repartir(Etape::Commande);
        }
    }

    /// Le sujet est renommé : « Autre action » vise le nouveau nom
    pub fn sujet_renomme(&mut self, nouveau_chemin: String) {
        if let Some(s) = &mut self.sujet {
            s.chemins = vec![nouveau_chemin];
        }
    }

    /// Clic sur une puce du fil d'Ariane : retour à cette étape (rang 0 : élément, 1 : commande, 2 : paramètre). Vrai si elle a changé.
    pub fn aller_etape(&mut self, rang: usize) -> bool {
        if self.etape == Etape::Progression || rang >= self.etape.rang() {
            return false;
        }
        match rang {
            0 => {
                self.sujet = None;
                self.sujet_perime = false;
                self.commande = None;
                self.resultat = None;
                self.dossier_nav = None;
                self.repartir(Etape::Element);
            }
            1 => {
                self.commande = None;
                self.resultat = None;
                self.dossier_nav = None;
                self.vers_commande();
            }
            _ => return false,
        }
        true
    }

    /// L'opération est partie : la palette montre l'avancement
    pub fn en_cours(&mut self, verbe: &str, tache: bool, annulation: bool) {
        self.attente = Some(Attente { verbe: verbe.to_string(), tache, annulation });
        self.progression = 0.0;
        self.resultat = None;
        self.repartir(Etape::Progression);
    }

    /// L'opération est finie : le résultat, avec « Annuler » s'il se peut. Les dossiers lus sont périmés.
    pub fn finir(&mut self, r: Resultat) {
        self.attente = None;
        self.lectures.clear();
        // Déplacé ou jeté : l'élément n'est plus au chemin du sujet. Renommage défait (« Annulé ») : il a repris son ancien nom. Un
        // échec ne change rien, sauf pour un déplacement ou une corbeille, qui ont pu aboutir en partie.
        self.sujet_perime = match self.commande {
            Some(CommandeEl::DeplacerVers | CommandeEl::Corbeille) => true,
            Some(CommandeEl::Renommer) => !r.erreur && !r.annulable,
            _ => false,
        };
        self.resultat = Some(r);
        self.repartir(Etape::Resultat);
    }

    /// Fil d'Ariane : (texte, rang de l'étape, courante ?) des puces à montrer
    pub fn fil(&self) -> Vec<(String, usize, bool)> {
        let sujet = self.sujet.as_ref().map(|s| s.libelle()).unwrap_or_default();
        let cmd = self.commande.map(assistant::libelle_court).unwrap_or_default();
        let param = match self.commande.map(|c| assistant::def(c).param) {
            Some(Param::Destination) => "Destination",
            Some(Param::Nom) => "Nom",
            Some(Param::Confirmation) => "Confirmation",
            _ => "",
        };
        match self.etape {
            Etape::Element => vec![("Élément".into(), 0, true)],
            Etape::Commande => vec![(sujet, 0, false), ("Commande".into(), 1, true)],
            Etape::Parametre => vec![(sujet, 0, false), (cmd, 1, false), (param.into(), 2, true)],
            Etape::Progression | Etape::Resultat => {
                let mut v = vec![(sujet, 0, false), (cmd, 1, false)];
                if let Some(d) = &self.destination {
                    v.push((d.clone(), 2, false));
                }
                v
            }
        }
    }

    /// Lignes de la liste : un titre de groupe à chaque changement de groupe (`c.g !== g` de la maquette)
    pub fn lignes(&self) -> Vec<LignePal> {
        let mut v = Vec::with_capacity(self.items.len() + 5);
        let mut groupe = "";
        for (k, it) in self.items.iter().enumerate() {
            if it.groupe != groupe {
                groupe = it.groupe;
                v.push(LignePal::Titre(it.groupe));
            }
            v.push(LignePal::Ligne(k));
        }
        v
    }

    /// Change à chaque requête, étape ou dossier navigué différent : la pastille de sélection saute au lieu de glisser, le décalage revient à zéro
    pub fn cle_liste(&self) -> u64 {
        let mut h = std::collections::hash_map::DefaultHasher::new();
        self.requete().hash(&mut h);
        self.etape.rang().hash(&mut h);
        (self.etape as u8).hash(&mut h);
        self.dossier_nav.hash(&mut h);
        h.finish()
    }

    /// Identité de la ligne choisie : une autre rejoue l'apparition de l'aperçu
    pub fn cle_commande(&self) -> u64 {
        let mut h = std::collections::hash_map::DefaultHasher::new();
        match self.items.get(self.choix) {
            Some(i) => {
                i.libelle.hash(&mut h);
                i.detail.hash(&mut h);
            }
            None => 0u8.hash(&mut h),
        }
        (self.etape as u8).hash(&mut h);
        h.finish()
    }

    /// Ce que la colonne de droite montre : la ligne choisie, avec ce que l'étape en dit
    pub fn apercu(&self) -> Option<ApercuPalette> {
        if !self.est_assistant() {
            return self.items.get(self.choix).map(apercu_de);
        }
        let sujet = self.sujet.as_ref();
        let nom = sujet.map(|s| s.libelle()).unwrap_or_default();
        match self.etape {
            Etape::Element => {
                let it = self.items.get(self.choix)?;
                match &it.action {
                    Action::ChoisirSujet(s) => {
                        let description = match (it.groupe, s.genre) {
                            ("SÉLECTION", GenreSujet::Plusieurs) => format!("{} choisis dans le dossier ouvert. Entrée : choisir la commande.", s.libelle()),
                            ("SÉLECTION", _) => "Sélectionné dans le dossier ouvert. Entrée : choisir la commande.".to_string(),
                            ("DOSSIER OUVERT", _) => "Aucun élément choisi : le dossier ouvert. Entrée : choisir la commande.".to_string(),
                            _ => it.detail.clone(),
                        };
                        let art = if s.genre == GenreSujet::Dossier { Art::Contenu(Box::new(self.contenu_de(&s.chemins[0], &[], String::new()))) } else { Art::Aucun };
                        Some(ApercuPalette { titre: it.libelle.clone(), description, art })
                    }
                    Action::Aller(Emplacement::Dossier(d)) => Some(ApercuPalette { titre: it.libelle.clone(), description: d.clone(), art: Art::Contenu(Box::new(self.contenu_de(d, &[], String::new()))) }),
                    _ => Some(apercu_de(it)),
                }
            }
            Etape::Commande => {
                let it = self.items.get(self.choix)?;
                let Action::ChoisirCommande(c) = it.action else { return None };
                let d = assistant::def(c);
                let suite = match (it.grise, d.param) {
                    (true, _) => format!("Ne s'applique pas ici : {}.", it.detail.to_lowercase()),
                    (_, Param::Aucun) => "S'applique tout de suite.".to_string(),
                    (_, Param::Destination) => "Vous choisirez la destination à l'étape suivante.".to_string(),
                    (_, Param::Nom) => "Vous taperez le nom à l'étape suivante.".to_string(),
                    (_, Param::Confirmation) => "Une confirmation sera demandée. Annuler reste possible quand l'élément va à la corbeille (pas sur un disque réseau ou amovible).".to_string(),
                };
                let art = match c {
                    CommandeEl::CopierVers | CommandeEl::DeplacerVers => Art::Copier,
                    CommandeEl::Corbeille => Art::Supprimer,
                    CommandeEl::NouveauDossier => Art::NouveauDossier,
                    _ => Art::Aucun,
                };
                Some(ApercuPalette { titre: d.libelle.to_string(), description: format!("Agit sur {nom}. {suite}"), art })
            }
            Etape::Parametre => {
                let c = self.commande?;
                let sujet = sujet?;
                match assistant::def(c).param {
                    Param::Destination => {
                        let it = self.items.get(self.choix)?;
                        let (Action::EntrerDans(d) | Action::ValiderIci(d)) = &it.action else {
                            return Some(ApercuPalette { titre: it.libelle.clone(), description: it.detail.clone(), art: Art::Aucun });
                        };
                        let noms = sujet.noms();
                        let contenu = self.contenu_de(d, &noms, assistant::phrase_effet(c, sujet, d));
                        let l = self.lecture(d);
                        let description = match l {
                            Some(l) if l.lu => format!("{}{} élément{}", l.libre.map_or(String::new(), |o| format!("{} libres · ", crate::systeme::taille(o))), crate::systeme::nombre(l.total), if l.total > 1 { "s" } else { "" }),
                            Some(_) => "Non lu (partage réseau ou disque amovible)".to_string(),
                            None => "Lecture…".to_string(),
                        };
                        Some(ApercuPalette { titre: assistant::nom_de(d).to_uppercase(), description, art: Art::Contenu(Box::new(contenu)) })
                    }
                    Param::Nom => {
                        let etat = self.etat_du_nom();
                        let saisi = self.requete();
                        let (titre, avant) = if c == CommandeEl::Renommer { ("RENOMMER", sujet.libelle()) } else { ("NOUVEAU DOSSIER", format!("dans {}", sujet.libelle())) };
                        let description = etat.raison().map_or("Le nom est libre : Entrée pour valider.".to_string(), |r| r.to_lowercase());
                        Some(ApercuPalette { titre: titre.to_string(), description, art: Art::Nom { avant, apres: saisi.trim().to_string(), raison: etat.raison() } })
                    }
                    _ => Some(ApercuPalette { titre: assistant::def(c).libelle.to_string(), description: format!("Agit sur {nom}."), art: Art::Phrase { texte: assistant::phrase_effet(c, sujet, ""), avertissement: false } }),
                }
            }
            Etape::Progression => {
                let verbe = self.attente.as_ref().map_or("OPÉRATION EN COURS…".to_string(), |a| a.verbe.to_uppercase());
                Some(ApercuPalette { titre: verbe, description: "Un conflit de nom s'affiche par-dessus : Remplacer, Garder les deux ou Ignorer.".to_string(), art: Art::Progression(self.progression) })
            }
            Etape::Resultat => {
                let r = self.resultat.as_ref()?;
                let titre = if r.erreur { "✗ ÉCHEC" } else { "✓ FAIT" };
                Some(ApercuPalette { titre: titre.to_string(), description: r.message.clone(), art: Art::Phrase { texte: if r.erreur { r.message.clone() } else { format!("{nom} · {}", r.message) }, avertissement: r.erreur } })
            }
        }
    }

    /// Contenu d'un dossier pour l'aperçu, avec ce qui va y arriver (`noms`) et la phrase d'effet. Seulement ce que le fil de fond a lu.
    fn contenu_de(&self, chemin: &str, noms: &[String], effet: String) -> Contenu {
        let l = self.lecture(chemin);
        let lu = l.is_some_and(|l| l.lu);
        let conflits = l.filter(|l| l.lu).map(|l| assistant::conflits_de(noms, &l.noms)).unwrap_or_default();
        let conflit = !conflits.is_empty();
        let effet = if conflit {
            "Un élément du même nom existe ici : vous choisirez Remplacer, Garder les deux ou Ignorer.".to_string()
        } else if !effet.is_empty() && l.is_some_and(|l| !l.lu) {
            format!("{effet}. Dossier non lu : les noms déjà pris ne sont pas vérifiés.")
        } else {
            effet
        };
        Contenu {
            elements: l.map(|l| l.apercu.iter().take(5).cloned().collect()).unwrap_or_default(),
            nouveaux: noms.iter().map(|n| (n.clone(), conflits.iter().any(|c| c == n))).collect(),
            lu,
            en_cours: l.is_none(),
            effet,
            conflit,
        }
    }

    /// La souris passe sur la ligne `i` : elle devient la ligne choisie (`p.onmousemove` de la maquette). Vrai si le choix change ;
    /// rien tant qu'une commande vient d'être lancée ou si la ligne n'existe pas.
    pub fn survoler(&mut self, i: usize) -> bool {
        if self.lance || i >= self.items.len() || self.choix == i {
            return false;
        }
        self.choix = i;
        true
    }

    pub fn deplacer(&mut self, d: i64) {
        if self.items.is_empty() {
            return;
        }
        let n = self.items.len() as i64;
        self.choix = (self.choix as i64 + d).clamp(0, n - 1) as usize;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    fn sources_vides() -> Sources<'static> {
        Sources { epingles: &[], recents: &[], recents_elements: Vec::new(), fichiers: Vec::new(), groupes: &[], sessions: &[], selection: Vec::new(), dossier_ouvert: None, volet: Vec::new(), autre_panneau: None }
    }

    /// Un dossier ouvert avec trois éléments, dont rapport.pdf choisi
    fn avec_selection() -> Sources<'static> {
        Sources {
            fichiers: vec![
                (r"C:\Docs\Factures".into(), true, Icone::Dossier),
                (r"C:\Docs\rapport.pdf".into(), false, Icone::Pdf),
                (r"C:\Docs\notes.txt".into(), false, Icone::Texte),
            ],
            selection: vec![(r"C:\Docs\rapport.pdf".into(), false, Icone::Pdf)],
            dossier_ouvert: Some(r"C:\Docs".into()),
            ..sources_vides()
        }
    }

    /// Palette ouverte sur la sélection, puis choisie jusqu'à la commande donnée
    fn jusqu_a(c: CommandeEl, s: &Sources) -> Palette {
        let mut p = Palette::nouvelle();
        p.remplir(s);
        assert_eq!(p.entrer(false), Suite::Rien);
        p.remplir(s);
        let k = p.items.iter().position(|i| matches!(i.action, Action::ChoisirCommande(x) if x == c)).expect("commande proposée");
        p.choix = k;
        p
    }

    /// Réponse du fil de fond à une lecture que la palette avait demandée (`a_demander` pose l'entrée `None`) : sans demande, elle est jetée
    fn servir(p: &mut Palette, l: Lecture) {
        p.lectures.entry(cle_lecture(&l.chemin)).or_insert(None);
        assert!(p.recevoir_lecture(l));
    }

    fn lecture(chemin: &str, noms: &[&str], sous: &[&str]) -> Lecture {
        Lecture {
            chemin: chemin.into(),
            lu: true,
            apercu: sous.iter().map(|n| (n.to_string(), String::new(), true)).chain(noms.iter().map(|n| (n.to_string(), "1,0 KO".to_string(), false))).collect(),
            sous_dossiers: sous.iter().map(|s| s.to_string()).collect(),
            noms: noms.iter().chain(sous.iter()).map(|n| n.to_lowercase()).collect::<HashSet<_>>(),
            total: noms.len() + sous.len(),
            libre: Some(181 << 30),
        }
    }

    #[test]
    fn commandes_experimentales_filtrees() {
        use crate::reglages::Experimentale;
        let gelees = [Commande::SauvegardeDestination, Commande::SauvegardeAjouter, Commande::SauvegardeVider, Commande::SauvegardeLancer, Commande::ListerTelephones];
        for c in gelees {
            assert!(!commande_permise(false, c), "{c:?} doit être fermée réglage faux");
            assert!(commande_permise(true, c), "{c:?} doit rester ouverte réglage vrai");
        }
        assert_eq!(commande_experimentale(Commande::SauvegardeLancer), Some(Experimentale::Sauvegarde));
        assert_eq!(commande_experimentale(Commande::ListerTelephones), Some(Experimentale::Mtp));
        // Comparer / synchroniser deux panneaux et l'OCR ne sont pas gelés
        for c in [Commande::Synchroniser, Commande::Comparer, Commande::LireTexteImage, Commande::Doublons, Commande::Reglages] {
            assert!(commande_permise(false, c), "{c:?}");
        }
    }

    /// Lot 16 : Ctrl+1/2/3 restent les balises de couleur et Ctrl+Maj+1/2/3 les vues ; les étiquettes (carrés) ont Alt+1/2/3, chacun une fois
    #[test]
    fn etiquettes_sans_conflit_de_touches() {
        let de = |c: Commande| COMMANDES.iter().find(|x| x.0 == c).map(|x| x.2).unwrap();
        assert_eq!(de(Commande::Etiquette1), "ALT+1");
        assert_eq!(de(Commande::Etiquette2), "ALT+2");
        assert_eq!(de(Commande::Etiquette3), "ALT+3");
        assert_eq!(de(Commande::BaliseRouge), "CTRL+1");
        assert_eq!(de(Commande::VueGrille), "CTRL+MAJ+2");
        for t in ["ALT+1", "ALT+2", "ALT+3"] {
            assert_eq!(COMMANDES.iter().filter(|x| x.2 == t).count(), 1, "{t}");
        }
    }

    /// Ce que la palette montre : réglage faux, aucune commande gelée ; réglage vrai, la table d'avant le gel
    #[test]
    fn table_de_la_palette_filtree() {
        let visibles = |exp: bool| COMMANDES.iter().filter(|(c, ..)| commande_permise(exp, *c)).count();
        assert_eq!(visibles(true), COMMANDES.len());
        assert_eq!(visibles(false), COMMANDES.len() - 5);
        assert!(COMMANDES.iter().filter(|(c, ..)| commande_permise(false, *c)).all(|(c, ..)| commande_experimentale(*c).is_none()));
    }

    /// Sessions et commandes générales sont toujours proposées, sous « Partout »
    #[test]
    fn palette_normale_garde_sessions_et_commandes() {
        let sessions = vec![r"TRAVAIL|C:\a|C:\b".to_string()];
        let sources = Sources { sessions: &sessions, ..sources_vides() };
        let mut p = Palette::nouvelle();
        p.saisie = Edition::champ("tr");
        p.remplir(&sources);
        assert!(p.items.iter().any(|i| matches!(&i.action, Action::OuvrirSession(n) if n == "TRAVAIL")));
        let mut p = Palette::nouvelle();
        p.saisie = Edition::champ("do");
        p.remplir(&sources);
        assert!(p.items.iter().any(|i| matches!(i.action, Action::Commande(Commande::Doublons))));
    }

    #[test]
    fn survol_d_une_ligne_la_choisit_comme_la_maquette() {
        let mut p = Palette::nouvelle();
        p.saisie = Edition::champ("do");
        p.remplir(&sources_vides());
        assert!(p.items.len() >= 2, "il faut au moins deux lignes pour le test");
        p.choix = 0;
        // Une autre ligne : le choix change et le dit ; la même : rien ne change
        assert!(p.survoler(1));
        assert_eq!(p.choix, 1);
        assert!(!p.survoler(1));
        // Une ligne qui n'existe pas ne déplace rien
        assert!(!p.survoler(p.items.len()));
        assert_eq!(p.choix, 1);
        // Commande lancée : la palette n'écoute plus la souris
        p.lance = true;
        assert!(!p.survoler(0));
        assert_eq!(p.choix, 1);
    }

    #[test]
    fn invite_selon_l_etape() {
        assert_eq!(Palette::nouvelle().invite(), "UN FICHIER, UN DOSSIER OU UNE COMMANDE… (3 LETTRES : TOUT LE PC)");
        let s = avec_selection();
        let mut p = jusqu_a(CommandeEl::CopierVers, &s);
        assert_eq!(p.invite(), "COMMANDE POUR RAPPORT.PDF");
        assert_eq!(p.entrer(false), Suite::Rien);
        assert!(p.invite().starts_with("DOSSIER DE DESTINATION"));
    }

    #[test]
    fn scores() {
        assert!(score("TERMINAL · JOURNAL ET OUVERTURE EXTERNE", "term").unwrap() > score("TERMINAL · JOURNAL ET OUVERTURE EXTERNE", "otl").unwrap_or(-9999));
        assert!(score("COMPRESSER EN ZIP", "czip").is_some());
        assert!(score("COMPRESSER EN ZIP", "xyz").is_none());
        assert!(score("terminal", "TER").unwrap() > score("un terminal", "ter").unwrap());
    }

    // ── Étape 1 : l'élément ──

    #[test]
    fn ouverture_sur_la_selection_puis_le_dossier_puis_partout() {
        let s = avec_selection();
        let mut p = Palette::nouvelle();
        p.remplir(&s);
        assert_eq!(p.etape, Etape::Element);
        // La sélection en tête, préchoisie
        assert_eq!((p.choix, p.items[0].groupe, p.items[0].libelle.as_str()), (0, "SÉLECTION", "rapport.pdf"));
        assert!(matches!(&p.items[0].action, Action::ChoisirSujet(su) if su.chemins == [r"C:\Docs\rapport.pdf"] && su.genre == GenreSujet::Fichier));
        // Puis les éléments du dossier (sans doublonner la sélection), puis « Partout »
        let groupes: Vec<&str> = p.lignes().iter().filter_map(|l| if let LignePal::Titre(t) = l { Some(*t) } else { None }).collect();
        assert_eq!(&groupes[..3], ["SÉLECTION", "DANS CE DOSSIER", "PARTOUT"]);
        let dedans: Vec<&str> = p.items.iter().filter(|i| i.groupe == "DANS CE DOSSIER").map(|i| i.libelle.as_str()).collect();
        assert_eq!(dedans, ["Factures", "notes.txt"]);
        // Les commandes générales sont là ; celles qui agissent sur un élément n'y sont plus
        assert!(p.items.iter().any(|i| matches!(i.action, Action::Commande(Commande::VueGrille)) && i.groupe == "PARTOUT"));
        for c in [Commande::Copier, Commande::Supprimer, Commande::Renommer, Commande::Zip, Commande::Ouvrir, Commande::CopierVersAutre] {
            assert!(!p.items.iter().any(|i| matches!(i.action, Action::Commande(x) if x == c)), "{c:?} appartient à l'étape Commande");
            assert!(commande_pour_element(c));
        }
        assert!(!commande_pour_element(Commande::VueGrille) && !commande_pour_element(Commande::Reglages));
    }

    #[test]
    fn plusieurs_elements_puis_aucun_element() {
        let mut s = avec_selection();
        s.selection = vec![(r"C:\Docs\Factures".into(), true, Icone::Dossier), (r"C:\Docs\rapport.pdf".into(), false, Icone::Pdf), (r"C:\Docs\notes.txt".into(), false, Icone::Texte)];
        let mut p = Palette::nouvelle();
        p.remplir(&s);
        assert_eq!(p.items[0].libelle, "3 éléments");
        assert!(matches!(&p.items[0].action, Action::ChoisirSujet(su) if su.genre == GenreSujet::Plusieurs && su.chemins.len() == 3));
        // Aucune sélection : le dossier ouvert
        s.selection.clear();
        let mut q = Palette::nouvelle();
        q.remplir(&s);
        assert_eq!((q.items[0].groupe, q.items[0].libelle.as_str()), ("DOSSIER OUVERT", "Docs"));
        assert!(matches!(&q.items[0].action, Action::ChoisirSujet(su) if su.ouvert));
        // Pas de dossier du tout (Ce PC) et pas de sélection : « Partout » seulement
        s.dossier_ouvert = None;
        s.fichiers.clear();
        let mut r = Palette::nouvelle();
        r.remplir(&s);
        assert!(r.items.iter().all(|i| i.groupe != "SÉLECTION" && i.groupe != "DOSSIER OUVERT"));
    }

    #[test]
    fn taper_filtre_les_elements_du_dossier_et_les_commandes_de_partout() {
        let s = avec_selection();
        let mut p = Palette::nouvelle();
        p.saisie = Edition::champ("not");
        p.remplir(&s);
        assert!(p.items.iter().any(|i| i.libelle == "notes.txt" && i.groupe == "DANS CE DOSSIER"));
        assert!(!p.items.iter().any(|i| i.libelle == "rapport.pdf"), "la sélection est filtrée comme le reste");
        let mut v = Palette::nouvelle();
        v.saisie = Edition::champ("vue g");
        v.remplir(&s);
        assert!(v.items.iter().any(|i| matches!(i.action, Action::Commande(Commande::VueGrille))));
    }

    #[test]
    fn resultats_de_l_index_apres_le_dossier_et_avant_partout() {
        let s = avec_selection();
        let mut p = Palette::nouvelle();
        p.saisie = Edition::champ("contrat");
        // Pas de résultats tant que l'index n'a pas répondu
        p.remplir(&s);
        assert!(p.items.iter().all(|i| i.groupe != "RÉSULTATS"));
        let t = vec![Trouve { chemin: r"D:\Perso\contrat.pdf".into(), dossier: false, icone: Icone::Pdf }, Trouve { chemin: r"D:\Perso\Contrats".into(), dossier: true, icone: Icone::Dossier }];
        // Une réponse pour une autre requête est ignorée (on a continué à taper)
        assert!(!p.recevoir_recherche("contr", t.clone()));
        assert!(p.recevoir_recherche("contrat", t));
        p.remplir(&s);
        let rang = |g: &str| p.items.iter().position(|i| i.groupe == g);
        assert!(rang("RÉSULTATS").unwrap() < rang("PARTOUT").unwrap_or(usize::MAX));
        assert!(matches!(&p.items[rang("RÉSULTATS").unwrap()].action, Action::ChoisirSujet(su) if su.chemins[0] == r"D:\Perso\contrat.pdf"));
    }

    #[test]
    fn la_recherche_et_les_lectures_ne_sont_demandees_qu_une_fois() {
        let s = avec_selection();
        let mut p = Palette::nouvelle();
        p.saisie = Edition::champ("co");
        p.remplir(&s);
        assert_eq!(p.a_demander().0, None, "moins de trois lettres : pas d'index");
        p.saisie = Edition::champ("cont");
        p.remplir(&s);
        assert_eq!(p.a_demander().0.as_deref(), Some("cont"));
        assert_eq!(p.a_demander().0, None, "la même requête n'est pas relancée");
        p.saisie = Edition::champ("conti");
        assert_eq!(p.a_demander().0.as_deref(), Some("conti"));
        // Le dossier survolé est lu une fois (aperçu de ses premiers éléments)
        let mut q = Palette::nouvelle();
        q.saisie = Edition::champ("fact");
        q.remplir(&s);
        let (_, lire) = q.a_demander();
        assert_eq!(lire, [r"C:\Docs\Factures"]);
        assert!(q.a_demander().1.is_empty());
        assert!(q.lecture(r"C:\Docs\Factures").is_none(), "demandée, pas encore revenue");
        servir(&mut q, lecture(r"C:\Docs\Factures", &["a.pdf"], &[]));
        assert!(q.lecture(r"c:\docs\factures\").is_some(), "clé sans casse ni barre finale");
    }

    // ── Étape 2 : la commande ──

    #[test]
    fn entree_sur_l_element_ouvre_les_commandes_de_cet_element() {
        let s = avec_selection();
        let mut p = Palette::nouvelle();
        p.saisie = Edition::champ("rap");
        p.remplir(&s);
        assert_eq!(p.entrer(false), Suite::Rien);
        assert_eq!((p.etape, p.saisie.valeur()), (Etape::Commande, String::new()), "le champ est vidé pour chercher une commande");
        assert_eq!(p.sujet.as_ref().map(|x| x.libelle()).as_deref(), Some("rapport.pdf"));
        p.remplir(&s);
        let noms: Vec<&str> = p.items.iter().filter(|i| !i.grise).map(|i| i.libelle.as_str()).collect();
        assert_eq!(&noms[..3], ["OUVRIR", "COPIER VERS…", "DÉPLACER VERS…"]);
        // Un fichier : « nouveau dossier dedans » et « onglet » sont grisés avec leur raison, après les autres
        let gris: Vec<(&str, &str)> = p.items.iter().filter(|i| i.grise).map(|i| (i.libelle.as_str(), i.detail.as_str())).collect();
        assert!(gris.contains(&("NOUVEAU DOSSIER DEDANS…", "UN DOSSIER SEULEMENT")), "{gris:?}");
        assert!(p.items.iter().rposition(|i| !i.grise) < p.items.iter().position(|i| i.grise));
        // Chaque ligne dit ce qu'elle demande, et son raccourci
        let copier = p.items.iter().find(|i| i.libelle == "COPIER VERS…").unwrap();
        assert_eq!(copier.detail, "DESTINATION ›");
        let renommer = p.items.iter().find(|i| i.libelle == "RENOMMER…").unwrap();
        assert_eq!((renommer.detail.as_str(), renommer.raccourci), ("NOM ›", "F2"));
        // Entrée sur une commande grisée ne fait rien
        p.choix = p.items.iter().position(|i| i.grise).unwrap();
        assert_eq!((p.entrer(false), p.etape), (Suite::Rien, Etape::Commande));
    }

    #[test]
    fn plusieurs_elements_grisent_renommer_et_gardent_copier() {
        let mut s = avec_selection();
        s.selection = vec![(r"C:\Docs\rapport.pdf".into(), false, Icone::Pdf), (r"C:\Docs\notes.txt".into(), false, Icone::Texte)];
        let p = jusqu_a(CommandeEl::CopierVers, &s);
        let it = |l: &str| p.items.iter().find(|i| i.libelle == l).unwrap();
        assert!(it("RENOMMER…").grise && it("RENOMMER…").detail == "UN SEUL ÉLÉMENT");
        assert!(!it("COPIER VERS…").grise && !it("METTRE À LA CORBEILLE").grise && !it("COMPRESSER VERS… (ZIP)").grise);
        let a = p.apercu().unwrap();
        assert!(a.description.starts_with("Agit sur 2 éléments."), "{}", a.description);
    }

    #[test]
    fn apercu_de_la_commande_dit_sur_quoi_elle_agit() {
        let s = avec_selection();
        let mut p = jusqu_a(CommandeEl::CopierVers, &s);
        let a = p.apercu().unwrap();
        assert_eq!(a.titre, "COPIER VERS…");
        assert_eq!(a.description, "Agit sur rapport.pdf. Vous choisirez la destination à l'étape suivante.");
        p.choix = p.items.iter().position(|i| matches!(i.action, Action::ChoisirCommande(CommandeEl::Ouvrir))).unwrap();
        assert!(p.apercu().unwrap().description.ends_with("S'applique tout de suite."));
        p.choix = p.items.iter().position(|i| matches!(i.action, Action::ChoisirCommande(CommandeEl::Corbeille))).unwrap();
        assert_eq!(p.apercu().unwrap().art, Art::Supprimer);
    }

    #[test]
    fn commande_sans_parametre_part_tout_de_suite() {
        let s = avec_selection();
        let mut p = jusqu_a(CommandeEl::CopierChemin, &s);
        match p.entrer(false) {
            Suite::Executer(e) => assert_eq!((e.commande, e.sujet.libelle(), e.dossier, e.nom), (CommandeEl::CopierChemin, "rapport.pdf".to_string(), None, None)),
            autre => panic!("{autre:?}"),
        }
    }

    // ── Retour arrière, puces du fil d'Ariane ──

    #[test]
    fn retour_arriere_remonte_d_une_etape_et_rouvre_la_recherche() {
        let s = avec_selection();
        let mut p = jusqu_a(CommandeEl::CopierVers, &s);
        assert_eq!(p.entrer(false), Suite::Rien);
        assert_eq!(p.etape, Etape::Parametre);
        assert!(p.reculer());
        assert_eq!((p.etape, p.commande), (Etape::Commande, None));
        assert!(p.sujet.is_some(), "l'élément reste choisi");
        // Sur la puce « élément » : un autre élément se cherche
        assert!(p.reculer());
        assert_eq!((p.etape, p.sujet.is_none()), (Etape::Element, true));
        // Au début, plus rien à remonter
        assert!(!p.reculer());
        assert_eq!(p.etape, Etape::Element);
    }

    #[test]
    fn clic_sur_une_puce_revient_a_son_etape() {
        let s = avec_selection();
        let mut p = jusqu_a(CommandeEl::CopierVers, &s);
        assert_eq!(p.entrer(false), Suite::Rien);
        // Les puces : élément, « Copier vers », « Destination » (courante)
        assert_eq!(p.fil(), [("rapport.pdf".to_string(), 0, false), ("Copier vers".to_string(), 1, false), ("Destination".to_string(), 2, true)]);
        assert!(!p.aller_etape(2), "la puce courante ne bouge rien");
        assert!(p.aller_etape(1));
        assert_eq!((p.etape, p.commande, p.sujet.is_some()), (Etape::Commande, None, true));
        assert_eq!(p.fil(), [("rapport.pdf".to_string(), 0, false), ("Commande".to_string(), 1, true)]);
        assert!(p.aller_etape(0));
        assert_eq!((p.etape, p.sujet.is_none()), (Etape::Element, true));
        assert_eq!(p.fil(), [("Élément".to_string(), 0, true)]);
    }

    // ── Étape 3 : destination ──

    fn sources_destination() -> (Vec<String>, Vec<String>, Sources<'static>) {
        let recents = vec![r"C:\Users\moi\Documents".to_string()];
        let epingles = vec![r"C:\Users\moi\Bureau".to_string()];
        (recents, epingles, Sources { volet: vec![("Téléchargements".into(), r"C:\Users\moi\Downloads".into())], autre_panneau: Some(r"D:\Archives".into()), ..avec_selection() })
    }

    #[test]
    fn destinations_autre_panneau_recents_favoris_puis_ici_et_parent() {
        let (recents, epingles, s) = sources_destination();
        let s = Sources { recents: Box::leak(recents.into_boxed_slice()), epingles: Box::leak(epingles.into_boxed_slice()), ..s };
        let mut p = jusqu_a(CommandeEl::CopierVers, &s);
        assert_eq!(p.entrer(false), Suite::Rien);
        p.remplir(&s);
        let ordre: Vec<&str> = p.lignes().iter().filter_map(|l| if let LignePal::Titre(t) = l { Some(*t) } else { None }).collect();
        assert_eq!(ordre, ["AUTRE PANNEAU", "RÉCENTS", "FAVORIS", "ICI"]);
        assert!(matches!(&p.items[0].action, Action::EntrerDans(d) if d == r"D:\Archives"));
        let ici: Vec<&str> = p.items.iter().filter(|i| i.groupe == "ICI").map(|i| i.detail.as_str()).collect();
        assert_eq!(ici, [r"C:\Docs", r"C:\"], "dossier courant puis parent");
    }

    #[test]
    fn entree_entre_dans_le_dossier_et_ctrl_entree_valide_ici() {
        let (_, _, s) = sources_destination();
        let mut p = jusqu_a(CommandeEl::DeplacerVers, &s);
        p.entrer(false);
        p.remplir(&s);
        // Entrée : on entre dans le dossier visé, rien ne part
        p.choix = 0;
        assert_eq!(p.entrer(false), Suite::Rien);
        assert_eq!((p.etape, p.dossier_nav.as_deref()), (Etape::Parametre, Some(r"D:\Archives")));
        servir(&mut p, lecture(r"D:\Archives", &["contrat.pdf"], &["2025", "2024"]));
        p.remplir(&s);
        // « Ici » en tête, le parent, puis les sous-dossiers lus
        assert!(matches!(&p.items[0].action, Action::ValiderIci(d) if d == r"D:\Archives"));
        assert_eq!(p.items[0].libelle, "DÉPLACER ICI · ARCHIVES");
        assert!(matches!(&p.items[1].action, Action::EntrerDans(d) if d == r"D:\"));
        let sous: Vec<&str> = p.items.iter().filter(|i| i.groupe == "SOUS-DOSSIERS").map(|i| i.libelle.as_str()).collect();
        assert_eq!(sous, ["2025", "2024"]);
        // Ctrl+Entrée sur un sous-dossier le valide sans y entrer
        p.choix = p.items.iter().position(|i| i.libelle == "2025").unwrap();
        match p.entrer(true) {
            Suite::Executer(e) => assert_eq!((e.commande, e.dossier.as_deref()), (CommandeEl::DeplacerVers, Some(r"D:\Archives\2025"))),
            autre => panic!("{autre:?}"),
        }
        assert_eq!(p.destination.as_deref(), Some("2025"));
        // Entrer dans un sous-dossier, puis « ici »
        p.destination = None;
        p.entrer(false);
        assert_eq!(p.dossier_nav.as_deref(), Some(r"D:\Archives\2025"));
        p.remplir(&s);
        p.choix = 0;
        assert!(matches!(p.entrer(false), Suite::Executer(e) if e.dossier.as_deref() == Some(r"D:\Archives\2025")));
    }

    #[test]
    fn navigation_dans_un_dossier_non_lu_ou_en_cours_de_lecture() {
        let (_, _, s) = sources_destination();
        let mut p = jusqu_a(CommandeEl::CopierVers, &s);
        p.entrer(false);
        p.remplir(&s);
        p.choix = 0;
        p.entrer(false);
        p.remplir(&s);
        assert!(p.items.iter().any(|i| i.libelle == "LECTURE DU DOSSIER…" && i.grise), "la lecture n'est pas revenue");
        assert!(matches!(p.items[0].action, Action::ValiderIci(_)), "on peut valider ici sans attendre");
        servir(&mut p, Lecture::non_lue(r"D:\Archives"));
        p.remplir(&s);
        assert!(p.items.iter().any(|i| i.libelle == "SOUS-DOSSIERS NON LUS" && i.grise), "partage réseau : non lu, pas d'attente");
        assert!(p.items[p.choix].action_est_ici());
    }

    impl Item {
        fn action_est_ici(&self) -> bool {
            matches!(self.action, Action::ValiderIci(_))
        }
    }

    #[test]
    fn apercu_destination_avec_place_libre_et_phrase_d_effet() {
        let (_, _, s) = sources_destination();
        let mut p = jusqu_a(CommandeEl::CopierVers, &s);
        p.entrer(false);
        p.remplir(&s);
        p.choix = 0;
        // Avant la lecture : « Lecture… », pas de phrase inventée sur les conflits
        let a = p.apercu().unwrap();
        assert_eq!((a.titre.as_str(), a.description.as_str()), ("ARCHIVES", "Lecture…"));
        servir(&mut p, lecture(r"D:\Archives", &["contrat.pdf", "z.txt"], &["2025"]));
        let a = p.apercu().unwrap();
        assert_eq!(a.description, "181 GO libres · 3 éléments");
        let Art::Contenu(c) = a.art else { panic!("{:?}", a.art) };
        assert!(c.lu && !c.conflit && !c.en_cours);
        assert_eq!(c.elements.iter().map(|e| e.0.as_str()).collect::<Vec<_>>(), ["2025", "contrat.pdf", "z.txt"]);
        assert_eq!(c.nouveaux, [("rapport.pdf".to_string(), false)]);
        assert_eq!(c.effet, "rapport.pdf sera copié dans Archives");
    }

    #[test]
    fn conflit_de_nom_avertit_et_annonce_la_carte_remplacer_garder_ignorer() {
        let (_, _, s) = sources_destination();
        let mut p = jusqu_a(CommandeEl::CopierVers, &s);
        p.entrer(false);
        p.remplir(&s);
        p.choix = 0;
        servir(&mut p, lecture(r"D:\Archives", &["Rapport.PDF"], &[]));
        let a = p.apercu().unwrap();
        let Art::Contenu(c) = a.art else { panic!() };
        assert!(c.conflit, "même nom sans tenir compte de la casse");
        assert_eq!(c.nouveaux, [("rapport.pdf".to_string(), true)]);
        assert!(c.effet.contains("Remplacer") && c.effet.contains("Garder les deux") && c.effet.contains("Ignorer"));
    }

    #[test]
    fn destination_non_lue_le_dit_sans_pretendre_verifier() {
        let (_, _, s) = sources_destination();
        let mut p = jusqu_a(CommandeEl::CopierVers, &s);
        p.entrer(false);
        p.remplir(&s);
        p.choix = 0;
        servir(&mut p, Lecture::non_lue(r"D:\Archives"));
        let a = p.apercu().unwrap();
        assert_eq!(a.description, "Non lu (partage réseau ou disque amovible)");
        let Art::Contenu(c) = a.art else { panic!() };
        assert!(!c.lu && !c.conflit && c.elements.is_empty());
        assert!(c.effet.contains("non lu") && c.effet.starts_with("rapport.pdf sera copié"));
    }

    // ── Étape 3 : nom, confirmation ──

    #[test]
    fn renommer_prerempli_avec_conflit_en_direct() {
        let s = avec_selection();
        let mut p = jusqu_a(CommandeEl::Renommer, &s);
        assert_eq!(p.entrer(false), Suite::Rien);
        assert_eq!((p.etape, p.requete().as_str()), (Etape::Parametre, "rapport.pdf"), "le nom d'avant est dans le champ");
        assert_eq!(p.saisie.selection(), (0, 7), "son radical est choisi, pas l'extension");
        p.remplir(&s);
        assert!(p.items[0].grise && p.items[0].detail == "C'EST DÉJÀ SON NOM");
        // Le dossier est lu : un nom pris est refusé, un nom libre accepté
        let (_, lire) = p.a_demander();
        assert_eq!(lire, [r"C:\Docs"]);
        servir(&mut p, lecture(r"C:\Docs", &["rapport.pdf", "notes.txt"], &[]));
        p.saisie = Edition::champ("Notes.TXT");
        p.remplir(&s);
        assert!(p.items[0].grise && p.items[0].detail == "UN ÉLÉMENT PORTE DÉJÀ CE NOM");
        assert_eq!(p.entrer(false), Suite::Rien, "un nom pris ne part pas");
        let a = p.apercu().unwrap();
        assert_eq!(a.art, Art::Nom { avant: "rapport.pdf".into(), apres: "Notes.TXT".into(), raison: Some("UN ÉLÉMENT PORTE DÉJÀ CE NOM") });
        p.saisie = Edition::champ("rapport-v2.pdf");
        p.remplir(&s);
        assert_eq!((p.items[0].libelle.as_str(), p.items[0].grise), ("RENOMMER EN « rapport-v2.pdf »", false));
        match p.entrer(false) {
            Suite::Executer(e) => assert_eq!((e.commande, e.nom.as_deref()), (CommandeEl::Renommer, Some("rapport-v2.pdf"))),
            autre => panic!("{autre:?}"),
        }
        // Nom invalide et vide
        p.saisie = Edition::champ("a/b");
        p.remplir(&s);
        assert!(p.items[0].grise && p.items[0].detail.starts_with("NOM INVALIDE"));
        p.saisie = Edition::champ("");
        p.remplir(&s);
        assert!(p.items[0].grise && p.items[0].detail == "TAPEZ UN NOM");
    }

    #[test]
    fn nouveau_dossier_dedans_verifie_le_nom_dans_le_dossier_lui_meme() {
        let mut s = avec_selection();
        s.selection = vec![(r"C:\Docs\Factures".into(), true, Icone::Dossier)];
        let mut p = jusqu_a(CommandeEl::NouveauDossier, &s);
        p.entrer(false);
        assert_eq!(p.requete(), "", "pas de nom proposé");
        assert_eq!(p.dossier_du_nom().as_deref(), Some(r"C:\Docs\Factures"));
        servir(&mut p, lecture(r"C:\Docs\Factures", &[], &["2025"]));
        p.saisie = Edition::champ("2025");
        p.remplir(&s);
        assert!(p.items[0].grise && p.items[0].detail == "UN ÉLÉMENT PORTE DÉJÀ CE NOM");
        p.saisie = Edition::champ("2026");
        p.remplir(&s);
        assert!(matches!(&p.items[0].action, Action::ValiderNom(n) if n == "2026"));
        assert_eq!(p.items[0].libelle, "CRÉER « 2026 »");
    }

    #[test]
    fn nom_sur_dossier_non_lu_laisse_l_operation_trancher() {
        let s = avec_selection();
        let mut p = jusqu_a(CommandeEl::Renommer, &s);
        p.entrer(false);
        p.saisie = Edition::champ("autre.pdf");
        servir(&mut p, Lecture::non_lue(r"C:\Docs"));
        p.remplir(&s);
        assert!(!p.items[0].grise && p.items[0].detail.contains("NON LU"));
    }

    #[test]
    fn corbeille_demande_une_confirmation_claire() {
        let s = avec_selection();
        let mut p = jusqu_a(CommandeEl::Corbeille, &s);
        assert_eq!(p.entrer(false), Suite::Rien);
        p.remplir(&s);
        assert_eq!(p.items[0].libelle, "METTRE À LA CORBEILLE · RAPPORT.PDF");
        assert_eq!(p.fil().last().unwrap().0, "Confirmation");
        let a = p.apercu().unwrap();
        assert_eq!(a.art, Art::Phrase { texte: "rapport.pdf ira à la corbeille. Annuler restera possible après.".into(), avertissement: false });
        // « Non, revenir » ramène aux commandes ; Entrée sur la première ligne part
        p.choix = 1;
        assert_eq!((p.entrer(false), p.etape), (Suite::Rien, Etape::Commande));
        let mut q = jusqu_a(CommandeEl::Corbeille, &s);
        q.entrer(false);
        q.remplir(&s);
        assert!(matches!(q.entrer(false), Suite::Executer(e) if e.commande == CommandeEl::Corbeille && e.sujet.libelle() == "rapport.pdf"));
    }

    // ── Progression, résultat, annulation ──

    #[test]
    fn progression_puis_resultat_avec_annuler_et_autre_action() {
        let (_, _, s) = sources_destination();
        let mut p = jusqu_a(CommandeEl::CopierVers, &s);
        p.entrer(false);
        p.remplir(&s);
        p.choix = 0;
        let Suite::Executer(_) = p.entrer(true) else { panic!("la destination se valide") };
        // Progression : la liste n'a qu'une ligne d'information, la saisie est fermée, l'aperçu montre la barre
        p.en_cours("Copie en cours…", false, false);
        p.progression = 0.4;
        p.remplir(&s);
        assert_eq!((p.etape, p.items.len(), p.items[0].grise, p.accepte_saisie()), (Etape::Progression, 1, true, false));
        assert_eq!(p.apercu().unwrap().art, Art::Progression(0.4));
        assert!(!p.reculer(), "pas de retour pendant l'opération");
        assert!(!p.aller_etape(0));
        assert_eq!(p.fil().iter().map(|x| x.0.as_str()).collect::<Vec<_>>(), ["rapport.pdf", "Copier vers", "Archives"], "les trois puces sont posées");
        // Fini : « Copié dans Archives », Annuler puis Autre action
        p.finir(Resultat::ok("Copié dans Archives", true));
        p.remplir(&s);
        assert_eq!(p.etape, Etape::Resultat);
        let l: Vec<&str> = p.items.iter().map(|i| i.libelle.as_str()).collect();
        assert_eq!(l, ["ANNULER", "AUTRE ACTION SUR RAPPORT.PDF"]);
        assert!(p.lectures.is_empty(), "les dossiers lus sont périmés après une copie");
        assert_eq!(p.apercu().unwrap().description, "Copié dans Archives");
        assert_eq!(p.choix, 1, "la ligne choisie d'office n'est pas « Annuler » : un double Entrée ne défait pas");
        p.choix = 0;
        assert_eq!(p.entrer(false), Suite::Defaire);
        // « Autre action » ramène aux commandes sur le même élément
        p.choix = 1;
        assert_eq!(p.entrer(false), Suite::Rien);
        assert_eq!((p.etape, p.sujet.as_ref().map(|x| x.libelle()).as_deref(), p.commande), (Etape::Commande, Some("rapport.pdf"), None));
    }

    #[test]
    fn resultat_en_erreur_ou_sans_annulation_n_a_que_autre_action() {
        let s = avec_selection();
        let mut p = jusqu_a(CommandeEl::CopierChemin, &s);
        p.finir(Resultat::erreur("DISQUE PLEIN"));
        p.remplir(&s);
        assert_eq!(p.items.len(), 1);
        assert!(matches!(p.items[0].action, Action::AutreAction));
        let a = p.apercu().unwrap();
        assert!(a.titre.contains("ÉCHEC") && matches!(a.art, Art::Phrase { avertissement: true, .. }));
        // Retour arrière sur le résultat : les commandes
        assert!(p.reculer());
        assert_eq!(p.etape, Etape::Commande);
    }

    #[test]
    fn modes_a_nom_gardent_leur_liste_simple() {
        let mut g = Palette::groupe(vec!["a".into()]);
        g.saisie = Edition::champ("rouge");
        g.remplir(&sources_vides());
        assert!(!g.est_assistant() && g.accepte_saisie());
        assert!(matches!(&g.items[0].action, Action::MettreDansGroupe(n) if n == "ROUGE"));
        assert_eq!(g.apercu().unwrap().titre, g.items[0].libelle);
    }

    // ── Liste, défilement, aperçu ──

    #[test]
    fn lignes_un_titre_a_chaque_changement_de_groupe() {
        let s = avec_selection();
        let mut p = Palette::nouvelle();
        p.remplir(&s);
        let l = p.lignes();
        assert_eq!(l[0], LignePal::Titre("SÉLECTION"));
        assert_eq!(l[1], LignePal::Ligne(0));
        assert_eq!(l[2], LignePal::Titre("DANS CE DOSSIER"));
        assert_eq!(l.iter().filter(|x| matches!(x, LignePal::Ligne(_))).count(), p.items.len());
        let mut q = Palette::nouvelle();
        q.remplir(&Sources { recents_elements: vec![(r"C:\Travail\contrat.pdf".into(), false, Icone::Pdf)], ..sources_vides() });
        assert_eq!(q.lignes()[0], LignePal::Titre("RÉCENTS"), "sans sélection, les éléments récents ouvrent la liste");
        assert!(matches!(&q.items[0].action, Action::ChoisirSujet(s) if s.chemins == [r"C:\Travail\contrat.pdf"]), "un élément réel, pas « reprendre »");
        assert!(!q.items[0].libelle.contains("REPRENDRE"));
    }

    #[test]
    fn surlignage_sous_chaine_accents_et_sous_sequence() {
        assert_eq!(plages_palette("VUE LISTE", "vue"), [(0, 3)]);
        assert_eq!(plages_palette("Éléments récents", "ele"), [(0, 3)], "accents et casse ignorés");
        assert_eq!(plages_palette("VUE LISTE", "li"), [(4, 6)]);
        // Les espaces de la requête sont ignorés : « v e » devient « ve », absent tel quel, donc lettre à lettre
        assert_eq!(plages_palette("VUE LISTE", "v e"), [(0, 1), (2, 3)]);
        assert_eq!(plages_palette("VUE LISTE", "e l"), [(2, 3), (4, 5)]);
        // Sous-chaîne absente : les lettres dans l'ordre, celles qui se touchent fondues en une plage
        assert_eq!(plages_palette("VUE LISTE", "vl"), [(0, 1), (4, 5)]);
        assert_eq!(plages_palette("ABCD", "acd"), [(0, 1), (2, 4)]);
        // Une lettre manquante, ou l'ordre inverse : rien n'est surligné
        assert!(plages_palette("VUE LISTE", "xyz").is_empty());
        assert!(plages_palette("ABC", "cba").is_empty());
        assert!(plages_palette("ABC", "").is_empty());
        assert!(plages_palette("ABC", "   ").is_empty());
        // Requête plus longue que le texte
        assert!(plages_palette("AB", "abcd").is_empty());
    }

    #[test]
    fn geometrie_de_la_liste_et_defilement() {
        let lignes = [LignePal::Titre("A"), LignePal::Ligne(0), LignePal::Ligne(1), LignePal::Titre("B"), LignePal::Ligne(2)];
        let g = geometrie(&lignes);
        assert_eq!(g[0], (LISTE_HAUT, TITRE_H));
        assert_eq!(g[1], (LISTE_HAUT + TITRE_H, RANG_H));
        assert_eq!(g[4], (LISTE_HAUT + TITRE_H + 2.0 * RANG_H + TITRE_H, RANG_H));
        assert_eq!(hauteur_contenu(&lignes), LISTE_HAUT + 2.0 * TITRE_H + 3.0 * RANG_H + LISTE_BAS);
        assert_eq!(hauteur_contenu(&[]), LISTE_HAUT + LISTE_BAS);
        // `scrollIntoView({block: 'nearest'})` : rien ne bouge si la ligne est visible
        assert_eq!(defilement_visible(0.0, 34.0, 46.0, 330.0, 800.0), 0.0);
        // Ligne sous la fenêtre : juste de quoi la montrer ; au-dessus : son haut
        assert_eq!(defilement_visible(0.0, 400.0, 46.0, 330.0, 800.0), 116.0);
        assert_eq!(defilement_visible(300.0, 100.0, 46.0, 330.0, 800.0), 100.0);
        // Jamais au-delà du contenu ni en dessous de zéro
        assert_eq!(defilement_visible(0.0, 790.0, 46.0, 330.0, 800.0), 470.0);
        assert_eq!(defilement_visible(50.0, 0.0, 46.0, 330.0, 200.0), 0.0);
    }

    #[test]
    fn apercu_d_une_commande_generale() {
        let item = |c: Commande| {
            let (_, libelle, raccourci, _) = COMMANDES.iter().find(|x| x.0 == c).copied().unwrap();
            let mut it = Item::nouveau(libelle, "", Icone::Fichier, Action::Commande(c), "PARTOUT");
            it.raccourci = raccourci;
            it
        };
        for (c, art) in [
            (Commande::VueListe, Art::Liste),
            (Commande::VueGrille, Art::Grille),
            (Commande::VueColonnes, Art::Colonnes),
            (Commande::Essentiel, Art::Essentiel),
            (Commande::NouveauDossier, Art::NouveauDossier),
            (Commande::Copier, Art::Copier),
            (Commande::Supprimer, Art::Supprimer),
            (Commande::RechercheGlobale, Art::Recherche),
            (Commande::Reglages, Art::Reglages),
        ] {
            let a = apercu_de(&item(c));
            assert_eq!(a.art, art, "{c:?}");
            assert!(!a.description.is_empty() && !a.titre.is_empty());
        }
        // Une commande sans dessin : titre et description (son raccourci, sinon ses mots-clés)
        let a = apercu_de(&item(Commande::Ouvrir));
        assert_eq!((a.art, a.description.as_str()), (Art::Aucun, "Raccourci : ENTREE"));
        assert!(apercu_de(&item(Commande::Partager)).description.starts_with("Mots-clés : share"));
    }

    #[test]
    fn apercu_d_un_dossier_de_partout_vient_de_la_lecture_asynchrone() {
        let s = Sources { epingles: &[], ..sources_vides() };
        let mut p = Palette::nouvelle();
        p.saisie = Edition::champ("travail");
        let epingles = vec![r"C:\Travail".to_string()];
        p.remplir(&Sources { epingles: &epingles, ..s });
        let k = p.items.iter().position(|i| matches!(&i.action, Action::Aller(Emplacement::Dossier(d)) if d == r"C:\Travail")).expect("dossier épinglé");
        p.choix = k;
        let (_, lire) = p.a_demander();
        assert_eq!(lire, [r"C:\Travail"], "lu par le fil de fond, jamais par le dessin");
        let Art::Contenu(c) = p.apercu().unwrap().art else { panic!() };
        assert!(c.en_cours && c.elements.is_empty());
        servir(&mut p, lecture(r"C:\Travail", &["a.txt"], &["Sous"]));
        let Art::Contenu(c) = p.apercu().unwrap().art else { panic!() };
        assert!(c.lu && c.elements.len() == 2 && c.nouveaux.is_empty() && c.effet.is_empty());
    }

    // ── Relecture du lot 15C : défauts corrigés ──

    #[test]
    fn des_resultats_d_index_n_inserent_pas_de_lignes_sous_le_choix_de_destination() {
        let (recents, epingles, s) = sources_destination();
        let s = Sources { recents: Box::leak(recents.into_boxed_slice()), epingles: Box::leak(epingles.into_boxed_slice()), ..s };
        let mut p = jusqu_a(CommandeEl::DeplacerVers, &s);
        assert_eq!(p.entrer(false), Suite::Rien);
        p.saisie = Edition::champ("doc");
        p.remplir(&s);
        // On vise « C:\Docs » (le dossier ouvert), tapé « doc » : il est après les récents
        let k = p.items.iter().position(|i| matches!(&i.action, Action::EntrerDans(d) if d == r"C:\Docs")).expect("dossier ouvert proposé");
        p.choix = k;
        // L'index répond : un dossier de plus s'insère AVANT la ligne visée (groupe RECHERCHE avant ICI)
        let trouves = vec![Trouve { chemin: r"D:\Travail\Documentation".into(), dossier: true, icone: Icone::Dossier }];
        assert!(p.recevoir_recherche("doc", trouves));
        p.remplir(&s);
        assert!(p.items.iter().any(|i| i.groupe == "RECHERCHE"), "la recherche a bien inséré une ligne");
        assert!(p.items.iter().position(|i| i.groupe == "RECHERCHE").unwrap() < p.choix, "elle est avant la ligne visée : le rang a bougé");
        match p.entrer(true) {
            Suite::Executer(e) => assert_eq!(e.dossier.as_deref(), Some(r"C:\Docs"), "Ctrl+Entrée valide toujours le dossier visé"),
            autre => panic!("{autre:?}"),
        }
        // Une frappe, elle, repart du haut de la liste
        let mut q = jusqu_a(CommandeEl::CopierVers, &s);
        q.entrer(false);
        q.remplir(&s);
        q.choix = 2;
        q.saisie = Edition::champ("a");
        q.choix = 0;
        q.remplir(&s);
        assert_eq!(q.choix, 0);
    }

    #[test]
    fn la_cle_d_une_ligne_suit_l_action_et_le_chemin() {
        let (_, _, s) = sources_destination();
        let mut p = jusqu_a(CommandeEl::CopierVers, &s);
        p.entrer(false);
        p.remplir(&s);
        let cles: HashSet<String> = p.items.iter().map(Item::cle).collect();
        assert_eq!(cles.len(), p.items.len(), "deux lignes ne partagent pas leur clé");
    }

    #[test]
    fn resultat_choisit_autre_action_d_office_et_un_double_entree_ne_defait_rien() {
        let s = avec_selection();
        let mut p = jusqu_a(CommandeEl::Renommer, &s);
        p.entrer(false);
        p.sujet_renomme(r"C:\Docs\rapport-v2.pdf".into());
        p.finir(Resultat::ok_avec_entree("Renommé en rapport-v2.pdf", 7));
        p.remplir(&s);
        assert!(p.items[0].libelle == "ANNULER" && matches!(p.items[p.choix].action, Action::AutreAction));
        // Un second Entrée tombe sur « Autre action » : aucune demande d'annulation
        assert_eq!(p.entrer(false), Suite::Rien);
        assert_eq!(p.etape, Etape::Commande);
        // Refaire la liste du résultat (une lecture arrive) garde le choix de l'utilisateur
        let mut q = jusqu_a(CommandeEl::CopierChemin, &s);
        q.finir(Resultat::ok_avec_entree("Copié", 3));
        q.remplir(&s);
        q.choix = 0;
        q.remplir(&s);
        assert_eq!(q.choix, 0, "le choix fait à la main n'est pas écrasé");
    }

    #[test]
    fn autre_action_apres_renommer_vise_le_nouveau_nom() {
        let s = avec_selection();
        let mut p = jusqu_a(CommandeEl::Renommer, &s);
        p.entrer(false);
        p.sujet_renomme(r"C:\Docs\rapport-v2.pdf".into());
        p.finir(Resultat::ok_avec_entree("Renommé en rapport-v2.pdf", 7));
        p.remplir(&s);
        assert!(!p.sujet_perime);
        assert_eq!(p.items.last().unwrap().libelle, "AUTRE ACTION SUR RAPPORT-V2.PDF");
        assert_eq!(p.entrer(false), Suite::Rien);
        assert_eq!(p.sujet.as_ref().unwrap().chemins, [r"C:\Docs\rapport-v2.pdf"], "les commandes suivantes visent le nouveau chemin");
        // Renommage défait (« Annulé ») : l'ancien nom est revenu, le sujet n'est plus fiable
        let mut q = jusqu_a(CommandeEl::Renommer, &s);
        q.entrer(false);
        q.sujet_renomme(r"C:\Docs\rapport-v2.pdf".into());
        q.finir(Resultat::ok("Annulé", false));
        assert!(q.sujet_perime);
    }

    #[test]
    fn autre_action_apres_deplacer_ou_corbeille_repart_de_l_element() {
        let (_, _, s) = sources_destination();
        for c in [CommandeEl::DeplacerVers, CommandeEl::Corbeille] {
            let mut p = jusqu_a(c, &s);
            p.entrer(false);
            p.finir(Resultat::ok_avec_entree("Fait", 9));
            p.remplir(&s);
            assert!(p.sujet_perime, "{c:?}");
            assert_eq!(p.items.last().unwrap().libelle, "AUTRE ACTION", "{c:?} : plus de nom d'un élément qui a bougé");
            assert_eq!(p.entrer(false), Suite::Rien);
            assert_eq!((p.etape, p.sujet.is_none(), p.sujet_perime), (Etape::Element, true, false), "{c:?} : le sujet est oublié, la corbeille ne vise pas l'ancien chemin");
            // Même sortie par Retour arrière sur le résultat
            let mut r = jusqu_a(c, &s);
            r.entrer(false);
            r.finir(Resultat::ok_avec_entree("Fait", 9));
            assert!(r.reculer());
            assert_eq!((r.etape, r.sujet.is_none()), (Etape::Element, true));
        }
        // Une copie ne déplace rien : « Autre action » garde l'élément
        let mut p = jusqu_a(CommandeEl::CopierVers, &s);
        p.entrer(false);
        p.finir(Resultat::ok_avec_entree("Copié", 9));
        assert!(!p.sujet_perime);
    }

    #[test]
    fn annuler_n_existe_que_pour_un_resultat_qui_a_une_entree_d_historique() {
        let s = avec_selection();
        let mut p = jusqu_a(CommandeEl::Renommer, &s);
        assert!(!p.peut_defaire(), "pas encore de résultat");
        p.finir(Resultat::erreur("un élément porte déjà ce nom"));
        assert!(!p.peut_defaire(), "renommage échoué : Ctrl+Z défairait une autre action");
        p.finir(Resultat::ok("Chemin copié", false));
        assert!(!p.peut_defaire());
        p.remplir(&s);
        assert!(p.items.iter().all(|i| !matches!(i.action, Action::Defaire)));
        p.finir(Resultat::ok_avec_entree("Renommé", 5));
        assert!(p.peut_defaire());
        p.remplir(&s);
        assert!(p.items.iter().any(|i| matches!(i.action, Action::Defaire)));
        p.etape = Etape::Commande;
        assert!(!p.peut_defaire(), "seulement sur l'écran de résultat");
    }

    #[test]
    fn une_lecture_non_attendue_ne_repeuple_pas_le_cache_et_une_recherche_ratee_se_redemande() {
        let mut p = Palette::nouvelle();
        let l = lecture(r"C:\Docs", &["a.txt"], &[]);
        assert!(!p.recevoir_lecture(l.clone()), "jamais demandée");
        assert!(p.lectures.is_empty());
        p.lectures.insert(cle_lecture(r"C:\Docs"), None);
        assert!(p.recevoir_lecture(l.clone()));
        assert!(!p.recevoir_lecture(l.clone()), "déjà reçue : un second exemplaire est jeté");
        // Une opération finit : la table est vidée, la lecture partie avant elle arrive après et n'est pas gardée
        p.lectures.insert(cle_lecture(r"C:\Docs"), None);
        p.finir(Resultat::ok("Fait", false));
        assert!(!p.recevoir_lecture(l), "état d'avant l'opération");
        assert!(p.lectures.is_empty());
        // Recherche d'index : demandée une fois, redemandée seulement si elle a échoué
        let mut q = Palette::nouvelle();
        q.saisie = Edition::champ("rapport");
        assert_eq!(q.a_demander().0.as_deref(), Some("rapport"));
        assert_eq!(q.a_demander().0, None, "une seule fois");
        q.recherche_echouee("autre");
        assert_eq!(q.a_demander().0, None, "l'échec d'une autre requête ne compte pas");
        q.recherche_echouee("rapport");
        assert_eq!(q.a_demander().0.as_deref(), Some("rapport"), "l'index a échoué : on redemande");
    }

    #[test]
    fn compresser_plusieurs_elements_de_dossiers_differents_est_grise() {
        let mut s = avec_selection();
        s.selection = vec![(r"C:\Docs\a.txt".into(), false, Icone::Texte), (r"D:\Autre\b.txt".into(), false, Icone::Texte)];
        let mut p = Palette::nouvelle();
        p.remplir(&s);
        assert_eq!(p.entrer(false), Suite::Rien);
        p.remplir(&s);
        let zip = p.items.iter().find(|i| matches!(i.action, Action::ChoisirCommande(CommandeEl::CompresserVers))).unwrap();
        assert!(zip.grise && zip.detail == "ÉLÉMENTS DE DOSSIERS DIFFÉRENTS");
    }

    // ── Quatre retours de l'auteur sur la palette 15C ──

    /// Les commandes de la palette d'avant le lot 15C (`COMMANDES` de HEAD 46d5cd4, 111 entrées) : aucune ne doit disparaître
    const ANCIENNES_COMMANDES: &[&str] = &[
        "Actualiser", "Administrateur", "AjouterPlateau", "Annuler", "Apercu", "Arbre", "ArracherOnglet", "BaliseBlanche",
        "BaliseGrise", "BaliseRouge", "Caches", "CheminUnix", "CheminWsl", "Chronologie", "Coller", "Comparer", "ComparerTextes",
        "Copier", "CopierChemin", "CopierVersAutre", "Couper", "Debloquer", "DeplacerPlateau", "DeposerPlateau", "DossierParDefaut",
        "DoublePanneau", "Doublons", "Ejecter", "EnregistrerSession", "Epingler", "EpinglerAccesRapide", "Essentiel",
        "ExclureRecherche", "Extraire", "FermerOnglet", "Galerie", "GlyphDossier", "Historique", "ImageJpg", "ImagePng",
        "ImageReduire", "ImageSansMeta", "InverserSelection", "LignesDossier", "LireTexteImage", "ListerTelephones", "Melanger",
        "ModeCapture", "NonFiable", "Note", "NouveauDossier", "NouveauFichier", "NouveauGroupe", "NouvelOnglet", "Ouvrir",
        "OuvrirAvec", "OuvrirJournalOperations", "OuvrirOnglet", "PartageProche", "Partager", "PoidsSonore", "Proprietes", "Ranger",
        "RattacherFenetre", "RechercheGlobale", "ReconstruireIndex", "Refaire", "Reglages", "RegleRangement", "Regrouper", "Regrouper",
        "Regrouper", "Regrouper", "Regrouper", "Regrouper", "Regrouper", "Renommer", "RenommerLot", "Restaurer", "RouvrirOnglet",
        "SansBalise", "SauvegardeAjouter", "SauvegardeDestination", "SauvegardeLancer", "SauvegardeVider", "SeptZip", "Sessions",
        "Sha256", "Sons", "Supprimer", "Synchroniser", "TaillesDossiers", "Terminal", "ToutChoisir", "TrierCroissant", "TrierDate",
        "TrierDecroissant", "TrierNom", "TrierTaille", "TrierType", "ViderCorbeille", "ViderPlateau", "VoirCorbeille", "VoirPanneau",
        "VoirPlateau", "VsCode", "VueColonnes", "VueGrille", "VueListe", "VueTailles", "Zip",
    ];

    #[test]
    fn aucune_commande_de_l_ancienne_palette_n_a_disparu() {
        assert_eq!(ANCIENNES_COMMANDES.len(), 111);
        let mut manquantes: Vec<String> = Vec::new();
        for nom in ANCIENNES_COMMANDES {
            // Une variante à paramètre (`Regrouper(..)`) compte pour toutes ses valeurs
            let cs: Vec<Commande> = COMMANDES.iter().map(|x| x.0).filter(|c| format!("{c:?}").split('(').next() == Some(*nom)).collect();
            if cs.is_empty() {
                manquantes.push(format!("{nom} : n'existe plus dans COMMANDES"));
            }
            for c in cs {
            let &(_, libelle, _, mots) = COMMANDES.iter().find(|x| x.0 == c).unwrap();
            if let Some(el) = commande_el_de(c) {
                // Commande d'élément : à l'étape 2 pour un élément, et en tapant son nom à l'étape 1 (sur la sélection)
                assert!(commande_pour_element(c), "{nom}");
                let s = Sources { selection: vec![(r"C:\Docs\Factures".into(), true, Icone::Dossier)], dossier_ouvert: Some(r"C:\Docs".into()), ..sources_vides() };
                let mut p = Palette::nouvelle();
                p.saisie = Edition::champ(assistant::def(el).libelle.trim_end_matches('…').trim_end_matches(" (ZIP)"));
                p.remplir(&s);
                if !p.items.iter().any(|i| matches!(&i.action, Action::CommandeSur(_, x) if *x == el)) {
                    manquantes.push(format!("{nom} : introuvable à l'étape 1 sous le nom de {el:?}"));
                }
                let mut q = jusqu_a(el, &s);
                q.remplir(&s);
                if !q.items.iter().any(|i| matches!(i.action, Action::ChoisirCommande(x) if x == el)) {
                    manquantes.push(format!("{nom} : absente de l'étape 2"));
                }
            } else if commande_experimentale(c).is_some() {
                // Fermées tant que « expérimental » est faux, comme dans l'ancienne palette
                assert!(!commande_permise(false, c) && commande_permise(true, c), "{nom}");
            } else {
                // « Partout » : par son libellé et par son premier mot-clé
                let sans = sources_vides();
                for q in [libelle.to_string(), mots.split_whitespace().next().unwrap_or("").to_string()] {
                    if q.is_empty() {
                        continue;
                    }
                    let mut p = Palette::nouvelle();
                    p.saisie = Edition::champ(&q);
                    p.remplir(&sans);
                    if !p.items.iter().any(|i| matches!(i.action, Action::Commande(x) if x == c)) {
                        manquantes.push(format!("{nom} : introuvable dans Partout en tapant « {q} »"));
                    }
                }
                let mut p = Palette::nouvelle();
                p.remplir(&sans);
                if !p.items.iter().any(|i| matches!(i.action, Action::Commande(x) if x == c)) {
                    manquantes.push(format!("{nom} : absente de la liste « Partout » sans requête"));
                }
            }
            }
        }
        assert!(manquantes.is_empty(), "commandes perdues : {manquantes:#?}");
    }

    #[test]
    fn ajouter_a_un_groupe_propose_d_abord_de_creer_un_nouveau_groupe() {
        let groupes = vec![crate::groupes::Groupe { nom: "ROUGE".into(), couleur: 'r', nombre: 2 }];
        let s = Sources { groupes: &groupes, ..sources_vides() };
        let mut p = Palette::groupe(vec![r"C:\Docs\a.txt".into()]);
        p.remplir(&s);
        assert_eq!(p.items[0].libelle, "CRÉER UN NOUVEAU GROUPE…", "en tête de la liste des groupes");
        assert!(matches!(p.items[0].action, Action::NouveauGroupe));
        assert!(p.items.iter().any(|i| i.libelle == "ROUGE"), "les groupes existants suivent");
        // Choisie : le champ attend le nom (la palette ne se ferme pas), Entrée sur la ligne d'information ne fait rien
        p.demander_nom_groupe();
        p.remplir(&s);
        assert!(p.items[0].grise && matches!(p.items[0].action, Action::Rien) && p.items.len() == 1);
        assert!(p.invite().starts_with("NOM DU NOUVEAU GROUPE"));
        // Le nom tapé : « Créer » ajoute l'élément au groupe créé (`groupes::creer`, violet en première couleur)
        p.saisie = Edition::champ("projet");
        p.remplir(&s);
        assert_eq!(p.items.len(), 1);
        assert!(matches!(&p.items[0].action, Action::MettreDansGroupe(n) if n == "PROJET") && p.items[0].libelle == "CRÉER LE GROUPE « PROJET »");
        // Un nom qui existe : on ajoute au groupe existant
        p.saisie = Edition::champ("rouge");
        p.remplir(&s);
        assert_eq!(p.items[0].libelle, "AJOUTER AU GROUPE « ROUGE »");
        // Avec un nom déjà tapé dans la liste, la création directe prend la tête
        let mut q = Palette::groupe(vec!["x".into()]);
        q.saisie = Edition::champ("neuf");
        q.remplir(&s);
        assert!(matches!(&q.items[0].action, Action::MettreDansGroupe(n) if n == "NEUF"));
    }

    #[test]
    fn recents_de_l_etape_1_sont_de_vrais_elements_et_jamais_des_reprises() {
        let reprises = vec![r"C:\Projets".to_string() + "\tPlan 2 Roue 2026.dwg", r"C:\Downloads".to_string() + "\tPlan 2 Roue 2026.dwg", r"C:\Vide".to_string() + "\t", r"C:\Absent".to_string() + "\tfantome.txt"];
        let recents = vec![r"D:\Archives".to_string(), "?contrat".to_string()];
        let existe = |c: &str| match c {
            r"C:\Downloads\Plan 2 Roue 2026.dwg" => Some(false),
            r"C:\Vide" | r"D:\Archives" => Some(true),
            _ => None,
        };
        let v = assistant::elements_recents_avec(&reprises, &recents, &existe);
        let chemins: Vec<&str> = v.iter().map(|x| x.0.as_str()).collect();
        assert_eq!(chemins, [r"C:\Downloads\Plan 2 Roue 2026.dwg", r"C:\Vide", r"D:\Archives"], "ce qui existe, sans « reprendre », sans recherche enregistrée, sans fantôme");
        assert!(!v[0].1 && v[1].1 && v[2].1, "fichier ou dossier selon le disque");
        // À l'étape 1 : section RÉCENTS, que des éléments à choisir, détail « RÉCENT · dossier », jamais « REPRENDRE »
        let s = Sources { recents_elements: v, ..sources_vides() };
        let mut p = Palette::nouvelle();
        p.remplir(&s);
        let recents: Vec<&Item> = p.items.iter().filter(|i| i.groupe == "RÉCENTS").collect();
        assert_eq!(recents.len(), 3);
        assert!(recents.iter().all(|i| matches!(i.action, Action::ChoisirSujet(_)) && !i.libelle.contains("REPRENDRE") && i.detail.starts_with("RÉCENT · ")));
        assert_eq!(recents[0].libelle, "Plan 2 Roue 2026.dwg");
        assert!(p.items.iter().all(|i| !i.libelle.contains("REPRENDRE") && !i.detail.contains("REPRENDRE")));
        // Une requête filtre les récents comme le reste
        p.saisie = Edition::champ("archives");
        p.remplir(&s);
        assert!(p.items.iter().any(|i| i.groupe == "RÉCENTS" && i.libelle == "Archives"));
        assert!(p.items.iter().all(|i| i.groupe != "RÉCENTS" || i.libelle != "Plan 2 Roue 2026.dwg"));
        // Étape destination : les récents sont des dossiers seulement
        let l = assistant::Lieux { autre_panneau: None, recents: &[r"D:\Archives".to_string(), "?contrat".to_string()], epingles: &[], volet: &[], courant: None };
        let d = assistant::destinations(&l, "", &[]);
        assert!(d.iter().all(|x| x.groupe != "RÉCENTS" || x.chemin == r"D:\Archives"));
    }

    #[test]
    fn une_commande_d_element_tapee_a_l_etape_1_saute_l_etape_2() {
        let s = avec_selection();
        let mut p = Palette::nouvelle();
        p.saisie = Edition::champ("renommer");
        p.remplir(&s);
        let k = p.items.iter().position(|i| matches!(&i.action, Action::CommandeSur(_, CommandeEl::Renommer))).expect("proposée sur la sélection");
        assert_eq!((p.items[k].groupe, p.items[k].detail.as_str()), ("SUR L'ÉLÉMENT", "SUR RAPPORT.PDF"));
        p.choix = k;
        assert_eq!(p.entrer(false), Suite::Rien);
        assert_eq!((p.etape, p.commande, p.requete().as_str()), (Etape::Parametre, Some(CommandeEl::Renommer), "rapport.pdf"));
        // Sans paramètre, elle part tout de suite
        let mut q = Palette::nouvelle();
        q.saisie = Edition::champ("copier le chemin");
        q.remplir(&s);
        q.choix = q.items.iter().position(|i| matches!(&i.action, Action::CommandeSur(_, CommandeEl::CopierChemin))).unwrap();
        assert!(matches!(q.entrer(false), Suite::Executer(e) if e.commande == CommandeEl::CopierChemin && e.sujet.libelle() == "rapport.pdf"));
        // Ce qui ne s'applique pas à l'élément n'est pas proposé (un fichier n'a pas de « nouveau dossier dedans »)
        let mut r = Palette::nouvelle();
        r.saisie = Edition::champ("nouveau dossier");
        r.remplir(&s);
        assert!(!r.items.iter().any(|i| matches!(&i.action, Action::CommandeSur(_, CommandeEl::NouveauDossier))));
    }

    #[test]
    fn le_pied_de_chaque_etape_montre_retour_et_suivant() {
        let s = avec_selection();
        let mut p = jusqu_a(CommandeEl::CopierVers, &s);
        p.remplir(&s);
        let touches = |p: &Palette| -> Vec<(&'static str, &'static str)> { p.pied().1 };
        assert!(touches(&p).contains(&("← / ⌫", "retour")) && touches(&p).contains(&("→ / ↵", "suivant")), "étape 2");
        p.entrer(false);
        assert!(touches(&p).contains(&("← / ⌫", "retour")) && touches(&p).iter().any(|g| g.0 == "→ / ↵"), "étape 3 destination");
        assert!(touches(&Palette::nouvelle()).contains(&("→ / ↵", "suivant")), "étape 1");
        let mut r = jusqu_a(CommandeEl::CopierChemin, &s);
        r.finir(Resultat::ok("Chemin copié", false));
        assert!(touches(&r).iter().any(|g| g.0.starts_with('←')), "résultat");
        // ← revient comme Retour arrière
        assert!(r.reculer());
        assert_eq!(r.etape, Etape::Commande);
    }

    #[test]
    fn cles_de_liste_et_de_commande() {
        let s = avec_selection();
        let mut p = Palette::nouvelle();
        p.remplir(&s);
        let (liste, commande) = (p.cle_liste(), p.cle_commande());
        p.deplacer(1);
        assert_eq!(p.cle_liste(), liste, "une flèche ne reconstruit pas la liste");
        assert_ne!(p.cle_commande(), commande, "mais change la ligne montrée");
        let c = p.cle_liste();
        p.saisie = Edition::champ("a");
        assert_ne!(p.cle_liste(), c, "une requête reconstruit la liste");
        // Une autre étape aussi : le décalage de la liste revient en haut
        let mut q = jusqu_a(CommandeEl::CopierVers, &s);
        let avant = q.cle_liste();
        q.entrer(false);
        assert_ne!(q.cle_liste(), avant);
    }
}
