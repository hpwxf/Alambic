# `MELODY` — random but controllable melodies

`MELODY` plays a looping sequence of notes that a clock steps through and
chance rewrites, in the spirit of Vermona's meloDICER, Noise Engineering's
Mimetic Digitalis and Mutable Instruments' Marbles (journal/D-047). It sits
in the `SEQUENCE` column of the app menu.

The sequence can stay fixed, drift a little, or change completely. Notes are
drawn from a scale, so the result is always in key. A carries the note, B
and C a chord built on it in the same scale, D a gate.

## Patching

| Port | Default jack | What it carries |
|---|---|---|
| `CLOCK` (input) | `TR1` | each rising edge plays the next step |
| `RESET` (input) | `TR2` | a rising edge arms the sequence: the next clock plays step 1 |
| `NOTE` | A | the step's note, 1 V/octave, C4 at 0 V |
| `CH1` | B | the chord's lower tone, 1 V/octave |
| `CH2` | C | the chord's upper tone, 1 V/octave |
| `GATE` | D | 5 V for `GATE` percent of the clock's period, then 0 V |

Every port goes through the app's **routing page** (a long press of `up`;
README, *Apps and the app menu*). To free D for another app, set
`GATE` to `OFF`. To have the gate on C, set `CH2` to `OFF` and `GATE` to `C`.
`CLOCK` and `RESET` can read any trigger jack, shared with other apps: a
`RATCHET` in another slot can clock `MELODY` from the same `TR1`.

Reset arms rather than jumps: a reset and a clock arriving together, or a
reset followed by a clock, both play step 1. A reset alone changes nothing
on the outputs. Before the first clock, the outputs carry step 1.

The outputs hold their levels between clocks. Settings that change the pitch
(`SCALE`, `ROOT`, `OCTAVE`, `CHORD`, `INVERT`, an edited note) take effect at
once, without waiting for a clock.

## What a step holds, and how it plays

A step holds a note in semitones above the root, from 0 to 48 (four
octaves). The note is stored as drawn or edited, not quantised: the scale
applies only on the way out. Changing the scale and changing it back plays
the same melody again.

What A plays for a step:

1. the step's note plus `TRANSPOSE` (with its CV input, as read at the clock
   that reached the step);
2. moved to the nearest note of `SCALE` (the lower of two equally near);
3. placed from `ROOT`, plus `OCTAVE` octaves;
4. sent at 1 V/octave, C4 at 0 V, rounded to the millivolt (half a
   millivolt at most, about 0.6 cent).

The outputs reach from −3 V to +6 V; a note past either end stays at it.

## Chance and draws

Each time the clock reaches a step, the step is redrawn first with the
chance `PROB`:

* `PROB 0%`: the loop repeats as it is (the default);
* `PROB 100%`: every step is new each time round;
* in between: the melody changes a little each time round, like a Turing
  Machine.

How a new note is drawn depends on `JUMP`:

* **`JUMP OFF`**: the note is drawn from the scale's notes between the root
  and `RANGE` octaves above it.
  * `SPREAD 100%` gives every note the same chance.
  * Below that, notes gather round `BIAS` (0% the bottom of the range, 100%
    the top), within `SPREAD` percent of the range either side, in a
    triangle: those nearest the bias come most often.
  * `SPREAD 0%` always gives the note at `BIAS`.
  * `REPEAT OFF` redraws a note equal to the step before's, up to four
    times; with a narrow spread, a repeat may still happen.
* **`JUMP 1` to `7`**: a walk from the step before. The note moves up or
  down by 1 to `JUMP` degrees of the scale, never by none. A move that would
  leave the range goes the other way. `SPREAD`, `BIAS` and `REPEAT` have no
  effect: the probe reports them inactive.

`REROLL` draws the whole sequence anew by the same rule, all 32 steps, so a
loop lengthened later also has fresh notes. In the editor, the right click
redraws one step.

The draws come from one random generator that always starts from the same
seed: the same patch played the same way replays the same melody on the
module, the simulator and VCV Rack.

## Chords

`CH1` and `CH2` carry two tones stacked on A's note in the scale, so the
chord is the scale's own: in C major, `TRIAD` on D gives D minor (F, A), on
G gives G major (B, D).

| `CHORD` | Tones above the note |
|---|---|
| `TRIAD` | the third and the fifth |
| `7TH` | the third and the seventh (the fifth left out) |
| `SUS2` | the second and the fifth |
| `SUS4` | the fourth and the fifth |
| `OCT` | an octave down and an octave up |

"Third" means two degrees of the scale up, so a pentatonic or whole-tone
scale gives its own flavour of chord. The chromatic scale has no thirds of
its own (two semitones up would be a second), so it borrows the major
scale's intervals from the note. `INVERT 1` raises the lower tone an octave.
`CH1` always carries the lower tone and `CH2` the upper.

There are two tones because a route has four lanes, one per jack: with the
note and the gate, two are left for the chord (journal/D-047, revision of
2026-11-26).

## Settings

