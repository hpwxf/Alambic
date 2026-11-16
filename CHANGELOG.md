# Changelog

Firmware and workspace versions follow `0.<minor>.<patch>`; the VCV Rack
plugin, which Rack 2 requires to have major version 2, ships as
`2.<minor>.<patch>` of the same release (checked by `xtask/tests/versions.rs`).
Architectural decisions behind each change are in `journal/` (French).

## 0.2.0 — 2026-11-16 (Rack plugin 2.2.0)

### Changed

- **Trigger inputs are no longer debounced.** A trigger level is accepted on
  its first 1 kHz sample, so the common 1 ms Eurorack trigger is counted;
  previously anything shorter than 3 ms was dropped. Buttons and encoder
  switches keep their 3-sample debounce. A mechanical push-button patched
  into a trigger jack now counts its bounces (journal/D-019).
- **No more "cable plugged" indicator on any backend.** The module's jacks
  cannot detect a cable, so the diagnostic screen's `P`/`.` column, the
  simulator's `p` key and `[cable]` marker, and the scenario `patch` event
  are gone; signal presence (`~`) is the only indicator, identical on the
  module, in the simulator and in Rack. Scenarios containing `patch` lines are
  rejected with a message: delete those lines. The C ABI's
  `oc_engine_set_cv_in` lost its `patched` argument (journal/D-019, D-015).
- **Pin claiming moved into `board.rs`.** The firmware's pins are reserved by
  signal group in `crates/oc-firmware/src/board.rs`; a host test fails if any
  other firmware source names a Teensy pin (journal/D-007). Pin 12 (SPI MISO)
  is now in the pinout table.
- The VCV Rack plugin targets macOS 11.0 or later, which removes the libc++
  "selected platform is no longer supported" warning; `--rack-dir` accepts a
  relative path.

### Fixed

- `cargo test --all-features` (the CI command) failed 5 of 9 simulator
  scenarios: golden screens are now always compared in braille, whatever the
  `octant` feature selects for the interactive simulator (journal/D-013).

### Added

- CI runs `cargo xtask flash --dry-run`, validating the real firmware image
  and logging its SHA-256 (journal/D-016).
- `TESTING.md` Level 9.3 checks trigger inputs for over-counting (exact
  bursts, slow LFO, idle noise).
- An ADR journal (`journal/`), a plan index (`PLAN.md`) and `docs/plan/`;
  `AGENTS.md` is the single guide for every coding agent.

### Documentation

- `README.md`, `AGENTS.md` and `TESTING.md` now state what has been verified
  on a real module (OLED, USB log, boot LED, encoders, buttons) and what has
  not (CV inputs, CV outputs and calibration, triggers).

## 0.1.0 — 2026-09-05

First foundation: cross-compiled Teensy 4.0 firmware, shared `oc-core`
behaviour with a diagnostic and a scope applet behind an `up`+`down` app
menu, native simulator with golden scenarios, VCV Rack 2 module over a
defensive C ABI, and a validated flashing tool.
