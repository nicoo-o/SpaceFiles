// Icônes en matrice de points : un motif par type de fichier
//
// Chaque motif est une grille de 7 colonnes au plus : `#` point blanc, `r` point rouge
// (accent Nothing, blanc quand la ligne est sélectionnée), `.` vide.

use crate::dossier::{Entree, Genre};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Icone {
    Dossier,
    Disque,
    Image,
    Musique,
    Video,
    Code,
    Archive,
    Executable,
    Document,
    Pdf,
    Tableur,
    Texte,
    Raccourci,
    Fichier,
    Maison,
    Bureau,
    Telechargement,
    Nuage,
    Serveur,
    Reglages,
    Horloge,
}

pub const TOUTES: [Icone; 21] = [
    Icone::Dossier,
    Icone::Disque,
    Icone::Image,
    Icone::Musique,
    Icone::Video,
    Icone::Code,
    Icone::Archive,
    Icone::Executable,
    Icone::Document,
    Icone::Pdf,
    Icone::Tableur,
    Icone::Texte,
    Icone::Raccourci,
    Icone::Fichier,
    Icone::Maison,
    Icone::Bureau,
    Icone::Telechargement,
    Icone::Nuage,
    Icone::Serveur,
    Icone::Reglages,
    Icone::Horloge,
];

impl Icone {
    pub fn index(self) -> usize {
        TOUTES.iter().position(|&i| i == self).unwrap_or(0)
    }

    pub fn motif(self) -> &'static [&'static str] {
        match self {
            Icone::Dossier => &["###....", "#..####", "#.....#", "#.....#", "#.....#", "#######"],
            Icone::Disque => &["#######", "#.....#", "#.....#", "#######", "#...r.#", "#######"],
            Icone::Image => &["#######", "#....r#", "#.....#", "#..#..#", "#.###.#", "#######"],
            Icone::Musique => &["..#####", "..#...#", "..#...#", "..#...#", "###.###", "###.###"],
            Icone::Video => &["#######", "#.r...#", "#.rr..#", "#.rrr.#", "#.rr..#", "#.r...#", "#######"],
            Icone::Code => &[".#...#.", "#..r..#", ".#...#."],
            Icone::Archive => &["#######", "#..#..#", "#..r..#", "#..#..#", "#..r..#", "#######"],
            Icone::Executable => &["#######", "#######", "#.....#", "#..r..#", "#.....#", "#######"],
            Icone::Document => &["####...", "#..##..", "#....#.", "#.##.#.", "#....#.", "#.##.#.", "######."],
            Icone::Pdf => &["####...", "#..##..", "#....#.", "#.rr.#.", "#.rr.#.", "#....#.", "######."],
            Icone::Tableur => &["#######", "#..#..#", "#######", "#..#..#", "#######"],
            Icone::Texte => &["#####", ".....", "####.", ".....", "###.."],
            Icone::Raccourci => &["..####", "....##", "...#.#", "..#..#", ".#....", "#....."],
            Icone::Fichier => &["###..", "#..#.", "#...#", "#...#", "#...#", "#####"],
            Icone::Maison => &["...#...", "..#.#..", ".#...#.", "#.....#", ".#.r.#.", ".#####."],
            Icone::Bureau => &["#######", "#.....#", "#.....#", "#######", "...#...", ".#####."],
            Icone::Telechargement => &["...#...", "...#...", ".#.#.#.", "..###..", "...r...", "#######"],
            Icone::Nuage => &["...##..", "..#..#.", ".##...#", "#.....#", "#..r..#", ".#####."],
            Icone::Serveur => &["#######", "#.r...#", "#######", "#.r...#", "#######"],
            Icone::Horloge => &[".#####.", "#..#..#", "#..#..#", "#..rr.#", "#.....#", "#.....#", ".#####."],
            Icone::Reglages => &["...#...", ".#.#.#.", "..#.#..", "##.r.##", "..#.#..", ".#.#.#.", "...#..."],
        }
    }
}

pub fn pour_extension(ext: &str) -> Icone {
    match ext {
        "JPG" | "JPEG" | "PNG" | "GIF" | "BMP" | "WEBP" | "TIF" | "TIFF" | "HEIC" | "AVIF" | "ICO" | "SVG" | "PSD" | "RAW" | "CR2" | "NEF" | "DNG" => Icone::Image,
        "MP3" | "FLAC" | "WAV" | "OGG" | "M4A" | "AAC" | "WMA" | "OPUS" | "AIFF" | "MID" | "MIDI" => Icone::Musique,
        "MP4" | "MKV" | "AVI" | "MOV" | "WMV" | "WEBM" | "FLV" | "M4V" | "MPG" | "MPEG" | "TS" | "M2TS" => Icone::Video,
        "RS" | "C" | "CPP" | "CC" | "H" | "HPP" | "CS" | "JS" | "MJS" | "JSX" | "TSX" | "PY" | "JAVA" | "GO" | "RB" | "PHP" | "HTML" | "HTM" | "CSS" | "SCSS"
        | "JSON" | "XML" | "YAML" | "YML" | "TOML" | "SH" | "PS1" | "PSM1" | "BAT" | "CMD" | "LUA" | "KT" | "SWIFT" | "SQL" | "MD" | "INI" | "CFG" | "CONF"
        | "VUE" | "DART" | "ZIG" => Icone::Code,
        "ZIP" | "RAR" | "7Z" | "TAR" | "GZ" | "TGZ" | "BZ2" | "XZ" | "ZST" | "ISO" | "CAB" | "IMG" => Icone::Archive,
        "EXE" | "MSI" | "APPX" | "MSIX" | "APPXBUNDLE" | "MSIXBUNDLE" | "COM" | "SCR" | "DLL" | "SYS" => Icone::Executable,
        "DOC" | "DOCX" | "ODT" | "RTF" | "PPT" | "PPTX" | "ODP" | "EPUB" => Icone::Document,
        "PDF" => Icone::Pdf,
        "XLS" | "XLSX" | "XLSM" | "CSV" | "ODS" => Icone::Tableur,
        "TXT" | "LOG" | "NFO" => Icone::Texte,
        "LNK" | "URL" => Icone::Raccourci,
        _ => Icone::Fichier,
    }
}

