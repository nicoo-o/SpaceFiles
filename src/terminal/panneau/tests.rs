//! Tests du panneau de terminal : tout ce qui se décide sans fenêtre ni shell (touches, collage, défilement, sélection).

use super::*;
use crate::terminal::souris::Bouton;

/// Un panneau sans session : la sortie du « shell » est écrite à la main dans l'émulateur
fn sans_session(colonnes: usize, lignes: usize) -> Panneau {
    Panneau {
        session: None,
        vt: Terminal::avec_historique(colonnes, lignes, 100),
        decalage: 0,
        ancre: None,
        fin_selection: None,
        erreur: None,
        fini: None,
        substitut: None,
        dossier: "C:\\essai".to_string(),
        dossier_connu: "C:\\essai".to_string(),
        annonces_vues: 0,
        invite_prete: false,
        frappe: false,
        en_attente: false,
        shell: Shell::Cmd,
        derniere_case_souris: None,
        ligne: Default::default(),
    }
}

fn nu() -> Mods {
    Mods::default()
}

fn modes(appli: bool) -> Modes {
    Modes { touches_curseur_appli: appli, ..Modes::default() }
}

fn octets(vk: u32, m: Mods, appli: bool) -> Option<Vec<u8>> {
    touche_vers_octets(vk, m, &modes(appli))
}

// ---------------------------------------------------------------- touches

#[test]
fn les_fleches_suivent_le_mode_curseur_application() {
    assert_eq!(octets(0x26, nu(), false).unwrap(), b"\x1b[A");
    assert_eq!(octets(0x28, nu(), false).unwrap(), b"\x1b[B");
    assert_eq!(octets(0x27, nu(), false).unwrap(), b"\x1b[C");
    assert_eq!(octets(0x25, nu(), false).unwrap(), b"\x1b[D");
    assert_eq!(octets(0x26, nu(), true).unwrap(), b"\x1bOA");
    assert_eq!(octets(0x24, nu(), true).unwrap(), b"\x1bOH");
    assert_eq!(octets(0x23, nu(), false).unwrap(), b"\x1b[F");
}

#[test]
fn les_modificateurs_passent_en_parametre_meme_en_mode_application() {
    let ctrl = Mods { ctrl: true, ..nu() };
    let maj = Mods { maj: true, ..nu() };
    let alt = Mods { alt: true, ..nu() };
    let tout = Mods { ctrl: true, alt: true, maj: true };
    assert_eq!(octets(0x27, ctrl, false).unwrap(), b"\x1b[1;5C");
    assert_eq!(octets(0x27, ctrl, true).unwrap(), b"\x1b[1;5C");
    assert_eq!(octets(0x25, maj, false).unwrap(), b"\x1b[1;2D");
    assert_eq!(octets(0x25, alt, false).unwrap(), b"\x1b[1;3D");
    assert_eq!(octets(0x26, tout, false).unwrap(), b"\x1b[1;8A");
    assert_eq!(octets(0x2E, ctrl, false).unwrap(), b"\x1b[3;5~");
}

#[test]
fn page_insertion_suppression_et_touches_de_fonction() {
    assert_eq!(octets(0x21, nu(), false).unwrap(), b"\x1b[5~");
    assert_eq!(octets(0x22, nu(), false).unwrap(), b"\x1b[6~");
    assert_eq!(octets(0x2D, nu(), false).unwrap(), b"\x1b[2~");
    assert_eq!(octets(0x2E, nu(), false).unwrap(), b"\x1b[3~");
    assert_eq!(octets(0x70, nu(), false).unwrap(), b"\x1bOP");
    assert_eq!(octets(0x73, nu(), false).unwrap(), b"\x1bOS");
    assert_eq!(octets(0x74, nu(), false).unwrap(), b"\x1b[15~");
    assert_eq!(octets(0x79, nu(), false).unwrap(), b"\x1b[21~");
    assert_eq!(octets(0x7B, nu(), false).unwrap(), b"\x1b[24~");
    assert_eq!(octets(0x70, Mods { ctrl: true, ..nu() }, false).unwrap(), b"\x1b[1;5P");
    assert_eq!(octets(0x7C, nu(), false), None, "F13 n'existe pas ici");
}

#[test]
fn entree_tab_echap_et_retour_arriere() {
    assert_eq!(octets(0x0D, nu(), false).unwrap(), b"\r");
    assert_eq!(octets(0x0D, Mods { ctrl: true, ..nu() }, false).unwrap(), b"\n");
    assert_eq!(octets(0x09, nu(), false).unwrap(), b"\t");
    assert_eq!(octets(0x09, Mods { maj: true, ..nu() }, false).unwrap(), b"\x1b[Z");
    assert_eq!(octets(0x1B, nu(), false).unwrap(), b"\x1b");
    assert_eq!(octets(0x08, nu(), false).unwrap(), b"\x7f");
    assert_eq!(octets(0x08, Mods { ctrl: true, ..nu() }, false).unwrap(), b"\x08");
    assert_eq!(octets(0x08, Mods { alt: true, ..nu() }, false).unwrap(), b"\x1b\x7f");
}

