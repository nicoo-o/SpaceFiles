// Pixel : les deux yeux de la barre d'état. Logique pure (états, priorité, période du jour, formes en DIP), sans Direct2D ni
// état global : le dessin est dans `rendu.rs`, l'horloge et les minuteries sont dans `fenetre/` et `minuteries/`.
// Décision de l'auteur : deux barres verticales arrondies, jamais un visage plus détaillé ; chaque état se reconnaît à sa
// FORME (la couleur ne porte jamais seule un état).

/// Ce que dit Pixel de l'application
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EtatPixel {
    /// Deux barres droites, un clignement rare
    Repos,
    /// Une opération tourne : les barres glissent doucement de gauche à droite
    Activite,
    /// Un conflit attend une réponse : barres plus hautes, immobiles
    Attention,
    /// Opération finie (1,5 s) : deux chevrons « ^ ^ »
    Reussite,
    /// Échec : deux croix
    Erreur,
    /// Nuit : les deux barres écrasées à 14 % de leur hauteur, yeux fermés
    Nuit,
}

/// Période de l'heure locale (comme SpaceNotch)
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Periode {
    /// 6 h à 18 h
    Jour,
    /// 18 h à 22 h
    Soir,
    /// 22 h à 6 h
    Nuit,
}

/// Teinte à peindre : le rendu la traduit en jeton de `theme.rs` (cyan de Pixel, accent, blancs, erreur)
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Teinte {
    Cyan,
    Ambre,
    Doux,
    Faible,
    Erreur,
}

/// Durée pendant laquelle Pixel garde les chevrons après une opération finie (ms)
pub const REUSSITE_MS: u128 = 1500;
/// Clignement (`blink` de la maquette, 9 s) : les yeux se ferment de 93 % à 96 % du cycle (270 ms) et se rouvrent de 96 % à
/// 100 % (360 ms), chaque tronçon avec la courbe `ease` du CSS. La minuterie de 9 s (armée seulement fenêtre visible, active et
/// animée) lance ces 630 ms ; le reste du cycle, les yeux sont ouverts et rien ne tourne.
pub const CLIGNEMENT_PERIODE_MS: u32 = 9000;
pub const CLIGNEMENT_FERME_MS: u64 = 270;
pub const CLIGNEMENT_OUVRE_MS: u64 = 360;
pub const CLIGNEMENT_MS: u64 = CLIGNEMENT_FERME_MS + CLIGNEMENT_OUVRE_MS;
/// Clignement de la palette (`peb` .28 s, 120 ms après une frappe) : les yeux se ferment à mi-course (50 %, `scaleY(.1)`) et se rouvrent
pub const CLIGNEMENT_PALETTE_MS: u64 = 280;
/// Délai entre la dernière frappe et le clignement de la palette (`setTimeout(palBlink, 120)`)
pub const CLIGNEMENT_PALETTE_DELAI_MS: u32 = 120;
/// Hauteur des barres des yeux « contents » (`.pe.hp i`, 5 px) : la palette lance une commande
pub const CONTENTS_H: f32 = 5.0;
/// Échelle verticale des yeux à la nuit (`scaleY(.14)`) et au creux du clignement (`scaleY(.1)`)
pub const NUIT_ECHELLE: f32 = 0.14;
pub const CLIGNEMENT_ECHELLE: f32 = 0.1;
/// Amplitude maximale du regard (DIP) : horizontale, verticale (la verticale est celle de la palette, ±4)
pub const REGARD_X_MAX: f32 = 3.0;
pub const REGARD_Y_MAX: f32 = 4.0;
/// Distance du pointeur (DIP) à partir de laquelle le regard atteint son amplitude maximale
pub const REGARD_PORTEE: f32 = 160.0;
/// Pas de quantification du regard (DIP) : un pointeur qui bouge peu ne relance ni repeinte ni animation
pub const REGARD_PAS: f32 = 0.5;
/// Période du glissement de l'activité (ms) : un aller-retour
pub const GLISSEMENT_PERIODE_MS: u128 = 1600;
/// Amplitude du glissement (DIP)
pub const GLISSEMENT: f32 = 3.0;
/// Largeur de la boîte de Pixel (DIP) ; sa hauteur est celle de ses barres (14, 18 en attention)
pub const LARGEUR: f32 = 26.0;

const BARRE_L: f32 = 8.0;
const BARRE_H: f32 = 14.0;
/// Barres du mode attention
const BARRE_H_HAUTE: f32 = 18.0;
/// Centre de chaque barre : écart de 9 DIP entre elles
const CENTRE: f32 = (BARRE_L + 9.0) / 2.0;

