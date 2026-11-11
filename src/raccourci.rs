// Win+E sans passer par Windows : un petit processus résident (lancé avec --resident à l'ouverture de session)
// intercepte la combinaison au clavier, comme PowerToys, et ouvre SpaceFiles.
// Le chemin de Windows (objet « Explorateur de fichiers ») est trop souvent détourné ou cassé par les mods.
// Alt+Espace n'est plus géré ici : le lanceur appartient à SpaceNotch, qui interroge le service de recherche du
// résident par le tube nommé (`eco` : recherche, ouverture d'un dossier). Le crochet ne regarde donc que Win et E.

use std::sync::atomic::{AtomicBool, AtomicIsize, Ordering};

use windows::core::{w, HSTRING};
use windows::Win32::Foundation::*;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Threading::CreateMutexW;
use windows::Win32::UI::Input::KeyboardAndMouse::*;
use windows::Win32::UI::Shell::ShellExecuteW;
use windows::Win32::UI::WindowsAndMessaging::*;

pub const CLASSE: windows::core::PCWSTR = w!("SpaceFilesResident");
const WM_OUVRIR: u32 = WM_APP + 20;
/// « Supprimer l'index » demandé par une fenêtre : le résident efface son index en mémoire (WM_APP + 23 est pris par le service)
const WM_EFFACER_INDEX: u32 = WM_APP + 24;

static WIN_ENFONCEE: AtomicBool = AtomicBool::new(false);
static E_AVALEE: AtomicBool = AtomicBool::new(false);
static mut FENETRE: HWND = HWND(std::ptr::null_mut());
/// Le crochet clavier posé (0 : aucun) : décroché AVANT l'arrêt de `eco`, jamais deux fois
static CROCHET: AtomicIsize = AtomicIsize::new(0);

/// Touche « vide » (non attribuée) : glissée entre Win enfoncée et relâchée, elle empêche le menu Démarrer de s'ouvrir
fn masquer_demarrer() {
    let touche = |haut: bool| INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT { wVk: VIRTUAL_KEY(0xE8), wScan: 0, dwFlags: if haut { KEYEVENTF_KEYUP } else { Default::default() }, time: 0, dwExtraInfo: 0 },
        },
    };
    unsafe {
        SendInput(&[touche(false), touche(true)], std::mem::size_of::<INPUT>() as i32);
    }
}

/// Les seules touches que le crochet regarde : Win (suivi de son état) et E (Win+E). Toute autre, Espace compris,
/// passe à Windows sans être lue : Alt+Espace appartient à SpaceNotch (`RegisterHotKey`).
fn touche_surveillee(vk: VIRTUAL_KEY) -> bool {
    matches!(vk, VK_LWIN | VK_RWIN | VK_E)
}

/// Ce que le crochet suit entre deux touches
struct Etat {
    win_enfoncee: bool,
    e_avalee: bool,
}

#[derive(Debug, PartialEq)]
enum Decision {
    /// La touche continue vers Windows
    Passer,
    /// La touche est avalée
    Avaler,
    /// Win+E : avalée, et SpaceFiles doit s'ouvrir
    Ouvrir,
}

