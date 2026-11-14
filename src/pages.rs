// Pages du coup d'œil : tout ce qui se lit page par page (texte et code longs, documents Office et OpenDocument, RTF, contenu d'une
// archive, octets en hexadécimal, images à plusieurs trames comme un TIFF ou un GIF). Le PDF est rendu par apercu.rs.
// Les fonctions de ce fichier sont pures (testées sans fenêtre) sauf `trame` (décodeur d'images de Windows) et `membres_xml`
// (tar.exe, qui lit aussi les .docx / .xlsx / .pptx : ce sont des zip).

use std::io::{Read, Seek, SeekFrom};

use crate::apercu::Image;

/// Lignes par page : celles que `apercu::texte_image` sait dessiner
pub const LIGNES_PAGE: usize = 26;
/// Largeur d'une page de texte, en colonnes (`texte_image` coupe au-delà de 110)
const COLONNES: usize = 108;
/// Au plus 400 pages de texte : le coup d'œil n'est pas un éditeur
const MAX_LIGNES: usize = LIGNES_PAGE * 400;
/// Texte lu au plus : 2 Mo
const MAX_OCTETS: usize = 2 << 20;
/// Hexadécimal : 16 octets par ligne, les 64 premiers Ko
const OCTETS_LIGNE: usize = 16;
const OCTETS_PAGE: usize = OCTETS_LIGNE * LIGNES_PAGE;
const MAX_HEXA: u64 = 64 << 10;
/// Un document Office plus gros n'est pas ouvert pour un simple coup d'œil
const MAX_DOCUMENT: u64 = 80 << 20;
/// Côté maximal d'une trame décodée pour le coup d'œil (la carte n'en montre pas plus)
pub const MAX_COTE: u32 = 2048;

/// Formats d'images qui peuvent avoir plusieurs trames (pages d'un TIFF, images d'un GIF ou d'une icône)
pub const IMAGES_MULTI: &[&str] = &["TIF", "TIFF", "GIF", "ICO"];

/// Extensions de documents dont le texte est extrait (zip de XML, ou RTF)
pub fn est_document(ext: &str) -> bool {
    matches!(ext, "DOCX" | "DOCM" | "DOTX" | "ODT" | "OTT" | "ODS" | "OTS" | "ODP" | "OTP" | "PPTX" | "PPSX" | "PPTM" | "XLSX" | "XLSM" | "RTF")
}

/// Pages de texte d'un fichier et ce qu'elles montrent (« » pour le texte lui-même), selon son extension :
/// archive (liste des éléments), document (texte extrait), sinon le fichier lu comme texte s'il en est un.
pub fn pages_de(chemin: &str, ext: &str) -> Option<(Vec<String>, &'static str)> {
    let (sections, nature) = if crate::archive::est_archive(ext) {
        // Une archive tar compressée se décompresse en entier pour être listée, sans annulation possible : bornée comme dans le volet
        let tar_compresse = matches!(ext, "TAR" | "GZ" | "TGZ" | "BZ2" | "TBZ2" | "XZ" | "TXZ" | "ZST");
        if tar_compresse && std::fs::metadata(chemin).ok()?.len() > 200 << 20 {
            return None;
        }
        (vec![liste_archive(chemin)?], "CONTENU DE L'ARCHIVE")
    } else if est_document(ext) {
        (document(chemin, ext)?, "TEXTE EXTRAIT")
    } else {
        (vec![lire_texte(chemin, MAX_OCTETS)?.lines().map(str::to_string).collect()], "")
    };
    let pages = paginer(sections);
    if pages.is_empty() {
        None
    } else {
        Some((pages, nature))
    }
}

/// Lit le début d'un fichier comme texte (UTF-8, UTF-16 ou ANSI) ; rien s'il est binaire
pub fn lire_texte(chemin: &str, max: usize) -> Option<String> {
    let mut f = std::fs::File::open(chemin).ok()?;
    let mut buf = Vec::new();
    f.by_ref().take(max as u64).read_to_end(&mut buf).ok()?;
    decoder(&buf)
}

