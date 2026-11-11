// Outils pour développeurs (E18) et petits services (E16) : état Git, terminal et VS Code ici,
// chemins WSL / Unix, empreinte SHA-256, balises de couleur.

use std::collections::HashMap;
use std::os::windows::process::CommandExt;
use std::process::Command;
use std::sync::Mutex;

use windows::core::HSTRING;
use windows::Win32::UI::Shell::ShellExecuteW;
use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

/// Pas de fenêtre de console pour les programmes lancés en arrière-plan
pub const SANS_CONSOLE: u32 = 0x0800_0000;

// ───────────────────────── Git ─────────────────────────

/// État Git du dossier ouvert
#[derive(Clone, Debug, Default)]
pub struct Git {
    pub branche: String,
    /// Nom (en minuscules) d'un élément du dossier → 'M' modifié, 'A' ajouté, '?' nouveau (non suivi)
    pub etats: HashMap<String, char>,
}

/// Racine du dépôt Git qui contient ce dossier (un `.git` dans lui ou un parent)
pub fn depot(dossier: &str) -> Option<String> {
    let mut p = std::path::Path::new(dossier);
    loop {
        if p.join(".git").exists() {
            return Some(p.to_string_lossy().into_owned());
        }
        p = p.parent()?;
    }
}

fn git(dossier: &str, args: &[&str]) -> Option<String> {
    // --no-optional-locks : `git status` ne réécrit pas .git/index, sinon la surveillance du dossier
    // verrait un changement, relirait le dossier, relancerait git… sans fin
    let sortie = Command::new("git")
        .arg("--no-optional-locks")
        // core.fsmonitor est une commande : un dépôt téléchargé pourrait la fixer dans son .git/config
        .args(["-c", "core.fsmonitor=false"])
        .arg("-C")
        .arg(dossier)
        .args(args)
        .env("GIT_OPTIONAL_LOCKS", "0")
        .creation_flags(SANS_CONSOLE)
        .output()
        .ok()?;
    sortie.status.success().then(|| String::from_utf8_lossy(&sortie.stdout).into_owned())
}

/// Branche et fichiers modifiés (lancé hors du fil de l'interface : `git status` peut prendre un moment)
pub fn etat(dossier: &str) -> Option<Git> {
    let racine = depot(dossier)?;
    // Un filtre (`filter.x.clean`) du .git/config s'exécute pendant `status` : on ne lance pas git
    // dans un dépôt qui en déclare, comme un dépôt extrait d'un zip inconnu
    if let Ok(config) = std::fs::read_to_string(std::path::Path::new(&racine).join(".git").join("config")) {
        let c = config.to_lowercase();
        if c.contains("[filter") || c.contains("fsmonitor") {
            return None;
        }
    }
    let prefixe = git(dossier, &["rev-parse", "--show-prefix"])?.trim().to_lowercase();
    let status = git(dossier, &["status", "--porcelain=v1", "-z", "-b", "--untracked-files=normal"])?;
    let mut g = Git::default();
    let mut morceaux = status.split('\0');
    while let Some(m) = morceaux.next() {
        if let Some(b) = m.strip_prefix("## ") {
            // « main...origin/main [ahead 1] » ou « No commits yet on main »
            let b = b.strip_prefix("No commits yet on ").unwrap_or(b);
            g.branche = b.split("...").next().unwrap_or(b).split(' ').next().unwrap_or("").to_string();
            continue;
        }
        if m.len() < 4 {
            continue;
        }
        let (code, chemin) = m.split_at(3);
        // Renommage : le chemin d'origine suit dans l'enregistrement suivant
        if code.starts_with('R') || code.starts_with('C') {
            morceaux.next();
        }
        let chemin = chemin.to_lowercase();
        let Some(reste) = chemin.strip_prefix(&prefixe) else { continue };
        let nom = reste.split('/').next().unwrap_or("").to_string();
        if nom.is_empty() {
            continue;
        }
        let etat = match code.trim() {
            "??" => '?',
            c if c.contains('A') => 'A',
            _ => 'M',
        };
        // Un dossier qui contient à la fois du modifié et du nouveau : modifié l'emporte
        let e = g.etats.entry(nom).or_insert(etat);
        if etat == 'M' {
            *e = 'M';
        }
    }
    Some(g)
}

