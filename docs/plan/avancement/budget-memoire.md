# Budget mémoire : l'ITCM, le niveau d'optimisation, l'écran noir en `z` (avancement, 2026-11-22)

Travail hors étape, sur la branche `memory-budget-flexram`. Décision : D-038, corrigée (§1) puis révisée trois fois le
même jour. Valeurs mesurées sur un module : indicatives.

## Fait

| Commit | Contenu |
|---|---|
| `75d283a` | D-038 §1 corrigé : Phazerville exécute ≈ 284 Kio de code en ITCM (9 bancs), pas depuis la flash |
| `4f6f048` | `cargo xtask size` lit le partage de la FlexRAM dans l'ELF (`__flexram_config`), avertit sous 16 Kio d'ITCM libres, échoue au-delà des bancs ; contrôle `dtcm-budget` |
| `73ce9a7` | le runtime configuré par le `build.rs` d'`oc-firmware`, sans la feature `rt` de `teensy4-bsp` ; partage en constante (`FLEXRAM_BANKS`) |
| `3028e65` (PR #14) | `scripts/profile-supersaw.sh` : filtre adapté aux lignes indentées de `cargo probe stats` (depuis `d56d1fe`), fusionné avec ce changement-là |
| `70e498d` | écran noir en `opt-level = "z"` : l'initialisation de l'OLED attend la fin de trame (`FramedSpi`) |
| `d7effd0` | le code froid (USB, sonde, réglages) compilé en `opt-level = "s"` |
| ce commit | cet avancement |

## Dépasser l'ITCM

Essai : une `static` placée dans `.text.itcm_fill` remplit l'ITCM. Remplie exactement (`.text` = 0x30000), l'image se
lie, `xtask size` passe, `flash --dry-run` la valide. Quatre octets de plus et l'éditeur de liens refuse :
`section '.text' will not fit in region 'ITCM': overflowed by 4 bytes`. Le dépassement n'est donc jamais une image
qui démarre mal ; ce qui manquait, c'était le préavis, que `xtask size` donne désormais (`warn` sous 16 Kio).

Rythme constaté : `.text` 138,5 Kio le 2026-11-18, 160 Kio le 2026-11-22.

## Le partage des bancs repris en main

`teensy4-bsp` sans `rt` : le `t4link.x` généré par notre `build.rs` est identique, octet pour octet, à celui du BSP ;
`.text` aussi ; `__flexram_config` = `0xfffaaaaa` (6 ITCM, 10 DTCM) dans les deux cas. Le module démarre sur cette
image. Repartager les bancs (option B de D-038) est désormais un changement de `FLEXRAM_BANKS`. Une surcharge par
variable d'environnement dans `imxrt-rt` et `teensy4-bsp` est préparée pour une issue en amont, non ouverte.

## Le niveau d'optimisation

### Taille

| Variante | `.text` | ITCM libre |
|---|---|---|
| `opt-level = 3` (référence) | 164 Kio | 32 Kio |
| tout en `s` | 111 Kio | 81 Kio |
| tout en `z` | 100 Kio | 91 Kio |
| tout en O3 sans `overflow-checks` | 180 Kio | — |
| code froid en `s` (`d7effd0`) | 154,5 Kio | 37,5 Kio |
| après #16 et #17 (pile, assignation des CV), O3 | 164,7 Kio | 27,3 Kio |
| après #16 et #17, code froid en `s` | 156,0 Kio | 36,0 Kio |
| code froid en `z` | 153,5 Kio | — |
| code froid, `oc-core` et `oc-firmware` en `s` | 112 Kio | — |

Sans `overflow-checks`, `.text` *grossit* : LLVM inline davantage. Le formatage (`core::fmt`) pèse ≈ 14 Kio ; la
monomorphisation se voit peu (un seul backend, une instance de chaque générique).

### Temps, tout le firmware

`SUPERSAW`, `MIX` 50 %, `STEREO ON`, sans CV, 10 s par cas, moyennes en µs (période audio : 21 µs) :

| Étape | O3 | `s` | `z` |
|---|---|---|---|
| `voice`, `SHAPE` 25 % | 0,98 | 2,05 | 2,72 |
| `voice`, `SHAPE` 0 | 0,69 | 1,64 | 2,28 |
| `audio`, `SHAPE` 25 % (max) | 3,86 (4,89) | 5,24 (7,04) | 6,16 (8,29) |
| `voice_params` | 0,33 | 0,47 | 0,63 |
| `update` | 2,00 | 4,55 | 4,60 |
| `publish` | 4,72 | 8,80 | 13,02 |
| `tick` (max) | 14,9 (107) | 18,2 (129) | 25,8 (226) |
| `render` | 93,9 | 115,2 | 204,9 |

`s` double le calcul d'un échantillon, `z` le multiplie par 2,8 à 3,3 : moins d'inlining, et en `z` sans vectorisation
des boucles. La boucle, elle, garde une marge immense (≈ 15 à 26 µs de `tick` sur 1 000).

### Ce qui peut passer en `s` sans coût

* **Le code froid** : ce que seule la boucle à 1 kHz appelle — pile USB (`usb-device`, `usbd-serial`, `imxrt-usbd`),
  protocole de la sonde (`postcard`, `cobs`, `serde`, `oc-protocol`, `heapless`), réglages (`sequential-storage`,
  `embassy-futures`, `futures-util`), `log`, `teensy4-panic`. Fait en `d7effd0` : −5,4 Kio avant l'assignation des CV par la sonde (protocole 8), −8,7 Kio après. Surtout,
  le protocole de la sonde va grossir (étapes à venir) : en `s`, sa croissance coûtera nettement moins d'ITCM qu'en O3
  (≈ 30 % de moins sur l'ensemble du firmware), pour un code dont le temps ne compte pas (`usb` ≈ 4 µs par passage, inchangé).
* **`s` plutôt que `z`** : un Kio de plus seulement, et l'outliner de `z` est le chemin le moins éprouvé (c'est lui
  qui a révélé l'écran noir, ci-dessous).
* **Avec LTO fat, une fonction garde le niveau du crate qui la définit**, générique comprise ; le code d'un autre
  crate inliné dans une fonction prend le niveau de celle-ci. Établi par la bissection ci-dessous.
* **Le gros de l'écart est dans `oc-core` et `oc-firmware`** (≈ 45 Kio), où le DSP (≈ 11 Kio de symboles
  d'`oc-core` sur le chemin audio) côtoie l'interface (≈ 68 Kio : `render` des applets, moteur, menus,
  modulation). Les passer en `s` demanderait de séparer le DSP dans un crate gardé en O3, et l'interruption
  d'`oc-firmware` du reste du firmware ; `#[optimize(size)]`, qui ferait le tri par fonction, n'est pas stable
  (rust#54882). Écarté tant que l'option B suffit.

### Code froid en `s`, sur le module

Trois relevés par image, chacun après un démarrage à neuf, moyennes en µs :

| | O3 | code froid en `s` |
|---|---|---|
| `usb` | 4,14 / 3,81 / 3,81 | 4,08 / 4,09 / 4,09 |
| `voice` | 0,98 | 0,98 |
| `audio` (min – max) | 3,86 (3,15 – 4,89) | 4,10 (3,16 – 4,89) |
| `tick` | 14,9 – 15,2 | 14,9 – 15,0 |

`audio` gagne 0,24 µs en moyenne alors que le code de l'interruption est identique, octet pour octet (`DMA0_DMA16` et
`LPSPI4` : même adresse, même taille ; `Sound::apply`, `Sound::setup`, `__aeabi_memcpy4` : mêmes tailles). Ces
fonctions ont glissé avec les 5 Kio gagnés devant elles ; `__aeabi_memcpy4`, appelée deux fois par interruption,
change d'alignement sur 8 octets (lecture des instructions par 64 bits sur le Cortex-M7). Explication probable, non
prouvée : un effet de placement que tout changement de taille du code peut produire, dans un sens ou dans l'autre.
Minimum et maximum sont inchangés ; 1 % de la période.

### Code froid en `s` sous bruit, sur le module

Le pire cas du protocole de l'étape 11, après #16 et #17 : `scripts/profile-supersaw.sh 10`, CV1 sur `NOTE` et CV2 sur
`DETUNE` (assignés par la sonde, effet `ADD`), bruit branché sur les deux, `MIX` 50 %, `STEREO ON`. Trois relevés de
10 s par cas et par image, la même séance : `master` (`a782775`, O3 partout) puis cette branche (`f6d0202`). Les
trois relevés d'un cas diffèrent de 0,01 µs au plus, sauf `tick` (0,16 µs). Moyennes en µs, (min – max) :

| | O3, `SHAPE` 25 % | code froid en `s`, `SHAPE` 25 % | O3, `SHAPE` 0 | code froid en `s`, `SHAPE` 0 |
|---|---|---|---|---|
| `audio` | 3,89 (3,26 – 5,04) | 4,07 (3,15 – 4,90) | 3,77 (2,98 – 4,88) | 4,03 (2,87 – 4,89) |
| `voice` | 0,98 (– 1,09) | 0,98 (– 1,07) | 0,69 (– 0,79) | 0,69 (– 0,77) |
| `voice_params` | 0,36 | 0,34 | 0,36 | 0,34 |
| `update` | 2,19 | 2,17 | 2,19 | 2,17 |
| `publish` | 5,04 | 5,04 | 5,02 | 5,02 |
| `tick` (max) | 15,34 – 15,50 (111) | 15,27 – 15,28 (111) | 15,20 – 15,21 (110) | 15,13 – 15,14 (110) |
| `render` | 99,98 | 100,0 | 99,68 | 99,80 |

Sonde en flux pendant 10 s (`cargo probe stats --fresh 10`) : `usb` 4,26 µs (1,71 – 6,67) en O3,
4,42 µs (1,79 – 6,65) avec le code froid en `s` ; gigue de la boucle contre 1 000 µs sous 16 µs dans les deux cas
(8 à 16 µs : 14,3 % des ticks en O3, 11,8 % en `s`).

Le bruit ne change rien à la conclusion sans CV : seul `audio` bouge en moyenne (+0,18 µs à `SHAPE` 25 %, +0,26 µs
à 0), avec un `voice` identique et un maximum un peu plus bas (4,90 contre 5,04 µs, 24 % de la période de 21 µs).
Même signature que l'effet de placement ci-dessus : le code de l'interruption n'est pas dans les crates passées en `s`.
`usb`, seul chemin compilé en `s` qui tourne ici, prend 0,16 µs de plus par passage, sur 1 000.

### Code froid en `s` et pile

Sur `master` après #16 (profondeur de la pile dans `xtask size`, moteur en `static`, `main` coupé en `boot` / `run`)
et #17, estimation de `cargo xtask size` (appels directs seulement) :

| | O3 | code froid en `s` |
|---|---|---|
| chemin le plus profond (démarrage : `boot`, `engine_in_static`), interruptions comprises | 11 824 o | 11 824 o |
| boucle (`run`, `Usb::serve`, `send_screen`, `send`, sérialiseur), sans les interruptions | 5 928 o | 5 968 o |

Le code froid en `s` ne touche pas le chemin du démarrage ; sur celui de la boucle, les cadres du sérialiseur se
répartissent autrement, à 40 o près.

## L'écran noir en `opt-level = "z"`

Symptôme : écran noir dès le démarrage en `z` (pas en `s` ni en O3), alors que le journal dit `oled init ok`, que
le framebuffer est juste (`cargo probe screen`) et que tous les morceaux d'écran sont envoyés et relâchés.

* **Hypothèse 1, écartée** : l'ordre des écritures GPIO (CS, DC) et LPSPI dans le pilote bloquant. Une broche qui
  attend sa propre écriture (`dsb`, relecture) n'a rien changé.
* **Hypothèse 2, écartée** : le budget de temps d'un morceau d'écran dans l'interruption. Marges élargies
  (`OLED_RELEASE_MICROS` 8 µs, morceaux deux fois plus petits) : toujours noir.
* **Bissection** : tout en `z` sauf `oc-drivers` en O3 → écran allumé. Comparaison des deux images fonction par
  fonction (`llvm-nm -S`) : seule `Ssd130x::transfer` diffère (604 contre 688 octets), plus des
  `OUTLINED_FUNCTION_*` de l'outliner ; le morceau factorisé dans `transfer` est anodin (remise du compteur
  d'emprunt d'un `RefCell`). La logique est la même, le rythme non.
* **Cause** : `Lpspi::flush` d'`imxrt-hal` (0.6.0, inchangé en 0.6.1) rend la main quand BUSY est bas et la FIFO
  d'émission vide ; le HAL note lui-même que BUSY peut ne pas être encore monté. Le pilote remonte alors CS en plein
  octet ; `DISPLAY_ON`, dernier envoi de l'initialisation, un octet seul, est perdu. La boucle d'attente compilée en
  `z` tombe dans la fenêtre, celle d'O3 non : le défaut était latent à tous les niveaux.
* **Correction** (`70e498d`) : `FramedSpi` enveloppe LPSPI4 pendant l'initialisation ; son `flush` attend aussi
  FRAME_COMPLETE, comme l'interruption pour ses morceaux. Vérifiée en `z` et en O3. Une issue pour `imxrt-hal` est
  préparée, non ouverte.

## Réserves

* Tout le firmware en `s` ou en `z` mesuré sans CV seulement ; ces images ne sont pas retenues. Le code froid en `s`,
  retenu, est mesuré aussi sous bruit.
* L'effet de placement sur `audio` est une explication probable, non prouvée.
* La sonde échoue parfois au premier appel juste après un démarrage (une reprise suffit) ; non analysé.
