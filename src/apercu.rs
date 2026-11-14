// Volet d'aperçu : miniature Windows (images, vidéos, PDF…), début des fichiers texte,
// informations (taille, dates, artiste, durée…). Tout est lu dans un fil séparé.

use windows::core::PCWSTR;
use windows::Win32::Foundation::{HWND, LPARAM, SIZE, WPARAM};
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::Storage::EnhancedStorage::*;
use windows::Win32::System::Com::*;
use windows::Win32::UI::Shell::PropertiesSystem::PROPERTYKEY;
use windows::Win32::UI::Shell::*;
use windows::Win32::UI::WindowsAndMessaging::{PostMessageW, WM_APP};

use crate::dossier::{self, Entree, Genre};
use crate::icones::{self, Icone};
use crate::pages;
use crate::systeme;

/// Aperçu prêt (LPARAM = Box<Apercu>)
pub const WM_APERCU: u32 = WM_APP + 5;
/// Vignette de la grille prête (LPARAM = Box<(chemin, Option<Image>)>)
pub const WM_VIGNETTE: u32 = WM_APP + 10;
/// Page du coup d'œil prête (LPARAM = Box<PageGalerie>)
pub const WM_GALERIE: u32 = WM_APP + 11;

/// Coup d'œil (Espace) : l'élément choisi en grand, ← → pour passer au suivant (galerie photo), ↑ ↓ pour tourner les pages
/// d'un PDF, d'un document, d'un long texte, d'une archive ou d'un TIFF
pub struct Galerie {
    pub numero: u64,
    pub nom: String,
    pub detail: String,
    pub icone: Icone,
    pub image: Option<Image>,
    pub charge: bool,
    /// Chemin et extension de l'élément (pour redemander une autre page) ; rien s'il n'a pas d'aperçu
    pub chemin: Option<String>,
    pub ext: String,
    /// Page affichée (à partir de 0) et nombre de pages (0 : pas encore connu)
    pub page: u32,
    pub pages: u32,
    /// Ce que la page montre quand ce n'est pas le fichier lui-même (« TEXTE EXTRAIT »)
    pub nature: &'static str,
    /// Augmente à chaque nouvelle image : clé du cache Direct2D
    pub version: u32,
    /// Zoom demandé (1 : l'élément tient dans la carte) et décalage du centre de l'image par rapport au centre de la zone (DIP)
    pub zoom: f32,
    pub decalage: (f32, f32),
    /// Échelle de rendu de l'image reçue et de celle demandée au fil (1 à `ECHELLE_MAX`) : texte, PDF et images sont redessinés
    /// plus grands quand on zoome, pour rester nets
    pub echelle: u32,
    pub echelle_demandee: u32,
    /// Montré par la visionneuse web (HTML, SVG…) : l'image reste vide, un contrôle WebView2 couvre la zone (`webvue.rs`)
    pub web: bool,
    /// Montré par le gestionnaire d'aperçu de Windows (Office…) : image vide, fenêtre du gestionnaire par-dessus (`hote_apercu.rs`)
    pub systeme: bool,
}

/// Zoom minimal et maximal du coup d'œil, et plus grande échelle de rendu (au-delà, l'image est seulement agrandie)
pub const ZOOM_MIN: f32 = 1.0;
pub const ZOOM_MAX: f32 = 8.0;
pub const ECHELLE_MAX: u32 = 3;

impl Galerie {
    /// État d'un coup d'œil qui s'ouvre : ajusté à la carte, au centre, échelle 1
    pub fn vierge(numero: u64, nom: String, detail: String, icone: Icone, chemin: Option<String>, ext: String) -> Self {
        Self {
            numero,
            nom,
            detail,
            icone,
            image: None,
            charge: chemin.is_none(),
            chemin,
            ext,
            page: 0,
            pages: 0,
            nature: "",
            version: 0,
            zoom: 1.0,
            decalage: (0.0, 0.0),
            echelle: 1,
            echelle_demandee: 1,
            web: false,
            systeme: false,
        }
    }

    /// Taille de l'image à l'écran (DIP) : l'image tient dans `zone` (largeur, hauteur) à zoom 1, puis `zoom` la grossit. Les pixels
    /// de l'image comptent pour `1 / echelle` : une image rendue à l'échelle 2 mesure le double sans paraître plus grande.
    pub fn taille_affichee(&self, img: (u32, u32), zone: (f32, f32)) -> (f32, f32) {
        let e = self.echelle.max(1) as f32;
        let (lw, lh) = (img.0 as f32 / e, img.1 as f32 / e);
        let k = (zone.0 / lw).min(zone.1 / lh).min(3.0);
        (lw * k * self.zoom, lh * k * self.zoom)
    }

    /// Ramène le décalage dans ce qui garde l'image visible : centrée si elle tient dans la zone, bords sur les bords sinon
    pub fn borner(&mut self, img: (u32, u32), zone: (f32, f32)) {
        let (w, h) = self.taille_affichee(img, zone);
        let (mx, my) = (((w - zone.0) / 2.0).max(0.0), ((h - zone.1) / 2.0).max(0.0));
        self.decalage = (self.decalage.0.clamp(-mx, mx), self.decalage.1.clamp(-my, my));
    }

    /// Multiplie le zoom par `facteur` en gardant fixe le point `curseur` (DIP, relatif au centre de la zone)
    pub fn zoomer(&mut self, facteur: f32, curseur: (f32, f32), img: (u32, u32), zone: (f32, f32)) {
        let nouveau = (self.zoom * facteur).clamp(ZOOM_MIN, ZOOM_MAX);
        let rapport = nouveau / self.zoom;
        self.decalage = (curseur.0 - (curseur.0 - self.decalage.0) * rapport, curseur.1 - (curseur.1 - self.decalage.1) * rapport);
        self.zoom = nouveau;
        self.borner(img, zone);
    }

    /// Rectangle de l'image (x, y, largeur, hauteur) relatif au coin de la zone, décalage borné ; il sert au dessin et au clic
    pub fn rect_image(&self, img: (u32, u32), zone: (f32, f32)) -> [f32; 4] {
        let (w, h) = self.taille_affichee(img, zone);
        let (mx, my) = (((w - zone.0) / 2.0).max(0.0), ((h - zone.1) / 2.0).max(0.0));
        let (dx, dy) = (self.decalage.0.clamp(-mx, mx), self.decalage.1.clamp(-my, my));
        [(zone.0 - w) / 2.0 + dx, (zone.1 - h) / 2.0 + dy, w, h]
    }

    /// Échelle de rendu qui suffit au zoom actuel
    pub fn echelle_voulue(&self) -> u32 {
        (self.zoom.ceil() as u32).clamp(1, ECHELLE_MAX)
    }
}

/// Ce que le fil du coup d'œil renvoie à la fenêtre
pub struct PageGalerie {
    pub numero: u64,
    pub page: u32,
    pub pages: u32,
    pub nature: &'static str,
    pub image: Option<Image>,
    pub echelle: u32,
}

/// Demande en attente pour le fil du coup d'œil : seule la plus récente compte (R19). (fenêtre, numéro, chemin, extension, page, échelle)
static GALERIE: (std::sync::Mutex<Option<(isize, u64, String, String, u32, u32)>>, std::sync::Condvar) = (std::sync::Mutex::new(None), std::sync::Condvar::new());

