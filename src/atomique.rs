// Écriture atomique d'un fichier de données : l'unique chemin par lequel réglages, index, étiquettes, historique et
// protocole écrivent. Avant : chaque module avait sa variante (temporaire à nom fixe, sans `sync_all`, `rename` non vérifié).

use std::path::Path;

/// Écrit `octets` dans `cible` : fichier temporaire à côté (nom unique : deux écrivains, par exemple le résident et une
/// fenêtre, ne se marchent pas dessus), forcé sur le disque, puis renommé sur la cible. Après une coupure de courant,
/// le fichier est l'ancien ou le nouveau, jamais un fichier vide ou tronqué. Crée le dossier parent au besoin.
pub(crate) fn ecrire(cible: &Path, octets: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    use std::sync::atomic::{AtomicU32, Ordering};
    static N: AtomicU32 = AtomicU32::new(0);
    if let Some(d) = cible.parent() {
        std::fs::create_dir_all(d)?;
    }
    let nom = cible.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let tmp = cible.with_file_name(format!("{nom}.{}.{}.tmp", std::process::id(), N.fetch_add(1, Ordering::Relaxed)));
    let ecrit = std::fs::File::create(&tmp).and_then(|mut f| {
        f.write_all(octets)?;
        f.sync_all()
    });
    if let Err(e) = ecrit {
        let _ = std::fs::remove_file(&tmp);
        return Err(e);
    }
    // Le renommage sur un fichier que quelqu'un lit à cet instant peut être refusé un court instant : quelques essais
    let mut dernier = Ok(());
    for _ in 0..20 {
        dernier = std::fs::rename(&tmp, cible);
        if dernier.is_ok() {
            return Ok(());
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    let _ = std::fs::remove_file(&tmp);
    dernier
}

/// Comme [`ecrire`], mais un échec va au journal au lieu d'être avalé : une donnée qui ne s'enregistre plus
/// (disque plein, dossier verrouillé) doit laisser une trace, sinon l'utilisateur la perd sans le savoir.
pub(crate) fn ecrire_journalise(cible: &Path, octets: &[u8], module: &str) {
    if let Err(e) = ecrire(cible, octets) {
        crate::journal::ecrire(crate::journal::Niveau::Erreur, module, &format!("ecriture impossible de {} : {e}", cible.display()));
    }
}

/// Première ligne d'un fichier texte de données : `#format=N` (A-02). Les anciennes versions l'ignorent (ni `|` ni date).
pub(crate) fn en_tete_format(version: u32) -> String {
    format!("#format={version}\r\n")
}

/// Version annoncée par la première ligne ; `None` pour un fichier d'avant l'en-tête (version 0, toujours lisible)
pub(crate) fn format_du_texte(texte: &str) -> Option<u32> {
    texte.trim_start_matches('\u{feff}').lines().next()?.trim().strip_prefix("#format=")?.trim().parse().ok()
}

/// Le fichier déjà sur disque vient-il d'une version plus récente que `connue` ? Alors on ne l'écrase pas : une version
/// future a pu changer le format, et réécrire en perdrait le contenu. Trace au journal, sans chemin.
pub(crate) fn plus_recent_que_connu(cible: &Path, connue: u32, module: &str) -> bool {
    let Ok(octets) = std::fs::read(cible) else { return false };
    match format_du_texte(&String::from_utf8_lossy(&octets)) {
        Some(v) if v > connue => {
            crate::journal::ecrire(crate::journal::Niveau::Erreur, module, &format!("format {v} plus récent que {connue} : données non enregistrées, mettre SpaceFiles à jour"));
            true
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn en_tete_de_format_se_relit() {
        assert_eq!(format_du_texte(&en_tete_format(1)), Some(1));
        assert_eq!(format_du_texte("\u{feff}#format=7\nx"), Some(7));
        assert_eq!(format_du_texte("1|c:\\a\n"), None);
        assert_eq!(format_du_texte(""), None);
        assert_eq!(format_du_texte("#format=abc"), None);
    }

    fn dossier(nom: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("atomique-{nom}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    #[test]
    fn cree_le_dossier_et_remplace_sans_residu() {
        let d = dossier("remplace");
        let f = d.join("sous").join("x.bin");
        assert!(ecrire(&f, b"un").is_ok());
        assert!(ecrire(&f, b"deux").is_ok());
        assert_eq!(std::fs::read(&f).unwrap(), b"deux");
        let restes = std::fs::read_dir(d.join("sous")).unwrap().flatten().filter(|e| e.file_name().to_string_lossy().ends_with(".tmp")).count();
        assert_eq!(restes, 0, "un temporaire est reste");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn contenu_vide_et_octets_quelconques() {
        let d = dossier("octets");
        let f = d.join("v.bin");
        let tout: Vec<u8> = (0..=255u8).collect();
        assert!(ecrire(&f, &tout).is_ok());
        assert_eq!(std::fs::read(&f).unwrap(), tout);
        assert!(ecrire(&f, b"").is_ok());
        assert_eq!(std::fs::read(&f).unwrap().len(), 0);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn echec_sans_residu_quand_la_cible_est_un_dossier() {
        let d = dossier("echec");
        let cible = d.join("cible");
        std::fs::create_dir_all(&cible).unwrap();
        assert!(ecrire(&cible, b"x").is_err(), "remplacer un dossier doit échouer");
        let restes = std::fs::read_dir(&d).unwrap().flatten().filter(|e| e.file_name().to_string_lossy().ends_with(".tmp")).count();
        assert_eq!(restes, 0, "un temporaire est reste apres l'echec");
        assert!(cible.is_dir(), "la cible d'origine doit rester intacte");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn ecrivains_simultanes_ne_se_marchent_pas_dessus() {
        let d = dossier("concurrent");
        let f = d.join("c.txt");
        let fils: Vec<_> = (0..8)
            .map(|i| {
                let f = f.clone();
                std::thread::spawn(move || {
                    for _ in 0..20 {
                        let _ = ecrire(&f, format!("ecrivain-{i}").as_bytes());
                    }
                })
            })
            .collect();
        for h in fils {
            h.join().unwrap();
        }
        let lu = std::fs::read_to_string(&f).unwrap();
        assert!(lu.starts_with("ecrivain-") && lu.len() == "ecrivain-0".len(), "contenu mélangé : {lu}");
        let _ = std::fs::remove_dir_all(&d);
    }
}
