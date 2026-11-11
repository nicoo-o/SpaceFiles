// Service de recherche du résident (phase 3, lot 3 ; phase 4 lot A : branché sur le serveur multi-instances de `eco/`).
// Il répond à `search.query` sur le tube `\\.\pipe\Eco.spacefiles.<utilisateur>` : les autres applications (SpaceNotch) viennent
// chercher un fichier sans lancer l'Explorateur. Protocole : `docs/ecosystem/protocol.md`. La lecture des trames, le tube et
// ses instances sont dans `src/eco/` ; ce fichier ne garde que ce qui est propre à la recherche.
//
// Le résident ne garde aucun index au repos (~4 Mo privés) : la première `search.query` charge `index.bin` en arrière-plan
// (réponse « loading » en attendant), et l'index est libéré après 5 minutes sans requête par une minuterie à usage unique
// du résident (`minuteries::timer_liberation`).
//
// Tout ce qui est décision (réponse, moment de la libération) est une fonction pure, testée.
// Confidentialité : le journal ne reçoit jamais le texte cherché ni un chemin, seulement des durées, des nombres et des états.

use std::sync::atomic::{AtomicBool, AtomicIsize, AtomicU64, Ordering};
use std::sync::OnceLock;
use std::time::Instant;

use windows::Win32::Foundation::*;
use windows::Win32::UI::WindowsAndMessaging::{PostMessageW, WM_APP};

use crate::eco::serveur::Connexion;
use crate::eco::trame::{reponse_code, reponse_chargement, reponse_recherche, Resultat};
use crate::journal::{ecrire, Niveau};

/// Longueur minimale du texte cherché, comme la palette (sinon réponse vide)
const Q_MIN: usize = 3;
/// L'index est libéré après ce délai sans requête
const LIBERATION_DEFAUT_MS: u64 = 5 * 60 * 1000;

/// Prévient la fenêtre du résident qu'une requête vient d'arriver : elle réarme la minuterie de libération
/// (SetTimer sur une fenêtre ne s'appelle que depuis le fil qui l'a créée)
pub const WM_SERVICE_ACTIVITE: u32 = WM_APP + 23;

/// Le texte cherché est-il assez long ? (sinon réponse vide, comme la palette)
fn q_assez_long(q: &str) -> bool {
    q.trim().chars().count() >= Q_MIN
}

fn joindre(dossier: &str, nom: &str) -> String {
    crate::dossier::joindre(dossier, nom)
}

// ───────────────────────── Libération de l'index ─────────────────────────

#[derive(Debug, PartialEq)]
pub enum Decision {
    Liberer,
    /// Pas encore : millisecondes restantes avant l'échéance
    Attendre(u64),
}

/// Faut-il libérer l'index ? Tout est en millisecondes sur la même horloge
pub fn decision_liberation(dernier_appel_ms: u64, maintenant_ms: u64, delai_ms: u64) -> Decision {
    let ecoule = maintenant_ms.saturating_sub(dernier_appel_ms);
    if ecoule >= delai_ms {
        Decision::Liberer
    } else {
        Decision::Attendre(delai_ms - ecoule)
    }
}

/// Délai de libération : 5 minutes, ou `SPACEFILES_LIBERATION_S` (essais seulement, absente en usage normal)
fn delai_depuis(env: Option<&str>) -> u64 {
    env.and_then(|s| s.trim().parse::<u64>().ok()).filter(|s| (1..=86_400).contains(s)).map_or(LIBERATION_DEFAUT_MS, |s| s * 1000)
}

pub fn delai_liberation_ms() -> u64 {
    delai_depuis(std::env::var("SPACEFILES_LIBERATION_S").ok().as_deref())
}

static DEBUT: OnceLock<Instant> = OnceLock::new();
static DERNIER_APPEL: AtomicU64 = AtomicU64::new(0);

fn maintenant_ms() -> u64 {
    DEBUT.get_or_init(Instant::now).elapsed().as_millis() as u64
}

