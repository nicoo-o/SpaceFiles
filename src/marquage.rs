// Marquage unifié (N-03) : un seul modèle pour ce qui s'appelait balises de couleur, étiquettes 1 à 3 et groupes.
// Un élément (fichier ou dossier) porte une « marque » : au plus une couleur (le point devant l'icône), au plus une
// étiquette (1 à 3 petits carrés après le nom) et autant de groupes nommés que l'on veut. Un groupe a un nom et une couleur.
// Donnée utilisateur à part des réglages : `%APPDATA%\SpaceFiles\marques.txt` (en-tête `#format=1`), lignes séparées par des
// tabulations : `G<TAB>couleur<TAB>NOM` (un groupe) et `M<TAB>chemin<TAB>couleur|-<TAB>étiquette|-<TAB>rang:NOM…` (un élément).
// Lecture tolérante (fichier absent ou abîmé = rien de marqué, ligne par ligne), écriture atomique, jamais de panique, aucun
// chemin dans le journal.
// Migration (une fois, au premier chargement sans `marques.txt`) : `etiquettes.txt` et les listes `balises`, `groupes` et
// `membres` des réglages sont fondus dans le nouveau fichier. Rien n'est effacé : `etiquettes.txt` est copié en
// `etiquettes.txt.avant-marquage` et les listes des réglages restent telles quelles (elles ne sont plus lues ensuite).
// La marque suit le fichier quand SpaceFiles le renomme ou le déplace (`suivre`) ; une copie est un autre fichier, elle ne
// l'hérite pas. Une étiquette ou une couleur d'un fichier disparu est oubliée au chargement, sur disque fixe seulement
// (aucune E/S réseau qui bloque) ; l'appartenance à un groupe reste (le fichier peut revenir de la corbeille).

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::SystemTime;

/// Plus grande étiquette : 3 carrés
pub const MAX_ETIQUETTE: u8 = 3;
/// Version du fichier (A-02) : un fichier annonçant mieux n'est jamais réécrit par cette version
const FORMAT: u32 = 1;
/// Taille maximale lue : un fichier plus gros est abîmé ou étranger, on repart de la migration plutôt que de tout charger
const TAILLE_MAX: u64 = 8 * 1024 * 1024;

/// Ce que porte un élément. `chemin` garde la casse d'origine (le nom affiché d'un élément de groupe en dépend).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Marque {
    pub chemin: String,
    pub balise: Option<char>,
    pub etiquette: Option<u8>,
    /// (rang d'entrée dans le groupe, nom) : le rang garde l'ordre d'ajout d'un groupe
    pub groupes: Vec<(u64, String)>,
}

impl Marque {
    fn vide(&self) -> bool {
        self.balise.is_none() && self.etiquette.is_none() && self.groupes.is_empty()
    }
}

/// Le modèle en mémoire : les groupes déclarés (nom, couleur) dans l'ordre de création, et la marque de chaque élément
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Modele {
    pub groupes: Vec<(String, char)>,
    pub marques: HashMap<String, Marque>,
    suivant: u64,
}

/// Clé d'un chemin : minuscules, séparateurs `\`, sans `\` final (sauf racine « c:\ »). Windows ne distingue ni la casse ni
/// `/` de `\` : deux écritures du même chemin ont la même clé.
pub fn cle(chemin: &str) -> String {
    let mut c: String = chemin.trim().chars().map(|x| if x == '/' { '\\' } else { x }).collect::<String>().to_lowercase();
    while c.len() > 3 && c.ends_with('\\') {
        c.pop();
    }
    c
}

/// Nom de groupe : sans `|` ni tabulation (séparateurs de fichier), en majuscules
pub fn normaliser(nom: &str) -> String {
    nom.trim().replace(['|', '\t', '\r', '\n'], " ").to_uppercase()
}

impl Modele {
    fn marque_de(&mut self, chemin: &str) -> &mut Marque {
        self.marques.entry(cle(chemin)).or_insert_with(|| Marque { chemin: chemin.trim().to_string(), ..Marque::default() })
    }

    fn nettoyer(&mut self) {
        self.marques.retain(|_, m| !m.vide());
    }

    /// Pose (ou retire, `None`) la couleur de balise sur tous ces chemins
    pub fn poser_balise(&mut self, chemins: &[String], b: Option<char>) {
        for c in chemins {
            if b.is_some() || self.marques.contains_key(&cle(c)) {
                self.marque_de(c).balise = b;
            }
        }
        self.nettoyer();
    }

