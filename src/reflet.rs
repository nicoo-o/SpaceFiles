// Reflet qui suit le curseur (phase 7, lot C3) : logique pure, sans Direct2D. Un élément cliquable survolé reçoit deux calques,
// un halo radial dans son fond (rayon 120 DIP, 9 % au centre) et une arête claire en haut (rayon 90 DIP, 30 % au centre ;
// lot 2 de l'audit UX : elle remplace la lueur de tout le contour à 55 %).
// Ce fichier décide SI un élément est éclairé et avec quoi (centre, rayons, alphas) ; le dessin est dans `rendu.rs`
// (`Rendu::peindre_reflet`, appelé sous le texte de chaque élément par `reflet_sous`, sinon en fin d'image par `dessiner_reflet` ;
// un seul pinceau radial réutilisé), la position du pointeur dans `fenetre/procedure.rs`.
// Les couleurs ne sont pas ici : le halo est le blanc des jetons `REFLET_FOND` et `REFLET_TRAIT` de `theme.rs`, qui passent
// par `contraste()` comme tout (encre en thème clair).

use windows::Win32::Graphics::Direct2D::Common::D2D_RECT_F;

/// Rayon du halo de fond (DIP)
pub const RAYON_HALO: f32 = 120.0;
/// Rayon de la lueur de l'arête haute (DIP)
pub const RAYON_LUEUR: f32 = 90.0;
/// Alpha au centre du halo (blanc, avant `contraste()`)
pub const ALPHA_HALO: f32 = 0.09;
/// Alpha de l'arête haute au plus près du curseur (blanc, avant `contraste()` ; en thème clair, l'encre à 0,18 : `theme::alpha_encre`).
/// Lot 2 de l'audit UX : la lueur de 1 px sur tout le contour (0,55) se lisait comme un trait isolé ; une arête claire en haut,
/// plus douce, donne le bord de verre sans cerner l'élément.
pub const ALPHA_LUEUR: f32 = 0.30;
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
/// mesuré au bord le plus proche, un point dehors à ce même bord). Plus utilisée au dessin depuis que l'arête ne suit que le
/// bord haut (`Plan::alpha_trait_maximal`) : gardée pour les tests, qui comparent l'arête au contour complet d'avant.
#[cfg(test)]
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
    /// Soulèvement vertical du fond au survol (DIP, 0 par défaut) : `zone` reste la zone au repos, qui identifie le reflet d'une
    /// image à l'autre ; seul le dessin applique le décalage (`decale`)
    pub decalage: f32,
    /// Élément éclairé, quand son dessin peut peindre lui-même le reflet entre son fond et son texte (`reflet_sous`) ; `None` :
    /// le reflet est peint par-dessus l'image entière, en dernière passe
    pub cible: Option<crate::app::Cible>,
}

impl Plan {
    /// Le plan tel qu'il se dessine : la zone suit le soulèvement du fond
    pub fn decale(&self) -> Plan {
        Plan { zone: D2D_RECT_F { top: self.zone.top + self.decalage, bottom: self.zone.bottom + self.decalage, ..self.zone }, decalage: 0.0, ..*self }
    }

    /// Alpha de l'arête haute au point de ce bord le plus proche du curseur (`centre` est dans la zone : sa distance au bord
    /// haut est son ordonnée relative) : le maximum que l'arête recevra
    pub fn alpha_trait_maximal(&self) -> f32 {
        alpha_a_distance((self.centre.1 - self.zone.top).max(0.0), self.rayon_lueur, self.alpha_lueur)
    }

    /// Bande du haut où l'arête est tracée : la zone sur toute sa largeur, haute du rayon de coin (le trait de 1 px et ses deux
    /// arcs de coin, jamais les côtés), bornée à la moitié de la hauteur et à 1 DIP au moins. Toujours dans la zone.
    pub fn bande_arete(&self) -> D2D_RECT_F {
        let haut = self.zone.bottom - self.zone.top;
        let h = self.rayon_coin.min(haut / 2.0).max(1.0).min(haut);
        D2D_RECT_F { left: self.zone.left, top: self.zone.top, right: self.zone.right, bottom: self.zone.top + h }
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
    Some(Plan { zone, rayon_coin, centre: (x, y), rayon_halo: RAYON_HALO, alpha_halo: ALPHA_HALO, rayon_lueur: RAYON_LUEUR, alpha_lueur: ALPHA_LUEUR, decalage: 0.0, cible: None })
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
        // L'arête est en haut : à 18 DIP du bord haut, elle n'est pas au maximum même pile sur le bord gauche
        assert!(p.alpha_trait_maximal() < ALPHA_LUEUR);
        let haut = planifier(bouton(), 8.0, false, Some((150.0, 100.0)), true).expect("pile sur le bord haut");
        assert!((haut.alpha_trait_maximal() - ALPHA_LUEUR).abs() < 1e-6);
        let coin = planifier(bouton(), 8.0, false, Some((220.0, 100.0)), true).expect("pile sur le coin haut");
        assert!((coin.alpha_trait_maximal() - ALPHA_LUEUR).abs() < 1e-6);
    }

    #[test]
    fn la_bande_de_l_arete_reste_dans_la_zone_et_suit_le_rayon_de_coin() {
        for (h, coin) in [(36.0, 12.0), (36.0, 8.0), (20.0, 12.0), (4.0, 16.0), (160.0, 16.0)] {
            let z = rect(100.0, 100.0, 120.0, h);
            let p = planifier(z, coin, false, Some((150.0, 100.0 + h / 2.0)), true).unwrap();
            let b = p.bande_arete();
            assert_eq!((b.left, b.right, b.top), (z.left, z.right, z.top), "pleine largeur, collée au bord haut");
            assert!(b.bottom <= z.bottom && b.bottom > b.top, "dans la zone, jamais vide : {b:?}");
            assert!(b.bottom - b.top <= coin.max(1.0) + 1e-6, "pas plus haute que le rayon de coin : {b:?}");
            assert!(b.bottom - b.top <= h / 2.0 + 1e-6 || h < 2.0, "pas plus que la moitié de la zone : {b:?}");
        }
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
        assert_eq!((p.rayon_halo, p.alpha_halo, p.rayon_lueur, p.alpha_lueur), (120.0, 0.09, 90.0, 0.30));
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
        let (arete_sombre, arete_claire) = (transformer(&lueur, false, false, false), transformer(&lueur, true, false, false));
        assert!(arete_claire.r < 0.1, "l'arête claire est de l'encre aussi");
        // Alphas séparés : en clair l'encre est plus douce que le blanc du sombre (0,18 contre 0,30)
        assert!((arete_sombre.a - ALPHA_LUEUR).abs() < 1e-6 && (arete_claire.a - 0.18).abs() < 1e-6, "sombre {} / clair {}", arete_sombre.a, arete_claire.a);
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
