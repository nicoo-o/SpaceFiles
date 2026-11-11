// Synchronisation cloud en cours (E10) : OneDrive et les autres clients « fichiers à la demande » marquent
// chaque fichier ou dossier pas encore synchronisé (envoi ou réception en cours). L'état se lit sur le
// fichier lui-même avec l'API Cloud Files, sans le télécharger.

use std::collections::HashSet;

use windows::core::HSTRING;
use windows::Win32::Foundation::CloseHandle;
use windows::Win32::Storage::CloudFilters::*;
use windows::Win32::Storage::FileSystem::*;

/// Le fichier ou dossier `chemin` attend-il d'être synchronisé ? None : pas un fichier cloud
fn pas_synchronise(chemin: &str) -> Option<bool> {
    unsafe {
        // Lecture des seuls attributs, sans suivre le point d'analyse : rien n'est téléchargé
        let Ok(h) = CreateFileW(
            &HSTRING::from(chemin),
            FILE_READ_ATTRIBUTES.0,
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            None,
            OPEN_EXISTING,
            FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT,
            None,
        ) else {
            return None;
        };
        // Assez grand pour l'identité de fichier la plus longue (4 Ko)
        let mut tampon = vec![0u64; 520];
        let ok = CfGetPlaceholderInfo(h, CF_PLACEHOLDER_INFO_BASIC, tampon.as_mut_ptr() as *mut _, (tampon.len() * 8) as u32, None).is_ok();
        let _ = CloseHandle(h);
        ok.then(|| (*(tampon.as_ptr() as *const CF_PLACEHOLDER_BASIC_INFO)).InSyncState == CF_IN_SYNC_STATE_NOT_IN_SYNC)
    }
}

/// Noms (en minuscules) des éléments de `dossier` en attente de synchronisation, parmi `noms` (fichiers cloud)
pub fn en_attente(dossier: &str, noms: &[String]) -> HashSet<String> {
    noms.iter().filter(|n| pas_synchronise(&crate::dossier::joindre(dossier, n)) == Some(true)).map(|n| n.to_lowercase()).collect()
}

#[cfg(test)]
mod tests {
    #[test]
    #[ignore]
    fn onedrive_reel() {
        let d = std::env::var("OneDrive").unwrap_or_default();
        let noms: Vec<String> = std::fs::read_dir(&d).unwrap().flatten().map(|e| e.file_name().to_string_lossy().to_string()).collect();
        let t = std::time::Instant::now();
        let v = super::en_attente(&d, &noms);
        let lus = noms.iter().filter(|n| super::pas_synchronise(&crate::dossier::joindre(&d, n)).is_some()).count();
        println!("{lus} etats lus");
        println!("{} elements, {} en attente ({:?}) en {:?}", noms.len(), v.len(), v, t.elapsed());
    }
}