/// Période du jour à cette heure locale (0 à 23). Une heure hors de 0..24 est ramenée dans la journée.
pub fn periode_du_jour(heure: u32) -> Periode {
    match heure % 24 {
        6..=17 => Periode::Jour,
        18..=21 => Periode::Soir,
        _ => Periode::Nuit,
    }
}

/// Réglage « Pixel selon l'heure » : l'heure locale (Auto) ou une période forcée
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum ChoixPixel {
    #[default]
    Auto,
    Jour,
    Soir,
    Nuit,
}

impl ChoixPixel {
    /// Ordre des segments à l'écran et des rangs de `depuis_rang` / `rang`
    pub const TOUS: [ChoixPixel; 4] = [ChoixPixel::Auto, ChoixPixel::Jour, ChoixPixel::Soir, ChoixPixel::Nuit];

    pub fn libelle(self) -> &'static str {
        match self {
            ChoixPixel::Auto => "Auto",
            ChoixPixel::Jour => "Jour",
            ChoixPixel::Soir => "Soir",
            ChoixPixel::Nuit => "Nuit",
        }
    }

    /// Valeur écrite dans reglages.toml
    pub fn cle(self) -> &'static str {
        match self {
            ChoixPixel::Auto => "auto",
            ChoixPixel::Jour => "jour",
            ChoixPixel::Soir => "soir",
            ChoixPixel::Nuit => "nuit",
        }
    }

    /// Clé inconnue : Auto, le comportement d'avant le réglage
    pub fn depuis_cle(t: &str) -> ChoixPixel {
        Self::TOUS.into_iter().find(|c| c.cle() == t).unwrap_or_default()
    }

    pub fn rang(self) -> usize {
        Self::TOUS.iter().position(|&c| c == self).unwrap_or(0)
    }

    pub fn depuis_rang(r: usize) -> ChoixPixel {
        Self::TOUS[r.min(Self::TOUS.len() - 1)]
    }
}

/// Période que Pixel affiche : celle de l'heure locale en Auto, sinon la période choisie
pub fn periode_effective(choix: ChoixPixel, heure: u32) -> Periode {
    match choix {
        ChoixPixel::Auto => periode_du_jour(heure),
        ChoixPixel::Jour => Periode::Jour,
        ChoixPixel::Soir => Periode::Soir,
        ChoixPixel::Nuit => Periode::Nuit,
    }
}

/// État de Pixel. Priorité : erreur > conflit > nuit (yeux fermés) > activité > réussite > repos. La nuit ferme les yeux
/// pour tout, sauf pour une erreur et un conflit : ceux-là doivent se voir quelle que soit l'heure.
pub fn etat_pixel(activite: bool, conflit: bool, erreur: bool, reussite_recente: bool, nuit: bool) -> EtatPixel {
    if erreur {
        EtatPixel::Erreur
    } else if conflit {
        EtatPixel::Attention
    } else if nuit {
        EtatPixel::Nuit
    } else if activite {
        EtatPixel::Activite
    } else if reussite_recente {
        EtatPixel::Reussite
    } else {
        EtatPixel::Repos
    }
}

/// Teinte d'un état à une période. Erreur : toujours la couleur d'erreur. Nuit : yeux fermés en blanc faible. Un état qui garde
/// les yeux ouverts la nuit (conflit) passe en blanc doux : le cyan et l'ambre sont des couleurs de jour et de soir.
pub fn teinte(etat: EtatPixel, periode: Periode) -> Teinte {
    match (etat, periode) {
        (EtatPixel::Erreur, _) => Teinte::Erreur,
        (EtatPixel::Nuit, _) => Teinte::Faible,
        (_, Periode::Jour) => Teinte::Cyan,
        (_, Periode::Soir) => Teinte::Ambre,
        (_, Periode::Nuit) => Teinte::Doux,
    }
}

/// Un élément à tracer, en DIP, par rapport au centre de la boîte de Pixel (x vers la droite, y vers le bas)
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Element {
    /// Rectangle plein aux angles arrondis, centré en (cx, cy)
    Barre { cx: f32, cy: f32, l: f32, h: f32, rayon: f32 },
    /// Trait de bout rond (une jambe de chevron, une branche de croix, un œil fermé)
    Trait { x1: f32, y1: f32, x2: f32, y2: f32 },
}

