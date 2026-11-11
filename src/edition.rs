// Champ de saisie dessiné par nous (renommage dans la ligne), en UTF-16 comme DirectWrite

#[derive(Clone)]
pub struct Edition {
    /// Ligne en cours de renommage
    pub index: usize,
    pub original: String,
    pub texte: Vec<u16>,
    pub curseur: usize,
    pub ancre: usize,
}

impl Edition {
    /// Champ de saisie libre (palette, renommage par lot) : curseur à la fin
    pub fn champ(texte: &str) -> Self {
        let texte: Vec<u16> = texte.encode_utf16().collect();
        let n = texte.len();
        Self { index: 0, original: String::new(), texte, curseur: n, ancre: n }
    }

    /// Sélectionne le nom sans l'extension, comme l'Explorateur
    pub fn nouveau(index: usize, nom: &str, est_dossier: bool) -> Self {
        let texte: Vec<u16> = nom.encode_utf16().collect();
        let fin = if est_dossier {
            texte.len()
        } else {
            texte.iter().rposition(|&c| c == '.' as u16).filter(|&i| i > 0).unwrap_or(texte.len())
        };
        Self { index, original: nom.to_string(), texte, curseur: fin, ancre: 0 }
    }

    pub fn valeur(&self) -> String {
        String::from_utf16_lossy(&self.texte)
    }

    pub fn selection(&self) -> (usize, usize) {
        (self.curseur.min(self.ancre), self.curseur.max(self.ancre))
    }

    pub fn texte_choisi(&self) -> String {
        let (a, b) = self.selection();
        String::from_utf16_lossy(&self.texte[a..b])
    }

    fn effacer_selection(&mut self) -> bool {
        let (a, b) = self.selection();
        if a == b {
            return false;
        }
        self.texte.drain(a..b);
        self.curseur = a;
        self.ancre = a;
        true
    }

    pub fn inserer(&mut self, s: &str) {
        self.effacer_selection();
        let u: Vec<u16> = s.encode_utf16().filter(|&c| c != '\r' as u16 && c != '\n' as u16).collect();
        let n = u.len();
        self.texte.splice(self.curseur..self.curseur, u);
        self.curseur += n;
        self.ancre = self.curseur;
    }

    /// Recule d'un caractère (sans couper une paire UTF-16)
    fn avant(&self, i: usize) -> usize {
        if i == 0 {
            return 0;
        }
        if i >= 2 && (0xDC00..0xE000).contains(&self.texte[i - 1]) && (0xD800..0xDC00).contains(&self.texte[i - 2]) {
            i - 2
        } else {
            i - 1
        }
    }

    fn apres(&self, i: usize) -> usize {
        if i >= self.texte.len() {
            return self.texte.len();
        }
        if (0xD800..0xDC00).contains(&self.texte[i]) && i + 1 < self.texte.len() {
            i + 2
        } else {
            i + 1
        }
    }

    pub fn effacer_arriere(&mut self) {
        if !self.effacer_selection() && self.curseur > 0 {
            let a = self.avant(self.curseur);
            self.texte.drain(a..self.curseur);
            self.curseur = a;
            self.ancre = a;
        }
    }

    pub fn effacer_avant(&mut self) {
        if !self.effacer_selection() && self.curseur < self.texte.len() {
            let b = self.apres(self.curseur);
            self.texte.drain(self.curseur..b);
            self.ancre = self.curseur;
        }
    }

    pub fn aller(&mut self, position: usize, etendre: bool) {
        self.curseur = position.min(self.texte.len());
        if !etendre {
            self.ancre = self.curseur;
        }
    }

    pub fn gauche(&mut self, etendre: bool) {
        let (a, b) = self.selection();
        if !etendre && a != b {
            self.aller(a, false);
        } else {
            self.aller(self.avant(self.curseur), etendre);
        }
    }

    pub fn droite(&mut self, etendre: bool) {
        let (a, b) = self.selection();
        if !etendre && a != b {
            self.aller(b, false);
        } else {
            self.aller(self.apres(self.curseur), etendre);
        }
    }

    pub fn tout(&mut self) {
        self.ancre = 0;
        self.curseur = self.texte.len();
    }

    pub fn couper(&mut self) -> String {
        let s = self.texte_choisi();
        self.effacer_selection();
        s
    }
}
