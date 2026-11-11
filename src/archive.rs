// Archives (E16) : zip, 7z, rar, tar… parcourues comme des dossiers, extraites, créées.
// Tout passe par le tar.exe de Windows (bsdtar / libarchive) : rien à installer.

use std::os::windows::process::CommandExt;
use std::process::Command;
use std::sync::{Arc, Mutex};

use crate::dossier::{Entree, Nuage};
use crate::outils::SANS_CONSOLE;

/// Extensions ouvertes comme des dossiers (Entrée)
pub const EXTENSIONS: &[&str] = &["ZIP", "7Z", "RAR", "TAR", "GZ", "TGZ", "BZ2", "TBZ2", "XZ", "TXZ", "CAB", "ISO", "ZST"];

pub fn est_archive(ext: &str) -> bool {
    EXTENSIONS.contains(&ext)
}

fn tar() -> Command {
    let windows = std::env::var("WINDIR").unwrap_or_else(|_| r"C:\Windows".into());
    let mut c = Command::new(format!(r"{windows}\System32\tar.exe"));
    c.creation_flags(SANS_CONSOLE);
    c
}

/// Élément d'une archive
#[derive(Clone, Debug)]
pub struct Membre {
    /// Nom tel que tar l'écrit (pour l'extraire)
    pub brut: String,
    /// Chemin normalisé : `dossier/fichier`, sans `./` ni `/` final
    pub chemin: String,
    pub dossier: bool,
    pub taille: u64,
    pub modif: u64,
}

/// Liste d'une archive, gardée pour naviguer dedans sans tout relire : (fichier, date du fichier, membres)
static DERNIERE: Mutex<Option<(String, u64, Arc<Vec<Membre>>)>> = Mutex::new(None);

/// Texte dans la page de codes ANSI (tar.exe écrit les noms ainsi quand sa sortie est redirigée)
fn depuis_ansi(octets: &[u8]) -> String {
    use windows::Win32::Globalization::{MultiByteToWideChar, CP_ACP, MULTI_BYTE_TO_WIDE_CHAR_FLAGS};
    if octets.is_empty() {
        return String::new();
    }
    unsafe {
        let n = MultiByteToWideChar(CP_ACP, MULTI_BYTE_TO_WIDE_CHAR_FLAGS(0), octets, None);
        let mut w = vec![0u16; n.max(0) as usize];
        let n = MultiByteToWideChar(CP_ACP, MULTI_BYTE_TO_WIDE_CHAR_FLAGS(0), octets, Some(&mut w));
        String::from_utf16_lossy(&w[..n.max(0) as usize])
    }
}

/// Mois en trois lettres (anglais) ou abrégé français (« oct. », « févr. », « déc. »)
fn mois(m: &str) -> Option<u32> {
    let m = m.trim_end_matches('.').to_lowercase();
    let noms = [
        ["jan", "janv"], ["feb", "févr"], ["mar", "mars"], ["apr", "avr"], ["may", "mai"], ["jun", "juin"],
        ["jul", "juil"], ["aug", "août"], ["sep", "sept"], ["oct", "oct"], ["nov", "nov"], ["dec", "déc"],
    ];
    // Découpe par caractères, jamais par octets : « août » fait 5 octets et `&m[..3]` tombait au milieu du û
    // (panique réelle du 04/11/2026, journal de SpaceFiles, `archive.rs:61`).
    let debut: String = m.chars().take(3).collect();
    noms.iter().position(|n| n.iter().any(|nom| nom.contains(m.as_str()) || (debut.chars().count() == 3 && nom.starts_with(&debut)))).map(|i| i as u32 + 1)
}

/// Date de la ligne `ls -l` en FILETIME (approximative : à la minute, heure locale prise pour UTC)
fn date(mo: &str, jour: &str, heure_ou_annee: &str) -> u64 {
    let (Some(m), Ok(j)) = (mois(mo), jour.parse::<u32>()) else { return 0 };
    let (annee, h, mi) = match heure_ou_annee.split_once(':') {
        Some((h, mi)) => (crate::systeme::annee_courante(), h.parse().unwrap_or(0), mi.parse().unwrap_or(0)),
        None => (heure_ou_annee.parse().unwrap_or(1980), 0, 0),
    };
    crate::systeme::filetime(annee, m, j, h, mi)
}

