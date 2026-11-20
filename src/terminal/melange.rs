//! T6 : shell mixte. Le terminal n'ouvre que PowerShell 5.1, mais l'auteur (et un débutant qui copie une commande d'un
//! tutoriel) tape aussi du cmd : `dir /b`, `set X=1`, `echo %PATH%`, `a && b`. PowerShell 5.1 répond par une erreur ou, pire,
//! fait autre chose (`set x=1` crée une variable nommée « x=1 »). Ce module décide, pour une ligne soumise, ce qu'il faut
//! réellement envoyer au shell : la ligne telle quelle, ou une version qui passe par `cmd.exe`.
//!
//! Module pur (ni fenêtre ni shell) : il se teste sans écran. Règles, de la plus forte à la plus faible :
//! 1. Préfixe `cmd:` ou `ps:` : l'auteur tranche, on lui obéit.
//! 2. Traductions qui gardent l'effet sur la session (un `cmd.exe` fils les perdrait à sa fin) : `set X=Y` → `$env:X`,
//!    `cd /d X` → `Set-Location`, `cd X && suite` → `Set-Location` puis la suite dans cmd.
//! 3. Détection : commande propre à cmd, `%VAR%`, `&&` `||` ` & ` hors guillemets, commande à commutateur `/x` que PowerShell
//!    prendrait pour un chemin (`dir /b`).
//! 4. Sinon PowerShell, sans rien changer.
//!
//! Limite assumée : une ligne envoyée à cmd s'exécute dans un cmd fils, qui hérite du dossier et de l'environnement mais ne
//! les change pas pour la suite (sauf les traductions de la règle 2). La ligne entière passe à cmd : pas de pipeline
//! PowerShell mélangé à du cmd dans la même ligne.

use super::session::Shell;

/// Ce qu'une ligne soumise devient
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Decision {
    /// La syntaxe reconnue (sert au badge CMD / PS) ; une traduction en PowerShell reste `Cmd` : c'est du cmd que l'auteur a tapé
    pub shell: Shell,
    /// Ce qu'on envoie vraiment au shell (sans la touche Entrée)
    pub envoi: String,
}

impl Decision {
    fn ps(envoi: &str) -> Self {
        Decision { shell: Shell::PowerShell, envoi: envoi.to_string() }
    }
    fn cmd(envoi: String) -> Self {
        Decision { shell: Shell::Cmd, envoi }
    }
}

/// Premiers mots qui n'existent que dans cmd : sous PowerShell 5.1 ils donnent « terme non reconnu » (vérifié avec `Get-Command`)
const CMD_SEULEMENT: [&str; 10] = ["ver", "vol", "assoc", "ftype", "mklink", "title", "color", "verify", "path", "rem"];

/// Alias PowerShell de commandes internes de cmd : `dir /b` appelle `Get-ChildItem /b`, un chemin qui n'existe pas
const ALIAS_AVEC_COMMUTATEUR: [&str; 14] =
    ["dir", "copy", "del", "erase", "type", "md", "mkdir", "rd", "rmdir", "move", "ren", "rename", "start", "echo"];

/// Ce qu'on envoie au shell pour une ligne soumise, et la syntaxe reconnue
pub fn decider(ligne: &str) -> Decision {
    let t = ligne.trim();
    // Une ligne multiligne (collage) ou vide reste à PowerShell : on ne devine rien sur ce qu'on ne voit pas en entier
    if t.is_empty() || t.contains('\n') {
        return Decision::ps(ligne);
    }
    if let Some(reste) = retirer_prefixe(t, "cmd:") {
        return vers_cmd(reste);
    }
    if let Some(reste) = retirer_prefixe(t, "ps:") {
        return Decision::ps(reste);
    }
    // L'auteur appelle déjà cmd lui-même (`cmd /c …`, `cmd --% …`) : l'envelopper une seconde fois casserait sa ligne
    if matches!(mot_et_reste(t).0.as_str(), "cmd" | "cmd.exe") {
        return Decision::ps(ligne);
    }
    if let Some(traduit) = traduire(t) {
        return Decision::cmd(traduit);
    }
    if est_cmd(t) {
        return Decision::cmd(envelopper(t));
    }
    Decision::ps(ligne)
}

/// `cmd:` ou `ps:` au début (casse libre), suivi ou non d'espaces ; renvoie ce qui suit
fn retirer_prefixe<'a>(t: &'a str, prefixe: &str) -> Option<&'a str> {
    let tete = t.get(..prefixe.len())?;
    tete.eq_ignore_ascii_case(prefixe).then(|| t[prefixe.len()..].trim_start())
}

