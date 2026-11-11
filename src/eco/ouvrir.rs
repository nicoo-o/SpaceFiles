// `explorer.open {path, newTab?, reveal?, reuse?}` : une autre application demande d'ouvrir un dossier dans SpaceFiles.
// Le fil du tube valide seulement le TEXTE du chemin (aucun accès au disque : un partage réseau mort ne doit rien
// bloquer), applique les limites de débit et met la demande dans une file bornée. Un fil de lancement, séparé du fil du
// crochet clavier et de la fenêtre cachée, lance une nouvelle fenêtre de `spacefiles.exe`. Le chemin n'est JAMAIS
// exécuté : il devient l'argument de `--ouvrir` (ou de `--montrer` pour `reveal`) et c'est la NOUVELLE fenêtre qui
// décide, dans son propre processus, s'il s'agit d'un dossier (ouvert) ou d'autre chose (dossier parent, élément choisi).
// « Ouvert » veut dire « la demande est en file », pas « la fenêtre est affichée ». Avec `reuse`, le fil de lancement
// essaie d'abord de confier le chemin à une fenêtre déjà ouverte (`reutiliser.rs`) et ne lance une nouvelle fenêtre que
// si aucune ne convient, la refuse ou ne répond pas.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{sync_channel, Receiver, SyncSender, TrySendError};
use std::sync::Mutex;
use std::time::Instant;

use super::trame::Erreur;

/// Au plus 5 ouvertures par seconde glissante (chaque ouverture lance un processus complet avec sa fenêtre)
const MAX_PAR_SECONDE: usize = 5;
/// Au plus 20 ouvertures par minute glissante
const MAX_PAR_MINUTE: usize = 20;
/// Seau de rafale : 5 ouvertures d'un coup, puis une de plus toutes les 2 s
const PRIX_MS: u64 = 2_000;
const CAPACITE_MS: u64 = 5 * PRIX_MS;
/// Demandes acceptées mais pas encore lancées : au-delà, `trop_vite` (le résident ne s'engorge pas)
const FILE_MAX: usize = 8;
/// Longueur maximale d'un chemin, en caractères
const CHEMIN_MAX: usize = 32_000;

struct Demande {
    chemin: String,
    reveler: bool,
    /// Essayer d'abord une fenêtre déjà ouverte (`reuse`)
    reutiliser: bool,
}

/// Trois limites, évaluées ensemble : fenêtre de 1 s, seau de rafale, fenêtre de 60 s
struct Limiteur {
    instants: VecDeque<u64>,
    par_seconde: usize,
    par_minute: usize,
    capacite_ms: u64,
    jetons_ms: u64,
    dernier_ms: u64,
}

impl Limiteur {
    fn standard() -> Limiteur {
        Limiteur { instants: VecDeque::new(), par_seconde: MAX_PAR_SECONDE, par_minute: MAX_PAR_MINUTE, capacite_ms: CAPACITE_MS, jetons_ms: CAPACITE_MS, dernier_ms: 0 }
    }

    /// Une ouverture est-elle permise maintenant ? Ne consomme rien : l'appelant `enregistre` après avoir mis la demande
    /// en file (une demande refusée plus loin ne prend la place de personne).
    fn verifier(&mut self, maintenant_ms: u64) -> bool {
        while self.instants.front().is_some_and(|&t| maintenant_ms.saturating_sub(t) >= 60_000) {
            self.instants.pop_front();
        }
        // Le seau se recharge avec le temps (une horloge qui recule ne recharge rien)
        self.jetons_ms = (self.jetons_ms + maintenant_ms.saturating_sub(self.dernier_ms)).min(self.capacite_ms);
        self.dernier_ms = self.dernier_ms.max(maintenant_ms);
        let derniere_seconde = self.instants.iter().filter(|&&t| maintenant_ms.saturating_sub(t) < 1000).count();
        derniere_seconde < self.par_seconde && self.instants.len() < self.par_minute && self.jetons_ms >= PRIX_MS
    }

    fn enregistrer(&mut self, maintenant_ms: u64) {
        self.instants.push_back(maintenant_ms);
        self.jetons_ms = self.jetons_ms.saturating_sub(PRIX_MS);
    }

    #[cfg(test)]
    fn autoriser(&mut self, maintenant_ms: u64) -> bool {
        let ok = self.verifier(maintenant_ms);
        if ok {
            self.enregistrer(maintenant_ms);
        }
        ok
    }
}

