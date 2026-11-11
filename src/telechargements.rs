// Téléchargements malins : badges DEJA INSTALLE (installeurs dont le programme est déjà là) et
// COPIE SUR LE PC (même nom, même taille, même contenu ailleurs, d'après l'index), et nettoyage
// automatique, désactivé par défaut et toujours vers la corbeille.

use std::collections::HashMap;
use std::ffi::c_void;

use windows::core::HSTRING;
use windows::Win32::Storage::FileSystem::{GetFileVersionInfoSizeW, GetFileVersionInfoW, VerQueryValueW};
use windows::Win32::System::Registry::{HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn noms() {
        assert_eq!(mots("WindowsPCHealthCheckSetup"), vec!["pc", "health", "check"]);
        assert_eq!(mots("vlc-3.0.20-win64"), vec!["vlc"]);
        let installes = vec![mots("PowerToys (Preview) x64"), mots("VLC media player"), mots("Microsoft Edge")];
        let ok = |n: &str| installes.iter().any(|i| correspond(&mots(n), i));
        assert!(ok("Microsoft PowerToys Installer"));
        assert!(ok("vlc-3.0.20-win64"));
        assert!(!ok("Microsoft Teams Setup"));
        assert_eq!(version_du_nom("jellyfin_10.11.6_windows-x64.exe"), vec![10, 11, 6]);
        assert_eq!(version_du_nom("wiztree_4_32_setup.exe"), Vec::<u32>::new());
        assert_eq!(version("v1.11.0-beta"), vec![1, 11, 0]);
        assert!(plus_recente(&[3, 0, 21], &[3, 0, 20, 0]));
        assert!(!plus_recente(&[3, 0], &[3, 0, 0, 0]));
    }

    #[test]
    #[ignore]
    fn telechargements_reels() {
        let d = crate::systeme::chemin_telechargements().unwrap();
        let fichiers: Vec<(String, u64)> = std::fs::read_dir(&d).unwrap().flatten().filter(|e| e.path().is_file()).map(|e| (e.file_name().to_string_lossy().into_owned(), e.metadata().unwrap().len())).collect();
        let t = std::time::Instant::now();
        let installes = programmes_installes();
        println!("registre : {} programmes en {:?}", installes.len(), t.elapsed());
        let t = std::time::Instant::now();
        let n = fichiers.iter().filter(|(n, _)| n.to_lowercase().ends_with(".exe")).filter(|(n, _)| deja_installe(&crate::dossier::joindre(&d, n), &installes) == Some(Badge::MiseAJour)).count();
        println!("dont mises a jour : {n}");
        println!("installeurs : {n} en {:?}", t.elapsed());
        let t = std::time::Instant::now();
        let b = badges(&d, &fichiers);
        println!("badges : {} en {:?}", b.len(), t.elapsed());
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Badge {
    /// Le programme de cet installeur est déjà installé (même version ou plus récente)
    Installe,
    /// Le programme est installé, mais cet installeur en apporte une version plus récente
    MiseAJour,
    /// Une copie identique existe ailleurs sur le PC (son chemin)
    Copie(String),
}

impl Badge {
    pub fn libelle(&self) -> &'static str {
        match self {
            Badge::Installe => "DEJA INSTALLE",
            Badge::MiseAJour => "MISE A JOUR",
            Badge::Copie(_) => "COPIE SUR LE PC",
        }
    }
}

/// « WindowsPCHealthCheckSetup » → « Windows PC Health Check Setup »
fn decamel(s: &str) -> String {
    let c: Vec<char> = s.chars().collect();
    let mut r = String::new();
    for (i, &x) in c.iter().enumerate() {
        if i > 0 && x.is_uppercase() {
            let avant = c[i - 1];
            let apres_min = c.get(i + 1).is_some_and(|n| n.is_lowercase());
            if avant.is_lowercase() || avant.is_ascii_digit() || (avant.is_uppercase() && apres_min) {
                r.push(' ');
            }
        }
        r.push(x);
    }
    r
}

/// Mots qui ne disent rien du programme : « setup », « x64 », numéros de version…
fn mots(s: &str) -> Vec<String> {
    const VIDES: [&str; 18] = ["setup", "installer", "install", "x64", "x86", "win64", "win32", "amd64", "arm64", "64bit", "32bit", "windows", "win", "full", "offline", "online", "preview", "version"];
    decamel(s)
        .to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|m| m.len() >= 2 && !VIDES.contains(m) && !m.chars().any(|c| c.is_ascii_digit()))
        .map(str::to_string)
        .collect()
}

/// Programme installé : mots de son nom, version affichée par Windows
pub struct Installe {
    mots: Vec<String>,
    version: Vec<u32>,
}

/// « 3.0.20.1 » → [3, 0, 20, 1] ; « v1.11.0-beta » → [1, 11, 0]
fn version(s: &str) -> Vec<u32> {
    let s = s.trim().trim_start_matches(['v', 'V']);
    let debut = s.find(|c: char| c.is_ascii_digit()).unwrap_or(s.len());
    s[debut..].split(|c: char| !c.is_ascii_digit() && c != '.').next().unwrap_or("").split('.').filter_map(|x| x.parse().ok()).collect()
}

