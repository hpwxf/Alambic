//! The panel controls: two quadrature encoders and four buttons.
//!
//! Encoder lines and switch contacts are wired to ground and rely on internal
//! pull-ups, so a pressed button reads **low**. The quadrature decoding is done
//! by [`oc_core::encoder::QuadratureDecoder`], which is shared with the
//! simulator and covered by the core's tests; this driver only samples pins and
//! accumulates the detents that the engine has not yet consumed.

use embedded_hal::digital::InputPin;

use oc_core::encoder::QuadratureDecoder;
use oc_core::platform::{BUTTONS, ControlEvents, Controls, ENCODERS};

use crate::triggers::Polarity;

/// One encoder's two quadrature lines and its push switch.
#[derive(Debug)]
pub struct EncoderPins<P> {
    /// Quadrature line A.
    pub line_a: P,
    /// Quadrature line B.
    pub line_b: P,
}

/// The panel controls.
///
/// Buttons are ordered as [`oc_core::platform::Button`]: left encoder switch,
/// right encoder switch, up, down.
#[derive(Debug)]
pub struct Panel<P> {
    encoders: [EncoderPins<P>; ENCODERS],
    buttons: [P; BUTTONS],
    decoders: [QuadratureDecoder; ENCODERS],
    /// Detents seen since the engine last polled.
    pending: [i8; ENCODERS],
    held: [bool; BUTTONS],
    polarity: Polarity,
    read_failures: u32,
}

impl<P> Panel<P>
where
    P: InputPin,
{
    /// Wraps the encoder and button pins.
    ///
    /// `polarity` describes the buttons: with the usual pull-up wiring they are
    /// [`Polarity::ActiveLow`].
    pub fn new(
        encoders: [EncoderPins<P>; ENCODERS],
        buttons: [P; BUTTONS],
        polarity: Polarity,
    ) -> Self {
        Self {
            encoders,
            buttons,
            decoders: [QuadratureDecoder::new(); ENCODERS],
            pending: [0; ENCODERS],
            held: [false; BUTTONS],
            polarity,
            read_failures: 0,
        }
    }

    /// Samples the encoders and buttons.
    ///
    /// This must run far more often than the engine polls, because a detent is
    /// four transitions and a missed transition is a lost step. Calling it once
    /// per 1 kHz tick handles a human turning a knob comfortably.
    pub fn sample(&mut self) {
        for index in 0..ENCODERS {
            let encoder = &mut self.encoders[index];
            let (Ok(line_a), Ok(line_b)) = (encoder.line_a.is_high(), encoder.line_b.is_high())
            else {
                self.read_failures = self.read_failures.saturating_add(1);
                continue;
            };
            let detents = self.decoders[index].update(line_a, line_b);
            if detents != 0 {
                self.pending[index] = self.pending[index].saturating_add(detents);
            }
        }

        for index in 0..BUTTONS {
            match self.buttons[index].is_high() {
                Ok(high) => self.held[index] = self.polarity.is_active(high),
                Err(_) => self.read_failures = self.read_failures.saturating_add(1),
            }
        }
    }

    /// Number of failed pin reads since boot; zero on healthy hardware.
    #[must_use]
    pub const fn read_failures(&self) -> u32 {
        self.read_failures
    }
}

impl<P> Controls for Panel<P> {
    fn poll(&mut self) -> ControlEvents {
        let events = ControlEvents {
            encoder_delta: self.pending,
            button_down: self.held,
        };
        // Movement is reported once; button levels persist until they change.
        self.pending = [0; ENCODERS];
        events
    }
}

#[cfg(test)]
mod tests {
    use core::cell::Cell;

    use embedded_hal::digital::{ErrorType, InputPin};

    use oc_core::platform::{BUTTONS, Button, Controls, ENCODERS};

    use super::{EncoderPins, Panel, Polarity};

    /// An input pin whose level the test drives through a shared cell.
    #[derive(Debug, Default)]
    struct FakePin {
        high: Cell<bool>,
        fail: Cell<bool>,
    }

    impl ErrorType for FakePin {
        type Error = embedded_hal::digital::ErrorKind;
    }

    impl InputPin for FakePin {
        fn is_high(&mut self) -> Result<bool, Self::Error> {
            if self.fail.get() {
                return Err(embedded_hal::digital::ErrorKind::Other);
            }
            Ok(self.high.get())
        }

