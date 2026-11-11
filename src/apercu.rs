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
use crate::systeme;

/// Aperçu prêt (LPARAM = Box<Apercu>)
pub const WM_APERCU: u32 = WM_APP + 5;
/// Vignette de la grille prête (LPARAM = Box<(chemin, Option<Image>)>)
pub const WM_VIGNETTE: u32 = WM_APP + 10;
/// Image du coup d'œil prête (LPARAM = Box<(numéro, Option<Image>)>)
pub const WM_GALERIE: u32 = WM_APP + 11;

/// Coup d'œil (Espace) : l'élément choisi en grand, ← → pour passer au suivant (galerie photo)
pub struct Galerie {
    pub numero: u64,
    pub nom: String,
    pub detail: String,
    pub icone: Icone,
    pub image: Option<Image>,
    pub charge: bool,
}

/// Demande en attente pour le fil du coup d'œil : seule la plus récente compte (R19)
static GALERIE: (std::sync::Mutex<Option<(isize, u64, String)>>, std::sync::Condvar) = (std::sync::Mutex::new(None), std::sync::Condvar::new());

/// Prépare la grande image du coup d'œil dans un fil séparé. Un seul fil, démarré au premier appel :
/// en parcourant les images aux flèches, les demandes dépassées sont remplacées au lieu d'être décodées.
pub fn charger_galerie(hwnd: HWND, numero: u64, chemin: String) {
    static FIL: std::sync::Once = std::sync::Once::new();
    *GALERIE.0.lock().unwrap_or_else(|e| e.into_inner()) = Some((hwnd.0 as isize, numero, chemin));
    GALERIE.1.notify_one();
    FIL.call_once(|| {
        std::thread::spawn(|| unsafe {
            let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE);
            loop {
                let demande = {
                    let mut g = GALERIE.0.lock().unwrap_or_else(|e| e.into_inner());
                    while g.is_none() {
                        g = GALERIE.1.wait(g).unwrap_or_else(|e| e.into_inner());
                    }
                    g.take()
                };
                let Some((fenetre, numero, chemin)) = demande else { continue };
                let p = Box::into_raw(Box::new((numero, miniature(&chemin, 1600))));
                if PostMessageW(HWND(fenetre as *mut _), WM_GALERIE, WPARAM(0), LPARAM(p as isize)).is_err() {
                    drop(Box::from_raw(p));
                }
            }
        });
    });
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
    let mut a = Apercu { numero, nom, icone: e.map_or(Icone::Dossier, icones::pour), image: None, texte: None, infos: Vec::new(), teintes: Vec::new() };
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
                if let Some((img, pages)) = pdf(chemin) {
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
        let u: Vec<u16> = o.get(chaines + dep..chaines + dep + len)?.chunks_exact(2).map(|c| u16::from_be_bytes([c[0], c[1]])).collect();
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
            for p in img.pixels.chunks_exact_mut(4) {
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

/// Première page d'un PDF (lecteur PDF de Windows), et le nombre de pages
fn pdf(chemin: &str) -> Option<(Image, u32)> {
    use windows::core::HSTRING;
    use windows::Data::Pdf::{PdfDocument, PdfPageRenderOptions};
    use windows::Graphics::Imaging::{BitmapAlphaMode, BitmapDecoder, BitmapPixelFormat};
    use windows::Storage::Streams::{Buffer, DataReader, InMemoryRandomAccessStream};
    use windows::Storage::StorageFile;
    let fichier = StorageFile::GetFileFromPathAsync(&HSTRING::from(chemin)).ok()?.get().ok()?;
    let doc = PdfDocument::LoadFromFileAsync(&fichier).ok()?.get().ok()?;
    let pages = doc.PageCount().ok()?;
    let page = doc.GetPage(0).ok()?;
    let options = PdfPageRenderOptions::new().ok()?;
    options.SetDestinationWidth(520).ok()?;
    let flux = InMemoryRandomAccessStream::new().ok()?;
    page.RenderWithOptionsToStreamAsync(&flux, &options).ok()?.get().ok()?;
    flux.Seek(0).ok()?;
    let decodeur = BitmapDecoder::CreateAsync(&flux).ok()?.get().ok()?;
    let image = decodeur.GetSoftwareBitmapConvertedAsync(BitmapPixelFormat::Bgra8, BitmapAlphaMode::Premultiplied).ok()?.get().ok()?;
    let (l, h) = (image.PixelWidth().ok()? as u32, image.PixelHeight().ok()? as u32);
    if l == 0 || h == 0 {
        return None;
    }
    let tampon = Buffer::Create(l * h * 4).ok()?;
    image.CopyToBuffer(&tampon).ok()?;
    let mut pixels = vec![0u8; (l * h * 4) as usize];
    DataReader::FromBuffer(&tampon).ok()?.ReadBytes(&mut pixels).ok()?;
    Some((Image { largeur: l, hauteur: h, pixels }, pages))
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
            if suite.starts_with("**") {
                if let Some(f) = suite[2..].find("**") {
                    special = Some((suite[2..2 + f].to_string(), Teinte::Gras, 4 + suite[2..2 + f].chars().count()));
                }
            } else if suite.starts_with('`') {
                if let Some(f) = suite[1..].find('`') {
                    special = Some((suite[1..1 + f].to_string(), Teinte::Chaine, 2 + suite[1..1 + f].chars().count()));
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
    if px.chunks_exact(4).all(|p| p[3] == 0) {
        for p in px.chunks_exact_mut(4) {
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
        let u: Vec<u16> = buf[2..].chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
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
    #[test]
    fn famille_d_une_police_et_chemin_court() {
        let o = std::fs::read(r"C:\Windows\Fonts\arial.ttf").expect("Arial est livrée avec Windows");
        assert_eq!(super::famille_police(&o).as_deref(), Some("Arial"));
        assert_eq!(super::famille_police(&o[..40]), None);
        assert_eq!(super::court("a/b.txt"), "a/b.txt");
        let c = super::court(&format!("{}/fin.txt", "dossier/".repeat(10)));
        assert!(c.starts_with('…') && c.ends_with("fin.txt") && c.chars().count() == 34);
    }
}
