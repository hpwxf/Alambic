//! The routing page of the applet with the focus (journal/D-046): where each
//! of its output ports goes, and which jack each input it reads by itself
//! comes from. Opened by a long press of `up`, closed the same way or by
//! the menu chord; a short press of `up` or `down` moves to the next
//! applet's page. A long press of `down` is kept for the applet's
//! options page, to come.
//!
//! Like the menu, a screen of the engine that owns the display without
//! owning the outputs: every applet keeps running behind it. The left
//! encoder picks a row, the right one turns it: an output steps through
//! `OFF` and `A` to `D`, skipping the jacks another port holds, an input
//! through the four jacks of its kind, which any applet may share. The
//! right encoder stops at the ends rather than wrapping round, so a quick
//! turn down always lands on `OFF`, first, and one up on the last free jack.

use core::fmt::Write as _;

use crate::apps::{AppHost, InputKind, InputSource, MAX_INSTANCES};
use crate::fmt::TextBuf;
use crate::framebuffer::{FrameBuffer, ROW_HEIGHT, ROWS};
use crate::gfx::controls::{
    LIST_LEFT, SCROLL_WIDTH, TITLE_LEFT, row_highlight, scroll_track, scroll_window,
};
use crate::gfx::{Ink, Point};
use crate::modulation::Source;
use crate::platform::{CV_CHANNELS, CvChannel};
use crate::text::fonts::FONT_5X8;
use crate::text::{self, TextStyle};

/// First screen row of the list, right under the title.
const FIRST_ROW: i32 = 1;

/// Rows of the list under the title.
const VISIBLE: usize = (ROWS - FIRST_ROW) as usize;

/// Where a row's name starts, right of the scroll bar.
const TEXT_INSET: i32 = LIST_LEFT + 2;

/// The gap between a row's columns, in pixels.
const COLUMN_GAP: i32 = 4;

/// The arrow between a port and its jack.
const ARROW_WIDTH: i32 = 10;

/// The jacks' names, by kind and index.
const OUTPUTS: [&str; CV_CHANNELS] = ["A", "B", "C", "D"];
const CV_INPUTS: [&str; CV_CHANNELS] = ["CV1", "CV2", "CV3", "CV4"];
const TRIGGER_INPUTS: [&str; CV_CHANNELS] = ["TR1", "TR2", "TR3", "TR4"];

/// One row of the page.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RouteRow {
    /// Output port `n` of the applet.
    Output(usize),
    /// Input port `n` of the applet.
    Input(usize),
}

/// The routing page.
#[derive(Debug, Clone, Copy, Default)]
pub struct RoutingPage {
    cursor: usize,
}

impl RoutingPage {
    /// A page with its first row picked.
    #[must_use]
    pub const fn new() -> Self {
        Self { cursor: 0 }
    }

    /// Opens on the first row.
    pub const fn open(&mut self) {
        self.cursor = 0;
    }

    /// How many rows the applet with the focus has: its outputs, then its
    /// inputs.
    fn len(host: &AppHost) -> usize {
        let app = host.focused();
        app.output_ports().len() + app.input_ports().len()
    }

    /// The row at `index`.
    fn row(host: &AppHost, index: usize) -> RouteRow {
        let outputs = host.focused().output_ports().len();
        if index < outputs {
            RouteRow::Output(index)
        } else {
            RouteRow::Input(index - outputs)
        }
    }

    /// The row picked.
    #[must_use]
    pub fn selected(&self, host: &AppHost) -> RouteRow {
        Self::row(host, self.cursor)
    }

    /// Feeds one tick of encoder movement: the left one picks a row,
    /// wrapping; the right one turns the row picked by as many steps.
    pub fn update(&mut self, deltas: [i8; 2], host: &mut AppHost) {
        let len = Self::len(host);
        if len == 0 {
            return;
        }
        let picks = i32::from(deltas[0]);
        if picks != 0 {
            let count = i32::try_from(len).unwrap_or(1);
            let cursor = i32::try_from(self.cursor).unwrap_or(0);
            self.cursor = usize::try_from((cursor + picks).rem_euclid(count)).unwrap_or(0);
        }
        self.cursor = self.cursor.min(len - 1);
        let turns = deltas[1];
        if turns == 0 {
            return;
        }
        let step = if turns > 0 { 1 } else { -1 };
        for _ in 0..turns.unsigned_abs() {
            match Self::row(host, self.cursor) {
                RouteRow::Output(port) => Self::step_output(host, port, step),
                RouteRow::Input(port) => Self::step_input(host, port, step),
            }
        }
    }

