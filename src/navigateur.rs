// Téléchargements du navigateur : détection PASSIVE dans le dossier Téléchargements (aucun accès au navigateur, aucune
// extension, aucun réseau). Un navigateur écrit le fichier sous un nom partiel (`nom.crdownload` pour Chrome, Edge, Opera et
// Brave, `nom.part` pour Firefox) puis le renomme à la fin : la présence, la taille et la disparition de ce partiel disent
// tout ce qu'on sait. Pas de pourcentage (le total est inconnu), pas de vitesse sans deux mesures espacées d'une seconde.
//
// Deux couches. La logique pure (`classer`, `comparer`, `marquer`, les libellés) ne touche ni au disque ni à la fenêtre : elle
// est testée. Le reste est l'abonnement unique au dossier (un fil, aucune minuterie : il attend l'événement système, et ne se
// réveille une fois par seconde que tant qu'un téléchargement avance), l'état partagé avec la fenêtre, et les marques NOUVEAU.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use windows::core::PCWSTR;
use windows::Win32::Foundation::*;
use windows::Win32::Storage::FileSystem::*;
use windows::Win32::System::Threading::*;
use windows::Win32::UI::WindowsAndMessaging::{PostMessageW, WM_APP};

use crate::pastilles::{Pastille, PhaseTelechargement};
use crate::surveillance::Poignee;

/// L'état des téléchargements a changé (aucun paramètre : la fenêtre relit l'état partagé). Posté, jamais envoyé.
pub const WM_TELECHARGEMENT: u32 = WM_APP + 33;

/// Un fichier partiel dont la taille ne bouge plus depuis ce délai est « EN PAUSE »
pub const PAUSE: Duration = Duration::from_secs(30);
/// Une vitesse n'est affichée que calculée sur deux mesures espacées d'au moins ce délai
const MESURE_VITESSE: Duration = Duration::from_secs(1);
/// Entre deux lectures du dossier, au moins ce délai (et tant qu'un téléchargement avance, au plus ce délai sans lecture)
const LECTURE_MIN: Duration = Duration::from_secs(1);
/// Un partiel qu'on ne suivait pas et dont la dernière écriture date de plus que ça est un reste (navigateur fermé en plein
/// téléchargement) : il n'ouvre pas de pastille
const PERIME: Duration = Duration::from_secs(120);
/// Un fichier au nom final inconnu (Unconfirmed) est retrouvé s'il a été écrit depuis moins que ça
const RETROUVER: Duration = Duration::from_secs(120);
/// Un téléchargement en pause depuis ce délai est oublié : sa pastille ne doit pas rester indéfiniment. Le fichier partiel
/// reste sur le disque, mais n'étant plus écrit il est ignoré aux lectures suivantes (voir `PERIME`) ; s'il reprend, il revient.
pub const ABANDON: Duration = Duration::from_secs(600);
/// Tant que tous les téléchargements suivis sont en pause, le fil se réveille au plus toutes les 10 s (pour l'oubli), jamais à 1 s
const ATTENTE_PAUSE: Duration = Duration::from_secs(10);
/// Marques NOUVEAU gardées au plus
pub const MAX_NOUVEAUX: usize = 20;
/// Les clés des pastilles de téléchargement ont ce bit : jamais égales au numéro d'une opération de fichiers
pub const CLE_BASE: u64 = 1 << 62;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Navigateur {
    /// `.crdownload` : Chrome, Edge, Opera, Brave (on ne sait pas lequel)
    Chromium,
    /// `.part` : Firefox
    Firefox,
}

/// Un fichier partiel vu dans le dossier
#[derive(Clone, PartialEq, Debug)]
pub struct Partiel {
    /// Nom du fichier partiel tel qu'il est sur le disque
    pub nom_partiel: String,
    /// Nom que le fichier aura à la fin ; `None` pour `Unconfirmed 123456.crdownload` (le navigateur ne l'a pas encore décidé)
    pub nom_final: Option<String>,
    pub navigateur: Navigateur,
    pub taille: u64,
}

/// Le nom est-il celui d'un fichier partiel de navigateur ? La taille est à remplir par l'appelant.
/// Reconnus : `a.zip.crdownload`, `Unconfirmed 123.crdownload` (nom final inconnu), `a.zip.part`, quelle que soit la casse.
/// Pas reconnus : `a.crdownload.txt` (l'extension partielle doit être la dernière), `.part` ou `.crdownload` seuls, et les
/// partiels d'autres navigateurs (Safari `.download`, ancien Opera `.opdownload`, Internet Explorer `.partial`).
pub fn classer(nom: &str) -> Option<Partiel> {
    let (suffixe, navigateur) = if finit_par(nom, ".crdownload") {
        (".crdownload", Navigateur::Chromium)
    } else if finit_par(nom, ".part") {
        (".part", Navigateur::Firefox)
    } else {
        return None;
    };
    // Le suffixe est ASCII et commence par « . » : la coupe tombe sur une frontière de caractère
    let base = nom.get(..nom.len() - suffixe.len())?;
    if base.is_empty() {
        return None;
    }
    let inconnu = navigateur == Navigateur::Chromium && est_unconfirmed(base);
    Some(Partiel { nom_partiel: nom.to_string(), nom_final: (!inconnu).then(|| base.to_string()), navigateur, taille: 0 })
}

/// `nom` finit-il par `suffixe` (ASCII), sans tenir compte de la casse ?
fn finit_par(nom: &str, suffixe: &str) -> bool {
    nom.len() >= suffixe.len() && nom.as_bytes()[nom.len() - suffixe.len()..].eq_ignore_ascii_case(suffixe.as_bytes())
}

/// « Unconfirmed 123456 » : le nom provisoire de Chrome et d'Edge avant que le nom final soit connu
fn est_unconfirmed(base: &str) -> bool {
    let b = base.as_bytes();
    let mot = b"unconfirmed ";
    b.len() > mot.len() && b[..mot.len()].eq_ignore_ascii_case(mot) && b[mot.len()..].iter().all(u8::is_ascii_digit)
}

/// Un téléchargement suivi : son partiel, depuis quand, et ce qu'on a mesuré
#[derive(Clone, PartialEq, Debug)]
pub struct Suivi {
    /// Identité stable (un compteur du processus) : elle survit au renommage d'un `Unconfirmed` et devient la clé de la pastille
    pub id: u64,
    pub partiel: Partiel,
    pub debut: Instant,
    /// Dernière fois où la taille a changé
    pub derniere_variation: Instant,
    pub en_pause: bool,
    /// L'utilisateur a masqué la pastille (✕) : le suivi continue, la pastille ne revient pas
    pub masque: bool,
    /// Octets par seconde, mesurés entre deux relevés espacés d'au moins une seconde ; `None` tant qu'on ne sait pas
    pub vitesse: Option<u64>,
    mesure: (u64, Instant),
}

impl Suivi {
    fn neuf(id: u64, partiel: Partiel, maintenant: Instant) -> Self {
        let mesure = (partiel.taille, maintenant);
        Suivi { id, partiel, debut: maintenant, derniere_variation: maintenant, en_pause: false, masque: false, vitesse: None, mesure }
    }
}

#[derive(Clone, PartialEq, Debug)]
pub enum Evenement {
    /// Un partiel apparaît
    Commence { id: u64, nom_final: Option<String>, navigateur: Navigateur },
    /// La taille a changé
    Progresse { id: u64, taille: u64 },
    /// La taille ne bouge plus depuis 30 s
    EnPause { id: u64 },
    /// Le partiel a disparu et le fichier au nom final existe
    Termine { id: u64, nom: String, taille: u64 },
    /// Le partiel a disparu sans que le nom final apparaisse (annulé, ou échec : on ne sait pas)
    Annule { id: u64, nom_final: Option<String> },
    /// En pause depuis `ABANDON` : le suivi s'arrête sans pastille de fin
    Oublie { id: u64 },
}

