// Clavier du panneau terminal : quelles touches vont au shell et lesquelles restent à l'explorateur. Tant que le panneau a le
// focus, tout part vers le shell (flèches, Entrée, Échap, Ctrl+lettre), sauf trois gestes de la fenêtre : Ctrl+K (palette),
// Ctrl+Alt+T (fermer le panneau) et Alt+F4. Ctrl+Maj+J (onglet JOURNAL) et les onglets (Ctrl+Maj+T / W, Ctrl+Tab) sont pris ici
// aussi. Les lettres arrivent par WM_CHAR (`caractere_terminal`), pas par ici.

use crate::commandes::nouvel_onglet_terminal;
use crate::terminal::onglets;
use crate::terminal::panneau::{defilement_de_touche, touche_vers_octets, Mods};
use crate::*;
use std::sync::Arc;

/// Ce que devient une touche enfoncée quand le panneau terminal est affiché
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum SuiteTouche {
    /// Rien pour le terminal : l'explorateur traite la touche comme d'habitude
    Explorateur,
    /// La touche est réglée ici ; son WM_CHAR (Entrée, Tab, Échap, Retour arrière) doit être avalé pour ne pas doubler
    Pris,
    /// La touche est réservée au terminal mais n'a rien produit : son WM_CHAR (lettres, Ctrl+lettre) l'enverra au shell
    Caractere,
}

/// Fonction que la session appelle de son fil : elle ne fait que poster `WM_TERMINAL` à la fenêtre
pub(crate) fn notifier_terminal(hwnd: HWND) -> Arc<dyn Fn() + Send + Sync> {
    // Le HWND passe en entier : le fil de la session ne touche à rien d'autre qu'à ce message
    let h = hwnd.0 as isize;
    Arc::new(move || {
        // SAFETY: PostMessageW est permis depuis n'importe quel fil ; un HWND détruit fait échouer l'appel, sans autre effet
        let _ = unsafe { PostMessageW(HWND(h as *mut _), WM_TERMINAL, WPARAM(0), LPARAM(0)) };
    })
}

impl Fenetre {
    /// Le clavier va au terminal : panneau affiché avec le focus, et aucune saisie ni fenêtre posée par-dessus
    pub(crate) fn terminal_a_le_clavier(&self) -> bool {
        self.terminal_affiche()
            && self.terminal_focus
            && !self.terminal_journal
            && !self.modale()
            && !self.historique
            && self.app.saisie_chemin.is_none()
            && self.app.edition.is_none()
            && self.page.formulaire.is_none()
    }

    /// Redimensionne la grille et la pseudo-console à la taille du panneau (rien si elle n'a pas changé)
    ///
    /// # Safety
    /// Fil d'interface seulement, `hwnd` étant la fenêtre qui possède cette `Fenetre` (jamais détruite pendant l'appel).
    pub(crate) unsafe fn ajuster_terminal(&mut self, hwnd: HWND) {
        if !self.terminal_affiche() {
            return;
        }
        let (colonnes, lignes) = self.dimensions_terminal(hwnd);
        // Tous les onglets : celui qu'on rouvre doit déjà avoir la bonne taille (sinon le shell s'affiche de travers)
        for p in &mut self.terminaux {
            p.redimensionner(colonnes, lignes);
        }
    }
}

const VK_J: u32 = 0x4A;
const VK_K: u32 = 0x4B;
const VK_T: u32 = 0x54;
const VK_C: u32 = 0x43;
const VK_V: u32 = 0x56;
const VK_W: u32 = 0x57;
const VK_TAB: u32 = 0x09;
const VK_PGHAUT: u32 = 0x21;
const VK_PGBAS: u32 = 0x22;
const VK_F4: u32 = 0x73;
const VK_RETOUR: u32 = 0x0D;
const VK_ECHAP: u32 = 0x1B;
const VK_INSERER: u32 = 0x2D;

