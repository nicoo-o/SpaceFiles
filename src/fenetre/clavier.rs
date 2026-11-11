// Clavier dans la fenêtre : touches de la page, de l'arbre, de la palette, du conflit, du lot et de la galerie, édition de texte.

use crate::*;
/// Touches de la page des réglages. Faux : touche non prise.
pub(crate) unsafe fn touche_page(hwnd: HWND, f: &mut Fenetre, vk: VIRTUAL_KEY, ctrl: bool, maj: bool, alt: bool) -> bool {
    let hauteur = hauteur_page(hwnd, f);
    if let Some(fo) = &mut f.page.formulaire {
        match vk {
            VK_ESCAPE => fermer_formulaire(f),
            VK_TAB if maj => fo.deplacer(-1),
            VK_TAB | VK_DOWN => fo.deplacer(1),
            VK_UP => fo.deplacer(-1),
            VK_RETURN if fo.focus == page::F_ANNULER => fermer_formulaire(f),
            VK_RETURN => soumettre(hwnd, f),
            VK_SPACE if fo.focus == page::F_GENRE => fo.changer_sorte(1),
            VK_SPACE if fo.focus == page::F_ANNULER => fermer_formulaire(f),
            VK_SPACE if fo.focus == page::F_AJOUTER => soumettre(hwnd, f),
            VK_LEFT | VK_RIGHT if fo.focus == page::F_GENRE => fo.changer_sorte(if vk == VK_LEFT { -1 } else { 1 }),
            VK_LEFT if fo.focus == page::F_ANNULER => fo.focus = page::F_AJOUTER,
            VK_RIGHT if fo.focus == page::F_AJOUTER => fo.focus = page::F_ANNULER,
            _ => {
                let Some(ed) = fo.champ() else { return true };
                match vk {
                    VK_LEFT => ed.gauche(maj),
                    VK_RIGHT => ed.droite(maj),
                    VK_HOME => ed.aller(0, maj),
                    VK_END => ed.aller(ed.texte.len(), maj),
                    VK_BACK => ed.effacer_arriere(),
                    VK_DELETE => ed.effacer_avant(),
                    VK_A if ctrl => ed.tout(),
                    VK_V if ctrl => {
                        if let Some(s) = operations::lire_texte(hwnd) {
                            ed.inserer(&s);
                        }
                    }
                    _ => return true,
                }
                // Le texte a changé : un nouvel essai vérifie de nouveau
                fo.forcer = false;
            }
        }
        return true;
    }
    let lignes = f.lignes();
    match vk {
        VK_ESCAPE | VK_BACK | VK_BROWSER_BACK => quitter_reglages(hwnd, f),
        VK_LEFT if alt => quitter_reglages(hwnd, f),
        // Maj+↑↓ sur un dossier épinglé : il change de place dans le volet (le focus le suit)
        VK_UP | VK_DOWN if maj && matches!(lignes.get(f.page.focus).map(|l| l.element), Some(Element::Epingle(_))) => {
            let Some(Element::Epingle(i)) = lignes.get(f.page.focus).map(|l| l.element) else { return true };
            let n = reglages::actuels().epingles.len();
            let j = if vk == VK_UP { i.checked_sub(1) } else { (i + 1 < n).then_some(i + 1) };
            if let Some(j) = j {
                reglages::modifier(|r| r.epingles.swap(i, j));
                f.page.focus = if j > i { f.page.focus + 1 } else { f.page.focus - 1 };
                f.rafraichir_lieux();
            }
        }
        // Tab, Maj+Tab, Ctrl+PgDn, Ctrl+PgUp : catégorie suivante ou précédente du rail
        VK_TAB if !ctrl && !alt => f.page.changer_categorie(&lignes, if maj { -1 } else { 1 }),
        VK_PRIOR | VK_NEXT if ctrl && !alt => f.page.changer_categorie(&lignes, if vk == VK_PRIOR { -1 } else { 1 }),
        // Une page sans carte à choisir (Raccourcis) défile à la place
        VK_UP | VK_DOWN | VK_PRIOR | VK_NEXT | VK_HOME | VK_END if !f.page.a_des_choix(&lignes) => {
            let pas = match vk {
                VK_UP => -page::INFO_H,
                VK_DOWN => page::INFO_H,
                VK_PRIOR => -hauteur,
                VK_NEXT => hauteur,
                VK_HOME => -1.0e6,
                _ => 1.0e6,
            };
            f.page.defiler(&lignes, pas, hauteur);
        }
        VK_UP => f.page.deplacer(&lignes, -1),
        VK_DOWN => f.page.deplacer(&lignes, 1),
        VK_PRIOR | VK_NEXT => {
            for _ in 0..6 {
                f.page.deplacer(&lignes, if vk == VK_PRIOR { -1 } else { 1 });
            }
        }
        VK_HOME => f.page.extreme(&lignes, true),
        VK_END => f.page.extreme(&lignes, false),
        VK_RETURN | VK_SPACE => activer_reglage(hwnd, f, f.page.focus, 0),
        VK_LEFT | VK_RIGHT if !alt => {
            if matches!(lignes.get(f.page.focus).map(|l| &l.valeur), Some(page::Valeur::Choix(_) | page::Valeur::Interrupteur(_) | page::Valeur::Segments(..))) {
                activer_reglage(hwnd, f, f.page.focus, if vk == VK_LEFT { -1 } else { 1 });
            }
        }
        VK_DELETE => {
            let retire = match lignes.get(f.page.focus).map(|l| l.element) {
                Some(Element::Ajoute(i)) => {
                    retirer_stockage(f, i);
                    true
                }
                Some(Element::Epingle(i)) => {
                    reglages::modifier(|r| {
                        if i < r.epingles.len() {
                            r.epingles.remove(i);
                        }
                    });
                    f.rafraichir_lieux();
                    true
                }
                Some(Element::Exclu(_) | Element::Regle(_)) => {
                    activer_reglage(hwnd, f, f.page.focus, 0);
                    true
                }
                _ => false,
            };
            if retire {
                let lignes = f.lignes();
                f.page.reajuster(&lignes);
            }
        }
        _ => return false,
    }
    if f.app.emplacement == Emplacement::Reglages {
        let lignes = f.lignes();
        f.page.montrer(&lignes, hauteur);
    }
    true
}