// ───────────────────────── Terminal, VS Code, chemins ─────────────────────────

/// `C:\` → `C:\.` : un `\` final devant un guillemet le transformerait en guillemet littéral
fn argument_dossier(d: &str) -> String {
    if d.ends_with('\\') {
        format!("{d}.")
    } else {
        d.to_string()
    }
}

/// Terminal Windows ouvert dans ce dossier (PowerShell s'il n'est pas installé)
pub fn terminal(dossier: &str) {
    unsafe {
        let r = ShellExecuteW(None, None, &HSTRING::from("wt.exe"), &HSTRING::from(format!("-d \"{}\"", argument_dossier(dossier))), None, SW_SHOWNORMAL);
        if r.0 as isize <= 32 {
            ShellExecuteW(None, None, &HSTRING::from("powershell.exe"), &HSTRING::from("-NoExit"), &HSTRING::from(dossier), SW_SHOWNORMAL);
        }
    }
}

/// VS Code ouvert sur ce dossier (ou ce fichier). Faux si `code` est introuvable.
pub fn vscode(chemin: &str) -> bool {
    // Guillemets forcés : cmd lirait `&`, `^` ou `(` d'un nom de dossier comme des commandes
    Command::new("cmd").args(["/c", "code"]).raw_arg(format!("\"{}\"", argument_dossier(chemin))).creation_flags(SANS_CONSOLE).spawn().is_ok()
}

/// `C:\Users\nico` → `/mnt/c/Users/nico` ; `\\zima\Media` → `//zima/Media`
pub fn chemin_wsl(c: &str) -> String {
    let b = c.trim_end_matches('\\');
    match b.as_bytes() {
        [l, b':', ..] => format!("/mnt/{}{}", (*l as char).to_ascii_lowercase(), b[2..].replace('\\', "/")),
        _ => b.replace('\\', "/"),
    }
}

/// `C:\Users\nico` → `C:/Users/nico`
pub fn chemin_unix(c: &str) -> String {
    c.replace('\\', "/")
}

// ───────────────────────── SHA-256 ─────────────────────────

const K: [u32; 64] = [
    0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5, 0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3,
    0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da,
    0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7, 0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967, 0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13,
    0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070,
    0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3, 0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208,
    0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
];

pub struct Sha256 {
    h: [u32; 8],
    tampon: Vec<u8>,
    longueur: u64,
}

impl Sha256 {
    pub fn new() -> Self {
        Self { h: [0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab, 0x5be0cd19], tampon: Vec::with_capacity(64), longueur: 0 }
    }

    fn bloc(&mut self, b: &[u8]) {
        let mut w = [0u32; 64];
        for i in 0..16 {
            w[i] = u32::from_be_bytes([b[4 * i], b[4 * i + 1], b[4 * i + 2], b[4 * i + 3]]);
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16].wrapping_add(s0).wrapping_add(w[i - 7]).wrapping_add(s1);
        }
        let [mut a, mut bb, mut c, mut d, mut e, mut f, mut g, mut h] = self.h;
        for i in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ (!e & g);
            let t1 = h.wrapping_add(s1).wrapping_add(ch).wrapping_add(K[i]).wrapping_add(w[i]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let maj = (a & bb) ^ (a & c) ^ (bb & c);
            let t2 = s0.wrapping_add(maj);
            h = g;
            g = f;
            f = e;
            e = d.wrapping_add(t1);
            d = c;
            c = bb;
            bb = a;
            a = t1.wrapping_add(t2);
        }
        for (x, y) in self.h.iter_mut().zip([a, bb, c, d, e, f, g, h]) {
            *x = x.wrapping_add(y);
        }
    }

    pub fn ajouter(&mut self, mut donnees: &[u8]) {
        self.longueur += donnees.len() as u64;
        if !self.tampon.is_empty() {
            let n = (64 - self.tampon.len()).min(donnees.len());
            self.tampon.extend_from_slice(&donnees[..n]);
            donnees = &donnees[n..];
            if self.tampon.len() == 64 {
                let t = std::mem::take(&mut self.tampon);
                self.bloc(&t);
                self.tampon = t;
                self.tampon.clear();
            }
        }
        while donnees.len() >= 64 {
            self.bloc(&donnees[..64]);
            donnees = &donnees[64..];
        }
        self.tampon.extend_from_slice(donnees);
    }

    pub fn finir(mut self) -> String {
        let bits = self.longueur * 8;
        let mut fin = std::mem::take(&mut self.tampon);
        fin.push(0x80);
        while fin.len() % 64 != 56 {
            fin.push(0);
        }
        fin.extend_from_slice(&bits.to_be_bytes());
        for b in fin.chunks(64) {
            self.bloc(b);
        }
        self.h.iter().map(|x| format!("{x:08x}")).collect()
    }
}

