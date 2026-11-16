// UI Automation (V13) : la fenêtre se dessine toute seule en Direct2D, un lecteur d'écran n'y voit donc rien.
// Un petit arbre de fournisseurs la décrit : la fenêtre > la liste (une ligne par élément), la barre d'état et les pastilles
// d'opération (« Copie, 42 pour cent »).
// Les fournisseurs sont appelés depuis des fils de UI Automation : ils ne lisent qu'un instantané copié ici,
// jamais l'état de l'application.

// Les constantes de UI Automation gardent leur nom Windows (UIA_NamePropertyId…), même dans les motifs
#![allow(non_upper_case_globals)]

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

use windows::core::{implement, IUnknownImpl, Interface, Result, BSTR, VARIANT};
use windows::Win32::Foundation::{BOOL, E_FAIL, HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::System::Com::SAFEARRAY;
use windows::Win32::System::Ole::{SafeArrayCreateVector, SafeArrayDestroy, SafeArrayPutElement};
use windows::Win32::System::Variant::{VT_I4, VT_UNKNOWN};
use windows::Win32::UI::Accessibility::*;
use windows::Win32::UI::WindowsAndMessaging::{GetWindowRect, PostMessageW, WM_APP};

/// Un lecteur d'écran agit sur la liste (motifs Selection, SelectionItem et Invoke) : le fil de UI Automation ne touche jamais
/// à l'état de l'application, il poste ce message à la fenêtre. `wparam` : l'action (`ACTION_*`), `lparam` : la ligne.
pub const WM_UIA_ACTION: u32 = WM_APP + 50;
pub const ACTION_CHOISIR: usize = 0;
pub const ACTION_AJOUTER: usize = 1;
pub const ACTION_RETIRER: usize = 2;
pub const ACTION_OUVRIR: usize = 3;

use crate::app::App;
use crate::icones;
use crate::pastilles::{self, Bouton, Pastille};
use crate::systeme;

/// Ce qu'un lecteur d'écran sait d'une pastille : son nom lu, ses boutons nommés, et de quoi la reconnaître d'un dessin à
/// l'autre (clé + libellé) et repérer un changement d'état sans réagir à chaque pour cent d'avancement.
#[derive(Clone, Default, PartialEq, Debug)]
pub struct PastilleLue {
    pub nom: String,
    pub cle: u64,
    pub libelle: String,
    /// Rang parmi les pastilles de même clé et de même libellé (les fins et erreurs portent la clé 0 : deux onglets qui finissent
    /// « COPIE · 3 ÉLÉMENTS » ne se distinguent que par là)
    pub occurrence: usize,
    /// État grossier : genre, pause, phase, message. Le pourcentage et les vitesses n'y sont pas (un message par pour cent
    /// noierait le lecteur d'écran).
    pub etat: String,
    /// Boutons visibles de la pastille, avec leur nom lu
    pub boutons: Vec<(Bouton, String)>,
}

impl PastilleLue {
    pub fn depuis(p: &Pastille) -> Self {
        // Un téléchargement change de mot de droite à chaque tick (taille, vitesse) : seule sa phase compte
        let mot = if matches!(p.genre, pastilles::Genre::Telechargement(_)) { None } else { p.etiquette.as_deref() };
        PastilleLue {
            nom: p.nom_accessible(),
            cle: p.cle,
            libelle: p.libelle.clone(),
            occurrence: 0,
            etat: format!("{:?}|{}|{:?}|{:?}", p.genre, p.en_pause, mot, p.message),
            boutons: p.boutons.iter().map(|&b| (b, pastilles::nom_bouton(p, b).to_string())).collect(),
        }
    }

    /// Toutes les pastilles de la liste, avec leur rang d'occurrence
    pub fn lues<'a>(liste: impl IntoIterator<Item = &'a Pastille>) -> Vec<PastilleLue> {
        let mut v: Vec<PastilleLue> = liste.into_iter().map(PastilleLue::depuis).collect();
        for k in 0..v.len() {
            v[k].occurrence = v[..k].iter().filter(|o| o.meme(&v[k])).count();
        }
        v
    }

    /// Même pastille d'un dessin à l'autre ?
    fn meme(&self, autre: &PastilleLue) -> bool {
        self.cle == autre.cle && self.libelle == autre.libelle
    }
}

/// Ce qui a changé entre deux listes de pastilles : de quoi lever les événements UIA
#[derive(Default, PartialEq, Debug)]
struct Changements {
    /// Rangs (dans la nouvelle liste) des pastilles apparues
    apparues: Vec<usize>,
    /// Rangs des pastilles déjà là dont l'état a changé (fini, erreur, pause, phase)
    changees: Vec<usize>,
    /// L'ensemble des enfants de la fenêtre n'est plus le même (apparition, disparition, ordre ou boutons)
    structure: bool,
}

fn changements(avant: &[PastilleLue], apres: &[PastilleLue]) -> Changements {
    let mut c = Changements::default();
    // Chaque ancienne pastille ne sert qu'une fois : deux copies de même nom ne se confondent pas
    let mut prises = vec![false; avant.len()];
    for (k, n) in apres.iter().enumerate() {
        match avant.iter().enumerate().find(|(j, o)| !prises[*j] && o.meme(n)) {
            None => c.apparues.push(k),
            Some((j, o)) => {
                prises[j] = true;
                if o.etat != n.etat {
                    c.changees.push(k);
                }
            }
        }
    }
    c.structure = !c.apparues.is_empty()
        || prises.iter().any(|p| !p)
        || avant.len() != apres.len()
        || avant.iter().zip(apres).any(|(o, n)| !o.meme(n) || o.boutons != n.boutons);
    c
}

