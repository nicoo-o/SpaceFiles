// Serveur de tube nommé multi-instances (protocole Eco v1). Plusieurs instances du même tube attendent en même temps,
// chacune servie par son propre fil bloquant : un client lent ou muet n'occupe qu'une instance, les autres restent
// disponibles. Rien ne tourne à vide : un fil attend dans `ConnectNamedPipe` (ou `ReadFile`) tant que personne n'appelle.
//
// Choix « un fil par instance » plutôt qu'entrées-sorties recouvertes sur un seul fil : le code est celui du lot 3 (déjà
// éprouvé : `ERROR_NO_DATA`, client muet, annulation), répété N fois ; N = 4 fils au repos (contre 1), pour quelques
// dizaines de Ko de pile engagée chacun. Un veilleur (fil qui vit le temps d'une connexion) coupe un client muet.
//
// Ce module ne connaît aucun appel : il lit des lignes, applique les bornes (`trame.rs`), appelle le gestionnaire
// et écrit sa réponse. Le gestionnaire (recherche, ouverture de dossier) est fourni par `eco/mod.rs`.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use windows::core::{PCWSTR, PWSTR};
use windows::Win32::Foundation::*;
use windows::Win32::Security::Authorization::{ConvertSidToStringSidW, ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1};
use windows::Win32::Security::{GetTokenInformation, TokenUser, PSECURITY_DESCRIPTOR, SECURITY_ATTRIBUTES, TOKEN_QUERY, TOKEN_USER};
use windows::Win32::Storage::FileSystem::{ReadFile, WriteFile, FILE_FLAG_FIRST_PIPE_INSTANCE, PIPE_ACCESS_DUPLEX};
use windows::Win32::System::Pipes::*;
use windows::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};
use windows::Win32::System::IO::CancelIoEx;

use super::trame::{analyser_trame, ligne_trop_longue, reponse_erreur, Appel, Erreur, MAX_OUVRIR};
use crate::journal::Niveau;

/// Instances en attente : au moins 4, pour que quelques clients (SpaceNotch, Hub, un essai) ne se bloquent jamais
pub const INSTANCES: usize = 4;
/// Un client connecté qui n'envoie rien est déconnecté après ce délai
const INACTIVITE_DEFAUT_MS: u64 = 10_000;

fn delai_inactivite_depuis(env: Option<&str>) -> u64 {
    env.and_then(|s| s.trim().parse::<u64>().ok()).filter(|ms| (100..=600_000).contains(ms)).unwrap_or(INACTIVITE_DEFAUT_MS)
}

/// 10 s, ou `SPACEFILES_INACTIVITE_MS` (essais seulement, absente en usage normal)
pub fn delai_inactivite() -> Duration {
    Duration::from_millis(delai_inactivite_depuis(std::env::var("SPACEFILES_INACTIVITE_MS").ok().as_deref()))
}

/// Ce que le gestionnaire sait de la connexion en cours
pub struct Connexion<'a> {
    h: HANDLE,
    arret: &'a AtomicBool,
    /// Une requête plus récente est déjà complète dans le tampon (la recherche en cours devient inutile)
    pub suivante_prete: bool,
}

impl Connexion<'_> {
    /// Le serveur s'arrête
    pub fn arret_demande(&self) -> bool {
        self.arret.load(Ordering::Relaxed)
    }

    /// Octets déjà arrivés dans le tube (une nouvelle requête attend), ou client parti
    pub fn requete_en_attente(&self) -> bool {
        octets_en_attente(self.h).map_or(true, |n| n > 0)
    }
}

/// Répond à un appel : `Some(ligne)` à écrire, ou `None` pour l'abandonner sans réponse (recherche dépassée)
pub type Gestionnaire = dyn Fn(&str, &Appel, &Connexion) -> Option<String> + Send + Sync;

pub struct Config {
    /// `\\.\pipe\Eco.<app>.<utilisateur>`
    pub nom_tube: String,
    pub instances: usize,
    pub inactivite: Duration,
    /// États seulement (jamais le contenu d'un appel) : en test, rien n'est écrit dans le vrai journal
    pub trace: fn(Niveau, &str),
}

