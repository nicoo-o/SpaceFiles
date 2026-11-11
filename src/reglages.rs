// Réglages de SpaceFiles : `%APPDATA%\SpaceFiles\reglages.toml`, lisible et copiable.
// Format TOML simple écrit et relu ici (pas de dépendance) : clés, textes, booléens, nombres, listes de textes,
// et une table `[[stockage]]` par stockage ajouté. Aucun mot de passe ici : ils vont dans le Gestionnaire
// d'identification de Windows.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

use crate::dossier::Colonne;

/// Dossier ouvert au lancement (sans argument)
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Depart {
    Personnel,
    CePC,
    Dernier,
}

impl Depart {
    pub const TOUS: [Depart; 3] = [Depart::Personnel, Depart::CePC, Depart::Dernier];

    fn cle(self) -> &'static str {
        match self {
            Depart::Personnel => "personnel",
            Depart::CePC => "ce-pc",
            Depart::Dernier => "dernier",
        }
    }

    pub fn libelle(self) -> &'static str {
        match self {
            Depart::Personnel => "Dossier personnel",
            Depart::CePC => "Ce PC",
            Depart::Dernier => "Dernier dossier",
        }
    }
}

/// Genre d'un stockage ajouté à la main
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Sorte {
    /// Partage Windows (SMB) : NAS, autre PC
    Smb,
    /// Serveur ZimaOS : partages SMB, accès distant par le réseau privé Zima
    Zima,
    /// WebDAV / Nextcloud, par le client WebDAV de Windows (\\hote@SSL\chemin)
    WebDav,
    /// N'importe quel dossier ou lecteur (Google Drive en mode dossier, disque réseau…)
    Dossier,
    /// Serveur SSH, par le client OpenSSH de Windows (clé SSH seulement), en lecture
    Sftp,
}

impl Sorte {
    pub const TOUTES: [Sorte; 5] = [Sorte::Zima, Sorte::Smb, Sorte::WebDav, Sorte::Sftp, Sorte::Dossier];

    fn cle(self) -> &'static str {
        match self {
            Sorte::Smb => "smb",
            Sorte::Zima => "zima",
            Sorte::WebDav => "webdav",
            Sorte::Dossier => "dossier",
            Sorte::Sftp => "sftp",
        }
    }

    fn depuis(s: &str) -> Sorte {
        Sorte::TOUTES.into_iter().find(|t| t.cle() == s).unwrap_or(Sorte::Dossier)
    }

    pub fn libelle(self) -> &'static str {
        match self {
            Sorte::Smb => "PARTAGE WINDOWS (SMB)",
            Sorte::Zima => "SERVEUR ZIMAOS",
            Sorte::WebDav => "WEBDAV / NEXTCLOUD",
            Sorte::Dossier => "DOSSIER OU LECTEUR",
            Sorte::Sftp => "SFTP (CLE SSH)",
        }
    }

    /// Exemple d'adresse affiché dans le champ vide
    pub fn exemple(self) -> &'static str {
        match self {
            Sorte::Smb => "\\\\NAS  OU  192.168.0.20",
            Sorte::Zima => "ZIMAOS  OU  10.240.X.X (ACCES DISTANT)",
            Sorte::WebDav => "HTTPS://CLOUD.EXEMPLE.FR/REMOTE.PHP/DAV",
            Sorte::Dossier => "G:\\MON DRIVE",
            Sorte::Sftp => "SERVEUR  OU  SERVEUR:2222/CHEMIN",
        }
    }

    /// Les serveurs vont dans la section SERVEURS du volet, les dossiers dans CLOUD
    pub fn est_serveur(self) -> bool {
        self != Sorte::Dossier
    }

    /// La fonction gelée derrière le réglage `experimental` que ce genre de stockage utilise, s'il y en a une
    pub fn experimentale(self) -> Option<Experimentale> {
        match self {
            Sorte::Sftp => Some(Experimentale::Sftp),
            Sorte::WebDav => Some(Experimentale::WebDav),
            Sorte::Smb | Sorte::Zima | Sorte::Dossier => None,
        }
    }

    /// Genres proposés par le formulaire d'ajout : SFTP et WebDAV seulement si les fonctions expérimentales sont actives
    pub fn proposees(experimental: bool) -> Vec<Sorte> {
        Sorte::TOUTES.into_iter().filter(|s| sorte_permise(experimental, *s)).collect()
    }
}

/// Fonctions gelées derrière le réglage `experimental` (peu testées : le code reste, l'accès est fermé)
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Experimentale {
    Sftp,
    WebDav,
    /// Copie vers un NAS (commandes SAUVEGARDE de la palette)
    Sauvegarde,
    Mtp,
}

/// Une fonction expérimentale est-elle atteignable ? Fonction pure : tous les filtres passent par elle
pub fn fonction_experimentale_permise(experimental: bool, genre: Experimentale) -> bool {
    match genre {
        Experimentale::Sftp | Experimentale::WebDav | Experimentale::Sauvegarde | Experimentale::Mtp => experimental,
    }
}

/// Un stockage de ce genre est-il montré (volet) et proposé (formulaire) ? SMB, ZimaOS et dossiers : toujours
pub fn sorte_permise(experimental: bool, sorte: Sorte) -> bool {
    sorte.experimentale().is_none_or(|g| fonction_experimentale_permise(experimental, g))
}

/// Fonction expérimentale visée par une adresse tapée : `sftp://…`, `http(s)://…` (WebDAV) ou son chemin
/// Windows `\\hote@SSL\…` (un `@` dans le nom d'hôte). Un partage SMB `\\serveur\partage` n'en est pas une.
pub fn genre_adresse(adresse: &str) -> Option<Experimentale> {
    let t = adresse.trim().trim_matches('"');
    let bas = t.to_ascii_lowercase();
    if bas.starts_with("sftp://") || bas.starts_with("sftp:\\\\") {
        Some(Experimentale::Sftp)
    } else if bas.starts_with("http://") || bas.starts_with("https://") || bas.starts_with("http:\\\\") || bas.starts_with("https:\\\\") {
        Some(Experimentale::WebDav)
    } else if t.strip_prefix("\\\\").is_some_and(|r| r.split('\\').next().unwrap_or("").contains('@')) {
        Some(Experimentale::WebDav)
    } else {
        None
    }
}

/// L'adresse peut-elle être ouverte ? Faux seulement pour SFTP / WebDAV quand les fonctions expérimentales sont coupées
pub fn adresse_permise(experimental: bool, adresse: &str) -> bool {
    genre_adresse(adresse).is_none_or(|g| fonction_experimentale_permise(experimental, g))
}

/// `adresse_permise` avec le réglage en vigueur, lu seulement si l'adresse vise une fonction expérimentale
pub fn adresse_permise_maintenant(adresse: &str) -> bool {
    genre_adresse(adresse).is_none_or(|g| fonction_experimentale_permise(experimental(), g))
}

/// Message de la barre d'état quand une fonction gelée est demandée
pub const MESSAGE_EXPERIMENTAL: &str = "FONCTION EXPERIMENTALE : ACTIVEZ-LA DANS LES REGLAGES";

/// Réglage `experimental` en vigueur (lu sans verrou ; premier appel : charge les réglages)
pub fn experimental() -> bool {
    if !EXPERIMENTAL_LU.load(Ordering::Relaxed) {
        let _ = actuels();
    }
    EXPERIMENTAL.load(Ordering::Relaxed)
}

static EXPERIMENTAL: AtomicBool = AtomicBool::new(false);
static EXPERIMENTAL_LU: AtomicBool = AtomicBool::new(false);

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Stockage {
    pub nom: String,
    pub sorte: Sorte,
    pub adresse: String,
    pub utilisateur: String,
}

