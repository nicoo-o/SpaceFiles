// Actions sur les fichiers et les vues : corbeille, comparaison, texte d'une image, onglets copiés, chemin reçu d'une autre application (reuse), groupes, lot, galerie, palette.

use crate::*;

/// Partage à proximité (O5) : la feuille de partage de Windows (Partage à proximité, Phone Link…)
pub(crate) unsafe fn partage_proche(hwnd: HWND, f: &mut Fenetre) {
    verbe_shell(hwnd, f, "Windows.ModernShare");
}

/// Vide la Corbeille (Windows demande confirmation), puis ses points se dissolvent (O7)
pub(crate) unsafe fn vider_corbeille(hwnd: HWND, f: &mut Fenetre) {
    let avant = corbeille::etat().map_or(0, |e| corbeille::points(&e));
    if corbeille::vider(hwnd) {
        // Mouvement réduit : la carte montre tout de suite la Corbeille vide, sans la dissolution de 1,2 s
        if reglages::animations_actives() {
            f.app.vidage = Some((std::time::Instant::now(), avant));
        }
        f.app.annoncer("CORBEILLE VIDEE", false);
        crate::app::toaster("Corbeille vidée");
        sons::jouer(sons::Son::Corbeille);
        animer(hwnd, f);
    }
}

/// Ctrl+K ou Ctrl+Maj+P : palette de commandes
pub(crate) fn ouvrir_palette(f: &mut Fenetre) {
    // L'index est relu dès maintenant : il sera prêt quand on aura tapé trois lettres
    index::demarrer();
    f.palette = Some(Palette::nouvelle());
    f.remplir_palette();
}

/// Après un changement de groupes : volet de tous les onglets, et liste d'un groupe ouvert
pub(crate) unsafe fn rafraichir_groupes(hwnd: HWND, f: &mut Fenetre) {
    let liste = groupes::liste();
    for a in f.tous() {
        a.groupes = liste.clone();
        if matches!(a.emplacement, Emplacement::Groupe(_)) {
            a.actualiser(hwnd);
        }
    }
    let _ = InvalidateRect(hwnd, None, false);
}

/// Z9 : texte de l'image ou du PDF choisi, écrit en .txt dans les données locales et ouvert dans l'aperçu.
/// La lecture (0,3 à 1 s par page, 5 pages de PDF au plus) se fait dans un fil : la fenêtre reste libre,
/// le résultat revient par WM_OCR (voir `texte_lu`).
pub(crate) unsafe fn lire_texte_image(hwnd: HWND, f: &mut Fenetre) {
    let chemins = if f.app.sans_fichiers() { Vec::new() } else { f.app.chemins_choisis() };
    let [c] = chemins.as_slice() else {
        f.app.annoncer("CHOISISSEZ UNE IMAGE OU UN PDF", true);
        return;
    };
    let ext = std::path::Path::new(c).extension().and_then(|e| e.to_str()).unwrap_or("");
    if !ocr::est_lisible(ext) {
        f.app.annoncer("IMAGE (PNG, JPG, BMP, GIF, TIFF) OU PDF ATTENDU", true);
        return;
    }
    if !ocr::reserver() {
        f.app.annoncer("UNE LECTURE EST DEJA EN COURS (AUTRE FENETRE POSSIBLE)", true);
        return;
    }
    f.app.annoncer("LECTURE…", false);
    ocr::lancer(hwnd, c.clone());
}