        fn is_low(&mut self) -> Result<bool, Self::Error> {
            self.is_high().map(|high| !high)
        }
    }

    fn panel() -> Panel<FakePin> {
        Panel::new(
            core::array::from_fn(|_| EncoderPins {
                line_a: FakePin::default(),
                line_b: FakePin::default(),
            }),
            core::array::from_fn(|_| FakePin::default()),
            Polarity::ActiveHigh,
        )
    }

    /// Drives one encoder through a full clockwise Gray-code cycle.
    fn turn_clockwise(panel: &mut Panel<FakePin>, index: usize) {
        for (a, b) in [(true, false), (true, true), (false, true), (false, false)] {
            panel.encoders[index].line_a.high.set(a);
            panel.encoders[index].line_b.high.set(b);
            panel.sample();
        }
    }

    /// Drives one encoder through a full anticlockwise cycle.
    fn turn_anticlockwise(panel: &mut Panel<FakePin>, index: usize) {
        for (a, b) in [(false, true), (true, true), (true, false), (false, false)] {
            panel.encoders[index].line_a.high.set(a);
            panel.encoders[index].line_b.high.set(b);
            panel.sample();
        }
    }

    #[test]
    fn a_full_cycle_reports_one_detent() {
        let mut panel = panel();
        turn_clockwise(&mut panel, 0);
        assert_eq!(panel.poll().encoder_delta, [1, 0]);
    }

    #[test]
    fn turning_the_other_way_reports_the_opposite_detent() {
        let mut panel = panel();
        turn_anticlockwise(&mut panel, 1);
        assert_eq!(panel.poll().encoder_delta, [0, -1]);
    }

    #[test]
    fn detents_accumulate_until_polled_and_then_reset() {
        let mut panel = panel();
        turn_clockwise(&mut panel, 0);
        turn_clockwise(&mut panel, 0);
        turn_clockwise(&mut panel, 0);
        assert_eq!(panel.poll().delta(0), 3, "movement must not be lost");
        assert_eq!(panel.poll().delta(0), 0, "and must be reported only once");
    }

    #[test]
    fn the_two_encoders_are_independent() {
        let mut panel = panel();
        turn_clockwise(&mut panel, 0);
        turn_anticlockwise(&mut panel, 1);
        assert_eq!(panel.poll().encoder_delta, [1, -1]);
    }

    #[test]
    fn a_partial_turn_reports_nothing_yet() {
        let mut panel = panel();
        panel.encoders[0].line_a.high.set(true);
        panel.sample();
        assert_eq!(panel.poll().delta(0), 0);
    }

    #[test]
    fn button_levels_persist_across_polls() {
        let mut panel = panel();
        panel.buttons[Button::Up.index()].high.set(true);
        panel.sample();
        assert!(panel.poll().is_down(Button::Up));
        assert!(
            panel.poll().is_down(Button::Up),
            "a held button must stay reported as held"
        );

        panel.buttons[Button::Up.index()].high.set(false);
        panel.sample();
        assert!(!panel.poll().is_down(Button::Up));
    }

    #[test]
    fn pull_up_wiring_reads_a_low_pin_as_pressed() {
        let mut panel = Panel::new(
            core::array::from_fn(|_| EncoderPins {
                line_a: FakePin::default(),
                line_b: FakePin::default(),
            }),
            core::array::from_fn(|_| FakePin::default()),
            Polarity::ActiveLow,
        );
        panel.sample();
        assert!(
            panel.poll().is_down(Button::LeftEncoder),
            "with pull-ups, an idle-low pin is a pressed button"
        );
    }

    #[test]
    fn buttons_are_independent() {
        let mut panel = panel();
        panel.buttons[Button::Down.index()].high.set(true);
        panel.sample();

        let events = panel.poll();
        for button in Button::ALL {
            assert_eq!(events.is_down(button), button == Button::Down);
        }
        assert_eq!(Button::ALL.len(), BUTTONS);
    }

    #[test]
    fn a_failing_pin_is_counted_and_does_not_invent_movement() {
        let mut panel = panel();
        panel.encoders[0].line_a.fail.set(true);
        turn_clockwise(&mut panel, 0);

        assert_eq!(
            panel.poll().delta(0),
            0,
            "no phantom detents from a dead pin"
        );
        assert!(panel.read_failures() >= 1);
        assert_eq!(ENCODERS, 2);
    }
}
