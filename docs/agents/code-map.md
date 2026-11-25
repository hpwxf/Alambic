# Code map for agents

Routed from `AGENTS.md`: what each crate does and where a given piece of
behaviour lives. The policies (`no_std`, `unsafe`) are in `AGENTS.md`; the
rationale behind each crate is in the journal entry cited.

## Crates

| Crate | Role |
|---|---|
| `crates/oc-core` | **All** behaviour: platform traits, `Engine::tick`, the applets and the app menu, the output parameters, routes and voices (`audio`: `Patch`, `Route`, `Voices`; backends play them through `audio::Player`, `audio::Follower`), CV modulation of a parameter and its submenu (`modulation`), graphics (`gfx`, `text`), buttons and the chord, calibration, settings, framebuffer, profiling. Knows nothing about registers or an OS. |
| `crates/oc-drivers` | Peripheral protocols (DAC8565, SH1106 / SSD1306 / SSD1309, triggers, shared SPI bus) over `embedded-hal` 1.0 only, tested against recording mock buses. |
| `crates/oc-firmware` | Pure wiring: `teensy4-bsp` / `imxrt-hal` resources into the `oc-core` traits, the 1 kHz loop, the audio path. The one crate that cannot be tested on the host. |
| `crates/oc-protocol` | Probe messages (`Request`, `Reply`), `postcard` + COBS framing, `FrameReader`, and the module's side (`server`) (journal/D-029). |
| `crates/oc-probe` | Host tool over the probe port: a `Client` over any byte stream (tested through an in-memory loopback on a mock engine); CLI `stats`, `stream`, `out`, `screen`, `reboot` (journal/D-029). |
| `crates/oc-image` | The pre-flash gate (`validate`, re-exported as `xtask::validate`), the flash contents of a validated ELF (`flash::flash_image`, byte for byte what `llvm-objcopy -O ihex` gives) or of an Intel HEX (`hex`, `boot`: reduced checks); pure over bytes (journal/D-031). |
| `crates/oc-halfkay` | The Teensy bootloader's write protocol as reports, transport-free (what `teensy_loader_cli --mcu=TEENSY40` sends) (journal/D-031). |
| `crates/oc-web`, `web/` | Browser probe and flasher: a WebAssembly core without I/O (`wasm-bindgen` pinned exactly, no `JsValue`, so host-tested) and a plain-JS page, built by `cargo xtask web` into `dist/web/` (journal/D-031). |
| `crates/oc-sim` | Host backend, ratatui TUI, deterministic virtual clock; runs the real engine. |
| `crates/oc-vcv-ffi` | `staticlib` / `rlib` exposing a defensive C ABI over `oc-core` (`oc_engine_*`): every function tolerates a null pointer or an out-of-range index and never lets a panic unwind (`catch_unwind`). |
| `vcv/OrnamentCrimeAlambic` | The Rack SDK plugin shim (C++): ports, a widget reading the ABI's framebuffer, no behaviour. Built by `cargo xtask vcv`. |
| `crates/oc-buildinfo` | Build-script helper of `oc-firmware`, `oc-sim`, `oc-vcv-ffi`: `OC_BUILD_ID`, the short commit hash (`-dirty` if tracked files differ), through `vergen-gix` plus its own `rerun-if-changed`; handed to `Engine::set_build_id`, never computed in `oc-core` (journal/D-025). |
| `xtask` | The only supported build / flash / VCV / web entry point: cross-compilation, size report and layout checklist, HEX packaging, the pre-flash gate, `vcv` (`xtask/src/vcv.rs`), `web` (`xtask/src/web.rs`). |

## Where things live

