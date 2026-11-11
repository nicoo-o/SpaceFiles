// Bibliothèque « Eco » : ce que SpaceFiles parle avec les autres applications (SpaceNotch, Hub). Protocole v1 :
// `docs/ecosystem/protocol.md`.
//   trame.rs       enveloppe d'une ligne (lecture, réponses), bornes 4 096 octets / 32 Ko
//   json.rs        JSON minimal lu à la main
//   serveur.rs     tube nommé multi-instances du résident (`Eco.spacefiles.<utilisateur>`)
//   ouvrir.rs      `explorer.open` : validation du chemin, limite de débit, ouverture d'une fenêtre
//   reutiliser.rs  `reuse` : trouver une fenêtre SpaceFiles ouverte, lui envoyer le chemin (WM_COPYDATA), relire la charge
//   client.rs      appeler / émettre vers une autre application (borné à 300 ms, serveur vérifié)
//   progression.rs événements `progress` vers SpaceNotch (fil unique d'émission, coalescence)
//   manifeste.rs   `%LOCALAPPDATA%\Eco\apps\spacefiles.json`
//   partage.rs     `%LOCALAPPDATA%\Eco\shared.json` (thème, accent, mouvement, langue, raccourcis)
// Le résident démarre tout ici (`demarrer`) ; l'Explorateur normal n'ouvre aucun tube.

use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use windows::Win32::Foundation::HWND;

use crate::journal::Niveau;

pub mod client;
pub mod json;
mod manifeste;
mod ouvrir;
pub mod partage;
pub mod progression;
pub mod reutiliser;
pub mod serveur;
pub mod trame;

/// Identifiant de l'application : nom du tube et du manifeste
pub const APP: &str = "spacefiles";

/// Une trace du journal (états seulement, jamais le contenu d'un appel ni un chemin). En test elle ne fait rien :
/// la suite ne doit pas écrire dans le vrai `journal.txt` de l'auteur.
pub fn trace(niveau: Niveau, module: &str, message: &str) {
    #[cfg(not(test))]
    crate::journal::ecrire(niveau, module, message);
    #[cfg(test)]
    let _ = (niveau, module, message);
}

fn trace_serveur(niveau: Niveau, message: &str) {
    trace(niveau, "eco-serveur", message);
}

