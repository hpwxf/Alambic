//! The Ratchet applet (journal/D-032): each rising edge on `TR1` is copied to
//! output A and starts a burst of triggers on output B.
//!
//! Each burst setting can take an input (not `PASSTHRU`, `SYNC`, `MODE`),
//! assigned in a submenu (journal/D-035, [`Assign`]): the input's signal is
//! scaled, shifted and bounded, then sets the setting (`SET`, shown `CVn=`),
//! is added to the value set on the panel (`ADD`, `CVn+`) or multiplies it
//! (`MUL`, `CVn*`, 5 V leaving it unchanged). The burst's settings are latched at the edge, inputs included, so a burst runs
//! as it started; a new edge restarts it. Triggers are 5 V for 5 ms, or
//! half the interval when the repeats come faster. Times are counted in the
//! ticks' elapsed microseconds: a repeat falls on the first tick at or past its
//! time, so on the module it is late by less than a tick.
//!
//! The rate of `TR1` is shown in beats per minute (one trigger a beat); with
//! `SYNC ON`, the time setting is a fraction of its period (`/4`: a quarter of
//! the time between two triggers) instead of milliseconds, until the rate is
//! lost.
//!
//! `SHAPE` is one signed curve, −3.0 to +3.0 octaves: 0 even, below 0 the
//! burst accelerates, above 0 it slows down. Two models place the repeats,
//! to compare in situation (`MODE` on page 2, see [`burst`]): `FIRST`, a first
//! interval then intervals changing by a constant factor up to the first times
//! `2^SHAPE`; `SPAN`, the repeats spread over a span, crowding towards its end
//! or its start. `PROB` is the chance each repeat fires.
//!
//! Panel: the left encoder picks a setting, the right one changes it; its
//! switch opens the setting's input submenu, where the left encoder picks a
//! line, the right one changes it, and its switch or `down` closes it; `up`
//! starts a burst by hand.

pub mod burst;
pub mod tempo;

use core::fmt::Write as _;

use self::burst::{
    Burst, CURVE_TENTHS, Count, Effect, MAX_INTERVAL_MICROS, MAX_REPEATS, MAX_SPAN_MICROS,
    MIN_INTERVAL_MICROS, Model, UNIT_MV, chance_from, count_from, curve_from, follow_time_micros,
    time_from,
};
use self::tempo::Tempo;
use crate::apps::{InputSnapshot, TickContext};
use crate::fixed::{self, ONE};
use crate::fmt::TextBuf;
use crate::framebuffer::{FrameBuffer, ROW_HEIGHT, WIDTH_I32};
use crate::gfx::{self, Ink, Point};
use crate::platform::{Button, CV_CHANNELS, CvChannel, MilliVolts};
use crate::text::fonts::FONT_5X8;
use crate::text::{self, Alignment, TextStyle};

/// A trigger's level.
pub const TRIGGER_MV: MilliVolts = 5_000;

/// A trigger's length, unless the repeats come faster.
pub const PULSE_MICROS: u32 = 5_000;

/// Settings shown per page: the burst on the first, the options on the
/// second.
const PAGE_ROWS: usize = 4;

/// Pages of settings.
const PAGES: usize = Setting::ALL.len().div_ceil(PAGE_ROWS);

/// Width of the scroll bar left of the settings, which shows the page.
const SCROLL_WIDTH: i32 = 3;

/// Left edge of the settings' rows, right of the scroll bar and a gap.
const ROW_LEFT: i32 = SCROLL_WIDTH + 1;

/// Top of the burst's timeline, under its playhead; it runs to the bottom
/// of the screen.
const TIMELINE_TOP: i32 = 48;

/// A five-column, three-row triangle at `at`, its tip down.
fn arrow_down(frame: &mut FrameBuffer, at: Point) {
    for row in 0..3 {
        // Rows narrow towards the tip.
        let half = 2 - row;
        gfx::hline(frame, at.x + 2 - half, at.y + row, 2 * half + 1, Ink::Lit);
    }
}

/// Right edge of the value column, left of the input column.
const VALUE_RIGHT: i32 = 98;

/// The submenu's frame: from under its title to above the bottom of the
/// timeline, which stays in view under it.
const OVERLAY_TOP: i32 = 3;

/// Height of the submenu's frame.
const OVERLAY_HEIGHT: i32 = 51;

/// Top of the submenu's first line, under the title in its frame.
const OVERLAY_LINES_TOP: i32 = 7;

/// Left edge of the submenu's text and bars' labels, inside the frame.
const OVERLAY_LEFT: i32 = 4;

/// Right edge of the submenu's text and bars, inside the frame.
const OVERLAY_RIGHT: i32 = WIDTH_I32 - 5;

/// Left edge of the scale on the formula line, after the input.
const SCALE_LEFT: i32 = 34;

/// Left edge of the lower bound's value, after `MIN`.
const MIN_VALUE_LEFT: i32 = 26;

/// Left edge of the effect's value, after `EFFECT`.
const EFFECT_VALUE_LEFT: i32 = 46;

/// Left edge of `MAX`.
const MAX_LABEL_LEFT: i32 = 70;

/// Left edge of the bars, after their labels.
const BAR_LEFT: i32 = 26;

/// Height of a bar.
const BAR_HEIGHT: i32 = 7;

/// Top of the bar of the input's signal; the setting's is one row under.
const SIGNAL_BAR_TOP: i32 = 33;

/// The range the bar of the input's signal spans, either side of 0 V.
const SIGNAL_BAR_MV: MilliVolts = 10_000;

/// `index` moved by `turns` among `len`, wrapping round.
fn wrapped(index: usize, len: usize, turns: i32) -> usize {
    let len = i32::try_from(len).unwrap_or(1);
    let moved = i32::try_from(index).unwrap_or(0) + turns;
    usize::try_from(moved.rem_euclid(len)).unwrap_or(0)
}

/// Shortest trigger.
const MIN_PULSE_MICROS: u32 = 1_000;

/// The settings at start.
const DEFAULT_BURST: Burst = Burst {
    model: Model::First,
    count: Count::Repeats(4),
    time_micros: 100_000,
    curve_tenths: 0,
    chance_percent: 100,
};

/// The longest time a model takes: a first interval, or a span.
const fn longest_time(model: Model) -> u32 {
    match model {
        Model::First => MAX_INTERVAL_MICROS,
        Model::Span => MAX_SPAN_MICROS,
    }
}

/// The settings the panel walks through.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Setting {
    /// How many repeats, or as long as the gate.
    Count,
    /// The first interval (`MODE FIRST`) or the span of the burst
    /// (`MODE SPAN`).
    Time,
    /// The curve, −3.0 to +3.0 octaves: 0 even, below accelerating, above
    /// slowing down.
    Shape,
    /// The chance each repeat fires.
    Chance,
    /// Whether B also carries the incoming trigger.
    Passthru,
    /// Whether `FIRST` is a fraction of `TR1`'s period.
    Sync,
    /// How the repeats are placed: from a first interval, or over a span.
    Mode,
}

