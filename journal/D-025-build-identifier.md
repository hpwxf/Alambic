# D-025 — Identifiant de build (hash court + `-dirty`) sur l'écran de démarrage des trois backends

| | |
|---|---|
| **Identifiant** | D-025 |
| **Date** | 2026-11-16 |
| **Auteur** | Pascal Havé |
| **Statut** | adoptée |
| **Commits** | ce commit (ADR) ; implémentation dans le commit suivant (`crates/oc-buildinfo`) |
| **Relations** | révise D-003 (un crate de plus) ; sert D-016 (savoir quelle image tourne) |

## 1. Contexte

L'écran de démarrage affiche `O&C Alambic v0.2.0`. La version ne suffit pas à dire **quelle image** tourne : entre
deux versions, tous les builds affichent la même chose, et le plugin Rack a déjà tourné avec un binaire périmé sans
que rien ne le montre (`9ccebca`). L'utilisateur veut, sous la version, le hash court du commit et `-dirty` si
l'arbre de travail diffère de ce commit, sur les trois backends.

Contraintes : `oc-core` est `no_std` et doit rester déterministe (ses tests et les écrans dorés ne peuvent pas
dépendre du commit courant) ; le firmware est croisé, le simulateur et le plugin sont hôtes ; un build hors dépôt
git (archive) doit rester possible.

## 2. Options envisagées

**A** — Un `build.rs` dans `oc-core` qui pose la valeur via `env!`. Une seule source, mais tout changement de
l'identifiant recompile le cœur et toute sa descendance, et la valeur entre dans les écrans dorés et les tests :
un écran doré changerait à chaque commit.

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
  pose, une chaîne vide n'affiche rien. Les écrans dorés et les tests du cœur ne voient jamais le commit.
* Chaque backend le pose au démarrage : `main.rs` du firmware (et dans son journal USB), la TUI et `replay` du
  simulateur, `oc_engine_new` pour Rack. La bibliothèque `Simulator::new` ne le pose pas : les scénarios dorés
  restent indépendants du commit.

## 5. Ce qui nous ferait changer d'avis

* Un temps de build visiblement alourdi par la relance sur chaque fichier suivi : se replier sur les seules
  métadonnées git, en acceptant un `-dirty` en retard sur les modifications non indexées.
* Un besoin de builds reproductibles bit à bit (même image depuis deux arbres identiques) : l'identifiant y est
  compatible, mais il faudrait alors une variable `OC_BUILD_ID` imposée par la CI plutôt que calculée.
