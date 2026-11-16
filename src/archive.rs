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

/// Liste d'une archive, gardée pour naviguer dedans sans tout relire : (fichier, signature du fichier, membres)
static DERNIERE: Mutex<Option<(String, (u64, u64), Arc<Vec<Membre>>)>> = Mutex::new(None);

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
    // Nom exact d'abord : « juil » commence par « jui » comme « juin », et la recherche par préfixe rendait juin pour juillet
    // (défaut trouvé par le test de date du 16/11/2026) ; le préfixe de trois lettres ne sert qu'en dernier recours.
    noms.iter()
        .position(|n| n.contains(&m.as_str()))
        .or_else(|| noms.iter().position(|n| debut.chars().count() == 3 && n.iter().any(|nom| nom.starts_with(&debut))))
        .map(|i| i as u32 + 1)
}

/// Numéro de jour d'une date du calendrier grégorien (jours depuis le 01/01/1970), pour comparer deux dates
fn numero_de_jour(annee: i64, mois: i64, jour: i64) -> i64 {
    let a = if mois <= 2 { annee - 1 } else { annee };
    let ere = a.div_euclid(400);
    let annee_ere = a - ere * 400;
    let jour_annee = (153 * (if mois > 2 { mois - 3 } else { mois + 9 }) + 2) / 5 + jour - 1;
    let jour_ere = annee_ere * 365 + annee_ere / 4 - annee_ere / 100 + jour_annee;
    ere * 146_097 + jour_ere - 719_468
}

/// Aujourd'hui en heure locale : (année, mois, jour), lu dans la date affichée « jj/mm/aaaa hh:mm »
fn aujourd_hui() -> (i64, i64, i64) {
    let texte = crate::systeme::date(crate::systeme::maintenant());
    let mut p = texte.split(' ').next().unwrap_or("").split('/').map(|n| n.parse::<i64>().unwrap_or(0));
    match (p.next(), p.next(), p.next()) {
        (Some(j), Some(m), Some(a)) if a > 0 => (a, m, j),
        _ => (1980, 1, 1),
    }
}

/// Date de la ligne `ls -l` en FILETIME (UTC, à la minute : tar n'écrit pas les secondes). L'heure lue est locale
/// (bsdtar la convertit avec le fuseau de la machine) ; `filetime` la ramène à UTC en tenant compte de l'heure d'été.
fn date(mo: &str, jour: &str, heure_ou_annee: &str) -> u64 {
    date_vers(mo, jour, heure_ou_annee, aujourd_hui())
}

