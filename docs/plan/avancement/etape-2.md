# Étape 2 — plusieurs applets (2026-08-19 → 2026-09-05)

Reconstruit le 2026-11-16 depuis `git log`.

## Fait

| Commit | Contenu |
|---|---|
| `83be36f`, `c8b3039` | étude `docs/supersaw/` (repliement, PolyBLEP, architecture audio, partage du bus SPI ; essai Squares-and-Circles sur le module) |
| `baf70c7` | écrans de référence du simulateur régénérés |
| `d97db13` | compteurs d'appuis par bouton, bascule AZERTY / QWERTY, haut + bas = remise à zéro de l'offset |
| `a839a03` | menu d'applets par accord, `AppHost`, `ButtonReader`, `ScopeApp` (D-021) ; entrée de menu contextuel Rack |
| `2a38b84` | `CLAUDE.md` |
| `190fb3f` | Rack SDK local ignoré par git |
| `8bdd9bc` | module Rack `Diagnostic` → `Alambic` (D-015) ; correctif clippy dans `xtask/tests` |
| `0b0fe3b` | `apps/` en module ; `InputSnapshot` et `TickContext` partagés |

## Mesuré

* Squares-and-Circles sur le Teensy du module : audio sans tic lié à l'écran malgré le bus SPI partagé
  (`docs/supersaw/` § 13).

## Corrigé

* L'accord haut + bas passe de la remise à zéro de l'offset (`d97db13`) au menu (`a839a03`) (révision de D-021).

## Constaté le 2026-11-16

* `cargo test --all-features` (commande de la CI) : 5 scénarios sur 9 échouent, la feature `octant` changeant les
  glyphes comparés aux écrans de référence en braille. Sans features : ≈ 290 tests, tous verts (D-013).
