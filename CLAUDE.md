# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What this is

Rust firmware for the **Ornament & Crime** Eurorack module (TLM Audio build,
Teensy 4.0 / NXP i.MX RT1062, Cortex-M7 @ 600 MHz): cross-compiled firmware, a
native simulator, a VCV Rack 2 module, and a safe flashing tool, all sharing
one behavioural core (`oc-core`). Read **`AGENTS.md`** before making any
structural change — it documents the facts about this repo that are easy to
get wrong by analogy with a typical embedded/workspace project (cross-compile
setup, signal units, safety-critical pin-naming rule, lint policy, testing
conventions). This file only summarizes the commands; `AGENTS.md` has the
architecture detail, `README.md` has user-facing usage, `TESTING.md` has the
full test protocol.

## Commands

```sh
cargo test                                  # host crates only (oc-firmware excluded, see below)
cargo test -p oc-core                       # single crate's tests
cargo test -p oc-core some_test_name        # single test
cargo clippy --all-targets --all-features -- -D warnings
cargo fmt --all -- --check

cargo run -p oc-sim                         # native simulator (ratatui TUI)
cargo run -p oc-sim -- run --record bug.scn # record a simulator scenario
cargo run -p oc-sim -- replay bug.scn       # replay one headless
UPDATE_SCREENS=1 cargo test -p oc-sim --test scenarios   # regen golden .screen snapshots after an intentional render change

cargo xtask build                           # cross-compile firmware (thumbv7em-none-eabihf, release)
cargo xtask size                            # section table + linker-layout checklist
cargo xtask hex                             # dist/oc-firmware.hex
cargo xtask build|hex --features ssd1306    # OLED override (default is SH1106); ssd1309 also available
cargo xtask flash --dry-run                 # validate only, upload nothing
cargo xtask flash                           # validate, show SHA-256 digest, confirm, upload
cargo xtask vcv build --rack-dir <path>     # build the VCV Rack 2 plugin (needs a downloaded Rack SDK)
cargo xtask vcv install --rack-dir <path>   # build + drop into the Rack user plugin folder
cargo xtask vcv clean                       # wipe plugin + oc-vcv-ffi build artefacts

cargo bench                                 # criterion benchmarks (crates/oc-core/benches/engine.rs)
```

`cargo fw` and `cargo sim` (aliases in `.cargo/config.toml`) shortcut the
firmware build and the simulator run. `cargo xtask` itself is also a cargo
alias (`run --package xtask --release --`).

**Never set `RUSTFLAGS` as an environment variable** — it replaces (not
appends to) `target.thumbv7em-none-eabihf.rustflags` from
`.cargo/config.toml` and silently drops `-Tt4link.x`, breaking the firmware
link. CI denies warnings via `-D warnings` on the command line instead.

`oc-firmware` only targets `thumbv7em-none-eabihf` and is excluded from
`default-members` in the root `Cargo.toml`, so bare `cargo build`/`test`/
`clippy` never touch it — build/lint it explicitly via `cargo xtask` or
`cargo clippy -p oc-firmware --target thumbv7em-none-eabihf`.

## Architecture

`oc-core` (`no_std`, `forbid(unsafe_code)`) is the single source of
behavioural truth: platform traits, `Engine::tick`, the applets and the app
menu, calibration, debouncing, quadrature decoding, framebuffer. The other crates
differ only in how they read/write signals:

| Crate | Role |
|---|---|
| `crates/oc-core` | All behaviour; knows nothing about registers or an OS |
| `crates/oc-drivers` | Peripheral protocols (DAC8565, SH1106/SSD1306/SSD1309, triggers, shared SPI) over `embedded-hal` 1.0, tested against recording mock buses |
| `crates/oc-firmware` | Pure wiring from `teensy4-bsp`/`imxrt-hal` to `oc-core`'s platform traits; the only crate touching registers; **no unit tests of its own** |
| `crates/oc-sim` | Host backend + ratatui TUI + deterministic virtual clock, running the real `oc-core` engine |
| `crates/oc-vcv-ffi` | `staticlib`/`rlib` exposing a defensive C ABI (`oc_engine_*`) over `oc-core`, `catch_unwind`-guarded |
| `vcv/OrnamentCrimeAlambic` | Rack SDK plugin shim (C++); module/port declaration and a framebuffer-reading widget only, no behaviour |
| `xtask` | The only supported build/flash/VCV entry point |

A new behaviour belongs in `Engine::tick` or in an applet reached through
`AppHost` (`crates/oc-core/src/apps.rs`), tested once via
`oc_core::testing::mock_engine` — never duplicated per backend, and never
added as an `oc-firmware` unit test. Holding `up`+`down` opens the app menu;
because of that, `up` and `down` fire their own action on release, arbitrated
in `crates/oc-core/src/buttons.rs`.

`crates/oc-firmware/src/board.rs` is the **only** file allowed to name a
Teensy pin; every other file reasons in `oc-core` types (`CvChannel`,
`TriggerChannel`, `Button`, …). Internal signal unit is millivolts (`i32`,
`oc_core::platform::MilliVolts`); `f32` appears only at the VCV Rack
boundary. Both analog stages of this hardware invert, encoded as *negative*
calibration slopes in `board.rs` — don't "fix" the sign without reading the
provenance comment there.

Full rationale for all of the above (cross-compilation specifics, memory
layout / why there's no `memory.x`, why `flip-link` isn't used, lint policy,
known gaps) is in `AGENTS.md`.
