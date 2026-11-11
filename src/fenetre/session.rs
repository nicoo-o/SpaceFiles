// Session de la fenêtre : onglets enregistrés dans les réglages, emplacement de départ, icône et DPI.

use crate::*;
/// Onglet enregistré dans les réglages : un chemin, ou « ce-pc »
pub(crate) fn vers_session(e: &Emplacement) -> Option<String> {
    match e {
        Emplacement::CePC => Some("ce-pc".into()),
        Emplacement::Dossier(c) | Emplacement::Recherche { racine: c, .. } => Some(c.clone()),
        Emplacement::Serveur(h) => Some(format!("\\\\{h}")),
        Emplacement::Archive { fichier, .. } => Some(dossier::dossier_de(fichier).to_string()),
        Emplacement::Sftp { hote, chemin } => Some(sftp::adresse(hote, chemin)),
        Emplacement::Reglages | Emplacement::Globale { .. } | Emplacement::Groupe(_) | Emplacement::Recents | Emplacement::Corbeille | Emplacement::Panneau | Emplacement::Doublons(_) => None,
    }
}

/// Dossiers épinglés, pour le volet
pub(crate) fn epingles_volet() -> Vec<app::Favori> {
    reglages::actuels()
        .epingles
        .iter()
        .map(|c| app::Favori { nom: dossier::nom_affiche(c).to_uppercase(), emplacement: Emplacement::dossier(c), icone: icones::Icone::Dossier })
        .collect()
}

pub(crate) fn depuis_session(s: &str) -> Option<Emplacement> {
    if s == "ce-pc" {
        return Some(Emplacement::CePC);
    }
    // Un onglet SFTP / WebDAV mémorisé n'est pas rouvert quand les fonctions expérimentales sont coupées (sans erreur) ;
    // l'onglet reste dans les réglages jusqu'à la prochaine fermeture
    if !reglages::adresse_permise_maintenant(s) {
        return None;
    }
    // Un dossier disparu depuis (clé USB retirée…) n'est pas rouvert ; un serveur l'est (il peut être lent à répondre)
    (s.starts_with("\\\\") || s.starts_with("sftp://") || std::path::Path::new(s).is_dir()).then(|| Emplacement::dossier(s))
}

/// Ouverture sans dossier précis : selon les réglages, le dossier personnel, Ce PC ou le dernier dossier
pub(crate) fn depart_reglage(r: &reglages::Reglages) -> Emplacement {
    let profil = std::env::var("USERPROFILE").ok().map(|c| Emplacement::dossier(&c)).unwrap_or(Emplacement::CePC);
    match r.depart {
        Depart::Personnel => profil,
        Depart::CePC => Emplacement::CePC,
        Depart::Dernier => depuis_session(&r.dernier).unwrap_or(profil),
    }
}

/// `\\hôte` seul : pas un dossier pour Windows, mais la fenêtre sait lister ses partages
fn serveur_seul(c: &str) -> bool {
    c.strip_prefix(r"\\").is_some_and(|h| !h.is_empty() && !h.trim_end_matches('\\').contains('\\'))
}

/// Racine d'un lecteur : `C:\` (ou `C:`)
fn racine_de_lecteur(c: &str) -> bool {
    let o = c.as_bytes();
    o.len() >= 2 && o[0].is_ascii_alphabetic() && o[1] == b':' && c[2..].trim_end_matches('\\').is_empty()
}

/// Où ouvrir la fenêtre et quoi y sélectionner, pour `--ouvrir <chemin>` (`reveler` faux) et `--montrer <chemin>`.
/// Décidé ICI, dans la nouvelle fenêtre, en une seule consultation du disque (`est_dossier` n'est appelé qu'une fois :
/// un dossier remplacé par un fichier entre deux questions ne donne pas deux réponses) :
/// - un dossier, sans `reveler` : ouvert, rien de sélectionné ;
/// - sinon (un fichier, un chemin qui n'existe pas, ou `reveler`) : le dossier parent, l'élément sélectionné ;
/// - `\\hôte` et la racine d'un lecteur n'ont pas de parent : ouverts tels quels.
/// Jamais l'Explorateur de Windows, jamais l'exécution du fichier. Les `\` finaux sont ignorés (`C:\a\` = `C:\a`).
pub(crate) fn resoudre_ouverture(chemin: &str, reveler: bool, est_dossier: impl Fn(&str) -> bool) -> (String, Option<String>) {
    let c = chemin.trim();
    if racine_de_lecteur(c) {
        return (format!("{}\\", &c[..2]), None);
    }
    if serveur_seul(c) {
        return (c.trim_end_matches('\\').to_string(), None);
    }
    let net = c.trim_end_matches('\\');
    if !reveler && est_dossier(c) {
        return (c.to_string(), None);
    }
    let parent = dossier::dossier_de(net);
    let nom = net.rsplit('\\').next().unwrap_or(net);
    (parent.to_string(), Some(nom.to_string()).filter(|n| !n.is_empty() && parent != net))
}

