# Étape 11 — générateurs audio : la voix ne nomme plus le Supersaw (avancement, terminée le 2026-11-21)

Décision : D-044, proposée puis adoptée sur mesure ; révise D-033 (forme de `Params` et de `Voice`) et D-037 (où vit
le banc). Étape 2 de la trajectoire de D-043. Valeurs mesurées sur un module : indicatives.

## Fait

| Lot | Commits | Contenu |
|---|---|---|
| plan | `7ac8496` | D-044 proposée : trait `Generator`, enum composé à l'aiguillage statique, banc dans l'app `SUPERSAW` |
| relevé avant | `26fa243` | `tests/voice_samples.rs` : ce que la voix joue pour chaque applet, figé avant le lot |
| 11a | `bbc05ae` | trait `Generator`, `Params` / `Setup` / `Voice` génériques, générateur `Basic` (niveaux et tonalités) ; banc déplacé dans `apps/supersaw/bank/` (`SupersawBank`) ; composition `apps/sound.rs` écrite à la main ; maintien par la sonde en surcouche (`held`, `any_held`) ; `#[inline]` sur le chemin par échantillon du banc |
| 11b | ce commit | `cargo probe stats` sur le module ; D-044 adoptée, D-033 et D-037 révisées ; cet avancement |

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
  pire cas, le bruit changeant le `Setup` à chaque tick ; trame, période et gigue inchangées.

## Réserves

* La composition (`apps/sound.rs`) est écrite à la main ; une macro si la liste des générateurs s'allonge.
* Un générateur à la fois : plusieurs emplacements et un routage vers les sorties attendent plusieurs apps sonores
  simultanées (étapes 3 et 5 de D-043).
* Pas de bibliothèque DSP partagée : elle vient au premier bloc partagé par deux apps.
