// JSON minimal des échanges Eco, lu à la main (pas de crate) : trames du tube (`trame.rs`), `shared.json` (`partage.rs`),
// réponses lues par le client (`client.rs`). Tout ce qui sort de la grammaire est rejeté ; l'imbrication, les clés en
// double et la taille des nombres sont bornées pour qu'une trame hostile ne coûte ni pile ni confusion.

/// Sous-ensemble de JSON lu à la main (pas de crate) : tout ce qui sort de la grammaire est rejeté
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Json {
    Nul,
    Bool(bool),
    Nombre(f64),
    Texte(String),
    Liste(Vec<Json>),
    Objet(Vec<(String, Json)>),
}

/// Imbrication maximale acceptée : une trame hostile faite de `[[[[…` ne doit pas épuiser la pile
const PROFONDEUR_MAX: usize = 8;
/// Clés d'un même objet d'une trame du tube
const CLES_MAX: usize = 256;
/// Clés d'un même objet de `shared.json` (fichier de 256 Ko au plus, lu une fois par seconde au plus : la vérification des
/// doublons reste bornée à ce nombre)
pub(crate) const CLES_MAX_PARTAGE: usize = 1024;

struct Lecteur<'a> {
    o: &'a [u8],
    p: usize,
    cles_max: usize,
}

impl Lecteur<'_> {
    fn espaces(&mut self) {
        while matches!(self.o.get(self.p), Some(b' ' | b'\t' | b'\r' | b'\n')) {
            self.p += 1;
        }
    }

    fn mot(&mut self, mot: &[u8], v: Json) -> Option<Json> {
        if self.o.get(self.p..self.p + mot.len())? == mot {
            self.p += mot.len();
            Some(v)
        } else {
            None
        }
    }

    fn valeur(&mut self, prof: usize) -> Option<Json> {
        match *self.o.get(self.p)? {
            b'{' => self.objet(prof),
            b'[' => self.liste(prof),
            b'"' => self.texte().map(Json::Texte),
            b't' => self.mot(b"true", Json::Bool(true)),
            b'f' => self.mot(b"false", Json::Bool(false)),
            b'n' => self.mot(b"null", Json::Nul),
            b'-' | b'0'..=b'9' => self.nombre(),
            _ => None,
        }
    }

    fn objet(&mut self, prof: usize) -> Option<Json> {
        // `prof` est le niveau de ce conteneur (0 pour le document) : 8 niveaux imbriqués au plus
        if prof >= PROFONDEUR_MAX {
            return None;
        }
        self.p += 1;
        let mut paires = Vec::new();
        self.espaces();
        if self.o.get(self.p) == Some(&b'}') {
            self.p += 1;
            return Some(Json::Objet(paires));
        }
        loop {
            self.espaces();
            if self.o.get(self.p) != Some(&b'"') {
                return None;
            }
            let cle = self.texte()?;
            self.espaces();
            if self.o.get(self.p) != Some(&b':') {
                return None;
            }
            self.p += 1;
            self.espaces();
            // Clé en double : rejetée (deux lectures possibles d'une même trame ouvrent la porte aux confusions)
            if paires.iter().any(|(k, _)| *k == cle) {
                return None;
            }
            // Au plus `cles_max` clés par objet (256 pour une trame, 1 024 pour shared.json) : la vérification des doublons reste bornée (une ligne de 32 Ko en tient des milliers)
            if paires.len() >= self.cles_max {
                return None;
            }
            let v = self.valeur(prof + 1)?;
            paires.push((cle, v));
            self.espaces();
            match self.o.get(self.p)? {
                b',' => self.p += 1,
                b'}' => {
                    self.p += 1;
                    return Some(Json::Objet(paires));
                }
                _ => return None,
            }
        }
    }

    fn liste(&mut self, prof: usize) -> Option<Json> {
        if prof >= PROFONDEUR_MAX {
            return None;
        }
        self.p += 1;
        let mut v = Vec::new();
        self.espaces();
        if self.o.get(self.p) == Some(&b']') {
            self.p += 1;
            return Some(Json::Liste(v));
        }
        loop {
            self.espaces();
            v.push(self.valeur(prof + 1)?);
            self.espaces();
            match self.o.get(self.p)? {
                b',' => self.p += 1,
                b']' => {
                    self.p += 1;
                    return Some(Json::Liste(v));
                }
                _ => return None,
            }
        }
    }

    fn quatre_hex(&mut self) -> Option<u32> {
        let h = std::str::from_utf8(self.o.get(self.p..self.p + 4)?).ok()?;
        if !h.bytes().all(|c| c.is_ascii_hexdigit()) {
            return None;
        }
        self.p += 4;
        u32::from_str_radix(h, 16).ok()
    }

    /// Chaîne entre guillemets, la position est sur le guillemet ouvrant
    fn texte(&mut self) -> Option<String> {
        self.p += 1;
        let mut tampon: Vec<u8> = Vec::new();
        loop {
            let c = *self.o.get(self.p)?;
            self.p += 1;
            match c {
                b'"' => return String::from_utf8(tampon).ok(),
                // Un caractère de contrôle brut n'a pas le droit d'être dans une chaîne JSON
                0..=0x1F => return None,
                b'\\' => {
                    let e = *self.o.get(self.p)?;
                    self.p += 1;
                    match e {
                        b'"' => tampon.push(b'"'),
                        b'\\' => tampon.push(b'\\'),
                        b'/' => tampon.push(b'/'),
                        b'b' => tampon.push(8),
                        b'f' => tampon.push(12),
                        b'n' => tampon.push(b'\n'),
                        b'r' => tampon.push(b'\r'),
                        b't' => tampon.push(b'\t'),
                        b'u' => {
                            let mut u = self.quatre_hex()?;
                            if (0xD800..0xDC00).contains(&u) {
                                // Moitié haute d'une paire de substitution : la basse doit suivre
                                if self.o.get(self.p..self.p + 2)? != b"\\u" {
                                    return None;
                                }
                                self.p += 2;
                                let bas = self.quatre_hex()?;
                                if !(0xDC00..0xE000).contains(&bas) {
                                    return None;
                                }
                                u = 0x10000 + ((u - 0xD800) << 10) + (bas - 0xDC00);
                            }
                            let ch = char::from_u32(u)?;
                            let mut b = [0u8; 4];
                            tampon.extend_from_slice(ch.encode_utf8(&mut b).as_bytes());
                        }
                        _ => return None,
                    }
                }
                _ => tampon.push(c),
            }
        }
    }

    /// Nombre selon la grammaire JSON (pas de `+`, pas de zéro de tête, pas de `.5`)
    fn nombre(&mut self) -> Option<Json> {
        let debut = self.p;
        let chiffres = |l: &mut Self| {
            let d = l.p;
            while matches!(l.o.get(l.p), Some(b'0'..=b'9')) {
                l.p += 1;
            }
            l.p - d
        };
        if self.o.get(self.p) == Some(&b'-') {
            self.p += 1;
        }
        match self.o.get(self.p)? {
            b'0' => self.p += 1,
            b'1'..=b'9' => {
                chiffres(self);
            }
            _ => return None,
        }
        if self.o.get(self.p) == Some(&b'.') {
            self.p += 1;
            if chiffres(self) == 0 {
                return None;
            }
        }
        if matches!(self.o.get(self.p), Some(b'e' | b'E')) {
            self.p += 1;
            if matches!(self.o.get(self.p), Some(b'+' | b'-')) {
                self.p += 1;
            }
            if chiffres(self) == 0 {
                return None;
            }
        }
        if self.p - debut > 32 {
            return None;
        }
        let n: f64 = std::str::from_utf8(&self.o[debut..self.p]).ok()?.parse().ok()?;
        n.is_finite().then_some(Json::Nombre(n))
    }
}

