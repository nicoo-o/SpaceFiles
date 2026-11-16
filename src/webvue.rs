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
        // Essai du repli sans WebView2 : SPACEFILES_SANS_WEBVIEW=1 le déclare absent (comme SPACEFILES_SANS_REGISTRE)
        if std::env::var_os("SPACEFILES_SANS_WEBVIEW").is_some() {
            return false;
        }
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

/// Vrai pour `file:///chemin` **local** : trois barres puis un chemin qui ne commence pas lui-même par une barre.
/// `file://hote/partage` (hôte), `file:////hote/partage` et `file:///\\hote\partage` (trois ou quatre barres, mais Windows y lit
/// le partage `\\hote\partage`) ouvriraient un chemin réseau et enverraient une authentification NTLM à un hôte inconnu :
/// refusés. Le contrôle écrit `\` comme `/` pour le schéma `file`, d'où la normalisation ; `%2F` et `%5C` en tête sont refusés
/// par prudence. Tout ce qui n'est pas reconnu est refusé (le doute profite au refus).
fn fichier_local(uri: &str) -> bool {
    let u = uri.replace('\\', "/").to_ascii_lowercase();
    match u.strip_prefix("file:///") {
        Some(reste) => !(reste.starts_with('/') || reste.starts_with("%2f") || reste.starts_with("%5c")),
        None => false,
    }
}

/// Navigation de la page : seulement un fichier local (un lien vers un autre fichier de l'ordinateur) ou `about:blank`
fn navigation_permise(uri: &str) -> bool {
    fichier_local(uri) || uri.eq_ignore_ascii_case("about:blank")
}

/// Ressource demandée par la page (image, feuille de style, cadre…) : un fichier local, ou un contenu qui ne sort pas du
/// document (`data:`, `blob:`, `about:`). http(s), ftp, `ws:`, `javascript:`, hôte réseau, tout le reste : refusé.
fn ressource_permise(uri: &str) -> bool {
    let u = uri.to_ascii_lowercase();
    fichier_local(uri) || u.starts_with("data:") || u.starts_with("blob:") || u.starts_with("about:")
}

/// Chemin d'un partage réseau (`\\serveur\partage\…`, `\\?\UNC\…`, `//serveur/…`) : jamais confié au contrôle, qui refuse
/// de toute façon les ressources réseau (le document lui-même serait refusé). Un lecteur réseau monté en `Z:` n'est pas
/// reconnu ici : son chemin est local pour Windows, et le partage est celui que l'auteur a lui-même choisi d'ouvrir.
pub fn chemin_reseau(chemin: &str) -> bool {
    let c = chemin.replace('/', "\\");
    if !c.starts_with("\\\\") {
        return false;
    }
    // `\\?\C:\…` et `\\.\C:\…` : chemins locaux au préfixe long ; `\\?\UNC\…` : réseau
    for prefixe in ["\\\\?\\", "\\\\.\\"] {
        if let Some(reste) = c.strip_prefix(prefixe) {
            let r = reste.as_bytes();
            return !(r.len() >= 2 && r[0].is_ascii_alphabetic() && r[1] == b':');
        }
    }
    true
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
            if args.Uri(&mut uri).is_ok() && !navigation_permise(&take_pwstr(uri)) {
                args.SetCancel(true)?;
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
            if !ressource_permise(&take_pwstr(uri)) {
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
    fn fichiers_locaux_permis() {
        for u in [
            "file:///C:/Dossier/page.html",
            "file:///c:/dossier/page%20x.html",
            "FILE:///C:/A/b.html",
            "file:///C:\\A\\b.html",
            "file:///D:/a#ancre",
            "file:///C:/%C3%A9t%C3%A9/a.htm",
        ] {
            assert!(navigation_permise(u) && ressource_permise(u), "doit passer : {u}");
        }
    }

    #[test]
    fn chemins_reseau_refuses_sous_toutes_les_ecritures() {
        for u in [
            "file://hote/partage/x.png",
            "file://HOTE/partage/x.png",
            "file:////hote/partage/x.png",
            "file://///hote/partage/x.png",
            "file:///\\\\hote\\partage\\x.png",
            "file:///\\hote\\partage",
            "file:///%5C%5Chote/x",
            "file:///%5chote/x",
            "file:///%2f%2fhote/x",
            "file:///%2Fhote/x",
            "file:\\\\hote\\partage",
            "file:/hote/x",
            "file://localhost/C:/x.png",
            "file://127.0.0.1/c$/x.png",
            "file:",
            "file://",
        ] {
            assert!(!navigation_permise(u), "navigation refusée : {u}");
            assert!(!ressource_permise(u), "ressource refusée : {u}");
        }
    }

    #[test]
    fn schemas_inattendus_refuses() {
        for u in [
            "http://exemple.fr/x.png",
            "HTTPS://exemple.fr/",
            "ftp://hote/x",
            "ws://hote/",
            "wss://hote/",
            "javascript:alert(1)",
            "ms-appx:///x",
            "ms-appx-web:///x",
            "edge://settings",
            "chrome://version",
            "view-source:file:///C:/a.html",
            "filesystem:file:///C:/a.html",
            "search-ms:query=a",
            "mailto:a@b.fr",
            "\\\\hote\\partage\\x.png",
            "C:\\a\\b.html",
            "//hote/x",
            "",
            " file:///C:/a.html",
        ] {
            assert!(!navigation_permise(u), "navigation refusée : {u:?}");
            assert!(!ressource_permise(u), "ressource refusée : {u:?}");
        }
    }

    #[test]
    fn contenu_du_document_seulement() {
        // La navigation n'accepte que fichier local et about:blank ; la page peut en plus charger data:, blob:, about:
        assert!(navigation_permise("about:blank") && navigation_permise("ABOUT:BLANK"));
        assert!(!navigation_permise("about:srcdoc") && !navigation_permise("data:text/html,x") && !navigation_permise("blob:null/1"));
        for u in ["data:image/png;base64,AAAA", "DATA:text/css,a{}", "blob:file:///C:/x", "about:blank", "about:srcdoc"] {
            assert!(ressource_permise(u), "ressource permise : {u}");
        }
    }

    #[test]
    fn urls_fabriquees_par_url_fichier() {
        // Ce que `appliquer` envoie au contrôle : local passe, réseau est refusé, même avec les préfixes longs
        assert!(navigation_permise(&url_fichier(r"C:\a\b.html")));
        assert!(navigation_permise(&url_fichier(r"\\?\C:\a\b.html")));
        assert!(!navigation_permise(&url_fichier(r"\\nas\docs\a.html")));
        assert!(!navigation_permise(&url_fichier(r"\\?\UNC\nas\docs\a.html")));
    }

    #[test]
    fn chemins_reseau_reconnus() {
        for c in [r"\\nas\docs\a.html", r"\\?\UNC\nas\docs\a.html", "//nas/docs/a.html", r"\\.\UNC\nas\x"] {
            assert!(chemin_reseau(c), "réseau : {c}");
        }
        for c in [r"C:\a\b.html", r"\\?\C:\a\b.html", r"\\.\C:\a", r"\\?\c:\a", "a.html", ""] {
            assert!(!chemin_reseau(c), "local : {c}");
        }
    }

    #[test]
    fn extensions_web() {
        assert!(est_web("html") && est_web("svg") && est_web("mhtml"));
        assert!(est_web("HTML") && est_web("SVG"), "les extensions du programme sont en majuscules");
        assert!(!est_web("txt") && !est_web("js") && !est_web("css"));
    }
}
