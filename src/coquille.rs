// Intégration avec le Shell de Windows : menu clic droit complet, glisser vers les autres applications,
// déposer depuis elles

use std::cell::RefCell;

use windows::core::{implement, Interface, Result, HSTRING, PCSTR, PCWSTR, PSTR};
use windows::Win32::Foundation::*;
use windows::Win32::System::Com::*;
use windows::Win32::System::Ole::*;
use windows::Win32::System::SystemServices::{MK_CONTROL, MK_RBUTTON, MK_SHIFT, MODIFIERKEYS_FLAGS};
use windows::Win32::UI::Shell::Common::ITEMIDLIST;
use windows::Win32::UI::Input::KeyboardAndMouse::GetKeyState;
use windows::Win32::UI::Shell::*;
use windows::Win32::UI::WindowsAndMessaging::*;

// Options de CMINVOKECOMMANDINFOEX
const CMIC_MASK_UNICODE: u32 = 0x0000_4000;
const CMIC_MASK_PTINVOKE: u32 = 0x2000_0000;
const CMIC_MASK_SHIFT_DOWN: u32 = 0x1000_0000;
const CMIC_MASK_CONTROL_DOWN: u32 = 0x4000_0000;

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// Identifiant Shell d'un élément (libéré automatiquement)
struct Pidl(*mut ITEMIDLIST);

impl Drop for Pidl {
    fn drop(&mut self) {
        unsafe { CoTaskMemFree(Some(self.0 as *const _)) }
    }
}

unsafe fn pidl(chemin: &str) -> Result<Pidl> {
    let w = wide(chemin);
    let mut p = std::ptr::null_mut();
    SHParseDisplayName(PCWSTR(w.as_ptr()), None, &mut p, 0, None)?;
    Ok(Pidl(p))
}

/// Objet Shell (menu, données à glisser…) pour des éléments d'un même dossier
unsafe fn objet_elements<T: Interface>(hwnd: HWND, chemins: &[String]) -> Result<T> {
    // Le dossier parent vient du premier élément : mélanger des dossiers (recherche, récents, groupes)
    // ferait agir le Shell sur des homonymes du premier dossier. On refuse plutôt que de viser à côté.
    let dossier = |c: &String| std::path::Path::new(c).parent().map(|p| p.to_string_lossy().to_lowercase());
    if chemins.iter().any(|c| dossier(c) != dossier(&chemins[0])) {
        return Err(windows::Win32::Foundation::E_INVALIDARG.into());
    }
    let pidls = chemins.iter().map(|c| pidl(c)).collect::<Result<Vec<_>>>()?;
    let parent: IShellFolder = SHBindToParent(pidls[0].0, None)?;
    let enfants: Vec<*const ITEMIDLIST> = pidls.iter().map(|p| ILFindLastID(p.0) as *const _).collect();
    parent.GetUIObjectOf(hwnd, &enfants, None)
}

/// Le dossier lui-même, vu par le Shell
unsafe fn dossier_shell(chemin: &str) -> Result<IShellFolder> {
    let p = pidl(chemin)?;
    SHGetDesktopFolder()?.BindToObject(p.0, None)
}

// ───────────────────────── Menu clic droit ─────────────────────────

thread_local! {
    /// Menu affiché : ses sous-menus (Ouvrir avec, Envoyer vers…) se remplissent à l'ouverture
    static MENU: RefCell<Option<IContextMenu2>> = const { RefCell::new(None) };
}

/// À appeler pour WM_INITMENUPOPUP, WM_DRAWITEM, WM_MEASUREITEM et WM_MENUCHAR
pub fn message_menu(msg: u32, wparam: WPARAM, lparam: LPARAM) -> Option<LRESULT> {
    MENU.with(|m| {
        let m = m.borrow();
        let menu = m.as_ref()?;
        unsafe {
            if let Ok(m3) = menu.cast::<IContextMenu3>() {
                let mut r = LRESULT(0);
                m3.HandleMenuMsg2(msg, wparam, lparam, Some(&mut r)).ok()?;
                return Some(r);
            }
            menu.HandleMenuMsg(msg, wparam, lparam).ok()?;
            Some(LRESULT(0))
        }
    })
}

pub enum Choix {
    Rien,
    /// « Renommer » : on renomme dans la ligne au lieu de laisser faire Windows
    Renommer,
    Fait,
}

const PREMIER: u32 = 1;
const DERNIER: u32 = 0x7FFF;