/// Combien de temps le fil peut dormir sans événement du dossier : une seconde tant qu'un téléchargement n'est pas en pause
/// (pour voir la pause arriver et mesurer la vitesse), 10 s si tous sont en pause (pour les oublier), `None` (attendre
/// l'événement système seul) quand rien n'est suivi.
pub fn delai_attente(suivis: &[Suivi]) -> Option<Duration> {
    if suivis.iter().any(|s| !s.en_pause) {
        Some(LECTURE_MIN)
    } else if !suivis.is_empty() {
        Some(ATTENTE_PAUSE)
    } else {
        None
    }
}

/// Dossier à surveiller. Usage normal : Téléchargements de l'utilisateur. Mode essai (`SPACEFILES_SANS_REGISTRE=1`) : seulement
/// `SPACEFILES_TELECHARGEMENTS`, chemin absolu d'un dossier existant ; sans elle, rien n'est surveillé (jamais le vrai dossier).
/// Hors mode essai la variable est ignorée.
pub fn choisir_dossier(sans_registre: bool, variable: Option<&str>, reel: impl FnOnce() -> Option<String>, est_dossier: impl Fn(&str) -> bool) -> Option<String> {
    if !sans_registre {
        return reel();
    }
    let d = variable.map(str::trim).filter(|d| !d.is_empty())?;
    (std::path::Path::new(d).is_absolute() && est_dossier(d)).then(|| d.to_string())
}

/// Compare ce qu'on suivait (`avant`) à ce que le dossier contient (`apres`, les partiels seuls) et dit ce qui s'est passé.
/// Rend aussi le nouvel état suivi. `retrouver` répond, pour un partiel disparu, au fichier fini : son nom et sa taille.
/// Règles : même nom de partiel = même téléchargement ; un `Unconfirmed` qui disparaît pendant qu'un partiel au nom connu
/// apparaît (taille au moins égale, même famille de navigateur) est un renommage, pas une fin ; un partiel disparu dont le
/// fichier final existe (et n'est pas vide alors que le partiel ne l'était pas) est terminé, sinon annulé.
pub fn comparer(avant: &[Suivi], apres: &[Partiel], suivant_id: &mut u64, retrouver: &dyn Fn(&Suivi) -> Option<(String, u64)>, maintenant: Instant) -> (Vec<Suivi>, Vec<Evenement>) {
    let mut suivis = Vec::new();
    let mut evenements = Vec::new();
    let mut apparies = vec![false; avant.len()];
    let mut inconnus: Vec<&Partiel> = Vec::new();
    for p in apres {
        match avant.iter().position(|s| s.partiel.nom_partiel.eq_ignore_ascii_case(&p.nom_partiel)) {
            Some(k) => {
                apparies[k] = true;
                let s = &avant[k];
                // En pause depuis trop longtemps : oublié (le partiel, toujours là mais figé, ne revient pas avant d'être réécrit)
                if s.en_pause && p.taille == s.partiel.taille && maintenant.duration_since(s.derniere_variation) >= ABANDON {
                    evenements.push(Evenement::Oublie { id: s.id });
                } else {
                    suivis.push(continuer(s, p, maintenant, &mut evenements));
                }
            }
            None => inconnus.push(p),
        }
    }
    let mut neufs: Vec<&Partiel> = Vec::new();
    for p in inconnus {
        // Un Unconfirmed disparu pendant qu'un partiel au nom connu apparaît (même famille, au moins aussi gros) : c'est lui, et
        // cela passe avant toute recherche d'un fichier fini. Un Unconfirmed vide ne se rattache à rien : rien ne dit que c'est lui.
        let renomme = p.nom_final.is_some().then(|| (0..avant.len()).find(|&k| !apparies[k] && avant[k].partiel.nom_final.is_none() && avant[k].partiel.navigateur == p.navigateur && avant[k].partiel.taille > 0 && p.taille >= avant[k].partiel.taille)).flatten();
        match renomme {
            Some(k) => {
                apparies[k] = true;
                suivis.push(continuer(&avant[k], p, maintenant, &mut evenements));
            }
            None => neufs.push(p),
        }
    }
    for p in neufs {
        let id = *suivant_id;
        *suivant_id += 1;
        evenements.push(Evenement::Commence { id, nom_final: p.nom_final.clone(), navigateur: p.navigateur });
        suivis.push(Suivi::neuf(id, p.clone(), maintenant));
    }
    for s in avant.iter().enumerate().filter(|(k, _)| !apparies[*k]).map(|(_, s)| s) {
        // Nom final inconnu et taille 0 : aucun fichier ne peut être reconnu (n'importe quel fichier vide irait) : annulé
        if s.partiel.nom_final.is_none() && s.partiel.taille == 0 {
            evenements.push(Evenement::Annule { id: s.id, nom_final: None });
            continue;
        }
        match retrouver(s) {
            // Firefox crée le fichier final vide dès le départ : vide à la fin d'un partiel qui ne l'était pas, c'est une annulation
            Some((nom, taille)) if taille > 0 || s.partiel.taille == 0 => evenements.push(Evenement::Termine { id: s.id, nom, taille }),
            _ => evenements.push(Evenement::Annule { id: s.id, nom_final: s.partiel.nom_final.clone() }),
        }
    }
    (suivis, evenements)
}

/// Le même téléchargement une lecture plus tard
fn continuer(s: &Suivi, p: &Partiel, maintenant: Instant, evenements: &mut Vec<Evenement>) -> Suivi {
    let mut n = s.clone();
    n.partiel = p.clone();
    if p.taille != s.partiel.taille {
        n.derniere_variation = maintenant;
        n.en_pause = false;
        evenements.push(Evenement::Progresse { id: s.id, taille: p.taille });
    } else if !s.en_pause && maintenant.duration_since(s.derniere_variation) >= PAUSE {
        n.en_pause = true;
        evenements.push(Evenement::EnPause { id: s.id });
    }
    let ecart = maintenant.duration_since(s.mesure.1);
    if ecart >= MESURE_VITESSE {
        n.vitesse = (p.taille > s.mesure.0).then(|| ((p.taille - s.mesure.0) as f64 / ecart.as_secs_f64()) as u64);
        n.mesure = (p.taille, maintenant);
    }
    if n.en_pause {
        n.vitesse = None;
    }
    n
}

/// Ajoute une marque NOUVEAU (dossier normalisé, nom en minuscules) : déjà présente, elle passe en dernier ; au-delà de
/// `MAX_NOUVEAUX`, la plus ancienne part
pub fn marquer(liste: &mut Vec<(String, String)>, dossier: &str, nom: &str) {
    let cle = (normaliser(dossier), nom.to_lowercase());
    liste.retain(|x| *x != cle);
    liste.push(cle);
    while liste.len() > MAX_NOUVEAUX {
        liste.remove(0);
    }
}

/// Dossier comparé sans casse ni `\` final
fn normaliser(dossier: &str) -> String {
    dossier.trim_end_matches('\\').to_lowercase()
}

/// Texte de droite d'une pastille en cours : « 12,4 MO », « 12,4 MO · 1,2 MO/S » (vitesse seulement si mesurée),
/// « EN PAUSE · 12,4 MO »
pub fn etiquette_en_cours(taille: u64, vitesse: Option<u64>, en_pause: bool) -> String {
    let t = crate::systeme::taille(taille);
    if en_pause {
        return format!("EN PAUSE · {t}");
    }
    match vitesse {
        Some(v) => format!("{t} · {}/S", crate::systeme::taille(v)),
        None => t,
    }
}

