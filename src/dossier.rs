// Lecture rapide des dossiers et des disques, et tri

use std::cmp::Ordering;
use std::sync::atomic::{AtomicU64, Ordering as AtomicOrdering};
use std::sync::Arc;

use windows::core::PCWSTR;
use windows::Win32::Foundation::{GetLastError, ERROR_ACCESS_DENIED, ERROR_FILE_NOT_FOUND, ERROR_PATH_NOT_FOUND};
use windows::Win32::Storage::FileSystem::*;
use windows::Win32::UI::Shell::StrCmpLogicalW;

/// Où se trouve l'explorateur : la liste des disques, un dossier, les résultats d'une recherche,
/// les partages d'un serveur, ou la page des réglages
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Emplacement {
    CePC,
    /// Chemin Windows : `C:\` pour une racine, `C:\Users\utilisateur` sinon (sans `\` final), `\\serveur\partage\…`
    Dossier(String),
    /// Recherche de `motif` dans `racine` et tous ses sous-dossiers
    Recherche { racine: String, motif: String },
    /// Partages d'un serveur réseau (nom ou adresse, sans `\\`)
    Serveur(String),
    Reglages,
    /// Intérieur d'une archive (zip, 7z, rar…) : `interne` = `dossier/sous-dossier`, vide à la racine
    Archive { fichier: String, interne: String },
    /// Recherche dans tout le PC, par l'index (E17)
    Globale { motif: String },
    /// Groupe créé par l'utilisateur (groupes.rs) : ses éléments, d'où qu'ils viennent
    Groupe(String),
    /// Fichiers et dossiers ouverts récemment (ceux que Windows retient), regroupés par date
    Recents,
    /// Contenu de la Corbeille de l'utilisateur (corbeille.rs), le plus récemment supprimé d'abord
    Corbeille,
    /// Éléments du Panneau de configuration (panneau.rs)
    Panneau,
    /// Dossier d'un serveur SFTP (lecture seule) : `hote` = « utilisateur@serveur:port », `chemin` = « /a/b »
    Sftp { hote: String, chemin: String },
    /// Fichiers identiques sous ce dossier (doublons.rs), en sections par lot
    Doublons(String),
}

/// Recherche en cours : une recherche plus récente arrête les précédentes
static RECHERCHE: AtomicU64 = AtomicU64::new(0);
/// Au-delà, on arrête : la liste reste utilisable
const MAX_RESULTATS: usize = 20_000;

impl Emplacement {
    /// Fonction expérimentale que demande cet emplacement (serveur SFTP, adresse WebDAV), s'il y en a une
    pub fn experimentale(&self) -> Option<crate::reglages::Experimentale> {
        match self {
            Emplacement::Sftp { .. } => Some(crate::reglages::Experimentale::Sftp),
            Emplacement::Dossier(c) => crate::reglages::genre_adresse(c),
            Emplacement::Serveur(h) => crate::reglages::genre_adresse(&format!("\\\\{h}")),
            _ => None,
        }
    }

    /// `?motif` : une recherche enregistrée (« ? » ne peut pas figurer dans un chemin Windows), épinglée comme un dossier
    pub fn dossier(chemin: &str) -> Self {
        // Le réglage n'est lu que pour une adresse sftp:// : les autres chemins ne touchent pas aux réglages
        let sftp_permis = crate::reglages::genre_adresse(chemin) != Some(crate::reglages::Experimentale::Sftp) || crate::reglages::experimental();
        Self::dossier_avec(chemin, sftp_permis)
    }

    /// Comme `dossier`, avec le réglage donné. `sftp_permis` faux : `sftp://…` n'est plus reconnu comme un serveur,
    /// il devient un dossier introuvable (les épingles, récents et onglets mémorisés restent inertes, sans effacement).
    pub fn dossier_avec(chemin: &str, sftp_permis: bool) -> Self {
        if let Some(e) = chemin.trim().strip_prefix("shell:").and_then(dossier_shell) {
            return e;
        }
        if let Some(motif) = chemin.strip_prefix('?') {
            return Emplacement::Globale { motif: motif.to_string() };
        }
        if let Some((hote, chemin)) = crate::sftp::decouper(chemin).filter(|_| sftp_permis) {
            return Emplacement::Sftp { hote, chemin };
        }
        let mut c = chemin.trim().replace('/', "\\");
        while c.len() > 3 && c.ends_with('\\') {
            c.pop();
        }
        if c.len() == 2 && c.ends_with(':') {
            c.push('\\');
        }
        // `\\serveur` seul : la liste de ses partages
        if let Some(hote) = c.strip_prefix("\\\\") {
            if !hote.is_empty() && !hote.contains('\\') {
                return Emplacement::Serveur(hote.to_string());
            }
        }
        Emplacement::Dossier(c)
    }

    pub fn parent(&self) -> Option<Emplacement> {
        match self {
            Emplacement::CePC | Emplacement::Reglages => None,
            Emplacement::Globale { .. } => Some(Emplacement::CePC),
            Emplacement::Groupe(_) | Emplacement::Recents | Emplacement::Corbeille | Emplacement::Panneau => None,
            Emplacement::Doublons(r) => Some(Emplacement::dossier(r)),
            Emplacement::Sftp { hote, chemin } => {
                (chemin != "/").then(|| Emplacement::Sftp { hote: hote.clone(), chemin: crate::sftp::normaliser(chemin.rsplit_once('/').map_or("/", |x| x.0)) })
            }
            Emplacement::Archive { fichier, interne } => Some(match interne.rsplit_once('/') {
                Some((p, _)) => Emplacement::Archive { fichier: fichier.clone(), interne: p.to_string() },
                None if !interne.is_empty() => Emplacement::Archive { fichier: fichier.clone(), interne: String::new() },
                None => Emplacement::dossier(dossier_de(fichier)),
            }),
            Emplacement::Serveur(_) => Some(Emplacement::CePC),
            Emplacement::Recherche { racine, .. } => Some(Emplacement::dossier(racine)),
            Emplacement::Dossier(c) => {
                if c.len() <= 3 {
                    return Some(Emplacement::CePC);
                }
                let i = c.rfind('\\')?;
                // `\\serveur\partage` remonte au serveur (dossier() reconnaît `\\serveur`)
                Some(Emplacement::dossier(&c[..i.max(2)]))
            }
        }
    }

    /// Nom du dernier élément du chemin (pour resélectionner un dossier en remontant)
    pub fn nom(&self) -> Option<&str> {
        match self {
            Emplacement::Dossier(c) if c.len() > 3 => c.rsplit('\\').next(),
            // En sortant d'une archive, c'est elle qui est resélectionnée
            Emplacement::Archive { fichier, interne } if interne.is_empty() => fichier.rsplit('\\').next(),
            Emplacement::Archive { interne, .. } => interne.rsplit('/').next(),
            Emplacement::Sftp { chemin, .. } if chemin.len() > 1 => chemin.rsplit('/').next(),
            _ => None,
        }
    }

    /// Libellé court (onglets)
    pub fn titre(&self) -> String {
        match self {
            Emplacement::CePC => "CE PC".into(),
            Emplacement::Reglages => "RÉGLAGES".into(),
            Emplacement::Serveur(h) => nom_hote(h),
            Emplacement::Recherche { motif, .. } => format!("« {} »", motif.to_uppercase()),
            Emplacement::Globale { motif } => format!("PC « {} »", motif.to_uppercase()),
            Emplacement::Groupe(n) => n.clone(),
            Emplacement::Recents => "RÉCENTS".into(),
            Emplacement::Corbeille => "CORBEILLE".into(),
            Emplacement::Panneau => "PANNEAU DE CONFIGURATION".into(),
            Emplacement::Doublons(r) => format!("DOUBLONS · {}", nom_affiche(r).to_uppercase()),
            Emplacement::Sftp { hote, .. } => self.nom().map(str::to_uppercase).unwrap_or_else(|| hote.to_uppercase()),
            Emplacement::Archive { .. } => self.nom().unwrap_or("ARCHIVE").to_uppercase(),
            Emplacement::Dossier(c) => match crate::systeme::nom_dossier_connu(c) {
                Some(n) => n.to_string(),
                None => self.nom().unwrap_or(c.trim_end_matches('\\')).to_uppercase(),
            },
        }
    }

