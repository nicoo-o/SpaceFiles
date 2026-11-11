// Banc de tests de la matrice des opérations destructives (PROJECT_REBOOT_MASTERPLAN.md, Part 9) : exerce
// `executer` d'operations.rs (le moteur IFileOperation) sur des dossiers temporaires, sans fenêtre (`hwnd` = 0).
//
// Règles du banc :
// - tout se passe sous `%TEMP%\spacefiles-matrice-<pid>-<nom>` ; `garde` panique AVANT d'appeler Windows si un
//   chemin sort de ce dossier (ou est vide, un lecteur, un dossier de l'utilisateur) ;
// - les tests « corbeille » mettent de vrais fichiers dans la vraie corbeille ; le banc retire ce qu'il y a mis
//   (le `$R…` et son `$I…`) même si le test échoue, et vérifie que nombre et taille de la corbeille sont ceux d'avant.
//   Il ne vide JAMAIS la corbeille (c'est celle de l'auteur) ;
// - un verrou statique sérialise les tests qui touchent COM ou la corbeille (cargo test les lance en parallèle).
//
// ┌──────────────────────────────────┬────────────────────────────────────────────────────────────────────────────┐
// │ Ligne de la matrice              │ Couverture                                                                 │
// ├──────────────────────────────────┼────────────────────────────────────────────────────────────────────────────┤
// │ Suppression → corbeille          │ corbeille_*                                                                │
// │ Suppression définitive           │ suppression_definitive_*                                                   │
// │ Écraser / garder les deux        │ ecraser_*, garder_les_deux_*                                               │
// │ Annuler (Ctrl+Z)                 │ annuler_* : le geste inverse, pas App::defaire (qui exige une fenêtre)     │
// │ Annulation à 0 / 50 / 99 %       │ annulation_a_0_pour_cent, annulation_en_cours_de_copie_*                   │
// │ Déplacement entre volumes        │ #[ignore] deplacement_entre_volumes_interrompu (écrire hors de %TEMP% est  │
// │                                  │ interdit au banc : procédure manuelle dans le test)                        │
// │ Coupure de partage réseau        │ #[ignore] coupure_de_partage_reseau (partage réel exigé)                   │
// │ Disque cible plein               │ NON COUVERTE : demande un disque virtuel (VHD) donc des droits             │
// │                                  │ d'administrateur ; procédure dans docs/audit/matrice-destructive-…md       │
// │ Verrouillé, lecture seule,       │ fichier_verrouille_*, lecture_seule_*, noms_unicode_*, chemin_long_*       │
// │ Unicode, chemin > 260            │                                                                            │
// └──────────────────────────────────┴────────────────────────────────────────────────────────────────────────────┘

use super::*;
use crate::historique;
use std::os::windows::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};

static VERROU: Mutex<()> = Mutex::new(());

/// Verrou entre PROCESSUS, en plus de `VERROU` (qui ne voit que les fils de ce processus). Ces tests comptent et vident la VRAIE
/// corbeille de l'auteur : deux `cargo test` en même temps (le contrôle de fin de tour en lance un pendant qu'une session en lance
/// un autre) se marchent dessus (« la corbeille n'est pas revenue à son état d'avant », « élément introuvable dans la corbeille »).
/// Observé le 09/11 : 6 suites sur 12 en échec quand deux boucles de `cargo test` tournaient ensemble, 0 sur 10 une par une.
/// Un mutex nommé fait attendre le second processus. Pris et rendu par le même fil (celui du test), car un mutex Windows est
/// attaché à son fil.
struct VerrouProcessus(windows::Win32::Foundation::HANDLE);

impl VerrouProcessus {
    fn prendre() -> VerrouProcessus {
        use windows::Win32::System::Threading::{CreateMutexW, WaitForSingleObject, INFINITE};
        // SAFETY : le nom est une constante terminée par un nul ; le handle rendu est fermé par `Drop`, sur le même fil.
        unsafe {
            let h = CreateMutexW(None, false, windows::core::w!("Local\\SpaceFilesTestsCorbeille")).expect("mutex nommé des tests de corbeille");
            // WAIT_ABANDONED (un autre processus de test est mort en le tenant) est acceptable : on le possède alors
            WaitForSingleObject(h, INFINITE);
            VerrouProcessus(h)
        }
    }
}

impl Drop for VerrouProcessus {
    fn drop(&mut self) {
        use windows::Win32::Foundation::CloseHandle;
        use windows::Win32::System::Threading::ReleaseMutex;
        // SAFETY : `self.0` est le mutex pris par ce fil dans `prendre`, libéré ici une seule fois.
        unsafe {
            let _ = ReleaseMutex(self.0);
            let _ = CloseHandle(self.0);
        }
    }
}

// ───────────────────────── Garde de sécurité ─────────────────────────

fn racine_du_banc(nom: &str) -> PathBuf {
    std::env::temp_dir().join(format!("spacefiles-matrice-{}-{nom}", std::process::id()))
}