    /// Moves output port `port` to the next jack in `step`'s direction
    /// through `OFF` and `A` to `D`, skipping the jacks others hold, and
    /// stays put at either end: `OFF`, first, is always free.
    fn step_output(host: &mut AppHost, port: usize, step: i32) {
        let position = host.focus();
        let Some(route) = host.route_at(position) else {
            return;
        };
        // `OFF` first, then `A` to `D`.
        let places = i32::try_from(CV_CHANNELS).unwrap_or(4) + 1;
        let current = route
            .output(port)
            .map_or(0, |jack| i32::try_from(jack.index()).unwrap_or(0) + 1);
        let mut place = current + step;
        while (0..places).contains(&place) {
            let output = usize::try_from(place - 1)
                .ok()
                .and_then(CvChannel::from_index);
            if host.set_route(position, port, output).is_ok() {
                return;
            }
            place += step;
        }
    }

    /// Moves input port `port` to the next of what it can read, staying put
    /// at the first and the last: `OFF`, the four jacks of its kind, then the
    /// ports the other instances offer, slot by slot (journal/D-048).
    fn step_input(host: &mut AppHost, port: usize, step: i32) {
        let position = host.focus();
        let Some(spec) = host.focused().input_ports().get(port).copied() else {
            return;
        };
        let Some(inputs) = host.inputs_at(position) else {
            return;
        };
        let (choices, count) = input_choices(host, position);
        let current = inputs.source(spec.kind, spec.lane);
        let index = choices[..count]
            .iter()
            .position(|&choice| Some(choice) == current)
            .unwrap_or(0);
        let last = i32::try_from(count).unwrap_or(1) - 1;
        let next = (i32::try_from(index).unwrap_or(0) + step).clamp(0, last);
        let _ = host.set_input(position, port, choices[usize::try_from(next).unwrap_or(0)]);
    }

    /// First row shown, so that the one picked stays on screen.
    const fn first_visible(self, len: usize) -> usize {
        let last_start = len.saturating_sub(VISIBLE);
        let first = self.cursor.saturating_sub(VISIBLE - 1);
        if first < last_start {
            first
        } else {
            last_start
        }
    }

    /// Draws the title, the applet's name, and a row per port: its name,
    /// then where it goes or where it comes from.
    pub fn render(&self, frame: &mut FrameBuffer, host: &AppHost) {
        frame.clear();
        let app = host.focused();
        let mut title = TextBuf::<24>::new();
        let _ = core::fmt::Write::write_fmt(&mut title, format_args!("ROUTE {}", app.name()));
        text::draw(
            frame,
            title.as_str(),
            Point::new(TITLE_LEFT, 0),
            TextStyle::lit(&FONT_5X8),
        );

        let len = Self::len(host);
        let first = self.first_visible(len);
        let rows = i32::try_from(VISIBLE).unwrap_or(1);
        if len > VISIBLE {
            scroll_window(
                frame,
                0,
                FIRST_ROW * ROW_HEIGHT,
                SCROLL_WIDTH,
                rows * ROW_HEIGHT,
                first,
                VISIBLE,
                len,
            );
        } else {
            let shown = i32::try_from(len.max(1)).unwrap_or(1);
            scroll_track(
                frame,
                0,
                FIRST_ROW * ROW_HEIGHT,
                SCROLL_WIDTH,
                shown * ROW_HEIGHT,
            );
        }
        let route = host.route_at(host.focus());
        let inputs = host.inputs_at(host.focus());
        // The arrows and the jacks in two columns right of the longest
        // name, so that every row reads the same way whatever the names.
        let longest = app
            .output_ports()
            .iter()
            .map(|port| port.name)
            .chain(app.input_ports().iter().map(|port| port.name))
            .map(|name| FONT_5X8.width_of(name))
            .max()
            .unwrap_or(0);
        let arrow_column = TEXT_INSET + longest + COLUMN_GAP;
        let jack_column = arrow_column + ARROW_WIDTH + COLUMN_GAP;
        for (slot, index) in (first..len).take(VISIBLE).enumerate() {
            let top = (FIRST_ROW + i32::try_from(slot).unwrap_or(0)) * ROW_HEIGHT;
            let picked = index == self.cursor;
            if picked {
                row_highlight(frame, top, ROW_HEIGHT, Ink::Lit);
            }
            let style = if picked {
                TextStyle::dark(&FONT_5X8)
            } else {
                TextStyle::lit(&FONT_5X8)
            };
            let (name, arrow, jack) = match Self::row(host, index) {
                RouteRow::Output(port) => (
                    app.output_ports()[port].name,
                    "->",
                    text_of(
                        route
                            .and_then(|route| route.output(port))
                            .map_or("OFF", |jack| OUTPUTS[jack.index()]),
                    ),
                ),
                RouteRow::Input(port) => {
                    let spec = app.input_ports()[port];
                    let source = inputs.and_then(|inputs| inputs.source(spec.kind, spec.lane));
                    (spec.name, "<-", source_text(host, spec.kind, source))
                }
            };
            text::draw(frame, name, Point::new(TEXT_INSET, top), style);
            text::draw(frame, arrow, Point::new(arrow_column, top), style);
            text::draw(frame, jack.as_str(), Point::new(jack_column, top), style);
        }
    }
}

