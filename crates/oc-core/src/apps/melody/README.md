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

The jacks are the defaults; the routing page moves each port as for any app
(README, *Apps and the app menu*).

Reset arms rather than jumps: a reset and a clock arriving together, or a
reset followed by a clock, both play step 1. A reset alone changes nothing
on the outputs. Before the first clock, the outputs carry step 1.

The outputs hold their levels between clocks. Settings that change the pitch
(`ROOT`, `SCALE`, `OCTAVE`, `CHORD`, `INVERT`, an edited note) take effect at
once, without waiting for a clock.

## What a step holds, and how it plays

A step holds a *place* in a window of pitches, drawn once, not a note
(journal/D-047, revision of 2026-11-27). The window turns every step's place
into a note at the outputs, so its three settings act at once on the whole
loop, without a clock and without redrawing anything:

* `SPREAD`, the window's width, 0 to 48 semitones: the melody stretches or
  narrows; at 0 it sits on one note;
* `CENTER`, where the window sits round the root, −100 % to +100 %: at 0 the
  root is in its middle, at +100 % the window starts on the root, at −100 %
  it ends on it; the melody slides without changing shape;
* `BIAS`, a curve over the places with the ends fixed, −100 % to +100 %: at
  0 straight; towards + the notes crowd towards the top of the window, the
  melody spends more time high; towards − towards the bottom.

Each of them keeps the order of the steps: a step higher than another stays
higher, so the melody's contour survives whatever is turned, and turning a
setting back gives the same melody again. While the window is narrow, steps
close in place may share a note of the scale; they part again when it
widens. Changing the scale and changing it back plays the same melody too.

What A plays for a step:

1. its place, bent by `BIAS`, placed by `SPREAD` and `CENTER` in the window,
   rounded to the semitone;
2. plus `TRANSPOSE` (with its CV input, as read at the clock that reached
   the step);
3. moved to the nearest note of `SCALE` (the lower of two equally near), so
   a note may fall a semitone outside the window;
4. placed from `ROOT`, plus `OCTAVE` octaves;
5. sent at 1 V/octave, C4 at 0 V, rounded to the millivolt (half a
   millivolt at most, about 0.6 cent).

The outputs reach from −3 V to +6 V; a note past either end stays at it.

## Chance and draws

Each time the clock reaches a step, the step is redrawn first with the
chance `PROB`:

* `PROB 0%`: the loop repeats as it is (the default);
* `PROB 100%`: every step is new each time round;
* in between: the melody changes a little each time round, like a Turing
  Machine.

How a new step is drawn depends on `JUMP`:

* **`JUMP OFF`**: an even place in the window, whatever the window is.
* **`JUMP NO REP`**: the same, redrawn when its note repeats the step
  before's, up to four times; in a window of one or two notes, a repeat may
  still happen.
* **`JUMP 1` to `7`**: a walk from the note the step before plays: 1 to
  `JUMP` degrees of the scale up or down, never none, within the window's
  notes (shorter when the window is narrower, the other way at a bound).
  The step takes the place that gives that note, so if the window is
  narrowed afterwards, the walk narrows with the rest of the loop.

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
scale's intervals from the note.

The inversion is an arrow beside the chord's name: ↑ raises the lower tone
an octave, ↓ lowers the upper one, a dash leaves the chord as stacked. In C
major on C, `TRIAD` gives E and G, ↑ G and the E above, ↓ the G below and E.
`CH1` always carries the lower tone and `CH2` the upper.

There are two tones because a route has four lanes, one per jack: with the
note and the gate, two are left for the chord (journal/D-047, revision of
2026-11-26).

## Settings

Three pages of four rows; the scroll bar left of the list shows which page
is on screen. Two rows hold two fields each: `KEY`, the root then the scale
(`C MAJOR`), and `CHORD`, the chord then its inversion's arrow. The left
encoder walks the fields one by one, left to right and row by row; the right
encoder changes the one picked. A row of one field is highlighted whole when
picked; in a row of two, only the field picked is boxed.

