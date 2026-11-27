//! `MELODY`'s steps, where the clock is in them, and how a new step is
//! drawn (journal/D-047, revision of 2026-11-27).
//!
//! A step holds a *place* in the window, from 0 to [`PLACE_MAX`], not a
//! note: the window turns it into a note at the outputs ([`super::window`]),
//! so turning the window's settings bends the whole loop without losing it.
//! Each step the clock reaches may be redrawn first, with a chance: never,
//! the loop repeats; always, every step is new. A draw is either an even
//! place over the window, or a walk of a few degrees from the note the step
//! before plays, found back as a place.
//!
//! Every draw comes from one xorshift generator seeded the same at start, so
//! a run replays exactly, on every backend.

use super::scale::Scale;
use super::window::{PLACE_MAX, Window};

/// The most steps a sequence has.
pub const MAX_STEPS: usize = 32;

/// The longest walk from one step to the next, in degrees.
pub const MAX_JUMP: u8 = 7;

/// Draws whose note repeats the step before's are retried this many times
/// without `REPEAT`; past them, the repeat stands (a window of one note
/// always repeats).
const REPEAT_RETRIES: u32 = 4;

/// The generator's state at start.
const SEED: u32 = 0x9E37_79B9;

/// How a new step is drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Draw {
    /// The scale its note is in.
    pub scale: Scale,
    /// The window its place sounds in.
    pub window: Window,
    /// A walk of up to so many degrees from the step before, never none;
    /// 0 draws an even place instead.
    pub jump: u8,
    /// Whether a step's note may be the same as the step before's.
    pub repeat: bool,
}

impl Draw {
    /// Even places, two octaves of the major scale round the root, repeats
    /// allowed.
    pub const DEFAULT: Self = Self {
        scale: Scale::Major,
        window: Window::DEFAULT,
        jump: 0,
        repeat: true,
    };

    /// The note `place` gives.
    fn note(self, place: u16) -> i32 {
        self.window.note(self.scale, place)
    }
}

/// The steps, the one playing, and the generator.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Sequence {
    places: [u16; MAX_STEPS],
    length: u8,
    /// The step playing, `None` while armed: before the first clock or after
    /// a reset, the next clock plays the first step.
    position: Option<u8>,
    seed: u32,
}

impl Sequence {
    /// `length` steps (1 to [`MAX_STEPS`]), every one drawn by `draw`, armed.
    #[must_use]
    pub fn new(length: u8, draw: Draw) -> Self {
        let mut sequence = Self {
            places: [PLACE_MAX / 2; MAX_STEPS],
            length: 1,
            position: None,
            seed: SEED,
        };
        sequence.set_length(length);
        sequence.reroll(draw);
        sequence
    }

    /// Steps in the loop.
    #[must_use]
    pub const fn length(&self) -> u8 {
        self.length
    }

    /// Sets the steps in the loop, 1 to [`MAX_STEPS`]; a step past the end
    /// keeps its place for when the loop grows again.
    pub fn set_length(&mut self, length: u8) {
        self.length = length.clamp(1, u8::try_from(MAX_STEPS).unwrap_or(u8::MAX));
    }

    /// The step playing, `None` while armed.
    #[must_use]
    pub fn position(&self) -> Option<usize> {
        self.position.map(usize::from)
    }

    /// The place step `index` holds in the window; the middle past the last.
    #[must_use]
    pub fn place(&self, index: usize) -> u16 {
        self.places.get(index).copied().unwrap_or(PLACE_MAX / 2)
    }

    /// Every step's place, those past the loop too.
    #[must_use]
    pub const fn places(&self) -> &[u16; MAX_STEPS] {
        &self.places
    }

    /// Sets step `index`'s place; nothing past the last step.
    pub fn set_place(&mut self, index: usize, place: u16) {
        if let Some(held) = self.places.get_mut(index) {
            *held = place;
        }
    }

    /// Arms the sequence: the next clock plays the first step.
    pub const fn reset(&mut self) {
        self.position = None;
    }

