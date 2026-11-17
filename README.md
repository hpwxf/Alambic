# Alambic — Ornament & Crime firmware in Rust

A Rust firmware for the [Ornament & Crime](https://ornament-and-cri.me/) Eurorack
module (TLM Audio build, **Teensy 4.0 / NXP i.MX RT1062**), designed to be more
approachable than the reference *Phazerville* firmware.

The distinguishing goal is not musical richness but **verifiability**: the same
behaviour runs on hardware, in a native simulator, and inside VCV Rack 2.

> Status: the cross-compilation chain, the shared core, the simulator, the
> Teensy firmware, the safe flashing tool and the VCV Rack 2 module are in
> place, with host tests throughout. The firmware has been brought up on a
> real module (TLM build): the SH1106 OLED, the USB CDC boot log, the encoders
> and the buttons work, but the CV inputs, the CV outputs and their
> calibration, and the trigger inputs have **not** been verified yet — see
> *Before the first flash* below. The VCV Rack plugin builds against the real
> Rack SDK and loads in Rack, but its panel and controls have not been checked
> systematically there. The Renode boot smoke test is not written.

## Layout

| Path                 | Role                                                              |
|----------------------|-------------------------------------------------------------------|
| `crates/oc-core`     | `no_std`, `forbid(unsafe_code)` core: platform traits, engine, UI  |
| `crates/oc-firmware` | Teensy 4.0 binary; the only crate that touches registers           |
| `crates/oc-protocol` | `no_std` messages and server of the live probe (postcard over COBS) |
| `crates/oc-probe`    | Host tool talking to a running module over its probe port          |
| `crates/oc-image`    | Pre-flash image gate; the flash contents of a validated ELF or HEX |
| `crates/oc-halfkay`  | The Teensy bootloader's write protocol, as reports to send          |
| `crates/oc-web`, `web/` | Browser probe and flasher: WebAssembly core and a static page   |
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
| `GFX DEMO`   | the graphics primitives on five pages (fonts, knobs, curves, sequencer, meters); left encoder turns the pages, right encoder the value, `up`/`down` the focused knob; outputs A–D carry the simulated LFO, gate, knob and needle |

Because holding both buttons is a deliberate gesture, `up` and `down` fire
their own action on **release**, and not at all when the chord formed while they
were down. That is the only way the first of the two presses cannot act before
the chord is even detectable.

## System settings

The last entry of the app menu, `SETUP`, opens the system settings: the app
the module starts in, the screen contrast, a 180° rotation for a module
mounted upside down, a horizontal picture shift, the direction of each
encoder, and the calibration wizard (`CALIBRATE`, below). `up`/`down` or
the left encoder move between lines, the right encoder changes the
highlighted value, and pressing the right encoder runs `SAVE` or `EXIT`.
Changes show at once; `SAVE` keeps and stores them (the screen confirms with
`SETTINGS SAVED`, or `SAVE FAILED!`), `EXIT` or `up` + `down` leaves and puts
back what was in force. The app is paused and the outputs sit
at 0 V while the screen is open. The screen always reads the encoders as
wired, whatever their direction setting.

Two gestures work at power-up, held until the splash screen ends (it recalls
them at the bottom):

| Held through the splash | Opens                                                         |
|-------------------------|---------------------------------------------------------------|
| left encoder switch     | the calibration wizard                                        |
| `up` + `down`           | the settings in **safe mode**: screen and encoders at their defaults until saved, to recover from a setting that made the module hard to use |

### Calibration wizard

The steps are those of the Phazerville firmware, so they will be familiar.
`up`/`down` go to the previous/next step, the right encoder adjusts or
chooses, pressing it toggles coarse steps on a DAC point or captures on an ADC
step; `up` + `down` abandons the wizard.

1. **Start fresh?** `YES` starts from the factory calibration.
2. **DAC A to D**, two points each: every output is driven to −3 V, then to
   +5 V; with a multimeter on the output being calibrated (red lead on the
   tip, COM on the sleeve), turn the right encoder until it reads the target.
   Leaving the +5 V point redraws that output's table through the two points.
   Starting fresh, A's table is copied to B, C and D as a starting point.
3. **ADC zero**: unplug every CV input and press the right encoder.
4. **ADC gain**: patch any output into `CV1`; the outputs go to +1 V, then
   +3 V, press at each. The gain applies to all four inputs.
5. **Save values?** `YES` keeps and stores the calibration, `NO` restores the
   previous one.

A capture far from the factory values (an input left patched, nothing in
`CV1`) is refused with a message instead of stored.

The simulator keeps the settings in a file (`--settings`, see below), VCV
Rack in the patch, the module in four sectors at the top of its own flash
(`0x601F_0000`, outside the firmware image, kept across firmware uploads). A
save takes a fraction of a second at most. That area is also where the
Teensyduino EEPROM emulation lives: a module that ran a firmware storing its
settings there (Phazerville, for one) starts here on the defaults, and finds
its old settings gone if it goes back to that firmware.

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

The screen shows, per channel, the measured level in volts, whether the input
is present (`P`) or absent (`.`), whether a signal is moving (`~`), the gate
level and the number of trigger edges counted, plus the tick duration and
count. Presence is *derived from the signal*, not from a cable: the module's
jacks cannot detect one, so an input is present when it has moved or sat more
than 50 mV away from 0 V within the last 300 ms, identically on the module,
in the simulator and in Rack (journal/D-024). A cable carrying a steady 0 V
therefore reads as absent.

### Build identifier

Under the version, every backend's splash screen shows which build is
running: the short commit hash, suffixed `-dirty` when a tracked file differed
from that commit at build time (untracked files do not count). It is computed
by `crates/oc-buildinfo` from each backend's build script, through
[vergen-gix](https://crates.io/crates/vergen-gix) (pure Rust, no `git` binary
needed), and is empty when building outside a git checkout. A source edit
shows as `-dirty` at the next build; a documentation-only edit, from the next
`git add` or commit. Set `OC_BUILD_ID` in the environment to impose a value
instead (journal/D-025).

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
the simulator boots into the boot splash screen (name, version, build
identifier and a border tracing itself around the screen) before the diagnostic applet takes over,
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
consecutive keys of the top letter row, skipping quit (`q`) and `p`:
press–turn–turn for the left encoder, turn–turn–press for the right
one:

| Key            | Action                                            |
|----------------|---------------------------------------------------|
| `Tab`          | select which CV input the arrows drive            |
| `←` / `→`      | selected CV input by ±100 mV (`Shift` for ±1 V)   |
| `Home`         | selected CV input to 0 V                          |
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

### Settings

The module's settings (calibration, startup applet, screen and encoder
preferences) can be kept between sessions in a file, a raw image of the
16 KiB settings flash the module itself uses, managed by the same store:

```sh
cargo run -p oc-sim -- run --settings module.bin   # created on first save
```

Without `--settings` the simulator starts from the defaults every time, which
is what the scenarios rely on. In VCV Rack the settings travel with the patch.

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

On the module itself, the engine times every stage of the loop on the core's
cycle counter (journal/D-029): the USB log prints it once a second, and the
probe gives it as a table:

```sh
cargo probe stats              # mean and longest time of each stage, loop jitter
cargo probe stats --fresh 10   # the same over the next ten seconds only
cargo probe stream --every 10  # inputs, codes, outputs and triggers every 10 ticks
cargo probe out A -2000        # hold output A at -2 V until Enter, printing the inputs
cargo probe screen --png s.png # capture the screen as the module shows it
cargo probe reboot             # restart into the bootloader, then `cargo xtask flash`
```

`cargo probe` is an alias of `cargo run -q -p oc-probe --`. The tool finds the
module's probe port by itself (`--port` to name it); it is the second of the
module's two USB serial ports, the first being the text log. Closing the tool
ends the session, which releases every held output.

### In the browser

The same probe, and a flasher, run in a web page (journal/D-031), for anyone
without the Rust toolchain:

```sh
cargo xtask web                              # builds dist/web/
python3 -m http.server -d dist/web 8000      # then open http://localhost:8000
```