#[derive(Clone, Debug)]
pub struct Reglages {
    pub depart: Depart,
    pub restaurer_onglets: bool,
    pub caches: bool,
    pub dates_relatives: bool,
    pub apercu: bool,
    pub animations: bool,
    pub sons: bool,
    pub un_clic: bool,
    /// Dossiers à ressort pendant un glisser : délai en ms, 0 = jamais
    pub ressort_ms: u32,
    /// Téléchargements : fichiers arrivés depuis plus de N jours mis à la corbeille (0 = jamais)
    pub nettoyage_jours: u32,
    /// Téléchargements : un fichier copié ailleurs sur le PC part à la corbeille
    pub nettoyer_copies: bool,
    /// Volet : section ARBORESCENCE (arbre de dossiers)
    pub arbre: bool,
    /// Téléchargements : une archive extraite part à la corbeille
    pub nettoyer_archives: bool,
    /// Règles de rangement « pdf,docx|D:\\Factures » (Z6), et application à chaque arrivée dans Téléchargements
    pub regles: Vec<String>,
    /// Sauvegarde vers le NAS (Z8) : dossier de destination, et dossiers à copier. Rien ne part sans la commande LANCER LA SAUVEGARDE
    pub sauvegarde_dest: String,
    pub sauvegarde: Vec<String>,
    /// Raccourcis choisis « CTRL+MAJ+D=Doublons » (touche, nom de la commande) : lus à l'ouverture et à chaque réglage
    pub raccourcis: Vec<String>,
    /// « dossier\tnom » : les 5 derniers dossiers quittés et leur sélection
    pub reprises: Vec<String>,
    /// Copie vers un NAS, un partage ou une clé : compare l'empreinte SHA-256 de chaque fichier à son original (V16)
    pub copie_verifiee: bool,
    /// Dossiers non fiables (V11) : pas d'aperçu, de vignettes, de propriétés ni de Git dedans ni dans leurs sous-dossiers
    pub non_fiables: Vec<String>,
    pub ranger_auto: bool,
    /// Téléchargements : détecte ceux du navigateur (fichiers .crdownload / .part) et les montre en pastille ; vrai par défaut
    pub telechargements_navigateur: bool,
    pub etat_cloud: bool,
    /// Densité de la liste : 0 compacte (30 DIP), 1 confort (40 DIP)
    pub taille_lignes: u32,
    /// Extensions des fichiers montrées (« photo.jpg ») ou cachées (« photo »)
    pub extensions: bool,
    /// Contraste renforcé (en plus du thème Contraste élevé de Windows, suivi tout seul)
    pub contraste: bool,
    /// Thème : auto (avis de shared.json, sinon sombre), sombre, clair, ou celui de Windows
    pub theme: crate::theme::ChoixTheme,
    /// Volet de gauche plus serré (lignes de 26 DIP au lieu de 32)
    pub volet_dense: bool,
    /// Mouvement « Réduit » (maquette v6) : coupe ressorts, reflet et dissolutions comme « animations désactivées »
    /// (clé `mouvement` : « complet » / « reduit »)
    pub mouvement_reduit: bool,
    /// Trame de points derrière la fenêtre (clé `trame`)
    pub trame: bool,
    /// Reflet qui suit le curseur au survol (clé `reflet`)
    pub reflet: bool,
    /// Les deux yeux de Pixel dans la barre d'état (clé `yeux_pixel`)
    pub yeux_pixel: bool,
    /// Pixel selon l'heure : Auto (heure locale) ou une période forcée (clé `pixel`)
    pub pixel: crate::pixel::ChoixPixel,
    /// Réponse automatique aux noms déjà pris : 0 demander, 1 remplacer, 2 garder les deux, 3 le plus récent, 4 ignorer
    pub conflit_defaut: u32,
    /// Vue des nouveaux onglets : 0 liste, 1 grille, 2 colonnes
    pub vue_defaut: u32,
    /// Sections du volet repliées / masquées (bits : rang dans rendu::SECTIONS_VOLET)
    pub volet_replie: u32,
    pub volet_masque: u32,
    /// Dossiers et disques exclus de l'index de recherche (« d:\ », « c:\jeux\ »)
    pub exclus: Vec<String>,
    /// Retenir les dossiers ouverts (palette)
    pub retenir_recents: bool,
    /// Copies et déplacements par le réseau (NAS…) : débit maximal en Mo/s, 0 = sans limite
    pub limite_reseau_mo: u32,
    /// Chronologie en points sous la liste (O3)
    pub chronologie: bool,
    /// Notes de dossier (O8) : « chemin|texte », gardées ici plutôt que dans un fichier caché du dossier
    pub notes: Vec<String>,
    /// Glyphs de dossier (O1) : « NOM|chemin »
    pub glyphes: Vec<String>,
    /// Poids sonore (O4) : un tic au survol d'un dossier, plus grave s'il est lourd
    pub poids_sonore: bool,
    /// Fonctions peu testées (SFTP, WebDAV, sauvegarde NAS, MTP) : fermées tant que ce réglage est faux.
    /// Absent du fichier mais avec un stockage SFTP / WebDAV déjà ajouté : lu vrai (voir `lire`)
    pub experimental: bool,
    pub stockages: Vec<Stockage>,
    /// Onglets ouverts à la fermeture, et dossier affiché
    pub onglets: Vec<String>,
    pub onglet_actif: usize,
    pub dernier: String,
    /// Dossiers épinglés dans le volet (Ctrl+D)
    pub epingles: Vec<String>,
    /// Derniers dossiers ouverts (palette Ctrl+Maj+P), du plus récent au plus ancien
    pub recents: Vec<String>,
    /// Balises de couleur : « r|chemin », « b|chemin », « g|chemin »
    pub balises: Vec<String>,
    /// Tri choisi par dossier : « D|0|1|chemin » (colonne N/D/T/S, croissant, mélangé)
    pub tris: Vec<String>,
    /// « L|dossier » : la vue (L liste, G grille, C colonnes) choisie pour un dossier
    pub vues: Vec<String>,
    /// « colonnes|lignes|dossier » : colonnes montrées et taille des lignes propres à un dossier (V8)
    pub dispositions: Vec<String>,
    /// Groupes créés (« couleur|NOM ») et leurs éléments (« NOM|chemin ») — groupes.rs
    pub groupes: Vec<String>,
    pub membres: Vec<String>,
    /// Colonnes de la liste (clic droit sur l'en-tête) : bits COL_* et date montrée (DATE_*)
    pub colonnes: u32,
    /// Regroupement choisi par dossier : « D|chemin » (lettre de Regroupement)
    pub regroupements: Vec<String>,
    /// Sessions nommées : « NOM|onglet1|onglet2… » (chemins comme pour les onglets restaurés)
    pub sessions: Vec<String>,
}

impl Default for Reglages {
    fn default() -> Self {
        Self {
            depart: Depart::Personnel,
            restaurer_onglets: false,
            caches: false,
            dates_relatives: false,
            // Maquette v6 : l'aperçu est ouvert pour un nouveau profil. Un profil existant qui a
            // déjà écrit `apercu = false` garde explicitement son choix.
            apercu: true,
            animations: true,
            sons: true,
            un_clic: false,
            ressort_ms: 700,
            nettoyage_jours: 0,
            nettoyer_copies: false,
            arbre: false,
            nettoyer_archives: false,
            regles: Vec::new(),
            sauvegarde_dest: String::new(),
            sauvegarde: Vec::new(),
            raccourcis: Vec::new(),
            reprises: Vec::new(),
            copie_verifiee: false,
            non_fiables: Vec::new(),
            ranger_auto: false,
            telechargements_navigateur: true,
            etat_cloud: true,
            taille_lignes: 1,
            extensions: true,
            contraste: false,
            theme: crate::theme::ChoixTheme::Auto,
            volet_dense: false,
            mouvement_reduit: false,
            trame: true,
            reflet: true,
            yeux_pixel: true,
            pixel: crate::pixel::ChoixPixel::Auto,
            conflit_defaut: 0,
            vue_defaut: 0,
            volet_replie: 0,
            volet_masque: 0,
            exclus: Vec::new(),
            retenir_recents: true,
            limite_reseau_mo: 0,
            chronologie: false,
            notes: Vec::new(),
            glyphes: Vec::new(),
            poids_sonore: false,
            experimental: false,
            stockages: Vec::new(),
            onglets: Vec::new(),
            onglet_actif: 0,
            dernier: String::new(),
            epingles: Vec::new(),
            recents: Vec::new(),
            balises: Vec::new(),
            tris: Vec::new(),
            vues: Vec::new(),
            dispositions: Vec::new(),
            groupes: Vec::new(),
            membres: Vec::new(),
            colonnes: COL_TYPE,
            regroupements: Vec::new(),
            sessions: Vec::new(),
        }
    }
}

pub const DELAIS_RESSORT: [u32; 4] = [0, 500, 700, 1200];
pub const JOURS_NETTOYAGE: [u32; 4] = [0, 30, 60, 90];
pub const LIMITES_RESEAU: [u32; 6] = [0, 2, 5, 10, 25, 50];

static REGLAGES: Mutex<Option<Reglages>> = Mutex::new(None);

/// Mode portable : un fichier « portable » (ou « portable.txt ») à côté du programme, et tout ce que
/// l'application garde (réglages, index) va dans « donnees » à côté de lui, rien dans le profil Windows
pub fn dossier_portable() -> Option<std::path::PathBuf> {
    static P: std::sync::OnceLock<Option<std::path::PathBuf>> = std::sync::OnceLock::new();
    P.get_or_init(|| {
        let exe = std::env::current_exe().ok()?;
        let d = exe.parent()?;
        (d.join("portable").exists() || d.join("portable.txt").exists()).then(|| d.join("donnees"))
    })
    .clone()
}

/// Nom du dossier de données sous `%APPDATA%` et `%LOCALAPPDATA%`, et son nom d'avant le renommage (copié une fois
/// vers le nouveau par `migration`, jamais modifié) : les seuls endroits du code où ces noms s'écrivent
pub const NOM_DONNEES: &str = "SpaceFiles";
pub const ANCIEN_NOM_DONNEES: &str = "NothingExplorer";

/// Dossier des données : `%APPDATA%\SpaceFiles` (ou `%LOCALAPPDATA%` si `local`), ou celui du mode portable
pub fn dossier_donnees(local: bool) -> Option<std::path::PathBuf> {
    if let Some(p) = dossier_portable() {
        return Some(p);
    }
    let base = std::env::var_os(if local { "LOCALAPPDATA" } else { "APPDATA" })?;
    Some(std::path::Path::new(&base).join(NOM_DONNEES))
}

pub fn chemin_fichier() -> Option<std::path::PathBuf> {
    Some(dossier_donnees(false)?.join("reglages.toml"))
}

/// Copie des réglages actuels (lus sur le disque la première fois)
/// `chemin` est-il dans l'un de ces dossiers ou l'un de leurs sous-dossiers ? (sans tenir compte de la casse)
pub fn est_sous(chemin: &str, dossiers: &[String]) -> bool {
    let c = chemin.trim_end_matches('\\').to_lowercase();
    dossiers.iter().any(|d| {
        let d = d.trim_end_matches('\\').to_lowercase();
        c == d || c.starts_with(&format!("{d}\\"))
    })
}

/// Dossier marqué non fiable (V11)
pub fn non_fiable(dossier: &str) -> bool {
    est_sous(dossier, &actuels().non_fiables)
}

pub fn actuels() -> Reglages {
    let mut g = REGLAGES.lock().unwrap_or_else(|e| e.into_inner());
    g.get_or_insert_with(charger).clone()
}