/// Pages de texte d'un fichier (document, archive, texte), gardées pour tourner les pages sans tout relire : (clé, pages, nature)
type CachePages = Option<(String, std::sync::Arc<Vec<String>>, &'static str)>;

/// Prépare la grande image du coup d'œil dans un fil séparé. Un seul fil, démarré au premier appel :
/// en parcourant les images aux flèches, les demandes dépassées sont remplacées au lieu d'être décodées.
pub fn charger_galerie(hwnd: HWND, numero: u64, chemin: String, ext: String, page: u32, echelle: u32) {
    static FIL: std::sync::Once = std::sync::Once::new();
    *GALERIE.0.lock().unwrap_or_else(|e| e.into_inner()) = Some((hwnd.0 as isize, numero, chemin, ext, page, echelle.clamp(1, ECHELLE_MAX)));
    GALERIE.1.notify_one();
    FIL.call_once(|| {
        std::thread::spawn(|| unsafe {
            let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE);
            let mut cache: CachePages = None;
            loop {
                let demande = {
                    let mut g = GALERIE.0.lock().unwrap_or_else(|e| e.into_inner());
                    while g.is_none() {
                        g = GALERIE.1.wait(g).unwrap_or_else(|e| e.into_inner());
                    }
                    g.take()
                };
                let Some((fenetre, numero, chemin, ext, page, echelle)) = demande else { continue };
                let s = image_galerie(&chemin, &ext, page, echelle, &mut cache);
                let p = Box::into_raw(Box::new(PageGalerie { numero, page: s.page, pages: s.pages, nature: s.nature, image: s.image, echelle }));
                if PostMessageW(HWND(fenetre as *mut _), WM_GALERIE, WPARAM(0), LPARAM(p as isize)).is_err() {
                    drop(Box::from_raw(p));
                }
            }
        });
    });
}

/// Une page rendue : l'image, la page réellement montrée (demandée, ramenée dans les bornes) et le nombre de pages
struct SortieGalerie {
    image: Option<Image>,
    page: u32,
    pages: u32,
    nature: &'static str,
}

impl SortieGalerie {
    fn une(image: Option<Image>) -> Self {
        Self { image, page: 0, pages: 1, nature: "" }
    }
}

/// Grande image du coup d'œil, par la même cascade que le volet d'aperçu : le shell ne fournit aucune miniature
/// pour un PDF ou une police (HRESULT 0x8004B200), là c'est le lecteur PDF de Windows ou GDI qui dessine.
/// Ordre : PDF (toutes ses pages) → police → texte, code, archive, document (pages de texte) → miniature du shell de plus en plus
/// petite (une taille refusée n'interdit pas les autres) → images à plusieurs trames et formats sans miniature (décodeur WIC) →
/// texte d'un fichier à l'extension inconnue → octets en hexadécimal : tout fichier a un aperçu.
unsafe fn image_galerie(chemin: &str, ext: &str, page: u32, echelle: u32, cache: &mut CachePages) -> SortieGalerie {
    let ext = ext.to_uppercase();
    if ext == "PDF" {
        if let Some((img, pages)) = pdf(chemin, 1400 * echelle, page) {
            return SortieGalerie { image: Some(img), page: page.min(pages.saturating_sub(1)), pages: pages.max(1), nature: "" };
        }
    }
    if matches!(ext.as_str(), "TTF" | "OTF" | "TTC") {
        let mut a = Apercu { numero: 0, nom: String::new(), icone: Icone::Dossier, image: None, texte: None, numeroter: false, infos: Vec::new(), teintes: Vec::new() };
        police(&mut a, chemin);
        if a.image.is_some() {
            return SortieGalerie::une(a.image.map(encrer));
        }
    }
    // Texte, code, archives et documents : le shell n'a pas de miniature, on dessine le texte page par page
    let liste = TEXTE.contains(&ext.as_str()) || crate::archive::est_archive(&ext) || pages::est_document(&ext);
    if liste {
        if let Some(s) = page_de_texte(chemin, &ext, page, echelle, cache) {
            return s;
        }
    }
    // Zoomée : l'image est décodée plus grande que la miniature du shell (qui s'arrête à 1 600 px)
    if echelle > 1 && !liste {
        if let Some((t, n)) = pages::trame(chemin, page, false, pages::MAX_COTE * echelle) {
            return SortieGalerie { image: Some(t), page: page.min(n - 1), pages: n, nature: "" };
        }
    }
    if let Some(img) = [1600, 520, 256].into_iter().find_map(|t| miniature(chemin, t)) {
        // Un TIFF ou un GIF à plusieurs trames : la miniature du shell n'en montre qu'une
        if pages::IMAGES_MULTI.contains(&ext.as_str()) {
            if let Some((t, n)) = pages::trame(chemin, page, true, pages::MAX_COTE * echelle) {
                return SortieGalerie { image: Some(t), page: page.min(n - 1), pages: n, nature: "" };
            }
        }
        return SortieGalerie::une(Some(img));
    }
    if let Some((t, n)) = pages::trame(chemin, page, false, pages::MAX_COTE * echelle) {
        return SortieGalerie { image: Some(t), page: page.min(n - 1), pages: n, nature: "" };
    }
    if !liste {
        if let Some(s) = page_de_texte(chemin, &ext, page, echelle, cache) {
            return s;
        }
    }
    if let Some((t, n)) = pages::hexa(chemin, page) {
        if let Some(img) = texte_image(&t, echelle) {
            return SortieGalerie { image: Some(encrer(img)), page: page.min(n - 1), pages: n, nature: "OCTETS (HEXADECIMAL)" };
        }
    }
    SortieGalerie::une(None)
}

/// Une page du texte d'un fichier (les pages sont lues une fois, puis gardées pour les pages suivantes)
unsafe fn page_de_texte(chemin: &str, ext: &str, page: u32, echelle: u32, cache: &mut CachePages) -> Option<SortieGalerie> {
    let cle = format!("{chemin}|{}", modifie(chemin));
    if cache.as_ref().is_none_or(|(k, _, _)| *k != cle) {
        let (v, nature) = pages::pages_de(chemin, ext)?;
        *cache = Some((cle, std::sync::Arc::new(v), nature));
    }
    let (_, v, nature) = cache.as_ref()?;
    let n = v.len() as u32;
    let page = page.min(n.saturating_sub(1));
    let img = texte_image(v.get(page as usize)?, echelle)?;
    Some(SortieGalerie { image: Some(encrer(img)), page, pages: n, nature })
}

/// Les images de texte et de police sont blanches sur transparent (GDI) : en thème clair, la carte est claire, on les repasse en noir
/// (prémultiplié : seul l'alpha reste)
fn encrer(mut img: Image) -> Image {
    if crate::theme::clair() {
        for p in img.pixels.as_chunks_mut::<4>().0 {
            p[0] = 0;
            p[1] = 0;
            p[2] = 0;
        }
    }
    img
}

/// Début d'un fichier texte dessiné en blanc sur transparent (GDI, chasse fixe), pour la grande carte du coup d'œil.
/// Lignes coupées à 110 caractères et 26 lignes au plus : ce n'est qu'un coup d'œil, pas un éditeur.
unsafe fn texte_image(texte: &str, echelle: u32) -> Option<Image> {
    const COLONNES: usize = 110;
    const LIGNES: usize = 26;
    const PAS: i32 = 24;
    // Zoomé : tout est dessiné `e` fois plus grand (police, pas, marges) au lieu d'agrandir des pixels
    let e = echelle.clamp(1, ECHELLE_MAX) as i32;
    let (l, h) = (1100 * e, (LIGNES as i32 * PAS + 16) * e);
    let dc = CreateCompatibleDC(None);
    let info = BITMAPINFO { bmiHeader: BITMAPINFOHEADER { biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32, biWidth: l, biHeight: -h, biPlanes: 1, biBitCount: 32, biCompression: BI_RGB.0, ..Default::default() }, ..Default::default() };
    let mut bits = std::ptr::null_mut();
    let mut resultat = None;
    if let Ok(bmp) = CreateDIBSection(dc, &info, DIB_RGB_COLORS, &mut bits, None, 0) {
        let ancien = SelectObject(dc, bmp);
        // SAFETY: la section DIB est de l × h pixels de 4 octets, vivante jusqu'au DeleteObject ci-dessous
        std::ptr::write_bytes(bits as *mut u8, 0, (l * h * 4) as usize);
        SetBkMode(dc, TRANSPARENT);
        SetTextColor(dc, windows::Win32::Foundation::COLORREF(0xFFFFFF));
        let nom: Vec<u16> = "Consolas".encode_utf16().chain(std::iter::once(0)).collect();
        let f = CreateFontW(-18 * e, 0, 0, 0, 400, 0, 0, 0, DEFAULT_CHARSET.0 as u32, 0, 0, ANTIALIASED_QUALITY.0 as u32, 0, PCWSTR(nom.as_ptr()));
        let ancienne = SelectObject(dc, f);
        for (i, ligne) in texte.lines().take(LIGNES).enumerate() {
            let coupee: String = if ligne.chars().count() > COLONNES { ligne.chars().take(COLONNES - 1).chain(std::iter::once('…')).collect() } else { ligne.to_string() };
            let t: Vec<u16> = coupee.encode_utf16().collect();
            let _ = TextOutW(dc, 8 * e, (8 + i as i32 * PAS) * e, &t);
        }
        SelectObject(dc, ancienne);
        let _ = DeleteObject(f);
        SelectObject(dc, ancien);
        if let Some(mut img) = pixels(bmp) {
            // GDI n'écrit pas d'alpha : la clarté du pixel en tient lieu (blanc sur transparent, prémultiplié)
            for p in img.pixels.as_chunks_mut::<4>().0 {
                let c = p[0].max(p[1]).max(p[2]);
                p.copy_from_slice(&[c, c, c, c]);
            }
            resultat = Some(img);
        }
        let _ = DeleteObject(bmp);
    }
    let _ = DeleteDC(dc);
    resultat
}

