//! Tests de l'émulateur (T1). Trois familles : le sens de chaque séquence, les flux réels enregistrés en T0
//! (`docs/terminal/flux-t0/`), et la robustesse (découpages quelconques, flux aléatoires : aucune panique,
//! invariants de la grille respectés après chaque morceau).

use super::*;

const CMD: &[u8] = include_bytes!("../../../docs/terminal/flux-t0/cmd-drapeau-0x0.bin");
const POWERSHELL: &[u8] = include_bytes!("../../../docs/terminal/flux-t0/powershell-drapeau-0x0.bin");

fn term(colonnes: usize, lignes: usize) -> Terminal {
    Terminal::new(colonnes, lignes)
}

fn ecrit(t: &mut Terminal, s: &str) {
    t.ecrire(s.as_bytes());
}

/// Ligne de l'écran contenant `texte`, si elle existe.
fn ligne_avec(t: &Terminal, texte: &str) -> Option<usize> {
    (0..t.lignes()).find(|&l| t.texte_ligne(l).contains(texte))
}

fn sans_orpheline(ligne: &[Cellule]) {
    for (i, c) in ligne.iter().enumerate() {
        match c.largeur {
            1 => {}
            2 => assert_eq!(ligne.get(i + 1).map(|s| s.largeur), Some(0), "moitié gauche sans moitié droite en {i}"),
            0 => assert!(i > 0 && ligne[i - 1].largeur == 2, "moitié droite orpheline en {i}"),
            n => panic!("largeur {n} invalide"),
        }
    }
}

/// Invariants qui doivent tenir après n'importe quelle suite d'octets et de redimensionnements.
fn verifier(t: &Terminal) {
    let e = &t.etat;
    assert!(e.cols >= 1 && e.cols <= COLONNES_MAX && e.lignes >= 1 && e.lignes <= LIGNES_MAX);
    assert_eq!(e.ecran.len(), e.lignes);
    for l in &e.ecran {
        assert_eq!(l.len(), e.cols);
        sans_orpheline(l);
    }
    if let Some(p) = &e.principal {
        assert_eq!(p.len(), e.lignes);
        for l in p {
            assert_eq!(l.len(), e.cols);
            sans_orpheline(l);
        }
    }
    assert!(e.col < e.cols, "colonne {} hors de {}", e.col, e.cols);
    assert!(e.lig < e.lignes, "ligne {} hors de {}", e.lig, e.lignes);
    assert!(e.haut <= e.bas && e.bas < e.lignes, "région {}..{} invalide", e.haut, e.bas);
    assert_eq!(e.tabs.len(), e.cols);
    assert!(e.historique.len() <= e.historique_max);
    for l in &e.historique {
        assert!(l.len() <= COLONNES_MAX);
        sans_orpheline(l);
    }
    assert!(e.reponses.len() <= REPONSES_MAX);
    assert!(e.marques.len() <= MARQUES_MAX);
    assert!(e.titre.chars().count() <= TITRE_MAX);
    assert!(e.dossier.as_ref().map_or(0, |d| d.chars().count()) <= DOSSIER_MAX);
}

/// Tout ce qui est observable, pour comparer deux terminaux nourris de façons différentes.
fn empreinte(t: &Terminal) -> String {
    format!(
        "{:?}|{:?}|{}|{:?}|{:?}|{:?}|{:?}|{:?}|{}",
        t.etat.ecran,
        t.curseur(),
        t.historique_len(),
        t.etat.historique,
        t.titre(),
        t.dossier(),
        t.marques(),
        t.modes(),
        t.ecran_alterne()
    )
}

struct Alea(u64);

impl Alea {
    fn suivant(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }

    fn borne(&mut self, n: usize) -> usize {
        (self.suivant() % n as u64) as usize
    }
}

/// Octets qui ressemblent à du VT : beaucoup de `ESC [ ? ; :`, de chiffres et de lettres de commande, un peu
/// d'UTF-8 (accent, CJK, emoji, combinant) et quelques octets quelconques.
fn octets_aleatoires(a: &mut Alea, n: usize, pur: bool) -> Vec<u8> {
    const SPECIAUX: &[u8] = b"\x1b\x1b\x1b[[[]?;;::0123456789mmHJKLMPX@rhl\x07\x08\x09\x0a\x0d !(78cDE~tqnpu";
    const UTF8: &[&str] = &["é", "日", "😀", "e\u{301}", "한", "\u{200d}", "\u{fe0f}", "▌", "ｱ"];
    let mut v = Vec::with_capacity(n);
    while v.len() < n {
        if pur {
            v.push((a.suivant() & 0xFF) as u8);
            continue;
        }
        match a.borne(10) {
            0..=3 => v.push(SPECIAUX[a.borne(SPECIAUX.len())]),
            4..=6 => v.push(0x20 + a.borne(95) as u8),
            7 => v.extend_from_slice(UTF8[a.borne(UTF8.len())].as_bytes()),
            8 => v.extend_from_slice(b"\x1b]0;titre\x07"),
            _ => v.push((a.suivant() & 0xFF) as u8),
        }
    }
    v
}

// ───────────────────────── texte et défilement ─────────────────────────

#[test]
fn texte_simple_et_retour_chariot() {
    let mut t = term(20, 5);
    ecrit(&mut t, "abc\r\ndef");
    assert_eq!(t.texte_ligne(0), "abc");
    assert_eq!(t.texte_ligne(1), "def");
    assert_eq!(t.curseur(), (3, 1));
    verifier(&t);
}