#[test]
fn alt_lettre_envoie_echap_puis_la_lettre() {
    let alt = Mods { alt: true, ..nu() };
    assert_eq!(octets(0x42, alt, false).unwrap(), b"\x1bb");
    assert_eq!(octets(0x42, Mods { alt: true, maj: true, ctrl: false }, false).unwrap(), b"\x1bB");
    assert_eq!(octets(0x35, alt, false).unwrap(), b"\x1b5");
    // sans Alt, une lettre arrive par WM_CHAR : rien à traduire ici
    assert_eq!(octets(0x42, nu(), false), None);
    // Ctrl+Alt (AltGr) ne doit pas devenir ESC + lettre
    assert_eq!(octets(0x42, Mods { alt: true, ctrl: true, maj: false }, false), None);
}

#[test]
fn les_defilements_clavier_sont_reserves_a_maj_et_ctrl_maj() {
    let maj = Mods { maj: true, ..nu() };
    let ctrl_maj = Mods { ctrl: true, maj: true, alt: false };
    assert_eq!(defilement_de_touche(0x21, maj), Some(Defil::PageHaut));
    assert_eq!(defilement_de_touche(0x22, maj), Some(Defil::PageBas));
    assert_eq!(defilement_de_touche(0x24, ctrl_maj), Some(Defil::Debut));
    assert_eq!(defilement_de_touche(0x23, ctrl_maj), Some(Defil::Fin));
    assert_eq!(defilement_de_touche(0x21, nu()), None, "PgHaut seul va au shell");
    assert_eq!(defilement_de_touche(0x24, maj), None, "Maj+Début seul va au shell");
}

// ---------------------------------------------------------------- texte reçu et collé

#[test]
fn les_unites_utf16_deviennent_de_l_utf8() {
    let mut p = sans_session(20, 5);
    assert_eq!(p.octets_de_unite('a' as u16), b"a");
    assert_eq!(p.octets_de_unite('é' as u16), "é".as_bytes());
    // 😀 = U+1F600 = D83D DE00 : rien après la moitié haute, le caractère entier après la basse
    assert!(p.octets_de_unite(0xD83D).is_empty());
    assert_eq!(p.octets_de_unite(0xDE00), "😀".as_bytes());
    // une moitié basse orpheline est ignorée, la suite continue de marcher
    assert!(p.octets_de_unite(0xDE00).is_empty());
    assert_eq!(p.octets_de_unite('z' as u16), b"z");
    // une moitié haute abandonnée est remplacée par la suivante
    assert!(p.octets_de_unite(0xD83D).is_empty());
    assert_eq!(p.octets_de_unite('b' as u16), b"b");
}

#[test]
fn le_collage_normalise_les_fins_de_ligne_et_retire_les_controles() {
    let p = sans_session(20, 5);
    assert_eq!(p.octets_de_collage("a\r\nb\nc\rd"), b"a\rb\rc\rd");
    assert_eq!(p.octets_de_collage("x\x1b[201~y"), b"x[201~y", "un ESC collé est retiré");
    assert_eq!(p.octets_de_collage("a\tb"), b"a\tb");
}

#[test]
fn le_collage_est_encadre_seulement_si_le_shell_l_a_demande() {
    let mut p = sans_session(20, 5);
    assert_eq!(p.octets_de_collage("ls"), b"ls");
    p.vt.ecrire(b"\x1b[?2004h");
    assert_eq!(p.octets_de_collage("ls"), b"\x1b[200~ls\x1b[201~");
    p.vt.ecrire(b"\x1b[?2004l");
    assert_eq!(p.octets_de_collage("ls"), b"ls");
}

// ---------------------------------------------------------------- défilement

fn remplir(p: &mut Panneau, n: usize) {
    for i in 0..n {
        p.vt.ecrire(format!("ligne {i}\r\n").as_bytes());
    }
}

#[test]
fn le_defilement_reste_entre_le_bas_et_le_haut_de_l_historique() {
    let mut p = sans_session(20, 5);
    remplir(&mut p, 30);
    let h = p.vt.historique_len();
    assert!(h > 20, "l'historique a été rempli ({h})");
    assert!(!p.defiler(-1), "déjà en bas");
    assert!(p.defiler(3));
    assert_eq!(p.decalage(), 3);
    assert!(p.defiler(1000));
    assert_eq!(p.decalage(), h);
    assert!(!p.defiler(1), "déjà tout en haut");
    assert!(p.defiler_clavier(Defil::Fin));
    assert_eq!(p.decalage(), 0);
    assert!(p.defiler_clavier(Defil::PageHaut));
    assert_eq!(p.decalage(), 4, "une page = lignes − 1");
    assert!(p.defiler_clavier(Defil::Debut));
    assert_eq!(p.decalage(), h);
}

#[test]
fn la_ligne_visible_suit_le_defilement() {
    let mut p = sans_session(20, 5);
    remplir(&mut p, 30);
    let texte = |p: &Panneau, i: usize| -> String { p.ligne_visible(i).map(|l| l.iter().map(|c| c.c).collect::<String>().trim_end().to_string()).unwrap_or_default() };
    // en bas : 30 lignes écrites, curseur sur la 31e (vide) ; l'écran montre les lignes 26 à 29 puis la ligne vide
    assert_eq!(texte(&p, 0), "ligne 26");
    assert_eq!(texte(&p, 3), "ligne 29");
    assert_eq!(texte(&p, 4), "");
    p.defiler(2);
    assert_eq!(texte(&p, 0), "ligne 24");
    assert_eq!(texte(&p, 4), "ligne 28");
    assert_eq!(p.rangee_du_curseur(), None, "le curseur n'est pas dans la vue remontée");
    p.defiler(-2);
    assert_eq!(p.rangee_du_curseur(), Some(4));
}

