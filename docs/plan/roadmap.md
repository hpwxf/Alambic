# Feuille de route

Étapes passées (reconstruites depuis `git log` le 2026-11-16) et à venir. Le bilan d'une ligne par étape est dans
`PLAN.md` ; le détail de chaque étape terminée dans `avancement/`.

## Étapes passées

| Étape | Dates | Commits | Plan |
|---|---|---|---|
| 0 — fondation | 2026-08-01 | `1524760` → `40e8c3f` | `.junie/plans/oc-rust-firmware-foundation.md` (étapes 1 à 5 du plan Junie) |
| 1 — mise en route sur le module | 2026-08-02 | `350ee90` → `2deca8a` | aucun (lots au fil des essais) |
| 2 — plusieurs applets | 2026-08-19 → 2026-09-05 | `83be36f` → `0b0fe3b` | aucun ; rationale dans les messages de `a839a03`, `8bdd9bc` |
| 3 — assainissement, sonde live, cadence | 2026-11-16 → 2026-11-17 | `c6c2192` → `bc42378`, `706410a` → ce commit | `revue-decisions-2026-11-16.md`, `etape-3b-sonde.md`, `etape-3c-cadence.md` |
| 4 — réglages système, calibration, persistance | 2026-11-16 | `92ab189` → `v0.3.0` | `etape-4-reglages-systeme.md` |
| 5 — primitives graphiques et applet de démo | 2026-11-17 | `53ee310` → ce commit | `etape-5-gfx.md` |
| 7 — chemin audio | 2026-11-17 → 2026-11-18 | `336817a` → `v0.5.0` | `etape-7-chemin-audio.md`, `rapport-etape-7-chemin-audio.md` |

## Étape 3 — critères d'acceptation, tenus

* `cargo test --all-features` vert (D-013, D-017) ;
* une impulsion trigger de 1 ms comptée au simulateur (lot 3a) et sur le module (lot 3c1 : 960 / 960 à 0,94 ms) ;
* règle du brochage vraie et testée (D-007) ; docs de vérification exactes (D-011, D-015) ;
* niveau 9 exécuté, consigné dans `avancement/etape-3.md` (sauf la mise à jour simultanée du DAC8565) ;
* D-023 tranchée (option C+), plan A suivi par git.

## Étapes suivantes (non planifiées)

| Étape | Contenu pressenti | Dépend de |
|---|---|---|
| 6 — premiers applets musicaux | Ratchet (D-032, plan `etape-6-ratchet.md`), puis une seconde à décrire ; à 1 kHz (D-023 C+) | D-019, D-023, D-032 |
| 8 — premier oscillateur | supersaw PolyBLEP sur le chemin audio de l'étape 7 (`docs/supersaw/` § 12–14) ; à planifier | D-033 |
| ? — seconde carte | Teensy 4.1 / O.R.N.8 (notes locales non suivies) | D-003, D-007 |

## Questions ouvertes

* ~~Cadence~~ : C+ à 1 kHz pour le contrôle, et depuis l'étape 7 un chemin audio à 47 619 Hz par DMA (D-033, option B de D-023)
* ADC non bloquant : en attente (Teensy 4.0) ; trois voies notées dans D-023, de l'horloge plus rapide à la chaîne timer → ADC_ETC → DMA
* Comment Squares-and-Circles partage-t-il le bus SPI sans tic sur ce module ? (D-010, `docs/supersaw/` § 13)
* ~~Plage réelle des entrées CV de la carte TLM T4~~ : ≈ −3,33 à +6,67 V, fixée par l'étage analogique (lot 3b5)
* `overflow-checks` en release au-delà des applets de diagnostic ? (D-018)
* Distribution du plugin Rack et licence (D-022) ?
