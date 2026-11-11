// Événements `progress` vers SpaceNotch : `{op, state, done, total}` (`state` : running|paused|done|error|cancelled), envoyés
// au tube `Eco.spacenotch.<utilisateur>` par UN fil d'émission, alimenté par une file bornée (un seul `running` en attente par opération). Le fil d'interface et les fils
// d'opérations n'appellent que `annoncer` : une présence de tube vérifiée au plus une fois par seconde, un verrou, un
// `try_send` ; jamais d'entrée-sortie de tube, jamais d'attente.
//
// Quand SpaceNotch est absent (pas de tube), `annoncer` ne fait rien d'autre : aucun fil n'est lancé, aucun appel de tube.
// Le fil d'émission naît au premier événement d'un tube présent et s'arrête après `INACTIVITE` sans événement (rien ne
// tourne à vide). Une opération de fichiers ne dépend jamais de l'émission : un échec est jeté, sans trace à l'écran.
//
// Coalescence : au plus 10 événements/s par opération (100 ms entre deux `running`), un changement d'état (pause, reprise)
// et l'état final `done|error|cancelled` sont toujours mis en file, et un état final n'est jamais jeté pour faire de la place.
// Cible `NonFiable`, muette ou d'un autre protocole : repit de 5 s sans aucune connexion (pas de réessai en boucle) ; un état
// final qui a échoué est réessayé UNE fois à l'échéance. Le journal ne reçoit ni chemin ni nom de fichier.

use std::collections::VecDeque;
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use super::client::{self, Cible, Issue};
use super::json::echapper;
use crate::journal::Niveau;

/// Le fil d'émission s'arrête après ce silence
const INACTIVITE: Duration = Duration::from_secs(30);
/// Événements en attente d'émission : au-delà, les nouveaux sont jetés (jamais d'attente côté opération)
const FILE_MAX: usize = 64;
/// Entre deux événements d'une même opération (au plus 10 par seconde)
const ECART_MS: u64 = 100;
/// La présence du tube n'est vérifiée qu'une fois par seconde
const CONTROLE_MS: u64 = 1000;
/// Opérations suivies en même temps (une opération qui disparaît sans état final ne s'accumule pas)
const OPERATIONS_MAX: usize = 64;
/// Après un échec `NonFiable`, `Protocole` ou `Delai` : aucune connexion pendant ce temps
const REPIT: Duration = Duration::from_secs(5);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Etat {
    Running,
    Paused,
    Done,
    Error,
    Cancelled,
}

impl Etat {
    fn cle(self) -> &'static str {
        match self {
            Etat::Running => "running",
            Etat::Paused => "paused",
            Etat::Done => "done",
            Etat::Error => "error",
            Etat::Cancelled => "cancelled",
        }
    }

    fn final_(self) -> bool {
        matches!(self, Etat::Done | Etat::Error | Etat::Cancelled)
    }
}

/// Faut-il émettre cet événement ? `dernier` : instant (ms) et état du dernier événement de l'opération
fn faut_emettre(dernier: Option<(u64, Etat)>, maintenant_ms: u64, etat: Etat) -> bool {
    if etat.final_() {
        return true;
    }
    match dernier {
        None => true,
        Some((_, e)) if e != etat => true,
        Some((t, _)) => maintenant_ms.saturating_sub(t) >= ECART_MS,
    }
}

/// Le texte de `args` : `{"op":"…","state":"…","done":N,"total":N}`
fn args_progress(op: &str, etat: Etat, fait: u64, total: u64) -> String {
    format!("{{\"op\":{},\"state\":\"{}\",\"done\":{fait},\"total\":{total}}}", echapper(op), etat.cle())
}

#[derive(Clone)]
struct Evenement {
    op: String,
    etat: Etat,
    fait: u64,
    total: u64,
    /// Tentatives déjà faites pour un état final (réessayé une seule fois, à la fin du repit)
    essais: u8,
}

struct Operation {
    op: String,
    dernier_ms: u64,
    etat: Etat,
    fait: u64,
    total: u64,
}

struct Interne {
    controle_ms: Option<u64>,
    present: bool,
    operations: Vec<Operation>,
    /// Événements en attente d'émission : au plus un `running` par opération en attente (le dernier chiffre remplace
    /// le précédent) ; un état final n'est jamais jeté pour faire de la place à autre chose
    file: VecDeque<Evenement>,
    /// Un fil d'émission tourne (il se déclare arrêté sous ce même verrou)
    fil_vivant: bool,
    /// Tant que l'horloge de l'émetteur est avant cet instant (ms), la cible est tenue pour indisponible : rien n'est envoyé,
    /// aucune connexion n'est tentée
    indisponible_jusqu_ms: Option<u64>,
}

struct Partage {
    interne: Mutex<Interne>,
    reveil: Condvar,
}

pub struct Emetteur {
    cible: Cible,
    debut: Instant,
    inactivite: Duration,
    /// Après un `NonFiable`, un `Protocole` ou un `Delai` : pas de connexion pendant ce temps (5 s)
    repit: Duration,
    partage: Arc<Partage>,
    /// Tests : nombre de fils d'émission lancés
    #[cfg(test)]
    fils_lances: Arc<std::sync::atomic::AtomicUsize>,
}

/// Raison lisible (sans contenu d'appel) d'un échec d'émission, pour le journal
fn etiquette(i: &Issue) -> String {
    match i {
        Issue::Ok(_) => "ok".to_string(),
        Issue::Absent => "absent".to_string(),
        Issue::Delai => "delai".to_string(),
        Issue::NonFiable => "non fiable".to_string(),
        Issue::Protocole => "protocole".to_string(),
        Issue::Erreur { code } => format!("erreur {}", code.chars().filter(|c| c.is_ascii_lowercase() || *c == '_').take(24).collect::<String>()),
    }
}

/// Ce qui justifie d'attendre avant de recommencer : la cible répond mais mal, ou pas à temps. (`Absent` : personne à qui parler,
/// la présence est revérifiée à chaque seconde ; `Erreur` : elle a répondu proprement, ce n'est pas une panne.)
fn demande_un_repit(i: &Issue) -> bool {
    matches!(i, Issue::NonFiable | Issue::Protocole | Issue::Delai)
}