fn decoder(buf: &[u8]) -> Option<String> {
    // UTF-16 (fichiers .reg, journaux Windows)
    if buf.starts_with(&[0xFF, 0xFE]) {
        let u: Vec<u16> = buf[2..].as_chunks::<2>().0.iter().map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
        return Some(String::from_utf16_lossy(&u));
    }
    let b = buf.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(buf);
    let tete = &b[..b.len().min(2000)];
    if tete.iter().filter(|&&c| c == 0).count() > 4 {
        return None; // binaire
    }
    // Octets de contrôle (hors tabulation, fin de ligne, saut de page, échappement) : un fichier compressé ou un exécutable
    if tete.iter().filter(|&&c| c < 32 && !matches!(c, 9 | 10 | 12 | 13 | 27)).count() > tete.len() / 20 + 2 {
        return None;
    }
    Some(match std::str::from_utf8(b) {
        Ok(s) => s.to_string(),
        // Coupé au milieu d'un caractère : seule la fin est invalide
        Err(e) if e.error_len().is_none() => String::from_utf8_lossy(&b[..e.valid_up_to()]).into_owned(),
        // Ancien fichier Windows (ANSI) : octet = caractère Latin-1
        Err(_) => b.iter().map(|&c| c as char).collect(),
    })
}

/// Coupe une ligne à `colonnes` caractères, de préférence sur une espace
pub fn envelopper(ligne: &str, colonnes: usize) -> Vec<String> {
    // Un caractère de contrôle (retour chariot isolé, ESC d'un journal coloré…) s'afficherait en carré dans GDI
    let ligne: String = ligne.replace('\t', "    ").chars().filter(|c| !c.is_control()).collect();
    let car: Vec<char> = ligne.trim_end().chars().collect();
    if car.len() <= colonnes {
        return vec![car.into_iter().collect()];
    }
    let mut sortie = Vec::new();
    let mut debut = 0;
    while car.len() - debut > colonnes {
        let fin = debut + colonnes;
        let coupe = (fin.saturating_sub(30).max(debut + 1)..fin).rev().find(|&i| car[i] == ' ').map_or(fin, |i| i + 1);
        sortie.push(car[debut..coupe].iter().collect::<String>().trim_end().to_string());
        debut = coupe;
    }
    sortie.push(car[debut..].iter().collect());
    sortie
}

/// Découpe des sections (une diapositive, un fichier…) en pages de `LIGNES_PAGE` lignes ; une section commence une nouvelle page
pub fn paginer(sections: Vec<Vec<String>>) -> Vec<String> {
    let mut pages = Vec::new();
    let mut restant = MAX_LIGNES;
    for section in sections {
        // Borné avant de découper : un XML de 32 Mo ne doit pas fabriquer des centaines de milliers de lignes pour n'en garder que 10 400
        let lignes: Vec<String> = section.iter().flat_map(|l| envelopper(l, COLONNES)).take(restant).collect();
        // Une section vide (diapositive sans texte) garde sa page
        let lignes = if lignes.is_empty() { vec![String::new()] } else { lignes };
        for page in lignes.chunks(LIGNES_PAGE) {
            if restant == 0 {
                return pages;
            }
            restant = restant.saturating_sub(page.len());
            pages.push(page.join("\n"));
        }
    }
    pages
}

/// Liste d'une archive, une ligne par élément
fn liste_archive(chemin: &str) -> Option<Vec<String>> {
    let membres = crate::archive::membres(chemin).ok()?;
    Some(membres.iter().take(MAX_LIGNES).map(|m| if m.dossier { format!("{}/", m.chemin) } else { format!("{}  ({})", m.chemin, crate::systeme::taille(m.taille)) }).collect())
}

// ---------------------------------------------------------------------------------------------------------------------
// Documents

