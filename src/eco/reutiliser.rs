// Réutiliser une fenêtre SpaceFiles déjà ouverte pour `explorer.open {reuse:true}` (phase 5, lot A). Côté résident :
// trouver la fenêtre (la plus haute, visible, d'un AUTRE processus lancé depuis le même exécutable), lui envoyer le
// chemin par `WM_COPYDATA` (marque `OUVRIR_ICI`) et dire si elle l'a accepté. Côté fenêtre : relire la charge sans
// lui faire confiance (n'importe quel processus du même utilisateur peut écrire à une fenêtre), voir `decoder`, refuser
// une charge devenue trop vieille (le résident s'est déjà replié), borner les fils en vol et déposer les résultats dans
// une file protégée (`FileOuvertures`) plutôt que dans un pointeur posté.
// Tout ce qui décide est pur et testé (charge, âge, choix des fenêtres, oubli d'une fenêtre figée, file, compteur) ;
// seuls `chercher` et `envoyer` touchent à Win32. Appelé depuis le fil de lancement du résident, jamais depuis le crochet
// clavier ni le fil du tube.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

use super::ouvrir::valider_chemin;

/// Marque des messages `WM_COPYDATA` qui portent un chemin à ouvrir ici : « NE » + 2 (voisine de `ONGLETS_COPIES`, « NE » + 1)
pub const OUVRIR_ICI: usize = 0x4E45_0002;
/// Taille maximale de la charge lue, en octets : un chemin fait 32 000 caractères au plus (64 000 octets en UTF-16)
pub const CHARGE_MAX: usize = 64 * 1024;
/// Le résident n'attend pas une fenêtre figée plus longtemps
const DELAI_MS: u32 = 2000;
/// Une charge plus vieille que cela à la réception est refusée sans rien ouvrir : sous les 2 000 ms du résident, qui s'est
/// alors déjà replié sur une nouvelle fenêtre (sinon l'onglet s'ouvrirait en double)
pub const AGE_MAX_MS: u64 = 1500;
/// Au plus ce nombre de fenêtres examinées (une boucle de `FindWindowEx` ne doit jamais tourner sans fin)
const FENETRES_MAX: usize = 64;
/// Fenêtres essayées au plus, quand la précédente refuse l'accès (UIPI : fenêtre élevée)
const ESSAIS_MAX: usize = 3;
/// Une fenêtre dont l'envoi a expiré est sautée pendant ce temps
pub const OUBLI_MS: u64 = 10_000;
/// Fils de consultation du disque en vol dans une même fenêtre ; au-delà la fenêtre refuse (le résident se replie)
pub const FILS_MAX: usize = 4;
/// Résultats en attente dans la file d'une fenêtre (bornée : une fenêtre qui ne les vide pas ne fait pas grossir la file)
const FILE_MAX: usize = 16;
/// `ERROR_ACCESS_DENIED` (UIPI : un processus normal n'envoie pas de message à une fenêtre élevée)
const ACCES_REFUSE: u32 = 5;

// ───────────────────────── Charge (pure) ─────────────────────────

/// Ce que contient une charge `OUVRIR_ICI` relue : chemin validé et normalisé, `reveler`, instant d'envoi (`GetTickCount64`)
#[derive(Debug, PartialEq, Clone)]
pub struct Charge {
    pub chemin: String,
    pub reveler: bool,
    pub tick: u64,
}

/// Charge UTF-16 : `"<0|1>\n<tick>\n<chemin>"`. `1` : montrer l'élément (`reveler`), `0` : ouvrir le dossier. `tick` : instant
/// d'envoi en millisecondes (`GetTickCount64`, commun à tous les processus de la session), pour que la fenêtre refuse une
/// charge qu'elle traite trop tard.
pub fn encoder(chemin: &str, reveler: bool, tick: u64) -> Vec<u16> {
    format!("{}\n{tick}\n{chemin}", if reveler { '1' } else { '0' }).encode_utf16().collect()
}