/// The most an input port can read: nothing, the four jacks of its kind,
/// and every lane of the other instances.
const MAX_CHOICES: usize = 1 + CV_CHANNELS + (MAX_INSTANCES - 1) * CV_CHANNELS;

/// What an input of the instance at `position` can read, in the order the
/// right encoder turns them, and how many.
fn input_choices(host: &AppHost, position: usize) -> ([InputSource; MAX_CHOICES], usize) {
    let mut choices = [InputSource::Off; MAX_CHOICES];
    // `OFF` first, as an output's: a quick turn down frees every jack.
    let mut count = 1;
    let jacks = (0..CV_CHANNELS).filter_map(InputSource::jack);
    let bus = host.bus(Some(position));
    let lanes = bus.sources().filter_map(|source| match source {
        Source::Lane { slot, port } => InputSource::lane(usize::from(slot), usize::from(port)),
        Source::Jack(_) => None,
    });
    for choice in jacks.chain(lanes) {
        if count < MAX_CHOICES {
            choices[count] = choice;
            count += 1;
        }
    }
    (choices, count)
}

/// What an input of `kind` reading `source` shows: its jack (`TR2`), the
/// slot and name of the port it reads (`1:BURST`, journal/D-048), or `OFF`.
fn source_text(host: &AppHost, kind: InputKind, source: Option<InputSource>) -> TextBuf<16> {
    let mut out = TextBuf::<16>::new();
    let names = match kind {
        InputKind::Cv => &CV_INPUTS,
        InputKind::Trigger => &TRIGGER_INPUTS,
    };
    match source {
        Some(InputSource::Jack(jack)) => {
            return text_of(names.get(usize::from(jack)).copied().unwrap_or("--"));
        }
        Some(InputSource::Lane { slot, port }) => {
            let name = host
                .bus(None)
                .port(usize::from(slot), usize::from(port))
                .map(|port| port.name);
            Source::Lane { slot, port }.write_name(&mut out, name, PORT_LETTERS);
        }
        Some(InputSource::Off) => return text_of("OFF"),
        None => return text_of("--"),
    }
    out
}

/// `text`, in a buffer of a row's width.
fn text_of(text: &str) -> TextBuf<16> {
    let mut out = TextBuf::<16>::new();
    let _ = out.write_str(text);
    out
}

/// The most letters of a port's name an input's row writes after its slot.
const PORT_LETTERS: usize = 6;

#[cfg(test)]
mod tests {
    use super::{RouteRow, RoutingPage};
    use crate::apps::{AppHost, AppId};
    use crate::audio::Route;
    use crate::framebuffer::FrameBuffer;
    use crate::platform::CvChannel;

