# Host tools for agents

Routed from `AGENTS.md`: the commands and the rules of the tools around the
firmware (flashing, probe, browser, VCV Rack, release, benchmarks). How to
run each verification level and what it proves: `TESTING.md`. User-facing
usage: `README.md`.

## Commands

```sh
# Firmware variants and flashing (journal/D-016, D-033)
cargo xtask build|hex --features ssd1306      # OLED override (default SH1106); ssd1309 also available
cargo xtask flash --features oled-spi-8mhz    # OLED at 8 MHz, DAC at 22 MHz (TESTING.md 9.3d)
cargo xtask flash --features settings-stress  # test firmware: 300 settings saves at power-up (TESTING.md 9.5c), never for a module in use
cargo xtask flash --dry-run                   # validate only, upload nothing
cargo xtask flash                             # validate, show SHA-256 digest, confirm, upload

# Live probe of a running module (journal/D-029)
cargo probe stats                             # also stream, out, screen, reboot
cargo probe app ratchet                       # the applets' model (D-043): also apps, get, set, act, select, assign (D-045)
scripts/profile-supersaw.sh 10 label         # SUPERSAW's profile through the probe (CV assigned by it)
INSTANCES=4 DETUNE_INPUT=cv1 scripts/profile-supersaw.sh 10 label   # four at once, one output each, all on CV1

# VCV Rack 2 plugin (needs a downloaded Rack SDK)
cargo xtask vcv build --rack-dir <path>
cargo xtask vcv install --rack-dir <path>     # build + drop into the Rack user plugin folder
cargo xtask vcv clean                         # wipe plugin + oc-vcv-ffi build artefacts

# Browser probe and flasher into dist/web/ (needs the wasm-bindgen CLI at the pinned version, journal/D-031)
cargo xtask web

# Measurements on the host
cargo run -p oc-core --example aliasing --release   # SUPERSAW's folding by FFT, C2-C8 (add a dir for WAVs)
cargo bench                                         # criterion; HTML report in target/criterion/report/index.html
cargo bench -p oc-core --bench graphics             # text and shapes against embedded-graphics, demo pages
```

**Release** (`.github/workflows/release.yml`, journal/D-034): pushing a tag
`vX.Y.Z` equal to the workspace version builds the firmware images onto a
GitHub release; `pages.yml` deploys `dist/web` to GitHub Pages. The Rack
plugin's version is `2.<minor>.<patch>` of the workspace's
(`xtask/tests/versions.rs`).

## Rules

* **The pre-flash gate stays a pure function over bytes**
  (`validate_image(&[u8]) -> Result<...>`), reachable from tests through
  `xtask/src/lib.rs`; never fold it back into `main.rs`.
  `xtask/tests/validate_image.rs` builds synthetic ELF buffers in memory and
  needs no firmware build. The gate lives in `oc-image` so the browser
  flasher runs the same one: keep that crate free of OS dependencies.
* **`oc-probe`'s command line** (`serialport`, `clap`, `png`, the `port`
  module) sits behind its default `cli` feature; the client and the text
  rendering must build without it, `oc-web` depends on them that way.
* **Restarting into the bootloader never depends on the probe protocol**:
  `cargo xtask flash` and, past a version refusal, `cargo probe reboot` set
  a module port to 134 baud, which every firmware since lot 3b3 honours
  (`oc_probe::port::restart_into_loader`). The probe messages a tool of any
  version needs (`Hello`, the version refusal, `RebootToLoader`) keep their
  rank: new variants of `Request`, `Reply` and `Error` go last
  (`crates/oc-protocol/tests/stable.rs`).
* **The HalfKay bootloader cannot be erased** by application firmware
  (separate chip, ROM); pressing PROGRAM always restores it. The flash
  validations save time and catch wrong-architecture images; they do not
  prevent bricking, which is not a real risk here.
* **Benchmarks are a regression guard, not a hardware measurement**
  (`TESTING.md`, Level 3); the hardware measurement is the engine's own
  profile (`docs/agents/firmware.md`, "Measuring costs").
* **The VCV Rack module is not checked systematically inside Rack**
  (`TESTING.md`, "What is not tested yet"), and CI does not build it: it
  needs the Rack SDK (`RACK_DIR` / `--rack-dir`).