/// Caractères qui font mentir un chemin affiché : direction du texte, séparateurs de ligne ou de paragraphe, espaces
/// de largeur nulle, marque d'ordre des octets
fn est_trompeur(c: char) -> bool {
    matches!(c, '\u{061C}' | '\u{200B}'..='\u{200F}' | '\u{2028}'..='\u{202E}' | '\u{2066}'..='\u{2069}' | '\u{FEFF}')
}

/// `CON`, `NUL`, `COM1`… avec ou sans extension, espaces et points finaux compris : des périphériques pour Windows
fn nom_reserve(composant: &str) -> bool {
    let radical = composant.split('.').next().unwrap_or("").trim_end_matches([' ', '.']).to_uppercase();
    match radical.as_str() {
        "CON" | "PRN" | "AUX" | "NUL" | "CONIN$" | "CONOUT$" => true,
        r => r.len() == 4 && (r.starts_with("COM") || r.starts_with("LPT")) && r.as_bytes()[3].is_ascii_digit() && r.as_bytes()[3] != b'0',
    }
}

/// Un chemin que le Shell sait ouvrir : complet, local (`C:\…`) ou UNC (`\\hôte`, `\\hôte\partage\…`). Les `/` deviennent
/// des `\` AVANT toute vérification (`\\./pipe/x` n'est pas un chemin réseau). Refuse tout le reste : relatif, programme
/// (`calc.exe`), URL (`https:`), flux (`C:\a:b`), espace de noms de périphériques (`\\.\`, `\\?\`, hôte fait de points),
/// nom de périphérique (`NUL`, `COM1.txt`), caractère de contrôle (NUL compris), caractère trompeur, caractère interdit
/// par Windows, 32 000 caractères et plus. Rend le chemin normalisé (c'est lui qui est lancé).
pub fn valider_chemin(brut: &str) -> Result<String, Erreur> {
    if brut.is_empty() {
        return Err(Erreur::Invalide);
    }
    if brut.chars().count() >= CHEMIN_MAX {
        return Err(Erreur::HorsBornes("path"));
    }
    let c = brut.replace('/', "\\");
    if c.chars().any(|ch| ch.is_control() || est_trompeur(ch) || matches!(ch, '"' | '<' | '>' | '|' | '*' | '?')) {
        return Err(Erreur::Invalide);
    }
    let o = c.as_bytes();
    let lecteur = o.len() >= 3 && o[0].is_ascii_alphabetic() && o[1] == b':' && o[2] == b'\\';
    let unc = c.starts_with(r"\\");
    if !lecteur && !unc {
        return Err(Erreur::Invalide);
    }
    // Les deux points de la lettre de lecteur sont les seuls permis : ni schéma (`https:`) ni flux de données (`C:\a:b`)
    if c[2..].contains(':') {
        return Err(Erreur::Invalide);
    }
    let reste = &c[2..];
    let mut composants = reste.split('\\');
    if unc {
        // L'hôte : jamais vide, jamais `.` ou `..` (espace de noms des périphériques, ex. `\\.\pipe\x`)
        let hote = composants.next().unwrap_or("");
        if hote.is_empty() || hote.chars().all(|ch| ch == '.') {
            return Err(Erreur::Invalide);
        }
    }
    if composants.any(nom_reserve) {
        return Err(Erreur::Invalide);
    }
    Ok(c)
}

/// Un argument de ligne de commande Windows : entre guillemets, les antislash finaux doublés (sinon `"C:\"` mange son
/// guillemet fermant). `valider_chemin` a déjà écarté tout guillemet à l'intérieur.
fn citer(arg: &str) -> String {
    let fin = arg.len() - arg.trim_end_matches('\\').len();
    format!("\"{}{}\"", arg, "\\".repeat(fin))
}

/// Arguments de la nouvelle fenêtre : toujours un drapeau dédié, jamais le chemin nu (qui pourrait être confié à
/// l'Explorateur de Windows, c'est-à-dire exécuté). La fenêtre résout le chemin dans son propre processus.
fn arguments_de_lancement(chemin: &str, reveler: bool) -> String {
    format!("{} {}", if reveler { "--montrer" } else { "--ouvrir" }, citer(chemin))
}

// ───────────────────────── File et fil de lancement ─────────────────────────

/// Ce qui lance une fenêtre (une ligne d'arguments) : la vraie, ou un faux qui note l'appel sans rien afficher
type Lanceur = Box<dyn Fn(&str) + Send + 'static>;

/// Ce qui confie un chemin à une fenêtre déjà ouverte : `(chemin, reveler)`, vrai si une fenêtre l'a accepté. La vraie
/// cherche et interroge des fenêtres (`reutiliser::reutiliser_fenetre`), une fausse sert aux essais.
type Reutilisateur = Box<dyn Fn(&str, bool) -> bool + Send + 'static>;

