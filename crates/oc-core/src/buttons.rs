//! Debouncing, edge detection and chord arbitration for the front-panel buttons.
//!
//! Every applet used to debounce the buttons itself. With more than one applet
//! that duplication stops being harmless: the `up` + `down` chord that opens the
//! app menu has to be arbitrated *before* an applet sees anything, otherwise the
//! first of the two buttons fires its own action long before the chord is even
//! detectable — a human pressing "both together" lands the two presses twenty to
//! fifty milliseconds apart, while the debouncer settles in three.
//!
//! The arbitration is deliberately timer-free: `up` and `down` act on **release**,
//! and only when the chord did not form while they were down. Nothing has to be
//! tuned, and the behaviour is identical at any tick rate.
//!
//! The two encoder switches keep acting on press. They take no part in the chord,
//! so there is nothing to arbitrate and no reason to make them feel sluggish.

use crate::debounce::{DEFAULT_STABLE_SAMPLES, Debouncer, Edge};
use crate::platform::{BUTTONS, Button, ControlEvents};

/// The two buttons taking part in the menu chord.
///
/// Their position here is what `ButtonReader::pending` is indexed by.
const CHORD_BUTTONS: [Button; 2] = [Button::Up, Button::Down];

/// What the buttons did during one tick, after debouncing and arbitration.
///
/// `pressed` is the *action* of a button, true for exactly one tick; an applet
/// reacts to it and never to a raw level. `held` is the debounced level, for the
/// rare case where an applet wants to know a button is still down.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ButtonEvents {
    pressed: [bool; BUTTONS],
    held: [bool; BUTTONS],
    menu_chord: bool,
}

impl ButtonEvents {
    /// Whether this button's action fires on this tick.
    #[must_use]
    pub fn pressed(&self, button: Button) -> bool {
        self.pressed.get(button.index()).copied().unwrap_or(false)
    }

    /// Whether this button is currently held, after debouncing.
    #[must_use]
    pub fn held(&self, button: Button) -> bool {
        self.held.get(button.index()).copied().unwrap_or(false)
    }

    /// Whether `up` and `down` became held together on this tick.
    #[must_use]
    pub const fn menu_chord(&self) -> bool {
        self.menu_chord
    }

    /// Drops every action and level, keeping the events otherwise valid.
    ///
    /// Used when the panel belongs to something other than the running applet.
    pub const fn silence(&mut self) {
        self.pressed = [false; BUTTONS];
        self.held = [false; BUTTONS];
        self.menu_chord = false;
    }
}

/// Debounces the four buttons and resolves the `up` + `down` chord.
#[derive(Debug, Clone, Copy)]
pub struct ButtonReader {
    debouncers: [Debouncer; BUTTONS],
    /// Whether `up` / `down` is down with its action not yet resolved.
    pending: [bool; 2],
    /// Whether a chord is standing, so holding both does not fire it again.
    chord_active: bool,
}

impl ButtonReader {
    /// A reader with every button released.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            debouncers: [Debouncer::new(DEFAULT_STABLE_SAMPLES); BUTTONS],
            pending: [false; 2],
            chord_active: false,
        }
    }

    /// Consumes one poll of the raw controls and returns the resulting actions.
    pub fn update(&mut self, controls: &ControlEvents) -> ButtonEvents {
        let mut events = ButtonEvents::default();
        let mut edges = [None; BUTTONS];

        for (index, debouncer) in self.debouncers.iter_mut().enumerate() {
            let Some(button) = Button::from_index(index) else {
                continue;
            };
            edges[index] = debouncer.update(controls.is_down(button));
            events.held[index] = debouncer.state();
        }

        for button in [Button::LeftEncoder, Button::RightEncoder] {
            events.pressed[button.index()] = edges[button.index()] == Some(Edge::Rising);
        }

        for (slot, button) in CHORD_BUTTONS.into_iter().enumerate() {
            if edges[button.index()] == Some(Edge::Rising) {
                self.pending[slot] = true;
            }
        }

        // Both held at once is the chord, whatever the gap between the two
        // presses. It fires once and swallows both pending actions; releasing
        // either button arms it again, so `up`+`down`, release `up`, press `up`
        // again is a second chord rather than a dead gesture.
        let both_held = events.held[Button::Up.index()] && events.held[Button::Down.index()];
        if both_held {
            if !self.chord_active {
                events.menu_chord = true;
                self.chord_active = true;
                self.pending = [false; 2];
            }
        } else {
            self.chord_active = false;
        }

        for (slot, button) in CHORD_BUTTONS.into_iter().enumerate() {
            if edges[button.index()] != Some(Edge::Falling) {
                continue;
            }
            if self.pending[slot] {
                events.pressed[button.index()] = true;
            }
            self.pending[slot] = false;
        }

        events
    }
}