/// Origine « Internet » d'un fichier (V7) : le flux NTFS `Zone.Identifier` que posent navigateurs et clients de
/// messagerie. Renvoie la zone (3 : Internet, 4 : site non fiable) et le domaine d'où il vient, s'il est noté.
pub fn origine_internet(chemin: &str) -> Option<(u32, Option<String>)> {
    let texte = String::from_utf8_lossy(&std::fs::read(format!("{chemin}:Zone.Identifier")).ok()?).to_string();
    let valeur = |cle: &str| texte.lines().find_map(|l| l.trim().strip_prefix(cle).map(|v| v.trim().to_string()));
    let zone: u32 = valeur("ZoneId=")?.parse().ok()?;
    // Seul le domaine est gardé : l'adresse complète peut porter un jeton de téléchargement
    let hote = valeur("HostUrl=").and_then(|u| u.split("://").nth(1).and_then(|r| r.split(['/', '?']).next()).map(str::to_string)).filter(|h| !h.is_empty());
    (zone >= 3).then_some((zone, hote))
}

/// Signature numérique d'un programme (V7)
#[derive(Debug, PartialEq, Eq)]
pub enum Signature {
    /// Pas un type de programme : la question ne se pose pas
    SansObjet,
    Aucune,
    Invalide,
    /// Trop gros pour être vérifié sans bloquer la liste (la vérification relit tout le fichier)
    NonVerifie,
    Valide(String),
}

impl Signature {
    pub fn texte(&self) -> String {
        match self {
            Signature::SansObjet => String::new(),
            Signature::Aucune => "NON SIGNE".into(),
            Signature::Invalide => "SIGNATURE INVALIDE".into(),
            Signature::NonVerifie => "NON VERIFIE".into(),
            Signature::Valide(nom) => nom.to_uppercase(),
        }
    }
}

/// Lettre du lecteur d'un chemin (`E:\x` → `E`)
pub fn lettre_lecteur(chemin: &str) -> Option<char> {
    let mut c = chemin.chars();
    let l = c.next().filter(char::is_ascii_alphabetic)?;
    (c.next() == Some(':')).then(|| l.to_ascii_uppercase())
}

