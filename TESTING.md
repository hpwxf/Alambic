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
confirm the two facts still marked provisional in `crates/oc-firmware/src/board.rs`:
the calibration slopes and the OLED controller.

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
  polarities (nominal and inverting calibration); monotonicity of the input
  conversion; edge-counter and quadrature-decoder invariants (never counts
  more edges than samples, never drifts by more than one detent per four
  transitions); framebuffer pixel independence.
* `diagnostic.rs` — behavioural tests driven through `Engine::tick` with the
  deterministic mocks in `oc_core::testing`: mode cycling (OFFS/RAMP/ZERO),
  trigger debouncing and counting, encoder selection and offset dialling with
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
configured produces a defined, inert result. The engine wired behind the ABI
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
`engine/tick_busy`, `applet/update`, `applet/render`,
`conversions/adc_code_to_millivolts` and
`conversions/millivolts_to_dac_code`. The firmware runs `Engine::tick` at
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

1. **Behavioural assertions** in `scenarios.rs` — expected CV outputs, cable
   presence, trigger counts, offset and mode after the sequence.
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
| `p`             | toggle the cable on the selected CV input    |
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

The same `cargo test -p xtask` invocation also runs the unit tests for
`xtask::size_check` (parser + layout checklist over fixture `llvm-size` text).
Those do not need a firmware build either; the live checklist on a real ELF is
Level 7 (`cargo xtask size`).

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
in `crates/oc-firmware/src/board.rs` and in `README.md`'s *Before the first
flash* section: the calibration slopes, and whether the panel is SH1106 (stock),
SSD1306, or SSD1309. Follow it in order; each step assumes the previous one succeeded.

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
   ls /dev/cu.usbmodem*
   screen /dev/cu.usbmodem* 115200
   ```

   Baud is conventional only; CDC ACM ignores it. Expect, in order:

   ```text
   oc-firmware X.Y.Z starting (oled=SH1106|SSD1306|SSD1309)
   usb cdc up
   adc ok
   triggers ok
   spi ok
   dac reset released
   oled init ok (…)   # or: oled init failed: Bus|Pin
   panel ok; entering 1 kHz loop
   tick=1000 last_us=…
   ```

   A once-per-second `tick=` line proves the 1 kHz loop is alive even when
   the OLED stays black. A panic after the logger is up also prints the
   panic message (`teensy4-panic` feature `log`) before the SOS blink.
4. Power the module (Eurorack rail or USB). Expect the OLED to first show the
   boot splash screen for about 1.5 seconds: the name and version
   (`O&C Rust vX.Y.Z`) centred on screen, with a one-pixel border tracing
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
   wait until it is fixed. The USB `tick=` heartbeat is the same signal when
   the panel is unreadable.

### 9.2 — CV inputs

For each of the four front-panel CV jacks in turn:

1. Leave it unpatched. The channel row must show `.` (no cable) and a
   reading near 0 V (exact value depends on the still-unmeasured calibration
   offset, see 9.4).
2. Patch in a known, static DC voltage from a calibrated source (a battery
   or bench supply through a suitable divider — **never exceed the module's
   input range**, nominally -6 V to +9 V before scaling; when unsure, start
   with ±1 V). The row must switch to `P` (patched) immediately.
3. After roughly one second of a steady level, the "active signal" indicator
   (`~`) must turn **off** — Level 1's tests already prove a static input is
   never mistaken for an active one; this step confirms the same on real
   hardware.
4. Sweep the source slowly (a few Hz) or wiggle it by hand. The `~` indicator
   must turn **on** while it is moving.
5. Note the reading shown on screen next to the applied voltage for later use
   in 9.4. Do not expect it to be exact yet: the slope in `board.rs` is
   provisional.
6. Confirm channel order: `CV1`, `CV2`, `CV3`, `CV4` on screen must correspond
   to the jacks in the same visual order as silkscreened on the panel. The
   pinout comment in `board.rs` calls out that the pins are *not* in that
   order internally — this step is what would catch a channel-swap bug that
   no host test can see.

### 9.3 — Trigger inputs

For each of the four trigger jacks:

1. Unpatched, the gate indicator must read low and the edge counter must not
   advance on its own.
2. Send a clean gate or trigger pulse (a Eurorack-level trigger source, or a
   push-button through a suitable pull-up/level circuit — **do not** drive
   the jack directly from a low-impedance source outside Eurorack levels).
   The edge counter must increment by exactly one per pulse, matching the
   debouncing behaviour proven on the host in Level 1.
3. Press the left encoder: all four trigger counters must reset to zero
   (`README.md`, encoder table).

### 9.4 — Calibration slopes

This step turns the "provisional" calibration in `board.rs` into a measured
one.

1. In **ZERO** mode (press `up`/`down` on the module until the mode row
   reads `ZERO`), measure all four CV outputs with a voltmeter. They should
   read close to 0 V; note the actual offset error on each channel.
2. In **OFFS** mode, turn the right encoder to set a known offset (the
   screen shows it in volts) on the selected channel and measure the actual
   output voltage. Repeat at a few points across the range (e.g. -3 V, 0 V,
   +3 V, +6 V) to derive the real gain, not just the offset.
3. Compare against the values already fed into the module for the inputs
   (9.2, step 5) the same way, at a few known input voltages.
4. If either slope or offset is off by more than the module's own precision
   allows for musical use, update `CV_INPUT_CALIBRATION` /
   `CV_OUTPUT_CALIBRATION` in `board.rs` accordingly, re-run
   `cargo test -p oc-core` (Level 1) to confirm the compile-time assertions
   still hold (both slopes must stay negative — the front end and the output
   stage both invert), and reflash.

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

### 9.6 — Sign-off

Record the outcome (pass/fail per section, measured calibration values, OLED
controller identified) wherever the project tracks hardware notes. Only after
this checklist passes should the module be considered fit for anything beyond
bench testing, and only after 9.4 should the calibration constants in
`board.rs` be treated as anything but a starting guess.

**Proves:** the full signal chain — analog front end, GPIO, SPI peripherals,
OLED — works on the actual module, and yields the real calibration.
**Does not prove:** long-term reliability, temperature drift, or behaviour
under Eurorack power-supply noise; none of that is in scope for this plan.

## What is not tested yet

* **The VCV Rack 2 module inside an actual running Rack instance.** The ABI
  is implemented and defensively tested (see *VCV Rack ABI robustness*
  above), and `vcv/OrnamentCrimeAlambic` has been built and linked successfully
  against a real Rack SDK, but nobody has yet loaded the resulting plugin
  into VCV Rack and patched cables to it. There is no automated way to do
  that; treat the panel layout and the knob-as-encoder interaction in
  `Diagnostic.cpp` as unverified until someone has.
* **Renode boot smoke test** — dropped as originally specified because the
  firmware drives no UART (every LPUART on pins 0–23 collides with the
  panel, as documented in `crates/oc-firmware/src/main.rs`). The plan file
  records the alternatives under consideration (USB CDC banner, semihosting,
  or observing SPI writes); none is implemented.
* **The firmware has never run on real hardware** — Level 9 above is
  untried in practice as of this writing; treat its content as a procedure
  to execute and refine, not as a report of results already obtained.

## Continuous integration

`.github/workflows/ci.yml` runs, on every push and pull request: `rustfmt`
check, `clippy` on host crates and separately on `oc-firmware` for
`thumbv7em-none-eabihf`, `cargo test --all-features` on Ubuntu and macOS,
`cargo xtask build && cargo xtask size && cargo xtask hex` with the resulting
`.hex` uploaded as a build artefact, and `cargo bench --no-run` to catch
benchmark compilation rot without paying for timing runs on shared runners.
CI covers Levels 0, 1, 2, 4, 6 and 7, plus the VCV Rack ABI robustness tests
(`oc-vcv-ffi` is a default workspace member); Levels 3 (bench timings), 5, 8
and 9, and the C++ plugin build against a real Rack SDK, are manual by
nature — the last needs a downloaded SDK CI does not provision — and are
not, and should not be, run unattended.
