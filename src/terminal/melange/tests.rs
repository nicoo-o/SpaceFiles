use super::*;
use std::process::Command;

fn shell(l: &str) -> Shell {
    decider(l).shell
}

fn envoi(l: &str) -> String {
    decider(l).envoi
}

// ---------- décision pure ----------

#[test]
fn une_commande_powershell_ordinaire_ne_change_pas() {
    for l in ["git status", "Get-ChildItem | Select-Object Name", "ls -Force", "cargo build --release", "cd Documents", "echo bonjour"] {
        let d = decider(l);
        assert_eq!(d.shell, Shell::PowerShell, "{l}");
        assert_eq!(d.envoi, l, "la ligne doit partir telle quelle : {l}");
    }
}

#[test]
fn ligne_vide_ou_multiligne_reste_a_powershell() {
    assert_eq!(decider("").shell, Shell::PowerShell);
    assert_eq!(decider("   ").envoi, "   ");
    // Un collage de plusieurs lignes : même avec du cmd dedans, on ne devine pas
    assert_eq!(decider("dir /b\nset X=1").shell, Shell::PowerShell);
    assert_eq!(decider("dir /b\nset X=1").envoi, "dir /b\nset X=1");
}

#[test]
fn prefixe_cmd_force_cmd() {
    assert_eq!(envoi("cmd: dir"), "cmd --% /s /c \"dir\"");
    assert_eq!(envoi("CMD:dir"), "cmd --% /s /c \"dir\"");
    assert_eq!(shell("cmd: Get-Date"), Shell::Cmd);
}

#[test]
fn prefixe_cmd_seul_ouvre_un_cmd_interactif() {
    assert_eq!(envoi("cmd:"), "cmd");
    assert_eq!(envoi("cmd:   "), "cmd");
}

#[test]
fn prefixe_ps_force_powershell_et_se_retire() {
    let d = decider("ps: dir /b");
    assert_eq!(d.shell, Shell::PowerShell);
    assert_eq!(d.envoi, "dir /b");
    assert_eq!(envoi("PS:Get-Date"), "Get-Date");
    assert_eq!(envoi("ps:"), "");
}

#[test]
fn un_mot_qui_commence_comme_un_prefixe_n_en_est_pas_un() {
    // `cmd:` exige les deux-points ; `ps` seul est la commande Get-Process
    assert_eq!(shell("ps"), Shell::PowerShell);
    assert_eq!(envoi("ps -Name explorer"), "ps -Name explorer");
    assert_eq!(shell("cmdlet"), Shell::PowerShell);
}

#[test]
fn les_commandes_propres_a_cmd_sont_reconnues() {
    for l in ["ver", "vol c:", "assoc .txt", "ftype txtfile", "mklink lien cible", "title Mon terminal", "color 0a", "path"] {
        assert_eq!(shell(l), Shell::Cmd, "{l}");
        assert_eq!(envoi(l), format!("cmd --% /s /c \"{l}\""));
    }
}

#[test]
fn les_commutateurs_cmd_sur_un_alias_powershell_passent_par_cmd() {
    for l in ["dir /b", "dir /s /b *.rs", "DIR /AD", "copy /y a.txt b.txt", "del /f /q x", "rd /s /q build", "type /?", "start /wait notepad", "md /x"] {
        assert_eq!(shell(l), Shell::Cmd, "{l}");
    }
    assert_eq!(envoi("dir /b"), "cmd --% /s /c \"dir /b\"");
}

#[test]
fn un_alias_sans_commutateur_reste_a_powershell() {
    for l in ["dir", "dir C:\\Windows", "copy a.txt b.txt", "type notes.txt", "del x.tmp", "dir -Force", "start notepad", "echo /etc/hosts"] {
        assert_eq!(shell(l), Shell::PowerShell, "{l}");
    }
}

#[test]
fn les_variables_pourcent_passent_par_cmd() {
    assert_eq!(shell("echo %USERNAME%"), Shell::Cmd);
    assert_eq!(shell("cd %USERPROFILE%\\Desktop && dir"), Shell::Cmd);
    assert_eq!(shell("echo %ProgramFiles(x86)%"), Shell::Cmd);
    assert_eq!(shell("git log --pretty=oneline -n 3 %TEMP%"), Shell::Cmd);
}