/// La résolution de la ligne de commande, calculée une fois (le disque n'est consulté qu'une fois pour tout le processus)
pub(crate) fn ouverture_demandee() -> Option<(String, Option<String>)> {
    static R: std::sync::OnceLock<Option<(String, Option<String>)>> = std::sync::OnceLock::new();
    R.get_or_init(|| {
        let mut args = std::env::args().skip(1);
        let reveler = match args.next()?.trim().trim_matches('"') {
            "--montrer" => true,
            "--ouvrir" => false,
            _ => return None,
        };
        let c = args.next().unwrap_or_default();
        (!c.trim().is_empty()).then(|| resoudre_ouverture(&c, reveler, |p| std::path::Path::new(p).is_dir()))
    })
    .clone()
}

/// La sorte de la première ligne de commande, jamais sa valeur : un chemin ne doit pas finir dans le journal
pub(crate) fn sorte_d_arguments(args: &[String]) -> &'static str {
    let Some(a) = args.first() else { return "aucun" };
    match a.trim().trim_matches('"') {
        "" => "vide",
        "--montrer" => "--montrer",
        "--ouvrir" => "--ouvrir",
        "--groupe" => "--groupe",
        "--chercher" => "--chercher",
        "--resident" => "--resident",
        "--rapide" => "ancien argument",
        _ => "dossier ou autre",
    }
}

/// Arguments d'anciennes versions, acceptés et ignorés
fn est_ancien_argument(a: &str) -> bool {
    a == "--rapide"
}

/// Dossier de départ d'après la ligne de commande (Win+E, double-clic, autres applications).
/// None : un dossier virtuel de Windows, confié à l'Explorateur. Some(None) : pas de dossier précis.
pub(crate) fn depart() -> Option<Option<Emplacement>> {
    let profil = || std::env::var("USERPROFILE").ok().map(|c| Emplacement::dossier(&c)).unwrap_or(Emplacement::CePC);
    let Some(arg) = std::env::args().nth(1) else { return Some(None) };
    let a = arg.trim().trim_matches('"').trim();
    if a.is_empty() {
        return Some(None);
    }
    // Arguments « un groupe » ou « un fichier à montrer dans son dossier »
    if a == "--groupe" {
        return Some(Some(Emplacement::Groupe(std::env::args().skip(2).collect::<Vec<_>>().join(" "))));
    }
    // `--montrer` (fichier à montrer) et `--ouvrir` (demande d'une autre application) : résolus ici, dans CE processus
    if a == "--montrer" || a == "--ouvrir" {
        return Some(ouverture_demandee().map(|(d, _)| Emplacement::dossier(&d)));
    }
    if let Some(emp) = a.get(..6).filter(|p| p.eq_ignore_ascii_case("shell:")).and_then(|_| dossier::dossier_shell(&a[6..])) {
        return Some(Some(emp));
    }
    if let Some((emp, _)) = dossier::emplacement_depuis_url(a) {
        // Lien vers un serveur SFTP / WebDAV alors que les fonctions expérimentales sont coupées : dossier personnel
        // (la fenêtre n'existe pas encore, pas de barre d'état pour dire pourquoi)
        let refuse = emp.experimentale().is_some_and(|g| !reglages::fonction_experimentale_permise(reglages::experimental(), g));
        return Some(Some(if refuse { profil() } else { emp }));
    }
    // Ancien argument de la recherche rapide Alt+Espace (retirée) : un vieux raccourci peut encore le passer.
    // Ignoré, sinon il filerait vers l'Explorateur de Windows ; la fenêtre s'ouvre comme sans argument.
    if est_ancien_argument(a) {
        return Some(None);
    }
    if a.starts_with("sftp://") {
        return Some(Some(if reglages::adresse_permise_maintenant(a) { Emplacement::dossier(a) } else { profil() }));
    }
    if a == "--chercher" {
        let motif: Vec<String> = std::env::args().skip(2).collect();
        return Some(Some(Emplacement::Globale { motif: motif.join(" ") }));
    }
    let u = a.to_uppercase();
    // Ce PC
    if u.contains("20D04FE0-3AEA-1069-A2D8-08002B30309D") {
        return Some(Some(Emplacement::CePC));
    }
    // Accueil, Accès rapide : le dossier personnel
    if u.contains("F874310E-B6B7-47DC-BC84-B9E6B38F5903") || u.contains("679F85CB-0220-4080-B29B-5540CC05AAB6") {
        return Some(Some(profil()));
    }
    // `\\serveur` seul n'est pas un dossier pour Windows, mais nous savons lister ses partages
    let serveur = a.strip_prefix("\\\\").is_some_and(|h| !h.is_empty() && !h.trim_end_matches('\\').contains('\\'));
    if serveur || std::path::Path::new(a).is_dir() {
        return Some(Some(Emplacement::dossier(a)));
    }
    // Archive (zip, 7z…) : parcourue comme un dossier
    let ext = a.rsplit_once('.').map(|(_, e)| e.to_uppercase()).unwrap_or_default();
    if archive::est_archive(&ext) && std::path::Path::new(a).is_file() {
        return Some(Some(Emplacement::Archive { fichier: a.to_string(), interne: String::new() }));
    }
    // Panneau de configuration, Corbeille, bibliothèques, .zip… : confiés à l'Explorateur lui-même.
    // Surtout pas ShellExecute « open » sur l'argument : quand on est l'explorateur par défaut, il nous relancerait en boucle.
    let windows = std::env::var("WINDIR").unwrap_or_else(|_| r"C:\Windows".into());
    unsafe {
        ShellExecuteW(
            None,
            None,
            &HSTRING::from(format!(r"{windows}\explorer.exe")),
            &HSTRING::from(format!("\"{a}\"")),
            None,
            SW_SHOWNORMAL,
        );
    }
    None
}

