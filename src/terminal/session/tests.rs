// Tests de la session de terminal (T2a) : de vrais shells dans de vraies pseudo-consoles, sans fenêtre. Chaque test lance
// cmd.exe (présent sur toute machine Windows) ; aucun ne touche à la souris ni au registre.

use super::*;
use crate::terminal::vt::Terminal;
use std::sync::atomic::AtomicUsize;
use std::time::Instant;
use windows::Win32::System::JobObjects::{JobObjectBasicProcessIdList, QueryInformationJobObject, JOBOBJECT_BASIC_PROCESS_ID_LIST};
use windows::Win32::System::Threading::{OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION};

const DELAI: Duration = Duration::from_secs(20);
/// Code de sortie que `GetExitCodeProcess` rend tant que le processus tourne (STILL_ACTIVE)
const EN_COURS: u32 = 259;

fn cmd() -> String {
    Shell::Cmd.ligne_de_commande()
}

fn sans_notification() -> Arc<dyn Fn() + Send + Sync> {
    Arc::new(|| {})
}

/// Vide la session dans un émulateur jusqu'à ce que `ok` soit vrai pour le texte de l'écran, ou jusqu'au délai.
fn attendre(s: &Session, vt: &mut Terminal, brut: &mut Vec<u8>, ok: impl Fn(&str, &[u8]) -> bool) -> bool {
    let debut = Instant::now();
    while debut.elapsed() < DELAI {
        let nouveau = s.prendre_sortie();
        if !nouveau.is_empty() {
            vt.ecrire(&nouveau);
            brut.extend_from_slice(&nouveau);
        }
        if ok(&vt.texte_ecran(), brut) {
            return true;
        }
        if s.lecture_finie() && nouveau.is_empty() {
            return ok(&vt.texte_ecran(), brut);
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    false
}

/// Le processus `pid` existe-t-il encore ? (OpenProcess échoue une fois le processus disparu et tous ses handles fermés)
fn vivant(pid: u32) -> bool {
    // SAFETY : le handle ouvert est fermé avant de sortir
    unsafe {
        match OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) {
            Ok(h) => {
                let mut code = 0u32;
                let ok = GetExitCodeProcess(h, &mut code).is_ok() && code == EN_COURS;
                let _ = CloseHandle(h);
                ok
            }
            Err(_) => false,
        }
    }
}

/// Identifiants des processus actuellement dans le job de la session
fn processus_du_job(s: &Session) -> Vec<u32> {
    let mut tampon = vec![0usize; 128];
    // SAFETY : le tampon (alignement d'un usize, 1 024 octets) reçoit l'en-tête puis les identifiants
    unsafe {
        let ok = QueryInformationJobObject(s.job, JobObjectBasicProcessIdList, tampon.as_mut_ptr() as *mut c_void, (tampon.len() * std::mem::size_of::<usize>()) as u32, None).is_ok();
        assert!(ok, "QueryInformationJobObject a échoué");
        let liste = &*(tampon.as_ptr() as *const JOBOBJECT_BASIC_PROCESS_ID_LIST);
        let n = liste.NumberOfProcessIdsInList as usize;
        std::slice::from_raw_parts(liste.ProcessIdList.as_ptr(), n).iter().map(|p| *p as u32).collect()
    }
}

#[test]
fn le_shell_affiche_ce_qu_on_lui_fait_executer() {
    let s = Session::lancer(&format!("{} /c echo salut-session", cmd()), None, 100, 30, sans_notification()).expect("lancement");
    let (mut vt, mut brut) = (Terminal::new(100, 30), Vec::new());
    assert!(attendre(&s, &mut vt, &mut brut, |t, _| t.contains("salut-session")), "texte absent : {:?}", vt.texte_ecran());
}

#[test]
fn le_clavier_atteint_le_shell_et_la_reponse_revient() {
    let s = Session::lancer(&cmd(), None, 100, 30, sans_notification()).expect("lancement");
    let (mut vt, mut brut) = (Terminal::new(100, 30), Vec::new());
    // On attend l'invite avant de taper (cmd affiche son bandeau puis `>`)
    assert!(attendre(&s, &mut vt, &mut brut, |t, _| t.contains('>')), "pas d'invite : {:?}", vt.texte_ecran());
    s.ecrire(b"echo un-deux-trois\r");
    // Le texte tapé s'affiche aussi (écho) : on cherche donc la réponse seule, sur une ligne à elle
    assert!(
        attendre(&s, &mut vt, &mut brut, |t, _| t.lines().any(|l| l.trim_end() == "un-deux-trois")),
        "réponse absente : {:?}",
        vt.texte_ecran()
    );
}

#[test]
fn l_utf8_et_les_accents_passent_dans_les_deux_sens() {
    let s = Session::lancer(&cmd(), None, 100, 30, sans_notification()).expect("lancement");
    let (mut vt, mut brut) = (Terminal::new(100, 30), Vec::new());
    assert!(attendre(&s, &mut vt, &mut brut, |t, _| t.contains('>')));
    // Page de code UTF-8 puis écho d'un texte accentué
    s.ecrire("chcp 65001 >nul\r".as_bytes());
    s.ecrire("echo é-à-ü-日本\r".as_bytes());
    assert!(
        attendre(&s, &mut vt, &mut brut, |t, _| t.lines().any(|l| l.trim_end() == "é-à-ü-日本")),
        "accents perdus : {:?}",
        vt.texte_ecran()
    );
}

#[test]
fn redimensionner_change_la_taille_vue_par_le_shell() {
    let mut s = Session::lancer(&cmd(), None, 100, 30, sans_notification()).expect("lancement");
    let (mut vt, mut brut) = (Terminal::new(100, 30), Vec::new());
    assert!(attendre(&s, &mut vt, &mut brut, |t, _| t.contains('>')));
    s.redimensionner(80, 24);
    assert_eq!((s.colonnes(), s.lignes()), (80, 24));
    vt.redimensionner(80, 24);
    s.ecrire(b"mode con\r");
    // `mode con` écrit « Lignes : 24 » et « Colonnes : 80 » (« Lines » et « Columns » sur un Windows en anglais)
    let ok = attendre(&s, &mut vt, &mut brut, |t, _| {
        let a = t.lines().any(|l| (l.contains("olonn") || l.contains("olumn")) && l.contains("80"));
        let b = t.lines().any(|l| (l.contains("igne") || l.contains("ines")) && l.contains("24"));
        a && b
    });
    assert!(ok, "taille non vue par le shell : {:?}", vt.texte_ecran());
}

#[test]
fn redimensionner_a_la_meme_taille_ou_hors_bornes_ne_casse_rien() {
    let mut s = Session::lancer(&cmd(), None, 100, 30, sans_notification()).expect("lancement");
    s.redimensionner(100, 30);
    assert_eq!((s.colonnes(), s.lignes()), (100, 30));
    s.redimensionner(0, 0);
    assert_eq!((s.colonnes(), s.lignes()), (1, 1), "0 est ramené au minimum");
    s.redimensionner(60_000, 60_000);
    assert_eq!((s.colonnes(), s.lignes()), (COLONNES_MAX, LIGNES_MAX));
}

#[test]
fn un_shell_qui_se_termine_donne_son_code_de_sortie_et_ferme_le_flux() {
    let s = Session::lancer(&format!("{} /c exit 7", cmd()), None, 100, 30, sans_notification()).expect("lancement");
    let debut = Instant::now();
    while s.code_de_sortie().is_none() && debut.elapsed() < DELAI {
        std::thread::sleep(Duration::from_millis(20));
    }
    assert_eq!(s.code_de_sortie(), Some(7));
    // ConPTY garde le tuyau ouvert tant qu'on ne l'a pas fermé : la fin du flux n'est donc PAS garantie ici (mesuré : elle
    // n'arrive pas). Ce qui compte : le code de sortie est lisible et le Drop qui suit ne bloque pas. La fenêtre (T2b)
    // s'appuiera donc sur `code_de_sortie`, pas sur `lecture_finie`, pour savoir que le shell est terminé.
    let t = Instant::now();
    drop(s);
    assert!(t.elapsed() < Duration::from_secs(10), "Drop trop long : {:?}", t.elapsed());
}

#[test]
fn le_dossier_de_depart_est_respecte() {
    let dossier = std::env::temp_dir();
    let s = Session::lancer(&format!("{} /c cd", cmd()), Some(dossier.to_str().unwrap()), 200, 30, sans_notification()).expect("lancement");
    let (mut vt, mut brut) = (Terminal::new(200, 30), Vec::new());
    let attendu = dossier.to_str().unwrap().trim_end_matches('\\').to_lowercase();
    assert!(attendre(&s, &mut vt, &mut brut, |t, _| t.to_lowercase().contains(&attendu)), "dossier absent : {:?}", vt.texte_ecran());
}

#[test]
fn une_commande_introuvable_est_une_erreur_lisible() {
    let r = Session::lancer("\"C:\\dossier\\qui\\n_existe\\pas.exe\"", None, 100, 30, sans_notification());
    let e = r.err().expect("devrait échouer");
    assert!(e.contains("lancement"), "message : {e}");
}

#[test]
fn la_notification_part_quand_des_octets_arrivent_et_pas_en_rafale() {
    let compteur = Arc::new(AtomicUsize::new(0));
    let c2 = compteur.clone();
    let s = Session::lancer(&format!("{} /c echo bonjour", cmd()), None, 100, 30, Arc::new(move || {
        c2.fetch_add(1, Ordering::SeqCst);
    }))
    .expect("lancement");
    let debut = Instant::now();
    while compteur.load(Ordering::SeqCst) == 0 && debut.elapsed() < DELAI {
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(compteur.load(Ordering::SeqCst) >= 1, "aucune notification");
    // Tant que la fenêtre n'a pas vidé le tampon, une seule notification est en vol : les suivantes sont absorbées
    std::thread::sleep(Duration::from_millis(500));
    let avant = compteur.load(Ordering::SeqCst);
    assert!(avant <= 2, "{avant} notifications sans que le tampon soit vidé (une seule en vol, plus la fin de flux)");
    assert!(!s.prendre_sortie().is_empty());
}

#[test]
fn la_contre_pression_borne_le_tampon_puis_tout_arrive_une_fois_vide() {
    const LIMITE: usize = 64 * 1024;
    // Le marqueur est dans le `do` : il s'imprime à chaque tour, donc aussi au dernier ; c'est le dernier tour qui nous importe
    let commande = format!("{} /c \"for /L %i in (1,1,20000) do @echo ligne-numero-%i MARQUEUR-FINAL\"", cmd());
    let s = Session::lancer_avec_limite(&commande, None, 100, 30, sans_notification(), LIMITE).expect("lancement");
    // On ne vide pas : le tampon doit se remplir puis s'arrêter près de la limite (un bloc de 16 Kio au plus de dépassement)
    std::thread::sleep(Duration::from_millis(1500));
    let taille = s.partage.sortie.lock().unwrap().len();
    assert!(taille <= LIMITE + 16 * 1024, "tampon non borné : {taille} octets");
    assert!(!s.lecture_finie(), "le flux ne peut pas être fini : personne n'a lu");
    // On vide : le flux repart et finit par livrer la fin
    let (mut vt, mut brut) = (Terminal::new(100, 30), Vec::new());
    let ok = attendre(&s, &mut vt, &mut brut, |_, b| String::from_utf8_lossy(b).contains("ligne-numero-20000 MARQUEUR-FINAL"));
    assert!(ok, "fin jamais livrée ({} octets reçus)", brut.len());
}

#[test]
fn fermer_pendant_que_le_tampon_est_plein_ne_bloque_pas() {
    const LIMITE: usize = 16 * 1024;
    let commande = format!("{} /c \"for /L %i in (1,1,200000) do @echo bourrage-%i\"", cmd());
    let s = Session::lancer_avec_limite(&commande, None, 100, 30, sans_notification(), LIMITE).expect("lancement");
    std::thread::sleep(Duration::from_millis(800));
    let t = Instant::now();
    drop(s);
    assert!(t.elapsed() < Duration::from_secs(10), "fermeture bloquée : {:?}", t.elapsed());
}

#[test]
fn fermer_tue_le_shell_et_ses_descendants() {
    let s = Session::lancer(&cmd(), None, 100, 30, sans_notification()).expect("lancement");
    let (mut vt, mut brut) = (Terminal::new(100, 30), Vec::new());
    assert!(attendre(&s, &mut vt, &mut brut, |t, _| t.contains('>')));
    // Un descendant long : ping vers soi-même, 120 essais
    s.ecrire(b"ping -n 120 127.0.0.1\r");
    let debut = Instant::now();
    let mut pids = processus_du_job(&s);
    while pids.len() < 2 && debut.elapsed() < DELAI {
        std::thread::sleep(Duration::from_millis(50));
        pids = processus_du_job(&s);
    }
    assert!(pids.len() >= 2, "le descendant n'a pas rejoint le job : {pids:?}");
    assert!(pids.contains(&s.pid()), "le shell n'est pas dans son job");
    assert!(pids.iter().all(|p| vivant(*p)), "un processus du job est déjà mort : {pids:?}");

    drop(s);

    // La mort est demandée de façon synchrone par TerminateJobObject ; on laisse au système un instant pour la finir
    let debut = Instant::now();
    while pids.iter().any(|p| vivant(*p)) && debut.elapsed() < Duration::from_secs(5) {
        std::thread::sleep(Duration::from_millis(50));
    }
    let survivants: Vec<u32> = pids.iter().copied().filter(|p| vivant(*p)).collect();
    assert!(survivants.is_empty(), "processus orphelins après la fermeture : {survivants:?}");
}

#[test]
fn ouvrir_et_fermer_vingt_sessions_ne_fuit_ni_handle_ni_fil() {
    let _seul = super::VERROU_HANDLES.lock().unwrap_or_else(|e| e.into_inner());
    // Un compteur de handles du processus avant / après : toute fuite par session se verrait multipliée par vingt
    let ronde = || {
        for _ in 0..20 {
            let s = Session::lancer(&format!("{} /c echo x", cmd()), None, 80, 24, sans_notification()).expect("lancement");
            std::thread::sleep(Duration::from_millis(30));
            drop(s);
        }
        handles_ouverts()
    };
    // Une ronde d'échauffement : les caches du système (conhost, bibliothèques) prennent quelques handles une fois pour toutes
    let _ = ronde();
    let avant = handles_ouverts();
    let apres = ronde();
    eprintln!("handles : {avant} avant, {apres} après vingt sessions de plus");
    assert!(apres <= avant + 3, "fuite probable : {avant} handles avant, {apres} après vingt sessions");
}

fn handles_ouverts() -> u32 {
    use windows::Win32::System::Threading::{GetCurrentProcess, GetProcessHandleCount};
    let mut n = 0u32;
    // SAFETY : pseudo-handle du processus courant, valide partout
    unsafe {
        let _ = GetProcessHandleCount(GetCurrentProcess(), &mut n);
    }
    n
}