/// Met l'événement en file. Un événement de même opération et de même état (non final) en attente est remplacé par le
/// nouveau (seul le dernier chiffre compte). File pleine : on jette d'abord le plus ancien événement non final, jamais un
/// état final tant qu'il reste autre chose.
fn empiler(file: &mut VecDeque<Evenement>, ev: Evenement) {
    if !ev.etat.final_() {
        if let Some(e) = file.iter_mut().rev().find(|e| e.op == ev.op) {
            if e.etat == ev.etat {
                e.fait = ev.fait;
                e.total = ev.total;
                return;
            }
            if e.etat.final_() {
                // L'opération est déjà finie : un chiffre en retard n'a plus de sens
                return;
            }
        }
    } else {
        // Un état final remplace le final de la même opération s'il y en a un (jamais deux finals pour une opération)
        file.retain(|e| !(e.op == ev.op && e.etat.final_()));
    }
    file.push_back(ev);
    while file.len() > FILE_MAX {
        match file.iter().position(|e| !e.etat.final_()) {
            Some(i) => {
                file.remove(i);
            }
            None => {
                file.pop_front();
            }
        }
    }
}

impl Emetteur {
    pub fn nouveau(cible: Cible, inactivite: Duration) -> Emetteur {
        Emetteur {
            cible,
            debut: Instant::now(),
            inactivite,
            repit: REPIT,
            partage: Arc::new(Partage {
                interne: Mutex::new(Interne { controle_ms: None, present: false, operations: Vec::new(), file: VecDeque::new(), fil_vivant: false, indisponible_jusqu_ms: None }),
                reveil: Condvar::new(),
            }),
            #[cfg(test)]
            fils_lances: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
        }
    }

    /// L'émetteur d'une application d'après l'utilisateur Windows et la variable d'essai
    pub fn pour(utilisateur: &str, essai: Option<&str>) -> Emetteur {
        Emetteur::nouveau(Cible::pour("spacenotch", utilisateur, essai), INACTIVITE)
    }

    /// Tests : un repit plus court que 5 s
    #[cfg(test)]
    fn avec_repit(mut self, repit: Duration) -> Emetteur {
        self.repit = repit;
        self
    }

    fn maintenant_ms(&self) -> u64 {
        self.debut.elapsed().as_millis() as u64
    }

    /// Signale l'état d'une opération. Appelable depuis n'importe quel fil, y compris celui de l'interface : jamais d'E/S de tube.
    /// Pour un état final, `fait` et `total` à 0 reprennent les derniers connus.
    pub fn annoncer(&self, op: &str, etat: Etat, fait: u64, total: u64) {
        let maintenant = self.maintenant_ms();
        let mut g = self.partage.interne.lock().unwrap_or_else(|e| e.into_inner());
        // Cible en repit (non fiable, muette, protocole inconnu) : rien ne part, aucune connexion n'est tentée. Un état final
        // est gardé pour être réessayé une fois à l'échéance ; un `running` ne vaut plus rien.
        let en_repit = g.indisponible_jusqu_ms.is_some_and(|d| maintenant < d);
        if en_repit && !etat.final_() {
            return;
        }
        // Présence du tube : au plus une vérification par seconde, et seulement quand une opération émet
        if !en_repit && g.controle_ms.is_none_or(|t| maintenant.saturating_sub(t) >= CONTROLE_MS) {
            g.present = client::present(&self.cible.nom_tube);
            g.controle_ms = Some(maintenant);
        }
        if !en_repit && !g.present {
            // Rien ne sera émis : on n'accumule pas d'état pour une opération que personne n'écoute
            g.operations.clear();
            g.file.clear();
            return;
        }
        let rang = g.operations.iter().position(|o| o.op == op);
        let dernier = rang.map(|i| (g.operations[i].dernier_ms, g.operations[i].etat));
        let (mut fait, mut total) = (fait, total);
        if let Some(i) = rang {
            // L'état final reprend les derniers chiffres vus (les événements coalescés comptent aussi)
            if etat.final_() && fait == 0 && total == 0 {
                (fait, total) = (g.operations[i].fait, g.operations[i].total);
                if etat == Etat::Done {
                    fait = total;
                }
            }
            g.operations[i].fait = fait;
            g.operations[i].total = total;
        }
        if !faut_emettre(dernier, maintenant, etat) {
            return;
        }
        match rang {
            Some(i) if etat.final_() => {
                g.operations.remove(i);
            }
            Some(i) => {
                g.operations[i].dernier_ms = maintenant;
                g.operations[i].etat = etat;
            }
            None if !etat.final_() => {
                if g.operations.len() >= OPERATIONS_MAX {
                    g.operations.remove(0);
                }
                g.operations.push(Operation { op: op.to_string(), dernier_ms: maintenant, etat, fait, total });
            }
            None => {}
        }
        // Un final annoncé pendant un repit n'a pas encore été tenté : l'échéance est sa tentative (une seule)
        empiler(&mut g.file, Evenement { op: op.to_string(), etat, fait, total, essais: u8::from(en_repit) });
        if g.fil_vivant {
            self.partage.reveil.notify_one();
            return;
        }
        let (partage, cible, inactivite, repit, debut) = (self.partage.clone(), self.cible.clone(), self.inactivite, self.repit, self.debut);
        match std::thread::Builder::new().name("eco-progress".into()).spawn(move || boucle(partage, cible, inactivite, repit, debut)) {
            Ok(_) => {
                g.fil_vivant = true;
                #[cfg(test)]
                self.fils_lances.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            }
            // Pas de fil : on ne garde pas d'événements que personne n'enverra
            Err(_) => g.file.clear(),
        }
    }

    /// Tests : y a-t-il un fil d'émission ?
    #[cfg(test)]
    fn fil_present(&self) -> bool {
        self.partage.interne.lock().unwrap_or_else(|e| e.into_inner()).fil_vivant
    }

