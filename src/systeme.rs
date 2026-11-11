// Petits services Windows : ouvrir un fichier, formater dates et tailles

use windows::core::PCWSTR;
use windows::Win32::Foundation::{FILETIME, HWND, SYSTEMTIME};
use windows::Win32::System::Time::{FileTimeToSystemTime, SystemTimeToTzSpecificLocalTime};
use windows::Win32::System::Com::CoTaskMemFree;
use windows::Win32::UI::Shell::*;

use crate::dossier::Emplacement;
use crate::icones::Icone;
use windows::Win32::UI::WindowsAndMessaging::{SW_HIDE, SW_SHOWNORMAL};

/// Chemin réel d'un dossier connu de Windows (suit les redirections, vers OneDrive par exemple)
fn dossier_connu(id: &windows::core::GUID) -> Option<String> {
    unsafe {
        let p = SHGetKnownFolderPath(id, KNOWN_FOLDER_FLAG(0), None).ok()?;
        let s = p.to_string().ok();
        CoTaskMemFree(Some(p.0 as *const _));
        s
    }
}

/// Dossiers connus du volet : (chemin en minuscules, libellé en majuscules, icône). Le libellé est le nom que le Shell
/// affiche (« Téléchargements » même si le dossier s'appelle « Downloads » sur le disque), lu une seule fois.
fn dossiers_connus() -> &'static Vec<(String, String, Icone, Emplacement)> {
    static LISTE: std::sync::OnceLock<Vec<(String, String, Icone, Emplacement)>> = std::sync::OnceLock::new();
    LISTE.get_or_init(lire_dossiers_connus)
}

/// Lance la lecture des dossiers connus dans un fil : au démarrage elle dure ~90 ms et se fait ainsi pendant la création de la
/// fenêtre au lieu de la précéder. Un appel à `favoris` / `nom_dossier_connu` avant la fin attend simplement le résultat.
pub fn precharger_favoris() {
    let _ = std::thread::Builder::new().name("favoris".into()).spawn(|| {
        let _ = dossiers_connus();
    });
}

fn lire_dossiers_connus() -> Vec<(String, String, Icone, Emplacement)> {
    let liste = [
        (&FOLDERID_Profile, "PERSONNEL", Icone::Maison),
        (&FOLDERID_Desktop, "", Icone::Bureau),
        (&FOLDERID_Documents, "", Icone::Document),
        (&FOLDERID_Downloads, "", Icone::Telechargement),
        (&FOLDERID_Pictures, "", Icone::Image),
        (&FOLDERID_Music, "", Icone::Musique),
        (&FOLDERID_Videos, "", Icone::Video),
    ];
    // SAFETY: appels Shell sur le fil courant (celui de la fenêtre ou le fil `favoris`, qui entre dans son propre appartement COM) ;
    // l'IShellItem et la chaîne rendue sont libérés avant le retour
    unsafe {
        let _ = windows::Win32::System::Com::CoInitializeEx(None, windows::Win32::System::Com::COINIT_APARTMENTTHREADED);
    }
    liste
        .into_iter()
        .filter_map(|(id, force, icone)| {
            let chemin = dossier_connu(id)?;
            let nom = if force.is_empty() { nom_shell(id).unwrap_or_else(|| crate::dossier::nom_affiche(&chemin)).to_uppercase() } else { force.to_string() };
            Some((chemin.trim_end_matches('\\').to_lowercase(), nom, icone, Emplacement::dossier(&chemin)))
        })
        .collect()
}

/// Nom d'un dossier connu tel que le Shell l'affiche (SIGDN_NORMALDISPLAY)
fn nom_shell(id: &windows::core::GUID) -> Option<String> {
    unsafe {
        let item: IShellItem = SHGetKnownFolderItem(id, KF_FLAG_DEFAULT, None).ok()?;
        let p = item.GetDisplayName(SIGDN_NORMALDISPLAY).ok()?;
        let s = p.to_string().ok();
        CoTaskMemFree(Some(p.0 as *const _));
        s
    }
}

/// Libellé du volet pour un dossier connu (onglets et fil d'Ariane l'utilisent aussi : un seul nom par dossier).
/// Aucun appel Shell ici : la liste est lue une fois, puis comparée au chemin.
pub fn nom_dossier_connu(chemin: &str) -> Option<&'static str> {
    let c = chemin.trim_end_matches('\\');
    dossiers_connus().iter().find(|(p, ..)| p.eq_ignore_ascii_case(c)).map(|(_, n, ..)| n.as_str())
}