#[test]
fn saut_de_ligne_seul_garde_la_colonne() {
    let mut t = term(20, 5);
    ecrit(&mut t, "ab\ncd");
    assert_eq!(t.texte_ligne(1), "  cd");
}

#[test]
fn retour_automatique_differe() {
    let mut t = term(5, 3);
    ecrit(&mut t, "abcde");
    assert_eq!(t.curseur(), (4, 0), "le curseur reste sur la dernière colonne tant qu'on n'écrit pas la suite");
    assert_eq!(t.texte_ligne(1), "");
    ecrit(&mut t, "fg");
    assert_eq!(t.texte_ligne(0), "abcde");
    assert_eq!(t.texte_ligne(1), "fg");
    // Un CRLF juste après une ligne pleine ne doit pas sauter une ligne de plus.
    let mut u = term(5, 4);
    ecrit(&mut u, "abcde\r\nx");
    assert_eq!(u.texte_ligne(1), "x");
}

#[test]
fn retour_automatique_coupe() {
    let mut t = term(5, 3);
    ecrit(&mut t, "\x1b[?7labcdefg");
    assert_eq!(t.texte_ligne(0), "abcdg");
    assert_eq!(t.texte_ligne(1), "");
    assert!(!t.modes().retour_auto);
}

#[test]
fn defilement_vers_l_historique() {
    let mut t = term(10, 3);
    ecrit(&mut t, "1\r\n2\r\n3\r\n4\r\n5");
    assert_eq!(t.historique_len(), 2);
    assert_eq!(t.texte_historique(0).as_deref(), Some("1"));
    assert_eq!(t.texte_historique(1).as_deref(), Some("2"));
    assert_eq!(t.texte_ecran(), "3\n4\n5");
    verifier(&t);
}

#[test]
fn historique_borne() {
    let mut t = Terminal::avec_historique(30, 3, 100);
    for i in 0..20_000 {
        ecrit(&mut t, &format!("ligne {i}\r\n"));
    }
    assert_eq!(t.historique_len(), 100);
    // Écran : l19998, l19999 et une ligne vide ; la dernière ligne d'historique est donc l19997.
    assert_eq!(t.texte_historique(99).as_deref(), Some("ligne 19997"));
    assert_eq!(t.lignes_ecoulees(), 19_998 - 100);
    verifier(&t);
}

#[test]
fn historique_sans_blancs_de_fin() {
    let mut t = term(200, 2);
    ecrit(&mut t, "court\r\nx\r\ny");
    assert_eq!(t.historique_len(), 1);
    assert_eq!(t.ligne_historique(0).map(|l| l.len()), Some(5), "les blancs de fin ne sont pas gardés");
}

#[test]
fn historique_desactive() {
    let mut t = Terminal::avec_historique(10, 2, 0);
    ecrit(&mut t, "1\r\n2\r\n3\r\n4");
    assert_eq!(t.historique_len(), 0);
    assert_eq!(t.lignes_ecoulees(), 2);
    assert_eq!(t.texte_ecran(), "3\n4");
}

#[test]
fn effacement_de_l_historique() {
    let mut t = term(10, 2);
    ecrit(&mut t, "1\r\n2\r\n3\r\n4\x1b[3J");
    assert_eq!(t.historique_len(), 0);
    assert!(t.lignes_ecoulees() >= 2);
    assert_eq!(t.texte_ecran(), "");
}

// ───────────────────────── couleurs ─────────────────────────

#[test]
fn sgr_basique() {
    let mut t = term(20, 2);
    ecrit(&mut t, "\x1b[31mA\x1b[1;4mB\x1b[0mC\x1b[42;93mD");
    let a = t.cellule(0, 0).unwrap();
    assert_eq!(a.attr.fg, Couleur::Indexee(1));
    assert_eq!(a.attr.drapeaux, 0);
    let b = t.cellule(1, 0).unwrap();
    assert_eq!(b.attr.fg, Couleur::Indexee(1));
    assert!(b.attr.a(Attr::GRAS) && b.attr.a(Attr::SOULIGNE));
    assert_eq!(t.cellule(2, 0).unwrap().attr, Attr::default());
    let d = t.cellule(3, 0).unwrap();
    assert_eq!((d.attr.bg, d.attr.fg), (Couleur::Indexee(2), Couleur::Indexee(11)));
}

#[test]
fn sgr_sans_parametre_remet_a_zero() {
    let mut t = term(20, 2);
    ecrit(&mut t, "\x1b[31;1m\x1b[mA\x1b[32m\x1b[0;mB");
    assert_eq!(t.cellule(0, 0).unwrap().attr, Attr::default());
    assert_eq!(t.cellule(1, 0).unwrap().attr, Attr::default());
}

#[test]
fn sgr_256_et_rvb() {
    let mut t = term(20, 2);
    ecrit(&mut t, "\x1b[38;5;10mx\x1b[48;2;1;2;3my\x1b[38:2::10:20:30mz\x1b[38:5:200mw\x1b[48;5;255mv");
    assert_eq!(t.cellule(0, 0).unwrap().attr.fg, Couleur::Indexee(10));
    let y = t.cellule(1, 0).unwrap();
    assert_eq!(y.attr.bg, Couleur::Rvb(1, 2, 3));
    assert_eq!(y.attr.fg, Couleur::Indexee(10), "48 ne touche pas au premier plan");
    assert_eq!(t.cellule(2, 0).unwrap().attr.fg, Couleur::Rvb(10, 20, 30));
    assert_eq!(t.cellule(3, 0).unwrap().attr.fg, Couleur::Indexee(200));
    assert_eq!(t.cellule(4, 0).unwrap().attr.bg, Couleur::Indexee(255));
}

