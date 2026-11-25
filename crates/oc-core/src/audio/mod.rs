//! The signal side of the outputs: how what an applet asks of them becomes
//! one sample at a time (journal/D-033, D-044).
//!
//! An applet's control part runs in [`Engine::tick`](crate::engine::Engine::tick)
//! and publishes, once a tick, the parameters of a [`Generator`]: something
//! that renders one sample of every output from a prepared [`Setup`]. A
//! [`Voice`] renders them at its own rate, keeping the generator's state
//! (phases, filters) across changes so a new note or level does not click.
//! The engine renders none: every backend plays [`Engine::patch`] at its
//! own rate, through a [`Player`] or, on the module, its audio interrupt's
//! [`Voices`], and a [`Follower`] tells either when it changed
//! (journal/D-040).
//!
//! [`Engine::patch`]: crate::engine::Engine::patch
//!
//! This module holds the mechanism only, and names no applet: the trait,
//! the voice, the [`Patch`] of every instance slot with its [`Route`]s and the
//! outputs a probe holds, the [`Voices`] that play it (journal/D-046), and
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
use crate::platform::{CV_CHANNELS, CvChannel, MilliVolts};

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

/// What one instance asks of the lanes of its generator (journal/D-044,
/// D-046): where they go is its [`Route`]'s business, not the applet's.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Params<G: Generator> {
    /// What the generator plays.
    pub sound: G::Params,
}

impl<G: Generator> Params<G> {
    /// Every lane at rest.
    pub const REST: Self = Self::new(G::REST);

    /// `sound` on every lane.
    #[must_use]
    pub const fn new(sound: G::Params) -> Self {
        Self { sound }
    }
}

/// Where each lane of a generator goes: an output, or nowhere (journal/D-046).
/// A generator renders [`CV_CHANNELS`] lanes; an instance declares how many
/// it uses as its output ports, and the route sends each to a jack.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Route {
    lanes: [Option<CvChannel>; CV_CHANNELS],
}

impl Route {
    /// Every lane to the output of the same rank: what one instance alone
    /// had before routes.
    pub const IDENTITY: Self = Self::first(CV_CHANNELS);

    /// No lane goes anywhere: an empty slot.
    pub const OFF: Self = Self {
        lanes: [None; CV_CHANNELS],
    };

    /// The first `count` lanes to the outputs of the same rank, the others
    /// nowhere: the default route of an instance with `count` ports.
    #[must_use]
    pub const fn first(count: usize) -> Self {
        let mut lanes = [None; CV_CHANNELS];
        let mut lane = 0;
        while lane < CV_CHANNELS && lane < count {
            lanes[lane] = CvChannel::from_index(lane);
            lane += 1;
        }
        Self { lanes }
    }

    /// The output `lane` goes to, if any.
    #[must_use]
    pub const fn output(&self, lane: usize) -> Option<CvChannel> {
        if lane < CV_CHANNELS {
            self.lanes[lane]
        } else {
            None
        }
    }

    /// Sends `lane` to `output`, or nowhere; a lane past the generator's is
    /// ignored. Whether another lane or instance holds `output` already is
    /// the caller's to check.
    pub const fn set(&mut self, lane: usize, output: Option<CvChannel>) {
        if lane < CV_CHANNELS {
            self.lanes[lane] = output;
        }
    }

    /// Whether no lane goes anywhere: the voice need not run.
    #[must_use]
    pub const fn is_off(&self) -> bool {
        let mut lane = 0;
        while lane < CV_CHANNELS {
            if self.lanes[lane].is_some() {
                return false;
            }
            lane += 1;
        }
        true
    }
}

/// One instance's parameters and where its lanes go.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Routed<G: Generator> {
    /// What its generator plays.
    pub params: Params<G>,
    /// Where each lane goes.
    pub route: Route,
}

impl<G: Generator> Routed<G> {
    /// An empty slot: at rest, going nowhere.
    pub const OFF: Self = Self {
        params: Params::REST,
        route: Route::OFF,
    };
}

/// What every output carries: each slot's parameters and route, and the
/// levels a host probe holds over them (journal/D-029, D-046). An output no
/// route reaches sits at 0 V; when two routes reach one, the later slot's
/// wins, which the engine never lets happen.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Patch<G: Generator, const N: usize> {
    /// One per instance slot.
    pub parts: [Routed<G>; N],
    /// A level per output that overrides whatever reaches it, `None` to let
    /// it through.
    pub held: [Option<MilliVolts>; CV_CHANNELS],
}

