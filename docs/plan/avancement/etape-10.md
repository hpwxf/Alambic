# Étape 10 — apps modulaires : trait `App` et modèle de paramètres (avancement, terminée le 2026-11-24)

Décision : D-043, proposée le 2026-11-20, adoptée sur mesure le 2026-11-21, révisée trois fois le même jour au fil
des essais de la page ; révise D-021 (`AppHost` fermé) et précise D-002. Étape 1 de la trajectoire de D-043. Branche
`d043-app-trait` (PR #10).

## Fait

| Lot | Commits | Contenu |
|---|---|---|
| plan | `8e475a4` | D-043 proposée : trait, registre sans accesseurs par app, modèle de paramètres, trajectoire en cinq étapes |
| 10a | `f295b3f` | trait `App` (`id`, `name`, `update`, `render`, `Any` en supertrait) ; `AppHost::get` / `get_mut` / `find::<T>()`, aiguillage par `&dyn App` ; accesseurs par app retirés d'`AppHost`, d'`Engine` (`app::<T>()`) et d'`oc-sim` ; `AppId` reste la liste de composition et la clé persistée |
| 10b | `dc042fc` | `oc_core::apps::param` : `ParamSpec` (`Number` / `Choice` / `Toggle`, décimales, unité, `assignable`), `ParamValue`, `Refused`, `write_summary` ; trait : `param_specs`, `get`, `set`, `observables`, `observe`, `actions`, `act` ; les six apps décrites ; ligne d'état du simulateur tirée du modèle ; `tests/params.rs` |
| 10d | `b5c444a` | protocole v4 : `ListApps`, `DescribeApp`, `GetParam`, `SetParam`, `Observe`, `Act`, `SelectApp` ; serveur par le trait ; `oc-probe` : client, module `apps`, CLI `apps`, `app`, `get`, `set`, `act`, `select` |
| 10c | `8b241ee` | `cargo probe stats` sur le module avant et après ; D-043 adoptée |
| 10e | `ff2c4e4`, `db5a8df` | panneau *Apps* de la page : liste, choix, *Run on the module*, réglages éditables, actions, observables ; `oc-web` : `AppList`, `AppView`, `Editor`, mêmes fonctions de texte que la CLI ; URL de la page marquées par l'empreinte du build |
| suites | `0ffeee3`, `69df592`, `f80ed9a` | relecture après un `set` (un réglage en déplace d'autres) ; libellé qui suit l'écran à côté du nom stable (protocole 5) ; faces inactives grisées (protocole 6) ; révisions de D-043 |

## Mesures

* **Écrans de référence** et tests inchangés après 10a et 10b.
* **Hôte** (criterion) : `engine/tick_*` et `voice/*` inchangés au bruit près (10a).
* **Code** (différence de `cargo xtask size`) : 10a `.text` +724 o, `.rodata` +192 o (vtables) ; 10b `.text`
  +2 784 o, `.rodata` +1 840 o (descripteurs, noms) ; 10d `.text` +5 908 o, `.rodata` +980 o. Lot entier : `.text`
  +9,2 Kio (163 Kio d'ITCM sur 192), `.rodata` +2,9 Kio.
* **Module** (10c, `SUPERSAW` `F32`, `NOTE` et `DETUNE` sous bruit, `STEREO ON`, `8e475a4` contre `b5c444a`) :
  `voice` 1,66 → 1,68 µs (max 1,84 → 1,75), `audio` 4,23 → 4,22 µs, `voice_params` 0,47 µs, `update` 15,10 →
  14,87 µs, `render` 53,9 µs, `tick` 26,6 µs ; gigue et trame identiques. Le trait n'a pas fui dans l'interruption.

## Essais sur le module

* CLI (TESTING 9.1b, étape 6) : `apps`, `app`, `get`, `set`, `act`, `select` sur les six apps.
* Page (TESTING 9.1c), dans Chrome : essayée par l'utilisateur dès le 2026-11-21, ce qui a amené les révisions
  (relecture après `GATE` off, libellé `SPAN`, faces inactives), puis de nouveau à l'étape 12 avec le sous-panneau
  CV : concluant.

## Réserves

* Les observables ne passent pas dans `Stream` : rien ne l'a encore demandé.
* Pas de clé `AppKey` séparée : `AppId` reste la liste de l'image et la clé persistée, jusqu'à la composition hors du
  cœur (étape 4 de la trajectoire).
* Une seule app tourne à la fois ; les sorties appartiennent toutes à l'app en cours et `MAILBOX` porte le `Setup`
  de la composition entière (`apps::sound`) : c'est l'objet de l'étape 13 (D-046).
* Composition en crates non faite (étape 4 de la trajectoire, si l'ITCM ou des apps tierces le demandent).
