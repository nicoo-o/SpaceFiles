//! Session de terminal (PLAN-TERMINAL, lot T2a) : un vrai shell (cmd, PowerShell) lancé dans une pseudo-console ConPTY,
//! avec un fil de lecture, un fil d'écriture et une fermeture qui ne laisse aucun processus derrière elle.
//!
//! Choix qui comptent (le reste se lit dans le code) :
//! - **Objet Job Windows** (`KILL_ON_JOB_CLOSE`) : le shell et tous ses descendants (ping, vim, un second powershell...)
//!   meurent avec la session, même si SpaceFiles est tué sans `Drop` (l'app est en `panic = "abort"`). `ClosePseudoConsole`
//!   seul ne garantit pas la mort d'un petit-fils qui ignore la fermeture.
//! - **Aucun verrou tenu pendant le dessin** : le fil de lecture ne fait que poser les octets dans un tampon et prévenir
//!   la fenêtre (`notifier`, qui poste un message Win32) ; la fenêtre vide ce tampon quand elle veut.
//! - **Tampon borné** (`LIMITE_TAMPON`) : si la fenêtre ne vide pas (cachée, occupée), le fil de lecture attend, le
//!   shell se bloque sur son écriture, la mémoire ne grossit pas (`yes` ne doit pas manger la RAM du résident).
//! - **Écriture dans un fil** : coller un gros texte ne bloque pas l'interface si le shell ne lit pas.
//!
//! Aucune session n'existe tant que le panneau n'a pas été ouvert : le résident ne paie rien.
//! `unsafe` : chaque bloc dit son invariant (durée de vie d'un handle, taille d'un tampon).

#![allow(dead_code)]

use std::ffi::c_void;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Sender};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

use windows::core::{PCWSTR, PWSTR};
use windows::Win32::Foundation::{CloseHandle, HANDLE, WAIT_OBJECT_0};
use windows::Win32::Storage::FileSystem::{ReadFile, WriteFile};
use windows::Win32::System::Console::{ClosePseudoConsole, CreatePseudoConsole, ResizePseudoConsole, COORD, HPCON};
use windows::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation, SetInformationJobObject, TerminateJobObject,
    JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
};
use windows::Win32::System::Pipes::CreatePipe;
use windows::Win32::System::Threading::{
    CreateProcessW, DeleteProcThreadAttributeList, GetExitCodeProcess, InitializeProcThreadAttributeList, ResumeThread, TerminateProcess,
    UpdateProcThreadAttribute, WaitForSingleObject, CREATE_SUSPENDED, EXTENDED_STARTUPINFO_PRESENT, INFINITE, LPPROC_THREAD_ATTRIBUTE_LIST, PROCESS_INFORMATION,
    STARTF_USESTDHANDLES, STARTUPINFOEXW,
};

/// Attribut qui rattache le processus fils à la pseudo-console (PROC_THREAD_ATTRIBUTE_PSEUDOCONSOLE de `winbase.h`)
const ATTRIBUT_PSEUDOCONSOLE: usize = 0x0002_0016;
/// Octets en attente d'être lus par la fenêtre au-delà desquels le fil de lecture s'arrête de lire (contre-pression)
pub const LIMITE_TAMPON: usize = 4 * 1024 * 1024;
/// Dimensions minimales et maximales demandées à la pseudo-console
const COLONNES_MAX: u16 = 1_000;
const LIGNES_MAX: u16 = 500;

/// Le shell que l'on lance dans une session
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Shell {
    PowerShell,
    Cmd,
}

impl Shell {
    /// Ligne de commande complète, avec le chemin absolu du dossier système (jamais une recherche dans le PATH ou le
    /// dossier courant : un `cmd.exe` planté dans un dossier ne doit pas être lancé à la place du vrai).
    pub fn ligne_de_commande(self) -> String {
        let racine = std::env::var("SystemRoot").unwrap_or_else(|_| "C:\\Windows".to_string());
        match self {
            Shell::Cmd => format!("\"{racine}\\System32\\cmd.exe\""),
            Shell::PowerShell => format!("\"{racine}\\System32\\WindowsPowerShell\\v1.0\\powershell.exe\" -NoLogo -NoExit -Command \"{INVITE_POWERSHELL}\""),
        }
    }
}

