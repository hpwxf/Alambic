//! The submenu that edits an [`Assign`] (journal/D-035, D-036): a frame
//! over the applet's screen, its top edge carrying the parameter and its
//! value now; the input, scale and shift read as a formula, the bounds on
//! one line, the effect and a `CENTER` button; a bar of the input's signal
//! and one of the parameter it moves.
//!
//! Panel: the left encoder picks a field, the right one turns it; the right
//! encoder's click closes the submenu, or applies `CENTER` when that is
//! picked (`down` is the panel's, journal/D-046). Opening it is the applet's (`RATCHET`: the
//! right encoder's click on a setting).

use super::{Assign, AssignLine, Window, usable_range};
use crate::apps::InputSnapshot;
use crate::fmt::TextBuf;
use crate::framebuffer::{FrameBuffer, ROW_HEIGHT, WIDTH_I32};
use crate::gfx::{self, Ink, Point};
use crate::platform::{Button, MilliVolts};
use crate::text::fonts::FONT_5X8;
use crate::text::{self, Alignment, TextStyle};

/// The submenu's frame: from under its title to above the bottom of the
/// screen, where the applet's screen stays in view under it.
const OVERLAY_TOP: i32 = 3;

/// Height of the submenu's frame.
const OVERLAY_HEIGHT: i32 = 51;

/// Top of the submenu's first line, under the title in its frame.
const OVERLAY_LINES_TOP: i32 = 7;

/// Left edge of the submenu's text and bars' labels, inside the frame.
const OVERLAY_LEFT: i32 = 4;

/// Right edge of the submenu's text and bars, inside the frame.
const OVERLAY_RIGHT: i32 = WIDTH_I32 - 5;

/// Left edge of the scale on the formula line, after the input, unless a
/// port's name pushes it right (journal/D-048).
const SCALE_LEFT: i32 = 34;

/// The gap between the input and the scale when a port's name is long.
const SCALE_GAP: i32 = 4;

/// The most letters of a port's name the submenu writes after its slot.
const PORT_NAME_LETTERS: usize = 5;

/// Left edge of the lower bound's value, after `MIN`.
const MIN_VALUE_LEFT: i32 = 26;

/// Left edge of the effect's value, after `EFFECT`.
const EFFECT_VALUE_LEFT: i32 = 46;

/// Left edge of `MAX`.
const MAX_LABEL_LEFT: i32 = 70;

/// Left edge of the bars, after their labels.
const BAR_LEFT: i32 = 26;

/// Height of a bar.
const BAR_HEIGHT: i32 = 7;

/// Top of the bar of the input's signal; the parameter's is one row under.
const SIGNAL_BAR_TOP: i32 = 33;

/// The range the bar of the input's signal spans, either side of 0 V.
const SIGNAL_BAR_MV: MilliVolts = 10_000;

/// What the submenu shows of the parameter it edits, given by the applet
/// at each drawing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Param<'a> {
    /// Its name, in the frame's top edge.
    pub name: &'a str,
    /// Its value now, inputs applied, in the frame's top edge.
    pub now: &'a str,
    /// Where its bar marks zero, in thousandths of its range: 500 for a
    /// bipolar parameter, 0 for the others.
    pub zero_permille: i32,
    /// Where the panel's value sits in its range, in thousandths.
    pub panel_permille: i32,
    /// Where its value now sits in its range, in thousandths.
    pub now_permille: i32,
}

/// The submenu open on one parameter: the field picked, the input's range
/// seen lately.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AssignEditor {
    line: usize,
    seen: Window,
}