/// Texte d'un document Office / OpenDocument / RTF : une section par diapositive, une seule sinon
fn document(chemin: &str, ext: &str) -> Option<Vec<Vec<String>>> {
    if std::fs::metadata(chemin).ok()?.len() > MAX_DOCUMENT {
        return None;
    }
    if ext == "RTF" {
        let t = lire_texte(chemin, MAX_OCTETS)?;
        return Some(vec![rtf_vers_texte(&t).lines().map(str::to_string).collect()]);
    }
    let sections = match ext {
        "DOCX" | "DOCM" | "DOTX" => {
            let x = membres_xml(chemin, &["word/document.xml"]).into_iter().next()?;
            vec![xml_vers_lignes(&x, &["w:p"], &["w:br", "w:cr"], &["w:tab"], &["w:instrText", "w:delText"])]
        }
        "ODT" | "OTT" | "ODS" | "OTS" | "ODP" | "OTP" => {
            let x = membres_xml(chemin, &["content.xml"]).into_iter().next()?;
            vec![xml_vers_lignes(&x, &["text:p", "text:h", "table:table-row"], &["text:line-break"], &["text:tab", "table:table-cell"], &[])]
        }
        "PPTX" | "PPSX" | "PPTM" => {
            let noms: Vec<String> = crate::archive::membres(chemin).ok()?.iter().map(|m| m.chemin.clone()).filter(|c| c.starts_with("ppt/slides/slide") && c.ends_with(".xml")).take(300).collect();
            let refs: Vec<&str> = noms.iter().map(String::as_str).collect();
            let mut v = Vec::new();
            for (i, x) in membres_xml(chemin, &refs).iter().enumerate() {
                let mut s = vec![format!("DIAPOSITIVE {}", i + 1), String::new()];
                s.extend(xml_vers_lignes(x, &["a:p"], &["a:br"], &[], &[]));
                v.push(s);
            }
            v
        }
        "XLSX" | "XLSM" => {
            let docs = membres_xml(chemin, &["xl/sharedStrings.xml", "xl/worksheets/sheet1.xml"]);
            let partages: Vec<String> = docs.iter().find(|d| d.contains("<sst")).map(|d| chaines_partagees(d)).unwrap_or_default();
            let feuille = docs.iter().find(|d| d.contains("<worksheet"))?;
            vec![feuille_xlsx(feuille, &partages)]
        }
        _ => return None,
    };
    if sections.iter().all(|s| s.iter().all(|l| l.trim().is_empty())) {
        return None;
    }
    Some(sections)
}

/// Documents XML (sans leur ordre garanti : celui de l'archive) des membres demandés, lus en un seul tar.exe
fn membres_xml(chemin: &str, noms: &[&str]) -> Vec<String> {
    let Some(octets) = crate::archive::membres_en_sortie(chemin, noms) else { return Vec::new() };
    let texte = String::from_utf8_lossy(&octets);
    texte.split("<?xml").filter(|p| !p.trim().is_empty()).map(|p| format!("<?xml{p}")).collect()
}

/// Texte d'un document XML. `fins` : balises dont la fermeture finit une ligne ; `sauts` : balises qui sautent une ligne ;
/// `tabs` : balises qui valent une tabulation ; `ignorees` : balises dont le contenu n'est pas du texte (codes de champs).
pub fn xml_vers_lignes(x: &str, fins: &[&str], sauts: &[&str], tabs: &[&str], ignorees: &[&str]) -> Vec<String> {
    let mut sortie = String::with_capacity(x.len() / 4);
    let mut ignore: Option<&str> = None;
    let mut reste = x;
    while let Some(debut) = reste.find('<') {
        if ignore.is_none() {
            sortie.push_str(&reste[..debut]);
        }
        reste = &reste[debut + 1..];
        let Some(fin) = reste.find('>') else { break };
        let balise = &reste[..fin];
        reste = &reste[fin + 1..];
        let fermante = balise.starts_with('/');
        let nom = balise.trim_start_matches('/').split(|c: char| c.is_whitespace() || c == '/').next().unwrap_or("");
        match ignore {
            Some(i) => {
                if fermante && nom == i {
                    ignore = None;
                }
                continue;
            }
            None => {
                if !fermante && !balise.ends_with('/') {
                    if let Some(i) = ignorees.iter().find(|&&i| i == nom) {
                        ignore = Some(i);
                        continue;
                    }
                }
            }
        }
        if (fermante && fins.contains(&nom)) || (!fermante && sauts.contains(&nom)) {
            sortie.push('\n');
        } else if !fermante && tabs.contains(&nom) {
            sortie.push('\t');
        }
    }
    if ignore.is_none() {
        sortie.push_str(reste);
    }
    let decode = decoder_entites(&sortie);
    // Pas plus d'une ligne vide d'affilée
    let mut lignes: Vec<String> = Vec::new();
    for l in decode.lines() {
        let l = l.trim_end().to_string();
        if l.is_empty() && lignes.last().is_none_or(|d| d.is_empty()) {
            continue;
        }
        lignes.push(l);
    }
    lignes
}