/// Relit la charge reçue par une fenêtre, ou None. Refuse tout ce qui n'est pas exactement ce que `encoder` produit : plus
/// de 64 Ko, longueur impaire (UTF-16 tronqué), surrogate isolé, pas de `\n`, drapeau autre que `0` / `1`, instant qui n'est
/// pas un entier décimal, chemin que `valider_chemin` refuse (relatif, programme, URL, périphérique, contrôle…).
pub fn decoder(octets: &[u8]) -> Option<Charge> {
    if octets.len() > CHARGE_MAX || octets.len() % 2 != 0 {
        return None;
    }
    let unites: Vec<u16> = octets.chunks_exact(2).map(|p| u16::from_le_bytes([p[0], p[1]])).collect();
    // Strict : un surrogate isolé est un refus, pas un caractère de remplacement
    let texte = String::from_utf16(&unites).ok()?;
    let (drapeau, reste) = texte.split_once('\n')?;
    let (tick, chemin) = reste.split_once('\n')?;
    let reveler = match drapeau {
        "1" => true,
        "0" => false,
        _ => return None,
    };
    if tick.is_empty() || tick.len() > 20 || !tick.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let tick = tick.parse::<u64>().ok()?;
    valider_chemin(chemin).ok().map(|chemin| Charge { chemin, reveler, tick })
}

/// La charge est-elle trop vieille pour être ouverte ? Un instant « dans le futur » (horloge forgée) compte pour un âge nul :
/// le chemin est de toute façon revalidé.
pub fn trop_ancienne(tick_envoi: u64, maintenant: u64) -> bool {
    maintenant.saturating_sub(tick_envoi) > AGE_MAX_MS
}

// ───────────────────────── Choix de la fenêtre (pur) ─────────────────────────

/// Une fenêtre de premier niveau trouvée, avec ce qu'il faut pour décider
#[derive(Debug, Clone, PartialEq)]
pub struct Candidat {
    pub fenetre: isize,
    pub visible: bool,
    pub pid: u32,
    /// Exécutable du processus (None si Windows refuse de le dire)
    pub exe: Option<String>,
}

/// Ce que le choix exige
#[derive(Debug, Clone, PartialEq)]
pub enum Regle {
    /// Usage normal : visible, autre processus que le résident, même exécutable que lui (insensible à la casse)
    Normale { pid_resident: u32, exe_resident: String },
    /// Mode essai (`SPACEFILES_PIPE` posée) : seule l'exclusion du résident reste
    Essai { pid_resident: u32 },
}

/// Les fenêtres qui conviennent, dans l'ordre reçu (ordre Z, la plus haute d'abord), au plus `max`, sans celles que
/// `ignorer` écarte (fenêtre figée récemment)
pub fn choisir_plusieurs(candidats: &[Candidat], regle: &Regle, max: usize, ignorer: impl Fn(isize) -> bool) -> Vec<isize> {
    candidats
        .iter()
        .filter(|c| match regle {
            Regle::Normale { pid_resident, exe_resident } => {
                c.visible && c.pid != *pid_resident && c.exe.as_deref().is_some_and(|e| e.eq_ignore_ascii_case(exe_resident))
            }
            Regle::Essai { pid_resident } => c.pid != *pid_resident,
        })
        .map(|c| c.fenetre)
        .filter(|&h| !ignorer(h))
        .take(max)
        .collect()
}

/// La première fenêtre qui convient
#[cfg(test)]
pub fn choisir(candidats: &[Candidat], regle: &Regle) -> Option<isize> {
    choisir_plusieurs(candidats, regle, 1, |_| false).first().copied()
}

/// Classe à chercher en mode essai : `SPACEFILES_CLASSE_CIBLE`, lue SEULEMENT si `SPACEFILES_PIPE` est posée (`essai`).
/// En usage normal la variable est ignorée, même posée.
pub fn classe_cible_pour(essai: bool, variable: Option<std::ffi::OsString>) -> Option<String> {
    if !essai {
        return None;
    }
    variable.and_then(|v| v.into_string().ok()).filter(|v| !v.is_empty())
}

/// Issue d'un envoi à une fenêtre
#[derive(Debug, PartialEq, Clone, Copy)]
pub enum Envoi {
    /// La fenêtre a répondu `1`
    Accepte,
    /// La fenêtre a répondu autre chose (`0`) : repli, on n'essaie pas la suivante
    Refuse,
    /// Délai dépassé ou fenêtre figée : repli, la fenêtre est oubliée un moment
    Expire,
    /// Échec immédiat pour accès refusé (UIPI) : on peut essayer la fenêtre suivante
    AccesRefuse,
}

