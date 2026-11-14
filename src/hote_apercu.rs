//! Hôte d'aperçu des gestionnaires de Windows (`IPreviewHandler`, P-01) pour le coup d'œil : Word, Excel, PowerPoint, RTF…
//! montrés par le gestionnaire que l'utilisateur a installé, sans que SpaceFiles ne sache les lire.
//!
//! **Désactivé par défaut** (réglage `apercu_systeme`) : c'est du code tiers. Garde-fous :
//! - le gestionnaire n'est activé qu'en `CLSCTX_LOCAL_SERVER` (processus de substitution `prevhost.exe`, comme l'Explorateur) :
//!   un plantage du gestionnaire n'emporte pas SpaceFiles (`panic = "abort"`), et un gestionnaire qui ne s'y prête pas est ignoré ;
//! - tout appel COM se fait sur un **fil dédié** (appartement mono-fil avec sa pompe de messages), jamais sur le fil d'interface ;
//! - délai de 3 s pour obtenir l'aperçu (une veille le fait respecter même si la fenêtre ne se repeint plus) : passé ce délai,
//!   le fil est abandonné (il décharge le gestionnaire dès qu'il rend la main : on ne tue jamais un fil) et le coup d'œil
//!   retombe sur l'aperçu habituel ; au plus 3 fils abandonnés encore bloqués, au-delà la fonction se coupe tant qu'ils durent ;
//! - seules les extensions de `GERE` sont confiées au gestionnaire : PDF, images, texte, HTML gardent leurs visionneuses.
//!
//! - **Cadre** (`IPreviewHandlerFrame`, posé par `IObjectWithSite::SetSite`) : le gestionnaire y renvoie les touches qu'il ne
//!   garde pas. Échap, F1 à F12, Alt+…, Ctrl+… (sauf Ctrl+A et Ctrl+C, laissés à la sélection du gestionnaire) sont reposés à la
//!   fenêtre principale ; Tab rend le clavier à SpaceFiles (`Tab` dans le coup d'œil le donne au gestionnaire : `prendre_le_focus`) ;
//! - **Superpositions** : la fenêtre du gestionnaire est une vraie fenêtre fille, elle passe donc au-dessus de ce que Direct2D
//!   dessine (palette, menus, cartes d'information). Tant qu'une superposition est ouverte, l'aperçu est masqué (`masque`) ;
//! - **Gel** : la fenêtre du gestionnaire appartient à un fil de `prevhost.exe` mais est enfant de la fenêtre principale ;
//!   Windows relie alors les files d'entrée des deux fils, donc un `prevhost` figé gèlerait clavier et souris de SpaceFiles.
//!   Une garde (fil à part) envoie un `WM_NULL` par seconde aux fenêtres étrangères posées sur la nôtre ; après 3 échecs de
//!   suite elle termine ce `prevhost` (jamais un autre processus : le nom de l'exécutable est vérifié) et l'aperçu retombe sur
//!   l'aperçu habituel. Le gel dure au plus 3 à 4 s. La pompe dort jusqu'à un message ou un ordre (aucun réveil à vide).

use std::ffi::c_void;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU8, AtomicUsize, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender, TryRecvError};
use std::sync::Arc;
use std::time::{Duration, Instant};

use windows::core::{IUnknown, IUnknown_Vtbl, Interface, GUID, HRESULT, HSTRING, PWSTR};
use windows::Win32::Foundation::{CloseHandle, BOOL, E_NOINTERFACE, E_POINTER, HANDLE, HWND, LPARAM, RECT, S_FALSE, S_OK, WPARAM};
use windows::Win32::Graphics::Gdi::InvalidateRect;
use windows::Win32::System::Com::{CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_LOCAL_SERVER, COINIT_APARTMENTTHREADED, STGM_READ, STGM_SHARE_DENY_NONE};
use windows::Win32::System::Registry::{RegGetValueW, HKEY_CLASSES_ROOT, RRF_RT_REG_SZ};
use windows::Win32::UI::Shell::PropertiesSystem::{IInitializeWithFile, IInitializeWithStream};
use windows::Win32::UI::Shell::{IPreviewHandler, SHCreateStreamOnFileEx};
use windows::Win32::System::Ole::IObjectWithSite;
use windows::Win32::System::Threading::{CreateEventW, GetCurrentProcessId, OpenProcess, QueryFullProcessImageNameW, SetEvent, TerminateProcess, INFINITE, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_TERMINATE};
use windows::Win32::UI::Input::KeyboardAndMouse::GetKeyState;
use windows::Win32::UI::Shell::{IPreviewHandlerFrame, IPreviewHandlerFrame_Vtbl, PREVIEWHANDLERFRAMEINFO};
use windows::Win32::UI::WindowsAndMessaging::{
    DispatchMessageW, EnumChildWindows, GetWindowThreadProcessId, MsgWaitForMultipleObjects, PeekMessageW, PostMessageW, SendMessageTimeoutW, ShowWindow, TranslateMessage, HACCEL, MSG, PM_REMOVE, QS_ALLINPUT,
    SMTO_ABORTIFHUNG, SMTO_BLOCK, SW_HIDE, SW_SHOWNA, WM_APP, WM_KEYDOWN, WM_NULL, WM_SYSKEYDOWN,
};