/// Icône en points (assets/spacefiles.ico, images PNG) : on prend la plus petite image au moins aussi grande
pub(crate) fn icone(taille: i32) -> HICON {
    const ICO: &[u8] = include_bytes!("../../assets/spacefiles.ico");
    let mot = |i: usize| u32::from_le_bytes([ICO[i], ICO[i + 1], ICO[i + 2], ICO[i + 3]]) as usize;
    let nombre = u16::from_le_bytes([ICO[4], ICO[5]]) as usize;
    let images = (0..nombre).map(|n| {
        let e = 6 + 16 * n;
        let cote = if ICO[e] == 0 { 256 } else { ICO[e] as i32 };
        (cote, mot(e + 12), mot(e + 8))
    });
    let choix = images.clone().filter(|i| i.0 >= taille).min_by_key(|i| i.0).or_else(|| images.max_by_key(|i| i.0));
    let Some((_, debut, longueur)) = choix else { return HICON::default() };
    unsafe {
        CreateIconFromResourceEx(&ICO[debut..debut + longueur], true, 0x0003_0000, taille, taille, LR_DEFAULTCOLOR)
            .unwrap_or_default()
    }
}

/// Échelle imposée par SPACEFILES_DPI (96, 144, 192…) : sert aux captures à 100 / 150 / 200 % sans changer l'écran
pub(crate) fn dpi_force() -> Option<u32> {
    std::env::var("SPACEFILES_DPI").ok()?.parse().ok().filter(|d| (48..=480).contains(d))
}

pub(crate) fn dpi_de(hwnd: HWND) -> u32 {
    // SAFETY: GetDpiForWindow ne lit que la fenêtre
    dpi_force().unwrap_or_else(|| unsafe { GetDpiForWindow(hwnd) })
}

// ── Sauvegarde continue de la session, marqueur de fermeture anormale ──

/// Nom du marqueur dans `%LOCALAPPDATA%\SpaceFiles` : présent tant qu'au moins une fenêtre principale est ouverte
pub(crate) const MARQUEUR_SESSION: &str = "session-ouverte";

/// Calme attendu après un changement d'onglets avant d'écrire la session (ms) : regroupe les changements rapprochés
pub(crate) const ATTENTE_SESSION_MS: u32 = 2000;