/// Classe le retour de `SendMessageTimeoutW` : `retour` (0 = échec ou délai), `reponse` (résultat du message), `erreur`
/// (`GetLastError` juste après l'appel)
pub fn classer(retour: isize, reponse: usize, erreur: u32) -> Envoi {
    if retour != 0 {
        if reponse == 1 {
            Envoi::Accepte
        } else {
            Envoi::Refuse
        }
    } else if erreur == ACCES_REFUSE {
        Envoi::AccesRefuse
    } else {
        Envoi::Expire
    }
}

/// Dernière fenêtre dont l'envoi a expiré : sautée pendant `OUBLI_MS` pour qu'un résident qui reçoit plusieurs demandes
/// ne reste pas 2 s sur chacune devant une fenêtre figée. L'horloge est donnée par l'appelant (testable).
#[derive(Debug, Default)]
pub struct Oubli(Option<(isize, u64)>);

impl Oubli {
    pub const fn vide() -> Oubli {
        Oubli(None)
    }

    pub fn noter(&mut self, fenetre: isize, maintenant_ms: u64) {
        self.0 = Some((fenetre, maintenant_ms));
    }

    pub fn ignore(&self, fenetre: isize, maintenant_ms: u64) -> bool {
        self.0.is_some_and(|(h, t)| h == fenetre && maintenant_ms.saturating_sub(t) < OUBLI_MS)
    }
}

// ───────────────────────── Côté fenêtre : file et compteur (purs) ─────────────────────────

/// Résultat d'une consultation du disque : dossier à ouvrir, élément à choisir
pub type Ouverture = (String, Option<String>);

/// Résultats déposés par les fils courts, retirés par la fenêtre à qui ils sont destinés. Le message posté à la fenêtre ne
/// porte aucun pointeur : un message forgé ne fait que vider cette file plus tôt.
pub type FileOuvertures = Mutex<VecDeque<(isize, Ouverture)>>;

/// Dépose un résultat pour cette fenêtre. Faux (rien déposé) si la file est pleine.
pub fn deposer(file: &FileOuvertures, fenetre: isize, o: Ouverture) -> bool {
    let mut f = file.lock().unwrap_or_else(|e| e.into_inner());
    if f.len() >= FILE_MAX {
        return false;
    }
    f.push_back((fenetre, o));
    true
}

/// Retire, dans l'ordre, les résultats de cette fenêtre ; ceux des autres restent
pub fn vider(file: &FileOuvertures, fenetre: isize) -> Vec<Ouverture> {
    let mut f = file.lock().unwrap_or_else(|e| e.into_inner());
    let mut pris = Vec::new();
    f.retain(|(h, o)| {
        if *h == fenetre {
            pris.push(o.clone());
            false
        } else {
            true
        }
    });
    pris
}

/// Réserve une place de fil (au plus `max` en vol). À rendre avec `liberer`.
pub fn reserver(compteur: &AtomicUsize, max: usize) -> bool {
    compteur.try_update(Ordering::SeqCst, Ordering::SeqCst, |n| (n < max).then_some(n + 1)).is_ok()
}

pub fn liberer(compteur: &AtomicUsize) {
    let _ = compteur.try_update(Ordering::SeqCst, Ordering::SeqCst, |n| n.checked_sub(1));
}

// ───────────────────────── Win32 ─────────────────────────

use windows::core::{HSTRING, PCWSTR, PWSTR};
use windows::Win32::Foundation::{CloseHandle, GetLastError, SetLastError, HANDLE, HWND, LPARAM, WIN32_ERROR, WPARAM};
use windows::Win32::System::DataExchange::COPYDATASTRUCT;
use windows::Win32::System::SystemInformation::GetTickCount64;
use windows::Win32::System::Threading::{
    GetCurrentProcess, OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
};
use windows::Win32::UI::WindowsAndMessaging::{
    AllowSetForegroundWindow, FindWindowExW, GetWindowThreadProcessId, IsWindowVisible, SendMessageTimeoutW, HWND_MESSAGE, SMTO_ABORTIFHUNG, WM_COPYDATA,
};

/// Horloge commune de l'envoi et de la réception (`GetTickCount64`, millisecondes depuis le démarrage de Windows)
pub fn maintenant_ms() -> u64 {
    // SAFETY : appel sans argument ni pointeur.
    unsafe { GetTickCount64() }
}

