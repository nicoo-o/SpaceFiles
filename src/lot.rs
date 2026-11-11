// Renommage par lot (E16) : motif avec compteur et date (EXIF), chercher / remplacer (texte ou regex),
// aperçu avant de valider, renommage en deux temps (les échanges de noms marchent), Ctrl+Z.

use regex::Regex;

use crate::edition::Edition;

/// Champs du formulaire
pub const MOTIF: usize = 0;
pub const CHERCHER: usize = 1;
pub const REMPLACER: usize = 2;
pub const LIBELLES: [&str; 3] = ["MOTIF", "CHERCHER", "REMPLACER PAR"];

pub struct Lot {
    pub dossier: String,
    /// Noms d'origine, dans l'ordre d'affichage (le compteur suit cet ordre)
    pub noms: Vec<String>,
    /// Date de chaque élément pour {date} : prise de vue (EXIF) pour une photo, sinon modification
    pub dates: Vec<u64>,
    pub champs: [Edition; 3],
    pub focus: usize,
    /// Chercher est une expression régulière (Ctrl+R)
    pub regex: bool,
    /// Défilement de l'aperçu (en lignes)
    pub defilement: usize,
}

/// Nom et extension : `photo.jpg` → (`photo`, `jpg`) ; un dossier ou `.gitignore` n'a pas d'extension
pub fn decouper(nom: &str, dossier: bool) -> (&str, &str) {
    match nom.rfind('.') {
        Some(i) if i > 0 && !dossier => (&nom[..i], &nom[i + 1..]),
        _ => (nom, ""),
    }
}

impl Lot {
    pub fn nouveau(dossier: String, noms: Vec<String>, dates: Vec<u64>) -> Self {
        Self { dossier, noms, dates, champs: [Edition::champ("{nom}"), Edition::champ(""), Edition::champ("")], focus: MOTIF, regex: false, defilement: 0 }
    }

    fn est_dossier(&self, nom: &str) -> bool {
        std::path::Path::new(&crate::dossier::joindre(&self.dossier, nom)).is_dir()
    }

    /// Nouveaux noms, ou l'erreur qui empêche de valider
    pub fn apercu(&self) -> Result<Vec<String>, String> {
        let motif = self.champs[MOTIF].valeur();
        let chercher = self.champs[CHERCHER].valeur();
        let remplacer = self.champs[REMPLACER].valeur();
        let re = if self.regex && !chercher.is_empty() {
            Some(Regex::new(&chercher).map_err(|_| "EXPRESSION REGULIERE INVALIDE".to_string())?)
        } else {
            None
        };
        let chiffres = self.noms.len().to_string().len();
        let mut v = Vec::with_capacity(self.noms.len());
        for (k, nom) in self.noms.iter().enumerate() {
            let (base, ext) = decouper(nom, self.est_dossier(nom));
            let n = k + 1;
            let date = self.dates.get(k).map_or(String::new(), |&d| crate::systeme::date_iso(d));
            let mut s = motif
                .replace("{nom}", base)
                .replace("{ext}", ext)
                .replace("{nnn}", &format!("{n:03}"))
                .replace("{nn}", &format!("{n:02}"))
                .replace("{n}", &format!("{n:0chiffres$}"))
                .replace("{date}", &date);
            if !chercher.is_empty() {
                s = match &re {
                    Some(re) => re.replace_all(&s, remplacer.as_str()).into_owned(),
                    None => s.replace(&chercher, &remplacer),
                };
            }
            // L'extension est gardée, sauf si le motif la place lui-même
            if !ext.is_empty() && !motif.contains("{ext}") {
                s = format!("{s}.{ext}");
            }
            v.push(s.trim().to_string());
        }
        // Vérifications : noms valides, distincts, sans écraser un autre élément du dossier
        let mut vus = std::collections::HashSet::new();
        let anciens: std::collections::HashSet<String> = self.noms.iter().map(|n| n.to_lowercase()).collect();
        for n in &v {
            if !crate::operations::nom_valide(n) {
                return Err(format!("NOM INVALIDE : « {n} »"));
            }
            if !vus.insert(n.to_lowercase()) {
                return Err(format!("DEUX ELEMENTS S'APPELLERAIENT « {n} »"));
            }
            if !anciens.contains(&n.to_lowercase()) && std::path::Path::new(&crate::dossier::joindre(&self.dossier, n)).exists() {
                return Err(format!("« {n} » EXISTE DEJA DANS LE DOSSIER"));
            }
        }
        Ok(v)
    }
}

