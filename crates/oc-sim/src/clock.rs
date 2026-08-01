//! Time policy of the simulator.
//!
//! The engine always sees a virtual clock that advances by exactly one tick
//! period, whatever the wall-clock pace. That is what makes a scenario replay
//! bit-for-bit reproducible: the speed control only decides *how many* ticks to
//! run, never *how long* a tick appears to take.

use std::time::Duration;

/// Nominal tick period of the firmware loop, in microseconds.
pub const TICK_MICROS: u64 = 1_000;

/// How the simulator decides when to run a tick.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Speed {
    /// No tick runs unless one is explicitly requested.
    #[default]
    Paused,
    /// One virtual millisecond per real millisecond.
    Realtime,
    /// Real time multiplied by a factor, to fast-forward long scenarios.
    Turbo(u32),
}

impl Speed {
    /// Short label for the status bar.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Paused => "PAUSED",
            Self::Realtime => "x1",
            Self::Turbo(_) => "TURBO",
        }
    }

    /// Number of ticks owed for `elapsed` of real time.
    #[must_use]
    pub fn ticks_for(self, elapsed: Duration) -> u64 {
        let factor = match self {
            Self::Paused => return 0,
            Self::Realtime => 1,
            Self::Turbo(factor) => u64::from(factor.max(1)),
        };
        let elapsed_micros = u64::try_from(elapsed.as_micros()).unwrap_or(u64::MAX);
        elapsed_micros.saturating_mul(factor) / TICK_MICROS
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::{Speed, TICK_MICROS};

    #[test]
    fn a_paused_simulator_owes_no_ticks() {
        assert_eq!(Speed::Paused.ticks_for(Duration::from_secs(10)), 0);
    }

    #[test]
    fn realtime_runs_one_tick_per_period() {
        let elapsed = Duration::from_micros(TICK_MICROS * 7);
        assert_eq!(Speed::Realtime.ticks_for(elapsed), 7);
    }

    #[test]
    fn a_partial_period_owes_nothing_yet() {
        let elapsed = Duration::from_micros(TICK_MICROS - 1);
        assert_eq!(Speed::Realtime.ticks_for(elapsed), 0);
    }

    #[test]
    fn turbo_multiplies_the_tick_count() {
        let elapsed = Duration::from_micros(TICK_MICROS * 3);
        assert_eq!(Speed::Turbo(50).ticks_for(elapsed), 150);
    }

    #[test]
    fn a_zero_turbo_factor_degrades_to_realtime() {
        let elapsed = Duration::from_micros(TICK_MICROS * 4);
        assert_eq!(Speed::Turbo(0).ticks_for(elapsed), 4);
    }

    #[test]
    fn an_absurd_elapsed_time_does_not_overflow() {
        assert!(Speed::Turbo(u32::MAX).ticks_for(Duration::MAX) > 0);
    }
}
