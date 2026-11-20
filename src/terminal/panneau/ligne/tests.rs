//! Tests de la ligne de saisie : l'éditeur et le rappel se testent sans fenêtre ni shell (`EtatLigne` est pur).

use super::*;

const D: &str = "C:\\projet";

fn nu() -> Mods {
    Mods::default()
}

fn ctrl() -> Mods {
    Mods { ctrl: true, ..nu() }
}

fn hist(lignes: &[&str]) -> Historique {
    let mut h = Historique::default();
    for (i, l) in lignes.iter().enumerate() {
        h.ajouter(l, D, i as u64);
    }
    h
}

fn taper(e: &mut EtatLigne, texte: &str) {
    for c in texte.chars() {
        assert_eq!(e.caractere(c), Effet::Pris);
    }
}

fn touche(e: &mut EtatLigne, vk: u32, m: Mods, h: &Historique) -> Effet {
    e.touche(vk, m, h, D, true)
}

#[test]
fn taper_puis_entree_rend_la_ligne_et_la_vide() {
    let mut e = EtatLigne::default();
    let h = Historique::default();
    taper(&mut e, "dir /w");
    assert_eq!(touche(&mut e, VK_RETURN, nu(), &h), Effet::Soumettre("dir /w".into()));
    assert!(e.vide());
}

#[test]
fn entree_sur_ligne_vide_soumet_une_ligne_vide() {
    let mut e = EtatLigne::default();
    assert_eq!(touche(&mut e, VK_RETURN, nu(), &Historique::default()), Effet::Soumettre(String::new()));
}

#[test]
fn le_curseur_et_les_suppressions_editent_la_ligne() {
    let mut e = EtatLigne::default();
    let h = Historique::default();
    taper(&mut e, "abcd");
    touche(&mut e, VK_LEFT, nu(), &h);
    touche(&mut e, VK_BACK, nu(), &h);
    assert_eq!(e.saisie().texte(), "abd");
    touche(&mut e, VK_DELETE, nu(), &h);
    assert_eq!(e.saisie().texte(), "ab");
    touche(&mut e, VK_HOME, nu(), &h);
    taper(&mut e, "X");
    assert_eq!(e.saisie().texte(), "Xab");
    touche(&mut e, VK_END, nu(), &h);
    taper(&mut e, "!");
    assert_eq!(e.saisie().texte(), "Xab!");
}

#[test]
fn les_gestes_de_mots_et_de_ligne() {
    let mut e = EtatLigne::default();
    let h = Historique::default();
    taper(&mut e, "git commit -m");
    touche(&mut e, VK_BACK, ctrl(), &h);
    assert_eq!(e.saisie().texte(), "git commit -");
    touche(&mut e, VK_W, ctrl(), &h);
    assert_eq!(e.saisie().texte(), "git commit ", "le tiret est un mot à part");
    touche(&mut e, VK_W, ctrl(), &h);
    assert_eq!(e.saisie().texte(), "git ");
    touche(&mut e, VK_U, ctrl(), &h);
    assert!(e.vide());
    taper(&mut e, "ab cd");
    touche(&mut e, VK_A, ctrl(), &h);
    touche(&mut e, VK_RIGHT, ctrl(), &h);
    taper(&mut e, "!");
    assert_eq!(e.saisie().texte(), "ab! cd");
}

#[test]
fn la_suggestion_grise_vient_de_l_historique_et_droite_l_accepte() {
    let h = hist(&["cargo build --release"]);
    let mut e = EtatLigne::default();
    taper(&mut e, "cargo b");
    assert_eq!(e.suggestion(&h, D).as_deref(), Some("uild --release"));
    assert_eq!(touche(&mut e, VK_RIGHT, nu(), &h), Effet::Pris);
    assert_eq!(e.saisie().texte(), "cargo build --release");
    assert_eq!(e.suggestion(&h, D), None);
}

