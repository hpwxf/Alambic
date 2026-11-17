# AGENTS.md — technical specifics of this repository

This file documents facts about *this* codebase that are easy to get wrong by
analogy with a typical embedded or workspace project. Read it before making
structural changes. User-facing instructions (how to run the simulator, key
maps, flashing) live in `README.md`; the test protocol lives in `TESTING.md`;
the rationale for every architectural decision lives in the ADR journal,
`journal/` (French; index in `journal/README.md`), and the project state in
`PLAN.md`. The original foundation plan,
`.junie/plans/oc-rust-firmware-foundation.md`, is kept as the step-0 plan.
This file is the shortest path to "how does this repository actually work".

**This file is the single source of truth for agents** (Claude Code, Junie,
Codex, …). Tool-specific files such as `CLAUDE.md` are local, untracked, and
only point here plus tool-specific notes; anything that matters for the
repository belongs in this file (journal/D-001).

## Plan and decision journal

**Read `PLAN.md` first**: map of the documents, one-line summary per finished
step, the step **in progress** with its lot checklist, the decisions already
taken. Then read only what the current lot needs: `docs/plan/roadmap.md`
(steps, open questions), `docs/plan/avancement/etape-N.md` (what a finished
step did, measured, corrected), `docs/plan/revue-decisions-2026-11-16.md`
(decisions flagged for review or revision). Update `PLAN.md` before committing
at the end of a lot; when a step ends, write `docs/plan/avancement/etape-N.md`.

**Architectural decisions go in `journal/` as ADRs** (French, one file per
decision, format and index in `journal/README.md`). `D-002`–`D-023` were
reconstructed retrospectively from the git history. From `D-024` on, the
**antériorité rule** applies: the ADR is committed *before or with* the code
it describes, as the first file of that commit. Never start an architectural
change without its entry; if the implementation diverges from a recorded
decision, *revise* that entry (`Statut: adoptée, révisée`, dated section)
rather than opening a new one. ADR numbers are never reassigned. Keep the
index table in `journal/README.md` and the "Décisions actées" table in
`PLAN.md` in sync. An assistant plan that carries an architectural decision
must be tracked by git, not hidden in `.git/info/exclude`.

Language: working docs (`PLAN.md`, `docs/plan/`, `journal/`) in French; code,
comments, commit messages, `README.md`, `AGENTS.md` and `TESTING.md` in
English.

**Definition of done**: a lot is done only when the CI commands pass locally
(journal/D-001, D-017):

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
cargo test -p oc-core                       # single crate's tests
cargo test -p oc-core some_test_name        # single test

cargo run -p oc-sim                         # native simulator (ratatui TUI)
cargo run -p oc-sim -- run --record bug.scn # record a simulator scenario
cargo run -p oc-sim -- replay bug.scn       # replay one headless
UPDATE_SCREENS=1 cargo test -p oc-sim --test scenarios   # regen golden .screen snapshots after an intentional render change
UPDATE_FONTS=1 cargo test -p oc-core --test fonts         # regen the text font tables from embedded-graphics

cargo xtask build                           # cross-compile firmware (thumbv7em-none-eabihf, release)
cargo xtask size                            # section table + linker-layout checklist
cargo xtask hex                             # dist/oc-firmware.hex
cargo xtask build|hex --features ssd1306    # OLED override (default is SH1106); ssd1309 also available
cargo xtask flash --features settings-stress # test firmware: 300 settings saves at power-up (TESTING.md 9.5c), never for a module in use
cargo xtask flash --dry-run                 # validate only, upload nothing
cargo xtask flash                           # validate, show SHA-256 digest, confirm, upload
cargo xtask vcv build --rack-dir <path>     # build the VCV Rack 2 plugin (needs a downloaded Rack SDK)
cargo xtask vcv install --rack-dir <path>   # build + drop into the Rack user plugin folder
cargo xtask vcv clean                       # wipe plugin + oc-vcv-ffi build artefacts
cargo probe stats                           # live profile of a running module (oc-probe; also stream, out, screen, reboot)