/// Garde `A-Za-z0-9._-`, le reste devient `_` (un `\` couperait le nom d'un tube ou d'un verrou)
pub fn assainir(s: &str) -> String {
    s.chars().map(|c| if c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-') { c } else { '_' }).collect()
}

/// `Eco.<app>.<utilisateur>` ; `essai` (variable `SPACEFILES_PIPE`, absente en usage normal) remplace l'utilisateur pour
/// qu'un résident ou un faux serveur de test ne rencontre pas le vrai tube.
pub fn nom_court(app: &str, utilisateur: &str, essai: Option<&str>) -> String {
    let brut = essai.filter(|e| !e.is_empty()).unwrap_or(utilisateur);
    let propre = assainir(brut);
    format!("Eco.{app}.{}", if propre.is_empty() { "utilisateur" } else { &propre })
}

/// `\\.\pipe\Eco.<app>.<utilisateur>`
pub fn nom_tube(app: &str, utilisateur: &str, essai: Option<&str>) -> String {
    format!(r"\\.\pipe\{}", nom_court(app, utilisateur, essai))
}

/// Variable d'essai `SPACEFILES_PIPE` (absente en usage normal)
pub fn essai() -> Option<String> {
    std::env::var("SPACEFILES_PIPE").ok().filter(|s| !s.is_empty())
}

fn utilisateur() -> String {
    std::env::var("USERNAME").unwrap_or_default()
}

/// Nom court du tube d'une application, d'après l'environnement (`Eco.spacenotch.utilisateur`, ou avec le suffixe d'essai)
pub fn nom_court_courant(app: &str) -> String {
    nom_court(app, &utilisateur(), essai().as_deref())
}

// ───────────────────────── Dossier Eco ─────────────────────────

/// `%LOCALAPPDATA%\Eco`. En mode essai, jamais le vrai dossier : si `LOCALAPPDATA` désigne le dossier réel de l'utilisateur
/// (l'essai a été lancé sans faux profil), rien n'est écrit. `reel` : le dossier connu du Shell, que l'environnement ne change pas.
fn choisir_dossier_eco(local: Option<&std::ffi::OsStr>, reel: Option<&Path>, temp: Option<&Path>, essai: bool) -> Option<PathBuf> {
    let local = Path::new(local?);
    if essai {
        // Le mode essai n'écrit que dans un faux profil : sous le dossier temporaire, et jamais le dossier réel
        let sous_temp = temp.is_some_and(|t| est_sous(local, t));
        if !sous_temp || reel.is_some_and(|r| meme_dossier(r, local)) {
            return None;
        }
    }
    Some(local.join("Eco"))
}

/// Chemin Windows comparable : canonicalisé si possible (`..`, 8.3, jonction), sans préfixe `\\?\`, sans casse, sans `\` final
fn normaliser(p: &Path) -> String {
    let c = std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf());
    let s = c.to_string_lossy().to_lowercase();
    let s = s.strip_prefix(r"\\?\").unwrap_or(&s);
    // Un chemin qui n'existe pas n'est pas canonicalisé : `.` et `..` sont résolus à la main
    let mut morceaux: Vec<&str> = Vec::new();
    for m in s.split('\\') {
        match m {
            "." => {}
            ".." => {
                if morceaux.len() > 1 {
                    morceaux.pop();
                }
            }
            m => morceaux.push(m),
        }
    }
    morceaux.join("\\").trim_end_matches('\\').to_string()
}

/// Même dossier ?
fn meme_dossier(a: &Path, b: &Path) -> bool {
    normaliser(a) == normaliser(b)
}

/// `a` est `b` ou se trouve dessous (le nom entier compte : `tt` n'est pas sous `t`)
fn est_sous(a: &Path, b: &Path) -> bool {
    let (a, b) = (normaliser(a), normaliser(b));
    a == b || a.starts_with(&format!("{b}\\"))
}

/// Dossier local réel de l'utilisateur d'après le Shell (indépendant de la variable d'environnement)
fn dossier_local_reel() -> Option<PathBuf> {
    use windows::Win32::System::Com::CoTaskMemFree;
    use windows::Win32::UI::Shell::{FOLDERID_LocalAppData, SHGetKnownFolderPath, KF_FLAG_DEFAULT};
    // SAFETY : le PWSTR rendu est une chaîne terminée par 0 allouée par le Shell ; elle est copiée puis libérée par CoTaskMemFree.
    unsafe {
        let p = SHGetKnownFolderPath(&FOLDERID_LocalAppData, KF_FLAG_DEFAULT, None).ok()?;
        let s = p.to_string().ok();
        CoTaskMemFree(Some(p.0 as *const _));
        s.map(PathBuf::from)
    }
}

pub(crate) fn dossier_eco() -> Option<PathBuf> {
    let essai = essai().is_some();
    choisir_dossier_eco(std::env::var_os("LOCALAPPDATA").as_deref(), if essai { dossier_local_reel() } else { None }.as_deref(), Some(std::env::temp_dir()).as_deref(), essai)
}

/// Écriture atomique : fichier temporaire à côté (nom unique : deux écrivains ne se marchent pas dessus), forcé sur le
/// disque, puis renommé sur la cible. Après une coupure, le fichier est l'ancien ou le nouveau, jamais un fichier vide.
pub(crate) fn ecrire_atomique(cible: &Path, texte: &str) -> std::io::Result<()> {
    use std::io::Write;
    use std::sync::atomic::{AtomicU32, Ordering};
    static N: AtomicU32 = AtomicU32::new(0);
    if let Some(d) = cible.parent() {
        std::fs::create_dir_all(d)?;
    }
    let nom = cible.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let tmp = cible.with_file_name(format!("{nom}.{}.{}.tmp", std::process::id(), N.fetch_add(1, Ordering::Relaxed)));
    let ecrit = std::fs::File::create(&tmp).and_then(|mut f| {
        f.write_all(texte.as_bytes())?;
        f.sync_all()
    });
    if let Err(e) = ecrit {
        let _ = std::fs::remove_file(&tmp);
        return Err(e);
    }
    // Le renommage sur un fichier que quelqu'un lit à cet instant peut être refusé un court instant : quelques essais
    let mut dernier = Ok(());
    for _ in 0..20 {
        dernier = std::fs::rename(&tmp, cible);
        if dernier.is_ok() {
            return Ok(());
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    let _ = std::fs::remove_file(&tmp);
    dernier
}

// ───────────────────────── Résident ─────────────────────────

static SERVEUR: Mutex<Option<serveur::Serveur>> = Mutex::new(None);
static DEMARRE: OnceLock<()> = OnceLock::new();

/// Répond à un appel du tube : `hello`, `search.query`, `explorer.open`
fn repondre(id: &str, appel: &trame::Appel, c: &serveur::Connexion) -> Option<String> {
    use trame::{reponse_erreur, reponse_hello, reponse_ouvert, Appel};
    match appel {
        Appel::Hello => Some(reponse_hello(id)),
        Appel::Recherche { q, limite } => crate::service_recherche::traiter(id, q, *limite, c),
        Appel::Ouvrir { chemin, reveler, reutiliser } => Some(match ouvrir::demander(chemin, *reveler, *reutiliser) {
            Ok(()) => reponse_ouvert(id),
            Err(e) => reponse_erreur(id, &e),
        }),
    }
}

/// Démarre ce que le résident offre aux autres applications : réglages partagés lus une fois, tube (4 instances),
/// puis manifeste (écrit seulement si le tube existe : un manifeste annonce un service qui répond).
pub fn demarrer(fenetre: HWND) {
    if DEMARRE.set(()).is_err() {
        return;
    }
    crate::service_recherche::initialiser(fenetre);
    ouvrir::initialiser();
    partage::charger_au_demarrage();
    let config = serveur::Config { nom_tube: nom_tube(APP, &utilisateur(), essai().as_deref()), instances: serveur::INSTANCES, inactivite: serveur::delai_inactivite(), trace: trace_serveur };
    let Some(s) = serveur::Serveur::demarrer(config, Box::new(repondre)) else { return };
    *SERVEUR.lock().unwrap_or_else(|e| e.into_inner()) = Some(s);
    manifeste::ecrire_au_demarrage();
}

/// Arrête le tube à la fermeture du résident (le manifeste reste : le Hub l'affiche « arrêté »)
pub fn arreter() {
    crate::service_recherche::arreter();
    ouvrir::arreter();
    let s = SERVEUR.lock().unwrap_or_else(|e| e.into_inner()).take();
    if let Some(s) = s {
        s.arreter();
    }
}

/// Tests seulement : déballe un résultat avec un message (le projet n'ajoute ni `unwrap()` ni `expect(`)
#[cfg(test)]
pub(crate) fn ok<T, E: std::fmt::Debug>(r: Result<T, E>, contexte: &str) -> T {
    match r {
        Ok(v) => v,
        Err(e) => panic!("{contexte} : {e:?}"),
    }
}

/// Tests seulement : comme `ok` pour une option
#[cfg(test)]
pub(crate) fn some<T>(o: Option<T>, contexte: &str) -> T {
    match o {
        Some(v) => v,
        None => panic!("{contexte} : rien"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsStr;

    #[test]
    fn nom_du_tube() {
        assert_eq!(nom_tube("spacefiles", "utilisateur", None), r"\\.\pipe\Eco.spacefiles.utilisateur");
        assert_eq!(nom_tube("spacefiles", "Jean Dupont", None), r"\\.\pipe\Eco.spacefiles.Jean_Dupont");
        assert_eq!(nom_tube("spacefiles", "a\\b", None), r"\\.\pipe\Eco.spacefiles.a_b");
        assert_eq!(nom_tube("spacefiles", "utilisateur", Some("essai1")), r"\\.\pipe\Eco.spacefiles.essai1");
        assert_eq!(nom_tube("spacefiles", "utilisateur", Some("")), r"\\.\pipe\Eco.spacefiles.utilisateur");
        assert_eq!(nom_tube("spacefiles", "", None), r"\\.\pipe\Eco.spacefiles.utilisateur");
        // Le tube de SpaceNotch suit la même règle (progress)
        assert_eq!(nom_court("spacenotch", "utilisateur", Some("t1")), "Eco.spacenotch.t1");
    }

    #[test]
    fn nom_assaini() {
        assert_eq!(assainir(r"a\b c"), "a_b_c");
        assert_eq!(assainir("essai-1.x_y"), "essai-1.x_y");
        assert_eq!(assainir("é"), "_");
    }

    /// Le mode essai n'écrit jamais dans le vrai dossier Eco, ni ailleurs que sous le dossier temporaire
    #[test]
    fn dossier_eco_du_mode_essai() {
        let reel = Path::new(r"C:\Users\x\AppData\Local");
        let temp = Path::new(r"C:\Temp\t");
        // Usage normal : toujours %LOCALAPPDATA%\Eco
        assert_eq!(choisir_dossier_eco(Some(OsStr::new(r"C:\Users\x\AppData\Local")), None, None, false), Some(PathBuf::from(r"C:\Users\x\AppData\Local\Eco")));
        // Essai avec faux profil sous le dossier temporaire : le dossier du faux profil
        assert_eq!(choisir_dossier_eco(Some(OsStr::new(r"C:\Temp\t\faux\Local")), Some(reel), Some(temp), true), Some(PathBuf::from(r"C:\Temp\t\faux\Local\Eco")));
        // Essai sans faux profil (même dossier que le vrai, casse et `\` final près) : rien
        assert_eq!(choisir_dossier_eco(Some(OsStr::new(r"c:\users\X\appdata\local\")), Some(reel), Some(temp), true), None);
        // Essai dont le profil n'est pas sous le dossier temporaire : rien, même s'il n'est pas le dossier réel
        assert_eq!(choisir_dossier_eco(Some(OsStr::new(r"D:\ailleurs\Local")), Some(reel), Some(temp), true), None);
        assert_eq!(choisir_dossier_eco(Some(OsStr::new(r"C:\Temp\t\..\autre")), Some(reel), Some(temp), true), None, "`..` n'échappe pas au contrôle");
        assert_eq!(choisir_dossier_eco(Some(OsStr::new(r"C:\Temp\tt\Local")), Some(reel), Some(temp), true), None, "préfixe de nom, pas dossier parent");
        assert_eq!(choisir_dossier_eco(Some(OsStr::new(r"C:\Temp\t\faux")), Some(reel), None, true), None);
        assert_eq!(choisir_dossier_eco(None, None, None, false), None);
    }

    /// Canonicalisation réelle : un dossier écrit avec `..` ou une autre casse reste reconnu
    #[test]
    fn comparaison_de_dossiers_canonicalisee() {
        let t = std::env::temp_dir().join(format!("eco-canon-{}", std::process::id()));
        ok(std::fs::create_dir_all(t.join("a")), "creation");
        let detour = t.join("a").join("..").join("a");
        assert!(meme_dossier(&detour, &t.join("a")));
        assert!(est_sous(&detour, &t));
        assert!(!est_sous(&t, &t.join("a")));
        assert!(est_sous(Path::new(&t.to_string_lossy().to_uppercase()), &t));
        let _ = std::fs::remove_dir_all(&t);
    }

    #[test]
    fn ecriture_atomique_remplace_sans_residu() {
        let d = std::env::temp_dir().join(format!("eco-atomique-{}", std::process::id()));
        let f = d.join("sous").join("x.json");
        ok(ecrire_atomique(&f, "un"), "premiere ecriture");
        ok(ecrire_atomique(&f, "deux"), "seconde ecriture");
        assert_eq!(ok(std::fs::read_to_string(&f), "lecture"), "deux");
        let restes = ok(std::fs::read_dir(d.join("sous")), "dossier").flatten().filter(|e| e.file_name().to_string_lossy().ends_with(".tmp")).count();
        assert_eq!(restes, 0, "un temporaire est reste");
        let _ = std::fs::remove_dir_all(&d);
    }
}
