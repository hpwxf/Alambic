//! What this image's applets can sound like (journal/D-044): the
//! generators it composes, the one [`Sound`] the voice runs.
//!
//! The list sits beside [`AppHost`](super::AppHost), where the image picks its
//! applets: `audio` holds the mechanism and names none of them. Each kind of
//! sound is one variant; the voice dispatches on it once a sample, statically,
//! and a [`Setup`] stays a value of fixed size the audio interrupt copies
//! (journal/D-040): the largest of the generators' setups, not their sum.
//!
//! Written by hand while the list is two long; a macro would write the same
//! three enums once it grows.

use crate::apps::MAX_INSTANCES;
use crate::apps::supersaw::bank::{SupersawBank, SupersawParams, SupersawSetup, SupersawState};
use crate::audio::basic::{Basic, BasicParams, BasicSetup, BasicState};
use crate::audio::{self, Generator};
use crate::platform::{CV_CHANNELS, MilliVolts};

/// What the outputs carry, as the running applet publishes it.
pub type Params = audio::Params<Sound>;

/// What [`Params`] decide at a rate, prepared in the loop.
pub type Setup = audio::Setup<Sound>;

/// The voice every backend runs.
pub type Voice = audio::Voice<Sound>;

/// What every output carries: each instance slot's parameters and route,
/// and the levels a probe holds (journal/D-046).
pub type Patch = audio::Patch<Sound, MAX_INSTANCES>;

/// What the audio context takes from the loop when the patch changed.
pub type Changes = audio::Changes<Sound, MAX_INSTANCES>;

/// The voices of every slot, routed to the outputs.
pub type Voices = audio::Voices<Sound, MAX_INSTANCES>;

/// What tells a backend's voices that the patch changed (journal/D-040).
pub type Follower = audio::Follower<Sound, MAX_INSTANCES>;

/// The voices a backend plays the engine's outputs with, following its
/// patch (journal/D-040, D-046).
pub type Player = audio::Player<Sound, MAX_INSTANCES>;

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
                SoundSetup::Supersaw(SupersawBank::setup(params, rate_hz))
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
                SupersawBank::apply(state, setup);
            }
            (_, SoundSetup::Basic(setup)) => {
                let mut fresh = Basic::start();
                Basic::apply(&mut fresh, setup);
                *state = SoundState::Basic(fresh);
            }
            (_, SoundSetup::Supersaw(setup)) => {
                let mut fresh = SupersawBank::start();
                SupersawBank::apply(&mut fresh, setup);
                *state = SoundState::Supersaw(fresh);
            }
        }
    }

    #[inline]
    fn next(state: &mut SoundState) -> [MilliVolts; CV_CHANNELS] {
        match state {
            SoundState::Basic(state) => Basic::next(state),
            SoundState::Supersaw(state) => SupersawBank::next(state),
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
    /// The supersaw bank it runs, if that is its kind: what an analysis
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
    use super::{Params, Voice};
    use crate::apps::supersaw::bank::{Part, SupersawParams};
    use crate::audio::basic::{BasicParams, Channel};

    #[test]
    fn a_change_of_kind_starts_the_new_kind_afresh() {
        let mut saw = SupersawParams::OFF;
        saw.bank.amplitude = 1_000;
        saw.bank.frequency = 110_000;
        saw.parts[0] = Some(Part::Mono);
        let mut voice = Voice::new(47_619);
        voice.set_params(&Params::supersaw(saw));
        let first: [_; 64] = core::array::from_fn(|_| voice.next_sample());
        voice.set_params(&Params::levels([5; 4]));
        assert_eq!(voice.next_sample(), [5; 4]);
        voice.set_params(&Params::supersaw(saw));
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
