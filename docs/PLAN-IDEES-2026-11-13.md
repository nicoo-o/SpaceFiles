# Plan d'application des idées du 13/11/2026, en phases (écrit le 14/11/2026)

Complète `docs/IDEES-2026-11-13.md` (les idées) sans le modifier. **Aucune ligne de code n'est écrite tant que l'auteur
n'a pas lancé une phase** (décision Q-01). Chaque phase finit par : `cargo test` et `cargo build --release` lus, capture
quand l'écran change, rapport avec les commandes et leurs résultats. Tout ajout de dépendance passe par la mesure de
l'exe et de la mémoire du résident (Q-03).

Statuts vérifiés dans le code le 14/11/2026 par recherche de motifs (pas de lecture complète des modules).
**FAIT** = déjà dans le code · **PARTIEL** · **À FAIRE** · **NON VÉRIFIÉ** = aucune preuve, à contrôler avant la phase.

## Déjà fait : on ne le refait pas

| Idée | Preuve |
|---|---|
| A-01 écriture atomique unique | `src/atomique.rs:9` et `:41` ; utilisée par réglages, étiquettes, historique, index, session, protocole Eco |
| A-06 table d'accents | la table `ACCENTS` (`src/page.rs:159`) ne sert plus qu'à la casse des libellés de réglages ; rien à faire de plus |
| A-07 Espace sur un dossier | taille calculée et affichée (capture du 14/11 : « DOSSIER-UN · 0 O ») |
| A-08 erreur de chemin sans majuscules | `src/app.rs:1336` |
| P-05 état de l'index (textes) | `src/index.rs:915-921` ; **reste à vérifier à l'écran** où ils s'affichent |

## Phase 1 : l'index et les données ne mentent plus (effort S, la meilleure valeur)

| Idée | Ce que ça apporte, en simple | Verdict |
|---|---|---|
| **P-02** Débordement de surveillance traité (`src/index.rs:602`, il est jeté) | Un fichier créé pendant une extraction ou une compilation devient trouvable tout de suite, pas 6 h plus tard | **Bonne : à faire en premier** |
| **P-05** Montrer l'âge et l'état de l'index (vérifier l'écran) | On sait dans quel index on cherche, et on peut le mettre à jour | Bonne, dépend de P-02 |
| **A-02** (reste) Version de schéma dans `etiquettes.txt`, `historique.txt`, `index.bin` | Une version future peut changer un format sans casser les données de l'utilisateur ; seul `reglages.toml` l'a aujourd'hui | Bonne : à faire avant de publier une version |

## Phase 2 : petites finitions utiles (effort S chacune)

| Idée | Ce que ça apporte | Verdict |
|---|---|---|
| **P-03** Commande « qui utilise ce fichier ? » | Dit quel programme empêche de supprimer ou déplacer un fichier ; le code existe (`outils::programmes_sur`), il suffit de l'exposer | **Bonne** : très peu de code, beaucoup d'utilité |
| **A-03** Fil d'Ariane : « … » cliquable | Remonter dans un chemin long sans retaper ; aujourd'hui le « … » est du texte mort (`src/rendu.rs:1192`) | Bonne |
| **A-10** Aide contextuelle (`?` ou F1) | Les raccourcis clés (Espace, Ctrl+K) se découvrent ; aujourd'hui aucune aide | Bonne |
| **A-05** Recherche dans les Réglages | Trouver un réglage parmi 75 champs sans parcourir 12 catégories | Bonne |
| **A-11** Mode Essentiel nommé « Récents + épingles » dans les Réglages | Une fonction qui existe devient visible et compréhensible | Correcte (petit gain) |
| **A-12** Journal lisible dans l'app (sans chemins) | L'utilisateur voit pourquoi quelque chose a échoué ; utile aussi pour les retours publics | Bonne pour une version publique |
| **P-04** Aperçu texte avec numéros de ligne | Lire logs et code dans le volet d'aperçu | Confort (★), au choix |
| **A-09** (reste) Dernière ligne de la palette coupée | Finition visuelle ; le compteur existe déjà (`src/rendu.rs:5295`) | À vérifier à l'écran, puis petite correction |

## Phase 3 : les opérations longues (le plus gros gain d'usage ; effort M)