/// Affiche le menu Windows des éléments (ou du fond du dossier si `chemins` est vide) au point écran (x, y)
pub unsafe fn menu(hwnd: HWND, chemins: &[String], dossier: Option<&str>, x: i32, y: i32, etendu: bool) -> Result<Choix> {
    let cm: IContextMenu = if chemins.is_empty() {
        let Some(d) = dossier else { return Ok(Choix::Rien) };
        dossier_shell(d)?.CreateViewObject(hwnd)?
    } else {
        objet_elements(hwnd, chemins)?
    };
    let hmenu = CreatePopupMenu()?;
    let mut drapeaux = CMF_NORMAL | CMF_CANRENAME;
    if etendu {
        drapeaux |= CMF_EXTENDEDVERBS;
    }
    if let Err(e) = cm.QueryContextMenu(hmenu, 0, PREMIER, DERNIER, drapeaux) {
        let _ = DestroyMenu(hmenu);
        return Err(e);
    }
    MENU.with(|m| *m.borrow_mut() = cm.cast::<IContextMenu2>().ok());
    // Boucle modale : un message qui reprendrait l'état de la fenêtre attend (voir `GardeModale`)
    let garde = crate::fenetre::GardeModale::nouvelle(hwnd);
    let id = TrackPopupMenuEx(hmenu, (TPM_RETURNCMD | TPM_RIGHTBUTTON).0, x, y, hwnd, None).0 as u32;
    drop(garde);
    MENU.with(|m| *m.borrow_mut() = None);

    let mut choix = Choix::Rien;
    if id >= PREMIER {
        let decalage = id - PREMIER;
        if verbe(&cm, decalage).eq_ignore_ascii_case("rename") {
            choix = Choix::Renommer;
        } else {
            let mut masque = CMIC_MASK_UNICODE | CMIC_MASK_PTINVOKE;
            if GetKeyState(0x11) < 0 {
                masque |= CMIC_MASK_CONTROL_DOWN;
            }
            if GetKeyState(0x10) < 0 {
                masque |= CMIC_MASK_SHIFT_DOWN;
            }
            let dossier_w = dossier.map(wide);
            let info = CMINVOKECOMMANDINFOEX {
                cbSize: std::mem::size_of::<CMINVOKECOMMANDINFOEX>() as u32,
                fMask: masque,
                hwnd,
                // Une commande désignée par son numéro se passe à la place du texte du verbe
                lpVerb: PCSTR(decalage as usize as *const u8),
                lpVerbW: PCWSTR(decalage as usize as *const u16),
                lpDirectoryW: dossier_w.as_ref().map_or(PCWSTR::null(), |d| PCWSTR(d.as_ptr())),
                nShow: SW_SHOWNORMAL.0,
                ptInvoke: POINT { x, y },
                ..Default::default()
            };
            let r = cm.InvokeCommand(&info as *const _ as *const CMINVOKECOMMANDINFO);
            choix = Choix::Fait;
            if let Err(e) = r {
                // L'utilisateur qui annule une boîte de dialogue n'est pas une erreur
                if e.code() != ERROR_CANCELLED.to_hresult() {
                    let _ = DestroyMenu(hmenu);
                    return Err(e);
                }
            }
        }
    }
    let _ = DestroyMenu(hmenu);
    Ok(choix)
}

/// Lance une commande du menu Windows par son verbe (« properties », « Windows.ModernShare »…) sans afficher le menu
pub unsafe fn invoquer(hwnd: HWND, chemins: &[String], verbe_a: &str) -> Result<()> {
    let cm: IContextMenu = objet_elements(hwnd, chemins)?;
    // Certains gestionnaires ne connaissent leurs verbes qu'après QueryContextMenu
    let hmenu = CreatePopupMenu()?;
    let _ = cm.QueryContextMenu(hmenu, 0, PREMIER, DERNIER, CMF_NORMAL);
    let a: Vec<u8> = verbe_a.bytes().chain(std::iter::once(0)).collect();
    let w = wide(verbe_a);
    let info = CMINVOKECOMMANDINFOEX {
        cbSize: std::mem::size_of::<CMINVOKECOMMANDINFOEX>() as u32,
        fMask: CMIC_MASK_UNICODE,
        hwnd,
        lpVerb: PCSTR(a.as_ptr()),
        lpVerbW: PCWSTR(w.as_ptr()),
        nShow: SW_SHOWNORMAL.0,
        ..Default::default()
    };
    let r = cm.InvokeCommand(&info as *const _ as *const CMINVOKECOMMANDINFO);
    let _ = DestroyMenu(hmenu);
    match r {
        Err(e) if e.code() != ERROR_CANCELLED.to_hresult() => Err(e),
        _ => Ok(()),
    }
}

