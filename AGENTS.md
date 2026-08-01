# AGENTS.md — technical specifics of this repository

This file documents facts about *this* codebase that are easy to get wrong by
analogy with a typical embedded or workspace project. Read it before making
structural changes. User-facing instructions (how to run the simulator, key
maps, flashing) live in `README.md`; the test protocol lives in `TESTING.md`;
the historical rationale for every decision lives in
`.junie/plans/oc-rust-firmware-foundation.md`. This file is the shortest path
to "how does this repository actually work".

## What this is

A Rust firmware for the **Ornament & Crime** Eurorack module (TLM Audio
build, **Teensy 4.0 / NXP i.MX RT1062**, Cortex-M7 @ 600 MHz), built as an
engineering foundation rather than a musically rich firmware: cross-compiled
firmware, a native simulator, a VCV Rack 2 module, and a safe flashing tool,
all sharing one behavioural core. Musical applets beyond a diagnostic I/O
screen are explicitly out of scope for now.

## Workspace layout

| Crate                | `no_std` | `unsafe` policy            | Role |
|-----------------------|----------|-----------------------------|------|
| `crates/oc-core`      | yes      | `forbid(unsafe_code)`       | **All** behaviour: platform traits, `Engine::tick`, `DiagnosticApp`, calibration, debouncing, quadrature decoding, framebuffer. Knows nothing about registers or an OS. |
| `crates/oc-drivers`   | yes      | `forbid(unsafe_code)`       | Peripheral protocols (DAC8565, SSD1306/SSD1309, triggers, shared SPI bus) over `embedded-hal` 1.0 only; tested on the host against recording mock buses. |
| `crates/oc-firmware`  | yes (bin)| `deny(unsafe_code)`, currently zero `unsafe` of its own | Pure wiring: turns `teensy4-bsp`/`imxrt-hal` resources into the `oc-core` platform traits, runs the 1 kHz loop. The one crate that cannot be tested on the host. |
| `crates/oc-sim`       | no       | ordinary                    | Host backend + ratatui TUI + deterministic virtual clock; runs the real `oc-core` engine. |
| `crates/oc-vcv-ffi`   | no       | ordinary (defensive C ABI)  | `staticlib`/`rlib` exposing a defensive C ABI over `oc-core` (`oc_engine_*`), linked into the VCV Rack 2 module. Every function tolerates a null pointer or an out-of-range index and never lets a Rust panic unwind across the boundary (`catch_unwind`). |
| `vcv/OrnamentCrimeRust` | n/a (C++) | n/a | The Rack SDK plugin shim: module/port declaration, a widget reading the ABI's framebuffer, no behaviour of its own. Built by `cargo xtask vcv build\|install`, not by hand. |
| `xtask`               | no       | ordinary                    | The only supported build/flash/VCV entry point: cross-compilation, size report, HEX packaging, the pre-flash validation gate (`xtask::validate`), and the `vcv` subcommand (`xtask/src/vcv.rs`). |

`oc-core` is the single source of behavioural truth. The firmware, the
simulator and VCV Rack differ *only* in how they read and write signals;
every actual decision (mode cycling, calibration, debouncing, rendering)
lives in `oc-core` and is tested once, on the host.

## Cross-compilation specifics

* **Target:** `thumbv7em-none-eabihf`. Toolchain pinned by
  `rust-toolchain.toml` (stable channel, `rust-src` + `llvm-tools` components,
  that target); `rustup` provisions everything on first build.
* **`oc-firmware` is excluded from `default-members`** in the root
  `Cargo.toml`. A bare `cargo build`/`cargo test`/`cargo clippy` therefore
  never touches ARM code and operates on host crates only. Build, inspect and
  package the firmware explicitly through `cargo xtask build|size|hex`, or
  `cargo fw` (alias in `.cargo/config.toml`).
* **There is no `memory.x` in this repository, on purpose.**
  `teensy4-bsp`'s `rt` feature drives `imxrt-rt::RuntimeBuilder` at build
  time, which generates a complete linker script (`t4link.x`) into `OUT_DIR`,
  memory regions *and* the i.MX RT boot sections (FlexSPI Configuration
  Block, Image Vector Table) included. `.cargo/config.toml` points the
  linker at it (`-Tt4link.x`). Adding a hand-written `memory.x` would
  conflict with it — see `crates/oc-firmware/MEMORY.md` for the full layout
  (FlexSPI flash, ITCM, DTCM, OCRAM addresses) and for `cargo xtask size`
  usage to verify it.
