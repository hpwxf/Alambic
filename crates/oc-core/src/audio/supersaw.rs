//! Seven detuned band-limited sawtooths around one pitch, in integers
//! (journal/D-037).
//!
//! The detune curve, the spread of the seven voices and the two mix laws are
//! Adam Szabo's measurements of the JP-8000 (*How to Emulate the Super Saw*,
//! 2010). Each sawtooth is corrected around its jump by a two-sample
//! polynomial (`PolyBLEP`), which removes most of the aliasing a naive ramp
//! folds back into the audible band; the sum goes through a one-pole
//! high-pass tuned on the fundamental. Everything that needs a division is
//! computed when the parameters change, never per sample, by [`Setup::new`],
//! which a backend may run outside its audio context (journal/D-040).
//!
//! The parts come out as falling sawtooths: the high-pass turns each jump
//! into a spike several times higher than the rest of the wave, and the
//! outputs have 6 V of room above 0 V for 3 V below, so the spikes go up;
//! that doubles the level reached without clipping, and the ear does not
//! hear a waveform's polarity (journal/D-037, revision of 2026-11-18).
//!
//! A second bank, in `f32` ([`Arithmetic::Float`], journal/D-039), plays the
//! same supersaw for comparison; [`Supersaw::arithmetic`] picks which one
//! runs.

mod float;

use super::wave::ONE;
use super::{MilliHertz, phase_step, scale};
use crate::platform::MilliVolts;

/// Sawtooths in the bank.
pub const VOICES: usize = 7;

/// The voice on the fundamental; the others spread below and above it.
pub const CENTRE: usize = 3;

/// The whole course of [`Supersaw::detune`] and [`Supersaw::mix`].
pub const FULL: u16 = 1_000;

/// One in 16.16 fixed point.
const UNIT: i64 = 1 << 16;

/// Each voice's relative offset from the fundamental at full detune, in
/// 16.16: Szabo's -0.110 to +0.107.
pub const OFFSETS: [i64; VOICES] = [-7_210, -4_121, -1_279, 0, 1_305, 4_074, 7_042];

/// The voices of each side in stereo, besides the centre: a wide, a middle
/// and a narrow one each, of mixed signs.
const LEFT: [usize; 3] = [0, 2, 5];
/// See [`LEFT`].
const RIGHT: [usize; 3] = [1, 4, 6];

/// Szabo's detune curve, a polynomial of degree 11 on the knob, at 65 points
/// over its course, in 16.16; checked against the polynomial in the tests.
const DETUNE_CURVE: [i64; 65] = [
    197, 586, 649, 641, 680, 800, 995, 1_237, //
    1_499, 1_756, 1_994, 2_207, 2_397, 2_570, 2_735, 2_901, //
    3_075, 3_262, 3_464, 3_681, 3_907, 4_139, 4_370, 4_595, //
    4_810, 5_014, 5_206, 5_389, 5_569, 5_753, 5_950, 6_169, //
    6_418, 6_706, 7_038, 7_419, 7_850, 8_330, 8_857, 9_425, //
    10_029, 10_666, 11_329, 12_018, 12_731, 13_472, 14_242, 15_046, //
    15_888, 16_766, 17_673, 18_596, 19_510, 20_383, 21_179, 21_864, //
    22_427, 22_901, 23_411, 24_223, 25_835, 29_080, 35_283, 46_450, //
    65_523,
];

/// `2 pi` in 16.16.
const TAU: u64 = 411_775;

/// Bits below the last place of the high-pass state: without them, a
/// one-pole filter in integers settles on a small offset instead of zero.
/// Eight keep the state times the coefficient below `2^58`.
const FILTER_BITS: u32 = 8;

/// How the bank computes its samples (journal/D-039).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub enum Arithmetic {
    /// Fixed point, the bank of journal/D-037.
    #[default]
    Fixed,
    /// Single-precision floating point, the trial of journal/D-039.
    Float,
}

