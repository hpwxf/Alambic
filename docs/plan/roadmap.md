# Feuille de route

Étapes passées (reconstruites depuis `git log` le 2026-11-16) et à venir. Le bilan d'une ligne par étape est dans
`PLAN.md` ; le détail de chaque étape terminée dans `avancement/`.

## Étapes passées

| Étape | Dates | Commits | Plan |
|---|---|---|---|
| 0 — fondation | 2026-08-01 | `1524760` → `40e8c3f` | `.junie/plans/oc-rust-firmware-foundation.md` (étapes 1 à 5 du plan Junie) |
| 1 — mise en route sur le module | 2026-08-02 | `350ee90` → `2deca8a` | aucun (lots au fil des essais) |
| 2 — plusieurs applets | 2026-08-19 → 2026-09-05 | `83be36f` → `0b0fe3b` | aucun ; rationale dans les messages de `a839a03`, `8bdd9bc` |

## Étape 3 — assainissement, puis choix de la cadence

Critères d'acceptation :

* `cargo test --all-features` vert localement et en CI (D-013, D-017) ;
* une impulsion trigger de 1 ms est comptée, au simulateur (scénario doré) et sur le module (niveau 9) (D-019) ;
* aucun fichier hors `board.rs` ne nomme une broche, ou la règle est reformulée et vérifiée (D-007) ;
* `README.md`, `AGENTS.md` et `board.rs` disent exactement ce qui a été vérifié sur le module et dans Rack
  (D-011, D-015) ;
* niveau 9 de `TESTING.md` exécuté, résultats consignés dans `avancement/etape-3.md` ;
* D-023 tranchée (statut `adoptée` ou `abandonnée`), et le plan ISR suivi par git sous `docs/plan/`.

## Étapes suivantes (non planifiées)

| Étape | Contenu pressenti | Dépend de |
|---|---|---|
| 4 — premier applet musical | quantizer ou générateur d'enveloppes / LFO ; calibration persistante | D-006, D-023 |
| 5 — chemin audio | supersaw PolyBLEP, DAC en DMA, priorités NVIC (`docs/supersaw/` § 12–14) | D-023 (option B) |
| ? — seconde carte | Teensy 4.1 / O.R.N.8 (notes locales non suivies) | D-003, D-007 |

## Questions ouvertes

* Cadence : contrôle (16 666 Hz) ou audio (48 kHz + DMA) ? (D-023)
* Comment Squares-and-Circles partage-t-il le bus SPI sans tic sur ce module ? (D-010, `docs/supersaw/` § 13)
* Plage réelle des entrées CV de la carte TLM T4 ? (D-006)
* `overflow-checks` en release au-delà des applets de diagnostic ? (D-018)
* Distribution du plugin Rack et licence (D-022) ?
