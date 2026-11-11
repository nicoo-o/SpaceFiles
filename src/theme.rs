// Palette et mesures Nothing (en pixels indépendants de la résolution, DIP)

use windows::Win32::Graphics::Direct2D::Common::D2D1_COLOR_F;

use crate::dossier::Colonne;

pub const fn rgba(r: u8, g: u8, b: u8, a: f32) -> D2D1_COLOR_F {
    D2D1_COLOR_F { r: r as f32 / 255.0, g: g as f32 / 255.0, b: b as f32 / 255.0, a }
}

/// Fond des fenêtres : noir pur opaque (maquette v6 `.win`), sans voile translucide ni teinte d'écran.
pub const VOILE: D2D1_COLOR_F = rgba(0, 0, 0, 1.0);
pub const BLANC: D2D1_COLOR_F = rgba(255, 255, 255, 1.0);
// Échelle de texte (maquette v6) : 100 % (valeurs), 65 % (étiquettes, dates, aides, états vides),
// 52 % réservé à « indisponible » et au texte tertiaire (4,5:1 sur le fond, vérifié par un test)
pub const BLANC_DOUX: D2D1_COLOR_F = rgba(255, 255, 255, 0.65);
pub const BLANC_FAIBLE: D2D1_COLOR_F = rgba(255, 255, 255, 0.52);
pub const BLANC_TRES_FAIBLE: D2D1_COLOR_F = rgba(255, 255, 255, 0.14);
/// Voile de survol de base (maquette `--hov`, 10 %)
pub const SURVOL: D2D1_COLOR_F = rgba(255, 255, 255, 0.10);
pub const SURVOL_FORT: D2D1_COLOR_F = rgba(255, 255, 255, 0.10);
/// Reflet qui suit le curseur (lot C3) : halo de fond (9 %) et lueur du contour (55 %). Du blanc, jamais d'ambre ni de cyan ;
/// en clair `contraste()` en fait de l'encre. Les valeurs sont celles de `reflet.rs`.
pub const REFLET_FOND: D2D1_COLOR_F = rgba(255, 255, 255, crate::reflet::ALPHA_HALO);
pub const REFLET_TRAIT: D2D1_COLOR_F = rgba(255, 255, 255, crate::reflet::ALPHA_LUEUR);
pub const POINT_TRAME: D2D1_COLOR_F = rgba(255, 255, 255, 0.06);
/// Séparateurs et traits fins (maquette `--s1`, 7 %)
pub const LIGNE: D2D1_COLOR_F = rgba(255, 255, 255, 0.07);
/// Liseré supérieur (maquette `.win:before`) : blanc 55 %, opacité 70 % au dessin.
pub const LISERE_HAUT: D2D1_COLOR_F = rgba(255, 255, 255, 0.55);
pub const ACCENT: D2D1_COLOR_F = rgba(0xE8, 0xA6, 0x6B, 1.0);
/// Ligne choisie avec focus : accent à 16 %, fond discret avec un point d'accent à gauche
pub const ACCENT_SELECTION: D2D1_COLOR_F = rgba(0xE8, 0xA6, 0x6B, 0.16);
/// Vignette choisie de la grille : contour d'accent à 40 %, sur le fond `SELECTION`
pub const BORDURE_SELECTION_GRILLE: D2D1_COLOR_F = rgba(0xE8, 0xA6, 0x6B, 0.40);
/// Ligne choisie (maquette v6) : fond neutre, le trait d'accent à gauche porte la sélection
pub const SELECTION: D2D1_COLOR_F = rgba(255, 255, 255, 0.07);
/// `SELECTION` en thème clair : la maquette `--sel` vaut 6 %, alors que `LIGNE` vaut 8 % ;
/// même valeur sombre, donc un jeton clair nommé (voir `contraste_ou`).
pub const SELECTION_CLAIR: D2D1_COLOR_F = rgba(16, 16, 20, 0.06);
pub const SELECTION_TERNE: D2D1_COLOR_F = rgba(255, 255, 255, 0.04);
/// Contour de l'élément choisi dans le volet (accent à 35 %, comme `.it.cur`)
pub const ACCENT_BORD: D2D1_COLOR_F = rgba(0xE8, 0xA6, 0x6B, 0.35);
/// Carte de l'aperçu et des surfaces posées sur le fond (maquette `--card`)
pub const CARTE: D2D1_COLOR_F = rgba(0x0B, 0x0C, 0x0F, 1.0);
/// `CARTE` en thème clair : `--card: #fff`, plus clair que le fond #F7F7F8.
pub const CARTE_CLAIR: D2D1_COLOR_F = rgba(255, 255, 255, 1.0);
/// Anneau de focus clavier (maquette `--ring`) : blanc 90 % en sombre, encre 90 % en clair.
pub const ANNEAU: D2D1_COLOR_F = rgba(255, 255, 255, 0.9);
pub const ANNEAU_CLAIR: D2D1_COLOR_F = rgba(16, 16, 20, 0.9);
/// Panneau de l'aperçu (maquette `--panel`)
pub const PANNEAU: D2D1_COLOR_F = rgba(255, 255, 255, 0.025);
/// Texte sélectionné dans un champ, dossier visé par un dépôt : accent à 55 %, doit rester franc
pub const ACCENT_VISE: D2D1_COLOR_F = rgba(0xE8, 0xA6, 0x6B, 0.55);
/// Erreurs et échecs seulement (message d'erreur, disque presque plein) : toujours accompagnée d'un mot ou d'un signe
pub const ERREUR: D2D1_COLOR_F = rgba(0xF0, 0x83, 0x6B, 1.0);
/// Rouge du groupe et de la balise « rouge » (l'ancien rouge Nothing #D71921) : couleur de l'utilisateur, pas un accent
pub const GROUPE_ROUGE: D2D1_COLOR_F = rgba(0xD7, 0x19, 0x21, 1.0);
/// Texte posé sur un aplat d'accent (le blanc sur l'ambre est illisible)
pub const ENCRE_SUR_ACCENT: D2D1_COLOR_F = rgba(0x0A, 0x0A, 0x0A, 1.0);
/// Teinte « Nombre » de l'aperçu de code (ambre plus clair que l'accent). En sombre elle reste telle quelle ; en clair elle
/// vaudrait 1,79:1 sur le fond : `vers_clair` l'envoie sur l'ambre foncé de l'accent clair (6,36:1)
pub const TEINTE_NOMBRE: D2D1_COLOR_F = rgba(232, 178, 90, 1.0);
pub const CHAMP_FOND: D2D1_COLOR_F = rgba(8, 8, 8, 0.96);
/// Cyan de Pixel le jour (6 h à 18 h) : couleur d'identité de Pixel, pas une couleur d'interface, elle ne sert à rien d'autre.
/// Le soir Pixel prend `ACCENT`, la nuit `BLANC_FAIBLE`. Dessin non textuel : seuil 3:1 (informatif dans l'audit)
pub const PIXEL_JOUR: D2D1_COLOR_F = rgba(0x7F, 0xE6, 0xFF, 1.0);
/// `PIXEL_JOUR` en thème clair : `vers_clair` l'y envoie (le cyan clair sur fond clair serait invisible)
pub const PIXEL_JOUR_CLAIR: D2D1_COLOR_F = rgba(0x0B, 0x7A, 0x99, 1.0);
/// Disque posé sous la case à cocher d'une vignette de la grille (survol, sélection multiple). Même valeur qu'un voile de
/// fond modal : c'est pourquoi `contraste_ou` lui donne sa valeur claire par jeton au lieu de la deviner à la teinte.
pub const DISQUE_CASE: D2D1_COLOR_F = rgba(0, 0, 0, 0.55);
/// `DISQUE_CASE` en thème clair : surface claire translucide (l'encre de la case reste lisible dessus)
pub const DISQUE_CASE_CLAIR: D2D1_COLOR_F = rgba(247, 247, 248, 0.85);

/// Titres, chiffres, étiquettes en majuscules : installée dans Windows (C:\Windows\Fonts\Doto.ttf), appelée par son nom
pub const POLICE: &str = "Doto";
/// Code, terminal et touches de raccourci : embarquée dans l'exe (`assets/fonts/JetBrainsMono-Regular.ttf`)
pub const POLICE_TEXTE: &str = "JetBrains Mono";
/// Si le chargement de la police embarquée échoue : livrée avec Windows 11
pub const POLICE_TEXTE_SECOURS: &str = "Cascadia Mono";

// ── Thème clair (phase 7 lot B) ──
// Le code peint en sombre (blancs translucides sur voile noir) : le thème clair est une transformation de ces jetons au
// point de passage unique `contraste`, jamais un second jeu de constantes, pour que « même design, valeurs différentes »
// reste vrai partout (y compris les couleurs écrites en dur dans rendu.rs).

use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU8, Ordering};

use crate::eco::partage::Theme as AvisTheme;

