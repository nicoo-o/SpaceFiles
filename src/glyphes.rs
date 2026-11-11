// Glyph de dossier (O1) : un motif de points choisi pour un dossier, dessiné à la place de son icône,
// comme les sonneries Glyph des téléphones Nothing. Gardés dans les réglages (« NOM|chemin »).

use std::collections::HashMap;
use std::sync::RwLock;

/// Motifs (7 × 7 au plus) : `#` point blanc, `r` point rouge, `.` vide
pub const MOTIFS: [(&str, [&str; 7]); 8] = [
    ("ANNEAU", ["..###..", ".#...#.", "#.....#", "#..r..#", "#.....#", ".#...#.", "..###.."]),
    ("ETOILE", ["...#...", "...#...", "..#r#..", "###r###", "..#r#..", "...#...", "...#..."]),
    ("CROIX", ["#.....#", ".#...#.", "..#.#..", "...r...", "..#.#..", ".#...#.", "#.....#"]),
    ("COEUR", [".......", ".##.##.", "#######", "###r###", ".#####.", "..###..", "...#..."]),
    ("ECLAIR", ["....##.", "...##..", "..##...", ".#####r", "...##..", "..##...", ".##...."]),
    ("VAGUE", [".......", ".......", ".##...#", "#..#.#.", "....r..", ".......", "......."]),
    ("LOSANGE", ["...#...", "..#.#..", ".#...#.", "#..r..#", ".#...#.", "..#.#..", "...#..."]),
    ("CIBLE", ["#######", "#.....#", "#.###.#", "#.#r#.#", "#.###.#", "#.....#", "#######"]),
];

/// Dossier (minuscules, sans « \ » final) → rang du motif
static DOSSIERS: RwLock<Option<HashMap<String, usize>>> = RwLock::new(None);

fn cle(chemin: &str) -> String {
    chemin.trim_end_matches('\\').to_lowercase()
}

/// Réglages chargés ou changés : la table est refaite
pub fn recharger(liste: &[String]) {
    let table = liste
        .iter()
        .filter_map(|x| {
            let (nom, chemin) = x.split_once('|')?;
            Some((cle(chemin), MOTIFS.iter().position(|m| m.0 == nom)?))
        })
        .collect();
    *DOSSIERS.write().unwrap_or_else(|e| e.into_inner()) = Some(table);
}

/// Motif du dossier, s'il en a un
pub fn de(chemin: &str) -> Option<&'static [&'static str]> {
    let g = DOSSIERS.read().unwrap_or_else(|e| e.into_inner());
    let i = *g.as_ref()?.get(&cle(chemin))?;
    Some(&MOTIFS[i].1)
}

/// Donne le motif `rang` (None : l'enlève) à ces dossiers
pub fn choisir(chemins: &[String], rang: Option<usize>) {
    let cles: Vec<String> = chemins.iter().map(|c| cle(c)).collect();
    crate::reglages::modifier(|r| {
        r.glyphes.retain(|x| !x.split_once('|').is_some_and(|(_, c)| cles.contains(&cle(c))));
        if let Some(i) = rang.filter(|&i| i < MOTIFS.len()) {
            for c in chemins {
                r.glyphes.push(format!("{}|{}", MOTIFS[i].0, c.trim_end_matches('\\')));
            }
        }
    });
}