impl Default for ButtonReader {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::{ButtonEvents, ButtonReader};
    use crate::platform::{BUTTONS, Button, ControlEvents, ENCODERS};

    /// What a run of ticks produced, summed. `no_std` rules out collecting the
    /// events into a `Vec`, and a tally is what every test here asks for anyway.
    #[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
    struct Tally {
        chords: u32,
        presses: [u32; BUTTONS],
        last: ButtonEvents,
    }

    impl Tally {
        fn add(&mut self, events: ButtonEvents) {
            self.chords += u32::from(events.menu_chord());
            for button in Button::ALL {
                self.presses[button.index()] += u32::from(events.pressed(button));
            }
            self.last = events;
        }

        const fn presses(&self, button: Button) -> u32 {
            self.presses[button.index()]
        }
    }

    /// Feeds one sample with exactly `held` down.
    fn step(reader: &mut ButtonReader, held: &[Button]) -> ButtonEvents {
        let mut controls = ControlEvents {
            encoder_delta: [0; ENCODERS],
            button_down: [false; BUTTONS],
        };
        for button in held {
            controls.button_down[button.index()] = true;
        }
        reader.update(&controls)
    }

    /// Feeds `count` identical samples into `tally`.
    fn steps(reader: &mut ButtonReader, held: &[Button], count: usize, tally: &mut Tally) {
        for _ in 0..count {
            tally.add(step(reader, held));
        }
    }

    #[test]
    fn a_lone_press_only_fires_once_the_button_is_released() {
        let mut reader = ButtonReader::new();
        let mut down = Tally::default();
        steps(&mut reader, &[Button::Up], 10, &mut down);
        assert_eq!(
            down.presses(Button::Up),
            0,
            "holding up must not fire anything: the chord may still form"
        );
        assert!(down.last.held(Button::Up));

        let mut released = Tally::default();
        steps(&mut reader, &[], 10, &mut released);
        assert_eq!(
            released.presses(Button::Up),
            1,
            "releasing fires the action exactly once"
        );
    }

    #[test]
    fn two_successive_presses_fire_twice() {
        let mut reader = ButtonReader::new();
        let mut tally = Tally::default();
        for _ in 0..2 {
            steps(&mut reader, &[Button::Down], 6, &mut tally);
            steps(&mut reader, &[], 6, &mut tally);
        }
        assert_eq!(tally.presses(Button::Down), 2);
    }

    #[test]
    fn a_slow_chord_fires_once_and_swallows_both_presses() {
        let mut reader = ButtonReader::new();
        let mut tally = Tally::default();
        // Forty ticks apart: far wider than the three-sample debounce window,
        // and representative of a real two-thumb press.
        steps(&mut reader, &[Button::Up], 40, &mut tally);
        steps(&mut reader, &[Button::Up, Button::Down], 20, &mut tally);
        steps(&mut reader, &[], 20, &mut tally);

        assert_eq!(tally.chords, 1, "the chord fires exactly once");
        assert_eq!(
            (tally.presses(Button::Up), tally.presses(Button::Down)),
            (0, 0),
            "neither button fires its own action"
        );
    }

