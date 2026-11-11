// SpaceFiles comme explorateur par défaut : Win+E, double-clic sur un dossier, « Ouvrir le dossier »
// des autres applications. Uniquement dans HKEY_CURRENT_USER (pas besoin d'être administrateur),
// réversible en un clic : on ajoute notre commande et on la rend « par défaut », sans toucher à celles de Windows.

use windows::core::{HSTRING, PCWSTR};
use windows::Win32::Foundation::ERROR_SUCCESS;
use windows::Win32::System::Registry::*;
use windows::Win32::UI::Shell::{SHChangeNotify, SHCNE_ASSOCCHANGED, SHCNF_IDLIST};

const VERBE: &str = "SpaceFiles";
const LIBELLE_VERBE: &str = "Ouvrir dans SpaceFiles";
/// Nom des mêmes entrées avant le renommage : reprises puis retirées par `migrer_anciennes_entrees`
const VERBE_ANCIEN: &str = "NothingExplorer";
/// Nom de l'exe d'avant : une ancienne entrée n'est retirée que si sa commande lance cet exe
const EXE_ANCIEN: &str = "nothing-explorer.exe";
/// Dossiers ordinaires, disques, et tous les dossiers du Shell
const CLASSES: [&str; 3] = ["Folder", "Directory", "Drive"];
/// Essai précédent pour Win+E (objet « Explorateur de fichiers » copié dans HKCU), retiré s'il est de nous
const ANCIEN_WIN_E: &str = r"Software\Classes\CLSID\{52205fd8-5dfb-447d-801a-d0b52f2e83e1}";

/// `SPACEFILES_SANS_REGISTRE=1` : ni lecture ni écriture des entrées de SpaceFiles dans le registre. Sert aux
/// lancements d'essai (tools/, vérifications) pour ne jamais toucher au vrai HKCU de l'utilisateur.
fn sans_registre_depuis(valeur: Option<&std::ffi::OsStr>) -> bool {
    valeur.is_some_and(|v| v == "1")
}

pub(crate) fn sans_registre() -> bool {
    sans_registre_depuis(std::env::var_os("SPACEFILES_SANS_REGISTRE").as_deref())
}

fn ecrire_valeur(cle: &str, nom: &str, valeur: &str) -> bool {
    if sans_registre() {
        return false;
    }
    unsafe {
        let mut h = HKEY::default();
        if RegCreateKeyExW(HKEY_CURRENT_USER, &HSTRING::from(cle), 0, None, REG_OPTION_NON_VOLATILE, KEY_WRITE, None, &mut h, None) != ERROR_SUCCESS {
            return false;
        }
        let w: Vec<u16> = valeur.encode_utf16().chain(std::iter::once(0)).collect();
        let octets = std::slice::from_raw_parts(w.as_ptr() as *const u8, w.len() * 2);
        let ok = RegSetValueExW(h, &HSTRING::from(nom), 0, REG_SZ, Some(octets)) == ERROR_SUCCESS;
        let _ = RegCloseKey(h);
        ok
    }
}

/// Valeur par défaut d'une clé (créée au besoin)
fn ecrire(cle: &str, valeur: &str) -> bool {
    ecrire_valeur(cle, "", valeur)
}

/// Valeur d'une clé de HKEY_CURRENT_USER ("" : la valeur par défaut)
pub fn lire_valeur(cle: &str, nom: &str) -> Option<String> {
    unsafe {
        let mut buf = [0u16; 512];
        let mut taille = (buf.len() * 2) as u32;
        let r = RegGetValueW(HKEY_CURRENT_USER, &HSTRING::from(cle), &HSTRING::from(nom), RRF_RT_REG_SZ, None, Some(buf.as_mut_ptr() as *mut _), Some(&mut taille));
        if r != ERROR_SUCCESS {
            return None;
        }
        let n = buf.iter().position(|&c| c == 0).unwrap_or(0);
        Some(String::from_utf16_lossy(&buf[..n]))
    }
}