/// Chemin de l'exécutable d'un processus dont on a un handle autorisé à répondre à la question
fn exe_du_handle(h: HANDLE) -> Option<String> {
    let mut tampon = vec![0u16; 4096];
    let mut taille = tampon.len() as u32;
    // SAFETY : `h` est un handle de processus valide (appelant) ; le tampon fait `taille` unités, Windows écrit au plus cela et
    // `taille` revient avec le nombre réellement écrit.
    unsafe { QueryFullProcessImageNameW(h, PROCESS_NAME_WIN32, PWSTR(tampon.as_mut_ptr()), &mut taille) }.ok()?;
    Some(String::from_utf16_lossy(&tampon[..taille as usize]))
}

/// Chemin de l'exécutable d'un autre processus
fn exe_du_processus(pid: u32) -> Option<String> {
    // SAFETY : handle limité aux questions, fermé avant de sortir.
    unsafe {
        let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
        let r = exe_du_handle(h);
        let _ = CloseHandle(h);
        r
    }
}

/// Exécutable du résident, dans la MÊME forme que celui des fenêtres (`QueryFullProcessImageNameW` des deux côtés : pas
/// `current_exe`, qui peut différer par un préfixe ou une jonction)
fn exe_du_resident() -> Option<String> {
    // SAFETY : `GetCurrentProcess` rend un pseudo-handle toujours valide pour ce processus, qui n'a pas à être fermé.
    exe_du_handle(unsafe { GetCurrentProcess() })
}

/// Les fenêtres de premier niveau de cette classe, la plus haute d'abord (ou les fenêtres message-only en mode essai)
fn enumerer(classe: &HSTRING, message_seul: bool, avec_exe: bool) -> Vec<Candidat> {
    let mut v = Vec::new();
    let parent = if message_seul { HWND_MESSAGE } else { HWND(std::ptr::null_mut()) };
    let mut h = HWND(std::ptr::null_mut());
    // SAFETY : `classe` vit pendant toute la boucle ; FindWindowExW n'écrit rien chez nous, un handle périmé donne seulement
    // une erreur (la fenêtre a pu disparaître entre deux appels : on s'arrête alors).
    unsafe {
        while v.len() < FENETRES_MAX {
            let suivant = FindWindowExW(parent, h, PCWSTR(classe.as_ptr()), PCWSTR::null());
            let Ok(s) = suivant else { break };
            if s.is_invalid() {
                break;
            }
            h = s;
            let mut pid = 0u32;
            GetWindowThreadProcessId(h, Some(&mut pid));
            v.push(Candidat { fenetre: h.0 as isize, visible: IsWindowVisible(h).as_bool(), pid, exe: if avec_exe { exe_du_processus(pid) } else { None } });
        }
    }
    v
}

/// Les fenêtres à essayer (3 au plus), avec le pid de chacune
fn chercher(maintenant: u64) -> Vec<(HWND, u32)> {
    let pid_resident = std::process::id();
    let cible = classe_cible_pour(super::essai().is_some(), std::env::var_os("SPACEFILES_CLASSE_CIBLE"));
    let (candidats, regle) = match cible {
        Some(classe) => (enumerer(&HSTRING::from(classe), true, false), Regle::Essai { pid_resident }),
        None => {
            let Some(exe) = exe_du_resident() else { return Vec::new() };
            (enumerer(&HSTRING::from("SpaceFiles"), false, true), Regle::Normale { pid_resident, exe_resident: exe })
        }
    };
    let oubli = OUBLI.lock().unwrap_or_else(|e| e.into_inner());
    choisir_plusieurs(&candidats, &regle, ESSAIS_MAX, |h| oubli.ignore(h, maintenant))
        .into_iter()
        .map(|h| (HWND(h as *mut _), candidats.iter().find(|c| c.fenetre == h).map_or(0, |c| c.pid)))
        .collect()
}

static OUBLI: Mutex<Oubli> = Mutex::new(Oubli::vide());