/// Minuterie sonnée : combien de millisecondes attendre encore (None : l'index est libéré ou n'était pas chargé)
pub fn au_declenchement() -> Option<u64> {
    match decision_liberation(DERNIER_APPEL.load(Ordering::SeqCst), maintenant_ms(), delai_liberation_ms()) {
        // Marge de 250 ms : l'horloge des minuteries Windows sonne parfois un peu tôt
        Decision::Attendre(reste) => Some(reste.max(250)),
        Decision::Liberer => {
            let charge = crate::index::actuel().map(|i| i.len());
            if crate::index::liberer() {
                if let Some(n) = charge {
                    ecrire(Niveau::Info, "service_recherche", &format!("index libere apres inactivite ({n} elements)"));
                }
                None
            } else {
                // Chargement ou construction en cours : on revient plus tard
                Some(5000)
            }
        }
    }
}

// ───────────────────────── Requêtes ─────────────────────────

/// Fenêtre du résident (pour lui signaler l'activité)
static FENETRE: AtomicIsize = AtomicIsize::new(0);
/// Un message d'activité est déjà dans la file du résident : pas de second tant qu'il n'est pas traité
static MESSAGE_POSTE: AtomicBool = AtomicBool::new(false);

/// Prépare le service (appelé par `eco::demarrer`, résident seulement)
pub fn initialiser(fenetre: HWND) {
    let _ = DEBUT.get_or_init(Instant::now);
    FENETRE.store(fenetre.0 as isize, Ordering::SeqCst);
}

/// Le résident se ferme : plus de message d'activité
pub fn arreter() {
    FENETRE.store(0, Ordering::SeqCst);
}

/// Le résident a traité le message d'activité (appelé par `minuteries::armer_liberation`)
pub fn activite_traitee() {
    MESSAGE_POSTE.store(false, Ordering::SeqCst);
}

fn signaler_activite() {
    // L'échéance de libération se lit dans DERNIER_APPEL : un message en attente suffit, même si d'autres requêtes suivent
    DERNIER_APPEL.store(maintenant_ms(), Ordering::SeqCst);
    let f = FENETRE.load(Ordering::SeqCst);
    if f != 0 && !MESSAGE_POSTE.swap(true, Ordering::SeqCst) {
        // SAFETY : PostMessageW accepte n'importe quel HWND ; s'il a disparu l'appel échoue sans effet.
        let posté = unsafe { PostMessageW(HWND(f as *mut _), WM_SERVICE_ACTIVITE, WPARAM(0), LPARAM(0)) }.is_ok();
        if !posté {
            MESSAGE_POSTE.store(false, Ordering::SeqCst);
        }
    }
}

/// Une requête plus lente que ceci est journalisée (durée et nombre de résultats seulement)
const REQUETE_LENTE_MS: u128 = 100;
/// Le premier « loading » d'un chargement est journalisé, pas les suivants
static CHARGEMENT_JOURNALISE: AtomicBool = AtomicBool::new(false);

/// Répond à `search.query`. None : abandonnée parce qu'une requête plus récente est déjà arrivée (aucune réponse).
pub fn traiter(id: &str, q: &str, limite: usize, c: &Connexion) -> Option<String> {
    signaler_activite();
    // Une requête plus récente est déjà complète dans le tampon : celle-ci est abandonnée sans réponse
    let r = if c.suivante_prete { None } else { rechercher(id, q, limite, c) };
    signaler_activite();
    r
}