/// Une ligne de `tar -tv` : `-rw-rw-rw-  0 0      0          12 oct. 20 18:05 ./a.txt`
fn ligne(l: &str) -> Option<Membre> {
    // Les 8 premiers champs sont séparés par des espaces ; le nom est le reste, tel quel
    let mut reste = l;
    let mut champs = Vec::with_capacity(8);
    for _ in 0..8 {
        reste = reste.trim_start();
        let fin = reste.find(' ')?;
        champs.push(&reste[..fin]);
        reste = &reste[fin..];
    }
    let brut = reste.strip_prefix(' ')?.to_string();
    let dossier = champs[0].starts_with('d');
    // Lien symbolique : « nom -> cible »
    let nom = if champs[0].starts_with('l') { brut.split(" -> ").next().unwrap_or(&brut).to_string() } else { brut.clone() };
    let chemin = nom.replace('\\', "/").trim_start_matches("./").trim_matches('/').to_string();
    if chemin.is_empty() || chemin == "." {
        return None;
    }
    Some(Membre { brut: if champs[0].starts_with('l') { nom } else { brut }, chemin, dossier, taille: champs[4].parse().unwrap_or(0), modif: date(champs[5], champs[6], champs[7]) })
}

fn date_fichier(f: &str) -> u64 {
    std::fs::metadata(f).and_then(|m| m.modified()).ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map_or(0, |d| d.as_secs())
}

/// Tous les éléments de l'archive (relue seulement si le fichier a changé)
pub fn membres(fichier: &str) -> Result<Arc<Vec<Membre>>, String> {
    let quand = date_fichier(fichier);
    if let Some((f, d, v)) = DERNIERE.lock().unwrap_or_else(|e| e.into_inner()).as_ref() {
        if f.eq_ignore_ascii_case(fichier) && *d == quand {
            return Ok(v.clone());
        }
    }
    let sortie = tar().arg("-tvf").arg(fichier).output().map_err(|_| "TAR.EXE INTROUVABLE".to_string())?;
    if !sortie.status.success() && sortie.stdout.is_empty() {
        let e = depuis_ansi(&sortie.stderr);
        return Err(format!("ARCHIVE ILLISIBLE · {}", e.lines().next().unwrap_or("").trim_start_matches("tar.exe: ").to_uppercase()));
    }
    let texte = depuis_ansi(&sortie.stdout);
    let v: Arc<Vec<Membre>> = Arc::new(texte.lines().filter_map(ligne).collect());
    *DERNIERE.lock().unwrap_or_else(|e| e.into_inner()) = Some((fichier.to_string(), quand, v.clone()));
    Ok(v)
}

/// Contenu d'un dossier de l'archive (`interne` vide : la racine)
pub fn lire(fichier: &str, interne: &str) -> Result<Vec<Entree>, String> {
    let tous = membres(fichier)?;
    let prefixe = if interne.is_empty() { String::new() } else { format!("{interne}/") };
    let mut vus: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    let mut v: Vec<Entree> = Vec::new();
    for m in tous.iter() {
        let Some(reste) = m.chemin.strip_prefix(&prefixe) else { continue };
        if reste.is_empty() {
            continue;
        }
        let (nom, sous) = match reste.split_once('/') {
            Some((n, _)) => (n, true),
            None => (reste, false),
        };
        let dossier = sous || m.dossier;
        match vus.get(&nom.to_lowercase()) {
            // Un dossier déjà vu (créé d'après un de ses fichiers) prend la date de sa propre ligne
            Some(&i) => {
                if !sous && m.dossier {
                    v[i].modif = m.modif;
                }
            }
            None => {
                vus.insert(nom.to_lowercase(), v.len());
                let mut e = crate::dossier::entree_fichier(nom, if dossier { 0 } else { m.taille }, if sous { 0 } else { m.modif }, dossier);
                e.nuage = Nuage::Aucun;
                v.push(e);
            }
        }
    }
    Ok(v)
}

/// Noms bruts des membres désignés par ces chemins internes (un dossier emporte son contenu)
pub fn bruts(fichier: &str, chemins: &[String]) -> Vec<String> {
    let Ok(tous) = membres(fichier) else { return Vec::new() };
    let mut v = Vec::new();
    for c in chemins {
        let dedans = format!("{c}/");
        for m in tous.iter() {
            if m.chemin.eq_ignore_ascii_case(c) || m.chemin.to_lowercase().starts_with(&dedans.to_lowercase()) {
                v.push(m.brut.clone());
            }
        }
    }
    v
}

