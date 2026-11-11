// Opérations sur les fichiers : presse-papiers, copie/déplacement/suppression (moteur de Windows),
// renommage, nouveau dossier

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use windows::core::{implement, w, Result, HRESULT, PCWSTR};
use windows::Win32::Foundation::*;
use windows::Win32::System::Com::*;
use windows::Win32::System::DataExchange::*;
use windows::Win32::System::Memory::*;
use windows::Win32::System::Ole::{CF_HDROP, CF_UNICODETEXT};
use windows::Win32::UI::Shell::*;
use windows::Win32::UI::WindowsAndMessaging::{PostMessageW, WM_APP};

/// Progression d'une opération (WPARAM = avancement sur 1000)
pub const WM_PROGRES: u32 = WM_APP + 3;
/// Fin d'une opération (LPARAM = Box<Bilan>)
pub const WM_OPERATION_FINIE: u32 = WM_APP + 4;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Genre {
    Copier,
    Deplacer,
    Corbeille,
    SupprimerDefinitivement,
}

impl Genre {
    pub fn libelle(self) -> &'static str {
        match self {
            Genre::Copier => "COPIE",
            Genre::Deplacer => "DEPLACEMENT",
            Genre::Corbeille => "CORBEILLE",
            Genre::SupprimerDefinitivement => "SUPPRESSION",
        }
    }
}

