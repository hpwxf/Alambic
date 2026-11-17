//! Patterns: what a sequencer shows (journal/D-030).

use super::Point;
use super::trig::{TURN, polar};
use super::{Ink, circle, fill_circle, fill_rect, rect, vline};
use crate::framebuffer::FrameBuffer;

/// A row of `size`-pixel squares, `gap` apart: filled for a set step, hollow
/// otherwise; the `current` one gets a second outline around it.
pub fn steps(
    frame: &mut FrameBuffer,
    at: Point,
    size: i32,
    gap: i32,
    pattern: &[bool],
    current: Option<usize>,
) {
    let mut x = at.x;
    for (index, &set) in pattern.iter().enumerate() {
        if set {
            fill_rect(frame, x, at.y, size, size, Ink::Lit);
        } else {
            rect(frame, x, at.y, size, size, Ink::Lit);
        }
        if current == Some(index) {
            rect(frame, x - 2, at.y - 2, size + 4, size + 4, Ink::Lit);
        }
        x += size + gap;
    }
}

/// A row of dots of `radius`, `gap` apart, filled for a set step.
pub fn dots(frame: &mut FrameBuffer, at: Point, radius: i32, gap: i32, pattern: &[bool]) {
    let mut x = at.x + radius;
    for &set in pattern {
        let center = Point::new(x, at.y + radius);
        if set {
            fill_circle(frame, center, radius, Ink::Lit);
        } else {
            circle(frame, center, radius, Ink::Lit);
        }
        x += 2 * radius + 1 + gap;
    }
}

/// A Euclidean ring: the steps as dots of `dot` radius around `center`, the
/// first at the top and the others clockwise; hits filled, rests hollow, and
/// the `current` step marked by a ring around its dot.
pub fn euclid_ring(
    frame: &mut FrameBuffer,
    center: Point,
    radius: i32,
    dot: i32,
    pattern: &[bool],
    current: Option<usize>,
) {
    let count = i32::try_from(pattern.len()).unwrap_or(0);
    if count == 0 {
        return;
    }
    for (index, &hit) in (0..count).zip(pattern) {
        // From the top (a quarter turn), clockwise (decreasing angles).
        let angle = TURN / 4 - index * TURN / count;
        let at = polar(center, radius, angle);
        if hit {
            fill_circle(frame, at, dot, Ink::Lit);
        } else {
            circle(frame, at, dot, Ink::Lit);
        }
        if current.and_then(|step| i32::try_from(step).ok()) == Some(index) {
            circle(frame, at, dot + 2, Ink::Lit);
        }
    }
}

/// White keys of an octave, and where the black ones sit (after which white
/// key), as semitone numbers.
const WHITE: [u8; 7] = [0, 2, 4, 5, 7, 9, 11];
const BLACK: [(u8, i32); 5] = [(1, 0), (3, 1), (6, 3), (8, 4), (10, 5)];

/// Width of a white key, its separating line included.
const KEY: i32 = 4;

/// A one-octave keyboard, `KEY * 7 + 1` columns wide and `height` rows high,
/// with `note` (a semitone, 0 for C) inverted. Returns its width.
pub fn keyboard(frame: &mut FrameBuffer, at: Point, height: i32, note: Option<u8>) -> i32 {
    let width = KEY * 7 + 1;
    rect(frame, at.x, at.y, width, height, Ink::Lit);
    for (index, &semitone) in (0..).zip(&WHITE) {
        let x = at.x + index * KEY;
        if index > 0 {
            vline(frame, x, at.y, height, Ink::Lit);
        }
        if note == Some(semitone) {
            fill_rect(frame, x + 1, at.y + 1, KEY - 1, height - 2, Ink::Invert);
        }
    }
    let black_height = height * 3 / 5;
    for &(semitone, after) in &BLACK {
        let x = at.x + (after + 1) * KEY - 1;
        let ink = if note == Some(semitone) {
            Ink::Dark
        } else {
            Ink::Lit
        };
        // A lit key, darkened when it is the note: inverted against the
        // white keys around it.
        fill_rect(frame, x, at.y, 3, black_height, Ink::Lit);
        if ink == Ink::Dark {
            fill_rect(frame, x + 1, at.y + 1, 1, black_height - 2, Ink::Dark);
        }
    }
    width
}
