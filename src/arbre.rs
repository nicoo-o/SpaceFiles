// Arbre de dossiers (V1) : section ARBORESCENCE du volet. L'état (dossiers dépliés, enfants déjà lus) est
// commun à tous les onglets. Les enfants d'un dossier se lisent dans un fil à part et reviennent par
// WM_ARBRE : un lecteur réseau ou OneDrive ne gèle jamais la fenêtre. Aucune surveillance : l'arbre se
// relit au dépliage et suit le dossier ouvert à chaque navigation.

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicIsize, Ordering};
use std::sync::Mutex;

use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::{PostMessageW, WM_APP};

/// Enfants d'un dossier lus : lparam = Box<(String, Vec<(String, String)>)> (clé, (nom, chemin))
pub const WM_ARBRE: u32 = WM_APP + 30;

/// Pas d'indentation d'un niveau, en DIP
pub const RETRAIT: f32 = 12.0;

#[derive(Clone, Debug, PartialEq)]
pub struct Ligne {
    pub nom: String,
    pub chemin: String,
    pub profondeur: usize,
    pub ouvert: bool,
    /// Pas encore lu : None ; sinon « a des sous-dossiers »
    pub enfants: Option<bool>,
}

#[derive(Default)]
struct Etat {
    ouverts: HashSet<String>,
    enfants: HashMap<String, Vec<(String, String)>>,
    demandes: HashSet<String>,
    /// Dernier dossier suivi : l'arbre ne redéplie ses ancêtres qu'à un changement de dossier
    dernier: String,
}

static ETAT: Mutex<Option<Etat>> = Mutex::new(None);
static FENETRE: AtomicIsize = AtomicIsize::new(0);

/// Fenêtre qui reçoit WM_ARBRE
pub fn lier(hwnd: HWND) {
    FENETRE.store(hwnd.0 as isize, Ordering::Relaxed);
}

fn avec<R>(f: impl FnOnce(&mut Etat) -> R) -> R {
    let mut g = ETAT.lock().unwrap_or_else(|e| e.into_inner());
    f(g.get_or_insert_with(Etat::default))
}

/// Clé d'un dossier : minuscules, sans « \ » final
pub fn cle(chemin: &str) -> String {
    chemin.trim_end_matches('\\').to_lowercase()
}

/// « C:\a\b » → « C: », « C:\a », « C:\a\b » (chemins d'origine, pas des clés)
fn chemins_jusqu_a(chemin: &str) -> Vec<String> {
    let c = chemin.trim_end_matches('\\');
    let mut v: Vec<String> = c.match_indices('\\').map(|(i, _)| c[..i].to_string()).filter(|p| !p.is_empty() && !p.ends_with('\\')).collect();
    v.push(c.to_string());
    v
}

/// Lit les sous-dossiers de `chemin` dans un fil et les renvoie à la fenêtre
fn lire_en_fond(chemin: String, k: String) {
    let hwnd = FENETRE.load(Ordering::Relaxed);
    if hwnd == 0 {
        return;
    }
    let caches = crate::reglages::actuels().caches;
    std::thread::spawn(move || {
        use std::os::windows::fs::MetadataExt;
        let base = format!("{}\\", chemin.trim_end_matches('\\'));
        let mut v: Vec<(String, String)> = std::fs::read_dir(&base)
            .map(|it| {
                it.flatten()
                    // Les liens (jonctions, raccourcis de dossier) ne sont pas suivis : pas de boucle possible
                    .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
                    .filter(|e| caches || e.metadata().is_ok_and(|m| m.file_attributes() & 0x6 == 0))
                    .map(|e| {
                        let nom = e.file_name().to_string_lossy().to_string();
                        let c = format!("{base}{nom}");
                        (nom, c)
                    })
                    .collect()
            })
            .unwrap_or_default();
        v.sort_by_key(|(n, _)| n.to_lowercase());
        let p = Box::into_raw(Box::new((k, v)));
        // SAFETY : la boîte appartient à ce fil jusqu'à l'envoi ; reprise ici si le message n'est pas parti
        if unsafe { PostMessageW(HWND(hwnd as *mut _), WM_ARBRE, WPARAM(0), LPARAM(p as isize)) }.is_err() {
            drop(unsafe { Box::from_raw(p) });
        }
    });
}

fn demander(e: &mut Etat, chemin: &str, k: &str, lire: &mut dyn FnMut(String, String)) {
    if e.demandes.insert(k.to_string()) {
        lire(chemin.to_string(), k.to_string());
    }
}

