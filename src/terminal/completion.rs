//! T7 : la complétion par Tab de la ligne de saisie du terminal. Module pur : il reçoit le texte tapé avant le curseur et le
//! dossier courant, il rend les candidats ; il ne connaît ni la fenêtre, ni le shell, ni la grille.
//!
//! Sources maison, sans appeler `TabExpansion2` de PowerShell (qui coûte un aller-retour dans le shell, ne marche pas pour cmd et
//! ne se maîtrise pas) :
//! - **chemins** : le dossier tapé (ou le dossier courant), dossiers d'abord, `\` derrière un dossier, guillemets doubles
//!   quand le nom contient un espace ou un signe que le shell lirait autrement ;
//! - **commandes** (premier mot d'une commande, ou après `| ; &`) : exécutables du PATH, commandes intégrées de cmd, alias et
//!   cmdlets usuels de PowerShell ;
//! - **sous-commandes et options** d'une dizaine de programmes courants (`BASE`, descriptions en français : T8 les réutilisera
//!   pour expliquer une commande).
//!
//! Ce que ça ne fait pas (bilan complet dans `docs/PLAN-ACTION-ESPACE-TERMINAL.md`) : variables (`$env:`, `%VAR%`), paramètres
//! des cmdlets, `~`, curseur au milieu de la ligne, options d'un programme absent de `BASE`.

