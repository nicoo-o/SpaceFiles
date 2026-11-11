// UI Automation (V13) : la fenêtre se dessine toute seule en Direct2D, un lecteur d'écran n'y voit donc rien.
// Un petit arbre de fournisseurs la décrit : la fenêtre > la liste (une ligne par élément), la barre d'état et les pastilles
// d'opération (« Copie, 42 pour cent »).
// Les fournisseurs sont appelés depuis des fils de UI Automation : ils ne lisent qu'un instantané copié ici,
// jamais l'état de l'application.

// Les constantes de UI Automation gardent leur nom Windows (UIA_NamePropertyId…), même dans les motifs
#![allow(non_upper_case_globals)]

use std::sync::Mutex;

use windows::core::{implement, Result, BSTR, VARIANT};
use windows::Win32::Foundation::{E_FAIL, HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::System::Com::SAFEARRAY;
use windows::Win32::System::Ole::{SafeArrayCreateVector, SafeArrayPutElement};
use windows::Win32::System::Variant::VT_I4;
use windows::Win32::UI::Accessibility::*;
use windows::Win32::UI::WindowsAndMessaging::GetWindowRect;

use crate::app::App;
use crate::icones;
use crate::systeme;

/// Plus de lignes que cela : seules les premières sont décrites (un dossier de 50 000 fichiers ne bloque pas le dessin)
const MAX_LIGNES: usize = 1000;

#[derive(Default)]
struct Instantane {
    fenetre: isize,
    dossier: String,
    lignes: Vec<String>,
    /// Ligne qui a le focus du clavier
    focus: Option<usize>,
    statut: String,
    /// Nom lu de chaque pastille d'opération, dans l'ordre d'affichage
    pastilles: Vec<String>,
    rect: RECT,
    /// Ce qui a servi à construire l'instantané : (dossier, nombre d'éléments)
    cle: (String, usize),
}

static INSTANTANE: Mutex<Option<Instantane>> = Mutex::new(None);

fn lire<T>(f: impl FnOnce(&Instantane) -> T) -> Option<T> {
    INSTANTANE.lock().unwrap_or_else(|e| e.into_inner()).as_ref().map(f)
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum Id {
    Fenetre,
    Liste,
    Ligne(usize),
    Statut,
    Pastille(usize),
}

#[implement(IRawElementProviderSimple, IRawElementProviderFragment, IRawElementProviderFragmentRoot)]
struct Fournisseur {
    hwnd: isize,
    id: Id,
}

fn fournisseur(hwnd: isize, id: Id) -> IRawElementProviderFragment {
    Fournisseur { hwnd, id }.into()
}

/// Nombre de pastilles d'opération (0 sans instantané)
fn nombre_pastilles() -> usize {
    lire(|i| i.pastilles.len()).unwrap_or(0)
}

/// Nombre de lignes de la liste (0 sans instantané)
fn nombre() -> usize {
    lire(|i| i.lignes.len()).unwrap_or(0)
}

impl IRawElementProviderSimple_Impl for Fournisseur_Impl {
    fn ProviderOptions(&self) -> Result<ProviderOptions> {
        Ok(ProviderOptions_ServerSideProvider)
    }

    fn GetPatternProvider(&self, _: UIA_PATTERN_ID) -> Result<windows::core::IUnknown> {
        Err(E_FAIL.into())
    }

    fn GetPropertyValue(&self, propriete: UIA_PROPERTY_ID) -> Result<VARIANT> {
        let nom = || match self.id {
            Id::Fenetre => "SPACEFILES".to_string(),
            Id::Liste => lire(|i| i.dossier.clone()).unwrap_or_default(),
            Id::Ligne(k) => lire(|i| i.lignes.get(k).cloned()).flatten().unwrap_or_default(),
            Id::Statut => lire(|i| i.statut.clone()).unwrap_or_default(),
            Id::Pastille(k) => lire(|i| i.pastilles.get(k).cloned()).flatten().unwrap_or_default(),
        };
        let type_ = match self.id {
            Id::Fenetre => UIA_PaneControlTypeId,
            Id::Liste => UIA_ListControlTypeId,
            Id::Ligne(_) => UIA_ListItemControlTypeId,
            Id::Statut | Id::Pastille(_) => UIA_TextControlTypeId,
        };
        Ok(match propriete {
            UIA_NamePropertyId => VARIANT::from(BSTR::from(nom())),
            UIA_ControlTypePropertyId => VARIANT::from(type_.0),
            UIA_IsControlElementPropertyId | UIA_IsContentElementPropertyId | UIA_IsEnabledPropertyId => VARIANT::from(true),
            UIA_IsKeyboardFocusablePropertyId => VARIANT::from(matches!(self.id, Id::Ligne(_))),
            UIA_HasKeyboardFocusPropertyId => VARIANT::from(matches!(self.id, Id::Ligne(k) if lire(|i| i.focus == Some(k)) == Some(true))),
            UIA_AutomationIdPropertyId => VARIANT::from(BSTR::from(format!("{:?}", self.id))),
            _ => VARIANT::default(),
        })
    }

    fn HostRawElementProvider(&self) -> Result<IRawElementProviderSimple> {
        if self.id == Id::Fenetre {
            // SAFETY: la fenêtre existe tant que l'application tourne ; UIA ne garde que l'interface rendue
            unsafe { UiaHostProviderFromHwnd(HWND(self.hwnd as *mut _)) }
        } else {
            Err(E_FAIL.into())
        }
    }
}

impl IRawElementProviderFragment_Impl for Fournisseur_Impl {
    fn Navigate(&self, direction: NavigateDirection) -> Result<IRawElementProviderFragment> {
        let h = self.hwnd;
        let n = nombre();
        let p = nombre_pastilles();
        let autre = |id| Ok(fournisseur(h, id));
        // Enfants de la fenêtre : la liste, la barre d'état, puis les pastilles
        match (self.id, direction) {
            (Id::Fenetre, NavigateDirection_FirstChild) => autre(Id::Liste),
            (Id::Fenetre, NavigateDirection_LastChild) if p > 0 => autre(Id::Pastille(p - 1)),
            (Id::Fenetre, NavigateDirection_LastChild) => autre(Id::Statut),
            (Id::Liste, NavigateDirection_Parent) | (Id::Statut, NavigateDirection_Parent) | (Id::Pastille(_), NavigateDirection_Parent) => autre(Id::Fenetre),
            (Id::Liste, NavigateDirection_NextSibling) => autre(Id::Statut),
            (Id::Statut, NavigateDirection_PreviousSibling) => autre(Id::Liste),
            (Id::Statut, NavigateDirection_NextSibling) if p > 0 => autre(Id::Pastille(0)),
            (Id::Pastille(0), NavigateDirection_PreviousSibling) => autre(Id::Statut),
            (Id::Pastille(k), NavigateDirection_PreviousSibling) if k > 0 => autre(Id::Pastille(k - 1)),
            (Id::Pastille(k), NavigateDirection_NextSibling) if k + 1 < p => autre(Id::Pastille(k + 1)),
            (Id::Liste, NavigateDirection_FirstChild) if n > 0 => autre(Id::Ligne(0)),
            (Id::Liste, NavigateDirection_LastChild) if n > 0 => autre(Id::Ligne(n - 1)),
            (Id::Ligne(_), NavigateDirection_Parent) => autre(Id::Liste),
            (Id::Ligne(k), NavigateDirection_NextSibling) if k + 1 < n => autre(Id::Ligne(k + 1)),
            (Id::Ligne(k), NavigateDirection_PreviousSibling) if k > 0 => autre(Id::Ligne(k - 1)),
            _ => Err(E_FAIL.into()),
        }
    }

    fn GetRuntimeId(&self) -> Result<*mut SAFEARRAY> {
        let (genre, k) = match self.id {
            // La fenêtre prend son identifiant chez son hôte
            Id::Fenetre => return Ok(std::ptr::null_mut()),
            Id::Liste => (1, 0),
            Id::Statut => (2, 0),
            Id::Ligne(k) => (3, k as i32),
            Id::Pastille(k) => (4, k as i32),
        };
        // SAFETY: tableau de 3 entiers créé puis rempli ; sa propriété passe à UI Automation
        unsafe {
            let a = SafeArrayCreateVector(VT_I4, 0, 3);
            for (i, v) in [UiaAppendRuntimeId as i32, genre, k].iter().enumerate() {
                SafeArrayPutElement(a, &(i as i32), v as *const i32 as *const _)?;
            }
            Ok(a)
        }
    }

    fn BoundingRectangle(&self) -> Result<UiaRect> {
        // Une seule zone, celle de la fenêtre : le lecteur d'écran n'a besoin de la position que pour son contour
        let r = lire(|i| i.rect).unwrap_or_default();
        Ok(UiaRect { left: r.left as f64, top: r.top as f64, width: (r.right - r.left) as f64, height: (r.bottom - r.top) as f64 })
    }

    fn GetEmbeddedFragmentRoots(&self) -> Result<*mut SAFEARRAY> {
        Ok(std::ptr::null_mut())
    }

    fn SetFocus(&self) -> Result<()> {
        Ok(())
    }

    fn FragmentRoot(&self) -> Result<IRawElementProviderFragmentRoot> {
        use windows::core::Interface;
        fournisseur(self.hwnd, Id::Fenetre).cast()
    }
}

impl IRawElementProviderFragmentRoot_Impl for Fournisseur_Impl {
    fn ElementProviderFromPoint(&self, _: f64, _: f64) -> Result<IRawElementProviderFragment> {
        Ok(fournisseur(self.hwnd, Id::Liste))
    }

    fn GetFocus(&self) -> Result<IRawElementProviderFragment> {
        match lire(|i| i.focus) {
            Some(Some(k)) => Ok(fournisseur(self.hwnd, Id::Ligne(k))),
            _ => Err(E_FAIL.into()),
        }
    }
}

/// Réponse à WM_GETOBJECT : le fournisseur racine si c'est UI Automation qui le demande
pub fn demande(hwnd: HWND, wparam: WPARAM, lparam: LPARAM) -> Option<LRESULT> {
    if lparam.0 as i32 != UiaRootObjectId {
        return None;
    }
    use windows::core::Interface;
    let racine: IRawElementProviderSimple = fournisseur(hwnd.0 as isize, Id::Fenetre).cast().ok()?;
    // SAFETY: `racine` est une interface vivante, UIA en prend sa propre référence
    Some(unsafe { UiaReturnRawElementProvider(hwnd, wparam, lparam, &racine) })
}

/// Description d'une ligne : ce que le lecteur d'écran lit quand elle prend le focus
fn decrire(e: &crate::dossier::Entree) -> String {
    let type_ = icones::nom_type(e);
    if e.est_conteneur() || e.taille == 0 {
        format!("{}, {}", e.nom, type_)
    } else {
        format!("{}, {}, {}", e.nom, type_, systeme::taille(e.taille))
    }
}

/// Texte de la barre d'état dit au lecteur d'écran : l'annonce en cours, sinon le nombre d'éléments
fn statut(app: &App) -> String {
    match app.annonce.as_ref().filter(|a| a.2.elapsed() < crate::app::DUREE_ANNONCE) {
        Some((t, _, _)) => t.clone(),
        None => format!("{} ELEMENTS", systeme::nombre(app.entrees.len())),
    }
}

/// Ce que la page des réglages dit au lecteur d'écran : catégorie affichée, une ligne « titre, valeur » par carte, carte choisie
pub struct PageLue {
    pub categorie: String,
    pub cartes: Vec<String>,
    pub focus: Option<usize>,
}

/// À appeler à chaque dessin : met l'instantané à jour et prévient le lecteur d'écran de ce qui a changé.
/// `page` : la page des réglages quand elle est affichée (elle remplace alors la liste du dossier).
pub fn maj(hwnd: HWND, app: &App, pastilles: &[String], page: Option<&PageLue>) {
    // SAFETY: lecture d'un compteur global de UI Automation
    if !unsafe { UiaClientsAreListening() }.as_bool() {
        return;
    }
    let dossier = match page {
        Some(p) => format!("Réglages, {}", p.categorie),
        None => app.emplacement.titre(),
    };
    // Une carte dont la valeur change change la clé : le lecteur relit alors la liste
    let cle = match page {
        Some(p) => (format!("{dossier}
{}", p.cartes.join("
")), p.cartes.len()),
        None => (dossier.clone(), app.entrees.len()),
    };
    let focus = match page {
        Some(p) => p.focus,
        None => app.selection.filter(|&k| k < MAX_LIGNES),
    };
    let texte = statut(app);
    let (focus_change, statut_change, liste_change);
    {
        let mut g = INSTANTANE.lock().unwrap_or_else(|e| e.into_inner());
        let i = g.get_or_insert_with(Instantane::default);
        i.fenetre = hwnd.0 as isize;
        liste_change = i.cle != cle;
        if liste_change {
            i.lignes = match page {
                Some(p) => p.cartes.clone(),
                None => app.entrees.iter().take(MAX_LIGNES).map(decrire).collect(),
            };
            i.cle = cle;
            i.dossier = dossier;
        }
        focus_change = liste_change || i.focus != focus;
        i.focus = focus;
        statut_change = i.statut != texte;
        i.statut = texte.clone();
        i.pastilles = pastilles.to_vec();
        let mut r = RECT::default();
        // SAFETY: `r` est valide pour l'écriture
        let _ = unsafe { GetWindowRect(hwnd, &mut r) };
        i.rect = r;
    }
    use windows::core::Interface;
    let h = hwnd.0 as isize;
    // SAFETY: les fournisseurs sont créés pour l'appel et libérés après ; les chaînes vivent jusqu'au retour
    unsafe {
        if focus_change {
            if let Some(k) = focus {
                if let Ok(p) = fournisseur(h, Id::Ligne(k)).cast::<IRawElementProviderSimple>() {
                    let _ = UiaRaiseAutomationEvent(&p, UIA_AutomationFocusChangedEventId);
                }
            }
        }
        if statut_change {
            if let Ok(p) = fournisseur(h, Id::Statut).cast::<IRawElementProviderSimple>() {
                let _ = UiaRaiseNotificationEvent(&p, NotificationKind_Other, NotificationProcessing_ImportantMostRecent, &BSTR::from(texte), &BSTR::from("statut"));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arbre_de_fournisseurs() {
        use windows::core::Interface;
        *INSTANTANE.lock().unwrap() = Some(Instantane { lignes: vec!["a.txt, TXT".into(), "b".into()], focus: Some(1), dossier: "ESSAI".into(), statut: "2 ELEMENTS".into(), pastilles: vec!["Copie, 42 pour cent".into()], ..Default::default() });
        let racine = fournisseur(0, Id::Fenetre);
        let liste = unsafe { racine.Navigate(NavigateDirection_FirstChild) }.unwrap();
        let ligne = unsafe { liste.Navigate(NavigateDirection_FirstChild) }.unwrap();
        let suivante = unsafe { ligne.Navigate(NavigateDirection_NextSibling) }.unwrap();
        assert!(unsafe { suivante.Navigate(NavigateDirection_NextSibling) }.is_err());
        let simple: IRawElementProviderSimple = suivante.cast().unwrap();
        let nom = unsafe { simple.GetPropertyValue(UIA_NamePropertyId) }.unwrap();
        assert_eq!(nom.to_string(), "b");
        let focus = unsafe { simple.GetPropertyValue(UIA_HasKeyboardFocusPropertyId) }.unwrap();
        assert!(bool::try_from(&focus).unwrap());
        // Les pastilles d'opération sont des enfants de la fenêtre, après la barre d'état, avec un nom lisible
        let derniere: IRawElementProviderSimple = unsafe { racine.Navigate(NavigateDirection_LastChild) }.unwrap().cast().unwrap();
        let nom = unsafe { derniere.GetPropertyValue(UIA_NamePropertyId) }.unwrap();
        assert_eq!(nom.to_string(), "Copie, 42 pour cent");
        let avant = unsafe { fournisseur(0, Id::Pastille(0)).Navigate(NavigateDirection_PreviousSibling) }.unwrap();
        let avant: IRawElementProviderSimple = avant.cast().unwrap();
        assert_eq!(unsafe { avant.GetPropertyValue(UIA_NamePropertyId) }.unwrap().to_string(), "2 ELEMENTS", "la barre d'état précède la première pastille");
    }
}