| Page | Row | Field | Range | Default | What it does |
|---|---|---|---|---|---|
| 1 | `LENGTH` | | 1–32 | 8 | steps in the loop; a shorter loop wraps at once, steps past the end keep their notes |
| 1 | `PROB` | | 0–100 % | 0 % | the chance a step is redrawn as the clock reaches it |
| 1 | `KEY` | `ROOT` | C–B | C | the scale's root; wraps round |
| 1 | `KEY` | `SCALE` | 13 scales | `MAJOR` | `CHROMATIC`, `MAJOR`, `MINOR`, `DORIAN`, `PHRYGIAN`, `LYDIAN`, `MIXOLYD`, `LOCRIAN`, `HARM MIN`, `PENTA MAJ`, `PENTA MIN`, `BLUES`, `WHOLE`; wraps round |
| 1 | `CHORD` | `CHORD` | 5 chords | `TRIAD` | the chord on `CH1` and `CH2`; wraps round |
| 1 | `CHORD` | `INVERT` | ↓, –, ↑ | – | the inversion (*Chords*) |
| 2 | `OCTAVE` | | −3 to +3 | 0 | octaves the whole moves by |
| 2 | `TRANSPOSE` | | −24 to +24 | 0 | semitones added before quantising, so the melody moves within the scale |
| 2 | `GATE` | | 1–99 % | 50 % | the gate's length, in percent of the clock's period (10 ms until two clocks give the rate) |
| 2 | `REROLL` | | `PRESS` | — | an action: the right encoder's click draws the whole sequence anew, and the row reads `DONE` for half a second |
| 3 | `JUMP` | | `OFF`, `NO REP`, 1–7 | `OFF` | how a step is drawn: an even place, one whose note never repeats the step before's, or a walk of up to so many degrees (*Chance and draws*) |
| 3 | `SPREAD` | | 0–48 semitones | 24 | the window's width: the loop stretches or narrows at once |
| 3 | `CENTER` | | −100 to +100 % | 0 % | the window's place round the root: 0 centred on it, +100 % starting on it, −100 % ending on it |
| 3 | `BIAS` | | −100 to +100 % | 0 % | the curve over the places: towards + the notes crowd high, towards − low; the ends stay |

The probe names each field as its own parameter (`ROOT`, `SCALE`, `CHORD`,
`INVERT` from −1 to +1, `JUMP` by its names).

## Editing the steps

A click on the **left encoder** enters the sequence; `EDIT` replaces
`MELODY` in the title, and no setting stays highlighted.

* The left encoder picks a step, framed on screen, round the loop.
* The right encoder moves the note it plays to the next or previous note of
  the scale, within the window's notes; the step takes the place in the
  window that gives it, so the edit bends with the window like the rest. In
  a window of one note (`SPREAD 0`) it does nothing.
* A right click redraws that step alone, by the rules above.
* Another left click leaves.

The clock keeps running while you edit: the edited step plays its new note
the next time it comes round, or at once if it is playing. With `PROB`
above 0 %, an edited step may be redrawn when the clock reaches it; set
`PROB 0%` to keep edits.

## CV inputs

Six settings take an input: `LENGTH`, `PROB`, `TRANSPOSE`, `SPREAD`,
`CENTER` and `BIAS`. A right click on one opens its input submenu, shared
with the other apps (README, *Assigning an input*): which `CV`, its scale,
shift, bounds and effect. `LENGTH`, `PROB` and `TRANSPOSE` read their input
at each clock, so a step plays as it started; the window's three act at
once, so an LFO on `SPREAD` breathes the melody wider and narrower while it
plays.

| Setting | With `ADD` | `SET` at 0 V |
|---|---|---|
| `LENGTH` | 4 steps a volt | 1 step |
| `PROB` | 20 % a volt | 0 % |
| `TRANSPOSE` | 12 semitones a volt (1 V/octave) | 0 |
| `SPREAD` | 12 semitones a volt | 0 |
| `CENTER`, `BIAS` | 20 % a volt | 0 % |

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
* **Four rows of settings**, the selected one highlighted; at the right of
  a row, its input (`+CV1`), or a small ring for a setting that can take one
  and has none.
* **The sequence**, at the bottom: one column per step of the loop, as wide
  as the screen allows (4 pixels at 32 steps). Each step's mark sits as high
  as the note it plays between the window's lowest and highest notes. The
  step playing is filled down to the bottom, a small playhead at the top of
  its column in inverse video (no row is kept for it above the marks); in the
  editor, the step edited is framed. The marks follow the window as it
  bends, `TRANSPOSE` included.

## Through the probe

The parameters are those of the table above, by name, except `REROLL`.
`STEP` (from 1) and `NOTE` (the MIDI note A plays) can be read; `REROLL`
and `RESET` are actions.

```sh
cargo probe app melody            # its parameters, values, what it plays
cargo probe set melody prob 25
cargo probe get melody note
cargo probe act melody reroll
```

The steps' notes are not visible to the probe, and are not saved: the
sequence starts from the same seed at each start.
