# Firmware notes for agents

Routed from `AGENTS.md`; read it before touching `crates/oc-firmware`, the
audio path or the screen path. The rationale is in the journal (D-004, D-008,
D-033, D-040, D-042); measured figures are in `PLAN.md` and
`docs/plan/avancement/`, never here. Where a module's own documentation says
it already, this file only points to it.

## Cross-compilation

* **Target** `thumbv7em-none-eabihf`; toolchain pinned by
  `rust-toolchain.toml` (bumped deliberately by hand) with `rust-src`,
  `llvm-tools`, that target and `wasm32-unknown-unknown` for `oc-web`;
  `rustup` provisions everything on first build.
* **`oc-firmware` is excluded from `default-members`**: a bare
  `cargo build|test|clippy` never touches ARM code. Build the firmware through
  `cargo xtask build|size|hex` or `cargo fw`.
* **No `memory.x`, on purpose.** `teensy4-bsp`'s `rt` feature drives
  `imxrt-rt::RuntimeBuilder`, which generates `t4link.x` (memory regions and
  the i.MX RT boot sections) into `OUT_DIR`; `.cargo/config.toml` links with
  `-Tt4link.x`. A hand-written `memory.x` would conflict with it.
* **No `flip-link`**: it needs a `MEMORY { RAM }` region. `imxrt-rt` already
  puts `.stack` at the bottom of DTCM, so an overflow faults; `cargo xtask
  size` checks it. Layout, reserved flash and the checklist:
  `crates/oc-firmware/MEMORY.md`.
* **Never set `RUSTFLAGS` in the environment**: it replaces
  `target.thumbv7em-none-eabihf.rustflags` and silently drops `-Tt4link.x`.
  CI denies warnings per invocation (`-D warnings`) instead.
* **LLVM tools come from the Rust sysroot** (`xtask/src/llvm.rs`), not from
  `PATH`.

## Two contexts (journal/D-008, D-033, D-042)

* **The control context** is a cooperative 1 kHz loop
  (`crates/oc-firmware/src/main.rs`): each tick samples the inputs (four
  blocking ADC conversions, the trigger edge latches, the panel), runs
  `Engine::tick`, then publishes `Engine::params` and the output
  calibration (`audio::Publisher`), and waits for its deadline (previous
  deadline + 1 ms) while serving USB. The publisher compares
  `Engine::params_revision` rather than the parameters, and builds the
  voice's `oc_core::apps::sound::Setup` there, outside any critical section,
  only when they changed (journal/D-040).
* **The audio path** owns the SPI bus at 47 619 Hz (PIT channel 0 on PERCLK,
  21 counts). Each timeout makes the DMA copy one slot of a ring into
  LPSPI4, so the DAC is written at instants the hardware fixes; the DMA's
  interrupt computes the next sample with its own `Voice`, two slots ahead,
  waits for the burst's echo and sends a piece of the screen; LPSPI4's
  frame-complete interrupt releases the OLED's chip select. One sample per
  period, no blocks (journal/D-042). Step by step: the module documentation
  of `crates/oc-firmware/src/audio.rs`; DMA, ring and handlers:
  `audio_irq.rs`.
* The two share mailboxes behind `critical_section::Mutex`, short copies
  only. The engine renders no voice (journal/D-040): on the module the
  interrupt plays and the loop reports what it played
  (`Engine::set_played`), the engine's outputs (`audio::MutedOutputs`)
  never written; the simulator plays an `oc_core::audio::Player` once a
  tick and writes it (`Engine::write_outputs`), Rack once a sample
  (`oc_engine_audio`). The firmware's `audio::Publisher` and the
  `Player` decide when to prepare through one `oc_core::audio::Follower`.

## The screen

* **Vocabulary.** The *framebuffer* is the screen's copy in RAM (128 x 64,
  1 024 bytes). A *frame* is one whole image. A *page* is one eighth of it, a
  band 8 pixels high (128 bytes), the unit the SH1106 / SSD130x controllers
  store and receive, not a screen. *Drawing* (`Stage::Render`) computes
  pixels, CPU only; *sending* copies them to the panel over the shared bus.
  The panel keeps showing what it last received.
* The engine draws every `board::RENDER_INTERVAL_TICKS` ticks and
  `present`s the frame: on the module (`audio::MailboxDisplay`) a 1 KiB copy
  into the mailbox, the latest frame winning. The interrupt sends it in
  chunks (`oc_drivers::ssd130x::FrameStream`, commands or pixels, never both,
  `board::OLED_CHUNK_BYTES` a period). The boot splash presents every tick.
* The engine still supports paged displays (`Display::page_count`, one page
  per tick after the drawing), but no backend uses them; the simulator, Rack
  and the tests send the frame in the tick that drew it.

## Measuring costs