use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Plafond de candidats gardés (le menu défile ; au-delà, taper une lettre de plus est plus utile que défiler)
const MAX_CANDIDATS: usize = 200;
/// Plafond d'entrées lues dans un dossier (un dossier de 100 000 fichiers ne doit pas figer la frappe)
const MAX_ENTREES: usize = 5000;
/// Au-delà, le PATH est relu (un programme installé entre-temps apparaît en une minute au plus)
const VIE_CACHE_PATH: Duration = Duration::from_secs(60);
const EXTENSIONS_EXEC: [&str; 5] = ["exe", "cmd", "bat", "com", "ps1"];
/// Ces commandes ne prennent que des dossiers : ne proposer qu'eux
const COMMANDES_DOSSIER: [&str; 9] = ["cd", "chdir", "sl", "set-location", "pushd", "push-location", "rmdir", "rd", "md"];
/// Caractères qui obligent à citer un nom de fichier (PowerShell les lirait comme de la syntaxe)
const A_CITER: &str = " &;()[]{}'$,`@#%";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Genre {
    Dossier,
    Fichier,
    Commande,
    SousCommande,
    Option,
    /// Une commande déjà tapée (liste automatique) : `brut` est la ligne entière, qui remplace tout ce qui est tapé
    Historique,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Candidat {
    /// Ce que le jeton devient, sans guillemets (chemin compris : `src\main.rs`)
    pub brut: String,
    /// Ce qui s'affiche dans le menu (le dernier maillon d'un chemin)
    pub nom: String,
    pub genre: Genre,
    pub description: &'static str,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Proposition {
    /// Rang (en caractères) du début du jeton à remplacer dans la ligne
    pub debut: usize,
    /// Ce qui était tapé du jeton, sans le guillemet ouvrant
    pub prefixe: String,
    /// Le jeton commençait par un guillemet : les candidats le gardent
    pub guillemet: bool,
    /// Un chemin : les guillemets s'appliquent ; commandes et options ne sont jamais citées
    pub chemins: bool,
    pub candidats: Vec<Candidat>,
}

impl Proposition {
    /// Le texte du candidat tel qu'il remplace le jeton : un dossier laisse son guillemet ouvert (on continue à taper dedans),
    /// un fichier le ferme
    pub fn texte_de(&self, c: &Candidat) -> String {
        self.citer(&c.brut, c.genre != Genre::Dossier)
    }

    fn citer(&self, brut: &str, ferme: bool) -> String {
        if self.chemins && (self.guillemet || brut.chars().any(|c| A_CITER.contains(c))) {
            format!("\"{brut}{}", if ferme { "\"" } else { "" })
        } else {
            brut.to_string()
        }
    }

    /// Le début commun à tous les candidats, s'il va plus loin que ce qui est tapé (casse du premier candidat)
    pub fn prefixe_commun(&self) -> Option<String> {
        let premier = self.candidats.first()?;
        let mut commun: Vec<char> = premier.brut.chars().collect();
        for c in &self.candidats[1..] {
            let n = commun.iter().zip(c.brut.chars()).take_while(|(a, b)| a.to_lowercase().eq(b.to_lowercase())).count();
            commun.truncate(n);
        }
        if commun.len() <= self.prefixe.chars().count() {
            return None;
        }
        let brut: String = commun.into_iter().collect();
        Some(self.citer(&brut, false))
    }
}

// ───────────────────────── Analyse de la ligne ─────────────────────────

struct Jeton {
    debut: usize,
    /// Le jeton tel que tapé (guillemet ouvrant compris)
    brut: String,
    /// Rang dans la commande (0 = le nom de la commande)
    rang: usize,
    /// Le premier mot de la commande, en minuscules
    commande: String,
}

/// Le dernier mot de ce qui précède le curseur, avec sa place dans la commande. Les guillemets doubles protègent les espaces ;
/// `| ; &` ouvrent une nouvelle commande ; les préfixes `cmd:` et `ps:` de T6 ne comptent pas comme un mot.
fn dernier_jeton(avant: &str) -> Jeton {
    let mut mots: Vec<String> = Vec::new();
    let mut courant: Option<(usize, String)> = None;
    let mut entre_guillemets = false;
    let mut n = 0;
    for (i, c) in avant.chars().enumerate() {
        n = i + 1;
        if entre_guillemets {
            if let Some((_, t)) = &mut courant {
                t.push(c);
            }
            entre_guillemets = c != '"';
            continue;
        }
        match c {
            c if c.is_whitespace() => cloturer(&mut courant, &mut mots),
            '|' | ';' | '&' => {
                cloturer(&mut courant, &mut mots);
                mots.clear();
            }
            c => {
                courant.get_or_insert((i, String::new())).1.push(c);
                entre_guillemets = c == '"';
            }
        }
    }
    let (debut, brut, rang) = match courant {
        Some((d, t)) => (d, t, mots.len()),
        None => (n, String::new(), mots.len()),
    };
    Jeton { debut, brut, rang, commande: mots.first().cloned().unwrap_or_default().to_lowercase() }
}

fn cloturer(courant: &mut Option<(usize, String)>, mots: &mut Vec<String>) {
    if let Some((_, t)) = courant.take() {
        // `cmd:` et `ps:` n'entrent pas dans la commande (ils ne sont reconnus qu'en tête)
        let prefixe_t6 = mots.is_empty() && (t.eq_ignore_ascii_case("cmd:") || t.eq_ignore_ascii_case("ps:"));
        if !prefixe_t6 {
            mots.push(t);
        }
    }
}

/// Sépare le guillemet ouvrant (et le fermant s'il est là) de ce qui est tapé
fn sans_guillemets(brut: &str) -> (String, bool) {
    match brut.strip_prefix('"') {
        Some(reste) => (reste.strip_suffix('"').unwrap_or(reste).to_string(), true),
        None => (brut.to_string(), false),
    }
}

// ───────────────────────── Chemins ─────────────────────────

#[derive(Clone, Copy, PartialEq, Eq)]
enum Filtre {
    Tout,
    Dossiers,
    Executables,
}

fn est_executable(nom: &str) -> bool {
    Path::new(nom)
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| EXTENSIONS_EXEC.iter().any(|x| x.eq_ignore_ascii_case(e)))
}

fn sans_extension(nom: &str) -> &str {
    match nom.rfind('.') {
        Some(i) if i > 0 => &nom[..i],
        _ => nom,
    }
}