#[test]
fn fin_accepte_la_suggestion_et_ctrl_e_aussi() {
    let h = hist(&["npm run dev"]);
    let mut e = EtatLigne::default();
    taper(&mut e, "npm");
    touche(&mut e, VK_END, nu(), &h);
    assert_eq!(e.saisie().texte(), "npm run dev");
    let mut e = EtatLigne::default();
    taper(&mut e, "npm");
    touche(&mut e, VK_E, ctrl(), &h);
    assert_eq!(e.saisie().texte(), "npm run dev");
}

#[test]
fn pas_de_suggestion_curseur_au_milieu_ni_ligne_vide() {
    let h = hist(&["cargo test"]);
    let mut e = EtatLigne::default();
    assert_eq!(e.suggestion(&h, D), None);
    taper(&mut e, "cargo");
    touche(&mut e, VK_LEFT, nu(), &h);
    assert_eq!(e.suggestion(&h, D), None);
    // Droite avec le curseur au milieu avance d'une case : rien n'est accepté
    touche(&mut e, VK_RIGHT, nu(), &h);
    assert_eq!(e.saisie().texte(), "cargo");
}

#[test]
fn la_suggestion_prefere_le_dossier_courant() {
    let mut h = Historique::default();
    h.ajouter("make all", "C:\\autre", 1);
    h.ajouter("make test", D, 2);
    h.ajouter("make docs", "C:\\autre", 3);
    let mut e = EtatLigne::default();
    taper(&mut e, "make ");
    assert_eq!(e.suggestion(&h, D).as_deref(), Some("test"));
}

#[test]
fn haut_rappelle_les_commandes_du_plus_recent_au_plus_ancien_et_bas_revient_au_brouillon() {
    let h = hist(&["ls", "cd src", "cargo test"]);
    let mut e = EtatLigne::default();
    taper(&mut e, "brouillon");
    // Avec un filtre « brouillon », rien ne correspond : la ligne ne bouge pas
    touche(&mut e, VK_UP, nu(), &h);
    assert_eq!(e.saisie().texte(), "brouillon");

    let mut e = EtatLigne::default();
    touche(&mut e, VK_UP, nu(), &h);
    assert_eq!(e.saisie().texte(), "cargo test");
    touche(&mut e, VK_UP, nu(), &h);
    assert_eq!(e.saisie().texte(), "cd src");
    touche(&mut e, VK_UP, nu(), &h);
    assert_eq!(e.saisie().texte(), "ls");
    // Au bout de l'historique, ↑ ne change plus rien
    touche(&mut e, VK_UP, nu(), &h);
    assert_eq!(e.saisie().texte(), "ls");
    touche(&mut e, VK_DOWN, nu(), &h);
    assert_eq!(e.saisie().texte(), "cd src");
    touche(&mut e, VK_DOWN, nu(), &h);
    touche(&mut e, VK_DOWN, nu(), &h);
    assert!(e.vide(), "↓ au bout rend le brouillon (ici vide)");
}

#[test]
fn haut_filtre_sur_ce_qui_est_tape_et_bas_rend_le_brouillon() {
    let h = hist(&["cargo build", "ls", "cargo test"]);
    let mut e = EtatLigne::default();
    taper(&mut e, "cargo");
    touche(&mut e, VK_UP, nu(), &h);
    assert_eq!(e.saisie().texte(), "cargo test");
    touche(&mut e, VK_UP, nu(), &h);
    assert_eq!(e.saisie().texte(), "cargo build");
    touche(&mut e, VK_DOWN, nu(), &h);
    touche(&mut e, VK_DOWN, nu(), &h);
    assert_eq!(e.saisie().texte(), "cargo");
}

#[test]
fn pas_de_suggestion_pendant_un_rappel_et_taper_quitte_le_rappel() {
    let h = hist(&["cargo test --release"]);
    let mut e = EtatLigne::default();
    touche(&mut e, VK_UP, nu(), &h);
    assert_eq!(e.suggestion(&h, D), None);
    // Une frappe fait de la ligne rappelée une ligne ordinaire
    taper(&mut e, " -q");
    assert_eq!(e.saisie().texte(), "cargo test --release -q");
    // ↓ n'a plus de rappel à suivre
    touche(&mut e, VK_DOWN, nu(), &h);
    assert_eq!(e.saisie().texte(), "cargo test --release -q");
}