/// What a supersaw plays, as an applet publishes it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Supersaw {
    /// The fundamental, the centre voice's pitch.
    pub frequency: MilliHertz,
    /// How far the side voices spread, in thousandths of the full course.
    pub detune: u16,
    /// How loud the side voices are against the centre, in thousandths.
    pub mix: u16,
    /// Peak level of one sawtooth; the sum is scaled to sound about as loud.
    pub amplitude: MilliVolts,
    /// Which of the two banks plays it.
    pub arithmetic: Arithmetic,
}

impl Supersaw {
    /// Nothing played.
    pub const OFF: Self = Self {
        frequency: 0,
        detune: 0,
        mix: 0,
        amplitude: 0,
        arithmetic: Arithmetic::Fixed,
    };
}

/// Which part of the bank an output carries.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Part {
    /// All seven voices.
    Mono,
    /// The centre and the left voices.
    Left,
    /// The centre and the right voices.
    Right,
    /// One sawtooth an octave below the fundamental, without the high-pass.
    Sub,
}

/// One sample of every part, in millivolts.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Frame {
    /// [`Part::Mono`].
    pub mono: MilliVolts,
    /// [`Part::Left`].
    pub left: MilliVolts,
    /// [`Part::Right`].
    pub right: MilliVolts,
    /// [`Part::Sub`].
    pub sub: MilliVolts,
}

impl Frame {
    /// The sample of `part`.
    #[must_use]
    pub const fn part(&self, part: Part) -> MilliVolts {
        match part {
            Part::Mono => self.mono,
            Part::Left => self.left,
            Part::Right => self.right,
            Part::Sub => self.sub,
        }
    }
}

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

    /// The band-limited value at the current phase, in `-ONE..=ONE`, then
    /// advances.
    fn next(&mut self) -> i32 {
        let value = polyblep_saw(self.phase, self.step, self.inverse);
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
        (1u64 << 48) / u64::from(step)
    }
}

/// Everything a supersaw's parameters decide for both banks, computed once
/// when they change: the steps and their inverses, the gains, the
/// normalisers and the high-pass coefficient (journal/D-040). Building it
/// touches no bank, so it can run outside the audio context;
/// [`Bank::apply`] only copies it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Setup {
    params: Supersaw,
    steps: [u32; VOICES],
    inverses: [u64; VOICES],
    sub_step: u32,
    sub_inverse: u64,
    gains: (i64, i64),
    normalisers: (i64, i64),
    coefficient: i64,
    float: float::Setup,
}

impl Setup {
    /// Nothing played: what a new bank holds.
    pub const OFF: Self = Self {
        params: Supersaw::OFF,
        steps: [0; VOICES],
        inverses: [0; VOICES],
        sub_step: 0,
        sub_inverse: 0,
        gains: (0, 0),
        normalisers: (0, 0),
        coefficient: 0,
        float: float::Setup::OFF,
    };

    /// What `params` decide at `rate_hz`: every division a bank needs.
    #[must_use]
    pub fn new(params: &Supersaw, rate_hz: u32) -> Self {
        let base = phase_step(params.frequency, rate_hz);
        let spread = detune_curve(params.detune);
        let steps = OFFSETS.map(|offset| {
            let shift = (i64::from(base) * offset * spread) >> 32;
            let step = (i64::from(base) + shift).clamp(0, i64::from(i32::MAX - 1));
            u32::try_from(step).unwrap_or(0)
        });
        let (centre, side) = mix_gains(params.mix);
        let normalisers = (normaliser(centre, side, 6), normaliser(centre, side, 3));
        Self {
            params: *params,
            steps,
            inverses: steps.map(inverse),
            sub_step: base / 2,
            sub_inverse: inverse(base / 2),
            gains: (centre, side),
            normalisers,
            coefficient: high_pass_coefficient(base),
            float: float::Setup::new(&steps, (centre, side), normalisers, params.amplitude),
        }
    }

    /// The parameters it was built from.
    #[must_use]
    pub const fn params(&self) -> &Supersaw {
        &self.params
    }
}

