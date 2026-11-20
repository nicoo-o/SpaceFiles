// Fenêtre principale : l'état d'une fenêtre (struct Fenetre et ses méthodes), les messages privés et les petites aides
// de lecture de LPARAM, de touches et de taille. Le reste de la fenêtre est réparti dans les sous-modules.

use crate::*;

mod arbre;
mod clavier;
mod logique;
mod procedure;
mod session;
pub(crate) mod souris;
mod terminal_clavier;

pub(crate) use arbre::*;
pub(crate) use clavier::*;
pub(crate) use procedure::*;
pub(crate) use session::*;
pub(crate) use souris::*;
pub(crate) use terminal_clavier::*;
pub(crate) const WM_MOUSELEAVE: u32 = 0x02A3;

/// Vérification d'un stockage ajouté terminée (LPARAM = Box<Result<String, String>>)
pub(crate) const WM_STOCKAGE: u32 = WM_APP + 6;

/// Le shell du panneau terminal a produit de la sortie ou s'est terminé : la fenêtre vide la session dans l'émulateur
/// (`Panneau::pomper`) et redessine. Posté par un fil de la session ; sans paramètre (le panneau est dans `Fenetre`).
pub(crate) const WM_TERMINAL: u32 = WM_APP + 60;

/// Un fil court a déposé le chemin reçu par `explorer.open {reuse}` dans `OUVERTURES` : la fenêtre l'ouvre en onglet.
/// Aucun paramètre (ni pointeur). Voir `recevoir_ouvrir_ici`.
pub(crate) const WM_OUVRIR_ICI: u32 = WM_APP + 31;
/// Fin de la vérification des dossiers d'une charge `ONGLETS_COPIES` (fil court de `recevoir_onglets_copies`) : sans pointeur,
/// la fenêtre prend ses dossiers dans `ONGLETS_RECUS`
pub(crate) const WM_ONGLETS_COPIES: u32 = WM_APP + 34;

/// Boucles modales de Windows en cours (menu du Shell, glisser vers une autre application, boîte de choix, « Ouvrir avec… ») :
/// leur boucle de messages distribue pendant que l'appelant tient parfois un `&mut Fenetre`. Un message qui ouvrirait un
/// second `&mut Fenetre` (via `etat_de`) en même temps serait un comportement indéfini : `retenir_si_modale`, appelée en tête de
/// la procédure, le garde donc de côté et la fin de la modale (`GardeModale`) le reposte.
static MODALES: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
/// Parmi `MODALES`, celles qui sont des glisser-déposer internes (`GardeModale::nouvelle_glisser`)
static GLISSES: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
/// Un `WM_DPICHANGED` est arrivé pendant une modale : `WM_APPARENCE` applique la nouvelle échelle à l'état une fois la modale finie
pub(crate) static ECHELLE_EN_ATTENTE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
/// Un `WM_PAINT` a été validé sans dessin pendant une modale : un nouveau est demandé à sa fin
static REDESSIN_EN_ATTENTE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Ce que la procédure fait d'un message qui arrive pendant une modale
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SortModale {
    /// Traité normalement (il ne prend pas l'état de la fenêtre, ou une modale ne peut pas le produire)
    Passer,
    /// Gardé, puis reposté à la fin de la modale (charge utile conservée)
    Retenir,
    /// `WM_PAINT` : zone validée sans dessin, redessin demandé à la fin
    Valider,
    /// Souris, clavier : la fenêtre n'est pas censée en recevoir pendant une modale (menu, propriétaire désactivé) ; s'il en
    /// vient un quand même, il est perdu plutôt que de prendre l'état (résultat 0)
    Ignorer,
    /// Réponse synchrone qui ne se retient pas (zone sous le pointeur, curseur, objet d'accessibilité) : traitée par Windows
    /// (`DefWindowProc`) sans toucher à l'état de la fenêtre
    Systeme,
    /// Message dont `lparam` n'est valable que pendant l'appel (pointeur de texte) : seul le signal est gardé, avec `lparam` à
    /// 0, puis `DefWindowProc` répond
    Signaler,
    /// `WM_DPICHANGED` : le rectangle proposé (`lparam`) n'est valable que pendant l'appel, donc la fenêtre est replacée tout de
    /// suite (sans toucher à l'état) et le changement d'échelle est rejoué à la fin (`ECHELLE_EN_ATTENTE`)
    Echelle,
}

/// Table de décision, pure (testée). Tous les messages privés `WM_APP..` viennent d'un fil ou d'un `PostMessage` et prennent
/// l'état de la fenêtre : ils attendent. Les minuteries aussi (un tick retenu est unique, voir `FileRetenue`).
pub(crate) fn sort_en_modale(msg: u32) -> SortModale {
    match msg {
        WM_PAINT => SortModale::Valider,
        // WM_CLOSE : le laisser passer détruirait la fenêtre (WM_DESTROY libère `Fenetre`) sous l'appelant de la modale
        WM_TIMER | WM_SIZE | WM_CLOSE => SortModale::Retenir,
        // WM_NCHITTEST, WM_SETCURSOR, WM_GETOBJECT : lisent l'état pour répondre tout de suite, donc ne peuvent pas attendre
        WM_NCHITTEST | WM_SETCURSOR | WM_GETOBJECT => SortModale::Systeme,
        // WM_SETTINGCHANGE : `lparam` pointe sur un texte que Windows reprend au retour
        WM_SETTINGCHANGE => SortModale::Signaler,
        WM_DPICHANGED => SortModale::Echelle,
        // Clavier (0x100..=0x109), souris (0x200..=0x20E), menu contextuel (0x7B). `WM_MOUSELEAVE` (0x2A3) est retenu : le perdre
        // laisserait le survol collé une fois la fenêtre revenue (le suivi de souris ne se réarme qu'à lui)
        0x0100..=0x0109 | 0x0200..=0x020E | 0x007B => SortModale::Ignorer,
        0x02A3 => SortModale::Retenir,
        // Ne touchent pas `Fenetre` : le premier rend le focus au clavier, le second remplit une table indépendante
        crate::hote_apercu::WM_HOTE_FOCUS | crate::arbre::WM_ARBRE => SortModale::Passer,
        m if (WM_APP..0xC000).contains(&m) => SortModale::Retenir,
        _ => SortModale::Passer,
    }
}

/// Pendant un glisser-déposer interne (`SHDoDragDrop`), l'appelant ne se sert plus de son `&mut Fenetre` après l'appel (les deux
/// sites rendent la main juste après, ou relisent l'état par `etat_de`) : le dessin et les minuteries (ouverture d'un dossier
/// survolé, surbrillance de la cible de dépôt) continuent donc. Tout le reste suit la table ordinaire.
pub(crate) fn sort_en_glisse(msg: u32) -> SortModale {
    match msg {
        WM_PAINT | WM_TIMER => SortModale::Passer,
        _ => sort_en_modale(msg),
    }
}

/// File des messages retenus : (message, wparam, lparam). Un message sans charge (`lparam == 0`) déjà en attente n'est pas
/// doublé (un tick de minuterie ou un « le dossier a changé » retenu dix fois n'agirait qu'une fois) ; un message qui porte un
/// pointeur est toujours gardé (chaque boîte doit être reçue une fois). Deux messages sont fusionnés en place : la taille (la
/// dernière gagne) et la progression d'une opération (le dernier millième gagne), qui sinon s'empileraient par milliers pendant
/// un menu ouvert longtemps.
#[derive(Default)]
pub(crate) struct FileRetenue(Vec<(u32, usize, isize)>);

impl FileRetenue {
    pub(crate) const fn vide() -> Self {
        FileRetenue(Vec::new())
    }
    pub(crate) fn empiler(&mut self, msg: u32, wparam: usize, lparam: isize) {
        if lparam == 0 && self.0.contains(&(msg, wparam, 0)) {
            return;
        }
        let fusionne = |e: &&mut (u32, usize, isize)| e.0 == msg && (msg == WM_SIZE || (msg == WM_PROGRES && e.2 == lparam));
        if let Some(e) = self.0.iter_mut().find(fusionne) {
            *e = (msg, wparam, lparam);
            return;
        }
        self.0.push((msg, wparam, lparam));
    }
    pub(crate) fn vider(&mut self) -> Vec<(u32, usize, isize)> {
        std::mem::take(&mut self.0)
    }
}

static RETENUS: std::sync::Mutex<FileRetenue> = std::sync::Mutex::new(FileRetenue::vide());

/// Posée autour d'un appel modal ; la détruire à la sortie reposte ce qui a été retenu.
/// Fil d'interface seulement (les compteurs sont atomiques, mais le repost vise la fenêtre de ce fil).
pub(crate) struct GardeModale(HWND, bool);

impl GardeModale {
    pub(crate) fn nouvelle(hwnd: HWND) -> Self {
        MODALES.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        GardeModale(hwnd, false)
    }
    /// Pour le glisser-déposer interne seulement (voir `sort_en_glisse` : l'appelant ne réutilise pas son `&mut Fenetre`)
    pub(crate) fn nouvelle_glisser(hwnd: HWND) -> Self {
        MODALES.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        GLISSES.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        GardeModale(hwnd, true)
    }
}

impl Drop for GardeModale {
    fn drop(&mut self) {
        if self.1 {
            GLISSES.fetch_sub(1, std::sync::atomic::Ordering::SeqCst);
        }
        if MODALES.fetch_sub(1, std::sync::atomic::Ordering::SeqCst) == 1 {
            let retenus = RETENUS.lock().unwrap_or_else(|e| e.into_inner()).vider();
            for (msg, w, l) in retenus {
                // SAFETY: `l` est soit 0, soit le pointeur de la boîte que l'expéditeur a donnée à ce message ; si l'envoi échoue
                // (fenêtre détruite) personne ne la recevra et `liberer_charge` la reprend avec son vrai type
                unsafe {
                    if PostMessageW(self.0, msg, WPARAM(w), LPARAM(l)).is_err() && l != 0 {
                        liberer_charge(msg, l);
                    }
                }
            }
            if REDESSIN_EN_ATTENTE.swap(false, std::sync::atomic::Ordering::SeqCst) {
                // SAFETY: sans effet de bord sur nos données ; échec silencieux si la fenêtre n'existe plus
                unsafe {
                    let _ = InvalidateRect(self.0, None, false);
                }
            }
        }
    }
}

/// Rend une boîte retenue qu'on n'a pas pu reposter, selon le message qui la portait. Un message dont `lparam` n'est pas un
/// pointeur (`WM_PROGRES`, numéro d'opération) tombe dans `_` : rien à libérer.
unsafe fn liberer_charge(msg: u32, p: isize) {
    match msg {
        WM_PALETTE_RECHERCHE => drop(Box::from_raw(p as *mut (String, Option<Vec<palette::Trouve>>))),
        WM_PALETTE_LECTURE => drop(Box::from_raw(p as *mut assistant::Lecture)),
        WM_PALETTE_TACHE | WM_OUTIL => drop(Box::from_raw(p as *mut Fini)),
        WM_LECTURE => drop(Box::from_raw(p as *mut Lecture)),
        WM_GIT => drop(Box::from_raw(p as *mut (u64, Option<Git>))),
        WM_FOND => drop(Box::from_raw(p as *mut MessageFond)),
        etats::WM_ETATS => drop(Box::from_raw(p as *mut Vec<(String, etats::Etat)>)),
        ocr::WM_OCR => drop(Box::from_raw(p as *mut ocr::Resultat)),
        WM_VIGNETTE => drop(Box::from_raw(p as *mut (String, Option<Image>))),
        WM_GALERIE => drop(Box::from_raw(p as *mut crate::apercu::PageGalerie)),
        WM_OPERATION_FINIE => drop(Box::from_raw(p as *mut Bilan)),
        WM_STOCKAGE => drop(Box::from_raw(p as *mut std::result::Result<String, String>)),
        WM_APERCU => drop(Box::from_raw(p as *mut Apercu)),
        _ => {}
    }
}

