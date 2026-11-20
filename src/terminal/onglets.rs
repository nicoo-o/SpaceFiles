//! Onglets du panneau terminal (PLAN-TERMINAL, T4a) : l'arithmétique des rangs et les libellés, sans Win32 ni écran.
//! La fenêtre garde la liste des panneaux et le rang de l'onglet actif ; ce module dit seulement quel rang devient actif
//! après un changement ou une fermeture, et ce que l'onglet affiche.

/// Nombre maximal d'onglets : chacun garde un shell vivant (de la mémoire et un processus), et l'en-tête du panneau doit
/// pouvoir tous les montrer (voir `zones::terminal_onglet_largeur`).
pub const ONGLETS_MAX: usize = 8;

/// Rang de l'onglet suivant, en bouclant. Sans onglet, 0.
pub fn suivant(actif: usize, n: usize) -> usize {
    if n == 0 {
        0
    } else {
        (actif + 1) % n
    }
}

/// Rang de l'onglet précédent, en bouclant. Sans onglet, 0.
pub fn precedent(actif: usize, n: usize) -> usize {
    if n == 0 {
        0
    } else {
        (actif + n - 1) % n
    }
}

/// Rang actif après la fermeture de l'onglet `ferme`, `restants` onglets étant encore ouverts. Fermer l'onglet affiché donne la
/// place à celui qui le suit (ou, s'il était le dernier, à celui qui le précède) ; fermer un autre onglet garde l'affiché.
pub fn apres_fermeture(actif: usize, ferme: usize, restants: usize) -> usize {
    if restants == 0 {
        return 0;
    }
    if ferme < actif {
        actif - 1
    } else {
        actif.min(restants - 1)
    }
}

/// Dernier élément d'un chemin Windows : « C:\\Users\\a\\projet » donne « projet », « C:\\ » donne « C: ». Vide si le chemin l'est.
pub fn dernier_element(chemin: &str) -> &str {
    let net = chemin.trim_end_matches(['\\', '/']);
    if net.is_empty() {
        return chemin.trim_matches(['\\', '/']);
    }
    net.rsplit(['\\', '/']).next().unwrap_or(net)
}

/// Texte d'un onglet : « 2 · projet ». `max` est le nombre de caractères que la place permet ; au-delà, le nom du dossier est
/// coupé et se termine par « … ». Un dossier inconnu ne laisse que le numéro.
pub fn libelle(rang: usize, dossier: &str, max: usize) -> String {
    let numero = (rang + 1).to_string();
    let nom = dernier_element(dossier);
    if nom.is_empty() {
        return numero;
    }
    let tete = format!("{numero} · ");
    let place = max.saturating_sub(tete.chars().count());
    if place == 0 {
        return numero;
    }
    if nom.chars().count() <= place {
        return format!("{tete}{nom}");
    }
    let coupe: String = nom.chars().take(place.saturating_sub(1)).collect();
    format!("{tete}{coupe}…")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn suivant_et_precedent_bouclent() {
        assert_eq!((suivant(0, 3), suivant(2, 3), suivant(0, 1)), (1, 0, 0));
        assert_eq!((precedent(0, 3), precedent(2, 3), precedent(0, 1)), (2, 1, 0));
        assert_eq!((suivant(0, 0), precedent(0, 0)), (0, 0), "sans onglet, jamais de panique");
    }

    #[test]
    fn fermer_l_onglet_affiche_donne_la_place_au_suivant_puis_au_precedent() {
        // trois onglets A B C ; on ferme B (affiché) : C prend le rang 1
        assert_eq!(apres_fermeture(1, 1, 2), 1);
        // on ferme C (affiché, dernier) : B devient l'onglet affiché
        assert_eq!(apres_fermeture(2, 2, 2), 1);
        // on ferme A (affiché, premier) : B passe au rang 0
        assert_eq!(apres_fermeture(0, 0, 2), 0);
    }

    #[test]
    fn fermer_un_autre_onglet_garde_l_onglet_affiche() {
        // A B C, B... on affiche C (2) et on ferme A (0) : C est maintenant au rang 1
        assert_eq!(apres_fermeture(2, 0, 2), 1);
        // on affiche A (0) et on ferme C (2) : A reste au rang 0
        assert_eq!(apres_fermeture(0, 2, 2), 0);
        // on affiche B (1) et on ferme C (2) : B reste au rang 1
        assert_eq!(apres_fermeture(1, 2, 2), 1);
    }

    #[test]
    fn fermer_le_dernier_onglet_ne_laisse_aucun_rang_invalide() {
        assert_eq!(apres_fermeture(0, 0, 0), 0);
        for n in 1..6usize {
            for actif in 0..n {
                for ferme in 0..n {
                    let r = apres_fermeture(actif, ferme, n - 1);
                    assert!(n == 1 || r < n - 1, "rang {r} hors de {} onglets (actif {actif}, fermé {ferme})", n - 1);
                }
            }
        }
    }

    #[test]
    fn le_dernier_element_d_un_chemin() {
        assert_eq!(dernier_element(r"C:\Users\a\projet"), "projet");
        assert_eq!(dernier_element(r"C:\Users\a\projet\"), "projet");
        assert_eq!(dernier_element("C:/x/y"), "y");
        assert_eq!(dernier_element(r"C:\"), "C:");
        assert_eq!(dernier_element(r"\\serveur\partage"), "partage");
        assert_eq!(dernier_element(""), "");
        assert_eq!(dernier_element(r"\"), "");
    }

    #[test]
    fn le_libelle_dit_le_numero_et_le_dossier() {
        assert_eq!(libelle(0, r"C:\Users\a\projet", 40), "1 · projet");
        assert_eq!(libelle(2, r"C:\Users\a\projet\", 40), "3 · projet");
        assert_eq!(libelle(0, "", 40), "1", "dossier inconnu : le numéro seul");
    }

    #[test]
    fn un_libelle_trop_long_est_coupe_avec_des_points() {
        let l = libelle(0, r"C:\nothing-explorer-tres-long", 12);
        assert_eq!(l.chars().count(), 12, "« {l} » doit tenir exactement dans 12 caractères");
        assert!(l.starts_with("1 · ") && l.ends_with('…'), "« {l} »");
        // trop étroit pour le moindre nom : le numéro seul
        assert_eq!(libelle(0, "projet", 4), "1");
        assert_eq!(libelle(0, "projet", 0), "1");
        // juste assez de place pour le nom entier : pas de points
        assert_eq!(libelle(0, "projet", 10), "1 · projet");
    }

    #[test]
    fn un_libelle_ne_coupe_pas_un_caractere_en_deux() {
        let l = libelle(0, "dossier-éééééé", 8);
        assert!(l.chars().count() <= 8, "« {l} »");
    }
}