#[test]
fn sgr_incomplet_ou_aberrant_est_ignore() {
    let mut t = term(20, 2);
    ecrit(&mut t, "\x1b[38mA\x1b[38;5mB\x1b[38;2;1mC\x1b[38;5;999mD\x1b[38;2;1;2;999mE\x1b[999mF\x1b[48:2mG");
    for c in 0..6 {
        let cel = t.cellule(c, 0).unwrap();
        if c == 4 {
            // Une composante RVB trop grande est ramenée à 255 plutôt que rejetée.
            assert_eq!(cel.attr.fg, Couleur::Rvb(1, 2, 255));
        }
    }
    assert_eq!(t.cellule(0, 0).unwrap().attr.fg, Couleur::Defaut);
    verifier(&t);
}

#[test]
fn sgr_drapeaux_et_annulations() {
    let mut t = term(20, 2);
    ecrit(&mut t, "\x1b[1;2;3;4;7;8;9mA\x1b[22;23;24;27;28;29mB\x1b[4mC\x1b[4:0mD");
    let a = t.cellule(0, 0).unwrap().attr;
    for d in [Attr::GRAS, Attr::PALE, Attr::ITALIQUE, Attr::SOULIGNE, Attr::INVERSE, Attr::CACHE, Attr::BARRE] {
        assert!(a.a(d), "drapeau {d} manquant");
    }
    assert_eq!(t.cellule(1, 0).unwrap().attr.drapeaux, 0);
    assert!(t.cellule(2, 0).unwrap().attr.a(Attr::SOULIGNE));
    assert!(!t.cellule(3, 0).unwrap().attr.a(Attr::SOULIGNE));
}

#[test]
fn l_effacement_garde_la_couleur_de_fond() {
    let mut t = term(6, 2);
    ecrit(&mut t, "\x1b[44m\x1b[2K");
    assert_eq!(t.cellule(3, 0).unwrap().attr.bg, Couleur::Indexee(4));
    assert_eq!(t.texte_ligne(0), "");
}

// ───────────────────────── curseur ─────────────────────────

#[test]
fn positionnement_du_curseur() {
    let mut t = term(20, 10);
    ecrit(&mut t, "\x1b[3;5H");
    assert_eq!(t.curseur(), (4, 2));
    ecrit(&mut t, "\x1b[A\x1b[2C\x1b[B\x1b[3D");
    assert_eq!(t.curseur(), (3, 2));
    ecrit(&mut t, "\x1b[0;0H");
    assert_eq!(t.curseur(), (0, 0));
    ecrit(&mut t, "\x1b[999;999H");
    assert_eq!(t.curseur(), (19, 9));
    ecrit(&mut t, "\x1b[5d\x1b[7G");
    assert_eq!(t.curseur(), (6, 4));
    ecrit(&mut t, "\x1b[2E");
    assert_eq!(t.curseur(), (0, 6));
    ecrit(&mut t, "\x1b[3F");
    assert_eq!(t.curseur(), (0, 3));
    verifier(&t);
}

#[test]
fn sauvegarde_du_curseur() {
    let mut t = term(20, 5);
    ecrit(&mut t, "\x1b[3;4H\x1b[31m\x1b7\x1b[1;1H\x1b[0m\x1b8x");
    assert_eq!(t.cellule(3, 2).unwrap().c, 'x');
    assert_eq!(t.cellule(3, 2).unwrap().attr.fg, Couleur::Indexee(1));
    ecrit(&mut t, "\x1b[5;5H\x1b[s\x1b[1;1H\x1b[u");
    assert_eq!(t.curseur(), (4, 4));
}

#[test]
fn region_de_defilement_et_origine() {
    let mut t = term(10, 5);
    ecrit(&mut t, "TOP\x1b[2;4r\x1b[2;1Hb\r\nc\r\nd\r\ne");
    assert_eq!(t.texte_ligne(0), "TOP");
    assert_eq!(t.texte_ligne(1), "c");
    assert_eq!(t.texte_ligne(2), "d");
    assert_eq!(t.texte_ligne(3), "e");
    assert_eq!(t.historique_len(), 0, "une région qui ne part pas du haut n'alimente pas l'historique");
    ecrit(&mut t, "\x1b[?6h\x1b[1;1H");
    assert_eq!(t.curseur(), (0, 1));
    ecrit(&mut t, "\x1b[6n");
    assert_eq!(t.prendre_reponses(), b"\x1b[1;1R");
    ecrit(&mut t, "\x1b[r\x1b[?6l\x1b[H");
    assert_eq!(t.curseur(), (0, 0));
    verifier(&t);
}

#[test]
fn region_invalide_ignoree() {
    let mut t = term(10, 5);
    ecrit(&mut t, "\x1b[4;2r\x1b[9;12r\x1b[3;3r");
    assert_eq!((t.etat.haut, t.etat.bas), (0, 4));
}

#[test]
fn index_inverse_en_haut_decale_vers_le_bas() {
    let mut t = term(10, 3);
    ecrit(&mut t, "a\r\nb\r\nc\x1b[1;1H\x1bM");
    assert_eq!(t.texte_ecran(), "\na\nb");
}

#[test]
fn tabulations() {
    let mut t = term(20, 2);
    ecrit(&mut t, "a\tb");
    assert_eq!(t.texte_ligne(0), format!("a{}b", " ".repeat(7)));
    assert_eq!(t.curseur(), (9, 0));
    ecrit(&mut t, "\x1b[Z");
    assert_eq!(t.curseur(), (8, 0));
    ecrit(&mut t, "\x1b[3g\x1b[1;1H\t");
    assert_eq!(t.curseur(), (19, 0), "sans taquet, la tabulation va en bout de ligne");
    verifier(&t);
}

