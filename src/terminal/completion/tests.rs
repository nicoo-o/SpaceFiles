use super::*;
use std::path::PathBuf;

/// Un dossier de travail neuf, supprimé à la fin du test
struct Atelier(PathBuf);

impl Atelier {
    fn nouveau(nom: &str) -> Self {
        let d = std::env::temp_dir().join(format!("spacefiles_t7_{}_{}", nom, std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        Atelier(d)
    }
    fn dossier(&self) -> &str {
        self.0.to_str().unwrap()
    }
    fn fichier(&self, rel: &str) {
        let p = self.0.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, b"x").unwrap();
    }
    fn creer_dossier(&self, rel: &str) {
        std::fs::create_dir_all(self.0.join(rel)).unwrap();
    }
}

impl Drop for Atelier {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn noms(p: &Proposition) -> Vec<&str> {
    p.candidats.iter().map(|c| c.nom.as_str()).collect()
}

#[test]
fn le_dernier_jeton_garde_sa_place_et_ses_guillemets() {
    let j = dernier_jeton("git ch");
    assert_eq!((j.debut, j.brut.as_str(), j.rang, j.commande.as_str()), (4, "ch", 1, "git"));
    let j = dernier_jeton("git ");
    assert_eq!((j.debut, j.brut.as_str(), j.rang), (4, "", 1));
    let j = dernier_jeton(r#"cd "C:\Program Fi"#);
    assert_eq!((j.debut, j.brut.as_str(), j.rang, j.commande.as_str()), (3, r#""C:\Program Fi"#, 1, "cd"));
    // `| ; &` ouvrent une nouvelle commande
    let j = dernier_jeton("dir | fin");
    assert_eq!((j.debut, j.rang), (6, 0));
    let j = dernier_jeton("cd a && gi");
    assert_eq!((j.brut.as_str(), j.rang), ("gi", 0));
    // les préfixes de T6 ne sont pas un mot
    let j = dernier_jeton("cmd: dir C");
    assert_eq!((j.rang, j.commande.as_str()), (1, "dir"));
    let j = dernier_jeton("ps: ls");
    assert_eq!(j.rang, 0);
    // un accent ne décale pas l'index (en caractères, pas en octets)
    let j = dernier_jeton("écho ab");
    assert_eq!(j.debut, 5);
}

#[test]
fn un_guillemet_ferme_ou_ouvert_se_retire_du_prefixe() {
    assert_eq!(sans_guillemets(r#""ab c"#), ("ab c".to_string(), true));
    assert_eq!(sans_guillemets(r#""ab c""#), ("ab c".to_string(), true));
    assert_eq!(sans_guillemets("abc"), ("abc".to_string(), false));
    assert_eq!(sans_guillemets("\""), (String::new(), true));
}

#[test]
fn les_chemins_donnent_dossiers_d_abord_avec_un_antislash() {
    let a = Atelier::nouveau("chemins");
    a.fichier("alpha.txt");
    a.creer_dossier("avion");
    a.fichier("beta.txt");
    let p = completer_avec("type a", a.dossier(), &[]);
    assert_eq!(noms(&p), ["avion\\", "alpha.txt"]);
    assert_eq!(p.debut, 5);
    assert!(p.chemins);
    // casse ignorée
    assert_eq!(noms(&completer_avec("type AL", a.dossier(), &[])), ["alpha.txt"]);
    // un nom sans correspondance : aucune proposition
    assert!(completer_avec("type zzz", a.dossier(), &[]).candidats.is_empty());
}

#[test]
fn un_chemin_tape_avec_slash_garde_son_slash_et_descend_d_un_niveau() {
    let a = Atelier::nouveau("sous");
    a.fichier("src/main.rs");
    a.fichier("src/mod.rs");
    a.creer_dossier("src/mur");
    let p = completer_avec("cat src/m", a.dossier(), &[]);
    assert_eq!(noms(&p), ["mur/", "main.rs", "mod.rs"]);
    assert_eq!(p.candidats[1].brut, "src/main.rs");
    // le dossier tapé avec antislash
    let p = completer_avec(r"cat src\mai", a.dossier(), &[]);
    assert_eq!(p.candidats[0].brut, r"src\main.rs");
}

#[test]
fn un_nom_avec_espace_est_cite_le_dossier_laisse_son_guillemet_ouvert() {
    let a = Atelier::nouveau("espace");
    a.creer_dossier("Mes Documents");
    a.fichier("Mon Fichier.txt");
    let p = completer_avec("cd M", a.dossier(), &[]);
    // cd : dossiers seulement
    assert_eq!(noms(&p), ["Mes Documents\\"]);
    assert_eq!(p.texte_de(&p.candidats[0]), "\"Mes Documents\\");
    let p = completer_avec("type Mo", a.dossier(), &[]);
    assert_eq!(p.texte_de(&p.candidats[0]), "\"Mon Fichier.txt\"");
    // le jeton déjà entre guillemets garde le sien, même sans espace dans le nom
    a.fichier("simple.txt");
    let p = completer_avec("type \"sim", a.dossier(), &[]);
    assert_eq!(p.texte_de(&p.candidats[0]), "\"simple.txt\"");
    // un nom sans signe spécial n'est pas cité
    let p = completer_avec("type sim", a.dossier(), &[]);
    assert_eq!(p.texte_de(&p.candidats[0]), "simple.txt");
}

#[test]
fn un_dossier_cite_ferme_se_recomplete_dans_son_contenu() {
    let a = Atelier::nouveau("cite");
    a.fichier("Mes Documents/note.txt");
    let ligne = r#"type "Mes Documents\"#;
    let p = completer_avec(ligne, a.dossier(), &[]);
    assert_eq!(noms(&p), ["note.txt"]);
    assert_eq!(p.texte_de(&p.candidats[0]), "\"Mes Documents\\note.txt\"");
    // le jeton fermé par son guillemet est relu de même
    let p = completer_avec(r#"type "Mes Documents\no""#, a.dossier(), &[]);
    assert_eq!(noms(&p), ["note.txt"]);
}

#[test]
fn les_noms_caches_ne_viennent_qu_avec_une_lettre_tapee() {
    let a = Atelier::nouveau("caches");
    a.fichier("visible.txt");
    a.fichier("zz.txt");
    // Windows : « caché » est un attribut ; on le pose avec attrib (le point seul ne cache rien)
    a.fichier("secret.txt");
    let chemin = a.0.join("secret.txt");
    let ok = std::process::Command::new("attrib").arg("+H").arg(&chemin).status().map(|s| s.success()).unwrap_or(false);
    assert!(ok, "attrib +H a échoué : le test ne prouve rien");
    assert_eq!(noms(&completer_avec("type ", a.dossier(), &[])), ["visible.txt", "zz.txt"]);
    assert_eq!(noms(&completer_avec("type s", a.dossier(), &[])), ["secret.txt"]);
}

#[test]
fn au_premier_mot_ce_sont_des_commandes_du_path_des_integrees_et_des_programmes_connus() {
    let programmes = vec!["gitk".to_string(), "Gimp".to_string(), "notepad".to_string()];
    let p = completer_avec("gi", ".", &programmes);
    let n = noms(&p);
    assert_eq!(n, ["Gimp", "git", "gitk"]);
    let git = p.candidats.iter().find(|c| c.nom == "git").unwrap();
    assert_eq!(git.genre, Genre::Commande);
    assert_eq!(git.description, "Gestion de versions");
    // intégrée : description française
    let p = completer_avec("mkd", ".", &programmes);
    assert_eq!(noms(&p), ["mkdir"]);
    assert_eq!(p.candidats[0].description, "Crée un dossier");
    // cmdlet, casse ignorée
    assert_eq!(noms(&completer_avec("get-ch", ".", &[])), ["Get-ChildItem"]);
    // ligne vide ou mot vide : trop de choix, aucune proposition
    assert!(completer_avec("", ".", &programmes).candidats.is_empty());
    assert!(completer_avec("ls; ", ".", &programmes).candidats.is_empty());
    // un programme du PATH sans doublon avec une intégrée
    let p = completer_avec("not", ".", &programmes);
    assert_eq!(noms(&p), ["notepad"]);
}

#[test]
fn apres_un_pipe_on_complete_encore_une_commande() {
    let p = completer_avec("dir | sele", ".", &[]);
    assert_eq!(noms(&p), ["Select-Object", "Select-String"]);
}

#[test]
fn les_sous_commandes_et_les_options_viennent_de_la_base() {
    let p = completer_avec("git ch", ".", &[]);
    assert_eq!(noms(&p), ["checkout", "cherry-pick"]);
    assert_eq!(p.candidats[0].genre, Genre::SousCommande);
    assert!(!p.chemins);
    assert_eq!(p.debut, 4);
    // les options commencent par un tiret
    let p = completer_avec("cargo build --rel", ".", &[]);
    assert_eq!(noms(&p), ["--release"]);
    assert_eq!(p.candidats[0].genre, Genre::Option);
    // le nom du programme tapé avec .exe ou un chemin reste reconnu
    assert_eq!(noms(&completer_avec("git.exe stat", ".", &[])), ["status"]);
    assert_eq!(noms(&completer_avec("GIT stat", ".", &[])), ["status"]);
    // `git <Tab>` : toutes les sous-commandes, aucun fichier
    let a = Atelier::nouveau("git");
    a.fichier("lisez-moi.txt");
    let p = completer_avec("git ", a.dossier(), &[]);
    assert!(p.candidats.iter().all(|c| c.genre == Genre::SousCommande));
    assert!(p.candidats.len() >= 25);
    // un second mot qui n'est aucune sous-commande est un fichier
    let p = completer_avec("git add lis", a.dossier(), &[]);
    assert_eq!(noms(&p), ["lisez-moi.txt"]);
    let p = completer_avec("git lis", a.dossier(), &[]);
    assert_eq!(noms(&p), ["lisez-moi.txt"]);
    // un programme inconnu : pas d'option inventée
    assert!(completer_avec("inconnu --ve", ".", &[]).candidats.is_empty());
}

#[test]
fn le_prefixe_commun_avance_la_frappe() {
    let a = Atelier::nouveau("commun");
    a.fichier("Programme-un.txt");
    a.fichier("programme-deux.txt");
    let p = completer_avec("type pro", a.dossier(), &[]);
    assert_eq!(p.prefixe_commun().as_deref(), Some("programme-"));
    // rien de plus à ajouter : pas de préfixe commun utile
    let p = completer_avec("type programme-", a.dossier(), &[]);
    assert_eq!(p.prefixe_commun(), None);
    // les sous-commandes `stash` et `status` partagent « sta », déjà tapé : rien à avancer
    let p = completer_avec("git sta", ".", &[]);
    assert_eq!(p.candidats.len(), 2);
    assert_eq!(p.prefixe_commun(), None);
    let p = completer_avec("cargo cl", ".", &[]);
    assert_eq!(noms(&p), ["clean", "clippy"]);
    assert_eq!(p.prefixe_commun(), None);
    // un seul candidat : le début commun est le candidat entier
    let p = completer_avec("rustup co", ".", &[]);
    assert_eq!(p.prefixe_commun().as_deref(), Some("component"));
    // une commune qui contient un espace est citée, guillemet ouvert
    let b = Atelier::nouveau("commun2");
    b.fichier("Mon Fichier a.txt");
    b.fichier("Mon Fichier b.txt");
    let p = completer_avec("type Mo", b.dossier(), &[]);
    assert_eq!(p.prefixe_commun().as_deref(), Some("\"Mon Fichier "));
}

#[test]
fn les_programmes_du_path_se_lisent_sans_extension_ni_doublon() {
    let a = Atelier::nouveau("path");
    a.fichier("outil.exe");
    a.fichier("Outil.cmd");
    a.fichier("autre.bat");
    a.fichier("script.ps1");
    a.fichier("notes.txt");
    let liste = lire_path(std::ffi::OsStr::new(a.dossier()));
    assert_eq!(liste.len(), 3, "{liste:?}");
    assert!(liste.iter().any(|n| n.eq_ignore_ascii_case("outil")));
    assert!(liste.iter().any(|n| n == "autre"));
    assert!(liste.iter().any(|n| n == "script"));
    assert!(!liste.iter().any(|n| n.contains("notes")));
    // un dossier inexistant dans le PATH est ignoré
    assert!(lire_path(std::ffi::OsStr::new(r"Z:\n'existe\pas")).is_empty());
}

#[test]
fn au_premier_mot_un_chemin_ne_propose_que_dossiers_et_executables() {
    let a = Atelier::nouveau("exec");
    a.fichier("outil.exe");
    a.fichier("notes.txt");
    a.creer_dossier("outils");
    let p = completer_avec(r".\o", a.dossier(), &[]);
    assert_eq!(noms(&p), ["outils\\", "outil.exe"]);
    assert_eq!(p.candidats[1].brut, r".\outil.exe");
}

#[test]
fn un_chemin_absolu_se_lit_depuis_sa_racine() {
    // Windows est sur toute machine qui lance ces tests
    let p = completer_avec(r"dir C:\Windows\Sys", r"D:\ailleurs", &[]);
    assert!(noms(&p).contains(&"System32\\"), "{:?}", noms(&p));
    assert_eq!(p.candidats[0].brut, r"C:\Windows\System32\");
}

#[test]
fn un_dossier_enorme_ou_inexistant_ne_plante_pas() {
    assert!(completer_avec(r"type Z:\nulle\part\a", ".", &[]).candidats.is_empty());
    assert!(completer_avec("type a", "", &[]).candidats.is_empty());
    let a = Atelier::nouveau("gros");
    for i in 0..(MAX_CANDIDATS + 30) {
        a.fichier(&format!("f{i:04}.txt"));
    }
    let p = completer_avec("type f", a.dossier(), &[]);
    assert_eq!(p.candidats.len(), MAX_CANDIDATS);
}

/// Le premier Tab lit le PATH (cache d'une minute ensuite) : la mesure s'affiche avec `--nocapture`, et un plafond large
/// attrape une régression grossière (disque réseau dans le PATH, lecture répétée)
#[test]
fn la_lecture_du_path_a_froid_et_en_cache_reste_courte() {
    let t0 = std::time::Instant::now();
    let n = programmes_du_path().len();
    let froid = t0.elapsed();
    let t1 = std::time::Instant::now();
    let _ = completer("gi", r"C:\Windows");
    let chaud = t1.elapsed();
    println!("T7 mesure : PATH lu en {froid:?} ({n} programmes), complétion « gi » avec cache en {chaud:?}");
    assert!(froid < std::time::Duration::from_secs(3), "PATH trop lent : {froid:?}");
    assert!(chaud < std::time::Duration::from_millis(500), "complétion avec cache trop lente : {chaud:?}");
}
