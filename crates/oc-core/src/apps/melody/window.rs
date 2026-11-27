//! Where `MELODY`'s steps sound (journal/D-047, revision of 2026-11-27).
//!
//! A step keeps a *place*, from 0 to [`PLACE_MAX`], drawn once; the window
//! turns it into a pitch at the outputs. Every setting of the window acts at
//! once on the whole loop, and every one is monotone in the place, so a step
//! higher than another stays higher: the melody's contour survives whatever
//! is turned, and turning a setting back gives the same melody again.
//!
//! * `SPREAD` is the window's width, 0 to [`MAX_SPREAD`] semitones;
//! * `CENTER` its place around the root, −100 % (it ends on the root) to
//!   +100 % (it starts on the root), 0 centred on it;
//! * `BIAS` a curve over the place with fixed ends, −100 % (the notes
//!   crowd towards the bottom) to +100 % (towards the top), 0 straight.
//!
//! Quantising to the scale comes after, so steps close in place may share a
//! note while the window is narrow, and part again when it widens. The
//! place a note comes from is found back by bisection, which the
//! monotony makes exact ([`Window::place_of`]).

use super::scale::Scale;
use crate::fixed::{ONE, exp2, log2};

/// The highest place: the top of the window.
pub const PLACE_MAX: u16 = u16::MAX;

/// The widest window, in semitones: four octaves.
pub const MAX_SPREAD: u8 = 48;

/// The bound of `CENTER` and `BIAS`, either way, in percent.
pub const MAX_PERCENT: i8 = 100;

/// How far `BIAS` bends the place at its bounds: an exponent from
/// `2^-BIAS_OCTAVES` to `2^BIAS_OCTAVES`, from a quarter to four.
const BIAS_OCTAVES: i64 = 2;

/// The window the steps' places sound in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Window {
    /// Its width, 0 to [`MAX_SPREAD`] semitones.
    pub spread: u8,
    /// Its place around the root, −100 to +100 %.
    pub center: i8,
    /// The curve over the place, −100 to +100 %.
    pub bias: i8,
}

impl Window {
    /// Two octaves centred on the root, straight.
    pub const DEFAULT: Self = Self {
        spread: 24,
        center: 0,
        bias: 0,
    };

    /// `place` bent by the bias, in 16.16 from 0 to one: `u^(2^(−2·b))`,
    /// its ends fixed.
    fn curved(self, place: u16) -> i64 {
        let one = i64::from(ONE);
        let u = i64::from(place) * one / i64::from(PLACE_MAX);
        if self.bias == 0 || u >= one {
            return u;
        }
        let Some(log) = log2(u) else {
            // The bottom place stays at the bottom.
            return 0;
        };
        let bias = i64::from(self.bias.clamp(-MAX_PERCENT, MAX_PERCENT));
        let exponent = exp2(-BIAS_OCTAVES * bias * one / i64::from(MAX_PERCENT));
        exp2(log * exponent / one).clamp(0, one)
    }

    /// The pitch of `place`, in semitones from the root, rounded to the
    /// nearest: before the transposition and the scale.
    #[must_use]
    pub fn pitch(self, place: u16) -> i32 {
        let one = i64::from(ONE);
        let spread = i64::from(self.spread.min(MAX_SPREAD));
        let center = i64::from(self.center.clamp(-MAX_PERCENT, MAX_PERCENT));
        let full = 2 * i64::from(MAX_PERCENT);
        // The bottom: half the width below the root at 0, none at +100 %,
        // all of it at −100 %.
        let low = -spread * one * (i64::from(MAX_PERCENT) - center) / full;
        let pitch = low + self.curved(place) * spread;
        i32::try_from((pitch + one / 2).div_euclid(one)).unwrap_or(0)
    }

    /// The note of `place` in `scale`, in semitones from the root.
    #[must_use]
    pub fn note(self, scale: Scale, place: u16) -> i32 {
        scale.quantize(self.pitch(place))
    }

    /// The lowest and the highest note the window gives in `scale`.
    #[must_use]
    pub fn bounds(self, scale: Scale) -> (i32, i32) {
        (self.note(scale, 0), self.note(scale, PLACE_MAX))
    }