    /// Pose l'étiquette `n` sur tous ces chemins, sauf si tous l'ont déjà : alors elle est retirée de tous (bascule).
    /// Une autre étiquette est remplacée. Rend vrai si posée.
    pub fn basculer_etiquette(&mut self, chemins: &[String], n: u8) -> bool {
        if !(1..=MAX_ETIQUETTE).contains(&n) {
            return false;
        }
        let toutes = !chemins.is_empty() && chemins.iter().all(|c| self.marques.get(&cle(c)).is_some_and(|m| m.etiquette == Some(n)));
        for c in chemins {
            if toutes {
                if let Some(m) = self.marques.get_mut(&cle(c)) {
                    m.etiquette = None;
                }
            } else {
                self.marque_de(c).etiquette = Some(n);
            }
        }
        self.nettoyer();
        !toutes
    }

    pub fn creer_groupe(&mut self, nom: &str, couleur: char) {
        if !self.groupes.iter().any(|(n, _)| n == nom) {
            self.groupes.push((nom.to_string(), couleur));
        }
    }

    /// Ajoute ces éléments au groupe (sans doublon, dans l'ordre donné). Rend le nombre d'ajouts.
    pub fn ajouter_au_groupe(&mut self, nom: &str, chemins: &[String]) -> usize {
        let mut n = 0;
        for c in chemins {
            if self.marques.get(&cle(c)).is_some_and(|m| m.groupes.iter().any(|(_, g)| g == nom)) {
                continue;
            }
            let rang = self.suivant;
            self.suivant += 1;
            self.marque_de(c).groupes.push((rang, nom.to_string()));
            n += 1;
        }
        n
    }

    pub fn retirer_du_groupe(&mut self, nom: &str, chemins: &[String]) {
        for c in chemins {
            if let Some(m) = self.marques.get_mut(&cle(c)) {
                m.groupes.retain(|(_, g)| g != nom);
            }
        }
        self.nettoyer();
    }

    pub fn supprimer_groupe(&mut self, nom: &str) {
        self.groupes.retain(|(n, _)| n != nom);
        for m in self.marques.values_mut() {
            m.groupes.retain(|(_, g)| g != nom);
        }
        self.nettoyer();
    }

    pub fn renommer_groupe(&mut self, ancien: &str, nouveau: &str) {
        for g in self.groupes.iter_mut().filter(|(n, _)| n == ancien) {
            g.0 = nouveau.to_string();
        }
        for m in self.marques.values_mut() {
            for g in m.groupes.iter_mut().filter(|(_, n)| n == ancien) {
                g.1 = nouveau.to_string();
            }
        }
    }

    pub fn colorer_groupe(&mut self, nom: &str, couleur: char) {
        for g in self.groupes.iter_mut().filter(|(n, _)| n == nom) {
            g.1 = couleur;
        }
    }

    /// Chemins (casse d'origine) des éléments d'un groupe, dans l'ordre où ils y sont entrés
    pub fn membres(&self, nom: &str) -> Vec<String> {
        let mut v: Vec<(u64, &str)> = self.marques.values().filter_map(|m| m.groupes.iter().find(|(_, g)| g == nom).map(|(r, _)| (*r, m.chemin.as_str()))).collect();
        v.sort();
        v.into_iter().map(|(_, c)| c.to_string()).collect()
    }

    /// Reporte la marque de chaque `ancien` (et, si c'est un dossier, de tout ce qu'il contient) sous son `nouveau`. Tout est lu
    /// avant d'être réécrit : un échange `a→b`, `b→a` (renommage par lot) ne s'écrase pas lui-même. Vrai si quelque chose a bougé.
    pub fn suivre(&mut self, paires: &[(String, String)]) -> bool {
        if self.marques.is_empty() {
            return false;
        }
        let paires: Vec<(String, String, String)> = paires.iter().map(|(a, n)| (cle(a), cle(n), n.trim().to_string())).filter(|(a, n, _)| a != n).collect();
        let mut deplaces: Vec<(String, String, Marque)> = Vec::new();
        for (c, m) in self.marques.iter() {
            for (a, n, n_brut) in &paires {
                let base = a.trim_end_matches('\\');
                if c == a || c.strip_prefix(base).is_some_and(|r| r.starts_with('\\')) {
                    let reste = &c[base.len()..];
                    let nouvelle_cle = format!("{}{}", n.trim_end_matches('\\'), reste);
                    // Casse d'origine : le nouveau chemin donné, prolongé de la fin du chemin d'avant (sa casse est gardée)
                    let fin = m.chemin.get(m.chemin.len().saturating_sub(reste.len())..).unwrap_or(reste);
                    let mut m2 = m.clone();
                    m2.chemin = format!("{}{}", n_brut.trim_end_matches('\\'), if reste.is_empty() { "" } else { fin });
                    deplaces.push((c.clone(), nouvelle_cle, m2));
                    break;
                }
            }
        }
        for (ancien, _, _) in &deplaces {
            self.marques.remove(ancien);
        }
        let bougees = !deplaces.is_empty();
        for (_, nouveau, m) in deplaces {
            self.marques.insert(nouveau, m);
        }
        bougees
    }

