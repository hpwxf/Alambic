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

## Mesures

`cargo xtask size`, contre `master` (`937c61f`) compilé dans un worktree :

| | `master` | étape 14 | écart |
|---|---|---|---|
| `.text` (ITCM) | 0x1ee34 (68,4 Kio libres) | 0x20dd8 (60,5 Kio libres) | +8 100 o (≈ 7,9 Kio) |
| `.rodata` | 0x8f08 | 0x9904 | +2 556 o |
| `.bss` | 0x6208 | 0x6208 | 0 (un emplacement reste de la taille de la plus grande applet) |
| pile estimée | 9 424 o | 10 344 o | +920 o (`Instance::new` de `MelodyApp`), 6 040 o libres |

Définition de fini passée localement : `cargo fmt --check`, les deux `clippy`, `cargo test --all-features`,
`cargo xtask build && cargo xtask size`.

## Réserves

Notes des pas ni sauvegardées ni exposées à la sonde ; pas de silences ni de densité de gates ; pas de verrou par
pas ; CV de hauteur arrondie au millivolt (D-041) ; non essayée sur le module ni dans Rack.