    /// The place that gives `note` in `scale`, the middle of the places that
    /// do; past the window's notes, the nearest it gives. A note the window
    /// skips (the bias steep near an end) gives the next one up.
    #[must_use]
    pub fn place_of(self, scale: Scale, note: i32) -> u16 {
        let (lowest, highest) = self.bounds(scale);
        let target = note.clamp(lowest, highest);
        // The first place at or above the target, then the last one with
        // the same note: the note's span of places.
        let first = Self::first_place(|place| self.note(scale, place) >= target);
        let found = self.note(scale, first);
        let past = Self::first_place(|place| self.note(scale, place) > found);
        let last = if past > first { past - 1 } else { first };
        first + (last - first) / 2
    }

    /// The lowest place for which `above` holds, [`PLACE_MAX`] when none
    /// does: `above` false then true as the place grows.
    fn first_place(above: impl Fn(u16) -> bool) -> u16 {
        let (mut low, mut high) = (0_u32, u32::from(PLACE_MAX) + 1);
        while low < high {
            let middle = low + (high - low) / 2;
            if above(u16::try_from(middle).unwrap_or(PLACE_MAX)) {
                high = middle;
            } else {
                low = middle + 1;
            }
        }
        u16::try_from(low).unwrap_or(PLACE_MAX)
    }
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::{MAX_SPREAD, PLACE_MAX, Window};
    use crate::apps::melody::scale::Scale;

    fn window(spread: u8, center: i8, bias: i8) -> Window {
        Window {
            spread,
            center,
            bias,
        }
    }

    #[test]
    fn the_center_places_the_window_round_the_root() {
        let ends = |center| {
            let window = window(24, center, 0);
            (window.pitch(0), window.pitch(PLACE_MAX))
        };
        assert_eq!(ends(0), (-12, 12), "centred");
        assert_eq!(ends(100), (0, 24), "starting on the root");
        assert_eq!(ends(-100), (-24, 0), "ending on the root");
        assert_eq!(ends(50), (-6, 18));
        assert_eq!(
            window(0, 0, 0).pitch(PLACE_MAX / 2),
            0,
            "no width: the root"
        );
    }

    #[test]
    fn the_bias_bends_the_middle_and_keeps_the_ends() {
        let middle = |bias| window(24, 100, bias).pitch(PLACE_MAX / 2);
        assert_eq!(middle(0), 12, "straight");
        assert!(middle(100) > 12, "towards the top: {}", middle(100));
        assert!(middle(-100) < 12, "towards the bottom: {}", middle(-100));
        for bias in [-100, -50, 50, 100] {
            let window = window(24, 100, bias);
            assert_eq!(
                (window.pitch(0), window.pitch(PLACE_MAX)),
                (0, 24),
                "{bias}"
            );
        }
    }

    #[test]
    fn a_note_found_back_is_the_note() {
        let window = window(24, 0, 30);
        for note in -12..=12 {
            if Scale::Major.contains(note) {
                let place = window.place_of(Scale::Major, note);
                assert_eq!(window.note(Scale::Major, place), note, "{note}");
            }
        }
        assert_eq!(
            window.note(Scale::Major, window.place_of(Scale::Major, 40)),
            12,
            "past the top: the top"
        );
    }

    proptest! {
        #[test]
        fn the_projection_is_monotone(
            spread in 0..=MAX_SPREAD,
            center in -100_i8..=100,
            bias in -100_i8..=100,
            a in any::<u16>(),
            b in any::<u16>(),
        ) {
            let window = window(spread, center, bias);
            let (low, high) = if a <= b { (a, b) } else { (b, a) };
            prop_assert!(window.pitch(low) <= window.pitch(high));
        }

        #[test]
        fn a_place_found_back_gives_its_note(
            spread in 0..=MAX_SPREAD,
            center in -100_i8..=100,
            bias in -100_i8..=100,
            place in any::<u16>(),
            scale in 0..Scale::ALL.len(),
        ) {
            let window = window(spread, center, bias);
            let scale = Scale::from_index(scale);
            let note = window.note(scale, place);
            prop_assert_eq!(window.note(scale, window.place_of(scale, note)), note);
        }
    }
}