/// The running state of a supersaw: seven sawtooths, a sub-octave, the
/// high-passes and the coefficients of the current parameters, in integers,
/// and the same in `f32` (journal/D-039). Both follow every change of
/// parameters, only the one [`Supersaw::arithmetic`] picks runs: switching
/// resumes the other where it stopped.
#[derive(Debug, Clone)]
pub struct Bank {
    params: Supersaw,
    saws: [Saw; VOICES],
    sub: Saw,
    /// Centre and side gains, in 1.15.
    gains: (i64, i64),
    /// `2^32` over the root of the summed squared gains, for all seven voices
    /// and for one side.
    normalisers: (i64, i64),
    /// High-pass coefficient, in 2.30.
    coefficient: i64,
    /// High-passes of [`Part::Mono`], [`Part::Left`], [`Part::Right`].
    filters: [HighPass; 3],
    /// The `f32` bank.
    float: float::Bank,
}

impl Bank {
    /// A silent bank, its phases spread so no two voices start together.
    #[must_use]
    pub const fn new() -> Self {
        // Multiples of the golden ratio of a turn: as far apart as can be.
        let mut saws = [Saw::at(0); VOICES];
        let mut index = 0;
        while index < VOICES {
            #[allow(clippy::cast_possible_truncation)]
            // `index < VOICES`.
            let turn = (index as u32).wrapping_mul(0x9E37_79B9);
            saws[index] = Saw::at(turn);
            index += 1;
        }
        // Silent until set: no gain, no level.
        Self {
            params: Supersaw::OFF,
            saws,
            sub: Saw::at(0),
            gains: (0, 0),
            normalisers: (0, 0),
            coefficient: 0,
            filters: [HighPass::NEW; 3],
            float: float::Bank::new(&saws),
        }
    }

    /// The parameters it plays.
    #[must_use]
    pub const fn params(&self) -> &Supersaw {
        &self.params
    }

    /// Each voice's phase step, a turn being `2^32` (the `f32` bank's are
    /// the same, converted): what an analysis needs
    /// to tell a voice's harmonics from what folds back.
    #[must_use]
    pub fn steps(&self) -> [u32; VOICES] {
        self.saws.map(|saw| saw.step)
    }

    /// Each voice's phase now in the integer bank, a turn being `2^32`: with
    /// [`Self::steps`],
    /// what a reference rendering needs to start where the bank does.
    #[must_use]
    pub fn phases(&self) -> [u32; VOICES] {
        self.saws.map(|saw| saw.phase)
    }

    /// Plays `params` at `rate_hz` from the next sample on, keeping the
    /// phases and the filters' state.
    pub fn set(&mut self, params: &Supersaw, rate_hz: u32) {
        self.apply(&Setup::new(params, rate_hz));
    }

    /// Plays what `setup` prepared from the next sample on, keeping the
    /// phases and the filters' state: copies only, no division.
    pub fn apply(&mut self, setup: &Setup) {
        self.params = setup.params;
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
        self.float.apply(&setup.float);
    }

    /// One sample of every part from the bank [`Supersaw::arithmetic`]
    /// picks, then advances.
    pub fn next_frame(&mut self) -> Frame {
        match self.params.arithmetic {
            Arithmetic::Fixed => self.next_fixed_frame(),
            Arithmetic::Float => self.float.next_frame(),
        }
    }

    /// One sample of every part from the integer bank, then advances.
    fn next_fixed_frame(&mut self) -> Frame {
        let mut values = [0i64; VOICES];
        for (value, saw) in values.iter_mut().zip(&mut self.saws) {
            *value = i64::from(saw.next());
        }
        let (centre_gain, side_gain) = self.gains;
        let centre = values[CENTRE] * centre_gain;
        let left = LEFT.iter().map(|&index| values[index]).sum::<i64>() * side_gain;
        let right = RIGHT.iter().map(|&index| values[index]).sum::<i64>() * side_gain;
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
            *part = scale(
                -filter.next(normalised, self.coefficient),
                self.params.amplitude,
            );
        }
        // Falling too, to keep its phase against the voices'.
        let sub = scale(-self.sub.next(), self.params.amplitude);
        Frame {
            mono: parts[0],
            left: parts[1],
            right: parts[2],
            sub,
        }
    }
}

