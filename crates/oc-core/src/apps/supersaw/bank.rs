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
//! voice's phase step, each voice's gain, the normalisers), in integers. Each
//! submodule turns that preparation into its own coefficients and renders
//! the samples; [`Supersaw::arithmetic`] picks which one runs.
//!
//! The bank is `SUPERSAW`'s own (journal/D-044): it lives with the applet,
//! and reaches the outputs as the [`SupersawBank`] generator. Should another
//! applet need it, it moves to a shared DSP library rather than back into
//! `audio`.

mod fixed;
mod float;

use crate::audio::{Generator, MilliHertz, phase_step};
use crate::fixed::div_wide;
use crate::platform::{CV_CHANNELS, MilliVolts};

/// Sawtooths in the bank.
pub const VOICES: usize = 7;

/// The voice on the fundamental; the others spread below and above it.
pub const CENTRE: usize = 3;

/// The whole course of [`Supersaw::detune`] and [`Supersaw::mix`].
pub const FULL: u16 = 1_000;

/// [`Supersaw::width`] of a square, half the period: the widest.
pub const SQUARE_WIDTH: u16 = FULL / 2;

/// The narrowest [`Supersaw::width`], 5 % of the period: narrower, a pulse
/// has so little power that keeping its loudness would raise the rounding
/// with it. A width and its complement sound the same, inverted, so half
/// the period is the other end.
pub const NARROWEST_WIDTH: u16 = 50;

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
    /// Each voice's waveform, in thousandths: a sawtooth at 0, a square at
    /// [`FULL`], their blend between (journal/D-037, revision of
    /// 2026-11-19); the loudness stays about the same over the course.
    pub shape: u16,
    /// The pulse's width at a full [`Self::shape`], in thousandths of the
    /// period, from [`NARROWEST_WIDTH`] to [`SQUARE_WIDTH`], the square
    /// (journal/D-037, revision of 2026-11-19): the blend's second sawtooth
    /// is that far back.
    pub width: u16,
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
        shape: 0,
        width: SQUARE_WIDTH,
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

/// Everything a supersaw's parameters decide for the bank they pick,
/// computed once when they change (journal/D-040). Building it touches no
/// bank, so it can run outside the audio context; [`Bank::apply`] only copies
/// it. The other bank's coefficients are not prepared: nothing renders them,
/// and switching [`Supersaw::arithmetic`] is itself a change of parameters.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Setup {
    params: Supersaw,
    coefficients: Coefficients,
}

/// The coefficients of the bank [`Supersaw::arithmetic`] picks.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Coefficients {
    Fixed(fixed::Setup),
    Float(float::Setup),
}

impl Setup {
    /// Nothing played: what a new bank holds.
    pub const OFF: Self = Self {
        params: Supersaw::OFF,
        coefficients: Coefficients::Fixed(fixed::Setup::OFF),
    };

    /// What `params` decide at `rate_hz`: the preparation both banks share,
    /// then the picked bank's coefficients from it.
    #[must_use]
    pub fn new(params: &Supersaw, rate_hz: u32) -> Self {
        let base = phase_step(params.frequency, rate_hz);
        let spread = detune_curve(params.detune);
        let steps = OFFSETS.map(|offset| {
            let shift = (i64::from(base) * offset * spread) >> 32;
            let step = (i64::from(base) + shift).clamp(0, i64::from(i32::MAX - 1));
            u32::try_from(step).unwrap_or(0)
        });
        let gains = voice_gains(params.mix);
        let one_side = gains[CENTRE] * gains[CENTRE]
            + LEFT
                .iter()
                .map(|&index| gains[index] * gains[index])
                .sum::<i64>();
        let all = gains.iter().map(|gain| gain * gain).sum::<i64>();
        let width = params.width.clamp(NARROWEST_WIDTH, SQUARE_WIDTH);
        let loudness = shape_loudness(params.shape, width);
        let normalisers = (
            louder(normaliser(all), loudness),
            louder(normaliser(one_side), loudness),
        );
        // In 1.15, from 0 (a sawtooth) to one (a square).
        let shape = fraction(params.shape) >> 1;
        // The second sawtooth's lead, a turn being `2^32`: the width back,
        // its complement on. The same pulse, but its narrow part comes out
        // upwards, where the outputs have 6 V of room instead of 3.
        let lead =
            u32::try_from(div_wide(u64::from(FULL - width) << 32, u32::from(FULL))).unwrap_or(0);
        let blend = (shape, lead);
        let coefficients = match params.arithmetic {
            Arithmetic::Fixed => Coefficients::Fixed(fixed::Setup::new(
                &steps,
                &gains,
                normalisers,
                blend,
                params.amplitude,
            )),
            Arithmetic::Float => Coefficients::Float(float::Setup::new(
                &steps,
                &gains,
                normalisers,
                blend,
                params.amplitude,
            )),
        };
        Self {
            params: *params,
            coefficients,
        }
    }