    fn supersaw_and_ratchet() -> AppHost {
        let mut host = AppHost::new();
        host.select(AppId::Supersaw);
        host.select_at(1, AppId::Ratchet);
        host
    }

    #[test]
    fn the_rows_are_the_outputs_then_the_inputs() {
        let mut host = supersaw_and_ratchet();
        let mut page = RoutingPage::new();
        assert_eq!(page.selected(&host), RouteRow::Output(0));
        page.update([2, 0], &mut host);
        assert_eq!(page.selected(&host), RouteRow::Output(2), "SUB");
        host.set_focus(1);
        page.open();
        page.update([-1, 0], &mut host);
        assert_eq!(page.selected(&host), RouteRow::Input(0), "RATCHET's CLOCK");
    }

    #[test]
    fn an_output_steps_past_the_jacks_others_hold() {
        let mut host = supersaw_and_ratchet();
        let mut page = RoutingPage::new();
        // SUB on C: one step down skips B and A, SUPERSAW's own, to OFF;
        // one more stays there; one up skips them again, back to C.
        page.update([2, -1], &mut host);
        assert_eq!(host.route().output(2), None, "C -> OFF");
        page.update([0, -1], &mut host);
        assert_eq!(host.route().output(2), None, "OFF is the end");
        page.update([0, 1], &mut host);
        assert_eq!(host.route().output(2), Some(CvChannel::Three), "OFF -> C");
        // Up again: D is RATCHET's, so C is the last free.
        page.update([0, 3], &mut host);
        assert_eq!(
            host.route().output(2),
            Some(CvChannel::Three),
            "C, the last free"
        );
        // Down again: SUB back to OFF, which frees C.
        page.update([0, -1], &mut host);
        assert_eq!(host.route().output(2), None);
        // RATCHET's B, on nothing (SUPERSAW held A to C, its A took D): a
        // step up from OFF skips A and B, SUPERSAW's, to C, now free.
        host.set_focus(1);
        page.open();
        page.update([1, 1], &mut host);
        assert_eq!(
            host.route().output(1),
            Some(CvChannel::Three),
            "OFF -> A, B taken: C"
        );
        assert_ne!(host.route(), Route::OFF);
    }

    #[test]
    fn a_quick_turn_down_lands_on_off() {
        let mut host = AppHost::new();
        host.select(AppId::Supersaw);
        let mut page = RoutingPage::new();
        assert_eq!(host.route().output(0), Some(CvChannel::One), "A on A");
        page.update([0, -20], &mut host);
        assert_eq!(host.route().output(0), None, "past every jack: OFF");
        page.update([0, 20], &mut host);
        assert_eq!(
            host.route().output(0),
            Some(CvChannel::Four),
            "back up: the last free jack"
        );
    }

    #[test]
    fn an_input_steps_through_off_and_the_jacks_of_its_kind() {
        let mut host = supersaw_and_ratchet();
        host.set_focus(1);
        let mut page = RoutingPage::new();
        page.update([2, 1], &mut host);
        assert_eq!(page.selected(&host), RouteRow::Input(0));
        let inputs = host.inputs_at(1).unwrap();
        assert_eq!(
            inputs.jack(crate::apps::InputKind::Trigger, 0),
            Some(1),
            "TR2"
        );
        page.update([0, -1], &mut host);
        let inputs = host.inputs_at(1).unwrap();
        assert_eq!(
            inputs.jack(crate::apps::InputKind::Trigger, 0),
            Some(0),
            "TR1"
        );
        page.update([0, -5], &mut host);
        let inputs = host.inputs_at(1).unwrap();
        assert_eq!(
            inputs.source(crate::apps::InputKind::Trigger, 0),
            Some(crate::apps::InputSource::Off),
            "OFF, the first: no wrapping round"
        );
        page.update([0, 9], &mut host);
        let inputs = host.inputs_at(1).unwrap();
        assert_eq!(
            inputs.jack(crate::apps::InputKind::Trigger, 0),
            Some(3),
            "TR4, the last"
        );
    }

    #[test]
    fn the_page_draws_a_row_per_port() {
        let host = supersaw_and_ratchet();
        let mut frame = FrameBuffer::new();
        RoutingPage::new().render(&mut frame, &host);
        assert!(frame.lit_pixels() > 0);
    }
}