    /// Oublie la couleur et l'étiquette des éléments que `absent` dit disparus (les groupes restent) ; rend le nombre d'éléments touchés
    pub fn purger(&mut self, absent: &dyn Fn(&str) -> bool) -> usize {
        let mut n = 0;
        for (c, m) in self.marques.iter_mut() {
            if (m.balise.is_some() || m.etiquette.is_some()) && absent(c) {
                m.balise = None;
                m.etiquette = None;
                n += 1;
            }
        }
        self.nettoyer();
        n
    }

    /// Texte du fichier, trié : deux écritures du même contenu sont identiques
    pub fn ecrire(&self) -> String {
        let mut s = crate::atomique::en_tete_format(FORMAT);
        for (nom, c) in &self.groupes {
            s.push_str(&format!("G\t{c}\t{nom}\n"));
        }
        let mut cles: Vec<&String> = self.marques.keys().collect();
        cles.sort();
        for k in cles {
            let m = &self.marques[k];
            let mut g = m.groupes.clone();
            g.sort();
            let mut ligne = format!("M\t{}\t{}\t{}", m.chemin, m.balise.map_or("-".to_string(), |b| b.to_string()), m.etiquette.map_or("-".to_string(), |e| e.to_string()));
            for (r, n) in g {
                ligne.push_str(&format!("\t{r}:{n}"));
            }
            ligne.push('\n');
            s.push_str(&ligne);
        }
        s
    }

    /// Lit le texte du fichier : une ligne abîmée est ignorée seule
    pub fn lire(texte: &str) -> Modele {
        let mut mo = Modele::default();
        for ligne in texte.trim_start_matches('\u{feff}').lines() {
            let ligne = ligne.trim_end_matches('\r');
            let champs: Vec<&str> = ligne.split('\t').collect();
            match champs.first().copied() {
                Some("G") if champs.len() >= 3 => {
                    let nom = normaliser(champs[2]);
                    if let (Some(c), false) = (champs[1].chars().next(), nom.is_empty()) {
                        mo.creer_groupe(&nom, c);
                    }
                }
                Some("M") if champs.len() >= 4 && !champs[1].trim().is_empty() => {
                    let balise = champs[2].chars().next().filter(|c| c.is_ascii_alphabetic());
                    let etiquette = champs[3].parse::<u8>().ok().filter(|n| (1..=MAX_ETIQUETTE).contains(n));
                    let mut groupes = Vec::new();
                    for x in &champs[4.min(champs.len())..] {
                        if let Some((r, n)) = x.split_once(':') {
                            if let (Ok(r), false) = (r.parse::<u64>(), normaliser(n).is_empty()) {
                                groupes.push((r, normaliser(n)));
                                mo.suivant = mo.suivant.max(r + 1);
                            }
                        }
                    }
                    let m = Marque { chemin: champs[1].trim().to_string(), balise, etiquette, groupes };
                    if !m.vide() {
                        mo.marques.insert(cle(&m.chemin), m);
                    }
                }
                _ => {}
            }
        }
        mo
    }