The left encoder picks a setting, the right one changes it. Four pages of
four; the scroll bar left of the list shows which page is on screen.

| Page | Setting | Range | Default | What it does |
|---|---|---|---|---|
| 1 | `LENGTH` | 1–32 | 8 | steps in the loop; a shorter loop wraps at once, steps past the end keep their notes |
| 1 | `PROB` | 0–100 % | 0 % | the chance a step is redrawn as the clock reaches it |
| 1 | `SCALE` | 13 scales | `MAJOR` | `CHROMATIC`, `MAJOR`, `MINOR`, `DORIAN`, `PHRYGIAN`, `LYDIAN`, `MIXOLYD`, `LOCRIAN`, `HARM MIN`, `PENTA MAJ`, `PENTA MIN`, `BLUES`, `WHOLE`; wraps round |
| 1 | `ROOT` | C–B | C | the scale's root; wraps round |
| 2 | `OCTAVE` | −3 to +3 | 0 | octaves the whole moves by |
| 2 | `TRANSPOSE` | −24 to +24 | 0 | semitones added before quantising, so the melody moves within the scale |
| 2 | `CHORD` | 5 chords | `TRIAD` | the chord on `CH1` and `CH2` |
| 2 | `INVERT` | 0–1 | 0 | raises the chord's lower tone an octave |
| 3 | `RANGE` | 1–4 oct | 2 oct | the octaves a drawn note spans above the root |
| 3 | `SPREAD` | 0–100 % | 100 % | how widely draws spread round `BIAS`; 100 % even |
| 3 | `BIAS` | 0–100 % | 50 % | where in the range draws gather |
| 3 | `JUMP` | `OFF`, 1–7 | `OFF` | a walk of up to so many degrees from the step before |
| 4 | `REPEAT` | `ON` / `OFF` | `ON` | whether a draw may repeat the step before |
| 4 | `GATE` | 1–99 % | 50 % | the gate's length, in percent of the clock's period (10 ms until two clocks give the rate) |
| 4 | `REROLL` | — | — | the right encoder's click draws the whole sequence anew |

## Editing the steps

A click on the **left encoder** enters the sequence; `EDIT` replaces
`MELODY` in the title, and no setting stays highlighted.

* The left encoder picks a step, framed on screen, round the loop.
* The right encoder moves its note to the next or previous note of the
  scale, from 0 to 48.
* A right click redraws that step alone, by the rules above.
* Another left click leaves.

The clock keeps running while you edit: the edited step plays its new note
the next time it comes round, or at once if it is playing. With `PROB`
above 0 %, an edited step may be redrawn when the clock reaches it; set
`PROB 0%` to keep edits.

## CV inputs

Six settings take an input: `LENGTH`, `PROB`, `TRANSPOSE`, `RANGE`, `SPREAD`
and `BIAS`. A right click on one opens its input submenu, shared with the
other apps (README, *Assigning an input*): which `CV`, its scale, shift,
bounds and effect. Inputs are read at each clock, so a step plays as it
started.

| Setting | With `ADD` | `SET` at 0 V |
|---|---|---|
| `LENGTH` | 4 steps a volt | 1 step |
| `PROB` | 20 % a volt | 0 % |
| `TRANSPOSE` | 12 semitones a volt (1 V/octave) | 0 |
| `RANGE` | 1 octave a volt | 1 octave |
| `SPREAD`, `BIAS` | 20 % a volt | 0 % |

`MUL` multiplies the panel's value by the input over 5 V. No setting has
an input at start, `TRANSPOSE` included. Give `TRANSPOSE` a `CV` with `ADD`
and a keyboard's or a sequencer's V/oct on it transposes the melody, which
stays in the scale. The screen marks an
assigned setting with its input (`+CV1`), and with `SET` shows `=CV1` in
place of the value.

## The screen

* **Title bar**: `MELODY` (or `EDIT`), the step playing (or edited) out of
  the loop's length (`06/16`; `--/16` while armed), and the name of the note
  A plays (or the edited step plays) on the right.
* **Four rows of settings**, the selected one highlighted, an assigned
  input on the right.
* **The sequence**, at the bottom: one column per step of the loop, as wide
  as the screen allows (4 pixels at 32 steps). Each step's mark sits as high
  as its note in the scale, scaled to `RANGE` or the highest note held. The
  step playing is filled down to the bottom, a small playhead at the top of
  its column in inverse video (no row is kept for it above the marks); in the
  editor, the step edited is framed. The marks show the notes as held, before
  `TRANSPOSE`.

## Through the probe

The parameters are those of the table above, by name, except `REROLL`.
`STEP` (from 1) and `NOTE` (the MIDI note A plays) can be read; `REROLL`
and `RESET` are actions.

```sh
cargo probe app melody            # its parameters, values, what it plays
cargo probe set melody prob 25
cargo probe get melody note
cargo probe act melody reroll
cargo probe route @0 gate=off     # D free for another app
```

The steps' notes are not visible to the probe, and are not saved: the
sequence starts from the same seed at each start.
