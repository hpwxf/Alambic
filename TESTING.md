# Testing protocols

This document is the single place that maps every verification level to a
command, what to look at, and what it does (or does not) prove. It replaces
scattered mentions across `README.md` and the plan file, and adds the one
level that was never written down: manual hardware validation.

Nothing here changes what already exists in the repository; it only documents
how to run it and how to read the result.

## Quick reference

| # | Level | Command | Runs on |
|---|-------|---------|---------|
| 0 | Static checks | `cargo fmt --all -- --check` / `cargo clippy --all-targets --all-features -- -D warnings` | host |
| 1 | `oc-core` unit & property tests | `cargo test -p oc-core` | host |
| 2 | `oc-drivers` protocol tests | `cargo test -p oc-drivers` | host |
| — | VCV Rack ABI robustness | `cargo test -p oc-vcv-ffi` | host |
| 3 | Performance guard rails | `cargo bench -p oc-core` | host |
| 4 | Simulator scenarios | `cargo test -p oc-sim --test scenarios` | host |
| 5 | Simulator, by hand | `cargo run -p oc-sim` | host |
| 6 | Pre-flash image gate + layout checklist unit tests | `cargo test -p xtask` | host |
| 7 | Firmware build & footprint | `cargo xtask build`, `cargo xtask size`, `cargo xtask hex` | host, cross-compiles ARM |
| 8 | Dry-run flash | `cargo xtask flash --dry-run` | host, needs a built ELF |
| 9 | Hardware validation | see [checklist](#level-9--manual-hardware-validation) | real module |
| — | Everything on the host at once | `cargo test --all-features` | host |

Level 9 is the only one that is not automated, and the only one that can
confirm what is still taken from the reference firmware's source: the factory
calibration (`Calibration::FACTORY` in `crates/oc-core/src/calibration.rs`)
and the OLED controller (`crates/oc-firmware/src/board.rs`).

## Level 0 — Static checks

```sh
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo clippy --package oc-firmware --target thumbv7em-none-eabihf -- -D warnings
```

The workspace enables `clippy::pedantic`, `missing_debug_implementations`,
`unreachable_pub` and `rust_2018_idioms` (`Cargo.toml`, `[workspace.lints]`).
Nothing else in this document is meaningful if this step fails: the firmware
lints are checked separately because `oc-firmware` is excluded from
`default-members` (see `AGENTS.md`) and would otherwise be silently skipped.

**Proves:** the code compiles cleanly and follows the project's style.
**Does not prove:** anything about runtime behaviour.

## Level 1 — `oc-core` unit and property tests

```sh
cargo test -p oc-core
```

Covers, from `crates/oc-core/tests/`:

* `conversions.rs` — `proptest` round-trips of ADC code ↔ millivolts and
  millivolts ↔ DAC code, over the whole input domain and both signal
  polarities (the inverting factory calibration and a non-inverting one); any
  input calibration stays within the supply rails; monotonicity of both
  conversions; edge-counter and quadrature-decoder invariants (never counts
  more edges than samples, never drifts by more than one detent per four
  transitions); framebuffer pixel independence.
* `diagnostic.rs` — behavioural tests driven through `Engine::tick` with the
  deterministic mocks in `oc_core::testing`: mode cycling (OFFS/RAMP/ZERO),
  trigger edge counting (single-sample, no time filter: journal/D-019),
  encoder selection and offset dialling with
  saturation at the output limits, signal-presence detection, microsecond
  counter rollover, render-interval decoupling from the signal path, and
  byte-for-byte reproducibility of the rendered screen for a fixed input
  sequence.
* `menu.rs` — behavioural tests of the app menu, likewise through
  `Engine::tick`: the `up`+`down` chord opens and closes it without either
  button firing its own action, navigation with the buttons and with the left
  encoder, launching from either encoder press, the running applet keeping its
  outputs live while the menu is up, menu input never reaching that applet, and
  app state surviving a switch away and back.
* `splash.rs` — the boot animation and the gate that holds the outputs at rest
  until it finishes.
* `audio.rs` — the voice against `f64` (the sine within one step of 2^15, the
  pitch of `TONE` within a tenth of a cent or a millihertz, 440 zero crossings
  a second at 48 kHz), and `TONE` on the engine: the tones it publishes
  (`Engine::params`), a probe holding an output over a tone, every CV applet
  publishing the levels it writes (journal/D-033).

Plus the in-file unit tests of every module, notably `buttons.rs` (debouncing
and chord arbitration), `menu.rs`, `apps.rs` and `scope.rs`.

**Proves:** the behaviour that all three backends (firmware, simulator, VCV)
share is correct, independently of any register or OS.
**Does not prove:** anything that depends on real timing, real voltages, or a
real display controller.

## Level 2 — `oc-drivers` protocol tests

```sh
cargo test -p oc-drivers
```

Each driver (`dac8565`, `ssd130x`, `triggers`, `panel`, `shared_bus`) is
exercised against a recording `embedded-hal` 1.0 mock bus, asserting the exact
bytes it would put on the wire — the DAC8565 command word layout, the
SH1106/SSD1306/SSD1309 initialisation sequences, chip-select timing around a shared
bus.

**Proves:** the byte sequence sent to each peripheral is the one the datasheet
requires.
**Does not prove:** that the peripheral is wired to the pins the driver
expects, or that it reacts as documented — that is Level 9.

## VCV Rack ABI robustness

```sh
cargo test -p oc-vcv-ffi
```

Covers `crates/oc-vcv-ffi/src/lib.rs`'s own unit tests and
`crates/oc-vcv-ffi/tests/abi.rs`: every exported function tolerates a null
`engine` pointer or an out-of-range channel/index (documented no-op or a
harmless default return, never a panic), two engines created back to back
stay independent, and ticking a freshly created engine before any input is
configured produces a defined, inert result. `oc_engine_audio` reads a held
level back as the tick wrote it at any rate, and plays `TONE` at the rate it
is given between two ticks (journal/D-033). The engine wired behind the ABI
is the same `oc_core::testing` mock backend `oc-core`'s own tests and
`oc-sim` use, so the actual signal-path behaviour is already covered by
Level 1; this level is only about the C ABI boundary itself.

Not a numbered level: it sits alongside Level 2 (both are host-side protocol/
boundary tests over mocks) rather than in the main firmware-readiness
sequence, since nothing here depends on, or blocks, hardware flashing.

**Proves:** the C ABI cannot be made to crash or corrupt memory by any input
a C++ caller could plausibly send, including programmer mistakes (null
pointers, stale indices).
**Does not prove:** that `vcv/OrnamentCrimeAlambic`'s C++ actually calls the ABI
correctly, or that the module behaves sensibly inside a running VCV Rack —
see *What is not tested yet*.

## Level 3 — Performance guard rails (criterion)

```sh
cargo bench -p oc-core
```

`crates/oc-core/benches/engine.rs` benchmarks `engine/tick_idle`,
`engine/tick_busy`, `applet/update`, `applet/render`, `menu/render`,
`conversions/adc_code_to_millivolts`,
`conversions/millivolts_to_dac_code` and `voice/tone_sample_to_codes` (one
sample of `TONE`'s four waveforms turned into DAC codes: what the 48 kHz
interrupt will compute each period, journal/D-033). `crates/oc-core/benches/graphics.rs`
compares `oc_core::text` and `oc_core::gfx` with `embedded-graphics`, each
comparison a group of two (`<name>/embedded_graphics`, `<name>/oc_core`),
and times each page of the graphics demonstration (`gfx_demo/<page>`).
Criterion writes an HTML report to `target/criterion/report/index.html`
(`html_reports` feature). The firmware runs `Engine::tick` at
1 kHz on a 600 MHz Cortex-M7, i.e. a one-millisecond budget; host timings are
not that number, but a sudden change in relative cost is a real regression
signal. As found during Step 2, rendering costs roughly three orders of
magnitude more than the state update, which is why the firmware redraws the
screen only every `RENDER_INTERVAL_TICKS` ticks (`board.rs`) while still
writing CV outputs every tick.

```sh
cargo bench --no-run   # CI only compiles; timing runs are for local judgement
```

**Proves:** rendering and conversions have not silently become orders of
magnitude slower.
**Does not prove:** the actual real-time margin on hardware, which depends on
compiler codegen for Cortex-M7 and on `-O3`/LTO — only `cargo xtask size` and
Level 9 speak to that.

## Level 4 — Simulator scenarios

```sh
cargo test -p oc-sim --test scenarios
```

`crates/oc-sim/tests/scenarios/*.scn` are committed, hand-editable input
sequences (five today: `cv_passthrough`, `trigger_burst`, `encoder_offset`,
`mode_cycle`, `app_menu`). Each is replayed against the real `oc_core::Engine`
and checked two ways:

1. **Behavioural assertions** in `scenarios.rs` — expected CV outputs,
   trigger counts, offset and mode after the sequence.
2. **Golden screen snapshot** — the rendered framebuffer is compared
   byte-for-byte, as dense terminal glyphs (classic braille by default;
   Unicode octants with `--features octant`), against the matching
   `*.screen` file. Committed snapshots always use the default braille
   renderer — regenerate them without the `octant` feature.

A dedicated test (`every_scenario_replays_identically_twice`) replays each
scenario twice and asserts an identical outcome, and
`every_scenario_file_has_a_test_and_a_snapshot` fails the build if a `.scn`
file is added without a corresponding assertion, catching silent drift.

After an intentional rendering change, regenerate the snapshots rather than
hand-editing them:

```sh
UPDATE_SCREENS=1 cargo test -p oc-sim --test scenarios
```

Always diff the regenerated `.screen` files before committing — this command
trusts the current code by construction, so a real bug would be baked in
silently otherwise.

To turn a bug found by hand into one of these committed scenarios:

```sh
cargo run -p oc-sim -- run --record bug.scn   # interact, then quit
cargo run -p oc-sim -- replay bug.scn         # headless replay, prints final state
```

**Proves:** the engine's behaviour and rendering are correct and
bit-for-bit deterministic for a known input sequence.
**Does not prove:** anything about the TUI's own input handling (Level 5) or
about the real hardware (Level 9).

## Level 5 — Simulator, by hand

```sh
cargo run -p oc-sim
```

Useful to explore behaviour interactively before committing it as a scenario,
and to sanity-check the TUI itself, which the scenario tests do not exercise
(they call the simulator's engine directly). Like the firmware and the VCV
Rack module, the simulator boots into the boot splash screen before the
diagnostic applet takes over — confirm the name/version banner and the border
tracing itself around the screen, the same thing 9.1 checks on real hardware.
Then press `m` to open the app menu, launch `SCOPE`, and confirm the outputs
panel follows. The key map is permanently on screen; it is also in
`README.md` ("The simulator"). The essentials:

| Key             | Action                                       |
|-----------------|-----------------------------------------------|
| `Tab`           | select which CV input the arrows drive       |
| `←`/`→`, `Home` | move / zero the selected CV input            |
| `z x c v`       | pulse triggers 1–4 (`w x c v` on AZERTY)     |
| `↑`/`↓`         | the module's up / down buttons               |
| `m`             | up + down together: the app menu             |
| `w`/`e`, `a`    | turn / press the left encoder (`z`/`e` AZERTY) |
| `r`/`t`, `y`    | turn / press the right encoder               |
| `1` `2` `3`     | paused / real time / 50× virtual clock       |
| `o` / `0`       | reset the module (replays the boot splash)   |
| `l`             | switch between AZERTY and QWERTY             |

In VCV Rack the same chord is unreachable with a mouse (two momentary buttons,
one pointer): right-click the module and choose *Open/close app menu*.

**Proves:** the module is usable and legible end to end from a human's
perspective.
**Does not prove:** anything by itself — it produces no artefact; turn any
interesting finding into a Level 4 scenario.

## Level 6 — Pre-flash image gate

```sh
cargo test -p xtask
```

`xtask/tests/validate_image.rs` builds synthetic ELF byte buffers in memory —
no firmware build required — and checks that `xtask::validate::validate_image`
rejects, each with a distinct actionable message: an x86-64 ELF, a 64-bit
ELF, a big-endian ELF, a truncated file, an empty file, a non-ELF file (e.g.
Intel HEX text), an image over the 1984 KiB Teensy 4.0 flash partition, and a
missing FlexSPI Configuration Block. A dedicated test
(`all_failures_are_reported_at_once`) asserts that a fixture with two
independent problems yields two rejections, not one — the whole point of the
gate is to report everything at once rather than stopping at the first
failure. Another test pins the SHA-256 digest against two known vectors so a
future refactor cannot silently change what gets hashed.

`real_firmware_elf_passes_when_present` additionally validates the actual
release ELF at `target/thumbv7em-none-eabihf/release/oc-firmware` when it
exists, and skips (does not fail) when it does not, so CI does not need to
build the firmware first just to run this suite.

`xtask/tests/pin_rule.rs` also runs here: it scans every source of
`crates/oc-firmware/src/` except `board.rs` and fails if one names a BSP pin
token (`P13`, `p13`) outside a comment, which keeps the pin-naming safety rule
of `AGENTS.md` true (journal/D-007).

The same `cargo test -p xtask` invocation also runs the unit tests for
`xtask::size_check` (parser + layout checklist over fixture `llvm-size` text).
Those do not need a firmware build either; the live checklist on a real ELF is
Level 7 (`cargo xtask size`).

`cargo test -p oc-image` checks the layout of the flash contents an image
describes (segments at their load address, erased gaps, refusal of a
segment outside the flash or overlapping another) and, when `cargo xtask hex`
has left a firmware ELF and a HEX no older than it,
`real_firmware_matches_its_hex_when_present` checks they hold the same bytes:
what the browser flasher writes is what `teensy_loader_cli` writes. Its
`hex_image` tests read synthetic Intel HEX files (gaps, CRLF, record types,
refusal of a bad checksum, a missing end record, data outside the flash or
written twice) and check the boot header (FCB, IVT, boot data, a
Teensyduino-style header accepted), and
`real_firmware_hex_reads_as_its_elf_when_present` reads the real HEX into the
same flash contents as its ELF.
`cargo test -p oc-halfkay` checks the bootloader reports cut from it, and
`cargo test -p oc-web` the browser core (journal/D-031).

**Proves:** the flashing tool correctly refuses the classes of corrupt or
wrong-architecture images it is designed to catch, and reports every problem
in one pass; the size-report parser understands SysV `llvm-size` output and
rejects broken layouts in fixtures.
**Does not prove:** that a validated image actually boots — only Level 9 (or,
eventually, a Renode smoke test — see *Open items*) does that.

## Level 7 — Firmware build and footprint

```sh
cargo xtask build     # cross-compile for thumbv7em-none-eabihf
cargo xtask size      # selective llvm-size table + layout checklist
cargo xtask hex       # dist/oc-firmware.hex

# Optional OLED controller override (stock O&C is SH1106, the default):
cargo xtask build --features ssd1306
cargo xtask hex -F ssd1309
```

`oc-firmware` is excluded from `default-members`, so these are the only
supported ways to produce and inspect it (see `AGENTS.md`); a bare `cargo
build`/`cargo test`/`cargo clippy` never touches ARM code. `build`/`size`/
`hex`/`flash` all accept `--features`/`-F` and forward it to the nested
Cargo invocation (comma-separated or repeated). There is deliberately no
unit test *in* `oc-firmware` — its coverage comes entirely from `oc-core`
and `oc-drivers`, which is why Levels 1 and 2 exist.

`cargo xtask size` no longer dumps a raw `llvm-size` total (that figure mixes
in DWARF and RAM reservations and is not flash size). It runs
`llvm-size --format=sysv --radix=16`, prints only the runtime sections, and
applies an automated layout checklist that **fails the command** if any hard
invariant is broken. The checks include, among others:

* required sections present (`.boot`, `.stack`, `.vector_table`, `.text`);
* `.boot` at FlexSPI base `0x6000_0000`;
* `.stack` at DTCM base `0x2000_0000`, entirely in DTCM, and **strictly below**
  `.vector_table` with no overlap (the anti-overflow property that replaces
  `flip-link` — see `crates/oc-firmware/MEMORY.md`);
* `.text` inside ITCM; `.data`/`.bss` in DTCM above the vector table;
* `.rodata`/`.heap` in OCRAM when present.

The pure parser/checklist logic lives in `xtask::size_check` and is covered by
`cargo test -p xtask` with fixture text (no firmware build required).

**Proves:** the firmware links, the linker script still places sections where
`MEMORY.md` requires, and the stack overflow fault path is intact.
**Does not prove:** that it boots or does anything correct on silicon, or the
true flash loadable size (use Level 8 / `cargo xtask flash --dry-run` for that).

## Level 8 — Dry-run flash

```sh
cargo xtask build
cargo xtask flash --dry-run
```

Runs every validation from Level 6 against the actual built ELF and prints
what it would do, without invoking `teensy_loader_cli`. Read the printed
facts:

```text
loadable size: 36400 bytes
entry: 0x60001031
initial SP: 0x20004000
reset handler: 0x60001031
elf sha256: 9566627c35083fb8...
```

The reset handler and entry point should land in FlexSPI flash
(`0x6000_0000..0x6800_0000`) or ITCM, and the initial stack pointer in DTCM
(`0x2000_0000..0x2008_0000`) — see `MEMORY.md` for the full map. If any of
these look implausible, stop: something changed in the boot sections and
`cargo xtask flash` (without `--dry-run`) must not be run.

```sh
cargo xtask flash              # validate, show the digest, ask [y/N], then upload
cargo xtask flash --yes        # skip confirmation (CI only)
```

**Proves:** the real firmware image passes every automated safety check, and
shows the exact digest that would be written.
**Does not prove:** the module works — it only gates what reaches it. Only
Level 9 confirms the firmware runs correctly, and note that even a *rejected*
image cannot brick the module — see *Hardware safety* in `README.md`.

## Level 9 — Manual hardware validation

This is the level that did not exist as a written procedure before this
document. It is the only way to confirm the two facts flagged as provisional
in `README.md`'s *Before the first flash* section: the factory calibration,
and whether the panel is SH1106 (stock), SSD1306, or SSD1309. Follow it in order; each step assumes the previous one succeeded.

### 9.0 — Before touching anything

* Re-read *Hardware safety* in `README.md`: the HalfKay bootloader is in ROM
  on a separate chip, so a bad firmware cannot brick the module or block a
  future upload — pressing PROGRAM always recovers it. The residual risk is
  electrical, not "bricking".
* Have `crates/oc-firmware/src/board.rs` open. It is the **only** place pin
  numbers are allowed to appear, and its header table states which pins are
  inputs — never configure a probe, jumper or generator to drive an input
  pin (`CV1`–`CV4`, the trigger inputs, the encoder/button lines) as an
  output.
* Do not connect anything to the CV or trigger jacks yet.

### 9.1 — First power-up, LED breadcrumbs, USB log, screen legibility

1. `cargo xtask flash --dry-run` first, read the printed facts, then
   `cargo xtask flash` for real. Keep the USB cable on the Teensy so the
   CDC log is available immediately after reset.
2. **Onboard LED (before SPI).** Count the flash groups on the Teensy LED
   (pin 13) during the first second of boot:
   - **1 flash** — `main` reached, USB log starting
   - **2 flashes** — ADC inputs mapped
   - **3 flashes** — triggers mapped; SPI is about to claim pin 13
   - **9 flashes (Morse SOS), pause, repeat** — Rust panic (`teensy4-panic`),
     not a boot stage. Open the USB serial port if it was already up, or
     bisect which `expect`/`panic!` fires.
   After stage 3 the LED pad becomes SPI SCK and must not be read as a
   status light anymore.
3. **USB CDC log.** On the host (macOS example):

   ```sh
   ls /dev/cu.usbmodem*          # two ports: the log, then the probe
   cat /dev/cu.usbmodem*1        # Ctrl-C to stop; add `| tee boot.log` to keep it
   ```

   The module shows as one USB device with **two** serial ports (journal/D-029):
   the first (lower interface number, usually `…1` on macOS) is the text log
   read here, the second (usually `…3`) speaks the binary probe protocol for `oc-probe`
   and shows nothing readable. Baud is conventional only, with one
   exception: **134 baud** on either port restarts the module into its
   bootloader, ready to flash (Teensyduino's convention), so never pick it in
   a monitor. The log is held until a program opens the port, so the boot
   lines come first whenever `cat` is started; once about 4 KiB are
   waiting, newer lines are dropped until then. Prefer `cat` to `screen`,
   which scrolls the first lines out of its own window. Expect, in order:

   ```text
   oc-firmware X.Y.Z starting (oled=SH1106|SSD1306|SSD1309)
   usb cdc up
   adc ok
   triggers ok
   spi ok
   dac reset released
   oled init ok (…)   # or: oled init failed: Bus|Pin
   panel ok; entering 1 kHz loop
   settings: Loaded            # or Empty, Failed(…); a warning if the flash is unavailable
   boot done; this log now shows events only, live data: cargo probe (stats, stream, inputs)
   ```

   After that the log only carries events: a settings save
   (`settings saved …` / `settings NOT saved …`), a restart into the
   bootloader, and the `settings-stress` lines (9.5c). Nothing is printed
   periodically: whether the loop runs, its profile and the inputs come from
   the probe (9.1b: `cargo probe info`, `stats`, `stream`, `inputs`). A
   panic still writes its message to the log, but nothing sends it any more
   once the panic handler blinks the SOS (journal/D-029;
   `docs/plan/panique-persistante.md`).
4. Power the module (Eurorack rail or USB). Expect the OLED to first show the
   boot splash screen for about 1.5 seconds: the name and version
   (`O&C Alambic vX.Y.Z`) centred on screen and, under it, the build
   identifier (short commit hash, `-dirty` if the tree differed; it must match
   `git rev-parse --short HEAD` of the flashed build), with a one-pixel border tracing
   itself clockwise all the way around the edge. Once the border completes
   its loop, the screen switches to the diagnostic screen: a banner row, four
   channel rows, a mode row, an output row, and a tick counter that
   increments continuously. No input is processed and the outputs stay at
   0 V for the duration of the splash screen (`oc_core::Engine::tick`).
5. **If the screen stays blank** but USB shows `oled init ok` or
   `oled init failed`, this is the expected symptom of an OLED controller
   mismatch or a bus/wiring fault (`README.md`, *Before the first flash*).
   Stock O&C is SH1106 (the firmware default). The `ssd1306` / `ssd1309`
   Cargo features select the other controllers for third-party panels.
   Forward them through the normal xtask gate (build, hex, and flash all
   accept `--features`/`-F`):

   ```sh
   cargo xtask flash --features ssd1306
   # or: cargo xtask flash -F ssd1309
   ```

   If a non-default controller makes the screen legible, update
   `board.rs`'s `OLED_CONTROLLER` default (or the crate default features) so
   the normal `cargo xtask flash` path stays authoritative again; if still blank with `oled init ok` on USB, suspect wiring
   (`OLED_CS`/`OLED_DC`/`OLED_RST` on pins 8/6/7) or panel power before the
   controller choice. If USB never appears and the LED never flashes, the
   image is not running (wrong HEX, not reaching `main`).
6. Confirm the tick counter (`TICKS` row) advances steadily and the reported
   tick duration is well under 1000 µs — a stalled or exploding value points
   at the main loop, not at the analog path, and everything below should
   wait until it is fixed. When the panel is unreadable, `cargo probe stats`
   is the same signal (tick count and period).

### 9.1b — Live probe

With the module running and the USB cable on the Teensy (journal/D-029):

1. `cargo probe info` prints the port it found and `oc-firmware X.Y.Z (build)`,
   the same as the splash screen.
2. `cargo probe stats --fresh 5` prints, after five seconds, one row per stage
   (`tick`, `acquire`, `update`, `outputs`, `render`, `present`, `page`,
   `sample`, `usb`) and the period, then the jitter histogram. The `page`
   count is eight per frame sent.
3. `cargo probe screen` prints the screen as text; it reads as the panel does,
   rotation setting included. `--png file.png` writes an image instead.
4. `cargo probe stream --count 5` prints five samples, one every 100 ms.
5. `cargo probe out A 1000` holds output A at 1 V (check it on a multimeter)
   until Enter; closing the tool releases it, `oc-probe` killed included.
6. `cargo probe reboot`, then `cargo xtask flash`: the flash goes through with
   no PROGRAM press.

### 9.1c — Browser probe and flasher (journal/D-031)

`cargo xtask web`, serve `dist/web/` on `localhost`, open it in Chrome.
Connect, pick the probe port: the status shows the firmware and build. Capture
the screen (it matches the panel), read the profile, start a stream, hold
output A at 1000 mV (multimeter) and release it. Then load
`target/thumbv7em-none-eabihf/release/oc-firmware` from a commit other than
the running one, check its fingerprint (the ELF's SHA-256) against
`cargo xtask flash --dry-run`, confirm and install: the module restarts into
the bootloader by itself, the
page lists HalfKay, the progress bar fills, and the module boots with the new
build identifier on its splash. Install once more from `dist/oc-firmware.hex`
(`cargo xtask hex`): the page says *Intel HEX*, the fingerprint is the `hex
sha256` of `cargo xtask flash --dry-run`, and the module boots. Record the
result in the step's progress file; a pass moves D-031 to `adoptée`.

### 9.2 — CV inputs

For each of the four front-panel CV jacks in turn:

1. Leave it unpatched. The channel row must show `.` (absent) and a reading
   near 0 V (exact value depends on how far this module is from the factory
   calibration, see 9.4). Read the exact figures with `cargo probe inputs
   --seconds 20`, from every tick: extremes, mean, peak to peak and raw ADC
   codes of all four inputs (the factory 0 V code is 2730). Record the worst
   channel: its offset and peak-to-peak noise size the presence threshold
   (journal/D-024). If an unpatched input reads `P`, its offset exceeds the 50 mV
   presence threshold of journal/D-024: note the reading, it decides whether
   the threshold or the calibration has to move.
2. Patch in a known, static DC voltage from a calibrated source (a battery
   or bench supply through a suitable divider — **never exceed the module's
   input range**; the inputs read about -3.3 V to +6.7 V with the factory
   calibration; when unsure, start with ±1 V). The reading must follow it and the row must show `P`
   immediately. Unplug it: the row must go back to `.` within about half a
   second (activity block plus 300 ms hold). Presence is derived from the
   signal, not from the jack (journal/D-024).
3. After roughly one second of a steady level, the "active signal" indicator
   (`~`) must turn **off** — Level 1's tests already prove a static input is
   never mistaken for an active one; this step confirms the same on real
   hardware.
4. Sweep the source slowly (a few Hz) or wiggle it by hand. The `~` indicator
   must turn **on** while it is moving.
5. Note the reading shown on screen next to the applied voltage for later use
   in 9.4. Do not expect it to be exact yet: the module still runs on the
   factory calibration.
6. Confirm channel order: `CV1`, `CV2`, `CV3`, `CV4` on screen must correspond
   to the jacks in the same visual order as silkscreened on the panel. The
   pinout comment in `board.rs` calls out that the pins are *not* in that
   order internally — this step is what would catch a channel-swap bug that
   no host test can see.

### 9.3 — Trigger inputs

For each of the four trigger jacks:

1. Unpatched, the gate indicator must read low and the edge counter must not
   advance on its own.
2. Send a clean gate or trigger pulse from an electronic Eurorack-level
   trigger source — **do not** drive the jack directly from a low-impedance
   source outside Eurorack levels. Trigger inputs are not debounced
   (journal/D-019), so a mechanical push-button would count its bounces. The
   edge counter must increment by exactly one per pulse, including for a
   1 ms trigger, matching the behaviour proven on the host in Level 1.
3. Press the left encoder: all four trigger counters must reset to zero
   (`README.md`, encoder table).

Because trigger inputs accept a level on its first sample (journal/D-019),
also check that nothing counts *too much*. Reset the counter before each
step and compare it with the expected count. `cargo probe triggers
--seconds N` counts the rising edges of all four inputs from a sample of
every tick, independently of the screen's counters, with the time between
edges on the module's clock (a steady clock keeps them within about a
tick's length of each other) and how long each pulse stayed high, to a
tick (which tells the width a source really sends); it says `INEXACT` if the host lost samples,
in which case run it again:

4. **Exact count.** Send a known number of 1 ms triggers (a sequencer or
   burst generator: 16 or 32 pulses, at 120 BPM sixteenths or faster). The
   counter must equal the number sent. Repeat with 5 ms and 10 ms triggers.
5. **Slow edges.** Patch a slow LFO (triangle or sine, ±5 V, 0.2 to 1 Hz)
   into the jack for ten cycles. The counter must read exactly ten. More
   means the input chatters while the slow edge crosses the threshold.
6. **Idle noise.** Leave a cable plugged in but undriven, or driven by a
   steady 0 V, for one minute next to busy modules. The counter must stay
   at zero.
7. **Short pulses.** The firmware captures trigger edges in the GPIO edge
   detector (journal/D-019), so a pulse shorter than a tick must still be
   counted exactly once. A clock source with a known rate and an adjustable
   width does it without counting by hand: at 120 BPM ×16 (31.25 ms), `cargo
   probe triggers --seconds 30` must report 960 rising edges (±1 for the
   window) at every width, down to the shortest the source sends, with a
   longest interval of about one period; twice the period means a lost
   pulse. Before edge capture, 39 % of 0.94 ms pulses were lost that way
   (`docs/plan/etape-3b-sonde.md`). Repeat at the widest setting: no edge
   may be counted twice.

A failure in 4 to 6 is a reason to add a short re-trigger hold-off after
each rising edge (accept the edge at once, ignore further rising edges for
about 2 ms), which keeps 1 ms triggers and rejects chatter; record it in
journal/D-019 before changing the code.

### 9.3b — Outputs into inputs (loopback)

Patch output A into CV1 with a short cable, then:

```sh
cargo probe sweep --out A --in 1            # -3 V to +6 V, 1 V steps
```

Each row is a level held on A, the mean reading of CV1 over 200 ms after
50 ms to settle (`--average-ms`, `--settle-ms`), their difference and the raw code. The difference is
the error of both paths together. For indication only, the one module
measured so far showed a few tens of millivolts with the factory
calibration and about ten after the wizard (9.4); record this module's own. Repeat for B,
C and D into their own inputs (`--out B --in 2`, …), which also checks the
channel order of the outputs. Nothing is held once the command ends,
whether it finished or failed.

### 9.3c — Ratchet (trigger bursts)

Open `RATCHET` from the app menu (journal/D-032). Patch output A into `TR2`
and output B into `TR3`, and a clock (or the `up` button) into `TR1`:

```sh
cargo probe triggers --seconds 10
```

With the defaults (`MODE FIRST`, 4 repeats, 100 ms, `SHAPE 0.0`, `PROB
100%`, `PASSTHRU ON`) and one trigger every second on `TR1`: `TR2` counts one
edge per trigger, `TR3` five, 100 ms apart (to a tick). With `SHAPE -1.0` the
gaps read about 100, 80, 63, 50 ms, with `+1.0` 100, 126, 159, 200. `MODE
SPAN` with `SYNC ON` and `SPAN ×1`: the repeats sit within each step, evenly
at `0.0`, crowding towards its end below 0, towards its start above; compare
both modes by ear. Then check `GATE` (repeats only while `TR1` stays high),
`PASSTHRU OFF` (four edges on `TR3`), `PROB 50%` (about half the repeats, on
their places), and an input: click the right encoder on `FIRST`, turn
`INPUT` to `CV2`, then put a volt on `CV2`: the `CV` bar's block moves to
+1 V, the frame's top edge shows the interval halved (and so do the repeats
once the submenu is closed: click again, or `down`), the `VAL` bar a band
from the panel's 100 ms to it. In the submenu, set `SCALE` to x0.50 (the
`CV` bar's block and band shrink to +0.5 V, the reading's stroke stays at
+1 V; the interval now about 71 ms), `MAX` to +0.5 V (a bracket on the `CV` bar; back
near 71 ms whatever the volt above), then `SCALE` back to x1.00, `MAX` to
+10 V and `EFFECT MUL` (with 5 V on `CV2` the interval stays 100 ms; with
2.5 V, 50 ms); the main screen shows `100 MS *CV2`, and with `SET`, `=CV2`
alone. Then `CENTER`: back to `ADD`, put an LFO on `CV2`, reopen the
submenu, let it swing a few cycles, pick `CENTER` and click: the `CV` bar's
signal swings about 0 V, the `VAL` bar's band either side of the panel's
value. Halve the LFO's level (or change its waveform to a narrower one),
wait 4 s for the range seen (the line along the top of the `CV` bar) to
shrink, and click again: the scale doubles. Note the readings at the LFO's extremes: the inputs read about
−3.3 V to +6.7 V with the factory calibration, so a wider LFO is clipped
there before any `SCALE`. With a
steady clock on `TR1`, the title bar shows its rate (120 BPM for a trigger
every 500 ms); set `SYNC ON` and `FIRST` to `/4`: the repeats on `TR3` come a
quarter of the clock's period apart from the second trigger on. On the
timeline, a dotted line marks where the next trigger is due: with a clock
faster than the burst, the repeats past it are drawn short and are indeed
cut on `TR3`. While a burst runs, the timeline shows it: a playhead
moves above it, the triggers sent turn bold, and with `PROB 50%` the
dropped repeats shrink to a dot. The scroll bar left of the settings shows
which of the two pages is on screen. If `TR2` and `TR3` count nothing, the 5 V triggers do not cross the inputs' threshold: note it, it
decides whether the trigger level has to change.

### 9.3d — Fast DAC and shared bus (journal/D-033, lot 7a)

The DAC's chip select is the SPI controller's own (PCS0) and the DAC runs at
22 MHz; the OLED runs at 22 MHz too, or at 8 MHz with
`cargo xtask flash --features oled-spi-8mhz`. With each firmware:

1. The screen draws normally on every applet (no garbage, no frozen rows),
   including after a minute of turning the encoders.
2. The output levels are right: repeat 9.3b (`cargo probe sweep`) on the
   four outputs, and `cargo probe out` at −3, 0 and +5 V with a multimeter.
   A shift or a jump means the DAC took bytes meant for the OLED.
3. Open `SCOPE` (it copies `CV1` to all four outputs) with an LFO on `CV1`,
   then `cargo probe stats`: the `outputs` stage now writes four words at
   22 MHz. For indication, about 16 µs at 8 MHz before this lot; record this
   module's value.

Keep the default if all three pass; otherwise note which failed and keep
`oled-spi-8mhz`.

On the TLM module this was written against (`b293d6a`, 2026-11-17): the
screen stayed clean at 22 MHz, the four outputs read −3.000/−2.999 V and
+4.997 V, `outputs` took 3.0 µs on average and 8.0 µs at most, one OLED page
49 µs (about 145 µs at 8 MHz). An observation on one module, not a target.

### 9.3e — Audio path (journal/D-033, lots 7c and 7e)

From this firmware on, the DAC is written by DMA at 47 619 Hz and an
interrupt computes the samples and sends the screen; the 1 kHz loop no
longer touches the bus. `cargo xtask flash`, then:

0. The boot log has `audio at 47619 Hz, DAC by DMA`, then a 50 ms check
   timed on the cycle counter: interrupts and DAC bursts with their rate
   (about 47 600 a second each), screen chunks and releases (none yet: the
   engine has drawn nothing), the interrupt's mean and longest duration, the
   shortest and longest period, the NVIC line and the DMA's error status.
   No burst means the DMA is not triggered; no interrupt, that its line or
   handler is wrong. At tick 3 000 a second line counts what happened since
   the boot: interrupts, bursts, chunks, releases and frames; chunks without
   releases mean LPSPI4's frame-complete interrupt does not run.
1. The splash plays and every applet draws normally, as in 9.3d step 1,
   `TONE` included: it writes the DAC every period, which is what showed
   the OLED taking DAC bits when a transfer's end was misread.
2. `cargo probe stats` on `DIAGNOSTIC`, then on `TONE`:
   * `audio_period`: mean 21.0 µs, max under twice that — it times the
     interrupt's entry, the DAC being written by DMA at the timer's
     instants;
   * `audio`: the interrupt's own time; record mean and max with `TONE`
     (four channels written each period) and with `DIAGNOSTIC` at rest (no
     DAC words);
   * `frame`: well under 20 ms (one frame drawn every 20 ticks);
   * `period` still 1 000 µs on average and `tick` + `sample` + `usb`
     well under a period: the loop keeps its rate with the interrupt
     taking its share.
3. The outputs are right: 9.3d step 2 again (`cargo probe sweep`,
   `cargo probe out` with a multimeter).
4. Settings: `SETUP` → `SAVE` still saves (toast), the outputs drop to 0 V
   during it and come back.
5. `TONE`, by ear only for now (lot 7d does the oscilloscope and the
   tuner): every note from C1 to C8 sounds at its own pitch, C5 and above
   included.

Record the values; with `oled-spi-8mhz`, repeat 1 and 2 (`frame` longer).

On the TLM module this was written against (2026-11-18, DAC by DMA, lot
7e): in `TONE`, `audio`
3.4 µs mean and 6.1 µs max, `audio_period` 18.4 to 23.5 µs, `frame`
1.34 ms, the loop at 1 000 µs; every screen stable, the four tones
audible; a Korg NTS-2 tuner on the sine reads in tune up to B7, and C8 at
the edge of its range (C7 or C8).

### 9.3f — Supersaw (journal/D-037, lot 8c)

`SUPERSAW` from the app menu, A (and B) into a mixer or headphones amp:

1. `cargo probe stats` in `SUPERSAW`, just after a restart (the profile
   counts from the splash): `audio` mean and max, at A2 and at C8 with
   `DETUNE` at 100 %; the interrupt must stay well under its 21 µs period.
2. By ear from C1 to C8 at `DETUNE` 0, 50 and 100 %: no click, no tone that
   does not follow the note (aliasing would decide the ×4 of lot 8d).
3. `STEREO ON`: A and B differ, the field widens; `OFF`: the same on both.
4. Inputs unplugged, `DETUNE` at 100 %: the seven bars stay still (an input
   nobody assigned moves nothing). `NOTE`'s `CV1`: a volt raises the pitch
   an octave (the header's hertz double). Assign `CV2` to `DETUNE` (right
   encoder's click, `INPUT` to `CV2`): a volt adds 20 %: the line shows the
   panel's `60%` and `+CV2`, the submenu's top edge the `80%` played. Assign an envelope on `CV4` to `LEVEL` with `MUL`: A follows it.

On the TLM module this was written against (2026-11-18, before the inputs
became assignable): `audio` 3.9 µs mean and 10.2 µs max at A2 and at C8
with `DETUNE` 100 %, `audio_period` 18.4 to 23.5 µs, `frame` 1.37 to 1.39 ms,
the loop at 1 000 µs; the sound right, `STEREO` included. With `CV2` and
`CV3` added unconditionally then, the unplugged inputs' noise (about
±5 codes, −22 to +12 mV) made the bars move at full detune, which Rack,
reading exactly 0 V, did not show. With the inputs assignable (lot 8b′),
the bars stay still, inputs unplugged, at full detune.

Before the module, `cargo run -p oc-core --example aliasing --release --<dir>` 
gives the folding, the clipping and the difference from an ideal
supersaw, and WAVs to listen to (lots 8c′ and 8c″): with the falling
sawtooths, the highest level that clips nowhere is about 2.0 V; at 3 V,
0.05 % of the samples touch the +6 V bound. Flashed and listened to on the
module with the falling sawtooths (2026-11-18): the sound is right.

### 9.4 — Calibration

This step measures how far the module is from the factory calibration, then
calibrates it with the wizard (`README.md`, *Calibration wizard*;
journal/D-026, D-027). Measure with the red lead on the jack's tip and COM on
its sleeve, so the sign reads directly.

1. In **ZERO** mode (press `up`/`down` on the module until the mode row
   reads `ZERO`), measure all four CV outputs with a voltmeter. They should
   read close to 0 V; note the actual offset error on each channel.
2. In **OFFS** mode, turn the right encoder to set a known offset (the
   screen shows it in volts) on the selected channel and measure the actual
   output voltage. Repeat at a few points across the range (e.g. -3 V, 0 V,
   +3 V, +6 V) to derive the real gain, not just the offset.
3. Compare against the values already fed into the module for the inputs
   (9.2, step 5) the same way, at a few known input voltages.
4. Record the offset and gain error of each channel, then run the wizard
   (hold the left encoder switch at power-up): each output at −3 V and +5 V,
   the inputs unplugged for the zero, an output patched into `CV1` for the
   gain. Repeat points 1 to 3: the errors must now be within a few millivolts.
   Never edit `Calibration::FACTORY` for one module: it is the reference
   firmware's default for every module. Power-cycle: the calibration must
   still be in force (the USB log reads `settings: Loaded` at boot).

### 9.5 — Encoders and buttons

1. Turn the left encoder: the selected channel indicator (`>`) must move
   across the four channels and wrap at the ends, matching
   `turning_the_left_encoder_selects_a_channel_and_wraps_around` in Level 1.
2. Turn the right encoder: the offset must change by 100 mV per detent and
   saturate at the output limits rather than wrapping (Level 1:
   `the_offset_saturates_at_the_output_limits`).
3. Press the right encoder: offset returns to exactly 0 V.
4. Press `up`/`down`: mode cycles `OFFS → RAMP → ZERO → OFFS`.
5. In **RAMP** mode, confirm on a scope or by ear (through a VCA/audio
   interface) that the four outputs are a saw wave roughly a quarter-period
   apart, per Level 1's `the_four_ramp_channels_are_a_quarter_period_apart`.

### 9.5b — System settings

1. Open the app menu, go to `SETUP`, press the right encoder: the settings
   screen opens and all four outputs read 0 V.
2. On `CONTRAST`, turn the right encoder: the panel brightens and dims at
   once. On `ROTATE`, one detent turns the whole picture upside down; on
   `SHIFT`, the picture moves sideways one column per detent.
3. On `ENC LEFT`, reverse the left encoder, choose `EXIT`: nothing is kept.
   Do it again and choose `SAVE`: in the diagnostic applet the left encoder
   now moves the channel selection the other way, the screen shows
   `SETTINGS SAVED` for a second and a half, and the USB log prints
   `settings saved`. Power-cycle: the setting is still there, and the boot
   log reads `settings: Loaded`.
4. Power-cycle holding the left encoder switch until the splash screen ends:
   the calibration wizard opens. Power-cycle holding `up` + `down`: they
   open in `SAFE MODE`, upright and with the encoders as wired, and the app
   menu does not open.

5. Sector rotation and power cuts during a save: see 9.5c, which automates
   both.

### 9.5c — Settings flash under stress (test firmware)

The settings store adds each save after the previous one and erases one of
its four 4 KiB sectors only when it has to reuse it, so the flash wears
evenly. That rotation (sectors filling, being reclaimed, erased) only shows
after dozens of saves, and a power cut only matters in the milliseconds of a
write or an erase: neither is practical by hand. A test firmware does both.

1. Calibrate and save first (9.4), so there is something to lose.
2. `cargo xtask flash --features settings-stress`. At power-up it saves the
   settings 300 times, alternating them with the contrast one step off, reads
   each save back, and shows progress (`STRESS OK 120/300 9 ERASES`). The
   last save restores the settings as they were.
3. The result stays on screen for 3 s, then `SETTINGS SAVED` (or
   `SAVE FAILED!`) shows over the splash, and the USB log, read from the
   boot, ends the run with `STRESS OK 300/300 n ERASES`. Pass: `OK`,
   `300/300`, and `n` above a dozen (the sectors were reclaimed).
4. Power cut: power-cycle; the run starts again; cut the power while it is
   counting, at any moment. Power up again and let the run finish: it must
   pass, its first load having found the settings (a failed load would leave
   the defaults and show as a calibration lost in step 5). Repeat a few times
   at different moments.
5. **Reflash the normal firmware** (`cargo xtask flash`): the stress build
   rewrites the sectors at every power-up. The module must start with the
   calibration of step 1 (`settings: Loaded` in the boot log; the contrast may be one step
   off if the last cut landed between two saves).

### 9.6 — Sign-off

Record the outcome (pass/fail per section, measured calibration values, OLED
controller identified) wherever the project tracks hardware notes. Only after
this checklist passes should the module be considered fit for anything beyond
bench testing, and only after 9.4 should the factory calibration be treated
as anything but a starting guess for this module.

**Proves:** the full signal chain — analog front end, GPIO, SPI peripherals,
OLED — works on the actual module, and yields the real calibration.
**Does not prove:** long-term reliability, temperature drift, or behaviour
under Eurorack power-supply noise; none of that is in scope for this plan.

## What is not tested yet

* **The VCV Rack 2 module inside a running Rack instance, systematically.**
  The ABI is implemented and defensively tested (see *VCV Rack ABI
  robustness* above), and the plugin builds against a real Rack SDK and loads
  in Rack (it showed in Rack's module browser, `8bdd9bc`), but there is no
  recorded check of patching cables to it. There is no automated way to do
  that; treat the panel layout, the knob-as-encoder interaction and the
  "Open/close app menu" context entry in `Alambic.cpp` as unverified until
  someone has. `TONE` played at Rack's sample rate through `oc_engine_audio`
  (journal/D-033) has not been heard in Rack either.
* **Renode boot smoke test** — dropped as originally specified because the
  firmware drives no UART (every LPUART on pins 0–23 collides with the
  panel, as documented in `crates/oc-firmware/src/main.rs`). The plan file
  records the alternatives under consideration (USB CDC banner, semihosting,
  or observing SPI writes); none is implemented.
* **Level 9 as a whole.** The firmware has been brought up on a real module
  (TLM build): 9.1 (screen) and 9.5 (encoders and buttons) were exercised
  while fixing the OLED controller and the pad pull-ups (`3f50a42`,
  `a8f3560`), but 9.2 (CV inputs), 9.3 (triggers) and 9.4 (calibration) have
  not been run. Treat those as a procedure to execute and refine, not as a
  report of results already obtained.

## Continuous integration

`.github/workflows/ci.yml` runs, on every push and pull request: `rustfmt`
check, `clippy` on host crates and separately on `oc-firmware` for
`thumbv7em-none-eabihf`, `cargo test --all-features` on Ubuntu (macOS only
on demand: *Run workflow* with "macos" ticked, because its runners are slow
and expensive),
`cargo xtask build && cargo xtask size && cargo xtask hex`, then
`cargo xtask flash --dry-run` (Level 8: the pre-flash gate on the real ELF,
plus the `.hex` SHA-256 in the log), with the resulting `.hex` uploaded as a
build artefact, and `cargo bench --no-run` to catch
benchmark compilation rot without paying for timing runs on shared runners.
CI covers Levels 0, 1, 2, 4, 6, 7 and 8, plus the VCV Rack ABI robustness tests
(`oc-vcv-ffi` is a default workspace member); Levels 3 (bench timings), 5
and 9, and the C++ plugin build against a real Rack SDK, are manual by
nature — the last needs a downloaded SDK CI does not provision — and are
not, and should not be, run unattended.