    /// Migration depuis les trois anciennes données : étiquettes (« N|chemin »), balises (« c|chemin »), groupes (« couleur|NOM »)
    /// et membres (« NOM|chemin »). Tolérante : une ligne qu'on ne comprend pas est laissée de côté.
    pub fn migrer(etiquettes: &str, balises: &[String], groupes: &[String], membres: &[String]) -> Modele {
        let mut mo = Modele::default();
        for g in groupes {
            if let Some((c, nom)) = g.split_once('|') {
                let nom = normaliser(nom);
                if let (Some(c), false) = (c.chars().next(), nom.is_empty()) {
                    mo.creer_groupe(&nom, c);
                }
            }
        }
        for m in membres {
            if let Some((nom, chemin)) = m.split_once('|') {
                let nom = normaliser(nom);
                if !nom.is_empty() && !chemin.trim().is_empty() {
                    mo.ajouter_au_groupe(&nom, &[chemin.to_string()]);
                }
            }
        }
        for l in balises {
            if let Some((c, chemin)) = l.split_once('|') {
                if let Some(c) = c.chars().next().filter(|c| c.is_ascii_alphabetic()) {
                    if !chemin.trim().is_empty() {
                        mo.marque_de(chemin).balise = Some(c);
                    }
                }
            }
        }
        for ligne in etiquettes.trim_start_matches('\u{feff}').lines() {
            let Some((n, chemin)) = ligne.trim_end_matches('\r').split_once('|') else { continue };
            let Ok(n) = n.trim().parse::<u8>() else { continue };
            if (1..=MAX_ETIQUETTE).contains(&n) && !chemin.trim().is_empty() {
                mo.marque_de(chemin).etiquette = Some(n);
            }
        }
        mo.nettoyer();
        mo
    }
}

// ───────────────────────── État partagé et fichier ─────────────────────────

struct Etat {
    modele: Modele,
    /// Date du fichier au dernier chargement ou à la dernière écriture : une autre fenêtre l'a-t-elle changé depuis ?
    vu: Option<SystemTime>,
}

static ETAT: Mutex<Option<Etat>> = Mutex::new(None);

fn chemin_fichier() -> Option<std::path::PathBuf> {
    Some(crate::reglages::dossier_donnees(false)?.join("marques.txt"))
}

fn date_du_fichier() -> Option<SystemTime> {
    std::fs::metadata(chemin_fichier()?).and_then(|m| m.modified()).ok()
}

/// Disque fixe (type 3) : seul endroit où l'on teste l'existence d'un fichier au chargement. Réseau, amovible, disque absent :
/// on garde la marque, un accès pourrait bloquer.
fn disque_fixe(cle: &str) -> bool {
    let b = cle.as_bytes();
    if b.len() < 3 || b[1] != b':' || !b[0].is_ascii_alphabetic() {
        return false;
    }
    let racine: Vec<u16> = format!("{}:\\", b[0] as char).encode_utf16().chain(std::iter::once(0)).collect();
    // SAFETY: `racine` est une chaîne UTF-16 terminée par zéro qui vit pendant tout l'appel
    unsafe { windows::Win32::Storage::FileSystem::GetDriveTypeW(windows::core::PCWSTR(racine.as_ptr())) == 3 }
}

fn disparu(cle: &str) -> bool {
    disque_fixe(cle) && !std::path::Path::new(cle).exists()
}

/// Lit `marques.txt`, ou le crée par migration. Aucun accès disque pendant les tests.
fn charger_depuis_disque() -> Modele {
    if cfg!(test) {
        return Modele::default();
    }
    let Some(p) = chemin_fichier() else { return Modele::default() };
    let existe = p.exists();
    if existe {
        let lisible = std::fs::metadata(&p).is_ok_and(|m| m.len() <= TAILLE_MAX);
        if lisible {
            if let Ok(octets) = std::fs::read(&p) {
                let mut mo = Modele::lire(&String::from_utf8_lossy(&octets));
                if mo.purger(&disparu) > 0 {
                    sauver(&mo);
                }
                return mo;
            }
        }
        // Fichier illisible ou trop gros : on le met de côté, jamais écrasé sans trace
        let _ = std::fs::rename(&p, p.with_extension("txt.abime"));
    }
    migrer_depuis_anciennes_donnees(&p)
}

fn migrer_depuis_anciennes_donnees(p: &std::path::Path) -> Modele {
    let anciennes = p.with_file_name("etiquettes.txt");
    let etiquettes = match std::fs::metadata(&anciennes) {
        Ok(m) if m.len() <= TAILLE_MAX => std::fs::read(&anciennes).map(|o| String::from_utf8_lossy(&o).into_owned()).unwrap_or_default(),
        _ => String::new(),
    };
    let r = crate::reglages::actuels();
    let mut mo = Modele::migrer(&etiquettes, &r.balises, &r.groupes, &r.membres);
    mo.purger(&disparu);
    // Sauvegarde de l'ancien fichier d'étiquettes, puis écriture du nouveau : si l'écriture échoue, l'ancien reste en place
    if !etiquettes.is_empty() {
        let _ = std::fs::copy(&anciennes, p.with_file_name("etiquettes.txt.avant-marquage"));
    }
    sauver(&mo);
    mo
}