/// Tri d'un dossier : celui choisi la dernière fois, sinon le plus récent d'abord (fichiers et dossiers
/// mélangés) dans Téléchargements, Bureau, Images, Vidéos et Musique, et par nom ailleurs
pub fn tri_du_dossier(chemin: &str) -> (Colonne, bool, bool) {
    let r = actuels();
    let memorise = r.tris.iter().find_map(|t| {
        let mut p = t.splitn(4, '|');
        let (col, croissant, melange, c) = (p.next()?, p.next()?, p.next()?, p.next()?);
        c.eq_ignore_ascii_case(chemin).then(|| {
            let col = match col { "D" => Colonne::Date, "T" => Colonne::Type, "S" => Colonne::Taille, "X" => Colonne::Dimensions, "U" => Colonne::Duree, "A" => Colonne::Auteur, "O" => Colonne::Origine, "G" => Colonne::Signature, _ => Colonne::Nom };
            (col, croissant == "1", melange == "1")
        })
    });
    memorise.unwrap_or_else(|| if crate::systeme::dossier_recent(chemin) { (Colonne::Date, false, true) } else { (Colonne::Nom, true, false) })
}

/// Note épinglée à un dossier (O8)
pub fn note_du_dossier(chemin: &str) -> Option<String> {
    let chemin = chemin.trim_end_matches('\\');
    actuels().notes.iter().find_map(|n| {
        let (c, t) = n.split_once('|')?;
        c.eq_ignore_ascii_case(chemin).then(|| t.to_string())
    })
}

/// Écrit (ou efface, texte vide) la note d'un dossier
pub fn noter_dossier(chemin: &str, texte: &str) {
    let chemin = chemin.trim_end_matches('\\').to_string();
    let texte = texte.trim().replace(['\r', '\n'], " ");
    modifier(|r| {
        r.notes.retain(|n| !n.split_once('|').is_some_and(|(c, _)| c.eq_ignore_ascii_case(&chemin)));
        if !texte.is_empty() {
            r.notes.push(format!("{chemin}|{texte}"));
        }
    });
}

/// Regroupement d'un dossier : celui choisi la dernière fois, sinon par date dans Téléchargements
pub fn regroupement_du_dossier(chemin: &str) -> crate::dossier::Regroupement {
    let r = actuels();
    let memorise = r.regroupements.iter().find_map(|t| {
        let (l, c) = t.split_once('|')?;
        c.eq_ignore_ascii_case(chemin).then(|| crate::dossier::Regroupement::depuis(l.chars().next().unwrap_or('-')))
    });
    memorise.unwrap_or_else(|| if crate::systeme::dossier_telechargements(chemin) { crate::dossier::Regroupement::Date } else { crate::dossier::Regroupement::Aucun })
}

pub fn memoriser_regroupement(chemin: &str, g: crate::dossier::Regroupement) {
    let ligne = format!("{}|{chemin}", g.lettre());
    let fin = format!("|{}", chemin.to_lowercase());
    modifier(|r| {
        r.regroupements.retain(|t| !t.to_lowercase().ends_with(&fin));
        r.regroupements.insert(0, ligne);
        r.regroupements.truncate(300);
    });
}

/// Vue d'un dossier (0 liste, 1 grille, 2 colonnes) : celle choisie la dernière fois, sinon la vue par défaut
pub fn vue_du_dossier(chemin: &str) -> u32 {
    let r = actuels();
    r.vues
        .iter()
        .find_map(|t| {
            let (v, c) = t.split_once('|')?;
            c.eq_ignore_ascii_case(chemin).then(|| ["L", "G", "C"].iter().position(|x| *x == v).unwrap_or(0) as u32)
        })
        .unwrap_or(r.vue_defaut)
}

pub fn memoriser_vue(chemin: &str, vue: u32) {
    let ligne = format!("{}|{chemin}", ["L", "G", "C"][(vue as usize).min(2)]);
    let fin = format!("|{}", chemin.to_lowercase());
    modifier(|r| {
        r.vues.retain(|t| !t.to_lowercase().ends_with(&fin));
        r.vues.insert(0, ligne);
        r.vues.truncate(300);
    });
}

/// Colonnes et taille de lignes du dossier affiché, quand il a les siennes (sinon les réglages généraux servent)
static LOCAL: std::sync::Mutex<Option<(u32, u32)>> = std::sync::Mutex::new(None);

/// Lit « colonnes|lignes|dossier » ; None si la ligne n'est pas celle de ce dossier
fn disposition(ligne: &str, chemin: &str) -> Option<(u32, u32)> {
    let mut p = ligne.splitn(3, '|');
    let (c, l, d) = (p.next()?.parse::<u32>().ok()?, p.next()?.parse::<u32>().ok()?, p.next()?);
    d.eq_ignore_ascii_case(chemin).then_some((c.min(2047), l.min(1)))
}

/// À appeler quand un dossier s'affiche (ou qu'on change d'onglet) : ses colonnes et ses lignes, ou celles par défaut
pub fn afficher_dossier(chemin: Option<&str>) {
    let r = actuels();
    let propre = chemin.and_then(|c| r.dispositions.iter().find_map(|l| disposition(l, c)));
    *LOCAL.lock().unwrap_or_else(|e| e.into_inner()) = propre;
    appliquer(&r);
}

/// Retient pour ce dossier les colonnes et la taille de lignes données
pub fn memoriser_disposition(chemin: &str, colonnes: u32, lignes: u32) {
    let fin = format!("|{}", chemin.to_lowercase());
    modifier(|r| {
        r.dispositions.retain(|t| !t.to_lowercase().ends_with(&fin));
        r.dispositions.insert(0, format!("{colonnes}|{lignes}|{chemin}"));
        r.dispositions.truncate(300);
    });
    afficher_dossier(Some(chemin));
}

/// Le dossier reprend les réglages généraux
pub fn oublier_disposition(chemin: &str) {
    let fin = format!("|{}", chemin.to_lowercase());
    modifier(|r| r.dispositions.retain(|t| !t.to_lowercase().ends_with(&fin)));
    afficher_dossier(Some(chemin));
}

/// Retient le tri choisi pour ce dossier (les 300 derniers dossiers)
pub fn memoriser_tri(chemin: &str, tri: (Colonne, bool, bool)) {
    let col = match tri.0 { Colonne::Nom => "N", Colonne::Date => "D", Colonne::Type => "T", Colonne::Taille => "S", Colonne::Dimensions => "X", Colonne::Duree => "U", Colonne::Auteur => "A", Colonne::Origine => "O", Colonne::Signature => "G" };
    let ligne = format!("{col}|{}|{}|{chemin}", tri.1 as u8, tri.2 as u8);
    if actuels().tris.first() == Some(&ligne) {
        return;
    }
    let fin = format!("|{}", chemin.to_lowercase());
    modifier(|r| {
        r.tris.retain(|t| !t.to_lowercase().ends_with(&fin));
        r.tris.insert(0, ligne);
        r.tris.truncate(300);
    });
}

/// Modifie les réglages et les enregistre tout de suite
pub fn modifier(f: impl FnOnce(&mut Reglages)) {
    let copie = {
        let mut g = REGLAGES.lock().unwrap_or_else(|e| e.into_inner());
        // Relire le fichier avant d'appliquer : une autre fenêtre (Win+E en lance une nouvelle) a pu
        // l'écrire depuis notre démarrage, et réécrire notre copie effacerait ses changements.
        // Fichier absent ou illisible : on garde la copie en mémoire.
        let r = match relire() {
            Some(neuf) => g.insert(neuf),
            None => g.get_or_insert_with(charger),
        };
        f(r);
        r.clone()
    };
    appliquer(&copie);
    enregistrer(&copie);
}

/// Effets immédiats de certains réglages
pub fn appliquer(r: &Reglages) {
    crate::sons::ACTIFS.store(r.sons, Ordering::Relaxed);
    ANIMATIONS.store(mouvement_local(r.animations, r.mouvement_reduit), Ordering::Relaxed);
    TRAME.store(r.trame, Ordering::Relaxed);
    REFLET.store(r.reflet, Ordering::Relaxed);
    YEUX_PIXEL.store(r.yeux_pixel, Ordering::Relaxed);
    PIXEL.store(r.pixel.rang() as u8, Ordering::Relaxed);
    ARBRE.store(r.arbre, Ordering::Relaxed);
    DATES_RELATIVES.store(r.dates_relatives, Ordering::Relaxed);
    ETAT_CLOUD.store(r.etat_cloud, Ordering::Relaxed);
    let propre = *LOCAL.lock().unwrap_or_else(|e| e.into_inner());
    COLONNES.store(propre.map_or(r.colonnes, |p| p.0), Ordering::Relaxed);
    TAILLE_LIGNES.store(propre.map_or(r.taille_lignes, |p| p.1), Ordering::Relaxed);
    EXTENSIONS.store(r.extensions, Ordering::Relaxed);
    VOLET_DENSE.store(r.volet_dense, Ordering::Relaxed);
    *RACCOURCIS.lock().unwrap_or_else(|e| e.into_inner()) = r.raccourcis.iter().filter_map(|x| x.split_once('=')).map(|(t, c)| (t.trim().to_uppercase(), c.trim().to_uppercase())).collect();
    relire_echelle_texte();
    let w = contraste_windows();
    CONTRASTE_WINDOWS.store(w, Ordering::Relaxed);
    CONTRASTE.store(r.contraste || w, Ordering::Relaxed);
    // Thème et mouvement dépendent des réglages ci-dessus : recalculés ici ; la fenêtre, elle, doit invalider et régler DWM
    // (`fenetre::rafraichir_apparence`, appelée après chaque changement de réglage)
    // Le thème effectif n'est PAS recalculé ici : il relit shared.json et le registre, et le faire hors de la fenêtre le ferait
    // changer sans que DWM et les menus suivent. `fenetre::rafraichir_apparence` le recalcule (après chaque réglage, au
    // démarrage dans main) ; la cible Direct2D se recrée de toute façon si `theme::clair()` change.
    crate::theme::fixer_choix(r.theme);
    rafraichir_mouvement();
    VOLET_REPLIE.store(r.volet_replie, Ordering::Relaxed);
    VOLET_MASQUE.store(r.volet_masque, Ordering::Relaxed);
    CHRONOLOGIE.store(r.chronologie, Ordering::Relaxed);
    POIDS_SONORE.store(r.poids_sonore, Ordering::Relaxed);
    EXPERIMENTAL.store(r.experimental, Ordering::Relaxed);
    EXPERIMENTAL_LU.store(true, Ordering::Relaxed);
    crate::glyphes::recharger(&r.glyphes);
}