/// Fils qui préparent les vignettes de la grille : les demandes les plus récentes d'abord
/// (en défilant vite, ce qui est à l'écran passe avant ce qui l'a quitté)
pub struct Vignettes {
    file: std::sync::Arc<(std::sync::Mutex<Vec<String>>, std::sync::Condvar)>,
}

impl Vignettes {
    pub fn nouveau(hwnd: HWND) -> Self {
        let file = std::sync::Arc::new((std::sync::Mutex::new(Vec::<String>::new()), std::sync::Condvar::new()));
        let fenetre = hwnd.0 as isize;
        for _ in 0..2 {
            let file = file.clone();
            std::thread::spawn(move || unsafe {
                let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE);
                loop {
                    let chemin = {
                        let (m, cv) = &*file;
                        let mut g = m.lock().unwrap_or_else(|e| e.into_inner());
                        while g.is_empty() {
                            g = cv.wait(g).unwrap_or_else(|e| e.into_inner());
                        }
                        g.pop().unwrap_or_default()
                    };
                    let r = Box::new((chemin.clone(), miniature(&chemin, 256)));
                    let p = Box::into_raw(r);
                    if PostMessageW(HWND(fenetre as *mut _), WM_VIGNETTE, WPARAM(0), LPARAM(p as isize)).is_err() {
                        drop(Box::from_raw(p));
                    }
                }
            });
        }
        Self { file }
    }

    /// Ajoute des demandes ; renvoie celles qui ont été oubliées (à redemander si elles reviennent à l'écran)
    pub fn demander(&self, chemins: Vec<String>) -> Vec<String> {
        if chemins.is_empty() {
            return Vec::new();
        }
        let (m, cv) = &*self.file;
        let mut g = m.lock().unwrap_or_else(|e| e.into_inner());
        g.extend(chemins);
        // Les plus anciennes demandes (hors de l'écran depuis longtemps) sont oubliées
        let n = g.len();
        let oubliees = if n > 300 { g.drain(..n - 300).collect() } else { Vec::new() };
        cv.notify_all();
        oubliees
    }
}

/// Image en pixels BGRA prémultipliés, ligne après ligne
pub struct Image {
    pub largeur: u32,
    pub hauteur: u32,
    pub pixels: Vec<u8>,
}

pub struct Apercu {
    pub numero: u64,
    pub nom: String,
    pub icone: Icone,
    pub image: Option<Image>,
    /// Premières lignes d'un fichier texte
    pub texte: Option<String>,
    /// Code et texte brut : le volet ajoute une colonne de numéros de ligne (jamais pour le Markdown mis en forme)
    pub numeroter: bool,
    /// Libellé, valeur
    pub infos: Vec<(String, String)>,
    /// Couleurs du texte (code coloré, Markdown mis en forme) : début et longueur en UTF-16
    pub teintes: Vec<(u32, u32, Teinte)>,
}

/// Couleur d'un morceau du texte de l'aperçu
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Teinte {
    MotCle,
    Chaine,
    Commentaire,
    Nombre,
    /// Markdown : titre (gras, plus grand), gras, lien, marques discrètes
    Titre,
    Gras,
    Lien,
    Discret,
}

/// Extensions lues comme du texte
const TEXTE: &[&str] = &[
    "TXT", "MD", "LOG", "INI", "CFG", "CONF", "TOML", "YML", "YAML", "JSON", "XML", "CSV", "HTML", "HTM", "CSS", "JS", "TS", "JSX",
    "TSX", "RS", "PY", "C", "H", "CPP", "HPP", "CS", "JAVA", "KT", "GO", "PHP", "RB", "LUA", "SH", "PS1", "BAT", "CMD", "SQL", "REG",
    "GITIGNORE", "SVG", "DIFF",
];

/// Prépare l'aperçu de `chemin` dans un fil séparé et l'envoie à la fenêtre
pub fn charger(hwnd: HWND, numero: u64, chemin: String, entree: Option<Entree>) {
    let fenetre = hwnd.0 as isize;
    std::thread::spawn(move || unsafe {
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE);
        let a = Box::new(preparer(numero, &chemin, entree.as_ref()));
        CoUninitialize();
        let p = Box::into_raw(a);
        if PostMessageW(HWND(fenetre as *mut _), WM_APERCU, WPARAM(0), LPARAM(p as isize)).is_err() {
            drop(Box::from_raw(p));
        }
    });
}

unsafe fn preparer(numero: u64, chemin: &str, e: Option<&Entree>) -> Apercu {
    let nom = match e {
        Some(e) => e.nom.clone(),
        None => chemin.trim_end_matches('\\').rsplit('\\').next().unwrap_or(chemin).to_string(),
    };
    let mut a = Apercu { numero, nom, icone: e.map_or(Icone::Dossier, icones::pour), image: None, texte: None, numeroter: false, infos: Vec::new(), teintes: Vec::new() };
    // Lire un fichier pour l'aperçu fait travailler des modules du Shell et des décodeurs : pas dans un dossier non fiable
    if crate::reglages::non_fiable(if e.is_some() { dossier::dossier_de(chemin) } else { chemin }) {
        a.infos.push(("DOSSIER".into(), "NON FIABLE · APERCU COUPE".into()));
        if let Some(e) = e {
            a.infos.push(("TYPE".into(), icones::nom_type(e).to_string()));
        }
        return a;
    }
    let Some(e) = e else {
        // Aucun élément choisi : le dossier ouvert lui-même
        a.infos.push(("TYPE".into(), "DOSSIER".into()));
        infos_dossier(&mut a, chemin);
        return a;
    };
    a.infos.push(("TYPE".into(), icones::nom_type(e).to_string()));
    match &e.genre {
        Genre::Disque { libre, total, .. } => {
            a.infos.push(("LIBRE".into(), systeme::taille(*libre)));
            a.infos.push(("TOTAL".into(), systeme::taille(*total)));
        }
        Genre::Dossier => infos_dossier(&mut a, chemin),
        Genre::Fichier => {
            a.infos.push(("TAILLE".into(), systeme::taille(e.taille)));
            // Fichier vide : sans cette ligne la carte n'a ni image ni texte et paraît cassée (audit P2-02)
            if e.taille == 0 && e.nuage == dossier::Nuage::Aucun {
                a.infos.push(("CONTENU".into(), "FICHIER VIDE · RIEN À AFFICHER".into()));
            }
            a.infos.push(("MODIFIÉ".into(), systeme::date(e.modif)));
            a.infos.push(("EMPLACEMENT".into(), dossier::dossier_de(chemin).to_string()));
            if e.ajout > e.modif + 20_000_000 {
                a.infos.push(("ARRIVE LE".into(), systeme::date(e.ajout)));
            }
            // Fichier en ligne seulement : le lire (même son flux) le téléchargerait
            if e.nuage == dossier::Nuage::Aucun {
                if let Some((zone, hote)) = crate::outils::origine_internet(chemin) {
                    let quoi = if zone >= 4 { "SITE NON FIABLE" } else { "INTERNET" };
                    a.infos.push(("ORIGINE".into(), hote.map_or(quoi.to_string(), |h| format!("{quoi} · {h}"))));
                }
                let s = crate::outils::signature(chemin);
                if s != crate::outils::Signature::SansObjet {
                    a.infos.push(("SIGNATURE".into(), s.texte()));
                }
            }
            if e.nuage != dossier::Nuage::Aucun {
                a.infos.push(("Disponibilité".into(), e.nuage.libelle().into()));
            }
            let en_ligne = e.nuage == dossier::Nuage::EnLigne;
            // PDF : la première page rendue par Windows, et le nombre de pages
            if e.ext == "PDF" && !en_ligne {
                if let Some((img, pages)) = pdf(chemin, 520, 0) {
                    a.image = Some(img);
                    a.infos.push(("PAGES".into(), systeme::nombre(pages as usize)));
                }
            }
            if matches!(e.ext.as_str(), "TTF" | "OTF" | "TTC") && !en_ligne {
                police(&mut a, chemin);
            }
            if a.image.is_none() {
                a.image = miniature(chemin, 520);
            }
            // Archive : son contenu, lu par tar.exe. Une archive tar compressée se lit en entier pour être listée : bornée en taille.
            let tar_compresse = matches!(e.ext.as_str(), "TAR" | "GZ" | "TGZ" | "BZ2" | "TBZ2" | "XZ" | "TXZ" | "ZST");
            if crate::archive::est_archive(&e.ext) && !en_ligne && (!tar_compresse || e.taille <= 200 << 20) {
                contenu_archive(&mut a, chemin);
            }
            // Fichier seulement en ligne : le lire le téléchargerait, on s'en abstient
            if a.image.is_none() && TEXTE.contains(&e.ext.as_str()) && !en_ligne {
                a.texte = debut_texte(chemin);
                a.numeroter = e.ext != "MD";
                if let Some(t) = &a.texte {
                    if e.ext == "MD" {
                        let (rendu, teintes) = markdown(t);
                        a.texte = Some(rendu);
                        a.teintes = teintes;
                    } else if e.ext == "DIFF" {
                        a.teintes = crate::comparaison::teintes(t);
                    } else if !matches!(e.ext.as_str(), "TXT" | "LOG" | "CSV") {
                        a.teintes = colorer(t, &e.ext);
                    }
                }
            }
            proprietes(&mut a, chemin);
        }
    }
    a
}

