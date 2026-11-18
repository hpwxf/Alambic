//! When the repeats of a burst fall (journal/D-032), under either of two
//! models the user compares in situation:
//!
//! * [`Model::First`]: the burst starts one *first interval* after the
//!   trigger, then the intervals change by the same factor each step, up to
//!   the first times `2^curve` for the last;
//! * [`Model::Span`]: the repeats are spread over a *span* of time, at
//!   `k / (N + 1)` of it for repeat `k` of `N`, bent by the curve. A span of
//!   one period of `TR1` fills the step exactly.
//!
//! The curve, in octaves from −3 to +3, sets both the way and how strongly:
//! 0 is even, below 0 the burst accelerates (shorter and shorter intervals,
//! the repeats crowding towards the end of a span), above 0 it slows down.
//! Each repeat fires with a chance, 0 to 100 %.

use crate::fixed::{self, ONE};
use crate::modulation::{Effect, multiplied};
use crate::platform::MilliVolts;

/// Shortest interval between two triggers, in microseconds.
pub const MIN_INTERVAL_MICROS: u32 = 2_000;

/// Longest interval between two triggers, in microseconds.
pub const MAX_INTERVAL_MICROS: u32 = 2_000_000;

/// Longest span of a burst, in microseconds.
pub const MAX_SPAN_MICROS: u32 = 8_000_000;

/// Fewest and most repeats of a counted burst.
pub const MAX_REPEATS: u8 = 32;

/// Repeats a gate burst can last at most: a gate left up does not run for ever.
pub const MAX_GATE_REPEATS: u32 = 64;

/// Repeats over which a gate burst, whose end is not known, applies its
/// shape (and, in [`Model::Span`], spreads over its span).
const GATE_REFERENCE_REPEATS: i64 = 8;

/// The curve's bound either side of 0 (even), in tenths of an octave: three
/// octaves, the last interval an eighth or eight times the first.
pub const CURVE_TENTHS: i32 = 30;

/// How the repeats are placed in time.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Model {
    /// From a first interval, changing by a constant factor.
    First,
    /// Spread over a span of time.
    Span,
}

/// How many repeats follow the trigger.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Count {
    /// This many, `1..=MAX_REPEATS`.
    Repeats(u8),
    /// As long as the trigger input stays high.
    Gate,
}

/// A burst's settings, as the panel sets them or as an input changed them at
/// a trigger.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Burst {
    /// How the repeats are placed.
    pub model: Model,
    /// How many repeats.
    pub count: Count,
    /// [`Model::First`]: the interval before the first repeat;
    /// [`Model::Span`]: the time the repeats are spread over. Microseconds.
    pub time_micros: u32,
    /// The curve, in tenths of an octave, `-CURVE_TENTHS..=CURVE_TENTHS`:
    /// below 0 accelerating, 0 even, above 0 slowing down.
    pub curve_tenths: i32,
    /// The chance each repeat fires, in percent, `0..=100`.
    pub chance_percent: u32,
}

impl Burst {
    /// How many repeats the burst asks for at most.
    #[must_use]
    pub const fn max_repeats(&self) -> u32 {
        match self.count {
            Count::Repeats(count) => count as u32,
            Count::Gate => MAX_GATE_REPEATS,
        }
    }

    /// Repeats the shape is spread over: the count, or the gate reference.
    fn shaped_repeats(&self) -> i64 {
        match self.count {
            Count::Repeats(count) => i64::from(count),
            Count::Gate => GATE_REFERENCE_REPEATS,
        }
    }

    /// The curve in octaves, 16.16, signed.
    fn curve_octaves(&self) -> i64 {
        i64::from(self.curve_tenths.clamp(-CURVE_TENTHS, CURVE_TENTHS)) * i64::from(ONE) / 10
    }

    /// Interval before repeat `index` (1 for the first), in microseconds,
    /// within [`MIN_INTERVAL_MICROS`]..=[`MAX_INTERVAL_MICROS`].
    #[must_use]
    pub fn interval_micros(&self, index: u32) -> u32 {
        let micros = match self.model {
            Model::First => self.first_model(index),
            Model::Span => self.span_model(index),
        };
        u32::try_from(micros.clamp(
            i64::from(MIN_INTERVAL_MICROS),
            i64::from(MAX_INTERVAL_MICROS),
        ))
        .unwrap_or(MAX_INTERVAL_MICROS)
    }

