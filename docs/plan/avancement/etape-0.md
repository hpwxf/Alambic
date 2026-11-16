# Étape 0 — fondation (2026-08-01)

Reconstruit le 2026-11-16 depuis `git log` et le plan `.junie/plans/oc-rust-firmware-foundation.md`.

## Fait

| Lot du plan Junie | Commit | Contenu |
|---|---|---|
| 1 — workspace, chaîne croisée, CI | `1524760` | workspace, `rust-toolchain.toml`, `t4link.x`, `xtask build|size|hex`, firmware témoin (bannière + LED), CI |
| 2 — `oc-core` | `555f492` | six traits, calibration, anti-rebond, quadrature, activité, framebuffer, `DiagnosticApp`, `Engine::tick`, 84 tests |
| 3 — simulateur | `84f9d86` | TUI ratatui, horloge virtuelle, scénarios + écrans dorés, 34 tests + 4 scénarios |
| 4 — firmware et flash | `ec140ed`, `d173510` | `oc-drivers` (DAC8565, SSD130x, triggers, panneau, bus partagé), `board.rs`, `xtask flash` + `validate.rs` |
| 5 — VCV Rack | `40e8c3f` | `oc-vcv-ffi`, plugin C++, `xtask vcv`, `TESTING.md`, `AGENTS.md` |

## Mesuré

* Bancs `criterion` : le rendu coûte environ mille fois la mise à jour d'état → rendu découplé (D-008).
* Plugin VCV compilé et lié contre un Rack SDK 2.6.x réel ; symboles `oc_engine_*` résolus.

## Corrigé (écarts au plan)

* Pas de `memory.x`, pas de `flip-link` (D-004).
* Crate `oc-drivers` ajouté (D-003).
* Aucun UART ; le smoke test Renode « bannière sur LPUART » devient infaisable et reste ouvert (D-011, D-012).
* Pas de détection USB du Teensy au flash (D-016).
* Pilote OLED maison plutôt que les crates `ssd1306` / `ssd1309` (D-009).