#[test]
fn un_pourcent_de_powershell_n_est_pas_une_variable_cmd() {
    for l in ["Get-Process | % { $_.Name }", "1..5 | %{ $_ * 2 }", "git log --format=\"%h %s\"", "\"50%\" ; \"100%\"", "Get-Date -f 'yyyy' ; 5 % 3"] {
        assert_eq!(shell(l), Shell::PowerShell, "{l}");
    }
    // Une ligne avec `$` est du PowerShell même si deux % entourent un mot
    assert_eq!(shell("$a = '%x%'"), Shell::PowerShell);
}

#[test]
fn les_enchainements_cmd_sont_reconnus_hors_guillemets() {
    for l in ["git add . && git commit -m x", "ping -n 1 hote || echo echec", "echo a & echo b", "build.bat && run.exe"] {
        assert_eq!(shell(l), Shell::Cmd, "{l}");
    }
}

#[test]
fn les_enchainements_dans_une_chaine_ou_un_appel_ne_comptent_pas() {
    for l in ["echo \"a && b\"", "Write-Host 'a || b'", "& 'C:\\Program Files\\app.exe'", "ping x 2>&1", "git commit -m \"fix & test\""] {
        assert_eq!(shell(l), Shell::PowerShell, "{l}");
    }
}

#[test]
fn une_apostrophe_sans_fermante_ne_masque_pas_la_suite() {
    // `it's` n'ouvre pas une chaîne : le && qui suit est vu
    assert_eq!(shell("echo it's ok && echo deux"), Shell::Cmd);
}

#[test]
fn l_auteur_qui_appelle_cmd_lui_meme_n_est_pas_enveloppe() {
    for l in ["cmd /c dir", "cmd --% /c \"echo %X%\"", "cmd.exe /k", "cmd"] {
        let d = decider(l);
        assert_eq!(d.shell, Shell::PowerShell, "{l}");
        assert_eq!(d.envoi, l);
    }
}

#[test]
fn set_devient_une_variable_d_environnement() {
    assert_eq!(envoi("set X=1"), "$env:X = '1'");
    assert_eq!(shell("set X=1"), Shell::Cmd);
    assert_eq!(envoi("SET MON_CHEMIN=C:\\a b\\c"), "$env:MON_CHEMIN = 'C:\\a b\\c'");
    assert_eq!(envoi("set T=l'un"), "$env:T = 'l''un'");
}

#[test]
fn set_avec_des_pourcents_assemble_les_variables() {
    assert_eq!(envoi("set PATH=%PATH%;C:\\outils"), "$env:PATH = $env:PATH + ';C:\\outils'");
    assert_eq!(envoi("set A=x%B%y"), "$env:A = 'x' + $env:B + 'y'");
    assert_eq!(envoi("set A=%B%%C%"), "$env:A = $env:B + $env:C");
    assert_eq!(envoi("set A=%ProgramFiles(x86)%\\z"), "$env:A = ${env:ProgramFiles(x86)} + '\\z'");
}

#[test]
fn set_vide_supprime_la_variable() {
    assert_eq!(envoi("set X="), "Remove-Item Env:X -ErrorAction SilentlyContinue");
}

#[test]
fn set_seul_ou_avec_commutateur_n_est_pas_traduit() {
    assert_eq!(decider("set"), Decision::ps("set"));
    assert_eq!(shell("set PATH"), Shell::PowerShell);
    // `set /p` et `set /a` n'ont pas d'équivalent à une ligne : on les laisse à cmd... ou à PowerShell s'ils n'y sont pas reconnus
    assert_eq!(decider("set /a x=1+2").envoi, "set /a x=1+2");
}

#[test]
fn un_nom_de_variable_invalide_n_est_pas_traduit() {
    assert_eq!(decider("set 1X=2").envoi, "set 1X=2");
    assert_eq!(decider("set A B=2").envoi, "set A B=2");
}

