//! The Melody applet (journal/D-047): a looping sequence of random but
//! controllable notes, in the spirit of the meloDICER, Mimetic Digitalis and
//! Marbles.
//!
//! Each rising edge on `TR1` (`CLOCK`) moves to the next step, redrawing it
//! first with the chance `PROB` ([`sequence`]); a rising edge on `TR2`
//! (`RESET`) arms the sequence, so the next clock plays its first step. A
//! carries the step's note, quantised to `SCALE` from `ROOT`, moved by
//! `OCTAVE` and `TRANSPOSE` (which takes `CV1` at one volt per octave from
//! the start); B, C and D carry a chord stacked in the scale on it
//! ([`scale::chord`]), or D a gate for `GATE` percent of the clock's period.
//! The outputs hold their levels between clocks: one change a step, nothing
//! per sample.
//!
//! The settings whose input can move them read it at each clock, so a step
//! plays as it started; the others act at once, a new scale too.
//!
//! Panel: the left encoder picks a setting, the right one changes it; the
//! right switch opens the setting's input submenu, or rerolls the whole
//! sequence on `REROLL`. The left switch enters the sequence at the bottom of
//! the screen: there the left encoder picks a step, the right one moves its
//! note through the scale, the right switch redraws that step, and the left
//! switch leaves. `up` and `down` stay the panel's (journal/D-046).

pub mod scale;
pub mod sequence;

use core::fmt::Write as _;

use self::scale::{CHORD_NAMES, Chord, MAX_INVERSION, ROOT_NAMES, SCALE_NAMES, Scale, chord};
use self::sequence::{Draw, MAX_JUMP, MAX_NOTE, MAX_RANGE, MAX_STEPS, Sequence};
use crate::apps::param::{ParamSpec, ParamValue, Refused, Unit};
use crate::apps::sound::Params;
use crate::apps::tone::note_name;
use crate::apps::{App, AppId, InputKind, InputPort, InputSnapshot, Port, TickContext};
use crate::fmt::TextBuf;
use crate::framebuffer::{FrameBuffer, HEIGHT_I32, ROW_HEIGHT, WIDTH_I32};
use crate::gfx::controls::{LIST_LEFT, SCROLL_WIDTH, TITLE_LEFT, row_highlight, scroll_bar};
use crate::gfx::{self, Ink, Point};
use crate::modulation::editor::Param;
use crate::modulation::slots::Modulations;
use crate::modulation::{Assign, Effect, multiplied};
use crate::platform::{Button, CV_CHANNELS, CvChannel, MilliVolts};
use crate::tempo::Tempo;
use crate::text::fonts::FONT_5X8;
use crate::text::{self, Alignment, TextStyle};

/// A gate's level on D.
pub const GATE_MV: MilliVolts = 5_000;

/// A gate's length while the clock's period is unknown.
pub const UNKNOWN_GATE_MICROS: u32 = 10_000;

/// The shortest gate.
const MIN_GATE_MICROS: u32 = 1_000;

/// The MIDI note at 0 V: C4, as `SUPERSAW` and `TONE` read their `NOTE`.
const ZERO_VOLT_NOTE: i32 = 60;

/// Settings shown per page, under the title bar and above the sequence.
const PAGE_ROWS: usize = 3;

/// Pages of settings.
const PAGES: usize = Setting::ALL.len().div_ceil(PAGE_ROWS);

/// Top of the sequence: the playhead's row, the notes below it.
const SEQUENCE_TOP: i32 = 34;

/// Top of the notes, under the playhead.
const NOTES_TOP: i32 = SEQUENCE_TOP + 4;

/// A note's mark: two rows.
const MARK_HEIGHT: i32 = 2;

/// Right edge of the value column, left of the input column.
const VALUE_RIGHT: i32 = 98;

/// Steps at start.
const DEFAULT_LENGTH: u8 = 8;

/// `index` moved by `turns` among `len`, wrapping round.
fn wrapped(index: usize, len: usize, turns: i32) -> usize {
    let len = i32::try_from(len).unwrap_or(1).max(1);
    let moved = i32::try_from(index).unwrap_or(0) + turns;
    usize::try_from(moved.rem_euclid(len)).unwrap_or(0)
}

/// The settings the panel walks through, in the order of the screen; the
/// index of each but `REROLL` is its parameter's.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Setting {
    /// Steps in the loop.
    Length,
    /// The chance a step is redrawn as the clock reaches it.
    Chance,
    /// The scale the notes are quantised to.
    Scale,
    /// The scale's root.
    Root,
    /// Octaves the whole moves by.
    Octave,
    /// Semitones the notes move by before they are quantised.
    Transpose,
    /// Octaves a drawn note spans above the root.
    Range,
    /// How evenly drawn notes spread over the range.
    Spread,
    /// Where in the range they gather.
    Bias,
    /// A walk of up to so many degrees from the step before, or off.
    Jump,
    /// Whether a drawn note may repeat the one before.
    Repeat,
    /// The chord on B, C and D.
    Chord,
    /// How many of its lowest tones go up an octave.
    Invert,
    /// What D carries: the chord's third tone or a gate.
    OutD,
    /// The gate's length, in percent of the clock's period.
    Gate,
    /// Draws the whole sequence anew.
    Reroll,
}