/// Un téléchargement fini ou annulé, montré en pastille quelques secondes
struct Fini {
    id: u64,
    nom: Option<String>,
    chemin: Option<String>,
    taille: u64,
    annule: bool,
    quand: Instant,
}

/// Tout ce que la fenêtre lit : les téléchargements en cours, les finis, les marques NOUVEAU
struct Etat {
    suivis: Vec<Suivi>,
    finis: Vec<Fini>,
    /// (dossier normalisé, nom en minuscules), du plus ancien au plus récent
    nouveaux: Vec<(String, String)>,
    prochain_id: u64,
    /// Change à chaque arrêt de l'abonnement : un résultat d'un fil arrêté est jeté
    generation: u64,
}

static ETAT: Mutex<Etat> = Mutex::new(Etat { suivis: Vec::new(), finis: Vec::new(), nouveaux: Vec::new(), prochain_id: 1, generation: 0 });
/// Une marque NOUVEAU a changé : la fenêtre repeint tout (la liste), pas seulement la bande des pastilles
static MARQUES_MODIFIEES: AtomicBool = AtomicBool::new(false);

fn etat() -> MutexGuard<'static, Etat> {
    ETAT.lock().unwrap_or_else(|e| e.into_inner())
}

/// Vrai une fois après un changement de marque NOUVEAU
pub fn marques_modifiees() -> bool {
    MARQUES_MODIFIEES.swap(false, Ordering::Relaxed)
}

/// Une pastille de téléchargement a-t-elle cette clé ?
pub fn est_cle(cle: u64) -> bool {
    cle & CLE_BASE != 0
}

/// Pastilles des téléchargements : ceux qui avancent, puis les finis et annulés encore frais
pub fn pastilles() -> Vec<Pastille> {
    let e = etat();
    let mut v = Vec::new();
    for s in e.suivis.iter().filter(|s| !s.masque) {
        let libelle = s.partiel.nom_final.clone().unwrap_or_else(|| "TÉLÉCHARGEMENT".to_string());
        let phase = if s.en_pause { PhaseTelechargement::EnPause } else { PhaseTelechargement::EnCours };
        v.push(Pastille::telechargement(phase, libelle, etiquette_en_cours(s.partiel.taille, s.vitesse, s.en_pause), CLE_BASE | s.id));
    }
    for f in &e.finis {
        let phase = if f.annule { PhaseTelechargement::Annule } else { PhaseTelechargement::Fini };
        if crate::pastilles::expiree(crate::pastilles::Genre::Telechargement(phase), f.quand.elapsed().as_millis()) {
            continue;
        }
        let libelle = f.nom.clone().unwrap_or_else(|| "TÉLÉCHARGEMENT".to_string());
        let etiquette = if f.annule { "ANNULÉ".to_string() } else { format!("TÉLÉCHARGÉ · {}", crate::systeme::taille(f.taille)) };
        v.push(Pastille::telechargement(phase, libelle, etiquette, CLE_BASE | f.id));
    }
    v
}

/// Un téléchargement avance (et n'est pas en pause) : Pixel est en activité, la grille 3 × 3 tourne
pub fn avance() -> bool {
    etat().suivis.iter().any(en_activite)
}

/// Compte pour Pixel, la grille et la minuterie d'activité : ni en pause ni masqué (sa pastille n'est pas à l'écran)
pub fn en_activite(s: &Suivi) -> bool {
    !s.en_pause && !s.masque
}

/// Depuis quand le plus ancien téléchargement qui avance est suivi (phase de la grille et de Pixel)
pub fn debut() -> Option<Instant> {
    etat().suivis.iter().filter(|s| en_activite(s)).map(|s| s.debut).min()
}

/// Un téléchargement s'est terminé il y a moins que `ms` (Pixel prend la forme « réussite »)
pub fn reussite_recente(ms: u128) -> bool {
    etat().finis.iter().any(|f| !f.annule && f.quand.elapsed().as_millis() < ms)
}

/// Une pastille de fin ou d'annulation attend d'être effacée
pub fn temporaires() -> bool {
    !etat().finis.is_empty()
}

/// Retire les finis périmés ; vrai s'il en reste un
pub fn expirer() -> bool {
    let mut e = etat();
    e.finis.retain(|f| {
        let phase = if f.annule { PhaseTelechargement::Annule } else { PhaseTelechargement::Fini };
        !crate::pastilles::expiree(crate::pastilles::Genre::Telechargement(phase), f.quand.elapsed().as_millis())
    });
    !e.finis.is_empty()
}

/// ✕ sur une pastille : un fini disparaît ; un téléchargement en cours est seulement masqué (le navigateur n'est pas arrêtable)
pub fn fermer(cle: u64) {
    let id = cle & !CLE_BASE;
    let mut e = etat();
    e.finis.retain(|f| f.id != id);
    for s in e.suivis.iter_mut().filter(|s| s.id == id) {
        s.masque = true;
    }
}

/// Chemin complet du fichier d'une pastille « TÉLÉCHARGÉ » (le bouton AFFICHER)
pub fn chemin_fini(cle: u64) -> Option<String> {
    let id = cle & !CLE_BASE;
    etat().finis.iter().find(|f| f.id == id).and_then(|f| f.chemin.clone())
}

/// Au moins une marque NOUVEAU existe (test rapide avant de chercher)
pub fn a_des_nouveaux() -> bool {
    !etat().nouveaux.is_empty()
}

/// Noms (en minuscules) marqués NOUVEAU dans ce dossier
pub fn nouveaux_dans(dossier: &str) -> Vec<String> {
    let e = etat();
    if e.nouveaux.is_empty() {
        return Vec::new();
    }
    let d = normaliser(dossier);
    e.nouveaux.iter().filter(|(x, _)| *x == d).map(|(_, n)| n.clone()).collect()
}

/// Le fichier est sélectionné ou ouvert : sa marque NOUVEAU disparaît
pub fn oublier(dossier: &str, nom: &str) {
    let mut e = etat();
    if e.nouveaux.is_empty() {
        return;
    }
    let cle = (normaliser(dossier), nom.to_lowercase());
    let avant = e.nouveaux.len();
    e.nouveaux.retain(|x| *x != cle);
    if e.nouveaux.len() != avant {
        MARQUES_MODIFIEES.store(true, Ordering::Relaxed);
    }
}

/// Applique le résultat d'une lecture à l'état partagé. `generation` : celle de l'abonnement qui a lu (un abonnement
/// arrêté depuis n'écrit plus rien). Vrai si l'état a changé.
fn appliquer(generation: u64, dossier: &str, suivis: Vec<Suivi>, evenements: Vec<Evenement>, maintenant: Instant) -> bool {
    let mut e = etat();
    if e.generation != generation {
        return false;
    }
    let change = !evenements.is_empty() || e.suivis != suivis;
    for ev in &evenements {
        match ev {
            Evenement::Termine { id, nom, taille } => {
                let masque = e.suivis.iter().any(|s| s.id == *id && s.masque);
                marquer(&mut e.nouveaux, dossier, nom);
                MARQUES_MODIFIEES.store(true, Ordering::Relaxed);
                if !masque {
                    e.finis.push(Fini { id: *id, nom: Some(nom.clone()), chemin: Some(crate::dossier::joindre(dossier, nom)), taille: *taille, annule: false, quand: maintenant });
                }
            }
            Evenement::Annule { id, nom_final } => {
                let masque = e.suivis.iter().any(|s| s.id == *id && s.masque);
                if !masque {
                    e.finis.push(Fini { id: *id, nom: nom_final.clone(), chemin: None, taille: 0, annule: true, quand: maintenant });
                }
            }
            Evenement::Commence { .. } | Evenement::Progresse { .. } | Evenement::EnPause { .. } | Evenement::Oublie { .. } => {}
        }
    }
    // Un ✕ posé pendant la lecture n'est pas perdu
    let mut suivis = suivis;
    for s in &mut suivis {
        s.masque = e.suivis.iter().any(|x| x.id == s.id && x.masque);
    }
    e.suivis = suivis;
    change
}