/// Dossier exclu de la recherche, sous la forme gardée dans les réglages (minuscules, « \ » final)
pub fn forme_exclu(chemin: &str) -> String {
    let mut c = chemin.trim().to_lowercase();
    if !c.ends_with('\\') {
        c.push('\\');
    }
    c
}

pub const CONFLITS_DEFAUT: [&str; 5] = ["Demander à chaque fois", "Remplacer (ancien à la corbeille)", "Garder les deux", "Le plus récent", "Ignorer"];
pub const TAILLES_LIGNES: [&str; 2] = ["COMPACT", "CONFORT"];
/// La densité confort (40 DIP) est celle de la maquette ; toute valeur non nulle la représente,
/// ce qui garde les anciens réglages 36 / 40 lisibles sans troisième cran.
pub fn lignes_confort() -> bool {
    TAILLE_LIGNES.load(Ordering::Relaxed) != 0
}
pub const VUES: [&str; 3] = ["Liste", "Grille", "Colonnes"];

/// Colonnes cachées (bits) et date montrée dans la colonne DATE (bits 8-9)
pub const COL_DATE: u32 = 1;
pub const COL_TYPE: u32 = 2;
pub const COL_TAILLE: u32 = 4;
/// Colonnes en plus, cachées au départ : le bit les montre
pub const COL_DIMENSIONS: u32 = 16;
pub const COL_DUREE: u32 = 32;
pub const COL_AUTEUR: u32 = 64;
/// Origine Internet et signataire (V7) : 128 et 1024, car 256 et 512 sont la date montrée
pub const COL_ORIGINE: u32 = 128;
pub const COL_SIGNE: u32 = 1024;
pub fn colonne_montree(bit: u32) -> bool {
    COLONNES.load(Ordering::Relaxed) & bit != 0
}
/// Une colonne de propriétés (dimensions, durée, auteur) est-elle montrée ?
pub fn colonnes_meta() -> bool {
    colonne_montree(COL_DIMENSIONS | COL_DUREE | COL_AUTEUR | COL_ORIGINE | COL_SIGNE)
}
/// 0 : la plus récente (modifié ou arrivé), 1 : modification, 2 : création / arrivée
pub fn date_montree() -> u32 {
    (COLONNES.load(Ordering::Relaxed) >> 8) & 3
}
pub fn colonne_cachee(bit: u32) -> bool {
    COLONNES.load(Ordering::Relaxed) & bit != 0
}

/// Réglages lus à chaque dessin : sans verrou
pub static ANIMATIONS: AtomicBool = AtomicBool::new(true);
pub static ARBRE: AtomicBool = AtomicBool::new(false);

/// Réglage de l'app seul : « Apparition en points » ET mouvement « Complet ». Le mouvement « Réduit » se comporte comme
/// des animations désactivées ; Windows et le partage Eco s'ajoutent ensuite dans `animations_actives_avec`.
pub fn mouvement_local(animations: bool, reduit: bool) -> bool {
    animations && !reduit
}

/// Réglages d'apparence lus à chaque dessin : sans verrou
static TRAME: AtomicBool = AtomicBool::new(true);
static REFLET: AtomicBool = AtomicBool::new(true);
static YEUX_PIXEL: AtomicBool = AtomicBool::new(true);
static PIXEL: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(0);

/// Trame de points derrière la fenêtre ?
pub fn trame_active() -> bool {
    TRAME.load(Ordering::Relaxed)
}

/// Reflet au survol voulu par l'utilisateur (le mouvement et le contraste Windows le coupent aussi : `reflet::actif_maintenant`)
pub fn reflet_voulu() -> bool {
    REFLET.load(Ordering::Relaxed)
}

pub fn yeux_pixel_actifs() -> bool {
    YEUX_PIXEL.load(Ordering::Relaxed)
}

pub fn choix_pixel() -> crate::pixel::ChoixPixel {
    crate::pixel::ChoixPixel::depuis_rang(PIXEL.load(Ordering::Relaxed) as usize)
}

/// Précédence (phase 5 lot D) : la réduction la plus restrictive gagne. `Reduced` partagé éteint ; `Full` partagé, ou
/// aucun avis, n'allume jamais rien de force (ni contre Windows, ni contre le réglage local).
pub fn animations_actives_avec(local: bool, windows: bool, partage: Option<crate::eco::partage::Mouvement>) -> bool {
    local && windows && partage != Some(crate::eco::partage::Mouvement::Reduced)
}

/// Mouvement autorisé, en cache : réglage de l'app ET « Effets d'animation » de Windows ET mouvement partagé
/// (`shared.json`). Interroger Windows (`SystemParametersInfoW`) et le fichier à chaque image coûterait ; la valeur est
/// recalculée à la lecture des réglages (`appliquer`), au changement système (WM_SETTINGCHANGE) et à la relecture de
/// `shared.json` (`fenetre::rafraichir_apparence`, au plus un contrôle de fichier par seconde).
pub struct CacheMouvement(AtomicBool);

impl CacheMouvement {
    pub const fn nouveau() -> Self {
        Self(AtomicBool::new(true))
    }

    pub fn actif(&self) -> bool {
        self.0.load(Ordering::Relaxed)
    }

    /// Range la valeur de `animations_actives_avec` ; vrai si elle a changé
    pub fn fixer(&self, local: bool, windows: bool, partage: Option<crate::eco::partage::Mouvement>) -> bool {
        let v = animations_actives_avec(local, windows, partage);
        self.0.swap(v, Ordering::Relaxed) != v
    }
}

static MOUVEMENT: CacheMouvement = CacheMouvement::nouveau();

/// Les animations sont-elles permises ? Lecture d'un atomique : appelable à chaque image. Faux : tout effet est immédiat.
pub fn animations_actives() -> bool {
    MOUVEMENT.actif()
}

/// Recalcule le cache depuis le réglage de l'app, Windows et `shared.json` ; vrai si la valeur a changé
pub fn rafraichir_mouvement() -> bool {
    use windows::Win32::UI::WindowsAndMessaging::{SystemParametersInfoW, SPI_GETCLIENTAREAANIMATION, SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS};
    let mut systeme = windows::Win32::Foundation::BOOL(1);
    // En cas d'échec de la requête on garde « actif » : le réglage de l'app reste seul juge
    // SAFETY : `systeme` est un BOOL (4 octets) local, vivant et exclusif pendant tout l'appel ; SPI_GETCLIENTAREAANIMATION
    // écrit exactement un BOOL à cette adresse (uiParam 0, aucun autre tampon). Pas de handle, appelable depuis tout fil.
    unsafe {
        let _ = SystemParametersInfoW(SPI_GETCLIENTAREAANIMATION, 0, Some(&mut systeme as *mut _ as *mut _), SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0));
    }
    MOUVEMENT.fixer(ANIMATIONS.load(Ordering::Relaxed), systeme.as_bool(), crate::eco::partage::mouvement_partage())
}
pub static DATES_RELATIVES: AtomicBool = AtomicBool::new(false);
pub static ETAT_CLOUD: AtomicBool = AtomicBool::new(true);
pub static COLONNES: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(COL_TYPE);
pub static TAILLE_LIGNES: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(1);
pub static EXTENSIONS: AtomicBool = AtomicBool::new(true);
/// Contraste renforcé actif : réglage ou thème Contraste élevé de Windows (lu à chaque changement de réglage)
/// Raccourcis choisis (touche, commande), tout en majuscules : lus à chaque touche, sans recopier les réglages
static RACCOURCIS: Mutex<Vec<(String, String)>> = Mutex::new(Vec::new());

/// Commande (nom en majuscules) liée à cette combinaison de touches par l'utilisateur
pub fn commande_de_touche(combo: &str) -> Option<String> {
    RACCOURCIS.lock().unwrap_or_else(|e| e.into_inner()).iter().find(|(t, _)| t == combo).map(|(_, c)| c.clone())
}

/// Touche choisie pour cette commande (nom en majuscules), s'il y en a une
pub fn touche_de_commande(nom: &str) -> Option<String> {
    RACCOURCIS.lock().unwrap_or_else(|e| e.into_inner()).iter().find(|(_, c)| c == nom).map(|(t, _)| t.clone())
}

pub static VOLET_DENSE: AtomicBool = AtomicBool::new(false);
/// Mode capture (Z12) : noms des éléments choisis masqués. Pas enregistré : un partage d'écran ne doit pas
/// survivre au redémarrage sans qu'on le sache.
pub static MODE_CAPTURE: AtomicBool = AtomicBool::new(false);
pub static CONTRASTE: AtomicBool = AtomicBool::new(false);
/// Le thème Contraste élevé de Windows est actif : les couleurs du contraste renforcé viennent alors du système
pub static CONTRASTE_WINDOWS: AtomicBool = AtomicBool::new(false);

/// Thème « Contraste élevé » de Windows : bit 1 de `Flags` dans `Control Panel\Accessibility\HighContrast`
fn contraste_windows() -> bool {
    crate::defaut::lire_valeur(r"Control Panel\Accessibility\HighContrast", "Flags").and_then(|f| f.trim().parse::<u32>().ok()).is_some_and(|f| f & 1 != 0)
}

/// « Taille du texte » de Windows (Accessibilité), en centièmes : 100 à 225 dans le registre
static TEXTE_POUR_CENT: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(100);

/// Facteur appliqué aux polices du texte. shortcut: plafonné à 1,5 car les hauteurs de ligne sont fixes en DIP, à relever avec elles.
pub fn echelle_texte() -> f32 {
    TEXTE_POUR_CENT.load(Ordering::Relaxed).clamp(100, 150) as f32 / 100.0
}

