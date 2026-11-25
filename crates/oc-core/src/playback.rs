//! What a backend plays the engine's outputs with: the mechanism of
//! [`crate::audio`] made concrete for this image's sounds ([`Sound`]) and
//! its instance slots ([`MAX_INSTANCES`]) (journal/D-040, D-046).
//!
//! The applets never reach here: they publish [`Params`](crate::apps::sound::Params)
//! and the engine gathers them into a [`Patch`]. A backend that plays in one
//! context follows the patch with a [`Player`]; one whose control and audio
//! run apart, like the module's loop and interrupt, splits it into a
//! [`Follower`] on the control side, [`Changes`] in between, and [`Voices`]
//! on the audio side.

use crate::apps::MAX_INSTANCES;
use crate::apps::sound::Sound;
use crate::audio;

/// What every output carries: each instance slot's parameters and route,
/// and the levels a probe holds (journal/D-046).
pub type Patch = audio::Patch<Sound, MAX_INSTANCES>;

/// What one slot's parameters decide at a rate, prepared outside the audio
/// context (journal/D-040).
pub type Setup = audio::Setup<Sound>;

/// What renders one slot, one sample at a time.
pub type Voice = audio::Voice<Sound>;

/// What the audio context takes from the control context when the patch
/// changed.
pub type Changes = audio::Changes<Sound, MAX_INSTANCES>;

/// The voices of every slot, routed to the outputs.
pub type Voices = audio::Voices<Sound, MAX_INSTANCES>;

/// What tells a backend's voices that the patch changed (journal/D-040).
pub type Follower = audio::Follower<Sound, MAX_INSTANCES>;

/// The voices a backend plays the engine's outputs with, in one context,
/// following its patch (journal/D-040, D-046).
pub type Player = audio::Player<Sound, MAX_INSTANCES>;