struct Etat {
    arret: AtomicBool,
    /// Handle de chaque instance, 0 une fois fermée. Fermé et annulé sous ce verrou : jamais d'annulation sur un handle réutilisé.
    tubes: Mutex<Vec<isize>>,
    config: Config,
    gestionnaire: Box<Gestionnaire>,
}

pub struct Serveur {
    etat: Arc<Etat>,
    fils: Vec<JoinHandle<()>>,
}

/// Ce que `servir_lignes` partage avec le veilleur de la connexion
struct Veille {
    envoi: Sender<()>,
    occupe: Arc<AtomicBool>,
    inactif: Arc<AtomicBool>,
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// SDDL « seul l'utilisateur du jeton peut tout faire sur ce tube » (D:P : pas d'héritage). La DACL par défaut d'un tube
/// donnerait aussi la lecture à Everyone et à l'accès anonyme : elle n'est pas assez stricte. `S:(ML;;NWNR;;;ME)` : étiquette
/// d'intégrité moyenne, un processus d'intégrité basse (bac à sable) ne peut ni lire ni écrire le tube. Un masque d'accès
/// restreint (`0x12019b`, sans FILE_CREATE_PIPE_INSTANCE) a été essayé et abandonné : il interdit aussi au résident de
/// créer ses instances 2 à 4 (la DACL du tube existant s'applique à lui), seule la première instance se crée.
fn sddl_utilisateur() -> Option<String> {
    // SAFETY : tampon de la taille demandée par GetTokenInformation ; le handle du jeton est fermé avant de sortir ;
    // la chaîne du SID est copiée puis libérée avec LocalFree (elle est allouée par ConvertSidToStringSidW).
    unsafe {
        let mut jeton = HANDLE::default();
        OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut jeton).ok()?;
        let mut taille = 0u32;
        let _ = GetTokenInformation(jeton, TokenUser, None, 0, &mut taille);
        // Alignement 8 : TOKEN_USER contient des pointeurs
        let mut tampon = vec![0u64; (taille as usize).div_ceil(8).max(1)];
        let ok = GetTokenInformation(jeton, TokenUser, Some(tampon.as_mut_ptr() as *mut _), (tampon.len() * 8) as u32, &mut taille).is_ok();
        let _ = CloseHandle(jeton);
        if !ok {
            return None;
        }
        let utilisateur = &*(tampon.as_ptr() as *const TOKEN_USER);
        let mut texte = PWSTR::null();
        ConvertSidToStringSidW(utilisateur.User.Sid, &mut texte).ok()?;
        let sid = texte.to_string().ok();
        let _ = LocalFree(HLOCAL(texte.0 as *mut _));
        sid.map(|s| format!("D:P(A;;GA;;;{s})S:(ML;;NWNR;;;ME)"))
    }
}

/// Crée une instance du tube. `premiere` : `FILE_FLAG_FIRST_PIPE_INSTANCE`, donc si un autre processus tient déjà ce nom
/// on renonce (ni partage ni usurpation) ; les suivantes rejoignent le nom que nous tenons déjà. Refuse les clients
/// distants. None si une étape échoue.
fn creer_instance(nom: &[u16], sddl: &[u16], premiere: bool, max: u32) -> Option<HANDLE> {
    // SAFETY : `sddl` et `nom` sont des chaînes UTF-16 terminées par 0 qui vivent pendant l'appel ; `sd` est écrit par
    // Windows (alloué avec LocalAlloc) et libéré par LocalFree après CreateNamedPipeW, qui en garde sa propre copie ;
    // `attributs` vit jusqu'à la fin de l'appel et son `nLength` est exact.
    unsafe {
        let mut sd = PSECURITY_DESCRIPTOR::default();
        ConvertStringSecurityDescriptorToSecurityDescriptorW(PCWSTR(sddl.as_ptr()), SDDL_REVISION_1, &mut sd, None).ok()?;
        let attributs = SECURITY_ATTRIBUTES { nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32, lpSecurityDescriptor: sd.0, bInheritHandle: false.into() };
        let ouverture = if premiere { PIPE_ACCESS_DUPLEX | FILE_FLAG_FIRST_PIPE_INSTANCE } else { PIPE_ACCESS_DUPLEX };
        let h = CreateNamedPipeW(
            PCWSTR(nom.as_ptr()),
            ouverture,
            PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT | PIPE_REJECT_REMOTE_CLIENTS,
            max,
            16 * 1024,
            16 * 1024,
            0,
            Some(&attributs),
        );
        let _ = LocalFree(HLOCAL(sd.0));
        (!h.is_invalid()).then_some(h)
    }
}