/// Décision du crochet pour une touche Win ou E, sans toucher au système (testable). `win_physique` : Win est
/// vraiment enfoncée d'après `GetAsyncKeyState` ; `autres` : Maj, Ctrl ou Alt le sont aussi.
fn decider(etat: &mut Etat, vk: VIRTUAL_KEY, bas: bool, win_physique: bool, autres: bool) -> Decision {
    match vk {
        VK_LWIN | VK_RWIN => {
            etat.win_enfoncee = bas;
            Decision::Passer
        }
        // Win relâchée avant E : win_enfoncee est déjà faux, et sans ce bras le relâchement de E passait sans rien
        // défaire, laissait e_avalee vrai ; le Win+E suivant était avalé sans rien poster et Windows ouvrait le menu Démarrer
        VK_E if !bas && std::mem::take(&mut etat.e_avalee) => Decision::Avaler,
        // Le relâchement de Win peut nous échapper (Win+L : il arrive sur l'écran de verrouillage) ; sans cette
        // vérification de l'état réel, chaque E seul serait avalé et ouvrirait l'explorateur
        VK_E if etat.win_enfoncee && !win_physique => {
            etat.win_enfoncee = false;
            Decision::Passer
        }
        // Win+E seul (pas Win+Maj+E…) : on l'avale ; la répétition de la touche tenue n'ouvre qu'une fois
        VK_E if etat.win_enfoncee && bas && !autres => {
            if std::mem::replace(&mut etat.e_avalee, true) {
                Decision::Avaler
            } else {
                Decision::Ouvrir
            }
        }
        _ => Decision::Passer,
    }
}

unsafe extern "system" fn crochet(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code == HC_ACTION as i32 {
        // SAFETY : avec HC_ACTION, lparam pointe vers un KBDLLHOOKSTRUCT valide le temps de l'appel (contrat de WH_KEYBOARD_LL)
        let k = &*(lparam.0 as *const KBDLLHOOKSTRUCT);
        if !touche_surveillee(VIRTUAL_KEY(k.vkCode as u16)) {
            return CallNextHookEx(None, code, wparam, lparam);
        }
        let bas = matches!(wparam.0 as u32, WM_KEYDOWN | WM_SYSKEYDOWN);
        let vk = VIRTUAL_KEY(k.vkCode as u16);
        // Les états physiques ne sont lus que pour E : Win seule n'en a pas besoin
        let (win_physique, autres) = if vk == VK_E {
            (
                GetAsyncKeyState(VK_LWIN.0 as i32) < 0 || GetAsyncKeyState(VK_RWIN.0 as i32) < 0,
                GetAsyncKeyState(VK_SHIFT.0 as i32) < 0 || GetAsyncKeyState(VK_CONTROL.0 as i32) < 0 || GetAsyncKeyState(VK_MENU.0 as i32) < 0,
            )
        } else {
            (false, false)
        };
        let mut etat = Etat { win_enfoncee: WIN_ENFONCEE.load(Ordering::Relaxed), e_avalee: E_AVALEE.load(Ordering::Relaxed) };
        let decision = decider(&mut etat, vk, bas, win_physique, autres);
        WIN_ENFONCEE.store(etat.win_enfoncee, Ordering::Relaxed);
        E_AVALEE.store(etat.e_avalee, Ordering::Relaxed);
        match decision {
            Decision::Passer => {}
            Decision::Avaler => return LRESULT(1),
            // Rien de lent ici (pas d'envoi de touche) : un crochet qui traîne retarde tout le clavier et
            // Windows peut perdre le relâchement de Win. Le travail se fait dans la fenêtre.
            Decision::Ouvrir => {
                let _ = PostMessageW(FENETRE, WM_OUVRIR, WPARAM(0), LPARAM(0));
                return LRESULT(1);
            }
        }
    }
    CallNextHookEx(None, code, wparam, lparam)
}

extern "system" fn procedure(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    unsafe {
        match msg {
            WM_OUVRIR => {
                // Win est encore enfoncée : la touche vide empêche le menu Démarrer de s'ouvrir au relâchement
                masquer_demarrer();
                if let Ok(exe) = std::env::current_exe() {
                    ShellExecuteW(None, None, &HSTRING::from(exe.as_os_str()), None, None, SW_SHOWNORMAL);
                }
                LRESULT(0)
            }
            WM_EFFACER_INDEX => {
                crate::index::effacer_local();
                LRESULT(0)
            }
            // Le service de recherche vient de servir une requête : la libération de l'index repart de zéro
            crate::service_recherche::WM_SERVICE_ACTIVITE => {
                crate::minuteries::armer_liberation(hwnd);
                LRESULT(0)
            }
            WM_TIMER if wparam.0 == crate::minuteries::MINUTERIE_LIBERATION => crate::minuteries::timer_liberation(hwnd),
            WM_CLOSE => {
                let _ = DestroyWindow(hwnd);
                LRESULT(0)
            }
            WM_DESTROY => {
                // Le crochet d'abord (plus aucune touche ne passe par nous), puis les services : ni tube ni fil ne
                // survivent à la fenêtre
                decrocher();
                crate::eco::arreter();
                PostQuitMessage(0);
                LRESULT(0)
            }
            _ => DefWindowProcW(hwnd, msg, wparam, lparam),
        }
    }
}