    pub fn enfant(&self, e: &Entree) -> Emplacement {
        match (&e.genre, self) {
            (Genre::Disque { racine, .. }, _) => Emplacement::dossier(racine),
            (_, Emplacement::Sftp { hote, chemin }) => Emplacement::Sftp { hote: hote.clone(), chemin: crate::sftp::joindre(chemin, &e.nom) },
            (_, Emplacement::Archive { fichier, interne }) => Emplacement::Archive {
                fichier: fichier.clone(),
                interne: if interne.is_empty() { e.nom.clone() } else { format!("{interne}/{}", e.nom) },
            },
            (_, _) if e.parent.is_some() => Emplacement::dossier(&joindre(e.parent.as_deref().unwrap_or(""), &e.nom)),
            (_, Emplacement::Dossier(c)) if c.ends_with('\\') => Emplacement::dossier(&format!("{c}{}", e.nom)),
            (_, Emplacement::Dossier(c)) => Emplacement::dossier(&format!("{c}\\{}", e.nom)),
            (_, _) => self.clone(),
        }
    }

    /// Segments du fil d'Ariane : texte affiché et emplacement visé
    pub fn segments(&self) -> Vec<(String, Emplacement)> {
        let chemin = match self {
            Emplacement::Dossier(c) => Some(c),
            Emplacement::Recherche { racine, .. } => Some(racine),
            Emplacement::Serveur(h) => return vec![(nom_hote(h), self.clone())],
            Emplacement::Reglages => return vec![("RÉGLAGES".into(), Emplacement::Reglages)],
            Emplacement::Groupe(n) => return vec![("GROUPES".into(), self.clone()), (n.clone(), self.clone())],
            Emplacement::Recents => return vec![("RÉCENTS".into(), self.clone())],
            Emplacement::Corbeille => return vec![("CORBEILLE".into(), self.clone())],
            Emplacement::Panneau => return vec![("PANNEAU DE CONFIGURATION".into(), self.clone())],
            Emplacement::Doublons(r) => return vec![(nom_affiche(r).to_uppercase(), Emplacement::dossier(r)), ("DOUBLONS".into(), self.clone())],
            Emplacement::Sftp { hote, chemin } => {
                let mut v = vec![(hote.to_uppercase(), Emplacement::Sftp { hote: hote.clone(), chemin: "/".into() })];
                let mut cumul = String::new();
                for m in chemin.split('/').filter(|m| !m.is_empty()) {
                    cumul = format!("{cumul}/{m}");
                    v.push((m.to_uppercase(), Emplacement::Sftp { hote: hote.clone(), chemin: cumul.clone() }));
                }
                return v;
            }
            Emplacement::Globale { motif } => return vec![("CE PC".into(), Emplacement::CePC), (format!("« {} »", motif.to_uppercase()), self.clone())],
            Emplacement::Archive { fichier, interne } => {
                // Le dossier de l'archive, l'archive, puis ses dossiers internes
                let racine = Emplacement::Archive { fichier: fichier.clone(), interne: String::new() };
                let mut v = Emplacement::dossier(dossier_de(fichier)).segments();
                v.push((fichier.rsplit('\\').next().unwrap_or(fichier).to_uppercase(), racine));
                let mut cumul = String::new();
                for m in interne.split('/').filter(|m| !m.is_empty()) {
                    cumul = if cumul.is_empty() { m.to_string() } else { format!("{cumul}/{m}") };
                    v.push((m.to_uppercase(), Emplacement::Archive { fichier: fichier.clone(), interne: cumul.clone() }));
                }
                return v;
            }
            Emplacement::CePC => None,
        };
        // Chemin réseau : le serveur, puis le partage et les dossiers
        if let Some(reste) = chemin.and_then(|c| c.strip_prefix("\\\\")) {
            let mut morceaux = reste.split('\\').filter(|m| !m.is_empty());
            let hote = morceaux.next().unwrap_or_default();
            let mut v = vec![(nom_hote(hote), Emplacement::Serveur(hote.to_string()))];
            let mut cumul = format!("\\\\{hote}");
            for m in morceaux {
                cumul = format!("{cumul}\\{m}");
                v.push((m.to_uppercase(), Emplacement::dossier(&cumul)));
            }
            if let Emplacement::Recherche { motif, .. } = self {
                v.push((format!("« {} »", motif.to_uppercase()), self.clone()));
            }
            return v;
        }
        let mut v = vec![("CE PC".to_string(), Emplacement::CePC)];
        if let Some(c) = chemin {
            let mut cumul = String::new();
            for morceau in c.split('\\').filter(|m| !m.is_empty()) {
                if cumul.is_empty() {
                    cumul = format!("{morceau}\\");
                } else if cumul.ends_with('\\') {
                    cumul.push_str(morceau);
                } else {
                    cumul = format!("{cumul}\\{morceau}");
                }
                let nom = crate::systeme::nom_dossier_connu(&cumul).map_or_else(|| morceau.to_uppercase(), str::to_string);
                v.push((nom, Emplacement::dossier(&cumul)));
            }
        }
        if let Emplacement::Recherche { motif, .. } = self {
            v.push((format!("« {} »", motif.to_uppercase()), self.clone()));
        }
        v
    }
}

/// Nom d'un serveur pour l'affichage : `cloud.exemple.fr@SSL` (WebDAV) devient `CLOUD.EXEMPLE.FR`
pub fn nom_hote(h: &str) -> String {
    h.split('@').next().unwrap_or(h).to_uppercase()
}

pub fn joindre(dossier: &str, nom: &str) -> String {
    if dossier.ends_with('\\') {
        format!("{dossier}{nom}")
    } else {
        format!("{dossier}\\{nom}")
    }
}

#[derive(Clone, Debug)]
pub enum Genre {
    Dossier,
    Fichier,
    Disque { racine: String, libre: u64, total: u64 },
}

/// Fichier d'un dossier synchronisé (OneDrive, Google Drive en mode dossier, Dropbox…)
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Nuage {
    /// Fichier ordinaire
    #[default]
    Aucun,
    /// Seulement en ligne : téléchargé à l'ouverture
    EnLigne,
    /// Présent sur le PC (peut être libéré)
    Local,
    /// « Toujours garder sur cet appareil »
    Epingle,
}

impl Nuage {
    /// Mot d'état en casse normale (maquette v6 : les mots d'état sont en Segoe, pas en capitales)
    pub fn libelle(self) -> &'static str {
        match self {
            Nuage::Aucun => "",
            Nuage::EnLigne => "En ligne seulement",
            Nuage::Local => "Sur ce PC",
            Nuage::Epingle => "Toujours sur ce PC",
        }
    }
}

/// Taille en casse normale pour un mot d'état (« 28 Go ») : `systeme::taille` rend des capitales (« 28,0 GO ») pour les colonnes
fn taille_normale(o: u64) -> String {
    let t = crate::systeme::taille(o);
    let (nombre, unite) = t.split_once(' ').unwrap_or((&t, ""));
    let nombre = nombre.strip_suffix(",0").unwrap_or(nombre);
    let unite = match unite {
        "O" => "o",
        "KO" => "Ko",
        "MO" => "Mo",
        "GO" => "Go",
        "TO" => "To",
        autre => autre,
    };
    format!("{nombre} {unite}")
}

/// Mot d'état d'un disque dans la liste de Ce PC, en casse normale. Presque plein : « Presque plein · 28 Go libres » si `tient`
/// l'accepte, sinon « Presque plein », sinon « Plein ». `tient` mesure un texte dans la colonne (Segoe 12).
pub fn etiquette_disque(presque_plein: bool, partage: bool, libre: u64, tient: &dyn Fn(&str) -> bool) -> String {
    if !presque_plein {
        return if partage { "Partage" } else { "Disque" }.to_string();
    }
    let long = format!("Presque plein · {} libres", taille_normale(libre));
    if tient(&long) {
        long
    } else if tient("Presque plein") {
        "Presque plein".to_string()
    } else {
        "Plein".to_string()
    }
}

/// Place d'un disque dans la carte du volet, en casse normale : « 181 Go libres sur 476 Go » (maquette v6)
pub fn libres_sur(libre: u64, total: u64) -> String {
    format!("{} libres sur {}", taille_normale(libre), taille_normale(total))
}

