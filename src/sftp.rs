// SFTP en lecture : parcourir un serveur SSH et en télécharger des fichiers, par le client OpenSSH
// livré avec Windows (sftp.exe). Authentification par clé SSH seulement (agent ou ~/.ssh) : aucun mot
// de passe n'est demandé ni gardé, et un serveur inconnu doit d'abord avoir été accepté avec `ssh`.

use std::io::Write;
use std::os::windows::process::CommandExt;
use std::process::{Command, Stdio};

use crate::dossier::Entree;

const SANS_FENETRE: u32 = 0x0800_0000;

/// `sftp://utilisateur@serveur:port/chemin` → (« utilisateur@serveur:port », « /chemin »)
pub fn decouper(adresse: &str) -> Option<(String, String)> {
    let a = adresse.trim();
    let reste = a.get(..7).filter(|p| p.eq_ignore_ascii_case("sftp://")).map(|_| &a[7..])?;
    let reste = reste.replace('\\', "/");
    let (hote, chemin) = match reste.find('/') {
        Some(i) => (reste[..i].to_string(), reste[i..].to_string()),
        None => (reste.clone(), "/".to_string()),
    };
    // Un hôte en « -o… » serait lu comme une option de sftp.exe (ProxyCommand = exécution de commande)
    if hote.is_empty() || hote.starts_with('-') {
        return None;
    }
    Some((hote, normaliser(&chemin)))
}

/// « /a//b/ » → « /a/b »
pub fn normaliser(chemin: &str) -> String {
    let morceaux: Vec<&str> = chemin.split('/').filter(|m| !m.is_empty() && *m != ".").collect();
    format!("/{}", morceaux.join("/"))
}

pub fn adresse(hote: &str, chemin: &str) -> String {
    format!("sftp://{hote}{chemin}")
}

pub fn joindre(chemin: &str, nom: &str) -> String {
    if chemin.ends_with('/') {
        format!("{chemin}{nom}")
    } else {
        format!("{chemin}/{nom}")
    }
}

/// Texte entre guillemets pour un script sftp
fn guillemets(s: &str) -> String {
    format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
}

/// Lance sftp.exe en mode lot avec ce script ; renvoie sa sortie
fn executer(hote: &str, script: &str) -> Result<String, String> {
    let exe = format!("{}\\System32\\OpenSSH\\sftp.exe", std::env::var("WINDIR").unwrap_or_else(|_| r"C:\Windows".into()));
    if !std::path::Path::new(&exe).exists() {
        return Err("CLIENT OPENSSH ABSENT · PARAMETRES > SYSTEME > FONCTIONNALITES FACULTATIVES".into());
    }
    let (cible, port) = match hote.rsplit_once(':') {
        Some((h, p)) if p.chars().all(|c| c.is_ascii_digit()) && !p.is_empty() => (h, Some(p)),
        _ => (hote, None),
    };
    let mut c = Command::new(exe);
    c.args(["-b", "-", "-o", "BatchMode=yes", "-o", "ConnectTimeout=10"]);
    if let Some(p) = port {
        c.args(["-P", p]);
    }
    c.arg("--").arg(cible).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).creation_flags(SANS_FENETRE);
    let mut p = c.spawn().map_err(|e| e.to_string().to_uppercase())?;
    if let Some(mut entree) = p.stdin.take() {
        let _ = entree.write_all(script.as_bytes());
    }
    let sortie = p.wait_with_output().map_err(|e| e.to_string().to_uppercase())?;
    if !sortie.status.success() {
        let err = String::from_utf8_lossy(&sortie.stderr).to_lowercase();
        return Err(if err.contains("permission denied") {
            "CLE SSH REFUSEE · AJOUTEZ VOTRE CLE PUBLIQUE AU SERVEUR (SSH-KEYGEN, PUIS AUTHORIZED_KEYS)".into()
        } else if err.contains("host key verification failed") || err.contains("no matching host key") {
            "SERVEUR INCONNU · CONNECTEZ-VOUS UNE FOIS AVEC SSH DANS UN TERMINAL POUR L'ACCEPTER".into()
        } else if err.contains("could not resolve") || err.contains("timed out") || err.contains("connection refused") {
            "SERVEUR SFTP INJOIGNABLE".into()
        } else if err.contains("no such file") || err.contains("not found") {
            "DOSSIER INTROUVABLE SUR LE SERVEUR".into()
        } else {
            err.lines().rev().find(|l| !l.trim().is_empty()).unwrap_or("ERREUR SFTP").trim().to_uppercase()
        });
    }
    Ok(String::from_utf8_lossy(&sortie.stdout).into_owned())
}