/// Lecture d'une entrée de SpaceFiles (verbes, Run) : soumise à `SPACEFILES_SANS_REGISTRE`, contrairement à
/// `lire_valeur`, qui sert aussi à lire un réglage système
fn lire_nos(cle: &str, nom: &str) -> Option<String> {
    if sans_registre() {
        return None;
    }
    lire_valeur(cle, nom)
}

fn lire(cle: &str) -> Option<String> {
    lire_nos(cle, "")
}

fn supprimer_valeur(cle: &str, nom: &str) {
    if sans_registre() {
        return;
    }
    unsafe {
        let _ = RegDeleteKeyValueW(HKEY_CURRENT_USER, &HSTRING::from(cle), &HSTRING::from(nom));
    }
}

fn supprimer_arbre(cle: &str) {
    if sans_registre() {
        return;
    }
    unsafe {
        let _ = RegDeleteTreeW(HKEY_CURRENT_USER, &HSTRING::from(cle));
    }
}

// ── Win+E : résident lancé avec la session (clé Run de HKCU), voir raccourci.rs ──

const RUN: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
const RESIDENT: &str = "SpaceFilesWinE";
const RESIDENT_ANCIEN: &str = "NothingExplorerWinE";

pub fn win_e_actif() -> bool {
    lire_nos(RUN, RESIDENT).is_some() || lire_nos(RUN, RESIDENT_ANCIEN).is_some()
}

/// Win+E ouvre SpaceFiles : le résident démarre maintenant et à chaque ouverture de session
pub fn activer_win_e() -> bool {
    let Ok(exe) = std::env::current_exe() else { return false };
    migrer_anciennes_entrees();
    let ok = ecrire_valeur(RUN, RESIDENT, &format!("\"{}\" --resident", exe.display()));
    if ok {
        crate::raccourci::demarrer();
    }
    ok
}

/// Retour à Windows : la clé Run disparaît et le résident se ferme
pub fn desactiver_win_e() {
    migrer_anciennes_entrees();
    supprimer_valeur(RUN, RESIDENT);
    crate::raccourci::arreter();
}

pub fn est_actif() -> bool {
    // L'ancien nom compte aussi tant que la reprise n'a pas eu lieu (elle se fait au lancement, en arrière-plan)
    matches!(lire(r"Software\Classes\Folder\shell").as_deref(), Some(VERBE) | Some(VERBE_ANCIEN))
}

/// Au lancement : reprend les entrées d'avant le renommage, retire l'essai Win+E d'une version précédente
pub fn actualiser() {
    migrer_anciennes_entrees();
    retirer_ancien_win_e();
    // Résident fermé (exe remplacé, tué…) alors que Win+E est activé : on le relance (il ne tourne qu'une fois)
    if win_e_actif() {
        crate::raccourci::demarrer();
    }
}

/// Valeur « par défaut » d'avant nous, gardée à côté pour que le retour soit exact
const ANCIENNE: &str = "SpaceFilesAncienne";
const ANCIENNE_ANCIEN: &str = "NothingExplorerAncienne";

pub fn activer() -> bool {
    let Ok(exe) = std::env::current_exe() else { return false };
    migrer_anciennes_entrees();
    let commande = format!("\"{}\" \"%1\"", exe.display());
    let mut ok = true;
    for c in CLASSES {
        let base = format!(r"Software\Classes\{c}\shell");
        ok &= ecrire(&format!(r"{base}\{VERBE}"), LIBELLE_VERBE);
        ok &= ecrire(&format!(r"{base}\{VERBE}\command"), &commande);
        // Commande écrite en entier avant de devenir la valeur par défaut : sinon un double-clic ne ferait rien
        if !ok {
            break;
        }
        if let Some(avant) = lire(&base).filter(|v| v != VERBE) {
            ok &= ecrire_valeur(&base, ANCIENNE, &avant);
        }
        ok &= ecrire(&base, VERBE);
    }
    if !ok {
        desactiver();
        return false;
    }
    prevenir();
    true
}