/// Extensions confiées au gestionnaire (sans point, insensible à la casse)
const GERE: &[&str] = &["doc", "docx", "xls", "xlsx", "ppt", "pptx", "odt", "ods", "odp", "rtf", "vsd", "vsdx"];

/// Délai laissé au gestionnaire pour afficher
pub const DELAI: Duration = Duration::from_secs(3);
/// Fils abandonnés encore bloqués au-delà desquels la fonction se coupe
const MAX_BLOQUES: usize = 3;
static BLOQUES: AtomicUsize = AtomicUsize::new(0);

/// Interface `IPreviewHandler` des gestionnaires de la coquille : clé `ShellEx\{…}` d'une extension
const CLE_GESTIONNAIRE: &str = "{8895b1c6-b41f-4c1c-a562-0d564250836f}";

const LANCEMENT: u8 = 0;
const PRET: u8 = 1;
const ECHEC: u8 = 2;

/// L'extension est de celles que le gestionnaire système prend en charge
pub fn gere(ext: &str) -> bool {
    GERE.iter().any(|e| ext.eq_ignore_ascii_case(e))
}

/// `{xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx}` (accolades facultatives) en GUID ; `None` si le texte est abîmé
fn guid_de_texte(t: &str) -> Option<GUID> {
    let t = t.trim().trim_start_matches('{').trim_end_matches('}');
    let p: Vec<&str> = t.split('-').collect();
    if p.len() != 5 || p.iter().zip([8, 4, 4, 4, 12]).any(|(s, n)| s.len() != n || !s.bytes().all(|b| b.is_ascii_hexdigit())) {
        return None;
    }
    let d1 = u32::from_str_radix(p[0], 16).ok()?;
    let d2 = u16::from_str_radix(p[1], 16).ok()?;
    let d3 = u16::from_str_radix(p[2], 16).ok()?;
    let reste = format!("{}{}", p[3], p[4]);
    let mut d4 = [0u8; 8];
    for (i, o) in d4.iter_mut().enumerate() {
        *o = u8::from_str_radix(&reste[2 * i..2 * i + 2], 16).ok()?;
    }
    Some(GUID::from_values(d1, d2, d3, d4))
}

/// Valeur par défaut d'une clé de `HKEY_CLASSES_ROOT`
fn valeur_defaut(sous_cle: &str) -> Option<String> {
    let mut buf = [0u16; 160];
    let mut taille = (buf.len() * 2) as u32;
    // SAFETY : `buf` et `taille` vivent pendant l'appel ; `taille` est la capacité réelle en octets
    let r = unsafe { RegGetValueW(HKEY_CLASSES_ROOT, &HSTRING::from(sous_cle), None, RRF_RT_REG_SZ, None, Some(buf.as_mut_ptr() as *mut _), Some(&mut taille)) };
    if r.0 != 0 {
        return None;
    }
    let n = (taille as usize / 2).saturating_sub(1).min(buf.len());
    let t = String::from_utf16_lossy(&buf[..n]);
    (!t.is_empty()).then_some(t)
}

/// Gestionnaire d'aperçu inscrit pour cette extension : sur l'extension elle-même, sinon sur son identifiant de programme
fn clsid_de(ext: &str) -> Option<GUID> {
    // Même interrupteur que les entrées de SpaceFiles : un essai sur faux profil ne dépend pas du poste
    if crate::defaut::sans_registre() {
        return None;
    }
    let ext = ext.trim_start_matches('.').to_ascii_lowercase();
    let direct = valeur_defaut(&format!(r".{ext}\ShellEx\{CLE_GESTIONNAIRE}"));
    let texte = direct.or_else(|| {
        let prog = valeur_defaut(&format!(".{ext}"))?;
        valeur_defaut(&format!(r"{prog}\ShellEx\{CLE_GESTIONNAIRE}"))
    })?;
    guid_de_texte(&texte)
}

/// Le coup d'œil peut confier ce fichier au gestionnaire système (extension prise en charge, gestionnaire inscrit, pas trop
/// de fils bloqués)
pub fn disponible_pour(ext: &str) -> bool {
    gere(ext) && BLOQUES.load(Ordering::Relaxed) < MAX_BLOQUES && clsid_de(ext).is_some()
}

enum Ordre {
    Rect(RECT),
    /// Cacher (vrai) ou remontrer (faux) la fenêtre du gestionnaire : une superposition est ouverte
    Masque(bool),
    /// Donner le clavier au gestionnaire (Tab dans le coup d'œil)
    Focus,
    Fermer,
}

/// Message posté à la fenêtre principale pour qu'elle reprenne le clavier (le fil de l'hôte ne peut pas le faire à sa place)
pub const WM_HOTE_FOCUS: u32 = WM_APP + 51;

/// Ce que partagent le fil d'interface, le fil de l'hôte et la veille
struct Partage {
    etat: AtomicU8,
    /// L'aperçu n'est plus voulu : le fil décharge le gestionnaire dès qu'il le tient
    abandon: AtomicBool,
    /// Une superposition recouvre l'aperçu : la fenêtre du gestionnaire reste cachée
    masque: AtomicBool,
    /// Ce fil est (ou a été) compté dans `BLOQUES` : le premier qui bascule l'indicateur décide, l'autre ne compte rien
    compte: AtomicBool,
    /// Événement qui réveille la pompe du fil quand un ordre arrive (plus de réveil toutes les 15 ms pour rien). Il vit autant
    /// que le `Partage` : un ordre envoyé après la fin du fil ne tombe jamais sur un handle réutilisé.
    reveil: isize,
}