/// `&amp;` `&lt;` `&gt;` `&quot;` `&apos;` `&#233;` `&#xE9;`
pub fn decoder_entites(t: &str) -> String {
    let mut sortie = String::with_capacity(t.len());
    let mut reste = t;
    while let Some(i) = reste.find('&') {
        sortie.push_str(&reste[..i]);
        reste = &reste[i..];
        let Some(fin) = reste.find(';').filter(|&f| f <= 10) else {
            sortie.push('&');
            reste = &reste[1..];
            continue;
        };
        let nom = &reste[1..fin];
        let c = match nom {
            "amp" => Some('&'),
            "lt" => Some('<'),
            "gt" => Some('>'),
            "quot" => Some('"'),
            "apos" => Some('\''),
            _ => nom.strip_prefix("#x").and_then(|h| u32::from_str_radix(h, 16).ok()).or_else(|| nom.strip_prefix('#').and_then(|d| d.parse().ok())).and_then(char::from_u32),
        };
        match c {
            Some(c) => {
                sortie.push(c);
                reste = &reste[fin + 1..];
            }
            None => {
                sortie.push('&');
                reste = &reste[1..];
            }
        }
    }
    sortie.push_str(reste);
    sortie
}

/// Chaînes de `sharedStrings.xml` d'un classeur
fn chaines_partagees(x: &str) -> Vec<String> {
    x.split("<si").skip(1).filter(|p| p.starts_with('>') || p.starts_with(' ')).map(|p| {
        let corps = p.split_once('>').map_or("", |(_, r)| r).split("</si>").next().unwrap_or("");
        xml_vers_lignes(corps, &[], &[], &[], &["rPh"]).join(" ")
    }).collect()
}

/// Une feuille de classeur : une ligne par ligne du tableau, cellules séparées par « | »
fn feuille_xlsx(x: &str, partages: &[String]) -> Vec<String> {
    let mut lignes = Vec::new();
    for ligne in x.split("<row ").skip(1) {
        let ligne = ligne.split("</row>").next().unwrap_or("");
        let mut cellules: Vec<String> = Vec::new();
        for cellule in ligne.split("<c ").skip(1) {
            let (entete, corps) = cellule.split_once('>').unwrap_or((cellule, ""));
            if entete.ends_with('/') {
                cellules.push(String::new());
                continue;
            }
            let corps = corps.split("</c>").next().unwrap_or("");
            let v = corps.split_once("<v>").and_then(|(_, r)| r.split("</v>").next());
            let texte = if entete.contains("t=\"s\"") {
                v.and_then(|i| i.trim().parse::<usize>().ok()).and_then(|i| partages.get(i).cloned()).unwrap_or_default()
            } else if entete.contains("t=\"inlineStr\"") {
                xml_vers_lignes(corps, &[], &[], &[], &[]).join(" ")
            } else {
                v.map(decoder_entites).unwrap_or_default()
            };
            cellules.push(texte);
        }
        while cellules.last().is_some_and(|c| c.is_empty()) {
            cellules.pop();
        }
        lignes.push(cellules.join(" | "));
    }
    lignes
}