cargo bench                                 # criterion benchmarks (crates/oc-core/benches/engine.rs)
```

`cargo fw`, `cargo sim` and `cargo probe` (aliases in `.cargo/config.toml`)
shortcut the firmware build, the simulator run and the probe tool. `cargo xtask` itself is also a cargo
alias (`run --package xtask --release --`).

## What this is

A Rust firmware for the **Ornament & Crime** Eurorack module (TLM Audio
build, **Teensy 4.0 / NXP i.MX RT1062**, Cortex-M7 @ 600 MHz), built as an
engineering foundation rather than a musically rich firmware: cross-compiled
firmware, a native simulator, a VCV Rack 2 module, and a safe flashing tool,
all sharing one behavioural core. Several applets can be hosted and picked
from a front-panel menu, but *musically rich* applets are explicitly out of
scope for now: the two shipped are a diagnostic I/O screen and a scope.

## Workspace layout

| Crate                | `no_std` | `unsafe` policy            | Role |
|-----------------------|----------|-----------------------------|------|
| `crates/oc-core`      | yes      | `forbid(unsafe_code)`       | **All** behaviour: platform traits, `Engine::tick`, the applets (`DiagnosticApp`, `ScopeApp`) and the app menu, button debouncing and chord arbitration, calibration, quadrature decoding, framebuffer. Knows nothing about registers or an OS. |
| `crates/oc-drivers`   | yes      | `forbid(unsafe_code)`       | Peripheral protocols (DAC8565, SH1106/SSD1306/SSD1309, triggers, shared SPI bus) over `embedded-hal` 1.0 only; tested on the host against recording mock buses. |
| `crates/oc-firmware`  | yes (bin)| `deny(unsafe_code)`; `unsafe` only in `flash_store.rs` (boot ROM calls) and `reboot.rs` (bootloader hand-over) | Pure wiring: turns `teensy4-bsp`/`imxrt-hal` resources into the `oc-core` platform traits, runs the 1 kHz loop. The one crate that cannot be tested on the host. |
| `crates/oc-protocol`  | yes      | `forbid(unsafe_code)`       | Messages between the firmware and the host probe (`Request`, `Reply`), `postcard` + COBS framing, `FrameReader`; tested on the host (journal/D-029). |
| `crates/oc-probe`     | no       | ordinary                    | Host tool over the module's probe port (`serialport`, without `libudev`): a `Client` over any byte stream, tested against the real `oc_protocol::server` through an in-memory loopback on a mock engine; CLI `stats`, `stream`, `out`, `screen`, `reboot` (journal/D-029). |
| `crates/oc-sim`       | no       | ordinary                    | Host backend + ratatui TUI + deterministic virtual clock; runs the real `oc-core` engine. |
| `crates/oc-vcv-ffi`   | no       | ordinary (defensive C ABI)  | `staticlib`/`rlib` exposing a defensive C ABI over `oc-core` (`oc_engine_*`), linked into the VCV Rack 2 module. Every function tolerates a null pointer or an out-of-range index and never lets a Rust panic unwind across the boundary (`catch_unwind`). |
| `vcv/OrnamentCrimeAlambic` | n/a (C++) | n/a | The Rack SDK plugin shim: module/port declaration, a widget reading the ABI's framebuffer, no behaviour of its own. Built by `cargo xtask vcv build\|install`, not by hand. |
| `crates/oc-buildinfo` | no       | ordinary                    | Build-script helper only (`[build-dependencies]` of `oc-firmware`, `oc-sim`, `oc-vcv-ffi`): sets `OC_BUILD_ID`, the short commit hash (`-dirty` if tracked files differ), read through `vergen-gix` (no `git` binary needed) plus its own `rerun-if-changed` on the workspace sources, which vergen alone does not watch, each backend hands to `Engine::set_build_id` for the splash. `oc-core` never computes it, so its tests and golden screens stay commit-independent (journal/D-025). |
| `xtask`               | no       | ordinary                    | The only supported build/flash/VCV entry point: cross-compilation, size report, HEX packaging, the pre-flash validation gate (`xtask::validate`), and the `vcv` subcommand (`xtask/src/vcv.rs`). |

`oc-core` is the single source of behavioural truth. The firmware, the
simulator and VCV Rack differ *only* in how they read and write signals;
every actual decision (mode cycling, calibration, debouncing, rendering)
lives in `oc-core` and is tested once, on the host.

## Cross-compilation specifics

* **Target:** `thumbv7em-none-eabihf`. Toolchain pinned by
  `rust-toolchain.toml` (**1.100.0**, bumped deliberately by hand;
  `rust-src` + `llvm-tools` components,
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
  This is a checked invariant, not folklore — `cargo xtask size` runs an
  automated layout checklist (see `MEMORY.md`) and fails if it is broken.
* **`CARGO_TARGET_DIR`/`RUSTFLAGS` caution:** CI deliberately does not set
  `RUSTFLAGS` as an environment variable, because that would *replace*
  `target.thumbv7em-none-eabihf.rustflags` from `.cargo/config.toml` and drop
  `-Tt4link.x`, breaking the firmware link silently. Warnings are denied per
  invocation (`-D warnings` on the command line) instead.
* **LLVM tools, not system binutils:** `xtask/src/llvm.rs` locates
  `llvm-size`/`llvm-objcopy` inside the active Rust sysroot
  (`rustc --print sysroot`), not on `PATH`, so the build stays reproducible
  across machines with different system toolchains.

## The loop and the screen

* **One loop, no separate display loop.** The firmware runs one cooperative
  1 kHz loop (`crates/oc-firmware/src/main.rs`, journal/D-008, D-023 option
  C+). Each tick: sample the inputs (four blocking ADC conversions, the
  trigger edge latches, the panel), then `Engine::tick` (panel routing, the
  applet, the four DAC outputs, then **at most one** screen action), then
  wait for the deadline (previous deadline + 1 ms) while serving USB. The
  signal path runs on every tick; the screen is a side task cut so that no
  tick outlasts the period.
* **Vocabulary.** The *framebuffer* is the screen's copy in RAM (128 x 64,
  1 024 bytes). A *frame* is one whole image. A *page* is one eighth of it,
  a horizontal band 8 pixels high (128 bytes), the unit the SH1106 / SSD130x
  controllers store and receive — not a screen. *Drawing* (`Stage::Render`)
  computes pixels into the framebuffer, CPU only; *sending* (`Stage::Page`,
  `Stage::Present`) copies it to the panel over the shared SPI bus. The panel
  keeps showing what it last received.
* **The screen action of a tick** is one of: nothing; draw a new frame
  (`RENDER_INTERVAL_TICKS` = 20 ticks since the last one, and no page still
  pending); send one page of the drawn frame. A display reports its parts
  through `Display::page_count`: the module's `PagedPanel` answers 8, so a
  frame goes out over the 8 ticks after the drawing; the simulator, Rack and
  the tests keep the default 1 and get the frame in the tick that drew it.
  The boot splash sends whole frames every tick (outputs at rest).
* **Over 20 ticks**, with indicative costs from one module (the profile
  measures them, `cargo probe stats`): tick 0 draws (≈ 0.29 ms sampling +
  ≈ 0.09 ms drawing the diagnostic screen since text is laid as column
  bytes, `oc_core::text`; ≈ 0.65 ms before); ticks 1–8 send pages 0–7
  (≈ 0.145 ms each, now the longest ticks, ≈ 0.46 ms with the sampling);
  ticks 9–19 have no screen work. 50 frames a second, 1 000 ticks a second. While the 8 pages go out,
  the panel shows the new frame on top and the previous one below (≈ 8 ms
  of each 20 ms frame); no page is ever sent half-redrawn.

## Signal representation

* **Internal unit: millivolts, `i32`** (`oc_core::platform::MilliVolts`).
  No floating point in the firmware's signal path; `f32` appears only at the
  VCV Rack boundary (`Alambic::process()` converting to/from Rack's
  volt-scaled `float`, never inside `oc-core` or `oc-vcv-ffi`'s own ABI,
  which stays `int32_t` millivolts). This keeps conversions exact and avoids
  FPU dependence in the critical loop.
* **Backends exchange raw converter codes, not millivolts**
  (`AnalogIn::read_code`, `AnalogOut::write_code`); `Engine` converts them
  with its current `oc_core::calibration::Calibration` (journal/D-027). The
  simulator and the VCV module model the module's converters through the
  mocks in `oc_core::testing`, so inputs are quantised to 12 bits there too;
  tests that compare an input with an output use `testing::adc_reading`.
* **Both the analog input and output stages of this hardware invert.**
  `Calibration::FACTORY` in `crates/oc-core/src/calibration.rs` expresses
  this as a *negative* input slope and a *decreasing* output table (one DAC
  code per octave, −3 V to +7 V, Phazerville's model), enforced by
  `const _: () = { assert!(...) }` checks — a positive slope there is a
  hardware-modelling bug, not a style choice. The factory values are the
  reference firmware's defaults (0 V input at ADC code 2730, 10 V over each
  converter's range); the per-module values come from the calibration wizard
  (journal/D-026, D-027), not from editing constants.

## Safety-critical conventions

* **`crates/oc-firmware/src/board.rs` is the single, exclusive place allowed
  to name a Teensy pin number.** Every other file reasons in terms of
  `oc-core` types (`CvChannel`, `TriggerChannel`, `Button`, …). This is a
  hard rule, not a preference: a wrong pin direction (configuring an
  input-only analog pad as a GPIO output) is the one way this firmware could
  cause real electrical damage. `board::split` consumes the BSP pin set and
  hands out signal groups (`cv`, `triggers`, `spi`, `dac`, `oled`, `panel`,
  `led_sck`) whose `claim` methods apply the direction and pad configuration;
  a new pin goes in the table *and* in a group there. Enforced by
  `xtask/tests/pin_rule.rs`, which fails if any other firmware source names a
  BSP pin token (`P13`, `p13`) outside a comment.
* The Teensy 4.0's **HalfKay** bootloader lives in ROM on a separate chip and
  cannot be erased by application firmware — pressing PROGRAM always
  restores it. A bad firmware cannot brick the module or block a future
  upload; `cargo xtask flash`'s validations exist to save time and catch
  wrong-architecture images, not to prevent bricking (which is not a real
  risk here).
* **The firmware drives no UART.** Every LPUART exposed on pins 0–23
  collides with the panel wiring (see the module doc comment at the top of
  `crates/oc-firmware/src/main.rs` for the exact conflicts). Host-side
  diagnostics use **USB CDC** on the Teensy's native USB device, polled from
  the 1 kHz loop (`crates/oc-firmware/src/usb.rs`): one composite device, two
  serial ports, the text log first and the binary probe protocol second
  (`oc-protocol`, journal/D-029); the OLED remains the on-panel channel. Early
  boot also flashes the onboard LED (pin 13) in stage groups before SPI
  claims that pad — see `crates/oc-firmware/src/boot_led.rs`.
* `oc-core` and `oc-drivers` are `forbid(unsafe_code)`. `oc-firmware` is
  `deny(unsafe_code)`; register access is confined to `teensy4-bsp`/`imxrt-hal`,
  and the two exceptions are `crates/oc-firmware/src/flash_store.rs`, which
  calls the i.MX RT1062 boot ROM's `FlexSPI` NOR driver to erase and program
  the settings sectors (no crate offers it; journal/D-028, D-003), and
  `crates/oc-firmware/src/reboot.rs`, which reads the secure-mode fuse and
  executes Teensyduino's `bkpt #251` to hand over to the bootloader
  (journal/D-029). Each carries a module-level `#![allow(unsafe_code)]` and a
  `SAFETY:` comment on every block. Any new `unsafe` must do the same, in a module of its own.
