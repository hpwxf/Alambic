//! The signal side of the outputs: what an applet asks of each output, and
//! the voice that turns it into one sample at a time (journal/D-033).
//!
//! An applet's control part runs in [`Engine::tick`](crate::engine::Engine::tick)
//! and publishes [`Params`] once a tick: for each output, a level held or a
//! [`Tone`]. A [`Voice`] renders them at its own rate, keeping the phase of
//! each output across changes so a new note or level does not click. The
//! engine runs one voice at the tick rate; a backend with an audio context
//! runs its own at that context's rate, from [`Engine::params`](crate::engine::Engine::params).

pub mod wave;

use self::wave::{ONE, Waveform};
use crate::calibration::{CV_OUT_MAX_MV, CV_OUT_MIN_MV};
use crate::platform::{CV_CHANNELS, MilliVolts};

/// A frequency in thousandths of a hertz.
pub type MilliHertz = u32;

/// A periodic signal on one output, centred on 0 V.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Tone {
    /// Its shape.
    pub wave: Waveform,
    /// Its frequency; a voice holds it below half its rate.
    pub frequency: MilliHertz,
    /// Its peak level: the signal swings from minus this to plus this.
    pub amplitude: MilliVolts,
}

/// What one output carries.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Channel {
    /// A level, held until the next parameters.
    Level(MilliVolts),
    /// A tone.
    Tone(Tone),
}

/// What every output carries, as an applet publishes it once a tick.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Params {
    /// One per output, A to D.
    pub channels: [Channel; CV_CHANNELS],
}

impl Params {
    /// Every output at rest.
    pub const REST: Self = Self::levels([0; CV_CHANNELS]);

    /// Each output held at its level.
    #[must_use]
    pub const fn levels(levels: [MilliVolts; CV_CHANNELS]) -> Self {
        let mut channels = [Channel::Level(0); CV_CHANNELS];
        let mut index = 0;
        while index < CV_CHANNELS {
            channels[index] = Channel::Level(levels[index]);
            index += 1;
        }
        Self { channels }
    }

    /// Holds output `index` at `level`, whatever it carried.
    pub const fn hold(&mut self, index: usize, level: MilliVolts) {
        self.channels[index] = Channel::Level(level);
    }
}

impl From<[MilliVolts; CV_CHANNELS]> for Params {
    fn from(levels: [MilliVolts; CV_CHANNELS]) -> Self {
        Self::levels(levels)
    }
}

/// Renders [`Params`] at a fixed rate, one sample of every output a call.
///
/// The per-sample work is a phase addition and a waveform lookup per tone;
/// the phase steps are computed when the parameters or the rate change.
#[derive(Debug, Clone)]
pub struct Voice {
    rate_hz: u32,
    params: Params,
    /// Phase added per sample, per output; zero for a level.
    steps: [u32; CV_CHANNELS],
    phases: [u32; CV_CHANNELS],
}

impl Voice {
    /// A voice at `rate_hz` samples a second (at least one), every output at
    /// rest.
    #[must_use]
    pub const fn new(rate_hz: u32) -> Self {
        Self {
            rate_hz: if rate_hz == 0 { 1 } else { rate_hz },
            params: Params::REST,
            steps: [0; CV_CHANNELS],
            phases: [0; CV_CHANNELS],
        }
    }

    /// Samples a second.
    #[must_use]
    pub const fn rate_hz(&self) -> u32 {
        self.rate_hz
    }

    /// Changes the rate, keeping the phases; the frequencies stay as they are.
    pub fn set_rate(&mut self, rate_hz: u32) {
        self.rate_hz = rate_hz.max(1);
        self.update_steps();
    }

    /// The parameters it renders.
    #[must_use]
    pub const fn params(&self) -> &Params {
        &self.params
    }

    /// Renders `params` from the next sample on. A tone keeps its phase, so a
    /// new frequency or shape continues the wave where it was.
    pub fn set_params(&mut self, params: &Params) {
        if self.params != *params {
            self.params = *params;
            self.update_steps();
        }
    }

    /// One sample of every output, then advances the phases.
    pub fn next_sample(&mut self) -> [MilliVolts; CV_CHANNELS] {
        let mut out = [0; CV_CHANNELS];
        for (index, channel) in self.params.channels.iter().enumerate() {
            out[index] = match *channel {
                Channel::Level(level) => level,
                Channel::Tone(tone) => {
                    let value = tone.wave.at(self.phases[index]);
                    self.phases[index] = self.phases[index].wrapping_add(self.steps[index]);
                    scale(value, tone.amplitude)
                }
            };
        }
        out
    }