impl Partage {
    fn nouveau(etat: u8, abandon: bool, compte: bool) -> Partage {
        // SAFETY : événement anonyme à réarmement automatique, fermé dans `Drop`
        let reveil = unsafe { CreateEventW(None, false, false, None) }.map(|h| h.0 as isize).unwrap_or(0);
        Partage { etat: AtomicU8::new(etat), abandon: AtomicBool::new(abandon), masque: AtomicBool::new(false), compte: AtomicBool::new(compte), reveil }
    }

    /// Envoie un ordre au fil et le réveille
    fn envoyer(&self, ordres: &Sender<Ordre>, o: Ordre) {
        let _ = ordres.send(o);
        if self.reveil != 0 {
            // SAFETY : le handle est valide tant que `self` vit
            unsafe {
                let _ = SetEvent(HANDLE(self.reveil as *mut _));
            }
        }
    }
}

impl Drop for Partage {
    fn drop(&mut self) {
        if self.reveil != 0 {
            // SAFETY : handle créé dans `nouveau`, jamais fermé ailleurs
            unsafe {
                let _ = CloseHandle(HANDLE(self.reveil as *mut _));
            }
        }
    }
}

struct Hote {
    ordres: Sender<Ordre>,
    partage: Arc<Partage>,
    debut: Instant,
    chemin: String,
    pose: RECT,
    masque: bool,
}

thread_local! {
    static HOTE: std::cell::RefCell<Option<Hote>> = const { std::cell::RefCell::new(None) };
}

fn egal(a: &RECT, b: &RECT) -> bool {
    a.left == b.left && a.top == b.top && a.right == b.right && a.bottom == b.bottom
}

/// Le gestionnaire a manqué son délai ou a échoué : le coup d'œil doit se rabattre sur l'aperçu habituel
pub fn a_echoue() -> bool {
    HOTE.with(|h| h.borrow().as_ref().is_some_and(|h| h.partage.etat.load(Ordering::Acquire) == ECHEC))
}

/// Fait repeindre la fenêtre depuis n'importe quel fil : le repli (ou l'affichage) ne dépend pas d'une autre peinture
fn repeindre(parent: isize) {
    // SAFETY : `InvalidateRect` accepte n'importe quel fil ; un handle périmé échoue sans effet
    unsafe {
        let _ = InvalidateRect(HWND(parent as *mut _), None, false);
    }
}

/// Délai dépassé : passe de `LANCEMENT` à `ECHEC` (une seule fois, quel que soit l'appelant), compte le fil comme bloqué et
/// fait repeindre la fenêtre pour que le repli se déclenche sans attendre autre chose
fn expirer(p: &Partage, ordres: &Sender<Ordre>, parent: isize, chemin: &str) {
    if p.etat.compare_exchange(LANCEMENT, ECHEC, Ordering::AcqRel, Ordering::Acquire).is_ok() {
        p.abandon.store(true, Ordering::Release);
        p.envoyer(ordres, Ordre::Fermer);
        if !p.compte.swap(true, Ordering::AcqRel) {
            BLOQUES.fetch_add(1, Ordering::Relaxed);
        }
        crate::journal::ecrire(crate::journal::Niveau::Avert, "hote_apercu", &format!("délai dépassé ({} s) : {chemin}", DELAI.as_secs()));
        repeindre(parent);
    }
}

