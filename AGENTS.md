# AGENTS.md — technical specifics of this repository

Facts about *this* codebase that are easy to get wrong by analogy with a
typical embedded or workspace project, and the rules every change must keep.

**This file is the single source of truth for agents** (Claude Code, Junie,
Codex, …). Tool-specific files such as `CLAUDE.md` are local, untracked, and
only point here plus tool-specific notes; anything that matters for the
repository belongs in this file or in a file it routes to, never only in a
tool file (journal/D-001).

## Where to read

| Need | Read |
|---|---|
| Project state, the lot in progress, decisions taken | **`PLAN.md`, first** |
| Why a choice was made | `journal/` (ADRs, French; index in `journal/README.md`) |
| Firmware: build chain, the loop and the audio interrupt, the screen, measuring costs | `docs/agents/firmware.md` |
| What each crate does, which file holds what | `docs/agents/code-map.md` |
| Host tools: flashing variants, probe, browser, VCV Rack, release, benchmarks | `docs/agents/tools.md` |
| Test protocol per level, the hardware checklist | `TESTING.md` |
| Memory map, boot sections, reserved flash | `crates/oc-firmware/MEMORY.md` |
| The hardware: principles, data sheets, this module's measurements (French) | `docs/hardware.md` |
| User-facing usage | `README.md` |

## Working rules

* **Read `PLAN.md` first**, then only what the current lot needs:
  `docs/plan/roadmap.md`, `docs/plan/avancement/etape-N.md`,
  `docs/plan/revue-decisions-2026-11-16.md`. Update `PLAN.md` before
  committing at the end of a lot; when a step ends, write
  `docs/plan/avancement/etape-N.md` and remove the step's lot table from
  `PLAN.md`, which keeps its one-line summary only.
* **Architectural decisions go in `journal/` as ADRs** (format in
  `journal/README.md`). From `D-024` on, the ADR is committed *before or
  with* the code it describes, as the first file of that commit. Never start
  an architectural change without its entry; if the implementation diverges
  from a recorded decision, *revise* that entry (`Statut: adoptée, révisée`,
  dated section) rather than opening a new one. Numbers are never reassigned.
  Keep the index in `journal/README.md` and the "Décisions actées" table in
  `PLAN.md` in sync. An assistant plan that carries an architectural decision
  must be tracked by git.
* **Language**: working docs (`PLAN.md`, `docs/plan/`, `journal/`,
  `docs/hardware.md`) in French; code, comments, commit messages,
  `README.md`, `AGENTS.md`, `docs/agents/` and `TESTING.md` in English.
* **Measurements** go in `docs/plan/avancement/`, not in this file or its
  annexes.
* **Definition of done**: a lot is done only when the CI commands pass
  locally (journal/D-001, D-017):

```sh
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo clippy -p oc-firmware --target thumbv7em-none-eabihf -- -D warnings
cargo test --all-features
cargo xtask build && cargo xtask size
```

## Commands

```sh
cargo test                                  # host crates only (oc-firmware excluded, see below)
cargo test -p oc-core some_test_name        # one crate, one test

cargo run -p oc-sim                         # native simulator (ratatui TUI)
cargo run -p oc-sim -- run --record bug.scn # record a simulator scenario
cargo run -p oc-sim -- replay bug.scn       # replay one headless
UPDATE_SCREENS=1 cargo test -p oc-sim --test scenarios   # regen golden .screen snapshots after an intentional render change
UPDATE_FONTS=1 cargo test -p oc-core --test fonts         # regen the text font tables from embedded-graphics

cargo xtask build                           # cross-compile the firmware (release)
cargo xtask size                            # section table + linker-layout checklist
cargo xtask hex                             # dist/oc-firmware.hex
cargo xtask flash --dry-run                 # validate only, upload nothing
cargo probe stats                           # live profile of a running module
```

`cargo xtask`, `cargo fw`, `cargo sim` and `cargo probe` are aliases in
`.cargo/config.toml`. Other commands: `docs/agents/tools.md`.

## What this is

A Rust firmware for the **Ornament & Crime** Eurorack module (TLM Audio
build, **Teensy 4.0 / NXP i.MX RT1062**, Cortex-M7 @ 600 MHz), built as an
engineering foundation: cross-compiled firmware, native simulator, VCV Rack 2
module and safe flashing tool, all on one behavioural core. Applets are
picked from a front-panel menu; the list is `AppId` in
`crates/oc-core/src/apps/mod.rs`.

| Crate | `no_std` | `unsafe` |
|---|---|---|
| `oc-core` (all behaviour), `oc-drivers` (peripheral protocols), `oc-protocol` (probe messages), `oc-halfkay` (bootloader reports) | yes | `forbid(unsafe_code)` |
| `oc-firmware` (wiring, the loop, the audio path) | yes (bin) | `deny(unsafe_code)`, three exceptions (below) |
| `oc-sim`, `oc-probe`, `oc-image`, `oc-web`, `oc-vcv-ffi`, `oc-buildinfo`, `xtask` | no | ordinary (`oc-vcv-ffi`: defensive C ABI) |

Details per crate: `docs/agents/code-map.md`.

## Rules that are easy to break

### Behaviour lives in `oc-core`

* **`oc-core` is the single source of behavioural truth.** The firmware, the
  simulator and VCV Rack differ only in how they read and write signals;
  every decision (mode cycling, calibration, debouncing, rendering) lives in
  `oc-core` and is tested once, on the host.
* **`Engine::tick` is the one function every backend calls**; a new
  behaviour belongs there or in an applet reached through `AppHost`, tested
  via `oc_core::testing::mock_engine`, never duplicated per backend.
