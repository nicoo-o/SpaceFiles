// Pastilles d'opération, posées au-dessus de la barre d'état : modèle (ce que chaque pastille dit) et géométrie (où elle se
// pose, ce qui est cliquable). Logique pure, sans Direct2D : le texte est mesuré par une fonction passée en paramètre,
// le dessin est dans `rendu.rs`, la construction depuis l'état des onglets dans `app.rs`.
// Règle du brief : on n'affiche que ce que l'application sait vraiment (pas de vitesse ni de taille inventées).

/// Hauteur d'une pastille et rayon de ses coins (DIP)
pub const HAUTEUR: f32 = 38.0;
pub const RAYON: f32 = 19.0;
/// Espace entre deux pastilles, et entre deux rangées
pub const ECART: f32 = 8.0;
/// Pastilles montrées au plus ; les autres se résument en « +N »
pub const MAX_VISIBLES: usize = 3;
/// Marge entre la pastille et le bord de la zone disponible
pub const MARGE_BORD: f32 = 20.0;
/// Barres de la jauge : 16 de 3 × 10 DIP
pub const BARRES: usize = 16;
pub const BARRE_L: f32 = 3.0;
pub const BARRE_H: f32 = 10.0;
pub const BARRE_ECART: f32 = 2.0;
/// Tête (grille 3 × 3, coche, croix) : disque de 24 DIP
pub const TETE: f32 = 24.0;
const REMPLISSAGE_G: f32 = 7.0;
const REMPLISSAGE_D: f32 = 14.0;
const ESPACE: f32 = 10.0;
const NOM_MAX: f32 = 150.0;
/// Le nom d'une carte d'information (`.nm2` du toast) : 320 DIP au plus
const NOM_MAX_INFO: f32 = 320.0;
const NOM_MIN: f32 = 40.0;
/// Bouton : marge de chaque côté de son texte, et largeur d'un bouton à glyphe (✕)
const BOUTON_MARGE: f32 = 10.0;
const BOUTON_GLYPHE: f32 = 26.0;
const PUCE_MAX: f32 = 130.0;
const MESSAGE_MAX: f32 = 240.0;

/// Combien de temps une pastille reste (ms) : fin 5 s, erreur et annulation 10 s ; une opération reste tant qu'elle dure
pub const FIN_MS: u128 = 5000;
pub const ERREUR_MS: u128 = 10_000;
pub const ANNULATION_MS: u128 = 10_000;
/// Téléchargement du navigateur : « TÉLÉCHARGÉ » reste 10 s (le temps d'AFFICHER), « ANNULÉ » 5 s ; en cours ou en pause, il reste
/// tant que le fichier partiel existe
pub const TELECHARGE_MS: u128 = 10_000;
pub const TELECHARGEMENT_ANNULE_MS: u128 = 5000;
/// Carte d'information (toast de la maquette) : elle se ferme seule 4,5 s après son apparition
pub const INFO_MS: u128 = 4500;
/// Cartes d'information gardées en même temps : la plus ancienne cède la place
pub const TOASTS_MAX: usize = 3;
/// Bit de la clé d'une carte d'information (celles des téléchargements portent le bit 62, les opérations un simple numéro)
pub const CLE_TOAST: u64 = 1 << 61;

