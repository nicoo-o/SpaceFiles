// Corbeille Glyph (O7) : remplissage de la Corbeille en points (vue Ce PC), et vidage avec dissolution

use std::sync::Mutex;

use windows::core::{HSTRING, PCWSTR};
use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::System::Registry::{RegGetValueW, HKEY_CURRENT_USER, RRF_RT_REG_DWORD};
use windows::Win32::UI::Shell::{SHEmptyRecycleBinW, SHQueryRecycleBinW, SHQUERYRBINFO};
use windows::Win32::UI::WindowsAndMessaging::PostMessageW;

/// Contenu de la Corbeille (tous les disques)
#[derive(Clone, Copy, Default)]
pub struct Etat {
    pub octets: u64,
    pub elements: u64,
    /// Place réservée à la Corbeille (somme des disques), 0 si inconnue
    pub capacite: u64,
}

static ETAT: Mutex<Option<Etat>> = Mutex::new(None);

pub fn etat() -> Option<Etat> {
    *ETAT.lock().unwrap_or_else(|e| e.into_inner())
}

/// Place réservée par Windows : « MaxCapacity » (Mo) de chaque disque, dans le registre de l'utilisateur
fn capacite() -> u64 {
    let base = r"Software\Microsoft\Windows\CurrentVersion\Explorer\BitBucket\Volume";
    let Some(cle) = crate::stockages::ouvrir(HKEY_CURRENT_USER, base) else { return 0 };
    crate::stockages::sous_cles(&cle)
        .iter()
        .filter_map(|v| {
            let mut mo = 0u32;
            let mut taille = 4u32;
            let r = unsafe { RegGetValueW(HKEY_CURRENT_USER, &HSTRING::from(format!(r"{base}\{v}")), &HSTRING::from("MaxCapacity"), RRF_RT_REG_DWORD, None, Some(&mut mo as *mut u32 as *mut _), Some(&mut taille)) };
            r.is_ok().then_some(mo as u64 * 1024 * 1024)
        })
        .sum()
}

/// Relit la Corbeille en arrière-plan ; la fenêtre est redessinée quand c'est prêt
pub fn actualiser(hwnd: HWND) {
    let h = hwnd.0 as isize;
    std::thread::spawn(move || {
        let mut info = SHQUERYRBINFO { cbSize: std::mem::size_of::<SHQUERYRBINFO>() as u32, ..Default::default() };
        if unsafe { SHQueryRecycleBinW(PCWSTR::null(), &mut info) }.is_err() {
            return;
        }
        let e = Etat { octets: info.i64Size.max(0) as u64, elements: info.i64NumItems.max(0) as u64, capacite: capacite() };
        *ETAT.lock().unwrap_or_else(|e| e.into_inner()) = Some(e);
        unsafe {
            let _ = PostMessageW(HWND(h as *mut _), crate::index::WM_INDEX, WPARAM(0), LPARAM(0));
        }
    });
}

/// Points allumés sur 100 : part de la place réservée (au moins un point s'il y a quelque chose)
pub fn points(e: &Etat) -> usize {
    if e.elements == 0 {
        return 0;
    }
    let capacite = if e.capacite > 0 { e.capacite } else { e.octets.max(1) };
    ((e.octets as f64 * 100.0 / capacite as f64).ceil() as usize).clamp(1, 100)
}

/// Vide la Corbeille ; Windows demande confirmation. Vrai si elle a été vidée
pub fn vider(hwnd: HWND) -> bool {
    let ok = unsafe { SHEmptyRecycleBinW(hwnd, PCWSTR::null(), 0) }.is_ok();
    if ok {
        let capacite = etat().map_or(0, |e| e.capacite);
        *ETAT.lock().unwrap_or_else(|e| e.into_inner()) = Some(Etat { capacite, ..Default::default() });
    }
    ok
}

