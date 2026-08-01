# Alambic — Ornament & Crime firmware in Rust

A Rust firmware for the [Ornament & Crime](https://ornament-and-cri.me/) Eurorack
module (TLM Audio build, **Teensy 4.0 / NXP i.MX RT1062**), designed to be more
approachable than the reference *Phazerville* firmware.

The distinguishing goal is not musical richness but **verifiability**: the same
behaviour runs on hardware, in a native simulator, and inside VCV Rack 2.

> Status: the cross-compilation chain, the shared core, the simulator, the
> Teensy firmware and the safe flashing tool are in place, with 177 host tests.
> The VCV Rack 2 module and the Renode boot smoke test are not written yet, and
> the firmware has **not** been run on real hardware — see *Before the first
> flash* below.

## Layout

| Path                 | Role                                                              |
|----------------------|-------------------------------------------------------------------|
| `crates/oc-core`     | `no_std`, `forbid(unsafe_code)` core: platform traits, engine, UI  |
| `crates/oc-firmware` | Teensy 4.0 binary; the only crate that touches registers           |
| `crates/oc-sim`      | Native backend and terminal UI, with a deterministic virtual clock |
| `crates/oc-vcv-ffi`  | `staticlib` exposing a C ABI to the VCV Rack 2 module              |
| `xtask`              | Build, packaging and flashing automation                           |

`oc-core` holds **all** behaviour. The three backends only differ in how they
read and write signals, which is what makes the simulator a meaningful proxy
for the hardware.

## The diagnostic applet

The first milestone's application is deliberately trivial musically: it makes
every input and output observable so that a freshly flashed module can be
validated from the front panel alone.

| Control              | Effect                                     |
|----------------------|--------------------------------------------|
| left encoder, turn   | select the channel driven by the offset    |
| left encoder, press  | reset the trigger counters                 |
| right encoder, turn  | change the offset by 100 mV per detent     |
| right encoder, press | set the offset back to 0 V                 |
| up / down            | next / previous output mode                |

Output modes:

* **OFFS** — the selected channel emits the offset, the other three mirror the
  matching CV input, exercising the input and output paths at once;
* **RAMP** — a two-second saw across the whole output range, each channel
  shifted by a quarter period;
* **ZERO** — all outputs pinned at 0 V, to measure the output offset error.

The screen shows, per channel, the measured level in volts, whether a cable is
reported (`P`), whether a signal is moving (`~`), the gate level and the number
of trigger edges counted, plus the tick duration and count.

## The simulator

`cargo run -p oc-sim` opens a terminal interface running the real `oc-core`
engine against an in-memory platform. The module's 128x64 screen is drawn with
braille characters, so every pixel stays individually visible.

| Key            | Action                                            |
|----------------|---------------------------------------------------|
| `Tab`          | select which CV input the arrows drive            |
| `←` / `→`      | selected CV input by ±100 mV (`Shift` for ±1 V)   |
| `Home`         | selected CV input to 0 V                          |
| `p`            | toggle the cable on the selected CV input         |
| `z` `x` `c` `v`| pulse triggers 1 to 4                             |
| `Z` `X` `C` `V`| hold triggers 1 to 4 high or low                  |
| `[` / `]`      | left encoder anticlockwise / clockwise            |
| `,` / `.`      | right encoder (`<` / `>` for ten detents)         |
| `Enter` / `b`  | press the left / right encoder                    |
| `↑` / `↓`      | the module's up / down buttons                    |
| `1` `2` `3`    | paused, real time, 50x                            |
| `Space`        | run a single tick while paused                    |
| `?`            | show the key map                                  |
| `q` / `Esc`    | quit                                              |

### Scenarios

Inputs can be recorded and replayed, which turns a bug found by hand into a
committed regression test:

```sh
cargo run -p oc-sim -- run --record bug.scn   # play, then quit
cargo run -p oc-sim -- replay bug.scn         # headless, prints the final state
```

The format is plain text and meant to be edited and reviewed:

```text
ticks 40
0  cv 2 -2500
0  patch 2 on
10 trigger 1 high
20 encoder 2 +15
```

`crates/oc-sim/tests/scenarios/` holds the committed scenarios, each with a
golden snapshot of the resulting screen. After an intentional change to the
rendering, regenerate them with:

```sh
UPDATE_SCREENS=1 cargo test -p oc-sim --test scenarios
```

## Performance

Measured on the host with `cargo bench`, for orientation only — the target is a
600 MHz Cortex-M7 with the code in ITCM:

| Benchmark        | Time     |
|------------------|----------|
| `applet/update`  | ~12 ns   |
| `applet/render`  | ~18 µs   |
| `engine/tick`    | ~17 µs   |

Screen rendering dominates by three orders of magnitude, and an OLED panel
cannot show more than about 60 frames per second anyway, so the redraw rate is
decoupled from the control loop through `Engine::set_render_interval`. The CV
outputs are refreshed on every tick regardless.

## Requirements

The toolchain is pinned by `rust-toolchain.toml`; `rustup` installs everything
on first build, including the `thumbv7em-none-eabihf` target and `llvm-tools`.

## Everyday commands

```sh
cargo test              # host crates only (see below)
cargo clippy --all-targets -- -D warnings
cargo run -p oc-sim     # simulator

cargo xtask build       # cross-compile the firmware (release)
cargo xtask size        # section-by-section footprint
cargo xtask hex         # dist/oc-firmware.hex
```

### Why the firmware is not in `default-members`

`oc-firmware` only compiles for `thumbv7em-none-eabihf`. It is a workspace
member but is excluded from `default-members`, so a bare `cargo build`,
`cargo test` or `cargo clippy` operates on host crates only and never tries to
build ARM code for the host. Build it explicitly:

```sh
cargo xtask build
# or
cargo build -p oc-firmware --target thumbv7em-none-eabihf --release
```

## Flashing the module

```sh
cargo xtask flash --dry-run    # validate everything, upload nothing
cargo xtask flash              # validate, show the digest, ask, then upload
cargo xtask flash --yes        # no confirmation prompt (for CI)
```

The upload itself is delegated to `teensy_loader_cli` (`brew install
teensy_loader_cli` on macOS), which speaks the proven HalfKay HID protocol.
What `xtask` adds is the validation that tool lacks; if any check fails it
reports **all** the problems, exits non-zero, and never invokes the uploader:

| Check                                      | On failure |
|--------------------------------------------|------------|
| ELF, 32-bit, little-endian, `ET_EXEC`      | abort      |
| machine is `EM_ARM`                        | abort      |
| loadable size within 1984 KiB              | abort      |
| FlexSPI Configuration Block at `0x6000_0000` | abort    |
| reset vector and stack pointer in valid memory | abort  |
| built for `thumbv7em-none-eabihf`          | abort      |
| Teensy detected                            | warning    |
| SHA-256 shown, confirmation requested      | `--yes` skips |

A sample run against the current firmware:

```text
loadable size: 36400 bytes
entry: 0x60001031
initial SP: 0x20004000
reset handler: 0x60001031
elf sha256: 9566627c35083fb8...
```

## Memory layout and boot

The memory map and the i.MX RT boot sections (FlexSPI Configuration Block,
Image Vector Table) are generated by `teensy4-bsp`, not hand-written. See
[`crates/oc-firmware/MEMORY.md`](crates/oc-firmware/MEMORY.md), which also
documents why `flip-link` is not used and how stack-overflow protection is
achieved instead.

## Before the first flash

The firmware compiles, links and passes every host test, but it has not yet been
run on a real module. Two things are derived from the reference firmware's source
rather than measured, and should be checked with the diagnostic applet before
being trusted:

* **the calibration slopes.** Both the input and the output stage invert, which
  `crates/oc-firmware/src/board.rs` expresses as negative slopes, but the exact
  gain and offset are per-unit properties.
* **the OLED controller.** SSD1306 is assumed; build with `--features ssd1309`
  if the screen stays blank.

The pinout itself is in one reviewed table in `crates/oc-firmware/src/board.rs`,
which is the only file allowed to name a pin. Note that the firmware drives
**no** UART: every LPUART available on pins 0-23 collides with the panel, so the
boot banner belongs on USB and is not wired up yet.

## Hardware safety

The Teensy 4.0 carries the **HalfKay** bootloader in ROM on a separate chip.
Application firmware cannot erase it, so a bad image can neither destroy the
module nor prevent a later upload: pressing PROGRAM always restores bootloader
mode. The residual risks are electrical (driving a pin that is wired as an
input, out-of-range voltages) and are addressed by a single reviewed pinout
table in `crates/oc-firmware/src/board.rs`, not by the upload tool.

## Licence

Dual-licensed under MIT or Apache-2.0, at your option.