/// Tests : une instance unique du tube (avec la même DACL), pour les faux serveurs des autres modules
#[cfg(test)]
pub(super) fn instance_de_test(nom: &str) -> Option<HANDLE> {
    creer_instance(&wide(nom), &wide(&sddl_utilisateur()?), true, 1)
}

fn ecrire_ligne(h: HANDLE, ligne: &str) -> bool {
    let mut octets = ligne.as_bytes().to_vec();
    octets.push(b'\n');
    let mut reste = &octets[..];
    while !reste.is_empty() {
        let mut ecrits = 0u32;
        // SAFETY : `reste` est un tampon valide pour sa longueur ; `h` est l'instance du fil courant, ouverte pour toute la connexion.
        if unsafe { WriteFile(h, Some(reste), Some(&mut ecrits), None) }.is_err() || ecrits == 0 {
            return false;
        }
        reste = &reste[ecrits as usize..];
    }
    true
}

/// Octets déjà arrivés dans le tube (une nouvelle requête attend) ; None si le client est parti
fn octets_en_attente(h: HANDLE) -> Option<u32> {
    let mut dispo = 0u32;
    // SAFETY : aucun tampon de lecture (taille 0) ; `dispo` est valide pendant l'appel.
    unsafe { PeekNamedPipe(h, None, 0, None, Some(&mut dispo), None) }.ok().map(|_| dispo)
}

impl Etat {
    fn trace(&self, niveau: Niveau, message: &str) {
        (self.config.trace)(niveau, message);
    }

    /// Sert un client connecté jusqu'à sa déconnexion ou jusqu'à `inactivite` de silence. Un veilleur (fil qui vit le
    /// temps de la connexion, bloqué sur un délai : rien ne tourne à vide) annule la lecture bloquée si rien n'arrive.
    fn servir(&self, h: HANDLE) {
        let delai = self.config.inactivite;
        let inactif = Arc::new(AtomicBool::new(false));
        // Vrai tant que le gestionnaire travaille : une recherche longue n'est pas du silence du client
        let occupe = Arc::new(AtomicBool::new(false));
        let (envoi, reception) = channel::<()>();
        let valeur = h.0 as isize;
        let (drapeau, travaille) = (inactif.clone(), occupe.clone());
        let veilleur = std::thread::Builder::new().name("eco-veille".into()).spawn(move || loop {
            match reception.recv_timeout(delai) {
                Ok(()) => {}
                Err(RecvTimeoutError::Timeout) if travaille.load(Ordering::SeqCst) => {}
                Err(RecvTimeoutError::Timeout) => {
                    drapeau.store(true, Ordering::SeqCst);
                    // SAFETY : `servir` attend ce fil (join) avant de rendre le handle : il est ouvert tant que ce fil tourne.
                    // CancelIoEx ne fait échouer que la lecture en attente ; si le service est occupé ailleurs, il n'y a
                    // rien à annuler et le prochain tour du délai réessaie.
                    unsafe {
                        let _ = CancelIoEx(HANDLE(valeur as *mut _), None);
                    }
                }
                Err(RecvTimeoutError::Disconnected) => break,
            }
        });
        // Sans veilleur, un client muet tiendrait l'instance sans fin : la connexion est fermée tout de suite
        let Ok(veilleur) = veilleur else {
            self.trace(Niveau::Avert, "veilleur impossible a creer, connexion fermee");
            return;
        };
        self.servir_lignes(h, &Veille { envoi, occupe, inactif: inactif.clone() });
        let _ = veilleur.join();
        if inactif.load(Ordering::SeqCst) {
            self.trace(Niveau::Avert, "client inactif deconnecte");
        }
    }

