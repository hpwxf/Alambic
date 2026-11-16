# Revue des décisions reconstruites — 2026-11-16

Relecture des 22 décisions rétrospectives (D-002 à D-023) contre le code, les tests et les documents. Deux
catégories :

* **Révision** — un problème net : la décision, son application ou sa documentation est fausse et il faut la
  changer ;
* **Revue** — à vérifier : la décision est défendable mais repose sur une hypothèse non vérifiée, un compromis non
  écrit, ou une mesure qui manque.

Chaque point renvoie à la section « Points ouverts » de l'ADR. Les décisions absentes des deux listes (D-003,
D-004, D-005, D-009 hors point mineur, D-010, D-014) sont jugées saines en l'état.

## À réviser

✅ = traité au lot 3a (2026-11-16) : `f862a9e` (R1), `3cf09e9` (R2), `61de0a6` (R3), `4766d31` (R4) ; voir la
section « Révision » ou « Points ouverts » de chaque ADR.

| # | ADR | Problème | Preuve | Correctif proposé |
|---|---|---|---|---|
| R1 ✅ | D-013, D-017 | **CI rouge sur `master`** : `cargo test --all-features` active `octant`, les écrans dorés sont en braille → 5 / 9 scénarios échouent | `cargo test --all-features` le 2026-11-16 ; sans features : tout vert | forcer le braille dans la comparaison dorée ; aligner « fait » local et CI |
| R2 ✅ | D-019 | **Triggers de moins de 3 ms perdus** : l'anti-rebond mécanique (3 échantillons à 1 kHz) s'applique à des fronts électroniques ; le scénario doré `trigger_burst` classe une impulsion de 1 ms comme rebond. Aggravé par la gigue d'≈ 1 ms toutes les 20 ms (D-008) | `debounce.rs`, `trigger_burst.scn` (TR2 : impulsions d'un tick ignorées) | `required = 1` (ou front sur 2 échantillons) pour les triggers ; capture par IRQ / cadence ISR avec D-023 |
| R3 ✅ | D-007 | **Règle de sécurité « seul `board.rs` nomme une broche » fausse dans le code** : `main.rs` réserve les 27 broches ; la broche 12 (SDI) est absente de la table ; rien ne vérifie la concordance | `grep pins.p crates/oc-firmware/src` | déplacer la réservation dans `board.rs` (fonction qui consomme `Pins`) |
| R4 ✅ | D-011, D-015 | **Documentation périmée** : « never run on real hardware » (`README.md`, `AGENTS.md`, `board.rs`) alors que `3f50a42` / `a8f3560` corrigent des défauts vus sur le module ; « never loaded into Rack » contredit par `8bdd9bc` | messages de commit, `docs/supersaw/` § 13 | écrire ce qui est vérifié (écran, panneau) et ce qui ne l'est pas (calibration, CV, triggers) |
| R5 ✅ | D-019, D-002 | **`is_patched` diverge entre backends** : `true` sur le module (pas de détection de jack), vrai `isConnected()` dans Rack → écrans de diagnostic différents | `cv_in.rs`, `Alambic.cpp` | retirer du trait, ou forcer `true` hors matériel |
| R6 ✅ | D-023 | **Plan A (ISR 16 666 Hz) incompatible avec l'étude audio** : OLED sous masque d'IRQ = ≈ 16 échéances manquées par trame, que `docs/supersaw/` § 13 juge inacceptable ; et le plan est **hors git** | `.git/info/exclude`, plan local § Budget | suivre le plan sous `docs/plan/`, mesurer (3b), choisir l'objectif avant la cadence |

## À revoir (vérifier)

| # | ADR | Point | Ce qui trancherait |
|---|---|---|---|
| V1 | D-006 | constantes de calibration nominales, plage d'entrée −5…+5 V non sourcée, aucune persistance | niveau 9 : mesure de 0 V, ±5 V et 1 V/oct par canal |
| V2 | D-008 | gigue de la boucle (tick d'OLED ≈ 2 ms), jamais mesurée | GPIO + oscilloscope ; chiffre dans `avancement/` |
| V3 ◐ | D-017 | toolchain `stable` flottante avec `-D warnings` + `pedantic` ; `rust-version = 1.85` jamais testé ; pas de `cargo deny` | épingler une version ou ajouter un job MSRV ; `deny.toml` — **épinglé 1.100.0 = MSRV le 2026-11-16 ; reste `cargo deny`** |
| V4 | D-018 | `overflow-checks = true` en release : un débordement fige le module (sorties gelées jusqu'à +6 V) | critère explicite : jusqu'au premier applet de jeu, puis arithmétique saturante + compteur d'erreurs |
| V5 ✅ | D-016 | validation de l'image réelle seulement au flash ; `flash --dry-run` absent de la CI ; `flash` reconstruit toujours (l'image flashée n'est pas l'artefact CI) | ajouter `cargo xtask flash --dry-run` au job firmware |
| V6 | D-020 | splash rendu à chaque tick : trame ≈ 1,05 ms > période de 1 ms, chaque tick dépasse pendant 1,5 s ; sortie « 0 V » dépend de l'ordre reset DAC / première écriture | rendre le splash à la cadence UI ; vérifier les sorties à la mise sous tension (niveau 9) |
| V7 | D-021 | haut / bas différés au relâchement : latence perçue jamais éprouvée sur le module ni dans Rack | essai au niveau 9 et dans Rack |
| V8 | D-022 | licence implicite : pas de fichiers `LICENSE-*`, `plugin.json` dit `MIT` seul, provenance du firmware de référence non notée | décision explicite + fichiers de licence |
| V9 | D-012 | smoke test Renode « ouvert » depuis l'étape 0, sans échéance | l'abandonner formellement ou le reformuler et le dater |
| V10 | D-002 | `oc_core::testing` public, non conditionnel, sert de backend de production au simulateur et à Rack | renommer (`host`) et/ou feature `host` |
| V11 | D-014 | mode de mise à jour simultanée du DAC8565 « inféré de la datasheet », jamais exercé | test du niveau 9 dédié |
| V12 | D-009 | features `ssd1306` / `ssd1309` exclusives sans garde (`ssd1309` gagne en silence) | `compile_error!` sur la combinaison |

## Hors périmètre ADR mais signalé

* `HARDWARE.md` et `TODO.md` sont exclus par `.git/info/exclude` (le plan ISR l'était aussi ; il est suivi sous
  `docs/plan/` depuis le lot 3a) : documents
  de travail invisibles en revue. `TODO.md` contient un brochage O.R.N.8 non sourcé, à ne pas réutiliser tel quel.
* `test_nvidia_nim.py`, `real.png`, `venv/` à la racine : non suivis, sans rapport avec le firmware.
* Dates d'auteur non monotones (`baf70c7` daté avant son parent `83be36f`) : historique réécrit.
