// Terminal intégré (PLAN-TERMINAL). T0 : prototype jetable de session ConPTY, compilé seulement pour les tests.
// T1 : `vt`, l'émulateur (flux d'octets → grille, curseur, historique), sans Win32.
// T2a : `session`, un vrai shell dans une pseudo-console ConPTY (fils de lecture et d'écriture, job qui tue les descendants).

// T2b : `panneau`, l'état d'un terminal affiché (session + émulateur + défilement + sélection + traduction des touches).

#[cfg(test)]
mod essai_t0;
// T4a : arithmétique des onglets du panneau (rang actif, fermeture, libellé)
pub mod onglets;
pub mod panneau;
// T5 : la ligne de saisie (éditeur) et l'historique des commandes (suggestion grise, rappel)
pub mod historique;
// T6 : shell mixte, décide si une ligne soumise passe telle quelle à PowerShell ou par cmd (préfixes `cmd:` / `ps:`)
pub mod melange;
// T7 : complétion par Tab (chemins, commandes, sous-commandes et options d'une base embarquée), module pur
pub mod completion;
pub mod saisie;
pub mod session;
// T3 : la souris et la molette vers les programmes plein écran (encodage pur)
pub mod souris;
pub mod vt;
