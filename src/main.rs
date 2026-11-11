// SpaceFiles : fenêtre principale (Win32 natif, sans bordure, fond noir opaque)
#![windows_subsystem = "windows"]

mod apercu;
mod app;
mod assistant;
mod arbre;
mod accessibilite;
mod comparaison;
mod mtp;
mod ocr;
mod panneau;
mod rangement;
mod archive;
mod barre;
mod conflit;
mod coquille;
mod defaut;
mod dossier;
mod doublons;
mod eco;
mod etats;
mod edition;
mod glyph;
mod etiquettes;
mod groupes;
mod journal;
mod journal_historique;
mod historique;
mod icones;
mod images;
mod index;
mod lot;
mod menu;
mod mouvement;
mod mesure_defilement;
mod migration;
mod navigateur;
mod operations;
mod outils;
mod page;
mod palette;
mod pastilles;
mod pixel;
mod raccourci;
mod service_recherche;
mod reglages;
mod reflet;
mod rendu;
mod sftp;
mod sons;
mod stockages;
mod surveillance;
mod synchro;
mod corbeille;
mod glyphes;
mod telechargements;
mod systeme;
mod theme;
mod commandes;
mod fenetre;
mod minuteries;

use std::ffi::c_void;
use std::sync::atomic::Ordering;

use windows::core::{w, Result, HSTRING};
use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Dwm::*;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Ole::{OleInitialize, RegisterDragDrop, RevokeDragDrop};
use windows::Win32::UI::Controls::MARGINS;
use windows::Win32::UI::HiDpi::*;
use windows::Win32::UI::Input::KeyboardAndMouse::*;
use windows::Win32::UI::Shell::ShellExecuteW;
use windows::Win32::UI::WindowsAndMessaging::*;

use apercu::{Apercu, Galerie, Image, Vignettes, WM_APERCU, WM_GALERIE, WM_VIGNETTE};
use app::{Affichage, App, Cible, Fini, Lecture, MessageFond, WM_FOND, WM_GIT, WM_LECTURE, WM_OUTIL, WM_PALETTE_TACHE};
use dossier::{Emplacement, Entree};
use edition::Edition;
use lot::Lot;
use outils::{Balise, Git};
use palette::{Commande, Palette};
use operations::{Bilan, WM_OPERATION_FINIE, WM_PROGRES};
use page::{Element, Formulaire, Page};
use reglages::{Depart, Stockage};
use rendu::{boutons_nav, case_grille, grille_x0, dedans, zone_essentiel_haut, zone_recherche_haut, zone_reglages_haut, zone_terminal, zones_onglets, zones_volet, Rendu, Vue, VuePage};
use stockages::Lieu;
use surveillance::WM_CHANGEMENT;
use theme::*;
use commandes::*;
use fenetre::*;
use minuteries::*;
use menu::Choix;