impl Default for Bank {
    fn default() -> Self {
        Self::new()
    }
}

/// `amount` in thousandths of [`FULL`] as a 16.16 fraction, at most one.
fn fraction(amount: u16) -> i64 {
    i64::from(amount.min(FULL)) * UNIT / i64::from(FULL)
}

/// Szabo's detune curve at `detune`, in 16.16: about 0.003 at rest, one at
/// full course.
#[must_use]
pub fn detune_curve(detune: u16) -> i64 {
    let position = fraction(detune) * 64;
    // The last point ends the last step: the top of the course is 63 and all.
    let step = (position >> 16).min(63);
    let within = position - (step << 16);
    let index = usize::try_from(step).unwrap_or(0);
    let low = DETUNE_CURVE[index];
    let high = DETUNE_CURVE[index + 1];
    low + (((high - low) * within) >> 16)
}

/// Szabo's two mix laws at `mix`, in 1.15: the centre fades from about one
/// to 0.44 while the sides rise from 0.04 to 0.59.
#[must_use]
pub fn mix_gains(mix: u16) -> (i64, i64) {
    let x = fraction(mix);
    // centre = 0.99785 - 0.55366 x
    let centre = 65_395 - ((36_285 * x) >> 16);
    // side = -0.73764 x^2 + 1.2841 x + 0.044372
    let side = ((-48_342 * ((x * x) >> 16)) >> 16) + ((84_155 * x) >> 16) + 2_908;
    // From 16.16 to 1.15.
    (centre >> 1, side >> 1)
}

/// `2^32 / sqrt(centre^2 + sides * side^2)`, the gains in 1.15: dividing the
/// sum by it keeps the loudness about the same whatever the mix.
fn normaliser(centre: i64, side: i64, sides: i64) -> i64 {
    let power = u64::try_from(centre * centre + sides * side * side).unwrap_or(0);
    // The centre gain stays above 0.44, so the root is far from zero.
    let root = i64::try_from(power.isqrt()).unwrap_or(1).max(1);
    (1 << 32) / root
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
    use super::{
        Bank, CENTRE, FULL, LEFT, OFFSETS, RIGHT, Supersaw, UNIT, detune_curve, mix_gains,
        polyblep_saw,
    };
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
    fn the_curve_starts_near_zero_and_ends_at_one() {
        assert_eq!(detune_curve(0), 197);
        assert!((detune_curve(FULL) - UNIT).abs() < 32);
        assert_eq!(detune_curve(FULL + 1), detune_curve(FULL));
    }

    #[test]
    fn the_mix_laws_cross_where_szabo_has_them() {
        let (centre, side) = mix_gains(0);
        assert!((centre - 32_697).abs() < 2 && (side - 1_454).abs() < 2);
        let (centre, side) = mix_gains(FULL);
        assert!(centre < side, "{centre} {side}");
    }

    #[test]
    fn the_sides_are_split_between_left_and_right() {
        let mut seen = [false; OFFSETS.len()];
        for index in LEFT.iter().chain(&RIGHT).chain(&[CENTRE]) {
            assert!(!seen[*index], "voice {index} twice");
            seen[*index] = true;
        }
        assert!(seen.iter().all(|&voice| voice));
    }

    #[test]
    fn silence_stays_silent() {
        let mut bank = Bank::new();
        for _ in 0..100 {
            assert_eq!(bank.next_frame(), super::Frame::default());
        }
    }

    #[test]
    fn full_detune_keeps_every_voice_below_half_a_turn() {
        let mut bank = Bank::new();
        let params = Supersaw {
            frequency: 20_000_000,
            detune: FULL,
            mix: FULL,
            amplitude: 3_000,
            arithmetic: super::Arithmetic::Fixed,
        };
        bank.set(&params, 47_619);
        assert!(bank.saws.iter().all(|saw| saw.step < 1 << 31));
        for _ in 0..1_000 {
            let frame = bank.next_frame();
            assert!(frame.mono.abs() <= 7_000, "{frame:?}");
        }
    }
}
