// Mots d'état des lignes CLOUD et SERVEURS du volet (lot 14b). Rien n'est affiché qui n'ait été mesuré :
// - serveur / partage SMB : le chemin répond-il dans les 2 s (test dans un fil, jamais dans le dessin) → « Connecté » / « Hors ligne » ;
// - cloud : le client du fournisseur tourne-t-il (instantané des processus, une fois par mesure) → « Actif » / « Hors ligne ».
// « À jour », « Synchronisation… » et « Erreur de synchro » de la maquette ne sont pas mesurables : ils n'existent pas ici.
// La mesure est ponctuelle (ouverture, retour de la fenêtre au premier plan, relecture des disques), sans minuterie.

use std::collections::{HashMap, HashSet};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use windows::Win32::Foundation::{CloseHandle, HWND, LPARAM, WPARAM};
use windows::Win32::System::Diagnostics::ToolHelp::{CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W, TH32CS_SNAPPROCESS};
use windows::Win32::UI::WindowsAndMessaging::{PostMessageW, WM_APP};

use crate::dossier::Emplacement;
use crate::stockages::Lieu;

/// Résultat d'une mesure (LPARAM = Box<Vec<(String, Etat)>>, clé = `Lieu.detail`)
pub const WM_ETATS: u32 = WM_APP + 41;