/// Version lue dans un nom de fichier : le premier groupe « 1.2 » ou plus (« jellyfin_10.11.6_windows » → 10.11.6)
fn version_du_nom(nom: &str) -> Vec<u32> {
    nom.split(|c: char| !c.is_ascii_digit() && c != '.')
        .map(|x| x.trim_matches('.'))
        .find(|x| x.contains('.') && x.split('.').all(|p| !p.is_empty() && p.len() <= 5))
        .map(version)
        .unwrap_or_default()
}

/// Noms des programmes installés (désinstallation, machine et utilisateur, 64 et 32 bits)
pub fn programmes_installes() -> Vec<Installe> {
    let mut v = Vec::new();
    for (racine, chemin) in [
        (HKEY_LOCAL_MACHINE, "SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Uninstall"),
        (HKEY_LOCAL_MACHINE, "SOFTWARE\\WOW6432Node\\Microsoft\\Windows\\CurrentVersion\\Uninstall"),
        (HKEY_CURRENT_USER, "SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Uninstall"),
    ] {
        let Some(c) = crate::stockages::ouvrir(racine, chemin) else { continue };
        for s in crate::stockages::sous_cles(&c) {
            let Some(k) = crate::stockages::ouvrir(racine, &format!("{chemin}\\{s}")) else { continue };
            let valeurs = crate::stockages::valeurs(&k);
            let lire = |n: &str| valeurs.iter().find(|(x, _)| x == n).map(|(_, v)| v.clone());
            if let Some(nom) = lire("DisplayName") {
                let m = mots(&nom);
                if !m.is_empty() {
                    v.push(Installe { mots: m, version: lire("DisplayVersion").map(|x| version(&x)).unwrap_or_default() });
                }
            }
        }
    }
    v
}

/// Nom du produit et version inscrits dans un .exe (ressource de version)
fn infos_exe(chemin: &str) -> (Option<String>, Vec<u32>) {
    unsafe {
        let h = HSTRING::from(chemin);
        let n = GetFileVersionInfoSizeW(&h, None);
        if n == 0 {
            return (None, Vec::new());
        }
        let mut bloc = vec![0u8; n as usize];
        if GetFileVersionInfoW(&h, 0, n, bloc.as_mut_ptr() as *mut c_void).is_err() {
            return (None, Vec::new());
        }
        let mut p: *mut c_void = std::ptr::null_mut();
        let mut l = 0u32;
        if !VerQueryValueW(bloc.as_ptr() as *const c_void, &HSTRING::from("\\VarFileInfo\\Translation"), &mut p, &mut l).as_bool() || l < 4 {
            return (None, Vec::new());
        }
        let t = std::slice::from_raw_parts(p as *const u16, 2);
        let (langue, page) = (t[0], t[1]);
        let texte = |cle: &str| -> Option<String> {
            let mut p: *mut c_void = std::ptr::null_mut();
            let mut l = 0u32;
            let c = format!("\\StringFileInfo\\{langue:04x}{page:04x}\\{cle}");
            if !VerQueryValueW(bloc.as_ptr() as *const c_void, &HSTRING::from(c), &mut p, &mut l).as_bool() || l == 0 {
                return None;
            }
            let s = std::slice::from_raw_parts(p as *const u16, l as usize);
            let fin = s.iter().position(|&c| c == 0).unwrap_or(s.len());
            Some(String::from_utf16_lossy(&s[..fin])).filter(|x| !x.trim().is_empty())
        };
        let v = texte("ProductVersion").or_else(|| texte("FileVersion")).map(|x| version(&x)).unwrap_or_default();
        (texte("ProductName"), v)
    }
}

/// Éditeurs : un seul de ces mots ne suffit pas (« Microsoft Edge » n'est pas « Microsoft Teams »)
const EDITEURS: [&str; 10] = ["microsoft", "google", "adobe", "intel", "nvidia", "amd", "apple", "mozilla", "oracle", "the"];

/// Les mots `c` de l'installeur désignent-ils le programme installé `i` ?
fn correspond(c: &[String], i: &[String]) -> bool {
    // Les deux premiers mots du programme installé sont dans le nom de l'installeur…
    i[..i.len().min(2)].iter().all(|m| c.contains(m))
        // … ou son premier mot, s'il est assez distinctif
        || (i[0].len() >= 3 && !EDITEURS.contains(&i[0].as_str()) && c.contains(&i[0]))
}

/// Versions comparées nombre par nombre (les zéros manquants comptent pour 0)
fn plus_recente(a: &[u32], b: &[u32]) -> bool {
    let n = a.len().max(b.len());
    (0..n).map(|k| (a.get(k).copied().unwrap_or(0), b.get(k).copied().unwrap_or(0))).find(|(x, y)| x != y).is_some_and(|(x, y)| x > y)
}