impl<G: Generator, const N: usize> Patch<G, N> {
    /// Every slot empty, every output at 0 V.
    pub const OFF: Self = Self {
        parts: [Routed::OFF; N],
        held: [None; CV_CHANNELS],
    };

    /// `params` in the first slot along `route`, the others empty.
    #[must_use]
    pub const fn single(params: Params<G>, route: Route) -> Self {
        let mut patch = Self::OFF;
        if N > 0 {
            patch.parts[0] = Routed { params, route };
        }
        patch
    }

    /// Holds output `index` at `level`, whatever reaches it.
    pub const fn hold(&mut self, index: usize, level: MilliVolts) {
        if index < CV_CHANNELS {
            self.held[index] = Some(level);
        }
    }
}

/// Everything [`Params`] decide at a rate, computed once when they change
/// (journal/D-040). Building it touches no voice, so a backend may do it
/// outside its audio context. It holds the generator's setup alone: the
/// parameters stay in the control context, which knows what changed, and
/// the rate is the voice's.
#[derive(Debug, Clone, Copy)]
pub struct Setup<G: Generator> {
    sound: G::Setup,
}

impl<G: Generator> Setup<G> {
    /// Every output at rest, whatever the rate.
    pub const REST: Self = Self {
        sound: G::REST_SETUP,
    };

    /// What `params` decide at `rate_hz` (at least one).
    #[must_use]
    pub fn new(params: &Params<G>, rate_hz: u32) -> Self {
        Self {
            sound: G::setup(&params.sound, rate_hz.max(1)),
        }
    }
}

/// Renders [`Params`] at a fixed rate, one sample of every output a call.
///
/// The per-sample work is the generator's; everything the parameters decide
/// is prepared by [`Setup::new`]. Its lanes reach the outputs through
/// [`Voices`]. It keeps the generator's state and its rate, not the
/// parameters: what changed is the control context's to know.
#[derive(Debug, Clone)]
pub struct Voice<G: Generator> {
    rate_hz: u32,
    state: G::State,
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
            state,
        }
    }

    /// Samples a second.
    #[must_use]
    pub const fn rate_hz(&self) -> u32 {
        self.rate_hz
    }

    /// Changes the rate the next parameters are prepared at
    /// ([`Self::set_params`]); what plays keeps its steps until then, so a
    /// backend prepares its parameters again ([`Player::set_rate`]).
    pub const fn set_rate(&mut self, rate_hz: u32) {
        self.rate_hz = if rate_hz == 0 { 1 } else { rate_hz };
    }

    /// Renders `params` from the next sample on, prepared at the voice's
    /// rate. A tone keeps its phase, so a new frequency or shape continues
    /// the wave where it was.
    pub fn set_params(&mut self, params: &Params<G>) {
        self.apply(&Setup::new(params, self.rate_hz));
    }

    /// Renders what `setup` prepared from the next sample on, keeping the
    /// phases as [`Self::set_params`] does: copies only, no division
    /// (journal/D-040). The setup is the caller's to prepare at the voice's
    /// rate.
    pub fn apply(&mut self, setup: &Setup<G>) {
        G::apply(&mut self.state, &setup.sound);
    }

    /// One sample of every lane, then advances.
    #[inline]
    pub fn next_sample(&mut self) -> [MilliVolts; CV_CHANNELS] {
        G::next(&mut self.state)
    }
}

/// What a backend's audio context takes from its control context: a
/// prepared setup for each slot whose parameters changed, every route and
/// every held level (journal/D-040, D-046). Routes and levels are a few
/// bytes, so they always come; a setup only when it must be prepared again.
#[derive(Debug, Clone, Copy)]
pub struct Changes<G: Generator, const N: usize> {
    setups: [Option<Setup<G>>; N],
    routes: [Route; N],
    held: [Option<MilliVolts>; CV_CHANNELS],
}

impl<G: Generator, const N: usize> Changes<G, N> {
    const NO_SETUP: Option<Setup<G>> = None;

    /// No setup, every route off, nothing held: every output at 0 V once
    /// applied, the voices kept as they are.
    pub const SILENCE: Self = Self {
        setups: [Self::NO_SETUP; N],
        routes: [Route::OFF; N],
        held: [None; CV_CHANNELS],
    };