/// Une boucle modale tient-elle l'état de la fenêtre ? (UI Automation refuse alors ses actions)
pub(crate) fn modale_en_cours() -> bool {
    MODALES.load(std::sync::atomic::Ordering::SeqCst) > 0
}

/// Appelée en tête de la procédure, avant tout `etat_de`. `Some(résultat)` = le message est réglé ici (retenu ou validé) ;
/// `None` = la procédure le traite normalement. Hors modale : toujours `None`, au prix d'une lecture atomique.
pub(crate) unsafe fn retenir_si_modale(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> Option<LRESULT> {
    if !modale_en_cours() {
        return None;
    }
    // Que des glisser-déposer en cours : table allégée ; une autre modale (menu, boîte) en même temps : table complète
    let sort = if MODALES.load(std::sync::atomic::Ordering::SeqCst) == GLISSES.load(std::sync::atomic::Ordering::SeqCst) {
        sort_en_glisse(msg)
    } else {
        sort_en_modale(msg)
    };
    match sort {
        SortModale::Passer => None,
        SortModale::Retenir => {
            // Le `lparam` d'une minuterie est un pointeur de rappel (nul ici) : forcé à 0 pour que le tick retenu soit unique
            let l = if msg == WM_TIMER { 0 } else { lparam.0 };
            RETENUS.lock().unwrap_or_else(|e| e.into_inner()).empiler(msg, wparam.0, l);
            Some(LRESULT(0))
        }
        SortModale::Echelle => {
            // Un message forgé par un autre processus peut porter un pointeur nul (`panic = abort` : on l'écarte)
            if lparam.0 == 0 {
                return Some(LRESULT(0));
            }
            // SAFETY: pour WM_DPICHANGED, `lparam` pointe sur un RECT valable pendant l'appel (contrat de Windows)
            let r = &*(lparam.0 as *const RECT);
            let _ = SetWindowPos(hwnd, None, r.left, r.top, r.right - r.left, r.bottom - r.top, SWP_NOZORDER | SWP_NOACTIVATE);
            ECHELLE_EN_ATTENTE.store(true, std::sync::atomic::Ordering::SeqCst);
            RETENUS.lock().unwrap_or_else(|e| e.into_inner()).empiler(WM_APPARENCE, 0, 0);
            Some(LRESULT(0))
        }
        SortModale::Ignorer => Some(LRESULT(0)),
        SortModale::Systeme => Some(DefWindowProcW(hwnd, msg, wparam, lparam)),
        SortModale::Signaler => {
            RETENUS.lock().unwrap_or_else(|e| e.into_inner()).empiler(msg, wparam.0, 0);
            Some(DefWindowProcW(hwnd, msg, wparam, lparam))
        }
        SortModale::Valider => {
            // Sans BeginPaint/EndPaint la zone resterait sale et Windows renverrait WM_PAINT sans fin pendant la modale
            let mut ps = PAINTSTRUCT::default();
            BeginPaint(hwnd, &mut ps);
            let _ = EndPaint(hwnd, &ps);
            REDESSIN_EN_ATTENTE.store(true, std::sync::atomic::Ordering::SeqCst);
            Some(LRESULT(0))
        }
    }
}

#[cfg(test)]
mod tests_modale {
    use super::*;

    #[test]
    fn les_messages_prives_sont_retenus() {
        // Les quatorze qui n'avaient aucune retenue, plus ceux qui en avaient une éparpillée
        for m in [
            WM_PROGRES, WM_CHANGEMENT, WM_LECTURE, WM_VIGNETTE, WM_GALERIE, WM_OPERATION_FINIE, etats::WM_ETATS, index::WM_INDEX, WM_GIT, WM_FOND, WM_OUTIL, ocr::WM_OCR, WM_STOCKAGE, WM_TERMINAL,
            WM_APERCU, WM_APPARENCE, minuteries::WM_IMAGE_ANIMATION, navigateur::WM_TELECHARGEMENT, WM_OUVRIR_ICI, WM_ONGLETS_COPIES, WM_PALETTE_RECHERCHE, WM_PALETTE_LECTURE, WM_PALETTE_TACHE,
            accessibilite::WM_UIA_ACTION,
        ] {
            assert_eq!(sort_en_modale(m), SortModale::Retenir, "message {m:#x}");
        }
    }

    #[test]
    fn minuterie_retenue_paint_valide_le_reste_passe() {
        assert_eq!(sort_en_modale(WM_TIMER), SortModale::Retenir);
        assert_eq!(sort_en_modale(WM_PAINT), SortModale::Valider);
        // Ne touchent pas `Fenetre`
        assert_eq!(sort_en_modale(crate::hote_apercu::WM_HOTE_FOCUS), SortModale::Passer);
        assert_eq!(sort_en_modale(crate::arbre::WM_ARBRE), SortModale::Passer);
        // Hors plage privée : clavier, taille, WM_COPYDATA (a son propre refus), messages enregistrés (>= 0xC000)
        for m in [WM_COPYDATA, WM_DESTROY, WM_QUERYENDSESSION, WM_ENDSESSION, WM_ACTIVATE, WM_ERASEBKGND, 0xC000, 0xC123] {
            assert_eq!(sort_en_modale(m), SortModale::Passer, "message {m:#x}");
        }
    }

    #[test]
    fn fermeture_echelle_et_sortie_de_souris() {
        // Fermer pendant une modale détruirait `Fenetre` sous l'appelant : retenu, rejoué à la fin
        assert_eq!(sort_en_modale(WM_CLOSE), SortModale::Retenir);
        // Le rectangle de WM_DPICHANGED ne survit pas à l'appel : traitement dédié
        assert_eq!(sort_en_modale(WM_DPICHANGED), SortModale::Echelle);
        // Perdre WM_MOUSELEAVE laisserait le survol collé
        assert_eq!(sort_en_modale(0x02A3), SortModale::Retenir);
    }

    #[test]
    fn le_glisser_garde_le_dessin_et_les_minuteries() {
        assert_eq!(sort_en_glisse(WM_PAINT), SortModale::Passer);
        assert_eq!(sort_en_glisse(WM_TIMER), SortModale::Passer);
        // Le reste suit la table ordinaire
        for m in [WM_PROGRES, WM_CLOSE, WM_SIZE, WM_DPICHANGED, WM_KEYDOWN, WM_NCHITTEST, WM_SETTINGCHANGE] {
            assert_eq!(sort_en_glisse(m), sort_en_modale(m), "message {m:#x}");
        }
    }

    #[test]
    fn taille_et_progression_sont_fusionnees() {
        let mut f = FileRetenue::vide();
        f.empiler(WM_PROGRES, 100, 1);
        f.empiler(WM_SIZE, 0, 0x0100_0100);
        f.empiler(WM_PROGRES, 500, 2);
        f.empiler(WM_PROGRES, 900, 1);
        f.empiler(WM_SIZE, 0, 0x0200_0200);
        // Une entrée par opération et une seule taille, à la place de la première, avec la dernière valeur
        assert_eq!(f.vider(), vec![(WM_PROGRES, 900, 1), (WM_SIZE, 0, 0x0200_0200), (WM_PROGRES, 500, 2)]);
    }

    #[test]
    fn entrees_reponses_synchrones_et_taille() {
        // Clavier : jamais l'état pendant une modale
        for m in [WM_KEYDOWN, WM_SYSKEYDOWN, WM_CHAR, WM_KEYUP] {
            assert_eq!(sort_en_modale(m), SortModale::Ignorer, "message {m:#x}");
        }
        // Les bornes de la plage des entrées de pointeur (0x200..=0x20E) et les deux messages isolés
        for m in [0x0200u32, 0x0201, 0x020A, 0x020E, 0x007B] {
            assert_eq!(sort_en_modale(m), SortModale::Ignorer, "message {m:#x}");
        }
        assert_eq!(sort_en_modale(0x020F), SortModale::Passer);
        // Réponse immédiate exigée : Windows répond, l'état n'est pas pris
        for m in [WM_NCHITTEST, WM_SETCURSOR, WM_GETOBJECT] {
            assert_eq!(sort_en_modale(m), SortModale::Systeme, "message {m:#x}");
        }
        assert_eq!(sort_en_modale(WM_SETTINGCHANGE), SortModale::Signaler);
        // La taille est rejouée à la fin dans l'ordre d'arrivée : la dernière gagne
        assert_eq!(sort_en_modale(WM_SIZE), SortModale::Retenir);
    }

    #[test]
    fn la_file_garde_l_ordre_et_chaque_charge() {
        let mut f = FileRetenue::vide();
        f.empiler(WM_LECTURE, 0, 0x1000);
        f.empiler(WM_LECTURE, 0, 0x2000);
        f.empiler(WM_OUTIL, 0, 0x3000);
        // Deux boîtes distinctes du même message : les deux sont gardées, dans l'ordre d'arrivée
        assert_eq!(f.vider(), vec![(WM_LECTURE, 0, 0x1000), (WM_LECTURE, 0, 0x2000), (WM_OUTIL, 0, 0x3000)]);
        assert!(f.vider().is_empty(), "la file se vide une seule fois");
    }

    #[test]
    fn un_message_sans_charge_n_est_pas_double() {
        let mut f = FileRetenue::vide();
        for _ in 0..5 {
            f.empiler(WM_TIMER, 7, 0);
            f.empiler(WM_CHANGEMENT, 3, 0);
        }
        f.empiler(WM_TIMER, 8, 0);
        // Même message avec un autre wparam : distinct
        assert_eq!(f.vider(), vec![(WM_TIMER, 7, 0), (WM_CHANGEMENT, 3, 0), (WM_TIMER, 8, 0)]);
    }
}

pub(crate) struct Fenetre {
    pub(crate) rendu: Rendu,
    /// Onglet affiché
    pub(crate) app: App,
    /// Autres onglets, dans l'ordre d'affichage (sans l'onglet affiché)
    pub(crate) fond: Vec<App>,
    /// Position de l'onglet affiché dans la barre
    pub(crate) rang: usize,
    pub(crate) dpi: f32,
    pub(crate) suivi_souris: bool,
    pub(crate) anime: bool,
    pub(crate) apercu_visible: bool,
    pub(crate) apercu: Option<Apercu>,
    /// Chemin dont l'aperçu a été demandé, et numéro de la demande
    pub(crate) apercu_demande: Option<String>,
    pub(crate) apercu_numero: u64,
    pub(crate) apercu_attente: Option<(String, Option<Entree>)>,
    /// Panneau terminal intégré (bouton `>_` du chemin ou Ctrl+Alt+T) : un panneau par onglet (T4a), chacun avec son shell ;
    /// vide = panneau fermé. Fermer un onglet détruit son panneau (et tue sa session), aucun shell ne tourne tant que le
    /// panneau est fermé
    pub(crate) terminaux: Vec<Box<crate::terminal::panneau::Panneau>>,
    /// Rang de l'onglet affiché dans `terminaux` (toujours valide quand la liste n'est pas vide)
    pub(crate) terminal_actif: usize,
    /// T4b : dernier dossier de la liste que le terminal connaît (déjà tapé au shell, ou venu de lui). Sert à ne réagir
    /// qu'à un changement : la liste ne renvoie pas au shell un dossier que le shell vient de lui donner
    pub(crate) terminal_dossier_liste: Option<String>,
    /// T4c : l'onglet JOURNAL du panneau est affiché à la place du shell actif (qui continue de tourner, caché). Le journal est
    /// en lecture seule : ni clavier, ni sélection, ni molette ne vont au shell tant qu'il est là
    pub(crate) terminal_journal: bool,
    /// T4c : dernières opérations lues dans `operations.csv`, et le compteur d'écritures (`journal::ecritures`) au moment de la
    /// lecture : `usize::MAX` = à relire (ouverture de l'onglet), sinon on relit seulement quand une opération a été écrite
    pub(crate) terminal_operations: Vec<crate::journal::Operation>,
    pub(crate) terminal_operations_version: usize,
    /// T5 : commandes tapées dans le terminal (suggestion grise, rappel par ↑), partagées par tous les onglets. Lu dans
    /// `%APPDATA%\SpaceFiles\historique-terminal.txt` à la première frappe, pas au démarrage (`historique_terminal_charge`)
    pub(crate) historique_terminal: crate::terminal::historique::Historique,
    pub(crate) historique_terminal_charge: bool,
    /// Le clavier va au shell (clic dans la grille) plutôt qu'à l'explorateur
    pub(crate) terminal_focus: bool,
    /// Un glissement de la souris sélectionne du texte dans la grille du terminal (la souris est capturée)
    pub(crate) terminal_glisse: bool,
    /// T3 : le bouton gauche est tenu dans la grille et c'est le programme du terminal (vim, htop…) qui reçoit le
    /// glissement, pas la sélection de texte (la souris est capturée)
    pub(crate) terminal_glisse_appli: bool,
    /// T3 : fraction de cran de molette pas encore envoyée au programme (pavé tactile de précision)
    pub(crate) terminal_molette_reste: f32,
    pub(crate) par_defaut: bool,
    /// Page des réglages (commune à tous les onglets)
    pub(crate) page: Page,
    /// Page ESPACE (analyse de l'espace disque) : l'état n'existe que tant que la page est affichée
    pub(crate) espace: espace::Etat,
    /// Cloud détecté (OneDrive, Google Drive…), relu avec les disques
    pub(crate) detectes: Vec<Lieu>,
    /// États mesurés des lieux du volet (lot 14b), la mesure en cours et l'heure de la dernière
    pub(crate) etats: crate::etats::Cache,
    pub(crate) etats_en_cours: bool,
    pub(crate) etats_mesure: Option<std::time::Instant>,
    /// Résident Win+E activé
    pub(crate) win_e: bool,
    /// Double panneau (F6) : l'autre panneau ; `app` est toujours celui qui a le focus
    pub(crate) second: Option<App>,
    /// Le panneau qui a le focus est celui de droite
    pub(crate) focus_droite: bool,
    /// Une touche a été utilisée depuis le dernier clic : le focus clavier se voit (anneau de 2 px).
    pub(crate) focus_clavier: bool,
    pub(crate) palette: Option<Palette>,
    /// Panneau Historique (lot 15B, Ctrl+Maj+H) : tiroir à droite ouvert ; il glisse (mouvement.rs) et ne prend pas le focus
    pub(crate) historique: bool,
    /// Défilement du tiroir (DIP depuis le haut de la liste) ; borné au dessin
    pub(crate) historique_defilement: f32,
    pub(crate) lot: Option<Lot>,
    pub(crate) galerie: Option<Galerie>,
    pub(crate) galerie_numero: u64,
    /// Coup d'œil : dernier point (DIP) du glissement qui déplace l'image zoomée ; `None` hors glissement
    pub(crate) galerie_glisse: Option<(f32, f32)>,
    /// La touche vient d'être prise par la carte des conflits : son caractère (WM_CHAR) ne va pas au filtre
    pub(crate) avaler_caractere: bool,
    /// Menu Nothing ouvert (barre d'actions, clic droit)
    pub(crate) menu: Option<menu::Menu>,
    /// Le bouton enfoncé a fermé un menu : son relâchement ne fait rien d'autre
    pub(crate) avaler_clic: bool,
    /// Applications du sous-menu OUVRIR AVEC du dernier clic droit
    pub(crate) applications: Vec<coquille::Application>,
    /// Dépôt avec le bouton droit en attente du menu : (sources, dossier)
    pub(crate) depot_droit: Option<(Vec<String>, String)>,
    /// Bouton droit enfoncé sur un élément choisi : point de départ d'un éventuel glisser (px)
    pub(crate) droit_depart: Option<POINT>,
    /// Fils de préparation des vignettes de la grille (créés au premier besoin)
    pub(crate) vignettes: Option<Vignettes>,
    /// Minuterie d'effacement de l'annonce déjà lancée : la relancer à chaque rendu la repousserait sans fin
    pub(crate) annonce_armee: bool,
    /// Emplacements des derniers onglets fermés (10 au plus), le plus récent en dernier : Ctrl+Maj+T les rouvre
    pub(crate) fermes: Vec<Emplacement>,
    /// Dernière signature des onglets vue au rendu (`vue_session`) : un écart arme la minuterie d'écriture de la session
    pub(crate) session_vue: Option<(Vec<Option<String>>, usize)>,
    /// Minuterie d'écriture de la session lancée : un changement de plus pendant l'attente ne la relance pas
    pub(crate) session_armee: bool,
    /// La fenêtre est active (WM_ACTIVATE) et n'est pas réduite (WM_SIZE) : conditions du clignement de Pixel
    pub(crate) active: bool,
    pub(crate) reduite: bool,
    /// Minuterie de clignement (9 s) armée ; elle lance un clignement de 630 ms (`Yeux::cligner`)
    pub(crate) clignement_arme: bool,
    /// Regard voulu des yeux de Pixel (DIP, quantifié), calculé depuis `pointeur` : un écart avec la valeur précédente est la seule
    /// raison de repeindre la bande sur un `WM_MOUSEMOVE`
    pub(crate) regard_voulu: (f32, f32),
    /// Heure (`mouvement::maintenant_ms`) de la dernière image peinte pendant que seuls les yeux de Pixel bougent
    pub(crate) image_yeux: u64,
    /// Pixel est dans la palette (et non dans la barre d'état), et depuis quand il a changé de place (glissement de 0,5 s)
    pub(crate) pixel_dans_palette: bool,
    pub(crate) relais_depuis: Option<std::time::Instant>,
    /// Une commande de la palette vient d'être lancée : Pixel se plisse (chevrons) un instant
    pub(crate) plisse_depuis: Option<std::time::Instant>,
    /// Pastille déployée par un clic (le survol déploie aussi, sans la fixer), et Échap l'a repliée pendant le survol
    pub(crate) pastille_fixee: Option<usize>,
    pub(crate) pastille_masquee: bool,
    /// Minuterie d'activité (100 ms) armée par `regler_activite`
    pub(crate) activite_armee: bool,
    /// Position du dernier `WM_MOUSEMOVE` (DIP, fenêtre) : centre du reflet. `None` : souris sortie, ou une touche a été pressée
    /// (un élément qui a le focus clavier n'est pas éclairé : la position du curseur ne dit rien)
    pub(crate) pointeur: Option<(f32, f32)>,
    /// Le bouton de la souris est enfoncé : le chrome survolé retombe à plat en 60 ms (`.chrome:active`). Le fondu et le ressort
    /// de survol sont suivis au dessin (`mouvement::Survols`).
    pub(crate) survol_appui: bool,
    /// La mesure de coût du reflet (`SPACEFILES_MESURE_REFLET`) est faite : elle ne tourne qu'une fois
    pub(crate) mesure_reflet_faite: bool,
    /// Dernière valeur de `reflet::actif_maintenant()` vue par `WM_APPARENCE` : un changement redessine toute la fenêtre
    pub(crate) reflet_actif: bool,
    /// Ouverture de la fenêtre (lot 12A) : demandée au lancement, l'horloge (`mouvement::maintenant_ms`) ne démarre qu'après le
    /// premier dessin (sinon la préparation de la cible Direct2D mangerait l'animation). `fermeture` : début de la sortie.
    pub(crate) ouverture_attend: bool,
    pub(crate) ouverture: Option<u64>,
    pub(crate) fermeture: Option<u64>,
    /// Mesure de l'ouverture (mode essai) : heure du premier dessin animé et images dessinées depuis
    pub(crate) mesure_ouverture: Option<(u64, u32)>,
    /// Mesure de la régularité des images (mode essai `SPACEFILES_MESURE_MOUVEMENT=1`)
    pub(crate) cadence: mouvement::Cadence,
    /// Fil de synchronisation sur l'écran (`DwmFlush`) tant qu'une animation tourne ; `true` dedans le fait sortir
    pub(crate) metronome: Option<std::sync::Arc<std::sync::atomic::AtomicBool>>,
}

/// Durée du glissement de Pixel entre la barre d'état et la palette, et du pli au lancement d'une commande
pub(crate) const RELAIS_MS: u128 = 500;

impl Fenetre {
    /// Convertit des pixels physiques en DIP
    pub(crate) fn dip(&self, px: i32) -> f32 {
        px as f32 * 96.0 / self.dpi
    }

    /// Un onglet de terminal de plus tient-il dans l'en-tête du panneau (place et nombre maximal) ?
    ///
    /// # Safety
    /// Fil d'interface ; `hwnd` est la fenêtre de cette `Fenetre`.
    pub(crate) unsafe fn terminal_peut_ajouter(&self, hwnd: HWND) -> bool {
        let (lp, hp) = taille_client(hwnd);
        let (lt, h) = (self.dip(lp as i32), self.dip(hp as i32));
        crate::rendu::terminal_peut_ajouter(largeur_liste(lt, self.apercu_visible), h, self.terminaux.len())
    }

    /// Colonnes × lignes que la grille du panneau terminal peut afficher dans la fenêtre actuelle
    pub(crate) unsafe fn dimensions_terminal(&self, hwnd: HWND) -> (usize, usize) {
        let (lp, hp) = taille_client(hwnd);
        let (lt, h) = (self.dip(lp as i32), self.dip(hp as i32));
        let ll = largeur_liste(lt, self.apercu_visible);
        let z = zone_terminal_grille(ll, h);
        let (cw, ch) = self.rendu.cellule_terminal();
        crate::terminal::panneau::dimensions_grille(z.right - z.left, z.bottom - z.top, cw, ch)
    }

    /// Case (rangée visible, colonne) de la grille du terminal sous le point (x, y) en DIP. Hors de la grille : `None`, ou,
    /// avec `borner`, la case la plus proche (un glissement qui sort de la grille continue de sélectionner jusqu'au bord).
    pub(crate) unsafe fn case_terminal_sous(&self, hwnd: HWND, x: f32, y: f32, borner: bool) -> Option<(usize, usize)> {
        // Le journal masque la grille : aucune case, donc ni sélection ni souris vers le programme
        if self.terminal_journal {
            return None;
        }
        let p = self.terminal_ref()?;
        let (lp, hp) = taille_client(hwnd);
        let (lt, h) = (self.dip(lp as i32), self.dip(hp as i32));
        let z = zone_terminal_grille(largeur_liste(lt, self.apercu_visible), h);
        if !borner && !(x >= z.left && x < z.right && y >= z.top && y < z.bottom) {
            return None;
        }
        let (cw, ch) = self.rendu.cellule_terminal();
        let (colonnes, lignes) = (p.terminal().colonnes().max(1), p.terminal().lignes().max(1));
        let col = (((x - z.left) / cw).floor().max(0.0) as usize).min(colonnes - 1);
        let rangee = (((y - z.top) / ch).floor().max(0.0) as usize).min(lignes - 1);
        Some((rangee, col))
    }

    /// T3 : appui du bouton gauche sur la grille quand le programme a demandé la souris (Maj garde la sélection de texte).
    /// Lui envoie l'appui, prend la capture et rend `true` ; `x` et `y` en DIP. Sans cela rien n'est fait.
    pub(crate) unsafe fn appui_vers_programme(&mut self, hwnd: HWND, x: f32, y: f32, maj: bool, ctrl: bool, alt: bool) -> bool {
        if maj || !self.terminal_ref().is_some_and(|p| p.souris_capturee()) {
            return false;
        }
        let Some((rangee, col)) = self.case_terminal_sous(hwnd, x, y, false) else { return false };
        if let Some(p) = self.terminal_mut() {
            // Une sélection de texte restée affichée n'a plus de sens une fois le clic parti vers le programme
            p.effacer_selection();
            p.oublier_case_souris();
            p.envoyer_souris(crate::terminal::souris::Evenement::Appui(crate::terminal::souris::Bouton::Gauche), col, rangee, ctrl, alt);
        }
        self.terminal_glisse_appli = true;
        windows::Win32::UI::Input::KeyboardAndMouse::SetCapture(hwnd);
        true
    }

    /// T3 : fin d'un glissement transmis au programme : il reçoit le relâchement (case bornée à la grille, même si le
    /// pointeur en est sorti). `liberer` rend la capture ; à `false` quand c'est la perte de la capture qui l'a causée.
    /// Sans glissement en cours, ne fait rien.
    pub(crate) unsafe fn finir_glisse_appli(&mut self, hwnd: HWND, x: f32, y: f32, ctrl: bool, alt: bool, liberer: bool) {
        // Drapeau baissé avant `ReleaseCapture`, qui envoie `WM_CAPTURECHANGED` tout de suite : le gestionnaire ne refait rien
        if !std::mem::take(&mut self.terminal_glisse_appli) {
            return;
        }
        if let Some((rangee, col)) = self.case_terminal_sous(hwnd, x, y, true) {
            if let Some(p) = self.terminal_mut() {
                p.envoyer_souris(crate::terminal::souris::Evenement::Relache(crate::terminal::souris::Bouton::Gauche), col, rangee, ctrl, alt);
            }
        }
        if let Some(p) = self.terminal_mut() {
            p.oublier_case_souris();
        }
        if liberer {
            let _ = windows::Win32::UI::Input::KeyboardAndMouse::ReleaseCapture();
        }
    }

    /// Le panneau terminal est dessiné et prend la souris et le clavier (jamais sur une page : Réglages, ESPACE)
    pub(crate) fn terminal_affiche(&self) -> bool {
        !self.terminaux.is_empty() && !self.app.emplacement.est_page()
    }

    /// Panneau de l'onglet affiché
    pub(crate) fn terminal_ref(&self) -> Option<&crate::terminal::panneau::Panneau> {
        self.terminaux.get(self.terminal_actif).map(|b| &**b)
    }

    /// Panneau de l'onglet affiché, modifiable
    pub(crate) fn terminal_mut(&mut self) -> Option<&mut crate::terminal::panneau::Panneau> {
        self.terminaux.get_mut(self.terminal_actif).map(|b| &mut **b)
    }

    /// T4b : garde la liste et le terminal sur le même dossier, dans les deux sens, en ne réagissant qu'aux changements.
    /// Shell → liste : un `cd` tapé dans l'onglet affiché ouvre ce dossier dans la liste (les autres onglets notent leur
    /// dossier sans toucher à la liste : changer d'onglet ne déplace rien). Liste → shell : un nouveau dossier de la liste
    /// est tapé au shell (`cd`) dès qu'il est à une invite vide ; sinon on réessaie à la prochaine invite.
    pub(crate) fn synchroniser_terminal(&mut self, hwnd: HWND) {
        if self.terminaux.is_empty() {
            return;
        }
        let actif = self.terminal_actif;
        let mut du_shell = None;
        for (i, p) in self.terminaux.iter_mut().enumerate() {
            let d = p.dossier_change_par_le_shell();
            if i == actif {
                du_shell = d;
            }
        }
        if let Some(d) = du_shell {
            let deja = self.app.dossier().is_some_and(|c| crate::terminal::panneau::meme_dossier(c, &d));
            if self.terminal_affiche() && !deja && std::path::Path::new(&d).is_dir() {
                self.terminal_dossier_liste = Some(d.clone());
                self.app.naviguer(hwnd, crate::dossier::Emplacement::Dossier(d));
            }
        }
        let Some(d) = self.app.dossier().map(str::to_owned) else { return };
        if self.terminal_dossier_liste.as_deref() == Some(d.as_str()) {
            return;
        }
        let Some(p) = self.terminal_mut() else { return };
        if crate::terminal::panneau::meme_dossier(&d, p.dossier_connu()) || p.aller_dans(&d) {
            self.terminal_dossier_liste = Some(d);
        }
    }

    /// Ajoute un onglet (déjà ouvert) et l'affiche ; le clavier lui va. Rend `false` sans rien faire s'il y en a trop
    pub(crate) fn ajouter_onglet_terminal(&mut self, p: crate::terminal::panneau::Panneau) -> bool {
        if self.terminaux.len() >= crate::terminal::onglets::ONGLETS_MAX {
            return false;
        }
        self.abandonner_glisse_terminal();
        self.terminaux.push(Box::new(p));
        self.terminal_actif = self.terminaux.len() - 1;
        self.terminal_journal = false;
        self.terminal_focus = true;
        self.terminal_molette_reste = 0.0;
        true
    }

    /// Un glissement commencé dans l'onglet affiché (sélection de texte ou souris transmise au programme) ne peut pas
    /// continuer quand l'onglet change ou se ferme : le programme reçoit son relâchement (sinon il croirait le bouton tenu
    /// jusqu'au prochain clic), la capture est rendue, et si le bouton est encore enfoncé son relâchement ne sera pas pris
    /// pour un clic sur ce qui se trouve dessous. À appeler AVANT de changer `terminal_actif` ou de retirer l'onglet.
    /// Sans glissement en cours, ne fait rien.
    fn abandonner_glisse_terminal(&mut self) {
        if !(self.terminal_glisse || self.terminal_glisse_appli) {
            return;
        }
        use windows::Win32::Foundation::POINT;
        use windows::Win32::Graphics::Gdi::ScreenToClient;
        use windows::Win32::UI::Input::KeyboardAndMouse::{GetCapture, GetKeyState, ReleaseCapture, VK_LBUTTON};
        use windows::Win32::UI::WindowsAndMessaging::GetCursorPos;
        // SAFETY : fil d'interface ; `GetCapture` rend la fenêtre qui détient la capture posée au début du glissement.
        // `ReleaseCapture` envoie `WM_CAPTURECHANGED` tout de suite et son gestionnaire reprend un `&mut Fenetre` : les deux
        // drapeaux sont donc baissés avant, pour qu'il ne refasse rien (même motif que `finir_glisse_appli`).
        unsafe {
            let hwnd = GetCapture();
            if self.terminal_glisse_appli && !hwnd.is_invalid() {
                let mut p = POINT::default();
                let (x, y) = if GetCursorPos(&mut p).is_ok() && ScreenToClient(hwnd, &mut p).as_bool() {
                    (self.dip(p.x), self.dip(p.y))
                } else {
                    (0.0, 0.0)
                };
                self.finir_glisse_appli(hwnd, x, y, false, false, false);
            }
            self.terminal_glisse = false;
            self.terminal_glisse_appli = false;
            if GetKeyState(VK_LBUTTON.0 as i32) < 0 {
                self.avaler_clic = true;
            }
            let _ = ReleaseCapture();
        }
    }

    /// Affiche l'onglet de rang `i` (sans effet s'il n'existe pas) ; un glissement en cours est abandonné proprement
    pub(crate) fn choisir_onglet_terminal(&mut self, i: usize) {
        if i >= self.terminaux.len() {
            return;
        }
        // Le journal affiché : un clic sur un onglet shell, même l'actif, rend la place au shell
        let etait_journal = std::mem::take(&mut self.terminal_journal);
        if i == self.terminal_actif {
            self.terminal_focus = self.terminal_focus || etait_journal;
            return;
        }
        self.abandonner_glisse_terminal();
        self.terminal_actif = i;
        self.terminal_focus = true;
        // États de souris propres à l'onglet quitté : la fraction de cran de molette et la dernière case envoyée
        self.terminal_molette_reste = 0.0;
        if let Some(p) = self.terminal_mut() {
            p.oublier_case_souris();
        }
    }

    /// T4c : affiche l'onglet JOURNAL (un glissement de sélection en cours est abandonné ; le clavier revient à l'explorateur,
    /// le journal n'a rien à saisir) ; un second clic sur l'onglet le quitte
    pub(crate) fn basculer_journal_terminal(&mut self) {
        if self.terminal_journal {
            self.terminal_journal = false;
            self.terminal_focus = true;
            return;
        }
        self.abandonner_glisse_terminal();
        self.terminal_journal = true;
        self.terminal_focus = false;
        self.terminal_operations_version = usize::MAX;
    }

    /// Relit `operations.csv` quand le journal est affiché et qu'il a changé depuis la dernière lecture ; rend `true` si la liste a
    /// été remplacée
    pub(crate) fn actualiser_operations_terminal(&mut self) -> bool {
        if !self.terminal_journal || !self.terminal_affiche() {
            return false;
        }
        let v = crate::journal::ecritures();
        if self.terminal_operations_version == v {
            return false;
        }
        self.terminal_operations_version = v;
        self.terminal_operations = crate::journal::dernieres_operations(crate::journal::OPERATIONS_AFFICHEES);
        true
    }

    /// Ferme l'onglet de rang `i` (son shell est tué par le `Drop`) ; le dernier fermé ferme tout le panneau
    pub(crate) fn fermer_onglet_terminal(&mut self, i: usize) {
        if i >= self.terminaux.len() {
            return;
        }
        if self.terminaux.len() == 1 {
            self.fermer_terminal();
            return;
        }
        let etait_actif = i == self.terminal_actif;
        if etait_actif {
            // Avant le retrait : le relâchement doit encore atteindre le programme de cet onglet
            self.abandonner_glisse_terminal();
        }
        self.terminaux.remove(i);
        self.terminal_actif = crate::terminal::onglets::apres_fermeture(self.terminal_actif, i, self.terminaux.len());
        if etait_actif {
            self.terminal_molette_reste = 0.0;
            if let Some(p) = self.terminal_mut() {
                p.oublier_case_souris();
            }
        }
    }

    /// Ferme le panneau terminal entier : toutes les sessions (et leurs descendants) sont tuées par le `Drop` des panneaux
    pub(crate) fn fermer_terminal(&mut self) {
        self.terminaux.clear();
        self.terminal_actif = 0;
        self.terminal_journal = false;
        self.terminal_focus = false;
        // Un glissement de sélection en cours n'a plus de cible : sans cela la capture resterait prise et le survol figé
        // `|` et non `||` : les deux drapeaux doivent être remis à zéro
        if std::mem::take(&mut self.terminal_glisse) | std::mem::take(&mut self.terminal_glisse_appli) {
            // SAFETY : appelé sur le fil d'interface, qui détient la capture de la souris posée au début du glissement
            let _ = unsafe { windows::Win32::UI::Input::KeyboardAndMouse::ReleaseCapture() };
        }
    }

    /// Un mouvement du lot 12A n'a pas fini : ouverture, fermeture ou glissement (pastille, vue, onglet). La minuterie
    /// d'animation reste armée pendant ce temps et s'arrête d'elle-même ensuite.
    pub(crate) fn mouvement_en_cours(&self, maintenant: u64) -> bool {
        self.mouvement_hors_yeux(maintenant) || self.rendu.mouvements.borrow().yeux.en_cours(maintenant)
    }

    /// Un mouvement autre que les yeux de Pixel n'a pas fini (ouverture, fermeture, glissements, fondus)
    pub(crate) fn mouvement_hors_yeux(&self, maintenant: u64) -> bool {
        self.ouverture_attend
            || self.fermeture.is_some()
            || self.ouverture.is_some_and(|d| maintenant < d + mouvement::OUVERTURE_MS.max(mouvement::CASCADE_TOTAL_MS))
            || self.rendu.mouvements.borrow().en_cours_hors_yeux(maintenant)
    }

    pub(crate) fn nb_onglets(&self) -> usize {
        self.fond.len() + 1
    }

    pub(crate) fn titres(&self) -> Vec<String> {
        let mut v: Vec<String> = self.fond.iter().map(App::titre).collect();
        v.insert(self.rang, self.app.titre());
        v
    }

    /// Tous les onglets, affiché compris
    pub(crate) fn tous(&mut self) -> impl Iterator<Item = &mut App> {
        std::iter::once(&mut self.app).chain(self.fond.iter_mut()).chain(self.second.iter_mut())
    }

    /// Pastilles de tous les onglets, dans l'ordre d'affichage, avec l'onglet de chacune (rang dans `tous()` ; `usize::MAX` pour
    /// l'annulation, commune à tous les onglets comme l'historique)
    pub(crate) fn pastilles(&self) -> Vec<(pastilles::Pastille, usize)> {
        let mut v: Vec<(pastilles::Pastille, usize)> = std::iter::once(&self.app)
            .chain(self.fond.iter())
            .chain(self.second.iter())
            .enumerate()
            .flat_map(|(k, a)| a.pastilles_propres().into_iter().map(move |p| (p, k)))
            .collect();
        // Téléchargements du navigateur : communs à tous les onglets (un seul abonnement pour le processus), comme l'annulation
        v.extend(navigateur::pastilles().into_iter().map(|p| (p, usize::MAX)));
        if let Some(l) = app::annulation_proposee() {
            v.push((pastilles::Pastille::annulation(l), usize::MAX));
        }
        // Cartes d'information (toasts) : communes à tous les onglets
        v.extend(app::toasts_vivants().into_iter().map(|p| (p, usize::MAX)));
        v.sort_by_key(|(p, _)| pastilles::rang_ordre(p));
        v
    }

    /// Centre d'opérations (N-01) : ce qui tourne dans chaque onglet, ce qui attend, puis les dernières opérations finies avec
    /// la cause de leur échec. Le rang d'une opération est celui de `tous()` : c'est lui que pause et arrêt visent.
    pub(crate) fn lignes_operations(&self) -> Vec<palette::LigneOp> {
        use palette::{GenreLigneOp, LigneOp};
        let plusieurs = self.nb_onglets() > 1;
        let mut v = Vec::new();
        for (k, a) in self.apps().enumerate() {
            let onglet = if plusieurs { format!(" · ONGLET {}", k + 1) } else { String::new() };
            if let Some(op) = &a.operation {
                let phase = a.phase_operation().and_then(|p| p.libelle()).map(|m| format!("{m} · ")).unwrap_or_default();
                let detail = format!("{phase}{} %{onglet}", (op.avancement * 100.0).round() as i32);
                v.push(LigneOp { genre: GenreLigneOp::EnCours(k), titre: op.nom_court.clone(), detail });
            }
            for (rang, nom) in a.noms_en_file().into_iter().enumerate() {
                v.push(LigneOp { genre: GenreLigneOp::EnFile, titre: nom, detail: format!("EN FILE · N°{}{onglet}", rang + 1) });
            }
        }
        for (nom, issue) in app::operations_recentes() {
            let echec = !(issue.starts_with("TERMINÉE") || issue.starts_with("ANNULÉE"));
            v.push(LigneOp { genre: GenreLigneOp::Finie { echec }, titre: nom, detail: issue });
        }
        v
    }

    /// Tous les onglets en lecture seule (même ordre que `tous`)
    pub(crate) fn apps(&self) -> impl Iterator<Item = &App> {
        std::iter::once(&self.app).chain(self.fond.iter()).chain(self.second.iter())
    }

    /// Une fenêtre modale cache les pastilles : palette, renommage par lot, coup d'œil, menu, carte des conflits
    pub(crate) fn modale(&self) -> bool {
        self.palette.is_some() || self.lot.is_some() || self.galerie.is_some() || self.menu.is_some() || self.apps().any(|a| a.conflits.is_some())
    }

    /// Une pastille de fin, d'erreur ou d'annulation attend d'être effacée (tous les onglets, pas seulement l'affiché)
    pub(crate) fn pastille_temporaire_presente(&self) -> bool {
        self.apps().any(|a| a.fin_op.is_some()) || app::annulation_proposee().is_some() || navigateur::temporaires() || !app::toasts_vivants().is_empty()
    }

    /// Pastille déployée : celle qui est survolée (sauf si Échap l'a repliée), sinon celle qu'un clic a fixée
    pub(crate) fn pastille_ouverte(&self, nombre: usize) -> Option<usize> {
        let survolee = match self.app.survol {
            Cible::Pastille(i) | Cible::BoutonPastille(i, _) if !self.pastille_masquee => Some(i),
            _ => None,
        };
        survolee.or(self.pastille_fixee).filter(|&i| i < nombre)
    }

    /// Ce que Pixel doit dire maintenant (état, phase du mouvement, heure, relais vers la palette). Déclenche le glissement
    /// quand la palette s'ouvre ou se ferme. Mouvement réduit : pose fixe, bascule immédiate.
    /// Invariant (unsafe) : fil d'interface, `hwnd` est la fenêtre principale vivante ; aucune référence n'est gardée après l'appel.
    pub(crate) unsafe fn vue_pixel(&mut self, hwnd: HWND) -> rendu::VuePixel {
        // Pause, reprise, fin d'opération : la minuterie d'activité suit (armée seulement si quelque chose bouge)
        regler_activite(hwnd, self);
        let anime = reglages::animations_actives();
        let dans = self.palette.is_some();
        if dans != self.pixel_dans_palette {
            self.pixel_dans_palette = dans;
            self.relais_depuis = Some(std::time::Instant::now());
            // La minuterie d'animation existante porte le glissement (elle s'arrête d'elle-même quand il est fini)
            animer(hwnd, self);
        }
        let cible = if dans { 1.0 } else { 0.0 };
        let place = match self.relais_depuis {
            Some(t) if anime && t.elapsed().as_millis() < RELAIS_MS => {
                let x = t.elapsed().as_millis() as f32 / RELAIS_MS as f32;
                // Ressort : arrivée rapide qui ralentit, comme le défilement
                let e = 1.0 - (1.0 - x).powi(3);
                if dans { e } else { 1.0 - e }
            }
            _ => {
                self.relais_depuis = None;
                cible
            }
        };
        if self.plisse_depuis.is_some_and(|t| t.elapsed().as_millis() >= RELAIS_MS) || !anime && self.plisse_depuis.is_some() {
            self.plisse_depuis = None;
        }
        let plisse = self.plisse_depuis.is_some();
        // En pause rien ne bouge : Pixel ne glisse plus
        // Un téléchargement du navigateur qui avance compte aussi (seuls ceux dont la taille bouge ; pas ceux en pause)
        let activite = self.apps().any(|a| a.en_activite() && !a.en_pause()) || navigateur::avance();
        let conflit = self.apps().any(|a| a.conflits.is_some());
        let erreur = self.apps().any(|a| a.en_erreur());
        let reussite = plisse || self.apps().any(|a| a.reussite_recente()) || navigateur::reussite_recente(pixel::REUSSITE_MS);
        // SAFETY: simple lecture de l'heure locale
        let heure = windows::Win32::System::SystemInformation::GetLocalTime().wHour as u32;
        let periode = pixel::periode_effective(reglages::choix_pixel(), heure);
        let etat = pixel::etat_pixel(activite, conflit, erreur, reussite, periode == pixel::Periode::Nuit);
        let debut = self.apps().filter_map(|a| a.operation.as_ref().map(|o| o.debut)).chain(navigateur::debut()).min();
        let ecoule = debut.map_or(0, |d| d.elapsed().as_millis());
        // Lot 13 : regard vers le pointeur, nuit, fondu de couleur et clignement viennent de `Yeux` (lu à chaque image, aucune
        // minuterie propre : le regard est repeint par le `WM_MOUSEMOVE`, le clignement par la minuterie de 9 s)
        let regard_voulu = self.calculer_regard_voulu(hwnd);
        self.regard_voulu = regard_voulu;
        let t = mouvement::maintenant_ms();
        // Lot 15A : une commande vient d'être lancée depuis la palette (`.pe.hp`) : barres de 5 DIP, couleur d'accent
        let lance = self.palette.as_ref().is_some_and(|p| p.lance);
        let teinte = if lance { pixel::Teinte::Ambre } else { pixel::teinte(etat, periode) };
        let yeux = {
            let mut m = self.rendu.mouvements.borrow_mut();
            m.yeux.contente(lance, t, anime);
            m.yeux.maj(regard_voulu, etat == pixel::EtatPixel::Nuit, teinte, t, anime)
        };
        let phase = match etat {
            // Mouvement réduit : pose fixe, décalée d'un cran vers la droite pour rester distincte du repos
            pixel::EtatPixel::Activite if anime => pixel::phase_glissement(ecoule),
            pixel::EtatPixel::Activite => 0.25,
            // Le clignement ne joue pas quand la nuit ferme déjà les yeux (`animation:none` de la maquette)
            pixel::EtatPixel::Repos if anime => yeux.clignement * (1.0 - yeux.aplatie),
            _ => 0.0,
        };
        rendu::VuePixel { etat, periode, phase, place, grille: anime.then(|| (ecoule % 1200) as f32 / 1200.0), yeux }
    }

    /// Regard que les yeux de Pixel doivent viser : vers le pointeur quand il est dans la fenêtre, la palette fermée, les animations
    /// et les yeux autorisés ; sinon droit devant. Arrondi au demi-DIP (`pixel::regard_pointeur`).
    /// Invariant (unsafe) : fil d'interface, `hwnd` est la fenêtre principale vivante (lecture de sa taille).
    pub(crate) unsafe fn calculer_regard_voulu(&self, hwnd: HWND) -> (f32, f32) {
        // Lot 15A : dans la palette, Pixel regarde vers la ligne choisie (`palEyes`, de -4 à +4 DIP vers le bas selon son rang), pas vers
        // le pointeur ; le glissement de 350 ms est celui du regard. Mouvement réduit : posé tout de suite (le regard reste).
        if let Some(p) = &self.palette {
            // Pendant le lancement les yeux se centrent (`palRun` : `palEyes(0,'hp')`)
            return if reglages::yeux_pixel_actifs() && !p.lance { (0.0, pixel::regard(p.choix, p.items.len())) } else { (0.0, 0.0) };
        }
        let actif = reglages::animations_actives() && reglages::yeux_pixel_actifs();
        if !actif {
            return (0.0, 0.0);
        }
        let (_, h) = taille_client(hwnd);
        pixel::regard_pointeur(self.pointeur, rendu::pixel_dans_statut(self.dip(h as i32)), true)
    }

    /// Le pointeur a bougé, est sorti ou a été retiré par une touche : si le regard voulu change, la bande de la barre d'état est
    /// repeinte (le dessin lance le glissement de 350 ms). Un déplacement qui ne change pas le regard arrondi ne coûte rien.
    /// Invariant (unsafe) : fil d'interface, `hwnd` est la fenêtre principale vivante ; aucune référence n'est gardée après l'appel.
    pub(crate) unsafe fn suivre_regard(&mut self, hwnd: HWND) {
        let voulu = self.calculer_regard_voulu(hwnd);
        if voulu != self.regard_voulu {
            self.regard_voulu = voulu;
            invalider_bande(hwnd, self);
        }
    }

    /// Un clic sur un bouton d'une pastille : `rang` est le rang dans `pastilles()`
    /// Invariant (unsafe) : fil d'interface, `hwnd` est la fenêtre principale vivante ; aucune référence n'est gardée après l'appel.
    pub(crate) unsafe fn clic_bouton_pastille(&mut self, hwnd: HWND, rang: usize, bouton: pastilles::Bouton) {
        // Une modale cache les pastilles : aucun clic ne les traverse (le dessin précédent a pu les montrer avant son ouverture)
        if self.modale() {
            return;
        }
        let liste = self.pastilles();
        let Some((p, onglet)) = liste.get(rang) else { return };
        // Le rang vient du dessin précédent : la pastille trouvée doit être celle qui a été dessinée (même clé) et porter ce bouton
        let dessinee = self.rendu.zones_pastilles.borrow().places.iter().find(|pl| pl.index == rang).map(|pl| pl.cle);
        if dessinee != Some(p.cle) || !pastilles::bouton_autorise(p, bouton) {
            return;
        }
        let (p, onglet) = (p.clone(), *onglet);
        self.agir_bouton_pastille(hwnd, &p, onglet, bouton);
    }

    /// Un bouton de pastille activé par l'accessibilité (motif Invoke) : la pastille est retrouvée par sa clé, son libellé et son rang d'occurrence
    /// (les fins et erreurs ont toutes la clé 0), pas par un rang, car la liste a pu changer entre la lecture du lecteur d'écran et ce message. Contrairement au clic, elle n'a pas besoin d'avoir
    /// été dessinée (un lecteur d'écran agit sans souris et la fenêtre peut être masquée). Faux si elle n'existe plus, si une modale est
    /// ouverte ou si le bouton n'est pas permis.
    /// Invariant (unsafe) : fil d'interface, `hwnd` est la fenêtre principale vivante ; aucune référence n'est gardée après l'appel.
    pub(crate) unsafe fn bouton_pastille_uia(&mut self, hwnd: HWND, cle: u64, libelle: &str, occurrence: usize, bouton: pastilles::Bouton) -> bool {
        if self.modale() {
            return false;
        }
        let Some((p, onglet)) = self.pastilles().into_iter().filter(|(p, _)| p.cle == cle && p.libelle == libelle).nth(occurrence) else { return false };
        if !pastilles::bouton_autorise(&p, bouton) {
            return false;
        }
        self.agir_bouton_pastille(hwnd, &p, onglet, bouton);
        true
    }

    /// Ouvre le dossier de destination de la première pastille qui en a un (raccourci Ctrl+Maj+O) ; faux s'il n'y en a pas
    /// Invariant (unsafe) : fil d'interface, `hwnd` est la fenêtre principale vivante ; aucune référence n'est gardée après l'appel.
    pub(crate) unsafe fn ouvrir_destination(&mut self, hwnd: HWND) -> bool {
        if self.modale() {
            return false;
        }
        let Some((p, onglet)) =
            self.pastilles().into_iter().find(|(p, _)| p.destination.is_some() && pastilles::bouton_autorise(p, pastilles::Bouton::Destination))
        else {
            return false;
        };
        self.agir_bouton_pastille(hwnd, &p, onglet, pastilles::Bouton::Destination);
        true
    }

    /// Le corps commun d'un bouton de pastille, une fois la pastille et le bouton validés par l'appelant
    /// Invariant (unsafe) : fil d'interface, `hwnd` est la fenêtre principale vivante ; aucune référence n'est gardée après l'appel.
    unsafe fn agir_bouton_pastille(&mut self, hwnd: HWND, p: &pastilles::Pastille, onglet: usize, bouton: pastilles::Bouton) {
        let destination = p.destination.clone();
        // Pastille d'un téléchargement du navigateur : ni onglet ni opération, la clé dit laquelle
        if navigateur::est_cle(p.cle) {
            match bouton {
                pastilles::Bouton::Afficher => {
                    // Le dossier s'ouvre sur le fichier choisi (même chemin que « montrer » de la palette) ; la pastille n'est
                    // fermée que si le fichier a vraiment été montré, sinon elle reste (le fichier a pu disparaître)
                    if let Some(chemin) = navigateur::chemin_fini(p.cle) {
                        if self.app.montrer(hwnd, &chemin) {
                            navigateur::fermer(p.cle);
                        }
                    }
                }
                pastilles::Bouton::Fermer => navigateur::fermer(p.cle),
                // Pas d'autre bouton sur ces pastilles (`bouton_autorise` l'a déjà refusé)
                _ => {}
            }
            self.pastille_fixee = None;
            return;
        }
        // Carte d'information : ni onglet ni opération, la clé dit laquelle (le ✕ est son seul bouton)
        if p.genre == pastilles::Genre::Info {
            if bouton == pastilles::Bouton::Fermer {
                app::fermer_toast(p.cle);
            }
            self.pastille_fixee = None;
            return;
        }
        match bouton {
            pastilles::Bouton::Pause => {
                if let Some(a) = self.tous().nth(onglet) {
                    a.basculer_pause();
                }
            }
            pastilles::Bouton::Annuler => {
                if let Some(a) = self.tous().nth(onglet) {
                    a.annuler_operation();
                }
            }
            pastilles::Bouton::Destination => {
                if let Some(d) = destination {
                    self.app.naviguer(hwnd, Emplacement::dossier(&d));
                }
            }
            pastilles::Bouton::Fermer => {
                if let Some(a) = self.tous().nth(onglet) {
                    a.fin_op = None;
                }
            }
            pastilles::Bouton::AnnulerAction => {
                self.app.annuler_derniere(hwnd);
                app::oublier_annulation();
            }
            // Réservé aux téléchargements du navigateur, traités plus haut
            pastilles::Bouton::Afficher => {}
        }
        self.pastille_fixee = None;
    }

    /// Retire les pastilles périmées ; vrai s'il en reste une qui doit disparaître seule (la minuterie d'annonce continue)
    pub(crate) fn pastilles_temporaires(&mut self) -> bool {
        let mut reste = false;
        for a in self.tous() {
            reste |= a.expirer_pastilles();
        }
        reste |= navigateur::expirer();
        reste |= app::purger_toasts();
        reste || app::annulation_proposee().is_some()
    }

    /// Ouvre un onglet juste après l'onglet affiché
    pub(crate) unsafe fn nouvel_onglet(&mut self, hwnd: HWND, emp: Emplacement) {
        let mut neuf = App::nouveau(emp);
        neuf.disques = self.app.disques.clone();
        neuf.nuages = self.app.nuages.clone();
        neuf.serveurs = self.app.serveurs.clone();
        neuf.etats = self.etats.clone();
        neuf.epingles = epingles_volet();
        let ancien = std::mem::replace(&mut self.app, neuf);
        self.fond.insert(self.rang, ancien);
        self.rang += 1;
        self.app.charger(hwnd);
    }

    /// Affiche l'onglet `k` (position dans la barre)
    pub(crate) fn aller_onglet(&mut self, k: usize) {
        if k >= self.nb_onglets() || k == self.rang {
            return;
        }
        let j = if k < self.rang { k } else { k - 1 };
        std::mem::swap(&mut self.app, &mut self.fond[j]);
        // L'ancien onglet affiché reprend sa place dans la barre
        let ancien = self.fond.remove(j);
        let place = if k < self.rang { self.rang - 1 } else { self.rang };
        self.fond.insert(place, ancien);
        self.rang = k;
        self.apercu_demande = None;
        // Colonnes et lignes sont propres au dossier affiché : celles du nouvel onglet
        reglages::afficher_dossier(self.app.dossier());
    }

    /// Stockages ajoutés (dans l'ordre des réglages)
    pub(crate) fn ajoutes(&self) -> Vec<Lieu> {
        let r = reglages::actuels();
        let (cloud, serveurs) = stockages::lieux_ajoutes(&r.stockages, r.experimental);
        let mut v: Vec<Lieu> = cloud.into_iter().chain(serveurs).collect();
        v.sort_by_key(|l| l.ajoute);
        v
    }

    /// Pour les rectangles de lignes de UI Automation : DPI, largeur du panneau qui a le focus et son décalage en x (le
    /// panneau de droite du double panneau commence après le volet, comme le test de clic `point_panneau`).
    pub(crate) unsafe fn geo_accessible(&self, hwnd: HWND) -> Option<accessibilite::Geo> {
        let (w, _) = taille_client(hwnd);
        let lt = w as f32 * 96.0 / self.dpi;
        let double = self.second.is_some();
        let largeur = theme::largeur_panneau(theme::largeur_liste(lt, self.apercu_visible), double);
        let decalage = if double && self.focus_droite { largeur - theme::largeur_volet() } else { 0.0 };
        Some(accessibilite::Geo { dpi: self.dpi, largeur, decalage })
    }

    /// La page des réglages telle que la lit un lecteur d'écran (None hors des réglages)
    pub(crate) fn page_lue(&self) -> Option<accessibilite::PageLue> {
        if self.app.emplacement != Emplacement::Reglages {
            return None;
        }
        let lignes = self.lignes();
        let (cartes, focus) = page::noms_accessibles(&lignes, self.page.categorie, self.page.focus);
        let pos = page::positions(&lignes, self.page.categorie);
        let places = page::plage(&lignes, self.page.categorie).map(|k| (page::PAGE_Y + pos[k].0 - self.page.defilement, pos[k].1, lignes[k].compacte())).collect();
        Some(accessibilite::PageLue { places, categorie: page::CATEGORIES[self.page.categorie].to_string(), cartes, focus })
    }

    /// Lignes de la page des réglages
    pub(crate) fn lignes(&self) -> Vec<page::Ligne> {
        page::lignes(&reglages::actuels(), &self.detectes, &self.ajoutes(), self.par_defaut, self.win_e)
    }

    /// Volet de tous les onglets : cloud détecté + stockages ajoutés
    pub(crate) fn rafraichir_lieux(&mut self) {
        let r = reglages::actuels();
        let (cloud, mut serveurs) = stockages::lieux_ajoutes(&r.stockages, r.experimental);
        // NAS connu du client Zima, s'il n'a pas déjà été ajouté à la main
        if let Some(z) = stockages::zima_client() {
            if !serveurs.iter().any(|s| s.detail.eq_ignore_ascii_case(z.detail.rsplit(' ').next().unwrap_or(""))) {
                serveurs.insert(0, z);
            }
        }
        let nuages: Vec<Lieu> = self.detectes.iter().cloned().chain(cloud).collect();
        let epingles = epingles_volet();
        let etats = self.etats.clone();
        for a in self.tous() {
            a.nuages = nuages.clone();
            a.serveurs = serveurs.clone();
            a.epingles = epingles.clone();
            a.etats = etats.clone();
        }
    }

    /// Lot 14b : mesure l'état des lignes CLOUD et SERVEURS dans un fil (jamais au dessin). Fenêtre cachée ou réduite : rien.
    /// Une mesure à la fois, au plus toutes les 30 s, sauf s'il y a un lieu jamais mesuré.
    pub(crate) unsafe fn mesurer_etats(&mut self, hwnd: HWND) {
        if self.etats_en_cours || self.reduite || !IsWindowVisible(hwnd).as_bool() {
            return;
        }
        let d = etats::demandes(&self.app.nuages, &self.app.serveurs);
        if d.is_empty() {
            return;
        }
        let nouveau = d.iter().any(|x| self.etats.lire(&x.cle).is_none());
        if !nouveau && self.etats_mesure.is_some_and(|t| t.elapsed() < etats::ECART) {
            return;
        }
        self.etats_en_cours = true;
        self.etats_mesure = Some(std::time::Instant::now());
        etats::lancer(hwnd, d);
    }

    /// Résultat d'une mesure : le cache est remplacé et recopié dans tous les onglets
    pub(crate) fn recevoir_etats(&mut self, mesures: Vec<(String, etats::Etat)>) {
        self.etats_en_cours = false;
        self.etats.remplacer(mesures);
        let etats = self.etats.clone();
        for a in self.tous() {
            a.etats = etats.clone();
        }
    }

    /// Double panneau : le focus passe à l'autre panneau (Tab, clic)
    pub(crate) fn basculer_panneau(&mut self) {
        if let Some(o) = &mut self.second {
            std::mem::swap(&mut self.app, o);
            self.focus_droite = !self.focus_droite;
            self.apercu_demande = None;
        }
    }

    /// F6 : ouvre ou ferme le double panneau
    pub(crate) unsafe fn double_panneau(&mut self, hwnd: HWND) {
        if self.second.take().is_some() {
            self.focus_droite = false;
            self.app.annoncer("PANNEAU UNIQUE", false);
            return;
        }
        self.double_panneau_sur(hwnd, self.app.emplacement.clone());
        self.app.annoncer("DOUBLE PANNEAU · TAB : CHANGER DE PANNEAU · F5 / MAJ+F6 : COPIER / DEPLACER EN FACE", false);
    }

    /// Ouvre le second panneau sur cet emplacement (F6 : le même que le premier ; une session : celui qu'elle a gardé)
    pub(crate) unsafe fn double_panneau_sur(&mut self, hwnd: HWND, emp: Emplacement) {
        let mut o = App::nouveau(emp);
        o.disques = self.app.disques.clone();
        o.nuages = self.app.nuages.clone();
        o.serveurs = self.app.serveurs.clone();
        o.etats = self.etats.clone();
        o.epingles = self.app.epingles.clone();
        o.charger(hwnd);
        self.second = Some(o);
        self.focus_droite = false;
    }

    /// F5 (double panneau), Maj+F5 / Maj+F6 : copie ou déplace la sélection dans le dossier de l'autre panneau
    pub(crate) unsafe fn vers_autre(&mut self, hwnd: HWND, deplacer: bool) {
        let Some(dest) = self.second.as_ref().and_then(|o| o.dossier().map(str::to_string)) else {
            self.app.annoncer("OUVREZ LE DOUBLE PANNEAU (F6) SUR UN DOSSIER", true);
            return;
        };
        let chemins = self.app.chemins_choisis();
        if chemins.is_empty() || self.app.sans_fichiers() {
            return;
        }
        // L'autre panneau resélectionne ce qui arrive
        self.app.deposer(hwnd, chemins, dest, deplacer);
    }

    /// Ctrl+D : épingle le dossier ouvert dans le volet (ou l'en retire)
    pub(crate) fn epingler(&mut self) {
        // Une recherche dans tout le PC s'épingle comme un dossier virtuel (« ?motif »)
        let Some(d) = (match &self.app.emplacement {
            Emplacement::Globale { motif } => Some(format!("?{motif}")),
            _ => self.app.dossier().map(str::to_string),
        }) else {
            self.app.annoncer("SEUL UN DOSSIER OU UNE RECHERCHE PEUT ETRE EPINGLE", true);
            return;
        };
        let deja = reglages::actuels().epingles.iter().any(|e| e.eq_ignore_ascii_case(&d));
        reglages::modifier(|r| {
            if deja {
                r.epingles.retain(|e| !e.eq_ignore_ascii_case(&d));
            } else {
                r.epingles.push(d.clone());
            }
        });
        self.rafraichir_lieux();
        self.app.annoncer(if deja { "RETIRE DES EPINGLES" } else if d.starts_with('?') { "RECHERCHE ENREGISTREE DANS LE VOLET" } else { "EPINGLE DANS LE VOLET" }, false);
        sons::jouer(sons::Son::Tic);
    }

    /// Recalcule les propositions de la palette
    pub(crate) fn remplir_palette(&mut self) {
        let r = reglages::actuels();
        let dossier_ouvert = self.app.dossier().map(str::to_string);
        // Les éléments du dossier ouvert : le chemin se compose sans rien lire (jamais `chemin_de`, qui touche la corbeille ou le disque)
        let fichiers = match &dossier_ouvert {
            Some(d) => self.app.entrees.iter().take(5000).map(|e| (dossier::joindre(d, &e.nom), e.est_conteneur(), icones::pour(e))).collect(),
            None => Vec::new(),
        };
        // Corbeille et panneau : la sélection y donne les fichiers `$R…`, pas les éléments que la liste montre. La palette ne la propose
        // pas (elle les déplacerait ou les jetterait sous un faux nom) ; les éléments restent atteignables en tapant leur nom.
        let selection = if self.app.sans_fichiers() || assistant::emplacement_sans_selection(&self.app.emplacement) {
            Vec::new()
        } else {
            self.app.choisis().into_iter().filter_map(|i| self.app.entrees.get(i)).filter_map(|e| Some((self.app.chemin_de(e)?, e.est_conteneur(), icones::pour(e)))).collect()
        };
        let volet = self.app.favoris.iter().chain(self.app.epingles.iter()).filter_map(|v| if let Emplacement::Dossier(c) = &v.emplacement { Some((v.nom.clone(), c.clone())) } else { None }).collect();
        let autre_panneau = self.second.as_ref().and_then(|o| o.dossier().map(str::to_string));
        let recents_elements = assistant::elements_recents(&r.reprises, &r.recents);
        if let Some(p) = &mut self.palette {
            p.remplir(&palette::Sources {
                epingles: &r.epingles,
                recents: &r.recents,
                recents_elements,
                fichiers,
                groupes: &self.app.groupes,
                sessions: &r.sessions,
                selection,
                dossier_ouvert,
                volet,
                autre_panneau,
            });
        }
    }

    /// Emplacements de tous les onglets, dans l'ordre de la barre
    pub(crate) fn emplacements(&self) -> Vec<Emplacement> {
        let mut v: Vec<Emplacement> = self.fond.iter().map(|a| a.emplacement.clone()).collect();
        v.insert(self.rang, self.app.emplacement.clone());
        v
    }

    /// Rouvre le dernier onglet fermé, à l'emplacement où il était
    pub(crate) unsafe fn rouvrir_onglet(&mut self, hwnd: HWND) {
        match self.fermes.pop() {
            Some(emp) => self.nouvel_onglet(hwnd, emp),
            None => self.app.annoncer("AUCUN ONGLET FERME A ROUVRIR", false),
        }
    }

    pub(crate) unsafe fn fermer_onglet(&mut self, hwnd: HWND, k: usize) {
        if self.fond.is_empty() {
            let _ = PostMessageW(hwnd, WM_CLOSE, WPARAM(0), LPARAM(0));
            return;
        }
        let parti = if k == self.rang { Some(&self.app) } else if k < self.nb_onglets() { self.fond.get(if k < self.rang { k } else { k - 1 }) } else { None };
        if let Some(e) = parti.map(|a| a.emplacement.clone()).filter(|e| *e != Emplacement::Reglages) {
            self.fermes.push(e);
            if self.fermes.len() > 10 {
                self.fermes.remove(0);
            }
        }
        if k == self.rang {
            self.app = if self.rang < self.fond.len() {
                self.fond.remove(self.rang)
            } else {
                self.rang -= 1;
                self.fond.remove(self.rang)
            };
            self.apercu_demande = None;
        } else if k < self.nb_onglets() {
            let j = if k < self.rang { k } else { k - 1 };
            self.fond.remove(j);
            if k < self.rang {
                self.rang -= 1;
            }
        }
    }
}

/// Relecture du 14/11 (audit, phase 4) : la durée de vie `'a` est libre et rien n'empêche d'appeler `etat_de` deux fois pour
/// la même fenêtre. Deux `&mut Fenetre` vivants en même temps seraient un comportement indéfini : un gestionnaire qui tient `f`
/// ne doit donc appeler aucune API Win32 qui envoie un message synchrone à `hwnd` (SetWindowPos, DestroyWindow, SendMessage…)
/// avant d'avoir fini de s'en servir ; il poste (`PostMessageW`) à la place, comme `WM_APPARENCE`. Le pointeur est nul avant
/// `WM_NCCREATE` et après `WM_DESTROY` : l'appelant doit traiter `None`.
pub(crate) unsafe fn etat_de<'a>(hwnd: HWND) -> Option<&'a mut Fenetre> {
    // SAFETY: fil d'interface seulement ; le pointeur vient de `Box::into_raw` posé à WM_NCCREATE et remis à 0 avant le `drop`
    // de WM_DESTROY. L'exclusivité du `&mut` n'est pas vérifiée ici : voir la règle ci-dessus.
    let p = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut Fenetre;
    p.as_mut()
}

pub(crate) fn x_lparam(l: LPARAM) -> i32 {
    (l.0 & 0xFFFF) as i16 as i32
}

pub(crate) fn y_lparam(l: LPARAM) -> i32 {
    ((l.0 >> 16) & 0xFFFF) as i16 as i32
}

pub(crate) unsafe fn taille_client(hwnd: HWND) -> (u32, u32) {
    let mut r = RECT::default();
    let _ = GetClientRect(hwnd, &mut r);
    ((r.right - r.left) as u32, (r.bottom - r.top) as u32)
}

/// Combinaison écrite comme dans la table des raccourcis (« CTRL+ALT+MAJ+D ») ; rien pour une lettre seule, qui sert à la frappe
pub(crate) fn combinaison(vk: VIRTUAL_KEY, ctrl: bool, alt: bool, maj: bool) -> Option<String> {
    let nom = match vk.0 {
        0x41..=0x5A | 0x30..=0x39 => (vk.0 as u8 as char).to_string(),
        0x70..=0x7B => format!("F{}", vk.0 - 0x6F),
        0x0D => "ENTREE".into(),
        0x2E => "SUPPR".into(),
        0x20 => "ESPACE".into(),
        0x09 => "TAB".into(),
        _ => return None,
    };
    if !(ctrl || alt || nom.starts_with('F') && nom.len() > 1) {
        return None;
    }
    Some([(ctrl, "CTRL+"), (alt, "ALT+"), (maj, "MAJ+")].iter().filter(|x| x.0).map(|x| x.1).collect::<String>() + &nom)
}

/// Les touches de la palette (Ctrl+K et Ctrl+Maj+P par défaut, ou celles choisies dans les réglages) : ouvre la palette, et la
/// ferme quand elle est ouverte
pub(crate) fn bascule_palette(vk: VIRTUAL_KEY, ctrl: bool, alt: bool, maj: bool) -> bool {
    let Some(c) = combinaison(vk, ctrl, alt, maj) else { return false };
    [("PALETTE", "CTRL+K"), ("PALETTESECONDAIRE", "CTRL+MAJ+P")].iter().any(|(nom, defaut)| reglages::touche_de_commande(nom).unwrap_or_else(|| defaut.to_string()) == c)
}

/// Ce que fait F5 selon le mode (lot 15B, `if(k==='F5'&&dual)` de la maquette)
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum ActionF5 {
    /// Copier la sélection dans le dossier de l'autre panneau
    Copier,
    /// Relire le dossier
    Actualiser,
}

/// F5 copie vers l'autre panneau en double panneau (Maj+F5 le fait toujours, et dit d'ouvrir le double panneau s'il manque) ;
/// avec Ctrl ou Alt, ou en simple panneau, il actualise comme avant.
pub(crate) fn action_f5(double: bool, maj: bool, ctrl: bool, alt: bool) -> ActionF5 {
    if maj || (double && !ctrl && !alt) {
        ActionF5::Copier
    } else {
        ActionF5::Actualiser
    }
}

pub(crate) fn touche(vk: VIRTUAL_KEY) -> bool {
    unsafe { GetKeyState(vk.0 as i32) < 0 }
}

/// Mode sombre immersif, couleur de la barre et de la bordure selon le thème. La barre DWM est masquée par le client
/// (WS_SYSMENU retiré) ; la bordure 1 px est peinte par DWM, sans alpha : on lui donne la couleur composée `--s2` sur
/// le fond (#242424 en sombre, #D2D2D3 en clair). La couleur de barre reste définie pour les états transitoires.
pub(crate) unsafe fn appliquer_dwm(hwnd: HWND, clair: bool) {
    // SAFETY: fil d'interface, `hwnd` est la fenêtre principale vivante. Chaque DwmSetWindowAttribute lit exactement 4 octets
    // (i32 ou COLORREF u32) sur une variable locale qui vit pendant l'appel ; un échec est ignoré (attribut non géré).
    let sombre: i32 = if clair { 0 } else { 1 };
    let _ = DwmSetWindowAttribute(hwnd, DWMWA_USE_IMMERSIVE_DARK_MODE, &sombre as *const _ as *const c_void, 4);
    // COLORREF 0x00BBGGRR. La bordure reprend `--s2` composé sur le fond : blanc 14 % sur noir = #242424 ;
    // en clair, encre 16 % sur #F7F7F8 = #D2D2D3.
    let barre: u32 = if clair { 0x00F0_F0F0 } else { 0x0044_4444 };
    let _ = DwmSetWindowAttribute(hwnd, DWMWA_CAPTION_COLOR, &barre as *const _ as *const c_void, 4);
    let bordure: u32 = if clair { 0x00D2_D2D3 } else { 0x0024_2424 };
    let _ = DwmSetWindowAttribute(hwnd, DWMWA_BORDER_COLOR, &bordure as *const _ as *const c_void, 4);
}

/// Relit ce qui peut avoir changé hors de l'application : mouvement réduit (Windows, `shared.json`) et thème (réglage
/// local, avis partagé, Windows). Appelée après un réglage, à un message de changement système, à l'activation de la
/// fenêtre et à la fin d'une lecture de dossier ; la relecture du fichier partagé reste limitée à une par seconde.
/// Rien n'est invalidé si le thème effectif n'a pas changé.
///
/// Ne touche jamais à `Fenetre` (pas de `&mut` refait) : la cible Direct2D compare elle-même le thème de sa création à
/// `theme::clair()` au dessin et se recrée (icônes et trame), donc un thème qui change par un autre chemin que cette
/// fonction est rattrapé au dessin suivant.
pub(crate) unsafe fn rafraichir_apparence(hwnd: HWND) {
    // SAFETY: fil d'interface ; seuls des atomiques, le registre, `shared.json` et des appels DWM / uxtheme sur `hwnd` sont
    // touchés, aucune référence vers l'état de la fenêtre n'est prise (l'appelant peut en tenir une).
    reglages::rafraichir_mouvement();
    theme::recalculer();
    if theme::a_appliquer() {
        let clair = theme::clair();
        appliquer_dwm(hwnd, clair);
        coquille::menus_theme(clair);
        let _ = InvalidateRect(hwnd, None, false);
    }
}

/// Demande à la fenêtre de rafraîchir son apparence au tour de boucle suivant (ex. `WM_ACTIVATE`, envoyé de façon synchrone
/// pendant que l'appelant peut tenir l'état : jamais de travail dans le message d'origine)
pub(crate) const WM_APPARENCE: u32 = WM_APP + 32;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn la_palette_prend_toute_la_souris_et_le_tiroir_sa_zone() {
        // Palette ouverte : tout va à la palette, dans le tiroir ou non
        assert_eq!(destinataire_souris(true, false, false), DestSouris::Palette);
        assert_eq!(destinataire_souris(true, true, true), DestSouris::Palette);
        // Tiroir ouvert : la souris est au tiroir sur son rectangle seulement
        assert_eq!(destinataire_souris(false, true, true), DestSouris::Tiroir);
        assert_eq!(destinataire_souris(false, true, false), DestSouris::Explorateur);
        // Tiroir fermé : un point « dans » son ancien rectangle n'y change rien
        assert_eq!(destinataire_souris(false, false, true), DestSouris::Explorateur);
        assert_eq!(destinataire_souris(false, false, false), DestSouris::Explorateur);
    }

    #[test]
    fn curseur_main_sur_lignes_et_puces_de_la_palette_seulement() {
        use crate::rendu::CiblePalette::*;
        assert!(main_sur_palette(Ligne(3)));
        assert!(main_sur_palette(Puce(0)));
        assert!(!main_sur_palette(Carte));
        assert!(!main_sur_palette(Dehors));
    }

    #[test]
    fn marge_droite_et_bas_de_liste_sont_du_fond_cliquable() {
        let (l, h) = (1200.0f32, 800.0f32);
        let milieu = liste_y() + 100.0;
        // Les lignes s'arrêtent avant la marge droite, qui reste dans la zone des fichiers (menu du fond, lasso, dépôt)
        let bord = bord_droit_ligne(l);
        assert!(dans_ligne(l, bord - 1.0));
        assert!(!dans_ligne(l, bord + 1.0));
        assert!(zone_fichiers(l, h, bord + 1.0, milieu));
        assert!(zone_fichiers(l, h, l - 2.0, milieu));
        // La marge est d'au moins 40 DIP entre les lignes et le bord (la maquette : 16)
        assert!(l - bord >= MARGE_LISTE_DROITE - 6.0 - 0.01);
        // Hors de la zone : sous l'en-tête, au-dessus de la barre d'état, volet à gauche, aperçu à droite
        assert!(!zone_fichiers(l, h, 100.0, milieu));
        assert!(!zone_fichiers(l, h, l + 1.0, milieu));
        assert!(!zone_fichiers(l, h, 600.0, liste_y() - 1.0));
        assert!(!zone_fichiers(l, h, 600.0, h - statut_h()));
    }

    #[test]
    fn ctrl_k_et_ctrl_maj_p_basculent_la_palette() {
        assert!(bascule_palette(VK_K, true, false, false));
        assert!(bascule_palette(VK_P, true, false, true));
        // Ni la lettre seule, ni Ctrl+Maj+K, ni Alt+K, ni Ctrl+P (impression) ni Alt+Espace (lanceur de SpaceNotch)
        assert!(!bascule_palette(VK_K, false, false, false));
        assert!(!bascule_palette(VK_K, true, false, true));
        assert!(!bascule_palette(VK_K, true, true, false));
        assert!(!bascule_palette(VK_P, true, false, false));
        assert!(!bascule_palette(VK_SPACE, false, true, false));
    }

    #[test]
    fn ctrl_k_ne_sert_a_aucune_autre_commande_de_la_table() {
        assert_eq!(combinaison(VK_K, true, false, false).as_deref(), Some("CTRL+K"));
        // Les touches de la palette sont celles de ses deux lignes de la table, et d'aucune autre commande
        let palettes: Vec<&str> = palette::COMMANDES.iter().filter(|c| matches!(c.0, Commande::Palette | Commande::PaletteSecondaire)).map(|c| c.2).collect();
        assert_eq!(palettes, ["CTRL+K", "CTRL+MAJ+P"]);
        assert!(palette::COMMANDES.iter().filter(|c| !matches!(c.0, Commande::Palette | Commande::PaletteSecondaire)).all(|c| !palettes.contains(&c.2)));
    }

    #[test]
    fn f5_copie_en_double_panneau_et_actualise_en_simple() {
        // Double panneau : F5 copie vers l'autre panneau
        assert_eq!(action_f5(true, false, false, false), ActionF5::Copier);
        // Simple panneau : F5 relit le dossier
        assert_eq!(action_f5(false, false, false, false), ActionF5::Actualiser);
        // Maj+F5 copie toujours (et dit d'ouvrir le double panneau s'il manque)
        assert_eq!(action_f5(false, true, false, false), ActionF5::Copier);
        assert_eq!(action_f5(true, true, false, false), ActionF5::Copier);
        // Ctrl+F5 et Alt+F5 gardent leur sens d'avant
        assert_eq!(action_f5(true, false, true, false), ActionF5::Actualiser);
        assert_eq!(action_f5(true, false, false, true), ActionF5::Actualiser);
    }

    #[test]
    fn f5_est_dans_la_table_des_raccourcis_pour_les_deux_modes() {
        let noms: Vec<&str> = palette::COMMANDES.iter().filter(|c| c.2 == "F5").map(|c| c.1).collect();
        assert!(noms.contains(&"ACTUALISER"), "{noms:?}");
        assert!(noms.iter().any(|n| n.starts_with("COPIER VERS L'AUTRE PANNEAU")), "{noms:?}");
        assert!(palette::COMMANDES.iter().any(|c| c.0 == palette::Commande::CopierVersAutre), "F5 en double panneau reste une commande de la table");
    }

    #[test]
    fn les_coordonnees_du_lparam_gardent_leur_signe() {
        // Deux moniteurs : un point à gauche ou au-dessus du principal a une coordonnée négative (mots de 16 bits signés)
        let l = |x: i16, y: i16| LPARAM((((y as u16 as u32) << 16) | x as u16 as u32) as isize);
        assert_eq!((x_lparam(l(10, 20)), y_lparam(l(10, 20))), (10, 20));
        assert_eq!((x_lparam(l(-5, -7)), y_lparam(l(-5, -7))), (-5, -7));
        assert_eq!((x_lparam(l(i16::MIN, i16::MAX)), y_lparam(l(i16::MIN, i16::MAX))), (-32768, 32767));
    }

    #[test]
    fn combinaison_ecrit_les_modificateurs_dans_l_ordre_de_la_table() {
        assert_eq!(combinaison(VK_D, true, true, true).as_deref(), Some("CTRL+ALT+MAJ+D"));
        assert_eq!(combinaison(VK_F2, false, false, false).as_deref(), Some("F2"));
        assert_eq!(combinaison(VK_F5, false, false, true).as_deref(), Some("MAJ+F5"));
        assert_eq!(combinaison(VK_RETURN, true, false, false).as_deref(), Some("CTRL+ENTREE"));
        assert_eq!(combinaison(VK_DELETE, false, true, true).as_deref(), Some("ALT+MAJ+SUPPR"));
        // Sans Ctrl ni Alt, seules les touches F ont un nom : Entrée, Suppr ou Maj+Suppr seuls restent aux gestes de la liste
        assert_eq!(combinaison(VK_RETURN, false, false, false), None);
        assert_eq!(combinaison(VK_DELETE, false, false, true), None);
        // Une lettre seule sert à la frappe, pas à un raccourci ; Maj+lettre aussi (majuscule)
        assert_eq!(combinaison(VK_D, false, false, false), None);
        // Une touche absente de la table n'a pas de nom
        assert_eq!(combinaison(VK_LEFT, true, false, false), None);
    }
}
