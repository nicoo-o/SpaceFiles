// Réglages partagés entre les applications : `%LOCALAPPDATA%\Eco\shared.json` (thème, accent, mouvement, langue,
// registre des raccourcis). Format : un objet JSON `{"theme":"dark","accent":"#D71921","motion":"full","language":"fr",
// "shortcuts":{"<commande>":"<touche>"}}` (noms de champs à relire par l'auteur : le protocole ne les fixait pas).
//
// Lecture tolérante : fichier absent, vide, abîmé, avec BOM UTF-8 (AUDIT R23) ou UTF-16 : valeurs par défaut, jamais
// d'erreur ni de panique. Champ invalide : la valeur par défaut de ce champ seulement. Champ inconnu (version plus
// récente) : conservé tel quel à la réécriture. Le résident lit une fois au démarrage (`lire`, en cache) ; chaque
// fenêtre, processus séparé, relit à la demande le seul avis de mouvement (`mouvement_partage`, phase 5 lot D). Les
// champs `avis_*` distinguent « le fichier dit X » de « valeur par défaut » : seul un avis explicite est appliqué.
// Phase 7 lot B : SpaceFiles honore `theme` (`theme_partage`, même relecture que le mouvement) ; l'accent n'est pas appliqué.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant, SystemTime};

use super::json::{analyser_json_avec, champ, Json, CLES_MAX_PARTAGE};
use crate::journal::Niveau;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Theme {
    Dark,
    Light,
    System,
}

impl Theme {
    pub fn cle(self) -> &'static str {
        match self {
            Theme::Dark => "dark",
            Theme::Light => "light",
            Theme::System => "system",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mouvement {
    Full,
    Reduced,
}

impl Mouvement {
    pub fn cle(self) -> &'static str {
        match self {
            Mouvement::Full => "full",
            Mouvement::Reduced => "reduced",
        }
    }
}

const ACCENT_DEFAUT: &str = "#D71921";
const LANGUE_DEFAUT: &str = "fr";
/// Clés que nous comprenons ; toute autre est gardée dans `inconnus`
const CLES: [&str; 5] = ["theme", "accent", "motion", "language", "shortcuts"];
/// Un fichier plus gros n'est pas lu (valeurs par défaut) : ce n'est pas un `shared.json`
const TAILLE_MAX: u64 = 256 * 1024;
/// Aligné sur la borne de clés d'un objet de `shared.json` (`CLES_MAX_PARTAGE`) : une borne plus basse serait inatteignable
const RACCOURCIS_MAX: usize = CLES_MAX_PARTAGE;

#[derive(Debug, Clone, PartialEq)]
pub struct Partage {
    pub theme: Theme,
    /// `#RRGGBB`
    pub accent: String,
    pub mouvement: Mouvement,
    /// Étiquette de langue (`fr`, `en-US`)
    pub langue: String,
    /// Identifiant de commande -> touche (`explorer.nouvelOnglet` -> `Ctrl+T`)
    pub raccourcis: Vec<(String, String)>,
    /// Champs d'une version plus récente, réécrits tels quels
    pub inconnus: Vec<(String, Json)>,
    /// Le fichier disait un thème valide (sinon `None` : aucun avis, `theme` n'est qu'un défaut)
    pub avis_theme: Option<Theme>,
    /// Le fichier disait un mouvement valide (sinon `None` : aucun avis)
    pub avis_mouvement: Option<Mouvement>,
}

impl Default for Partage {
    fn default() -> Self {
        Partage { theme: Theme::Dark, accent: ACCENT_DEFAUT.to_string(), mouvement: Mouvement::Full, langue: LANGUE_DEFAUT.to_string(), raccourcis: Vec::new(), inconnus: Vec::new(), avis_theme: None, avis_mouvement: None }
    }
}

/// Ce que la lecture a trouvé (pour le journal)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Etat {
    Lu,
    Absent,
    /// Vide, pas du JSON, trop gros, octets invalides : valeurs par défaut. Le contenu est mauvais, relire ne changerait rien
    Abime,
    /// Échec d'entrée-sortie passager (fichier verrouillé, ouverture refusée après les essais) : le contenu est inconnu, ce
    /// n'est PAS un fichier abîmé ; l'appelant garde son dernier avis et relit plus tard
    Illisible,
}

/// `#RRGGBB` : sept caractères, six chiffres hexadécimaux
fn accent_valide(s: &str) -> bool {
    s.len() == 7 && s.starts_with('#') && s[1..].bytes().all(|c| c.is_ascii_hexdigit())
}

fn langue_valide(s: &str) -> bool {
    (2..=35).contains(&s.len()) && s.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'-')
}

/// Texte du fichier depuis ses octets : UTF-8 (BOM toléré) ou UTF-16 avec BOM (PowerShell 5.1 écrit ainsi). None : illisible.
fn decoder(octets: &[u8]) -> Option<String> {
    if let Some(reste) = octets.strip_prefix(&[0xFF, 0xFE]) {
        let u: Vec<u16> = reste.chunks_exact(2).map(|p| u16::from_le_bytes([p[0], p[1]])).collect();
        return String::from_utf16(&u).ok();
    }
    let reste = octets.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(octets);
    std::str::from_utf8(reste).ok().map(str::to_string)
}