// ───────────────────────── effacements et insertions ─────────────────────────

#[test]
fn effacements_de_ligne() {
    let mut t = term(10, 2);
    ecrit(&mut t, "abcdef\x1b[1;3H\x1b[K");
    assert_eq!(t.texte_ligne(0), "ab");
    ecrit(&mut t, "\x1b[1;1Habcdef\x1b[1;3H\x1b[1K");
    assert_eq!(t.texte_ligne(0), "   def");
    ecrit(&mut t, "\x1b[2K");
    assert_eq!(t.texte_ligne(0), "");
}

#[test]
fn effacements_d_ecran() {
    let mut t = term(10, 3);
    ecrit(&mut t, "abc\r\ndef\r\nghi\x1b[2;2H\x1b[J");
    assert_eq!(t.texte_ecran(), "abc\nd");
    ecrit(&mut t, "\x1b[1;1Habc\r\ndef\r\nghi\x1b[2;2H\x1b[1J");
    assert_eq!(t.texte_ecran(), "\n  f\nghi");
    ecrit(&mut t, "\x1b[2J");
    assert_eq!(t.texte_ecran(), "");
}

#[test]
fn caracteres_inseres_effaces_supprimes() {
    let mut t = term(10, 2);
    ecrit(&mut t, "abcdef\x1b[1;3H\x1b[2P");
    assert_eq!(t.texte_ligne(0), "abef");
    ecrit(&mut t, "\x1b[2J\x1b[1;1Habcdef\x1b[1;3H\x1b[2@");
    assert_eq!(t.texte_ligne(0), "ab  cdef");
    ecrit(&mut t, "\x1b[2J\x1b[1;1Habcdef\x1b[1;3H\x1b[2X");
    assert_eq!(t.texte_ligne(0), "ab  ef");
    // Des nombres énormes ne dépassent pas la ligne.
    ecrit(&mut t, "\x1b[1;3H\x1b[9999@\x1b[9999P\x1b[9999X");
    verifier(&t);
}

#[test]
fn mode_insertion() {
    let mut t = term(10, 2);
    ecrit(&mut t, "abcd\x1b[1;2H\x1b[4hXY\x1b[4lZ");
    assert_eq!(t.texte_ligne(0), "aXYZcd");
}

#[test]
fn lignes_inserees_et_supprimees() {
    let mut t = term(10, 4);
    ecrit(&mut t, "a\r\nb\r\nc\r\nd\x1b[2;1H\x1b[L");
    assert_eq!(t.texte_ecran(), "a\n\nb\nc");
    ecrit(&mut t, "\x1b[M");
    assert_eq!(t.texte_ecran(), "a\nb\nc");
    ecrit(&mut t, "\x1b[S");
    assert_eq!(t.texte_ecran(), "b\nc");
    ecrit(&mut t, "\x1b[T");
    assert_eq!(t.texte_ecran(), "\nb\nc");
    verifier(&t);
}

// ───────────────────────── écran alterné et modes ─────────────────────────

#[test]
fn ecran_alterne() {
    let mut t = term(20, 5);
    ecrit(&mut t, "principal");
    ecrit(&mut t, "\x1b[?1049h");
    assert!(t.ecran_alterne());
    assert_eq!(t.texte_ecran(), "");
    ecrit(&mut t, "alt");
    assert!(t.texte_ligne(0).ends_with("alt"));
    for i in 0..20 {
        ecrit(&mut t, &format!("\r\nl{i}"));
    }
    assert_eq!(t.historique_len(), 0, "l'écran alterné n'a pas d'historique");
    ecrit(&mut t, "\x1b[?1049l");
    assert!(!t.ecran_alterne());
    assert_eq!(t.texte_ecran(), "principal");
    assert_eq!(t.curseur(), (9, 0));
    verifier(&t);
}

#[test]
fn ecran_alterne_47_sans_sauvegarde_du_curseur() {
    let mut t = term(20, 5);
    ecrit(&mut t, "ab\x1b[?47hxyz\x1b[?47l");
    assert_eq!(t.texte_ecran(), "ab");
    assert!(!t.ecran_alterne());
}

#[test]
fn les_modes_de_conpty_sont_ignores_proprement() {
    let mut t = term(20, 3);
    ecrit(&mut t, "\x1b[?9001h\x1b[?1004h");
    assert_eq!(t.texte_ecran(), "");
    assert_eq!(t.curseur(), (0, 0));
    assert!(t.prendre_reponses().is_empty());
    assert!(t.modes().focus);
    ecrit(&mut t, "\x1b[?9001l\x1b[?1004l");
    assert!(!t.modes().focus);
    assert_eq!(t.modes(), Modes::default());
    // Modes inconnus ou à plusieurs paramètres.
    ecrit(&mut t, "\x1b[?12;7727;2026h\x1b[?4242l");
    assert_eq!(t.texte_ecran(), "");
}

#[test]
fn un_seul_niveau_de_suivi_de_la_souris_a_la_fois() {
    let mut t = term(20, 3);
    ecrit(&mut t, "\x1b[?1003h");
    assert!(t.modes().souris_mouvement && !t.modes().souris_glisse);
    // Demander un autre niveau remplace le précédent (xterm)
    ecrit(&mut t, "\x1b[?1002h");
    let m = t.modes();
    assert!(m.souris_glisse && !m.souris_mouvement && !m.souris_appui);
    // Couper un niveau qui n'est pas le courant ne touche pas au courant
    ecrit(&mut t, "\x1b[?1003l");
    assert!(t.modes().souris_glisse);
    ecrit(&mut t, "\x1b[?1002l");
    assert!(!t.modes().souris_glisse);
}

