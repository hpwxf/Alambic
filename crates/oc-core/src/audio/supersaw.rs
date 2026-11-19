//! Seven detuned band-limited sawtooths around one pitch (journal/D-037),
//! computed in two arithmetics: fixed point (`fixed`, the bank of D-037)
//! and `f32` (`float`, the trial of journal/D-039).
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
//! This module holds what both arithmetics share: the parameters and the
//! parts, Szabo's curves, and the preparation they both start from (each
//! voice's phase step, the gains, the normalisers), in integers. Each
//! submodule turns that preparation into its own coefficients and renders
//! the samples; [`Supersaw::arithmetic`] picks which one runs.

mod fixed;
mod float;

use super::{MilliHertz, phase_step};
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

/// Each voice's starting phase, a turn being `2^32`: multiples of the golden
/// ratio of a turn, as far apart as can be, so no two voices start together.
const PHASES: [u32; VOICES] = {
    let mut phases = [0; VOICES];
    let mut index = 0;
    while index < VOICES {
        #[allow(clippy::cast_possible_truncation)]
        // `index < VOICES`.
        let turn = (index as u32).wrapping_mul(0x9E37_79B9);
        phases[index] = turn;
        index += 1;
    }
    phases
};

/// Everything a supersaw's parameters decide for both banks, computed once
/// when they change (journal/D-040). Building it touches no bank, so it can
/// run outside the audio context; [`Bank::apply`] only copies it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Setup {
    params: Supersaw,
    fixed: fixed::Setup,
    float: float::Setup,
}

impl Setup {
    /// Nothing played: what a new bank holds.
    pub const OFF: Self = Self {
        params: Supersaw::OFF,
        fixed: fixed::Setup::OFF,
        float: float::Setup::OFF,
    };

    /// What `params` decide at `rate_hz`: the preparation both banks share,
    /// then each bank's coefficients from it.
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
        let gains = (centre, side);
        let normalisers = (normaliser(centre, side, 6), normaliser(centre, side, 3));
        Self {
            params: *params,
            fixed: fixed::Setup::new(&steps, gains, normalisers, params.amplitude),
            float: float::Setup::new(&steps, gains, normalisers, params.amplitude),
        }
    }

    /// The parameters it was built from.
    #[must_use]
    pub const fn params(&self) -> &Supersaw {
        &self.params
    }
}

/// The running state of a supersaw in both arithmetics. Both follow every
/// change of parameters, only the one [`Supersaw::arithmetic`] picks runs:
/// switching resumes the other where it stopped.
#[derive(Debug, Clone)]
pub struct Bank {
    params: Supersaw,
    fixed: fixed::Bank,
    float: float::Bank,
}

impl Bank {
    /// A silent bank, its phases spread so no two voices start together.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            params: Supersaw::OFF,
            fixed: fixed::Bank::new(&PHASES),
            float: float::Bank::new(&PHASES),
        }
    }

    /// The parameters it plays.
    #[must_use]
    pub const fn params(&self) -> &Supersaw {
        &self.params
    }

    /// Each voice's phase step in the bank [`Supersaw::arithmetic`] picks, a
    /// turn being `2^32` (the `f32` bank's are the integer ones rounded to 24
    /// bits): what an analysis needs to tell a voice's harmonics from what
    /// folds back.
    #[must_use]
    pub fn steps(&self) -> [u32; VOICES] {
        match self.params.arithmetic {
            Arithmetic::Fixed => self.fixed.steps(),
            Arithmetic::Float => self.float.steps(),
        }
    }

    /// Each voice's phase now in the bank [`Supersaw::arithmetic`] picks, a
    /// turn being `2^32`: with [`Self::steps`], what a reference rendering
    /// needs to start where the bank playing does. The other bank's phases
    /// stand where it stopped.
    #[must_use]
    pub fn phases(&self) -> [u32; VOICES] {
        match self.params.arithmetic {
            Arithmetic::Fixed => self.fixed.phases(),
            Arithmetic::Float => self.float.phases(),
        }
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
        self.fixed.apply(&setup.fixed);
        self.float.apply(&setup.float);
    }

    /// One sample of every part from the bank [`Supersaw::arithmetic`]
    /// picks, then advances.
    pub fn next_frame(&mut self) -> Frame {
        match self.params.arithmetic {
            Arithmetic::Fixed => self.fixed.next_frame(),
            Arithmetic::Float => self.float.next_frame(),
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

#[cfg(test)]
mod tests {
    use super::{
        Arithmetic, Bank, CENTRE, FULL, LEFT, OFFSETS, PHASES, RIGHT, Supersaw, UNIT, VOICES,
        detune_curve, mix_gains,
    };

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
    fn the_steps_are_those_of_the_bank_playing() {
        let mut params = Supersaw {
            frequency: 440_000,
            detune: FULL,
            mix: FULL / 2,
            amplitude: 1_000,
            arithmetic: Arithmetic::Fixed,
        };
        let mut bank = Bank::new();
        bank.set(&params, 47_619);
        let fixed = bank.steps();
        params.arithmetic = Arithmetic::Float;
        bank.set(&params, 47_619);
        // The same steps, rounded to an `f32`'s 24 bits.
        for (float, fixed) in bank.steps().iter().zip(fixed) {
            assert!(float.abs_diff(fixed) <= 1 << 8, "{float} against {fixed}");
        }
    }

    #[test]
    fn the_phases_are_those_of_the_bank_playing() {
        // A thousand samples in each arithmetic: the bank playing has moved
        // a thousand steps on, the other has not moved.
        let mut params = Supersaw {
            frequency: 440_000,
            detune: FULL,
            mix: FULL / 2,
            amplitude: 1_000,
            arithmetic: Arithmetic::Float,
        };
        let mut bank = Bank::new();
        bank.set(&params, 47_619);
        let start = bank.phases();
        // The integer phases rounded to an `f32`'s 24 bits.
        for (got, want) in start.iter().zip(PHASES) {
            assert!(got.abs_diff(want) <= 1 << 8, "{got} against {want}");
        }
        for _ in 0..1_000 {
            bank.next_frame();
        }
        let moved = |from: &[u32; VOICES], steps: &[u32; VOICES]| -> [u32; VOICES] {
            core::array::from_fn(|index| from[index].wrapping_add(steps[index].wrapping_mul(1_000)))
        };
        let expected = moved(&start, &bank.steps());
        for (got, want) in bank.phases().iter().zip(expected) {
            // The `f32` phase drifts by its roundings: far below a
            // thousandth of a turn over a thousand samples.
            let apart = got.wrapping_sub(want).cast_signed().unsigned_abs();
            assert!(apart < 1 << 22, "{got} against {want}");
        }
        params.arithmetic = Arithmetic::Fixed;
        bank.set(&params, 47_619);
        assert_eq!(bank.phases(), PHASES, "the integer bank has not moved");
        for _ in 0..1_000 {
            bank.next_frame();
        }
        assert_eq!(bank.phases(), moved(&PHASES, &bank.steps()));
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
        assert!(bank.steps().iter().all(|&step| step < 1 << 31));
        for _ in 0..1_000 {
            let frame = bank.next_frame();
            assert!(frame.mono.abs() <= 7_000, "{frame:?}");
        }
    }
}
