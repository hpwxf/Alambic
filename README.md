# Alambic — Ornament & Crime firmware in Rust

A Rust firmware for the [Ornament & Crime](https://ornament-and-cri.me/) Eurorack
module (TLM Audio build, **Teensy 4.0 / NXP i.MX RT1062**), designed to be more
approachable than the reference *Phazerville* firmware.

The distinguishing goal is not musical richness but **verifiability**: the same
behaviour runs on hardware, in a native simulator, and inside VCV Rack 2.

> Status: the cross-compilation chain, the shared core, the simulator, the
> Teensy firmware, the safe flashing tool and the VCV Rack 2 module are in
> place, with host tests throughout. The VCV Rack plugin builds and links
> against the real Rack SDK but has not yet been opened inside a running Rack
> instance; the Renode boot smoke test is not written; and the firmware has
> **not** been run on real hardware — see *Before the first flash* below.

## Layout

| Path                 | Role                                                              |
|----------------------|-------------------------------------------------------------------|
| `crates/oc-core`     | `no_std`, `forbid(unsafe_code)` core: platform traits, engine, UI  |
| `crates/oc-firmware` | Teensy 4.0 binary; the only crate that touches registers           |
| `crates/oc-sim`      | Native backend and terminal UI, with a deterministic virtual clock |
| `crates/oc-vcv-ffi`  | `staticlib` exposing a C ABI to the VCV Rack 2 module              |
| `vcv/OrnamentCrimeAlambic` | The Rack SDK plugin shim: module declaration and widget only, no behaviour |
| `xtask`              | Build, packaging, flashing and VCV plugin automation                |

`oc-core` holds **all** behaviour. The three backends only differ in how they
read and write signals, which is what makes the simulator a meaningful proxy
for the hardware.

## Apps and the app menu

The module runs one app at a time and picks between them from the front panel.
**Holding `up` and `down` together** opens the app menu; `up`/`down` or the left
encoder move the highlight, either encoder press launches the highlighted app,
and holding both again closes the menu without changing anything. The running
app keeps tracking its inputs and driving its outputs while the menu is up, so
opening it never interrupts a patch. In the simulator press `m`; in VCV Rack
the chord is unreachable with a mouse, so right-click the module and choose
*Open/close app menu*.

| App          | What it does                                             |
|--------------|----------------------------------------------------------|
| `DIAGNOSTIC` | the I/O diagnostic screen the module boots into          |
| `SCOPE`      | a scrolling view of `CV1`, buffered to all four outputs  |

Because holding both buttons is a deliberate gesture, `up` and `down` fire
their own action on **release**, and not at all when the chord formed while they
were down. That is the only way the first of the two presses cannot act before
the chord is even detectable.

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
| up / down            | next / previous output mode, on release     |
| up + down together   | open the app menu                          |

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
braille characters by default (2x4 dots per cell), so every pixel stays
individually visible and almost every terminal font can render it; the
"module screen" panel keeps this fixed 128x64 size (64x16 characters)
regardless of how wide the terminal is. If your font leaves visible
horizontal banding between braille rows, rebuild with
`cargo run -p oc-sim --features octant` to use denser Unicode octant glyphs
instead (same resolution; needs a font that covers the Symbols for Legacy
Computing Supplement — see below). Like the firmware and the VCV Rack module,
the simulator boots into the boot splash screen (name, version and a border
tracing itself around the screen) before the diagnostic applet takes over,
and `r` replays that same boot sequence at any time, exactly like a host's
"Initialize" action.

**Fonts that render octants well** (for `--features octant`): the block is
Unicode 16.0 *Symbols for Legacy Computing Supplement* (around U+1CD00).
SauceCodePro Nerd Font / Source Code Pro usually do **not** include it yet.
Good bets as of 2025–2026:

* [JuliaMono](https://juliamono.netlify.app/) — deliberately wide Unicode;
* [Cascadia Code](https://github.com/microsoft/cascadia-code) (recent builds);
* [Noto Sans Mono](https://fonts.google.com/noto/specimen/Noto+Sans+Mono) /
  Noto Sans Symbols 2 as a fallback face if your terminal can stack fonts;
* Some [Nerd Font](https://www.nerdfonts.com/) like [CaskaydiaMono Nerd font](https://github.com/ryanoasis/nerd-fonts/releases/download/v3.4.0/CascadiaMono.zip) 
  patched builds pick up octants when the upstream face has them.

Quick check in the terminal (should look like a solid 2×4 block, not `?`/`�`):

```text
echo -e '\U1cd00\U1cd01\U1cd02\U1cd03  \U1cd3f'
```

The keyboard has two selectable layouts, **AZERTY** (default) and
**QWERTY**, toggled with `l`; a permanent "help" panel below "outputs"
always shows the key map, key and meaning aligned in columns, for whichever
layout is active. They differ only on trigger 1 and the left encoder's
turn: pressing the physical key that types `w` on QWERTY types `z` on
AZERTY (and vice versa), so those bindings swap between the two:

| Action                              | AZERTY | QWERTY |
|---------------------------------------|:------:|:------:|
| pulse / hold trigger 1                | `w` / `W` | `z` / `Z` |
| turn the left encoder anticlockwise (`Shift` for ten detents) | `z` / `Z` | `w` / `W` |

Everything else is the same on both layouts. Both encoders live on six
consecutive keys of the top letter row, skipping only quit (`q`) and patch
(`p`): press–turn–turn for the left encoder, turn–turn–press for the right
one:

| Key            | Action                                            |
|----------------|---------------------------------------------------|
| `Tab`          | select which CV input the arrows drive            |
| `←` / `→`      | selected CV input by ±100 mV (`Shift` for ±1 V)   |
| `Home`         | selected CV input to 0 V                          |
| `p`            | toggle the cable on the selected CV input         |
| `x` `c` `v`    | pulse triggers 2 to 4                             |
| `X` `C` `V`    | hold triggers 2 to 4 high or low                  |
| `↑` / `↓`      | the module's up / down buttons                    |
| `m`            | press up and down together: the app menu          |
| `Shift+↑` / `Shift+↓` | hold up / down until pressed again (see below) |
| `a`            | press the left encoder                            |
| `e`            | turn the left encoder clockwise (`Shift` for ten detents) |
| `r` / `t`      | turn the right encoder anticlockwise / clockwise (`Shift` for ten detents) |
| `y`            | press the right encoder                           |
| `1` `2` `3`    | paused, real time, 50x                            |
| `Space`        | run a single tick while paused                    |
| `o` / `0`      | reset the module (replays the boot splash screen) |
| `l`            | switch between AZERTY and QWERTY                  |
| `q` / `Esc`    | quit                                              |

A plain `↑`/`↓`/`z x c v` press is only held for a handful of ticks — long
enough to register as a clean edge, too short for two separate keystrokes to
reliably overlap. That is enough for the module's up/down and trigger
reactions on their own, but **up and down held together** opens the app menu,
which needs a real overlap. `m` presses both in the same tick, which is the
one-keystroke way to do it; to hold them by hand instead, press `Shift+↑` then
`Shift+↓` (in either order), and `Shift+↑`/`Shift+↓` again to release them.

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
cargo xtask size        # selective section table + layout checklist
cargo xtask hex         # dist/oc-firmware.hex

# OLED controller override (stock O&C is SH1106):
cargo xtask build --features ssd1306
cargo xtask hex -F ssd1309
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

## VCV Rack 2 module

`crates/oc-vcv-ffi` builds `oc-core` into a `staticlib` behind a small,
defensive C ABI (every function tolerates a null pointer or an out-of-range
index, and no Rust panic is allowed to unwind across the boundary). The C++
side, `vcv/OrnamentCrimeAlambic`, is a thin shim required by the Rack SDK: module
declaration, port/param mapping, and a widget that reads the framebuffer —
**no behaviour lives there**. The whole point is that the module inside VCV
Rack runs the exact same `oc-core` engine as the firmware and the simulator,
right down to the boot sequence: a freshly constructed engine (`Alambic()`)
and Rack's own "Initialize" action (`Alambic::onReset`, wired to
`oc_engine_reset`) both show the same splash screen — the module's name and
version centred on the screen, with a one-pixel border tracing itself around
the edge — before the diagnostic screen takes over.

Building it needs a [Rack SDK](https://vcvrack.com/downloads) (not the full
Rack source) matching your platform, extracted anywhere:

```sh
cargo xtask vcv build --rack-dir /path/to/Rack-SDK     # or set $RACK_DIR
cargo xtask vcv install --rack-dir /path/to/Rack-SDK   # build + drop into your Rack user folder
cargo xtask vcv clean                                  # wipe plugin + oc-vcv-ffi artefacts
```

`build` and `install` rebuild `oc-vcv-ffi`, regenerate its C header with
`cbindgen`, and copy it next to the plugin sources before invoking the Rack
SDK's own `Makefile` — a plain `make` in `vcv/OrnamentCrimeAlambic` never sees a
stale header or a staticlib built for the wrong profile. `vcv install`
finishes by running the SDK's own `install` target, which already knows the
correct plugin folder for the current OS. If a C++ rebuild looks polluted by
older objects or a previously linked `plugin.*`, run `vcv clean` first: it
removes the plugin's `build`/`dep`/`dist` trees, the linked binary, the
copied header, and Cargo's host artefacts for `oc-vcv-ffi`, without needing
a Rack SDK path.

**Opening the app menu in Rack.** The module's buttons are Rack's momentary
push buttons, and a mouse has one pointer, so the hardware's up + down chord
cannot be played on the panel. Right-click the module and choose
**Open/close app menu** instead; it holds both buttons down for ten engine
ticks, which is the same gesture the engine sees from two thumbs. Everything
after that — moving the highlight, launching — works from the panel as usual.

The plugin has been built and linked successfully against a real Rack SDK
(2.6.x) during development, producing a loadable `.vcvplugin`; it has not yet
been exercised inside a running VCV Rack instance — treat the panel layout
and the encoder-emulation-via-knob interaction as a first pass to refine once
it is.

## Testing

Every verification level — static checks, host unit and property tests,
criterion benchmarks, simulator scenarios, the pre-flash image gate, and the
step-by-step **manual hardware validation checklist** — is documented with
its exact command and what it does (and does not) prove in
[`TESTING.md`](TESTING.md). Start there before touching a real module.

## Before the first flash

The firmware compiles, links and passes every host test, but it has not yet been
run on a real module. Two things are derived from the reference firmware's source
rather than measured, and should be checked with the diagnostic applet before
being trusted:

* **the calibration slopes.** Both the input and the output stage invert, which
  `crates/oc-firmware/src/board.rs` expresses as negative slopes, but the exact
  gain and offset are per-unit properties.
* **the OLED controller.** SH1106 is assumed (stock O&C); build or flash with
  `cargo xtask … --features ssd1306` or `--features ssd1309` if the screen stays blank.

The pinout itself is in one reviewed table in `crates/oc-firmware/src/board.rs`,
which is the only file allowed to name a pin. Note that the firmware drives
**no** UART: every LPUART available on pins 0-23 collides with the panel. Boot
diagnostics go out over **USB CDC** instead (a virtual serial port on the
Teensy's native USB). After flashing with the cable still plugged in:

```sh
# macOS — the device usually appears as cu.usbmodem*
ls /dev/cu.usbmodem* 2>/dev/null
screen /dev/cu.usbmodem* 115200
```

Expect lines such as `oc-firmware … starting (oled=…)`, `oled init ok|failed`,
and a once-per-second `tick=…` heartbeat. Early boot also blinks the onboard
LED (pin 13) in stage groups **before** SPI claims that pad: 1 = `main`
reached, 2 = ADC mapped, 3 = triggers mapped / about to take SPI. A Morse SOS
(9 flashes) is still the panic handler, not a boot stage.

Before connecting the real module, follow the **Level 9 — Manual hardware
validation** checklist in [`TESTING.md`](TESTING.md#level-9--manual-hardware-validation):
power-up and screen legibility, per-channel CV input checks, trigger checks,
the calibration-slope measurement procedure, and encoder/button checks, in
that order.

## Hardware safety

The Teensy 4.0 carries the **HalfKay** bootloader in ROM on a separate chip.
Application firmware cannot erase it, so a bad image can neither destroy the
module nor prevent a later upload: pressing PROGRAM always restores bootloader
mode. The residual risks are electrical (driving a pin that is wired as an
input, out-of-range voltages) and are addressed by a single reviewed pinout
table in `crates/oc-firmware/src/board.rs`, not by the upload tool.

## Licence

Dual-licensed under MIT or Apache-2.0, at your option.