    /// [`Model::First`]: the first interval times `2^(curve × u)`, `u` the
    /// progress from the first repeat (0) to the last (1), past 1 for a gate
    /// burst beyond its reference.
    fn first_model(&self, index: u32) -> i64 {
        let one = i64::from(ONE);
        let steps = self.shaped_repeats() - 1;
        let progress = if steps <= 0 {
            0
        } else {
            (i64::from(index.max(1)) - 1) * one / steps
        };
        let factor = fixed::exp2(self.curve_octaves() * progress / one);
        i64::from(self.time_micros) * factor / one
    }

    /// [`Model::Span`]: the span times the distance between the bent
    /// positions of repeats `index - 1` and `index`; past the last shaped
    /// repeat (a gate burst), the last interval again.
    fn span_model(&self, index: u32) -> i64 {
        let repeats = self.shaped_repeats();
        let index = i64::from(index.max(1)).min(repeats);
        let time = i64::from(self.time_micros);
        if self.is_even() {
            // Exactly, without the 16.16 rounding of the bent positions.
            return time * index / (repeats + 1) - time * (index - 1) / (repeats + 1);
        }
        let position = |repeat: i64| {
            let x = repeat * i64::from(ONE) / (repeats + 1);
            self.bend(x)
        };
        time * (position(index) - position(index - 1)) / i64::from(ONE)
    }

    /// Whether the repeats are even: a curve of 0.
    fn is_even(&self) -> bool {
        self.curve_tenths == 0
    }

    /// Where `x` (0 to 1, in 16.16) lands once bent: even for a curve of 0;
    /// slowing down (a positive curve) with `(2^(c x) − 1) / (2^c − 1)`,
    /// `c = 2 |curve|`, which starts flat; accelerating with its mirror.
    fn bend(&self, x: i64) -> i64 {
        let one = i64::from(ONE);
        if self.is_even() {
            return x;
        }
        let curve = 2 * self.curve_octaves().abs();
        let slow = |x: i64| (fixed::exp2(curve * x / one) - one) * one / (fixed::exp2(curve) - one);
        if self.curve_tenths > 0 {
            slow(x)
        } else {
            one - slow(one - x)
        }
    }
}

/// The time an input sets when a setting follows it, at 0 V: the first
/// interval of [`Model::First`], the span of [`Model::Span`].
#[must_use]
pub const fn follow_time_micros(model: Model) -> u32 {
    match model {
        Model::First => 100_000,
        Model::Span => 400_000,
    }
}

/// The count an input sets, adds to or multiplies: four repeats per volt,
/// from one at 0 V when it is followed. A gate burst stays a gate burst
/// unless the count follows the input.
#[must_use]
pub fn count_from(base: Count, millivolts: MilliVolts, effect: Effect) -> Count {
    let added = millivolts * 4 / 1_000;
    let total = match (base, effect) {
        (_, Effect::Set) => i64::from(1 + added),
        (Count::Gate, _) => return Count::Gate,
        (Count::Repeats(count), Effect::Add) => i64::from(i32::from(count) + added),
        (Count::Repeats(count), Effect::Mul) => multiplied(i64::from(count), millivolts),
    };
    let bounded = total.clamp(1, i64::from(MAX_REPEATS));
    Count::Repeats(u8::try_from(bounded).unwrap_or(1))
}

/// The time (first interval or span) an input sets, scales 1 V/oct on the
/// rate (a volt more halves it) or multiplies; [`follow_time_micros`] at 0 V
/// when it is followed.
#[must_use]
pub fn time_from(model: Model, base_micros: u32, millivolts: MilliVolts, effect: Effect) -> u32 {
    let longest = match model {
        Model::First => MAX_INTERVAL_MICROS,
        Model::Span => MAX_SPAN_MICROS,
    };
    let time = match effect {
        Effect::Set => scale(follow_time_micros(model), -volts_q16(millivolts)),
        Effect::Add => scale(base_micros, -volts_q16(millivolts)),
        Effect::Mul => {
            let product = multiplied(i64::from(base_micros), millivolts).max(0);
            u32::try_from(product).unwrap_or(u32::MAX)
        }
    };
    time.clamp(MIN_INTERVAL_MICROS, longest)
}

