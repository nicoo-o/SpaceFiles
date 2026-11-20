# T2 — état au 18/11/2026 (non commité, non installé)

## Fait et vérifié
- `cargo test` : 920 réussis, 0 échec, 11 ignorés (le test chronométré `eco::reutiliser::...fait_replier_tout_de_suite` a échoué une fois sous charge, réussi ensuite).
- `cargo build --release` : réussi, exe 4 657 664 octets (avant T2 : 4 576 768, soit +80 896).
- Capture réelle (`--terminal`, profil isolé, messages postés à la fenêtre, aucune souris) : PowerShell démarre dans le panneau, prompt, curseur bloc, sortie de 40 lignes avec défilement, `é ✓ 日本` corrects (double largeur comprise), en-tête sans chevauchement. **Faite avant les corrections de relecture ci-dessous** : pas refaite après.
- Une Entrée envoyée = une Entrée reçue par le shell (mesuré avec un gestionnaire PSReadLine). Un doublon vu en route venait d'un `lParam` de test mal formé, pas du code.
- Mémoire du processus avec panneau ouvert et une session PowerShell : environ 71 Mo de working set (le résident seul : 19 Mo). Mesure grossière, un seul essai.
- Glyphe bleu au-dessus du prompt : le prompt de l'auteur utilise une icône absente de JetBrains Mono (police de repli à prévoir, T3).

## Relecture `rust-reviewer` (aucun bloquant)

### Corrigés le 18/11/2026 (tests 920 / build release OK, pas revus à l'écran)
1. Échap sans focus ne ferme plus le panneau tant que le shell est actif (on sort par Ctrl+Alt+T ou le bouton) ; shell terminé : Échap ferme.
3. Code de sortie : `WaitForSingleObject(processus, 0)` d'abord, plus de confusion avec 259 (`STILL_ACTIVE`) ; constante `EN_COURS` supprimée.
4. F10 n'est plus lu comme Alt+F10 (Alt = bit 29 du `lParam` du `WM_SYSKEYDOWN`, ou touche Alt enfoncée).
5. `fermer_terminal` remet `terminal_glisse` à faux et fait `ReleaseCapture` s'il y avait un glissement.
12. Commentaires `SAFETY` / sections `# Safety` ajoutés (session.rs, grille_terminal.rs, terminal_clavier.rs).

### Restent à traiter (non bloquants, hors de ce lot)
2. Fermer le panneau tue aussi les applications lancées depuis le shell (JobObject `KILL_ON_JOB_CLOSE`), sans confirmation ; la garde de `WM_CLOSE` ne regarde pas le terminal.
6. Chemin « sous modale » de la destruction : la session n'est pas détruite.
7. Canal d'écriture non borné (collage de centaines de Mo).
8. Liste d'attributs ConPTY dans un `Vec<u8>` : utiliser `Vec<usize>`.
9. Débit non mesuré (`yes`) : `pomper` peut passer 4 Mo d'un coup sur le fil d'interface.
10. Alt + touche non alphanumérique (AZERTY) mal envoyée au shell.
11. `x_de` arrondit en DIP et non en pixels (filets possibles à 125 % / 150 %).
13. `WM_CAPTURECHANGED` pas traité (capture perdue pendant un glissement).

## À vérifier à la souris (auteur) — reportés dans `docs/ui/rodage-f1-2026-11-09.md`
Molette dans le panneau, sélection au glisser, copie, clic pour donner/retirer le focus, glisser de la séparation, Échap avec shell actif.

## Décisions prises
D-T1 = JetBrains Mono 13 ; fermer le panneau détruit la session ; journal des opérations absent du panneau jusqu'à T4 ; Échap avec focus va au shell, sans focus il ne ferme pas un shell actif ; pas de clignotement ni d'italique.