| Looking for… | File |
|---|---|
| Platform traits (`AnalogIn`, `Clock`, `Display`…) | `crates/oc-core/src/platform.rs` |
| The tick every backend calls | `crates/oc-core/src/engine.rs` |
| The `App` trait, the registry and dispatch (`AppId`, `AppHost`) | `crates/oc-core/src/apps/mod.rs` (journal/D-043) |
| What an applet exposes: parameters, observables, actions (`ParamSpec`, `write_summary`) | `crates/oc-core/src/apps/param.rs` (journal/D-043) |
| The applets | `crates/oc-core/src/apps/`: `diagnostic.rs`, `scope.rs`, `gfx_demo.rs`, `ratchet/`, `tone.rs`, `supersaw.rs` |
| The supersaw's DSP, the applet's own | `crates/oc-core/src/apps/supersaw/dsp/mod.rs` (the sound's parameters `Supersaw`, parts, Szabo's curves, `Coefficients::new`: the preparation in integers), `dsp/oscillator.rs` (`Oscillator`: the seven voices in `f32`, integer phases, journal/D-039) |
| The supersaw as a generator | `crates/oc-core/src/apps/supersaw/generator.rs` (`SupersawGenerator`: `SupersawParams`, `SupersawSetup`, `SupersawState`, journal/D-044) |
| The generators this image composes (`Sound`) and what an applet publishes (`Params`) | `crates/oc-core/src/apps/sound.rs` (journal/D-044) |
| What a backend plays them with, for this image's instance slots (`Patch`, `Setup`, `Voice`, `Changes`, `Voices`, `Follower`, `Player`; `MAX_INSTANCES` in `apps/mod.rs`) | `crates/oc-core/src/playback.rs` (journal/D-040, D-046) |
| The `Generator` trait, the voice, `Setup`, routes and the voices of every slot, the waveforms, levels and tones (`Basic`) | `crates/oc-core/src/audio/mod.rs`, `audio/wave.rs`, `audio/basic.rs` (journal/D-033, D-040, D-044, D-046) |
| A parameter modulated by a CV input, and its submenu | `crates/oc-core/src/modulation/mod.rs`, `modulation/editor.rs` (journal/D-035, D-036); an applet's inputs and open submenu, a parameter's slot: `modulation/slots.rs` (`Modulations`, `slot_of`); reached by the probe through `App::assigns` and `assign` / `set_assign` on `dyn App` (journal/D-045) |
| The app menu (`up`+`down`) | `crates/oc-core/src/menu.rs` |
| Button debouncing and the `up`+`down` chord | `crates/oc-core/src/buttons.rs` |
| The system settings screen (`SETUP`, boot gestures) | `crates/oc-core/src/system.rs` (routing in `engine.rs`) |
| The calibration wizard | `crates/oc-core/src/wizard.rs` |
| ADC / DAC conversion and calibration | `crates/oc-core/src/calibration.rs` |
| Input presence derived from the signal | `crates/oc-core/src/presence.rs` |
| Persistent settings and the flash store | `crates/oc-core/src/settings.rs` (`MemoryFlash` in `testing.rs`) |
| The boot splash | `crates/oc-core/src/splash.rs` |
| Graphics primitives | `crates/oc-core/src/gfx/` (journal/D-030) |
| Text and fonts | `crates/oc-core/src/text/mod.rs`; generated `text/fonts.rs`; the `embedded-graphics` test adapter is `crates/oc-core/tests/common/mod.rs` |
| Where the loop's time goes | `crates/oc-core/src/profile.rs` |
| Deterministic mocks used by every `oc-core` test | `crates/oc-core/src/testing.rs` |
| The pinout (the only file allowed to name a pin) | `crates/oc-firmware/src/board.rs` (rule test: `xtask/tests/pin_rule.rs`) |
| The audio path and its mailboxes | `crates/oc-firmware/src/audio.rs`; DMA, ring, handlers: `audio_irq.rs` |
| The two USB serial ports and the log | `crates/oc-firmware/src/usb.rs`, `usb_log.rs` |
| The OLED's blocking initialisation bus, whose `flush` waits for the last bit | `crates/oc-firmware/src/framed_spi.rs` |
| The Teensy's settings flash (boot ROM calls) | `crates/oc-firmware/src/flash_store.rs` |
| The OLED's frame in chunks, the DAC's words, FIFO words | `crates/oc-drivers/src/ssd130x.rs` (`FrameStream`), `dac8565.rs`, `fifo.rs` |
| The probe protocol and the module's side of it | `crates/oc-protocol/src/lib.rs`, `server.rs` |
| Pre-flash validation | `crates/oc-image/src/validate.rs` |
| Flash contents from an ELF or a HEX; HalfKay reports | `crates/oc-image/src/flash.rs` (`hex.rs`, `boot.rs`); `crates/oc-halfkay/src/lib.rs` |
| Flash orchestration (confirm, invoke loader) | `xtask/src/flash.rs` |
| The browser probe and flasher | `crates/oc-web/src/lib.rs`, `web/`, `xtask/src/web.rs` |
| The C ABI linked into VCV Rack | `crates/oc-vcv-ffi/src/lib.rs` |
| The Rack plugin shim, and its stand-in for the chord | `vcv/OrnamentCrimeAlambic/src/Alambic.cpp` (`appendContextMenu`) |
| Memory map, boot sections | `crates/oc-firmware/MEMORY.md` |
| Release notes; version rule (Rack plugin = `2.<minor>.<patch>`) | `CHANGELOG.md` (checked by `xtask/tests/versions.rs`) |
| Step-0 foundation plan | `.junie/plans/oc-rust-firmware-foundation.md` |