/// Suite de `lire_texte_image` à l'arrivée du résultat du fil : écrit le .txt et l'ouvre dans l'aperçu
pub(crate) unsafe fn texte_lu(hwnd: HWND, f: &mut Fenetre, r: ocr::Resultat) {
    match ocr::suite(&r.source, r.texte) {
        ocr::Suite::Vide => f.app.annoncer("AUCUN TEXTE RECONNU", false),
        ocr::Suite::Ecrire(fichier, t) => {
            let Some(dossier) = reglages::dossier_donnees(true) else { return };
            let _ = std::fs::create_dir_all(&dossier);
            if std::fs::write(dossier.join(&fichier), t).is_err() {
                f.app.annoncer("TEXTE NON ECRIT", true);
                return;
            }
            f.app.a_reselectionner = vec![fichier];
            f.apercu_visible = true;
            f.app.naviguer(hwnd, Emplacement::Dossier(dossier.to_string_lossy().into_owned()));
            f.app.annoncer(format!("TEXTE LU EN {} MS", r.duree_ms), false);
        }
        ocr::Suite::Erreur(e) => f.app.annoncer(e, true),
    }
}

/// Marque des messages WM_COPYDATA qui portent des onglets (V6) : « NE » + 1
pub(crate) const ONGLETS_COPIES: usize = 0x4E45_0001;

/// Envoie ces dossiers à une autre fenêtre de l'explorateur (pas celle-ci). Vrai si elle les a reçus.
pub(crate) unsafe fn envoyer_onglets(hwnd: HWND, dossiers: &[String]) -> bool {
    use windows::Win32::System::DataExchange::COPYDATASTRUCT;
    let utilisable = |h: HWND| h != hwnd && IsWindowVisible(h).as_bool();
    // Les fenêtres de cette classe : au plus quelques-unes, la première utilisable est prise
    let mut cible = HWND(std::ptr::null_mut());
    let mut h = FindWindowExW(None, None, w!("SpaceFiles"), None).unwrap_or_default();
    while !h.is_invalid() {
        if utilisable(h) {
            cible = h;
            break;
        }
        h = FindWindowExW(None, h, w!("SpaceFiles"), None).unwrap_or_default();
    }
    if cible.is_invalid() {
        return false;
    }
    let texte: Vec<u16> = dossiers.join("\n").encode_utf16().collect();
    let cd = COPYDATASTRUCT { dwData: ONGLETS_COPIES, cbData: (texte.len() * 2) as u32, lpData: texte.as_ptr() as *mut _ };
    // Réponse attendue 2 s au plus : une fenêtre figée ne bloque pas celle-ci
    let mut r = 0usize;
    SendMessageTimeoutW(cible, WM_COPYDATA, WPARAM(hwnd.0 as usize), LPARAM(&cd as *const _ as isize), SMTO_ABORTIFHUNG, 2000, Some(&mut r));
    r == 1
}