#[test]
fn cd_d_devient_set_location() {
    assert_eq!(envoi("cd /d C:\\Windows"), "Set-Location -LiteralPath 'C:\\Windows' -ErrorAction Stop");
    assert_eq!(envoi("CD /D \"D:\\Mes Documents\""), "Set-Location -LiteralPath 'D:\\Mes Documents' -ErrorAction Stop");
    assert_eq!(envoi("chdir /d e:\\x"), "Set-Location -LiteralPath 'e:\\x' -ErrorAction Stop");
    assert_eq!(shell("cd /d C:\\"), Shell::Cmd);
}

#[test]
fn cd_avec_variable_pourcent_est_developpe() {
    assert_eq!(envoi("cd /d %USERPROFILE%\\Desktop"), "Set-Location -LiteralPath ($env:USERPROFILE + '\\Desktop') -ErrorAction Stop");
    assert_eq!(envoi("cd %TEMP%"), "Set-Location -LiteralPath ($env:TEMP) -ErrorAction Stop");
    assert_eq!(
        envoi("cd \"%USERPROFILE%\\Mes Docs\" && dir /b"),
        "Set-Location -LiteralPath ($env:USERPROFILE + '\\Mes Docs') -ErrorAction Stop; cmd --% /s /c \"dir /b\""
    );
}

#[test]
fn cd_simple_reste_a_powershell() {
    for l in ["cd C:\\Windows", "cd ..", "cd", "cd Documents"] {
        assert_eq!(decider(l), Decision::ps(l), "{l}");
    }
}

#[test]
fn cd_pointpoint_colle_et_cd_antislash() {
    assert_eq!(envoi("cd.."), "Set-Location ..");
    assert_eq!(envoi("cd\\"), "Set-Location \\");
    assert_eq!(envoi("cd.. && dir /b"), "Set-Location ..; cmd --% /s /c \"dir /b\"");
}

#[test]
fn cd_puis_suite_garde_le_dossier_dans_la_session() {
    assert_eq!(
        envoi("cd /d C:\\src && cargo build"),
        "Set-Location -LiteralPath 'C:\\src' -ErrorAction Stop; cmd --% /s /c \"cargo build\""
    );
    assert_eq!(
        envoi("cd \"C:\\Mes Projets\" && git status && git pull"),
        "Set-Location -LiteralPath 'C:\\Mes Projets' -ErrorAction Stop; cmd --% /s /c \"git status && git pull\""
    );
    assert_eq!(
        envoi("cd C:\\x && dir /b"),
        "Set-Location -LiteralPath 'C:\\x' -ErrorAction Stop; cmd --% /s /c \"dir /b\""
    );
}

#[test]
fn cd_avec_une_suite_incomplete_n_est_pas_traduit() {
    // `&&` sans rien après, ou autre chose qu'un enchaînement : on ne touche à rien (PowerShell répondra son erreur)
    assert_eq!(shell("cd C:\\x &&"), Shell::Cmd);
    assert_eq!(decider("cd C:\\x toto").envoi, "cd C:\\x toto");
}

#[test]
fn le_prefixe_cmd_traduit_aussi() {
    assert_eq!(envoi("cmd: set X=2"), "$env:X = '2'");
    assert_eq!(envoi("cmd: cd /d C:\\x"), "Set-Location -LiteralPath 'C:\\x' -ErrorAction Stop");
    assert_eq!(envoi("cmd: echo %PATH%"), "cmd --% /s /c \"echo %PATH%\"");
}

#[test]
fn guillemets_et_tubes_restent_dans_la_ligne_envoyee_a_cmd() {
    assert_eq!(envoi("dir /b | find \"win\""), "cmd --% /s /c \"dir /b | find \"win\"\"");
    assert_eq!(envoi("\"C:\\Program Files\\x.exe\" /? && echo ok"), "cmd --% /s /c \"\"C:\\Program Files\\x.exe\" /? && echo ok\"");
}

#[test]
fn espaces_autour_de_la_ligne_sont_ignores_pour_decider() {
    assert_eq!(shell("   dir /b   "), Shell::Cmd);
    assert_eq!(envoi("  cmd:  ver  "), "cmd --% /s /c \"ver\"");
}