Ordre imposé : A-16, puis N-01, puis N-02. P-07 se décide avec N-01.

| Idée | Ce que ça apporte | Verdict |
|---|---|---|
| **A-16** Nommer l'attente (« préparation », « en attente du réseau ») | Une jauge immobile ne ressemble plus à un blocage ; aucun code de ce type aujourd'hui | **Bonne, prérequis** |
| **N-01** Centre d'opérations (liste, pause, annulation, cause d'échec) | Piloter une longue copie ou extraction au clavier, voir pourquoi elle a échoué ; aujourd'hui des pastilles éphémères par onglet | **Bonne, retenue par l'auteur** |
| **N-02** Reprise d'une opération interrompue (partage perdu, veille, disque retiré) | Ne pas perdre une copie de plusieurs Go parce que le réseau a coupé ; `noter_reprise` ne reprend aujourd'hui qu'une sélection | **Bonne, retenue par l'auteur** |
| **P-07** Plafonner les opérations simultanées | Aujourd'hui une opération à la fois par onglet mais **aucune limite entre onglets** : trois copies sur un disque lent le saturent | Bonne, à décider avec N-01 |

## Phase 4 : accessibilité et lisibilité

| Idée | Ce que ça apporte | Verdict |
|---|---|---|
| **A-13** Accessibilité UIA (motifs Selection et Invoke) | Un lecteur d'écran peut utiliser la liste ; aujourd'hui `GetPatternProvider` renvoie toujours une erreur (`src/accessibilite.rs:82`) | **Bonne : obligation plus que confort** (effort M) |
| **A-14** Texte au-delà de 150 % : hauteurs de ligne variables | Les gros textes ne sont plus rognés ; vérification partielle seulement | À vérifier puis à faire (M) |
| **A-15** Rendus inutiles (le reflet invalide la scène entière : 3,4 à 4,2 ms) | Moins de CPU et de batterie ; **vérifié le 14/11 : faux** (`invalider_reflet` n'invalide que la zone du reflet) | Inutile |

## Phase 5 : réglages crédibles

| Idée | Ce que ça apporte | Verdict |
|---|---|---|
| **A-04** Page Raccourcis éditable (capture de touche, conflits) | Changer un raccourci sans éditer un fichier ; aujourd'hui « rien à choisir » (`src/page.rs:856`) | Bonne mais M : ou mention honnête en attendant |

## Phase 6 : à arbitrer ou à mesurer avant tout code

| Idée | Ce que ça apporte | Ce qui bloque |
|---|---|---|
| **N-03** Marquage unifié (balises, étiquettes, groupes en un modèle) | Un seul système au lieu de trois qui se recouvrent | Migration des données non tranchée (Q-05) ; statut dans le code **non vérifié** |
| **N-04** Recherche de contenu indexée, optionnelle | `contenu:` ne relit plus 4 000 fichiers à chaque requête | Effort L ; mesure disque et CPU ; l'index ne contient aujourd'hui que des noms |
| **N-05** Espaces de travail nommés | Rappeler un jeu d'onglets d'un raccourci | Fait le 14/11 : les sessions existantes couvraient déjà le jeu d'onglets ; on y a ajouté le panneau et l'onglet actif |
| **P-01** Aperçu par les gestionnaires système (`IPreviewHandler`) | PDF, Office, vidéo dans le volet sans dépendance | Effort L, ★★★, mais des gestionnaires qui peuvent bloquer : hôte optionnel avec délai |
| **P-06** Export du diff des deux panneaux | Garder une trace d'une synchronisation | Aujourd'hui seul un diff entre deux fichiers existe |

## En réserve ou refusées

- **En réserve :** E-01 à E-06 (actions utilisateur, plus de deux panneaux, miniatures vidéo, Everything, MTP, historique filtrable).
- **Refusées, maintenues :** X-01 à X-07, et X-08 (menu contextuel Windows 11) tant que l'exe n'est pas signé.
- **Le découpage profond de `rendu.rs` et `procedure.rs`** passe après la phase 3, ou au fil des modifications de ces fichiers.

## Ordre conseillé

Phase 1 → 2 → 3 → 4 → 5 → 6. Les phases 1 et 2 suffisent à une version publique plus honnête ; la phase 3 est celle que
l'utilisateur remarque le plus.