    fn servir_lignes(&self, h: HANDLE, veille: &Veille) {
        let mut tampon: Vec<u8> = Vec::new();
        let mut bloc = [0u8; 4096];
        loop {
            if self.arret.load(Ordering::SeqCst) {
                return;
            }
            if let Some(fin) = tampon.iter().position(|&c| c == b'\n') {
                let ligne: Vec<u8> = tampon.drain(..=fin).collect();
                let ligne = &ligne[..ligne.len() - 1];
                if ligne_trop_longue(ligne) {
                    self.refuser_trop_long(h);
                    return;
                }
                let Ok(texte) = std::str::from_utf8(ligne) else {
                    if !ecrire_ligne(h, &reponse_erreur("", &Erreur::Invalide)) {
                        return;
                    }
                    continue;
                };
                let (id, appel) = analyser_trame(texte);
                let reponse = match appel {
                    // Entre 4 096 octets et 32 Ko, seul `explorer.open` passe ; les autres appels sont refusés puis la
                    // connexion est fermée, comme pour une ligne au-delà de 32 Ko
                    Err(Erreur::TropLong) => {
                        self.refuser_trop_long(h);
                        return;
                    }
                    Ok(a) => {
                        let c = Connexion { h, arret: &self.arret, suivante_prete: tampon.contains(&b'\n') };
                        veille.inactif.store(false, Ordering::SeqCst);
                        veille.occupe.store(true, Ordering::SeqCst);
                        let r = (self.gestionnaire)(&id, &a, &c);
                        veille.occupe.store(false, Ordering::SeqCst);
                        r
                    }
                    Err(e) => Some(reponse_erreur(&id, &e)),
                };
                if let Some(r) = reponse {
                    if !ecrire_ligne(h, &r) {
                        return;
                    }
                }
                let _ = veille.envoi.send(());
                continue;
            }
            // + 1 : le `\r` d'une ligne pile à la borne peut être arrivé avant son `\n`. La borne est celle de
            // `explorer.open` : le nom de l'appel n'est connu qu'une fois la ligne complète.
            if tampon.len() > MAX_OUVRIR + 1 {
                self.refuser_trop_long(h);
                return;
            }
            let mut lus = 0u32;
            // SAFETY : `bloc` est un tampon valide pour sa longueur ; `h` reste ouvert tant que ce fil sert (il est fermé par lui seul).
            if unsafe { ReadFile(h, Some(&mut bloc), Some(&mut lus), None) }.is_err() || lus == 0 {
                return;
            }
            tampon.extend_from_slice(&bloc[..lus as usize]);
            let _ = veille.envoi.send(());
        }
    }

    /// Erreur « trop long » puis fermeture, comme le protocole le dit : le client a le temps (2 s) de lire la réponse avant la coupure
    fn refuser_trop_long(&self, h: HANDLE) {
        self.trace(Niveau::Avert, "trame trop longue refusee, connexion fermee");
        if !ecrire_ligne(h, &reponse_erreur("", &Erreur::TropLong)) {
            return;
        }
        let fin = Instant::now() + Duration::from_secs(2);
        // Le client lit puis ferme : le tube se brise. Sinon on coupe quand même après 2 s.
        while Instant::now() < fin && !self.arret.load(Ordering::SeqCst) {
            let mut jeter = [0u8; 1024];
            let mut lus = 0u32;
            match octets_en_attente(h) {
                None => return,
                // SAFETY : tampon valide pour sa longueur ; les octets lus sont ignorés.
                Some(n) if n > 0 => {
                    let _ = unsafe { ReadFile(h, Some(&mut jeter), Some(&mut lus), None) };
                }
                Some(_) => std::thread::sleep(Duration::from_millis(20)),
            }
        }
    }