/// Résultats des fils courts de `recevoir_ouvrir_ici`, en attente d'être ouverts en onglets par la fenêtre à qui ils sont destinés
pub(crate) static OUVERTURES: eco::reutiliser::FileOuvertures = std::sync::Mutex::new(std::collections::VecDeque::new());
/// Fils courts en vol (4 au plus, toutes fenêtres du processus confondues)
static FILS_OUVRIR_ICI: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// Réception de `OUVRIR_ICI` (`explorer.open {reuse}`, voir `eco::reutiliser`). N'importe quel processus du même utilisateur
/// peut écrire à cette fenêtre : la charge est relue et revalidée comme une demande du tube (`eco::reutiliser::decoder`),
/// toute autre charge est refusée, de même qu'une charge de plus de 1,5 s (le résident s'est déjà replié : ouvrir aussi ici
/// donnerait deux ouvertures) ou quand 4 consultations sont déjà en vol (le résident se replie). La fenêtre est restaurée et
/// mise au premier plan tout de suite. La consultation du disque (un partage réseau mort peut durer très longtemps) se fait
/// dans un fil court, jamais ici : le résultat est déposé dans `OUVERTURES` et `WM_OUVRIR_ICI` (sans pointeur) demande à la
/// fenêtre de le prendre ; `ouvrir_ici` ouvre alors l'onglet. Rend vrai quand la demande est acceptée et son traitement lancé
/// (c'est la réponse `1` du `WM_COPYDATA`) ; l'onglet apparaît juste après.
///
/// SAFETY (appelant) : à n'appeler que depuis le traitement de `WM_COPYDATA` de la fenêtre `hwnd`, sur son fil d'interface, avec
/// `cd` lu depuis le `lparam` de ce message (non nul, vérifié par l'appelant) : le bloc `lpData` / `cbData` est alors lisible
/// jusqu'au retour.
pub(crate) unsafe fn recevoir_ouvrir_ici(hwnd: HWND, cd: &windows::Win32::System::DataExchange::COPYDATASTRUCT) -> bool {
    use eco::reutiliser::{decoder, liberer, maintenant_ms, reserver, trop_ancienne, CHARGE_MAX, FILS_MAX};
    if cd.lpData.is_null() || cd.cbData as usize > CHARGE_MAX {
        return false;
    }
    // SAFETY: contrat de l'appelant (WM_COPYDATA : cbData octets lisibles pendant l'appel) ; la taille est bornée à 64 Ko
    // juste au-dessus, et `decoder` copie ce qu'il garde avant que l'appel ne rende la main.
    let octets = std::slice::from_raw_parts(cd.lpData as *const u8, cd.cbData as usize);
    let Some(charge) = decoder(octets) else { return false };
    if trop_ancienne(charge.tick, maintenant_ms()) || etat_de(hwnd).is_none() {
        return false;
    }
    if !reserver(&FILS_OUVRIR_ICI, FILS_MAX) {
        return false;
    }
    // Tout de suite, avant le disque : la fenêtre se montre même si la lecture du dossier traîne
    if IsIconic(hwnd).as_bool() {
        let _ = ShowWindow(hwnd, SW_RESTORE);
    }
    // Refusé par Windows si le droit de premier plan n'a pas été cédé : la fenêtre clignote alors dans la barre des tâches
    let _ = SetForegroundWindow(hwnd);
    let fenetre = hwnd.0 as isize;
    let lance = std::thread::Builder::new()
        .name("ouvrir-ici".into())
        .spawn(move || {
            let r = resoudre_ouverture(&charge.chemin, charge.reveler, |p| std::path::Path::new(p).is_dir());
            if eco::reutiliser::deposer(&OUVERTURES, fenetre, r) {
                // SAFETY: aucun pointeur dans le message ; si la fenêtre n'existe plus le post échoue et le résultat
                // déposé est retiré tout de suite (rien ne reste dans la file).
                let poste = unsafe { PostMessageW(HWND(fenetre as *mut _), WM_OUVRIR_ICI, WPARAM(0), LPARAM(0)) };
                if poste.is_err() {
                    eco::reutiliser::vider(&OUVERTURES, fenetre);
                }
            }
            liberer(&FILS_OUVRIR_ICI);
        })
        .is_ok();
    if !lance {
        liberer(&FILS_OUVRIR_ICI);
    }
    lance
}

/// Suite de `recevoir_ouvrir_ici` : un onglet sur le dossier résolu et l'élément choisi. Ce qui visait l'onglet affiché (menu
/// ouvert, palette, renommage par lot, renommage en cours, galerie, dépôt du bouton droit) est fermé AVANT que `nouvel_onglet`
/// ne remplace `f.app` : sinon un index de menu ou une saisie en cours s'appliquerait au nouvel onglet.
pub(crate) unsafe fn ouvrir_ici(hwnd: HWND, f: &mut Fenetre, (dossier, nom): (String, Option<String>)) {
    fermer_menu(f);
    f.palette = None;
    f.lot = None;
    f.galerie = None;
    f.depot_droit = None;
    f.app.annuler_renommage();
    f.nouvel_onglet(hwnd, Emplacement::dossier(&dossier));
    // La lecture du dossier est lancée mais son résultat n'arrive que par un message, traité après celui-ci : la
    // sélection est donc prise en compte à l'arrivée de la liste (comme pour `--montrer` au démarrage)
    if let Some(nom) = nom {
        f.app.a_reselectionner = vec![nom];
    }
    let _ = InvalidateRect(hwnd, None, false);
}

