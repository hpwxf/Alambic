//! Quadrature decoding for the two rotary encoders.
//!
//! The panel encoders produce a full Gray-code cycle — four transitions — per
//! mechanical detent. The decoder accumulates transitions and only reports a
//! movement once a whole detent has been travelled, so contact bounce inside a
//! detent cancels out instead of producing spurious steps.
//!
//! Backends that already receive detent counts (the simulator, VCV Rack) skip
//! this and report deltas directly through
//! [`ControlEvents`](crate::platform::ControlEvents).

/// Transitions per mechanical detent.
const TRANSITIONS_PER_DETENT: i8 = 4;

/// Movement associated with each `(previous, current)` Gray-code pair.
///
/// The index is `previous << 2 | current`, where a state packs `A` in bit 1 and
/// `B` in bit 0. Invalid transitions (both lines changing at once, which means
/// a sample was missed) map to zero: a lost step is better than a wrong one.
#[rustfmt::skip]
const TRANSITION: [i8; 16] = [
     0, -1,  1,  0,
     1,  0,  0, -1,
    -1,  0,  0,  1,
     0,  1, -1,  0,
];

/// Decodes one quadrature encoder.
#[derive(Debug, Clone, Copy, Default)]
pub struct QuadratureDecoder {
    previous: u8,
    accumulator: i8,
}

impl QuadratureDecoder {
    /// A decoder starting from the `(low, low)` state.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            previous: 0,
            accumulator: 0,
        }
    }

    /// Feeds one sample of the two encoder lines.
    ///
    /// Returns the number of whole detents travelled since the previous
    /// report, which is almost always zero, `1` or `-1`.
    pub fn update(&mut self, line_a: bool, line_b: bool) -> i8 {
        let current = (u8::from(line_a) << 1) | u8::from(line_b);
        let index = usize::from((self.previous << 2) | current);
        self.previous = current;

        let movement = TRANSITION[index];
        if movement == 0 {
            return 0;
        }

        // Reversing direction discards the partial detent instead of letting
        // it combine with movement the other way.
        if self.accumulator.signum() != movement.signum() {
            self.accumulator = 0;
        }
        self.accumulator += movement;

        let detents = self.accumulator / TRANSITIONS_PER_DETENT;
        if detents != 0 {
            self.accumulator -= detents * TRANSITIONS_PER_DETENT;
        }
        detents
    }

    /// Forgets any partially travelled detent.
    pub const fn reset(&mut self) {
        self.accumulator = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::QuadratureDecoder;

    /// One full clockwise Gray-code cycle: 00 -> 10 -> 11 -> 01 -> 00.
    const CLOCKWISE: [(bool, bool); 4] =
        [(true, false), (true, true), (false, true), (false, false)];

    /// One full counter-clockwise cycle: 00 -> 01 -> 11 -> 10 -> 00.
    const COUNTER_CLOCKWISE: [(bool, bool); 4] =
        [(false, true), (true, true), (true, false), (false, false)];

    fn travel(decoder: &mut QuadratureDecoder, steps: &[(bool, bool)]) -> i32 {
        steps
            .iter()
            .map(|&(a, b)| i32::from(decoder.update(a, b)))
            .sum()
    }

    #[test]
    fn one_clockwise_cycle_is_one_detent() {
        let mut decoder = QuadratureDecoder::new();
        assert_eq!(travel(&mut decoder, &CLOCKWISE), 1);
    }

    #[test]
    fn one_counter_clockwise_cycle_is_minus_one_detent() {
        let mut decoder = QuadratureDecoder::new();
        assert_eq!(travel(&mut decoder, &COUNTER_CLOCKWISE), -1);
    }

    #[test]
    fn partial_movement_reports_nothing() {
        let mut decoder = QuadratureDecoder::new();
        assert_eq!(travel(&mut decoder, &CLOCKWISE[..3]), 0);
    }

    #[test]
    fn bouncing_inside_a_detent_cancels_out() {
        let mut decoder = QuadratureDecoder::new();
        // Forward, back, forward, back: the knob never leaves its detent.
        let jitter = [(true, false), (false, false), (true, false), (false, false)];
        assert_eq!(travel(&mut decoder, &jitter), 0);
    }

    #[test]
    fn many_cycles_accumulate_exactly() {
        let mut decoder = QuadratureDecoder::new();
        let mut total = 0;
        for _ in 0..100 {
            total += travel(&mut decoder, &CLOCKWISE);
        }
        assert_eq!(total, 100);
    }

    #[test]
    fn an_impossible_transition_is_ignored() {
        let mut decoder = QuadratureDecoder::new();
        // 00 -> 11 means a sample was missed; direction is unknowable.
        assert_eq!(decoder.update(true, true), 0);
    }

    #[test]
    fn reversing_direction_does_not_produce_a_phantom_detent() {
        let mut decoder = QuadratureDecoder::new();
        // Two transitions forward, then the same two backwards: the knob is
        // back where it started, so nothing should be reported.
        assert_eq!(travel(&mut decoder, &CLOCKWISE[..2]), 0);
        assert_eq!(travel(&mut decoder, &[CLOCKWISE[0], (false, false)]), 0);
    }

    #[test]
    fn direction_changes_do_not_accumulate_across_detents() {
        let mut decoder = QuadratureDecoder::new();
        let mut net = 0;
        for _ in 0..10 {
            net += travel(&mut decoder, &CLOCKWISE);
            net += travel(&mut decoder, &COUNTER_CLOCKWISE);
        }
        assert_eq!(net, 0);
    }
}