/// Relit « Taille du texte » ; vrai si le facteur a changé (les polices sont alors à recréer)
pub fn relire_echelle_texte() -> bool {
    use windows::Win32::System::Registry::{RegGetValueW, HKEY_CURRENT_USER, RRF_RT_REG_DWORD};
    let mut v = 100u32;
    let mut taille = 4u32;
    // SAFETY: `v` fait 4 octets, taille annoncée 4
    let r = unsafe { RegGetValueW(HKEY_CURRENT_USER, &windows::core::HSTRING::from(r"Software\Microsoft\Accessibility"), &windows::core::HSTRING::from("TextScaleFactor"), RRF_RT_REG_DWORD, None, Some(&mut v as *mut u32 as *mut _), Some(&mut taille)) };
    if r.is_err() {
        v = 100;
    }
    TEXTE_POUR_CENT.swap(v, Ordering::Relaxed).clamp(100, 150) != v.clamp(100, 150)
}

/// Relit l'état du thème Contraste élevé (à chaque changement de réglage du système, pas seulement des nôtres) ; vrai s'il a changé
pub fn relire_contraste_windows() -> bool {
    let w = contraste_windows();
    let avant = CONTRASTE_WINDOWS.swap(w, Ordering::Relaxed);
    let r = relire().unwrap_or_default();
    CONTRASTE.store(r.contraste || w, Ordering::Relaxed);
    avant != w
}
pub static CHRONOLOGIE: AtomicBool = AtomicBool::new(false);
pub static POIDS_SONORE: AtomicBool = AtomicBool::new(false);
pub static VOLET_REPLIE: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
pub static VOLET_MASQUE: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

/// Réglages du fichier, tels qu'il est maintenant (None s'il est absent ou illisible)
fn relire() -> Option<Reglages> {
    let octets = std::fs::read(chemin_fichier()?).ok()?;
    // Lossy : un fichier retouché à la main en ANSI ne doit pas être pris pour un fichier vide,
    // puis écrasé par les valeurs par défaut au prochain enregistrement
    Some(lire(&String::from_utf8_lossy(&octets)))
}

fn charger() -> Reglages {
    let r = relire().unwrap_or_default();
    appliquer(&r);
    r
}

/// Nom du fichier écrit par l'export
pub const FICHIER_EXPORT: &str = "reglages-spacefiles.toml";

/// Copie les réglages dans `dossier` ; renvoie le fichier écrit. Les mots de passe n'y sont pas : ils sont dans le coffre
/// d'identifiants de Windows (stockages.rs), un stockage importé demandera donc à être reconnecté.
pub fn exporter(dossier: &std::path::Path) -> Result<std::path::PathBuf, String> {
    let c = dossier.join(FICHIER_EXPORT);
    std::fs::write(&c, ecrire(&actuels())).map_err(|e| format!("EXPORT IMPOSSIBLE : {e}"))?;
    Ok(c)
}

/// Remplace les réglages par ceux du fichier (écrit par l'export). Un fichier sans aucune ligne `clé = valeur` est refusé.
pub fn importer(fichier: &std::path::Path) -> Result<(), String> {
    let octets = std::fs::read(fichier).map_err(|e| format!("LECTURE IMPOSSIBLE : {e}"))?;
    let texte = String::from_utf8_lossy(&octets);
    if !texte.lines().any(|l| l.contains('=') && !l.trim_start().starts_with('#')) {
        return Err("CE FICHIER N'EST PAS UN FICHIER DE REGLAGES".into());
    }
    let neuf = lire(&texte);
    *REGLAGES.lock().unwrap_or_else(|e| e.into_inner()) = Some(neuf.clone());
    appliquer(&neuf);
    enregistrer(&neuf);
    Ok(())
}

pub fn reinitialiser() {
    modifier(|r| {
        let ancien = std::mem::take(r);
        // Les stockages, épingles, balises et récents restent : on ne perd pas ses données en remettant l'apparence à zéro
        *r = Reglages { stockages: ancien.stockages, epingles: ancien.epingles, balises: ancien.balises, recents: ancien.recents, tris: ancien.tris, vues: ancien.vues, dispositions: ancien.dispositions, groupes: ancien.groupes, membres: ancien.membres, regroupements: ancien.regroupements, sessions: ancien.sessions, regles: ancien.regles, sauvegarde_dest: ancien.sauvegarde_dest, sauvegarde: ancien.sauvegarde, reprises: ancien.reprises, non_fiables: ancien.non_fiables, notes: ancien.notes, glyphes: ancien.glyphes, experimental: ancien.experimental, ..Reglages::default() };
    });
}

fn enregistrer(r: &Reglages) {
    let Some(c) = chemin_fichier() else { return };
    if let Some(d) = c.parent() {
        let _ = std::fs::create_dir_all(d);
    }
    // Écrit à côté, forcé sur le disque (sync_all), puis remplace : après une coupure de courant,
    // le fichier est l'ancien ou le nouveau, jamais un fichier vide
    let tmp = c.with_extension("toml.tmp");
    let ecrit = std::fs::File::create(&tmp).and_then(|mut f| {
        use std::io::Write;
        f.write_all(ecrire(r).as_bytes())?;
        f.sync_all()
    });
    if ecrit.is_ok() {
        let _ = std::fs::rename(&tmp, &c);
    }
}

// ── Format ──

fn texte_toml(s: &str) -> String {
    let mut r = String::from("\"");
    for c in s.chars() {
        match c {
            '\\' => r.push_str("\\\\"),
            '"' => r.push_str("\\\""),
            '\n' => r.push_str("\\n"),
            '\t' => r.push_str("\\t"),
            c if (c as u32) < 0x20 => r.push_str(&format!("\\u{:04X}", c as u32)),
            c => r.push(c),
        }
    }
    r.push('"');
    r
}

fn ecrire(r: &Reglages) -> String {
    let mut s = String::from("# Réglages de SpaceFiles (modifiables ici ou avec Ctrl+, dans l'application)\n\n");
    s += &format!("depart = {}\n", texte_toml(r.depart.cle()));
    s += &format!("restaurer_onglets = {}\n", r.restaurer_onglets);
    s += &format!("fichiers_caches = {}\n", r.caches);
    s += &format!("dates_relatives = {}\n", r.dates_relatives);
    s += &format!("apercu = {}\n", r.apercu);
    s += &format!("animations = {}\n", r.animations);
    s += &format!("sons = {}\n", r.sons);
    s += &format!("un_clic = {}\n", r.un_clic);
    s += &format!("ressort_ms = {}\n", r.ressort_ms);
    s += &format!("nettoyage_jours = {}\n", r.nettoyage_jours);
    s += &format!("nettoyer_copies = {}\n", r.nettoyer_copies);
    s += &format!("arbre = {}\n", r.arbre);
    s += &format!("nettoyer_archives = {}\n", r.nettoyer_archives);
    s += &format!("ranger_auto = {}\n", r.ranger_auto);
    s += &format!("copie_verifiee = {}\n", r.copie_verifiee);
    s += &format!("telechargements_navigateur = {}\n", r.telechargements_navigateur);
    s += &format!("colonnes = {}\n", r.colonnes);
    s += &format!("etat_cloud = {}\n", r.etat_cloud);
    s += &format!("taille_lignes = {}\n", r.taille_lignes);
    s += &format!("extensions = {}\n", r.extensions);
    s += &format!("contraste = {}\n", r.contraste);
    s += &format!("theme = {}\n", texte_toml(r.theme.cle()));
    s += &format!("volet_dense = {}\n", r.volet_dense);
    s += &format!("mouvement = {}\n", texte_toml(if r.mouvement_reduit { "reduit" } else { "complet" }));
    s += &format!("trame = {}\n", r.trame);
    s += &format!("reflet = {}\n", r.reflet);
    s += &format!("yeux_pixel = {}\n", r.yeux_pixel);
    s += &format!("pixel = {}\n", texte_toml(r.pixel.cle()));
    s += &format!("conflit_defaut = {}\n", r.conflit_defaut);
    s += &format!("vue_defaut = {}\n", r.vue_defaut);
    s += &format!("volet_replie = {}\n", r.volet_replie);
    s += &format!("volet_masque = {}\n", r.volet_masque);
    s += &format!("retenir_recents = {}\n", r.retenir_recents);
    s += &format!("limite_reseau_mo = {}\n", r.limite_reseau_mo);
    s += &format!("chronologie = {}\n", r.chronologie);
    s += &format!("poids_sonore = {}\n", r.poids_sonore);
    s += "\n# Fonctions expérimentales : SFTP, WebDAV, sauvegarde NAS, MTP (peu testées)\n";
    s += &format!("experimental = {}\n", r.experimental);
    s += "\n# Session (écrit à la fermeture)\n";
    let onglets: Vec<String> = r.onglets.iter().map(|o| texte_toml(o)).collect();
    s += &format!("onglets = [{}]\n", onglets.join(", "));
    s += &format!("onglet_actif = {}\n", r.onglet_actif);
    s += &format!("dernier = {}\n", texte_toml(&r.dernier));
    let liste = |l: &[String]| l.iter().map(|o| texte_toml(o)).collect::<Vec<_>>().join(", ");
    s += &format!("recents = [{}]\n", liste(&r.recents));
    s += "\n# Dossiers épinglés et balises de couleur (r rouge, b blanc, g gris)\n";
    s += &format!("epingles = [{}]\n", liste(&r.epingles));
    s += &format!("balises = [{}]\n", liste(&r.balises));
    s += &format!("tris = [{}]\n", liste(&r.tris));
    s += &format!("vues = [{}]\n", liste(&r.vues));
    s += &format!("dispositions = [{}]\n", liste(&r.dispositions));
    s += &format!("regroupements = [{}]\n", liste(&r.regroupements));
    s += &format!("sessions = [{}]\n", liste(&r.sessions));
    s += &format!("regles = [{}]\n", liste(&r.regles));
    s += &format!("sauvegarde_dest = {}\n", texte_toml(&r.sauvegarde_dest));
    s += &format!("sauvegarde = [{}]\n", liste(&r.sauvegarde));
    s += &format!("raccourcis = [{}]\n", liste(&r.raccourcis));
    s += &format!("reprises = [{}]\n", liste(&r.reprises));
    s += &format!("non_fiables = [{}]\n", liste(&r.non_fiables));
    s += "\n# Recherche : dossiers et disques exclus de l'index\n";
    s += &format!("exclus = [{}]\n", liste(&r.exclus));
    s += "\n# Notes de dossier (chemin|texte)\n";
    s += &format!("notes = [{}]\n", liste(&r.notes));
    s += "\n# Glyphs de dossier (MOTIF|chemin)\n";
    s += &format!("glyphes = [{}]\n", liste(&r.glyphes));
    s += "\n# Groupes (couleur|NOM) et leurs éléments (NOM|chemin)\n";
    s += &format!("groupes = [{}]\n", liste(&r.groupes));
    s += &format!("membres = [{}]\n", liste(&r.membres));
    for st in &r.stockages {
        s += "\n[[stockage]]\n";
        s += &format!("nom = {}\n", texte_toml(&st.nom));
        s += &format!("sorte = {}\n", texte_toml(st.sorte.cle()));
        s += &format!("adresse = {}\n", texte_toml(&st.adresse));
        s += &format!("utilisateur = {}\n", texte_toml(&st.utilisateur));
    }
    s
}