/// Favoris du volet : dossier personnel, Bureau, Documents, Téléchargements, Images, Musique, Vidéos
pub fn favoris() -> Vec<(String, Emplacement, Icone)> {
    let mut v: Vec<(String, Emplacement, Icone)> = dossiers_connus().iter().map(|(_, nom, icone, e)| (nom.clone(), e.clone(), *icone)).collect();
    v.insert(v.len().min(1), ("RÉCENTS".into(), Emplacement::Recents, Icone::Horloge));
    v
}

/// Fichiers et dossiers ouverts récemment : les raccourcis que Windows range dans « Recent »,
/// suivis jusqu'à leur cible (celles qui existent encore). La date d'arrivée est celle de l'ouverture.
pub fn recents() -> Vec<crate::dossier::Entree> {
    use std::os::windows::fs::MetadataExt;
    use windows::core::Interface;
    use windows::Win32::System::Com::*;
    let Some(dossier) = std::env::var_os("APPDATA").map(|a| std::path::Path::new(&a).join("Microsoft\\Windows\\Recent")) else { return Vec::new() };
    let mut liens: Vec<(u64, std::path::PathBuf)> = std::fs::read_dir(&dossier)
        .map(|it| {
            it.flatten()
                .filter(|e| e.path().extension().is_some_and(|x| x.eq_ignore_ascii_case("lnk")))
                .filter_map(|e| Some((e.metadata().ok()?.last_write_time(), e.path())))
                .collect()
        })
        .unwrap_or_default();
    liens.sort_by(|a, b| b.0.cmp(&a.0));
    let mut vus = std::collections::HashSet::new();
    let mut v = Vec::new();
    unsafe {
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        for (quand, lnk) in liens.into_iter().take(300) {
            if v.len() >= 150 {
                break;
            }
            let Ok(lien) = CoCreateInstance::<_, IShellLinkW>(&ShellLink, None, CLSCTX_INPROC_SERVER) else { continue };
            let Ok(fichier) = lien.cast::<IPersistFile>() else { continue };
            if fichier.Load(&windows::core::HSTRING::from(lnk.as_os_str()), STGM_READ).is_err() {
                continue;
            }
            let mut buf = [0u16; 1024];
            // Sans résolution (SLGP_RAWPATH évité) : rapide, et pas de recherche sur le disque
            if lien.GetPath(&mut buf, std::ptr::null_mut(), 0).is_err() {
                continue;
            }
            let n = buf.iter().position(|&c| c == 0).unwrap_or(0);
            let cible = String::from_utf16_lossy(&buf[..n]);
            if cible.len() < 3 || !vus.insert(cible.to_lowercase()) {
                continue;
            }
            let Ok(m) = std::fs::metadata(&cible) else { continue };
            let Some((parent, nom)) = cible.trim_end_matches('\\').rsplit_once('\\') else { continue };
            let parent = if parent.len() == 2 { format!("{parent}\\") } else { parent.to_string() };
            let mut e = crate::dossier::entree_fichier(nom, m.len(), m.last_write_time(), m.is_dir());
            e.ajout = quand;
            e.parent = Some(std::sync::Arc::from(parent.as_str()));
            v.push(e);
        }
    }
    v
}

/// Dossiers où l'on cherche d'abord le plus récent : Téléchargements, Bureau, Images, Vidéos, Musique
pub fn dossier_recent(chemin: &str) -> bool {
    static LISTE: std::sync::OnceLock<Vec<String>> = std::sync::OnceLock::new();
    let liste = LISTE.get_or_init(|| {
        [&FOLDERID_Downloads, &FOLDERID_Desktop, &FOLDERID_Pictures, &FOLDERID_Videos, &FOLDERID_Music, &FOLDERID_Screenshots]
            .into_iter()
            .filter_map(dossier_connu)
            .collect()
    });
    let c = chemin.trim_end_matches('\\');
    liste.iter().any(|d| d.eq_ignore_ascii_case(c))
}

/// Le dossier Téléchargements (regroupé par date la première fois)
/// Chemin du dossier Téléchargements
pub fn chemin_telechargements() -> Option<String> {
    dossier_connu(&FOLDERID_Downloads)
}

pub fn dossier_telechargements(chemin: &str) -> bool {
    static D: std::sync::OnceLock<Option<String>> = std::sync::OnceLock::new();
    D.get_or_init(|| dossier_connu(&FOLDERID_Downloads)).as_deref().is_some_and(|d| d.eq_ignore_ascii_case(chemin.trim_end_matches('\\')))
}