/// Montre `cible` (chemin, extension, rectangle en pixels dans la fenêtre parente) ou ferme l'aperçu si `cible` est vide.
/// `masque` : une superposition (palette, menu, carte d'information) recouvre la zone, l'aperçu se cache le temps qu'elle dure.
/// Appelé à chaque peinture : ne fait rien quand rien n'a changé.
pub fn synchroniser(parent: HWND, cible: Option<(&str, &str, RECT)>, masque: bool) {
    let Some((chemin, ext, rect)) = cible else {
        fermer();
        return;
    };
    let hwnd = parent.0 as isize;
    let faut_lancer = HOTE.with(|h| {
        let mut h = h.borrow_mut();
        match h.as_mut() {
            Some(x) if x.chemin == chemin => {
                let etat = x.partage.etat.load(Ordering::Acquire);
                if etat == LANCEMENT && x.debut.elapsed() > DELAI {
                    expirer(&x.partage, &x.ordres, hwnd, chemin);
                } else if etat != ECHEC {
                    if !egal(&x.pose, &rect) {
                        x.pose = rect;
                        x.partage.envoyer(&x.ordres, Ordre::Rect(rect));
                    }
                    if x.masque != masque {
                        x.masque = masque;
                        x.partage.masque.store(masque, Ordering::Release);
                        x.partage.envoyer(&x.ordres, Ordre::Masque(masque));
                    }
                }
                false
            }
            _ => true,
        }
    });
    if !faut_lancer {
        return;
    }
    fermer();
    let Some(clsid) = clsid_de(ext) else {
        // Pas de gestionnaire : échec immédiat, pour que l'aperçu habituel prenne le relais
        installer_echec(chemin, rect);
        return;
    };
    let (ordres, recu) = channel();
    let partage = Arc::new(Partage::nouveau(LANCEMENT, false, false));
    partage.masque.store(masque, Ordering::Release);
    let (p2, c2) = (partage.clone(), chemin.to_string());
    let lance = std::thread::Builder::new().name("hote-apercu".into()).spawn(move || {
        travailler(hwnd, &c2, clsid, rect, &p2, &recu);
        // Fin du fil : s'il avait été compté comme bloqué, il ne l'est plus
        if p2.compte.swap(true, Ordering::AcqRel) {
            BLOQUES.fetch_sub(1, Ordering::Relaxed);
        }
    });
    if lance.is_err() {
        installer_echec(chemin, rect);
        return;
    }
    // Veille : si le gestionnaire ne répond pas (et que la fenêtre ne se repeint plus), elle déclenche l'abandon et la
    // peinture qui fait le repli
    let (pv, ov, cv) = (partage.clone(), ordres.clone(), chemin.to_string());
    let _ = std::thread::Builder::new().name("hote-apercu-veille".into()).spawn(move || {
        std::thread::sleep(DELAI + Duration::from_millis(200));
        if pv.etat.load(Ordering::Acquire) == LANCEMENT && !pv.abandon.load(Ordering::Acquire) {
            expirer(&pv, &ov, hwnd, &cv);
        }
    });
    // Garde : un `prevhost` qui ne répond plus ne doit pas geler la fenêtre principale (files d'entrée liées)
    let (pg, og, cg) = (partage.clone(), ordres.clone(), chemin.to_string());
    let _ = std::thread::Builder::new().name("hote-apercu-garde".into()).spawn(move || garder(&pg, &og, hwnd, &cg));
    HOTE.with(|h| *h.borrow_mut() = Some(Hote { ordres, partage, debut: Instant::now(), chemin: chemin.to_string(), pose: rect, masque }));
}

fn installer_echec(chemin: &str, rect: RECT) {
    let (ordres, _) = channel();
    let partage = Arc::new(Partage::nouveau(ECHEC, true, true));
    HOTE.with(|h| *h.borrow_mut() = Some(Hote { ordres, partage, debut: Instant::now(), chemin: chemin.to_string(), pose: rect, masque: false }));
}

/// Tab dans le coup d'œil : le clavier passe au gestionnaire (défilement, sélection) ; Tab ou Échap le rendent
pub fn prendre_le_focus() {
    HOTE.with(|h| {
        if let Some(h) = h.borrow().as_ref().filter(|h| h.partage.etat.load(Ordering::Acquire) == PRET) {
            h.partage.envoyer(&h.ordres, Ordre::Focus);
        }
    });
}

/// Décharge le gestionnaire (au fil de s'en occuper) et oublie l'aperçu en cours
pub fn fermer() {
    if let Some(h) = HOTE.with(|h| h.borrow_mut().take()) {
        h.partage.abandon.store(true, Ordering::Release);
        h.partage.envoyer(&h.ordres, Ordre::Fermer);
    }
}

/// Échec côté fil : passe en `ECHEC` (sauf si l'abandon l'a déjà fait) et fait repeindre pour que le repli se déclenche
fn finir_en_echec(p: &Partage, parent: isize) {
    let _ = p.etat.compare_exchange(LANCEMENT, ECHEC, Ordering::AcqRel, Ordering::Acquire);
    repeindre(parent);
}

/// Corps du fil : crée, initialise et affiche le gestionnaire, puis pompe les messages jusqu'à l'ordre de fermeture
fn travailler(parent: isize, chemin: &str, clsid: GUID, rect: RECT, p: &Partage, recu: &Receiver<Ordre>) {
    // SAFETY : tout COM vit sur ce fil (appartement mono-fil) du `CoInitializeEx` au `CoUninitialize` ; l'HWND parent n'est
    // qu'un identifiant : la fenêtre peut avoir été détruite entre-temps, c'est pourquoi `demarrer` teste `abandon` avant de
    // poser le gestionnaire dessus (un handle périmé ne fait qu'échouer)
    unsafe {
        if CoInitializeEx(None, COINIT_APARTMENTTHREADED).is_err() {
            finir_en_echec(p, parent);
            return;
        }
        let Some(g) = demarrer(parent, chemin, clsid, rect, &p.abandon) else {
            finir_en_echec(p, parent);
            CoUninitialize();
            return;
        };
        // Passer à PRET seulement si personne n'a abandonné entre-temps ; sinon décharger tout de suite (`Unload` détruit la
        // fenêtre posée dans la fenêtre principale)
        if p.abandon.load(Ordering::Acquire) || p.etat.compare_exchange(LANCEMENT, PRET, Ordering::AcqRel, Ordering::Acquire).is_err() {
            let _ = g.Unload();
            drop(g);
            CoUninitialize();
            return;
        }
        if p.masque.load(Ordering::Acquire) {
            masquer(parent, true);
        }
        repeindre(parent);
        // Pompe événementielle : le fil dort jusqu'à un message Windows (le gestionnaire en reçoit) ou un ordre (événement)
        let reveil = [HANDLE(p.reveil as *mut _)];
        let attente = if p.reveil != 0 { &reveil[..] } else { &[][..] };
        'pompe: loop {
            let mut m = MSG::default();
            while PeekMessageW(&mut m, None, 0, 0, PM_REMOVE).as_bool() {
                let _ = TranslateMessage(&m);
                DispatchMessageW(&m);
            }
            loop {
                match recu.try_recv() {
                    Ok(Ordre::Rect(r)) => {
                        let _ = g.SetRect(&r);
                    }
                    Ok(Ordre::Masque(m)) => masquer(parent, m),
                    Ok(Ordre::Focus) => {
                        let _ = g.SetFocus();
                    }
                    Ok(Ordre::Fermer) | Err(TryRecvError::Disconnected) => break 'pompe,
                    Err(TryRecvError::Empty) => break,
                }
            }
            // Sans événement (création refusée) : repli sur un réveil toutes les 15 ms
            let delai = if attente.is_empty() { 15 } else { INFINITE };
            MsgWaitForMultipleObjects(Some(attente), false, delai, QS_ALLINPUT);
        }
        if let Ok(site) = g.cast::<IObjectWithSite>() {
            let _ = site.SetSite(None::<&IUnknown>);
        }
        let _ = g.Unload();
        drop(g);
        CoUninitialize();
    }
}

