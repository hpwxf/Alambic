//! `MELODY`'s steps, where the clock is in them, and how a new note is
//! drawn (journal/D-047).
//!
//! A step holds a note in semitones above the root, unquantised, so that a
//! scale changed and changed back gives the same melody. Each step the clock
//! reaches may be redrawn first, with a chance: never, the loop repeats;
//! always, every note is new. A draw is either from the scale's notes over
//! a range, spread around a centre, or a walk from the step before.
//!
//! Every draw comes from one xorshift generator seeded the same at start, so
//! a run replays exactly, on every backend.

use super::scale::Scale;

/// The most steps a sequence has.
pub const MAX_STEPS: usize = 32;

/// The highest note a step holds: four octaves above the root.
pub const MAX_NOTE: u8 = 48;

/// The most octaves a draw spans.
pub const MAX_RANGE: u8 = 4;

/// The longest walk from one step to the next, in degrees.
pub const MAX_JUMP: u8 = 7;

/// Draws a note equal to the one before is retried this many times without
/// `REPEAT`; past them, the repeat stands (a spread of 0 always repeats).
const REPEAT_RETRIES: u32 = 4;

/// The generator's state at start.
const SEED: u32 = 0x9E37_79B9;

/// How a new note is drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Draw {
    /// The scale its note is in.
    pub scale: Scale,
    /// Octaves above the root it may reach, 1 to [`MAX_RANGE`].
    pub range_octaves: u8,
    /// 100 % spreads the notes evenly over the range; less gathers them
    /// round `bias_percent`, 0 % on it.
    pub spread_percent: u8,
    /// Where in the range the notes gather, from its bottom (0 %) to its
    /// top (100 %).
    pub bias_percent: u8,
    /// A walk of up to so many degrees from the step before, never none;
    /// 0 draws over the range instead.
    pub jump: u8,
    /// Whether a note may be the same as the step before's.
    pub repeat: bool,
}

impl Draw {
    /// Over two octaves of the major scale, evenly, repeats allowed.
    pub const DEFAULT: Self = Self {
        scale: Scale::Major,
        range_octaves: 2,
        spread_percent: 100,
        bias_percent: 50,
        jump: 0,
        repeat: true,
    };

    /// The highest degree of the range: its top is an octave of the root,
    /// always in the scale.
    fn top_degree(self) -> i32 {
        self.scale.notes_per_octave() * i32::from(self.range_octaves.clamp(1, MAX_RANGE))
    }
}

