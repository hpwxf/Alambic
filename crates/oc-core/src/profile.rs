//! Where the time of a tick goes, measured on the clock's fine counter
//! (journal/D-029).
//!
//! The engine times its own stages ([`Stage::Tick`] down to
//! [`Stage::Present`]) and the period between two ticks; a backend adds what
//! happens outside the engine ([`Stage::Sample`], [`Stage::Usb`]) or below it
//! ([`Stage::Page`], one display page sent) through
//! [`crate::Engine::profile_mut`]. On the module the fine counter is the
//! core's cycle counter, so a stage costs a few cycles to time and is known
//! to 1.7 ns; a clock without a fine counter leaves the profile empty.
//!
//! Durations are kept in fine counts and converted on the way out
//! ([`Profile::nanos`]), so recording stays a subtraction and a few
//! comparisons.

use serde::{Deserialize, Serialize};

/// A timed part of the loop.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Stage {
    /// A whole [`crate::Engine::tick`].
    Tick,
    /// Reading the panel and the inputs.
    Acquire,
    /// The applet, the wizard or the menu deciding the outputs.
    Update,
    /// Converting and writing the outputs.
    Outputs,
    /// Drawing the frame, on the ticks that redraw.
    Render,
    /// Sending the frame to the display.
    Present,
    /// Sending one display page (backend).
    Page,
    /// Sampling the inputs before the tick (backend).
    Sample,
    /// Serving USB (backend).
    Usb,
}

impl Stage {
    /// Every stage, in report order.
    pub const ALL: [Self; 9] = [
        Self::Tick,
        Self::Acquire,
        Self::Update,
        Self::Outputs,
        Self::Render,
        Self::Present,
        Self::Page,
        Self::Sample,
        Self::Usb,
    ];

    /// Number of stages.
    pub const COUNT: usize = Self::ALL.len();

    /// Position in [`Self::ALL`].
    #[must_use]
    pub const fn index(self) -> usize {
        self as usize
    }

    /// Short lowercase name, for logs and the host tool.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Tick => "tick",
            Self::Acquire => "acquire",
            Self::Update => "update",
            Self::Outputs => "outputs",
            Self::Render => "render",
            Self::Present => "present",
            Self::Page => "page",
            Self::Sample => "sample",
            Self::Usb => "usb",
        }
    }
}

/// Count, extremes and total of a series of durations, in fine counts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Stat {
    count: u32,
    min: u32,
    max: u32,
    total: u64,
}

impl Stat {
    /// No sample yet.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            count: 0,
            min: 0,
            max: 0,
            total: 0,
        }
    }

    /// Adds one duration. Saturates rather than wraps: a statistic left
    /// running for weeks must not turn into nonsense (journal/D-018).
    pub fn record(&mut self, value: u32) {
        if self.count == 0 {
            self.min = value;
            self.max = value;
        } else {
            self.min = self.min.min(value);
            self.max = self.max.max(value);
        }
        self.count = self.count.saturating_add(1);
        self.total = self.total.saturating_add(u64::from(value));
    }

    /// Number of durations recorded.
    #[must_use]
    pub const fn count(&self) -> u32 {
        self.count
    }

    /// Shortest duration, `None` before the first.
    #[must_use]
    pub const fn min(&self) -> Option<u32> {
        if self.count == 0 {
            None
        } else {
            Some(self.min)
        }
    }

    /// Longest duration, `None` before the first.
    #[must_use]
    pub const fn max(&self) -> Option<u32> {
        if self.count == 0 {
            None
        } else {
            Some(self.max)
        }
    }

    /// Mean duration, rounded down, `None` before the first.
    #[must_use]
    pub fn mean(&self) -> Option<u32> {
        if self.count == 0 {
            return None;
        }
        u32::try_from(self.total / u64::from(self.count)).ok()
    }
}

/// Buckets of the period histogram ([`Profile::jitter`]).
pub const JITTER_BUCKETS: usize = 16;

/// Bucket of a deviation of `micros` from the expected period: 0 below one
/// microsecond, then `k` for `[2^(k-1), 2^k)`, the last one open-ended.
const fn jitter_bucket(micros: u32) -> usize {
    let bucket = micros.bit_width() as usize;
    if bucket < JITTER_BUCKETS {
        bucket
    } else {
        JITTER_BUCKETS - 1
    }
}

/// The time of each stage and the loop period since the last reset.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Profile {
    fine_per_micro: u32,
    stages: [Stat; Stage::COUNT],
    period: Stat,
    expected_period_micros: u32,
    jitter: [u32; JITTER_BUCKETS],
}

impl Profile {
    /// An empty profile for a fine counter running `fine_per_micro` times a
    /// microsecond; zero means there is no fine counter and nothing is
    /// recorded.
    #[must_use]
    pub const fn new(fine_per_micro: u32) -> Self {
        Self {
            fine_per_micro,
            stages: [Stat::new(); Stage::COUNT],
            period: Stat::new(),
            expected_period_micros: 0,
            jitter: [0; JITTER_BUCKETS],
        }
    }

    /// Whether durations are recorded at all.
    #[must_use]
    pub const fn is_enabled(&self) -> bool {
        self.fine_per_micro > 0
    }