/// Écriture atomique : un arrêt en cours d'écriture laisse l'ancien contenu
fn sauver(mo: &Modele) {
    if cfg!(test) {
        return;
    }
    let Some(p) = chemin_fichier() else { return };
    if crate::atomique::plus_recent_que_connu(&p, FORMAT, "marques") {
        return;
    }
    crate::atomique::ecrire_journalise(&p, mo.ecrire().as_bytes(), "marques");
}

fn etat(g: &mut Option<Etat>) -> &mut Etat {
    g.get_or_insert_with(|| Etat { modele: charger_depuis_disque(), vu: date_du_fichier() })
}

/// Lecture : le modèle tel qu'il est en mémoire (une seule prise du verrou, aucun accès disque)
pub fn lire<R>(f: impl FnOnce(&Modele) -> R) -> R {
    let mut g = ETAT.lock().unwrap_or_else(|e| e.into_inner());
    f(&etat(&mut g).modele)
}

/// Modification : si une autre fenêtre a réécrit le fichier depuis, on repart de sa version avant d'appliquer le changement
/// (sinon la dernière à écrire effacerait les marques posées par l'autre) ; puis écriture atomique.
pub fn modifier<R>(f: impl FnOnce(&mut Modele) -> R) -> R {
    let mut g = ETAT.lock().unwrap_or_else(|e| e.into_inner());
    let e = etat(&mut g);
    // Relecture à chaque modification (fichier de quelques ko, écritures rares) : comparer seulement la date laissait passer
    // deux écritures dans la même granularité d'horloge de deux fenêtres, la seconde effaçant la première
    if !cfg!(test) {
        if let Some(p) = chemin_fichier() {
            if std::fs::metadata(&p).is_ok_and(|m| m.len() <= TAILLE_MAX) {
                if let Ok(octets) = std::fs::read(&p) {
                    e.modele = Modele::lire(&String::from_utf8_lossy(&octets));
                }
            }
        }
    }
    let r = f(&mut e.modele);
    sauver(&e.modele);
    e.vu = date_du_fichier();
    r
}

#[cfg(test)]
mod tests {
    use super::*;

    fn k(s: &str) -> String {
        cle(s)
    }

    fn v(s: &[&str]) -> Vec<String> {
        s.iter().map(|x| x.to_string()).collect()
    }

    fn p(a: &str, b: &str) -> Vec<(String, String)> {
        vec![(a.to_string(), b.to_string())]
    }

    #[test]
    fn le_fichier_porte_sa_version_et_se_relit() {
        let mut m = Modele::default();
        m.basculer_etiquette(&v(&[r"C:\a\b.txt"]), 2);
        let texte = m.ecrire();
        assert!(texte.starts_with("#format=1\r\n"));
        assert_eq!(Modele::lire(&texte), m, "l'en-tête n'est pas lu comme une marque");
        assert_eq!(Modele::default().ecrire(), "#format=1\r\n");
    }