const ATTR_EPINGLE: u32 = 0x0008_0000; // FILE_ATTRIBUTE_PINNED
const ATTR_RAPPEL_OUVERTURE: u32 = 0x0004_0000; // FILE_ATTRIBUTE_RECALL_ON_OPEN
const ATTR_RAPPEL_ACCES: u32 = 0x0040_0000; // FILE_ATTRIBUTE_RECALL_ON_DATA_ACCESS
const ATTR_HORS_LIGNE: u32 = 0x0000_1000; // FILE_ATTRIBUTE_OFFLINE

/// Étiquettes de point d'analyse des fichiers cloud : IO_REPARSE_TAG_CLOUD, CLOUD_1 … CLOUD_F
fn etiquette_cloud(tag: u32) -> bool {
    tag & 0xFFFF_0FFF == 0x9000_001A
}

fn etat_nuage(attr: u32, tag: u32) -> Nuage {
    let point = attr & FILE_ATTRIBUTE_REPARSE_POINT.0 != 0;
    if attr & ATTR_EPINGLE != 0 {
        Nuage::Epingle
    } else if attr & (ATTR_RAPPEL_ACCES | ATTR_RAPPEL_OUVERTURE | ATTR_HORS_LIGNE) != 0 {
        Nuage::EnLigne
    } else if point && etiquette_cloud(tag) {
        Nuage::Local
    } else {
        Nuage::Aucun
    }
}

#[derive(Clone, Debug)]
pub struct Entree {
    pub nom: String,
    /// Nom en UTF-16 terminé par 0 : pour le tri naturel et l'affichage sans conversion
    pub nom_w: Vec<u16>,
    pub genre: Genre,
    pub taille: u64,
    /// Date de modification (FILETIME, en centaines de nanosecondes)
    pub modif: u64,
    /// Date d'arrivée dans le dossier (création : téléchargement, copie, extraction)
    pub ajout: u64,
    /// Extension en majuscules, sans le point
    pub ext: String,
    pub cache: bool,
    /// Dossier de l'élément, pour les résultats de recherche (sinon c'est le dossier ouvert)
    pub parent: Option<Arc<str>>,
    /// Jonction ou lien symbolique : la recherche n'y entre pas (évite les boucles)
    pub lien: bool,
    pub nuage: Nuage,
    /// Dimensions, durée, auteur : lus en arrière-plan quand ces colonnes sont montrées
    pub meta: Option<Arc<Meta>>,
}

/// Propriétés lues dans le fichier (images, vidéos, musique, documents)
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Meta {
    pub largeur: u32,
    pub hauteur: u32,
    /// Durée en centaines de nanosecondes
    pub duree: u64,
    pub auteur: String,
    /// Domaine d'où vient le fichier (flux Zone.Identifier), ou « INTERNET » ; vide sinon (V7)
    pub origine: String,
    /// Signataire d'un exécutable, « NON SIGNE » ou « INVALIDE » ; vide pour les autres fichiers (V7)
    pub signature: String,
}

impl Meta {
    pub fn dimensions(&self) -> String {
        if self.largeur == 0 { String::new() } else { format!("{} × {}", self.largeur, self.hauteur) }
    }

    pub fn duree_texte(&self) -> String {
        let s = self.duree / 10_000_000;
        match s {
            0 => String::new(),
            s if s >= 3600 => format!("{}:{:02}:{:02}", s / 3600, s / 60 % 60, s % 60),
            s => format!("{}:{:02}", s / 60, s % 60),
        }
    }
}

impl Entree {
    pub fn est_conteneur(&self) -> bool {
        !matches!(self.genre, Genre::Fichier)
    }

    /// Dernier changement : modifié ou arrivé dans le dossier (un vieux fichier tout juste téléchargé est récent)
    pub fn recente(&self) -> u64 {
        self.modif.max(self.ajout)
    }

    /// Date de la colonne DATE, selon le choix fait sur l'en-tête
    pub fn date_colonne(&self) -> u64 {
        match crate::reglages::date_montree() {
            1 => self.modif,
            2 if self.ajout > 0 => self.ajout,
            _ => self.recente(),
        }
    }
}

/// Regrouper l'affichage (comme Files) : sections repliables avec leur nombre d'éléments
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Regroupement {
    Aucun,
    Date,
    Type,
    Taille,
    Nom,
    Groupe,
    /// Quarantaine (Z1) : venus d'Internet, puis programmes non signés, puis le reste
    Origine,
    /// Lots de fichiers identiques (vue des doublons seulement)
    Doublon,
}

impl Regroupement {
    pub const TOUS: [Regroupement; 7] = [Regroupement::Aucun, Regroupement::Date, Regroupement::Type, Regroupement::Taille, Regroupement::Nom, Regroupement::Groupe, Regroupement::Origine];

    pub fn libelle(self) -> &'static str {
        match self {
            Regroupement::Aucun => "SANS REGROUPEMENT",
            Regroupement::Date => "DATE",
            Regroupement::Type => "TYPE",
            Regroupement::Taille => "TAILLE",
            Regroupement::Nom => "NOM (LETTRE)",
            Regroupement::Groupe => "GROUPE",
            Regroupement::Origine => "ORIGINE (INTERNET, NON SIGNE)",
            Regroupement::Doublon => "DOUBLONS",
        }
    }

    pub fn lettre(self) -> char {
        match self {
            Regroupement::Aucun => '-',
            Regroupement::Date => 'D',
            Regroupement::Type => 'T',
            Regroupement::Taille => 'S',
            Regroupement::Nom => 'N',
            Regroupement::Groupe => 'G',
            Regroupement::Origine => 'O',
            Regroupement::Doublon => 'X',
        }
    }

    pub fn depuis(c: char) -> Regroupement {
        Regroupement::TOUS.into_iter().find(|r| r.lettre() == c).unwrap_or(Regroupement::Aucun)
    }
}

/// Quarantaine : l'origine et la signature viennent des propriétés lues en fond, d'où « ANALYSE EN COURS » au début
fn section_origine(e: &Entree) -> (u32, String) {
    if e.est_conteneur() {
        return (3, "DOSSIERS".into());
    }
    // Un fichier seulement en ligne n'est jamais lu (le lire le téléchargerait)
    if e.nuage == Nuage::EnLigne {
        return (2, "LE RESTE".into());
    }
    match e.meta.as_deref() {
        None => (4, "ANALYSE EN COURS".into()),
        Some(m) if !m.origine.is_empty() => (0, "VENU D'INTERNET".into()),
        Some(m) if matches!(m.signature.as_str(), "NON SIGNE" | "SIGNATURE INVALIDE" | "NON VERIFIE") => (1, "PROGRAMMES A VERIFIER".into()),
        Some(_) => (2, "LE RESTE".into()),
    }
}

