// Barre d'actions rapides, sous le chemin (comme la barre de commandes de l'Explorateur) :
// sept contrôles visibles — Nouveau ; Copier, Supprimer ; Trier ; trois vues.
// Les autres commandes restent dans les menus, le menu contextuel et la palette ; les boutons à menu ouvrent un menu Windows sombre.

use windows::Win32::Graphics::Direct2D::Common::D2D_RECT_F;

use crate::palette::Commande;
use crate::rendu::rect;
use crate::theme::*;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Outil {
    Nouveau,
    Couper,
    Copier,
    Coller,
    Renommer,
    Partager,
    Supprimer,
    Proprietes,
    Trier,
    Afficher,
    Outils,
    VueListe,
    VueGrille,
    VueColonnes,
    VueTailles,
    Double,
    Plus,
}

pub const BOUTON: f32 = 36.0;
/// Groupe Compact / Confort, à gauche du groupe des vues
pub const DENSITE_LARGEUR: f32 = 128.0;
const DENSITE_ECART: f32 = 16.0;
const DENSITE_BOUTON: f32 = 61.0;
const ICONE: f32 = 30.0;
const CHEVRON: f32 = 16.0;
const ESPACE: f32 = 2.0;
const SEPARATION: f32 = 14.0;

impl Outil {
    /// Rayon de coin du fond de l'outil : une pilule (`BOUTON / 2`) pour « + NOUVEAU » (maquette `.btn`), 10 pour les autres.
    /// Le fond, le contour et le reflet du curseur le partagent : le reflet avait un coin de 8 et dessinait, autour de la pilule,
    /// une seconde forme carrée aux coins arrondis.
    pub fn rayon_coin(self) -> f32 {
        if self == Outil::Nouveau { BOUTON / 2.0 } else { 10.0 }
    }

