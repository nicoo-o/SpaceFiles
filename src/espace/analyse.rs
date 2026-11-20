// Page ESPACE, onglet ANALYSE (E1) : l'arbre des dossiers trié par taille, lu dans l'index enregistré.
//
// Logique pure, sans fenêtre : l'arbre est une liste à plat (une ligne par élément visible) qu'on reconstruit quand on
// ouvre ou ferme un dossier. Les tailles sont celles de l'index (logiques) ; un dossier n'y a pas de taille propre, donc
// `Index::tailles_cumulees` (un passage, 26 ms pour 2,84 M d'éléments, 21,7 Mo) donne la taille de chacun. Ce tableau est
// tenu par l'`Arbre` : le fermer (quitter la page) libère la mémoire.

use std::collections::HashSet;
use std::sync::Arc;

use crate::index::Index;

/// Une ligne visible de l'arbre
#[derive(Clone, Debug, PartialEq)]
pub struct Ligne {
    /// Numéro du nœud dans l'index
    pub id: u32,
    /// Profondeur : 0 pour un disque
    pub niveau: u16,
    pub nom: String,
    pub dossier: bool,
    /// Taille cumulée (octets)
    pub taille: u64,
    /// Part de la taille du dossier parent (des disques, pour un disque) : de 0,0 à 1,0
    pub part: f32,
    pub ouvert: bool,
    /// Texte discret à côté du nom (bilans E3) : le dossier d'un fichier, le nombre de fichiers d'un groupe ; vide dans l'arbre
    pub detail: String,
    /// Bilans E3 : le fichier que la ligne montre (Entrée ouvre son dossier) ; `None` pour une ligne d'arbre ou un groupe
    pub fichier: Option<u32>,
    /// Bilans E3 : genre dont la couleur marque la ligne ; `None` dans l'arbre et pour les groupes d'âge
    pub pastille: Option<Genre>,
}

/// Nombre maximal de rectangles dans la carte d'un dossier : au-delà, les plus petits se regroupent en un seul « autres »
pub const CELLULES_MAX: usize = 60;

/// Genre d'un élément de la carte : une couleur par genre, lue dans la légende (18/11/2026, choix de l'auteur)
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Genre {
    Dossier,
    Image,
    Video,
    Audio,
    Document,
    Archive,
    Code,
    Programme,
    Autre,
}

/// Les genres dans l'ordre de la légende
pub const GENRES: [Genre; 9] =
    [Genre::Dossier, Genre::Image, Genre::Video, Genre::Audio, Genre::Document, Genre::Archive, Genre::Code, Genre::Programme, Genre::Autre];

impl Genre {
    /// Mot de la légende
    pub fn libelle(self) -> &'static str {
        match self {
            Genre::Dossier => "Dossier",
            Genre::Image => "Image",
            Genre::Video => "Vidéo",
            Genre::Audio => "Audio",
            Genre::Document => "Document",
            Genre::Archive => "Archive",
            Genre::Code => "Code",
            Genre::Programme => "Programme",
            Genre::Autre => "Autre",
        }
    }

    /// Genre d'un nom de fichier, d'après son extension (sans la casse) ; un dossier est toujours `Dossier`
    pub fn de(nom: &str, dossier: bool) -> Genre {
        if dossier {
            return Genre::Dossier;
        }
        let Some((_, ext)) = nom.rsplit_once('.') else { return Genre::Autre };
        // Minuscules sans allouer : les bilans (E3) appellent ceci pour chacun des millions de fichiers de l'index.
        // Une extension de plus de 8 octets n'est dans aucune liste ci-dessous.
        let mut tampon = [0u8; 8];
        let Some(dest) = tampon.get_mut(..ext.len()) else { return Genre::Autre };
        dest.copy_from_slice(ext.as_bytes());
        dest.make_ascii_lowercase();
        let Ok(ext) = std::str::from_utf8(dest) else { return Genre::Autre };
        match ext {
            "jpg" | "jpeg" | "png" | "gif" | "bmp" | "webp" | "heic" | "tif" | "tiff" | "svg" | "ico" | "raw" | "psd" => Genre::Image,
            "mp4" | "mkv" | "avi" | "mov" | "wmv" | "webm" | "flv" | "m4v" => Genre::Video,
            "mp3" | "wav" | "flac" | "aac" | "ogg" | "m4a" | "wma" | "opus" => Genre::Audio,
            "pdf" | "doc" | "docx" | "xls" | "xlsx" | "ppt" | "pptx" | "txt" | "md" | "odt" | "ods" | "rtf" | "csv" | "epub" => Genre::Document,
            "zip" | "rar" | "7z" | "tar" | "gz" | "bz2" | "xz" | "iso" | "cab" | "wim" | "vhd" | "vhdx" | "img" => Genre::Archive,
            "rs" | "js" | "ts" | "py" | "c" | "h" | "cpp" | "cs" | "java" | "go" | "json" | "toml" | "yaml" | "yml" | "html" | "css" | "xml" | "ps1" | "sh" | "bat" | "lock" | "map" => Genre::Code,
            "exe" | "dll" | "msi" | "sys" | "bin" | "dat" | "pak" | "msix" | "appx" | "so" | "lib" | "pdb" | "obj" => Genre::Programme,
            _ => Genre::Autre,
        }
    }
}