struct Ouvreur {
    envoi: SyncSender<Demande>,
    limiteur: Mutex<Limiteur>,
    debut: Instant,
}

impl Ouvreur {
    /// Démarre le fil de lancement. None si le fil ne peut pas être créé.
    fn nouveau(lancer: Lanceur, reutiliser: Reutilisateur, limiteur: Limiteur) -> Option<Ouvreur> {
        let (envoi, reception) = sync_channel::<Demande>(FILE_MAX);
        std::thread::Builder::new().name("eco-ouvrir".into()).spawn(move || boucle_de_lancement(reception, lancer, reutiliser)).ok()?;
        Some(Ouvreur { envoi, limiteur: Mutex::new(limiteur), debut: Instant::now() })
    }

    /// Valide, limite le débit, met en file. Une file pleine ne consomme aucune place de débit.
    fn demander(&self, chemin: &str, reveler: bool, reutiliser: bool) -> Result<(), Erreur> {
        let chemin = valider_chemin(chemin)?;
        let maintenant = self.debut.elapsed().as_millis() as u64;
        let mut l = self.limiteur.lock().unwrap_or_else(|e| e.into_inner());
        if !l.verifier(maintenant) {
            return Err(Erreur::TropVite);
        }
        match self.envoi.try_send(Demande { chemin, reveler, reutiliser }) {
            Ok(()) => {
                l.enregistrer(maintenant);
                Ok(())
            }
            Err(TrySendError::Full(_)) => Err(Erreur::TropVite),
            Err(TrySendError::Disconnected(_)) => Err(Erreur::Interne),
        }
    }
}

/// Le fil de lancement : une demande après l'autre. Il vit tant que l'`Ouvreur` existe (l'envoi fermé le fait sortir).
/// Une demande `reuse` essaie d'abord une fenêtre ouverte (au plus 2 s d'attente) ; si elle ne l'accepte pas, la demande
/// est lancée comme n'importe quelle autre : `reuse` ne perd jamais une ouverture.
fn boucle_de_lancement(reception: Receiver<Demande>, lancer: Lanceur, reutiliser: Reutilisateur) {
    use windows::Win32::System::Com::{CoInitializeEx, CoUninitialize, COINIT_APARTMENTTHREADED, COINIT_DISABLE_OLE1DDE};
    // SAFETY : initialise COM pour CE fil seulement (ShellExecuteW le demande) ; CoUninitialize en fin de fil le défait
    // quand l'initialisation a réussi.
    let com = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE) }.is_ok();
    for d in reception {
        if d.reutiliser && reutiliser(&d.chemin, d.reveler) {
            continue;
        }
        lancer(&arguments_de_lancement(&d.chemin, d.reveler));
    }
    if com {
        // SAFETY : appariée à l'initialisation réussie plus haut, sur le même fil.
        unsafe { CoUninitialize() };
    }
}

fn lancer_pour_de_bon(arguments: &str) {
    use windows::core::HSTRING;
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::Threading::GetProcessId;
    use windows::Win32::UI::Shell::{ShellExecuteExW, SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW};
    use windows::Win32::UI::WindowsAndMessaging::{AllowSetForegroundWindow, SW_SHOWNORMAL};
    let Ok(exe) = std::env::current_exe() else {
        super::trace(crate::journal::Niveau::Avert, "eco-ouvrir", "lancement impossible : exe introuvable");
        return;
    };
    // Les HSTRING (terminés par 0) doivent vivre jusqu'à la fin de l'appel : ils sont liés à des variables, pas à des temporaires.
    let fichier = HSTRING::from(exe.as_os_str());
    let parametres = HSTRING::from(arguments);
    let mut info = SHELLEXECUTEINFOW {
        cbSize: std::mem::size_of::<SHELLEXECUTEINFOW>() as u32,
        // On veut le processus lancé : c'est lui, et non le résident, qui doit pouvoir prendre le premier plan.
        fMask: SEE_MASK_NOCLOSEPROCESS,
        lpFile: windows::core::PCWSTR(fichier.as_ptr()),
        lpParameters: windows::core::PCWSTR(parametres.as_ptr()),
        nShow: SW_SHOWNORMAL.0,
        ..Default::default()
    };
    // SAFETY : `info` est entièrement initialisée (cbSize exact, le reste à zéro) et `fichier` / `parametres` vivent
    // jusqu'à la fin de l'appel. Appelé depuis le fil `eco-ouvrir` (COM en appartement monothread, voir
    // `boucle_de_lancement`), jamais depuis le fil du crochet clavier.
    let lance = unsafe { ShellExecuteExW(&mut info) };
    if lance.is_err() {
        // On ne note que l'état, jamais le chemin
        super::trace(crate::journal::Niveau::Avert, "eco-ouvrir", "echec du lancement d'une fenetre");
        return;
    }
    // Pas de handle (le Shell n'a pas lancé de processus lui-même) : rien à autoriser, la fenêtre s'ouvre quand même.
    if info.hProcess.is_invalid() {
        return;
    }
    // SAFETY : `hProcess` est un handle de processus valide que le Shell nous remet (SEE_MASK_NOCLOSEPROCESS) ; il
    // reste ouvert jusqu'au CloseHandle ci-dessous, donc l'identifiant lu est celui du processus que nous venons de lancer.
    let pid = unsafe { GetProcessId(info.hProcess) };
    if pid != 0 {
        // SAFETY : appel sans pointeur ; le résident cède au processus fils le droit de prendre le premier plan. Un
        // refus de Windows (le résident n'a pas ce droit) n'est pas une erreur : la fenêtre s'ouvre, sans premier plan garanti.
        let accorde = unsafe { AllowSetForegroundWindow(pid) }.is_ok();
        super::trace(
            crate::journal::Niveau::Info,
            "eco-ouvrir",
            if accorde { "premier plan accorde a la fenetre lancee" } else { "premier plan refuse par Windows pour la fenetre lancee" },
        );
    }
    // SAFETY : handle obtenu plus haut, fermé une seule fois, plus utilisé ensuite.
    let _ = unsafe { CloseHandle(info.hProcess) };
}

