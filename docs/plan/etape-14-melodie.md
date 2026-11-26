# Étape 14 — applet `MELODY` : mélodies aléatoires contrôlables

> Plan de l'étape 14, suivi par git (journal/D-047).

## Contexte

Demande : une app qui génère une mélodie cyclique de notes aléatoires mais contrôlables, dans l'esprit du meloDICER
(Vermona), de Mimetic Digitalis (Noise Engineering) et de Marbles (Mutable Instruments). Premier applet « séquence » :
il lit deux triggers (horloge, reset), publie des CV de hauteur (A la note, B–D un accord diatonique, ou D en gate)
et montre la séquence en bas de l'écran, éditable pas à pas.

Choix tranchés avec l'utilisateur : gamme + tonique + transposition par CV (V/oct) ; accords diatoniques à type
réglable ; D en option gate ; aléa avec `RANGE`, `SPREAD` / `BIAS`, marche (`JUMP`) et pas de répétition, action
`REROLL`.

Tout vit dans `oc-core` (règle d'AGENTS.md) ; ni firmware, ni sim, ni Rack ne nomment l'applet : le registre
(`AppId`, `Instance`) suffit, la sonde, la page et Rack le voient par le trait `App` (D-043, D-045, D-046).

## Comportement

**Séquence.** `MAX_STEPS` = 32 pas ; chaque pas garde une note en demi-tons au-dessus de la tonique (0..=48), non
quantifiée en mémoire : changer `SCALE` re-quantifie à la sortie et reste réversible. Au démarrage, séquence tirée
d'une graine fixe (xorshift comme `RatchetApp::roll`, déterministe pour les tests et les rejeux).

**Horloge et reset.** `TR1` = `CLOCK`, `TR2` = `RESET`, déclarés en `InputPort` (routables, D-046). Front montant de
`CLOCK` : avance d'un pas (modulo `LENGTH`). `RESET` arme le retour : l'horloge suivante joue le pas 1 (convention
Eurorack) ; reset et horloge dans le même tick : pas 1. Avant la première horloge, position armée.

**Remplacement.** À chaque pas atteint, avant la sortie, la note est remplacée avec la probabilité `PROB` (façon
Turing Machine : 0 % boucle figée, 100 % tout aléatoire). La nouvelle note :
* `JUMP OFF` : tirée parmi les notes de la gamme dans `RANGE` octaves ; `SPREAD` 100 % = uniforme, plus bas =
  triangulaire resserrée autour de `BIAS` (position dans l'étendue, 0–100 %) ; `REPEAT OFF` : un tirage égal à la
  note précédente est refait (borne fixe de tentatives, déterministe).
* `JUMP 1..7` : marche aléatoire depuis la note du pas précédent, ±1..`JUMP` degrés, jamais 0, réfléchie aux bords
  de l'étendue (`SPREAD`, `BIAS`, `REPEAT` inactifs, `param_active`).
`REROLL` retire toute la séquence (même règle, pas à pas).

**Hauteur de sortie.** `note = quantize(ROOT + pas + TRANSPOSE)` dans `SCALE`, plus `OCTAVE` ; millivolts =
demi-tons × 1000 / 12 arrondi (erreur ≤ 0,5 mV ≈ 0,6 cent ; D-041 reste ouverte), bornés par la calibration
(−3..+6 V). Niveaux tenus (`Params::levels`), changés seulement à l'horloge : un `Setup` par pas, rien par échantillon.

**Accords (B, C, D).** Tierces empilées dans la gamme depuis le degré de A. `CHORD` : `TRIAD` (+2, +4, +7 degrés),
`7TH` (+2, +4, +6), `SUS2` (+1, +4, +7), `SUS4` (+3, +4, +7), `OCT` (+7, +14, −7). `INVERT` 0..2 : monte d'une
octave les 1 ou 2 tons les plus bas de B–D. Gamme `CHROMATIC` : degrés pris dans la gamme majeure depuis la note.
`OUT D` : `CHORD` (défaut) ou `GATE` : 5 V pendant `GATE` % de la période de `CLOCK` mesurée (`Tempo`), 10 ms tant
qu'elle est inconnue.

**Gammes.** Masque de 12 bits par gamme : `CHROMATIC`, `MAJOR`, `MINOR`, `DORIAN`, `PHRYGIAN`, `LYDIAN`,
`MIXOLYDIAN`, `LOCRIAN`, `HARM MIN`, `PENTA MAJ`, `PENTA MIN`, `BLUES`, `WHOLE`. `ROOT` C..B. Quantification au plus
proche, égalité vers le bas.

## Panneau et écran

* Écran : ligne de titre (`MELODY`, pas courant `03/16`, nom de la note de A via `tone::note_name`), trois lignes
  de réglages par page avec ascenseur (comme Ratchet), puis de y ≈ 34 à 63 la séquence : `LENGTH` colonnes de
  128 / `LENGTH` px, un trait horizontal par pas à une hauteur proportionnelle à la note (échelle : `RANGE` ou la plus
  haute note gardée), le pas joué marqué d'une flèche (`arrow_down` de Ratchet, à partager) et rempli.
* Mode normal : encodeur gauche choisit le réglage, droit le change ; clic droit ouvre le sous-menu d'entrée CV
  (`Modulations`, D-036) sur un réglage assignable, ou exécute `REROLL` sur sa ligne.
* **Clic gauche : bascule en édition de l'affichage** (« EDIT » au titre) : encodeur gauche choisit le pas (curseur
  encadré), droit sa hauteur, d'une note de la gamme à la suivante (0..=48) ; clic droit retire ce seul pas ; clic
  gauche ressort. `up` / `down` restent au panneau (focus, routage, D-046).
* Réglages (16, pages de 3) : `LENGTH` 1–32, `PROB` 0–100 %, `RANGE` 1–4 oct, `SPREAD`, `BIAS`, `JUMP` OFF/1–7,
  `REPEAT`, `SCALE`, `ROOT`, `OCTAVE` −3..+3, `TRANSPOSE` −24..+24 dt, `CHORD`, `INVERT`, `OUT D`, `GATE` 1–99 %,
  `REROLL`. Assignables : `LENGTH`, `PROB`, `RANGE`, `SPREAD`, `BIAS`, `TRANSPOSE` (`Modulations<6>`), lus à
  l'horloge ; `TRANSPOSE` prend `CV1` en `ADD` au démarrage, 1 V = 12 demi-tons (comme `NOTE` de `SUPERSAW`).

## Trait `App`

`PARAM_SPECS` (les 16 sauf `REROLL`), `get` / `set` / `param_active` ; observables `STEP`, `NOTE` ; actions
`REROLL`, `RESET` ; `assigns`, `assign_sweep` ; ports de sortie `NOTE`, `CH1`, `CH2`, `CH3/G` ; entrées `CLOCK`,
`RESET`. Notes des pas non exposées à la sonde (réserve).

## Fichiers

* `crates/oc-core/src/apps/melody/` : `mod.rs` (`MelodyApp`, panneau, rendu, `impl App`), `scale.rs` (gammes,
  quantification, pas de degré, accords), `sequence.rs` (pas, position, reset, tirages, marche, `REROLL`).
* `crates/oc-core/src/apps/mod.rs` : `AppId::Melody` **ajouté après `Nothing`** (index persisté 7) ; `ALL`,
  `name`, `category` ; nouvelle `Category::Sequence` (« SEQUENCE », avant `System`) ; variante `Instance::Melody`
  et ses trois `match` ; doc de module. Vérifier le doc « Last in the menu » de `Nothing` (le menu trie par
  catégorie, `menu.rs`).
* Partage : `ratchet/tempo.rs` → `crates/oc-core/src/tempo.rs` (réexporté par `ratchet` pour ne rien casser), plutôt
  qu'un import d'applet à applet ; `arrow_down` → `gfx` (ou `gfx::controls`).
* Réutiliser : `Modulations` / `slot_of` (`modulation/slots.rs`), `ParamSpec::number/choice/toggle().assignable()`
  (`apps/param.rs`), `tone::note_name`, `Params::levels`, `gfx::controls::{scroll_bar, row_highlight}`,
  `TextBuf`, `text::draw`, `ratchet::wrapped` (à rendre commun si besoin).
* Tests : unitaires dans `scale.rs`, `sequence.rs` (proptest : toute note tirée est dans la gamme et l'étendue ;
  `PROB` 0 % rejoue la même boucle ; `JUMP` ne répète jamais) ; intégration `crates/oc-core/tests/melody.rs` par
  `testing::mock_engine` (horloge, reset, longueur, édition au panneau, accords, gate, `REROLL`) ; ajouter
  `AppId::Melody` aux boucles de `tests/audio.rs` ; `tests/menu.rs` pour la nouvelle catégorie.
* Simulateur : `crates/oc-sim/tests/scenarios/melody.scn`, `melody_edit.scn` ; écrans régénérés par
  `UPDATE_SCREENS=1` (dont `app_menu*.screen`, la colonne de gauche change), diff relu avant commit.
* Docs : ADR **`journal/D-047-applet-melody.md`** (français, premier fichier du commit de code, index de
  `journal/README.md` et tableau « Décisions actées » de `PLAN.md`) ; ce plan copié en
  `docs/plan/etape-14-melodie.md` (suivi par git) ; `PLAN.md` (étape 14 et lots) ; `README.md` (usage) ;
  `TESTING.md` (niveau 9 : A à l'accordeur, horloge de `RATCHET` ou externe, reset, accords, gate) ;
  `docs/agents/code-map.md` ; `CHANGELOG.md` ; `docs/plan/avancement/etape-14.md` en fin d'étape.

## Lots

| Lot | Contenu |
|---|---|
| 14a | ADR D-047, `docs/plan/etape-14-melodie.md`, `PLAN.md` (commit de doc) |
| 14b | `tempo` et `arrow_down` partagés ; `scale.rs` + tests |
| 14c | `sequence.rs` : horloge, reset, longueur, remplacement, `SPREAD` / `BIAS`, `JUMP`, `REPEAT`, `REROLL` + tests |
| 14d | `MelodyApp`, registre, panneau (réglages, édition, sous-menu), sorties, trait `App` ; tests d'intégration |
| 14e | rendu ; scénarios du simulateur et écrans de référence |
| 14f | docs, `cargo xtask size` (coût en ITCM ; 21,7 Kio libres, alerte sous 16 Kio), essai sur le module, avancement |

## Vérification

* Définition de fini (AGENTS.md) : `cargo fmt --all -- --check` ; `cargo clippy --all-targets --all-features -- -D
  warnings` ; `cargo clippy -p oc-firmware -p oc-board -p oc-realtime --target thumbv7em-none-eabihf -- -D
  warnings` ; `cargo test --all-features` ; `cargo xtask build && cargo xtask size`.
* `cargo run -p oc-sim` : lancer `MELODY` (catégorie `SEQUENCE`), horloge à la main ou par `RATCHET` dans un autre
  emplacement routé, vérifier l'affichage, le pointeur, l'édition au clic gauche.
* `cargo probe` : `DescribeApp`, `SetParam` sur `SCALE`, `Act REROLL`, `Observe STEP`.
* Module (niveau 9) : A à l'accordeur sur `CHROMATIC` puis `MAJOR`, B–D en accord, D en gate, reset par `TR2`.

## Réserves prévues

Notes des pas non sauvegardées ni exposées à la sonde ; pas de silences / densité de gates ; pas de verrou par pas ;
quantification au millivolt (D-041).
