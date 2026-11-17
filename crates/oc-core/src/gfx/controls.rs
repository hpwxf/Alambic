//! Controls: what an applet shows for a parameter (journal/D-030).
//!
//! The shapes follow the screens of Squares-and-Circles (a knob is a ring with
//! a pointer, a selected value sits in an inverted box, a caption in a small
//! font tops a value), drawn with the bases of [`super`] and [`crate::text`].
//! Every control takes its fonts from the caller's [`TextStyle`]s.

use super::Point;
use super::trig::{TURN, polar};
use super::{Ink, Ratio, circle, fill_rect, hline, line, pixel, rect, vline};
use crate::framebuffer::{FrameBuffer, WIDTH_I32};
use crate::text::{self, Baseline, TextStyle};

/// Where a knob's pointer starts, at the bottom left (seven-thirty).
const KNOB_START: i32 = TURN * 5 / 8;

/// How far a knob's pointer turns from empty to full: three quarters.
const KNOB_SWEEP: i32 = TURN * 3 / 4;

/// A knob: a ring of `radius` around `center` and a pointer from the centre,
/// turned clockwise from seven-thirty (empty) to four-thirty (full).
pub fn knob(frame: &mut FrameBuffer, center: Point, radius: i32, value: Ratio, ink: Ink) {
    circle(frame, center, radius, ink);
    let angle = KNOB_START - value.of(KNOB_SWEEP);
    let tip = polar(center, radius - 1, angle);
    line(frame, center, tip, ink);
}

/// A knob followed by its label, centred on the knob's row, as S&C lays a
/// parameter: `at` is the knob's centre; returns the column after the label.
pub fn labelled_knob(
    frame: &mut FrameBuffer,
    at: Point,
    radius: i32,
    value: Ratio,
    label: &str,
    style: TextStyle,
) -> i32 {
    knob(frame, at, radius, value, ink_of(style));
    let x = at.x + radius + 3;
    text::draw(
        frame,
        label,
        Point::new(x, at.y),
        style.with_baseline(Baseline::Middle),
    );
    x + style.font.width_of(label)
}

/// A selector: an up-and-down arrow, then the selected `value`; `at` is the
/// top-left corner of the text, the arrow sits before it, centred on the
/// font's height. Returns the column after the value.
pub fn selector(frame: &mut FrameBuffer, at: Point, value: &str, style: TextStyle) -> i32 {
    let ink = ink_of(style);
    let middle = at.y + style.font.height() / 2;
    // Two three-column triangles, tip up above the middle row, tip down
    // below it.
    pixel(frame, at.x + 1, middle - 3, ink);
    hline(frame, at.x, middle - 2, 3, ink);
    hline(frame, at.x, middle, 3, ink);
    pixel(frame, at.x + 1, middle + 1, ink);
    let x = at.x + 5;
    text::draw(frame, value, Point::new(x, at.y), style);
    x + style.font.width_of(value)
}

/// `value` dark in a lit box one pixel wider all round, as S&C shows a
/// selected setting; `at` is the box's top-left corner. Returns its width.
pub fn value_box(frame: &mut FrameBuffer, at: Point, value: &str, style: TextStyle) -> i32 {
    let width = style.font.width_of(value) + 2;
    let height = style.font.height() + 2;
    fill_rect(frame, at.x, at.y, width, height, Ink::Lit);
    text::draw(
        frame,
        value,
        Point::new(at.x + 1, at.y + 1),
        TextStyle::dark(style.font),
    );
    width
}

/// A caption in a small font above a value: `at` is the caption's top-left
/// corner; the value starts on the row below the caption.
pub fn caption_value(
    frame: &mut FrameBuffer,
    at: Point,
    caption: &str,
    value: &str,
    caption_style: TextStyle,
    value_style: TextStyle,
) {
    text::draw(frame, caption, at, caption_style);
    let below = Point::new(at.x, at.y + caption_style.font.height());
    text::draw(frame, value, below, value_style);
}

/// A horizontal bar: an outline and, inside it, a fill from the left in
/// proportion to `value`.
pub fn hbar(frame: &mut FrameBuffer, x: i32, y: i32, width: i32, height: i32, value: Ratio) {
    rect(frame, x, y, width, height, Ink::Lit);
    fill_rect(
        frame,
        x + 1,
        y + 1,
        value.of(width - 2),
        height - 2,
        Ink::Lit,
    );
}

/// A vertical bar: an outline and a fill from the bottom.
pub fn vbar(frame: &mut FrameBuffer, x: i32, y: i32, width: i32, height: i32, value: Ratio) {
    rect(frame, x, y, width, height, Ink::Lit);
    let filled = value.of(height - 2);
    fill_rect(
        frame,
        x + 1,
        y + height - 1 - filled,
        width - 2,
        filled,
        Ink::Lit,
    );
}

/// A bipolar bar: a centre mark, and a fill from the centre towards the
/// right for a positive `value`, the left for a negative one, `value`
/// being within `-max..=max`.
pub fn bipolar_hbar(
    frame: &mut FrameBuffer,
    x: i32,
    y: i32,
    width: i32,
    height: i32,
    value: Ratio,
) {
    rect(frame, x, y, width, height, Ink::Lit);
    let centre = x + width / 2;
    vline(frame, centre, y - 1, height + 2, Ink::Lit);
    let half = width / 2 - 1;
    let length = Ratio::new(value.value.abs(), value.max).of(half);
    if value.value >= 0 {
        fill_rect(frame, centre + 1, y + 1, length, height - 2, Ink::Lit);
    } else {
        fill_rect(frame, centre - length, y + 1, length, height - 2, Ink::Lit);
    }
}

/// The title bar: `title` at the top left, one dot per page at the right (the
/// current one filled), a line under both. Returns the first free row.
pub fn header(
    frame: &mut FrameBuffer,
    title: &str,
    page: usize,
    pages: usize,
    style: TextStyle,
) -> i32 {
    text::draw(frame, title, Point::zero(), style);
    let height = style.font.height();
    let dot = 3;
    let mut x = WIDTH_I32 - 1 - dot / 2;
    for index in (0..pages).rev() {
        let center = Point::new(x, height / 2);
        if index == page {
            super::fill_circle(frame, center, dot / 2 + 1, Ink::Lit);
        } else {
            circle(frame, center, dot / 2 + 1, Ink::Lit);
        }
        x -= dot + 3;
    }
    hline(frame, 0, height + 1, WIDTH_I32, Ink::Lit);
    height + 3
}

/// The ink a text style draws with, for the shapes beside its text.
const fn ink_of(style: TextStyle) -> Ink {
    style.color
}
