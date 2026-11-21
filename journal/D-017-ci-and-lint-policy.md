# D-017 — CI GitHub Actions, `clippy::pedantic` hôte et ARM, toolchain `stable` non épinglée

| | |
|---|---|
| **Identifiant** | D-017 |
| **Date** | 2026-11-16 (rétrospective ; décision prise le 2026-08-01) |
| **Auteur** | Pascal Havé |
| **Statut** | adoptée (rétrospective), révisée |
| **Commits** | `1524760` (`ci.yml`, lints de workspace, `rust-toolchain.toml`), `8bdd9bc` (correctif clippy qui bloquait `-D warnings`), 2026-11-16 (toolchain épinglée 1.100.0 ; tests macOS à la demande) |

## 1. Contexte

Le plan voulait que la CI compile les trois cibles, exécute les tests et les benchmarks, avec `deny(warnings)`,
`clippy::pedantic` et `rustfmt`. Le firmware est exclu de `default-members` (D-003), donc un `clippy` nu ne le voit
pas ; et `RUSTFLAGS` ne peut pas servir à refuser les avertissements (D-004).

## 2. Options envisagées

**A** — CI minimale (fmt + test hôte). **B** — CI complète : `rustfmt` ; `clippy --all-targets --all-features
-D warnings` sur l'hôte **et** `clippy -p oc-firmware --target thumbv7em-none-eabihf -D warnings` ; tests sur
Linux et macOS ; image firmware (`build`, `size`, `hex`) publiée en artefact ; benchmarks compilés (`--no-run`).

Pour la toolchain : canal `stable` flottant, ou version épinglée (comme `make_sound/D-016`, Rust 1.98.1).

## 3. Ce que l'assistant a proposé

**B**, et le canal `stable` (`1524760`). `rust-version = "1.85"` est déclaré dans le workspace.

## 4. Ce que nous avons décidé, et pourquoi

**B**. Lints de workspace : `clippy::pedantic` (warn), `missing_debug_implementations`, `unreachable_pub`,
`rust_2018_idioms` ; seul `module_name_repetitions` est permis globalement, tout autre `#[allow]` est local et
justifié en une ligne. Le plugin VCV est hors CI (SDK téléchargé à la main, `190fb3f`).

## 5. Ce qui nous ferait changer d'avis

* Une CI qui casse sans changement de code, parce qu'un nouveau `stable` ajoute un lint `pedantic` : c'est le prix du
  canal flottant combiné à `-D warnings`.
* Un contributeur sur la MSRV déclarée qui ne compile pas : la MSRV n'est vérifiée nulle part.

## Révision (2026-11-16) — toolchain épinglée, MSRV = toolchain

`rust-toolchain.toml` passe du canal `stable` à **`1.100.0`**, et `rust-version` de `1.85` à **`1.100`**, à la demande
de l'utilisateur. Une seule version fait foi : celle qu'utilisent le développement local et la CI, qui est aussi la
version minimale déclarée — la MSRV est donc testée par chaque job, sans job dédié. La monter est un commit délibéré
(changer les deux valeurs, corriger les nouveaux lints).

Constaté au passage, exactement le risque du point ouvert : clippy 1.100 a fait échouer `-D warnings` sur du code
inchangé (`manual_is_multiple_of`, `collapsible_if` avec les chaînes de `let` de l'édition 2024,
`assert_is_empty`). Huit corrections mécaniques, sans changement de comportement, dans le même commit. Le passage
ouvre aussi la voie aux dépendances qui exigent une version récente (vergen 10 : 1.96, D-025).

## Révision (2026-11-16) — tests macOS à la demande

À la demande de l'utilisateur, le job de tests ne tourne plus par défaut que sur Linux. Les runners macOS sont lents
et facturés bien plus cher ; le job `test-macos` ne s'exécute que sur lancement manuel (`workflow_dispatch`, case
« macos » cochée). Ce qui est perdu par défaut : une régression propre à macOS (chemins, `make`/SDK, `llvm-tools` du
sysroot) n'est plus vue à chaque push ; elle reste couverte par le développement local, qui se fait sur macOS, et par
un lancement manuel avant une version.

## Points ouverts (revue du 2026-11-16)

* **Traité au lot 3a** (D-013, révision du 2026-11-16 ; « fait » = commandes CI dans `AGENTS.md`, D-001). **La CI échouait** sur `cargo test --all-features` (D-013 : feature `octant` contre écrans de référence en
  braille). La commande documentée dans `CLAUDE.md` (`cargo test`) ne lance pas les features et passe : la
  définition locale de « fait » et la CI divergent.
* **Traité** (révision ci-dessus), sauf `cargo deny` (point suivant). Constat d'origine : canal `stable` non épinglé + `-D warnings` + `pedantic` : CI non reproductible dans le temps. Et `rust-version =
  "1.85"` n'est testé par aucun job. Épingler une version (et la monter délibérément), ou ajouter un job MSRV.
* Pas de politique de dépendances (`cargo deny` : licences, sources, avis de sécurité), contrairement à
  `make_sound/D-008` et `D-016`. Lié à D-022.