/// The curve an input sets, moves (an octave per volt, from 0, even, at 0 V
/// when it is followed) or multiplies; within ±[`CURVE_TENTHS`].
#[must_use]
pub fn curve_from(base_tenths: i32, millivolts: MilliVolts, effect: Effect) -> i32 {
    let curve = match effect {
        Effect::Set => i64::from(millivolts / 100),
        Effect::Add => i64::from(base_tenths + millivolts / 100),
        Effect::Mul => multiplied(i64::from(base_tenths), millivolts),
    };
    let bounded = curve.clamp(-i64::from(CURVE_TENTHS), i64::from(CURVE_TENTHS));
    i32::try_from(bounded).unwrap_or(0)
}

/// The chance an input sets, adds to (20 % per volt, from 0 % at 0 V when it
/// is followed: 100 % at 5 V) or multiplies.
#[must_use]
pub fn chance_from(base_percent: u32, millivolts: MilliVolts, effect: Effect) -> u32 {
    let base = i64::from(base_percent);
    let chance = match effect {
        Effect::Set => i64::from(millivolts / 50),
        Effect::Add => base + i64::from(millivolts / 50),
        Effect::Mul => multiplied(base, millivolts),
    };
    u32::try_from(chance.clamp(0, 100)).unwrap_or(100)
}

/// Millivolts as volts in 16.16.
fn volts_q16(millivolts: MilliVolts) -> i64 {
    i64::from(millivolts) * i64::from(ONE) / 1_000
}

/// `value * 2^octaves`, `octaves` in 16.16.
fn scale(value: u32, octaves: i64) -> u32 {
    let scaled = i64::from(value) * fixed::exp2(octaves) / i64::from(ONE);
    u32::try_from(scaled.max(0)).unwrap_or(u32::MAX)
}

#[cfg(test)]
mod tests {
    use super::{
        Burst, Count, MAX_INTERVAL_MICROS, MIN_INTERVAL_MICROS, Model, chance_from, count_from,
        curve_from, time_from,
    };
    use crate::modulation::Effect;

    fn burst(model: Model, count: Count, curve_tenths: i32) -> Burst {
        Burst {
            model,
            count,
            time_micros: 100_000,
            curve_tenths,
            chance_percent: 100,
        }
    }

    fn intervals<const N: usize>(burst: &Burst) -> [u32; N] {
        core::array::from_fn(|index| burst.interval_micros(u32::try_from(index).unwrap() + 1))
    }

    fn close(actual: u32, expected: u32) -> bool {
        actual.abs_diff(expected) <= 1 + expected / 500
    }

    fn falling(values: &[u32]) -> bool {
        values.windows(2).all(|pair| pair[1] < pair[0])
    }

    fn rising(values: &[u32]) -> bool {
        values.windows(2).all(|pair| pair[1] > pair[0])
    }

    #[test]
    fn a_curve_of_zero_is_even_in_both_models() {
        let first = burst(Model::First, Count::Repeats(4), 0);
        assert_eq!(intervals::<4>(&first), [100_000; 4]);
        // Four repeats over 100 ms: at a fifth, two fifths... of it.
        let span = burst(Model::Span, Count::Repeats(4), 0);
        assert_eq!(intervals::<4>(&span), [20_000; 4]);
    }

    #[test]
    fn the_first_model_accelerates_below_zero_and_slows_above() {
        let fast = intervals::<4>(&burst(Model::First, Count::Repeats(4), -10));
        let slow = intervals::<4>(&burst(Model::First, Count::Repeats(4), 10));
        assert!(falling(&fast) && rising(&slow), "{fast:?} {slow:?}");
        // An octave: the last interval half, or twice, the first.
        assert_eq!((fast[0], slow[0]), (100_000, 100_000));
        assert!(
            close(fast[3], 50_000) && close(slow[3], 200_000),
            "{fast:?} {slow:?}"
        );
    }

    #[test]
    fn the_span_model_spreads_the_repeats_within_the_span() {
        let sum = |curve| {
            intervals::<4>(&burst(Model::Span, Count::Repeats(4), curve))
                .iter()
                .sum::<u32>()
        };
        // Even, the fourth of four repeats sits at 4/5 of the span; bent, it
        // moves towards the end (accelerating) or away from it (slowing).
        assert_eq!(sum(0), 80_000);
        assert!(sum(-20) > 80_000 && sum(-20) < 100_000);
        assert!(sum(20) < 80_000);
        let fast = intervals::<4>(&burst(Model::Span, Count::Repeats(4), -20));
        let slow = intervals::<4>(&burst(Model::Span, Count::Repeats(4), 20));
        assert!(falling(&fast) && rising(&slow), "{fast:?} {slow:?}");
    }