    /// Texte court (boutons à libellé) ; vide : icône seule
    pub fn libelle(self) -> &'static str {
        match self {
            Outil::Nouveau => "NOUVEAU",
            Outil::Trier => "TRIER",
            Outil::Afficher => "AFFICHER",
            Outil::Outils => "OUTILS",
            _ => "",
        }
    }

    /// Bulle au survol : nom et raccourci
    pub fn bulle(self) -> &'static str {
        match self {
            Outil::Nouveau => "NOUVEAU DOSSIER, FICHIER, ONGLET",
            Outil::Couper => "COUPER · CTRL+X",
            Outil::Copier => "COPIER · CTRL+C",
            Outil::Coller => "COLLER · CTRL+V",
            Outil::Renommer => "RENOMMER · F2",
            Outil::Partager => "PARTAGER",
            Outil::Supprimer => "SUPPRIMER · SUPPR",
            Outil::Proprietes => "PROPRIETES · ALT+ENTREE",
            Outil::Trier => "TRIER PAR NOM, DATE, TYPE, TAILLE",
            Outil::Afficher => "AFFICHAGE, CACHES, APERCU",
            Outil::Outils => "OUTILS NOTHING : ARCHIVES, BALISES, DEV…",
            Outil::VueListe => "VUE LISTE · CTRL+MAJ+1",
            Outil::VueGrille => "VUE GRILLE · CTRL+MAJ+2",
            Outil::VueColonnes => "VUE COLONNES · CTRL+MAJ+3",
            Outil::VueTailles => "TAILLES DES DOSSIERS · CTRL+MAJ+4",
            Outil::Double => "DOUBLE PANNEAU · F6",
            Outil::Plus => "PLUS D'ACTIONS",
        }
    }

    /// Ouvre un menu (sinon : une commande directe)
    pub fn a_menu(self) -> bool {
        matches!(self, Outil::Nouveau | Outil::Trier | Outil::Afficher | Outil::Outils | Outil::Plus)
    }

    pub fn commande(self) -> Option<Commande> {
        Some(match self {
            Outil::Couper => Commande::Couper,
            Outil::Copier => Commande::Copier,
            Outil::Coller => Commande::Coller,
            Outil::Renommer => Commande::Renommer,
            Outil::Partager => Commande::Partager,
            Outil::Supprimer => Commande::Supprimer,
            Outil::Proprietes => Commande::Proprietes,
            Outil::VueListe => Commande::VueListe,
            Outil::VueGrille => Commande::VueGrille,
            Outil::VueColonnes => Commande::VueColonnes,
            Outil::VueTailles => Commande::VueTailles,
            Outil::Double => Commande::DoublePanneau,
            _ => return None,
        })
    }

    /// Icône en points (grille de pas ~2,6 DIP, centrée)
    pub fn motif(self) -> &'static [(i32, i32)] {
        match self {
            Outil::Nouveau => &[(0, -2), (0, -1), (0, 0), (0, 1), (0, 2), (-2, 0), (-1, 0), (1, 0), (2, 0)],
            Outil::Couper => &[(-2, -2), (2, -2), (-1, -1), (1, -1), (0, 0), (-1, 1), (1, 1), (-2, 2), (-3, 2), (2, 2), (3, 2)],
            Outil::Copier => &[(-2, -2), (-1, -2), (0, -2), (-2, -1), (-2, 0), (-1, -1), (0, -1), (1, -1), (2, -1), (-1, 0), (2, 0), (-1, 1), (2, 1), (-1, 2), (0, 2), (1, 2), (2, 2)],
            Outil::Coller => &[(-1, -3), (0, -3), (1, -3), (-2, -2), (2, -2), (-2, -1), (2, -1), (-2, 0), (2, 0), (-2, 1), (2, 1), (-2, 2), (-1, 2), (0, 2), (1, 2), (2, 2)],
            Outil::Renommer => &[(2, -2), (1, -1), (0, 0), (-1, 1), (-2, 2), (3, -3), (0, 2), (1, 2), (2, 2)],
            Outil::Partager => &[(-2, -1), (-2, 0), (-2, 1), (-2, 2), (-1, 2), (0, 2), (1, 2), (2, 2), (2, 1), (0, 0), (1, -1), (2, -2), (1, -2), (2, -1)],
            Outil::Supprimer => &[(0, -3), (-2, -2), (-1, -2), (0, -2), (1, -2), (2, -2), (-1, -1), (1, -1), (-1, 0), (1, 0), (-1, 1), (1, 1), (-1, 2), (0, 2), (1, 2)],
            Outil::Proprietes => &[(0, -2), (-1, 0), (0, 0), (0, 1), (0, 2), (-1, 2), (1, 2)],
            Outil::Trier => &[(-1, -2), (-1, -1), (-1, 0), (-1, 1), (-1, 2), (-2, -1), (0, -1), (2, -2), (2, -1), (2, 0), (2, 1), (2, 2), (1, 1), (3, 1)],
            Outil::Afficher => &[(-3, 0), (-2, -1), (-1, -2), (0, -2), (1, -2), (2, -1), (3, 0), (-2, 1), (-1, 2), (0, 2), (1, 2), (2, 1), (0, 0)],
            Outil::Outils => &[(0, -2), (0, 2), (-2, 0), (2, 0), (-1, -1), (1, 1), (1, -1), (-1, 1), (0, 0)],
            Outil::VueListe => &[(-2, -2), (0, -2), (1, -2), (2, -2), (-2, 0), (0, 0), (1, 0), (2, 0), (-2, 2), (0, 2), (1, 2), (2, 2)],
            Outil::VueGrille => &[(-2, -2), (-1, -2), (-2, -1), (-1, -1), (1, -2), (2, -2), (1, -1), (2, -1), (-2, 1), (-1, 1), (-2, 2), (-1, 2), (1, 1), (2, 1), (1, 2), (2, 2)],
            Outil::VueColonnes => &[(-2, -2), (-2, -1), (-2, 0), (-2, 1), (-2, 2), (0, -2), (0, -1), (0, 0), (0, 1), (0, 2), (2, -2), (2, -1), (2, 0), (2, 1), (2, 2)],
            Outil::VueTailles => &[(-2, -2), (-1, -2), (0, -2), (1, -2), (2, -2), (-2, 0), (-1, 0), (0, 0), (-2, 2)],
            Outil::Double => &[(-2, -2), (-2, -1), (-2, 0), (-2, 1), (-2, 2), (-1, -2), (-1, 2), (1, -2), (1, 2), (2, -2), (2, -1), (2, 0), (2, 1), (2, 2), (0, -2), (0, -1), (0, 0), (0, 1), (0, 2)],
            Outil::Plus => &[(-2, 0), (0, 0), (2, 0)],
        }
    }
}