impl Setting {
    /// Every setting, top to bottom.
    pub const ALL: [Self; 7] = [
        Self::Count,
        Self::Time,
        Self::Shape,
        Self::Chance,
        Self::Passthru,
        Self::Sync,
        Self::Mode,
    ];

    /// Where this setting's input is kept, `None` for the options, which
    /// take none.
    const fn input_slot(self) -> Option<usize> {
        match self {
            Self::Count => Some(0),
            Self::Time => Some(1),
            Self::Shape => Some(2),
            Self::Chance => Some(3),
            Self::Passthru | Self::Sync | Self::Mode => None,
        }
    }

    const fn label(self) -> &'static str {
        match self {
            Self::Count => "COUNT",
            Self::Time => "FIRST",
            Self::Shape => "SHAPE",
            Self::Chance => "PROB",
            Self::Passthru => "PASSTHRU",
            Self::Sync => "SYNC",
            Self::Mode => "MODE",
        }
    }
}

/// The fractions of `TR1`'s period `FIRST` can be with `SYNC ON`, shortest
/// last: (multiplier, divisor).
pub const DIVISIONS: [(u32, u32); 15] = [
    (2, 1),
    (3, 2),
    (4, 3),
    (1, 1),
    (3, 4),
    (2, 3),
    (1, 2),
    (1, 3),
    (1, 4),
    (1, 6),
    (1, 8),
    (1, 12),
    (1, 16),
    (1, 24),
    (1, 32),
];
/// What `FIRST` is measured in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FirstUnit {
    /// Milliseconds.
    Millis,
    /// A fraction of `TR1`'s period (`SYNC ON`).
    Period,
}

/// The default fraction: a quarter of the period.
const DEFAULT_DIVISION: usize = 8;

/// Bound of `SHIFT`, `MIN` and `MAX` either side of 0 V.
const ASSIGN_RANGE_MV: MilliVolts = 10_000;

/// Bound of `SCALE` either side of 0 %.
const SCALE_RANGE_PERCENT: i32 = 200;

/// `SCALE` per detent.
const SCALE_STEP_PERCENT: i32 = 5;

/// `SHIFT` per detent.
const SHIFT_STEP_MV: MilliVolts = 50;

/// `MIN` and `MAX` per detent.
const BOUND_STEP_MV: MilliVolts = 100;

/// How long a slice of the readings window lasts: the range seen covers the
/// last one to two of them, so it follows a signal that changes (a new
/// waveform, a smaller swing) within twice this, and still holds a whole
/// cycle of an LFO down to a quarter of a hertz.
pub const WINDOW_MICROS: u32 = 2_000_000;

/// The lowest and highest reading lately: the slice under way and the one
/// before, each [`WINDOW_MICROS`] long.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Window {
    current: Option<(MilliVolts, MilliVolts)>,
    previous: Option<(MilliVolts, MilliVolts)>,
    /// Time since the slice under way began.
    age_micros: u32,
}

impl Window {
    const fn new() -> Self {
        Self {
            current: None,
            previous: None,
            age_micros: 0,
        }
    }

    /// Adds `reading`, `elapsed` after the last one; a slice that has lasted
    /// its time becomes the previous one, the oldest is forgotten.
    fn add(&mut self, reading: MilliVolts, elapsed: u32) {
        self.age_micros = self.age_micros.saturating_add(elapsed);
        if self.age_micros >= WINDOW_MICROS {
            self.previous = self.current.take();
            self.age_micros = 0;
        }
        self.current = Some(widened(self.current, (reading, reading)));
    }

    /// The range over both slices.
    fn range(&self) -> Option<(MilliVolts, MilliVolts)> {
        match (self.current, self.previous) {
            (Some(current), previous) => Some(widened(previous, current)),
            (None, previous) => previous,
        }
    }
}

/// `range` widened to take `other` in.
fn widened(
    range: Option<(MilliVolts, MilliVolts)>,
    other: (MilliVolts, MilliVolts),
) -> (MilliVolts, MilliVolts) {
    range.map_or(other, |(low, high)| (low.min(other.0), high.max(other.1)))
}

/// The least span of readings `CENTER` works from.
pub const CENTER_MIN_SPAN_MV: MilliVolts = 100;

/// `numerator / denominator` rounded to the nearest multiple of `step`.
fn rounded(numerator: i64, denominator: i64, step: i64) -> i64 {
    let unit = denominator * step;
    let half = unit / 2 * numerator.signum();
    (numerator + half) / unit * step
}

/// The range of the shaped signal that sweeps `setting` over all its values
/// under `effect` (journal/D-035): from the signal that gives its least
/// value to the one that gives its most when it follows; centred on 0 V,
/// around the panel's value, when it is added; 0 to 10 V (x0 to x2, x1 in
/// the middle) when it multiplies.
#[must_use]
pub fn usable_range(setting: Setting, model: Model, effect: Effect) -> (MilliVolts, MilliVolts) {
    // Octaves as millivolts, at one volt per octave.
    let octaves = |longer: u32, shorter: u32| {
        fixed::log2(i64::from(longer) * i64::from(ONE) / i64::from(shorter.max(1)))
            .map_or(0, |octaves| {
                MilliVolts::try_from(octaves * 1_000 / i64::from(ONE)).unwrap_or(0)
            })
    };
    let (low, high) = match setting {
        // Four repeats per volt, one at 0 V.
        Setting::Count => (0, (i32::from(MAX_REPEATS) - 1) * 1_000 / 4),
        // A volt more halves the time, which follows from its 0 V value.
        Setting::Time => {
            let follow = follow_time_micros(model);
            (
                -octaves(longest_time(model), follow),
                octaves(follow, MIN_INTERVAL_MICROS),
            )
        }
        // A tenth per 100 mV.
        Setting::Shape => (-CURVE_TENTHS * 100, CURVE_TENTHS * 100),
        // 20 % per volt.
        Setting::Chance => (0, 5_000),
        Setting::Passthru | Setting::Sync | Setting::Mode => (0, 0),
    };
    match effect {
        Effect::Set => (low, high),
        Effect::Add => {
            let half = (high - low) / 2;
            (-half, half)
        }
        Effect::Mul => (0, 2 * UNIT_MV),
    }
}

/// The inputs a setting can take, in the order the right encoder turns them.
const INPUTS: [Option<CvChannel>; 5] = [
    None,
    Some(CvChannel::One),
    Some(CvChannel::Two),
    Some(CvChannel::Three),
    Some(CvChannel::Four),
];

/// The effects, in the order the right encoder turns them.
const EFFECTS: [Effect; 3] = [Effect::Set, Effect::Add, Effect::Mul];