/// Ce que `main` fait des onglets enregistrés
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Restauration {
    Rien,
    /// Fermeture normale et réglage « RESTAURER LES ONGLETS »
    Normale,
    /// Le dernier arrêt n'était pas propre : les onglets reviennent même si le réglage est faux
    ApresPlantage,
}

/// Faut-il rouvrir les onglets enregistrés ? Un dossier demandé (clic, argument) l'emporte toujours
pub(crate) fn faut_restaurer(restaurer_onglets: bool, marqueur_present: bool, depart_explicite: bool) -> Restauration {
    if depart_explicite {
        Restauration::Rien
    } else if marqueur_present {
        Restauration::ApresPlantage
    } else if restaurer_onglets {
        Restauration::Normale
    } else {
        Restauration::Rien
    }
}

/// Session à enregistrer : (onglets, rang de l'onglet actif). Les onglets SFTP / WebDAV que `permis` refuse
/// (fonctions expérimentales coupées, donc jamais ouverts) restent dans la liste précédente à leur rang :
/// ils ne disparaissent pas des réglages. Le rang compte aussi ces onglets gelés.
pub(crate) fn session_a_ecrire(emplacements: &[Emplacement], rang: usize, precedents: &[String], permis: &dyn Fn(&str) -> bool) -> (Vec<String>, usize) {
    let mut onglets: Vec<String> = emplacements.iter().filter_map(vers_session).collect();
    let mut actif = emplacements[..rang.min(emplacements.len())].iter().filter(|e| vers_session(e).is_some()).count();
    for (i, s) in precedents.iter().enumerate().filter(|(_, s)| !permis(s)) {
        let place = i.min(onglets.len());
        onglets.insert(place, s.clone());
        if place <= actif {
            actif += 1;
        }
    }
    let actif = if onglets.is_empty() { 0 } else { actif.min(onglets.len() - 1) };
    (onglets, actif)
}

/// Rang de l'onglet actif une fois les onglets non rouvrables retirés (`gardes[i]` : l'onglet enregistré i s'ouvre)
pub(crate) fn rang_apres_filtre(gardes: &[bool], actif: usize) -> usize {
    gardes.iter().take(actif).filter(|g| **g).count()
}

/// La session calculée diffère-t-elle de celle des réglages ? Sinon rien à écrire
pub(crate) fn session_differe(r: &reglages::Reglages, onglets: &[String], actif: usize, dernier: Option<&str>) -> bool {
    r.onglets != onglets || r.onglet_actif != actif || dernier.is_some_and(|d| r.dernier != d)
}

/// Contenu du marqueur avec ce processus en plus (une ligne par processus à fenêtre principale)
pub(crate) fn marqueur_avec(contenu: &str, pid: u32) -> String {
    let mut pids = pids_du_marqueur(contenu);
    if !pids.contains(&pid) {
        pids.push(pid);
    }
    pids.iter().map(|p| format!("{p}\n")).collect()
}

/// Contenu du marqueur sans ce processus ; vide quand c'était le dernier (le fichier se supprime alors)
pub(crate) fn marqueur_sans(contenu: &str, pid: u32) -> String {
    pids_du_marqueur(contenu).into_iter().filter(|p| *p != pid).map(|p| format!("{p}\n")).collect()
}

fn pids_du_marqueur(contenu: &str) -> Vec<u32> {
    contenu.lines().filter_map(|l| l.trim().parse().ok()).collect()
}

/// Le marqueur trouvé au démarrage dit-il que le dernier arrêt n'était pas propre ? Oui si le fichier existe et
/// qu'aucun des processus qu'il liste ne tourne encore (sinon c'est une autre fenêtre, ouverte en même temps)
pub(crate) fn marqueur_signale_plantage(contenu: Option<&str>, vivant: &dyn Fn(u32) -> bool) -> bool {
    contenu.is_some_and(|c| !pids_du_marqueur(c).into_iter().any(vivant))
}

fn processus_vivant(pid: u32) -> bool {
    use windows::Win32::System::Threading::{GetExitCodeProcess, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION};
    // SAFETY: le handle ouvert ici est fermé avant de sortir ; GetExitCodeProcess n'écrit que dans `code`
    unsafe {
        let Ok(h) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) else { return false };
        let mut code = 0u32;
        let ok = GetExitCodeProcess(h, &mut code).is_ok();
        let _ = windows::Win32::Foundation::CloseHandle(h);
        ok && code == 259 // STILL_ACTIVE
    }
}