* Costs are measured, not assumed: `cargo probe stats` shows the loop's
  stages and the interrupt's (`audio`, `audio_period`, `frame`, and inside
  `audio` the voice's own `voice` and `voice_params`). The interrupt spins
  until the DAC's burst echoes, so `audio` hides any computation shorter than
  the burst (journal/D-039).
* The loop's stages are wall-clock time, the audio interrupt included: a
  stage that lasts long enough takes one (4.2 µs), so a stage's mean moves
  with where the interrupt falls, and only sums of stages compare. `publish`
  (`audio::Publisher::publish`, after the tick) is the loop's share of new
  parameters: the comparison, and `Setup::new` when they changed
  (journal/D-040).
* The end of a transfer is read from its echo in the receive FIFO: the busy
  flag and the transfer-complete flag answer early, and the OLED then takes
  DAC bits.
* Every access to the PIT waits on its 1 MHz domain for microseconds: never
  read it per period.
* The engine times its stages through `Clock::fine_count` (the core's cycle
  counter on the module; the virtual clock's time, read without advancing
  it, elsewhere), so profiling never changes a test or a golden screen
  (`oc_core::profile`, journal/D-029). Host benchmarks are a regression guard,
  not a hardware measurement (`docs/agents/tools.md`).
* `scripts/profile-supersaw.sh` sets `SUPERSAW` through the probe and runs
  `stats` in each case; the CV assignments stay the panel's. `render`
  depends on the page shown (about 95 µs on `SUPERSAW`'s first page, its
  voices animated, 54 µs on the second), and moves the tick's mean by
  about 2 µs: compare a page with the same page.

## DSP in `f32`

What the supersaw bank taught (journal/D-039, its revisions of 2026-11-21
for the module's figures; D-040 and D-044 for the loop's side). They hold
for any block that runs per sample in the audio interrupt.

* **Accumulators stay integers.** A phase is a `u32`, a turn being `2^32`:
  it wraps for free and keeps 32 bits everywhere, where an `f32` phase near
  the end of its turn keeps 24 and its sum drifts (the bank was 10 dB
  further from the ideal supersaw at C2 until its phases became integers,
  lot 9j). Read it as a fraction of a turn per sample; take distances
  (to a jump, between two phases) in integers first. The same goes for
  delay-line positions, LFO phases and counters.
* **Avoid floating-point comparisons per sample.** The target compiles for
  the generic FPv4 unit, without FPv5's branch-free selections (`VSEL`,
  `VMINNM`): every `f32` comparison is a `vcmp`, a transfer of the FPU's
  flags (`vmrs`, which stalls) and a branch. Decide in integers when the
  data is (phases); compare `to_bits()` when both values are at least zero
  (same order); or write the arithmetic so that it needs no test. Removing
  the bank's phase comparisons took `voice` from 1.56 to 1.18 µs at
  `SHAPE` 25 % (lot 9i). To hold a level, round by adding a signed half
  (`copysign`), convert (saturating), then clamp the integer: 1.15 to
  1.07 µs more (`millivolts` in the bank). `-C target-cpu=cortex-m7` was tried and dropped
  (no floating-point instruction left in the image; a toolchain change,
  D-004).
* **The FPU is single precision only.** `f64` is emulated in software:
  never on the module. 64-bit integer division is a library routine too
  (some hundred cycles): `oc_core::fixed::div_wide` divides 64 bits by 32
  with the core's divider when the quotient fits. An `f32` division is in
  hardware, fine in a `Setup`, never per sample (D-040).
* **Everything the parameters decide goes in `Generator::setup`**, run in
  the loop when they change (D-040): reciprocals, coefficients, gains. Under
  a moving CV it runs every tick, in `audio::Publisher`, so its cost shows
  in `publish`, not in `voice`.
* **`core` has no `sqrt`, `round` or `floor`.** Prepare roots in integers
  (`isqrt`) in the `Setup`; round by hand. An `as` conversion from `f32` to
  an integer saturates (and maps NaN to zero), as the core's `vcvt` does.
* **Measure on the module, not on the host.** For these choices x86 answers
  the other way (bits comparisons: 29 -> 39 ns on the host, -24 % on the
  module; `div_wide`: 57 -> 112 ns on the host, about 2 µs saved a `Setup`
  on the module). The figure is `voice` in `cargo probe stats`; criterion
  only guards against gross regressions.
* **Validate against a reference, not for equality.** Pin a checksum of
  the first samples (`crates/oc-core/tests/voice_samples.rs`) so that a
  change of structure cannot change a bit unnoticed, retaking it only for
  an intended change; compare with an integer or ideal rendering within a
  tolerance; and measure the spectrum against the ideal at low and high
  notes (`cargo run -p oc-core --example aliasing --release`): only that
  showed the drift of `f32` phases.
* **The interrupt uses the FPU.** Once the loop computes in floating point
  too, entering the interrupt stacks the FPU's registers (lazily, up to 18
  words): not measured yet, watch `audio` when it happens.
