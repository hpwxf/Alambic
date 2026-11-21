//! The supersaw bank in fixed point, journal/D-037: ramps in 1.15, the
//! `PolyBLEP` correction in 16.16 through each voice's `2^48 / dt`, the
//! high-pass in 2.30 with guard bits. Everything that needs a division is
//! in [`Setup::new`], computed when the parameters change (journal/D-040).

use super::{CENTRE, Frame, LEFT, RIGHT, VOICES};
use crate::audio::scale;
use crate::audio::wave::ONE;
use crate::platform::MilliVolts;

/// `2 pi` in 16.16.
const TAU: u64 = 411_775;

/// Bits below the last place of the high-pass state: without them, a
/// one-pole filter in integers settles on a small offset instead of zero.
/// Eight keep the state times the coefficient below `2^58`.
const FILTER_BITS: u32 = 8;

/// One free-running sawtooth with what its correction needs.
#[derive(Debug, Clone, Copy)]
struct Saw {
    phase: u32,
    step: u32,
    /// `2^48 / step`: turns a phase within a step of the jump into its
    /// distance in 16.16 steps with a multiplication.
    inverse: u64,
}

impl Saw {
    const fn at(phase: u32) -> Self {
        Self {
            phase,
            step: 0,
            inverse: 0,
        }
    }

    /// The band-limited value at the current phase, then advances: a
    /// sawtooth less `shape` (1.15, zero to one) of the same `lead` on (a
    /// turn being `2^32`), which blends it into a pulse of that width, a
    /// square at half a turn.
    fn next(&mut self, shape: i32, lead: u32) -> i32 {
        let mut value = polyblep_saw(self.phase, self.step, self.inverse);
        if shape != 0 {
            let ahead = polyblep_saw(self.phase.wrapping_add(lead), self.step, self.inverse);
            value -= (shape * ahead) >> 15;
        }
        self.phase = self.phase.wrapping_add(self.step);
        value
    }
}

/// A sawtooth at `phase`, rising from `-ONE` to `ONE` over the turn, with
/// its jump at the wrap smoothed over the samples on either side.
///
/// `inverse` is `2^48 / step`. Within a step after the jump, at `x` steps,
/// the residual is `-(1 - x)^2`; within a step before it, at `x` steps to go,
/// `(1 - x)^2`; zero elsewhere.
#[must_use]
fn polyblep_saw(phase: u32, step: u32, inverse: u64) -> i32 {
    #[allow(clippy::cast_possible_wrap)]
    // The top 16 bits of a `u32`, so below `2^16`.
    let naive = (phase >> 16) as i32 - ONE;
    let residual = if phase < step {
        -square_of_rest(u64::from(phase), inverse)
    } else if phase > u32::MAX - step {
        square_of_rest((1u64 << 32) - u64::from(phase), inverse)
    } else {
        0
    };
    naive - residual
}

/// `(1 - distance / step)^2` scaled by [`ONE`], for a distance of at most a
/// step.
fn square_of_rest(distance: u64, inverse: u64) -> i32 {
    // `distance <= step`, so the product stays below `2^48` and `x <= 1`.
    let x = (distance * inverse) >> 32;
    let rest = u64::from(1u32 << 16).saturating_sub(x);
    #[allow(clippy::cast_possible_truncation)]
    // `rest <= 2^16`, so the square over `2^17` is at most `2^15`.
    let value = ((rest * rest) >> 17) as i32;
    value
}

/// A one-pole high-pass, `y = a (y + x - x_prev)`.
#[derive(Debug, Clone, Copy)]
struct HighPass {
    previous: i32,
    /// The output, [`FILTER_BITS`] below the last place.
    state: i64,
}

impl HighPass {
    /// At rest.
    const NEW: Self = Self {
        previous: 0,
        state: 0,
    };

    /// Filters `input` with the coefficient `a` in 2.30.
    fn next(&mut self, input: i32, coefficient: i64) -> i32 {
        let change = i64::from(input - self.previous) << FILTER_BITS;
        self.previous = input;
        self.state = ((self.state + change) * coefficient) >> 30;
        #[allow(clippy::cast_possible_truncation)]
        // The output of a high-pass stays within twice its input's range.
        let output = (self.state >> FILTER_BITS) as i32;
        output
    }
}

/// `2^48 / step`, zero at rest: what [`Saw::inverse`] holds.
fn inverse(step: u32) -> u64 {
    if step == 0 {
        0
    } else {
        crate::fixed::div_wide(1 << 48, step)
    }
}

/// What the parameters decide for the integer bank, part of the supersaw's
/// [`super::Setup`] (journal/D-040).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Setup {
    steps: [u32; VOICES],
    inverses: [u64; VOICES],
    sub_step: u32,
    sub_inverse: u64,
    /// Each voice's gain, in 1.15.
    gains: [i64; VOICES],
    /// `2^32` over the root of the summed squared gains, for all seven voices
    /// and for one side.
    normalisers: (i64, i64),
    /// High-pass coefficient, in 2.30.
    coefficient: i64,
    /// The voices' waveform, in 1.15: 0 a sawtooth, one a pulse.
    shape: i32,
    /// The pulse's width, the blend's second sawtooth's lead, a turn being
    /// `2^32`.
    lead: u32,
    /// Peak level of one sawtooth.
    amplitude: MilliVolts,
}

