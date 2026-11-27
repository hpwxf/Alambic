//! The Melody applet (journal/D-047): a looping sequence of random but
//! controllable notes, in the spirit of the meloDICER, Mimetic Digitalis and
//! Marbles.
//!
//! Each rising edge on `TR1` (`CLOCK`) moves to the next step, redrawing it
//! first with the chance `PROB` ([`sequence`]); a rising edge on `TR2`
//! (`RESET`) arms the sequence, so the next clock plays its first step. A
//! carries the step's note, quantised to `SCALE` from `ROOT`, moved by
//! `OCTAVE` and `TRANSPOSE` (which an input moves at one volt per octave);
//! `CH1` and `CH2` carry a chord stacked in the scale on it
//! ([`scale::chord`]), `GATE` a gate for `GATE` percent of the clock's
//! period. Which of the four ports reaches which jack is the routing page's
//! (journal/D-046), not a setting of the applet's.
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
pub mod window;

use core::fmt::Write as _;

use self::scale::{CHORD_NAMES, Chord, ROOT_NAMES, SCALE_NAMES, Scale, chord};
use self::sequence::{Draw, MAX_JUMP, MAX_STEPS, Sequence};
use self::window::{MAX_PERCENT, MAX_SPREAD, PLACE_MAX, Window};
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
use crate::platform::{Button, CV_CHANNELS, MilliVolts};
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

/// Settings shown per page, under the title bar and above the sequence:
/// four, as `RATCHET` shows, the sequence keeping the bottom third.
const PAGE_ROWS: usize = 4;

/// Pages of settings.
const PAGES: usize = ROWS.len().div_ceil(PAGE_ROWS);

/// Top of the notes, a row under the last setting. No row is kept for the
/// playhead: it sits at the top of its column, in inverse video.
const NOTES_TOP: i32 = 5 * ROW_HEIGHT + 1;

/// The playhead's height: a triangle of three rows, its tip down.
const PLAYHEAD_ROWS: i32 = 3;

/// A note's mark: two rows.
const MARK_HEIGHT: i32 = 2;

/// Right edge of the value column, left of the input column.
const VALUE_RIGHT: i32 = 98;

/// Steps at start.
const DEFAULT_LENGTH: u8 = 8;

/// How long `REROLL` reads `DONE` after a reroll: long enough to see it,
/// short enough to press again.
const REROLLED_MICROS: u32 = 500_000;

/// Pixels between the two fields of a row.
const FIELD_GAP: i32 = 6;

/// The inversion's arrow: five columns.
const ARROW_WIDTH: i32 = 5;

/// `index` moved by `turns` among `len`, wrapping round.
fn wrapped(index: usize, len: usize, turns: i32) -> usize {
    let len = i32::try_from(len).unwrap_or(1).max(1);
    let moved = i32::try_from(index).unwrap_or(0) + turns;
    usize::try_from(moved.rem_euclid(len)).unwrap_or(0)
}

/// What the panel sets, one field each, in the order of the parameters: the
/// index of each but `REROLL` is its parameter's. The screen lays them out
/// in [`ROWS`], some two to a row, and the left encoder walks them in
/// [`FIELDS`]' order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Setting {
    /// Steps in the loop.
    Length,
    /// The chance a step is redrawn as the clock reaches it.
    Chance,
    /// The scale's root.
    Root,
    /// The scale the notes are quantised to.
    Scale,
    /// The chord on `CH1` and `CH2`.
    Chord,
    /// Its inversion: the lower tone up an octave, the upper one down, or
    /// neither.
    Invert,
    /// Octaves the whole moves by.
    Octave,
    /// Semitones the notes move by before they are quantised.
    Transpose,
    /// The gate's length, in percent of the clock's period.
    Gate,
    /// How a step is drawn: an even place in the window, one whose note
    /// does not repeat the step before's, or a walk of up to so many degrees
    /// from it.
    Jump,
    /// The window's width, in semitones ([`window`]).
    Spread,
    /// The window's place around the root.
    Center,
    /// The curve the steps' places are bent by in the window.
    Bias,
    /// Draws the whole sequence anew.
    Reroll,
}