/// Nombre d'éléments, taille décompressée et premiers chemins d'une archive
fn contenu_archive(a: &mut Apercu, chemin: &str) {
    const MAX: usize = 40;
    let Ok(membres) = crate::archive::membres(chemin) else {
        a.infos.push(("CONTENU".into(), "ILLISIBLE".into()));
        return;
    };
    let fichiers = membres.iter().filter(|m| !m.dossier).count();
    a.infos.push(("CONTENU".into(), format!("{} FICHIER{} · {} DOSSIER{}", systeme::nombre(fichiers), if fichiers > 1 { "S" } else { "" }, systeme::nombre(membres.len() - fichiers), if membres.len() - fichiers > 1 { "S" } else { "" })));
    a.infos.push(("DECOMPRESSE".into(), systeme::taille(membres.iter().map(|m| m.taille).sum())));
    let mut t: String = membres.iter().take(MAX).map(|m| if m.dossier { format!("{}/\n", court(&m.chemin)) } else { format!("{}  {}\n", court(&m.chemin), systeme::taille(m.taille)) }).collect();
    if membres.len() > MAX {
        t += &format!("… +{} AUTRES", systeme::nombre(membres.len() - MAX));
    }
    a.texte = Some(t);
}

/// Un long chemin perd son début, pas sa fin : le volet le coupait à droite, là où est le nom du fichier
fn court(chemin: &str) -> String {
    const MAX: usize = 34;
    let n = chemin.chars().count();
    if n <= MAX {
        return chemin.to_string();
    }
    format!("…{}", chemin.chars().skip(n - (MAX - 1)).collect::<String>())
}

/// Nom de famille (nameID 1) d'une police TrueType / OpenType, lu dans sa table `name`. Première police d'une collection .ttc.
fn famille_police(o: &[u8]) -> Option<String> {
    let u16_ = |i: usize| o.get(i..i + 2).map(|b| u16::from_be_bytes([b[0], b[1]]) as usize);
    let u32_ = |i: usize| o.get(i..i + 4).map(|b| u32::from_be_bytes(b.try_into().unwrap()) as usize);
    let debut = if o.get(..4)? == b"ttcf" { u32_(12)? } else { 0 };
    let tables = u16_(debut + 4)?;
    let table = (0..tables).map(|k| debut + 12 + 16 * k).find(|&e| o.get(e..e + 4) == Some(b"name"))?;
    let base = u32_(table + 8)?;
    let (n, chaines) = (u16_(base + 2)?, base + u16_(base + 4)?);
    (0..n).find_map(|k| {
        let r = base + 6 + 12 * k;
        // Windows (3), nom de famille (1) : UTF-16 gros-boutiste
        (u16_(r)? == 3 && u16_(r + 6)? == 1).then_some(())?;
        let (len, dep) = (u16_(r + 8)?, u16_(r + 10)?);
        let u: Vec<u16> = o.get(chaines + dep..chaines + dep + len)?.as_chunks::<2>().0.iter().map(|c| u16::from_be_bytes([c[0], c[1]])).collect();
        String::from_utf16(&u).ok()
    })
}

/// Aperçu d'une police : son nom et un texte d'essai dessiné avec elle (installée pour ce processus seulement)
unsafe fn police(a: &mut Apercu, chemin: &str) {
    let Some(famille) = std::fs::read(chemin).ok().and_then(|o| famille_police(&o)) else { return };
    a.infos.push(("FAMILLE".into(), famille.to_uppercase()));
    let w: Vec<u16> = chemin.encode_utf16().chain(std::iter::once(0)).collect();
    // FR_PRIVATE : la police n'est visible que de ce processus et disparaît avec lui, rien n'est installé
    if AddFontResourceExW(PCWSTR(w.as_ptr()), FR_PRIVATE, None) == 0 {
        return;
    }
    let (l, h) = (520, 260);
    let dc = CreateCompatibleDC(None);
    let info = BITMAPINFO { bmiHeader: BITMAPINFOHEADER { biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32, biWidth: l, biHeight: -h, biPlanes: 1, biBitCount: 32, biCompression: BI_RGB.0, ..Default::default() }, ..Default::default() };
    let mut bits = std::ptr::null_mut();
    if let Ok(bmp) = CreateDIBSection(dc, &info, DIB_RGB_COLORS, &mut bits, None, 0) {
        let ancien = SelectObject(dc, bmp);
        // SAFETY: la section DIB est de l × h pixels de 4 octets, vivante jusqu'au DeleteObject ci-dessous
        std::ptr::write_bytes(bits as *mut u8, 0, (l * h * 4) as usize);
        SetBkMode(dc, TRANSPARENT);
        SetTextColor(dc, windows::Win32::Foundation::COLORREF(0xFFFFFF));
        let nom: Vec<u16> = famille.encode_utf16().chain(std::iter::once(0)).collect();
        let mut y = 8;
        for (taille, texte) in [(64, "Aa Bb Cc"), (34, "ABCDEFGHIJKLM"), (34, "abcdefghijklm"), (34, "0123456789 &?!")] {
            let f = CreateFontW(-taille, 0, 0, 0, 400, 0, 0, 0, DEFAULT_CHARSET.0 as u32, 0, 0, ANTIALIASED_QUALITY.0 as u32, 0, PCWSTR(nom.as_ptr()));
            let ancienne = SelectObject(dc, f);
            let t: Vec<u16> = texte.encode_utf16().collect();
            let _ = TextOutW(dc, 8, y, &t);
            y += taille + taille / 4;
            SelectObject(dc, ancienne);
            let _ = DeleteObject(f);
        }
        SelectObject(dc, ancien);
        if let Some(mut img) = pixels(bmp) {
            // GDI n'écrit pas d'alpha : la clarté du pixel en tient lieu (blanc sur transparent, prémultiplié)
            for p in img.pixels.as_chunks_mut::<4>().0 {
                let c = p[0].max(p[1]).max(p[2]);
                p.copy_from_slice(&[c, c, c, c]);
            }
            a.image = Some(img);
        }
        let _ = DeleteObject(bmp);
    }
    let _ = DeleteDC(dc);
    let _ = RemoveFontResourceExW(PCWSTR(w.as_ptr()), FR_PRIVATE.0, None);
}