impl Setting {
    /// Every setting, top to bottom.
    pub const ALL: [Self; 16] = [
        Self::Length,
        Self::Chance,
        Self::Scale,
        Self::Root,
        Self::Octave,
        Self::Transpose,
        Self::Range,
        Self::Spread,
        Self::Bias,
        Self::Jump,
        Self::Repeat,
        Self::Chord,
        Self::Invert,
        Self::OutD,
        Self::Gate,
        Self::Reroll,
    ];

    /// Its parameter's index, `None` for `REROLL`, an action.
    const fn param(self) -> Option<usize> {
        match self {
            Self::Reroll => None,
            other => Some(other as usize),
        }
    }

    /// Where this setting's input is kept, `None` for one that takes none:
    /// its rank among the assignable parameters (journal/D-045).
    const fn input_slot(self) -> Option<usize> {
        match self {
            Self::Length => Some(0),
            Self::Chance => Some(1),
            Self::Transpose => Some(2),
            Self::Range => Some(3),
            Self::Spread => Some(4),
            Self::Bias => Some(5),
            _ => None,
        }
    }

    /// How an input moves it: its bounds, its value at 0 V under `SET`, and
    /// how many units a number of millivolts makes (`units / millivolts`).
    // Every bound here is below a hundred; `From` is not `const`.
    #[allow(clippy::cast_possible_wrap, clippy::cast_possible_truncation)]
    const fn rate(self) -> Option<Rate> {
        let rate = match self {
            // Four steps a volt.
            Self::Length => Rate::new((1, MAX_STEPS as i32), 1, (1, 250)),
            // One volt an octave.
            Self::Transpose => Rate::new((-MAX_TRANSPOSE, MAX_TRANSPOSE), 0, (12, 1_000)),
            Self::Range => Rate::new((1, MAX_RANGE as i32), 1, (1, 1_000)),
            // 20 % a volt, as `RATCHET`'s `PROB`.
            Self::Chance | Self::Spread | Self::Bias => Rate::new((0, 100), 0, (1, 50)),
            _ => return None,
        };
        Some(rate)
    }

    const fn label(self) -> &'static str {
        match self {
            Self::Length => "LENGTH",
            Self::Chance => "PROB",
            Self::Scale => "SCALE",
            Self::Root => "ROOT",
            Self::Octave => "OCTAVE",
            Self::Transpose => "TRANSPOSE",
            Self::Range => "RANGE",
            Self::Spread => "SPREAD",
            Self::Bias => "BIAS",
            Self::Jump => "JUMP",
            Self::Repeat => "REPEAT",
            Self::Chord => "CHORD",
            Self::Invert => "INVERT",
            Self::OutD => "OUT D",
            Self::Gate => "GATE",
            Self::Reroll => "REROLL",
        }
    }
}

/// The most semitones `TRANSPOSE` moves the notes by, either way.
pub const MAX_TRANSPOSE: i32 = 24;

/// The most octaves `OCTAVE` moves the whole by, either way.
pub const MAX_OCTAVE: i32 = 3;

/// How an input moves a setting that takes one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Rate {
    bounds: (i32, i32),
    /// Its value at 0 V under `SET`.
    zero: i32,
    /// Units per millivolts, as a fraction.
    ratio: (i32, i32),
}

impl Rate {
    const fn new(bounds: (i32, i32), zero: i32, ratio: (i32, i32)) -> Self {
        Self {
            bounds,
            zero,
            ratio,
        }
    }

    /// `millivolts` as units, rounded to the nearest.
    fn units(self, millivolts: MilliVolts) -> i64 {
        let (units, per) = (i64::from(self.ratio.0), i64::from(self.ratio.1));
        let product = i64::from(millivolts) * units;
        (product + product.signum() * per / 2) / per
    }

    /// `base` moved by `millivolts` under `effect`, within its bounds.
    fn apply(self, base: i32, millivolts: MilliVolts, effect: Effect) -> i32 {
        let moved = match effect {
            Effect::Set => i64::from(self.zero) + self.units(millivolts),
            Effect::Add => i64::from(base) + self.units(millivolts),
            Effect::Mul => multiplied(i64::from(base), millivolts),
        };
        let bounded = moved.clamp(i64::from(self.bounds.0), i64::from(self.bounds.1));
        i32::try_from(bounded).unwrap_or(self.zero)
    }

    /// The signal that sweeps it from its least to its most under `SET`.
    const fn sweep(self) -> (MilliVolts, MilliVolts) {
        let (units, per) = self.ratio;
        (
            (self.bounds.0 - self.zero) * per / units,
            (self.bounds.1 - self.zero) * per / units,
        )
    }

    /// Where `value` sits in its bounds, in thousandths.
    fn permille(self, value: i32) -> i32 {
        let (low, high) = self.bounds;
        let span = i64::from((high - low).max(1));
        let at = (i64::from(value) - i64::from(low)) * 1_000 / span;
        i32::try_from(at.clamp(0, 1_000)).unwrap_or(0)
    }
}

/// The signal that sweeps `setting` over all its values when it follows its
/// input (journal/D-035); `None` for a setting that takes no input.
#[must_use]
pub fn sweep(setting: Setting) -> Option<(MilliVolts, MilliVolts)> {
    setting.rate().map(Rate::sweep)
}