/// Retour exact à Windows : notre commande disparaît, la valeur d'avant revient
pub fn desactiver() {
    migrer_anciennes_entrees();
    for c in CLASSES {
        let base = format!(r"Software\Classes\{c}\shell");
        supprimer_arbre(&format!(r"{base}\{VERBE}"));
        unsafe {
            if lire(&base).as_deref() == Some(VERBE) {
                match lire_valeur(&base, ANCIENNE) {
                    Some(avant) => {
                        ecrire(&base, &avant);
                    }
                    None => {
                        let _ = RegDeleteKeyValueW(HKEY_CURRENT_USER, &HSTRING::from(base.as_str()), PCWSTR::null());
                    }
                }
            }
        }
        supprimer_valeur(&base, ANCIENNE);
    }
    retirer_ancien_win_e();
    prevenir();
}

fn retirer_ancien_win_e() {
    let commande = lire(&format!(r"{ANCIEN_WIN_E}\shell\opennewwindow\command")).unwrap_or_default();
    if commande.to_lowercase().contains("nothing-explorer") {
        supprimer_arbre(ANCIEN_WIN_E);
        if !sans_registre() {
            unsafe {
                let _ = RegDeleteKeyW(HKEY_CURRENT_USER, &HSTRING::from(ANCIEN_WIN_E));
            }
        }
    }
}