/// Programmes qui ont un fichier ouvert sous `racine`, d'après le Gestionnaire de redémarrage de Windows.
/// shortcut: les 5 000 premiers fichiers seulement (le Gestionnaire ne sait pas interroger un volume entier) ; un programme
/// qui n'a ouvert qu'un fichier plus loin n'est pas nommé, le message général reste alors affiché.
pub fn programmes_sur(racine: &str) -> Vec<String> {
    use windows::core::{PCWSTR, PWSTR};
    use windows::Win32::System::RestartManager::*;
    const MAX_FICHIERS: usize = 5_000;
    let mut fichiers: Vec<String> = Vec::new();
    let mut dossiers = vec![std::path::PathBuf::from(racine)];
    while let Some(d) = dossiers.pop() {
        if fichiers.len() >= MAX_FICHIERS {
            break;
        }
        let Ok(lecture) = std::fs::read_dir(&d) else { continue };
        for e in lecture.flatten() {
            match e.file_type() {
                Ok(t) if t.is_dir() => dossiers.push(e.path()),
                Ok(t) if t.is_file() => fichiers.push(e.path().to_string_lossy().into_owned()),
                _ => {}
            }
        }
    }
    let mut noms: Vec<String> = Vec::new();
    // SAFETY: les tampons (clé de session, liste de processus) vivent pendant tout l'appel ; la session est fermée avant de sortir.
    unsafe {
        let mut session = 0u32;
        let mut cle = [0u16; 33];
        if RmStartSession(&mut session, 0, PWSTR(cle.as_mut_ptr())).0 != 0 {
            return noms;
        }
        for lot in fichiers.chunks(400) {
            let w: Vec<Vec<u16>> = lot.iter().map(|f| f.encode_utf16().chain(std::iter::once(0)).collect()).collect();
            let p: Vec<PCWSTR> = w.iter().map(|x| PCWSTR(x.as_ptr())).collect();
            if RmRegisterResources(session, Some(&p), None, None).0 != 0 {
                continue;
            }
            let (mut besoin, mut recu, mut raisons) = (0u32, 16u32, 0u32);
            let mut liste = vec![RM_PROCESS_INFO::default(); 16];
            let mut r = RmGetList(session, &mut besoin, &mut recu, Some(liste.as_mut_ptr()), &mut raisons);
            if r.0 == 234 {
                // ERROR_MORE_DATA : on redemande avec la bonne taille
                liste = vec![RM_PROCESS_INFO::default(); besoin as usize];
                recu = besoin;
                r = RmGetList(session, &mut besoin, &mut recu, Some(liste.as_mut_ptr()), &mut raisons);
            }
            if r.0 == 0 {
                for i in liste.iter().take(recu as usize) {
                    let n = i.strAppName.iter().position(|&c| c == 0).unwrap_or(0);
                    let nom = String::from_utf16_lossy(&i.strAppName[..n]);
                    if !nom.is_empty() && !noms.contains(&nom) {
                        noms.push(nom);
                    }
                }
            }
        }
        let _ = RmEndSession(session);
    }
    noms
}

/// Éjecte un disque amovible : verrouille le volume (échoue tant qu'un programme y garde un fichier ouvert),
/// le démonte, puis demande l'éjection. L'erreur dit ce qui a bloqué, en une phrase pour la barre d'état.
/// Les programmes qui gardent un fichier ouvert sont nommés par le Gestionnaire de redémarrage (`programmes_sur`).
pub fn ejecter(lettre: char) -> Result<(), String> {
    use windows::core::PCWSTR;
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::Storage::FileSystem::{CreateFileW, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING, FILE_FLAGS_AND_ATTRIBUTES};
    use windows::Win32::System::IO::DeviceIoControl;
    const GENERIC_RW: u32 = 0xC000_0000;
    const FSCTL_LOCK_VOLUME: u32 = 0x9_0018;
    const FSCTL_DISMOUNT_VOLUME: u32 = 0x9_0020;
    const IOCTL_STORAGE_MEDIA_REMOVAL: u32 = 0x2D_4804;
    const IOCTL_STORAGE_EJECT_MEDIA: u32 = 0x2D_4808;
    let nom: Vec<u16> = format!("\\\\.\\{lettre}:").encode_utf16().chain(std::iter::once(0)).collect();
    // SAFETY: `nom` est terminé par un zéro et vit pendant tout l'appel ; le handle est fermé avant de sortir.
    unsafe {
        let h = CreateFileW(PCWSTR(nom.as_ptr()), GENERIC_RW, FILE_SHARE_READ | FILE_SHARE_WRITE, None, OPEN_EXISTING, FILE_FLAGS_AND_ATTRIBUTES(0), None)
            .map_err(|_| format!("{lettre}: NON ACCESSIBLE (DROITS OU DISQUE DEJA RETIRE)"))?;
        let ioctl = |code: u32, entree: Option<&[u8]>| {
            let mut n = 0u32;
            DeviceIoControl(h, code, entree.map(|e| e.as_ptr() as *const _), entree.map_or(0, |e| e.len() as u32), None, 0, Some(&mut n), None).is_ok()
        };
        // Un antivirus ou l'indexeur lâche souvent le volume après un instant : quelques essais avant de renoncer
        let mut verrou = false;
        for _ in 0..5 {
            verrou = ioctl(FSCTL_LOCK_VOLUME, None);
            if verrou {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(300));
        }
        let r = if !verrou {
            let qui = programmes_sur(&format!("{lettre}:\\"));
            Err(if qui.is_empty() {
                format!("{lettre}: OCCUPE : UN PROGRAMME GARDE DES FICHIERS OUVERTS SUR CE DISQUE (LES FERMER, PUIS REESSAYER)")
            } else {
                format!("{lettre}: OCCUPE PAR {} : LES FERMER, PUIS REESSAYER", qui.join(", ").to_uppercase())
            })
        } else if !ioctl(FSCTL_DISMOUNT_VOLUME, None) {
            Err(format!("{lettre}: IMPOSSIBLE A DEMONTER"))
        } else {
            let _ = ioctl(IOCTL_STORAGE_MEDIA_REMOVAL, Some(&[0u8]));
            if ioctl(IOCTL_STORAGE_EJECT_MEDIA, None) { Ok(()) } else { Err(format!("{lettre}: DEMONTE, MAIS LE MATERIEL REFUSE L'EJECTION (LE RETIRER SANS DANGER)")) }
        };
        let _ = CloseHandle(h);
        r
    }
}