    /// Moves to the next step, the first after the last or while armed,
    /// redrawing it first with `chance_percent`: the step it plays.
    pub fn clock(&mut self, chance_percent: u8, draw: Draw) -> usize {
        let next = match self.position {
            Some(position) if position + 1 < self.length => position + 1,
            _ => 0,
        };
        self.position = Some(next);
        let next = usize::from(next);
        if self.roll(chance_percent) {
            self.redraw(next, draw);
        }
        next
    }

    /// Draws every step anew, each from the one before: those past the loop
    /// too, so a longer loop is fresh as well.
    pub fn reroll(&mut self, draw: Draw) {
        for index in 0..MAX_STEPS {
            self.redraw(index, draw);
        }
    }

    /// Draws step `index` anew from the step before it in the loop.
    pub fn redraw(&mut self, index: usize, draw: Draw) {
        if index >= MAX_STEPS {
            return;
        }
        let length = usize::from(self.length);
        let before = if index == 0 { length - 1 } else { index - 1 };
        let previous = self.places[before.min(MAX_STEPS - 1)];
        self.places[index] = self.drawn(previous, draw);
    }

    /// A new place after `previous`.
    fn drawn(&mut self, previous: u16, draw: Draw) -> u16 {
        if draw.jump > 0 {
            return self.walked(previous, draw);
        }
        let mut place = self.random_place();
        let mut retries = 0;
        while !draw.repeat && draw.note(place) == draw.note(previous) && retries < REPEAT_RETRIES {
            place = self.random_place();
            retries += 1;
        }
        place
    }

    /// The place of a note 1 to `jump` degrees up or down from the one
    /// `previous` gives, within the window's notes: away from a bound it
    /// would cross, shorter when the window is narrower than the jump, and
    /// further when it skips the note aimed at (the bias steep near an end),
    /// so it never stays put while the window has two notes.
    fn walked(&mut self, previous: u16, draw: Draw) -> u16 {
        let scale = draw.scale;
        let (lowest, highest) = draw.window.bounds(scale);
        let (bottom, top) = (scale.degree_of(lowest), scale.degree_of(highest));
        let here = draw.note(previous);
        let from = scale.degree_of(here).clamp(bottom, top);
        let by = 1 + self.below(i32::from(draw.jump.min(MAX_JUMP)));
        let sign = if self.random() & 1 == 0 { 1 } else { -1 };
        // The distance drawn first, then the shorter ones, then the longer:
        // a window narrower than the jump still gives a move.
        let span = top - bottom;
        let distances = (1..=by).rev().chain(by + 1..=span);
        for distance in distances {
            for direction in [sign, -sign] {
                let degree = from + direction * distance;
                if !(bottom..=top).contains(&degree) {
                    continue;
                }
                let place = draw.window.place_of(scale, scale.note_of(degree));
                if draw.note(place) != here {
                    return place;
                }
            }
        }
        previous
    }

    /// Whether a step with `chance_percent` is redrawn.
    fn roll(&mut self, chance_percent: u8) -> bool {
        match chance_percent {
            0 => false,
            100.. => true,
            chance => self.random() % 100 < u32::from(chance),
        }
    }

    /// An even place over the window.
    fn random_place(&mut self) -> u16 {
        u16::try_from(self.random() >> 16).unwrap_or(PLACE_MAX / 2)
    }

    /// An even draw from 0 to `count - 1`; 0 for a count under 1.
    fn below(&mut self, count: i32) -> i32 {
        let Ok(count) = u32::try_from(count) else {
            return 0;
        };
        if count == 0 {
            return 0;
        }
        i32::try_from(self.random() % count).unwrap_or(0)
    }

    /// The next state of the xorshift generator.
    const fn random(&mut self) -> u32 {
        let mut x = self.seed;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.seed = x;
        x
    }
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::{Draw, MAX_JUMP, MAX_STEPS, Sequence};
    use crate::apps::melody::scale::Scale;
    use crate::apps::melody::window::{MAX_SPREAD, Window};

    fn draws() -> impl Strategy<Value = Draw> {
        (
            0..Scale::ALL.len(),
            0..=MAX_SPREAD,
            -100_i8..=100,
            -100_i8..=100,
            0..=MAX_JUMP,
            any::<bool>(),
        )
            .prop_map(|(scale, spread, center, bias, jump, repeat)| Draw {
                scale: Scale::from_index(scale),
                window: Window {
                    spread,
                    center,
                    bias,
                },
                jump,
                repeat,
            })
    }

