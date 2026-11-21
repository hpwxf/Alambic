# Étape 11 — générateurs audio : la voix ne nomme plus le Supersaw (avancement, terminée le 2026-11-21)

Décision : D-044, proposée puis adoptée sur mesure ; révise D-033 (forme de `Params` et de `Voice`) et D-037 (où vit
le banc). Étape 2 de la trajectoire de D-043. Valeurs mesurées sur un module : indicatives.

## Fait

| Lot | Commits | Contenu |
|---|---|---|
| plan | `7ac8496` | D-044 proposée : trait `Generator`, enum composé à l'aiguillage statique, banc dans l'app `SUPERSAW` |
| relevé avant | `26fa243` | `tests/voice_samples.rs` : ce que la voix joue pour chaque applet, figé avant le lot |
| 11a | `bbc05ae` | trait `Generator`, `Params` / `Setup` / `Voice` génériques, générateur `Basic` (niveaux et tonalités) ; banc déplacé dans `apps/supersaw/bank/` (`SupersawBank`) ; composition `apps/sound.rs` écrite à la main ; maintien par la sonde en surcouche (`held`, `any_held`) ; `#[inline]` sur le chemin par échantillon du banc |
| 11b | `13d85a6`, `c1ae6d5` | `cargo probe stats` sur le module, avec et sans modulation ; D-044 adoptée, D-033 et D-037 révisées ; cet avancement |
| 11c | `9eda318`, `2895915` | étape `publish` au profil (protocole 7) : `audio::publish` échappait à la mesure ; le `Setup` du Supersaw ne prépare plus que le banc choisi (révision de D-039) ; bancs criterion `voice/supersaw_setup`, `supersaw_f32_setup` |
| 11d | `666ea74`, `f162933`, `a487802`, ce commit | `scripts/profile-supersaw.sh` ; `Engine::params_revision` et `audio::Publisher` : un tick inchangé ne compare plus les `Params` sous section critique (révision de D-040) ; `fixed::div_wide`, 64 bits divisés par 32 avec le diviseur 32 bits du cœur, pour les inverses, `phase_step`, les normaliseurs et `lead` |

## Mesures

* **Échantillons** : identiques au bit près à `26fa243` (`TONE`, `SUPERSAW` en `INT` et en `F32`, la rampe du
  diagnostic, les triggers de `RATCHET`) ; écrans de référence intacts.
* **Hôte** (criterion contre `master`) : `voice/tone` 19,65 → 20,18 ns (dans la dispersion de `master`),
  `voice/supersaw` 36,5 → 30,8 ns, `square` 47,4 → 37,3 ns, `f32` 33,8 → 28,9 ns. Sans `#[inline]`, jusqu'à 26 % de
  perte en LTO *thin* (le profil des bancs) ; le firmware, en LTO *fat*, n'en dépend pas.
