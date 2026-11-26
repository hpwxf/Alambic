//! The rate of a clock on a trigger input, as `RATCHET` shows it and syncs
//! to (journal/D-032) and `MELODY` sizes its gate by (journal/D-047): the
//! mean of the last few intervals between rising edges, one trigger counted
//! as one beat. Shared here rather than imported from one applet by another.

/// Intervals averaged.
const INTERVALS: usize = 4;

/// Longest interval still taken as a tempo: past it, the rate is unknown
/// until two new edges come.
pub const MAX_PERIOD_MICROS: u32 = 2_000_000;

/// The mean period of the last edges.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Tempo {
    intervals: [u32; INTERVALS],
    /// Intervals held, at most [`INTERVALS`].
    held: usize,
    /// Next slot to write.
    next: usize,
    /// Time since the last edge, `None` before the first.
    since_micros: Option<u32>,
}

impl Tempo {
    /// No edge seen.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            intervals: [0; INTERVALS],
            held: 0,
            next: 0,
            since_micros: None,
        }
    }

    /// Lets `elapsed` microseconds pass; past [`MAX_PERIOD_MICROS`] without
    /// an edge, the rate is forgotten.
    pub fn advance(&mut self, elapsed: u32) {
        if let Some(since) = self.since_micros {
            let since = since.saturating_add(elapsed);
            if since > MAX_PERIOD_MICROS {
                *self = Self::new();
            } else {
                self.since_micros = Some(since);
            }
        }
    }

    /// Records a rising edge now.
    pub fn edge(&mut self) {
        if let Some(interval) = self.since_micros.filter(|&since| since > 0) {
            self.intervals[self.next] = interval;
            self.next = (self.next + 1) % INTERVALS;
            self.held = (self.held + 1).min(INTERVALS);
        }
        self.since_micros = Some(0);
    }

    /// The mean period, `None` before two edges or after a long silence.
    #[must_use]
    pub fn period_micros(&self) -> Option<u32> {
        if self.held == 0 {
            return None;
        }
        let total: u64 = self.intervals[..self.held]
            .iter()
            .map(|&interval| u64::from(interval))
            .sum();
        let count = u64::try_from(self.held).unwrap_or(1);
        u32::try_from(total / count).ok()
    }

    /// The rate in beats per minute, rounded, `None` when unknown.
    #[must_use]
    pub fn bpm(&self) -> Option<u32> {
        self.period_micros()
            .filter(|&period| period > 0)
            .map(|period| (60_000_000 + period / 2) / period)
    }
}

impl Default for Tempo {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::{MAX_PERIOD_MICROS, Tempo};

    /// Edges every `period` microseconds, `count` of them, ticked by 1 ms.
    fn steady(tempo: &mut Tempo, period: u32, count: u32) {
        for _ in 0..count {
            tempo.edge();
            for _ in 0..period / 1_000 {
                tempo.advance(1_000);
            }
        }
    }

    #[test]
    fn a_steady_clock_reads_as_its_bpm() {
        let mut tempo = Tempo::new();
        assert_eq!(tempo.bpm(), None);
        tempo.edge();
        assert_eq!(tempo.bpm(), None, "one edge is not a rate");
        let mut tempo = Tempo::new();
        steady(&mut tempo, 500_000, 6);
        assert_eq!(tempo.period_micros(), Some(500_000));
        assert_eq!(tempo.bpm(), Some(120));
    }

    #[test]
    fn the_last_four_intervals_are_averaged() {
        let mut tempo = Tempo::new();
        steady(&mut tempo, 400_000, 5);
        steady(&mut tempo, 600_000, 2);
        tempo.edge();
        // 400, 400, 600, 600.
        assert_eq!(tempo.period_micros(), Some(500_000));
    }

    #[test]
    fn a_long_silence_forgets_the_rate() {
        let mut tempo = Tempo::new();
        steady(&mut tempo, 500_000, 4);
        tempo.advance(MAX_PERIOD_MICROS);
        assert_eq!(tempo.bpm(), None);
        tempo.edge();
        assert_eq!(tempo.bpm(), None);
    }
}