impl AssignEditor {
    /// Opened on the input field, nothing seen yet.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            line: 0,
            seen: Window::new(),
        }
    }

    /// The field the right encoder turns.
    #[must_use]
    pub const fn line(&self) -> AssignLine {
        AssignLine::ALL[self.line]
    }

    /// The lowest and highest reading of the input over the last
    /// [`super::WINDOW_MICROS`] to twice that, `None` before the first.
    #[must_use]
    pub fn range(&self) -> Option<(MilliVolts, MilliVolts)> {
        self.seen.range()
    }

    /// Follows the panel for `assign`, whose parameter `sweep` sweeps end to
    /// end when it follows its input; returns whether the submenu stays
    /// open.
    pub fn follow(
        &mut self,
        assign: &mut Assign,
        input: &InputSnapshot,
        sweep: (MilliVolts, MilliVolts),
    ) -> bool {
        let click = input.buttons.pressed(Button::RightEncoder);
        if click && self.line() == AssignLine::Center {
            if let Some(seen) = self.seen.range() {
                assign.center(seen, usable_range(sweep, assign.effect));
            }
            return true;
        }
        // Closed by the right switch alone: `down` is the panel's
        // (journal/D-046).
        if click {
            return false;
        }
        // A port assigned has no shaping to set: its one line is the input.
        if assign.assigns() {
            self.line = 0;
        } else {
            let len = i32::try_from(AssignLine::ALL.len()).unwrap_or(1);
            let picks = i32::from(input.controls.delta(0));
            let line = i32::try_from(self.line).unwrap_or(0) + picks;
            self.line = usize::try_from(line.rem_euclid(len)).unwrap_or(0);
        }
        let turns = i32::from(input.controls.delta(1));
        if turns != 0 {
            let before = assign.input;
            assign.turn(self.line(), turns, &input.bus);
            if assign.input != before {
                self.seen = Window::new();
            }
        }
        true
    }

    /// Adds `reading`, what the input reads now, to the range seen,
    /// `elapsed` after the last.
    pub fn observe(&mut self, reading: MilliVolts, elapsed: u32) {
        self.seen.add(reading, elapsed);
    }

    /// Draws the submenu for `assign` over what the frame holds, its input
    /// reading `reading`; `port_name` names the port it reads, if it reads
    /// one (`1:NOTE`), `None` for one gone (`1:--`).
    pub fn render(
        &self,
        frame: &mut FrameBuffer,
        assign: &Assign,
        reading: MilliVolts,
        port_name: Option<&str>,
        param: &Param<'_>,
    ) {
        render_overlay_frame(frame, param.name, param.now);
        let line = self.line();
        let lit = TextStyle::lit(&FONT_5X8);
        let source = input_text(assign, port_name);
        if assign.assigns() {
            // An assignment, strictly: the port, and what it sets.
            let width = FONT_5X8.width_of(source.as_str());
            let top = overlay_row(0);
            gfx::fill_rect(
                frame,
                OVERLAY_LEFT - 2,
                top,
                width + 4,
                ROW_HEIGHT,
                Ink::Lit,
            );
            text::draw(
                frame,
                source.as_str(),
                Point::new(OVERLAY_LEFT, top),
                TextStyle::dark(&FONT_5X8),
            );
            text::draw(
                frame,
                "ASSIGNED",
                Point::new(OVERLAY_LEFT, overlay_row(1)),
                lit,
            );
            self.render_signal_bar(frame, assign, reading, SIGNAL_BAR_TOP);
            render_param_bar(frame, assign, param, SIGNAL_BAR_TOP + ROW_HEIGHT + 3);
            return;
        }
        for (shown, row, x) in [
            (AssignLine::Min, 1, OVERLAY_LEFT),
            (AssignLine::Max, 1, MAX_LABEL_LEFT),
            (AssignLine::Effect, 2, OVERLAY_LEFT),
        ] {
            text::draw(frame, shown.label(), Point::new(x, overlay_row(row)), lit);
        }
        let scale_left =
            SCALE_LEFT.max(OVERLAY_LEFT + FONT_5X8.width_of(source.as_str()) + SCALE_GAP);
        for (shown, row, x, align) in [
            (AssignLine::Input, 0, OVERLAY_LEFT, Alignment::Left),
            (AssignLine::Scale, 0, scale_left, Alignment::Left),
            (AssignLine::Shift, 0, OVERLAY_RIGHT, Alignment::Right),
            (AssignLine::Min, 1, MIN_VALUE_LEFT, Alignment::Left),
            (AssignLine::Max, 1, OVERLAY_RIGHT, Alignment::Right),
            (AssignLine::Effect, 2, EFFECT_VALUE_LEFT, Alignment::Left),
            (AssignLine::Center, 2, OVERLAY_RIGHT, Alignment::Right),
        ] {
            let value = if shown == AssignLine::Input {
                source
            } else {
                assign.field(shown)
            };
            let width = FONT_5X8.width_of(value.as_str());
            let left = if align == Alignment::Right {
                x - width
            } else {
                x
            };
            let top = overlay_row(row);
            let style = if shown == line {
                // The field the right encoder turns, in a lit box.
                gfx::fill_rect(frame, left - 2, top, width + 4, ROW_HEIGHT, Ink::Lit);
                TextStyle::dark(&FONT_5X8)
            } else {
                if shown == AssignLine::Center {
                    // An action: a button, outlined.
                    gfx::rect(frame, left - 2, top, width + 4, ROW_HEIGHT, Ink::Lit);
                }
                lit
            };
            text::draw(frame, value.as_str(), Point::new(left, top), style);
        }

        self.render_signal_bar(frame, assign, reading, SIGNAL_BAR_TOP);
        render_param_bar(frame, assign, param, SIGNAL_BAR_TOP + ROW_HEIGHT + 3);
    }

    /// The bar of the input's signal at `top`, −10 V to +10 V with a mark at
    /// 0 V, on the scale of what the parameter receives: the signal once
    /// scaled, shifted and bounded as a band from 0 V and a block, so that
    /// `SCALE` and `SHIFT` move them; the reading as it comes as a thin
    /// stroke, taller than the block, the lowest to highest reading seen as a
    /// line along the top; the bounds, when they bound anything, as brackets.
    fn render_signal_bar(
        &self,
        frame: &mut FrameBuffer,
        assign: &Assign,
        reading: MilliVolts,
        top: i32,
    ) {
        text::draw(
            frame,
            "CV",
            Point::new(OVERLAY_LEFT, top),
            TextStyle::lit(&FONT_5X8),
        );
        let width = OVERLAY_RIGHT - BAR_LEFT;
        let x_of = |millivolts: MilliVolts| {
            let clamped = millivolts.clamp(-SIGNAL_BAR_MV, SIGNAL_BAR_MV);
            BAR_LEFT + (clamped + SIGNAL_BAR_MV) * width / (2 * SIGNAL_BAR_MV)
        };
        render_bar_frame(frame, top, x_of(0));
        if !assign.assigns() && (assign.min_mv > -SIGNAL_BAR_MV || assign.max_mv < SIGNAL_BAR_MV) {
            for (x, inward) in [(x_of(assign.min_mv), 1), (x_of(assign.max_mv), -1)] {
                gfx::vline(frame, x, top - 1, BAR_HEIGHT + 2, Ink::Lit);
                gfx::pixel(frame, x + inward, top - 1, Ink::Lit);
                gfx::pixel(frame, x + inward, top + BAR_HEIGHT, Ink::Lit);
            }
        }
        if assign.input.is_none() {
            return;
        }
        if let Some((low, high)) = self.seen.range() {
            gfx::hline(frame, x_of(low), top, x_of(high) - x_of(low) + 1, Ink::Lit);
        }
        let shaped = x_of(assign.shaped(reading));
        let zero = x_of(0);
        let (left, right) = (zero.min(shaped), zero.max(shaped));
        gfx::fill_rect(frame, left, top + 2, right - left + 1, 3, Ink::Lit);
        gfx::fill_rect(frame, shaped - 1, top + 1, 3, BAR_HEIGHT - 2, Ink::Lit);
        gfx::vline(frame, x_of(reading), top - 1, BAR_HEIGHT + 2, Ink::Lit);
    }
}