#[test]
fn modes_declares() {
    let mut t = term(20, 3);
    ecrit(&mut t, "\x1b[?25l\x1b[?2004h\x1b[?1h\x1b[?1000;1006h");
    let m = t.modes();
    assert!(!m.curseur_visible && m.collage_encadre && m.touches_curseur_appli && m.souris_appui && m.souris_sgr);
    assert!(!t.curseur_visible());
    ecrit(&mut t, "\x1b[!p");
    assert_eq!(t.modes(), Modes::default());
    ecrit(&mut t, "\x1b[4 q");
    assert_eq!(t.forme_curseur(), 4);
}

#[test]
fn reinitialisation_totale() {
    let mut t = term(20, 3);
    ecrit(&mut t, "abc\x1b[?1049hxyz\x1b[31m\x1bc");
    assert!(!t.ecran_alterne());
    assert_eq!(t.texte_ecran(), "");
    assert_eq!(t.curseur(), (0, 0));
    assert_eq!(t.etat.attr, Attr::default());
    verifier(&t);
}

// ───────────────────────── titre, dossier, marques ─────────────────────────

#[test]
fn titre_par_osc() {
    let mut t = term(20, 3);
    ecrit(&mut t, "\x1b]0;Mon titre\x07");
    assert_eq!(t.titre(), "Mon titre");
    ecrit(&mut t, "\x1b]2;Autre; avec point-virgule\x1b\\");
    assert_eq!(t.titre(), "Autre; avec point-virgule");
    ecrit(&mut t, &format!("\x1b]0;{}\x07", "x".repeat(10_000)));
    assert_eq!(t.titre().chars().count(), TITRE_MAX);
    assert_eq!(t.texte_ecran(), "", "un OSC n'écrit rien à l'écran");
}

#[test]
fn dossier_par_osc_9_9() {
    let mut t = term(20, 3);
    assert_eq!(t.dossier(), None);
    ecrit(&mut t, "\x1b]9;9;C:\\Users\\x\x1b\\");
    assert_eq!(t.dossier(), Some("C:\\Users\\x"));
    ecrit(&mut t, "\x1b]9;9;\"C:\\Mes documents;v2\"\x07");
    assert_eq!(t.dossier(), Some("C:\\Mes documents;v2"));
    // Un OSC 9 d'un autre genre (notification) ne change rien.
    ecrit(&mut t, "\x1b]9;4;1;50\x07\x1b]9;9;\x07");
    assert_eq!(t.dossier(), Some("C:\\Mes documents;v2"));
}

#[test]
fn marques_osc_133() {
    let mut t = term(20, 3);
    ecrit(&mut t, "\x1b]133;A\x07$ \x1b]133;B\x07cmd\r\n\x1b]133;C\x07\r\n\r\n\r\n\x1b]133;D;3\x07\x1b]133;D\x07\x1b]133;Z\x07");
    let m = t.marques();
    assert_eq!(m.len(), 5);
    assert_eq!(m[0], Marque { genre: 'A', ligne: 0, code: None });
    assert_eq!(m[1], Marque { genre: 'B', ligne: 0, code: None });
    assert_eq!(m[2], Marque { genre: 'C', ligne: 1, code: None });
    assert_eq!(m[3], Marque { genre: 'D', ligne: 4, code: Some(3) }, "ligne absolue : stable quand l'écran défile");
    assert_eq!(m[4], Marque { genre: 'D', ligne: 4, code: None });
    assert_eq!(t.historique_len(), 2);
}

#[test]
fn marques_bornees_et_absentes_en_ecran_alterne() {
    let mut t = term(20, 3);
    for _ in 0..1_000 {
        ecrit(&mut t, "\x1b]133;A\x07");
    }
    assert_eq!(t.marques().len(), MARQUES_MAX);
    let avant = t.marques().len();
    ecrit(&mut t, "\x1b[?1049h\x1b]133;A\x07\x1b[?1049l");
    assert_eq!(t.marques().len(), avant);
}

// ───────────────────────── caractères larges, accents ─────────────────────────

#[test]
fn caracteres_larges() {
    let mut t = term(10, 2);
    ecrit(&mut t, "a日b");
    assert_eq!(t.cellule(1, 0).unwrap().largeur, 2);
    assert_eq!(t.cellule(2, 0).unwrap().largeur, 0);
    assert_eq!(t.texte_ligne(0), "a日b");
    assert_eq!(t.curseur(), (4, 0));
    // Écrire sur la moitié droite détruit le caractère entier.
    ecrit(&mut t, "\x1b[1;3Hx");
    assert_eq!(t.texte_ligne(0), "a xb");
    // Écrire sur la moitié gauche aussi.
    ecrit(&mut t, "\x1b[1;1H日\x1b[1;2Hy");
    assert_eq!(t.texte_ligne(0), " yxb");
    verifier(&t);
}

#[test]
fn caractere_large_en_bout_de_ligne_passe_a_la_suivante() {
    let mut t = term(3, 3);
    ecrit(&mut t, "ab日");
    assert_eq!(t.texte_ligne(0), "ab");
    assert_eq!(t.texte_ligne(1), "日");
    verifier(&t);
    // Sans retour automatique, il se pose sur les deux dernières colonnes.
    let mut u = term(3, 3);
    ecrit(&mut u, "\x1b[?7lab日");
    assert_eq!(u.texte_ligne(0), "a日");
    verifier(&u);
}

