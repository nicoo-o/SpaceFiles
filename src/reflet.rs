// Reflet qui suit le curseur (phase 7, lot C3) : logique pure, sans Direct2D. Un élément cliquable survolé reçoit deux calques,
// un halo radial dans son fond (rayon 120 DIP, 9 % au centre) et une lueur sur son contour (rayon 90 DIP, 55 % au centre).
// Ce fichier décide SI un élément est éclairé et avec quoi (centre, rayons, alphas) ; le dessin est dans `rendu.rs`
// (`Rendu::dessiner_reflet`, un seul pinceau radial réutilisé), la position du pointeur dans `fenetre/procedure.rs`.
// Les couleurs ne sont pas ici : le halo est le blanc des jetons `REFLET_FOND` et `REFLET_TRAIT` de `theme.rs`, qui passent
// par `contraste()` comme tout (encre en thème clair).

use windows::Win32::Graphics::Direct2D::Common::D2D_RECT_F;

/// Rayon du halo de fond (DIP)
pub const RAYON_HALO: f32 = 120.0;
/// Rayon de la lueur du contour (DIP)
pub const RAYON_LUEUR: f32 = 90.0;
/// Alpha au centre du halo (blanc, avant `contraste()`)
pub const ALPHA_HALO: f32 = 0.09;
/// Alpha au centre de la lueur du contour (blanc, avant `contraste()`)
pub const ALPHA_LUEUR: f32 = 0.55;
/// Au-delà de cette taille, pas de reflet : un rayon de 120 DIP sur une grande zone ferait une lampe de poche
pub const LARGEUR_MAX: f32 = 480.0;
pub const HAUTEUR_MAX: f32 = 160.0;

/// Le reflet existe-t-il ? Mouvement réduit : jamais (l'état de survol existant suffit) ; thème Contraste élevé de Windows :
/// jamais non plus (les couleurs viennent du système, un halo translucide les trahirait).
pub fn actif(animations: bool, contraste_windows: bool) -> bool {
    animations && !contraste_windows
}

/// Même question, lue dans l'état global, avec le réglage « Reflet au survol » en plus
pub fn actif_maintenant() -> bool {
    crate::reglages::reflet_voulu() && actif(crate::reglages::animations_actives(), crate::reglages::CONTRASTE_WINDOWS.load(std::sync::atomic::Ordering::Relaxed))
}

/// Un élément est éclairable s'il est actif et tient dans 480 × 160 DIP
pub fn eligible(r: &D2D_RECT_F, desactive: bool) -> bool {
    let (l, h) = (r.right - r.left, r.bottom - r.top);
    !desactive && l > 0.0 && h > 0.0 && l <= LARGEUR_MAX && h <= HAUTEUR_MAX
}

/// Point dans le rectangle, bords compris (un curseur posé pile sur le trait éclaire encore l'élément)
pub fn contient(r: &D2D_RECT_F, x: f32, y: f32) -> bool {
    x >= r.left && x <= r.right && y >= r.top && y <= r.bottom
}

/// Distance du point au contour du rectangle (0 sur le trait, y compris pour un point dedans au bord ; un point dedans est
/// mesuré au bord le plus proche, un point dehors à ce même bord)
pub fn distance_au_bord(r: &D2D_RECT_F, x: f32, y: f32) -> f32 {
    let dx = (r.left - x).max(x - r.right).max(0.0);
    let dy = (r.top - y).max(y - r.bottom).max(0.0);
    if dx > 0.0 || dy > 0.0 {
        return dx.hypot(dy);
    }
    (x - r.left).min(r.right - x).min(y - r.top).min(r.bottom - y)
}

/// Alpha d'un dégradé radial linéaire à la distance `d` du centre : `alpha` au centre, 0 au rayon et au-delà
pub fn alpha_a_distance(d: f32, rayon: f32, alpha: f32) -> f32 {
    if rayon <= 0.0 {
        return 0.0;
    }
    alpha * (1.0 - d / rayon).clamp(0.0, 1.0)
}

/// Ce qu'il faut dessiner pour un élément éclairé (coordonnées de la fenêtre, DIP)
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Plan {
    pub zone: D2D_RECT_F,
    /// Rayon de coin de l'élément : le halo est découpé à ce rectangle arrondi
    pub rayon_coin: f32,
    pub centre: (f32, f32),
    pub rayon_halo: f32,
    pub alpha_halo: f32,
    pub rayon_lueur: f32,
    pub alpha_lueur: f32,
}

impl Plan {
    /// Alpha de la lueur au point du contour le plus proche du curseur : le maximum que le trait recevra
    pub fn alpha_trait_maximal(&self) -> f32 {
        alpha_a_distance(distance_au_bord(&self.zone, self.centre.0, self.centre.1), self.rayon_lueur, self.alpha_lueur)
    }
}