/// The input a setting takes and how its signal is shaped (journal/D-035):
/// `clamp(cv × scale + shift, min, max)`, then the effect.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Assign {
    /// The input, `None` for the panel's value alone.
    pub input: Option<CvChannel>,
    /// What the shaped signal does to the panel's value.
    pub effect: Effect,
    /// The input's gain, in percent; negative inverts it.
    pub scale_percent: i32,
    /// Added after the gain.
    pub shift_mv: MilliVolts,
    /// The shaped signal's lower bound.
    pub min_mv: MilliVolts,
    /// The shaped signal's upper bound, never below `min_mv`.
    pub max_mv: MilliVolts,
}

impl Assign {
    /// No input: the signal at unity, unshifted, unbounded, added.
    pub const NONE: Self = Self {
        input: None,
        effect: Effect::Add,
        scale_percent: 100,
        shift_mv: 0,
        min_mv: -ASSIGN_RANGE_MV,
        max_mv: ASSIGN_RANGE_MV,
    };

    /// `millivolts` as the setting receives it: scaled, shifted, bounded.
    #[must_use]
    pub fn shape(&self, millivolts: MilliVolts) -> MilliVolts {
        let shaped =
            i64::from(millivolts) * i64::from(self.scale_percent) / 100 + i64::from(self.shift_mv);
        let bounded = shaped.clamp(i64::from(self.min_mv), i64::from(self.max_mv));
        MilliVolts::try_from(bounded).unwrap_or(0)
    }

    /// The shaped signal and its effect, `None` without an input.
    fn reading(&self, cv: &[MilliVolts; CV_CHANNELS]) -> Option<(MilliVolts, Effect)> {
        self.input
            .map(|channel| (self.shape(cv[channel.index()]), self.effect))
    }

    /// How the main screen names it, as an operation on the panel's value:
    /// `=CV2`, `+CV2` or `*CV2`.
    fn write(&self, out: &mut TextBuf<8>) {
        if let Some(channel) = self.input {
            let symbol = match self.effect {
                Effect::Set => '=',
                Effect::Add => '+',
                Effect::Mul => '*',
            };
            let _ = write!(out, "{symbol}CV{}", channel.index() + 1);
        }
    }

    /// The value of `line` as the submenu shows it: the input, the scale as
    /// a factor (`x0.50`), the shift and the bounds in volts, the effect.
    fn field(&self, line: AssignLine) -> TextBuf<16> {
        let mut out = TextBuf::<16>::new();
        let _ = match line {
            AssignLine::Input => match self.input {
                Some(channel) => write!(out, "CV{}", channel.index() + 1),
                None => write!(out, "--"),
            },
            AssignLine::Scale => {
                let sign = if self.scale_percent < 0 { "-" } else { "" };
                let magnitude = self.scale_percent.unsigned_abs();
                write!(out, "x{sign}{}.{:02}", magnitude / 100, magnitude % 100)
            }
            AssignLine::Shift => {
                write_volts(&mut out, self.shift_mv, 2);
                write!(out, "V")
            }
            AssignLine::Min => {
                write_volts(&mut out, self.min_mv, 1);
                Ok(())
            }
            AssignLine::Max => {
                write_volts(&mut out, self.max_mv, 1);
                Ok(())
            }
            AssignLine::Center => write!(out, "{}", line.label()),
            AssignLine::Effect => write!(
                out,
                "{}",
                match self.effect {
                    Effect::Set => "SET",
                    Effect::Add => "ADD",
                    Effect::Mul => "MUL",
                }
            ),
        };
        out
    }

    /// Sets the scale and shift that carry `seen`, the lowest and highest
    /// reading, onto `target`, the range of the shaped signal the setting can
    /// use: the middles meet, the spans too as far as the scale's range
    /// allows; both rounded to their steps. Leaves them as they are and
    /// returns `false` when the readings span less than [`CENTER_MIN_SPAN_MV`],
    /// a signal too still to measure.
    pub fn center(
        &mut self,
        seen: (MilliVolts, MilliVolts),
        target: (MilliVolts, MilliVolts),
    ) -> bool {
        let span = i64::from(seen.1) - i64::from(seen.0);
        if span < i64::from(CENTER_MIN_SPAN_MV) {
            return false;
        }
        let wanted = i64::from(target.1) - i64::from(target.0);
        let scale = rounded(wanted * 100, span, i64::from(SCALE_STEP_PERCENT)).clamp(
            i64::from(SCALE_STEP_PERCENT),
            i64::from(SCALE_RANGE_PERCENT),
        );
        // Twice the middles, to stay in whole millivolts.
        let seen_middle = i64::from(seen.0) + i64::from(seen.1);
        let target_middle = i64::from(target.0) + i64::from(target.1);
        let shift = rounded(
            target_middle * 100 - seen_middle * scale,
            200,
            i64::from(SHIFT_STEP_MV),
        )
        .clamp(-i64::from(ASSIGN_RANGE_MV), i64::from(ASSIGN_RANGE_MV));
        self.scale_percent = i32::try_from(scale).unwrap_or(100);
        self.shift_mv = MilliVolts::try_from(shift).unwrap_or(0);
        true
    }

    /// Whether the setting follows its input, the panel's value unused.
    const fn sets(&self) -> bool {
        self.input.is_some() && matches!(self.effect, Effect::Set)
    }

    /// Turns `line` by `turns` detents, each value stopping at its ends.
    fn turn(&mut self, line: AssignLine, turns: i32) {
        match line {
            AssignLine::Input => self.input = step_through(&INPUTS, self.input, turns),
            AssignLine::Effect => self.effect = step_through(&EFFECTS, self.effect, turns),
            AssignLine::Center => {}
            AssignLine::Scale => {
                self.scale_percent = (self.scale_percent + turns * SCALE_STEP_PERCENT)
                    .clamp(-SCALE_RANGE_PERCENT, SCALE_RANGE_PERCENT);
            }
            AssignLine::Shift => {
                self.shift_mv = (self.shift_mv + turns * SHIFT_STEP_MV)
                    .clamp(-ASSIGN_RANGE_MV, ASSIGN_RANGE_MV);
            }
            AssignLine::Min => {
                self.min_mv =
                    (self.min_mv + turns * BOUND_STEP_MV).clamp(-ASSIGN_RANGE_MV, self.max_mv);
            }
            AssignLine::Max => {
                self.max_mv =
                    (self.max_mv + turns * BOUND_STEP_MV).clamp(self.min_mv, ASSIGN_RANGE_MV);
            }
        }
    }
}

/// The value `turns` places from `current` in `values`, stopping at both
/// ends.
fn step_through<T: Copy + PartialEq>(values: &[T], current: T, turns: i32) -> T {
    let last = i32::try_from(values.len()).unwrap_or(1) - 1;
    let index = values
        .iter()
        .position(|&value| value == current)
        .unwrap_or(0);
    let next = (i32::try_from(index).unwrap_or(0) + turns).clamp(0, last);
    values[usize::try_from(next).unwrap_or(0)]
}