/// Les entrées du dossier désigné par `prefixe` (relatif au dossier courant, ou absolu) dont le nom commence par ce qui reste
/// tapé. Un nom caché ou système n'est proposé que si une lettre au moins est tapée.
fn chemins(prefixe: &str, dossier: &str, filtre: Filtre) -> Vec<Candidat> {
    let (dir_tape, nom_tape) = match prefixe.rfind(['\\', '/']) {
        Some(i) => (&prefixe[..=i], &prefixe[i + 1..]),
        None => ("", prefixe),
    };
    let separateur = dir_tape.chars().last().unwrap_or('\\');
    let base = if dir_tape.is_empty() { Path::new(dossier).to_path_buf() } else { Path::new(dossier).join(dir_tape) };
    let Ok(lecture) = std::fs::read_dir(&base) else { return Vec::new() };
    let nom_min = nom_tape.to_lowercase();
    let mut trouves: Vec<Candidat> = Vec::new();
    for entree in lecture.take(MAX_ENTREES).flatten() {
        let nom = entree.file_name().to_string_lossy().into_owned();
        if !nom.to_lowercase().starts_with(&nom_min) {
            continue;
        }
        let Ok(type_) = entree.file_type() else { continue };
        let est_dossier = type_.is_dir() || (type_.is_symlink() && entree.path().is_dir());
        if (filtre == Filtre::Dossiers && !est_dossier) || (filtre == Filtre::Executables && !est_dossier && !est_executable(&nom)) {
            continue;
        }
        if nom_tape.is_empty() && cache_ou_systeme(&entree) {
            continue;
        }
        let (affiche, genre) = if est_dossier { (format!("{nom}{separateur}"), Genre::Dossier) } else { (nom, Genre::Fichier) };
        trouves.push(Candidat { brut: format!("{dir_tape}{affiche}"), nom: affiche, genre, description: "" });
    }
    // Dossiers d'abord : on se déplace plus qu'on n'ouvre
    trouves.sort_by_key(|c| (c.genre != Genre::Dossier, c.nom.to_lowercase()));
    trouves.truncate(MAX_CANDIDATS);
    trouves
}

#[cfg(windows)]
fn cache_ou_systeme(entree: &std::fs::DirEntry) -> bool {
    use std::os::windows::fs::MetadataExt;
    // FILE_ATTRIBUTE_HIDDEN (0x2) | FILE_ATTRIBUTE_SYSTEM (0x4)
    entree.metadata().map(|m| m.file_attributes() & 0x6 != 0).unwrap_or(false)
}

#[cfg(not(windows))]
fn cache_ou_systeme(entree: &std::fs::DirEntry) -> bool {
    entree.file_name().to_string_lossy().starts_with('.')
}

// ───────────────────────── Commandes ─────────────────────────