/// Page `numero` (à partir de 0, ramenée dans les bornes) d'un PDF (lecteur PDF de Windows), et le nombre de pages
fn pdf(chemin: &str, largeur: u32, numero: u32) -> Option<(Image, u32)> {
    use windows::core::HSTRING;
    use windows::Data::Pdf::{PdfDocument, PdfPageRenderOptions};
    use windows::Graphics::Imaging::BitmapDecoder;
    use windows::Storage::Streams::InMemoryRandomAccessStream;
    use windows::Storage::StorageFile;
    let fichier = StorageFile::GetFileFromPathAsync(&HSTRING::from(chemin)).ok()?.get().ok()?;
    let doc = PdfDocument::LoadFromFileAsync(&fichier).ok()?.get().ok()?;
    let pages = doc.PageCount().ok()?;
    if pages == 0 {
        return None;
    }
    let page = doc.GetPage(numero.min(pages - 1)).ok()?;
    let options = PdfPageRenderOptions::new().ok()?;
    options.SetDestinationWidth(largeur).ok()?;
    let flux = InMemoryRandomAccessStream::new().ok()?;
    page.RenderWithOptionsToStreamAsync(&flux, &options).ok()?.get().ok()?;
    flux.Seek(0).ok()?;
    let decodeur = BitmapDecoder::CreateAsync(&flux).ok()?.get().ok()?;
    let image = decodeur.GetSoftwareBitmapAsync().ok()?.get().ok()?;
    image_de_bitmap(&image).map(|i| (i, pages))
}

/// Pixels BGRA prémultipliés d'un bitmap WinRT (converti au besoin)
pub(crate) fn image_de_bitmap(bitmap: &windows::Graphics::Imaging::SoftwareBitmap) -> Option<Image> {
    use windows::Graphics::Imaging::{BitmapAlphaMode, BitmapPixelFormat, SoftwareBitmap};
    use windows::Storage::Streams::{Buffer, DataReader};
    let image = SoftwareBitmap::ConvertWithAlpha(bitmap, BitmapPixelFormat::Bgra8, BitmapAlphaMode::Premultiplied).ok()?;
    let (l, h) = (image.PixelWidth().ok()? as u32, image.PixelHeight().ok()? as u32);
    if l == 0 || h == 0 {
        return None;
    }
    let tampon = Buffer::Create(l * h * 4).ok()?;
    image.CopyToBuffer(&tampon).ok()?;
    let mut pixels = vec![0u8; (l * h * 4) as usize];
    DataReader::FromBuffer(&tampon).ok()?.ReadBytes(&mut pixels).ok()?;
    Some(Image { largeur: l, hauteur: h, pixels })
}

/// Mots-clés reconnus dans les fichiers de code (tous langages confondus)
const MOTS: &[&str] = &[
    "fn", "let", "mut", "pub", "use", "mod", "struct", "enum", "impl", "trait", "match", "if", "else", "for", "while", "loop", "return",
    "break", "continue", "in", "as", "where", "const", "static", "type", "self", "Self", "true", "false", "None", "Some", "Ok", "Err",
    "unsafe", "def", "class", "import", "from", "pass", "lambda", "yield", "with", "try", "except", "finally", "raise", "async", "await",
    "function", "var", "new", "this", "null", "undefined", "export", "default", "extends", "interface", "public", "private",
    "protected", "void", "int", "float", "double", "char", "bool", "string", "package", "func", "go", "defer", "case", "switch", "do",
    "typedef", "unsigned", "include", "define", "elif", "fi", "then", "echo", "local", "and", "or", "not", "is", "nil", "end",
    "require", "param", "foreach", "throw", "catch", "True", "False", "SELECT", "FROM", "WHERE", "INSERT", "UPDATE", "DELETE", "JOIN",
    "CREATE", "TABLE", "AND", "OR", "NOT", "NULL", "ORDER", "BY", "GROUP", "INTO", "VALUES",
];

/// Colore un fichier de code : mots-clés, chaînes, commentaires, nombres
fn colorer(t: &str, ext: &str) -> Vec<(u32, u32, Teinte)> {
    let diese = matches!(ext, "PY" | "SH" | "PS1" | "TOML" | "YML" | "YAML" | "INI" | "CFG" | "CONF" | "RB" | "GITIGNORE");
    let tirets = matches!(ext, "SQL" | "LUA");
    let balises = matches!(ext, "HTML" | "HTM" | "XML" | "SVG");
    let c: Vec<char> = t.chars().collect();
    // Position UTF-16 de chaque caractère
    let mut pos = Vec::with_capacity(c.len() + 1);
    let mut p = 0u32;
    for ch in &c {
        pos.push(p);
        p += ch.len_utf16() as u32;
    }
    pos.push(p);
    let n = c.len();
    let mut v = Vec::new();
    let ajouter = |a: usize, b: usize, te: Teinte, v: &mut Vec<(u32, u32, Teinte)>| {
        if b > a {
            v.push((pos[a], pos[b] - pos[a], te));
        }
    };
    let fin_ligne = |i: usize| (i..n).find(|&k| c[k] == '\n').unwrap_or(n);
    let commence = |i: usize, motif: &str| motif.chars().enumerate().all(|(k, m)| c.get(i + k) == Some(&m));
    let mut i = 0;
    while i < n {
        let ch = c[i];
        // Commentaires en bloc
        let bloc = if commence(i, "/*") && !balises {
            Some("*/")
        } else if balises && commence(i, "<!--") {
            Some("-->")
        } else {
            None
        };
        if let Some(fin) = bloc {
            let mut j = i + 2;
            while j < n && !commence(j, fin) {
                j += 1;
            }
            let j = (j + fin.chars().count()).min(n);
            ajouter(i, j, Teinte::Commentaire, &mut v);
            i = j;
            continue;
        }
        // Commentaires de fin de ligne
        if (commence(i, "//") && !balises) || (diese && ch == '#') || (tirets && commence(i, "--")) || (ext == "REG" && ch == ';') {
            let j = fin_ligne(i);
            ajouter(i, j, Teinte::Commentaire, &mut v);
            i = j;
            continue;
        }
        // Chaînes (sur une ligne)
        if ch == '"' || ch == '`' || (ch == '\'' && ext != "RS") {
            let mut j = i + 1;
            while j < n && c[j] != ch && c[j] != '\n' {
                if c[j] == '\\' {
                    j += 1;
                }
                j += 1;
            }
            let j = (j + 1).min(n);
            ajouter(i, j, Teinte::Chaine, &mut v);
            i = j;
            continue;
        }
        let avant_mot = i == 0 || !(c[i - 1].is_alphanumeric() || c[i - 1] == '_');
        if ch.is_ascii_digit() && avant_mot {
            let mut j = i;
            while j < n && (c[j].is_ascii_alphanumeric() || c[j] == '.' || c[j] == '_') {
                j += 1;
            }
            ajouter(i, j, Teinte::Nombre, &mut v);
            i = j;
            continue;
        }
        if (ch.is_alphabetic() || ch == '_') && avant_mot {
            let mut j = i;
            while j < n && (c[j].is_alphanumeric() || c[j] == '_') {
                j += 1;
            }
            let mot: String = c[i..j].iter().collect();
            // HTML / XML : le nom de la balise
            let balise = balises && i > 0 && (c[i - 1] == '<' || c[i - 1] == '/');
            if balise || MOTS.contains(&mot.as_str()) {
                ajouter(i, j, Teinte::MotCle, &mut v);
            }
            i = j;
            continue;
        }
        i += 1;
    }
    v
}

