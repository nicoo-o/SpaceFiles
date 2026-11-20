// Journal d'erreurs (R8) : avec `panic = "abort"` l'app se ferme sans rien afficher. Le crochet de panic
// écrit la cause et l'endroit dans %LOCALAPPDATA%\SpaceFiles\journal.txt avant l'arrêt, pour qu'un
// plantage laisse une trace. Le fichier est plafonné : au-delà de 256 Ko il passe en journal.ancien.txt.
// `ecrire` est le point d'entrée unique pour toute autre trace (l'app n'a pas de console : rien d'autre ne se voit).

use std::io::Write;
use std::path::Path;

const MAX_OCTETS: u64 = 256 * 1024;

/// Gravité d'une trace du journal (`Avert` n'a pas encore d'appelant : le jeu complet fait partie du protocole)
#[allow(dead_code)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Niveau {
    Info,
    Avert,
    Erreur,
}

impl Niveau {
    fn libelle(self) -> &'static str {
        match self {
            Niveau::Info => "INFO",
            Niveau::Avert => "AVERT",
            Niveau::Erreur => "ERREUR",
        }
    }
}

/// Date et heure locales « AAAA-MM-JJ HH:MM:SS »
pub fn horodatage() -> String {
    let t = unsafe { windows::Win32::System::SystemInformation::GetLocalTime() };
    format!("{:04}-{:02}-{:02} {:02}:{:02}:{:02}", t.wYear, t.wMonth, t.wDay, t.wHour, t.wMinute, t.wSecond)
}

/// Une ligne de journal « DATE NIVEAU module message ». La date est un paramètre pour que la mise en forme se teste ;
/// les sauts de ligne du message deviennent des espaces (une trace = une ligne).
fn mettre_en_forme(date: &str, niveau: Niveau, module: &str, message: &str) -> String {
    let message = message.replace("\r\n", " ").replace(['\r', '\n'], " ");
    format!("{date} {} {module} {message}\r\n", niveau.libelle())
}

/// Ajoute une trace d'une ligne à journal.txt (même fichier et même plafond que les paniques).
/// Hors des chemins chauds (défilement, rendu, minuteries) : chaque appel ouvre et ferme le fichier.
pub fn ecrire(niveau: Niveau, module: &str, message: &str) {
    let Some(dossier) = crate::reglages::dossier_donnees(true) else { return };
    let _ = std::fs::create_dir_all(&dossier);
    ajouter(&dossier.join("journal.txt"), &mettre_en_forme(&horodatage(), niveau, module, message));
}

/// À appeler une fois, au début de `main`
pub fn installer() {
    std::panic::set_hook(Box::new(|info| {
        let Some(dossier) = crate::reglages::dossier_donnees(true) else { return };
        let _ = std::fs::create_dir_all(&dossier);
        let entete = format!("{} · version {}", horodatage(), env!("CARGO_PKG_VERSION"));
        let fil = std::thread::current().name().unwrap_or("sans nom").to_string();
        ajouter(&dossier.join("journal.txt"), &format!("{entete} · fil « {fil} »\r\n{info}\r\n\r\n"));
    }));
}

/// Ajoute au journal, en le faisant tourner s'il dépasse la taille maximale
fn ajouter(fichier: &Path, texte: &str) {
    if std::fs::metadata(fichier).is_ok_and(|m| m.len() > MAX_OCTETS) {
        let _ = std::fs::rename(fichier, fichier.with_file_name("journal.ancien.txt"));
    }
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(fichier) {
        let _ = f.write_all(texte.as_bytes());
    }
}

