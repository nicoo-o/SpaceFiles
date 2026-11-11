// Manifeste de SpaceFiles : `%LOCALAPPDATA%\Eco\apps\spacefiles.json` (schéma : `docs/ecosystem/manifest.schema.json`).
// Écrit par le résident à son démarrage, une fois le tube créé ; jamais supprimé à l'arrêt (le Hub affiche « arrêté »).
// Mode essai (`SPACEFILES_PIPE`) : dans le dossier Eco du faux profil, jamais dans le vrai (voir `dossier_eco`).
// Le chemin de l'exe y figure : un client s'en sert pour reconnaître le processus qui sert le tube (`client.rs`).

use std::path::Path;

use super::json::echapper;
use super::trame::{CAPACITES, PROTOCOLE};
use crate::journal::Niveau;

/// Le document du manifeste ; `tube` est le nom sans préfixe (`Eco.spacefiles.<utilisateur>`)
pub fn contenu(exe: &str, version: &str, tube: &str) -> String {
    let capacites: Vec<String> = CAPACITES.iter().map(|c| echapper(c)).collect();
    format!(
        "{{\n  \"id\": {},\n  \"name\": \"SpaceFiles\",\n  \"version\": {},\n  \"exe\": {},\n  \"pipe\": {},\n  \"protocol\": {PROTOCOLE},\n  \"capabilities\": [{}],\n  \"settingsLink\": \"spacefiles://settings\"\n}}\n",
        echapper(super::APP),
        echapper(version),
        echapper(exe),
        echapper(tube),
        capacites.join(", ")
    )
}

fn ecrire_dans(dossier_eco: &Path, exe: &str, version: &str, tube: &str) -> std::io::Result<()> {
    super::ecrire_atomique(&dossier_eco.join("apps").join(format!("{}.json", super::APP)), &contenu(exe, version, tube))
}

