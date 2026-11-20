//! Ligne de saisie du terminal (PLAN-TERMINAL, lot T5) : l'éditeur de la commande en cours de frappe. Aucun Win32 ni dessin ici :
//! le texte, le curseur et les gestes d'édition (mots, effacements, collage) se testent sans écran. Le panneau y envoie les
//! touches tant que le shell attend une commande ; à Entrée il envoie la ligne entière au vrai shell.
//!
//! Le curseur compte des **caractères** (pas des octets) : un accent ou un emoji ne se coupe jamais en deux.

/// Catégorie d'un caractère pour les déplacements et effacements « par mot »
#[derive(Clone, Copy, PartialEq, Eq)]
enum Classe {
    Espace,
    Mot,
    Signe,
}

fn classe(c: char) -> Classe {
    if c.is_whitespace() {
        Classe::Espace
    } else if c.is_alphanumeric() || c == '_' {
        Classe::Mot
    } else {
        Classe::Signe
    }
}

/// Plafond de la ligne : un collage énorme ne doit pas figer le dessin ni l'envoi au shell
pub const LONGUEUR_MAX: usize = 4_000;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Saisie {
    /// Le texte, un élément par caractère (les fins de ligne d'un collage y restent en `'\n'`)
    texte: Vec<char>,
    /// Position du curseur : 0 = avant le premier caractère, `texte.len()` = après le dernier
    curseur: usize,
}

impl Saisie {
    pub fn vide(&self) -> bool {
        self.texte.is_empty()
    }

    #[cfg(test)]
    pub fn curseur(&self) -> usize {
        self.curseur
    }

    pub fn longueur(&self) -> usize {
        self.texte.len()
    }

    pub fn texte(&self) -> String {
        self.texte.iter().collect()
    }

    /// Le curseur est après le dernier caractère : seul endroit où la suggestion grise s'accepte
    pub fn curseur_a_la_fin(&self) -> bool {
        self.curseur == self.texte.len()
    }

    /// Insère du texte au curseur. Les caractères de contrôle sont retirés (un ESC collé ne doit pas partir au shell) ;
    /// `\r\n` et `\r` deviennent `\n` ; la tabulation devient une espace. Rend `true` si quelque chose a été inséré.
    pub fn inserer(&mut self, ajout: &str) -> bool {
        let mut propre: Vec<char> = Vec::with_capacity(ajout.len());
        let mut precedent_cr = false;
        for c in ajout.chars() {
            match c {
                '\r' => {
                    propre.push('\n');
                    precedent_cr = true;
                    continue;
                }
                '\n' => {
                    if !precedent_cr {
                        propre.push('\n');
                    }
                }
                '\t' => propre.push(' '),
                c if c.is_control() => {}
                c => propre.push(c),
            }
            precedent_cr = false;
        }
        let place = LONGUEUR_MAX.saturating_sub(self.texte.len());
        propre.truncate(place);
        if propre.is_empty() {
            return false;
        }
        let n = propre.len();
        self.texte.splice(self.curseur..self.curseur, propre);
        self.curseur += n;
        true
    }

    pub fn inserer_caractere(&mut self, c: char) -> bool {
        let mut tampon = [0u8; 4];
        self.inserer(c.encode_utf8(&mut tampon))
    }

    /// Remplace tout le texte (rappel de l'historique, suggestion acceptée) ; le curseur va à la fin
    pub fn remplacer(&mut self, texte: &str) {
        self.texte.clear();
        self.curseur = 0;
        self.inserer(texte);
    }

    /// Vide la ligne et rend ce qu'elle contenait
    pub fn prendre(&mut self) -> String {
        let t = self.texte();
        self.texte.clear();
        self.curseur = 0;
        t
    }

    pub fn gauche(&mut self) -> bool {
        if self.curseur == 0 {
            return false;
        }
        self.curseur -= 1;
        true
    }

    pub fn droite(&mut self) -> bool {
        if self.curseur >= self.texte.len() {
            return false;
        }
        self.curseur += 1;
        true
    }

    pub fn debut(&mut self) -> bool {
        let change = self.curseur != 0;
        self.curseur = 0;
        change
    }

    pub fn fin(&mut self) -> bool {
        let change = self.curseur != self.texte.len();
        self.curseur = self.texte.len();
        change
    }