/// Forme comparable d'un chemin : sans préfixe `\\?\`, barres arrière, minuscules, sans barre finale
fn normaliser(s: &str) -> String {
    s.trim_start_matches(r"\\?\").replace('/', "\\").trim_end_matches('\\').to_lowercase()
}

/// Panique (avant tout appel à Windows) si `chemin` n'est pas strictement dans `racine`, ou si `racine` n'est pas un
/// dossier du banc sous `%TEMP%`. Un bug de test ne doit jamais pouvoir toucher les fichiers de l'auteur.
fn garde(racine: &Path, chemin: &str) {
    let r = normaliser(&racine.to_string_lossy());
    let temp = normaliser(&std::env::temp_dir().to_string_lossy());
    assert!(r.starts_with(&format!("{temp}\\spacefiles-matrice-")), "GARDE : la racine {r} n'est pas un dossier du banc sous %TEMP%");
    let c = normaliser(chemin);
    assert!(!c.is_empty(), "GARDE : chemin vide");
    assert!(!c.split('\\').any(|s| s == "." || s == ".."), "GARDE : remontée de dossier dans {chemin}");
    // Un lecteur (« c: ») ou un dossier de l'utilisateur ne passe jamais, même par erreur de racine
    assert!(c.len() > 2 && !c.ends_with(':'), "GARDE : {chemin} est un lecteur");
    for var in ["USERPROFILE", "APPDATA", "LOCALAPPDATA", "TEMP", "SystemRoot", "ProgramFiles"] {
        if let Some(v) = std::env::var_os(var) {
            assert!(c != normaliser(&v.to_string_lossy()), "GARDE : {chemin} est un dossier protégé ({var})");
        }
    }
    assert!(c.starts_with(&format!("{r}\\")), "GARDE : {chemin} est hors de {r}");
}

// ───────────────────────── Aides ─────────────────────────

/// `n` octets déterministes (congruentiel linéaire), différents pour deux graines différentes
fn octets(n: usize, graine: u64) -> Vec<u8> {
    let mut x = graine.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
    (0..n)
        .map(|_| {
            x = x.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            (x >> 33) as u8
        })
        .collect()
}

/// Empreinte FNV-1a 64 bits d'un fichier, lu par blocs
fn empreinte(p: impl AsRef<Path>) -> u64 {
    use std::io::Read;
    let mut f = std::fs::File::open(p.as_ref()).unwrap_or_else(|e| panic!("lecture de {:?} : {e}", p.as_ref()));
    let mut h: u64 = 0xcbf29ce484222325;
    let mut buf = vec![0u8; 1 << 20];
    loop {
        let n = f.read(&mut buf).unwrap();
        if n == 0 {
            return h;
        }
        for b in &buf[..n] {
            h = (h ^ *b as u64).wrapping_mul(0x100000001b3);
        }
    }
}

/// (nombre d'éléments, octets) de la corbeille de tous les disques, lus sans le cache de corbeille.rs
fn mesure_corbeille() -> (i64, i64) {
    let mut info = SHQUERYRBINFO { cbSize: std::mem::size_of::<SHQUERYRBINFO>() as u32, ..Default::default() };
    unsafe { SHQueryRecycleBinW(PCWSTR::null(), &mut info) }.expect("SHQueryRecycleBinW");
    (info.i64NumItems, info.i64Size)
}

/// Retire un `$R…` de la corbeille (fichier ou dossier) et sa fiche `$I…`. Ne touche qu'à un chemin de corbeille.
fn retirer_de_la_corbeille(r: &str) {
    let (dossier, nom) = r.rsplit_once('\\').expect("chemin de corbeille");
    assert!(dossier.to_lowercase().contains("$recycle.bin") && nom.starts_with("$R"), "pas un élément de corbeille : {r}");
    supprimer_tout(Path::new(r));
    supprimer_tout(Path::new(&format!("{dossier}\\$I{}", &nom[2..])));
}

/// Filet de sécurité : retire de la corbeille du lecteur de %TEMP% tout élément dont l'origine est un dossier du banc
/// (`%TEMP%\spacefiles-matrice-…`), même si le bilan d'une opération ne l'a pas annoncé (voir le chemin > 260). Ne touche
/// jamais à un élément d'une autre origine.
fn balayer_corbeille() {
    let temp = normaliser(&std::env::temp_dir().to_string_lossy());
    let Some(lecteur) = temp.get(..2) else { return };
    let Ok(sids) = std::fs::read_dir(format!("{lecteur}\\$Recycle.Bin")) else { return };
    for sid in sids.flatten() {
        let Ok(fichiers) = std::fs::read_dir(sid.path()) else { continue };
        for f in fichiers.flatten() {
            let nom = f.file_name().to_string_lossy().into_owned();
            let Some(suite) = nom.strip_prefix("$I") else { continue };
            let Ok(contenu) = std::fs::read(f.path()) else { continue };
            let Some((_, _, origine)) = crate::corbeille::analyser_i(&contenu) else { continue };
            if normaliser(&origine).starts_with(&format!("{temp}\\spacefiles-matrice-")) {
                retirer_de_la_corbeille(&f.path().with_file_name(format!("$R{suite}")).to_string_lossy());
            }
        }
    }
}

/// `$R…` de la corbeille du lecteur de %TEMP% dont l'origine est exactement `origine` (quand le bilan ne l'a pas donné)
fn chercher_dans_corbeille(origine: &str) -> Option<String> {
    let temp = normaliser(&std::env::temp_dir().to_string_lossy());
    let cible = normaliser(origine);
    for sid in std::fs::read_dir(format!("{}\\$Recycle.Bin", temp.get(..2)?)).ok()?.flatten() {
        for f in std::fs::read_dir(sid.path()).ok()?.flatten() {
            let nom = f.file_name().to_string_lossy().into_owned();
            let Some(suite) = nom.strip_prefix("$I") else { continue };
            let Some((_, _, o)) = std::fs::read(f.path()).ok().and_then(|c| crate::corbeille::analyser_i(&c)) else { continue };
            if normaliser(&o) == cible {
                return Some(f.path().with_file_name(format!("$R{suite}")).to_string_lossy().into_owned());
            }
        }
    }
    None
}

/// Suppression récursive qui lève la lecture seule au passage
fn supprimer_tout(p: &Path) {
    let Ok(m) = std::fs::symlink_metadata(p) else { return };
    let mut perm = m.permissions();
    if perm.readonly() {
        #[allow(clippy::permissions_set_readonly_false)]
        perm.set_readonly(false);
        let _ = std::fs::set_permissions(p, perm);
    }
    if m.is_dir() {
        if let Ok(d) = std::fs::read_dir(p) {
            for e in d.flatten() {
                supprimer_tout(&e.path());
            }
        }
        let _ = std::fs::remove_dir(p);
    } else {
        let _ = std::fs::remove_file(p);
    }
}

/// Dossier de travail + COM + verrou + suivi de ce qui a été mis dans la corbeille. Le `Drop` nettoie même en cas d'échec.
struct Banc {
    racine: PathBuf,
    /// `$R…` posés dans la corbeille par ce test
    corbeille: Vec<String>,
    avant: (i64, i64),
    com: bool,
    _verrou: std::sync::MutexGuard<'static, ()>,
    /// Déclaré après `_verrou` : rendu en dernier à la destruction, une fois la corbeille nettoyée par `Drop`
    _verrou_processus: VerrouProcessus,
}

impl Banc {
    fn nouveau(nom: &str) -> Banc {
        let verrou = VERROU.lock().unwrap_or_else(|e| e.into_inner());
        let verrou_processus = VerrouProcessus::prendre();
        let racine = racine_du_banc(nom);
        garde(&racine, &format!("{}\\x", racine.display()));
        supprimer_tout(&racine);
        std::fs::create_dir_all(&racine).unwrap();
        let com = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE) }.is_ok();
        Banc { racine, corbeille: Vec::new(), avant: mesure_corbeille(), com, _verrou: verrou, _verrou_processus: verrou_processus }
    }

    fn p(&self, relatif: &str) -> String {
        format!("{}\\{}", self.racine.display(), relatif.replace('/', "\\"))
    }

    fn ecrire(&self, relatif: &str, contenu: &[u8]) -> String {
        let chemin = self.p(relatif);
        if let Some((d, _)) = chemin.rsplit_once('\\') {
            std::fs::create_dir_all(d).unwrap();
        }
        std::fs::write(&chemin, contenu).unwrap();
        chemin
    }

    fn dossier(&self, relatif: &str) -> String {
        let chemin = self.p(relatif);
        std::fs::create_dir_all(&chemin).unwrap();
        chemin
    }

    /// Exécute une opération comme `lancer` (mêmes appels à `executer`, même bilan), mais dans le fil du test
    fn lancer(&mut self, genre: Genre, elements: &[Element]) -> Bilan {
        self.lancer_avec(genre, elements, Controle::default())
    }

    fn lancer_avec(&mut self, genre: Genre, elements: &[Element], controle: Controle) -> Bilan {
        for e in elements {
            garde(&self.racine, &e.source);
            if let Some(d) = &e.dest {
                garde(&self.racine, d);
            }
            if let Some(r) = &e.remplace {
                garde(&self.racine, r);
            }
        }
        COUPURE.with(|c| c.set(false));
        let resultats: Notes = Arc::new(Mutex::new(Vec::new()));
        let mis_de_cote = Arc::new(Mutex::new(Vec::new()));
        // SAFETY: fil du test en appartement monofil (CoInitializeEx dans `nouveau`), hwnd 0 = pas de fenêtre propriétaire
        let r = unsafe { executer(0, 1, genre, elements, controle, resultats.clone(), mis_de_cote.clone(), false) };
        let resultats = std::mem::take(&mut *resultats.lock().unwrap());
        let remplaces = std::mem::take(&mut *mis_de_cote.lock().unwrap());
        for n in resultats.iter().filter_map(|(_, n)| n.as_ref()).filter(|_| genre == Genre::Corbeille) {
            self.corbeille.push(n.clone());
        }
        for (_, n) in &remplaces {
            self.corbeille.push(n.clone());
        }
        Bilan { numero: 1, genre, nombre: elements.len(), annule: matches!(r, Ok(true)), erreur: r.err().map(|e| message_com(&e)), resultats, remplaces }
    }
}

