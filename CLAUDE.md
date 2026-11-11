# SpaceFiles (ex-Nothing Explorer)

@AGENTS.md

Ce fichier ne garde que ce qui est propre à Claude Code. Les règles du projet sont dans `AGENTS.md`.

## Ce qui est imposé par des hooks (`.claude/settings.json`)

- `bloquer-souris.js` (PreToolUse) : refuse toute commande ou outil qui déplace la souris de l'auteur (renforcé le
  10/11/2026 ; un jumeau global, `~/.claude/hooks/gardes.js`, protège les autres projets et demande l'accord avant
  un `git` qui publie ou détruit).
- `verifier-fin-de-tour.js` (Stop) : si le Rust a changé, lance `cargo build --release` puis `cargo test` et refuse de
  finir s'ils échouent ou si le nombre de tests baisse (3 refus au plus par état, puis il laisse passer).

## Outils du projet

- Skills : `/snapshot` (capture sans entrée), `/audit` (lecture seule, rapport dans `docs/audit/`), `/cloture`
  (état dans `PLAN.md`, mémoire), `/deleguer` (planifier puis confier à une session Sonnet et vérifier).
- Skill global `/spacehub` : vue d'ensemble de l'écosystème (SpaceNotch, Hub, SpaceHub) et état de chaque app ; à
  lancer avant une tâche qui touche le dialogue entre apps (`src/eco/`).
- Subagents : `reperage` (Haiku, recherche mécanique : à lui confier plutôt que de chercher soi-même),
  `implementeur` (Sonnet, tâche cadrée), `rust-reviewer` (Opus, après toute modif Win32, Direct2D, COM ou `unsafe`).
- Relecture de sécurité approfondie du Rust (`unsafe`, FFI, paniques, concurrence) : `/rust-review:rust-review`
  (plugin Trail of Bits, ajouté le 11/11/2026). **Seulement à la demande de l'auteur** : il lance plusieurs
  sous-agents et pèse lourd sur le plafond du compte ; choisir `haiku` ou `sonnet` comme modèle des ouvriers et un
  sous-dossier précis. Pour une relecture courante, `rust-reviewer` suffit. Bogue difficile : skill global
  `diagnosing-bugs` (construire d'abord une commande qui échoue sur le bogue).
- Plugins : `rust-analyzer-lsp` (navigation dans le code). `superpowers` et `ponytail` sont désactivés : le premier
  impose un brainstorming à chaque demande, le second coupe le travail demandé.

## Façon de travailler

- Tâche de moins de 20 minutes : la faire directement, sans brainstorming ni plan. Gros chantier (nouvelle vue,
  nouveau moteur de fichiers, refonte de la fenêtre) : plan en quelques lignes validé avant le code, étapes dans
  `PLAN.md`. Pas de worktree.
- Compte Pro : Opus pèse sur le même plafond que Sonnet. Défaut `sonnet` ; `/model opusplan` pour un gros chantier ;
  Opus seulement pour planifier (`/deleguer`) et relire. `haiku` pour la recherche pure. `CLAUDE_CODE_SUBAGENT_MODEL=sonnet` est posé dans `~/.claude/settings.json` (11/11/2026) : il ne vaut que pour les
  sous-agents sans `model:` (`general-purpose`, `Explore`) ; le `model:` de chaque agent du projet garde la priorité.
  Ne pas définir `CLAUDE_CODE_SUBAGENT_MODEL_FORCE`, qui, lui, les écraserait.
- Captures : au-delà de deux images, les faire lire par le sous-agent global `regard` (voir `/snapshot`).
- Une session coûte par sa longueur : `/clear` entre deux tâches sans lien, `/compact` quand le contexte grossit.
  L'état durable vit dans `PLAN.md`, pas dans la conversation.

## Compact instructions

À la compaction, conserver : les fichiers modifiés, les commandes de build et de test lancées avec leur résultat
exact, le point en cours de `docs/ORDRE.md`, et les décisions de l'auteur pas encore écrites dans `PLAN.md`.

## graphify

This project has a knowledge graph at graphify-out/ with god nodes, community structure, and cross-file relationships.

Rules:
- When graphify-out/graph.json exists, run `graphify query "<question>"` before your first grep, search or source-file read for a codebase question, and put this rule in every subagent prompt that explores code. Read raw files only for lines the graph or an earlier read already located. Use `graphify path "<A>" "<B>"` for relationships and `graphify explain "<concept>"` for focused concepts. These return a scoped subgraph, usually much smaller than GRAPH_REPORT.md or raw grep output.
- If graphify-out/wiki/index.md exists, use it for broad navigation instead of raw source browsing.
- Read graphify-out/GRAPH_REPORT.md only for broad architecture review or when query/path/explain do not surface enough context.
- After modifying code, run `graphify update .` to keep the graph current (AST-only, no API cost).
- A committed graphify-out/graph.json is rewritten whole on every build, so a large diff to it in a pull request is expected. Do not flag it as churn or suggest skipping the graph rebuild or its hooks.