/// Date d'une ligne `ls -l` (« Mar 15 12:00 » ou « Mar 15 2024 ») en FILETIME
fn date(mois: &str, jour: &str, heure_ou_annee: &str) -> u64 {
    const MOIS: [&str; 12] = ["jan", "feb", "mar", "apr", "may", "jun", "jul", "aug", "sep", "oct", "nov", "dec"];
    let Some(m) = MOIS.iter().position(|x| mois.eq_ignore_ascii_case(x)) else { return 0 };
    let j: i64 = jour.parse().unwrap_or(1);
    let maintenant_s = (crate::systeme::maintenant() / 10_000_000) as i64 - 11_644_473_600;
    let annee_courante = 1970 + maintenant_s / 31_556_952;
    let (annee, h, mi) = match heure_ou_annee.split_once(':') {
        Some((h, mi)) => (annee_courante, h.parse().unwrap_or(0), mi.parse().unwrap_or(0)),
        None => (heure_ou_annee.parse().unwrap_or(annee_courante), 0, 0),
    };
    // Jours depuis 1970 (algorithme de H. Hinnant)
    let jours = |a: i64, m: i64, j: i64| {
        let a = if m <= 2 { a - 1 } else { a };
        let ere = a.div_euclid(400);
        let ae = a - ere * 400;
        let jm = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + j - 1;
        let je = ae * 365 + ae / 4 - ae / 100 + jm;
        ere * 146_097 + je - 719_468
    };
    let mut s = jours(annee, m as i64 + 1, j) * 86_400 + h * 3600 + mi * 60;
    // « Mar 15 12:00 » dans le futur : c'était l'an dernier
    if heure_ou_annee.contains(':') && s > maintenant_s + 86_400 {
        s = jours(annee - 1, m as i64 + 1, j) * 86_400 + h * 3600 + mi * 60;
    }
    ((s + 11_644_473_600).max(0) as u64) * 10_000_000
}

/// Contenu d'un dossier distant
pub fn lire(hote: &str, chemin: &str) -> Result<Vec<Entree>, String> {
    let sortie = executer(hote, &format!("ls -la {}\n", guillemets(chemin)))?;
    let mut v = Vec::new();
    for ligne in sortie.lines() {
        if ligne.starts_with("sftp>") || ligne.len() < 10 {
            continue;
        }
        // 8 champs (droits, liens, propriétaire, groupe, taille, mois, jour, heure), puis le nom
        let mut champs = Vec::new();
        let mut reste = ligne;
        for _ in 0..8 {
            reste = reste.trim_start();
            let fin = reste.find(char::is_whitespace).unwrap_or(reste.len());
            champs.push(&reste[..fin]);
            reste = &reste[fin..];
        }
        let nom = reste.strip_prefix(' ').unwrap_or(reste).trim_start();
        if champs.len() < 8 || nom.is_empty() || nom == "." || nom == ".." {
            continue;
        }
        let droits = champs[0];
        let dossier = droits.starts_with('d');
        let lien = droits.starts_with('l');
        let nom = if lien { nom.split(" -> ").next().unwrap_or(nom) } else { nom };
        let taille = champs[4].parse().unwrap_or(0);
        let mut e = crate::dossier::entree_fichier(nom, if dossier { 0 } else { taille }, date(champs[5], champs[6], champs[7]), dossier);
        e.cache = nom.starts_with('.');
        e.lien = lien;
        v.push(e);
    }
    Ok(v)
}

/// Télécharge ces chemins distants dans le dossier local `dest`
pub fn telecharger(hote: &str, distants: &[String], dest: &str) -> Result<(), String> {
    let local = dest.replace('\\', "/");
    let script: String = distants.iter().map(|d| format!("get -r {} {}\n", guillemets(d), guillemets(&format!("{local}/")))).collect();
    executer(hote, &script).map(|_| ())
}

/// Dossier temporaire pour ouvrir un fichier distant
pub fn dossier_temporaire(hote: &str) -> String {
    let propre: String = hote.chars().map(|c| if c.is_alphanumeric() || c == '.' || c == '-' { c } else { '_' }).collect();
    std::env::temp_dir().join(crate::reglages::NOM_DONNEES).join("sftp").join(propre).to_string_lossy().into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn adresses() {
        assert_eq!(decouper("sftp://moi@nas:2222/home/moi/"), Some(("moi@nas:2222".into(), "/home/moi".into())));
        assert_eq!(decouper("SFTP://nas"), Some(("nas".into(), "/".into())));
        assert_eq!(decouper("\\\\nas"), None);
        assert_eq!(normaliser("/a//b/./"), "/a/b");
        assert!(date("Jan", "1", "1970") == 116_444_736_000_000_000);
    }
}
