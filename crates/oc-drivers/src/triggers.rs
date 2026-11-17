//! The four trigger inputs, read through GPIO pins.
//!
//! Ornament & Crime buffers its gate inputs through inverting transistors, so a
//! high gate at the jack reads as a **low** level at the microcontroller. The
//! polarity is therefore configurable rather than assumed: getting it wrong
//! would invert every gate on the module, and the diagnostic applet is what
//! makes that immediately visible.
//!
//! Sampling the level once per tick misses a pulse that starts and ends
//! between two samples; on the module, a third of the 1 ms triggers were lost
//! that way (journal/D-023, D-019). A pin that also latches its edge in
//! hardware ([`EdgeCapture`]) closes the gap: [`Triggers::sample_capturing`]
//! reports a gate that rose since the previous sample even if it has already
//! fallen, for one sample, so the core sees one rising edge for it whatever
//! its width.

use embedded_hal::digital::InputPin;

use oc_core::platform::{DigitalIn, TRIGGER_CHANNELS, TriggerChannel};

/// Which electrical level at the pin means "gate present at the jack".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Polarity {
    /// A low pin level means an active gate, as produced by an inverting
    /// input buffer. This is what Ornament & Crime uses.
    #[default]
    ActiveLow,
    /// A high pin level means an active gate.
    ActiveHigh,
}

impl Polarity {
    /// Translates a raw pin level into a logical gate state.
    #[must_use]
    pub const fn is_active(self, pin_is_high: bool) -> bool {
        match self {
            Self::ActiveLow => !pin_is_high,
            Self::ActiveHigh => pin_is_high,
        }
    }
}

/// A pin whose hardware remembers its active edge (the one where the gate at
/// the jack rises: a falling pin behind an inverting buffer) until read.
///
/// On the i.MX RT this is the GPIO edge detector's status bit, which latches
/// with no interrupt taken; the firmware configures which edge it watches.
pub trait EdgeCapture {
    /// Whether the active edge came since the previous call; clears it.
    fn take_active_edge(&mut self) -> bool;
}

/// The four trigger inputs.
///
/// Reads are cached: [`Self::sample`] polls the hardware once per tick, and the
/// [`DigitalIn`] implementation then answers from that snapshot. Without this,
/// the four channels would be sampled at four slightly different instants, and
/// [`DigitalIn::trigger_state`] takes `&self` so it could not poll anyway.
#[derive(Debug)]
pub struct Triggers<P> {
    pins: [P; TRIGGER_CHANNELS],
    polarity: Polarity,
    levels: [bool; TRIGGER_CHANNELS],
    read_failures: u32,
}

impl<P> Triggers<P>
where
    P: InputPin,
{
    /// Wraps four input pins, in channel order.
    pub fn new(pins: [P; TRIGGER_CHANNELS], polarity: Polarity) -> Self {
        Self {
            pins,
            polarity,
            levels: [false; TRIGGER_CHANNELS],
            read_failures: 0,
        }
    }

    /// Samples all four pins, so that the tick sees one coherent snapshot.
    pub fn sample(&mut self) {
        for (index, pin) in self.pins.iter_mut().enumerate() {
            match pin.is_high() {
                Ok(high) => self.levels[index] = self.polarity.is_active(high),
                Err(_) => {
                    // Hold the previous level: a momentary read failure must
                    // not be reported as a gate edge.
                    self.read_failures = self.read_failures.saturating_add(1);
                }
            }
        }
    }

    /// Samples all four pins like [`Self::sample`], and also reports as
    /// active any gate whose edge the hardware latched since the previous
    /// sample: a pulse shorter than a tick is seen once instead of never.
    ///
    /// A long pulse is not counted twice. Its edge either comes before this
    /// sample, which then also reads it high, or after the level was read, in
    /// which case the next sample takes the latch while the level is still
    /// high (or reports the pulse one more sample if it has ended).
    pub fn sample_capturing(&mut self)
    where
        P: EdgeCapture,
    {
        self.sample();
        for (level, pin) in self.levels.iter_mut().zip(&mut self.pins) {
            if pin.take_active_edge() {
                *level = true;
            }
        }
    }

    /// Number of failed pin reads since boot; zero on healthy hardware.
    #[must_use]
    pub const fn read_failures(&self) -> u32 {
        self.read_failures
    }

    /// The configured polarity.
    #[must_use]
    pub const fn polarity(&self) -> Polarity {
        self.polarity
    }
}

impl<P> DigitalIn for Triggers<P> {
    fn trigger_state(&self, channel: TriggerChannel) -> bool {
        self.levels[channel.index()]
    }
}

#[cfg(test)]
mod tests {
    use embedded_hal::digital::{ErrorType, InputPin};

    use oc_core::platform::{DigitalIn, TRIGGER_CHANNELS, TriggerChannel};

    use std::vec::Vec;

    use super::{EdgeCapture, Polarity, Triggers};

    /// An input pin whose level, and failure behaviour, the test controls.
    #[derive(Debug, Default)]
    struct FakePin {
        high: bool,
        fail: bool,
        /// The latched active edge, as the hardware would hold it.
        edge: bool,
    }

    impl EdgeCapture for FakePin {
        fn take_active_edge(&mut self) -> bool {
            core::mem::take(&mut self.edge)
        }
    }

    impl ErrorType for FakePin {
        type Error = embedded_hal::digital::ErrorKind;
    }