/// Un document JSON complet ; du texte après la valeur le fait rejeter
pub(crate) fn analyser_json(s: &str) -> Option<Json> {
    analyser_json_avec(s, CLES_MAX)
}

/// Comme `analyser_json` avec une autre borne de clés par objet (`CLES_MAX_PARTAGE` pour `shared.json`)
pub(crate) fn analyser_json_avec(s: &str, cles_max: usize) -> Option<Json> {
    let mut l = Lecteur { o: s.as_bytes(), p: 0, cles_max };
    l.espaces();
    let v = l.valeur(0)?;
    l.espaces();
    (l.p == l.o.len()).then_some(v)
}

pub(crate) fn champ<'a>(o: &'a [(String, Json)], cle: &str) -> Option<&'a Json> {
    o.iter().find(|(k, _)| k == cle).map(|(_, v)| v)
}

/// Chaîne JSON entre guillemets : guillemets, antislash et caractères de contrôle échappés, le reste (accents,
/// emojis) reste tel quel en UTF-8
pub(crate) fn echapper(s: &str) -> String {
    let mut r = String::with_capacity(s.len() + 2);
    r.push('"');
    for c in s.chars() {
        match c {
            '"' => r.push_str("\\\""),
            '\\' => r.push_str("\\\\"),
            '\n' => r.push_str("\\n"),
            '\r' => r.push_str("\\r"),
            '\t' => r.push_str("\\t"),
            c if (c as u32) < 0x20 => r.push_str(&format!("\\u{:04x}", c as u32)),
            c => r.push(c),
        }
    }
    r.push('"');
    r
}

/// Un nombre sans `.0` inutile (`3`, pas `3.0`) ; relu à l'identique par `analyser_json`
#[cfg(test)]
fn nombre_texte(n: f64) -> String {
    if n.fract() == 0.0 && n.abs() < 1e15 {
        format!("{}", n as i64)
    } else {
        format!("{n}")
    }
}