    /// Début du mot qui précède le curseur : saute les espaces, puis un bloc de lettres ou un bloc de signes
    fn debut_du_mot_precedent(&self) -> usize {
        let mut i = self.curseur;
        while i > 0 && classe(self.texte[i - 1]) == Classe::Espace {
            i -= 1;
        }
        if i > 0 {
            let k = classe(self.texte[i - 1]);
            while i > 0 && classe(self.texte[i - 1]) == k {
                i -= 1;
            }
        }
        i
    }

    /// Fin du mot qui suit le curseur (même règle, vers la droite)
    fn fin_du_mot_suivant(&self) -> usize {
        let n = self.texte.len();
        let mut i = self.curseur;
        while i < n && classe(self.texte[i]) == Classe::Espace {
            i += 1;
        }
        if i < n {
            let k = classe(self.texte[i]);
            while i < n && classe(self.texte[i]) == k {
                i += 1;
            }
        }
        i
    }

    pub fn mot_gauche(&mut self) -> bool {
        let i = self.debut_du_mot_precedent();
        let change = i != self.curseur;
        self.curseur = i;
        change
    }

    pub fn mot_droite(&mut self) -> bool {
        let i = self.fin_du_mot_suivant();
        let change = i != self.curseur;
        self.curseur = i;
        change
    }

    /// Retour arrière : efface le caractère avant le curseur
    pub fn retour_arriere(&mut self) -> bool {
        if self.curseur == 0 {
            return false;
        }
        self.curseur -= 1;
        self.texte.remove(self.curseur);
        true
    }

    /// Suppr : efface le caractère sous le curseur
    pub fn supprimer(&mut self) -> bool {
        if self.curseur >= self.texte.len() {
            return false;
        }
        self.texte.remove(self.curseur);
        true
    }

    /// Ctrl+Retour arrière (et Ctrl+W) : efface le mot avant le curseur
    pub fn supprimer_mot_gauche(&mut self) -> bool {
        let i = self.debut_du_mot_precedent();
        if i == self.curseur {
            return false;
        }
        self.texte.drain(i..self.curseur);
        self.curseur = i;
        true
    }

    /// Ctrl+Suppr : efface le mot après le curseur
    pub fn supprimer_mot_droite(&mut self) -> bool {
        let i = self.fin_du_mot_suivant();
        if i == self.curseur {
            return false;
        }
        self.texte.drain(self.curseur..i);
        true
    }

    /// Ctrl+U : efface tout ce qui précède le curseur
    pub fn supprimer_jusqu_au_debut(&mut self) -> bool {
        if self.curseur == 0 {
            return false;
        }
        self.texte.drain(..self.curseur);
        self.curseur = 0;
        true
    }

    /// Ce qui s'affiche sur **une seule ligne** : les fins de ligne d'un collage deviennent « ↵ ». Chaque caractère reste une case.
    pub fn texte_affiche(&self) -> Vec<char> {
        self.texte.iter().map(|&c| if c == '\n' { '↵' } else { c }).collect()
    }