/// Application capable d'ouvrir un type de fichier (sous-menu OUVRIR AVEC)
pub struct Application {
    pub nom: String,
    gestionnaire: IAssocHandler,
}

/// Applications conseillées par Windows pour cette extension (« .txt »), sans doublons
pub unsafe fn applications(ext: &str) -> Vec<Application> {
    let mut v: Vec<Application> = Vec::new();
    let w = wide(&format!(".{}", ext.trim_start_matches('.')));
    let Ok(liste) = SHAssocEnumHandlers(PCWSTR(w.as_ptr()), ASSOC_FILTER_RECOMMENDED) else { return v };
    let mut un = [None];
    let mut n = 0;
    while liste.Next(&mut un, Some(&mut n)).is_ok() && n == 1 {
        let Some(g) = un[0].take() else { break };
        let Ok(p) = g.GetUIName() else { continue };
        let nom = p.to_string().unwrap_or_default();
        CoTaskMemFree(Some(p.0 as *const _));
        if !nom.is_empty() && !v.iter().any(|a| a.nom.eq_ignore_ascii_case(&nom)) {
            v.push(Application { nom, gestionnaire: g });
        }
        if v.len() >= 12 {
            break;
        }
    }
    v
}

/// Ouvre les fichiers avec cette application
pub unsafe fn ouvrir_avec(hwnd: HWND, chemins: &[String], a: &Application) -> Result<()> {
    let donnees: IDataObject = objet_elements(hwnd, chemins)?;
    a.gestionnaire.Invoke(&donnees)
}

/// Boîte « Ouvrir avec » de Windows (toutes les applications, Microsoft Store, « toujours utiliser »)
pub unsafe fn choisir_application(hwnd: HWND, chemin: &str) -> Result<()> {
    let w = wide(chemin);
    let info = OPENASINFO { pcszFile: PCWSTR(w.as_ptr()), pcszClass: PCWSTR::null(), oaifInFlags: OAIF_ALLOW_REGISTRATION | OAIF_REGISTER_EXT | OAIF_EXEC };
    match SHOpenWithDialog(hwnd, &info) {
        Err(e) if e.code() != ERROR_CANCELLED.to_hresult() => Err(e),
        _ => Ok(()),
    }
}

/// Nom interne d'une commande du menu (« rename », « delete »…)
unsafe fn verbe(cm: &IContextMenu, decalage: u32) -> String {
    let mut tampon = [0u16; 64];
    if cm.GetCommandString(decalage as usize, GCS_VERBW, None, PSTR(tampon.as_mut_ptr() as *mut u8), tampon.len() as u32).is_ok() {
        let n = tampon.iter().position(|&c| c == 0).unwrap_or(0);
        String::from_utf16_lossy(&tampon[..n])
    } else {
        String::new()
    }
}

// ───────────────────────── Glisser vers une autre application ─────────────────────────

/// Glisse les éléments (bloquant jusqu'au dépôt). Le Shell fournit l'aperçu et le curseur.
pub unsafe fn glisser(hwnd: HWND, chemins: &[String]) -> Result<DROPEFFECT> {
    let donnees: IDataObject = objet_elements(hwnd, chemins)?;
    // Boucle modale jusqu'au dépôt (voir `GardeModale`)
    let _garde = crate::fenetre::GardeModale::nouvelle(hwnd);
    SHDoDragDrop(hwnd, &donnees, None, DROPEFFECT_COPY | DROPEFFECT_MOVE | DROPEFFECT_LINK)
}

// ───────────────────────── Déposer dans SpaceFiles ─────────────────────────

/// Liens entre la cible de dépôt et la fenêtre
pub struct Rappels {
    /// Point écran → dossier où déposer, ou None si on ne peut pas déposer là
    pub destination: unsafe fn(HWND, POINT) -> Option<String>,
    /// Point écran → ce qu'il faut mettre en évidence (None : rien)
    pub survol: unsafe fn(HWND, Option<POINT>),
    /// Fichiers déposés : (sources, dossier, déplacer ?)
    pub deposer: unsafe fn(HWND, Vec<String>, String, bool),
    /// Fichiers déposés avec le bouton droit : (sources, dossier, point écran) — on demande quoi faire
    pub deposer_droit: unsafe fn(HWND, Vec<String>, String, POINT),
}

