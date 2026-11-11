// Actions en lot sur les images, par WIC (le décodeur de Windows : HEIC et WebP s'ils sont installés).
// Chaque image donne un nouveau fichier à côté de l'original, jamais écrasé ; aucune métadonnée n'est
// recopiée (lieu, appareil…), l'orientation EXIF est appliquée aux pixels pour que l'image reste droite.

use windows::core::{GUID, HSTRING};
use windows::Win32::Foundation::GENERIC_READ;
use windows::Win32::Graphics::Imaging::*;
use windows::Win32::System::Com::*;
use windows::core::PROPVARIANT;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Traitement {
    Jpg,
    Png,
    /// Plus grand côté ramené à ce nombre de pixels (jamais agrandi)
    Reduire(u32),
    /// Même format, sans métadonnées
    SansMeta,
}

impl Traitement {
    pub fn libelle(self) -> String {
        match self {
            Traitement::Jpg => "CONVERSION EN JPG".into(),
            Traitement::Png => "CONVERSION EN PNG".into(),
            Traitement::Reduire(n) => format!("REDUCTION A {n} PX"),
            Traitement::SansMeta => "RETRAIT DES METADONNEES".into(),
        }
    }
}

/// Images que WIC sait lire
pub fn est_image(ext: &str) -> bool {
    matches!(ext.to_ascii_lowercase().as_str(), "jpg" | "jpeg" | "jfif" | "png" | "bmp" | "gif" | "tif" | "tiff" | "heic" | "heif" | "webp")
}

/// Traite toutes les images ; renvoie (réussies, première erreur)
pub fn traiter_tout(chemins: &[String], t: Traitement) -> (usize, Option<String>) {
    unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
    }
    let mut n = 0;
    let mut erreur = None;
    for c in chemins {
        match traiter(c, t) {
            Ok(_) => n += 1,
            Err(e) => {
                erreur.get_or_insert(format!("{} : {e}", c.rsplit('\\').next().unwrap_or(c).to_uppercase()));
            }
        }
    }
    (n, erreur)
}

/// Format de sortie : (conteneur WIC, format de pixels, extension)
fn sortie(t: Traitement, ext: &str) -> (GUID, GUID, &'static str) {
    let jpg = (GUID_ContainerFormatJpeg, GUID_WICPixelFormat24bppBGR, "jpg");
    let png = (GUID_ContainerFormatPng, GUID_WICPixelFormat32bppBGRA, "png");
    match t {
        Traitement::Jpg => jpg,
        Traitement::Png => png,
        // Garder le format quand on sait l'écrire ; sinon JPG pour les photos (HEIC, WebP), PNG pour le reste
        _ => match ext.to_ascii_lowercase().as_str() {
            "jpg" | "jpeg" | "jfif" | "heic" | "heif" | "webp" => jpg,
            "bmp" => (GUID_ContainerFormatBmp, GUID_WICPixelFormat24bppBGR, "bmp"),
            "tif" | "tiff" => (GUID_ContainerFormatTiff, GUID_WICPixelFormat32bppBGRA, "tif"),
            _ => png,
        },
    }
}

/// Nom libre à côté de l'original : « photo (1920px).jpg », « photo (1920px) 2.jpg »…
/// Premier nom libre, réservé par un fichier vide créé en exclusif : `InitializeFromFilename` écraserait
/// un fichier apparu entre le choix du nom et l'écriture (R18)
fn reserver(dossier: &str, base: &str, ext: &str) -> std::io::Result<String> {
    loop {
        let c = nom_libre(dossier, base, ext);
        match std::fs::OpenOptions::new().write(true).create_new(true).open(&c) {
            Ok(_) => return Ok(c),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e),
        }
    }
}

fn nom_libre(dossier: &str, base: &str, ext: &str) -> String {
    let mut k = 1;
    loop {
        let nom = if k == 1 { format!("{base}.{ext}") } else { format!("{base} {k}.{ext}") };
        let c = crate::dossier::joindre(dossier, &nom);
        if !std::path::Path::new(&c).exists() {
            return c;
        }
        k += 1;
    }
}

/// Orientation EXIF (1 à 8) de l'image, 1 si absente
unsafe fn orientation(cadre: &IWICBitmapFrameDecode) -> u16 {
    let Ok(lecteur) = cadre.GetMetadataQueryReader() else { return 1 };
    for nom in ["/app1/ifd/{ushort=274}", "/ifd/{ushort=274}"] {
        let mut v = PROPVARIANT::default();
        if lecteur.GetMetadataByName(&HSTRING::from(nom), &mut v).is_ok() {
            if let Ok(o) = u16::try_from(&v) {
                return o;
            }
        }
    }
    1
}

