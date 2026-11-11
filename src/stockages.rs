// Stockages du volet : cloud détecté (OneDrive, Google Drive, Dropbox, iCloud… tous les fournisseurs
// inscrits auprès de Windows) et stockages ajoutés dans les réglages (ZimaOS, partages SMB, WebDAV, dossiers).
// Les mots de passe vont dans le Gestionnaire d'identification de Windows, jamais dans nos fichiers.

use windows::core::{HSTRING, PCWSTR, PWSTR};
use windows::Win32::Foundation::*;
use windows::Win32::NetworkManagement::NetManagement::NetApiBufferFree;
use windows::Win32::NetworkManagement::WNet::*;
use windows::Win32::Security::Credentials::*;
use windows::Win32::Storage::FileSystem::{NetShareEnum, SHARE_INFO_1, STYPE_DISKTREE};
use windows::Win32::System::Registry::*;
use windows::Win32::UI::Shell::SHLoadIndirectString;

use crate::dossier::{self, Emplacement, Entree, Genre};
use crate::icones::Icone;
use crate::reglages::{Sorte, Stockage};

/// Entrée du volet (section CLOUD ou SERVEURS)
#[derive(Clone, Debug, PartialEq)]
pub struct Lieu {
    pub nom: String,
    pub emplacement: Emplacement,
    pub icone: Icone,
    /// Position dans les stockages ajoutés (None : détecté automatiquement)
    pub ajoute: Option<usize>,
    /// Chemin affiché dans les réglages
    pub detail: String,
}

// ── Registre ──

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

pub struct Cle(HKEY);

impl Drop for Cle {
    fn drop(&mut self) {
        unsafe {
            let _ = RegCloseKey(self.0);
        }
    }
}

pub fn ouvrir(racine: HKEY, chemin: &str) -> Option<Cle> {
    let mut h = HKEY::default();
    unsafe { (RegOpenKeyExW(racine, &HSTRING::from(chemin), 0, KEY_READ, &mut h) == ERROR_SUCCESS).then_some(Cle(h)) }
}

pub fn sous_cles(c: &Cle) -> Vec<String> {
    let mut v = Vec::new();
    for i in 0.. {
        let mut nom = [0u16; 512];
        let mut n = nom.len() as u32;
        if unsafe { RegEnumKeyExW(c.0, i, PWSTR(nom.as_mut_ptr()), &mut n, None, PWSTR::null(), None, None) } != ERROR_SUCCESS {
            break;
        }
        v.push(String::from_utf16_lossy(&nom[..n as usize]));
    }
    v
}

/// Valeurs texte d'une clé : (nom, donnée)
pub fn valeurs(c: &Cle) -> Vec<(String, String)> {
    let mut v = Vec::new();
    for i in 0.. {
        let mut nom = [0u16; 512];
        let mut n = nom.len() as u32;
        let mut donnees = vec![0u8; 4096];
        let mut taille = donnees.len() as u32;
        let mut genre = 0u32;
        let r = unsafe { RegEnumValueW(c.0, i, PWSTR(nom.as_mut_ptr()), &mut n, None, Some(&mut genre), Some(donnees.as_mut_ptr()), Some(&mut taille)) };
        if r == ERROR_NO_MORE_ITEMS {
            break;
        }
        if r != ERROR_SUCCESS || (genre != REG_SZ.0 && genre != REG_EXPAND_SZ.0) {
            continue;
        }
        let u: Vec<u16> = donnees[..taille as usize].chunks_exact(2).map(|p| u16::from_le_bytes([p[0], p[1]])).collect();
        let fin = u.iter().position(|&x| x == 0).unwrap_or(u.len());
        v.push((String::from_utf16_lossy(&nom[..n as usize]), String::from_utf16_lossy(&u[..fin])));
    }
    v
}

/// Texte éventuellement indirect (« @fichier.dll,-123 »)
fn texte_indirect(s: &str) -> String {
    if !s.starts_with('@') {
        return s.to_string();
    }
    let mut buf = [0u16; 512];
    unsafe {
        if SHLoadIndirectString(&HSTRING::from(s), &mut buf, None).is_ok() {
            let n = buf.iter().position(|&x| x == 0).unwrap_or(0);
            return String::from_utf16_lossy(&buf[..n]);
        }
    }
    s.to_string()
}