/// What the panel sets, inputs aside.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Settings {
    /// Steps in the loop.
    pub length: u8,
    /// The chance a step is redrawn, in percent.
    pub chance_percent: u8,
    /// The scale.
    pub scale: Scale,
    /// The root, 0 for C.
    pub root: u8,
    /// Octaves the whole moves by.
    pub octave: i8,
    /// Semitones the notes move by before they are quantised.
    pub transpose: i8,
    /// Octaves a drawn note spans.
    pub range_octaves: u8,
    /// See [`Draw::spread_percent`].
    pub spread_percent: u8,
    /// See [`Draw::bias_percent`].
    pub bias_percent: u8,
    /// See [`Draw::jump`].
    pub jump: u8,
    /// See [`Draw::repeat`].
    pub repeat: bool,
    /// The chord on B, C and D.
    pub chord: Chord,
    /// How many of its lowest tones go up an octave.
    pub inversion: u8,
    /// Whether D carries a gate rather than the chord's third tone.
    pub gate_on_d: bool,
    /// The gate's length, in percent of the clock's period.
    pub gate_percent: u8,
}

impl Settings {
    /// The settings at start.
    pub const DEFAULT: Self = Self {
        length: DEFAULT_LENGTH,
        chance_percent: 0,
        scale: Draw::DEFAULT.scale,
        root: 0,
        octave: 0,
        transpose: 0,
        range_octaves: Draw::DEFAULT.range_octaves,
        spread_percent: Draw::DEFAULT.spread_percent,
        bias_percent: Draw::DEFAULT.bias_percent,
        jump: Draw::DEFAULT.jump,
        repeat: Draw::DEFAULT.repeat,
        chord: Chord::Triad,
        inversion: 0,
        gate_on_d: false,
        gate_percent: 50,
    };

    /// How a new note is drawn with these settings.
    #[must_use]
    pub const fn draw(&self) -> Draw {
        Draw {
            scale: self.scale,
            range_octaves: self.range_octaves,
            spread_percent: self.spread_percent,
            bias_percent: self.bias_percent,
            jump: self.jump,
            repeat: self.repeat,
        }
    }

    /// The value of a setting that takes an input, as a number.
    fn number(&self, setting: Setting) -> i32 {
        match setting {
            Setting::Length => i32::from(self.length),
            Setting::Chance => i32::from(self.chance_percent),
            Setting::Transpose => i32::from(self.transpose),
            Setting::Range => i32::from(self.range_octaves),
            Setting::Spread => i32::from(self.spread_percent),
            Setting::Bias => i32::from(self.bias_percent),
            _ => 0,
        }
    }

    /// Sets a setting that takes an input from a number within its bounds.
    fn set_number(&mut self, setting: Setting, value: i32) {
        let byte = u8::try_from(value).unwrap_or(0);
        match setting {
            Setting::Length => self.length = byte,
            Setting::Chance => self.chance_percent = byte,
            Setting::Transpose => self.transpose = i8::try_from(value).unwrap_or(0),
            Setting::Range => self.range_octaves = byte,
            Setting::Spread => self.spread_percent = byte,
            Setting::Bias => self.bias_percent = byte,
            _ => {}
        }
    }
}

/// What `MELODY` is set by (journal/D-043), in the order of the screen.
// Every bound here is below a hundred; `From` is not `const`.
#[allow(clippy::cast_possible_wrap, clippy::cast_possible_truncation)]
pub const PARAM_SPECS: [ParamSpec; 15] = [
    ParamSpec::number("LENGTH", (1, MAX_STEPS as i32), 1, 0, Unit::None).assignable(),
    ParamSpec::number("PROB", (0, 100), 1, 0, Unit::Percent).assignable(),
    ParamSpec::choice("SCALE", &SCALE_NAMES),
    ParamSpec::choice("ROOT", &ROOT_NAMES),
    ParamSpec::number("OCTAVE", (-MAX_OCTAVE, MAX_OCTAVE), 1, 0, Unit::None),
    ParamSpec::number(
        "TRANSPOSE",
        (-MAX_TRANSPOSE, MAX_TRANSPOSE),
        1,
        0,
        Unit::None,
    )
    .assignable(),
    ParamSpec::number("RANGE", (1, MAX_RANGE as i32), 1, 0, Unit::Octaves).assignable(),
    ParamSpec::number("SPREAD", (0, 100), 1, 0, Unit::Percent).assignable(),
    ParamSpec::number("BIAS", (0, 100), 1, 0, Unit::Percent).assignable(),
    ParamSpec::number("JUMP", (0, MAX_JUMP as i32), 1, 0, Unit::None),
    ParamSpec::toggle("REPEAT"),
    ParamSpec::choice("CHORD", &CHORD_NAMES),
    ParamSpec::number("INVERT", (0, MAX_INVERSION as i32), 1, 0, Unit::None),
    ParamSpec::choice("OUT D", &["CHORD", "GATE"]),
    ParamSpec::number("GATE", (1, 99), 1, 0, Unit::Percent),
];

/// What `MELODY` can be read for: the step playing (from 1) and the note A
/// carries.
// `MAX_STEPS` is 32; `From` is not `const`.
#[allow(clippy::cast_possible_wrap, clippy::cast_possible_truncation)]
pub const OBSERVABLES: [ParamSpec; 2] = [
    ParamSpec::number("STEP", (1, MAX_STEPS as i32), 1, 0, Unit::None),
    ParamSpec::number("NOTE", (0, 127), 1, 0, Unit::Note),
];