    /// The phase step of each tone at the current rate.
    fn update_steps(&mut self) {
        for (step, channel) in self.steps.iter_mut().zip(&self.params.channels) {
            *step = match *channel {
                Channel::Level(_) => 0,
                Channel::Tone(tone) => phase_step(tone.frequency, self.rate_hz),
            };
        }
    }
}

/// The phase a sample advances for `frequency` at `rate_hz`, held below half
/// a turn: at or above half the rate, a tone would alias to another pitch.
#[must_use]
pub fn phase_step(frequency: MilliHertz, rate_hz: u32) -> u32 {
    let turn = 1u64 << 32;
    let step = (u64::from(frequency) << 32) / (u64::from(rate_hz.max(1)) * 1_000);
    // One step short of half a turn: the highest pitch that is still itself.
    u32::try_from(step.min(turn / 2 - 1)).unwrap_or(u32::MAX)
}

/// `value` in `-ONE..=ONE` as a level of peak `amplitude`, rounded and held
/// within what the outputs can produce.
fn scale(value: i32, amplitude: MilliVolts) -> MilliVolts {
    let level = (i64::from(value) * i64::from(amplitude) + i64::from(ONE / 2)) >> 15;
    #[allow(clippy::cast_possible_truncation)]
    // Clamped to the output range, a few volts.
    let level = level.clamp(i64::from(CV_OUT_MIN_MV), i64::from(CV_OUT_MAX_MV)) as MilliVolts;
    level
}

#[cfg(test)]
mod tests {
    use super::wave::Waveform;
    use super::{Channel, Params, Tone, Voice, phase_step};

    fn tone(wave: Waveform, frequency: u32, amplitude: i32) -> Channel {
        Channel::Tone(Tone {
            wave,
            frequency,
            amplitude,
        })
    }

    #[test]
    fn levels_are_held_as_they_are() {
        let mut voice = Voice::new(48_000);
        voice.set_params(&Params::levels([1, -2, 3, 4_000]));
        assert_eq!(voice.next_sample(), [1, -2, 3, 4_000]);
        assert_eq!(voice.next_sample(), [1, -2, 3, 4_000]);
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
    fn a_square_alternates_at_its_frequency() {
        // 1 kHz at 4 kHz: two samples up, two down.
        let mut voice = Voice::new(4_000);
        let mut params = Params::REST;
        params.channels[3] = tone(Waveform::Square, 1_000_000, 2_000);
        voice.set_params(&params);
        let samples: [i32; 8] = core::array::from_fn(|_| voice.next_sample()[3]);
        assert_eq!(
            samples,
            [2_000, 2_000, -2_000, -2_000, 2_000, 2_000, -2_000, -2_000]
        );
    }

    #[test]
    fn a_new_frequency_keeps_the_phase() {
        let mut voice = Voice::new(4_000);
        let mut params = Params::REST;
        params.channels[0] = tone(Waveform::Sine, 1_000_000, 1_000);
        voice.set_params(&params);
        assert_eq!(voice.next_sample()[0], 0);
        // A quarter turn on: the top of the sine, whatever the new pitch.
        params.channels[0] = tone(Waveform::Sine, 500_000, 1_000);
        voice.set_params(&params);
        assert_eq!(voice.next_sample()[0], 1_000);
    }

    #[test]
    fn a_new_rate_keeps_the_pitch() {
        let mut voice = Voice::new(4_000);
        let mut params = Params::REST;
        params.channels[1] = tone(Waveform::Saw, 1_000_000, 1_000);
        voice.set_params(&params);
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
    fn levels_beyond_the_outputs_are_held_at_their_edge() {
        let mut voice = Voice::new(4_000);
        let mut params = Params::REST;
        params.channels[2] = tone(Waveform::Square, 1_000_000, 50_000);
        voice.set_params(&params);
        let first = voice.next_sample()[2];
        assert_eq!(first, crate::calibration::CV_OUT_MAX_MV);
    }

    #[test]
    fn holding_replaces_a_tone() {
        let mut params = Params::REST;
        params.channels[0] = tone(Waveform::Sine, 440_000, 1_000);
        params.hold(0, 250);
        assert_eq!(params.channels[0], Channel::Level(250));
        assert_eq!(Params::from([7; 4]), Params::levels([7; 4]));
    }
}