impl Drop for Banc {
    fn drop(&mut self) {
        for r in &self.corbeille {
            retirer_de_la_corbeille(r);
        }
        balayer_corbeille();
        supprimer_tout(&self.racine);
        if self.com {
            unsafe { CoUninitialize() };
        }
        // La corbeille est à l'état d'avant (sauf si l'auteur l'a utilisée pendant ce test : la mesure le dirait)
        let apres = mesure_corbeille();
        if !std::thread::panicking() {
            assert_eq!(apres, self.avant, "la corbeille de l'auteur n'est pas revenue à son état d'avant le test");
        } else if apres != self.avant {
            eprintln!("ATTENTION : corbeille {:?} -> {:?} après un test en échec", self.avant, apres);
        }
    }
}

fn el(source: &str, dest: Option<&str>) -> Element {
    Element::simple(source.to_string(), dest.map(str::to_string))
}

// ───────────────────────── Garde de sécurité (testée elle-même) ─────────────────────────

#[test]
#[should_panic(expected = "GARDE")]
fn garde_refuse_un_chemin_hors_du_dossier_temporaire() {
    garde(&racine_du_banc("garde"), r"C:\Windows\notepad.exe");
}

#[test]
#[should_panic(expected = "GARDE")]
fn garde_refuse_un_chemin_vide() {
    garde(&racine_du_banc("garde"), "");
}

