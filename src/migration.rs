// Migration des données de l'ancien dossier `NothingExplorer` vers `SpaceFiles` (phase 1e, étape 1).
// Les chemins sont des paramètres : le moteur ne touche jamais au profil Windows tout seul, il se teste sur des
// dossiers temporaires. Règle d'or : l'ancien dossier n'est jamais modifié (copie, jamais déplacement), et le
// nouveau n'apparaît qu'une fois la copie finie (copie dans « .migration » puis renommage) : une coupure en milieu
// de copie ne laisse jamais un dossier à moitié rempli.

use std::path::{Path, PathBuf};
use std::time::Instant;

/// Nom du fichier laissé dans le nouveau dossier pour dire d'où viennent les données
pub const FICHIER_TRACE: &str = "migre-depuis-nothingexplorer.txt";

#[derive(Debug, PartialEq, Eq)]
pub enum Resultat {
    /// Le nouveau dossier existe déjà : rien n'a été touché
    DejaFait,
    /// Pas d'ancien dossier (installation neuve) : rien à copier
    RienAMigrer,
    /// Tout a été copié
    Complete { fichiers: u64, octets: u64 },
    /// Le nouveau dossier est créé mais certains fichiers n'ont pas pu être copiés (verrouillés, accès refusé)
    Partielle { fichiers: u64, octets: u64, echecs: Vec<String> },
    /// La copie ou le renommage final a échoué : pas de nouveau dossier, reprise au lancement suivant
    Echec(String),
}

impl Resultat {
    /// Une phrase pour le journal
    pub fn resume(&self) -> String {
        match self {
            Resultat::DejaFait => "deja fait (le nouveau dossier existe)".into(),
            Resultat::RienAMigrer => "rien a migrer (pas d'ancien dossier)".into(),
            Resultat::Complete { fichiers, octets } => format!("complete : {fichiers} fichiers, {octets} octets"),
            Resultat::Partielle { fichiers, octets, echecs } => {
                format!("partielle : {fichiers} fichiers, {octets} octets, {} en echec ({})", echecs.len(), echecs.join(" | "))
            }
            Resultat::Echec(e) => format!("echec : {e}"),
        }
    }

    pub fn est_grave(&self) -> bool {
        matches!(self, Resultat::Echec(_) | Resultat::Partielle { .. })
    }
}

/// Copie récursive de `src` vers `dst`. Un fichier ou dossier illisible est compté dans `echecs` sans arrêter la copie.
fn copier_arbre(src: &Path, dst: &Path, fichiers: &mut u64, octets: &mut u64, echecs: &mut Vec<String>) {
    if let Err(e) = std::fs::create_dir_all(dst) {
        echecs.push(format!("{} ({e})", dst.display()));
        return;
    }
    let entrees = match std::fs::read_dir(src) {
        Ok(r) => r,
        Err(e) => {
            echecs.push(format!("{} ({e})", src.display()));
            return;
        }
    };
    for entree in entrees {
        let entree = match entree {
            Ok(e) => e,
            Err(e) => {
                echecs.push(format!("{} ({e})", src.display()));
                continue;
            }
        };
        let (de, vers) = (entree.path(), dst.join(entree.file_name()));
        match entree.file_type() {
            Ok(t) if t.is_dir() => copier_arbre(&de, &vers, fichiers, octets, echecs),
            Ok(_) => match std::fs::copy(&de, &vers) {
                Ok(n) => {
                    *fichiers += 1;
                    *octets += n;
                }
                Err(e) => {
                    // Un fichier verrouillé laisse parfois un fichier vide à l'arrivée : on ne garde pas de moitié de fichier
                    let _ = std::fs::remove_file(&vers);
                    echecs.push(format!("{} ({e})", de.display()));
                }
            },
            Err(e) => echecs.push(format!("{} ({e})", de.display())),
        }
    }
}

