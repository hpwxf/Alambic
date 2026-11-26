//! Modulating a parameter by a CV input (journal/D-035, D-036): which
//! input, how its signal is shaped (`clamp(cv × scale + shift, min, max)`),
//! what it does to the panel's value (`SET`, `ADD`, `MUL`), and the range
//! seen lately that `CENTER` fits it to. The submenu that edits it is
//! [`editor`]; the state an applet keeps of its inputs, [`slots`].
//!
//! What a parameter means stays with its applet: the signal that sweeps it
//! end to end when it follows its input (the `sweep` this module takes), how
//! an effect applies to its value (see [`multiplied`] for `MUL`), and how it
//! shows.

pub mod editor;
pub mod slots;

use core::fmt::Write as _;

use serde::{Deserialize, Serialize};

use crate::apps::param::Refused;
use crate::fmt::TextBuf;
use crate::framebuffer::FrameBuffer;
use crate::gfx::{self, Point};
use crate::platform::{CV_CHANNELS, CvChannel, MilliVolts};
use crate::text::{self, Alignment, TextStyle};

/// What an input does to the value its parameter has on the panel
/// (journal/D-035).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Effect {
    /// The parameter follows the input, from a fixed value at 0 V.
    Set,
    /// The input is added to the panel's value.
    Add,
    /// The panel's value is multiplied by the input over [`UNIT_MV`].
    Mul,
}

/// The input that leaves a value unchanged under [`Effect::Mul`]: 5 V, the
/// top of a unipolar envelope or LFO.
pub const UNIT_MV: MilliVolts = 5_000;

/// `value * millivolts / UNIT_MV`, rounded to the nearest: [`Effect::Mul`]
/// on a value in whole units.
#[must_use]
pub fn multiplied(value: i64, millivolts: MilliVolts) -> i64 {
    let product = value * i64::from(millivolts);
    let unit = i64::from(UNIT_MV);
    (product + product.signum() * unit / 2) / unit
}

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

/// The range of the shaped signal that sweeps a parameter over all its
/// values under `effect`: `sweep` itself when the parameter follows it (from
/// the signal that gives its least value to the one that gives its most);
/// the same width centred on 0 V, so around the panel's value, when it is
/// added; 0 to 10 V (x0 to x2, x1 in the middle) when it multiplies.
#[must_use]
pub const fn usable_range(
    sweep: (MilliVolts, MilliVolts),
    effect: Effect,
) -> (MilliVolts, MilliVolts) {
    match effect {
        Effect::Set => sweep,
        Effect::Add => {
            let half = (sweep.1 - sweep.0) / 2;
            (-half, half)
        }
        Effect::Mul => (0, 2 * UNIT_MV),
    }
}

/// Radius of the mark of a setting that can take an input and has none.
const INPUT_MARK_RADIUS: i32 = 2;

/// Draws, at the right of a setting's row from `top` up to `right`, what its
/// input is (journal/D-036): its name and effect (`+CV1`), or, for a setting
/// that could take one and has none, a small ring, as a jack would show, so
/// that the settings an input can move stand out at a glance. In `style`'s
/// ink, so it reads on a highlighted row as on the others.
pub fn draw_input(
    frame: &mut FrameBuffer,
    assign: &Assign,
    right: i32,
    top: i32,
    style: TextStyle,
) {
    if assign.input.is_some() {
        let mut name = TextBuf::<8>::new();
        assign.write(&mut name);
        text::draw(
            frame,
            name.as_str(),
            Point::new(right, top),
            style.aligned(Alignment::Right),
        );
    } else {
        let centre = Point::new(right - INPUT_MARK_RADIUS - 1, top + 3);
        gfx::circle(frame, centre, INPUT_MARK_RADIUS, style.color);
    }
}

/// The inputs a parameter can take, in the order the right encoder turns them.
const INPUTS: [Option<CvChannel>; 5] = [
    None,
    Some(CvChannel::One),
    Some(CvChannel::Two),
    Some(CvChannel::Three),
    Some(CvChannel::Four),
];

/// The effects, in the order the right encoder turns them.
const EFFECTS: [Effect; 3] = [Effect::Set, Effect::Add, Effect::Mul];

/// The input a parameter takes and how its signal is shaped (journal/D-035):
/// `clamp(cv × scale + shift, min, max)`, then the effect.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
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

    /// `millivolts` as the parameter receives it: scaled, shifted, bounded.
    #[must_use]
    pub fn shape(&self, millivolts: MilliVolts) -> MilliVolts {
        let shaped =
            i64::from(millivolts) * i64::from(self.scale_percent) / 100 + i64::from(self.shift_mv);
        let bounded = shaped.clamp(i64::from(self.min_mv), i64::from(self.max_mv));
        MilliVolts::try_from(bounded).unwrap_or(0)
    }

    /// The shaped signal and its effect, `None` without an input.
    #[must_use]
    pub fn reading(&self, cv: &[MilliVolts; CV_CHANNELS]) -> Option<(MilliVolts, Effect)> {
        self.input
            .map(|channel| (self.shape(cv[channel.index()]), self.effect))
    }

    /// How the main screen names it, as an operation on the panel's value:
    /// `=CV2`, `+CV2` or `*CV2`.
    pub fn write(&self, out: &mut TextBuf<8>) {
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
    /// a factor (`x0.50`), the shift and the bounds in volts, the effect; a
    /// host writes it the same way.
    #[must_use]
    pub fn field(&self, line: AssignLine) -> TextBuf<16> {
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
    /// reading, onto `target`, the range of the shaped signal the parameter can
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

    /// `self` if the submenu could have set it: the scale, the shift and the
    /// bounds within the ranges the right encoder stops at, the lower bound
    /// not above the upper. A value set from afar (journal/D-045) behaves as
    /// one turned by hand, as [`ParamSpec::check`](crate::apps::param::ParamSpec::check)
    /// has it for a parameter; it need not fall on a detent.
    ///
    /// # Errors
    ///
    /// [`Refused::OutOfRange`] for a value outside its range, or bounds the
    /// wrong way round.
    pub const fn check(self) -> Result<Self, Refused> {
        let in_range = self.scale_percent.abs() <= SCALE_RANGE_PERCENT
            && self.shift_mv.abs() <= ASSIGN_RANGE_MV
            && -ASSIGN_RANGE_MV <= self.min_mv
            && self.min_mv <= self.max_mv
            && self.max_mv <= ASSIGN_RANGE_MV;
        if in_range {
            Ok(self)
        } else {
            Err(Refused::OutOfRange)
        }
    }

    /// Whether the parameter follows its input, the panel's value unused.
    #[must_use]
    pub const fn sets(&self) -> bool {
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
    /// shift that carry the readings seen onto the parameter's usable range.
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