/// What `MELODY` can be asked to do: draw the whole sequence anew, as
/// `REROLL` does; arm it, as `RESET` does.
pub const ACTIONS: [&str; 2] = ["REROLL", "RESET"];

/// What each setting takes at start: `CV1` added to `TRANSPOSE`, the V/oct
/// a sequencer is transposed by; nothing for the others.
const DEFAULT_ASSIGNS: [Assign; 6] = [
    Assign::NONE,
    Assign::NONE,
    Assign {
        input: Some(CvChannel::One),
        ..Assign::NONE
    },
    Assign::NONE,
    Assign::NONE,
    Assign::NONE,
];

/// Semitones from C4 as millivolts at one volt per octave, rounded to the
/// nearest (journal/D-041 leaves the half millivolt).
fn millivolts(semitones: i32) -> MilliVolts {
    let product = semitones * 1_000;
    (product + product.signum() * 6) / 12
}

/// A looping melody of random but controllable notes on A, a chord or a
/// gate on B to D.
#[derive(Debug, Clone)]
pub struct MelodyApp {
    settings: Settings,
    sequence: Sequence,
    /// The transposition the step playing started with, inputs included.
    transpose: i32,
    /// The rate of `CLOCK`, which the gate is a fraction of.
    tempo: Tempo,
    /// The input of each setting that takes one, and the submenu.
    inputs: Modulations<6>,
    selected: usize,
    /// The step the panel edits, `None` outside the sequence.
    editing: Option<usize>,
    clock: bool,
    reset: bool,
    gate_micros: u32,
    outputs: [MilliVolts; CV_CHANNELS],
}

impl MelodyApp {
    /// The default settings, a sequence drawn from them, armed.
    #[must_use]
    pub fn new() -> Self {
        let settings = Settings::DEFAULT;
        let mut app = Self {
            settings,
            sequence: Sequence::new(settings.length, settings.draw()),
            transpose: 0,
            tempo: Tempo::new(),
            inputs: Modulations::new(DEFAULT_ASSIGNS),
            selected: 0,
            editing: None,
            clock: false,
            reset: false,
            gate_micros: 0,
            outputs: [0; CV_CHANNELS],
        };
        app.outputs = app.levels();
        app
    }

    /// The settings as the panel set them.
    #[must_use]
    pub const fn settings(&self) -> &Settings {
        &self.settings
    }

    /// The steps and where the clock is in them.
    #[must_use]
    pub const fn sequence(&self) -> &Sequence {
        &self.sequence
    }

    /// The setting the right encoder changes.
    #[must_use]
    pub const fn selected(&self) -> Setting {
        Setting::ALL[self.selected]
    }

    /// The page of settings on screen, from 0: the selected setting's.
    #[must_use]
    pub const fn page(&self) -> usize {
        self.selected / PAGE_ROWS
    }

    /// The step the panel edits, `None` outside the sequence.
    #[must_use]
    pub const fn editing(&self) -> Option<usize> {
        self.editing
    }

    /// Whether the input submenu is open.
    #[must_use]
    pub const fn assigning(&self) -> bool {
        self.inputs.is_open()
    }

    /// The input `setting` takes, `None` for one that takes none.
    #[must_use]
    pub fn assign(&self, setting: Setting) -> Option<&Assign> {
        setting.input_slot().map(|slot| self.inputs.assign(slot))
    }

    /// The levels of A to D now.
    #[must_use]
    pub const fn outputs(&self) -> [MilliVolts; CV_CHANNELS] {
        self.outputs
    }

    /// Follows the panel, `CLOCK` and `RESET`, sets the outputs.
    pub fn update(&mut self, input: &InputSnapshot) -> [MilliVolts; CV_CHANNELS] {
        self.follow_panel(input);
        let elapsed = input.elapsed_micros;
        self.gate_micros = self.gate_micros.saturating_sub(elapsed);
        self.inputs.read(&input.cv, elapsed);
        self.tempo.advance(elapsed);

        let reset = input.triggers[1];
        if reset && !self.reset {
            self.sequence.reset();
        }
        self.reset = reset;
        let clock = input.triggers[0];
        if clock && !self.clock {
            self.tempo.edge();
            self.step();
        }
        self.clock = clock;

        self.outputs = self.levels();
        self.outputs
    }

    /// The panel's settings, each input applied as it reads now.
    fn with_inputs(&self) -> Settings {
        let mut settings = self.settings;
        for setting in Setting::ALL {
            let (Some(slot), Some(rate)) = (setting.input_slot(), setting.rate()) else {
                continue;
            };
            if let Some((millivolts, effect)) = self.inputs.reading(slot) {
                let moved = rate.apply(settings.number(setting), millivolts, effect);
                settings.set_number(setting, moved);
            }
        }
        settings
    }

