// Page NETTOYAGE (E4a) : ce que Windows et les pilotes laissent derrière eux et que l'on peut effacer sans risque.
//
// La liste blanche (`docs/espace/E4-liste-blanche.md`, validée par l'auteur le 19/11/2026) est faite de **modèles de
// catégorie**, valables sur tout Windows 10 ou 11 : une racine lue par l'API du profil (jamais un chemin écrit en dur),
// une règle, un mode de suppression, un texte de risque. La mesure (E4a) se fait dans un fil de fond à basse priorité,
// jamais devant l'utilisateur, et ne descend que dans ces quelques dossiers. La suppression (E4b) ne part que d'une
// confirmation de l'utilisateur sur une liste exacte, dans un autre fil de fond, et repasse chaque élément par la porte
// (`est_autorise`, jonction, règle recalculée) juste avant de l'effacer.
//
// Garde-fous, testés : une racine hors du profil de l'utilisateur désactive la catégorie (aucun chemin de repli) ; les
// jonctions et liens symboliques ne sont jamais suivis (un lien rencontré est retiré lui-même, sa cible reste intacte) ;
// `est_autorise` n'accepte qu'un descendant exact d'une racine ; un fichier verrouillé est ignoré et compté, sans erreur.

use std::os::windows::ffi::OsStrExt;
use std::os::windows::fs::MetadataExt;
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Mutex;
use std::time::{Duration, SystemTime};

use windows::core::PCWSTR;
use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::System::Com::{CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_ALL, COINIT_APARTMENTTHREADED, COINIT_DISABLE_OLE1DDE};
use windows::Win32::System::Threading::{GetCurrentThread, SetThreadPriority, THREAD_PRIORITY_BELOW_NORMAL};
use windows::Win32::UI::Shell::{
    FileOperation, IFileOperation, IShellItem, SHCreateItemFromParsingName, SHEmptyRecycleBinW, SHQueryRecycleBinW, FILEOPERATION_FLAGS,
    FOFX_RECYCLEONDELETE, FOF_ALLOWUNDO, FOF_NOCONFIRMATION, FOF_NOCONFIRMMKDIR, FOF_NOERRORUI, FOF_SILENT, FOF_WANTNUKEWARNING,
    FOLDERID_LocalAppData, FOLDERID_Profile, SHQUERYRBINFO,
};
use windows::Win32::UI::WindowsAndMessaging::{PostMessageW, WM_APP};

/// Message posté à la fenêtre quand la mesure est prête ; le résultat se prend avec `prendre`
pub const WM_NETTOYAGE: u32 = WM_APP + 61;
/// Message posté à la fenêtre quand la suppression est finie ; le bilan se prend avec `prendre_bilan`
pub const WM_NETTOYAGE_FINI: u32 = WM_APP + 62;
/// Message posté à la fenêtre après chaque élément envoyé à la Corbeille : la page se redessine avec `progres`
pub const WM_NETTOYAGE_PROGRES: u32 = WM_APP + 63;

/// Attribut Windows d'une jonction ou d'un lien symbolique
const ATTRIBUT_REPARSE: u32 = 0x400;
const MO: u64 = 1024 * 1024;
const GO: u64 = 1024 * MO;
/// Âge (jours) au-delà duquel un élément de `Temp` est proposé : choix de l'auteur, 19/11/2026
pub const JOURS_TEMP: u64 = 7;
/// Rapports de plantage gardés (les plus récents)
pub const DUMPS_GARDES: usize = 2;

/// Comment une catégorie est effacée (dit à l'utilisateur avant, dans la confirmation)
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mode {
    /// Vers la Corbeille : récupérable
    Corbeille,
    /// Suppression directe : un cache de plusieurs Go n'a pas de sens dans la Corbeille
    Direct,
    /// Vidage de la Corbeille Windows : non récupérable
    Definitif,
}

impl Mode {
    pub fn libelle(self) -> &'static str {
        match self {
            Mode::Corbeille => "CORBEILLE",
            Mode::Direct => "DIRECT",
            Mode::Definitif => "DÉFINITIF",
        }
    }

    /// Ce qui arrivera, en toutes lettres, dans la confirmation
    pub fn description(self) -> &'static str {
        match self {
            Mode::Corbeille => "envoyé à la Corbeille, récupérable",
            Mode::Direct => "effacé directement, non récupérable",
            Mode::Definitif => "Corbeille vidée sur tous les lecteurs, non récupérable",
        }
    }
}

/// Ce qui, dans une racine, est proposé à la suppression
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Regle {
    /// Tout le contenu de la racine
    Tout,
    /// Les éléments (fichier ou dossier) dont le fichier le plus récent a plus de `JOURS_TEMP` jours
    PlusVieuxQue(u64),
    /// Les fichiers `.dmp` de la racine, sauf les `n` plus récents
    DumpsSaufRecents(usize),
    /// Le contenu de la Corbeille Windows, mesuré par l'API du Shell (pas par un parcours)
    CorbeilleWindows,
}

/// Dossier de départ d'une racine, lu par l'API Windows
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Base {
    /// `%LOCALAPPDATA%`
    Local,
    /// Dossier temporaire de l'utilisateur (`GetTempPath`)
    Temp,
}

#[derive(Debug)]
pub struct Modele {
    pub id: &'static str,
    pub libelle: &'static str,
    /// Ce que l'effacement coûte, dit à l'utilisateur
    pub risque: &'static str,
    /// Racines : une base et un chemin relatif (vide pour la base elle-même)
    pub racines: &'static [(Base, &'static str)],
    pub regle: Regle,
    pub mode: Mode,
    /// Taille minimale pour que la catégorie soit affichée
    pub seuil: u64,
    /// Taille au-delà de laquelle elle est signalée comme anormale (cache de shaders de plusieurs Go)
    pub anormal: Option<u64>,
}

const RISQUE_SHADERS: &str = "Recréé par le pilote ; saccades et chargements plus longs au premier lancement de chaque jeu.";

/// Les sept modèles validés. Aucun nom de programme n'y figure : seuls les dossiers des pilotes graphiques et de Windows.
pub const MODELES: [Modele; 7] = [
    Modele {
        id: "temp",
        libelle: "Fichiers temporaires",
        risque: "Sans effet ; seuls les éléments de plus de 7 jours sont proposés, ceux qui servent encore sont ignorés.",
        racines: &[(Base::Temp, "")],
        regle: Regle::PlusVieuxQue(JOURS_TEMP),
        mode: Mode::Corbeille,
        seuil: MO,
        anormal: None,
    },
    Modele {
        id: "d3d",
        libelle: "Cache de shaders DirectX",
        risque: RISQUE_SHADERS,
        racines: &[(Base::Local, "D3DSCache")],
        regle: Regle::Tout,
        mode: Mode::Direct,
        seuil: MO,
        anormal: Some(2 * GO),
    },
    Modele {
        id: "nvidia",
        libelle: "Cache de shaders NVIDIA",
        risque: RISQUE_SHADERS,
        racines: &[(Base::Local, r"NVIDIA\DXCache"), (Base::Local, r"NVIDIA\GLCache")],
        regle: Regle::Tout,
        mode: Mode::Direct,
        seuil: MO,
        anormal: Some(2 * GO),
    },
    Modele {
        id: "amd",
        libelle: "Cache de shaders AMD",
        risque: RISQUE_SHADERS,
        racines: &[(Base::Local, r"AMD\DxCache"), (Base::Local, r"AMD\GLCache")],
        regle: Regle::Tout,
        mode: Mode::Direct,
        seuil: MO,
        anormal: Some(2 * GO),
    },
    Modele {
        id: "intel",
        libelle: "Cache de shaders Intel",
        risque: RISQUE_SHADERS,
        racines: &[(Base::Local, r"Intel\ShaderCache")],
        regle: Regle::Tout,
        mode: Mode::Direct,
        seuil: MO,
        anormal: Some(2 * GO),
    },
    Modele {
        id: "plantages",
        libelle: "Rapports de plantage",
        risque: "Efface la trace d'un plantage, pas sa cause ; les 2 plus récents sont gardés.",
        racines: &[(Base::Local, "CrashDumps")],
        regle: Regle::DumpsSaufRecents(DUMPS_GARDES),
        mode: Mode::Corbeille,
        seuil: MO,
        anormal: None,
    },
    Modele {
        id: "corbeille",
        libelle: "Corbeille Windows",
        risque: "Non récupérable : tout ce qu'elle contient disparaît, sur tous les lecteurs.",
        racines: &[],
        regle: Regle::CorbeilleWindows,
        mode: Mode::Definitif,
        // Une Corbeille qui contient quelque chose est proposée, même de quelques octets
        seuil: 1,
        anormal: None,
    },
];

/// Dossiers de l'utilisateur dont les racines sont tirées ; injectables pour tester sur un faux profil
#[derive(Clone, Debug, Default)]
pub struct Racines {
    pub profil: Option<PathBuf>,
    pub local: Option<PathBuf>,
    pub temp: Option<PathBuf>,
}

impl Racines {
    /// Les dossiers réels de l'utilisateur courant, lus par les API du Shell (ils suivent les redirections)
    pub fn systeme() -> Self {
        Self {
            profil: crate::systeme::dossier_connu(&FOLDERID_Profile).map(PathBuf::from),
            local: crate::systeme::dossier_connu(&FOLDERID_LocalAppData).map(PathBuf::from),
            temp: Some(std::env::temp_dir()),
        }
    }

    fn base(&self, b: Base) -> Option<&Path> {
        match b {
            Base::Local => self.local.as_deref(),
            Base::Temp => self.temp.as_deref(),
        }
    }
}

/// Noms de dossiers que rien ne doit jamais toucher, cherchés sous le profil
const INTERDITS: [&str; 5] = ["windows", "program files", "program files (x86)", "prefetch", "system volume information"];

/// Composants d'un chemin en minuscules, sans le séparateur final ; `None` s'il contient `..` ou `.`
/// (un chemin lexical honnête : aucune évasion par remontée)
fn composants(p: &Path) -> Option<Vec<String>> {
    let mut v = Vec::new();
    for c in p.components() {
        match c {
            Component::Prefix(x) => v.push(x.as_os_str().to_string_lossy().to_lowercase()),
            Component::RootDir => v.push("\\".to_string()),
            Component::Normal(n) => v.push(n.to_string_lossy().to_lowercase()),
            Component::ParentDir | Component::CurDir => return None,
        }
    }
    Some(v)
}