/// Faux lanceur d'essai : ajoute la ligne d'arguments à un fichier, n'affiche rien. N'existe qu'en mode essai
/// (`SPACEFILES_PIPE` posée) ET si `SPACEFILES_FAUX_LANCEUR` donne le fichier : absent en usage normal.
fn faux_lanceur_pour(essai: bool, variable: Option<std::ffi::OsString>) -> Option<std::path::PathBuf> {
    if !essai {
        return None;
    }
    variable.filter(|v| !v.is_empty()).map(std::path::PathBuf::from)
}

fn faux_lanceur_demande() -> Option<std::path::PathBuf> {
    faux_lanceur_pour(super::essai().is_some(), std::env::var_os("SPACEFILES_FAUX_LANCEUR"))
}

fn noter_dans(fichier: &std::path::Path, arguments: &str) {
    use std::io::Write;
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(fichier) {
        let _ = writeln!(f, "{arguments}");
    }
}

fn lanceur_courant() -> Lanceur {
    match faux_lanceur_demande() {
        Some(fichier) => Box::new(move |a| noter_dans(&fichier, a)),
        None => Box::new(lancer_pour_de_bon),
    }
}

fn reutilisateur_courant() -> Reutilisateur {
    Box::new(super::reutiliser::reutiliser_fenetre)
}

static ACTIF: AtomicBool = AtomicBool::new(false);
static OUVREUR: Mutex<Option<Ouvreur>> = Mutex::new(None);

/// Le résident accepte les demandes (le fil de lancement ne démarre qu'à la première)
pub fn initialiser() {
    ACTIF.store(true, Ordering::SeqCst);
}

/// Plus de demande ; le fil de lancement sort quand sa file est vide
pub fn arreter() {
    ACTIF.store(false, Ordering::SeqCst);
    let _ = OUVREUR.lock().unwrap_or_else(|e| e.into_inner()).take();
}