    #[test]
    fn a_gate_burst_keeps_going_past_its_reference() {
        let first = burst(Model::First, Count::Gate, -10);
        assert!(close(first.interval_micros(8), 50_000));
        assert!(first.interval_micros(16) < first.interval_micros(8));
        let span = burst(Model::Span, Count::Gate, 0);
        assert_eq!(span.interval_micros(20), span.interval_micros(8));
    }

    #[test]
    fn intervals_stay_within_bounds() {
        let fast = Burst {
            time_micros: MIN_INTERVAL_MICROS,
            ..burst(Model::First, Count::Repeats(32), -20)
        };
        assert_eq!(fast.interval_micros(32), MIN_INTERVAL_MICROS);
        let slow = Burst {
            time_micros: MAX_INTERVAL_MICROS,
            ..burst(Model::First, Count::Repeats(32), 20)
        };
        assert_eq!(slow.interval_micros(32), MAX_INTERVAL_MICROS);
    }

    #[test]
    fn an_input_added_to_a_setting_moves_it_by_its_scale() {
        assert_eq!(
            count_from(Count::Repeats(4), 1_000, Effect::Add),
            Count::Repeats(8)
        );
        assert_eq!(count_from(Count::Gate, 5_000, Effect::Add), Count::Gate);
        assert!(time_from(Model::First, 100_000, 1_000, Effect::Add).abs_diff(50_000) <= 50);
        assert_eq!(curve_from(-5, 1_000, Effect::Add), 5, "an octave per volt");
        assert_eq!(curve_from(0, 9_000, Effect::Add), 30);
        assert_eq!(chance_from(50, 1_000, Effect::Add), 70);
        assert_eq!(chance_from(50, -5_000, Effect::Add), 0);
        assert_eq!(
            count_from(Count::Repeats(4), -10_000, Effect::Add),
            Count::Repeats(1)
        );
        assert_eq!(
            time_from(Model::First, 100_000, -10_000, Effect::Add),
            MAX_INTERVAL_MICROS
        );
    }

    #[test]
    fn an_input_followed_sets_a_setting_from_zero_volts() {
        assert_eq!(
            count_from(Count::Repeats(20), 0, Effect::Set),
            Count::Repeats(1)
        );
        assert_eq!(
            count_from(Count::Gate, 1_000, Effect::Set),
            Count::Repeats(5)
        );
        assert_eq!(time_from(Model::First, 700_000, 0, Effect::Set), 100_000);
        assert_eq!(time_from(Model::Span, 700_000, 0, Effect::Set), 400_000);
        assert_eq!(curve_from(15, 0, Effect::Set), 0);
        assert_eq!(curve_from(15, -1_500, Effect::Set), -15);
        assert_eq!(chance_from(100, 2_500, Effect::Set), 50);
        assert_eq!(chance_from(0, 9_000, Effect::Set), 100);
    }

    #[test]
    fn an_input_multiplying_a_setting_leaves_it_at_five_volts() {
        assert_eq!(
            count_from(Count::Repeats(4), 5_000, Effect::Mul),
            Count::Repeats(4)
        );
        assert_eq!(
            count_from(Count::Repeats(4), 2_500, Effect::Mul),
            Count::Repeats(2)
        );
        assert_eq!(
            count_from(Count::Repeats(4), 0, Effect::Mul),
            Count::Repeats(1)
        );
        assert_eq!(count_from(Count::Gate, 2_500, Effect::Mul), Count::Gate);
        assert_eq!(
            time_from(Model::First, 100_000, 10_000, Effect::Mul),
            200_000
        );
        assert_eq!(
            time_from(Model::First, 100_000, 0, Effect::Mul),
            MIN_INTERVAL_MICROS
        );
        assert_eq!(curve_from(-10, 7_500, Effect::Mul), -15);
        assert_eq!(curve_from(20, 10_000, Effect::Mul), 30);
        assert_eq!(chance_from(80, 2_500, Effect::Mul), 40);
        assert_eq!(chance_from(80, -5_000, Effect::Mul), 0);
    }
}