// ── Contenu de la Corbeille dans notre vue (V9) ──
// Lu dans les fichiers `$I` de `X:\$Recycle.Bin\<SID>` (nom d'origine, taille, date de suppression) sans passer par
// les objets du Shell ; le contenu lui-même est le fichier `$R` voisin, que l'on ouvre, copie ou prévisualise tel quel.

/// Chemin d'origine (minuscules) → (fichier `$R`, fichier `$I`)
static REELS: Mutex<Option<std::collections::HashMap<String, (String, String)>>> = Mutex::new(None);

/// Lit un fichier `$I` : taille, date de suppression (FILETIME), chemin d'origine. Versions 1 (Windows 7 à 8) et 2 (10 et 11).
pub fn analyser_i(o: &[u8]) -> Option<(u64, u64, String)> {
    let u64_ = |i: usize| o.get(i..i + 8).map(|b| u64::from_le_bytes(b.try_into().unwrap()));
    let (version, taille, date) = (u64_(0)?, u64_(8)?, u64_(16)?);
    let texte = match version {
        1 => o.get(24..)?,
        2 => {
            let n = u32::from_le_bytes(o.get(24..28)?.try_into().ok()?) as usize;
            o.get(28..28 + n.checked_mul(2)?)?
        }
        _ => return None,
    };
    let unites: Vec<u16> = texte.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).take_while(|&u| u != 0).collect();
    let chemin = String::from_utf16(&unites).ok().filter(|c| !c.is_empty())?;
    Some((taille, date, chemin))
}

/// Éléments de la Corbeille de l'utilisateur, tous disques fixes confondus
pub fn lire() -> Vec<crate::dossier::Entree> {
    // Un fil par disque présent : chercher les 26 lettres l'une après l'autre prenait 0,6 s
    let presents = unsafe { windows::Win32::Storage::FileSystem::GetLogicalDrives() };
    let parts: Vec<_> = std::thread::scope(|s| {
        let fils: Vec<_> = ('A'..='Z').filter(|l| presents & (1 << (*l as u8 - b'A')) != 0).map(|l| s.spawn(move || lire_disque(l))).collect();
        fils.into_iter().filter_map(|f| f.join().ok()).collect()
    });
    let mut reels = std::collections::HashMap::new();
    let mut v = Vec::new();
    for (e, r) in parts {
        v.extend(e);
        reels.extend(r);
    }
    *REELS.lock().unwrap_or_else(|e| e.into_inner()) = Some(reels);
    v
}

type Lu = (Vec<crate::dossier::Entree>, std::collections::HashMap<String, (String, String)>);

/// Corbeille d'un disque
fn lire_disque(lettre: char) -> Lu {
    let mut reels = std::collections::HashMap::new();
    let mut v = Vec::new();
    {
        let racine = format!(r"{lettre}:\$Recycle.Bin");
        // Les dossiers des autres utilisateurs refusent l'accès : ils sont ignorés
        let Ok(sids) = std::fs::read_dir(&racine) else { return (v, reels) };
        for sid in sids.flatten() {
            let Ok(fichiers) = std::fs::read_dir(sid.path()) else { continue };
            for f in fichiers.flatten() {
                let nom_i = f.file_name().to_string_lossy().into_owned();
                let Some(suite) = nom_i.strip_prefix("$I") else { continue };
                let chemin_i = f.path();
                let chemin_r = chemin_i.with_file_name(format!("$R{suite}"));
                let (Ok(octets), Ok(meta)) = (std::fs::read(&chemin_i), std::fs::symlink_metadata(&chemin_r)) else { continue };
                let Some((taille, date, origine)) = analyser_i(&octets) else { continue };
                let Some((parent, nom)) = origine.rsplit_once('\\') else { continue };
                let mut e = crate::dossier::entree_fichier(nom, if meta.is_dir() { 0 } else { taille }, date, meta.is_dir());
                e.ajout = date;
                e.parent = Some(std::sync::Arc::from(parent));
                reels.insert(origine.to_lowercase(), (chemin_r.to_string_lossy().into_owned(), chemin_i.to_string_lossy().into_owned()));
                v.push(e);
            }
        }
    }
    (v, reels)
}