    /// Fenêtre d'affichage de `largeur` cases qui garde le curseur visible : (premier caractère montré, curseur dans la fenêtre).
    /// Le curseur occupe au plus la dernière case de la fenêtre.
    pub fn fenetre(&self, largeur: usize) -> (usize, usize) {
        if largeur == 0 {
            return (0, 0);
        }
        // Le curseur occupe la dernière case au plus ; avant : tout tient
        if self.curseur < largeur {
            return (0, self.curseur);
        }
        let debut = self.curseur + 1 - largeur;
        (debut, self.curseur - debut)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn avec(t: &str) -> Saisie {
        let mut s = Saisie::default();
        s.inserer(t);
        s
    }

    #[test]
    fn la_frappe_insere_au_curseur() {
        let mut s = avec("abd");
        s.gauche();
        s.inserer_caractere('c');
        assert_eq!(s.texte(), "abcd");
        assert_eq!(s.curseur(), 3);
    }

    #[test]
    fn le_curseur_compte_des_caracteres_pas_des_octets() {
        let mut s = avec("é😀a");
        assert_eq!(s.longueur(), 3);
        s.gauche();
        s.gauche();
        assert_eq!(s.curseur(), 1);
        s.retour_arriere();
        assert_eq!(s.texte(), "😀a");
        s.supprimer();
        assert_eq!(s.texte(), "a");
    }

    #[test]
    fn les_bords_ne_font_rien() {
        let mut s = avec("ab");
        assert!(!s.droite());
        s.debut();
        assert!(!s.gauche());
        assert!(!s.retour_arriere());
        s.fin();
        assert!(!s.supprimer());
        assert_eq!(s.texte(), "ab");
        let mut v = Saisie::default();
        assert!(!v.mot_gauche() && !v.mot_droite() && !v.supprimer_mot_gauche() && !v.supprimer_mot_droite() && !v.supprimer_jusqu_au_debut());
    }

    #[test]
    fn les_mots_sautent_lettres_et_signes_separement() {
        let mut s = avec("git commit -m \"fix\"");
        s.mot_gauche();
        assert_eq!(s.curseur(), 18, "avant le guillemet fermant : le mot « fix » est sauté en bloc au pas suivant");
        s.mot_gauche();
        assert_eq!(s.curseur(), 15, "« fix »");
        s.mot_gauche();
        assert_eq!(s.curseur(), 14, "le guillemet ouvrant");
        s.mot_gauche();
        assert_eq!(s.curseur(), 12, "« -m » : le m puis le tiret sont deux blocs, on s'arrête avant le m");
        s.debut();
        s.mot_droite();
        assert_eq!(s.curseur(), 3, "après « git »");
        s.mot_droite();
        assert_eq!(s.curseur(), 10, "après « commit »");
    }

    #[test]
    fn effacer_un_mot_efface_le_bloc_avant_le_curseur() {
        let mut s = avec("cd C:\\Users\\utilisateur");
        s.supprimer_mot_gauche();
        assert_eq!(s.texte(), "cd C:\\Users\\");
        s.supprimer_mot_gauche();
        assert_eq!(s.texte(), "cd C:\\Users");
        s.supprimer_mot_gauche();
        assert_eq!(s.texte(), "cd C:\\");
        let mut s = avec("echo   ");
        s.supprimer_mot_gauche();
        assert_eq!(s.texte(), "", "les espaces de fin partent avec le mot");
    }

    #[test]
    fn effacer_un_mot_a_droite_et_jusqu_au_debut() {
        let mut s = avec("alpha beta gamma");
        s.debut();
        s.supprimer_mot_droite();
        assert_eq!(s.texte(), " beta gamma");
        assert_eq!(s.curseur(), 0);
        let mut s = avec("alpha beta");
        s.gauche();
        s.gauche();
        s.supprimer_jusqu_au_debut();
        assert_eq!(s.texte(), "ta");
        assert_eq!(s.curseur(), 0);
    }

    #[test]
    fn un_collage_est_nettoye() {
        let mut s = Saisie::default();
        assert!(s.inserer("a\u{1b}[31mb\tc\r\nd\re\nf"));
        assert_eq!(s.texte(), "a[31mb c\nd\ne\nf", "ESC retiré, tabulation en espace, \\r\\n compté une fois");
        assert_eq!(s.texte_affiche().iter().collect::<String>(), "a[31mb c↵d↵e↵f");
        assert!(!Saisie::default().inserer("\u{7}\u{1b}"), "rien d'inséré : pas de changement");
    }

    #[test]
    fn un_collage_enorme_est_plafonne() {
        let mut s = Saisie::default();
        s.inserer(&"x".repeat(LONGUEUR_MAX + 500));
        assert_eq!(s.longueur(), LONGUEUR_MAX);
        assert!(!s.inserer("y"), "plein : rien de plus");
        assert_eq!(s.longueur(), LONGUEUR_MAX);
    }

    #[test]
    fn remplacer_et_prendre() {
        let mut s = avec("ancien");
        s.remplacer("nouveau");
        assert_eq!(s.texte(), "nouveau");
        assert!(s.curseur_a_la_fin());
        assert_eq!(s.prendre(), "nouveau");
        assert!(s.vide());
        assert_eq!(s.curseur(), 0);
    }

    #[test]
    fn la_fenetre_garde_le_curseur_visible() {
        let mut s = avec("0123456789");
        assert_eq!(s.fenetre(20), (0, 10), "tout tient");
        assert_eq!(s.fenetre(10), (1, 9), "le curseur en fin de ligne garde sa case : le premier caractère sort");
        s.debut();
        assert_eq!(s.fenetre(10), (0, 0));
        assert_eq!(s.fenetre(0), (0, 0));
        let mut long = avec(&"a".repeat(100));
        let (debut, dans) = long.fenetre(30);
        assert_eq!(debut + dans, 100);
        assert!(dans < 30);
        long.debut();
        assert_eq!(long.fenetre(30), (0, 0));
    }
}