#[derive(Debug, PartialEq)]
enum Valeur {
    Texte(String),
    Bool(bool),
    Nombre(i64),
    Liste(Vec<String>),
}

/// Lit un texte TOML entre guillemets à partir de `c` (après le guillemet ouvrant) ; renvoie le reste
fn lire_texte(c: &str) -> Option<(String, &str)> {
    let mut r = String::new();
    let mut it = c.char_indices();
    while let Some((i, ch)) = it.next() {
        match ch {
            '"' => return Some((r, &c[i + 1..])),
            '\\' => match it.next()?.1 {
                'n' => r.push('\n'),
                't' => r.push('\t'),
                'u' => {
                    let hex: String = (0..4).filter_map(|_| it.next().map(|x| x.1)).collect();
                    r.push(char::from_u32(u32::from_str_radix(&hex, 16).ok()?)?);
                }
                autre => r.push(autre),
            },
            ch => r.push(ch),
        }
    }
    None
}

fn lire_valeur(v: &str) -> Option<Valeur> {
    let v = v.trim();
    if let Some(reste) = v.strip_prefix('"') {
        return lire_texte(reste).map(|(t, _)| Valeur::Texte(t));
    }
    if let Some(mut reste) = v.strip_prefix('[') {
        let mut liste = Vec::new();
        loop {
            reste = reste.trim_start().trim_start_matches(',').trim_start();
            if reste.starts_with(']') || reste.is_empty() {
                return Some(Valeur::Liste(liste));
            }
            let (t, suite) = lire_texte(reste.strip_prefix('"')?)?;
            liste.push(t);
            reste = suite;
        }
    }
    match v {
        "true" => Some(Valeur::Bool(true)),
        "false" => Some(Valeur::Bool(false)),
        n => n.parse().ok().map(Valeur::Nombre),
    }
}

fn lire(texte: &str) -> Reglages {
    let mut r = Reglages::default();
    let mut stockage: Option<Stockage> = None;
    let mut cle_experimental = false;
    for ligne in texte.lines() {
        let ligne = ligne.trim();
        if ligne.is_empty() || ligne.starts_with('#') {
            continue;
        }
        if ligne == "[[stockage]]" {
            r.stockages.extend(stockage.take());
            stockage = Some(Stockage { nom: String::new(), sorte: Sorte::Dossier, adresse: String::new(), utilisateur: String::new() });
            continue;
        }
        let Some((cle, valeur)) = ligne.split_once('=') else { continue };
        let Some(valeur) = lire_valeur(valeur) else { continue };
        let cle = cle.trim();
        if let Some(st) = &mut stockage {
            if let Valeur::Texte(t) = valeur {
                match cle {
                    "nom" => st.nom = t,
                    "sorte" => st.sorte = Sorte::depuis(&t),
                    "adresse" => st.adresse = t,
                    "utilisateur" => st.utilisateur = t,
                    _ => {}
                }
            }
            continue;
        }
        match (cle, valeur) {
            ("depart", Valeur::Texte(t)) => r.depart = Depart::TOUS.into_iter().find(|d| d.cle() == t).unwrap_or(Depart::Personnel),
            ("restaurer_onglets", Valeur::Bool(b)) => r.restaurer_onglets = b,
            ("fichiers_caches", Valeur::Bool(b)) => r.caches = b,
            ("dates_relatives", Valeur::Bool(b)) => r.dates_relatives = b,
            ("apercu", Valeur::Bool(b)) => r.apercu = b,
            ("animations", Valeur::Bool(b)) => r.animations = b,
            ("sons", Valeur::Bool(b)) => r.sons = b,
            ("un_clic", Valeur::Bool(b)) => r.un_clic = b,
            ("ressort_ms", Valeur::Nombre(n)) => r.ressort_ms = n.clamp(0, 5000) as u32,
            ("nettoyage_jours", Valeur::Nombre(n)) => r.nettoyage_jours = n.clamp(0, 3650) as u32,
            ("nettoyer_copies", Valeur::Bool(b)) => r.nettoyer_copies = b,
            ("arbre", Valeur::Bool(b)) => r.arbre = b,
            // Alt+Espace n'est plus géré par SpaceFiles : la clé d'un ancien fichier est lue puis oubliée, jamais réécrite
            ("recherche_rapide", _) => {}
            ("nettoyer_archives", Valeur::Bool(b)) => r.nettoyer_archives = b,
            ("ranger_auto", Valeur::Bool(b)) => r.ranger_auto = b,
            ("copie_verifiee", Valeur::Bool(b)) => r.copie_verifiee = b,
            // Une valeur qui n'est pas un booléen tombe dans `_` : le défaut (vrai) reste
            ("telechargements_navigateur", Valeur::Bool(b)) => r.telechargements_navigateur = b,
            ("regles", Valeur::Liste(l)) => r.regles = l,
            ("sauvegarde_dest", Valeur::Texte(t)) => r.sauvegarde_dest = t,
            ("sauvegarde", Valeur::Liste(l)) => r.sauvegarde = l,
            ("raccourcis", Valeur::Liste(l)) => r.raccourcis = l,
            ("reprises", Valeur::Liste(l)) => r.reprises = l,
            ("non_fiables", Valeur::Liste(l)) => r.non_fiables = l,
            ("colonnes", Valeur::Nombre(n)) => r.colonnes = n.clamp(0, 2047) as u32,
            ("taille_lignes", Valeur::Nombre(n)) => r.taille_lignes = n.clamp(0, 1) as u32,
            ("extensions", Valeur::Bool(b)) => r.extensions = b,
            ("contraste", Valeur::Bool(b)) => r.contraste = b,
            ("theme", Valeur::Texte(t)) => r.theme = crate::theme::ChoixTheme::depuis_cle(&t),
            ("volet_dense", Valeur::Bool(b)) => r.volet_dense = b,
            // Valeur inconnue : le défaut (mouvement complet) reste
            ("mouvement", Valeur::Texte(t)) => r.mouvement_reduit = t == "reduit",
            ("trame", Valeur::Bool(b)) => r.trame = b,
            ("reflet", Valeur::Bool(b)) => r.reflet = b,
            ("yeux_pixel", Valeur::Bool(b)) => r.yeux_pixel = b,
            ("pixel", Valeur::Texte(t)) => r.pixel = crate::pixel::ChoixPixel::depuis_cle(&t),
            ("conflit_defaut", Valeur::Nombre(n)) => r.conflit_defaut = n.clamp(0, 4) as u32,
            ("vue_defaut", Valeur::Nombre(n)) => r.vue_defaut = n.clamp(0, 2) as u32,
            ("volet_replie", Valeur::Nombre(n)) => r.volet_replie = n.clamp(0, 63) as u32,
            ("volet_masque", Valeur::Nombre(n)) => r.volet_masque = n.clamp(0, 63) as u32,
            ("retenir_recents", Valeur::Bool(b)) => r.retenir_recents = b,
            ("limite_reseau_mo", Valeur::Nombre(n)) => r.limite_reseau_mo = n.clamp(0, 10_000) as u32,
            ("chronologie", Valeur::Bool(b)) => r.chronologie = b,
            ("exclus", Valeur::Liste(l)) => r.exclus = l,
            ("notes", Valeur::Liste(l)) => r.notes = l,
            ("glyphes", Valeur::Liste(l)) => r.glyphes = l,
            ("poids_sonore", Valeur::Bool(b)) => r.poids_sonore = b,
            ("experimental", Valeur::Bool(b)) => {
                r.experimental = b;
                cle_experimental = true;
            }
            ("etat_cloud", Valeur::Bool(b)) => r.etat_cloud = b,
            ("onglets", Valeur::Liste(l)) => r.onglets = l,
            ("onglet_actif", Valeur::Nombre(n)) => r.onglet_actif = n.max(0) as usize,
            ("dernier", Valeur::Texte(t)) => r.dernier = t,
            ("recents", Valeur::Liste(l)) => r.recents = l,
            ("epingles", Valeur::Liste(l)) => r.epingles = l,
            ("balises", Valeur::Liste(l)) => r.balises = l,
            ("tris", Valeur::Liste(l)) => r.tris = l,
            ("vues", Valeur::Liste(l)) => r.vues = l,
            ("dispositions", Valeur::Liste(l)) => r.dispositions = l,
            ("regroupements", Valeur::Liste(l)) => r.regroupements = l,
            ("sessions", Valeur::Liste(l)) => r.sessions = l,
            ("groupes", Valeur::Liste(l)) => r.groupes = l,
            ("membres", Valeur::Liste(l)) => r.membres = l,
            _ => {}
        }
    }
    r.stockages.extend(stockage);
    r.stockages.retain(|s| !s.adresse.is_empty());
    // Fichier d'avant le gel : un serveur SFTP / WebDAV déjà dans le volet ne doit pas disparaître. Une clé présente
    // (même fausse) est toujours respectée ; l'enregistrement suivant écrit la clé.
    if !cle_experimental && r.stockages.iter().any(|s| s.sorte.experimentale().is_some()) {
        r.experimental = true;
    }
    r
}