pub struct Bilan {
    /// Numéro de l'opération (le même que dans `App::Operation`)
    pub numero: u64,
    pub genre: Genre,
    pub nombre: usize,
    pub annule: bool,
    pub erreur: Option<String>,
    /// Pour chaque élément traité : chemin d'origine, et nouveau chemin (copie, destination, place dans la corbeille)
    pub resultats: Vec<(String, Option<String>)>,
    /// Éléments remplacés (conflits) : chemin, place dans la corbeille
    pub remplaces: Vec<(String, String)>,
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

// ───────────────────────── Presse-papiers ─────────────────────────

/// Bloc mémoire global rempli avec ces octets, pour le presse-papiers
unsafe fn bloc_global(octets: &[u8]) -> Option<HANDLE> {
    let h = GlobalAlloc(GMEM_MOVEABLE, octets.len()).ok()?;
    let p = GlobalLock(h) as *mut u8;
    if p.is_null() {
        let _ = GlobalFree(h);
        return None;
    }
    std::ptr::copy_nonoverlapping(octets.as_ptr(), p, octets.len());
    let _ = GlobalUnlock(h);
    Some(HANDLE(h.0))
}

/// Met des fichiers dans le presse-papiers, au même format que l'Explorateur.
/// `couper` : le collage les déplacera au lieu de les copier.
pub fn copier_fichiers(hwnd: HWND, chemins: &[String], couper: bool) -> bool {
    // DROPFILES suivi des chemins UTF-16, chacun terminé par 0, puis un 0 final
    let entete = DROPFILES { pFiles: std::mem::size_of::<DROPFILES>() as u32, pt: POINT::default(), fNC: FALSE, fWide: TRUE };
    let mut octets = unsafe {
        std::slice::from_raw_parts(&entete as *const _ as *const u8, std::mem::size_of::<DROPFILES>()).to_vec()
    };
    for c in chemins {
        for u in wide(c) {
            octets.extend_from_slice(&u.to_le_bytes());
        }
    }
    octets.extend_from_slice(&[0, 0]);
    let effet: u32 = if couper { 2 } else { 1 }; // DROPEFFECT_MOVE / DROPEFFECT_COPY
    unsafe {
        if OpenClipboard(hwnd).is_err() {
            return false;
        }
        let _ = EmptyClipboard();
        let mut ok = false;
        if let Some(h) = bloc_global(&octets) {
            ok = SetClipboardData(CF_HDROP.0 as u32, h).is_ok();
            // Refusé : le bloc n'appartient pas au système, il fuirait
            if !ok {
                let _ = GlobalFree(HGLOBAL(h.0));
            }
        }
        if let Some(h) = bloc_global(&effet.to_le_bytes()) {
            if SetClipboardData(RegisterClipboardFormatW(w!("Preferred DropEffect")), h).is_err() {
                let _ = GlobalFree(HGLOBAL(h.0));
            }
        }
        let _ = CloseClipboard();
        ok
    }
}

/// Y a-t-il des fichiers à coller ? (sans ouvrir le presse-papiers)
pub fn fichiers_a_coller() -> bool {
    unsafe { IsClipboardFormatAvailable(CF_HDROP.0 as u32).is_ok() }
}

/// Fichiers présents dans le presse-papiers, et s'ils ont été coupés
pub fn lire_fichiers(hwnd: HWND) -> Option<(Vec<String>, bool)> {
    unsafe {
        if IsClipboardFormatAvailable(CF_HDROP.0 as u32).is_err() || OpenClipboard(hwnd).is_err() {
            return None;
        }
        let mut chemins = Vec::new();
        if let Ok(h) = GetClipboardData(CF_HDROP.0 as u32) {
            let hdrop = HDROP(h.0);
            let n = DragQueryFileW(hdrop, u32::MAX, None);
            for i in 0..n {
                let len = DragQueryFileW(hdrop, i, None) as usize;
                let mut buf = vec![0u16; len + 1];
                DragQueryFileW(hdrop, i, Some(&mut buf));
                chemins.push(String::from_utf16_lossy(&buf[..len]));
            }
        }
        let mut couper = false;
        if let Ok(h) = GetClipboardData(RegisterClipboardFormatW(w!("Preferred DropEffect"))) {
            let g = HGLOBAL(h.0);
            let p = GlobalLock(g) as *const u32;
            if !p.is_null() {
                couper = GlobalSize(g) >= 4 && *p & 2 != 0;
                let _ = GlobalUnlock(g);
            }
        }
        let _ = CloseClipboard();
        if chemins.is_empty() {
            None
        } else {
            Some((chemins, couper))
        }
    }
}

/// Vide le presse-papiers (après avoir collé des fichiers coupés, comme l'Explorateur)
pub fn vider(hwnd: HWND) {
    unsafe {
        if OpenClipboard(hwnd).is_ok() {
            let _ = EmptyClipboard();
            let _ = CloseClipboard();
        }
    }
}

pub fn copier_texte(hwnd: HWND, texte: &str) {
    let mut octets = Vec::new();
    for u in wide(texte) {
        octets.extend_from_slice(&u.to_le_bytes());
    }
    unsafe {
        if OpenClipboard(hwnd).is_err() {
            return;
        }
        let _ = EmptyClipboard();
        if let Some(h) = bloc_global(&octets) {
            if SetClipboardData(CF_UNICODETEXT.0 as u32, h).is_err() {
                let _ = GlobalFree(HGLOBAL(h.0));
            }
        }
        let _ = CloseClipboard();
    }
}

pub fn lire_texte(hwnd: HWND) -> Option<String> {
    unsafe {
        if IsClipboardFormatAvailable(CF_UNICODETEXT.0 as u32).is_err() || OpenClipboard(hwnd).is_err() {
            return None;
        }
        let mut r = None;
        if let Ok(h) = GetClipboardData(CF_UNICODETEXT.0 as u32) {
            let g = HGLOBAL(h.0);
            let p = GlobalLock(g) as *const u16;
            if !p.is_null() {
                // Borné par la taille du bloc : une autre appli peut poser un texte sans zéro final
                let max = GlobalSize(g) / 2;
                let mut n = 0;
                while n < max && *p.add(n) != 0 {
                    n += 1;
                }
                r = Some(String::from_utf16_lossy(std::slice::from_raw_parts(p, n)));
                let _ = GlobalUnlock(g);
            }
        }
        let _ = CloseClipboard();
        r
    }
}

// ───────────────────────── Moteur de copie de Windows ─────────────────────────

/// Reçoit la progression de Windows et la transmet à la fenêtre
#[implement(IFileOperationProgressSink)]
struct Suivi {
    fenetre: isize,
    /// Numéro de l'opération, repris dans WM_PROGRES pour que seul l'onglet concerné avance
    numero: u64,
    annule: Arc<AtomicBool>,
    /// Au moins un rappel a refusé un élément à cause de l'annulation : le moteur traite E_ABORT comme « élément
    /// ignoré » et ne lève pas `GetAnyOperationsAborted`, donc c'est `executer` qui doit le savoir (D1)
    refuse: Arc<AtomicBool>,
    /// Pause demandée : on retient Windows dans ses rappels, la copie s'arrête net
    pause: Arc<AtomicBool>,
    /// Éléments de premier niveau demandés (les rappels arrivent aussi pour leur contenu), avec leur `IShellItem` :
    /// c'est lui qui les identifie (B1), le Shell rendant des chemins abrégés au-delà de 260 caractères
    sources: Vec<(String, IShellItem)>,
    resultats: Arc<Mutex<Vec<(String, Option<String>)>>>,
    /// Éléments déjà présents mis à la corbeille avant d'être remplacés
    remplaces: Vec<(String, IShellItem)>,
    mis_de_cote: Arc<Mutex<Vec<(String, String)>>>,
    /// Copie réseau bridée (réglage) : octets par seconde, 0 = sans limite
    limite: u64,
    /// Octets à copier en tout (pour traduire la progression de Windows en octets)
    octets: u64,
    debut: std::time::Instant,
    /// Dernier avancement annoncé par le Shell (unités du Shell, en pratique des octets) : repris par l'événement `paused`
    /// envoyé à SpaceNotch (`eco::progression`)
    dernier_fait: AtomicU64,
    dernier_total: AtomicU64,
}

/// Chemin sur le réseau : `\\serveur\partage` ou lecteur réseau (Z:)
pub fn sur_le_reseau(chemin: &str) -> bool {
    if chemin.starts_with("\\\\") {
        return true;
    }
    let Some(racine) = chemin.get(..3).filter(|r| r.ends_with(":\\")) else { return false };
    let w: Vec<u16> = racine.encode_utf16().chain(std::iter::once(0)).collect();
    unsafe { windows::Win32::Storage::FileSystem::GetDriveTypeW(PCWSTR(w.as_ptr())) == 4 }
}

/// Destination où une copie peut mal tourner sans qu'on le voie : partage réseau, NAS, clé USB ou disque amovible
pub fn support_externe(chemin: &str) -> bool {
    sur_le_reseau(chemin) || amovible(chemin)
}

/// Lecteur amovible (clé USB, disque externe) : type 2 de `GetDriveTypeW`
pub fn amovible(chemin: &str) -> bool {
    let Some(racine) = chemin.get(..3).filter(|r| r.ends_with(":\\")) else { return false };
    let w: Vec<u16> = racine.encode_utf16().chain(std::iter::once(0)).collect();
    unsafe { windows::Win32::Storage::FileSystem::GetDriveTypeW(PCWSTR(w.as_ptr())) == 2 }
}

/// Taille totale d'éléments (dossiers parcourus), pour la limite de vitesse
fn taille_totale(chemins: &[String]) -> u64 {
    fn taille(p: &std::path::Path) -> u64 {
        match std::fs::symlink_metadata(p) {
            Ok(m) if m.is_dir() => std::fs::read_dir(p).map(|d| d.flatten().map(|e| taille(&e.path())).sum()).unwrap_or(0),
            Ok(m) => m.len(),
            Err(_) => 0,
        }
    }
    chemins.iter().map(|c| taille(std::path::Path::new(c))).sum()
}

/// Chemin de fichier d'un élément du Shell
fn chemin_de(item: Option<&IShellItem>) -> Option<String> {
    unsafe {
        let p = item?.GetDisplayName(SIGDN_FILESYSPATH).ok()?;
        let s = p.to_string().ok();
        CoTaskMemFree(Some(p.0 as *const _));
        s
    }
}

/// Le Shell abrège un chemin de plus de 260 caractères (`SPACEF~1\FICHIE~1.TXT`, de faux noms 8.3 que `GetLongPathNameW`
/// ne retrouve pas) : deux fichiers voisins ont des abrégés qui se confondent. Deux éléments sont donc comparés par
/// `IShellItem::Compare` (identité canonique du Shell). Si la comparaison échoue, repli sur l'égalité exacte des chemins.
fn meme_element(a: &IShellItem, b: &IShellItem, chemin_a: Option<&str>, chemin_b: &str) -> bool {
    // SAFETY: deux `IShellItem` vivants, appelés depuis le fil d'interface de l'opération
    match unsafe { a.Compare(b, SICHINT_CANONICAL.0 as u32) } {
        Ok(ordre) => ordre == 0,
        Err(_) => chemin_a.is_some_and(|c| c.trim_end_matches('\\').eq_ignore_ascii_case(chemin_b.trim_end_matches('\\'))),
    }
}

/// Chemin d'un élément créé par l'opération (copie, déplacement, corbeille) qui existe vraiment : un chemin abrégé est
/// relu en entier depuis l'identifiant du Shell (PIDL, tampon de 32 768 unités) ; s'il reste faux on ne rend rien (I1) :
/// pas d'entrée d'historique vaut mieux qu'une entrée qui pointe vers un chemin inexistant.
fn chemin_reel(item: Option<&IShellItem>) -> Option<String> {
    let s = chemin_de(item)?;
    if !s.contains('~') {
        return Some(s);
    }
    // Chemin abrégé qui existe (vrai nom 8.3) : on le développe ; sinon on relit le chemin du PIDL
    if std::path::Path::new(&s).exists() {
        return Some(developper(&s));
    }
    // SAFETY: le PIDL rendu par `SHGetIDListFromObject` est libéré par `CoTaskMemFree` ; le tampon a la taille annoncée
    unsafe {
        let pidl = SHGetIDListFromObject(item?).ok()?;
        let mut tampon = vec![0u16; 32768];
        let ok = SHGetPathFromIDListEx(pidl, &mut tampon, GPFIDL_DEFAULT).as_bool();
        CoTaskMemFree(Some(pidl as *const _));
        let fin = tampon.iter().position(|&u| u == 0).unwrap_or(tampon.len());
        let long = String::from_utf16_lossy(&tampon[..fin]);
        (ok && std::path::Path::new(&long).exists()).then(|| developper(&long))
    }
}

/// Chemin développé par `GetLongPathNameW` (`SPACEF~1` -> `spacefiles-matrice-…`), sans préfixe long ; inchangé s'il échoue.
/// L'élément doit exister : un élément supprimé n'a plus de nom court à développer.
fn developper(chemin: &str) -> String {
    let entree = wide(&avec_prefixe_long(chemin));
    // SAFETY: `entree` est terminé par 0 et vit pendant les deux appels ; le tampon a la taille annoncée par le premier
    unsafe {
        let taille = windows::Win32::Storage::FileSystem::GetLongPathNameW(PCWSTR(entree.as_ptr()), None);
        if taille == 0 {
            return chemin.to_string();
        }
        let mut tampon = vec![0u16; taille as usize];
        let n = windows::Win32::Storage::FileSystem::GetLongPathNameW(PCWSTR(entree.as_ptr()), Some(&mut tampon)) as usize;
        if n == 0 || n >= tampon.len() {
            return chemin.to_string();
        }
        let long = String::from_utf16_lossy(&tampon[..n]);
        if let Some(reste) = long.strip_prefix(r"\\?\UNC\") {
            format!(r"\\{reste}")
        } else {
            long.strip_prefix(r"\\?\").unwrap_or(&long).to_string()
        }
    }
}

/// Chemin au format long de Win32 (`\\?\C:\…`, `\\?\UNC\serveur\…`), seul à passer au-delà de 260 caractères.
/// Un chemin déjà préfixé, ou relatif, est rendu tel quel : les chemins courts ne changent pas de comportement.
pub fn avec_prefixe_long(chemin: &str) -> String {
    // Déjà préfixé, ou chemin de périphérique `\\.\` : rendu tel quel (`\\?\UNC\.\…` n'existe pas)
    if chemin.starts_with(r"\\?\") || chemin.starts_with(r"\\.\") {
        chemin.to_string()
    } else if let Some(reste) = chemin.strip_prefix(r"\\") {
        format!(r"\\?\UNC\{reste}")
    } else if chemin.get(1..3) == Some(r":\") {
        format!(r"\\?\{chemin}")
    } else {
        chemin.to_string()
    }
}

impl Suivi {
    fn noter(&self, item: Option<&IShellItem>, nouveau: Option<&IShellItem>, r: HRESULT) {
        if r.is_err() {
            if reseau(r) {
                COUPURE.with(|c| c.set(true));
            }
            return;
        }
        // L'élément est reconnu par son `IShellItem` (B1), jamais par son chemin : au-delà de 260 caractères le Shell rend un
        // chemin abrégé qui peut désigner plusieurs éléments voisins. Le chemin annoncé est celui de la demande.
        let Some(item) = item else { return };
        let vu = chemin_de(Some(item));
        let meme = |c: &&(String, IShellItem)| meme_element(item, &c.1, vu.as_deref(), &c.0);
        if let Some((origine, _)) = self.remplaces.iter().find(meme) {
            // Les appels COM se font avant de prendre le verrou (M1)
            let n = chemin_reel(nouveau);
            if let (Some(n), Ok(mut v)) = (n, self.mis_de_cote.lock()) {
                v.push((origine.clone(), n));
            }
            return;
        }
        if let Some((origine, _)) = self.sources.iter().find(meme) {
            let n = chemin_reel(nouveau);
            if let Ok(mut v) = self.resultats.lock() {
                v.push((origine.clone(), n));
            }
        }
    }

    /// Copie réseau bridée : en avance sur le débit permis, on retient Windows dans son rappel
    fn freiner(&self, total: u32, fait: u32) {
        if self.limite == 0 || total == 0 || self.octets == 0 {
            return;
        }
        let faits = self.octets as f64 * fait as f64 / total as f64;
        let permis = self.debut.elapsed().as_secs_f64() * self.limite as f64;
        if faits > permis {
            let attente = ((faits - permis) / self.limite as f64).min(0.5);
            std::thread::sleep(std::time::Duration::from_secs_f64(attente));
        }
    }

    fn verifier(&self) -> Result<()> {
        if self.pause.load(Ordering::Relaxed) && !self.annule.load(Ordering::Relaxed) {
            // Une annonce à l'entrée en pause ; la reprise est annoncée par la mise à jour suivante. Sans effet si SpaceNotch est absent.
            crate::eco::progression::annoncer(self.numero, crate::eco::progression::Etat::Paused, self.dernier_fait.load(Ordering::Relaxed), self.dernier_total.load(Ordering::Relaxed));
        }
        while self.pause.load(Ordering::Relaxed) && !self.annule.load(Ordering::Relaxed) {
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
        if self.annule.load(Ordering::Relaxed) {
            self.refuse.store(true, Ordering::Relaxed);
            Err(E_ABORT.into())
        } else {
            Ok(())
        }
    }
}

#[allow(non_snake_case)]
impl IFileOperationProgressSink_Impl for Suivi_Impl {
    fn StartOperations(&self) -> Result<()> {
        Ok(())
    }
    fn FinishOperations(&self, _: HRESULT) -> Result<()> {
        Ok(())
    }
    fn PreRenameItem(&self, _: u32, _: Option<&IShellItem>, _: &PCWSTR) -> Result<()> {
        self.verifier()
    }
    fn PostRenameItem(&self, _: u32, _: Option<&IShellItem>, _: &PCWSTR, _: HRESULT, _: Option<&IShellItem>) -> Result<()> {
        Ok(())
    }
    fn PreMoveItem(&self, _: u32, _: Option<&IShellItem>, _: Option<&IShellItem>, _: &PCWSTR) -> Result<()> {
        self.verifier()
    }
    fn PostMoveItem(&self, _: u32, item: Option<&IShellItem>, _: Option<&IShellItem>, _: &PCWSTR, r: HRESULT, nouveau: Option<&IShellItem>) -> Result<()> {
        self.noter(item, nouveau, r);
        Ok(())
    }
    fn PreCopyItem(&self, _: u32, _: Option<&IShellItem>, _: Option<&IShellItem>, _: &PCWSTR) -> Result<()> {
        self.verifier()
    }
    fn PostCopyItem(&self, _: u32, item: Option<&IShellItem>, _: Option<&IShellItem>, _: &PCWSTR, r: HRESULT, nouveau: Option<&IShellItem>) -> Result<()> {
        self.noter(item, nouveau, r);
        Ok(())
    }
    fn PreDeleteItem(&self, _: u32, _: Option<&IShellItem>) -> Result<()> {
        self.verifier()
    }
    fn PostDeleteItem(&self, _: u32, item: Option<&IShellItem>, r: HRESULT, nouveau: Option<&IShellItem>) -> Result<()> {
        // Mis à la corbeille : `nouveau` est l'élément dans la corbeille (de quoi le restaurer)
        self.noter(item, nouveau, r);
        Ok(())
    }
    fn PreNewItem(&self, _: u32, _: Option<&IShellItem>, _: &PCWSTR) -> Result<()> {
        Ok(())
    }
    fn PostNewItem(&self, _: u32, _: Option<&IShellItem>, _: &PCWSTR, _: &PCWSTR, _: u32, _: HRESULT, _: Option<&IShellItem>) -> Result<()> {
        Ok(())
    }
    fn UpdateProgress(&self, total: u32, fait: u32) -> Result<()> {
        if total > 0 {
            let millieme = (fait as u64 * 1000 / total as u64).min(1000) as usize;
            unsafe {
                let _ = PostMessageW(HWND(self.fenetre as *mut _), WM_PROGRES, WPARAM(millieme), LPARAM(self.numero as isize));
            }
            // Pour SpaceNotch (`progress`) : coalescé et sans entrée-sortie ici, voir `eco::progression`
            self.dernier_fait.store(fait as u64, Ordering::Relaxed);
            self.dernier_total.store(total as u64, Ordering::Relaxed);
            crate::eco::progression::annoncer(self.numero, crate::eco::progression::Etat::Running, fait as u64, total as u64);
        }
        self.freiner(total, fait);
        self.verifier()
    }
    fn ResetTimer(&self) -> Result<()> {
        Ok(())
    }
    fn PauseTimer(&self) -> Result<()> {
        Ok(())
    }
    fn ResumeTimer(&self) -> Result<()> {
        Ok(())
    }
}

/// Tests seulement : dernier avancement (millièmes) annoncé par le Shell à la boîte de progression
#[cfg(test)]
static PROGRES_DIALOGUE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// Boîte de progression « muette » donnée à `SetProgressDialog` (D2). Windows l'interroge pendant la copie d'un fichier
/// (`GetOperationStatus`) pour savoir si l'utilisateur a annulé : `PDOPS_CANCELLED` interrompt la copie dans le fichier,
/// là où `IFileOperationProgressSink` n'est rappelé qu'entre deux éléments. Aucune fenêtre : la progression est en points.
/// Le Shell peut appeler depuis un autre fil : un seul drapeau atomique, aucun verrou.
#[implement(IOperationsProgressDialog)]
struct Annulation {
    annule: Arc<AtomicBool>,
}

#[allow(non_snake_case)]
impl IOperationsProgressDialog_Impl for Annulation_Impl {
    fn StartProgressDialog(&self, _: HWND, _: u32) -> Result<()> {
        Ok(())
    }
    fn StopProgressDialog(&self) -> Result<()> {
        Ok(())
    }
    fn SetOperation(&self, _: SPACTION) -> Result<()> {
        Ok(())
    }
    fn SetMode(&self, _: u32) -> Result<()> {
        Ok(())
    }
    fn UpdateProgress(&self, fait: u64, total: u64, _: u64, _: u64, _: u64, _: u64) -> Result<()> {
        // Tests : l'avancement vu par le Shell pendant la copie d'un fichier, seul repère qui existe pendant le fichier
        // (la cible est verrouillée en lecture tant que la copie dure)
        #[cfg(test)]
        PROGRES_DIALOGUE.store(fait * 1000 / total.max(1), Ordering::Relaxed);
        #[cfg(not(test))]
        let _ = (fait, total);
        Ok(())
    }
    fn UpdateLocations(&self, _: Option<&IShellItem>, _: Option<&IShellItem>, _: Option<&IShellItem>) -> Result<()> {
        Ok(())
    }
    fn ResetTimer(&self) -> Result<()> {
        Ok(())
    }
    fn PauseTimer(&self) -> Result<()> {
        Ok(())
    }
    fn ResumeTimer(&self) -> Result<()> {
        Ok(())
    }
    fn GetMilliseconds(&self, courant: *mut u64, restant: *mut u64) -> Result<()> {
        // SAFETY: pointeurs fournis par le Shell pour cet appel ; ils peuvent être nuls
        unsafe {
            if !courant.is_null() {
                *courant = 0;
            }
            if !restant.is_null() {
                *restant = 0;
            }
        }
        Ok(())
    }
    fn GetOperationStatus(&self) -> Result<windows::Win32::UI::Shell::PropertiesSystem::PDOPSTATUS> {
        use windows::Win32::UI::Shell::PropertiesSystem::{PDOPS_CANCELLED, PDOPS_RUNNING};
        Ok(if self.annule.load(Ordering::Relaxed) { PDOPS_CANCELLED } else { PDOPS_RUNNING })
    }
}

unsafe fn element(chemin: &str) -> Result<IShellItem> {
    let w = wide(chemin);
    SHCreateItemFromParsingName(PCWSTR(w.as_ptr()), None)
}

/// Un élément d'une opération : où il va, sous quel nom, et ce qu'il remplace
#[derive(Clone, Debug)]
pub struct Element {
    pub source: String,
    /// Dossier de destination (aucun pour une suppression)
    pub dest: Option<String>,
    /// Nouveau nom (conflit : « garder les deux »)
    pub nom: Option<String>,
    /// Élément déjà présent à mettre à la corbeille juste avant (conflit : « remplacer »)
    pub remplace: Option<String>,
}

impl Element {
    pub fn simple(source: String, dest: Option<String>) -> Self {
        Element { source, dest, nom: None, remplace: None }
    }
}

type Notes = Arc<Mutex<Vec<(String, Option<String>)>>>;

thread_local! {
    /// Un élément a échoué pour cause de réseau pendant l'opération de ce fil
    static COUPURE: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Opérations qui attendent le retour du réseau pour reprendre (numéros)
static ATTENTES: Mutex<Vec<u64>> = Mutex::new(Vec::new());

pub fn attend_reseau(numero: u64) -> bool {
    ATTENTES.lock().unwrap_or_else(|e| e.into_inner()).contains(&numero)
}

/// Erreur de réseau (serveur perdu, connexion coupée) : de quoi reprendre plus tard
fn reseau(h: HRESULT) -> bool {
    let h = h.0 as u32;
    let code = if h & 0xFFFF_0000 == 0x8007_0000 { h & 0xFFFF } else { h };
    matches!(code, 51 | 53 | 59 | 64 | 67 | 121 | 1222 | 1231 | 1232 | 1236 | 1240)
}

/// L'opération en cours est-elle bridée par la limite de vitesse réseau ? (barre d'état)
pub static LIMITEE: AtomicBool = AtomicBool::new(false);

unsafe fn executer(hwnd: isize, numero: u64, genre: Genre, elements: &[Element], controle: Controle, resultats: Notes, mis_de_cote: Arc<Mutex<Vec<(String, String)>>>, reprise: bool) -> Result<bool> {
    let op: IFileOperation = CoCreateInstance(&FileOperation, None, CLSCTX_ALL)?;
    // Pas de fenêtre de progression Windows : la nôtre est en points. Les conflits sont réglés avant
    // (conflit.rs) ; restent à Windows les questions rares (suppression définitive, droits…).
    let mut drapeaux = FOF_SILENT.0 | FOF_NOCONFIRMMKDIR.0;
    match genre {
        Genre::Copier | Genre::Deplacer => {
            drapeaux |= FOF_ALLOWUNDO.0;
            // Reprise après coupure : ce qui était à moitié copié est remplacé sans question
            if reprise {
                drapeaux |= FOF_NOCONFIRMATION.0;
            }
            // Coller dans le même dossier : « Nom - Copie » comme l'Explorateur
            let meme_dossier = elements.iter().all(|e| {
                e.nom.is_none() && e.dest.as_deref().is_some_and(|d| e.source.rsplit_once('\\').is_some_and(|(p, _)| p.trim_end_matches('\\').eq_ignore_ascii_case(d.trim_end_matches('\\'))))
            });
            if meme_dossier {
                drapeaux |= FOF_RENAMEONCOLLISION.0;
            }
            // Les éléments remplacés partent à la corbeille (récupérables) sans nouvelle question : la carte
            // des conflits vient d'avoir la réponse. Windows prévient quand même si la corbeille est impossible.
            if elements.iter().any(|e| e.remplace.is_some()) {
                drapeaux |= FOFX_RECYCLEONDELETE.0 as u32 | FOF_NOCONFIRMATION.0 | FOF_WANTNUKEWARNING.0;
            }
        }
        Genre::Corbeille => drapeaux |= FOF_ALLOWUNDO.0 | FOFX_RECYCLEONDELETE.0 as u32,
        Genre::SupprimerDefinitivement => {}
    }
    // Tests seulement : Windows ouvre de vraies boîtes (suppression définitive, lecture seule, fichier en cours
    // d'utilisation) et attend un clic ; sans elles la suite dépendait de la durée de vie de ces boîtes (12 s à plus
    // de 500 s) et affichait des fenêtres sur le bureau de l'auteur. L'exe garde le comportement ci-dessus.
    #[cfg(test)]
    {
        drapeaux |= FOF_NOCONFIRMATION.0 | FOF_NOERRORUI.0;
    }
    op.SetOperationFlags(FILEOPERATION_FLAGS(drapeaux))?;
    op.SetOwnerWindow(HWND(hwnd as *mut _))?;
    let sources: Vec<String> = elements.iter().map(|e| e.source.clone()).collect();
    // Un `IShellItem` par source et par élément remplacé, créés avant `Advise` : le sink les compare à ceux que le moteur lui
    // rend (B1) et la boucle ci-dessous les réutilise
    let items: Vec<IShellItem> = elements.iter().map(|e| element(&e.source)).collect::<Result<_>>()?;
    let items_remplaces: Vec<Option<IShellItem>> = elements.iter().map(|e| e.remplace.as_deref().map(|r| element(r)).transpose()).collect::<Result<_>>()?;
    let sources_suivies: Vec<(String, IShellItem)> = sources.iter().cloned().zip(items.iter().cloned()).collect();
    let remplaces: Vec<(String, IShellItem)> = elements.iter().zip(&items_remplaces).filter_map(|(e, i)| Some((e.remplace.clone()?, i.clone()?))).collect();
    // Limite de vitesse (réglages) : seulement pour une copie ou un déplacement qui passe par le réseau
    let mo = crate::reglages::actuels().limite_reseau_mo as u64;
    let reseau = matches!(genre, Genre::Copier | Genre::Deplacer) && elements.iter().any(|e| sur_le_reseau(&e.source) || e.dest.as_deref().is_some_and(sur_le_reseau));
    let limite = if reseau { mo << 20 } else { 0 };
    let octets = if limite > 0 { taille_totale(&sources) } else { 0 };
    LIMITEE.store(limite > 0 && octets > 0, Ordering::Relaxed);
    let debut = std::time::Instant::now();
    let refuse = Arc::new(AtomicBool::new(false));
    let annule_dialogue = controle.annule.clone();
    let suivi: IFileOperationProgressSink = Suivi { fenetre: hwnd, numero, annule: controle.annule, refuse: refuse.clone(), pause: controle.pause, sources: sources_suivies, resultats, remplaces, mis_de_cote, limite, octets, debut, dernier_fait: AtomicU64::new(0), dernier_total: AtomicU64::new(0) }.into();
    let jeton = op.Advise(&suivi)?;
    // SAFETY (durée de vie) : COM compte les références ; `op` garde la boîte jusqu'à sa libération en fin de fonction
    let dialogue: IOperationsProgressDialog = Annulation { annule: annule_dialogue }.into();
    // Une amélioration, pas une condition : si le Shell refuse (E_NOTIMPL, stratégie de groupe), l'opération se fait
    // quand même, avec l'annulation entre deux éléments seulement (I2)
    if let Err(e) = op.SetProgressDialog(&dialogue) {
        crate::journal::ecrire(crate::journal::Niveau::Avert, "operations", &format!("SetProgressDialog refusé : {}", message_com(&e)));
    }

    // Dossiers de destination : un seul IShellItem par dossier
    let mut dossiers: Vec<(String, IShellItem)> = Vec::new();
    for (i, e) in elements.iter().enumerate() {
        let item = items[i].clone();
        if let Some(r) = &items_remplaces[i] {
            op.DeleteItem(r, None)?;
        }
        let dest = match &e.dest {
            Some(d) => match dossiers.iter().find(|(c, _)| c.eq_ignore_ascii_case(d)) {
                Some((_, i)) => Some(i.clone()),
                None => {
                    let i = element(d)?;
                    dossiers.push((d.clone(), i.clone()));
                    Some(i)
                }
            },
            None => None,
        };
        let nom = e.nom.as_deref().map(wide);
        let nom = nom.as_ref().map_or(PCWSTR::null(), |w| PCWSTR(w.as_ptr()));
        match (genre, &dest) {
            (Genre::Copier, Some(d)) => op.CopyItem(&item, d, nom, None)?,
            (Genre::Deplacer, Some(d)) => op.MoveItem(&item, d, nom, None)?,
            _ => op.DeleteItem(&item, None)?,
        }
    }
    let r = op.PerformOperations();
    let _ = op.Unadvise(jeton);
    // Annulé = Windows l'a signalé, ou un de nos rappels a refusé un élément à cause de l'annulation (D1)
    let interrompu = op.GetAnyOperationsAborted().map(|b| b.as_bool()).unwrap_or(false) || refuse.load(Ordering::Relaxed);
    match r {
        Ok(()) => Ok(interrompu),
        // Seules ces erreurs veulent dire « annulé » (COPYENGINE_E_USER_CANCELLED = 0x80270000, rendu par le moteur quand
        // notre boîte de progression répond PDOPS_CANCELLED au milieu d'un fichier) : une vraie erreur (disque plein, accès
        // refusé) n'est pas masquée parce que l'utilisateur a aussi annulé (M2)
        Err(e) if e.code() == E_ABORT || e.code() == ERROR_CANCELLED.to_hresult() || e.code().0 as u32 == 0x8027_0000 => Ok(true),
        Err(e) => Err(e),
    }
}

/// Commandes d'une opération en cours, partagées avec son fil
#[derive(Clone, Default)]
pub struct Controle {
    pub annule: Arc<AtomicBool>,
    pub pause: Arc<AtomicBool>,
}

/// Lance une copie, un déplacement ou une suppression dans un fil séparé.
/// `vides` : dossiers sources fusionnés dans un dossier existant, retirés après un déplacement s'ils sont vides.
pub fn lancer(hwnd: HWND, numero: u64, genre: Genre, elements: Vec<Element>, vides: Vec<String>, controle: Controle) {
    let fenetre = hwnd.0 as isize;
    std::thread::spawn(move || unsafe {
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE);
        let resultats = Arc::new(Mutex::new(Vec::new()));
        let mis_de_cote = Arc::new(Mutex::new(Vec::new()));
        let mut restants = elements.clone();
        let mut essais = 0;
        let r = loop {
            COUPURE.with(|c| c.set(false));
            let r = executer(fenetre, numero, genre, &restants, controle.clone(), resultats.clone(), mis_de_cote.clone(), essais > 0);
            let coupe = matches!(&r, Err(e) if reseau(e.code())) || COUPURE.with(|c| c.get());
            if !coupe || genre == Genre::SupprimerDefinitivement || essais >= 20 || controle.annule.load(Ordering::Relaxed) {
                break r;
            }
            // Coupure réseau (NAS, partage) : ce qui est fait ne se refait pas, le reste attend le retour du réseau
            let faits: std::collections::HashSet<String> = resultats.lock().unwrap_or_else(|e| e.into_inner()).iter().map(|(a, _)| a.trim_end_matches('\\').to_lowercase()).collect();
            restants.retain(|e| !faits.contains(&e.source.trim_end_matches('\\').to_lowercase()));
            if restants.is_empty() {
                break Ok(false);
            }
            for e in &mut restants {
                if e.remplace.as_deref().is_some_and(|p| !std::path::Path::new(p).exists()) {
                    e.remplace = None;
                }
            }
            ATTENTES.lock().unwrap_or_else(|e| e.into_inner()).push(numero);
            let _ = windows::Win32::Graphics::Gdi::InvalidateRect(HWND(fenetre as *mut _), None, false);
            let joignable = |e: &Element| std::path::Path::new(&e.source).exists() && e.dest.as_deref().map_or(true, |d| std::path::Path::new(d).exists());
            // Jusqu'à 10 minutes, un essai toutes les 3 secondes
            let mut revenu = false;
            for _ in 0..200 {
                if controle.annule.load(Ordering::Relaxed) {
                    break;
                }
                std::thread::sleep(std::time::Duration::from_secs(3));
                if joignable(&restants[0]) {
                    revenu = true;
                    break;
                }
            }
            ATTENTES.lock().unwrap_or_else(|e| e.into_inner()).retain(|&n| n != numero);
            let _ = windows::Win32::Graphics::Gdi::InvalidateRect(HWND(fenetre as *mut _), None, false);
            if !revenu {
                break if controle.annule.load(Ordering::Relaxed) { Ok(true) } else { r };
            }
            essais += 1;
        };
        if genre == Genre::Deplacer && matches!(r, Ok(false)) {
            // Les plus profonds d'abord ; un dossier qui n'est pas vide (élément ignoré) reste
            let mut vides = vides;
            vides.sort_by_key(|v| std::cmp::Reverse(v.len()));
            for v in vides {
                let _ = std::fs::remove_dir(v);
            }
        }
        // État final pour SpaceNotch (`progress`), jamais bloquant ; les chiffres sont ceux de la dernière mise à jour
        let etat_final = match &r {
            Ok(false) => crate::eco::progression::Etat::Done,
            Ok(true) => crate::eco::progression::Etat::Cancelled,
            Err(_) => crate::eco::progression::Etat::Error,
        };
        crate::eco::progression::annoncer(numero, etat_final, 0, 0);
        let resultats = std::mem::take(&mut *resultats.lock().unwrap_or_else(|e| e.into_inner()));
        let remplaces = std::mem::take(&mut *mis_de_cote.lock().unwrap_or_else(|e| e.into_inner()));
        let bilan = Box::new(Bilan {
            numero,
            resultats,
            remplaces,
            genre,
            nombre: elements.len(),
            annule: matches!(r, Ok(true)),
            erreur: r.err().map(|e| message_com(&e)),
        });
        CoUninitialize();
        let p = Box::into_raw(bilan);
        if PostMessageW(HWND(fenetre as *mut _), WM_OPERATION_FINIE, WPARAM(0), LPARAM(p as isize)).is_err() {
            drop(Box::from_raw(p));
        }
    });
}

// ───────────────────────── Renommer, nouveau dossier ─────────────────────────

/// Caractères interdits par Windows dans un nom
pub fn nom_valide(nom: &str) -> bool {
    let n = nom.trim();
    // Point ou espace final : Windows les retire d'un chemin court mais pas d'un chemin préfixé `\\?\` (plus de 260 caractères)
    if nom.ends_with('.') || nom.ends_with(' ') {
        return false;
    }
    !n.is_empty() && n != "." && n != ".." && !n.chars().any(|c| "\\/:*?\"<>|".contains(c) || (c as u32) < 32) && !nom_reserve(n)
}

/// Noms que Windows réserve à ses périphériques (CON, PRN, AUX, NUL, COM0 à COM9, LPT0 à LPT9) : seuls ou suivis d'une extension
/// (`con.txt`), sans tenir compte de la casse, ils ne se créent pas ou ouvrent le périphérique. Les chiffres en exposant (COM¹) aussi.
pub fn nom_reserve(nom: &str) -> bool {
    // Le radical est ce qui précède le premier point ; Windows ignore aussi les espaces qui le terminent (« con .txt »)
    let radical = nom.split('.').next().unwrap_or("").trim_end().to_uppercase();
    match radical.as_str() {
        "CON" | "PRN" | "AUX" | "NUL" | "CONIN$" | "CONOUT$" => true,
        r => {
            let mut c = r.chars();
            let prefixe: String = c.by_ref().take(3).collect();
            let reste: Vec<char> = c.collect();
            (prefixe == "COM" || prefixe == "LPT") && reste.len() == 1 && (reste[0].is_ascii_digit() || matches!(reste[0], '¹' | '²' | '³'))
        }
    }
}

/// Déplace sans jamais remplacer : `fs::rename` écrase un fichier qui apparaît entre la vérification et
/// l'appel, `MoveFileW` échoue à la place (R18)
pub fn deplacer_sans_ecraser(de: &str, vers: &str) -> std::io::Result<()> {
    // `MoveFileW` refuse au-delà de 260 caractères sans préfixe long (D5) ; en dessous, le chemin reste tel quel
    // (le préfixe supprime la normalisation de Windows : barres, points finaux)
    let long = |p: &str| if p.len() >= 248 { avec_prefixe_long(p) } else { p.to_string() };
    // SAFETY: les deux `HSTRING` (terminés par un nul) sont des temporaires qui vivent jusqu'à la fin de l'instruction,
    // donc pendant tout l'appel à `MoveFileW`
    unsafe {
        windows::Win32::Storage::FileSystem::MoveFileW(&windows::core::HSTRING::from(long(de)), &windows::core::HSTRING::from(long(vers)))
            .map_err(|e| std::io::Error::from_raw_os_error((e.code().0 & 0xFFFF) as i32))
    }
}

/// Renommer ne passe pas par `IFileOperation` (R6, assumé) : un renommage est atomique, ne pose aucune
/// question, et l'annulation est tenue par l'historique de l'application
pub fn renommer(dossier: &str, ancien: &str, nouveau: &str) -> std::result::Result<(), String> {
    let base = dossier.trim_end_matches('\\');
    let (de, vers) = (format!("{base}\\{ancien}"), format!("{base}\\{nouveau}"));
    deplacer_sans_ecraser(&de, &vers).map_err(|e| message_io(&e))?;
    // L'étiquette suit le nouveau nom (renommage, Ctrl+Z et palette passent tous par ici)
    crate::etiquettes::suivre(&de, &vers);
    Ok(())
}

/// Crée « Nouveau dossier », ou « Nouveau dossier (2) »… s'il existe déjà. Renvoie le nom choisi.
pub fn nouveau_dossier(dossier: &str) -> std::result::Result<String, String> {
    let base = dossier.trim_end_matches('\\');
    for n in 1..1000 {
        let nom = if n == 1 { "Nouveau dossier".to_string() } else { format!("Nouveau dossier ({n})") };
        match std::fs::create_dir(format!("{base}\\{nom}")) {
            Ok(()) => return Ok(nom),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(message_io(&e)),
        }
    }
    Err("TROP DE DOSSIERS NOUVEAUX".into())
}

/// « Nouveau document texte.txt » (ou (2), (3)…) dans ce dossier ; renvoie le nom créé
pub fn nouveau_fichier(dossier: &str) -> std::result::Result<String, String> {
    let base = dossier.trim_end_matches('\\');
    for n in 1..1000 {
        let nom = if n == 1 { "Nouveau document texte.txt".to_string() } else { format!("Nouveau document texte ({n}).txt") };
        match std::fs::OpenOptions::new().write(true).create_new(true).open(format!("{base}\\{nom}")) {
            Ok(_) => return Ok(nom),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(message_io(&e)),
        }
    }
    Err("TROP DE FICHIERS NOUVEAUX".into())
}

/// Erreur Windows (code Win32) dite simplement, avec ce qu'on peut y faire
pub fn message_code(code: u32) -> Option<&'static str> {
    Some(match code {
        5 => "ACCES REFUSE · CTRL+MAJ+A : ROUVRIR CE DOSSIER EN ADMINISTRATEUR",
        32 | 33 => "FICHIER OUVERT DANS UNE AUTRE APPLICATION · FERMEZ-LA PUIS REESSAYEZ",
        39 | 112 => "DISQUE PLEIN · LIBEREZ DE LA PLACE PUIS REESSAYEZ",
        2 | 3 => "ELEMENT INTROUVABLE · IL A PEUT-ETRE ETE DEPLACE (ACTUALISEZ LE DOSSIER)",
        15 | 21 => "LECTEUR INDISPONIBLE · CLE OU DISQUE RETIRE ?",
        19 => "DISQUE PROTEGE EN ECRITURE",
        80 | 183 => "CE NOM EXISTE DEJA",
        123 => "NOM INVALIDE · CARACTERES INTERDITS : \\ / : * ? \" < > |",
        145 => "LE DOSSIER N'EST PAS VIDE",
        206 => "NOM OU CHEMIN TROP LONG",
        53 | 67 | 1231 => "SERVEUR INJOIGNABLE · VERIFIEZ LE RESEAU",
        1326 => "IDENTIFIANTS REFUSES PAR LE SERVEUR",
        1223 => "ANNULE",
        _ => return None,
    })
}

/// Erreur COM (opérations du Shell) dite simplement
pub fn message_com(e: &windows::core::Error) -> String {
    let h = e.code().0 as u32;
    let code = if h & 0xFFFF_0000 == 0x8007_0000 { h & 0xFFFF } else { h };
    message_code(code).map(str::to_string).unwrap_or_else(|| e.message().to_string().trim().to_uppercase())
}

pub fn message_io(e: &std::io::Error) -> String {
    if let Some(m) = e.raw_os_error().and_then(|c| message_code(c as u32)) {
        return m.into();
    }
    match e.kind() {
        std::io::ErrorKind::PermissionDenied => message_code(5).unwrap_or("ACCES REFUSE").into(),
        std::io::ErrorKind::AlreadyExists => "CE NOM EXISTE DEJA".into(),
        std::io::ErrorKind::NotFound => message_code(2).unwrap_or("ELEMENT INTROUVABLE").into(),
        _ => e.to_string().to_uppercase(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nom_valide_refuse_un_point_ou_un_espace_final() {
        // Sous `\\?\` Windows ne retire plus ces caractères : le comportement dépendrait de la longueur du chemin (M6)
        assert!(!nom_valide("fichier."));
        assert!(!nom_valide("fichier "));
        assert!(!nom_valide("a.txt."));
        assert!(nom_valide("fichier.txt"));
        assert!(nom_valide(".gitignore"));
    }

    #[test]
    fn nom_valide_refuse_les_noms_reserves_de_windows() {
        for n in ["CON", "con", "Prn", "AUX", "nul", "COM1", "com9", "COM0", "LPT1", "lpt0", "LPT9", "con.txt", "NUL.tar.gz", "Aux.log", "COM3.txt", "COM¹"] {
            assert!(!nom_valide(n), "{n} est réservé");
        }
        // Un nom qui contient seulement ces lettres, ou un numéro de port hors 0 à 9, reste permis
        for n in ["console", "confiture.txt", "COM10", "LPT", "COM", "auxiliaire", "nulle.txt", "a.con", "my.con.txt", "rapport.pdf"] {
            assert!(nom_valide(n), "{n} est un nom permis");
        }
    }

    #[test]
    fn prefixe_long_laisse_un_chemin_de_peripherique_tel_quel() {
        assert_eq!(avec_prefixe_long(r"\\.\PHYSICALDRIVE0"), r"\\.\PHYSICALDRIVE0");
        assert_eq!(avec_prefixe_long(r"\\.\C:"), r"\\.\C:");
    }

    #[test]
    fn prefixe_long_ajoute_seulement_ou_il_manque() {
        assert_eq!(avec_prefixe_long(r"C:\a\b.txt"), r"\\?\C:\a\b.txt");
        assert_eq!(avec_prefixe_long(r"\\serveur\partage\b.txt"), r"\\?\UNC\serveur\partage\b.txt");
        assert_eq!(avec_prefixe_long(r"\\?\C:\a\b.txt"), r"\\?\C:\a\b.txt");
        assert_eq!(avec_prefixe_long("relatif.txt"), "relatif.txt");
    }

    #[test]
    fn deplacer_sans_ecraser_refuse_le_remplacement() {
        let d = std::env::temp_dir().join(format!("ne-deplacer-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        let (a, b) = (d.join("a.txt"), d.join("b.txt"));
        std::fs::write(&a, "a").unwrap();
        std::fs::write(&b, "b").unwrap();
        let e = deplacer_sans_ecraser(a.to_str().unwrap(), b.to_str().unwrap()).unwrap_err();
        assert_eq!(e.kind(), std::io::ErrorKind::AlreadyExists);
        assert_eq!(std::fs::read_to_string(&b).unwrap(), "b");
        // Changement de casse seul : permis
        deplacer_sans_ecraser(a.to_str().unwrap(), d.join("A.txt").to_str().unwrap()).unwrap();
        let _ = std::fs::remove_dir_all(&d);
    }
}

#[cfg(test)]
mod matrice_tests;
