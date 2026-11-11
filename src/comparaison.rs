// Comparer deux fichiers texte (Z4) : différences ligne à ligne, écrites en format « diff » (+ ajouté, - retiré,
// @@ saut) dans un fichier de travail que l'aperçu colore. Les lignes communes au début et à la fin sont écartées
// avant le calcul : seul le milieu coûte (limite MAX_MILIEU lignes de chaque côté).

use crate::apercu::Teinte;

/// Lignes différentes tolérées de chaque côté : la table du calcul prend MAX² cases (16 Mo à 2000)
const MAX_MILIEU: usize = 2000;
/// Lignes inchangées montrées autour de chaque différence
const CONTEXTE: usize = 3;

/// Différences de `a` vers `b`, ou Err si le milieu dépasse la limite. Texte vide si les deux sont identiques.
pub fn diff(a: &str, b: &str) -> Result<String, String> {
    let (a, b): (Vec<&str>, Vec<&str>) = (a.lines().collect(), b.lines().collect());
    let debut = a.iter().zip(&b).take_while(|(x, y)| x == y).count();
    let fin = a[debut..].iter().rev().zip(b[debut..].iter().rev()).take_while(|(x, y)| x == y).count();
    let (ma, mb) = (&a[debut..a.len() - fin], &b[debut..b.len() - fin]);
    if ma.is_empty() && mb.is_empty() {
        return Ok(String::new());
    }
    if ma.len() > MAX_MILIEU || mb.len() > MAX_MILIEU {
        return Err(format!("TROP DE LIGNES DIFFERENTES (PLUS DE {MAX_MILIEU} DE CHAQUE COTE)"));
    }
    // Plus longue sous-suite commune du milieu, table remplie depuis la fin pour être relue dans l'ordre
    let (n, m) = (ma.len(), mb.len());
    let mut t = vec![0u32; (n + 1) * (m + 1)];
    let i = |x: usize, y: usize| x * (m + 1) + y;
    for x in (0..n).rev() {
        for y in (0..m).rev() {
            t[i(x, y)] = if ma[x] == mb[y] { t[i(x + 1, y + 1)] + 1 } else { t[i(x + 1, y)].max(t[i(x, y + 1)]) };
        }
    }
    // Opérations dans l'ordre : (signe, numéro de ligne dans a, texte)
    let mut ops: Vec<(char, usize, &str)> = (0..debut).map(|k| (' ', k, a[k])).collect();
    let (mut x, mut y) = (0, 0);
    while x < n || y < m {
        if x < n && y < m && ma[x] == mb[y] {
            ops.push((' ', debut + x, ma[x]));
            x += 1;
            y += 1;
        } else if y < m && (x == n || t[i(x, y + 1)] > t[i(x + 1, y)]) {
            ops.push(('+', debut + x, mb[y]));
            y += 1;
        } else {
            ops.push(('-', debut + x, ma[x]));
            x += 1;
        }
    }
    ops.extend((0..fin).map(|k| (' ', a.len() - fin + k, a[a.len() - fin + k])));
    // Ne garder que les différences et leur contexte
    let mut montrer = vec![false; ops.len()];
    for (k, o) in ops.iter().enumerate() {
        if o.0 != ' ' {
            montrer[k.saturating_sub(CONTEXTE)..(k + CONTEXTE + 1).min(ops.len())].fill(true);
        }
    }
    let mut s = String::new();
    let mut saut = false;
    for (k, o) in ops.iter().enumerate() {
        if !montrer[k] {
            saut = true;
            continue;
        }
        if saut && !s.is_empty() {
            s += &format!("@@ LIGNE {} @@\n", o.1 + 1);
        }
        saut = false;
        s += &format!("{} {}\n", o.0, o.2);
    }
    Ok(s)
}

/// Couleurs de l'aperçu : ajouts, retraits et sauts (début et longueur en UTF-16)
pub fn teintes(t: &str) -> Vec<(u32, u32, Teinte)> {
    let mut v = Vec::new();
    let mut p = 0u32;
    for l in t.split_inclusive('\n') {
        let n = l.encode_utf16().count() as u32;
        let te = match l.chars().next() {
            Some('+') => Some(Teinte::Chaine),
            Some('-') => Some(Teinte::MotCle),
            Some('@') => Some(Teinte::Titre),
            _ => None,
        };
        if let Some(te) = te {
            v.push((p, n, te));
        }
        p += n;
    }
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn differences_de_lignes() {
        assert_eq!(diff("a\nb\nc\n", "a\nb\nc\n").unwrap(), "");
        let d = diff("un\ndeux\ntrois\nquatre\n", "un\nDEUX\ntrois\nquatre\ncinq\n").unwrap();
        assert_eq!(d, "  un\n- deux\n+ DEUX\n  trois\n  quatre\n+ cinq\n");
        // Loin de la différence, le contexte est coupé
        let long: String = (1..=30).map(|k| format!("l{k}\n")).collect();
        let d = diff(&long, &long.replace("l15\n", "X\n")).unwrap();
        assert!(d.contains("- l15\n+ X\n") && !d.contains("  l1\n") && d.contains("  l12\n"));
        let t = teintes("+ a\n  b\n- é\n");
        assert_eq!(t, vec![(0, 4, Teinte::Chaine), (8, 4, Teinte::MotCle)]);
        let enorme: String = (0..2500).map(|k| format!("a{k}\n")).collect();
        assert!(diff(&enorme, "x\n").is_err());
    }
}
