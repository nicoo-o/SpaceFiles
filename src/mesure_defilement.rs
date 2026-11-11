// Mesure du défilement (mode essai seulement) : `SPACEFILES_MESURE_DEFILEMENT=n[,grille]` avec `SPACEFILES_SANS_REGISTRE=1`.
// Une fois le dossier lu, la liste descend de haut en bas de n DIP par image, une image après l'autre (chaque image en
// demande une autre), puis le journal reçoit le nombre d'images, les temps de `Rendu::dessiner` et les intervalles entre
// deux images (le temps perçu : il contient l'attente de la synchro verticale). Aucune entrée souris ni clavier.
// Hors mode essai, `demandee()` renvoie None une fois pour toutes : aucun coût.

use std::cell::RefCell;
use std::time::Instant;

use windows::Win32::Foundation::HWND;
use windows::Win32::Graphics::Gdi::InvalidateRect;

use crate::app::{Affichage, App};

/// Images de chauffe (sans défilement, non comptées) : caches de texte et de géométrie, premières vignettes
const CHAUFFE: usize = 20;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Demande {
    /// Pas du défilement en DIP par image
    pub pas: f32,
    pub grille: bool,
}

/// `n` ou `n,grille` : n de 1 à 5000 DIP par image
fn lire(valeur: Option<&str>) -> Option<Demande> {
    let v = valeur?;
    let (n, option) = match v.split_once(',') {
        Some((n, o)) => (n, Some(o.trim())),
        None => (v, None),
    };
    let pas: f32 = n.trim().parse().ok().filter(|p| (1.0..=5000.0).contains(p))?;
    match option {
        None => Some(Demande { pas, grille: false }),
        Some("grille") => Some(Demande { pas, grille: true }),
        Some(_) => None,
    }
}

/// Lue une fois : appelée à chaque WM_PAINT
pub fn demandee() -> Option<Demande> {
    static VALEUR: std::sync::OnceLock<Option<Demande>> = std::sync::OnceLock::new();
    *VALEUR.get_or_init(|| {
        if !crate::defaut::sans_registre() {
            return None;
        }
        lire(std::env::var("SPACEFILES_MESURE_DEFILEMENT").ok().as_deref())
    })
}

/// Médiane, 95e centile (rang supérieur) et maximum ; `None` si rien n'est mesuré
fn resume(valeurs: &[f64]) -> Option<(f64, f64, f64)> {
    if valeurs.is_empty() {
        return None;
    }
    let mut t = valeurs.to_vec();
    t.sort_by(|a, b| a.total_cmp(b));
    let n = t.len();
    let mediane = if n % 2 == 1 { t[n / 2] } else { (t[n / 2 - 1] + t[n / 2]) / 2.0 };
    let p95 = t[((n as f64 * 0.95).ceil() as usize).clamp(1, n) - 1];
    Some((mediane, p95, t[n - 1]))
}

#[derive(Default)]
struct Etat {
    /// Images vues depuis que le dossier est prêt
    vues: usize,
    debut_image: Option<Instant>,
    debut_mesure: Option<Instant>,
    dernier_debut: Option<Instant>,
    dessins: Vec<f64>,
    intervalles: Vec<f64>,
    fini: bool,
}

thread_local! {
    // Fil d'interface seulement : la procédure de la fenêtre est la seule à l'appeler
    static ETAT: RefCell<Etat> = RefCell::new(Etat::default());
}

/// À appeler juste avant le dessin. Renvoie vrai si la mesure est en cours (le dessin sera chronométré par `apres_image`).
pub fn avant_image(app: &mut App, demande: Demande) -> bool {
    ETAT.with(|e| {
        let mut e = e.borrow_mut();
        // Dossier pas encore lu, ou première image pas encore dessinée (hauteur de liste inconnue) : dessin ordinaire
        if e.fini || app.chargement || app.entrees.is_empty() || app.hauteur_liste <= 0.0 {
            return false;
        }
        if e.vues == 0 {
            if demande.grille {
                app.affichage = Affichage::Grille;
            }
            app.defilement = 0.0;
            app.defilement_cible = 0.0;
        }
        e.debut_image = Some(Instant::now());
        true
    })
}

/// À appeler juste après le dessin d'une image chronométrée : enregistre, avance la liste, demande l'image suivante.
pub fn apres_image(hwnd: HWND, app: &mut App, demande: Demande) {
    ETAT.with(|e| {
        let mut e = e.borrow_mut();
        let Some(debut) = e.debut_image.take() else { return };
        let fin = Instant::now();
        e.vues += 1;
        if e.vues > CHAUFFE {
            e.dessins.push(fin.duration_since(debut).as_secs_f64() * 1000.0);
            if let Some(avant) = e.dernier_debut {
                e.intervalles.push(debut.duration_since(avant).as_secs_f64() * 1000.0);
            }
            e.dernier_debut = Some(debut);
            e.debut_mesure.get_or_insert(debut);
            let avant = app.defilement_cible;
            app.defiler(demande.pas);
            app.defilement = app.defilement_cible;
            if app.defilement_cible <= avant {
                e.fini = true;
                let total = fin.duration_since(e.debut_mesure.unwrap_or(debut)).as_secs_f64();
                let (dm, dp, dx) = resume(&e.dessins).unwrap_or_default();
                let (im, ip, ix) = resume(&e.intervalles).unwrap_or_default();
                let lents = |limite: f64| e.intervalles.iter().filter(|&&t| t > limite).count();
                crate::journal::ecrire(
                    crate::journal::Niveau::Info,
                    "defilement",
                    &format!(
                        "MESURE vue={} entrees={} pas={} dip/image lecture={:.1} ms images={} duree={:.2} s fps={:.1} \
                         dessiner(ms) mediane={dm:.3} p95={dp:.3} max={dx:.3} ; intervalle(ms) mediane={im:.3} p95={ip:.3} max={ix:.3} ; \
                         intervalles>16.7ms={} >33.4ms={} sur {}",
                        if demande.grille { "grille" } else { "liste" },
                        app.entrees.len(),
                        demande.pas,
                        app.duree_ms,
                        e.dessins.len(),
                        total,
                        e.dessins.len() as f64 / total.max(1e-9),
                        lents(16.7),
                        lents(33.4),
                        e.intervalles.len()
                    ),
                );
                return;
            }
        }
        // Chaque image en demande une autre, sans minuterie : le rythme est celui de la fenêtre (synchro verticale comprise)
        // SAFETY : hwnd est la fenêtre en cours de WM_PAINT, sur son fil
        unsafe {
            let _ = InvalidateRect(hwnd, None, false);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lecture_de_la_demande() {
        assert_eq!(lire(Some("200")), Some(Demande { pas: 200.0, grille: false }));
        assert_eq!(lire(Some(" 50 , grille ")), Some(Demande { pas: 50.0, grille: true }));
        assert_eq!(lire(Some("0")), None);
        assert_eq!(lire(Some("6000")), None);
        assert_eq!(lire(Some("abc")), None);
        assert_eq!(lire(Some("10,autre")), None);
        assert_eq!(lire(None), None);
    }

    #[test]
    fn resume_median_p95_max() {
        assert_eq!(resume(&[]), None);
        assert_eq!(resume(&[4.0]), Some((4.0, 4.0, 4.0)));
        let v: Vec<f64> = (1..=100).rev().map(f64::from).collect();
        assert_eq!(resume(&v), Some((50.5, 95.0, 100.0)));
        assert_eq!(resume(&[3.0, 1.0, 2.0]), Some((2.0, 3.0, 3.0)));
    }
}