/// Thème effectif de la fenêtre : vrai = clair. Jamais vrai sous le thème Contraste élevé de Windows (il reste prioritaire)
static CLAIR: AtomicBool = AtomicBool::new(false);
/// Thème dont la fenêtre (DWM, images) porte la marque : `a_appliquer` le compare à `CLAIR`, qu'un réglage peut avoir
/// déjà changé (`reglages::appliquer` recalcule sans connaître la fenêtre)
static APPLIQUE: AtomicBool = AtomicBool::new(false);
/// Réglage local (`ChoixTheme` en octet), posé par `reglages::appliquer`
static CHOIX_LOCAL: AtomicU8 = AtomicU8::new(0);

pub fn clair() -> bool {
    CLAIR.load(Ordering::Relaxed)
}

/// Réglage `theme` de `reglages.toml`
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum ChoixTheme {
    /// Suit l'avis de `shared.json` s'il y en a un, sinon sombre
    #[default]
    Auto,
    Sombre,
    Clair,
    /// Suit le thème des applications de Windows
    Systeme,
}

impl ChoixTheme {
    pub const TOUS: [ChoixTheme; 4] = [ChoixTheme::Auto, ChoixTheme::Sombre, ChoixTheme::Clair, ChoixTheme::Systeme];

    pub fn cle(self) -> &'static str {
        match self {
            ChoixTheme::Auto => "auto",
            ChoixTheme::Sombre => "sombre",
            ChoixTheme::Clair => "clair",
            ChoixTheme::Systeme => "systeme",
        }
    }

    /// Valeur inconnue : `Auto` (jamais d'erreur sur un fichier retouché à la main)
    pub fn depuis_cle(cle: &str) -> Self {
        let cle = cle.trim().to_lowercase();
        Self::TOUS.into_iter().find(|c| c.cle() == cle).unwrap_or_default()
    }

    pub fn libelle(self) -> &'static str {
        match self {
            ChoixTheme::Auto => "Auto",
            ChoixTheme::Sombre => "Sombre",
            ChoixTheme::Clair => "Clair",
            ChoixTheme::Systeme => "Système",
        }
    }
}

/// Précédence du thème (phase 5) : réglage local explicite > avis de `shared.json` > Windows > sombre. Vrai = clair.
/// `Auto` suit l'avis partagé (`system` renvoie à Windows), sinon sombre ; `Systeme` suit Windows.
pub fn theme_effectif(local: ChoixTheme, avis_partage: Option<AvisTheme>, windows_clair: bool) -> bool {
    match local {
        ChoixTheme::Sombre => false,
        ChoixTheme::Clair => true,
        ChoixTheme::Systeme => windows_clair,
        ChoixTheme::Auto => match avis_partage {
            Some(AvisTheme::Light) => true,
            Some(AvisTheme::System) => windows_clair,
            Some(AvisTheme::Dark) | None => false,
        },
    }
}