/// Une touche enfoncée alors que le terminal a le clavier (`f.terminal_a_le_clavier()` est vrai)
///
/// # Safety
/// Fil d'interface seulement, depuis la procédure de `hwnd` (appels Win32 sur cette fenêtre et le presse-papiers).
pub(crate) unsafe fn touche_terminal(hwnd: HWND, f: &mut Fenetre, vk: VIRTUAL_KEY, ctrl: bool, alt: bool, maj: bool) -> SuiteTouche {
    let v = u32::from(vk.0);
    // Les trois gestes qui restent à la fenêtre
    if (ctrl && !alt && v == VK_K) || (ctrl && alt && v == VK_T) || (alt && v == VK_F4) {
        return SuiteTouche::Explorateur;
    }
    // Ctrl+Maj+J : l'onglet JOURNAL (T4c) ; son WM_CHAR (Ctrl+J = saut de ligne) est avalé
    if ctrl && maj && !alt && v == VK_J {
        f.basculer_journal_terminal();
        let _ = InvalidateRect(hwnd, None, false);
        return SuiteTouche::Pris;
    }
    // Un modificateur seul ne dit rien au shell
    if matches!(v, 0x10..=0x12 | 0xA0..=0xA5) {
        return SuiteTouche::Caractere;
    }
    // T4a : les onglets du panneau. Pris avant le shell (même terminé) ; leur WM_CHAR (W, T, Tab) est avalé par `Pris`.
    // Ctrl+Maj+T / Ctrl+Maj+W comme Windows Terminal ; tant que le terminal a le clavier, ils ne rouvrent pas un onglet de
    // l'explorateur.
    if ctrl && !alt {
        let n = f.terminaux.len();
        let cible = match (v, maj) {
            (VK_T, true) => {
                nouvel_onglet_terminal(hwnd, f);
                let _ = InvalidateRect(hwnd, None, false);
                return SuiteTouche::Pris;
            }
            (VK_W, true) => {
                let actif = f.terminal_actif;
                f.fermer_onglet_terminal(actif);
                let _ = InvalidateRect(hwnd, None, false);
                return SuiteTouche::Pris;
            }
            (VK_TAB, false) | (VK_PGBAS, false) => Some(onglets::suivant(f.terminal_actif, n)),
            (VK_TAB, true) | (VK_PGHAUT, false) => Some(onglets::precedent(f.terminal_actif, n)),
            _ => None,
        };
        if let Some(i) = cible {
            f.choisir_onglet_terminal(i);
            let _ = InvalidateRect(hwnd, None, false);
            return SuiteTouche::Pris;
        }
    }
    f.charger_historique_terminal();
    // Champs nommés (pas `terminal_mut()`) : le panneau et l'historique sont empruntés ensemble
    let Some(p) = f.terminaux.get_mut(f.terminal_actif).map(|b| &mut **b) else { return SuiteTouche::Explorateur };

    // Shell terminé (ou jamais démarré) : Entrée relance, Échap ferme l'onglet (le dernier ferme le panneau), le reste est
    // sans effet
    if !p.actif() {
        match v {
            VK_RETOUR => {
                p.relancer(notifier_terminal(hwnd));
                let _ = InvalidateRect(hwnd, None, false);
            }
            VK_ECHAP => {
                let actif = f.terminal_actif;
                f.fermer_onglet_terminal(actif);
                let _ = InvalidateRect(hwnd, None, false);
            }
            _ => {}
        }
        return SuiteTouche::Pris;
    }

    let m = Mods { ctrl, alt, maj };

    // Copier : Ctrl+Maj+C, ou Ctrl+C quand du texte est sélectionné (sans sélection, Ctrl+C reste l'interruption du shell)
    if ctrl && !alt && v == VK_C && (maj || p.a_selection()) {
        if let Some(t) = p.texte_selection() {
            operations::copier_texte(hwnd, &t);
            p.effacer_selection();
            let _ = InvalidateRect(hwnd, None, false);
        }
        return SuiteTouche::Pris;
    }
    // Coller : Ctrl+V, Ctrl+Maj+V ou Maj+Inser
    if (ctrl && !alt && v == VK_V) || (maj && !ctrl && !alt && v == VK_INSERER) {
        if let Some(t) = operations::lire_texte(hwnd) {
            // T5 : une invite vide ou une ligne de SpaceFiles prend le collage ; sinon (ou trop long) il part au shell
            if !p.coller_ligne(&t, &f.historique_terminal) {
                let octets = p.octets_de_collage(&t);
                p.envoyer(&octets);
            }
            p.effacer_selection();
            let _ = InvalidateRect(hwnd, None, false);
        }
        return SuiteTouche::Pris;
    }
    // Défilement de l'historique (Maj+PgHaut / PgBas, Ctrl+Maj+Début / Fin)
    if let Some(d) = defilement_de_touche(v, m) {
        if p.defiler_clavier(d) {
            let _ = InvalidateRect(hwnd, None, false);
        }
        return SuiteTouche::Pris;
    }
    // T5 : à l'invite, la ligne de SpaceFiles prend l'édition, le rappel et la suggestion ; Entrée l'envoie au shell
    if let Some(soumise) = p.touche_ligne(v, m, &f.historique_terminal) {
        if let Some(ligne) = soumise {
            let dossier = p.dossier_connu().to_string();
            enregistrer_commande(&mut f.historique_terminal, &ligne, &dossier);
        }
        p.effacer_selection();
        let _ = InvalidateRect(hwnd, None, false);
        return SuiteTouche::Pris;
    }
    // Entrée, Tab, Échap, flèches, F1 à F12, Alt+lettre… : traduits ici
    if let Some(octets) = touche_vers_octets(v, m, &p.modes()) {
        p.envoyer(&octets);
        p.effacer_selection();
        let _ = InvalidateRect(hwnd, None, false);
        return SuiteTouche::Pris;
    }
    SuiteTouche::Caractere
}