/// Exécute une recherche. None : abandonnée parce qu'une nouvelle requête est arrivée (aucune réponse).
fn rechercher(id: &str, q: &str, limite: usize, c: &Connexion) -> Option<String> {
    if !q_assez_long(q) {
        return Some(reponse_recherche(id, &[], false));
    }
    // `contenu:` lirait des fichiers de l'utilisateur : réservé aux fenêtres de l'Explorateur
    if q.to_lowercase().contains("contenu:") {
        return Some(reponse_code(id, "non_supporte", "contenu: n'est pas pris en charge par ce service"));
    }
    let debut = Instant::now();
    let index = match crate::index::preparer_service() {
        crate::index::EtatService::Pret(i) => {
            CHARGEMENT_JOURNALISE.store(false, Ordering::Relaxed);
            i
        }
        crate::index::EtatService::Chargement => {
            if !CHARGEMENT_JOURNALISE.swap(true, Ordering::Relaxed) {
                ecrire(Niveau::Info, "service_recherche", "requete pendant le chargement de l'index");
            }
            return Some(reponse_chargement(id));
        }
        crate::index::EtatService::Efface => return Some(reponse_code(id, "index_efface", "l'index a ete supprime ; il sera recree par l'Explorateur")),
    };
    // limite + 1 : un résultat de plus dit qu'il y en avait d'autres (`truncated`)
    let annule = || c.arret_demande() || c.requete_en_attente();
    let entrees = match crate::index::chercher_dans(&index, q.trim(), limite + 1, &annule) {
        Ok(v) => v,
        Err(e) if e == "ANNULE" => {
            if debut.elapsed().as_millis() > REQUETE_LENTE_MS {
                ecrire(Niveau::Info, "service_recherche", &format!("requete lente abandonnee apres {} ms", debut.elapsed().as_millis()));
            }
            return None;
        }
        Err(_) => return Some(reponse_code(id, "index", "index indisponible")),
    };
    let tronque = entrees.len() > limite;
    let resultats: Vec<Resultat> = entrees
        .into_iter()
        .take(limite)
        .map(|e| {
            let dossier = e.parent.as_deref().unwrap_or("").to_string();
            (e.nom.clone(), joindre(&dossier, &e.nom), dossier)
        })
        .collect();
    let reponse = reponse_recherche(id, &resultats, tronque);
    if debut.elapsed().as_millis() > REQUETE_LENTE_MS {
        ecrire(Niveau::Info, "service_recherche", &format!("requete lente : {} resultats en {} ms", resultats.len(), debut.elapsed().as_millis()));
    }
    Some(reponse)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn q_trop_court() {
        assert!(!q_assez_long(""));
        assert!(!q_assez_long("a"));
        assert!(!q_assez_long("ab"));
        assert!(!q_assez_long("  a "));
        assert!(q_assez_long("abc"));
        // Caractères et non octets : trois lettres accentuées comptent pour trois
        assert!(q_assez_long("été"));
        assert!(!q_assez_long("éé"));
    }

    #[test]
    fn decision_de_liberation() {
        let cinq = 300_000;
        assert_eq!(decision_liberation(1_000, 1_000, cinq), Decision::Attendre(cinq));
        assert_eq!(decision_liberation(1_000, 100_000, cinq), Decision::Attendre(201_000));
        assert_eq!(decision_liberation(1_000, 300_999, cinq), Decision::Attendre(1));
        assert_eq!(decision_liberation(1_000, 301_000, cinq), Decision::Liberer);
        assert_eq!(decision_liberation(1_000, 9_000_000, cinq), Decision::Liberer);
        // Horloge qui recule : jamais de libération immédiate
        assert_eq!(decision_liberation(5_000, 1_000, cinq), Decision::Attendre(cinq));
    }

    #[test]
    fn delai_par_defaut_et_essai() {
        assert_eq!(delai_depuis(None), 300_000);
        assert_eq!(delai_depuis(Some("8")), 8_000);
        assert_eq!(delai_depuis(Some("0")), 300_000);
        assert_eq!(delai_depuis(Some("abc")), 300_000);
        assert_eq!(delai_depuis(Some("999999")), 300_000);
    }

    #[test]
    fn chemin_complet_d_un_resultat() {
        assert_eq!(joindre(r"C:\Users", "a.txt"), r"C:\Users\a.txt");
        assert_eq!(joindre(r"C:\", "a.txt"), r"C:\a.txt");
    }
}