/// Résident d'essai : `SPACEFILES_PIPE` (absente en usage normal) donne un second résident à un faux profil, sans
/// crochet clavier, avec son propre verrou, sa propre classe de fenêtre et son propre tube : il ne gêne pas le vrai
fn essai() -> Option<String> {
    std::env::var("SPACEFILES_PIPE").ok().filter(|s| !s.is_empty())
}

/// Boucle du processus résident ; revient quand on le ferme (désactivation) ou s'il tourne déjà
pub fn resident() {
    let essai = essai();
    unsafe {
        // Un seul résident par session
        let nom_verrou = match &essai {
            // Nettoyé : un `\` dans le nom d'un objet « Local\ » le ferait échouer
            Some(e) => HSTRING::from(format!("Local\\SpaceFilesResident.{}", crate::eco::assainir(e))),
            None => HSTRING::from("Local\\SpaceFilesResident"),
        };
        let Ok(_verrou) = CreateMutexW(None, true, &nom_verrou) else { return };
        if GetLastError() == ERROR_ALREADY_EXISTS {
            return;
        }
        let Ok(module) = GetModuleHandleW(None) else { return };
        let instance: HINSTANCE = module.into();
        // Le résident d'essai a sa propre classe : `arreter` et `effacer_index` (FindWindow) ne visent jamais lui
        let classe_fenetre = if essai.is_some() { w!("SpaceFilesResidentEssai") } else { CLASSE };
        let classe = WNDCLASSW { lpfnWndProc: Some(procedure), hInstance: instance, lpszClassName: classe_fenetre, ..Default::default() };
        RegisterClassW(&classe);
        // Fenêtre invisible (pas « message-only » : FindWindow doit la trouver pour la fermer)
        let Ok(hwnd) = CreateWindowExW(WINDOW_EX_STYLE::default(), classe_fenetre, classe_fenetre, WINDOW_STYLE::default(), 0, 0, 0, 0, None, None, instance, None) else { return };
        FENETRE = hwnd;
        // Ce que le résident offre aux autres applications (tube Eco multi-instances, manifeste, réglages partagés) :
        // AVANT le crochet, pour que rien de lent (lecture de `shared.json`, écriture du manifeste) ne se trouve entre la
        // pose du crochet et la boucle de messages. L'index n'est chargé qu'à la première recherche.
        crate::eco::demarrer(hwnd);
        // Un résident d'essai ne pose pas de crochet : deux crochets Win+E ouvriraient deux fenêtres
        if essai.is_none() {
            match SetWindowsHookExW(WH_KEYBOARD_LL, Some(crochet), instance, 0) {
                Ok(c) => CROCHET.store(c.0 as isize, Ordering::SeqCst),
                Err(_) => {
                    crate::eco::arreter();
                    return;
                }
            }
        }
        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).into() {
            DispatchMessageW(&msg);
        }
        decrocher();
        crate::eco::arreter();
    }
}

/// Retire le crochet clavier s'il est posé (une seule fois)
fn decrocher() {
    let c = CROCHET.swap(0, Ordering::SeqCst);
    if c != 0 {
        // SAFETY : `c` vient de SetWindowsHookExW et le swap garantit qu'il n'est décroché qu'une fois.
        unsafe {
            let _ = UnhookWindowsHookEx(HHOOK(c as *mut _));
        }
    }
}

