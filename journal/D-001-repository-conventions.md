# D-001 — Conventions du dépôt : `PLAN.md` index, `docs/plan/`, journal, langue, définition de « fait »

| | |
|---|---|
| **Identifiant** | D-001 |
| **Date** | 2026-11-16 |
| **Auteur** | Pascal Havé |
| **Statut** | adoptée, révisée |
| **Commits** | commit d'ouverture du journal (D-001 à D-023) |

## 1. Contexte

Le dépôt a 24 commits, écrits en sessions d'assistants successives (Junie en août, Claude en septembre) qui ne
partagent aucun contexte. La continuité reposait sur quatre supports hétérogènes :

* `.junie/plans/oc-rust-firmware-foundation.md` — le plan de fondation, en français, avec ses « Key Decisions »
  et ses écarts (pas de `memory.x`, pas de `flip-link`, Renode non fait). `AGENTS.md` le désigne comme l'endroit
  où vit « the historical rationale for every decision » ;
* `AGENTS.md`, `TESTING.md`, `README.md`, `crates/oc-firmware/MEMORY.md` — documentation technique en anglais, à
  jour sur certains points et périmée sur d'autres (voir D-011, D-015) ;
* les messages de commit, très argumentés pour certains (`a839a03`, `8bdd9bc`), d'une ligne pour d'autres ;
* des fichiers **exclus de git** par `.git/info/exclude` : `HARDWARE.md`, `TODO.md` et un second plan,
  `.junie/plans/isr-16khz-dual-rate.md`, qui contient la prochaine décision d'architecture (D-023) — déplacé sous
  `docs/plan/etape-3-plan-a-isr-16khz.md` au lot 3a.

Aucun de ces supports ne répond à « pourquoi ce choix, contre quoi, et qu'est-ce qui le remettrait en cause ».
Les dépôts frères `make_sound` (D-001) et `slidekit` (D-002, D-016) ont déjà payé cette question.

## 2. Options envisagées

**A** — Garder l'existant : le plan Junie comme mémoire des décisions, `AGENTS.md` comme référence. Rien à écrire,
mais le plan Junie est un document de livraison figé (ses étapes sont cochées) : il ne sait pas recevoir les
décisions postérieures (menu d'applets, SH1106, USB CDC) ni leurs révisions.

**B** — Un `PLAN.md` unique qui absorbe tout. Simple, mais `slidekit` a dû l'éclater à 89 Ko (slidekit/D-016).

**C** — L'état final de `make_sound` : `PLAN.md` réduit à un **index** (carte, bilan d'une ligne par étape, étape en
cours, décisions actées), détail sous `docs/plan/`, arbitrages dans `journal/`, entrées antérieures reconstruites
et marquées comme telles (méthode `slidekit`).

## 3. Ce que l'assistant a proposé

**C**, en reprenant le format d'ADR de `make_sound/journal/README.md` (cinq champs, règle d'antériorité), la section
« entrées rétrospectives » de `slidekit/journal/README.md`, et en ajoutant une section « Points ouverts (revue) »
pour consigner les doutes de la relecture sans changer le statut des décisions.

## 4. Ce que nous avons décidé, et pourquoi

**C**, avec les conventions suivantes :

* **Documents** : `PLAN.md` est le point d'entrée imposé par `AGENTS.md`. `docs/plan/roadmap.md` (étapes, questions
  ouvertes), `docs/plan/avancement/etape-N.md` (fait, mesuré, corrigé, pour chaque étape terminée), un fichier
  `docs/plan/etape-N-*.md` au démarrage de chaque étape. L'architecture reste dans `AGENTS.md` (déjà exacte sur la
  structure, et lue par les agents) : pas de `docs/plan/architecture.md` en doublon.
* **Consignes aux agents** : `AGENTS.md` est la **seule source de vérité** pour tous les agents (Claude Code, Junie,
  …). `CLAUDE.md` n'est plus suivi par git (`.gitignore`) : fichier local qui importe `AGENTS.md` (`@AGENTS.md`) et
  n'ajoute que des notes propres à Claude Code. Ce qui compte pour le dépôt (commandes, règles, plan, journal) va
  dans `AGENTS.md`, jamais seulement dans un fichier d'outil.