#[test]
fn les_accents_ne_cassent_pas_la_decision() {
    assert_eq!(shell("echo é && echo à"), Shell::Cmd);
    assert_eq!(envoi("set NOM=Hélène"), "$env:NOM = 'Hélène'");
    assert_eq!(shell("Écrire-Quelque-Chose é"), Shell::PowerShell);
    // Un caractère de plusieurs octets au début ne fait pas paniquer la recherche du préfixe
    assert_eq!(shell("éé"), Shell::PowerShell);
    assert_eq!(shell("é"), Shell::PowerShell);
}

#[test]
fn commutateur_ou_chemin() {
    assert!(est_commutateur("/b"));
    assert!(est_commutateur("/s"));
    assert!(est_commutateur("/AD"));
    assert!(est_commutateur("/a:d"));
    assert!(est_commutateur("/?"));
    assert!(est_commutateur("/wait"));
    assert!(!est_commutateur("/"));
    assert!(!est_commutateur("/etc/hosts"));
    assert!(!est_commutateur("/beaucoup"));
    assert!(!est_commutateur("-b"));
    assert!(!est_commutateur("C:\\x"));
}

#[test]
fn valeur_en_powershell_cas_limites() {
    assert_eq!(valeur_en_powershell("abc"), "'abc'");
    assert_eq!(valeur_en_powershell("50%"), "'50%'");
    assert_eq!(valeur_en_powershell("%A%"), "$env:A");
    assert_eq!(valeur_en_powershell("a % b %c"), "'a % b %c'");
    assert_eq!(valeur_en_powershell("%a b%"), "'%a b%'");
}

// ---------- avec le vrai PowerShell 5.1 (et le vrai cmd.exe) ----------

/// Encodage base64 d'un texte UTF-16LE, la forme que `-EncodedCommand` attend : ni guillemets ni échappements à deviner
fn base64_utf16(s: &str) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let octets: Vec<u8> = s.encode_utf16().flat_map(u16::to_le_bytes).collect();
    let mut sortie = String::new();
    for bloc in octets.chunks(3) {
        let n = (u32::from(bloc[0]) << 16) | (u32::from(*bloc.get(1).unwrap_or(&0)) << 8) | u32::from(*bloc.get(2).unwrap_or(&0));
        for k in 0..4 {
            if k <= bloc.len() {
                sortie.push(T[((n >> (18 - 6 * k)) & 63) as usize] as char);
            } else {
                sortie.push('=');
            }
        }
    }
    sortie
}

/// Joue `script` dans un vrai PowerShell 5.1 (dossier de départ `C:\Windows`) et rend sa sortie standard
fn jouer(script: &str) -> String {
    let racine = std::env::var("SystemRoot").unwrap_or_else(|_| "C:\\Windows".to_string());
    let exe = format!("{racine}\\System32\\WindowsPowerShell\\v1.0\\powershell.exe");
    // Entrée standard en tube (fermé à la fin) et non nulle : `find` dans un tube cmd ne tient pas un handle d'entrée nul
    let sortie = Command::new(exe)
        .args(["-NoProfile", "-NonInteractive", "-EncodedCommand", &base64_utf16(script)])
        .current_dir(&racine)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .and_then(|e| e.wait_with_output())
        .expect("lancement de powershell.exe");
    if !sortie.stderr.is_empty() {
        eprintln!("stderr de PowerShell : {}", String::from_utf8_lossy(&sortie.stderr));
    }
    String::from_utf8_lossy(&sortie.stdout).replace("\r\n", "\n")
}

#[test]
fn base64_connu() {
    // « A » en UTF-16LE = 41 00 → « QQA= »
    assert_eq!(base64_utf16("A"), "QQA=");
    assert_eq!(base64_utf16("AB"), "QQBCAA==");
}

#[test]
fn vrai_dir_b_via_cmd() {
    let d = decider("dir /b win.ini");
    assert_eq!(d.shell, Shell::Cmd);
    assert_eq!(jouer(&d.envoi).trim(), "win.ini");
}