/// Prévient le résident qui tourne que l'index supprimé ne doit plus être servi (ni gardé en mémoire)
pub fn effacer_index() {
    unsafe {
        if let Ok(h) = FindWindowW(CLASSE, None) {
            let _ = PostMessageW(h, WM_EFFACER_INDEX, WPARAM(0), LPARAM(0));
        }
    }
}

/// Ferme le résident en cours (désactivation)
pub fn arreter() {
    unsafe {
        if let Ok(h) = FindWindowW(CLASSE, None) {
            let _ = PostMessageW(h, WM_CLOSE, WPARAM(0), LPARAM(0));
        }
    }
}

/// Démarre le résident tout de suite (sans attendre la prochaine ouverture de session)
pub fn demarrer() {
    if let Ok(exe) = std::env::current_exe() {
        unsafe {
            ShellExecuteW(None, None, &HSTRING::from(exe.as_os_str()), w!("--resident"), None, SW_HIDE);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn etat() -> Etat {
        Etat { win_enfoncee: false, e_avalee: false }
    }

    #[test]
    fn win_e_ouvre_une_fois_et_avale_le_relachement() {
        let mut e = etat();
        assert_eq!(decider(&mut e, VK_LWIN, true, true, false), Decision::Passer);
        assert_eq!(decider(&mut e, VK_E, true, true, false), Decision::Ouvrir);
        // Répétition de la touche tenue : avalée, pas de seconde ouverture
        assert_eq!(decider(&mut e, VK_E, true, true, false), Decision::Avaler);
        assert_eq!(decider(&mut e, VK_E, false, true, false), Decision::Avaler);
        assert_eq!(decider(&mut e, VK_LWIN, false, false, false), Decision::Passer);
    }

    /// Win↓ E↓ Win↑ E↑ : le relâchement de E est avalé, et le Win+E suivant ouvre encore
    #[test]
    fn win_relachee_avant_e() {
        let mut e = etat();
        decider(&mut e, VK_LWIN, true, true, false);
        assert_eq!(decider(&mut e, VK_E, true, true, false), Decision::Ouvrir);
        decider(&mut e, VK_LWIN, false, false, false);
        assert_eq!(decider(&mut e, VK_E, false, false, false), Decision::Avaler);
        assert!(!e.e_avalee);
        decider(&mut e, VK_LWIN, true, true, false);
        assert_eq!(decider(&mut e, VK_E, true, true, false), Decision::Ouvrir);
    }

    #[test]
    fn e_seul_et_win_maj_e_passent() {
        let mut e = etat();
        assert_eq!(decider(&mut e, VK_E, true, false, false), Decision::Passer);
        assert_eq!(decider(&mut e, VK_E, false, false, false), Decision::Passer);
        decider(&mut e, VK_LWIN, true, true, false);
        assert_eq!(decider(&mut e, VK_E, true, true, true), Decision::Passer);
        // Win relâchée sans que nous l'ayons vu (Win+L) : E seul n'est plus avalé
        let mut e = Etat { win_enfoncee: true, e_avalee: false };
        assert_eq!(decider(&mut e, VK_E, true, false, false), Decision::Passer);
        assert!(!e.win_enfoncee);
    }

    #[test]
    fn crochet_ne_lit_que_win_et_e() {
        assert!(touche_surveillee(VK_LWIN) && touche_surveillee(VK_RWIN) && touche_surveillee(VK_E));
    }

    #[test]
    fn alt_espace_passe_a_windows() {
        // Ni Espace ni Alt (VK_MENU, VK_LMENU, VK_RMENU) ne sont avalées : SpaceNotch les reçoit par RegisterHotKey
        for t in [VK_SPACE, VK_MENU, VK_LMENU, VK_RMENU, VK_SHIFT, VK_CONTROL, VK_RETURN] {
            assert!(!touche_surveillee(t), "{t:?} ne doit pas être lue par le crochet");
        }
    }
}