/// Vrai si `chemin` est un descendant strict de `parent` (comparaison sans casse, par composants)
fn descendant_strict(chemin: &Path, parent: &Path) -> bool {
    match (composants(chemin), composants(parent)) {
        (Some(c), Some(p)) => c.len() > p.len() && c[..p.len()] == p[..],
        _ => false,
    }
}

/// Racines d'un modèle qui sont autorisées : sous le profil de l'utilisateur, hors des dossiers interdits.
/// Une racine hors du profil (un `Temp` redirigé vers la racine d'un autre disque, un dossier partagé) est écartée.
pub fn racines_valides(m: &Modele, r: &Racines) -> Vec<PathBuf> {
    let Some(profil) = r.profil.as_deref() else { return Vec::new() };
    let nb_profil = composants(profil).map_or(0, |c| c.len());
    m.racines
        .iter()
        .filter_map(|(base, sous)| {
            let b = r.base(*base)?;
            // `GetTempPath` rend parfois un chemin court (`C:\Users\NICOLA~1\…`) alors que le profil est en noms longs :
            // sans développement, la comparaison échouerait et la catégorie disparaîtrait sans un mot
            let b = PathBuf::from(crate::operations::developper(&b.to_string_lossy()));
            let chemin = if sous.is_empty() { b } else { b.join(sous) };
            if !descendant_strict(&chemin, profil) {
                return None;
            }
            // Une jonction dans un dossier parent (`AppData\Local` déplacé sur un autre disque, par exemple) ferait
            // sortir du profil sans que la comparaison de texte le voie : on refuse toute la chaîne du profil à la racine
            let traverse_un_lien = chemin.ancestors().filter(|a| descendant_strict(a, profil)).any(|a| {
                std::fs::symlink_metadata(a).is_ok_and(|m| m.file_attributes() & ATTRIBUT_REPARSE != 0)
            });
            if traverse_un_lien {
                return None;
            }
            let c = composants(&chemin)?;
            if c[nb_profil..].iter().any(|n| INTERDITS.contains(&n.as_str())) {
                return None;
            }
            Some(chemin)
        })
        .collect()
}

/// Un chemin peut-il être effacé par ce modèle ? Oui seulement s'il est un descendant exact d'une racine valide ; pour
/// les rapports de plantage, seulement un fichier `.dmp` directement dans la racine. Jamais pour la Corbeille Windows
/// (elle se vide par l'API du Shell, pas par des chemins). C'est la porte unique que la suppression franchit avant tout effacement.
pub fn est_autorise(m: &Modele, chemin: &Path, r: &Racines) -> bool {
    if m.regle == Regle::CorbeilleWindows {
        return false;
    }
    racines_valides(m, r).iter().any(|racine| {
        // Toujours un enfant direct de la racine : les modèles proposent des éléments de premier niveau, et un parent
        // intermédiaire (jamais contrôlé comme lien) ne doit pas pouvoir se glisser entre la racine et le chemin
        if !descendant_strict(chemin, racine) || chemin.parent().map(composants) != Some(composants(racine)) {
            return false;
        }
        // Win32 retire les points et espaces de fin d'un nom (`foo.` devient `foo`, `...` devient la racine) : un tel
        // nom désignerait un autre élément que celui qu'on a contrôlé. Un `:` ouvrirait un flux de données.
        let nom = chemin.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        if nom.is_empty() || nom.ends_with('.') || nom.ends_with(' ') || nom.contains(':') {
            return false;
        }
        match m.regle {
            Regle::DumpsSaufRecents(_) => est_dmp(chemin),
            _ => true,
        }
    })
}

fn est_dmp(p: &Path) -> bool {
    p.extension().is_some_and(|e| e.eq_ignore_ascii_case("dmp"))
}

/// Une catégorie mesurée : ce que le modèle propose sur ce PC
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Categorie {
    pub id: &'static str,
    pub libelle: &'static str,
    pub risque: &'static str,
    pub mode: Mode,
    pub octets: u64,
    pub fichiers: u64,
    /// Éléments proposés : entrées de premier niveau, ou éléments de la Corbeille
    pub elements: u64,
    /// Dossiers ou fichiers qu'on n'a pas pu lire (verrouillés, refusés) : ignorés
    pub illisibles: u64,
    /// Plus gros que ce qu'un cache normal occupe
    pub anormal: bool,
    /// Les éléments de premier niveau proposés, tels que la mesure les a vus (vide pour la Corbeille Windows). La
    /// suppression ne leur fait pas confiance : chacun repasse par la porte juste avant d'être effacé.
    pub chemins: Vec<PathBuf>,
}

#[derive(Default)]
struct Mesure {
    octets: u64,
    fichiers: u64,
    /// Date du fichier le plus récent ; à défaut de fichier, celle du dossier lui-même
    recent: Option<SystemTime>,
    illisibles: u64,
}

/// Un élément de premier niveau d'une racine, mesuré
struct Element {
    chemin: PathBuf,
    est_fichier: bool,
    mesure: Mesure,
    date: SystemTime,
}

/// Parcourt un dossier sans jamais suivre une jonction ni un lien symbolique, sans récursion (pas de dépassement de pile)
fn parcourir(racine: &Path, m: &mut Mesure) {
    let mut pile = vec![racine.to_path_buf()];
    while let Some(d) = pile.pop() {
        let Ok(it) = std::fs::read_dir(&d) else {
            m.illisibles += 1;
            continue;
        };
        for e in it {
            let Ok(e) = e else {
                m.illisibles += 1;
                continue;
            };
            // `DirEntry::metadata` ne suit pas les liens sous Windows
            let Ok(meta) = e.metadata() else {
                m.illisibles += 1;
                continue;
            };
            if meta.file_attributes() & ATTRIBUT_REPARSE != 0 {
                continue;
            }
            if meta.is_dir() {
                pile.push(e.path());
            } else {
                m.octets += meta.len();
                m.fichiers += 1;
                if let Ok(t) = meta.modified() {
                    m.recent = Some(m.recent.map_or(t, |r| r.max(t)));
                }
            }
        }
    }
}

/// Entrées de premier niveau d'une racine, mesurées. `seulement_fichiers` évite de descendre dans les sous-dossiers
/// quand la règle ne s'intéresse qu'aux fichiers de la racine.
fn elements(racine: &Path, seulement_fichiers: bool) -> (Vec<Element>, u64) {
    let mut v = Vec::new();
    let mut illisibles = 0;
    let Ok(it) = std::fs::read_dir(racine) else { return (v, 1) };
    for e in it {
        let Ok(e) = e else {
            illisibles += 1;
            continue;
        };
        let Ok(meta) = e.metadata() else {
            illisibles += 1;
            continue;
        };
        if meta.file_attributes() & ATTRIBUT_REPARSE != 0 {
            continue;
        }
        let date = meta.modified().unwrap_or(SystemTime::UNIX_EPOCH);
        if meta.is_dir() {
            if seulement_fichiers {
                continue;
            }
            let mut mesure = Mesure::default();
            parcourir(&e.path(), &mut mesure);
            mesure.recent = Some(mesure.recent.unwrap_or(date));
            v.push(Element { chemin: e.path(), est_fichier: false, mesure, date });
        } else {
            let mesure = Mesure { octets: meta.len(), fichiers: 1, recent: Some(date), illisibles: 0 };
            v.push(Element { chemin: e.path(), est_fichier: true, mesure, date });
        }
    }
    (v, illisibles)
}

/// Mesure un modèle fondé sur des dossiers. `None` si aucune racine n'existe ou n'est utilisable, ou si la catégorie
/// est sous son seuil : elle n'est alors pas affichée.
fn mesurer_dossiers(m: &Modele, r: &Racines, maintenant: SystemTime) -> Option<Categorie> {
    let mut cat = Categorie { id: m.id, libelle: m.libelle, risque: m.risque, mode: m.mode, octets: 0, fichiers: 0, elements: 0, illisibles: 0, anormal: false, chemins: Vec::new() };
    let mut trouvee = false;
    for racine in racines_valides(m, r) {
        // La racine elle-même ne doit être ni absente ni une jonction : on ne descend pas dans un lien
        let Ok(meta) = std::fs::symlink_metadata(&racine) else { continue };
        if !meta.is_dir() || meta.file_attributes() & ATTRIBUT_REPARSE != 0 {
            continue;
        }
        trouvee = true;
        let (mut liste, illisibles) = elements(&racine, matches!(m.regle, Regle::DumpsSaufRecents(_)));
        cat.illisibles += illisibles;
        match m.regle {
            Regle::Tout => {}
            Regle::PlusVieuxQue(jours) => {
                let limite = maintenant.checked_sub(Duration::from_secs(jours * 86_400)).unwrap_or(SystemTime::UNIX_EPOCH);
                liste.retain(|e| e.mesure.recent.unwrap_or(e.date) < limite);
            }
            Regle::DumpsSaufRecents(gardes) => {
                liste.retain(|e| e.est_fichier && est_dmp(&e.chemin));
                liste.sort_by(|a, b| b.date.cmp(&a.date));
                liste.drain(..gardes.min(liste.len()));
            }
            Regle::CorbeilleWindows => return None,
        }
        for e in &liste {
            cat.octets += e.mesure.octets;
            cat.fichiers += e.mesure.fichiers;
            cat.illisibles += e.mesure.illisibles;
        }
        cat.elements += liste.len() as u64;
        cat.chemins.extend(liste.iter().map(|e| e.chemin.clone()));
    }
    if !trouvee || cat.octets < m.seuil.max(1) {
        return None;
    }
    cat.anormal = m.anormal.is_some_and(|s| cat.octets >= s);
    Some(cat)
}

/// Octets et éléments de la Corbeille de tous les lecteurs, par l'API du Shell
pub fn taille_corbeille() -> Option<(u64, u64)> {
    let mut info = SHQUERYRBINFO { cbSize: std::mem::size_of::<SHQUERYRBINFO>() as u32, ..Default::default() };
    // SAFETY: `info` est une structure locale dont `cbSize` est renseigné ; un chemin nul demande tous les lecteurs
    unsafe { SHQueryRecycleBinW(PCWSTR::null(), &mut info) }.ok()?;
    Some((info.i64Size.max(0) as u64, info.i64NumItems.max(0) as u64))
}