/// Crée un raccourci « nom - Raccourci.lnk » dans `dest` pour chaque source ; renvoie combien
pub unsafe fn raccourcis(sources: &[String], dest: &str) -> Result<usize> {
    let dossier = dest.trim_end_matches('\\');
    let mut n = 0;
    for s in sources {
        let lien: IShellLinkW = CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER)?;
        lien.SetPath(&HSTRING::from(s.as_str()))?;
        let _ = lien.SetWorkingDirectory(&HSTRING::from(parent(s)));
        let nom: String = s.trim_end_matches('\\').rsplit('\\').next().unwrap_or(s).chars().filter(|c| *c != ':').collect();
        let mut cible = format!("{dossier}\\{nom} - Raccourci.lnk");
        let mut k = 2;
        while std::path::Path::new(&cible).exists() {
            cible = format!("{dossier}\\{nom} - Raccourci ({k}).lnk");
            k += 1;
        }
        let fichier: IPersistFile = lien.cast()?;
        fichier.Save(&HSTRING::from(cible), TRUE)?;
        n += 1;
    }
    Ok(n)
}

/// Chemins d'un glisser-déposer de fichiers (format CF_HDROP)
unsafe fn fichiers(d: &IDataObject) -> Option<Vec<String>> {
    let format = FORMATETC { cfFormat: CF_HDROP.0, ptd: std::ptr::null_mut(), dwAspect: DVASPECT_CONTENT.0, lindex: -1, tymed: TYMED_HGLOBAL.0 as u32 };
    let mut m = d.GetData(&format).ok()?;
    let hdrop = HDROP(m.u.hGlobal.0);
    let n = DragQueryFileW(hdrop, u32::MAX, None);
    let mut v = Vec::new();
    for i in 0..n {
        let len = DragQueryFileW(hdrop, i, None) as usize;
        let mut buf = vec![0u16; len + 1];
        DragQueryFileW(hdrop, i, Some(&mut buf));
        v.push(String::from_utf16_lossy(&buf[..len]));
    }
    ReleaseStgMedium(&mut m);
    if v.is_empty() {
        None
    } else {
        Some(v)
    }
}

fn parent(chemin: &str) -> &str {
    let c = chemin.trim_end_matches('\\');
    match c.rfind('\\') {
        Some(i) if i <= 2 => &c[..=i],
        Some(i) => &c[..i],
        None => c,
    }
}

fn meme_chemin(a: &str, b: &str) -> bool {
    a.trim_end_matches('\\').eq_ignore_ascii_case(b.trim_end_matches('\\'))
}

/// Effet d'un dépôt de fichiers, comme l'Explorateur : déplacer sur le même disque, copier sinon,
/// Ctrl force la copie, Maj le déplacement
fn effet_fichiers(sources: &[String], dest: &str, touches: MODIFIERKEYS_FLAGS, permis: DROPEFFECT) -> DROPEFFECT {
    // Groupe ou téléphone du volet : rien ne bouge sur le disque, la source ne doit rien effacer
    if dest.starts_with('\u{1}') {
        return if permis.0 & DROPEFFECT_COPY.0 != 0 { DROPEFFECT_COPY } else if permis.0 & DROPEFFECT_LINK.0 != 0 { DROPEFFECT_LINK } else { DROPEFFECT_NONE };
    }
    let dest_n = dest.trim_end_matches('\\').to_lowercase();
    for s in sources {
        let s_n = s.trim_end_matches('\\').to_lowercase();
        // Un dossier dans lui-même ou dans un de ses sous-dossiers
        if dest_n == s_n || dest_n.starts_with(&format!("{s_n}\\")) {
            return DROPEFFECT_NONE;
        }
    }
    let deja_la = sources.iter().all(|s| meme_chemin(parent(s), dest));
    let racine = |c: &str| c.get(..2).map(str::to_lowercase);
    let meme_disque = !dest.starts_with("\\\\") && sources.iter().all(|s| racine(s) == racine(dest));
    let voulu = if touches.0 & MK_CONTROL.0 != 0 {
        DROPEFFECT_COPY
    } else if touches.0 & MK_SHIFT.0 != 0 || meme_disque {
        DROPEFFECT_MOVE
    } else {
        DROPEFFECT_COPY
    };
    // Déplacer vers le dossier où ils sont déjà ne fait rien
    if voulu == DROPEFFECT_MOVE && deja_la {
        return DROPEFFECT_NONE;
    }
    if permis.0 & voulu.0 != 0 {
        voulu
    } else if permis.0 & DROPEFFECT_COPY.0 != 0 && !deja_la {
        DROPEFFECT_COPY
    } else {
        DROPEFFECT_NONE
    }
}