/// Envoie la charge à une fenêtre et classe l'issue
fn envoyer(cible: HWND, chemin: &str, reveler: bool) -> Envoi {
    // L'instant est pris à chaque envoi : une fenêtre essayée après une autre n'hérite pas d'un âge déjà entamé
    let charge = encoder(chemin, reveler, maintenant_ms());
    let cd = COPYDATASTRUCT { dwData: OUVRIR_ICI, cbData: (charge.len() * 2) as u32, lpData: charge.as_ptr() as *mut _ };
    let mut reponse = 0usize;
    // SAFETY : `cd` et `charge` vivent jusqu'au retour de l'appel (WM_COPYDATA est synchrone : le destinataire copie ce
    // qu'il garde avant de répondre). SMTO_ABORTIFHUNG + 2 s : une fenêtre figée ne bloque pas ce fil plus longtemps.
    // `SetLastError(0)` avant : sans cela une ancienne erreur serait prise pour celle de l'appel.
    let (r, erreur) = unsafe {
        SetLastError(WIN32_ERROR(0));
        let r = SendMessageTimeoutW(cible, WM_COPYDATA, WPARAM(0), LPARAM(&cd as *const _ as isize), SMTO_ABORTIFHUNG, DELAI_MS, Some(&mut reponse));
        (r, GetLastError().0)
    };
    classer(r.0, reponse, erreur)
}