/// Un rectangle de la carte : un élément du dossier montré, ou le regroupement des plus petits
#[derive(Clone, Debug, PartialEq)]
pub struct Cellule {
    /// Numéro du nœud dans l'index ; `None` pour le regroupement « autres »
    pub id: Option<u32>,
    pub nom: String,
    pub dossier: bool,
    pub genre: Genre,
    pub taille: u64,
    /// Part de la taille du dossier montré : de 0,0 à 1,0
    pub part: f32,
}

pub struct Arbre {
    index: Arc<Index>,
    cumul: Vec<u64>,
    ouverts: HashSet<u32>,
    lignes: Vec<Ligne>,
    /// Taille de tout ce que l'index contient (somme des disques)
    pub total: u64,
    /// Bilans par type, gros fichiers et âge (E3) : calculés au premier besoin, gardés tant que l'arbre vit
    bilans: Option<Bilans>,
}

/// Nombre de fichiers montrés dans la liste « gros fichiers »
pub const GROS_MAX: usize = 100;

/// Les trois bilans de l'index (E3), chacun déjà sous forme de lignes de liste. Rien n'est supprimé ni modifié : ce sont
/// des lectures de l'index enregistré.
#[derive(Debug, Default)]
pub struct Bilans {
    /// Un groupe par genre de fichier, du plus lourd au plus léger
    pub types: Vec<Ligne>,
    /// Les `GROS_MAX` plus gros fichiers, du plus gros au plus petit
    pub gros: Vec<Ligne>,
    /// Un groupe par tranche d'âge de la date de modification, du plus récent au plus ancien, puis « date inconnue »
    pub ages: Vec<Ligne>,
}

/// Tranches d'âge, de la plus récente à la plus ancienne : bornes hautes en jours (un mois = 30 jours) et libellés
const TRANCHES: [(u64, &str); 4] = [(30, "Moins d'un mois"), (182, "1 à 6 mois"), (365, "6 mois à 1 an"), (730, "1 à 2 ans")];
const TRANCHE_ANCIENNE: &str = "Plus de 2 ans";
const TRANCHE_INCONNUE: &str = "Date inconnue";

/// Secondes entre 1601 (origine des FILETIME) et 1970 (origine unix)
const FILETIME_VERS_UNIX: u64 = 11_644_473_600;

/// Rang de la tranche d'âge d'une date de modification (FILETIME) ; `TRANCHES.len() + 1` quand la date est inconnue (0)
pub fn tranche_de(modif: u64, maintenant: u64) -> usize {
    if modif == 0 {
        return TRANCHES.len() + 1;
    }
    let modifie = (modif / 10_000_000).saturating_sub(FILETIME_VERS_UNIX);
    let jours = maintenant.saturating_sub(modifie) / 86_400;
    TRANCHES.iter().position(|&(borne, _)| jours < borne).unwrap_or(TRANCHES.len())
}

fn libelle_tranche(rang: usize) -> &'static str {
    match TRANCHES.get(rang) {
        Some(&(_, l)) => l,
        None if rang == TRANCHES.len() => TRANCHE_ANCIENNE,
        None => TRANCHE_INCONNUE,
    }
}