impl Default for AssignEditor {
    fn default() -> Self {
        Self::new()
    }
}

/// The input field as the submenu writes it: `CV1`, or a port of another
/// instance by its slot and name, `1:NOTE` (journal/D-048), `1:--` for a
/// port that is no more.
fn input_text(assign: &Assign, port_name: Option<&str>) -> TextBuf<16> {
    let mut out = TextBuf::<16>::new();
    match assign.input {
        Some(source) => source.write_name(&mut out, port_name, PORT_NAME_LETTERS),
        None => out = assign.field(AssignLine::Input),
    }
    out
}

/// The bar of `param` at `top`, over its whole range: the panel's value as a
/// tick (unless the input sets it), the value now as a block, and the
/// stretch the input moves it as a band between.
fn render_param_bar(frame: &mut FrameBuffer, assign: &Assign, param: &Param<'_>, top: i32) {
    text::draw(
        frame,
        "VAL",
        Point::new(OVERLAY_LEFT, top),
        TextStyle::lit(&FONT_5X8),
    );
    let width = OVERLAY_RIGHT - BAR_LEFT;
    let x_of = |permille: i32| BAR_LEFT + permille.clamp(0, 1_000) * width / 1_000;
    render_bar_frame(frame, top, x_of(param.zero_permille));
    let now = x_of(param.now_permille);
    let from = if assign.sets() {
        x_of(param.zero_permille)
    } else {
        let panel = x_of(param.panel_permille);
        gfx::vline(frame, panel, top - 1, BAR_HEIGHT + 2, Ink::Lit);
        panel
    };
    let (left, right) = (from.min(now), from.max(now));
    gfx::fill_rect(frame, left, top + 2, right - left + 1, 3, Ink::Lit);
    gfx::fill_rect(frame, now - 1, top, 3, BAR_HEIGHT, Ink::Lit);
}