// ── Cloud détecté ──

/// Marques dont la casse est celle du fournisseur (maquette v6 : Ce PC et Réglages écrivent « OneDrive », « iCloud », « ZimaOS »)
const MARQUES: [&str; 5] = ["OneDrive", "Google Drive", "Dropbox", "iCloud", "ZimaOS"];

/// Remet les marques connues dans leur casse (« ONEDRIVE » → « OneDrive », « zimaos » → « ZimaOS »), le reste du nom tel quel : le
/// volet met lui-même les noms en capitales (`.d` de la maquette, Doto en majuscules), ils ne sont plus stockés en capitales.
/// Un mot entier seulement : « monzimaos » reste tel quel.
pub fn casse_de_marque(nom: &str) -> String {
    let mut s = nom.trim().to_string();
    for marque in MARQUES {
        // Minuscules ASCII seulement : les positions en octets restent celles de `s`
        let bas = marque.to_ascii_lowercase();
        let mut depuis = 0;
        while let Some(i) = s.to_ascii_lowercase()[depuis..].find(&bas).map(|i| i + depuis) {
            let fin = i + bas.len();
            let avant = s[..i].chars().next_back().is_none_or(|c| !c.is_alphanumeric());
            let apres = s[fin..].chars().next().is_none_or(|c| !c.is_alphanumeric());
            if avant && apres {
                s.replace_range(i..fin, marque);
            }
            depuis = fin;
        }
    }
    s
}

/// « OneDrive - Personal » → OneDrive ; « OneDrive - Entreprise » → OneDrive · Entreprise (le compte garde sa casse)
fn nom_cloud(affiche: &str) -> String {
    let a = affiche.trim();
    match a.split_once(" - ") {
        Some((fournisseur, "Personal" | "Personnel")) => casse_de_marque(fournisseur),
        Some((fournisseur, compte)) => format!("{} · {}", casse_de_marque(fournisseur), compte.trim()),
        None => casse_de_marque(a),
    }
}

/// Tous les dossiers synchronisés inscrits auprès de Windows (SyncRootManager), plus Google Drive
/// en mode « lecteur » (un disque nommé Google Drive), plus Dropbox / iCloud anciens.
pub fn cloud_detecte(disques: &[Entree]) -> Vec<Lieu> {
    // Mode recette (`SPACEFILES_SANS_REGISTRE=1`) : ne jamais exposer les comptes cloud du vrai profil dans une capture.
    if crate::defaut::sans_registre() {
        return Vec::new();
    }
    let mut v: Vec<Lieu> = Vec::new();
    let mut ajouter = |nom: String, chemin: String| {
        let deja = v.iter().any(|l| matches!(&l.emplacement, Emplacement::Dossier(c) if c.eq_ignore_ascii_case(chemin.trim_end_matches('\\'))));
        if !deja && std::path::Path::new(&chemin).is_dir() {
            v.push(Lieu { nom, emplacement: Emplacement::dossier(&chemin), icone: Icone::Nuage, ajoute: None, detail: chemin });
        }
    };
    if let Some(racine) = ouvrir(HKEY_LOCAL_MACHINE, r"SOFTWARE\Microsoft\Windows\CurrentVersion\Explorer\SyncRootManager") {
        for id in sous_cles(&racine) {
            let Some(c) = ouvrir(racine.0, &id) else { continue };
            let affiche = valeurs(&c).into_iter().find(|(n, _)| n == "DisplayNameResource").map(|(_, d)| texte_indirect(&d)).unwrap_or_else(|| id.clone());
            let Some(racines) = ouvrir(c.0, "UserSyncRoots") else { continue };
            for (_, chemin) in valeurs(&racines) {
                // Dossier temporaire interne de OneDrive : pas pour l'utilisateur
                if chemin.to_lowercase().ends_with("cloudtemp") {
                    continue;
                }
                ajouter(nom_cloud(&affiche), chemin);
            }
        }
    }
    // Google Drive pour ordinateur, mode « streaming » : un disque virtuel
    for d in disques {
        if let Genre::Disque { racine, .. } = &d.genre {
            if d.nom.to_uppercase().starts_with("GOOGLE DRIVE") {
                let mon_drive = format!("{racine}Mon Drive");
                let my_drive = format!("{racine}My Drive");
                let chemin = [mon_drive, my_drive].into_iter().find(|c| std::path::Path::new(c).is_dir()).unwrap_or(racine.clone());
                ajouter("Google Drive".into(), chemin);
            }
        }
    }
    let profil = std::env::var("USERPROFILE").unwrap_or_default();
    // Dropbox ancien (sans SyncRootManager) : son info.json donne le dossier
    for base in [std::env::var("LOCALAPPDATA").unwrap_or_default(), std::env::var("APPDATA").unwrap_or_default()] {
        if let Ok(json) = std::fs::read_to_string(format!(r"{base}\Dropbox\info.json")) {
            if let Some(chemin) = json_chemin(&json) {
                ajouter("Dropbox".into(), chemin);
            }
        }
    }
    ajouter("iCloud".into(), format!(r"{profil}\iCloudDrive"));
    v
}