/// Crée le gestionnaire, l'initialise (par fichier, sinon par flux) et lance l'aperçu. Toute erreur : `None`, journalisée.
unsafe fn demarrer(parent: isize, chemin: &str, clsid: GUID, rect: RECT, abandon: &AtomicBool) -> Option<IPreviewHandler> {
    let journaliser = |m: &str| crate::journal::ecrire(crate::journal::Niveau::Avert, "hote_apercu", &format!("{m} ({chemin})"));
    let g: IPreviewHandler = match CoCreateInstance(&clsid, None::<&windows::core::IUnknown>, CLSCTX_LOCAL_SERVER) {
        Ok(g) => g,
        Err(e) => {
            journaliser(&format!("activation refusée : {e}"));
            return None;
        }
    };
    let h = HSTRING::from(chemin);
    // Lecture partagée : Word ne s'ouvre pas en lecture seule pendant le coup d'œil, un renommage ne bute pas sur notre flux
    let mode = STGM_READ.0 | STGM_SHARE_DENY_NONE.0;
    let initialise = if let Ok(f) = g.cast::<IInitializeWithFile>() {
        f.Initialize(&h, mode)
    } else if let Ok(s) = g.cast::<IInitializeWithStream>() {
        match SHCreateStreamOnFileEx(&h, mode, 0, false, None) {
            Ok(flux) => s.Initialize(&flux, mode),
            Err(e) => Err(e),
        }
    } else {
        journaliser("ni fichier ni flux");
        return None;
    };
    if let Err(e) = initialise {
        journaliser(&format!("initialisation : {e}"));
        return None;
    }
    // La fenêtre principale a pu disparaître pendant l'activation : ne rien poser dessus
    if abandon.load(Ordering::Acquire) {
        let _ = g.Unload();
        return None;
    }
    // Cadre : le gestionnaire lui rend les touches qu'il ne garde pas (sans cadre, le clavier resterait chez lui)
    if let Ok(site) = g.cast::<IObjectWithSite>() {
        let cadre = Box::into_raw(Box::new(Cadre { vtbl: &VTBL_CADRE, refs: AtomicU32::new(1), parent }));
        // SAFETY : `cadre` vient d'être alloué avec un compte de 1 que `from_raw` reprend ; le gestionnaire prend le sien par `SetSite`
        let cadre = IPreviewHandlerFrame::from_raw(cadre as *mut c_void);
        let _ = site.SetSite(&cadre);
    }
    if let Err(e) = g.SetWindow(HWND(parent as *mut _), &rect).and_then(|_| g.DoPreview()) {
        journaliser(&format!("aperçu : {e}"));
        let _ = g.Unload();
        return None;
    }
    // Certains gestionnaires (Word) prennent la taille du parent à `DoPreview` : on impose le rectangle une fois l'aperçu posé
    let _ = g.SetRect(&rect);
    crate::journal::ecrire(crate::journal::Niveau::Info, "hote_apercu", &format!("posé en [{},{},{},{}] ({chemin})", rect.left, rect.top, rect.right, rect.bottom));
    Some(g)
}

// ───────────────────────────── Fenêtres du gestionnaire (autre processus) posées sur la nôtre ─────────────────────────────

unsafe extern "system" fn recueillir(h: HWND, l: LPARAM) -> BOOL {
    // SAFETY : `l` est l'adresse du `Vec` de `etrangeres`, vivant pendant l'énumération
    let v = &mut *(l.0 as *mut Vec<HWND>);
    let mut pid = 0u32;
    GetWindowThreadProcessId(h, Some(&mut pid));
    if pid != 0 && pid != GetCurrentProcessId() {
        v.push(h);
    }
    BOOL(1)
}

/// Fenêtres descendantes de `parent` qui appartiennent à un autre processus : celles du gestionnaire (`prevhost`)
fn etrangeres(parent: isize) -> Vec<HWND> {
    let mut v: Vec<HWND> = Vec::new();
    // SAFETY : le rappel ne touche que `v` ; un handle périmé ne fait que n'énumérer rien
    unsafe {
        let _ = EnumChildWindows(HWND(parent as *mut _), Some(recueillir), LPARAM(&mut v as *mut Vec<HWND> as isize));
    }
    v
}