    /// Takes in `newer`, handed after `self` and not yet applied: its
    /// setups where it has one, its routes and levels.
    pub fn merge(&mut self, newer: &Self) {
        for (setup, newer) in self.setups.iter_mut().zip(&newer.setups) {
            if newer.is_some() {
                *setup = *newer;
            }
        }
        self.routes = newer.routes;
        self.held = newer.held;
    }

    /// Forgets the setups once [`Voices::apply`] took them, keeping the
    /// routes and levels: a later [`Self::merge`] brings only what moved.
    pub fn applied(&mut self) {
        self.setups = [Self::NO_SETUP; N];
    }
}

/// The voices of every slot, and how their lanes reach the outputs: what a
/// backend renders the engine's [`Patch`] with, one sample at a time
/// (journal/D-046). An empty slot's voice does not run.
#[derive(Debug, Clone)]
pub struct Voices<G: Generator, const N: usize> {
    /// One voice per slot.
    slots: [Voice<G>; N],
    routes: [Route; N],
    /// Whether each slot's route reaches an output: decided when the routes
    /// change, so that an empty slot costs one test a sample.
    active: [bool; N],
    held: [Option<MilliVolts>; CV_CHANNELS],
    /// Whether any output is held: decided when the levels change, so that
    /// a sample with none held pays nothing for the overlay.
    any_held: bool,
}

impl<G: Generator, const N: usize> Voices<G, N> {
    /// Voices at `rate_hz` samples a second (at least one), every slot
    /// empty: every output at 0 V.
    #[must_use]
    pub fn new(rate_hz: u32) -> Self {
        Self {
            slots: core::array::from_fn(|_| Voice::new(rate_hz)),
            routes: [Route::OFF; N],
            active: [false; N],
            held: [None; CV_CHANNELS],
            any_held: false,
        }
    }

    /// Samples a second.
    #[must_use]
    pub fn rate_hz(&self) -> u32 {
        self.slots.first().map_or(1, Voice::rate_hz)
    }

    /// Changes the rate the next changes are prepared at; what plays keeps
    /// its steps until then ([`Voice::set_rate`]).
    pub fn set_rate(&mut self, rate_hz: u32) {
        for voice in &mut self.slots {
            voice.set_rate(rate_hz);
        }
    }

    /// Plays what `changes` brings from the next sample on: copies only, no
    /// division (journal/D-040).
    pub fn apply(&mut self, changes: &Changes<G, N>) {
        for (voice, setup) in self.slots.iter_mut().zip(&changes.setups) {
            if let Some(setup) = setup {
                voice.apply(setup);
            }
        }
        self.routes = changes.routes;
        self.active = changes.routes.map(|route| !route.is_off());
        self.held = changes.held;
        self.any_held = changes.held.iter().any(Option::is_some);
    }

    /// One sample of every output, then advances: each lane where its route
    /// sends it, 0 V where none does, a held output at its level.
    #[inline]
    pub fn next_sample(&mut self) -> [MilliVolts; CV_CHANNELS] {
        let mut out = [0; CV_CHANNELS];
        for ((voice, route), &active) in self.slots.iter_mut().zip(&self.routes).zip(&self.active) {
            if !active {
                continue;
            }
            for (level, output) in voice.next_sample().into_iter().zip(route.lanes) {
                if let Some(output) = output {
                    out[output.index()] = level;
                }
            }
        }
        if self.any_held {
            for (sample, held) in out.iter_mut().zip(self.held) {
                if let Some(level) = held {
                    *sample = level;
                }
            }
        }
        out
    }
}

/// Follows the patch a publisher numbers
/// ([`Engine::params_revision`](crate::engine::Engine::params_revision)):
/// what tells a backend's voices that it changed, by a few bytes rather than
/// a whole [`Patch`] every tick or every sample (journal/D-040). When the
/// revision moves, only the slots whose parameters changed are prepared
/// again.
#[derive(Debug, Clone, Copy)]
pub struct Follower<G: Generator, const N: usize> {
    /// The revision last followed, if any.
    revision: Option<u32>,
    /// The parameters of each slot last prepared.
    prepared: [Option<Params<G>>; N],
}

impl<G: Generator, const N: usize> Follower<G, N> {
    const NOTHING: Option<Params<G>> = None;