fn chemin_marqueur() -> Option<std::path::PathBuf> {
    Some(reglages::dossier_donnees(true)?.join(MARQUEUR_SESSION))
}

/// Au démarrage, avant de poser notre marqueur : le dernier arrêt était-il anormal ?
pub(crate) fn arret_precedent_anormal() -> bool {
    let contenu = chemin_marqueur().and_then(|c| std::fs::read_to_string(c).ok());
    marqueur_signale_plantage(contenu.as_deref(), &processus_vivant)
}

/// Une fenêtre principale s'ouvre : notre processus entre dans le marqueur
pub(crate) fn poser_marqueur() {
    let Some(c) = chemin_marqueur() else { return };
    if let Some(d) = c.parent() {
        let _ = std::fs::create_dir_all(d);
    }
    let avant = std::fs::read_to_string(&c).unwrap_or_default();
    // Les processus morts d'un arrêt anormal ancien ne doivent pas retenir le fichier : on ne garde que les vivants
    let vivants: String = avant.lines().filter_map(|l| l.trim().parse::<u32>().ok()).filter(|p| processus_vivant(*p)).map(|p| format!("{p}\n")).collect();
    let _ = std::fs::write(&c, marqueur_avec(&vivants, std::process::id()));
}

/// Fermeture normale (ou fin de session Windows) : notre processus sort du marqueur, le fichier part avec le dernier
pub(crate) fn lever_marqueur() {
    let Some(c) = chemin_marqueur() else { return };
    let reste = marqueur_sans(&std::fs::read_to_string(&c).unwrap_or_default(), std::process::id());
    if reste.is_empty() {
        let _ = std::fs::remove_file(&c);
    } else {
        let _ = std::fs::write(&c, reste);
    }
}

/// Signature légère de ce que la session enregistre, comparée à chaque rendu : un changement arme la minuterie
pub(crate) fn vue_session(f: &Fenetre) -> (Vec<Option<String>>, usize) {
    (f.emplacements().iter().map(vers_session).collect(), f.rang)
}