    /// Moves to the next step with the settings its inputs give now, and
    /// opens D's gate.
    fn step(&mut self) {
        let latched = self.with_inputs();
        self.transpose = i32::from(latched.transpose);
        self.sequence.set_length(latched.length);
        self.sequence.clock(latched.chance_percent, latched.draw());
        let gate = self
            .tempo
            .period_micros()
            .map_or(UNKNOWN_GATE_MICROS, |period| {
                let part = u64::from(period) * u64::from(self.settings.gate_percent) / 100;
                u32::try_from(part).unwrap_or(u32::MAX)
            });
        self.gate_micros = gate.max(MIN_GATE_MICROS);
    }

    /// Draws the whole sequence anew, with the inputs as they read now.
    fn reroll(&mut self) {
        let draw = self.with_inputs().draw();
        self.sequence.reroll(draw);
    }

    /// The note step `index` plays, in semitones above the root: moved by
    /// the transposition, quantised to the scale.
    fn played(&self, index: usize, transpose: i32) -> i32 {
        let note = i32::from(self.sequence.note(index)) + transpose;
        self.settings.scale.quantize(note)
    }

    /// Semitones above the root as semitones from C4.
    fn above_c4(&self, note: i32) -> i32 {
        i32::from(self.settings.root) + 12 * i32::from(self.settings.octave) + note
    }

    /// The MIDI note A carries.
    fn midi_note(&self) -> i32 {
        let step = self.sequence.position().unwrap_or(0);
        ZERO_VOLT_NOTE + self.above_c4(self.played(step, self.transpose))
    }

    /// What A to D carry: the step playing (the first while armed), its
    /// chord, or the gate on D.
    fn levels(&self) -> [MilliVolts; CV_CHANNELS] {
        let settings = &self.settings;
        let step = self.sequence.position().unwrap_or(0);
        let note = self.played(step, self.transpose);
        let tones = chord(settings.scale, note, settings.chord, settings.inversion);
        let mut levels = [
            millivolts(self.above_c4(note)),
            millivolts(self.above_c4(tones[0])),
            millivolts(self.above_c4(tones[1])),
            millivolts(self.above_c4(tones[2])),
        ];
        if settings.gate_on_d {
            levels[3] = if self.gate_micros > 0 { GATE_MV } else { 0 };
        }
        levels
    }

    fn follow_panel(&mut self, input: &InputSnapshot) {
        if self.inputs.follow(input, sweep(self.selected())) {
            return;
        }
        if input.buttons.pressed(Button::LeftEncoder) {
            self.editing = match self.editing {
                Some(_) => None,
                None => Some(self.sequence.position().unwrap_or(0)),
            };
        }
        let length = usize::from(self.sequence.length());
        if let Some(step) = self.editing {
            let step = step.min(length - 1);
            let picks = i32::from(input.controls.delta(0));
            let step = wrapped(step, length, picks);
            self.editing = Some(step);
            let turns = i32::from(input.controls.delta(1));
            if turns != 0 {
                let note = i32::from(self.sequence.note(step));
                let moved = self
                    .settings
                    .scale
                    .step(note, turns)
                    .clamp(0, i32::from(MAX_NOTE));
                self.sequence
                    .set_note(step, u8::try_from(moved).unwrap_or(0));
            }
            if input.buttons.pressed(Button::RightEncoder) {
                let draw = self.with_inputs().draw();
                self.sequence.redraw(step, draw);
            }
            return;
        }
        let picks = i32::from(input.controls.delta(0));
        if picks != 0 {
            self.selected = wrapped(self.selected, Setting::ALL.len(), picks);
        }
        if input.buttons.pressed(Button::RightEncoder) {
            match self.selected() {
                Setting::Reroll => self.reroll(),
                setting => {
                    if let Some(slot) = setting.input_slot() {
                        self.inputs.open(slot);
                    }
                }
            }
        }
        let turns = i32::from(input.controls.delta(1));
        if turns != 0 {
            self.change(self.selected(), turns);
        }
    }

    /// Turns `setting` by `turns` detents, within its bounds; a choice
    /// wraps round.
    fn change(&mut self, setting: Setting, turns: i32) {
        let Some(index) = setting.param() else {
            return;
        };
        let Some(ParamValue::Number(value)) = self.get(index) else {
            if let Some(ParamValue::Choice(choice)) = self.get(index) {
                let count = match PARAM_SPECS[index].kind {
                    crate::apps::param::Kind::Choice(names) => names.len(),
                    _ => 1,
                };
                let next = wrapped(usize::from(choice), count, turns);
                let _ = self.set(index, ParamValue::Choice(u8::try_from(next).unwrap_or(0)));
            } else if let Some(ParamValue::Toggle(on)) = self.get(index) {
                let _ = self.set(index, ParamValue::Toggle(!on));
            }
            return;
        };
        let crate::apps::param::Kind::Number { min, max, .. } = PARAM_SPECS[index].kind else {
            return;
        };
        let _ = self.set(index, ParamValue::Number((value + turns).clamp(min, max)));
    }