fn pousser(e: &mut Etat, out: &mut Vec<Ligne>, nom: &str, chemin: &str, profondeur: usize, lire: &mut dyn FnMut(String, String)) {
    let k = cle(chemin);
    let ouvert = e.ouverts.contains(&k);
    let enfants = e.enfants.get(&k).cloned();
    out.push(Ligne { nom: nom.to_string(), chemin: chemin.to_string(), profondeur, ouvert, enfants: enfants.as_ref().map(|v| !v.is_empty()) });
    if ouvert {
        match enfants {
            Some(v) => {
                for (n, c) in &v {
                    pousser(e, out, n, c, profondeur + 1, lire);
                }
            }
            None => demander(e, chemin, &k, lire),
        }
    }
}

fn aplatir_dans(e: &mut Etat, racines: &[(String, String)], courant: &str, lire: &mut dyn FnMut(String, String)) -> Vec<Ligne> {
    let suivi = cle(courant);
    if !suivi.is_empty() && suivi != e.dernier {
        e.dernier = suivi;
        for p in chemins_jusqu_a(courant) {
            e.ouverts.insert(cle(&p));
        }
    }
    let mut out = Vec::new();
    for (nom, chemin) in racines {
        pousser(e, &mut out, nom, chemin, 0, lire);
    }
    out
}

/// Lignes visibles de l'arbre, dans l'ordre d'affichage. `racines` : (nom, chemin) des disques ;
/// `courant` : dossier ouvert, dont les ancêtres se déplient.
pub fn aplatir(racines: &[(String, String)], courant: &str) -> Vec<Ligne> {
    avec(|e| aplatir_dans(e, racines, courant, &mut lire_en_fond))
}

/// Réponse du fil de lecture
pub fn recevoir(k: String, enfants: Vec<(String, String)>) {
    avec(|e| {
        e.demandes.remove(&k);
        e.enfants.insert(k, enfants);
    })
}

pub fn deplier(chemin: &str) {
    let k = cle(chemin);
    avec(|e| {
        e.ouverts.insert(k.clone());
        // Relu à chaque dépliage : le contenu a pu changer depuis la dernière fois
        e.demandes.remove(&k);
        demander(e, chemin, &k, &mut lire_en_fond);
    })
}

pub fn replier(chemin: &str) {
    avec(|e| {
        e.ouverts.remove(&cle(chemin));
    })
}

pub fn basculer(chemin: &str) {
    if avec(|e| e.ouverts.contains(&cle(chemin))) {
        replier(chemin)
    } else {
        deplier(chemin)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lignes(e: &mut Etat, courant: &str) -> (Vec<Ligne>, Vec<String>) {
        let mut lus = Vec::new();
        let racines = vec![("C:".to_string(), "C:\\".to_string())];
        let v = aplatir_dans(e, &racines, courant, &mut |c, _| lus.push(c));
        (v, lus)
    }

    #[test]
    fn ancetres() {
        assert_eq!(chemins_jusqu_a(r"C:\a\b"), vec!["C:", r"C:\a", r"C:\a\b"]);
        assert_eq!(chemins_jusqu_a("C:\\"), vec!["C:"]);
        assert_eq!(chemins_jusqu_a(r"\\nas\partage\x"), vec![r"\\nas", r"\\nas\partage", r"\\nas\partage\x"]);
    }

    #[test]
    fn suit_le_dossier_ouvert() {
        let mut e = Etat::default();
        // Rien de lu : la racine et ses ancêtres sont dépliés, leur lecture est demandée une fois
        let (v, lus) = lignes(&mut e, r"C:\Users");
        assert_eq!(v.len(), 1);
        assert!(v[0].ouvert && v[0].enfants.is_none());
        assert_eq!(lus, vec!["C:\\"]);
        let (_, lus) = lignes(&mut e, r"C:\Users");
        assert!(lus.is_empty(), "pas de seconde lecture en attendant la réponse");
        e.enfants.insert("c:".into(), vec![("Users".into(), r"C:\Users".into()), ("Windows".into(), r"C:\Windows".into())]);
        e.demandes.clear();
        let (v, lus) = lignes(&mut e, r"C:\Users");
        assert_eq!(v.iter().map(|l| (l.nom.as_str(), l.profondeur, l.ouvert)).collect::<Vec<_>>(), vec![("C:", 0, true), ("Users", 1, true), ("Windows", 1, false)]);
        assert_eq!(lus, vec![r"C:\Users"]);
        // Replier à la main tient tant qu'on ne change pas de dossier
        e.ouverts.remove("c:");
        let (v, _) = lignes(&mut e, r"C:\Users");
        assert_eq!(v.len(), 1);
        let (v, _) = lignes(&mut e, r"C:\Windows");
        assert_eq!(v.len(), 3);
    }
}
