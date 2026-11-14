//! Visionneuse web du coup d'œil (Espace) pour les fichiers HTML, XHTML, MHTML et SVG : un contrôle WebView2 enfant de la
//! fenêtre principale, posé sur la zone d'image de la carte. Scripts, réseau, menus, outils de développement et touches
//! du navigateur sont coupés : la page s'affiche comme un document, rien ne s'exécute. Le contrôle naît à l'ouverture de
//! l'aperçu et est détruit à sa fermeture (les processus WebView2 s'arrêtent avec lui : pas de mémoire gardée en résident).
//! Sans le runtime WebView2 (livré avec Windows 11), `disponible()` rend faux et le coup d'œil retombe sur l'aperçu texte.
//!
//! Tout se passe sur le fil d'interface : `synchroniser` est appelé à chaque peinture de la fenêtre avec ce qu'il faut
//! montrer (chemin, rectangle en pixels), et compare avec ce qui est déjà affiché. La création du contrôle est asynchrone
//! (callbacks sur la boucle de messages) : elle ne bloque jamais la fenêtre.

use std::cell::RefCell;
use std::sync::OnceLock;

use webview2_com::Microsoft::Web::WebView2::Win32::*;
use webview2_com::{
    take_pwstr, AcceleratorKeyPressedEventHandler, CoreWebView2EnvironmentOptions, CreateCoreWebView2ControllerCompletedHandler,
    CreateCoreWebView2EnvironmentCompletedHandler, DownloadStartingEventHandler, NavigationStartingEventHandler, NewWindowRequestedEventHandler,
    WebResourceRequestedEventHandler,
};
use windows::core::{Interface, HSTRING, PCWSTR, PWSTR};
use windows::Win32::Foundation::{HWND, LPARAM, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::InvalidateRect;
use windows::Win32::System::WinRT::EventRegistrationToken;
use windows::Win32::UI::Input::KeyboardAndMouse::{GetFocus, SetFocus};
use windows::Win32::UI::WindowsAndMessaging::{IsChild, PostMessageW, WM_KEYDOWN};

/// Extensions montrées par la visionneuse web (sans point, en minuscules)
pub fn est_web(ext: &str) -> bool {
    // `Entree::ext` est en majuscules : comparer sans tenir compte de la casse
    ["html", "htm", "xhtml", "mhtml", "mht", "svg"].iter().any(|e| ext.eq_ignore_ascii_case(e))
}

/// Le runtime WebView2 est installé (test fait une fois : une lecture de registre, sans rien démarrer)
pub fn disponible() -> bool {
    static OUI: OnceLock<bool> = OnceLock::new();
    *OUI.get_or_init(|| unsafe {
        let mut version = PWSTR::null();
        let r = GetAvailableCoreWebView2BrowserVersionString(PCWSTR::null(), &mut version);
        let ok = r.is_ok() && !version.is_null();
        crate::journal::ecrire(crate::journal::Niveau::Info, "webvue", &format!("runtime disponible : {ok} ({r:?})"));
        if ok {
            let _ = take_pwstr(version);
        }
        ok
    })
}

enum Etat {
    Repos,
    Creation,
    Pret,
    Echec,
}

struct Vue {
    etat: Etat,
    controleur: Option<ICoreWebView2Controller>,
    webview: Option<ICoreWebView2>,
    /// Ce qui doit être montré : chemin et rectangle en pixels dans la zone cliente de la fenêtre parente
    cible: Option<(String, RECT)>,
    /// Chemin chargé dans le contrôle et rectangle posé (pour ne rien refaire quand rien ne change)
    affiche: Option<String>,
    pose: Option<RECT>,
}

thread_local! {
    static VUE: RefCell<Vue> = const { RefCell::new(Vue { etat: Etat::Repos, controleur: None, webview: None, cible: None, affiche: None, pose: None }) };
}

/// La création du contrôle a échoué (runtime cassé, dossier de données illisible) : le coup d'œil doit se rabattre sur le texte
pub fn a_echoue() -> bool {
    VUE.with(|v| matches!(v.borrow().etat, Etat::Echec))
}

/// `file:///C:/dossier/page%20x.html` à partir d'un chemin Windows (UNC : `file://serveur/partage/…`)
fn url_fichier(chemin: &str) -> String {
    // Préfixes de chemin long : `\\?\C:\…` et `\\?\UNC\serveur\…`
    let normal;
    let chemin = if let Some(unc) = chemin.strip_prefix(r"\\?\UNC\") {
        normal = format!(r"\\{unc}");
        normal.as_str()
    } else {
        chemin.strip_prefix(r"\\?\").unwrap_or(chemin)
    };
    let (hote, reste) = match chemin.strip_prefix("\\\\") {
        Some(unc) => ("//", unc),
        None => ("///", chemin),
    };
    let mut url = format!("file:{hote}");
    for o in reste.replace('\\', "/").bytes() {
        match o {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' | b'/' | b':' => url.push(o as char),
            _ => url.push_str(&format!("%{o:02X}")),
        }
    }
    url
}

/// Montre `cible` (chemin, rectangle en pixels) dans le contrôle, ou le détruit si `cible` est vide
pub unsafe fn synchroniser(parent: HWND, cible: Option<(&str, RECT)>) {
    let Some((chemin, rect)) = cible else {
        fermer(parent);
        return;
    };
    let demarrer = VUE.with(|v| {
        let mut v = v.borrow_mut();
        v.cible = Some((chemin.to_string(), rect));
        if matches!(v.etat, Etat::Repos) {
            v.etat = Etat::Creation;
            return true;
        }
        if matches!(v.etat, Etat::Pret) {
            appliquer(&mut v);
        }
        false
    });
    if demarrer {
        if let Err(e) = creer(parent) {
            crate::journal::ecrire(crate::journal::Niveau::Avert, "webvue", &format!("création : {e}"));
            VUE.with(|v| v.borrow_mut().etat = Etat::Echec);
        }
    }
}

/// Détruit le contrôle et rend le clavier à la fenêtre : les processus WebView2 s'arrêtent
pub unsafe fn fermer(parent: HWND) {
    let controleur = VUE.with(|v| {
        let mut v = v.borrow_mut();
        let rien = matches!(v.etat, Etat::Repos) && v.controleur.is_none();
        let c = v.controleur.take();
        *v = Vue { etat: Etat::Repos, controleur: None, webview: None, cible: None, affiche: None, pose: None };
        if rien { None } else { Some(c) }
    });
    if let Some(c) = controleur {
        if let Some(c) = c {
            let _ = c.Close();
        }
        // Le focus revient à la fenêtre seulement s'il y était (le contrôle ou la fenêtre) : sinon on activerait une fenêtre inactive
        let f = GetFocus();
        if f.0.is_null() || f == parent || IsChild(parent, f).as_bool() {
            let _ = SetFocus(parent);
        }
    }
}

/// Fait défiler la page par une commande de l'hôte (les scripts de la page, eux, restent coupés) ; `vk` : ↑ ↓ PgUp PgDn Début Fin
pub unsafe fn defiler(vk: u16) {
    let script = match vk {
        0x26 => "window.scrollBy(0,-60)",
        0x28 => "window.scrollBy(0,60)",
        0x21 => "window.scrollBy(0,-0.9*window.innerHeight)",
        0x22 => "window.scrollBy(0,0.9*window.innerHeight)",
        0x24 => "window.scrollTo(0,0)",
        0x23 => "window.scrollTo(0,document.documentElement.scrollHeight)",
        _ => return,
    };
    let w = VUE.with(|v| v.borrow().webview.clone());
    if let Some(w) = w {
        let _ = w.ExecuteScript(&HSTRING::from(script), None);
    }
}

/// Pose le rectangle, rend le contrôle visible et charge le fichier s'il a changé
unsafe fn appliquer(v: &mut Vue) {
    let (Some(c), Some(w), Some((chemin, rect))) = (v.controleur.as_ref(), v.webview.as_ref(), v.cible.as_ref()) else { return };
    let r = *rect;
    if v.pose.is_none_or(|p| p.left != r.left || p.top != r.top || p.right != r.right || p.bottom != r.bottom) {
        let _ = c.SetBounds(r);
        v.pose = Some(r);
    }
    let _ = c.SetIsVisible(true);
    if v.affiche.as_deref() != Some(chemin) {
        v.affiche = Some(chemin.clone());
        let _ = w.Navigate(&HSTRING::from(url_fichier(chemin)));
        // Le clavier reste à la fenêtre (Espace ferme, ← → changent d'élément) : un contrôle qui prend le focus garde Espace pour
        // lui, et `AcceleratorKeyPressed` ne le voit pas (constaté). Le défilement passe par `defiler`.
    }
}

/// Environnement puis contrôleur, chacun par callback ; à la fin, `appliquer` montre ce que la peinture a demandé entre-temps
unsafe fn creer(parent: HWND) -> windows::core::Result<()> {
    let dossier = crate::reglages::dossier_donnees(true).map(|d| d.join("webview2")).unwrap_or_else(|| std::env::temp_dir().join("SpaceFiles-webview2"));
    let _ = std::fs::create_dir_all(&dossier);
    let options = CoreWebView2EnvironmentOptions::default();
    // Aucune résolution de nom : une page ne peut charger ni image, ni feuille de style, ni police distante (les fichiers
    // locaux et les partages réseau Windows, eux, ne passent pas par là). Pas de comptes, de synchronisation ni d'envois.
    options.set_additional_browser_arguments("--host-resolver-rules=\"MAP * ~NOTFOUND\" --disable-background-networking --disable-sync --no-pings".to_string());
    let options: ICoreWebView2EnvironmentOptions = options.into();
    let h = parent.0 as isize;
    let rappel = CreateCoreWebView2EnvironmentCompletedHandler::create(Box::new(move |erreur, env| {
        let parent = HWND(h as *mut _);
        let env = match (erreur, env) {
            (Ok(()), Some(e)) => e,
            (Err(e), _) => return echouer(parent, format!("environnement : {e}")),
            _ => return echouer(parent, "environnement : aucun rendu".to_string()),
        };
        let env_suite = env.clone();
        let suite = CreateCoreWebView2ControllerCompletedHandler::create(Box::new(move |erreur, controleur| {
            let parent = HWND(h as *mut _);
            match (erreur, controleur) {
                (Ok(()), Some(c)) => {
                    if let Err(e) = configurer(parent, &c, &env_suite) {
                        let _ = c.Close();
                        return echouer(parent, format!("réglages : {e}"));
                    }
                    VUE.with(|v| {
                        let mut v = v.borrow_mut();
                        if matches!(v.etat, Etat::Creation) {
                            v.webview = c.CoreWebView2().ok();
                            v.controleur = Some(c);
                            v.etat = Etat::Pret;
                            appliquer(&mut v);
                        } else {
                            // L'aperçu s'est fermé pendant la création
                            let _ = c.Close();
                        }
                    });
                    let _ = InvalidateRect(parent, None, false);
                    Ok(())
                }
                (Err(e), _) => echouer(parent, format!("contrôleur : {e}")),
                _ => echouer(parent, "contrôleur : aucun rendu".to_string()),
            }
        }));
        match env.CreateCoreWebView2Controller(parent, &suite) {
            Ok(()) => Ok(()),
            Err(e) => echouer(parent, format!("contrôleur (appel) : {e}")),
        }
    }));
    CreateCoreWebView2EnvironmentWithOptions(PCWSTR::null(), &HSTRING::from(dossier.as_os_str()), &options, &rappel)
}

/// Échec de création : l'état le dit, la fenêtre se repeint et le coup d'œil se rabat sur le texte
unsafe fn echouer(parent: HWND, raison: String) -> windows::core::Result<()> {
    crate::journal::ecrire(crate::journal::Niveau::Avert, "webvue", &raison);
    VUE.with(|v| v.borrow_mut().etat = Etat::Echec);
    let _ = InvalidateRect(parent, None, false);
    Ok(())
}

/// Réglages de sécurité du contrôle et transmission des touches d'aperçu à la fenêtre principale
unsafe fn configurer(parent: HWND, c: &ICoreWebView2Controller, env: &ICoreWebView2Environment) -> windows::core::Result<()> {
    let w = c.CoreWebView2()?;
    let s = w.Settings()?;
    s.SetIsScriptEnabled(false)?;
    s.SetAreDefaultScriptDialogsEnabled(false)?;
    s.SetAreDevToolsEnabled(false)?;
    s.SetAreDefaultContextMenusEnabled(false)?;
    s.SetIsStatusBarEnabled(false)?;
    s.SetIsWebMessageEnabled(false)?;
    if let Ok(s3) = s.cast::<ICoreWebView2Settings3>() {
        // Ni Ctrl+P, ni Ctrl+S, ni F5, ni F12 : le contrôle n'est pas un navigateur
        let _ = s3.SetAreBrowserAcceleratorKeysEnabled(false);
    }
    // Navigation : seuls les fichiers locaux ; un lien http(s) ou autre est refusé
    let mut jeton = EventRegistrationToken::default();
    let nav = NavigationStartingEventHandler::create(Box::new(|_, args| {
        if let Some(args) = args {
            let mut uri = PWSTR::null();
            if args.Uri(&mut uri).is_ok() {
                let uri = take_pwstr(uri).to_ascii_lowercase();
                if !(uri.starts_with("file:") || uri.starts_with("about:blank")) {
                    args.SetCancel(true)?;
                }
            }
        }
        Ok(())
    }));
    w.add_NavigationStarting(&nav, &mut jeton)?;
    // Ni nouvelle fenêtre (un lien target="_blank" échapperait au filtre ci-dessus), ni téléchargement
    let neuve = NewWindowRequestedEventHandler::create(Box::new(|_, args| {
        if let Some(args) = args {
            args.SetHandled(true)?;
        }
        Ok(())
    }));
    w.add_NewWindowRequested(&neuve, &mut jeton)?;
    if let Ok(w4) = w.cast::<ICoreWebView2_4>() {
        let tele = DownloadStartingEventHandler::create(Box::new(|_, args| {
            if let Some(args) = args {
                args.SetCancel(true)?;
            }
            Ok(())
        }));
        w4.add_DownloadStarting(&tele, &mut jeton)?;
    }
    // Ressources (images, feuilles de style…) : seuls les fichiers locaux. Un `file://hote/partage/x.png` ouvrirait un chemin
    // réseau Windows et enverrait une authentification NTLM à un hôte inconnu, que la résolution de noms coupée ne couvre pas.
    w.AddWebResourceRequestedFilter(&HSTRING::from("*"), COREWEBVIEW2_WEB_RESOURCE_CONTEXT_ALL)?;
    let env = env.clone();
    let filtre = WebResourceRequestedEventHandler::create(Box::new(move |_, args| {
        if let Some(args) = args {
            let mut uri = PWSTR::null();
            args.Request()?.Uri(&mut uri)?;
            let uri = take_pwstr(uri).to_ascii_lowercase();
            // « file:/// » (chemin local) passe ; « file://hote/ » (partage réseau), http(s), ftp… sont refusés
            if !(uri.starts_with("file:///") || uri.starts_with("data:") || uri.starts_with("about:") || uri.starts_with("blob:")) {
                let refus = env.CreateWebResourceResponse(None::<&windows::Win32::System::Com::IStream>, 403, &HSTRING::from("Refusé"), &HSTRING::new())?;
                args.SetResponse(&refus)?;
            }
        }
        Ok(())
    }));
    w.add_WebResourceRequested(&filtre, &mut jeton)?;
    // Quand le contrôle a le clavier, Échap, Espace, Entrée et ← → reviennent à la fenêtre (fermer, ouvrir, élément voisin)
    let h = parent.0 as isize;
    let touches = AcceleratorKeyPressedEventHandler::create(Box::new(move |_, args| {
        if let Some(args) = args {
            let mut genre = COREWEBVIEW2_KEY_EVENT_KIND::default();
            let mut vk = 0u32;
            args.KeyEventKind(&mut genre)?;
            args.VirtualKey(&mut vk)?;
            // Une touche système (Alt+←) n'est pas la touche simple : on la laisse à la page
            let appui = genre == COREWEBVIEW2_KEY_EVENT_KIND_KEY_DOWN;
            if appui && matches!(vk, 0x1B | 0x20 | 0x0D | 0x25 | 0x27) {
                let _ = PostMessageW(HWND(h as *mut _), WM_KEYDOWN, WPARAM(vk as usize), LPARAM(1));
                args.SetHandled(true)?;
            }
        }
        Ok(())
    }));
    c.add_AcceleratorKeyPressed(&touches, &mut jeton)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn url_de_fichier_local() {
        assert_eq!(url_fichier(r"C:\Dossier\page x.html"), "file:///C:/Dossier/page%20x.html");
        assert_eq!(url_fichier(r"C:\été\a.htm"), "file:///C:/%C3%A9t%C3%A9/a.htm");
    }

    #[test]
    fn url_de_chemin_long() {
        assert_eq!(url_fichier(r"\\?\C:\a\b.html"), "file:///C:/a/b.html");
        assert_eq!(url_fichier(r"\\?\UNC\nas\docs\a.html"), "file://nas/docs/a.html");
    }

    #[test]
    fn url_de_partage_reseau() {
        assert_eq!(url_fichier(r"\\nas\docs\a#1.html"), "file://nas/docs/a%231.html");
    }

    #[test]
    fn extensions_web() {
        assert!(est_web("html") && est_web("svg") && est_web("mhtml"));
        assert!(est_web("HTML") && est_web("SVG"), "les extensions du programme sont en majuscules");
        assert!(!est_web("txt") && !est_web("js") && !est_web("css"));
    }
}