/// Z4 : les deux fichiers texte choisis, comparés ligne à ligne ; le résultat s'ouvre dans l'aperçu
pub(crate) unsafe fn comparer_textes(hwnd: HWND, f: &mut Fenetre) {
    let chemins = if f.app.sans_fichiers() { Vec::new() } else { f.app.chemins_choisis() };
    let [a, b] = chemins.as_slice() else {
        f.app.annoncer("CHOISISSEZ DEUX FICHIERS TEXTE (CTRL+CLIC OU MAJ+FLECHES)", true);
        return;
    };
    // Au-delà, la lecture ralentirait la fenêtre : la comparaison n'est pas faite
    const MAX: u64 = 5 << 20;
    if [a, b].iter().any(|c| std::fs::metadata(c).map_or(true, |m| !m.is_file() || m.len() > MAX)) {
        f.app.annoncer("DEUX FICHIERS DE MOINS DE 5 MO SONT NECESSAIRES", true);
        return;
    }
    let lire = |c: &str| std::fs::read(c).map(|o| String::from_utf8_lossy(&o).into_owned());
    let (Ok(ta), Ok(tb)) = (lire(a), lire(b)) else {
        f.app.annoncer("LECTURE IMPOSSIBLE", true);
        return;
    };
    match comparaison::diff(&ta, &tb) {
        Ok(d) if d.is_empty() => f.app.annoncer("LES DEUX FICHIERS ONT LE MEME TEXTE", false),
        Ok(d) => {
            let nom = |c: &str| c.rsplit('\\').next().unwrap_or(c).to_string();
            let Some(dossier) = reglages::dossier_donnees(true) else { return };
            let _ = std::fs::create_dir_all(&dossier);
            let fichier = format!("{} -- {}.diff", nom(a), nom(b));
            if std::fs::write(dossier.join(&fichier), format!("--- {a}\n+++ {b}\n{d}")).is_err() {
                f.app.annoncer("COMPARAISON NON ECRITE", true);
                return;
            }
            f.app.a_reselectionner = vec![fichier];
            f.apercu_visible = true;
            f.app.naviguer(hwnd, Emplacement::Dossier(dossier.to_string_lossy().into_owned()));
        }
        Err(e) => f.app.annoncer(e, true),
    }
}

/// Palette qui demande le nom du groupe où mettre les éléments choisis
pub(crate) fn demander_groupe(f: &mut Fenetre) {
    let chemins = if f.app.sans_fichiers() { Vec::new() } else { f.app.chemins_choisis() };
    if chemins.is_empty() {
        f.app.annoncer("CHOISISSEZ D'ABORD DES ELEMENTS", true);
        return;
    }
    f.palette = Some(Palette::groupe(chemins));
    f.remplir_palette();
}

