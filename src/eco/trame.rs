// Enveloppe des échanges Eco (protocole v1, `docs/ecosystem/protocol.md`) : lecture d'une requête, construction des
// réponses et des événements, lecture d'une réponse côté client. Tout est pur (aucun Win32), donc testé sans tube.
// Une ligne = un objet JSON UTF-8 terminé par `\n` ; 4 096 octets au plus, 32 Ko pour `explorer.open` seulement.

use super::json::{analyser_json, champ, echapper, Json};

/// Une ligne (requête ou réponse) fait 4 096 octets au plus, saut de ligne exclu. Des octets et non des caractères :
/// une ligne qui tient en octets tient en caractères, la borne est donc au moins aussi stricte que le protocole.
pub const MAX_TRAME: usize = 4096;
/// `explorer.open` seul peut aller jusqu'à 32 Ko (les chemins sont longs, 32 000 caractères permis)
pub const MAX_OUVRIR: usize = 32 * 1024;
/// Résultats par réponse : 1 à 30, 30 par défaut
const LIMITE_DEFAUT: usize = 30;
/// Un identifiant d'appel plus long est refusé (il est recopié dans la réponse, qui doit rester sous la borne)
const ID_MAX: usize = 64;
/// Version du protocole que nous parlons
pub const PROTOCOLE: u32 = 1;
/// Ce que `hello` annonce : l'ordre est celui de `manifeste.rs`
pub const CAPACITES: [&str; 2] = ["search.query", "explorer.open"];

#[derive(Debug, PartialEq)]
pub enum Appel {
    Hello,
    Recherche { q: String, limite: usize },
    /// `newTab` est lu (un type faux est refusé) mais ignoré. `reutiliser` (`reuse`) : confier le chemin à une fenêtre
    /// SpaceFiles déjà ouverte avant d'en lancer une nouvelle (faux par défaut ; un type faux est refusé)
    Ouvrir { chemin: String, reveler: bool, reutiliser: bool },
}