/// Plus de lignes que cela : seules les premières sont décrites (un dossier de 50 000 fichiers ne bloque pas le dessin)
const MAX_LIGNES: usize = 1000;

#[derive(Default)]
struct Instantane {
    fenetre: isize,
    dossier: String,
    lignes: Vec<String>,
    /// Ligne qui a le focus du clavier
    focus: Option<usize>,
    /// Lignes sélectionnées (la sélection multiple de la liste)
    choisies: Vec<usize>,
    statut: String,
    /// Chaque pastille, dans l'ordre d'affichage (celles que « +N » cache y sont aussi : un lecteur d'écran les atteint)
    pastilles: Vec<PastilleLue>,
    rect: RECT,
    /// Rectangle écran (pixels) de chaque ligne quand on sait le calculer (vue liste du dossier) ; vide sinon, la ligne prend alors
    /// le rectangle de la fenêtre. Une ligne hors de la zone visible porte un rectangle nul.
    rects: Vec<RECT>,
    /// Ce qui a servi à construire l'instantané : (dossier, nombre d'éléments)
    cle: (String, usize, u64),
    /// Nom réel de chaque ligne (sans la description lue) : une action n'est exécutée que si la ligne porte encore ce nom
    noms: Vec<String>,
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
    /// Bouton `j` de la pastille `k` (Pause, Annuler, Destination, Fermer…)
    Bouton(usize, usize),
}