/// Miniature colorée d'un type de fichier sans vignette réelle (`TH` de la maquette v6). Les autres types gardent leurs points.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Pastille {
    /// Dégradé à 135° : bleu `#1e5fa8` jusqu'à 55 %, puis ambre `#e8a66b`
    Png,
    /// Fond `#3a3f4b`, bord intérieur de 5 px `#7fb3f0`
    Exe,
    /// Fond `#e5e5e5`, bandeau haut de 5 px `#c94a3a`
    Pdf,
    /// Fond `#5a5f6b`, bandeau haut de 5 px `#e8a66b`
    Zip,
}

/// Épaisseur du bord (exe) ou du bandeau (pdf, zip), en DIP : 5 px dans la maquette, quelle que soit la taille de la pastille
pub const PASTILLE_BORD: f32 = 5.0;

impl Pastille {
    /// Couleur de fond (pour `Png` : le bleu de départ) en RVB
    pub fn fond(self) -> [u8; 3] {
        match self {
            Pastille::Png => [0x1e, 0x5f, 0xa8],
            Pastille::Exe => [0x3a, 0x3f, 0x4b],
            Pastille::Pdf => [0xe5, 0xe5, 0xe5],
            Pastille::Zip => [0x5a, 0x5f, 0x6b],
        }
    }

    /// Couleur du bord intérieur ou du bandeau (pour `Png` : l'ambre d'arrivée) en RVB
    pub fn accent(self) -> [u8; 3] {
        match self {
            Pastille::Png => [0xe8, 0xa6, 0x6b],
            Pastille::Exe => [0x7f, 0xb3, 0xf0],
            Pastille::Pdf => [0xc9, 0x4a, 0x3a],
            Pastille::Zip => [0xe8, 0xa6, 0x6b],
        }
    }
}

/// Pastille d'une extension (majuscules ou minuscules, sans le point) ; `None` pour tout autre type
pub fn pastille_pour(extension: &str) -> Option<Pastille> {
    match extension.to_ascii_lowercase().as_str() {
        "png" => Some(Pastille::Png),
        "exe" => Some(Pastille::Exe),
        "pdf" => Some(Pastille::Pdf),
        "zip" => Some(Pastille::Zip),
        _ => None,
    }
}

/// Pastille d'une entrée : seulement un fichier (un dossier nommé « x.png » reste un dossier)
pub fn pastille_de(e: &Entree) -> Option<Pastille> {
    match e.genre {
        Genre::Fichier => pastille_pour(&e.ext),
        _ => None,
    }
}

pub fn pour(e: &Entree) -> Icone {
    match e.genre {
        Genre::Dossier => Icone::Dossier,
        Genre::Disque { .. } => Icone::Disque,
        Genre::Fichier => pour_extension(&e.ext),
    }
}

/// Nom de type lisible pour la colonne TYPE
pub fn nom_type(e: &Entree) -> &str {
    match pour(e) {
        Icone::Dossier => "DOSSIER",
        Icone::Disque => "DISQUE",
        _ if e.ext.is_empty() => "FICHIER",
        _ => &e.ext,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pastilles_des_quatre_types_quelle_que_soit_la_casse() {
        for (ext, p) in [("png", Pastille::Png), ("PNG", Pastille::Png), ("Png", Pastille::Png), ("exe", Pastille::Exe), ("EXE", Pastille::Exe), ("pdf", Pastille::Pdf), ("PDF", Pastille::Pdf), ("zip", Pastille::Zip), ("ZIP", Pastille::Zip)] {
            assert_eq!(pastille_pour(ext), Some(p), "{ext}");
        }
    }

    #[test]
    fn autres_types_et_dossiers_sans_pastille() {
        for ext in ["txt", "jpg", "rs", "", "7z", "pngg"] {
            assert_eq!(pastille_pour(ext), None, "{ext}");
        }
        // Un dossier nommé « photos.png » garde son extension en majuscules dans `ext`, mais reste un dossier
        let mut dossier = crate::dossier::entree_dossier("photos.png", "C:\\");
        dossier.ext = "PNG".into();
        assert_eq!(pastille_de(&dossier), None);
        let fichier = crate::dossier::entree_fichier("a.png", 1, 0, false);
        assert_eq!(fichier.ext, "PNG");
        assert_eq!(pastille_de(&fichier), Some(Pastille::Png));
    }

    #[test]
    fn couleurs_de_la_maquette() {
        assert_eq!(Pastille::Png.fond(), [0x1e, 0x5f, 0xa8]);
        assert_eq!(Pastille::Png.accent(), [0xe8, 0xa6, 0x6b]);
        assert_eq!(Pastille::Exe.accent(), [0x7f, 0xb3, 0xf0]);
        assert_eq!(Pastille::Pdf.accent(), [0xc9, 0x4a, 0x3a]);
        assert_eq!(Pastille::Zip.fond(), [0x5a, 0x5f, 0x6b]);
    }
}
