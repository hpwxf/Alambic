# Changelog

Firmware and workspace versions follow `0.<minor>.<patch>`; the VCV Rack
plugin, which Rack 2 requires to have major version 2, ships as
`2.<minor>.<patch>` of the same release (checked by `xtask/tests/versions.rs`).
Architectural decisions behind each change are in `journal/` (French).

## Unreleased

### Changed

- **The core converts, the backends pass raw codes.** `AnalogIn::read_code`
  and `AnalogOut::write_code` replace the millivolt `read_cv` / `write_cv`;
  `Engine` owns a `Calibration` (`calibration()`, `set_calibration()`) and
  converts both ways. The DAC8565 driver sends codes and no longer holds a
  calibration (journal/D-027).
- **Factory calibration taken from the reference firmware.** CV inputs read
  0 V at ADC code 2730 (two thirds of the range, not mid-scale), 10 V over
  the 4096 codes, so about -3.33 V to +6.67 V instead of -5 V to +5 V. CV
  outputs use a table of one DAC code per octave (-3 V to +7 V, Phazerville's
  model), 10 V over the DAC range, 0 V at code 40984 instead of 43690. On the
  module, every output therefore moves by about 0.4 V for the same request
  compared with 0.2.0 until it is calibrated.
- **Settings and their persistence (core, simulator, Rack).**
  `oc_core::settings` holds the calibration, the startup applet and the screen
  and encoder preferences, encoded with `postcard` and kept in flash by
  `sequential-storage` (wear levelling, CRC, repair after a power cut).
  `Engine::apply_settings` installs them; a reset restarts in the startup
  applet. The simulator keeps them in a flash image (`run --settings FILE`);
  the VCV Rack module keeps them in the patch (`oc_engine_settings_save` /
  `oc_engine_settings_load` in the C ABI, base64 in `dataToJson`). The
  firmware does not persist them yet (journal/D-028).
- **System settings screen.** `SETUP`, the last entry of the app menu, edits
  the startup app, the contrast, a 180° rotation, a picture shift and each
  encoder's direction, live; `SAVE` keeps and stores them, `EXIT` or the
  `up` + `down` chord puts back what was in force. Held through the splash
  screen, the left encoder switch opens it on the calibration line and
  `up` + `down` in safe mode (screen and encoders at their defaults until
  saved); the splash recalls both gestures. Rotation and shift are applied to
  the pixels in the core, contrast through the new `Display::set_contrast`
  (journal/D-026).
- **Simulator and VCV Rack quantise CV inputs like the module's 12-bit ADC**
  (about 2.44 mV per step): an input of 1.000 V now reads 1.001 V there, as
  it would on the hardware.

## 0.2.0 — 2026-11-16 (Rack plugin 2.2.0)

### Changed

- **Trigger inputs are no longer debounced.** A trigger level is accepted on
  its first 1 kHz sample, so the common 1 ms Eurorack trigger is counted;
  previously anything shorter than 3 ms was dropped. Buttons and encoder
  switches keep their 3-sample debounce. A mechanical push-button patched
  into a trigger jack now counts its bounces (journal/D-019).
- **Input presence is derived from the signal, no longer reported by the
  backend.** The module's jacks cannot detect a cable, so the firmware used to
  claim every input was patched while Rack reported the truth. Now
  `oc_core::presence` decides on every backend from the levels alone: a CV
  input is present (`P`) when it has moved or sat more than 100 mV from 0 V
  within the last 300 ms; trigger inputs offer a first-edge latch or a 2 s
  clock hold. Applets use it for defaults on unused inputs. The simulator's
  `p` key and `[cable]` marker and the scenario `patch` event are gone
  (scenarios containing `patch` lines are rejected with a message: delete
  those lines), and the C ABI's `oc_engine_set_cv_in` lost its `patched`
  argument (journal/D-019, D-024, D-015).
- **Pin claiming moved into `board.rs`.** The firmware's pins are reserved by
  signal group in `crates/oc-firmware/src/board.rs`; a host test fails if any
  other firmware source names a Teensy pin (journal/D-007). Pin 12 (SPI MISO)
  is now in the pinout table.
- The VCV Rack plugin targets macOS 11.0 or later, which removes the libc++
  "selected platform is no longer supported" warning; `--rack-dir` accepts a
  relative path.

### Fixed

- The VCV Rack plugin is relinked whenever the Rust staticlib changes. The
  SDK's link rule only depended on the C++ objects, so a Rust-only change
  left a stale `plugin.dylib` (the splash still said v0.1.0 under a 2.2.0
  manifest).
- `cargo test --all-features` (the CI command) failed 5 of 9 simulator
  scenarios: golden screens are now always compared in braille, whatever the
  `octant` feature selects for the interactive simulator (journal/D-013).

### Changed (toolchain)

- Rust toolchain pinned to **1.100.0** (was the floating `stable` channel) and
  MSRV raised from 1.85 to 1.100 (journal/D-017).

### Added

- Every backend's splash screen shows the build identifier under the version:
  short commit hash, `-dirty` when tracked files differed, read through
  vergen-gix so no `git` binary is needed (journal/D-025);
  the firmware also logs it on USB.
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
