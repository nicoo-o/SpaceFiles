// Conflits de noms avant une copie ou un déplacement : au lieu de la question de Windows, une carte
// Nothing compare les deux fichiers côte à côte. Les dossiers de même nom sont fusionnés (on descend
// dedans) ; seuls les fichiers en double sont demandés. L'opération part ensuite avec la liste exacte :
// remplacés (l'ancien va à la corbeille), renommés « nom (2) », ignorés.

use std::collections::HashSet;
use std::os::windows::fs::MetadataExt;

use crate::dossier::{self, Entree};
use crate::operations::{Element, Genre};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Choix {
    /// L'ancien part à la corbeille, le nouveau prend sa place
    Remplacer,
    /// Le nouveau arrive sous « nom (2).ext »
    Garder,
    Ignorer,
    /// Remplacer seulement si le nouveau est plus récent, sinon ignorer
    PlusRecent,
}

impl Choix {
    /// Réponse automatique du réglage CONFLITS PAR DEFAUT (0 : demander)
    pub fn par_defaut(k: u32) -> Option<Choix> {
        [None, Some(Choix::Remplacer), Some(Choix::Garder), Some(Choix::PlusRecent), Some(Choix::Ignorer)].get(k as usize).copied().flatten()
    }

    pub const TOUS: [Choix; 4] = [Choix::Remplacer, Choix::Garder, Choix::PlusRecent, Choix::Ignorer];

    pub fn touche(self) -> char {
        match self {
            Choix::Remplacer => 'R',
            Choix::Garder => 'G',
            Choix::Ignorer => 'I',
            Choix::PlusRecent => 'N',
        }
    }

    pub fn libelle(self) -> &'static str {
        match self {
            Choix::Remplacer => "REMPLACER",
            Choix::Garder => "GARDER LES DEUX",
            Choix::Ignorer => "IGNORER",
            Choix::PlusRecent => "LE PLUS RECENT",
        }
    }

    pub fn detail(self) -> &'static str {
        match self {
            Choix::Remplacer => "ANCIEN A LA CORBEILLE",
            Choix::Garder => "NOUVEAU RENOMME (2)",
            Choix::Ignorer => "RIEN NE CHANGE",
            Choix::PlusRecent => "REMPLACE SI PLUS RECENT",
        }
    }

    pub fn de_touche(c: char) -> Option<Choix> {
        Choix::TOUS.into_iter().find(|x| x.touche() == c)
    }
}

/// Un côté de la comparaison
pub struct Fiche {
    pub chemin: String,
    pub entree: Entree,
}

impl Fiche {
    fn lire(chemin: &str, m: &std::fs::Metadata) -> Fiche {
        let nom = chemin.trim_end_matches('\\').rsplit('\\').next().unwrap_or(chemin);
        let dossier = m.is_dir();
        Fiche { chemin: chemin.to_string(), entree: dossier::entree_fichier(nom, if dossier { 0 } else { m.len() }, m.last_write_time(), dossier) }
    }
}

pub struct Conflit {
    pub source: Fiche,
    pub cible: Fiche,
    pub choix: Option<Choix>,
}

/// Opération retenue le temps de répondre
pub struct Conflits {
    pub genre: Genre,
    /// Dossier de destination demandé (pour le titre)
    pub dest: String,
    /// Ce qui passe sans question
    pub libres: Vec<Element>,
    pub liste: Vec<Conflit>,
    /// Conflit affiché
    pub rang: usize,
    /// Dossiers sources fusionnés (déplacement : retirés s'ils se vident)
    pub vides: Vec<String>,
    /// Pour l'historique (le même que l'opération d'origine)
    pub trace: crate::app::Trace,
    /// À sortir de la corbeille après l'opération (voir App::lancer_puis)
    pub apres: Vec<(String, String)>,
}

impl Conflits {
    /// Choix pour le conflit affiché (`tous` : pour lui et tous ceux qui restent sans réponse).
    /// Vrai quand tout est répondu.
    pub fn choisir(&mut self, c: Choix, tous: bool) -> bool {
        if tous {
            for (k, x) in self.liste.iter_mut().enumerate() {
                if k == self.rang || x.choix.is_none() {
                    x.choix = Some(c);
                }
            }
        } else if let Some(x) = self.liste.get_mut(self.rang) {
            x.choix = Some(c);
        }
        match self.liste.iter().position(|x| x.choix.is_none()) {
            Some(k) => {
                self.rang = k;
                false
            }
            None => true,
        }
    }

    pub fn deplacer(&mut self, pas: isize) {
        let n = self.liste.len() as isize;
        if n > 0 {
            self.rang = (self.rang as isize + pas).rem_euclid(n) as usize;
        }
    }

    /// Les éléments à envoyer au moteur de copie
    pub fn elements(self) -> Vec<Element> {
        let mut v = self.libres;
        let mut pris: HashSet<String> = HashSet::new();
        for x in self.liste {
            let parent = dossier::dossier_de(&x.cible.chemin).to_string();
            let choix = match x.choix.unwrap_or(Choix::Ignorer) {
                Choix::PlusRecent if x.source.entree.modif > x.cible.entree.modif => Choix::Remplacer,
                Choix::PlusRecent => Choix::Ignorer,
                c => c,
            };
            match choix {
                Choix::Remplacer => v.push(Element { source: x.source.chemin, dest: Some(parent), nom: None, remplace: Some(x.cible.chemin) }),
                Choix::Garder => {
                    let nom = nom_libre(&parent, &x.source.entree.nom, x.source.entree.est_conteneur(), &mut pris);
                    v.push(Element { source: x.source.chemin, dest: Some(parent), nom: Some(nom), remplace: None });
                }
                _ => {}
            }
        }
        v
    }
}