/// Le plan du reflet, ou `None` : reflet coupé, élément désactivé ou trop grand, pointeur inconnu (souris absente, touche
/// pressée) ou hors de l'élément. Le centre est la position du dernier `WM_MOUSEMOVE`, sans lissage.
pub fn planifier(zone: D2D_RECT_F, rayon_coin: f32, desactive: bool, pointeur: Option<(f32, f32)>, actif: bool) -> Option<Plan> {
    if !actif || !eligible(&zone, desactive) {
        return None;
    }
    let (x, y) = pointeur?;
    if !contient(&zone, x, y) {
        return None;
    }
    Some(Plan { zone, rayon_coin, centre: (x, y), rayon_halo: RAYON_HALO, alpha_halo: ALPHA_HALO, rayon_lueur: RAYON_LUEUR, alpha_lueur: ALPHA_LUEUR })
}

/// Position du pointeur imposée pour une capture sans souris : `SPACEFILES_REFLET=x,y` (DIP, coordonnées de la fenêtre).
/// Honorée seulement avec `SPACEFILES_SANS_REGISTRE=1` (mode essai) ; absente en usage normal.
pub fn pointeur_d_essai() -> Option<(f32, f32)> {
    // Lue une fois : appelée à chaque WM_PAINT, l'environnement ne change pas en cours de route
    static VALEUR: std::sync::OnceLock<Option<(f32, f32)>> = std::sync::OnceLock::new();
    *VALEUR.get_or_init(|| {
        if !crate::defaut::sans_registre() {
            return None;
        }
        lire_pointeur(std::env::var("SPACEFILES_REFLET").ok().as_deref())
    })
}

fn lire_pointeur(valeur: Option<&str>) -> Option<(f32, f32)> {
    let (x, y) = valeur?.split_once(',')?;
    let (x, y) = (x.trim().parse::<f32>().ok()?, y.trim().parse::<f32>().ok()?);
    (x.is_finite() && y.is_finite()).then_some((x, y))
}

