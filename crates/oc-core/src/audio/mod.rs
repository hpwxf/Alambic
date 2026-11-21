//! The signal side of the outputs: how what an applet asks of them becomes
//! one sample at a time (journal/D-033, D-044).
//!
//! An applet's control part runs in [`Engine::tick`](crate::engine::Engine::tick)
//! and publishes, once a tick, the parameters of a [`Generator`]: something
//! that renders one sample of every output from a prepared [`Setup`]. A
//! [`Voice`] renders them at its own rate, keeping the generator's state
//! (phases, filters) across changes so a new note or level does not click.
//! The engine runs one voice at the tick rate; a backend with an audio
//! context runs its own at that context's rate, from
//! [`Engine::params`](crate::engine::Engine::params).
//!
//! This module holds the mechanism only, and names no applet: the trait,
//! the voice, [`Params`] with the outputs a probe holds over the sound, and
//! the generic blocks ([`wave`], [`basic`]'s levels and tones, [`phase_step`]).
//! Which generators an image has is decided where it composes its applets
//! ([`crate::apps::sound`]).
//!
//! Everything the parameters decide (phase steps, a bank's coefficients) is
//! computed once per change by [`Generator::setup`], through [`Setup::new`],
//! which touches no voice: a backend can run it in its control context and
//! hand the audio context a [`Setup`] that [`Voice::apply`] only copies
//! (journal/D-040).

pub mod basic;
pub mod wave;

use core::fmt::Debug;

use self::wave::ONE;
use crate::calibration::{CV_OUT_MAX_MV, CV_OUT_MIN_MV};
use crate::platform::{CV_CHANNELS, MilliVolts};

/// A frequency in thousandths of a hertz.
pub type MilliHertz = u32;

/// What renders one sample of every output (journal/D-044). An applet
/// publishes its [`Generator::Params`]; the loop turns them into a
/// [`Generator::Setup`] once per change; the audio context applies it to the
/// [`Generator::State`] it keeps and asks for samples.
///
/// A generator renders the four outputs at once: a bank of oscillators feeds
/// several of them, so the unit is not one output. It is named by a marker
/// type (a unit struct), hence the supertraits, which the derives of
/// [`Params`], [`Setup`] and [`Voice`] ask of it.
pub trait Generator: Copy + PartialEq + Debug {
    /// What an applet publishes, once a tick.
    type Params: Copy + PartialEq + Debug;
    /// What the parameters decide at a rate, computed outside the audio
    /// context: every division, every root.
    type Setup: Copy + Debug;
    /// What runs in the audio context: phases, filters, the coefficients
    /// copied from the setup.
    type State: Clone + Debug;

    /// Every output at rest.
    const REST: Self::Params;
    /// [`Self::REST`]'s setup, whatever the rate.
    const REST_SETUP: Self::Setup;

    /// What `params` decide at `rate_hz` (at least one).
    fn setup(params: &Self::Params, rate_hz: u32) -> Self::Setup;

    /// A fresh state, at rest.
    fn start() -> Self::State;

    /// Plays what `setup` prepared from the next sample on, keeping the
    /// phases and the filters: copies only, no division.
    fn apply(state: &mut Self::State, setup: &Self::Setup);

    /// One sample of every output, then advances.
    fn next(state: &mut Self::State) -> [MilliVolts; CV_CHANNELS];
}

/// What every output carries: a generator's parameters, and the outputs a
/// host probe holds at a level over them (journal/D-029).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Params<G: Generator> {
    /// What the generator plays.
    pub sound: G::Params,
    /// A level per output that overrides the sound, `None` to let it through.
    pub held: [Option<MilliVolts>; CV_CHANNELS],
}

impl<G: Generator> Params<G> {
    /// Every output at rest.
    pub const REST: Self = Self::new(G::REST);

    /// `sound` on every output, none held.
    #[must_use]
    pub const fn new(sound: G::Params) -> Self {
        Self {
            sound,
            held: [None; CV_CHANNELS],
        }
    }

    /// Holds output `index` at `level`, whatever the sound plays there.
    pub const fn hold(&mut self, index: usize, level: MilliVolts) {
        self.held[index] = Some(level);
    }
}