/// Délai au-delà duquel un partage est dit hors ligne (un partage coupé fait attendre Windows bien plus longtemps)
pub const DELAI: Duration = Duration::from_secs(2);
/// Deux mesures de suite, sauf lieu nouveau : au moins 30 s d'écart
pub const ECART: Duration = Duration::from_secs(30);

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Etat {
    /// Serveur dont le partage a répondu
    Connecte,
    /// Serveur qui n'a pas répondu
    HorsLigne,
    /// Client cloud absent des processus : ses fichiers restent lisibles, mais rien ne se synchronise
    Arrete,
    /// Client cloud en cours d'exécution
    Actif,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Genre {
    Serveur,
    Cloud,
}

/// Couleur du mot : accent, gris ou erreur (maquette `c-a`, `c-n`, `c-e`)
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Ton {
    Accent,
    Neutre,
    #[allow(dead_code)]
    Erreur,
}

/// Signe devant le mot : `✓` de `.st.ck` ou point de `.st:before`
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Marque {
    Coche,
    Point,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Mot {
    pub texte: &'static str,
    pub ton: Ton,
    pub marque: Marque,
}

/// État déduit des deux mesures : un serveur ne dépend que de sa réponse, un cloud que de son client
pub fn etat(genre: Genre, joignable: bool, client_actif: bool) -> Etat {
    match (genre, joignable, client_actif) {
        (Genre::Serveur, true, _) => Etat::Connecte,
        (Genre::Cloud, _, true) => Etat::Actif,
        (Genre::Cloud, _, false) => Etat::Arrete,
        _ => Etat::HorsLigne,
    }
}

/// Mot, couleur et signe d'un état. La couleur ne porte jamais seule l'état : le mot est toujours là.
pub fn mot(e: Etat) -> Mot {
    match e {
        Etat::Connecte => Mot { texte: "Connecté", ton: Ton::Neutre, marque: Marque::Coche },
        Etat::Actif => Mot { texte: "Actif", ton: Ton::Accent, marque: Marque::Point },
        Etat::HorsLigne => Mot { texte: "Hors ligne", ton: Ton::Neutre, marque: Marque::Point },
        Etat::Arrete => Mot { texte: "Client arrêté", ton: Ton::Neutre, marque: Marque::Point },
    }
}

/// Processus du client d'un fournisseur cloud, d'après le nom du lieu (« OneDrive · Entreprise » → OneDrive).
/// Un nom inconnu (dossier ajouté à la main) : aucun client, donc aucun mot.
pub fn processus_client(nom: &str) -> Option<&'static [&'static str]> {
    let bas = nom.trim().to_lowercase();
    let table: [(&str, &[&str]); 4] = [
        ("onedrive", &["onedrive.exe"]),
        ("google drive", &["googledrivefs.exe"]),
        ("dropbox", &["dropbox.exe"]),
        ("icloud", &["iclouddrive.exe", "icloudservices.exe"]),
    ];
    table.into_iter().find_map(|(marque, procs)| {
        let reste = bas.strip_prefix(marque)?;
        // Mot entier : « onedrivexyz » n'est pas OneDrive
        reste.chars().next().is_none_or(|c| !c.is_alphanumeric()).then_some(procs)
    })
}

/// Le client tourne-t-il ? `actifs` : noms d'exécutables en minuscules
pub fn client_actif(procs: &[&str], actifs: &HashSet<String>) -> bool {
    procs.iter().any(|p| actifs.contains(*p))
}

/// Test d'un serveur : un partage (`\\hote\partage`) répond-il à `Path::exists`, ou, pour un serveur seul (`\\hote`, sans
/// partage : `Path::exists` y échoue même joignable), son port SMB 445 accepte-t-il une connexion. Pas de test fiable pour
/// SFTP (`sftp://`) ni WebDAV (`\\hote@SSL\…`, client WebClient lent) : aucun mot plutôt qu'un mot inventé.
pub fn a_tester(l: &Lieu) -> Option<Nature> {
    match &l.emplacement {
        Emplacement::Dossier(c) => {
            let hote = c.strip_prefix("\\\\")?.split('\\').next()?;
            (!hote.is_empty() && !hote.contains('@')).then(|| Nature::Serveur(c.clone()))
        }
        Emplacement::Serveur(h) => (!h.is_empty() && !h.contains('@') && !h.contains(':')).then(|| Nature::Hote(h.clone())),
        _ => None,
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Nature {
    /// Chemin d'un partage
    Serveur(String),
    /// Nom ou adresse d'un serveur seul : port SMB
    Hote(String),
    Cloud(&'static [&'static str]),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Demande {
    /// `Lieu.detail`
    pub cle: String,
    pub nature: Nature,
}

/// Ce qu'il y a à mesurer pour les lieux affichés (les autres n'auront jamais de mot)
pub fn demandes(nuages: &[Lieu], serveurs: &[Lieu]) -> Vec<Demande> {
    let clouds = nuages.iter().filter_map(|l| processus_client(&l.nom).map(|p| Demande { cle: l.detail.clone(), nature: Nature::Cloud(p) }));
    let srv = serveurs.iter().filter_map(|l| a_tester(l).map(|nature| Demande { cle: l.detail.clone(), nature }));
    clouds.chain(srv).collect()
}

/// Cache des états mesurés, par `Lieu.detail`. Inconnu (jamais mesuré) : `None`, donc rien d'affiché.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Cache(HashMap<String, Etat>);

impl Cache {
    pub fn lire(&self, cle: &str) -> Option<Etat> {
        self.0.get(cle).copied()
    }

    /// Une mesure complète remplace l'ancienne : un lieu retiré ou devenu non testable n'affiche plus rien
    pub fn remplacer(&mut self, mesures: Vec<(String, Etat)>) {
        self.0 = mesures.into_iter().collect();
    }
}

/// Noms (minuscules) des exécutables en cours, par un instantané des processus
fn processus_actifs() -> HashSet<String> {
    let mut v = HashSet::new();
    // SAFETY: `e.dwSize` est la taille de la structure (contrat de ToolHelp) ; l'instantané est refermé avant de sortir
    unsafe {
        let Ok(h) = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) else { return v };
        let mut e = PROCESSENTRY32W { dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32, ..Default::default() };
        if Process32FirstW(h, &mut e).is_ok() {
            loop {
                let n = e.szExeFile.iter().position(|&c| c == 0).unwrap_or(e.szExeFile.len());
                v.insert(String::from_utf16_lossy(&e.szExeFile[..n]).to_lowercase());
                if Process32NextW(h, &mut e).is_err() {
                    break;
                }
            }
        }
        let _ = CloseHandle(h);
    }
    v
}

/// Le port SMB (445) du serveur accepte-t-il une connexion avant `delai` ?
fn port_smb_ouvert(hote: &str, delai: Duration) -> bool {
    use std::net::ToSocketAddrs;
    (hote, 445).to_socket_addrs().is_ok_and(|mut a| a.any(|ad| std::net::TcpStream::connect_timeout(&ad, delai).is_ok()))
}

/// Mesure bloquante (à lancer dans un fil). Chaque partage est sondé dans son propre fil, tous en parallèle : un partage coupé
/// laisse son fil attendre Windows, la mesure n'attend que `delai` au total et le dit hors ligne.
pub fn mesurer(demandes: &[Demande], delai: Duration, actifs: &dyn Fn() -> HashSet<String>) -> Vec<(String, Etat)> {
    let (tx, rx) = mpsc::channel::<(String, bool)>();
    let mut attendus = 0;
    for d in demandes {
        let (tx, cle, nature) = (tx.clone(), d.cle.clone(), d.nature.clone());
        match nature {
            Nature::Serveur(chemin) => {
                attendus += 1;
                std::thread::spawn(move || {
                    let _ = tx.send((cle, std::path::Path::new(&chemin).exists()));
                });
            }
            Nature::Hote(hote) => {
                attendus += 1;
                std::thread::spawn(move || {
                    let _ = tx.send((cle, port_smb_ouvert(&hote, delai)));
                });
            }
            Nature::Cloud(_) => {}
        }
    }
    drop(tx);
    // Une seule lecture des processus, seulement s'il y a un cloud à juger
    let procs = if demandes.iter().any(|d| matches!(d.nature, Nature::Cloud(_))) { actifs() } else { HashSet::new() };
    let fin = Instant::now() + delai;
    let mut reponses: HashMap<String, bool> = HashMap::new();
    while reponses.len() < attendus {
        let reste = fin.saturating_duration_since(Instant::now());
        match rx.recv_timeout(reste) {
            Ok((cle, ok)) => {
                reponses.insert(cle, ok);
            }
            Err(_) => break,
        }
    }
    demandes
        .iter()
        .map(|d| {
            let e = match &d.nature {
                Nature::Serveur(_) | Nature::Hote(_) => etat(Genre::Serveur, reponses.get(&d.cle).copied().unwrap_or(false), false),
                Nature::Cloud(p) => etat(Genre::Cloud, false, client_actif(p, &procs)),
            };
            (d.cle.clone(), e)
        })
        .collect()
}

/// Lance la mesure dans un fil et poste `WM_ETATS` à la fenêtre
pub fn lancer(hwnd: HWND, demandes: Vec<Demande>) {
    let fenetre = hwnd.0 as isize;
    std::thread::spawn(move || {
        let r = Box::new(mesurer(&demandes, DELAI, &processus_actifs));
        // SAFETY: le pointeur est repris par la fenêtre (`Box::from_raw`) ou ici si l'envoi échoue
        unsafe {
            let p = Box::into_raw(r);
            if PostMessageW(HWND(fenetre as *mut _), WM_ETATS, WPARAM(0), LPARAM(p as isize)).is_err() {
                drop(Box::from_raw(p));
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::icones::Icone;

    fn lieu(nom: &str, chemin: &str) -> Lieu {
        Lieu { nom: nom.into(), emplacement: Emplacement::dossier(chemin), icone: Icone::Serveur, ajoute: None, detail: chemin.into() }
    }

    #[test]
    fn mot_et_couleur_selon_le_genre_et_la_mesure() {
        assert_eq!(etat(Genre::Serveur, true, false), Etat::Connecte);
        assert_eq!(etat(Genre::Serveur, false, true), Etat::HorsLigne);
        // Un client qui tourne ne rend pas un serveur joignable, et un partage joignable ne rend pas un cloud actif
        assert_eq!(etat(Genre::Cloud, true, false), Etat::Arrete);
        assert_eq!(etat(Genre::Cloud, false, true), Etat::Actif);
        let m = mot(Etat::Connecte);
        assert_eq!((m.texte, m.ton, m.marque), ("Connecté", Ton::Neutre, Marque::Coche));
        let m = mot(Etat::Actif);
        assert_eq!((m.texte, m.ton), ("Actif", Ton::Accent));
        assert_eq!(mot(Etat::HorsLigne).texte, "Hors ligne");
    }

    #[test]
    fn aucun_mot_non_mesurable() {
        for e in [Etat::Connecte, Etat::Actif, Etat::HorsLigne, Etat::Arrete] {
            let t = mot(e).texte;
            assert!(!t.contains("jour") && !t.contains("ynchro"), "{t}");
        }
    }

    #[test]
    fn client_depuis_le_nom_du_lieu() {
        assert_eq!(processus_client("OneDrive"), Some(&["onedrive.exe"][..]));
        assert_eq!(processus_client("OneDrive · Entreprise"), Some(&["onedrive.exe"][..]));
        assert_eq!(processus_client("Google Drive"), Some(&["googledrivefs.exe"][..]));
        assert_eq!(processus_client("DROPBOX"), Some(&["dropbox.exe"][..]));
        assert!(processus_client("iCloud").unwrap().contains(&"iclouddrive.exe"));
        // Mot entier seulement, et rien pour un dossier quelconque
        assert_eq!(processus_client("OneDrivexyz"), None);
        assert_eq!(processus_client("Mon dossier"), None);
    }

    #[test]
    fn client_actif_compare_en_minuscules_exactes() {
        let actifs: HashSet<String> = ["explorer.exe", "onedrive.exe"].iter().map(|s| s.to_string()).collect();
        assert!(client_actif(&["onedrive.exe"], &actifs));
        assert!(client_actif(&["iclouddrive.exe", "onedrive.exe"], &actifs));
        assert!(!client_actif(&["dropbox.exe"], &actifs));
    }

    #[test]
    fn seuls_les_partages_reseau_sont_testes() {
        assert_eq!(a_tester(&lieu("NAS", r"\\nas\Photos")), Some(Nature::Serveur(r"\\nas\Photos".into())));
        // Serveur seul : le port SMB, pas `Path::exists`
        assert_eq!(a_tester(&lieu("ZIMA", r"\\10.0.0.5")), Some(Nature::Hote("10.0.0.5".into())));
        assert_eq!(a_tester(&lieu("SFTP", "sftp://hote/home")), None);
        assert_eq!(a_tester(&lieu("WEBDAV", r"\\cloud.fr@SSL\dav")), None);
        assert_eq!(a_tester(&lieu("LOCAL", r"C:\Dossier")), None);
    }

    #[test]
    fn serveur_seul_teste_par_son_port_smb() {
        // Le 445 de cette machine peut être ouvert ou non : on vérifie que la sonde rend vite un booléen, et faux pour un nom
        // qui ne se résout pas
        let debut = Instant::now();
        let _ = port_smb_ouvert("127.0.0.1", Duration::from_millis(500));
        assert!(!port_smb_ouvert("sf-lot14b-hote-inexistant.invalid", Duration::from_millis(500)));
        assert!(debut.elapsed() < Duration::from_secs(5));
    }

    #[test]
    fn demandes_par_lieu() {
        let nuages = [lieu("OneDrive", r"C:\Users\x\OneDrive"), lieu("Mon dossier", r"D:\x")];
        let serveurs = [lieu("NAS", r"\\nas\p"), lieu("SFTP", "sftp://h/x")];
        let d = demandes(&nuages, &serveurs);
        assert_eq!(d.len(), 2);
        assert_eq!(d[0].cle, r"C:\Users\x\OneDrive");
        assert_eq!(d[1].nature, Nature::Serveur(r"\\nas\p".into()));
    }

    #[test]
    fn cache_inconnu_puis_mot() {
        let mut c = Cache::default();
        assert_eq!(c.lire("a"), None);
        c.remplacer(vec![("a".into(), Etat::Connecte)]);
        assert_eq!(c.lire("a"), Some(Etat::Connecte));
        // Une mesure ultérieure sans ce lieu : il n'affiche plus rien
        c.remplacer(vec![("b".into(), Etat::HorsLigne)]);
        assert_eq!(c.lire("a"), None);
        assert_eq!(c.lire("b"), Some(Etat::HorsLigne));
    }

    #[test]
    fn mesure_serveur_joignable_ou_non_et_cloud_selon_les_processus() {
        let dossier = std::env::temp_dir();
        let d = vec![
            Demande { cle: "ok".into(), nature: Nature::Serveur(dossier.to_string_lossy().into()) },
            Demande { cle: "ko".into(), nature: Nature::Serveur(dossier.join("sf-lot14b-inexistant-xyz").to_string_lossy().into()) },
            Demande { cle: "actif".into(), nature: Nature::Cloud(&["onedrive.exe"]) },
            Demande { cle: "absent".into(), nature: Nature::Cloud(&["dropbox.exe"]) },
        ];
        let faux = || -> HashSet<String> { ["onedrive.exe".to_string()].into_iter().collect() };
        let m: HashMap<String, Etat> = mesurer(&d, Duration::from_secs(5), &faux).into_iter().collect();
        assert_eq!(m["ok"], Etat::Connecte);
        assert_eq!(m["ko"], Etat::HorsLigne);
        assert_eq!(m["actif"], Etat::Actif);
        assert_eq!(m["absent"], Etat::Arrete);
    }

    #[test]
    fn instantane_des_processus_contient_le_test() {
        // Le binaire de test tourne : l'instantané n'est pas vide et les noms sont en minuscules
        let p = processus_actifs();
        assert!(!p.is_empty());
        assert!(p.iter().all(|n| *n == n.to_lowercase()));
    }
}