/// Commandes intégrées de cmd, alias et cmdlets usuels de PowerShell (nom, description)
const INTEGREES: &[(&str, &str)] = &[
    ("cd", "Change de dossier"),
    ("chdir", "Change de dossier"),
    ("cls", "Efface l'écran"),
    ("copy", "Copie des fichiers"),
    ("del", "Supprime des fichiers"),
    ("dir", "Liste le contenu du dossier"),
    ("echo", "Affiche un texte"),
    ("exit", "Ferme le shell"),
    ("md", "Crée un dossier"),
    ("mkdir", "Crée un dossier"),
    ("move", "Déplace des fichiers"),
    ("popd", "Revient au dossier mémorisé par pushd"),
    ("pushd", "Mémorise le dossier puis change de dossier"),
    ("rd", "Supprime un dossier"),
    ("ren", "Renomme un fichier"),
    ("rmdir", "Supprime un dossier"),
    ("set", "Affiche ou règle une variable d'environnement"),
    ("start", "Lance un programme ou ouvre un fichier"),
    ("tree", "Dessine l'arborescence d'un dossier"),
    ("type", "Affiche le contenu d'un fichier"),
    ("ver", "Version de Windows"),
    ("where", "Cherche un programme dans le PATH"),
    ("cat", "Affiche le contenu d'un fichier"),
    ("clear", "Efface l'écran"),
    ("cp", "Copie des fichiers"),
    ("gc", "Affiche le contenu d'un fichier (Get-Content)"),
    ("gci", "Liste le contenu du dossier (Get-ChildItem)"),
    ("gcm", "Cherche une commande (Get-Command)"),
    ("gps", "Liste les processus (Get-Process)"),
    ("ls", "Liste le contenu du dossier"),
    ("mv", "Déplace des fichiers"),
    ("ps", "Liste les processus"),
    ("pwd", "Affiche le dossier courant"),
    ("rm", "Supprime des fichiers"),
    ("sls", "Cherche un texte dans des fichiers (Select-String)"),
    ("Clear-Host", "Efface l'écran"),
    ("Copy-Item", "Copie un fichier ou un dossier"),
    ("ForEach-Object", "Applique une action à chaque élément"),
    ("Get-ChildItem", "Liste le contenu du dossier"),
    ("Get-Command", "Cherche une commande"),
    ("Get-Content", "Affiche le contenu d'un fichier"),
    ("Get-Date", "Date et heure"),
    ("Get-Help", "Affiche l'aide d'une commande"),
    ("Get-Location", "Affiche le dossier courant"),
    ("Get-Process", "Liste les processus"),
    ("Get-Service", "Liste les services"),
    ("Invoke-WebRequest", "Télécharge une page ou un fichier"),
    ("Measure-Object", "Compte, additionne, calcule"),
    ("Move-Item", "Déplace un fichier ou un dossier"),
    ("New-Item", "Crée un fichier ou un dossier"),
    ("Out-File", "Écrit le résultat dans un fichier"),
    ("Remove-Item", "Supprime un fichier ou un dossier"),
    ("Rename-Item", "Renomme un fichier ou un dossier"),
    ("Resolve-Path", "Donne le chemin complet"),
    ("Select-Object", "Garde certaines propriétés ou lignes"),
    ("Select-String", "Cherche un texte dans des fichiers"),
    ("Set-Content", "Écrit du texte dans un fichier"),
    ("Set-Location", "Change de dossier"),
    ("Sort-Object", "Trie le résultat"),
    ("Start-Process", "Lance un programme"),
    ("Stop-Process", "Arrête un processus"),
    ("Test-Path", "Dit si un chemin existe"),
    ("Where-Object", "Filtre le résultat"),
    ("Write-Output", "Affiche un texte"),
];

/// Un programme connu : ses noms, ce qu'il fait, ses sous-commandes et options (les options commencent par `-`)
struct Programme {
    noms: &'static [&'static str],
    description: &'static str,
    entrees: &'static [(&'static str, &'static str)],
}