/// Markdown mis en forme : titres, listes à puces, citations, gras, code, liens (texte réécrit + couleurs)
fn markdown(t: &str) -> (String, Vec<(u32, u32, Teinte)>) {
    let mut sortie = String::new();
    let mut teintes = Vec::new();
    let mut pos = 0u32;
    let mut dans_code = false;
    let mut pousser = |s: &str, te: Option<Teinte>, sortie: &mut String, teintes: &mut Vec<(u32, u32, Teinte)>| {
        let l = s.encode_utf16().count() as u32;
        if let (Some(te), true) = (te, l > 0) {
            teintes.push((pos, l, te));
        }
        pos += l;
        sortie.push_str(s);
    };
    for ligne in t.lines() {
        let l = ligne.trim_start();
        if l.starts_with("```") {
            dans_code = !dans_code;
            pousser("┄┄┄┄┄┄┄┄\n", Some(Teinte::Discret), &mut sortie, &mut teintes);
            continue;
        }
        if dans_code {
            pousser(&format!("  {ligne}\n"), Some(Teinte::Chaine), &mut sortie, &mut teintes);
            continue;
        }
        let dieses = l.chars().take_while(|&c| c == '#').count();
        if (1..=6).contains(&dieses) && l[dieses..].starts_with(' ') {
            let titre = l[dieses..].trim();
            let titre = if dieses <= 2 { titre.to_uppercase() } else { titre.to_string() };
            pousser(&titre, Some(Teinte::Titre), &mut sortie, &mut teintes);
            pousser("\n", None, &mut sortie, &mut teintes);
            continue;
        }
        if l.chars().all(|c| c == '-' || c == '*' || c == '_') && l.len() >= 3 {
            pousser("────────────────\n", Some(Teinte::Discret), &mut sortie, &mut teintes);
            continue;
        }
        let retrait = &ligne[..ligne.len() - l.len()];
        let (prefixe, reste) = if let Some(r) = l.strip_prefix("- ").or_else(|| l.strip_prefix("* ")).or_else(|| l.strip_prefix("+ ")) {
            (format!("{retrait}• "), r)
        } else if let Some(r) = l.strip_prefix("> ") {
            ("│ ".to_string(), r)
        } else {
            (retrait.to_string(), l)
        };
        let citation = l.starts_with("> ");
        pousser(&prefixe, Some(Teinte::Discret), &mut sortie, &mut teintes);
        // En ligne : **gras**, `code`, [lien](adresse)
        let c: Vec<char> = reste.chars().collect();
        let mut i = 0;
        let mut texte = String::new();
        let defaut = if citation { Some(Teinte::Discret) } else { None };
        while i < c.len() {
            let suite: String = c[i..].iter().collect();
            let mut special = None;
            if let Some(reste) = suite.strip_prefix("**") {
                if let Some(f) = reste.find("**") {
                    special = Some((reste[..f].to_string(), Teinte::Gras, 4 + reste[..f].chars().count()));
                }
            } else if let Some(reste) = suite.strip_prefix('`') {
                if let Some(f) = reste.find('`') {
                    special = Some((reste[..f].to_string(), Teinte::Chaine, 2 + reste[..f].chars().count()));
                }
            } else if suite.starts_with('[') {
                if let (Some(f), true) = (suite.find("]("), true) {
                    if let Some(g) = suite[f..].find(')') {
                        let lib = suite[1..f].to_string();
                        special = Some((lib, Teinte::Lien, suite[..f + g + 1].chars().count()));
                    }
                }
            }
            match special {
                Some((s, te, saut)) => {
                    pousser(&std::mem::take(&mut texte), defaut, &mut sortie, &mut teintes);
                    pousser(&s, Some(te), &mut sortie, &mut teintes);
                    i += saut;
                }
                None => {
                    texte.push(c[i]);
                    i += 1;
                }
            }
        }
        texte.push('\n');
        pousser(&texte, defaut, &mut sortie, &mut teintes);
    }
    (sortie, teintes)
}

/// Date de modification d'un chemin en FILETIME ; 0 s'il est illisible.
fn modifie(chemin: &str) -> u64 {
    use std::time::UNIX_EPOCH;
    std::fs::metadata(chemin)
        .ok()
        .and_then(|m| m.modified().ok())
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_secs() * 10_000_000 + u64::from(d.subsec_nanos()) / 100 + 116_444_736_000_000_000)
        .unwrap_or(0)
}

/// Métadonnées du dossier, comme la maquette : type, taille, date et emplacement.
fn infos_dossier(a: &mut Apercu, chemin: &str) {
    a.infos.push(("TAILLE".into(), "—".into()));
    let date = systeme::date(modifie(chemin));
    if !date.is_empty() {
        a.infos.push(("MODIFIÉ".into(), date));
    }
    a.infos.push(("EMPLACEMENT".into(), dossier::dossier_de(chemin).to_string()));
}

/// Miniature fournie par Windows (la même que dans l'Explorateur). Rien si le fichier n'en a pas.
pub unsafe fn miniature(chemin: &str, cote: i32) -> Option<Image> {
    let w: Vec<u16> = chemin.encode_utf16().chain(std::iter::once(0)).collect();
    let usine: IShellItemImageFactory = SHCreateItemFromParsingName(PCWSTR(w.as_ptr()), None).ok()?;
    let hbmp = usine.GetImage(SIZE { cx: cote, cy: cote }, SIIGBF_THUMBNAILONLY | SIIGBF_BIGGERSIZEOK).ok()?;
    let image = pixels(hbmp);
    let _ = DeleteObject(hbmp);
    image
}

/// Copie les pixels d'une image GDI (32 bits, du haut vers le bas)
unsafe fn pixels(hbmp: HBITMAP) -> Option<Image> {
    let mut bm = BITMAP::default();
    if GetObjectW(hbmp, std::mem::size_of::<BITMAP>() as i32, Some(&mut bm as *mut _ as *mut _)) == 0 {
        return None;
    }
    let (l, h) = (bm.bmWidth, bm.bmHeight.abs());
    if l <= 0 || h <= 0 {
        return None;
    }
    let mut info = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: l,
            biHeight: -h,
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            ..Default::default()
        },
        ..Default::default()
    };
    let mut px = vec![0u8; (l * h * 4) as usize];
    let dc = GetDC(None);
    let n = GetDIBits(dc, hbmp, 0, h as u32, Some(px.as_mut_ptr() as *mut _), &mut info, DIB_RGB_COLORS);
    ReleaseDC(None, dc);
    if n == 0 {
        return None;
    }
    // Photos : souvent sans canal alpha (tout à 0) → opaques
    if px.as_chunks::<4>().0.iter().all(|p| p[3] == 0) {
        for p in px.as_chunks_mut::<4>().0 {
            p[3] = 255;
        }
    }
    Some(Image { largeur: l as u32, hauteur: h as u32, pixels: px })
}

fn debut_texte(chemin: &str) -> Option<String> {
    use std::io::Read;
    let mut f = std::fs::File::open(chemin).ok()?;
    let mut buf = vec![0u8; 6000];
    let n = f.read(&mut buf).ok()?;
    buf.truncate(n);
    // UTF-16 (fichiers .reg, journaux Windows)
    let texte = if buf.starts_with(&[0xFF, 0xFE]) {
        let u: Vec<u16> = buf[2..].as_chunks::<2>().0.iter().map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
        String::from_utf16_lossy(&u)
    } else {
        let b = buf.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(&buf);
        if b.iter().take(2000).filter(|&&c| c == 0).count() > 4 {
            return None; // binaire
        }
        match std::str::from_utf8(b) {
            Ok(s) => s.to_string(),
            // Coupé au milieu d'un caractère : seule la fin est invalide
            Err(e) if e.error_len().is_none() => String::from_utf8_lossy(&b[..e.valid_up_to()]).into_owned(),
            // Ancien fichier Windows (ANSI) : octet = caractère Latin-1
            Err(_) => b.iter().map(|&c| c as char).collect(),
        }
    };
    Some(texte.lines().take(80).map(|l| l.replace('\t', "    ")).collect::<Vec<_>>().join("\n"))
}

