// Doublons (O9) : fichiers identiques sous un dossier. Même taille d'abord (rien à lire), puis une empreinte
// rapide (début et fin du fichier), enfin le SHA-256 complet pour confirmer. Les lots sont rangés par place
// à gagner ; la liste les montre en sections (« 3 × 12 MO · 24 MO A GAGNER »).

use std::collections::HashMap;
use std::io::{Read, Seek, SeekFrom};
use std::os::windows::fs::MetadataExt;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use crate::dossier::Entree;

/// Lot de chaque fichier trouvé (chemin en minuscules) : rang (le plus gros gain d'abord) et titre de section
static LOTS: Mutex<Option<HashMap<String, (u32, String)>>> = Mutex::new(None);
/// Recherche en cours : une nouvelle arrête la précédente
static TRAVAIL: AtomicU64 = AtomicU64::new(0);

/// Au-delà, le parcours s'arrête (dossier énorme)
const MAX_FICHIERS: usize = 300_000;
/// Plus gros que ça, l'empreinte rapide (début, milieu, fin) suffit : tout relire serait trop long
const MAX_COMPLET: u64 = 1 << 30;

/// Section d'un élément de la liste des doublons
pub fn section(e: &Entree) -> (u32, String) {
    let c = crate::dossier::joindre(e.parent.as_deref().unwrap_or(""), &e.nom).to_lowercase();
    LOTS.lock().unwrap_or_else(|x| x.into_inner()).as_ref().and_then(|m| m.get(&c).cloned()).unwrap_or((u32::MAX, String::new()))
}

/// Empreinte rapide : taille, 64 Ko au début, au milieu et à la fin
fn empreinte_rapide(chemin: &str, taille: u64) -> Option<u64> {
    use std::hash::{Hash, Hasher};
    let mut f = std::fs::File::open(chemin).ok()?;
    let mut h = std::collections::hash_map::DefaultHasher::new();
    taille.hash(&mut h);
    let mut buf = vec![0u8; 64 << 10];
    for pos in [0, taille / 2, taille.saturating_sub(64 << 10)] {
        f.seek(SeekFrom::Start(pos)).ok()?;
        let n = f.read(&mut buf).ok()?;
        buf[..n].hash(&mut h);
    }
    Some(h.finish())
}

/// Range des chemins par clé, ne garde que les clés partagées par au moins deux fichiers
fn par_cle<K: std::hash::Hash + Eq>(chemins: Vec<(String, u64, u64)>, cle: impl Fn(&str, u64) -> Option<K>, travail: u64) -> Option<Vec<Vec<(String, u64, u64)>>> {
    let mut m: HashMap<K, Vec<(String, u64, u64)>> = HashMap::new();
    for c in chemins {
        if TRAVAIL.load(Ordering::Relaxed) != travail {
            return None;
        }
        if let Some(k) = cle(&c.0, c.1) {
            m.entry(k).or_default().push(c);
        }
    }
    Some(m.into_values().filter(|v| v.len() > 1).collect())
}

/// Cherche les doublons sous `racine` (sous-dossiers compris)
pub fn chercher(racine: &str) -> Result<Vec<Entree>, String> {
    let travail = TRAVAIL.fetch_add(1, Ordering::SeqCst) + 1;
    // 1. Tous les fichiers non vides, rangés par taille
    let mut par_taille: HashMap<u64, Vec<(String, u64, u64)>> = HashMap::new();
    let mut pile = vec![racine.to_string()];
    let mut n = 0usize;
    while let Some(d) = pile.pop() {
        let Ok(lecture) = std::fs::read_dir(&d) else { continue };
        for e in lecture.flatten() {
            let Ok(m) = std::fs::symlink_metadata(e.path()) else { continue };
            let chemin = crate::dossier::joindre(&d, &e.file_name().to_string_lossy());
            // Fichiers cloud seulement en ligne : les lire les téléchargerait
            const EN_LIGNE: u32 = 0x0040_0000 | 0x0004_0000 | 0x0000_1000;
            if m.is_dir() {
                if m.file_attributes() & 0x400 == 0 {
                    pile.push(chemin);
                }
            } else if m.len() > 0 && m.file_attributes() & EN_LIGNE == 0 {
                par_taille.entry(m.len()).or_default().push((chemin, m.len(), m.last_write_time()));
                n += 1;
            }
        }
        if n > MAX_FICHIERS || TRAVAIL.load(Ordering::Relaxed) != travail {
            break;
        }
    }
    if TRAVAIL.load(Ordering::Relaxed) != travail {
        return Err("RECHERCHE ARRETEE".into());
    }
    // 2. Même taille → même empreinte rapide → même SHA-256
    let candidats: Vec<(String, u64, u64)> = par_taille.into_values().filter(|v| v.len() > 1).flatten().collect();
    let rapides = par_cle(candidats, |c, t| empreinte_rapide(c, t).map(|h| (t, h)), travail).ok_or("RECHERCHE ARRETEE")?;
    let mut lots = Vec::new();
    // Lots seulement confirmés par l'empreinte rapide : annoncés comme tels, pas comme des doublons prouvés
    let mut non_verifies = std::collections::HashSet::new();
    for groupe in rapides {
        if groupe[0].1 > MAX_COMPLET {
            non_verifies.insert(groupe[0].0.clone());
            lots.push(groupe);
            continue;
        }
        lots.extend(par_cle(groupe, |c, _| crate::outils::empreinte(c).ok(), travail).ok_or("RECHERCHE ARRETEE")?);
    }
    // 3. Le plus de place à gagner d'abord
    lots.sort_by_key(|l| std::cmp::Reverse(l[0].1 * (l.len() as u64 - 1)));
    let mut carte = HashMap::new();
    let mut v = Vec::new();
    for (rang, lot) in lots.iter().enumerate() {
        let t = lot[0].1;
        let titre = format!("{} × {} · {} A GAGNER{}", lot.len(), crate::systeme::taille(t), crate::systeme::taille(t * (lot.len() as u64 - 1)), if non_verifies.contains(&lot[0].0) { " · NON VERIFIE (CTRL+ALT+H POUR COMPARER)" } else { "" });
        for (c, taille, modif) in lot {
            carte.insert(c.to_lowercase(), (rang as u32, titre.clone()));
            let (parent, nom) = c.rsplit_once('\\').unwrap_or(("", c));
            let mut e = crate::dossier::entree_fichier(nom, *taille, *modif, false);
            e.parent = Some(Arc::from(if parent.len() == 2 { format!("{parent}\\") } else { parent.to_string() }.as_str()));
            v.push(e);
        }
    }
    *LOTS.lock().unwrap_or_else(|x| x.into_inner()) = Some(carte);
    Ok(v)
}

/// Résumé pour la barre d'état : nombre de lots et place à gagner
pub fn resume(entrees: &[Entree]) -> String {
    let m = LOTS.lock().unwrap_or_else(|x| x.into_inner());
    let Some(m) = m.as_ref() else { return String::new() };
    let mut lots: HashMap<u32, (usize, u64)> = HashMap::new();
    for e in entrees {
        let c = crate::dossier::joindre(e.parent.as_deref().unwrap_or(""), &e.nom).to_lowercase();
        if let Some((r, _)) = m.get(&c) {
            let x = lots.entry(*r).or_insert((0, e.taille));
            x.0 += 1;
        }
    }
    let gain: u64 = lots.values().map(|(n, t)| t * (*n as u64).saturating_sub(1)).sum();
    format!("{} LOT{} · {} A GAGNER", lots.len(), if lots.len() > 1 { "S" } else { "" }, crate::systeme::taille(gain))
}