/// La base embarquée. Mesure du poids : voir `docs/PLAN-ACTION-ESPACE-TERMINAL.md` (T7).
const BASE: &[Programme] = &[
    Programme {
        noms: &["git"],
        description: "Gestion de versions",
        entrees: &[
            ("add", "Ajoute des fichiers à l'index"),
            ("bisect", "Cherche par dichotomie le commit fautif"),
            ("blame", "Auteur de chaque ligne d'un fichier"),
            ("branch", "Liste ou crée des branches"),
            ("checkout", "Change de branche ou restaure des fichiers"),
            ("cherry-pick", "Reprend un commit d'une autre branche"),
            ("clean", "Supprime les fichiers non suivis"),
            ("clone", "Copie un dépôt distant"),
            ("commit", "Enregistre les changements"),
            ("config", "Lit ou règle la configuration"),
            ("diff", "Montre les différences"),
            ("fetch", "Récupère le distant sans fusionner"),
            ("grep", "Cherche un texte dans le dépôt"),
            ("init", "Crée un dépôt"),
            ("log", "Historique des commits"),
            ("merge", "Fusionne une branche"),
            ("mv", "Déplace ou renomme un fichier suivi"),
            ("pull", "Récupère et fusionne"),
            ("push", "Envoie les commits"),
            ("rebase", "Rejoue les commits sur une autre base"),
            ("remote", "Gère les dépôts distants"),
            ("reset", "Défait des commits ou l'index"),
            ("restore", "Restaure des fichiers"),
            ("revert", "Annule un commit par un nouveau commit"),
            ("rm", "Supprime un fichier suivi"),
            ("show", "Détail d'un commit"),
            ("stash", "Met les changements de côté"),
            ("status", "État du dépôt"),
            ("switch", "Change de branche"),
            ("tag", "Gère les étiquettes"),
            ("--all", "Tout, pas seulement l'essentiel"),
            ("--amend", "Corrige le dernier commit"),
            ("--force-with-lease", "Force l'envoi sans écraser le travail des autres"),
            ("--hard", "Jette aussi les fichiers modifiés"),
            ("--help", "Affiche l'aide"),
            ("--no-verify", "Saute les hooks"),
            ("--oneline", "Une ligne par commit"),
            ("--soft", "Garde l'index et les fichiers"),
            ("--staged", "Ce qui est dans l'index"),
            ("--stat", "Résumé des fichiers touchés"),
            ("--version", "Version de git"),
        ],
    },
    Programme {
        noms: &["cargo"],
        description: "Outil de construction Rust",
        entrees: &[
            ("add", "Ajoute une dépendance"),
            ("bench", "Lance les mesures de performance"),
            ("build", "Compile le projet"),
            ("check", "Vérifie sans produire de binaire"),
            ("clean", "Supprime le dossier target"),
            ("clippy", "Analyse le code (lints)"),
            ("doc", "Construit la documentation"),
            ("fix", "Applique les corrections automatiques"),
            ("fmt", "Met le code en forme"),
            ("init", "Crée un projet dans le dossier courant"),
            ("install", "Installe un programme Rust"),
            ("metadata", "Décrit le projet en JSON"),
            ("new", "Crée un nouveau projet"),
            ("package", "Prépare une archive du paquet"),
            ("publish", "Publie sur crates.io"),
            ("remove", "Retire une dépendance"),
            ("run", "Compile puis lance"),
            ("search", "Cherche un paquet sur crates.io"),
            ("test", "Lance les tests"),
            ("tree", "Arbre des dépendances"),
            ("uninstall", "Désinstalle un programme Rust"),
            ("update", "Met à jour les dépendances"),
            ("--all-targets", "Tous les binaires, tests et exemples"),
            ("--bin", "Un binaire précis"),
            ("--example", "Un exemple précis"),
            ("--features", "Active des fonctionnalités"),
            ("--help", "Affiche l'aide"),
            ("--lib", "La bibliothèque seulement"),
            ("--locked", "Respecte Cargo.lock"),
            ("--manifest-path", "Chemin du Cargo.toml"),
            ("--no-default-features", "Sans les fonctionnalités par défaut"),
            ("--offline", "Sans réseau"),
            ("--package", "Un paquet précis"),
            ("--quiet", "Moins de messages"),
            ("--release", "Version optimisée"),
            ("--target", "Plateforme cible"),
            ("--verbose", "Plus de messages"),
            ("--version", "Version de cargo"),
            ("--workspace", "Tous les paquets de l'espace de travail"),
        ],
    },
    Programme {
        noms: &["npm"],
        description: "Gestionnaire de paquets Node",
        entrees: &[
            ("audit", "Cherche des failles dans les dépendances"),
            ("ci", "Installation propre depuis le verrou"),
            ("exec", "Lance un programme d'un paquet"),
            ("init", "Crée un package.json"),
            ("install", "Installe les dépendances"),
            ("link", "Lie un paquet local"),
            ("ls", "Arbre des dépendances"),
            ("outdated", "Paquets périmés"),
            ("pack", "Prépare une archive du paquet"),
            ("publish", "Publie le paquet"),
            ("run", "Lance un script du package.json"),
            ("start", "Lance le script start"),
            ("test", "Lance le script test"),
            ("uninstall", "Retire une dépendance"),
            ("update", "Met à jour les dépendances"),
            ("version", "Change la version du paquet"),
            ("view", "Informations d'un paquet publié"),
            ("--dry-run", "Simule sans rien faire"),
            ("--force", "Force l'opération"),
            ("--global", "Installation globale"),
            ("--help", "Affiche l'aide"),
            ("--legacy-peer-deps", "Ignore les conflits de dépendances homologues"),
            ("--production", "Sans les dépendances de développement"),
            ("--save-dev", "Dépendance de développement"),
            ("--silent", "Sans messages"),
            ("--version", "Version de npm"),
        ],
    },
    Programme {
        noms: &["docker"],
        description: "Conteneurs",
        entrees: &[
            ("build", "Construit une image"),
            ("compose", "Gère plusieurs conteneurs ensemble"),
            ("container", "Gère les conteneurs"),
            ("cp", "Copie entre conteneur et dossier"),
            ("exec", "Lance une commande dans un conteneur"),
            ("image", "Gère les images"),
            ("images", "Liste les images"),
            ("inspect", "Détails d'un objet Docker"),
            ("login", "Se connecte à un registre"),
            ("logs", "Journal d'un conteneur"),
            ("network", "Gère les réseaux"),
            ("ps", "Liste les conteneurs"),
            ("pull", "Télécharge une image"),
            ("push", "Envoie une image"),
            ("restart", "Redémarre un conteneur"),
            ("rm", "Supprime un conteneur"),
            ("rmi", "Supprime une image"),
            ("run", "Lance un conteneur"),
            ("start", "Démarre un conteneur arrêté"),
            ("stop", "Arrête un conteneur"),
            ("system", "Gère l'espace utilisé"),
            ("tag", "Donne un nom à une image"),
            ("volume", "Gère les volumes"),
            ("--detach", "En arrière-plan"),
            ("--env", "Variable d'environnement"),
            ("--help", "Affiche l'aide"),
            ("--name", "Nom du conteneur"),
            ("--publish", "Publie un port"),
            ("--rm", "Supprime le conteneur à l'arrêt"),
            ("--version", "Version de docker"),
            ("--volume", "Monte un dossier"),
        ],
    },
    Programme {
        noms: &["winget"],
        description: "Gestionnaire de paquets Windows",
        entrees: &[
            ("export", "Exporte la liste des paquets"),
            ("hash", "Calcule l'empreinte d'un installeur"),
            ("import", "Installe depuis une liste"),
            ("install", "Installe un programme"),
            ("list", "Liste les programmes installés"),
            ("search", "Cherche un programme"),
            ("settings", "Ouvre les réglages"),
            ("show", "Détails d'un programme"),
            ("source", "Gère les sources"),
            ("uninstall", "Désinstalle un programme"),
            ("upgrade", "Met à jour un ou tous les programmes"),
            ("validate", "Vérifie un manifeste"),
            ("--accept-package-agreements", "Accepte les licences du paquet"),
            ("--accept-source-agreements", "Accepte les licences de la source"),
            ("--exact", "Nom exact seulement"),
            ("--help", "Affiche l'aide"),
            ("--id", "Identifiant du paquet"),
            ("--name", "Nom du paquet"),
            ("--silent", "Installation silencieuse"),
            ("--source", "Source à utiliser"),
            ("--version", "Version du paquet ou de winget"),
        ],
    },
    Programme {
        noms: &["python", "py", "python3"],
        description: "Interpréteur Python",
        entrees: &[
            ("-c", "Exécute le code donné"),
            ("-m", "Lance un module (pip, venv, http.server…)"),
            ("-V", "Version de Python"),
            ("--help", "Affiche l'aide"),
            ("--version", "Version de Python"),
        ],
    },
    Programme {
        noms: &["pip", "pip3"],
        description: "Paquets Python",
        entrees: &[
            ("check", "Vérifie les dépendances"),
            ("download", "Télécharge sans installer"),
            ("freeze", "Liste les paquets installés au format requirements"),
            ("install", "Installe un paquet"),
            ("list", "Liste les paquets installés"),
            ("show", "Détails d'un paquet installé"),
            ("uninstall", "Désinstalle un paquet"),
            ("--help", "Affiche l'aide"),
            ("--upgrade", "Met à jour"),
            ("--user", "Pour l'utilisateur seulement"),
            ("--version", "Version de pip"),
            ("-r", "Lit la liste dans un fichier"),
        ],
    },
    Programme {
        noms: &["dotnet"],
        description: "Outils .NET",
        entrees: &[
            ("add", "Ajoute un paquet ou une référence"),
            ("build", "Compile le projet"),
            ("clean", "Supprime les fichiers produits"),
            ("format", "Met le code en forme"),
            ("new", "Crée un projet ou un fichier"),
            ("pack", "Fabrique un paquet NuGet"),
            ("publish", "Prépare le déploiement"),
            ("restore", "Restaure les dépendances"),
            ("run", "Compile puis lance"),
            ("test", "Lance les tests"),
            ("tool", "Gère les outils .NET"),
            ("watch", "Relance à chaque modification"),
            ("--configuration", "Debug ou Release"),
            ("--help", "Affiche l'aide"),
            ("--no-restore", "Ne restaure pas les dépendances"),
            ("--version", "Version du SDK"),
        ],
    },
    Programme {
        noms: &["gh"],
        description: "GitHub en ligne de commande",
        entrees: &[
            ("api", "Appelle l'API GitHub"),
            ("auth", "Connexion à GitHub"),
            ("gist", "Gère les gists"),
            ("issue", "Gère les tickets"),
            ("pr", "Gère les demandes de fusion"),
            ("release", "Gère les versions publiées"),
            ("repo", "Gère les dépôts"),
            ("run", "Exécutions des workflows"),
            ("workflow", "Gère les workflows"),
            ("--help", "Affiche l'aide"),
            ("--version", "Version de gh"),
        ],
    },
    Programme {
        noms: &["rustup"],
        description: "Chaînes d'outils Rust",
        entrees: &[
            ("component", "Gère les composants"),
            ("default", "Choisit la chaîne par défaut"),
            ("doc", "Ouvre la documentation"),
            ("install", "Installe une chaîne d'outils"),
            ("run", "Lance une commande avec une chaîne"),
            ("show", "Chaînes installées"),
            ("target", "Gère les plateformes cibles"),
            ("toolchain", "Gère les chaînes d'outils"),
            ("update", "Met à jour les chaînes"),
            ("which", "Chemin d'un programme de la chaîne"),
            ("--help", "Affiche l'aide"),
            ("--version", "Version de rustup"),
        ],
    },
];