/// Tests seulement : le test de fuite de handles compte ceux de tout le processus, un test qui garde un shell ouvert
/// pendant ce temps le fausse. Les deux prennent ce verrou.
#[cfg(test)]
pub(crate) static VERROU_HANDLES: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// T4b : enveloppe l'invite de PowerShell (celle du profil de l'utilisateur, gardée telle quelle et appelée en premier)
/// pour qu'elle annonce le dossier courant au terminal (OSC 9;9, `ESC ] 9 ; 9 ; chemin ESC \`) : c'est ainsi que la liste
/// suit un `cd` tapé ici. Hors du fournisseur de fichiers (registre, certificats), rien n'est annoncé. Sans guillemets
/// doubles : le script voyage dans la ligne de commande du processus.
const INVITE_POWERSHELL: &str = "$global:__sf=$function:prompt; function global:prompt{$r=if($global:__sf){& $global:__sf}else{'PS> '}; $e=[char]27; if($PWD.Provider.Name -eq 'FileSystem'){[string]$r+$e+']9;9;'+$PWD.ProviderPath+$e+'\\'}else{[string]$r}}";

/// Ce que partagent le fil de lecture et la fenêtre
struct Partage {
    /// Octets lus et pas encore pris par la fenêtre
    sortie: Mutex<Vec<u8>>,
    /// Taille du tampon au-delà de laquelle le fil de lecture attend (contre-pression)
    limite: usize,
    /// Une notification est déjà en vol : inutile d'en poster une autre avant que la fenêtre ait vidé le tampon
    notifie: AtomicBool,
    /// Le tuyau de sortie est fermé : plus rien ne viendra (le shell est mort ou la session se ferme)
    lecture_finie: AtomicBool,
    /// La session se ferme : le fil de lecture jette ce qu'il lit au lieu d'attendre qu'on le vide
    arret: AtomicBool,
}

/// Une pseudo-console et le shell qui y tourne. `Drop` ferme tout.
pub struct Session {
    pc: HPCON,
    job: HANDLE,
    processus: HANDLE,
    pid: u32,
    entree: Option<Sender<Vec<u8>>>,
    partage: Arc<Partage>,
    lecteur: Option<JoinHandle<()>>,
    ecrivain: Option<JoinHandle<()>>,
    guetteur: Option<JoinHandle<()>>,
    colonnes: u16,
    lignes: u16,
}

