// T3 : la souris vers les programmes plein écran (vim, less, htop, mc). Un programme qui demande le suivi de la souris
// (`ESC[?1000h` et suivants) reçoit des séquences à la place de la sélection de texte ; sans suivi demandé, la molette sur
// l'écran alterné devient des flèches (comme Windows Terminal), pour que `less` et `more` défilent. Rien de Win32 ici : le
// panneau appelle ces fonctions, la fenêtre ne fait que lui donner les cases et les boutons.

use super::vt::Modes;

/// Bouton de la souris tel que le décrit le protocole xterm (0 = gauche, 1 = milieu, 2 = droit). Seul le gauche est
/// branché sur la fenêtre pour l'instant (le clic droit et le clic milieu y gardent leur rôle) ; l'encodeur les connaît.
#[allow(dead_code)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Bouton {
    Gauche,
    Milieu,
    Droit,
}

impl Bouton {
    fn code(self) -> u32 {
        match self {
            Bouton::Gauche => 0,
            Bouton::Milieu => 1,
            Bouton::Droit => 2,
        }
    }
}

/// Ce qui s'est passé sous la souris
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Evenement {
    Appui(Bouton),
    Relache(Bouton),
    /// Déplacement, avec le bouton tenu s'il y en a un
    Mouvement(Option<Bouton>),
    /// Un cran de molette : `true` = vers le haut (vers l'historique)
    Molette(bool),
}

/// Le programme du terminal a demandé à recevoir la souris
pub fn suivi_demande(m: &Modes) -> bool {
    m.souris_appui || m.souris_glisse || m.souris_mouvement
}

/// Octets à envoyer au programme pour cet événement, ou `None` s'il ne les a pas demandés (mode `?1000` : appuis, relâchements
/// et molette ; `?1002` : plus les glissements bouton tenu ; `?1003` : plus tous les mouvements). `col` et `rangee` partent de 0.
/// Sans le codage SGR (`?1006`), le protocole ancien tient chaque coordonnée dans un octet : au-delà de la colonne ou de la
/// rangée 222, rien n'est envoyé (mieux que des octets faux).
pub fn encoder(m: &Modes, e: Evenement, col: usize, rangee: usize, ctrl: bool, alt: bool) -> Option<Vec<u8>> {
    if !suivi_demande(m) {
        return None;
    }
    let mut cb = match e {
        Evenement::Appui(b) | Evenement::Relache(b) => b.code(),
        Evenement::Mouvement(b) => {
            let tenu = b.is_some();
            // Un mouvement sans bouton n'est suivi qu'en `?1003` ; bouton tenu, dès `?1002`
            if !(m.souris_mouvement || (tenu && m.souris_glisse)) {
                return None;
            }
            32 + b.map_or(3, Bouton::code)
        }
        Evenement::Molette(haut) => {
            if haut {
                64
            } else {
                65
            }
        }
    };
    if alt {
        cb += 8;
    }
    if ctrl {
        cb += 16;
    }
    let (x, y) = (col + 1, rangee + 1);
    if m.souris_sgr {
        let fin = if matches!(e, Evenement::Relache(_)) { 'm' } else { 'M' };
        return Some(format!("\x1b[<{cb};{x};{y}{fin}").into_bytes());
    }
    // Protocole ancien : un relâchement n'a pas de bouton (3), les coordonnées sont décalées de 32 dans un seul octet
    if matches!(e, Evenement::Relache(_)) {
        cb = (cb & !3) | 3;
    }
    if x > 222 || y > 222 {
        return None;
    }
    Some(vec![0x1b, b'[', b'M', (32 + cb) as u8, (32 + x) as u8, (32 + y) as u8])
}

/// Flèches que la molette envoie sur l'écran alterné quand le programme n'a pas demandé la souris : `crans` > 0 monte.
/// Trois flèches par cran, dans la forme que le programme a demandée (`ESC O A` en mode application, sinon `ESC [ A`).
pub fn fleches_de_molette(m: &Modes, crans: isize) -> Vec<u8> {
    let lettre = if crans > 0 { b'A' } else { b'B' };
    let intro: &[u8] = if m.touches_curseur_appli { b"\x1bO" } else { b"\x1b[" };
    let n = crans.unsigned_abs().saturating_mul(3).min(60);
    let mut sortie = Vec::with_capacity(n * 3);
    for _ in 0..n {
        sortie.extend_from_slice(intro);
        sortie.push(lettre);
    }
    sortie
}

#[cfg(test)]
mod tests {
    use super::*;

    fn modes(appui: bool, glisse: bool, mouvement: bool, sgr: bool) -> Modes {
        Modes { souris_appui: appui, souris_glisse: glisse, souris_mouvement: mouvement, souris_sgr: sgr, ..Modes::default() }
    }

    #[test]
    fn sans_suivi_demande_rien_ne_part() {
        let m = Modes::default();
        assert!(!suivi_demande(&m));
        assert_eq!(encoder(&m, Evenement::Appui(Bouton::Gauche), 3, 4, false, false), None);
        assert_eq!(encoder(&m, Evenement::Molette(true), 3, 4, false, false), None);
    }