    /// A follower that has followed nothing: its first [`Self::follow`]
    /// prepares every slot.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            revision: None,
            prepared: [Self::NOTHING; N],
        }
    }

    /// What `patch` changes at `rate_hz`, if `revision` is not the one last
    /// followed; nothing otherwise.
    pub fn follow(
        &mut self,
        revision: u32,
        patch: &Patch<G, N>,
        rate_hz: u32,
    ) -> Option<Changes<G, N>> {
        if self.revision == Some(revision) {
            return None;
        }
        self.revision = Some(revision);
        let mut changes = Changes::SILENCE;
        for (slot, part) in patch.parts.iter().enumerate() {
            if self.prepared[slot] != Some(part.params) {
                changes.setups[slot] = Some(Setup::new(&part.params, rate_hz));
                self.prepared[slot] = Some(part.params);
            }
            changes.routes[slot] = part.route;
        }
        changes.held = patch.held;
        Some(changes)
    }

    /// Forgets what it followed, so the next [`Self::follow`] prepares every
    /// slot whatever its revision.
    pub const fn forget(&mut self) {
        self.revision = None;
        self.prepared = [Self::NOTHING; N];
    }
}

impl<G: Generator, const N: usize> Default for Follower<G, N> {
    fn default() -> Self {
        Self::new()
    }
}

/// Voices that follow the patch a publisher numbers: what a backend plays
/// the engine's outputs with, at its own rate (journal/D-040, D-046).
#[derive(Debug, Clone)]
pub struct Player<G: Generator, const N: usize> {
    voices: Voices<G, N>,
    follower: Follower<G, N>,
}

impl<G: Generator, const N: usize> Player<G, N> {
    /// A player at `rate_hz` samples a second (at least one), every output
    /// at 0 V until its first [`Self::play`].
    #[must_use]
    pub fn new(rate_hz: u32) -> Self {
        Self {
            voices: Voices::new(rate_hz),
            follower: Follower::new(),
        }
    }

    /// Samples a second.
    #[must_use]
    pub fn rate_hz(&self) -> u32 {
        self.voices.rate_hz()
    }

    /// Changes the rate, keeping the phases; the frequencies stay as they
    /// are: every slot is prepared again at the new rate on the next
    /// [`Self::play`].
    pub fn set_rate(&mut self, rate_hz: u32) {
        self.voices.set_rate(rate_hz);
        self.follower.forget();
    }

    /// One sample of every output of `patch`, then advances; the voices are
    /// prepared again only when `revision` moved on, and only the slots that
    /// changed.
    pub fn play(&mut self, revision: u32, patch: &Patch<G, N>) -> [MilliVolts; CV_CHANNELS] {
        if let Some(changes) = self.follower.follow(revision, patch, self.voices.rate_hz()) {
            self.voices.apply(&changes);
        }
        self.voices.next_sample()
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
    use super::{Follower, Params, Patch, Player, Route, Setup, Voice, Voices, phase_step};
    use crate::platform::CvChannel;

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
        let patch = Patch::<Basic, 1>::single(
            with(1, tone(Waveform::Saw, 1_000_000, 1_000)),
            Route::IDENTITY,
        );
        let mut player = Player::new(4_000);
        player.play(1, &patch);
        player.set_rate(8_000);
        assert_eq!(player.rate_hz(), 8_000);
        let samples: [i32; 9] = core::array::from_fn(|_| player.play(1, &patch)[1]);
        assert_eq!(
            samples[0], samples[8],
            "one period every 8 samples at 8 kHz"
        );
        let distinct = (0..8).all(|a| (0..a).all(|b| samples[a] != samples[b]));
        assert!(distinct, "not a shorter period: {samples:?}");
    }

    #[test]
    fn holding_overrides_the_sound_on_that_output_only() {
        let mut params = with(0, tone(Waveform::Square, 1_000_000, 2_000));
        params.sound.channels[1] = Channel::Level(-7);
        let mut patch = Patch::<Basic, 1>::single(params, Route::IDENTITY);
        patch.hold(0, 250);
        let mut player = Player::new(4_000);
        assert_eq!(player.play(1, &patch)[..2], [250, -7]);
    }

