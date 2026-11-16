//! Whether an input carries something, inferred from the signal alone.
//!
//! The module cannot detect a cable (its jacks have no switch wired to the
//! microcontroller), and a host that could, such as VCV Rack, must not tell
//! the core either, or the same applet would behave differently on each
//! backend (journal/D-019). An applet that wants a default value for an
//! unused input therefore asks this module, which decides from the levels
//! it is fed and nothing else, identically everywhere (journal/D-024).
//!
//! There is one mechanism, *recent activity with hold*: an input is present
//! while it has shown evidence of a signal within the last hold period. The
//! settings form a closed set; applets pick one, they do not invent their
//! own:
//!
//! | Type                         | Evidence                                    | Hold     |
//! |------------------------------|---------------------------------------------|----------|
//! | [`CvPresence`]               | activity, or a level away from 0 V          | 300 ms   |
//! | [`TriggerPresence::latch`]   | a rising edge                               | until [`TriggerPresence::reset`] |
//! | [`TriggerPresence::clock`]   | a rising edge                               | 2 s      |
//!
//! Time is counted in elapsed microseconds, not ticks, so the durations hold
//! whatever the signal rate.
//!
//! The known blind spot: a cable sitting at exactly 0 V (within
//! [`CV_LEVEL_THRESHOLD_MV`]) reads as absent. Applets should pick defaults
//! close to what 0 V would give, and show the presence state on screen.

use crate::debounce::Edge;
use crate::platform::MilliVolts;
use crate::signal::{DEFAULT_ACTIVITY_THRESHOLD_MV, SignalDetector};

/// A level further than this from 0 V counts as evidence on a CV input.
///
/// Above the offset error expected from the provisional calibration, and
/// musically negligible (a little over a semitone at 1 V/oct).
pub const CV_LEVEL_THRESHOLD_MV: MilliVolts = 100;

/// How long a CV input stays present without evidence. Long enough that an
/// LFO crossing 0 V does not flicker absent.
pub const CV_HOLD_MICROS: u32 = 300_000;

/// How long a clock input stays present after its last rising edge.
pub const CLOCK_HOLD_MICROS: u32 = 2_000_000;

/// Whether an input currently carries something.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Presence {
    /// No evidence within the hold period: use the applet's default.
    #[default]
    Absent,
    /// Evidence of a signal within the hold period.
    Present,
}

impl Presence {
    /// Whether the input is present.
    #[must_use]
    pub const fn is_present(self) -> bool {
        matches!(self, Self::Present)
    }
}

/// How long evidence lasts. Private: the public types fix it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Hold {
    /// Present for this many microseconds after the last evidence.
    For(u32),
    /// Present from the first evidence until reset.
    Latch,
}

/// The shared mechanism: recent evidence, held for a while.
#[derive(Debug, Clone, Copy)]
struct HeldEvidence {
    hold: Hold,
    remaining_micros: u32,
    presence: Presence,
}

impl HeldEvidence {
    const fn new(hold: Hold) -> Self {
        Self {
            hold,
            remaining_micros: 0,
            presence: Presence::Absent,
        }
    }

    fn update(&mut self, evidence: bool, elapsed_micros: u32) -> Presence {
        match self.hold {
            Hold::Latch => {
                if evidence {
                    self.presence = Presence::Present;
                }
            }
            Hold::For(hold) => {
                if evidence {
                    self.remaining_micros = hold;
                    self.presence = Presence::Present;
                } else {
                    self.remaining_micros = self.remaining_micros.saturating_sub(elapsed_micros);
                    if self.remaining_micros == 0 {
                        self.presence = Presence::Absent;
                    }
                }
            }
        }
        self.presence
    }

    const fn reset(&mut self) {
        self.remaining_micros = 0;
        self.presence = Presence::Absent;
    }
}

/// Presence of one CV input, with the activity detector it is built on.
#[derive(Debug, Clone, Copy)]
pub struct CvPresence {
    signal: SignalDetector,
    held: HeldEvidence,
}

impl CvPresence {
    /// A CV input with no evidence yet.
    #[must_use]
    pub fn new() -> Self {
        Self {
            signal: SignalDetector::default(),
            held: HeldEvidence::new(Hold::For(CV_HOLD_MICROS)),
        }
    }

    /// Feeds one sample, `elapsed_micros` after the previous one.
    pub fn update(&mut self, millivolts: MilliVolts, elapsed_micros: u32) -> Presence {
        self.signal.update(millivolts);
        let evidence = self.signal.is_active(DEFAULT_ACTIVITY_THRESHOLD_MV)
            || millivolts.unsigned_abs() > CV_LEVEL_THRESHOLD_MV.unsigned_abs();
        self.held.update(evidence, elapsed_micros)
    }

    /// Current presence.
    #[must_use]
    pub const fn presence(&self) -> Presence {
        self.held.presence
    }

    /// The activity detector the presence is derived from (level, span).
    #[must_use]
    pub const fn signal(&self) -> &SignalDetector {
        &self.signal
    }
}

impl Default for CvPresence {
    fn default() -> Self {
        Self::new()
    }
}

/// Presence of one trigger or clock input.
#[derive(Debug, Clone, Copy)]
pub struct TriggerPresence {
    held: HeldEvidence,
}

impl TriggerPresence {
    /// Present from the first rising edge until [`Self::reset`]: "has this
    /// input ever been clocked?" (the Phazerville idiom).
    #[must_use]
    pub const fn latch() -> Self {
        Self {
            held: HeldEvidence::new(Hold::Latch),
        }
    }