/// La ligne passée à `cmd.exe` : `--%` arrête l'analyse de PowerShell, `/s` fait retirer les guillemets extérieurs seulement
/// (vérifié sous PowerShell 5.1 : `|`, `&&`, `%VAR%`, `>nul` et un exécutable entre guillemets arrivent intacts à cmd)
fn envelopper(commande: &str) -> String {
    format!("cmd --% /s /c \"{commande}\"")
}

/// Une ligne que l'auteur a marquée `cmd:`
fn vers_cmd(reste: &str) -> Decision {
    if reste.is_empty() {
        // `cmd:` seul ouvre un cmd interactif dans le terminal
        return Decision::cmd("cmd".to_string());
    }
    match traduire(reste) {
        Some(traduit) => Decision::cmd(traduit),
        None => Decision::cmd(envelopper(reste)),
    }
}

/// Premier mot en minuscules et le reste (après les espaces)
fn mot_et_reste(t: &str) -> (String, &str) {
    match t.find(char::is_whitespace) {
        Some(i) => (t[..i].to_ascii_lowercase(), t[i..].trim_start()),
        None => (t.to_ascii_lowercase(), ""),
    }
}

fn est_nom_variable(s: &str) -> bool {
    let mut c = s.chars();
    matches!(c.next(), Some(p) if p.is_ascii_alphabetic() || p == '_') && c.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// Une chaîne PowerShell littérale (apostrophes doublées)
fn litteral(s: &str) -> String {
    format!("'{}'", s.replace('\'', "''"))
}

/// `$env:NOM` (entre accolades si le nom a des caractères spéciaux, comme `ProgramFiles(x86)`)
fn variable_env(nom: &str) -> String {
    if est_nom_variable(nom) {
        format!("$env:{nom}")
    } else {
        format!("${{env:{nom}}}")
    }
}

/// Les `%NOM%` d'une valeur cmd deviennent des `$env:NOM` PowerShell, le reste des littéraux : `a%X%b` → `'a' + $env:X + 'b'`
fn valeur_en_powershell(valeur: &str) -> String {
    let mut morceaux: Vec<String> = Vec::new();
    let mut courant = String::new();
    let car: Vec<char> = valeur.chars().collect();
    let mut i = 0;
    while i < car.len() {
        if car[i] == '%' {
            if let Some(fin) = (i + 1..car.len()).find(|&j| car[j] == '%') {
                let nom: String = car[i + 1..fin].iter().collect();
                if nom_pourcent_valide(&nom) {
                    if !courant.is_empty() {
                        morceaux.push(litteral(&courant));
                        courant.clear();
                    }
                    morceaux.push(variable_env(&nom));
                    i = fin + 1;
                    continue;
                }
            }
        }
        courant.push(car[i]);
        i += 1;
    }
    if !courant.is_empty() || morceaux.is_empty() {
        morceaux.push(litteral(&courant));
    }
    morceaux.join(" + ")
}

/// Le nom entre deux `%` d'une variable cmd : lettres, chiffres, `_`, parenthèses (`ProgramFiles(x86)`)
fn nom_pourcent_valide(nom: &str) -> bool {
    !nom.is_empty() && nom.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '(' || c == ')')
}

/// Un chemin tapé après `cd` : entre guillemets, ou un seul mot sans `&`
fn chemin_apres_cd(s: &str) -> Option<(String, &str)> {
    let s = s.trim_start();
    if let Some(suite) = s.strip_prefix('"') {
        let fin = suite.find('"')?;
        return Some((suite[..fin].to_string(), suite[fin + 1..].trim_start()));
    }
    let fin = s.find(char::is_whitespace).unwrap_or(s.len());
    let mot = &s[..fin];
    if mot.is_empty() || mot.contains('&') {
        return None;
    }
    Some((mot.to_string(), s[fin..].trim_start()))
}

/// Les traductions qui gardent l'effet sur la session. `None` : pas concerné.
fn traduire(t: &str) -> Option<String> {
    let (mot, reste) = mot_et_reste(t);
    match mot.as_str() {
        // `cd..` et `cd\` sont valides dans cmd seulement
        "cd.." => Some(format!("Set-Location ..{}", suite_apres_cd(reste)?)),
        "cd\\" => Some(format!("Set-Location \\{}", suite_apres_cd(reste)?)),
        "cd" | "chdir" => traduire_cd(reste),
        "set" => traduire_set(reste),
        _ => None,
    }
}

/// Ce qui suit `cd..` : rien, ou `&& suite` (renvoyé sous forme de fin de ligne PowerShell)
fn suite_apres_cd(reste: &str) -> Option<String> {
    if reste.is_empty() {
        return Some(String::new());
    }
    let suite = reste.strip_prefix("&&")?.trim();
    (!suite.is_empty()).then(|| format!("; {}", envelopper(suite)))
}