/// « nom (2).ext », « nom (3).ext »… : le premier libre dans `dossier` et pas déjà pris par ce lot
pub fn nom_libre(dossier: &str, nom: &str, conteneur: bool, pris: &mut HashSet<String>) -> String {
    let (base, ext) = match nom.rfind('.') {
        Some(i) if i > 0 && !conteneur => (&nom[..i], &nom[i..]),
        _ => (nom, ""),
    };
    for n in 2.. {
        let essai = format!("{base} ({n}){ext}");
        let cle = format!("{}\\{}", dossier.trim_end_matches('\\'), essai).to_lowercase();
        if !pris.contains(&cle) && std::fs::symlink_metadata(dossier::joindre(dossier, &essai)).is_err() {
            pris.insert(cle);
            return essai;
        }
    }
    unreachable!()
}

/// Sépare les sources en éléments libres et en conflits. Rien à demander : `liste` est vide.
pub fn preparer(genre: Genre, sources: &[String], dest: &str, trace: crate::app::Trace) -> Conflits {
    let mut c = Conflits { genre, dest: dest.to_string(), libres: Vec::new(), liste: Vec::new(), rang: 0, vides: Vec::new(), trace, apres: Vec::new() };
    for s in sources {
        parcourir(&mut c, s, dest, 0);
    }
    c
}

fn parcourir(c: &mut Conflits, source: &str, dest: &str, profondeur: usize) {
    let libre = |c: &mut Conflits| c.libres.push(Element::simple(source.to_string(), Some(dest.to_string())));
    let nom = source.trim_end_matches('\\').rsplit('\\').next().unwrap_or("");
    // Racine d'un disque, même dossier (copie « - Copie »), ou dossier copié dans lui-même : à Windows
    let source_bas = source.trim_end_matches('\\').to_lowercase();
    let dest_bas = dest.trim_end_matches('\\').to_lowercase();
    if nom.is_empty() || nom.ends_with(':') || dossier::dossier_de(source).trim_end_matches('\\').eq_ignore_ascii_case(dest.trim_end_matches('\\')) || dest_bas.starts_with(&format!("{source_bas}\\")) {
        return libre(c);
    }
    let cible = dossier::joindre(dest, nom);
    let (Ok(mc), Ok(ms)) = (std::fs::symlink_metadata(&cible), std::fs::symlink_metadata(source)) else { return libre(c) };
    // Deux vrais dossiers (pas des liens) : on fusionne en descendant dedans
    let lien = |m: &std::fs::Metadata| m.file_attributes() & 0x400 != 0; // FILE_ATTRIBUTE_REPARSE_POINT
    if ms.is_dir() && mc.is_dir() && !lien(&ms) && !lien(&mc) && profondeur < 64 {
        if let Ok(rd) = std::fs::read_dir(source) {
            for e in rd.flatten() {
                parcourir(c, &e.path().to_string_lossy(), &cible, profondeur + 1);
            }
            if c.genre == Genre::Deplacer {
                c.vides.push(source.to_string());
            }
            return;
        }
    }
    c.liste.push(Conflit { source: Fiche::lire(source, &ms), cible: Fiche::lire(&cible, &mc), choix: None });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fusion_et_conflits() {
        let base = std::env::temp_dir().join(format!("ne-conflit-{}", std::process::id()));
        let (a, b) = (base.join("a"), base.join("b"));
        for d in [a.join("photos"), b.join("photos")] {
            std::fs::create_dir_all(d).unwrap();
        }
        std::fs::write(a.join("photos").join("x.jpg"), "1").unwrap();
        std::fs::write(a.join("photos").join("y.jpg"), "1").unwrap();
        std::fs::write(b.join("photos").join("x.jpg"), "2").unwrap();
        std::fs::write(b.join("photos").join("x (2).jpg"), "2").unwrap();
        std::fs::write(a.join("seul.txt"), "1").unwrap();
        let s = |p: &std::path::Path| p.to_string_lossy().to_string();
        let mut c = preparer(Genre::Deplacer, &[s(&a.join("photos")), s(&a.join("seul.txt"))], &s(&b), crate::app::Trace::Aucune);
        assert_eq!(c.liste.len(), 1, "seul x.jpg est en conflit");
        assert_eq!(c.libres.len(), 2, "y.jpg et seul.txt passent");
        assert_eq!(c.vides, vec![s(&a.join("photos"))]);
        assert!(c.choisir(Choix::Garder, false));
        let e = c.elements();
        let renomme = e.iter().find(|e| e.nom.is_some()).unwrap();
        assert_eq!(renomme.nom.as_deref(), Some("x (3).jpg"));
        assert_eq!(renomme.dest.as_deref(), Some(s(&b.join("photos")).as_str()));
        let _ = std::fs::remove_dir_all(base);
    }

    #[test]
    fn nom_libre_dossier() {
        let mut pris = HashSet::new();
        let d = std::env::temp_dir().to_string_lossy().to_string();
        let n = nom_libre(&d, "ne.dossier.test", true, &mut pris);
        assert!(n.starts_with("ne.dossier.test ("));
        assert_ne!(nom_libre(&d, "ne.dossier.test", true, &mut pris), n);
    }
}
