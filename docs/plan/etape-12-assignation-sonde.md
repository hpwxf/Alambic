# Étape 12 : assignation des CV par la sonde, sous-panneau commun (D-045)

## Context

La sonde (protocole v7, lot 10d/10e) lit et règle les paramètres d'une app par le trait `App`
(`param_specs` / `get` / `set`), et `ParamSpec::assignable` dit déjà qu'un paramètre prend une CV — mais
rien ne permet de lire ni de changer **son assignation** (`modulation::Assign` : entrée, `SCALE`, `SHIFT`,
`MIN` / `MAX`, effet). Le sous-menu est déjà commun sur le module (`oc_core::modulation::editor`, D-036),
mais l'état reste dupliqué dans chaque app (`TONE`, `RATCHET`, `SUPERSAW`) : `assigns: [Assign; N]`, `assigning: Option<AssignEditor>`,
`cv`, `assign()`, `assigning()`, `reading()`, et l'aiguillage `follow` / `observe` / `render_submenu`
copiés dans `apps/supersaw/mod.rs` (l. 299-355, 459-478) et `apps/ratchet/mod.rs` (l. 327-352, 397-422,
560-590).

But : un seul mécanisme, générique sur « tout paramètre `assignable` », servi au panneau, à la sonde, à la
CLI et à la page web ; aucune app n'écrit de code propre à la sonde pour ses CV.

Constat qui rend le générique possible : dans les deux apps, l'emplacement d'un `Assign` est **le rang du
paramètre parmi les `assignable`** de `PARAM_SPECS` (SUPERSAW : 0..5 = NOTE..LEVEL ; RATCHET : COUNT 0,
FIRST 1, SHAPE 2, PROB 3). L'index de paramètre suffit donc à nommer une assignation, sans table par app.

## Worktree

`git worktree add ../Alambic-assign -b probe-cv-assign master` (base `master` : `memory-budget-flexram`
n'est pas fusionnée et n'apporte rien ici). Travail et commits dans `../Alambic-assign` uniquement.

## Lots

### 12a — ADR et plan (premier commit)

* `journal/D-045-cv-assignment-by-the-trait.md` (proposée) : assignation au trait `App`, emplacement = rang
  parmi les `assignable`, état partagé dans `modulation`, protocole v8, `CENTER` côté hôte. Vérifier le
  numéro libre au moment du commit (la branche `entropic` a un `D-036` en collision : ne pas réutiliser).
  Index `journal/README.md` + tableau « Décisions actées » de `PLAN.md`.
* Copier ce plan en `docs/plan/etape-12-assignation-sonde.md` (un plan qui porte une décision doit être suivi
  par git), ligne de l'étape 12 dans `PLAN.md`.

### 12b — cœur : état mutualisé et trait

`crates/oc-core/src/modulation/` :
* `slot_of(specs: &[ParamSpec], index) -> Option<usize>` : rang parmi les `assignable`.
* `Assign::checked(self) -> Self` (ou `Result<_, Refused>`) : bornes de `turn` appliquées à une valeur venue
  d'ailleurs (`SCALE` ±200 %, `SHIFT` / `MIN` / `MAX` ±10 V, `min ≤ max`, arrondi aux pas) — une valeur
  réglée de loin se comporte comme une tournée à la main (même règle que `ParamSpec::check`).
* `Modulations<const N: usize>` (nom à confirmer, fichier `modulation/slots.rs`) : `assigns: [Assign; N]`,
  `editor: Option<AssignEditor>`, `cv`. Méthodes qui reprennent le code dupliqué : `update_cv`,
  `follow(slot, input, sweep) -> bool` (sous-menu ouvert ⇒ consommé), `open(slot)`, `observe(slot, elapsed)`,
  `reading(slot)`, `assign(slot)`, `line()`, `render(frame, slot, &Param)`, `as_slice()` /
  `as_mut_slice()`. Les dérivés serde (`Assign`, `Effect`, `CvChannel` si absent) pour le protocole.
