// Historique des actions sur les fichiers, pour Ctrl+Z / Ctrl+Y : commun à tous les onglets, comme dans l'Explorateur.
// Seules les actions qu'on sait défaire proprement y entrent (pas la suppression définitive).
// Défaire une action produit son inverse, rangé dans l'autre pile : Ctrl+Y défait l'inverse, et ainsi de suite.

use std::sync::Mutex;

pub enum Action {
    Renommage { dossier: String, ancien: String, nouveau: String },
    NouveauDossier { chemin: String },
    /// Dossier vide retiré (inverse de NouveauDossier : le recréer)
    DossierRetire { chemin: String },
    /// Copies créées (Ctrl+Z : à la corbeille), et ce qu'elles ont remplacé : (chemin, place dans la
    /// corbeille), remis en place après
    Copie { crees: Vec<String>, remplaces: Vec<(String, String)> },
    /// (origine, emplacement actuel), et ce que le déplacement a remplacé
    Deplacement { paires: Vec<(String, String)>, remplaces: Vec<(String, String)> },
    /// (origine, fichier dans la corbeille)
    Corbeille { paires: Vec<(String, String)> },
    /// Renommage par lot : (ancien nom, nouveau nom) dans `dossier`
    RenommageLot { dossier: String, paires: Vec<(String, String)> },
}