* **The top 64 KiB of the flash (`0x601F_0000`..) is reserved**: the first
  four 4 KiB sectors hold the settings (`board::SETTINGS_FLASH_OFFSET`), the
  rest is Teensyduino's EEPROM emulation and, in the last sector, the restore
  program. `xtask::validate` rejects an image that reaches it
  (`RESERVED_FLASH_START`); never move the settings into the last sector.

## Code style and lints

* Edition 2024 (`rust-toolchain.toml`, `rustfmt.toml`); MSRV `1.100`
  (`[workspace.package] rust-version`), equal to the pinned toolchain so
  every CI job tests it (journal/D-017).
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
  belongs there (or in an applet reached through `AppHost`), tested once via
  `oc_core::testing::mock_engine`, not duplicated per backend. Holding
  `up`+`down` opens the app menu; because of that, `up` and `down` fire their
  own action on release, arbitrated in `crates/oc-core/src/buttons.rs`.
  No backend reports whether a cable is plugged in (the module cannot
  know); an applet that needs a default for an unused input asks
  `oc_core::presence` (`CvPresence`, `TriggerPresence::latch|clock`), which
  derives it from the levels alone — never add a backend-specific source
  such as Rack's `isConnected()` (journal/D-019, D-024).
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
  signal path (`RENDER_INTERVAL_TICKS` in `board.rs`). The hardware
  measurement is the engine's own profile (`oc_core::profile`, journal/D-029):
  the engine times its stages through `Clock::fine_count`, which the
  firmware backs with the core's cycle counter and the virtual clock with
  its own time, read without advancing it, so profiling never changes a
  test or a golden screen.

