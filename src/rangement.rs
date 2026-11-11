// Règles de rangement (Z6) : « un PDF qui arrive dans Téléchargements va dans Documents\Factures ».
// Une règle s'écrit « pdf,docx|D:\Documents\Factures » dans reglages.toml. Ce module choisit seulement quoi ranger
// où ; le déplacement passe par le moteur de Windows (App::ranger), donc les conflits sont demandés et Ctrl+Z le défait.

/// Une règle : les extensions (minuscules, sans point) et le dossier d'arrivée
#[derive(Debug, PartialEq, Eq)]
pub struct Regle {
    pub types: Vec<String>,
    pub dossier: String,
}

/// Fichiers que les navigateurs écrivent encore : on n'y touche pas
const EN_COURS: [&str; 6] = ["crdownload", "part", "tmp", "download", "opdownload", "partial"];

/// Un fichier modifié depuis moins de 15 s est peut-être encore en cours d'écriture (centaines de nanosecondes)
const RECENT: u64 = 150_000_000;

impl Regle {
    /// « pdf docx > D:\Factures » (saisie) ou « pdf,docx|D:\Factures » (réglages). None : texte inutilisable.
    pub fn analyser(texte: &str) -> Option<Regle> {
        let (types, dossier) = texte.split_once('>').or_else(|| texte.split_once('|'))?;
        let types: Vec<String> = types
            .split(|c: char| c == ',' || c == ';' || c.is_whitespace())
            .map(|t| t.trim_start_matches("*.").trim_start_matches('.').to_lowercase())
            .filter(|t| !t.is_empty())
            .collect();
        let dossier = dossier.trim().trim_matches('"').trim_end_matches('\\').to_string();
        // Un chemin complet seulement (lecteur ou partage) : « Factures » tout court viserait on ne sait quel dossier
        let complet = dossier.len() > 3 && (dossier.as_bytes()[1] == b':' && dossier.as_bytes()[2] == b'\\' || dossier.starts_with("\\\\"));
        let propre = |t: &String| !t.contains(['\\', '/', ':', '*', '?', '"', '<', '>', '|']);
        (complet && !types.is_empty() && types.iter().all(propre) && !types.iter().any(|t| EN_COURS.contains(&t.as_str()))).then_some(Regle { types, dossier })
    }

    /// Forme enregistrée dans les réglages
    pub fn ecrire(&self) -> String {
        format!("{}|{}", self.types.join(","), self.dossier)
    }

    /// « PDF, DOCX → D:\Factures »
    pub fn libelle(&self) -> String {
        format!("{} → {}", self.types.join(", ").to_uppercase(), self.dossier)
    }
}

/// Fichiers de `dossier` que les règles envoient ailleurs : (nom, dossier d'arrivée). La première règle qui convient gagne.
/// `fichiers` : nom et date de modification ; `maintenant` en FILETIME.
pub fn choisir(regles: &[String], fichiers: &[(String, u64)], maintenant: u64, dossier: &str) -> Vec<(String, String)> {
    let regles: Vec<Regle> = regles.iter().filter_map(|r| Regle::analyser(r)).collect();
    let ici = dossier.trim_end_matches('\\');
    fichiers
        .iter()
        .filter(|(_, modif)| maintenant.saturating_sub(*modif) >= RECENT)
        .filter_map(|(nom, _)| {
            let ext = nom.rsplit_once('.')?.1.to_lowercase();
            let r = regles.iter().find(|r| r.types.contains(&ext))?;
            (!r.dossier.eq_ignore_ascii_case(ici)).then(|| (nom.clone(), r.dossier.clone()))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn analyse_des_regles() {
        let r = Regle::analyser("PDF .docx, *.xls > D:\\Documents\\Factures\\").unwrap();
        assert_eq!(r.types, ["pdf", "docx", "xls"]);
        assert_eq!(r.dossier, "D:\\Documents\\Factures");
        assert_eq!(Regle::analyser(&r.ecrire()), Some(r));
        // Pas de chemin complet, pas de type, extension de téléchargement en cours : refusés
        assert!(Regle::analyser("pdf > Factures").is_none());
        assert!(Regle::analyser("> D:\\Factures").is_none());
        assert!(Regle::analyser("crdownload > D:\\Factures").is_none());
        assert!(Regle::analyser("p/f > D:\\Factures").is_none());
        assert!(Regle::analyser("zip > \\\\nas\\partage\\archives").is_some());
    }

    #[test]
    fn choix_des_fichiers() {
        let regles = vec!["pdf|D:\\Factures".to_string(), "pdf,zip|D:\\Autre".to_string(), "mauvaise".to_string()];
        let (vieux, maintenant) = (1_000_000_000, 5_000_000_000);
        let fichiers = vec![
            ("a.PDF".to_string(), vieux),
            ("b.zip".to_string(), vieux),
            ("c.txt".to_string(), vieux),
            ("sans-extension".to_string(), vieux),
            // Écrit à l'instant : peut-être en cours de téléchargement
            ("d.pdf".to_string(), maintenant - 1),
        ];
        let v = choisir(&regles, &fichiers, maintenant, "C:\\Users\\x\\Downloads");
        // La première règle gagne pour le PDF ; le ZIP va à la seconde
        assert_eq!(v, [("a.PDF".to_string(), "D:\\Factures".to_string()), ("b.zip".to_string(), "D:\\Autre".to_string())]);
        // Un fichier déjà dans son dossier d'arrivée ne bouge pas
        assert!(choisir(&regles, &fichiers[..1], maintenant, "d:\\factures\\").is_empty());
    }
}