/// Lit le contenu d'un fichier de réglages partagés. Ne panique jamais.
pub fn depuis_octets(octets: &[u8]) -> (Partage, Etat) {
    let Some(texte) = decoder(octets) else { return (Partage::default(), Etat::Abime) };
    let Some(Json::Objet(o)) = analyser_json_avec(texte.trim(), CLES_MAX_PARTAGE) else { return (Partage::default(), Etat::Abime) };
    let mut p = Partage::default();
    match champ(&o, "theme") {
        Some(Json::Texte(t)) if t == "dark" => p.theme = Theme::Dark,
        Some(Json::Texte(t)) if t == "light" => p.theme = Theme::Light,
        Some(Json::Texte(t)) if t == "system" => p.theme = Theme::System,
        _ => {}
    }
    if matches!(champ(&o, "theme"), Some(Json::Texte(t)) if ["dark", "light", "system"].contains(&t.as_str())) {
        p.avis_theme = Some(p.theme);
    }
    if let Some(Json::Texte(a)) = champ(&o, "accent") {
        if accent_valide(a) {
            p.accent = a.to_uppercase();
        }
    }
    match champ(&o, "motion") {
        Some(Json::Texte(t)) if t == "full" => p.avis_mouvement = Some(Mouvement::Full),
        Some(Json::Texte(t)) if t == "reduced" => p.avis_mouvement = Some(Mouvement::Reduced),
        _ => {}
    }
    if let Some(m) = p.avis_mouvement {
        p.mouvement = m;
    }
    if let Some(Json::Texte(l)) = champ(&o, "language") {
        if langue_valide(l) {
            p.langue = l.clone();
        }
    }
    if let Some(Json::Objet(r)) = champ(&o, "shortcuts") {
        p.raccourcis = r
            .iter()
            .filter_map(|(k, v)| match v {
                Json::Texte(t) if !k.is_empty() && !t.is_empty() => Some((k.clone(), t.clone())),
                _ => None,
            })
            .take(RACCOURCIS_MAX)
            .collect();
    }
    p.inconnus = o.iter().filter(|(k, _)| !CLES.contains(&k.as_str())).cloned().collect();
    (p, Etat::Lu)
}

/// Lit le fichier. Une erreur passagère (renommage en cours par un autre écrivain) est retentée quelques fois.
pub fn lire_depuis(chemin: &Path) -> (Partage, Etat) {
    match std::fs::metadata(chemin) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return (Partage::default(), Etat::Absent),
        Ok(m) if m.len() > TAILLE_MAX => return (Partage::default(), Etat::Abime),
        _ => {}
    }
    lire_contenu(chemin)
}

/// Ouverture et lecture bornée, sans `metadata` (l'appelant a déjà la taille : voir `mouvement_partage`). Quatre essais ;
/// un échec qui dure est `Illisible`, jamais `Abime`.
fn lire_contenu(chemin: &Path) -> (Partage, Etat) {
    for essai in 0..4 {
        // Lecture bornée : un fichier qui grossit entre le contrôle de taille et la lecture ne dépasse pas la borne
        let lu = std::fs::File::open(chemin).and_then(|f| {
            let mut octets = Vec::new();
            f.take(TAILLE_MAX + 1).read_to_end(&mut octets).map(|_| octets)
        });
        match lu {
            Ok(octets) if octets.len() as u64 > TAILLE_MAX => return (Partage::default(), Etat::Abime),
            Ok(octets) => return depuis_octets(&octets),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return (Partage::default(), Etat::Absent),
            Err(_) if essai < 3 => std::thread::sleep(std::time::Duration::from_millis(10)),
            Err(_) => {}
        }
    }
    (Partage::default(), Etat::Illisible)
}

static CACHE: OnceLock<Partage> = OnceLock::new();

/// Les réglages partagés, lus une seule fois (le premier appel lit le disque) ; valeurs par défaut si le fichier manque.
/// Le journal dit l'état de la lecture en une ligne.
pub fn lire() -> &'static Partage {
    CACHE.get_or_init(|| {
        let Some(chemin) = super::dossier_eco().map(|d| d.join("shared.json")) else { return Partage::default() };
        let (p, etat) = lire_depuis(&chemin);
        match etat {
            Etat::Lu => super::trace(
                Niveau::Info,
                "eco",
                &format!("shared.json lu : theme {}, accent {}, mouvement {}, langue {}, avis de theme {}, {} raccourci(s), {} champ(s) inconnu(s) conserve(s)", p.theme.cle(), p.accent, p.mouvement.cle(), p.langue, if p.avis_theme.is_some() { "oui" } else { "non" }, p.raccourcis.len(), p.inconnus.len()),
            ),
            Etat::Absent => super::trace(Niveau::Info, "eco", "shared.json absent : valeurs par defaut"),
            Etat::Abime => super::trace(Niveau::Avert, "eco", "shared.json abime : valeurs par defaut"),
            Etat::Illisible => super::trace(Niveau::Avert, "eco", "shared.json illisible (entree-sortie) : valeurs par defaut"),
        }
        p
    })
}