fn programme(commande: &str) -> Option<&'static Programme> {
    let nom = commande.trim_matches('"');
    let nom = nom.rsplit(['\\', '/']).next().unwrap_or(nom);
    let nom = sans_extension(nom).to_lowercase();
    BASE.iter().find(|p| p.noms.contains(&nom.as_str()))
}

/// Les noms des programmes du PATH, dédoublonnés et triés ; relus après `VIE_CACHE_PATH`
fn programmes_du_path() -> Arc<Vec<String>> {
    static CACHE: Mutex<Option<(Instant, Arc<Vec<String>>)>> = Mutex::new(None);
    let Ok(mut verrou) = CACHE.lock() else { return Arc::new(Vec::new()) };
    if let Some((quand, liste)) = verrou.as_ref() {
        if quand.elapsed() < VIE_CACHE_PATH {
            return liste.clone();
        }
    }
    let liste = Arc::new(lire_path(&std::env::var_os("PATH").unwrap_or_default()));
    *verrou = Some((Instant::now(), liste.clone()));
    liste
}

/// Les programmes (extension retirée) des dossiers d'une variable PATH
fn lire_path(path: &std::ffi::OsStr) -> Vec<String> {
    let mut noms: Vec<String> = Vec::new();
    for dossier in std::env::split_paths(path) {
        let Ok(lecture) = std::fs::read_dir(&dossier) else { continue };
        for e in lecture.take(MAX_ENTREES * 4).flatten() {
            let nom = e.file_name().to_string_lossy().into_owned();
            if est_executable(&nom) {
                noms.push(sans_extension(&nom).to_string());
            }
        }
    }
    noms.sort_by_key(|n| n.to_lowercase());
    noms.dedup_by(|a, b| a.eq_ignore_ascii_case(b));
    noms
}