    /// The value of `setting` in `settings`, as the screen shows it.
    fn write_value(setting: Setting, settings: &Settings, out: &mut TextBuf<16>) {
        let on_off = |on: bool| if on { "ON" } else { "OFF" };
        let _ = match setting {
            Setting::Length => write!(out, "{}", settings.length),
            Setting::Chance => write!(out, "{}%", settings.chance_percent),
            Setting::Scale => write!(out, "{}", SCALE_NAMES[settings.scale.index()]),
            Setting::Root => write!(out, "{}", ROOT_NAMES[usize::from(settings.root % 12)]),
            Setting::Octave => write_signed(out, i32::from(settings.octave)),
            Setting::Transpose => write_signed(out, i32::from(settings.transpose)),
            Setting::Range => write!(out, "{} OCT", settings.range_octaves),
            Setting::Spread => write!(out, "{}%", settings.spread_percent),
            Setting::Bias => write!(out, "{}%", settings.bias_percent),
            Setting::Jump => match settings.jump {
                0 => write!(out, "OFF"),
                jump => write!(out, "{jump}"),
            },
            Setting::Repeat => write!(out, "{}", on_off(settings.repeat)),
            Setting::Chord => write!(out, "{}", CHORD_NAMES[settings.chord.index()]),
            Setting::Invert => write!(out, "{}", settings.inversion),
            Setting::OutD => write!(out, "{}", if settings.gate_on_d { "GATE" } else { "CHORD" }),
            Setting::Gate => write!(out, "{}%", settings.gate_percent),
            Setting::Reroll => Ok(()),
        };
    }

    /// Draws the settings, the sequence and, open, the input submenu.
    pub fn render(&self, frame: &mut FrameBuffer, _context: &TickContext) {
        frame.clear();
        self.render_title(frame);
        self.render_settings(frame);
        self.render_sequence(frame);
        if self.inputs.is_open() {
            self.render_submenu(frame);
        }
    }

    /// The title bar: the applet, or `EDIT` in the sequence; the step
    /// playing or edited, of the loop; the note it plays.
    fn render_title(&self, frame: &mut FrameBuffer) {
        let lit = TextStyle::lit(&FONT_5X8);
        let (title, step) = match self.editing {
            Some(step) => ("EDIT", Some(step)),
            None => ("MELODY", self.sequence.position()),
        };
        text::draw(frame, title, Point::new(TITLE_LEFT, 0), lit);
        let mut at = TextBuf::<8>::new();
        let _ = match step {
            Some(step) => write!(at, "{:02}/{:02}", step + 1, self.sequence.length()),
            None => write!(at, "--/{:02}", self.sequence.length()),
        };
        text::draw(frame, at.as_str(), Point::new(48, 0), lit);
        let midi = match self.editing {
            Some(step) => ZERO_VOLT_NOTE + self.above_c4(self.played(step, self.transpose)),
            None => self.midi_note(),
        };
        let name = note_name(u8::try_from(midi.clamp(0, 127)).unwrap_or(0));
        text::draw(
            frame,
            name.as_str(),
            Point::new(WIDTH_I32 - 1, 0),
            lit.aligned(Alignment::Right),
        );
    }

    /// A page of settings under the title bar.
    fn render_settings(&self, frame: &mut FrameBuffer) {
        let lit = TextStyle::lit(&FONT_5X8);
        let rows = i32::try_from(PAGE_ROWS).unwrap_or(3);
        scroll_bar(
            frame,
            0,
            ROW_HEIGHT,
            SCROLL_WIDTH,
            rows * ROW_HEIGHT,
            self.page(),
            PAGES,
        );
        let first = self.page() * PAGE_ROWS;
        let shown = &Setting::ALL[first..(first + PAGE_ROWS).min(Setting::ALL.len())];
        for (row, &setting) in (1..).zip(shown) {
            let top = row * ROW_HEIGHT;
            // In the sequence, the panel is the steps': no setting stands out.
            let highlighted = setting == self.selected() && self.editing.is_none();
            if highlighted {
                row_highlight(frame, top, ROW_HEIGHT, Ink::Lit);
            }
            let style = if highlighted {
                TextStyle::dark(&FONT_5X8)
            } else {
                lit
            };
            text::draw(
                frame,
                setting.label(),
                Point::new(LIST_LEFT + 2, top),
                style,
            );
            // A setting that follows its input has no use for the panel's
            // value: its input stands in its place.
            if !self.assign(setting).is_some_and(Assign::sets) {
                let mut value = TextBuf::<16>::new();
                Self::write_value(setting, &self.settings, &mut value);
                text::draw(
                    frame,
                    value.as_str(),
                    Point::new(VALUE_RIGHT, top),
                    style.aligned(Alignment::Right),
                );
            }
            if let Some(assign) = self.assign(setting) {
                let mut name = TextBuf::<8>::new();
                assign.write(&mut name);
                text::draw(
                    frame,
                    name.as_str(),
                    Point::new(WIDTH_I32 - 3, top),
                    style.aligned(Alignment::Right),
                );
            }
        }
    }