/// Traite une image ; renvoie le chemin du fichier créé
pub fn traiter(chemin: &str, t: Traitement) -> Result<String, String> {
    let (dossier, nom) = chemin.rsplit_once('\\').ok_or("CHEMIN INVALIDE")?;
    let (stem, ext) = nom.rsplit_once('.').unwrap_or((nom, ""));
    let (conteneur, pixels, ext_sortie) = sortie(t, ext);
    if t == Traitement::Jpg && matches!(ext.to_ascii_lowercase().as_str(), "jpg" | "jpeg") || t == Traitement::Png && ext.eq_ignore_ascii_case("png") {
        return Err("DEJA DANS CE FORMAT".into());
    }
    unsafe {
        let usine: IWICImagingFactory = CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER).map_err(|e| e.message())?;
        let decodeur = usine
            .CreateDecoderFromFilename(&HSTRING::from(chemin), None, GENERIC_READ, WICDecodeMetadataCacheOnDemand)
            .map_err(|_| "FORMAT NON PRIS EN CHARGE".to_string())?;
        let cadre = decodeur.GetFrame(0).map_err(|e| e.message())?;
        let mut source: IWICBitmapSource = windows::core::Interface::cast(&cadre).map_err(|e: windows::core::Error| e.message())?;

        // Orientation EXIF appliquée aux pixels (les métadonnées ne suivent pas)
        let transfo = match orientation(&cadre) {
            2 => Some(WICBitmapTransformFlipHorizontal),
            3 => Some(WICBitmapTransformRotate180),
            4 => Some(WICBitmapTransformFlipVertical),
            5 => Some(WICBitmapTransformOptions(WICBitmapTransformRotate90.0 | WICBitmapTransformFlipHorizontal.0)),
            6 => Some(WICBitmapTransformRotate90),
            7 => Some(WICBitmapTransformOptions(WICBitmapTransformRotate270.0 | WICBitmapTransformFlipHorizontal.0)),
            8 => Some(WICBitmapTransformRotate270),
            _ => None,
        };
        if let Some(o) = transfo {
            let r = usine.CreateBitmapFlipRotator().map_err(|e| e.message())?;
            r.Initialize(&source, o).map_err(|e| e.message())?;
            source = windows::core::Interface::cast(&r).map_err(|e: windows::core::Error| e.message())?;
        }

        let (mut l, mut h) = (0u32, 0u32);
        source.GetSize(&mut l, &mut h).map_err(|e| e.message())?;
        let mut suffixe = String::new();
        if let Traitement::Reduire(max) = t {
            if l.max(h) <= max {
                return Err("DEJA ASSEZ PETITE".into());
            }
            let k = max as f64 / l.max(h) as f64;
            let (nl, nh) = (((l as f64 * k).round() as u32).max(1), ((h as f64 * k).round() as u32).max(1));
            let s = usine.CreateBitmapScaler().map_err(|e| e.message())?;
            s.Initialize(&source, nl, nh, WICBitmapInterpolationModeHighQualityCubic).map_err(|e| e.message())?;
            source = windows::core::Interface::cast(&s).map_err(|e: windows::core::Error| e.message())?;
            (l, h) = (nl, nh);
            suffixe = format!(" ({max}px)");
        } else if t == Traitement::SansMeta {
            suffixe = " (sans metadonnees)".into();
        }

        let conv = usine.CreateFormatConverter().map_err(|e| e.message())?;
        conv.Initialize(&source, &pixels, WICBitmapDitherTypeNone, None, 0.0, WICBitmapPaletteTypeMedianCut).map_err(|e| e.message())?;

        let cible = reserver(dossier, &format!("{stem}{suffixe}"), ext_sortie).map_err(|e| e.to_string())?;
        let flux = usine.CreateStream().map_err(|e| e.message())?;
        if let Err(e) = flux.InitializeFromFilename(&HSTRING::from(cible.as_str()), windows::Win32::Foundation::GENERIC_WRITE.0) {
            let _ = std::fs::remove_file(&cible);
            return Err(e.message());
        }
        let ecrire = || -> windows::core::Result<()> {
            let encodeur = usine.CreateEncoder(&conteneur, std::ptr::null())?;
            encodeur.Initialize(&flux, WICBitmapEncoderNoCache)?;
            let mut sortie = None;
            let mut options = None;
            encodeur.CreateNewFrame(&mut sortie, &mut options)?;
            let sortie = sortie.ok_or_else(windows::core::Error::empty)?;
            sortie.Initialize(options.as_ref())?;
            sortie.SetSize(l, h)?;
            let mut p = pixels;
            sortie.SetPixelFormat(&mut p)?;
            sortie.WriteSource(&conv, std::ptr::null())?;
            sortie.Commit()?;
            encodeur.Commit()
        };
        let r = ecrire();
        drop(flux);
        if let Err(e) = r {
            let _ = std::fs::remove_file(&cible);
            return Err(e.message());
        }
        Ok(cible)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Huit fils réservent « photo.png » en même temps : huit noms différents, aucun fichier écrasé
    #[test]
    fn reservation_concurrente() {
        let d = std::env::temp_dir().join(format!("ne-reserver-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        let dossier = d.to_str().unwrap().to_string();
        let fils: Vec<_> = (0..8).map(|_| { let dossier = dossier.clone(); std::thread::spawn(move || reserver(&dossier, "photo", "png").unwrap()) }).collect();
        let mut noms: Vec<String> = fils.into_iter().map(|f| f.join().unwrap()).collect();
        noms.sort();
        noms.dedup();
        assert_eq!(noms.len(), 8);
        assert_eq!(std::fs::read_dir(&d).unwrap().count(), 8);
        let _ = std::fs::remove_dir_all(&d);
    }
}
