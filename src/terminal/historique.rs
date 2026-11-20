//! Historique des commandes du terminal (PLAN-TERMINAL, lot T5) : ce que l'auteur a tapé dans la ligne de saisie, avec le dossier
//! où il l'a tapé. Sert à la suggestion grise (la fin d'une commande déjà tapée, d'abord dans ce dossier) et au rappel par ↑ / ↓.
//!
//! Confidentialité (D-T7) : une ligne qui ressemble à un secret n'est **jamais** gardée (ni en mémoire ni sur disque) ; une ligne
//! commencée par une espace non plus (convention des shells pour « ne pas l'enregistrer »). Le fichier est
//! `%LOCALAPPDATA%\SpaceFiles\historique-terminal.txt` (décision D-T7) : ce dossier n'est jamais synchronisé
//! (contrairement à `%APPDATA%`, itinérant sur un domaine) ; le filtre ci-dessous reste une prudence, pas une garantie.
//!
//! Tout ce qui se décide sans disque est pur et testé ; la lecture et l'écriture du fichier sont en bas de ce module.

use std::path::Path;

/// Nombre de commandes gardées : au-delà, les plus anciennes partent
pub const MAX_ENTREES: usize = 5_000;
/// Une ligne plus longue n'est pas une commande qu'on rappellera (collage d'un script) : pas gardée
pub const MAX_LONGUEUR: usize = 500;

/// Mots qui font écarter une ligne (comparés en minuscules). Voir D-T7.
const MOTS_SECRETS: [&str; 16] = [
    "password", "passwd", "token", "secret", "apikey", "api_key", "api-key", "motdepasse", "mot de passe", "bearer ", "pass:",
    "-asplaintext", "securestring", "net user", "pwd=", "credential",
];

/// Débuts de jetons d'accès connus (GitHub, GitLab, Slack, AWS, clés `sk-`) : comparés seulement en début de mot, pour ne pas
/// écarter `task-list` à cause de `sk-`
const PREFIXES_JETONS: [&str; 7] = ["ghp_", "gho_", "github_pat_", "glpat-", "sk-", "xox", "akia"];

/// `://utilisateur:mot-de-passe@hôte` : des identifiants dans une adresse
fn identifiants_dans_adresse(bas: &str) -> bool {
    bas.match_indices("://").any(|(i, _)| {
        let reste = &bas[i + 3..];
        let autorite = reste.split(['/', ' ', '?']).next().unwrap_or("");
        autorite.contains('@') && autorite.split('@').next().is_some_and(|u| u.contains(':'))
    })
}

/// Un mot qui finit par `_pat` (`GITHUB_PAT`) : jeton d'accès personnel ; `_path` ne doit pas être écarté
fn variable_pat(bas: &str) -> bool {
    bas.match_indices("_pat").any(|(i, m)| bas[i + m.len()..].chars().next().map_or(true, |c| !c.is_alphanumeric()))
}