#[test]
fn emoji_sur_deux_colonnes() {
    let mut t = term(10, 2);
    ecrit(&mut t, "😀x");
    assert_eq!(t.cellule(0, 0).unwrap().largeur, 2);
    assert_eq!(t.cellule(2, 0).unwrap().c, 'x');
    assert_eq!(t.curseur(), (3, 0));
}

#[test]
fn largeur_des_caracteres() {
    assert_eq!(largeur_char('a'), 1);
    assert_eq!(largeur_char('é'), 1);
    assert_eq!(largeur_char('▌'), 1);
    assert_eq!(largeur_char('日'), 2);
    assert_eq!(largeur_char('한'), 2);
    assert_eq!(largeur_char('Ａ'), 2);
    assert_eq!(largeur_char('😀'), 2);
    assert_eq!(largeur_char('\u{301}'), 0);
    assert_eq!(largeur_char('\u{200d}'), 0);
    assert_eq!(largeur_char('\u{fe0f}'), 0);
    assert_eq!(largeur_char('\u{7f}'), 0);
}

#[test]
fn accents_et_combinants() {
    let mut t = term(20, 2);
    ecrit(&mut t, "é à ü ñ œ €");
    assert_eq!(t.texte_ligne(0), "é à ü ñ œ €");
    ecrit(&mut t, "\r\ne\u{301}x");
    assert_eq!(t.texte_ligne(1), "ex", "le combinant est ignoré, il ne décale pas la suite");
}

#[test]
fn utf8_coupe_au_milieu_d_un_caractere() {
    let s = "é日😀 fin";
    let mut a = term(20, 2);
    a.ecrire(s.as_bytes());
    let mut b = term(20, 2);
    for o in s.as_bytes() {
        b.ecrire(std::slice::from_ref(o));
    }
    assert_eq!(b.texte_ligne(0), s);
    assert_eq!(empreinte(&a), empreinte(&b));
}

#[test]
fn utf8_invalide_ne_plante_pas() {
    let mut t = term(20, 2);
    t.ecrire(b"a\xff\xfeb\xc3(c\xe6\x97");
    verifier(&t);
    assert!(t.texte_ligne(0).starts_with('a'));
}

// ───────────────────────── réponses au shell ─────────────────────────

#[test]
fn reponses_de_position_et_d_appareil() {
    let mut t = term(80, 24);
    ecrit(&mut t, "\r\n\r\nabc\x1b[6n");
    assert_eq!(t.prendre_reponses(), b"\x1b[3;4R");
    assert!(t.prendre_reponses().is_empty(), "la file est vidée à chaque lecture");
    ecrit(&mut t, "\x1b[5n\x1b[c\x1b[0c\x1b[>c");
    assert_eq!(t.prendre_reponses(), b"\x1b[0n\x1b[?1;2c\x1b[?1;2c");
}

#[test]
fn les_reponses_sont_bornees() {
    let mut t = term(80, 24);
    for _ in 0..10_000 {
        ecrit(&mut t, "\x1b[6n");
    }
    assert!(t.prendre_reponses().len() <= REPONSES_MAX);
}

#[test]
fn cloche() {
    let mut t = term(10, 2);
    ecrit(&mut t, "a\x07b\x07\x07");
    assert_eq!(t.prendre_cloches(), 3);
    assert_eq!(t.prendre_cloches(), 0);
    assert_eq!(t.texte_ligne(0), "ab");
}

// ───────────────────────── taille et redimensionnement ─────────────────────────

#[test]
fn plafonds_de_taille() {
    let t = term(100_000, 100_000);
    assert_eq!((t.colonnes(), t.lignes()), (COLONNES_MAX, LIGNES_MAX));
    let mut u = term(0, 0);
    assert_eq!((u.colonnes(), u.lignes()), (1, 1));
    ecrit(&mut u, "abc\r\ndef\x1b[5;5H\x1b[2J日");
    verifier(&u);
    u.redimensionner(0, 0);
    u.redimensionner(5_000, 5_000);
    assert_eq!((u.colonnes(), u.lignes()), (COLONNES_MAX, LIGNES_MAX));
    verifier(&u);
}

#[test]
fn la_cellule_reste_petite() {
    assert!(std::mem::size_of::<Cellule>() <= 16, "{} octets", std::mem::size_of::<Cellule>());
}

#[test]
fn reduire_la_hauteur_garde_le_haut_dans_l_historique() {
    let mut t = term(10, 5);
    ecrit(&mut t, "1\r\n2\r\n3\r\n4\r\n5");
    t.redimensionner(10, 3);
    assert_eq!(t.historique_len(), 2);
    assert_eq!(t.texte_ecran(), "3\n4\n5");
    assert_eq!(t.curseur(), (1, 2));
    t.redimensionner(10, 5);
    assert_eq!(t.historique_len(), 0);
    assert_eq!(t.texte_ecran(), "1\n2\n3\n4\n5");
    assert_eq!(t.curseur(), (1, 4));
    verifier(&t);
}

#[test]
fn reduire_la_hauteur_sacrifie_d_abord_les_lignes_vides_du_bas() {
    let mut t = term(10, 5);
    ecrit(&mut t, "a\r\nb");
    t.redimensionner(10, 3);
    assert_eq!(t.historique_len(), 0);
    assert_eq!(t.texte_ecran(), "a\nb");
    assert_eq!(t.curseur(), (1, 1));
}

#[test]
fn redimensionner_la_largeur() {
    let mut t = term(10, 3);
    ecrit(&mut t, "abcdef\r\nab日");
    t.redimensionner(3, 3);
    assert_eq!(t.texte_ligne(0), "abc");
    assert_eq!(t.texte_ligne(1), "ab", "un caractère large coupé par le bord disparaît entier");
    verifier(&t);
    t.redimensionner(12, 3);
    assert_eq!(t.colonnes(), 12);
    verifier(&t);
}