/// Jour local d'une date FILETIME : (numéro du jour, année, mois)
fn jour_local(ft: u64) -> Option<(i64, u16, u16)> {
    let f = FILETIME { dwLowDateTime: ft as u32, dwHighDateTime: (ft >> 32) as u32 };
    let mut utc = SYSTEMTIME::default();
    let mut loc = SYSTEMTIME::default();
    unsafe {
        FileTimeToSystemTime(&f, &mut utc).ok()?;
        SystemTimeToTzSpecificLocalTime(None, &utc, &mut loc).ok()?;
    }
    // Jours depuis une origine fixe (calendrier grégorien)
    let (a, m, j) = (loc.wYear as i64, loc.wMonth as i64, loc.wDay as i64);
    let (a2, m2) = if m <= 2 { (a - 1, m + 9) } else { (a, m - 3) };
    let ere = a2.div_euclid(400);
    let ae = a2 - ere * 400;
    let ja = (153 * m2 + 2) / 5 + j - 1;
    let je = ae * 365 + ae / 4 - ae / 100 + ja;
    Some((ere * 146097 + je, loc.wYear, loc.wMonth))
}

/// Sections du regroupement par date
pub const TRANCHES_DATE: [&str; 7] = ["AUJOURD'HUI", "HIER", "CETTE SEMAINE", "CE MOIS-CI", "CETTE ANNEE", "PLUS ANCIEN", "SANS DATE"];

/// Section d'une date : 0 aujourd'hui, 1 hier, 2 cette semaine, 3 ce mois-ci, 4 cette année, 5 plus ancien, 6 sans date
pub fn tranche_date(ft: u64) -> u32 {
    if ft == 0 {
        return 6;
    }
    let maintenant = unsafe { windows::Win32::System::SystemInformation::GetSystemTimeAsFileTime() };
    let maintenant = (maintenant.dwHighDateTime as u64) << 32 | maintenant.dwLowDateTime as u64;
    let (Some((auj, a, m)), Some((jour, a2, m2))) = (jour_local(maintenant), jour_local(ft)) else { return 6 };
    let ecart = auj - jour;
    // Jours écoulés depuis lundi (le jour 0 de l'origine est un mercredi)
    let depuis_lundi = (auj + 2).rem_euclid(7);
    match ecart {
        e if e <= 0 => 0,
        1 => 1,
        e if e <= depuis_lundi => 2,
        _ if a == a2 && m == m2 => 3,
        _ if a == a2 => 4,
        _ => 5,
    }
}

/// Ouvre un fichier avec son application associée
pub fn lancer(hwnd: HWND, chemin: &str) {
    let w: Vec<u16> = chemin.encode_utf16().chain(std::iter::once(0)).collect();
    let verbe: Vec<u16> = "open\0".encode_utf16().collect();
    // SAFETY: les tampons finissent par 0 et vivent jusqu'au retour des appels
    let mut r = unsafe { ShellExecuteW(hwnd, PCWSTR(verbe.as_ptr()), PCWSTR(w.as_ptr()), PCWSTR::null(), PCWSTR::null(), SW_SHOWNORMAL) };    if r.0 as isize <= 32 {
        // Sans fenêtre propriétaire ni verbe : le Shell choisit l'action par défaut
        r = unsafe { ShellExecuteW(HWND::default(), PCWSTR::null(), PCWSTR(w.as_ptr()), PCWSTR::null(), PCWSTR::null(), SW_SHOWNORMAL) };
        if r.0 as isize <= 32 {
            crate::journal::operation("OUVRIR ECHEC", &format!("code {} : {chemin}", r.0 as isize));
            crate::journal::ecrire(crate::journal::Niveau::Erreur, "systeme", &format!("ShellExecute a echoue (code {}) : {chemin}", r.0 as isize));
        }
    }
}

/// Applique un verbe du Shell à un dossier, sans fenêtre (« pintohome » : l'épingler à l'Accès rapide de
/// Windows, que les boîtes Ouvrir et Enregistrer de toutes les applis montrent). Vrai si le Shell l'a accepté.
pub fn verbe_dossier(chemin: &str, verbe: &str) -> bool {
    let w: Vec<u16> = chemin.encode_utf16().chain(std::iter::once(0)).collect();
    let v: Vec<u16> = verbe.encode_utf16().chain(std::iter::once(0)).collect();
    // SAFETY: les deux tampons finissent par 0 et vivent jusqu'au retour de l'appel
    let r = unsafe { ShellExecuteW(HWND::default(), PCWSTR(v.as_ptr()), PCWSTR(w.as_ptr()), PCWSTR::null(), PCWSTR::null(), SW_HIDE) };
    r.0 as isize > 32
}