* **Code** (différence de `cargo xtask size`) : `.text` +1 380 o (164 Kio sur 192 d'ITCM), `.rodata` +400 o,
  `.data` −16 o (`MAILBOX`, qui porte le `Setup`).
* **Module** (`SUPERSAW` `F32`, `NOTE` et `DETUNE` sous bruit, `MIX` 50 %, `SHAPE` 25 %, `STEREO ON` ; `b5c444a`
  contre `bbc05ae`) :

  | Étape | Avant | Après |
  |---|---|---|
  | `voice` moyenne / max | 1,68 / 1,75 µs | 1,56 / 1,65 µs |
  | `audio` moyenne / max | 4,22 / 5,66 µs | 4,21 / 5,70 µs |
  | `voice_params` moyenne / max | 0,47 / 0,47 µs | 0,50 / 0,52 µs |
  | `update` + `outputs` moyenne | 14,87 + 1,47 µs | 13,65 + 2,27 µs |
  | `tick` moyenne / max | 26,65 / 83,17 µs | 25,71 / 82,13 µs |

  `voice` −7 % ; `voice_params` +0,03 µs, dans le bruit (le `Setup` du Supersaw dominait déjà la somme) et dans le
  pire cas, le bruit changeant le `Setup` à chaque tick ; trame, période et gigue inchangées. Le partage entre
  `update` et `outputs` suit l'endroit où tombe l'interruption audio (les étapes de la boucle la comptent) ; seule
  leur somme compte.
* **Module, sans modulation** (mêmes réglages, `NOTE` et `DETUNE` fixes, `bbc05ae`) : `voice` 1,56 / 1,65 µs,
  `audio` 4,18 / 5,02 µs, aucun `voice_params`, `update` 4,71 µs, `outputs` 0,91 µs (jamais interrompu), `tick`
  15,25 µs. La modulation coûte ≈ 10,5 µs par tick à la boucle, +0,03 µs à `audio` ; un second `Setup::new`, dans
  `audio::publish`, échappe au profil.

* **Un seul banc préparé** (11c) : `Setup` 424 → 320 o ; hôte, `Setup::new` 66,8 → 57,3 ns (`INT`), 66,3 → 63,7 ns
  (`F32`) ; firmware `.text` +228 o, `.rodata` −96 o, `.data` −104 o ; échantillons identiques au bit près.
* **Module après 11c** (`2895915`, `MIX` 50 %, `STEREO ON`, 10 s par cas, script par `cargo probe set` / `stats`),
  moyennes en µs :

  | Cas | `voice` | `voice_params` | `update` | `publish` | `tick` |
  |---|---|---|---|---|---|
  | `INT`, `SHAPE` 25 %, sans CV | 1,34 | — | 5,26 | 1,58 | 15,92 |
  | `F32`, `SHAPE` 25 %, sans CV | 1,57 | — | 4,70 | 1,55 | 15,27 |
  | `INT`, `SHAPE` 25 %, `NOTE` et `DETUNE` sous bruit | 1,37 | 0,39 | 12,33 | 10,78 | 25,00 |
  | `F32`, `SHAPE` 25 %, sous bruit | 1,57 | 0,35 | 9,45 | 6,27 | 21,71 |
  | `INT`, `SHAPE` 0, sous bruit | 1,25 | 0,39 | 12,01 | 10,85 | 24,52 |
  | `F32`, `SHAPE` 0, sous bruit | 1,04 | 0,35 | 9,23 | 7,56 | 20,01 |

  * En `F32` sous bruit, contre `bbc05ae` : `update` 13,65 → 9,45 µs, `tick` 25,71 → 21,71 µs, `voice_params`
    0,50 → 0,35 µs (`MAILBOX` plus petite) ; `voice` inchangé.
  * `Setup::new` sur le module, par différence avec le cas sans CV (`update` et `publish` en font chacun un) :
    ≈ 4,7 µs en `F32`, ≈ 7 à 9 µs en `INT` (les divisions 64 bits des inverses, émulées).
  * `publish` coûte 1,55 µs à chaque tick même sans changement : deux sections critiques et la comparaison de
    `Params` (80 o) et de la calibration (88 o).

* **Module après 11d** (`NOTE` sous le bruit de CV1, assignée au démarrage ; `DETUNE` fixe ; `SHAPE` 25 %, `STEREO
  ON` ; script), moyennes en µs :

  | Cas | `update` | `publish` | `tick` |
  |---|---|---|---|
  | `INT`, `f162933` (révision seule) | 12,14 | 10,86 | 26,77 |
  | `INT`, `a487802` (avec `div_wide`) | 10,83 | 8,86 | 25,33 |
  | `F32`, `f162933` | 9,19 | 7,47 | 22,03 |
  | `F32`, `a487802` | 9,17 | 7,36 | 22,04 |
  | `TONE`, `2895915` / `a487802` | 5,59 / 5,39 | 6,28 / 4,45 | 18,61 / 18,46 |

  * Avec `NOTE` et `DETUNE` sous bruit (page 1), `a487802` contre `2895915` : `INT` `update` 12,33 → 11,12 µs,
    `publish` 10,78 → 9,08 µs ; `F32` `update` 9,45 → 9,44 µs, `publish` 6,27 → 7,08 µs, dans la dispersion de
    `publish` en `F32` d'un relevé à l'autre (6,3 à 7,5 µs à firmware égal) ; `render` 100,7 µs.
  * `div_wide` : ≈ 2 µs de moins par `Setup` en `INT` (les huit inverses), ≈ 1,8 µs pour `TONE` (`phase_step`),
    rien en `F32` ; `.text` +1 572 o ; sur l'hôte, qui divise 64 bits en matériel, `Setup::new` 57 → 112 ns.
  * La révision : un tick inchangé coûte 1,02 µs à `publish` (minimum, `GFX DEMO` et `SUPERSAW`), contre 1,46 à
    1,56 µs avant, soit ≈ 0,5 µs de moins par tick et non 1,5 ; ≈ 0,9 µs est le plancher de plusieurs étapes
    (`outputs`, `present`), peut-être le coût de la mesure elle-même, à vérifier.
  * `render` à 95 µs dans ces relevés, faits sur la page 1 de `SUPERSAW` (l'éventail des voix, animé), contre 54 µs
    sur la page 2 dans ceux de l'utilisateur ; identique en `2895915`, `f162933` et `a487802`. `tick` max ≈ 120 µs
    en découle.

## Réserves


* La voix du moteur (1 kHz) et `audio::publish` préparent toujours chacune un `Setup` quand les paramètres changent.

* La composition (`apps/sound.rs`) est écrite à la main ; une macro si la liste des générateurs s'allonge.
* Un générateur à la fois : plusieurs emplacements et un routage vers les sorties attendent plusieurs apps sonores
  simultanées (étapes 3 et 5 de D-043).
* Pas de bibliothèque DSP partagée : elle vient au premier bloc partagé par deux apps.
