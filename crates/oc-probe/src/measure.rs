//! What the level 9 checks read off a stream of samples (`TESTING.md` 9.2,
//! 9.3): each input's spread, each trigger's rising edges, an output looped
//! back into an input. Pure functions over samples, so the tests run on the
//! host and the hardware run only adds the cable.

use oc_core::platform::{CV_CHANNELS, MilliVolts, TRIGGER_CHANNELS};
use oc_protocol::Sample;

/// One input over a run: extremes and mean, in calibrated millivolts and in
/// raw ADC codes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct InputSpread {
    /// Lowest reading, mV.
    pub min_mv: MilliVolts,
    /// Highest reading, mV.
    pub max_mv: MilliVolts,
    /// Mean reading, mV, rounded towards zero.
    pub mean_mv: MilliVolts,
    /// Lowest raw code.
    pub min_code: u16,
    /// Highest raw code.
    pub max_code: u16,
}

impl InputSpread {
    /// Peak to peak, mV.
    #[must_use]
    pub const fn peak_to_peak_mv(&self) -> MilliVolts {
        self.max_mv - self.min_mv
    }
}

/// The spread of every input over `samples`; `None` without samples.
#[must_use]
pub fn input_spread(samples: &[Sample]) -> Option<[InputSpread; CV_CHANNELS]> {
    let first = samples.first()?;
    let mut spread = [InputSpread::default(); CV_CHANNELS];
    for (index, input) in spread.iter_mut().enumerate() {
        let mut total = 0_i64;
        *input = InputSpread {
            min_mv: first.cv_in[index],
            max_mv: first.cv_in[index],
            mean_mv: 0,
            min_code: first.cv_codes[index],
            max_code: first.cv_codes[index],
        };
        for sample in samples {
            input.min_mv = input.min_mv.min(sample.cv_in[index]);
            input.max_mv = input.max_mv.max(sample.cv_in[index]);
            input.min_code = input.min_code.min(sample.cv_codes[index]);
            input.max_code = input.max_code.max(sample.cv_codes[index]);
            total += i64::from(sample.cv_in[index]);
        }
        let count = i64::try_from(samples.len()).unwrap_or(i64::MAX);
        input.mean_mv = MilliVolts::try_from(total / count).unwrap_or(0);
    }
    Some(spread)
}

/// What counting edges over a stream found.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct EdgeCount {
    /// Rising edges seen on each trigger.
    pub rising: [u32; TRIGGER_CHANNELS],
    /// Ticks between the first and last sample that no sample covers: a
    /// count is only exact when this is zero.
    pub missed_ticks: u64,
    /// Ticks covered.
    pub ticks: u64,
    /// Shortest, longest and total ticks between two rising edges of each
    /// trigger: a steady clock keeps them within a tick, a bounce shows as
    /// a short one, a missed edge as one twice as long.
    pub intervals: [Intervals; TRIGGER_CHANNELS],
}

/// Ticks between successive rising edges of one trigger.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Intervals {
    /// Intervals measured (one fewer than the edges).
    pub count: u64,
    /// Shortest, ticks.
    pub min: u64,
    /// Longest, ticks.
    pub max: u64,
    /// All of them added, ticks.
    pub total: u64,
}

impl Intervals {
    fn record(&mut self, ticks: u64) {
        if self.count == 0 {
            self.min = ticks;
            self.max = ticks;
        } else {
            self.min = self.min.min(ticks);
            self.max = self.max.max(ticks);
        }
        self.count += 1;
        self.total += ticks;
    }

    /// The mean interval in ticks, `None` with fewer than two edges.
    #[must_use]
    pub fn mean(&self) -> Option<f64> {
        #[allow(clippy::cast_precision_loss)] // tick counts far below 2^52
        (self.count > 0).then(|| self.total as f64 / self.count as f64)
    }
}

/// Counts the rising edges of every trigger over `samples`, which must be
/// one per tick (`stream --every 1`); a gap in the tick numbers means samples
/// were lost, and is reported rather than guessed over.
#[must_use]
pub fn count_edges(samples: &[Sample]) -> EdgeCount {
    let mut count = EdgeCount::default();
    let Some(first) = samples.first() else {
        return count;
    };
    let mut previous = first;
    let mut last_edge: [Option<u64>; TRIGGER_CHANNELS] = [None; TRIGGER_CHANNELS];
    for sample in &samples[1..] {
        let step = sample.tick.saturating_sub(previous.tick);
        count.missed_ticks += step.saturating_sub(1);
        let rose = sample.triggers & !previous.triggers;
        for (index, edges) in count.rising.iter_mut().enumerate() {
            if rose & (1 << index) != 0 {
                *edges += 1;
                if let Some(last) = last_edge[index] {
                    count.intervals[index].record(sample.tick - last);
                }
                last_edge[index] = Some(sample.tick);
            }
        }
        previous = sample;
    }
    count.ticks = previous.tick - first.tick;
    count
}

