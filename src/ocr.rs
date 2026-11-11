// Lecture du texte d'une image ou d'un PDF scanné (Z9) par la reconnaissance de texte de Windows (Windows.Media.Ocr) :
// rien à installer, les langues sont celles du profil de l'utilisateur. Le résultat est écrit en .txt par l'appelant.

use std::sync::atomic::{AtomicBool, Ordering};
use windows::core::HSTRING;
use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::System::Com::{CoInitializeEx, CoUninitialize, COINIT_MULTITHREADED};
use windows::Win32::UI::WindowsAndMessaging::{PostMessageW, WM_APP};
use windows::Data::Pdf::{PdfDocument, PdfPageRenderOptions};
use windows::Graphics::Imaging::{BitmapAlphaMode, BitmapDecoder, BitmapPixelFormat, SoftwareBitmap};
use windows::Media::Ocr::OcrEngine;
use windows::core::Interface;
use windows::Storage::Streams::{IRandomAccessStream, InMemoryRandomAccessStream};
use windows::Storage::{FileAccessMode, StorageFile};

/// Pages d'un PDF lues au plus : chaque page prend de 0,3 à 1 s, et la fenêtre attend
pub const MAX_PAGES: u32 = 5;
/// Largeur de rendu d'une page de PDF : assez fine pour de petits caractères, sans gonfler la mémoire
const LARGEUR_PAGE: u32 = 1800;

pub fn est_lisible(extension: &str) -> bool {
    matches!(extension.to_ascii_uppercase().as_str(), "PNG" | "JPG" | "JPEG" | "BMP" | "GIF" | "TIF" | "TIFF" | "PDF")
}

/// Texte reconnu (une page après l'autre pour un PDF), ou la raison de l'échec
pub fn lire(chemin: &str) -> Result<String, String> {
    let moteur = OcrEngine::TryCreateFromUserProfileLanguages().map_err(|_| "AUCUNE LANGUE DE RECONNAISSANCE DISPONIBLE (PARAMETRES WINDOWS > LANGUE)".to_string())?;
    let erreur = |_| "FICHIER ILLISIBLE".to_string();
    let fichier = StorageFile::GetFileFromPathAsync(&HSTRING::from(chemin)).and_then(|o| o.get()).map_err(erreur)?;
    let mut texte = String::new();
    if chemin.to_ascii_lowercase().ends_with(".pdf") {
        let doc = PdfDocument::LoadFromFileAsync(&fichier).and_then(|o| o.get()).map_err(erreur)?;
        let options = PdfPageRenderOptions::new().map_err(erreur)?;
        options.SetDestinationWidth(LARGEUR_PAGE).map_err(erreur)?;
        for k in 0..doc.PageCount().map_err(erreur)?.min(MAX_PAGES) {
            let flux = InMemoryRandomAccessStream::new().map_err(erreur)?;
            doc.GetPage(k).and_then(|p| p.RenderWithOptionsToStreamAsync(&flux, &options)).and_then(|o| o.get()).map_err(erreur)?;
            flux.Seek(0).map_err(erreur)?;
            texte += &page(&moteur, &flux.cast().map_err(erreur)?)?;
            texte += "\r\n";
        }
    } else {
        let flux = fichier.OpenAsync(FileAccessMode::Read).and_then(|o| o.get()).map_err(erreur)?;
        texte = page(&moteur, &flux.cast().map_err(erreur)?)?;
    }
    Ok(texte.trim().to_string())
}

/// Reconnaît le texte d'une image décodée depuis ce flux
fn page(moteur: &OcrEngine, flux: &IRandomAccessStream) -> Result<String, String> {
    let erreur = |_| "IMAGE ILLISIBLE".to_string();
    let decodeur = BitmapDecoder::CreateAsync(flux).and_then(|o| o.get()).map_err(erreur)?;
    let image: SoftwareBitmap = decodeur.GetSoftwareBitmapConvertedAsync(BitmapPixelFormat::Bgra8, BitmapAlphaMode::Premultiplied).and_then(|o| o.get()).map_err(erreur)?;
    // Le moteur refuse les images plus grandes que MaxImageDimension : mieux vaut le dire que lire de travers
    let max = OcrEngine::MaxImageDimension().unwrap_or(2600);
    if image.PixelWidth().unwrap_or(0) as u32 > max || image.PixelHeight().unwrap_or(0) as u32 > max {
        return Err(format!("IMAGE TROP GRANDE POUR LA RECONNAISSANCE (PLUS DE {max} POINTS)"));
    }
    let r = moteur.RecognizeAsync(&image).and_then(|o| o.get()).map_err(|_| "RECONNAISSANCE ECHOUEE".to_string())?;
    Ok(r.Text().map(|t| t.to_string()).unwrap_or_default())
}