impl Action {
    pub fn libelle(&self) -> &'static str {
        match self {
            Action::Renommage { .. } => "RENOMMAGE",
            Action::RenommageLot { .. } => "RENOMMAGE PAR LOT",
            Action::NouveauDossier { .. } | Action::DossierRetire { .. } => "NOUVEAU DOSSIER",
            Action::Copie { .. } => "COPIE",
            Action::Deplacement { .. } => "DEPLACEMENT",
            Action::Corbeille { .. } => "MISE A LA CORBEILLE",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Pile {
    /// Ctrl+Z
    Annuler,
    /// Ctrl+Y
    Refaire,
}

impl Pile {
    pub fn autre(self) -> Pile {
        match self {
            Pile::Annuler => Pile::Refaire,
            Pile::Refaire => Pile::Annuler,
        }
    }
}

const MAX: usize = 50;
/// (action, libellé de l'action d'origine, identifiant de son entrée du journal) : l'inverse d'une copie garde le libellé « COPIE »
/// et l'identifiant, pour que le panneau Historique sache laquelle de ses entrées vient d'être défaite ou refaite (0 : sans entrée)
static PILES: Mutex<[Vec<(Action, &'static str, u64)>; 2]> = Mutex::new([Vec::new(), Vec::new()]);

/// Range une action dans une pile, sans toucher à l'autre
pub fn empiler(pile: Pile, a: Action, libelle: &'static str, id: u64) {
    let perdu = {
        let mut p = PILES.lock().unwrap_or_else(|e| e.into_inner());
        let v = &mut p[pile as usize];
        let perdu = if v.len() >= MAX { Some(v.remove(0).2) } else { None };
        v.push((a, libelle, id));
        perdu
    };
    // Poussée hors de la pile Ctrl+Z (50 au plus) : son entrée n'a plus de bouton « Annuler »
    if let (Some(id), Pile::Annuler) = (perdu, pile) {
        crate::journal_historique::marquer(id, crate::journal_historique::Etat::Passe);
    }
}

/// Nouvelle action de l'utilisateur : on ne peut plus refaire ce qui avait été défait. Rend l'identifiant de son entrée du journal :
/// la palette à étapes défait par lui sa propre action (`App::defaire_entree`), jamais « la dernière de la pile ».
pub fn noter(a: Action) -> u64 {
    let libelle = a.libelle();
    crate::journal::operation(libelle, &a.detail());
    let (titre, detail) = a.entree_journal();
    let id = crate::journal_historique::enregistrer(&titre, &detail);
    PILES.lock().unwrap_or_else(|e| e.into_inner())[Pile::Refaire as usize].clear();
    empiler(Pile::Annuler, a, libelle, id);
    id
}

/// Nom d'un chemin : dernier élément, ou le disque (« D: ») pour une racine
fn nom_de(c: &str) -> String {
    c.trim_end_matches('\\').rsplit('\\').next().unwrap_or(c).to_string()
}

/// Nom du dossier qui contient `c`
fn parent_de(c: &str) -> String {
    nom_de(c.trim_end_matches('\\').rsplit_once('\\').map_or("", |p| p.0))
}

impl Action {
    /// « PHOTO.JPG + 2 » : de quoi reconnaître l'action dans l'historique
    pub fn detail(&self) -> String {
        let nom = |c: &str| nom_de(c);
        let plusieurs = |premier: Option<String>, n: usize| match (premier, n) {
            (Some(p), n) if n > 1 => format!("{p} + {}", n - 1),
            (Some(p), _) => p,
            (None, _) => String::new(),
        };
        match self {
            Action::Renommage { ancien, nouveau, .. } => format!("{ancien} → {nouveau}"),
            Action::RenommageLot { paires, .. } => plusieurs(paires.first().map(|p| format!("{} → {}", p.0, p.1)), paires.len()),
            Action::NouveauDossier { chemin } | Action::DossierRetire { chemin } => nom(chemin),
            Action::Copie { crees, .. } => plusieurs(crees.first().map(|c| nom(c)), crees.len()),
            Action::Deplacement { paires, .. } | Action::Corbeille { paires } => plusieurs(paires.first().map(|p| nom(&p.0)), paires.len()),
        }
    }

    /// Les deux lignes de l'entrée du panneau Historique (`HIST` de la maquette) : « Copie · capture-hub.png » puis, en petit,
    /// « Bureau → Documents ». Les dossiers sont nommés par leur dernier élément.
    pub fn entree_journal(&self) -> (String, String) {
        let quoi = |premier: Option<String>, n: usize| match (premier, n) {
            (Some(p), n) if n > 1 => format!("{n} éléments · {p} …"),
            (Some(p), _) => p,
            (None, _) => String::new(),
        };
        match self {
            Action::Renommage { dossier, ancien, nouveau } => (format!("Renommage · {nouveau}"), format!("{ancien} → {nouveau} · {}", nom_de(dossier))),
            Action::RenommageLot { dossier, paires } => {
                let (n, premier) = (paires.len(), paires.first());
                let titre = if n > 1 { format!("Renommage par lot · {n} éléments") } else { format!("Renommage par lot · {}", premier.map_or("", |p| p.1.as_str())) };
                (titre, format!("{} → {} · {}", premier.map_or("", |p| p.0.as_str()), premier.map_or("", |p| p.1.as_str()), nom_de(dossier)))
            }
            Action::NouveauDossier { chemin } | Action::DossierRetire { chemin } => (format!("Nouveau dossier · {}", nom_de(chemin)), parent_de(chemin)),
            Action::Copie { crees, .. } => (format!("Copie · {}", quoi(crees.first().map(|c| nom_de(c)), crees.len())), crees.first().map_or(String::new(), |c| format!("→ {}", parent_de(c)))),
            Action::Deplacement { paires, .. } => (
                format!("Déplacement · {}", quoi(paires.first().map(|p| nom_de(&p.0)), paires.len())),
                paires.first().map_or(String::new(), |p| format!("{} → {}", parent_de(&p.0), parent_de(&p.1))),
            ),
            Action::Corbeille { paires } => (format!("Corbeille · {}", quoi(paires.first().map(|p| nom_de(&p.0)), paires.len())), paires.first().map_or(String::new(), |p| parent_de(&p.0))),
        }
    }
}

/// Remonte en haut de la pile l'action de l'entrée `id`, pour la défaire sans toucher aux autres ; faux si elle n'y est plus
pub fn remonter_id(pile: Pile, id: u64) -> bool {
    let mut p = PILES.lock().unwrap_or_else(|e| e.into_inner());
    let v = &mut p[pile as usize];
    match v.iter().position(|c| id != 0 && c.2 == id) {
        Some(i) => {
            let c = v.remove(i);
            v.push(c);
            true
        }
        None => false,
    }
}

/// Dernière action de la pile, retirée
pub fn reprendre(pile: Pile) -> Option<(Action, &'static str, u64)> {
    PILES.lock().unwrap_or_else(|e| e.into_inner())[pile as usize].pop()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::journal_historique::{instantane, Etat};

    fn p(a: &str, b: &str) -> (String, String) {
        (a.to_string(), b.to_string())
    }

    /// Une entrée par type d'action : libellé « Verbe · nom » et détail en petit dessous, comme `HIST` de la maquette
    #[test]
    fn entree_du_journal_par_type_d_action() {
        let r = Action::Renommage { dossier: r"C:\Users\a\Images".into(), ancien: "logo.png".into(), nouveau: "logo-v2.png".into() };
        assert_eq!(r.entree_journal(), ("Renommage · logo-v2.png".to_string(), "logo.png → logo-v2.png · Images".to_string()));
        let n = Action::NouveauDossier { chemin: r"C:\Users\a\Documents\Projets".into() };
        assert_eq!(n.entree_journal(), ("Nouveau dossier · Projets".to_string(), "Documents".to_string()));
        let c = Action::Copie { crees: vec![r"C:\Users\a\Documents\capture-hub.png".into()], remplaces: Vec::new() };
        assert_eq!(c.entree_journal(), ("Copie · capture-hub.png".to_string(), "→ Documents".to_string()));
        let d = Action::Deplacement { paires: vec![p(r"C:\Users\a\Bureau\a.txt", r"D:\Docs\a.txt"), p(r"C:\Users\a\Bureau\b.txt", r"D:\Docs\b.txt"), p(r"C:\Users\a\Bureau\c.txt", r"D:\Docs\c.txt")], remplaces: Vec::new() };
        assert_eq!(d.entree_journal(), ("Déplacement · 3 éléments · a.txt …".to_string(), "Bureau → Docs".to_string()));
        let t = Action::Corbeille { paires: vec![p(r"C:\Users\a\Documents\ancien.txt", r"C:\$Recycle.Bin\S\$R1.txt")] };
        assert_eq!(t.entree_journal(), ("Corbeille · ancien.txt".to_string(), "Documents".to_string()));
        let l = Action::RenommageLot { dossier: r"D:\".into(), paires: vec![p("a.jpg", "photo-1.jpg"), p("b.jpg", "photo-2.jpg")] };
        assert_eq!(l.entree_journal(), ("Renommage par lot · 2 éléments".to_string(), "a.jpg → photo-1.jpg · D:".to_string()));
        // Un disque à la racine se nomme « D: » ; le détail court garde son format d'avant
        assert_eq!(parent_de(r"D:\x.txt"), "D:");
        assert_eq!(d.detail(), "a.txt + 2");
    }

    /// `noter` crée l'entrée annulable, Ctrl+Z / « Annuler » la marque annulée, une pile pleine ou un échec la rend définitive.
    /// Un seul test touche aux piles (elles sont communes à tout le processus).
    #[test]
    fn noter_lie_l_entree_a_la_pile() {
        noter(Action::NouveauDossier { chemin: r"C:\essai\Nouveau dossier".into() });
        let e = instantane().into_iter().find(|e| e.libelle == "Nouveau dossier · Nouveau dossier").expect("entrée notée");
        assert_eq!(e.etat, Etat::Annulable);
        assert_ne!(e.id, 0);
        // Le bouton « Annuler » : l'action remonte en tête de pile, puis se reprend avec le même identifiant
        assert!(!remonter_id(Pile::Annuler, 0));
        assert!(!remonter_id(Pile::Annuler, u64::MAX));
        assert!(remonter_id(Pile::Annuler, e.id));
        let (_, libelle, id) = reprendre(Pile::Annuler).expect("action reprise");
        assert_eq!((libelle, id), ("NOUVEAU DOSSIER", e.id));
        // Défaite : le journal affiche « Annulé · … » et le ✓
        crate::journal_historique::marquer(id, Etat::Annule);
        let e = instantane().into_iter().find(|x| x.id == id).unwrap();
        assert_eq!(e.etat, Etat::Annule);
        assert_eq!(e.libelle_affiche(), "Annulé · Nouveau dossier · Nouveau dossier");
        // Pile pleine : la plus ancienne action perd son bouton
        let premier = crate::journal_historique::enregistrer("Plus ancienne", "");
        empiler(Pile::Annuler, Action::NouveauDossier { chemin: r"C:\a".into() }, "NOUVEAU DOSSIER", premier);
        for k in 0..MAX {
            noter(Action::NouveauDossier { chemin: format!(r"C:\essai\{k}") });
        }
        assert_eq!(instantane().into_iter().find(|x| x.id == premier).unwrap().etat, Etat::Passe);
        while reprendre(Pile::Annuler).is_some() {}
    }
}

/// Sort des éléments de la corbeille vers leur place d'origine. Renvoie le nombre restauré.
/// La corbeille garde chaque élément sous `$R…` avec sa fiche `$I…` (nom et date d'origine) : on remet
/// `$R…` à sa place et on retire la fiche, exactement ce que fait « Restaurer ».
pub fn restaurer(paires: &[(String, String)]) -> Result<usize, String> {
    let mut n = 0;
    let mut erreur = None;
    for (origine, corbeille) in paires {
        if std::path::Path::new(origine).exists() {
            erreur = Some("UN ELEMENT PORTE DEJA CE NOM A SA PLACE D'ORIGINE".to_string());
            continue;
        }
        match crate::operations::deplacer_sans_ecraser(corbeille, origine) {
            Ok(()) => {
                n += 1;
                if let Some((dossier, nom)) = corbeille.rsplit_once('\\') {
                    if let Some(reste) = nom.strip_prefix("$R") {
                        let _ = std::fs::remove_file(format!("{dossier}\\$I{reste}"));
                    }
                }
            }
            Err(_) => erreur = Some("ELEMENT INTROUVABLE DANS LA CORBEILLE".to_string()),
        }
    }
    match erreur {
        Some(e) if n == 0 => Err(e),
        _ => Ok(n),
    }
}
