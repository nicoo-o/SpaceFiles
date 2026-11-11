// Client Eco : appeler une autre application (ou lui envoyer un événement) par son tube nommé, sans jamais bloquer
// plus que le budget (300 ms par défaut) et sans exception ni panique. Les fonctions attendent sur le fil appelant (les
// entrées-sorties sont recouvertes pour pouvoir être annulées au bout du budget) : elles ne s'appellent JAMAIS depuis le
// fil d'interface, seulement depuis un fil à part (aujourd'hui : le fil d'émission de `progression.rs`).
//
// Règle côté client du protocole (`docs/ecosystem/protocol.md`) : la DACL du tube protège le serveur, pas le client. Avant
// d'envoyer quoi que ce soit : ouverture avec `SECURITY_SQOS_PRESENT | SECURITY_IDENTIFICATION` (le serveur ne peut
// qu'identifier le client), puis le processus serveur (`GetNamedPipeServerProcessId`) doit avoir le même SID que nous,
// une intégrité moyenne ou plus, ne pas être dans un AppContainer, et (hors essai) être l'exe du manifeste de l'application.
// Sinon `NonFiable` : rien n'est envoyé.

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use windows::core::{PCWSTR, PWSTR};
use windows::Win32::Foundation::*;
use windows::Win32::Security::{EqualSid, GetSidSubAuthority, GetSidSubAuthorityCount, GetTokenInformation, TokenIntegrityLevel, TokenIsAppContainer, TokenUser, TOKEN_INFORMATION_CLASS, TOKEN_MANDATORY_LABEL, TOKEN_QUERY, TOKEN_USER};
use windows::Win32::Storage::FileSystem::{CreateFileW, ReadFile, WriteFile, FILE_FLAG_OVERLAPPED, FILE_SHARE_MODE, OPEN_EXISTING, SECURITY_IDENTIFICATION, SECURITY_SQOS_PRESENT};
use windows::Win32::System::Pipes::{GetNamedPipeServerProcessId, WaitNamedPipeW};
use windows::Win32::System::Threading::{CreateEventW, GetCurrentProcess, OpenProcess, OpenProcessToken, QueryFullProcessImageNameW, WaitForSingleObject, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION};
use windows::Win32::System::IO::{CancelIoEx, GetOverlappedResult, OVERLAPPED};

use super::json::{champ, Json};
use super::trame::{analyser_reponse, trame_sortante, Corps, MAX_TRAME, PROTOCOLE};

/// Budget total d'un appel : connexion, vérification, envoi et réponse
pub const BUDGET_DEFAUT: Duration = Duration::from_millis(300);
/// Une réponse de `hello` est gardée aussi longtemps
const DUREE_CACHE_HELLO: Duration = Duration::from_secs(60);
/// Intégrité « moyenne » (SECURITY_MANDATORY_MEDIUM_RID) : en dessous, le serveur est un processus bridé
const INTEGRITE_MOYENNE: u32 = 0x2000;

#[derive(Debug, Clone, PartialEq)]
pub struct Reponse {
    pub resultat: Vec<(String, Json)>,
}

/// Résultat typé d'un appel : jamais d'exception
#[derive(Debug, PartialEq)]
pub enum Issue {
    /// Le serveur a répondu par un `result`
    Ok(Reponse),
    /// Pas de tube de ce nom : l'application n'est pas lancée
    Absent,
    /// Le budget est épuisé (connexion, envoi ou réponse trop lents)
    Delai,
    /// Le processus qui sert le tube n'est pas celui qu'on attend : rien n'a été envoyé
    NonFiable,
    /// Réponse illisible, pas du protocole que nous parlons, ou `hello` annonçant un protocole plus récent que le nôtre
    Protocole,
    /// Le serveur a répondu par une erreur nommée (`inconnu`, `version`, `hors_bornes`…)
    Erreur { code: String },
}

/// Quel exécutable doit servir le tube
#[derive(Debug, Clone)]
pub enum ExeAttendu {
    /// Aucune exigence : tube injecté pour un essai
    Quelconque,
    /// Celui du manifeste de cette application (`%LOCALAPPDATA%\Eco\apps\<app>.json`) ; sans manifeste : non fiable
    Manifeste(String),
    /// Ce chemin exact (sans tenir compte de la casse) : tests seulement, sans appelant dans l'exe
    #[cfg(test)]
    Chemin(String),
}

/// À qui l'on parle : le tube et ce qu'on exige de l'exe qui le sert
#[derive(Debug, Clone)]
pub struct Cible {
    pub nom_tube: String,
    pub attendu: ExeAttendu,
}

impl Cible {
    /// Une application d'après l'utilisateur Windows ; `essai` (variable `SPACEFILES_PIPE`) donne un tube d'essai, sans
    /// exigence sur l'exe (un faux serveur de test n'est pas l'exe du manifeste)
    pub fn pour(app: &str, utilisateur: &str, essai: Option<&str>) -> Cible {
        let essai = essai.filter(|e| !e.is_empty());
        Cible {
            nom_tube: super::nom_tube(app, utilisateur, essai),
            attendu: if essai.is_some() { ExeAttendu::Quelconque } else { ExeAttendu::Manifeste(app.to_string()) },
        }
    }

