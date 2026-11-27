# Étape 14 — applet `MELODY` : mélodies aléatoires contrôlables (avancement, en cours)

Décision : D-047, adoptée le 2026-11-26 (choix tranchés avec l'utilisateur). Plan :
[etape-14-melodie.md](../etape-14-melodie.md).

## Fait

| Lot | Contenu |
|---|---|
| 14a | D-047, plan de l'étape, `PLAN.md` |
| 14b | `oc_core::tempo` (sorti de `ratchet`), `gfx::patterns::playhead` (sorti de `ratchet`) ; `apps/melody/scale.rs` : treize gammes en masques de 12 bits, quantification au plus proche (égalité vers le bas), degrés, accords empilés (`TRIAD`, `7TH`, `SUS2`, `SUS4`, `OCT`), renversement |
| 14c | `apps/melody/sequence.rs` : 32 pas en demi-tons, horloge, reset armé, longueur, remplacement à `PROB`, tirage `SPREAD` / `BIAS`, `REPEAT`, marche `JUMP`, `REROLL` ; xorshift à graine fixe ; proptests (gamme et étendue tenues, marche sans répétition) |
| 14d | `MelodyApp` : seize réglages sur six pages (quinze sur cinq depuis 14g), six assignables (`Modulations<6>`, `CV1` en `ADD` sur `TRANSPOSE` jusqu'à 14i), édition des pas au clic gauche, sorties tenues, D en gate ; trait `App` (15 paramètres, `STEP` / `NOTE`, `REROLL` / `RESET`), ports `NOTE`, `CH1`, `CH2`, `CH3/G` (`GATE` depuis 14g), entrées `CLOCK`, `RESET` ; `AppId::Melody` (index 7), catégorie `SEQUENCE` ; `tests/melody.rs` (11 tests) |
| 14e | rendu (titre, réglages, séquence et pointeur, cadre d'édition, sous-menu) ; scénarios `melody`, `melody_edit` ; `app_menu_open.screen` régénéré (ligne `SEQUENCE` seule) |
| 14f | README, TESTING 9.3i, CHANGELOG, code-map ; mesures ci-dessous. **Reste** : essai sur le module (9.3i) |
| 14g | révision de D-047 : `OUT D` retiré (le routage choisit les prises), ports `NOTE`, `CH1`, `CH2`, `GATE`, accord de deux tons, `INVERT` 0–1 ; un reset tient la note jouée ; test du routage de la gate ; guide `crates/oc-core/src/apps/melody/README.md`, résumé dans le README racine |
| 14h | quatre lignes de réglages par page au lieu de trois (quatre pages : la boucle et sa tonalité, la hauteur et l'accord, le tirage, le reste), séquence sous la quatrième ; écrans `melody` et `melody_edit` régénérés |
| 14i | aucune entrée assignée au démarrage, `TRANSPOSE` compris : un réglage modulable comme les autres (demande de l'utilisateur) ; test : `CV1` ne bouge rien avant l'assignation |
| 14j | plus de ligne réservée au pointeur : triangle en inverse vidéo en haut de la colonne jouée, rogné à sa largeur ; 4 px de plus pour les hauteurs des notes ; écrans régénérés |
| 14k | un petit rond à droite des réglages modulables sans entrée (`modulation::draw_input`, partagé par `RATCHET`, `SUPERSAW`, `TONE`, `MELODY`) ; écrans régénérés |
| 14l | douze lignes sur trois pages : `REPEAT` fondu dans `JUMP` (`OFF`, `NO REP`, 1–7), `KEY` (tonique, gamme) et `CHORD` (accord, inversion) à deux champs, encodeur gauche sur les champs, champ choisi seul encadré ; inversion −1 / 0 / +1 en flèche ; `SPREAD` et `BIAS` masqués pendant une marche ; scénario `melody_chord` ; révision de D-047 |
| 14m | mesures de l'étape refaites et corrigées ; construction de `MELODY` en place essayée (`MelodyApp::EMPTY` puis `start` par référence : `Instance::new` 968 → 696 o ; avec des bras `const { … }` pour toutes les apps, 352 o), puis écartée au profit de la forme simple `Self::App(App::new())`, le coût restant loin du seuil ; commentaire dans `Instance::new` |
| 14n | révision de D-047 (`dd16109`) puis code : chaque pas garde une place 16 bits dans une fenêtre (`melody/window.rs` : `pitch` par la courbe de `BIAS` en `log2` / `exp2` entiers, `place_of` par dichotomie, exacte car la projection est monotone) ; `SPREAD` 0–48 dt, `CENTER` ±100 %, `BIAS` ±100 % en direct, entrées comprises ; `RANGE` et la concentration au tirage retirés ; marche `JUMP` en degrés par la projection inversée, plus courte si la fenêtre est étroite, plus loin si elle saute la note visée ; édition sur la note jouée ; marques mises à l'échelle de la fenêtre ; proptests (projection monotone, place retrouvée, notes dans la fenêtre, marche sans répétition) |
| 14o | révision de D-047 (`5a7041b`) puis code : rythme aléatoire qui boucle, tiré à part ; par pas un rang de jeu et un rang de liaison sur 8 bits (`Beat`), comparés en direct à `DENSITY` (assignable) et `LEGATO` ; `R PROB` et `R REROLL` ; pas silencieux : note tenue, gate fermée ; pas lié : gate tenue jusqu'à lui, un tick bas avant une gate neuve après une gate tenue ; éditeur : clic droit court = gate, appui long droit = nouvelle note ; marques en pointillés (silence) et reliées (liaison) ; quatrième page. Coût : `.text` +1,5 Kio (57,1 Kio libres), chaque emplacement +96 o (`ENGINE` 4 624 o), pile estimée 12 408 o (3 976 o libres, +1 280 o : `engine_in_static` 4 432 o, `select_at` 1 368 o, `Instance::new` 1 240 o) |
| 14p | révision de D-038 : applet choisie une fois le moteur en place (`Engine::without_apps`, `AppHost::EMPTY`, puis `START APP` par les réglages lus en flash ou par défaut) ; pile estimée 12 408 → 10 056 o (6 328 o libres), le plus profond devenant la boucle (`run`, `Usb::serve`, `select_at`, `Instance::new` : 9 088 o) ; `Engine::new` non inliné essayé (18 024 o, écarté) ; `.text` −56 o ; commentaire de `Instance::new` mis à jour |

## Mesures

`cargo xtask size` et `llvm-nm`, contre `master` (`937c61f`) compilé dans un worktree, à la fin de l'étape
(14n, la fenêtre ; 14m entre parenthèses quand il diffère) ; tailles des types lues sur la cible (`thumbv7em-none-eabihf`, `size_of` en erreur de compilation) :

| | `master` | étape 14 | écart |
|---|---|---|---|
| `.text` (ITCM) | 0x1ee34 (68,4 Kio libres) | 0x215b4 (58,6 Kio libres ; 14m 0x21038) | +10 112 o (≈ 9,9 Kio) |
| `.rodata` | 0x8f08 | 0x9ab4 (14m 0x9974) | +2 988 o |
| `.bss` | 0x6208 | 0x6208 | 0 en apparence (voir ci-dessous) |
| `ENGINE` (statique) | 3 920 o | 4 240 o (14m 4 112) | +320 o |
| pile estimée (main + 3 interruptions) | 9 424 o | 11 128 o (14m 10 432) | +1 704 o, 5 256 o libres |
| interruption la plus profonde | 728 o | 728 o | 0 |

* **`MELODY` est la plus grande applet** : 300 o en 14m (`Modulations<6>` 172 o, `Sequence` 40 o, le tempo, les
  réglages, les sorties), 332 o depuis 14n (un pas en 16 bits : `Sequence` 72 o), contre 256 o pour `RatchetApp`, la
  plus grande jusque-là. Un emplacement (`Instance`) passe de 256 à 336 o, `AppHost` (quatre emplacements) de
  1 096 à 1 416 o.
* **`.bss` ne bouge pas par hasard** : l'`Engine` statique grandit de 192 o, mais finit à 0x2000_8010 et la
  variable suivante, `usb::ENDPOINT_STATE`, est alignée à 0x2000_9000 ; les 192 o tombent dans le bourrage. Ce
  n'est pas, comme écrit d'abord, qu'un emplacement garderait la taille d'une applet plus grande.
* **Pile** : le chemin le plus profond est, comme sur `master`, celui du démarrage
  (`main → boot → engine_in_static → AppHost::select_at → Instance::new`) ; ses cadres grandissent avec les
  emplacements :

  | Cadre | `master` | 14m | 14n |
  |---|---|---|---|
  | `engine_in_static` | 3 024 o | 3 408 o | 3 664 o |
  | `AppHost::select_at` | 840 o | 984 o | 1 080 o |
  | `Instance::new` | 536 o | 968 o | 1 032 o |
  | main | 8 456 o | 9 464 o | 10 160 o |

  `engine_in_static` (+384 o, deux `AppHost`) et `select_at` (+144 o, trois `Instance`) suivent la taille des
  emplacements. `Instance::new` garde à la fois l'applet construite et l'`Instance` qui l'emballe : un `const fn`
  appelé à l'exécution est un appel ordinaire, que Rust ne promet pas de construire en place.
* **Construction en place, essayée et écartée** (14m), mesures de `Instance::new` / main :

  | Variante | `Instance::new` | main |
  |---|---|---|
  | forme simple, `Self::App(App::new())` (retenue) | 968 o | 9 464 o |
  | `MELODY` : constante `EMPTY` écrite dans l'instance, puis `start` par référence | 696 o | 9 192 o |
  | + bras `const { … }` pour les autres apps | 368 o | 8 864 o |
  | + `DIAGNOSTIC` rendue `const` (`.text` −512 o, `.rodata` +1 968 o) | 352 o | 8 848 o |

  Gain réel (jusqu'à −616 o) mais coût loin du seuil (alerte sous 2 Kio libres, 5,9 Kio libres) : l'utilisateur
  garde la forme simple, partout la même ; un commentaire dans `Instance::new` le dit et renvoie ici.

Définition de fini passée localement : `cargo fmt --check`, les deux `clippy`, `cargo test --all-features`,
`cargo xtask build && cargo xtask size`.

## Réserves

Notes des pas ni sauvegardées ni exposées à la sonde ; pas de silences ni de densité de gates ; pas de verrou par
pas ; CV de hauteur arrondie au millivolt (D-041) ; non essayée sur le module ni dans Rack.