/// The fields of the input submenu, in the order the left encoder picks
/// them: the input, its scale and shift on the first line, read as a
/// formula, the bounds on the second, the effect and `CENTER` on the third.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AssignLine {
    /// Which input.
    Input,
    /// The gain.
    Scale,
    /// The offset.
    Shift,
    /// The lower bound.
    Min,
    /// The upper bound.
    Max,
    /// What the signal does to the panel's value.
    Effect,
    /// An action, not a value: the right encoder's click sets the scale and
    /// shift that carry the readings seen onto the setting's usable range.
    Center,
}

impl AssignLine {
    /// Every line, top to bottom.
    pub const ALL: [Self; 7] = [
        Self::Input,
        Self::Scale,
        Self::Shift,
        Self::Min,
        Self::Max,
        Self::Effect,
        Self::Center,
    ];

    const fn label(self) -> &'static str {
        match self {
            Self::Input => "INPUT",
            Self::Scale => "SCALE",
            Self::Shift => "SHIFT",
            Self::Min => "MIN",
            Self::Max => "MAX",
            Self::Effect => "EFFECT",
            Self::Center => "CENTER",
        }
    }
}

/// Writes `millivolts` as signed volts with `decimals` (1 or 2) digits after
/// the point, truncated: `+1.23`, `-0.5`.
fn write_volts<const N: usize>(out: &mut TextBuf<N>, millivolts: MilliVolts, decimals: u32) {
    let sign = if millivolts < 0 { '-' } else { '+' };
    let magnitude = millivolts.unsigned_abs();
    let _ = if decimals >= 2 {
        write!(
            out,
            "{sign}{}.{:02}",
            magnitude / 1_000,
            magnitude % 1_000 / 10
        )
    } else {
        write!(
            out,
            "{sign}{}.{}",
            magnitude / 1_000,
            magnitude % 1_000 / 100
        )
    };
}

/// A burst under way.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Running {
    /// Its settings, latched at the edge.
    burst: Burst,
    /// Repeats already sent.
    sent: u32,
    /// Time since the last trigger of the burst.
    since_micros: u32,
    /// The triggers B carried so far: bit 0 the incoming one, bit `n` the
    /// `n`th repeat.
    fired: u64,
}

impl Running {
    /// Time since the burst started.
    fn elapsed_micros(&self) -> u64 {
        (1..=self.sent)
            .map(|index| u64::from(self.burst.interval_micros(index)))
            .sum::<u64>()
            + u64::from(self.since_micros)
    }
}

/// Retriggers `TR1` onto A and bursts it onto B.
#[derive(Debug, Clone)]
pub struct RatchetApp {
    settings: Burst,
    passthru: bool,
    /// What `FIRST` is measured in (`SYNC`).
    first_unit: FirstUnit,
    /// The fraction, an index in [`DIVISIONS`].
    division: usize,
    /// The rate of `TR1`.
    tempo: Tempo,
    /// The state of the generator that rolls each repeat's chance.
    seed: u32,
    /// The input of each setting that takes one (see `Setting::input_slot`).
    assigns: [Assign; 4],
    selected: usize,
    /// The line picked in the selected setting's input submenu, `None` while
    /// it is closed.
    assigning: Option<usize>,
    /// The lowest and highest reading of the submenu's input, lately.
    observed: Window,
    gate: bool,
    running: Option<Running>,
    pulse_a_micros: u32,
    pulse_b_micros: u32,
    cv: [MilliVolts; CV_CHANNELS],
    outputs: [MilliVolts; CV_CHANNELS],
}