#[derive(Clone, PartialEq, Debug)]
pub struct Forme {
    pub elements: Vec<Element>,
    /// Épaisseur des traits (DIP)
    pub epaisseur: f32,
}

impl Forme {
    /// La même forme décalée (regard vers une ligne de la palette)
    pub fn decalee(mut self, dx: f32, dy: f32) -> Forme {
        for e in &mut self.elements {
            match e {
                Element::Barre { cx, cy, .. } => {
                    *cx += dx;
                    *cy += dy;
                }
                Element::Trait { x1, y1, x2, y2 } => {
                    *x1 += dx;
                    *x2 += dx;
                    *y1 += dy;
                    *y2 += dy;
                }
            }
        }
        self
    }

    /// Les barres écrasées verticalement : `k` = 0 inchangées, `k` = 1 à l'échelle de la nuit (`scaleY(.14)`). Les traits (chevrons,
    /// croix) ne changent pas : seuls des yeux ouverts se ferment.
    pub fn aplatie(mut self, k: f32) -> Forme {
        let echelle = 1.0 - (1.0 - NUIT_ECHELLE) * k.clamp(0.0, 1.0);
        for e in &mut self.elements {
            if let Element::Barre { h, rayon, .. } = e {
                *h *= echelle;
                *rayon = rayon.min(*h / 2.0);
            }
        }
        self
    }
}

impl Forme {
    /// Les barres ramenées vers `CONTENTS_H` : `k` = 0 inchangées, `k` = 1 à 5 DIP de haut (yeux contents de la palette). Les traits
    /// (chevrons, croix) ne changent pas.
    pub fn contente(mut self, k: f32) -> Forme {
        let k = k.clamp(0.0, 1.0);
        for e in &mut self.elements {
            if let Element::Barre { h, rayon, .. } = e {
                *h += (CONTENTS_H - *h) * k;
                *rayon = rayon.min(*h / 2.0);
            }
        }
        self
    }
}

/// Fermeture des yeux (0 = ouverts, 1 = creux) `ecoule_ms` après le début du clignement de la palette (`peb`, 280 ms : creux à
/// 140 ms, puis réouverture) ; 0 au-delà
pub fn phase_clignement_palette(ecoule_ms: u64) -> f32 {
    if ecoule_ms >= CLIGNEMENT_PALETTE_MS {
        return 0.0;
    }
    let demi = CLIGNEMENT_PALETTE_MS / 2;
    if ecoule_ms < demi {
        crate::theme::ease(ecoule_ms as f32 / demi as f32)
    } else {
        1.0 - crate::theme::ease((ecoule_ms - demi) as f32 / demi as f32)
    }
}

/// Fermeture des yeux (0 = ouverts, 1 = creux du clignement) `ecoule_ms` après le début d'un clignement ; 0 hors des 630 ms
pub fn phase_clignement(ecoule_ms: u64) -> f32 {
    if ecoule_ms >= CLIGNEMENT_MS {
        return 0.0;
    }
    if ecoule_ms < CLIGNEMENT_FERME_MS {
        crate::theme::ease(ecoule_ms as f32 / CLIGNEMENT_FERME_MS as f32)
    } else {
        1.0 - crate::theme::ease((ecoule_ms - CLIGNEMENT_FERME_MS) as f32 / CLIGNEMENT_OUVRE_MS as f32)
    }
}

/// Décalage du regard (DIP) vers le pointeur. Les yeux visent la direction du pointeur depuis le centre de Pixel ; l'amplitude
/// monte avec la distance jusqu'à `REGARD_PORTEE`, puis reste au maximum (3 DIP en x, 4 en y). Sans pointeur, ou quand les
/// animations sont coupées : droit devant (0, 0). Le résultat est arrondi au pas de `REGARD_PAS`.
/// Note : la maquette ne fait suivre le pointeur à aucun œil de la fenêtre (seul `palEyes` règle `--ly`, pour la palette) ; ce calcul
/// est celui du brief du lot 13, sans formule de référence.
pub fn regard_pointeur(pointeur: Option<(f32, f32)>, centre: (f32, f32), actif: bool) -> (f32, f32) {
    let Some((px, py)) = pointeur.filter(|_| actif) else { return (0.0, 0.0) };
    let (dx, dy) = (px - centre.0, py - centre.1);
    let d = dx.hypot(dy);
    if !d.is_finite() || d < 0.5 {
        return (0.0, 0.0);
    }
    let k = (d / REGARD_PORTEE).min(1.0);
    let q = |v: f32| (v / REGARD_PAS).round() * REGARD_PAS;
    (q(dx / d * k * REGARD_X_MAX), q(dy / d * k * REGARD_Y_MAX))
}