#[implement(IRawElementProviderSimple, IRawElementProviderFragment, IRawElementProviderFragmentRoot, ISelectionProvider, ISelectionItemProvider, IInvokeProvider)]
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

    fn GetPatternProvider(&self, motif: UIA_PATTERN_ID) -> Result<windows::core::IUnknown> {
        let moi = self.to_object();
        match (self.id, motif) {
            (Id::Liste, UIA_SelectionPatternId) => moi.to_interface::<ISelectionProvider>().cast(),
            (Id::Ligne(_), UIA_SelectionItemPatternId) => moi.to_interface::<ISelectionItemProvider>().cast(),
            (Id::Ligne(_) | Id::Bouton(..), UIA_InvokePatternId) => moi.to_interface::<IInvokeProvider>().cast(),
            _ => Err(E_FAIL.into()),
        }
    }

    fn GetPropertyValue(&self, propriete: UIA_PROPERTY_ID) -> Result<VARIANT> {
        let nom = || match self.id {
            Id::Fenetre => "SPACEFILES".to_string(),
            Id::Liste => lire(|i| i.dossier.clone()).unwrap_or_default(),
            Id::Ligne(k) => lire(|i| i.lignes.get(k).cloned()).flatten().unwrap_or_default(),
            Id::Statut => lire(|i| i.statut.clone()).unwrap_or_default(),
            Id::Pastille(k) => lire(|i| i.pastilles.get(k).map(|p| p.nom.clone())).flatten().unwrap_or_default(),
            Id::Bouton(k, j) => lire(|i| i.pastilles.get(k).and_then(|p| p.boutons.get(j)).map(|(_, n)| n.clone())).flatten().unwrap_or_default(),
        };
        let type_ = match self.id {
            Id::Fenetre => UIA_PaneControlTypeId,
            Id::Liste => UIA_ListControlTypeId,
            Id::Ligne(_) => UIA_ListItemControlTypeId,
            Id::Statut => UIA_TextControlTypeId,
            // Une pastille qui porte des boutons est un groupe nommé, pas un simple texte
            Id::Pastille(_) => UIA_GroupControlTypeId,
            Id::Bouton(..) => UIA_ButtonControlTypeId,
        };
        Ok(match propriete {
            UIA_NamePropertyId => VARIANT::from(BSTR::from(nom())),
            UIA_ControlTypePropertyId => VARIANT::from(type_.0),
            UIA_IsControlElementPropertyId | UIA_IsContentElementPropertyId | UIA_IsEnabledPropertyId => VARIANT::from(true),
            UIA_IsOffscreenPropertyId => VARIANT::from(matches!(self.id, Id::Ligne(k) if lire(|i| i.rects.get(k).is_some_and(|r| r.right <= r.left)) == Some(true))),
            UIA_IsKeyboardFocusablePropertyId => VARIANT::from(matches!(self.id, Id::Ligne(_))),
            UIA_HasKeyboardFocusPropertyId => VARIANT::from(matches!(self.id, Id::Ligne(k) if lire(|i| i.focus == Some(k)) == Some(true))),
            UIA_SelectionItemIsSelectedPropertyId => VARIANT::from(matches!(self.id, Id::Ligne(k) if lire(|i| i.choisies.contains(&k)) == Some(true))),
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
        // Nombre de boutons de la pastille `k` : ce sont ses enfants
        let nb = |k: usize| lire(|i| i.pastilles.get(k).map_or(0, |p| p.boutons.len())).unwrap_or(0);
        // Enfants de la fenêtre : la liste, la barre d'état, puis les pastilles (chacune porte ses boutons)
        match (self.id, direction) {
            (Id::Pastille(k), NavigateDirection_FirstChild) if nb(k) > 0 => autre(Id::Bouton(k, 0)),
            (Id::Pastille(k), NavigateDirection_LastChild) if nb(k) > 0 => autre(Id::Bouton(k, nb(k) - 1)),
            (Id::Bouton(k, _), NavigateDirection_Parent) => autre(Id::Pastille(k)),
            (Id::Bouton(k, j), NavigateDirection_NextSibling) if j + 1 < nb(k) => autre(Id::Bouton(k, j + 1)),
            (Id::Bouton(k, j), NavigateDirection_PreviousSibling) if j > 0 => autre(Id::Bouton(k, j - 1)),
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
            // Une pastille porte au plus quatre boutons : k * 8 + j reste unique
            Id::Bouton(k, j) => (5, (k * 8 + j) as i32),
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
        // Une ligne de la vue liste a son propre rectangle (contour du lecteur d'écran, exploration tactile) ; le reste prend la fenêtre
        let r = lire(|i| match self.id {
            Id::Ligne(k) => i.rects.get(k).copied().unwrap_or(i.rect),
            _ => i.rect,
        })
        .unwrap_or_default();
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

/// Demande une action à la fenêtre (exécutée sur le fil d'interface)
/// Actions demandées par UI Automation, lues par la fenêtre dans l'ordre : (action, ligne, nom de la ligne quand le lecteur l'a vue)
static ACTIONS: Mutex<std::collections::VecDeque<(usize, usize, String)>> = Mutex::new(std::collections::VecDeque::new());

/// Prochaine action en attente (fil d'interface)
pub fn prendre_action() -> Option<(usize, usize, String)> {
    ACTIONS.lock().unwrap_or_else(|e| e.into_inner()).pop_front()
}

fn demander(hwnd: isize, action: usize, ligne: usize) -> Result<()> {
    // Une boucle modale (menu du Shell, glisser) tient l'état de la fenêtre : on refuse plutôt que d'agir dessous
    if crate::fenetre::modale_en_cours() {
        return Err(windows::core::HRESULT(UIA_E_ELEMENTNOTENABLED as i32).into());
    }
    // La ligne que le lecteur a lue : si l'ordre ou le contenu du dossier a changé depuis, la fenêtre ne fait rien
    let Some(nom) = lire(|i| i.noms.get(ligne).cloned()).flatten() else { return Err(windows::core::HRESULT(UIA_E_ELEMENTNOTAVAILABLE as i32).into()) };
    ACTIONS.lock().unwrap_or_else(|e| e.into_inner()).push_back((action, ligne, nom));
    // SAFETY: `PostMessageW` accepte n'importe quel fil ; une fenêtre détruite renvoie simplement une erreur
    let r = unsafe { PostMessageW(HWND(hwnd as *mut _), WM_UIA_ACTION, WPARAM(0), LPARAM(0)) };
    if r.is_err() {
        ACTIONS.lock().unwrap_or_else(|e| e.into_inner()).pop_back();
    }
    r
}

/// Clics de lecteur d'écran sur un bouton de pastille, lus par la fenêtre dans l'ordre : (clé, libellé, occurrence, bouton). La
/// pastille se retrouve par sa clé, son libellé et son rang d'occurrence, pas par son rang : la liste a pu changer entre le moment où le lecteur l'a lue et
/// celui où la fenêtre agit.
static ACTIONS_PASTILLES: Mutex<std::collections::VecDeque<(u64, String, usize, Bouton)>> = Mutex::new(std::collections::VecDeque::new());

/// Prochain clic de bouton de pastille en attente (fil d'interface)
pub fn prendre_action_pastille() -> Option<(u64, String, usize, Bouton)> {
    ACTIONS_PASTILLES.lock().unwrap_or_else(|e| e.into_inner()).pop_front()
}

/// Vrai quand une fenêtre modale (palette, menu, galerie…) cache les pastilles : `Invoke` le dit au lecteur d'écran au lieu de
/// répondre « fait » à une action que la fenêtre refuserait
static PASTILLES_CACHEES: AtomicBool = AtomicBool::new(false);

/// À appeler avant `maj` : une modale cache-t-elle les pastilles ?
pub fn pastilles_cachees(cachees: bool) {
    PASTILLES_CACHEES.store(cachees, Ordering::Relaxed);
}

/// Vrai quand l'instantané n'a pas suivi les dessins (aucun lecteur d'écran à l'écoute, ou premier dessin) : la comparaison des
/// pastilles ne vaut alors rien et ne doit rien annoncer
static INSTANTANE_PERIME: AtomicBool = AtomicBool::new(true);

fn demander_bouton(hwnd: isize, k: usize, j: usize) -> Result<()> {
    if crate::fenetre::modale_en_cours() || PASTILLES_CACHEES.load(Ordering::Relaxed) {
        return Err(windows::core::HRESULT(UIA_E_ELEMENTNOTENABLED as i32).into());
    }
    // Le bouton que le lecteur a lu : s'il n'existe plus dans l'instantané, l'élément n'est plus là
    let Some((cle, libelle, occurrence, bouton)) =
        lire(|i| i.pastilles.get(k).and_then(|p| p.boutons.get(j).map(|(b, _)| (p.cle, p.libelle.clone(), p.occurrence, *b)))).flatten()
    else {
        return Err(windows::core::HRESULT(UIA_E_ELEMENTNOTAVAILABLE as i32).into());
    };
    ACTIONS_PASTILLES.lock().unwrap_or_else(|e| e.into_inner()).push_back((cle, libelle, occurrence, bouton));
    // SAFETY: `PostMessageW` accepte n'importe quel fil ; une fenêtre détruite renvoie simplement une erreur
    let r = unsafe { PostMessageW(HWND(hwnd as *mut _), WM_UIA_ACTION, WPARAM(0), LPARAM(0)) };
    if r.is_err() {
        ACTIONS_PASTILLES.lock().unwrap_or_else(|e| e.into_inner()).pop_back();
    }
    r
}

impl ISelectionProvider_Impl for Fournisseur_Impl {
    fn GetSelection(&self) -> Result<*mut SAFEARRAY> {
        let choisies = lire(|i| i.choisies.clone()).unwrap_or_default();
        // SAFETY: tableau d'interfaces créé puis rempli ; `SafeArrayPutElement` prend sa propre référence, la sienne passe à UIA
        unsafe {
            let a = SafeArrayCreateVector(VT_UNKNOWN, 0, choisies.len() as u32);
            for (i, k) in choisies.iter().enumerate() {
                let mis = fournisseur(self.hwnd, Id::Ligne(*k)).cast::<IRawElementProviderSimple>().and_then(|p| SafeArrayPutElement(a, &(i as i32), p.as_raw()));
                if let Err(e) = mis {
                    // Le tableau et les références déjà posées seraient perdus
                    let _ = SafeArrayDestroy(a);
                    return Err(e);
                }
            }
            Ok(a)
        }
    }

    fn CanSelectMultiple(&self) -> Result<BOOL> {
        Ok(true.into())
    }

    fn IsSelectionRequired(&self) -> Result<BOOL> {
        Ok(false.into())
    }
}

impl ISelectionItemProvider_Impl for Fournisseur_Impl {
    fn Select(&self) -> Result<()> {
        match self.id {
            Id::Ligne(k) => demander(self.hwnd, ACTION_CHOISIR, k),
            _ => Err(E_FAIL.into()),
        }
    }

    fn AddToSelection(&self) -> Result<()> {
        match self.id {
            Id::Ligne(k) => demander(self.hwnd, ACTION_AJOUTER, k),
            _ => Err(E_FAIL.into()),
        }
    }

    fn RemoveFromSelection(&self) -> Result<()> {
        match self.id {
            Id::Ligne(k) => demander(self.hwnd, ACTION_RETIRER, k),
            _ => Err(E_FAIL.into()),
        }
    }

    fn IsSelected(&self) -> Result<BOOL> {
        Ok(matches!(self.id, Id::Ligne(k) if lire(|i| i.choisies.contains(&k)) == Some(true)).into())
    }

    fn SelectionContainer(&self) -> Result<IRawElementProviderSimple> {
        fournisseur(self.hwnd, Id::Liste).cast()
    }
}

impl IInvokeProvider_Impl for Fournisseur_Impl {
    fn Invoke(&self) -> Result<()> {
        match self.id {
            Id::Ligne(k) => demander(self.hwnd, ACTION_OUVRIR, k),
            Id::Bouton(k, j) => demander_bouton(self.hwnd, k, j),
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

/// À la destruction de la fenêtre : dit à UI Automation de lâcher les fournisseurs (`UiaReturnRawElementProvider` avec des
/// valeurs nulles, forme documentée de la déconnexion)
pub fn deconnecter(hwnd: HWND) {
    // SAFETY: hwnd est la fenêtre en cours de destruction ; WPARAM/LPARAM nuls et pas de fournisseur = déconnexion
    unsafe {
        let _ = UiaReturnRawElementProvider(hwnd, WPARAM(0), LPARAM(0), None::<&IRawElementProviderSimple>);
    }
}

/// Rectangle (x0, y0, x1, y1) en DIP, coordonnées du panneau, de l'élément `e` selon la vue : ligne de la liste, case de la grille
/// ou ligne de la colonne centrale des colonnes. Même géométrie que le dessin et le test de clic (`case_grille`,
/// `colonnes_miller`, `liste_y`, `hauteur_ligne`, `bord_droit_ligne`) ; non rogné.
pub(crate) fn rect_element(app: &App, l: f32, e: usize) -> [f32; 4] {
    use crate::app::{Affichage, GRILLE_ECART, GRILLE_NOM};
    let (haut, hl) = (crate::theme::liste_y(), crate::theme::hauteur_ligne());
    match app.affichage {
        Affichage::Grille => {
            let (par, hr) = app.pas();
            let (col, rang) = (e % par, e / par);
            let x = crate::rendu::grille_x0() + col as f32 * (app.cote_grille + GRILLE_ECART);
            let y = haut + rang as f32 * hr - app.defilement;
            [x, y, x + app.cote_grille, y + app.cote_grille + GRILLE_NOM]
        }
        Affichage::Colonnes => {
            let (cx, cw) = crate::theme::colonnes_miller(l)[1];
            let y = haut + e as f32 * hl - app.defilement;
            [cx, y, cx + cw, y + hl]
        }
        Affichage::Liste => {
            let y = haut + app.rang_de(e) as f32 * hl - app.defilement;
            [crate::theme::largeur_volet() + crate::theme::MARGE_LISTE - 6.0, y, crate::fenetre::souris::bord_droit_ligne(l), y + hl]
        }
    }
}

/// Rectangle (DIP) d'une carte de la page des réglages : la carte elle-même, ou la bande d'une ligne d'information
pub(crate) fn rect_carte_page(l: f32, y: f32, h: f32, info: bool) -> [f32; 4] {
    if info {
        [crate::page::gauche(), y, crate::page::droite(l), y + h]
    } else {
        let c = crate::page::rect_carte(y, l);
        [c.left, c.top, c.right, c.bottom]
    }
}

/// Rogne un rectangle à la zone visible de la liste (`haut`..`bas`) ; None s'il n'en reste rien
pub(crate) fn rogner(r: [f32; 4], haut: f32, bas: f32) -> Option<[f32; 4]> {
    let (a, b) = (r[1].max(haut), r[3].min(bas));
    (b > a).then_some([r[0], a, r[2], b])
}

/// Rectangles écran (pixels) des éléments donnés en DIP (coordonnées du panneau), rognés à la zone visible `haut`..`bas_marge`
/// avant le pied de page ; un élément entièrement hors de la zone reçoit un rectangle nul. `decalage` : x du panneau (DIP),
/// non nul pour le panneau de droite du double panneau.
fn vers_ecran(hwnd: HWND, geo: Geo, haut: f32, elements: impl Iterator<Item = [f32; 4]>) -> Vec<RECT> {
    use windows::Win32::Foundation::POINT;
    use windows::Win32::Graphics::Gdi::ClientToScreen;
    use windows::Win32::UI::WindowsAndMessaging::GetClientRect;
    let mut c = RECT::default();
    let mut o = POINT::default();
    // SAFETY: `c` et `o` sont valides pour l'écriture ; hwnd est la fenêtre qui dessine
    unsafe {
        if GetClientRect(hwnd, &mut c).is_err() || !ClientToScreen(hwnd, &mut o).as_bool() {
            return Vec::new();
        }
    }
    let k = geo.dpi / 96.0;
    let bas = (c.bottom - c.top) as f32 / k - crate::theme::statut_h();
    elements
        .map(|r| match rogner(r, haut, bas) {
            Some(r) => RECT { left: o.x + ((r[0] + geo.decalage) * k) as i32, top: o.y + (r[1] * k) as i32, right: o.x + ((r[2] + geo.decalage) * k) as i32, bottom: o.y + (r[3] * k) as i32 },
            None => RECT::default(),
        })
        .collect()
}

/// Géométrie du panneau actif pour les rectangles de lignes : DPI, largeur du panneau (DIP) et x de son bord gauche
/// (décalage du panneau de droite en double panneau)
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Geo {
    pub dpi: f32,
    pub largeur: f32,
    pub decalage: f32,
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
    /// Place de chaque carte dans la fenêtre : (y du haut en DIP, défilement déduit ; hauteur ; ligne d'information sans carte)
    pub places: Vec<(f32, f32, bool)>,
    pub categorie: String,
    pub cartes: Vec<String>,
    pub focus: Option<usize>,
}

/// À appeler à chaque dessin : met l'instantané à jour et prévient le lecteur d'écran de ce qui a changé.
/// `page` : la page des réglages quand elle est affichée (elle remplace alors la liste du dossier).
pub fn maj(hwnd: HWND, app: &App, pastilles: &[PastilleLue], page: Option<&PageLue>, geo: Option<Geo>) {
    // `geo` : géométrie du panneau actif quand les rectangles de lignes sont calculables
    // SAFETY: lecture d'un compteur global de UI Automation
    if !unsafe { UiaClientsAreListening() }.as_bool() {
        INSTANTANE_PERIME.store(true, Ordering::Relaxed);
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
")), p.cartes.len(), 0),
        None => {
            use std::hash::{Hash, Hasher};
            // Un tri ou un renommage garde le nombre d'éléments : l'empreinte des noms fait relire la liste
            let mut h = std::collections::hash_map::DefaultHasher::new();
            for e in app.entrees.iter().take(MAX_LIGNES) {
                e.nom.hash(&mut h);
            }
            (dossier.clone(), app.entrees.len(), h.finish())
        }
    };
    let focus = match page {
        Some(p) => p.focus,
        None => app.selection.filter(|&k| k < MAX_LIGNES),
    };
    let texte = statut(app);
    let (focus_change, statut_change, liste_change);
    // Pastilles apparues, changées d'état ou disparues depuis le dessin précédent : (changements, noms lus de la nouvelle liste)
    let pastilles_vues;
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
            i.noms = match page {
                Some(_) => Vec::new(),
                None => app.entrees.iter().take(MAX_LIGNES).map(|e| e.nom.clone()).collect(),
            };
            i.cle = cle;
            i.dossier = dossier;
        }
        focus_change = liste_change || i.focus != focus;
        i.focus = focus;
        i.choisies = match page {
            Some(_) => Vec::new(),
            None => {
                // La ligne au focus compte comme choisie seulement quand rien n'est marqué
                let mut v: Vec<usize> = app.marques.iter().copied().chain(app.selection.filter(|_| app.marques.is_empty())).filter(|&k| k < MAX_LIGNES).collect();
                v.sort_unstable();
                v.dedup();
                v
            }
        };
        statut_change = i.statut != texte;
        i.statut = texte.clone();
        // Au retour d'un lecteur d'écran, l'ancienne liste est périmée : on ne l'annonce pas comme « apparue »
        pastilles_vues = if INSTANTANE_PERIME.swap(false, Ordering::Relaxed) {
            Changements { structure: true, ..Changements::default() }
        } else {
            changements(&i.pastilles, pastilles)
        };
        i.pastilles = pastilles.to_vec();
        let mut r = RECT::default();
        // SAFETY: `r` est valide pour l'écriture
        let _ = unsafe { GetWindowRect(hwnd, &mut r) };
        i.rect = r;
        i.rects = match (page, geo) {
            (None, Some(g)) => vers_ecran(hwnd, g, crate::theme::liste_y(), (0..i.lignes.len()).map(|e| rect_element(app, g.largeur, e))),
            (Some(p), Some(g)) => vers_ecran(hwnd, g, crate::page::PAGE_Y, p.places.iter().map(|&(y, h, info)| rect_carte_page(g.largeur, y, h, info))),
            _ => Vec::new(),
        };
    }
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
        // Les pastilles apparaissent et disparaissent sans que rien ne prenne le focus : sans événement, un lecteur d'écran
        // ne saurait jamais qu'une copie vient de finir ou d'échouer. Structure d'abord (l'arbre a changé), puis une annonce
        // par pastille apparue (toutes, dans l'ordre) ou dont l'état a changé (seulement la plus récente).
        if pastilles_vues.structure {
            if let Ok(racine) = fournisseur(h, Id::Fenetre).cast::<IRawElementProviderSimple>() {
                let _ = UiaRaiseStructureChangedEvent(&racine, StructureChangeType_ChildrenInvalidated, std::ptr::null_mut(), 0);
            }
        }
        for (rangs, traitement) in [(&pastilles_vues.apparues, NotificationProcessing_All), (&pastilles_vues.changees, NotificationProcessing_ImportantMostRecent)] {
            for &k in rangs {
                if let (Ok(p), Some(l)) = (fournisseur(h, Id::Pastille(k)).cast::<IRawElementProviderSimple>(), pastilles.get(k)) {
                    let _ = UiaRaiseNotificationEvent(&p, NotificationKind_ActionCompleted, traitement, &BSTR::from(l.nom.as_str()), &BSTR::from("pastille"));
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// L'instantané et les files d'actions sont globaux : les essais qui les touchent passent l'un après l'autre
    static VERROU_ESSAI: Mutex<()> = Mutex::new(());
    fn verrou() -> std::sync::MutexGuard<'static, ()> {
        VERROU_ESSAI.lock().unwrap_or_else(|e| e.into_inner())
    }

    #[test]
    fn arbre_de_fournisseurs() {
        let _v = verrou();
        use windows::core::Interface;
        *INSTANTANE.lock().unwrap() = Some(Instantane { lignes: vec!["a.txt, TXT".into(), "b".into()], focus: Some(1), dossier: "ESSAI".into(), statut: "2 ELEMENTS".into(), pastilles: vec![PastilleLue { nom: "Copie, 42 pour cent".into(), ..Default::default() }], ..Default::default() });
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

        // Motifs : la liste expose Selection, une ligne SelectionItem et Invoke, le reste n'en expose aucun
        *INSTANTANE.lock().unwrap() = Some(Instantane { lignes: vec!["a".into(), "b".into(), "c".into()], noms: vec!["a".into(), "b".into(), "c".into()], choisies: vec![0, 2], ..Default::default() });
        let simple = |id| -> IRawElementProviderSimple { fournisseur(0, id).cast().unwrap() };
        assert!(unsafe { simple(Id::Liste).GetPatternProvider(UIA_SelectionPatternId) }.is_ok());
        assert!(unsafe { simple(Id::Liste).GetPatternProvider(UIA_InvokePatternId) }.is_err());
        assert!(unsafe { simple(Id::Ligne(1)).GetPatternProvider(UIA_SelectionItemPatternId) }.is_ok());
        assert!(unsafe { simple(Id::Ligne(1)).GetPatternProvider(UIA_InvokePatternId) }.is_ok());
        assert!(unsafe { simple(Id::Statut).GetPatternProvider(UIA_InvokePatternId) }.is_err());
        let liste: ISelectionProvider = unsafe { simple(Id::Liste).GetPatternProvider(UIA_SelectionPatternId) }.unwrap().cast().unwrap();
        assert!(unsafe { liste.CanSelectMultiple() }.unwrap().as_bool());
        assert!(!unsafe { liste.IsSelectionRequired() }.unwrap().as_bool());
        let choisie = |k| -> bool { let p: ISelectionItemProvider = unsafe { simple(Id::Ligne(k)).GetPatternProvider(UIA_SelectionItemPatternId) }.unwrap().cast().unwrap(); unsafe { p.IsSelected() }.unwrap().as_bool() };
        assert!(choisie(0) && !choisie(1) && choisie(2), "l'état de sélection vient de l'instantané");
        // Une action vers une fenêtre qui n'existe pas (handle 1) échoue proprement au lieu de toucher l'état
        let morte: IRawElementProviderSimple = fournisseur(1, Id::Ligne(1)).cast().unwrap();
        let p: IInvokeProvider = unsafe { morte.GetPatternProvider(UIA_InvokePatternId) }.unwrap().cast().unwrap();
        assert!(unsafe { p.Invoke() }.is_err());
    }

    fn lue(p: &Pastille) -> PastilleLue {
        PastilleLue::depuis(p)
    }

    /// Une pastille d'opération expose ses boutons comme enfants nommés, chacun avec le motif Invoke ; la pastille elle-même n'en a pas
    #[test]
    fn boutons_de_pastille_exposes_avec_invoke() {
        let _v = verrou();
        use windows::core::Interface;
        let copie = Pastille::operation("COPIE · 3 ÉLÉMENTS".into(), 0.42, false, Some(r"C:\dest".into())).avec_cle(7);
        let erreur = Pastille::erreur("DÉPLACEMENT · 1 ÉLÉMENT".into(), "accès refusé".into());
        *INSTANTANE.lock().unwrap() = Some(Instantane { pastilles: vec![lue(&copie), lue(&erreur)], ..Default::default() });
        let simple = |id| -> IRawElementProviderSimple { fournisseur(0, id).cast().unwrap() };
        let nom = |id| unsafe { simple(id).GetPropertyValue(UIA_NamePropertyId) }.unwrap().to_string();
        // Destination, Pause, Annuler dans l'ordre de la pastille ; l'erreur n'a que Fermer
        assert_eq!(nom(Id::Bouton(0, 0)), "Ouvrir le dossier de destination");
        assert_eq!(nom(Id::Bouton(0, 1)), "Mettre en pause");
        assert_eq!(nom(Id::Bouton(0, 2)), "Annuler l'opération");
        assert_eq!(nom(Id::Bouton(1, 0)), "Fermer");
        assert_eq!(nom(Id::Bouton(5, 0)), "", "un bouton absent n'a pas de nom");
        for id in [Id::Bouton(0, 0), Id::Bouton(0, 1), Id::Bouton(0, 2), Id::Bouton(1, 0)] {
            assert!(unsafe { simple(id).GetPatternProvider(UIA_InvokePatternId) }.is_ok(), "{id:?} : Invoke");
            let t = unsafe { simple(id).GetPropertyValue(UIA_ControlTypePropertyId) }.unwrap();
            assert_eq!(i32::try_from(&t).unwrap(), UIA_ButtonControlTypeId.0, "{id:?} : bouton");
        }
        assert!(unsafe { simple(Id::Pastille(0)).GetPatternProvider(UIA_InvokePatternId) }.is_err(), "la pastille n'est pas un bouton");
        // Navigation : pastille > boutons, frères dans l'ordre, retour au parent
        let premier = unsafe { fournisseur(0, Id::Pastille(0)).Navigate(NavigateDirection_FirstChild) }.unwrap();
        let deuxieme = unsafe { premier.Navigate(NavigateDirection_NextSibling) }.unwrap();
        let s: IRawElementProviderSimple = deuxieme.cast().unwrap();
        assert_eq!(unsafe { s.GetPropertyValue(UIA_NamePropertyId) }.unwrap().to_string(), "Mettre en pause");
        let dernier = unsafe { fournisseur(0, Id::Pastille(0)).Navigate(NavigateDirection_LastChild) }.unwrap();
        assert!(unsafe { dernier.Navigate(NavigateDirection_NextSibling) }.is_err());
        let parent: IRawElementProviderSimple = unsafe { dernier.Navigate(NavigateDirection_Parent) }.unwrap().cast().unwrap();
        assert_eq!(unsafe { parent.GetPropertyValue(UIA_NamePropertyId) }.unwrap().to_string(), "Copie, 42 pour cent");
        assert!(unsafe { fournisseur(0, Id::Pastille(1)).Navigate(NavigateDirection_FirstChild) }.is_ok(), "l'erreur a son bouton Fermer");
        *INSTANTANE.lock().unwrap() = Some(Instantane { pastilles: vec![lue(&Pastille::attente("COPIE".into()))], ..Default::default() });
        assert!(unsafe { fournisseur(0, Id::Pastille(0)).Navigate(NavigateDirection_FirstChild) }.is_err(), "une pastille en attente n'a pas de bouton");
        // Identifiants d'exécution distincts : un doublon ferait confondre deux boutons au lecteur d'écran
        let mut vus = std::collections::HashSet::new();
        for (k, j) in [(0usize, 0usize), (0, 1), (0, 2), (1, 0), (1, 1), (2, 0)] {
            assert!(vus.insert(k * 8 + j));
        }
    }

    /// Un Invoke sur un bouton vers une fenêtre qui n'existe pas échoue proprement et ne laisse rien en file ; un bouton qui n'existe
    /// plus répond « élément indisponible »
    #[test]
    fn invoke_de_bouton_sans_fenetre_ne_laisse_rien_en_file() {
        let _v = verrou();
        use windows::core::Interface;
        let copie = Pastille::operation("COPIE · 1 ÉLÉMENT".into(), 0.1, false, None).avec_cle(3);
        *INSTANTANE.lock().unwrap() = Some(Instantane { pastilles: vec![lue(&copie)], ..Default::default() });
        while prendre_action_pastille().is_some() {}
        let simple = |id| -> IRawElementProviderSimple { fournisseur(1, id).cast().unwrap() };
        let invoquer = |id| -> Result<()> { let p: IInvokeProvider = unsafe { simple(id).GetPatternProvider(UIA_InvokePatternId) }.unwrap().cast().unwrap(); unsafe { p.Invoke() } };
        assert!(invoquer(Id::Bouton(0, 0)).is_err(), "fenêtre morte (handle 1)");
        assert!(prendre_action_pastille().is_none(), "l'action est retirée quand le message n'a pas pu partir");
        let e = invoquer(Id::Bouton(0, 9)).unwrap_err();
        assert_eq!(e.code(), windows::core::HRESULT(UIA_E_ELEMENTNOTAVAILABLE as i32), "bouton absent");
        let e = invoquer(Id::Bouton(4, 0)).unwrap_err();
        assert_eq!(e.code(), windows::core::HRESULT(UIA_E_ELEMENTNOTAVAILABLE as i32), "pastille absente");
    }

    /// Deux fins de même libellé (clé 0, deux onglets) se distinguent par leur rang d'occurrence, et une modale refuse l'Invoke
    #[test]
    fn occurrences_et_modale() {
        let _v = verrou();
        use windows::core::Interface;
        let fin = Pastille::fin("COPIE · 3 ÉLÉMENTS".into(), Some(r"C:".into()));
        let autre = Pastille::fin("DÉPLACEMENT · 1 ÉLÉMENT".into(), None);
        let v = PastilleLue::lues(&[fin.clone(), autre.clone(), fin.clone()]);
        assert_eq!(v.iter().map(|p| p.occurrence).collect::<Vec<_>>(), vec![0, 0, 1]);
        *INSTANTANE.lock().unwrap() = Some(Instantane { pastilles: v, ..Default::default() });
        while prendre_action_pastille().is_some() {}
        pastilles_cachees(true);
        let simple: IRawElementProviderSimple = fournisseur(1, Id::Bouton(2, 0)).cast().unwrap();
        let p: IInvokeProvider = unsafe { simple.GetPatternProvider(UIA_InvokePatternId) }.unwrap().cast().unwrap();
        let e = unsafe { p.Invoke() }.unwrap_err();
        pastilles_cachees(false);
        assert_eq!(e.code(), windows::core::HRESULT(UIA_E_ELEMENTNOTENABLED as i32), "modale ouverte : refusé, pas « fait »");
        assert!(prendre_action_pastille().is_none());
    }

    #[test]
    fn changements_de_pastilles() {
        let copie = |part: f32| Pastille::operation("COPIE · 3 ÉLÉMENTS".into(), part, false, None);
        let fin = Pastille::fin("COPIE · 3 ÉLÉMENTS".into(), None);
        let erreur = Pastille::erreur("COPIE · 3 ÉLÉMENTS".into(), "disque plein".into());
        let l = |v: &[Pastille]| -> Vec<PastilleLue> { PastilleLue::lues(v) };
        // Rien avant, une pastille : elle apparaît
        let c = changements(&[], &l(&[copie(0.1)]));
        assert_eq!((c.apparues.as_slice(), c.changees.as_slice(), c.structure), (&[0][..], &[][..], true));
        // Le pourcentage seul ne dit rien (un message par pour cent noierait le lecteur d'écran), mais le nom lu suit
        let (a, b) = (l(&[copie(0.10)]), l(&[copie(0.11)]));
        assert_ne!(a[0].nom, b[0].nom);
        assert_eq!(changements(&a, &b), Changements::default());
        // Pause : changement d'état, et le bouton change de nom (donc de structure)
        let pause = Pastille::operation("COPIE · 3 ÉLÉMENTS".into(), 0.5, true, None);
        let c = changements(&l(&[copie(0.5)]), &l(&[pause.clone()]));
        assert_eq!(c.changees, vec![0]);
        assert!(c.apparues.is_empty() && c.structure);
        // Fini, puis échec : même libellé, état différent
        assert_eq!(changements(&l(&[copie(1.0)]), &l(&[fin.clone()])).changees, vec![0]);
        assert_eq!(changements(&l(&[copie(0.3)]), &l(&[erreur.clone()])).changees, vec![0]);
        // Disparition : la structure change, sans annonce
        let c = changements(&l(&[fin.clone()]), &[]);
        assert!(c.structure && c.apparues.is_empty() && c.changees.is_empty());
        // Deux pastilles de même libellé (cle 0) : chaque ancienne ne sert qu'une fois, la seconde est une apparition
        let c = changements(&l(&[fin.clone()]), &l(&[fin.clone(), fin.clone()]));
        assert_eq!(c.apparues, vec![1]);
        // Téléchargement : taille et vitesse changent à chaque tick sans événement ; fini = changement d'état
        let tele = |phase, mot: &str| Pastille::telechargement(phase, "photo.jpg".into(), mot.into(), 42);
        let en_cours = |mot: &str| tele(pastilles::PhaseTelechargement::EnCours, mot);
        assert_eq!(changements(&l(&[en_cours("1,0 mo")]), &l(&[en_cours("2,0 mo")])), Changements::default());
        let c = changements(&l(&[en_cours("2,0 mo")]), &l(&[tele(pastilles::PhaseTelechargement::Fini, "TÉLÉCHARGÉ · 2,0 mo")]));
        assert_eq!(c.changees, vec![0]);
    }

    /// Le centre du rectangle d'un élément doit retomber sur cet élément au test de clic, dans chaque vue : sinon un lecteur d'écran
    /// ou un outil de pointage désignerait une autre case
    #[test]
    fn rectangle_d_un_element_dans_chaque_vue() {
        use crate::app::Affichage;
        use crate::dossier::Emplacement;
        let l = 900.0;
        for vue in [Affichage::Liste, Affichage::Grille, Affichage::Colonnes] {
            let mut app = App::nouveau(Emplacement::CePC);
            app.affichage = vue;
            app.entrees = (0..40).map(|k| crate::dossier::entree_fichier(&format!("f{k}.txt"), 10, 0, false)).collect();
            app.par_rangee = 4;
            app.cote_grille = 120.0;
            for e in [0usize, 1, 5, 17] {
                let r = rect_element(&app, l, e);
                assert!(r[2] > r[0] && r[3] > r[1], "{vue:?} {e} : rectangle non vide");
                let (cx, cy) = ((r[0] + r[2]) / 2.0, (r[1] + r[3]) / 2.0);
                let touche = match vue {
                    Affichage::Grille => crate::rendu::case_grille(&app, cx, cy),
                    _ => {
                        let i = ((cy - crate::theme::liste_y() + app.defilement) / crate::theme::hauteur_ligne()) as usize;
                        (cx >= r[0] && cx < r[2]).then_some(i)
                    }
                };
                assert_eq!(touche, Some(e), "{vue:?} : le centre du rectangle de {e} doit viser {e}");
            }
        }
    }

    #[test]
    fn rogner_a_la_zone_visible() {
        assert_eq!(rogner([10.0, 100.0, 50.0, 140.0], 120.0, 500.0), Some([10.0, 120.0, 50.0, 140.0]), "coupée en haut");
        assert_eq!(rogner([10.0, 480.0, 50.0, 520.0], 120.0, 500.0), Some([10.0, 480.0, 50.0, 500.0]), "coupée en bas");
        assert_eq!(rogner([10.0, 0.0, 50.0, 40.0], 120.0, 500.0), None, "hors zone");
        assert_eq!(rogner([10.0, 600.0, 50.0, 640.0], 120.0, 500.0), None);
    }
}