#[test]
fn vrai_dir_b_sans_traduction_echoue_sous_powershell() {
    // La raison d'être du module : sans lui, le même `dir /b win.ini` ne donne pas le nom du fichier
    assert!(!jouer("dir /b win.ini").contains("win.ini\n"));
}

#[test]
fn vrai_tube_et_guillemets_arrivent_a_cmd() {
    // `findstr` et non `find` : lancé depuis Git Bash, le PATH met le `find` d'Unix devant celui de Windows
    let d = decider("dir /b win.ini | findstr \"win\"");
    assert_eq!(jouer(&d.envoi).trim(), "win.ini");
}

#[test]
fn vrai_et_commercial_et_variable_pourcent() {
    let d = decider("echo %USERNAME%-ok && echo deux");
    let attendu = format!("{}-ok \ndeux\n", std::env::var("USERNAME").unwrap());
    assert_eq!(jouer(&d.envoi), attendu);
}

#[test]
fn vrai_ver_n_existe_que_dans_cmd() {
    let d = decider("ver");
    assert!(jouer(&d.envoi).contains("Microsoft Windows"));
    assert!(!jouer("ver").contains("Microsoft Windows"));
}

#[test]
fn vrai_set_garde_la_variable_dans_la_session() {
    let script = format!("{}\n{}\n$env:ESSAI_T6_A", envoi("set ESSAI_T6_A=un"), envoi("set ESSAI_T6_A=%ESSAI_T6_A%-deux"));
    assert_eq!(jouer(&script).trim(), "un-deux");
    // `set X=` la retire
    let script = format!("{}\n{}\n\"[\" + $env:ESSAI_T6_B + \"]\"", envoi("set ESSAI_T6_B=x"), envoi("set ESSAI_T6_B="));
    assert_eq!(jouer(&script).trim(), "[]");
}

#[test]
fn vrai_set_avec_apostrophe_et_espaces() {
    // ASCII seulement dans la valeur : la sortie du processus fils passe par la page de codes OEM
    let script = format!("{}\n$env:ESSAI_T6_C", envoi("set ESSAI_T6_C=l'ete de Jean"));
    assert_eq!(jouer(&script).trim(), "l'ete de Jean");
}

#[test]
fn vrai_cd_d_change_le_dossier_de_la_session() {
    let script = format!("{}\n(Get-Location).Path", envoi("cd /d C:\\Windows\\System32"));
    assert_eq!(jouer(&script).trim(), "C:\\Windows\\System32");
}

#[test]
fn vrai_cd_puis_suite_change_le_dossier_et_lance_la_suite() {
    let script = format!("{}\n(Get-Location).Path", envoi("cd /d C:\\Windows\\System32 && echo ici"));
    assert_eq!(jouer(&script), "ici\nC:\\Windows\\System32\n");
}

#[test]
fn vrai_cd_qui_echoue_abandonne_la_suite() {
    // Comme cmd : `cd inexistant && suite` ne lance pas la suite
    let sortie = jouer(&envoi("cd /d C:\\dossier_qui_n_existe_pas_t6 && echo JAMAIS"));
    assert!(!sortie.contains("JAMAIS"), "la suite a été jouée : {sortie:?}");
}

#[test]
fn vrai_cd_pointpoint() {
    let script = format!("Set-Location C:\\Windows\\System32\n{}\n(Get-Location).Path", envoi("cd.."));
    assert_eq!(jouer(&script).trim(), "C:\\Windows");
}

#[test]
fn vrai_prefixe_ps_et_ligne_powershell_inchangee() {
    assert_eq!(jouer(&envoi("ps: Write-Output bonjour")).trim(), "bonjour");
    assert_eq!(jouer(&envoi("Get-ChildItem C:\\Windows\\win.ini | Select-Object -ExpandProperty Name")).trim(), "win.ini");
}

#[test]
fn vrai_prefixe_cmd() {
    let d = decider("cmd: echo ok");
    assert_eq!(jouer(&d.envoi).trim(), "ok");
}