/// Un caractère (WM_CHAR) tapé alors que le terminal a le clavier : il part vers le shell. Rend `true` si le message est réglé.
///
/// # Safety
/// Fil d'interface seulement, depuis la procédure de `hwnd`.
pub(crate) unsafe fn caractere_terminal(hwnd: HWND, f: &mut Fenetre, unite: u16) -> bool {
    if f.terminal_mut().is_none() {
        return false;
    }
    f.charger_historique_terminal();
    // Champs nommés (pas `terminal_mut()`) : la liste automatique lit l'historique pendant que le panneau est emprunté
    let Some(p) = f.terminaux.get_mut(f.terminal_actif).map(|b| &mut **b) else { return false };
    if !p.actif() {
        return true;
    }
    // T5 : à l'invite, le caractère entre dans la ligne de SpaceFiles (octets vides) ; sinon il part au shell
    let octets = p.caractere_ligne(unite, &f.historique_terminal);
    if !octets.is_empty() {
        p.envoyer(&octets);
    }
    p.effacer_selection();
    let _ = InvalidateRect(hwnd, None, false);
    true
}

/// Fichier de l'historique des commandes du terminal (`%LOCALAPPDATA%` : jamais synchronisé d'un PC à l'autre, c'est un choix de T5)
fn chemin_historique() -> Option<std::path::PathBuf> {
    crate::reglages::dossier_donnees(true).map(|d| d.join("historique-terminal.txt"))
}

/// Historique de PSReadLine (PowerShell 5 et 7 le partagent) : lu une seule fois, pour amorcer le nôtre
fn chemin_psreadline() -> Option<std::path::PathBuf> {
    let base = std::env::var_os("APPDATA")?;
    Some(std::path::Path::new(&base).join(r"Microsoft\Windows\PowerShell\PSReadLine\ConsoleHost_history.txt"))
}

/// Au-delà de cette taille le fichier est réécrit à la relecture (les doublons et les entrées en trop disparaissent)
const TAILLE_COMPACTAGE: u64 = 1_000_000;

/// Date en secondes Unix (0 si l'horloge est avant 1970)
fn maintenant() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

/// Note une commande envoyée au shell : en mémoire, puis à la fin du fichier. Les lignes qui ressemblent à un secret ne sont
/// ni gardées ni écrites (`Historique::ajouter` les refuse).
fn enregistrer_commande(h: &mut crate::terminal::historique::Historique, ligne: &str, dossier: &str) {
    let Some(entree) = h.ajouter(ligne, dossier, maintenant()) else { return };
    if let Some(chemin) = chemin_historique() {
        crate::terminal::historique::ajouter_au_fichier(&chemin, entree);
    }
}

impl Fenetre {
    /// Lit l'historique des commandes à la première frappe dans le terminal (pas au démarrage : budget de démarrage)
    pub(crate) fn charger_historique_terminal(&mut self) {
        if self.historique_terminal_charge {
            return;
        }
        self.historique_terminal_charge = true;
        let Some(chemin) = chemin_historique() else { return };
        // Premier usage (pas de fichier) : on amorce avec l'historique de PSReadLine, écrit une fois pour toutes dans le nôtre
        if !chemin.exists() {
            if let Some(octets) = chemin_psreadline().and_then(|c| std::fs::read(c).ok()) {
                let importe = crate::terminal::historique::depuis_psreadline(&String::from_utf8_lossy(&octets));
                if !importe.est_vide() {
                    crate::terminal::historique::reecrire(&chemin, &importe);
                }
            }
        }
        self.historique_terminal = crate::terminal::historique::charger(&chemin);
        let gros = std::fs::metadata(&chemin).map(|m| m.len() > TAILLE_COMPACTAGE).unwrap_or(false);
        if gros {
            crate::terminal::historique::reecrire(&chemin, &self.historique_terminal);
        }
    }
}
