//! Detection of a live signal on a CV input.
//!
//! The real Ornament & Crime has no cable-detection switch on its jacks, so
//! "is something happening on this input?" has to be inferred. The detector
//! measures the peak-to-peak excursion over a sliding block of samples: a
//! patched but static input shows a small span, an unpatched one shows only
//! converter noise, and a modulated one stands out clearly.
//!
//! Blocks rather than a true sliding window keep the state to a handful of
//! integers per channel, which matters when this runs at 1 kHz on four
//! channels.

use crate::platform::MilliVolts;

/// Samples per measurement block; about a quarter of a second at 1 kHz.
pub const DEFAULT_WINDOW: u16 = 256;

/// Peak-to-peak excursion above which an input counts as active, in
/// millivolts. Comfortably above the few-millivolt noise floor of a 12-bit
/// converter over a 10 V span.
pub const DEFAULT_ACTIVITY_THRESHOLD_MV: MilliVolts = 40;

/// Tracks the excursion of one CV input.
#[derive(Debug, Clone, Copy)]
pub struct SignalDetector {
    window: u16,
    seen: u16,
    block_min: MilliVolts,
    block_max: MilliVolts,
    span: MilliVolts,
    latest: MilliVolts,
}

impl SignalDetector {
    /// A detector measuring over blocks of `window` samples. A window of zero
    /// behaves as one, i.e. the span is recomputed from every sample.
    #[must_use]
    pub const fn new(window: u16) -> Self {
        Self {
            window: if window == 0 { 1 } else { window },
            seen: 0,
            block_min: MilliVolts::MAX,
            block_max: MilliVolts::MIN,
            span: 0,
            latest: 0,
        }
    }

    /// Feeds one sample and returns the most recently published span.
    pub const fn update(&mut self, millivolts: MilliVolts) -> MilliVolts {
        self.latest = millivolts;

        if millivolts < self.block_min {
            self.block_min = millivolts;
        }
        if millivolts > self.block_max {
            self.block_max = millivolts;
        }

        self.seen += 1;
        if self.seen >= self.window {
            self.span = self.block_max.saturating_sub(self.block_min);
            self.seen = 0;
            self.block_min = MilliVolts::MAX;
            self.block_max = MilliVolts::MIN;
        }

        self.span
    }

    /// Most recent sample.
    #[must_use]
    pub const fn level(&self) -> MilliVolts {
        self.latest
    }

    /// Peak-to-peak excursion measured over the last complete block.
    #[must_use]
    pub const fn span(&self) -> MilliVolts {
        self.span
    }

    /// Whether the last complete block moved by at least `threshold`.
    #[must_use]
    pub const fn is_active(&self, threshold: MilliVolts) -> bool {
        self.span >= threshold
    }

    /// Discards the current block and the published span.
    pub const fn reset(&mut self) {
        self.seen = 0;
        self.block_min = MilliVolts::MAX;
        self.block_max = MilliVolts::MIN;
        self.span = 0;
    }
}

impl Default for SignalDetector {
    fn default() -> Self {
        Self::new(DEFAULT_WINDOW)
    }
}

#[cfg(test)]
mod tests {
    use super::{DEFAULT_ACTIVITY_THRESHOLD_MV, SignalDetector};

    #[test]
    fn a_static_input_is_not_active() {
        let mut detector = SignalDetector::new(8);
        for _ in 0..32 {
            detector.update(1_234);
        }
        assert_eq!(detector.span(), 0);
        assert!(!detector.is_active(DEFAULT_ACTIVITY_THRESHOLD_MV));
        assert_eq!(detector.level(), 1_234);
    }

    #[test]
    fn a_modulated_input_is_active() {
        let mut detector = SignalDetector::new(4);
        for step in 0..16 {
            detector.update(if step % 2 == 0 { -2_000 } else { 2_000 });
        }
        assert_eq!(detector.span(), 4_000);
        assert!(detector.is_active(DEFAULT_ACTIVITY_THRESHOLD_MV));
    }

    #[test]
    fn the_span_is_only_published_at_block_boundaries() {
        let mut detector = SignalDetector::new(4);
        assert_eq!(detector.update(0), 0);
        assert_eq!(detector.update(1_000), 0);
        assert_eq!(detector.update(0), 0);
        assert_eq!(detector.update(0), 1_000);
    }

    #[test]
    fn noise_below_the_threshold_stays_inactive() {
        let mut detector = SignalDetector::new(4);
        for step in 0..8 {
            detector.update(if step % 2 == 0 { -5 } else { 5 });
        }
        assert_eq!(detector.span(), 10);
        assert!(!detector.is_active(DEFAULT_ACTIVITY_THRESHOLD_MV));
    }

    #[test]
    fn extreme_samples_do_not_overflow_the_span() {
        let mut detector = SignalDetector::new(2);
        detector.update(i32::MIN);
        detector.update(i32::MAX);
        assert_eq!(detector.span(), i32::MAX);
    }

    #[test]
    fn a_zero_length_window_publishes_every_sample() {
        let mut detector = SignalDetector::new(0);
        assert_eq!(detector.update(500), 0);
        assert_eq!(detector.update(900), 0);
    }

    #[test]
    fn resetting_forgets_the_previous_block() {
        let mut detector = SignalDetector::new(2);
        detector.update(-1_000);
        detector.update(1_000);
        assert_eq!(detector.span(), 2_000);
        detector.reset();
        assert_eq!(detector.span(), 0);
    }
}