fn main() -> Result<()> {
    // Avant tout accès aux réglages et au journal : le dossier SpaceFiles doit déjà contenir les anciennes données
    migration::migrer_profil();
    journal::installer();
    // Résident Win+E : pas de fenêtre, juste le raccourci
    if std::env::args().nth(1).as_deref() == Some("--resident") {
        raccourci::resident();
        return Ok(());
    }
    let Some(depart) = depart() else { return Ok(()) };
    // Les dossiers connus du Shell (favoris du volet) se lisent pendant que la fenêtre se crée : ~90 ms gagnés avant l'affichage
    systeme::precharger_favoris();
    // Une ligne par lancement : sans elle, un plantage avant la fenêtre ne dit pas quelle commande l'a provoqué (la sorte d'argument seulement : un chemin demandé par une autre application n'a rien à faire dans le journal)
    journal::ecrire(journal::Niveau::Info, "main", &format!("demarrage version {} argument : {}", env!("CARGO_PKG_VERSION"), sorte_d_arguments(&std::env::args().skip(1).collect::<Vec<_>>())));
    std::thread::spawn(defaut::actualiser);
    let r = reglages::actuels();
    // Onglets : ceux de la dernière fois (si demandé et ouvert sans dossier précis), sinon un seul
    let mut onglets: Vec<Emplacement> = Vec::new();
    // Marqueur laissé par une fenêtre qui n'a pas fermé proprement : les onglets reviennent même si le réglage est faux
    let restauration = faut_restaurer(r.restaurer_onglets, arret_precedent_anormal(), depart.is_some());
    let mut actif = 0;
    if restauration != Restauration::Rien {
        let rouverts: Vec<Option<Emplacement>> = r.onglets.iter().map(|s| depuis_session(s)).collect();
        actif = rang_apres_filtre(&rouverts.iter().map(Option::is_some).collect::<Vec<_>>(), r.onglet_actif);
        onglets = rouverts.into_iter().flatten().collect();
    }
    let apres_plantage = restauration == Restauration::ApresPlantage && !onglets.is_empty();
    if apres_plantage {
        journal::ecrire(journal::Niveau::Avert, "session", &format!("session restauree apres une fermeture anormale ({} onglets)", onglets.len()));
    }
    let actif = if onglets.is_empty() { 0 } else { actif.min(onglets.len() - 1) };
    if onglets.is_empty() {
        onglets.push(depart.unwrap_or_else(|| depart_reglage(&r)));
    }
    let affiche = onglets.remove(actif);

    unsafe {
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        // OLE : glisser-déposer, presse-papiers et menus du Shell
        OleInitialize(None)?;
        // Thème effectif calculé une fois ici (réglages déjà lus plus haut), avant les menus et la fenêtre
        theme::recalculer();
        coquille::menus_theme(theme::clair());
        let instance: HINSTANCE = GetModuleHandleW(None)?.into();

        let classe = WNDCLASSW {
            style: CS_HREDRAW | CS_VREDRAW | CS_DBLCLKS,
            lpfnWndProc: Some(procedure),
            hInstance: instance,
            hCursor: LoadCursorW(None, IDC_ARROW)?,
            hIcon: icone(GetSystemMetrics(SM_CXICON)),
            lpszClassName: w!("SpaceFiles"),
            ..Default::default()
        };
        RegisterClassW(&classe);

        let mut premier = App::sans_favoris(affiche);
        if apres_plantage {
            premier.annoncer("SESSION RESTAUREE APRES UNE FERMETURE ANORMALE", false);
        }
        if let Some((_, Some(nom))) = std::env::args().nth(1).and_then(|a| dossier::emplacement_depuis_url(a.trim().trim_matches('"'))) {
            premier.a_reselectionner = vec![nom];
        }
        // `--montrer` / `--ouvrir` : l'élément à choisir dans le dossier ouvert (résolu une seule fois, voir `ouverture_demandee`)
        if let Some((_, Some(nom))) = ouverture_demandee() {
            premier.a_reselectionner = vec![nom];
        }
        let etat = Box::new(Fenetre {
            rendu: Rendu::nouveau()?,
            app: premier,
            fond: onglets.into_iter().map(App::sans_favoris).collect(),
            rang: actif,
            dpi: 96.0,
            suivi_souris: false,
            anime: false,
            apercu_visible: r.apercu,
            apercu: None,
            apercu_demande: None,
            apercu_numero: 0,
            apercu_attente: None,
            terminal_visible: false,
            terminal_lignes: Vec::new(),
            par_defaut: defaut::est_actif(),
            page: Page::nouvelle(),
            detectes: Vec::new(),
            etats: Default::default(),
            etats_en_cours: false,
            etats_mesure: None,
            win_e: defaut::win_e_actif(),
            second: None,
            focus_droite: false,
            focus_clavier: false,
            palette: None,
            historique: false,
            historique_defilement: 0.0,
            lot: None,
            galerie: None,
            galerie_numero: 0,
            avaler_caractere: false,
            menu: None,
            avaler_clic: false,
            applications: Vec::new(),
            depot_droit: None,
            droit_depart: None,
            vignettes: None,
            annonce_armee: false,
            fermes: Vec::new(),
            session_vue: None,
            session_armee: false,
            active: false,
            reduite: false,
            clignement_arme: false,
            regard_voulu: (0.0, 0.0),
            image_yeux: 0,
            pixel_dans_palette: false,
            relais_depuis: None,
            plisse_depuis: None,
            pastille_fixee: None,
            pastille_masquee: false,
            activite_armee: false,
            pointeur: None,
            survol_appui: false,
            mesure_reflet_faite: false,
            reflet_actif: true,
            ouverture_attend: reglages::animations_actives(),
            ouverture: None,
            fermeture: None,
            mesure_ouverture: None,
            cadence: mouvement::Cadence::nouveau(),
            metronome: None,
        });

        let hwnd = CreateWindowExW(
            WINDOW_EX_STYLE::default(),
            w!("SpaceFiles"),
            w!("SpaceFiles"),
            // Sans WS_SYSMENU : DWM ne dessine pas ses boutons par-dessus les nôtres,
            // tout en gardant Snap, l'ombre et les animations de réduction
            WS_SYSMENU | WS_THICKFRAME | WS_MINIMIZEBOX | WS_MAXIMIZEBOX,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            1200,
            760,
            None,
            None,
            instance,
            Some(Box::into_raw(etat) as *const c_void),
        )?;
        // Fenêtre principale ouverte : jusqu'à sa fermeture normale, un redémarrage la considère comme interrompue
        poser_marqueur();
        let petite = icone(GetSystemMetrics(SM_CXSMICON));
        SendMessageW(hwnd, WM_SETICON, WPARAM(ICON_SMALL as usize), LPARAM(petite.0 as isize));
        // Grande icône nette pour la barre des tâches et Alt+Tab (fond transparent)
        let grande = icone(GetSystemMetrics(SM_CXICON) * 2);
        SendMessageW(hwnd, WM_SETICON, WPARAM(ICON_BIG as usize), LPARAM(grande.0 as isize));

        // La lecture du premier dossier démarre pendant que la fenêtre s'affiche
        if let Some(f) = etat_de(hwnd) {
            f.rafraichir_lieux();
            for a in f.tous() {
                a.charger(hwnd);
            }
            f.app.charger_disques(hwnd);
            // Mode essai (`SPACEFILES_PALETTE_OUVERTE=1`, avec `SPACEFILES_SANS_REGISTRE=1`) : palette ouverte au lancement, pour les captures.
            // Sans `index::demarrer` (qu'`ouvrir_palette` lance) : le parcours du disque fausserait la mesure du processeur au repos.
            if palette::ouverte_par_essai() {
                f.palette = Some(Palette::nouvelle());
                f.remplir_palette();
            }
        }
        let depot = coquille::Depot::nouveau(
            hwnd,
            coquille::Rappels { destination: depot_destination, survol: depot_survol, deposer: depot_deposer, deposer_droit: depot_menu },
        );
        let _ = RegisterDragDrop(hwnd, &depot);

        // Taille initiale adaptée à l'échelle de l'écran
        let dpi = dpi_de(hwnd) as i32;
        // Jamais plus grande que la zone de travail : à 200 % de DPI sur un petit écran, 1200 × 760 DIP déborderaient
        let mut zone = RECT::default();
        let _ = SystemParametersInfoW(SPI_GETWORKAREA, 0, Some(&mut zone as *mut RECT as *mut _), SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0));
        let (l, h) = (1440 * dpi / 96, 900 * dpi / 96);
        let (l, h) = if zone.right > zone.left { (l.min((zone.right - zone.left) * 9 / 10), h.min((zone.bottom - zone.top) * 9 / 10)) } else { (l, h) };
        let _ = SetWindowPos(hwnd, None, 0, 0, l, h, SWP_NOMOVE | SWP_NOZORDER);

        // Cadre DWM : coins arrondis et ombre native, client noir opaque (plus d'acrylique)
        let coins = DWMWCP_ROUND;
        let _ = DwmSetWindowAttribute(hwnd, DWMWA_WINDOW_CORNER_PREFERENCE, &coins as *const _ as *const c_void, 4);
        let fond = DWMSBT_NONE;
        let _ = DwmSetWindowAttribute(hwnd, DWMWA_SYSTEMBACKDROP_TYPE, &fond as *const _ as *const c_void, 4);
        // Mode sombre immersif, couleurs de la barre et de la bordure : selon le thème (changent à chaud, voir `rafraichir_apparence`)
        appliquer_dwm(hwnd, theme::clair());
        theme::a_appliquer();
        let _ = DwmExtendFrameIntoClientArea(hwnd, &MARGINS { cxLeftWidth: -1, cxRightWidth: -1, cyTopHeight: -1, cyBottomHeight: -1 });
        // Le cadre DWM tient au style posé à la création ; une fois l'état DWM établi, on retire SYSMENU pour que
        // Windows ne dessine plus ses boutons (le mod nothing-boutons y superpose des points fantômes)
        let style = GetWindowLongPtrW(hwnd, GWL_STYLE);
        SetWindowLongPtrW(hwnd, GWL_STYLE, style & !(WS_SYSMENU.0 as isize));
        let _ = SetWindowPos(hwnd, None, 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE | SWP_FRAMECHANGED);

        // Favoris du volet : la lecture lancée au début de `main` est finie (ou on l'attend ici), avant le premier dessin
        if let Some(f) = etat_de(hwnd) {
            for a in f.tous() {
                a.charger_favoris();
            }
        }
        let _ = ShowWindow(hwnd, SW_SHOW);
        // Ouverture animée (lot 12A) : la minuterie d'animation porte les 560 ms et s'arrête d'elle-même ; coupée en mouvement réduit
        if let Some(f) = etat_de(hwnd) {
            if f.ouverture_attend {
                animer(hwnd, f);
            }
        }
        // Téléchargements du navigateur : un seul abonnement au dossier Téléchargements pour tout le processus (si le réglage le veut)
        navigateur::regler(hwnd);

        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).into() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
        Ok(())
    }
}