/// Un bouton placé : zone, et libellé affiché ou non
#[derive(Clone, Copy)]
pub struct Place {
    pub outil: Outil,
    pub zone: D2D_RECT_F,
    pub texte: bool,
    /// Trait de séparation avant ce bouton
    pub separe: bool,
}

// Sept contrôles visibles, comme la maquette v6 : Nouveau, Copier, Supprimer, Trier et trois vues.
// Les autres commandes restent dans les menus, le menu contextuel et la palette.
const GAUCHE: &[&[Outil]] = &[
    &[Outil::Nouveau],
    &[Outil::Copier, Outil::Supprimer],
    &[Outil::Trier],
];
const DROITE: &[&[Outil]] = &[&[Outil::VueListe, Outil::VueGrille, Outil::VueColonnes]];
/// Retirés en premier quand la place manque (ils restent dans « ••• » et la palette)
const SECONDAIRES: &[Outil] = &[Outil::Proprietes, Outil::Partager];
/// Retirés ensuite, une fois les vues parties
const SACRIFIES: &[Outil] = &[Outil::Outils, Outil::Afficher, Outil::Renommer, Outil::Couper];

fn largeur(o: Outil, texte: bool, mesurer: &dyn Fn(&str) -> f32) -> f32 {
    // « NOUVEAU » garde son libellé et son contour de pilule, même quand les autres passent en icônes seules
    if o == Outil::Nouveau {
        return ICONE + mesurer("NOUVEAU") + 34.0;
    }
    if o.a_menu() && o != Outil::Plus {
        ICONE + if texte { mesurer(o.libelle()) + 6.0 } else { 0.0 } + CHEVRON
    } else {
        BOUTON
    }
}

/// Place les boutons entre `x0` et `x1`. Si tout ne tient pas : sans libellés, sans les vues, puis sans les moins utiles.
pub fn placer(x0: f32, x1: f32, mesurer: &dyn Fn(&str) -> f32) -> Vec<Place> {
    let dispo = x1 - x0;
    let total = |groupes: &[Vec<Outil>], texte: bool| -> f32 {
        let n = groupes.iter().filter(|g| !g.is_empty()).count();
        groupes.iter().flatten().map(|&o| largeur(o, texte, mesurer) + ESPACE).sum::<f32>() + SEPARATION * n.saturating_sub(1) as f32
    };
    let complet: Vec<Vec<Outil>> = GAUCHE.iter().map(|g| g.to_vec()).collect();
    let reduit: Vec<Vec<Outil>> = complet.iter().map(|g| g.iter().copied().filter(|o| !SECONDAIRES.contains(o)).collect()).collect();
    let mut droite: Vec<Vec<Outil>> = DROITE.iter().map(|g| g.to_vec()).collect();
    let place = |g: &[Vec<Outil>], d: &[Vec<Outil>], t: bool| total(g, t) + total(d, t) + SEPARATION * 2.0;
    // Par préférence : libellés avant Partager / Propriétés, et les vues avant tout le reste
    let essais = [(&complet, true), (&reduit, true), (&complet, false), (&reduit, false)];
    let (mut gauche, texte) = match essais.iter().find(|(g, t)| place(g, &droite, *t) <= dispo) {
        Some((g, t)) => ((*g).clone(), *t),
        None => {
            droite[0].clear();
            (reduit.clone(), false)
        }
    };
    for o in SACRIFIES {
        if place(&gauche, &droite, texte) <= dispo {
            break;
        }
        for g in gauche.iter_mut() {
            g.retain(|x| x != o);
        }
    }

    let y = BARRE_Y + BARRE_PAD_HAUT;
    let mut v = Vec::new();
    let ranger = |groupes: &[Vec<Outil>], mut x: f32, v: &mut Vec<Place>| {
        let mut premier = true;
        for g in groupes.iter().filter(|g| !g.is_empty()) {
            if !premier {
                x += SEPARATION;
            }
            for (k, &o) in g.iter().enumerate() {
                let w = largeur(o, texte, mesurer);
                v.push(Place { outil: o, zone: D2D_RECT_F { left: x, top: y, right: x + w, bottom: y + BOUTON }, texte: texte || o == Outil::Nouveau, separe: k == 0 && !premier });
                x += w + ESPACE;
            }
            premier = false;
        }
    };
    ranger(&gauche, x0, &mut v);
    let wd = total(&droite, texte);
    ranger(&droite, x1 - wd, &mut v);
    v
}