    /// Boucle d'une instance : attendre un client, le servir, libérer l'instance pour le suivant
    fn boucle(&self, h: HANDLE) {
        while !self.arret.load(Ordering::SeqCst) {
            // SAFETY : `h` est une instance créée par `demarrer`, ouverte jusqu'à la fin de cette fonction (le fil la ferme).
            let (connecte, parti) = match unsafe { ConnectNamedPipe(h, None) } {
                Ok(()) => (true, false),
                // Le client s'est connecté entre la création (ou la déconnexion) et cet appel : c'est un succès.
                // ERROR_NO_DATA : il s'est connecté puis a fermé avant que nous le servions.
                Err(e) => (e.code() == ERROR_PIPE_CONNECTED.to_hresult(), e.code() == ERROR_NO_DATA.to_hresult()),
            };
            if self.arret.load(Ordering::SeqCst) {
                break;
            }
            if !connecte {
                // Jamais d'arrêt définitif : on libère l'instance et on se remet en attente (une pause évite de tourner
                // à vide si l'erreur persiste ; pas de pause pour un client qui est simplement parti)
                // SAFETY : même handle ; l'instance redevient disponible pour le client suivant.
                unsafe {
                    let _ = DisconnectNamedPipe(h);
                }
                if !parti {
                    std::thread::sleep(Duration::from_millis(200));
                }
                continue;
            }
            self.servir(h);
            // SAFETY : même handle ; la déconnexion libère l'instance pour le client suivant.
            unsafe {
                let _ = DisconnectNamedPipe(h);
            }
        }
    }
}

impl Serveur {
    /// Crée les instances (la première avec `FILE_FLAG_FIRST_PIPE_INSTANCE`) puis un fil par instance. None si le nom est
    /// déjà pris ou si le jeton est illisible : le journal le dit, le résident continue sans service.
    pub fn demarrer(config: Config, gestionnaire: Box<Gestionnaire>) -> Option<Serveur> {
        let Some(sddl) = sddl_utilisateur() else {
            (config.trace)(Niveau::Erreur, "jeton illisible : service non demarre");
            return None;
        };
        let nom = wide(&config.nom_tube);
        let sddl = wide(&sddl);
        let n = config.instances.clamp(1, 16);
        let Some(premiere) = creer_instance(&nom, &sddl, true, n as u32) else {
            (config.trace)(Niveau::Erreur, "creation du tube refusee (nom deja pris ?) : service non demarre");
            return None;
        };
        let mut handles = vec![premiere];
        for _ in 1..n {
            match creer_instance(&nom, &sddl, false, n as u32) {
                Some(h) => handles.push(h),
                None => {
                    (config.trace)(Niveau::Avert, "instance supplementaire du tube refusee");
                    break;
                }
            }
        }
        let etat = Arc::new(Etat { arret: AtomicBool::new(false), tubes: Mutex::new(handles.iter().map(|h| h.0 as isize).collect()), config, gestionnaire });
        let mut fils = Vec::new();
        for (i, h) in handles.iter().enumerate() {
            // Un HANDLE ne traverse pas un fil (pointeur brut) : on passe sa valeur
            let valeur = h.0 as isize;
            let e = etat.clone();
            let fil = std::thread::Builder::new().name(format!("eco-serveur-{i}")).spawn(move || {
                let h = HANDLE(valeur as *mut _);
                e.boucle(h);
                // Fermé sous le verrou, après avoir remis 0 : `arreter` n'annule jamais un handle fermé
                let mut t = e.tubes.lock().unwrap_or_else(|x| x.into_inner());
                t[i] = 0;
                // SAFETY : instance créée par `demarrer`, plus utilisée après cette ligne.
                unsafe {
                    let _ = CloseHandle(h);
                }
            });
            match fil {
                Ok(f) => fils.push(f),
                Err(_) => {
                    let mut t = etat.tubes.lock().unwrap_or_else(|x| x.into_inner());
                    // SAFETY : le fil n'a pas démarré, ce handle n'a pas d'autre propriétaire.
                    unsafe {
                        let _ = CloseHandle(*h);
                    }
                    t[i] = 0;
                    etat.trace(Niveau::Erreur, "fil d'une instance non demarre");
                }
            }
        }
        if fils.is_empty() {
            return None;
        }
        etat.trace(Niveau::Info, &format!("service demarre : {} instances (nom du tube d'apres USERNAME, qui peut differer du SID)", fils.len()));
        Some(Serveur { etat, fils })
    }

