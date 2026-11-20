// Instantané de la dernière analyse automatique (E4c), comparaison avec l'analyse précédente et texte de la carte
// « récupérable » de Ce PC (E9).
//
// Un petit fichier texte dans %LOCALAPPDATA%\SpaceFiles (quelques centaines d'octets : date, puis octets
// récupérables par catégorie), jamais l'arbre : le résident reste dans son budget de mémoire. La mesure est celle de la
// page NETTOYAGE (`nettoyage::mesurer_tout`), lancée en fond à l'ouverture de la fenêtre quand la dernière date de plus
// de 24 h. Rien n'est supprimé ici : l'analyse automatique ne fait que mesurer.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use windows::Win32::System::Threading::{GetCurrentThread, SetThreadPriority, THREAD_MODE_BACKGROUND_BEGIN};

use super::nettoyage::{self, Categorie, Racines};

/// Âge à partir duquel la dernière analyse est refaite
pub const DELAI_SECONDES: u64 = 24 * 3600;

/// Première ligne du fichier : change si le format change (un fichier d'une autre version est ignoré, jamais mal lu)
const ENTETE: &str = "SpaceFiles-analyse 1";
const NOM_FICHIER: &str = "analyse-auto.txt";

/// Ce que la dernière analyse a trouvé
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Instantane {
    /// Secondes depuis le 1er janvier 1970
    pub date: u64,
    /// (identifiant de la catégorie, octets récupérables)
    pub categories: Vec<(String, u64)>,
}