/// Forme d'un état. `phase` : pour `Activite`, la position dans le cycle de glissement (0 à 1, sinus) ; pour `Repos`, la
/// fermeture de l'œil (0 = ouvert, 1 = clignement complet, l'œil réduit à un dixième de sa hauteur) ; ignorée ailleurs.
/// Mouvement réduit : l'appelant passe 0, la pose est fixe.
pub fn forme(etat: EtatPixel, phase: f32) -> Forme {
    let deux = |f: &dyn Fn(f32) -> Vec<Element>| [-CENTRE, CENTRE].into_iter().flat_map(f).collect::<Vec<_>>();
    let barre = |h: f32, dx: f32| deux(&|c| vec![Element::Barre { cx: c + dx, cy: 0.0, l: BARRE_L, h, rayon: 2.0 }]);
    let elements = match etat {
        EtatPixel::Repos => barre((BARRE_H * (1.0 - (1.0 - CLIGNEMENT_ECHELLE) * phase.clamp(0.0, 1.0))).max(1.5), 0.0),
        EtatPixel::Activite => barre(BARRE_H, GLISSEMENT * (phase * std::f32::consts::TAU).sin()),
        EtatPixel::Attention => barre(BARRE_H_HAUTE, 0.0),
        EtatPixel::Reussite => deux(&|c| {
            vec![Element::Trait { x1: c - 4.0, y1: 3.0, x2: c, y2: -4.0 }, Element::Trait { x1: c, y1: -4.0, x2: c + 4.0, y2: 3.0 }]
        }),
        EtatPixel::Erreur => deux(&|c| {
            vec![Element::Trait { x1: c - 3.5, y1: -4.5, x2: c + 3.5, y2: 4.5 }, Element::Trait { x1: c - 3.5, y1: 4.5, x2: c + 3.5, y2: -4.5 }]
        }),
        // Yeux fermés : les barres de repos à `scaleY(.14)` (`.win[data-px=nuit] .pe i`), pas un trait d'une autre forme
        EtatPixel::Nuit => return forme(EtatPixel::Repos, 0.0).aplatie(1.0),
    };
    Forme { elements, epaisseur: 2.0 }
}

/// Forme à peindre : celle de l'état, avec les yeux de `aplatie` (0 à 1) fermés par la nuit. La nuit part des barres de repos et les
/// écrase peu à peu (`transition: height .25s`) ; en sortie de nuit, `aplatie` redescend vers 0 sur la forme du nouvel état.
pub fn forme_affichee(etat: EtatPixel, phase: f32, aplatie: f32) -> Forme {
    let base = if etat == EtatPixel::Nuit { forme(EtatPixel::Repos, 0.0) } else { forme(etat, phase) };
    base.aplatie(aplatie)
}

/// Phase du glissement à `ecoule_ms` depuis le début de l'activité (0 à 1)
pub fn phase_glissement(ecoule_ms: u128) -> f32 {
    (ecoule_ms % GLISSEMENT_PERIODE_MS) as f32 / GLISSEMENT_PERIODE_MS as f32
}

/// Décalage vertical du regard dans la palette (`palEyes(Math.round(pi / (n - 1) * 8 - 4))`) : ligne `rang` sur `total`, de -4 DIP
/// (première) à +4 DIP (dernière), arrondi au DIP entier. Une seule ligne, ou aucune : droit devant. Le regard fait donc 8 DIP
/// d'amplitude ; la maquette le centre sur 0 (le brief du lot 15A disait « de 0 à 8 px » : la maquette fait foi).
pub fn regard(rang: usize, total: usize) -> f32 {
    if total < 2 {
        return 0.0;
    }
    ((rang.min(total - 1) as f32 / (total - 1) as f32) * 8.0 - 4.0).round()
}

/// Texte de l'état vide de la liste (`#emptyTxt` de la maquette) : « Aucun résultat pour « requête ». » quand un filtre est tapé,
/// sinon « Dossier vide. Glisse des fichiers ici. ». Une requête faite d'espaces ne compte pas.
pub fn texte_vide(requete: &str) -> String {
    let q = requete.trim();
    if q.is_empty() {
        "Dossier vide. Glisse des fichiers ici.".to_string()
    } else {
        format!("Aucun résultat pour « {q} ».")
    }
}