/// Un octet de la page de codes Windows 1252 (celle d'un `\'xx` RTF) : 0x80-0x9F portent ’ € œ… et non des caractères de contrôle
fn cp1252(o: u8) -> char {
    const HAUT: [char; 32] = [
        '€', '\u{81}', '‚', 'ƒ', '„', '…', '†', '‡', 'ˆ', '‰', 'Š', '‹', 'Œ', '\u{8D}', 'Ž', '\u{8F}', '\u{90}', '‘', '’', '“', '”', '•', '–', '—', '˜', '™', 'š', '›', 'œ', '\u{9D}', 'ž', 'Ÿ',
    ];
    if (0x80..0xA0).contains(&o) {
        HAUT[(o - 0x80) as usize]
    } else {
        o as char
    }
}

/// Texte d'un RTF : sans les groupes de service (polices, couleurs, styles, images), `\par` finit une ligne
pub fn rtf_vers_texte(s: &str) -> String {
    let c: Vec<char> = s.chars().collect();
    let mut sortie = String::new();
    let (mut i, mut profondeur) = (0usize, 0usize);
    let mut saut_jusqu_a: Option<usize> = None;
    let mut uc = 1usize;
    while i < c.len() {
        match c[i] {
            '{' => {
                profondeur += 1;
                let suite: String = c[i + 1..(i + 14).min(c.len())].iter().collect();
                if saut_jusqu_a.is_none() && ["\\*", "\\fonttbl", "\\colortbl", "\\stylesheet", "\\info", "\\pict", "\\header", "\\footer"].iter().any(|g| suite.starts_with(g)) {
                    saut_jusqu_a = Some(profondeur);
                }
                i += 1;
            }
            '}' => {
                if saut_jusqu_a == Some(profondeur) {
                    saut_jusqu_a = None;
                }
                profondeur = profondeur.saturating_sub(1);
                i += 1;
            }
            '\\' => {
                i += 1;
                let Some(&n) = c.get(i) else { break };
                if n == '\\' || n == '{' || n == '}' {
                    if saut_jusqu_a.is_none() {
                        sortie.push(n);
                    }
                    i += 1;
                } else if n == '\'' {
                    let h: String = c[i + 1..(i + 3).min(c.len())].iter().collect();
                    if let (Ok(o), true) = (u8::from_str_radix(&h, 16), saut_jusqu_a.is_none()) {
                        sortie.push(cp1252(o));
                    }
                    i += 3;
                } else if n.is_ascii_alphabetic() {
                    let debut = i;
                    while i < c.len() && c[i].is_ascii_alphabetic() {
                        i += 1;
                    }
                    let mot: String = c[debut..i].iter().collect();
                    let num_debut = i;
                    if i < c.len() && c[i] == '-' {
                        i += 1;
                    }
                    while i < c.len() && c[i].is_ascii_digit() {
                        i += 1;
                    }
                    let num: String = c[num_debut..i].iter().collect();
                    if i < c.len() && c[i] == ' ' {
                        i += 1;
                    }
                    if saut_jusqu_a.is_none() {
                        match mot.as_str() {
                            "par" | "line" | "sect" | "page" => sortie.push('\n'),
                            "tab" => sortie.push('\t'),
                            "u" => {
                                if let Some(ch) = num.parse::<i32>().ok().map(|n| if n < 0 { n + 65536 } else { n }).and_then(|n| char::from_u32(n as u32)) {
                                    sortie.push(ch);
                                }
                                // \ucN remplaçants suivent \uN (1 par défaut) ; un \'xx compte pour un, une accolade n'en fait jamais partie
                                for _ in 0..uc {
                                    match (c.get(i), c.get(i + 1)) {
                                        (Some('\\'), Some('\'')) => i += 4,
                                        (Some('{' | '}') | None, _) => break,
                                        _ => i += 1,
                                    }
                                }
                                i = i.min(c.len());
                            }
                            "uc" => uc = num.parse::<usize>().unwrap_or(1).min(8),
                            _ => {}
                        }
                    }
                } else {
                    i += 1;
                }
            }
            '\r' | '\n' => i += 1,
            ch => {
                if saut_jusqu_a.is_none() {
                    sortie.push(ch);
                }
                i += 1;
            }
        }
    }
    sortie
}