#[test]
fn le_premier_haut_saute_la_ligne_identique_a_celle_tapee() {
    let h = hist(&["cargo test", "cargo build"]);
    let mut e = EtatLigne::default();
    taper(&mut e, "cargo build");
    touche(&mut e, VK_UP, nu(), &h);
    assert_eq!(e.saisie().texte(), "cargo build", "pas de commande plus ancienne qui commence ainsi : la ligne reste");
}

#[test]
fn echap_vide_la_ligne() {
    let mut e = EtatLigne::default();
    let h = Historique::default();
    taper(&mut e, "abc");
    assert_eq!(touche(&mut e, VK_ESCAPE, nu(), &h), Effet::Pris);
    assert!(e.vide());
}

#[test]
fn ctrl_c_abandonne_une_ligne_commencee_sinon_interrompt_le_shell() {
    let mut e = EtatLigne::default();
    let h = Historique::default();
    taper(&mut e, "rm -rf");
    assert_eq!(touche(&mut e, VK_C, ctrl(), &h), Effet::Pris);
    assert!(e.vide());
    // Ligne vide : la touche est laissée au chemin habituel, et son caractère 0x03 aussi
    assert_eq!(touche(&mut e, VK_C, ctrl(), &h), Effet::Passer);
    assert_eq!(e.caractere('\u{3}'), Effet::Passer);
}

#[test]
fn les_touches_inconnues_deleguent_la_ligne_au_shell() {
    let mut e = EtatLigne::default();
    let h = Historique::default();
    taper(&mut e, "cd Do");
    // Ctrl+Tab : pas une complétion de SpaceFiles, la ligne appartient au shell (Tab seul est pris, voir T7 plus bas)
    assert_eq!(e.touche(0x09, ctrl(), &h, D, true), Effet::Deleguer);
    // Une touche que même le chemin habituel ne sait pas traduire : rien à déléguer
    assert_eq!(e.touche(0x41, nu(), &h, D, false), Effet::Passer);
    // Ctrl+lettre (WM_CHAR de contrôle) : la ligne part avant le contrôle
    assert_eq!(e.caractere('\u{12}'), Effet::Deleguer);
}

#[test]
fn alt_ne_touche_pas_a_la_ligne_et_altgr_laisse_passer_ses_caracteres() {
    let mut e = EtatLigne::default();
    let h = Historique::default();
    taper(&mut e, "a");
    // Alt+Retour arrière : le shell veut la touche (traduite), la ligne part vers lui
    let alt = Mods { alt: true, ..nu() };
    assert_eq!(e.touche(VK_BACK, alt, &h, D, true), Effet::Deleguer);
    // AltGr + touche (Ctrl+Alt) sans traduction : rien ne part, le caractère arrive ensuite par WM_CHAR
    let altgr = Mods { ctrl: true, alt: true, maj: false };
    let mut e = EtatLigne::default();
    taper(&mut e, "a");
    assert_eq!(e.touche(0x30, altgr, &h, D, false), Effet::Passer);
    assert_eq!(e.caractere('@'), Effet::Pris);
    assert_eq!(e.saisie().texte(), "a@");
}

#[test]
fn le_collage_s_insere_au_curseur_et_trop_long_se_delegue() {
    let mut e = EtatLigne::default();
    taper(&mut e, "ac");
    e.saisie.gauche();
    assert_eq!(e.coller("b"), Effet::Pris);
    assert_eq!(e.saisie().texte(), "abc");
    let enorme = "x".repeat(LONGUEUR_MAX);
    assert_eq!(e.coller(&enorme), Effet::Deleguer);
    assert_eq!(e.saisie().texte(), "abc", "rien n'est tronqué en silence");
}

#[test]
fn les_caracteres_imprimables_s_inserent_y_compris_les_accents() {
    let mut e = EtatLigne::default();
    taper(&mut e, "é€ 😀");
    assert_eq!(e.saisie().texte(), "é€ 😀");
}