#[test]
fn la_ligne_absolue_ne_depend_pas_du_defilement_de_la_ligne_affichee() {
    let mut p = sans_session(20, 5);
    remplir(&mut p, 30);
    let bas = p.ligne_absolue(0);
    p.defiler(3);
    assert_eq!(p.ligne_absolue(0), bas - 3);
    assert_eq!(p.ligne_absolue(3), bas);
}

// ---------------------------------------------------------------- sélection

#[test]
fn selection_d_une_partie_de_ligne() {
    let mut p = sans_session(20, 5);
    p.vt.ecrire(b"bonjour le monde");
    assert!(!p.a_selection());
    p.commencer_selection(0, 8);
    assert!(!p.a_selection(), "un simple clic ne sélectionne rien");
    p.etendre_selection(0, 10);
    assert!(p.a_selection());
    assert_eq!(p.texte_selection().as_deref(), Some("le"));
    assert_eq!(p.selection_sur_rangee(0), Some((8, 10)));
    assert_eq!(p.selection_sur_rangee(1), None);
}

#[test]
fn selection_a_l_envers_et_sur_plusieurs_lignes() {
    let mut p = sans_session(20, 5);
    p.vt.ecrire(b"premiere\r\nseconde\r\ntroisieme");
    // du milieu de la 3e ligne au milieu de la 1re, de bas en haut
    p.commencer_selection(2, 4);
    p.etendre_selection(0, 3);
    assert_eq!(p.texte_selection().as_deref(), Some("miere\nseconde\ntroi"), "la colonne de fin est exclue");
    assert_eq!(p.selection_sur_rangee(0), Some((3, 20)));
    assert_eq!(p.selection_sur_rangee(1), Some((0, 20)));
    assert_eq!(p.selection_sur_rangee(2), Some((0, 4)));
}

#[test]
fn la_selection_traverse_l_historique_et_survit_au_defilement() {
    let mut p = sans_session(20, 5);
    remplir(&mut p, 30);
    p.defiler(6);
    p.commencer_selection(0, 0);
    p.etendre_selection(1, 20);
    let attendu = p.texte_selection().unwrap();
    // les coordonnées sont absolues : défiler ne change pas le texte sélectionné
    p.defiler(-6);
    assert_eq!(p.texte_selection().unwrap(), attendu);
    assert_eq!(attendu.lines().count(), 2);
    p.effacer_selection();
    assert!(p.texte_selection().is_none());
}

#[test]
fn la_selection_ignore_les_blancs_de_fin_et_les_moities_de_caracteres_larges() {
    let mut p = sans_session(20, 5);
    p.vt.ecrire("日本 x".as_bytes());
    p.commencer_selection(0, 0);
    p.etendre_selection(0, 20);
    assert_eq!(p.texte_selection().as_deref(), Some("日本 x"));
}

#[test]
fn une_selection_de_blancs_est_vide() {
    let mut p = sans_session(20, 5);
    p.commencer_selection(0, 2);
    p.etendre_selection(0, 9);
    assert!(p.a_selection());
    assert_eq!(p.texte_selection(), None, "rien de visible à copier");
}

#[test]
fn redimensionner_efface_la_selection_et_borne_le_decalage() {
    let mut p = sans_session(20, 5);
    remplir(&mut p, 30);
    p.commencer_selection(0, 0);
    p.etendre_selection(2, 5);
    p.redimensionner(30, 8);
    assert_eq!((p.vt.colonnes(), p.vt.lignes()), (30, 8));
    assert!(!p.a_selection());
    // dimensions plancher
    p.redimensionner(1, 1);
    assert_eq!((p.vt.colonnes(), p.vt.lignes()), (COLONNES_MIN, LIGNES_MIN));
}

// ---------------------------------------------------------------- grille

#[test]
fn les_dimensions_de_la_grille_suivent_la_zone_et_la_cellule() {
    assert_eq!(dimensions_grille(800.0, 200.0, 8.0, 16.0), (100, 12));
    assert_eq!(dimensions_grille(805.0, 207.0, 8.0, 16.0), (100, 12), "on ne compte que les cellules entières");
    assert_eq!(dimensions_grille(20.0, 10.0, 8.0, 16.0), (COLONNES_MIN, LIGNES_MIN), "plancher");
    assert_eq!(dimensions_grille(100_000.0, 100_000.0, 8.0, 16.0), (1_000, 500), "plafonds de l'émulateur");
    assert_eq!(dimensions_grille(800.0, 200.0, 0.0, 16.0), (COLONNES_MIN, LIGNES_MIN), "cellule non mesurée");
}

// ---------------------------------------------------------------- un vrai shell