* **`oc-firmware` has no unit tests, on purpose**: add them to `oc-core` or
  `oc-drivers` and wire the firmware to the tested abstraction.
* **No backend reports whether a cable is plugged in** (the module cannot
  know). An applet that needs a default for an unused input asks
  `oc_core::presence`, which derives it from the levels; never add a
  backend-specific source such as Rack's `isConnected()` (journal/D-019,
  D-024).
* **Holding `up`+`down` opens the app menu**, so `up` and `down` fire their
  own action on release (`crates/oc-core/src/buttons.rs`).

### Signals

* **An applet publishes what each output carries, not samples**
  (`oc_core::audio::Params`: a level held, or a tone). A `Voice` renders them
  one sample at a time at its backend's rate, keeping each output's 32-bit
  phase across changes (journal/D-033, D-042). **Anything that depends on the
  parameters alone belongs in `Setup::new`**, never per sample
  (journal/D-040).
* **Internal unit: millivolts, `i32`** (`oc_core::platform::MilliVolts`). No
  floating point in the signal path, with one exception on trial: the `f32`
  supersaw bank (`audio/supersaw/float.rs`, journal/D-039), millivolts at its
  edges, `core` only (no `sqrt` or `round` without `std`). Otherwise `f32`
  appears only in Rack's C++ (`Alambic::process()`); `oc-vcv-ffi`'s ABI stays
  `int32_t` millivolts.
* **Backends exchange raw converter codes**, not millivolts
  (`AnalogIn::read_code`, `AnalogOut::write_code`); `Engine` converts them
  with its `Calibration` (journal/D-027). The simulator and Rack quantise
  inputs to 12 bits too; tests comparing an input with an output use
  `testing::adc_reading`.
* **Both analog stages invert.** `Calibration::FACTORY` expresses it as a
  *negative* input slope and a *decreasing* output table, enforced by
  `const` assertions: a positive slope there is a hardware-modelling bug.
  Per-module values come from the calibration wizard, not from editing
  constants (journal/D-006, D-026, D-027).

### Firmware and hardware safety

* **`crates/oc-firmware/src/board.rs` is the only file allowed to name a
  Teensy pin.** A wrong pin direction is the one way this firmware could
  damage the hardware. `board::split` hands out signal groups whose `claim`
  methods apply direction and pad configuration; a new pin goes in the table
  *and* in a group. Enforced by `xtask/tests/pin_rule.rs` (journal/D-007).
* **`unsafe` is confined to three modules** of `oc-firmware`, each with a
  module-level `#![allow(unsafe_code)]` and a `SAFETY:` comment on every
  block: `flash_store.rs` (boot ROM flash driver, journal/D-028),
  `reboot.rs` (fuse read and hand-over to the bootloader, journal/D-029),
  `audio_irq.rs` (the DAC's DMA, its ring, the audio handlers,
  journal/D-033). Any new `unsafe` goes in a module of its own, the same way.
* **The firmware drives no UART**: every LPUART on pins 0–23 collides with
  the panel (`crates/oc-firmware/src/main.rs`). Diagnostics use USB CDC (a
  text log port and a binary probe port) and the boot LED (journal/D-011).
* **The top 64 KiB of the flash (`0x601F_0000`..) is reserved**: the settings
  in its first four sectors, Teensyduino's EEPROM emulation and restore
  program above. `xtask::validate` rejects an image that reaches it; never
  move the settings into the last sector (`MEMORY.md`).
* **`oc-firmware` is outside `default-members`**, and **`RUSTFLAGS` must
  never be set in the environment** (it drops `-Tt4link.x`). There is no
  `memory.x` and no `flip-link`, on purpose (`docs/agents/firmware.md`).

### Tests

* **Never hand-edit a `.screen` file**: regenerate with
  `UPDATE_SCREENS=1 cargo test -p oc-sim --test scenarios` and diff before
  committing (`crates/oc-sim/tests/scenarios/`).
* **Tool crates keep their seams**: the pre-flash gate is a pure function
  over bytes in `oc-image`, free of OS dependencies; `oc-probe`'s command
  line sits behind its `cli` feature (`docs/agents/tools.md`).

## Code style and lints

* Edition 2024; MSRV equal to the pinned toolchain (`rust-toolchain.toml`,
  `[workspace.package] rust-version`), so every CI job tests it
  (journal/D-017).
* Workspace lints in the root `Cargo.toml`: `clippy::pedantic`,
  `missing_debug_implementations`, `unreachable_pub`, `rust_2018_idioms`
  (warn). Clippy runs with `-D warnings` on the host *and* for `oc-firmware`
  on its target, separately because of `default-members`.
* `clippy::module_name_repetitions` is allowed workspace-wide; anything else
  gets a narrowly scoped `#[allow]` with a one-line justification. Do not
  blanket-allow a pedantic lint to make a function pass; prefer restructuring
  (see the history of `validate_image` in `crates/oc-image/src/validate.rs`).
* Doc comments explain **why**, not what; British-leaning English, full
  sentences, no emoji. Every `pub` / `pub(crate)` item needs one.
* let-else and inline format arguments (`format!("{x}")`) are the norm.
* `cargo fmt --all -- --check` must pass; `rustfmt.toml` only pins the
  edition.

## Not done yet

Do not assume these work; the reserves of each step in `PLAN.md` are the full
list, and `TESTING.md` ("What is not tested yet") the verification side.

* The VCV Rack module loads in Rack but has not been checked systematically
  there, nor listened to.
* No Renode smoke test (journal/D-012).
* On the module, the DAC8565 simultaneous update is not checked; the inputs
  read within about 20 mV only (one gain shared by the four inputs).