// ---------------------------------------------------------------------------------------------------------------------
// Hexadécimal

/// Une page d'hexadécimal (offset, octets, texte) et le nombre de pages ; rien pour un fichier vide
pub fn hexa(chemin: &str, page: u32) -> Option<(String, u32)> {
    let mut f = std::fs::File::open(chemin).ok()?;
    let longueur = f.metadata().ok()?.len().min(MAX_HEXA);
    if longueur == 0 {
        return None;
    }
    let pages = longueur.div_ceil(OCTETS_PAGE as u64) as u32;
    let page = page.min(pages - 1);
    let debut = u64::from(page) * OCTETS_PAGE as u64;
    f.seek(SeekFrom::Start(debut)).ok()?;
    let mut buf = vec![0u8; (longueur - debut).min(OCTETS_PAGE as u64) as usize];
    f.read_exact(&mut buf).ok()?;
    Some((lignes_hexa(&buf, debut), pages))
}

/// `000010  48 65 6C 6C 6F …  |Hello…|`
pub fn lignes_hexa(octets: &[u8], debut: u64) -> String {
    octets
        .chunks(OCTETS_LIGNE)
        .enumerate()
        .map(|(i, l)| {
            let hex: String = (0..OCTETS_LIGNE).map(|k| l.get(k).map_or("   ".to_string(), |o| format!("{o:02X} "))).collect();
            let ascii: String = l.iter().map(|&o| if (32..127).contains(&o) { o as char } else { '.' }).collect();
            // La dernière ligne, plus courte, garde la barre de fermeture alignée avec les autres
            format!("{:06X}  {}|{:<16}|", debut + (i * OCTETS_LIGNE) as u64, hex, ascii)
        })
        .collect::<Vec<_>>()
        .join("\n")
}

// ---------------------------------------------------------------------------------------------------------------------
// Images