/// « 20/10/2026 14:03 » en heure locale
pub fn date(ft: u64) -> String {
    if ft == 0 {
        return String::new();
    }
    let f = FILETIME { dwLowDateTime: ft as u32, dwHighDateTime: (ft >> 32) as u32 };
    let mut utc = SYSTEMTIME::default();
    let mut loc = SYSTEMTIME::default();
    unsafe {
        if FileTimeToSystemTime(&f, &mut utc).is_err() || SystemTimeToTzSpecificLocalTime(None, &utc, &mut loc).is_err() {
            return String::new();
        }
    }
    format!("{:02}/{:02}/{} {:02}:{:02}", loc.wDay, loc.wMonth, loc.wYear, loc.wHour, loc.wMinute)
}

/// Jour local d'une date (FILETIME) : nombre de jours depuis 1601, à l'heure de ce PC
pub fn numero_jour(ft: u64) -> u32 {
    static BIAIS: std::sync::OnceLock<i64> = std::sync::OnceLock::new();
    let biais = *BIAIS.get_or_init(|| unsafe {
        let utc = maintenant();
        let f = FILETIME { dwLowDateTime: utc as u32, dwHighDateTime: (utc >> 32) as u32 };
        let (mut s, mut l, mut fl) = (SYSTEMTIME::default(), SYSTEMTIME::default(), FILETIME::default());
        if FileTimeToSystemTime(&f, &mut s).is_err() || SystemTimeToTzSpecificLocalTime(None, &s, &mut l).is_err() || windows::Win32::System::Time::SystemTimeToFileTime(&l, &mut fl).is_err() {
            return 0;
        }
        ((fl.dwHighDateTime as u64) << 32 | fl.dwLowDateTime as u64) as i64 - utc as i64
    });
    ((ft as i64 + biais).max(0) / 864_000_000_000) as u32
}

/// « MAR 12/10 » pour un numéro de jour (numero_jour)
pub fn jour_court(jour: u32) -> String {
    let ft = jour as u64 * 864_000_000_000 + 1;
    let f = FILETIME { dwLowDateTime: ft as u32, dwHighDateTime: (ft >> 32) as u32 };
    let mut s = SYSTEMTIME::default();
    unsafe {
        if FileTimeToSystemTime(&f, &mut s).is_err() {
            return String::new();
        }
    }
    const JOURS: [&str; 7] = ["DIM", "LUN", "MAR", "MER", "JEU", "VEN", "SAM"];
    format!("{} {:02}/{:02}", JOURS[s.wDayOfWeek as usize % 7], s.wDay, s.wMonth)
}

/// Premier jour du mois ? (repère dans la chronologie)
pub fn debut_de_mois(jour: u32) -> bool {
    let ft = jour as u64 * 864_000_000_000 + 1;
    let f = FILETIME { dwLowDateTime: ft as u32, dwHighDateTime: (ft >> 32) as u32 };
    let mut s = SYSTEMTIME::default();
    unsafe { FileTimeToSystemTime(&f, &mut s).is_ok() && s.wDay == 1 }
}

/// « IL Y A 5 MIN », « HIER 14:02 », sinon la date complète (réglage dates relatives)
pub fn date_relative(ft: u64) -> String {
    if ft == 0 {
        return String::new();
    }
    let maintenant = unsafe { windows::Win32::System::SystemInformation::GetSystemTimeAsFileTime() };
    let maintenant = (maintenant.dwHighDateTime as u64) << 32 | maintenant.dwLowDateTime as u64;
    let complete = date(ft);
    if ft > maintenant {
        return complete;
    }
    let s = (maintenant - ft) / 10_000_000;
    let heure = complete.rsplit(' ').next().unwrap_or("").to_string();
    match s {
        0..=59 => "A L'INSTANT".into(),
        60..=3599 => format!("IL Y A {} MIN", s / 60),
        3600..=21599 => format!("IL Y A {} H", s / 3600),
        _ => {
            // Jours calendaires : on compare les dates locales
            let jour = |t: u64| date(t).split(' ').next().unwrap_or("").to_string();
            let hier = jour(maintenant - 864_000_000_000);
            if jour(ft) == jour(maintenant) {
                format!("AUJOURD'HUI {heure}")
            } else if jour(ft) == hier {
                format!("HIER {heure}")
            } else if s < 7 * 86400 {
                format!("IL Y A {} JOURS", (s + 86399) / 86400)
            } else {
                complete
            }
        }
    }
}

/// Date pour la colonne MODIFIE selon le réglage
pub fn date_liste(ft: u64) -> String {
    if crate::reglages::DATES_RELATIVES.load(std::sync::atomic::Ordering::Relaxed) {
        date_relative(ft)
    } else {
        date(ft)
    }
}