/// Cache ou remontre les fenêtres du gestionnaire, puis fait repeindre ce qu'elles couvraient
fn masquer(parent: isize, cacher: bool) {
    for h in etrangeres(parent) {
        // SAFETY : `ShowWindow` accepte la fenêtre d'un autre fil ; un hôte figé bloque ce fil, que la garde sait résoudre
        unsafe {
            let _ = ShowWindow(h, if cacher { SW_HIDE } else { SW_SHOWNA });
        }
    }
    repeindre(parent);
}

/// Échecs consécutifs du `WM_NULL` au bout desquels le `prevhost` est jugé figé
pub const ECHECS_AVANT_ARRET: u32 = 3;

/// Compte les échecs de réponse de suite : vrai quand l'hôte est jugé figé (un seul succès remet le compte à zéro)
fn fige(echecs: &mut u32, a_repondu: bool) -> bool {
    if a_repondu {
        *echecs = 0;
        false
    } else {
        *echecs += 1;
        *echecs >= ECHECS_AVANT_ARRET
    }
}

/// Termine le processus `pid` seulement si c'est `prevhost.exe` : on ne tue jamais autre chose que l'hôte qu'on a lancé
fn terminer_prevhost(pid: u32) -> bool {
    // SAFETY : handle ouvert ici et refermé ici ; le tampon est de 520 caractères et `taille` en annonce la capacité
    unsafe {
        let Ok(h) = OpenProcess(PROCESS_TERMINATE | PROCESS_QUERY_LIMITED_INFORMATION, false, pid) else { return false };
        let mut buf = [0u16; 520];
        let mut taille = buf.len() as u32;
        let ok = QueryFullProcessImageNameW(h, PROCESS_NAME_WIN32, PWSTR(buf.as_mut_ptr()), &mut taille).is_ok();
        let nom = String::from_utf16_lossy(&buf[..(taille as usize).min(buf.len())]);
        let est_prevhost = ok && nom.rsplit(['\\', '/']).next().is_some_and(|n| n.eq_ignore_ascii_case("prevhost.exe"));
        let tue = est_prevhost && TerminateProcess(h, 1).is_ok();
        let _ = CloseHandle(h);
        tue
    }
}

/// Garde (fil à part) : pendant que l'aperçu vit, envoie un `WM_NULL` à chaque fenêtre étrangère posée sur la nôtre ; si l'une ne
/// répond plus `ECHECS_AVANT_ARRET` fois de suite, termine son `prevhost` et fait passer l'aperçu en échec (repli habituel).
fn garder(p: &Partage, ordres: &Sender<Ordre>, parent: isize, chemin: &str) {
    let mut echecs = 0u32;
    loop {
        std::thread::sleep(Duration::from_secs(1));
        if p.abandon.load(Ordering::Acquire) {
            return;
        }
        let fenetres = etrangeres(parent);
        if fenetres.is_empty() {
            continue;
        }
        let mut repond = true;
        let mut pid_fige = 0u32;
        for h in &fenetres {
            let mut r = 0usize;
            // SAFETY : appel depuis ce fil (jamais celui de l'interface) ; SMTO_ABORTIFHUNG rend la main si le fil cible est figé
            let ok = unsafe { SendMessageTimeoutW(*h, WM_NULL, WPARAM(0), LPARAM(0), SMTO_ABORTIFHUNG | SMTO_BLOCK, 1000, Some(&mut r)) };
            if ok.0 == 0 {
                repond = false;
                // SAFETY : lecture de l'identifiant de processus d'une fenêtre
                unsafe {
                    GetWindowThreadProcessId(*h, Some(&mut pid_fige));
                }
            }
        }
        if fige(&mut echecs, repond) {
            let tue = pid_fige != 0 && terminer_prevhost(pid_fige);
            crate::journal::ecrire(crate::journal::Niveau::Avert, "hote_apercu", &format!("gestionnaire figé : prevhost {} ({chemin})", if tue { "terminé" } else { "non terminé" }));
            if p.etat.swap(ECHEC, Ordering::AcqRel) != ECHEC {
                p.abandon.store(true, Ordering::Release);
                p.envoyer(ordres, Ordre::Fermer);
                if !p.compte.swap(true, Ordering::AcqRel) {
                    BLOQUES.fetch_add(1, Ordering::Relaxed);
                }
            }
            repeindre(parent);
            return;
        }
    }
}

// ───────────────────────────── Cadre `IPreviewHandlerFrame` (objet COM à la main : windows 0.58 n'a pas son trait) ─────────────────────────────

/// Que faire d'une touche que le gestionnaire ne garde pas
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
enum Relais {
    /// Le gestionnaire la traite (défilement, sélection…)
    Garder,
    /// Reposée à la fenêtre principale
    Poster,
    /// Le clavier revient à la fenêtre principale (la touche n'est pas reposée)
    Rendre,
    /// Reposée, puis le clavier revient (Échap : le coup d'œil se ferme)
    PosterEtRendre,
}