/// Au-delà, la signature n'est pas vérifiée
const TAILLE_VERIFIEE_MAX: u64 = 64 << 20;

/// Signataire d'un programme par `WinVerifyTrust`, sans réseau (ni révocation ni téléchargement de certificats).
/// shortcut: signature intégrée seulement ; un fichier signé par catalogue (beaucoup de fichiers de Windows) sort « NON SIGNE »,
/// à améliorer avec `CryptCATAdminAcquireContext` si ce cas gêne.
pub fn signature(chemin: &str) -> Signature {
    use windows::core::{GUID, PCWSTR};
    use windows::Win32::Foundation::HWND;
    use windows::Win32::Security::Cryptography::{CertGetNameStringW, CERT_NAME_SIMPLE_DISPLAY_TYPE};
    use windows::Win32::Security::WinTrust::*;
    let ext = chemin.rsplit('.').next().unwrap_or("").to_ascii_lowercase();
    if !matches!(ext.as_str(), "exe" | "dll" | "msi" | "sys" | "ocx" | "scr" | "cab") {
        return Signature::SansObjet;
    }
    // WinVerifyTrust calcule l'empreinte de tout le fichier : 0,3 à 0,5 s pour un installeur de 100 Mo, des secondes au-delà
    if std::fs::metadata(chemin).map_or(0, |m| m.len()) > TAILLE_VERIFIEE_MAX {
        return Signature::NonVerifie;
    }
    let w: Vec<u16> = chemin.encode_utf16().chain(std::iter::once(0)).collect();
    // Invariant : `fichier` et `w` vivent plus longtemps que les deux appels à WinVerifyTrust qui les lisent
    unsafe {
        let mut fichier = WINTRUST_FILE_INFO { cbStruct: std::mem::size_of::<WINTRUST_FILE_INFO>() as u32, pcwszFilePath: PCWSTR(w.as_ptr()), ..Default::default() };
        let mut donnees = WINTRUST_DATA {
            cbStruct: std::mem::size_of::<WINTRUST_DATA>() as u32,
            dwUIChoice: WTD_UI_NONE,
            fdwRevocationChecks: WTD_REVOKE_NONE,
            dwUnionChoice: WTD_CHOICE_FILE,
            Anonymous: WINTRUST_DATA_0 { pFile: &mut fichier },
            dwStateAction: WTD_STATEACTION_VERIFY,
            dwProvFlags: WTD_CACHE_ONLY_URL_RETRIEVAL,
            ..Default::default()
        };
        let mut action: GUID = WINTRUST_ACTION_GENERIC_VERIFY_V2;
        let r = WinVerifyTrust(HWND::default(), &mut action, &mut donnees as *mut _ as *mut _);
        let mut nom = None;
        if r != 0x800B0100u32 as i32 {
            // L'état vérifié donne la chaîne de certificats : le premier est celui du signataire
            let prov = WTHelperProvDataFromStateData(donnees.hWVTStateData);
            let signataire = WTHelperGetProvSignerFromChain(prov, 0, false, 0);
            let cert = if signataire.is_null() { std::ptr::null_mut() } else { WTHelperGetProvCertFromChain(signataire, 0) };
            if !cert.is_null() && !(*cert).pCert.is_null() {
                let mut tampon = [0u16; 256];
                let n = CertGetNameStringW((*cert).pCert, CERT_NAME_SIMPLE_DISPLAY_TYPE, 0, None, Some(&mut tampon)) as usize;
                if n > 1 {
                    nom = Some(String::from_utf16_lossy(&tampon[..n - 1]));
                }
            }
        }
        donnees.dwStateAction = WTD_STATEACTION_CLOSE;
        WinVerifyTrust(HWND::default(), &mut action, &mut donnees as *mut _ as *mut _);
        match (r, nom) {
            (0, Some(n)) => Signature::Valide(n),
            // TRUST_E_NOSIGNATURE, TRUST_E_PROVIDER_UNKNOWN, TRUST_E_SUBJECT_FORM_UNKNOWN : rien à vérifier
            (r, _) if [0x800B0100u32, 0x800B0001, 0x800B0003].contains(&(r as u32)) => Signature::Aucune,
            _ => Signature::Invalide,
        }
    }
}