/// « 12,4 MO »
pub fn taille(o: u64) -> String {
    const UNITES: [&str; 5] = ["O", "KO", "MO", "GO", "TO"];
    if o < 1024 {
        return format!("{o} O");
    }
    let mut v = o as f64;
    let mut u = 0;
    while v >= 1024.0 && u < UNITES.len() - 1 {
        v /= 1024.0;
        u += 1;
    }
    let s = if v < 100.0 { format!("{v:.1}") } else { format!("{v:.0}") };
    format!("{} {}", s.replace('.', ","), UNITES[u])
}

/// Nombre avec espace des milliers : « 12 345 »
pub fn nombre(n: usize) -> String {
    let s = n.to_string();
    let mut r = String::new();
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i) % 3 == 0 {
            r.push(' ');
        }
        r.push(c);
    }
    r
}

/// Maintenant, en FILETIME
pub fn maintenant() -> u64 {
    let t = unsafe { windows::Win32::System::SystemInformation::GetSystemTimeAsFileTime() };
    (t.dwHighDateTime as u64) << 32 | t.dwLowDateTime as u64
}

pub fn annee_courante() -> u32 {
    let mut st = SYSTEMTIME::default();
    let f = maintenant();
    let ft = FILETIME { dwLowDateTime: f as u32, dwHighDateTime: (f >> 32) as u32 };
    unsafe {
        let _ = FileTimeToSystemTime(&ft, &mut st);
    }
    st.wYear as u32
}

/// FILETIME (UTC) d'une date donnée en heure locale
pub fn filetime(annee: u32, mois: u32, jour: u32, heure: u32, minute: u32) -> u64 {
    let local = SYSTEMTIME { wYear: annee as u16, wMonth: mois as u16, wDay: jour as u16, wHour: heure as u16, wMinute: minute as u16, ..Default::default() };
    let (mut st, mut ft) = (SYSTEMTIME::default(), FILETIME::default());
    unsafe {
        if windows::Win32::System::Time::TzSpecificLocalTimeToSystemTime(None, &local, &mut st).is_err() || windows::Win32::System::Time::SystemTimeToFileTime(&st, &mut ft).is_err() {
            return 0;
        }
    }
    (ft.dwHighDateTime as u64) << 32 | ft.dwLowDateTime as u64
}

/// « 2026-10-20 » en heure locale (renommage par lot)
pub fn date_iso(ft: u64) -> String {
    let d = date(ft);
    let mut p = d.split(' ').next().unwrap_or("").split('/');
    match (p.next(), p.next(), p.next()) {
        (Some(j), Some(m), Some(a)) => format!("{a}-{m}-{j}"),
        _ => String::new(),
    }
}

/// Boîte de choix de Windows : un dossier (`dossier`) ou un fichier .toml. None si l'utilisateur annule.
pub fn choisir(hwnd: HWND, dossier: bool) -> Option<String> {
    use windows::Win32::System::Com::{CoCreateInstance, CLSCTX_INPROC_SERVER};
    // SAFETY: boîte modale sur le fil d'interface ; la chaîne rendue est copiée puis libérée
    unsafe {
        let d: IFileOpenDialog = CoCreateInstance(&FileOpenDialog, None, CLSCTX_INPROC_SERVER).ok()?;
        let options = d.GetOptions().ok()?;
        d.SetOptions(options | if dossier { FOS_PICKFOLDERS } else { FOS_FORCEFILESYSTEM }).ok()?;
        if !dossier {
            let nom: Vec<u16> = "Réglages (*.toml)".encode_utf16().chain(std::iter::once(0)).collect();
            let motif: Vec<u16> = "*.toml".encode_utf16().chain(std::iter::once(0)).collect();
            let filtre = [windows::Win32::UI::Shell::Common::COMDLG_FILTERSPEC { pszName: PCWSTR(nom.as_ptr()), pszSpec: PCWSTR(motif.as_ptr()) }];
            d.SetFileTypes(&filtre).ok()?;
        }
        // Boucle modale (voir `GardeModale`) ; la garde se détruit à la sortie de la fonction, `Show` rendu
        let _garde = crate::fenetre::GardeModale::nouvelle(hwnd);
        d.Show(hwnd).ok()?;
        let r = d.GetResult().ok()?;
        let p = r.GetDisplayName(SIGDN_FILESYSPATH).ok()?;
        let s = p.to_string().ok();
        CoTaskMemFree(Some(p.0 as *const _));
        s
    }
}
