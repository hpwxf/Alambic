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
  `Engine::tick`, then `audio::publish`es `Engine::params` and the output
  calibration, and waits for its deadline (previous deadline + 1 ms) while
  serving USB. `publish` builds the voice's `oc_core::apps::sound::Setup` there,
  outside any critical section (journal/D-040).
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
  only. The engine's own outputs are muted on the module
  (`audio::MutedOutputs`); elsewhere the engine's voice runs at the tick rate,
  and Rack runs another at its sample rate (`oc_engine_audio`).

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
  (`audio::publish`, after the tick) is the loop's share of new parameters:
  the comparison, and `Setup::new` when they changed (journal/D-040).
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