    /// Tests : plus rien en attente (le fil sort à son prochain réveil)
    #[cfg(test)]
    fn fermer(&self) {
        let mut g = self.partage.interne.lock().unwrap_or_else(|e| e.into_inner());
        g.file.clear();
        g.indisponible_jusqu_ms = None;
    }

    /// Tests : la cible est-elle en repit (une tentative a échoué, plus aucune connexion avant l'échéance) ?
    #[cfg(test)]
    fn en_repit(&self) -> bool {
        let maintenant = self.maintenant_ms();
        self.partage.interne.lock().unwrap_or_else(|e| e.into_inner()).indisponible_jusqu_ms.is_some_and(|d| maintenant < d)
    }

    #[cfg(test)]
    fn en_attente(&self) -> Vec<(String, Etat)> {
        self.partage.interne.lock().unwrap_or_else(|e| e.into_inner()).file.iter().map(|e| (e.op.clone(), e.etat)).collect()
    }
}

/// Le fil d'émission : `hello` (gardé 60 s) pour s'assurer qu'on parle à SpaceNotch et à son protocole, puis `progress`.
/// Il s'arrête après `inactivite` sans rien à envoyer. Un échec `NonFiable`, `Protocole` ou `Delai` met la cible en repit
/// (aucune connexion pendant `repit`) ; un état final qui a échoué est gardé et réessayé UNE fois à l'échéance.
fn boucle(partage: Arc<Partage>, cible: Cible, inactivite: Duration, repit: Duration, debut: Instant) {
    let mut disponible: Option<bool> = None;
    loop {
        // 1. Prendre le prochain événement, ou attendre : événement, échéance du repit, ou inactivité
        let mut ev = {
            let mut g = partage.interne.lock().unwrap_or_else(|e| e.into_inner());
            loop {
                let maintenant = debut.elapsed().as_millis() as u64;
                let reste_repit = g.indisponible_jusqu_ms.and_then(|d| d.checked_sub(maintenant)).filter(|&r| r > 0);
                if reste_repit.is_none() {
                    g.indisponible_jusqu_ms = None;
                    if let Some(ev) = g.file.pop_front() {
                        break ev;
                    }
                }
                let attente = match reste_repit {
                    Some(r) => Duration::from_millis(r).min(inactivite),
                    None => inactivite,
                };
                let (g2, delai) = partage.reveil.wait_timeout(g, attente).unwrap_or_else(|e| e.into_inner());
                g = g2;
                // Rien à envoyer et personne n'a écrit pendant toute l'inactivité : le fil s'arrête (déclaré sous le verrou,
                // donc le prochain `annoncer` en relance un)
                if delai.timed_out() && g.file.is_empty() {
                    g.fil_vivant = false;
                    return;
                }
            }
        };
        // 2. Envoyer, sans verrou
        let r = match client::appeler(&cible, "hello", "{}", client::BUDGET_DEFAUT) {
            Issue::Ok(_) => client::emettre(&cible, "progress", &args_progress(&ev.op, ev.etat, ev.fait, ev.total), client::BUDGET_DEFAUT).err(),
            autre => Some(autre),
        };
        // Une ligne de journal à chaque changement de disponibilité, pas à chaque événement
        let ok = r.is_none();
        if disponible != Some(ok) {
            disponible = Some(ok);
            match &r {
                None => super::trace(Niveau::Info, "eco-client", "emission de progress vers SpaceNotch : disponible"),
                Some(i) => super::trace(Niveau::Info, "eco-client", &format!("emission de progress vers SpaceNotch : indisponible ({})", etiquette(i))),
            }
        }
        // 3. Un échec « répit » : plus de connexion avant l'échéance ; l'état final est gardé, une seule fois
        if r.as_ref().is_some_and(demande_un_repit) {
            let mut g = partage.interne.lock().unwrap_or_else(|e| e.into_inner());
            g.indisponible_jusqu_ms = Some(debut.elapsed().as_millis() as u64 + repit.as_millis() as u64);
            // Les `running` en attente ne valent plus rien ; les états finals restent
            g.file.retain(|e| e.etat.final_());
            // Ceux qui attendaient déjà ont leur « seconde chance » : l'échéance est leur tentative
            for e in g.file.iter_mut() {
                e.essais = 1;
            }
            if ev.etat.final_() && ev.essais == 0 {
                ev.essais = 1;
                g.file.push_front(ev);
            }
        }
    }
}

// ───────────────────────── Point d'entrée du résident et des fenêtres ─────────────────────────

#[cfg(not(test))]
static EMETTEUR: std::sync::OnceLock<Emetteur> = std::sync::OnceLock::new();

/// Tests : l'émetteur que `annoncer` utilise, pour UN numéro d'opération seulement (aucun en dehors de ces tests : la suite ne
/// touche jamais le vrai tube de SpaceNotch ; les autres opérations des tests, lancées en parallèle, ne l'atteignent pas)
#[cfg(test)]
static EMETTEUR_TEST: Mutex<Option<(u64, Arc<Emetteur>)>> = Mutex::new(None);