/// Yeux de l'état vide (`eyeSvg('open')`) : (teinte, échelle verticale). La nuit écrase les yeux à 14 % comme ceux de la barre d'état.
pub fn yeux_vides(periode: Periode) -> (Teinte, f32) {
    match periode {
        Periode::Nuit => (teinte(EtatPixel::Nuit, periode), NUIT_ECHELLE),
        _ => (teinte(EtatPixel::Repos, periode), 1.0),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn periode_aux_heures_limites() {
        assert_eq!(periode_du_jour(5), Periode::Nuit);
        assert_eq!(periode_du_jour(6), Periode::Jour);
        assert_eq!(periode_du_jour(17), Periode::Jour);
        assert_eq!(periode_du_jour(18), Periode::Soir);
        assert_eq!(periode_du_jour(21), Periode::Soir);
        assert_eq!(periode_du_jour(22), Periode::Nuit);
        assert_eq!(periode_du_jour(0), Periode::Nuit);
        assert_eq!(periode_du_jour(23), Periode::Nuit);
    }

    #[test]
    fn periode_forcee_ou_auto() {
        for h in [0, 5, 6, 12, 17, 18, 21, 22, 23] {
            assert_eq!(periode_effective(ChoixPixel::Auto, h), periode_du_jour(h), "auto à {h} h");
            assert_eq!(periode_effective(ChoixPixel::Jour, h), Periode::Jour, "jour forcé à {h} h");
            assert_eq!(periode_effective(ChoixPixel::Soir, h), Periode::Soir, "soir forcé à {h} h");
            assert_eq!(periode_effective(ChoixPixel::Nuit, h), Periode::Nuit, "nuit forcée à {h} h");
        }
    }

    #[test]
    fn choix_pixel_cles_et_rangs() {
        for (r, c) in ChoixPixel::TOUS.into_iter().enumerate() {
            assert_eq!(ChoixPixel::depuis_cle(c.cle()), c);
            assert_eq!(c.rang(), r);
            assert_eq!(ChoixPixel::depuis_rang(r), c);
        }
        assert_eq!(ChoixPixel::depuis_cle("n'importe quoi"), ChoixPixel::Auto);
        assert_eq!(ChoixPixel::depuis_rang(99), ChoixPixel::Nuit);
    }

    /// Non-régression de la table de priorité : les 32 combinaisons, écrites une à une par la règle du brief
    #[test]
    fn priorite_complete_des_etats() {
        for bits in 0..32u32 {
            let (activite, conflit, erreur, reussite, nuit) = (bits & 1 != 0, bits & 2 != 0, bits & 4 != 0, bits & 8 != 0, bits & 16 != 0);
            let attendu = if erreur {
                EtatPixel::Erreur
            } else if conflit {
                EtatPixel::Attention
            } else if nuit {
                EtatPixel::Nuit
            } else if activite {
                EtatPixel::Activite
            } else if reussite {
                EtatPixel::Reussite
            } else {
                EtatPixel::Repos
            };
            assert_eq!(etat_pixel(activite, conflit, erreur, reussite, nuit), attendu, "bits {bits:05b}");
        }
    }

    #[test]
    fn priorites_nommees() {
        // Erreur et conflit passent avant tout, même la nuit
        assert_eq!(etat_pixel(true, true, true, true, true), EtatPixel::Erreur);
        assert_eq!(etat_pixel(true, true, false, true, true), EtatPixel::Attention);
        // La nuit ferme les yeux : même pendant une opération ou juste après
        assert_eq!(etat_pixel(true, false, false, false, true), EtatPixel::Nuit);
        assert_eq!(etat_pixel(false, false, false, true, true), EtatPixel::Nuit);
        // Le jour : activité avant réussite avant repos
        assert_eq!(etat_pixel(true, false, false, true, false), EtatPixel::Activite);
        assert_eq!(etat_pixel(false, false, false, true, false), EtatPixel::Reussite);
        assert_eq!(etat_pixel(false, false, false, false, false), EtatPixel::Repos);
    }

    #[test]
    fn teintes_selon_l_heure() {
        assert_eq!(teinte(EtatPixel::Repos, Periode::Jour), Teinte::Cyan);
        assert_eq!(teinte(EtatPixel::Activite, Periode::Soir), Teinte::Ambre);
        assert_eq!(teinte(EtatPixel::Nuit, Periode::Nuit), Teinte::Faible);
        assert_eq!(teinte(EtatPixel::Attention, Periode::Nuit), Teinte::Doux);
        for p in [Periode::Jour, Periode::Soir, Periode::Nuit] {
            assert_eq!(teinte(EtatPixel::Erreur, p), Teinte::Erreur);
        }
    }

    fn barres(f: &Forme) -> Vec<(f32, f32, f32, f32)> {
        f.elements.iter().filter_map(|e| if let Element::Barre { cx, cy, l, h, .. } = *e { Some((cx, cy, l, h)) } else { None }).collect()
    }

    #[test]
    fn repos_deux_barres_droites_de_8_sur_14_ecart_9() {
        let b = barres(&forme(EtatPixel::Repos, 0.0));
        assert_eq!(b.len(), 2);
        assert!(b.iter().all(|&(_, cy, l, h)| cy == 0.0 && l == 8.0 && h == 14.0));
        // Écart entre les bords intérieurs : 9 DIP
        assert_eq!((b[1].0 - 4.0) - (b[0].0 + 4.0), 9.0);
    }

    #[test]
    fn clignement_ecrase_les_barres_sans_les_faire_disparaitre() {
        let ferme = barres(&forme(EtatPixel::Repos, 1.0));
        assert!(ferme.iter().all(|b| b.3 > 1.0 && b.3 < 3.0), "{ferme:?}");
        let mi = barres(&forme(EtatPixel::Repos, 0.5));
        assert!(mi.iter().all(|b| b.3 > ferme[0].3 && b.3 < 14.0));
    }

    #[test]
    fn activite_glisse_de_plus_ou_moins_3_dip() {
        let x = |phase: f32| barres(&forme(EtatPixel::Activite, phase))[0].0;
        let repos = barres(&forme(EtatPixel::Repos, 0.0))[0].0;
        assert!((x(0.0) - repos).abs() < 1e-4);
        assert!((x(0.25) - repos - 3.0).abs() < 1e-4);
        assert!((x(0.75) - repos + 3.0).abs() < 1e-4);
        // Les deux barres glissent ensemble
        let b = barres(&forme(EtatPixel::Activite, 0.25));
        assert!((b[1].0 - b[0].0 - 17.0).abs() < 1e-4);
    }

    #[test]
    fn attention_plus_haute_et_immobile() {
        let a = barres(&forme(EtatPixel::Attention, 0.0));
        let b = barres(&forme(EtatPixel::Attention, 0.6));
        assert_eq!(a, b);
        assert!(a.iter().all(|x| x.3 > 14.0));
    }

    #[test]
    fn reussite_deux_chevrons_pointe_en_haut() {
        let f = forme(EtatPixel::Reussite, 0.0);
        assert_eq!(f.elements.len(), 4);
        // Chaque chevron : deux jambes qui se rejoignent au sommet (y le plus petit)
        for paire in f.elements.chunks(2) {
            let (Element::Trait { x2: sx, y2: sy, .. }, Element::Trait { x1: tx, y1: ty, .. }) = (paire[0], paire[1]) else { panic!("traits attendus") };
            assert_eq!((sx, sy), (tx, ty));
            assert!(sy < 0.0);
        }
    }

    #[test]
    fn erreur_deux_croix_et_nuit_deux_traits_horizontaux() {
        let croix = forme(EtatPixel::Erreur, 0.0);
        assert_eq!(croix.elements.len(), 4, "deux branches par croix");
        // Les branches d'une croix se coupent en son centre
        for paire in croix.elements.chunks(2) {
            let (Element::Trait { x1: a, y1: b, x2: c, y2: d }, Element::Trait { x1: e, y1: f, x2: g, y2: h }) = (paire[0], paire[1]) else { panic!("traits attendus") };
            assert!(((a + c) / 2.0 - (e + g) / 2.0).abs() < 1e-4 && ((b + d) / 2.0 - (f + h) / 2.0).abs() < 1e-4);
        }
        // Nuit : les deux barres de repos écrasées à 14 % (`scaleY(.14)`), même largeur et mêmes centres
        let nuit = barres(&forme(EtatPixel::Nuit, 0.0));
        let repos = barres(&forme(EtatPixel::Repos, 0.0));
        assert_eq!(nuit.len(), 2);
        for (n, r) in nuit.iter().zip(&repos) {
            assert_eq!((n.0, n.1, n.2), (r.0, r.1, r.2));
            assert!((n.3 - 14.0 * 0.14).abs() < 1e-4, "hauteur de nuit {}", n.3);
        }
    }

    #[test]
    fn nuit_est_le_repos_aplati_au_maximum() {
        assert_eq!(forme_affichee(EtatPixel::Nuit, 0.0, 1.0), forme(EtatPixel::Nuit, 0.0));
        // Au début de la transition (k = 0) la nuit part des barres de repos entières
        assert_eq!(forme_affichee(EtatPixel::Nuit, 0.0, 0.0), forme(EtatPixel::Repos, 0.0));
        // À mi-course, entre les deux
        let h = |k: f32| barres(&forme_affichee(EtatPixel::Nuit, 0.0, k))[0].3;
        assert!(h(0.5) < h(0.0) && h(0.5) > h(1.0));
        // Les chevrons (traits) ne s'écrasent pas
        assert_eq!(forme_affichee(EtatPixel::Reussite, 0.0, 1.0), forme(EtatPixel::Reussite, 0.0));
        // Sortie de nuit sur un état à barres : la hauteur remonte
        assert!(barres(&forme_affichee(EtatPixel::Activite, 0.0, 0.5))[0].3 < 14.0);
    }

    #[test]
    fn clignement_ferme_en_270_ms_puis_rouvre_en_360_ms() {
        assert_eq!(phase_clignement(0), 0.0);
        assert!((phase_clignement(CLIGNEMENT_FERME_MS) - 1.0).abs() < 1e-3, "creux à 270 ms");
        assert!(phase_clignement(135) > 0.0 && phase_clignement(135) < 1.0);
        assert!(phase_clignement(450) > 0.0 && phase_clignement(450) < 1.0, "rouvre");
        assert_eq!(phase_clignement(CLIGNEMENT_MS), 0.0);
        assert_eq!(phase_clignement(10_000), 0.0, "au-delà : ouvert");
        // Le creux est bien à scaleY(.1) : hauteur 1,4 sur 14
        let h = barres(&forme(EtatPixel::Repos, 1.0))[0].3;
        assert!((h - 14.0 * CLIGNEMENT_ECHELLE).abs() < 0.2, "{h}");
    }

    #[test]
    fn regard_centre_sans_pointeur_ou_sans_animation() {
        assert_eq!(regard_pointeur(None, (100.0, 100.0), true), (0.0, 0.0));
        assert_eq!(regard_pointeur(Some((500.0, 20.0)), (100.0, 100.0), false), (0.0, 0.0), "mouvement réduit ou yeux coupés");
        // Pointeur pile sur Pixel : droit devant
        assert_eq!(regard_pointeur(Some((100.0, 100.0)), (100.0, 100.0), true), (0.0, 0.0));
    }

    #[test]
    fn regard_vers_la_gauche_puis_la_droite_et_borne() {
        let c = (100.0, 500.0);
        let (gx, _) = regard_pointeur(Some((-300.0, 500.0)), c, true);
        let (dx, _) = regard_pointeur(Some((900.0, 500.0)), c, true);
        assert!(gx < 0.0 && dx > 0.0, "gauche {gx}, droite {dx}");
        assert_eq!((gx, dx), (-REGARD_X_MAX, REGARD_X_MAX), "loin : amplitude maximale");
        // Vers le haut : y négatif, borné à 4
        let (_, hy) = regard_pointeur(Some((100.0, 0.0)), c, true);
        assert_eq!(hy, -REGARD_Y_MAX);
        // Aucune position, aussi lointaine soit-elle, ne dépasse les bornes
        for &(px, py) in &[(-1e6f32, -1e6f32), (1e6, 1e6), (3.0, 7.0), (1e9, 0.0), (0.0, 1e9)] {
            let (x, y) = regard_pointeur(Some((px, py)), c, true);
            assert!(x.abs() <= REGARD_X_MAX && y.abs() <= REGARD_Y_MAX, "({px}, {py}) -> ({x}, {y})");
        }
        // Proche : amplitude partielle, croissante avec la distance
        let (p1, _) = regard_pointeur(Some((140.0, 500.0)), c, true);
        let (p2, _) = regard_pointeur(Some((200.0, 500.0)), c, true);
        assert!(p1 > 0.0 && p1 < p2 && p2 < REGARD_X_MAX, "{p1} {p2}");
    }

    #[test]
    fn regard_quantifie_au_demi_dip() {
        let (x, y) = regard_pointeur(Some((173.0, 431.0)), (100.0, 500.0), true);
        for v in [x, y] {
            assert!(((v / REGARD_PAS).round() * REGARD_PAS - v).abs() < 1e-5, "{v}");
        }
        // Deux pointeurs voisins donnent le même regard : pas de repeinte pour un pixel de plus
        assert_eq!(regard_pointeur(Some((400.0, 500.0)), (100.0, 500.0), true), regard_pointeur(Some((401.0, 500.0)), (100.0, 500.0), true));
    }

    #[test]
    fn texte_de_l_etat_vide() {
        assert_eq!(texte_vide(""), "Dossier vide. Glisse des fichiers ici.");
        assert_eq!(texte_vide("   "), "Dossier vide. Glisse des fichiers ici.");
        assert_eq!(texte_vide("rapport"), "Aucun résultat pour « rapport ».");
        assert_eq!(texte_vide("  a b "), "Aucun résultat pour « a b ».");
    }

    #[test]
    fn yeux_de_l_etat_vide_selon_la_periode() {
        assert_eq!(yeux_vides(Periode::Jour), (Teinte::Cyan, 1.0));
        assert_eq!(yeux_vides(Periode::Soir), (Teinte::Ambre, 1.0));
        let (t, e) = yeux_vides(Periode::Nuit);
        assert_eq!((t, e), (Teinte::Faible, NUIT_ECHELLE));
    }

    #[test]
    fn chaque_etat_a_une_forme_distincte() {
        let tous = [EtatPixel::Repos, EtatPixel::Activite, EtatPixel::Attention, EtatPixel::Reussite, EtatPixel::Erreur, EtatPixel::Nuit];
        for (i, a) in tous.iter().enumerate() {
            for b in &tous[i + 1..] {
                // Activité à la phase 0 est la forme de repos décalée de 0 : on compare à la phase 0,25
                assert_ne!(forme(*a, 0.25).elements, forme(*b, 0.25).elements, "{a:?} et {b:?} se confondent sans la couleur");
            }
        }
    }

    #[test]
    fn regard_de_la_palette() {
        assert_eq!(regard(0, 10), -4.0);
        assert_eq!(regard(9, 10), 4.0);
        assert_eq!(regard(0, 1), 0.0);
        assert_eq!(regard(0, 0), 0.0);
        assert!(regard(4, 9).abs() < 1e-5);
        // Toujours borné à ±4 DIP et entier, quelle que soit la position ; jamais en dehors, même rang hors liste
        for total in 0..60usize {
            for rang in 0..70usize {
                let r = regard(rang, total);
                assert!((-4.0..=4.0).contains(&r) && r.fract() == 0.0, "regard({rang}, {total}) = {r}");
            }
        }
        // Monotone : plus la ligne est basse, plus le regard descend
        assert!((0..40).map(|k| regard(k, 40)).collect::<Vec<_>>().windows(2).all(|w| w[0] <= w[1]));
        let f = forme(EtatPixel::Repos, 0.0).decalee(0.0, 4.0);
        assert!(barres(&f).iter().all(|b| b.1 == 4.0));
    }

    #[test]
    fn yeux_contents_a_5_dip() {
        let h = |k: f32| barres(&forme(EtatPixel::Repos, 0.0).contente(k))[0].3;
        assert_eq!(h(0.0), 14.0);
        assert_eq!(h(1.0), CONTENTS_H);
        assert!(h(0.5) < 14.0 && h(0.5) > CONTENTS_H);
        // Les chevrons ne changent pas
        assert_eq!(forme(EtatPixel::Reussite, 0.0).contente(1.0), forme(EtatPixel::Reussite, 0.0));
    }

    #[test]
    fn clignement_de_la_palette_dure_280_ms_avec_le_creux_au_milieu() {
        assert_eq!(phase_clignement_palette(0), 0.0);
        assert!((phase_clignement_palette(CLIGNEMENT_PALETTE_MS / 2) - 1.0).abs() < 1e-3);
        assert!(phase_clignement_palette(70) > 0.0 && phase_clignement_palette(70) < 1.0);
        assert!(phase_clignement_palette(210) > 0.0 && phase_clignement_palette(210) < 1.0);
        assert_eq!(phase_clignement_palette(CLIGNEMENT_PALETTE_MS), 0.0);
        assert_eq!(phase_clignement_palette(5000), 0.0);
    }

    #[test]
    fn phase_du_glissement_est_cyclique() {
        assert_eq!(phase_glissement(0), 0.0);
        assert!((phase_glissement(800) - 0.5).abs() < 1e-6);
        assert_eq!(phase_glissement(1600), 0.0);
    }
}