/// Copie `ancien` vers `nouveau` si `nouveau` n'existe pas encore. N'écrit jamais dans `ancien`.
pub fn migrer(ancien: &Path, nouveau: &Path) -> Resultat {
    migrer_avec_date(ancien, nouveau, &crate::journal::horodatage())
}

fn migrer_avec_date(ancien: &Path, nouveau: &Path, date: &str) -> Resultat {
    if nouveau.exists() {
        return Resultat::DejaFait;
    }
    if !ancien.is_dir() {
        return Resultat::RienAMigrer;
    }
    let mut temporaire = nouveau.as_os_str().to_owned();
    temporaire.push(".migration");
    let temporaire = PathBuf::from(temporaire);
    // Un « .migration » trouvé ici vient d'une copie interrompue : on repart de zéro
    if temporaire.exists() {
        if let Err(e) = std::fs::remove_dir_all(&temporaire) {
            return Resultat::Echec(format!("ancien dossier temporaire non supprimable ({e})"));
        }
    }
    let (mut fichiers, mut octets, mut echecs) = (0u64, 0u64, Vec::new());
    copier_arbre(ancien, &temporaire, &mut fichiers, &mut octets, &mut echecs);
    if !temporaire.is_dir() {
        return Resultat::Echec(echecs.join(" | "));
    }
    let mut trace = format!(
        "Donnees copiees depuis l'ancien dossier le {date}\r\nAncien chemin : {}\r\nFichiers copies : {fichiers}\r\nOctets copies : {octets}\r\nFichiers en echec : {}\r\n",
        ancien.display(),
        echecs.len()
    );
    for e in &echecs {
        trace.push_str(&format!("  {e}\r\n"));
    }
    let _ = std::fs::write(temporaire.join(FICHIER_TRACE), trace);
    if let Err(e) = std::fs::rename(&temporaire, nouveau) {
        let _ = std::fs::remove_dir_all(&temporaire);
        return Resultat::Echec(format!("renommage impossible ({e})"));
    }
    if echecs.is_empty() {
        Resultat::Complete { fichiers, octets }
    } else {
        Resultat::Partielle { fichiers, octets, echecs }
    }
}

