//! The supersaw bank in `f32`, the trial of journal/D-039: the same voices,
//! curves and parts as the integer bank, written as floating point would
//! write them, so the two can be compared on the module.
//!
//! The phases run as the integer bank's, a turn being `2^32`, and wrap for
//! free: in `f32` a phase near the end of its turn keeps 24 bits, and its
//! sum drifts off pitch at low notes (journal/D-039, F2). Each sample
//! reads it as a fraction of a turn; the `PolyBLEP` correction multiplies
//! by each voice's `1 / dt`, computed when the parameters change, so there
//! is still no division per sample. What the
//! two banks share as a table or a root (the detune curve, the mix laws,
//! the normalisers, prepared in integers by the parent module) is converted: `core` has no
//! `sqrt` without `std`, and Szabo's polynomial loses all its precision in
//! `f32`. Only the samples leave as millivolts, rounded and held within the
//! outputs.

use super::{CENTRE, Frame, LEFT, RIGHT, VOICES};
use crate::calibration::{CV_OUT_MAX_MV, CV_OUT_MIN_MV};
use crate::platform::MilliVolts;

/// One free-running sawtooth with what its correction needs.
#[derive(Debug, Clone, Copy)]
struct Saw {
    /// Where it is in its turn, a turn being `2^32`.
    phase: u32,
    /// The phase advanced per sample, below half a turn.
    step: u32,
    /// One over the step as a fraction of a turn, zero at rest.
    inverse: f32,
}

impl Saw {
    const fn at(phase: u32) -> Self {
        Self {
            phase,
            step: 0,
            inverse: 0.0,
        }
    }

    /// The band-limited value at the current phase, then advances: a
    /// sawtooth less `shape` (zero to one) of the same `lead` on (a turn
    /// being `2^32`), which blends it into a pulse of that width, a square
    /// at half a turn.
    fn next(&mut self, shape: f32, lead: u32) -> f32 {
        let mut value = polyblep_saw(self.phase, self.step, self.inverse);
        if below(0.0, shape) {
            let ahead = self.phase.wrapping_add(lead);
            value -= shape * polyblep_saw(ahead, self.step, self.inverse);
        }
        self.phase = self.phase.wrapping_add(self.step);
        value
    }
}

/// A sawtooth at `phase` (a turn being `2^32`), rising from -1 to 1 over the
/// turn, with its jump at the wrap smoothed over the samples on either
/// side; the integer bank's `polyblep_saw`, its unit being one. Where it is
/// against the jump is decided in integers, and the distance to it is taken
/// there too, exactly, before it becomes a fraction.
fn polyblep_saw(phase: u32, step: u32, inverse: f32) -> f32 {
    let naive = 2.0 * turns(phase) - 1.0;
    let residual = if phase < step {
        let rest = 1.0 - turns(phase) * inverse;
        -rest * rest
    } else if phase > u32::MAX - step {
        let rest = 1.0 - turns(phase.wrapping_neg()) * inverse;
        rest * rest
    } else {
        0.0
    };
    naive - residual
}

/// Whether `low < high`, both at least zero: the order of their bits is
/// theirs, so an integer comparison gives it. A floating-point one costs
/// the core a transfer of the FPU's flags and a stall before the branch,
/// once per voice and per sample here (journal/D-039).
fn below(low: f32, high: f32) -> bool {
    low.to_bits() < high.to_bits()
}

/// A one-pole high-pass, `y = a (y + x - x_prev)`.
#[derive(Debug, Clone, Copy)]
struct HighPass {
    previous: f32,
    state: f32,
}

impl HighPass {
    /// At rest.
    const NEW: Self = Self {
        previous: 0.0,
        state: 0.0,
    };

    fn next(&mut self, input: f32, coefficient: f32) -> f32 {
        self.state = coefficient * (self.state + input - self.previous);
        self.previous = input;
        self.state
    }
}

/// One over a step of the integer bank as a fraction of a turn, zero at
/// rest.
fn inverse(step: u32) -> f32 {
    if step == 0 { 0.0 } else { 1.0 / turns(step) }
}