/// Décision pour une touche : Échap et les touches de fonction vont à SpaceFiles, Tab rend le clavier, Alt+… et Ctrl+… vont à
/// SpaceFiles sauf Ctrl+A et Ctrl+C (sélection et copie dans le gestionnaire) ; le reste appartient au gestionnaire
fn decider(vk: u32, ctrl: bool, alt: bool) -> Relais {
    match vk {
        0x1B => Relais::PosterEtRendre,
        0x09 if !ctrl && !alt => Relais::Rendre,
        0x70..=0x7B => Relais::Poster,
        _ if alt => Relais::Poster,
        0x41 | 0x43 if ctrl => Relais::Garder,
        _ if ctrl => Relais::Poster,
        _ => Relais::Garder,
    }
}

#[repr(C)]
struct Cadre {
    vtbl: *const IPreviewHandlerFrame_Vtbl,
    refs: AtomicU32,
    parent: isize,
}

unsafe extern "system" fn cadre_interroger(this: *mut c_void, iid: *const GUID, sortie: *mut *mut c_void) -> HRESULT {
    if sortie.is_null() || iid.is_null() {
        return E_POINTER;
    }
    if *iid == IUnknown::IID || *iid == IPreviewHandlerFrame::IID {
        *sortie = this;
        cadre_ajouter(this);
        S_OK
    } else {
        *sortie = std::ptr::null_mut();
        E_NOINTERFACE
    }
}

unsafe extern "system" fn cadre_ajouter(this: *mut c_void) -> u32 {
    (*(this as *const Cadre)).refs.fetch_add(1, Ordering::Relaxed) + 1
}

unsafe extern "system" fn cadre_liberer(this: *mut c_void) -> u32 {
    let n = (*(this as *const Cadre)).refs.fetch_sub(1, Ordering::AcqRel) - 1;
    if n == 0 {
        // SAFETY : l'objet a été alloué par `Box::new` dans `demarrer` et plus personne n'en tient de référence
        drop(Box::from_raw(this as *mut Cadre));
    }
    n
}

unsafe extern "system" fn cadre_contexte(_this: *mut c_void, info: *mut PREVIEWHANDLERFRAMEINFO) -> HRESULT {
    if info.is_null() {
        return E_POINTER;
    }
    // Pas de table d'accélérateurs : les touches utiles passent par `TranslateAccelerator`
    *info = PREVIEWHANDLERFRAMEINFO { haccel: HACCEL(std::ptr::null_mut()), cAccelEntries: 0 };
    S_OK
}

unsafe extern "system" fn cadre_touche(this: *mut c_void, msg: *const MSG) -> HRESULT {
    if msg.is_null() {
        return S_FALSE;
    }
    let (m, parent) = (&*msg, (*(this as *const Cadre)).parent);
    if m.message != WM_KEYDOWN && m.message != WM_SYSKEYDOWN {
        return S_FALSE;
    }
    let ctrl = GetKeyState(0x11) < 0;
    let alt = m.message == WM_SYSKEYDOWN || GetKeyState(0x12) < 0;
    let relais = decider(m.wParam.0 as u32, ctrl, alt);
    if relais == Relais::Garder {
        return S_FALSE;
    }
    let h = HWND(parent as *mut _);
    if matches!(relais, Relais::Poster | Relais::PosterEtRendre) {
        let _ = PostMessageW(h, m.message, m.wParam, m.lParam);
    }
    if matches!(relais, Relais::Rendre | Relais::PosterEtRendre) {
        // Ce fil n'est pas celui de la fenêtre : c'est elle qui reprend le clavier
        let _ = PostMessageW(h, WM_HOTE_FOCUS, WPARAM(0), LPARAM(0));
    }
    S_OK
}