    /// The parameters it was built from.
    #[must_use]
    pub const fn params(&self) -> &Supersaw {
        &self.params
    }
}

/// The running state of a supersaw in both arithmetics. Only the one
/// [`Supersaw::arithmetic`] picks runs and follows the parameters; the other
/// keeps its phases, and switching hands it new coefficients and resumes it
/// where it stopped.
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
        match &setup.coefficients {
            Coefficients::Fixed(coefficients) => self.fixed.apply(coefficients),
            Coefficients::Float(coefficients) => self.float.apply(coefficients),
        }
    }

    /// One sample of every part from the bank [`Supersaw::arithmetic`]
    /// picks, then advances.
    #[inline]
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

/// What the supersaw generator plays (journal/D-044): the bank's
/// parameters, and which part each output carries, `None` for 0 V.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SupersawParams {
    /// The bank.
    pub bank: Supersaw,
    /// The part each output carries, A to D.
    pub parts: [Option<Part>; CV_CHANNELS],
}

impl SupersawParams {
    /// Nothing played, no output fed.
    pub const OFF: Self = Self {
        bank: Supersaw::OFF,
        parts: [None; CV_CHANNELS],
    };
}

/// What [`SupersawParams`] decide at a rate: the bank's [`Setup`], and the
/// parts.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SupersawSetup {
    bank: Setup,
    parts: [Option<Part>; CV_CHANNELS],
}

/// What runs: the bank, and the part each output takes from it.
#[derive(Debug, Clone)]
pub struct SupersawState {
    bank: Bank,
    parts: [Option<Part>; CV_CHANNELS],
}

impl SupersawState {
    /// The bank it runs, for an analysis that needs its steps and phases.
    #[must_use]
    pub const fn bank(&self) -> &Bank {
        &self.bank
    }
}

/// The bank as a generator: one frame of every part a sample, each output
/// taking its own.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SupersawBank;

impl Generator for SupersawBank {
    type Params = SupersawParams;
    type Setup = SupersawSetup;
    type State = SupersawState;

    const REST: SupersawParams = SupersawParams::OFF;
    const REST_SETUP: SupersawSetup = SupersawSetup {
        bank: Setup::OFF,
        parts: [None; CV_CHANNELS],
    };

    fn setup(params: &SupersawParams, rate_hz: u32) -> SupersawSetup {
        SupersawSetup {
            bank: Setup::new(&params.bank, rate_hz),
            parts: params.parts,
        }
    }

    fn start() -> SupersawState {
        SupersawState {
            bank: Bank::new(),
            parts: [None; CV_CHANNELS],
        }
    }

    fn apply(state: &mut SupersawState, setup: &SupersawSetup) {
        state.bank.apply(&setup.bank);
        state.parts = setup.parts;
    }

    #[inline]
    fn next(state: &mut SupersawState) -> [MilliVolts; CV_CHANNELS] {
        let frame = state.bank.next_frame();
        state
            .parts
            .map(|part| part.map_or(0, |part| frame.part(part)))
    }
}

