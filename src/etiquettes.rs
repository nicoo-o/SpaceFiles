// Étiquettes (lot 16, maquette `.sq` / `tagSel`) : 1, 2 ou 3 petits carrés après le nom d'un fichier ou d'un dossier.
// Donnée utilisateur à part des réglages : `%APPDATA%\SpaceFiles\etiquettes.txt`, une ligne « N|chemin » (N de 1 à 3, chemin
// en minuscules, séparateurs `\`). Lecture tolérante (fichier absent ou abîmé = aucune étiquette), écriture atomique (fichier
// voisin puis renommage), jamais de panique, aucun chemin dans le journal.
// L'étiquette suit le fichier quand SpaceFiles le renomme ou le déplace (`suivre`) ; une copie est un autre fichier, elle ne
// l'hérite pas. Un fichier disparu est oublié au chargement, sur disque fixe seulement (aucune E/S réseau qui bloque).

use std::collections::HashMap;
use std::sync::Mutex;

/// Plus grande étiquette : 3 carrés
pub const MAX: u8 = 3;
/// Taille maximale lue : un fichier plus gros est abîmé ou étranger, on repart de zéro plutôt que de tout charger
const TAILLE_MAX: u64 = 8 * 1024 * 1024;

type Carte = HashMap<String, u8>;

/// Carte chargée à la première utilisation (`None` : pas encore lue)
static ETAT: Mutex<Option<Carte>> = Mutex::new(None);

/// Clé d'un chemin : minuscules, séparateurs `\`, sans `\` final (sauf racine « c:\ »). Windows ne distingue ni la casse ni
/// `/` de `\` : deux écritures du même chemin ont la même clé.
pub fn cle(chemin: &str) -> String {
    let mut c: String = chemin.trim().chars().map(|x| if x == '/' { '\\' } else { x }).collect::<String>().to_lowercase();
    while c.len() > 3 && c.ends_with('\\') {
        c.pop();
    }
    c
}

/// Lit le texte du fichier : les lignes abîmées (sans `|`, N hors de 1 à 3, chemin vide) sont ignorées une à une
fn lire(texte: &str) -> Carte {
    let mut m = Carte::new();
    for ligne in texte.trim_start_matches('\u{feff}').lines() {
        let Some((n, chemin)) = ligne.trim_end_matches('\r').split_once('|') else { continue };
        let Ok(n) = n.trim().parse::<u8>() else { continue };
        if !(1..=MAX).contains(&n) || chemin.trim().is_empty() {
            continue;
        }
        m.insert(cle(chemin), n);
    }
    m
}

/// Texte du fichier, trié par chemin : deux écritures du même contenu sont identiques
fn ecrire(m: &Carte) -> String {
    let mut lignes: Vec<String> = m.iter().map(|(c, n)| format!("{n}|{c}")).collect();
    lignes.sort_by(|a, b| a[2..].cmp(&b[2..]));
    let mut s = lignes.join("\n");
    if !s.is_empty() {
        s.push('\n');
    }
    s
}

/// Retire les chemins que `absent` dit disparus ; rend le nombre d'oubliés
fn purger(m: &mut Carte, absent: &dyn Fn(&str) -> bool) -> usize {
    let avant = m.len();
    m.retain(|c, _| !absent(c));
    avant - m.len()
}

/// Pose `n` sur toutes les clés, sauf si toutes l'ont déjà : alors elle est retirée de toutes (bascule). Rend vrai si posée.
fn basculer_dans(m: &mut Carte, cles: &[String], n: u8) -> bool {
    let toutes = !cles.is_empty() && cles.iter().all(|c| m.get(c) == Some(&n));
    for c in cles {
        if toutes {
            m.remove(c);
        } else {
            m.insert(c.clone(), n);
        }
    }
    !toutes
}

/// Reporte les étiquettes de chaque `ancien` (et, si c'est un dossier, de tout ce qu'il contient) sous son `nouveau`. Tout est lu
/// avant d'être réécrit : un échange `a→b`, `b→a` (renommage par lot) ne s'écrase pas lui-même. Vrai si quelque chose a bougé.
fn suivre_dans(m: &mut Carte, paires: &[(String, String)]) -> bool {
    if m.is_empty() {
        return false;
    }
    let paires: Vec<(String, String)> = paires.iter().map(|(a, n)| (cle(a), cle(n))).filter(|(a, n)| a != n).collect();
    let mut deplaces: Vec<(String, String, u8)> = Vec::new();
    for (c, v) in m.iter() {
        for (a, n) in &paires {
            let base = a.trim_end_matches('\\');
            if c == a || c.strip_prefix(base).is_some_and(|r| r.starts_with('\\')) {
                deplaces.push((c.clone(), format!("{}{}", n.trim_end_matches('\\'), &c[base.len()..]), *v));
                break;
            }
        }
    }
    for (ancien, _, _) in &deplaces {
        m.remove(ancien);
    }
    for (_, nouveau, v) in &deplaces {
        m.insert(nouveau.clone(), *v);
    }
    !deplaces.is_empty()
}