#[test]
fn redimensionner_en_ecran_alterne() {
    let mut t = term(10, 5);
    ecrit(&mut t, "principal\x1b[?1049hxx");
    t.redimensionner(6, 3);
    verifier(&t);
    t.redimensionner(20, 8);
    verifier(&t);
    ecrit(&mut t, "\x1b[?1049l");
    verifier(&t);
    assert!(t.texte_ecran().starts_with("princi"));
}

#[test]
fn la_generation_change() {
    let mut t = term(10, 2);
    let g = t.generation();
    ecrit(&mut t, "");
    assert_eq!(t.generation(), g, "un lot vide ne change rien");
    ecrit(&mut t, "a");
    assert_ne!(t.generation(), g);
    let g = t.generation();
    t.redimensionner(11, 2);
    assert_ne!(t.generation(), g);
}

// ───────────────────────── flux réels de ConPTY (T0) ─────────────────────────

#[test]
fn flux_reel_cmd() {
    let mut t = term(120, 30);
    t.ecrire(CMD);
    verifier(&t);
    let texte = t.texte_ecran();
    assert!(texte.contains("Microsoft Windows"), "bannière absente :\n{texte}");
    assert!(texte.contains("bonjour"), "la sortie de la commande est absente :\n{texte}");
    assert!(!texte.contains('\x1b'), "une séquence est restée à l'écran :\n{texte}");
    assert!(!t.titre().is_empty(), "le titre OSC 0 n'a pas été lu");
    assert_eq!(t.dossier(), Some("C:\\"), "dernier dossier annoncé par OSC 9;9");
    assert!(t.prendre_reponses().is_empty() || t.prendre_reponses().len() <= REPONSES_MAX);
    let invite = ligne_avec(&t, "C:\\>").expect("invite C:\\> absente");
    assert!(t.texte_ligne(invite).starts_with("C:\\>"));
}

#[test]
fn flux_reel_powershell() {
    let mut t = term(120, 30);
    t.ecrire(POWERSHELL);
    verifier(&t);
    let texte = t.texte_ecran();
    assert!(texte.contains("PS C:\\"), "invite absente :\n{texte}");
    assert!(!texte.contains('\x1b'), "une séquence est restée à l'écran :\n{texte}");
    let l = (0..t.lignes()).find(|&l| t.texte_ligne(l) == "bonjour").expect("« bonjour » absent");
    let colore = t.ligne(l).unwrap().iter().any(|c| c.c == 'b' && c.attr.fg == Couleur::Indexee(10));
    assert!(colore, "« bonjour » devrait être en vert (38;5;10) :\n{texte}");
    let invites = t.marques().iter().filter(|m| m.genre == 'A').count();
    assert_eq!(invites, 3, "marques : {:?}", t.marques());
    assert!(t.dossier().is_some_and(|d| d.starts_with("C:\\")));
}

#[test]
fn flux_reels_identiques_quel_que_soit_le_drapeau() {
    // T0 : les quatre drapeaux de CreatePseudoConsole donnent le même flux ; l'émulateur donne donc le même écran.
    let cmd8: &[u8] = include_bytes!("../../../docs/terminal/flux-t0/cmd-drapeau-0x18.bin");
    let ps8: &[u8] = include_bytes!("../../../docs/terminal/flux-t0/powershell-drapeau-0x18.bin");
    for (a, b) in [(CMD, cmd8), (POWERSHELL, ps8)] {
        let (mut x, mut y) = (term(120, 30), term(120, 30));
        x.ecrire(a);
        y.ecrire(b);
        assert_eq!(empreinte(&x), empreinte(&y));
    }
}

#[test]
fn flux_reel_octet_par_octet_ou_en_morceaux_donne_le_meme_ecran() {
    for flux in [CMD, POWERSHELL] {
        let mut bloc = term(120, 30);
        bloc.ecrire(flux);
        let mut un_par_un = term(120, 30);
        for o in flux {
            un_par_un.ecrire(std::slice::from_ref(o));
        }
        assert_eq!(empreinte(&bloc), empreinte(&un_par_un));
        for graine in 1..=20u64 {
            let mut a = Alea(graine.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1);
            let mut morceaux = term(120, 30);
            let mut reste = flux;
            while !reste.is_empty() {
                let n = (1 + a.borne(40)).min(reste.len());
                morceaux.ecrire(&reste[..n]);
                reste = &reste[n..];
            }
            assert_eq!(empreinte(&bloc), empreinte(&morceaux), "graine {graine}");
        }
    }
}

#[test]
fn flux_reel_dans_une_petite_fenetre() {
    // Même flux dans une grille étroite : rien ne doit planter ni sortir de la grille.
    for (c, l) in [(1, 1), (2, 2), (10, 3), (40, 5)] {
        for flux in [CMD, POWERSHELL] {
            let mut t = term(c, l);
            t.ecrire(flux);
            verifier(&t);
        }
    }
}

// ───────────────────────── robustesse ─────────────────────────

#[test]
fn sequences_coupees_a_chaque_octet() {
    let flux = "\x1b[31;1mrouge\x1b[0m \x1b[38;2;10;20;30mrvb\x1b[0m\x1b]0;titre\x07\x1b]9;9;C:\\a\x1b\\\x1b[2;3Hx\x1b[?1049hy\x1b[?1049l日é";
    let mut bloc = term(30, 5);
    bloc.ecrire(flux.as_bytes());
    let mut octets = term(30, 5);
    for o in flux.as_bytes() {
        octets.ecrire(std::slice::from_ref(o));
    }
    assert_eq!(empreinte(&bloc), empreinte(&octets));
    assert_eq!(bloc.titre(), "titre");
    assert_eq!(bloc.dossier(), Some("C:\\a"));
}