/// Zones des deux crans de densité (Confort puis Compact), à gauche du groupe des vues. Vides si la barre est trop étroite.
pub fn zones_densite(barre: &[Place]) -> [Option<D2D_RECT_F>; 2] {
    let Some(premier) = barre.iter().find(|p| matches!(p.outil, Outil::VueListe | Outil::VueGrille | Outil::VueColonnes | Outil::VueTailles | Outil::Double)) else {
        return [None, None];
    };
    let gauche = barre
        .iter()
        .filter(|p| !matches!(p.outil, Outil::VueListe | Outil::VueGrille | Outil::VueColonnes | Outil::VueTailles | Outil::Double | Outil::Plus))
        .map(|p| p.zone.right)
        .fold(0.0, f32::max);
    let x = premier.zone.left - DENSITE_ECART - DENSITE_LARGEUR;
    if x < gauche + 12.0 || x + DENSITE_LARGEUR > premier.zone.left - 8.0 {
        return [None, None];
    }
    let y = premier.zone.top;
    [Some(rect(x + 3.0, y, DENSITE_BOUTON, BOUTON)), Some(rect(x + 64.0, y, DENSITE_BOUTON, BOUTON))]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn une_seule_forme_pour_le_bouton_nouveau() {
        // Fond, contour et reflet lisent le même rayon : la pilule (moitié de la hauteur), jamais le coin de 8 du reflet d'avant
        assert_eq!(Outil::Nouveau.rayon_coin(), BOUTON / 2.0);
        for o in [Outil::Copier, Outil::Supprimer, Outil::Trier, Outil::VueListe, Outil::Plus] {
            assert_eq!(o.rayon_coin(), 10.0);
        }
        // Le rayon d'une pilule ne dépasse jamais la moitié de la hauteur du bouton
        assert!(Outil::Nouveau.rayon_coin() * 2.0 <= BOUTON);
    }

    #[test]
    fn repli() {
        let m = |t: &str| t.len() as f32 * 9.0;
        let large = placer(0.0, 1400.0, &m);
        assert_eq!(large.len(), 7);
        assert!(large.iter().all(|p| p.texte));
        assert!(large.iter().any(|p| p.outil == Outil::Copier));
        assert!(large.iter().any(|p| p.outil == Outil::Supprimer));
        assert!(!large.iter().any(|p| matches!(p.outil, Outil::Couper | Outil::Coller | Outil::Renommer | Outil::Partager | Outil::Proprietes | Outil::Afficher | Outil::Outils | Outil::Double | Outil::VueTailles | Outil::Plus)));
        let etroit = placer(0.0, 300.0, &m);
        // « NOUVEAU » garde son libellé et sa pilule ; les autres passent en icônes seules
        assert!(etroit.iter().filter(|p| p.texte).all(|p| p.outil == Outil::Nouveau));
        assert_eq!(etroit.iter().filter(|p| p.texte).count(), 1);
        assert!(!etroit.iter().any(|p| matches!(p.outil, Outil::VueListe | Outil::VueGrille | Outil::VueColonnes)));
        assert!(etroit.last().unwrap().zone.right <= 300.0 + 0.1);
        // Rien ne se chevauche
        for w in etroit.windows(2) {
            assert!(w[0].zone.right <= w[1].zone.left);
        }
    }
}