* Trait `App` (`apps/mod.rs`) : `fn assigns(&self) -> &[Assign] { &[] }`,
  `fn assigns_mut(&mut self) -> &mut [Assign] { &mut [] }` ; puis, en méthodes par défaut sur ces deux-là,
  `fn assign(&self, param) -> Option<Assign>` et
  `fn set_assign(&mut self, param, Assign) -> Result<Assign, Refused>` (`NoSuch` si non assignable). Une app
  n'implémente que les deux accesseurs, plus `fn assign_sweep(&self, param) -> Option<(mV, mV)>` (la plage
  qui balaie le paramètre, pour `CENTER` côté hôte et la barre de la page).
* SUPERSAW et RATCHET branchées sur `Modulations` ; leurs accesseurs publics (`assign(Setting)`,
  `assigning()`) gardés en délégation pour les tests existants. **Écrans de référence identiques**,
  `tests/modulation.rs` et `tests/params.rs` inchangés au comportement près.
* Tests nouveaux (`crates/oc-core/tests/params.rs` ou `modulation.rs`) : pour chaque app de
  `AppHost`, `assign(i).is_some() == specs[i].assignable` ; `set_assign` hors bornes ramené ; effet visible
  dans `observe` / `params()` via `testing::mock_engine` avec une CV.

### 12c — protocole et serveur (v8)

`crates/oc-protocol/src/lib.rs`, `server.rs` :
* `VERSION = 8`. `Request::GetAssign { app, param }`, `Request::SetAssign { app, param, assign }` →
  `Reply::Assign(Assign)` (appliquée). `DescribeApp` émet un `Reply::Assign` après le `Spec` de chaque
  paramètre assignable : la page n'a qu'un échange à faire.
* Serveur par le trait (`Engine::app_mut`), comme `SetParam` ; erreurs par `Refused`.
* Tests aller-retour postcard + serveur (modèle des tests v4 existants).

### 12d — client et CLI

`crates/oc-probe/src/client.rs`, `apps.rs`, `main.rs` :
* `get_assign` / `set_assign` ; `apps::write_assign` (texte `+CV2 x0.50 +0.00V [-1.0,+5.0]`, réutilise
  `Assign::write` / le format des champs du sous-menu plutôt qu'un second format).
* CLI : `assign <app> <param>` (lecture), `assign <app> <param> --input cv2|none --effect set|add|mul
  --scale 50 --shift 0.5 --min -1 --max 5` (champs omis = gardés : lire, modifier, écrire).
* `--center` : fenêtre de lecture côté hôte sur `Stream` (deux tranches de `WINDOW_MICROS`), puis
  `Assign::center(seen, usable_range(sweep, effect))` — même fonction que le module. Nécessite le `sweep`
  du paramètre, `assign_sweep`, que `Reply::Assign` porte.

### 12e — page web : le sous-panneau commun

`crates/oc-web/src/applets.rs` + page : un composant `AssignPanel` unique, instancié sous chaque réglage
dont `assignable` est vrai (la « marque CV » de 10e devient son bouton d'ouverture) : entrée, effet,
`SCALE`, `SHIFT`, `MIN` / `MAX`, état lu de `Reply::Assign`, écrit par `SetAssign`. Mêmes fonctions de texte
que la CLI (`oc_probe::apps`). Tests contre le vrai serveur, comme 10e.

## Hors périmètre (noté en réserve)

* Persistance des assignations (réserve de l'étape 8) : l'accès par le trait la prépare (`assigns()` est ce
  que les réglages sauveraient), mais pas dans cette étape.
* Ouverture du sous-menu du module depuis la sonde.

## Vérification

* Commandes de fin de lot d'`AGENTS.md` (fmt, clippy hôte et cible, `cargo test --all-features`,
  `cargo xtask build && cargo xtask size`) ; `.text` / `.rodata` relevés dans `etape-12.md`.
* `cargo test -p oc-sim --test scenarios` sans `UPDATE_SCREENS` : écrans de référence inchangés après 12b.
* Module (TESTING 9.1b) : `cargo probe assign SUPERSAW NOTE`, puis `--input cv2 --effect add`, vérifier le
  `+CV2` à l'écran et le sous-menu du panneau ; `--center` avec un LFO sur CV2.
* Chrome (TESTING 9.1c) : panneau Apps, ouvrir le sous-panneau CV d'un réglage de chaque app.