#[test]
fn flux_aleatoire_vt_sans_panique_et_invariants_tenus() {
    for graine in 1..=40u64 {
        let mut a = Alea(graine.wrapping_mul(0x2545_F491_4F6C_DD1D) | 1);
        let mut t = Terminal::avec_historique(1 + a.borne(120), 1 + a.borne(40), 50);
        for tour in 0..300 {
            let n = 1 + a.borne(80);
            let octets = octets_aleatoires(&mut a, n, false);
            t.ecrire(&octets);
            verifier(&t);
            if tour % 7 == 0 {
                t.redimensionner(1 + a.borne(150), 1 + a.borne(50));
                verifier(&t);
            }
            let _ = t.prendre_reponses();
        }
    }
}

#[test]
fn flux_aleatoire_d_octets_purs_sans_panique() {
    for graine in 1..=40u64 {
        let mut a = Alea(graine.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1);
        let mut t = Terminal::avec_historique(1 + a.borne(100), 1 + a.borne(30), 50);
        for _ in 0..200 {
            let n = 1 + a.borne(200);
            let octets = octets_aleatoires(&mut a, n, true);
            t.ecrire(&octets);
            verifier(&t);
        }
    }
}

#[test]
fn flux_aleatoire_decoupe_ou_non_donne_le_meme_etat() {
    for graine in 1..=25u64 {
        let mut a = Alea(graine.wrapping_mul(0xD1B5_4A32_D192_ED03) | 1);
        let flux = octets_aleatoires(&mut a, 4_000, false);
        let mut bloc = Terminal::avec_historique(40, 12, 50);
        bloc.ecrire(&flux);
        let mut morceaux = Terminal::avec_historique(40, 12, 50);
        let mut reste = flux.as_slice();
        while !reste.is_empty() {
            let n = (1 + a.borne(30)).min(reste.len());
            morceaux.ecrire(&reste[..n]);
            reste = &reste[n..];
        }
        assert_eq!(empreinte(&bloc), empreinte(&morceaux), "graine {graine}");
        assert_eq!(bloc.prendre_reponses(), morceaux.prendre_reponses(), "graine {graine}");
    }
}

#[test]
fn nombres_enormes_dans_les_sequences() {
    let mut t = term(20, 5);
    ecrit(&mut t, "\x1b[99999999999;99999999999H\x1b[99999999999A\x1b[99999999999B\x1b[99999999999C\x1b[99999999999D");
    ecrit(&mut t, "\x1b[99999999999L\x1b[99999999999M\x1b[99999999999S\x1b[99999999999T\x1b[99999999999@");
    ecrit(&mut t, "\x1b[1;2;3;4;5;6;7;8;9;10;11;12;13;14;15;16;17;18;19;20;21;22;23;24;25;26;27;28;29;30;31;32;33;34;35m");
    ecrit(&mut t, "\x1b[0;0;0;0r\x1b[0;0H\x1b[?0;0;0h");
    verifier(&t);
}

#[test]
fn osc_et_dcs_interminables_ou_non_termines() {
    let mut t = term(20, 5);
    t.ecrire(b"\x1b]0;");
    t.ecrire(&vec![b'a'; 100_000]);
    t.ecrire(b"\x1b\\ok");
    t.ecrire(b"\x1bP1;2|");
    t.ecrire(&vec![b'z'; 100_000]);
    t.ecrire(b"\x1b\\fin");
    verifier(&t);
    assert!(t.titre().chars().count() <= TITRE_MAX);
    assert!(t.texte_ecran().ends_with("fin"));
}

#[test]
fn gros_volume_en_peu_de_temps() {
    // 200 000 lignes de 100 caractères colorés : doit rester très en deçà de la seconde en release comme en debug lent.
    let ligne = format!("\x1b[32m{}\x1b[0m\r\n", "x".repeat(100));
    let mut t = term(120, 30);
    let debut = std::time::Instant::now();
    for _ in 0..20_000 {
        t.ecrire(ligne.as_bytes());
    }
    let ecoule = debut.elapsed();
    assert!(t.historique_len() <= HISTORIQUE_DEFAUT);
    verifier(&t);
    assert!(ecoule.as_secs() < 20, "trop lent : {ecoule:?}");
}

#[test]
fn accent_coupe_suivi_d_un_caractere_incomplet_ne_perd_rien() {
    // Régression : `vte` 0.15.0 perdait l'octet « 0 » quand un lot finissait par la moitié d'un accent et que le
    // suivant complétait l'accent, portait un chiffre, puis se terminait par le début d'un autre caractère.
    let flux: &[u8] = &[b'L', 195, 169, b'0', 230, 151, 165];
    let mut bloc = term(40, 3);
    bloc.ecrire(flux);
    assert_eq!(bloc.texte_ligne(0), "Lé0日");
    for k in 1..flux.len() {
        let mut t = term(40, 3);
        for morceau in flux.chunks(k) {
            t.ecrire(morceau);
        }
        assert_eq!(empreinte(&t), empreinte(&bloc), "morceaux de {k}");
    }
    let mut c = term(40, 3);
    c.ecrire(&[b'L', 195]);
    c.ecrire(&[169, b'0', 230]);
    c.ecrire(&[151, 165]);
    assert_eq!(c.texte_ligne(0), "Lé0日");
}