/// Touches d'édition communes aux champs (palette, renommage par lot). Faux : touche non prise.
pub(crate) unsafe fn editer(hwnd: HWND, ed: &mut Edition, vk: VIRTUAL_KEY, ctrl: bool, maj: bool) -> bool {
    match vk {
        VK_LEFT => ed.gauche(maj),
        VK_RIGHT => ed.droite(maj),
        VK_HOME => ed.aller(0, maj),
        VK_END => ed.aller(ed.texte.len(), maj),
        VK_BACK => ed.effacer_arriere(),
        VK_DELETE => ed.effacer_avant(),
        VK_A if ctrl => ed.tout(),
        VK_C if ctrl => operations::copier_texte(hwnd, &ed.texte_choisi()),
        VK_X if ctrl => {
            let s = ed.couper();
            operations::copier_texte(hwnd, &s);
        }
        VK_V if ctrl => {
            if let Some(s) = operations::lire_texte(hwnd) {
                ed.inserer(&s.replace(['\r', '\n'], " "));
            }
        }
        _ => return false,
    }
    true
}

/// Clavier dans l'arbre du volet : ↑↓ Début Fin déplacent, → déplie ou descend, ← replie ou remonte,
/// Entrée ouvre le dossier, Échap rend la main à la liste. Faux : touche non prise.
pub(crate) unsafe fn touche_arbre(hwnd: HWND, f: &mut Fenetre, vk: VIRTUAL_KEY) -> bool {
    let lignes = f.app.lignes_arbre();
    if lignes.is_empty() {
        f.app.arbre_focus = false;
        return false;
    }
    let courant = f.app.dossier().map(arbre::cle).unwrap_or_default();
    let sous_curseur = f.app.arbre_curseur.clone().unwrap_or(courant);
    let n = lignes.len();
    let i = lignes.iter().position(|l| arbre::cle(&l.chemin) == sous_curseur).unwrap_or(0);
    let l = &lignes[i];
    let mut j = i;
    match vk {
        VK_UP => j = i.saturating_sub(1),
        VK_DOWN => j = (i + 1).min(n - 1),
        VK_HOME => j = 0,
        VK_END => j = n - 1,
        VK_RIGHT => {
            if l.ouvert && l.enfants == Some(true) {
                j = (i + 1).min(n - 1);
            } else if l.enfants != Some(false) {
                arbre::deplier(&l.chemin);
            }
        }
        VK_LEFT => {
            if l.ouvert {
                arbre::replier(&l.chemin);
            } else if let Some(p) = (0..i).rev().find(|&k| lignes[k].profondeur < l.profondeur) {
                j = p;
            }
        }
        VK_RETURN => f.app.naviguer(hwnd, Emplacement::dossier(&l.chemin)),
        VK_ESCAPE => f.app.arbre_focus = false,
        _ => return false,
    }
    f.app.arbre_curseur = Some(arbre::cle(&lignes[j].chemin));
    montrer_ligne_arbre(hwnd, f, j);
    true
}