#[derive(Default)]
struct EnCours {
    donnees: Option<IDataObject>,
    /// Fichiers ordinaires : on les copie nous-mêmes (barre de progression en points)
    fichiers: Option<Vec<String>>,
    /// Autres données (pièce jointe, image d'un navigateur…) : confiées au Shell du dossier visé
    shell: Option<(String, IDropTarget)>,
    effet: DROPEFFECT,
    /// Glisser avec le bouton droit : au dépôt, un menu demande quoi faire
    droit: bool,
}

#[implement(IDropTarget)]
pub struct Depot {
    fenetre: isize,
    rappels: Rappels,
    aide: Option<IDropTargetHelper>,
    etat: RefCell<EnCours>,
}

impl Depot {
    pub fn nouveau(hwnd: HWND, rappels: Rappels) -> IDropTarget {
        let aide = unsafe { CoCreateInstance(&CLSID_DragDropHelper, None, CLSCTX_INPROC_SERVER).ok() };
        Depot { fenetre: hwnd.0 as isize, rappels, aide, etat: RefCell::new(EnCours::default()) }.into()
    }

    fn hwnd(&self) -> HWND {
        HWND(self.fenetre as *mut _)
    }

    /// Calcule l'effet au point donné, et prépare le dépôt Shell si besoin
    unsafe fn survoler(&self, touches: MODIFIERKEYS_FLAGS, pt: &POINTL, effet: *mut DROPEFFECT) {
        let p = POINT { x: pt.x, y: pt.y };
        let permis = *effet;
        let dest = (self.rappels.destination)(self.hwnd(), p);
        let mut e = self.etat.borrow_mut();
        let resultat = match (&dest, e.fichiers.clone()) {
            (None, _) => DROPEFFECT_NONE,
            (Some(d), Some(f)) => effet_fichiers(&f, d, touches, permis),
            (Some(d), None) => {
                // Le dossier visé a changé : on quitte l'ancien dépôt Shell et on entre dans le nouveau
                if e.shell.as_ref().map_or(true, |(c, _)| !meme_chemin(c, d)) {
                    if let Some((_, ancien)) = e.shell.take() {
                        let _ = ancien.DragLeave();
                    }
                    if let (Ok(cible), Some(donnees)) = (dossier_shell(d).and_then(|s| s.CreateViewObject::<_, IDropTarget>(self.hwnd())), &e.donnees) {
                        let mut ef = permis;
                        if cible.DragEnter(donnees, touches, *pt, &mut ef).is_ok() {
                            e.shell = Some((d.clone(), cible));
                        }
                    }
                }
                match &e.shell {
                    Some((_, cible)) => {
                        let mut ef = permis;
                        if cible.DragOver(touches, *pt, &mut ef).is_ok() {
                            ef
                        } else {
                            DROPEFFECT_NONE
                        }
                    }
                    None => DROPEFFECT_NONE,
                }
            }
        };
        e.effet = resultat;
        *effet = resultat;
        drop(e);
        (self.rappels.survol)(self.hwnd(), if resultat == DROPEFFECT_NONE { None } else { Some(p) });
    }

    unsafe fn terminer(&self) {
        let mut e = self.etat.borrow_mut();
        if let Some((_, cible)) = e.shell.take() {
            let _ = cible.DragLeave();
        }
        *e = EnCours::default();
        drop(e);
        (self.rappels.survol)(self.hwnd(), None);
    }
}

#[allow(non_snake_case)]
impl IDropTarget_Impl for Depot_Impl {
    fn DragEnter(&self, donnees: Option<&IDataObject>, touches: MODIFIERKEYS_FLAGS, pt: &POINTL, effet: *mut DROPEFFECT) -> Result<()> {
        unsafe {
            {
                let mut e = self.etat.borrow_mut();
                *e = EnCours::default();
                e.donnees = donnees.cloned();
                e.fichiers = donnees.and_then(|d| fichiers(d));
                e.droit = touches.0 & MK_RBUTTON.0 != 0;
            }
            if let (Some(aide), Some(d)) = (&self.aide, donnees) {
                let _ = aide.DragEnter(self.hwnd(), d, &POINT { x: pt.x, y: pt.y }, *effet);
            }
            self.survoler(touches, pt, effet);
        }
        Ok(())
    }

