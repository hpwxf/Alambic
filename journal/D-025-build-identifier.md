# D-025 — Identifiant de build (hash court + `-dirty`) sur l'écran de démarrage des trois backends

| | |
|---|---|
| **Identifiant** | D-025 |
| **Date** | 2026-11-16 |
| **Auteur** | Pascal Havé |
| **Statut** | adoptée, révisée |
| **Commits** | `b2565ff` (ADR), `e8d5f85` (implémentation), `0a684ce` (correctif `ls-files`), révision : passage à vergen-gix |
| **Relations** | révise D-003 (un crate de plus) ; sert D-016 (savoir quelle image tourne) |

## 1. Contexte

L'écran de démarrage affiche `O&C Alambic v0.2.0`. La version ne suffit pas à dire **quelle image** tourne : entre
deux versions, tous les builds affichent la même chose, et le plugin Rack a déjà tourné avec un binaire périmé sans
que rien ne le montre (`9ccebca`). L'utilisateur veut, sous la version, le hash court du commit et `-dirty` si
l'arbre de travail diffère de ce commit, sur les trois backends.

Contraintes : `oc-core` est `no_std` et doit rester déterministe (ses tests et les écrans de référence ne peuvent pas
dépendre du commit courant) ; le firmware est croisé, le simulateur et le plugin sont hôtes ; un build hors dépôt
git (archive) doit rester possible.

## 2. Options envisagées

**A** — Un `build.rs` dans `oc-core` qui pose la valeur via `env!`. Une seule source, mais tout changement de
l'identifiant recompile le cœur et toute sa descendance, et la valeur entre dans les écrans de référence et les tests :
un écran de référence changerait à chaque commit.

**B** — Un `build.rs` dans chacun des trois crates terminaux (`oc-firmware`, `oc-sim`, `oc-vcv-ffi`), qui passent la
chaîne au cœur au démarrage (`Engine::set_build_id`). Le cœur ne fait que l'afficher ; par défaut elle est vide,
donc les tests restent déterministes. Mais la logique git serait copiée trois fois.

**C** — **B**, avec la logique dans un petit crate hôte `oc-buildinfo`, utilisé comme `[build-dependencies]` par les
trois crates terminaux.

## 3. Ce que l'assistant a proposé

**C**.

## 4. Ce que nous avons décidé, et pourquoi

**C** :

* `crates/oc-buildinfo` (hôte, `std`, aucune dépendance) expose `emit()`, appelée par le `build.rs` des trois crates
  terminaux. Elle pose `OC_BUILD_ID` = `git rev-parse --short HEAD`, suffixé `-dirty` si
  `git status --porcelain --untracked-files=no` n'est pas vide — les fichiers non suivis ne comptent pas, comme
  `git describe --dirty`. Hors dépôt git ou sans `git`, la valeur est vide et l'écran n'affiche que la version.
* `git status` est lancé avec `GIT_OPTIONAL_LOCKS=0` : sinon il réécrit `.git/index` au passage, ce qui relancerait
  le script au build suivant, indéfiniment.
* Relance du script : sur `.git/HEAD`, `.git/index`, les refs, **et chaque fichier suivi** (`git ls-files`), pour que
  modifier un source sans l'indexer passe bien en `-dirty`. Les artefacts non suivis (`target/`, `build/` du plugin)
  n'en font pas partie, donc pas de reconstruction en boucle.
* `oc-core` reste sans `build.rs` : `SplashScreen` porte un `build_id: &'static str`, `Engine::set_build_id` le
  pose, une chaîne vide n'affiche rien. Les écrans de référence et les tests du cœur ne voient jamais le commit.
* Chaque backend le pose au démarrage : `main.rs` du firmware (et dans son journal USB), la TUI et `replay` du
  simulateur, `oc_engine_new` pour Rack. La bibliothèque `Simulator::new` ne le pose pas : les scénarios de référence
  restent indépendants du commit.

## 5. Ce qui nous ferait changer d'avis

* Un temps de build visiblement alourdi par la relance sur chaque fichier suivi : se replier sur les seules
  métadonnées git, en acceptant un `-dirty` en retard sur les modifications non indexées.
* Un besoin de builds reproductibles bit à bit (même image depuis deux arbres identiques) : l'identifiant y est
  compatible, mais il faudrait alors une variable `OC_BUILD_ID` imposée par la CI plutôt que calculée.

## Révision (2026-11-16) — vergen-gix plutôt que le binaire `git`

L'utilisateur a relevé que la détection git était écrite à la main alors que des crates répandues le font. Comparées
(code lu, 2026-11-16) :

| Crate | Sans binaire `git` | `-dirty` | Notes |
|---|---|---|---|
| **vergen-gix** 10.0.3 | oui (gitoxide, pur Rust) | oui (suivis ; non suivis en option) | famille vergen ≈ 59 M téléchargements ; MSRV 1.96 |
| vergen-git2 | oui (libgit2, code C) | oui | compilateur C requis |
| vergen-gitcl | non | oui | même principe que l'implémentation initiale |
| `built` 0.8.1 (`gix`) | oui | oui | écrit un fichier Rust plutôt que des variables d'environnement |

Mesuré sur vergen-gix dans un projet jetable : 145 crates, ≈ 15 s de build propre ; et **un défaut** : vergen ne
relance le script que sur `.git/HEAD`, la ref courante et `build.rs` — un source modifié sans `git add` recompile le
crate mais laisse `VERGEN_GIT_DIRTY=false`.

Décidé (option retenue par l'utilisateur) : **vergen-gix**, avec la feature `emit_and_set` (API documentée ;
`emit_to` est `#[doc(hidden)]`), et `oc-buildinfo` qui garde deux responsabilités :

* composer `OC_BUILD_ID` (`VERGEN_GIT_SHA` court, `-dirty` si `VERGEN_GIT_DIRTY`), vide si vergen ne trouve pas de
  dépôt (valeur `VERGEN_IDEMPOTENT_OUTPUT`), `OC_BUILD_ID` de l'environnement toujours prioritaire ;
* **corriger le défaut de relance** sans git : `rerun-if-changed` sur les chemins sources du workspace (`Cargo.toml`,
  `Cargo.lock`, `rust-toolchain.toml`, `.cargo/config.toml`, `src`/`tests`/`benches`/`build.rs`/`Cargo.toml` de chaque
  crate et de `xtask`, sources, `Makefile`, `plugin.json` et `res/` du plugin, **hors** l'en-tête C que `cargo xtask
  vcv` y recopie), **et `.git/index`**, trouvé sans le binaire (`.git` dossier, ou fichier `gitdir:` d'un worktree),
  pour qu'un `git add` suffise. Jamais un dossier où un build écrit, sinon chaque build relance le suivant.

Prix accepté : une modification de documentation non indexée ne passe en `-dirty` qu'au prochain `git add` ou commit ;
145 crates de plus à compiler (une fois par profil). Gagné : plus de dépendance au binaire `git` (vérifié : build avec
un `PATH` sans `git`, identifiant correct), et la lecture du dépôt (worktrees, sous-modules, packs) confiée à
gitoxide plutôt qu'à du code maison.