fn chemin_fichier() -> Option<std::path::PathBuf> {
    Some(crate::reglages::dossier_donnees(false)?.join("etiquettes.txt"))
}

/// Disque fixe (type 3) : seul endroit où l'on teste l'existence d'un fichier au chargement. Réseau, amovible, disque absent :
/// on garde l'étiquette, un accès pourrait bloquer.
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

/// Lit le fichier (et oublie les fichiers disparus) à la première utilisation. Aucun accès disque pendant les tests.
fn charger(g: &mut Option<Carte>) -> &mut Carte {
    g.get_or_insert_with(|| {
        if cfg!(test) {
            return Carte::new();
        }
        let Some(p) = chemin_fichier() else { return Carte::new() };
        if std::fs::metadata(&p).map_or(true, |m| m.len() > TAILLE_MAX) {
            return Carte::new();
        }
        let Ok(octets) = std::fs::read(&p) else { return Carte::new() };
        let mut m = lire(&String::from_utf8_lossy(&octets));
        if purger(&mut m, &disparu) > 0 {
            sauver(&m);
        }
        m
    })
}

/// Écriture atomique : un arrêt en cours d'écriture laisse l'ancien contenu
fn sauver(m: &Carte) {
    if cfg!(test) {
        return;
    }
    let Some(p) = chemin_fichier() else { return };
    if let Some(d) = p.parent() {
        let _ = std::fs::create_dir_all(d);
    }
    let tmp = p.with_extension("txt.tmp");
    if std::fs::write(&tmp, ecrire(m)).is_ok() && std::fs::rename(&tmp, &p).is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
}

fn avec<R>(f: impl FnOnce(&mut Carte) -> R) -> R {
    let mut g = ETAT.lock().unwrap_or_else(|e| e.into_inner());
    f(charger(&mut g))
}

/// Étiquette (1 à 3) de ce chemin
pub fn de(chemin: &str) -> Option<u8> {
    avec(|m| if m.is_empty() { None } else { m.get(&cle(chemin)).copied() })
}

/// Raccourci pour le dessin : une seule prise du verrou pour savoir s'il y a la moindre étiquette
pub fn existe_une() -> bool {
    avec(|m| !m.is_empty())
}

/// Pose l'étiquette `n` sur ces chemins, ou la retire si tous l'ont déjà. Rend vrai si elle a été posée.
pub fn basculer(chemins: &[String], n: u8) -> bool {
    if !(1..=MAX).contains(&n) {
        return false;
    }
    let cles: Vec<String> = chemins.iter().map(|c| cle(c)).collect();
    avec(|m| {
        let pose = basculer_dans(m, &cles, n);
        sauver(m);
        pose
    })
}

/// L'étiquette suit un renommage ou un déplacement fait par SpaceFiles (dossier : ce qu'il contient aussi)
pub fn suivre(ancien: &str, nouveau: &str) {
    suivre_paires(&[(ancien.to_string(), nouveau.to_string())]);
}