/// Mesure tous les modèles ; ne garde que les catégories présentes sur ce PC et au-dessus de leur seuil, la plus
/// grosse d'abord. `corbeille` donne (octets, éléments) de la Corbeille Windows : injectable pour les tests.
pub fn mesurer_tout(r: &Racines, maintenant: SystemTime, corbeille: impl FnOnce() -> Option<(u64, u64)>) -> Vec<Categorie> {
    let mut v = Vec::new();
    for m in &MODELES {
        if m.regle == Regle::CorbeilleWindows {
            continue;
        }
        v.extend(mesurer_dossiers(m, r, maintenant));
    }
    if let (Some(m), Some((octets, elements))) = (MODELES.iter().find(|m| m.regle == Regle::CorbeilleWindows), corbeille()) {
        if elements > 0 && octets >= m.seuil {
            v.push(Categorie { id: m.id, libelle: m.libelle, risque: m.risque, mode: m.mode, octets, fichiers: elements, elements, illisibles: 0, anormal: false, chemins: Vec::new() });
        }
    }
    v.sort_by(|a, b| b.octets.cmp(&a.octets));
    v
}

/// Nombre avec une espace entre les milliers (« 3 377 »)
fn milliers(n: u64) -> String {
    let s = n.to_string();
    let mut r = String::with_capacity(s.len() + s.len() / 3);
    for (i, ch) in s.chars().enumerate() {
        if i > 0 && (s.len() - i) % 3 == 0 {
            r.push(' ');
        }
        r.push(ch);
    }
    r
}

/// « 3 377 éléments · 580 fichiers », accordé au singulier ; un seul compte quand les deux sont égaux (la Corbeille
/// ne donne qu'un nombre d'éléments, et le répéter en « fichiers » n'apprendrait rien)
pub fn detail_comptes(elements: u64, fichiers: u64) -> String {
    let el = format!("{} {}", milliers(elements), if elements > 1 { "éléments" } else { "élément" });
    if elements == fichiers {
        return el;
    }
    format!("{el} · {} {}", milliers(fichiers), if fichiers > 1 { "fichiers" } else { "fichier" })
}

/// Résultat de la mesure en cours d'acheminement vers la fenêtre
static RESULTAT: Mutex<Option<Vec<Categorie>>> = Mutex::new(None);

/// Lance la mesure dans un fil de fond à basse priorité ; la fenêtre reçoit `WM_NETTOYAGE` quand c'est prêt
/// Rend faux si le fil n'a pas pu démarrer : l'appelant baisse alors son drapeau « en cours », sinon la page attendrait
/// pour toujours.
pub fn lancer(hwnd: HWND) -> bool {
    let h = hwnd.0 as isize;
    std::thread::Builder::new().name("nettoyage".into()).spawn(move || {
        // SAFETY: le pseudo-handle du fil courant est toujours valide
        unsafe {
            let _ = SetThreadPriority(GetCurrentThread(), THREAD_PRIORITY_BELOW_NORMAL);
        }
        let r = mesurer_tout(&Racines::systeme(), SystemTime::now(), taille_corbeille);
        *RESULTAT.lock().unwrap_or_else(|e| e.into_inner()) = Some(r);
        // SAFETY: PostMessageW est sûr depuis n'importe quel fil ; si la fenêtre n'existe plus, l'appel échoue sans effet
        unsafe {
            let _ = PostMessageW(HWND(h as *mut _), WM_NETTOYAGE, WPARAM(0), LPARAM(0));
        }
    })
    .is_ok()
}

/// Reprend le résultat posté par `lancer`
pub fn prendre() -> Option<Vec<Categorie>> {
    RESULTAT.lock().unwrap_or_else(|e| e.into_inner()).take()
}

/// Total des catégories mesurées (octets)
pub fn total(c: &[Categorie]) -> u64 {
    c.iter().map(|x| x.octets).sum()
}

// ───────────────────────── Suppression (E4b) ─────────────────────────

/// Ce qui est arrivé à une catégorie demandée
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BilanCategorie {
    pub id: &'static str,
    pub libelle: &'static str,
    pub mode: Mode,
    /// Éléments effectivement disparus de leur emplacement
    pub supprimes: u64,
    /// Octets de ces éléments (libérés pour DIRECT et DÉFINITIF ; seulement déplacés pour CORBEILLE)
    pub octets: u64,
    /// Éléments qu'on n'a pas pu effacer (fichier verrouillé, droit refusé) ou que leur règle ne propose plus (devenus
    /// récents) : ignorés, jamais une erreur
    pub en_usage: u64,
    /// Éléments que la porte a refusés au dernier moment (hors liste blanche, jonction apparue entre-temps)
    pub refuses: u64,
}

/// Résultat d'une suppression : une ligne par catégorie demandée
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Bilan {
    pub categories: Vec<BilanCategorie>,
}

impl Bilan {
    pub fn supprimes(&self) -> u64 {
        self.categories.iter().map(|c| c.supprimes).sum()
    }

    /// Éléments laissés en place (en usage, plus proposés, ou refusés par la porte)
    pub fn ignores(&self) -> u64 {
        self.categories.iter().map(|c| c.en_usage + c.refuses).sum()
    }

    /// Octets réellement rendus au disque (hors CORBEILLE, où les fichiers existent toujours)
    pub fn octets_liberes(&self) -> u64 {
        self.categories.iter().filter(|c| c.mode != Mode::Corbeille).map(|c| c.octets).sum()
    }

    /// Octets envoyés à la Corbeille (récupérables ; le disque ne les retrouve qu'au vidage)
    pub fn octets_corbeille(&self) -> u64 {
        self.categories.iter().filter(|c| c.mode == Mode::Corbeille).map(|c| c.octets).sum()
    }

    /// « 12 éléments supprimés · 1,2 Go libérés · 3 éléments ignorés (en cours d'usage) »
    fn resume(&self) -> String {
        let s = self.supprimes();
        let mut p = vec![format!("{} {}", milliers(s), if s > 1 { "éléments supprimés" } else { "élément supprimé" })];
        if self.octets_liberes() > 0 {
            p.push(format!("{} libérés", crate::systeme::taille(self.octets_liberes())));
        }
        if self.octets_corbeille() > 0 {
            p.push(format!("{} envoyés à la Corbeille", crate::systeme::taille(self.octets_corbeille())));
        }
        let i = self.ignores();
        if i > 0 {
            p.push(format!("{} {} (en cours d'usage)", milliers(i), if i > 1 { "éléments ignorés" } else { "élément ignoré" }));
        }
        p.join(" · ")
    }

    /// Phrase montrée sur la page après le nettoyage
    pub fn message(&self) -> String {
        match (self.supprimes(), self.ignores()) {
            (0, 0) => "Rien à supprimer".to_string(),
            (0, i) => format!("Rien n'a pu être supprimé · {} {} (en cours d'usage)", milliers(i), if i > 1 { "éléments ignorés" } else { "élément ignoré" }),
            _ => format!("Nettoyage terminé · {}", self.resume()),
        }
    }

    /// Entrée de l'historique des opérations : (titre, détail)
    pub fn journal(&self) -> (String, String) {
        let noms: Vec<&str> = self.categories.iter().map(|c| c.libelle).collect();
        (format!("Nettoyage · {}", noms.join(", ")), self.resume())
    }
}

/// Écrit le bilan dans l'historique des opérations et dans le journal texte. L'entrée n'a pas de bouton « Annuler » :
/// le Shell ne sait pas défaire un vidage, et un cache effacé se recrée tout seul.
pub fn journaliser(b: &Bilan) {
    let (titre, detail) = b.journal();
    crate::journal::operation(&titre, &detail);
    let id = crate::journal_historique::enregistrer(&titre, &detail);
    crate::journal_historique::marquer(id, crate::journal_historique::Etat::Passe);
}

/// Sort d'un élément après la porte et l'effacement
#[derive(Debug, PartialEq, Eq)]
enum Issue {
    Supprime,
    /// Déjà parti : rien à compter
    Absent,
    EnUsage,
    Refuse,
}

/// Retire un fichier ; un fichier en lecture seule perd son attribut puis on réessaie une fois. Faux si c'est impossible
/// (verrouillé par un programme, droit refusé).
fn retirer_fichier(p: &Path) -> bool {
    if std::fs::remove_file(p).is_ok() {
        return true;
    }
    // `symlink_metadata` : on ne lit jamais les droits de la cible d'un lien
    match std::fs::symlink_metadata(p) {
        Ok(m) if m.permissions().readonly() => {
            let origine = m.permissions();
            let mut perm = origine.clone();
            #[allow(clippy::permissions_set_readonly_false)] // Windows seulement : l'attribut, pas des bits unix
            perm.set_readonly(false);
            if std::fs::set_permissions(p, perm).is_err() {
                return false;
            }
            if std::fs::remove_file(p).is_ok() {
                return true;
            }
            // Resté (verrouillé) : il retrouve son attribut, on ne laisse pas un fichier modifié derrière nous
            let _ = std::fs::set_permissions(p, origine);
            false
        }
        _ => false,
    }
}

/// Vrai si `d` est, à cet instant, un vrai dossier (ni jonction, ni lien symbolique, ni point de montage)
fn est_vrai_dossier(d: &Path) -> bool {
    std::fs::symlink_metadata(d).is_ok_and(|m| m.is_dir() && m.file_attributes() & ATTRIBUT_REPARSE == 0)
}