/// What the parameters decide for the `f32` bank, part of the supersaw's
/// [`super::Setup`] (journal/D-040).
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct Setup {
    steps: [u32; VOICES],
    inverses: [f32; VOICES],
    sub_step: u32,
    sub_inverse: f32,
    gains: [f32; VOICES],
    normalisers: (f32, f32),
    coefficient: f32,
    shape: f32,
    lead: u32,
    amplitude: f32,
}

impl Setup {
    /// From the preparation both banks share: the voices' steps, their gains
    /// in 1.15, the normalisers `2^32 / root` of gains in 1.15, and the
    /// blend: the shape in 1.15 and the lead a turn being `2^32`.
    pub(super) fn new(
        steps: &[u32; VOICES],
        gains: &[i64; VOICES],
        normalisers: (i64, i64),
        (shape, lead): (i64, u32),
        amplitude: MilliVolts,
    ) -> Self {
        let base = steps[CENTRE];
        Self {
            steps: *steps,
            inverses: steps.map(inverse),
            sub_step: base / 2,
            sub_inverse: inverse(base / 2),
            gains: gains.map(|gain| fraction(gain, 15)),
            // `2^32 / (root 2^15)`: one over the root is it over `2^17`.
            normalisers: (fraction(normalisers.0, 17), fraction(normalisers.1, 17)),
            // `1 / (1 + 2 pi f / fs)`, the integer bank's coefficient.
            coefficient: 1.0 / (1.0 + core::f32::consts::TAU * turns(base)),
            shape: fraction(shape, 15),
            lead,
            amplitude: fraction(i64::from(amplitude), 0),
        }
    }
}

/// The running state of the `f32` supersaw.
#[derive(Debug, Clone)]
pub(super) struct Bank {
    saws: [Saw; VOICES],
    sub: Saw,
    /// Each voice's gain.
    gains: [f32; VOICES],
    /// One over the root of the summed squared gains, for all seven voices
    /// and for one side.
    normalisers: (f32, f32),
    /// High-pass coefficient.
    coefficient: f32,
    /// The voices' waveform: 0 a sawtooth, one a pulse.
    shape: f32,
    /// The second sawtooth's lead, a turn being `2^32`.
    lead: u32,
    /// Peak level of one sawtooth, in millivolts.
    amplitude: f32,
    /// High-passes of the mono, left and right parts.
    filters: [HighPass; 3],
}

impl Bank {
    /// A silent bank starting at `phases`, a turn being `2^32`.
    pub(super) const fn new(phases: &[u32; VOICES]) -> Self {
        let mut saws = [Saw::at(0); VOICES];
        let mut index = 0;
        while index < VOICES {
            saws[index] = Saw::at(phases[index]);
            index += 1;
        }
        Self {
            saws,
            sub: Saw::at(0),
            gains: [0.0; VOICES],
            normalisers: (0.0, 0.0),
            coefficient: 0.0,
            shape: 0.0,
            lead: 1 << 31,
            amplitude: 0.0,
            filters: [HighPass::NEW; 3],
        }
    }

    /// Each voice's phase step, a turn being `2^32` as in the integer bank.
    pub(super) fn steps(&self) -> [u32; VOICES] {
        self.saws.map(|saw| saw.step)
    }

    /// Each voice's phase now, a turn being `2^32` as in the integer bank.
    pub(super) fn phases(&self) -> [u32; VOICES] {
        self.saws.map(|saw| saw.phase)
    }

    /// Plays what `setup` prepared from the next sample on, keeping the
    /// phases and the filters' state.
    pub(super) fn apply(&mut self, setup: &Setup) {
        for ((saw, &step), &inverse) in self.saws.iter_mut().zip(&setup.steps).zip(&setup.inverses)
        {
            saw.step = step;
            saw.inverse = inverse;
        }
        self.sub.step = setup.sub_step;
        self.sub.inverse = setup.sub_inverse;
        self.gains = setup.gains;
        self.normalisers = setup.normalisers;
        self.coefficient = setup.coefficient;
        self.shape = setup.shape;
        self.lead = setup.lead;
        self.amplitude = setup.amplitude;
    }