impl Setting {
    /// Every setting, in the order of the parameters, `REROLL` last.
    pub const ALL: [Self; 14] = [
        Self::Length,
        Self::Chance,
        Self::Root,
        Self::Scale,
        Self::Chord,
        Self::Invert,
        Self::Octave,
        Self::Transpose,
        Self::Gate,
        Self::Jump,
        Self::Spread,
        Self::Center,
        Self::Bias,
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
            Self::Spread => Some(3),
            Self::Center => Some(4),
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
            // One volt an octave of width.
            Self::Spread => Rate::new((0, MAX_SPREAD as i32), 0, (12, 1_000)),
            // 20 % a volt, as `RATCHET`'s `PROB`.
            Self::Chance => Rate::new((0, 100), 0, (1, 50)),
            Self::Center | Self::Bias => {
                Rate::new((-(MAX_PERCENT as i32), MAX_PERCENT as i32), 0, (1, 50))
            }
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
            Self::Spread => "SPREAD",
            Self::Center => "CENTER",
            Self::Bias => "BIAS",
            Self::Jump => "JUMP",
            Self::Chord => "CHORD",
            Self::Invert => "INVERT",
            Self::Gate => "GATE",
            Self::Reroll => "REROLL",
        }
    }
}

/// The fields in the order the left encoder walks them: [`ROWS`] read
/// left to right, top to bottom.
pub const FIELDS: [Setting; 14] = [
    Setting::Length,
    Setting::Chance,
    Setting::Root,
    Setting::Scale,
    Setting::Chord,
    Setting::Invert,
    Setting::Octave,
    Setting::Transpose,
    Setting::Gate,
    Setting::Reroll,
    Setting::Jump,
    Setting::Spread,
    Setting::Center,
    Setting::Bias,
];

/// The rows of the screen, four a page, three pages: the loop, its key and
/// its chord; the pitch, the gate and the action; how steps are drawn and
/// the window they sound in. A row of
/// two fields shows both values, the root before the scale, the chord
/// before its inversion's arrow.
pub const ROWS: [&[Setting]; 12] = [
    &[Setting::Length],
    &[Setting::Chance],
    &[Setting::Root, Setting::Scale],
    &[Setting::Chord, Setting::Invert],
    &[Setting::Octave],
    &[Setting::Transpose],
    &[Setting::Gate],
    &[Setting::Reroll],
    &[Setting::Jump],
    &[Setting::Spread],
    &[Setting::Center],
    &[Setting::Bias],
];

/// The row `setting` is on.
fn row_of(setting: Setting) -> usize {
    ROWS.iter()
        .position(|row| row.contains(&setting))
        .unwrap_or(0)
}

/// How the screen labels a row: its field's name, or for two, what they
/// make together.
fn row_label(row: &[Setting]) -> &'static str {
    match row {
        [Setting::Root, Setting::Scale] => "KEY",
        [first, ..] => first.label(),
        [] => "",
    }
}

/// [`Setting::Jump`]'s values: a free draw, one that never repeats the
/// step before, a walk of 1 to 7 degrees.
pub const JUMP_NAMES: [&str; 2 + MAX_JUMP as usize] =
    ["OFF", "NO REP", "1", "2", "3", "4", "5", "6", "7"];

/// [`Setting::Invert`]'s range: the upper tone down an octave (−1), the
/// chord as stacked (0), the lower tone up an octave (+1).
pub const MAX_INVERSION: i32 = 1;

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
    /// The window the steps' places sound in: its width, place and curve.
    pub window: Window,
    /// See [`Draw::jump`].
    pub jump: u8,
    /// See [`Draw::repeat`]; `JUMP`'s `NO REP` when `false` without a walk.
    pub repeat: bool,
    /// The chord on `CH1` and `CH2`.
    pub chord: Chord,
    /// Its inversion, −1 to +1 ([`MAX_INVERSION`]).
    pub inversion: i8,
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
        window: Window::DEFAULT,
        jump: Draw::DEFAULT.jump,
        repeat: Draw::DEFAULT.repeat,
        chord: Chord::Triad,
        inversion: 0,
        gate_percent: 50,
    };

    /// How a new note is drawn with these settings.
    #[must_use]
    pub const fn draw(&self) -> Draw {
        Draw {
            scale: self.scale,
            window: self.window,
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
            Setting::Spread => i32::from(self.window.spread),
            Setting::Center => i32::from(self.window.center),
            Setting::Bias => i32::from(self.window.bias),
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
            Setting::Spread => self.window.spread = byte,
            Setting::Center => self.window.center = i8::try_from(value).unwrap_or(0),
            Setting::Bias => self.window.bias = i8::try_from(value).unwrap_or(0),
            _ => {}
        }
    }
}

