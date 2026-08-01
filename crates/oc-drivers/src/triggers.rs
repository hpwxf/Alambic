//! The four trigger inputs, read through GPIO pins.
//!
//! Ornament & Crime buffers its gate inputs through inverting transistors, so a
//! high gate at the jack reads as a **low** level at the microcontroller. The
//! polarity is therefore configurable rather than assumed: getting it wrong
//! would invert every gate on the module, and the diagnostic applet is what
//! makes that immediately visible.

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

    use super::{Polarity, Triggers};

    /// An input pin whose level, and failure behaviour, the test controls.
    #[derive(Debug, Default)]
    struct FakePin {
        high: bool,
        fail: bool,
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