/// Confie ce chemin à une fenêtre SpaceFiles ouverte. Vrai seulement si elle a répondu `1` (acceptée) ; tout le reste
/// (aucune fenêtre, `0`, délai de 2 s, fenêtre figée) rend faux et l'appelant lance une nouvelle fenêtre. Une fenêtre qui
/// refuse l'accès tout de suite (UIPI) fait essayer la suivante (3 au plus) ; un délai ou un `0` non.
pub fn reutiliser_fenetre(chemin: &str, reveler: bool) -> bool {
    let cibles = chercher(maintenant_ms());
    if cibles.is_empty() {
        super::trace(crate::journal::Niveau::Info, "eco-ouvrir", "reuse : aucune fenetre a reutiliser");
        return false;
    }
    for (cible, pid) in cibles {
        // SAFETY : appel sans pointeur ; le droit de premier plan est cédé à la fenêtre cible pour qu'elle puisse se montrer.
        // Un refus de Windows n'est pas une erreur : la fenêtre s'ouvre quand même, sans premier plan garanti.
        let accorde = unsafe { AllowSetForegroundWindow(pid) }.is_ok();
        super::trace(
            crate::journal::Niveau::Info,
            "eco-ouvrir",
            if accorde { "reuse : premier plan accorde a la fenetre cible" } else { "reuse : premier plan refuse par Windows pour la fenetre cible" },
        );
        match envoyer(cible, chemin, reveler) {
            Envoi::Accepte => {
                super::trace(crate::journal::Niveau::Info, "eco-ouvrir", "reuse : chemin accepte par la fenetre");
                return true;
            }
            Envoi::Refuse => {
                super::trace(crate::journal::Niveau::Info, "eco-ouvrir", "reuse : fenetre qui refuse, repli sur une nouvelle fenetre");
                return false;
            }
            Envoi::Expire => {
                OUBLI.lock().unwrap_or_else(|e| e.into_inner()).noter(cible.0 as isize, maintenant_ms());
                super::trace(crate::journal::Niveau::Avert, "eco-ouvrir", "reuse : fenetre sans reponse, repli sur une nouvelle fenetre");
                return false;
            }
            Envoi::AccesRefuse => super::trace(crate::journal::Niveau::Info, "eco-ouvrir", "reuse : acces refuse a une fenetre, essai de la suivante"),
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    fn octets(u: &[u16]) -> Vec<u8> {
        u.iter().flat_map(|x| x.to_le_bytes()).collect()
    }

    fn unites(s: &str) -> Vec<u8> {
        octets(&s.encode_utf16().collect::<Vec<_>>())
    }

    fn charge(chemin: &str, reveler: bool, tick: u64) -> Option<Charge> {
        Some(Charge { chemin: chemin.to_string(), reveler, tick })
    }

    #[test]
    fn charge_aller_retour() {
        for (chemin, reveler, tick) in [(r"C:\Users", false, 0u64), (r"C:\a b\c.txt", true, 12_345), (r"\\hote\partage", false, 1), (r"C:\été\日本語 😀", true, u64::MAX), (r"C:\", false, 99)] {
            let u = encoder(chemin, reveler, tick);
            assert_eq!(decoder(&octets(&u)), charge(chemin, reveler, tick), "{chemin}");
        }
        assert_eq!(encoder(r"C:\x", true, 42), "1\n42\nC:\\x".encode_utf16().collect::<Vec<_>>());
        assert_eq!(encoder(r"C:\x", false, 7), "0\n7\nC:\\x".encode_utf16().collect::<Vec<_>>());
        // Les `/` sont normalisés comme pour la demande du tube : c'est le chemin normalisé qui est ouvert
        assert_eq!(decoder(&unites("0\n5\nC:/Users/nico")), charge(r"C:\Users\nico", false, 5));
    }

    #[test]
    fn charge_hostile_refusee() {
        // Pas de `\n`, un seul `\n`, drapeau inconnu ou vide, instant absent ou mal formé, chemin vide
        for t in [
            "", "0", "1C:\\x", "0C:\\x\n", "2\n1\nC:\\x", "\n1\nC:\\x", "x\n1\nC:\\x", "01\n1\nC:\\x", "0\n1\n", "1\n1\n", "0\nC:\\x", "0\n\nC:\\x", "0\n-1\nC:\\x",
            "0\n+1\nC:\\x", "0\n1.5\nC:\\x", "0\n 1\nC:\\x", "0\n0x10\nC:\\x", "0\n18446744073709551616\nC:\\x", "0\n000000000000000000001\nC:\\x", "0\nabc\nC:\\x",
        ] {
            assert_eq!(decoder(&unites(t)), None, "{t:?}");
        }
        // Chemins que la demande du tube refuserait aussi
        for t in ["calc.exe", "relatif\\x", "https://x", "\\\\.\\pipe\\x", "C:\\a:flux", "C:\\NUL", "C:\\a\u{0}b", "C:\\a\nb", "C:\\a\u{202e}b", "C:\\a?"] {
            assert_eq!(decoder(&unites(&format!("0\n1\n{t}"))), None, "{t:?}");
            assert_eq!(decoder(&unites(&format!("1\n1\n{t}"))), None, "{t:?}");
        }
        // UTF-16 tronqué : un octet de trop, ou un surrogate isolé (haut sans bas, bas seul)
        let mut impair = unites("0\n1\nC:\\x");
        impair.push(0x41);
        assert_eq!(decoder(&impair), None, "longueur impaire");
        let mut haut_isole = encoder(r"C:\x", false, 1);
        haut_isole.push(0xD83D);
        assert_eq!(decoder(&octets(&haut_isole)), None, "surrogate haut en fin");
        let mut bas_isole = encoder(r"C:\x", false, 1);
        let n = bas_isole.len();
        bas_isole.insert(n - 1, 0xDE00);
        assert_eq!(decoder(&octets(&bas_isole)), None, "surrogate bas isolé");
        let tout = octets(&"0\n1\nC:\\😀".encode_utf16().collect::<Vec<_>>());
        assert_eq!(decoder(&tout[..tout.len() - 2]), None, "moitié d'un emoji");
        // Trop long : plus de 64 Ko, même si tout le reste est correct
        let enorme = unites(&format!("0\n1\nC:\\{}", "d".repeat(CHARGE_MAX)));
        assert!(enorme.len() > CHARGE_MAX);
        assert_eq!(decoder(&enorme), None, "au-delà de 64 Ko");
        // 32 000 caractères ou plus : refusés par valider_chemin même sous 64 Ko
        let long = unites(&format!("0\n1\nC:\\{}", "d".repeat(32_000)));
        assert!(long.len() <= CHARGE_MAX);
        assert_eq!(decoder(&long), None, "32 000 caractères");
        // Un chemin valide de 31 000 caractères passe
        let borne = unites(&format!("0\n1\nC:\\{}", "d".repeat(31_000)));
        assert!(decoder(&borne).is_some());
    }

    /// Une charge traitée trop tard (le résident s'est déjà replié) n'ouvre rien
    #[test]
    fn age_de_la_charge() {
        assert!(!trop_ancienne(10_000, 10_000));
        assert!(!trop_ancienne(10_000, 11_500), "1 500 ms pile : encore acceptée");
        assert!(trop_ancienne(10_000, 11_501));
        assert!(trop_ancienne(10_000, 14_000), "traitée après un blocage de 4 s");
        assert!(!trop_ancienne(20_000, 10_000), "instant dans le futur : âge nul, le chemin reste revalidé");
        assert!(AGE_MAX_MS < DELAI_MS as u64, "la fenêtre refuse avant que le résident ait renoncé");
    }

    #[test]
    fn marque_distincte_de_celle_des_onglets() {
        assert_ne!(OUVRIR_ICI, crate::ONGLETS_COPIES);
        assert_eq!(OUVRIR_ICI, crate::ONGLETS_COPIES + 1);
    }

    #[test]
    fn la_fenetre_cachee_du_resident_n_a_pas_la_classe_des_fenetres() {
        // Si les deux classes étaient égales, le résident serait pris pour une fenêtre à réutiliser (il est de toute façon
        // exclu par son pid, mais ce serait une erreur de conception)
        // SAFETY : `CLASSE` est un littéral `w!` terminé par 0.
        let classe_resident = unsafe { crate::raccourci::CLASSE.to_string() }.unwrap_or_default();
        assert!(!classe_resident.is_empty());
        assert_ne!(classe_resident, "SpaceFiles");
    }

    fn c(fenetre: isize, visible: bool, pid: u32, exe: Option<&str>) -> Candidat {
        Candidat { fenetre, visible, pid, exe: exe.map(str::to_string) }
    }

    fn normale() -> Regle {
        Regle::Normale { pid_resident: 100, exe_resident: r"C:\Apps\SpaceFiles\spacefiles.exe".into() }
    }

    #[test]
    fn choix_de_la_fenetre() {
        let bon = r"C:\Apps\SpaceFiles\spacefiles.exe";
        // La première qui convient, dans l'ordre reçu (la plus haute d'abord)
        assert_eq!(choisir(&[c(1, true, 200, Some(bon)), c(2, true, 300, Some(bon))], &normale()), Some(1));
        // Exclusion du résident (même processus), de l'invisible, d'un autre exe, d'un exe inconnu
        assert_eq!(choisir(&[c(1, true, 100, Some(bon)), c(2, true, 300, Some(bon))], &normale()), Some(2), "même processus que le résident");
        assert_eq!(choisir(&[c(1, false, 200, Some(bon)), c(2, true, 300, Some(bon))], &normale()), Some(2), "invisible");
        assert_eq!(choisir(&[c(1, true, 200, Some(r"C:\Autre\spacefiles.exe")), c(2, true, 300, Some(bon))], &normale()), Some(2), "exe différent");
        assert_eq!(choisir(&[c(1, true, 200, None), c(2, true, 300, Some(bon))], &normale()), Some(2), "exe inconnu");
        // Casse du chemin sans importance
        assert_eq!(choisir(&[c(1, true, 200, Some(r"c:\apps\spacefiles\SPACEFILES.EXE"))], &normale()), Some(1));
        // Rien ne convient
        assert_eq!(choisir(&[], &normale()), None);
        assert_eq!(choisir(&[c(1, true, 100, Some(bon)), c(2, false, 200, Some(bon)), c(3, true, 300, Some(r"C:\x.exe"))], &normale()), None);
    }

    #[test]
    fn choix_de_plusieurs_fenetres() {
        let bon = r"C:\Apps\SpaceFiles\spacefiles.exe";
        let toutes: Vec<Candidat> = (1..=6).map(|i| c(i, true, 200 + i as u32, Some(bon))).collect();
        assert_eq!(choisir_plusieurs(&toutes, &normale(), ESSAIS_MAX, |_| false), vec![1, 2, 3], "trois au plus, dans l'ordre Z");
        // Une fenêtre oubliée est sautée, la suivante prend sa place dans les trois
        assert_eq!(choisir_plusieurs(&toutes, &normale(), ESSAIS_MAX, |h| h == 1), vec![2, 3, 4]);
        assert_eq!(choisir_plusieurs(&toutes, &normale(), ESSAIS_MAX, |_| true), Vec::<isize>::new());
        assert_eq!(choisir_plusieurs(&toutes[..2], &normale(), ESSAIS_MAX, |_| false), vec![1, 2]);
    }

    #[test]
    fn choix_en_mode_essai() {
        let r = Regle::Essai { pid_resident: 100 };
        // Ni visibilité ni exécutable exigés, mais le résident reste exclu
        assert_eq!(choisir(&[c(1, false, 200, None)], &r), Some(1));
        assert_eq!(choisir(&[c(1, false, 100, None), c(2, false, 200, Some(r"C:\x.exe"))], &r), Some(2));
        assert_eq!(choisir(&[c(1, true, 100, None)], &r), None);
    }

    /// `SPACEFILES_CLASSE_CIBLE` n'existe qu'en mode essai : sans `SPACEFILES_PIPE`, jamais
    #[test]
    fn classe_d_essai_reservee_au_mode_essai() {
        use std::ffi::OsString;
        let v = || Some(OsString::from("ClasseEssai"));
        assert_eq!(classe_cible_pour(false, v()), None, "usage normal : ignorée");
        assert_eq!(classe_cible_pour(true, None), None);
        assert_eq!(classe_cible_pour(true, Some(OsString::new())), None);
        assert_eq!(classe_cible_pour(true, v()), Some("ClasseEssai".to_string()));
    }

    /// Seul un échec immédiat pour accès refusé passe à la fenêtre suivante ; un délai ou un `0` non
    #[test]
    fn issue_d_un_envoi() {
        assert_eq!(classer(1, 1, 0), Envoi::Accepte);
        assert_eq!(classer(1, 0, 0), Envoi::Refuse, "réponse 0");
        assert_eq!(classer(1, 2, 0), Envoi::Refuse, "toute réponse autre que 1");
        assert_eq!(classer(0, 0, ACCES_REFUSE), Envoi::AccesRefuse, "UIPI");
        assert_eq!(classer(0, 0, 1460), Envoi::Expire, "ERROR_TIMEOUT");
        assert_eq!(classer(0, 0, 0), Envoi::Expire, "fenêtre figée (SMTO_ABORTIFHUNG)");
        assert_eq!(classer(0, 1, 0), Envoi::Expire, "la réponse n'est pas lue quand l'appel a échoué");
    }

    /// Une fenêtre dont l'envoi a expiré est sautée 10 s, pas une autre, pas plus longtemps
    #[test]
    fn oubli_d_une_fenetre_figee() {
        let mut o = Oubli::vide();
        assert!(!o.ignore(7, 0), "rien noté");
        o.noter(7, 1_000);
        assert!(o.ignore(7, 1_000));
        assert!(o.ignore(7, 10_999));
        assert!(!o.ignore(7, 11_000), "10 s écoulées");
        assert!(!o.ignore(8, 1_500), "une autre fenêtre n'est pas concernée");
        // Une horloge qui recule ne fait pas oublier plus tôt ni planter
        assert!(o.ignore(7, 0));
        // La dernière note remplace la précédente
        o.noter(8, 20_000);
        assert!(o.ignore(8, 20_100));
        assert!(!o.ignore(7, 20_100));
    }

    #[test]
    fn file_des_ouvertures() {
        let file: FileOuvertures = Mutex::new(VecDeque::new());
        let o = |d: &str, n: Option<&str>| (d.to_string(), n.map(str::to_string));
        assert!(vider(&file, 1).is_empty(), "file vide");
        assert!(deposer(&file, 1, o(r"C:\a", None)));
        assert!(deposer(&file, 2, o(r"C:\b", Some("x"))));
        assert!(deposer(&file, 1, o(r"C:\c", Some("y"))));
        // Chaque fenêtre ne retire que ses résultats, dans l'ordre de dépôt
        assert_eq!(vider(&file, 1), vec![o(r"C:\a", None), o(r"C:\c", Some("y"))]);
        assert!(vider(&file, 1).is_empty(), "retirés une seule fois (un message forgé ne fait rien de plus)");
        assert_eq!(vider(&file, 2), vec![o(r"C:\b", Some("x"))]);
        assert!(vider(&file, 3).is_empty(), "une fenêtre inconnue ne reçoit rien");
        // Bornée
        for k in 0..FILE_MAX {
            assert!(deposer(&file, 9, o(&format!(r"C:\{k}"), None)));
        }
        assert!(!deposer(&file, 9, o(r"C:\trop", None)), "file pleine : refusée");
        assert_eq!(vider(&file, 9).len(), FILE_MAX);
        assert!(deposer(&file, 9, o(r"C:\apres", None)));
    }

    #[test]
    fn fils_en_vol_bornes() {
        let n = AtomicUsize::new(0);
        for _ in 0..FILS_MAX {
            assert!(reserver(&n, FILS_MAX));
        }
        assert!(!reserver(&n, FILS_MAX), "au-delà de 4 : refusé");
        assert_eq!(n.load(Ordering::SeqCst), FILS_MAX, "un refus ne prend pas de place");
        liberer(&n);
        assert!(reserver(&n, FILS_MAX), "une place rendue est réutilisable");
        for _ in 0..10 {
            liberer(&n);
        }
        assert_eq!(n.load(Ordering::SeqCst), 0, "jamais sous zéro");
        assert!(reserver(&n, FILS_MAX));
    }
}