/// Au démarrage du résident : lit (et met en cache) les réglages partagés
pub fn charger_au_demarrage() {
    let _ = lire();
}

/// Où lire `shared.json` : jamais en mode portable (isolement : rien du profil n'entre ni ne sort), ni sans dossier Eco.
/// Le mode portable est testé AVANT d'appeler `dossier_eco` : aucun accès au disque ni à l'environnement dans ce cas.
fn chemin_lecture(portable: bool, dossier_eco: impl FnOnce() -> Option<PathBuf>) -> Option<PathBuf> {
    if portable {
        return None;
    }
    dossier_eco().map(|d| d.join("shared.json"))
}

fn chemin_courant() -> Option<PathBuf> {
    chemin_lecture(crate::reglages::dossier_portable().is_some(), super::dossier_eco)
}

/// Un contrôle du fichier au plus toutes les secondes, à la fin d'une lecture de dossier (jamais de fil ni de minuterie)
const PERIODE_CONTROLE: Duration = Duration::from_secs(1);

/// Date de modification et taille : si elles n'ont pas changé, le fichier n'est pas relu. Limite connue : une réécriture
/// qui garde la même date ET la même taille est invisible (granularité FAT/exFAT de 2 s).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Signature {
    modifie: Option<SystemTime>,
    taille: u64,
}

/// Résultat du contrôle de date et de taille
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Controle {
    Absent,
    Present(Signature),
    /// `metadata` a échoué autrement que par « introuvable » : on ne sait rien
    Echec,
}

/// Résultat de la lecture d'un fichier présent
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Lecture {
    /// Lu (ou abîmé = aucun avis) : l'avis du fichier
    Avis(Option<Mouvement>, Option<Theme>),
    /// Échec d'entrée-sortie : contenu inconnu
    Echec,
}

/// Mémoire du dernier contrôle : l'avis de mouvement, la signature du fichier qui l'a donné, l'heure du contrôle.
/// L'horloge, le contrôle et la lecture sont injectés (tests sans disque ni attente).
#[derive(Debug, Default)]
struct Tampon {
    controle: Option<Instant>,
    /// Signature du fichier dont `avis` est issu ; `None` : fichier absent, ou rien de lu avec succès
    signature: Option<Signature>,
    avis: Option<Mouvement>,
    /// Avis de thème du même fichier, relu avec lui (phase 7 lot B : SpaceFiles honore `theme`)
    theme: Option<Theme>,
    /// Le dernier contrôle ou la dernière lecture a échoué (E/S) : l'avis gardé est l'ancien, relu au contrôle suivant
    echec: bool,
}

impl Tampon {
    /// Sur un échec d'E/S, ni la signature ni l'avis ne changent : le dernier avis connu reste, et le contrôle suivant
    /// (une seconde plus tard) relit. `lire` n'est appelée que si la signature a changé.
    fn avis(&mut self, maintenant: Instant, controle: impl FnOnce() -> Controle, lire: impl FnOnce() -> Lecture) -> Option<Mouvement> {
        if self.controle.is_some_and(|c| maintenant.saturating_duration_since(c) < PERIODE_CONTROLE) {
            return self.avis;
        }
        self.controle = Some(maintenant);
        self.echec = false;
        match controle() {
            Controle::Echec => self.echec = true,
            Controle::Absent => {
                self.signature = None;
                self.avis = None;
                self.theme = None;
            }
            Controle::Present(sig) => {
                if self.signature != Some(sig) {
                    match lire() {
                        Lecture::Avis(a, t) => {
                            self.signature = Some(sig);
                            self.avis = a;
                            self.theme = t;
                        }
                        Lecture::Echec => self.echec = true,
                    }
                }
            }
        }
        self.avis
    }
}

static TAMPON: Mutex<Tampon> = Mutex::new(Tampon { controle: None, signature: None, avis: None, theme: None, echec: false });

/// L'avis de mouvement du fichier partagé : `Some(Reduced)` / `Some(Full)` s'il le dit, `None` sinon (fichier absent,
/// abîmé, champ absent, mode portable). Relu à la demande, avec un contrôle de date et de taille au plus une fois par
/// seconde. Un échec d'entrée-sortie garde le dernier avis connu. Le journal reçoit les changements d'avis et les échecs
/// (états seulement, une fois par changement).
pub fn mouvement_partage() -> Option<Mouvement> {
    avis_partages().0
}

/// L'avis de thème du fichier partagé (`dark`, `light`, `system`), relu par le même contrôle que le mouvement : un seul
/// `metadata` par seconde pour les deux. `None` : aucun avis (le thème par défaut du fichier n'est pas un avis).
pub fn theme_partage() -> Option<Theme> {
    avis_partages().1
}