/// « 1 234 fichiers », « 1 fichier »
fn fichiers_en_texte(n: u64) -> String {
    if n == 1 { "1 fichier".into() } else { format!("{} fichiers", crate::systeme::nombre(n as usize)) }
}

/// Part de `taille` dans `total`, bornée à [0, 1] ; 0 quand le total est nul
pub fn part(taille: u64, total: u64) -> f32 {
    if total == 0 {
        return 0.0;
    }
    ((taille as f64 / total as f64) as f32).clamp(0.0, 1.0)
}

/// « 42 % », « < 1 % » pour une part non nulle sous 1 %, « 0 % » pour rien
pub fn pourcent(p: f32) -> String {
    if p <= 0.0 {
        "0 %".into()
    } else if p < 0.01 {
        "< 1 %".into()
    } else {
        format!("{} %", (p * 100.0).round() as u32)
    }
}

/// Âge d'une date de construction : « il y a 3 h », « il y a 2 j » (`secondes` = écart en secondes)
pub fn age(secondes: u64) -> String {
    match secondes {
        0..=59 => "à l'instant".into(),
        60..=3599 => format!("il y a {} min", secondes / 60),
        3600..=86_399 => format!("il y a {} h", secondes / 3600),
        _ => format!("il y a {} j", secondes / 86_400),
    }
}

impl Arbre {
    /// Arbre sur l'index donné : les disques, du plus gros au plus petit, tous fermés
    pub fn nouveau(index: Arc<Index>) -> Self {
        let cumul = index.tailles_cumulees();
        let total = index.racines().iter().map(|&r| cumul[r as usize]).fold(0u64, u64::saturating_add);
        let mut a = Self { index, cumul, ouverts: HashSet::new(), lignes: Vec::new(), total, bilans: None };
        a.reconstruire();
        a
    }

    pub fn lignes(&self) -> &[Ligne] {
        &self.lignes
    }

    /// Les bilans, s'ils ont déjà été calculés par `preparer_bilans`
    pub fn bilans(&self) -> Option<&Bilans> {
        self.bilans.as_ref()
    }

    /// Calcule les bilans (un seul passage sur tous les fichiers de l'index) s'ils ne le sont pas encore ; `maintenant` en
    /// secondes unix. Les dates de l'index ne changent pas tant que l'arbre vit : le résultat est gardé.
    pub fn preparer_bilans(&mut self, maintenant: u64) {
        if self.bilans.is_none() {
            self.bilans = Some(self.calculer_bilans(maintenant));
        }
    }

