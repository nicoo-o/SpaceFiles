// Groupes créés par l'utilisateur (comme les « tags » de Files) : un nom, une couleur, et des fichiers ou
// dossiers de n'importe où. Un élément peut être dans plusieurs groupes. Section GROUPES du volet ; ouvrir un
// groupe liste ses éléments (Emplacement::Groupe).
// Enregistrés dans les réglages : `groupes = ["r|PROJET X"]` (couleur|nom), `membres = ["PROJET X|C:\a.txt"]`.

use std::collections::HashMap;
use std::sync::Mutex;

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

/// Chemin en minuscules → noms de ses groupes ; refait après chaque changement
static INDEX: Mutex<Option<HashMap<String, Vec<String>>>> = Mutex::new(None);

fn avec_index<R>(f: impl FnOnce(&HashMap<String, Vec<String>>) -> R) -> R {
    let mut g = INDEX.lock().unwrap_or_else(|e| e.into_inner());
    let m = g.get_or_insert_with(|| {
        let mut m: HashMap<String, Vec<String>> = HashMap::new();
        for x in crate::reglages::actuels().membres {
            if let Some((nom, chemin)) = x.split_once('|') {
                m.entry(chemin.to_lowercase()).or_default().push(nom.to_string());
            }
        }
        m
    });
    f(m)
}

fn invalider() {
    *INDEX.lock().unwrap_or_else(|e| e.into_inner()) = None;
}

fn normaliser(nom: &str) -> String {
    nom.trim().replace('|', " ").to_uppercase()
}

/// Groupes dans l'ordre de création, avec leur nombre d'éléments
pub fn liste() -> Vec<Groupe> {
    let r = crate::reglages::actuels();
    r.groupes
        .iter()
        .filter_map(|g| {
            let (c, nom) = g.split_once('|')?;
            let nombre = r.membres.iter().filter(|m| m.split_once('|').is_some_and(|(n, _)| n == nom)).count();
            Some(Groupe { nom: nom.to_string(), couleur: c.chars().next().unwrap_or('r'), nombre })
        })
        .collect()
}

pub fn existe(nom: &str) -> bool {
    let nom = normaliser(nom);
    liste().iter().any(|g| g.nom == nom)
}

/// Groupes d'un élément
pub fn de(chemin: &str) -> Vec<String> {
    avec_index(|m| if m.is_empty() { Vec::new() } else { m.get(&chemin.to_lowercase()).cloned().unwrap_or_default() })
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
    crate::reglages::actuels()
        .membres
        .iter()
        .filter_map(|m| m.split_once('|').filter(|(n, _)| *n == nom).map(|(_, c)| c.to_string()))
        .collect()
}

/// Crée un groupe (couleur suivante dans la liste). Renvoie son nom final, ou None s'il est vide.
pub fn creer(nom: &str) -> Option<String> {
    let nom = normaliser(nom);
    if nom.is_empty() {
        return None;
    }
    if !existe(&nom) {
        let n = liste().len();
        let c = COULEURS[n % COULEURS.len()].0;
        crate::reglages::modifier(|r| r.groupes.push(format!("{c}|{nom}")));
    }
    Some(nom)
}

pub fn renommer(ancien: &str, nouveau: &str) -> Option<String> {
    let nouveau = normaliser(nouveau);
    if nouveau.is_empty() || nouveau == ancien || existe(&nouveau) {
        return None;
    }
    crate::reglages::modifier(|r| {
        for g in r.groupes.iter_mut() {
            if let Some((c, n)) = g.split_once('|') {
                if n == ancien {
                    *g = format!("{c}|{nouveau}");
                }
            }
        }
        for m in r.membres.iter_mut() {
            if let Some((n, ch)) = m.split_once('|') {
                if n == ancien {
                    *m = format!("{nouveau}|{ch}");
                }
            }
        }
    });
    invalider();
    Some(nouveau)
}

pub fn colorer(nom: &str, couleur: char) {
    crate::reglages::modifier(|r| {
        for g in r.groupes.iter_mut() {
            if g.split_once('|').is_some_and(|(_, n)| n == nom) {
                *g = format!("{couleur}|{nom}");
            }
        }
    });
}

/// Supprime le groupe (les fichiers ne sont pas touchés)
pub fn supprimer(nom: &str) {
    crate::reglages::modifier(|r| {
        r.groupes.retain(|g| g.split_once('|').map_or(true, |(_, n)| n != nom));
        r.membres.retain(|m| m.split_once('|').map_or(true, |(n, _)| n != nom));
    });
    invalider();
}

/// Ajoute ces éléments au groupe (sans doublons). Renvoie le nombre d'ajouts.
pub fn ajouter(nom: &str, chemins: &[String]) -> usize {
    let deja: Vec<String> = membres(nom).iter().map(|c| c.to_lowercase()).collect();
    let nouveaux: Vec<String> = chemins.iter().filter(|c| !deja.contains(&c.to_lowercase())).map(|c| format!("{nom}|{c}")).collect();
    let n = nouveaux.len();
    if n > 0 {
        crate::reglages::modifier(|r| r.membres.extend(nouveaux));
        invalider();
    }
    n
}

pub fn retirer(nom: &str, chemins: &[String]) {
    let enlever: Vec<String> = chemins.iter().map(|c| format!("{nom}|{c}").to_lowercase()).collect();
    crate::reglages::modifier(|r| r.membres.retain(|m| !enlever.contains(&m.to_lowercase())));
    invalider();
}

/// Tous les éléments choisis sont-ils dans ce groupe ?
pub fn contient_tous(nom: &str, chemins: &[String]) -> bool {
    !chemins.is_empty() && chemins.iter().all(|c| de(c).iter().any(|n| n == nom))
}

/// Le groupe suit un renommage ou un déplacement fait dans l'explorateur
pub fn suivre(ancien: &str, nouveau: &str) {
    if de(ancien).is_empty() {
        return;
    }
    let a = ancien.to_lowercase();
    crate::reglages::modifier(|r| {
        for m in r.membres.iter_mut() {
            if let Some((n, ch)) = m.split_once('|') {
                if ch.to_lowercase() == a {
                    *m = format!("{n}|{nouveau}");
                }
            }
        }
    });
    invalider();
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