/// Dimensions, durée et auteur d'un fichier (colonnes de la liste) ; COM doit être prêt sur ce fil
/// `complet` : lit aussi l'origine et la signature, même si leurs colonnes sont cachées (regroupement par origine)
pub fn lire_meta(chemin: &str, complet: bool) -> dossier::Meta {
    let mut m = dossier::Meta::default();
    let w: Vec<u16> = chemin.encode_utf16().chain(std::iter::once(0)).collect();
    // Les propriétés du Shell sont lentes (10 à 50 ms par fichier) : seulement si une de leurs colonnes est montrée
    let proprietes = crate::reglages::colonne_montree(crate::reglages::COL_DIMENSIONS | crate::reglages::COL_DUREE | crate::reglages::COL_AUTEUR);
    if proprietes {
        lire_proprietes(&w, &mut m);
    }
    // Lues seulement si leur colonne est montrée : la vérification d'une signature coûte plus qu'une propriété
    if complet || crate::reglages::colonne_montree(crate::reglages::COL_ORIGINE) {
        m.origine = crate::outils::origine_internet(chemin).map(|(zone, hote)| hote.unwrap_or_else(|| if zone >= 4 { "SITE NON FIABLE".into() } else { "INTERNET".into() })).unwrap_or_default();
    }
    if complet || crate::reglages::colonne_montree(crate::reglages::COL_SIGNE) {
        m.signature = crate::outils::signature(chemin).texte();
    }
    m
}

fn lire_proprietes(w: &[u16], m: &mut dossier::Meta) {
    unsafe {
        let Ok(item) = SHCreateItemFromParsingName::<_, _, IShellItem2>(PCWSTR(w.as_ptr()), None) else { return };
        let texte = |cle: &PROPERTYKEY| -> Option<String> {
            let p = item.GetString(cle).ok()?;
            let s = p.to_string().ok();
            CoTaskMemFree(Some(p.0 as *const _));
            s.map(|s| s.trim().to_string()).filter(|s| !s.is_empty())
        };
        let nombre = |cle: &PROPERTYKEY| item.GetUInt32(cle).unwrap_or(0);
        m.largeur = nombre(&PKEY_Image_HorizontalSize);
        m.hauteur = nombre(&PKEY_Image_VerticalSize);
        if m.largeur == 0 {
            m.largeur = nombre(&PKEY_Video_FrameWidth);
            m.hauteur = nombre(&PKEY_Video_FrameHeight);
        }
        m.duree = item.GetUInt64(&PKEY_Media_Duration).unwrap_or(0);
        m.auteur = texte(&PKEY_Author).or_else(|| texte(&PKEY_Music_Artist)).or_else(|| texte(&PKEY_Music_AlbumArtist)).unwrap_or_default();
    }
}

