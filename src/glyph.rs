// Progression des copies dans la barre des tâches : le bouton de la fenêtre se remplit pendant une opération

use std::cell::RefCell;

use windows::Win32::Foundation::HWND;
use windows::Win32::System::Com::{CoCreateInstance, CLSCTX_INPROC_SERVER};
use windows::Win32::UI::Shell::{ITaskbarList3, TaskbarList, TBPF_NOPROGRESS, TBPF_NORMAL};

/// Opération en cours, telle que la barre des tâches la montre
#[derive(Clone, PartialEq)]
pub struct Avancement {
    /// De 0 à 1
    pub part: f32,
}

struct Etat {
    barre: Option<ITaskbarList3>,
    /// Dernière valeur envoyée (sur 1000) : la barre n'est pas rappelée à chaque image
    dernier: Option<u32>,
}

thread_local! {
    static ETAT: RefCell<Option<Etat>> = const { RefCell::new(None) };
}

unsafe fn ouvrir() -> Etat {
    let barre: Option<ITaskbarList3> = CoCreateInstance(&TaskbarList, None, CLSCTX_INPROC_SERVER).ok();
    if let Some(b) = &barre {
        let _ = b.HrInit();
    }
    // Valeur impossible : le premier appel écrit toujours
    Etat { barre, dernier: Some(u32::MAX) }
}

/// `op` : None quand aucune opération n'est en cours
pub fn progression(hwnd: HWND, op: Option<Avancement>) {
    let valeur = op.as_ref().map(|a| (a.part.clamp(0.0, 1.0) * 1000.0) as u32);
    ETAT.with(|e| unsafe {
        let mut e = e.borrow_mut();
        let etat = e.get_or_insert_with(|| ouvrir());
        if etat.dernier == valeur {
            return;
        }
        etat.dernier = valeur;
        if let Some(b) = &etat.barre {
            match valeur {
                Some(v) => {
                    let _ = b.SetProgressState(hwnd, TBPF_NORMAL);
                    let _ = b.SetProgressValue(hwnd, v as u64, 1000);
                }
                None => {
                    let _ = b.SetProgressState(hwnd, TBPF_NOPROGRESS);
                }
            }
        }
    });
}
