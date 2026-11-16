// Profils Simple et Expert (phase 1 du plan de l'avenir) : une seule table dit ce que chaque profil montre. La palette, les
// menus, la touche de raccourci et le rendu la consultent ; aucun `if profil` ne doit traîner ailleurs. Expert montre tout
// (comportement d'avant) ; Simple n'ouvre que la liste blanche ci-dessous : une commande ajoutée plus tard reste donc
// masquée en Simple tant qu'on ne l'a pas inscrite ici, plutôt que visible par oubli.

use crate::menu::Autre;
use crate::palette::Commande;
use crate::reglages::{COL_DATE, COL_TAILLE, COL_TYPE};
use std::sync::atomic::{AtomicBool, Ordering};

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Profil {
    Simple,
    /// Défaut : les installations existantes gardent tout ce qu'elles ont
    #[default]
    Expert,
}

impl Profil {
    /// Valeur écrite dans `reglages.toml`
    pub fn cle(self) -> &'static str {
        match self {
            Profil::Simple => "simple",
            Profil::Expert => "expert",
        }
    }

    /// Lecture tolérante : toute valeur inconnue donne Expert (jamais une fonction retirée par erreur)
    pub fn depuis(texte: &str) -> Profil {
        if texte.trim().eq_ignore_ascii_case("simple") {
            Profil::Simple
        } else {
            Profil::Expert
        }
    }

    pub fn autre(self) -> Profil {
        match self {
            Profil::Simple => Profil::Expert,
            Profil::Expert => Profil::Simple,
        }
    }
}

// Profil en vigueur, lu sans verrou par la palette, les menus et le rendu (même schéma que `reglages::experimental`)
static SIMPLE: AtomicBool = AtomicBool::new(false);

pub fn actuel() -> Profil {
    if SIMPLE.load(Ordering::Relaxed) {
        Profil::Simple
    } else {
        Profil::Expert
    }
}

pub fn fixer(p: Profil) {
    SIMPLE.store(p == Profil::Simple, Ordering::Relaxed);
}

/// Pastilles de balise et carrés d'étiquette dessinés dans les listes : Expert seulement (les marques restent enregistrées)
pub fn balises_visibles() -> bool {
    actuel() == Profil::Expert
}

/// Commandes ouvertes en Simple : actions de base sur les fichiers, onglets, vues liste et grille, tris, aide, réglages
const SIMPLES: &[Commande] = &[
    Commande::Ouvrir,
    Commande::OuvrirOnglet,
    Commande::OuvrirAvec,
    Commande::Couper,
    Commande::Copier,
    Commande::Coller,
    Commande::Renommer,
    Commande::Supprimer,
    Commande::Partager,
    Commande::Proprietes,
    Commande::NouveauFichier,
    Commande::NouveauDossier,
    Commande::NouvelOnglet,
    Commande::FermerOnglet,
    Commande::RouvrirOnglet,
    Commande::ToutChoisir,
    Commande::Actualiser,
    Commande::Annuler,
    Commande::Refaire,
    Commande::Apercu,
    Commande::Galerie,
    Commande::VueListe,
    Commande::VueGrille,
    Commande::TrierNom,
    Commande::TrierDate,
    Commande::TrierType,
    Commande::TrierTaille,
    Commande::TrierCroissant,
    Commande::TrierDecroissant,
    Commande::Restaurer,
    Commande::VoirCorbeille,
    Commande::Palette,
    Commande::PaletteSecondaire,
    Commande::SaisirChemin,
    Commande::SaisirCheminSecondaire,
    Commande::Aide,
    Commande::Reglages,
    Commande::ChercherReglage,
    Commande::BasculerProfil,
    Commande::OuvrirDestination,
];

/// Cette commande est-elle montrée (palette, menus) et exécutable (raccourci compris) dans ce profil ?
pub fn commande_visible(p: Profil, c: Commande) -> bool {
    p == Profil::Expert || SIMPLES.contains(&c)
}

