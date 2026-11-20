// Prototype jetable du terminal (T0, PLAN-TERMINAL) : une session ConPTY pilotée sans interface, pour mesurer ce que le plan
// ne sait pas encore (le drapeau « passthrough » réellement accepté, les séquences OSC 133 / 9;9 qui arrivent ou non).
// Compilé seulement pour les tests : rien de ceci n'entre dans l'exe tant que T1 n'a pas décidé de la forme définitive.

use std::ffi::c_void;
use std::time::{Duration, Instant};

use windows::core::PWSTR;
use windows::Win32::Foundation::{CloseHandle, HANDLE, WAIT_OBJECT_0};
use windows::Win32::Storage::FileSystem::{ReadFile, WriteFile};
use windows::Win32::System::Console::{ClosePseudoConsole, CreatePseudoConsole, COORD, HPCON};
use windows::Win32::System::Pipes::CreatePipe;
use windows::Win32::System::Threading::{
    CreateProcessW, DeleteProcThreadAttributeList, InitializeProcThreadAttributeList, TerminateProcess, UpdateProcThreadAttribute, WaitForSingleObject,
    EXTENDED_STARTUPINFO_PRESENT, LPPROC_THREAD_ATTRIBUTE_LIST, PROCESS_INFORMATION, STARTUPINFOEXW,
};

/// Attribut qui rattache le processus fils à la pseudo-console (PROC_THREAD_ATTRIBUTE_PSEUDOCONSOLE de `winbase.h`)
const ATTRIBUT_PSEUDOCONSOLE: usize = 0x0002_0016;

/// Résultat d'une session : octets reçus, et si le fils s'est terminé de lui-même
pub struct Flux {
    pub octets: Vec<u8>,
    pub fini_seul: bool,
}