/// Lecture terminée sur le fil d'arrière-plan (LPARAM = Box<Resultat>)
pub const WM_OCR: u32 = WM_APP + 13;

/// Ce que le fil renvoie à la fenêtre
pub struct Resultat {
    pub source: String,
    pub texte: Result<String, String>,
    pub duree_ms: u128,
}

/// Une lecture est en cours : pas de seconde en même temps (chaque page coûte 0,3 à 1 s de processeur).
/// Le drapeau est global au processus, donc partagé par toutes les fenêtres, et c'est voulu : le coût est celui du
/// processeur de la machine, pas d'une fenêtre ; deux lectures en parallèle (une par fenêtre) le doubleraient.
/// Conséquence : la fenêtre B peut refuser pendant que A lit, d'où le message qui parle d'une autre fenêtre.
static EN_COURS: AtomicBool = AtomicBool::new(false);

/// Réserve la place de la lecture ; faux si une autre tourne déjà
pub fn reserver() -> bool {
    EN_COURS.compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire).is_ok()
}

/// Libère la place : à appeler à l'arrivée du résultat, erreur comprise
pub fn liberer() {
    EN_COURS.store(false, Ordering::Release);
}

/// Réception du résultat par la fenêtre : libère la place, qu'il s'agisse d'un texte ou d'une erreur
pub fn recevoir(r: Box<Resultat>) -> Resultat {
    liberer();
    *r
}

/// Lit ce fichier dans un fil ; le résultat revient à la fenêtre par WM_OCR. Suppose `reserver()` réussi.
pub fn lancer(hwnd: HWND, chemin: String) {
    let fenetre = hwnd.0 as isize;
    std::thread::spawn(move || {
        let debut = std::time::Instant::now();
        // Windows.Media.Ocr et PDF attendent leurs opérations asynchrones : le fil a besoin de COM
        // SAFETY: appariés sur ce fil ; aucun objet WinRT ne survit à `lire`.
        let initialise = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) }.is_ok();
        let texte = lire(&chemin);
        if initialise {
            unsafe { CoUninitialize() };
        }
        let r = Box::new(Resultat { source: chemin, texte, duree_ms: debut.elapsed().as_millis() });
        let p = Box::into_raw(r);
        // SAFETY: `p` vient de Box::into_raw ; la fenêtre le libère à la réception, sinon on le libère ici
        // (fenêtre fermée : le post échoue).
        unsafe {
            if PostMessageW(HWND(fenetre as *mut _), WM_OCR, WPARAM(0), LPARAM(p as isize)).is_err() {
                drop(Box::from_raw(p));
                liberer();
            }
        }
    });
}

/// Ce que la fenêtre fait d'un résultat de lecture
#[derive(Debug, PartialEq)]
pub enum Suite {
    Vide,
    /// Nom du fichier .ocr.txt à écrire, son contenu
    Ecrire(String, String),
    Erreur(String),
}

/// Décision sans Win32 : texte vide, texte à écrire sous `<nom>.ocr.txt`, ou erreur
pub fn suite(source: &str, texte: Result<String, String>) -> Suite {
    match texte {
        Ok(t) if t.is_empty() => Suite::Vide,
        Ok(t) => {
            let nom = source.rsplit('\\').next().unwrap_or(source);
            Suite::Ecrire(format!("{nom}.ocr.txt"), t)
        }
        Err(e) => Suite::Erreur(e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn suite_de_lecture() {
        assert_eq!(suite("C:\\a\\scan.png", Ok(String::new())), Suite::Vide);
        assert_eq!(suite("C:\\a\\scan.png", Ok("bonjour".into())), Suite::Ecrire("scan.png.ocr.txt".into(), "bonjour".into()));
        assert_eq!(suite("scan.pdf", Err("X".into())), Suite::Erreur("X".into()));
    }

    /// Un seul test touche au drapeau global : deux tests en parallèle se le disputeraient
    #[test]
    fn une_seule_lecture_a_la_fois() {
        assert!(reserver());
        assert!(!reserver());
        liberer();
        assert!(reserver());
        liberer();
        // Le drapeau est libéré à l'arrivée du résultat, y compris d'un échec (sans OCR réel : un `Resultat` construit à la main)
        for texte in [Ok("bonjour".to_string()), Err("RECONNAISSANCE ECHOUEE".to_string())] {
            assert!(reserver());
            let r = recevoir(Box::new(Resultat { source: "C:\\a\\scan.png".into(), texte: texte.clone(), duree_ms: 1 }));
            assert_eq!(r.texte, texte);
            assert!(reserver(), "la place doit être libre après le résultat");
            liberer();
        }
    }

    #[test]
    fn extensions_lisibles() {
        assert!(est_lisible("png") && est_lisible("PDF") && est_lisible("Jpeg"));
        assert!(!est_lisible("txt") && !est_lisible(""));
    }
}
