# Changelog

Firmware and workspace versions follow `0.<minor>.<patch>`; the VCV Rack
plugin, which Rack 2 requires to have major version 2, ships as
`2.<minor>.<patch>` of the same release (checked by `xtask/tests/versions.rs`).
Architectural decisions behind each change are in `journal/` (French).

## Unreleased

### Changed

- `RATCHET`'s screen: a scroll bar left of the settings shows the page, in
  place of the arrows in the title bar; the timeline is shorter (15 pixels),
  and while a burst runs it shows that burst, a playhead above it, the
  triggers sent drawn bold and the repeats `PROB` dropped as a dot.
- `RATCHET`'s inputs (journal/D-035): a click on the right encoder opens the
  setting's input submenu instead of turning through `CVn=` / `CVn+`. It
  picks the input and sets a `SCALE` (−200 to +200 %, 5 % a detent), a `SHIFT` (±10 V,
  50 mV), `MIN` and `MAX` bounds (100 mV) applied to the signal, then its
  `EFFECT`: `SET` (follows it), `ADD` (adds it) or, new, `MUL` (multiplies
  the panel's value, 5 V leaving it unchanged). It is a frame over the main
  screen, the setting and the value a burst would take now in its top edge;
  two bars show the input's signal (once scaled, shifted and bounded, the
  reading as it comes, the range seen over the last 2 to 4 s, the bounds) and the setting it moves (the panel's value, the value now, the stretch between).
  The right encoder's click or `down` closes it. A `CENTER` button sets the
  scale and shift that carry the readings seen onto the range of signal that
  sweeps the setting end to end: centred on 0 V (around the panel's value)
  with `ADD`, from its least to its most value with `SET`, 0 to 10 V with
  `MUL`. A `ratchet` group in `benches/engine.rs` times the main screen
  and the submenu. The main screen names an
  input as an operation on the panel's value, `+CV2` or `*CV2`, and `=CV2`
  alone, in place of a value it no longer uses.

## 0.5.0 — 2026-11-18 (Rack plugin 2.5.0)

### Added

- **Browser probe and flasher** (journal/D-031, tried on a module):
  `cargo xtask web` builds a static page into `dist/web/` that talks to the
  module from Chrome or Edge — the probe over Web Serial (screen, profile,
  stream, held outputs) and the flash over WebHID, from the firmware ELF
  behind the same gate as `cargo xtask flash`, or from an Intel HEX file
  with reduced checks (records, flash range, the boot header).
- `oc-image` (the pre-flash gate, moved from `xtask`, and the flash contents
  of a validated ELF or HEX), `oc-halfkay` (the bootloader's reports), `oc-web`
  (their WebAssembly bindings).
- **`RATCHET`**, the first musical applet (journal/D-032): each rising edge
  on `TR1` is copied to output A and starts a burst of 5 ms triggers on B,
  the incoming one passed through or not (`PASSTHRU`). The burst has a count
  (1 to 32) or runs while the gate stays up; a signed `SHAPE` (−3.0 to +3.0
  octaves: 0 even, below accelerating, above slowing down); `PROB`, the chance
  each repeat fires. `MODE` compares two placements: `FIRST`, a first interval
  then intervals changing geometrically, and `SPAN`, the repeats spread over a
  span. `SYNC ON` makes the first interval or span a fraction of `TR1`'s
  period (×2 to /32, ×1.5, ×4/3, ×3/4 and ×2/3 included), whose rate shows in BPM; the timeline marks where the next trigger
  is due. Each burst setting can take an input, `CVn=` (follows it) or `CVn+`
  (adds it), chosen by a click on the right encoder; inputs are sampled at
  the edge, and a new edge restarts the burst.
- `oc_core::fixed`: base-2 logarithm and exponential in 16.16 fixed point.
- **`TONE`**, a test applet for the audio path (journal/D-033): one pitch on
  the four outputs, a sine, a triangle, a sawtooth and a square; the note on
  the left encoder (C1 to C8), the peak level on the right one (0 to 3 V),
  `CV1` at one volt per octave. VCV Rack plays it at its sample rate.
- `oc_core::audio`: the output parameters an applet publishes (a held level
  or a tone per output, `Engine::params`) and the `Voice` that renders them
  at a fixed rate with 32-bit phases; `oc_engine_audio` in the Rack ABI.

### Changed

- The boot log checks the audio path for 50 ms timed on the cycle counter
  (interrupts and DAC bursts with their rate, the interrupt's duration, the
  period's bounds, the DMA's error status) and, at tick 3 000, counts what
  it did since the boot, screen chunks and frames included.
- **The outputs and the screen run at audio rate** (journal/D-033, lots 7c
  and 7e): at 47 619 Hz the PIT triggers a DMA copy of the four DAC words,
  so the DAC is written at instants the hardware fixes; the DMA's interrupt
  computes the next sample of the engine's output parameters and sends a
  piece of the screen; the 1 kHz loop publishes the parameters and hands
  its frames over. `cargo probe stats` shows the interrupt
  (`audio`, `audio_period`, `frame`). The outputs rest at 0 V while the
  settings are saved.
- `oc_drivers::ssd130x::FrameStream` (a frame in chunks),
  `dac8565::ChangedCodes` and `fifo::words`, for a sender without a
  blocking bus; `Ssd130x::release`; `Profile::merge`.
- The DAC's chip select is LPSPI4's own (PCS0), the shared bus picks the chip
  select and the clock of each device before each transfer, and the DAC and
  the OLED run at 22 MHz (`oled-spi-8mhz` puts the OLED back at 8 MHz); a DAC
  channel is written only when its code changes (journal/D-010, D-033).
- The app menu lists its entries right under its title, to fit a fifth
  applet.
- `oc-probe`'s command line is behind a default `cli` feature;
  `render::screen_grey` gives the screen's pixels to any output.
- `rust-toolchain.toml` also provisions `wasm32-unknown-unknown`.

## 0.4.0 — 2026-11-17 (Rack plugin 2.4.0)

### Added

- **Graphics primitives** (`oc_core::gfx`, journal/D-030): byte-wise
  rectangles, straight lines and inverted areas (the same pixels as
  `embedded-graphics`), lines, circles and discs that can also invert,
  integer trigonometry; controls (knob, selector, inverted value box,
  caption, bars, title bar), plots (trace, min/max waveform, histogram,
  needle scale) and sequencer patterns (steps, dots, Euclidean ring,
  one-octave keyboard), after the screens of Squares-and-Circles.
- **`GFX DEMO`**, a third applet showing them on five pages: fonts, knobs,
  curves, sequencer and meters, animated from the ticks' time; its outputs
  carry the simulated signals.

- **Loop profile.** `oc_core::profile` times each stage of `Engine::tick`
  (acquire, update, outputs, render, present) and the period between ticks,
  with a jitter histogram, on the new `Clock::fine_count` (journal/D-029).
  The firmware backs it with the core's cycle counter and logs it on USB,
  two lines a second after the heartbeat (`TESTING.md` 9.1).

- **Live probe, module side.** The firmware's USB device now has two
  serial ports: the text log, as before, and a binary port speaking the
  probe protocol (`oc-protocol`: postcard over COBS; profile, sample
  stream, forced outputs, screen capture, restart into the bootloader).
  `imxrt-log` is replaced by a `log` backend of the firmware's own. The OLED
  frame is sent and timed page by page. Setting either port to 134 baud
  restarts into the bootloader (journal/D-029).
- `Engine::force_output`, `Engine::raw_codes`, `Ssd130x::flush_page`.
- `TickContext` carries the levels actually written (`cv_out`) and whether a
  probe holds an output; the diagnostic screen's `OUT` row shows them, as
  `OUT*` while an output is held.
- **`oc-probe`**, the host tool over the probe port (`cargo probe`):
  `stats` (the profile as a table), `stream` (inputs, codes, outputs,
  triggers), `out` (hold an output, watch the inputs), `screen` (text or
  PNG, as the module shows it), `reboot` (into the bootloader).
  For level 9: `inputs` (spread of every input), `triggers` (rising edges
  from a sample of every tick, with the pulse widths, flagged when samples
  were lost), `sweep`
  (an output stepped through levels into a patched input).
  Samples carry the module's clock (protocol version 2): durations and
  trigger intervals are real time, since a tick that redraws the screen
  lasts longer than the others.

- **`benches/graphics.rs`**: text and shape drawing against
  `embedded-graphics`, each comparison a group of two, and the demo pages;
  criterion now also writes an HTML report
  (`target/criterion/report/index.html`).

### Changed

- **The USB log carries the boot and events only.** The once-a-second
  heartbeat, the two profile lines and the per-input extents are gone: the
  probe gives them live (`cargo probe info`, `stats`, `inputs`). Read the
  log with `cat` rather than `screen`, which scrolls the first lines out of
  its own window.

- **Text is drawn about ten times faster.** `oc_core::text` lays each glyph
  of the 5x8 font as five column bytes straight into the framebuffer's
  page layout, instead of setting its pixels one at a time through
  `embedded-graphics`; the pixels are the same, checked glyph by glyph
  against `embedded-graphics` in the tests. The diagnostic screen's
  drawing falls from 15.7 to 2.8 us on the host (criterion,
  `cargo bench -p oc-core --bench graphics`) and from about 646 to 88 us on the
  module, so the redraw tick is no longer the loop's tightest (journal/D-009).
  The font is part of the `TextStyle`: `oc_core::text::fonts` carries six
  `embedded-graphics` ASCII fonts (4x6 to 10x20) as generated column tables
  (`UPDATE_FONTS=1 cargo test -p oc-core --test fonts`), each checked pixel
  for pixel against its source; an applet picks its own.
- **`embedded-graphics` is a test dependency only.** `oc-core` draws with its
  own `gfx` and `text` (`gfx::Point`, `text::Alignment`, `text::Baseline`,
  `Ink` as the text colour) and `FrameBuffer` no longer implements
  `DrawTarget`; the tests and benchmarks check against `embedded-graphics`
  through an adapter. The firmware image shrinks by about 1.3 KiB.

### Fixed

- The loop profile counted the last boot-splash tick as its first period
  (about 1.5 ms on the module), since only the statistics were cleared when
  the splash ended, not the start of that tick.

- **The first log lines never reached a terminal.** The log was sent as
  soon as the host enumerated the module, before any program opened the
  port. It now waits for a terminal (DTR), so the boot lines are the first
  ones `screen` shows, however late it starts (journal/D-029).

- **Every redraw stalled the loop for about 2 ms.** The tick that drew
  the screen also sent the whole frame, longer than a period, so the
  module ran at about 947 ticks a second. The engine now draws in one tick
  and sends one page per following tick (`Display::page_count`,
  `Display::present_page`; one page by default, so the simulator and Rack
  are unchanged).

- **Short trigger pulses were missed.** Triggers were sampled once per
  tick, so a pulse that started and ended between two samples was never
  seen (39 % of 0.94 ms pulses on the module). The GPIO edge detector now
  latches each trigger's edge, read once per tick with no interrupt taken;
  the VCV module applies the same rule between two ticks
  (`oc_drivers::triggers::EdgeCapture`, journal/D-019).

- **The firmware loop ran at about 775 Hz, not 1 kHz.** The deadline was
  taken after the input sampling, which stretched every period by its
  cost; the period now starts before it, and periods follow a fixed
  schedule so the spin's own lateness no longer adds up.
- The USB heartbeat line printed the settings detail after `last_us=` and
  the tick period at the end of the line.

## 0.3.0 — 2026-11-16 (Rack plugin 2.3.0)

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
  `oc_engine_settings_load` in the C ABI, base64 in `dataToJson`), the
  module in four 4 KiB sectors of its own flash at `0x601F_0000`, erased and
  programmed through the i.MX RT1062 boot ROM (`flash_store.rs`, the
  firmware's only `unsafe`). That area overlaps Teensyduino's EEPROM
  emulation: settings another firmware kept there are ignored, then replaced
  on the first save. `cargo xtask` now rejects an image reaching
  `0x601F_0000` (journal/D-028).
- **System settings screen.** `SETUP`, the last entry of the app menu, edits
  the startup app, the contrast, a 180° rotation, a picture shift and each
  encoder's direction, live; `SAVE` keeps and stores them, `EXIT` or the
  `up` + `down` chord puts back what was in force. Held through the splash
  screen, the left encoder switch opens it on the calibration line and
  `up` + `down` in safe mode (screen and encoders at their defaults until
  saved); the splash recalls both gestures. Rotation and shift are applied to
  the pixels in the core, contrast through the new `Display::set_contrast`
  (journal/D-026).
- **Input presence threshold lowered to 50 mV** (from 100 mV), measured on a
  calibrated module: unplugged inputs read within ±10 mV with 17 mV of noise
  (journal/D-024).
- **Settings flash stress test** (test firmware only):
  `cargo xtask flash --features settings-stress` saves the settings 300 times
  at power-up, reading each back, and reports the result and the number of
  sector erases on screen and on every USB heartbeat line; cutting the power
  during the run tests recovery from a cut mid-write (`TESTING.md` 9.5c).
  Normal saves now log the erase count too.
- **Save confirmation.** After a `SAVE` (settings screen or wizard) a box
  over the screen says how it went for 1.5 s: `SETTINGS SAVED`,
  `SAVE FAILED!`, `KEPT UNTIL RESTART` (simulator without `--settings`) or
  `SAVED WITH THE PATCH` (Rack). The backend reports the outcome
  (`Engine::report_save`). The firmware's USB heartbeat line now ends with
  the last settings event (`settings=loaded`, `saved`, …), since the boot
  lines are gone by the time a serial monitor connects.
- **Calibration wizard**, with the Phazerville firmware's steps: start
  fresh, DAC A to D at −3 V and +5 V with a multimeter (the table is redrawn
  through the two points), ADC zero with the inputs unplugged, ADC gain on
  `CV1` at +1 V and +3 V through a patched output, save. Reached from
  `CALIBRATE` in `SETUP`, or by holding the left encoder switch at power-up.
  Captures are averaged over a quarter of a second after the outputs settle,
  and refused when far from the factory values (journal/D-027).
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