    /// One sample of every part, then advances.
    #[inline]
    pub(super) fn next_frame(&mut self) -> Frame {
        // Each voice weighed by its gain.
        let mut values = [0.0f32; VOICES];
        for ((value, saw), gain) in values.iter_mut().zip(&mut self.saws).zip(self.gains) {
            *value = saw.next(self.shape, self.lead) * gain;
        }
        let centre = values[CENTRE];
        let left = LEFT.iter().map(|&index| values[index]).sum::<f32>();
        let right = RIGHT.iter().map(|&index| values[index]).sum::<f32>();
        let (all, one_side) = self.normalisers;
        let sums = [
            (centre + left + right) * all,
            (centre + left) * one_side,
            (centre + right) * one_side,
        ];
        let mut parts = [0; 3];
        for ((part, filter), sum) in parts.iter_mut().zip(&mut self.filters).zip(sums) {
            // Falling sawtooths, as in the integer bank.
            *part = millivolts(-filter.next(sum, self.coefficient) * self.amplitude);
        }
        // A sawtooth whatever the shape.
        let sub = millivolts(-self.sub.next(0.0, 0) * self.amplitude);
        Frame {
            mono: parts[0],
            left: parts[1],
            right: parts[2],
            sub,
        }
    }
}

/// A phase or a step of the integer bank, a turn being `2^32`, as a fraction
/// of a turn.
const fn turns(phase: u32) -> f32 {
    #[allow(clippy::cast_precision_loss)]
    // A fraction of a turn needs 24 bits, not 32.
    let phase = phase as f32;
    phase / 4_294_967_296.0
}

/// `value` with `bits` fractional bits.
fn fraction(value: i64, bits: u32) -> f32 {
    #[allow(clippy::cast_precision_loss)]
    // Gains, normalisers and levels, all far below `2^24` in their unit.
    let value = value as f32;
    #[allow(clippy::cast_precision_loss)]
    // A power of two, exact in `f32`.
    let unit = (1u64 << bits) as f32;
    value / unit
}

/// `level` rounded to the millivolt and held within what the outputs can
/// produce.
fn millivolts(level: f32) -> MilliVolts {
    #[allow(clippy::cast_precision_loss)]
    // The output range, a few thousand millivolts.
    let (low, high) = (CV_OUT_MIN_MV as f32, CV_OUT_MAX_MV as f32);
    let held = level.clamp(low, high);
    let rounded = if held < 0.0 { held - 0.5 } else { held + 0.5 };
    #[allow(clippy::cast_possible_truncation)]
    // Held within the output range, a few thousand millivolts.
    let rounded = rounded as MilliVolts;
    rounded
}

#[cfg(test)]
mod tests {
    use super::{below, millivolts, polyblep_saw};

    #[test]
    fn below_orders_non_negative_values_as_the_comparison_does() {
        let values = [
            0.0,
            f32::from_bits(1),
            f32::MIN_POSITIVE,
            1e-7,
            0.25,
            0.5 - f32::EPSILON,
            0.5,
            1.0 - f32::EPSILON / 2.0,
            1.0,
            1.0 + f32::EPSILON,
            2.0,
        ];
        for low in values {
            for high in values {
                assert_eq!(below(low, high), low < high, "{low} < {high}");
            }
        }
    }

    /// `1 / step` of a step of `2^24`, a 256th of a turn.
    const INVERSE: f32 = 256.0;

    #[test]
    fn the_saw_is_naive_away_from_its_jump() {
        assert!(polyblep_saw(1 << 31, 1 << 24, INVERSE).abs() < 1e-6);
        assert!((polyblep_saw(3 << 30, 1 << 24, INVERSE) - 0.5).abs() < 1e-6);
    }

    #[test]
    fn the_saw_is_continuous_across_its_jump() {
        assert!(polyblep_saw(0, 1 << 24, INVERSE).abs() < 1e-6);
        assert!(polyblep_saw(u32::MAX, 1 << 24, INVERSE).abs() < 1e-3);
    }

    #[test]
    fn levels_are_rounded_and_held() {
        assert_eq!(millivolts(1.4), 1);
        assert_eq!(millivolts(-1.6), -2);
        assert_eq!(millivolts(1e9), crate::calibration::CV_OUT_MAX_MV);
        assert_eq!(millivolts(-1e9), crate::calibration::CV_OUT_MIN_MV);
    }
}