// ── Abonnement au dossier ────────────────────────────────────────────────────────────────────────

/// Tant qu'il existe, le dossier Téléchargements est surveillé (un seul pour tout le processus). Le détruire arrête le fil.
struct Abonnement {
    arret: Arc<Poignee>,
    /// Génération de l'état pour laquelle le fil écrit : un fil qui meurt ne retire que son propre abonnement
    generation: u64,
}

impl Drop for Abonnement {
    fn drop(&mut self) {
        // SAFETY: `arret` est un événement valide tant que l'Arc vit ; SetEvent ne garde aucune référence
        unsafe {
            let _ = SetEvent(self.arret.0);
        }
    }
}

static ABONNEMENT: Mutex<Option<Abonnement>> = Mutex::new(None);

/// Dossier à surveiller (règles dans `choisir_dossier`). `SPACEFILES_TELECHARGEMENTS` n'est lue qu'avec `SPACEFILES_SANS_REGISTRE=1`,
/// le mode d'essai des faux profils : un essai ne touche jamais au vrai dossier. Le dossier choisi est écrit au journal.
fn dossier_surveille() -> Option<String> {
    let sans_registre = std::env::var("SPACEFILES_SANS_REGISTRE").as_deref() == Ok("1");
    let variable = std::env::var("SPACEFILES_TELECHARGEMENTS").ok();
    let d = choisir_dossier(sans_registre, variable.as_deref(), crate::systeme::chemin_telechargements, |d| std::path::Path::new(d).is_dir());
    match &d {
        Some(d) => crate::journal::ecrire(crate::journal::Niveau::Info, "navigateur", &format!("dossier surveille : {d}")),
        None => crate::journal::ecrire(crate::journal::Niveau::Info, "navigateur", "aucun dossier a surveiller"),
    }
    d
}

/// Met l'abonnement en accord avec le réglage `telechargements_navigateur` : démarré s'il est vrai, détruit (avec l'état :
/// pastilles et marques) s'il est faux. Idempotent. Fil d'interface.
pub fn regler(hwnd: HWND) {
    if crate::reglages::actuels().telechargements_navigateur {
        demarrer(hwnd);
    } else {
        arreter();
    }
}

fn demarrer(hwnd: HWND) {
    let mut a = ABONNEMENT.lock().unwrap_or_else(|e| e.into_inner());
    if a.is_some() {
        return;
    }
    let Some(dossier) = dossier_surveille() else { return };
    // SAFETY: création d'un événement sans nom ; le handle est possédé par `Poignee` (fermé au dernier Arc)
    let Some(arret) = (unsafe { CreateEventW(None, TRUE, FALSE, PCWSTR::null()).ok() }) else { return };
    let arret = Arc::new(Poignee(arret));
    let generation = etat().generation;
    let (arret_fil, fenetre) = (arret.clone(), hwnd.0 as isize);
    // Un fil qui ne démarre pas (mémoire, limite du système) ne laisse pas d'abonnement fantôme : le prochain `regler` réessaie
    let lance = std::thread::Builder::new().name("telechargements".into()).spawn(move || boucle(dossier, fenetre, arret_fil, generation));
    if let Err(e) = lance {
        crate::journal::ecrire(crate::journal::Niveau::Avert, "navigateur", &format!("fil non demarre : {e}"));
        return;
    }
    *a = Some(Abonnement { arret, generation });
}

/// Le fil s'arrête sans qu'on le lui ait demandé (dossier supprimé ou partage coupé, attente en échec) : si c'est encore
/// l'abonnement courant, il est retiré (le prochain `regler` en relance un), les suivis sont vidés, la fenêtre prévenue et la
/// cause écrite au journal. Un abonnement déjà arrêté ou remplacé (génération différente) ne touche à rien.
fn fil_mort(generation: u64, fenetre: isize, cause: &str) {
    let mien = {
        let mut a = ABONNEMENT.lock().unwrap_or_else(|e| e.into_inner());
        if a.as_ref().is_some_and(|x| x.generation == generation) {
            a.take()
        } else {
            None
        }
    };
    if mien.is_none() {
        return;
    }
    {
        let mut e = etat();
        if e.generation != generation {
            return;
        }
        e.generation += 1;
        e.suivis.clear();
    }
    crate::journal::ecrire(crate::journal::Niveau::Avert, "navigateur", &format!("surveillance arretee : {cause}"));
    // SAFETY: PostMessageW ne garde rien ; un `hwnd` détruit fait seulement échouer l'appel
    unsafe {
        let _ = PostMessageW(HWND(fenetre as *mut _), WM_TELECHARGEMENT, WPARAM(0), LPARAM(0));
    }
}

/// Coupe l'abonnement (le fil s'arrête) et oublie tout : plus de pastille ni de marque. Aussi appelé à la fermeture de la fenêtre.
pub fn arreter() {
    let ancien = ABONNEMENT.lock().unwrap_or_else(|e| e.into_inner()).take();
    if ancien.is_some() {
        let mut e = etat();
        e.generation += 1;
        e.suivis.clear();
        e.finis.clear();
        e.nouveaux.clear();
        MARQUES_MODIFIEES.store(true, Ordering::Relaxed);
    }
    drop(ancien);
}

/// Partiels du dossier avec leur taille. Seuls les noms finissant par `.crdownload` / `.part` sont regardés de plus près (une
/// ouverture d'attributs, jamais du contenu) ; un partiel qu'on ne suivait pas et qui n'a pas été écrit récemment est ignoré.
fn lire_partiels(dossier: &str, suivis: &[Suivi]) -> Vec<Partiel> {
    let Ok(it) = std::fs::read_dir(dossier) else { return Vec::new() };
    let mut v = Vec::new();
    for entree in it.flatten() {
        let nom = entree.file_name().to_string_lossy().into_owned();
        let Some(mut p) = classer(&nom) else { continue };
        // La taille lue dans la liste du dossier est celle d'avant l'écriture en cours : on ouvre les attributs du fichier
        let Ok(m) = std::fs::metadata(entree.path()) else { continue };
        if !m.is_file() {
            continue;
        }
        p.taille = m.len();
        let suivi = suivis.iter().any(|s| s.partiel.nom_partiel.eq_ignore_ascii_case(&p.nom_partiel));
        if !suivi && m.modified().ok().and_then(|t| t.elapsed().ok()).is_some_and(|age| age > PERIME) {
            continue;
        }
        v.push(p);
    }
    v
}