impl RatchetApp {
    /// The default settings, nothing running.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            settings: DEFAULT_BURST,
            passthru: true,
            first_unit: FirstUnit::Millis,
            division: DEFAULT_DIVISION,
            tempo: Tempo::new(),
            assigns: [Assign::NONE; 4],
            seed: 0x2545_F491,
            selected: 0,
            assigning: None,
            observed: Window::new(),
            gate: false,
            running: None,
            pulse_a_micros: 0,
            pulse_b_micros: 0,
            cv: [0; CV_CHANNELS],
            outputs: [0; CV_CHANNELS],
        }
    }

    /// The burst settings as the panel sets them.
    #[must_use]
    pub const fn settings(&self) -> &Burst {
        &self.settings
    }

    /// Whether B carries the incoming trigger too.
    #[must_use]
    pub const fn passthru(&self) -> bool {
        self.passthru
    }

    /// The setting the right encoder changes.
    #[must_use]
    pub const fn selected(&self) -> Setting {
        Setting::ALL[self.selected]
    }

    /// The rate of `TR1` in beats per minute, `None` when unknown.
    #[must_use]
    pub fn bpm(&self) -> Option<u32> {
        self.tempo.bpm()
    }

    /// Whether `FIRST` is a fraction of `TR1`'s period, and which.
    #[must_use]
    pub const fn sync(&self) -> Option<(u32, u32)> {
        match self.first_unit {
            FirstUnit::Period => Some(DIVISIONS[self.division]),
            FirstUnit::Millis => None,
        }
    }

    /// The input `setting` takes, `None` for one that takes none.
    #[must_use]
    pub fn assign(&self, setting: Setting) -> Option<&Assign> {
        setting.input_slot().map(|slot| &self.assigns[slot])
    }

    /// The page of settings on screen, from 0: the selected setting's.
    #[must_use]
    pub const fn page(&self) -> usize {
        self.selected / PAGE_ROWS
    }

    /// The line picked in the selected setting's input submenu, `None`
    /// while it is closed.
    #[must_use]
    pub const fn assigning(&self) -> Option<AssignLine> {
        match self.assigning {
            Some(line) => Some(AssignLine::ALL[line]),
            None => None,
        }
    }

    /// The lowest and highest reading of the submenu's input over the last
    /// [`WINDOW_MICROS`] to twice that, `None` before the first.
    #[must_use]
    pub fn observed(&self) -> Option<(MilliVolts, MilliVolts)> {
        self.observed.range()
    }

    /// Repeats sent by the burst under way, `None` when none runs.
    #[must_use]
    pub fn repeats_sent(&self) -> Option<u32> {
        self.running.map(|running| running.sent)
    }

    /// Follows the panel and `TR1`, runs the burst, sets the outputs.
    pub fn update(&mut self, input: &InputSnapshot) -> [MilliVolts; CV_CHANNELS] {
        self.follow_panel(input);
        let elapsed = input.elapsed_micros;
        self.pulse_a_micros = self.pulse_a_micros.saturating_sub(elapsed);
        self.pulse_b_micros = self.pulse_b_micros.saturating_sub(elapsed);
        self.cv = input.cv;
        self.observe(elapsed);

        let gate = input.triggers[0];
        let rising = gate && !self.gate;
        self.gate = gate;
        self.tempo.advance(elapsed);
        if rising {
            self.tempo.edge();
        }
        self.advance(elapsed, gate);
        if rising || input.buttons.pressed(Button::Up) {
            self.start();
        }

        self.outputs = [
            if self.pulse_a_micros > 0 {
                TRIGGER_MV
            } else {
                0
            },
            if self.pulse_b_micros > 0 {
                TRIGGER_MV
            } else {
                0
            },
            0,
            0,
        ];
        self.outputs
    }

    /// The settings of a burst started now: the panel's, with each setting's
    /// input applied as it reads.
    fn latched(&self) -> Burst {
        self.with_inputs(self.synced())
    }

    /// The panel's settings, the time a fraction of `TR1`'s period with
    /// `SYNC ON` and the rate known, before the inputs.
    fn synced(&self) -> Burst {
        let mut burst = self.settings;
        if let (Some((multiplier, divisor)), Some(period)) =
            (self.sync(), self.tempo.period_micros())
        {
            let time = u64::from(period) * u64::from(multiplier) / u64::from(divisor);
            let bounded = time.clamp(
                u64::from(MIN_INTERVAL_MICROS),
                u64::from(longest_time(burst.model)),
            );
            burst.time_micros = u32::try_from(bounded).unwrap_or(MAX_INTERVAL_MICROS);
        }
        burst
    }

    /// `burst` with each setting's input applied as it reads.
    fn with_inputs(&self, mut burst: Burst) -> Burst {
        let reading = |setting: Setting| {
            self.assign(setting)
                .and_then(|assign| assign.reading(&self.cv))
        };
        if let Some((millivolts, effect)) = reading(Setting::Count) {
            burst.count = count_from(burst.count, millivolts, effect);
        }
        if let Some((millivolts, effect)) = reading(Setting::Time) {
            burst.time_micros = time_from(burst.model, burst.time_micros, millivolts, effect);
        }
        if let Some((millivolts, effect)) = reading(Setting::Shape) {
            burst.curve_tenths = curve_from(burst.curve_tenths, millivolts, effect);
        }
        if let Some((millivolts, effect)) = reading(Setting::Chance) {
            burst.chance_percent = chance_from(burst.chance_percent, millivolts, effect);
        }
        burst
    }

    /// Starts a burst: the trigger on A, on B too if set, the first repeat
    /// one interval later.
    fn start(&mut self) {
        let burst = self.latched();
        self.pulse_a_micros = PULSE_MICROS;
        if self.passthru {
            self.pulse_b_micros = pulse_for(burst.interval_micros(1));
        }
        self.running = Some(Running {
            burst,
            sent: 0,
            since_micros: 0,
            fired: u64::from(self.passthru),
        });
    }

    /// Sends the repeats whose time has come; ends the burst when it has sent
    /// them all, or when its gate fell.
    fn advance(&mut self, elapsed: u32, gate: bool) {
        let Some(mut running) = self.running else {
            return;
        };
        running.since_micros = running.since_micros.saturating_add(elapsed);
        loop {
            if running.sent >= running.burst.max_repeats()
                || (running.burst.count == Count::Gate && !gate)
            {
                self.running = None;
                return;
            }
            let interval = running.burst.interval_micros(running.sent + 1);
            if running.since_micros < interval {
                break;
            }
            running.since_micros -= interval;
            running.sent += 1;
            let next = running.burst.interval_micros(running.sent + 1);
            // A repeat that does not fire leaves its time to pass all the same.
            if self.roll(running.burst.chance_percent) {
                self.pulse_b_micros = pulse_for(interval.min(next));
                running.fired |= 1_u64.checked_shl(running.sent).unwrap_or(0);
            }
        }
        self.running = Some(running);
    }

    fn follow_panel(&mut self, input: &InputSnapshot) {
        let picks = i32::from(input.controls.delta(0));
        let turns = i32::from(input.controls.delta(1));
        if let (Some(line), Some(slot)) = (self.assigning, self.selected().input_slot()) {
            self.follow_submenu(input, line, slot, picks, turns);
            return;
        }
        if picks != 0 {
            self.selected = wrapped(self.selected, Setting::ALL.len(), picks);
        }
        if input.buttons.pressed(Button::RightEncoder) && self.selected().input_slot().is_some() {
            self.assigning = Some(0);
            self.observed = Window::new();
        }
        if turns != 0 {
            self.change(self.selected(), turns);
        }
    }

    /// The panel in the input submenu of the setting whose input is kept in
    /// `slot`, `line` picked.
    fn follow_submenu(
        &mut self,
        input: &InputSnapshot,
        line: usize,
        slot: usize,
        picks: i32,
        turns: i32,
    ) {
        let centering = AssignLine::ALL[line] == AssignLine::Center;
        if input.buttons.pressed(Button::RightEncoder) && centering {
            if let Some(seen) = self.observed.range() {
                let target = usable_range(
                    self.selected(),
                    self.settings.model,
                    self.assigns[slot].effect,
                );
                self.assigns[slot].center(seen, target);
            }
            return;
        }
        if input.buttons.pressed(Button::RightEncoder) || input.buttons.pressed(Button::Down) {
            self.assigning = None;
            return;
        }
        let line = wrapped(line, AssignLine::ALL.len(), picks);
        self.assigning = Some(line);
        if turns != 0 {
            let assign = &mut self.assigns[slot];
            let before = assign.input;
            assign.turn(AssignLine::ALL[line], turns);
            if assign.input != before {
                self.observed = Window::new();
            }
        }
    }

    /// Adds the input's reading to the submenu's window, `elapsed` after the
    /// last.
    fn observe(&mut self, elapsed: u32) {
        let Some(channel) = self
            .assigning
            .and(self.assign(self.selected()))
            .and_then(|assign| assign.input)
        else {
            return;
        };
        self.observed.add(self.cv[channel.index()], elapsed);
    }

    /// Turns `setting` by `turns` detents.
    fn change(&mut self, setting: Setting, turns: i32) {
        let burst = &mut self.settings;
        match setting {
            Setting::Count => {
                burst.count = match burst.count {
                    Count::Gate if turns < 0 => Count::Repeats(MAX_REPEATS),
                    Count::Gate => Count::Gate,
                    Count::Repeats(count) => {
                        let next = i32::from(count) + turns;
                        if next > i32::from(MAX_REPEATS) {
                            Count::Gate
                        } else {
                            Count::Repeats(u8::try_from(next.max(1)).unwrap_or(1))
                        }
                    }
                };
            }
            Setting::Time if self.first_unit == FirstUnit::Period => {
                let last = i32::try_from(DIVISIONS.len()).unwrap_or(1) - 1;
                let next = (i32::try_from(self.division).unwrap_or(0) + turns).clamp(0, last);
                self.division = usize::try_from(next).unwrap_or(DEFAULT_DIVISION);
            }
            Setting::Time => {
                // A sixteenth of the time per detent, at least a
                // millisecond, kept in whole milliseconds.
                let millis = burst.time_micros / 1_000;
                let step = (millis / 16).max(1);
                let step = i64::from(step) * i64::from(turns);
                let next = (i64::from(millis) + step) * 1_000;
                let bounded = next.clamp(
                    i64::from(MIN_INTERVAL_MICROS),
                    i64::from(longest_time(burst.model)),
                );
                burst.time_micros = u32::try_from(bounded).unwrap_or(MIN_INTERVAL_MICROS);
            }
            Setting::Shape => {
                burst.curve_tenths =
                    (burst.curve_tenths + turns).clamp(-CURVE_TENTHS, CURVE_TENTHS);
            }
            Setting::Chance => {
                let next = i64::from(burst.chance_percent) + i64::from(turns);
                burst.chance_percent = u32::try_from(next.clamp(1, 100)).unwrap_or(100);
            }
            Setting::Passthru => self.passthru = !self.passthru,
            Setting::Sync => {
                self.first_unit = match self.first_unit {
                    FirstUnit::Millis => FirstUnit::Period,
                    FirstUnit::Period => FirstUnit::Millis,
                };
            }
            Setting::Mode => {
                burst.model = match burst.model {
                    Model::First => Model::Span,
                    Model::Span => Model::First,
                };
                // Each model keeps its time within its own range.
                burst.time_micros = burst.time_micros.min(longest_time(burst.model));
            }
        }
    }

    /// Whether a repeat with `chance` percent fires: a xorshift generator,
    /// seeded the same at every start, so a run replays exactly.
    const fn roll(&mut self, chance: u32) -> bool {
        if chance >= 100 {
            return true;
        }
        let mut x = self.seed;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.seed = x;
        x % 100 < chance
    }

    /// Draws the settings, a timeline of the burst they make now, and the
    /// state of `TR1`, A and B.
    pub fn render(&self, frame: &mut FrameBuffer, _context: &TickContext) {
        frame.clear();
        self.render_main(frame);
        if let (Some(line), Some(assign)) = (self.assigning(), self.assign(self.selected())) {
            self.render_submenu(frame, line, assign);
        }
    }

    /// The main screen: the title bar, the page of settings, the timeline.
    fn render_main(&self, frame: &mut FrameBuffer) {
        let lit = TextStyle::lit(&FONT_5X8);
        text::draw(frame, "RATCHET", Point::zero(), lit);
        let mut rate = TextBuf::<12>::new();
        let _ = match self.tempo.bpm() {
            Some(bpm) => write!(rate, "{bpm:>3} BPM"),
            None => write!(rate, "--- BPM"),
        };
        text::draw(frame, rate.as_str(), Point::new(42, 0), lit);
        self.render_lights(frame);

        let rows = i32::try_from(PAGE_ROWS).unwrap_or(4);
        self.render_scroll_bar(frame, rows * ROW_HEIGHT);
        let first = self.page() * PAGE_ROWS;
        let shown = &Setting::ALL[first..(first + PAGE_ROWS).min(Setting::ALL.len())];
        for (row, &setting) in (1..).zip(shown) {
            let top = row * ROW_HEIGHT;
            let highlighted = setting == self.selected();
            if highlighted {
                gfx::fill_rect(
                    frame,
                    ROW_LEFT,
                    top,
                    WIDTH_I32 - ROW_LEFT,
                    ROW_HEIGHT,
                    Ink::Lit,
                );
            }
            let style = if highlighted {
                TextStyle::dark(&FONT_5X8)
            } else {
                lit
            };
            text::draw(
                frame,
                self.label(setting),
                Point::new(ROW_LEFT + 2, top),
                style,
            );
            // A setting that follows its input has no use for the panel's
            // value: its input stands in its place.
            if !self.assign(setting).is_some_and(Assign::sets) {
                let mut value = TextBuf::<16>::new();
                self.write_value(setting, &self.settings, self.sync(), &mut value);
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

        self.render_timeline(frame, TIMELINE_TOP);
    }

    /// How the screen names `setting`: the time is a span in `MODE SPAN`.
    fn label(&self, setting: Setting) -> &'static str {
        match (setting, self.settings.model) {
            (Setting::Time, Model::Span) => "SPAN",
            _ => setting.label(),
        }
    }

    /// The lights of `TR1`, A and B at the right of the title bar.
    fn render_lights(&self, frame: &mut FrameBuffer) {
        let lights = [
            ("1", self.gate),
            ("A", self.pulse_a_micros > 0),
            ("B", self.pulse_b_micros > 0),
        ];
        for (index, (name, on)) in (0..).zip(lights) {
            let x = WIDTH_I32 - 30 + index * 10;
            if on {
                gfx::fill_rect(frame, x - 1, 0, 7, ROW_HEIGHT, Ink::Lit);
                text::draw(frame, name, Point::new(x, 0), TextStyle::dark(&FONT_5X8));
            } else {
                text::draw(frame, name, Point::new(x, 0), TextStyle::lit(&FONT_5X8));
            }
        }
    }

    /// The input submenu of the selected setting, `line` picked, as a frame
    /// over the main screen whose top edge carries the setting and the value
    /// a burst would take now: the input, scale and shift read as a formula,
    /// the bounds, the effect, then a bar of the input's signal and one of
    /// the setting it moves. The bottom of the timeline stays in view.
    fn render_submenu(&self, frame: &mut FrameBuffer, line: AssignLine, assign: &Assign) {
        let setting = self.selected();
        let latched = self.latched();
        let mut now = TextBuf::<16>::new();
        self.write_value(setting, &latched, None, &mut now);
        render_overlay_frame(frame, self.label(setting), now.as_str());

        let lit = TextStyle::lit(&FONT_5X8);
        for (shown, row, x) in [
            (AssignLine::Min, 1, OVERLAY_LEFT),
            (AssignLine::Max, 1, MAX_LABEL_LEFT),
            (AssignLine::Effect, 2, OVERLAY_LEFT),
        ] {
            text::draw(frame, shown.label(), Point::new(x, overlay_row(row)), lit);
        }
        for (shown, row, x, align) in [
            (AssignLine::Input, 0, OVERLAY_LEFT, Alignment::Left),
            (AssignLine::Scale, 0, SCALE_LEFT, Alignment::Left),
            (AssignLine::Shift, 0, OVERLAY_RIGHT, Alignment::Right),
            (AssignLine::Min, 1, MIN_VALUE_LEFT, Alignment::Left),
            (AssignLine::Max, 1, OVERLAY_RIGHT, Alignment::Right),
            (AssignLine::Effect, 2, EFFECT_VALUE_LEFT, Alignment::Left),
            (AssignLine::Center, 2, OVERLAY_RIGHT, Alignment::Right),
        ] {
            let value = assign.field(shown);
            let width = FONT_5X8.width_of(value.as_str());
            let left = if align == Alignment::Right {
                x - width
            } else {
                x
            };
            let top = overlay_row(row);
            let style = if shown == line {
                // The field the right encoder turns, in a lit box.
                gfx::fill_rect(frame, left - 2, top, width + 4, ROW_HEIGHT, Ink::Lit);
                TextStyle::dark(&FONT_5X8)
            } else {
                if shown == AssignLine::Center {
                    // An action: a button, outlined.
                    gfx::rect(frame, left - 2, top, width + 4, ROW_HEIGHT, Ink::Lit);
                }
                lit
            };
            text::draw(frame, value.as_str(), Point::new(left, top), style);
        }

        self.render_signal_bar(frame, assign, SIGNAL_BAR_TOP);
        self.render_setting_bar(
            frame,
            setting,
            assign,
            &latched,
            SIGNAL_BAR_TOP + ROW_HEIGHT + 3,
        );
    }

    /// The bar of the input's signal at `top`, −10 V to +10 V with a mark at
    /// 0 V, on the scale of what the setting receives: the signal once
    /// scaled, shifted and bounded as a band from 0 V and a block, so that
    /// `SCALE` and `SHIFT` move them; the reading as it comes as a thin
    /// stroke, taller than the block, the lowest to highest reading seen as a
    /// line along the top; the bounds, when they bound anything, as brackets.
    fn render_signal_bar(&self, frame: &mut FrameBuffer, assign: &Assign, top: i32) {
        text::draw(
            frame,
            "CV",
            Point::new(OVERLAY_LEFT, top),
            TextStyle::lit(&FONT_5X8),
        );
        let width = OVERLAY_RIGHT - BAR_LEFT;
        let x_of = |millivolts: MilliVolts| {
            let clamped = millivolts.clamp(-SIGNAL_BAR_MV, SIGNAL_BAR_MV);
            BAR_LEFT + (clamped + SIGNAL_BAR_MV) * width / (2 * SIGNAL_BAR_MV)
        };
        render_bar_frame(frame, top, x_of(0));
        if assign.min_mv > -SIGNAL_BAR_MV || assign.max_mv < SIGNAL_BAR_MV {
            for (x, inward) in [(x_of(assign.min_mv), 1), (x_of(assign.max_mv), -1)] {
                gfx::vline(frame, x, top - 1, BAR_HEIGHT + 2, Ink::Lit);
                gfx::pixel(frame, x + inward, top - 1, Ink::Lit);
                gfx::pixel(frame, x + inward, top + BAR_HEIGHT, Ink::Lit);
            }
        }
        let Some(channel) = assign.input else {
            return;
        };
        if let Some((low, high)) = self.observed.range() {
            gfx::hline(frame, x_of(low), top, x_of(high) - x_of(low) + 1, Ink::Lit);
        }
        let reading = self.cv[channel.index()];
        let shaped = x_of(assign.shape(reading));
        let zero = x_of(0);
        let (left, right) = (zero.min(shaped), zero.max(shaped));
        gfx::fill_rect(frame, left, top + 2, right - left + 1, 3, Ink::Lit);
        gfx::fill_rect(frame, shaped - 1, top + 1, 3, BAR_HEIGHT - 2, Ink::Lit);
        gfx::vline(frame, x_of(reading), top - 1, BAR_HEIGHT + 2, Ink::Lit);
    }

    /// The bar of `setting` at `top`, over its whole range: the panel's value
    /// as a tick (unless the input sets it), the value a burst would take now
    /// as a block, and the stretch the input moves it as a band between.
    fn render_setting_bar(
        &self,
        frame: &mut FrameBuffer,
        setting: Setting,
        assign: &Assign,
        latched: &Burst,
        top: i32,
    ) {
        text::draw(
            frame,
            "VAL",
            Point::new(OVERLAY_LEFT, top),
            TextStyle::lit(&FONT_5X8),
        );
        let width = OVERLAY_RIGHT - BAR_LEFT;
        let x_of = |permille: i32| BAR_LEFT + permille.clamp(0, 1_000) * width / 1_000;
        // A bipolar setting is marked at its middle, the others at their start.
        let zero = if setting == Setting::Shape { 500 } else { 0 };
        render_bar_frame(frame, top, x_of(zero));
        let now = x_of(position(setting, latched));
        let from = if assign.sets() {
            x_of(zero)
        } else {
            let panel = x_of(position(setting, &self.synced()));
            gfx::vline(frame, panel, top - 1, BAR_HEIGHT + 2, Ink::Lit);
            panel
        };
        let (left, right) = (from.min(now), from.max(now));
        gfx::fill_rect(frame, left, top + 2, right - left + 1, 3, Ink::Lit);
        gfx::fill_rect(frame, now - 1, top, 3, BAR_HEIGHT, Ink::Lit);
    }

    /// The scroll bar left of the settings, `height` pixels under the title
    /// bar: a dotted track, and a thumb over the share of it the page on
    /// screen takes.
    fn render_scroll_bar(&self, frame: &mut FrameBuffer, height: i32) {
        let pages = i32::try_from(PAGES).unwrap_or(1);
        let page = i32::try_from(self.page()).unwrap_or(0);
        gfx::dotted_vline(frame, SCROLL_WIDTH / 2, ROW_HEIGHT, height, 2, Ink::Lit);
        let top = ROW_HEIGHT + page * height / pages;
        let bottom = ROW_HEIGHT + (page + 1) * height / pages;
        gfx::fill_rect(frame, 0, top, SCROLL_WIDTH, bottom - top, Ink::Lit);
    }

    /// The value of `setting` in `settings`, as the screen shows it; the
    /// time as a fraction of `TR1`'s period when `sync` gives one.
    fn write_value(
        &self,
        setting: Setting,
        settings: &Burst,
        sync: Option<(u32, u32)>,
        out: &mut TextBuf<16>,
    ) {
        let _ = match setting {
            Setting::Count => match settings.count {
                Count::Repeats(count) => write!(out, "{count}"),
                Count::Gate => write!(out, "GATE"),
            },
            Setting::Time => match sync {
                Some((1, divisor)) => write!(out, "/{divisor}"),
                Some((multiplier, 1)) => write!(out, "X{multiplier}"),
                Some((3, 2)) => write!(out, "X1.5"),
                Some((multiplier, divisor)) => write!(out, "X{multiplier}/{divisor}"),
                None => write!(out, "{} MS", settings.time_micros / 1_000),
            },
            Setting::Shape => {
                let sign = match settings.curve_tenths {
                    0 => "",
                    curve if curve < 0 => "-",
                    _ => "+",
                };
                let tenths = settings.curve_tenths.unsigned_abs();
                write!(out, "{sign}{}.{}", tenths / 10, tenths % 10)
            }
            Setting::Chance => write!(out, "{}%", settings.chance_percent),
            Setting::Passthru => write!(out, "{}", if self.passthru { "ON" } else { "OFF" }),
            Setting::Sync => write!(
                out,
                "{}",
                if self.first_unit == FirstUnit::Period {
                    "ON"
                } else {
                    "OFF"
                }
            ),
            Setting::Mode => write!(
                out,
                "{}",
                match settings.model {
                    Model::First => "FIRST",
                    Model::Span => "SPAN",
                }
            ),
        };
    }

    /// The triggers of the burst on B, as marks on a line (eight repeats for
    /// a gate burst), from `top` to the bottom of the screen: the burst under
    /// way, or else the one a trigger would start now.
    ///
    /// Under way, a playhead above the line follows the burst: the triggers
    /// B carried are drawn bold, the repeats `PROB` dropped as a dot.
    ///
    /// With `TR1`'s rate known, a dotted line marks where the next trigger is
    /// due, one period after the start: the repeats past it would be cut by
    /// the restart, and are drawn short. The line is scaled to the longer of
    /// the burst and the period, so the mark stays on screen.
    fn render_timeline(&self, frame: &mut FrameBuffer, top: i32) {
        let burst = self
            .running
            .map_or_else(|| self.latched(), |running| running.burst);
        let repeats = burst.max_repeats().min(if burst.count == Count::Gate {
            8
        } else {
            u32::MAX
        });
        let total: u64 = (1..=repeats)
            .map(|index| u64::from(burst.interval_micros(index)))
            .sum();
        let next_trigger = self.tempo.period_micros().map(u64::from);
        let span = total.max(next_trigger.unwrap_or(0)).max(1);
        let baseline = crate::framebuffer::HEIGHT_I32 - 1;
        let height = baseline - top;
        gfx::hline(frame, 0, baseline, WIDTH_I32, Ink::Lit);
        let width = u64::from((WIDTH_I32 - 1).unsigned_abs());
        let x_of = |at: u64| i32::try_from(at.min(span) * width / span).unwrap_or(0);
        let mark = |frame: &mut FrameBuffer, at: u64, index: u32, tall: bool| {
            let x = x_of(at);
            let length = if tall { height } else { height / 3 };
            let Some(running) = self.running.filter(|running| index <= running.sent) else {
                gfx::vline(frame, x, baseline - length, length, Ink::Lit);
                return;
            };
            if running.fired & 1_u64.checked_shl(index).unwrap_or(0) != 0 {
                gfx::fill_rect(frame, x - 1, baseline - length, 3, length, Ink::Lit);
            } else {
                gfx::pixel(frame, x, baseline - 1, Ink::Lit);
            }
        };
        mark(frame, 0, 0, self.passthru);
        let mut at: u64 = 0;
        for index in 1..=repeats {
            at += u64::from(burst.interval_micros(index));
            let cut = next_trigger.is_some_and(|due| at >= due);
            mark(frame, at, index, !cut);
        }
        if let Some(due) = next_trigger {
            gfx::dotted_vline(frame, x_of(due), top, height, 2, Ink::Lit);
        }
        if let Some(running) = self.running {
            let x = x_of(running.elapsed_micros());
            arrow_down(frame, Point::new(x - 2, top - 4));
        }
    }
}