/// Premier `"path": "…"` d'un petit JSON (Dropbox), sans analyseur JSON complet
fn json_chemin(json: &str) -> Option<String> {
    let i = json.find("\"path\"")?;
    let reste = &json[i + 6..];
    let debut = reste.find('"')? + 1;
    let mut s = String::new();
    let mut echappe = false;
    for c in reste[debut..].chars() {
        match (echappe, c) {
            (true, c) => {
                s.push(c);
                echappe = false;
            }
            (false, '\\') => echappe = true,
            (false, '"') => return Some(s),
            (false, c) => s.push(c),
        }
    }
    None
}

// ── Stockages ajoutés ──

/// Chemin Windows d'un stockage : `zima` → `\\zima`, `https://hote/dav` → `\\hote@SSL\dav`
pub fn chemin(sorte: Sorte, adresse: &str) -> String {
    let a = adresse.trim().trim_end_matches(['\\', '/']);
    match sorte {
        Sorte::Dossier => a.to_string(),
        Sorte::Sftp if a.to_ascii_lowercase().starts_with("sftp://") => a.to_string(),
        Sorte::Sftp => format!("sftp://{}", a.replacen(":/", "/", 1)),
        // Déjà un chemin Windows (\\hote@SSL\…)
        Sorte::WebDav if a.starts_with("\\\\") => a.to_string(),
        Sorte::WebDav => {
            let (ssl, reste) = if let Some(r) = a.strip_prefix("https://") {
                (true, r)
            } else if let Some(r) = a.strip_prefix("http://") {
                (false, r)
            } else {
                (true, a.trim_start_matches('\\'))
            };
            let (hote, chemin) = reste.split_once('/').unwrap_or((reste, ""));
            let (hote, port) = hote.split_once(':').map_or((hote, None), |(h, p)| (h, Some(p)));
            let mut unc = format!("\\\\{hote}");
            if ssl {
                unc.push_str("@SSL");
            }
            if let Some(p) = port {
                unc.push('@');
                unc.push_str(p);
            }
            if !chemin.is_empty() {
                unc.push('\\');
                unc.push_str(&chemin.replace('/', "\\"));
            }
            unc
        }
        Sorte::Smb | Sorte::Zima => {
            // « http://192.168.0.18/ » collé depuis le navigateur : seul le nom du serveur compte
            let sans = ["http://", "https://"].iter().find_map(|p| a.get(..p.len()).filter(|d| d.eq_ignore_ascii_case(p)).map(|_| &a[p.len()..])).unwrap_or(a);
            format!("\\\\{}", sans.trim_start_matches('\\').trim_end_matches(['/', '\\']).replace('/', "\\"))
        }
    }
}