/// Remplace chaque chemin Windows d'un texte par son dernier élément : le journal affiché dans l'app ne montre
/// ni nom d'utilisateur ni arborescence (utile aussi quand on copie une capture dans un rapport public).
pub fn sans_chemins(texte: &str) -> String {
    texte
        .split(' ')
        .map(|mot| {
            let nu = mot.trim_matches(|c| matches!(c, '«' | '»' | '"' | '\'' | '(' | ')' | ',' | ';'));
            let est_chemin = nu.contains(":\\") || nu.starts_with("\\\\");
            if est_chemin {
                let fin = nu.rsplit('\\').find(|p| !p.is_empty()).unwrap_or("");
                format!("…\\{fin}")
            } else {
                mot.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Lit le journal d'erreurs et rend les traces d'erreur, d'avertissement et de plantage, de la plus récente à la
/// plus ancienne : (date et origine, message), chemins retirés. Les lignes d'information sont écartées.
pub fn analyser_traces(contenu: &str, max: usize) -> Vec<(String, String)> {
    let mut v: Vec<(String, String)> = Vec::new();
    for ligne in contenu.lines() {
        let l = ligne.trim();
        if l.len() < 20 || !l.as_bytes()[..4].iter().all(u8::is_ascii_digit) {
            continue;
        }
        let (date, reste) = l.split_at(19);
        let reste = reste.trim_start();
        if let Some(r) = reste.strip_prefix("ERREUR ").or_else(|| reste.strip_prefix("AVERT ")) {
            let niveau = if reste.starts_with("ERREUR") { "ERREUR" } else { "AVERT" };
            let (module, message) = r.split_once(' ').unwrap_or((r, ""));
            v.push((format!("{date} · {niveau} · {module}"), sans_chemins(message)));
        } else if reste.contains("· fil ") {
            v.push((format!("{date} · PLANTAGE"), sans_chemins(reste)));
        }
    }
    v.reverse();
    v.truncate(max);
    v
}

/// Les `max` dernières traces graves du journal courant (liste vide si le fichier est absent)
pub fn dernieres_traces(max: usize) -> Vec<(String, String)> {
    let Some(d) = crate::reglages::dossier_donnees(true) else { return Vec::new() };
    let contenu = std::fs::read(d.join("journal.txt")).map(|o| String::from_utf8_lossy(&o).into_owned()).unwrap_or_default();
    analyser_traces(&contenu, max)
}

pub fn fichier_operations() -> Option<std::path::PathBuf> {
    Some(crate::reglages::dossier_donnees(true)?.join("operations.csv"))
}

/// Combien d'opérations l'onglet JOURNAL du panneau terminal (T4c) garde en mémoire ; le dessin en montre ce qui tient
pub const OPERATIONS_AFFICHEES: usize = 200;

/// Nombre d'opérations écrites par ce processus depuis son lancement : l'onglet JOURNAL ne relit le fichier que quand il change
static ECRITURES: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

pub fn ecritures() -> usize {
    ECRITURES.load(std::sync::atomic::Ordering::Relaxed)
}

/// Une ligne du journal des opérations (`operations.csv`), affichée par l'onglet JOURNAL du panneau terminal (T4c)
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Operation {
    pub date: String,
    pub action: String,
    pub detail: String,
}

impl Operation {
    /// Heure seule : la date complète est déjà dans le fichier.
    pub fn heure(&self) -> &str {
        self.date.get(11..16).unwrap_or(&self.date)
    }
}

/// Les `max` dernières opérations, de la plus ancienne à la plus récente. Un fichier absent ou
/// illisible renvoie une liste vide : le panneau dira qu'il n'y a rien, sans inventer de commandes.
pub fn dernieres_operations(max: usize) -> Vec<Operation> {
    if max == 0 {
        return Vec::new();
    }
    let Some(fichier) = fichier_operations() else { return Vec::new() };
    let Ok(texte) = std::fs::read_to_string(fichier) else { return Vec::new() };
    dernieres_du_texte(&texte, max)
}

/// Les `max` dernières lignes valides du texte d'`operations.csv`, de la plus ancienne à la plus récente
fn dernieres_du_texte(texte: &str, max: usize) -> Vec<Operation> {
    let mut operations: Vec<Operation> = texte.lines().filter_map(analyser_operation).collect();
    if operations.len() > max {
        operations.drain(..operations.len() - max);
    }
    operations
}

/// Une ligne « date;action;détail ». Les guillemets doublés et les points-virgules d'un détail
/// sont respectés ; l'en-tête et les lignes incomplètes sont ignorés.
fn analyser_operation(ligne: &str) -> Option<Operation> {
    let ligne = ligne.trim_start_matches('\u{feff}').trim_end_matches('\r');
    if ligne.is_empty() || ligne.starts_with("DATE;") {
        return None;
    }
    let champs = champs_csv(ligne);
    if champs.len() != 3 || champs[0].is_empty() || champs[1].is_empty() {
        return None;
    }
    Some(Operation { date: champs[0].clone(), action: champs[1].clone(), detail: champs[2].clone() })
}

/// Découpe une ligne CSV à séparateur `;`, avec guillemets et guillemets doublés.
fn champs_csv(ligne: &str) -> Vec<String> {
    let mut champs = Vec::new();
    let mut champ = String::new();
    let mut entre_guillemets = false;
    let mut caracteres = ligne.chars().peekable();
    while let Some(c) = caracteres.next() {
        if entre_guillemets {
            if c == '"' {
                if caracteres.peek() == Some(&'"') {
                    caracteres.next();
                    champ.push('"');
                } else {
                    entre_guillemets = false;
                }
            } else {
                champ.push(c);
            }
        } else if c == '"' {
            entre_guillemets = true;
        } else if c == ';' {
            champs.push(std::mem::take(&mut champ));
        } else {
            champ.push(c);
        }
    }
    champs.push(champ);
    champs
}

/// Un champ CSV : entre guillemets (doublés à l'intérieur) dès qu'il contient `;`, `"` ou un saut de ligne
fn champ_csv(s: &str) -> String {
    if s.contains([';', '"', '\n', '\r']) {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s.to_string()
    }
}

/// Journal des opérations (Z7) : une ligne « date;action;détail » par copie, déplacement, mise à la corbeille
/// ou renommage, dans operations.csv (séparateur `;` : Excel en français l'ouvre en colonnes). Même plafond
/// que le journal d'erreurs. Une suppression définitive n'y figure pas : elle n'entre pas dans l'historique.
pub fn operation(action: &str, detail: &str) {
    let Some(fichier) = fichier_operations() else { return };
    if let Some(d) = fichier.parent() {
        let _ = std::fs::create_dir_all(d);
    }
    let date = horodatage();
    let neuf = !fichier.exists();
    // BOM UTF-8 en tête : sans lui Excel lit les accents en ANSI
    let entete = if neuf { "\u{feff}DATE;ACTION;DETAIL\r\n" } else { "" };
    ajouter(&fichier, &format!("{entete}{date};{};{}\r\n", champ_csv(action), champ_csv(detail)));
    ECRITURES.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Un chemin n'apparaît jamais en entier dans le journal affiché ; le reste du texte est intact
    #[test]
    fn chemins_retires_du_journal_affiche() {
        assert_eq!(sans_chemins("lecture de C:\\Users\\Jean\\Documents\\a.txt impossible"), "lecture de …\\a.txt impossible");
        assert_eq!(sans_chemins("« \\\\NAS\\partage\\x » refusé"), "« …\\x » refusé");
        assert_eq!(sans_chemins("rien à retirer"), "rien à retirer");
    }

    /// Les traces graves sortent de la plus récente à la plus ancienne ; l'information est écartée ; le plafond tient
    #[test]
    fn traces_graves_du_plus_recent_au_plus_ancien() {
        let j = "2026-11-14 10:00:00 INFO eco connecté\r\n2026-11-14 10:01:00 ERREUR index échec C:\\Users\\Jean\\x.db\r\n\
                 2026-11-14 10:02:00 AVERT reseau lent\r\n2026-11-14 10:03:00 · version 1.0 · fil « main »\r\npanicked at src\\a.rs\r\n";
        let t = analyser_traces(j, 10);
        assert_eq!(t.len(), 3);
        assert!(t[0].0.contains("PLANTAGE") && t[1].0.contains("AVERT") && t[2].0.contains("ERREUR"));
        assert!(!t[2].1.contains("Jean"));
        assert_eq!(analyser_traces(j, 2).len(), 2);
        assert!(analyser_traces("", 5).is_empty());
    }

    #[test]
    fn mise_en_forme_d_une_trace() {
        let l = mettre_en_forme("2026-11-01 09:05:03", Niveau::Avert, "systeme", "ouverture\r\nimpossible\nde x\r");
        assert_eq!(l, "2026-11-01 09:05:03 AVERT systeme ouverture impossible de x \r\n");
        assert_eq!(mettre_en_forme("d", Niveau::Info, "m", "ok"), "d INFO m ok\r\n");
        assert_eq!(mettre_en_forme("d", Niveau::Erreur, "m", "").lines().count(), 1);
    }

    #[test]
    fn champs_csv() {
        assert_eq!(champ_csv("COPIE"), "COPIE");
        assert_eq!(champ_csv("a;b"), "\"a;b\"");
        assert_eq!(champ_csv("dit \"oui\""), "\"dit \"\"oui\"\"\"");
    }

    #[test]
    fn terminal_lit_les_operations() {
        let o = analyser_operation("2026-11-05 10:04:03;COPIE;\"Vers D:\\Photos; triées\"").unwrap();
        assert_eq!(o.heure(), "10:04");
        assert_eq!(o.action, "COPIE");
        assert_eq!(o.detail, "Vers D:\\Photos; triées");
        let o = analyser_operation("2026-11-05 10:04:03;RENOMMAGE;\"dit \"\"oui\"\"\"").unwrap();
        assert_eq!(o.detail, "dit \"oui\"");
        assert!(analyser_operation("DATE;ACTION;DETAIL").is_none());
        assert!(analyser_operation("2026-11-05 10:04:03;COPIE").is_none());
        assert!(analyser_operation("").is_none());
        assert_eq!(Operation { date: "pas-une-date".into(), action: "X".into(), detail: String::new() }.heure(), "pas-une-date");
    }

    /// Le journal garde ses données (critère de T4c) : l'ordre du fichier est conservé, le plafond coupe les plus anciennes, et
    /// l'en-tête avec son BOM comme une ligne cassée sont ignorés
    #[test]
    fn terminal_garde_les_dernieres_operations_dans_l_ordre() {
        let t = "\u{feff}DATE;ACTION;DETAIL\r\n2026-11-05 10:00:00;COPIE;a\r\nligne cassée\r\n2026-11-05 10:01:00;DEPLACEMENT;b\r\n2026-11-05 10:02:00;CORBEILLE;c\r\n";
        let toutes = dernieres_du_texte(t, 10);
        assert_eq!(toutes.iter().map(|o| o.detail.as_str()).collect::<Vec<_>>(), ["a", "b", "c"]);
        let deux = dernieres_du_texte(t, 2);
        assert_eq!(deux.iter().map(|o| o.action.as_str()).collect::<Vec<_>>(), ["DEPLACEMENT", "CORBEILLE"]);
        assert!(dernieres_du_texte(t, 0).is_empty());
        assert!(dernieres_du_texte("", 5).is_empty());
    }

    /// Vraie panique d'un fil, dans un faux %LOCALAPPDATA% : à lancer seul (`--ignored`), il change l'environnement
    #[test]
    #[ignore]
    fn panique_reelle() {
        let d = std::env::temp_dir().join(format!("ne-panique-{}", std::process::id()));
        std::env::set_var("LOCALAPPDATA", &d);
        installer();
        let _ = std::thread::Builder::new().name("essai".into()).spawn(|| panic!("boum de test")).unwrap().join();
        let _ = std::panic::take_hook();
        let texte = std::fs::read_to_string(d.join(crate::reglages::NOM_DONNEES).join("journal.txt")).unwrap();
        assert!(texte.contains("boum de test") && texte.contains("journal.rs") && texte.contains("« essai »"), "{texte}");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn rotation() {
        let d = std::env::temp_dir().join(format!("ne-journal-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        let j = d.join("journal.txt");
        ajouter(&j, "un\r\n");
        ajouter(&j, "deux\r\n");
        assert_eq!(std::fs::read_to_string(&j).unwrap(), "un\r\ndeux\r\n");
        // Trop gros : l'ancien est mis de côté, le nouveau repart de zéro
        std::fs::write(&j, vec![b'x'; MAX_OCTETS as usize + 1]).unwrap();
        ajouter(&j, "neuf\r\n");
        assert_eq!(std::fs::read_to_string(&j).unwrap(), "neuf\r\n");
        assert!(d.join("journal.ancien.txt").exists());
        let _ = std::fs::remove_dir_all(&d);
    }
}