    /// Tube et exigence donnés tels quels (tests : un faux serveur à un nom unique)
    #[cfg(test)]
    pub fn avec(nom_tube: &str, attendu: ExeAttendu) -> Cible {
        Cible { nom_tube: nom_tube.to_string(), attendu }
    }
}

struct Poignee(HANDLE);

impl Drop for Poignee {
    fn drop(&mut self) {
        // SAFETY : le handle appartient à cette poignée et n'est fermé qu'ici.
        unsafe {
            let _ = CloseHandle(self.0);
        }
    }
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

fn restant_ms(fin: Instant) -> u32 {
    fin.saturating_duration_since(Instant::now()).as_millis().min(u32::MAX as u128) as u32
}

/// Le tube existe-t-il ? (une instance libre ou occupée), sans l'ouvrir : `WaitNamedPipe` n'ouvre rien et ne se connecte
/// pas ; 1 ms de patience au plus.
pub fn present(nom_tube: &str) -> bool {
    let nom = wide(nom_tube);
    // SAFETY : `nom` est terminé par 0 et vit pendant l'appel.
    if unsafe { WaitNamedPipeW(PCWSTR(nom.as_ptr()), 1) }.as_bool() {
        return true;
    }
    // SAFETY : lecture du dernier code d'erreur du fil, juste après l'appel qui a échoué.
    // Existe mais toutes les instances sont occupées : le délai de 1 ms expire
    unsafe { GetLastError() == ERROR_SEM_TIMEOUT }
}

// ───────────────────────── Vérification du serveur ─────────────────────────

/// Ce que `serveur_fiable` a établi sur le processus serveur
#[derive(Debug, Clone, Copy)]
struct Profil {
    meme_sid: bool,
    integrite: u32,
    app_container: bool,
    /// None : exe non exigé ; Some(faux) : ce n'est pas l'exe attendu
    exe_ok: Option<bool>,
}

/// Décision pure (testée) : mêmes SID, intégrité moyenne ou plus, pas d'AppContainer, exe attendu si exigé
fn profil_fiable(p: &Profil) -> bool {
    p.meme_sid && p.integrite >= INTEGRITE_MOYENNE && !p.app_container && p.exe_ok != Some(false)
}

fn info_jeton(jeton: HANDLE, classe: TOKEN_INFORMATION_CLASS) -> Option<Vec<u64>> {
    // SAFETY : tampon aligné sur 8 octets et de la taille demandée par GetTokenInformation ; `jeton` est ouvert par l'appelant.
    unsafe {
        let mut taille = 0u32;
        let _ = GetTokenInformation(jeton, classe, None, 0, &mut taille);
        if taille == 0 {
            return None;
        }
        let mut t = vec![0u64; (taille as usize).div_ceil(8)];
        GetTokenInformation(jeton, classe, Some(t.as_mut_ptr() as *mut _), (t.len() * 8) as u32, &mut taille).ok()?;
        Some(t)
    }
}

fn jeton_de(processus: HANDLE) -> Option<Poignee> {
    let mut jeton = HANDLE::default();
    // SAFETY : `processus` est un handle de processus ouvert par l'appelant ; `jeton` reçoit un nouveau handle, fermé par `Poignee`.
    unsafe { OpenProcessToken(processus, TOKEN_QUERY, &mut jeton) }.ok()?;
    Some(Poignee(jeton))
}

fn chemin_exe(processus: HANDLE) -> Option<String> {
    let mut tampon = vec![0u16; 32_768];
    let mut n = tampon.len() as u32;
    // SAFETY : tampon de `n` unités UTF-16 ; `n` est mis à jour avec la longueur écrite.
    unsafe { QueryFullProcessImageNameW(processus, PROCESS_NAME_WIN32, PWSTR(tampon.as_mut_ptr()), &mut n) }.ok()?;
    Some(String::from_utf16_lossy(&tampon[..n as usize]))
}

fn exe_depuis_manifeste(texte: &str) -> Option<String> {
    let Some(Json::Objet(o)) = super::json::analyser_json(texte.trim_start_matches('\u{feff}')) else { return None };
    match champ(&o, "exe") {
        Some(Json::Texte(t)) if !t.is_empty() => Some(t.clone()),
        _ => None,
    }
}

/// Taille maximale lue d'un manifeste (le vrai en fait moins de 2 Ko)
const MANIFESTE_MAX: u64 = 64 * 1024;

type EntreeManifeste = (std::path::PathBuf, std::time::SystemTime, u64, Option<String>);

/// Dernier `exe` lu par manifeste : (chemin, date de modification, taille, exe). Le disque n'est relu que si le fichier a
/// changé : la vérification d'un serveur ne lit ni ne décode un fichier à chaque connexion.
static MANIFESTES: Mutex<Vec<EntreeManifeste>> = Mutex::new(Vec::new());
#[cfg(test)]
static MANIFESTES_LUS: AtomicU32 = AtomicU32::new(0);

fn exe_du_fichier(f: &std::path::Path) -> Option<String> {
    use std::io::Read;
    let m = std::fs::metadata(f).ok()?;
    let (date, taille) = (m.modified().ok()?, m.len());
    let mut cache = MANIFESTES.lock().unwrap_or_else(|e| e.into_inner());
    if let Some((_, _, _, exe)) = cache.iter().find(|(c, d, t, _)| c == f && *d == date && *t == taille) {
        return exe.clone();
    }
    // Lecture bornée : un manifeste gonflé (ou un fichier sans fin) ne coûte jamais plus que `MANIFESTE_MAX`
    let mut octets = Vec::new();
    std::fs::File::open(f).ok()?.take(MANIFESTE_MAX + 1).read_to_end(&mut octets).ok()?;
    #[cfg(test)]
    MANIFESTES_LUS.fetch_add(1, Ordering::SeqCst);
    let exe = if octets.len() as u64 > MANIFESTE_MAX { None } else { exe_depuis_manifeste(&String::from_utf8_lossy(&octets)) };
    cache.retain(|(c, _, _, _)| c != f);
    cache.push((f.to_path_buf(), date, taille, exe.clone()));
    exe
}

fn exe_du_manifeste(app: &str) -> Option<String> {
    exe_du_fichier(&super::dossier_eco()?.join("apps").join(format!("{app}.json")))
}

/// Même programme ? Sans tenir compte de la casse (Unicode compris), puis d'après les chemins FINAUX (jonction, nom 8.3,
/// lien). Le manifeste est écrit par un processus du même utilisateur : cette règle ne protège que d'un imposteur
/// accidentel, pas d'un processus malveillant du même compte (voir `protocol.md`).
fn meme_exe(a: &str, b: &str) -> bool {
    let bas = |s: &str| s.to_lowercase();
    if bas(a) == bas(b) {
        return true;
    }
    let final_ = |s: &str| std::fs::canonicalize(s).ok().map(|p| bas(p.to_string_lossy().trim_start_matches(r"\\?\")));
    matches!((final_(a), final_(b)), (Some(x), Some(y)) if x == y)
}

/// Le processus qui tient ce tube est-il digne de confiance ? Toute étape qui échoue (processus plus privilégié dont on ne
/// peut ouvrir le jeton, par exemple) donne « non ».
fn serveur_fiable(h: HANDLE, attendu: &ExeAttendu) -> bool {
    let mut pid = 0u32;
    // SAFETY : `h` est l'extrémité cliente ouverte par `Tube::ouvrir` ; `pid` est valide pendant l'appel.
    if unsafe { GetNamedPipeServerProcessId(h, &mut pid) }.is_err() || pid == 0 {
        return false;
    }
    // SAFETY : handle de processus fermé par `Poignee` ; droit minimal (requêtes limitées).
    let Ok(processus) = (unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) }).map(Poignee) else { return false };
    let Some(jeton_serveur) = jeton_de(processus.0) else { return false };
    // SAFETY : pseudo-handle du processus courant (ne se ferme pas).
    let Some(jeton_client) = jeton_de(unsafe { GetCurrentProcess() }) else { return false };
    let (Some(u_serveur), Some(u_client)) = (info_jeton(jeton_serveur.0, TokenUser), info_jeton(jeton_client.0, TokenUser)) else { return false };
    // SAFETY : chaque tampon contient un TOKEN_USER complet (aligné sur 8), vivant jusqu'à la fin de la fonction ; le SID
    // pointe dans ce même tampon.
    let meme_sid = unsafe {
        let a = &*(u_serveur.as_ptr() as *const TOKEN_USER);
        let b = &*(u_client.as_ptr() as *const TOKEN_USER);
        EqualSid(a.User.Sid, b.User.Sid).is_ok()
    };
    let Some(etiquette) = info_jeton(jeton_serveur.0, TokenIntegrityLevel) else { return false };
    // SAFETY : le tampon contient un TOKEN_MANDATORY_LABEL ; le SID pointe dans le tampon, `GetSidSubAuthorityCount`
    // donne le nombre de sous-autorités, la dernière est le niveau d'intégrité.
    let integrite = unsafe {
        let e = &*(etiquette.as_ptr() as *const TOKEN_MANDATORY_LABEL);
        let n = *GetSidSubAuthorityCount(e.Label.Sid);
        if n == 0 {
            0
        } else {
            *GetSidSubAuthority(e.Label.Sid, (n - 1) as u32)
        }
    };
    let app_container = info_jeton(jeton_serveur.0, TokenIsAppContainer).is_none_or(|v| v.first().is_some_and(|&x| x as u32 != 0));
    let exe_ok = match attendu {
        ExeAttendu::Quelconque => None,
        #[cfg(test)]
        ExeAttendu::Chemin(c) => Some(chemin_exe(processus.0).is_some_and(|e| meme_exe(&e, c))),
        ExeAttendu::Manifeste(app) => Some(match (exe_du_manifeste(app), chemin_exe(processus.0)) {
            (Some(m), Some(e)) => meme_exe(&m, &e),
            _ => false,
        }),
    };
    profil_fiable(&Profil { meme_sid, integrite, app_container, exe_ok })
}

// ───────────────────────── Connexion ─────────────────────────

struct Tube(Poignee);

impl Tube {
    fn h(&self) -> HANDLE {
        (self.0).0
    }

    /// Ouvre le tube avant `fin`, attend une instance libre si elles sont toutes occupées
    fn ouvrir(nom_tube: &str, fin: Instant) -> Result<Tube, Issue> {
        let nom = wide(nom_tube);
        loop {
            if Instant::now() >= fin {
                return Err(Issue::Delai);
            }
            // SAFETY : `nom` est terminé par 0 et vit pendant l'appel ; le handle rendu est fermé par `Poignee`.
            let r = unsafe {
                CreateFileW(PCWSTR(nom.as_ptr()), (GENERIC_READ | GENERIC_WRITE).0, FILE_SHARE_MODE(0), None, OPEN_EXISTING, FILE_FLAG_OVERLAPPED | SECURITY_SQOS_PRESENT | SECURITY_IDENTIFICATION, None)
            };
            match r {
                Ok(h) => return Ok(Tube(Poignee(h))),
                Err(e) if e.code() == ERROR_PIPE_BUSY.to_hresult() => {
                    // Toutes les instances sont occupées : on attend qu'une se libère, dans la limite du budget
                    // SAFETY : même chaîne que ci-dessus.
                    let _ = unsafe { WaitNamedPipeW(PCWSTR(nom.as_ptr()), restant_ms(fin).max(1)) };
                }
                // Un tube du même nom créé par un autre compte : la DACL nous refuse
                Err(e) if e.code() == ERROR_ACCESS_DENIED.to_hresult() => return Err(Issue::NonFiable),
                Err(_) => return Err(Issue::Absent),
            }
        }
    }

    fn ecrire_tout(&self, octets: &[u8], fin: Instant) -> Result<(), Issue> {
        let mut reste = octets;
        while !reste.is_empty() {
            let ev = evenement()?;
            let mut ov = OVERLAPPED { hEvent: ev.0, ..Default::default() };
            // SAFETY : `reste` et `ov` vivent jusqu'au retour de `terminer`, qui n'en rend la main qu'une fois l'E/S finie
            // ou annulée puis attendue ; `self.h()` est ouvert avec FILE_FLAG_OVERLAPPED.
            let n = unsafe {
                let r = WriteFile(self.h(), Some(reste), None, Some(&mut ov));
                terminer(self.h(), &mut ov, r, fin)?
            };
            if n == 0 {
                return Err(Issue::Protocole);
            }
            reste = &reste[n as usize..];
        }
        Ok(())
    }

    /// Lit une ligne (sans son `\n`), 4 096 octets au plus
    fn lire_ligne(&self, fin: Instant) -> Result<String, Issue> {
        let mut tampon: Vec<u8> = Vec::new();
        let mut bloc = [0u8; 1024];
        loop {
            if let Some(p) = tampon.iter().position(|&c| c == b'\n') {
                return String::from_utf8(tampon[..p].to_vec()).map_err(|_| Issue::Protocole);
            }
            // + 1 : le `\r` d'une ligne pile à la borne
            if tampon.len() > MAX_TRAME + 1 {
                return Err(Issue::Protocole);
            }
            let ev = evenement()?;
            let mut ov = OVERLAPPED { hEvent: ev.0, ..Default::default() };
            // SAFETY : `bloc` et `ov` vivent jusqu'au retour de `terminer` (E/S finie ou annulée puis attendue).
            let n = unsafe {
                let r = ReadFile(self.h(), Some(&mut bloc), None, Some(&mut ov));
                terminer(self.h(), &mut ov, r, fin)?
            };
            if n == 0 {
                return Err(Issue::Protocole);
            }
            tampon.extend_from_slice(&bloc[..n as usize]);
        }
    }
}

fn evenement() -> Result<Poignee, Issue> {
    // SAFETY : événement anonyme à réinitialisation manuelle, fermé par `Poignee`.
    unsafe { CreateEventW(None, true, false, None) }.map(Poignee).map_err(|_| Issue::Protocole)
}

/// Attend la fin d'une entrée-sortie recouverte, au plus jusqu'à `fin`. Passé ce délai elle est annulée ET attendue : le
/// tampon et l'OVERLAPPED de l'appelant ne sont libérés qu'une fois le noyau lâché. Rend le nombre d'octets transférés.
///
/// # Safety
/// `h` est le handle sur lequel `lance` vient d'être lancé avec `ov` ; `ov` et le tampon vivent jusqu'au retour.
unsafe fn terminer(h: HANDLE, ov: &mut OVERLAPPED, lance: windows::core::Result<()>, fin: Instant) -> Result<u32, Issue> {
    let mut n = 0u32;
    match lance {
        Ok(()) => {}
        Err(e) if e.code() == ERROR_IO_PENDING.to_hresult() => {
            if WaitForSingleObject(ov.hEvent, restant_ms(fin)) != WAIT_OBJECT_0 {
                let _ = CancelIoEx(h, Some(ov));
                let _ = GetOverlappedResult(h, ov, &mut n, true);
                return Err(Issue::Delai);
            }
        }
        // Tube brisé, client refusé… : l'E/S n'a pas démarré
        Err(_) => return Err(Issue::Protocole),
    }
    GetOverlappedResult(h, ov, &mut n, false).map(|_| n).map_err(|_| Issue::Protocole)
}

// ───────────────────────── Appels ─────────────────────────

static COMPTEUR: AtomicU32 = AtomicU32::new(0);
/// (nom du tube, instant, réponse) de chaque `hello` réussi
static CACHE_HELLO: Mutex<Vec<(String, Instant, Reponse)>> = Mutex::new(Vec::new());

fn identifiant() -> String {
    format!("c{}", COMPTEUR.fetch_add(1, Ordering::Relaxed))
}

fn hello_en_cache(nom_tube: &str) -> Option<Reponse> {
    let mut c = CACHE_HELLO.lock().unwrap_or_else(|e| e.into_inner());
    c.retain(|(_, t, _)| t.elapsed() < DUREE_CACHE_HELLO);
    c.iter().find(|(n, _, _)| n == nom_tube).map(|(_, _, r)| r.clone())
}

/// `hello` annonçant un protocole plus récent que le nôtre (ou pas de `protocol`) : on ne sait pas le parler, on se dégrade
fn controler_hello(r: Reponse) -> Issue {
    match champ(&r.resultat, "protocol") {
        Some(Json::Nombre(n)) if *n >= 1.0 && *n <= PROTOCOLE as f64 => Issue::Ok(r),
        _ => Issue::Protocole,
    }
}

/// Connexion + vérification du serveur. Rend le tube prêt à écrire.
fn se_connecter(nom_tube: &str, attendu: &ExeAttendu, fin: Instant) -> Result<Tube, Issue> {
    let t = Tube::ouvrir(nom_tube, fin)?;
    if !serveur_fiable(t.h(), attendu) {
        return Err(Issue::NonFiable);
    }
    Ok(t)
}

/// Appelle `nom` chez `cible`. `args` : objet JSON déjà écrit. `hello` : la réponse est gardée 60 s (un `hello` raté
/// n'est pas gardé).
pub fn appeler(cible: &Cible, nom: &str, args: &str, budget: Duration) -> Issue {
    let (nom_tube, attendu) = (cible.nom_tube.as_str(), &cible.attendu);
    if nom == "hello" {
        if let Some(r) = hello_en_cache(nom_tube) {
            return Issue::Ok(r);
        }
    }
    let fin = Instant::now() + budget;
    let issue = (|| -> Result<Issue, Issue> {
        let t = se_connecter(nom_tube, attendu, fin)?;
        let id = identifiant();
        let trame = trame_sortante("call", &id, nom, args);
        t.ecrire_tout(format!("{trame}\n").as_bytes(), fin)?;
        let ligne = t.lire_ligne(fin)?;
        match analyser_reponse(&ligne) {
            Some((rid, Corps::Resultat(r))) if rid == id => Ok(Issue::Ok(Reponse { resultat: r })),
            Some((rid, Corps::Erreur(code))) if rid == id || rid.is_empty() => Ok(Issue::Erreur { code }),
            _ => Err(Issue::Protocole),
        }
    })();
    let issue = issue.unwrap_or_else(|e| e);
    match (nom, issue) {
        ("hello", Issue::Ok(r)) => match controler_hello(r) {
            Issue::Ok(r) => {
                CACHE_HELLO.lock().unwrap_or_else(|e| e.into_inner()).push((nom_tube.to_string(), Instant::now(), r.clone()));
                Issue::Ok(r)
            }
            autre => autre,
        },
        (_, i) => i,
    }
}

/// Envoie un événement (aucune réponse attendue). Mêmes garde-fous qu'un appel : tube présent, serveur vérifié, budget.
pub fn emettre(cible: &Cible, nom: &str, args: &str, budget: Duration) -> Result<(), Issue> {
    let (nom_tube, attendu) = (cible.nom_tube.as_str(), &cible.attendu);
    let fin = Instant::now() + budget;
    let t = se_connecter(nom_tube, attendu, fin)?;
    let trame = trame_sortante("event", &identifiant(), nom, args);
    t.ecrire_tout(format!("{trame}\n").as_bytes(), fin)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::eco::serveur::{Config, Gestionnaire, Serveur, INSTANCES};
    use crate::eco::trame::{reponse_code, reponse_hello, Appel};
    use crate::journal::Niveau;
    use std::sync::atomic::AtomicUsize;
    use std::sync::Arc;

    fn sans_trace(_: Niveau, _: &str) {}

    fn nom_unique(s: &str) -> String {
        static N: AtomicUsize = AtomicUsize::new(0);
        format!(r"\\.\pipe\Eco.spacefiles.client-{}-{}-{s}", std::process::id(), N.fetch_add(1, Ordering::SeqCst))
    }

    fn serveur(nom: &str, g: Box<Gestionnaire>) -> Serveur {
        let config = Config { nom_tube: nom.to_string(), instances: INSTANCES, inactivite: Duration::from_millis(1500), trace: sans_trace };
        crate::eco::some(Serveur::demarrer(config, g), "serveur de test")
    }

    fn defaut() -> Box<Gestionnaire> {
        Box::new(|id, appel, _| Some(if matches!(appel, Appel::Hello) { reponse_hello(id) } else { reponse_code(id, "essai", "x") }))
    }

    #[test]
    fn cible_selon_l_environnement() {
        let c = Cible::pour("spacenotch", "utilisateur", None);
        assert_eq!(c.nom_tube, r"\\.\pipe\Eco.spacenotch.utilisateur");
        assert!(matches!(&c.attendu, ExeAttendu::Manifeste(a) if a == "spacenotch"), "usage normal : l'exe du manifeste est exige");
        // Tube d'essai : aucun exe exigé (un faux serveur n'est pas l'exe du manifeste)
        let e = Cible::pour("spacenotch", "utilisateur", Some("t1"));
        assert_eq!(e.nom_tube, r"\\.\pipe\Eco.spacenotch.t1");
        assert!(matches!(e.attendu, ExeAttendu::Quelconque));
        assert!(matches!(Cible::pour("spacenotch", "utilisateur", Some("")).attendu, ExeAttendu::Manifeste(_)));
    }

    #[test]
    fn decision_de_fiabilite() {
        let bon = Profil { meme_sid: true, integrite: 0x2000, app_container: false, exe_ok: None };
        assert!(profil_fiable(&bon));
        assert!(profil_fiable(&Profil { integrite: 0x3000, exe_ok: Some(true), ..bon }), "haute intégrité, exe attendu");
        assert!(!profil_fiable(&Profil { meme_sid: false, ..bon }), "autre compte");
        assert!(!profil_fiable(&Profil { integrite: 0x1000, ..bon }), "intégrité basse");
        assert!(!profil_fiable(&Profil { integrite: 0, ..bon }));
        assert!(!profil_fiable(&Profil { app_container: true, ..bon }));
        assert!(!profil_fiable(&Profil { exe_ok: Some(false), ..bon }), "pas l'exe du manifeste");
    }

    /// Le manifeste n'est relu que s'il change (date ou taille), et la lecture est bornée
    #[test]
    fn manifeste_en_cache_invalide_par_la_date() {
        let d = std::env::temp_dir().join(format!("eco-man-{}", std::process::id()));
        crate::eco::ok(std::fs::create_dir_all(&d), "dossier");
        let f = d.join("x.json");
        crate::eco::ok(std::fs::write(&f, "{\"exe\":\"C:\\\\a.exe\"}"), "ecriture");
        for _ in 0..50 {
            assert_eq!(exe_du_fichier(&f), Some(r"C:\a.exe".to_string()));
        }
        // Preuve que le disque n'est pas relu : même taille, même date, autre contenu : c'est l'ancien qui est servi
        let date = crate::eco::ok(std::fs::metadata(&f).and_then(|m| m.modified()), "date");
        crate::eco::ok(std::fs::write(&f, "{\"exe\":\"C:\\\\b.exe\"}"), "meme taille");
        let h = crate::eco::ok(std::fs::OpenOptions::new().write(true).open(&f), "ouverture");
        crate::eco::ok(h.set_modified(date), "date rétablie");
        drop(h);
        assert_eq!(exe_du_fichier(&f), Some(r"C:\a.exe".to_string()), "servi par le cache, pas relu");
        // Modifié (taille différente) : relu
        crate::eco::ok(std::fs::write(&f, "{\"exe\":\"C:\\\\autre-programme.exe\"}"), "reecriture");
        assert_eq!(exe_du_fichier(&f), Some(r"C:\autre-programme.exe".to_string()));
        // Gonflé au-delà de la borne : refusé sans tout lire
        let mut gros = String::from("{\"exe\":\"C:\\\\a.exe\",\"x\":\"");
        gros.push_str(&"y".repeat(MANIFESTE_MAX as usize));
        gros.push_str("\"}");
        crate::eco::ok(std::fs::write(&f, gros), "gros");
        assert_eq!(exe_du_fichier(&f), None);
        assert_eq!(exe_du_fichier(&d.join("absent.json")), None);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn exe_lu_dans_le_manifeste() {
        assert_eq!(exe_depuis_manifeste("{\"id\":\"a\",\"exe\":\"C:\\\\x\\\\a.exe\"}"), Some(r"C:\x\a.exe".to_string()));
        assert_eq!(exe_depuis_manifeste("\u{feff}{\"exe\":\"C:\\\\a.exe\"}"), Some(r"C:\a.exe".to_string()), "BOM toléré");
        for t in ["", "{}", "{\"exe\":3}", "{\"exe\":\"\"}", "pas du json"] {
            assert_eq!(exe_depuis_manifeste(t), None, "{t}");
        }
        assert!(meme_exe(r"C:\Prog\A.exe", r"c:\prog\a.EXE"));
        assert!(meme_exe(r"C:\Prog\Été.exe", r"c:\prog\été.EXE"), "casse Unicode");
        assert!(!meme_exe(r"C:\Prog\A.exe", r"C:\Prog\B.exe"));
    }

    #[test]
    fn tube_absent_rend_absent_vite() {
        let nom = nom_unique("absent");
        let debut = Instant::now();
        assert_eq!(appeler(&Cible::avec(&nom, ExeAttendu::Quelconque), "hello", "{}", BUDGET_DEFAUT), Issue::Absent);
        assert_eq!(emettre(&Cible::avec(&nom, ExeAttendu::Quelconque), "progress", "{}", BUDGET_DEFAUT), Err(Issue::Absent));
        assert!(!present(&nom));
        assert!(debut.elapsed() < BUDGET_DEFAUT, "Absent doit etre immediat : {:?}", debut.elapsed());
    }

    #[test]
    fn appel_hello_et_cache() {
        let nom = nom_unique("hello");
        let appels = Arc::new(AtomicUsize::new(0));
        let compte = appels.clone();
        let s = serveur(
            &nom,
            Box::new(move |id, appel, _| {
                compte.fetch_add(1, Ordering::SeqCst);
                Some(if matches!(appel, Appel::Hello) { reponse_hello(id) } else { reponse_code(id, "essai", "x") })
            }),
        );
        assert!(present(&nom));
        let Issue::Ok(r) = appeler(&Cible::avec(&nom, ExeAttendu::Quelconque), "hello", "{}", BUDGET_DEFAUT) else { panic!("hello refuse") };
        assert_eq!(champ(&r.resultat, "app"), Some(&Json::Texte("spacefiles".into())));
        // Deuxième hello : réponse gardée, le serveur n'est pas rappelé
        assert!(matches!(appeler(&Cible::avec(&nom, ExeAttendu::Quelconque), "hello", "{}", BUDGET_DEFAUT), Issue::Ok(_)));
        assert_eq!(appels.load(Ordering::SeqCst), 1, "le hello doit etre en cache");
        // Un autre appel n'est jamais en cache
        assert_eq!(appeler(&Cible::avec(&nom, ExeAttendu::Quelconque), "search.query", "{\"q\":\"abc\"}", BUDGET_DEFAUT), Issue::Erreur { code: "essai".into() });
        assert_eq!(appels.load(Ordering::SeqCst), 2);
        assert!(s.arreter());
    }

    #[test]
    fn serveur_d_un_autre_exe_est_non_fiable_et_rien_n_est_envoye() {
        let nom = nom_unique("exe");
        let recu = Arc::new(AtomicUsize::new(0));
        let r = recu.clone();
        let s = serveur(
            &nom,
            Box::new(move |id, _, _| {
                r.fetch_add(1, Ordering::SeqCst);
                Some(reponse_hello(id))
            }),
        );
        let ici = crate::eco::ok(std::env::current_exe(), "exe courant").to_string_lossy().into_owned();
        assert!(matches!(appeler(&Cible::avec(&nom, ExeAttendu::Chemin(ici.to_uppercase())), "x.y", "{}", BUDGET_DEFAUT), Issue::Erreur { .. } | Issue::Ok(_)), "notre propre exe est accepte");
        let avant = recu.load(Ordering::SeqCst);
        assert_eq!(appeler(&Cible::avec(&nom, ExeAttendu::Chemin(r"C:\autre\programme.exe".into())), "hello", "{}", BUDGET_DEFAUT), Issue::NonFiable);
        assert_eq!(emettre(&Cible::avec(&nom, ExeAttendu::Chemin(r"C:\autre\programme.exe".into())), "progress", "{}", BUDGET_DEFAUT), Err(Issue::NonFiable));
        assert_eq!(recu.load(Ordering::SeqCst), avant, "rien ne doit etre envoye a un serveur non fiable");
        // Manifeste exigé mais absent du dossier Eco d'essai : non fiable aussi
        assert_eq!(appeler(&Cible::avec(&nom, ExeAttendu::Manifeste("application-sans-manifeste-zz".into())), "x.y", "{}", BUDGET_DEFAUT), Issue::NonFiable);
        assert!(s.arreter());
    }

    #[test]
    fn serveur_lent_rend_delai_dans_le_budget() {
        let nom = nom_unique("lent");
        let s = serveur(
            &nom,
            Box::new(|id, _, _| {
                std::thread::sleep(Duration::from_millis(900));
                Some(reponse_hello(id))
            }),
        );
        let debut = Instant::now();
        assert_eq!(appeler(&Cible::avec(&nom, ExeAttendu::Quelconque), "search.query", "{\"q\":\"abc\"}", BUDGET_DEFAUT), Issue::Delai);
        let ecoule = debut.elapsed();
        assert!(ecoule >= Duration::from_millis(250) && ecoule < Duration::from_millis(600), "{ecoule:?}");
        assert!(s.arreter());
    }

    #[test]
    fn toutes_les_instances_occupees_donne_delai() {
        let nom = nom_unique("plein");
        let s = serveur(&nom, defaut());
        // Autant de clients muets que d'instances : un cinquième appel ne trouve rien de libre
        let muets: Vec<Tube> = (0..INSTANCES).map(|_| crate::eco::ok(Tube::ouvrir(&nom, Instant::now() + Duration::from_secs(2)), "client muet")).collect();
        let debut = Instant::now();
        assert_eq!(appeler(&Cible::avec(&nom, ExeAttendu::Quelconque), "x.y", "{}", Duration::from_millis(200)), Issue::Delai);
        assert!(debut.elapsed() < Duration::from_millis(500), "{:?}", debut.elapsed());
        drop(muets);
        assert!(s.arreter());
    }

    #[test]
    fn reponse_d_un_autre_identifiant_ou_illisible_est_protocole() {
        // Faux serveur de test : même serveur, mais le gestionnaire répond avec un autre identifiant
        let nom = nom_unique("id");
        let s = serveur(&nom, Box::new(|_, _, _| Some(reponse_hello("autre-identifiant"))));
        assert_eq!(appeler(&Cible::avec(&nom, ExeAttendu::Quelconque), "search.query", "{\"q\":\"abc\"}", BUDGET_DEFAUT), Issue::Protocole);
        assert!(s.arreter());
        let nom = nom_unique("illisible");
        let s = serveur(&nom, Box::new(|_, _, _| Some("ceci n'est pas du json".to_string())));
        assert_eq!(appeler(&Cible::avec(&nom, ExeAttendu::Quelconque), "search.query", "{\"q\":\"abc\"}", BUDGET_DEFAUT), Issue::Protocole);
        assert!(s.arreter());
    }

    /// Un `hello` qui annonce un protocole plus récent que le nôtre : l'appelant se dégrade (`Protocole`), sans plantage ni cache
    #[test]
    fn hello_d_un_protocole_plus_recent() {
        let nom = nom_unique("v2");
        let appels = Arc::new(AtomicUsize::new(0));
        let c = appels.clone();
        let s = serveur(
            &nom,
            Box::new(move |id, _, _| {
                c.fetch_add(1, Ordering::SeqCst);
                Some(reponse_hello(id).replace("\"protocol\":1", "\"protocol\":2").replace("\"capabilities\":[", "\"capabilities\":[\"futur.chose\","))
            }),
        );
        assert_eq!(appeler(&Cible::avec(&nom, ExeAttendu::Quelconque), "hello", "{}", BUDGET_DEFAUT), Issue::Protocole);
        assert_eq!(appeler(&Cible::avec(&nom, ExeAttendu::Quelconque), "hello", "{}", BUDGET_DEFAUT), Issue::Protocole);
        assert_eq!(appels.load(Ordering::SeqCst), 2, "un hello refuse n'est pas garde");
        assert!(s.arreter());
        // Et les autres cas : protocol absent, texte, zéro
        for corps in ["{\"app\":\"x\"}", "{\"protocol\":\"1\"}", "{\"protocol\":0}"] {
            let r = Reponse { resultat: match crate::eco::json::analyser_json(corps) { Some(Json::Objet(o)) => o, _ => Vec::new() } };
            assert_eq!(controler_hello(r), Issue::Protocole, "{corps}");
        }
    }

    #[test]
    fn serveur_arrete_en_pleine_vie_rend_absent() {
        let nom = nom_unique("arret");
        let s = serveur(&nom, defaut());
        assert!(matches!(appeler(&Cible::avec(&nom, ExeAttendu::Quelconque), "x.y", "{}", BUDGET_DEFAUT), Issue::Erreur { .. }));
        assert!(s.arreter());
        let debut = Instant::now();
        assert_eq!(appeler(&Cible::avec(&nom, ExeAttendu::Quelconque), "x.y", "{}", BUDGET_DEFAUT), Issue::Absent);
        assert!(debut.elapsed() < BUDGET_DEFAUT);
    }
}