static VTBL_CADRE: IPreviewHandlerFrame_Vtbl = IPreviewHandlerFrame_Vtbl {
    base__: IUnknown_Vtbl { QueryInterface: cadre_interroger, AddRef: cadre_ajouter, Release: cadre_liberer },
    GetWindowContext: cadre_contexte,
    TranslateAccelerator: cadre_touche,
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seules_les_extensions_listees_sont_confiees() {
        assert!(gere("DOCX") && gere("xlsx") && gere("Rtf"));
        // Les visionneuses intégrées gardent leurs formats
        assert!(!gere("pdf") && !gere("png") && !gere("html") && !gere("txt") && !gere(""));
    }

    #[test]
    fn guid_lu_du_registre() {
        let g = guid_de_texte(CLE_GESTIONNAIRE).expect("guid valide");
        assert_eq!((g.data1, g.data2, g.data3), (0x8895b1c6, 0xb41f, 0x4c1c));
        assert_eq!(g.data4, [0xa5, 0x62, 0x0d, 0x56, 0x42, 0x50, 0x83, 0x6f]);
        // Sans accolades aussi ; abîmé : refusé, jamais de panique
        assert!(guid_de_texte("8895B1C6-B41F-4C1C-A562-0D564250836F").is_some());
        for mauvais in ["", "{}", "{8895b1c6-b41f-4c1c-a562}", "{zzzzzzzz-b41f-4c1c-a562-0d564250836f}", "{8895b1c6-b41f-4c1c-a562-0d564250836}", "é{8895b1c6-b41f-4c1c-a562-0d564250836f}"] {
            assert!(guid_de_texte(mauvais).is_none(), "{mauvais}");
        }
    }

    #[test]
    fn delai_et_coupure() {
        assert_eq!(DELAI, Duration::from_secs(3));
        // Une extension non prise en charge n'interroge même pas le registre
        assert!(!disponible_pour("zzz"));
    }

    /// Le délai ne joue qu'une fois, même demandé par deux côtés (fil d'interface et veille) : un seul compte de fil bloqué
    #[test]
    fn expiration_une_seule_fois() {
        let p = Partage::nouveau(LANCEMENT, false, false);
        let (tx, rx) = channel();
        let avant = BLOQUES.load(Ordering::Relaxed);
        expirer(&p, &tx, 0, "x");
        expirer(&p, &tx, 0, "x");
        assert_eq!(p.etat.load(Ordering::Relaxed), ECHEC);
        assert!(p.abandon.load(Ordering::Relaxed));
        assert_eq!(BLOQUES.load(Ordering::Relaxed), avant + 1);
        assert!(matches!(rx.try_recv(), Ok(Ordre::Fermer)) && rx.try_recv().is_err());
        // Un fil déjà prêt n'est pas abandonné
        let q = Partage::nouveau(PRET, false, false);
        expirer(&q, &tx, 0, "x");
        assert_eq!(q.etat.load(Ordering::Relaxed), PRET);
        BLOQUES.fetch_sub(1, Ordering::Relaxed);
    }

    /// Un ordre réveille la pompe : l'événement est signalé, puis rendu au repos (réarmement automatique) par l'attente
    #[test]
    fn un_ordre_reveille_la_pompe() {
        let p = Partage::nouveau(PRET, false, false);
        assert_ne!(p.reveil, 0);
        let (tx, rx) = channel();
        let h = [HANDLE(p.reveil as *mut _)];
        // SAFETY : handle valide tant que `p` vit
        unsafe {
            // Rien d'envoyé : l'attente expire (WAIT_TIMEOUT = 258)
            assert_eq!(MsgWaitForMultipleObjects(Some(&h), false, 0, QS_ALLINPUT).0, 258);
            p.envoyer(&tx, Ordre::Fermer);
            assert_eq!(MsgWaitForMultipleObjects(Some(&h), false, 1000, QS_ALLINPUT).0, 0);
            assert_eq!(MsgWaitForMultipleObjects(Some(&h), false, 0, QS_ALLINPUT).0, 258);
        }
        assert!(matches!(rx.try_recv(), Ok(Ordre::Fermer)));
    }

    #[test]
    fn touches_du_gestionnaire_ou_de_l_application() {
        assert_eq!(decider(0x1B, false, false), Relais::PosterEtRendre);
        assert_eq!(decider(0x09, false, false), Relais::Rendre);
        assert_eq!(decider(0x09, true, false), Relais::Poster);
        assert_eq!(decider(0x71, false, false), Relais::Poster);
        assert_eq!(decider(0x4B, true, false), Relais::Poster, "Ctrl+K : la palette");
        assert_eq!(decider(0x41, true, false), Relais::Garder, "Ctrl+A reste au gestionnaire");
        assert_eq!(decider(0x43, true, false), Relais::Garder);
        assert_eq!(decider(0x25, false, true), Relais::Poster, "Alt+Gauche");
        assert_eq!(decider(0x28, false, false), Relais::Garder, "les flèches défilent le document");
        assert_eq!(decider(0x20, false, false), Relais::Garder);
    }

    #[test]
    fn un_hote_est_fige_apres_trois_silences_de_suite() {
        let mut n = 0;
        assert!(!fige(&mut n, false));
        assert!(!fige(&mut n, false));
        assert!(!fige(&mut n, true), "un succès remet le compte à zéro");
        assert!(!fige(&mut n, false));
        assert!(!fige(&mut n, false));
        assert!(fige(&mut n, false));
    }

    #[test]
    fn le_cadre_repond_a_son_interface_et_se_libere() {
        // SAFETY : objet alloué ici, compte 1, libéré par le dernier `Release`
        unsafe {
            let c = Box::into_raw(Box::new(Cadre { vtbl: &VTBL_CADRE, refs: AtomicU32::new(1), parent: 0 })) as *mut c_void;
            let mut sortie = std::ptr::null_mut();
            assert_eq!(cadre_interroger(c, &IPreviewHandlerFrame::IID, &mut sortie), S_OK);
            assert_eq!(sortie, c);
            assert_eq!(cadre_interroger(c, &GUID::from_u128(7), &mut sortie), E_NOINTERFACE);
            assert!(sortie.is_null());
            assert_eq!(cadre_liberer(c), 1);
            let mut info = PREVIEWHANDLERFRAMEINFO { haccel: HACCEL(std::ptr::null_mut()), cAccelEntries: 9 };
            assert_eq!(cadre_contexte(c, &mut info), S_OK);
            assert_eq!(info.cAccelEntries, 0);
            assert_eq!(cadre_liberer(c), 0);
        }
    }

    #[test]
    fn prevhost_seul_est_terminable() {
        // Notre propre processus n'est pas `prevhost.exe` : il ne doit jamais être terminé
        assert!(!terminer_prevhost(std::process::id()));
    }
}