/// Où en est un téléchargement du navigateur
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PhaseTelechargement {
    EnCours,
    EnPause,
    Fini,
    Annule,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Genre {
    /// L'opération en cours
    Operation,
    /// Une opération de la file, derrière celle qui tourne
    Attente,
    /// Opération finie
    Fin,
    /// Opération en échec
    Erreur,
    /// Action qu'on peut encore défaire (Ctrl+Z)
    Annulation,
    /// Téléchargement du navigateur, vu par le dossier Téléchargements (voir `navigateur.rs`) : indéterminé, sans jauge
    Telechargement(PhaseTelechargement),
    /// Retour d'une action qui n'a pas de carte propre (« Chemin copié ») : coche, texte, ✕ ; se ferme seule après `INFO_MS`
    Info,
}

/// Ce que dessine la tête de la pastille
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tete {
    /// Grille 3 × 3 « hypnotique » : l'opération travaille
    Grille,
    /// Coche grise : fini
    Coche,
    /// Croix en couleur d'erreur : échec
    Croix,
    /// Croix grise : annulé (on ne sait pas si c'est un échec : jamais la couleur d'erreur)
    CroixGrise,
    /// Disque vide : en attente
    Vide,
    /// Flèche de retour : action annulable
    Retour,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Bouton {
    /// Pause ou reprise de l'opération en cours
    Pause,
    /// Annuler l'opération en cours (et vider la file)
    Annuler,
    /// Ouvrir le dossier de destination
    Destination,
    /// Fermer la pastille
    Fermer,
    /// Défaire la dernière action (Ctrl+Z)
    AnnulerAction,
    /// Montrer le fichier téléchargé dans son dossier (pastille « TÉLÉCHARGÉ »)
    Afficher,
}

#[derive(Clone, PartialEq, Debug)]
pub struct Pastille {
    pub genre: Genre,
    /// Nom court en majuscules : « COPIE · 3 ÉLÉMENTS »
    pub libelle: String,
    /// Avancement de 0 à 1 (opération en cours seulement)
    pub avancement: f32,
    pub tete: Tete,
    /// Boutons qui apparaissent au déploiement (toujours visibles pour une erreur et une annulation)
    pub boutons: Vec<Bouton>,
    /// Dossier de destination, si l'opération le connaît
    pub destination: Option<String>,
    pub en_pause: bool,
    /// Mot posé à droite à la place de la jauge : « EN ATTENTE », « TERMINÉ », « ERREUR »
    pub etiquette: Option<String>,
    /// Message de l'opération en échec
    pub message: Option<String>,
    /// Identité stable (numéro de l'opération) : le rang d'une pastille change quand la liste change, pas sa clé. Un clic sur un
    /// bouton vérifie que la pastille sous la souris au dessin est encore la même (0 : sans clé)
    pub cle: u64,
}

/// « COPIE · 3 ÉLÉMENTS » : le nom court d'une opération sur `nombre` éléments
pub fn nom_court(genre: &str, nombre: usize) -> String {
    format!("{genre} · {nombre} ÉLÉMENT{}", if nombre > 1 { "S" } else { "" })
}

impl Pastille {
    pub fn operation(libelle: String, avancement: f32, en_pause: bool, destination: Option<String>) -> Self {
        let mut boutons = vec![Bouton::Pause, Bouton::Annuler];
        if destination.is_some() {
            boutons.insert(0, Bouton::Destination);
        }
        Pastille { genre: Genre::Operation, libelle, avancement: avancement.clamp(0.0, 1.0), tete: Tete::Grille, boutons, destination, en_pause, etiquette: None, message: None, cle: 0 }
    }

    pub fn attente(libelle: String) -> Self {
        Pastille { genre: Genre::Attente, libelle, avancement: 0.0, tete: Tete::Vide, boutons: Vec::new(), destination: None, en_pause: false, etiquette: Some("EN ATTENTE".into()), message: None, cle: 0 }
    }

    pub fn fin(libelle: String, destination: Option<String>) -> Self {
        let mut boutons = vec![Bouton::Fermer];
        if destination.is_some() {
            boutons.insert(0, Bouton::Destination);
        }
        Pastille { genre: Genre::Fin, libelle, avancement: 1.0, tete: Tete::Coche, boutons, destination, en_pause: false, etiquette: Some("TERMINÉ".into()), message: None, cle: 0 }
    }

    pub fn erreur(libelle: String, message: String) -> Self {
        Pastille { genre: Genre::Erreur, libelle, avancement: 0.0, tete: Tete::Croix, boutons: vec![Bouton::Fermer], destination: None, en_pause: false, etiquette: Some("ERREUR".into()), message: Some(message), cle: 0 }
    }

    pub fn annulation(libelle: String) -> Self {
        Pastille { genre: Genre::Annulation, libelle, avancement: 0.0, tete: Tete::Retour, boutons: vec![Bouton::AnnulerAction], destination: None, en_pause: false, etiquette: None, message: None, cle: 0 }
    }

    /// Pastille d'un téléchargement du navigateur. `etiquette` : le mot de droite (taille, « EN PAUSE · … », « TÉLÉCHARGÉ · … »,
    /// « ANNULÉ »). Jamais de jauge ni de pourcentage : le total est inconnu. Aucun bouton d'annulation (on n'arrête pas le
    /// navigateur) : ✕ masque seulement ; « TÉLÉCHARGÉ » ajoute AFFICHER. `cle` : clé stable (voir `Pastille::cle`).
    pub fn telechargement(phase: PhaseTelechargement, libelle: String, etiquette: String, cle: u64) -> Self {
        let (tete, boutons) = match phase {
            PhaseTelechargement::EnCours | PhaseTelechargement::EnPause => (Tete::Grille, vec![Bouton::Fermer]),
            PhaseTelechargement::Fini => (Tete::Coche, vec![Bouton::Afficher, Bouton::Fermer]),
            PhaseTelechargement::Annule => (Tete::CroixGrise, vec![Bouton::Fermer]),
        };
        Pastille {
            genre: Genre::Telechargement(phase),
            libelle,
            avancement: 0.0,
            tete,
            boutons,
            destination: None,
            // La grille d'un téléchargement en pause est fixe (même rendu que la pause d'une opération)
            en_pause: phase == PhaseTelechargement::EnPause,
            etiquette: Some(etiquette),
            message: None,
            cle,
        }
    }

    /// Carte d'information (le `toast` de la maquette : `type:'info'`, déployée d'emblée, texte sans majuscules forcées)
    pub fn info(libelle: String, cle: u64) -> Self {
        Pastille { genre: Genre::Info, libelle, avancement: 1.0, tete: Tete::Coche, boutons: vec![Bouton::Fermer], destination: None, en_pause: false, etiquette: None, message: None, cle }
    }

    pub fn avec_cle(mut self, cle: u64) -> Self {
        self.cle = cle;
        self
    }

    /// Erreur et annulation montrent leurs boutons sans survol : l'utilisateur doit pouvoir y répondre. Un téléchargement fini
    /// aussi : AFFICHER est la raison d'être de la pastille, et elle ne reste que 10 s.
    pub fn deployee_en_permanence(&self) -> bool {
        matches!(self.genre, Genre::Erreur | Genre::Annulation | Genre::Info | Genre::Telechargement(PhaseTelechargement::Fini))
    }

    /// Nom lu par un lecteur d'écran : « Copie, 42 pour cent »
    pub fn nom_accessible(&self) -> String {
        if let Genre::Telechargement(phase) = self.genre {
            // Le nom du fichier garde sa casse ; le mot de droite se lit tel quel (« 12,4 mo, 1,2 mo/s »)
            let fichier = if self.libelle == "TÉLÉCHARGEMENT" { String::new() } else { format!(", {}", self.libelle) };
            let droite = self.etiquette.as_deref().unwrap_or("").to_lowercase().replace(" · ", ", ").replace("/s", " par seconde");
            return match phase {
                PhaseTelechargement::EnCours => format!("Téléchargement{fichier}, en cours, {droite}"),
                PhaseTelechargement::EnPause => format!("Téléchargement{fichier}, {droite}"),
                PhaseTelechargement::Fini => format!("Téléchargement{fichier}, {droite}"),
                PhaseTelechargement::Annule => format!("Téléchargement{fichier}, annulé"),
            };
        }
        if self.genre == Genre::Info {
            return self.libelle.clone();
        }
        let mot = self.libelle.split(" · ").next().unwrap_or(&self.libelle);
        let mut lettres = mot.chars();
        let mot: String = lettres.next().map(|c| c.to_uppercase().collect::<String>() + &lettres.as_str().to_lowercase()).unwrap_or_default();
        match self.genre {
            Genre::Operation if self.en_pause => format!("{mot}, en pause, {} pour cent", (self.avancement * 100.0).round() as i32),
            Genre::Operation => format!("{mot}, {} pour cent", (self.avancement * 100.0).round() as i32),
            Genre::Attente => format!("{mot}, en attente"),
            Genre::Fin => format!("{mot}, terminé"),
            Genre::Erreur => format!("{mot}, erreur, {}", self.message.as_deref().unwrap_or("").to_lowercase()),
            Genre::Annulation => format!("{}, annuler avec Ctrl Z", self.libelle.to_lowercase()),
            // Déjà rendus plus haut
            Genre::Telechargement(_) | Genre::Info => String::new(),
        }
    }
}

/// Ce bouton existe-t-il sur cette pastille ? Un clic arrivé avec un rang périmé ne doit jamais agir sur une autre pastille
/// (une file qui a avancé, une pastille retirée) : le bouton visé doit faire partie de la pastille trouvée au rang.
pub fn bouton_autorise(p: &Pastille, b: Bouton) -> bool {
    p.boutons.contains(&b)
}

/// Pixel montre l'erreur tant que sa pastille vit : une erreur plus vieille que 10 s ne compte plus
pub fn erreur_vivante(est_erreur: bool, age_ms: u128) -> bool {
    est_erreur && !expiree(Genre::Erreur, age_ms)
}

/// Une pastille est-elle périmée après `age_ms` ? Une opération (en cours ou en file) ne l'est jamais.
pub fn expiree(genre: Genre, age_ms: u128) -> bool {
    match genre {
        Genre::Operation | Genre::Attente => false,
        Genre::Fin => age_ms >= FIN_MS,
        Genre::Erreur => age_ms >= ERREUR_MS,
        Genre::Annulation => age_ms >= ANNULATION_MS,
        Genre::Info => age_ms >= INFO_MS,
        Genre::Telechargement(PhaseTelechargement::EnCours | PhaseTelechargement::EnPause) => false,
        Genre::Telechargement(PhaseTelechargement::Fini) => age_ms >= TELECHARGE_MS,
        Genre::Telechargement(PhaseTelechargement::Annule) => age_ms >= TELECHARGEMENT_ANNULE_MS,
    }
}

/// Rang d'affichage : l'opération en cours d'abord, la file, les téléchargements qui avancent, puis les erreurs, les fins
/// d'opération, les téléchargements finis ou annulés, l'annulation
pub fn rang_ordre(p: &Pastille) -> u8 {
    match p.genre {
        Genre::Operation => 0,
        Genre::Attente => 1,
        Genre::Telechargement(PhaseTelechargement::EnCours | PhaseTelechargement::EnPause) => 2,
        Genre::Erreur => 3,
        Genre::Fin => 4,
        Genre::Telechargement(PhaseTelechargement::Fini | PhaseTelechargement::Annule) => 5,
        Genre::Annulation => 6,
        // La plus récente en dernier, comme `dock.appendChild` de la maquette
        Genre::Info => 7,
    }
}

/// Plus grande largeur du nom d'une carte
fn nom_max(p: &Pastille) -> f32 {
    if p.genre == Genre::Info { NOM_MAX_INFO } else { NOM_MAX }
}

/// Une carte d'information gardée par la fenêtre
#[derive(Clone, Debug, PartialEq)]
pub struct Toast {
    pub cle: u64,
    pub texte: String,
    pub quand: std::time::Instant,
}

/// Les cartes d'information en cours (`toast()` de la maquette). Pur : l'heure est un paramètre. L'état vit dans une variable
/// globale de `app.rs` (comme l'annulation proposée), commune à tous les onglets.
#[derive(Debug, Default)]
pub struct Toasts {
    liste: Vec<Toast>,
    suivant: u64,
}

impl Toasts {
    pub const fn nouveau() -> Toasts {
        Toasts { liste: Vec::new(), suivant: 0 }
    }

    /// Ajoute une carte ; au-delà de `TOASTS_MAX` la plus ancienne part. Rend sa clé.
    pub fn ajouter(&mut self, texte: String, maintenant: std::time::Instant) -> u64 {
        self.suivant += 1;
        let cle = CLE_TOAST | self.suivant;
        self.liste.push(Toast { cle, texte, quand: maintenant });
        while self.liste.len() > TOASTS_MAX {
            self.liste.remove(0);
        }
        cle
    }

    /// Retire les cartes arrivées à `INFO_MS` ; vrai s'il en reste (la minuterie qui efface les pastilles doit continuer)
    pub fn purger(&mut self, maintenant: std::time::Instant) -> bool {
        self.liste.retain(|t| !expiree(Genre::Info, maintenant.saturating_duration_since(t.quand).as_millis()));
        !self.liste.is_empty()
    }

    /// Le ✕ d'une carte
    pub fn fermer(&mut self, cle: u64) {
        self.liste.retain(|t| t.cle != cle);
    }

    /// Les cartes vivantes, de la plus ancienne à la plus récente
    pub fn pastilles(&self, maintenant: std::time::Instant) -> Vec<Pastille> {
        self.liste
            .iter()
            .filter(|t| !expiree(Genre::Info, maintenant.saturating_duration_since(t.quand).as_millis()))
            .map(|t| Pastille::info(t.texte.clone(), t.cle))
            .collect()
    }
}

#[derive(Clone, Copy, PartialEq, Debug, Default)]
pub struct Zone {
    pub x: f32,
    pub y: f32,
    pub l: f32,
    pub h: f32,
}

impl Zone {
    pub fn contient(&self, px: f32, py: f32) -> bool {
        px >= self.x && px < self.x + self.l && py >= self.y && py < self.y + self.h
    }

    pub fn droite(&self) -> f32 {
        self.x + self.l
    }
}

/// Une pastille posée : sa zone et tout ce qui se dessine ou se clique dedans
#[derive(Clone, PartialEq, Debug)]
pub struct Place {
    /// Rang de la pastille dans la liste donnée
    pub index: usize,
    /// Clé de la pastille (voir `Pastille::cle`)
    pub cle: u64,
    /// Partie visible de la pastille (fond, clics). Plus étroite que `pleine` pendant son déploiement
    pub zone: Zone,
    /// Largeur de l'intérieur posé (égale à `zone.l` hors déploiement animé) : le dessin rogne l'intérieur à `zone`
    pub pleine: f32,
    pub tete: Zone,
    /// Nom court, tronqué par « … » pour tenir
    pub libelle: String,
    pub zone_libelle: Zone,
    /// Jauge de 16 barres (opération en cours)
    pub jauge: Option<Zone>,
    pub pourcent: Option<Zone>,
    /// Mot de droite (« TERMINÉ », « EN ATTENTE », « ERREUR »)
    pub etiquette: Option<(String, Zone)>,
    /// Message d'erreur (déployée)
    pub message: Option<(String, Zone)>,
    /// Puce du dossier de destination (déployée) : cliquable, le chemin complet est l'infobulle
    pub destination: Option<(String, Zone)>,
    /// Touches « CTRL » « Z » (pastille d'annulation)
    pub touches: Option<Zone>,
    /// Boutons visibles : le bouton, sa zone, son texte (vide : bouton à glyphe ✕)
    pub boutons: Vec<(Bouton, Zone, String)>,
    pub deployee: bool,
}

#[derive(Clone, PartialEq, Debug, Default)]
pub struct Disposition {
    pub places: Vec<Place>,
    /// Pastille « +N » des pastilles qui ne tiennent pas : N et sa zone
    pub plus: Option<(usize, Zone)>,
}

impl Disposition {
    /// Ce qui se trouve sous ce point : une pastille, et éventuellement un de ses boutons
    pub fn sous(&self, x: f32, y: f32) -> Option<(usize, Option<Bouton>)> {
        for p in &self.places {
            if !p.zone.contient(x, y) {
                continue;
            }
            if let Some((b, _, _)) = p.boutons.iter().find(|(_, z, _)| z.contient(x, y)) {
                return Some((p.index, Some(*b)));
            }
            if let Some((_, z)) = &p.destination {
                if z.contient(x, y) && p.boutons.iter().any(|(b, _, _)| *b == Bouton::Destination) {
                    return Some((p.index, Some(Bouton::Destination)));
                }
            }
            return Some((p.index, None));
        }
        None
    }

    /// Rectangle qui contient tout (pour n'invalider que cette bande) : (x, y, l, h)
    pub fn enveloppe(&self) -> Option<Zone> {
        let mut zones = self.places.iter().map(|p| p.zone).chain(self.plus.iter().map(|p| p.1));
        let premiere = zones.next()?;
        let (mut g, mut h, mut d, mut b) = (premiere.x, premiere.y, premiere.droite(), premiere.y + premiere.h);
        for z in zones {
            g = g.min(z.x);
            h = h.min(z.y);
            d = d.max(z.droite());
            b = b.max(z.y + z.h);
        }
        Some(Zone { x: g, y: h, l: d - g, h: b - h })
    }
}

/// Texte qui tient dans `max` : trop long, il perd ses derniers caractères et finit par « … »
pub fn tronquer(texte: &str, max: f32, mesure: &dyn Fn(&str) -> f32) -> String {
    // Tolérance d'un centième de DIP : la largeur de la pastille est la somme des mesures, la place du nom en est la différence ;
    // l'arrondi des flottants coupait « lent.iso » en « lent.i… » alors que tout tenait
    if mesure(texte) <= max + 0.01 {
        return texte.to_string();
    }
    let mut lettres: Vec<char> = texte.chars().collect();
    while !lettres.is_empty() {
        lettres.pop();
        let essai: String = lettres.iter().collect::<String>().trim_end().to_string() + "…";
        if mesure(&essai) <= max {
            return essai;
        }
    }
    "…".to_string()
}

fn bouton_largeur(texte: &str, mesure: &dyn Fn(&str) -> f32) -> f32 {
    if texte.is_empty() {
        BOUTON_GLYPHE
    } else {
        mesure(texte) + 2.0 * BOUTON_MARGE
    }
}

/// Texte d'un bouton ; vide pour un bouton à glyphe ✕
fn bouton_texte(b: Bouton, p: &Pastille) -> &'static str {
    match b {
        Bouton::Pause if p.en_pause => "REPRISE",
        Bouton::Pause => "PAUSE",
        Bouton::AnnulerAction => "ANNULER",
        Bouton::Afficher => "AFFICHER",
        Bouton::Annuler | Bouton::Fermer | Bouton::Destination => "",
    }
}

/// Nom du dernier dossier d'un chemin (la puce de destination) : « Documents » pour `C:\Users\n\Documents`
pub fn nom_dossier(chemin: &str) -> String {
    let c = chemin.trim_end_matches('\\');
    match c.rsplit('\\').next() {
        Some(n) if !n.is_empty() && !n.ends_with(':') => n.to_string(),
        _ => c.to_string(),
    }
}

/// Largeurs d'une pastille : ce qui est déployé, la place du message, la largeur totale
struct Plan {
    deployee: bool,
    /// Largeur laissée au message d'erreur (0 : il est omis, la fenêtre est trop étroite)
    message_max: f32,
    largeur: f32,
    /// Largeur que prend la pastille une fois son déploiement fini (égale à `largeur` hors déploiement animé)
    pleine: f32,
}

/// Tout ce qui n'est pas le nom, le message ni les boutons : tête, jauge, pourcentage, mot de droite, touches
fn largeur_fixe(p: &Pastille, touches_l: f32, pourcent_l: f32, mesure: &dyn Fn(&str) -> f32) -> f32 {
    let mut l = REMPLISSAGE_G + TETE + ESPACE + REMPLISSAGE_D;
    if p.genre == Genre::Operation {
        l += ESPACE + BARRES as f32 * BARRE_L + (BARRES - 1) as f32 * BARRE_ECART + ESPACE + pourcent_l;
    }
    if let Some(e) = &p.etiquette {
        l += ESPACE + mesure(e);
    }
    if p.genre == Genre::Annulation {
        l += ESPACE + touches_l;
    }
    l
}

/// Puce de destination (déployée) : sa largeur, si la pastille en a une
fn puce_largeur(p: &Pastille, mesure: &dyn Fn(&str) -> f32) -> Option<f32> {
    let d = p.destination.as_deref().filter(|_| p.boutons.contains(&Bouton::Destination))?;
    Some(mesure(&tronquer(&nom_dossier(d), PUCE_MAX, mesure)) + 16.0)
}

fn planifier(p: &Pastille, ouverte: bool, dispo: f32, touches_l: f32, pourcent_l: f32, mesure: &dyn Fn(&str) -> f32) -> Plan {
    let permanente = p.deployee_en_permanence();
    let fixe = largeur_fixe(p, touches_l, pourcent_l, mesure);
    let boutons: f32 = p.boutons.iter().filter(|b| **b != Bouton::Destination).map(|b| ESPACE + bouton_largeur(bouton_texte(*b, p), mesure)).sum();
    let puce = puce_largeur(p, mesure).map_or(0.0, |l| ESPACE + l);
    let mut deployee = permanente || ouverte;
    // Survolée dans une fenêtre trop étroite : elle reste fermée plutôt que de déborder
    if deployee && !permanente && fixe + boutons + puce + NOM_MIN > dispo {
        deployee = false;
    }
    let (boutons, puce) = if deployee { (boutons, puce) } else { (0.0, 0.0) };
    let mut message_max = 0.0;
    let mut message = 0.0;
    if deployee {
        if let Some(m) = &p.message {
            let budget = (dispo - fixe - boutons - puce - NOM_MIN - ESPACE).clamp(0.0, MESSAGE_MAX);
            if budget >= 24.0 {
                message_max = budget;
                message = ESPACE + mesure(m).min(budget);
            }
        }
    }
    let reste = fixe + boutons + puce + message;
    let nom = mesure(&p.libelle).min(nom_max(p)).min((dispo - reste).max(NOM_MIN));
    let largeur = (reste + nom).min(dispo);
    Plan { deployee, message_max, largeur, pleine: largeur }
}

/// Pose les pastilles. `largeur_fenetre` et `gauche` bornent la zone (le volet est à gauche), `bas` est le bord bas de la rangée la
/// plus basse (juste au-dessus de la barre d'état). `touches_l` : largeur des touches « CTRL » « Z » (elle suit le texte agrandi de
/// Windows, le rendu la connaît). `ouverte` : la pastille déployée (survol, clic). Les 3 premières sont posées,
/// côte à côte et centrées, avec retour à la ligne vers le haut ; le reste devient « +N ».
#[cfg(test)]
pub fn mettre_en_page(largeur_fenetre: f32, gauche: f32, bas: f32, pastilles: &[Pastille], ouverte: Option<usize>, touches_l: f32, pourcent_l: f32, mesure: &dyn Fn(&str) -> f32) -> Disposition {
    let deploiements: Vec<f32> = (0..pastilles.len()).map(|i| f32::from(ouverte == Some(i))).collect();
    mettre_en_page_anime(largeur_fenetre, gauche, bas, pastilles, &deploiements, touches_l, pourcent_l, mesure)
}

/// Comme `mettre_en_page`, avec le déploiement de chaque pastille en cours de route (`.card .more` de la maquette : `max-width`
/// 0 → 440 en 400 ms au ressort). `deploiements[i]` : 0 = repliée, 1 = déployée, entre les deux la largeur est interpolée (la place
/// des boutons, de la puce et du message se révèle par la droite) ; la pastille garde alors l'intérieur de sa version déployée,
/// que le dessin rogne à la zone. Hors de 0 et 1 la mise en page est exactement celle de `mettre_en_page`.
pub fn mettre_en_page_anime(largeur_fenetre: f32, gauche: f32, bas: f32, pastilles: &[Pastille], deploiements: &[f32], touches_l: f32, pourcent_l: f32, mesure: &dyn Fn(&str) -> f32) -> Disposition {
    let x_min = gauche + MARGE_BORD;
    let dispo = (largeur_fenetre - MARGE_BORD - x_min).max(60.0);
    let visibles = pastilles.len().min(MAX_VISIBLES);
    let cache = pastilles.len() - visibles;
    let plans: Vec<Plan> = pastilles
        .iter()
        .enumerate()
        .take(visibles)
        .map(|(i, p)| {
            let k = deploiements.get(i).copied().unwrap_or(0.0).clamp(0.0, 1.0);
            if k >= 1.0 {
                return planifier(p, true, dispo, touches_l, pourcent_l, mesure);
            }
            let ferme = planifier(p, false, dispo, touches_l, pourcent_l, mesure);
            if k <= 0.0 {
                return ferme;
            }
            let ouvert = planifier(p, true, dispo, touches_l, pourcent_l, mesure);
            Plan { largeur: ferme.largeur + (ouvert.largeur - ferme.largeur) * k, ..ouvert }
        })
        .collect();
    let plus_l = if cache > 0 { mesure(&format!("+{cache}")) + 28.0 } else { 0.0 };

    // Rangées, de bas en haut : (rang de la pastille, ou None pour « +N », largeur)
    let mut rangees: Vec<Vec<(Option<usize>, f32)>> = vec![Vec::new()];
    let mut courante = 0.0;
    let mut poser_dans_une_rangee = |quoi: Option<usize>, l: f32| {
        let vide = rangees.last().is_some_and(|r| r.is_empty());
        if !vide && courante + ECART + l > dispo {
            rangees.push(Vec::new());
            courante = l;
        } else {
            courante = if vide { l } else { courante + ECART + l };
        }
        if let Some(r) = rangees.last_mut() {
            r.push((quoi, l));
        }
    };
    for (i, plan) in plans.iter().enumerate() {
        poser_dans_une_rangee(Some(i), plan.largeur);
    }
    if cache > 0 {
        poser_dans_une_rangee(None, plus_l);
    }

    let mut d = Disposition::default();
    for (r, rangee) in rangees.iter().enumerate() {
        let total: f32 = rangee.iter().map(|x| x.1).sum::<f32>() + ECART * rangee.len().saturating_sub(1) as f32;
        let mut x = x_min + (dispo - total) / 2.0;
        let y = bas - HAUTEUR - r as f32 * (HAUTEUR + ECART);
        for (quoi, l) in rangee {
            let zone = Zone { x, y, l: *l, h: HAUTEUR };
            match quoi {
                Some(i) => d.places.push(poser(&pastilles[*i], *i, zone, &plans[*i], touches_l, pourcent_l, mesure)),
                None => d.plus = Some((cache, zone)),
            }
            x += l + ECART;
        }
    }
    d
}

/// Découpe l'intérieur d'une pastille de zone `zone`, de droite à gauche : boutons, puce, message, touches, pourcentage et
/// jauge, mot de droite ; le nom prend ce qui reste
fn poser(p: &Pastille, index: usize, zone_vue: Zone, plan: &Plan, touches_l: f32, pourcent_l: f32, mesure: &dyn Fn(&str) -> f32) -> Place {
    // Déploiement en cours : l'intérieur est posé dans la largeur pleine, `zone` (clics, fond) est la partie déjà révélée
    let zone = Zone { l: plan.pleine.max(zone_vue.l), ..zone_vue };
    let cy = zone.y + HAUTEUR / 2.0;
    let mut x = zone.x + REMPLISSAGE_G;
    let tete = Zone { x, y: cy - TETE / 2.0, l: TETE, h: TETE };
    x += TETE + ESPACE;
    let mut droite = zone.droite() - REMPLISSAGE_D;
    let mut place = Place {
        index,
        cle: p.cle,
        zone: zone_vue,
        pleine: zone.l,
        tete,
        libelle: String::new(),
        zone_libelle: Zone::default(),
        jauge: None,
        pourcent: None,
        etiquette: None,
        message: None,
        destination: None,
        touches: None,
        boutons: Vec::new(),
        deployee: plan.deployee,
    };
    if plan.deployee {
        for b in p.boutons.iter().rev().filter(|b| **b != Bouton::Destination) {
            let texte = bouton_texte(*b, p);
            let l = bouton_largeur(texte, mesure);
            place.boutons.push((*b, Zone { x: droite - l, y: cy - 13.0, l, h: 26.0 }, texte.to_string()));
            droite -= l + ESPACE;
        }
        place.boutons.reverse();
        if let (Some(d), Some(l)) = (p.destination.as_deref(), puce_largeur(p, mesure)) {
            let z = Zone { x: droite - l, y: cy - 11.0, l, h: 22.0 };
            place.destination = Some((tronquer(&nom_dossier(d), PUCE_MAX, mesure), z));
            // La puce est aussi un bouton : un clic dessus ouvre le dossier
            place.boutons.insert(0, (Bouton::Destination, z, String::new()));
            droite -= l + ESPACE;
        }
        if let (Some(m), true) = (&p.message, plan.message_max > 0.0) {
            let t = tronquer(m, plan.message_max, mesure);
            let l = mesure(&t);
            place.message = Some((t, Zone { x: droite - l, y: zone.y, l, h: HAUTEUR }));
            droite -= l + ESPACE;
        }
    }
    if p.genre == Genre::Annulation {
        place.touches = Some(Zone { x: droite - touches_l, y: cy - 9.0, l: touches_l, h: 18.0 });
        droite -= touches_l + ESPACE;
    }
    if p.genre == Genre::Operation {
        let pc = Zone { x: droite - pourcent_l, y: zone.y, l: pourcent_l, h: HAUTEUR };
        place.pourcent = Some(pc);
        let jl = BARRES as f32 * BARRE_L + (BARRES - 1) as f32 * BARRE_ECART;
        let jx = pc.x - ESPACE - jl;
        place.jauge = Some(Zone { x: jx, y: cy - BARRE_H / 2.0, l: jl, h: BARRE_H });
        droite = jx - ESPACE;
    }
    if let Some(e) = &p.etiquette {
        let l = mesure(e);
        place.etiquette = Some((e.clone(), Zone { x: droite - l, y: zone.y, l, h: HAUTEUR }));
        droite -= l + ESPACE;
    }
    let dispo_nom = (droite - x).max(0.0);
    place.libelle = tronquer(&p.libelle, dispo_nom.min(nom_max(p)), mesure);
    place.zone_libelle = Zone { x, y: zone.y, l: dispo_nom, h: HAUTEUR };
    place
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Largeur des touches CTRL Z dans les tests
    const TL: f32 = 60.4;
    /// Largeur du pourcentage dans les tests
    const PL: f32 = 40.0;

    /// Mesure de test : 8 DIP par caractère
    fn m(t: &str) -> f32 {
        t.chars().count() as f32 * 8.0
    }

    fn op(nom: &str, pc: f32) -> Pastille {
        Pastille::operation(nom.to_string(), pc, false, Some(r"C:\Users\n\Documents".to_string()))
    }

    fn dans(petit: &Zone, grand: &Zone) -> bool {
        petit.x >= grand.x - 0.01 && petit.droite() <= grand.droite() + 0.01 && petit.y >= grand.y - 0.01 && petit.y + petit.h <= grand.y + grand.h + 0.01
    }

    #[test]
    fn une_pastille_centree_au_ras_de_la_barre() {
        let d = mettre_en_page(1000.0, 220.0, 500.0, &[op("COPIE · 3 ÉLÉMENTS", 0.4)], None, TL, PL, &m);
        assert_eq!(d.places.len(), 1);
        assert!(d.plus.is_none());
        let z = d.places[0].zone;
        assert_eq!(z.h, 38.0);
        assert_eq!(z.y + z.h, 500.0, "posée sur le bord bas donné");
        // Centrée dans [gauche + marge ; largeur - marge] = [240 ; 980]
        assert!(((z.x + z.droite()) / 2.0 - 610.0).abs() < 0.5, "centre à {}", (z.x + z.droite()) / 2.0);
        // Jauge de 16 barres de 3 DIP, écart 2 : 78 DIP
        assert_eq!(d.places[0].jauge.unwrap().l, 78.0);
        assert!(d.places[0].pourcent.is_some());
    }

    #[test]
    fn trois_pastilles_cote_a_cote_sans_se_chevaucher() {
        let p = vec![op("COPIE · 3 ÉLÉMENTS", 0.4), Pastille::attente("DEPLACEMENT · 2 ÉLÉMENTS".into()), Pastille::fin("COPIE · 1 ÉLÉMENT".into(), None)];
        let d = mettre_en_page(1600.0, 220.0, 700.0, &p, None, TL, PL, &m);
        assert_eq!(d.places.len(), 3);
        assert!(d.plus.is_none());
        let ys: Vec<f32> = d.places.iter().map(|x| x.zone.y).collect();
        assert!(ys.iter().all(|y| *y == ys[0]), "une seule rangée : {ys:?}");
        for w in d.places.windows(2) {
            assert!(w[0].zone.droite() + ECART <= w[1].zone.x + 0.01, "chevauchement");
        }
        assert_eq!(d.places.iter().map(|x| x.index).collect::<Vec<_>>(), vec![0, 1, 2]);
    }

    #[test]
    fn quatre_pastilles_trois_plus_un() {
        let p: Vec<Pastille> = (0..4).map(|i| op(&format!("COPIE · {} ÉLÉMENTS", i + 2), 0.1)).collect();
        let d = mettre_en_page(3000.0, 220.0, 700.0, &p, None, TL, PL, &m);
        assert_eq!(d.places.len(), 3);
        let (n, z) = d.plus.expect("pastille +N");
        assert_eq!(n, 1);
        assert_eq!(z.h, 38.0);
        // Cinq pastilles : +2
        let p: Vec<Pastille> = (0..5).map(|_| op("COPIE", 0.1)).collect();
        assert_eq!(mettre_en_page(3000.0, 220.0, 700.0, &p, None, TL, PL, &m).plus.unwrap().0, 2);
    }

    #[test]
    fn fenetre_etroite_retour_a_la_ligne_vers_le_haut() {
        let p = vec![op("COPIE · 3 ÉLÉMENTS", 0.4), op("DEPLACEMENT · 2 ÉLÉMENTS", 0.2)];
        // Zone de 420 DIP : deux pastilles de plus de 250 ne tiennent pas côte à côte
        let d = mettre_en_page(680.0, 220.0, 500.0, &p, None, TL, PL, &m);
        assert_eq!(d.places.len(), 2);
        let (a, b) = (d.places[0].zone, d.places[1].zone);
        assert_eq!(a.y + a.h, 500.0);
        assert_eq!(b.y + b.h, 500.0 - HAUTEUR - ECART, "la seconde monte d'une rangée");
        for z in [a, b] {
            assert!(z.x >= 240.0 - 0.01 && z.droite() <= 660.0 + 0.01, "hors de la zone : {z:?}");
        }
    }

    #[test]
    fn pastille_ouverte_plus_large_avec_ses_boutons() {
        let p = vec![op("COPIE · 3 ÉLÉMENTS", 0.4)];
        let fermee = mettre_en_page(1600.0, 220.0, 500.0, &p, None, TL, PL, &m);
        let ouverte = mettre_en_page(1600.0, 220.0, 500.0, &p, Some(0), TL, PL, &m);
        assert!(fermee.places[0].boutons.is_empty());
        assert!(ouverte.places[0].zone.l > fermee.places[0].zone.l + 60.0);
        let pl = &ouverte.places[0];
        let boutons: Vec<Bouton> = pl.boutons.iter().map(|b| b.0).collect();
        assert_eq!(boutons, vec![Bouton::Destination, Bouton::Pause, Bouton::Annuler]);
        assert_eq!(pl.destination.as_ref().unwrap().0, "Documents");
        for (_, z, _) in &pl.boutons {
            assert!(dans(z, &pl.zone), "bouton hors de la pastille : {z:?} / {:?}", pl.zone);
        }
        // Les boutons ne se chevauchent pas
        for w in pl.boutons.windows(2) {
            assert!(w[0].1.droite() <= w[1].1.x + 0.01);
        }
        // Le texte du bouton pause suit l'état
        let mut en_pause = op("COPIE", 0.4);
        en_pause.en_pause = true;
        let o = mettre_en_page(1600.0, 220.0, 500.0, &[en_pause], Some(0), TL, PL, &m);
        assert!(o.places[0].boutons.iter().any(|b| b.2 == "REPRISE"));
    }

    #[test]
    fn sans_destination_pas_de_puce() {
        let p = vec![Pastille::operation("SUPPRESSION · 2 ÉLÉMENTS".into(), 0.3, false, None)];
        let d = mettre_en_page(1600.0, 220.0, 500.0, &p, Some(0), TL, PL, &m);
        assert!(d.places[0].destination.is_none());
        assert!(d.places[0].boutons.iter().all(|b| b.0 != Bouton::Destination));
    }

    #[test]
    fn texte_long_tronque_par_une_ellipse() {
        let long = "COPIE · UN NOM D'OPERATION ABSOLUMENT TROP LONG POUR TENIR";
        let d = mettre_en_page(1600.0, 220.0, 500.0, &[op(long, 0.5)], None, TL, PL, &m);
        let pl = &d.places[0];
        assert!(pl.libelle.ends_with('…'), "{}", pl.libelle);
        assert!(m(&pl.libelle) <= 150.0 + 0.01);
        assert!(m(&pl.libelle) <= pl.zone_libelle.l + 0.01);
        assert_eq!(tronquer("COURT", 100.0, &m), "COURT");
        assert_eq!(tronquer("ABCDEFGHIJ", 40.0, &m), "ABCD…");
        assert_eq!(tronquer("ABC", 4.0, &m), "…");
    }

    #[test]
    fn fenetre_tres_etroite_ne_deborde_pas() {
        let p = vec![op("COPIE · 3 ÉLÉMENTS", 0.4)];
        let d = mettre_en_page(420.0, 220.0, 500.0, &p, Some(0), TL, PL, &m);
        let z = d.places[0].zone;
        assert!(z.l <= 420.0 - 20.0 - 240.0 + 0.01, "largeur {}", z.l);
        assert!(z.x >= 240.0 - 0.01);
        assert!(d.places[0].libelle.chars().count() >= 1);
    }

    #[test]
    fn erreur_et_annulation_deployees_sans_survol() {
        let e = Pastille::erreur("COPIE · 1 ÉLÉMENT".into(), "ACCES REFUSE".into());
        let a = Pastille::annulation("SUPPRIMÉ · 1 ÉLÉMENT".into());
        let d = mettre_en_page(1600.0, 220.0, 500.0, &[e, a], None, TL, PL, &m);
        let (pe, pa) = (&d.places[0], &d.places[1]);
        assert!(pe.deployee && pe.message.as_ref().unwrap().0 == "ACCES REFUSE");
        assert_eq!(pe.boutons.iter().map(|b| b.0).collect::<Vec<_>>(), vec![Bouton::Fermer]);
        assert!(pa.deployee && pa.touches.is_some());
        assert_eq!(pa.boutons.iter().map(|b| (b.0, b.2.as_str())).collect::<Vec<_>>(), vec![(Bouton::AnnulerAction, "ANNULER")]);
        assert!(pe.etiquette.as_ref().unwrap().0 == "ERREUR");
    }

    #[test]
    fn sous_le_point_trouve_la_pastille_et_son_bouton() {
        let p = vec![op("COPIE · 3 ÉLÉMENTS", 0.4)];
        let d = mettre_en_page(1600.0, 220.0, 500.0, &p, Some(0), TL, PL, &m);
        let pl = &d.places[0];
        let (_, z, _) = pl.boutons.iter().find(|b| b.0 == Bouton::Annuler).unwrap();
        assert_eq!(d.sous(z.x + 2.0, z.y + 2.0), Some((0, Some(Bouton::Annuler))));
        assert_eq!(d.sous(pl.tete.x + 2.0, pl.tete.y + 2.0), Some((0, None)));
        assert_eq!(d.sous(1.0, 1.0), None);
        // La puce de destination ouvre le dossier
        let (_, zd) = pl.destination.as_ref().unwrap().clone();
        assert_eq!(d.sous(zd.x + 1.0, zd.y + 1.0), Some((0, Some(Bouton::Destination))));
        let env = d.enveloppe().unwrap();
        assert!(dans(&pl.zone, &env));
    }

    #[test]
    fn ordre_operation_file_puis_le_reste() {
        let mut v = vec![Pastille::annulation("A".into()), Pastille::fin("F".into(), None), Pastille::attente("W".into()), op("O", 0.1), Pastille::erreur("E".into(), "x".into())];
        v.sort_by_key(rang_ordre);
        let genres: Vec<Genre> = v.iter().map(|p| p.genre).collect();
        assert_eq!(genres, vec![Genre::Operation, Genre::Attente, Genre::Erreur, Genre::Fin, Genre::Annulation]);
    }

    #[test]
    fn durees_des_pastilles() {
        assert!(!expiree(Genre::Operation, 10_000_000));
        assert!(!expiree(Genre::Attente, 10_000_000));
        assert!(!expiree(Genre::Fin, 4999));
        assert!(expiree(Genre::Fin, 5000));
        assert!(!expiree(Genre::Erreur, 9999));
        assert!(expiree(Genre::Erreur, 10_000));
        assert!(!expiree(Genre::Annulation, 9999));
        assert!(expiree(Genre::Annulation, 10_000));
    }

    #[test]
    fn noms_courts_et_accessibles() {
        assert_eq!(nom_court("COPIE", 1), "COPIE · 1 ÉLÉMENT");
        assert_eq!(nom_court("COPIE", 3), "COPIE · 3 ÉLÉMENTS");
        assert_eq!(op("COPIE · 3 ÉLÉMENTS", 0.42).nom_accessible(), "Copie, 42 pour cent");
        let mut p = op("COPIE · 3 ÉLÉMENTS", 0.5);
        p.en_pause = true;
        assert_eq!(p.nom_accessible(), "Copie, en pause, 50 pour cent");
        assert_eq!(Pastille::attente("DEPLACEMENT · 2 ÉLÉMENTS".into()).nom_accessible(), "Deplacement, en attente");
        assert_eq!(Pastille::fin("COPIE · 1 ÉLÉMENT".into(), None).nom_accessible(), "Copie, terminé");
        assert_eq!(Pastille::erreur("COPIE · 1 ÉLÉMENT".into(), "ACCES REFUSE".into()).nom_accessible(), "Copie, erreur, acces refuse");
        assert!(Pastille::annulation("SUPPRIMÉ · 1 ÉLÉMENT".into()).nom_accessible().contains("Ctrl Z"));
    }

    #[test]
    fn nom_de_dossier_de_la_puce() {
        assert_eq!(nom_dossier(r"C:\Users\n\Documents"), "Documents");
        assert_eq!(nom_dossier(r"C:\Users\n\Documents\"), "Documents");
        assert_eq!(nom_dossier(r"D:\"), r"D:");
        assert_eq!(nom_dossier(r"\\nas\partage"), "partage");
    }

    #[test]
    fn avancement_borne() {
        assert_eq!(Pastille::operation("X".into(), 1.7, false, None).avancement, 1.0);
        assert_eq!(Pastille::operation("X".into(), -1.0, false, None).avancement, 0.0);
    }

    #[test]
    fn un_clic_perime_n_agit_pas_sur_une_autre_pastille() {
        let o = op("COPIE", 0.4).avec_cle(7);
        let a = Pastille::attente("COPIE".into());
        // Le rang visé est maintenant celui d'une pastille en attente : Pause et Annuler n'y existent pas
        assert!(bouton_autorise(&o, Bouton::Pause) && bouton_autorise(&o, Bouton::Annuler));
        assert!(!bouton_autorise(&a, Bouton::Pause) && !bouton_autorise(&a, Bouton::Annuler));
        assert!(!bouton_autorise(&o, Bouton::AnnulerAction));
        assert!(bouton_autorise(&Pastille::annulation("X".into()), Bouton::AnnulerAction));
        // La clé suit la pastille jusque dans sa zone : deux opérations de même rang d'un dessin à l'autre se distinguent
        let d = mettre_en_page(1600.0, 220.0, 500.0, &[o.clone(), op("COPIE", 0.1).avec_cle(8)], None, TL, PL, &m);
        assert_eq!(d.places.iter().map(|p| p.cle).collect::<Vec<_>>(), vec![7, 8]);
    }

    fn tel(phase: PhaseTelechargement, nom: &str, droite: &str, cle: u64) -> Pastille {
        Pastille::telechargement(phase, nom.into(), droite.into(), cle)
    }

    #[test]
    fn telechargement_sans_jauge_ni_annulation() {
        use PhaseTelechargement::*;
        let en_cours = tel(EnCours, "essai.zip", "12,4 MO", 5);
        assert_eq!((en_cours.tete, en_cours.avancement, en_cours.en_pause), (Tete::Grille, 0.0, false));
        assert_eq!(en_cours.boutons, vec![Bouton::Fermer], "✕ masque ; on ne peut pas arrêter le navigateur");
        assert!(!bouton_autorise(&en_cours, Bouton::Annuler) && !bouton_autorise(&en_cours, Bouton::Pause) && !bouton_autorise(&en_cours, Bouton::Afficher));
        let pause = tel(EnPause, "essai.zip", "EN PAUSE · 12,4 MO", 5);
        assert!(pause.en_pause && pause.tete == Tete::Grille, "grille fixe");
        let fini = tel(Fini, "essai.zip", "TÉLÉCHARGÉ · 12,4 MO", 5);
        assert_eq!((fini.tete, fini.boutons.clone()), (Tete::Coche, vec![Bouton::Afficher, Bouton::Fermer]));
        assert!(bouton_autorise(&fini, Bouton::Afficher) && !bouton_autorise(&en_cours, Bouton::Afficher));
        let annule = tel(Annule, "essai.zip", "ANNULÉ", 5);
        assert_eq!((annule.tete, annule.boutons.clone()), (Tete::CroixGrise, vec![Bouton::Fermer]));
        assert!(fini.deployee_en_permanence() && !en_cours.deployee_en_permanence() && !annule.deployee_en_permanence());
        // Aucune jauge ni pourcentage dans la mise en page, mais le mot de droite et, déployée, AFFICHER
        let d = mettre_en_page(1600.0, 220.0, 500.0, &[en_cours.clone(), fini], None, TL, PL, &m);
        assert!(d.places[0].jauge.is_none() && d.places[0].pourcent.is_none());
        assert_eq!(d.places[0].etiquette.as_ref().unwrap().0, "12,4 MO");
        let boutons: Vec<(Bouton, &str)> = d.places[1].boutons.iter().map(|b| (b.0, b.2.as_str())).collect();
        assert_eq!(boutons, vec![(Bouton::Afficher, "AFFICHER"), (Bouton::Fermer, "")]);
        for (_, z, _) in &d.places[1].boutons {
            assert!(dans(z, &d.places[1].zone));
        }
        assert_eq!(d.places[1].cle, 5);
    }

    #[test]
    fn telechargement_durees_et_ordre() {
        use PhaseTelechargement::*;
        assert!(!expiree(Genre::Telechargement(EnCours), 10_000_000) && !expiree(Genre::Telechargement(EnPause), 10_000_000));
        assert!(!expiree(Genre::Telechargement(Fini), 9999) && expiree(Genre::Telechargement(Fini), 10_000));
        assert!(!expiree(Genre::Telechargement(Annule), 4999) && expiree(Genre::Telechargement(Annule), 5000));
        // Les téléchargements qui avancent passent après la file d'opérations, les finis après les fins d'opération
        let mut v = vec![
            Pastille::annulation("A".into()),
            tel(Fini, "f", "", 3),
            Pastille::fin("F".into(), None),
            Pastille::erreur("E".into(), "x".into()),
            tel(EnCours, "c", "", 2),
            Pastille::attente("W".into()),
            op("O", 0.1),
        ];
        v.sort_by_key(rang_ordre);
        assert_eq!(
            v.iter().map(|p| p.genre).collect::<Vec<_>>(),
            vec![Genre::Operation, Genre::Attente, Genre::Telechargement(EnCours), Genre::Erreur, Genre::Fin, Genre::Telechargement(Fini), Genre::Annulation]
        );
    }

    #[test]
    fn telechargement_noms_accessibles() {
        use PhaseTelechargement::*;
        assert_eq!(tel(EnCours, "essai.zip", "12,4 MO · 1,2 MO/S", 1).nom_accessible(), "Téléchargement, essai.zip, en cours, 12,4 mo, 1,2 mo par seconde");
        assert_eq!(tel(EnCours, "TÉLÉCHARGEMENT", "3,0 MO", 1).nom_accessible(), "Téléchargement, en cours, 3,0 mo");
        assert_eq!(tel(EnPause, "essai.zip", "EN PAUSE · 12,4 MO", 1).nom_accessible(), "Téléchargement, essai.zip, en pause, 12,4 mo");
        assert_eq!(tel(Fini, "essai.zip", "TÉLÉCHARGÉ · 12,4 MO", 1).nom_accessible(), "Téléchargement, essai.zip, téléchargé, 12,4 mo");
        assert_eq!(tel(Annule, "essai.zip", "ANNULÉ", 1).nom_accessible(), "Téléchargement, essai.zip, annulé");
    }

    #[test]
    fn telechargement_un_clic_perime_n_agit_pas_ailleurs() {
        // Deux pastilles de téléchargement : chacune garde sa clé dans sa zone, quel que soit son rang
        let a = tel(PhaseTelechargement::Fini, "a", "TÉLÉCHARGÉ · 1 O", crate::navigateur::CLE_BASE | 1);
        let b = tel(PhaseTelechargement::Fini, "b", "TÉLÉCHARGÉ · 1 O", crate::navigateur::CLE_BASE | 2);
        let d = mettre_en_page(1600.0, 220.0, 500.0, &[a, b.clone()], None, TL, PL, &m);
        assert_eq!(d.places.iter().map(|p| p.cle).collect::<Vec<_>>(), vec![crate::navigateur::CLE_BASE | 1, crate::navigateur::CLE_BASE | 2]);
        // Le rang 0 est maintenant `b` : sa clé n'est plus celle dessinée, le contrôle de `clic_bouton_pastille` refuse
        assert_ne!(d.places[0].cle, b.cle);
        assert!(crate::navigateur::est_cle(b.cle) && !crate::navigateur::est_cle(7));
    }

    #[test]
    fn erreur_de_pixel_expire_avec_sa_pastille() {
        assert!(erreur_vivante(true, 0));
        assert!(erreur_vivante(true, ERREUR_MS - 1));
        assert!(!erreur_vivante(true, ERREUR_MS));
        assert!(!erreur_vivante(false, 0));
    }

    #[test]
    fn pourcentage_mesure_agrandit_la_pastille_sans_couper_le_texte() {
        let p = vec![op("COPIE", 0.4)];
        let petit = mettre_en_page(1600.0, 220.0, 500.0, &p, None, TL, 40.0, &m);
        let grand = mettre_en_page(1600.0, 220.0, 500.0, &p, None, TL, 62.0, &m);
        assert!((grand.places[0].zone.l - petit.places[0].zone.l - 22.0).abs() < 0.01);
        assert_eq!(grand.places[0].pourcent.unwrap().l, 62.0);
    }
    // ── Lot 13 : cartes d'information (toast) ──

    #[test]
    fn carte_info_coche_fermer_deployee_et_4_5_s() {
        let p = Pastille::info("Chemin copié".into(), CLE_TOAST | 1);
        assert_eq!((p.genre, p.tete, p.boutons.clone()), (Genre::Info, Tete::Coche, vec![Bouton::Fermer]));
        assert!(p.deployee_en_permanence(), "`card open` dès l'apparition");
        assert!(p.etiquette.is_none() && p.destination.is_none() && p.message.is_none());
        assert_eq!(p.nom_accessible(), "Chemin copié", "lu tel quel, sans majuscule forcée");
        assert!(!expiree(Genre::Info, 4499) && expiree(Genre::Info, 4500), "fermeture automatique à 4,5 s");
        assert_eq!(INFO_MS, 4500);
    }

    #[test]
    fn carte_info_passe_apres_les_operations() {
        let mut v = vec![Pastille::info("a".into(), CLE_TOAST | 1), op("COPIE", 0.5), Pastille::fin("COPIE".into(), None), Pastille::annulation("SUPPRIMÉ".into())];
        v.sort_by_key(rang_ordre);
        assert_eq!(v.iter().map(|p| p.genre).collect::<Vec<_>>(), vec![Genre::Operation, Genre::Fin, Genre::Annulation, Genre::Info]);
    }

    #[test]
    fn carte_info_garde_un_texte_plus_long_que_150_dip() {
        // 30 caractères à 8 DIP = 240 DIP : une carte d'opération le couperait à 150, une carte d'information non
        let long = "Renommé en « rapport final »".to_string();
        assert!(m(&long) > NOM_MAX && m(&long) < NOM_MAX_INFO);
        let d = mettre_en_page(1600.0, 220.0, 700.0, &[Pastille::info(long.clone(), CLE_TOAST | 1)], None, TL, PL, &m);
        assert_eq!(d.places[0].libelle, long, "texte entier");
        assert!(d.places[0].deployee && d.places[0].boutons.iter().any(|b| b.0 == Bouton::Fermer));
        let fin = mettre_en_page(1600.0, 220.0, 700.0, &[Pastille::fin(long.clone(), None)], None, TL, PL, &m);
        assert_ne!(fin.places[0].libelle, long, "la carte de fin, elle, est coupée à 150");
    }

    #[test]
    fn toasts_ajout_expiration_fermeture_et_plafond() {
        let t0 = std::time::Instant::now();
        let ms = |n: u64| t0 + std::time::Duration::from_millis(n);
        let mut t = Toasts::nouveau();
        let a = t.ajouter("Chemin copié".into(), t0);
        let b = t.ajouter("Renommé".into(), ms(1000));
        assert_ne!(a, b);
        assert!(a & CLE_TOAST != 0 && a & (1 << 62) == 0, "clé distincte de celles des opérations et des téléchargements");
        assert_eq!(t.pastilles(ms(2000)).len(), 2);
        // La première expire à 4,5 s, la seconde 1 s plus tard
        assert_eq!(t.pastilles(ms(4500)).len(), 1);
        assert!(t.purger(ms(4600)), "il en reste une");
        assert!(!t.purger(ms(5500)), "plus rien : la minuterie peut s'arrêter");
        assert!(t.pastilles(ms(5500)).is_empty());
        // Le ✕ retire la carte tout de suite
        let c = t.ajouter("x".into(), ms(6000));
        t.fermer(c);
        assert!(t.pastilles(ms(6001)).is_empty());
        // Au plus 3 cartes : la plus ancienne part
        for k in 0..5 {
            t.ajouter(format!("n{k}"), ms(7000));
        }
        let libelles: Vec<String> = t.pastilles(ms(7001)).into_iter().map(|p| p.libelle).collect();
        assert_eq!(libelles, vec!["n2", "n3", "n4"]);
    }
}