/// `amount` in thousandths of [`FULL`] as a 16.16 fraction, at most one.
fn fraction(amount: u16) -> i64 {
    // In 32 bits: a division by a constant becomes a multiplication there,
    // a library call in 64 (journal/D-040).
    i64::from(u32::from(amount.min(FULL)) * (1 << 16) / u32::from(FULL))
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

/// Each voice's gain at `mix`, in 1.15: the centre's law for [`CENTRE`],
/// the sides' for the six others ([`mix_gains`]). What both banks, and any
/// reference rendering, weigh the voices by.
#[must_use]
pub fn voice_gains(mix: u16) -> [i64; VOICES] {
    let (centre, side) = mix_gains(mix);
    core::array::from_fn(|index| if index == CENTRE { centre } else { side })
}

/// How much louder, in root mean square, a voice at `shape` and `width` is
/// than a sawtooth, in 16.16: one at the sawtooth, `sqrt(3)` at the square.
///
/// A voice at `s` and `w` is `saw(t) - s saw(t - w)`. The two sawtooths'
/// product averages `1/3 - 2 w (1 - w)`, so its mean square is
/// `(1 - s)^2 / 3 + 4 s w (1 - w)`, against `1/3` for the sawtooth: the
/// ratio's root. At `w = 1/2`, `(1 + s + s^2) / 3`.
#[must_use]
pub fn shape_loudness(shape: u16, width: u16) -> i64 {
    let s = fraction(shape);
    let w = fraction(width);
    let rest = UNIT - s;
    // `(1 - s)^2 + 12 s w (1 - w)`, in 16.16.
    let ratio = ((rest * rest) >> 16) + ((12 * s * ((w * (UNIT - w)) >> 16)) >> 16);
    // The root of a 16.16 value, in 16.16; exact at one.
    let root = u64::try_from(ratio << 16).unwrap_or(0).isqrt();
    i64::try_from(root).unwrap_or(UNIT).max(1)
}

/// `2^32 / sqrt(power)`, `power` the summed squares of the gains in 1.15 of
/// the voices a part sums: dividing the sum by it keeps the loudness about
/// the same whatever the mix.
fn normaliser(power: i64) -> i64 {
    let power = u64::try_from(power).unwrap_or(0);
    // The centre gain stays above 0.44, so the root is far from zero.
    let root = u32::try_from(power.isqrt()).unwrap_or(u32::MAX).max(1);
    i64::try_from(div_wide(1 << 32, root)).unwrap_or(i64::MAX)
}

/// `normaliser` in 16.16 steps over `loudness` in 16.16: the normaliser of
/// a part whose voices are `loudness` louder than sawtooths.
fn louder(normaliser: i64, loudness: i64) -> i64 {
    let scaled = u64::try_from(normaliser).unwrap_or(0) << 16;
    let loudness = u32::try_from(loudness).unwrap_or(u32::MAX).max(1);
    i64::try_from(div_wide(scaled, loudness)).unwrap_or(i64::MAX)
}

#[cfg(test)]
mod tests {
    use super::{
        Arithmetic, Bank, CENTRE, FULL, LEFT, NARROWEST_WIDTH, OFFSETS, PHASES, RIGHT,
        SQUARE_WIDTH, Supersaw, UNIT, VOICES, detune_curve, mix_gains, shape_loudness,
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
    fn a_square_is_louder_by_the_root_of_three() {
        assert_eq!(
            shape_loudness(0, SQUARE_WIDTH),
            UNIT,
            "exactly one: a sawtooth unchanged"
        );
        // At no shape, the width does not matter.
        assert_eq!(shape_loudness(0, NARROWEST_WIDTH), UNIT);
        let square = shape_loudness(FULL, SQUARE_WIDTH);
        assert!((square - 113_512).abs() <= 2, "{square}");
        assert!(shape_loudness(FULL / 2, SQUARE_WIDTH) < square);
        // A quarter-width pulse: `12 / 4 * 3 / 4 = 2.25`, a root of 1.5.
        let quarter = shape_loudness(FULL, FULL / 4);
        assert!((quarter - 98_304).abs() <= 2, "{quarter}");
    }

    #[test]
    fn the_steps_are_those_of_the_bank_playing() {
        let mut params = Supersaw {
            frequency: 440_000,
            detune: FULL,
            mix: FULL / 2,
            shape: 0,
            width: SQUARE_WIDTH,
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
            shape: 0,
            width: SQUARE_WIDTH,
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
            shape: 0,
            width: SQUARE_WIDTH,
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