/// Efface un dossier et tout son contenu sans jamais suivre un lien : une jonction ou un lien symbolique rencontré est
/// retiré lui-même (`RemoveDirectory` sur une jonction ne touche pas à sa cible) et on ne descend pas dedans. Sans
/// récursion. Vrai si le dossier a disparu ; `octets` additionne ce qui a été effacé, même en cas d'échec partiel.
fn effacer_direct(racine: &Path, octets: &mut u64) -> bool {
    let mut complet = true;
    let mut dossiers = Vec::new();
    let mut pile = vec![racine.to_path_buf()];
    while let Some(d) = pile.pop() {
        // Course possible entre la vérification et ce parcours : un dossier remplacé par une jonction dans l'intervalle ne
        // doit pas être énuméré (`read_dir` la suivrait). La fenêtre est réduite à l'instant qui sépare ce contrôle de
        // l'ouverture, pas supprimée : l'ouverture par handle sans suivre les liens serait la fermeture complète. Sous
        // les droits de l'utilisateur, l'auteur d'une telle course peut déjà tout effacer lui-même.
        if !est_vrai_dossier(&d) {
            // Devenu un lien : on retire le lien lui-même (jamais sa cible), sinon on le laisse
            if std::fs::symlink_metadata(&d).is_ok_and(|m| m.file_attributes() & ATTRIBUT_REPARSE != 0) {
                complet &= std::fs::remove_dir(&d).is_ok();
            } else {
                complet = false;
            }
            continue;
        }
        dossiers.push(d.clone());
        let Ok(it) = std::fs::read_dir(&d) else {
            complet = false;
            continue;
        };
        for e in it {
            let Ok(e) = e else {
                complet = false;
                continue;
            };
            let p = e.path();
            let Ok(meta) = e.metadata() else {
                complet = false;
                continue;
            };
            let attr = meta.file_attributes();
            if attr & ATTRIBUT_REPARSE != 0 {
                // Dossier-lien (0x10) : `remove_dir` ; fichier-lien : `remove_file`. Jamais la cible.
                let ok = if attr & 0x10 != 0 { std::fs::remove_dir(&p).is_ok() } else { std::fs::remove_file(&p).is_ok() };
                complet &= ok;
            } else if meta.is_dir() {
                pile.push(p);
            } else if retirer_fichier(&p) {
                *octets += meta.len();
            } else {
                complet = false;
            }
        }
    }
    // Un dossier est toujours rangé après son parent : à l'envers, les enfants partent avant leurs parents
    for d in dossiers.iter().rev() {
        complet &= std::fs::remove_dir(d).is_ok();
    }
    complet
}

/// Vrai si `chemin` (un `.dmp`) est l'un des `n` plus récents de son dossier, ou si on ne peut pas le savoir : dans le doute
/// on garde.
fn parmi_les_recents(chemin: &Path, n: usize) -> bool {
    let Some(parent) = chemin.parent() else { return true };
    let Ok(it) = std::fs::read_dir(parent) else { return true };
    let mut v: Vec<(SystemTime, PathBuf)> = it
        .flatten()
        .filter(|e| est_dmp(&e.path()))
        .filter_map(|e| {
            let m = e.metadata().ok()?;
            m.is_file().then(|| (m.modified().unwrap_or(SystemTime::UNIX_EPOCH), e.path()))
        })
        .collect();
    v.sort_by(|a, b| b.0.cmp(&a.0));
    let cible = composants(chemin);
    v.iter().take(n).any(|(_, p)| composants(p) == cible)
}

/// La porte, refaite juste avant l'effacement d'un élément : le contenu du disque a pu changer depuis la mesure.
/// Rend ses métadonnées (sans suivre de lien) s'il peut être effacé, sinon ce qu'il faut en dire.
fn verifier(m: &Modele, chemin: &Path, r: &Racines, maintenant: SystemTime) -> Result<std::fs::Metadata, Issue> {
    if !est_autorise(m, chemin, r) {
        return Err(Issue::Refuse);
    }
    let meta = match std::fs::symlink_metadata(chemin) {
        Ok(x) => x,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Err(Issue::Absent),
        Err(_) => return Err(Issue::EnUsage),
    };
    if meta.file_attributes() & ATTRIBUT_REPARSE != 0 {
        return Err(Issue::Refuse);
    }
    // La règle qui l'avait fait proposer doit être toujours vraie : un fichier de `Temp` touché depuis la mesure sert
    // peut-être en ce moment ; un rapport de plantage tout neuf fait sortir un ancien des deux gardés.
    let toujours_propose = match m.regle {
        Regle::Tout => true,
        Regle::PlusVieuxQue(jours) => {
            let limite = maintenant.checked_sub(Duration::from_secs(jours * 86_400)).unwrap_or(SystemTime::UNIX_EPOCH);
            let date = meta.modified().unwrap_or(SystemTime::UNIX_EPOCH);
            let mut me = Mesure::default();
            if meta.is_dir() {
                parcourir(chemin, &mut me);
            }
            me.recent.unwrap_or(date) < limite
        }
        Regle::DumpsSaufRecents(n) => meta.is_file() && est_dmp(chemin) && !parmi_les_recents(chemin, n),
        Regle::CorbeilleWindows => false,
    };
    if toujours_propose {
        Ok(meta)
    } else {
        Err(Issue::EnUsage)
    }
}

/// Efface un élément de premier niveau d'une catégorie : porte, puis le mode du modèle. Rend le sort et les octets
/// libérés.
fn supprimer_element(m: &Modele, chemin: &Path, r: &Racines, maintenant: SystemTime, corbeille: &dyn Fn(&Path) -> bool) -> (Issue, u64) {
    let meta = match verifier(m, chemin, r, maintenant) {
        Ok(x) => x,
        Err(i) => return (i, 0),
    };
    let mut octets = 0;
    let parti = match m.mode {
        Mode::Direct => {
            if meta.is_dir() {
                effacer_direct(chemin, &mut octets);
            } else if retirer_fichier(chemin) {
                octets = meta.len();
            }
            // Un élément compte comme supprimé quand son chemin a disparu, pas quand une fonction l'a dit
            std::fs::symlink_metadata(chemin).is_err()
        }
        Mode::Corbeille => {
            let mut me = Mesure::default();
            if meta.is_dir() {
                parcourir(chemin, &mut me);
                octets = me.octets;
            } else {
                octets = meta.len();
            }
            let _ = corbeille(chemin);
            std::fs::symlink_metadata(chemin).is_err()
        }
        // Un modèle définitif n'a pas de chemin : la porte l'a déjà refusé
        Mode::Definitif => return (Issue::Refuse, 0),
    };
    if parti {
        (Issue::Supprime, octets)
    } else if m.mode == Mode::Direct {
        // Effacement direct partiel : ce qui est parti l'est vraiment, on le compte
        (Issue::EnUsage, octets)
    } else {
        // Corbeille : un élément resté en place n'a libéré aucun octet (le renommage échoue en bloc)
        (Issue::EnUsage, 0)
    }
}

/// Efface ce que l'utilisateur a confirmé. `corbeille` envoie un élément à la Corbeille, `vider` vide la Corbeille
/// Windows, `taille_corbeille` la mesure : injectables, pour qu'un test ne touche jamais à la vraie Corbeille. Rien ici ne
/// s'arrête sur une erreur : ce qui ne peut pas partir est compté et le reste continue.
pub fn supprimer(
    demandes: &[Categorie],
    r: &Racines,
    maintenant: SystemTime,
    corbeille: &dyn Fn(&Path) -> bool,
    vider: &dyn Fn() -> bool,
    taille_corbeille: &dyn Fn() -> Option<(u64, u64)>,
) -> Bilan {
    let mut bilan = Bilan::default();
    PROG_FAIT.store(0, Ordering::Relaxed);
    PROG_TOTAL.store(demandes.iter().map(|d| d.chemins.len().max(1)).sum(), Ordering::Relaxed);
    for d in demandes {
        // Un identifiant inconnu n'a pas de règle : on n'efface rien
        let Some(m) = MODELES.iter().find(|m| m.id == d.id) else { continue };
        let mut b = BilanCategorie { id: m.id, libelle: m.libelle, mode: m.mode, supprimes: 0, octets: 0, en_usage: 0, refuses: 0 };
        if m.regle == Regle::CorbeilleWindows {
            let avant = taille_corbeille();
            let ok = vider();
            let apres = taille_corbeille();
            match (avant, apres) {
                (Some((o, n)), Some((o2, n2))) => {
                    b.supprimes = n.saturating_sub(n2);
                    b.octets = o.saturating_sub(o2);
                    // Ce qui reste après le vidage (un lecteur déconnecté, un fichier verrouillé) est ignoré
                    b.en_usage = n2;
                }
                // Seconde mesure impossible : on s'en remet au résultat de l'API, sans compter deux fois
                (Some((o, n)), None) => {
                    if ok {
                        b.supprimes = n;
                        b.octets = o;
                    } else {
                        b.en_usage = n.max(d.elements);
                    }
                }
                (None, Some((_, n2))) => b.en_usage = n2,
                (None, None) => b.en_usage = if ok { 0 } else { d.elements },
            }
        } else {
            for chemin in &d.chemins {
                let (issue, octets) = supprimer_element(m, chemin, r, maintenant, corbeille);
                PROG_FAIT.fetch_add(1, Ordering::Relaxed);
                b.octets += octets;
                match issue {
                    Issue::Supprime => b.supprimes += 1,
                    Issue::Absent => {}
                    Issue::EnUsage => b.en_usage += 1,
                    Issue::Refuse => b.refuses += 1,
                }
            }
        }
        if m.regle == Regle::CorbeilleWindows {
            PROG_FAIT.fetch_add(1, Ordering::Relaxed);
        }
        bilan.categories.push(b);
    }
    bilan
}

/// UTF-16 terminé par un zéro, pour l'API du Shell
fn large(p: &Path) -> Vec<u16> {
    p.as_os_str().encode_wide().chain(std::iter::once(0)).collect()
}

/// Envoie un élément à la Corbeille par `IFileOperation` (annulable depuis l'Explorateur). Il faut que le fil ait appelé
/// `CoInitializeEx`. Pas de fenêtre ni de message d'erreur de Windows ; `FOF_WANTNUKEWARNING` garde la question de
/// Windows quand la Corbeille est impossible (élément trop gros), plutôt qu'une suppression définitive en silence : la
/// question reste à l'utilisateur, et un refus compte comme « en cours d'usage ».
///
/// # Safety
/// À appeler depuis un fil où COM est initialisé (`CoInitializeEx`).
///
/// Aucune fenêtre propriétaire : avec la fenêtre de l'application pour propriétaire, la question de Windows (élément
/// trop gros pour la Corbeille, par exemple le cache de shaders de 25 Go) la rendait modale, donc désactivait la fenêtre
/// de l'auteur — plus de clic ni de clavier, sans qu'on voie la question (constat du 19/11/2026). Sans propriétaire,
/// la question reste une fenêtre indépendante et SpaceFiles garde la main.
unsafe fn vers_corbeille(_hwnd: HWND, chemin: &Path) -> bool {
    let essai = || -> windows::core::Result<bool> {
        let op: IFileOperation = CoCreateInstance(&FileOperation, None, CLSCTX_ALL)?;
        let drapeaux = FOF_SILENT.0
            | FOF_NOCONFIRMMKDIR.0
            | FOF_ALLOWUNDO.0
            | FOFX_RECYCLEONDELETE.0
            | FOF_NOCONFIRMATION.0
            | FOF_WANTNUKEWARNING.0
            | FOF_NOERRORUI.0;
        op.SetOperationFlags(FILEOPERATION_FLAGS(drapeaux))?;
        let w = large(chemin);
        let item: IShellItem = SHCreateItemFromParsingName(PCWSTR(w.as_ptr()), None)?;
        op.DeleteItem(&item, None)?;
        op.PerformOperations()?;
        Ok(!op.GetAnyOperationsAborted()?.as_bool())
    };
    essai().unwrap_or(false)
}

