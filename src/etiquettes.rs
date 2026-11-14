// Étiquettes (lot 16, maquette `.sq` / `tagSel`) : 1, 2 ou 3 petits carrés après le nom d'un fichier ou d'un dossier.
// Depuis N-03, façade mince sur le modèle unique `marquage.rs` (qui porte aussi les balises de couleur et les groupes, dans
// `marques.txt`). L'API reste celle d'avant pour les appelants ; la logique, le format et les tests de fond sont dans `marquage.rs`.

use crate::marquage;

/// Plus grande étiquette : 3 carrés
pub const MAX: u8 = marquage::MAX_ETIQUETTE;

/// Clé d'un chemin (minuscules, séparateurs `\`, sans `\` final)
pub use marquage::cle;

/// Étiquette (1 à 3) de ce chemin
pub fn de(chemin: &str) -> Option<u8> {
    marquage::lire(|m| if m.marques.is_empty() { None } else { m.marques.get(&cle(chemin)).and_then(|x| x.etiquette) })
}

/// Raccourci pour le dessin : une seule prise du verrou pour savoir s'il y a la moindre étiquette
pub fn existe_une() -> bool {
    marquage::lire(|m| m.marques.values().any(|x| x.etiquette.is_some()))
}

/// Pose l'étiquette `n` sur ces chemins, ou la retire si tous l'ont déjà. Rend vrai si elle a été posée.
pub fn basculer(chemins: &[String], n: u8) -> bool {
    if !(1..=MAX).contains(&n) {
        return false;
    }
    marquage::modifier(|m| m.basculer_etiquette(chemins, n))
}

/// La marque suit un renommage ou un déplacement fait par SpaceFiles (dossier : ce qu'il contient aussi)
pub fn suivre(ancien: &str, nouveau: &str) {
    suivre_paires(&[(ancien.to_string(), nouveau.to_string())]);
}

/// Plusieurs renommages ou déplacements d'un coup (lot, déplacement de plusieurs éléments). Les trois sortes de marque
/// (étiquette, balise, groupes) suivent ensemble.
pub fn suivre_paires(paires: &[(String, String)]) {
    marquage::modifier(|m| {
        m.suivre(paires);
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Vrai renommage sur disque (dossier temporaire), aller puis retour comme Ctrl+Z : l'étiquette suit, la copie ne l'hérite pas
    #[test]
    fn renommage_reel_annulation_et_copie() {
        let base = std::env::temp_dir().join(format!("ne-etq-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).unwrap();
        let d = base.to_string_lossy().to_string();
        std::fs::write(base.join("a.txt"), b"x").unwrap();
        let chemin = |n: &str| format!("{d}\\{n}");
        assert!(basculer(&[chemin("a.txt")], 3));
        crate::operations::renommer(&d, "a.txt", "b.txt").unwrap();
        assert_eq!((de(&chemin("a.txt")), de(&chemin("b.txt"))), (None, Some(3)));
        // Ctrl+Z : `defaire` rappelle `renommer` dans l'autre sens
        crate::operations::renommer(&d, "b.txt", "a.txt").unwrap();
        assert_eq!((de(&chemin("a.txt")), de(&chemin("b.txt"))), (Some(3), None));
        // Une copie est un autre fichier : aucune étiquette (rien ne la suit, `fin_operation` ne traite que Deplacer)
        std::fs::copy(base.join("a.txt"), base.join("copie.txt")).unwrap();
        assert_eq!(de(&chemin("copie.txt")), None);
        // Renommage par lot
        std::fs::write(base.join("c.txt"), b"y").unwrap();
        assert!(basculer(&[chemin("c.txt")], 1));
        crate::lot::renommer(&d, &[("c.txt".to_string(), "e.txt".to_string())]).unwrap();
        assert_eq!((de(&chemin("c.txt")), de(&chemin("e.txt"))), (None, Some(1)));
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn api_publique_sans_disque_pendant_les_tests() {
        let a = r"Z:\lot16-test\Unique.TXT".to_string();
        assert_eq!(de(&a), None);
        assert!(basculer(&[a.clone()], 2));
        assert_eq!(de(r"z:/lot16-test/unique.txt"), Some(2));
        assert!(existe_une());
        suivre(&a, r"Z:\lot16-test\Autre.txt");
        assert_eq!(de(&a), None);
        assert_eq!(de(r"Z:\lot16-test\autre.txt"), Some(2));
        assert!(!basculer(&[r"Z:\lot16-test\autre.txt".to_string()], 2));
        assert_eq!(de(r"Z:\lot16-test\autre.txt"), None);
        // Étiquette hors de 1 à 3 : refusée
        assert!(!basculer(&[a.clone()], 4));
        assert_eq!(de(&a), None);
    }
}