fn large(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// Lance `ligne_de_commande` dans une pseudo-console créée avec `drapeaux`, envoie `saisie` au clavier du fils, lit tout ce
/// qui sort jusqu'à la fin du fils (ou `delai`). Erreur = texte lisible (création refusée, etc.).
pub fn lancer(ligne_de_commande: &str, drapeaux: u32, saisie: &[u8], delai: Duration, attente_apres_fin: Duration) -> Result<Flux, String> {
    unsafe {
        let (mut entree_lue, mut entree_ecrite, mut sortie_lue, mut sortie_ecrite) = (HANDLE::default(), HANDLE::default(), HANDLE::default(), HANDLE::default());
        CreatePipe(&mut entree_lue, &mut entree_ecrite, None, 0).map_err(|e| format!("CreatePipe (entrée) : {e}"))?;
        CreatePipe(&mut sortie_lue, &mut sortie_ecrite, None, 0).map_err(|e| format!("CreatePipe (sortie) : {e}"))?;
        // 120 colonnes x 30 lignes : assez large pour qu'une invite ne soit pas coupée
        let pc: HPCON = match CreatePseudoConsole(COORD { X: 120, Y: 30 }, entree_lue, sortie_ecrite, drapeaux) {
            Ok(p) => p,
            Err(e) => {
                for h in [entree_lue, entree_ecrite, sortie_lue, sortie_ecrite] {
                    let _ = CloseHandle(h);
                }
                return Err(format!("CreatePseudoConsole(drapeaux {drapeaux:#x}) refusé : {e}"));
            }
        };
        // Côtés confiés à la pseudo-console : on ne garde que nos deux extrémités
        let _ = CloseHandle(entree_lue);
        let _ = CloseHandle(sortie_ecrite);

        // Liste d'attributs portant la pseudo-console
        let mut taille = 0usize;
        let _ = InitializeProcThreadAttributeList(LPPROC_THREAD_ATTRIBUTE_LIST::default(), 1, 0, &mut taille);
        let mut tampon = vec![0u8; taille];
        let liste = LPPROC_THREAD_ATTRIBUTE_LIST(tampon.as_mut_ptr() as *mut c_void);
        InitializeProcThreadAttributeList(liste, 1, 0, &mut taille).map_err(|e| format!("InitializeProcThreadAttributeList : {e}"))?;
        UpdateProcThreadAttribute(liste, 0, ATTRIBUT_PSEUDOCONSOLE, Some(pc.0 as *const c_void), std::mem::size_of::<HPCON>(), None, None)
            .map_err(|e| format!("UpdateProcThreadAttribute : {e}"))?;

        let mut si = STARTUPINFOEXW::default();
        si.StartupInfo.cb = std::mem::size_of::<STARTUPINFOEXW>() as u32;
        si.lpAttributeList = liste;
        // Constat de T0 : sans ceci, un fils lancé depuis un processus qui a une console (cargo test, un terminal) écrit dans la
        // console du PARENT et non dans la pseudo-console (sa bannière apparaissait dans la sortie de `cargo test`). Poignées
        // standard déclarées vides = le fils prend celles de la pseudo-console (même précaution que wezterm). Sans effet pour
        // SpaceFiles (sous-système « windows », sans console), mais nécessaire pour les tests.
        si.StartupInfo.dwFlags = windows::Win32::System::Threading::STARTF_USESTDHANDLES;
        let mut commande = large(ligne_de_commande);
        let mut pi = PROCESS_INFORMATION::default();
        let cree = CreateProcessW(None, PWSTR(commande.as_mut_ptr()), None, None, false, EXTENDED_STARTUPINFO_PRESENT, None, None, &si.StartupInfo, &mut pi);
        DeleteProcThreadAttributeList(liste);
        cree.map_err(|e| format!("CreateProcessW({ligne_de_commande}) : {e}"))?;

        // Lecture dans un fil : sans elle, un fils qui écrit beaucoup bloquerait, et ClosePseudoConsole attend la fin de la sortie
        let lecture = sortie_lue.0 as isize;
        let fil = std::thread::spawn(move || {
            let h = HANDLE(lecture as *mut c_void);
            let mut recu = Vec::new();
            let mut tampon = [0u8; 8192];
            loop {
                let mut n = 0u32;
                if ReadFile(h, Some(&mut tampon), Some(&mut n), None).is_err() || n == 0 {
                    break;
                }
                recu.extend_from_slice(&tampon[..n as usize]);
            }
            recu
        });

        if !saisie.is_empty() {
            let mut ecrits = 0u32;
            let _ = WriteFile(entree_ecrite, Some(saisie), Some(&mut ecrits), None);
        }

        let debut = Instant::now();
        let fini_seul = WaitForSingleObject(pi.hProcess, delai.as_millis() as u32) == WAIT_OBJECT_0;
        if !fini_seul {
            let _ = TerminateProcess(pi.hProcess, 1);
        }
        let _ = debut;
        let mut code = 0u32;
        let _ = windows::Win32::System::Threading::GetExitCodeProcess(pi.hProcess, &mut code);
        println!("T0 code de sortie de « {ligne_de_commande} » : {code:#x} (259 = encore en vie), fini seul {fini_seul}");
        // Attente avant de fermer la pseudo-console : ConPTY dessine l'écran en différé, on mesure si fermer tout de suite après la
        // fin du fils perd le dernier texte (voir `mesure_drapeaux_et_osc`, résultats dans docs/terminal/T0-mesures.md).
        std::thread::sleep(attente_apres_fin);
        // Ferme la pseudo-console : la sortie se vide puis se termine, le fil de lecture s'arrête
        ClosePseudoConsole(pc);
        let _ = CloseHandle(entree_ecrite);
        let octets = fil.join().unwrap_or_default();
        let _ = CloseHandle(sortie_lue);
        let _ = CloseHandle(pi.hProcess);
        let _ = CloseHandle(pi.hThread);
        Ok(Flux { octets, fini_seul })
    }
}

/// Nombre d'occurrences d'une séquence dans un flux
pub fn compter(flux: &[u8], motif: &[u8]) -> usize {
    flux.windows(motif.len()).filter(|f| *f == motif).count()
}

/// Flux rendu lisible : ESC devient `␛`, BEL `␇`, le reste des contrôles en `\xNN`
pub fn lisible(flux: &[u8]) -> String {
    let mut s = String::new();
    for &o in flux {
        match o {
            0x1b => s.push('␛'),
            0x07 => s.push('␇'),
            b'\r' => s.push_str("\\r"),
            b'\n' => s.push_str("\\n\n"),
            0x20..=0x7e => s.push(o as char),
            0x80..=0xff => s.push(o as char),
            _ => s.push_str(&format!("\\x{o:02x}")),
        }
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Critère d'acceptation de T0 : un test d'intégration lit « ok » rendu par un vrai shell à travers ConPTY (cmd interactif).
    /// Le premier essai échouait parce que `STARTF_USESTDHANDLES` manquait (le fils écrivait dans la console du test) : ce n'était
    /// pas une perte de sortie par ConPTY. La question « fils court, fermeture immédiate » est mesurée à part (`mesure_drapeaux_et_osc`).
    #[test]
    fn cmd_echo_ok() {
        let flux = lancer("cmd.exe /q", 0, b"echo ok\r\n", Duration::from_millis(2000), Duration::from_millis(300)).expect("session ConPTY");
        let texte = String::from_utf8_lossy(&flux.octets).into_owned();
        println!("T0 flux reçu ({} octets) : {}", flux.octets.len(), lisible(&flux.octets));
        assert!(texte.contains("ok"), "« ok » absent du flux : {texte:?}");
    }

    /// Mesure (b) et (c) : pour chaque drapeau candidat et chaque shell, la création est-elle acceptée, et les séquences
    /// OSC 133 (marque d'invite) et OSC 9;9 (dossier courant) émises par le shell arrivent-elles dans le flux ?
    /// Lancer avec `--ignored --nocapture` ; `T0_SORTIE` = dossier où écrire les flux bruts (sinon le dossier temporaire).
    #[test]
    #[ignore]
    fn mesure_drapeaux_et_osc() {
        let sortie = std::env::var("T0_SORTIE").map(std::path::PathBuf::from).unwrap_or_else(|_| std::env::temp_dir().join("spacefiles-t0"));
        std::fs::create_dir_all(&sortie).unwrap();

        // Invite qui émet OSC 133;A (début d'invite) puis OSC 9;9 (dossier courant), puis un `cd` et la sortie
        let saisie_cmd: &[u8] = b"prompt $E]133;A$E\\$E]9;9;$P$E\\$P$G\r\ncd \\\r\necho bonjour\r\n";
        let saisie_ps = "function prompt { \"$([char]27)]133;A$([char]27)\\$([char]27)]9;9;$($PWD.Path)$([char]27)\\PS $($PWD.Path)> \" }\r\ncd \\\r\nWrite-Host 'bonjour' -ForegroundColor Green\r\n";

        // Constat à garder : un fils qui meurt aussitôt (`cmd /c echo ok`) perd-il son texte si on ferme sans attendre ?
        for attente in [0u64, 1000] {
            if let Ok(f) = lancer("cmd.exe /c echo ok", 0, b"", Duration::from_secs(15), Duration::from_millis(attente)) {
                println!("MESURE T0 cmd /c echo ok (attente {attente} ms avant fermeture) : « ok » présent = {}, {} octets", compter(&f.octets, b"ok") > 0, f.octets.len());
            }
        }

        let cas: [(&str, &str, &[u8]); 2] = [("cmd", "cmd.exe", saisie_cmd), ("powershell", "powershell.exe -NoLogo -NoProfile", saisie_ps.as_bytes())];
        for (nom, commande, saisie) in cas {
            for drapeaux in [0u32, 0x8, 0x10, 0x18] {
                let etiquette = format!("{nom}-drapeau-{drapeaux:#x}");
                match lancer(commande, drapeaux, saisie, Duration::from_millis(if nom == "cmd" { 2500 } else { 6000 }), Duration::from_millis(500)) {
                    Ok(f) => {
                        let osc133 = compter(&f.octets, b"\x1b]133;");
                        let osc99 = compter(&f.octets, b"\x1b]9;9;");
                        let couleur = compter(&f.octets, b"\x1b[");
                        println!(
                            "MESURE T0 {etiquette} : accepté, {} octets, fini seul {}, OSC 133 = {osc133}, OSC 9;9 = {osc99}, séquences CSI = {couleur}",
                            f.octets.len(),
                            f.fini_seul
                        );
                        std::fs::write(sortie.join(format!("{etiquette}.bin")), &f.octets).unwrap();
                        std::fs::write(sortie.join(format!("{etiquette}.txt")), lisible(&f.octets)).unwrap();
                    }
                    Err(e) => println!("MESURE T0 {etiquette} : {e}"),
                }
            }
        }
    }
}