/// Vide la Corbeille Windows de tous les lecteurs, sans confirmation ni progression de Windows (l'utilisateur vient de
/// confirmer ici, avec la taille exacte)
///
/// # Safety
/// `hwnd` est valide ou nul, et l'appel se fait depuis un fil de fond : aucune interface n'est affichée
/// (`SHERB_NOCONFIRMATION | SHERB_NOPROGRESSUI`), donc la fenêtre d'un autre fil ne peut pas bloquer l'appel.
unsafe fn vider_corbeille(hwnd: HWND) -> bool {
    // SHERB_NOCONFIRMATION | SHERB_NOPROGRESSUI | SHERB_NOSOUND
    SHEmptyRecycleBinW(hwnd, PCWSTR::null(), 1 | 2 | 4).is_ok()
}

/// Une suppression à la fois : ni deux fils, ni une re-mesure par-dessus
static SUPPRESSION_EN_COURS: AtomicBool = AtomicBool::new(false);
/// Bilan de la suppression en cours d'acheminement vers la fenêtre
static BILAN: Mutex<Option<Bilan>> = Mutex::new(None);

/// Vrai tant qu'un fil de suppression tourne
pub fn suppression_en_cours() -> bool {
    SUPPRESSION_EN_COURS.load(Ordering::Acquire)
}

/// Éléments traités et à traiter par la suppression en cours (pour dire « 120 / 894 » : sans cela, une longue suppression
/// ressemble à un gel). Chaque `supprimer` les remet à jour ; un test en parallèle ne fausse que cet affichage.
static PROG_FAIT: AtomicUsize = AtomicUsize::new(0);
static PROG_TOTAL: AtomicUsize = AtomicUsize::new(0);

/// (traités, total) de la suppression en cours
pub fn progres() -> (usize, usize) {
    (PROG_FAIT.load(Ordering::Relaxed), PROG_TOTAL.load(Ordering::Relaxed))
}

/// La fermeture a déjà été refusée une fois pendant cette suppression
static FERMETURE_REFUSEE: AtomicBool = AtomicBool::new(false);

/// Premier appel : faux (la fermeture est refusée et on le dit) ; les suivants, vrai (la fermeture passe). Remis à zéro
/// par `prendre_bilan`, donc une nouvelle suppression protège de nouveau la fermeture.
pub fn fermeture_deja_refusee() -> bool {
    FERMETURE_REFUSEE.swap(true, Ordering::AcqRel)
}

/// Garde du fil de suppression : libère COM à la sortie du fil, et le drapeau « en cours » si le bilan n'a pas été posté
/// (panique avec `unwind`) ; sinon c'est `prendre_bilan` qui le baisse.
struct GardeSuppression {
    com: bool,
    bilan_poste: bool,
}

impl Drop for GardeSuppression {
    fn drop(&mut self) {
        if self.com {
            // SAFETY: appelé sur le fil qui a réussi `CoInitializeEx` ; aucun objet COM ne lui survit (ils sont locaux à
            // `vers_corbeille`, relâchés avant le retour de `supprimer`)
            unsafe { CoUninitialize() };
        }
        if !self.bilan_poste {
            SUPPRESSION_EN_COURS.store(false, Ordering::Release);
        }
    }
}

/// Lance la suppression des catégories confirmées dans un fil de fond à basse priorité (COM en STA : `IFileOperation`) ;
/// la fenêtre reçoit `WM_NETTOYAGE_FINI`. Faux si un fil tourne déjà ou n'a pas pu démarrer.
pub fn lancer_suppression(hwnd: HWND, demandes: Vec<Categorie>) -> bool {
    if SUPPRESSION_EN_COURS.swap(true, Ordering::AcqRel) {
        return false;
    }
    let h = hwnd.0 as isize;
    let demarre = std::thread::Builder::new()
        .name("nettoyage-suppression".into())
        .spawn(move || {
            let fenetre = HWND(h as *mut _);
            // SAFETY: tout ce bloc vit sur ce fil : le pseudo-handle du fil est valide ; COM est initialisé avant son
            // premier usage et libéré après le dernier (aucun objet COM ne survit à la fin de `supprimer`) ; PostMessageW
            // est sûr depuis n'importe quel fil, et échoue sans effet si la fenêtre n'existe plus.
            unsafe {
                let _ = SetThreadPriority(GetCurrentThread(), THREAD_PRIORITY_BELOW_NORMAL);
                let com = CoInitializeEx(None, COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE).is_ok();
                // Rend COM et, en cas de panique (tests et debug : `unwind`), le drapeau « en cours », quoi qu'il arrive
                let mut garde = GardeSuppression { com, bilan_poste: false };
                let b = supprimer(
                    &demandes,
                    &Racines::systeme(),
                    SystemTime::now(),
                    &|p| {
                        let ok = vers_corbeille(fenetre, p);
                        // La page lit `progres` à son prochain dessin
                        let _ = PostMessageW(fenetre, WM_NETTOYAGE_PROGRES, WPARAM(0), LPARAM(0));
                        ok
                    },
                    &|| vider_corbeille(fenetre),
                    &taille_corbeille,
                );
                *BILAN.lock().unwrap_or_else(|e| e.into_inner()) = Some(b);
                // Le drapeau reste levé jusqu'à `prendre_bilan` (côté fenêtre) : aucune re-mesure ne peut partir entre la
                // fin de l'effacement et la réception du bilan
                let _ = PostMessageW(fenetre, WM_NETTOYAGE_FINI, WPARAM(0), LPARAM(0));
                garde.bilan_poste = true;
            }
        })
        .is_ok();
    if !demarre {
        SUPPRESSION_EN_COURS.store(false, Ordering::Release);
    }
    demarre
}