    impl InputPin for FakePin {
        fn is_high(&mut self) -> Result<bool, Self::Error> {
            if self.fail {
                return Err(embedded_hal::digital::ErrorKind::Other);
            }
            Ok(self.high)
        }

        fn is_low(&mut self) -> Result<bool, Self::Error> {
            self.is_high().map(|high| !high)
        }
    }

    fn triggers(polarity: Polarity) -> Triggers<FakePin> {
        Triggers::new(core::array::from_fn(|_| FakePin::default()), polarity)
    }

    #[test]
    fn an_inverting_buffer_reads_a_low_pin_as_an_active_gate() {
        let mut triggers = triggers(Polarity::ActiveLow);
        triggers.sample();
        assert!(
            triggers.trigger_state(TriggerChannel::One),
            "a low pin is an active gate on Ornament & Crime"
        );

        triggers.pins[0].high = true;
        triggers.sample();
        assert!(!triggers.trigger_state(TriggerChannel::One));
    }

    #[test]
    fn active_high_wiring_is_also_supported() {
        let mut triggers = triggers(Polarity::ActiveHigh);
        triggers.sample();
        assert!(!triggers.trigger_state(TriggerChannel::One));

        triggers.pins[0].high = true;
        triggers.sample();
        assert!(triggers.trigger_state(TriggerChannel::One));
    }

    #[test]
    fn channels_are_independent_and_in_order() {
        let mut triggers = triggers(Polarity::ActiveHigh);
        triggers.pins[2].high = true;
        triggers.sample();

        for channel in TriggerChannel::ALL {
            assert_eq!(
                triggers.trigger_state(channel),
                channel.index() == 2,
                "only channel three should be active, {channel:?} disagrees"
            );
        }
    }

    #[test]
    fn state_only_changes_when_sampled() {
        let mut triggers = triggers(Polarity::ActiveHigh);
        triggers.pins[0].high = true;
        assert!(
            !triggers.trigger_state(TriggerChannel::One),
            "the snapshot must not change behind the tick's back"
        );
        triggers.sample();
        assert!(triggers.trigger_state(TriggerChannel::One));
    }

    /// Rising edges the core would count over successive samples.
    fn rising_edges(states: &[bool]) -> usize {
        states.windows(2).filter(|pair| !pair[0] && pair[1]).count()
    }

    #[test]
    fn a_pulse_between_two_samples_is_seen_once() {
        let mut triggers = triggers(Polarity::ActiveHigh);
        let mut states = Vec::new();
        triggers.sample_capturing();
        states.push(triggers.trigger_state(TriggerChannel::One));
        // Rose and fell between two samples: only the latch knows.
        triggers.pins[0].edge = true;
        for _ in 0..3 {
            triggers.sample_capturing();
            states.push(triggers.trigger_state(TriggerChannel::One));
        }
        assert_eq!(states, [false, true, false, false]);
        assert_eq!(rising_edges(&states), 1);
    }

    #[test]
    fn a_long_pulse_is_not_counted_twice() {
        let mut triggers = triggers(Polarity::ActiveHigh);
        let mut states = Vec::new();
        triggers.sample_capturing();
        states.push(triggers.trigger_state(TriggerChannel::One));
        // The edge lands after this sample read the level: the latch is only
        // taken by the next one, while the gate is still high.
        triggers.pins[0].high = true;
        triggers.sample_capturing();
        states.push(triggers.trigger_state(TriggerChannel::One));
        triggers.pins[0].edge = true;
        for _ in 0..2 {
            triggers.sample_capturing();
            states.push(triggers.trigger_state(TriggerChannel::One));
        }
        // Same, but the gate ended before the latch was taken.
        triggers.pins[0].high = false;
        triggers.pins[0].edge = true;
        triggers.sample_capturing();
        states.push(triggers.trigger_state(TriggerChannel::One));
        triggers.sample_capturing();
        states.push(triggers.trigger_state(TriggerChannel::One));
        assert_eq!(states, [false, true, true, true, true, false]);
        assert_eq!(rising_edges(&states), 1);
    }

    #[test]
    fn a_latched_edge_counts_with_an_inverting_buffer_too() {
        let mut triggers = triggers(Polarity::ActiveLow);
        for pin in &mut triggers.pins {
            pin.high = true;
        }
        triggers.sample_capturing();
        assert!(!triggers.trigger_state(TriggerChannel::Four));
        triggers.pins[3].edge = true;
        triggers.sample_capturing();
        assert!(triggers.trigger_state(TriggerChannel::Four));
        assert!(!triggers.trigger_state(TriggerChannel::Three));
    }

    #[test]
    fn a_failing_pin_holds_its_previous_level_and_is_counted() {
        let mut triggers = triggers(Polarity::ActiveHigh);
        triggers.pins[1].high = true;
        triggers.sample();
        assert!(triggers.trigger_state(TriggerChannel::Two));

        triggers.pins[1].fail = true;
        triggers.sample();
        assert!(
            triggers.trigger_state(TriggerChannel::Two),
            "a read failure must not look like a gate falling"
        );
        assert_eq!(triggers.read_failures(), 1);
    }

    #[test]
    fn the_polarity_is_reported_for_the_boot_banner() {
        assert_eq!(
            triggers(Polarity::ActiveLow).polarity(),
            Polarity::ActiveLow
        );
        assert_eq!(Polarity::default(), Polarity::ActiveLow);
    }

    #[test]
    fn all_channels_are_covered_by_the_array() {
        assert_eq!(TriggerChannel::ALL.len(), TRIGGER_CHANNELS);
    }
}