fn avis_partages() -> (Option<Mouvement>, Option<Theme>) {
    let mut t = TAMPON.lock().unwrap_or_else(|e| e.into_inner());
    let (avant, theme_avant, echec_avant) = (t.avis, t.theme, t.echec);
    // Le mode portable est jugé avant tout accès au disque ; la fonction n'est appelée qu'au contrôle (une fois par seconde)
    let avis = t.avis(
        Instant::now(),
        || {
            let Some(chemin) = chemin_courant() else { return Controle::Absent };
            match std::fs::metadata(chemin) {
                Ok(m) => Controle::Present(Signature { modifie: m.modified().ok(), taille: m.len() }),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => Controle::Absent,
                Err(_) => Controle::Echec,
            }
        },
        || match chemin_courant().map(|c| lire_contenu(&c)) {
            Some((p, Etat::Lu)) => Lecture::Avis(p.avis_mouvement, p.avis_theme),
            Some((_, Etat::Illisible)) => Lecture::Echec,
            _ => Lecture::Avis(None, None),
        },
    );
    if avis != avant {
        super::trace(Niveau::Info, "eco", &format!("shared.json : avis de mouvement {}", if avis.is_some() { "oui" } else { "non" }));
    }
    if t.theme != theme_avant {
        super::trace(Niveau::Info, "eco", &format!("shared.json : avis de theme {}", t.theme.map_or("aucun", Theme::cle)));
    }
    if t.echec && !echec_avant {
        super::trace(Niveau::Avert, "eco", "shared.json : controle ou lecture impossible (entree-sortie), dernier avis garde");
    }
    (avis, t.theme)
}

/// Le document à écrire : champs connus d'abord, puis les champs inconnus tels que lus
#[cfg(test)]
fn document(p: &Partage) -> String {
    use super::json::{echapper, serialiser};
    let raccourcis: Vec<String> = p.raccourcis.iter().map(|(k, v)| format!("{}:{}", echapper(k), echapper(v))).collect();
    let mut champs = vec![
        format!("\"theme\":{}", echapper(p.theme.cle())),
        format!("\"accent\":{}", echapper(&p.accent)),
        format!("\"motion\":{}", echapper(p.mouvement.cle())),
        format!("\"language\":{}", echapper(&p.langue)),
        format!("\"shortcuts\":{{{}}}", raccourcis.join(",")),
    ];
    champs.extend(p.inconnus.iter().map(|(k, v)| format!("{}:{}", echapper(k), serialiser(v))));
    format!("{{{}}}\n", champs.join(","))
}