/// Plusieurs renommages ou déplacements d'un coup (lot, déplacement de plusieurs éléments)
pub fn suivre_paires(paires: &[(String, String)]) {
    avec(|m| {
        if suivre_dans(m, paires) {
            sauver(m);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn k(s: &str) -> String {
        cle(s)
    }

    fn p(a: &str, b: &str) -> Vec<(String, String)> {
        vec![(a.to_string(), b.to_string())]
    }

    #[test]
    fn un_echange_par_lot_ne_s_ecrase_pas() {
        let mut m = Carte::new();
        m.insert(k(r"C:\d\a.txt"), 1);
        m.insert(k(r"C:\d\b.txt"), 2);
        let paires = vec![(r"C:\d\a.txt".to_string(), r"C:\d\b.txt".to_string()), (r"C:\d\b.txt".to_string(), r"C:\d\a.txt".to_string())];
        assert!(suivre_dans(&mut m, &paires));
        assert_eq!(m.get(&k(r"c:\d\a.txt")), Some(&2));
        assert_eq!(m.get(&k(r"c:\d\b.txt")), Some(&1));
    }

    #[test]
    fn la_cle_ignore_la_casse_les_barres_et_le_dernier_separateur() {
        assert_eq!(cle(r"C:\Docs\Rapport.PDF"), cle("c:/docs/rapport.pdf"));
        assert_eq!(cle(r"C:\Docs\Dossier\"), cle(r"c:\docs\dossier"));
        assert_eq!(cle(r"C:\"), r"c:\");
    }

    #[test]
    fn ecriture_puis_lecture_rendent_la_meme_carte() {
        let mut m = Carte::new();
        m.insert(k(r"C:\a\un.txt"), 1);
        m.insert(k(r"D:\b\deux ¦ é.txt"), 3);
        m.insert(k(r"C:\a\trois|barre.txt"), 2);
        let texte = ecrire(&m);
        assert_eq!(lire(&texte), m);
        assert_eq!(ecrire(&lire(&texte)), texte);
    }

    #[test]
    fn fichier_abime_ou_vide_ne_donne_que_les_lignes_valides() {
        assert!(lire("").is_empty());
        assert!(lire("n'importe quoi\n\n|\n9|c:\\x\n0|c:\\y\n-1|c:\\z\n2|\n").is_empty());
        let m = lire("\u{feff}1|C:\\A\\b.txt\r\nfoo\r\n3|c:/c/d.txt\r\n2|\r\n");
        assert_eq!(m.len(), 2);
        assert_eq!(m.get(&k(r"c:\a\b.txt")), Some(&1));
        assert_eq!(m.get(&k(r"c:\c\d.txt")), Some(&3));
        // Octets invalides : lus avec perte comme dans `charger`, sans panique
        assert!(lire(&String::from_utf8_lossy(&[0xff, 0xfe, 0x00, b'|', 0x80])).is_empty());
    }

    #[test]
    fn la_meme_touche_bascule() {
        let mut m = Carte::new();
        let a = vec![k(r"C:\a.txt")];
        assert!(basculer_dans(&mut m, &a, 2));
        assert_eq!(m.get(&a[0]), Some(&2));
        // Une autre étiquette remplace
        assert!(basculer_dans(&mut m, &a, 3));
        assert_eq!(m.get(&a[0]), Some(&3));
        // La même la retire
        assert!(!basculer_dans(&mut m, &a, 3));
        assert!(m.is_empty());
    }

    #[test]
    fn plusieurs_elements_tous_etiquetes_puis_tous_liberes() {
        let mut m = Carte::new();
        let tous = vec![k(r"C:\a"), k(r"C:\b"), k(r"C:\c")];
        m.insert(tous[0].clone(), 1);
        // Un seul l'avait : on pose sur tous
        assert!(basculer_dans(&mut m, &tous, 1));
        assert!(tous.iter().all(|c| m.get(c) == Some(&1)));
        // Tous l'ont : on retire de tous
        assert!(!basculer_dans(&mut m, &tous, 1));
        assert!(m.is_empty());
        // Rien de choisi : rien ne change
        assert!(basculer_dans(&mut m, &[], 1) || m.is_empty());
    }

    #[test]
    fn l_etiquette_suit_un_renommage() {
        let mut m = Carte::new();
        m.insert(k(r"C:\Docs\a.txt"), 2);
        assert!(suivre_dans(&mut m, &p(r"C:\Docs\a.txt", r"C:\Docs\b.txt")));
        assert_eq!(m.get(&k(r"c:\docs\b.txt")), Some(&2));
        assert!(!m.contains_key(&k(r"c:\docs\a.txt")));
        // Un fichier sans étiquette ne crée rien
        assert!(!suivre_dans(&mut m, &p(r"C:\Docs\z.txt", r"C:\Docs\y.txt")));
        assert_eq!(m.len(), 1);
    }

    #[test]
    fn l_etiquette_suit_un_deplacement_et_son_annulation() {
        let mut m = Carte::new();
        m.insert(k(r"C:\Docs\a.txt"), 3);
        assert!(suivre_dans(&mut m, &p(r"C:\Docs\a.txt", r"D:\Archives\a.txt")));
        assert_eq!(m.get(&k(r"d:\archives\a.txt")), Some(&3));
        // Ctrl+Z est un déplacement en sens inverse
        assert!(suivre_dans(&mut m, &p(r"D:\Archives\a.txt", r"C:\Docs\a.txt")));
        assert_eq!(m.get(&k(r"c:\docs\a.txt")), Some(&3));
        assert_eq!(m.len(), 1);
    }

    #[test]
    fn renommer_un_dossier_emmene_ce_qu_il_contient_et_rien_d_autre() {
        let mut m = Carte::new();
        m.insert(k(r"C:\Projets\Vieux"), 1);
        m.insert(k(r"C:\Projets\Vieux\sous\f.txt"), 2);
        m.insert(k(r"C:\Projets\VieuxAutre\f.txt"), 3);
        assert!(suivre_dans(&mut m, &p(r"C:\Projets\Vieux", r"C:\Projets\Neuf")));
        assert_eq!(m.get(&k(r"c:\projets\neuf")), Some(&1));
        assert_eq!(m.get(&k(r"c:\projets\neuf\sous\f.txt")), Some(&2));
        assert_eq!(m.get(&k(r"c:\projets\vieuxautre\f.txt")), Some(&3));
        assert_eq!(m.len(), 3);
    }

    #[test]
    fn purge_des_fichiers_disparus() {
        let mut m = Carte::new();
        m.insert(k(r"C:\la.txt"), 1);
        m.insert(k(r"C:\partie.txt"), 2);
        m.insert(k(r"\\serveur\partage\x.txt"), 3);
        // Le prédicat réel ne dit « disparu » que pour un disque fixe : le réseau n'est jamais interrogé
        assert!(!disparu(&k(r"\\serveur\partage\x.txt")));
        assert!(!disparu("pas un chemin"));
        let n = purger(&mut m, &|c| c.ends_with("partie.txt"));
        assert_eq!(n, 1);
        assert_eq!(m.len(), 2);
        assert!(!m.contains_key(&k(r"c:\partie.txt")));
    }

    /// Vrai renommage sur disque (dossier temporaire), aller puis retour comme Ctrl+Z : l'étiquette suit, la copie ne l'hérite pas
    #[test]
    fn renommage_reel_annulation_et_copie() {
        let base = std::env::temp_dir().join(format!("ne-etq-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).unwrap();
        let d = base.to_string_lossy().to_string();
        std::fs::write(base.join("a.txt"), b"x").unwrap();
        let chemin = |n: &str| format!("{d}\\{n}");
        assert!(basculer(&[chemin("a.txt")], 3));
        crate::operations::renommer(&d, "a.txt", "b.txt").unwrap();
        assert_eq!((de(&chemin("a.txt")), de(&chemin("b.txt"))), (None, Some(3)));
        // Ctrl+Z : `defaire` rappelle `renommer` dans l'autre sens
        crate::operations::renommer(&d, "b.txt", "a.txt").unwrap();
        assert_eq!((de(&chemin("a.txt")), de(&chemin("b.txt"))), (Some(3), None));
        // Une copie est un autre fichier : aucune étiquette (rien ne la suit, `fin_operation` ne traite que Deplacer)
        std::fs::copy(base.join("a.txt"), base.join("copie.txt")).unwrap();
        assert_eq!(de(&chemin("copie.txt")), None);
        // Renommage par lot
        std::fs::write(base.join("c.txt"), b"y").unwrap();
        assert!(basculer(&[chemin("c.txt")], 1));
        crate::lot::renommer(&d, &[("c.txt".to_string(), "e.txt".to_string())]).unwrap();
        assert_eq!((de(&chemin("c.txt")), de(&chemin("e.txt"))), (None, Some(1)));
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn api_publique_sans_disque_pendant_les_tests() {
        let a = r"Z:\lot16-test\Unique.TXT".to_string();
        assert_eq!(de(&a), None);
        assert!(basculer(&[a.clone()], 2));
        assert_eq!(de(r"z:/lot16-test/unique.txt"), Some(2));
        assert!(existe_une());
        suivre(&a, r"Z:\lot16-test\Autre.txt");
        assert_eq!(de(&a), None);
        assert_eq!(de(r"Z:\lot16-test\autre.txt"), Some(2));
        assert!(!basculer(&[r"Z:\lot16-test\autre.txt".to_string()], 2));
        assert_eq!(de(r"Z:\lot16-test\autre.txt"), None);
        // Étiquette hors de 1 à 3 : refusée
        assert!(!basculer(&[a.clone()], 4));
        assert_eq!(de(&a), None);
    }
}