/// Top of line `row` (from 0) in the submenu's frame.
const fn overlay_row(row: i32) -> i32 {
    OVERLAY_LINES_TOP + row * ROW_HEIGHT
}

/// Clears the submenu's place and draws its frame, `label` and `now` each in
/// a gap of its top edge, a pixel clear either side.
fn render_overlay_frame(frame: &mut FrameBuffer, label: &str, now: &str) {
    let lit = TextStyle::lit(&FONT_5X8);
    gfx::fill_rect(
        frame,
        0,
        0,
        WIDTH_I32,
        OVERLAY_TOP + OVERLAY_HEIGHT + 1,
        Ink::Dark,
    );
    gfx::rect(frame, 0, OVERLAY_TOP, WIDTH_I32, OVERLAY_HEIGHT, Ink::Lit);
    gfx::fill_rect(
        frame,
        4,
        0,
        FONT_5X8.width_of(label) + 4,
        ROW_HEIGHT - 1,
        Ink::Dark,
    );
    text::draw(frame, label, Point::new(6, 0), lit);
    let right = WIDTH_I32 - 6;
    let width = FONT_5X8.width_of(now);
    gfx::fill_rect(
        frame,
        right - width - 2,
        0,
        width + 4,
        ROW_HEIGHT - 1,
        Ink::Dark,
    );
    text::draw(
        frame,
        now,
        Point::new(right, 0),
        lit.aligned(Alignment::Right),
    );
}