    #[test]
    fn a_route_sends_each_lane_to_its_output() {
        let params = Params::new(BasicParams::levels([1, 2, 3, 4]));
        let mut route = Route::OFF;
        route.set(0, Some(CvChannel::Four));
        route.set(2, Some(CvChannel::One));
        let mut player = Player::new(4_000);
        let patch = Patch::<Basic, 1>::single(params, route);
        assert_eq!(
            player.play(1, &patch),
            [3, 0, 0, 1],
            "unrouted outputs at 0 V"
        );
        assert_eq!(Route::first(2).output(1), Some(CvChannel::Two));
        assert_eq!(Route::first(2).output(2), None);
        assert!(Route::OFF.is_off() && !Route::first(1).is_off());
    }

    #[test]
    fn two_slots_share_the_outputs() {
        let mut patch = Patch::<Basic, 2>::OFF;
        patch.parts[0].params = Params::new(BasicParams::levels([1, 2, 3, 4]));
        patch.parts[0].route = Route::first(2);
        patch.parts[1].params = Params::new(BasicParams::levels([5, 6, 7, 8]));
        patch.parts[1].route.set(0, Some(CvChannel::Three));
        patch.parts[1].route.set(1, Some(CvChannel::Four));
        let mut player = Player::new(4_000);
        assert_eq!(player.play(1, &patch), [1, 2, 5, 6]);
        patch.parts[1].route = Route::OFF;
        assert_eq!(
            player.play(2, &patch),
            [1, 2, 0, 0],
            "an empty slot is silent"
        );
    }

    #[test]
    fn only_the_slots_that_changed_are_prepared_again() {
        let mut patch = Patch::<Basic, 2>::OFF;
        patch.parts[0].params = with(0, tone(Waveform::Sine, 440_000, 1_000));
        let mut follower = Follower::new();
        let first = follower.follow(1, &patch, 1_000).unwrap();
        assert!(
            first.setups.iter().all(Option::is_some),
            "everything at first"
        );
        patch.parts[1].route = Route::first(1);
        let routed = follower.follow(2, &patch, 1_000).unwrap();
        assert!(
            routed.setups.iter().all(Option::is_none),
            "a route is no setup"
        );
        assert_eq!(routed.routes[1], Route::first(1));
        patch.parts[1].params = with(0, tone(Waveform::Saw, 110_000, 500));
        let moved = follower.follow(3, &patch, 1_000).unwrap();
        assert!(moved.setups[0].is_none() && moved.setups[1].is_some());
    }

    #[test]
    fn changes_merge_until_applied() {
        let mut patch = Patch::<Basic, 2>::OFF;
        let mut follower = Follower::new();
        let mut pending = follower.follow(1, &patch, 1_000).unwrap();
        patch.parts[1].route = Route::first(1);
        pending.merge(&follower.follow(2, &patch, 1_000).unwrap());
        assert!(
            pending.setups.iter().all(Option::is_some),
            "the first setups kept"
        );
        assert_eq!(pending.routes[1], Route::first(1), "the newer routes");
        let mut voices = Voices::<Basic, 2>::new(1_000);
        voices.apply(&pending);
        pending.applied();
        assert!(pending.setups.iter().all(Option::is_none));
        assert_eq!(pending.routes[1], Route::first(1));
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
    fn a_follower_prepares_once_per_revision() {
        let patch = Patch::<Basic, 1>::single(
            with(0, tone(Waveform::Sine, 440_000, 1_000)),
            Route::IDENTITY,
        );
        let mut follower = Follower::new();
        assert!(follower.follow(7, &patch, 1_000).is_some(), "the first one");
        assert!(follower.follow(7, &patch, 1_000).is_none(), "same revision");
        assert!(follower.follow(8, &patch, 1_000).is_some(), "moved on");
        follower.forget();
        let again = follower.follow(8, &patch, 1_000).unwrap();
        assert!(again.setups[0].is_some(), "forgotten: prepared again");
    }

    #[test]
    fn a_player_plays_what_a_voice_given_the_params_plays() {
        let mut player = Player::<Basic, 1>::new(47_619);
        let mut voice = BasicVoice::new(47_619);
        for (revision, frequency) in [(1, 110_000), (2, 4_186_000), (3, 220_000)] {
            let params = with(0, tone(Waveform::Sine, frequency, 1_000));
            voice.set_params(&params);
            let patch = Patch::single(params, Route::IDENTITY);
            for _ in 0..500 {
                assert_eq!(player.play(revision, &patch), voice.next_sample());
            }
        }
    }
}