/// Écrit la session dans les réglages si elle a changé ; vrai si une écriture a eu lieu
pub(crate) fn sauver_session(f: &Fenetre) -> bool {
    let emps = f.emplacements();
    let rang = f.rang;
    let dernier = vers_session(&f.app.emplacement);
    let r = reglages::actuels();
    let (onglets, actif) = session_a_ecrire(&emps, rang, &r.onglets, &reglages::adresse_permise_maintenant);
    if !session_differe(&r, &onglets, actif, dernier.as_deref()) {
        return false;
    }
    reglages::modifier(|r| {
        // `modifier` relit le fichier : les onglets gelés à conserver sont ceux du fichier, pas ceux de notre copie
        let (onglets, actif) = session_a_ecrire(&emps, rang, &r.onglets, &reglages::adresse_permise_maintenant);
        r.onglets = onglets;
        r.onglet_actif = actif;
        if let Some(d) = dernier {
            r.dernier = d;
        }
    });
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tout_permis(_: &str) -> bool {
        true
    }
    fn sans_sftp(s: &str) -> bool {
        !s.starts_with("sftp://")
    }
    fn d(c: &str) -> Emplacement {
        Emplacement::dossier(c)
    }

    #[test]
    fn ancien_argument_rapide_ignore() {
        assert!(est_ancien_argument("--rapide"));
        for a in ["--resident", "--groupe", "--montrer", "--ouvrir", "--chercher", "C:\\", ""] {
            assert!(!est_ancien_argument(a), "{a:?} ne doit pas être ignoré");
        }
    }

    /// `--ouvrir` / `--montrer` : résolus dans la nouvelle fenêtre, jamais confiés à l'Explorateur, une seule question au disque
    #[test]
    fn resolution_de_l_ouverture() {
        use std::cell::Cell;
        let dossiers = |c: &str| matches!(c, r"C:\Users" | r"C:\Users\" | r"\\hote\partage" | r"D:\");
        let r = |c: &str, rev: bool| resoudre_ouverture(c, rev, dossiers);
        let s = |d: &str, n: Option<&str>| (d.to_string(), n.map(str::to_string));
        // Dossier : ouvert tel quel, rien de sélectionné
        assert_eq!(r(r"C:\Users", false), s(r"C:\Users", None));
        assert_eq!(r(r"C:\Users\", false), s(r"C:\Users\", None));
        assert_eq!(r(r"\\hote\partage", false), s(r"\\hote\partage", None));
        // Fichier ou chemin inexistant : dossier parent, élément choisi (jamais exécuté, jamais l'Explorateur)
        assert_eq!(r(r"C:\Users\a.txt", false), s(r"C:\Users", Some("a.txt")));
        assert_eq!(r(r"C:\inexistant", false), s(r"C:\", Some("inexistant")));
        assert_eq!(r(r"C:\Windows\notepad.exe", false), s(r"C:\Windows", Some("notepad.exe")));
        assert_eq!(r(r"\\hote\partage\x.txt", false), s(r"\\hote\partage", Some("x.txt")));
        // Chemin qui finit par `\` mais n'est pas un dossier connu : le `\` final ne donne pas un nom vide
        assert_eq!(r(r"C:\inexistant\", false), s(r"C:\", Some("inexistant")));
        // reveal : le parent et l'élément, même pour un dossier
        assert_eq!(r(r"C:\Users", true), s(r"C:\", Some("Users")));
        assert_eq!(r(r"C:\Users\a.txt", true), s(r"C:\Users", Some("a.txt")));
        // Racine d'un lecteur et serveur seul : pas de parent, ouverts tels quels, avec ou sans reveal
        for rev in [false, true] {
            assert_eq!(r(r"C:\", rev), s(r"C:\", None), "racine, reveal {rev}");
            assert_eq!(r(r"D:\", rev), s(r"D:\", None));
            assert_eq!(r(r"\\hote", rev), s(r"\\hote", None), "serveur seul, reveal {rev}");
            assert_eq!(r(r"\\hote\", rev), s(r"\\hote", None));
        }
        // Une seule consultation du disque par résolution (course dossier/fichier)
        for (c, rev) in [(r"C:\Users", false), (r"C:\x", false), (r"C:\Users", true), (r"C:\", false)] {
            let n = Cell::new(0);
            let _ = resoudre_ouverture(c, rev, |_| {
                n.set(n.get() + 1);
                true
            });
            assert!(n.get() <= 1, "{c} : {} consultations", n.get());
        }
        // Un « dossier » devenu fichier entre-temps : la réponse du disque (non) donne le parent, de façon cohérente
        assert_eq!(resoudre_ouverture(r"C:\x", false, |_| false), s(r"C:\", Some("x")));
    }

    /// Le journal de démarrage ne contient que la sorte d'argument, jamais sa valeur
    #[test]
    fn journal_sans_valeur_d_argument() {
        let v = |a: &[&str]| a.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(sorte_d_arguments(&v(&[])), "aucun");
        assert_eq!(sorte_d_arguments(&v(&["--ouvrir", r"C:\Secret\plan.docx"])), "--ouvrir");
        assert_eq!(sorte_d_arguments(&v(&["--montrer", r"C:\Secret"])), "--montrer");
        assert_eq!(sorte_d_arguments(&v(&[r"C:\Secret\dossier"])), "dossier ou autre");
        assert_eq!(sorte_d_arguments(&v(&["\"\""])), "vide");
        for a in [&v(&["--ouvrir", "secret-abc"])[..], &v(&["secret-abc"])[..], &v(&["--groupe", "secret-abc"])[..]] {
            assert!(!sorte_d_arguments(a).contains("secret"));
        }
    }

    #[test]
    fn restauration_selon_les_trois_entrees() {
        assert_eq!(faut_restaurer(false, false, false), Restauration::Rien);
        assert_eq!(faut_restaurer(true, false, false), Restauration::Normale);
        assert_eq!(faut_restaurer(false, true, false), Restauration::ApresPlantage);
        assert_eq!(faut_restaurer(true, true, false), Restauration::ApresPlantage);
    }

    #[test]
    fn un_dossier_demande_l_emporte_sur_tout() {
        for r in [false, true] {
            for m in [false, true] {
                assert_eq!(faut_restaurer(r, m, true), Restauration::Rien);
            }
        }
    }

    #[test]
    fn session_ecrite_comme_avant_sans_onglet_gele() {
        let emps = [d("C:\\a"), Emplacement::Reglages, d("C:\\b"), Emplacement::CePC];
        // Actif = « C:\b » (rang 2) : un seul onglet enregistrable avant lui (Réglages n'en est pas un)
        let (o, a) = session_a_ecrire(&emps, 2, &[], &tout_permis);
        assert_eq!(o, vec!["C:\\a", "C:\\b", "ce-pc"]);
        assert_eq!(a, 1);
    }

    #[test]
    fn session_ecrite_onglet_unique() {
        assert_eq!(session_a_ecrire(&[d("C:\\a")], 0, &[], &tout_permis), (vec!["C:\\a".to_string()], 0));
    }

    #[test]
    fn session_ecrite_sans_onglet_enregistrable() {
        assert_eq!(session_a_ecrire(&[Emplacement::Reglages], 0, &[], &tout_permis), (vec![], 0));
    }

    #[test]
    fn onglet_gele_conserve_a_son_rang() {
        let precedents = vec!["C:\\a".to_string(), "sftp://h/x".to_string(), "C:\\b".to_string()];
        let (o, a) = session_a_ecrire(&[d("C:\\a"), d("C:\\b")], 1, &precedents, &sans_sftp);
        assert_eq!(o, vec!["C:\\a", "sftp://h/x", "C:\\b"]);
        // L'onglet actif est « C:\b » : sa place compte l'onglet gelé placé devant lui
        assert_eq!(a, 2);
    }

    #[test]
    fn onglet_gele_en_tete_decale_l_actif() {
        let precedents = vec!["sftp://h/x".to_string()];
        let (o, a) = session_a_ecrire(&[d("C:\\a")], 0, &precedents, &sans_sftp);
        assert_eq!(o, vec!["sftp://h/x", "C:\\a"]);
        assert_eq!(a, 1);
    }

    #[test]
    fn onglet_ferme_et_permis_disparait() {
        let precedents = vec!["C:\\a".to_string(), "C:\\vieux".to_string()];
        let (o, _) = session_a_ecrire(&[d("C:\\a")], 0, &precedents, &sans_sftp);
        assert_eq!(o, vec!["C:\\a"]);
    }

    #[test]
    fn rang_apres_filtre_compte_les_gardes_avant_l_actif() {
        assert_eq!(rang_apres_filtre(&[true, false, true, true], 3), 2);
        assert_eq!(rang_apres_filtre(&[false, true], 1), 0);
        assert_eq!(rang_apres_filtre(&[true, true], 9), 2);
    }

    #[test]
    fn rien_a_ecrire_quand_la_session_est_identique() {
        let r = reglages::Reglages { onglets: vec!["C:\\a".into()], onglet_actif: 0, dernier: "C:\\a".into(), ..Default::default() };
        assert!(!session_differe(&r, &["C:\\a".to_string()], 0, Some("C:\\a")));
        assert!(!session_differe(&r, &["C:\\a".to_string()], 0, None));
        assert!(session_differe(&r, &["C:\\a".to_string(), "C:\\b".to_string()], 0, Some("C:\\a")));
        assert!(session_differe(&r, &["C:\\a".to_string()], 1, Some("C:\\a")));
        assert!(session_differe(&r, &["C:\\a".to_string()], 0, Some("C:\\b")));
    }

    #[test]
    fn marqueur_liste_les_processus() {
        let m = marqueur_avec("", 10);
        let m = marqueur_avec(&m, 20);
        assert_eq!(m, "10\n20\n");
        assert_eq!(marqueur_avec(&m, 10), m);
        assert_eq!(marqueur_sans(&m, 10), "20\n");
        assert_eq!(marqueur_sans("20\n", 20), "");
    }

    #[test]
    fn plantage_si_marqueur_sans_processus_vivant() {
        let aucun = |_: u32| false;
        let tous = |_: u32| true;
        assert!(!marqueur_signale_plantage(None, &tous));
        assert!(marqueur_signale_plantage(Some("10\n20\n"), &aucun));
        assert!(!marqueur_signale_plantage(Some("10\n20\n"), &|p| p == 20));
        // Marqueur vide ou illisible : l'arrêt n'était pas propre
        assert!(marqueur_signale_plantage(Some(""), &tous));
        assert!(marqueur_signale_plantage(Some("n'importe quoi"), &tous));
    }

    #[test]
    fn mon_processus_est_vivant() {
        assert!(processus_vivant(std::process::id()));
    }
}
