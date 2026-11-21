//! The supersaw bank in `f32`, the trial of journal/D-039: the same voices,
//! curves and parts as the integer bank, written as floating point would
//! write them, so the two can be compared on the module.
//!
//! The phases run in `[0, 1)` and wrap by subtraction; the `PolyBLEP`
//! correction multiplies by each voice's `1 / dt`, computed when the
//! parameters change, so there is still no division per sample. What the
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
    /// Where it is in its turn, in `[0, 1)`.
    phase: f32,
    /// The turn advanced per sample, below one half.
    step: f32,
    /// `1 / step`, zero at rest.
    inverse: f32,
}

impl Saw {
    const REST: Self = Self {
        phase: 0.0,
        step: 0.0,
        inverse: 0.0,
    };

    /// The band-limited value at the current phase, then advances: a
    /// sawtooth less `shape` (zero to one) of the same `lead` on (a fraction
    /// of a turn), which blends it into a pulse of that width, a square at
    /// half a turn.
    fn next(&mut self, shape: f32, lead: f32) -> f32 {
        let mut value = polyblep_saw(self.phase, self.step, self.inverse);
        if shape != 0.0 {
            let mut ahead = self.phase + lead;
            if ahead >= 1.0 {
                ahead -= 1.0;
            }
            value -= shape * polyblep_saw(ahead, self.step, self.inverse);
        }
        self.phase += self.step;
        if self.phase >= 1.0 {
            self.phase -= 1.0;
        }
        value
    }
}

/// A sawtooth at `phase`, rising from -1 to 1 over the turn, with its jump
/// at the wrap smoothed over the samples on either side; the integer bank's
/// `polyblep_saw`, its unit being one.
fn polyblep_saw(phase: f32, step: f32, inverse: f32) -> f32 {
    let naive = 2.0 * phase - 1.0;
    let residual = if phase < step {
        let rest = 1.0 - phase * inverse;
        -rest * rest
    } else if phase > 1.0 - step {
        let rest = 1.0 - (1.0 - phase) * inverse;
        rest * rest
    } else {
        0.0
    };
    naive - residual
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

/// A step of the integer bank as a fraction of a turn, and its inverse,
/// zero at rest.
fn step_and_inverse(step: u32) -> (f32, f32) {
    let turn = turns(step);
    (turn, if step == 0 { 0.0 } else { 1.0 / turn })
}

/// What the parameters decide for the `f32` bank, part of the supersaw's
/// [`super::Setup`] (journal/D-040).
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct Setup {
    steps: [f32; VOICES],
    inverses: [f32; VOICES],
    sub_step: f32,
    sub_inverse: f32,
    gains: [f32; VOICES],
    normalisers: (f32, f32),
    coefficient: f32,
    shape: f32,
    lead: f32,
    amplitude: f32,
}

impl Setup {
    /// Nothing played.
    pub(super) const OFF: Self = Self {
        steps: [0.0; VOICES],
        inverses: [0.0; VOICES],
        sub_step: 0.0,
        sub_inverse: 0.0,
        gains: [0.0; VOICES],
        normalisers: (0.0, 0.0),
        coefficient: 0.0,
        shape: 0.0,
        lead: 0.5,
        amplitude: 0.0,
    };

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
        let pairs = steps.map(step_and_inverse);
        let base = steps[CENTRE];
        let (sub_step, sub_inverse) = step_and_inverse(base / 2);
        Self {
            steps: pairs.map(|(step, _)| step),
            inverses: pairs.map(|(_, inverse)| inverse),
            sub_step,
            sub_inverse,
            gains: gains.map(|gain| fraction(gain, 15)),
            // `2^32 / (root 2^15)`: one over the root is it over `2^17`.
            normalisers: (fraction(normalisers.0, 17), fraction(normalisers.1, 17)),
            // `1 / (1 + 2 pi f / fs)`, the integer bank's coefficient.
            coefficient: 1.0 / (1.0 + core::f32::consts::TAU * turns(base)),
            shape: fraction(shape, 15),
            lead: turns(lead),
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
    /// The pulse's width, a fraction of a turn.
    lead: f32,
    /// Peak level of one sawtooth, in millivolts.
    amplitude: f32,
    /// High-passes of the mono, left and right parts.
    filters: [HighPass; 3],
}

impl Bank {
    /// A silent bank starting at `phases`, a turn being `2^32`.
    pub(super) const fn new(phases: &[u32; VOICES]) -> Self {
        let mut saws = [Saw::REST; VOICES];
        let mut index = 0;
        while index < VOICES {
            saws[index].phase = turns(phases[index]);
            index += 1;
        }
        Self {
            saws,
            sub: Saw::REST,
            gains: [0.0; VOICES],
            normalisers: (0.0, 0.0),
            coefficient: 0.0,
            shape: 0.0,
            lead: 0.5,
            amplitude: 0.0,
            filters: [HighPass::NEW; 3],
        }
    }

    /// Each voice's phase step, a turn being `2^32` as in the integer bank:
    /// the step it plays, the integer one rounded to an `f32`.
    pub(super) fn steps(&self) -> [u32; VOICES] {
        self.saws.map(|saw| from_turns(saw.step))
    }

    /// Each voice's phase now, a turn being `2^32` as in the integer bank.
    pub(super) fn phases(&self) -> [u32; VOICES] {
        self.saws.map(|saw| from_turns(saw.phase))
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
        let sub = millivolts(-self.sub.next(0.0, 0.0) * self.amplitude);
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

/// A fraction of a turn in `[0, 1)` as a phase or a step of the integer
/// bank, a turn being `2^32`: the inverse of [`turns`].
fn from_turns(turn: f32) -> u32 {
    // A power of two: the product is exact.
    let turn = turn * 4_294_967_296.0;
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    // A fraction in `[0, 1)`: the product in `[0, 2^32)`, held at the top
    // should it round up to `2^32`.
    let turn = turn as u32;
    turn
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
    use super::{from_turns, millivolts, polyblep_saw, turns};

    #[test]
    fn the_saw_is_naive_away_from_its_jump() {
        let step = turns(1 << 24);
        assert!(polyblep_saw(0.5, step, 1.0 / step).abs() < 1e-6);
        assert!((polyblep_saw(0.75, step, 1.0 / step) - 0.5).abs() < 1e-6);
    }

    #[test]
    fn the_saw_is_continuous_across_its_jump() {
        let step = turns(1 << 24);
        assert!(polyblep_saw(0.0, step, 1.0 / step).abs() < 1e-6);
        assert!(polyblep_saw(1.0 - 1e-7, step, 1.0 / step).abs() < 1e-3);
    }

    #[test]
    fn turns_go_there_and_back() {
        // A step of 24 bits or fewer survives the round trip exactly.
        for step in [0, 1 << 8, 0x00AB_CDEF << 8, 1 << 31] {
            assert_eq!(from_turns(turns(step)), step);
        }
    }

    #[test]
    fn levels_are_rounded_and_held() {
        assert_eq!(millivolts(1.4), 1);
        assert_eq!(millivolts(-1.6), -2);
        assert_eq!(millivolts(1e9), crate::calibration::CV_OUT_MAX_MV);
        assert_eq!(millivolts(-1e9), crate::calibration::CV_OUT_MIN_MV);
    }
}