/// Mesure de coût : `SPACEFILES_MESURE_REFLET=n`, 1 à 500 (n images avec reflet, n sans, puis le résultat au journal). Même condition
/// que le pointeur d'essai.
pub fn mesure_demandee() -> Option<u32> {
    static VALEUR: std::sync::OnceLock<Option<u32>> = std::sync::OnceLock::new();
    *VALEUR.get_or_init(|| {
        if !crate::defaut::sans_registre() {
            return None;
        }
        // Au plus 500 : la mesure bloque l'interface pendant 4 × n images
        std::env::var("SPACEFILES_MESURE_REFLET").ok()?.trim().parse().ok().filter(|n| (1..=500).contains(n))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rendu::rect;

    fn bouton() -> D2D_RECT_F {
        rect(100.0, 100.0, 120.0, 36.0)
    }

    #[test]
    fn bouton_ordinaire_est_eligible() {
        assert!(eligible(&bouton(), false));
    }

    #[test]
    fn element_trop_large_ou_trop_haut_sans_reflet() {
        assert!(!eligible(&rect(0.0, 0.0, 480.5, 36.0), false), "plus de 480 DIP de large");
        assert!(!eligible(&rect(0.0, 0.0, 120.0, 160.5), false), "plus de 160 DIP de haut");
        assert!(eligible(&rect(0.0, 0.0, 480.0, 160.0), false), "480 × 160 pile : éclairé");
        assert!(!eligible(&rect(0.0, 0.0, 800.0, 36.0), false), "ligne de réglages large");
    }

    #[test]
    fn element_desactive_ou_vide_sans_reflet() {
        assert!(!eligible(&bouton(), true));
        assert!(!eligible(&rect(10.0, 10.0, 0.0, 30.0), false));
        assert!(planifier(bouton(), 8.0, true, Some((150.0, 110.0)), true).is_none());
    }

    #[test]
    fn curseur_hors_element_sans_reflet() {
        assert!(planifier(bouton(), 8.0, false, Some((99.0, 110.0)), true).is_none());
        assert!(planifier(bouton(), 8.0, false, Some((150.0, 137.0)), true).is_none());
        assert!(planifier(bouton(), 8.0, false, None, true).is_none(), "pas de pointeur : souris absente ou touche pressée");
    }

    #[test]
    fn curseur_au_bord_eclaire_et_la_lueur_est_au_maximum() {
        let p = planifier(bouton(), 8.0, false, Some((100.0, 118.0)), true).expect("pile sur le bord gauche");
        assert_eq!(distance_au_bord(&p.zone, 100.0, 118.0), 0.0);
        assert!((p.alpha_trait_maximal() - ALPHA_LUEUR).abs() < 1e-6);
        let coin = planifier(bouton(), 8.0, false, Some((220.0, 136.0)), true).expect("pile sur le coin");
        assert!((coin.alpha_trait_maximal() - ALPHA_LUEUR).abs() < 1e-6);
    }

    #[test]
    fn lueur_decroit_avec_la_distance_au_bord() {
        let au_centre = planifier(bouton(), 8.0, false, Some((160.0, 118.0)), true).unwrap();
        // 18 DIP du bord haut et bas, 60 des côtés : le plus proche est à 18
        assert_eq!(distance_au_bord(&au_centre.zone, 160.0, 118.0), 18.0);
        let attendu = ALPHA_LUEUR * (1.0 - 18.0 / RAYON_LUEUR);
        assert!((au_centre.alpha_trait_maximal() - attendu).abs() < 1e-6);
        assert!(au_centre.alpha_trait_maximal() < ALPHA_LUEUR);
        // Au rayon et au-delà : plus rien
        assert_eq!(alpha_a_distance(RAYON_LUEUR, RAYON_LUEUR, ALPHA_LUEUR), 0.0);
        assert_eq!(alpha_a_distance(RAYON_LUEUR * 2.0, RAYON_LUEUR, ALPHA_LUEUR), 0.0);
        assert_eq!(alpha_a_distance(0.0, RAYON_LUEUR, ALPHA_LUEUR), ALPHA_LUEUR);
        assert_eq!(alpha_a_distance(10.0, 0.0, 0.3), 0.0, "rayon nul : pas de division par zéro");
    }

    #[test]
    fn distance_d_un_point_dehors() {
        let r = bouton();
        assert_eq!(distance_au_bord(&r, 90.0, 118.0), 10.0);
        assert_eq!(distance_au_bord(&r, 90.0, 90.0), 10.0f32.hypot(10.0));
    }

    #[test]
    fn mouvement_reduit_ou_contraste_windows_coupent_le_reflet() {
        assert!(actif(true, false));
        assert!(!actif(false, false), "mouvement réduit");
        assert!(!actif(true, true), "Contraste élevé de Windows");
        assert!(!actif(false, true));
        assert!(planifier(bouton(), 8.0, false, Some((150.0, 110.0)), false).is_none());
    }

    #[test]
    fn plan_porte_les_valeurs_de_la_decision() {
        let p = planifier(bouton(), 8.0, false, Some((150.0, 110.0)), true).unwrap();
        assert_eq!((p.rayon_halo, p.alpha_halo, p.rayon_lueur, p.alpha_lueur), (120.0, 0.09, 90.0, 0.55));
        assert_eq!(p.centre, (150.0, 110.0));
        assert_eq!(p.rayon_coin, 8.0);
    }

    #[test]
    fn alphas_selon_le_theme() {
        use crate::theme::{rgba, transformer};
        let halo = rgba(255, 255, 255, ALPHA_HALO);
        let sombre = transformer(&halo, false, false, false);
        let clair = transformer(&halo, true, false, false);
        assert_eq!(sombre.a, ALPHA_HALO, "sombre : blanc inchangé");
        assert!(sombre.r > 0.99);
        assert!(clair.r < 0.1 && clair.a < sombre.a, "clair : de l'encre, un peu plus faible ({} contre {})", clair.a, sombre.a);
        let lueur = rgba(255, 255, 255, ALPHA_LUEUR);
        assert!(transformer(&lueur, true, false, false).r < 0.1, "la lueur claire est de l'encre aussi");
        assert!(transformer(&lueur, true, false, false).a > 0.0);
    }

    #[test]
    fn lecture_de_la_position_d_essai() {
        assert_eq!(lire_pointeur(Some("300, 120.5")), Some((300.0, 120.5)));
        assert_eq!(lire_pointeur(Some("300")), None);
        assert_eq!(lire_pointeur(Some("a,b")), None);
        assert_eq!(lire_pointeur(Some("nan,1")), None);
        assert_eq!(lire_pointeur(None), None);
    }

    /// Pire cas du reflet sur un texte : le halo au maximum sous le texte secondaire ne fait jamais passer le rapport sous 4,5:1.
    /// Même calcul que la ligne de l'audit (`theme::tableau_contrastes`).
    #[test]
    fn texte_secondaire_sous_halo_maximal_reste_lisible() {
        for (clair, renforce) in [(false, false), (true, false), (false, true), (true, true)] {
            let r = crate::theme::tests::rapport_sous_halo(clair, renforce);
            assert!(r >= 4.5, "clair={clair} renforcé={renforce} : {r:.2}:1 < 4,5:1");
        }
    }
}