/// Appel de `main` : migre `%APPDATA%` puis `%LOCALAPPDATA%` (jamais le mode portable) et journalise chaque résultat
/// avec le temps passé. À appeler avant toute lecture de réglages.
pub fn migrer_profil() {
    if crate::reglages::dossier_portable().is_some() {
        return;
    }
    // Les deux migrations d'abord, le journal ensuite : le journal vit dans %LOCALAPPDATA%\SpaceFiles, et l'écrire
    // entre les deux créerait ce dossier avant la migration de %LOCALAPPDATA% (elle verrait « déjà fait »)
    let mut traces = Vec::new();
    for variable in ["APPDATA", "LOCALAPPDATA"] {
        let Some(base) = std::env::var_os(variable) else { continue };
        let base = Path::new(&base);
        let debut = Instant::now();
        let r = migrer(&base.join(crate::reglages::ANCIEN_NOM_DONNEES), &base.join(crate::reglages::NOM_DONNEES));
        // Rien d'écrit dans le journal quand il n'y avait rien à faire : sinon un dossier SpaceFiles serait créé pour rien
        if r == Resultat::DejaFait || r == Resultat::RienAMigrer {
            continue;
        }
        traces.push((variable, r, debut.elapsed().as_millis()));
    }
    for (variable, r, ms) in traces {
        let niveau = if r.est_grave() { crate::journal::Niveau::Erreur } else { crate::journal::Niveau::Info };
        crate::journal::ecrire(niveau, "migration", &format!("{variable} : {} en {ms} ms", r.resume()));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;
    use std::os::windows::fs::OpenOptionsExt;

    /// Dossier temporaire unique, supprimé à la fin du test par `Drop`
    struct Temp(PathBuf);
    impl Temp {
        fn new(nom: &str) -> Temp {
            let d = std::env::temp_dir().join(format!("ne-migration-{nom}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&d);
            std::fs::create_dir_all(&d).unwrap();
            Temp(d)
        }
    }
    impl Drop for Temp {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    /// Faux ancien dossier avec sous-dossiers, accents et espaces
    fn fabriquer(ancien: &Path) {
        std::fs::create_dir_all(ancien.join("sous dossier").join("é profond")).unwrap();
        std::fs::write(ancien.join("reglages.toml"), "fond_vivant = true\r\ntheme = \"nuit\"\r\n").unwrap();
        std::fs::write(ancien.join("operations.csv"), "\u{feff}DATE;ACTION;DETAIL\r\n").unwrap();
        std::fs::write(ancien.join("sous dossier").join("réglage été.txt"), "accentué").unwrap();
        std::fs::write(ancien.join("sous dossier").join("é profond").join("data.bin"), (0..=255u8).collect::<Vec<_>>()).unwrap();
        std::fs::write(ancien.join("vide.txt"), "").unwrap();
    }

    /// chemin relatif -> (octets, date de modification) de tout l'arbre
    fn empreinte(racine: &Path) -> BTreeMap<String, (u64, std::time::SystemTime)> {
        fn rec(d: &Path, racine: &Path, m: &mut BTreeMap<String, (u64, std::time::SystemTime)>) {
            for e in std::fs::read_dir(d).unwrap() {
                let p = e.unwrap().path();
                let meta = std::fs::metadata(&p).unwrap();
                let rel = p.strip_prefix(racine).unwrap().to_string_lossy().into_owned();
                if meta.is_dir() {
                    m.insert(rel, (0, meta.modified().unwrap()));
                    rec(&p, racine, m);
                } else {
                    m.insert(rel, (meta.len(), meta.modified().unwrap()));
                }
            }
        }
        let mut m = BTreeMap::new();
        rec(racine, racine, &mut m);
        m
    }

    #[test]
    fn copie_complete_identique_et_ancien_intact() {
        let t = Temp::new("complete");
        let (ancien, nouveau) = (t.0.join("NothingExplorer"), t.0.join("SpaceFiles"));
        fabriquer(&ancien);
        let avant = empreinte(&ancien);
        let r = migrer_avec_date(&ancien, &nouveau, "2026-11-01 10:00:00");
        assert_eq!(r, Resultat::Complete { fichiers: 5, octets: 36 + 23 + 9 + 256 });
        // Octet pour octet
        for (rel, (taille, _)) in &avant {
            let (a, n) = (ancien.join(rel), nouveau.join(rel));
            if a.is_file() {
                assert_eq!(std::fs::read(&a).unwrap(), std::fs::read(&n).unwrap(), "{rel}");
                assert_eq!(*taille, std::fs::metadata(&n).unwrap().len());
            } else {
                assert!(n.is_dir(), "{rel}");
            }
        }
        assert!(nouveau.join(FICHIER_TRACE).is_file());
        let trace = std::fs::read_to_string(nouveau.join(FICHIER_TRACE)).unwrap();
        assert!(trace.contains("2026-11-01 10:00:00") && trace.contains("Fichiers copies : 5"), "{trace}");
        // Ancien : mêmes fichiers, tailles et dates
        assert_eq!(avant, empreinte(&ancien));
        // Pas de dossier temporaire oublié
        assert!(!t.0.join("SpaceFiles.migration").exists());
    }

    #[test]
    fn ne_jamais_ecraser_un_nouveau_dossier() {
        let t = Temp::new("ecrase");
        let (ancien, nouveau) = (t.0.join("NothingExplorer"), t.0.join("SpaceFiles"));
        fabriquer(&ancien);
        std::fs::create_dir_all(&nouveau).unwrap();
        std::fs::write(nouveau.join("reglages.toml"), "neuf").unwrap();
        assert_eq!(migrer(&ancien, &nouveau), Resultat::DejaFait);
        assert_eq!(std::fs::read_to_string(nouveau.join("reglages.toml")).unwrap(), "neuf");
        assert!(!nouveau.join("operations.csv").exists());
    }

    #[test]
    fn ancien_absent_rien_a_faire() {
        let t = Temp::new("absent");
        let nouveau = t.0.join("SpaceFiles");
        assert_eq!(migrer(&t.0.join("NothingExplorer"), &nouveau), Resultat::RienAMigrer);
        assert!(!nouveau.exists() && !t.0.join("SpaceFiles.migration").exists());
    }

    #[test]
    fn idempotence() {
        let t = Temp::new("idem");
        let (ancien, nouveau) = (t.0.join("NothingExplorer"), t.0.join("SpaceFiles"));
        fabriquer(&ancien);
        assert!(matches!(migrer(&ancien, &nouveau), Resultat::Complete { .. }));
        // Un fichier ajouté après coup dans le nouveau dossier survit au deuxième appel
        std::fs::write(nouveau.join("ajout.txt"), "x").unwrap();
        assert_eq!(migrer(&ancien, &nouveau), Resultat::DejaFait);
        assert!(nouveau.join("ajout.txt").is_file());
    }

    #[test]
    fn reprise_apres_migration_interrompue() {
        let t = Temp::new("reprise");
        let (ancien, nouveau) = (t.0.join("NothingExplorer"), t.0.join("SpaceFiles"));
        fabriquer(&ancien);
        // Reste d'une copie coupée : un fichier à moitié écrit et un fichier qui n'existe pas dans l'ancien
        let reste = t.0.join("SpaceFiles.migration");
        std::fs::create_dir_all(&reste).unwrap();
        std::fs::write(reste.join("reglages.toml"), "fond_v").unwrap();
        std::fs::write(reste.join("intrus.txt"), "x").unwrap();
        assert!(matches!(migrer(&ancien, &nouveau), Resultat::Complete { .. }));
        assert_eq!(
            std::fs::read(ancien.join("reglages.toml")).unwrap(),
            std::fs::read(nouveau.join("reglages.toml")).unwrap()
        );
        assert!(!nouveau.join("intrus.txt").exists());
        assert!(!reste.exists());
    }

    #[test]
    fn fichier_verrouille_copie_partielle() {
        let t = Temp::new("verrou");
        let (ancien, nouveau) = (t.0.join("NothingExplorer"), t.0.join("SpaceFiles"));
        fabriquer(&ancien);
        // Handle ouvert sans aucun partage : toute autre ouverture échoue (violation de partage)
        let _verrou = std::fs::OpenOptions::new().read(true).write(true).share_mode(0).open(ancien.join("operations.csv")).unwrap();
        let r = migrer(&ancien, &nouveau);
        match r {
            Resultat::Partielle { fichiers, echecs, .. } => {
                assert_eq!(fichiers, 4);
                assert_eq!(echecs.len(), 1);
                assert!(echecs[0].contains("operations.csv"), "{echecs:?}");
            }
            autre => panic!("attendu Partielle, obtenu {autre:?}"),
        }
        assert!(nouveau.join("reglages.toml").is_file());
        assert!(!nouveau.join("operations.csv").exists());
        let trace = std::fs::read_to_string(nouveau.join(FICHIER_TRACE)).unwrap();
        assert!(trace.contains("Fichiers en echec : 1") && trace.contains("operations.csv"), "{trace}");
        // L'ancien fichier verrouillé est toujours là
        assert!(ancien.join("operations.csv").is_file());
    }

    #[test]
    fn noms_avec_accents_et_espaces() {
        let t = Temp::new("accents espaces");
        let (ancien, nouveau) = (t.0.join("Ancien Données é"), t.0.join("Nouveau Données é"));
        fabriquer(&ancien);
        assert!(matches!(migrer(&ancien, &nouveau), Resultat::Complete { .. }));
        assert_eq!(
            std::fs::read_to_string(nouveau.join("sous dossier").join("réglage été.txt")).unwrap(),
            "accentué"
        );
        assert!(nouveau.join("sous dossier").join("é profond").join("data.bin").is_file());
    }
}