* **`flip-link` is deliberately not used.** It requires a `MEMORY { RAM }`
  region and fails against the ITCM/DTCM/OCRAM regions `imxrt-rt` generates.
  Stack-overflow protection instead relies on `imxrt-rt` placing `.stack` at
  the very bottom of DTCM (`0x2000_0000`) below `.vector_table`/`.data`/
  `.bss`: an overflow grows downward out of DTCM and faults immediately.
  This is a checked invariant, not folklore — `MEMORY.md` explains how to
  verify it with `cargo xtask size`.
* **`CARGO_TARGET_DIR`/`RUSTFLAGS` caution:** CI deliberately does not set
  `RUSTFLAGS` as an environment variable, because that would *replace*
  `target.thumbv7em-none-eabihf.rustflags` from `.cargo/config.toml` and drop
  `-Tt4link.x`, breaking the firmware link silently. Warnings are denied per
  invocation (`-D warnings` on the command line) instead.
* **LLVM tools, not system binutils:** `xtask/src/llvm.rs` locates
  `llvm-size`/`llvm-objcopy` inside the active Rust sysroot
  (`rustc --print sysroot`), not on `PATH`, so the build stays reproducible
  across machines with different system toolchains.

## Signal representation

* **Internal unit: millivolts, `i32`** (`oc_core::platform::MilliVolts`).
  No floating point in the firmware's signal path; `f32` appears only at the
  VCV Rack boundary (`Diagnostic::process()` converting to/from Rack's
  volt-scaled `float`, never inside `oc-core` or `oc-vcv-ffi`'s own ABI,
  which stays `int32_t` millivolts). This keeps conversions exact and avoids
  FPU dependence in the critical loop.
* **Both the analog input and output stages of this hardware invert.**
  `crates/oc-firmware/src/board.rs` expresses this as *negative* calibration
  slopes (`nanovolts_per_code`, `millicodes_per_volt`), and enforces it with
  `const _: () = { assert!(...) }` compile-time checks — a positive slope
  there is a hardware-modelling bug, not a style choice. Do not "fix" the
  sign without re-reading the provenance comment in `board.rs`.
* Calibration constants in `board.rs` are **provisional**, derived from the
  reference firmware's source rather than measured on real hardware. Treat
  them as placeholders until Level 9 of `TESTING.md` has been run.

## Safety-critical conventions

* **`crates/oc-firmware/src/board.rs` is the single, exclusive place allowed
  to name a Teensy pin number.** Every other file reasons in terms of
  `oc-core` types (`CvChannel`, `TriggerChannel`, `Button`, …). This is a
  hard rule, not a preference: a wrong pin direction (configuring an
  input-only analog pad as a GPIO output) is the one way this firmware could
  cause real electrical damage.
* The Teensy 4.0's **HalfKay** bootloader lives in ROM on a separate chip and
  cannot be erased by application firmware — pressing PROGRAM always
  restores it. A bad firmware cannot brick the module or block a future
  upload; `cargo xtask flash`'s validations exist to save time and catch
  wrong-architecture images, not to prevent bricking (which is not a real
  risk here).
* **The firmware drives no UART.** Every LPUART exposed on pins 0–23
  collides with the panel wiring (see the module doc comment at the top of
  `crates/oc-firmware/src/main.rs` for the exact conflicts). There is no
  serial console; the OLED screen is the only diagnostic channel until a USB
  CDC banner is implemented.
* `oc-core` and `oc-drivers` are `forbid(unsafe_code)`. `oc-firmware` is
  `deny(unsafe_code)` and currently contains none of its own; all register
  access is confined to `teensy4-bsp`/`imxrt-hal`. If that ever changes,
  every `unsafe` block must carry a `# Safety` doc section stating the
  invariant it upholds.

## Code style and lints

* Edition 2024 (`rust-toolchain.toml`, `rustfmt.toml`); MSRV `1.85`
  (`[workspace.package] rust-version`).
* Workspace-wide lints in the root `Cargo.toml`:
  `clippy::pedantic` (warn), `missing_debug_implementations` (warn),
  `unreachable_pub` (warn), `rust_2018_idioms` (warn). CI runs
  `cargo clippy --all-targets --all-features -- -D warnings` (host) and the
  same for `oc-firmware` on its real target — both must pass, and both are
  checked separately because of the `default-members` exclusion above.
* `clippy::module_name_repetitions` is allowed workspace-wide (numeric casts
  and DSP/fixed-point naming make it noisy); everything else is reviewed
  case by case with a narrowly scoped `#[allow]` plus a one-line
  justification comment — do not blanket-allow a pedantic lint to make a
  function pass; prefer restructuring (see the `validate_image` history in
  `xtask/src/validate.rs` for the expected shape of that trade-off).