    #[test]
    fn la_cle_ignore_la_casse_les_barres_et_le_dernier_separateur() {
        assert_eq!(cle(r"C:\Docs\Rapport.PDF"), cle("c:/docs/rapport.pdf"));
        assert_eq!(cle(r"C:\Docs\Dossier\"), cle(r"c:\docs\dossier"));
        assert_eq!(cle(r"C:\"), r"c:\");
    }

    #[test]
    fn ecriture_puis_lecture_rendent_le_meme_modele() {
        let mut m = Modele::default();
        m.creer_groupe("PROJET X", 'p');
        m.creer_groupe("À LIRE ¦ É", 'u');
        m.poser_balise(&v(&[r"C:\a\un.txt"]), Some('r'));
        m.basculer_etiquette(&v(&[r"D:\b\deux ¦ é.txt"]), 3);
        m.ajouter_au_groupe("PROJET X", &v(&[r"C:\a\un.txt", r"D:\b\deux ¦ é.txt"]));
        m.ajouter_au_groupe("À LIRE ¦ É", &v(&[r"C:\a\un.txt"]));
        let texte = m.ecrire();
        let relu = Modele::lire(&texte);
        assert_eq!(relu, m);
        assert_eq!(relu.ecrire(), texte);
        assert_eq!(relu.membres("PROJET X"), v(&[r"C:\a\un.txt", r"D:\b\deux ¦ é.txt"]), "l'ordre d'ajout est gardé, la casse d'origine aussi");
    }

    #[test]
    fn fichier_abime_ou_vide_ne_donne_que_les_lignes_valides() {
        assert_eq!(Modele::lire(""), Modele::default());
        assert!(Modele::lire("n'importe quoi\n\nM\n\nM\t\t-\t1\nM\tc:\\x\t-\t9\nG\t\tx\nG\tr\t\n").marques.is_empty());
        let m = Modele::lire("\u{feff}#format=1\r\nG\tr\trouge\r\nM\tC:\\A\\b.txt\tr\t1\t0:ROUGE\r\nfoo\r\nM\tc:/c/d.txt\t-\t3\r\nM\tc:\\vide\t-\t-\r\n");
        assert_eq!(m.groupes, vec![("ROUGE".to_string(), 'r')]);
        assert_eq!(m.marques.len(), 2, "la ligne sans aucune marque n'est pas gardée");
        assert_eq!(m.marques[&k(r"c:\a\b.txt")].balise, Some('r'));
        assert_eq!(m.marques[&k(r"c:\c\d.txt")].etiquette, Some(3));
        // Octets invalides : lus avec perte comme dans `charger`, sans panique
        assert!(Modele::lire(&String::from_utf8_lossy(&[0xff, 0xfe, 0x00, b'\t', 0x80])).marques.is_empty());
    }

    #[test]
    fn la_meme_etiquette_bascule_et_une_autre_remplace() {
        let mut m = Modele::default();
        let a = v(&[r"C:\a.txt"]);
        assert!(m.basculer_etiquette(&a, 2));
        assert_eq!(m.marques[&k(&a[0])].etiquette, Some(2));
        assert!(m.basculer_etiquette(&a, 3));
        assert_eq!(m.marques[&k(&a[0])].etiquette, Some(3));
        assert!(!m.basculer_etiquette(&a, 3));
        assert!(m.marques.is_empty(), "une marque vide ne reste pas");
        assert!(!m.basculer_etiquette(&a, 4), "hors de 1 à 3 : refusée");
        assert!(m.marques.is_empty());
    }

    #[test]
    fn plusieurs_elements_tous_etiquetes_puis_tous_liberes() {
        let mut m = Modele::default();
        let tous = v(&[r"C:\a", r"C:\b", r"C:\c"]);
        m.basculer_etiquette(&tous[..1], 1);
        assert!(m.basculer_etiquette(&tous, 1), "un seul l'avait : on pose sur tous");
        assert!(tous.iter().all(|c| m.marques[&k(c)].etiquette == Some(1)));
        assert!(!m.basculer_etiquette(&tous, 1), "tous l'ont : on retire de tous");
        assert!(m.marques.is_empty());
    }

    #[test]
    fn balise_etiquette_et_groupes_coexistent_et_se_retirent_separement() {
        let mut m = Modele::default();
        let a = v(&[r"C:\a.txt"]);
        m.poser_balise(&a, Some('b'));
        m.basculer_etiquette(&a, 2);
        m.ajouter_au_groupe("X", &a);
        m.ajouter_au_groupe("Y", &a);
        assert_eq!(m.ajouter_au_groupe("X", &a), 0, "pas de doublon");
        let marque = &m.marques[&k(&a[0])];
        assert_eq!((marque.balise, marque.etiquette, marque.groupes.len()), (Some('b'), Some(2), 2));
        m.poser_balise(&a, None);
        m.retirer_du_groupe("X", &a);
        let marque = &m.marques[&k(&a[0])];
        assert_eq!((marque.balise, marque.etiquette, marque.groupes.len()), (None, Some(2), 1));
        m.basculer_etiquette(&a, 2);
        m.retirer_du_groupe("Y", &a);
        assert!(m.marques.is_empty());
        // Retirer la balise d'un élément jamais marqué ne crée rien
        m.poser_balise(&v(&[r"C:\z.txt"]), None);
        assert!(m.marques.is_empty());
    }

    #[test]
    fn groupes_renommer_colorer_supprimer() {
        let mut m = Modele::default();
        m.creer_groupe("A", 'p');
        m.creer_groupe("A", 'r');
        assert_eq!(m.groupes.len(), 1, "créer deux fois ne duplique pas");
        m.ajouter_au_groupe("A", &v(&[r"C:\x", r"C:\y"]));
        m.renommer_groupe("A", "B");
        assert_eq!(m.groupes, vec![("B".to_string(), 'p')]);
        assert_eq!(m.membres("B"), v(&[r"C:\x", r"C:\y"]));
        assert!(m.membres("A").is_empty());
        m.colorer_groupe("B", 'v');
        assert_eq!(m.groupes[0].1, 'v');
        m.supprimer_groupe("B");
        assert!(m.groupes.is_empty() && m.marques.is_empty());
    }

    #[test]
    fn la_marque_suit_un_renommage_et_un_deplacement_avec_la_casse() {
        let mut m = Modele::default();
        m.basculer_etiquette(&v(&[r"C:\Docs\a.txt"]), 2);
        m.ajouter_au_groupe("G", &v(&[r"C:\Docs\a.txt"]));
        m.poser_balise(&v(&[r"C:\Docs\a.txt"]), Some('r'));
        assert!(m.suivre(&p(r"C:\Docs\a.txt", r"C:\Docs\B.txt")));
        let mq = &m.marques[&k(r"c:\docs\b.txt")];
        assert_eq!((mq.etiquette, mq.balise, mq.groupes.len()), (Some(2), Some('r'), 1), "les trois marques suivent ensemble");
        assert_eq!(mq.chemin, r"C:\Docs\B.txt", "la casse du nouveau nom est gardée");
        assert!(!m.marques.contains_key(&k(r"c:\docs\a.txt")));
        // Ctrl+Z est un déplacement en sens inverse
        assert!(m.suivre(&p(r"C:\Docs\B.txt", r"D:\Archives\a.txt")));
        assert!(m.suivre(&p(r"D:\Archives\a.txt", r"C:\Docs\a.txt")));
        assert_eq!(m.marques.len(), 1);
        assert_eq!(m.membres("G"), v(&[r"C:\Docs\a.txt"]));
        // Un fichier sans marque ne crée rien
        assert!(!m.suivre(&p(r"C:\Docs\z.txt", r"C:\Docs\y.txt")));
        assert_eq!(m.marques.len(), 1);
    }

    #[test]
    fn un_echange_par_lot_ne_s_ecrase_pas() {
        let mut m = Modele::default();
        m.basculer_etiquette(&v(&[r"C:\d\a.txt"]), 1);
        m.basculer_etiquette(&v(&[r"C:\d\b.txt"]), 2);
        let paires = vec![(r"C:\d\a.txt".to_string(), r"C:\d\b.txt".to_string()), (r"C:\d\b.txt".to_string(), r"C:\d\a.txt".to_string())];
        assert!(m.suivre(&paires));
        assert_eq!(m.marques[&k(r"c:\d\a.txt")].etiquette, Some(2));
        assert_eq!(m.marques[&k(r"c:\d\b.txt")].etiquette, Some(1));
    }

    #[test]
    fn renommer_un_dossier_emmene_ce_qu_il_contient_et_rien_d_autre() {
        let mut m = Modele::default();
        m.basculer_etiquette(&v(&[r"C:\Projets\Vieux"]), 1);
        m.basculer_etiquette(&v(&[r"C:\Projets\Vieux\Sous\f.txt"]), 2);
        m.basculer_etiquette(&v(&[r"C:\Projets\VieuxAutre\f.txt"]), 3);
        assert!(m.suivre(&p(r"C:\Projets\Vieux", r"C:\Projets\Neuf")));
        assert_eq!(m.marques[&k(r"c:\projets\neuf")].etiquette, Some(1));
        assert_eq!(m.marques[&k(r"c:\projets\neuf\sous\f.txt")].etiquette, Some(2));
        assert_eq!(m.marques[&k(r"c:\projets\vieuxautre\f.txt")].etiquette, Some(3));
        assert_eq!(m.marques[&k(r"c:\projets\neuf\sous\f.txt")].chemin, r"C:\Projets\Neuf\Sous\f.txt", "la casse de l'intérieur est gardée");
        assert_eq!(m.marques.len(), 3);
    }

    #[test]
    fn purge_des_fichiers_disparus_sans_toucher_aux_groupes() {
        let mut m = Modele::default();
        m.basculer_etiquette(&v(&[r"C:\la.txt", r"C:\partie.txt", r"\\serveur\partage\x.txt"]), 1);
        m.ajouter_au_groupe("G", &v(&[r"C:\partie.txt"]));
        // Le prédicat réel ne dit « disparu » que pour un disque fixe : le réseau n'est jamais interrogé
        assert!(!disparu(&k(r"\\serveur\partage\x.txt")));
        assert!(!disparu("pas un chemin"));
        assert_eq!(m.purger(&|c| c.ends_with("partie.txt")), 1);
        assert_eq!(m.marques.len(), 3, "l'appartenance au groupe garde l'élément");
        assert_eq!(m.marques[&k(r"c:\partie.txt")].etiquette, None);
        assert_eq!(m.marques[&k(r"c:\partie.txt")].groupes.len(), 1);
    }

    #[test]
    fn migration_des_trois_anciennes_donnees() {
        let etiquettes = "#format=1\r\n2|c:\\docs\\a.txt\r\n3|c:\\autre\\b.txt\r\n";
        let balises = v(&[r"r|c:\docs\a.txt", r"g|d:\x.txt"]);
        let groupes = v(&["p|PROJET X", "u|VIDE"]);
        let membres = v(&[r"PROJET X|C:\Docs\A.txt", r"PROJET X|E:\z.txt"]);
        let m = Modele::migrer(etiquettes, &balises, &groupes, &membres);
        assert_eq!(m.groupes, vec![("PROJET X".to_string(), 'p'), ("VIDE".to_string(), 'u')], "un groupe vide survit");
        // Le même fichier porte maintenant ses trois marques sur une seule entrée
        let a = &m.marques[&k(r"c:\docs\a.txt")];
        assert_eq!((a.balise, a.etiquette, a.groupes.len()), (Some('r'), Some(2), 1));
        assert_eq!(a.chemin, r"C:\Docs\A.txt", "la casse d'origine vient du groupe");
        assert_eq!(m.marques[&k(r"d:\x.txt")].balise, Some('g'));
        assert_eq!(m.marques[&k(r"c:\autre\b.txt")].etiquette, Some(3));
        assert_eq!(m.membres("PROJET X"), v(&[r"C:\Docs\A.txt", r"E:\z.txt"]), "l'ordre des membres est gardé");
        assert_eq!(m.marques.len(), 4);
        // Les nouvelles données se relisent à l'identique
        assert_eq!(Modele::lire(&m.ecrire()), m);
    }

    #[test]
    fn migration_tolerante_aux_donnees_abimees() {
        let m = Modele::migrer("n'importe quoi\n9|c:\\x\n0|c:\\y\n2|\n|\n", &v(&["rien", "|c:\\a", "r|", "1|c:\\b"]), &v(&["sans barre", "|SANS COULEUR", "r|"]), &v(&["x", "|c:\\q", "G|", "G|  "]));
        assert!(m.marques.is_empty(), "{:?}", m.marques);
        assert!(m.groupes.is_empty(), "{:?}", m.groupes);
        assert_eq!(Modele::migrer("", &[], &[], &[]), Modele::default());
    }

    #[test]
    fn noms_de_groupe_sans_separateur_de_fichier() {
        assert_eq!(normaliser("  a|b\tc "), "A B C");
        let mut m = Modele::default();
        m.creer_groupe(&normaliser("x\ty"), 'r');
        m.ajouter_au_groupe(&normaliser("x\ty"), &v(&[r"C:\f"]));
        assert_eq!(Modele::lire(&m.ecrire()), m);
    }
}

/// Nom du fichier écrit à côté de l'export des réglages : groupes, balises et étiquettes vivent dans `marques.txt`, pas dans le TOML
pub const FICHIER_EXPORT: &str = "marques-spacefiles.txt";

/// Copie les marques dans `dossier` (écriture atomique) ; renvoie le fichier écrit
pub fn exporter(dossier: &std::path::Path) -> Result<std::path::PathBuf, String> {
    let c = dossier.join(FICHIER_EXPORT);
    let texte = lire(|m| m.ecrire());
    crate::atomique::ecrire(&c, texte.as_bytes()).map_err(|e| format!("EXPORT DES MARQUES IMPOSSIBLE : {e}"))?;
    Ok(c)
}

/// Remplace les marques par celles du fichier ; `Ok(false)` s'il n'existe pas (export d'avant, rien à importer)
pub fn importer(fichier: &std::path::Path) -> Result<bool, String> {
    let meta = match std::fs::metadata(fichier) {
        Ok(m) => m,
        Err(_) => return Ok(false),
    };
    if meta.len() > TAILLE_MAX {
        return Err("FICHIER DE MARQUES TROP GROS".into());
    }
    let octets = std::fs::read(fichier).map_err(|e| format!("LECTURE DES MARQUES IMPOSSIBLE : {e}"))?;
    let neuf = Modele::lire(&String::from_utf8_lossy(&octets));
    modifier(|m| *m = neuf);
    Ok(true)
}