    /// Present for [`CLOCK_HOLD_MICROS`] after each rising edge: a stopped
    /// clock becomes absent again.
    #[must_use]
    pub const fn clock() -> Self {
        Self {
            held: HeldEvidence::new(Hold::For(CLOCK_HOLD_MICROS)),
        }
    }

    /// Feeds the edge (if any) seen this tick, `elapsed_micros` after the
    /// previous one.
    pub fn update(&mut self, edge: Option<Edge>, elapsed_micros: u32) -> Presence {
        self.held.update(edge == Some(Edge::Rising), elapsed_micros)
    }

    /// Current presence.
    #[must_use]
    pub const fn presence(&self) -> Presence {
        self.held.presence
    }

    /// Back to absent, e.g. when the applet is reset.
    pub const fn reset(&mut self) {
        self.held.reset();
    }
}

#[cfg(test)]
mod tests {
    use super::{
        CLOCK_HOLD_MICROS, CV_HOLD_MICROS, CV_LEVEL_THRESHOLD_MV, CvPresence, Presence,
        TriggerPresence,
    };
    use crate::debounce::Edge;

    const TICK: u32 = 1_000;

    fn feed(presence: &mut CvPresence, millivolts: i32, ticks: u32) -> Presence {
        let mut last = presence.presence();
        for _ in 0..ticks {
            last = presence.update(millivolts, TICK);
        }
        last
    }

    #[test]
    fn an_input_starts_absent() {
        assert_eq!(CvPresence::new().presence(), Presence::Absent);
        assert_eq!(TriggerPresence::latch().presence(), Presence::Absent);
        assert_eq!(TriggerPresence::clock().presence(), Presence::Absent);
    }

    #[test]
    fn a_static_level_away_from_zero_is_present_at_once() {
        let mut presence = CvPresence::new();
        assert_eq!(presence.update(2_000, TICK), Presence::Present);
        assert_eq!(presence.update(-2_000, TICK), Presence::Present);
    }

    #[test]
    fn a_static_level_near_zero_stays_absent() {
        let mut presence = CvPresence::new();
        assert_eq!(
            feed(&mut presence, CV_LEVEL_THRESHOLD_MV, 2_000),
            Presence::Absent
        );
        assert_eq!(feed(&mut presence, -12, 2_000), Presence::Absent);
    }

    #[test]
    fn a_slow_lfo_does_not_flicker_absent_at_its_zero_crossings() {
        // A ±5 V triangle at 0.5 Hz, sampled at 1 kHz for five periods.
        let mut presence = CvPresence::new();
        presence.update(5_000, TICK);
        for tick in 0..10_000i32 {
            let phase = tick % 2_000;
            let level = if phase < 1_000 {
                5_000 - phase * 10
            } else {
                -5_000 + (phase - 1_000) * 10
            };
            assert_eq!(
                presence.update(level, TICK),
                Presence::Present,
                "absent at tick {tick}, level {level} mV"
            );
        }
    }

    #[test]
    fn a_removed_source_becomes_absent_after_the_hold() {
        let mut presence = CvPresence::new();
        feed(&mut presence, 3_000, 10);
        // The step down to 0 V shows in the activity block for up to one
        // window, then the hold runs out.
        let ticks_to_absent = (0..2_000u32)
            .find(|_| presence.update(0, TICK) == Presence::Absent)
            .expect("an unplugged input must eventually read absent");
        let window = u32::from(crate::signal::DEFAULT_WINDOW);
        assert!(
            ticks_to_absent >= CV_HOLD_MICROS / TICK - 1,
            "absent after {ticks_to_absent} ticks, before the hold ran out"
        );
        assert!(
            ticks_to_absent <= CV_HOLD_MICROS / TICK + 2 * window,
            "absent only after {ticks_to_absent} ticks"
        );
    }

    #[test]
    fn a_latched_trigger_stays_present_until_reset() {
        let mut presence = TriggerPresence::latch();
        assert_eq!(presence.update(None, TICK), Presence::Absent);
        assert_eq!(presence.update(Some(Edge::Falling), TICK), Presence::Absent);
        assert_eq!(presence.update(Some(Edge::Rising), TICK), Presence::Present);
        for _ in 0..100_000 {
            presence.update(None, TICK);
        }
        assert_eq!(presence.presence(), Presence::Present);
        presence.reset();
        assert_eq!(presence.presence(), Presence::Absent);
    }

    #[test]
    fn a_stopped_clock_becomes_absent_after_its_hold() {
        let mut presence = TriggerPresence::clock();
        presence.update(Some(Edge::Rising), TICK);
        let hold_ticks = CLOCK_HOLD_MICROS / TICK;
        for _ in 0..hold_ticks - 1 {
            assert_eq!(presence.update(None, TICK), Presence::Present);
        }
        assert_eq!(presence.update(None, TICK), Presence::Absent);
    }

    #[test]
    fn the_hold_is_measured_in_time_not_ticks() {
        // The same 2 s hold at a 60 us signal period (journal/D-023).
        let mut presence = TriggerPresence::clock();
        presence.update(Some(Edge::Rising), 60);
        let ticks = CLOCK_HOLD_MICROS.div_ceil(60);
        for _ in 0..ticks - 1 {
            assert_eq!(presence.update(None, 60), Presence::Present);
        }
        assert_eq!(presence.update(None, 60), Presence::Absent);
    }
}