/// Retire la marque d'origine Internet (comme « Débloquer » des propriétés de Windows)
pub fn debloquer(chemin: &str) -> Result<(), String> {
    std::fs::remove_file(format!("{chemin}:Zone.Identifier")).map_err(|e| e.to_string())
}

/// SHA-256 d'un fichier (lu par morceaux de 1 Mo)
pub fn empreinte(chemin: &str) -> Result<String, String> {
    use std::io::Read;
    let mut f = std::fs::File::open(chemin).map_err(|e| e.to_string().to_uppercase())?;
    let mut h = Sha256::new();
    let mut buf = vec![0u8; 1 << 20];
    loop {
        let n = f.read(&mut buf).map_err(|e| e.to_string().to_uppercase())?;
        if n == 0 {
            break;
        }
        h.ajouter(&buf[..n]);
    }
    Ok(h.finir())
}

/// Compare par SHA-256 chaque fichier copié à son original (dossiers parcourus, liens ignorés).
/// Renvoie le nombre de fichiers vérifiés et les noms de ceux qui diffèrent ou manquent.
pub fn verifier_copies(paires: &[(String, String)]) -> (usize, Vec<String>) {
    use std::path::{Path, PathBuf};
    fn lister(source: &Path, copie: &Path, v: &mut Vec<(PathBuf, PathBuf)>) {
        let Ok(t) = std::fs::symlink_metadata(source).map(|m| m.file_type()) else { return };
        if t.is_symlink() {
            return;
        }
        if t.is_dir() {
            for e in std::fs::read_dir(source).into_iter().flatten().flatten() {
                lister(&e.path(), &copie.join(e.file_name()), v);
            }
        } else {
            v.push((source.into(), copie.into()));
        }
    }
    let mut fichiers = Vec::new();
    for (s, c) in paires {
        lister(Path::new(s), Path::new(c), &mut fichiers);
    }
    let mut mauvais = Vec::new();
    for (s, c) in &fichiers {
        let (a, b) = (empreinte(&s.to_string_lossy()), empreinte(&c.to_string_lossy()));
        if !matches!((&a, &b), (Ok(x), Ok(y)) if x == y) {
            mauvais.push(s.file_name().map_or(String::new(), |n| n.to_string_lossy().into_owned()));
        }
    }
    (fichiers.len() - mauvais.len(), mauvais)
}

