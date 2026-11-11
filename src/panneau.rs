// Panneau de configuration dans notre vue (V9) : ses éléments listés par le Shell, ouverts par leur nom d'analyse.
// Ce n'est pas un dossier de fichiers : chaque élément n'est qu'un nom et de quoi le lancer.

use std::collections::HashMap;
use std::sync::Mutex;

use windows::core::PCWSTR;
use windows::Win32::System::Com::{CoInitializeEx, CoTaskMemFree, CoUninitialize, COINIT_APARTMENTTHREADED};
use windows::Win32::UI::Shell::{IEnumShellItems, IShellItem, SHGetKnownFolderItem, BHID_EnumItems, FOLDERID_ControlPanelFolder, KF_FLAG_DEFAULT, SIGDN_DESKTOPABSOLUTEPARSING, SIGDN_NORMALDISPLAY};

use crate::dossier::{entree_fichier, Entree};

/// Nom affiché (minuscules) → nom d'analyse du Shell (`::{GUID}\…`), pour ouvrir l'élément choisi
static PARSING: Mutex<Option<HashMap<String, String>>> = Mutex::new(None);

/// Éléments du Panneau de configuration. Le fil de lecture n'a pas COM : il est initialisé ici le temps de l'énumération.
pub fn lire() -> Result<Vec<Entree>, String> {
    // SAFETY: appariés ; un résultat S_FALSE (déjà initialisé) demande quand même son CoUninitialize.
    let initialise = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) }.is_ok();
    let r = enumerer();
    if initialise {
        unsafe { CoUninitialize() };
    }
    r
}

fn enumerer() -> Result<Vec<Entree>, String> {
    let mut v = Vec::new();
    let mut noms = HashMap::new();
    // SAFETY: les chaînes rendues par le Shell sont copiées puis libérées par `CoTaskMemFree` ; les objets COM vivent le temps de la boucle.
    unsafe {
        let dossier: IShellItem = SHGetKnownFolderItem(&FOLDERID_ControlPanelFolder, KF_FLAG_DEFAULT, None).map_err(|e| format!("PANNEAU DE CONFIGURATION INACCESSIBLE : {e}"))?;
        let enumeration: IEnumShellItems = dossier.BindToHandler(None, &BHID_EnumItems).map_err(|e| format!("PANNEAU ILLISIBLE : {e}"))?;
        loop {
            let mut item = [None];
            let mut n = 0u32;
            if enumeration.Next(&mut item, Some(&mut n)).is_err() || n == 0 {
                break;
            }
            let Some(item) = item[0].take() else { break };
            let texte = |sigdn| -> Option<String> {
                let p = item.GetDisplayName(sigdn).ok()?;
                let s = PCWSTR(p.0).to_string().ok();
                CoTaskMemFree(Some(p.0 as *const _));
                s
            };
            let (Some(nom), Some(analyse)) = (texte(SIGDN_NORMALDISPLAY), texte(SIGDN_DESKTOPABSOLUTEPARSING)) else { continue };
            noms.insert(nom.to_lowercase(), analyse);
            let mut e = entree_fichier(&nom, 0, 0, false);
            e.ext = "PANNEAU".into();
            v.push(e);
        }
    }
    *PARSING.lock().unwrap_or_else(|e| e.into_inner()) = Some(noms);
    Ok(v)
}

/// De quoi lancer l'élément affiché sous ce nom
pub fn analyse(nom: &str) -> Option<String> {
    PARSING.lock().unwrap_or_else(|e| e.into_inner()).as_ref()?.get(&nom.to_lowercase()).cloned()
}

#[cfg(test)]
mod tests {
    #[test]
    fn elements_du_panneau() {
        use windows::Win32::System::Com::{CoInitializeEx, CoUninitialize, COINIT_APARTMENTTHREADED};
        unsafe {
            let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
            let v = super::lire().expect("lecture du Panneau de configuration");
            assert!(v.len() > 5, "{} éléments", v.len());
            let premier = &v[0].nom;
            assert!(super::analyse(premier).is_some_and(|a| !a.is_empty()));
            CoUninitialize();
        }
    }
}