/// Section d'un élément : (ordre, titre) — les sections sont rangées par ordre puis par titre
pub fn section(e: &Entree, mode: Regroupement, chemin: &str) -> (u32, String) {
    let conteneur = e.est_conteneur();
    match mode {
        Regroupement::Aucun => (0, String::new()),
        Regroupement::Doublon => crate::doublons::section(e),
        Regroupement::Date => {
            let t = crate::systeme::tranche_date(e.date_colonne());
            (t, crate::systeme::TRANCHES_DATE[t as usize].to_string())
        }
        Regroupement::Type if conteneur => (0, "DOSSIERS".into()),
        Regroupement::Type => (1, crate::icones::nom_type(e).to_uppercase()),
        Regroupement::Taille if conteneur => (0, "DOSSIERS".into()),
        Regroupement::Taille => {
            const KO: u64 = 1024;
            match e.taille {
                0 => (1, "VIDES".into()),
                t if t < 16 * KO => (2, "MINUSCULES · MOINS DE 16 KO".into()),
                t if t < KO * KO => (3, "PETITS · MOINS DE 1 MO".into()),
                t if t < 128 * KO * KO => (4, "MOYENS · MOINS DE 128 MO".into()),
                t if t < KO * KO * KO => (5, "GRANDS · MOINS DE 1 GO".into()),
                _ => (6, "ENORMES · PLUS DE 1 GO".into()),
            }
        }
        Regroupement::Nom => {
            let c = e.nom.chars().find(|c| c.is_alphanumeric()).map_or('#', |c| {
                match c.to_uppercase().next().unwrap_or(c) {
                    'À' | 'Â' | 'Ä' | 'Á' => 'A',
                    'É' | 'È' | 'Ê' | 'Ë' => 'E',
                    'Î' | 'Ï' | 'Í' => 'I',
                    'Ô' | 'Ö' | 'Ó' => 'O',
                    'Ù' | 'Û' | 'Ü' | 'Ú' => 'U',
                    'Ç' => 'C',
                    x => x,
                }
            });
            if c.is_ascii_digit() {
                (0, "0-9".into())
            } else if c.is_ascii_uppercase() {
                (1 + c as u32, c.to_string())
            } else {
                (200, "#".into())
            }
        }
        Regroupement::Origine => section_origine(e),
        Regroupement::Groupe => {
            let complet = match &e.parent {
                Some(p) => joindre(p, &e.nom),
                None => joindre(chemin, &e.nom),
            };
            match crate::groupes::de(&complet).into_iter().next() {
                Some(g) => (0, g),
                None => (1, "SANS GROUPE".into()),
            }
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Colonne {
    Nom,
    Date,
    Type,
    Taille,
    Dimensions,
    Duree,
    Auteur,
    Origine,
    Signature,
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// Lit un emplacement. Appelée hors du fil de l'interface.
pub fn lire(emp: &Emplacement, caches: bool) -> Result<Vec<Entree>, String> {
    match emp {
        Emplacement::CePC => Ok(lire_disques()),
        Emplacement::Dossier(c) => {
            let mut v = Vec::with_capacity(256);
            parcourir(c, caches, |e| v.push(e))?;
            Ok(v)
        }
        Emplacement::Recherche { racine, motif } => Ok(rechercher(racine, motif, caches)),
        Emplacement::Serveur(h) => crate::stockages::partages(h),
        Emplacement::Reglages => Ok(Vec::new()),
        Emplacement::Archive { fichier, interne } => crate::archive::lire(fichier, interne),
        Emplacement::Globale { motif } => crate::index::chercher_en_attendant(motif),
        Emplacement::Groupe(n) => Ok(crate::groupes::lire(n)),
        Emplacement::Recents => Ok(crate::systeme::recents()),
        Emplacement::Corbeille => Ok(crate::corbeille::lire()),
        Emplacement::Panneau => crate::panneau::lire(),
        Emplacement::Doublons(r) => crate::doublons::chercher(r),
        Emplacement::Sftp { hote, chemin } => crate::sftp::lire(hote, chemin),
    }
}

/// Dossier qui contient ce fichier : `C:\a\b.zip` → `C:\a`, `C:\b.zip` → `C:\`
pub fn dossier_de(fichier: &str) -> &str {
    match fichier.rsplit_once('\\') {
        Some((d, _)) if d.len() == 2 && d.ends_with(':') => &fichier[..3],
        Some((d, _)) => d,
        None => fichier,
    }
}

/// Nom d'un dossier pour en nommer une archive : `C:\Users\nico` → `nico`, `D:\` → `D`
/// Dossiers « shell: » les plus utiles, ouverts comme des dossiers ordinaires (le reste reste confié à l'Explorateur)
pub fn dossier_shell(nom: &str) -> Option<Emplacement> {
    let var = |v: &str| std::env::var(v).ok();
    let (appdata, profil, donnees) = (var("APPDATA")?, var("USERPROFILE")?, var("ProgramData")?);
    let menu = format!(r"{appdata}\Microsoft\Windows\Start Menu");
    let chemin = match nom.trim().to_lowercase().as_str() {
        "recyclebinfolder" => return Some(Emplacement::Corbeille),
        "controlpanelfolder" => return Some(Emplacement::Panneau),
        "startup" => format!(r"{menu}\Programs\Startup"),
        "common startup" => format!(r"{donnees}\Microsoft\Windows\Start Menu\Programs\StartUp"),
        "start menu" => menu,
        "programs" => format!(r"{menu}\Programs"),
        "sendto" => format!(r"{appdata}\Microsoft\Windows\SendTo"),
        "recent" => format!(r"{appdata}\Microsoft\Windows\Recent"),
        "templates" => format!(r"{appdata}\Microsoft\Windows\Templates"),
        "appdata" => appdata,
        "local appdata" => var("LOCALAPPDATA")?,
        "common appdata" => donnees,
        "profile" => profil,
        "personal" => format!(r"{profil}\Documents"),
        "my pictures" => format!(r"{profil}\Pictures"),
        "my music" => format!(r"{profil}\Music"),
        "my video" => format!(r"{profil}\Videos"),
        "desktop" => format!(r"{profil}\Desktop"),
        "downloads" => format!(r"{profil}\Downloads"),
        "fonts" => format!(r"{}\Fonts", var("WINDIR")?),
        _ => return None,
    };
    Some(Emplacement::Dossier(chemin))
}

/// Décode `%XX` et `+` d'un morceau d'adresse
fn decoder_url(s: &str) -> String {
    let o = s.as_bytes();
    let mut v = Vec::with_capacity(o.len());
    let mut i = 0;
    while i < o.len() {
        match o[i] {
            b'%' if s.get(i + 1..i + 3).is_some_and(|h| u8::from_str_radix(h, 16).is_ok()) => {
                v.push(u8::from_str_radix(&s[i + 1..i + 3], 16).unwrap());
                i += 3;
            }
            b'+' => {
                v.push(b' ');
                i += 1;
            }
            c => {
                v.push(c);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&v).into_owned()
}

/// Alias de protocole d'avant le renommage, toujours reconnu (des liens existants le portent)
const PROTOCOLE_ANCIEN: &str = "nothingexplorer";
const PROTOCOLE: &str = "spacefiles";

/// Adresse `spacefiles://open?path=…`, `…/search?q=…` ou `…/group?name=…` (ou la même avec l'alias
/// `nothingexplorer://`) : où aller, et l'élément à choisir.
/// Navigation seule : une adresse venue d'une page web ne lance ni n'efface jamais rien.
pub fn emplacement_depuis_url(url: &str) -> Option<(Emplacement, Option<String>)> {
    let url = url.trim();
    let longueur = [PROTOCOLE, PROTOCOLE_ANCIEN].into_iter().find(|n| url.get(..n.len()).is_some_and(|p| p.eq_ignore_ascii_case(n)))?.len();
    let reste = &url[longueur..];
    let reste = reste.strip_prefix(':')?.trim_start_matches('/');
    // Forme courte : spacefiles://C:\chemin
    if crate::outils::lettre_lecteur(reste).is_some() {
        return Some((Emplacement::dossier(&decoder_url(reste)), None));
    }
    let (action, requete) = reste.split_once('?').unwrap_or((reste, ""));
    let champ = |nom: &str| requete.split('&').find_map(|p| p.strip_prefix(nom)?.strip_prefix('=')).map(decoder_url).filter(|v| !v.is_empty());
    match action.trim_end_matches('/') {
        "open" => {
            let p = champ("path")?;
            // Un fichier : son dossier s'ouvre et il est choisi
            if std::path::Path::new(&p).is_file() {
                let nom = p.rsplit('\\').next().map(str::to_string);
                return Some((Emplacement::dossier(dossier_de(&p)), nom));
            }
            Some((Emplacement::dossier(&p), None))
        }
        "search" => Some((Emplacement::Globale { motif: champ("q")? }, None)),
        "group" => Some((Emplacement::Groupe(champ("name")?), None)),
        _ => None,
    }
}

pub fn nom_affiche(d: &str) -> String {
    if let Some(motif) = d.strip_prefix('?') {
        return format!("« {motif} »");
    }
    let t = d.trim_end_matches('\\');
    t.rsplit('\\').next().unwrap_or(t).trim_end_matches(':').to_string()
}

/// Taille de tout ce que contient un dossier (vue « tailles »). S'arrête dès que `continuer` devient faux.
pub fn taille_totale(chemin: &str, continuer: &dyn Fn() -> bool) -> u64 {
    let mut total = 0u64;
    let mut a_lire = vec![chemin.to_string()];
    while let Some(d) = a_lire.pop() {
        if !continuer() {
            break;
        }
        let _ = parcourir(&d, true, |e| match e.genre {
            Genre::Dossier if !e.lien => a_lire.push(joindre(&d, &e.nom)),
            Genre::Fichier => total += e.taille,
            _ => {}
        });
    }
    total
}

/// Élément sans dossier réel (intérieur d'une archive, résultat de l'index)
pub fn entree_fichier(nom: &str, taille: u64, modif: u64, dossier: bool) -> Entree {
    let ext = if dossier { String::new() } else { nom.rfind('.').filter(|&i| i > 0).map(|i| nom[i + 1..].to_uppercase()).unwrap_or_default() };
    Entree {
        nom_w: wide(nom),
        nom: nom.to_string(),
        genre: if dossier { Genre::Dossier } else { Genre::Fichier },
        taille,
        modif,
        ajout: modif,
        ext,
        cache: false,
        parent: None,
        lien: false,
        nuage: Nuage::Aucun,
        meta: None,
    }
}

/// Dossier (partage d'un serveur, dossier cloud…) tel qu'il apparaît dans une liste
pub fn entree_dossier(nom: &str, parent: &str) -> Entree {
    Entree {
        nom_w: wide(nom),
        nom: nom.to_string(),
        genre: Genre::Dossier,
        taille: 0,
        modif: 0,
        ajout: 0,
        ext: String::new(),
        cache: false,
        parent: Some(Arc::from(parent)),
        lien: false,
        nuage: Nuage::Aucun,
        meta: None,
    }
}

// ── Filtre tapé dans un dossier (lot 14) : `parseQ` / `matchQ` de la maquette ──

/// Une lettre sans accent ni majuscule. Toujours une lettre pour une lettre : les positions d'un nom normalisé sont celles du nom
/// (le surlignage en dépend).
fn normaliser_car(c: char) -> char {
    let m = c.to_lowercase().next().unwrap_or(c);
    match m {
        'à' | 'á' | 'â' | 'ã' | 'ä' | 'å' => 'a',
        'ç' => 'c',
        'è' | 'é' | 'ê' | 'ë' => 'e',
        'ì' | 'í' | 'î' | 'ï' => 'i',
        'ñ' => 'n',
        'ò' | 'ó' | 'ô' | 'õ' | 'ö' => 'o',
        'ù' | 'ú' | 'û' | 'ü' => 'u',
        'ý' | 'ÿ' => 'y',
        autre => autre,
    }
}

/// Texte sans accent ni casse (`nz` de la maquette)
pub fn normaliser(s: &str) -> String {
    s.chars().map(normaliser_car).collect()
}

/// Une condition du filtre autre qu'un mot du nom
#[derive(Clone, Debug, PartialEq)]
pub enum Critere {
    /// `ext:pdf`, `ext:pdf,docx` ou `*.pdf` : extensions en minuscules, sans point (les dossiers n'en ont pas)
    Extensions(Vec<String>),
    /// `taille>10mo` : strictement plus gros que ce nombre d'octets (les dossiers n'ont pas de taille)
    TailleMin(f64),
    /// `taille<2go` : strictement plus petit
    TailleMax(f64),
    /// `modif:2026` : l'année est dans la date de modification affichée
    Annee(u32),
    /// `modif:7` ou `modif:7j` : modifié dans les N derniers jours (comme la recherche dans tout le PC, `index.rs`)
    Jours(u64),
    /// `etiquette:2` : porte l'étiquette N (1 à 3 carrés). Elle dépend du chemin complet, que `accepte` n'a pas : voir `Filtre::etiquette`.
    Etiquette(u8),
}

/// Filtre analysé : des mots (sans accent ni casse) qui doivent tous être dans le nom, et des critères
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Filtre {
    pub mots: Vec<String>,
    pub criteres: Vec<Critere>,
}

/// `10mo`, `1,5go`, `200k`, `3` (Mo par défaut, comme la maquette) → octets. Base 1024, comme l'affichage des tailles et `index.rs`.
fn octets_du_filtre(s: &str) -> Option<f64> {
    let fin = s.find(|c: char| !(c.is_ascii_digit() || c == '.' || c == ',')).unwrap_or(s.len());
    if fin == 0 {
        return None;
    }
    let n: f64 = s[..fin].replace(',', ".").parse().ok()?;
    let mult = match &s[fin..] {
        "" | "m" | "mo" => 1024.0 * 1024.0,
        "o" => 1.0,
        "k" | "ko" => 1024.0,
        "g" | "go" => 1024.0 * 1024.0 * 1024.0,
        "t" | "to" => 1024f64.powi(4),
        _ => return None,
    };
    Some(n * mult)
}

/// Analyse le filtre. Un jeton qui finit par `:`, `<` ou `>` est en cours de frappe : ignoré. Un jeton qui ressemble à un critère
/// mais n'en est pas un (`taille>abc`, `modif:2026x`) reste un mot, comme dans la maquette.
pub fn analyser_filtre(texte: &str) -> Filtre {
    let mut f = Filtre::default();
    for jeton in texte.split_whitespace() {
        if jeton.ends_with([':', '<', '>']) {
            continue;
        }
        let bas = jeton.to_lowercase();
        let extensions = |liste: &str| -> Option<Vec<String>> {
            let v: Vec<String> = liste.split([',', ';']).map(|x| x.trim_start_matches('.').to_string()).filter(|x| !x.is_empty()).collect();
            (!v.is_empty() && v.iter().all(|x| x.chars().all(|c| c.is_alphanumeric() || c == '_'))).then_some(v)
        };
        let critere = if let Some(e) = bas.strip_prefix("ext:") {
            extensions(e).map(Critere::Extensions)
        } else if let Some(e) = bas.strip_prefix("*.") {
            extensions(e).map(Critere::Extensions)
        } else if let Some(v) = bas.strip_prefix("taille>") {
            octets_du_filtre(v).map(Critere::TailleMin)
        } else if let Some(v) = bas.strip_prefix("taille<") {
            octets_du_filtre(v).map(Critere::TailleMax)
        } else if let Some(v) = bas.strip_prefix("etiquette:") {
            v.parse::<u8>().ok().filter(|n| (1..=crate::etiquettes::MAX).contains(n)).map(Critere::Etiquette)
        } else if let Some(v) = bas.strip_prefix("modif:") {
            let chiffres = |s: &str| !s.is_empty() && s.chars().all(|c| c.is_ascii_digit());
            if chiffres(v) && v.len() == 4 {
                v.parse().ok().map(Critere::Annee)
            } else {
                let n = v.strip_suffix(['j', 'd']).unwrap_or(v);
                (chiffres(n) && n.len() <= 3).then(|| n.parse().ok().map(Critere::Jours)).flatten()
            }
        } else {
            None
        };
        match critere {
            Some(c) => f.criteres.push(c),
            None => f.mots.push(normaliser(jeton)),
        }
    }
    f
}

impl Filtre {
    pub fn est_vide(&self) -> bool {
        self.mots.is_empty() && self.criteres.is_empty()
    }

    /// Étiquette demandée par `etiquette:N`, s'il y en a une : à tester avec le chemin complet de l'entrée (`accepte` ne le connaît pas)
    pub fn etiquette(&self) -> Option<u8> {
        self.criteres.iter().find_map(|c| if let Critere::Etiquette(n) = c { Some(*n) } else { None })
    }

    /// L'entrée passe-t-elle le filtre ? `maintenant` (FILETIME) sert à `modif:N`.
    pub fn accepte(&self, e: &Entree, maintenant: u64) -> bool {
        if !self.mots.is_empty() {
            let nom = normaliser(&e.nom);
            if !self.mots.iter().all(|m| nom.contains(m.as_str())) {
                return false;
            }
        }
        self.criteres.iter().all(|c| match c {
            Critere::Extensions(v) => {
                !e.est_conteneur() && e.nom.rsplit_once('.').is_some_and(|(avant, ext)| !avant.is_empty() && v.iter().any(|x| x.eq_ignore_ascii_case(ext)))
            }
            Critere::TailleMin(n) => !e.est_conteneur() && e.taille as f64 > *n,
            Critere::TailleMax(n) => !e.est_conteneur() && (e.taille as f64) < *n,
            Critere::Annee(a) => crate::systeme::date(e.modif).contains(&format!("/{a}")),
            Critere::Jours(j) => e.modif >= maintenant.saturating_sub(j * 864_000_000_000),
            // Décidé à part, avec le chemin complet
            Critere::Etiquette(_) => true,
        })
    }
}

/// Plages (début, fin exclue, en lettres) du nom que le filtre fait correspondre, à souligner : chaque mot partout où il est, sans
/// accent ni casse ; deux plages qui se touchent ou se chevauchent n'en font qu'une (`hi` de la maquette). Les jetons `ext:`,
/// `taille>`, `modif:` et `*.pdf` ne sont pas des mots du nom : jamais soulignés.
pub fn plages_correspondance(nom: &str, filtre: &str) -> Vec<(usize, usize)> {
    let mots = analyser_filtre(filtre).mots;
    if mots.is_empty() {
        return Vec::new();
    }
    let z: Vec<char> = nom.chars().map(normaliser_car).collect();
    let mut marque = vec![false; z.len()];
    for m in &mots {
        let t: Vec<char> = m.chars().collect();
        if t.is_empty() || t.len() > z.len() {
            continue;
        }
        let mut i = 0;
        while i + t.len() <= z.len() {
            if z[i..i + t.len()] == t[..] {
                marque[i..i + t.len()].iter_mut().for_each(|x| *x = true);
                i += t.len();
            } else {
                i += 1;
            }
        }
    }
    let mut plages = Vec::new();
    let mut debut = None;
    for (i, &m) in marque.iter().enumerate() {
        match (m, debut) {
            (true, None) => debut = Some(i),
            (false, Some(d)) => {
                plages.push((d, i));
                debut = None;
            }
            _ => {}
        }
    }
    if let Some(d) = debut {
        plages.push((d, marque.len()));
    }
    plages
}

/// Cherche dans tous les sous-dossiers, en largeur (les résultats proches d'abord)
fn rechercher(racine: &str, motif: &str, caches: bool) -> Vec<Entree> {
    let numero = RECHERCHE.fetch_add(1, AtomicOrdering::SeqCst) + 1;
    let filtre = analyser_filtre(motif);
    let maintenant = crate::systeme::maintenant();
    let mut resultats = Vec::new();
    let mut a_lire = std::collections::VecDeque::from([racine.to_string()]);
    while let Some(dossier) = a_lire.pop_front() {
        // Une recherche plus récente a commencé : celle-ci ne sert plus
        if RECHERCHE.load(AtomicOrdering::Relaxed) != numero || resultats.len() >= MAX_RESULTATS {
            break;
        }
        let parent: Arc<str> = Arc::from(dossier.as_str());
        let _ = parcourir(&dossier, caches, |mut e| {
            if matches!(e.genre, Genre::Dossier) && !e.lien {
                a_lire.push_back(joindre(&dossier, &e.nom));
            }
            if filtre.accepte(&e, maintenant) && filtre.etiquette().map_or(true, |n| crate::etiquettes::de(&joindre(&dossier, &e.nom)) == Some(n)) {
                e.parent = Some(parent.clone());
                resultats.push(e);
            }
        });
    }
    resultats
}

/// Lit le contenu d'un dossier, élément par élément
fn parcourir(chemin: &str, caches: bool, mut recevoir: impl FnMut(Entree)) -> Result<(), String> {
    let motif = if chemin.ends_with('\\') { format!("{chemin}*") } else { format!("{chemin}\\*") };
    let motif_w = wide(&motif);
    unsafe {
        let mut d = WIN32_FIND_DATAW::default();
        // FindExInfoBasic saute le nom court 8.3 ; LARGE_FETCH lit par gros blocs
        let h = FindFirstFileExW(
            PCWSTR(motif_w.as_ptr()),
            FindExInfoBasic,
            &mut d as *mut _ as *mut _,
            FindExSearchNameMatch,
            None,
            FIND_FIRST_EX_LARGE_FETCH,
        );
        let h = match h {
            Ok(h) => h,
            Err(_) => {
                let e = GetLastError();
                return Err(match e {
                    ERROR_ACCESS_DENIED => "ACCES REFUSE".into(),
                    ERROR_FILE_NOT_FOUND | ERROR_PATH_NOT_FOUND => "DOSSIER INTROUVABLE".into(),
                    _ => format!("ERREUR {}", e.0),
                });
            }
        };
        loop {
            let len = d.cFileName.iter().position(|&c| c == 0).unwrap_or(d.cFileName.len());
            let nom_u = &d.cFileName[..len];
            let special = nom_u == [b'.' as u16] || nom_u == [b'.' as u16, b'.' as u16];
            let attr = d.dwFileAttributes;
            let cache = attr & (FILE_ATTRIBUTE_HIDDEN.0 | FILE_ATTRIBUTE_SYSTEM.0) != 0;
            if !special && (caches || !cache) {
                let nom = String::from_utf16_lossy(nom_u);
                let dossier = attr & FILE_ATTRIBUTE_DIRECTORY.0 != 0;
                let ext = if dossier {
                    String::new()
                } else {
                    nom.rfind('.').filter(|&i| i > 0).map(|i| nom[i + 1..].to_uppercase()).unwrap_or_default()
                };
                let mut nom_w = nom_u.to_vec();
                nom_w.push(0);
                recevoir(Entree {
                    nom,
                    nom_w,
                    genre: if dossier { Genre::Dossier } else { Genre::Fichier },
                    taille: if dossier { 0 } else { ((d.nFileSizeHigh as u64) << 32) | d.nFileSizeLow as u64 },
                    modif: ((d.ftLastWriteTime.dwHighDateTime as u64) << 32) | d.ftLastWriteTime.dwLowDateTime as u64,
                    ajout: ((d.ftCreationTime.dwHighDateTime as u64) << 32) | d.ftCreationTime.dwLowDateTime as u64,
                    ext,
                    cache,
                    parent: None,
                    // Les dossiers OneDrive sont aussi des points d'analyse : ce ne sont pas des liens
                    lien: attr & FILE_ATTRIBUTE_REPARSE_POINT.0 != 0 && !etiquette_cloud(d.dwReserved0),
                    nuage: etat_nuage(attr, d.dwReserved0),
                    meta: None,
                });
            }
            if FindNextFileW(h, &mut d).is_err() {
                break;
            }
        }
        let _ = FindClose(h);
    }
    Ok(())
}

fn lire_disques() -> Vec<Entree> {
    let mut v = Vec::new();
    unsafe {
        let mut buf = [0u16; 512];
        let n = GetLogicalDriveStringsW(Some(&mut buf)) as usize;
        for racine in buf[..n.min(buf.len())].split(|&c| c == 0).filter(|s| !s.is_empty()) {
            let racine_s = String::from_utf16_lossy(racine);
            let racine_w = wide(&racine_s);
            let p = PCWSTR(racine_w.as_ptr());
            let mut etiquette = [0u16; 128];
            let _ = GetVolumeInformationW(p, Some(&mut etiquette), None, None, None, None);
            let len = etiquette.iter().position(|&c| c == 0).unwrap_or(0);
            let mut etiquette = String::from_utf16_lossy(&etiquette[..len]);
            if etiquette.is_empty() {
                etiquette = match GetDriveTypeW(p) {
                    2 => "AMOVIBLE".into(),
                    4 => "RESEAU".into(),
                    5 => "CD".into(),
                    _ => "DISQUE LOCAL".into(),
                };
            }
            let (mut libre, mut total) = (0u64, 0u64);
            let _ = GetDiskFreeSpaceExW(p, Some(&mut libre), Some(&mut total), None);
            let lettre = racine_s.trim_end_matches('\\').to_string();
            let nom = format!("{} ({lettre})", etiquette.to_uppercase());
            v.push(Entree {
                nom_w: wide(&nom),
                nom,
                genre: Genre::Disque { racine: racine_s, libre, total },
                taille: total,
                modif: 0,
                ajout: 0,
                ext: String::new(),
                cache: false,
                parent: None,
                lien: false,
                nuage: Nuage::Aucun,
                meta: None,
            });
        }
    }
    v
}

fn cmp_noms(a: &Entree, b: &Entree) -> Ordering {
    unsafe { StrCmpLogicalW(PCWSTR(a.nom_w.as_ptr()), PCWSTR(b.nom_w.as_ptr())).cmp(&0) }
}

/// Compare une valeur des propriétés ; les éléments sans valeur vont à la fin quel que soit le sens
fn cmp_meta(a: &Entree, b: &Entree, croissant: bool, f: impl Fn(&Meta) -> u64) -> Ordering {
    let (x, y) = (a.meta.as_deref().map(&f).unwrap_or(0), b.meta.as_deref().map(&f).unwrap_or(0));
    match (x == 0, y == 0) {
        (true, false) => if croissant { Ordering::Greater } else { Ordering::Less },
        (false, true) => if croissant { Ordering::Less } else { Ordering::Greater },
        _ => x.cmp(&y).then_with(|| cmp_noms(a, b)),
    }
}

/// Compare un texte des propriétés dans le sens demandé ; sans valeur : à la fin dans les deux sens
fn cmp_texte(a: &Entree, b: &Entree, croissant: bool, f: impl Fn(&Meta) -> &str) -> Ordering {
    let (x, y) = (a.meta.as_deref().map_or("", &f), b.meta.as_deref().map_or("", &f));
    match (x.is_empty(), y.is_empty()) {
        (true, false) => if croissant { Ordering::Greater } else { Ordering::Less },
        (false, true) => if croissant { Ordering::Less } else { Ordering::Greater },
        _ => {
            let o = x.to_lowercase().cmp(&y.to_lowercase()).then_with(|| cmp_noms(a, b));
            if croissant { o } else { o.reverse() }
        }
    }
}

/// Dossiers d'abord (comme l'Explorateur) sauf si `melange`, puis la colonne choisie, puis le nom.
/// La date est la plus récente entre modification et arrivée dans le dossier.
pub fn trier(v: &mut [Entree], col: Colonne, croissant: bool, melange: bool) {
    v.sort_by(|a, b| {
        let conteneurs = b.est_conteneur().cmp(&a.est_conteneur());
        if conteneurs != Ordering::Equal && !melange {
            return conteneurs;
        }
        let o = match col {
            Colonne::Nom => cmp_noms(a, b),
            Colonne::Date => a.date_colonne().cmp(&b.date_colonne()).then_with(|| cmp_noms(a, b)),
            Colonne::Type => a.ext.cmp(&b.ext).then_with(|| cmp_noms(a, b)),
            Colonne::Taille => a.taille.cmp(&b.taille).then_with(|| cmp_noms(a, b)),
            // Sans valeur : à la fin dans les deux sens
            Colonne::Dimensions => cmp_meta(a, b, croissant, |m| m.largeur as u64 * m.hauteur as u64),
            Colonne::Duree => cmp_meta(a, b, croissant, |m| m.duree),
            Colonne::Auteur => return cmp_texte(a, b, croissant, |m| &m.auteur),
            Colonne::Origine => return cmp_texte(a, b, croissant, |m| &m.origine),
            Colonne::Signature => return cmp_texte(a, b, croissant, |m| &m.signature),
        };
        if croissant { o } else { o.reverse() }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disque_presque_plein_dit_les_go_libres_en_casse_normale() {
        let go = 1024u64 * 1024 * 1024;
        assert_eq!(etiquette_disque(true, false, 28 * go, &|_| true), "Presque plein · 28 Go libres");
        assert_eq!(etiquette_disque(true, false, 28 * go + go / 2, &|_| true), "Presque plein · 28,5 Go libres");
        // Colonne étroite : on perd d'abord les Go, puis le mot « presque »
        assert_eq!(etiquette_disque(true, false, 28 * go, &|t| t == "Presque plein"), "Presque plein");
        assert_eq!(etiquette_disque(true, false, 28 * go, &|_| false), "Plein");
        assert_eq!(etiquette_disque(false, false, 28 * go, &|_| true), "Disque");
        assert_eq!(etiquette_disque(false, true, 28 * go, &|_| true), "Partage");
    }

    #[test]
    fn etats_cloud_en_casse_normale() {
        assert_eq!(Nuage::EnLigne.libelle(), "En ligne seulement");
        assert_eq!(Nuage::Local.libelle(), "Sur ce PC");
        assert_eq!(Nuage::Epingle.libelle(), "Toujours sur ce PC");
        assert_eq!(Nuage::Aucun.libelle(), "");
    }

    #[test]
    fn carte_de_disque_du_volet_en_casse_normale() {
        let go = 1024u64 * 1024 * 1024;
        assert_eq!(libres_sur(181 * go, 476 * go), "181 Go libres sur 476 Go");
        assert_eq!(libres_sur(28 * go + go / 2, 100 * go), "28,5 Go libres sur 100 Go");
    }

    #[test]
    fn filtre_etiquette_un_a_trois() {
        assert_eq!(analyser_filtre("etiquette:2").criteres, [Critere::Etiquette(2)]);
        assert_eq!(analyser_filtre("rapport ETIQUETTE:3").etiquette(), Some(3));
        assert_eq!(analyser_filtre("rapport ETIQUETTE:3").mots, ["rapport"]);
        // Hors de 1 à 3, ou en cours de frappe : un mot, ou rien (comme les autres critères)
        assert_eq!(analyser_filtre("etiquette:4").mots, ["etiquette:4"]);
        assert!(analyser_filtre("etiquette:").est_vide());
        assert_eq!(analyser_filtre("etiquette:0").etiquette(), None);
        // `accepte` ne voit pas le chemin : il laisse passer, c'est l'appelant qui teste l'étiquette
        let f = analyser_filtre("etiquette:1");
        assert!(f.accepte(&avec("a.txt", 1, 1), 2));
    }

    const MO: u64 = 1024 * 1024;
    const JOUR: u64 = 864_000_000_000;

    fn avec(nom: &str, taille: u64, modif: u64) -> Entree {
        Entree { nom: nom.into(), nom_w: wide(nom), genre: Genre::Fichier, taille, modif, ajout: 0, ext: String::new(), cache: false, parent: None, lien: false, nuage: Nuage::Aucun, meta: None }
    }

    fn passe(filtre: &str, e: &Entree) -> bool {
        analyser_filtre(filtre).accepte(e, crate::systeme::filetime(2026, 11, 8, 12, 0))
    }

    #[test]
    fn filtre_ext_mots_et_jetons_incomplets() {
        let f = analyser_filtre("Contrat ext:PDF");
        assert_eq!(f, Filtre { mots: vec!["contrat".into()], criteres: vec![Critere::Extensions(vec!["pdf".into()])] });
        assert!(passe("ext:pdf", &avec("Contrat-bail.PDF", 1, 0)));
        assert!(!passe("ext:pdf", &avec("notes.docx", 1, 0)));
        assert!(passe("ext:pdf,docx", &avec("notes.docx", 1, 0)));
        assert!(passe("*.md", &avec("PLAN.md", 1, 0)));
        // Un dossier n'a pas d'extension, un fichier caché « .pdf » non plus
        let mut dossier = avec("archives.pdf", 0, 0);
        dossier.genre = Genre::Dossier;
        assert!(!passe("ext:pdf", &dossier));
        assert!(!passe("ext:pdf", &avec(".pdf", 1, 0)));
        // Mots : sans accent ni casse, tous requis
        assert!(passe("ETE cafe", &avec("Été au Café.txt", 1, 0)));
        assert!(!passe("ete thé", &avec("Été au Café.txt", 1, 0)));
        // Jeton en cours de frappe : ignoré (`ext:`, `taille>`, `taille<`, `modif:`)
        for incomplet in ["ext:", "taille>", "taille<", "modif:", "ext: taille> modif:"] {
            assert!(analyser_filtre(incomplet).est_vide(), "{incomplet}");
            assert!(passe(incomplet, &avec("n'importe quoi", 1, 0)), "{incomplet}");
        }
        // Un jeton qui ressemble à un critère sans en être un reste un mot
        assert_eq!(analyser_filtre("taille>abc").mots, ["taille>abc"]);
        assert_eq!(analyser_filtre("modif:2026x").mots, ["modif:2026x"]);
    }

    #[test]
    fn filtre_taille_unites_et_virgule() {
        let t = |filtre: &str, taille: u64| passe(filtre, &avec("a.bin", taille, 0));
        // Mo par défaut ; strictement plus grand / plus petit
        assert!(t("taille>10", 11 * MO) && !t("taille>10", 10 * MO) && !t("taille>10", 9 * MO));
        assert!(t("taille>10mo", 11 * MO) && !t("taille>10mo", 5 * MO));
        assert!(t("taille<2go", 1024 * MO) && !t("taille<2go", 3 * 1024 * MO));
        assert!(t("TAILLE>1,5go", 2 * 1024 * MO) && !t("taille>1,5go", 1024 * MO));
        assert!(t("taille>1.5Go", 2 * 1024 * MO));
        assert!(t("taille<500ko", 100 * 1024) && !t("taille<500ko", MO));
        assert!(t("taille>1k", 2048) && t("taille<10o", 9));
        // Bornes cumulées
        assert!(t("taille>1mo taille<3mo", 2 * MO) && !t("taille>1mo taille<3mo", 4 * MO));
        // Un dossier n'a pas de taille
        let mut dossier = avec("d", 0, 0);
        dossier.genre = Genre::Dossier;
        assert!(!passe("taille<1mo", &dossier));
        // Unité inconnue : un mot, rien ne passe
        assert!(!t("taille>10xx", 99 * MO));
    }

    #[test]
    fn filtre_modif_annee_et_jours() {
        let maintenant = crate::systeme::filetime(2026, 11, 8, 12, 0);
        let de = |annee, mois, jour| avec("a.txt", 1, crate::systeme::filetime(annee, mois, jour, 14, 3));
        assert_eq!(analyser_filtre("modif:2026").criteres, [Critere::Annee(2026)]);
        assert!(passe("modif:2026", &de(2026, 3, 1)));
        assert!(!passe("modif:2026", &de(2025, 12, 31)));
        // Un à trois chiffres : les N derniers jours, comme `index.rs`
        assert_eq!(analyser_filtre("modif:7").criteres, [Critere::Jours(7)]);
        assert_eq!(analyser_filtre("modif:30j").criteres, [Critere::Jours(30)]);
        assert!(analyser_filtre("modif:20260").criteres.is_empty());
        assert!(Filtre { criteres: vec![Critere::Jours(7)], ..Filtre::default() }.accepte(&avec("a", 1, maintenant - 6 * JOUR), maintenant));
        assert!(!Filtre { criteres: vec![Critere::Jours(7)], ..Filtre::default() }.accepte(&avec("a", 1, maintenant - 8 * JOUR), maintenant));
    }

    #[test]
    fn plages_de_surlignage() {
        // Un mot partout où il est, accents et casse ignorés (positions en lettres)
        assert_eq!(plages_correspondance("Café café", "CAFE"), [(0, 4), (5, 9)]);
        assert_eq!(plages_correspondance("budget-2026.xlsx", "bud 26"), [(0, 3), (9, 11)]);
        // Plusieurs mots qui se chevauchent ou se touchent : une seule plage
        assert_eq!(plages_correspondance("contrat-bail.pdf", "contr trat"), [(0, 7)]);
        assert_eq!(plages_correspondance("abcd", "ab cd"), [(0, 4)]);
        // Rien à souligner
        assert!(plages_correspondance("contrat.pdf", "").is_empty());
        assert!(plages_correspondance("contrat.pdf", "zzz").is_empty());
        // Les critères ne sont pas des mots du nom : `ext:pdf` ne souligne pas « pdf »
        assert!(plages_correspondance("contrat.pdf", "ext:pdf").is_empty());
        assert!(plages_correspondance("contrat.pdf", "*.pdf taille>1mo modif:2026").is_empty());
        assert_eq!(plages_correspondance("contrat.pdf", "ext:pdf con"), [(0, 3)]);
        // Lettres, pas octets : « é » compte pour une lettre
        assert_eq!(plages_correspondance("été-plan", "plan"), [(4, 8)]);
    }

    use std::sync::Arc;

    fn fichier(nom: &str, meta: Option<Meta>) -> Entree {
        Entree { nom: nom.into(), nom_w: wide(nom), genre: Genre::Fichier, taille: 1, modif: 0, ajout: 0, ext: String::new(), cache: false, parent: None, lien: false, nuage: Nuage::Aucun, meta: meta.map(Arc::new) }
    }

    #[test]
    fn adresses_spacefiles_et_alias() {
        for p in ["spacefiles", "nothingexplorer"] {
            let (e, nom) = emplacement_depuis_url(&format!("{p}://open?path=C%3A%5CUsers%5Cutilisateur")).unwrap();
            assert_eq!(e, Emplacement::dossier(r"C:\Users\utilisateur"));
            assert_eq!(nom, None);
            assert_eq!(emplacement_depuis_url(&format!("{}:search?q=ext%3Apdf+facture", p.to_uppercase())).unwrap().0, Emplacement::Globale { motif: "ext:pdf facture".into() });
            assert_eq!(emplacement_depuis_url(&format!("{p}://group?name=Projet")).unwrap().0, Emplacement::Groupe("Projet".into()));
            assert!(emplacement_depuis_url(&format!("{p}://delete?path=C%3A%5C")).is_none());
            assert!(emplacement_depuis_url(&format!("{p}://open")).is_none());
            assert_eq!(emplacement_depuis_url(&format!(r"{p}://C:\Users\utilisateur")).unwrap().0, Emplacement::dossier(r"C:\Users\utilisateur"));
        }
        assert!(emplacement_depuis_url("spacefile://open?path=C%3A%5C").is_none());
        assert!(emplacement_depuis_url("https://exemple.org").is_none());
    }

    #[test]
    fn dossiers_shell() {
        assert_eq!(Emplacement::dossier("shell:RecycleBinFolder"), Emplacement::Corbeille);
        assert!(matches!(Emplacement::dossier("shell:Startup"), Emplacement::Dossier(c) if c.ends_with(r"Start Menu\Programs\Startup")));
        assert!(dossier_shell("inconnu").is_none());
    }

    #[test]
    fn recherche_epinglee() {
        assert_eq!(Emplacement::dossier("?ext:pdf modif:7"), Emplacement::Globale { motif: "ext:pdf modif:7".into() });
        assert_eq!(nom_affiche("?ext:pdf"), "« ext:pdf »");
        assert_eq!(Emplacement::dossier(r"C:\projets"), Emplacement::Dossier(r"C:\projets".into()));
    }

    #[test]
    fn sftp_ferme_ou_ouvert_selon_le_reglage() {
        use crate::reglages::Experimentale;
        // Réglage vrai : comme avant le gel
        let e = Emplacement::dossier_avec("sftp://nico@serveur/home/nico", true);
        assert_eq!(e, Emplacement::Sftp { hote: "nico@serveur".into(), chemin: "/home/nico".into() });
        assert_eq!(e.experimentale(), Some(Experimentale::Sftp));
        // Réglage faux : plus un serveur, un chemin inerte qui reste identifié comme expérimental (refusé par `naviguer`)
        let ferme = Emplacement::dossier_avec("sftp://nico@serveur/home/nico", false);
        assert!(matches!(ferme, Emplacement::Dossier(_)), "{ferme:?}");
        assert_eq!(ferme.experimentale(), Some(Experimentale::Sftp));
        // Les chemins ordinaires et les partages SMB ne dépendent pas du réglage
        for c in [r"C:\projets", r"\\nas\Media", r"G:\Mon Drive"] {
            assert_eq!(Emplacement::dossier_avec(c, false), Emplacement::dossier_avec(c, true), "{c}");
            assert_eq!(Emplacement::dossier_avec(c, false).experimentale(), None, "{c}");
        }
        assert_eq!(Emplacement::dossier_avec(r"\\nas", false), Emplacement::Serveur("nas".into()));
    }

    #[test]
    fn webdav_reconnu_comme_experimental() {
        use crate::reglages::Experimentale;
        assert_eq!(Emplacement::dossier_avec(r"\\cloud.fr@SSL\remote.php\dav", false).experimentale(), Some(Experimentale::WebDav));
        assert_eq!(Emplacement::dossier_avec(r"\\cloud.fr@SSL", true).experimentale(), Some(Experimentale::WebDav));
        assert_eq!(Emplacement::Corbeille.experimentale(), None);
    }

    #[test]
    fn quarantaine() {
        let sec = |e: &Entree| section_origine(e).1;
        assert_eq!(sec(&fichier("a", None)), "ANALYSE EN COURS");
        assert_eq!(sec(&fichier("a", Some(Meta { origine: "exemple.org".into(), signature: "NON SIGNE".into(), ..Meta::default() }))), "VENU D'INTERNET");
        assert_eq!(sec(&fichier("a.exe", Some(Meta { signature: "NON SIGNE".into(), ..Meta::default() }))), "PROGRAMMES A VERIFIER");
        assert_eq!(sec(&fichier("a.exe", Some(Meta { signature: "NON VERIFIE".into(), ..Meta::default() }))), "PROGRAMMES A VERIFIER");
        assert_eq!(sec(&fichier("a.exe", Some(Meta { signature: "EDITEUR".into(), ..Meta::default() }))), "LE RESTE");
        assert_eq!(sec(&fichier("a.txt", Some(Meta::default()))), "LE RESTE");
        assert_eq!(Regroupement::depuis('O'), Regroupement::Origine);
    }
}