/// Secondes depuis 1970 (0 si l'horloge est avant)
pub fn secondes(t: SystemTime) -> u64 {
    t.duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

/// Faut-il refaire l'analyse ? Oui sans instantané, à partir de 24 h, et si la date enregistrée est dans le futur (horloge
/// reculée : sinon l'analyse ne se referait jamais).
pub fn faut_relancer(derniere: Option<u64>, maintenant: u64) -> bool {
    match derniere {
        None => true,
        Some(d) => d > maintenant || maintenant - d >= DELAI_SECONDES,
    }
}

/// L'instantané d'une mesure
pub fn de_mesure(cats: &[Categorie], maintenant: SystemTime) -> Instantane {
    Instantane { date: secondes(maintenant), categories: cats.iter().map(|c| (c.id.to_string(), c.octets)).collect() }
}

pub fn encoder(i: &Instantane) -> String {
    let mut s = format!("{ENTETE}\ndate {}\n", i.date);
    for (id, octets) in &i.categories {
        s.push_str(&format!("cat {id} {octets}\n"));
    }
    s
}

/// Lit un instantané ; `None` pour tout ce qui n'est pas exactement notre format (fichier abîmé, autre version)
pub fn decoder(texte: &str) -> Option<Instantane> {
    let mut lignes = texte.lines();
    if lignes.next()? != ENTETE {
        return None;
    }
    let date = lignes.next()?.strip_prefix("date ")?.parse().ok()?;
    let mut categories = Vec::new();
    for l in lignes {
        let mut m = l.strip_prefix("cat ")?.split(' ');
        let (id, octets) = (m.next()?, m.next()?);
        if id.is_empty() || m.next().is_some() {
            return None;
        }
        categories.push((id.to_string(), octets.parse().ok()?));
    }
    Some(Instantane { date, categories })
}

/// Total récupérable d'un instantané
pub fn total(i: &Instantane) -> u64 {
    i.categories.iter().map(|(_, o)| *o).sum()
}

/// Ce qui a changé entre deux analyses
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Ecart {
    /// Octets récupérables de la plus récente moins ceux de la précédente (négatif : il y en a moins)
    pub delta: i64,
    /// Secondes écoulées entre les deux analyses
    pub depuis: u64,
    /// Écart de chaque catégorie présente dans l'une ou l'autre, la plus grande variation d'abord (les égalités sont omises)
    pub par_categorie: Vec<(String, i64)>,
}

fn signe(a: u64, b: u64) -> i64 {
    i64::try_from(i128::from(a) - i128::from(b)).unwrap_or(if a > b { i64::MAX } else { i64::MIN })
}

/// Compare la plus récente analyse à la précédente
pub fn comparer(avant: &Instantane, apres: &Instantane) -> Ecart {
    let mut ids: Vec<&str> = avant.categories.iter().chain(&apres.categories).map(|(id, _)| id.as_str()).collect();
    ids.sort_unstable();
    ids.dedup();
    let octets = |i: &Instantane, id: &str| i.categories.iter().find(|(x, _)| x == id).map_or(0, |(_, o)| *o);
    let mut par_categorie: Vec<(String, i64)> =
        ids.into_iter().map(|id| (id.to_string(), signe(octets(apres, id), octets(avant, id)))).filter(|(_, d)| *d != 0).collect();
    par_categorie.sort_by(|a, b| b.1.unsigned_abs().cmp(&a.1.unsigned_abs()).then_with(|| a.0.cmp(&b.0)));
    Ecart { delta: signe(total(apres), total(avant)), depuis: apres.date.saturating_sub(avant.date), par_categorie }
}

/// Durée courte : « 5 MIN », « 6 H », « 3 J »
pub fn duree_courte(secondes: u64) -> String {
    match secondes {
        s if s < 3600 => format!("{} MIN", (s / 60).max(1)),
        s if s < 86_400 => format!("{} H", s / 3600),
        s => format!("{} J", s / 86_400),
    }
}

/// Texte de la carte « récupérable » de Ce PC : « RÉCUPÉRABLE : 3,2 GO · +1,1 GO DEPUIS 2 J ». Sans analyse précédente, ou
/// sans changement, seul le total s'affiche (un « +0 » ne dirait rien).
pub fn libelle_carte(courant: &Instantane, precedent: Option<&Instantane>, taille: impl Fn(u64) -> String) -> String {
    let base = format!("RÉCUPÉRABLE : {}", taille(total(courant)));
    let Some(p) = precedent else { return base };
    let e = comparer(p, courant);
    if e.delta == 0 {
        return base;
    }
    let signe = if e.delta > 0 { '+' } else { '−' };
    format!("{base} · {signe}{} DEPUIS {}", taille(e.delta.unsigned_abs()), duree_courte(e.depuis))
}

/// Dernière analyse et celle d'avant, gardées en mémoire (un fichier lu à chaque dessin de Ce PC serait un coût pour rien).
/// `None` dehors : pas encore chargé ; `Some(None)` : pas d'analyse.
static RESUME: Mutex<Option<Option<(Instantane, Option<Instantane>)>>> = Mutex::new(None);

/// La dernière analyse enregistrée et la précédente, si elle existe
pub fn resume() -> Option<(Instantane, Option<Instantane>)> {
    let mut r = RESUME.lock().ok()?;
    if r.is_none() {
        *r = Some(chemin().and_then(|c| Some((lire_de(&c)?, lire_de(&precedent_de(&c))))));
    }
    r.clone().flatten()
}

fn oublier_resume() {
    if let Ok(mut r) = RESUME.lock() {
        *r = None;
    }
}

/// Fichier où passe l'ancienne analyse quand une nouvelle la remplace
fn precedent_de(chemin: &Path) -> PathBuf {
    chemin.with_extension("prec")
}

fn chemin() -> Option<PathBuf> {
    Some(crate::reglages::dossier_donnees(true)?.join(NOM_FICHIER))
}

pub fn lire_de(chemin: &Path) -> Option<Instantane> {
    decoder(&std::fs::read_to_string(chemin).ok()?)
}

/// Écrit l'instantané : fichier voisin puis renommage, pour qu'un arrêt en pleine écriture ne laisse jamais un fichier
/// tronqué à la place du précédent
pub fn enregistrer_dans(chemin: &Path, i: &Instantane) -> bool {
    let tmp = chemin.with_extension("tmp");
    if let Some(d) = chemin.parent() {
        let _ = std::fs::create_dir_all(d);
    }
    // L'analyse qu'on remplace devient « la précédente » (E9), mais seulement si elle est lisible : un fichier abîmé ne doit pas
    // écraser une bonne précédente
    if lire_de(chemin).is_some() {
        let _ = std::fs::copy(chemin, precedent_de(chemin));
    }
    let ok = std::fs::write(&tmp, encoder(i)).is_ok() && std::fs::rename(&tmp, chemin).is_ok();
    if !ok {
        let _ = std::fs::remove_file(&tmp);
    }
    ok
}

/// Une analyse automatique court : on n'en lance pas une seconde
static EN_COURS: AtomicBool = AtomicBool::new(false);

/// À l'ouverture de la fenêtre : si la dernière analyse date de plus de 24 h (ou n'existe pas), mesure en fond à priorité
/// d'arrière-plan (processeur, disque et mémoire : `THREAD_MODE_BACKGROUND_BEGIN`) puis enregistre l'instantané. N'ouvre
/// aucune page, n'affiche rien, ne supprime rien. Rend vrai si un fil est parti.
pub fn lancer_si_ancienne() -> bool {
    let Some(chemin) = chemin() else { return false };
    if !faut_relancer(lire_de(&chemin).map(|i| i.date), secondes(SystemTime::now())) {
        return false;
    }
    // Pas pendant un effacement : la mesure verrait des éléments en train de disparaître
    if nettoyage::suppression_en_cours() || EN_COURS.swap(true, Ordering::AcqRel) {
        return false;
    }
    let parti = std::thread::Builder::new()
        .name("analyse-auto".into())
        .spawn(move || {
            // SAFETY: le pseudo-handle du fil courant est toujours valide ; le mode d'arrière-plan ne vaut que pour ce fil
            unsafe {
                let _ = SetThreadPriority(GetCurrentThread(), THREAD_MODE_BACKGROUND_BEGIN);
            }
            let maintenant = SystemTime::now();
            let cats = nettoyage::mesurer_tout(&Racines::systeme(), maintenant, nettoyage::taille_corbeille);
            enregistrer_dans(&chemin, &de_mesure(&cats, maintenant));
            oublier_resume();
            EN_COURS.store(false, Ordering::Release);
        })
        .is_ok();
    if !parti {
        EN_COURS.store(false, Ordering::Release);
    }
    parti
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cat(id: &'static str, octets: u64) -> Categorie {
        Categorie { id, libelle: "x", risque: "faible", mode: nettoyage::Mode::Corbeille, octets, fichiers: 1, elements: 1, illisibles: 0, anormal: false, chemins: Vec::new() }
    }

    #[test]
    fn l_analyse_se_refait_a_partir_de_24_heures() {
        let t = 1_800_000_000;
        assert!(faut_relancer(None, t), "sans instantané : à faire");
        assert!(!faut_relancer(Some(t - 3600), t));
        assert!(!faut_relancer(Some(t - (DELAI_SECONDES - 1)), t), "23 h 59 : trop tôt");
        assert!(faut_relancer(Some(t - DELAI_SECONDES), t), "24 h pile : à refaire");
        assert!(faut_relancer(Some(t - 10 * 86_400), t));
    }

    #[test]
    fn une_date_dans_le_futur_force_l_analyse() {
        assert!(faut_relancer(Some(2_000_000_000), 1_800_000_000), "horloge reculée : sinon plus jamais d'analyse");
    }

    #[test]
    fn l_instantane_fait_l_aller_retour() {
        let i = de_mesure(&[cat("temp", 500), cat("d3d", 20)], UNIX_EPOCH + std::time::Duration::from_secs(1_800_000_000));
        assert_eq!(i.date, 1_800_000_000);
        assert_eq!(i.categories, vec![("temp".to_string(), 500), ("d3d".to_string(), 20)]);
        assert_eq!(decoder(&encoder(&i)), Some(i));
        let vide = Instantane { date: 5, categories: Vec::new() };
        assert_eq!(decoder(&encoder(&vide)), Some(vide), "une analyse sans rien à nettoyer est un résultat");
    }

    #[test]
    fn un_fichier_abime_ou_d_une_autre_version_est_ignore() {
        assert_eq!(decoder(""), None);
        assert_eq!(decoder("SpaceFiles-analyse 2\ndate 5\n"), None, "autre version");
        assert_eq!(decoder("SpaceFiles-analyse 1\ndate abc\n"), None);
        assert_eq!(decoder("SpaceFiles-analyse 1\ndate 5\ncat temp\n"), None, "octets manquants");
        assert_eq!(decoder("SpaceFiles-analyse 1\ndate 5\ncat temp 1 2\n"), None, "champ en trop");
        assert_eq!(decoder("SpaceFiles-analyse 1\ndate 5\ncat temp -3\n"), None, "octets négatifs");
        assert_eq!(decoder("SpaceFiles-analyse 1\ndate 5\nn'importe quoi\n"), None);
    }

    #[test]
    fn l_enregistrement_remplace_le_precedent_sans_laisser_de_fichier_voisin() {
        let d = std::env::temp_dir().join(format!("ne-instantane-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        let f = d.join("sous-dossier").join(NOM_FICHIER);
        assert_eq!(lire_de(&f), None, "rien d'enregistré");
        let a = Instantane { date: 100, categories: vec![("temp".into(), 7)] };
        let b = Instantane { date: 200, categories: vec![("temp".into(), 9), ("d3d".into(), 1)] };
        assert!(enregistrer_dans(&f, &a));
        assert_eq!(lire_de(&f), Some(a));
        assert!(enregistrer_dans(&f, &b));
        assert_eq!(lire_de(&f), Some(b));
        assert!(!f.with_extension("tmp").exists(), "le fichier voisin est renommé, pas laissé");
        let _ = std::fs::remove_dir_all(&d);
    }

    fn inst(date: u64, c: &[(&str, u64)]) -> Instantane {
        Instantane { date, categories: c.iter().map(|(i, o)| (i.to_string(), *o)).collect() }
    }

    #[test]
    fn deux_analyses_donnent_l_ecart_attendu() {
        let a = inst(1_000, &[("temp", 500), ("d3d", 20), ("vieux", 7)]);
        let b = inst(1_000 + 2 * 86_400, &[("temp", 800), ("d3d", 20), ("neuf", 100)]);
        let e = comparer(&a, &b);
        assert_eq!(e.depuis, 2 * 86_400);
        assert_eq!(e.delta, 800 + 20 + 100 - (500 + 20 + 7));
        // d3d est égal : omis ; tri par variation absolue décroissante
        assert_eq!(e.par_categorie, vec![("temp".to_string(), 300), ("neuf".to_string(), 100), ("vieux".to_string(), -7)]);
        assert_eq!(comparer(&b, &b).delta, 0);
        assert!(comparer(&b, &b).par_categorie.is_empty());
    }

    #[test]
    fn le_libelle_de_la_carte_dit_le_total_et_l_ecart() {
        let t = |o: u64| format!("{o} O");
        let a = inst(0, &[("temp", 500)]);
        let b = inst(3 * 86_400, &[("temp", 800)]);
        assert_eq!(libelle_carte(&b, None, t), "RÉCUPÉRABLE : 800 O", "première analyse : le total seul");
        assert_eq!(libelle_carte(&b, Some(&a), t), "RÉCUPÉRABLE : 800 O · +300 O DEPUIS 3 J");
        assert_eq!(libelle_carte(&a, Some(&b), t), "RÉCUPÉRABLE : 500 O · −300 O DEPUIS 1 MIN", "dates à l'envers : durée plancher");
        assert_eq!(libelle_carte(&b, Some(&b), t), "RÉCUPÉRABLE : 800 O", "sans changement : pas de « +0 »");
    }

    #[test]
    fn les_durees_courtes() {
        assert_eq!(duree_courte(0), "1 MIN");
        assert_eq!(duree_courte(59 * 60), "59 MIN");
        assert_eq!(duree_courte(3600), "1 H");
        assert_eq!(duree_courte(86_399), "23 H");
        assert_eq!(duree_courte(86_400), "1 J");
    }

    #[test]
    fn l_ancienne_analyse_devient_la_precedente_sauf_si_elle_est_abimee() {
        let d = std::env::temp_dir().join(format!("ne-instantane-prec-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        let f = d.join(NOM_FICHIER);
        let (a, b, c) = (inst(100, &[("temp", 1)]), inst(200, &[("temp", 2)]), inst(300, &[("temp", 3)]));
        assert!(enregistrer_dans(&f, &a));
        assert_eq!(lire_de(&precedent_de(&f)), None, "première analyse : pas de précédente");
        assert!(enregistrer_dans(&f, &b));
        assert_eq!(lire_de(&precedent_de(&f)), Some(a));
        // Fichier courant abîmé : la bonne précédente reste
        std::fs::write(&f, "n'importe quoi").unwrap();
        assert!(enregistrer_dans(&f, &c));
        assert_eq!(lire_de(&f), Some(c));
        assert_eq!(lire_de(&precedent_de(&f)).map(|i| i.date), Some(100), "l'abîmé n'a pas écrasé la précédente");
        let _ = std::fs::remove_dir_all(&d);
    }
}
