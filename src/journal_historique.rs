// Journal des actions du panneau Historique (lot 15B, `HIST` de la maquette) : une entrée par action que `historique.rs` sait
// défaire (renommage, nouveau dossier, copie, déplacement, corbeille, renommage par lot). Il survit au redémarrage dans
// `%APPDATA%\SpaceFiles\historique.txt` (texte, une ligne par entrée, 200 au plus, écriture atomique). Les piles d'annulation
// vivent en mémoire : une entrée relue d'une session précédente n'est plus annulable (✓). Le fichier n'est lu qu'à la première
// utilisation (ouverture du panneau ou première action), jamais au démarrage ; aucun accès disque pendant les tests.

use std::sync::Mutex;

/// Entrées gardées (les plus anciennes sont oubliées)
pub const MAX_ENTREES: usize = 200;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Etat {
    /// Le bouton « Annuler » : l'action est encore dans la pile Ctrl+Z de la session
    Annulable,
    /// Déjà annulée (« Annulé · … », ✓)
    Annule,
    /// Plus annulable (session précédente, pile pleine) : ✓
    Passe,
}

impl Etat {
    fn code(self) -> char {
        match self {
            Etat::Passe => '0',
            Etat::Annulable => '1',
            Etat::Annule => '2',
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entree {
    /// Lien avec la pile d'annulation (0 : entrée relue du fichier, jamais annulable)
    pub id: u64,
    /// « AAAA-MM-JJ HH:MM » (heure locale)
    pub quand: String,
    /// « Copie · capture-hub.png » (sans le préfixe « Annulé · », ajouté à l'affichage)
    pub libelle: String,
    /// « Bureau → Documents »
    pub detail: String,
    pub etat: Etat,
}

impl Entree {
    /// Libellé montré : « Annulé · » devant celui d'une action défaite (`h[1]='Annulé · '+h[1]` de la maquette)
    pub fn libelle_affiche(&self) -> String {
        if self.etat == Etat::Annule {
            format!("Annulé · {}", self.libelle)
        } else {
            self.libelle.clone()
        }
    }
}

/// Un champ tient sur une ligne et ne contient pas la tabulation qui sépare les champs
fn nettoyer(s: &str) -> String {
    s.chars().map(|c| if c.is_control() { ' ' } else { c }).collect::<String>().trim().to_string()
}

/// « quand ⇥ état ⇥ libellé ⇥ détail »
pub fn ligne(e: &Entree) -> String {
    format!("{}\t{}\t{}\t{}", nettoyer(&e.quand), e.etat.code(), nettoyer(&e.libelle), nettoyer(&e.detail))
}

/// Relit une ligne ; `None` si elle est corrompue (champs manquants, date illisible, état inconnu). Une entrée « annulable » du
/// fichier devient « passée » : sa pile d'annulation n'existe plus.
pub fn lire_ligne(l: &str) -> Option<Entree> {
    let mut champs = l.trim_end_matches(['\r', '\n']).splitn(4, '\t');
    let quand = champs.next()?.trim();
    let etat = match champs.next()? {
        "0" | "1" => Etat::Passe,
        "2" => Etat::Annule,
        _ => return None,
    };
    let libelle = champs.next()?.trim();
    let detail = champs.next()?.trim();
    if libelle.is_empty() || analyser_date(quand).is_none() {
        return None;
    }
    Some(Entree { id: 0, quand: quand.to_string(), libelle: libelle.to_string(), detail: detail.to_string(), etat })
}

/// Le fichier entier, du plus ancien au plus récent : les lignes corrompues sont ignorées, au plus `MAX_ENTREES` gardées (les dernières)
pub fn lire(texte: &str) -> Vec<Entree> {
    let mut v: Vec<Entree> = texte.trim_start_matches('\u{feff}').lines().filter_map(lire_ligne).collect();
    if v.len() > MAX_ENTREES {
        v.drain(..v.len() - MAX_ENTREES);
    }
    v
}

/// Le texte du fichier pour ces entrées (les `MAX_ENTREES` dernières)
pub fn ecrire(entrees: &[Entree]) -> String {
    let debut = entrees.len().saturating_sub(MAX_ENTREES);
    entrees[debut..].iter().map(|e| format!("{}\r\n", ligne(e))).collect()
}

/// (année, mois, jour, heure, minute) de « AAAA-MM-JJ HH:MM[:SS] »
fn analyser_date(s: &str) -> Option<(i64, i64, i64, i64, i64)> {
    let (date, heure) = s.split_once(' ')?;
    let mut d = date.split('-');
    let (a, m, j) = (d.next()?.parse().ok()?, d.next()?.parse().ok()?, d.next()?.parse().ok()?);
    let mut h = heure.split(':');
    let (hh, mm): (i64, i64) = (h.next()?.parse().ok()?, h.next()?.parse().ok()?);
    if d.next().is_some() || !(1..=12).contains(&m) || !(1..=31).contains(&j) || !(0..24).contains(&hh) || !(0..60).contains(&mm) {
        return None;
    }
    Some((a, m, j, hh, mm))
}

/// Numéro du jour (calendrier grégorien) pour comparer des dates sans bibliothèque
fn jours(a: i64, m: i64, j: i64) -> i64 {
    let a = if m <= 2 { a - 1 } else { a };
    let ere = a.div_euclid(400);
    let annee_ere = a - ere * 400;
    let mp = (m + 9) % 12;
    let jour_annee = (153 * mp + 2) / 5 + j - 1;
    ere * 146_097 + annee_ere * 365 + annee_ere / 4 - annee_ere / 100 + jour_annee
}

/// L'heure d'une entrée comme le montre la colonne de 40 DIP : « 09:12 » aujourd'hui, « Hier », sinon « 07/11 ».
/// `maintenant` : « AAAA-MM-JJ HH:MM » (passé en paramètre pour que la règle se teste).
pub fn heure_affichee(quand: &str, maintenant: &str) -> String {
    let (Some((a, m, j, hh, mm)), Some((na, nm, nj, _, _))) = (analyser_date(quand), analyser_date(maintenant)) else {
        return String::new();
    };
    match jours(na, nm, nj) - jours(a, m, j) {
        0 => format!("{hh:02}:{mm:02}"),
        1 => "Hier".to_string(),
        _ => format!("{j:02}/{m:02}"),
    }
}

/// L'horloge locale au format du journal
pub fn maintenant() -> String {
    crate::journal::horodatage().chars().take(16).collect()
}

/// Les entrées en mémoire (du plus ancien au plus récent)
pub struct Journal {
    charge: bool,
    entrees: Vec<Entree>,
    prochain: u64,
}

impl Journal {
    pub const fn vide() -> Journal {
        Journal { charge: false, entrees: Vec::new(), prochain: 1 }
    }

    /// Nouvelle action de la session : annulable tant que sa pile la garde. Rend son identifiant.
    pub fn ajouter(&mut self, quand: &str, libelle: &str, detail: &str) -> u64 {
        let id = self.prochain;
        self.prochain += 1;
        self.entrees.push(Entree { id, quand: quand.to_string(), libelle: nettoyer(libelle), detail: nettoyer(detail), etat: Etat::Annulable });
        if self.entrees.len() > MAX_ENTREES {
            self.entrees.remove(0);
        }
        id
    }

    /// Change l'état de l'entrée `id` ; faux si elle n'existe plus (oubliée au-delà de 200) ou si `id` vaut 0
    pub fn marquer(&mut self, id: u64, etat: Etat) -> bool {
        match self.entrees.iter_mut().find(|e| id != 0 && e.id == id) {
            Some(e) => {
                e.etat = etat;
                true
            }
            None => false,
        }
    }

    /// Du plus récent au plus ancien (l'ordre du tiroir : `HIST.unshift`)
    pub fn recentes_d_abord(&self) -> Vec<Entree> {
        self.entrees.iter().rev().cloned().collect()
    }

    pub fn entrees(&self) -> &[Entree] {
        &self.entrees
    }

    /// Entrées du fichier, placées avant celles de la session
    fn precharger(&mut self, anciennes: Vec<Entree>) {
        if self.charge {
            return;
        }
        self.charge = true;
        let mut tout = anciennes;
        tout.append(&mut self.entrees);
        if tout.len() > MAX_ENTREES {
            tout.drain(..tout.len() - MAX_ENTREES);
        }
        self.entrees = tout;
    }
}

static JOURNAL: Mutex<Journal> = Mutex::new(Journal::vide());

fn chemin_fichier() -> Option<std::path::PathBuf> {
    Some(crate::reglages::dossier_donnees(false)?.join("historique.txt"))
}

/// Lit le fichier une seule fois, à la première utilisation. Pas d'accès disque pendant les tests.
fn charger(j: &mut Journal) {
    if j.charge {
        return;
    }
    let anciennes = if cfg!(test) { Vec::new() } else { chemin_fichier().and_then(|p| std::fs::read(p).ok()).map(|o| lire(&String::from_utf8_lossy(&o))).unwrap_or_default() };
    j.precharger(anciennes);
}

/// Écriture atomique (fichier voisin puis renommage) : un arrêt en cours d'écriture laisse l'ancien contenu
fn sauver(j: &Journal) {
    if cfg!(test) {
        return;
    }
    let Some(p) = chemin_fichier() else { return };
    if let Some(d) = p.parent() {
        let _ = std::fs::create_dir_all(d);
    }
    let tmp = p.with_extension("txt.tmp");
    if std::fs::write(&tmp, ecrire(j.entrees())).is_ok() && std::fs::rename(&tmp, &p).is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
}

/// Une action de l'utilisateur vient d'entrer dans l'historique : rend l'identifiant qui la lie à la pile d'annulation
pub fn enregistrer(libelle: &str, detail: &str) -> u64 {
    let mut j = JOURNAL.lock().unwrap_or_else(|e| e.into_inner());
    charger(&mut j);
    let id = j.ajouter(&maintenant(), libelle, detail);
    sauver(&j);
    id
}

/// L'état d'une entrée change (annulée, refaite, plus annulable)
pub fn marquer(id: u64, etat: Etat) {
    if id == 0 {
        return;
    }
    let mut j = JOURNAL.lock().unwrap_or_else(|e| e.into_inner());
    charger(&mut j);
    if j.marquer(id, etat) {
        sauver(&j);
    }
}

/// Les entrées à montrer, la plus récente d'abord ; lit le fichier la première fois (ouverture du panneau)
pub fn instantane() -> Vec<Entree> {
    let mut j = JOURNAL.lock().unwrap_or_else(|e| e.into_inner());
    charger(&mut j);
    j.recentes_d_abord()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn e(id: u64, quand: &str, libelle: &str, detail: &str, etat: Etat) -> Entree {
        Entree { id, quand: quand.into(), libelle: libelle.into(), detail: detail.into(), etat }
    }

    #[test]
    fn ligne_aller_retour() {
        let x = e(0, "2026-11-09 09:12", "Copie · capture-hub.png", "Bureau → Documents", Etat::Annule);
        assert_eq!(lire_ligne(&ligne(&x)), Some(x));
    }

    #[test]
    fn une_entree_annulable_du_fichier_n_est_plus_annulable() {
        let x = e(7, "2026-11-09 09:12", "Renommage · a.txt", "Images", Etat::Annulable);
        let relue = lire_ligne(&ligne(&x)).unwrap();
        assert_eq!(relue.etat, Etat::Passe);
        assert_eq!(relue.id, 0);
    }

    #[test]
    fn texte_entier_aller_retour() {
        let v = vec![e(0, "2026-11-08 10:00", "A", "x", Etat::Passe), e(0, "2026-11-09 11:30", "B · c", "y → z", Etat::Annule)];
        assert_eq!(lire(&ecrire(&v)), v);
    }

    #[test]
    fn lignes_corrompues_ignorees() {
        let texte = "n'importe quoi\r\n2026-11-09 09:12\t1\tCopie · a\tBureau\r\n2026-13-45 99:99\t0\tX\ty\r\n2026-11-09 09:13\t9\tEtat inconnu\ty\r\n2026-11-09 09:14\t0\r\n\r\n2026-11-09 09:15\t0\t\tlibelle vide\r\n2026-11-09 09:16\t2\tB\td\r\n";
        let v = lire(texte);
        assert_eq!(v.len(), 2);
        assert_eq!(v[0].libelle, "Copie · a");
        assert_eq!(v[1].etat, Etat::Annule);
    }

    #[test]
    fn bornage_a_200_en_gardant_les_dernieres() {
        let mut texte = String::new();
        for k in 0..250 {
            texte.push_str(&format!("2026-11-09 09:12\t0\tAction {k}\td\r\n"));
        }
        let v = lire(&texte);
        assert_eq!(v.len(), MAX_ENTREES);
        assert_eq!(v[0].libelle, "Action 50");
        assert_eq!(v[199].libelle, "Action 249");
        // L'écriture borne aussi
        let beaucoup: Vec<Entree> = (0..300).map(|k| e(0, "2026-11-09 09:12", &format!("A{k}"), "", Etat::Passe)).collect();
        assert_eq!(ecrire(&beaucoup).lines().count(), MAX_ENTREES);
        // Et la mémoire
        let mut j = Journal::vide();
        for k in 0..210 {
            j.ajouter("2026-11-09 09:12", &format!("A{k}"), "");
        }
        assert_eq!(j.entrees().len(), MAX_ENTREES);
        assert_eq!(j.entrees()[0].libelle, "A10");
    }

    #[test]
    fn un_champ_ne_casse_pas_la_ligne() {
        let x = e(1, "2026-11-09 09:12", "Nom\tavec\ttabulations", "deux\nlignes", Etat::Passe);
        let relue = lire_ligne(&ligne(&x)).unwrap();
        assert_eq!(relue.libelle, "Nom avec tabulations");
        assert_eq!(relue.detail, "deux lignes");
    }

    #[test]
    fn heure_ou_hier() {
        let maintenant = "2026-11-09 14:00";
        assert_eq!(heure_affichee("2026-11-09 09:12", maintenant), "09:12");
        assert_eq!(heure_affichee("2026-11-08 23:59", maintenant), "Hier");
        assert_eq!(heure_affichee("2026-11-07 10:00", maintenant), "07/11");
        // Changement de mois et d'année
        assert_eq!(heure_affichee("2026-10-31 10:00", "2026-11-01 08:00"), "Hier");
        assert_eq!(heure_affichee("2025-12-31 22:00", "2026-01-01 00:05"), "Hier");
        assert_eq!(heure_affichee("2024-02-29 10:00", "2024-03-01 08:00"), "Hier");
        assert_eq!(heure_affichee("illisible", maintenant), "");
    }

    #[test]
    fn annule_prefixe_le_libelle() {
        let mut x = e(1, "2026-11-09 09:12", "Copie · a.png", "d", Etat::Annulable);
        assert_eq!(x.libelle_affiche(), "Copie · a.png");
        x.etat = Etat::Annule;
        assert_eq!(x.libelle_affiche(), "Annulé · Copie · a.png");
    }

    #[test]
    fn marquer_une_entree() {
        let mut j = Journal::vide();
        let a = j.ajouter("2026-11-09 09:12", "A", "");
        let b = j.ajouter("2026-11-09 09:13", "B", "");
        assert!(j.marquer(a, Etat::Annule));
        assert!(!j.marquer(0, Etat::Annule));
        assert!(!j.marquer(99, Etat::Annule));
        let v = j.recentes_d_abord();
        assert_eq!((v[0].id, v[0].etat), (b, Etat::Annulable));
        assert_eq!((v[1].id, v[1].etat), (a, Etat::Annule));
    }

    #[test]
    fn le_fichier_passe_avant_la_session() {
        let mut j = Journal::vide();
        j.ajouter("2026-11-09 09:12", "Session", "");
        j.precharger(vec![e(0, "2026-11-08 10:00", "Ancienne", "", Etat::Passe)]);
        let v = j.entrees();
        assert_eq!((v[0].libelle.as_str(), v[1].libelle.as_str()), ("Ancienne", "Session"));
        // Une seconde lecture ne double rien
        j.precharger(vec![e(0, "2026-11-08 10:00", "Autre", "", Etat::Passe)]);
        assert_eq!(j.entrees().len(), 2);
    }
}