/// Extrait toute l'archive (membres vide) ou ces membres dans `dest`
pub fn extraire(fichier: &str, membres: &[String], dest: &str) -> Result<(), String> {
    std::fs::create_dir_all(dest).map_err(|e| e.to_string().to_uppercase())?;
    let sortie = tar().arg("-xf").arg(fichier).arg("-C").arg(dest).arg("--").args(membres).output().map_err(|_| "TAR.EXE INTROUVABLE".to_string())?;
    if sortie.status.success() {
        Ok(())
    } else {
        let e = depuis_ansi(&sortie.stderr);
        Err(format!("EXTRACTION · {}", e.lines().next().unwrap_or("ERREUR").trim_start_matches("tar.exe: ").to_uppercase()))
    }
}

/// Dossier temporaire où un fichier de l'archive est extrait pour être ouvert
pub fn dossier_temporaire(fichier: &str) -> String {
    let mut h = crate::outils::Sha256::new();
    h.ajouter(fichier.to_lowercase().as_bytes());
    let cle = &h.finir()[..12];
    let tmp = std::env::temp_dir();
    tmp.join(crate::reglages::NOM_DONNEES).join("archives").join(cle).to_string_lossy().into_owned()
}

/// Crée une archive (le format suit l'extension : .zip, .7z, .tar.gz…) avec ces éléments du dossier `dossier`
pub fn creer(dest: &str, dossier: &str, noms: &[String]) -> Result<(), String> {
    let sortie = tar().arg("-a").arg("-cf").arg(dest).arg("-C").arg(dossier).arg("--").args(noms).output().map_err(|_| "TAR.EXE INTROUVABLE".to_string())?;
    if sortie.status.success() {
        Ok(())
    } else {
        let _ = std::fs::remove_file(dest);
        let e = depuis_ansi(&sortie.stderr);
        Err(format!("COMPRESSION · {}", e.lines().next().unwrap_or("ERREUR").trim_start_matches("tar.exe: ").to_uppercase()))
    }
}

/// Nom libre pour une nouvelle archive ou un dossier d'extraction : `nom.zip`, `nom (2).zip`…
pub fn nom_libre(dossier: &str, base: &str, ext: &str) -> String {
    for k in 1.. {
        let nom = if k == 1 { format!("{base}{ext}") } else { format!("{base} ({k}){ext}") };
        if !std::path::Path::new(&crate::dossier::joindre(dossier, &nom)).exists() {
            return nom;
        }
    }
    unreachable!()
}

/// Nom d'archive sans ses extensions : `photos.tar.gz` → `photos`
pub fn sans_extension(nom: &str) -> String {
    let mut s = nom.to_string();
    for _ in 0..2 {
        match s.rsplit_once('.') {
            Some((b, e)) if !b.is_empty() && (est_archive(&e.to_uppercase()) || e.eq_ignore_ascii_case("tar")) => s = b.to_string(),
            _ => break,
        }
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lignes() {
        let m = ligne("-rw-rw-rw-  0 0      0           6 oct. 20 18:05 ./été  2 espaces.txt").unwrap();
        assert_eq!(m.chemin, "été  2 espaces.txt");
        assert_eq!(m.brut, "./été  2 espaces.txt");
        assert_eq!(m.taille, 6);
        assert!(!m.dossier);
        let d = ligne("drwxrwxrwx  0 0      0           0 Jan 02  2019 sous/").unwrap();
        assert!(d.dossier);
        assert_eq!(d.chemin, "sous");
        assert!(d.modif > 0);
        assert!(ligne("drwxrwxrwx  0 0      0           0 oct. 20 18:05 ./").is_none());
        assert_eq!(sans_extension("photos.tar.gz"), "photos");
        assert_eq!(sans_extension("a.b.zip"), "a.b");
    }

    #[test]
    fn mois_accentues_ne_paniquent_pas() {
        // « août » : 5 octets, dont « û » sur deux octets. La découpe `&m[..3]` paniquait (journal du 04/11/2026).
        assert_eq!(mois("août"), Some(8));
        assert_eq!(mois("AOÛT"), Some(8));
        assert_eq!(mois("déc."), Some(12));
        assert_eq!(mois("févr."), Some(2));
        assert_eq!(mois("ééé"), None, "inconnu, mais sans panique");
    }

    #[test]
    fn ligne_d_aout_ne_panique_pas() {
        let m = ligne("-rw-rw-rw-  0 0      0           6 août 20 18:05 ./a.txt").unwrap();
        assert_eq!(m.chemin, "a.txt");
        assert!(m.modif > 0, "la date d'août est lue");
    }
}