/// L'installeur `chemin` installe-t-il un programme déjà présent ? DEJA INSTALLE, ou MISE A JOUR s'il
/// apporte une version plus récente que celle installée
pub fn deja_installe(chemin: &str, installes: &[Installe]) -> Option<Badge> {
    let nom = chemin.rsplit('\\').next().unwrap_or(chemin);
    let exe = nom.to_ascii_lowercase().ends_with(".exe");
    let cherche = |c: &[String]| (!c.is_empty() && c.concat().len() >= 3).then(|| installes.iter().find(|i| correspond(c, &i.mots))).flatten();
    // Le nom de fichier d'abord : lire la ressource de version d'un .exe est lent la première fois
    // (l'antivirus l'analyse), on ne le fait que si le nom ne suffit pas
    let mut infos = None;
    let trouve = match cherche(&mots(nom.rsplit_once('.').map_or(nom, |x| x.0))) {
        Some(i) => i,
        None if exe => {
            let (produit, v) = infos_exe(chemin);
            let i = produit.and_then(|p| cherche(&mots(&p)))?;
            infos = Some(v);
            i
        }
        None => return None,
    };
    let mut v = version_du_nom(nom);
    if v.is_empty() && exe {
        v = infos.unwrap_or_else(|| infos_exe(chemin).1);
    }
    Some(if !v.is_empty() && !trouve.version.is_empty() && plus_recente(&v, &trouve.version) { Badge::MiseAJour } else { Badge::Installe })
}

/// Même contenu ? Début et fin comparés (1 Mo et 64 Ko) : assez sûr pour un même nom et une même taille
fn meme_contenu(a: &str, b: &str) -> bool {
    use std::io::{Read, Seek, SeekFrom};
    let lire = |c: &str| -> Option<Vec<u8>> {
        let mut f = std::fs::File::open(c).ok()?;
        let mut v = vec![0u8; 1 << 20];
        let n = f.read(&mut v).ok()?;
        v.truncate(n);
        let taille = f.metadata().ok()?.len();
        if taille > (1 << 20) + 65536 {
            f.seek(SeekFrom::End(-65536)).ok()?;
            let mut fin = vec![0u8; 65536];
            f.read_exact(&mut fin).ok()?;
            v.extend(fin);
        }
        Some(v)
    };
    matches!((lire(a), lire(b)), (Some(x), Some(y)) if x == y)
}

/// Badges des fichiers du dossier Téléchargements `dossier` : (nom, taille, est un installeur)
pub fn badges(dossier: &str, fichiers: &[(String, u64)]) -> HashMap<String, Badge> {
    let mut v = HashMap::new();
    let installeurs: Vec<&(String, u64)> = fichiers.iter().filter(|(n, _)| n.to_ascii_lowercase().ends_with(".exe") || n.to_ascii_lowercase().ends_with(".msi")).collect();
    if !installeurs.is_empty() {
        let installes = programmes_installes();
        for (nom, _) in installeurs {
            if let Some(b) = deja_installe(&crate::dossier::joindre(dossier, nom), &installes) {
                v.insert(nom.to_lowercase(), b);
            }
        }
    }
    let cherches: HashMap<String, u64> = fichiers.iter().filter(|(n, t)| *t > 0 && !v.contains_key(&n.to_lowercase())).map(|(n, t)| (n.to_lowercase(), *t)).collect();
    for (nom, autre) in crate::index::copies_ailleurs(&cherches, dossier) {
        let ici = crate::dossier::joindre(dossier, &nom);
        if autre.iter().take(3).any(|c| meme_contenu(&ici, c)) {
            if let Some(c) = autre.into_iter().next() {
                v.insert(nom, Badge::Copie(c));
            }
        }
    }
    v
}

/// Met des fichiers à la corbeille depuis un fil de fond (nettoyage après extraction)
pub fn a_la_corbeille(chemins: &[String]) -> bool {
    use windows::Win32::System::Com::*;
    use windows::Win32::UI::Shell::*;
    unsafe {
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        let Ok(op) = CoCreateInstance::<_, IFileOperation>(&FileOperation, None, CLSCTX_ALL) else { return false };
        // FOF_WANTNUKEWARNING : sans Corbeille (clé USB, NAS), Windows demande avant de supprimer pour de bon
        let drapeaux = FOF_SILENT.0 | FOF_NOCONFIRMATION.0 | FOF_WANTNUKEWARNING.0 | FOF_ALLOWUNDO.0 | FOFX_RECYCLEONDELETE.0 as u32;
        if op.SetOperationFlags(FILEOPERATION_FLAGS(drapeaux)).is_err() {
            return false;
        }
        for c in chemins {
            if let Ok(i) = SHCreateItemFromParsingName::<_, _, IShellItem>(&HSTRING::from(c.as_str()), None) {
                let _ = op.DeleteItem(&i, None);
            }
        }
        op.PerformOperations().is_ok()
    }
}
