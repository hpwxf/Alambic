# D-003 — Découpage en crates ; `oc-firmware` hors `default-members`

| | |
|---|---|
| **Identifiant** | D-003 |
| **Date** | 2026-11-16 (rétrospective ; décision prise le 2026-08-01) |
| **Auteur** | Pascal Havé |
| **Statut** | adoptée (rétrospective), révisée |
| **Commits** | `1524760` (workspace à cinq membres), `ec140ed` (ajout de `oc-drivers`), D-025 (ajout de `oc-buildinfo`) |

## 1. Contexte

D-002 isole le comportement. Restent trois natures de code : les protocoles de périphériques (DAC8565, OLED,
lecture des triggers et du panneau), le câblage d'une carte précise, et l'outillage hôte (simulateur, ABI C, build
et flash). Une contrainte pratique s'ajoute : le firmware ne compile que pour `thumbv7em-none-eabihf`.

## 2. Options envisagées

**A** — Cinq crates comme dans le plan : `oc-core`, `oc-firmware` (qui contient aussi les pilotes), `oc-sim`,
`oc-vcv-ffi`, `xtask`. Les pilotes ne sont alors testables que sur la cible.

**B** — Sortir les pilotes dans un sixième crate `oc-drivers`, `no_std`, `forbid(unsafe_code)`, écrit **uniquement**
contre `embedded-hal` 1.0, testé sur l'hôte contre des bus simulés qui enregistrent les octets écrits.

Et, pour le firmware : membre normal du workspace (chaque `cargo build` hôte échoue ou demande `--exclude`), ou
membre exclu de `default-members`.

## 3. Ce que l'assistant a proposé

Le plan Junie prévoyait **A** (« `oc-firmware/` # bin ARM : teensy4-bsp, ADC, LPSPI, DAC8565, SSD1309 »). Au
moment d'écrire le firmware, l'assistant a fait **B** : le message de `ec140ed` justifie le nouveau crate par la
testabilité (« fully exercised on the host against recording mock buses ») et par le fait que `oc-firmware` devient
« pure wiring with no `unsafe` of its own ». L'exclusion de `default-members` est dans `1524760`.

## 4. Ce que nous avons décidé, et pourquoi

**B**, et `oc-firmware` exclu de `default-members`. Rôles :

| Crate | Rôle |
|---|---|
| `oc-core` | tout le comportement (D-002) |
| `oc-drivers` | protocoles de périphériques sur `embedded-hal` 1.0, testés hôte |
| `oc-firmware` | câblage `teensy4-bsp` / `imxrt-hal` → traits ; seul crate qui touche aux registres (via le HAL) |
| `oc-sim` | backend hôte + TUI (D-013) |
| `oc-vcv-ffi` | ABI C pour Rack (D-015) |
| `xtask` | seul point d'entrée build / flash / VCV (D-016) |

Le gain de B s'est vérifié à la première mise en route : le défaut de l'OLED (`3f50a42`, mode d'adressage SSD1306
au lieu de SH1106, plafond de trame LPSPI) s'est corrigé dans `oc-drivers`, avec des tests hôte, sans toucher au
comportement.

L'exclusion du firmware fait que `cargo build|test|clippy` nus ne touchent que l'hôte ; le prix est que le firmware
doit être linté **à part** (`cargo clippy -p oc-firmware --target …`), ce que la CI fait (D-017).

## 5. Ce qui nous ferait changer d'avis

* Si une seconde carte apparaît (Teensy 4.1 / O.R.N.8, évoquée dans les notes locales), le câblage devrait se
  dédoubler en deux crates firmware ou en un crate à features, et `oc-drivers` devrait gagner les pilotes manquants
  (ADC externe, codec) sans changer de nature.
* Si `oc-drivers` devait dépendre de `imxrt-hal` (DMA, par exemple), la frontière « testé sur l'hôte » tomberait :
  mieux vaudrait alors mettre l'adaptateur DMA dans `oc-firmware` et garder `oc-drivers` pur.

## Révision (2026-08-01, `ec140ed`)

Ajout de `oc-drivers` (sixième crate), absent du plan de fondation. Consigné ici a posteriori.
## Révision (2026-11-16, D-025) — `oc-buildinfo`

Un septième crate, `crates/oc-buildinfo` : bibliothèque hôte, utilisée seulement comme `[build-dependencies]` par
`oc-firmware`, `oc-sim` et `oc-vcv-ffi`, pour calculer l'identifiant de build (hash court + `-dirty`). `oc-core`
n'en dépend pas et reste sans `build.rs`. Raisons et détails dans D-025.

## Révision (2026-11-16, D-028) — `unsafe` dans `oc-firmware`

`oc-firmware` reste `deny(unsafe_code)` mais cesse d'être « sans `unsafe` » : l'écriture de la flash par l'API ROM
du RT1062 (aucun crate ne la fournit) demande des appels par pointeurs de fonction. Ils sont confinés à
`oc-firmware/src/flash_store.rs`, derrière un `#[allow(unsafe_code)]` local, chaque bloc avec sa section `# Safety`.
`oc-core` et `oc-drivers` restent `forbid(unsafe_code)`.