/// Compare les deux panneaux (double panneau) : chaque élément porte son écart, ceux qui manquent
/// de l'autre côté ou y sont plus anciens sont choisis. `copier` : on les y copie aussitôt.
pub(crate) unsafe fn comparer_panneaux(hwnd: HWND, f: &mut Fenetre, copier: bool) {
    let Some(autre) = f.second.as_mut() else {
        f.app.annoncer("OUVREZ D'ABORD LE DOUBLE PANNEAU (F6)", true);
        return;
    };
    if f.app.dossier().is_none() || autre.dossier().is_none() {
        f.app.annoncer("LES DEUX PANNEAUX DOIVENT MONTRER UN DOSSIER", true);
        return;
    }
    let ici = f.app.comparer_avec(autre);
    let la = autre.comparer_avec(&f.app);
    let compte = |m: &std::collections::HashMap<String, app::Ecart>, e: app::Ecart| m.values().filter(|&&x| x == e).count();
    let (seuls_ici, seuls_la) = (compte(&ici, app::Ecart::SeulIci), compte(&la, app::Ecart::SeulIci));
    let (recents_ici, recents_la) = (compte(&ici, app::Ecart::PlusRecent), compte(&la, app::Ecart::PlusRecent));
    let tailles = compte(&ici, app::Ecart::Taille);
    autre.comparaison = Some(la);
    f.app.comparaison = Some(ici);
    let n = f.app.choisir_ecarts();
    if seuls_ici + seuls_la + recents_ici + recents_la + tailles == 0 {
        f.app.annoncer("DOSSIERS IDENTIQUES", false);
    } else if copier && n > 0 {
        f.vers_autre(hwnd, false);
    } else {
        f.app.annoncer(
            format!("ICI : {seuls_ici} EN PLUS, {recents_ici} PLUS RECENTS · EN FACE : {seuls_la} EN PLUS, {recents_la} PLUS RECENTS{}", if tailles > 0 { format!(" · {tailles} TAILLES DIFFERENTES") } else { String::new() }),
            false,
        );
    }
    let _ = InvalidateRect(hwnd, None, false);
}