#[test]
fn un_vrai_shell_repond_a_travers_le_panneau() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::{Duration, Instant};
    let reveils = Arc::new(AtomicUsize::new(0));
    let r = reveils.clone();
    let mut p = Panneau::ouvrir(Shell::Cmd, "C:\\Windows", 80, 24, Arc::new(move || {
        r.fetch_add(1, Ordering::SeqCst);
    }));
    assert!(p.erreur().is_none(), "le shell démarre : {:?}", p.erreur());
    assert!(p.actif());
    p.envoyer(b"echo panneau-ok-42\r");
    let fin = Instant::now() + Duration::from_secs(15);
    let mut vu = false;
    while Instant::now() < fin {
        p.pomper();
        // l'écho de la commande contient aussi le mot : on cherche la ligne de résultat seule
        if (0..p.terminal().lignes()).any(|i| p.terminal().texte_ligne(i) == "panneau-ok-42") {
            vu = true;
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    assert!(vu, "la sortie du shell est dans la grille :\n{}", p.terminal().texte_ecran());
    assert!(reveils.load(Ordering::SeqCst) > 0, "la fenêtre a été prévenue");
    // fin du shell : détectée et signalée, sans nouvelle sortie à attendre
    p.envoyer(b"exit 7\r");
    let fin = Instant::now() + Duration::from_secs(15);
    while Instant::now() < fin && p.fini().is_none() {
        p.pomper();
        std::thread::sleep(Duration::from_millis(50));
    }
    assert_eq!(p.fini(), Some(7));
    assert!(!p.actif());
    // après la fin, taper ne fait plus rien (pas de panique)
    p.envoyer(b"echo x\r");
    // relancer redonne un shell
    p.relancer(Arc::new(|| {}));
    assert!(p.actif());
    assert_eq!(p.fini(), None);
}

#[test]
fn un_shell_qui_ne_demarre_pas_laisse_une_erreur_et_pas_de_panique() {
    let mut p = sans_session(20, 5);
    p.shell = Shell::Cmd;
    p.dossier = "C:\\dossier\\qui\\n'existe\\pas\\du\\tout".to_string();
    p.demarrer(20, 5, Arc::new(|| {}));
    assert!(p.erreur().is_some());
    assert!(!p.actif());
    assert!(!p.pomper());
    p.envoyer(b"x"); // sans session : sans effet
}

// ---------------------------------------------------------------- T3 : programmes plein écran dans un vrai shell

/// Pompe le panneau jusqu'à ce que `cond` soit vraie (ou `delai` écoulé) ; rend la dernière valeur de `cond`
fn attendre(p: &mut Panneau, delai: std::time::Duration, cond: impl Fn(&Panneau) -> bool) -> bool {
    let fin = std::time::Instant::now() + delai;
    while std::time::Instant::now() < fin {
        p.pomper();
        if cond(p) {
            return true;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    p.pomper();
    cond(p)
}

fn shell_cmd() -> Panneau {
    let p = Panneau::ouvrir(Shell::Cmd, "C:\\Windows", 80, 24, Arc::new(|| {}));
    assert!(p.erreur().is_none(), "le shell démarre : {:?}", p.erreur());
    p
}

fn ecran_contient(p: &Panneau, mot: &str) -> bool {
    (0..p.terminal().lignes()).any(|i| p.terminal().texte_ligne(i).contains(mot))
}

#[test]
fn un_programme_prend_l_ecran_alterne_puis_le_rend_avec_la_souris_demandee() {
    use std::time::Duration;
    let mut p = shell_cmd();
    assert!(!p.terminal().ecran_alterne());
    assert!(!p.souris_capturee(), "avant tout programme, la souris reste au panneau");
    // Un programme plein écran de test. ConPTY ne relaie pas un `ESC[?1000h` écrit à la main par le programme : un
    // programme console demande la souris par le mode de la console (ENABLE_MOUSE_INPUT, 0x10, avec ENABLE_EXTENDED_FLAGS
    // 0x80 et sans QUICK_EDIT 0x40), et c'est ConPTY qui envoie alors `ESC[?1003;1006h` au terminal. Le script fait comme
    // vim ou less sous Windows, puis restaure le mode et rend l'écran principal.
    let script = std::env::temp_dir().join(format!("spacefiles_t3_souris_{}.ps1", std::process::id()));
    std::fs::write(
        &script,
        "Add-Type -Namespace W -Name K -MemberDefinition '[DllImport(\"kernel32.dll\")] public static extern IntPtr GetStdHandle(int n); [DllImport(\"kernel32.dll\")] public static extern bool GetConsoleMode(IntPtr h, out uint m); [DllImport(\"kernel32.dll\")] public static extern bool SetConsoleMode(IntPtr h, uint m);'\r\n\
         $h=[W.K]::GetStdHandle(-10); $m=[uint32]0; [void][W.K]::GetConsoleMode($h,[ref]$m)\r\n\
         $n=$m -bor 0x90; if ($n -band 0x40) { $n=$n-0x40 }\r\n\
         [void][W.K]::SetConsoleMode($h,[uint32]$n)\r\n\
         [Console]::Out.Write([char]27+'[?1049h'+'PLEIN-ECRAN-OK')\r\n\
         Start-Sleep 6\r\n\
         [void][W.K]::SetConsoleMode($h,$m)\r\n\
         [Console]::Out.Write([char]27+'[?1049l')\r\n",
    )
    .unwrap();
    p.envoyer(format!("powershell -NoProfile -ExecutionPolicy Bypass -File \"{}\"\r", script.display()).as_bytes());
    assert!(
        attendre(&mut p, Duration::from_secs(30), |p| p.terminal().ecran_alterne() && ecran_contient(p, "PLEIN-ECRAN-OK")),
        "l'écran alterné est pris et montre le texte du programme :\n{}",
        p.terminal().texte_ecran()
    );
    assert!(attendre(&mut p, Duration::from_secs(10), |p| p.souris_capturee()), "le programme a demandé la souris");
    // Le programme suit la souris : la molette lui revient (consommée) au lieu de défiler l'historique
    assert!(p.molette_vers_programme(1, 3, 3, false, false));
    assert!(p.envoyer_souris(Evenement::Appui(Bouton::Gauche), 3, 3, false, false));
    assert!(p.envoyer_souris(Evenement::Relache(Bouton::Gauche), 3, 3, false, false));
    // ConPTY demande le suivi complet (?1003) en codage SGR (?1006)
    assert!(p.modes().souris_mouvement && p.modes().souris_sgr, "ConPTY demande ?1003 et ?1006");
    // Un mouvement dans la même case que le dernier envoi n'est pas renvoyé ; dans une autre case, il l'est
    assert!(!p.envoyer_souris(Evenement::Mouvement(Some(Bouton::Gauche)), 3, 3, false, false));
    assert!(p.envoyer_souris(Evenement::Mouvement(Some(Bouton::Gauche)), 4, 3, false, false));
    assert!(!p.envoyer_souris(Evenement::Mouvement(Some(Bouton::Gauche)), 4, 3, false, false));
    // Survol sans bouton (?1003) : envoyé aussi, une fois par case
    assert!(p.envoyer_souris(Evenement::Mouvement(None), 5, 3, false, false));
    assert!(!p.envoyer_souris(Evenement::Mouvement(None), 5, 3, false, false));
    // Le programme se termine : l'écran principal revient, la souris aussi
    assert!(
        attendre(&mut p, Duration::from_secs(30), |p| !p.terminal().ecran_alterne()),
        "l'écran principal revient :\n{}",
        p.terminal().texte_ecran()
    );
    assert!(attendre(&mut p, Duration::from_secs(10), |p| !p.souris_capturee()), "la souris est rendue au panneau");
    assert!(!p.molette_vers_programme(1, 0, 0, false, false), "sur l'écran principal, la molette défile l'historique");
    let _ = std::fs::remove_file(&script);
}

/// Cherche un programme dans le PATH (sans lancer de shell) ; les tests qui en dépendent sont ignorés s'il manque
fn dans_le_path(nom: &str) -> bool {
    std::env::var_os("PATH").is_some_and(|p| std::env::split_paths(&p).any(|d| d.join(nom).is_file()))
}

#[test]
fn python_interactif_repond_aux_touches_du_clavier() {
    use std::time::Duration;
    if !dans_le_path("py.exe") && !dans_le_path("python.exe") {
        eprintln!("python absent : test ignoré");
        return;
    }
    let lanceur = if dans_le_path("py.exe") { "py" } else { "python" };
    let mut p = shell_cmd();
    p.envoyer(format!("{lanceur}\r").as_bytes());
    assert!(
        attendre(&mut p, Duration::from_secs(30), |p| ecran_contient(p, ">>>")),
        "l'invite de python s'affiche :\n{}",
        p.terminal().texte_ecran()
    );
    // Une ligne saisie au clavier, puis la flèche haut qui la rappelle (édition de ligne du programme)
    p.envoyer(b"print(6*7)\r");
    assert!(attendre(&mut p, Duration::from_secs(15), |p| ecran_contient(p, "42")), "python répond :\n{}", p.terminal().texte_ecran());
    p.envoyer(b"\x1b[A\r");
    assert!(
        attendre(&mut p, Duration::from_secs(15), |p| (0..p.terminal().lignes()).filter(|&i| p.terminal().texte_ligne(i).trim() == "42").count() >= 2),
        "la flèche haut rappelle la ligne précédente :\n{}",
        p.terminal().texte_ecran()
    );
    p.envoyer(b"exit()\r");
}

#[test]
fn winget_affiche_sa_version_dans_le_panneau() {
    use std::time::Duration;
    let Some(local) = std::env::var_os("LOCALAPPDATA") else { return };
    if !std::path::Path::new(&local).join("Microsoft\\WindowsApps\\winget.exe").is_file() {
        eprintln!("winget absent : test ignoré");
        return;
    }
    let mut p = shell_cmd();
    p.envoyer(b"winget --version\r");
    assert!(
        attendre(&mut p, Duration::from_secs(30), |p| (0..p.terminal().lignes()).any(|i| p.terminal().texte_ligne(i).trim_start().starts_with("v1."))),
        "winget donne sa version :\n{}",
        p.terminal().texte_ecran()
    );
}

#[test]
fn more_avance_page_par_page_aux_touches_du_clavier() {
    use std::time::Duration;
    let mut p = shell_cmd();
    p.envoyer(b"(for /l %i in (1,1,200) do @echo ligne%i) | more\r");
    assert!(
        attendre(&mut p, Duration::from_secs(30), |p| ecran_contient(p, "ligne1")),
        "la première page de more s'affiche :\n{}",
        p.terminal().texte_ecran()
    );
    // more s'arrête à chaque page : la dernière ligne n'est pas là tant qu'on n'a pas appuyé sur Espace
    assert!(!ecran_contient(&p, "ligne200"), "more attend une touche :\n{}", p.terminal().texte_ecran());
    let fin = std::time::Instant::now() + Duration::from_secs(40);
    while std::time::Instant::now() < fin && !ecran_contient(&p, "ligne200") {
        p.envoyer(b" ");
        attendre(&mut p, Duration::from_millis(300), |p| ecran_contient(p, "ligne200"));
    }
    assert!(ecran_contient(&p, "ligne200"), "Espace fait avancer more jusqu'au bout :\n{}", p.terminal().texte_ecran());
}

// ---------------------------------------------------------------- T4b : dossier synchronisé liste ↔ terminal

#[test]
fn deux_chemins_designent_le_meme_dossier_sans_regarder_la_casse_ni_la_barre_finale() {
    assert!(meme_dossier("C:\\Users\\Nico", "c:\\users\\nico\\"));
    assert!(meme_dossier("C:\\", "C:"));
    assert!(!meme_dossier("C:\\Users", "C:\\Users\\Nico"));
}

#[test]
fn le_cd_de_powershell_est_un_chemin_litteral_dont_les_apostrophes_sont_doublees() {
    let c = commande_cd(Shell::PowerShell, "C:\\Tom's [dossier]").unwrap();
    assert_eq!(String::from_utf8(c).unwrap(), "cd -LiteralPath 'C:\\Tom''s [dossier]'\r");
    // les apostrophes typographiques sont aussi des apostrophes pour PowerShell
    let c = String::from_utf8(commande_cd(Shell::PowerShell, "C:\\l\u{2019}été").unwrap()).unwrap();
    assert_eq!(c, "cd -LiteralPath 'C:\\l\u{2019}\u{2019}été'\r");
}

#[test]
fn le_cd_refuse_un_chemin_vide_ou_avec_un_caractere_de_controle() {
    assert!(commande_cd(Shell::PowerShell, "").is_none());
    assert!(commande_cd(Shell::PowerShell, "C:\\a\r\nformat c:").is_none(), "une fin de ligne lancerait autre chose");
    assert!(commande_cd(Shell::PowerShell, "C:\\a\u{1b}[31m").is_none());
    assert!(commande_cd(Shell::Cmd, "C:\\Windows").is_none(), "cmd n'annonce pas son dossier : jamais piloté");
}

#[test]
fn un_dossier_annonce_par_le_shell_n_est_rendu_qu_une_fois() {
    let mut p = sans_session(40, 5);
    assert_eq!(p.dossier_change_par_le_shell(), None, "rien annoncé : rien à suivre");
    p.vt.ecrire(b"\x1b]9;9;C:\\essai\x1b\\");
    assert_eq!(p.dossier_change_par_le_shell(), None, "le même dossier que celui du lancement n'est pas un changement");
    p.vt.ecrire(b"\x1b]9;9;C:\\autre\x1b\\");
    assert_eq!(p.dossier_change_par_le_shell().as_deref(), Some("C:\\autre"));
    assert_eq!(p.dossier_change_par_le_shell(), None, "déjà rendu");
    assert_eq!(p.dossier_connu(), "C:\\autre");
    p.vt.ecrire(b"\x1b]9;9;c:\\AUTRE\\\x1b\\");
    assert_eq!(p.dossier_change_par_le_shell(), None, "casse et barre finale ne changent rien");
}

#[test]
fn sans_session_on_ne_tape_rien_au_shell() {
    let mut p = sans_session(40, 5);
    p.invite_prete = true;
    assert!(!p.peut_changer_de_dossier(), "pas de session vivante");
    assert!(!p.aller_dans("C:\\Windows"));
    assert_eq!(p.dossier_connu(), "C:\\essai", "rien n'a bougé");
}

/// PowerShell est ouvert avec l'invite enveloppée : il annonce son dossier, accepte un `cd` de la liste, et un `cd` tapé à
/// la main est vu comme un changement du shell
#[test]
fn powershell_annonce_son_dossier_obeit_au_cd_de_la_liste_et_signale_un_cd_tape() {
    use std::time::Duration;
    let _seul = crate::terminal::session::VERROU_HANDLES.lock().unwrap_or_else(|e| e.into_inner());
    let mut p = Panneau::ouvrir(Shell::PowerShell, "C:\\Windows", 100, 24, Arc::new(|| {}));
    assert!(p.erreur().is_none(), "le shell démarre : {:?}", p.erreur());
    assert!(
        attendre(&mut p, Duration::from_secs(40), |p| p.peut_changer_de_dossier()),
        "la première invite annonce le dossier :\n{}",
        p.terminal().texte_ecran()
    );
    assert_eq!(p.dossier_change_par_le_shell(), None, "il est bien là où on l'a lancé");
    // Une ligne commencée n'est jamais écrasée : le cd attend la prochaine invite
    p.envoyer(b"x");
    assert!(!p.peut_changer_de_dossier(), "l'utilisateur a commencé à taper");
    assert!(!p.aller_dans("C:\\Windows\\System32"));
    p.envoyer(b"\x08\r");
    assert!(!p.peut_changer_de_dossier(), "Entrée : la commande tourne, l'invite n'est pas revenue");
    assert!(
        attendre(&mut p, Duration::from_secs(20), |p| p.peut_changer_de_dossier()),
        "l'invite revient :\n{}",
        p.terminal().texte_ecran()
    );
    // liste → shell
    assert!(p.aller_dans("C:\\Windows\\System32"));
    assert!(!p.peut_changer_de_dossier(), "la commande tourne : on n'en envoie pas une seconde");
    assert!(
        attendre(&mut p, Duration::from_secs(20), |p| p.peut_changer_de_dossier()),
        "l'invite revient :\n{}",
        p.terminal().texte_ecran()
    );
    assert!(meme_dossier(p.terminal().dossier().unwrap_or(""), "C:\\Windows\\System32"), "annoncé : {:?}", p.terminal().dossier());
    assert_eq!(p.dossier_change_par_le_shell(), None, "c'est le cd de la liste, pas un changement du shell");
    // shell → liste : un cd tapé
    p.envoyer(b"cd ..\r");
    assert!(
        attendre(&mut p, Duration::from_secs(20), |p| p.peut_changer_de_dossier()),
        "l'invite revient :\n{}",
        p.terminal().texte_ecran()
    );
    assert_eq!(p.dossier_change_par_le_shell().map(|d| d.to_ascii_lowercase()).as_deref(), Some("c:\\windows"));
}


#[test]
fn powershell_reel_la_ligne_de_saisie_prend_la_frappe_et_propose_la_suggestion() {
    use crate::terminal::historique::Historique;
    use std::time::Duration;
    let _seul = crate::terminal::session::VERROU_HANDLES.lock().unwrap_or_else(|e| e.into_inner());
    let mut p = Panneau::ouvrir(Shell::PowerShell, r"C:\Windows", 100, 24, Arc::new(|| {}));
    assert!(p.erreur().is_none(), "le shell démarre : {:?}", p.erreur());
    assert!(
        attendre(&mut p, Duration::from_secs(40), |p| p.saisie_prete()),
        "la première invite rend la ligne prête :\n{}",
        p.terminal().texte_ecran()
    );
    let mut h = Historique::default();
    h.ajouter("echo bonjour-suggestion", r"C:\Windows", 1);
    for c in "echo bon".encode_utf16() {
        assert!(p.caractere_ligne(c, &crate::terminal::historique::Historique::default()).is_empty(), "la ligne prend le caractère");
    }
    let vue = p.vue_ligne(&h).expect("une vue de la ligne");
    assert_eq!(vue.visibles.iter().collect::<String>(), "echo bon");
    assert_eq!(vue.suggestion.iter().collect::<String>(), "jour-suggestion");
    // Entrée : la commande part, rien n'est affiché en double, l'invite revient et la ligne est de nouveau prête
    assert_eq!(p.touche_ligne(0x0D, nu(), &h), Some(Some("echo bon".to_string())));
    assert!(
        attendre(&mut p, Duration::from_secs(20), |p| p.saisie_prete()),
        "l'invite revient :\n{}",
        p.terminal().texte_ecran()
    );
    assert!(p.terminal().texte_ecran().contains("bon"), "la commande a été exécutée :\n{}", p.terminal().texte_ecran());
}

/// Tape `texte` dans la ligne de saisie du panneau, puis Entrée, et attend le retour de l'invite
fn taper_et_valider(p: &mut Panneau, h: &crate::terminal::historique::Historique, texte: &str) {
    for c in texte.encode_utf16() {
        assert!(p.caractere_ligne(c, &crate::terminal::historique::Historique::default()).is_empty(), "la ligne prend le caractère");
    }
    assert_eq!(p.touche_ligne(0x0D, nu(), h), Some(Some(texte.to_string())), "Entrée rend la frappe pour l'historique");
    assert!(
        attendre(p, std::time::Duration::from_secs(20), |p| p.saisie_prete()),
        "l'invite revient après « {texte} » :\n{}",
        p.terminal().texte_ecran()
    );
}

/// T6 avec le vrai PowerShell : du cmd tapé est exécuté, `set` garde la variable dans la session, le badge suit la frappe
#[test]
fn powershell_reel_du_cmd_tape_s_execute_et_le_badge_suit() {
    use crate::terminal::historique::Historique;
    use std::time::Duration;
    let _seul = crate::terminal::session::VERROU_HANDLES.lock().unwrap_or_else(|e| e.into_inner());
    let mut p = Panneau::ouvrir(Shell::PowerShell, r"C:\Windows", 110, 40, Arc::new(|| {}));
    assert!(p.erreur().is_none(), "le shell démarre : {:?}", p.erreur());
    assert!(attendre(&mut p, Duration::from_secs(40), |p| p.saisie_prete()), "première invite :\n{}", p.terminal().texte_ecran());
    let h = Historique::default();

    // Le badge suit ce qui est tapé : PS pour du PowerShell, CMD dès que la ligne est du cmd
    for c in "Get-Date".encode_utf16() {
        p.caractere_ligne(c, &crate::terminal::historique::Historique::default());
    }
    assert_eq!(p.vue_ligne(&h).expect("vue").badge(), "PS");
    assert_eq!(p.touche_ligne(0x0D, nu(), &h), Some(Some("Get-Date".to_string())));
    assert!(attendre(&mut p, Duration::from_secs(20), |p| p.saisie_prete()), "invite :\n{}", p.terminal().texte_ecran());
    for c in "ver".encode_utf16() {
        p.caractere_ligne(c, &crate::terminal::historique::Historique::default());
    }
    assert_eq!(p.vue_ligne(&h).expect("vue").badge(), "CMD");
    assert_eq!(p.touche_ligne(0x0D, nu(), &h), Some(Some("ver".to_string())), "l'historique garde ce qui a été tapé");
    assert!(attendre(&mut p, Duration::from_secs(20), |p| p.saisie_prete()), "invite :\n{}", p.terminal().texte_ecran());
    // `ver` n'existe pas dans PowerShell : la réponse prouve que la ligne est passée par cmd
    // (« [Version » en anglais, « [version » en français : on ne teste que le début)
    assert!(p.terminal().texte_ecran().contains("Microsoft Windows ["), "ver répond :\n{}", p.terminal().texte_ecran());

    // `set` devient une variable de la session : PowerShell la relit ensuite
    taper_et_valider(&mut p, &h, "set ESSAI_T6=sept");
    taper_et_valider(&mut p, &h, "\"[\" + $env:ESSAI_T6.ToUpper() + \"]\"");
    assert!(p.terminal().texte_ecran().contains("[SEPT]"), "la variable est restée :\n{}", p.terminal().texte_ecran());

    // `cd /d` change le dossier de la session, et `cmd:` force cmd pour une ligne que PowerShell comprendrait
    taper_et_valider(&mut p, &h, r"cd /d C:\Windows\System32");
    taper_et_valider(&mut p, &h, "cmd: echo %CD%-fin");
    assert!(p.terminal().texte_ecran().contains(r"C:\Windows\System32-fin"), "cmd a vu le nouveau dossier :\n{}", p.terminal().texte_ecran());
    taper_et_valider(&mut p, &h, "ps: Write-Output pas-cmd");
    assert!(p.terminal().texte_ecran().contains("pas-cmd"));
}

/// T7 avec le vrai PowerShell : Tab complète un nom de fichier du dossier annoncé, le menu cycle, et la ligne complétée
/// s'exécute vraiment (le contenu du fichier choisi s'affiche)
#[test]
fn powershell_reel_tab_complete_un_fichier_et_la_ligne_s_execute() {
    use crate::terminal::historique::Historique;
    use std::time::Duration;
    let _seul = crate::terminal::session::VERROU_HANDLES.lock().unwrap_or_else(|e| e.into_inner());
    let dossier = std::env::temp_dir().join(format!("spacefiles_t7_reel_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dossier);
    std::fs::create_dir_all(&dossier).unwrap();
    std::fs::write(dossier.join("alpha1.txt"), "contenu-t7-un").unwrap();
    std::fs::write(dossier.join("alpha2.txt"), "contenu-t7-deux").unwrap();
    let mut p = Panneau::ouvrir(Shell::PowerShell, &dossier.to_string_lossy(), 110, 30, Arc::new(|| {}));
    assert!(p.erreur().is_none(), "le shell démarre : {:?}", p.erreur());
    assert!(attendre(&mut p, Duration::from_secs(40), |p| p.peut_changer_de_dossier()), "première invite :\n{}", p.terminal().texte_ecran());
    let h = Historique::default();
    for c in "Get-Content al".encode_utf16() {
        assert!(p.caractere_ligne(c, &crate::terminal::historique::Historique::default()).is_empty());
    }
    // Premier Tab : début commun « alpha » et menu ; deuxième Tab : premier candidat ; troisième : second
    assert_eq!(p.touche_ligne(0x09, nu(), &h), Some(None));
    let vue = p.vue_ligne(&h).expect("vue");
    assert_eq!(vue.visibles.iter().collect::<String>(), "Get-Content alpha");
    assert_eq!(vue.menu.as_ref().map(|m| m.total), Some(2), "menu de deux fichiers");
    assert_eq!(p.touche_ligne(0x09, nu(), &h), Some(None));
    assert_eq!(p.touche_ligne(0x09, nu(), &h), Some(None));
    let vue = p.vue_ligne(&h).expect("vue");
    assert_eq!(vue.visibles.iter().collect::<String>(), "Get-Content alpha2.txt");
    assert_eq!(p.touche_ligne(0x0D, nu(), &h), Some(Some("Get-Content alpha2.txt".to_string())));
    assert!(attendre(&mut p, Duration::from_secs(20), |p| p.saisie_prete()), "invite :\n{}", p.terminal().texte_ecran());
    assert!(p.terminal().texte_ecran().contains("contenu-t7-deux"), "le fichier complété a été lu :\n{}", p.terminal().texte_ecran());
    drop(p);
    let _ = std::fs::remove_dir_all(&dossier);
}
