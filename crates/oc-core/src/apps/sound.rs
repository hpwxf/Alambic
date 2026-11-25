//! What this image's applets can sound like (journal/D-044): the
//! generators it composes, the one [`Sound`] the voice runs, and the
//! [`Params`] an applet publishes. What a backend plays them with is
//! [`crate::playback`].
//!
//! The list sits beside [`AppHost`](super::AppHost), where the image picks its
//! applets: `audio` holds the mechanism and names none of them. Each kind of
//! sound is one variant; the voice dispatches on it once a sample, statically,
//! and a [`Setup`](crate::playback::Setup) stays a value of fixed size the audio interrupt copies
//! (journal/D-040): the largest of the generators' setups, not their sum.
//!
//! Written by hand while the list is two long; a macro would write the same
//! three enums once it grows.

use crate::apps::supersaw::generator::{
    SupersawGenerator, SupersawParams, SupersawSetup, SupersawState,
};
use crate::audio::basic::{Basic, BasicParams, BasicSetup, BasicState};
use crate::audio::{self, Generator};
use crate::platform::{CV_CHANNELS, MilliVolts};

/// What the outputs carry, as the running applet publishes it.
pub type Params = audio::Params<Sound>;

/// Every kind of sound this image's applets make.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Sound;

/// The parameters of one kind of sound.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SoundParams {
    /// Levels and tones, output by output: the CV applets, `TONE`.
    Basic(BasicParams),
    /// `SUPERSAW`'s bank.
    Supersaw(SupersawParams),
}

/// The setup of one kind of sound.
// No heap to box the bank into, and the size of the largest kind is the
// point (journal/D-044): what the interrupt copies, or keeps, is the
// largest one, where the voice of D-033 held them all.
#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SoundSetup {
    /// See [`SoundParams::Basic`].
    Basic(BasicSetup),
    /// See [`SoundParams::Supersaw`].
    Supersaw(SupersawSetup),
}

/// The running state of one kind of sound.
// No heap to box the bank into, and the size of the largest kind is the
// point (journal/D-044): what the interrupt copies, or keeps, is the
// largest one, where the voice of D-033 held them all.
#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone)]
pub enum SoundState {
    /// See [`SoundParams::Basic`].
    Basic(BasicState),
    /// See [`SoundParams::Supersaw`].
    Supersaw(SupersawState),
}

impl Generator for Sound {
    type Params = SoundParams;
    type Setup = SoundSetup;
    type State = SoundState;

    const REST: SoundParams = SoundParams::Basic(Basic::REST);
    const REST_SETUP: SoundSetup = SoundSetup::Basic(Basic::REST_SETUP);

    fn setup(params: &SoundParams, rate_hz: u32) -> SoundSetup {
        match params {
            SoundParams::Basic(params) => SoundSetup::Basic(Basic::setup(params, rate_hz)),
            SoundParams::Supersaw(params) => {
                SoundSetup::Supersaw(SupersawGenerator::setup(params, rate_hz))
            }
        }
    }

    fn start() -> SoundState {
        SoundState::Basic(Basic::start())
    }

    /// Applies `setup`; a setup of another kind than the state starts that
    /// kind afresh (a tone after the supersaw does not resume the bank).
    fn apply(state: &mut SoundState, setup: &SoundSetup) {
        match (&mut *state, setup) {
            (SoundState::Basic(state), SoundSetup::Basic(setup)) => Basic::apply(state, setup),
            (SoundState::Supersaw(state), SoundSetup::Supersaw(setup)) => {
                SupersawGenerator::apply(state, setup);
            }
            (_, SoundSetup::Basic(setup)) => {
                let mut fresh = Basic::start();
                Basic::apply(&mut fresh, setup);
                *state = SoundState::Basic(fresh);
            }
            (_, SoundSetup::Supersaw(setup)) => {
                let mut fresh = SupersawGenerator::start();
                SupersawGenerator::apply(&mut fresh, setup);
                *state = SoundState::Supersaw(fresh);
            }
        }
    }

    #[inline]
    fn next(state: &mut SoundState) -> [MilliVolts; CV_CHANNELS] {
        match state {
            SoundState::Basic(state) => Basic::next(state),
            SoundState::Supersaw(state) => SupersawGenerator::next(state),
        }
    }
}

impl Params {
    /// Each output held at its level, through the levels-and-tones
    /// generator: what a CV applet publishes.
    #[must_use]
    pub const fn levels(levels: [MilliVolts; CV_CHANNELS]) -> Self {
        Self::new(SoundParams::Basic(BasicParams::levels(levels)))
    }

    /// The levels and tones `sound` sets.
    #[must_use]
    pub const fn basic(sound: BasicParams) -> Self {
        Self::new(SoundParams::Basic(sound))
    }

    /// `SUPERSAW`'s bank, as `sound` sets it.
    #[must_use]
    pub const fn supersaw(sound: SupersawParams) -> Self {
        Self::new(SoundParams::Supersaw(sound))
    }

    /// The levels and tones it plays, if that is its kind.
    #[must_use]
    pub const fn as_basic(&self) -> Option<&BasicParams> {
        match &self.sound {
            SoundParams::Basic(sound) => Some(sound),
            SoundParams::Supersaw(_) => None,
        }
    }

    /// The supersaw it plays, if that is its kind.
    #[must_use]
    pub const fn as_supersaw(&self) -> Option<&SupersawParams> {
        match &self.sound {
            SoundParams::Supersaw(sound) => Some(sound),
            SoundParams::Basic(_) => None,
        }
    }
}

impl From<[MilliVolts; CV_CHANNELS]> for Params {
    fn from(levels: [MilliVolts; CV_CHANNELS]) -> Self {
        Self::levels(levels)
    }
}

impl SoundState {
    /// The supersaw it runs, if that is its kind: what an analysis
    /// needs of its steps and phases.
    #[must_use]
    pub const fn supersaw(&self) -> Option<&SupersawState> {
        match self {
            SoundState::Supersaw(state) => Some(state),
            SoundState::Basic(_) => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Params;
    use crate::apps::supersaw::dsp::Part;
    use crate::apps::supersaw::generator::SupersawParams;
    use crate::audio::basic::{BasicParams, Channel};
    use crate::playback::Voice;

    #[test]
    fn a_change_of_kind_starts_the_new_kind_afresh() {
        let mut sound = SupersawParams::OFF;
        sound.saw.amplitude = 1_000;
        sound.saw.frequency = 110_000;
        sound.parts[0] = Some(Part::Mono);
        let mut voice = Voice::new(47_619);
        voice.set_params(&Params::supersaw(sound));
        let first: [_; 64] = core::array::from_fn(|_| voice.next_sample());
        voice.set_params(&Params::levels([5; 4]));
        assert_eq!(voice.next_sample(), [5; 4]);
        voice.set_params(&Params::supersaw(sound));
        let again: [_; 64] = core::array::from_fn(|_| voice.next_sample());
        assert_eq!(first, again, "the bank starts again from its first phases");
    }

    #[test]
    fn levels_are_the_basic_kind() {
        let params = Params::from([1, 2, 3, 4]);
        assert_eq!(
            params.as_basic().map(|basic| basic.channels[3]),
            Some(Channel::Level(4))
        );
        assert_eq!(params, Params::basic(BasicParams::levels([1, 2, 3, 4])));
        assert!(params.as_supersaw().is_none());
    }
}
