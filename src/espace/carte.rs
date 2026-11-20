// Page ESPACE, vue CARTE (E2) : la disposition en rectangles (« treemap ») des éléments d'un dossier.
//
// Logique pure, sans fenêtre : on donne des poids (tailles) triés du plus gros au plus petit et un rectangle, on reçoit un
// rectangle par poids, dans le même ordre. Algorithme « squarified » (Bruls, Huizing, van Wijk) : on remplit des rangées
// le long du côté le plus court tant que le pire rapport hauteur / largeur s'améliore, pour des rectangles proches du carré
// (lisibles, cliquables). Les surfaces sont proportionnelles aux poids ; un poids nul reçoit un rectangle vide.

/// Un rectangle en DIP : origine en haut à gauche
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub l: f32,
    pub h: f32,
}

impl Rect {
    pub fn contient(&self, px: f32, py: f32) -> bool {
        px >= self.x && px < self.x + self.l && py >= self.y && py < self.y + self.h
    }
}

/// Pire rapport (≥ 1) des rectangles d'une rangée de surfaces `s` posée sur un côté de longueur `cote`
fn pire(s: &[f64], cote: f64) -> f64 {
    let somme: f64 = s.iter().sum();
    if somme <= 0.0 || cote <= 0.0 {
        return f64::INFINITY;
    }
    let (mini, maxi) = s.iter().fold((f64::INFINITY, 0.0_f64), |(a, b), &v| (a.min(v), b.max(v)));
    let c2 = cote * cote;
    ((c2 * maxi) / (somme * somme)).max((somme * somme) / (c2 * mini.max(f64::MIN_POSITIVE)))
}

/// Un rectangle par poids, dans l'ordre reçu. `poids` doit être trié du plus grand au plus petit (sinon les rectangles
/// restent justes mais moins carrés). Les surfaces sont proportionnelles aux poids et remplissent `(x, y, l, h)`.
pub fn disposer(poids: &[u64], x: f32, y: f32, l: f32, h: f32) -> Vec<Rect> {
    let vide = Rect { x, y, l: 0.0, h: 0.0 };
    let mut sortie = vec![vide; poids.len()];
    let total: f64 = poids.iter().map(|&p| p as f64).sum();
    if poids.is_empty() || total <= 0.0 || l <= 0.0 || h <= 0.0 {
        return sortie;
    }
    // Surfaces en DIP² ; le dernier poids non nul borne la boucle (les zéros restent vides)
    let echelle = (l as f64 * h as f64) / total;
    let n = poids.iter().rposition(|&p| p > 0).map_or(0, |k| k + 1);
    let surfaces: Vec<f64> = poids[..n].iter().map(|&p| p as f64 * echelle).collect();
    let (mut rx, mut ry, mut rl, mut rh) = (x as f64, y as f64, l as f64, h as f64);
    let mut debut = 0;
    while debut < n {
        // Rangée le long du côté le plus court du rectangle restant
        let horizontale = rl >= rh;
        let cote = if horizontale { rh } else { rl };
        let mut fin = debut + 1;
        while fin < n && pire(&surfaces[debut..=fin], cote) <= pire(&surfaces[debut..fin], cote) {
            fin += 1;
        }
        let somme: f64 = surfaces[debut..fin].iter().sum();
        // Épaisseur de la rangée, perpendiculaire à son côté ; la dernière rangée prend tout le reste (pas d'écart d'arrondi)
        let epaisseur = if fin == n { if horizontale { rl } else { rh } } else if horizontale { somme / rh } else { somme / rl };
        let mut pos = if horizontale { ry } else { rx };
        for k in debut..fin {
            let part = if somme > 0.0 { surfaces[k] / somme * cote } else { 0.0 };
            sortie[k] = if horizontale {
                Rect { x: rx as f32, y: pos as f32, l: epaisseur as f32, h: part as f32 }
            } else {
                Rect { x: pos as f32, y: ry as f32, l: part as f32, h: epaisseur as f32 }
            };
            pos += part;
        }
        if horizontale {
            rx += epaisseur;
            rl -= epaisseur;
        } else {
            ry += epaisseur;
            rh -= epaisseur;
        }
        debut = fin;
    }
    sortie
}

#[cfg(test)]
mod tests {
    use super::*;

    fn aire(r: &Rect) -> f64 {
        r.l as f64 * r.h as f64
    }