    /// Fine counts per microsecond.
    #[must_use]
    pub const fn fine_per_micro(&self) -> u32 {
        self.fine_per_micro
    }

    /// Records one duration of `stage`.
    pub fn record(&mut self, stage: Stage, fine: u32) {
        if self.is_enabled() {
            self.stages[stage.index()].record(fine);
        }
    }

    /// The durations of `stage`.
    #[must_use]
    pub const fn stage(&self, stage: Stage) -> &Stat {
        &self.stages[stage.index()]
    }

    /// The period the loop aims at, which the jitter is measured against;
    /// zero (the default) leaves the histogram empty.
    pub const fn set_expected_period_micros(&mut self, micros: u32) {
        self.expected_period_micros = micros;
    }

    /// The period the loop aims at.
    #[must_use]
    pub const fn expected_period_micros(&self) -> u32 {
        self.expected_period_micros
    }

    /// Records the time between two tick starts.
    pub fn record_period(&mut self, fine: u32) {
        if !self.is_enabled() {
            return;
        }
        self.period.record(fine);
        if self.expected_period_micros > 0 {
            let micros = fine / self.fine_per_micro;
            let deviation = micros.abs_diff(self.expected_period_micros);
            self.jitter[jitter_bucket(deviation)] += 1;
        }
    }

    /// The times between two tick starts.
    #[must_use]
    pub const fn period(&self) -> &Stat {
        &self.period
    }

    /// How far each period was from the expected one: bucket 0 counts the
    /// periods within a microsecond, bucket `k` those `2^(k-1)` to `2^k`
    /// microseconds off, early or late; the last bucket takes the rest.
    #[must_use]
    pub const fn jitter(&self) -> &[u32; JITTER_BUCKETS] {
        &self.jitter
    }

    /// Forgets every duration, keeping the counter rate and the expected
    /// period.
    pub const fn clear(&mut self) {
        *self = Self {
            expected_period_micros: self.expected_period_micros,
            ..Self::new(self.fine_per_micro)
        };
    }

    /// `fine` counts in nanoseconds; zero without a fine counter.
    #[must_use]
    pub fn nanos(&self, fine: u32) -> u64 {
        if self.fine_per_micro == 0 {
            return 0;
        }
        u64::from(fine) * 1_000 / u64::from(self.fine_per_micro)
    }
}

#[cfg(test)]
mod tests {
    use super::{JITTER_BUCKETS, Profile, Stage, Stat, jitter_bucket};

    #[test]
    fn a_stat_tracks_extremes_and_mean() {
        let mut stat = Stat::new();
        assert_eq!((stat.min(), stat.max(), stat.mean()), (None, None, None));
        for value in [30, 10, 20] {
            stat.record(value);
        }
        assert_eq!(stat.count(), 3);
        assert_eq!(
            (stat.min(), stat.max(), stat.mean()),
            (Some(10), Some(30), Some(20))
        );
    }

    #[test]
    fn a_stat_saturates_instead_of_wrapping() {
        let mut stat = Stat::new();
        for _ in 0..3 {
            stat.record(u32::MAX);
        }
        assert_eq!(stat.mean(), Some(u32::MAX));
    }

    #[test]
    fn jitter_buckets_double_and_end_open() {
        assert_eq!(jitter_bucket(0), 0);
        assert_eq!(jitter_bucket(1), 1);
        assert_eq!((jitter_bucket(2), jitter_bucket(3)), (2, 2));
        assert_eq!((jitter_bucket(4), jitter_bucket(7)), (3, 3));
        assert_eq!(jitter_bucket(u32::MAX), JITTER_BUCKETS - 1);
    }

    #[test]
    fn periods_land_in_the_bucket_of_their_deviation() {
        let mut profile = Profile::new(600);
        profile.set_expected_period_micros(1_000);
        profile.record_period(600 * 1_000);
        profile.record_period(600 * 1_003);
        profile.record_period(600 * 990);
        assert_eq!(profile.jitter()[0], 1);
        assert_eq!(profile.jitter()[2], 1, "3 us late");
        assert_eq!(profile.jitter()[4], 1, "10 us early");
        assert_eq!(profile.period().max(), Some(600 * 1_003));
    }

    #[test]
    fn without_a_fine_counter_nothing_is_recorded() {
        let mut profile = Profile::new(0);
        profile.set_expected_period_micros(1_000);
        profile.record(Stage::Tick, 5);
        profile.record_period(1_000);
        assert_eq!(profile.stage(Stage::Tick).count(), 0);
        assert_eq!(profile.period().count(), 0);
        assert_eq!(profile.nanos(5), 0);
    }

    #[test]
    fn clearing_keeps_the_rate_and_the_target() {
        let mut profile = Profile::new(600);
        profile.set_expected_period_micros(1_000);
        profile.record(Stage::Page, 78_600);
        profile.clear();
        assert_eq!(profile.stage(Stage::Page).count(), 0);
        assert_eq!(profile.expected_period_micros(), 1_000);
        assert_eq!(profile.nanos(78_600), 131_000);
    }

    #[test]
    fn stages_index_their_report_order() {
        for (position, stage) in Stage::ALL.into_iter().enumerate() {
            assert_eq!(stage.index(), position, "{}", stage.name());
        }
    }
}