/// Top of line `row` (from 0) in the submenu's frame.
const fn overlay_row(row: i32) -> i32 {
    OVERLAY_LINES_TOP + row * ROW_HEIGHT
}

/// Clears the submenu's place and draws its frame, `label` and `now` each in
/// a gap of its top edge, a pixel clear either side.
fn render_overlay_frame(frame: &mut FrameBuffer, label: &str, now: &str) {
    let lit = TextStyle::lit(&FONT_5X8);
    gfx::fill_rect(
        frame,
        0,
        0,
        WIDTH_I32,
        OVERLAY_TOP + OVERLAY_HEIGHT + 1,
        Ink::Dark,
    );
    gfx::rect(frame, 0, OVERLAY_TOP, WIDTH_I32, OVERLAY_HEIGHT, Ink::Lit);
    gfx::fill_rect(
        frame,
        4,
        0,
        FONT_5X8.width_of(label) + 4,
        ROW_HEIGHT - 1,
        Ink::Dark,
    );
    text::draw(frame, label, Point::new(6, 0), lit);
    let right = WIDTH_I32 - 6;
    let width = FONT_5X8.width_of(now);
    gfx::fill_rect(
        frame,
        right - width - 2,
        0,
        width + 4,
        ROW_HEIGHT - 1,
        Ink::Dark,
    );
    text::draw(
        frame,
        now,
        Point::new(right, 0),
        lit.aligned(Alignment::Right),
    );
}

/// The outline of a bar at `top`, between the bars' edges, with a mark at
/// `zero` above and below.
fn render_bar_frame(frame: &mut FrameBuffer, top: i32, zero: i32) {
    gfx::hline(
        frame,
        BAR_LEFT,
        top + BAR_HEIGHT / 2,
        OVERLAY_RIGHT - BAR_LEFT + 1,
        Ink::Lit,
    );
    gfx::vline(frame, BAR_LEFT, top, BAR_HEIGHT, Ink::Lit);
    gfx::vline(frame, OVERLAY_RIGHT, top, BAR_HEIGHT, Ink::Lit);
    gfx::vline(frame, zero, top, 2, Ink::Lit);
    gfx::vline(frame, zero, top + BAR_HEIGHT - 2, 2, Ink::Lit);
}