// ───────────────────────── Balises de couleur ─────────────────────────

/// Couleurs des balises : rouge, blanc, gris (palette Nothing)
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Balise {
    Rouge,
    Blanc,
    Gris,
}

impl Balise {
    fn lettre(self) -> char {
        match self {
            Balise::Rouge => 'r',
            Balise::Blanc => 'b',
            Balise::Gris => 'g',
        }
    }
    fn depuis(c: char) -> Option<Self> {
        match c {
            'r' => Some(Balise::Rouge),
            'b' => Some(Balise::Blanc),
            'g' => Some(Balise::Gris),
            _ => None,
        }
    }
}

/// Chemin en minuscules → balise ; chargé une fois depuis les réglages
static BALISES: Mutex<Option<HashMap<String, Balise>>> = Mutex::new(None);

fn avec_balises<R>(f: impl FnOnce(&mut HashMap<String, Balise>) -> R) -> R {
    let mut g = BALISES.lock().unwrap_or_else(|e| e.into_inner());
    let m = g.get_or_insert_with(|| {
        crate::reglages::actuels()
            .balises
            .iter()
            .filter_map(|l| {
                let (c, p) = l.split_once('|')?;
                Some((p.to_lowercase(), Balise::depuis(c.chars().next()?)?))
            })
            .collect()
    });
    f(m)
}

pub fn balise(chemin: &str) -> Option<Balise> {
    avec_balises(|m| if m.is_empty() { None } else { m.get(&chemin.to_lowercase()).copied() })
}

/// Pose (ou retire, None) une balise sur ces chemins, et l'enregistre
pub fn baliser(chemins: &[String], b: Option<Balise>) {
    let cles: Vec<String> = chemins.iter().map(|c| c.to_lowercase()).collect();
    avec_balises(|m| {
        for c in &cles {
            match b {
                Some(b) => {
                    m.insert(c.clone(), b);
                }
                None => {
                    m.remove(c);
                }
            }
        }
    });
    // Appliqué à la dernière version du fichier, pas à la carte de ce processus : une autre fenêtre a pu
    // poser d'autres balises depuis (voir reglages::modifier)
    crate::reglages::modifier(|r| {
        r.balises.retain(|l| l.split_once('|').map_or(true, |(_, p)| !cles.iter().any(|c| c == &p.to_lowercase())));
        if let Some(b) = b {
            r.balises.extend(cles.iter().map(|c| format!("{}|{c}", b.lettre())));
        }
    });
}