#[cfg(test)]
mod tests {
    /// Toutes les combinaisons local x Windows x partagé : seul un `Reduced` partagé éteint de plus, `Full` n'allume rien
    #[test]
    fn animations_avec_mouvement_partage() {
        use crate::eco::partage::Mouvement::{Full, Reduced};
        for local in [false, true] {
            for windows in [false, true] {
                assert_eq!(super::animations_actives_avec(local, windows, None), local && windows);
                assert_eq!(super::animations_actives_avec(local, windows, Some(Full)), local && windows, "Full n'allume jamais de force");
                assert!(!super::animations_actives_avec(local, windows, Some(Reduced)), "Reduced éteint");
            }
        }
    }

    /// Le cache rend exactement `animations_actives_avec` pour toutes les entrées (instance locale : aucun état global)
    #[test]
    fn cache_de_mouvement_comme_animations_actives_avec() {
        use crate::eco::partage::Mouvement::{Full, Reduced};
        let cache = CacheMouvement::nouveau();
        assert!(cache.actif(), "avant tout calcul : actif");
        for local in [false, true] {
            for windows in [false, true] {
                for partage in [None, Some(Full), Some(Reduced)] {
                    let attendu = animations_actives_avec(local, windows, partage);
                    let avant = cache.actif();
                    assert_eq!(cache.fixer(local, windows, partage), avant != attendu, "changement signalé seulement s'il y en a un");
                    assert_eq!(cache.actif(), attendu);
                }
            }
        }
    }

    /// `theme` : aller-retour, valeur inconnue ou absente = auto, et rien d'autre ne bouge
    #[test]
    fn theme_dans_le_fichier() {
        use crate::theme::ChoixTheme;
        for c in ChoixTheme::TOUS {
            let mut r = Reglages::default();
            r.theme = c;
            assert_eq!(lire(&ecrire(&r)).theme, c);
        }
        assert!(ecrire(&Reglages::default()).contains("theme = \"auto\""));
        assert_eq!(lire("theme = \"clair\"\nsons = false\n").theme, ChoixTheme::Clair);
        assert_eq!(lire("theme = \"violet\"\n").theme, ChoixTheme::Auto);
        assert_eq!(lire("theme = 3\n").theme, ChoixTheme::Auto);
        assert_eq!(lire("sons = false\n").theme, ChoixTheme::Auto);
        assert_eq!(lire("theme = \"CLAIR\"\n").theme, ChoixTheme::Clair);
        assert!(!lire("theme = \"clair\"\nsons = false\n").sons);
    }