/// Entrée de menu qui n'est pas une commande de palette : en Simple, seulement l'ouverture avec un programme, la
/// sélection, l'impression et les trois colonnes de base
pub fn autre_visible(p: Profil, a: &Autre) -> bool {
    if p == Profil::Expert {
        return true;
    }
    match a {
        Autre::Application(_) | Autre::ChoisirApplication | Autre::Deselectionner | Autre::CopierNoms | Autre::DeposerIci(_) => true,
        Autre::Verbe(v) => matches!(*v, "print" | "edit" | "setdesktopwallpaper"),
        Autre::BasculerColonne(bit) => matches!(*bit, COL_DATE | COL_TYPE | COL_TAILLE),
        _ => false,
    }
}

/// Sous-menu montré en Simple (le contenu est filtré à part ; un sous-menu vidé disparaît)
pub fn sous_visible(p: Profil, titre: &str) -> bool {
    p == Profil::Expert || matches!(titre, "OUVRIR AVEC" | "NOUVEAU" | "TRIER PAR" | "AFFICHAGE")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::palette::COMMANDES;

    #[test]
    fn expert_montre_toute_la_table() {
        assert!(COMMANDES.iter().all(|(c, ..)| commande_visible(Profil::Expert, *c)));
    }

    #[test]
    fn simple_ne_montre_aucune_entree_avancee() {
        // Ce que le plan écarte du profil Simple : double panneau, marques, regroupements, historique, doublons, sauvegarde
        for c in [
            Commande::DoublePanneau,
            Commande::CopierVersAutre,
            Commande::Comparer,
            Commande::Synchroniser,
            Commande::BaliseRouge,
            Commande::Etiquette1,
            Commande::NouveauGroupe,
            Commande::Regrouper(1),
            Commande::Historique,
            Commande::Doublons,
            Commande::SauvegardeLancer,
            Commande::RenommerLot,
            Commande::Terminal,
            Commande::ImageJpg,
            Commande::Zip,
            Commande::RechercheGlobale,
            Commande::VueColonnes,
        ] {
            assert!(!commande_visible(Profil::Simple, c), "{c:?} doit rester caché en Simple");
        }
    }

    #[test]
    fn simple_garde_les_actions_de_base() {
        for c in [Commande::Ouvrir, Commande::Copier, Commande::Coller, Commande::Supprimer, Commande::Renommer, Commande::NouveauDossier, Commande::Palette, Commande::Aide, Commande::BasculerProfil] {
            assert!(commande_visible(Profil::Simple, c), "{c:?} doit rester visible en Simple");
        }
    }

    #[test]
    fn liste_simple_sans_doublon_et_dans_la_table() {
        for (i, c) in SIMPLES.iter().enumerate() {
            assert!(!SIMPLES[..i].contains(c), "{c:?} deux fois");
            assert!(COMMANDES.iter().any(|(x, ..)| x == c), "{c:?} absente de COMMANDES");
        }
    }

    #[test]
    fn lecture_tolerante_et_aller_retour() {
        assert_eq!(Profil::depuis("simple"), Profil::Simple);
        assert_eq!(Profil::depuis(" SIMPLE "), Profil::Simple);
        assert_eq!(Profil::depuis("expert"), Profil::Expert);
        assert_eq!(Profil::depuis("n'importe quoi"), Profil::Expert);
        assert_eq!(Profil::depuis(""), Profil::Expert);
        for p in [Profil::Simple, Profil::Expert] {
            assert_eq!(Profil::depuis(p.cle()), p);
            assert_ne!(p.autre(), p);
        }
        assert_eq!(Profil::default(), Profil::Expert);
    }

    #[test]
    fn menus_simples() {
        assert!(autre_visible(Profil::Simple, &Autre::ChoisirApplication));
        assert!(!autre_visible(Profil::Simple, &Autre::AutrePanneau));
        assert!(!autre_visible(Profil::Simple, &Autre::NouveauGroupe));
        assert!(!autre_visible(Profil::Simple, &Autre::Verbe("runas")));
        assert!(autre_visible(Profil::Simple, &Autre::BasculerColonne(COL_DATE)));
        assert!(!autre_visible(Profil::Simple, &Autre::BasculerColonne(crate::reglages::COL_ORIGINE)));
        assert!(autre_visible(Profil::Expert, &Autre::AutrePanneau));
        assert!(sous_visible(Profil::Simple, "TRIER PAR"));
        assert!(!sous_visible(Profil::Simple, "BALISES"));
        assert!(sous_visible(Profil::Expert, "BALISES"));
    }
}
