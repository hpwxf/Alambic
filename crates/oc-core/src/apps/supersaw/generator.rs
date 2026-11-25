//! The supersaw as a generator (journal/D-044): what `SUPERSAW` publishes,
//! what the loop prepares from it, what the audio context runs, each output
//! taking its part of one oscillator.
//!
//! Each stage holds what its context needs, once: the parameters in the
//! control context, the coefficients in between, the oscillator in the
//! audio context; the parts travel with each (journal/D-040).

use super::dsp::{Coefficients, Oscillator, Part, Supersaw};
use crate::audio::Generator;
use crate::platform::{CV_CHANNELS, MilliVolts};

/// What the supersaw generator plays: the sound, and which part each output
/// carries, `None` for 0 V.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SupersawParams {
    /// The sound: pitch, detune, mix, shape and level.
    pub saw: Supersaw,
    /// The part each output carries, A to D.
    pub parts: [Option<Part>; CV_CHANNELS],
}

impl SupersawParams {
    /// Nothing played, no output fed.
    pub const OFF: Self = Self {
        saw: Supersaw::OFF,
        parts: [None; CV_CHANNELS],
    };
}

/// What [`SupersawParams`] decide at a rate: the oscillator's coefficients,
/// and the parts.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SupersawSetup {
    coefficients: Coefficients,
    parts: [Option<Part>; CV_CHANNELS],
}

/// What runs: the oscillator, and the part each output takes from it.
#[derive(Debug, Clone)]
pub struct SupersawState {
    oscillator: Oscillator,
    parts: [Option<Part>; CV_CHANNELS],
}

impl SupersawState {
    /// The oscillator it runs, for an analysis that needs its steps and
    /// phases.
    #[must_use]
    pub const fn oscillator(&self) -> &Oscillator {
        &self.oscillator
    }
}

/// The supersaw generator: one frame of every part a sample, each output
/// taking its own.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SupersawGenerator;

impl Generator for SupersawGenerator {
    type Params = SupersawParams;
    type Setup = SupersawSetup;
    type State = SupersawState;

    const REST: SupersawParams = SupersawParams::OFF;
    const REST_SETUP: SupersawSetup = SupersawSetup {
        coefficients: Coefficients::OFF,
        parts: [None; CV_CHANNELS],
    };

    fn setup(params: &SupersawParams, rate_hz: u32) -> SupersawSetup {
        SupersawSetup {
            coefficients: Coefficients::new(&params.saw, rate_hz),
            parts: params.parts,
        }
    }

    fn start() -> SupersawState {
        SupersawState {
            oscillator: Oscillator::new(),
            parts: [None; CV_CHANNELS],
        }
    }

    fn apply(state: &mut SupersawState, setup: &SupersawSetup) {
        state.oscillator.apply(&setup.coefficients);
        state.parts = setup.parts;
    }

    #[inline]
    fn next(state: &mut SupersawState) -> [MilliVolts; CV_CHANNELS] {
        let frame = state.oscillator.next_frame();
        state
            .parts
            .map(|part| part.map_or(0, |part| frame.part(part)))
    }
}