/// `cd /d X`, `cd /d X && suite`, `cd X && suite` ; un `cd X` simple reste à PowerShell (même effet)
fn traduire_cd(reste: &str) -> Option<String> {
    let (avec_d, apres) = match reste.get(..2) {
        Some(d) if d.eq_ignore_ascii_case("/d") && reste[2..].starts_with(char::is_whitespace) => (true, reste[2..].trim_start()),
        _ => (false, reste),
    };
    let (chemin, fin) = chemin_apres_cd(apres)?;
    // Un `%VAR%` dans le chemin doit être développé ici : PowerShell ne le ferait pas
    let (cible, avec_variable) = if contient_variable_pourcent(&chemin) {
        (format!("({})", valeur_en_powershell(&chemin)), true)
    } else {
        (litteral(&chemin), false)
    };
    if fin.is_empty() {
        // `cd X` seul : PowerShell sait le faire ; `cd /d X` et `cd %VAR%` non
        return (avec_d || avec_variable).then(|| format!("Set-Location -LiteralPath {cible} -ErrorAction Stop"));
    }
    let suite = fin.strip_prefix("&&")?.trim();
    if suite.is_empty() {
        return None;
    }
    // La suite ne part que si le `cd` a réussi : `-ErrorAction Stop` abandonne le reste de la ligne PowerShell (vérifié)
    Some(format!("Set-Location -LiteralPath {cible} -ErrorAction Stop; {}", envelopper(suite)))
}

/// `set NOM=valeur` → `$env:NOM = …` ; `set NOM=` → suppression. `set /p`, `set /a`, `set` seul, `set PREFIXE` : pas concerné
fn traduire_set(reste: &str) -> Option<String> {
    let egal = reste.find('=')?;
    let (nom, valeur) = (&reste[..egal], &reste[egal + 1..]);
    if !est_nom_variable(nom) {
        return None;
    }
    if valeur.is_empty() {
        return Some(format!("Remove-Item Env:{nom} -ErrorAction SilentlyContinue"));
    }
    Some(format!("$env:{nom} = {}", valeur_en_powershell(valeur)))
}

/// Le texte sans ce qui est entre guillemets (remplacé par des espaces) : `&&` dans une chaîne ne compte pas. Une
/// apostrophe ou un guillemet sans fermant est un caractère ordinaire (`echo it's`).
fn sans_chaines(t: &str) -> String {
    let car: Vec<char> = t.chars().collect();
    let mut sortie = String::with_capacity(t.len());
    let mut i = 0;
    while i < car.len() {
        let c = car[i];
        if c == '"' || c == '\'' {
            if let Some(fin) = (i + 1..car.len()).find(|&j| car[j] == c) {
                sortie.extend(std::iter::repeat(' ').take(fin - i + 1));
                i = fin + 1;
                continue;
            }
        }
        sortie.push(c);
        i += 1;
    }
    sortie
}

/// `%NOM%` quelque part dans la ligne (hors guillemets simples de PowerShell : une ligne avec `$` est du PowerShell)
fn contient_variable_pourcent(t: &str) -> bool {
    if t.contains('$') {
        return false;
    }
    let car: Vec<char> = t.chars().collect();
    let mut i = 0;
    while i < car.len() {
        if car[i] == '%' {
            if let Some(fin) = (i + 1..car.len()).find(|&j| car[j] == '%') {
                let nom: String = car[i + 1..fin].iter().collect();
                if nom_pourcent_valide(&nom) {
                    return true;
                }
            }
        }
        i += 1;
    }
    false
}

/// `&&`, `||` ou ` & ` hors guillemets : PowerShell 5.1 ne les connaît pas (le `&` d'appel au début d'une ligne reste à lui)
fn contient_enchainement(t: &str) -> bool {
    let nue = sans_chaines(t);
    nue.contains("&&") || nue.contains("||") || nue.contains(" & ")
}

/// Un argument de la forme `/b`, `/s`, `/a:d`, `/?` : un commutateur cmd, pas un chemin
fn est_commutateur(arg: &str) -> bool {
    let Some(corps) = arg.strip_prefix('/') else { return false };
    let (lettres, _suite) = match corps.split_once(':') {
        Some((l, s)) => (l, Some(s)),
        None => (corps, None),
    };
    (1..=6).contains(&lettres.len()) && lettres.chars().all(|c| c.is_ascii_alphanumeric() || c == '?')
}

fn est_cmd(t: &str) -> bool {
    let (mot, reste) = mot_et_reste(t);
    if CMD_SEULEMENT.contains(&mot.as_str()) || contient_enchainement(t) {
        return true;
    }
    if contient_variable_pourcent(t) {
        return true;
    }
    ALIAS_AVEC_COMMUTATEUR.contains(&mot.as_str()) && reste.split_whitespace().any(est_commutateur)
}

#[cfg(test)]
mod tests;