#[derive(Debug, PartialEq)]
pub enum Erreur {
    TropLong,
    Invalide,
    ChampManquant(&'static str),
    HorsBornes(&'static str),
    Version,
    Inconnu(String),
    /// Trop d'ouvertures de fenêtre en peu de temps (`explorer.open`)
    TropVite,
    /// Le résident ne peut pas traiter la demande maintenant (fenêtre cachée absente, message non posté)
    Interne,
}

impl Erreur {
    pub fn code(&self) -> &'static str {
        match self {
            Erreur::TropLong => "trop_long",
            Erreur::Invalide => "invalide",
            Erreur::ChampManquant(_) => "champ_manquant",
            Erreur::HorsBornes(_) => "hors_bornes",
            Erreur::Version => "version",
            Erreur::Inconnu(_) => "inconnu",
            Erreur::TropVite => "trop_vite",
            Erreur::Interne => "interne",
        }
    }

    fn message(&self) -> String {
        match self {
            Erreur::TropLong => format!("ligne de plus de {MAX_TRAME} octets"),
            Erreur::Invalide => "trame invalide".to_string(),
            Erreur::ChampManquant(c) => format!("champ manquant : {c}"),
            Erreur::HorsBornes(c) => format!("valeur hors bornes : {c}"),
            Erreur::Version => "version du protocole non prise en charge".to_string(),
            // Le nom est borné par la trame (4 096 octets) et échappé à l'écriture
            Erreur::Inconnu(n) => format!("appel inconnu : {}", n.chars().take(40).collect::<String>()),
            Erreur::TropVite => "trop d'ouvertures de fenetre, reessayer plus tard".to_string(),
            Erreur::Interne => "service indisponible".to_string(),
        }
    }
}

/// Lit une ligne de requête : identifiant (vide si illisible) et appel, ou l'erreur à renvoyer. Ne panique jamais.
/// Une ligne de plus de 4 096 octets n'est acceptée que si c'est un `explorer.open` valide et de 32 Ko au plus.
pub fn analyser_trame(ligne: &str) -> (String, Result<Appel, Erreur>) {
    let ligne = ligne.trim_end_matches(['\r', '\n']);
    if ligne.len() > MAX_OUVRIR {
        return (String::new(), Err(Erreur::TropLong));
    }
    let longue = ligne.len() > MAX_TRAME;
    // Une ligne longue n'est permise qu'à `explorer.open` : sans ce nom en toutes lettres, inutile d'analyser 32 Ko
    // (une écriture du nom avec des échappements Unicode est refusée comme trop longue, sans conséquence)
    if longue && !ligne.contains("\"explorer.open\"") {
        return (String::new(), Err(Erreur::TropLong));
    }
    let Some(Json::Objet(o)) = analyser_json(ligne) else {
        return (String::new(), Err(if longue { Erreur::TropLong } else { Erreur::Invalide }));
    };
    // La borne dépend du nom : on le lit d'abord. Une ligne longue qui n'est pas un `explorer.open` est « trop longue »,
    // quelle que soit sa validité par ailleurs (le seul identifiant sûr est alors celui qu'on n'a pas lu).
    if longue && champ(&o, "name") != Some(&Json::Texte("explorer.open".to_string())) {
        return (String::new(), Err(Erreur::TropLong));
    }
    let id = match champ(&o, "id") {
        None => String::new(),
        Some(Json::Texte(t)) if t.len() <= ID_MAX => t.clone(),
        Some(_) => return (String::new(), Err(Erreur::Invalide)),
    };
    let r = (|| {
        match champ(&o, "v") {
            None => {}
            Some(Json::Nombre(n)) if *n == PROTOCOLE as f64 => {}
            Some(Json::Nombre(n)) if *n > PROTOCOLE as f64 => return Err(Erreur::Version),
            Some(_) => return Err(Erreur::Invalide),
        }
        match champ(&o, "kind") {
            None => {}
            Some(Json::Texte(k)) if k == "call" => {}
            Some(_) => return Err(Erreur::Invalide),
        }
        let nom = match champ(&o, "name") {
            Some(Json::Texte(t)) => t.as_str(),
            Some(_) => return Err(Erreur::Invalide),
            None => return Err(Erreur::ChampManquant("name")),
        };
        match nom {
            "hello" => Ok(Appel::Hello),
            "search.query" => {
                let args = args_objet(&o)?;
                let q = match champ(args, "q") {
                    Some(Json::Texte(t)) => t.clone(),
                    Some(_) => return Err(Erreur::Invalide),
                    None => return Err(Erreur::ChampManquant("q")),
                };
                let limite = match champ(args, "limit") {
                    None | Some(Json::Nul) => LIMITE_DEFAUT,
                    Some(Json::Nombre(n)) if n.fract() == 0.0 && (1.0..=LIMITE_DEFAUT as f64).contains(n) => *n as usize,
                    Some(Json::Nombre(_)) => return Err(Erreur::HorsBornes("limit")),
                    Some(_) => return Err(Erreur::Invalide),
                };
                Ok(Appel::Recherche { q, limite })
            }
            "explorer.open" => {
                let args = args_objet(&o)?;
                let chemin = match champ(args, "path") {
                    Some(Json::Texte(t)) => t.clone(),
                    Some(_) => return Err(Erreur::Invalide),
                    None => return Err(Erreur::ChampManquant("path")),
                };
                let booleen = |cle: &str| match champ(args, cle) {
                    None | Some(Json::Nul) => Ok(false),
                    Some(Json::Bool(b)) => Ok(*b),
                    Some(_) => Err(Erreur::Invalide),
                };
                booleen("newTab")?;
                let reveler = booleen("reveal")?;
                Ok(Appel::Ouvrir { chemin, reveler, reutiliser: booleen("reuse")? })
            }
            autre => Err(Erreur::Inconnu(autre.to_string())),
        }
    })();
    (id, r)
}

fn args_objet(o: &[(String, Json)]) -> Result<&[(String, Json)], Erreur> {
    match champ(o, "args") {
        Some(Json::Objet(a)) => Ok(a),
        Some(_) => Err(Erreur::Invalide),
        None => Err(Erreur::ChampManquant("args")),
    }
}

/// Ligne reçue sans son `\n` : un `\r` final (client Windows) n'en fait pas partie
pub fn sans_fin_de_ligne(l: &[u8]) -> &[u8] {
    l.strip_suffix(b"\r").unwrap_or(l)
}

/// Une ligne de plus de 32 Ko : trop longue pour tous les appels (le `\r` final ne compte pas, comme dans `analyser_trame`).
/// Entre 4 096 octets et 32 Ko, c'est `analyser_trame` qui tranche selon le nom de l'appel.
pub fn ligne_trop_longue(l: &[u8]) -> bool {
    sans_fin_de_ligne(l).len() > MAX_OUVRIR
}

// ───────────────────────── Réponses ─────────────────────────

/// Un résultat de recherche : nom, chemin complet, dossier
pub type Resultat = (String, String, String);

fn enveloppe(id: &str, corps: &str) -> String {
    format!("{{\"v\":1,\"id\":{},\"from\":\"spacefiles\",\"kind\":\"reply\",{corps}}}", echapper(id))
}

pub fn reponse_erreur(id: &str, e: &Erreur) -> String {
    reponse_code(id, e.code(), &e.message())
}

pub fn reponse_code(id: &str, code: &str, message: &str) -> String {
    enveloppe(id, &format!("\"error\":{{\"code\":{},\"message\":{}}}", echapper(code), echapper(message)))
}

pub fn reponse_hello(id: &str) -> String {
    let capacites: Vec<String> = CAPACITES.iter().map(|c| echapper(c)).collect();
    enveloppe(
        id,
        &format!("\"result\":{{\"app\":\"spacefiles\",\"version\":{},\"protocol\":{PROTOCOLE},\"capabilities\":[{}]}}", echapper(env!("CARGO_PKG_VERSION")), capacites.join(",")),
    )
}

/// `explorer.open` accepté : la demande est dans la file du résident (la fenêtre n'est pas attendue)
pub fn reponse_ouvert(id: &str) -> String {
    enveloppe(id, "\"result\":{\"opened\":true}")
}

/// Index pas encore prêt : réponse immédiate, le client réessaiera (il se dégrade à 300 ms)
pub fn reponse_chargement(id: &str) -> String {
    enveloppe(id, "\"result\":{\"results\":[],\"loading\":true}")
}

/// Réponse d'une recherche sous `MAX_TRAME` : si elle déborde, on retire des résultats par la fin (`truncated` passe à
/// vrai) ; une trame n'est jamais coupée en deux
pub fn reponse_recherche(id: &str, resultats: &[Resultat], tronque: bool) -> String {
    let construire = |n: usize, tronque: bool| {
        let liste: Vec<String> = resultats[..n]
            .iter()
            .map(|(nom, chemin, dossier)| format!("{{\"name\":{},\"path\":{},\"dir\":{}}}", echapper(nom), echapper(chemin), echapper(dossier)))
            .collect();
        enveloppe(id, &format!("\"result\":{{\"results\":[{}],\"truncated\":{tronque}}}", liste.join(",")))
    };
    let mut n = resultats.len();
    loop {
        let r = construire(n, tronque || n < resultats.len());
        if r.len() <= MAX_TRAME || n == 0 {
            return r;
        }
        n -= 1;
    }
}

// ───────────────────────── Côté client ─────────────────────────

/// Un appel ou un événement envoyé par SpaceFiles ; `args` est un objet JSON déjà écrit (champs échappés par l'appelant)
pub fn trame_sortante(genre: &str, id: &str, nom: &str, args: &str) -> String {
    format!("{{\"v\":{PROTOCOLE},\"id\":{},\"from\":\"spacefiles\",\"kind\":{},\"name\":{},\"args\":{args}}}", echapper(id), echapper(genre), echapper(nom))
}

/// Ce qu'une réponse contient
#[derive(Debug, PartialEq)]
pub enum Corps {
    Resultat(Vec<(String, Json)>),
    Erreur(String),
}

/// Lit une réponse (`kind:"reply"`, `v` absent ou 1). None : pas une réponse du protocole que nous parlons.
pub fn analyser_reponse(ligne: &str) -> Option<(String, Corps)> {
    let Some(Json::Objet(o)) = analyser_json(ligne.trim_end_matches(['\r', '\n'])) else { return None };
    match champ(&o, "v") {
        None => {}
        Some(Json::Nombre(n)) if *n == PROTOCOLE as f64 => {}
        Some(_) => return None,
    }
    if champ(&o, "kind") != Some(&Json::Texte("reply".to_string())) {
        return None;
    }
    let id = match champ(&o, "id") {
        Some(Json::Texte(t)) => t.clone(),
        None => String::new(),
        Some(_) => return None,
    };
    match (champ(&o, "result"), champ(&o, "error")) {
        (Some(Json::Objet(r)), None) => Some((id, Corps::Resultat(r.clone()))),
        (None, Some(Json::Objet(e))) => match champ(e, "code") {
            Some(Json::Texte(c)) => Some((id, Corps::Erreur(c.chars().take(40).collect()))),
            _ => None,
        },
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn requete(args: &str) -> String {
        format!("{{\"v\":1,\"id\":\"7\",\"from\":\"spacenotch\",\"kind\":\"call\",\"name\":\"search.query\",\"args\":{args}}}")
    }

    #[test]
    fn trame_hello_valide() {
        let (id, a) = analyser_trame("{\"v\":1,\"id\":\"1\",\"from\":\"x\",\"kind\":\"call\",\"name\":\"hello\"}");
        assert_eq!(id, "1");
        assert_eq!(a, Ok(Appel::Hello));
        // Champ inconnu ignoré, fin de ligne tolérée
        assert_eq!(analyser_trame("{\"name\":\"hello\",\"zzz\":[1,2,{\"a\":null}]}\r\n").1, Ok(Appel::Hello));
    }

    #[test]
    fn trame_recherche_valide_et_limite_par_defaut() {
        assert_eq!(analyser_trame(&requete("{\"q\":\"windows\",\"limit\":5}")).1, Ok(Appel::Recherche { q: "windows".into(), limite: 5 }));
        assert_eq!(analyser_trame(&requete("{\"q\":\"windows\"}")).1, Ok(Appel::Recherche { q: "windows".into(), limite: 30 }));
        assert_eq!(analyser_trame(&requete("{\"q\":\"windows\",\"limit\":30.0}")).1, Ok(Appel::Recherche { q: "windows".into(), limite: 30 }));
        assert_eq!(analyser_trame(&requete("{\"q\":\"windows\",\"limit\":1}")).1, Ok(Appel::Recherche { q: "windows".into(), limite: 1 }));
    }

    #[test]
    fn limite_hors_bornes() {
        for l in ["0", "31", "-1", "2.5", "1e3", "99999999999999999999"] {
            assert_eq!(analyser_trame(&requete(&format!("{{\"q\":\"abc\",\"limit\":{l}}}"))).1, Err(Erreur::HorsBornes("limit")), "limit = {l}");
        }
        assert_eq!(analyser_trame(&requete("{\"q\":\"abc\",\"limit\":\"5\"}")).1, Err(Erreur::Invalide));
    }

    #[test]
    fn champs_manquants() {
        assert_eq!(analyser_trame("{\"id\":\"1\"}").1, Err(Erreur::ChampManquant("name")));
        assert_eq!(analyser_trame("{\"name\":\"search.query\"}").1, Err(Erreur::ChampManquant("args")));
        assert_eq!(analyser_trame("{\"name\":\"search.query\",\"args\":{}}").1, Err(Erreur::ChampManquant("q")));
        assert_eq!(analyser_trame("{\"name\":\"search.query\",\"args\":{\"q\":3}}").1, Err(Erreur::Invalide));
        assert_eq!(analyser_trame("{\"name\":\"search.query\",\"args\":[]}").1, Err(Erreur::Invalide));
        // L'identifiant est repris dans l'erreur
        assert_eq!(analyser_trame("{\"id\":\"abc\"}").0, "abc");
    }

    #[test]
    fn trame_trop_longue() {
        let longue = requete(&format!("{{\"q\":\"{}\"}}", "a".repeat(5000)));
        assert_eq!(analyser_trame(&longue).1, Err(Erreur::TropLong));
        // Pile à la borne : acceptée si la trame est valide
        let base = requete("{\"q\":\"\"}");
        let q = "a".repeat(MAX_TRAME - base.len());
        let pile = requete(&format!("{{\"q\":\"{q}\"}}"));
        assert_eq!(pile.len(), MAX_TRAME);
        assert!(matches!(analyser_trame(&pile).1, Ok(Appel::Recherche { .. })));
        assert_eq!(analyser_trame(&format!("{pile} ")).1, Err(Erreur::TropLong));
        // Plus de 4 096 octets et illisible : trop longue, pas « invalide » (le nom est inconnu)
        assert_eq!(analyser_trame(&"x".repeat(5000)).1, Err(Erreur::TropLong));
        assert_eq!(analyser_trame(&"x".repeat(100)).1, Err(Erreur::Invalide));
    }

    #[test]
    fn trames_invalides_sans_panique() {
        for t in ["", "x", "[]", "42", "null", "{\"name\":1}", "{\"id\":5,\"name\":\"hello\"}", "{\"name\":\"hello\",\"kind\":\"event\"}", "{\"name\":\"hello\",\"v\":\"1\"}"] {
            assert!(analyser_trame(t).1.is_err(), "devait etre rejetee : {t:?}");
        }
        assert_eq!(analyser_trame(&"[".repeat(3000)).1, Err(Erreur::Invalide));
        assert_eq!(analyser_trame(&"[".repeat(9000)).1, Err(Erreur::TropLong));
        assert_eq!(analyser_trame(&format!("{{\"a\":{}1{}}}", "[".repeat(50), "]".repeat(50))).1, Err(Erreur::Invalide));
    }

    #[test]
    fn identifiant_trop_long_refuse() {
        let t = format!("{{\"id\":\"{}\",\"name\":\"hello\"}}", "i".repeat(ID_MAX + 1));
        assert_eq!(analyser_trame(&t), (String::new(), Err(Erreur::Invalide)));
        let t = format!("{{\"id\":\"{}\",\"name\":\"hello\"}}", "i".repeat(ID_MAX));
        assert_eq!(analyser_trame(&t).1, Ok(Appel::Hello));
    }

    #[test]
    fn cles_dupliquees_dans_une_trame() {
        assert_eq!(analyser_trame("{\"name\":\"hello\",\"name\":\"search.query\"}").1, Err(Erreur::Invalide));
        assert_eq!(analyser_trame(&requete("{\"q\":\"abc\",\"q\":\"def\"}")).1, Err(Erreur::Invalide));
    }

    #[test]
    fn retour_chariot_final_hors_longueur() {
        let pile = vec![b'a'; MAX_OUVRIR];
        assert!(!ligne_trop_longue(&pile));
        let mut avec_cr = pile.clone();
        avec_cr.push(b'\r');
        assert!(!ligne_trop_longue(&avec_cr), "32 Ko + CR : acceptée au niveau du tampon");
        avec_cr.push(b'a');
        assert!(ligne_trop_longue(&avec_cr));
        let mut trop = pile;
        trop.push(b'a');
        assert!(ligne_trop_longue(&trop));
        // Même règle dans l'analyse de la trame
        let base = requete("{\"q\":\"\"}");
        let ligne = requete(&format!("{{\"q\":\"{}\"}}", "a".repeat(MAX_TRAME - base.len())));
        assert!(matches!(analyser_trame(&format!("{ligne}\r")).1, Ok(Appel::Recherche { .. })));
    }

    /// Table « version reçue / version parlée » du protocole (nous parlons la 1)
    #[test]
    fn versionnage_table() {
        let appel = |v: &str| analyser_trame(&format!("{{{v}\"name\":\"hello\"}}")).1;
        assert_eq!(appel(""), Ok(Appel::Hello), "v absent : 1");
        assert_eq!(appel("\"v\":1,"), Ok(Appel::Hello));
        assert_eq!(appel("\"v\":1.0,"), Ok(Appel::Hello));
        assert_eq!(appel("\"v\":2,"), Err(Erreur::Version));
        assert_eq!(appel("\"v\":99,"), Err(Erreur::Version));
        assert_eq!(appel("\"v\":0,"), Err(Erreur::Invalide));
        assert_eq!(appel("\"v\":-1,"), Err(Erreur::Invalide));
        assert_eq!(appel("\"v\":\"1\","), Err(Erreur::Invalide));
        // Un `v` trop haut est refusé même sur un appel qu'on ne connaît pas : la version prime sur le nom
        assert_eq!(analyser_trame("{\"v\":2,\"name\":\"zzz\"}").1, Err(Erreur::Version));
    }

    #[test]
    fn champ_nom_et_genre_inconnus() {
        // Champ inconnu ignoré, `name` inconnu -> inconnu, `kind` inconnu -> invalide
        assert_eq!(analyser_trame("{\"name\":\"hello\",\"futur\":{\"a\":[1]},\"x\":null}").1, Ok(Appel::Hello));
        assert_eq!(analyser_trame("{\"id\":\"9\",\"name\":\"zzz.yyy\"}"), ("9".to_string(), Err(Erreur::Inconnu("zzz.yyy".into()))));
        assert_eq!(analyser_trame("{\"name\":\"hello\",\"kind\":\"zzz\"}").1, Err(Erreur::Invalide));
        assert_eq!(analyser_trame("{\"name\":\"hello\",\"kind\":\"call\"}").1, Ok(Appel::Hello));
        let r = reponse_erreur("9", &Erreur::Inconnu("zzz.yyy".into()));
        assert!(r.contains("\"code\":\"inconnu\"") && r.contains("\"id\":\"9\""));
    }

    #[test]
    fn explorer_open_arguments() {
        let ouvrir = |args: &str| analyser_trame(&format!("{{\"id\":\"o\",\"name\":\"explorer.open\",\"args\":{args}}}")).1;
        assert_eq!(ouvrir("{\"path\":\"C:\\\\Users\"}"), Ok(Appel::Ouvrir { chemin: r"C:\Users".into(), reveler: false, reutiliser: false }));
        assert_eq!(ouvrir("{\"path\":\"C:\\\\a.txt\",\"reveal\":true,\"newTab\":false}"), Ok(Appel::Ouvrir { chemin: r"C:\a.txt".into(), reveler: true, reutiliser: false }));
        assert_eq!(ouvrir("{}"), Err(Erreur::ChampManquant("path")));
        assert_eq!(ouvrir("{\"path\":3}"), Err(Erreur::Invalide));
        assert_eq!(ouvrir("{\"path\":\"C:\\\\\",\"reveal\":\"oui\"}"), Err(Erreur::Invalide));
        assert_eq!(ouvrir("{\"path\":\"C:\\\\\",\"newTab\":1}"), Err(Erreur::Invalide));
        assert_eq!(analyser_trame("{\"name\":\"explorer.open\"}").1, Err(Erreur::ChampManquant("args")));
    }

    /// `reuse` : absent ou nul = faux, booléen lu tel quel, tout autre type = `invalide` (comme `newTab` et `reveal`)
    #[test]
    fn explorer_open_reuse() {
        let ouvrir = |args: &str| analyser_trame(&format!("{{\"id\":\"o\",\"name\":\"explorer.open\",\"args\":{args}}}")).1;
        let attendu = |reutiliser| Ok(Appel::Ouvrir { chemin: r"C:\a".into(), reveler: false, reutiliser });
        assert_eq!(ouvrir("{\"path\":\"C:\\\\a\"}"), attendu(false));
        assert_eq!(ouvrir("{\"path\":\"C:\\\\a\",\"reuse\":null}"), attendu(false));
        assert_eq!(ouvrir("{\"path\":\"C:\\\\a\",\"reuse\":true}"), attendu(true));
        assert_eq!(ouvrir("{\"path\":\"C:\\\\a\",\"reuse\":false}"), attendu(false));
        for faux in ["\"oui\"", "1", "0", "[]", "{}", "\"true\""] {
            assert_eq!(ouvrir(&format!("{{\"path\":\"C:\\\\a\",\"reuse\":{faux}}}")), Err(Erreur::Invalide), "reuse = {faux}");
        }
        // `reuse` et `reveal` se lisent indépendamment
        assert_eq!(
            ouvrir("{\"path\":\"C:\\\\a\",\"reveal\":true,\"reuse\":true}"),
            Ok(Appel::Ouvrir { chemin: r"C:\a".into(), reveler: true, reutiliser: true })
        );
    }

    /// 32 Ko : seul `explorer.open` passe la borne de 4 096 octets, les autres appels sont « trop longs »
    #[test]
    fn explorer_open_jusqu_a_32_ko() {
        let chemin = format!("C:\\\\{}", "d".repeat(20_000));
        let ligne = format!("{{\"name\":\"explorer.open\",\"args\":{{\"path\":\"{chemin}\"}}}}");
        assert!(ligne.len() > MAX_TRAME && ligne.len() < MAX_OUVRIR);
        assert!(matches!(analyser_trame(&ligne).1, Ok(Appel::Ouvrir { .. })));
        let enorme = format!("{{\"name\":\"explorer.open\",\"args\":{{\"path\":\"{}\"}}}}", "d".repeat(MAX_OUVRIR));
        assert_eq!(analyser_trame(&enorme).1, Err(Erreur::TropLong));
        let recherche = format!("{{\"name\":\"search.query\",\"args\":{{\"q\":\"{}\"}}}}", "a".repeat(20_000));
        assert_eq!(analyser_trame(&recherche).1, Err(Erreur::TropLong));
        let hello = format!("{{\"name\":\"hello\",\"x\":\"{}\"}}", "a".repeat(5_000));
        assert_eq!(analyser_trame(&hello).1, Err(Erreur::TropLong));
    }

    #[test]
    fn reponses_hello_ouvert_et_chargement() {
        let h = reponse_hello("1");
        assert!(h.contains("\"app\":\"spacefiles\"") && h.contains("\"protocol\":1"));
        assert!(h.contains("\"capabilities\":[\"search.query\",\"explorer.open\"]"), "{h}");
        assert!(analyser_json(&h).is_some());
        assert!(reponse_ouvert("3").contains("\"opened\":true"));
        let c = reponse_chargement("2");
        assert!(c.contains("\"results\":[]") && c.contains("\"loading\":true") && analyser_json(&c).is_some());
        let e = reponse_erreur("4", &Erreur::TropVite);
        assert!(e.contains("\"code\":\"trop_vite\"") && analyser_json(&e).is_some());
    }

    #[test]
    fn reponse_sous_la_borne_par_troncature_de_resultats() {
        let dossier = format!(r"C:\{}", "d".repeat(100));
        let r: Vec<Resultat> = (0..30).map(|i| (format!("fichier{i}.txt"), format!("{dossier}\\fichier{i}.txt"), dossier.clone())).collect();
        let rep = reponse_recherche("5", &r, false);
        assert!(rep.len() <= MAX_TRAME, "{} octets", rep.len());
        assert!(rep.contains("\"truncated\":true"));
        // Toujours une ligne JSON complète, avec moins de résultats
        let Some(Json::Objet(o)) = analyser_json(&rep) else { panic!("trame coupée") };
        let Some(Json::Objet(res)) = champ(&o, "result") else { panic!("pas de result") };
        let Some(Json::Liste(l)) = champ(res, "results") else { panic!("pas de results") };
        assert!(!l.is_empty() && l.len() < 30);
        // Peu de résultats : rien n'est retiré, `truncated` suit l'appelant
        let court = reponse_recherche("5", &r[..2], false);
        assert!(court.contains("\"truncated\":false") && court.matches("\"name\"").count() == 2);
        assert!(reponse_recherche("5", &r[..2], true).contains("\"truncated\":true"));
        // Chemins énormes : jamais plus que la borne, quitte à ne rien renvoyer
        let enormes = vec![("n".to_string(), "p".repeat(5000), "d".to_string())];
        let rep = reponse_recherche("5", &enormes, false);
        assert!(rep.len() <= MAX_TRAME && rep.contains("\"results\":[]") && rep.contains("\"truncated\":true"));
    }

    #[test]
    fn lecture_d_une_reponse() {
        let h = reponse_hello("a1");
        let Some((id, Corps::Resultat(r))) = analyser_reponse(&h) else { panic!("hello illisible") };
        assert_eq!(id, "a1");
        assert_eq!(champ(&r, "protocol"), Some(&Json::Nombre(1.0)));
        assert_eq!(analyser_reponse(&reponse_erreur("b", &Erreur::Version)), Some(("b".to_string(), Corps::Erreur("version".into()))));
        // Ni une réponse du protocole 2, ni un appel, ni un objet sans result ni error, ni les deux à la fois
        for t in [
            "{\"v\":2,\"id\":\"a\",\"kind\":\"reply\",\"result\":{}}",
            "{\"v\":1,\"id\":\"a\",\"kind\":\"call\",\"result\":{}}",
            "{\"v\":1,\"id\":\"a\",\"kind\":\"reply\"}",
            "{\"v\":1,\"id\":\"a\",\"kind\":\"reply\",\"result\":{},\"error\":{\"code\":\"x\"}}",
            "{\"v\":1,\"id\":\"a\",\"kind\":\"reply\",\"error\":{}}",
            "pas du json",
        ] {
            assert_eq!(analyser_reponse(t), None, "{t}");
        }
    }

    #[test]
    fn trame_sortante_relue_par_le_serveur() {
        let t = trame_sortante("call", "c1", "search.query", "{\"q\":\"abc\"}");
        assert_eq!(analyser_trame(&t), ("c1".to_string(), Ok(Appel::Recherche { q: "abc".into(), limite: 30 })));
        // Un événement n'est pas un appel : notre serveur le refuse (aucun événement entrant en version 1)
        assert_eq!(analyser_trame(&trame_sortante("event", "e", "progress", "{}")).1, Err(Erreur::Invalide));
    }
}