    /// Nombre de fils d'instance vivants (tous bloqués en attente au repos)
    #[cfg(test)]
    pub fn fils_vivants(&self) -> usize {
        self.fils.iter().filter(|f| !f.is_finished()).count()
    }

    /// Arrête le service : annule l'attente de chaque fil puis les attend (2 s au plus). Vrai si aucun fil n'est resté.
    pub fn arreter(self) -> bool {
        self.etat.arret.store(true, Ordering::SeqCst);
        for _ in 0..40 {
            if self.fils.iter().all(|f| f.is_finished()) {
                break;
            }
            {
                let t = self.etat.tubes.lock().unwrap_or_else(|e| e.into_inner());
                for &h in t.iter().filter(|&&h| h != 0) {
                    // SAFETY : le handle est valide sous ce verrou (le fil ne le ferme qu'en le prenant). CancelIoEx réveille
                    // le ConnectNamedPipe ou le ReadFile bloqué ; renouvelé car le fil peut ne pas encore être bloqué.
                    unsafe {
                        let _ = CancelIoEx(HANDLE(h as *mut _), None);
                    }
                }
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        let tous = self.fils.iter().all(|f| f.is_finished());
        let restants = self.fils.iter().filter(|f| !f.is_finished()).count();
        for f in self.fils {
            if f.is_finished() {
                let _ = f.join();
            }
        }
        if tous {
            self.etat.trace(Niveau::Info, "service arrete");
        } else {
            self.etat.trace(Niveau::Avert, &format!("{restants} fil(s) du service ne se sont pas arretes en 2 s"));
        }
        tous
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::eco::trame::{reponse_code, reponse_hello, reponse_ouvert};
    use std::io::{BufRead, BufReader, Write};
    use std::sync::atomic::AtomicUsize;

    fn sans_trace(_: Niveau, _: &str) {}

    fn nom_unique(s: &str) -> String {
        static N: AtomicUsize = AtomicUsize::new(0);
        format!(r"\\.\pipe\Eco.spacefiles.test-{}-{}-{s}", std::process::id(), N.fetch_add(1, Ordering::SeqCst))
    }

    fn demarrer(nom: &str, inactivite_ms: u64) -> Serveur {
        let config = Config { nom_tube: nom.to_string(), instances: INSTANCES, inactivite: Duration::from_millis(inactivite_ms), trace: sans_trace };
        let g: Box<Gestionnaire> = Box::new(|id, appel, _| {
            Some(match appel {
                Appel::Hello => reponse_hello(id),
                Appel::Ouvrir { chemin, .. } => reponse_code(id, "ouvert", &format!("{}", chemin.len())),
                Appel::Recherche { .. } => reponse_ouvert(id),
            })
        });
        crate::eco::some(Serveur::demarrer(config, g), "serveur de test")
    }

    /// Client synchrone du test : se connecte (attend une instance libre), envoie une ligne, lit une ligne
    struct Client(BufReader<std::fs::File>);

    impl Client {
        fn connecter(nom: &str) -> Client {
            let fin = Instant::now() + Duration::from_secs(5);
            loop {
                match std::fs::OpenOptions::new().read(true).write(true).open(nom) {
                    Ok(f) => return Client(BufReader::new(f)),
                    // ERROR_PIPE_BUSY (231) : toutes les instances sont occupées, une se libérera
                    Err(e) if e.raw_os_error() == Some(231) && Instant::now() < fin => std::thread::sleep(Duration::from_millis(5)),
                    Err(e) => panic!("connexion impossible : {e}"),
                }
            }
        }

        fn appeler(&mut self, ligne: &str) -> String {
            crate::eco::ok(self.0.get_mut().write_all(format!("{ligne}\n").as_bytes()), "ecriture");
            let mut r = String::new();
            crate::eco::ok(self.0.read_line(&mut r), "lecture");
            r
        }
    }

    const HELLO: &str = "{\"v\":1,\"id\":\"h\",\"kind\":\"call\",\"name\":\"hello\"}";

    #[test]
    fn serveur_repond_puis_s_arrete_sans_fil_orphelin() {
        let nom = nom_unique("a");
        let s = demarrer(&nom, 5_000);
        assert_eq!(s.fils_vivants(), INSTANCES);
        let mut c = Client::connecter(&nom);
        let r = c.appeler(HELLO);
        assert!(r.contains("\"app\":\"spacefiles\"") && r.contains("explorer.open"), "{r}");
        // Même connexion : une seconde requête (la connexion reste ouverte)
        assert!(c.appeler(HELLO).contains("\"id\":\"h\""));
        let debut = Instant::now();
        // Le client est encore connecté : l'arrêt doit quand même aboutir
        assert!(s.arreter(), "un fil est reste");
        assert!(debut.elapsed() < Duration::from_secs(3));
        assert!(std::fs::OpenOptions::new().read(true).write(true).open(&nom).is_err(), "le tube doit avoir disparu");
    }

    #[test]
    fn un_client_muet_ne_bloque_pas_les_autres() {
        let _serie = SERIE.lock().unwrap_or_else(|e| e.into_inner());
        let nom = nom_unique("b");
        let s = demarrer(&nom, 1_500);
        let mut muet = Client::connecter(&nom);
        let debut = Instant::now();
        // Trois autres clients, en même temps : tous servis pendant que le muet occupe une instance
        let fils: Vec<_> = (0..3)
            .map(|_| {
                let n = nom.clone();
                std::thread::spawn(move || {
                    let mut c = Client::connecter(&n);
                    (0..5).all(|_| c.appeler(HELLO).contains("\"id\":\"h\""))
                })
            })
            .collect();
        for f in fils {
            assert_eq!(f.join().ok(), Some(true), "un client n'a pas ete servi");
        }
        assert!(debut.elapsed() < Duration::from_millis(1_200), "les clients ont attendu le muet : {:?}", debut.elapsed());
        // Le muet est coupé après le délai d'inactivité : sa lecture rend la fin de flux
        let mut ligne = String::new();
        let lu = muet.0.read_line(&mut ligne);
        assert!(matches!(lu, Ok(0) | Err(_)), "le client muet n'a pas ete coupe : {lu:?}");
        assert!(debut.elapsed() >= Duration::from_millis(1_400), "coupe trop tot : {:?}", debut.elapsed());
        assert!(s.arreter());
    }

    static NOTES: Mutex<Vec<String>> = Mutex::new(Vec::new());
    /// Deux tests regardent le journal du serveur sur les coupures pour inactivité : jamais en même temps
    static SERIE: Mutex<()> = Mutex::new(());

    fn noter(_: Niveau, message: &str) {
        NOTES.lock().unwrap_or_else(|e| e.into_inner()).push(message.to_string());
    }

    /// M5 : un gestionnaire lent n'est pas un client inactif (ni coupure ni trace « inactif »), et le client est ensuite servi
    #[test]
    fn un_gestionnaire_lent_n_est_pas_du_silence() {
        let _serie = SERIE.lock().unwrap_or_else(|e| e.into_inner());
        NOTES.lock().unwrap_or_else(|e| e.into_inner()).clear();
        let nom = nom_unique("lent");
        let config = Config { nom_tube: nom.clone(), instances: INSTANCES, inactivite: Duration::from_millis(400), trace: noter };
        let g: Box<Gestionnaire> = Box::new(|id, appel, _| {
            if matches!(appel, Appel::Recherche { .. }) {
                std::thread::sleep(Duration::from_millis(1_300));
            }
            Some(reponse_hello(id))
        });
        let s = crate::eco::some(Serveur::demarrer(config, g), "serveur de test");
        let mut c = Client::connecter(&nom);
        let r = c.appeler("{\"id\":\"l\",\"name\":\"search.query\",\"args\":{\"q\":\"abc\"}}");
        assert!(r.contains("\"id\":\"l\""), "reponse perdue : {r}");
        assert!(c.appeler(HELLO).contains("\"id\":\"h\""), "client coupe apres un traitement long");
        drop(c);
        // Laisse `servir` finir et journaliser
        std::thread::sleep(Duration::from_millis(300));
        assert!(s.arreter());
        let notes = NOTES.lock().unwrap_or_else(|e| e.into_inner()).clone();
        assert!(!notes.iter().any(|n| n.contains("inactif")), "un traitement long a ete pris pour du silence : {notes:?}");
    }

    #[test]
    fn clients_qui_partent_aussitot_n_arretent_pas_le_service() {
        let nom = nom_unique("c");
        let s = demarrer(&nom, 5_000);
        // ERROR_NO_DATA côté serveur : ces clients se connectent et ferment avant d'être servis
        for _ in 0..40 {
            drop(Client::connecter(&nom));
        }
        let mut c = Client::connecter(&nom);
        assert!(c.appeler(HELLO).contains("\"id\":\"h\""));
        assert_eq!(s.fils_vivants(), INSTANCES);
        assert!(s.arreter());
    }

    #[test]
    fn plus_de_4096_octets_sauf_explorer_open() {
        let nom = nom_unique("d");
        let s = demarrer(&nom, 5_000);
        // search.query de 5 000 octets : trop_long, puis la connexion est fermée
        let mut c = Client::connecter(&nom);
        let r = c.appeler(&format!("{{\"id\":\"x\",\"name\":\"search.query\",\"args\":{{\"q\":\"{}\"}}}}", "a".repeat(5_000)));
        assert!(r.contains("\"code\":\"trop_long\""), "{r}");
        let mut suite = String::new();
        assert!(matches!(c.0.read_line(&mut suite), Ok(0) | Err(_)), "la connexion devait etre fermee");
        // explorer.open de 20 000 octets : transmis au gestionnaire (qui répond ici la longueur du chemin)
        let mut c = Client::connecter(&nom);
        let r = c.appeler(&format!("{{\"id\":\"o\",\"name\":\"explorer.open\",\"args\":{{\"path\":\"C:\\\\{}\"}}}}", "d".repeat(20_000)));
        assert!(r.contains("\"code\":\"ouvert\"") && r.contains("\"message\":\"20003\""), "{}", &r[..r.len().min(200)]);
        // Au-delà de 32 Ko, même lui est refusé (lu jusqu'à 32 Ko puis coupé, la réponse peut arriver ou la connexion se briser)
        let mut c = Client::connecter(&nom);
        let enorme = format!("{{\"id\":\"o\",\"name\":\"explorer.open\",\"args\":{{\"path\":\"C:\\\\{}\"}}}}\n", "d".repeat(40_000));
        let _ = c.0.get_mut().write_all(enorme.as_bytes());
        let mut r = String::new();
        let _ = c.0.read_line(&mut r);
        assert!(r.is_empty() || r.contains("\"code\":\"trop_long\""), "{r}");
        assert!(s.arreter());
    }

    #[test]
    fn un_second_serveur_ne_prend_pas_le_nom() {
        let nom = nom_unique("e");
        let s = demarrer(&nom, 5_000);
        let config = Config { nom_tube: nom.clone(), instances: INSTANCES, inactivite: Duration::from_secs(5), trace: sans_trace };
        assert!(Serveur::demarrer(config, Box::new(|_, _, _| None)).is_none());
        assert!(s.arreter());
    }

    #[test]
    fn delai_d_inactivite_par_defaut_et_essai() {
        assert_eq!(delai_inactivite_depuis(None), 10_000);
        assert_eq!(delai_inactivite_depuis(Some("500")), 500);
        assert_eq!(delai_inactivite_depuis(Some("5")), 10_000);
        assert_eq!(delai_inactivite_depuis(Some("x")), 10_000);
        assert_eq!(delai_inactivite_depuis(Some("99999999")), 10_000);
    }
}