/// Everything [`Params`] decide at a rate, computed once when they change
/// (journal/D-040). Building it touches no voice, so a backend may do it
/// outside its audio context.
#[derive(Debug, Clone, Copy)]
pub struct Setup<G: Generator> {
    rate_hz: u32,
    params: Params<G>,
    sound: G::Setup,
}

impl<G: Generator> Setup<G> {
    /// Every output at rest at `rate_hz` (at least one).
    #[must_use]
    pub const fn rest(rate_hz: u32) -> Self {
        Self {
            rate_hz: if rate_hz == 0 { 1 } else { rate_hz },
            params: Params::REST,
            sound: G::REST_SETUP,
        }
    }

    /// What `params` decide at `rate_hz` (at least one).
    #[must_use]
    pub fn new(params: &Params<G>, rate_hz: u32) -> Self {
        let rate_hz = rate_hz.max(1);
        Self {
            rate_hz,
            params: *params,
            sound: G::setup(&params.sound, rate_hz),
        }
    }

    /// The parameters it was built from.
    #[must_use]
    pub const fn params(&self) -> &Params<G> {
        &self.params
    }

    /// The rate it was built for.
    #[must_use]
    pub const fn rate_hz(&self) -> u32 {
        self.rate_hz
    }
}

/// Renders [`Params`] at a fixed rate, one sample of every output a call.
///
/// The per-sample work is the generator's, then a level for each held
/// output; everything the parameters decide is prepared by [`Setup::new`].
#[derive(Debug, Clone)]
pub struct Voice<G: Generator> {
    rate_hz: u32,
    params: Params<G>,
    state: G::State,
    /// Whether any output is held: decided when the parameters change, so
    /// that a sample with none held pays nothing for the overlay.
    any_held: bool,
}

impl<G: Generator> Voice<G> {
    /// A voice at `rate_hz` samples a second (at least one), every output at
    /// rest.
    #[must_use]
    pub fn new(rate_hz: u32) -> Self {
        let mut state = G::start();
        G::apply(&mut state, &G::REST_SETUP);
        Self {
            rate_hz: rate_hz.max(1),
            params: Params::REST,
            state,
            any_held: false,
        }
    }

    /// Samples a second.
    #[must_use]
    pub const fn rate_hz(&self) -> u32 {
        self.rate_hz
    }

    /// Changes the rate, keeping the phases; the frequencies stay as they are.
    pub fn set_rate(&mut self, rate_hz: u32) {
        self.apply(&Setup::new(&self.params, rate_hz));
    }

    /// The parameters it renders.
    #[must_use]
    pub const fn params(&self) -> &Params<G> {
        &self.params
    }

    /// Renders `params` from the next sample on. A tone keeps its phase, so a
    /// new frequency or shape continues the wave where it was.
    pub fn set_params(&mut self, params: &Params<G>) {
        if self.params != *params {
            self.apply(&Setup::new(params, self.rate_hz));
        }
    }

    /// Renders what `setup` prepared, at its rate, from the next sample on,
    /// keeping the phases as [`Self::set_params`] does: copies only, no
    /// division (journal/D-040).
    pub fn apply(&mut self, setup: &Setup<G>) {
        self.rate_hz = setup.rate_hz;
        self.params = setup.params;
        self.any_held = setup.params.held.iter().any(Option::is_some);
        G::apply(&mut self.state, &setup.sound);
    }

    /// One sample of every output, then advances; a held output gives its
    /// level instead of the sound's.
    #[inline]
    pub fn next_sample(&mut self) -> [MilliVolts; CV_CHANNELS] {
        let mut out = G::next(&mut self.state);
        if self.any_held {
            for (sample, held) in out.iter_mut().zip(self.params.held) {
                if let Some(level) = held {
                    *sample = level;
                }
            }
        }
        out
    }
}

/// The phase a sample advances for `frequency` at `rate_hz`, held below half
/// a turn: at or above half the rate, a tone would alias to another pitch.
#[must_use]
pub fn phase_step(frequency: MilliHertz, rate_hz: u32) -> u32 {
    let turn = 1u64 << 32;
    // Divided in 32 bits whenever the rate in millihertz fits (journal/D-040).
    let numerator = u64::from(frequency) << 32;
    let step = match rate_hz.max(1).checked_mul(1_000) {
        Some(millihertz) => crate::fixed::div_wide(numerator, millihertz),
        None => numerator / (u64::from(rate_hz) * 1_000),
    };
    // One step short of half a turn: the highest pitch that is still itself.
    u32::try_from(step.min(turn / 2 - 1)).unwrap_or(u32::MAX)
}