/// Le fichier réel (`$R…`) derrière un élément affiché de la Corbeille
pub fn reel(origine: &str) -> Option<String> {
    REELS.lock().unwrap_or_else(|e| e.into_inner()).as_ref()?.get(&origine.to_lowercase()).map(|x| x.0.clone())
}

/// Remet un élément à son emplacement d'origine ; refuse d'écraser quoi que ce soit
pub fn restaurer(origine: &str) -> Result<(), String> {
    let (r, i) = REELS.lock().unwrap_or_else(|e| e.into_inner()).as_ref().and_then(|m| m.get(&origine.to_lowercase()).cloned()).ok_or("ELEMENT INTROUVABLE DANS LA CORBEILLE")?;
    if std::path::Path::new(origine).exists() {
        return Err(format!("{origine} EXISTE DEJA : RIEN N'EST ECRASE"));
    }
    if let Some(p) = std::path::Path::new(origine).parent() {
        std::fs::create_dir_all(p).map_err(|e| format!("DOSSIER D'ORIGINE INACCESSIBLE : {e}"))?;
    }
    std::fs::rename(&r, origine).map_err(|e| format!("RESTAURATION IMPOSSIBLE : {e}"))?;
    let _ = std::fs::remove_file(i);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn restauration_sans_ecraser() {
        let d = std::env::temp_dir().join(format!("ne-corbeille-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        let (r, i, origine) = (d.join("$RABC.txt"), d.join("$IABC.txt"), d.join("sous").join("a.txt"));
        std::fs::write(&r, "contenu").unwrap();
        std::fs::write(&i, "meta").unwrap();
        let (o, cle) = (origine.to_string_lossy().into_owned(), origine.to_string_lossy().to_lowercase());
        let enregistrer = || REELS.lock().unwrap().get_or_insert_with(Default::default).insert(cle.clone(), (r.to_string_lossy().into_owned(), i.to_string_lossy().into_owned()));
        enregistrer();
        restaurer(&o).unwrap();
        assert_eq!(std::fs::read_to_string(&origine).unwrap(), "contenu");
        assert!(!i.exists());
        // Un second élément du même nom ne remplace pas le premier
        std::fs::write(&r, "autre").unwrap();
        enregistrer();
        assert!(restaurer(&o).unwrap_err().contains("EXISTE DEJA"));
        assert_eq!(std::fs::read_to_string(&origine).unwrap(), "contenu");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn fichier_i_versions_1_et_2() {
        let utf16 = |s: &str| s.encode_utf16().flat_map(|u| u.to_le_bytes()).collect::<Vec<u8>>();
        let mut v2 = [2u64.to_le_bytes(), 1234u64.to_le_bytes(), 99u64.to_le_bytes()].concat();
        let nom = r"C:\Users\nico\été.txt";
        v2.extend((nom.encode_utf16().count() as u32 + 1).to_le_bytes());
        v2.extend(utf16(nom));
        v2.extend([0, 0]);
        assert_eq!(analyser_i(&v2), Some((1234, 99, nom.to_string())));
        let mut v1 = [1u64.to_le_bytes(), 5u64.to_le_bytes(), 7u64.to_le_bytes()].concat();
        v1.extend(utf16(r"D:\a.bin"));
        v1.extend(vec![0u8; 520 - 18]);
        assert_eq!(analyser_i(&v1), Some((5, 7, r"D:\a.bin".to_string())));
        assert_eq!(analyser_i(&[0u8; 10]), None);
        // Longueur annoncée plus grande que le fichier : refusé, pas de panique
        let mut faux = [2u64.to_le_bytes(), 1u64.to_le_bytes(), 1u64.to_le_bytes()].concat();
        faux.extend(u32::MAX.to_le_bytes());
        assert_eq!(analyser_i(&faux), None);
    }
}