    fn calculer_bilans(&self, maintenant: u64) -> Bilans {
        use std::cmp::Reverse;
        use std::collections::BinaryHeap;

        let mut par_genre = [(0u64, 0u64); GENRES.len()];
        let mut par_tranche = [(0u64, 0u64); TRANCHES.len() + 2];
        // Tas à `GROS_MAX` places dont le sommet est le plus petit des gros : un fichier ne le remplace que s'il le dépasse
        let mut tas: BinaryHeap<Reverse<(u64, u32)>> = BinaryHeap::with_capacity(GROS_MAX + 1);
        for (id, nom, taille, modif) in self.index.fichiers() {
            let g = &mut par_genre[Genre::de(nom, false) as usize];
            g.0 = g.0.saturating_add(taille);
            g.1 += 1;
            let t = &mut par_tranche[tranche_de(modif, maintenant)];
            t.0 = t.0.saturating_add(taille);
            t.1 += 1;
            if tas.len() < GROS_MAX {
                tas.push(Reverse((taille, id)));
            } else if tas.peek().is_some_and(|&Reverse((petit, _))| taille > petit) {
                tas.pop();
                tas.push(Reverse((taille, id)));
            }
        }
        let total = self.total;
        let groupe = |nom: &str, (taille, n): (u64, u64), pastille: Option<Genre>| Ligne {
            id: 0,
            niveau: 0,
            nom: nom.to_string(),
            dossier: false,
            taille,
            part: part(taille, total),
            ouvert: false,
            detail: fichiers_en_texte(n),
            fichier: None,
            pastille,
        };

        let mut types: Vec<Ligne> =
            GENRES.iter().filter(|&&g| g != Genre::Dossier && par_genre[g as usize].1 > 0).map(|&g| groupe(g.libelle(), par_genre[g as usize], Some(g))).collect();
        types.sort_by(|a, b| b.taille.cmp(&a.taille).then_with(|| a.nom.cmp(&b.nom)));

        let mut gros: Vec<(u64, u32)> = tas.into_iter().map(|Reverse(x)| x).collect();
        // Même ordre que l'arbre : du plus gros au plus petit, puis par nom
        gros.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| self.index.nom_de(a.1).to_lowercase().cmp(&self.index.nom_de(b.1).to_lowercase())));
        let gros = gros
            .into_iter()
            .map(|(taille, id)| {
                let nom = self.index.nom_de(id).to_string();
                Ligne {
                    id,
                    niveau: 0,
                    pastille: Some(Genre::de(&nom, false)),
                    nom,
                    dossier: false,
                    taille,
                    part: part(taille, total),
                    ouvert: false,
                    detail: self.dossier_du_fichier(id).unwrap_or_default(),
                    fichier: Some(id),
                }
            })
            .collect();

        let ages = (0..par_tranche.len()).filter(|&r| par_tranche[r].1 > 0).map(|r| groupe(libelle_tranche(r), par_tranche[r], None)).collect();
        Bilans { types, gros, ages }
    }

    /// Chemin Windows du dossier qui contient le nœud `id` (None pour un disque)
    pub fn dossier_du_fichier(&self, id: u32) -> Option<String> {
        self.index.parent_de(id).map(|p| self.index.chemin_de(p))
    }

    /// L'arbre est-il bâti sur cet index précis ? Faux dès qu'une reconstruction en a installé un autre.
    pub fn est_sur(&self, index: &Arc<Index>) -> bool {
        Arc::ptr_eq(&self.index, index)
    }

    /// Date de construction de l'index (secondes depuis 1970)
    pub fn construit(&self) -> u64 {
        self.index.construit
    }

    /// Nœuds `ids` du plus gros au plus petit ; à taille égale, par nom (sans la casse) pour un ordre stable
    fn trier(&self, mut ids: Vec<u32>) -> Vec<u32> {
        ids.sort_by(|&a, &b| {
            self.cumul[b as usize]
                .cmp(&self.cumul[a as usize])
                .then_with(|| self.index.nom_de(a).to_lowercase().cmp(&self.index.nom_de(b).to_lowercase()))
        });
        ids
    }

    fn ajouter_lignes(&mut self, ids: Vec<u32>, niveau: u16, total_parent: u64) {
        for id in self.trier(ids) {
            let dossier = self.index.est_dossier(id);
            let ouvert = dossier && self.ouverts.contains(&id);
            let taille = self.cumul[id as usize];
            let nom = if niveau == 0 { self.index.chemin_de(id) } else { self.index.nom_de(id).to_string() };
            self.lignes.push(Ligne { id, niveau, nom, dossier, taille, part: part(taille, total_parent), ouvert, detail: String::new(), fichier: None, pastille: None });
            if ouvert {
                let enfants = self.index.enfants(id);
                self.ajouter_lignes(enfants, niveau + 1, taille);
            }
        }
    }

    /// Refait la liste à plat depuis les dossiers ouverts
    fn reconstruire(&mut self) {
        self.lignes.clear();
        let racines = self.index.racines();
        let total = self.total;
        self.ajouter_lignes(racines, 0, total);
    }

    /// Ouvre le dossier de la ligne `k` ; faux si ce n'est pas un dossier, s'il est déjà ouvert ou s'il est vide
    pub fn ouvrir(&mut self, k: usize) -> bool {
        let Some(l) = self.lignes.get(k) else { return false };
        if !l.dossier || l.ouvert || self.index.enfants(l.id).is_empty() {
            return false;
        }
        let id = l.id;
        self.ouverts.insert(id);
        self.reconstruire();
        true
    }

    /// Ferme le dossier de la ligne `k` (et garde ses sous-dossiers ouverts en mémoire : ils se rouvrent comme avant)
    pub fn fermer(&mut self, k: usize) -> bool {
        let Some(l) = self.lignes.get(k) else { return false };
        if !l.ouvert {
            return false;
        }
        let id = l.id;
        self.ouverts.remove(&id);
        self.reconstruire();
        true
    }

    /// Ligne du dossier qui contient la ligne `k` (None pour un disque)
    pub fn parent_de(&self, k: usize) -> Option<usize> {
        let niveau = self.lignes.get(k)?.niveau;
        if niveau == 0 {
            return None;
        }
        (0..k).rev().find(|&i| self.lignes[i].niveau == niveau - 1)
    }

    /// Chemin Windows complet de la ligne `k`
    pub fn chemin(&self, k: usize) -> Option<String> {
        self.lignes.get(k).map(|l| self.index.chemin_de(l.id))
    }

    /// Les rectangles de la carte du dossier `parent` (les disques si `None`), du plus gros au plus petit. Les éléments
    /// vides n'ont pas de surface : ils n'y figurent pas. Au-delà de `CELLULES_MAX`, la fin de la liste devient « autres ».
    pub fn cellules(&self, parent: Option<u32>) -> Vec<Cellule> {
        let ids = match parent {
            Some(p) => self.index.enfants(p),
            None => self.index.racines(),
        };
        let total = parent.map_or(self.total, |p| self.cumul[p as usize]);
        let ids: Vec<u32> = self.trier(ids).into_iter().filter(|&id| self.cumul[id as usize] > 0).collect();
        let (gardes, reste) = if ids.len() > CELLULES_MAX { ids.split_at(CELLULES_MAX - 1) } else { (&ids[..], &ids[..0]) };
        let mut cellules: Vec<Cellule> = gardes
            .iter()
            .map(|&id| {
                let taille = self.cumul[id as usize];
                let nom = if parent.is_none() { self.index.chemin_de(id) } else { self.index.nom_de(id).to_string() };
                let dossier = self.index.est_dossier(id);
                Cellule { id: Some(id), genre: Genre::de(&nom, dossier), nom, dossier, taille, part: part(taille, total) }
            })
            .collect();
        if !reste.is_empty() {
            let taille = reste.iter().map(|&id| self.cumul[id as usize]).fold(0u64, u64::saturating_add);
            cellules.push(Cellule { id: None, nom: format!("{} autres éléments", reste.len()), dossier: false, genre: Genre::Autre, taille, part: part(taille, total) });
        }
        cellules
    }

    /// Les `n` plus gros éléments du dossier `parent`, sans le regroupement « autres » : l'aperçu dessiné dans un dossier de la carte
    pub fn apercu(&self, parent: u32, n: usize) -> Vec<Cellule> {
        let mut v = self.cellules(Some(parent));
        v.retain(|c| c.id.is_some());
        v.truncate(n);
        v
    }

    /// Nom court du nœud `id` (le chemin complet pour un disque)
    pub fn libelle_de(&self, id: u32, disque: bool) -> String {
        if disque { self.index.chemin_de(id) } else { self.index.nom_de(id).to_string() }
    }

    /// Chemin Windows complet du nœud `id`
    pub fn chemin_de(&self, id: u32) -> String {
        self.index.chemin_de(id)
    }

    /// Taille cumulée du nœud `id`
    pub fn taille_de(&self, id: u32) -> u64 {
        self.cumul.get(id as usize).copied().unwrap_or(0)
    }

    /// Les dossiers qui contiennent `id`, du disque jusqu'au parent direct (vide pour un disque)
    pub fn ancetres_de(&self, id: u32) -> Vec<u32> {
        let mut v = Vec::new();
        let mut courant = self.index.parent_de(id);
        while let Some(p) = courant {
            v.push(p);
            courant = self.index.parent_de(p);
        }
        v.reverse();
        v
    }

    /// Ouvre tous les dossiers qui contiennent `id` pour que sa ligne soit visible ; rend le rang de cette ligne
    pub fn reveler(&mut self, id: u32) -> Option<usize> {
        let ancetres = self.ancetres_de(id);
        if ancetres.iter().any(|a| !self.ouverts.contains(a)) {
            self.ouverts.extend(ancetres);
            self.reconstruire();
        }
        self.rang_de(id)
    }

    /// Rang de la ligne qui montre le nœud `id`, si elle est visible
    pub fn rang_de(&self, id: u32) -> Option<usize> {
        self.lignes.iter().position(|l| l.id == id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::index::DOSSIER_RACINE;

    /// C:  (22) ─ A (15) ─ un.txt 10, S (5) ─ deux.txt 5 ; B (7) ─ trois.txt 7 ; vide (0) ; D: (3) ─ x.bin 3
    fn essai() -> Arc<Index> {
        let mut i = Index::default();
        i.construit = 1_000;
        let c = i.ajouter("C:", DOSSIER_RACINE, true, 0, 0);
        let d = i.ajouter("D:", DOSSIER_RACINE, true, 0, 0);
        let b = i.ajouter("B", c, true, 0, 0);
        let a = i.ajouter("A", c, true, 0, 0);
        i.ajouter("vide", c, true, 0, 0);
        i.ajouter("x.bin", d, false, 3, 0);
        i.ajouter("trois.txt", b, false, 7, 0);
        i.ajouter("un.txt", a, false, 10, 0);
        let s = i.ajouter("S", a, true, 0, 0);
        i.ajouter("deux.txt", s, false, 5, 0);
        Arc::new(i)
    }

    fn noms(a: &Arbre) -> Vec<String> {
        a.lignes().iter().map(|l| format!("{}{}", " ".repeat(l.niveau as usize), l.nom)).collect()
    }

    #[test]
    fn les_disques_sont_tries_du_plus_gros_au_plus_petit() {
        let a = Arbre::nouveau(essai());
        assert_eq!(noms(&a), vec!["C:\\", "D:\\"]);
        assert_eq!(a.lignes()[0].taille, 22);
        assert_eq!(a.lignes()[1].taille, 3);
        assert_eq!(a.total, 25);
    }

    #[test]
    fn ouvrir_montre_les_enfants_tries_par_taille() {
        let mut a = Arbre::nouveau(essai());
        assert!(a.ouvrir(0));
        // A (15) avant B (7) avant vide (0), alors que B a été rangé avant A dans l'index
        assert_eq!(noms(&a), vec!["C:\\", " A", " B", " vide", "D:\\"]);
        assert!(a.lignes()[0].ouvert);
    }

    #[test]
    fn la_carte_liste_les_enfants_par_taille_sans_les_vides() {
        let a = Arbre::nouveau(essai());
        let disques = a.cellules(None);
        assert_eq!(disques.iter().map(|c| c.nom.as_str()).collect::<Vec<_>>(), vec!["C:\\", "D:\\"]);
        assert!((disques[0].part - 22.0 / 25.0).abs() < 1e-6);
        // Dans C: : A (15) puis B (7) ; « vide » (0 octet) n'a pas de surface
        let c = disques[0].id.unwrap();
        let dedans = a.cellules(Some(c));
        assert_eq!(dedans.iter().map(|x| x.nom.as_str()).collect::<Vec<_>>(), vec!["A", "B"]);
        assert!(dedans.iter().all(|x| x.dossier));
        assert!((dedans[0].part - 15.0 / 22.0).abs() < 1e-6);
        assert_eq!(a.taille_de(c), 22);
        // Un fichier n'a pas d'enfants
        let un = a.cellules(Some(dedans[0].id.unwrap()));
        assert_eq!(un.iter().map(|x| (x.nom.as_str(), x.dossier)).collect::<Vec<_>>(), vec![("un.txt", false), ("S", true)]);
        assert!(a.cellules(Some(un[0].id.unwrap())).is_empty());
    }

    #[test]
    fn la_carte_regroupe_les_plus_petits_au_dela_du_maximum() {
        let mut i = Index::default();
        let r = i.ajouter("C:", DOSSIER_RACINE, true, 0, 0);
        for k in 0..100u64 {
            i.ajouter(&format!("f{k:03}"), r, false, 1000 - k, 0);
        }
        let a = Arbre::nouveau(Arc::new(i));
        let c = a.cellules(a.cellules(None)[0].id);
        assert_eq!(c.len(), CELLULES_MAX);
        let autres = c.last().unwrap();
        assert_eq!((autres.id, autres.dossier), (None, false));
        assert!(autres.nom.starts_with("41 "), "100 éléments - 59 gardés = 41 regroupés : {}", autres.nom);
        // Rien ne se perd : la somme des rectangles est la taille du dossier
        assert_eq!(c.iter().map(|x| x.taille).sum::<u64>(), a.total);
    }

    #[test]
    fn egalite_de_taille_ordre_par_nom() {
        let mut i = Index::default();
        let r = i.ajouter("C:", DOSSIER_RACINE, true, 0, 0);
        i.ajouter("beta.txt", r, false, 4, 0);
        i.ajouter("Alpha.txt", r, false, 4, 0);
        i.ajouter("gros.bin", r, false, 9, 0);
        let mut a = Arbre::nouveau(Arc::new(i));
        a.ouvrir(0);
        assert_eq!(noms(&a), vec!["C:\\", " gros.bin", " Alpha.txt", " beta.txt"]);
    }

    #[test]
    fn les_parts_sont_celles_du_dossier_parent() {
        let mut a = Arbre::nouveau(essai());
        a.ouvrir(0);
        a.ouvrir(1);
        let l = a.lignes();
        // A = 15 sur les 22 de C: ; un.txt = 10 sur les 15 de A
        assert!((l[1].part - 15.0 / 22.0).abs() < 1e-6);
        assert_eq!(noms(&a)[2], "  un.txt");
        assert!((l[2].part - 10.0 / 15.0).abs() < 1e-6);
        // Un disque : part de la somme des disques
        assert!((l[0].part - 22.0 / 25.0).abs() < 1e-6);
    }

    #[test]
    fn fermer_et_rouvrir_garde_les_sous_dossiers_ouverts() {
        let mut a = Arbre::nouveau(essai());
        a.ouvrir(0); // C:
        a.ouvrir(1); // A
        a.ouvrir(3); // S, rang 3 : C:\, A, un.txt, S
        assert_eq!(a.lignes().len(), 8);
        assert!(a.fermer(0));
        assert_eq!(noms(&a), vec!["C:\\", "D:\\"]);
        a.ouvrir(0);
        assert_eq!(a.lignes().len(), 8, "A et S étaient ouverts : ils le restent");
    }

    #[test]
    fn ouvrir_refuse_un_fichier_un_dossier_vide_ou_deja_ouvert() {
        let mut a = Arbre::nouveau(essai());
        a.ouvrir(0);
        let vide = a.lignes().iter().position(|l| l.nom == "vide").unwrap();
        assert!(!a.ouvrir(vide), "dossier vide");
        assert!(!a.ouvrir(0), "déjà ouvert");
        a.ouvrir(1);
        let fichier = a.lignes().iter().position(|l| l.nom == "un.txt").unwrap();
        assert!(!a.ouvrir(fichier), "un fichier");
        assert!(!a.fermer(fichier), "un fichier ne se ferme pas");
        assert!(!a.ouvrir(99), "hors liste");
    }

    #[test]
    fn parent_chemin_et_rang() {
        let mut a = Arbre::nouveau(essai());
        a.ouvrir(0);
        a.ouvrir(1);
        let un = a.lignes().iter().position(|l| l.nom == "un.txt").unwrap();
        assert_eq!(a.parent_de(un), Some(1));
        assert_eq!(a.parent_de(1), Some(0));
        assert_eq!(a.parent_de(0), None);
        assert_eq!(a.chemin(un).as_deref(), Some("C:\\A\\un.txt"));
        assert_eq!(a.chemin(0).as_deref(), Some("C:\\"));
        let id = a.lignes()[un].id;
        assert_eq!(a.rang_de(id), Some(un));
        assert_eq!(a.rang_de(9999), None);
    }

    #[test]
    fn index_vide() {
        let a = Arbre::nouveau(Arc::new(Index::default()));
        assert!(a.lignes().is_empty());
        assert_eq!(a.total, 0);
        assert_eq!(part(5, 0), 0.0);
    }

    #[test]
    fn pourcentage_et_age_en_texte() {
        assert_eq!(pourcent(0.0), "0 %");
        assert_eq!(pourcent(0.004), "< 1 %");
        assert_eq!(pourcent(0.426), "43 %");
        assert_eq!(pourcent(1.0), "100 %");
        assert_eq!(part(1, 4), 0.25);
        assert_eq!(part(9, 4), 1.0, "borné à 1");
        assert_eq!(age(10), "à l'instant");
        assert_eq!(age(7_200), "il y a 2 h");
        assert_eq!(age(3 * 86_400 + 5), "il y a 3 j");
        assert_eq!(age(300), "il y a 5 min");
    }
}