* **Plans d'assistant** : le plan de fondation `.junie/plans/oc-rust-firmware-foundation.md` reste en place comme plan
  de l'étape 0 ; un plan d'assistant qui porte une décision d'architecture doit être **suivi par git** — le plan ISR
  exclu par `.git/info/exclude` ne peut pas rester la seule trace de D-023.
* **Langue** : documents de travail (`PLAN.md`, `docs/plan/`, `journal/`) en français ; code, commentaires, messages
  de commit, `README.md`, `AGENTS.md`, `TESTING.md` en anglais.
* **Définition de « fait »** : `cargo fmt --all -- --check`, `cargo clippy --all-targets --all-features -- -D warnings`,
  `cargo clippy -p oc-firmware --target thumbv7em-none-eabihf -- -D warnings`, `cargo test --all-features` et
  `cargo xtask build && cargo xtask size` passent en local — c'est-à-dire **exactement ce que rejoue la CI**
  (D-017). Toute différence entre la commande locale et la commande CI est un défaut (voir D-013, point ouvert).
* **Écarts consignés** : quand l'implémentation contredit un plan ou une décision, on corrige le plan et on révise
  l'ADR dans le même commit.

## 5. Ce qui nous ferait changer d'avis

* Si une étape se termine sans fichier d'avancement, ou si une décision d'architecture entre dans un commit sans
  entrée du journal : la convention n'est pas tenue et il faut la simplifier plutôt que la laisser pourrir.
* Si `AGENTS.md` et le journal se mettent à se contredire, il faudra sortir l'architecture dans
  `docs/plan/architecture.md` (en français) et réduire `AGENTS.md` aux pièges.

## Révision du 2026-11-18 — `HARDWARE.md` versionné sous `docs/hardware.md`

À la demande de l'utilisateur, le guide du matériel, jusque-là un fichier local non suivi, devient
`docs/hardware.md`, en français comme les autres documents de travail. Il distingue ce qui relève du principe, de la
spécification (fiches, manuel de référence), de ce module (assemblage et mesures) et du firmware ; la même distinction
vaut pour les rapports d'étape. La partie générique de l'ancien fichier (exemples RTIC, tableau Linux / embarqué,
« ADC externes ») n'est pas reprise : elle ne décrivait pas ce module.


## Révision du 2026-11-20 — `AGENTS.md` réduit, détail dans `docs/agents/`

À la demande de l'utilisateur. `AGENTS.md` faisait 446 lignes, chargées à chaque session d'agent, et répétait le
code (docs de module de `main.rs`, `audio.rs`, `oc_core::audio`), `PLAN.md` (les réserves des étapes),
`crates/oc-firmware/MEMORY.md` et lui-même (tableau des crates et index des fichiers) ; il portait aussi des mesures
déjà périmées (coût de l'interruption audio). La cause est la taille, pas une contradiction avec le journal : la
piste du § 5 (`docs/plan/architecture.md` en français) n'est pas prise.

* `AGENTS.md` garde ce que toute tâche demande : où lire, règles de travail (plan, journal, langue, « fait »),
  commandes courantes, règles faciles à casser, style. Il reste la **seule source de vérité** des agents.
* Le détail par domaine passe dans des annexes en anglais que `AGENTS.md` route, lues quand la tâche touche leur
  domaine : `docs/agents/firmware.md` (chaîne croisée, les deux contextes, l'écran, la mesure des coûts),
  `docs/agents/code-map.md` (les crates, où vit quoi), `docs/agents/tools.md` (sonde, navigateur, VCV Rack,
  publication, bancs). Ce qu'une docs de module dit déjà n'y est pas recopié : elles y renvoient.
* Aucune mesure dans ces fichiers (elles vont dans `docs/plan/avancement/`) ; les lacunes connues renvoient aux
  réserves de `PLAN.md`.

Ce qui ferait revenir en arrière : un agent qui casse une règle parce qu'elle n'est plus que dans une annexe (la
remonter dans `AGENTS.md`) ; une annexe qui dérive du code (la réduire à des renvois vers les docs de module).