#[test]
#[should_panic(expected = "GARDE")]
fn garde_refuse_un_lecteur() {
    garde(&racine_du_banc("garde"), r"C:\");
}

#[test]
#[should_panic(expected = "GARDE")]
fn garde_refuse_la_remontee_de_dossier() {
    let r = racine_du_banc("garde");
    garde(&r, &format!(r"{}\..\..\autre.txt", r.display()));
}

#[test]
#[should_panic(expected = "GARDE")]
fn garde_refuse_le_dossier_de_l_utilisateur() {
    garde(&racine_du_banc("garde"), &std::env::var("USERPROFILE").expect("USERPROFILE"));
}

#[test]
#[should_panic(expected = "GARDE")]
fn garde_refuse_une_racine_qui_n_est_pas_un_dossier_du_banc() {
    // Une racine mal calculée (ici le dossier temporaire lui-même) ne doit pas élargir la zone permise
    let temp = std::env::temp_dir();
    garde(&temp, &format!(r"{}\fichier.txt", temp.display()));
}

#[test]
fn garde_accepte_un_chemin_du_banc() {
    let r = racine_du_banc("garde");
    garde(&r, &format!(r"{}\sous\a.txt", r.display()));
    garde(&r, &format!(r"\\?\{}\sous\a.txt", r.display()));
}

#[test]
#[should_panic(expected = "GARDE")]
fn garde_bloque_executer_avant_tout_appel_a_windows() {
    // La source n'existe pas : si la garde manquait, Windows répondrait « introuvable » (erreur, pas panique)
    let mut b = Banc::nouveau("garde-executer");
    b.lancer(Genre::SupprimerDefinitivement, &[el(r"C:\spacefiles-garde-inexistant\x.txt", None)]);
}

// ───────────────────────── Suppression → corbeille ─────────────────────────

#[test]
fn corbeille_fichier_recuperable() {
    let mut b = Banc::nouveau("corbeille-fichier");
    let contenu = octets(5000, 1);
    let f = b.ecrire("a.txt", &contenu);
    let avant = mesure_corbeille();
    let bilan = b.lancer(Genre::Corbeille, &[el(&f, None)]);
    assert!(bilan.erreur.is_none() && !bilan.annule, "{:?}", bilan.erreur);
    assert!(!Path::new(&f).exists(), "le fichier devait quitter son dossier");
    let (origine, dans_corbeille) = bilan.resultats[0].clone();
    assert_eq!(origine, f);
    let r = dans_corbeille.expect("le bilan doit donner l'élément de la corbeille");
    assert_eq!(std::fs::read(&r).unwrap(), contenu, "le contenu est dans la corbeille");
    assert_eq!(mesure_corbeille().0, avant.0 + 1, "un élément de plus dans la corbeille");
    // Récupérable : le geste de Ctrl+Z
    assert_eq!(historique::restaurer(&[(f.clone(), r)]), Ok(1));
    assert_eq!(std::fs::read(&f).unwrap(), contenu);
}

#[test]
fn corbeille_dossier_recuperable() {
    let mut b = Banc::nouveau("corbeille-dossier");
    b.ecrire("d/un.txt", &octets(300, 2));
    b.ecrire("d/sous/deux.bin", &octets(70_000, 3));
    let d = b.p("d");
    let bilan = b.lancer(Genre::Corbeille, &[el(&d, None)]);
    assert!(bilan.erreur.is_none(), "{:?}", bilan.erreur);
    assert!(!Path::new(&d).exists());
    let r = bilan.resultats[0].1.clone().expect("élément de corbeille");
    assert_eq!(historique::restaurer(&[(d.clone(), r)]), Ok(1));
    assert_eq!(std::fs::read(b.p("d/un.txt")).unwrap(), octets(300, 2));
    assert_eq!(std::fs::read(b.p("d/sous/deux.bin")).unwrap(), octets(70_000, 3));
}

// ───────────────────────── Suppression définitive ─────────────────────────

#[test]
fn suppression_definitive_fichier_et_dossier() {
    let mut b = Banc::nouveau("definitive");
    let f = b.ecrire("a.txt", &octets(1000, 4));
    b.ecrire("d/x.txt", &octets(10, 5));
    let d = b.p("d");
    let avant = mesure_corbeille();
    let bilan = b.lancer(Genre::SupprimerDefinitivement, &[el(&f, None), el(&d, None)]);
    assert!(bilan.erreur.is_none() && !bilan.annule, "{:?}", bilan.erreur);
    assert!(!Path::new(&f).exists() && !Path::new(&d).exists());
    assert_eq!(mesure_corbeille(), avant, "rien dans la corbeille");
    assert!(bilan.resultats.iter().all(|(_, n)| n.is_none()), "pas d'élément de corbeille à annoncer : {:?}", bilan.resultats);
}

// ───────────────────────── Écraser / garder les deux ─────────────────────────

#[test]
fn ecraser_une_copie_met_l_ancien_a_la_corbeille() {
    let mut b = Banc::nouveau("ecraser-copie");
    let nouveau = b.ecrire("src/a.txt", &octets(4000, 6));
    let ancien = b.ecrire("dst/a.txt", &octets(2500, 7));
    let dst = b.p("dst");
    let e = Element { source: nouveau.clone(), dest: Some(dst), nom: None, remplace: Some(ancien.clone()) };
    let bilan = b.lancer(Genre::Copier, &[e]);
    assert!(bilan.erreur.is_none(), "{:?}", bilan.erreur);
    assert_eq!(std::fs::read(&ancien).unwrap(), octets(4000, 6), "le nouveau remplace l'ancien");
    assert_eq!(std::fs::read(&nouveau).unwrap(), octets(4000, 6), "la source d'une copie reste");
    assert_eq!(bilan.remplaces.len(), 1, "{:?}", bilan.remplaces);
    let (chemin, dans_corbeille) = bilan.remplaces[0].clone();
    assert_eq!(chemin, ancien);
    assert_eq!(std::fs::read(&dans_corbeille).unwrap(), octets(2500, 7), "l'ancien est récupérable dans la corbeille");
}

#[test]
fn ecraser_un_deplacement_met_l_ancien_a_la_corbeille() {
    let mut b = Banc::nouveau("ecraser-deplacement");
    let nouveau = b.ecrire("src/a.txt", &octets(4000, 8));
    let ancien = b.ecrire("dst/a.txt", &octets(2500, 9));
    let dst = b.p("dst");
    let e = Element { source: nouveau.clone(), dest: Some(dst), nom: None, remplace: Some(ancien.clone()) };
    let bilan = b.lancer(Genre::Deplacer, &[e]);
    assert!(bilan.erreur.is_none(), "{:?}", bilan.erreur);
    assert_eq!(std::fs::read(&ancien).unwrap(), octets(4000, 8));
    assert!(!Path::new(&nouveau).exists(), "la source d'un déplacement part");
    assert_eq!(bilan.remplaces.len(), 1, "{:?}", bilan.remplaces);
    assert_eq!(std::fs::read(&bilan.remplaces[0].1).unwrap(), octets(2500, 9));
}

#[test]
fn garder_les_deux_ne_perd_rien() {
    let mut b = Banc::nouveau("garder-deux");
    let nouveau = b.ecrire("src/a.txt", &octets(1234, 10));
    let ancien = b.ecrire("dst/a.txt", &octets(999, 11));
    let dst = b.p("dst");
    let avant = mesure_corbeille();
    let e = Element { source: nouveau.clone(), dest: Some(dst), nom: Some("a (2).txt".into()), remplace: None };
    let bilan = b.lancer(Genre::Copier, &[e]);
    assert!(bilan.erreur.is_none(), "{:?}", bilan.erreur);
    assert_eq!(std::fs::read(&ancien).unwrap(), octets(999, 11), "l'ancien est intact");
    assert_eq!(std::fs::read(b.p("dst/a (2).txt")).unwrap(), octets(1234, 10), "le nouveau a son nom");
    assert_eq!(std::fs::read(&nouveau).unwrap(), octets(1234, 10));
    assert!(bilan.remplaces.is_empty());
    assert_eq!(mesure_corbeille(), avant, "rien n'est parti à la corbeille");
}

// ───────────────────────── Annuler (Ctrl+Z), geste par geste ─────────────────────────
// `App::defaire` exige une fenêtre : on rejoue ce qu'il lance (app.rs, `defaire`) avec les mêmes fonctions.

#[test]
fn annuler_une_copie_remet_l_etat_d_avant() {
    let mut b = Banc::nouveau("annuler-copie");
    let source = b.ecrire("src/a.txt", &octets(3000, 12));
    let dst = b.dossier("dst");
    let bilan = b.lancer(Genre::Copier, &[el(&source, Some(&dst))]);
    let copie = bilan.resultats[0].1.clone().expect("copie créée");
    assert!(Path::new(&copie).exists());
    // Ctrl+Z : les copies créées partent à la corbeille
    let inverse = b.lancer(Genre::Corbeille, &[el(&copie, None)]);
    assert!(inverse.erreur.is_none(), "{:?}", inverse.erreur);
    assert!(!Path::new(&copie).exists(), "la copie est supprimée");
    assert_eq!(std::fs::read(&source).unwrap(), octets(3000, 12), "la source n'a pas bougé");
}

#[test]
fn annuler_une_copie_avec_remplacement_rend_l_ancien() {
    let mut b = Banc::nouveau("annuler-copie-remplace");
    let source = b.ecrire("src/a.txt", &octets(3000, 13));
    let ancien = b.ecrire("dst/a.txt", &octets(1500, 14));
    let dst = b.p("dst");
    let e = Element { source: source.clone(), dest: Some(dst), nom: None, remplace: Some(ancien.clone()) };
    let bilan = b.lancer(Genre::Copier, &[e]);
    let copie = bilan.resultats[0].1.clone().expect("copie créée");
    // Ctrl+Z : la copie à la corbeille, puis l'ancien revient (`apres` de fin_operation → historique::restaurer)
    let inverse = b.lancer(Genre::Corbeille, &[el(&copie, None)]);
    assert!(inverse.erreur.is_none(), "{:?}", inverse.erreur);
    assert_eq!(historique::restaurer(&bilan.remplaces), Ok(1));
    assert_eq!(std::fs::read(&ancien).unwrap(), octets(1500, 14), "l'ancien contenu est revenu");
    assert_eq!(std::fs::read(&source).unwrap(), octets(3000, 13));
}

#[test]
fn annuler_un_deplacement_remet_en_place() {
    let mut b = Banc::nouveau("annuler-deplacement");
    let source = b.ecrire("src/a.txt", &octets(3000, 15));
    let dst = b.dossier("dst");
    let src = b.p("src");
    let bilan = b.lancer(Genre::Deplacer, &[el(&source, Some(&dst))]);
    let actuel = bilan.resultats[0].1.clone().expect("nouvel emplacement");
    assert!(!Path::new(&source).exists() && Path::new(&actuel).exists());
    // Ctrl+Z : retour dans le dossier d'origine
    let inverse = b.lancer(Genre::Deplacer, &[el(&actuel, Some(&src))]);
    assert!(inverse.erreur.is_none(), "{:?}", inverse.erreur);
    assert_eq!(std::fs::read(&source).unwrap(), octets(3000, 15));
    assert!(!Path::new(&actuel).exists());
}

#[test]
fn annuler_une_mise_a_la_corbeille_restaure() {
    let mut b = Banc::nouveau("annuler-corbeille");
    let f = b.ecrire("a.txt", &octets(2048, 16));
    let bilan = b.lancer(Genre::Corbeille, &[el(&f, None)]);
    let r = bilan.resultats[0].1.clone().expect("élément de corbeille");
    assert!(!Path::new(&f).exists());
    // Ctrl+Z : `Action::Corbeille` → historique::restaurer
    assert_eq!(historique::restaurer(&[(f.clone(), r.clone())]), Ok(1));
    assert_eq!(std::fs::read(&f).unwrap(), octets(2048, 16));
    assert!(!Path::new(&r).exists(), "plus dans la corbeille");
    // Et Ctrl+Y remet à la corbeille
    let encore = b.lancer(Genre::Corbeille, &[el(&f, None)]);
    assert!(encore.erreur.is_none() && !Path::new(&f).exists());
}

#[test]
fn annuler_un_renommage_rend_l_ancien_nom() {
    let b = Banc::nouveau("annuler-renommage");
    let f = b.ecrire("ancien.txt", &octets(777, 17));
    let dossier = b.racine.to_string_lossy().into_owned();
    renommer(&dossier, "ancien.txt", "nouveau.txt").unwrap();
    assert!(!Path::new(&f).exists() && Path::new(&b.p("nouveau.txt")).exists());
    // Ctrl+Z : le renommage inverse (app.rs, `Action::Renommage`)
    renommer(&dossier, "nouveau.txt", "ancien.txt").unwrap();
    assert_eq!(std::fs::read(&f).unwrap(), octets(777, 17));
    // Ne remplace jamais : un homonyme apparu entre-temps fait échouer l'annulation sans rien perdre
    b.ecrire("autre.txt", b"autre");
    let e = renommer(&dossier, "ancien.txt", "autre.txt").unwrap_err();
    assert!(e.contains("EXISTE"), "{e}");
    assert_eq!(std::fs::read(b.p("autre.txt")).unwrap(), b"autre");
}

// ───────────────────────── Annulation en cours d'opération ─────────────────────────

#[test]
fn annulation_a_0_pour_cent_ne_touche_a_rien() {
    let mut b = Banc::nouveau("annulation-0");
    let source = b.ecrire("src/gros.bin", &octets(8 << 20, 18));
    let dst = b.dossier("dst");
    let controle = Controle::default();
    controle.annule.store(true, Ordering::Relaxed); // annulée avant le premier octet
    let bilan = b.lancer_avec(Genre::Copier, &[el(&source, Some(&dst))], controle);
    eprintln!("OBSERVE annulation-0 : annule={}, erreur={:?}, resultats={:?}, cible existe={}", bilan.annule, bilan.erreur, bilan.resultats, Path::new(&b.p("dst/gros.bin")).exists());
    assert!(!Path::new(&b.p("dst/gros.bin")).exists(), "rien n'a été copié");
    assert!(bilan.resultats.is_empty(), "aucun résultat pour une copie annulée");
    assert_eq!(std::fs::read_dir(&dst).unwrap().count(), 0, "dossier cible vide");
    assert_eq!(empreinte(&source), empreinte_attendue(8 << 20, 18), "source intacte");
}

/// D1 (corrigé) : annulée avant le premier octet, la copie ne fait rien (le test ci-dessus le prouve) ; `Bilan.annule`
/// devait rester faux car les rappels `Pre*Item` rendent E_ABORT, que le moteur traite comme « élément ignoré » sans
/// lever `GetAnyOperationsAborted`. `Suivi.refuse` mémorise le refus et `executer` en tire `annule = true`.
#[test]
fn annulation_a_0_pour_cent_est_signalee_comme_annulee() {
    let mut b = Banc::nouveau("annulation-0-bilan");
    let source = b.ecrire("src/gros.bin", &octets(8 << 20, 18));
    let dst = b.dossier("dst");
    let controle = Controle::default();
    controle.annule.store(true, Ordering::Relaxed);
    let bilan = b.lancer_avec(Genre::Copier, &[el(&source, Some(&dst))], controle);
    assert!(bilan.annule, "bilan.annule doit être vrai (erreur : {:?})", bilan.erreur);
}

fn empreinte_attendue(n: usize, graine: u64) -> u64 {
    // Calculée en mémoire, sans écrire de fichier
    let mut h: u64 = 0xcbf29ce484222325;
    for b in octets(n, graine) {
        h = (h ^ b as u64).wrapping_mul(0x100000001b3);
    }
    h
}

/// Ce qu'on observe d'une copie de `nb` fichiers de `taille` octets dont l'annulation est demandée par un fil observateur
/// quand le fichier n° `k` a reçu `pour_mille`/1000 de ses octets.
struct CopieAnnulee {
    bilan: Bilan,
    /// L'observateur a demandé l'annulation (le seuil a été atteint en cours de copie)
    demandee: bool,
    sources: Vec<String>,
    cibles: Vec<String>,
    taille: usize,
    _banc: Banc,
}

fn copie_annulee_a(nom: &str, nb: usize, taille: usize, k: usize, pour_mille: u64) -> CopieAnnulee {
    let par_dialogue = nb == 1;
    PROGRES_DIALOGUE.store(0, Ordering::Relaxed);
    let mut b = Banc::nouveau(nom);
    let donnees: Vec<Arc<Vec<u8>>> = (0..nb).map(|i| Arc::new(octets(taille, 100 + i as u64))).collect();
    let sources: Vec<String> = (0..nb).map(|i| b.ecrire(&format!("src/f{i}.bin"), &donnees[i])).collect();
    let dst = b.dossier("dst");
    let cibles: Vec<String> = (0..nb).map(|i| b.p(&format!("dst/f{i}.bin"))).collect();
    let controle = Controle::default();
    let fini = Arc::new(AtomicBool::new(false));
    let demandee = Arc::new(AtomicBool::new(false));
    let observateur = {
        let (annule, fini, demandee, cible, donnees) = (controle.annule.clone(), fini.clone(), demandee.clone(), cibles[k].clone(), donnees[k].clone());
        std::thread::spawn(move || {
            use std::io::{Read, Seek, SeekFrom};
            // Windows réserve d'emblée la taille finale de la cible : la taille du fichier ne mesure rien. On regarde donc
            // si les octets situés juste avant le seuil sont arrivés (ceux d'une zone non écrite se lisent en zéros).
            let seuil = (taille as u64 * pour_mille / 1000).max(4096);
            let debut = seuil - 4096;
            while !fini.load(Ordering::Relaxed) {
                // Un seul fichier : la cible est verrouillée en lecture tant que dure la copie, on suit donc l'avancement que
                // le Shell annonce à notre boîte de progression (`PROGRES_DIALOGUE`)
                if par_dialogue && PROGRES_DIALOGUE.load(Ordering::Relaxed) >= pour_mille {
                    annule.store(true, Ordering::Relaxed);
                    demandee.store(true, Ordering::Relaxed);
                    return;
                }
                if let Ok(mut f) = std::fs::File::open(&cible) {
                    let mut fenetre = [0u8; 4096];
                    if f.seek(SeekFrom::Start(debut)).is_ok() && f.read_exact(&mut fenetre).is_ok() && fenetre[..] == donnees[debut as usize..seuil as usize] {
                        annule.store(true, Ordering::Relaxed);
                        demandee.store(true, Ordering::Relaxed);
                        return;
                    }
                }
                std::thread::yield_now();
            }
        })
    };
    let elements: Vec<Element> = sources.iter().map(|s| el(s, Some(&dst))).collect();
    let bilan = b.lancer_avec(Genre::Copier, &elements, controle);
    fini.store(true, Ordering::Relaxed);
    observateur.join().unwrap();
    CopieAnnulee { bilan, demandee: demandee.load(Ordering::Relaxed), sources, cibles, taille, _banc: b }
}

impl CopieAnnulee {
    /// Invariants qui valent quel que soit le moment de l'annulation : sources intactes, aucun fichier partiel dans la
    /// destination (absent, ou complet et identique), tout résultat annoncé existe et est complet. Renvoie le nombre de copies complètes.
    fn verifier_sans_perte_ni_partiel(&self) -> usize {
        let mut completes = 0;
        for (i, (s, c)) in self.sources.iter().zip(&self.cibles).enumerate() {
            assert_eq!(empreinte(s), empreinte_attendue(self.taille, 100 + i as u64), "source {i} intacte");
            if let Ok(m) = std::fs::metadata(c) {
                assert_eq!(m.len(), self.taille as u64, "fichier partiel laissé dans la destination : {c} ({} octets)", m.len());
                assert_eq!(empreinte(c), empreinte(s), "copie {i} corrompue");
                completes += 1;
            }
        }
        for (_, n) in &self.bilan.resultats {
            let n = n.as_deref().expect("chemin de la copie");
            assert!(self.cibles.iter().any(|c| c.eq_ignore_ascii_case(n)) && Path::new(n).exists(), "résultat annoncé mais absent : {n}");
        }
        completes
    }
}

#[test]
fn annulation_a_50_pour_cent_d_un_lot_arrete_la_suite() {
    // 10 fichiers de 16 Mo, annulation demandée quand le 5e est à moitié copié
    let c = copie_annulee_a("annulation-lot-50", 10, 16 << 20, 4, 500);
    let completes = c.verifier_sans_perte_ni_partiel();
    eprintln!("OBSERVE annulation lot 50 % : demandée={}, annule={}, erreur={:?}, copies complètes={completes}/10", c.demandee, c.bilan.annule, c.bilan.erreur);
    assert!(c.demandee, "le seuil n'a pas été vu : annulation non exercée");
    assert!(c.bilan.annule, "bilan.annule doit être vrai (erreur : {:?})", c.bilan.erreur);
    assert!(completes < 10, "l'annulation devait épargner la fin du lot ({completes}/10 copiés)");
}

#[test]
fn annulation_a_99_pour_cent_d_un_lot_ne_laisse_rien_de_partiel() {
    // Annulation demandée quand le dernier fichier est copié à 99 % : le fichier en cours se termine ou est retiré,
    // mais jamais laissé à moitié
    let c = copie_annulee_a("annulation-lot-99", 6, 16 << 20, 5, 990);
    let completes = c.verifier_sans_perte_ni_partiel();
    eprintln!("OBSERVE annulation lot 99 % : demandée={}, annule={}, erreur={:?}, copies complètes={completes}/6", c.demandee, c.bilan.annule, c.bilan.erreur);
    assert!(c.demandee, "le seuil n'a pas été vu : annulation non exercée");
    assert!(c.bilan.erreur.is_none());
}

#[test]
fn annulation_dans_un_seul_gros_fichier_ne_laisse_pas_de_partiel() {
    // D2 (corrigé) : Échap pendant la copie d'UN fichier l'interrompt grâce à la boîte de progression muette
    // (`Annulation`, `GetOperationStatus` rend PDOPS_CANCELLED) ; sans elle le moteur ne rappelle notre sink qu'entre
    // éléments et le fichier arrivait complet. Attendu : annulée, aucune copie complète, aucun partiel.
    for (nom, pm) in [("annulation-fichier-50", 500), ("annulation-fichier-99", 990)] {
        let c = copie_annulee_a(nom, 1, 256 << 20, 0, pm);
        let completes = c.verifier_sans_perte_ni_partiel();
        eprintln!("OBSERVE {nom} : demandée={}, annule={}, erreur={:?}, copie complète={completes}/1", c.demandee, c.bilan.annule, c.bilan.erreur);
        assert!(c.demandee, "le seuil n'a pas été vu : annulation non exercée");
        assert!(c.bilan.annule, "D2 : la copie d'un seul fichier doit être interrompue (annule faux, {completes}/1 copié)");
        assert_eq!(completes, 0, "une copie interrompue ne laisse aucun fichier (ni partiel ni complet)");
    }
}

// ───────────────────────── Lignes que le banc ne peut pas exercer seul ─────────────────────────

#[test]
#[ignore = "écrire sur un 2e volume sort de %TEMP% (interdit au banc) ; procédure manuelle ci-dessous"]
fn deplacement_entre_volumes_interrompu() {
    // Procédure manuelle : avoir un 2e volume local inscriptible (D:, clé USB, VHD). Y créer un dossier de test et,
    // sur C:, un fichier de 2 Go. Les déplacer de C: vers D: avec SpaceFiles (Ctrl+X, Ctrl+V), appuyer sur Échap à
    // mi-course. Attendu : le fichier source est toujours sur C: intact (empreinte identique), la barre d'état dit
    // « DEPLACEMENT ANNULE », aucun fichier partiel côté D: (ou un reste signalé). Recommencer en débranchant le volume.
    unreachable!("test documenté, jamais exécuté automatiquement");
}

#[test]
#[ignore = "exige un partage réseau réel qu'on puisse couper ; ne se simule pas"]
fn coupure_de_partage_reseau() {
    // Procédure manuelle : copier 1 Go vers \\serveur\partage depuis SpaceFiles, couper le réseau (Wi-Fi) à 30 %.
    // Attendu : la barre d'état annonce l'attente du réseau (`attend_reseau`), la copie reprend seule au retour du
    // réseau (essai toutes les 3 s, 10 minutes au plus), les éléments déjà finis ne sont pas refaits, la source est intacte.
    unreachable!("test documenté, jamais exécuté automatiquement");
}

// ───────────────────────── Fichiers verrouillés, lecture seule, Unicode, chemins longs ─────────────────────────

#[test]
fn fichier_verrouille_pas_de_perte_et_erreur_lisible() {
    let b = Banc::nouveau("verrouille");
    let f = b.ecrire("v.txt", &octets(500, 20));
    let dossier = b.racine.to_string_lossy().into_owned();
    // Handle ouvert sans aucun partage (ni lecture, ni écriture, ni suppression)
    let verrou = std::fs::OpenOptions::new().read(true).share_mode(0).open(&f).unwrap();
    let e = renommer(&dossier, "v.txt", "w.txt").unwrap_err();
    assert!(e.contains("AUTRE APPLICATION"), "message lisible attendu, reçu : {e}");
    assert!(std::fs::remove_file(&f).is_err(), "la suppression directe échoue aussi");
    drop(verrou);
    assert_eq!(std::fs::read(&f).unwrap(), octets(500, 20), "aucune perte");
    renommer(&dossier, "v.txt", "w.txt").unwrap();
}

#[test]
// `executer` ajoute FOF_NOERRORUI en test : sans cela Windows ouvre « Fichier en cours d'utilisation » et attend un clic
// (c'était le blocage D3 du premier passage). Dans l'exe la boîte reste : l'auteur peut répondre Réessayer / Annuler.
fn fichier_verrouille_par_le_moteur_de_windows() {
    let mut b = Banc::nouveau("verrouille-shell");
    let f = b.ecrire("v.txt", &octets(500, 21));
    let verrou = std::fs::OpenOptions::new().read(true).share_mode(0).open(&f).unwrap();
    let bilan = b.lancer(Genre::Corbeille, &[el(&f, None)]);
    eprintln!("OBSERVE verrouille-shell : erreur={:?} annule={}", bilan.erreur, bilan.annule);
    assert!(Path::new(&f).exists(), "le fichier verrouillé ne doit pas disparaître");
    assert!(bilan.resultats.is_empty(), "rien n'a été mis à la corbeille");
    // Sans la boîte de Windows, l'échec est signalé par `annule` (opération interrompue) : jamais comme un succès
    assert!(bilan.annule || bilan.erreur.is_some(), "un fichier verrouillé ne doit pas être annoncé comme réussi");
    drop(verrou);
    assert_eq!(std::fs::read(&f).unwrap(), octets(500, 21), "aucune perte");
}

#[test]
fn lecture_seule_corbeille_copie_et_suppression_sans_perte() {
    let mut b = Banc::nouveau("lecture-seule");
    let source = b.ecrire("src/ro.txt", &octets(900, 22));
    let mut perm = std::fs::metadata(&source).unwrap().permissions();
    perm.set_readonly(true);
    std::fs::set_permissions(&source, perm).unwrap();
    let dst = b.dossier("dst");
    // Copie : le contenu arrive intact
    let bilan = b.lancer(Genre::Copier, &[el(&source, Some(&dst))]);
    assert!(bilan.erreur.is_none(), "{:?}", bilan.erreur);
    assert_eq!(std::fs::read(b.p("dst/ro.txt")).unwrap(), octets(900, 22));
    // Corbeille : soit le fichier est dans la corbeille et récupérable, soit il est intact avec une erreur lisible
    let copie = b.p("dst/ro.txt");
    let bilan = b.lancer(Genre::Corbeille, &[el(&copie, None)]);
    eprintln!("OBSERVE lecture-seule corbeille : erreur={:?}, existe encore={}", bilan.erreur, Path::new(&copie).exists());
    match (&bilan.erreur, Path::new(&copie).exists()) {
        (None, false) => {
            let r = bilan.resultats[0].1.clone().expect("élément de corbeille");
            assert_eq!(std::fs::read(&r).unwrap(), octets(900, 22));
        }
        (Some(e), true) => assert!(!e.is_empty()),
        autre => panic!("état incohérent (ni mis à la corbeille ni intact) : {autre:?}"),
    }
    // Suppression définitive de la source en lecture seule : disparue sans erreur, ou intacte avec une erreur lisible
    let bilan = b.lancer(Genre::SupprimerDefinitivement, &[el(&source, None)]);
    eprintln!("OBSERVE lecture-seule définitive : erreur={:?}, existe encore={}", bilan.erreur, Path::new(&source).exists());
    match (&bilan.erreur, Path::new(&source).exists()) {
        (None, false) => {}
        (Some(e), true) => {
            assert!(!e.is_empty());
            assert_eq!(std::fs::read(&source).unwrap(), octets(900, 22), "intact");
        }
        autre => panic!("état incohérent : {autre:?}"),
    }
}

#[test]
fn noms_unicode_copie_deplacement_corbeille_definitive() {
    let mut b = Banc::nouveau("unicode");
    let nom = "été – 日本語 – ñandú – 😀 – Ω.txt";
    let source = b.ecrire(&format!("src/{nom}"), &octets(1500, 23));
    let dst = b.dossier("dst");
    let src = b.p("src");
    // Copie
    let bilan = b.lancer(Genre::Copier, &[el(&source, Some(&dst))]);
    assert!(bilan.erreur.is_none(), "{:?}", bilan.erreur);
    let copie = b.p(&format!("dst/{nom}"));
    assert_eq!(std::fs::read(&copie).unwrap(), octets(1500, 23));
    assert_eq!(bilan.resultats[0].1.as_deref(), Some(copie.as_str()), "le bilan rend le nom exact");
    // Déplacement aller-retour
    let aller = b.lancer(Genre::Deplacer, &[el(&copie, Some(&src))]);
    assert!(aller.erreur.is_none(), "{:?}", aller.erreur);
    assert!(!Path::new(&copie).exists());
    // Corbeille puis restauration au nom exact
    let bilan = b.lancer(Genre::Corbeille, &[el(&source, None)]);
    assert!(bilan.erreur.is_none(), "{:?}", bilan.erreur);
    let r = bilan.resultats[0].1.clone().expect("élément de corbeille");
    assert_eq!(historique::restaurer(&[(source.clone(), r)]), Ok(1));
    assert_eq!(std::fs::read(&source).unwrap(), octets(1500, 23));
    // Suppression définitive
    let bilan = b.lancer(Genre::SupprimerDefinitivement, &[el(&source, None)]);
    assert!(bilan.erreur.is_none() && !Path::new(&source).exists(), "{:?}", bilan.erreur);
}

#[test]
fn chemin_de_plus_de_260_caracteres_pas_de_perte() {
    let mut b = Banc::nouveau("long");
    let profond = format!("{}/{}/{}/{}/{}/{}", "d".repeat(50), "e".repeat(50), "f".repeat(50), "g".repeat(50), "h".repeat(50), "i".repeat(50));
    let f = b.ecrire(&format!("{profond}/fichier-long.txt"), &octets(2000, 24));
    assert!(f.len() > 260, "le chemin doit dépasser 260 caractères ({})", f.len());
    let dst = b.dossier("dst");
    // Copie
    let bilan = b.lancer(Genre::Copier, &[el(&f, Some(&dst))]);
    eprintln!("OBSERVE long copie : erreur={:?}, copie existe={}", bilan.erreur, Path::new(&b.p("dst/fichier-long.txt")).exists());
    match &bilan.erreur {
        None => assert_eq!(std::fs::read(b.p("dst/fichier-long.txt")).unwrap(), octets(2000, 24)),
        Some(e) => assert!(!e.is_empty(), "erreur lisible attendue"),
    }
    assert_eq!(std::fs::read(&f).unwrap(), octets(2000, 24), "la source d'une copie est intacte");
    // Corbeille : récupérable ou intact avec erreur
    let bilan = b.lancer(Genre::Corbeille, &[el(&f, None)]);
    eprintln!("OBSERVE long corbeille : erreur={:?}, existe encore={}", bilan.erreur, Path::new(&f).exists());
    match (&bilan.erreur, Path::new(&f).exists()) {
        (None, false) => {
            // DÉFAUT CONSTATÉ (rapporté, non corrigé) : le fichier est bien à la corbeille mais le bilan ne le dit pas
            // (`resultats` vide), donc Ctrl+Z n'aurait rien à restaurer. Le test retrouve l'élément par la corbeille.
            eprintln!("OBSERVE long corbeille : resultats du bilan = {:?}", bilan.resultats);
            // D4 (corrigé) : le bilan doit annoncer l'élément, sinon pas d'entrée d'historique donc pas de Ctrl+Z
            assert_eq!(bilan.resultats.len(), 1, "D4 : le bilan doit annoncer la mise à la corbeille : {:?}", bilan.resultats);
            let r = bilan.resultats.first().and_then(|x| x.1.clone()).or_else(|| chercher_dans_corbeille(&f)).expect("le fichier a disparu mais n'est nulle part dans la corbeille : PERTE");
            // D5 (corrigé) : Ctrl+Z doit remettre le fichier lui-même, sans repli sur `std::fs`
            let geste_de_ctrl_z = historique::restaurer(&[(f.clone(), r.clone())]);
            eprintln!("OBSERVE long corbeille : historique::restaurer = {geste_de_ctrl_z:?}");
            assert_eq!(geste_de_ctrl_z, Ok(1), "D5 : la restauration d'un chemin > 260 doit réussir");
            assert_eq!(std::fs::read(&f).unwrap(), octets(2000, 24), "aucune perte de données");
        }
        (Some(e), true) => assert!(!e.is_empty()),
        autre => panic!("état incohérent : {autre:?}"),
    }
}

// ───────────────────────── Chemins longs : appariement des éléments (B1, I1) ─────────────────────────

/// Sous-dossier profond (plus de 200 caractères avant le nom du fichier) : `racine/<nom>/…`
fn profond(nom: &str) -> String {
    format!("{nom}/{}/{}/{}/{}/{}", "e".repeat(50), "f".repeat(50), "g".repeat(50), "h".repeat(50), "i".repeat(50))
}

#[test]
fn deux_fichiers_longs_voisins_donnent_deux_paires_distinctes_et_se_restaurent() {
    let mut b = Banc::nouveau("long-voisins");
    let d = profond("dossier-long");
    let jan = b.ecrire(&format!("{d}/rapport-2025-janvier.docx"), &octets(1111, 30));
    let fev = b.ecrire(&format!("{d}/rapport-2025-fevrier.docx"), &octets(2222, 31));
    assert!(jan.len() > 260 && fev.len() > 260);
    let bilan = b.lancer(Genre::Corbeille, &[el(&jan, None), el(&fev, None)]);
    assert!(bilan.erreur.is_none(), "{:?}", bilan.erreur);
    assert_eq!(bilan.resultats.len(), 2, "{:?}", bilan.resultats);
    assert_eq!(bilan.resultats[0].0, jan, "première paire = première source");
    assert_eq!(bilan.resultats[1].0, fev, "seconde paire = seconde source");
    assert_ne!(bilan.resultats[0].1, bilan.resultats[1].1, "deux éléments de corbeille distincts");
    // Ctrl+Z : les deux reviennent, chacun avec son contenu
    let paires: Vec<(String, String)> = bilan.resultats.iter().map(|(o, n)| (o.clone(), n.clone().expect("élément de corbeille"))).collect();
    assert_eq!(historique::restaurer(&paires), Ok(2));
    assert_eq!(std::fs::read(&jan).unwrap(), octets(1111, 30));
    assert_eq!(std::fs::read(&fev).unwrap(), octets(2222, 31));
}

#[test]
fn deplacer_depuis_deux_dossiers_cousins_longs_revient_chacun_chez_soi() {
    let mut b = Banc::nouveau("long-cousins");
    let (da, db) = (profond("dossier-cousin-un"), profond("dossier-cousin-deux"));
    let fa = b.ecrire(&format!("{da}/un.txt"), &octets(1500, 32));
    let fb = b.ecrire(&format!("{db}/deux.txt"), &octets(2500, 33));
    let dst = b.dossier("dst");
    assert!(fa.len() > 260 && fb.len() > 260);
    let bilan = b.lancer(Genre::Deplacer, &[el(&fa, Some(&dst)), el(&fb, Some(&dst))]);
    assert!(bilan.erreur.is_none(), "{:?}", bilan.erreur);
    assert_eq!(bilan.resultats.len(), 2, "{:?}", bilan.resultats);
    assert_eq!(bilan.resultats[0].0, fa);
    assert_eq!(bilan.resultats[1].0, fb);
    // Ctrl+Z (app.rs, `Action::Deplacement`) : chaque fichier retourne dans le dossier de son origine
    for (origine, actuel) in &bilan.resultats {
        let actuel = actuel.clone().expect("nouvel emplacement");
        let parent = origine.rsplit_once('\\').unwrap().0.to_string();
        let inverse = b.lancer(Genre::Deplacer, &[el(&actuel, Some(&parent))]);
        assert!(inverse.erreur.is_none(), "{:?}", inverse.erreur);
    }
    assert_eq!(std::fs::read(&fa).unwrap(), octets(1500, 32));
    assert_eq!(std::fs::read(&fb).unwrap(), octets(2500, 33));
}

#[test]
fn un_vrai_nom_avec_tilde_n_est_pas_pris_pour_un_abrege() {
    let mut b = Banc::nouveau("tilde-reel");
    let reel = b.ecrire("RAPPOR~1.DOC", &octets(700, 34));
    let autre = b.ecrire("rapport-long-nom.doc", &octets(800, 35));
    let bilan = b.lancer(Genre::Corbeille, &[el(&reel, None), el(&autre, None)]);
    assert!(bilan.erreur.is_none(), "{:?}", bilan.erreur);
    assert_eq!(bilan.resultats.len(), 2, "{:?}", bilan.resultats);
    assert_eq!(bilan.resultats[0].0, reel);
    assert_eq!(bilan.resultats[1].0, autre);
    let paires: Vec<(String, String)> = bilan.resultats.iter().map(|(o, n)| (o.clone(), n.clone().expect("corbeille"))).collect();
    assert_eq!(historique::restaurer(&paires), Ok(2));
    assert_eq!(std::fs::read(&reel).unwrap(), octets(700, 34));
    assert_eq!(std::fs::read(&autre).unwrap(), octets(800, 35));
}

#[test]
fn destination_longue_le_bilan_donne_un_chemin_qui_existe() {
    let mut b = Banc::nouveau("long-destination");
    let source = b.ecrire("src/court.txt", &octets(600, 36));
    let dst = b.dossier(&profond("destination-longue"));
    assert!(dst.len() > 230);
    let bilan = b.lancer(Genre::Copier, &[el(&source, Some(&dst))]);
    assert!(bilan.erreur.is_none(), "{:?}", bilan.erreur);
    assert_eq!(bilan.resultats.len(), 1, "{:?}", bilan.resultats);
    let attendu = format!("{dst}\\court.txt");
    match &bilan.resultats[0].1 {
        Some(c) => {
            assert!(!c.contains('~'), "chemin abrégé dans le bilan : {c}");
            assert!(c.eq_ignore_ascii_case(&attendu) && Path::new(c).exists(), "{c} (attendu {attendu})");
        }
        None => panic!("I1 : le chemin long de la copie doit être retrouvé (sinon pas de Ctrl+Z)"),
    }
}