/// Renomme en deux temps (noms provisoires d'abord) : `a→b` et `b→a` dans le même lot fonctionnent.
/// En cas d'échec, ce qui a été fait est défait.
pub fn renommer(dossier: &str, paires: &[(String, String)]) -> Result<(), String> {
    let chemin = |n: &str| crate::dossier::joindre(dossier, n);
    let paires: Vec<&(String, String)> = paires.iter().filter(|(a, b)| a != b).collect();
    let provisoire = |k: usize| format!(".nothing-lot-{}-{k}", std::process::id());
    let mut faits: Vec<usize> = Vec::new();
    for (k, (a, _)) in paires.iter().enumerate() {
        if let Err(e) = std::fs::rename(chemin(a), chemin(&provisoire(k))) {
            for &j in faits.iter().rev() {
                let _ = std::fs::rename(chemin(&provisoire(j)), chemin(&paires[j].0));
            }
            return Err(format!("« {a} » : {}", e.to_string().to_uppercase()));
        }
        faits.push(k);
    }
    let mut erreur = None;
    let mut suivis: Vec<(String, String)> = Vec::new();
    for (k, (a, b)) in paires.iter().enumerate() {
        if let Err(e) = std::fs::rename(chemin(&provisoire(k)), chemin(b)) {
            // On remet au moins l'ancien nom
            let _ = std::fs::rename(chemin(&provisoire(k)), chemin(a));
            erreur.get_or_insert(format!("« {b} » : {}", e.to_string().to_uppercase()));
        } else {
            suivis.push((chemin(a), chemin(b)));
        }
    }
    // Les étiquettes suivent les seuls éléments réellement renommés
    crate::etiquettes::suivre_paires(&suivis);
    erreur.map_or(Ok(()), Err)
}

/// Date de prise de vue d'une photo (EXIF, lue par Windows), sinon None
pub fn date_prise(chemin: &str) -> Option<u64> {
    use windows::core::PCWSTR;
    use windows::Win32::Storage::EnhancedStorage::PKEY_Photo_DateTaken;
    use windows::Win32::UI::Shell::{IShellItem2, SHCreateItemFromParsingName};
    let w: Vec<u16> = chemin.encode_utf16().chain(std::iter::once(0)).collect();
    unsafe {
        let item: IShellItem2 = SHCreateItemFromParsingName(PCWSTR(w.as_ptr()), None).ok()?;
        let ft = item.GetFileTime(&PKEY_Photo_DateTaken).ok()?;
        let t = (ft.dwHighDateTime as u64) << 32 | ft.dwLowDateTime as u64;
        (t > 0).then_some(t)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lot(noms: &[&str]) -> Lot {
        Lot::nouveau(r"C:\nexistepas-nothing".into(), noms.iter().map(|s| s.to_string()).collect(), vec![0; noms.len()])
    }

    #[test]
    fn motifs() {
        let mut l = lot(&["IMG_001.jpg", "IMG_002.jpg", "notes.txt"]);
        l.champs[MOTIF] = Edition::champ("vacances {nn}");
        assert_eq!(l.apercu().unwrap(), vec!["vacances 01.jpg", "vacances 02.jpg", "vacances 03.txt"]);
        l.champs[MOTIF] = Edition::champ("{nom}");
        l.champs[CHERCHER] = Edition::champ(r"IMG_(\d+)");
        l.champs[REMPLACER] = Edition::champ("photo-$1");
        l.regex = true;
        assert_eq!(l.apercu().unwrap(), vec!["photo-001.jpg", "photo-002.jpg", "notes.txt"]);
        l.champs[CHERCHER] = Edition::champ("(");
        assert!(l.apercu().is_err());
        l.regex = false;
        l.champs[CHERCHER] = Edition::champ("IMG_");
        l.champs[REMPLACER] = Edition::champ("");
        assert_eq!(l.apercu().unwrap(), vec!["001.jpg", "002.jpg", "notes.txt"]);
        // Deux fois le même nom : refusé
        l.champs[MOTIF] = Edition::champ("x");
        l.champs[CHERCHER] = Edition::champ("");
        assert!(l.apercu().is_err());
    }

    #[test]
    fn echange() {
        let d = std::env::temp_dir().join(format!("nothing-lot-test-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join("a.txt"), "A").unwrap();
        std::fs::write(d.join("b.txt"), "B").unwrap();
        let ds = d.to_string_lossy().into_owned();
        renommer(&ds, &[("a.txt".into(), "b.txt".into()), ("b.txt".into(), "a.txt".into())]).unwrap();
        assert_eq!(std::fs::read_to_string(d.join("a.txt")).unwrap(), "B");
        assert_eq!(std::fs::read_to_string(d.join("b.txt")).unwrap(), "A");
        std::fs::remove_dir_all(&d).unwrap();
    }
}
