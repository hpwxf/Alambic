//! Levels and tones, output by output: the generator the CV applets and
//! `TONE` share (journal/D-033, D-044).
//!
//! Each output holds a level, or plays a tone: a waveform at a frequency and
//! a peak level, centred on 0 V, its 32-bit phase kept when its parameters
//! change so a new note does not click.

use super::wave::Waveform;
use super::{Generator, MilliHertz, phase_step, scale};
use crate::platform::{CV_CHANNELS, MilliVolts};

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

/// What every output carries, A to D.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct BasicParams {
    /// One per output, A to D.
    pub channels: [Channel; CV_CHANNELS],
}

impl BasicParams {
    /// Every output at 0 V.
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
}

/// What [`BasicParams`] decide at a rate: each tone's phase step.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BasicSetup {
    channels: [Channel; CV_CHANNELS],
    steps: [u32; CV_CHANNELS],
}

/// What runs: the channels, their steps, and the phase of each output.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BasicState {
    channels: [Channel; CV_CHANNELS],
    steps: [u32; CV_CHANNELS],
    phases: [u32; CV_CHANNELS],
}

/// The levels-and-tones generator.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Basic;

impl Generator for Basic {
    type Params = BasicParams;
    type Setup = BasicSetup;
    type State = BasicState;

    const REST: BasicParams = BasicParams::REST;
    const REST_SETUP: BasicSetup = BasicSetup {
        channels: BasicParams::REST.channels,
        steps: [0; CV_CHANNELS],
    };

    fn setup(params: &BasicParams, rate_hz: u32) -> BasicSetup {
        BasicSetup {
            channels: params.channels,
            steps: params.channels.map(|channel| match channel {
                Channel::Level(_) => 0,
                Channel::Tone(tone) => phase_step(tone.frequency, rate_hz),
            }),
        }
    }

    fn start() -> BasicState {
        BasicState {
            channels: BasicParams::REST.channels,
            steps: [0; CV_CHANNELS],
            phases: [0; CV_CHANNELS],
        }
    }

    fn apply(state: &mut BasicState, setup: &BasicSetup) {
        state.channels = setup.channels;
        state.steps = setup.steps;
    }

    #[inline]
    fn next(state: &mut BasicState) -> [MilliVolts; CV_CHANNELS] {
        let mut out = [0; CV_CHANNELS];
        for (index, channel) in state.channels.iter().enumerate() {
            out[index] = match *channel {
                Channel::Level(level) => level,
                Channel::Tone(tone) => {
                    let value = tone.wave.at(state.phases[index]);
                    state.phases[index] = state.phases[index].wrapping_add(state.steps[index]);
                    scale(value, tone.amplitude)
                }
            };
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::super::wave::Waveform;
    use super::super::{Params, Voice};
    use super::{Basic, BasicParams, Channel, Tone};

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
    fn levels_are_held_as_they_are() {
        let mut voice = BasicVoice::new(48_000);
        voice.set_params(&Params::new(BasicParams::levels([1, -2, 3, 4_000])));
        assert_eq!(voice.next_sample(), [1, -2, 3, 4_000]);
        assert_eq!(voice.next_sample(), [1, -2, 3, 4_000]);
    }

    #[test]
    fn a_square_alternates_at_its_frequency() {
        // 1 kHz at 4 kHz: two samples up, two down.
        let mut voice = BasicVoice::new(4_000);
        voice.set_params(&with(3, tone(Waveform::Square, 1_000_000, 2_000)));
        let samples: [i32; 8] = core::array::from_fn(|_| voice.next_sample()[3]);
        assert_eq!(
            samples,
            [2_000, 2_000, -2_000, -2_000, 2_000, 2_000, -2_000, -2_000]
        );
    }

    #[test]
    fn a_new_frequency_keeps_the_phase() {
        let mut voice = BasicVoice::new(4_000);
        voice.set_params(&with(0, tone(Waveform::Sine, 1_000_000, 1_000)));
        assert_eq!(voice.next_sample()[0], 0);
        // A quarter turn on: the top of the sine, whatever the new pitch.
        voice.set_params(&with(0, tone(Waveform::Sine, 500_000, 1_000)));
        assert_eq!(voice.next_sample()[0], 1_000);
    }

    #[test]
    fn levels_beyond_the_outputs_are_held_at_their_edge() {
        let mut voice = BasicVoice::new(4_000);
        voice.set_params(&with(2, tone(Waveform::Square, 1_000_000, 50_000)));
        let first = voice.next_sample()[2];
        assert_eq!(first, crate::calibration::CV_OUT_MAX_MV);
    }
}