    #[test]
    fn sgr_appui_et_relachement() {
        let m = modes(true, false, false, true);
        assert_eq!(encoder(&m, Evenement::Appui(Bouton::Gauche), 0, 0, false, false).unwrap(), b"\x1b[<0;1;1M");
        assert_eq!(encoder(&m, Evenement::Relache(Bouton::Gauche), 9, 4, false, false).unwrap(), b"\x1b[<0;10;5m");
        assert_eq!(encoder(&m, Evenement::Appui(Bouton::Droit), 2, 2, false, false).unwrap(), b"\x1b[<2;3;3M");
    }

    #[test]
    fn sgr_n_a_pas_de_limite_de_coordonnee() {
        let m = modes(true, false, false, true);
        assert_eq!(encoder(&m, Evenement::Appui(Bouton::Gauche), 499, 299, false, false).unwrap(), b"\x1b[<0;500;300M");
    }

    #[test]
    fn modificateurs_ctrl_et_alt() {
        let m = modes(true, false, false, true);
        assert_eq!(encoder(&m, Evenement::Appui(Bouton::Gauche), 0, 0, true, false).unwrap(), b"\x1b[<16;1;1M");
        assert_eq!(encoder(&m, Evenement::Appui(Bouton::Gauche), 0, 0, false, true).unwrap(), b"\x1b[<8;1;1M");
    }

    #[test]
    fn molette_haut_et_bas() {
        let m = modes(true, false, false, true);
        assert_eq!(encoder(&m, Evenement::Molette(true), 1, 1, false, false).unwrap(), b"\x1b[<64;2;2M");
        assert_eq!(encoder(&m, Evenement::Molette(false), 1, 1, false, false).unwrap(), b"\x1b[<65;2;2M");
    }

    #[test]
    fn mouvements_selon_le_mode() {
        let appui = modes(true, false, false, true);
        let glisse = modes(false, true, false, true);
        let tout = modes(false, false, true, true);
        let g = Evenement::Mouvement(Some(Bouton::Gauche));
        let libre = Evenement::Mouvement(None);
        // ?1000 : jamais de mouvement
        assert_eq!(encoder(&appui, g, 1, 1, false, false), None);
        // ?1002 : seulement bouton tenu
        assert_eq!(encoder(&glisse, g, 1, 1, false, false).unwrap(), b"\x1b[<32;2;2M");
        assert_eq!(encoder(&glisse, libre, 1, 1, false, false), None);
        // ?1003 : tout, un mouvement libre porte le code 35
        assert_eq!(encoder(&tout, libre, 1, 1, false, false).unwrap(), b"\x1b[<35;2;2M");
        assert_eq!(encoder(&tout, g, 1, 1, false, false).unwrap(), b"\x1b[<32;2;2M");
    }

    #[test]
    fn protocole_ancien_decale_de_32_et_relache_sans_bouton() {
        let m = modes(true, false, false, false);
        assert_eq!(encoder(&m, Evenement::Appui(Bouton::Gauche), 0, 0, false, false).unwrap(), vec![0x1b, b'[', b'M', 32, 33, 33]);
        // Relâchement : bouton 3, quel que soit le bouton lâché
        assert_eq!(encoder(&m, Evenement::Relache(Bouton::Droit), 4, 9, false, false).unwrap(), vec![0x1b, b'[', b'M', 35, 37, 42]);
    }

    #[test]
    fn protocole_ancien_refuse_les_grandes_coordonnees() {
        let m = modes(true, false, false, false);
        assert!(encoder(&m, Evenement::Appui(Bouton::Gauche), 221, 0, false, false).is_some());
        assert_eq!(encoder(&m, Evenement::Appui(Bouton::Gauche), 222, 0, false, false), None);
        assert_eq!(encoder(&m, Evenement::Appui(Bouton::Gauche), 0, 300, false, false), None);
    }

    #[test]
    fn fleches_de_molette_normales_et_application() {
        let normal = Modes::default();
        assert_eq!(fleches_de_molette(&normal, 1), b"\x1b[A\x1b[A\x1b[A");
        assert_eq!(fleches_de_molette(&normal, -1), b"\x1b[B\x1b[B\x1b[B");
        let appli = Modes { touches_curseur_appli: true, ..Modes::default() };
        assert_eq!(fleches_de_molette(&appli, 1), b"\x1bOA\x1bOA\x1bOA");
    }

    #[test]
    fn fleches_de_molette_bornees() {
        let m = Modes::default();
        // Un pavé tactile qui envoie un énorme cran ne doit pas inonder le programme
        assert_eq!(fleches_de_molette(&m, 1_000_000).len(), 60 * 3);
        assert!(fleches_de_molette(&m, 0).is_empty() || fleches_de_molette(&m, 0) == b"");
    }
}