/// Informations des fichiers multimédia : titre, artiste, album, durée, dimensions
unsafe fn proprietes(a: &mut Apercu, chemin: &str) {
    let w: Vec<u16> = chemin.encode_utf16().chain(std::iter::once(0)).collect();
    let Ok(item) = SHCreateItemFromParsingName::<_, _, IShellItem2>(PCWSTR(w.as_ptr()), None) else { return };
    let texte = |cle: &PROPERTYKEY| -> Option<String> {
        let p = item.GetString(cle).ok()?;
        let s = p.to_string().ok();
        CoTaskMemFree(Some(p.0 as *const _));
        s.filter(|s| !s.trim().is_empty())
    };
    let cles: [(&str, &PROPERTYKEY); 5] = [
        ("TITRE", &PKEY_Title),
        ("ARTISTE", &PKEY_Music_Artist),
        ("ALBUM", &PKEY_Music_AlbumTitle),
        ("DIMENSIONS", &PKEY_Image_Dimensions),
        ("APPAREIL", &PKEY_Photo_CameraModel),
    ];
    for (libelle, cle) in cles {
        if let Some(v) = texte(cle) {
            a.infos.push((libelle.into(), v));
        }
    }
    // Durée en centaines de nanosecondes
    if let Ok(d) = item.GetUInt64(&PKEY_Media_Duration) {
        if d > 0 {
            let s = d / 10_000_000;
            a.infos.push(("DUREE".into(), if s >= 3600 { format!("{}:{:02}:{:02}", s / 3600, s / 60 % 60, s % 60) } else { format!("{}:{:02}", s / 60, s % 60) }));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn galerie_vide() -> Galerie {
        Galerie::vierge(1, "a.png".into(), String::new(), crate::icones::pour_extension("png"), None, "png".into())
    }

    #[test]
    fn zoom_borne_et_point_fixe() {
        let mut g = galerie_vide();
        let (img, zone) = ((800, 600), (400.0, 300.0));
        // Ajustement : l'image remplit la zone sans la dépasser
        assert_eq!(g.taille_affichee(img, zone), (400.0, 300.0));
        g.zoomer(2.0, (100.0, 0.0), img, zone);
        assert_eq!(g.zoom, 2.0);
        // Le point sous le curseur ne bouge pas : décalage = −u, dans la limite du surplus (200 DIP de chaque côté)
        assert_eq!(g.decalage, (-100.0, 0.0));
        // Le zoom reste entre ZOOM_MIN et ZOOM_MAX
        g.zoomer(100.0, (0.0, 0.0), img, zone);
        assert_eq!(g.zoom, ZOOM_MAX);
        g.zoomer(0.0001, (0.0, 0.0), img, zone);
        assert_eq!(g.zoom, ZOOM_MIN);
        // Revenu à 1 : l'image n'a plus de surplus, elle est recentrée
        assert_eq!(g.decalage, (0.0, 0.0));
    }

    #[test]
    fn decalage_reste_dans_l_image_et_echelle_voulue() {
        let mut g = galerie_vide();
        let (img, zone) = ((800, 600), (400.0, 300.0));
        g.zoomer(2.0, (0.0, 0.0), img, zone);
        g.decalage = (10_000.0, -10_000.0);
        g.borner(img, zone);
        assert_eq!(g.decalage, (200.0, -150.0));
        // Image 800 × 600 à zoom 2 dans 400 × 300 : bords alignés sur ceux de la zone, côté gauche et bas
        assert_eq!(g.rect_image(img, zone), [0.0, -300.0, 800.0, 600.0]);
        assert_eq!(g.echelle_voulue(), 2);
        g.zoom = 1.0;
        assert_eq!(g.echelle_voulue(), 1);
        g.zoom = 8.0;
        assert_eq!(g.echelle_voulue(), ECHELLE_MAX);
    }

    #[test]
    fn echelle_deux_ne_change_pas_la_taille_affichee() {
        let mut g = galerie_vide();
        let zone = (400.0, 300.0);
        let a = g.taille_affichee((800, 600), zone);
        g.echelle = 2;
        assert_eq!(g.taille_affichee((1600, 1200), zone), a);
    }

    #[test]
    fn famille_d_une_police_et_chemin_court() {
        let o = std::fs::read(r"C:\Windows\Fonts\arial.ttf").expect("Arial est livrée avec Windows");
        assert_eq!(super::famille_police(&o).as_deref(), Some("Arial"));
        assert_eq!(super::famille_police(&o[..40]), None);
        assert_eq!(super::court("a/b.txt"), "a/b.txt");
        let c = super::court(&format!("{}/fin.txt", "dossier/".repeat(10)));
        assert!(c.starts_with('…') && c.ends_with("fin.txt") && c.chars().count() == 34);
    }

    /// Le coup d'œil passe par la même cascade que le volet : un PDF (aucune miniature du shell) et une police donnent une image
    #[test]
    fn image_galerie_pdf_et_police() {
        unsafe {
            let _ = windows::Win32::System::Com::CoInitializeEx(None, windows::Win32::System::Com::COINIT_APARTMENTTHREADED);
        }
        // PDF d'une page, table xref calculée à la main
        let objets = [
            "<< /Type /Catalog /Pages 2 0 R >>".to_string(),
            "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_string(),
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 100] /Contents 4 0 R >>".to_string(),
            "<< /Length 31 >>
stream
0 0 0 rg 20 20 160 60 re f
endstream".to_string(),
        ];
        let mut pdf = String::from("%PDF-1.4
");
        let mut positions = Vec::new();
        for (i, o) in objets.iter().enumerate() {
            positions.push(pdf.len());
            pdf.push_str(&format!("{} 0 obj
{}
endobj
", i + 1, o));
        }
        let xref = pdf.len();
        pdf.push_str(&format!("xref
0 {}
0000000000 65535 f 
", objets.len() + 1));
        for p in positions {
            pdf.push_str(&format!("{p:010} 00000 n 
"));
        }
        pdf.push_str(&format!("trailer
<< /Size {} /Root 1 0 R >>
startxref
{xref}
%%EOF
", objets.len() + 1));
        let chemin = std::env::temp_dir().join("spacefiles-test-galerie.pdf");
        std::fs::write(&chemin, pdf).unwrap();
        let c = chemin.to_string_lossy().replace('/', "\\");
        let s = unsafe { super::image_galerie(&c, "PDF", 0, 1, &mut None) };
        let _ = std::fs::remove_file(&chemin);
        let img = s.image.expect("le lecteur PDF de Windows rend la page");
        assert!(img.largeur >= 1000 && img.hauteur > 0, "{}x{}", img.largeur, img.hauteur);
        assert_eq!((s.page, s.pages), (0, 1));
        let police = unsafe { super::image_galerie(r"C:\Windows\Fonts\arial.ttf", "TTF", 0, 1, &mut None) }.image.expect("un échantillon de la police");
        assert!(police.largeur > 0 && police.pixels.iter().any(|&o| o != 0));
        let t = unsafe { super::texte_image("fn main() {
    println!(\"é\");
}", 1) }.expect("le texte est dessiné");
        assert!(t.pixels.iter().any(|&o| o != 0));
    }

    fn com() {
        unsafe {
            let _ = windows::Win32::System::Com::CoInitializeEx(None, windows::Win32::System::Com::COINIT_APARTMENTTHREADED);
        }
    }

    /// Fichier de test dans un dossier à part (le nom porte le numéro du test : les tests tournent en parallèle)
    fn fichier_test(nom: &str, octets: &[u8]) -> String {
        let dossier = std::env::temp_dir().join("spacefiles-test-pages");
        std::fs::create_dir_all(&dossier).unwrap();
        let chemin = dossier.join(nom);
        std::fs::write(&chemin, octets).unwrap();
        chemin.to_string_lossy().replace('/', "\\")
    }

    /// Un PDF de `n` pages (table xref calculée à la main)
    fn pdf_de(n: usize) -> Vec<u8> {
        let mut objets = vec![String::new(), String::new()];
        let kids: Vec<String> = (0..n).map(|i| format!("{} 0 R", 3 + i * 2)).collect();
        objets[0] = "<< /Type /Catalog /Pages 2 0 R >>".into();
        objets[1] = format!("<< /Type /Pages /Kids [{}] /Count {n} >>", kids.join(" "));
        for i in 0..n {
            objets.push(format!("<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 100] /Contents {} 0 R >>", 4 + i * 2));
            let flux = "0 0 0 rg 20 20 160 60 re f";
            objets.push(format!("<< /Length {} >>\nstream\n{flux}\nendstream", flux.len()));
        }
        let mut pdf = String::from("%PDF-1.4\n");
        let mut positions = Vec::new();
        for (i, o) in objets.iter().enumerate() {
            positions.push(pdf.len());
            pdf.push_str(&format!("{} 0 obj\n{o}\nendobj\n", i + 1));
        }
        let xref = pdf.len();
        pdf.push_str(&format!("xref\n0 {}\n0000000000 65535 f \n", objets.len() + 1));
        for p in positions {
            pdf.push_str(&format!("{p:010} 00000 n \n"));
        }
        pdf.push_str(&format!("trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n", objets.len() + 1));
        pdf.into_bytes()
    }

    #[test]
    fn pdf_de_plusieurs_pages_toutes_atteignables() {
        com();
        let c = fichier_test("trois-pages.pdf", &pdf_de(3));
        for demandee in [0u32, 1, 2, 9] {
            let s = unsafe { super::image_galerie(&c, "PDF", demandee, 1, &mut None) };
            assert!(s.image.is_some(), "page {demandee} rendue");
            assert_eq!(s.pages, 3);
            assert_eq!(s.page, demandee.min(2), "la page demandée est ramenée dans les bornes");
        }
    }

    #[test]
    fn texte_long_en_pages_et_cache() {
        com();
        let texte: String = (0..70).map(|i| format!("ligne numéro {i}\n")).collect();
        let c = fichier_test("long.txt", texte.as_bytes());
        let mut cache = None;
        let s = unsafe { super::image_galerie(&c, "TXT", 0, 1, &mut cache) };
        assert_eq!((s.pages, s.page, s.nature), (3, 0, ""));
        assert!(s.image.is_some() && cache.is_some());
        let s = unsafe { super::image_galerie(&c, "txt", 2, 1, &mut cache) };
        assert_eq!((s.pages, s.page), (3, 2));
        assert!(s.image.expect("dernière page").pixels.iter().any(|&o| o != 0));
        let s = unsafe { super::image_galerie(&c, "TXT", 50, 1, &mut cache) };
        assert_eq!(s.page, 2, "au-delà de la fin : dernière page");
    }

    #[test]
    fn document_docx_texte_extrait_par_tar() {
        com();
        let dossier = std::env::temp_dir().join("spacefiles-test-pages").join("docx-src");
        let _ = std::fs::remove_dir_all(&dossier);
        std::fs::create_dir_all(dossier.join("word")).unwrap();
        let paragraphes: String = (0..40).map(|i| format!("<w:p><w:r><w:t>Paragraphe {i} &amp; suite</w:t></w:r></w:p>")).collect();
        std::fs::write(dossier.join("word").join("document.xml"), format!("<?xml version=\"1.0\"?><w:document><w:body>{paragraphes}</w:body></w:document>")).unwrap();
        let zip = std::env::temp_dir().join("spacefiles-test-pages").join("source.zip");
        let _ = std::fs::remove_file(&zip);
        crate::archive::creer(&zip.to_string_lossy(), &dossier.to_string_lossy(), &["word".to_string()]).expect("tar.exe crée le zip");
        let docx = std::env::temp_dir().join("spacefiles-test-pages").join("lettre.docx");
        std::fs::copy(&zip, &docx).unwrap();
        let c = docx.to_string_lossy().replace('/', "\\");
        let (pages, nature) = crate::pages::pages_de(&c, "DOCX").expect("le texte du document est extrait");
        assert_eq!(nature, "TEXTE EXTRAIT");
        assert_eq!(pages.len(), 2, "40 paragraphes = 26 + 14 lignes");
        assert!(pages[0].starts_with("Paragraphe 0 & suite"), "{}", &pages[0][..30]);
        let s = unsafe { super::image_galerie(&c, "DOCX", 1, 1, &mut None) };
        assert_eq!((s.pages, s.page, s.nature), (2, 1, "TEXTE EXTRAIT"));
        assert!(s.image.is_some());
        // L'archive elle-même se lit comme une liste d'éléments
        let (pages, nature) = crate::pages::pages_de(&zip.to_string_lossy(), "ZIP").expect("liste de l'archive");
        assert_eq!(nature, "CONTENU DE L'ARCHIVE");
        assert!(pages[0].contains("word/document.xml"), "{}", pages[0]);
    }

    #[test]
    fn fichier_inconnu_binaire_en_hexadecimal() {
        com();
        let octets: Vec<u8> = (0..=255u8).cycle().take(2000).collect();
        let c = fichier_test("inconnu.xyz", &octets);
        let s = unsafe { super::image_galerie(&c, "XYZ", 0, 1, &mut None) };
        assert!(s.image.is_some());
        assert_eq!(s.nature, "OCTETS (HEXADECIMAL)");
        assert_eq!(s.pages, 5, "2000 octets = 5 pages de 416 octets");
        // Un fichier texte sans extension connue s'affiche comme du texte, pas en hexadécimal
        let c = fichier_test("notes.zzz", "bonjour\nle monde\n".as_bytes());
        let s = unsafe { super::image_galerie(&c, "ZZZ", 0, 1, &mut None) };
        assert!(s.image.is_some() && s.nature.is_empty() && s.pages == 1, "{:?}", (s.pages, s.nature));
    }
}