/// `date` avec « aujourd'hui » donné. Sans année, bsdtar écrit l'heure pour les dates des six derniers mois **et**
/// des six prochains (un fichier daté du futur) : l'année est celle, parmi la précédente, l'actuelle et la suivante,
/// dont la date tombe le plus près d'aujourd'hui (l'ancien code prenait toujours l'année courante : un fichier de
/// décembre vu en février recevait la mauvaise année).
fn date_vers(mo: &str, jour: &str, heure_ou_annee: &str, aujourdhui: (i64, i64, i64)) -> u64 {
    let (Some(m), Ok(j)) = (mois(mo), jour.parse::<u32>()) else { return 0 };
    let (h, mi) = match heure_ou_annee.split_once(':') {
        Some((h, mi)) => (h.parse().unwrap_or(0), mi.parse().unwrap_or(0)),
        None => return crate::systeme::filetime(heure_ou_annee.parse().unwrap_or(1980), m, j, 0, 0),
    };
    let (a0, m0, j0) = aujourdhui;
    let aujourdhui = numero_de_jour(a0, m0, j0);
    let mut annees = [a0 - 1, a0, a0 + 1];
    annees.sort_by_key(|&a| (numero_de_jour(a, m as i64, j as i64) - aujourdhui).abs());
    // Un 29 février n'existe pas dans toutes les années : on prend la plus proche qui donne une date valide
    annees.iter().map(|&a| crate::systeme::filetime(a as u32, m, j, h, mi)).find(|&ft| ft != 0).unwrap_or(0)
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

/// Empreinte d'un fichier : date de modification en centaines de nanosecondes (la précision du système de fichiers,
/// pas la seconde : deux écritures dans la même seconde différaient mal) et taille. `None` si illisible : jamais gardé.
fn signature(f: &str) -> Option<(u64, u64)> {
    let m = std::fs::metadata(f).ok()?;
    let d = m.modified().ok()?.duration_since(std::time::UNIX_EPOCH).ok()?;
    Some((d.as_secs() * 10_000_000 + u64::from(d.subsec_nanos() / 100), m.len()))
}

/// Tous les éléments de l'archive (relue seulement si le fichier a changé)
pub fn membres(fichier: &str) -> Result<Arc<Vec<Membre>>, String> {
    let quand = signature(fichier);
    if let (Some(q), Some((f, d, v))) = (quand, DERNIERE.lock().unwrap_or_else(|e| e.into_inner()).as_ref()) {
        if f.eq_ignore_ascii_case(fichier) && *d == q {
            return Ok(v.clone());
        }
    }
    let sortie = tar().arg("-tvf").arg(fichier).output().map_err(|e| erreur_lancement(&e))?;
    if !sortie.status.success() && sortie.stdout.is_empty() {
        let e = depuis_ansi(&sortie.stderr);
        return Err(format!("ARCHIVE ILLISIBLE · {}", e.lines().next().unwrap_or("").trim_start_matches("tar.exe: ").to_uppercase()));
    }
    let texte = depuis_ansi(&sortie.stdout);
    let v: Arc<Vec<Membre>> = Arc::new(texte.lines().filter_map(ligne).collect());
    // Signature relue APRÈS tar : si l'archive a changé pendant la lecture, l'ancienne empreinte ne correspond plus
    // à la liste et la prochaine lecture recommence (au pire une lecture de trop, jamais une liste périmée gardée)
    if let (Some(avant), Some(apres)) = (quand, signature(fichier)) {
        if avant == apres {
            *DERNIERE.lock().unwrap_or_else(|e| e.into_inner()) = Some((fichier.to_string(), avant, v.clone()));
        }
    }
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

/// Message d'une commande `tar` qui n'a pas pu être lancée : « introuvable » seulement si c'est vrai
/// (une ligne de commande trop longue donnait aussi « TAR.EXE INTROUVABLE », message trompeur).
fn erreur_lancement(e: &std::io::Error) -> String {
    if e.kind() == std::io::ErrorKind::NotFound {
        "TAR.EXE INTROUVABLE".to_string()
    } else {
        format!("TAR.EXE NE SE LANCE PAS · {}", e.to_string().to_uppercase())
    }
}

/// Un nom que Windows traiterait autrement qu'un fichier ordinaire : périphérique réservé (`CON`, `NUL`, `COM1`,
/// `LPT1.log`…), flux de données (`a:b`), point ou espace final (supprimé par Win32, donc un autre nom que celui de l'archive).
/// tar.exe les extrait sans rien dire (essai du 16/11/2026) ; ouvrir ensuite `CON` ouvre la console, pas le fichier.
fn nom_dangereux(chemin: &str) -> bool {
    chemin.replace('\\', "/").split('/').any(|partie| {
        if partie.is_empty() || partie == "." {
            return false;
        }
        let racine = partie.split('.').next().unwrap_or("").trim_end().to_uppercase();
        let peripherique = matches!(racine.as_str(), "CON" | "PRN" | "AUX" | "NUL" | "CONIN$" | "CONOUT$")
            || ["COM", "LPT"].iter().any(|p| racine.strip_prefix(p).is_some_and(|n| matches!(n, "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9" | "¹" | "²" | "³")));
        peripherique || partie.contains(':') || (partie != ".." && (partie.ends_with('.') || partie.ends_with(' ')))
    })
}

/// Longueur maximale des noms passés à un appel de `tar` : la ligne de commande Windows plafonne à 32 767 caractères
const LIGNE_MAX: usize = 16_000;

/// Découpe les noms en groupes dont la ligne de commande reste sous `LIGNE_MAX` (guillemets et espaces compris)
fn par_lots(noms: &[String]) -> Vec<&[String]> {
    let mut lots = Vec::new();
    let (mut debut, mut taille) = (0, 0);
    for (i, n) in noms.iter().enumerate() {
        let t = n.len() + 3;
        if taille + t > LIGNE_MAX && i > debut {
            lots.push(&noms[debut..i]);
            debut = i;
            taille = 0;
        }
        taille += t;
    }
    if debut < noms.len() {
        lots.push(&noms[debut..]);
    }
    lots
}

/// Extrait toute l'archive (membres vide) ou ces membres dans `dest`.
/// Refuse d'abord, sans rien écrire, une archive qui contient un nom que Windows traiterait en périphérique ou en flux.
pub fn extraire(fichier: &str, membres: &[String], dest: &str) -> Result<(), String> {
    if let Ok(tous) = self::membres(fichier) {
        let demandes: std::collections::HashSet<&str> = membres.iter().map(String::as_str).collect();
        if let Some(m) = tous.iter().find(|m| (demandes.is_empty() || demandes.contains(m.brut.as_str())) && nom_dangereux(&m.chemin)) {
            return Err(format!("EXTRACTION REFUSÉE · NOM RÉSERVÉ PAR WINDOWS · {}", m.chemin.to_uppercase()));
        }
    }
    std::fs::create_dir_all(dest).map_err(|e| e.to_string().to_uppercase())?;
    let un_lot = |noms: &[String]| -> Result<(), String> {
        let sortie = tar().arg("-xf").arg(fichier).arg("-C").arg(dest).arg("--").args(noms).output().map_err(|e| erreur_lancement(&e))?;
        if sortie.status.success() {
            Ok(())
        } else {
            let e = depuis_ansi(&sortie.stderr);
            Err(format!("EXTRACTION · {}", e.lines().next().unwrap_or("ERREUR").trim_start_matches("tar.exe: ").to_uppercase()))
        }
    };
    if membres.is_empty() {
        return un_lot(&[]);
    }
    for lot in par_lots(membres) {
        un_lot(lot)?;
    }
    Ok(())
}

/// Contenu brut de ces éléments (noms comme `Membre::chemin`), écrit les uns après les autres dans l'ordre de l'archive :
/// sert à lire le XML d'un .docx ou d'un .xlsx sans rien écrire sur le disque. 32 Mo au plus.
pub fn membres_en_sortie(fichier: &str, noms: &[&str]) -> Option<Vec<u8>> {
    use std::io::Read;
    use std::process::Stdio;
    if noms.is_empty() {
        return None;
    }
    let mut enfant = tar().arg("-xOf").arg(fichier).arg("--").args(noms).stdout(Stdio::piped()).stderr(Stdio::null()).spawn().ok()?;
    let mut octets = Vec::new();
    if let Some(sortie) = enfant.stdout.as_mut() {
        let _ = sortie.take(32 << 20).read_to_end(&mut octets);
    }
    // Coupé à 32 Mo : on arrête tar au lieu de l'attendre
    let _ = enfant.kill();
    let _ = enfant.wait();
    if octets.is_empty() {
        None
    } else {
        Some(octets)
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
    (1..)
        .map(|k| if k == 1 { format!("{base}{ext}") } else { format!("{base} ({k}){ext}") })
        .find(|nom| !std::path::Path::new(&crate::dossier::joindre(dossier, nom)).exists())
        .unwrap_or_else(|| format!("{base}{ext}"))
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
    fn les_douze_mois_en_anglais_et_en_francais() {
        let en = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
        let fr = ["janv.", "févr.", "mars", "avr.", "mai", "juin", "juil.", "août", "sept.", "oct.", "nov.", "déc."];
        for i in 0..12 {
            assert_eq!(mois(en[i]), Some(i as u32 + 1), "{}", en[i]);
            assert_eq!(mois(fr[i]), Some(i as u32 + 1), "{}", fr[i]);
        }
    }

    // ───── Archives hostiles, extraites par le vrai tar.exe de cette machine (audit externe, lot 4) ─────
    // L'auditeur ne prouve aucune faille mais demande de ne pas « supposer » que libarchive protège : chaque test fabrique une
    // archive tar octet par octet (aucun outil ne sait écrire `../`), l'extrait, puis cherche ce qui serait sorti de `dest`.

    struct M<'a> {
        nom: &'a str,
        genre: u8,
        lien: &'a str,
        contenu: &'a [u8],
    }

    fn fichier<'a>(nom: &'a str, contenu: &'a [u8]) -> M<'a> {
        M { nom, genre: b'0', lien: "", contenu }
    }
    fn repertoire(nom: &str) -> M<'_> {
        M { nom, genre: b'5', lien: "", contenu: b"" }
    }
    fn symbolique<'a>(nom: &'a str, cible: &'a str) -> M<'a> {
        M { nom, genre: b'2', lien: cible, contenu: b"" }
    }
    fn physique<'a>(nom: &'a str, cible: &'a str) -> M<'a> {
        M { nom, genre: b'1', lien: cible, contenu: b"" }
    }

    fn ecrire_champ(h: &mut [u8; 512], debut: usize, texte: &[u8]) {
        h[debut..debut + texte.len()].copy_from_slice(texte);
    }

    /// Archive au format ustar (en-têtes de 512 octets), sans passer par aucun outil
    fn fabriquer_tar(membres: &[M]) -> Vec<u8> {
        let mut t = Vec::new();
        for m in membres {
            assert!(m.nom.len() <= 100 && m.lien.len() <= 100, "nom trop long pour ustar : {}", m.nom);
            let mut h = [0u8; 512];
            ecrire_champ(&mut h, 0, m.nom.as_bytes());
            ecrire_champ(&mut h, 100, b"0000644\0");
            ecrire_champ(&mut h, 108, b"0000000\0");
            ecrire_champ(&mut h, 116, b"0000000\0");
            ecrire_champ(&mut h, 124, format!("{:011o}\0", m.contenu.len()).as_bytes());
            ecrire_champ(&mut h, 136, format!("{:011o}\0", 1_700_000_000u64).as_bytes());
            ecrire_champ(&mut h, 148, b"        ");
            h[156] = m.genre;
            ecrire_champ(&mut h, 157, m.lien.as_bytes());
            ecrire_champ(&mut h, 257, b"ustar\0");
            ecrire_champ(&mut h, 263, b"00");
            let somme: u32 = h.iter().map(|&o| o as u32).sum();
            ecrire_champ(&mut h, 148, format!("{somme:06o}\0 ").as_bytes());
            t.extend_from_slice(&h);
            t.extend_from_slice(m.contenu);
            t.resize(t.len().div_ceil(512) * 512, 0);
        }
        t.resize(t.len() + 1024, 0);
        t
    }

    /// Dossier d'essai vide : `<temp>\spacefiles-test-archive\<nom>`
    fn bac(nom: &str) -> std::path::PathBuf {
        let p = std::env::temp_dir().join("spacefiles-test-archive").join(nom);
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    /// Écrit l'archive et l'extrait en entier dans `<bac>\dest`. Rend (archive, dest, résultat de l'extraction).
    fn extraire_tout(bac: &std::path::Path, membres: &[M]) -> (String, std::path::PathBuf, Result<(), String>) {
        let a = bac.join("a.tar");
        std::fs::write(&a, fabriquer_tar(membres)).unwrap();
        let dest = bac.join("dest");
        let r = extraire(&a.to_string_lossy(), &[], &dest.to_string_lossy());
        (a.to_string_lossy().into_owned(), dest, r)
    }

    /// Ce qui est sorti de `dest` : tout ce qui porte ce préfixe dans le bac ou ses deux parents
    fn evade(bac: &std::path::Path, prefixe: &str) -> Vec<String> {
        let mut v = Vec::new();
        for d in [Some(bac), bac.parent(), bac.parent().and_then(|p| p.parent())].into_iter().flatten() {
            if let Ok(lu) = std::fs::read_dir(d) {
                v.extend(lu.flatten().map(|e| e.path().to_string_lossy().into_owned()).filter(|p| p.rsplit('\\').next().is_some_and(|n| n.starts_with(prefixe))));
            }
        }
        v
    }

    #[test]
    fn extraction_refuse_les_remontees() {
        let b = bac("remontees");
        let (_, dest, r) = extraire_tout(
            &b,
            &[fichier("../EVADE-up.txt", b"x"), fichier("sous/../../EVADE-up2.txt", b"x"), fichier("../../EVADE-up3.txt", b"x"), fichier("ok.txt", b"bon")],
        );
        eprintln!("remontées : {r:?} ; dest = {:?}", std::fs::read_dir(&dest).map(|d| d.flatten().map(|e| e.file_name()).collect::<Vec<_>>()));
        assert_eq!(evade(&b, "EVADE-up"), Vec::<String>::new(), "un membre `..` a écrit hors de la destination");
    }

    #[test]
    fn extraction_n_ecrit_pas_a_un_chemin_absolu() {
        let b = bac("absolu");
        // Chemin absolu vers le bac lui-même (jamais vers la racine du disque : si tar.exe suivait, l'essai resterait inoffensif)
        let cible = format!("{}/EVADE-abs.txt", b.to_string_lossy().replace('\\', "/"));
        let unc = format!("//?/{}/EVADE-abs2.txt", b.to_string_lossy().replace('\\', "/"));
        let (_, dest, r) = extraire_tout(&b, &[fichier(&cible, b"x"), fichier(&unc, b"x"), fichier("ok.txt", b"bon")]);
        eprintln!("absolu : {r:?} ; dest = {:?}", std::fs::read_dir(&dest).map(|d| d.flatten().map(|e| e.file_name()).collect::<Vec<_>>()));
        assert_eq!(evade(&b, "EVADE-abs"), Vec::<String>::new(), "un chemin absolu a écrit hors de la destination");
    }

    #[test]
    fn extraction_ne_suit_pas_un_lien_symbolique_vers_l_exterieur() {
        let b = bac("symbolique");
        let dehors = b.to_string_lossy().replace('\\', "/");
        let (_, dest, r) = extraire_tout(
            &b,
            &[symbolique("lien", &dehors), fichier("lien/EVADE-sym.txt", b"x"), symbolique("lien2", ".."), fichier("lien2/EVADE-sym2.txt", b"x")],
        );
        eprintln!("symbolique : {r:?} ; dest = {:?}", std::fs::read_dir(&dest).map(|d| d.flatten().map(|e| e.file_name()).collect::<Vec<_>>()));
        assert_eq!(evade(&b, "EVADE-sym"), Vec::<String>::new(), "un lien symbolique a fait écrire hors de la destination");
    }

    #[test]
    fn extraction_ne_cree_pas_de_lien_physique_vers_l_exterieur() {
        let b = bac("physique");
        std::fs::write(b.join("secret.txt"), "SECRET").unwrap();
        let (_, dest, r) = extraire_tout(
            &b,
            &[physique("dur", "../secret.txt"), fichier("dur", b"ECRASE"), physique("dur2", &format!("{}/secret.txt", b.to_string_lossy().replace('\\', "/"))), fichier("dur2", b"ECRASE")],
        );
        eprintln!("physique : {r:?} ; dest = {:?}", std::fs::read_dir(&dest).map(|d| d.flatten().map(|e| e.file_name()).collect::<Vec<_>>()));
        assert_eq!(std::fs::read_to_string(b.join("secret.txt")).unwrap(), "SECRET", "un lien physique a servi à écraser un fichier hors de la destination");
    }

    #[test]
    fn extraction_noms_reserves_de_windows() {
        let b = bac("reserves");
        let (a, dest, r) = extraire_tout(
            &b,
            &[fichier("CON", b"x"), fichier("NUL", b"x"), fichier("aux.txt", b"x"), fichier("COM1", b"x"), fichier("LPT1.log", b"x"), fichier("dossier/PRN", b"x"), fichier("ok.txt", b"bon")],
        );
        let presents: Vec<String> = std::fs::read_dir(&dest).map(|d| d.flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect()).unwrap_or_default();
        eprintln!("réservés : {r:?} ; dest = {presents:?} ; liste = {:?}", membres(&a).map(|m| m.iter().map(|x| x.chemin.clone()).collect::<Vec<_>>()));
        assert_eq!(evade(&b, "EVADE"), Vec::<String>::new());
        // Sans la garde, tar.exe créait CON, NUL, COM1… (constaté le 16/11/2026) : on refuse avant d'écrire quoi que ce soit
        assert!(r.as_ref().is_err_and(|e| e.contains("RÉSERVÉ")), "extraction refusée avec un message clair : {r:?}");
        assert!(presents.is_empty(), "rien n'est écrit : {presents:?}");
        // Un membre qu'on ne demande pas ne bloque pas : seul le fichier sain est extrait
        let dest2 = b.join("dest-choisi");
        extraire(&a, &["ok.txt".to_string()], &dest2.to_string_lossy()).expect("le fichier sain s'extrait");
        assert_eq!(std::fs::read(dest2.join("ok.txt")).unwrap(), b"bon");
        assert!(!dest2.join("CON").exists());
    }

    #[test]
    fn noms_dangereux_reconnus() {
        for n in ["CON", "con", "NUL", "aux.txt", "COM1", "com9.log", "LPT1.log", "dossier/PRN", "dossier\\nul.x", "a:flux", "fin.", "fin ", "CONIN$", "a/COM1 .txt"] {
            assert!(nom_dangereux(n), "{n} doit être refusé");
        }
        for n in ["ok.txt", "console.txt", "COM10", "COM0", "LPT", "auxiliaire", "a/b/c.tar.gz", "..a", ".cache", "dossier/.", "COMMENT.md", "nul_x"] {
            assert!(!nom_dangereux(n), "{n} doit passer");
        }
    }

    #[test]
    fn noms_par_lots_sous_la_limite() {
        let noms: Vec<String> = (0..5_000).map(|i| format!("dossier-long/sous-dossier/fichier-{i:05}.txt")).collect();
        let lots = par_lots(&noms);
        assert!(lots.len() > 1, "5 000 noms ne tiennent pas sur une seule ligne");
        assert_eq!(lots.iter().map(|l| l.len()).sum::<usize>(), noms.len(), "aucun nom perdu ni doublé");
        for l in &lots {
            assert!(l.iter().map(|n| n.len() + 3).sum::<usize>() <= LIGNE_MAX);
        }
        assert!(par_lots(&[]).is_empty());
        let un_geant = vec!["x".repeat(LIGNE_MAX + 10)];
        assert_eq!(par_lots(&un_geant).len(), 1, "un nom plus long que la limite part seul");
    }

    #[test]
    fn la_liste_est_relue_si_l_archive_change_dans_la_meme_seconde() {
        let b = bac("cache");
        let a = b.join("a.tar");
        std::fs::write(&a, fabriquer_tar(&[fichier("avant.txt", b"x")])).unwrap();
        let t0 = std::time::UNIX_EPOCH + std::time::Duration::from_secs(1_800_000_000);
        std::fs::OpenOptions::new().write(true).open(&a).unwrap().set_modified(t0).unwrap();
        let chemin = a.to_string_lossy().into_owned();
        assert_eq!(membres(&chemin).unwrap()[0].chemin, "avant.txt");
        // Même taille, même seconde (1 ms plus tard) : l'ancienne empreinte à la seconde ne voyait pas la différence
        std::fs::write(&a, fabriquer_tar(&[fichier("apres.txt", b"x")])).unwrap();
        assert_eq!(std::fs::metadata(&a).unwrap().len(), fabriquer_tar(&[fichier("avant.txt", b"x")]).len() as u64, "même taille");
        std::fs::OpenOptions::new().write(true).open(&a).unwrap().set_modified(t0 + std::time::Duration::from_millis(1)).unwrap();
        assert_eq!(membres(&chemin).unwrap()[0].chemin, "apres.txt");
        // Même date, taille différente : relue aussi
        std::fs::write(&a, fabriquer_tar(&[fichier("apres.txt", b"x"), fichier("plus.txt", b"y")])).unwrap();
        std::fs::OpenOptions::new().write(true).open(&a).unwrap().set_modified(t0 + std::time::Duration::from_millis(1)).unwrap();
        assert_eq!(membres(&chemin).unwrap().len(), 2);
        // Inchangée : la même liste est rendue sans relecture (même allocation)
        let (l1, l2) = (membres(&chemin).unwrap(), membres(&chemin).unwrap());
        assert!(Arc::ptr_eq(&l1, &l2));
    }

    #[test]
    fn la_signature_a_la_precision_de_100_ns() {
        let b = bac("signature");
        let f = b.join("s.bin");
        std::fs::write(&f, b"abc").unwrap();
        let t0 = std::time::UNIX_EPOCH + std::time::Duration::from_secs(1_800_000_000);
        let ouvrir = || std::fs::OpenOptions::new().write(true).open(&f).unwrap();
        ouvrir().set_modified(t0).unwrap();
        let s0 = signature(&f.to_string_lossy()).unwrap();
        ouvrir().set_modified(t0 + std::time::Duration::from_nanos(100)).unwrap();
        let s1 = signature(&f.to_string_lossy()).unwrap();
        assert_eq!(s1.0, s0.0 + 1, "100 ns d'écart se voient");
        assert_eq!(s0.1, 3);
        assert!(signature(&b.join("absent").to_string_lossy()).is_none());
    }

    #[test]
    fn la_date_des_membres_est_l_heure_locale_convertie_en_utc() {
        // Aller-retour avec la conversion du système : la ligne de tar donne l'heure locale affichée
        let ft = crate::systeme::filetime(2026, 7, 14, 9, 30);
        assert_ne!(ft, 0);
        assert_eq!(crate::systeme::date(ft), "14/07/2026 09:30");
        let m = date_vers("juil.", "14", "09:30", (2026, 11, 16));
        assert_eq!(m, ft, "même instant UTC que la date locale 14/07/2026 09:30");
        assert_eq!(crate::systeme::date(date_vers("Jul", "14", "09:30", (2026, 11, 16))), "14/07/2026 09:30");
        // Avec l'année : minuit local
        assert_eq!(crate::systeme::date(date_vers("oct.", "20", "2019", (2026, 11, 16))), "20/10/2019 00:00");
    }

    #[test]
    fn l_annee_est_choisie_au_plus_pres_d_aujourd_hui() {
        let vu = |mo: &str, j: &str, h: &str, auj| crate::systeme::date(date_vers(mo, j, h, auj));
        // Un fichier de décembre vu en février : décembre de l'année précédente
        assert_eq!(vu("déc.", "20", "10:00", (2026, 2, 10)), "20/12/2025 10:00");
        // Un fichier de janvier vu en novembre : janvier de l'année suivante (fichier daté du futur)
        assert_eq!(vu("janv.", "5", "10:00", (2026, 11, 16)), "05/01/2027 10:00");
        // Cas ordinaires : l'année courante
        assert_eq!(vu("oct.", "20", "18:05", (2026, 11, 16)), "20/10/2026 18:05");
        assert_eq!(vu("Mar", "3", "08:00", (2026, 6, 1)), "03/03/2026 08:00");
        // Autour du 31 décembre / 1er janvier
        assert_eq!(vu("Dec", "31", "23:59", (2027, 1, 1)), "31/12/2026 23:59");
        assert_eq!(vu("Jan", "1", "00:01", (2026, 12, 31)), "01/01/2027 00:01");
        // 29 février : année bissextile la plus proche
        assert_eq!(vu("Feb", "29", "12:00", (2028, 3, 1)), "29/02/2028 12:00");
    }

    #[test]
    fn numero_de_jour_connu() {
        assert_eq!(numero_de_jour(1970, 1, 1), 0);
        assert_eq!(numero_de_jour(2000, 3, 1), 11_017);
        assert_eq!(numero_de_jour(2026, 11, 16) - numero_de_jour(2026, 11, 15), 1);
        assert_eq!(numero_de_jour(2027, 1, 1) - numero_de_jour(2026, 1, 1), 365);
        assert_eq!(numero_de_jour(2028, 1, 1) - numero_de_jour(2027, 1, 1), 365);
        assert_eq!(numero_de_jour(2029, 1, 1) - numero_de_jour(2028, 1, 1), 366);
    }

    #[test]
    fn deux_noms_qui_ne_different_que_par_la_casse() {
        let b = bac("casse");
        let (a, dest, r) = extraire_tout(&b, &[fichier("Lisez.TXT", b"un"), fichier("lisez.txt", b"deux"), fichier("D/x.txt", b"1"), fichier("d/y.txt", b"2")]);
        let presents: Vec<String> = std::fs::read_dir(&dest).map(|d| d.flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect()).unwrap_or_default();
        eprintln!("casse : {r:?} ; dest = {presents:?}");
        let vus = lire(&a, "").unwrap();
        eprintln!("casse : liste = {:?}", vus.iter().map(|e| e.nom.clone()).collect::<Vec<_>>());
        assert_eq!(vus.len(), 2, "Lisez.TXT / lisez.txt et D / d ne font qu'une entrée chacun (Windows ne distingue pas la casse)");
    }

    #[test]
    fn tres_grand_nombre_de_membres() {
        let b = bac("beaucoup");
        let noms: Vec<String> = (0..6_000).map(|i| format!("d/fichier-{i:05}.txt")).collect();
        let mut membres_tar: Vec<M> = vec![repertoire("d/")];
        membres_tar.extend(noms.iter().map(|n| fichier(n, b"")));
        let a = b.join("a.tar");
        std::fs::write(&a, fabriquer_tar(&membres_tar)).unwrap();
        let a = a.to_string_lossy().into_owned();
        let t0 = std::time::Instant::now();
        let liste = membres(&a).expect("liste");
        eprintln!("beaucoup : {} membres listés en {:?}", liste.len(), t0.elapsed());
        assert_eq!(liste.len(), 6_001);
        // Extraction de toute l'archive : aucun nom en argument
        let dest = b.join("dest-tout");
        extraire(&a, &[], &dest.to_string_lossy()).expect("tout extraire");
        assert_eq!(std::fs::read_dir(dest.join("d")).unwrap().count(), 6_000);
        // Extraction du seul dossier « d » : `bruts` emporte son contenu, soit 20 001 noms sur la ligne de commande
        let bruts_d = bruts(&a, &["d".to_string()]);
        eprintln!("beaucoup : {} noms bruts pour le dossier d", bruts_d.len());
        let dest2 = b.join("dest-d");
        let r = extraire(&a, &bruts_d, &dest2.to_string_lossy());
        eprintln!("beaucoup : extraire(dossier) = {r:?}");
        assert!(r.is_ok(), "le dossier de 6 000 fichiers ne s'extrait pas : {r:?}");
        assert_eq!(std::fs::read_dir(dest2.join("d")).unwrap().count(), 6_000);
    }

    #[test]
    fn ligne_d_aout_ne_panique_pas() {
        let m = ligne("-rw-rw-rw-  0 0      0           6 août 20 18:05 ./a.txt").unwrap();
        assert_eq!(m.chemin, "a.txt");
        assert!(m.modif > 0, "la date d'août est lue");
    }
}
