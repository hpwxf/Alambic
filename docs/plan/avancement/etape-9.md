# Étape 9 — le Supersaw en `f32` (avancement, terminée le 2026-11-21)

Décision : D-039, proposée le 2026-11-19, adoptée le 2026-11-21 : le banc `f32` reste, le banc entier part. Révise
D-005 (le DSP en `f32`), D-037 (le banc, `SHAPE`, `WIDTH`, les deux pages) et D-040 (la voix préparée dans la boucle,
ouverte en chemin). Valeurs mesurées sur un module : indicatives.

## Fait

| Lot | Commits | Contenu |
|---|---|---|
| 9a | `287e0f3` | banc `f32` à côté du banc entier, réglage `CALC` (`INT` / `F32`), écart au banc entier < 2 mV RMS |
| 9b | `38f9de3`, `998e98f` | étapes `voice` et `voice_params` dans l'interruption (protocole 3) ; premiers relevés, sans `SHAPE` : `F32` ≈ 8 % moins cher |
| 9c | `ab43613` | `STEREO` et `CALC` en page 2, ascenseur de `RATCHET` partagé |
| 9d | `d8b0970` | la voix préparée dans la boucle (`Setup`), copiée par l'interruption (D-040) |
| 9e | `8abc24d` | une arithmétique par sous-module (`fixed.rs`, `float.rs`) |
| 9f | `562b40b` | `voice_gains`, seule source de la répartition ; référence naïve d'`aliasing` corrigée |
| 9g | `9e19e3c` | `SHAPE`, de la scie au carré |
| 9h | `ef9edac` | `WIDTH`, largeur de l'impulsion |
| — | `2895915` | un seul banc préparé par `Setup` (étape 11, lot 11c) |
| 9i | `14aad64`, `f67d414` | comparaisons sans flottant : phases par leurs bits, `millivolts` par conversion saturante puis borne entière |
| 9j | `6b1b25d` | F2 : phases entières du banc `f32` |
| 9k | ce commit | D-039 adoptée : banc entier, `CALC`, `Arithmetic` et `SUPERSAW_CALC` retirés ; cet avancement |

## Mesures

* **`voice` sur le module** (`MIX` 50 %, `STEREO ON`, `NOTE` sous bruit ; µs, moyenne) :

  | | `INT` | `F32` F1 | `F32` 9i | `F32` 9j | `F32` 9i + 9j + `millivolts` |
  |---|---|---|---|---|---|
  | `SHAPE` 25 % | 1,37 | 1,56 | 1,18 | 1,15 | **1,07** |
  | `SHAPE` 0 | 1,25 | 1,03 | 0,88 | 0,86 | **0,78** |

* **Après le retrait** (`db252e8`, mêmes réglages) : `voice` 1,04 / 0,74 µs (`SHAPE` 25 % / 0), l'aiguillage entre
  les deux bancs en moins ; `update` 8,39 µs, `publish` 7,41 µs, `voice_params` 0,34 µs.
* **`Setup` sous CV** (étapes `update` et `publish`, chacune en prépare un) : ≈ 4,7 µs en `F32`, 7 à 9 µs en `INT`.
* **Précision** (exemple `aliasing`, écart à la supersaw idéale sous 10 kHz) : F1 −28,5 dB à C2, F2 −39,2 dB, comme
  le banc entier, à 0,1 dB près à toutes les notes.
* **Code** (différence de `cargo xtask size`, `7bd42e7` contre ce commit) : retrait du banc entier, `.text`
  −4 976 o (0x28600 → 0x27290, 156,6 Kio d'ITCM sur 192), `.rodata` −364 o, `.data` −384 o (`MAILBOX`, le `Setup`
  plus petit) : ≈ 5,6 Kio d'image en moins.

## Réserves

* L'écoute comparée des deux bancs aux notes basses n'a pas été faite avant le retrait ; le banc `f32` seul a été
  écouté sur le module par l'utilisateur après, sans remarque.
* Le double `Setup` de la boucle (la voix du moteur et `audio::Publisher`) est levé à l'étape 11 (lot 11e,
  `c339f5c`) : le moteur ne rend plus de voix (D-040, révision du 2026-11-22).