## Known gaps (do not assume these are done)

* **The VCV Rack 2 module has not been checked systematically inside Rack.**
  It builds and links against a real Rack SDK and loads in Rack (it showed in
  Rack's module browser under its old name, see `8bdd9bc`), but there is no
  recorded check of the panel layout, the knob-as-encoder interaction or the
  "Open/close app menu" context entry in `Alambic.cpp`. Building it
  requires a separately downloaded [Rack SDK](https://vcvrack.com/downloads)
  (`RACK_DIR`/`--rack-dir`), which is why CI does not build it.
* **Renode smoke test** — dropped as originally specified (no UART to log
  to). USB CDC logging is now wired in `oc-firmware` for real hardware; a
  Renode observation path (semihosting, SPI-write watch) is still not
  implemented.
* **The firmware has been brought up on a real module, mostly verified.**
  Checked on the TLM build: the SH1106 OLED, the USB log and probe ports, the
  boot LED, the encoders and the buttons (`3f50a42`, `a8f3560`), the
  calibration and settings flash (step 4), and with the probe the CV inputs,
  the CV outputs (multimeter), the trigger inputs and the output-to-input
  loopback (`docs/plan/avancement/etape-3.md`). **Not** checked: the DAC8565
  simultaneous update. Known limits measured there: the inputs read within
  about 20 mV only (one gain shared by the four inputs); and
  trigger pulses shorter than about 3 ms were missed (39 % of 0.94 ms
  pulses) while triggers were only sampled: the GPIO edge detector now
  latches them (journal/D-019, lot 3c1; 960 of 960 such pulses counted on the
  module, none at rest). OLED overrides for other builds go through the
  normal gate: `cargo xtask build|size|hex|flash --features ssd1306` (or
  `ssd1309`).

## Where things live (quick index)

| Looking for…                                   | File |
|--------------------------------------------------|------|
| Platform traits (`AnalogIn`, `Clock`, `Display`…) | `crates/oc-core/src/platform.rs` |
| The main control loop                             | `crates/oc-core/src/engine.rs` |
| The diagnostic applet's logic and rendering       | `crates/oc-core/src/apps/diagnostic.rs` |
| The scope applet                                  | `crates/oc-core/src/apps/scope.rs` |
| The app registry and dispatch (`AppId`, `AppHost`) | `crates/oc-core/src/apps/mod.rs` |
| The app menu (`up`+`down`) and its rendering      | `crates/oc-core/src/menu.rs` |
| The system settings screen (`SETUP`, boot gestures) | `crates/oc-core/src/system.rs` (routing in `engine.rs`) |
| The calibration wizard (Phazerville's steps)      | `crates/oc-core/src/wizard.rs` |
| The Rack stand-in for the `up`+`down` chord       | `vcv/OrnamentCrimeAlambic/src/Alambic.cpp` (`appendContextMenu`) |
| Button debouncing and the `up`+`down` chord       | `crates/oc-core/src/buttons.rs` |
| The boot splash screen (name/version, progressive border) | `crates/oc-core/src/splash.rs` |
| Text drawing (fonts as column bytes, the font in the style; same pixels as `embedded-graphics`) | `crates/oc-core/src/text/mod.rs`; generated fonts `text/fonts.rs` (`UPDATE_FONTS=1 cargo test -p oc-core --test fonts`) |
| ADC/DAC unit conversion and calibration           | `crates/oc-core/src/calibration.rs` |
| Input presence derived from the signal (defaults for unused inputs) | `crates/oc-core/src/presence.rs` |
| Where the loop's time goes (stages, period jitter; the module's cycle counter) | `crates/oc-core/src/profile.rs` (firmware log lines: `crates/oc-firmware/src/profile_log.rs`) |
| The probe protocol (messages, COBS framing) and the module's side of it | `crates/oc-protocol/src/lib.rs`, `crates/oc-protocol/src/server.rs` |
| The two USB serial ports and the log backend | `crates/oc-firmware/src/usb.rs`, `crates/oc-firmware/src/usb_log.rs` |
| Persistent settings, their format and the flash store | `crates/oc-core/src/settings.rs` (`MemoryFlash` in `testing.rs`) |
| The Teensy's settings flash (boot ROM calls, `unsafe`) | `crates/oc-firmware/src/flash_store.rs` |
| Deterministic mocks used by every `oc-core` test  | `crates/oc-core/src/testing.rs` |
| The pinout (the only file allowed to name a pin)  | `crates/oc-firmware/src/board.rs` (rule test: `xtask/tests/pin_rule.rs`) |
| Memory map / boot sections rationale              | `crates/oc-firmware/MEMORY.md` |
| Pre-flash validation logic                        | `xtask/src/validate.rs` |
| Flash orchestration (confirm, invoke loader)       | `xtask/src/flash.rs` |
| The C ABI linked into VCV Rack                    | `crates/oc-vcv-ffi/src/lib.rs` |
| VCV plugin build/install orchestration            | `xtask/src/vcv.rs` |
| The VCV Rack plugin shim (module, widget)         | `vcv/OrnamentCrimeAlambic/src/Alambic.cpp` |
| Full test protocol                                | `TESTING.md` |
| Release notes; version rule (Rack plugin = `2.<minor>.<patch>`, checked by `xtask/tests/versions.rs`) | `CHANGELOG.md` |
| Project state, steps, decisions taken             | `PLAN.md` |
| Architectural decisions (ADRs, French)            | `journal/` (index in `journal/README.md`) |
| Step-0 foundation plan (requirements, delivery)   | `.junie/plans/oc-rust-firmware-foundation.md` |