    fn DragOver(&self, touches: MODIFIERKEYS_FLAGS, pt: &POINTL, effet: *mut DROPEFFECT) -> Result<()> {
        unsafe {
            self.survoler(touches, pt, effet);
            if let Some(aide) = &self.aide {
                let _ = aide.DragOver(&POINT { x: pt.x, y: pt.y }, *effet);
            }
        }
        Ok(())
    }

    fn DragLeave(&self) -> Result<()> {
        unsafe {
            if let Some(aide) = &self.aide {
                let _ = aide.DragLeave();
            }
            self.terminer();
        }
        Ok(())
    }

    fn Drop(&self, donnees: Option<&IDataObject>, touches: MODIFIERKEYS_FLAGS, pt: &POINTL, effet: *mut DROPEFFECT) -> Result<()> {
        unsafe {
            self.survoler(touches, pt, effet);
            if let (Some(aide), Some(d)) = (&self.aide, donnees) {
                let _ = aide.Drop(d, &POINT { x: pt.x, y: pt.y }, *effet);
            }
            let p = POINT { x: pt.x, y: pt.y };
            let dest = (self.rappels.destination)(self.hwnd(), p);
            let mut e = self.etat.borrow_mut();
            if *effet != DROPEFFECT_NONE {
                if let (Some(d), Some(f)) = (dest, e.fichiers.take()) {
                    let deplacer = *effet == DROPEFFECT_MOVE;
                    let droit = e.droit;
                    drop(e);
                    if droit {
                        // La source ne doit rien effacer : c'est le menu qui décidera
                        *effet = DROPEFFECT_COPY;
                        (self.rappels.deposer_droit)(self.hwnd(), f, d, p);
                    } else {
                        (self.rappels.deposer)(self.hwnd(), f, d, deplacer);
                    }
                    self.terminer();
                    return Ok(());
                }
                if let (Some((_, cible)), Some(d)) = (e.shell.take(), donnees) {
                    drop(e);
                    let _ = cible.Drop(d, touches, *pt, effet);
                    self.terminer();
                    return Ok(());
                }
            }
            drop(e);
            self.terminer();
        }
        Ok(())
    }
}

/// Menus Win32 (clic droit) en thème sombre ou clair selon le thème de la fenêtre : fonctions non documentées d'uxtheme,
/// utilisées par l'Explorateur. À rappeler quand le thème change (le menu est recréé à chaque clic droit).
///
/// Ordinaux 135 (SetPreferredAppMode) et 136 (FlushMenuThemes) : valables depuis Windows 10 1903 (build 18362) ; en 1809 le
/// 135 est AllowDarkModeForApp (BOOL), sans effet utile ici. Le projet cible Windows 11 : introuvables, on ne fait rien.
pub fn menus_theme(clair: bool) {
    use windows::Win32::System::LibraryLoader::{GetModuleHandleW, GetProcAddress, LoadLibraryW};
    // SAFETY: appelée sur le fil d'interface. uxtheme.dll est déjà chargée dans tout processus à fenêtres (GetModuleHandleW
    // ne compte pas de référence : rien à libérer) ; en dernier recours on la charge une fois, elle reste ensuite chargée.
    // Chaque adresse est transmutée vers la signature réelle de l'ordinal (SetPreferredAppMode(int) -> int,
    // FlushMenuThemes() sans argument) et n'est appelée que si GetProcAddress l'a trouvée.
    unsafe {
        let ux = match GetModuleHandleW(windows::core::w!("uxtheme.dll")) {
            Ok(m) => m,
            Err(_) => match LoadLibraryW(windows::core::w!("uxtheme.dll")) {
                Ok(m) => m,
                Err(_) => return,
            },
        };
        // 135 : SetPreferredAppMode(ForceDark = 2, ForceLight = 3), 136 : FlushMenuThemes
        if let Some(f) = GetProcAddress(ux, PCSTR(135 as *const u8)) {
            let f: unsafe extern "system" fn(i32) -> i32 = std::mem::transmute(f);
            f(if clair { 3 } else { 2 });
        }
        if let Some(f) = GetProcAddress(ux, PCSTR(136 as *const u8)) {
            let f: unsafe extern "system" fn() = std::mem::transmute(f);
            f();
        }
    }
}