It needs Chrome or Edge on a desktop (Web Serial and WebHID; neither Firefox
nor Safari has them) and a secure context: `localhost` or https. The probe
side does what `cargo probe` does: screen capture (live too), profile, sample
stream, held outputs. The flash side takes the firmware **ELF**, with the
same checks as `cargo xtask flash`, or an Intel **HEX** (the CI artefact,
say) with reduced checks ([ELF or HEX](#elf-or-hex)); it shows the file's
SHA-256 and writes it through the bootloader (HalfKay) after an explicit
confirmation; with the probe
connected it restarts the module into the bootloader itself. `cargo xtask
web` needs the `wasm-bindgen` command line at the version `Cargo.lock` pins
(it says which, and how to install it). Both sides have been tried on a
module from Chrome on macOS; `cargo xtask flash` stays the reference for
development. After rebuilding the page, reload it without the cache
(Cmd+Shift+R): Chrome otherwise keeps the previous `app.js`.

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

### ELF or HEX

The build produces an **ELF**,
`target/thumbv7em-none-eabihf/release/oc-firmware`; `cargo xtask hex` turns
it into an **Intel HEX** file, `dist/oc-firmware.hex`, with the toolchain's
own `llvm-objcopy`. The HEX is the format PJRC's tools read: the Teensy
Loader application and `teensy_loader_cli`. Use it to install without this
repository's tooling, or to hand a firmware to someone who uses PJRC's
loader; CI uploads it as a build artefact. `cargo xtask flash` writes the
same HEX itself before invoking the loader, so running `cargo xtask hex`
first is never needed.

The checks above need the ELF: a HEX holds only addresses and bytes, so
neither the architecture, nor the target, nor the vector table can be
verified from it. The browser flasher ([In the browser](#in-the-browser))
therefore prefers the ELF, and gives a HEX reduced checks: valid records
and checksums, an end record (a truncated download is refused), every byte
inside the application flash and none written twice, and the boot header
the boot ROM reads (FlexSPI Configuration Block, Image Vector Table, boot
data), which every Teensy 4.0 firmware carries (journal/D-031). The flash
contents the page derives from the ELF and from its HEX are the same, byte
for byte (`real_firmware_matches_its_hex_when_present`,
`real_firmware_hex_reads_as_its_elf_when_present`, `TESTING.md`).

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
version centred on the screen, the build identifier under it, with a one-pixel border tracing itself around
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

The firmware has run on a real module (TLM build): the SH1106 OLED, the USB
CDC boot log, the boot LED, the encoders and the buttons were checked while
bringing it up. The analog and trigger paths have not been, and Level 9 of
`TESTING.md` has not been run as a whole. What is derived from the reference
firmware's source rather than measured, and should be checked with the
diagnostic applet before being trusted:

* **the calibration.** The module starts from the reference firmware's
  factory calibration (`Calibration::FACTORY` in
  `crates/oc-core/src/calibration.rs`: both stages invert, 0 V input at ADC
  code 2730, 10 V over each converter's range); the exact gain and offset are
  per-unit properties, to be measured by the calibration wizard (in
  progress).
* **the CV inputs and the trigger inputs** themselves (pin mapping, polarity,
  levels).
* **the OLED controller** on other builds. SH1106 is the default and is what
  the TLM build carries; build or flash with `cargo xtask … --features ssd1306`
  or `--features ssd1309` if the screen stays blank or only a band of rows
  lights up.

The pinout itself is in one reviewed table in `crates/oc-firmware/src/board.rs`,
which is the only file allowed to name a pin. Note that the firmware drives
**no** UART: every LPUART available on pins 0-23 collides with the panel. Boot
diagnostics go out over **USB CDC** instead (a virtual serial port on the
Teensy's native USB), which shows two serial ports: the text log first, the
binary probe protocol second (journal/D-029). After flashing with the cable
still plugged in:

```sh
# macOS — the ports usually appear as cu.usbmodem*1 (log) and *3 (probe)
ls /dev/cu.usbmodem* 2>/dev/null
cat /dev/cu.usbmodem*1          # or: cat … | tee boot.log; Ctrl-C to stop
```

The log carries the boot and the events only (settings loaded or saved,
errors, restarts); live values and the loop profile come from `cargo probe`
(`stats`, `stream`, `inputs`). It is held until a program opens the port, so
there is no hurry: the boot lines come first, however late it starts. `cat`
keeps every line in the terminal's scrollback, where `screen` scrolls them
out of its own window (`screen -L` would keep them in `screenlog.0`).

Setting either port to 134 baud restarts the module into its bootloader
(Teensyduino's convention): `stty -f /dev/cu.usbmodem…1 134` with macOS's own `stty` (`/bin/stty`), or `stty -F …` with GNU coreutils.

Expect lines such as `oc-firmware 0.4.0 (a1b2c3d) starting (oled=…)`, the
boot stages down to `entering 1 kHz loop`, `settings: Loaded`, a last line
pointing to `cargo probe` for live data, then nothing until an event (`TESTING.md` 9.1). Early boot also blinks the onboard
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