/// Le fichier fini d'un partiel disparu : son nom et sa taille. Nom final connu : ce fichier-là. Inconnu (Unconfirmed) : le
/// fichier ordinaire de même taille écrit le plus récemment (un renommage garde la taille et la date d'écriture).
fn retrouver(dossier: &str, s: &Suivi) -> Option<(String, u64)> {
    if let Some(nom) = &s.partiel.nom_final {
        let m = std::fs::metadata(crate::dossier::joindre(dossier, nom)).ok().filter(|m| m.is_file())?;
        return Some((nom.clone(), m.len()));
    }
    // Une taille nulle ne désigne aucun fichier (`comparer` ne demande pas, mais la fonction ne doit pas deviner)
    if s.partiel.taille == 0 {
        return None;
    }
    let mut meilleur: Option<(Duration, String)> = None;
    for entree in std::fs::read_dir(dossier).ok()?.flatten() {
        let nom = entree.file_name().to_string_lossy().into_owned();
        if classer(&nom).is_some() {
            continue;
        }
        let Ok(m) = entree.metadata() else { continue };
        if !m.is_file() || m.len() != s.partiel.taille {
            continue;
        }
        let Some(age) = m.modified().ok().and_then(|t| t.elapsed().ok()).filter(|a| *a <= RETROUVER) else { continue };
        if meilleur.as_ref().map_or(true, |(a, _)| age < *a) {
            meilleur = Some((age, nom));
        }
    }
    meilleur.map(|(_, nom)| (nom, s.partiel.taille))
}

/// Attente sur la notification du dossier et l'événement d'arrêt
fn attendre(handles: &[HANDLE], ms: u32) -> WAIT_EVENT {
    // SAFETY: les handles sont valides pour l'appel : la notification est ouverte par `boucle` jusqu'à son retour, et l'événement
    // d'arrêt vit tant que l'Arc de l'appelant existe ; WaitForMultipleObjects ne garde aucune référence
    unsafe { WaitForMultipleObjects(handles, FALSE, ms) }
}

/// L'événement `h` est-il signalé (après au plus `ms` ms d'attente) ?
fn signale(h: HANDLE, ms: u32) -> bool {
    // SAFETY: `h` est l'événement d'arrêt, valide tant que l'Arc de l'appelant vit
    unsafe { WaitForSingleObject(h, ms) == WAIT_OBJECT_0 }
}

/// Le fil de l'abonnement. Il dort sur l'événement système (changement de nom ou de taille dans le dossier, non récursif) :
/// aucune minuterie au repos. Il se réveille aussi seul selon `delai_attente` : une seconde tant qu'un téléchargement n'est pas
/// en pause, 10 s si tous le sont, jamais si rien n'est suivi. Le dossier est relu au plus une fois par seconde, rafales
/// regroupées sur 150 ms et au plus 1 s après le premier signal. Une sortie qu'on n'a pas demandée passe par `fil_mort`.
fn boucle(dossier: String, fenetre: isize, arret: Arc<Poignee>, generation: u64) {
    let chemin_w: Vec<u16> = dossier.encode_utf16().chain(std::iter::once(0)).collect();
    let filtre = FILE_NOTIFY_CHANGE_FILE_NAME | FILE_NOTIFY_CHANGE_SIZE;
    // SAFETY: `chemin_w` se termine par 0 et vit pendant l'appel ; le handle rendu est fermé plus bas, avant le retour
    let ouvert = unsafe { FindFirstChangeNotificationW(PCWSTR(chemin_w.as_ptr()), FALSE, filtre) };
    let changement = match ouvert {
        Ok(c) => c,
        Err(e) => return fil_mort(generation, fenetre, &format!("ouverture du dossier impossible : {e}")),
    };
    let cause = surveiller(&dossier, fenetre, &arret, generation, changement);
    // SAFETY: `changement` vient de FindFirstChangeNotificationW ci-dessus et n'est fermé qu'ici, une seule fois
    unsafe {
        let _ = FindCloseChangeNotification(changement);
    }
    if let Some(c) = cause {
        fil_mort(generation, fenetre, &c);
    }
}

