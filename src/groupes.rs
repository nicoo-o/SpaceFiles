// Groupes créés par l'utilisateur (comme les « tags » de Files) : un nom, une couleur, et des fichiers ou
// dossiers de n'importe où. Un élément peut être dans plusieurs groupes. Section GROUPES du volet ; ouvrir un
// groupe liste ses éléments (Emplacement::Groupe).
// Enregistrés dans les réglages : `groupes = ["r|PROJET X"]` (couleur|nom), `membres = ["PROJET X|C:\a.txt"]`.

use crate::marquage::{self, normaliser};

/// Couleurs proposées, dans l'ordre où un nouveau groupe les prend : le violet d'abord (choix de l'auteur, 08/11/2026), puis
/// les teintes de la maquette (bleu, vert), puis celles de Nothing
pub const COULEURS: [(char, &str); 7] = [('p', "VIOLET"), ('u', "BLEU"), ('v', "VERT"), ('a', "AMBRE"), ('r', "ROUGE"), ('b', "BLANC"), ('g', "GRIS")];

#[derive(Clone, Debug, PartialEq)]
pub struct Groupe {
    pub nom: String,
    pub couleur: char,
    /// Nombre d'éléments (pour le volet)
    pub nombre: usize,
}

/// Groupes dans l'ordre de création, avec leur nombre d'éléments
pub fn liste() -> Vec<Groupe> {
    marquage::lire(|m| {
        m.groupes
            .iter()
            .map(|(nom, couleur)| Groupe { nom: nom.clone(), couleur: *couleur, nombre: m.marques.values().filter(|x| x.groupes.iter().any(|(_, g)| g == nom)).count() })
            .collect()
    })
}

pub fn existe(nom: &str) -> bool {
    let nom = normaliser(nom);
    marquage::lire(|m| m.groupes.iter().any(|(n, _)| *n == nom))
}

/// Groupes d'un élément
pub fn de(chemin: &str) -> Vec<String> {
    marquage::lire(|m| {
        if m.marques.is_empty() {
            return Vec::new();
        }
        let mut g = m.marques.get(&marquage::cle(chemin)).map(|x| x.groupes.clone()).unwrap_or_default();
        g.sort();
        g.into_iter().map(|(_, n)| n).collect()
    })
}

/// Chemins des éléments d'un groupe, ou d'une expression « A+B,C » (V5)
pub fn membres(nom: &str) -> Vec<String> {
    if nom.contains(['+', ',']) && !existe(nom) {
        return evaluer(nom, &membres_simples);
    }
    membres_simples(nom)
}

/// « A+B,C » : ET avec « + », OU avec « , » (le ET lie plus fort) ; un groupe inconnu est vide
pub fn evaluer(expr: &str, membres_de: &dyn Fn(&str) -> Vec<String>) -> Vec<String> {
    let mut sortie: Vec<String> = Vec::new();
    for ou in expr.split(',') {
        let mut termes = ou.split('+').map(|t| membres_de(&normaliser(t)));
        let Some(mut commun) = termes.next() else { continue };
        for t in termes {
            commun.retain(|c| t.contains(c));
        }
        for c in commun {
            if !sortie.contains(&c) {
                sortie.push(c);
            }
        }
    }
    sortie
}

fn membres_simples(nom: &str) -> Vec<String> {
    marquage::lire(|m| m.membres(nom))
}

/// Crée un groupe (couleur suivante dans la liste). Renvoie son nom final, ou None s'il est vide.
pub fn creer(nom: &str) -> Option<String> {
    let nom = normaliser(nom);
    if nom.is_empty() {
        return None;
    }
    if !existe(&nom) {
        marquage::modifier(|m| {
            let c = COULEURS[m.groupes.len() % COULEURS.len()].0;
            m.creer_groupe(&nom, c);
        });
    }
    Some(nom)
}

pub fn renommer(ancien: &str, nouveau: &str) -> Option<String> {
    let nouveau = normaliser(nouveau);
    if nouveau.is_empty() || nouveau == ancien || existe(&nouveau) {
        return None;
    }
    marquage::modifier(|m| m.renommer_groupe(ancien, &nouveau));
    Some(nouveau)
}

pub fn colorer(nom: &str, couleur: char) {
    marquage::modifier(|m| m.colorer_groupe(nom, couleur));
}

/// Supprime le groupe (les fichiers ne sont pas touchés)
pub fn supprimer(nom: &str) {
    marquage::modifier(|m| m.supprimer_groupe(nom));
}

/// Ajoute ces éléments au groupe (sans doublons). Renvoie le nombre d'ajouts.
pub fn ajouter(nom: &str, chemins: &[String]) -> usize {
    marquage::modifier(|m| m.ajouter_au_groupe(nom, chemins))
}

pub fn retirer(nom: &str, chemins: &[String]) {
    marquage::modifier(|m| m.retirer_du_groupe(nom, chemins));
}

/// Tous les éléments choisis sont-ils dans ce groupe ?
pub fn contient_tous(nom: &str, chemins: &[String]) -> bool {
    !chemins.is_empty() && chemins.iter().all(|c| de(c).iter().any(|n| n == nom))
}

/// Le groupe suit un renommage ou un déplacement fait dans l'explorateur (comme les étiquettes et les balises, en un seul
/// modèle : un second appel après `etiquettes::suivre` ne trouve plus rien à bouger)
pub fn suivre(ancien: &str, nouveau: &str) {
    crate::etiquettes::suivre(ancien, nouveau);
}

/// Éléments d'un groupe, comme une liste de dossier (les éléments disparus sont ignorés)
pub fn lire(nom: &str) -> Vec<crate::dossier::Entree> {
    use std::os::windows::fs::MetadataExt;
    membres(nom)
        .into_iter()
        .filter_map(|chemin| {
            let m = std::fs::metadata(&chemin).ok()?;
            let (parent, n) = chemin.trim_end_matches('\\').rsplit_once('\\')?;
            let parent = if parent.len() == 2 { format!("{parent}\\") } else { parent.to_string() };
            let mut e = crate::dossier::entree_fichier(n, m.len(), m.last_write_time(), m.is_dir());
            e.ajout = m.creation_time();
            e.parent = Some(std::sync::Arc::from(parent.as_str()));
            Some(e)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::evaluer;

    #[test]
    fn et_ou_de_groupes() {
        let membres = |g: &str| -> Vec<String> {
            match g {
                "ROUGE" => vec!["a", "b", "c"],
                "BLEU" => vec!["b", "c", "d"],
                "VERT" => vec!["e"],
                _ => vec![],
            }
            .into_iter()
            .map(String::from)
            .collect()
        };
        assert_eq!(evaluer("rouge+bleu", &membres), ["b", "c"]);
        assert_eq!(evaluer("rouge,vert", &membres), ["a", "b", "c", "e"]);
        assert_eq!(evaluer("rouge+bleu,vert", &membres), ["b", "c", "e"]);
        assert!(evaluer("rouge+inconnu", &membres).is_empty());
    }
}
