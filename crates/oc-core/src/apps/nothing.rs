//! What an empty slot runs (journal/D-046, revision of 2026-11-25): no
//! port, no setting, every output it could reach left to the others.
//!
//! The pool always holds [`MAX_INSTANCES`](super::MAX_INSTANCES) applets;
//! a slot nobody uses holds this one, which the menu replaces like any
//! other, and which `NOTHING` in the menu puts back. Its screen says what
//! the slot is and how to fill it.

use crate::apps::sound::Params;
use crate::apps::{App, AppId, InputSnapshot, Port, TickContext};
use crate::framebuffer::{FrameBuffer, WIDTH_I32};
use crate::gfx::Point;
use crate::gfx::controls::TITLE_LEFT;
use crate::text::fonts::FONT_5X8;
use crate::text::{self, TextStyle};

/// The lines under the title, centred.
const HINT: [&str; 2] = ["EMPTY SLOT", "HOLD UP+DOWN: APPS"];

/// Where the hint's first line starts, in pixels from the top.
const HINT_TOP: i32 = 24;

/// An empty slot.
#[derive(Debug, Clone, Copy, Default)]
pub struct NothingApp;

impl NothingApp {
    /// An empty slot.
    #[must_use]
    pub const fn new() -> Self {
        Self
    }
}

impl App for NothingApp {
    fn id(&self) -> AppId {
        AppId::Nothing
    }

    /// Every output at rest; its route reaches none anyway.
    fn update(&mut self, _input: &InputSnapshot) -> Params {
        Params::REST
    }

    /// No port: it holds no output, the others take them all.
    fn output_ports(&self) -> &'static [Port] {
        &[]
    }

    fn render(&self, frame: &mut FrameBuffer, _context: &TickContext) {
        frame.clear();
        let style = TextStyle::lit(&FONT_5X8);
        text::draw(frame, self.name(), Point::new(TITLE_LEFT, 0), style);
        for (line, top) in HINT.iter().zip((HINT_TOP..).step_by(10)) {
            let left = (WIDTH_I32 - FONT_5X8.width_of(line)) / 2;
            text::draw(frame, line, Point::new(left, top), style);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::NothingApp;
    use crate::apps::{App, AppId, InputSnapshot, TickContext};
    use crate::framebuffer::FrameBuffer;

    #[test]
    fn nothing_holds_no_output_and_says_so() {
        let mut app = NothingApp::new();
        assert_eq!(app.id(), AppId::Nothing);
        assert_eq!(
            (
                app.output_ports().len(),
                app.input_ports().len(),
                app.param_specs().len()
            ),
            (0, 0, 0)
        );
        let _ = app.update(&InputSnapshot::default());
        let mut frame = FrameBuffer::new();
        app.render(&mut frame, &TickContext::default());
        assert!(frame.lit_pixels() > 0);
    }
}