fn large(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

fn borner(colonnes: u16, lignes: u16) -> (u16, u16) {
    (colonnes.clamp(1, COLONNES_MAX), lignes.clamp(1, LIGNES_MAX))
}

impl Session {
    /// Lance `ligne_de_commande` dans une pseudo-console de `colonnes × lignes`, dans `dossier` (sinon le dossier courant du
    /// processus). `notifier` est appelé depuis le fil de lecture quand des octets (ou la fin du flux) attendent : il doit
    /// seulement prévenir la fenêtre (PostMessageW), jamais toucher à l'interface. Erreur = texte lisible pour la barre d'état.
    pub fn lancer(ligne_de_commande: &str, dossier: Option<&str>, colonnes: u16, lignes: u16, notifier: Arc<dyn Fn() + Send + Sync>) -> Result<Session, String> {
        Self::lancer_avec_limite(ligne_de_commande, dossier, colonnes, lignes, notifier, LIMITE_TAMPON)
    }

    /// Comme `lancer`, avec une limite de tampon choisie (les tests la réduisent pour prouver la contre-pression sans
    /// produire des mégaoctets de sortie).
    pub(crate) fn lancer_avec_limite(ligne_de_commande: &str, dossier: Option<&str>, colonnes: u16, lignes: u16, notifier: Arc<dyn Fn() + Send + Sync>, limite: usize) -> Result<Session, String> {
        let (colonnes, lignes) = borner(colonnes, lignes);
        // SAFETY : tous les handles créés ici sont soit fermés sur chaque chemin d'erreur, soit confiés à la `Session` (ou à
        // l'un de ses deux fils) qui les ferme ; les pointeurs passés aux API pointent vers des variables qui vivent
        // jusqu'à la fin de l'appel.
        unsafe {
            let (mut entree_lue, mut entree_ecrite, mut sortie_lue, mut sortie_ecrite) = (HANDLE::default(), HANDLE::default(), HANDLE::default(), HANDLE::default());
            CreatePipe(&mut entree_lue, &mut entree_ecrite, None, 0).map_err(|e| format!("tuyau d'entrée : {e}"))?;
            if let Err(e) = CreatePipe(&mut sortie_lue, &mut sortie_ecrite, None, 0) {
                let _ = CloseHandle(entree_lue);
                let _ = CloseHandle(entree_ecrite);
                return Err(format!("tuyau de sortie : {e}"));
            }
            let fermer_tuyaux = |hs: &[HANDLE]| {
                for h in hs {
                    let _ = CloseHandle(*h);
                }
            };
            let pc = match CreatePseudoConsole(COORD { X: colonnes as i16, Y: lignes as i16 }, entree_lue, sortie_ecrite, 0) {
                Ok(p) => p,
                Err(e) => {
                    fermer_tuyaux(&[entree_lue, entree_ecrite, sortie_lue, sortie_ecrite]);
                    return Err(format!("pseudo-console refusée : {e}"));
                }
            };
            // Les deux côtés confiés à la pseudo-console ne servent plus ici
            let _ = CloseHandle(entree_lue);
            let _ = CloseHandle(sortie_ecrite);

            let job = match creer_job() {
                Ok(j) => j,
                Err(e) => {
                    ClosePseudoConsole(pc);
                    fermer_tuyaux(&[entree_ecrite, sortie_lue]);
                    return Err(e);
                }
            };

            let pi = match creer_processus(pc, ligne_de_commande, dossier) {
                Ok(p) => p,
                Err(e) => {
                    let _ = CloseHandle(job);
                    ClosePseudoConsole(pc);
                    fermer_tuyaux(&[entree_ecrite, sortie_lue]);
                    return Err(e);
                }
            };
            // Le processus démarre suspendu : il rejoint le job AVANT d'exécuter la moindre instruction, donc aucun
            // descendant ne peut lui échapper
            if let Err(e) = AssignProcessToJobObject(job, pi.hProcess) {
                let _ = TerminateProcess(pi.hProcess, 1);
                let _ = CloseHandle(pi.hThread);
                let _ = CloseHandle(pi.hProcess);
                let _ = CloseHandle(job);
                ClosePseudoConsole(pc);
                fermer_tuyaux(&[entree_ecrite, sortie_lue]);
                return Err(format!("rattachement du shell au job : {e}"));
            }
            ResumeThread(pi.hThread);
            let _ = CloseHandle(pi.hThread);

            let partage = Arc::new(Partage { sortie: Mutex::new(Vec::new()), limite, notifie: AtomicBool::new(false), lecture_finie: AtomicBool::new(false), arret: AtomicBool::new(false) });

            // Constat de T2a : ConPTY ne ferme pas le flux de sortie quand le shell se termine ; sans ce guetteur, un `exit` sans
            // dernière sortie ne préviendrait jamais la fenêtre. Un échec de création du fil n'est pas fatal (repli : on voit
            // la fin au prochain octet reçu).
            let guetteur = {
                let partage = partage.clone();
                let notifier = notifier.clone();
                let processus = pi.hProcess.0 as isize;
                std::thread::Builder::new().name("terminal-guet".into()).spawn(move || fil_guet(processus, partage, notifier)).ok()
            };
            let lecteur = {
                let partage = partage.clone();
                let lecture = sortie_lue.0 as isize;
                std::thread::Builder::new().name("terminal-lecture".into()).spawn(move || fil_lecture(lecture, partage, notifier))
            };
            let (tx, rx) = channel::<Vec<u8>>();
            let ecrivain = {
                let ecriture = entree_ecrite.0 as isize;
                std::thread::Builder::new().name("terminal-ecriture".into()).spawn(move || fil_ecriture(ecriture, rx))
            };
            let (lecteur, ecrivain) = match (lecteur, ecrivain) {
                (Ok(l), Ok(e)) => (l, e),
                (l, e) => {
                    // Création d'un fil impossible (mémoire) : on défait tout ; les fils déjà partis s'arrêtent à la fermeture
                    let _ = TerminateJobObject(job, 1);
                    ClosePseudoConsole(pc);
                    drop(tx);
                    partage.arret.store(true, Ordering::SeqCst);
                    if let Some(g) = guetteur {
                        let _ = g.join();
                    }
                    if let Ok(l) = l {
                        let _ = l.join();
                    } else {
                        let _ = CloseHandle(sortie_lue);
                    }
                    if let Ok(e) = e {
                        let _ = e.join();
                    } else {
                        let _ = CloseHandle(entree_ecrite);
                    }
                    let _ = CloseHandle(pi.hProcess);
                    let _ = CloseHandle(job);
                    return Err("impossible de créer les fils du terminal".to_string());
                }
            };

            Ok(Session { pc, job, processus: pi.hProcess, pid: pi.dwProcessId, entree: Some(tx), partage, lecteur: Some(lecteur), ecrivain: Some(ecrivain), guetteur, colonnes, lignes })
        }
    }

    /// Identifiant du shell (pour les tests et le suivi de processus)
    pub fn pid(&self) -> u32 {
        self.pid
    }

    pub fn colonnes(&self) -> u16 {
        self.colonnes
    }

    pub fn lignes(&self) -> u16 {
        self.lignes
    }

    /// Envoie des octets au clavier du shell (UTF-8 ou séquences de touches VT). Ne bloque jamais : le fil d'écriture les
    /// remet au shell à son rythme.
    pub fn ecrire(&self, octets: &[u8]) {
        if octets.is_empty() {
            return;
        }
        if let Some(tx) = &self.entree {
            let _ = tx.send(octets.to_vec());
        }
    }

    /// Change la taille de la pseudo-console (le shell reçoit le signal de redimensionnement et redessine). Sans effet si la
    /// taille ne change pas.
    pub fn redimensionner(&mut self, colonnes: u16, lignes: u16) {
        let (colonnes, lignes) = borner(colonnes, lignes);
        if colonnes == self.colonnes && lignes == self.lignes {
            return;
        }
        // SAFETY : `pc` est valide tant que la session existe (fermé seulement dans `Drop`)
        let ok = unsafe { ResizePseudoConsole(self.pc, COORD { X: colonnes as i16, Y: lignes as i16 }) }.is_ok();
        if ok {
            self.colonnes = colonnes;
            self.lignes = lignes;
        }
    }

    /// Vide le tampon de sortie : tous les octets lus depuis le dernier appel. À appeler quand la fenêtre reçoit la
    /// notification. Le drapeau « notification en vol » retombe AVANT la lecture, pour qu'un octet arrivé juste après
    /// déclenche une nouvelle notification au lieu d'être oublié.
    pub fn prendre_sortie(&self) -> Vec<u8> {
        self.partage.notifie.store(false, Ordering::SeqCst);
        let mut s = self.partage.sortie.lock().unwrap_or_else(|e| e.into_inner());
        std::mem::take(&mut *s)
    }

    /// Le tuyau de sortie est fermé et vidé côté fil : plus aucun octet ne viendra. Il peut rester du texte à prendre.
    pub fn lecture_finie(&self) -> bool {
        self.partage.lecture_finie.load(Ordering::SeqCst)
    }

    /// Code de sortie du shell s'il est terminé, `None` s'il tourne encore
    pub fn code_de_sortie(&self) -> Option<u32> {
        let mut code = 0u32;
        // SAFETY : `processus` est un handle valide jusqu'à `Drop`
        unsafe {
            // Le code 259 est aussi une vraie valeur de sortie (`exit 259`) : l'état du handle tranche, pas le code
            if WaitForSingleObject(self.processus, 0) != WAIT_OBJECT_0 {
                return None;
            }
            GetExitCodeProcess(self.processus, &mut code).is_ok().then_some(code)
        }
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        // 1. Le fil de lecture jette désormais ce qu'il lit (il ne peut plus rester bloqué sur la contre-pression)
        self.partage.arret.store(true, Ordering::SeqCst);
        // 2. Le fil d'écriture se termine quand l'émetteur disparaît
        self.entree.take();
        // SAFETY : handles valides, fermés une seule fois ici ; les deux fils ferment leurs propres extrémités de tuyau
        unsafe {
            // 3. Le job tue le shell et tous ses descendants sur-le-champ (même un petit-fils qui ignorerait la fermeture)
            let _ = TerminateJobObject(self.job, 1);
            // 4. La pseudo-console se ferme : les tuyaux se rompent, ce qui débloque les deux fils
            ClosePseudoConsole(self.pc);
        }
        if let Some(l) = self.lecteur.take() {
            let _ = l.join();
        }
        if let Some(e) = self.ecrivain.take() {
            let _ = e.join();
        }
        // Le job a tué le shell : le guetteur se réveille et s'arrête ; il doit finir avant la fermeture du handle
        if let Some(g) = self.guetteur.take() {
            let _ = g.join();
        }
        // SAFETY : les trois fils sont joints ; plus personne n'utilise ces handles, fermés une seule fois ici
        unsafe {
            let _ = CloseHandle(self.processus);
            let _ = CloseHandle(self.job);
        }
    }
}

/// Objet Job dont la fermeture tue tous les processus rattachés
unsafe fn creer_job() -> Result<HANDLE, String> {
    let job = CreateJobObjectW(None, PCWSTR::null()).map_err(|e| format!("objet job : {e}"))?;
    let mut info = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
    info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
    if let Err(e) = SetInformationJobObject(job, JobObjectExtendedLimitInformation, &info as *const _ as *const c_void, std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32) {
        let _ = CloseHandle(job);
        return Err(format!("réglage du job : {e}"));
    }
    Ok(job)
}

/// Crée le shell, suspendu, rattaché à la pseudo-console. L'appelant le rattache au job puis le reprend.
unsafe fn creer_processus(pc: HPCON, ligne_de_commande: &str, dossier: Option<&str>) -> Result<PROCESS_INFORMATION, String> {
    let mut taille = 0usize;
    // Premier appel : demande la taille de la liste d'attributs (il « échoue » par construction)
    let _ = InitializeProcThreadAttributeList(LPPROC_THREAD_ATTRIBUTE_LIST::default(), 1, 0, &mut taille);
    let mut tampon = vec![0u8; taille];
    let liste = LPPROC_THREAD_ATTRIBUTE_LIST(tampon.as_mut_ptr() as *mut c_void);
    InitializeProcThreadAttributeList(liste, 1, 0, &mut taille).map_err(|e| format!("liste d'attributs : {e}"))?;
    if let Err(e) = UpdateProcThreadAttribute(liste, 0, ATTRIBUT_PSEUDOCONSOLE, Some(pc.0 as *const c_void), std::mem::size_of::<HPCON>(), None, None) {
        DeleteProcThreadAttributeList(liste);
        return Err(format!("attribut pseudo-console : {e}"));
    }
    let mut si = STARTUPINFOEXW::default();
    si.StartupInfo.cb = std::mem::size_of::<STARTUPINFOEXW>() as u32;
    si.lpAttributeList = liste;
    // Constat de T0 : sans ceci, un fils lancé depuis un processus qui a une console (cargo test) écrit dans la console du
    // PARENT et non dans la pseudo-console. Sans effet pour SpaceFiles (sous-système « windows »).
    si.StartupInfo.dwFlags = STARTF_USESTDHANDLES;
    let mut commande = large(ligne_de_commande);
    let dossier_large = dossier.map(large);
    let dossier_pcwstr = dossier_large.as_ref().map(|d| PCWSTR(d.as_ptr())).unwrap_or(PCWSTR::null());
    let mut pi = PROCESS_INFORMATION::default();
    let cree = CreateProcessW(None, PWSTR(commande.as_mut_ptr()), None, None, false, EXTENDED_STARTUPINFO_PRESENT | CREATE_SUSPENDED, None, dossier_pcwstr, &si.StartupInfo, &mut pi);
    DeleteProcThreadAttributeList(liste);
    cree.map_err(|e| format!("lancement de « {ligne_de_commande} » : {e}"))?;
    Ok(pi)
}

/// Fil de lecture : tuyau de sortie → tampon partagé → notification. Possède et ferme son handle.
fn fil_lecture(lecture: isize, partage: Arc<Partage>, notifier: Arc<dyn Fn() + Send + Sync>) {
    let h = HANDLE(lecture as *mut c_void);
    let mut bloc = [0u8; 16 * 1024];
    loop {
        let mut n = 0u32;
        // SAFETY : `h` reste valide (seul ce fil le ferme, après la boucle) ; `bloc` vit pendant l'appel
        let lu = unsafe { ReadFile(h, Some(&mut bloc), Some(&mut n), None) };
        if lu.is_err() || n == 0 {
            break;
        }
        if partage.arret.load(Ordering::SeqCst) {
            continue; // la session se ferme : on vide le tuyau sans rien garder
        }
        // Contre-pression : le tampon est plein, on laisse le shell se bloquer sur son écriture
        loop {
            let plein = partage.sortie.lock().unwrap_or_else(|e| e.into_inner()).len() >= partage.limite;
            if !plein || partage.arret.load(Ordering::SeqCst) {
                break;
            }
            std::thread::sleep(Duration::from_millis(2));
        }
        partage.sortie.lock().unwrap_or_else(|e| e.into_inner()).extend_from_slice(&bloc[..n as usize]);
        if !partage.notifie.swap(true, Ordering::SeqCst) {
            notifier();
        }
    }
    partage.lecture_finie.store(true, Ordering::SeqCst);
    notifier();
    // SAFETY : dernier usage du handle, plus aucune lecture en cours
    unsafe {
        let _ = CloseHandle(h);
    }
}

/// Fil guetteur : attend la fin du shell puis prévient la fenêtre (une seule fois, sauf si la session se ferme).
/// Ne possède pas le handle : la session le ferme après avoir joint ce fil.
fn fil_guet(processus: isize, partage: Arc<Partage>, notifier: Arc<dyn Fn() + Send + Sync>) {
    // SAFETY : le handle reste valide jusqu'à la jonction de ce fil dans `Drop`
    unsafe {
        WaitForSingleObject(HANDLE(processus as *mut c_void), INFINITE);
    }
    if !partage.arret.load(Ordering::SeqCst) {
        notifier();
    }
}

/// Fil d'écriture : file d'octets → tuyau d'entrée. Possède et ferme son handle ; s'arrête quand l'émetteur disparaît ou
/// que le tuyau est rompu.
fn fil_ecriture(ecriture: isize, rx: std::sync::mpsc::Receiver<Vec<u8>>) {
    let h = HANDLE(ecriture as *mut c_void);
    'boucle: while let Ok(octets) = rx.recv() {
        let mut reste = &octets[..];
        while !reste.is_empty() {
            let mut n = 0u32;
            // SAFETY : `h` reste valide jusqu'à la fin de ce fil ; `reste` vit pendant l'appel
            if unsafe { WriteFile(h, Some(reste), Some(&mut n), None) }.is_err() || n == 0 {
                break 'boucle;
            }
            reste = &reste[(n as usize).min(reste.len())..];
        }
    }
    // SAFETY : dernier usage du handle
    unsafe {
        let _ = CloseHandle(h);
    }
}

#[cfg(test)]
mod tests;