fn jeton_connu(bas: &str) -> bool {
    PREFIXES_JETONS.iter().any(|p| {
        bas.match_indices(p).any(|(i, _)| bas[..i].chars().next_back().map_or(true, |c| !c.is_alphanumeric()))
    })
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entree {
    pub ligne: String,
    /// Dossier où la commande a été tapée (tel que le shell l'annonce)
    pub dossier: String,
    /// Secondes depuis 1970 : sert seulement à ordonner dans le fichier
    pub date: u64,
}

#[derive(Debug, Default)]
pub struct Historique {
    /// Du plus ancien au plus récent
    entrees: Vec<Entree>,
}

/// La ligne ressemble à un secret (mot de passe, jeton…) : à ne pas enregistrer
pub fn est_secret(ligne: &str) -> bool {
    let bas = ligne.to_lowercase();
    MOTS_SECRETS.iter().any(|m| bas.contains(m)) || jeton_connu(&bas) || variable_pat(&bas) || identifiants_dans_adresse(&bas)
}

/// Une ligne qu'on garde : une commande non vide, d'une seule ligne, de longueur raisonnable, sans secret, qui ne commence pas
/// par une espace
pub fn est_gardable(ligne: &str) -> bool {
    !ligne.trim().is_empty()
        && !ligne.starts_with(' ')
        && !ligne.contains(['\n', '\r'])
        && ligne.chars().count() <= MAX_LONGUEUR
        && !est_secret(ligne)
}

fn meme_dossier(a: &str, b: &str) -> bool {
    a.trim_end_matches('\\').eq_ignore_ascii_case(b.trim_end_matches('\\'))
}

impl Historique {
    #[cfg(test)]
    pub fn len(&self) -> usize {
        self.entrees.len()
    }

    pub fn est_vide(&self) -> bool {
        self.entrees.is_empty()
    }

    /// Entrée numéro `rang` en partant de la plus récente (0 = la dernière tapée)
    #[cfg(test)]
    pub fn recente(&self, rang: usize) -> Option<&Entree> {
        self.entrees.len().checked_sub(rang + 1).and_then(|i| self.entrees.get(i))
    }

    /// Ajoute une commande tapée dans `dossier`. `None` si elle n'est pas gardable ; sinon l'entrée ajoutée (la même ligne tapée
    /// au même endroit n'existe qu'une fois : l'ancienne est retirée, la nouvelle devient la plus récente).
    pub fn ajouter(&mut self, ligne: &str, dossier: &str, date: u64) -> Option<&Entree> {
        if !est_gardable(ligne) {
            return None;
        }
        let ligne = ligne.trim_end();
        self.entrees.retain(|e| !(e.ligne == ligne && meme_dossier(&e.dossier, dossier)));
        self.entrees.push(Entree { ligne: ligne.to_string(), dossier: dossier.to_string(), date });
        if self.entrees.len() > MAX_ENTREES {
            let trop = self.entrees.len() - MAX_ENTREES;
            self.entrees.drain(..trop);
        }
        self.entrees.last()
    }

    /// La suggestion grise : la commande **la mieux placée** qui commence par `debut` et le prolonge. Ordre : celles tapées dans
    /// ce dossier d'abord, puis les autres ; dans chaque groupe la plus récente. Rend seulement la **fin** à ajouter. Rien pour
    /// un début vide (sinon la dernière commande s'afficherait sur chaque invite).
    pub fn suggerer(&self, debut: &str, dossier: &str) -> Option<&str> {
        if debut.is_empty() {
            return None;
        }
        let prolonge = |e: &&Entree| e.ligne.len() > debut.len() && e.ligne.starts_with(debut);
        let ici = self.entrees.iter().rev().filter(prolonge).find(|e| meme_dossier(&e.dossier, dossier));
        let retenue = ici.or_else(|| self.entrees.iter().rev().find(prolonge))?;
        Some(&retenue.ligne[debut.len()..])
    }

    /// La liste automatique : au plus `max` lignes distinctes qui commencent par `debut` et le prolongent. Ordre de la
    /// suggestion grise (dossier courant d'abord, puis le reste), la plus récente en premier dans chaque groupe : la première
    /// est donc celle que la grise montre. Rien pour un début vide.
    pub fn proposer(&self, debut: &str, dossier: &str, max: usize) -> Vec<&str> {
        let mut rendues: Vec<&str> = Vec::new();
        if debut.is_empty() || max == 0 {
            return rendues;
        }
        let prolonge = |e: &&Entree| e.ligne.len() > debut.len() && e.ligne.starts_with(debut);
        for ici in [true, false] {
            for e in self.entrees.iter().rev().filter(prolonge) {
                if rendues.len() == max {
                    return rendues;
                }
                if meme_dossier(&e.dossier, dossier) == ici && !rendues.contains(&e.ligne.as_str()) {
                    rendues.push(&e.ligne);
                }
            }
        }
        rendues
    }

    /// Les lignes distinctes, de la plus récente à la plus ancienne (rappel par ↑ : une commande tapée dans trois dossiers
    /// ne revient qu'une fois). `rang` 0 = la plus récente ; `commence_par` filtre (vide = tout).
    pub fn rappeler(&self, rang: usize, commence_par: &str) -> Option<&str> {
        let mut vues: Vec<&str> = Vec::new();
        for e in self.entrees.iter().rev() {
            if !e.ligne.starts_with(commence_par) || vues.contains(&e.ligne.as_str()) {
                continue;
            }
            if vues.len() == rang {
                return Some(&e.ligne);
            }
            vues.push(&e.ligne);
        }
        None
    }
}

// ---- Fichier ----

/// Échappe `\`, tabulation et fins de ligne pour tenir sur une ligne du fichier
fn echapper(t: &str) -> String {
    let mut s = String::with_capacity(t.len());
    for c in t.chars() {
        match c {
            '\\' => s.push_str("\\\\"),
            '\t' => s.push_str("\\t"),
            '\n' => s.push_str("\\n"),
            '\r' => s.push_str("\\r"),
            c => s.push(c),
        }
    }
    s
}

fn desechapper(t: &str) -> String {
    let mut s = String::with_capacity(t.len());
    let mut it = t.chars();
    while let Some(c) = it.next() {
        if c != '\\' {
            s.push(c);
            continue;
        }
        match it.next() {
            Some('t') => s.push('\t'),
            Some('n') => s.push('\n'),
            Some('r') => s.push('\r'),
            Some(autre) => s.push(autre),
            None => {}
        }
    }
    s
}

/// Une entrée → une ligne de fichier : `date <tab> dossier <tab> commande`
pub fn ligne_de_fichier(e: &Entree) -> String {
    format!("{}\t{}\t{}", e.date, echapper(&e.dossier), echapper(&e.ligne))
}

/// Une ligne de fichier → une entrée ; `None` si elle est abîmée ou si elle ne passerait plus le filtre (un fichier édité à la
/// main ne ramène pas un secret dans les suggestions)
pub fn entree_de_ligne(texte: &str) -> Option<Entree> {
    let mut champs = texte.splitn(3, '\t');
    let date = champs.next()?.trim().parse::<u64>().ok()?;
    let dossier = desechapper(champs.next()?);
    let ligne = desechapper(champs.next()?);
    est_gardable(&ligne).then_some(Entree { ligne, dossier, date })
}

impl Historique {
    /// Relit un fichier : lignes abîmées ignorées, doublons repliés sur la plus récente, plafond appliqué
    pub fn depuis_texte(texte: &str) -> Historique {
        let mut h = Historique::default();
        for l in texte.lines() {
            if let Some(e) = entree_de_ligne(l.trim_start_matches('\u{feff}')) {
                h.ajouter(&e.ligne, &e.dossier, e.date);
            }
        }
        h
    }

    /// Tout l'historique, une entrée par ligne (réécriture complète du fichier)
    pub fn vers_texte(&self) -> String {
        let mut s = String::new();
        for e in &self.entrees {
            s.push_str(&ligne_de_fichier(e));
            s.push('\n');
        }
        s
    }
}

/// Amorce l'historique avec celui de PSReadLine (`ConsoleHost_history.txt`, une commande par ligne), pour que la suggestion
/// serve dès la première frappe. Mêmes filtres que partout (secrets, lignes en retrait, trop longues) ; une ligne qui finit par
/// un accent grave continue sur la suivante : seule, elle n'est pas une commande, on l'écarte. Le dossier d'origine est inconnu
/// (vide) : ces commandes passent après celles tapées dans le dossier courant. Seules les `MAX_ENTREES` dernières comptent.
pub fn depuis_psreadline(texte: &str) -> Historique {
    let mut h = Historique::default();
    let lignes: Vec<&str> = texte.lines().map(|l| l.trim_start_matches('\u{feff}').trim_end()).collect();
    let debut = lignes.len().saturating_sub(MAX_ENTREES * 2);
    for l in &lignes[debut..] {
        if !l.ends_with('`') {
            h.ajouter(l, "", 0);
        }
    }
    h
}

/// Lit le fichier d'historique ; absent ou illisible = historique vide
pub fn charger(chemin: &Path) -> Historique {
    match std::fs::read(chemin) {
        Ok(octets) => Historique::depuis_texte(&String::from_utf8_lossy(&octets)),
        Err(_) => Historique::default(),
    }
}

/// Ajoute une entrée à la fin du fichier (le doublon éventuel est replié à la relecture). Une erreur d'écriture est sans suite :
/// l'historique reste en mémoire.
pub fn ajouter_au_fichier(chemin: &Path, e: &Entree) {
    use std::io::Write;
    if let Some(parent) = chemin.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(chemin) {
        let _ = writeln!(f, "{}", ligne_de_fichier(e));
    }
}

/// Réécrit le fichier d'après la mémoire (écriture dans un fichier voisin puis renommage : une coupure ne laisse pas un
/// fichier tronqué). À faire quand le fichier a trop grossi.
pub fn reecrire(chemin: &Path, h: &Historique) {
    let voisin = chemin.with_extension("tmp");
    if std::fs::write(&voisin, h.vers_texte()).is_ok() && std::fs::rename(&voisin, chemin).is_err() {
        let _ = std::fs::remove_file(&voisin);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const D: &str = "C:\\Users\\utilisateur\\projet";
    const AUTRE: &str = "C:\\Windows";

    fn historique(lignes: &[(&str, &str)]) -> Historique {
        let mut h = Historique::default();
        for (i, (l, d)) in lignes.iter().enumerate() {
            h.ajouter(l, d, 100 + i as u64);
        }
        h
    }

    #[test]
    fn les_secrets_ne_sont_pas_gardes() {
        let mut h = Historique::default();
        for ligne in [
            "$env:TOKEN = 'abc'",
            "set PASSWORD=hunter2",
            "curl -H \"Authorization: Bearer xyz\" https://a.b",
            "$secret = Read-Host",
            "git clone https://user:Passwd@host/repo",
            "git clone https://alice:motdepasse1@host/repo",
            "git clone https://alice:abc@host/repo",
            "openssl enc -pass pass:hunter2",
            "openssl enc -pass:hunter2",
            "ConvertTo-SecureString 'x' -AsPlainText -Force",
            "net user bob hunter2 /add",
            "gh auth login --with-token",
            "git remote set-url origin https://x:y@host/r",
            "export GH=ghp_abcdef",
            "echo github_pat_11AAAA",
            "curl -u x:glpat-abc https://a.b",
            "echo sk-abcdef",
            "echo xoxb-123",
            "echo AKIAIOSFODNN7",
            "setx GITHUB_PAT 1",
            "Get-Credential",
            "server=a;pwd=zzz",
            "setx API_KEY 123",
            "setx api-key 123",
            "mon motdepasse est",
            "mon mot de passe est",
            "  commence par une espace",
            "",
            "   ",
            "ligne\nsur deux lignes",
        ] {
            assert!(h.ajouter(ligne, D, 1).is_none(), "ne doit pas être gardée : {ligne:?}");
        }
        assert!(h.est_vide());
        assert!(h.ajouter("git status", D, 2).is_some());
        assert_eq!(h.len(), 1);
        // des commandes proches mais sans secret restent gardées (pas de faux positif sur les débuts de jetons ni les adresses)
        for ligne in ["task-list", "cargo run --task-x", "git clone https://github.com/a/b", "$env:MY_PATH = 'x'", "set PYTHON_PATH=a", r"cd C:\a@b", "ssh bob@host", "echo https://a.b/x@y:z"] {
            assert!(h.ajouter(ligne, D, 3).is_some(), "doit être gardée : {ligne:?}");
        }
    }

    #[test]
    fn une_ligne_trop_longue_n_est_pas_gardee() {
        let mut h = Historique::default();
        assert!(h.ajouter(&"a".repeat(MAX_LONGUEUR + 1), D, 1).is_none());
        assert!(h.ajouter(&"a".repeat(MAX_LONGUEUR), D, 1).is_some());
    }

    #[test]
    fn la_meme_commande_au_meme_endroit_n_existe_qu_une_fois() {
        let mut h = historique(&[("git status", D), ("ls", D), ("git status", D)]);
        assert_eq!(h.len(), 2);
        assert_eq!(h.recente(0).unwrap().ligne, "git status", "devenue la plus récente");
        // Casse et barre finale du dossier : même dossier pour Windows
        h.ajouter("ls", "c:\\users\\utilisateur\\projet\\", 200);
        assert_eq!(h.len(), 2);
        // Un autre dossier : une autre entrée
        h.ajouter("ls", AUTRE, 201);
        assert_eq!(h.len(), 3);
    }

    #[test]
    fn la_suggestion_prefere_le_dossier_courant_puis_la_plus_recente() {
        let h = historique(&[("git commit -m ici", D), ("git commit -m ailleurs", AUTRE), ("git push", AUTRE)]);
        // Dans D : celle de D, même si une plus récente existe ailleurs
        assert_eq!(h.suggerer("git c", D), Some("ommit -m ici"));
        // Dans un dossier inconnu : la plus récente qui prolonge
        assert_eq!(h.suggerer("git ", "C:\\autre"), Some("push"));
        assert_eq!(h.suggerer("git commit", AUTRE), Some(" -m ailleurs"));
    }

    #[test]
    fn la_suggestion_ne_dit_rien_sans_prolongement() {
        let h = historique(&[("git status", D)]);
        assert_eq!(h.suggerer("", D), None, "début vide : pas de suggestion");
        assert_eq!(h.suggerer("git status", D), None, "déjà complète");
        assert_eq!(h.suggerer("git statusx", D), None);
        assert_eq!(h.suggerer("Git", D), None, "la casse compte : la fin ajoutée doit prolonger ce qui est tapé");
        assert_eq!(h.suggerer("zzz", D), None);
    }

    #[test]
    fn la_liste_automatique_donne_le_dossier_courant_d_abord_sans_doublon_et_au_plus_max() {
        let h = historique(&[
            ("git push", AUTRE),
            ("git commit -m ici", D),
            ("git status", AUTRE),
            ("git status", D),
            ("git log", AUTRE),
            ("ls", D),
        ]);
        // Dans D : « git status » et « git commit… » (tapées ici, la plus récente d'abord), puis les autres, sans répéter
        assert_eq!(h.proposer("git ", D, 10), vec!["git status", "git commit -m ici", "git log", "git push"]);
        // Le premier est celui que la grise montre
        assert_eq!(h.suggerer("git ", D), Some("status"));
        // Coupé au maximum demandé
        assert_eq!(h.proposer("git ", D, 2), vec!["git status", "git commit -m ici"]);
        // Rien pour un début vide, complet, ou sans prolongement
        assert!(h.proposer("", D, 5).is_empty());
        assert!(h.proposer("ls", D, 5).is_empty());
        assert!(h.proposer("zzz", D, 5).is_empty());
        assert!(h.proposer("git ", D, 0).is_empty());
    }

    #[test]
    fn la_suggestion_coupe_apres_un_caractere_accentue() {
        let h = historique(&[("echo élève", D)]);
        assert_eq!(h.suggerer("echo é", D), Some("lève"), "la coupe tombe sur une frontière de caractère");
    }

    #[test]
    fn le_rappel_remonte_les_lignes_distinctes() {
        let h = historique(&[("a", D), ("b", D), ("a", AUTRE), ("c", D)]);
        // Du plus récent au plus ancien : c, a (tapée dans AUTRE, plus récente), b ; le « a » de D n'est pas répété
        assert_eq!(h.rappeler(0, ""), Some("c"));
        assert_eq!(h.rappeler(1, ""), Some("a"));
        assert_eq!(h.rappeler(2, ""), Some("b"));
        assert_eq!(h.rappeler(3, ""), None);
        assert_eq!(h.rappeler(0, "b"), Some("b"));
        assert_eq!(h.rappeler(1, "b"), None);
    }

    #[test]
    fn le_plafond_retire_les_plus_anciennes() {
        let mut h = Historique::default();
        for i in 0..(MAX_ENTREES + 10) {
            h.ajouter(&format!("cmd {i}"), D, i as u64);
        }
        assert_eq!(h.len(), MAX_ENTREES);
        assert_eq!(h.recente(0).unwrap().ligne, format!("cmd {}", MAX_ENTREES + 9));
        assert_eq!(h.recente(MAX_ENTREES - 1).unwrap().ligne, "cmd 10");
    }

    #[test]
    fn le_fichier_fait_l_aller_retour_avec_tabulations_et_barres() {
        let mut h = Historique::default();
        h.ajouter("echo a\tb\\c", "C:\\dossier\\avec\ttab", 42);
        h.ajouter("dir", D, 43);
        let texte = h.vers_texte();
        assert_eq!(texte.lines().count(), 2, "une entrée, une ligne");
        let relu = Historique::depuis_texte(&texte);
        assert_eq!(relu.recente(1).unwrap(), &Entree { ligne: "echo a\tb\\c".into(), dossier: "C:\\dossier\\avec\ttab".into(), date: 42 });
        assert_eq!(relu.recente(0).unwrap().ligne, "dir");
    }

    #[test]
    fn la_relecture_ignore_l_abime_et_les_secrets_et_replie_les_doublons() {
        let texte = "\u{feff}pas une ligne\n\
                     12\tC:\\a\tgit status\n\
                     abc\tC:\\a\tdate illisible\n\
                     13\tC:\\a\tset PASSWORD=x\n\
                     14\tC:\\a\tls\n\
                     15\tC:\\a\tgit status\n\
                     16\tsans commande\n";
        let h = Historique::depuis_texte(texte);
        assert_eq!(h.len(), 2, "git status (une fois) et ls");
        assert_eq!(h.recente(0).unwrap().ligne, "git status");
        assert_eq!(h.recente(0).unwrap().date, 15);
        assert_eq!(h.recente(1).unwrap().ligne, "ls");
    }

    #[test]
    fn le_fichier_se_lit_se_complete_et_se_reecrit_sur_le_disque() {
        let dossier = std::env::temp_dir().join(format!("spacefiles-historique-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dossier);
        let chemin = dossier.join("sous").join("historique-terminal.txt");
        assert!(charger(&chemin).est_vide(), "fichier absent : historique vide");
        let mut h = Historique::default();
        let e = h.ajouter("cargo test", D, 7).unwrap().clone();
        ajouter_au_fichier(&chemin, &e); // crée aussi le dossier
        let e2 = h.ajouter("cargo build", D, 8).unwrap().clone();
        ajouter_au_fichier(&chemin, &e2);
        ajouter_au_fichier(&chemin, &e); // doublon écrit : replié à la relecture
        let relu = charger(&chemin);
        assert_eq!(relu.len(), 2);
        assert_eq!(relu.recente(0).unwrap().ligne, "cargo test");
        reecrire(&chemin, &relu);
        assert_eq!(std::fs::read_to_string(&chemin).unwrap().lines().count(), 2);
        assert!(!chemin.with_extension("tmp").exists(), "pas de fichier voisin oublié");
        let _ = std::fs::remove_dir_all(&dossier);
    }

    #[test]
    fn l_import_de_psreadline_filtre_les_secrets_et_les_fragments() {
        let texte = "\u{feff}git status\ncargo build\n$env:GITHUB_PAT = 'x'\nGet-ChildItem `\n  -Recurse\n  indente\nnet user a b\ngit status\n\n";
        let h = depuis_psreadline(texte);
        // « git status » (doublon replié), « cargo build » ; le secret, le fragment en accent grave, la ligne en retrait : rien
        assert_eq!(h.len(), 2);
        assert_eq!(h.recente(0).unwrap().ligne, "git status");
        assert_eq!(h.recente(1).unwrap().ligne, "cargo build");
        assert!(h.suggerer("Get-Child", "").is_none());
        assert_eq!(h.suggerer("cargo b", r"C:\x"), Some("uild"), "dossier inconnu : suggéré partout, après ceux du dossier");
    }

    #[test]
    fn l_import_garde_les_dernieres_commandes_seulement() {
        let texte: String = (0..MAX_ENTREES * 3).map(|i| format!("cmd {i}\n")).collect();
        let h = depuis_psreadline(&texte);
        assert_eq!(h.len(), MAX_ENTREES);
        assert_eq!(h.recente(0).unwrap().ligne, format!("cmd {}", MAX_ENTREES * 3 - 1));
    }
}