// ---------------------------------------------------------------- vue

fn s(t: &str) -> Saisie {
    let mut x = Saisie::default();
    x.inserer(t);
    x
}

#[test]
fn la_vue_d_une_ligne_vide_est_absente() {
    assert_eq!(composer_vue(&Saisie::default(), Some("x"), 5, 80), None);
}

#[test]
fn la_vue_place_la_ligne_apres_l_invite_et_la_suggestion_apres_la_ligne() {
    let v = composer_vue(&s("ca"), Some("rgo"), 5, 80).unwrap();
    assert_eq!(v.col, 5);
    assert_eq!(v.visibles, vec!['c', 'a']);
    assert_eq!(v.curseur, 2);
    assert_eq!(v.suggestion, vec!['r', 'g', 'o']);
    assert_eq!(v.sous_curseur(), Some('r'));
}

#[test]
fn la_suggestion_est_tronquee_a_la_place_qui_reste() {
    // Rangée de 10 cases, invite sur 4, ligne de 2 : 4 cases restent
    let v = composer_vue(&s("ab"), Some("cdefghij"), 4, 10).unwrap();
    assert_eq!(v.suggestion, vec!['c', 'd', 'e', 'f']);
}

#[test]
fn la_vue_garde_le_curseur_visible_dans_une_rangee_trop_etroite() {
    let longue = "0123456789".repeat(3);
    let v = composer_vue(&s(&longue), None, 2, 12).unwrap();
    assert!(v.visibles.len() <= 10);
    assert!(v.curseur <= v.visibles.len());
    assert_eq!(v.visibles.last(), Some(&'9'));
    assert_eq!(v.sous_curseur(), None, "au bout de la ligne, sans suggestion : rien sous le curseur");
}

#[test]
fn pas_de_vue_sans_place() {
    assert_eq!(composer_vue(&s("a"), None, 80, 80), None);
}

#[test]
fn la_suggestion_n_est_pas_posee_quand_le_curseur_est_au_milieu() {
    let mut x = s("abc");
    x.gauche();
    let v = composer_vue(&x, Some("zzz"), 0, 40).unwrap();
    assert!(v.suggestion.is_empty());
    assert_eq!(v.sous_curseur(), Some('c'));
}

#[test]
fn les_fins_de_ligne_collees_s_affichent_en_fleche() {
    let v = composer_vue(&s("a\nb"), None, 0, 40).unwrap();
    assert_eq!(v.visibles, vec!['a', '↵', 'b']);
}

#[test]
fn haut_sans_rien_a_rappeler_laisse_la_main_au_shell() {
    // Historique vide : ↑ n'est pas pris, sinon la touche disparaîtrait au lieu d'atteindre l'historique de PSReadLine
    let mut e = EtatLigne::default();
    assert_eq!(touche(&mut e, VK_UP, nu(), &Historique::default()), Effet::Deleguer);
    // Un filtre sans correspondance (« brouillon ») : même chose, la ligne n'a pas bougé
    let h = hist(&["ls"]);
    taper(&mut e, "brouillon");
    assert_eq!(touche(&mut e, VK_UP, nu(), &h), Effet::Deleguer);
    assert_eq!(e.saisie().texte(), "brouillon");
}

#[test]
fn un_caractere_au_dela_de_la_longueur_maximale_est_delegue_et_non_perdu() {
    let mut e = EtatLigne::default();
    for _ in 0..crate::terminal::saisie::LONGUEUR_MAX {
        assert_eq!(e.caractere('a'), Effet::Pris);
    }
    assert_eq!(e.caractere('b'), Effet::Deleguer);
}

// ───────────────────────── T7 : complétion par Tab ─────────────────────────

/// Un dossier temporaire garni de fichiers, supprimé à la fin du test
struct Dossier(std::path::PathBuf);