/// État lu d'une classe (`Folder`, `Directory`, `Drive`) : tout ce que la décision de reprise regarde
#[derive(Debug, Default, Clone, PartialEq)]
struct EtatClasse {
    /// Commande de l'ancien verbe `NothingExplorer` (`None` : il n'existe pas)
    ancien_verbe: Option<String>,
    nouveau_verbe: bool,
    /// Valeur par défaut de `…\shell`
    defaut: Option<String>,
    sauvegarde: Option<String>,
    ancienne_sauvegarde: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
enum ActionClasse {
    CreerVerbe,
    CopierSauvegarde(String),
    DefautVersNouveau,
    SupprimerAncienVerbe,
    SupprimerAncienneSauvegarde,
}

/// Une commande qui lance l'exe d'avant le renommage (jamais l'entrée d'un autre programme)
fn lance_ancien_exe(commande: &str) -> bool {
    let c = commande.trim().trim_start_matches('"');
    let bas = c.to_lowercase();
    let Some(fin) = bas.find(".exe") else { return false };
    let chemin = &bas[..fin + 4];
    chemin.rsplit(['\\', '/']).next() == Some(EXE_ANCIEN)
}

/// Que faire d'une classe à partir de son état, dans l'ordre : la valeur de sauvegarde est copiée et le nouveau
/// verbe créé avant que quoi que ce soit d'ancien disparaisse, jamais d'état sans l'un ni l'autre
fn decider_classe(e: &EtatClasse) -> Vec<ActionClasse> {
    let nous = e.ancien_verbe.as_deref().map(lance_ancien_exe);
    if nous == Some(false) {
        return Vec::new();
    }
    let mut a = Vec::new();
    if let (Some(v), None) = (&e.ancienne_sauvegarde, &e.sauvegarde) {
        a.push(ActionClasse::CopierSauvegarde(v.clone()));
    }
    if nous == Some(true) {
        if !e.nouveau_verbe {
            a.push(ActionClasse::CreerVerbe);
        }
        if e.defaut.as_deref() == Some(VERBE_ANCIEN) {
            a.push(ActionClasse::DefautVersNouveau);
        }
        a.push(ActionClasse::SupprimerAncienVerbe);
    }
    if e.ancienne_sauvegarde.is_some() {
        a.push(ActionClasse::SupprimerAncienneSauvegarde);
    }
    a
}

#[derive(Debug, Clone, PartialEq)]
enum ActionRun {
    CreerNouveau,
    SupprimerAncien,
}

/// Même règle pour la valeur de la clé Run : la nouvelle est créée avant que l'ancienne soit retirée
fn decider_run(ancien: Option<&str>, nouveau: bool) -> Vec<ActionRun> {
    match ancien {
        Some(c) if lance_ancien_exe(c) => {
            let mut a = Vec::new();
            if !nouveau {
                a.push(ActionRun::CreerNouveau);
            }
            a.push(ActionRun::SupprimerAncien);
            a
        }
        _ => Vec::new(),
    }
}

/// Reprend les entrées d'avant le renommage (verbes de dossier, valeur de sauvegarde, valeur Run) vers les nouvelles,
/// sans rien perdre : le « retour exact » de `desactiver` continue de marcher
fn migrer_anciennes_entrees() {
    if sans_registre() {
        return;
    }
    let Ok(exe) = std::env::current_exe() else { return };
    for c in CLASSES {
        let base = format!(r"Software\Classes\{c}\shell");
        let etat = EtatClasse {
            ancien_verbe: lire(&format!(r"{base}\{VERBE_ANCIEN}\command")),
            nouveau_verbe: lire(&format!(r"{base}\{VERBE}\command")).is_some(),
            defaut: lire(&base),
            sauvegarde: lire_nos(&base, ANCIENNE),
            ancienne_sauvegarde: lire_nos(&base, ANCIENNE_ANCIEN),
        };
        for action in decider_classe(&etat) {
            match action {
                ActionClasse::CopierSauvegarde(v) => {
                    ecrire_valeur(&base, ANCIENNE, &v);
                }
                ActionClasse::CreerVerbe => {
                    ecrire(&format!(r"{base}\{VERBE}"), LIBELLE_VERBE);
                    ecrire(&format!(r"{base}\{VERBE}\command"), &format!("\"{}\" \"%1\"", exe.display()));
                }
                ActionClasse::DefautVersNouveau => {
                    ecrire(&base, VERBE);
                }
                ActionClasse::SupprimerAncienVerbe => supprimer_arbre(&format!(r"{base}\{VERBE_ANCIEN}")),
                ActionClasse::SupprimerAncienneSauvegarde => supprimer_valeur(&base, ANCIENNE_ANCIEN),
            }
        }
    }
    let ancien = lire_nos(RUN, RESIDENT_ANCIEN);
    for action in decider_run(ancien.as_deref(), lire_nos(RUN, RESIDENT).is_some()) {
        match action {
            ActionRun::CreerNouveau => {
                ecrire_valeur(RUN, RESIDENT, &format!("\"{}\" --resident", exe.display()));
            }
            ActionRun::SupprimerAncien => supprimer_valeur(RUN, RESIDENT_ANCIEN),
        }
    }
    prevenir();
}

fn prevenir() {
    unsafe { SHChangeNotify(SHCNE_ASSOCCHANGED, SHCNF_IDLIST, None, None) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ActionClasse::*;

    const ANCIEN_CMD: &str = r#""C:\Users\x\AppData\Local\Programs\NothingExplorer\nothing-explorer.exe" "%1""#;

    fn etat() -> EtatClasse {
        EtatClasse::default()
    }

    #[test]
    fn rien_d_installe_ne_fait_rien() {
        assert!(decider_classe(&etat()).is_empty());
        assert!(decider_run(None, false).is_empty());
    }

    #[test]
    fn ancien_seul_est_repris_puis_retire() {
        let e = EtatClasse { ancien_verbe: Some(ANCIEN_CMD.into()), defaut: Some("NothingExplorer".into()), ancienne_sauvegarde: Some("none".into()), ..etat() };
        assert_eq!(decider_classe(&e), vec![CopierSauvegarde("none".into()), CreerVerbe, DefautVersNouveau, SupprimerAncienVerbe, SupprimerAncienneSauvegarde]);
    }

    #[test]
    fn ancien_sans_sauvegarde_ni_defaut() {
        let e = EtatClasse { ancien_verbe: Some(ANCIEN_CMD.into()), defaut: Some("autre".into()), ..etat() };
        assert_eq!(decider_classe(&e), vec![CreerVerbe, SupprimerAncienVerbe]);
    }

    #[test]
    fn nouveau_seul_ne_change_rien() {
        let e = EtatClasse { nouveau_verbe: true, defaut: Some("SpaceFiles".into()), sauvegarde: Some("none".into()), ..etat() };
        assert!(decider_classe(&e).is_empty());
        assert!(decider_run(None, true).is_empty());
    }

    #[test]
    fn les_deux_gardent_le_nouveau_et_sa_sauvegarde() {
        let e = EtatClasse { ancien_verbe: Some(ANCIEN_CMD.into()), nouveau_verbe: true, defaut: Some("SpaceFiles".into()), sauvegarde: Some("A".into()), ancienne_sauvegarde: Some("B".into()), ..etat() };
        // la sauvegarde déjà prise par la nouvelle version n'est pas écrasée par l'ancienne
        assert_eq!(decider_classe(&e), vec![SupprimerAncienVerbe, SupprimerAncienneSauvegarde]);
    }

    #[test]
    fn ancien_vers_un_autre_programme_est_laisse() {
        let e = EtatClasse { ancien_verbe: Some(r#""C:\Autre\autre.exe" "%1""#.into()), defaut: Some("NothingExplorer".into()), ancienne_sauvegarde: Some("none".into()), ..etat() };
        assert!(decider_classe(&e).is_empty());
        assert!(decider_run(Some(r#""C:\Autre\autre.exe" --resident"#), false).is_empty());
        // un nom qui ressemble seulement à l'ancien exe n'est pas le sien
        assert!(!lance_ancien_exe(r"C:\x\ancien-nothing-explorer.exe"));
        assert!(!lance_ancien_exe(""));
    }

    #[test]
    fn sauvegarde_ancienne_seule_est_copiee() {
        let e = EtatClasse { ancienne_sauvegarde: Some("none".into()), ..etat() };
        assert_eq!(decider_classe(&e), vec![CopierSauvegarde("none".into()), SupprimerAncienneSauvegarde]);
    }

    #[test]
    fn run_ancien_est_cree_puis_retire() {
        let cmd = r#""C:\P\NothingExplorer\nothing-explorer.exe" --resident"#;
        assert_eq!(decider_run(Some(cmd), false), vec![ActionRun::CreerNouveau, ActionRun::SupprimerAncien]);
        assert_eq!(decider_run(Some(cmd), true), vec![ActionRun::SupprimerAncien]);
        assert!(lance_ancien_exe(r"C:\Program Files\NE\Nothing-Explorer.EXE --resident"));
    }

    #[test]
    fn variable_sans_registre() {
        use std::ffi::OsStr;
        assert!(sans_registre_depuis(Some(OsStr::new("1"))));
        assert!(!sans_registre_depuis(Some(OsStr::new("0"))));
        assert!(!sans_registre_depuis(Some(OsStr::new(""))));
        assert!(!sans_registre_depuis(None));
        // Avec la variable posée, aucune lecture n'a lieu, même sur une clé qui existe (réglage système sans
        // rapport avec nous) : c'est ce qui protège le vrai HKCU pendant les lancements d'essai
        std::env::set_var("SPACEFILES_SANS_REGISTRE", "1");
        assert!(sans_registre());
        assert_eq!(lire_nos(r"Control Panel\International", "LocaleName"), None);
        assert!(!est_actif() && !win_e_actif());
        std::env::remove_var("SPACEFILES_SANS_REGISTRE");
    }
}