fn commandes(prefixe: &str, programmes: &[String]) -> Vec<Candidat> {
    let min = prefixe.to_lowercase();
    let mut trouves: Vec<Candidat> = Vec::new();
    let mut vus: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut ajouter = |nom: &str, description: &'static str| {
        if nom.to_lowercase().starts_with(&min) && vus.insert(nom.to_lowercase()) {
            trouves.push(Candidat { brut: nom.to_string(), nom: nom.to_string(), genre: Genre::Commande, description });
        }
    };
    for (nom, d) in INTEGREES {
        ajouter(nom, d);
    }
    for p in BASE {
        for nom in p.noms {
            ajouter(nom, p.description);
        }
    }
    for nom in programmes {
        ajouter(nom, "");
    }
    trouves.sort_by_key(|c| c.nom.to_lowercase());
    trouves.truncate(MAX_CANDIDATS);
    trouves
}

fn entrees_du_programme(p: &Programme, prefixe: &str, options: bool) -> Vec<Candidat> {
    let min = prefixe.to_lowercase();
    p.entrees
        .iter()
        .filter(|(nom, _)| nom.starts_with('-') == options && nom.to_lowercase().starts_with(&min))
        .map(|(nom, d)| Candidat {
            brut: (*nom).to_string(),
            nom: (*nom).to_string(),
            genre: if options { Genre::Option } else { Genre::SousCommande },
            description: d,
        })
        .collect()
}