/// Une trame d'une image par le décodeur de Windows (WIC) et le nombre de trames. `multi` : ne rien rendre pour une image à une
/// seule trame (la miniature du shell est alors meilleure et moins lourde) ; sinon sert de repli pour les formats sans miniature
/// (WebP, HEIC, AVIF… selon les codecs installés).
pub fn trame(chemin: &str, trame: u32, multi: bool, cote_max: u32) -> Option<(Image, u32)> {
    use windows::core::HSTRING;
    use windows::Graphics::Imaging::{BitmapAlphaMode, BitmapDecoder, BitmapInterpolationMode, BitmapPixelFormat, BitmapTransform, ColorManagementMode, ExifOrientationMode};
    use windows::Storage::StorageFile;
    let fichier = StorageFile::GetFileFromPathAsync(&HSTRING::from(chemin)).ok()?.get().ok()?;
    let flux = fichier.OpenReadAsync().ok()?.get().ok()?;
    let decodeur = BitmapDecoder::CreateAsync(&flux).ok()?.get().ok()?;
    let n = decodeur.FrameCount().ok()?;
    if n == 0 || (multi && n < 2) {
        return None;
    }
    let cadre = decodeur.GetFrameAsync(trame.min(n - 1)).ok()?.get().ok()?;
    // Une image de plusieurs dizaines de mégapixels pèserait des centaines de Mo : on laisse la miniature du shell
    let (l, h) = (cadre.PixelWidth().ok()?, cadre.PixelHeight().ok()?);
    if l == 0 || h == 0 || u64::from(l) * u64::from(h) > 40_000_000 {
        return None;
    }
    // Réduite à 2 048 px de côté au décodage : la carte du coup d'œil n'en montre pas plus, et une grande trame (6 000 × 6 000 =
    // 144 Mo par copie) dépasserait le budget mémoire du résident ou la taille maximale d'un bitmap Direct2D
    let echelle = (f64::from(cote_max) / f64::from(l.max(h))).min(1.0);
    let transformation = BitmapTransform::new().ok()?;
    transformation.SetScaledWidth(((f64::from(l) * echelle).round() as u32).max(1)).ok()?;
    transformation.SetScaledHeight(((f64::from(h) * echelle).round() as u32).max(1)).ok()?;
    transformation.SetInterpolationMode(BitmapInterpolationMode::Fant).ok()?;
    let image = cadre
        .GetSoftwareBitmapTransformedAsync(BitmapPixelFormat::Bgra8, BitmapAlphaMode::Premultiplied, &transformation, ExifOrientationMode::IgnoreExifOrientation, ColorManagementMode::DoNotColorManage)
        .ok()?
        .get()
        .ok()?;
    crate::apercu::image_de_bitmap(&image).map(|i| (i, n))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn envelopper_coupe_sur_une_espace_sans_rien_perdre() {
        let ligne = "mot ".repeat(60);
        let v = envelopper(&ligne, 108);
        assert!(v.len() >= 2);
        assert!(v.iter().all(|l| l.chars().count() <= 108));
        assert_eq!(v.join(" ").split_whitespace().count(), 60);
        // Un mot plus long que la ligne est coupé net, sans boucle infinie
        let long = "x".repeat(300);
        let v = envelopper(&long, 108);
        assert_eq!(v.iter().map(|l| l.len()).sum::<usize>(), 300);
        assert_eq!(envelopper("court", 108), vec!["court".to_string()]);
        // Retour chariot et séquence d'échappement ne laissent aucun caractère de contrôle ; la tabulation devient des espaces
        assert_eq!(envelopper("a\r\x1b[31mb\tc", 108), vec!["a[31mb    c".to_string()]);
    }

    #[test]
    fn paginer_coupe_toutes_les_vingt_six_lignes_et_une_section_ouvre_une_page() {
        let lignes: Vec<String> = (0..60).map(|i| format!("ligne {i}")).collect();
        let p = paginer(vec![lignes]);
        assert_eq!(p.len(), 3);
        assert_eq!(p[0].lines().count(), 26);
        assert_eq!(p[2].lines().count(), 8);
        let p = paginer(vec![vec!["a".into()], vec![], vec!["c".into()]]);
        assert_eq!(p.len(), 3, "une diapositive vide garde sa page");
        assert!(paginer(vec![]).is_empty());
    }

    #[test]
    fn paginer_borne_le_nombre_de_pages() {
        let lignes: Vec<String> = (0..MAX_LIGNES * 2).map(|i| i.to_string()).collect();
        assert_eq!(paginer(vec![lignes]).len(), 400);
    }

    #[test]
    fn xml_donne_des_paragraphes_et_decode_les_entites() {
        let x = r#"<w:body><w:p><w:r><w:t>Bonjour &amp; bienvenue</w:t></w:r></w:p><w:p><w:r><w:t>l&#233;ger</w:t><w:tab/><w:t>fin</w:t></w:r></w:p><w:p><w:instrText> PAGEREF _Toc1 </w:instrText><w:t>visible</w:t></w:p></w:body>"#;
        let v = xml_vers_lignes(x, &["w:p"], &["w:br"], &["w:tab"], &["w:instrText"]);
        assert_eq!(v, vec!["Bonjour & bienvenue", "léger\tfin", "visible"]);
    }

    #[test]
    fn xml_replie_les_lignes_vides_en_double() {
        let v = xml_vers_lignes("<p>a</p><p></p><p></p><p></p><p>b</p>", &["p"], &[], &[], &[]);
        assert_eq!(v, vec!["a", "", "b"]);
    }

    #[test]
    fn entites_inconnues_et_esperluettes_seules_restent_telles_quelles() {
        assert_eq!(decoder_entites("a & b &inconnue; &#x41;&#66;"), "a & b &inconnue; AB");
        assert_eq!(decoder_entites("fin &"), "fin &");
    }

    #[test]
    fn classeur_lit_chaines_partagees_nombres_et_cellules_vides() {
        let partages = chaines_partagees("<sst><si><t>Nom</t></si><si><t>Age</t></si><si><r><t>Za</t></r><r><t>ra</t></r></si></sst>");
        assert_eq!(partages, vec!["Nom", "Age", "Zara"]);
        let feuille = r#"<sheetData><row r="1"><c r="A1" t="s"><v>0</v></c><c r="B1" t="s"><v>1</v></c></row><row r="2"><c r="A2" t="s"><v>2</v></c><c r="B2"><v>31</v></c><c r="C2"/></row></sheetData>"#;
        assert_eq!(feuille_xlsx(feuille, &partages), vec!["Nom | Age", "Zara | 31"]);
    }

    #[test]
    fn rtf_garde_le_texte_les_accents_et_saute_les_polices() {
        let rtf = r"{\rtf1\ansi{\fonttbl{\f0 Arial;}}{\colortbl;\red0\green0\blue0;}\pard Bonjour \b monde\b0\par \'e9t\'e9\par \u233?a}";
        assert_eq!(rtf_vers_texte(rtf), "Bonjour monde\nété\néa");
    }

    #[test]
    fn rtf_saute_les_remplacants_unicode_et_lit_la_page_1252() {
        // \uc1 : 舗 est suivi d'un remplaçant \'92 (un seul) ; \'92 seul est une apostrophe typographique, \'80 un euro
        assert_eq!(rtf_vers_texte(concat!(r"{\rtf1\uc1 l\", "u8217", r"\'92\'e9t\u233\'e9 \'80}")), "l’été €");
        // \uc0 : aucun remplaçant, l'accolade fermante n'est pas mangée et le groupe suivant se lit
        assert_eq!(rtf_vers_texte(r"{\rtf1\uc0 {a\u233}{b}}"), "aéb");
    }

    #[test]
    fn hexa_formate_offset_octets_et_ascii() {
        let t = lignes_hexa(b"Hello, SpaceFiles!\x00", 0x10);
        let l: Vec<&str> = t.lines().collect();
        assert_eq!(l.len(), 2);
        assert!(l[0].starts_with("000010  48 65 6C 6C 6F 2C 20 53 70 61 63 65 46 69 6C 65 |Hello, SpaceFile|"), "{}", l[0]);
        assert!(l[1].starts_with("000020  73 21 00 "), "{}", l[1]);
        assert!(l[1].ends_with("|s!.             |"), "{}", l[1]);
        assert_eq!(l[0].chars().count(), l[1].chars().count(), "la barre de fermeture reste alignée");
    }

    #[test]
    fn decoder_distingue_texte_et_binaire() {
        assert_eq!(decoder(b"abc\r\ndef").as_deref(), Some("abc\r\ndef"));
        assert_eq!(decoder("é".as_bytes()).as_deref(), Some("é"));
        assert_eq!(decoder(&[b'a', 0xE9, b'b']).as_deref(), Some("aéb"), "ANSI lu en Latin-1");
        assert!(decoder(&[0, 1, 2, 3, 0, 0, 0, 0, 9, 8, 7]).is_none());
        let compresse: Vec<u8> = (1..200u8).filter(|c| !matches!(c, 9 | 10 | 12 | 13 | 27)).take(150).collect();
        assert!(decoder(&compresse).is_none(), "octets de contrôle en nombre");
        assert_eq!(decoder(&[0xFF, 0xFE, b'a', 0, b'b', 0]).as_deref(), Some("ab"));
    }

    #[test]
    fn documents_reconnus() {
        assert!(est_document("DOCX") && est_document("RTF") && est_document("PPTX") && !est_document("TXT"));
    }
}