/// Nom du serveur dans un chemin réseau (`\\zima\Media` → `zima`)
pub fn hote(unc: &str) -> Option<String> {
    let r = unc.strip_prefix("\\\\")?;
    Some(r.split('\\').next().unwrap_or(r).to_string())
}

/// Lieux du volet pour les stockages ajoutés. Réglage `experimental` faux : SFTP et WebDAV sont cachés (ils restent
/// dans le fichier de réglages) ; `ajoute` garde le rang dans les réglages, trous compris.
pub fn lieux_ajoutes(stockages: &[Stockage], experimental: bool) -> (Vec<Lieu>, Vec<Lieu>) {
    let (mut cloud, mut serveurs) = (Vec::new(), Vec::new());
    for (i, s) in stockages.iter().enumerate().filter(|(_, s)| crate::reglages::sorte_permise(experimental, s.sorte)) {
        let c = chemin(s.sorte, &s.adresse);
        let lieu = Lieu {
            nom: casse_de_marque(&s.nom),
            emplacement: Emplacement::dossier(&c),
            icone: if s.sorte.est_serveur() { Icone::Serveur } else { Icone::Nuage },
            ajoute: Some(i),
            detail: c,
        };
        if s.sorte.est_serveur() {
            serveurs.push(lieu);
        } else {
            cloud.push(lieu);
        }
    }
    (cloud, serveurs)
}

/// Noms habituels d'un serveur ZimaOS / CasaOS sur le réseau local
const NOMS_ZIMA: [&str; 4] = ["zimaos", "zimaos.local", "casaos", "casaos.local"];

/// Valeur texte de `"cle": "…"` dans un petit JSON (sans analyseur complet)
fn json_texte(json: &str, cle: &str) -> Option<String> {
    let i = json.find(&format!("\"{cle}\""))?;
    let reste = json[i + cle.len() + 2..].trim_start().strip_prefix(':')?.trim_start().strip_prefix('"')?;
    Some(reste[..reste.find('"')?].to_string())
}

/// Adresse IP du NAS lue dans la configuration du client Zima, avec le JSON lu (pour le nom de l'appareil)
pub fn zima_client_ip() -> Option<(String, String)> {
    let json = std::fs::read_to_string(format!(r"{}\Zima\config.json", std::env::var("APPDATA").ok()?)).ok()?;
    let ips = json_texte(&json, "lan_ipv4").or_else(|| json_texte(&json, "remote_ipv4"))?;
    // Plusieurs adresses possibles : la première est celle du réseau local, la suivante l'accès distant
    let ip = ips.split_whitespace().find(|ip| ip.parse::<std::net::Ipv4Addr>().is_ok())?.to_string();
    Some((ip, json))
}

/// Le client Zima (application ZimaOS pour Windows) connaît le NAS : son adresse sur le réseau local
/// est dans `%APPDATA%\Zima\config.json`. Le serveur apparaît alors tout seul dans SERVEURS.
/// (Les identifiants SMB restent ceux que Windows a déjà mémorisés : on n'y touche pas.)
pub fn zima_client() -> Option<Lieu> {
    let (ip, json) = zima_client_ip()?;
    let nom = json_texte(&json, "device_name").filter(|n| !n.trim().is_empty()).unwrap_or_else(|| "ZimaOS".into());
    let chemin = format!("\\\\{ip}");
    Some(Lieu { nom: casse_de_marque(&nom), emplacement: Emplacement::dossier(&chemin), icone: Icone::Serveur, ajoute: None, detail: format!("CLIENT ZIMA · {chemin}") })
}

/// Adresse vide pour un serveur ZimaOS : on cherche les noms habituels sur le réseau
pub fn trouver_zima() -> Option<String> {
    use std::net::ToSocketAddrs;
    NOMS_ZIMA.into_iter().find(|n| (*n, 445).to_socket_addrs().is_ok_and(|mut a| a.next().is_some())).map(str::to_string)
}