/// `value`, `ONE` being the peak, as a level of peak `amplitude`, rounded
/// and held within what the outputs can produce.
pub(crate) fn scale(value: i32, amplitude: MilliVolts) -> MilliVolts {
    let level = (i64::from(value) * i64::from(amplitude) + i64::from(ONE / 2)) >> 15;
    #[allow(clippy::cast_possible_truncation)]
    // Clamped to the output range, a few volts.
    let level = level.clamp(i64::from(CV_OUT_MIN_MV), i64::from(CV_OUT_MAX_MV)) as MilliVolts;
    level
}

#[cfg(test)]
mod tests {
    use super::basic::{Basic, BasicParams, Channel, Tone};
    use super::wave::Waveform;
    use super::{Params, Setup, Voice, phase_step};

    type BasicVoice = Voice<Basic>;

    fn tone(wave: Waveform, frequency: u32, amplitude: i32) -> Channel {
        Channel::Tone(Tone {
            wave,
            frequency,
            amplitude,
        })
    }

    fn with(index: usize, channel: Channel) -> Params<Basic> {
        let mut sound = BasicParams::REST;
        sound.channels[index] = channel;
        Params::new(sound)
    }

    #[test]
    fn a_step_is_the_fraction_of_a_turn_per_sample() {
        assert_eq!(phase_step(1_000_000, 4_000), 1 << 30, "1 kHz at 4 kHz");
        assert_eq!(phase_step(0, 48_000), 0);
        assert_eq!(
            phase_step(30_000_000, 48_000),
            (1 << 31) - 1,
            "held below half the rate"
        );
    }

    #[test]
    fn a_new_rate_keeps_the_pitch() {
        let mut voice = BasicVoice::new(4_000);
        voice.set_params(&with(1, tone(Waveform::Saw, 1_000_000, 1_000)));
        voice.set_rate(8_000);
        assert_eq!(voice.rate_hz(), 8_000);
        let samples: [i32; 9] = core::array::from_fn(|_| voice.next_sample()[1]);
        assert_eq!(
            samples[0], samples[8],
            "one period every 8 samples at 8 kHz"
        );
        assert!(samples[0] < samples[1] && samples[1] < samples[2]);
    }

    #[test]
    fn holding_overrides_the_sound_on_that_output_only() {
        let mut voice = BasicVoice::new(4_000);
        let mut params = with(0, tone(Waveform::Square, 1_000_000, 2_000));
        params.sound.channels[1] = Channel::Level(-7);
        params.hold(0, 250);
        voice.set_params(&params);
        assert_eq!(voice.next_sample()[..2], [250, -7]);
    }

    #[test]
    fn a_setup_applied_plays_what_set_params_plays() {
        // Prepared elsewhere and applied, or set: the same samples, across
        // changes of pitch (journal/D-040).
        let (mut set, mut applied) = (BasicVoice::new(47_619), BasicVoice::new(47_619));
        for frequency in [110_000, 4_186_000, 220_000] {
            let params = with(0, tone(Waveform::Sine, frequency, 1_000));
            set.set_params(&params);
            applied.apply(&Setup::new(&params, 47_619));
            for _ in 0..500 {
                assert_eq!(set.next_sample(), applied.next_sample());
            }
        }
    }

    #[test]
    fn a_setup_brings_its_rate_and_parameters() {
        let params = with(0, tone(Waveform::Sine, 440_000, 1_000));
        let setup = Setup::new(&params, 8_000);
        assert_eq!((setup.rate_hz(), setup.params()), (8_000, &params));
        let mut voice = BasicVoice::new(4_000);
        voice.apply(&setup);
        assert_eq!((voice.rate_hz(), voice.params()), (8_000, &params));
        assert_eq!(Setup::<Basic>::rest(0).rate_hz(), 1);
        assert_eq!(Setup::<Basic>::rest(48_000).params(), &Params::REST);
    }
}