/// Corps de la boucle ; `None` : arrêt demandé (ou génération périmée), `Some(cause)` : sortie anormale
fn surveiller(dossier: &str, fenetre: isize, arret: &Arc<Poignee>, generation: u64, changement: HANDLE) -> Option<String> {
    let mut derniere: Option<Instant> = None;
    let mut a_lire = true;
    loop {
        if !a_lire {
            let delai = delai_attente(&etat().suivis);
            let r = attendre(&[changement, arret.0], delai.map_or(INFINITE, |d| d.as_millis() as u32));
            if r == WAIT_OBJECT_0 {
                // Rafale (une copie, un téléchargement qui écrit) : on attend un peu et on regroupe, mais pas au-delà d'une
                // seconde après le premier signal (une écriture continue ne doit pas repousser la lecture sans fin)
                let premier = Instant::now();
                loop {
                    // SAFETY: `changement` est ouvert (voir `boucle`) ; réarme la notification
                    if unsafe { FindNextChangeNotification(changement) }.is_err() {
                        return Some("notification du dossier perdue (dossier supprime ou partage coupe)".into());
                    }
                    let reste = LECTURE_MIN.saturating_sub(premier.elapsed());
                    if reste.is_zero() || attendre(&[changement, arret.0], reste.as_millis().min(150) as u32) != WAIT_OBJECT_0 {
                        break;
                    }
                }
            } else if r == WAIT_EVENT(WAIT_OBJECT_0.0 + 1) {
                return None;
            } else if r != WAIT_TIMEOUT {
                return Some(format!("attente en echec ({})", r.0));
            }
        }
        a_lire = false;
        if signale(arret.0, 0) {
            return None;
        }
        // Au plus une lecture par seconde
        if let Some(d) = derniere {
            let reste = LECTURE_MIN.saturating_sub(d.elapsed());
            if !reste.is_zero() && signale(arret.0, reste.as_millis() as u32) {
                return None;
            }
        }
        derniere = Some(Instant::now());
        let (avant, mut suivant_id) = {
            let e = etat();
            (e.suivis.clone(), e.prochain_id)
        };
        let partiels = lire_partiels(dossier, &avant);
        let maintenant = Instant::now();
        let (suivis, evenements) = comparer(&avant, &partiels, &mut suivant_id, &|s| retrouver(dossier, s), maintenant);
        {
            // Le compteur d'identités avance sous le même verrou que l'état
            let mut e = etat();
            if e.generation != generation {
                return None;
            }
            e.prochain_id = e.prochain_id.max(suivant_id);
        }
        let change = appliquer(generation, dossier, suivis, evenements, maintenant);
        // La génération est relue juste avant de poster : un abonnement arrêté entre-temps ne réveille plus la fenêtre
        if change && etat().generation == generation {
            // SAFETY: PostMessageW ne garde rien ; un `hwnd` détruit fait seulement échouer l'appel
            unsafe {
                let _ = PostMessageW(HWND(fenetre as *mut _), WM_TELECHARGEMENT, WPARAM(0), LPARAM(0));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn partiel(nom: &str, taille: u64) -> Partiel {
        Partiel { taille, ..classer(nom).expect("partiel") }
    }

    /// Le fichier fini n'existe pas
    fn rien(_: &Suivi) -> Option<(String, u64)> {
        None
    }

    fn lancer(partiels: &[Partiel], t0: Instant) -> (Vec<Suivi>, u64) {
        let mut id = 1;
        let (s, e) = comparer(&[], partiels, &mut id, &rien, t0);
        assert!(e.iter().all(|x| matches!(x, Evenement::Commence { .. })));
        (s, id)
    }

    #[test]
    fn noms_de_partiels_reconnus() {
        let p = classer("a.zip.crdownload").unwrap();
        assert_eq!((p.nom_final.as_deref(), p.navigateur, p.nom_partiel.as_str()), (Some("a.zip"), Navigateur::Chromium, "a.zip.crdownload"));
        let p = classer("a.zip.part").unwrap();
        assert_eq!((p.nom_final.as_deref(), p.navigateur), (Some("a.zip"), Navigateur::Firefox));
        // Nom sans extension
        assert_eq!(classer("LISEZMOI.part").unwrap().nom_final.as_deref(), Some("LISEZMOI"));
        assert_eq!(classer("notes.crdownload").unwrap().nom_final.as_deref(), Some("notes"));
        // Majuscules
        assert_eq!(classer("Photo.JPG.CRDOWNLOAD").unwrap().nom_final.as_deref(), Some("Photo.JPG"));
        assert_eq!(classer("Photo.JPG.PART").unwrap().navigateur, Navigateur::Firefox);
        // Accents : la coupe reste sur une frontière de caractère
        assert_eq!(classer("été.zip.part").unwrap().nom_final.as_deref(), Some("été.zip"));
    }

    #[test]
    fn unconfirmed_a_un_nom_final_inconnu() {
        for n in ["Unconfirmed 123456.crdownload", "unconfirmed 7.crdownload", "UNCONFIRMED 99.CRDOWNLOAD"] {
            let p = classer(n).unwrap();
            assert_eq!(p.nom_final, None, "{n}");
            assert_eq!(p.navigateur, Navigateur::Chromium);
        }
        // Pas un Unconfirmed : un vrai nom de fichier
        assert_eq!(classer("Unconfirmed notes.crdownload").unwrap().nom_final.as_deref(), Some("Unconfirmed notes"));
        assert_eq!(classer("Unconfirmed 12.part").unwrap().nom_final.as_deref(), Some("Unconfirmed 12"));
        assert_eq!(classer("Unconfirmed .crdownload").unwrap().nom_final.as_deref(), Some("Unconfirmed "));
    }

    #[test]
    fn noms_qui_ne_sont_pas_des_partiels() {
        for n in ["a.crdownload.txt", "a.zip", "a.part.zip", "partie", "a.partial", "a.download", "a.opdownload", "a.part ", "", ".part", ".crdownload", "crdownload", "a.crdown"] {
            assert!(classer(n).is_none(), "{n:?} ne doit pas être reconnu");
        }
    }

    #[test]
    fn commence_progresse_puis_termine() {
        let t0 = Instant::now();
        let (suivis, mut id) = lancer(&[partiel("a.zip.crdownload", 100)], t0);
        assert_eq!(suivis.len(), 1);
        assert_eq!(suivis[0].id, 1);
        // La taille grandit : Progresse, pas de pause
        let t1 = t0 + Duration::from_secs(2);
        let (s2, e) = comparer(&suivis, &[partiel("a.zip.crdownload", 5000)], &mut id, &rien, t1);
        assert_eq!(e, vec![Evenement::Progresse { id: 1, taille: 5000 }]);
        assert_eq!(s2[0].derniere_variation, t1);
        // Le partiel disparaît et a.zip existe : Termine avec la taille finale
        let existe = |s: &Suivi| (s.partiel.nom_final.as_deref() == Some("a.zip")).then(|| ("a.zip".to_string(), 9000));
        let (s3, e) = comparer(&s2, &[], &mut id, &existe, t1 + Duration::from_secs(1));
        assert!(s3.is_empty());
        assert_eq!(e, vec![Evenement::Termine { id: 1, nom: "a.zip".into(), taille: 9000 }]);
    }

    #[test]
    fn rien_ne_change_rien_a_dire() {
        let t0 = Instant::now();
        let (suivis, mut id) = lancer(&[partiel("a.zip.part", 10)], t0);
        let (s, e) = comparer(&suivis, &[partiel("a.zip.part", 10)], &mut id, &rien, t0 + Duration::from_secs(5));
        assert!(e.is_empty());
        assert_eq!(s[0].id, suivis[0].id);
    }

    #[test]
    fn pause_apres_trente_secondes_sans_changement() {
        let t0 = Instant::now();
        let (suivis, mut id) = lancer(&[partiel("a.zip.crdownload", 10)], t0);
        let (s, e) = comparer(&suivis, &[partiel("a.zip.crdownload", 10)], &mut id, &rien, t0 + Duration::from_millis(29_900));
        assert!(e.is_empty() && !s[0].en_pause, "29,9 s : pas encore");
        let (s, e) = comparer(&s, &[partiel("a.zip.crdownload", 10)], &mut id, &rien, t0 + Duration::from_secs(30));
        assert_eq!(e, vec![Evenement::EnPause { id: 1 }]);
        assert!(s[0].en_pause);
        // Une seule fois : la lecture suivante ne répète pas l'événement
        let (s, e) = comparer(&s, &[partiel("a.zip.crdownload", 10)], &mut id, &rien, t0 + Duration::from_secs(40));
        assert!(e.is_empty() && s[0].en_pause);
        // La taille bouge de nouveau : Progresse, et la pause est levée
        let (s, e) = comparer(&s, &[partiel("a.zip.crdownload", 20)], &mut id, &rien, t0 + Duration::from_secs(41));
        assert_eq!(e, vec![Evenement::Progresse { id: 1, taille: 20 }]);
        assert!(!s[0].en_pause);
        assert_eq!(s[0].derniere_variation, t0 + Duration::from_secs(41));
    }

    #[test]
    fn un_changement_repousse_la_pause() {
        let t0 = Instant::now();
        let (s, mut id) = lancer(&[partiel("a.zip.crdownload", 10)], t0);
        let (s, _) = comparer(&s, &[partiel("a.zip.crdownload", 11)], &mut id, &rien, t0 + Duration::from_secs(25));
        let (s, e) = comparer(&s, &[partiel("a.zip.crdownload", 11)], &mut id, &rien, t0 + Duration::from_secs(50));
        assert!(e.is_empty() && !s[0].en_pause, "25 s seulement depuis le dernier changement");
    }

    #[test]
    fn disparu_sans_nom_final_est_annule() {
        let t0 = Instant::now();
        let (s, mut id) = lancer(&[partiel("a.zip.crdownload", 100)], t0);
        let (s2, e) = comparer(&s, &[], &mut id, &rien, t0 + Duration::from_secs(3));
        assert!(s2.is_empty());
        assert_eq!(e, vec![Evenement::Annule { id: 1, nom_final: Some("a.zip".into()) }]);
    }

    #[test]
    fn firefox_fichier_final_vide_est_une_annulation() {
        // Firefox crée `a.zip` vide dès le départ : s'il reste vide alors que le partiel avait des octets, rien n'a été téléchargé
        let t0 = Instant::now();
        let (s, mut id) = lancer(&[partiel("a.zip.part", 4096)], t0);
        let vide = |_: &Suivi| Some(("a.zip".to_string(), 0));
        let (_, e) = comparer(&s, &[], &mut id, &vide, t0 + Duration::from_secs(2));
        assert_eq!(e, vec![Evenement::Annule { id: 1, nom_final: Some("a.zip".into()) }]);
        // Un partiel vide (téléchargement d'un fichier vide) qui finit avec un fichier vide est bien terminé
        let (s, mut id) = lancer(&[partiel("b.txt.part", 0)], t0);
        let (_, e) = comparer(&s, &[], &mut id, &|_: &Suivi| Some(("b.txt".to_string(), 0)), t0 + Duration::from_secs(2));
        assert_eq!(e, vec![Evenement::Termine { id: 1, nom: "b.txt".into(), taille: 0 }]);
    }

    #[test]
    fn unconfirmed_renomme_n_est_pas_une_fin() {
        let t0 = Instant::now();
        let (s, mut id) = lancer(&[partiel("Unconfirmed 42.crdownload", 1000)], t0);
        assert_eq!(s[0].partiel.nom_final, None);
        // Il disparaît et `b.zip.crdownload` apparaît, au moins aussi gros : même téléchargement, même identité
        let (s2, e) = comparer(&s, &[partiel("b.zip.crdownload", 1500)], &mut id, &rien, t0 + Duration::from_secs(2));
        assert_eq!(e, vec![Evenement::Progresse { id: 1, taille: 1500 }]);
        assert_eq!((s2.len(), s2[0].id, s2[0].partiel.nom_final.as_deref()), (1, 1, Some("b.zip")));
        // Plus petit : ce n'est pas le même (annulé, et un neuf commence)
        let (s3, e) = comparer(&s, &[partiel("b.zip.crdownload", 10)], &mut id, &rien, t0 + Duration::from_secs(2));
        assert!(matches!(e[0], Evenement::Commence { id: 2, .. }) && matches!(e[1], Evenement::Annule { id: 1, .. }), "{e:?}");
        assert_eq!(s3[0].id, 2);
        // Un Unconfirmed sans successeur qui disparaît : le fichier est retrouvé (taille et date), sinon annulé
        let trouve = |s: &Suivi| (s.partiel.nom_final.is_none()).then(|| ("Rapport.pdf".to_string(), s.partiel.taille));
        let (_, e) = comparer(&s, &[], &mut id, &trouve, t0 + Duration::from_secs(2));
        assert_eq!(e, vec![Evenement::Termine { id: 1, nom: "Rapport.pdf".into(), taille: 1000 }]);
        let (_, e) = comparer(&s, &[], &mut id, &rien, t0 + Duration::from_secs(2));
        assert!(matches!(e[0], Evenement::Annule { id: 1, nom_final: None }));
    }

    #[test]
    fn deux_telechargements_a_la_fois_gardent_chacun_leur_identite() {
        let t0 = Instant::now();
        let (s, mut id) = lancer(&[partiel("a.zip.crdownload", 1), partiel("b.zip.part", 1)], t0);
        assert_eq!(s.iter().map(|x| x.id).collect::<Vec<_>>(), vec![1, 2]);
        // a finit, b avance, c commence
        let existe_a = |s: &Suivi| (s.partiel.nom_final.as_deref() == Some("a.zip")).then(|| ("a.zip".to_string(), 50));
        let (s2, e) = comparer(&s, &[partiel("b.zip.part", 7), partiel("c.iso.crdownload", 3)], &mut id, &existe_a, t0 + Duration::from_secs(2));
        assert_eq!(s2.iter().map(|x| (x.id, x.partiel.nom_final.clone().unwrap())).collect::<Vec<_>>(), vec![(2, "b.zip".to_string()), (3, "c.iso".to_string())]);
        assert!(e.contains(&Evenement::Termine { id: 1, nom: "a.zip".into(), taille: 50 }));
        assert!(e.contains(&Evenement::Progresse { id: 2, taille: 7 }));
        assert!(e.contains(&Evenement::Commence { id: 3, nom_final: Some("c.iso".into()), navigateur: Navigateur::Chromium }));
    }

    #[test]
    fn vitesse_seulement_apres_deux_mesures_espacees_d_une_seconde() {
        let t0 = Instant::now();
        let (s, mut id) = lancer(&[partiel("a.zip.crdownload", 0)], t0);
        assert_eq!(s[0].vitesse, None);
        // 0,5 s plus tard : trop tôt pour une vitesse
        let (s, _) = comparer(&s, &[partiel("a.zip.crdownload", 500_000)], &mut id, &rien, t0 + Duration::from_millis(500));
        assert_eq!(s[0].vitesse, None);
        // 2 s après le premier relevé : 2 000 000 octets en 2 s
        let (s, _) = comparer(&s, &[partiel("a.zip.crdownload", 2_000_000)], &mut id, &rien, t0 + Duration::from_secs(2));
        assert_eq!(s[0].vitesse, Some(1_000_000));
        // Arrêt des écritures : la vitesse retombe à rien au relevé suivant
        let (s, _) = comparer(&s, &[partiel("a.zip.crdownload", 2_000_000)], &mut id, &rien, t0 + Duration::from_secs(4));
        assert_eq!(s[0].vitesse, None);
    }

    #[test]
    fn pause_longue_oubliee() {
        let t0 = Instant::now();
        let (s, mut id) = lancer(&[partiel("a.zip.part", 10)], t0);
        let (s, _) = comparer(&s, &[partiel("a.zip.part", 10)], &mut id, &rien, t0 + PAUSE);
        assert!(s[0].en_pause);
        // 9 min 59 s sans changement : encore là ; 10 min : oublié sans pastille de fin
        let (s2, e) = comparer(&s, &[partiel("a.zip.part", 10)], &mut id, &rien, t0 + ABANDON - Duration::from_secs(1));
        assert!(e.is_empty() && s2.len() == 1);
        let (s3, e) = comparer(&s2, &[partiel("a.zip.part", 10)], &mut id, &rien, t0 + ABANDON);
        assert!(s3.is_empty());
        assert_eq!(e, vec![Evenement::Oublie { id: 1 }]);
        // Le fichier a repris : ce n'est pas un oubli
        let (s4, e) = comparer(&s2, &[partiel("a.zip.part", 11)], &mut id, &rien, t0 + ABANDON);
        assert_eq!((s4.len(), e), (1, vec![Evenement::Progresse { id: 1, taille: 11 }]));
    }

    #[test]
    fn delai_de_reveil_selon_les_suivis() {
        let t0 = Instant::now();
        assert_eq!(delai_attente(&[]), None, "rien de suivi : l'événement système seul");
        let (mut s, _) = lancer(&[partiel("a.zip.part", 1), partiel("b.zip.part", 1)], t0);
        assert_eq!(delai_attente(&s), Some(LECTURE_MIN));
        s[0].en_pause = true;
        assert_eq!(delai_attente(&s), Some(LECTURE_MIN), "un seul en pause : l'autre avance");
        s[1].en_pause = true;
        assert_eq!(delai_attente(&s), Some(ATTENTE_PAUSE), "tous en pause : pas de réveil à 1 s");
        // Activité de Pixel : ni en pause ni masqué
        s[0].en_pause = false;
        assert!(en_activite(&s[0]));
        s[0].masque = true;
        assert!(!en_activite(&s[0]) && !en_activite(&s[1]));
    }

    #[test]
    fn unconfirmed_vide_est_annule() {
        let t0 = Instant::now();
        let (s, mut id) = lancer(&[partiel("Unconfirmed 5.crdownload", 0)], t0);
        // Même si un fichier vide existe quelque part, et si un partiel apparaît : jamais rattaché au hasard
        let tout_vide = |_: &Suivi| Some(("vide.txt".to_string(), 0));
        let (s2, e) = comparer(&s, &[], &mut id, &tout_vide, t0 + Duration::from_secs(2));
        assert!(s2.is_empty());
        assert_eq!(e, vec![Evenement::Annule { id: 1, nom_final: None }]);
        let (_, e) = comparer(&s, &[partiel("x.zip.crdownload", 500)], &mut id, &tout_vide, t0 + Duration::from_secs(2));
        assert!(matches!(e[0], Evenement::Commence { .. }) && matches!(e[1], Evenement::Annule { id: 1, .. }), "{e:?}");
    }

    #[test]
    fn unconfirmed_disparu_cherche_d_abord_un_partiel_neuf() {
        let t0 = Instant::now();
        let (s, mut id) = lancer(&[partiel("Unconfirmed 5.crdownload", 800)], t0);
        // Le partiel neuf de même famille l'emporte : le disque n'est même pas consulté
        let interdit = |_: &Suivi| -> Option<(String, u64)> { panic!("retrouver ne doit pas être appelé") };
        let (s2, e) = comparer(&s, &[partiel("n.zip.crdownload", 800)], &mut id, &interdit, t0 + Duration::from_secs(1));
        assert_eq!((s2[0].id, e.len()), (1, 0), "même taille : même téléchargement, rien à dire");
        // Une autre famille (Firefox) ne le rattache pas
        let (_, e) = comparer(&s, &[partiel("n.zip.part", 900)], &mut id, &|_: &Suivi| None, t0 + Duration::from_secs(1));
        assert!(matches!(e[0], Evenement::Commence { .. }) && matches!(e[1], Evenement::Annule { id: 1, .. }), "{e:?}");
    }

    #[test]
    fn dossier_d_essai_et_dossier_reel() {
        let reel = || Some(r"C:\Users\n\Downloads".to_string());
        let existe = |d: &str| d.starts_with(r"C:\Essai");
        // Usage normal : la variable est ignorée
        assert_eq!(choisir_dossier(false, Some(r"C:\Essai\dl"), reel, existe).as_deref(), Some(r"C:\Users\n\Downloads"));
        // Mode essai : la variable seule, jamais le vrai dossier
        assert_eq!(choisir_dossier(true, Some(r"C:\Essai\dl"), reel, existe).as_deref(), Some(r"C:\Essai\dl"));
        assert_eq!(choisir_dossier(true, None, reel, existe), None, "sans variable : rien");
        assert_eq!(choisir_dossier(true, Some("  "), reel, existe), None);
        assert_eq!(choisir_dossier(true, Some(r"dl\relatif"), reel, existe), None, "chemin relatif refusé");
        assert_eq!(choisir_dossier(true, Some(r"D:\Absent"), reel, existe), None, "dossier inexistant refusé");
    }

    #[test]
    fn marques_nouveau_limitees_a_vingt() {
        let mut l = Vec::new();
        for k in 0..25 {
            marquer(&mut l, r"C:\Users\n\Downloads\", &format!("F{k}.zip"));
        }
        assert_eq!(l.len(), MAX_NOUVEAUX);
        assert_eq!(l[0].1, "f5.zip", "les plus anciennes sont parties");
        assert_eq!(l[MAX_NOUVEAUX - 1].1, "f24.zip");
        // Le dossier est normalisé (casse, \ final)
        assert_eq!(l[0].0, r"c:\users\n\downloads");
        // Marquer deux fois le même fichier ne double pas la marque, et la remonte en dernier
        marquer(&mut l, r"c:\USERS\n\downloads", "F10.ZIP");
        marquer(&mut l, r"C:\Users\n\Downloads", "f10.zip");
        assert_eq!(l.len(), MAX_NOUVEAUX);
        assert_eq!(l.iter().filter(|x| x.1 == "f10.zip").count(), 1);
        assert_eq!(l[MAX_NOUVEAUX - 1].1, "f10.zip");
    }

    #[test]
    fn libelles_de_droite() {
        assert_eq!(etiquette_en_cours(5 * 1_048_576, None, false), "5,0 MO");
        assert_eq!(etiquette_en_cours(1_048_576 * 12 + 400_000, None, false), "12,4 MO");
        assert_eq!(etiquette_en_cours(1_048_576 * 12 + 400_000, Some(1_258_291), false), "12,4 MO · 1,2 MO/S");
        assert_eq!(etiquette_en_cours(2048, Some(500), true), "EN PAUSE · 2,0 KO", "en pause : pas de vitesse");
        assert_eq!(etiquette_en_cours(0, None, false), "0 O");
    }

    /// L'état partagé : une fin pose une marque NOUVEAU et une pastille, ✕ la retire, la marque s'oublie à la sélection,
    /// un abonnement arrêté n'écrit plus. Un seul test touche à `ETAT` (global) : pas de course entre tests.
    #[test]
    fn etat_partage_fin_marque_et_oubli() {
        let dossier = r"C:\Essai\Telechargements";
        let t0 = Instant::now();
        let g = etat().generation;
        let (s, mut id) = lancer(&[partiel("a.zip.crdownload", 100)], t0);
        assert!(appliquer(g, dossier, s.clone(), vec![Evenement::Commence { id: 1, nom_final: Some("a.zip".into()), navigateur: Navigateur::Chromium }], t0));
        assert!(avance());
        assert_eq!(pastilles().len(), 1);
        assert!(est_cle(pastilles()[0].cle) && pastilles()[0].cle & !CLE_BASE == 1);
        // ✕ en cours : masquée mais toujours suivie
        fermer(CLE_BASE | 1);
        assert!(pastilles().is_empty() && !avance(), "masqué : ni pastille ni activité de Pixel");
        assert!(etat().suivis.len() == 1, "mais toujours suivi");
        // Fin : marque NOUVEAU posée même pour une pastille masquée, mais pas de pastille « TÉLÉCHARGÉ » (l'utilisateur l'a masquée)
        let existe = |_: &Suivi| Some(("a.zip".to_string(), 9000));
        let (s2, e) = comparer(&s, &[], &mut id, &existe, t0 + Duration::from_secs(1));
        assert!(appliquer(g, dossier, s2, e, t0 + Duration::from_secs(1)));
        assert!(!avance() && pastilles().is_empty());
        assert_eq!(nouveaux_dans(dossier), vec!["a.zip".to_string()]);
        assert!(nouveaux_dans(r"C:\Autre").is_empty());
        assert!(marques_modifiees() && !marques_modifiees(), "le drapeau se prend une fois");
        // Un second téléchargement, non masqué : pastille TÉLÉCHARGÉ avec le chemin complet pour AFFICHER
        let (s3, e) = comparer(&[], &[partiel("b.iso.part", 5)], &mut id, &rien, t0);
        assert!(appliquer(g, dossier, s3.clone(), e, t0));
        let (s4, e) = comparer(&s3, &[], &mut id, &|_: &Suivi| Some(("b.iso".to_string(), 77)), t0);
        assert!(appliquer(g, dossier, s4, e, Instant::now()));
        let p = pastilles();
        assert_eq!(p.len(), 1);
        assert_eq!(p[0].libelle, "b.iso");
        assert_eq!(p[0].etiquette.as_deref(), Some("TÉLÉCHARGÉ · 77 O"));
        assert_eq!(chemin_fini(p[0].cle).as_deref(), Some(r"C:\Essai\Telechargements\b.iso"));
        assert!(temporaires() && reussite_recente(1500));
        assert_eq!(nouveaux_dans(dossier).len(), 2);
        // Sélectionné ou ouvert : la marque disparaît
        oublier(r"C:\Essai\Telechargements\", "A.ZIP");
        assert_eq!(nouveaux_dans(dossier), vec!["b.iso".to_string()]);
        assert!(marques_modifiees());
        // ✕ sur la pastille finie
        fermer(p[0].cle);
        assert!(pastilles().is_empty() && !temporaires());
        // Une génération périmée (abonnement arrêté depuis) n'écrit plus rien
        assert!(!appliquer(g + 1, dossier, Vec::new(), vec![Evenement::Annule { id: 9, nom_final: None }], t0));
        assert!(pastilles().is_empty());
        // Arrêt : tout est oublié
        ABONNEMENT.lock().unwrap().replace(Abonnement { arret: Arc::new(Poignee(HANDLE(std::ptr::null_mut()))), generation: g });
        arreter();
        assert!(!a_des_nouveaux() && etat().generation == g + 1);
    }
}
