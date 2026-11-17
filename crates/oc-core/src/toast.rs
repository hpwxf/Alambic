//! A short message drawn over whatever is on screen, such as the result of
//! a settings save ("Calibration saved to EEPROM!" in the reference firmware).
//!
//! The backend saves, so only it knows how the save went; it reports the
//! outcome to the engine ([`crate::Engine::report_save`]), which shows the
//! matching message for [`DURATION_MICROS`] on top of the screen in use.

use embedded_graphics::Drawable as _;
use embedded_graphics::pixelcolor::BinaryColor;
use embedded_graphics::prelude::{Point, Primitive as _, Size};
use embedded_graphics::primitives::{PrimitiveStyleBuilder, Rectangle};

use crate::framebuffer::{FrameBuffer, HEIGHT_I32, WIDTH_I32};
use crate::text::fonts::FONT_5X8;
use crate::text::{self, Alignment, TextStyle};

/// How long a message stays up, in microseconds.
pub const DURATION_MICROS: u32 = 1_500_000;

/// How a settings save went, as the backend reports it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SaveOutcome {
    /// Written to persistent storage (the module's flash, the simulator's
    /// file).
    Stored,
    /// Kept in the host's document: saved when the user saves it (VCV Rack's
    /// patch).
    WithDocument,
    /// Nowhere to store it: the settings last until the module restarts.
    SessionOnly,
    /// The storage refused it.
    Failed,
}

impl SaveOutcome {
    /// The two lines shown.
    const fn lines(self) -> [&'static str; 2] {
        match self {
            Self::Stored => ["SETTINGS", "SAVED"],
            Self::WithDocument => ["SAVED WITH", "THE PATCH"],
            Self::SessionOnly => ["KEPT UNTIL", "RESTART"],
            Self::Failed => ["SAVE FAILED!", "NOT STORED"],
        }
    }
}

/// A message being shown, and for how much longer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Toast {
    outcome: SaveOutcome,
    remaining_micros: u32,
}

impl Toast {
    /// A message for `outcome`, at the start of its time on screen.
    #[must_use]
    pub const fn new(outcome: SaveOutcome) -> Self {
        Self {
            outcome,
            remaining_micros: DURATION_MICROS,
        }
    }

    /// The outcome shown.
    #[must_use]
    pub const fn outcome(&self) -> SaveOutcome {
        self.outcome
    }

    /// Counts `elapsed_micros` down; `false` once the message has expired.
    pub const fn advance(&mut self, elapsed_micros: u32) -> bool {
        self.remaining_micros = self.remaining_micros.saturating_sub(elapsed_micros);
        self.remaining_micros > 0
    }

    /// Draws a framed box in the middle of `frame`, over what is there.
    pub fn draw(&self, frame: &mut FrameBuffer) {
        let size = Size::new(96, 28);
        let top_left = Point::new((WIDTH_I32 - 96) / 2, (HEIGHT_I32 - 28) / 2);
        let style = PrimitiveStyleBuilder::new()
            .fill_color(BinaryColor::Off)
            .stroke_color(BinaryColor::On)
            .stroke_width(1)
            .build();
        let _ = Rectangle::new(top_left, size)
            .into_styled(style)
            .draw(frame);

        let centred = TextStyle::lit(&FONT_5X8).aligned(Alignment::Center);
        for (index, line) in self.outcome.lines().into_iter().enumerate() {
            let y = top_left.y + 5 + i32::try_from(index).unwrap_or(0) * 10;
            text::draw(frame, line, Point::new(WIDTH_I32 / 2, y), centred);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{DURATION_MICROS, SaveOutcome, Toast};
    use crate::framebuffer::FrameBuffer;

    #[test]
    fn a_toast_expires_after_its_duration() {
        let mut toast = Toast::new(SaveOutcome::Stored);
        assert!(toast.advance(DURATION_MICROS - 1));
        assert!(!toast.advance(1));
    }

    #[test]
    fn every_outcome_draws_a_box_over_a_lit_screen() {
        for outcome in [
            SaveOutcome::Stored,
            SaveOutcome::WithDocument,
            SaveOutcome::SessionOnly,
            SaveOutcome::Failed,
        ] {
            let mut frame = FrameBuffer::new();
            frame.fill();
            let lit = frame.lit_pixels();
            Toast::new(outcome).draw(&mut frame);
            assert!(frame.lit_pixels() < lit, "{outcome:?} punches a dark box");
        }
    }
}
