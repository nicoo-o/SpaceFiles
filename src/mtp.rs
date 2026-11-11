// Téléphones et appareils MTP (V3), première étape : les reconnaître. Windows Portable Devices (WPD) donne le nom
// de chaque appareil branché ; le contenu (dossiers, photos) n'est pas encore lu : voir ORDRE.md, point 20.

use windows::core::{PCWSTR, PWSTR};
use windows::Win32::Devices::PortableDevices::{IPortableDeviceManager, PortableDeviceManager};
use windows::Win32::System::Com::{CoCreateInstance, CoTaskMemFree, CLSCTX_INPROC_SERVER};

/// Noms des appareils portables branchés (vide s'il n'y en a pas). Le fil appelant doit avoir initialisé COM.
pub fn appareils() -> Result<Vec<String>, String> {
    // SAFETY: les tableaux passés à WPD ont la taille annoncée ; les identifiants rendus par GetDevices sont
    // alloués par COM et libérés ici un à un après lecture.
    unsafe {
        let gestionnaire: IPortableDeviceManager = CoCreateInstance(&PortableDeviceManager, None, CLSCTX_INPROC_SERVER).map_err(|_| "GESTIONNAIRE D'APPAREILS INDISPONIBLE".to_string())?;
        gestionnaire.RefreshDeviceList().map_err(|_| "LISTE DES APPAREILS ILLISIBLE".to_string())?;
        let mut n = 0u32;
        gestionnaire.GetDevices(std::ptr::null_mut(), &mut n).map_err(|_| "LISTE DES APPAREILS ILLISIBLE".to_string())?;
        if n == 0 {
            return Ok(Vec::new());
        }
        let mut ids = vec![PWSTR::null(); n as usize];
        gestionnaire.GetDevices(ids.as_mut_ptr(), &mut n).map_err(|_| "LISTE DES APPAREILS ILLISIBLE".to_string())?;
        let mut noms = Vec::new();
        for id in ids.iter().take(n as usize) {
            let mut len = 0u32;
            let _ = gestionnaire.GetDeviceFriendlyName(PCWSTR(id.0), PWSTR::null(), &mut len);
            let mut tampon = vec![0u16; len.max(1) as usize];
            let nom = match gestionnaire.GetDeviceFriendlyName(PCWSTR(id.0), PWSTR(tampon.as_mut_ptr()), &mut len) {
                Ok(()) => String::from_utf16_lossy(&tampon[..(len as usize).saturating_sub(1)]),
                Err(_) => "APPAREIL SANS NOM".to_string(),
            };
            noms.push(nom);
            CoTaskMemFree(Some(id.0 as *const _));
        }
        Ok(noms)
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn liste_sans_planter() {
        // Sans téléphone branché la liste est vide ; avec, elle porte des noms : dans les deux cas, pas d'erreur
        unsafe {
            let _ = windows::Win32::System::Com::CoInitializeEx(None, windows::Win32::System::Com::COINIT_APARTMENTTHREADED);
        }
        let r = super::appareils();
        assert!(r.is_ok(), "{r:?}");
    }
}