/// Les applications de Windows sont en thème clair (`AppsUseLightTheme`). Sans registre (`SPACEFILES_SANS_REGISTRE=1`),
/// ou valeur absente : sombre, le thème par défaut de SpaceFiles.
fn windows_clair() -> bool {
    use windows::Win32::System::Registry::{RegGetValueW, HKEY_CURRENT_USER, RRF_RT_REG_DWORD};
    if crate::defaut::sans_registre() {
        return false;
    }
    let mut v = 0u32;
    let mut taille = 4u32;
    let cle = windows::core::HSTRING::from(r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize");
    // SAFETY: `v` est un u32 local de 4 octets, taille annoncée 4 ; RRF_RT_REG_DWORD n'écrit rien d'autre
    let r = unsafe { RegGetValueW(HKEY_CURRENT_USER, &cle, &windows::core::HSTRING::from("AppsUseLightTheme"), RRF_RT_REG_DWORD, None, Some(&mut v as *mut u32 as *mut _), Some(&mut taille)) };
    r.is_ok() && v != 0
}

/// Mémorise le réglage local `theme` (appelé par `reglages::appliquer`, avant `recalculer`)
pub fn fixer_choix(c: ChoixTheme) {
    CHOIX_LOCAL.store(c as u8, Ordering::Relaxed);
}

/// Recalcule le thème effectif (réglage local, avis partagé relu au plus une fois par seconde, Windows) ; vrai s'il a
/// changé. La fenêtre se cale ensuite avec `a_appliquer`.
pub fn recalculer() -> bool {
    let local = ChoixTheme::TOUS[(CHOIX_LOCAL.load(Ordering::Relaxed) as usize).min(3)];
    let avis = crate::eco::partage::theme_partage();
    let veut_windows = local == ChoixTheme::Systeme || (local == ChoixTheme::Auto && avis == Some(AvisTheme::System));
    let clair = theme_effectif(local, avis, veut_windows && windows_clair()) && !crate::reglages::CONTRASTE_WINDOWS.load(Ordering::Relaxed);
    CLAIR.swap(clair, Ordering::Relaxed) != clair
}

/// Vrai si le thème effectif diffère de celui que la fenêtre porte ; l'appelant règle alors DWM, recrée les images fixes et
/// invalide. Au démarrage, appelée une fois après le réglage initial de DWM pour caler la marque.
pub fn a_appliquer() -> bool {
    let c = clair();
    APPLIQUE.swap(c, Ordering::Relaxed) != c
}

/// Valeurs du thème clair (maquette `.win[data-t=light]`)
const ENCRE_CLAIRE: (u8, u8, u8) = (16, 16, 20);
const FOND_CLAIR: (u8, u8, u8) = (247, 247, 248);
pub const ACCENT_CLAIR: D2D1_COLOR_F = rgba(0x8A, 0x52, 0x17, 1.0);
pub const ERREUR_CLAIRE: D2D1_COLOR_F = rgba(0xB3, 0x40, 0x2B, 1.0);
pub const BLANC_PUR: D2D1_COLOR_F = rgba(255, 255, 255, 1.0);

/// Alpha de l'encre claire pour un blanc sombre d'alpha `a`. Les paliers de la maquette sont exacts ; toute autre valeur
/// (fondus, apparition, opacités calculées) garde son alpha d'entrée, borné à [0 ; 1] : règle simple, le fondu reste un fondu.
fn alpha_encre(a: f32) -> f32 {
    const PALIERS: [(f32, f32); 11] = [(1.0, 0.95), (0.75, 0.68), (0.65, 0.68), (0.55, 0.40), (0.52, 0.60), (0.45, 0.55), (0.14, 0.16), (0.10, 0.09), (0.09, 0.06), (0.07, 0.08), (0.06, 0.05)];
    PALIERS.iter().find(|(entree, _)| (a - entree).abs() < 0.005).map_or(a.clamp(0.0, 1.0), |&(_, sortie)| sortie)
}

fn meme_teinte(c: &D2D1_COLOR_F, k: &D2D1_COLOR_F) -> bool {
    (c.r - k.r).abs() < 0.002 && (c.g - k.g).abs() < 0.002 && (c.b - k.b).abs() < 0.002
}

/// Un jeton sombre vu en thème clair. Sans effet sur ce qui n'est pas listé (couleurs de groupes, de balises, `GROUPE_ROUGE`).
pub fn vers_clair(c: &D2D1_COLOR_F) -> D2D1_COLOR_F {
    let avec_alpha = |k: (u8, u8, u8), a: f32| rgba(k.0, k.1, k.2, a);
    // Texte posé sur un aplat d'accent : alpha 1 exactement (le VOILE a la même teinte mais 0,92)
    if meme_teinte(c, &ENCRE_SUR_ACCENT) && c.a >= 0.99 {
        return D2D1_COLOR_F { a: c.a, ..BLANC_PUR };
    }
    if meme_teinte(c, &ACCENT) {
        return D2D1_COLOR_F { a: c.a, ..ACCENT_CLAIR };
    }
    if meme_teinte(c, &ERREUR) {
        return D2D1_COLOR_F { a: c.a, ..ERREUR_CLAIRE };
    }
    if meme_teinte(c, &TEINTE_NOMBRE) {
        return D2D1_COLOR_F { a: c.a, ..ACCENT_CLAIR };
    }
    if meme_teinte(c, &PIXEL_JOUR) {
        return D2D1_COLOR_F { a: c.a, ..PIXEL_JOUR_CLAIR };
    }
    if c.r > 0.99 && c.g > 0.99 && c.b > 0.99 {
        return avec_alpha(ENCRE_CLAIRE, alpha_encre(c.a));
    }
    // Surfaces presque noires et presque opaques : voile, champs, cartes de menu. Les ombres et les voiles de fond
    // modaux (noir translucide, alpha < 0,9) restent noirs.
    if c.r < 0.1 && c.g < 0.1 && c.b < 0.1 && c.a >= 0.9 {
        return if meme_teinte(c, &CHAMP_FOND) { D2D1_COLOR_F { a: c.a, ..BLANC_PUR } } else { avec_alpha(FOND_CLAIR, c.a) };
    }
    *c
}

/// Couleur peinte une fois pour toutes (icônes et trame en images, créées avec la cible) : suit le thème, mais pas le
/// contraste renforcé ni le thème Contraste élevé de Windows (comme avant)
pub fn teinte_fixe(c: &D2D1_COLOR_F) -> D2D1_COLOR_F {
    if clair() { vers_clair(c) } else { *c }
}

/// Point de passage unique de toute couleur avant le pinceau (`rendu.rs`, `couleur`) : thème clair, contraste renforcé,
/// thème Contraste élevé de Windows. En sombre sans contraste, la couleur sort inchangée (au bit près).
pub fn contraste(c: &D2D1_COLOR_F) -> D2D1_COLOR_F {
    use crate::reglages::{CONTRASTE, CONTRASTE_WINDOWS};
    transformer(c, clair(), CONTRASTE.load(Ordering::Relaxed), CONTRASTE_WINDOWS.load(Ordering::Relaxed))
}

/// Comme `contraste` sans le palier du contraste renforcé (qui presque triplerait le halo du reflet : 9 % deviendrait 20 %) :
/// thème clair oui, renforcé non. Contraste élevé de Windows : le reflet n'existe pas, donc jamais appelée dans ce cas.
pub fn sans_renfort(c: &D2D1_COLOR_F) -> D2D1_COLOR_F {
    transformer(c, clair(), false, false)
}

/// Comme `contraste`, mais en thème clair la couleur est `en_clair` (jeton nommé) au lieu de la transformation par teinte :
/// pour les couleurs dont la valeur sombre est ambiguë (noir translucide de disque, identique à un voile de fond)
pub fn contraste_ou(c: &D2D1_COLOR_F, en_clair: &D2D1_COLOR_F) -> D2D1_COLOR_F {
    use crate::reglages::{CONTRASTE, CONTRASTE_WINDOWS};
    transformer_ou(c, en_clair, clair(), CONTRASTE.load(Ordering::Relaxed), CONTRASTE_WINDOWS.load(Ordering::Relaxed))
}

fn transformer_ou(c: &D2D1_COLOR_F, en_clair: &D2D1_COLOR_F, clair: bool, renforce: bool, windows: bool) -> D2D1_COLOR_F {
    if clair && !windows {
        return *en_clair;
    }
    transformer(c, clair, renforce, windows)
}

fn identique(a: &D2D1_COLOR_F, b: &D2D1_COLOR_F) -> bool {
    a.r.to_bits() == b.r.to_bits() && a.g.to_bits() == b.g.to_bits() && a.b.to_bits() == b.b.to_bits() && a.a.to_bits() == b.a.to_bits()
}

/// Cœur de `contraste`, état passé en paramètres (testable sans état global). Contraste élevé de Windows d'abord : ni thème
/// clair ni accent ne s'appliquent s'il est actif.
pub(crate) fn transformer(c: &D2D1_COLOR_F, clair: bool, renforce: bool, windows: bool) -> D2D1_COLOR_F {
    if windows {
        return contraste_avec(c, true);
    }
    if clair {
        let x = vers_clair(c);
        // Contraste renforcé : les paliers d'alpha du sombre, lus sur la couleur d'origine (blanc 52 % -> 85 %, voile -> opaque),
        // pour les seules couleurs que `vers_clair` a transformées : un voile modal noir translucide n'est pas rendu opaque
        return if renforce && !identique(&x, c) { D2D1_COLOR_F { a: contraste_avec(c, false).a, ..x } } else { x };
    }
    if renforce {
        return contraste_avec(c, false);
    }
    *c
}

/// Contraste renforcé : les blancs translucides (textes secondaires, traits, survols, trame) deviennent francs et le voile
/// opaque. L'accent, l'erreur et les couleurs de groupes gardent leur teinte. Le drapeau Contraste élevé de Windows est
/// passé en paramètre (testable sans état global) : vrai, les couleurs viennent du système.
fn contraste_avec(c: &D2D1_COLOR_F, contraste_windows: bool) -> D2D1_COLOR_F {
    let blanc = c.r > 0.99 && c.g > 0.99 && c.b > 0.99 && c.a < 1.0;
    let voile = c.r < 0.05 && c.g < 0.05 && c.b < 0.05 && c.a > 0.5;
    // Thème Contraste élevé de Windows : fond, texte et accent viennent des couleurs que l'utilisateur a choisies
    if contraste_windows {
        use windows::Win32::Graphics::Gdi::{GetSysColor, COLOR_HOTLIGHT, COLOR_WINDOW, COLOR_WINDOWTEXT};
        let couleur = |i| {
            // SAFETY: GetSysColor lit une table du système, sans pointeur ; valeur COLORREF 0x00BBGGRR
            let v = unsafe { GetSysColor(i) };
            rgba((v & 0xFF) as u8, (v >> 8 & 0xFF) as u8, (v >> 16 & 0xFF) as u8, 1.0)
        };
        let proche = |k: &D2D1_COLOR_F| (c.r - k.r).abs() < 0.02 && (c.g - k.g).abs() < 0.02 && (c.b - k.b).abs() < 0.02;
        let rouge = proche(&ACCENT) || proche(&ERREUR);
        if voile {
            return couleur(COLOR_WINDOW);
        }
        if rouge {
            return D2D1_COLOR_F { a: 1.0, ..couleur(COLOR_HOTLIGHT) };
        }
        if blanc || (c.r > 0.99 && c.g > 0.99 && c.b > 0.99) {
            let t = couleur(COLOR_WINDOWTEXT);
            let a = match c.a {
                a if a >= 0.25 => 1.0,
                a if a >= 0.12 => 0.55,
                a if a >= 0.05 => 0.2,
                _ => 0.0,
            };
            return D2D1_COLOR_F { a, ..t };
        }
        return *c;
    }
    let a = if voile {
        1.0
    } else if blanc {
        match c.a {
            a if a >= 0.6 => 1.0,
            a if a >= 0.25 => 0.85,
            a if a >= 0.12 => 0.55,
            a if a >= 0.09 => 0.4,
            a if a >= 0.05 => 0.2,
            _ => 0.0,
        }
    } else {
        c.a
    };
    D2D1_COLOR_F { a, ..*c }
}

pub const HAUTEUR_TITRE: f32 = 52.0;
/// Largeur du volet quand il est affiché. Le reste du code passe par `largeur_volet()`.
pub const LARGEUR_VOLET: f32 = 268.0;
/// Seuil du responsive de la maquette (`@media (max-width:1100px)`) et largeur du volet à ce seuil.
pub const LARGEUR_ETROITE: f32 = 1100.0;
pub const LARGEUR_VOLET_ETROIT: f32 = 240.0;
/// Dernière largeur client vue au dessin (DIP), pour les appels sans largeur des messages souris.
static ETROIT: AtomicBool = AtomicBool::new(false);

/// Mémorise la largeur client (DIP) avant le calcul de la disposition. Le responsive ne change pas le réglage utilisateur.
pub fn memoriser_largeur(l: f32) {
    ETROIT.store(l <= LARGEUR_ETROITE, Ordering::Relaxed);
}

/// Largeur du volet pour une largeur de fenêtre donnée : nulle en mode Essentiel, 240 px à 1100 px ou moins.
pub fn largeur_volet_pour(l: f32, essentiel: bool) -> f32 {
    if essentiel {
        0.0
    } else if l <= LARGEUR_ETROITE {
        LARGEUR_VOLET_ETROIT
    } else {
        LARGEUR_VOLET
    }
}

/// Largeur voulue du volet (la cible de l'animation) : nulle en mode Essentiel (F9), où il est replié et laisse toute la place à la liste
pub fn largeur_volet_cible() -> f32 {
    let essentiel = crate::app::ESSENTIEL.load(std::sync::atomic::Ordering::Relaxed);
    largeur_volet_pour(if ETROIT.load(Ordering::Relaxed) { LARGEUR_ETROITE } else { LARGEUR_ETROITE + 1.0 }, essentiel)
}

/// Largeur du volet et de l'aperçu **telles qu'affichées** pendant leur glissement (lot 12D, `.body` de la maquette : colonnes de la
/// grille en 500 ms au ressort). Bits d'un `f32` ; `u32::MAX` (un NaN) = pas d'animation, on prend la cible. Posées par le rendu à
/// chaque image avant le calcul de la disposition, lues partout où la largeur sert (dessin, zones de clic, colonnes).
static VOLET_AFFICHE: AtomicU32 = AtomicU32::new(u32::MAX);
static APERCU_AFFICHE: AtomicU32 = AtomicU32::new(u32::MAX);

fn lire_affiche(a: &AtomicU32) -> Option<f32> {
    let v = f32::from_bits(a.load(Ordering::Relaxed));
    (!v.is_nan()).then_some(v)
}

/// Pose les largeurs affichées du volet et de l'aperçu ; `None` : retour à la cible (repos, mouvement réduit)
pub fn poser_largeurs_affichees(volet: Option<f32>, apercu: Option<f32>) {
    VOLET_AFFICHE.store(volet.map_or(u32::MAX, f32::to_bits), Ordering::Relaxed);
    APERCU_AFFICHE.store(apercu.map_or(u32::MAX, f32::to_bits), Ordering::Relaxed);
}

/// Largeur utile du volet : celle qui est affichée (en glissement) ou sa cible
pub fn largeur_volet() -> f32 {
    lire_affiche(&VOLET_AFFICHE).map_or_else(largeur_volet_cible, |v| v.max(0.0))
}

/// L'aperçu reste un réglage utilisateur, mais il est masqué par le responsive à 1100 px et moins.
pub fn apercu_visible_pour(l: f32, demande: bool) -> bool {
    demande && l > LARGEUR_ETROITE
}
pub const PAS_TRAME: f32 = 10.0;
pub const BORD_REDIM: f32 = 6.0;
/// Boutons de fenêtre (réduire, agrandir, fermer), alignés à droite
pub const BOUTON_LARGEUR: f32 = 40.0;

// Volet de gauche : en-tête FAVORIS, favoris, espace, en-tête DISQUES, disques
pub const VOLET_Y: f32 = HAUTEUR_TITRE + 12.0;
pub const VOLET_ENTETE: f32 = 30.0;
/// Hauteur d'une ligne du volet : 38 DIP, ou 30 en volet dense (réglage DENSITE DU VOLET)
pub fn volet_ligne() -> f32 {
    if crate::reglages::VOLET_DENSE.load(std::sync::atomic::Ordering::Relaxed) { 30.0 } else { 38.0 }
}
pub const VOLET_ESPACE: f32 = 12.0;
/// Marge intérieure du volet (maquette v6 : padding 12)
pub const VOLET_PAD: f32 = 12.0;
/// Carte d'un disque dans le volet (nom, jauge, place libre)
pub const VOLET_DISQUE: f32 = 78.0;

/// Panneau Réglages : rail de navigation de 236 DIP et marge du contenu
pub const REGLAGES_NAV: f32 = 236.0;
pub const REGLAGES_MARGE: f32 = 24.0;
/// Page des réglages en cartes (maquette v6 `.pg`, `.scard`, `.sw`, `.seg`) : padding 24 / 32 de la page
pub const REGLAGES_PAD_X: f32 = 32.0;
pub const REGLAGES_PAD_Y: f32 = 24.0;
/// Carte d'un réglage : 70 de haut (62 au minimum dans la maquette, plus un titre et deux lignes de description),
/// marge haute 10, rayon 16, tuile d'icône de 36 au rayon 12
pub const REGLAGE_CARTE_H: f32 = 70.0;
pub const REGLAGE_CARTE_ECART: f32 = 10.0;
pub const REGLAGE_CARTE_RAYON: f32 = 16.0;
pub const REGLAGE_TUILE: f32 = 36.0;
pub const REGLAGE_TUILE_RAYON: f32 = 12.0;
/// Interrupteur 40 x 24 (rayon 12), bille de 16 décalée de 16 quand il est actif
pub const REGLAGE_INTER_L: f32 = 40.0;
pub const REGLAGE_INTER_H: f32 = 24.0;
pub const REGLAGE_BILLE: f32 = 16.0;
/// Contrôle segmenté : hauteur, rayon du cadre
pub const REGLAGE_SEG_H: f32 = 28.0;
pub const REGLAGE_SEG_RAYON: f32 = 12.0;

// Barre de navigation : précédent, suivant, parent, puis le fil d'Ariane
// Bloc de 58 DIP : 14 en haut, 38 de contenu, 6 en bas. Le chemin commence donc à 52.
pub const NAV_Y: f32 = HAUTEUR_TITRE + 14.0;
pub const NAV_H: f32 = 38.0;
pub const NAV_BOUTON: f32 = 32.0;
pub fn nav_x() -> f32 {
    largeur_volet() + 16.0
}
pub fn chemin_x() -> f32 {
    nav_x() + 3.0 * NAV_BOUTON + 14.0
}

// Barre d'actions rapides, sous le chemin : bloc de 56 DIP (8 + 36 + 12)
pub const BARRE_Y: f32 = NAV_Y + NAV_H + 6.0;
pub const BARRE_H: f32 = 56.0;
pub const BARRE_PAD_HAUT: f32 = 8.0;

// Liste des fichiers : en-tête à 166, liste à 200
pub const ENTETE_Y: f32 = BARRE_Y + BARRE_H;
pub const ENTETE_H: f32 = 34.0;
pub const LISTE_Y: f32 = ENTETE_Y + ENTETE_H;
/// Hauteur d'une ligne de la liste : 30 DIP en compact, 40 DIP en confort
pub fn hauteur_ligne() -> f32 {
    if crate::reglages::lignes_confort() { 40.0 } else { 30.0 }
}
pub const STATUT_H: f32 = 36.0;

/// Amplitude du soulèvement des éléments de chrome survolés (maquette `.chrome:hover`) ; les durées sont dans `mouvement.rs`.
pub const SURVOL_DECALAGE: f32 = -2.0;

/// Courbe du ressort de la maquette (`--spring`) : x1 .2, y1 1.25, x2 .3, y2 1.
/// Retourne y pour x dans [0, 1] ; le dépassement au milieu est voulu.
pub fn ressort(x: f32) -> f32 {
    bezier(x, 0.2, 1.25, 0.3, 1.0)
}

/// `ease` du CSS (`transition` sans courbe) : cubic-bezier(.25, .1, .25, 1), celle des fondus de fond de la maquette
pub fn ease(x: f32) -> f32 {
    bezier(x, 0.25, 0.1, 0.25, 1.0)
}

/// Sortie de la maquette (`cubic-bezier(.4, 0, 1, 1)`, celle de `replay`) : part doucement, finit vite
pub fn ease_in(x: f32) -> f32 {
    bezier(x, 0.4, 0.0, 1.0, 1.0)
}

/// Mot-clé `ease-in` du CSS, `cubic-bezier(.42, 0, 1, 1)` : suppression d'une ligne (l. 565) et sortie d'une carte d'opération (l. 472)
pub fn ease_in_css(x: f32) -> f32 {
    bezier(x, 0.42, 0.0, 1.0, 1.0)
}

/// `cubic-bezier(.5, 0, .8, .4)` : fermeture de la palette (l. 645), part lentement et finit vite
pub fn ease_palette_fermeture(x: f32) -> f32 {
    bezier(x, 0.5, 0.0, 0.8, 0.4)
}

/// `cubic-bezier(.4, 0, .2, 1)` : fermeture de l'aperçu flottant (l. 497)
pub fn ease_standard(x: f32) -> f32 {
    bezier(x, 0.4, 0.0, 0.2, 1.0)
}

/// Courbe de Bézier CSS `cubic-bezier(x1, y1, x2, y2)` : y pour x dans [0, 1] (0 avant, 1 après), x cherché par dichotomie
pub fn bezier(x: f32, x1: f32, y1: f32, x2: f32, y2: f32) -> f32 {
    if x <= 0.0 {
        return 0.0;
    }
    if x >= 1.0 {
        return 1.0;
    }
    let bx = |u: f32| 3.0 * (1.0 - u).powi(2) * u * x1 + 3.0 * (1.0 - u) * u * u * x2 + u.powi(3);
    let by = |u: f32| 3.0 * (1.0 - u).powi(2) * u * y1 + 3.0 * (1.0 - u) * u * u * y2 + u.powi(3);
    let (mut bas, mut haut) = (0.0, 1.0);
    for _ in 0..24 {
        let milieu = (bas + haut) / 2.0;
        if bx(milieu) < x {
            bas = milieu;
        } else {
            haut = milieu;
        }
    }
    by((bas + haut) / 2.0)
}
/// Bande de la chronologie en points, au-dessus de la barre d'état
pub const CHRONO_H: f32 = 30.0;
pub const MARGE_LISTE: f32 = 24.0;
/// Marge droite de la zone des fichiers (liste, grille, colonnes) : entre la fin des lignes et la barre de défilement ou le volet
/// d'aperçu. Elle est vide et cliquable (menu du fond, lasso, dépôt) : avec un dossier plein il n'y avait plus aucun endroit où
/// cliquer. La maquette met 16 DIP de marge (`.list{padding:8px 16px}`) ; 40 laisse une bande qu'on atteint à la souris.
pub const MARGE_LISTE_DROITE: f32 = 40.0;

/// Vide ajouté sous la dernière ligne, au bout du défilement : trois lignes, pour pouvoir cliquer le fond (menu « Coller »,
/// lasso, dépôt) même quand le dossier remplit la fenêtre
pub fn marge_basse_liste() -> f32 {
    3.0 * hauteur_ligne()
}

/// Hauteur défilable de `rangees` rangées de `hauteur_rangee` DIP, marge basse comprise
pub fn hauteur_defilable(rangees: usize, hauteur_rangee: f32) -> f32 {
    rangees as f32 * hauteur_rangee + marge_basse_liste()
}

/// Défilement maximal d'une liste de `rangees` rangées dans une fenêtre de `hauteur_liste` DIP : la marge basse peut remonter
/// jusqu'en bas de la zone visible
pub fn defilement_maximal(rangees: usize, hauteur_rangee: f32, hauteur_liste: f32) -> f32 {
    (hauteur_defilable(rangees, hauteur_rangee) - hauteur_liste).max(0.0)
}

/// Épaisseur de l'anneau de focus (blanc, 2 DIP)
pub const ANNEAU_EPAISSEUR: f32 = 2.0;

/// Rectangle (x, y, largeur, hauteur) que suit le trait de l'anneau de focus d'une capsule de liste ou de grille `capsule`.
/// Le trait est centré sur ce rectangle : en le rentrant d'une demi-épaisseur, son bord extérieur coïncide avec la capsule
/// et ne sort jamais de la ligne. Avant, l'anneau débordait de 2 DIP et le clip de la liste en rognait le haut sur la
/// première ligne (et le bas sur la dernière).
pub fn trait_anneau_dans(capsule: [f32; 4]) -> [f32; 4] {
    let d = ANNEAU_EPAISSEUR / 2.0;
    [capsule[0] + d, capsule[1] + d, (capsule[2] - 2.0 * d).max(0.0), (capsule[3] - 2.0 * d).max(0.0)]
}

/// Onglets, dans la barre de titre après le nom de l'application (et non après le volet)
pub const ONGLETS_X: f32 = 190.0;
pub const ONGLET_MIN: f32 = 130.0;
pub const ONGLET_MAX: f32 = 190.0;
pub const ONGLET_ECART: f32 = 6.0;
/// Trois icônes de 32 DIP avant les contrôles système, à droite de la barre haute
pub const BOUTON_HAUT: f32 = 32.0;
pub const ECART_HAUT: f32 = 6.0;
pub const MARGE_HAUT: f32 = 12.0;
/// Volet d'aperçu à droite (Alt+P)
pub const LARGEUR_APERCU: f32 = 288.0;

/// Largeur voulue du volet d'aperçu (la cible de l'animation) : 288 DIP s'il est affiché, sinon 0
pub fn largeur_apercu_cible(l: f32, apercu: bool) -> f32 {
    if apercu_visible_pour(l, apercu) { LARGEUR_APERCU } else { 0.0 }
}

/// Largeur affichée du volet d'aperçu : en glissement la valeur posée par le rendu, sinon la cible
pub fn largeur_apercu(l: f32, apercu: bool) -> f32 {
    lire_affiche(&APERCU_AFFICHE).map_or_else(|| largeur_apercu_cible(l, apercu), |v| v.clamp(0.0, LARGEUR_APERCU))
}

/// Largeur occupée par la liste (la fenêtre moins le volet d'aperçu, tel qu'affiché)
pub fn largeur_liste(l: f32, apercu: bool) -> f32 {
    let a = largeur_apercu(l, apercu);
    if a > 0.0 {
        let essentiel = crate::app::ESSENTIEL.load(std::sync::atomic::Ordering::Relaxed);
        (l - a).max(largeur_volet_pour(l, essentiel).max(largeur_volet()) + 300.0)
    } else {
        l
    }
}

/// Colonnes de la liste pour une fenêtre de largeur `l` : (colonne, x, largeur).
/// Quand la place manque, le type puis la date disparaissent (largeur nulle) pour laisser lire le nom.
pub fn colonnes(l: f32) -> [(Colonne, f32, f32); 9] {
    use crate::reglages::{colonne_cachee, COL_DATE, COL_TAILLE, COL_TYPE};
    let date_voulue = if colonne_cachee(COL_DATE) { 0.0 } else { 190.0 };
    use crate::reglages::{colonne_montree, COL_AUTEUR, COL_DIMENSIONS, COL_DUREE, COL_ORIGINE, COL_SIGNE};
    let (mut date, mut typ, taille) = (date_voulue, if colonne_cachee(COL_TYPE) { 0.0 } else { 110.0 }, if colonne_cachee(COL_TAILLE) { 0.0 } else { 110.0 });
    let (mut origine, mut signe) = (if colonne_montree(COL_ORIGINE) { 150.0 } else { 0.0 }, if colonne_montree(COL_SIGNE) { 170.0 } else { 0.0 });
    let (mut dims, mut duree, mut auteur) = (
        if colonne_montree(COL_DIMENSIONS) { 120.0 } else { 0.0 },
        if colonne_montree(COL_DUREE) { 80.0 } else { 0.0 },
        if colonne_montree(COL_AUTEUR) { 150.0 } else { 0.0 },
    );
    let x0 = largeur_volet() + MARGE_LISTE;
    let fin = l - MARGE_LISTE_DROITE;
    const NOM_MIN: f32 = 220.0;
    let reste = |date: f32, typ: f32, dims: f32, duree: f32, auteur: f32, origine: f32, signe: f32| fin - x0 - date - typ - taille - dims - duree - auteur - origine - signe;
    // Place qui manque : l'auteur part d'abord, puis le type (l'extension se lit dans le nom), puis la
    // durée et les dimensions (demandées exprès), puis la date
    if reste(date, typ, dims, duree, auteur, origine, signe) < NOM_MIN {
        signe = 0.0;
    }
    if reste(date, typ, dims, duree, auteur, origine, signe) < NOM_MIN {
        origine = 0.0;
    }
    if reste(date, typ, dims, duree, auteur, origine, signe) < NOM_MIN {
        auteur = 0.0;
    }
    if reste(date, typ, dims, duree, auteur, origine, signe) < NOM_MIN {
        typ = 0.0;
    }
    if reste(date, typ, dims, duree, auteur, origine, signe) < NOM_MIN {
        duree = 0.0;
    }
    if reste(date, typ, dims, duree, auteur, origine, signe) < NOM_MIN {
        dims = 0.0;
    }
    if reste(date, typ, dims, duree, auteur, origine, signe) < NOM_MIN {
        date = 0.0;
    }
    let nom = reste(date, typ, dims, duree, auteur, origine, signe).max(120.0);
    // Ordre à l'écran : nom, date, type, dimensions, durée, auteur, origine, signature, taille (toujours au bout)
    let xd = x0 + nom + date + typ;
    [
        (Colonne::Nom, x0, nom),
        (Colonne::Date, x0 + nom, date),
        (Colonne::Type, x0 + nom + date, typ),
        (Colonne::Taille, xd + dims + duree + auteur + origine + signe, taille),
        (Colonne::Dimensions, xd, dims),
        (Colonne::Duree, xd + dims, duree),
        (Colonne::Auteur, xd + dims + duree, auteur),
        (Colonne::Origine, xd + dims + duree + auteur, origine),
        (Colonne::Signature, xd + dims + duree + auteur + origine, signe),
    ]
}

/// Largeur d'un panneau (en coordonnées du panneau, volet compris) : la liste entière, ou sa moitié en double panneau
pub fn largeur_panneau(ll: f32, double: bool) -> f32 {
    if double {
        largeur_volet() + ((ll - largeur_volet()) / 2.0).floor()
    } else {
        ll
    }
}

/// Colonnes Miller pour un panneau de largeur `l` : parent, dossier ouvert, contenu de l'élément choisi (x, largeur)
pub fn colonnes_miller(l: f32) -> [(f32, f32); 3] {
    let x0 = largeur_volet() + MARGE_LISTE - 6.0;
    let total = (l - MARGE_LISTE_DROITE - x0).max(300.0);
    let a = (total * 0.28).floor();
    let b = ((total - a) / 2.0).floor();
    [(x0, a - 8.0), (x0 + a, b - 8.0), (x0 + a + b, total - a - b)]
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    #[test]
    fn anneau_de_la_premiere_ligne_reste_dans_le_clip_de_la_liste() {
        // Capsule d'une ligne de liste à defilement = 0 : fond à y + 1, hauteur - 2 ; l'anneau ne doit pas passer sous LISTE_Y
        for h in [30.0f32, 40.0] {
            let capsule = [100.0, LISTE_Y + 1.0, 600.0, h - 2.0];
            let t = trait_anneau_dans(capsule);
            let (haut, bas) = (t[1] - ANNEAU_EPAISSEUR / 2.0, t[1] + t[3] + ANNEAU_EPAISSEUR / 2.0);
            assert!(haut >= LISTE_Y, "le haut de l'anneau ({haut}) est sous le clip ({LISTE_Y})");
            assert!(bas <= LISTE_Y + h, "le bas de l'anneau sort de la ligne");
        }
        // Vignette de la grille en première rangée : fond à y = LISTE_Y
        let t = trait_anneau_dans([40.0, LISTE_Y, 120.0, 150.0]);
        assert!(t[1] - ANNEAU_EPAISSEUR / 2.0 >= LISTE_Y);
        // Dernière ligne collée au bas du clip
        let bas_clip = LISTE_Y + 300.0;
        let t = trait_anneau_dans([100.0, bas_clip - 29.0, 600.0, 28.0]);
        assert!(t[1] + t[3] + ANNEAU_EPAISSEUR / 2.0 <= bas_clip);
    }

    #[test]
    fn marge_basse_donne_trois_lignes_de_fond_au_bout_du_defilement() {
        let h = hauteur_ligne();
        // 100 lignes dans 400 DIP : le défilement va jusqu'à ce que la marge de 3 lignes soit entièrement visible
        let max = defilement_maximal(100, h, 400.0);
        assert_eq!(max, 100.0 * h + 3.0 * h - 400.0);
        // Au bout, 3 lignes de vide sous la dernière
        assert_eq!(max + 400.0 - 100.0 * h, 3.0 * h);
        // Peu de lignes : le contenu et sa marge tiennent, pas de défilement
        assert_eq!(defilement_maximal(2, h, 400.0), 0.0);
        // Un dossier plein pile sur la hauteur défile quand même de la marge, pour qu'on puisse cliquer le fond
        let rangees = (400.0 / h) as usize;
        assert!(defilement_maximal(rangees, h, rangees as f32 * h) >= 3.0 * h - 0.01);
        assert!(hauteur_defilable(10, h) > 10.0 * h);
    }

    #[test]
    fn colonnes_laissent_la_marge_droite() {
        let l = 1200.0;
        let fin = colonnes(l).iter().filter(|c| c.2 > 0.0).map(|c| c.1 + c.2).fold(0.0f32, f32::max);
        assert!(fin <= l - MARGE_LISTE_DROITE + 0.01, "les colonnes ({fin}) mordent sur la marge droite");
        let m = colonnes_miller(l);
        assert!(m[2].0 + m[2].1 <= l - MARGE_LISTE_DROITE + 0.01);
    }

    #[test]
    fn contraste_renforce() {
        assert_eq!(contraste_avec(&BLANC_DOUX, false).a, 1.0);
        // 52 % (indisponible) devient 85 % en contraste renforcé : encore distinct du texte courant
        assert_eq!(contraste_avec(&BLANC_FAIBLE, false).a, 0.85);
        assert_eq!(contraste_avec(&POINT_TRAME, false).a, 0.2);
        assert_eq!(contraste_avec(&VOILE, false).a, 1.0);
        // L'accent et le blanc plein ne bougent pas
        assert_eq!(contraste_avec(&ACCENT_SELECTION, false).a, ACCENT_SELECTION.a);
        assert_eq!(contraste_avec(&BLANC, false).a, 1.0);
        // Le rouge de groupe garde sa teinte même en contraste renforcé
        assert_eq!(contraste_avec(&GROUPE_ROUGE, false).a, 1.0);
    }

    /// Luminance relative WCAG 
    fn luminance(c: &D2D1_COLOR_F) -> f32 {
        let lin = |v: f32| if v <= 0.03928 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) };
        0.2126 * lin(c.r) + 0.7152 * lin(c.g) + 0.0722 * lin(c.b)
    }

    /// Rapport de contraste WCAG de `texte` (composé sur `fond` si translucide) sur `fond`
    fn rapport(texte: &D2D1_COLOR_F, fond: &D2D1_COLOR_F) -> f32 {
        let mix = |t: f32, f: f32| t * texte.a + f * (1.0 - texte.a);
        let compose = D2D1_COLOR_F { r: mix(texte.r, fond.r), g: mix(texte.g, fond.g), b: mix(texte.b, fond.b), a: 1.0 };
        let (a, b) = (luminance(&compose), luminance(fond));
        (a.max(b) + 0.05) / (a.min(b) + 0.05)
    }

    /// Texte secondaire et fond tels qu'ils sont sous le halo du reflet à son maximum (centre, alpha plein), après `contraste()` :
    /// le halo est posé APRÈS le texte, donc il couvre le texte comme son fond. Pire cas du lot C3. Deux couleurs opaques.
    pub(crate) fn couleurs_sous_halo(clair: bool, renforce: bool) -> (D2D1_COLOR_F, D2D1_COLOR_F) {
        let vu = |c: &D2D1_COLOR_F| transformer(c, clair, renforce, false);
        let sur = |dessus: &D2D1_COLOR_F, dessous: &D2D1_COLOR_F| {
            let mix = |d: f32, b: f32| d * dessus.a + b * (1.0 - dessus.a);
            D2D1_COLOR_F { r: mix(dessus.r, dessous.r), g: mix(dessus.g, dessous.g), b: mix(dessus.b, dessous.b), a: 1.0 }
        };
        let fond = D2D1_COLOR_F { a: 1.0, ..vu(&VOILE) };
        let halo = transformer(&REFLET_FOND, clair, false, false);
        (sur(&halo, &sur(&vu(&BLANC_DOUX), &fond)), sur(&halo, &fond))
    }

    pub(crate) fn rapport_sous_halo(clair: bool, renforce: bool) -> f32 {
        let (texte, fond) = couleurs_sous_halo(clair, renforce);
        rapport(&texte, &fond)
    }

    #[test]
    fn contrastes_wcag_des_jetons() {
        let noir = rgba(0, 0, 0, 1.0);
        let voile = D2D1_COLOR_F { a: 1.0, ..VOILE };
        for (nom, jeton) in [("ACCENT", &ACCENT), ("ERREUR", &ERREUR), ("BLANC_FAIBLE", &BLANC_FAIBLE)] {
            for (fond_nom, fond) in [("noir", &noir), ("voile", &voile)] {
                let r = rapport(jeton, fond);
                assert!(r >= 4.5, "{nom} sur {fond_nom} : {r:.2}:1 < 4,5:1");
            }
        }
        let r = rapport(&ENCRE_SUR_ACCENT, &ACCENT);
        assert!(r >= 4.5, "ENCRE_SUR_ACCENT sur ACCENT : {r:.2}:1 < 4,5:1");
    }

    #[test]
    fn jetons_distincts() {
        let egal = |a: &D2D1_COLOR_F, b: &D2D1_COLOR_F| a.r == b.r && a.g == b.g && a.b == b.b;
        assert!(!egal(&ACCENT, &ERREUR));
        assert!(!egal(&ACCENT, &GROUPE_ROUGE));
        assert!(!egal(&ERREUR, &GROUPE_ROUGE));
    }

    fn meme(a: &D2D1_COLOR_F, b: &D2D1_COLOR_F) -> bool {
        a.r == b.r && a.g == b.g && a.b == b.b && a.a == b.a
    }

    const ENCRE: D2D1_COLOR_F = rgba(16, 16, 20, 1.0);

    /// Chaque jeton nommé, vu en clair : la valeur de la maquette (`.win[data-t=light]`)
    #[test]
    fn jetons_en_theme_clair() {
        let encre = |a: f32| D2D1_COLOR_F { a, ..ENCRE };
        let attendus: [(&str, D2D1_COLOR_F, D2D1_COLOR_F); 18] = [
            ("BLANC", BLANC, encre(0.95)),
            ("BLANC_DOUX", BLANC_DOUX, encre(0.68)),
            ("BLANC_FAIBLE", BLANC_FAIBLE, encre(0.60)),
            ("BLANC_TRES_FAIBLE", BLANC_TRES_FAIBLE, encre(0.16)),
            ("SURVOL", SURVOL, encre(0.09)),
            ("SURVOL_FORT", SURVOL_FORT, encre(0.09)),
            ("POINT_TRAME", POINT_TRAME, encre(0.05)),
            ("LIGNE", LIGNE, encre(0.08)),
            ("LISERE_HAUT", LISERE_HAUT, encre(0.40)),
            ("VOILE", VOILE, rgba(247, 247, 248, 1.0)),
            ("CHAMP_FOND", CHAMP_FOND, rgba(255, 255, 255, 0.96)),
            ("ACCENT", ACCENT, rgba(0x8A, 0x52, 0x17, 1.0)),
            ("ACCENT_SELECTION", ACCENT_SELECTION, rgba(0x8A, 0x52, 0x17, 0.16)),
            ("ACCENT_VISE", ACCENT_VISE, rgba(0x8A, 0x52, 0x17, 0.55)),
            ("ERREUR", ERREUR, rgba(0xB3, 0x40, 0x2B, 1.0)),
            ("ENCRE_SUR_ACCENT", ENCRE_SUR_ACCENT, rgba(255, 255, 255, 1.0)),
            ("PIXEL_JOUR", PIXEL_JOUR, rgba(0x0B, 0x7A, 0x99, 1.0)),
            ("ANNEAU", ANNEAU, encre(0.9)),
        ];
        for (nom, jeton, clair) in attendus {
            let r = transformer(&jeton, true, false, false);
            assert!(meme(&r, &clair), "{nom} : obtenu ({}, {}, {}, {}), attendu ({}, {}, {}, {})", r.r, r.g, r.b, r.a, clair.r, clair.g, clair.b, clair.a);
        }
    }

    /// Responsive 1100 px : volet de 240 px et aperçu masqué, sans toucher au réglage `apercu`.
    #[test]
    fn responsive_du_volet_et_de_l_apercu() {
        assert_eq!(largeur_volet_pour(1440.0, false), LARGEUR_VOLET);
        assert_eq!(largeur_volet_pour(1100.0, false), LARGEUR_VOLET_ETROIT);
        assert_eq!(largeur_volet_pour(1099.0, false), LARGEUR_VOLET_ETROIT);
        assert_eq!(largeur_volet_pour(1440.0, true), 0.0, "Essentiel replie le volet");
        assert!(apercu_visible_pour(1440.0, true));
        assert!(!apercu_visible_pour(1100.0, true), "à 1100 px, l'aperçu est masqué");
        assert!(!apercu_visible_pour(1440.0, false), "le réglage reste prioritaire");
        assert_eq!(largeur_liste(1100.0, true), 1100.0);
        assert_eq!(largeur_liste(1440.0, true), 1152.0);
        assert_eq!(largeur_liste(1440.0, false), 1440.0);
    }

    /// Focus clavier : l'anneau suit la maquette `--ring` (blanc 90 % / encre 90 %).
    #[test]
    fn anneau_de_focus() {
        assert!(meme(&ANNEAU, &rgba(255, 255, 255, 0.9)));
        assert!(meme(&ANNEAU_CLAIR, &rgba(16, 16, 20, 0.9)));
        assert!(meme(&transformer(&ANNEAU, true, false, false), &ANNEAU_CLAIR));
    }

    /// Les deux jetons clairs dont la valeur sombre est ambiguë ont bien leur valeur de maquette.
    #[test]
    fn surfaces_et_selection_claires() {
        assert!(meme(&SELECTION_CLAIR, &rgba(16, 16, 20, 0.06)));
        assert!(meme(&CARTE_CLAIR, &rgba(255, 255, 255, 1.0)));
        assert!(meme(&transformer(&LIGNE, true, false, false), &rgba(16, 16, 20, 0.08)));
    }

    /// Hors paliers : l'alpha d'entrée est gardé (fondus, apparition en points), borné à [0 ; 1]
    #[test]
    fn alpha_hors_paliers_garde_son_alpha() {
        let r = transformer(&D2D1_COLOR_F { a: 0.32, ..BLANC }, true, false, false);
        assert!(meme(&r, &D2D1_COLOR_F { a: 0.32, ..ENCRE }));
        let r = transformer(&D2D1_COLOR_F { a: 0.3, ..ACCENT }, true, false, false);
        assert!(meme(&r, &D2D1_COLOR_F { a: 0.3, ..ACCENT_CLAIR }), "la teinte d'accent change, l'alpha reste");
        assert_eq!(alpha_encre(1.7), 1.0);
        assert_eq!(alpha_encre(-0.2), 0.0);
    }

    /// En sombre, sans contraste : la sortie est l'entrée, au bit près
    #[test]
    fn sombre_identique_a_l_entree() {
        let tous = [BLANC, BLANC_DOUX, BLANC_FAIBLE, BLANC_TRES_FAIBLE, SURVOL, SURVOL_FORT, POINT_TRAME, LIGNE, VOILE, CHAMP_FOND, ACCENT, ACCENT_SELECTION, ACCENT_VISE, ERREUR, ENCRE_SUR_ACCENT, PIXEL_JOUR, GROUPE_ROUGE, rgba(16, 16, 16, 0.98), rgba(0, 0, 0, 0.55), rgba(232, 178, 90, 1.0), D2D1_COLOR_F { a: 0.37, ..BLANC }];
        for c in tous {
            let r = transformer(&c, false, false, false);
            assert!(c.r.to_bits() == r.r.to_bits() && c.g.to_bits() == r.g.to_bits() && c.b.to_bits() == r.b.to_bits() && c.a.to_bits() == r.a.to_bits(), "{c:?} change en sombre");
        }
    }

    /// Ce que le thème clair ne touche pas : couleurs de groupes et de balises, ombres et voiles de fond translucides
    #[test]
    fn clair_laisse_groupes_et_ombres() {
        let intactes = [GROUPE_ROUGE, rgba(140, 140, 140, 1.0), rgba(0xE8, 0xA3, 0x17, 1.0), rgba(0x46, 0xB8, 0x7C, 1.0), rgba(0x4C, 0x8D, 0xF6, 1.0), rgba(0, 0, 0, 0.35), rgba(0, 0, 0, 0.55), rgba(0, 0, 0, 0.18)];
        for c in intactes {
            assert!(meme(&transformer(&c, true, false, false), &c), "{c:?} doit rester inchangée en clair");
        }
        // Cartes de menu et bandeaux presque noirs et opaques : surface claire, alpha gardé
        assert!(meme(&transformer(&rgba(16, 16, 16, 0.98), true, false, false), &rgba(247, 247, 248, 0.98)));
        assert!(meme(&transformer(&rgba(0, 0, 0, 0.97), true, false, false), &rgba(247, 247, 248, 0.97)));
    }

    /// Le thème Contraste élevé de Windows passe avant le thème clair : mêmes couleurs que sans lui
    #[test]
    fn contraste_eleve_passe_avant_le_clair() {
        for c in [BLANC, BLANC_DOUX, VOILE, ACCENT, ERREUR, GROUPE_ROUGE] {
            assert!(meme(&transformer(&c, true, true, true), &transformer(&c, false, true, true)));
            assert!(meme(&transformer(&c, true, false, true), &contraste_avec(&c, true)));
        }
    }

    /// Clair + contraste renforcé : une couleur que le thème clair ne transforme pas (voile modal, ombre, groupe) garde son alpha
    #[test]
    fn renforce_clair_ne_touche_pas_aux_voiles_modaux() {
        for a in [0.35, 0.55, 0.6] {
            let c = rgba(0, 0, 0, a);
            assert!(meme(&transformer(&c, true, true, false), &c), "voile noir {a} rendu opaque");
        }
        assert!(meme(&transformer(&GROUPE_ROUGE, true, true, false), &GROUPE_ROUGE));
        // Les couleurs transformées gardent le renforcement
        assert_eq!(transformer(&VOILE, true, true, false).a, 1.0);
    }

    /// Le jeton du disque de case : valeur sombre inchangée, valeur claire = surface claire (par `contraste_ou`, état global lu
    /// ici seulement par ses deux entrées, donc on teste les constantes et la règle)
    #[test]
    fn disque_de_case_a_un_jeton_clair() {
        assert!(meme(&DISQUE_CASE, &rgba(0, 0, 0, 0.55)));
        assert!(meme(&DISQUE_CASE_CLAIR, &rgba(247, 247, 248, 0.85)));
        // Sans le jeton, la transformation par teinte le laisserait noir : c'est la raison d'être de `contraste_ou`
        assert!(meme(&transformer(&DISQUE_CASE, true, false, false), &DISQUE_CASE));
        assert!(meme(&transformer_ou(&DISQUE_CASE, &DISQUE_CASE_CLAIR, true, false, false), &DISQUE_CASE_CLAIR));
        assert!(meme(&transformer_ou(&DISQUE_CASE, &DISQUE_CASE_CLAIR, true, true, false), &DISQUE_CASE_CLAIR));
        assert!(meme(&transformer_ou(&DISQUE_CASE, &DISQUE_CASE_CLAIR, false, false, false), &DISQUE_CASE), "sombre : inchangé");
        assert!(meme(&transformer_ou(&DISQUE_CASE, &DISQUE_CASE_CLAIR, true, true, true), &contraste_avec(&DISQUE_CASE, true)), "Contraste élevé passe avant");
    }

    /// Contraste renforcé en clair : encre franche, voile opaque ; sombre renforcé inchangé
    #[test]
    fn contraste_renforce_en_clair() {
        let r = transformer(&BLANC_FAIBLE, true, true, false);
        assert!(meme(&r, &D2D1_COLOR_F { a: 0.85, ..ENCRE }));
        assert_eq!(transformer(&VOILE, true, true, false).a, 1.0);
        assert_eq!(transformer(&POINT_TRAME, true, true, false).a, 0.2);
        assert!(meme(&transformer(&BLANC_DOUX, false, true, false), &contraste_avec(&BLANC_DOUX, false)));
    }

    /// WCAG sur le fond clair #F7F7F8 : encres, accent, erreur, et blanc sur accent clair
    #[test]
    fn contrastes_wcag_du_theme_clair() {
        let fond = rgba(247, 247, 248, 1.0);
        for (nom, jeton) in [("encre principale", BLANC), ("encre secondaire", BLANC_DOUX), ("encre tertiaire", BLANC_FAIBLE), ("ACCENT clair", ACCENT), ("ERREUR claire", ERREUR)] {
            let c = transformer(&jeton, true, false, false);
            let r = rapport(&c, &fond);
            assert!(r >= 4.5, "{nom} sur #F7F7F8 : {r:.2}:1 < 4,5:1");
        }
        let accent = transformer(&ACCENT, true, false, false);
        let blanc_sur_accent = transformer(&ENCRE_SUR_ACCENT, true, false, false);
        let r = rapport(&blanc_sur_accent, &accent);
        assert!(r >= 4.5, "blanc sur ACCENT clair : {r:.2}:1 < 4,5:1");
    }

    /// La teinte « Nombre » de l'aperçu de code : inchangée en sombre, ambre foncé de l'accent clair en clair (illisible sinon)
    #[test]
    fn teinte_nombre_lisible_en_clair() {
        assert!(meme(&TEINTE_NOMBRE, &rgba(232, 178, 90, 1.0)));
        assert!(meme(&transformer(&TEINTE_NOMBRE, false, false, false), &TEINTE_NOMBRE), "sombre : sortie inchangée");
        let clair = transformer(&TEINTE_NOMBRE, true, false, false);
        assert!(meme(&clair, &rgba(0x8A, 0x52, 0x17, 1.0)), "clair : #8A5217");
        let r = rapport(&clair, &rgba(247, 247, 248, 1.0));
        assert!(r >= 4.5, "teinte Nombre en clair : {r:.2}:1 < 4,5:1");
        // Ce que le brief reprochait à la valeur sombre posée sur le fond clair
        assert!(rapport(&TEINTE_NOMBRE, &rgba(247, 247, 248, 1.0)) < 2.0);
    }

    /// Audit des contrastes pour `tools/design/audit-contraste.ps1` (lancé avec `--ignored --nocapture`). Une ligne par paire :
    /// `PAIRE;THEME;RAPPORT;SEUIL;ETAT`. Seuil 4,5 : texte, ETAT `ECHEC` s'il est sous le seuil (le script sort alors en 1).
    /// Seuil 3,0 : couleurs de groupes et de balises, simple information (`AVERT`, jamais bloquant : ce sont des couleurs
    /// choisies par l'utilisateur). Les valeurs viennent de la transformation de thème, pas de copies.
    #[test]
    #[ignore = "audit : lancé par tools/design/audit-contraste.ps1"]
    fn tableau_contrastes() {
        let mut lignes: Vec<String> = Vec::new();
        for (theme, clair) in [("SOMBRE", false), ("CLAIR", true)] {
            let vu = |c: &D2D1_COLOR_F| transformer(c, clair, false, false);
            let fond = D2D1_COLOR_F { a: 1.0, ..vu(&VOILE) };
            let accent = vu(&ACCENT);
            let mut ajouter = |paire: &str, texte: D2D1_COLOR_F, sur: &D2D1_COLOR_F, seuil: f32, bloquant: bool| {
                let r = rapport(&texte, sur);
                let etat = if r >= seuil { "OK" } else if bloquant { "ECHEC" } else { "AVERT" };
                lignes.push(format!("{paire};{theme};{r:.2};{seuil:.1};{etat}"));
            };
            ajouter("ENCRE PRINCIPALE SUR FOND", vu(&BLANC), &fond, 4.5, true);
            ajouter("ENCRE SECONDAIRE SUR FOND", vu(&BLANC_DOUX), &fond, 4.5, true);
            ajouter("ENCRE TERTIAIRE SUR FOND", vu(&BLANC_FAIBLE), &fond, 4.5, true);
            ajouter("ACCENT SUR FOND", accent, &fond, 4.5, true);
            ajouter("ERREUR SUR FOND", vu(&ERREUR), &fond, 4.5, true);
            ajouter("ENCRE_SUR_ACCENT SUR ACCENT", vu(&ENCRE_SUR_ACCENT), &accent, 4.5, true);
            ajouter("TEINTE NOMBRE SUR FOND", vu(&TEINTE_NOMBRE), &fond, 4.5, true);
            // Reflet du lot C3 : texte secondaire sous le halo à son maximum (le halo couvre le texte comme son fond)
            let (texte_halo, fond_halo) = couleurs_sous_halo(clair, false);
            ajouter("ENCRE SECONDAIRE SOUS HALO MAXIMAL", texte_halo, &fond_halo, 4.5, true);
            // Pixel : un dessin, pas du texte (seuil 3:1 des éléments graphiques, informatif)
            ajouter("PIXEL JOUR SUR FOND", vu(&PIXEL_JOUR), &fond, 3.0, false);
            ajouter("PIXEL SOIR SUR FOND", accent, &fond, 3.0, false);
            for (c, nom) in crate::groupes::COULEURS {
                ajouter(&format!("GROUPE {nom} SUR FOND"), vu(&crate::rendu::Rendu::couleur_groupe(c)), &fond, 3.0, false);
            }
            use crate::outils::Balise;
            for (b, nom) in [(Balise::Rouge, "ROUGE"), (Balise::Blanc, "BLANCHE"), (Balise::Gris, "GRISE")] {
                ajouter(&format!("BALISE {nom} SUR FOND"), vu(&crate::rendu::Rendu::couleur_balise(b)), &fond, 3.0, false);
            }
        }
        println!("PAIRE;THEME;RAPPORT;SEUIL;ETAT");
        for l in &lignes {
            println!("{l}");
        }
        assert!(!lignes.iter().any(|l| l.ends_with(";ECHEC")), "une paire de texte est sous 4,5:1");
    }

    /// Précédence : réglage local explicite > avis partagé > Windows > sombre
    #[test]
    fn precedence_du_theme() {
        use crate::eco::partage::Theme::{Dark, Light, System};
        for windows in [false, true] {
            // Réglage local explicite : bat l'avis partagé et Windows
            for avis in [None, Some(Dark), Some(Light), Some(System)] {
                assert!(!theme_effectif(ChoixTheme::Sombre, avis, windows));
                assert!(theme_effectif(ChoixTheme::Clair, avis, windows));
                assert_eq!(theme_effectif(ChoixTheme::Systeme, avis, windows), windows, "Systeme suit Windows, pas l'avis");
            }
            // Auto : l'avis partagé, `system` renvoie à Windows, rien : sombre
            assert!(theme_effectif(ChoixTheme::Auto, Some(Light), windows));
            assert!(!theme_effectif(ChoixTheme::Auto, Some(Dark), windows));
            assert_eq!(theme_effectif(ChoixTheme::Auto, Some(System), windows), windows);
            assert!(!theme_effectif(ChoixTheme::Auto, None, windows), "aucun avis : sombre, même si Windows est clair");
        }
    }

    #[test]
    fn choix_de_theme_depuis_le_fichier() {
        for c in ChoixTheme::TOUS {
            assert_eq!(ChoixTheme::depuis_cle(c.cle()), c);
        }
        assert_eq!(ChoixTheme::depuis_cle(" Clair "), ChoixTheme::Clair);
        assert_eq!(ChoixTheme::depuis_cle("violet"), ChoixTheme::Auto);
        assert_eq!(ChoixTheme::depuis_cle(""), ChoixTheme::Auto);
        assert_eq!(ChoixTheme::default(), ChoixTheme::Auto);
    }

    #[test]
    fn ressort_de_la_maquette() {
        assert_eq!(ressort(0.0), 0.0);
        assert_eq!(ressort(1.0), 1.0);
        assert!(ressort(0.5) > 1.0, "le ressort dépasse avant de se poser");
        assert!(ressort(0.1) < 1.0);
        assert_eq!(ressort(2.0), 1.0);
        assert_eq!(ressort(-1.0), 0.0);
    }

    #[test]
    fn courbe_css_ease() {
        assert_eq!((ease(0.0), ease(1.0), ease(-1.0), ease(3.0)), (0.0, 1.0, 0.0, 1.0));
        assert_eq!((ease_in(0.0), ease_in(1.0)), (0.0, 1.0));
        assert!(ease_in(0.25) < 0.2 && ease_in(0.75) > 0.55, "ease_in part lentement : {} {}", ease_in(0.25), ease_in(0.75));
        // `ease` monte vite au début, sans dépasser 1
        assert!(ease(0.25) > 0.35 && ease(0.5) < 1.0, "ease : {} {}", ease(0.25), ease(0.5));
        let mut avant = 0.0;
        for k in 0..=100 {
            let v = ease(k as f32 / 100.0);
            assert!(v >= avant - 1e-6, "ease monotone");
            avant = v;
        }
        // Le ressort est bien un cas de `bezier`
        assert_eq!(ressort(0.37), bezier(0.37, 0.2, 1.25, 0.3, 1.0));
    }

    #[test]
    fn accent_et_erreur_opaques_en_contraste_eleve() {
        // `contraste_avec` prend le drapeau en paramètre : aucun état global n'est touché
        assert_eq!(contraste_avec(&ACCENT, true).a, 1.0);
        assert_eq!(contraste_avec(&ERREUR, true).a, 1.0);
    }
}
