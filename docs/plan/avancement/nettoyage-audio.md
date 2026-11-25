# Hors étape — les paramètres de l'audio sans copies, la pile du démarrage (2026-11-25)

Après l'étape 13, à la demande de l'utilisateur : des copies des `Params` en trop (`Setup`, `Voice`, la couche
relais du supersaw, le `Follower`), `Player` visible des applets, `audio.rs` du firmware mal nommé, la pile.
Révisions de D-033, D-038, D-039, D-040 et D-044. Branche `audio-params-cleanup`.

## Fait

| Commit | Contenu |
|---|---|
| `61b26ea` | `oc_core::playback` : les types concrets des backends (`Patch`, `Setup`, `Voice`, `Changes`, `Voices`, `Follower`, `Player`) sortent d'`apps::sound`, qui garde `Sound` et `Params` (D-044) |
| `a9cdd4d` | supersaw : `dsp/` (`Supersaw`, parties, courbes, `Coefficients`, `Oscillator`) et `generator.rs` (`SupersawGenerator`, `{ saw, parts }`, `{ coefficients, parts }`, `{ oscillator, parts }`), la couche `bank::Setup` / `bank::Bank` retirée (D-039) |
| `48f3fab` | `audio::Setup` ne porte que `G::Setup`, `Voice` que sa fréquence et son état ; `Player::set_rate` par `Follower::forget` (D-040) |
| `78604b6` | `Patch` numérote ses changements (révision globale et une par emplacement), le moteur écrit dans le sien, `Follower` ne garde que des numéros ; `Engine::params_revision` retiré (D-040) |
| `5c6ee9f` | le magasin des réglages dans une `static` (`settings_in_static`), hors du cadre de `boot` (D-038) |
| `e4d06d4` | `audio.rs` / `audio_irq.rs` du firmware deviennent `realtime/` : `mod.rs` (état partagé, bus, calage, boîte aux lettres), `dac.rs`, `screen.rs`, `irq.rs` ; la boîte aux lettres en deux moitiés (D-033) |
| ce commit | `scripts/profile-supersaw.sh` : `INSTANCES=N`, `DETUNE_INPUT` ; cet avancement |

Échantillons identiques au bit près à chaque commit (`tests/voice_samples.rs`, exemple `aliasing`), écrans de
référence inchangés.

## Mesures

* **Code et pile** (`cargo xtask size`, contre `master` `145275f`) :

  | | `master` | branche |
  |---|---|---|
  | `.text` | 0x2a908 | 0x29dc4 (−2 884 o ; 22,0 Kio d'ITCM libres) |
  | `.rodata` | 0x85e4 | 0x843c (−424 o) |
  | `.data` | 0x24cc | 0x22c4 (−520 o, `MAILBOX` : un `Params` de moins par `Setup`) |
  | `.bss` | 0x525c | 0x625c (+4 096 o : le magasin des réglages, 1 048 o, et l'alignement des tampons USB sur 4 Kio, en DTCM) |
  | pile, chemin le plus profond | ≈ 12 744 o (démarrage : `boot` 6 400 o) | ≈ 8 968 o (la boucle : `run`, `Usb::serve`, `serve_instances`) ; 7 416 o libres |

  Le cadre de `boot` gardait quatre copies du magasin des réglages (0x200, 0x3a8, deux 0x410) ; `realtime::start`,
  plus petit après le découpage, s'y inlinait avec l'état de 2 Kio qu'il bâtit (+1 904 o) : hors ligne.

* **Module** (`scripts/profile-supersaw.sh` à `DETUNE_INPUT=cv1` : `MIX` 50 %, `SHAPE` 25 %, `STEREO ON`, `NOTE` et
  `DETUNE` de chaque instance sous le bruit de CV1 ; 10 s ; µs, moyenne (max)) ; C2 : deux `SUPERSAW`, la première
  sur A à C, la seconde sur D ; C4 : quatre, chacune sur une sortie :

  | | C2 `master` | C2 branche | C4 `master` | C4 branche |
  |---|---|---|---|---|
  | `publish` | 28,10 (32,3) | **21,07** (22,1) | 41,88 (43,6) | **32,32** (39,0) |
  | `voice_params` | 0,89 | **0,69** | 1,24 | **0,95** |
  | `update` | 7,53 | 7,98 | 11,94 | 11,73 |
  | `tick` | 19,71 | 20,09 | 27,53 | 27,66 |
  | `audio` | 4,31 (5,54) | 4,32 (5,46) | 5,43 (7,30) | 5,45 (7,05) |
  | `voice` | 1,82 | 1,84 | 3,15 | 3,18 |
  | `audio_period` max | 23,55 | 23,55 | 23,56 | 23,55 |

  * `publish` −25 % (C2) et −23 % (C4). De C2 à C4, il se lit comme un coût par `SUPERSAW` repréparée plus un coût
    fixe par tick qui change : ≈ 6,9 + 14,3 µs sur `master`, ≈ 5,6 + 9,9 µs sur la branche.
  * Une seule `SUPERSAW` (branche, `NOTE` sous CV1, `DETUNE` sur CV2) : `publish` 16,6 µs, contre ≈ 5 µs à l'étape 11
    (un emplacement) : le coût fixe vient des quatre emplacements de `Changes`, bâtis, rendus par valeur et fusionnés
    à chaque changement (non instrumenté).
  * `voice_params` −23 % : le `Setup` copié par l'interruption est plus petit. `audio`, `voice`, `audio_period`
    inchangés au bruit près.

## Réserves

* Le coût fixe de `publish` (≈ 10 µs par tick qui change, quel que soit le nombre d'instances) : bâtir les `Setup`
  sur place et ne copier dans la boîte aux lettres que les emplacements changés, après l'avoir instrumenté.
* Une instance routée nulle part est préparée quand même (un essai à deux `SUPERSAW` sans sortie sur quatre :
  `publish` inchangé) : le `Follower` pourrait sauter les emplacements dont la route est `OFF`.
* `.bss` : le trou d'alignement devant `usb::ENDPOINT_STATE` (3 200 o) ne se récupère pas sans maîtriser l'ordre de
  `.bss` ; sans enjeu en DTCM.