/// What `MELODY` is set by (journal/D-043), in the order of the screen.
// Every bound here is below a hundred; `From` is not `const`.
#[allow(clippy::cast_possible_wrap, clippy::cast_possible_truncation)]
pub const PARAM_SPECS: [ParamSpec; 13] = [
    ParamSpec::number("LENGTH", (1, MAX_STEPS as i32), 1, 0, Unit::None).assignable(),
    ParamSpec::number("PROB", (0, 100), 1, 0, Unit::Percent).assignable(),
    ParamSpec::choice("ROOT", &ROOT_NAMES),
    ParamSpec::choice("SCALE", &SCALE_NAMES),
    ParamSpec::choice("CHORD", &CHORD_NAMES),
    ParamSpec::number("INVERT", (-MAX_INVERSION, MAX_INVERSION), 1, 0, Unit::None),
    ParamSpec::number("OCTAVE", (-MAX_OCTAVE, MAX_OCTAVE), 1, 0, Unit::None),
    ParamSpec::number(
        "TRANSPOSE",
        (-MAX_TRANSPOSE, MAX_TRANSPOSE),
        1,
        0,
        Unit::None,
    )
    .assignable(),
    ParamSpec::number("GATE", (1, 99), 1, 0, Unit::Percent),
    ParamSpec::choice("JUMP", &JUMP_NAMES),
    ParamSpec::number("SPREAD", (0, MAX_SPREAD as i32), 1, 0, Unit::None).assignable(),
    ParamSpec::number(
        "CENTER",
        (-(MAX_PERCENT as i32), MAX_PERCENT as i32),
        1,
        0,
        Unit::Percent,
    )
    .assignable(),
    ParamSpec::number(
        "BIAS",
        (-(MAX_PERCENT as i32), MAX_PERCENT as i32),
        1,
        0,
        Unit::Percent,
    )
    .assignable(),
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

/// Semitones from C4 as millivolts at one volt per octave, rounded to the
/// nearest (journal/D-041 leaves the half millivolt).
fn millivolts(semitones: i32) -> MilliVolts {
    let product = semitones * 1_000;
    (product + product.signum() * 6) / 12
}

/// A looping melody of random but controllable notes, its chord's two tones
/// and a gate.
#[derive(Debug, Clone)]
pub struct MelodyApp {
    settings: Settings,
    sequence: Sequence,
    /// The transposition the step playing started with, inputs included.
    transpose: i32,
    /// The step the outputs carry: the last the clock played, which a
    /// reset leaves until the next clock; the first before any.
    playing: usize,
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
    /// Time left showing `DONE` on `REROLL`'s row.
    rerolled_micros: u32,
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
            playing: 0,
            tempo: Tempo::new(),
            // No input at start, `TRANSPOSE` included: one is given in the
            // submenu, as to any other setting.
            inputs: Modulations::new([Assign::NONE; 6]),
            selected: 0,
            editing: None,
            clock: false,
            reset: false,
            gate_micros: 0,
            rerolled_micros: 0,
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
        FIELDS[self.selected]
    }

    /// The page of settings on screen, from 0: the selected setting's.
    #[must_use]
    pub fn page(&self) -> usize {
        row_of(self.selected()) / PAGE_ROWS
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
        self.rerolled_micros = self.rerolled_micros.saturating_sub(elapsed);
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
        self.playing = self.sequence.clock(latched.chance_percent, latched.draw());
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
        self.rerolled_micros = REROLLED_MICROS;
    }

    /// The window as it reads now, its inputs applied: they bend the loop
    /// at once, not at the next clock (journal/D-047, revision of
    /// 2026-11-27).
    #[must_use]
    pub fn window(&self) -> Window {
        self.with_inputs().window
    }

    /// The note step `index` plays, in semitones above the root: its place
    /// in the window, moved by the transposition, quantised to the scale.
    fn played(&self, index: usize, transpose: i32) -> i32 {
        self.played_in(self.window(), index, transpose)
    }

    /// The same in `window`, read once by a caller that plays several steps.
    fn played_in(&self, window: Window, index: usize, transpose: i32) -> i32 {
        let pitch = window.pitch(self.sequence.place(index));
        self.settings.scale.quantize(pitch + transpose)
    }

    /// The note step `index` plays now, in semitones above the root, the
    /// transposition the step started with included.
    #[must_use]
    pub fn played_note(&self, index: usize) -> i32 {
        self.played(index, self.transpose)
    }

    /// Semitones above the root as semitones from C4.
    fn above_c4(&self, note: i32) -> i32 {
        i32::from(self.settings.root) + 12 * i32::from(self.settings.octave) + note
    }

    /// The MIDI note A carries.
    fn midi_note(&self) -> i32 {
        let step = self.playing;
        ZERO_VOLT_NOTE + self.above_c4(self.played(step, self.transpose))
    }

    /// What the four ports carry: the step playing, its chord, the gate.
    fn levels(&self) -> [MilliVolts; CV_CHANNELS] {
        let settings = &self.settings;
        let step = self.playing;
        let note = self.played(step, self.transpose);
        let [low, high] = chord(settings.scale, note, settings.chord, settings.inversion);
        [
            millivolts(self.above_c4(note)),
            millivolts(self.above_c4(low)),
            millivolts(self.above_c4(high)),
            if self.gate_micros > 0 { GATE_MV } else { 0 },
        ]
    }

    fn follow_panel(&mut self, input: &InputSnapshot) {
        if self.inputs.follow(input, sweep(self.selected())) {
            return;
        }
        if input.buttons.pressed(Button::LeftEncoder) {
            self.editing = match self.editing {
                Some(_) => None,
                None => Some(self.playing.min(usize::from(self.sequence.length()) - 1)),
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
                // The note shown moves a degree a detent, and the step takes
                // the place that gives it; a window of one note has nothing
                // to move to.
                let window = self.window();
                let scale = self.settings.scale;
                let (lowest, highest) = window.bounds(scale);
                if lowest < highest {
                    let note = window.note(scale, self.sequence.place(step));
                    let moved = scale.step(note, turns).clamp(lowest, highest);
                    self.sequence.set_place(step, window.place_of(scale, moved));
                }
            }
            if input.buttons.pressed(Button::RightEncoder) {
                let draw = self.with_inputs().draw();
                self.sequence.redraw(step, draw);
            }
            return;
        }
        let picks = i32::from(input.controls.delta(0));
        if picks != 0 {
            self.selected = wrapped(self.selected, FIELDS.len(), picks);
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
    /// wraps round, but `JUMP`, an amount, stops at its ends.
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
                let next = if setting == Setting::Jump {
                    let last = i32::try_from(count).unwrap_or(1) - 1;
                    let moved = (i32::from(choice) + turns).clamp(0, last);
                    usize::try_from(moved).unwrap_or(0)
                } else {
                    wrapped(usize::from(choice), count, turns)
                };
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
        let _ = match setting {
            Setting::Length => write!(out, "{}", settings.length),
            Setting::Chance => write!(out, "{}%", settings.chance_percent),
            Setting::Scale => write!(out, "{}", SCALE_NAMES[settings.scale.index()]),
            Setting::Root => write!(out, "{}", ROOT_NAMES[usize::from(settings.root % 12)]),
            Setting::Octave => write_signed(out, i32::from(settings.octave)),
            Setting::Transpose => write_signed(out, i32::from(settings.transpose)),
            Setting::Spread => write!(out, "{} ST", settings.window.spread),
            Setting::Center => write_percent(out, i32::from(settings.window.center)),
            Setting::Bias => write_percent(out, i32::from(settings.window.bias)),
            Setting::Jump => write!(out, "{}", JUMP_NAMES[jump_choice(settings)]),
            Setting::Chord => write!(out, "{}", CHORD_NAMES[settings.chord.index()]),
            Setting::Gate => write!(out, "{}%", settings.gate_percent),
            // An action, which says how to fire it ([`Self::value_text`]
            // shows `DONE` once fired).
            Setting::Reroll => write!(out, "PRESS"),
            // An arrow, drawn rather than written ([`invert_arrow`]).
            Setting::Invert => Ok(()),
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

    /// A page of settings under the title bar: a row of one field is
    /// highlighted whole when picked; in a row of two, only the field picked.
    fn render_settings(&self, frame: &mut FrameBuffer) {
        let lit = TextStyle::lit(&FONT_5X8);
        let rows = i32::try_from(PAGE_ROWS).unwrap_or(4);
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
        let shown = &ROWS[first..(first + PAGE_ROWS).min(ROWS.len())];
        // In the sequence, the panel is the steps': no setting stands out.
        let picked = self.editing.is_none().then(|| self.selected());
        for (row, &fields) in (1..).zip(shown) {
            let top = row * ROW_HEIGHT;
            let whole = fields.len() == 1 && picked.is_some_and(|picked| fields.contains(&picked));
            if whole {
                row_highlight(frame, top, ROW_HEIGHT, Ink::Lit);
            }
            let style = if whole {
                TextStyle::dark(&FONT_5X8)
            } else {
                lit
            };
            text::draw(
                frame,
                row_label(fields),
                Point::new(LIST_LEFT + 2, top),
                style,
            );
            // The fields right to left, each ending a gap left of the next.
            let mut right = VALUE_RIGHT;
            for &setting in fields.iter().rev() {
                let boxed = fields.len() > 1 && picked == Some(setting);
                let width = self.draw_field(frame, setting, right, top, style, boxed);
                right -= width + FIELD_GAP;
            }
            if let [setting] = fields
                && let Some(assign) = self.assign(*setting)
            {
                crate::modulation::draw_input(frame, assign, WIDTH_I32 - 3, top, style);
            }
        }
    }

    /// Draws `setting`'s value ending at `right` on the row from `top`, in a
    /// lit box when `boxed`: its width, for the field left of it.
    fn draw_field(
        &self,
        frame: &mut FrameBuffer,
        setting: Setting,
        right: i32,
        top: i32,
        style: TextStyle,
        boxed: bool,
    ) -> i32 {
        let style = if boxed {
            TextStyle::dark(&FONT_5X8)
        } else {
            style
        };
        let width = if setting == Setting::Invert {
            ARROW_WIDTH
        } else if self.hidden(setting) {
            0
        } else {
            FONT_5X8.width_of(self.value_text(setting).as_str())
        };
        if boxed {
            gfx::fill_rect(
                frame,
                right - width - 1,
                top,
                width + 2,
                ROW_HEIGHT,
                Ink::Lit,
            );
        }
        if setting == Setting::Invert {
            invert_arrow(
                frame,
                right - ARROW_WIDTH,
                top,
                self.settings.inversion,
                style.color,
            );
        } else if !self.hidden(setting) {
            let value = self.value_text(setting);
            text::draw(
                frame,
                value.as_str(),
                Point::new(right, top),
                style.aligned(Alignment::Right),
            );
        }
        width
    }

    /// `setting`'s value as its row shows it: `REROLL` says `DONE` for a
    /// moment after a reroll, so a press is seen to have acted.
    fn value_text(&self, setting: Setting) -> TextBuf<16> {
        let mut value = TextBuf::<16>::new();
        if setting == Setting::Reroll && self.rerolled_micros > 0 {
            let _ = write!(value, "DONE");
        } else {
            Self::write_value(setting, &self.settings, &mut value);
        }
        value
    }

    /// Whether the screen leaves `setting`'s value out: one that follows its
    /// input, which stands in its place. The window's settings act during a
    /// walk too, which moves in it.
    fn hidden(&self, setting: Setting) -> bool {
        self.assign(setting).is_some_and(Assign::sets)
    }

    /// The steps of the loop at the bottom of the screen, side by side: each
    /// note's mark as high as its pitch, the step playing filled below its
    /// mark, the playhead at the top of its column, the step edited framed.
    fn render_sequence(&self, frame: &mut FrameBuffer) {
        let length = usize::from(self.sequence.length());
        // Scaled to the window's notes, so a mark sits where its note is in
        // the window: the window's width shows as the marks' spread.
        let window = self.window();
        let scale = self.settings.scale;
        let lowest = scale.quantize(window.pitch(0) + self.transpose);
        let highest = scale.quantize(window.pitch(PLACE_MAX) + self.transpose);
        let notes_span = (highest - lowest).max(1);
        let bottom = HEIGHT_I32 - MARK_HEIGHT;
        let height = bottom - NOTES_TOP;
        let count = i32::try_from(length).unwrap_or(1);
        let playing = self.sequence.position();
        for index in 0..i32::try_from(length).unwrap_or(1) {
            let step = usize::try_from(index).unwrap_or(0);
            let note = self.played_in(window, step, self.transpose);
            // Columns from x = 1, a pixel left of the first and one right of
            // each for the editor's frame, which so stays on the screen at
            // both ends.
            let span = WIDTH_I32 - 1;
            let x = 1 + index * span / count;
            let width = (1 + (index + 1) * span / count - x - 1).max(1);
            let y = bottom - (note - lowest).clamp(0, notes_span) * height / notes_span;
            if playing == Some(step) {
                gfx::fill_rect(frame, x, y, width, HEIGHT_I32 - y, Ink::Lit);
                playhead(frame, x, width);
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

/// The playhead over the column from `x`, `width` wide: a triangle at the
/// top of the notes, its tip down, in inverse video, so it shows over a mark
/// that reaches the top as over nothing; cut to the column, narrow at 32
/// steps, so its neighbours keep their marks.
fn playhead(frame: &mut FrameBuffer, x: i32, width: i32) {
    let centre = x + (width - 1) / 2;
    let right = x + width - 1;
    for row in 0..PLAYHEAD_ROWS {
        // Rows narrow towards the tip.
        let half = PLAYHEAD_ROWS - 1 - row;
        let from = (centre - half).max(x);
        let to = (centre + half).min(right);
        gfx::hline(frame, from, NOTES_TOP + row, to - from + 1, Ink::Invert);
    }
}

/// `JUMP`'s value among [`JUMP_NAMES`]: `OFF`, `NO REP`, or the walk.
fn jump_choice(settings: &Settings) -> usize {
    match (settings.jump, settings.repeat) {
        (0, true) => 0,
        (0, false) => 1,
        (jump, _) => 1 + usize::from(jump),
    }
}

/// The inversion as an arrow in the five columns from `x` on the row from
/// `top`: up for the lower tone raised, down for the upper one lowered, a
/// dash for neither.
fn invert_arrow(frame: &mut FrameBuffer, x: i32, top: i32, inversion: i8, ink: Ink) {
    if inversion == 0 {
        gfx::hline(frame, x + 1, top + 3, ARROW_WIDTH - 2, ink);
        return;
    }
    // From the tip, rows widen away from where it points.
    for row in 0..3 {
        let y = if inversion > 0 {
            top + 1 + row
        } else {
            top + 5 - row
        };
        gfx::hline(frame, x + 2 - row, y, 2 * row + 1, ink);
    }
}

/// `value` in percent with its sign, `0%` without.
fn write_percent(out: &mut TextBuf<16>, value: i32) -> core::fmt::Result {
    if value > 0 {
        write!(out, "+{value}%")
    } else {
        write!(out, "{value}%")
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

/// The note, the chord's two tones and the gate, on A to D by default;
/// named by what they carry, the routing page setting the jack, or none
/// (journal/D-046). Four, as many as a route has lanes: the chord has two
/// tones so that the gate has a port (journal/D-047, revision of
/// 2026-11-26).
const PORTS: [Port; 4] = [
    Port { name: "NOTE" },
    Port { name: "CH1" },
    Port { name: "CH2" },
    Port { name: "GATE" },
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
            Setting::Spread => number(i32::from(settings.window.spread)),
            Setting::Center => number(i32::from(settings.window.center)),
            Setting::Bias => number(i32::from(settings.window.bias)),
            Setting::Jump => choice(jump_choice(settings)),
            Setting::Chord => choice(settings.chord.index()),
            Setting::Invert => number(i32::from(settings.inversion)),
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
            (Setting::Spread, ParamValue::Number(spread)) => settings.window.spread = byte(spread),
            (Setting::Center, ParamValue::Number(center)) => {
                settings.window.center = signed(center);
            }
            (Setting::Bias, ParamValue::Number(bias)) => settings.window.bias = signed(bias),
            (Setting::Jump, ParamValue::Choice(choice)) => {
                // `OFF` and `NO REP` draw over the range; past them, a walk.
                settings.repeat = choice != 1;
                settings.jump = choice.saturating_sub(1);
            }
            (Setting::Chord, ParamValue::Choice(chord)) => {
                settings.chord = Chord::from_index(usize::from(chord));
            }
            (Setting::Invert, ParamValue::Number(inversion)) => {
                settings.inversion = signed(inversion);
            }
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
    use super::{
        FIELDS, MelodyApp, PAGES, PARAM_SPECS, ROWS, Setting, Settings, millivolts, row_label,
        sweep,
    };
    use crate::apps::param::ParamValue;
    use crate::apps::{App, InputSnapshot};
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
    fn reroll_says_to_press_and_that_it_acted() {
        let mut app = MelodyApp::new();
        assert_eq!(app.value_text(Setting::Reroll).as_str(), "PRESS");
        app.act(0).unwrap();
        assert_eq!(
            app.value_text(Setting::Reroll).as_str(),
            "DONE",
            "from the probe too"
        );
        let tick = InputSnapshot {
            elapsed_micros: 1_000,
            ..InputSnapshot::default()
        };
        for _ in 0..500 {
            MelodyApp::update(&mut app, &tick);
        }
        assert_eq!(
            app.value_text(Setting::Reroll).as_str(),
            "PRESS",
            "half a second on"
        );
    }

    #[test]
    fn the_rows_hold_every_field_once_in_three_pages() {
        let mut walked = ROWS.iter().flat_map(|row| row.iter().copied());
        for field in FIELDS {
            assert_eq!(walked.next(), Some(field), "the rows read in FIELDS' order");
        }
        assert_eq!(walked.next(), None);
        for setting in Setting::ALL {
            assert!(FIELDS.contains(&setting), "{setting:?}");
        }
        assert_eq!(PAGES, 3);
        assert_eq!(row_label(ROWS[2]), "KEY");
        assert_eq!(row_label(ROWS[3]), "CHORD");
    }

    #[test]
    fn jump_holds_the_free_draw_no_repeat_and_the_walk() {
        let mut app = MelodyApp::new();
        let jump = Setting::Jump.param().unwrap();
        app.set(jump, ParamValue::Choice(1)).unwrap();
        assert_eq!(
            (app.settings.jump, app.settings.repeat),
            (0, false),
            "NO REP"
        );
        app.set(jump, ParamValue::Choice(4)).unwrap();
        assert_eq!(app.settings.jump, 3, "a walk of three");
        assert!(
            !app.hidden(Setting::Spread),
            "the window acts on a walk too"
        );
        app.set(jump, ParamValue::Choice(0)).unwrap();
        assert_eq!((app.settings.jump, app.settings.repeat), (0, true), "OFF");
        app.change(Setting::Jump, -3);
        assert_eq!(app.get(jump), Some(ParamValue::Choice(0)), "stops at OFF");
    }

    #[test]
    fn transpose_follows_its_input_at_a_volt_an_octave() {
        assert_eq!(sweep(Setting::Transpose), Some((-2_000, 2_000)));
        assert_eq!(sweep(Setting::Length), Some((0, 7_750)));
        assert_eq!(
            sweep(Setting::Spread),
            Some((0, 4_000)),
            "a volt an octave of width"
        );
        assert_eq!(sweep(Setting::Center), Some((-5_000, 5_000)));
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
