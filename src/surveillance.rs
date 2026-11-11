// Surveillance du dossier ouvert : prévient la fenêtre quand son contenu change,
// que ce soit par SpaceFiles ou par une autre application

use std::sync::Arc;

use windows::core::PCWSTR;
use windows::Win32::Foundation::*;
use windows::Win32::Storage::FileSystem::*;
use windows::Win32::System::Threading::*;
use windows::Win32::UI::WindowsAndMessaging::{PostMessageW, WM_APP};

/// Le dossier surveillé a changé (WPARAM = jeton de la surveillance)
pub const WM_CHANGEMENT: u32 = WM_APP + 2;

/// Handle Windows fermé quand plus personne ne l'utilise
pub(crate) struct Poignee(pub(crate) HANDLE);
// SAFETY: le handle est un événement Windows (objet noyau) : il s'utilise depuis n'importe quel fil (SetEvent, attente) et n'est
// fermé qu'ici, par `Drop`, quand le dernier `Arc` disparaît
unsafe impl Send for Poignee {}
// SAFETY: voir `Send` : les seuls appels faits dessus (SetEvent, attentes) sont sûrs à plusieurs fils à la fois
unsafe impl Sync for Poignee {}
impl Drop for Poignee {
    fn drop(&mut self) {
        // SAFETY: `self.0` est un handle valide possédé par cette valeur, fermé une seule fois (le `Drop` ne s'exécute qu'une fois)
        unsafe {
            let _ = CloseHandle(self.0);
        }
    }
}

/// Tant qu'il existe, le dossier est surveillé. Le détruire arrête la surveillance.
pub struct Surveillant {
    arret: Arc<Poignee>,
}

impl Drop for Surveillant {
    fn drop(&mut self) {
        // SAFETY: `arret` est un événement valide tant que l'Arc vit (le fil en garde un second) ; SetEvent ne garde aucune référence
        unsafe {
            let _ = SetEvent(self.arret.0);
        }
    }
}

pub fn surveiller(hwnd: HWND, chemin: &str, jeton: u64) -> Option<Surveillant> {
    // SAFETY: création d'un événement sans nom ; le handle est ensuite possédé par `Poignee`
    let arret = Arc::new(Poignee(unsafe { CreateEventW(None, TRUE, FALSE, PCWSTR::null()).ok()? }));
    let arret_fil = arret.clone();
    let chemin_w: Vec<u16> = chemin.encode_utf16().chain(std::iter::once(0)).collect();
    let fenetre = hwnd.0 as isize;
    // SAFETY (tout le bloc du fil) : `chemin_w` se termine par 0 et vit autant que le fil ; `arret_fil` (Arc) garde valide le handle
    // d'arrêt ; le handle de notification est ouvert ici, utilisé sur ce seul fil et fermé avant la fin du fil
    std::thread::spawn(move || unsafe {
        let filtre = FILE_NOTIFY_CHANGE_FILE_NAME | FILE_NOTIFY_CHANGE_DIR_NAME | FILE_NOTIFY_CHANGE_SIZE | FILE_NOTIFY_CHANGE_LAST_WRITE | FILE_NOTIFY_CHANGE_ATTRIBUTES;
        let Ok(changement) = FindFirstChangeNotificationW(PCWSTR(chemin_w.as_ptr()), FALSE, filtre) else { return };
        'externe: loop {
            let r = WaitForMultipleObjects(&[changement, arret_fil.0], FALSE, INFINITE);
            if r != WAIT_OBJECT_0 {
                break;
            }
            // Les changements arrivent souvent en rafale (copie de 100 fichiers) : on attend un peu
            // et on regroupe tout en une seule actualisation
            loop {
                // Dossier supprimé ou partage coupé : le handle reste signalé, on ne reboucle pas dessus
                if FindNextChangeNotification(changement).is_err() {
                    break 'externe;
                }
                if WaitForMultipleObjects(&[changement, arret_fil.0], FALSE, 150) != WAIT_OBJECT_0 {
                    break;
                }
            }
            if WaitForSingleObject(arret_fil.0, 0) == WAIT_OBJECT_0 {
                break;
            }
            let _ = PostMessageW(HWND(fenetre as *mut _), WM_CHANGEMENT, WPARAM(jeton as usize), LPARAM(0));
        }
        let _ = FindCloseChangeNotification(changement);
    });
    Some(Surveillant { arret })
}