    fn recouvrement(a: &Rect, b: &Rect) -> f64 {
        let l = ((a.x + a.l).min(b.x + b.l) - a.x.max(b.x)) as f64;
        let h = ((a.y + a.h).min(b.y + b.h) - a.y.max(b.y)) as f64;
        if l > 0.01 && h > 0.01 {
            l * h
        } else {
            0.0
        }
    }

    #[test]
    fn les_surfaces_remplissent_le_rectangle_et_suivent_les_poids() {
        let poids = [600u64, 300, 200, 100, 100, 50, 30, 10, 5, 5];
        let rs = disposer(&poids, 10.0, 20.0, 800.0, 400.0);
        assert_eq!(rs.len(), poids.len());
        let somme: f64 = rs.iter().map(aire).sum();
        assert!((somme - 800.0 * 400.0).abs() < 1.0, "somme des surfaces : {somme}");
        let total: u64 = poids.iter().sum();
        for (r, &p) in rs.iter().zip(&poids) {
            let attendu = 800.0 * 400.0 * p as f64 / total as f64;
            assert!((aire(r) - attendu).abs() < attendu * 0.001 + 0.5, "poids {p} : surface {} au lieu de {attendu}", aire(r));
        }
    }

    #[test]
    fn rien_ne_sort_ni_ne_se_chevauche() {
        let poids: Vec<u64> = (1..=40).rev().map(|k| k * k * 7).collect();
        let rs = disposer(&poids, 5.0, 7.0, 640.0, 360.0);
        for r in &rs {
            assert!(r.l >= 0.0 && r.h >= 0.0, "taille négative : {r:?}");
            assert!(r.x >= 5.0 - 0.01 && r.y >= 7.0 - 0.01 && r.x + r.l <= 645.0 + 0.01 && r.y + r.h <= 367.0 + 0.01, "hors bornes : {r:?}");
        }
        for i in 0..rs.len() {
            for j in i + 1..rs.len() {
                assert_eq!(recouvrement(&rs[i], &rs[j]), 0.0, "{i} et {j} se chevauchent : {:?} {:?}", rs[i], rs[j]);
            }
        }
    }

    #[test]
    fn les_rectangles_restent_proches_du_carre() {
        let poids = [500u64, 400, 300, 200, 100, 90, 80, 70, 60, 50];
        let rs = disposer(&poids, 0.0, 0.0, 600.0, 400.0);
        for r in &rs {
            let rapport = (r.l / r.h).max(r.h / r.l);
            assert!(rapport < 6.0, "rectangle trop allongé : {r:?} ({rapport})");
        }
    }

    #[test]
    fn cas_limites() {
        assert!(disposer(&[], 0.0, 0.0, 100.0, 100.0).is_empty());
        // Un seul élément : il prend tout
        let un = disposer(&[42], 3.0, 4.0, 100.0, 50.0);
        assert_eq!(un, vec![Rect { x: 3.0, y: 4.0, l: 100.0, h: 50.0 }]);
        // Des zéros : rectangles vides, les autres se partagent tout
        let rs = disposer(&[10, 10, 0, 0], 0.0, 0.0, 100.0, 100.0);
        assert_eq!((rs[2].l * rs[2].h, rs[3].l * rs[3].h), (0.0, 0.0));
        assert!((aire(&rs[0]) + aire(&rs[1]) - 10_000.0).abs() < 1.0);
        // Tout à zéro, ou une surface nulle : que des rectangles vides, sans panique
        assert!(disposer(&[0, 0], 0.0, 0.0, 100.0, 100.0).iter().all(|r| aire(r) == 0.0));
        assert!(disposer(&[5, 5], 0.0, 0.0, 0.0, 100.0).iter().all(|r| aire(r) == 0.0));
        // Très déséquilibré : un géant et des miettes
        let rs = disposer(&[1_000_000_000_000, 1, 1], 0.0, 0.0, 300.0, 200.0);
        assert!(aire(&rs[0]) > 59_990.0);
    }

    #[test]
    fn contient_vise_le_bon_rectangle() {
        let rs = disposer(&[3, 1], 0.0, 0.0, 400.0, 100.0);
        assert!(rs[0].contient(10.0, 10.0));
        assert!(!rs[1].contient(10.0, 10.0));
        let (cx, cy) = (rs[1].x + rs[1].l / 2.0, rs[1].y + rs[1].h / 2.0);
        assert!(rs[1].contient(cx, cy) && !rs[0].contient(cx, cy));
    }
}