// ───────────────────────── Point d'entrée ─────────────────────────

/// Les candidats pour `avant` (ce qui précède le curseur), avec les programmes du PATH réel
pub fn completer(avant: &str, dossier: &str) -> Proposition {
    completer_avec(avant, dossier, &programmes_du_path())
}

/// Comme `completer`, avec la liste des programmes donnée (les tests ne dépendent pas du PATH de la machine)
pub fn completer_avec(avant: &str, dossier: &str, programmes: &[String]) -> Proposition {
    let j = dernier_jeton(avant);
    let (prefixe, guillemet) = sans_guillemets(&j.brut);
    let chemin = guillemet || prefixe.contains(['\\', '/']);
    let filtre = if COMMANDES_DOSSIER.contains(&j.commande.as_str()) { Filtre::Dossiers } else { Filtre::Tout };
    let mut chemins_utilises = false;
    let candidats = if j.rang == 0 {
        if chemin {
            chemins_utilises = true;
            chemins(&prefixe, dossier, Filtre::Executables)
        } else if prefixe.is_empty() {
            Vec::new()
        } else {
            commandes(&prefixe, programmes)
        }
    } else {
        let prog = programme(&j.commande);
        if prefixe.starts_with('-') && !chemin {
            prog.map(|p| entrees_du_programme(p, &prefixe, true)).unwrap_or_default()
        } else if chemin {
            chemins_utilises = true;
            chemins(&prefixe, dossier, filtre)
        } else {
            let sous = match prog {
                Some(p) if j.rang == 1 => entrees_du_programme(p, &prefixe, false),
                _ => Vec::new(),
            };
            // `git <Tab>` : les sous-commandes seules ; un mot qui n'en est aucune est un fichier (`git add src`)
            if !sous.is_empty() || (prog.is_some() && j.rang == 1 && prefixe.is_empty()) {
                sous
            } else {
                chemins_utilises = true;
                chemins(&prefixe, dossier, filtre)
            }
        }
    };
    Proposition { debut: j.debut, prefixe, guillemet, chemins: chemins_utilises, candidats }
}

#[cfg(test)]
mod tests;