impl Setup {
    /// Nothing played.
    pub(super) const OFF: Self = Self {
        steps: [0; VOICES],
        inverses: [0; VOICES],
        sub_step: 0,
        sub_inverse: 0,
        gains: [0; VOICES],
        normalisers: (0, 0),
        coefficient: 0,
        shape: 0,
        lead: 1 << 31,
        amplitude: 0,
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
        let base = steps[CENTRE];
        Self {
            steps: *steps,
            inverses: steps.map(inverse),
            sub_step: base / 2,
            sub_inverse: inverse(base / 2),
            gains: *gains,
            normalisers,
            coefficient: high_pass_coefficient(base),
            shape: i32::try_from(shape).unwrap_or(0),
            lead,
            amplitude,
        }
    }
}

/// The running state of the integer supersaw.
#[derive(Debug, Clone)]
pub(super) struct Bank {
    saws: [Saw; VOICES],
    sub: Saw,
    gains: [i64; VOICES],
    normalisers: (i64, i64),
    coefficient: i64,
    shape: i32,
    lead: u32,
    amplitude: MilliVolts,
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
            gains: [0; VOICES],
            normalisers: (0, 0),
            coefficient: 0,
            shape: 0,
            lead: 1 << 31,
            amplitude: 0,
            filters: [HighPass::NEW; 3],
        }
    }

    /// Each voice's phase step, a turn being `2^32`.
    pub(super) fn steps(&self) -> [u32; VOICES] {
        self.saws.map(|saw| saw.step)
    }

    /// Each voice's phase now, a turn being `2^32`.
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
        // Each voice weighed by its gain: a value in 1.15 times a gain in
        // 1.15, in 2.30.
        let mut values = [0i64; VOICES];
        for ((value, saw), gain) in values.iter_mut().zip(&mut self.saws).zip(self.gains) {
            *value = i64::from(saw.next(self.shape, self.lead)) * gain;
        }
        let centre = values[CENTRE];
        let left = LEFT.iter().map(|&index| values[index]).sum::<i64>();
        let right = RIGHT.iter().map(|&index| values[index]).sum::<i64>();
        let (all, one_side) = self.normalisers;
        let sums = [
            (centre + left + right, all),
            (centre + left, one_side),
            (centre + right, one_side),
        ];
        let mut parts = [0; 3];
        for ((part, filter), (sum, normaliser)) in parts.iter_mut().zip(&mut self.filters).zip(sums)
        {
            // A sum in 2.30 times `2^32 / norm` in 1.15, back to 1.15.
            #[allow(clippy::cast_possible_truncation)]
            // At most the seven gains' sum over their root: a few `ONE`.
            let normalised = ((sum * normaliser) >> 32) as i32;
            // Falling sawtooths: the high-pass turns each jump into a
            // spike, and the outputs have 6 V of room above 0 V for 3 V
            // below, so the spikes go up.
            *part = scale(-filter.next(normalised, self.coefficient), self.amplitude);
        }
        // Falling too, to keep its phase against the voices'.
        // A sawtooth whatever the shape.
        let sub = scale(-self.sub.next(0, 0), self.amplitude);
        Frame {
            mono: parts[0],
            left: parts[1],
            right: parts[2],
            sub,
        }
    }
}

/// The coefficient of a one-pole high-pass at the pitch of `step`, in 2.30:
/// `1 / (1 + 2 pi f / fs)`.
fn high_pass_coefficient(step: u32) -> i64 {
    // `2 pi step` over a turn, in 0.32: below `2^34`, since `step < 2^31`.
    let omega = (u64::from(step) * TAU) >> 16;
    i64::try_from((1u64 << 62) / ((1u64 << 32) + omega)).unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::{Saw, polyblep_saw};
    use crate::audio::wave::ONE;

    #[test]
    fn the_saw_is_naive_away_from_its_jump() {
        let step = 1 << 24;
        let inverse = (1u64 << 48) / u64::from(step);
        assert_eq!(polyblep_saw(1 << 31, step, inverse), 0);
        assert_eq!(polyblep_saw(3 << 30, step, inverse), ONE / 2);
    }

    #[test]
    fn the_saw_is_continuous_across_its_jump() {
        // Right on the jump the residual cancels it: both sides meet at zero.
        let step = 1 << 24;
        let inverse = (1u64 << 48) / u64::from(step);
        assert_eq!(polyblep_saw(0, step, inverse), 0);
        assert!(polyblep_saw(u32::MAX, step, inverse).abs() <= 1);
        // Half a step after it: a quarter of the jump still to go.
        // The naive ramp has risen 128 by then.
        assert_eq!(polyblep_saw(step / 2, step, inverse), 128 - ONE + ONE / 4);
    }

    #[test]
    fn a_full_shape_is_a_square_of_the_same_peak() {
        // Away from its jumps, a square at minus one, then plus one.
        let step = 1 << 24;
        let inverse = (1u64 << 48) / u64::from(step);
        for (phase, level) in [
            (1u32 << 29, -ONE),
            (3 << 29, -ONE),
            (5 << 29, ONE),
            (7 << 29, ONE),
        ] {
            let mut saw = Saw {
                phase,
                step,
                inverse,
            };
            assert_eq!(saw.next(ONE, 1 << 31), level, "at {phase}");
        }
    }
}
