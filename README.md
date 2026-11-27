# Alambic — Ornament & Crime firmware in Rust

A Rust firmware for the [Ornament & Crime](https://ornament-and-cri.me/) Eurorack
module (TLM Audio build, **Teensy 4.0 / NXP i.MX RT1062**), designed to be more
approachable than the reference *Phazerville* firmware.

The distinguishing goal is not musical richness but **verifiability**: the same
behaviour runs on hardware, in a native simulator, and inside VCV Rack 2.

> Status: the cross-compilation chain, the shared core, the simulator, the
> Teensy firmware, the safe flashing tool and the VCV Rack 2 module are in
> place, with host tests throughout. On a real module (TLM build), the SH1106
> OLED, the USB log and probe ports, the encoders and buttons, the CV inputs
> and outputs, the trigger inputs, the calibration wizard, the settings flash
> and the audio path have been checked (Level 9 of `TESTING.md`). Not checked:
> the DAC8565's simultaneous update; the inputs read within about 20 mV only.
> The VCV Rack plugin builds against the real Rack SDK and loads in Rack, but
> its panel and controls have not been checked systematically there, nor its
> audio listened to. The Renode boot smoke test is not written.

## Layout

| Path                 | Role                                                              |
|----------------------|-------------------------------------------------------------------|
| `crates/oc-core`     | `no_std`, `forbid(unsafe_code)` core: platform traits, engine, UI  |
| `crates/oc-drivers`  | `no_std` peripheral protocols (DAC8565, OLED controllers, triggers, shared SPI bus) |
| `crates/oc-firmware` | Teensy 4.0 binary; the only crate that touches registers           |
| `crates/oc-protocol` | `no_std` messages and server of the live probe (postcard over COBS) |
| `crates/oc-probe`    | Host tool talking to a running module over its probe port          |
| `crates/oc-image`    | Pre-flash image gate; the flash contents of a validated ELF or HEX |
| `crates/oc-halfkay`  | The Teensy bootloader's write protocol, as reports to send          |
| `crates/oc-web`, `web/` | Browser probe and flasher: WebAssembly core and a static page   |
| `crates/oc-sim`      | Native backend and terminal UI, with a deterministic virtual clock |
| `crates/oc-vcv-ffi`  | `staticlib` exposing a C ABI to the VCV Rack 2 module              |
| `vcv/OrnamentCrimeAlambic` | The Rack SDK plugin shim: module declaration and widget only, no behaviour |
| `crates/oc-buildinfo` | Build-script helper: the build identifier (see below)             |
| `xtask`              | Build, packaging, flashing, browser and VCV plugin automation       |

`oc-core` holds **all** behaviour. The three backends only differ in how they
read and write signals, which is what makes the simulator a meaningful proxy
for the hardware.

## Apps and the app menu

The module has four slots, each running an app on its own outputs
(journal/D-046), one of them on the screen and the panel at a time; a slot
nobody uses runs `NOTHING`, which holds no output and says how to fill it. A
short press of `up` or `down` moves to the slot on the left or right, empty
ones too, from the last round to the first, on the routing page too. A
long press of `up` (one second) opens the shown app's **routing page**, and
closes it again; a long press of `down` does nothing yet, kept for the app's
options page to come (journal/D-046, revision of 2026-11-26). On the page, the left encoder
picks a row, the right one turns it. An output (`A`, `B`, `SUB` for
`SUPERSAW`) steps through `OFF` and the jacks `A` to `D`, skipping those
another app holds; an input the app reads by itself (`RATCHET`'s `CLOCK`,
`SCOPE`'s `IN`) steps through the four jacks of its kind, which apps may
share. The right encoder stops at the ends instead of wrapping round: a
quick turn down lands on `OFF`, the first place, and one up on the last free
jack. An app's CV inputs are set in their own submenu: one of the four
jacks, or past `CV4` an output port of another app that carries a level
(journal/D-048), read without a cable or a converter, routed to a jack or
not. A jack modulates the setting (scale, shift, bounds, effect); a port is
an assignment, strictly: the setting takes the value the port carries, as
`SET` reads it, and its own value no longer shows. `SUPERSAW`'s `NOTE`
assigned `1:NOTE` (slot 1, the port's name; `=1:NOTE` on its screen) plays
`MELODY`'s pitches themselves, C4 at 0 V. A port that
carries sound (`SUPERSAW`, `TONE`) and the app's own are not offered; the
level read is the one the other app published on the tick before. Apps no longer use `up` and `down`
themselves. A column of marks left of the title, as wide as a list's scroll
bar, shows the slots on the app's screen and its routing page: a dot for
each that runs an app, nothing for an empty one, the one shown across the
column.

**Holding `up` and `down` together** opens the app menu, in two columns,
each with its scroll bar: the categories on the left (`TOOLS`: `DIAGNOSTIC`,
`SCOPE`, `GFX DEMO`; `TRIGGERS`: `RATCHET`; `SOUND`: `TONE`, `SUPERSAW`;
`SEQUENCE`: `MELODY`; `SYSTEM`: `NOTHING`, `SETUP`), the apps of the highlighted one on the right.
The left encoder picks the category, the right one the app in it; `up`/`down`
step to the previous or next app across the categories. The slots' marks stay left of the title, showing which
slot the app picked will take. The right encoder
press launches the highlighted app in the place of the one shown, which starts
afresh, and holding both again closes the menu without changing anything.
An app put in a slot takes the outputs no app uses, in order (`RATCHET`
beside `SUPERSAW` gets D); `NOTHING`, last of the apps, empties the slot shown
and frees its outputs, which go to 0 V. The running
app keeps tracking its inputs and driving its outputs while the menu is up, so
opening it never interrupts a patch. In the simulator press `m`; in VCV Rack
the chord is unreachable with a mouse, so right-click the module and choose
*Open/close app menu*.

| App          | What it does                                             |
|--------------|----------------------------------------------------------|
| `DIAGNOSTIC` | the I/O diagnostic screen the module boots into          |
| `SCOPE`      | a scrolling view of `CV1`, buffered to all four outputs  |
| `RATCHET`    | bursts of triggers on B after each rising edge on `TR1` (below) |
| `TONE`       | a test tone at one pitch on the four outputs, a sine on A, a triangle on B, a sawtooth on C and a square on D (the last two alias, as a test tone may); the left encoder picks `NOTE` (C1 to C8, A4 = 440 Hz at start) or `LEVEL` (the peak level, 0 to 3 V around 0 V), the right one changes it, its click opens the setting's input submenu as in `SUPERSAW`; no input at start, so the tone stays clean; the pitch played is in the title row. The module plays it at 47 619 Hz, the DAC written by DMA (journal/D-033), VCV Rack at its own sample rate |
| `SUPERSAW`   | seven detuned band-limited sawtooths around one pitch (below) |
| `MELODY`     | a looping sequence of random but controllable notes on A, quantised to a scale, a two-tone chord on B and C, a gate on D (below) |
| `GFX DEMO`   | the graphics primitives on five pages (fonts, knobs, curves, sequencer, meters); left encoder turns the pages, its click moves to the next knob, right encoder the value; outputs A–D carry the simulated LFO, gate, knob and needle |

Because holding both buttons is a deliberate gesture, `up` and `down` fire
their own action on **release**, and not at all when the chord formed while they
were down. That is the only way the first of the two presses cannot act before
the chord is even detectable.

### `RATCHET`

Each rising edge on `TR1` is copied to A and starts a burst of triggers on B
(journal/D-032). The left encoder picks a setting, the right one changes it,
the left encoder's click fires a burst by hand.

* `COUNT`, the repeats in a burst, 1 to 32; one more detent gives `GATE`:
  repeats for as long as `TR1` stays high.
* `MODE` picks how the repeats are placed, between two to compare:
  * `FIRST`: the time setting is the first interval; the following ones
    change geometrically, the last being the first times 2^`SHAPE`;
  * `SPAN`: the time setting, then labelled `SPAN`, is the time over which
    the repeats spread; `SHAPE` crowds them towards its end or its start.
* `FIRST` or `SPAN`, the time setting: a duration in milliseconds, or with
  `SYNC ON` a fraction of `TR1`'s period (×2, ×1.5, ×4/3, ×1, ×3/4, ×2/3,
  /2 … /32; for a span, ×1 fills the step). The rate of `TR1` shows in BPM,
  and a dotted line on the timeline marks where the next trigger is due.
* `SHAPE`, −3.0 to +3.0, an abstract curve rather than a unit: from an
  exponential one at −3.0, through a straight line at 0 (even repeats), to a
  logarithmic one at +3.0. Below 0 the burst accelerates, above 0 it slows
  down.
* `PROB`, the chance each repeat fires; `PASSTHRU` copies the incoming
  trigger to B too.

Each burst setting can take an input (*Assigning an input*, below).

### `SUPERSAW`

Seven detuned band-limited sawtooths around one pitch (journal/D-037): A and
B carry all seven, or the left and right halves with `STEREO ON`; C a
sawtooth an octave below; D rests at 0 V. The left encoder picks a setting
over two pages, the right one changes it:

* page 1: `NOTE` (C1–C8, A2 at start), `DETUNE` and `MIX` (0–100 %),
  `SHAPE` (from the sawtooth to a square, 0–100 %, its waveform drawn beside
  it) and `WIDTH` (the width of that square's pulse, 50 down to 5 %);
* page 2: `LEVEL` (0–3 V) and `STEREO`.

Every setting but `STEREO` takes an input (*Assigning an input*,
below). `NOTE` takes `CV1` at start, added at one volt per octave (`SET` from
C4 at 0 V, `MUL` on the frequency); the others none. `DETUNE`, `MIX` and
`SHAPE` move 20 % a volt, `WIDTH` its whole course over 0–5 V, `LEVEL` 0–3 V
over 0–5 V, `MUL` on it a VCA. The screen shows the pitch played, each
setting's input (`+CV1`) and the seven voices: where each sits in pitch and
how loud.

### `MELODY`

A looping sequence of random but controllable notes, in the spirit of the
meloDICER, Mimetic Digitalis and Marbles (journal/D-047). `TR1` clocks it,
`TR2` arms its first step; each step reached is redrawn with the chance
`PROB`, from a scale over a range or by a walk from the step before. A
carries the note at one volt per octave (C4 at 0 V), B and C a chord stacked
in the scale, D a gate; the routing page sends each where it should go. The
bottom of the screen shows the loop with a playhead, and a click on the left
encoder edits its steps there.

How it plays, every setting and its input:
[`crates/oc-core/src/apps/melody/README.md`](crates/oc-core/src/apps/melody/README.md).

### Assigning an input

In `RATCHET`, `SUPERSAW` and `MELODY`, a click on the right encoder opens, over the main
screen, the input of the highlighted setting (journal/D-035, D-036): which
`CV`, a `SCALE` (−200 to +200 %) and a `SHIFT` (±10 V) applied to it,
`MIN` / `MAX` bounds, and its `EFFECT` on the panel's value: `SET` (follows
it, `=CVn` in place of the value), `ADD` (`+CVn`) or `MUL` (multiplies it,
5 V leaving it unchanged, `*CVn`). A bar shows the input's signal (live, and
the range seen), another the setting it moves; `CENTER` sets the scale and
shift that carry the range seen onto the setting's usable range. The right
encoder's click closes it. On the main screen, a setting that can take an
input shows it at the right of its row (`+CV2`), or a small ring while it
has none, so those an input can move stand out at a glance.

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
| left encoder, tap    | next output mode                           |
| left encoder, hold 1 s | reset the trigger counters               |
| right encoder, turn  | change the offset by 100 mV per detent     |
| right encoder, press | set the offset back to 0 V                 |
| up / down            | counted only: they move between apps       |
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
and `o` (or `0`) replays that same boot sequence at any time, exactly like a
host's "Initialize" action.

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
| `↑` / `↓`      | the module's up / down: the slot on the left / right (on release) |
| `m`            | press up and down together: open or close the app menu |
| `Shift+↑` / `Shift+↓` | hold up / down, again to release: held a second, a long press (see below) |
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
A long press is formed the same way, as a terminal sends no key release and
holding a key down would only repeat it: `Shift+↑`, a second of real time,
then `Shift+↑` again opens the shown app's routing page, and closes it. In
the app menu, `m` closes it; the routing page has no meaning there. The encoders' presses and turns do
what each app gives them; the panel's gestures are under *Apps and the app
menu*.

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
golden snapshot of the resulting screen; how they are checked and
regenerated: `TESTING.md`, Level 4.

## Performance

`cargo bench` times the tick, the applets, the conversions and the graphics
on the host (`TESTING.md`, Level 3): a regression guard, not the module's
figures, since the target is a 600 MHz Cortex-M7 with the code in ITCM.
Screen rendering costs about three orders of magnitude more than the state
update, and an OLED panel cannot show more than about 60 frames per second
anyway, so the redraw rate is decoupled from the control loop through
`Engine::set_render_interval`.

On the module itself, the engine times every stage of the loop on the core's
cycle counter (journal/D-029), the audio interrupt's included, and the probe
gives it as a table:

```sh
cargo probe stats              # mean and longest time of each stage, loop jitter
cargo probe stats --fresh 10   # the same over the next ten seconds only
cargo probe stream --every 10  # inputs, codes, outputs and triggers every 10 ticks
cargo probe out A -2000        # hold output A at -2 V until Enter, printing the inputs
cargo probe screen --png s.png # capture the screen as the module shows it
cargo probe reboot             # restart into the bootloader (any firmware version: at 134 baud if need be)

cargo probe apps               # the applets, the running one marked
cargo probe app ratchet        # its parameters and values, observables, actions
cargo probe set ratchet prob 50  # as the panel would, within its bounds
cargo probe get supersaw freq  # a parameter or what it plays now
cargo probe act ratchet burst  # an action, here a burst as the left click starts one
cargo probe select tone        # put it in the slot shown; `select tone @2` in slot 2
cargo probe assign supersaw detune input=cv2 effect=add  # a parameter's CV input, field by field
cargo probe assign supersaw detune --center 5  # fit it to what CV2 carries, as CENTER does
cargo probe assign @1 note input=1:note  # another app's port, as the submenu names it: slot 1 (@0), NOTE
cargo probe add ratchet        # put RATCHET in the first empty slot, on the free outputs
cargo probe instances          # the four slots, @0 to @3, where their ports go
cargo probe route @0 sub=off   # its routing page from afar: sub=off, burst=d, clock=tr2
cargo probe set @1 prob 50     # @N names one instance; a name, the first of that app
cargo probe focus @1           # show it on the screen; `remove @1` empties the slot
```

`cargo probe` is an alias of `cargo run -q -p oc-probe --`. The tool finds the
module's probe port by itself (`--port` to name it); it is the second of the
module's two USB serial ports, the first being the text log. Closing the tool
ends the session, which releases every held output.

### In the browser

The same probe, and a flasher, run in a web page (journal/D-031), for anyone
without the Rust toolchain. Each version tag publishes it at
**https://hpwxf.github.io/Alambic/**, and attaches the firmware to the
[GitHub release](https://github.com/hpwxf/Alambic/releases): the ELF and HEX
for the stock SH1106 panel, the HEX for SSD1306 and SSD1309 panels, and their
SHA-256 (journal/D-034). To build the page yourself:

```sh
cargo xtask web                              # builds dist/web/
python3 -m http.server -d dist/web 8000      # then open http://localhost:8000
```

It needs Chrome or Edge on a desktop (Web Serial and WebHID; neither Firefox
nor Safari has them) and a secure context: `localhost` or https. The probe
side does what `cargo probe` does: screen capture (live too), profile, sample
stream, held outputs, and an *Apps* panel that reads and sets each applet's
settings and their CV inputs as `cargo probe set` and `assign` do, `CENTER`
included: it reads the input from the stream for a few seconds and fits the
scale and shift with the module's own function. The flash side takes the firmware **ELF**, with the
same checks as `cargo xtask flash`, or an Intel **HEX** (the CI artefact,
say) with reduced checks ([ELF or HEX](#elf-or-hex)); it shows the file's
SHA-256 and writes it through the bootloader (HalfKay) after an explicit
confirmation; with the probe
connected it restarts the module into the bootloader itself. `cargo xtask
web` needs the `wasm-bindgen` command line at the version `Cargo.lock` pins
(it says which, and how to install it). Both sides have been tried on a
module from Chrome on macOS; `cargo xtask flash` stays the reference for
development. Each build stamps the page's scripts with a fingerprint of
what it built (`app.js?v=…`), so a plain reload picks up a rebuilt page:
Chrome would otherwise keep the previous `app.js` under a new page, whose
new buttons then do nothing.

## Requirements

The toolchain is pinned by `rust-toolchain.toml`; `rustup` installs everything
on first build, including the `thumbv7em-none-eabihf` target and `llvm-tools`.

## Everyday commands

```sh
cargo test              # host crates only (see below)
cargo clippy --all-targets --all-features -- -D warnings
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
cargo xtask flash              # validate, show the digest, ask, restart the module into its bootloader, upload
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

It then prints the loadable size, the entry point, the initial stack pointer
and the SHA-256; how to read them: `TESTING.md`, Level 8.

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

The plugin builds and links against a real Rack SDK (2.6.x) and loads in
Rack, but it has not been checked systematically there, nor its audio
listened to: treat the panel layout and the encoder-emulation-via-knob
interaction as a first pass (`TESTING.md`, *What is not tested yet*).

## Testing

Every verification level — static checks, host unit and property tests,
criterion benchmarks, simulator scenarios, the pre-flash image gate, and the
step-by-step **manual hardware validation checklist** — is documented with
its exact command and what it does (and does not) prove in
[`TESTING.md`](TESTING.md). Start there before touching a real module.

## Before the first flash

The firmware has run on a real module (TLM build), and Level 9 of
`TESTING.md` has been run on it: screen, panel, USB ports, CV inputs and
outputs, trigger inputs, calibration, settings and the audio path. On
another module, check with the diagnostic applet before trusting it:

* **the calibration.** A module starts from the reference firmware's
  factory calibration (`Calibration::FACTORY` in
  `crates/oc-core/src/calibration.rs`: both stages invert, 0 V input at ADC
  code 2730, 10 V over each converter's range); the exact gain and offset are
  per-unit properties, measured by the calibration wizard (*System settings*
  above) and saved in the module's flash.
* **the CV inputs and the trigger inputs** on another build (pin mapping,
  polarity, levels).
* **the OLED controller** on other builds. SH1106 is the default and is what
  the TLM build carries; build or flash with `cargo xtask … --features ssd1306`
  or `--features ssd1309` if the screen stays blank or only a band of rows
  lights up.

The pinout itself is in one reviewed table in `crates/oc-board/src/lib.rs`,
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

The lines to expect, and what the onboard LED's flash groups mean before
SPI claims its pad (a Morse SOS is a panic): `TESTING.md` 9.1.

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
table in `crates/oc-board/src/lib.rs`, not by the upload tool.

## Licence

Dual-licensed under MIT or Apache-2.0, at your option.