    #[test]
    fn the_clock_loops_over_the_length_from_armed() {
        let mut sequence = Sequence::new(3, Draw::DEFAULT);
        assert_eq!(sequence.position(), None, "armed");
        let played: [usize; 7] = core::array::from_fn(|_| sequence.clock(0, Draw::DEFAULT));
        assert_eq!(played, [0, 1, 2, 0, 1, 2, 0]);
        sequence.reset();
        assert_eq!(
            sequence.clock(0, Draw::DEFAULT),
            0,
            "the first after a reset"
        );
    }

    #[test]
    fn a_shorter_loop_wraps_at_once() {
        let mut sequence = Sequence::new(8, Draw::DEFAULT);
        for _ in 0..6 {
            sequence.clock(0, Draw::DEFAULT);
        }
        sequence.set_length(4);
        assert_eq!(sequence.clock(0, Draw::DEFAULT), 0);
        sequence.set_length(0);
        assert_eq!(sequence.length(), 1);
        sequence.set_length(200);
        assert_eq!(usize::from(sequence.length()), MAX_STEPS);
    }

    #[test]
    fn no_chance_repeats_the_loop_full_chance_changes_it() {
        let mut sequence = Sequence::new(8, Draw::DEFAULT);
        let first = *sequence.places();
        for _ in 0..64 {
            sequence.clock(0, Draw::DEFAULT);
        }
        assert_eq!(*sequence.places(), first, "0 %: the same loop");
        for _ in 0..8 {
            sequence.clock(100, Draw::DEFAULT);
        }
        assert_ne!(sequence.places()[..8], first[..8], "100 %: new steps");
        assert_eq!(
            sequence.places()[8..],
            first[8..],
            "past the loop: untouched"
        );
    }

    #[test]
    fn the_same_start_replays_the_same_melody() {
        let run = || {
            let mut sequence = Sequence::new(16, Draw::DEFAULT);
            let places: [u16; 100] = core::array::from_fn(|_| {
                let step = sequence.clock(30, Draw::DEFAULT);
                sequence.place(step)
            });
            places
        };
        assert_eq!(run(), run());
    }

    #[test]
    fn a_window_of_no_width_plays_the_root() {
        let draw = Draw {
            window: Window {
                spread: 0,
                ..Window::DEFAULT
            },
            ..Draw::DEFAULT
        };
        let sequence = Sequence::new(8, draw);
        assert!(sequence.places().iter().all(|&place| draw.note(place) == 0));
    }

    #[test]
    fn widening_and_narrowing_back_gives_the_same_notes() {
        let sequence = Sequence::new(16, Draw::DEFAULT);
        let notes = |spread| {
            let draw = Draw {
                window: Window {
                    spread,
                    ..Window::DEFAULT
                },
                ..Draw::DEFAULT
            };
            sequence.places().map(|place| draw.note(place))
        };
        let wide = notes(24);
        assert_ne!(notes(6), wide, "narrower: other notes");
        assert_eq!(notes(24), wide, "back: the same melody");
    }

    proptest! {
        #[test]
        fn every_drawn_note_is_in_the_scale_and_the_window(draw in draws(), chance in 0..=100_u8) {
            let mut sequence = Sequence::new(16, draw);
            for _ in 0..64 {
                sequence.clock(chance, draw);
            }
            let (lowest, highest) = draw.window.bounds(draw.scale);
            for &place in sequence.places() {
                let note = draw.note(place);
                prop_assert!(draw.scale.contains(note), "{note} in {:?}", draw.scale);
                prop_assert!((lowest..=highest).contains(&note), "{note}");
            }
        }

        #[test]
        fn a_walk_never_repeats_in_a_wide_window(draw in draws(), length in 2..=32_u8) {
            let draw = Draw {
                jump: draw.jump.max(1),
                window: Window { spread: draw.window.spread.max(12), ..draw.window },
                ..draw
            };
            let sequence = Sequence::new(length, draw);
            for pair in sequence.places().windows(2) {
                prop_assert_ne!(draw.note(pair[0]), draw.note(pair[1]));
            }
        }
    }
}
