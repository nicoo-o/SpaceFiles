// Tiroir Historique de la fenêtre : géométrie et zones de clic, sorties de rendu.rs (audit du 14/11/2026, découpage).
// Aucune dépendance à Direct2D hors le type de rectangle : c'est ce qui permet de le tester sans fenêtre.

use super::*;

/// Tiroir Historique (`.hist` de la maquette) : 340 DIP de large, du haut de la zone de contenu au pied (`top:54px; bottom:40px`),
/// marge de 16, entrées de 51 DIP (9 + 33 + 9, filet de 1), bouton « Annuler » de 64 × 26
pub const TIROIR_L: f32 = 340.0;
pub(super) const HIST_PAD: f32 = 16.0;
pub const HIST_RANG: f32 = 51.0;
pub(super) const HIST_ENTETE: f32 = 48.0;
pub(super) const HIST_NOTE_H: f32 = 28.0;
pub(super) const HIST_BOUTON_L: f32 = 64.0;
pub(super) const HIST_BOUTON_H: f32 = 26.0;
pub(super) const HIST_TEMPS_L: f32 = 40.0;

/// Le tiroir sorti (sans décalage) dans une fenêtre de `l` × `h` DIP
pub fn zone_tiroir(l: f32, h: f32) -> D2D_RECT_F {
    rect(l - TIROIR_L, HAUTEUR_TITRE + 1.0, TIROIR_L, (h - statut_h() - HAUTEUR_TITRE - 1.0).max(0.0))
}

/// Le bas de la liste : au-dessus de la marge, et de la note « session » quand elle est là
pub(super) fn hist_bas_liste(zone: &D2D_RECT_F, avec_note: bool) -> f32 {
    zone.bottom - HIST_PAD - if avec_note { HIST_NOTE_H } else { 0.0 }
}

/// Défilement le plus bas du tiroir pour `n` entrées (DIP) ; 0 quand tout tient
pub fn historique_defilement_max(n: usize, zone: &D2D_RECT_F, avec_note: bool) -> f32 {
    let visible = hist_bas_liste(zone, avec_note) - (zone.top + HIST_ENTETE);
    (n as f32 * HIST_RANG - visible).max(0.0)
}

/// Où tombe un clic dans le tiroir
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CibleHistorique {
    /// Le bouton « Annuler » de l'entrée d'identifiant `id`
    Annuler(u64),
    /// Ailleurs dans le tiroir
    Tiroir,
    /// Hors du tiroir (ou tiroir rentré)
    Dehors,
}

/// Zones de clic du tiroir au dernier dessin, décalage du glissement compris
#[derive(Clone, Default)]
pub struct ZonesHistorique {
    pub(super) tiroir: Option<D2D_RECT_F>,
    pub(super) boutons: Vec<(D2D_RECT_F, u64)>,
}

impl ZonesHistorique {
    pub fn cible(&self, x: f32, y: f32) -> CibleHistorique {
        if let Some((_, id)) = self.boutons.iter().find(|(r, _)| dedans(r, x, y)) {
            return CibleHistorique::Annuler(*id);
        }
        match self.tiroir {
            Some(r) if dedans(&r, x, y) => CibleHistorique::Tiroir,
            _ => CibleHistorique::Dehors,
        }
    }
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tiroir_colle_a_droite_sous_le_titre() {
        let z = zone_tiroir(1280.0, 800.0);
        assert_eq!(z.right, 1280.0);
        assert_eq!(z.left, 1280.0 - TIROIR_L);
        assert_eq!(z.top, HAUTEUR_TITRE + 1.0);
        assert_eq!(z.bottom, 800.0 - statut_h());
        // fenêtre minuscule : jamais de hauteur négative
        assert_eq!(zone_tiroir(300.0, 10.0).bottom - zone_tiroir(300.0, 10.0).top, 0.0);
    }

    #[test]
    fn defilement_nul_quand_tout_tient_puis_croit() {
        let z = zone_tiroir(1280.0, 800.0);
        assert_eq!(historique_defilement_max(0, &z, false), 0.0);
        assert_eq!(historique_defilement_max(3, &z, true), 0.0);
        let (a, b) = (historique_defilement_max(100, &z, false), historique_defilement_max(101, &z, false));
        assert!(a > 0.0 && (b - a - HIST_RANG).abs() < 0.01);
        // la note « session » réduit la place visible
        assert!(historique_defilement_max(100, &z, true) > a);
    }

    #[test]
    fn clic_dans_le_tiroir() {
        let zones = ZonesHistorique { tiroir: Some(rect(100.0, 50.0, 340.0, 400.0)), boutons: vec![(rect(120.0, 100.0, 64.0, 26.0), 7)] };
        assert_eq!(zones.cible(130.0, 110.0), CibleHistorique::Annuler(7));
        assert_eq!(zones.cible(300.0, 300.0), CibleHistorique::Tiroir);
        assert_eq!(zones.cible(10.0, 10.0), CibleHistorique::Dehors);
        assert_eq!(ZonesHistorique::default().cible(130.0, 110.0), CibleHistorique::Dehors);
    }
}