fn message_erreur(code: WIN32_ERROR) -> String {
    match code {
        ERROR_LOGON_FAILURE | ERROR_ACCESS_DENIED => "IDENTIFIANT OU MOT DE PASSE INCORRECT".into(),
        ERROR_BAD_NETPATH | ERROR_BAD_NET_NAME => "SERVEUR OU PARTAGE INTROUVABLE".into(),
        ERROR_NO_NET_OR_BAD_PATH | ERROR_NO_NETWORK => "RESEAU INDISPONIBLE".into(),
        ERROR_SESSION_CREDENTIAL_CONFLICT => "DEJA CONNECTE AVEC UN AUTRE IDENTIFIANT (DECONNECTEZ-VOUS D'ABORD)".into(),
        ERROR_BAD_NET_RESP | ERROR_UNEXP_NET_ERR => "LE SERVEUR NE REPOND PAS".into(),
        e => format!("ERREUR RESEAU {}", e.0),
    }
}

/// Mémorise le mot de passe dans le Gestionnaire d'identification (comme `cmdkey /add`) :
/// Windows s'en sert ensuite tout seul pour ce serveur, même dans l'Explorateur
fn memoriser(hote: &str, utilisateur: &str, mot_de_passe: &[u16]) -> Result<(), String> {
    let mut cible = wide(hote);
    let mut nom = wide(utilisateur);
    let mut secret: Vec<u8> = mot_de_passe.iter().flat_map(|c| c.to_le_bytes()).collect();
    let cred = CREDENTIALW {
        Type: CRED_TYPE_DOMAIN_PASSWORD,
        TargetName: PWSTR(cible.as_mut_ptr()),
        CredentialBlobSize: secret.len() as u32,
        CredentialBlob: secret.as_mut_ptr(),
        Persist: CRED_PERSIST_ENTERPRISE,
        UserName: PWSTR(nom.as_mut_ptr()),
        ..Default::default()
    };
    let r = unsafe { CredWriteW(&cred, 0) };
    secret.iter_mut().for_each(|o| *o = 0);
    r.map_err(|e| format!("IDENTIFIANT NON ENREGISTRE ({})", e.code().0 & 0xFFFF))
}

/// Oublie le mot de passe mémorisé pour ce serveur
pub fn oublier(hote: &str) {
    unsafe {
        let _ = CredDeleteW(&HSTRING::from(hote), CRED_TYPE_DOMAIN_PASSWORD, 0);
    }
}

/// Vérifie qu'un stockage répond et ouvre la session réseau (fil séparé : peut prendre plusieurs secondes).
/// Renvoie l'adresse retenue (ZimaOS trouvé sur le réseau si elle était vide).
pub fn verifier(sorte: Sorte, adresse: &str, utilisateur: &str, mot_de_passe: &[u16]) -> Result<String, String> {
    let adresse = if adresse.trim().is_empty() && sorte == Sorte::Zima {
        trouver_zima().ok_or("AUCUN SERVEUR ZIMAOS TROUVE SUR LE RESEAU : TAPEZ SON ADRESSE")?
    } else {
        adresse.trim().to_string()
    };
    if adresse.is_empty() {
        return Err("ADRESSE MANQUANTE".into());
    }
    // SFTP : l'utilisateur fait partie de l'adresse ; la clé SSH suffit, pas de mot de passe
    if sorte == Sorte::Sftp {
        if !mot_de_passe.is_empty() {
            return Err("SFTP : CLE SSH SEULEMENT, PAS DE MOT DE PASSE".into());
        }
        let a = chemin(sorte, &adresse);
        let (mut hote, chemin_distant) = crate::sftp::decouper(&a).ok_or("ADRESSE INVALIDE")?;
        if !utilisateur.trim().is_empty() && !hote.contains('@') {
            hote = format!("{}@{hote}", utilisateur.trim());
        }
        crate::sftp::lire(&hote, &chemin_distant)?;
        return Ok(crate::sftp::adresse(&hote, &chemin_distant));
    }
    let unc = chemin(sorte, &adresse);
    if sorte == Sorte::Dossier {
        return if std::path::Path::new(&unc).is_dir() { Ok(adresse) } else { Err("DOSSIER INTROUVABLE".into()) };
    }
    let serveur = hote(&unc).ok_or("ADRESSE INVALIDE")?;
    // Session vers le serveur : un partage précis, sinon IPC$ (sert à lister les partages)
    let distant = if unc.trim_start_matches('\\').contains('\\') { unc.clone() } else { format!("{unc}\\IPC$") };
    let mut distant_w = wide(&distant);
    let ressource = NETRESOURCEW { dwType: RESOURCETYPE_ANY, lpRemoteName: PWSTR(distant_w.as_mut_ptr()), ..Default::default() };
    let mdp: Vec<u16> = mot_de_passe.iter().copied().chain(std::iter::once(0)).collect();
    let user = wide(utilisateur);
    let r = unsafe {
        WNetAddConnection2W(
            &ressource,
            if mot_de_passe.is_empty() { PCWSTR::null() } else { PCWSTR(mdp.as_ptr()) },
            if utilisateur.is_empty() { PCWSTR::null() } else { PCWSTR(user.as_ptr()) },
            NET_CONNECT_FLAGS(0),
        )
    };
    let mut mdp = mdp;
    mdp.iter_mut().for_each(|c| *c = 0);
    match r {
        // Mémorisé seulement une fois accepté : un mot de passe faux n'écrase pas un identifiant valide
        NO_ERROR | ERROR_ALREADY_ASSIGNED => {
            if !mot_de_passe.is_empty() || !utilisateur.is_empty() {
                memoriser(&serveur, utilisateur, mot_de_passe)?;
            }
            Ok(adresse)
        }
        e => Err(message_erreur(e)),
    }
}