/// The outline of a bar at `top`, between the bars' edges, with a mark at
/// `zero` above and below.
fn render_bar_frame(frame: &mut FrameBuffer, top: i32, zero: i32) {
    gfx::hline(
        frame,
        BAR_LEFT,
        top + BAR_HEIGHT / 2,
        OVERLAY_RIGHT - BAR_LEFT + 1,
        Ink::Lit,
    );
    gfx::vline(frame, BAR_LEFT, top, BAR_HEIGHT, Ink::Lit);
    gfx::vline(frame, OVERLAY_RIGHT, top, BAR_HEIGHT, Ink::Lit);
    gfx::vline(frame, zero, top, 2, Ink::Lit);
    gfx::vline(frame, zero, top + BAR_HEIGHT - 2, 2, Ink::Lit);
}

/// Where `setting` sits in its range in `burst`, in thousandths: the count
/// from one repeat to `GATE`, the time on a logarithmic scale (the inputs
/// move it by octaves), the curve from −3.0 to +3.0, the chance from 0 %.
fn position(setting: Setting, burst: &Burst) -> i32 {
    let permille = match setting {
        Setting::Count => match burst.count {
            Count::Repeats(count) => (i64::from(count) - 1) * 1_000 / i64::from(MAX_REPEATS),
            Count::Gate => 1_000,
        },
        Setting::Time => {
            let octaves = |micros: u32| {
                fixed::log2(i64::from(micros) * i64::from(ONE) / i64::from(MIN_INTERVAL_MICROS))
                    .unwrap_or(0)
            };
            let full = octaves(longest_time(burst.model)).max(1);
            octaves(burst.time_micros) * 1_000 / full
        }
        Setting::Shape => {
            (i64::from(burst.curve_tenths) + i64::from(CURVE_TENTHS)) * 1_000
                / (2 * i64::from(CURVE_TENTHS))
        }
        Setting::Chance => i64::from(burst.chance_percent) * 10,
        Setting::Passthru | Setting::Sync | Setting::Mode => 0,
    };
    i32::try_from(permille.clamp(0, 1_000)).unwrap_or(0)
}

impl Default for RatchetApp {
    fn default() -> Self {
        Self::new()
    }
}

/// A trigger's length before a repeat `interval` away: 5 ms, or half the
/// interval when that is shorter, never under a millisecond.
const fn pulse_for(interval: u32) -> u32 {
    let half = interval / 2;
    let pulse = if half < PULSE_MICROS {
        half
    } else {
        PULSE_MICROS
    };
    if pulse < MIN_PULSE_MICROS {
        MIN_PULSE_MICROS
    } else {
        pulse
    }
}