    #[test]
    fn pressing_both_in_the_same_tick_is_one_chord() {
        let mut reader = ButtonReader::new();
        let mut tally = Tally::default();
        steps(&mut reader, &[Button::Up, Button::Down], 20, &mut tally);
        assert_eq!(tally.chords, 1, "holding the chord does not repeat it");
        steps(&mut reader, &[], 10, &mut tally);
        assert_eq!(tally.chords, 1, "releasing does not fire another");
    }

    #[test]
    fn one_after_the_other_is_two_single_presses_and_no_chord() {
        let mut reader = ButtonReader::new();
        let mut tally = Tally::default();
        steps(&mut reader, &[Button::Up], 8, &mut tally);
        steps(&mut reader, &[], 8, &mut tally);
        steps(&mut reader, &[Button::Down], 8, &mut tally);
        steps(&mut reader, &[], 8, &mut tally);

        assert_eq!(
            tally.chords, 0,
            "the presses never overlap, so there is no chord"
        );
        assert_eq!(
            (tally.presses(Button::Up), tally.presses(Button::Down)),
            (1, 1)
        );
    }

    #[test]
    fn releasing_one_button_arms_the_chord_again() {
        let mut reader = ButtonReader::new();
        let mut tally = Tally::default();
        steps(&mut reader, &[Button::Up, Button::Down], 10, &mut tally);
        // Let go of up only; down stays held throughout.
        steps(&mut reader, &[Button::Down], 10, &mut tally);
        steps(&mut reader, &[Button::Up, Button::Down], 10, &mut tally);
        assert_eq!(
            tally.chords, 2,
            "pressing up again while down is held is a second chord"
        );

        steps(&mut reader, &[], 10, &mut tally);
        assert_eq!(
            (tally.presses(Button::Up), tally.presses(Button::Down)),
            (0, 0),
            "every press was consumed by a chord"
        );
    }

    #[test]
    fn a_bouncing_contact_still_fires_a_single_press() {
        let mut reader = ButtonReader::new();
        for raw in [true, false, true, false, true, true, true, true] {
            let held: &[Button] = if raw { &[Button::Up] } else { &[] };
            step(&mut reader, held);
        }
        let mut tally = Tally::default();
        steps(&mut reader, &[], 8, &mut tally);
        assert_eq!(tally.presses(Button::Up), 1);
    }

    #[test]
    fn the_encoder_switches_still_act_on_press() {
        let mut reader = ButtonReader::new();
        let mut tally = Tally::default();
        steps(
            &mut reader,
            &[Button::LeftEncoder, Button::RightEncoder],
            6,
            &mut tally,
        );
        assert_eq!(
            (
                tally.presses(Button::LeftEncoder),
                tally.presses(Button::RightEncoder)
            ),
            (1, 1),
            "encoder presses are not deferred to release"
        );
        assert_eq!(
            tally.chords, 0,
            "the encoder switches take no part in the chord"
        );
    }

    #[test]
    fn held_follows_the_debounced_level_through_a_chord() {
        let mut reader = ButtonReader::new();
        let mut tally = Tally::default();
        steps(&mut reader, &[Button::Up, Button::Down], 8, &mut tally);
        assert!(tally.last.held(Button::Up) && tally.last.held(Button::Down));

        steps(&mut reader, &[], 8, &mut tally);
        assert!(!tally.last.held(Button::Up) && !tally.last.held(Button::Down));
    }

    #[test]
    fn silencing_drops_every_action() {
        let mut reader = ButtonReader::new();
        step(&mut reader, &[Button::LeftEncoder]);
        step(&mut reader, &[Button::LeftEncoder]);
        let mut events = step(&mut reader, &[Button::LeftEncoder]);
        assert!(events.held(Button::LeftEncoder));

        events.silence();
        assert!(!events.held(Button::LeftEncoder));
        assert!(!events.pressed(Button::LeftEncoder));
        assert!(!events.menu_chord());
    }
}