/// One step of an output looped back into an input.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SweepPoint {
    /// The level held on the output, mV.
    pub out_mv: MilliVolts,
    /// The mean reading of the input, mV.
    pub in_mv: MilliVolts,
    /// The mean raw code of the input.
    pub in_code: u16,
}

impl SweepPoint {
    /// Input minus output, mV: the error of the two paths together.
    #[must_use]
    pub const fn error_mv(&self) -> MilliVolts {
        self.in_mv - self.out_mv
    }
}

/// The point read on `input` while `out_mv` was held, from the samples
/// taken once the level settled.
#[must_use]
pub fn sweep_point(out_mv: MilliVolts, input: usize, settled: &[Sample]) -> Option<SweepPoint> {
    let spread = input_spread(settled)?;
    let count = u64::try_from(settled.len()).ok()?;
    let codes: u64 = settled
        .iter()
        .map(|sample| u64::from(sample.cv_codes[input]))
        .sum();
    Some(SweepPoint {
        out_mv,
        in_mv: spread[input].mean_mv,
        in_code: u16::try_from(codes / count).unwrap_or(u16::MAX),
    })
}

/// The levels of a sweep from `from` to `to` in `step` increments, both ends
/// included; empty when `step` is not positive.
#[must_use]
pub fn sweep_levels(from: MilliVolts, to: MilliVolts, step: MilliVolts) -> Vec<MilliVolts> {
    if step <= 0 {
        return Vec::new();
    }
    let (low, high) = (from.min(to), from.max(to));
    let mut levels = Vec::new();
    let mut level = low;
    while level <= high {
        levels.push(level);
        level += step;
    }
    if levels.last() != Some(&high) {
        levels.push(high);
    }
    if from > to {
        levels.reverse();
    }
    levels
}

#[cfg(test)]
mod tests {
    use super::{count_edges, input_spread, sweep_levels, sweep_point};
    use oc_protocol::Sample;

    fn sample(tick: u64, cv1: i32, triggers: u8) -> Sample {
        Sample {
            tick,
            cv_in: [cv1, 0, 0, 0],
            cv_codes: [2_730, 2_730, 2_730, 2_730],
            cv_out: [0; 4],
            triggers,
        }
    }

    #[test]
    fn the_spread_gives_extremes_and_mean() {
        let samples = [sample(1, -10, 0), sample(2, 4, 0), sample(3, 12, 0)];
        let spread = input_spread(&samples).unwrap();
        assert_eq!(
            (spread[0].min_mv, spread[0].max_mv, spread[0].mean_mv),
            (-10, 12, 2)
        );
        assert_eq!(spread[0].peak_to_peak_mv(), 22);
        assert!(input_spread(&[]).is_none());
    }

    #[test]
    fn rising_edges_are_counted_per_trigger() {
        let samples = [
            sample(1, 0, 0b0000),
            sample(2, 0, 0b0001),
            sample(3, 0, 0b0011),
            sample(4, 0, 0b0000),
            sample(5, 0, 0b0001),
        ];
        let count = count_edges(&samples);
        assert_eq!(count.rising, [2, 1, 0, 0]);
        assert_eq!((count.intervals[0].min, count.intervals[0].max), (3, 3));
        assert_eq!(count.intervals[1].count, 0);
        assert_eq!((count.missed_ticks, count.ticks), (0, 4));
    }

    #[test]
    fn lost_samples_are_reported() {
        let samples = [sample(10, 0, 0), sample(11, 0, 1), sample(15, 0, 0)];
        assert_eq!(count_edges(&samples).missed_ticks, 3);
    }

    #[test]
    fn a_sweep_includes_both_ends_in_either_direction() {
        assert_eq!(sweep_levels(-3_000, 0, 1_000), [-3_000, -2_000, -1_000, 0]);
        assert_eq!(sweep_levels(0, 2_500, 1_000), [0, 1_000, 2_000, 2_500]);
        assert_eq!(sweep_levels(1_000, -1_000, 1_000), [1_000, 0, -1_000]);
        assert_eq!(sweep_levels(0, 1_000, 0), Vec::<i32>::new());
    }

    #[test]
    fn a_sweep_point_averages_the_settled_samples() {
        let settled = [sample(1, 990, 0), sample(2, 1_010, 0)];
        let point = sweep_point(1_000, 0, &settled).unwrap();
        assert_eq!(
            (point.in_mv, point.error_mv(), point.in_code),
            (1_000, 0, 2_730)
        );
    }
}