/// Avancement d'une opération de fichiers de cette fenêtre (`numero` : celui de `App::Operation`). Appelé depuis le suivi
/// de l'opération et depuis la fin de son fil ; ne bloque jamais.
pub fn annoncer(numero: u64, etat: Etat, fait: u64, total: u64) {
    let op = format!("{}-{numero}", std::process::id());
    #[cfg(test)]
    {
        let e = EMETTEUR_TEST.lock().unwrap_or_else(|x| x.into_inner()).clone();
        if let Some((n, e)) = e {
            if n == numero {
                e.annoncer(&op, etat, fait, total);
            }
        }
    }
    #[cfg(not(test))]
    EMETTEUR
        .get_or_init(|| Emetteur::pour(&std::env::var("USERNAME").unwrap_or_default(), super::essai().as_deref()))
        .annoncer(&op, etat, fait, total);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::eco::json::{analyser_json, champ, Json};
    use crate::eco::serveur::instance_de_test;
    use crate::eco::trame::{analyser_trame, reponse_hello, Appel};
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use windows::Win32::Foundation::*;
    use windows::Win32::Storage::FileSystem::{ReadFile, WriteFile};
    use windows::Win32::System::Pipes::{ConnectNamedPipe, DisconnectNamedPipe};

    fn nom_unique(s: &str) -> String {
        static N: AtomicUsize = AtomicUsize::new(0);
        format!(r"\\.\pipe\Eco.spacenotch.progress-{}-{}-{s}", std::process::id(), N.fetch_add(1, Ordering::SeqCst))
    }

    /// Faux SpaceNotch : un tube, un fil, répond à `hello`, garde toutes les autres lignes reçues avec leur instant
    struct Faux {
        nom: String,
        recu: Arc<Mutex<Vec<(Instant, String)>>>,
        arret: Arc<AtomicBool>,
        /// Connexions acceptées depuis le début
        connexions: Arc<AtomicUsize>,
        /// Vrai : le faux serveur ne répond plus à `hello` (cible figée)
        muet: Arc<AtomicBool>,
        fil: Option<std::thread::JoinHandle<()>>,
    }

    impl Faux {
        fn demarrer(nom: &str) -> Faux {
            let h = crate::eco::some(instance_de_test(nom), "tube du faux serveur");
            let valeur = h.0 as isize;
            let recu = Arc::new(Mutex::new(Vec::new()));
            let arret = Arc::new(AtomicBool::new(false));
            let connexions = Arc::new(AtomicUsize::new(0));
            let muet = Arc::new(AtomicBool::new(false));
            let (r, a, c, m) = (recu.clone(), arret.clone(), connexions.clone(), muet.clone());
            let fil = std::thread::spawn(move || {
                let h = HANDLE(valeur as *mut _);
                let mut reste: Vec<u8> = Vec::new();
                while !a.load(Ordering::SeqCst) {
                    // SAFETY : `h` est l'instance créée plus haut, fermée seulement en fin de fil.
                    let connecte = match unsafe { ConnectNamedPipe(h, None) } {
                        Ok(()) => true,
                        Err(e) => e.code() == ERROR_PIPE_CONNECTED.to_hresult(),
                    };
                    if connecte && !a.load(Ordering::SeqCst) {
                        c.fetch_add(1, Ordering::SeqCst);
                        loop {
                            let mut bloc = [0u8; 4096];
                            let mut lus = 0u32;
                            // SAFETY : tampon valide pour sa longueur ; lecture synchrone sur l'instance du fil.
                            if unsafe { ReadFile(h, Some(&mut bloc), Some(&mut lus), None) }.is_err() || lus == 0 {
                                break;
                            }
                            reste.extend_from_slice(&bloc[..lus as usize]);
                            while let Some(p) = reste.iter().position(|&c| c == b'\n') {
                                let ligne = String::from_utf8_lossy(&reste[..p]).into_owned();
                                reste.drain(..=p);
                                let (id, appel) = analyser_trame(&ligne);
                                if appel == Ok(Appel::Hello) {
                                    if m.load(Ordering::SeqCst) {
                                        continue;
                                    }
                                    let mut rep = reponse_hello(&id).replace("spacefiles", "spacenotch").into_bytes();
                                    rep.push(b'\n');
                                    let mut ecrits = 0u32;
                                    // SAFETY : tampon valide pour sa longueur.
                                    let _ = unsafe { WriteFile(h, Some(&rep), Some(&mut ecrits), None) };
                                } else {
                                    r.lock().unwrap_or_else(|e| e.into_inner()).push((Instant::now(), ligne));
                                }
                            }
                        }
                    }
                    reste.clear();
                    // SAFETY : même handle ; libère l'instance pour le client suivant.
                    unsafe {
                        let _ = DisconnectNamedPipe(h);
                    }
                }
                // SAFETY : handle créé pour ce fil, plus utilisé après.
                unsafe {
                    let _ = CloseHandle(h);
                }
            });
            Faux { nom: nom.to_string(), recu, arret, connexions, muet, fil: Some(fil) }
        }

        fn lignes(&self) -> Vec<(Instant, String)> {
            self.recu.lock().unwrap_or_else(|e| e.into_inner()).clone()
        }

        fn attendre(&self, delai: Duration, condition: impl Fn(&[(Instant, String)]) -> bool) -> bool {
            let fin = Instant::now() + delai;
            while Instant::now() < fin {
                if condition(&self.lignes()) {
                    return true;
                }
                std::thread::sleep(Duration::from_millis(10));
            }
            condition(&self.lignes())
        }
    }

    impl Drop for Faux {
        fn drop(&mut self) {
            self.arret.store(true, Ordering::SeqCst);
            // Réveille le ConnectNamedPipe en attente
            let _ = std::fs::OpenOptions::new().read(true).write(true).open(&self.nom);
            if let Some(f) = self.fil.take() {
                let _ = f.join();
            }
        }
    }

    fn etat_de(ligne: &str) -> String {
        let Some(Json::Objet(o)) = analyser_json(ligne) else { return String::new() };
        let Some(Json::Objet(a)) = champ(&o, "args") else { return String::new() };
        match champ(a, "state") {
            Some(Json::Texte(t)) => t.clone(),
            _ => String::new(),
        }
    }

    #[test]
    fn emetteur_du_resident_vise_le_tube_de_spacenotch() {
        let e = Emetteur::pour("utilisateur", Some("t1"));
        assert_eq!(e.cible.nom_tube, r"\\.\pipe\Eco.spacenotch.t1");
        assert_eq!(e.inactivite, INACTIVITE);
        assert_eq!(Emetteur::pour("utilisateur", None).cible.nom_tube, r"\\.\pipe\Eco.spacenotch.utilisateur");
    }

    #[test]
    fn coalescence_decision() {
        let r = Etat::Running;
        assert!(faut_emettre(None, 0, r), "premier événement");
        assert!(!faut_emettre(Some((0, r)), 50, r), "moins de 100 ms");
        assert!(!faut_emettre(Some((0, r)), 99, r));
        assert!(faut_emettre(Some((0, r)), 100, r));
        assert!(faut_emettre(Some((0, r)), 5, Etat::Paused), "changement d'état");
        for f in [Etat::Done, Etat::Error, Etat::Cancelled] {
            assert!(faut_emettre(Some((0, r)), 1, f), "l'état final est toujours émis : {f:?}");
        }
        // Horloge qui recule : jamais d'émission en rafale
        assert!(!faut_emettre(Some((500, r)), 100, r));
    }

    #[test]
    fn forme_de_l_evenement() {
        let a = args_progress("12-3", Etat::Running, 40, 100);
        assert_eq!(a, "{\"op\":\"12-3\",\"state\":\"running\",\"done\":40,\"total\":100}");
        assert!(analyser_json(&a).is_some());
        for (e, c) in [(Etat::Paused, "paused"), (Etat::Done, "done"), (Etat::Error, "error"), (Etat::Cancelled, "cancelled")] {
            assert!(args_progress("o", e, 0, 0).contains(&format!("\"state\":\"{c}\"")));
        }
        assert!(args_progress("a\"b\\c", Etat::Done, 1, 1).contains("a\\\"b\\\\c"), "op échappé");
    }

    #[test]
    fn aucun_fil_ni_appel_quand_spacenotch_est_absent() {
        let e = Emetteur::nouveau(Cible::avec(&nom_unique("absent"), client::ExeAttendu::Quelconque), Duration::from_millis(300));
        let debut = Instant::now();
        for i in 0..500u64 {
            e.annoncer("op", Etat::Running, i, 500);
        }
        e.annoncer("op", Etat::Done, 0, 0);
        assert_eq!(e.fils_lances.load(Ordering::SeqCst), 0, "un fil a ete lance sans SpaceNotch");
        assert!(!e.fil_present());
        assert!(debut.elapsed() < Duration::from_millis(500), "annoncer ne doit rien coûter : {:?}", debut.elapsed());
        assert!(e.partage.interne.lock().unwrap_or_else(|x| x.into_inner()).operations.is_empty());
    }

    #[test]
    fn running_puis_done_coalesces_et_fil_arrete_apres_inactivite() {
        let nom = nom_unique("ok");
        let faux = Faux::demarrer(&nom);
        let e = Emetteur::nouveau(Cible::avec(&nom, client::ExeAttendu::Quelconque), Duration::from_millis(400));
        let debut = Instant::now();
        // ~1 s d'opération : 200 mises à jour, une toutes les 5 ms
        for i in 1..=200u64 {
            e.annoncer("42-1", Etat::Running, i * 10, 2000);
            std::thread::sleep(Duration::from_millis(5));
        }
        e.annoncer("42-1", Etat::Done, 0, 0);
        let duree = debut.elapsed();
        assert!(faux.attendre(Duration::from_secs(5), |l| l.iter().any(|(_, t)| etat_de(t) == "done")), "pas d'evenement done");
        let lignes = faux.lignes();
        let etats: Vec<String> = lignes.iter().map(|(_, t)| etat_de(t)).collect();
        assert_eq!(etats.first().map(String::as_str), Some("running"));
        assert_eq!(etats.last().map(String::as_str), Some("done"), "{etats:?}");
        let n_running = etats.iter().filter(|s| *s == "running").count();
        assert!(n_running >= 2, "la progression doit etre transmise : {n_running}");
        // Coalescé : au plus 10 par seconde (+ le premier)
        let max = (duree.as_secs_f64() * 10.0).ceil() as usize + 2;
        assert!(n_running <= max, "{n_running} running en {duree:?} (max {max})");
        assert!(n_running < 100, "pas de coalescence : {n_running}");
        // Chaque ligne est un événement bien formé, sans chemin ; l'état final reprend les derniers chiffres (done = total)
        let Some(Json::Objet(o)) = analyser_json(&lignes[lignes.len() - 1].1) else { panic!("derniere ligne illisible") };
        assert_eq!(champ(&o, "kind"), Some(&Json::Texte("event".into())));
        assert_eq!(champ(&o, "name"), Some(&Json::Texte("progress".into())));
        let Some(Json::Objet(a)) = champ(&o, "args") else { panic!("pas d'args") };
        assert_eq!(champ(a, "done"), champ(a, "total"));
        assert_eq!(champ(a, "total"), Some(&Json::Nombre(2000.0)));
        assert!(lignes.iter().all(|(_, t)| !t.contains("C:\\") && etat_de(t) != ""));
        // Écart entre deux running : jamais sous 100 ms
        let instants: Vec<Instant> = lignes.iter().filter(|(_, t)| etat_de(t) == "running").map(|(i, _)| *i).collect();
        assert!(instants.windows(2).all(|p| p[1].duration_since(p[0]) >= Duration::from_millis(60)), "ecart trop court entre deux running");
        // Le fil d'émission s'arrête seul après l'inactivité (400 ms)
        assert_eq!(e.fils_lances.load(Ordering::SeqCst), 1);
        std::thread::sleep(Duration::from_millis(900));
        assert!(!e.fil_present(), "le fil d'emission aurait du s'arreter");
        // Un nouvel événement le relance
        e.annoncer("42-2", Etat::Running, 1, 2);
        assert_eq!(e.fils_lances.load(Ordering::SeqCst), 2);
        e.fermer();
    }


    fn ev(op: &str, etat: Etat) -> Evenement {
        Evenement { op: op.to_string(), etat, fait: 1, total: 2, essais: 0 }
    }

    /// I3(a) : la file garde le dernier chiffre de chaque opération, et un état final n'est jamais jeté
    #[test]
    fn la_file_ne_jette_jamais_un_etat_final() {
        let mut f = VecDeque::new();
        for i in 0..10 {
            empiler(&mut f, ev(&format!("fin-{i}"), Etat::Done));
        }
        // Une rafale de 500 `running` d'opérations différentes : la file reste bornée, les 10 finals y sont encore
        for i in 0..500 {
            empiler(&mut f, ev(&format!("run-{i}"), Etat::Running));
        }
        assert!(f.len() <= FILE_MAX, "{}", f.len());
        for i in 0..10 {
            assert!(f.iter().any(|e| e.op == format!("fin-{i}") && e.etat == Etat::Done), "final {i} jeté");
        }
        // Rien que des finals : on garde les plus récents (bornée quand même)
        let mut g = VecDeque::new();
        for i in 0..(FILE_MAX + 20) {
            empiler(&mut g, ev(&format!("o-{i}"), Etat::Error));
        }
        assert_eq!(g.len(), FILE_MAX);
        assert!(g.iter().any(|e| e.op == format!("o-{}", FILE_MAX + 19)), "le plus récent est gardé");
        // Même opération, même état : le dernier chiffre remplace l'ancien (un seul en attente)
        let mut h = VecDeque::new();
        for i in 1..=50u64 {
            empiler(&mut h, Evenement { op: "x".into(), etat: Etat::Running, fait: i, total: 100, essais: 0 });
        }
        assert_eq!(h.len(), 1);
        assert_eq!(h[0].fait, 50);
        // Pause, reprise, fin : les changements d'état restent dans l'ordre ; un final remplace l'entrée de l'opération
        empiler(&mut h, ev("x", Etat::Paused));
        empiler(&mut h, ev("x", Etat::Running));
        empiler(&mut h, ev("x", Etat::Cancelled));
        empiler(&mut h, ev("x", Etat::Done));
        let etats: Vec<Etat> = h.iter().map(|e| e.etat).collect();
        assert_eq!(etats, [Etat::Running, Etat::Paused, Etat::Running, Etat::Done]);
        // Un chiffre en retard après l'état final est ignoré
        empiler(&mut h, ev("x", Etat::Running));
        assert_eq!(h.back().map(|e| e.etat), Some(Etat::Done));
    }

    /// Attend (au plus `delai`) que la condition devienne vraie ; rend sa dernière valeur
    fn attendre_condition(delai: Duration, condition: impl Fn() -> bool) -> bool {
        let fin = Instant::now() + delai;
        while Instant::now() < fin {
            if condition() {
                return true;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        condition()
    }

    /// I3(b) : une cible non fiable ne reçoit pas de connexions en boucle ; l'état final est gardé et réessayé une seule fois.
    /// Aucun délai fixe ne décide du résultat (la suite entière tourne en parallèle : un fil peut démarrer 1 s en retard) : on
    /// attend les ÉVÉNEMENTS (première connexion reçue ET cible en repit), puis on compte. Le repit (2,5 s) est assez long pour
    /// que la rafale d'annonces, quelle que soit la charge, tombe dedans.
    #[test]
    fn cible_non_fiable_connexions_bornees() {
        let nom = nom_unique("nonfiable");
        let faux = Faux::demarrer(&nom);
        let cible = Cible::avec(&nom, client::ExeAttendu::Chemin(r"C:\autre\programme.exe".into()));
        let e = Emetteur::nouveau(cible, Duration::from_secs(5)).avec_repit(Duration::from_millis(2500));
        e.annoncer("n-1", Etat::Running, 1, 10);
        e.annoncer("n-1", Etat::Done, 0, 0);
        assert!(
            attendre_condition(Duration::from_secs(10), || faux.connexions.load(Ordering::SeqCst) >= 1 && e.en_repit()),
            "la première tentative aurait dû avoir lieu et mettre la cible en repit"
        );
        // Pendant le repit : une rafale d'annonces ne tente aucune connexion
        let avant = faux.connexions.load(Ordering::SeqCst);
        assert_eq!(avant, 1, "une première tentative, une seule");
        for i in 0..500u64 {
            e.annoncer("n-2", Etat::Running, i, 500);
            e.annoncer(&format!("n-3-{}", i % 3), Etat::Running, i, 500);
        }
        assert!(e.en_repit(), "la rafale aurait dû tenir dans le repit (2,5 s)");
        assert_eq!(faux.connexions.load(Ordering::SeqCst), avant, "aucune connexion pendant le repit");
        assert!(e.en_attente().iter().any(|(o, s)| o == "n-1" && *s == Etat::Done), "l'état final est gardé");
        assert!(e.en_attente().iter().all(|(_, s)| s.final_()), "les `running` ne sont pas gardés pendant le repit");
        // À l'échéance : UNE seule nouvelle tentative, puis plus rien (pas de réessai en boucle)
        assert!(attendre_condition(Duration::from_secs(15), || faux.connexions.load(Ordering::SeqCst) > avant), "la seconde tentative de l'état final n'a pas eu lieu");
        assert!(attendre_condition(Duration::from_secs(5), || e.en_attente().is_empty()), "l'état final abandonné après sa seconde tentative");
        let apres = faux.connexions.load(Ordering::SeqCst);
        assert_eq!(apres, avant + 1, "une seule tentative de plus pour l'état final");
        std::thread::sleep(Duration::from_millis(1500));
        assert_eq!(faux.connexions.load(Ordering::SeqCst), apres, "plus aucune connexion");
        assert!(faux.lignes().is_empty(), "rien n'a été émis vers une cible non fiable");
        e.fermer();
    }

    /// I3(b) : une cible figée (ne répond pas à `hello`) ; quand elle revient, l'état final en attente part
    #[test]
    fn cible_figee_puis_revenue_recoit_l_etat_final() {
        let nom = nom_unique("figee");
        let faux = Faux::demarrer(&nom);
        faux.muet.store(true, Ordering::SeqCst);
        let e = Emetteur::nouveau(Cible::avec(&nom, client::ExeAttendu::Quelconque), Duration::from_secs(5)).avec_repit(Duration::from_millis(3000));
        e.annoncer("f-1", Etat::Running, 1, 10);
        e.annoncer("f-1", Etat::Done, 0, 0);
        // `hello` reste sans réponse : le délai (300 ms) met la cible en repit (on attend l'événement, pas un délai fixe)
        assert!(
            attendre_condition(Duration::from_secs(10), || faux.connexions.load(Ordering::SeqCst) >= 1 && e.en_repit()),
            "la cible muette aurait dû être mise en repit"
        );
        let pendant = faux.connexions.load(Ordering::SeqCst);
        for i in 0..200u64 {
            e.annoncer("f-2", Etat::Running, i, 200);
        }
        assert_eq!(faux.connexions.load(Ordering::SeqCst), pendant, "aucune connexion pendant le repit");
        // La cible répond de nouveau : à l'échéance l'état final est réessayé et arrive
        faux.muet.store(false, Ordering::SeqCst);
        assert!(faux.attendre(Duration::from_secs(5), |l| l.iter().any(|(_, t)| etat_de(t) == "done")), "l'état final aurait dû être réessayé");
        assert!(faux.lignes().iter().all(|(_, t)| etat_de(t) != "running"), "un running périmé est parti");
        e.fermer();
    }

    #[test]
    fn pause_et_annulation_sont_emises() {
        let nom = nom_unique("pause");
        let faux = Faux::demarrer(&nom);
        let e = Emetteur::nouveau(Cible::avec(&nom, client::ExeAttendu::Quelconque), Duration::from_secs(5));
        e.annoncer("7-1", Etat::Running, 10, 100);
        e.annoncer("7-1", Etat::Paused, 10, 100);
        e.annoncer("7-1", Etat::Running, 20, 100);
        e.annoncer("7-1", Etat::Cancelled, 0, 0);
        assert!(faux.attendre(Duration::from_secs(5), |l| l.iter().any(|(_, t)| etat_de(t) == "cancelled")));
        let etats: Vec<String> = faux.lignes().iter().map(|(_, t)| etat_de(t)).collect();
        assert_eq!(etats, ["running", "paused", "running", "cancelled"]);
        e.fermer();
    }

    #[test]
    fn spacenotch_qui_apparait_en_cours_de_route() {
        let nom = nom_unique("tard");
        let e = Emetteur::nouveau(Cible::avec(&nom, client::ExeAttendu::Quelconque), Duration::from_secs(5));
        e.annoncer("9-1", Etat::Running, 1, 10);
        assert!(!e.fil_present(), "SpaceNotch absent : aucun fil");
        let faux = Faux::demarrer(&nom);
        // La présence n'est revérifiée qu'une fois par seconde
        std::thread::sleep(Duration::from_millis(1100));
        e.annoncer("9-1", Etat::Running, 5, 10);
        e.annoncer("9-1", Etat::Done, 0, 0);
        assert!(faux.attendre(Duration::from_secs(5), |l| l.iter().any(|(_, t)| etat_de(t) == "done")));
        e.fermer();
    }

    /// Une vraie copie par le moteur de Windows (le `Suivi` de `operations.rs`), dans un dossier jetable : `running` puis
    /// `done`, coalescés ; puis la même copie sans SpaceNotch : aucun fil d'émission. Aucune souris, aucune fenêtre.
    #[test]
    fn copie_reelle_via_le_suivi() {
        use crate::operations::{lancer, Controle, Element, Genre};
        use windows::Win32::Foundation::HWND;

        let racine = std::env::temp_dir().join(format!("eco-progress-copie-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&racine);
        let (src, dst) = (racine.join("src"), racine.join("dst"));
        crate::eco::ok(std::fs::create_dir_all(&src), "creation source");
        crate::eco::ok(std::fs::create_dir_all(&dst), "creation destination");
        for i in 0..3 {
            crate::eco::ok(std::fs::write(src.join(format!("f{i}.bin")), vec![7u8; 6 * 1024 * 1024]), "fichier de test");
        }
        let elements = |dossier_dest: &std::path::Path| -> Vec<Element> {
            (0..3).map(|i| Element::simple(src.join(format!("f{i}.bin")).to_string_lossy().into_owned(), Some(dossier_dest.to_string_lossy().into_owned()))).collect()
        };

        // 1. Avec un faux SpaceNotch
        let nom = nom_unique("copie");
        let faux = Faux::demarrer(&nom);
        let avec = Arc::new(Emetteur::nouveau(Cible::avec(&nom, client::ExeAttendu::Quelconque), Duration::from_secs(5)));
        *EMETTEUR_TEST.lock().unwrap_or_else(|x| x.into_inner()) = Some((910_001, avec.clone()));
        let debut = Instant::now();
        lancer(HWND(std::ptr::null_mut()), 910_001, Genre::Copier, elements(&dst), Vec::new(), Controle::default());
        let fini = faux.attendre(Duration::from_secs(60), |l| l.iter().any(|(_, t)| etat_de(t) == "done"));
        let duree = debut.elapsed();
        let etats: Vec<String> = faux.lignes().iter().map(|(_, t)| etat_de(t)).collect();
        assert!(fini, "pas de done apres la copie : {etats:?}");
        assert_eq!(etats.first().map(String::as_str), Some("running"), "{etats:?}");
        assert_eq!(etats.last().map(String::as_str), Some("done"));
        let n_running = etats.iter().filter(|s| *s == "running").count();
        assert!(n_running as f64 <= duree.as_secs_f64() * 10.0 + 2.0, "{n_running} running en {duree:?}");
        assert!(faux.lignes().iter().all(|(_, t)| !t.contains("f0.bin") && !t.contains("eco-progress")), "un nom de fichier ou un chemin a fuite");
        assert_eq!(crate::eco::ok(std::fs::read(dst.join("f2.bin")), "copie arrivee").len(), 6 * 1024 * 1024, "la copie doit aboutir normalement");
        avec.fermer();

        // 2. Sans SpaceNotch : la copie aboutit, aucun fil d'émission
        let sans = Arc::new(Emetteur::nouveau(Cible::avec(&nom_unique("copie-absent"), client::ExeAttendu::Quelconque), Duration::from_secs(5)));
        *EMETTEUR_TEST.lock().unwrap_or_else(|x| x.into_inner()) = Some((910_002, sans.clone()));
        let dst2 = racine.join("dst2");
        crate::eco::ok(std::fs::create_dir_all(&dst2), "creation destination 2");
        lancer(HWND(std::ptr::null_mut()), 910_002, Genre::Copier, elements(&dst2), Vec::new(), Controle::default());
        let fin = Instant::now() + Duration::from_secs(60);
        while !dst2.join("f2.bin").exists() && Instant::now() < fin {
            std::thread::sleep(Duration::from_millis(50));
        }
        std::thread::sleep(Duration::from_millis(500));
        assert!(dst2.join("f2.bin").exists(), "la copie sans SpaceNotch n'a pas abouti");
        assert_eq!(sans.fils_lances.load(Ordering::SeqCst), 0, "un fil d'emission est ne sans SpaceNotch");
        assert!(!sans.fil_present());
        *EMETTEUR_TEST.lock().unwrap_or_else(|x| x.into_inner()) = None;
        let _ = std::fs::remove_dir_all(&racine);
    }

    /// Preuve croisée de SpaceNotch (phase 4, lot B), JAMAIS lancée par `cargo test` seul (`#[ignore]`) : émet de vrais
    /// `hello` puis de vrais `progress` (`running` coalescés, puis `done`) vers le tube d'un serveur EXTERNE, celui de
    /// SpaceNotch ou de son harnais (`tools/bench/preuve-lot-b-notch.cs`). Le tube vient de `SPACEFILES_ECO_CIBLE_ESSAI` (nom court,
    /// sans `\\.\pipe\`) : sans cette variable le test échoue tout de suite, il ne devine jamais un tube et ne touche donc
    /// jamais le vrai SpaceNotch. Réglages d'essai : `SPACEFILES_ECO_DUREE_MS` (durée d'émission, 1000 par défaut) et
    /// `SPACEFILES_ECO_HELLOS` (nombre de `hello` chronométrés avant l'émission, 0 par défaut).
    /// Commande : `cargo test -- --ignored --nocapture emettre_vers_serveur_externe`.
    #[test]
    #[ignore = "preuve croisée : demande un serveur externe (SPACEFILES_ECO_CIBLE_ESSAI)"]
    fn emettre_vers_serveur_externe() {
        let nom = std::env::var("SPACEFILES_ECO_CIBLE_ESSAI").unwrap_or_default();
        assert!(!nom.is_empty(), "SPACEFILES_ECO_CIBLE_ESSAI doit nommer le tube du serveur externe (sans le préfixe)");
        let nombre = |cle: &str, defaut: u64| std::env::var(cle).ok().and_then(|v| v.parse::<u64>().ok()).unwrap_or(defaut);
        let duree_ms = nombre("SPACEFILES_ECO_DUREE_MS", 1000);
        let hellos = nombre("SPACEFILES_ECO_HELLOS", 0) as usize;
        let complet = format!(r"\\.\pipe\{nom}");

        // 1. `hello` : le serveur doit se dire SpaceNotch et parler le protocole 1
        let cible = Cible::avec(&complet, client::ExeAttendu::Quelconque);
        match client::appeler(&cible, "hello", "{}", client::BUDGET_DEFAUT) {
            Issue::Ok(r) => {
                assert_eq!(champ(&r.resultat, "app"), Some(&Json::Texte("spacenotch".into())), "hello : pas SpaceNotch");
                assert_eq!(champ(&r.resultat, "protocol"), Some(&Json::Nombre(1.0)));
                println!("HELLO_OK app=spacenotch protocole=1");
            }
            autre => panic!("hello : {autre:?}"),
        }

        // 2. `hello` en série. La réponse est gardée 60 s PAR NOM de tube : chaque tour change la casse de quelques lettres
        //    (Windows ne distingue pas la casse d'un nom de tube), donc aucun tour ne répond depuis le cache.
        if hellos > 0 {
            let lettres: Vec<usize> = complet.char_indices().filter(|(i, c)| *i > 9 && c.is_ascii_alphabetic()).map(|(i, _)| i).collect();
            assert!(lettres.len() >= 11, "nom de tube trop court pour varier la casse");
            let mut durees: Vec<f64> = Vec::with_capacity(hellos);
            let debut = Instant::now();
            for n in 0..hellos {
                let mut octets = complet.clone().into_bytes();
                for (bit, &pos) in lettres.iter().enumerate().take(11) {
                    if n >> bit & 1 == 1 {
                        octets[pos] ^= 0x20;
                    }
                }
                let c = Cible::avec(&String::from_utf8(octets).unwrap_or_default(), client::ExeAttendu::Quelconque);
                let t = Instant::now();
                let r = client::appeler(&c, "hello", "{}", client::BUDGET_DEFAUT);
                durees.push(t.elapsed().as_secs_f64() * 1000.0);
                assert!(matches!(r, Issue::Ok(_)), "hello n°{n} : {r:?}");
            }
            let total = debut.elapsed().as_secs_f64() * 1000.0;
            durees.sort_by(|a, b| a.total_cmp(b));
            let p = |q: f64| durees[((durees.len() as f64 * q).ceil() as usize).saturating_sub(1)];
            println!("HELLO_SERIE n={hellos} total_ms={total:.1} p50_ms={:.3} p95_ms={:.3} max_ms={:.3}", p(0.5), p(0.95), durees[durees.len() - 1]);
        }

        // 3. `progress` réels par l'émetteur de production : 200 mises à jour par seconde demandées, 10 par seconde émises
        let e = Emetteur::nouveau(cible, Duration::from_secs(30));
        let debut = Instant::now();
        let mut i = 0u64;
        while (debut.elapsed().as_millis() as u64) < duree_ms {
            i += 1;
            e.annoncer("999-1", Etat::Running, i * 10, 1_000_000);
            std::thread::sleep(Duration::from_millis(5));
        }
        e.annoncer("999-1", Etat::Done, 0, 0);
        // Laisse le fil d'émission vider sa file (l'état final part en dernier), puis s'arrête de lui-même
        std::thread::sleep(Duration::from_millis(1500));
        println!("EMIS mises_a_jour={i} duree_ms={}", debut.elapsed().as_millis());
        e.fermer();
    }
}