/// Écrit le manifeste du résident. Le journal dit seulement ce qui s'est passé (ni chemin ni nom de fichier).
pub fn ecrire_au_demarrage() {
    // Mode portable : tout reste dans le dossier de l'exe ; rien n'est publié dans le profil de l'utilisateur
    if crate::reglages::dossier_portable().is_some() {
        super::trace(Niveau::Info, "eco", "manifeste non ecrit (mode portable)");
        return;
    }
    let Some(dossier) = super::dossier_eco() else {
        super::trace(Niveau::Avert, "eco", "manifeste non ecrit (dossier Eco introuvable, ou essai sans faux profil)");
        return;
    };
    let Ok(exe) = std::env::current_exe() else { return };
    match ecrire_dans(&dossier, &exe.to_string_lossy(), env!("CARGO_PKG_VERSION"), &super::nom_court_courant(super::APP)) {
        Ok(()) => super::trace(Niveau::Info, "eco", "manifeste ecrit"),
        Err(e) => super::trace(Niveau::Avert, "eco", &format!("manifeste non ecrit ({:?})", e.kind())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::eco::json::{analyser_json, champ, Json};
    use crate::eco::ok;

    const SCHEMA: &str = include_str!("../../docs/ecosystem/manifest.schema.json");

    /// Valide un document contre le schéma, sans crate JSON Schema : champs `required`, `type` de chaque propriété
    /// présente, `pattern` de `id`, `minimum` de `protocol`, éléments de `capabilities`
    fn valider(doc: &str) -> Result<(), String> {
        let Some(Json::Objet(schema)) = analyser_json(SCHEMA) else { return Err("schema illisible".into()) };
        let Some(Json::Objet(manifeste)) = analyser_json(doc) else { return Err("manifeste illisible".into()) };
        let Some(Json::Liste(requis)) = champ(&schema, "required") else { return Err("pas de required".into()) };
        for r in requis {
            let Json::Texte(nom) = r else { return Err("required mal forme".into()) };
            if champ(&manifeste, nom).is_none() {
                return Err(format!("champ requis absent : {nom}"));
            }
        }
        let Some(Json::Objet(props)) = champ(&schema, "properties") else { return Err("pas de properties".into()) };
        for (nom, valeur) in &manifeste {
            let Some(Json::Objet(p)) = champ(props, nom) else { continue };
            let Some(Json::Texte(genre)) = champ(p, "type") else { continue };
            let bon = match (genre.as_str(), valeur) {
                ("string", Json::Texte(_)) | ("object", Json::Objet(_)) | ("array", Json::Liste(_)) => true,
                ("integer", Json::Nombre(n)) => n.fract() == 0.0,
                _ => false,
            };
            if !bon {
                return Err(format!("type faux : {nom} devrait etre {genre}"));
            }
            if let (Some(Json::Texte(motif)), Json::Texte(t)) = (champ(p, "pattern"), valeur) {
                let re = regex::Regex::new(motif).map_err(|e| e.to_string())?;
                if !re.is_match(t) {
                    return Err(format!("{nom} ne respecte pas {motif}"));
                }
            }
            if let (Some(Json::Nombre(min)), Json::Nombre(n)) = (champ(p, "minimum"), valeur) {
                if n < min {
                    return Err(format!("{nom} sous le minimum"));
                }
            }
            if let (Some(Json::Objet(item)), Json::Liste(l)) = (champ(p, "items"), valeur) {
                if champ(item, "type") == Some(&Json::Texte("string".into())) && l.iter().any(|e| !matches!(e, Json::Texte(_))) {
                    return Err(format!("{nom} : un element n'est pas une chaine"));
                }
            }
        }
        Ok(())
    }

    #[test]
    fn manifeste_conforme_au_schema() {
        let doc = contenu(r"C:\Users\x\AppData\Local\Programs\SpaceFiles\spacefiles.exe", "0.1.0", "Eco.spacefiles.x");
        assert_eq!(valider(&doc), Ok(()), "{doc}");
        let Some(Json::Objet(m)) = analyser_json(&doc) else { panic!("illisible") };
        assert_eq!(champ(&m, "id"), Some(&Json::Texte("spacefiles".into())));
        assert_eq!(champ(&m, "name"), Some(&Json::Texte("SpaceFiles".into())));
        assert_eq!(champ(&m, "protocol"), Some(&Json::Nombre(1.0)));
        assert_eq!(champ(&m, "pipe"), Some(&Json::Texte("Eco.spacefiles.x".into())));
        assert_eq!(champ(&m, "settingsLink"), Some(&Json::Texte("spacefiles://settings".into())));
        assert_eq!(champ(&m, "capabilities"), Some(&Json::Liste(vec![Json::Texte("search.query".into()), Json::Texte("explorer.open".into())])));
        // Les capacités du manifeste sont celles de `hello`
        assert!(crate::eco::trame::reponse_hello("1").contains("\"capabilities\":[\"search.query\",\"explorer.open\"]"));
    }

    /// Le validateur à la main refuse bien ce que le schéma interdit (sinon le test ci-dessus ne prouverait rien)
    #[test]
    fn le_validateur_refuse_un_manifeste_faux() {
        assert!(valider("{\"id\":\"spacefiles\"}").is_err(), "champs requis absents");
        let sans_exe = "{\"id\":\"a\",\"name\":\"A\",\"version\":\"1\",\"pipe\":\"p\",\"protocol\":1}";
        assert!(valider(sans_exe).unwrap_err().contains("exe"));
        let base = |id: &str, protocole: &str| format!("{{\"id\":{id},\"name\":\"A\",\"version\":\"1\",\"exe\":\"x\",\"pipe\":\"p\",\"protocol\":{protocole}}}");
        assert_eq!(valider(&base("\"a-b\"", "1")), Ok(()));
        assert!(valider(&base("\"Majuscule\"", "1")).is_err(), "id hors du motif");
        assert!(valider(&base("\"a\"", "0")).is_err(), "protocol sous le minimum");
        assert!(valider(&base("\"a\"", "1.5")).is_err(), "protocol non entier");
        assert!(valider(&base("3", "1")).is_err(), "id non chaine");
    }

    #[test]
    fn ecriture_dans_un_dossier_jetable() {
        let d = std::env::temp_dir().join(format!("eco-manifeste-{}", std::process::id()));
        ok(ecrire_dans(&d, r"C:\x\spacefiles.exe", "0.1.0", "Eco.spacefiles.t"), "ecriture");
        let texte = ok(std::fs::read_to_string(d.join("apps").join("spacefiles.json")), "lecture");
        assert_eq!(valider(&texte), Ok(()));
        // Réécrit à chaque démarrage : un second passage remplace sans erreur
        ok(ecrire_dans(&d, r"C:\y\spacefiles.exe", "0.2.0", "Eco.spacefiles.t"), "seconde ecriture");
        let texte = ok(std::fs::read_to_string(d.join("apps").join("spacefiles.json")), "relecture");
        assert!(texte.contains("0.2.0") && texte.contains(r"C:\\y\\spacefiles.exe"));
        let _ = std::fs::remove_dir_all(&d);
    }
}