/// Accepte la demande d'un client du tube. Appelé depuis un fil du tube : ni disque, ni fenêtre, ni ShellExecute ici.
pub fn demander(chemin: &str, reveler: bool, reutiliser: bool) -> Result<(), Erreur> {
    // Le texte d'abord : une demande invalide reste invalide même si le résident s'arrête
    valider_chemin(chemin)?;
    if !ACTIF.load(Ordering::SeqCst) {
        return Err(Erreur::Interne);
    }
    let mut o = OUVREUR.lock().unwrap_or_else(|e| e.into_inner());
    if o.is_none() {
        *o = Ouvreur::nouveau(lanceur_courant(), reutilisateur_courant(), Limiteur::standard());
    }
    match o.as_ref() {
        Some(ouvreur) => ouvreur.demander(chemin, reveler, reutiliser),
        None => Err(Erreur::Interne),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc::channel;
    use std::sync::Arc;
    use std::time::Duration;

    fn ok_norm(c: &str) -> String {
        super::super::ok(valider_chemin(c), c)
    }

    #[test]
    fn chemins_acceptes() {
        for c in [r"C:\", r"C:\Users", r"d:\a b\c.txt", r"\\hote", r"\\hote\partage", r"\\hote\partage\dossier\fichier.txt", r"C:\été\日本語 😀", r"C:\Users\", r"C:\x\CONSOLE.txt", r"C:\x\COM10", r"C:\x\COM0", r"C:\x\NULL"] {
            assert_eq!(ok_norm(c), c, "{c}");
        }
        // Les `/` deviennent des `\` : ce qui est lancé est le chemin normalisé
        assert_eq!(ok_norm("C:/Users/nico"), r"C:\Users\nico");
        assert_eq!(ok_norm("//hote/partage"), r"\\hote\partage");
    }

    #[test]
    fn chemins_refuses() {
        for c in [
            "", "dossier", r"..\x", "calc.exe", "https://x", "file:///C:/x", "C:", "C:x", r"\Windows", r"\\", r"\\\x",
            r"\\.\pipe\x", r"\\?\C:\x", r"C:\a:flux", r"C:\a\b:c", "ms-settings:display",
            // I1 : le `/` est remplacé avant la vérification, les hôtes de périphériques sont refusés
            r"\\./pipe/x", r"\\.", r"\\..", r"\\./C/x", "//./pipe/x", "//?/C:/x", r"\\...\x", "\\\\.\\",
            // Noms de périphériques, avec ou sans extension, en dernier composant ou au milieu
            r"C:\NUL", r"C:\x\COM1.txt", r"C:\x\com9", r"C:\LPT3", r"C:\x\con.", r"C:\x\aux .txt", r"C:\nul\y", r"\\hote\partage\PRN",
        ] {
            assert_eq!(valider_chemin(c), Err(Erreur::Invalide), "{c}");
        }
        // Contrôles (NUL compris), direction du texte, séparateurs U+2028/2029, espaces de largeur nulle, BOM, interdits
        for c in [
            "C:\\a\u{0}b", "C:\\a\nb", "C:\\a\tb", "C:\\a\u{7f}", "C:\\a\u{202e}txt.exe", "C:\\a\u{2066}b", "C:\\a\u{200f}",
            "C:\\a\u{2028}b", "C:\\a\u{2029}b", "C:\\a\u{200b}b", "C:\\a\u{200c}", "C:\\a\u{200d}", "C:\\a\u{feff}b",
            "C:\\a\"b", "C:\\a<b", "C:\\a|b", "C:\\a*", "C:\\a?",
        ] {
            assert_eq!(valider_chemin(c), Err(Erreur::Invalide), "{c:?}");
        }
    }

    #[test]
    fn longueur_du_chemin() {
        let limite = format!("C:\\{}", "d".repeat(CHEMIN_MAX - 4));
        assert_eq!(limite.chars().count(), CHEMIN_MAX - 1);
        assert_eq!(ok_norm(&limite), limite);
        assert_eq!(valider_chemin(&format!("{limite}d")), Err(Erreur::HorsBornes("path")));
        assert_eq!(valider_chemin(&format!("C:\\{}", "d".repeat(33_000))), Err(Erreur::HorsBornes("path")));
        // Compté en caractères, pas en octets
        assert_eq!(ok_norm(&format!("C:\\{}", "é".repeat(20_000))).chars().count(), 20_003);
    }

    #[test]
    fn limite_de_debit_glissante() {
        let mut l = Limiteur::standard();
        // 20 appels en 1 s : 5 passent
        let acceptes = (0..20).filter(|i| l.autoriser(1_000 + i * 40)).count();
        assert_eq!(acceptes, MAX_PAR_SECONDE);
        // Le seau est vide : une ouverture revient seulement 2 s après la rafale
        assert!(!l.autoriser(2_100), "seau vide");
        assert!(l.autoriser(3_000), "une recharge toutes les 2 s");
        assert!(!l.autoriser(3_100));
        assert!(l.autoriser(5_100));
        // Horloge qui recule : jamais d'autorisation en trop
        let mut r = Limiteur::standard();
        assert_eq!((0..10).filter(|_| r.autoriser(0)).count(), MAX_PAR_SECONDE);
    }

    /// Une rafale de 5 puis une toutes les 2 s : plafond de 20 par minute
    #[test]
    fn plafond_par_minute() {
        let mut l = Limiteur::standard();
        // Une ouverture toutes les 2 s pendant 2 minutes : 5 de rafale + 1 par 2 s, mais jamais plus de 20 par minute
        let mut acceptes = Vec::new();
        for k in 0..60u64 {
            let t = k * 2_000;
            if l.autoriser(t) {
                acceptes.push(t);
            }
        }
        for &t in &acceptes {
            let dans_la_minute = acceptes.iter().filter(|&&u| u >= t && u < t + 60_000).count();
            assert!(dans_la_minute <= MAX_PAR_MINUTE, "{dans_la_minute} en une minute à partir de {t}");
        }
        assert!(acceptes.len() > 20);
        // Seau de rafale : un client calme garde sa rafale
        let mut c = Limiteur::standard();
        assert_eq!((0..10).filter(|_| c.autoriser(10_000_000)).count(), 5);
    }

    #[test]
    fn argument_cite_avec_antislash_final() {
        assert_eq!(citer(r"C:\Users"), r#""C:\Users""#);
        assert_eq!(citer(r"C:\"), r#""C:\\""#);
        assert_eq!(citer(r"C:\a b\"), r#""C:\a b\\""#);
        assert_eq!(citer(r"\\hote\partage\\"), r#""\\hote\partage\\\\""#);
    }

    /// Ce que la nouvelle fenêtre reçoit : toujours un drapeau, jamais le chemin nu (donc jamais exécuté)
    #[test]
    fn arguments_du_lancement() {
        assert_eq!(arguments_de_lancement(r"C:\Users", false), r#"--ouvrir "C:\Users""#);
        assert_eq!(arguments_de_lancement(r"C:\Users\", false), r#"--ouvrir "C:\Users\\""#);
        assert_eq!(arguments_de_lancement(r"\\hote", false), r#"--ouvrir "\\hote""#);
        assert_eq!(arguments_de_lancement(r"C:\Users\a.txt", true), r#"--montrer "C:\Users\a.txt""#);
        assert_eq!(arguments_de_lancement(r"C:\", true), r#"--montrer "C:\\""#);
        assert_eq!(arguments_de_lancement(r"C:\Windows\notepad.exe", false), r#"--ouvrir "C:\Windows\notepad.exe""#);
    }

    fn limiteur_large() -> Limiteur {
        Limiteur { instants: VecDeque::new(), par_seconde: 1000, par_minute: 1000, capacite_ms: u64::MAX / 4, jetons_ms: u64::MAX / 4, dernier_ms: 0 }
    }

    /// Un lanceur qui note les arguments reçus et qui peut être bloqué
    fn lanceur_enregistreur() -> (Lanceur, Arc<Mutex<Vec<String>>>) {
        let vu = Arc::new(Mutex::new(Vec::new()));
        let v2 = vu.clone();
        (Box::new(move |a| v2.lock().unwrap_or_else(|e| e.into_inner()).push(a.to_string())), vu)
    }

    /// `reutiliser` qui ne trouve jamais de fenêtre : le comportement d'avant `reuse`
    fn jamais_reutiliser() -> Reutilisateur {
        Box::new(|_, _| false)
    }

    /// `reutiliser` qui note ses appels `(chemin, reveler)` et répond `reponse`
    fn reutilisateur_enregistreur(reponse: bool) -> (Reutilisateur, Arc<Mutex<Vec<(String, bool)>>>) {
        let vu = Arc::new(Mutex::new(Vec::new()));
        let v2 = vu.clone();
        (
            Box::new(move |c, r| {
                v2.lock().unwrap_or_else(|e| e.into_inner()).push((c.to_string(), r));
                reponse
            }),
            vu,
        )
    }

    fn attendre(vu: &Arc<Mutex<Vec<String>>>, n: usize) -> Vec<String> {
        for _ in 0..200 {
            let v = vu.lock().unwrap_or_else(|e| e.into_inner()).clone();
            if v.len() >= n {
                return v;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        vu.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    /// File, validation et arguments de bout en bout, sans fenêtre : le lanceur est remplacé
    #[test]
    fn demande_lancee_par_le_fil_de_lancement() {
        let (lanceur, vu) = lanceur_enregistreur();
        let o = super::super::some(Ouvreur::nouveau(lanceur, jamais_reutiliser(), Limiteur::standard()), "ouvreur");
        assert_eq!(o.demander(r"C:\Users", false, false), Ok(()));
        assert_eq!(o.demander("C:/a b/c.txt", true, false), Ok(()));
        assert_eq!(o.demander("calc.exe", false, false), Err(Erreur::Invalide));
        assert_eq!(o.demander(r"\\./pipe/x", false, false), Err(Erreur::Invalide));
        let v = attendre(&vu, 2);
        assert_eq!(v, vec![r#"--ouvrir "C:\Users""#.to_string(), r#"--montrer "C:\a b\c.txt""#.to_string()]);
        // Le lancement n'a lieu ni sur ce fil ni sur celui d'une fenêtre : un fil à part
        assert_eq!(o.limiteur.lock().unwrap_or_else(|e| e.into_inner()).instants.len(), 2, "les refus ne consomment pas de débit");
    }

    /// `reuse` et une fenêtre qui refuse (ou absente, ou figée) : repli, la ligne de lancement part comme sans `reuse`
    #[test]
    fn reuse_sans_fenetre_se_replie_sur_le_lancement() {
        let (lanceur, lance) = lanceur_enregistreur();
        let (reut, essais) = reutilisateur_enregistreur(false);
        let o = super::super::some(Ouvreur::nouveau(lanceur, reut, Limiteur::standard()), "ouvreur");
        assert_eq!(o.demander(r"C:\Users", false, true), Ok(()));
        assert_eq!(o.demander("C:/a b/c.txt", true, true), Ok(()));
        let v = attendre(&lance, 2);
        assert_eq!(v, vec![r#"--ouvrir "C:\Users""#.to_string(), r#"--montrer "C:\a b\c.txt""#.to_string()]);
        // La fenêtre a été tentée avec le chemin normalisé et le bon drapeau, dans l'ordre
        assert_eq!(*essais.lock().unwrap_or_else(|e| e.into_inner()), vec![(r"C:\Users".to_string(), false), (r"C:\a b\c.txt".to_string(), true)]);
    }

    /// `reuse` et une fenêtre qui accepte : aucun lancement
    #[test]
    fn reuse_accepte_ne_lance_rien() {
        let (lanceur, lance) = lanceur_enregistreur();
        let (reut, essais) = reutilisateur_enregistreur(true);
        let o = super::super::some(Ouvreur::nouveau(lanceur, reut, Limiteur::standard()), "ouvreur");
        assert_eq!(o.demander(r"C:\Users", false, true), Ok(()));
        assert_eq!(o.demander(r"C:\Users\a.txt", true, true), Ok(()));
        // Une demande sans `reuse` derrière elles : traitées dans l'ordre, donc quand celle-ci est lancée les deux autres
        // ont déjà été confiées à la fenêtre
        assert_eq!(o.demander(r"C:\Fin", false, false), Ok(()));
        let v = attendre(&lance, 1);
        assert_eq!(v, vec![r#"--ouvrir "C:\Fin""#.to_string()], "seule la demande sans reuse est lancée");
        assert_eq!(essais.lock().unwrap_or_else(|e| e.into_inner()).len(), 2, "la fenêtre n'a vu que les demandes reuse");
        // Elles ont consommé du débit comme les autres
        assert_eq!(o.limiteur.lock().unwrap_or_else(|e| e.into_inner()).instants.len(), 3);
    }

    /// `reuse:false` : le chemin d'avant, la fenêtre n'est même pas interrogée
    #[test]
    fn sans_reuse_la_fenetre_n_est_pas_interrogee() {
        let (lanceur, lance) = lanceur_enregistreur();
        let (reut, essais) = reutilisateur_enregistreur(true);
        let o = super::super::some(Ouvreur::nouveau(lanceur, reut, Limiteur::standard()), "ouvreur");
        assert_eq!(o.demander(r"C:\Users", false, false), Ok(()));
        let v = attendre(&lance, 1);
        assert_eq!(v, vec![r#"--ouvrir "C:\Users""#.to_string()]);
        assert!(essais.lock().unwrap_or_else(|e| e.into_inner()).is_empty());
    }

    /// Une demande `reuse` invalide est refusée comme les autres, sans toucher à la fenêtre ni au débit
    #[test]
    fn reuse_invalide_refuse_avant_tout() {
        let (lanceur, lance) = lanceur_enregistreur();
        let (reut, essais) = reutilisateur_enregistreur(true);
        let o = super::super::some(Ouvreur::nouveau(lanceur, reut, Limiteur::standard()), "ouvreur");
        assert_eq!(o.demander("calc.exe", false, true), Err(Erreur::Invalide));
        assert_eq!(o.demander(r"\\.\pipe\x", true, true), Err(Erreur::Invalide));
        std::thread::sleep(Duration::from_millis(50));
        assert!(essais.lock().unwrap_or_else(|e| e.into_inner()).is_empty());
        assert!(lance.lock().unwrap_or_else(|e| e.into_inner()).is_empty());
        assert_eq!(o.limiteur.lock().unwrap_or_else(|e| e.into_inner()).instants.len(), 0);
    }

    /// Le seau de débit s'applique aussi à `reuse` : 5 en rafale, puis `trop_vite`
    #[test]
    fn reuse_consomme_le_seau() {
        let (reut, _) = reutilisateur_enregistreur(true);
        let (lanceur, _) = lanceur_enregistreur();
        let o = super::super::some(Ouvreur::nouveau(lanceur, reut, Limiteur::standard()), "ouvreur");
        let acceptes = (0..10).filter(|_| o.demander(r"C:\x", false, true).is_ok()).count();
        assert_eq!(acceptes, MAX_PAR_SECONDE);
        assert_eq!(o.demander(r"C:\x", false, true), Err(Erreur::TropVite));
    }

    /// Le lancement est sur un autre fil que l'appelant (le crochet clavier ne doit jamais attendre ShellExecute)
    #[test]
    fn lancement_hors_du_fil_de_l_appelant() {
        let (tx, rx) = channel();
        let lanceur: Lanceur = Box::new(move |_| {
            let _ = tx.send(std::thread::current().id());
        });
        let o = super::super::some(Ouvreur::nouveau(lanceur, jamais_reutiliser(), Limiteur::standard()), "ouvreur");
        assert_eq!(o.demander(r"C:\x", false, false), Ok(()));
        let id = super::super::ok(rx.recv_timeout(Duration::from_secs(5)), "lancement");
        assert_ne!(id, std::thread::current().id());
    }

    /// Un lancement lent ne bloque pas l'appelant : file pleine = trop_vite sans attendre, sans consommer de débit
    #[test]
    fn file_pleine_sans_consommer_de_debit() {
        let (porte_tx, porte_rx) = channel::<()>();
        let porte_rx = Mutex::new(porte_rx);
        let lanceur: Lanceur = Box::new(move |_| {
            let _ = porte_rx.lock().unwrap_or_else(|e| e.into_inner()).recv();
        });
        let o = super::super::some(Ouvreur::nouveau(lanceur, jamais_reutiliser(), limiteur_large()), "ouvreur");
        let debut = Instant::now();
        let mut acceptes = 0;
        let mut refus = None;
        for _ in 0..(FILE_MAX + 3) {
            match o.demander(r"C:\x", false, false) {
                Ok(()) => acceptes += 1,
                Err(e) => {
                    refus = Some(e);
                    break;
                }
            }
        }
        assert_eq!(refus, Some(Erreur::TropVite));
        assert!(acceptes >= FILE_MAX && acceptes <= FILE_MAX + 1, "{acceptes} acceptées (la file + celle en cours de lancement)");
        assert!(debut.elapsed() < Duration::from_secs(2), "l'appelant n'a pas attendu le lancement");
        assert_eq!(o.limiteur.lock().unwrap_or_else(|e| e.into_inner()).instants.len(), acceptes, "le refus n'a pas pris de place");
        drop(porte_tx);
    }

    #[test]
    fn demande_refusee_sans_resident() {
        // Le résident n'est pas démarré dans la suite de tests : une demande valide est refusée proprement
        arreter();
        assert_eq!(demander(r"C:\Users", false, false), Err(Erreur::Interne));
        assert_eq!(demander(r"C:\Users", false, true), Err(Erreur::Interne));
        assert_eq!(demander("calc.exe", false, false), Err(Erreur::Invalide));
        assert_eq!(demander("calc.exe", false, true), Err(Erreur::Invalide));
        assert!(OUVREUR.lock().unwrap_or_else(|e| e.into_inner()).is_none());
    }

    /// Le faux lanceur n'existe qu'en mode essai : sans `SPACEFILES_PIPE`, jamais
    #[test]
    fn faux_lanceur_reserve_au_mode_essai() {
        use std::ffi::OsString;
        let v = || Some(OsString::from(r"C:\t\faux.txt"));
        assert_eq!(faux_lanceur_pour(false, v()), None, "usage normal : jamais de faux lanceur");
        assert_eq!(faux_lanceur_pour(true, None), None);
        assert_eq!(faux_lanceur_pour(true, Some(OsString::new())), None);
        assert_eq!(faux_lanceur_pour(true, v()), Some(std::path::PathBuf::from(r"C:\t\faux.txt")));
        let f = std::env::temp_dir().join(format!("faux-lanceur-test-{}.txt", std::process::id()));
        let _ = std::fs::remove_file(&f);
        noter_dans(&f, r#"--ouvrir "C:\x""#);
        noter_dans(&f, r#"--montrer "C:\y""#);
        assert_eq!(super::super::ok(std::fs::read_to_string(&f), "lecture"), "--ouvrir \"C:\\x\"\n--montrer \"C:\\y\"\n");
        let _ = std::fs::remove_file(&f);
    }
}