pub(crate) unsafe fn touche_palette(hwnd: HWND, f: &mut Fenetre, vk: VIRTUAL_KEY, ctrl: bool, maj: bool) {
    let Some(p) = &mut f.palette else { return };
    // Une commande vient d'être lancée (yeux contents, 160 ms) : la palette n'écoute plus
    if p.lance {
        return;
    }
    match vk {
        VK_ESCAPE => f.palette = None,
        // Ctrl+K et Ctrl+Maj+P ferment la palette comme ils l'ouvrent
        _ if bascule_palette(vk, ctrl, touche(VK_MENU), maj) => f.palette = None,
        // Entrée : l'étape suivante, ou la commande part ; Ctrl+Entrée : valide le dossier visé comme « ici »
        VK_RETURN => lancer_palette(hwnd, f, ctrl),
        VK_UP => p.deplacer(-1),
        VK_DOWN => p.deplacer(1),
        // Tab garde son rôle de flèche : les catégories du lot 15A ont laissé la place aux étapes
        VK_TAB => p.deplacer(if maj { -1 } else { 1 }),
        VK_PRIOR => p.deplacer(-8),
        VK_NEXT => p.deplacer(8),
        // Retour arrière sur un champ vide : l'étape précédente
        VK_BACK if p.est_assistant() && p.saisie.texte.is_empty() => {
            if p.reculer() {
                f.remplir_palette();
            }
        }
        // ← sur un champ vide : l'étape précédente, comme Retour arrière ; → sur un champ vide : l'étape suivante, comme Entrée. Champ
        // non vide : ces flèches déplacent le curseur de texte (elles tombent dans `editer`).
        VK_LEFT if p.est_assistant() && p.saisie.texte.is_empty() && !ctrl && !maj => {
            if p.reculer() {
                f.remplir_palette();
            }
        }
        VK_RIGHT if p.est_assistant() && p.saisie.texte.is_empty() && !ctrl && !maj => lancer_palette(hwnd, f, false),
        // Ctrl+Z sur le résultat : « Annuler », seulement si ce résultat a une entrée à défaire (un renommage échoué, la copie d'un
        // chemin ou une compression n'en ont pas : Ctrl+Z défairait sinon une autre action de la pile globale)
        VK_Z if ctrl && p.peut_defaire() => defaire_etape(hwnd, f),
        _ if !p.accepte_saisie() => {}
        _ => {
            let avant = p.requete();
            if editer(hwnd, &mut p.saisie, vk, ctrl, maj) {
                p.choix = 0;
                // Les yeux de Pixel clignent 120 ms après une frappe (`input`), pas après ← → ou Ctrl+A
                let change = p.requete() != avant;
                f.remplir_palette();
                if change {
                    armer_clignement_palette(hwnd);
                }
            }
        }
    }
    lectures_palette(hwnd, f);
}

/// Entrée (ou clic sur une ligne). Dans la palette à étapes, `Palette::entrer` change d'étape ou rend la commande prête ; une commande
/// générale (« Partout ») ou un lieu part comme avant : les yeux de Pixel passent contents (`palRun`), la commande suit 160 ms plus tard.
/// Sans animation, sans yeux ou dans un mode qui demande un nom : elle part tout de suite.
pub(crate) unsafe fn lancer_palette(hwnd: HWND, f: &mut Fenetre, ctrl: bool) {
    let Some(p) = f.palette.as_mut().filter(|p| !p.lance) else { return };
    if !p.est_assistant() {
        executer_palette(hwnd, f);
        return;
    }
    match p.entrer(ctrl) {
        palette::Suite::Rien => f.remplir_palette(),
        palette::Suite::Executer(ex) => executer_etape(hwnd, f, ex),
        palette::Suite::Defaire => defaire_etape(hwnd, f),
        palette::Suite::Lancer => {
            let attendre = reglages::animations_actives() && reglages::yeux_pixel_actifs();
            if !attendre || p.items.is_empty() {
                executer_palette(hwnd, f);
            } else {
                p.lance = true;
                // SAFETY: fil d'interface, `hwnd` est la fenêtre principale vivante
                if SetTimer(hwnd, minuteries::MINUTERIE_PALETTE_LANCE, palette::LANCEMENT_MS, None) == 0 {
                    // Sans minuterie rien ne refermerait la palette : la commande part tout de suite
                    p.lance = false;
                    executer_palette(hwnd, f);
                }
            }
        }
    }
    lectures_palette(hwnd, f);
}