    #[test]
    fn raccourcis_choisis() {
        let r = lire("raccourcis = [\"ctrl+maj+d=Doublons\"]
");
        appliquer(&r);
        assert_eq!(commande_de_touche("CTRL+MAJ+D").as_deref(), Some("DOUBLONS"));
        assert_eq!(touche_de_commande("DOUBLONS").as_deref(), Some("CTRL+MAJ+D"));
        assert_eq!(commande_de_touche("CTRL+D"), None);
        assert!(crate::palette::commande_nommee("doublons").is_some());
        appliquer(&Reglages::default());
        assert_eq!(commande_de_touche("CTRL+MAJ+D"), None);
    }

    /// Un fichier écrit avant le retrait du fond vivant, de la veille et du Glyph de la pastille garde ses
    /// clés mortes : elles sont ignorées, le reste se lit comme avant
    #[test]
    fn anciennes_cles_ignorees() {
        let r = lire("glyph = true
fond_vivant = true
veille_minutes = 15
sons = false
conflit_defaut = 2
");
        assert!(!r.sons);
        assert_eq!(r.conflit_defaut, 2);
        // Réécrit, le fichier ne les contient plus
        let ecrit = ecrire(&r);
        assert!(!ecrit.contains("fond_vivant") && !ecrit.contains("veille_minutes") && !ecrit.contains("glyph ="));
    }

    /// Un fichier écrit quand Alt+Espace existait encore contient `recherche_rapide` : il se charge sans erreur,
    /// le reste est lu, et la clé disparaît dès la réécriture
    #[test]
    fn cle_recherche_rapide_retiree_toleree() {
        for v in ["true", "false"] {
            let r = lire(&format!("recherche_rapide = {v}
sons = false
conflit_defaut = 3
"));
            assert!(!r.sons);
            assert_eq!(r.conflit_defaut, 3);
            assert!(!ecrire(&r).contains("recherche_rapide"));
        }
        // Mauvais type : toujours ignoré
        let r = lire("recherche_rapide = \"oui\"
sons = false
");
        assert!(!r.sons);
    }

    #[test]
    fn export_puis_import() {
        let d = std::env::temp_dir().join(format!("ne-export-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        let f = d.join(FICHIER_EXPORT);
        let mut r = Reglages::default();
        r.sons = !r.sons;
        r.conflit_defaut = 3;
        r.volet_dense = true;
        r.sauvegarde_dest = r"\\zima\Sauvegardes".into();
        r.sauvegarde = vec![r"C:\Essai".into()];
        r.epingles = vec![r"C:\Essai".into()];
        std::fs::write(&f, ecrire(&r)).unwrap();
        let relu = lire(&std::fs::read_to_string(&f).unwrap());
        assert_eq!(relu.sons, r.sons);
        assert_eq!((relu.conflit_defaut, relu.volet_dense), (3, true));
        assert_eq!(relu.epingles, r.epingles);
        assert_eq!((relu.sauvegarde_dest, relu.sauvegarde), (r.sauvegarde_dest.clone(), r.sauvegarde.clone()));
        std::fs::write(d.join("vide.toml"), "# rien
").unwrap();
        assert!(importer(&d.join("vide.toml")).is_err());
        assert!(importer(&d.join("absent.toml")).is_err());
        let _ = std::fs::remove_dir_all(&d);
    }

    use super::*;

    #[test]
    fn dossiers_non_fiables() {
        let l = vec![r"C:\Telechargements\suspect\".to_string()];
        assert!(est_sous(r"c:\telechargements\SUSPECT", &l));
        assert!(est_sous(r"C:\Telechargements\suspect\a\b", &l));
        // Un dossier au nom qui commence pareil n'est pas un sous-dossier
        assert!(!est_sous(r"C:\Telechargements\suspect2", &l));
        assert!(!est_sous(r"C:\Telechargements", &l));
    }

    #[test]
    fn aller_retour() {
        let mut r = Reglages::default();
        r.depart = Depart::Dernier;
        r.sons = false;
        r.ressort_ms = 1200;
        r.onglets = vec![r"C:\Users\utilisateur".into(), r#"D:\dossier "spécial""#.into()];
        r.dernier = r"\\zima\Media".into();
        r.epingles = vec![r"C:\projets".into()];
        r.vues = vec![r"G|C:\photos".into()];
        r.reprises = vec!["C:\\projets\tplan.md".into()];
        r.balises = vec![r"r|C:\a b\c.txt".into()];
        r.stockages.push(Stockage { nom: "ZIMA".into(), sorte: Sorte::Zima, adresse: r"\\10.0.1.5".into(), utilisateur: "nico".into() });
        r.stockages.push(Stockage { nom: "DRIVE".into(), sorte: Sorte::Dossier, adresse: r"G:\Mon Drive".into(), utilisateur: String::new() });
        let relu = lire(&ecrire(&r));
        assert_eq!(relu.depart, Depart::Dernier);
        assert!(!relu.sons);
        assert_eq!(relu.ressort_ms, 1200);
        assert_eq!(relu.onglets, r.onglets);
        assert_eq!(relu.dernier, r.dernier);
        assert_eq!(relu.epingles, r.epingles);
        assert_eq!(relu.reprises, r.reprises);
        assert_eq!(relu.vues, r.vues);
        assert_eq!(relu.balises, r.balises);
        assert_eq!(relu.stockages, r.stockages);
    }

    /// Lot 10B : les cinq clés s'écrivent et se relisent (valeurs non défaut), et un ancien fichier sans elles charge les défauts
    #[test]
    fn reglages_de_la_maquette_aller_retour() {
        let mut r = Reglages::default();
        r.mouvement_reduit = true;
        r.trame = false;
        r.reflet = false;
        r.yeux_pixel = false;
        r.pixel = crate::pixel::ChoixPixel::Nuit;
        let texte = ecrire(&r);
        for ligne in ["mouvement = \"reduit\"", "trame = false", "reflet = false", "yeux_pixel = false", "pixel = \"nuit\""] {
            assert!(texte.lines().any(|l| l == ligne), "ligne absente : {ligne}");
        }
        let relu = lire(&texte);
        assert!(relu.mouvement_reduit && !relu.trame && !relu.reflet && !relu.yeux_pixel);
        assert_eq!(relu.pixel, crate::pixel::ChoixPixel::Nuit);
        // Les défauts, eux, font aussi l'aller-retour
        let d = lire(&ecrire(&Reglages::default()));
        assert!(!d.mouvement_reduit && d.trame && d.reflet && d.yeux_pixel);
        assert_eq!(d.pixel, crate::pixel::ChoixPixel::Auto);
    }

    #[test]
    fn ancien_fichier_sans_les_cles_de_la_maquette() {
        let r = lire("depart = \"personnel\"\nsons = false\nanimations = true\n");
        assert!(!r.sons);
        assert!(!r.mouvement_reduit && r.trame && r.reflet && r.yeux_pixel);
        assert_eq!(r.pixel, crate::pixel::ChoixPixel::Auto);
        // Valeurs inconnues : les défauts
        let r = lire("mouvement = \"bizarre\"\npixel = \"minuit\"\ntrame = 3\n");
        assert!(!r.mouvement_reduit && r.trame);
        assert_eq!(r.pixel, crate::pixel::ChoixPixel::Auto);
    }

    /// Mouvement « Réduit » éteint tout, même avec Windows et le partage favorables ; « Complet » laisse Windows et le partage décider
    #[test]
    fn mouvement_reduit_coupe_les_animations() {
        use crate::eco::partage::Mouvement::Full;
        for animations in [false, true] {
            assert!(!animations_actives_avec(mouvement_local(animations, true), true, None), "réduit, animations {animations}");
            assert!(!animations_actives_avec(mouvement_local(animations, true), true, Some(Full)));
        }
        assert!(animations_actives_avec(mouvement_local(true, false), true, None), "complet : rien n'est coupé");
        assert!(!animations_actives_avec(mouvement_local(true, false), false, None), "complet mais Windows réduit : coupé");
        assert!(!animations_actives_avec(mouvement_local(false, false), true, None), "Apparition en points coupée : coupé");
        // Le reflet suit (reflet::actif reçoit ce résultat)
        assert!(!crate::reflet::actif(animations_actives_avec(mouvement_local(true, true), true, None), false));
    }

    #[test]
    fn fichier_abime() {
        let r = lire("depart = 12\nsons = peut-être\n[[stockage]]\nnom = \"sans adresse\"\n");
        assert_eq!(r.depart, Depart::Personnel);
        assert!(r.sons);
        assert!(r.stockages.is_empty());
    }

    /// `telechargements_navigateur` : vrai par défaut, absent ou illisible = vrai, aller-retour dans les deux sens
    #[test]
    fn telechargements_navigateur_lecture_et_aller_retour() {
        assert!(Reglages::default().telechargements_navigateur);
        assert!(lire("").telechargements_navigateur, "clé absente");
        assert!(lire("telechargements_navigateur = peut-être\n").telechargements_navigateur, "valeur inconnue");
        assert!(lire("telechargements_navigateur = \"non\"\n").telechargements_navigateur, "texte au lieu d'un booléen");
        assert!(lire("telechargements_navigateur = 0\n").telechargements_navigateur, "nombre au lieu d'un booléen");
        assert!(!lire("telechargements_navigateur = false\nsons = true\n").telechargements_navigateur);
        for b in [true, false] {
            let mut r = Reglages::default();
            r.telechargements_navigateur = b;
            let ecrit = ecrire(&r);
            assert!(ecrit.contains(&format!("telechargements_navigateur = {b}")));
            assert_eq!(lire(&ecrit).telechargements_navigateur, b);
        }
    }

    #[test]
    fn densite_deux_crans_et_type_masque_par_defaut() {
        let r = Reglages::default();
        assert_eq!(TAILLES_LIGNES.len(), 2);
        assert_eq!(r.taille_lignes, 1, "confort par défaut");
        assert_ne!(r.colonnes & COL_TYPE, 0, "TYPE masqué par défaut");
    }

    #[test]
    fn disposition_par_dossier() {
        assert_eq!(super::disposition(r"132|2|C:\Photos", r"c:\photos"), Some((132, 1)));
        assert_eq!(super::disposition(r"132|2|C:\Photos", r"C:\Autre"), None);
        // Valeurs hors bornes ramenées dans les limites (2 = ancienne aérée, désormais confort) ; ligne illisible ignorée
        assert_eq!(super::disposition(r"99999|7|D:\x", r"D:\x"), Some((2047, 1)));
        assert_eq!(super::disposition(r"abc|1|D:\x", r"D:\x"), None);
    }

    const SFTP: &str = "[[stockage]]\nnom = \"SERVEUR\"\nsorte = \"sftp\"\nadresse = \"nico@serveur/home\"\n";
    const WEBDAV: &str = "[[stockage]]\nnom = \"CLOUD\"\nsorte = \"webdav\"\nadresse = \"https://cloud.fr/dav\"\n";
    const SMB: &str = "[[stockage]]\nnom = \"NAS\"\nsorte = \"smb\"\nadresse = \"nas\"\n";

    #[test]
    fn fonction_experimentale_pure() {
        for g in [Experimentale::Sftp, Experimentale::WebDav, Experimentale::Sauvegarde, Experimentale::Mtp] {
            assert!(fonction_experimentale_permise(true, g), "{g:?} doit passer réglage vrai");
            assert!(!fonction_experimentale_permise(false, g), "{g:?} doit être fermé réglage faux");
        }
    }

    #[test]
    fn genres_de_stockage_filtres() {
        // SMB, ZimaOS et dossiers restent, réglage vrai comme faux
        for s in [Sorte::Smb, Sorte::Zima, Sorte::Dossier] {
            assert!(sorte_permise(false, s) && sorte_permise(true, s), "{s:?}");
        }
        for s in [Sorte::Sftp, Sorte::WebDav] {
            assert!(!sorte_permise(false, s) && sorte_permise(true, s), "{s:?}");
        }
    }

    #[test]
    fn formulaire_ne_propose_pas_sftp_ni_webdav_gele() {
        assert_eq!(Sorte::proposees(false), vec![Sorte::Zima, Sorte::Smb, Sorte::Dossier]);
        // Réglage vrai : la liste d'avant le gel, dans le même ordre
        assert_eq!(Sorte::proposees(true), Sorte::TOUTES.to_vec());
    }

    #[test]
    fn analyse_d_adresse() {
        assert_eq!(genre_adresse("sftp://nico@serveur/home"), Some(Experimentale::Sftp));
        assert_eq!(genre_adresse("  SFTP://serveur  "), Some(Experimentale::Sftp));
        assert_eq!(genre_adresse("\"sftp:\\\\serveur\\home\""), Some(Experimentale::Sftp));
        assert_eq!(genre_adresse("https://cloud.fr/remote.php/dav"), Some(Experimentale::WebDav));
        assert_eq!(genre_adresse("http://nas:8080/dav"), Some(Experimentale::WebDav));
        assert_eq!(genre_adresse(r"\\cloud.fr@SSL\remote.php\dav"), Some(Experimentale::WebDav));
        assert_eq!(genre_adresse(r"\\nas@8080\dav"), Some(Experimentale::WebDav));
        // Partages Windows, lecteurs et chemins locaux : jamais concernés
        for a in [r"\\nas\Media", r"\\192.168.0.20", r"\\zima\Media@2024", r"G:\Mon Drive", r"C:\Users\utilisateur", "ce pc", "facture"] {
            assert_eq!(genre_adresse(a), None, "{a}");
        }
    }

    #[test]
    fn adresse_permise_selon_le_reglage() {
        assert!(!adresse_permise(false, "sftp://serveur/a"));
        assert!(!adresse_permise(false, "https://cloud.fr/dav"));
        assert!(!adresse_permise(false, r"\\cloud.fr@SSL\dav"));
        assert!(adresse_permise(true, "sftp://serveur/a") && adresse_permise(true, "https://cloud.fr/dav"));
        // SMB : pas de changement, réglage faux
        assert!(adresse_permise(false, r"\\nas\Media") && adresse_permise(false, r"D:\Photos"));
    }

    #[test]
    fn profil_vide_experimental_faux() {
        assert!(!Reglages::default().experimental);
        assert!(!lire("").experimental);
        assert!(!lire("sons = false\n").experimental);
    }

    /// Fichier d'avant le gel : clé absente mais serveur SFTP / WebDAV présent, la valeur lue est vrai (rien ne disparaît)
    #[test]
    fn cle_absente_avec_stockage_experimental_lue_vraie() {
        for st in [SFTP, WEBDAV] {
            let r = lire(&format!("sons = false\n\n{st}"));
            assert!(r.experimental, "{st}");
            assert_eq!(r.stockages.len(), 1);
        }
        // Un stockage SMB seul ne change rien : défaut faux
        assert!(!lire(&format!("sons = false\n\n{SMB}")).experimental);
        // Plusieurs stockages dont un SFTP : vrai
        assert!(lire(&format!("{SMB}\n{SFTP}")).experimental);
    }

    #[test]
    fn cle_presente_toujours_respectee() {
        // Faux explicite malgré le serveur SFTP : le lieu est caché mais reste dans les réglages
        let r = lire(&format!("experimental = false\n\n{SFTP}"));
        assert!(!r.experimental);
        assert_eq!(r.stockages.len(), 1);
        let (cloud, serveurs) = crate::stockages::lieux_ajoutes(&r.stockages, r.experimental);
        assert!(cloud.is_empty() && serveurs.is_empty());
        // Vrai explicite sans aucun stockage
        assert!(lire("experimental = true\n").experimental);
    }

    #[test]
    fn experimental_aller_retour_et_stockages_conserves() {
        let mut r = Reglages::default();
        r.stockages.push(Stockage { nom: "SERVEUR".into(), sorte: Sorte::Sftp, adresse: "nico@serveur/home".into(), utilisateur: String::new() });
        for valeur in [false, true] {
            r.experimental = valeur;
            let ecrit = ecrire(&r);
            assert!(ecrit.contains(&format!("experimental = {valeur}\n")));
            let relu = lire(&ecrit);
            // La clé écrite l'emporte sur la règle « stockage SFTP présent » : faux reste faux
            assert_eq!(relu.experimental, valeur);
            assert_eq!(relu.stockages, r.stockages);
        }
    }

    #[test]
    fn fichier_ansi() {
        // Un fichier retouché en ANSI (é = 0xE9) ne doit pas être pris pour vide : les lignes valides restent
        let octets = b"sons = false\nnote = \"caf\xe9\"\n";
        let r = lire(&String::from_utf8_lossy(octets));
        assert!(!r.sons);
    }
}