impl Dossier {
    fn avec(nom: &str, fichiers: &[&str]) -> Dossier {
        let chemin = std::env::temp_dir().join(format!("spacefiles_t7_ligne_{nom}_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&chemin);
        std::fs::create_dir_all(&chemin).unwrap();
        for f in fichiers {
            std::fs::write(chemin.join(f), b"x").unwrap();
        }
        Dossier(chemin)
    }

    fn texte(&self) -> String {
        self.0.to_string_lossy().into_owned()
    }
}

impl Drop for Dossier {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

const TAB: u32 = 0x09;

fn tab(e: &mut EtatLigne, d: &Dossier, m: Mods) -> Effet {
    e.touche(TAB, m, &Historique::default(), &d.texte(), true)
}

fn maj() -> Mods {
    Mods { maj: true, ..nu() }
}

#[test]
fn tab_complete_un_candidat_unique_sans_menu() {
    let d = Dossier::avec("unique", &["alpha1.txt", "alpha2.txt", "zeta.txt"]);
    let mut e = EtatLigne::default();
    taper(&mut e, "type ze");
    assert_eq!(tab(&mut e, &d, nu()), Effet::Pris);
    assert_eq!(e.saisie().texte(), "type zeta.txt");
    assert!(e.vue_menu().is_none());
}

#[test]
fn tab_ajoute_le_debut_commun_puis_cycle_dans_le_menu() {
    let d = Dossier::avec("cycle", &["alpha1.txt", "alpha2.txt", "zeta.txt"]);
    let mut e = EtatLigne::default();
    taper(&mut e, "type al");
    // Premier Tab : le début commun, le menu s'ouvre sans rien choisir
    assert_eq!(tab(&mut e, &d, nu()), Effet::Pris);
    assert_eq!(e.saisie().texte(), "type alpha");
    let menu = e.vue_menu().expect("menu ouvert");
    assert_eq!((menu.total, menu.selection, menu.lignes.len()), (2, None, 2));
    // Tabs suivants : un candidat après l'autre, et on reboucle
    tab(&mut e, &d, nu());
    assert_eq!(e.saisie().texte(), "type alpha1.txt");
    assert_eq!(e.vue_menu().unwrap().selection, Some(0));
    tab(&mut e, &d, nu());
    assert_eq!(e.saisie().texte(), "type alpha2.txt");
    tab(&mut e, &d, nu());
    assert_eq!(e.saisie().texte(), "type alpha1.txt");
    // Maj+Tab recule
    tab(&mut e, &d, maj());
    assert_eq!(e.saisie().texte(), "type alpha2.txt");
}

#[test]
fn les_fleches_parcourent_le_menu_et_echap_retablit_la_ligne() {
    let d = Dossier::avec("fleches", &["alpha1.txt", "alpha2.txt"]);
    let mut e = EtatLigne::default();
    let h = Historique::default();
    taper(&mut e, "type al");
    tab(&mut e, &d, nu());
    assert_eq!(e.touche(VK_DOWN, nu(), &h, &d.texte(), true), Effet::Pris);
    assert_eq!(e.saisie().texte(), "type alpha1.txt");
    assert_eq!(e.touche(VK_DOWN, nu(), &h, &d.texte(), true), Effet::Pris);
    assert_eq!(e.saisie().texte(), "type alpha2.txt");
    assert_eq!(e.touche(VK_UP, nu(), &h, &d.texte(), true), Effet::Pris);
    assert_eq!(e.saisie().texte(), "type alpha1.txt");
    // Échap : la ligne d'avant le premier Tab, le menu fermé, la ligne pas vidée
    assert_eq!(e.touche(VK_ESCAPE, nu(), &h, &d.texte(), true), Effet::Pris);
    assert_eq!(e.saisie().texte(), "type al");
    assert!(e.vue_menu().is_none());
}

#[test]
fn taper_ferme_le_menu_et_garde_le_candidat_mais_maj_seul_ne_le_ferme_pas() {
    let d = Dossier::avec("fermeture", &["alpha1.txt", "alpha2.txt"]);
    let mut e = EtatLigne::default();
    let h = Historique::default();
    taper(&mut e, "type al");
    tab(&mut e, &d, nu());
    tab(&mut e, &d, nu());
    // Presser Maj (début de Maj+Tab) laisse le menu
    e.touche(0x10, nu(), &h, &d.texte(), false);
    assert!(e.vue_menu().is_some());
    // Un caractère : le menu se ferme, le candidat choisi reste dans la ligne
    e.caractere(' ');
    assert!(e.vue_menu().is_none());
    assert_eq!(e.saisie().texte(), "type alpha1.txt ");
    // Une touche d'édition ferme aussi le menu
    let mut e = EtatLigne::default();
    taper(&mut e, "type al");
    tab(&mut e, &d, nu());
    e.touche(VK_BACK, nu(), &h, &d.texte(), true);
    // Le menu de Tab est fermé (rien n'est choisi) ; la liste automatique peut en rouvrir une pour le mot raccourci
    assert_eq!(e.saisie().texte(), "type alph");
    assert!(e.vue_menu().map_or(true, |m| m.selection.is_none()));
}

#[test]
fn tab_est_refuse_sur_ligne_vide_curseur_au_milieu_ou_sans_candidat() {
    let d = Dossier::avec("refus", &["alpha1.txt"]);
    let h = Historique::default();
    let mut e = EtatLigne::default();
    // Ligne vide : prise, sans effet
    assert_eq!(tab(&mut e, &d, nu()), Effet::Pris);
    assert!(e.vide());
    // Curseur au milieu : rien ne change
    taper(&mut e, "type al");
    e.touche(VK_LEFT, nu(), &h, &d.texte(), true);
    assert_eq!(tab(&mut e, &d, nu()), Effet::Pris);
    assert_eq!(e.saisie().texte(), "type al");
    assert!(e.vue_menu().is_none());
    // Aucun candidat : la ligne reste
    let mut e = EtatLigne::default();
    taper(&mut e, "type qqq");
    tab(&mut e, &d, nu());
    assert_eq!(e.saisie().texte(), "type qqq");
    assert!(e.vue_menu().is_none());
}

#[test]
fn la_suggestion_grise_se_tait_pendant_le_menu() {
    let d = Dossier::avec("suggestion", &["alpha1.txt", "alpha2.txt"]);
    let h = hist(&["type alpha1.txt et la suite"]);
    let mut e = EtatLigne::default();
    taper(&mut e, "type al");
    assert!(e.suggestion(&h, D).is_some());
    tab(&mut e, &d, nu());
    assert!(e.suggestion(&h, D).is_none());
}

#[test]
fn un_menu_long_defile_par_fenetres_de_huit_rangees() {
    let noms: Vec<String> = (0..12).map(|i| format!("fic{i:02}.txt")).collect();
    let refs: Vec<&str> = noms.iter().map(String::as_str).collect();
    let d = Dossier::avec("long", &refs);
    let mut e = EtatLigne::default();
    taper(&mut e, "type fic");
    // Rien de commun au-delà de « fic » : le premier Tab choisit déjà le premier candidat, neuf autres mènent au rang 9
    tab(&mut e, &d, nu());
    for _ in 0..9 {
        tab(&mut e, &d, nu());
    }
    // Dixième candidat choisi (rang 9) : la fenêtre a glissé, la sélection reste dedans
    let menu = e.vue_menu().unwrap();
    assert_eq!((menu.total, menu.lignes.len(), menu.debut), (12, MENU_RANGEES, 2));
    assert_eq!(menu.selection, Some(7));
    assert_eq!(menu.lignes[7].nom, "fic09.txt");
}

// ───────────────────────── Liste automatique (sans Tab) ─────────────────────────

/// Tape comme le fait le panneau : chaque caractère entre dans la ligne, puis la liste se rafraîchit
fn frapper(e: &mut EtatLigne, texte: &str, h: &Historique, dossier: &str) {
    for c in texte.chars() {
        assert_eq!(e.caractere(c), Effet::Pris);
        e.rafraichir(h, dossier);
    }
}

fn bas(e: &mut EtatLigne, h: &Historique) -> Effet {
    e.touche(VK_DOWN, nu(), h, D, true)
}

#[test]
fn la_liste_automatique_vient_a_la_deuxieme_lettre_et_met_l_historique_d_abord() {
    let h = hist(&["git status", "git log", "ls"]);
    let mut e = EtatLigne::default();
    frapper(&mut e, "g", &h, D);
    assert!(e.vue_menu().is_none(), "une seule lettre : pas de liste");
    frapper(&mut e, "i", &h, D);
    let menu = e.vue_menu().expect("la liste apparaît dès la 2e lettre");
    // L'historique d'abord (la plus récente en tête), rien n'est choisi, la ligne n'a pas bougé
    assert_eq!(menu.selection, None);
    assert_eq!(menu.lignes[0].nom, "git log");
    assert_eq!(menu.lignes[1].nom, "git status");
    assert_eq!((menu.lignes[0].genre, menu.lignes[0].description), (Genre::Historique, "historique"));
    assert_eq!(e.saisie().texte(), "gi");
}

#[test]
fn la_liste_automatique_se_ferme_quand_il_n_y_a_plus_rien_a_proposer() {
    let h = hist(&["git status"]);
    let d = Dossier::avec("auto_vide", &[]);
    let mut e = EtatLigne::default();
    frapper(&mut e, "git", &h, &d.texte());
    assert!(e.vue_menu().is_some());
    // Un mot que rien ne prolonge (ni historique ni PATH ni fichier)
    frapper(&mut e, "zqxjk", &h, &d.texte());
    assert!(e.vue_menu().is_none());
}

#[test]
fn les_fleches_choisissent_dans_la_liste_automatique_et_inserent_la_ligne_entiere() {
    let h = hist(&["git status", "git log"]);
    let mut e = EtatLigne::default();
    frapper(&mut e, "gi", &h, D);
    assert_eq!(bas(&mut e, &h), Effet::Pris);
    assert_eq!(e.saisie().texte(), "git log");
    assert_eq!(e.vue_menu().unwrap().selection, Some(0));
    assert_eq!(bas(&mut e, &h), Effet::Pris);
    assert_eq!(e.saisie().texte(), "git status");
    assert_eq!(e.touche(VK_UP, nu(), &h, D, true), Effet::Pris);
    assert_eq!(e.saisie().texte(), "git log");
    // La suggestion grise se tait quand un choix est fait (la ligne montre déjà ce choix)
    assert_eq!(e.suggestion(&h, D), None);
}

#[test]
fn entree_valide_le_choix_sans_executer_puis_la_seconde_entree_execute() {
    let h = hist(&["git status", "git log"]);
    let mut e = EtatLigne::default();
    frapper(&mut e, "gi", &h, D);
    bas(&mut e, &h);
    assert_eq!(touche(&mut e, VK_RETURN, nu(), &h), Effet::Pris, "valide le choix, n'exécute pas");
    assert_eq!(e.saisie().texte(), "git log");
    assert!(e.vue_menu().is_none());
    assert_eq!(touche(&mut e, VK_RETURN, nu(), &h), Effet::Soumettre("git log".to_string()));
}

#[test]
fn entree_sans_choix_soumet_la_ligne_tapee() {
    let h = hist(&["git status"]);
    let mut e = EtatLigne::default();
    frapper(&mut e, "gi", &h, D);
    assert!(e.vue_menu().is_some());
    assert_eq!(touche(&mut e, VK_RETURN, nu(), &h), Effet::Soumettre("gi".to_string()));
}

#[test]
fn echap_ferme_la_liste_automatique_puis_vide_la_ligne() {
    let h = hist(&["git status", "git log"]);
    let mut e = EtatLigne::default();
    frapper(&mut e, "gi", &h, D);
    bas(&mut e, &h);
    assert_eq!(touche(&mut e, VK_ESCAPE, nu(), &h), Effet::Pris);
    assert_eq!(e.saisie().texte(), "gi", "la ligne d'avant le choix revient");
    assert!(e.vue_menu().is_none());
    assert_eq!(touche(&mut e, VK_ESCAPE, nu(), &h), Effet::Pris);
    assert!(e.vide(), "le second Échap vide la ligne");
}

#[test]
fn tab_sans_choix_garde_son_travail_habituel() {
    let d = Dossier::avec("auto_tab", &["alpha1.txt", "alpha2.txt", "zeta.txt"]);
    let h = Historique::default();
    let mut e = EtatLigne::default();
    frapper(&mut e, "type al", &h, &d.texte());
    // La liste automatique montre déjà les deux fichiers, sans rien choisir
    let menu = e.vue_menu().expect("liste automatique");
    assert_eq!((menu.total, menu.selection), (2, None));
    assert_eq!(e.saisie().texte(), "type al");
    // Tab : le début commun, puis le menu de Tab (comme sans liste automatique)
    assert_eq!(tab(&mut e, &d, nu()), Effet::Pris);
    assert_eq!(e.saisie().texte(), "type alpha");
    assert_eq!(e.vue_menu().unwrap().total, 2);
    tab(&mut e, &d, nu());
    assert_eq!(e.saisie().texte(), "type alpha1.txt");
}

#[test]
fn un_fichier_choisi_dans_la_liste_remplace_le_mot_et_la_liste_se_ferme_en_tapant() {
    let d = Dossier::avec("auto_fichier", &["alpha1.txt", "alpha2.txt"]);
    let h = Historique::default();
    let mut e = EtatLigne::default();
    frapper(&mut e, "type al", &h, &d.texte());
    e.touche(VK_DOWN, nu(), &h, &d.texte(), true);
    assert_eq!(e.saisie().texte(), "type alpha1.txt");
    assert_eq!(e.vue_menu().unwrap().lignes[0].genre, Genre::Fichier);
    // Taper un espace garde le choix ; mot vide ensuite : pas de liste
    frapper(&mut e, " ", &h, &d.texte());
    assert_eq!(e.saisie().texte(), "type alpha1.txt ");
    assert!(e.vue_menu().is_none());
}

#[test]
fn la_suggestion_grise_reste_avec_la_liste_automatique_et_droite_l_accepte() {
    let h = hist(&["type alpha1.txt"]);
    let d = Dossier::avec("auto_grise", &["alpha1.txt", "alpha2.txt"]);
    let mut e = EtatLigne::default();
    frapper(&mut e, "type al", &h, &d.texte());
    assert!(e.vue_menu().is_some());
    assert_eq!(e.suggestion(&h, D), Some("pha1.txt".to_string()));
    assert_eq!(touche(&mut e, VK_RIGHT, nu(), &h), Effet::Pris);
    assert_eq!(e.saisie().texte(), "type alpha1.txt");
    assert!(e.vue_menu().is_none());
}

#[test]
fn effacer_des_lettres_rafraichit_la_liste() {
    let h = hist(&["git status"]);
    let mut e = EtatLigne::default();
    frapper(&mut e, "git", &h, D);
    assert!(e.vue_menu().is_some());
    touche(&mut e, VK_BACK, nu(), &h);
    assert_eq!(e.saisie().texte(), "gi");
    assert!(e.vue_menu().is_some());
    touche(&mut e, VK_BACK, nu(), &h);
    assert_eq!(e.saisie().texte(), "g");
    assert!(e.vue_menu().is_none(), "une lettre : la liste disparaît");
}

#[test]
fn pas_de_liste_curseur_au_milieu_multiligne_ou_pendant_un_rappel() {
    let h = hist(&["git status", "git log"]);
    let mut e = EtatLigne::default();
    frapper(&mut e, "gi", &h, D);
    // Curseur au milieu : la liste se ferme et ne revient pas
    touche(&mut e, VK_LEFT, nu(), &h);
    assert!(e.vue_menu().is_none());
    e.rafraichir(&h, D);
    assert!(e.vue_menu().is_none());
    // Collage de plusieurs lignes
    let mut e = EtatLigne::default();
    e.coller("git status\ngit log");
    e.rafraichir(&h, D);
    assert!(e.vue_menu().is_none());
}