#[cfg(test)]
mod tests {
    #[test]
    fn copies_verifiees() {
        let base = std::env::temp_dir().join(format!("ne-verif-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let (a, b) = (base.join("a"), base.join("b"));
        std::fs::create_dir_all(a.join("sous")).unwrap();
        std::fs::create_dir_all(b.join("sous")).unwrap();
        for (nom, texte) in [("x.txt", "un"), ("sous\\y.txt", "deux"), ("z.txt", "trois")] {
            std::fs::write(a.join(nom), texte).unwrap();
            std::fs::write(b.join(nom), texte).unwrap();
        }
        let paires = [(a.to_string_lossy().into_owned(), b.to_string_lossy().into_owned())];
        assert_eq!(super::verifier_copies(&paires), (3, vec![]));
        // Une copie abîmée, une copie absente
        std::fs::write(b.join("sous\\y.txt"), "DEUX").unwrap();
        std::fs::remove_file(b.join("z.txt")).unwrap();
        let (bons, mauvais) = super::verifier_copies(&paires);
        assert_eq!(bons, 1);
        assert_eq!(mauvais.len(), 2);
        assert!(mauvais.contains(&"y.txt".to_string()) && mauvais.contains(&"z.txt".to_string()));
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn signature_des_programmes() {
        use super::Signature::*;
        let f = std::env::temp_dir().join(format!("ne-sig-{}.exe", std::process::id()));
        std::fs::write(&f, b"MZ pas un vrai programme").unwrap();
        assert!(matches!(super::signature(f.to_str().unwrap()), Aucune | Invalide));
        let _ = std::fs::remove_file(&f);
        assert_eq!(super::signature("C:\\x\\notes.txt"), SansObjet);
        assert_eq!(Aucune.texte(), "NON SIGNE");
        // Un programme à signature intégrée présent sur cette machine (ignoré ailleurs)
        let dossier = r"C:\Users\utilisateur\AppData\Local\Programs\@codebufffreebuff-desktop";
        let petit = format!(r"{dossier}\resources\git\mingw64\bin\Avalonia.Base.dll");
        if std::path::Path::new(&petit).exists() {
            assert!(matches!(super::signature(&petit), Valide(n) if !n.is_empty()));
        }
        // Signé aussi, mais 180 Mo : au-dessus de la limite de vérification
        let gros = format!(r"{dossier}\Freebuff.exe");
        if std::path::Path::new(&gros).exists() {
            assert_eq!(super::signature(&gros), NonVerifie);
        }
    }

    #[test]
    fn origine_et_deblocage() {
        let f = std::env::temp_dir().join(format!("ne-zone-{}.txt", std::process::id()));
        let c = f.to_str().unwrap().to_string();
        std::fs::write(&f, "x").unwrap();
        assert_eq!(super::origine_internet(&c), None);
        std::fs::write(format!("{c}:Zone.Identifier"), "[ZoneTransfer]\r\nZoneId=3\r\nHostUrl=https://exemple.org/a/b?token=secret\r\n").unwrap();
        assert_eq!(super::origine_internet(&c), Some((3, Some("exemple.org".to_string()))));
        super::debloquer(&c).unwrap();
        assert_eq!(super::origine_internet(&c), None);
        // Zone locale (0) : pas une origine Internet
        std::fs::write(format!("{c}:Zone.Identifier"), "[ZoneTransfer]\r\nZoneId=0\r\n").unwrap();
        assert_eq!(super::origine_internet(&c), None);
        let _ = std::fs::remove_file(&f);
    }

    use super::*;

    #[test]
    fn programme_qui_garde_un_fichier() {
        let d = std::env::temp_dir().join(format!("ne-rm-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        let f = d.join("tenu.txt");
        std::fs::write(&f, "x").unwrap();
        // Le fichier reste ouvert par ce test : le Gestionnaire doit nommer un programme
        let _ouvert = std::fs::File::open(&f).unwrap();
        let qui = programmes_sur(&d.to_string_lossy());
        assert!(!qui.is_empty(), "aucun programme nommé");
        drop(_ouvert);
        assert!(programmes_sur(&d.to_string_lossy()).is_empty());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn lettres_de_lecteur() {
        assert_eq!(lettre_lecteur("e:\\photos"), Some('E'));
        assert_eq!(lettre_lecteur("E:"), Some('E'));
        assert_eq!(lettre_lecteur("\\\\nas\\partage"), None);
        assert_eq!(lettre_lecteur("1:\\"), None);
    }

    #[test]
    fn sha256() {
        let h = |s: &[u8]| {
            let mut x = Sha256::new();
            x.ajouter(s);
            x.finir()
        };
        assert_eq!(h(b""), "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855");
        assert_eq!(h(b"abc"), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
        let long = vec![b'a'; 1_000_000];
        let mut x = Sha256::new();
        for m in long.chunks(777) {
            x.ajouter(m);
        }
        assert_eq!(x.finir(), "cdc76e5c9914fb9281a1c7e284d73e67f1809a48a497200e046d39ccc7112cd0");
    }

    #[test]
    fn chemins() {
        assert_eq!(chemin_wsl(r"C:\Users\nico"), "/mnt/c/Users/nico");
        assert_eq!(chemin_wsl(r"D:\"), "/mnt/d");
        assert_eq!(chemin_wsl(r"\\zima\Media"), "//zima/Media");
        assert_eq!(chemin_unix(r"C:\a\b"), "C:/a/b");
    }
}