/// Partages d'un serveur (dossiers partagés, sans les partages d'administration `C$`, `IPC$`…)
pub fn partages(hote: &str) -> Result<Vec<Entree>, String> {
    // WebDAV n'a pas de liste de partages : il faut un chemin
    if hote.contains('@') {
        return Err("AJOUTEZ LE CHEMIN COMPLET DU DOSSIER WEBDAV".into());
    }
    let serveur = format!("\\\\{hote}");
    let mut v = Vec::new();
    unsafe {
        let mut buf: *mut u8 = std::ptr::null_mut();
        let (mut lus, mut total) = (0u32, 0u32);
        let r = NetShareEnum(&HSTRING::from(serveur.as_str()), 1, &mut buf, u32::MAX, &mut lus, &mut total, None);
        if r != NO_ERROR.0 && r != ERROR_MORE_DATA.0 {
            return Err(message_erreur(WIN32_ERROR(r)));
        }
        if !buf.is_null() {
            let infos = std::slice::from_raw_parts(buf as *const SHARE_INFO_1, lus as usize);
            for i in infos {
                if i.shi1_type.0 & 0xFF != STYPE_DISKTREE.0 {
                    continue;
                }
                let Ok(nom) = i.shi1_netname.to_string() else { continue };
                if nom.ends_with('$') {
                    continue;
                }
                // Espace libre de chaque partage (ZimaOS, NAS) : montré comme un disque, en points
                let racine = format!("{serveur}\\{nom}");
                let (mut libre, mut total) = (0u64, 0u64);
                let mut e = dossier::entree_dossier(&nom, &serveur);
                if windows::Win32::Storage::FileSystem::GetDiskFreeSpaceExW(&HSTRING::from(format!("{racine}\\")), Some(&mut libre), Some(&mut total), None).is_ok() && total > 0 {
                    e.genre = Genre::Disque { racine, libre, total };
                    e.taille = total;
                }
                v.push(e);
            }
            NetApiBufferFree(Some(buf as *const _));
        }
    }
    Ok(v)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chemins() {
        assert_eq!(chemin(Sorte::Zima, "zimaos"), r"\\zimaos");
        assert_eq!(chemin(Sorte::Zima, "http://192.168.0.18/"), r"\\192.168.0.18");
        assert_eq!(chemin(Sorte::Smb, r"\\nas\Media\"), r"\\nas\Media");
        assert_eq!(chemin(Sorte::Smb, "192.168.0.20/photos"), r"\\192.168.0.20\photos");
        assert_eq!(chemin(Sorte::WebDav, "https://cloud.fr/remote.php/dav/files/nico"), r"\\cloud.fr@SSL\remote.php\dav\files\nico");
        assert_eq!(chemin(Sorte::WebDav, "http://nas:8080/dav"), r"\\nas@8080\dav");
        assert_eq!(hote(r"\\zima\Media"), Some("zima".into()));
        assert_eq!(nom_cloud("OneDrive - Personal"), "OneDrive");
        assert_eq!(nom_cloud("OneDrive - Ecole"), "OneDrive · Ecole");
        assert_eq!(json_chemin(r#"{"personal": {"path": "C:\\Users\\n\\Dropbox", "host": 1}}"#), Some(r"C:\Users\n\Dropbox".into()));
    }

    fn stockage(nom: &str, sorte: Sorte, adresse: &str) -> Stockage {
        Stockage { nom: nom.into(), sorte, adresse: adresse.into(), utilisateur: String::new() }
    }

    #[test]
    fn volet_cache_sftp_et_webdav_sans_toucher_au_smb() {
        let tous = [
            stockage("nas", Sorte::Smb, "nas"),
            stockage("serveur", Sorte::Sftp, "nico@serveur/home"),
            stockage("cloud", Sorte::WebDav, "https://cloud.fr/dav"),
            stockage("drive", Sorte::Dossier, r"G:\Mon Drive"),
            stockage("zima", Sorte::Zima, "zimaos"),
        ];
        let noms = |(c, s): (Vec<Lieu>, Vec<Lieu>)| c.into_iter().chain(s).map(|l| (l.nom, l.ajoute)).collect::<Vec<_>>();
        // Gelé : SFTP (rang 1) et WebDAV (rang 2) cachés, les rangs des autres gardés pour retrouver le réglage
        let gele = noms(lieux_ajoutes(&tous, false));
        assert!(gele.contains(&("nas".into(), Some(0))) && gele.contains(&("drive".into(), Some(3))) && gele.contains(&("zima".into(), Some(4))));
        assert!(!gele.iter().any(|(n, _)| n == "SERVEUR" || n == "CLOUD"), "{gele:?}");
        // Réglage vrai : les cinq, comme avant le gel
        assert_eq!(noms(lieux_ajoutes(&tous, true)).len(), 5);
    }


    #[test]
    fn noms_de_stockage_en_casse_de_marque() {
        assert_eq!(casse_de_marque("ONEDRIVE"), "OneDrive");
        assert_eq!(casse_de_marque("zimaos"), "ZimaOS");
        assert_eq!(casse_de_marque("GOOGLE DRIVE"), "Google Drive");
        assert_eq!(casse_de_marque("dropbox"), "Dropbox");
        assert_eq!(casse_de_marque("ICLOUD"), "iCloud");
        assert_eq!(casse_de_marque("iCloud Drive"), "iCloud Drive");
        // Un mot entier seulement ; le reste garde la casse tapée
        assert_eq!(casse_de_marque("monzimaos"), "monzimaos");
        assert_eq!(casse_de_marque("  NAS salon  "), "NAS salon");
        assert_eq!(casse_de_marque("ZIMAOS et DROPBOX"), "ZimaOS et Dropbox");
        assert_eq!(nom_cloud("Dropbox"), "Dropbox");
        assert_eq!(nom_cloud("OneDrive - Personnel"), "OneDrive");
    }
    #[test]
    fn emplacements_reseau() {
        assert_eq!(Emplacement::dossier(r"\\zima"), Emplacement::Serveur("zima".into()));
        assert_eq!(Emplacement::dossier(r"\\zima\Media").parent(), Some(Emplacement::Serveur("zima".into())));
        let segs = Emplacement::dossier(r"\\zima\Media\Films").segments();
        assert_eq!(segs.iter().map(|s| s.0.as_str()).collect::<Vec<_>>(), ["ZIMA", "MEDIA", "FILMS"]);
        assert_eq!(segs[1].1, Emplacement::Dossier(r"\\zima\Media".into()));
    }
}