/// Copie dans le presse-papiers les écarts des deux panneaux, en Markdown ou en CSV (P-06). Ne change ni la sélection ni les
/// fichiers : la comparaison est recalculée ici, sans toucher aux écarts affichés.
pub(crate) unsafe fn exporter_ecarts(hwnd: HWND, f: &mut Fenetre, csv: bool) {
    let Some(autre) = f.second.as_ref() else {
        f.app.annoncer("OUVREZ D'ABORD LE DOUBLE PANNEAU (F6)", true);
        return;
    };
    let (Some(d_ici), Some(d_la)) = (f.app.dossier(), autre.dossier()) else {
        f.app.annoncer("LES DEUX PANNEAUX DOIVENT MONTRER UN DOSSIER", true);
        return;
    };
    let ici = f.app.comparer_avec(autre);
    let la = autre.comparer_avec(&f.app);
    let lister = |a: &App, m: &std::collections::HashMap<String, app::Ecart>| -> Vec<(String, &'static str)> {
        let mut v: Vec<(String, &'static str)> =
            a.tout.iter().filter_map(|e| m.get(&e.nom.to_lowercase()).filter(|x| **x != app::Ecart::Pareil).map(|x| (e.nom.clone(), x.libelle()))).collect();
        v.sort_by_key(|(n, _)| n.to_lowercase());
        v
    };
    // Côté « en face » : seulement ce qui n'existe pas ici (le reste est déjà dit du côté « ici », vu de l'autre sens)
    let en_face: Vec<(String, &'static str)> = lister(autre, &la).into_iter().filter(|(_, e)| *e == app::Ecart::SeulIci.libelle()).map(|(n, _)| (n, "SEULEMENT EN FACE")).collect();
    let sortie = if csv { comparaison::ecarts_csv } else { comparaison::ecarts_markdown };
    let texte = sortie(d_ici, d_la, &lister(&f.app, &ici), &en_face);
    operations::copier_texte(hwnd, &texte);
    f.app.annoncer(format!("ECARTS COPIES EN {} : {} ICI, {} EN FACE", if csv { "CSV" } else { "MARKDOWN" }, lister(&f.app, &ici).len(), en_face.len()), false);
}

/// Rouvre les onglets d'une session nommée, après ceux qui sont ouverts
pub(crate) unsafe fn ouvrir_session(hwnd: HWND, f: &mut Fenetre, nom: &str) {
    let Some(ligne) = reglages::actuels().sessions.into_iter().find(|x| x.split('|').next() == Some(nom)) else { return };
    let emps: Vec<Emplacement> = reglages::onglets_de_session(&ligne).into_iter().filter_map(depuis_session).collect();
    if emps.is_empty() {
        f.app.annoncer(format!("SESSION « {nom} » : PLUS AUCUN DOSSIER N'EXISTE"), true);
        return;
    }
    let n = emps.len();
    let debut = f.nb_onglets();
    for e in emps {
        f.nouvel_onglet(hwnd, e);
    }
    // L'onglet qui était actif à l'enregistrement redevient l'onglet actif (seulement s'il fait partie de ceux rouverts)
    if let Some(a) = reglages::champ_de_session(&ligne, "actif").and_then(|v| v.parse::<usize>().ok()).filter(|&a| a < n) {
        f.aller_onglet(debut + a);
    }
    // Dossier du second panneau : rouvert s'il existe encore et si le double panneau n'est pas déjà ouvert
    let face = reglages::champ_de_session(&ligne, "face").and_then(depuis_session);
    let avec_face = face.is_some() && f.second.is_none();
    if let (Some(e), true) = (face, avec_face) {
        f.double_panneau_sur(hwnd, e);
    }
    f.app.annoncer(format!("SESSION « {nom} » · {n} ONGLET{} OUVERT{}{}", if n > 1 { "S" } else { "" }, if n > 1 { "S" } else { "" }, if avec_face { " · DOUBLE PANNEAU" } else { "" }), false);
    let _ = InvalidateRect(hwnd, None, false);
}

/// Change les colonnes montrées : pour le dossier ouvert s'il en est un (V8), sinon pour tous
pub(crate) fn changer_colonnes(f: &Fenetre, nouvelles: impl Fn(u32) -> u32) {
    let (actuelles, lignes) = (reglages::COLONNES.load(Ordering::Relaxed), reglages::TAILLE_LIGNES.load(Ordering::Relaxed));
    match f.app.dossier() {
        Some(d) => reglages::memoriser_disposition(d, nouvelles(actuelles), lignes),
        None => reglages::modifier(|r| r.colonnes = nouvelles(r.colonnes)),
    }
}

/// F2 avec plusieurs éléments, ou la palette : renommage par lot
pub(crate) fn ouvrir_lot(f: &mut Fenetre) {
    match f.app.commencer_lot() {
        Some(l) => f.lot = Some(l),
        None => f.app.annoncer("CHOISISSEZ LES ELEMENTS A RENOMMER, DANS UN DOSSIER", true),
    }
}

/// Espace : coup d'œil sur l'élément choisi
pub(crate) unsafe fn ouvrir_galerie(hwnd: HWND, f: &mut Fenetre) {
    let Some(e) = f.app.selection.and_then(|i| f.app.entrees.get(i)).cloned() else { return };
    f.galerie_numero += 1;
    let numero = f.galerie_numero;
    let detail = match &e.genre {
        dossier::Genre::Fichier => format!("{} · {} · {}", icones::nom_type(&e), systeme::taille(e.taille), systeme::date(e.modif)),
        _ => format!("{} · {}", icones::nom_type(&e), systeme::date(e.modif)),
    };
    let chemin = f.app.chemin_de(&e);
    // Le drapeau du dossier ouvert ne couvre pas les résultats de recherche, les Récents ni les Groupes : tester le chemin lui-même
    let fichier = matches!(e.genre, dossier::Genre::Fichier)
        && e.nuage != dossier::Nuage::EnLigne
        && !f.app.non_fiable
        && !chemin.as_deref().is_some_and(|c| reglages::non_fiable(dossier::dossier_de(c)));
    let chemin = chemin.filter(|_| fichier);
    let mut g = Galerie::vierge(numero, e.nom.clone(), detail, icones::pour(&e), chemin.clone(), e.ext.clone());
    // HTML, SVG… : la visionneuse web (scripts et réseau coupés) montre la page ; sans runtime WebView2, l'aperçu texte habituel
    g.web = chemin.is_some() && crate::webvue::est_web(&e.ext) && crate::webvue::disponible();
    let web = g.web;
    // Sans runtime WebView2 : le dire une fois, sinon le source brut passe pour un défaut d'affichage (repli essayé le 14/11)
    if !web && chemin.is_some() && crate::webvue::est_web(&e.ext) {
        f.app.annoncer("WEBVIEW2 ABSENT · SOURCE DE LA PAGE AFFICHE", false);
    }
    // Office, RTF… : gestionnaire d'aperçu de Windows, seulement si l'auteur l'a activé (P-01) ; l'aperçu habituel sert de repli
    g.systeme = !web && chemin.is_some() && reglages::actuels().apercu_systeme && crate::hote_apercu::disponible_pour(&e.ext);
    let systeme = g.systeme;
    f.galerie = Some(g);
    if let (Some(c), false) = (chemin, web || systeme) {
        apercu::charger_galerie(hwnd, numero, c, e.ext.clone(), 0, 1);
    }
}

/// Visionneuse web du coup d'œil : pose le contrôle WebView2 sur la zone d'image quand l'élément est une page web, le détruit
/// sinon. Appelé à chaque peinture ; si la création échoue (runtime absent ou cassé), l'aperçu texte prend le relais.
pub(crate) unsafe fn synchroniser_web(hwnd: HWND, f: &mut Fenetre) {
    synchroniser_systeme(hwnd, f);
    let Some(g) = f.galerie.as_mut().filter(|g| g.web) else {
        crate::webvue::fermer(hwnd);
        return;
    };
    if crate::webvue::a_echoue() {
        g.web = false;
        let (numero, ext, chemin) = (g.numero, g.ext.clone(), g.chemin.clone());
        crate::webvue::fermer(hwnd);
        if let Some(c) = chemin {
            apercu::charger_galerie(hwnd, numero, c, ext, 0, 1);
        }
        return;
    }
    let Some(chemin) = g.chemin.clone() else { return };
    let (l, h) = crate::fenetre::taille_client(hwnd);
    let [x, y, w, hz] = crate::rendu::Rendu::zone_image_galerie(f.dip(l as i32), f.dip(h as i32));
    let k = f.dpi / 96.0;
    let rect = RECT { left: (x * k).round() as i32, top: (y * k).round() as i32, right: ((x + w) * k).round() as i32, bottom: ((y + hz) * k).round() as i32 };
    crate::webvue::synchroniser(hwnd, Some((chemin.as_str(), rect)));
}

/// Gestionnaire d'aperçu de Windows (`hote_apercu.rs`) : pose sa fenêtre sur la zone d'image quand l'élément s'y prête, la
/// décharge sinon ; s'il échoue ou dépasse son délai, l'aperçu habituel prend le relais.
unsafe fn synchroniser_systeme(hwnd: HWND, f: &mut Fenetre) {
    let Some(g) = f.galerie.as_mut().filter(|g| g.systeme) else {
        crate::hote_apercu::fermer();
        return;
    };
    if crate::hote_apercu::a_echoue() {
        g.systeme = false;
        let (numero, ext, chemin) = (g.numero, g.ext.clone(), g.chemin.clone());
        crate::hote_apercu::fermer();
        if let Some(c) = chemin {
            apercu::charger_galerie(hwnd, numero, c, ext, 0, 1);
        }
        return;
    }
    let (Some(chemin), ext) = (g.chemin.clone(), g.ext.clone()) else { return };
    let (l, h) = crate::fenetre::taille_client(hwnd);
    let [x, y, w, hz] = crate::rendu::Rendu::zone_image_galerie(f.dip(l as i32), f.dip(h as i32));
    let k = f.dpi / 96.0;
    let rect = RECT { left: (x * k).round() as i32, top: (y * k).round() as i32, right: ((x + w) * k).round() as i32, bottom: ((y + hz) * k).round() as i32 };
    // La fenêtre du gestionnaire (autre processus) passe au-dessus de notre dessin : on la cache sous la palette, un menu ou un toast
    let masque = f.palette.is_some() || f.menu.is_some() || f.app.menu_barre.is_some() || !crate::app::toasts_vivants().is_empty();
    crate::hote_apercu::synchroniser(hwnd, Some((chemin.as_str(), ext.as_str(), rect)), masque);
}

/// Coup d'œil : tourne les pages, `vers(page, pages)` donnant la nouvelle ; sans effet s'il n'y en a qu'une
pub(crate) unsafe fn page_galerie(hwnd: HWND, f: &mut Fenetre, vers: impl Fn(u32, u32) -> u32) {
    let Some(g) = f.galerie.as_mut() else { return };
    let (Some(chemin), true) = (g.chemin.clone(), g.pages > 1) else { return };
    let nouvelle = vers(g.page, g.pages).min(g.pages - 1);
    if nouvelle != g.page {
        // L'ancienne page reste affichée jusqu'à l'arrivée de la nouvelle : pas de clignotement
        g.page = nouvelle;
        apercu::charger_galerie(hwnd, g.numero, chemin, g.ext.clone(), nouvelle, g.echelle_demandee);
    }
}

/// Zone de l'image du coup d'œil en DIP (largeur, hauteur) et son centre (x, y) : ce que le zoom et le déplacement mesurent
unsafe fn zone_galerie(hwnd: HWND, f: &Fenetre) -> ((f32, f32), (f32, f32)) {
    let (l, h) = crate::fenetre::taille_client(hwnd);
    let [x, y, w, hz] = crate::rendu::Rendu::zone_image_galerie(f.dip(l as i32), f.dip(h as i32));
    ((w, hz), (x + w / 2.0, y + hz / 2.0))
}

/// Coup d'œil : zoom ×`facteur` autour du point `pointeur` (DIP de la fenêtre ; le centre de la zone si absent). Redemande l'image à
/// une plus grande échelle quand le zoom l'exige : le texte, le PDF et les images restent nets.
pub(crate) unsafe fn zoomer_galerie(hwnd: HWND, f: &mut Fenetre, facteur: f32, pointeur: Option<(f32, f32)>) {
    let (zone, centre) = zone_galerie(hwnd, f);
    let Some(g) = f.galerie.as_mut() else { return };
    let Some(img) = g.image.as_ref().map(|i| (i.largeur, i.hauteur)) else { return };
    let curseur = pointeur.map_or((0.0, 0.0), |(x, y)| (x - centre.0, y - centre.1));
    g.zoomer(facteur, curseur, img, zone);
    redemander_echelle(hwnd, g);
}

/// Coup d'œil : revient à l'ajustement (zoom 1, centré)
pub(crate) unsafe fn ajuster_galerie(hwnd: HWND, f: &mut Fenetre) {
    let Some(g) = f.galerie.as_mut() else { return };
    g.zoom = 1.0;
    g.decalage = (0.0, 0.0);
    redemander_echelle(hwnd, g);
}

/// Coup d'œil : déplace l'image zoomée de (dx, dy) DIP
pub(crate) unsafe fn deplacer_galerie(hwnd: HWND, f: &mut Fenetre, dx: f32, dy: f32) {
    let (zone, _) = zone_galerie(hwnd, f);
    let Some(g) = f.galerie.as_mut() else { return };
    let Some(img) = g.image.as_ref().map(|i| (i.largeur, i.hauteur)) else { return };
    g.decalage = (g.decalage.0 + dx, g.decalage.1 + dy);
    g.borner(img, zone);
}

/// Demande au fil du coup d'œil la même page à l'échelle que le zoom exige, si ce n'est pas déjà fait
unsafe fn redemander_echelle(hwnd: HWND, g: &mut Galerie) {
    let voulue = g.echelle_voulue();
    if voulue != g.echelle_demandee {
        g.echelle_demandee = voulue;
        if let Some(c) = g.chemin.clone() {
            apercu::charger_galerie(hwnd, g.numero, c, g.ext.clone(), g.page, voulue);
        }
    }
}