/// Écrit une valeur en JSON compact : sert à réécrire `shared.json` sans perdre les champs qu'on ne connaît pas.
/// Compilé en test seulement, comme `partage::ecrire` qui l'appelle (aucun appelant avant les phases 6 et 7).
#[cfg(test)]
pub(crate) fn serialiser(v: &Json) -> String {
    match v {
        Json::Nul => "null".to_string(),
        Json::Bool(b) => b.to_string(),
        Json::Nombre(n) => nombre_texte(*n),
        Json::Texte(t) => echapper(t),
        Json::Liste(l) => format!("[{}]", l.iter().map(serialiser).collect::<Vec<_>>().join(",")),
        Json::Objet(o) => format!("{{{}}}", o.iter().map(|(k, v)| format!("{}:{}", echapper(k), serialiser(v))).collect::<Vec<_>>().join(",")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trames_invalides_sans_panique() {
        for t in [
            "", "x", "{", "}", "{\"name\":", "{\"name\":\"hello\"", "{\"name\":\"hello\"} x", "{name:\"hello\"}", "{'name':'hello'}",
            "{\"name\":\"hello\",}", "{\"a\":01}", "{\"a\":+1}", "{\"a\":.5}", "{\"a\":1e}", "{\"a\":\"\\x\"}", "{\"a\":\"\\u12\"}", "{\"a\":\"\\ud800\"}",
            "{\"a\":\"\\ud800\\u0041\"}", "{\"a\":\"\\udc00\"}", "{\"a\":\"ligne\nbrute\"}", "{\"a\":tru}",
        ] {
            assert!(analyser_json(t).is_none(), "devait etre rejete : {t:?}");
        }
        // Imbrication hostile : rejetée sans épuiser la pile
        assert!(analyser_json(&"[".repeat(3000)).is_none());
        assert!(analyser_json(&format!("{{\"a\":{}1{}}}", "[".repeat(50), "]".repeat(50))).is_none());
    }

    #[test]
    fn echappement_guillemets_antislash_controles() {
        assert_eq!(echapper(r#"C:\Users\"bob"\a.txt"#), r#""C:\\Users\\\"bob\"\\a.txt""#);
        assert_eq!(echapper("a\nb\tc\r\u{1}"), "\"a\\nb\\tc\\r\\u0001\"");
        // Aller-retour par l'analyseur
        let s = "x\"\\y\n\u{7}z";
        assert_eq!(analyser_json(&echapper(s)), Some(Json::Texte(s.to_string())));
    }

    #[test]
    fn echappement_unicode() {
        let s = r"C:\Users\Zoé\Documents\été 日本語 😀.txt";
        let e = echapper(s);
        assert!(e.contains("Zoé") && e.contains("😀"), "l'UTF-8 reste tel quel");
        assert_eq!(analyser_json(&e), Some(Json::Texte(s.to_string())));
        // Séquences \u de l'appelant, paire de substitution comprise
        assert_eq!(analyser_json("\"\\u00e9\\ud83d\\ude00\""), Some(Json::Texte("é😀".to_string())));
    }

    #[test]
    fn profondeur_exactement_huit_niveaux() {
        let imbrique = |n: usize| format!("{}1{}", "[".repeat(n), "]".repeat(n));
        assert!(analyser_json(&imbrique(8)).is_some(), "8 niveaux acceptés");
        assert!(analyser_json(&imbrique(9)).is_none(), "9 niveaux refusés");
        let objets = |n: usize| format!("{}1{}", "{\"a\":".repeat(n), "}".repeat(n));
        assert!(analyser_json(&objets(8)).is_some());
        assert!(analyser_json(&objets(9)).is_none());
    }

    #[test]
    fn cles_par_objet_bornees() {
        let objet = |n: usize| format!("{{{}}}", (0..n).map(|i| format!("\"k{i}\":1")).collect::<Vec<_>>().join(","));
        assert!(analyser_json(&objet(CLES_MAX)).is_some(), "256 clés : permis");
        assert!(analyser_json(&objet(CLES_MAX + 1)).is_none(), "257 clés : refusé");
        assert!(analyser_json(&objet(5000)).is_none());
    }

    #[test]
    fn cles_dupliquees_rejetees() {
        assert!(analyser_json("{\"a\":1,\"a\":2}").is_none());
        // Même clé dans deux objets différents : permis
        assert!(analyser_json("{\"a\":{\"a\":1},\"b\":{\"a\":2}}").is_some());
    }

    #[test]
    fn serialisation_aller_retour() {
        let doc = "{\"a\":[1,2.5,-3,true,null,\"é\\\"\"],\"b\":{\"c\":\"x\"},\"d\":12345678901}";
        let Some(v) = analyser_json(doc) else { panic!("document valide refuse") };
        let texte = serialiser(&v);
        assert_eq!(analyser_json(&texte), Some(v));
        assert!(texte.contains("\"a\":[1,2.5,-3,true,null"), "{texte}");
    }
}