/// The steps, the one playing, and the generator.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Sequence {
    notes: [u8; MAX_STEPS],
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
            notes: [0; MAX_STEPS],
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
    /// keeps its note for when the loop grows again.
    pub fn set_length(&mut self, length: u8) {
        self.length = length.clamp(1, u8::try_from(MAX_STEPS).unwrap_or(u8::MAX));
    }

    /// The step playing, `None` while armed.
    #[must_use]
    pub fn position(&self) -> Option<usize> {
        self.position.map(usize::from)
    }

    /// The note step `index` holds, in semitones above the root; 0 past the
    /// last.
    #[must_use]
    pub fn note(&self, index: usize) -> u8 {
        self.notes.get(index).copied().unwrap_or(0)
    }

    /// Every step's note, those past the loop too.
    #[must_use]
    pub const fn notes(&self) -> &[u8; MAX_STEPS] {
        &self.notes
    }

    /// Sets step `index`'s note, at most [`MAX_NOTE`]; nothing past the
    /// last step.
    pub fn set_note(&mut self, index: usize, note: u8) {
        if let Some(held) = self.notes.get_mut(index) {
            *held = note.min(MAX_NOTE);
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
        let previous = i32::from(self.notes[before.min(MAX_STEPS - 1)]);
        let note = self.drawn(previous, draw);
        self.notes[index] = u8::try_from(note.clamp(0, i32::from(MAX_NOTE))).unwrap_or(0);
    }

    /// A new note after `previous`, in semitones above the root.
    fn drawn(&mut self, previous: i32, draw: Draw) -> i32 {
        let top = draw.top_degree();
        let scale = draw.scale;
        let from = scale.degree_of(previous).clamp(0, top);
        let degree = if draw.jump > 0 {
            self.walked(from, top, i32::from(draw.jump.min(MAX_JUMP)))
        } else {
            let mut degree = self.spread(top, draw);
            let mut retries = 0;
            while !draw.repeat && degree == from && retries < REPEAT_RETRIES {
                degree = self.spread(top, draw);
                retries += 1;
            }
            degree
        };
        scale.note_of(degree)
    }

    /// A degree from 0 to `top`: even at a spread of 100 %, a triangle round
    /// the bias below, as wide as the spread.
    fn spread(&mut self, top: i32, draw: Draw) -> i32 {
        let span = top + 1;
        if draw.spread_percent >= 100 {
            return self.below(span);
        }
        let centre = top * i32::from(draw.bias_percent.min(100)) / 100;
        let half = top * i32::from(draw.spread_percent) / 100;
        // The sum of two even draws over `-half..=half`, halved.
        let width = 2 * half + 1;
        let offset = self.below(width) + self.below(width) - 2 * half;
        (centre + offset / 2).clamp(0, top)
    }

    /// `from` moved by 1 to `jump` degrees, up or down, within 0 to `top`:
    /// away from a bound it would cross, never staying put.
    fn walked(&mut self, from: i32, top: i32, jump: i32) -> i32 {
        let by = 1 + self.below(jump);
        let up = self.random() & 1 == 0;
        let (first, second) = if up {
            (from + by, from - by)
        } else {
            (from - by, from + by)
        };
        if (0..=top).contains(&first) {
            first
        } else if (0..=top).contains(&second) {
            second
        } else if up {
            // A range narrower than the jump: the far end.
            if from == top { 0 } else { top }
        } else if from == 0 {
            top
        } else {
            0
        }
    }

    /// Whether a step with `chance_percent` is redrawn.
    fn roll(&mut self, chance_percent: u8) -> bool {
        match chance_percent {
            0 => false,
            100.. => true,
            chance => self.random() % 100 < u32::from(chance),
        }
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

    use super::{Draw, MAX_JUMP, MAX_NOTE, MAX_RANGE, MAX_STEPS, Sequence};
    use crate::apps::melody::scale::Scale;

    fn draws() -> impl Strategy<Value = Draw> {
        (
            0..Scale::ALL.len(),
            1..=MAX_RANGE,
            0..=100_u8,
            0..=100_u8,
            0..=MAX_JUMP,
            any::<bool>(),
        )
            .prop_map(|(scale, range, spread, bias, jump, repeat)| Draw {
                scale: Scale::from_index(scale),
                range_octaves: range,
                spread_percent: spread,
                bias_percent: bias,
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
        let first = *sequence.notes();
        for _ in 0..64 {
            sequence.clock(0, Draw::DEFAULT);
        }
        assert_eq!(*sequence.notes(), first, "0 %: the same loop");
        for _ in 0..8 {
            sequence.clock(100, Draw::DEFAULT);
        }
        assert_ne!(sequence.notes()[..8], first[..8], "100 %: new notes");
        assert_eq!(
            sequence.notes()[8..],
            first[8..],
            "past the loop: untouched"
        );
    }

    #[test]
    fn the_same_start_replays_the_same_melody() {
        let run = || {
            let mut sequence = Sequence::new(16, Draw::DEFAULT);
            let notes: [u8; 100] = core::array::from_fn(|_| {
                let step = sequence.clock(30, Draw::DEFAULT);
                sequence.note(step)
            });
            notes
        };
        assert_eq!(run(), run());
    }

    #[test]
    fn no_spread_draws_the_bias() {
        let draw = Draw {
            spread_percent: 0,
            bias_percent: 50,
            ..Draw::DEFAULT
        };
        let sequence = Sequence::new(8, draw);
        // Two octaves of the major scale: degree 7 in the middle, C an
        // octave up.
        assert!(sequence.notes().iter().all(|&note| note == 12));
    }

    #[test]
    fn edited_notes_stay_within_the_top() {
        let mut sequence = Sequence::new(4, Draw::DEFAULT);
        sequence.set_note(1, 200);
        assert_eq!(sequence.note(1), MAX_NOTE);
        sequence.set_note(MAX_STEPS, 3);
        assert_eq!(sequence.note(MAX_STEPS), 0, "no such step");
    }

    proptest! {
        #[test]
        fn every_drawn_note_is_in_the_scale_and_the_range(draw in draws(), chance in 0..=100_u8) {
            let mut sequence = Sequence::new(16, draw);
            for _ in 0..64 {
                sequence.clock(chance, draw);
            }
            for &note in sequence.notes() {
                prop_assert!(draw.scale.contains(i32::from(note)), "{note} in {:?}", draw.scale);
                prop_assert!(note <= 12 * draw.range_octaves, "{note}");
            }
        }

        #[test]
        fn a_walk_never_repeats(draw in draws(), length in 2..=32_u8) {
            let draw = Draw { jump: draw.jump.max(1), ..draw };
            let sequence = Sequence::new(length, draw);
            for pair in sequence.notes().windows(2) {
                prop_assert_ne!(pair[0], pair[1]);
            }
        }
    }
}
