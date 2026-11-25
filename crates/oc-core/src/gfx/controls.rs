//! Controls: what an applet shows for a parameter (journal/D-030).
//!
//! The shapes follow the screens of Squares-and-Circles (a knob is a ring with
//! a pointer, a selected value sits in an inverted box, a caption in a small
//! font tops a value), drawn with the bases of [`super`] and [`crate::text`].
//! Every control takes its fonts from the caller's [`TextStyle`]s.

use super::Point;
use super::trig::{TURN, polar};
use super::{Ink, Ratio, circle, dotted_vline, fill_rect, hline, line, pixel, rect, vline};
use crate::framebuffer::{FrameBuffer, ROW_HEIGHT, WIDTH_I32};
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

/// Where every applet's title starts on its top row: right of the column
/// where the engine marks the instances running, a pixel apart, where a
/// list's rows start (journal/D-046).
pub const TITLE_LEFT: i32 = LIST_LEFT;

/// The instances running, one mark each down the left column of the top
/// row, as wide as a list's scroll bar ([`SCROLL_WIDTH`]) and in line with
/// it: the one with the focus across the whole width, the others a pixel on
/// the bar's track. Drawn over an applet's screen or its routing page, left
/// of the title ([`TITLE_LEFT`]), when more than one runs (journal/D-046).
pub fn instance_marks(frame: &mut FrameBuffer, count: usize, focus: usize) {
    if count < 2 {
        return;
    }
    fill_rect(frame, 0, 0, SCROLL_WIDTH, ROW_HEIGHT, Ink::Dark);
    for position in 0..count.min(4) {
        let y = i32::try_from(position * 2).unwrap_or(0);
        if position == focus {
            fill_rect(frame, 0, y, SCROLL_WIDTH, 1, Ink::Lit);
        } else {
            fill_rect(frame, SCROLL_WIDTH / 2, y, 1, 1, Ink::Lit);
        }
    }
}

/// The title bar: `title` at the top left ([`TITLE_LEFT`]), one dot per page
/// at the right (the current one filled), a line under both. Returns the
/// first free row.
pub fn header(
    frame: &mut FrameBuffer,
    title: &str,
    page: usize,
    pages: usize,
    style: TextStyle,
) -> i32 {
    text::draw(frame, title, Point::new(TITLE_LEFT, 0), style);
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

/// Width of the scroll bar at the left edge of a list, the same on every
/// screen that has one so that their rows line up from one screen to the next.
pub const SCROLL_WIDTH: i32 = 3;

/// Left edge of a list's rows, right of the scroll bar and a gap.
pub const LIST_LEFT: i32 = SCROLL_WIDTH + 1;

/// The band over the selected row of a list, `height` high from `top`, from
/// [`LIST_LEFT`] to the right edge: the scroll bar stays outside it.
pub fn row_highlight(frame: &mut FrameBuffer, top: i32, height: i32, ink: Ink) {
    fill_rect(frame, LIST_LEFT, top, WIDTH_I32 - LIST_LEFT, height, ink);
}

/// The track of a [`scroll_bar`] without its thumb: the place of one on a
/// screen with a single page, so that it lines up with the screens that
/// scroll.
pub fn scroll_track(frame: &mut FrameBuffer, x: i32, y: i32, width: i32, height: i32) {
    dotted_vline(frame, x + width / 2, y, height, 2, Ink::Lit);
}

/// A scroll bar `width` pixels wide and `height` high from `(x, y)`, for a
/// list shown `pages` at a time: a dotted track down its middle, and a thumb
/// over the share of it that `page`, from 0, takes.
pub fn scroll_bar(
    frame: &mut FrameBuffer,
    x: i32,
    y: i32,
    width: i32,
    height: i32,
    page: usize,
    pages: usize,
) {
    scroll_window(frame, x, y, width, height, page, 1, pages.max(1));
}

/// A scroll bar for a list of `total` items of which `shown`, from `first`,
/// are on screen: the thumb's length is the share shown and its place the
/// share above it, so that a window sliding one item at a time reads as
/// such, not as a page. A window past the end is held at the last place.
#[allow(clippy::too_many_arguments)]
// A box and a window: grouping either into a struct for its only callers
// would add a type without making a call any clearer.
pub fn scroll_window(
    frame: &mut FrameBuffer,
    x: i32,
    y: i32,
    width: i32,
    height: i32,
    first: usize,
    shown: usize,
    total: usize,
) {
    let total = total.max(1);
    let shown = shown.clamp(1, total);
    let first = first.min(total - shown);
    let share = |items: usize| {
        let items = i32::try_from(items).unwrap_or(i32::MAX);
        let total = i32::try_from(total).unwrap_or(i32::MAX);
        items.saturating_mul(height) / total
    };
    scroll_track(frame, x, y, width, height);
    let top = y + share(first);
    let bottom = y + share(first + shown);
    fill_rect(frame, x, top, width, bottom - top, Ink::Lit);
}

/// The ink a text style draws with, for the shapes beside its text.
const fn ink_of(style: TextStyle) -> Ink {
    style.color
}
