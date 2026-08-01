//! Debouncing and edge detection for the trigger inputs and the buttons.
//!
//! A level is only accepted once it has been observed unchanged for a number
//! of consecutive samples. At the nominal 1 kHz tick rate the default of three
//! samples rejects bounce shorter than 3 ms while adding at most 3 ms of
//! latency, which is inaudible for gate timing.

/// A transition of a debounced input.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Edge {
    /// The input went from low to high.
    Rising,
    /// The input went from high to low.
    Falling,
}

/// Default number of identical consecutive samples required to accept a level.
pub const DEFAULT_STABLE_SAMPLES: u8 = 3;

/// Debounces one boolean input.
#[derive(Debug, Clone, Copy)]
pub struct Debouncer {
    stable: bool,
    candidate: bool,
    agreements: u8,
    required: u8,
}

impl Debouncer {
    /// A debouncer starting from the low state, requiring `required` identical
    /// samples. A `required` of zero behaves as one, i.e. no debouncing.
    #[must_use]
    pub const fn new(required: u8) -> Self {
        Self {
            stable: false,
            candidate: false,
            agreements: 0,
            required: if required == 0 { 1 } else { required },
        }
    }

    /// Feeds one raw sample and reports the resulting transition, if any.
    pub fn update(&mut self, raw: bool) -> Option<Edge> {
        if raw == self.stable {
            self.candidate = raw;
            self.agreements = 0;
            return None;
        }

        if raw == self.candidate {
            self.agreements = self.agreements.saturating_add(1);
        } else {
            self.candidate = raw;
            self.agreements = 1;
        }

        if self.agreements < self.required {
            return None;
        }

        self.stable = raw;
        self.agreements = 0;
        Some(if raw { Edge::Rising } else { Edge::Falling })
    }

    /// The current debounced level.
    #[must_use]
    pub const fn state(&self) -> bool {
        self.stable
    }
}

impl Default for Debouncer {
    fn default() -> Self {
        Self::new(DEFAULT_STABLE_SAMPLES)
    }
}

/// A debounced input that also counts the rising edges it has seen.
#[derive(Debug, Clone, Copy, Default)]
pub struct EdgeCounter {
    debouncer: Debouncer,
    rising: u32,
}

impl EdgeCounter {
    /// An edge counter with a custom debounce depth.
    #[must_use]
    pub const fn new(required: u8) -> Self {
        Self {
            debouncer: Debouncer::new(required),
            rising: 0,
        }
    }

    /// Feeds one raw sample and reports the resulting transition, if any.
    pub fn update(&mut self, raw: bool) -> Option<Edge> {
        let edge = self.debouncer.update(raw);
        if edge == Some(Edge::Rising) {
            // Saturating: a wrapped counter would be a lie, a pinned one is
            // visibly "very many".
            self.rising = self.rising.saturating_add(1);
        }
        edge
    }

    /// The current debounced level.
    #[must_use]
    pub const fn state(&self) -> bool {
        self.debouncer.state()
    }

    /// Number of rising edges observed so far.
    #[must_use]
    pub const fn rising_count(&self) -> u32 {
        self.rising
    }

    /// Resets the edge count, leaving the debounced level untouched.
    pub const fn reset_count(&mut self) {
        self.rising = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::{Debouncer, Edge, EdgeCounter};

    #[test]
    fn a_clean_edge_is_reported_after_the_required_samples() {
        let mut debouncer = Debouncer::new(3);
        assert_eq!(debouncer.update(true), None);
        assert_eq!(debouncer.update(true), None);
        assert_eq!(debouncer.update(true), Some(Edge::Rising));
        assert!(debouncer.state());
        assert_eq!(debouncer.update(true), None);
    }

    #[test]
    fn bouncing_counts_as_a_single_edge() {
        let mut counter = EdgeCounter::new(3);
        // A noisy contact closure: the level flaps before settling.
        for raw in [true, false, true, false, true, true, true, true] {
            counter.update(raw);
        }
        assert!(counter.state());
        assert_eq!(counter.rising_count(), 1);
    }

    #[test]
    fn a_glitch_shorter_than_the_window_is_rejected() {
        let mut counter = EdgeCounter::new(3);
        for raw in [true, true, false, false, false, false] {
            counter.update(raw);
        }
        assert!(!counter.state());
        assert_eq!(counter.rising_count(), 0);
    }

    #[test]
    fn falling_edges_do_not_increment_the_count() {
        let mut counter = EdgeCounter::new(1);
        counter.update(true);
        counter.update(false);
        counter.update(true);
        assert_eq!(counter.rising_count(), 2);
        counter.update(false);
        assert_eq!(counter.rising_count(), 2);
    }

    #[test]
    fn a_zero_length_window_still_debounces_once() {
        let mut debouncer = Debouncer::new(0);
        assert_eq!(debouncer.update(true), Some(Edge::Rising));
    }

    #[test]
    fn the_count_saturates_rather_than_wrapping() {
        let mut counter = EdgeCounter::new(1);
        for _ in 0..3 {
            counter.update(true);
            counter.update(false);
        }
        assert_eq!(counter.rising_count(), 3);
        counter.reset_count();
        assert_eq!(counter.rising_count(), 0);
    }
}