/// Carte des conflits : R G N I (Maj : pour tous les restants), ← → pour passer de l'un à l'autre, Échap abandonne
pub(crate) unsafe fn touche_conflit(hwnd: HWND, a: &mut App, vk: VIRTUAL_KEY, maj: bool) {
    let Some(c) = a.conflits.as_mut() else { return };
    match vk {
        VK_ESCAPE => a.abandonner_conflits(hwnd),
        VK_LEFT | VK_UP => c.deplacer(-1),
        VK_RIGHT | VK_DOWN | VK_TAB => c.deplacer(1),
        _ => {
            if let Some(choix) = char::from_u32(vk.0 as u32).and_then(conflit::Choix::de_touche) {
                choisir_conflit(hwnd, a, choix, maj);
            }
        }
    }
}

/// Réponse au conflit affiché ; quand tout est répondu, l'opération part
pub(crate) unsafe fn choisir_conflit(hwnd: HWND, a: &mut App, choix: conflit::Choix, tous: bool) {
    let Some(c) = a.conflits.as_mut() else { return };
    sons::jouer(sons::Son::Tic);
    if c.choisir(choix, tous) {
        let Some(c) = a.conflits.take() else { return };
        let (genre, vides, trace, apres) = (c.genre, c.vides.clone(), c.trace, c.apres.clone());
        a.lancer_elements(hwnd, genre, c.elements(), vides, trace, apres);
    }
}

pub(crate) unsafe fn touche_lot(hwnd: HWND, f: &mut Fenetre, vk: VIRTUAL_KEY, ctrl: bool, maj: bool) {
    let Some(l) = &mut f.lot else { return };
    match vk {
        VK_ESCAPE => f.lot = None,
        VK_RETURN => {
            let Some(l) = f.lot.take() else { return };
            match f.app.appliquer_lot(hwnd, &l) {
                Ok(n) => {
                    f.app.annoncer(format!("{n} ELEMENTS RENOMMES · CTRL+Z POUR ANNULER"), false);
                    sons::jouer(sons::Son::Fini);
                }
                Err(e) => {
                    f.app.annoncer(e, true);
                    sons::jouer(sons::Son::Erreur);
                    f.lot = Some(l);
                }
            }
        }
        VK_TAB | VK_DOWN if !maj => l.focus = (l.focus + 1) % 3,
        VK_TAB | VK_UP => l.focus = (l.focus + 2) % 3,
        VK_PRIOR => l.defilement = l.defilement.saturating_sub(8),
        VK_NEXT => l.defilement = (l.defilement + 8).min(l.noms.len().saturating_sub(1)),
        VK_R if ctrl => l.regex = !l.regex,
        _ => {
            let k = l.focus;
            editer(hwnd, &mut l.champs[k], vk, ctrl, maj);
        }
    }
}

pub(crate) unsafe fn touche_galerie(hwnd: HWND, f: &mut Fenetre, vk: VIRTUAL_KEY) {
    let n = f.app.entrees.len() as i64;
    let i = f.app.selection.unwrap_or(0) as i64;
    let aller = |f: &mut Fenetre, k: i64| {
        if n > 0 {
            f.app.choisir_seul(k.clamp(0, n - 1) as usize);
            f.app.montrer_selection();
            ouvrir_galerie(hwnd, f);
        }
    };
    match vk {
        VK_ESCAPE | VK_SPACE => f.galerie = None,
        VK_LEFT | VK_UP | VK_PRIOR => aller(f, i - 1),
        VK_RIGHT | VK_DOWN | VK_NEXT => aller(f, i + 1),
        VK_HOME => aller(f, 0),
        VK_END => aller(f, n - 1),
        VK_RETURN => {
            f.galerie = None;
            if let Some(k) = f.app.selection {
                f.app.ouvrir(hwnd, k);
            }
        }
        _ => {}
    }
}