/// Reprend le bilan posté par `lancer_suppression` (fil d'interface, à la réception de `WM_NETTOYAGE_FINI`) et baisse le
/// drapeau « en cours » : à partir d'ici une nouvelle mesure ou suppression peut partir.
pub fn prendre_bilan() -> Option<Bilan> {
    let b = BILAN.lock().unwrap_or_else(|e| e.into_inner()).take();
    SUPPRESSION_EN_COURS.store(false, Ordering::Release);
    FERMETURE_REFUSEE.store(false, Ordering::Release);
    b
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    /// Faux profil dans un dossier temporaire de test : jamais de vraie donnée touchée
    struct Faux {
        base: PathBuf,
        r: Racines,
    }

    impl Faux {
        fn nouveau(nom: &str) -> Self {
            let base = std::env::temp_dir().join(format!("ne-nettoyage-{}-{nom}", std::process::id()));
            let _ = fs::remove_dir_all(&base);
            let profil = base.join("Users").join("test");
            let local = profil.join("AppData").join("Local");
            let temp = local.join("Temp");
            fs::create_dir_all(&temp).unwrap();
            Self { base, r: Racines { profil: Some(profil), local: Some(local), temp: Some(temp) } }
        }

        fn local(&self) -> &Path {
            self.r.local.as_deref().unwrap()
        }

        fn temp(&self) -> &Path {
            self.r.temp.as_deref().unwrap()
        }

        /// Crée un fichier de `octets` octets daté de `jours` jours avant `MAINTENANT`
        fn fichier(&self, chemin: &Path, octets: usize, jours: u64) {
            fs::create_dir_all(chemin.parent().unwrap()).unwrap();
            fs::write(chemin, vec![7u8; octets]).unwrap();
            let f = fs::OpenOptions::new().write(true).open(chemin).unwrap();
            f.set_modified(maintenant() - Duration::from_secs(jours * 86_400)).unwrap();
        }
    }

    impl Drop for Faux {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.base);
        }
    }

    /// Une date fixe pour tous les tests : « maintenant »
    fn maintenant() -> SystemTime {
        static T: std::sync::OnceLock<SystemTime> = std::sync::OnceLock::new();
        *T.get_or_init(SystemTime::now)
    }

    fn mesurer(f: &Faux) -> Vec<Categorie> {
        mesurer_tout(&f.r, maintenant(), || None)
    }

    fn ids(v: &[Categorie]) -> Vec<&'static str> {
        v.iter().map(|c| c.id).collect()
    }

    fn modele(id: &str) -> &'static Modele {
        MODELES.iter().find(|m| m.id == id).unwrap()
    }

    #[test]
    fn les_modeles_sont_coherents() {
        let mut vus = std::collections::HashSet::new();
        for m in &MODELES {
            assert!(vus.insert(m.id), "identifiant en double : {}", m.id);
            for (_, sous) in m.racines {
                // Un chemin relatif sous la base : ni absolu, ni remontée, ni lecteur
                assert!(!sous.contains("..") && !sous.contains(':') && !sous.starts_with('\\'), "racine douteuse : {sous}");
            }
            // La Corbeille Windows est le seul modèle sans dossier, et le seul définitif
            assert_eq!(m.racines.is_empty(), m.regle == Regle::CorbeilleWindows);
            assert_eq!(m.mode == Mode::Definitif, m.regle == Regle::CorbeilleWindows);
            assert!(m.seuil >= 1);
        }
        assert_eq!(MODELES.len(), 7);
    }

    #[test]
    fn temp_ne_propose_que_ce_qui_a_plus_de_sept_jours() {
        let f = Faux::nouveau("temp");
        // Dossier dont le fichier le plus récent a 10 jours : proposé ; un autre qui a un fichier de 1 jour : gardé tout entier
        f.fichier(&f.temp().join("ancien").join("a.bin"), 2 * MO as usize, 10);
        f.fichier(&f.temp().join("ancien").join("sous").join("b.bin"), 100, 30);
        f.fichier(&f.temp().join("mixte").join("vieux.bin"), 3 * MO as usize, 40);
        f.fichier(&f.temp().join("mixte").join("neuf.bin"), 10, 1);
        f.fichier(&f.temp().join("vieux.log"), MO as usize, 8);
        f.fichier(&f.temp().join("recent.log"), 5 * MO as usize, 3);
        let v = mesurer(&f);
        assert_eq!(ids(&v), vec!["temp"]);
        let c = &v[0];
        assert_eq!(c.octets, 2 * MO + 100 + MO);
        assert_eq!(c.fichiers, 3);
        assert_eq!(c.elements, 2);
        assert_eq!(c.mode, Mode::Corbeille);
        assert!(!c.anormal);
    }

    #[test]
    fn une_categorie_sous_son_seuil_ou_vide_n_est_pas_affichee() {
        let f = Faux::nouveau("seuil");
        // Dossier vide, puis quelques octets seulement : rien à montrer
        fs::create_dir_all(f.local().join("D3DSCache")).unwrap();
        assert!(mesurer(&f).is_empty());
        f.fichier(&f.local().join("D3DSCache").join("petit.bin"), 1000, 0);
        assert!(mesurer(&f).is_empty());
        f.fichier(&f.local().join("D3DSCache").join("gros.bin"), 2 * MO as usize, 0);
        assert_eq!(ids(&mesurer(&f)), vec!["d3d"]);
    }

    #[test]
    fn les_caches_de_pilote_n_apparaissent_que_si_le_pilote_est_la() {
        // Sans carte NVIDIA, la ligne NVIDIA n'existe pas
        let f = Faux::nouveau("sans-nvidia");
        f.fichier(&f.local().join("D3DSCache").join("x.bin"), 2 * MO as usize, 0);
        assert_eq!(ids(&mesurer(&f)), vec!["d3d"]);
        // Avec NVIDIA : deux dossiers additionnés, ligne propre ; AMD absent reste absent
        let g = Faux::nouveau("avec-nvidia");
        g.fichier(&g.local().join("NVIDIA").join("DXCache").join("a.bin"), 3 * MO as usize, 0);
        g.fichier(&g.local().join("NVIDIA").join("GLCache").join("b").join("c.bin"), 2 * MO as usize, 0);
        let v = mesurer(&g);
        assert_eq!(ids(&v), vec!["nvidia"]);
        assert_eq!(v[0].octets, 5 * MO);
        assert_eq!(v[0].mode, Mode::Direct);
        // AMD et Intel suivent la même règle
        g.fichier(&g.local().join("AMD").join("DxCache").join("a.bin"), 4 * MO as usize, 0);
        g.fichier(&g.local().join("Intel").join("ShaderCache").join("a.bin"), MO as usize, 0);
        assert_eq!(ids(&mesurer(&g)), vec!["nvidia", "amd", "intel"]);
    }

    #[test]
    fn un_cache_de_plusieurs_go_est_signale() {
        let f = Faux::nouveau("anormal");
        // Fichier creux : 3 Go annoncés sans les écrire (set_len)
        let p = f.local().join("D3DSCache").join("enorme.bin");
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::File::create(&p).unwrap().set_len(3 * GO).unwrap();
        let v = mesurer(&f);
        assert_eq!(v[0].id, "d3d");
        assert!(v[0].anormal);
    }

    #[test]
    fn les_rapports_de_plantage_gardent_les_deux_plus_recents() {
        let f = Faux::nouveau("dumps");
        let d = f.local().join("CrashDumps");
        for (i, jours) in [1u64, 2, 5, 9, 20].iter().enumerate() {
            f.fichier(&d.join(format!("p{i}.dmp")), MO as usize, *jours);
        }
        f.fichier(&d.join("lisez-moi.txt"), 10 * MO as usize, 50);
        f.fichier(&d.join("sous").join("z.dmp"), 10 * MO as usize, 50);
        let v = mesurer(&f);
        assert_eq!(ids(&v), vec!["plantages"]);
        // p0 (1 j) et p1 (2 j) gardés ; p2, p3, p4 proposés ; ni le .txt ni le sous-dossier
        assert_eq!(v[0].octets, 3 * MO);
        assert_eq!(v[0].elements, 3);
        // Avec deux rapports seulement : rien à proposer
        let g = Faux::nouveau("dumps2");
        g.fichier(&g.local().join("CrashDumps").join("a.dmp"), 5 * MO as usize, 1);
        g.fichier(&g.local().join("CrashDumps").join("b.dmp"), 5 * MO as usize, 2);
        assert!(mesurer(&g).is_empty());
    }

    #[test]
    fn un_temp_redirige_hors_du_profil_desactive_la_categorie() {
        let mut f = Faux::nouveau("hors-profil");
        // %TEMP% redirigé vers « la racine d'un autre disque » : ici un dossier frère du profil
        let hors = f.base.join("D");
        f.fichier(&hors.join("vieux.bin"), 2 * MO as usize, 30);
        f.r.temp = Some(hors.clone());
        assert!(mesurer(&f).is_empty());
        assert!(racines_valides(modele("temp"), &f.r).is_empty());
        // Le profil lui-même n'est pas une racine permise non plus
        f.r.temp = f.r.profil.clone();
        assert!(racines_valides(modele("temp"), &f.r).is_empty());
        // Sans profil connu, rien n'est autorisé
        f.r.profil = None;
        assert!(racines_valides(modele("d3d"), &f.r).is_empty());
    }

    #[test]
    fn une_jonction_n_est_jamais_suivie() {
        let f = Faux::nouveau("jonction");
        let hors = f.base.join("hors-liste");
        f.fichier(&hors.join("precieux.bin"), 9 * MO as usize, 100);
        f.fichier(&f.local().join("D3DSCache").join("c.bin"), 2 * MO as usize, 0);
        // Jonction dans le cache vers un dossier hors liste (une jonction ne demande aucun droit particulier)
        let lien = f.local().join("D3DSCache").join("lien");
        let ok = std::process::Command::new("cmd").args(["/C", "mklink", "/J"]).arg(&lien).arg(&hors).output().unwrap();
        assert!(ok.status.success(), "mklink /J a échoué : {}", String::from_utf8_lossy(&ok.stdout));
        assert!(lien.join("precieux.bin").exists());
        let v = mesurer(&f);
        assert_eq!(v[0].octets, 2 * MO, "le contenu de la jonction ne doit pas être compté");
        assert_eq!(v[0].elements, 1);
        // Une racine qui est elle-même une jonction est ignorée
        let g = Faux::nouveau("racine-jonction");
        let cible = g.base.join("ailleurs");
        g.fichier(&cible.join("x.bin"), 3 * MO as usize, 0);
        fs::create_dir_all(g.local().join("NVIDIA")).unwrap();
        let ok = std::process::Command::new("cmd").args(["/C", "mklink", "/J"]).arg(g.local().join("NVIDIA").join("DXCache")).arg(&cible).output().unwrap();
        assert!(ok.status.success());
        assert!(mesurer(&g).is_empty());
        // Nettoyage : retirer la jonction avant le dossier, pour que la suppression du faux profil ne descende pas dedans
        let _ = fs::remove_dir(&lien);
        let _ = fs::remove_dir(g.local().join("NVIDIA").join("DXCache"));
    }

    #[test]
    fn une_jonction_dans_un_dossier_parent_desactive_la_categorie() {
        // `NVIDIA` est une jonction vers un dossier hors du profil : `NVIDIA\DXCache` ressemble à un descendant du profil
        // par son texte, mais sort du profil dans les faits. La catégorie est écartée et la porte refuse le chemin.
        let f = Faux::nouveau("jonction-parent");
        let cible = f.base.join("autre-disque").join("NVIDIA");
        f.fichier(&cible.join("DXCache").join("x.bin"), 3 * MO as usize, 0);
        let lien = f.local().join("NVIDIA");
        let ok = std::process::Command::new("cmd").args(["/C", "mklink", "/J"]).arg(&lien).arg(&cible).output().unwrap();
        assert!(ok.status.success(), "mklink /J a échoué : {}", String::from_utf8_lossy(&ok.stdout));
        assert!(mesurer(&f).is_empty(), "rien ne doit être mesuré derrière une jonction du profil");
        let m = modele("nvidia");
        assert!(racines_valides(m, &f.r).is_empty());
        assert!(!est_autorise(m, &lien.join("DXCache").join("x.bin"), &f.r));
        let _ = fs::remove_dir(&lien);
    }

    #[test]
    fn les_nombres_sont_ecrits_avec_des_espaces_et_accordes() {
        assert_eq!(milliers(0), "0");
        assert_eq!(milliers(999), "999");
        assert_eq!(milliers(1000), "1 000");
        assert_eq!(milliers(3377), "3 377");
        assert_eq!(milliers(1_234_567), "1 234 567");
        assert_eq!(detail_comptes(1, 1), "1 élément");
        assert_eq!(detail_comptes(22, 22), "22 éléments");
        assert_eq!(detail_comptes(3377, 580), "3 377 éléments · 580 fichiers");
        assert_eq!(detail_comptes(1, 2), "1 élément · 2 fichiers");
        assert_eq!(detail_comptes(2, 1), "2 éléments · 1 fichier");
    }

    #[test]
    fn la_porte_n_accepte_qu_un_enfant_direct_exact() {
        let f = Faux::nouveau("porte");
        let temp = f.temp().to_path_buf();
        let d3d = f.local().join("D3DSCache");
        let m_temp = modele("temp");
        let m_d3d = modele("d3d");
        // Enfants directs : acceptés (casse ignorée) ; un petit-enfant est refusé (on retire le dossier, jamais l'intérieur)
        assert!(est_autorise(m_temp, &temp.join("a"), &f.r));
        assert!(!est_autorise(m_temp, &temp.join("a").join("b.tmp"), &f.r));
        assert!(est_autorise(m_d3d, &d3d.join("x.bin"), &f.r));
        assert!(est_autorise(m_d3d, Path::new(&d3d.to_string_lossy().to_uppercase()).join("X.BIN").as_path(), &f.r));
        // La racine elle-même, un frère, le parent, un autre modèle : refusés
        assert!(!est_autorise(m_d3d, &d3d, &f.r));
        assert!(!est_autorise(m_d3d, &f.local().join("D3DSCache2").join("x.bin"), &f.r));
        assert!(!est_autorise(m_d3d, f.local(), &f.r));
        assert!(!est_autorise(m_d3d, &temp.join("a.tmp"), &f.r));
        assert!(!est_autorise(modele("nvidia"), &d3d.join("x.bin"), &f.r));
        // Évasion par « .. » : refusée même si le début du chemin est bon
        assert!(!est_autorise(m_d3d, &d3d.join("..").join("..").join("Roaming").join("x"), &f.r));
        assert!(!est_autorise(m_temp, &temp.join("..").join("Microsoft").join("x"), &f.r));
        // Dossiers système
        assert!(!est_autorise(m_d3d, Path::new(r"C:\Windows\System32\x.dll"), &f.r));
        assert!(!est_autorise(m_temp, Path::new(r"C:\Program Files\App\x.exe"), &f.r));
        assert!(!est_autorise(m_temp, Path::new(r"C:\Windows\Prefetch\X.pf"), &f.r));
    }

    #[test]
    fn la_porte_des_plantages_ne_prend_que_les_dmp_de_la_racine() {
        let f = Faux::nouveau("porte-dmp");
        let d = f.local().join("CrashDumps");
        let m = modele("plantages");
        assert!(est_autorise(m, &d.join("a.dmp"), &f.r));
        assert!(est_autorise(m, &d.join("A.DMP"), &f.r));
        assert!(!est_autorise(m, &d.join("a.txt"), &f.r));
        assert!(!est_autorise(m, &d.join("sous").join("a.dmp"), &f.r));
    }

    #[test]
    fn la_corbeille_windows_n_a_aucun_chemin_autorise() {
        let f = Faux::nouveau("porte-corbeille");
        assert!(!est_autorise(modele("corbeille"), &f.temp().join("x"), &f.r));
        assert!(racines_valides(modele("corbeille"), &f.r).is_empty());
    }

    #[test]
    fn un_dossier_interdit_sous_la_racine_est_ecarte() {
        // Un profil dont une racine passerait par un dossier système n'est pas nettoyé
        let mut f = Faux::nouveau("interdit");
        let faux_local = f.r.profil.clone().unwrap().join("Windows").join("Local");
        fs::create_dir_all(faux_local.join("D3DSCache")).unwrap();
        f.r.local = Some(faux_local);
        assert!(racines_valides(modele("d3d"), &f.r).is_empty());
    }

    #[test]
    fn la_corbeille_s_ajoute_et_la_liste_est_triee_par_taille() {
        let f = Faux::nouveau("tri");
        f.fichier(&f.local().join("D3DSCache").join("x.bin"), 2 * MO as usize, 0);
        f.fichier(&f.local().join("NVIDIA").join("DXCache").join("x.bin"), 8 * MO as usize, 0);
        let v = mesurer_tout(&f.r, maintenant(), || Some((44 * MO, 22)));
        assert_eq!(ids(&v), vec!["corbeille", "nvidia", "d3d"]);
        assert_eq!(v[0].mode, Mode::Definitif);
        assert_eq!(v[0].elements, 22);
        assert_eq!(total(&v), 54 * MO);
        // Corbeille vide ou illisible : pas de ligne
        assert_eq!(ids(&mesurer_tout(&f.r, maintenant(), || Some((0, 0)))), vec!["nvidia", "d3d"]);
        assert_eq!(ids(&mesurer_tout(&f.r, maintenant(), || None)), vec!["nvidia", "d3d"]);
    }

    #[test]
    fn un_profil_vide_ne_donne_aucune_categorie() {
        let f = Faux::nouveau("vide");
        assert!(mesurer(&f).is_empty());
    }

    // ───── Suppression (E4b) : toujours sur un faux profil, jamais la vraie Corbeille ─────

    /// Le moteur de test : la « Corbeille » est un dossier du faux profil, le vidage ne fait rien
    fn supprimer_faux(f: &Faux, v: &[Categorie]) -> Bilan {
        let poubelle = f.base.join("poubelle");
        fs::create_dir_all(&poubelle).unwrap();
        supprimer(v, &f.r, maintenant(), &|p| fs::rename(p, poubelle.join(p.file_name().unwrap())).is_ok(), &|| false, &|| None)
    }

    /// Une demande écrite à la main (ce qu'une mesure ancienne ou un appelant fautif pourrait envoyer)
    fn demande(id: &'static str, chemins: Vec<PathBuf>) -> Categorie {
        let m = modele(id);
        Categorie { id: m.id, libelle: m.libelle, risque: m.risque, mode: m.mode, octets: 0, fichiers: 0, elements: chemins.len() as u64, illisibles: 0, anormal: false, chemins }
    }

    fn categorie(v: &[Categorie], id: &str) -> Categorie {
        v.iter().find(|c| c.id == id).unwrap_or_else(|| panic!("catégorie {id} absente")).clone()
    }

    #[test]
    fn le_mode_direct_efface_le_contenu_et_compte_les_octets() {
        let f = Faux::nouveau("direct");
        let d3d = f.local().join("D3DSCache");
        f.fichier(&d3d.join("a.bin"), 2 * MO as usize, 0);
        f.fichier(&d3d.join("sous").join("b.bin"), 3 * MO as usize, 0);
        let v = mesurer(&f);
        assert_eq!(categorie(&v, "d3d").chemins.len(), 2);
        let b = supprimer_faux(&f, &[categorie(&v, "d3d")]);
        assert_eq!((b.supprimes(), b.octets_liberes(), b.ignores()), (2, 5 * MO, 0));
        assert_eq!(b.octets_corbeille(), 0);
        // La racine reste (un cache vide est normal) ; plus rien à proposer
        assert!(d3d.is_dir() && fs::read_dir(&d3d).unwrap().next().is_none());
        assert!(mesurer(&f).is_empty());
    }

    #[test]
    fn le_mode_corbeille_n_envoie_que_les_anciens() {
        let f = Faux::nouveau("corbeille");
        f.fichier(&f.temp().join("ancien").join("a.bin"), 2 * MO as usize, 10);
        f.fichier(&f.temp().join("recent.log"), 5 * MO as usize, 3);
        let v = mesurer(&f);
        let b = supprimer_faux(&f, &[categorie(&v, "temp")]);
        assert_eq!((b.supprimes(), b.octets_corbeille(), b.octets_liberes()), (1, 2 * MO, 0));
        assert!(!f.temp().join("ancien").exists());
        assert!(f.base.join("poubelle").join("ancien").join("a.bin").exists());
        assert!(f.temp().join("recent.log").exists(), "ce qui a moins de 7 jours ne part jamais");
    }

    #[test]
    fn un_chemin_hors_liste_blanche_est_refuse() {
        let f = Faux::nouveau("refus");
        f.fichier(&f.base.join("hors").join("precieux.bin"), 100, 100);
        f.fichier(&f.local().join("Roaming-voisin").join("x.bin"), 100, 100);
        f.fichier(&f.local().join("D3DSCache").join("ok.bin"), 2 * MO as usize, 0);
        f.fichier(&f.temp().join("vieux.bin"), 100, 100);
        let intrus = vec![
            f.base.join("hors").join("precieux.bin"),
            f.local().join("Roaming-voisin").join("x.bin"),
            // La racine elle-même, le profil, un dossier système, une évasion par « .. », et le Temp sous le modèle d3d
            f.local().join("D3DSCache"),
            f.r.profil.clone().unwrap(),
            PathBuf::from(r"C:\Windows\System32\kernel32.dll"),
            f.local().join("D3DSCache").join("..").join("Roaming-voisin").join("x.bin"),
            f.temp().join("vieux.bin"),
        ];
        let n = intrus.len() as u64;
        let b = supprimer_faux(&f, &[demande("d3d", intrus)]);
        assert_eq!((b.supprimes(), b.categories[0].refuses), (0, n), "{b:?}");
        assert!(f.base.join("hors").join("precieux.bin").exists());
        assert!(f.local().join("Roaming-voisin").join("x.bin").exists());
        assert!(f.local().join("D3DSCache").join("ok.bin").exists());
        assert!(f.temp().join("vieux.bin").exists());
        assert!(Path::new(r"C:\Windows\System32\kernel32.dll").exists());
        // Un identifiant inconnu n'a pas de règle : rien n'est fait, pas même une ligne de bilan
        let mut inconnue = demande("d3d", vec![f.local().join("D3DSCache").join("ok.bin")]);
        inconnue.id = "inconnue";
        assert!(supprimer_faux(&f, &[inconnue]).categories.is_empty());
        assert!(f.local().join("D3DSCache").join("ok.bin").exists());
    }

    #[test]
    fn une_jonction_est_retiree_sans_toucher_a_sa_cible() {
        let f = Faux::nouveau("sup-jonction");
        let hors = f.base.join("hors-liste");
        f.fichier(&hors.join("precieux.bin"), 9 * MO as usize, 100);
        let d3d = f.local().join("D3DSCache");
        f.fichier(&d3d.join("c.bin"), 2 * MO as usize, 0);
        f.fichier(&d3d.join("dossier").join("d.bin"), MO as usize, 0);
        // Une jonction au premier niveau, une autre plus profond : aucune ne doit être suivie
        for lien in [d3d.join("lien"), d3d.join("dossier").join("lien2")] {
            let ok = std::process::Command::new("cmd").args(["/C", "mklink", "/J"]).arg(&lien).arg(&hors).output().unwrap();
            assert!(ok.status.success(), "mklink /J a échoué : {}", String::from_utf8_lossy(&ok.stdout));
        }
        // Mesure à la main : celle de `mesurer` ignore les jonctions, la suppression doit aussi savoir les rencontrer
        let b = supprimer_faux(&f, &[demande("d3d", vec![d3d.join("c.bin"), d3d.join("dossier")])]);
        assert_eq!(b.supprimes(), 2, "{b:?}");
        assert!(hors.join("precieux.bin").exists(), "la cible d'une jonction ne doit jamais être effacée");
        assert!(!d3d.join("dossier").exists());
        // Une jonction posée comme élément de premier niveau est refusée par la porte, mais sa cible reste elle aussi
        let b = supprimer_faux(&f, &[demande("d3d", vec![d3d.join("lien")])]);
        assert_eq!((b.supprimes(), b.categories[0].refuses), (0, 1));
        assert!(hors.join("precieux.bin").exists());
        let _ = fs::remove_dir(d3d.join("lien"));
    }

    #[test]
    fn un_nom_qui_finit_par_un_point_ou_une_espace_est_refuse() {
        // Win32 retire les points et espaces de fin : « ... » désignerait le dossier parent, « foo. » son voisin « foo »
        let f = Faux::nouveau("nom-piege");
        let verbatim = |p: &Path| PathBuf::from(format!(r"\\?\{}", p.display()));
        let piege = f.temp().join("...");
        let point = f.temp().join("foo.");
        let espace = f.temp().join("bar ");
        for p in [&piege, &point, &espace] {
            fs::create_dir_all(verbatim(p)).unwrap();
            fs::write(verbatim(p).join("x.bin"), vec![1u8; 10]).unwrap();
        }
        f.fichier(&f.temp().join("foo"), 10, 100);
        let voisin_vrai = f.temp().join("foo");
        let b = supprimer_faux(&f, &[demande("temp", vec![piege.clone(), point.clone(), espace.clone()])]);
        assert_eq!((b.supprimes(), b.categories[0].refuses), (0, 3), "{b:?}");
        assert!(voisin_vrai.exists() && f.temp().is_dir(), "ni le voisin ni Temp ne doivent partir");
        assert!(verbatim(&piege).join("x.bin").exists());
        for p in [&piege, &point, &espace] {
            let _ = fs::remove_dir_all(verbatim(p));
        }
    }

    #[test]
    fn seul_un_enfant_direct_de_la_racine_est_autorise() {
        let f = Faux::nouveau("enfant-direct");
        f.fichier(&f.local().join("D3DSCache").join("a").join("b.bin"), 100, 100);
        let m = modele("d3d");
        assert!(est_autorise(m, &f.local().join("D3DSCache").join("a"), &f.r));
        assert!(!est_autorise(m, &f.local().join("D3DSCache").join("a").join("b.bin"), &f.r));
    }

    #[test]
    fn un_dossier_devenu_jonction_est_retire_sans_etre_enumere() {
        let f = Faux::nouveau("effacer-jonction");
        let hors = f.base.join("hors-liste");
        f.fichier(&hors.join("precieux.bin"), 1000, 100);
        let racine = f.local().join("D3DSCache").join("x");
        fs::create_dir_all(racine.parent().unwrap()).unwrap();
        let ok = std::process::Command::new("cmd").args(["/C", "mklink", "/J"]).arg(&racine).arg(&hors).output().unwrap();
        assert!(ok.status.success());
        // Comme si « x » avait été remplacé par une jonction entre la vérification et l'effacement
        let mut octets = 0;
        let _ = effacer_direct(&racine, &mut octets);
        assert_eq!(octets, 0, "rien ne doit être compté : la cible n'est pas énumérée");
        assert!(hors.join("precieux.bin").exists(), "la cible d'une jonction ne doit jamais être effacée");
        assert!(fs::symlink_metadata(&racine).is_err(), "la jonction elle-même est retirée");
    }

    #[test]
    fn la_corbeille_qui_echoue_ne_compte_aucun_octet() {
        let f = Faux::nouveau("corbeille-echec");
        f.fichier(&f.temp().join("ancien").join("a.bin"), 2 * MO as usize, 10);
        let v = mesurer(&f);
        let b = supprimer(&[categorie(&v, "temp")], &f.r, maintenant(), &|_| false, &|| false, &|| None);
        assert_eq!((b.supprimes(), b.categories[0].en_usage, b.octets_corbeille(), b.octets_liberes()), (0, 1, 0, 0), "{b:?}");
        assert!(f.temp().join("ancien").exists());
    }

    #[test]
    fn la_corbeille_windows_sans_seconde_mesure_n_est_pas_comptee_deux_fois() {
        let f = Faux::nouveau("vidage-sans-mesure");
        let c = demande("corbeille", Vec::new());
        let appels = std::cell::Cell::new(0);
        let mesure = || {
            appels.set(appels.get() + 1);
            (appels.get() == 1).then_some((4 * MO, 3))
        };
        let b = supprimer(&[c.clone()], &f.r, maintenant(), &|_| false, &|| true, &mesure);
        assert_eq!((b.supprimes(), b.categories[0].en_usage, b.categories[0].octets), (3, 0, 4 * MO), "{b:?}");
        appels.set(0);
        let b = supprimer(&[c], &f.r, maintenant(), &|_| false, &|| false, &mesure);
        assert_eq!((b.supprimes(), b.categories[0].en_usage, b.categories[0].octets), (0, 3, 0), "{b:?}");
    }

    #[test]
    fn un_fichier_verrouille_est_ignore_et_compte() {
        use std::os::windows::fs::OpenOptionsExt;
        let f = Faux::nouveau("verrou");
        let d3d = f.local().join("D3DSCache");
        f.fichier(&d3d.join("pris.bin"), 2 * MO as usize, 0);
        f.fichier(&d3d.join("libre.bin"), 3 * MO as usize, 0);
        let v = mesurer(&f);
        // Ouvert sans aucun partage : comme un programme qui s'en sert (le défaut de Rust autorise la suppression)
        let _tenu = fs::OpenOptions::new().read(true).share_mode(0).open(d3d.join("pris.bin")).unwrap();
        let b = supprimer_faux(&f, &[categorie(&v, "d3d")]);
        assert_eq!((b.supprimes(), b.categories[0].en_usage, b.categories[0].refuses), (1, 1, 0), "{b:?}");
        assert_eq!(b.octets_liberes(), 3 * MO);
        assert!(d3d.join("pris.bin").exists() && !d3d.join("libre.bin").exists());
        assert_eq!(b.message(), "Nettoyage terminé · 1 élément supprimé · 3,00 Mo libérés · 1 élément ignoré (en cours d'usage)".replace("3,00 Mo", &crate::systeme::taille(3 * MO)));
    }

    #[test]
    fn la_regle_est_reverifiee_juste_avant_l_effacement() {
        let f = Faux::nouveau("regle");
        // Temp : un élément ancien à la mesure, touché depuis
        f.fichier(&f.temp().join("sert.bin"), 2 * MO as usize, 10);
        f.fichier(&f.temp().join("fini.bin"), 2 * MO as usize, 10);
        // Plantages : cinq rapports, p2 à p4 proposés ; p2 devient le plus récent après la mesure
        let d = f.local().join("CrashDumps");
        for (i, jours) in [1u64, 2, 5, 9, 20].iter().enumerate() {
            f.fichier(&d.join(format!("p{i}.dmp")), MO as usize, *jours);
        }
        let v = mesurer(&f);
        assert_eq!(categorie(&v, "plantages").chemins.len(), 3);
        fs::OpenOptions::new().write(true).open(f.temp().join("sert.bin")).unwrap().set_modified(maintenant()).unwrap();
        fs::OpenOptions::new().write(true).open(d.join("p2.dmp")).unwrap().set_modified(maintenant()).unwrap();
        let b = supprimer_faux(&f, &[categorie(&v, "temp"), categorie(&v, "plantages")]);
        assert!(f.temp().join("sert.bin").exists(), "redevenu récent : gardé");
        assert!(!f.temp().join("fini.bin").exists());
        // p0 et p1 sont les gardés d'origine, p2 vient de devenir le plus récent : seuls p3 et p4 partent
        for gardé in ["p0.dmp", "p1.dmp", "p2.dmp"] {
            assert!(d.join(gardé).exists(), "{gardé} doit rester");
        }
        assert!(!d.join("p3.dmp").exists() && !d.join("p4.dmp").exists());
        assert_eq!(b.supprimes(), 3);
        assert_eq!(b.ignores(), 2, "sert.bin et p2.dmp : {b:?}");
    }

    #[test]
    fn une_racine_devenue_jonction_entre_la_mesure_et_l_effacement_est_refusee() {
        let f = Faux::nouveau("racine-changee");
        let d3d = f.local().join("D3DSCache");
        f.fichier(&d3d.join("a.bin"), 2 * MO as usize, 0);
        let v = mesurer(&f);
        // Après la mesure, le dossier devient une jonction (vers l'ancien contenu, déplacé hors du profil)
        let ailleurs = f.base.join("ailleurs");
        fs::rename(&d3d, &ailleurs).unwrap();
        let ok = std::process::Command::new("cmd").args(["/C", "mklink", "/J"]).arg(&d3d).arg(&ailleurs).output().unwrap();
        assert!(ok.status.success());
        let b = supprimer_faux(&f, &[categorie(&v, "d3d")]);
        assert_eq!((b.supprimes(), b.categories[0].refuses), (0, 1), "{b:?}");
        assert!(ailleurs.join("a.bin").exists());
        let _ = fs::remove_dir(&d3d);
    }

    #[test]
    fn la_corbeille_windows_se_vide_et_la_difference_est_comptee() {
        let f = Faux::nouveau("vidage");
        let etat = std::cell::Cell::new((44 * MO, 22u64));
        let demandes = [Categorie { chemins: Vec::new(), ..demande("corbeille", Vec::new()) }];
        let b = supprimer(&demandes, &f.r, maintenant(), &|_| false, &|| { etat.set((0, 0)); true }, &|| Some(etat.get()));
        assert_eq!((b.supprimes(), b.octets_liberes(), b.ignores()), (22, 44 * MO, 0));
        assert_eq!(b.categories[0].mode, Mode::Definitif);
        // Vidage refusé : rien ne bouge, tout est compté comme ignoré
        let etat = std::cell::Cell::new((44 * MO, 22u64));
        let mut d = demande("corbeille", Vec::new());
        d.elements = 22;
        let b = supprimer(&[d], &f.r, maintenant(), &|_| false, &|| false, &|| Some(etat.get()));
        assert_eq!((b.supprimes(), b.octets_liberes(), b.ignores()), (0, 0, 22));
        // Aucun chemin n'a jamais été touché : la Corbeille ne passe pas par les chemins
        assert!(f.temp().is_dir());
    }

    #[test]
    fn les_phrases_du_bilan_sont_accordees() {
        let c = |mode, supprimes, octets, en_usage| BilanCategorie { id: "x", libelle: "Essai", mode, supprimes, octets, en_usage, refuses: 0 };
        let vide = Bilan::default();
        assert_eq!(vide.message(), "Rien à supprimer");
        let rien = Bilan { categories: vec![c(Mode::Direct, 0, 0, 2)] };
        assert_eq!(rien.message(), "Rien n'a pu être supprimé · 2 éléments ignorés (en cours d'usage)");
        let mixte = Bilan { categories: vec![c(Mode::Direct, 3, 5 * MO, 0), c(Mode::Corbeille, 1, 2 * MO, 0)] };
        assert_eq!(
            mixte.message(),
            format!("Nettoyage terminé · 4 éléments supprimés · {} libérés · {} envoyés à la Corbeille", crate::systeme::taille(5 * MO), crate::systeme::taille(2 * MO))
        );
        let (titre, detail) = mixte.journal();
        assert_eq!(titre, "Nettoyage · Essai, Essai");
        assert!(detail.starts_with("4 éléments supprimés"));
    }
}