    /// The steps of the loop at the bottom of the screen, side by side: each
    /// note's mark as high as its pitch, the step playing filled below its
    /// mark under the playhead, the step edited framed.
    fn render_sequence(&self, frame: &mut FrameBuffer) {
        let length = usize::from(self.sequence.length());
        let scale = self.settings.scale;
        let notes = &self.sequence.notes()[..length];
        let quantised = |note: u8| {
            scale
                .quantize(i32::from(note))
                .clamp(0, i32::from(MAX_NOTE))
        };
        let highest = notes.iter().map(|&note| quantised(note)).max().unwrap_or(0);
        let top_note = highest
            .max(12 * i32::from(self.settings.range_octaves))
            .max(1);
        let bottom = HEIGHT_I32 - MARK_HEIGHT;
        let height = bottom - NOTES_TOP;
        let count = i32::try_from(length).unwrap_or(1);
        let playing = self.sequence.position();
        for (index, &note) in (0..).zip(notes) {
            let step = usize::try_from(index).unwrap_or(0);
            let x = index * WIDTH_I32 / count;
            let width = ((index + 1) * WIDTH_I32 / count - x - 1).max(1);
            let y = bottom - quantised(note) * height / top_note;
            if playing == Some(step) {
                gfx::fill_rect(frame, x, y, width, HEIGHT_I32 - y, Ink::Lit);
                gfx::patterns::playhead(frame, Point::new(x + width / 2 - 2, SEQUENCE_TOP));
            } else {
                gfx::fill_rect(frame, x, y, width, MARK_HEIGHT, Ink::Lit);
            }
            if self.editing == Some(step) {
                gfx::rect(
                    frame,
                    x - 1,
                    NOTES_TOP - 1,
                    width + 2,
                    HEIGHT_I32 - NOTES_TOP + 1,
                    Ink::Lit,
                );
            }
        }
    }

    /// The selected setting's input submenu over the main screen: the
    /// setting, the value a step would take now and where both sit in its
    /// range (journal/D-036).
    fn render_submenu(&self, frame: &mut FrameBuffer) {
        let setting = self.selected();
        let Some(rate) = setting.rate() else {
            return;
        };
        let latched = self.with_inputs();
        let mut now = TextBuf::<16>::new();
        Self::write_value(setting, &latched, &mut now);
        let param = Param {
            name: setting.label(),
            now: now.as_str(),
            // `TRANSPOSE` is marked at its middle, the others at their
            // start.
            zero_permille: rate.permille(rate.zero),
            panel_permille: rate.permille(self.settings.number(setting)),
            now_permille: rate.permille(latched.number(setting)),
        };
        self.inputs.render(frame, &param);
    }
}

/// `value` with its sign, `0` without.
fn write_signed(out: &mut TextBuf<16>, value: i32) -> core::fmt::Result {
    if value > 0 {
        write!(out, "+{value}")
    } else {
        write!(out, "{value}")
    }
}

/// The note, then the chord, D a gate if set; named by what they carry, the
/// routing page setting the jack (journal/D-046).
const PORTS: [Port; 4] = [
    Port { name: "NOTE" },
    Port { name: "CH1" },
    Port { name: "CH2" },
    Port { name: "CH3/G" },
];

/// `TR1` clocks the steps, `TR2` arms the first; the routing page may plug
/// other triggers in (journal/D-046).
const INPUTS: [InputPort; 2] = [
    InputPort {
        name: "CLOCK",
        kind: InputKind::Trigger,
        lane: 0,
    },
    InputPort {
        name: "RESET",
        kind: InputKind::Trigger,
        lane: 1,
    },
];

impl App for MelodyApp {
    fn id(&self) -> AppId {
        AppId::Melody
    }

    fn update(&mut self, input: &InputSnapshot) -> Params {
        MelodyApp::update(self, input).into()
    }

    fn render(&self, frame: &mut FrameBuffer, context: &TickContext) {
        MelodyApp::render(self, frame, context);
    }