* Doc comments explain **why**, not what; British-leaning English, full
  sentences, no emoji. Every `pub`/`pub(crate)` item needs one.
* `if let ... else` (let-else) and inline format arguments (`format!("{x}")`)
  are the norm; match existing usage rather than the older `match`/positional
  `format!("{}", x)` styles.
* `cargo fmt --all -- --check` must pass; `rustfmt.toml` only pins the
  edition, so default formatting otherwise applies.

## Testing conventions

Full protocol, per level, with exact commands and what each proves: see
[`TESTING.md`](TESTING.md). Highlights relevant to making changes:

* `oc-core` and `oc-drivers` carry the entire behavioural test burden;
  `oc-firmware` intentionally has **no** unit tests of its own — do not add
  any there, add them to `oc-core`/`oc-drivers` and wire the firmware to use
  the tested abstraction instead.
* `Engine::tick` is the one function every backend calls; a new behaviour
  belongs there (or in `DiagnosticApp`), tested once via
  `oc_core::testing::mock_engine`, not duplicated per backend.
* `crates/oc-sim/tests/scenarios/*.scn` are golden, hand-editable regression
  tests with matching `*.screen` snapshots. Never hand-edit a `.screen` file;
  regenerate with `UPDATE_SCREENS=1 cargo test -p oc-sim --test scenarios`
  and diff the result before committing.
* `xtask/tests/validate_image.rs` builds synthetic ELF byte buffers in
  memory; it does not require a firmware build, and `xtask::validate` must
  stay a pure function over bytes (`validate_image(&[u8]) -> Result<...>`)
  reachable from tests via `xtask/src/lib.rs` — do not fold that logic back
  into `main.rs`.
* `criterion` benchmarks (`crates/oc-core/benches/engine.rs`) are a
  regression guard, not a hardware measurement: rendering costs roughly
  three orders of magnitude more than the state update on the host, which is
  why `Engine::set_render_interval` decouples screen redraw from the 1 kHz
  signal path (`RENDER_INTERVAL_TICKS` in `board.rs`).

## Known gaps (do not assume these are done)

* **The VCV Rack 2 module has never been loaded into a running Rack
  instance.** `crates/oc-vcv-ffi` and `vcv/OrnamentCrimeRust` are implemented
  and `vcv/OrnamentCrimeRust` has been built and linked against a real Rack
  SDK, but the panel layout and the knob-as-encoder interaction in
  `Diagnostic.cpp` have not been exercised inside Rack itself. Building it
  requires a separately downloaded [Rack SDK](https://vcvrack.com/downloads)
  (`RACK_DIR`/`--rack-dir`), which is why CI does not build it.
* **Renode smoke test** — dropped as originally specified (no UART to log
  to); alternatives (USB CDC banner, semihosting, SPI-write observation) are
  recorded in the plan file but none is implemented.
* **The firmware has never run on real hardware.** Calibration slopes and
  the OLED controller choice (`SSD1306` vs `--features ssd1309`) are
  unverified; see `TESTING.md` Level 9.
* **`cargo xtask` does not forward extra Cargo `--features`** to its
  internal `build_firmware` call (`xtask/src/cargo.rs`); switching the OLED
  controller for a one-off test currently requires a manual `cargo build`
  outside the `xtask` gate (see `TESTING.md`, Level 9.1).

## Where things live (quick index)

| Looking for…                                   | File |
|--------------------------------------------------|------|
| Platform traits (`AnalogIn`, `Clock`, `Display`…) | `crates/oc-core/src/platform.rs` |
| The main control loop                             | `crates/oc-core/src/engine.rs` |
| The diagnostic applet's logic and rendering       | `crates/oc-core/src/app.rs` |
| ADC/DAC unit conversion and calibration           | `crates/oc-core/src/calibration.rs` |
| Deterministic mocks used by every `oc-core` test  | `crates/oc-core/src/testing.rs` |
| The pinout (the only file allowed to name a pin)  | `crates/oc-firmware/src/board.rs` |
| Memory map / boot sections rationale              | `crates/oc-firmware/MEMORY.md` |
| Pre-flash validation logic                        | `xtask/src/validate.rs` |
| Flash orchestration (confirm, invoke loader)       | `xtask/src/flash.rs` |
| The C ABI linked into VCV Rack                    | `crates/oc-vcv-ffi/src/lib.rs` |
| VCV plugin build/install orchestration            | `xtask/src/vcv.rs` |
| The VCV Rack plugin shim (module, widget)         | `vcv/OrnamentCrimeRust/src/Diagnostic.cpp` |
| Full test protocol                                | `TESTING.md` |
| Full requirements, decisions, delivery status     | `.junie/plans/oc-rust-firmware-foundation.md` |