/// Écriture atomique (temporaire + `sync_all` + renommage), champs inconnus conservés. Compilée en test seulement :
/// SpaceFiles n'écrit pas `shared.json` avant les phases 6 et 7 (c'est le Hub qui l'écrit), donc aucun appelant ne
/// justifie encore du code dans l'exe (pas de `allow(dead_code)`). La version d'usage sera `ecrire(&Partage)` =
/// `ecrire_vers(<dossier Eco>/shared.json, …)`.
#[cfg(test)]
pub fn ecrire_vers(chemin: &Path, p: &Partage) -> std::io::Result<()> {
    super::ecrire_atomique(chemin, &document(p))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::eco::json::analyser_json;
    use crate::eco::ok;

    fn dossier(nom: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("eco-partage-{}-{nom}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    #[test]
    fn valeurs_par_defaut_sans_fichier() {
        let d = dossier("absent");
        let (p, etat) = lire_depuis(&d.join("shared.json"));
        assert_eq!((p, etat), (Partage::default(), Etat::Absent));
        let defaut = Partage::default();
        assert_eq!((defaut.theme, defaut.mouvement, defaut.langue.as_str(), defaut.accent.as_str()), (Theme::Dark, Mouvement::Full, "fr", "#D71921"));
    }

    #[test]
    fn fichier_valide() {
        let t = "{\"theme\":\"light\",\"accent\":\"#00ff7f\",\"motion\":\"reduced\",\"language\":\"en-US\",\"shortcuts\":{\"a.b\":\"Ctrl+K\",\"x\":3,\"\":\"y\"}}";
        let (p, etat) = depuis_octets(t.as_bytes());
        assert_eq!(etat, Etat::Lu);
        assert_eq!((p.theme, p.mouvement, p.langue.as_str()), (Theme::Light, Mouvement::Reduced, "en-US"));
        assert_eq!(p.accent, "#00FF7F");
        // Une entrée dont la touche n'est pas du texte, ou sans identifiant, est ignorée
        assert_eq!(p.raccourcis, vec![("a.b".to_string(), "Ctrl+K".to_string())]);
        assert!(p.inconnus.is_empty());
    }

    #[test]
    fn vide_abime_ou_pas_du_json() {
        for t in ["", "   \n", "{", "pas du json", "[1,2]", "42", "{\"theme\":", "\0\0\0\0", "{\"a\":1,\"a\":2}"] {
            assert_eq!(depuis_octets(t.as_bytes()), (Partage::default(), Etat::Abime), "{t:?}");
        }
        // Octets qui ne sont pas de l'UTF-8
        assert_eq!(depuis_octets(&[0xC3, 0x28, 0xFF, 0xFD]), (Partage::default(), Etat::Abime));
        assert_eq!(depuis_octets(&[0xFF, 0xFE, 0x41]).1, Etat::Abime, "UTF-16 tronqué");
    }

    #[test]
    fn bom_utf8_et_utf_16_tolerees() {
        let t = "{\"theme\":\"system\"}";
        let mut avec_bom = vec![0xEF, 0xBB, 0xBF];
        avec_bom.extend_from_slice(t.as_bytes());
        assert_eq!(depuis_octets(&avec_bom).0.theme, Theme::System);
        let mut utf16 = vec![0xFF, 0xFE];
        utf16.extend(t.encode_utf16().flat_map(u16::to_le_bytes));
        assert_eq!(depuis_octets(&utf16), (Partage { theme: Theme::System, avis_theme: Some(Theme::System), ..Partage::default() }, Etat::Lu));
    }

    // ───── Avis (phase 5 lot D) ─────

    #[test]
    fn aucun_avis_sans_fichier_valide_ni_champ() {
        // Absent, vide, abîmé : valeurs par défaut, jamais un avis
        let d = dossier("avis-absent");
        let (p, _) = lire_depuis(&d.join("shared.json"));
        assert_eq!((p.avis_theme, p.avis_mouvement), (None, None));
        for t in ["", "{", "pas du json", "[1]"] {
            let (p, _) = depuis_octets(t.as_bytes());
            assert_eq!((p.avis_theme, p.avis_mouvement), (None, None), "{t:?}");
        }
        // Fichier présent mais champs absents ou invalides : le défaut Dark / Full n'est pas un avis
        for t in ["{}", "{\"accent\":\"#00FF00\"}", "{\"theme\":\"contraste\",\"motion\":3}", "{\"motion\":\"lent\"}"] {
            let (p, etat) = depuis_octets(t.as_bytes());
            assert_eq!(etat, Etat::Lu);
            assert_eq!((p.avis_theme, p.avis_mouvement), (None, None), "{t:?}");
        }
    }

    #[test]
    fn avis_explicites_avec_bom() {
        let (p, _) = depuis_octets(b"{\"theme\":\"light\",\"motion\":\"reduced\"}");
        assert_eq!((p.avis_theme, p.avis_mouvement), (Some(Theme::Light), Some(Mouvement::Reduced)));
        let (p, _) = depuis_octets(b"\xEF\xBB\xBF{\"motion\":\"full\"}");
        assert_eq!((p.avis_theme, p.avis_mouvement), (None, Some(Mouvement::Full)));
        assert_eq!(p.mouvement, Mouvement::Full);
        let (p, _) = depuis_octets(b"{\"theme\":\"system\"}");
        assert_eq!(p.avis_theme, Some(Theme::System));
    }

    #[test]
    fn lecture_refusee_en_portable_ou_sans_dossier() {
        let eco = || Some(PathBuf::from(r"C:\x\Eco"));
        // Portable : le dossier Eco n'est même pas demandé (aucun accès au disque ni à l'environnement)
        let appels = std::cell::Cell::new(0);
        let compte = || {
            appels.set(appels.get() + 1);
            eco()
        };
        assert_eq!(chemin_lecture(true, compte), None, "portable : isolement");
        assert_eq!(appels.get(), 0, "portable : dossier_eco jamais appelé");
        assert_eq!(chemin_lecture(false, || None), None);
        assert_eq!(chemin_lecture(false, eco), Some(PathBuf::from(r"C:\x\Eco\shared.json")));
    }

    fn sig(n: u64) -> Controle {
        Controle::Present(Signature { modifie: Some(SystemTime::UNIX_EPOCH + Duration::from_secs(n)), taille: 10 })
    }

    fn avis(m: Mouvement) -> Lecture {
        Lecture::Avis(Some(m), None)
    }

    #[test]
    fn tampon_un_controle_par_seconde_et_relecture_si_la_date_change() {
        let t0 = Instant::now();
        let mut t = Tampon::default();
        let (mut metas, mut lectures) = (0, 0);
        let mut appel = |t: &mut Tampon, dt_ms: u64, c: Controle, l: Lecture| {
            t.avis(t0 + Duration::from_millis(dt_ms), || { metas += 1; c }, || { lectures += 1; l })
        };
        assert_eq!(appel(&mut t, 0, sig(1), avis(Mouvement::Reduced)), Some(Mouvement::Reduced));
        // Sous une seconde : ni metadata ni lecture, même si le fichier a changé
        assert_eq!(appel(&mut t, 500, sig(2), avis(Mouvement::Full)), Some(Mouvement::Reduced));
        assert_eq!(appel(&mut t, 999, sig(2), avis(Mouvement::Full)), Some(Mouvement::Reduced));
        // Une seconde passée, même signature : un metadata, pas de relecture
        assert_eq!(appel(&mut t, 1000, sig(1), avis(Mouvement::Full)), Some(Mouvement::Reduced));
        // Date changée : relecture
        assert_eq!(appel(&mut t, 2000, sig(2), avis(Mouvement::Full)), Some(Mouvement::Full));
        // Fichier supprimé : aucun avis, sans lecture
        assert_eq!(appel(&mut t, 3000, Controle::Absent, avis(Mouvement::Full)), None);
        // Fichier recréé : relu
        assert_eq!(appel(&mut t, 4000, sig(5), Lecture::Avis(None, None)), None);
        assert_eq!((metas, lectures), (5, 3), "metadata aux appels 1, 4, 5, 6, 7 ; lecture aux appels 1, 5, 7");
    }

    /// L'avis de thème voyage avec celui du mouvement : même contrôle, même relecture, remis à zéro si le fichier disparaît
    #[test]
    fn tampon_porte_l_avis_de_theme() {
        let t0 = Instant::now();
        let mut t = Tampon::default();
        let a = |t: &mut Tampon, ms: u64, c: Controle, l: Lecture| t.avis(t0 + Duration::from_millis(ms), || c, || l);
        a(&mut t, 0, sig(1), Lecture::Avis(None, Some(Theme::Light)));
        assert_eq!(t.theme, Some(Theme::Light));
        // Sous une seconde : pas de relecture, l'avis ne bouge pas
        a(&mut t, 500, sig(2), Lecture::Avis(None, Some(Theme::Dark)));
        assert_eq!(t.theme, Some(Theme::Light));
        a(&mut t, 1000, sig(2), Lecture::Avis(Some(Mouvement::Reduced), Some(Theme::Dark)));
        assert_eq!((t.theme, t.avis), (Some(Theme::Dark), Some(Mouvement::Reduced)));
        // Échec d'E/S : le dernier avis de thème est gardé
        a(&mut t, 2000, sig(3), Lecture::Echec);
        assert_eq!(t.theme, Some(Theme::Dark));
        a(&mut t, 3000, Controle::Absent, Lecture::Echec);
        assert_eq!(t.theme, None, "fichier supprimé : aucun avis");
    }

    /// Un `shared.json` écrit à la main avec seulement `{"theme":"light"}` donne un avis de thème, et aucun de mouvement
    #[test]
    fn theme_seul_donne_un_avis_de_theme() {
        let (p, etat) = depuis_octets(b"{\"theme\":\"light\"}");
        assert_eq!((etat, p.avis_theme, p.avis_mouvement), (Etat::Lu, Some(Theme::Light), None));
    }

    /// Un écrivain verrouille le fichier puis le libère, même date et même taille : le tampon ne fige pas « aucun avis »
    #[test]
    fn tampon_echec_d_es_garde_le_dernier_avis_et_relit_au_controle_suivant() {
        let t0 = Instant::now();
        let mut t = Tampon::default();
        let a = |t: &mut Tampon, ms: u64, c: Controle, l: Lecture| t.avis(t0 + Duration::from_millis(ms), || c, || l);
        // Premier contrôle : lecture impossible (verrou) -> aucun avis connu, signature NON mémorisée
        assert_eq!(a(&mut t, 0, sig(1), Lecture::Echec), None);
        assert!(t.echec && t.signature.is_none());
        // Libéré, mêmes date et taille : relu
        assert_eq!(a(&mut t, 1000, sig(1), avis(Mouvement::Reduced)), Some(Mouvement::Reduced));
        assert!(!t.echec);
        // Verrou passager ensuite : le dernier avis connu est gardé, pas « aucun avis »
        assert_eq!(a(&mut t, 2000, sig(2), Lecture::Echec), Some(Mouvement::Reduced));
        assert!(t.echec);
        // Contrôle impossible (metadata refusé) : idem
        assert_eq!(a(&mut t, 3000, Controle::Echec, Lecture::Avis(None, None)), Some(Mouvement::Reduced));
        // Libéré, nouvelle date : relu
        assert_eq!(a(&mut t, 4000, sig(2), avis(Mouvement::Full)), Some(Mouvement::Full));
        // Absent est un vrai « aucun avis », pas un échec
        assert_eq!(a(&mut t, 5000, Controle::Absent, Lecture::Echec), None);
        assert!(!t.echec);
    }

    /// Un vrai verrou exclusif (Windows) : la lecture est `Illisible`, pas `Abime`, puis `Lu` une fois libéré
    #[test]
    fn fichier_verrouille_est_illisible_pas_abime() {
        use std::os::windows::fs::OpenOptionsExt;
        let d = dossier("verrou");
        let f = d.join("shared.json");
        ok(std::fs::create_dir_all(&d), "creation du dossier");
        ok(std::fs::write(&f, "{\"motion\":\"reduced\"}"), "ecriture");
        let verrou = ok(std::fs::OpenOptions::new().read(true).write(true).share_mode(0).open(&f), "verrou exclusif");
        assert_eq!(lire_depuis(&f).1, Etat::Illisible);
        assert_eq!(lire_contenu(&f).1, Etat::Illisible);
        drop(verrou);
        let (p, etat) = lire_depuis(&f);
        assert_eq!((etat, p.avis_mouvement), (Etat::Lu, Some(Mouvement::Reduced)));
        let _ = std::fs::remove_dir_all(&d);
    }

    /// shared.json de 300 clés inconnues : la borne des trames (256) ne l'invalide pas, `motion` est gardé
    #[test]
    fn beaucoup_de_cles_inconnues_gardent_le_mouvement() {
        let mut t = String::from("{\"motion\":\"reduced\"");
        for i in 0..300 {
            t.push_str(&format!(",\"x{i}\":{i}"));
        }
        t.push('}');
        let (p, etat) = depuis_octets(t.as_bytes());
        assert_eq!((etat, p.avis_mouvement, p.inconnus.len()), (Etat::Lu, Some(Mouvement::Reduced), 300));
        // Au-delà de la borne du fichier (1 024 clés) : abîmé
        let mut t = String::from("{\"motion\":\"reduced\"");
        for i in 0..CLES_MAX_PARTAGE {
            t.push_str(&format!(",\"x{i}\":1"));
        }
        t.push('}');
        assert_eq!(depuis_octets(t.as_bytes()).1, Etat::Abime);
        // Les raccourcis peuvent atteindre leur borne (alignée sur celle des clés)
        let mut r = String::from("{\"shortcuts\":{");
        for i in 0..600 {
            r.push_str(&format!("{}\"c{i}\":\"K\"", if i == 0 { "" } else { "," }));
        }
        r.push_str("}}");
        assert_eq!(depuis_octets(r.as_bytes()).0.raccourcis.len(), 600);
    }

    #[test]
    fn champ_invalide_garde_son_defaut_sans_toucher_aux_autres() {
        let t = "{\"theme\":\"contraste\",\"accent\":\"rouge\",\"motion\":3,\"language\":\"fr fr\",\"shortcuts\":[1]}";
        assert_eq!(depuis_octets(t.as_bytes()), (Partage::default(), Etat::Lu));
        let (p, _) = depuis_octets(b"{\"theme\":\"light\",\"accent\":\"#12345\",\"language\":\"de\"}");
        assert_eq!((p.theme, p.accent.as_str(), p.langue.as_str()), (Theme::Light, "#D71921", "de"));
    }

    #[test]
    fn champs_inconnus_conserves_a_la_reecriture() {
        let d = dossier("inconnus");
        let f = d.join("shared.json");
        let source = "{\"theme\":\"light\",\"futur\":{\"a\":[1,2,{\"b\":null}]},\"version_schema\":7,\"motion\":\"reduced\"}";
        ok(std::fs::create_dir_all(&d), "creation du dossier");
        ok(std::fs::write(&f, source), "ecriture");
        let (mut p, _) = lire_depuis(&f);
        assert_eq!(p.inconnus.len(), 2);
        p.theme = Theme::Dark;
        assert!(ecrire_vers(&f, &p).is_ok());
        let (relu, etat) = lire_depuis(&f);
        assert_eq!(etat, Etat::Lu);
        assert_eq!(relu.theme, Theme::Dark, "le champ connu est mis a jour");
        assert_eq!(relu.mouvement, Mouvement::Reduced, "le champ connu non touche reste");
        assert_eq!(relu.inconnus, p.inconnus, "les champs d'une version plus recente survivent");
        assert_eq!(champ_texte(&f, "version_schema"), "7");
        let _ = std::fs::remove_dir_all(&d);
    }

    fn champ_texte(f: &Path, cle: &str) -> String {
        let t = ok(std::fs::read_to_string(f), "lecture");
        match analyser_json(&t) {
            Some(Json::Objet(o)) => champ(&o, cle).map(crate::eco::json::serialiser).unwrap_or_default(),
            _ => String::new(),
        }
    }

    /// Deux écrivains en même temps (et un lecteur) : le fichier est toujours un `shared.json` complet
    #[test]
    fn ecritures_concurrentes() {
        let d = dossier("concurrent");
        let f = d.join("shared.json");
        assert!(ecrire_vers(&f, &Partage::default()).is_ok());
        let ecrivains: Vec<_> = [Theme::Light, Theme::System]
            .into_iter()
            .map(|theme| {
                let f = f.clone();
                std::thread::spawn(move || {
                    let mut p = Partage { theme, ..Partage::default() };
                    (0..60).all(|i| {
                        p.raccourcis = vec![(format!("cmd.{i}"), "Ctrl+X".to_string())];
                        ecrire_vers(&f, &p).is_ok()
                    })
                })
            })
            .collect();
        let mut invalides = 0;
        for _ in 0..200 {
            let (_, etat) = lire_depuis(&f);
            if etat != Etat::Lu {
                invalides += 1;
            }
        }
        for e in ecrivains {
            assert_eq!(e.join().ok(), Some(true), "une ecriture a echoue");
        }
        assert_eq!(invalides, 0, "un lecteur a vu un fichier absent ou abime");
        let (p, etat) = lire_depuis(&f);
        assert_eq!(etat, Etat::Lu);
        assert_eq!(p.raccourcis.len(), 1);
        let restes = std::fs::read_dir(&d).map(|r| r.flatten().filter(|e| e.file_name().to_string_lossy().ends_with(".tmp")).count()).unwrap_or(99);
        assert_eq!(restes, 0, "des temporaires sont restes");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn fichier_trop_gros_ignore() {
        let d = dossier("gros");
        let f = d.join("shared.json");
        ok(std::fs::create_dir_all(&d), "creation du dossier");
        ok(std::fs::write(&f, vec![b' '; TAILLE_MAX as usize + 1]), "ecriture");
        assert_eq!(lire_depuis(&f), (Partage::default(), Etat::Abime));
        let _ = std::fs::remove_dir_all(&d);
    }

    /// Phase 6, lot B : les octets EXACTS qu'écrit l'écrivain du Hub (SpaceNotch, `EcoSharedWriter`) pour une fusion. Fixture
    /// littérale : même texte que `attendu` de `tools/bench/preuve-phase6-hub-apparence.cs` (cas g) et que le test C#
    /// `EcoSharedWriterTests.The_merged_bytes_are_exactly_the_fixture_that_the_rust_test_reads` (dépôt SpaceNotch) ; le fichier d'avant était
    /// `{"accent":"#00ff7f","language":"en-US","future":{"a":[1,2.50]},"theme":"dark"}` avec le choix clair + réduit.
    /// Si l'un des deux côtés change de forme (indentation, ordre, séparateurs), ce test ou le harnais casse.
    #[test]
    fn octets_ecrits_par_le_hub() {
        let octets = "{\n  \"accent\": \"#00ff7f\",\n  \"language\": \"en-US\",\n  \"future\": {\"a\":[1,2.50]},\n  \"theme\": \"light\",\n  \"motion\": \"reduced\"\n}";
        let (p, etat) = depuis_octets(octets.as_bytes());
        assert_eq!(etat, Etat::Lu);
        assert_eq!((p.theme, p.mouvement), (Theme::Light, Mouvement::Reduced));
        assert_eq!((p.avis_theme, p.avis_mouvement), (Some(Theme::Light), Some(Mouvement::Reduced)));
        assert_eq!((p.accent.as_str(), p.langue.as_str()), ("#00FF7F", "en-US"));
        // Le champ inconnu écrit par une version plus récente est conservé, et le fichier n'est pas pris pour abîmé
        assert_eq!(p.inconnus.len(), 1);
        assert_eq!(p.inconnus[0].0, "future");

        // Les trois autres valeurs de thème que le Hub écrit, chacune avec les deux mouvements, par la même forme
        for (theme, attendu_theme) in [("dark", Theme::Dark), ("light", Theme::Light), ("system", Theme::System)] {
            for (mouvement, attendu_mouvement) in [("full", Mouvement::Full), ("reduced", Mouvement::Reduced)] {
                let t = format!("{{\n  \"theme\": \"{theme}\",\n  \"motion\": \"{mouvement}\"\n}}");
                let (p, etat) = depuis_octets(t.as_bytes());
                assert_eq!(etat, Etat::Lu, "{t}");
                assert_eq!((p.avis_theme, p.avis_mouvement), (Some(attendu_theme), Some(attendu_mouvement)), "{t}");
            }
        }

        // Le fichier vide `{}` que « Réinitialiser » écrit : lu, sans aucun avis
        let (p, etat) = depuis_octets(b"{}");
        assert_eq!((etat, p.avis_theme, p.avis_mouvement), (Etat::Lu, None, None));
    }

    /// Phase 6, lot B (passe 2) : le sous-ensemble que le Hub (`EcoSharedCompat`, SpaceNotch) tient pour commun aux deux lecteurs.
    /// Mêmes bornes et mêmes fixtures que `EcoSharedWriterTests` (`Rejected` et `Accepted`) : chaque fichier rejeté ici est
    /// « Abîmé » pour SpaceFiles, donc un fichier que le Hub écrirait dans cet état ferait perdre tout avis à SpaceFiles.
    #[test]
    fn hors_sous_ensemble_commun_rejete_par_spacefiles() {
        let imbrique = |n: usize| format!("{}1{}", "{\"a\":".repeat(n), "}".repeat(n));
        let cles = |n: usize| (0..n).map(|i| format!("\"k{i}\":1")).collect::<Vec<_>>().join(",");
        let abimes = [
            format!("{{\"x\":{}}}", imbrique(8)),
            "{\"x\":[[[[[[[[1]]]]]]]]}".to_string(),
            format!("{{{}}}", cles(1025)),
            format!("{{\"x\":{{{}}}}}", cles(1025)),
            "{\"x\":{\"a\":1,\"a\":2}}".to_string(),
            "{\"theme\":\"light\",\"x\":1,\"theme\":\"light\"}".to_string(),
            "{\"x\":1e400}".to_string(),
            "{\"x\":1.2345678901234567890123456789012}".to_string(),
            "{\"x\":\"\\ud800\"}".to_string(),
            "{\"x\":\"\\udc00\"}".to_string(),
            "{\"x\":\"\\ud800\\u0041\"}".to_string(),
        ];
        for t in &abimes {
            assert_eq!(depuis_octets(t.as_bytes()).1, Etat::Abime, "{}", &t[..t.len().min(60)]);
        }
        let lus = [
            format!("{{\"x\":{}}}", imbrique(7)),
            "{\"x\":[[[[[[[1]]]]]]]}".to_string(),
            format!("{{{}}}", cles(1022)),
            format!("{{\"x\":{{{}}}}}", cles(1024)),
            "{\"x\":{\"a\":1},\"y\":{\"a\":2}}".to_string(),
            "{\"x\":1.234567890123456789012345678901}".to_string(),
            "{\"x\":\"\\ud83d\\ude00\\u00e9\"}".to_string(),
            "{\"x\":1e308,\"y\":-0.5E-3,\"z\":[true,false,null]}".to_string(),
        ];
        for t in &lus {
            assert_eq!(depuis_octets(t.as_bytes()).1, Etat::Lu, "{}", &t[..t.len().min(60)]);
        }
    }
}