    fn output_ports(&self) -> &'static [Port] {
        &PORTS
    }

    fn input_ports(&self) -> &'static [InputPort] {
        &INPUTS
    }

    fn param_specs(&self) -> &'static [ParamSpec] {
        &PARAM_SPECS
    }

    fn param_active(&self, index: usize) -> bool {
        let walking = self.settings.jump > 0;
        match Setting::ALL.get(index) {
            // A walk draws from the step before, not over the range.
            Some(Setting::Spread | Setting::Bias | Setting::Repeat) => !walking,
            Some(Setting::Gate) => self.settings.gate_on_d,
            _ => true,
        }
    }

    fn get(&self, index: usize) -> Option<ParamValue> {
        let settings = &self.settings;
        let number = |value: i32| Some(ParamValue::Number(value));
        let choice = |value: usize| Some(ParamValue::Choice(u8::try_from(value).unwrap_or(0)));
        match Setting::ALL.get(index)? {
            Setting::Length => number(i32::from(settings.length)),
            Setting::Chance => number(i32::from(settings.chance_percent)),
            Setting::Scale => choice(settings.scale.index()),
            Setting::Root => choice(usize::from(settings.root)),
            Setting::Octave => number(i32::from(settings.octave)),
            Setting::Transpose => number(i32::from(settings.transpose)),
            Setting::Range => number(i32::from(settings.range_octaves)),
            Setting::Spread => number(i32::from(settings.spread_percent)),
            Setting::Bias => number(i32::from(settings.bias_percent)),
            Setting::Jump => number(i32::from(settings.jump)),
            Setting::Repeat => Some(ParamValue::Toggle(settings.repeat)),
            Setting::Chord => choice(settings.chord.index()),
            Setting::Invert => number(i32::from(settings.inversion)),
            Setting::OutD => choice(usize::from(settings.gate_on_d)),
            Setting::Gate => number(i32::from(settings.gate_percent)),
            Setting::Reroll => None,
        }
    }

    fn set(&mut self, index: usize, value: ParamValue) -> Result<ParamValue, Refused> {
        let spec = PARAM_SPECS.get(index).ok_or(Refused::NoSuch)?;
        let value = spec.check(value)?;
        let settings = &mut self.settings;
        let byte = |value: i32| u8::try_from(value).unwrap_or(0);
        let signed = |value: i32| i8::try_from(value).unwrap_or(0);
        match (Setting::ALL[index], value) {
            (Setting::Length, ParamValue::Number(length)) => {
                settings.length = byte(length);
                // The loop shortens at once, not at the next clock.
                self.sequence.set_length(settings.length);
            }
            (Setting::Chance, ParamValue::Number(chance)) => settings.chance_percent = byte(chance),
            (Setting::Scale, ParamValue::Choice(scale)) => {
                settings.scale = Scale::from_index(usize::from(scale));
            }
            (Setting::Root, ParamValue::Choice(root)) => settings.root = root,
            (Setting::Octave, ParamValue::Number(octave)) => settings.octave = signed(octave),
            (Setting::Transpose, ParamValue::Number(semitones)) => {
                settings.transpose = signed(semitones);
            }
            (Setting::Range, ParamValue::Number(octaves)) => settings.range_octaves = byte(octaves),
            (Setting::Spread, ParamValue::Number(spread)) => settings.spread_percent = byte(spread),
            (Setting::Bias, ParamValue::Number(bias)) => settings.bias_percent = byte(bias),
            (Setting::Jump, ParamValue::Number(jump)) => settings.jump = byte(jump),
            (Setting::Repeat, ParamValue::Toggle(on)) => settings.repeat = on,
            (Setting::Chord, ParamValue::Choice(chord)) => {
                settings.chord = Chord::from_index(usize::from(chord));
            }
            (Setting::Invert, ParamValue::Number(inversion)) => {
                settings.inversion = byte(inversion);
            }
            (Setting::OutD, ParamValue::Choice(out)) => settings.gate_on_d = out == 1,
            (Setting::Gate, ParamValue::Number(gate)) => settings.gate_percent = byte(gate),
            _ => {}
        }
        // A change without a clock still shows at the outputs.
        self.outputs = self.levels();
        self.get(index).ok_or(Refused::NoSuch)
    }

    fn observables(&self) -> &'static [ParamSpec] {
        &OBSERVABLES
    }

    fn observe(&self, index: usize) -> Option<ParamValue> {
        let number = match index {
            0 => i32::try_from(self.sequence.position()? + 1).unwrap_or(0),
            1 => self.midi_note().clamp(0, 127),
            _ => return None,
        };
        Some(ParamValue::Number(number))
    }

    fn actions(&self) -> &'static [&'static str] {
        &ACTIONS
    }

    fn act(&mut self, index: usize) -> Result<(), Refused> {
        match index {
            0 => self.reroll(),
            1 => self.sequence.reset(),
            _ => return Err(Refused::NoSuch),
        }
        self.outputs = self.levels();
        Ok(())
    }

    fn assigns(&self) -> &[Assign] {
        self.inputs.as_slice()
    }

    fn assigns_mut(&mut self) -> &mut [Assign] {
        self.inputs.as_mut_slice()
    }

    fn assign_sweep(&self, index: usize) -> Option<(MilliVolts, MilliVolts)> {
        sweep(*Setting::ALL.get(index)?)
    }
}

impl Default for MelodyApp {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::{MelodyApp, PARAM_SPECS, Setting, Settings, millivolts, sweep};
    use crate::fmt::TextBuf;
    use crate::modulation::slots::slot_of;

    #[test]
    fn a_semitone_is_a_twelfth_of_a_volt_rounded() {
        assert_eq!(millivolts(0), 0);
        assert_eq!(millivolts(12), 1_000);
        assert_eq!(millivolts(1), 83);
        assert_eq!(millivolts(2), 167);
        assert_eq!(millivolts(-1), -83);
        assert_eq!(millivolts(-36), -3_000);
    }

    #[test]
    fn every_parameter_is_its_setting() {
        for (index, spec) in PARAM_SPECS.iter().enumerate() {
            let setting = Setting::ALL[index];
            assert_eq!(spec.name, setting.label(), "{index}");
            assert_eq!(setting.param(), Some(index));
            assert_eq!(
                slot_of(&PARAM_SPECS, index),
                setting.input_slot(),
                "{index}"
            );
            assert_eq!(spec.assignable, sweep(setting).is_some(), "{index}");
        }
        assert_eq!(Setting::Reroll.param(), None);
        assert_eq!(Setting::ALL.len(), PARAM_SPECS.len() + 1);
    }

    #[test]
    fn transpose_follows_its_input_at_a_volt_an_octave() {
        assert_eq!(sweep(Setting::Transpose), Some((-2_000, 2_000)));
        assert_eq!(sweep(Setting::Length), Some((0, 7_750)));
        assert_eq!(sweep(Setting::Range), Some((0, 3_000)));
    }

    #[test]
    fn every_value_fits_its_column() {
        for setting in Setting::ALL {
            let mut value = TextBuf::<16>::new();
            MelodyApp::write_value(setting, &Settings::DEFAULT, &mut value);
            assert!(value.as_str().len() <= 9, "{setting:?}: {}", value.as_str());
        }
    }
}
