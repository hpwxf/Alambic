//! The system settings screen (journal/D-026).
//!
//! Reached from the `SETUP` entry of the app menu, or at power-up by holding
//! `up` + `down` (safe mode) through the splash screen; holding the left
//! encoder switch instead goes straight to the calibration wizard
//! ([`crate::wizard`]), which the `CALIBRATE` line also opens. It edits a *draft* of the [`Settings`]: the engine runs
//! with the draft while the screen is open, so a contrast, rotation or shift
//! change shows at once, and either keeps it (`SAVE`, which also asks the
//! backend to store it) or goes back to what was in force on entry (`EXIT`,
//! or the `up` + `down` chord).
//!
//! The interaction follows the reference firmware's setup screens: `up` and
//! `down` (or the left encoder) move between lines, the right encoder adjusts
//! the highlighted value, and pressing the right encoder runs the
//! highlighted action. This screen always reads the encoders as wired,
//! whatever their reversal setting, so that a wrong setting cannot make the
//! very screen that fixes it unusable.

use crate::gfx::{self, Ink, Point};

use crate::apps::AppId;
use crate::buttons::ButtonEvents;
use crate::fmt::TextBuf;
use crate::framebuffer::{FrameBuffer, ROW_HEIGHT, ROWS, WIDTH_I32};
use crate::platform::Button;
use crate::settings::{DisplaySettings, Settings};
use crate::text::fonts::FONT_5X8;
use crate::text::{self, Alignment, TextStyle};

/// Contrast change per detent of the right encoder.
pub const CONTRAST_STEP: u8 = 16;

/// Largest picture shift, in columns, either way.
pub const MAX_COLUMN_SHIFT: i8 = 8;

/// Column where values start, so labels and values form two columns.
const VALUE_COLUMN: i32 = 62;

/// Horizontal padding of a line, so text does not touch the highlight edge.
const TEXT_INSET: i32 = 2;

/// Screen rows available for lines, below the title.
const VISIBLE_LINES: usize = (ROWS - 1) as usize;

/// One line of the settings screen, top to bottom.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Line {
    /// The applet the module starts in.
    StartupApp,
    /// Screen contrast.
    Contrast,
    /// Screen turned by 180°.
    Rotate,
    /// Horizontal picture shift.
    Shift,
    /// Direction of the left encoder.
    LeftEncoder,
    /// Direction of the right encoder.
    RightEncoder,
    /// Opens the calibration wizard (`crate::wizard`).
    Calibrate,
    /// Keep the draft and store it.
    Save,
    /// Leave without keeping the draft.
    Exit,
}

impl Line {
    /// Every line, in screen order.
    pub const ALL: [Self; 9] = [
        Self::StartupApp,
        Self::Contrast,
        Self::Rotate,
        Self::Shift,
        Self::LeftEncoder,
        Self::RightEncoder,
        Self::Calibrate,
        Self::Save,
        Self::Exit,
    ];

    /// Label in the left column.
    const fn label(self) -> &'static str {
        match self {
            Self::StartupApp => "START APP",
            Self::Contrast => "CONTRAST",
            Self::Rotate => "ROTATE",
            Self::Shift => "SHIFT",
            Self::LeftEncoder => "ENC LEFT",
            Self::RightEncoder => "ENC RIGHT",
            Self::Calibrate => "CALIBRATE",
            Self::Save => "SAVE",
            Self::Exit => "EXIT",
        }
    }

    /// Position in [`Self::ALL`].
    const fn index(self) -> usize {
        self as usize
    }
}

/// What the settings screen decided on this tick.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SystemOutcome {
    /// Leave, keeping these settings in force and storing them.
    Save(Settings),
    /// Leave, going back to these settings (those in force on entry).
    Exit(Settings),
    /// Start the calibration wizard.
    Calibrate,
}

/// The system settings screen.
#[derive(Debug, Clone, Copy)]
pub struct SystemScreen {
    original: Settings,
    draft: Settings,
    cursor: usize,
    safe_mode: bool,
}

impl Default for SystemScreen {
    fn default() -> Self {
        Self::new()
    }
}

impl SystemScreen {
    /// A closed screen, holding the defaults.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            original: Settings::DEFAULT,
            draft: Settings::DEFAULT,
            cursor: 0,
            safe_mode: false,
        }
    }

    /// Opens on `settings`, the ones in force, with `line` highlighted.
    pub const fn open(&mut self, settings: Settings, line: Line) {
        self.original = settings;
        self.draft = settings;
        self.cursor = line.index();
        self.safe_mode = false;
    }

    /// Opens in safe mode: the screen and encoder preferences in the draft go
    /// back to their defaults, so a module made hard to use by them (screen
    /// turned, encoders reversed) can be repaired. Nothing is stored unless
    /// the draft is saved; leaving restores `settings` as they were.
    pub const fn open_safe(&mut self, settings: Settings) {
        self.open(settings, Line::StartupApp);
        self.draft.display = DisplaySettings::DEFAULT;
        self.draft.encoder_reversed = Settings::DEFAULT.encoder_reversed;
        self.safe_mode = true;
    }

    /// The settings being edited, which the engine runs with while open.
    #[must_use]
    pub const fn draft(&self) -> &Settings {
        &self.draft
    }

    /// The settings in force when the screen opened.
    #[must_use]
    pub const fn original(&self) -> &Settings {
        &self.original
    }

    /// The highlighted line.
    #[must_use]
    pub const fn line(&self) -> Line {
        Line::ALL[self.cursor]
    }

    /// Whether the screen was opened in safe mode.
    #[must_use]
    pub const fn is_safe_mode(&self) -> bool {
        self.safe_mode
    }

    /// Feeds one tick of panel input; `deltas` are the encoder detents as
    /// wired (left, right).
    pub fn update(&mut self, buttons: &ButtonEvents, deltas: [i8; 2]) -> Option<SystemOutcome> {
        let mut steps = i32::from(deltas[0]);
        if buttons.pressed(Button::Down) {
            steps += 1;
        }
        if buttons.pressed(Button::Up) {
            steps -= 1;
        }
        self.move_by(steps);
        self.adjust(i32::from(deltas[1]));

        if !buttons.pressed(Button::RightEncoder) {
            return None;
        }
        match self.line() {
            Line::Save => Some(SystemOutcome::Save(self.draft)),
            Line::Exit => Some(SystemOutcome::Exit(self.original)),
            Line::Calibrate => Some(SystemOutcome::Calibrate),
            _ => None,
        }
    }

    /// Moves the highlight, wrapping in both directions.
    fn move_by(&mut self, steps: i32) {
        if steps == 0 {
            return;
        }
        let count = i32::try_from(Line::ALL.len()).unwrap_or(1);
        let cursor = i32::try_from(self.cursor).unwrap_or(0);
        self.cursor = usize::try_from((cursor + steps).rem_euclid(count)).unwrap_or(0);
    }

    /// Applies `detents` of the right encoder to the highlighted value.
    fn adjust(&mut self, detents: i32) {
        if detents == 0 {
            return;
        }
        let line = self.line();
        let draft = &mut self.draft;
        match line {
            Line::StartupApp => {
                let count = i32::try_from(AppId::ALL.len()).unwrap_or(1);
                let current = i32::try_from(draft.startup_app.index()).unwrap_or(0);
                let next = usize::try_from((current + detents).rem_euclid(count)).unwrap_or(0);
                draft.startup_app = AppId::from_index(next).unwrap_or_default();
            }
            Line::Contrast => {
                let step = i32::from(CONTRAST_STEP);
                let next = (i32::from(draft.display.contrast) + detents * step).clamp(0, 255);
                draft.display.contrast = u8::try_from(next).unwrap_or(u8::MAX);
            }
            Line::Rotate => draft.display.rotated = !draft.display.rotated,
            Line::Shift => {
                let limit = i32::from(MAX_COLUMN_SHIFT);
                let next = (i32::from(draft.display.column_shift) + detents).clamp(-limit, limit);
                draft.display.column_shift = i8::try_from(next).unwrap_or(0);
            }
            Line::LeftEncoder => draft.encoder_reversed[0] = !draft.encoder_reversed[0],
            Line::RightEncoder => draft.encoder_reversed[1] = !draft.encoder_reversed[1],
            Line::Calibrate | Line::Save | Line::Exit => {}
        }
    }

    /// The value shown for a line, if it has one.
    fn value(&self, line: Line, out: &mut TextBuf<12>) {
        use core::fmt::Write as _;

        let draft = &self.draft;
        let direction = |reversed: bool| if reversed { "REVERSED" } else { "NORMAL" };
        let _ = match line {
            Line::StartupApp => out.write_str(draft.startup_app.name()),
            Line::Contrast => write!(out, "{}", draft.display.contrast),
            Line::Rotate => out.write_str(if draft.display.rotated { "ON" } else { "OFF" }),
            Line::Shift => write!(out, "{:+}", draft.display.column_shift),
            Line::LeftEncoder => out.write_str(direction(draft.encoder_reversed[0])),
            Line::RightEncoder => out.write_str(direction(draft.encoder_reversed[1])),
            Line::Calibrate | Line::Save | Line::Exit => Ok(()),
        };
    }

    /// First line shown, so that the highlight stays on screen.
    fn first_visible(&self) -> usize {
        let last_start = Line::ALL.len().saturating_sub(VISIBLE_LINES);
        self.cursor
            .saturating_sub(VISIBLE_LINES - 1)
            .min(last_start)
    }

    /// Draws the title and the visible lines, the highlight in inverse video.
    pub fn render(&self, frame: &mut FrameBuffer) {
        frame.clear();
        text::draw(frame, "SETUP", Point::zero(), TextStyle::lit(&FONT_5X8));
        if self.safe_mode {
            text::draw(
                frame,
                "SAFE MODE",
                Point::new(WIDTH_I32 - 1, 0),
                TextStyle::lit(&FONT_5X8).aligned(Alignment::Right),
            );
        }

        let first = self.first_visible();
        for (slot, &line) in Line::ALL.iter().skip(first).take(VISIBLE_LINES).enumerate() {
            let row = 1 + i32::try_from(slot).unwrap_or(0);
            let top = row * ROW_HEIGHT;
            let highlighted = line.index() == self.cursor;
            if highlighted {
                let at: Point = Point::new(0, top);
                gfx::fill_rect(frame, at.x, at.y, WIDTH_I32, ROW_HEIGHT, Ink::Lit);
            }
            let style = if highlighted {
                TextStyle::dark(&FONT_5X8)
            } else {
                TextStyle::lit(&FONT_5X8)
            };
            text::draw(frame, line.label(), Point::new(TEXT_INSET, top), style);
            let mut value = TextBuf::<12>::new();
            self.value(line, &mut value);
            text::draw(frame, value.as_str(), Point::new(VALUE_COLUMN, top), style);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{CONTRAST_STEP, Line, MAX_COLUMN_SHIFT, SystemOutcome, SystemScreen};
    use crate::apps::AppId;
    use crate::buttons::{ButtonEvents, ButtonReader};
    use crate::framebuffer::FrameBuffer;
    use crate::platform::{BUTTONS, Button, ControlEvents, ENCODERS};
    use crate::settings::{DisplaySettings, Settings};

    /// Button events with `button` firing on this tick (see the same helper
    /// in `menu`).
    fn press(button: Button) -> ButtonEvents {
        let mut reader = ButtonReader::new();
        let mut controls = ControlEvents {
            encoder_delta: [0; ENCODERS],
            button_down: [false; BUTTONS],
        };
        for down in [true, false] {
            controls.button_down[button.index()] = down;
            for _ in 0..8 {
                let events = reader.update(&controls);
                if events.pressed(button) {
                    return events;
                }
            }
        }
        panic!("{button:?} never fired");
    }

    fn opened_at(line: Line) -> SystemScreen {
        let mut screen = SystemScreen::new();
        screen.open(Settings::DEFAULT, line);
        screen
    }

    #[test]
    fn up_down_and_the_left_encoder_move_between_lines() {
        let mut screen = opened_at(Line::StartupApp);
        screen.update(&press(Button::Down), [0, 0]);
        assert_eq!(screen.line(), Line::Contrast);
        screen.update(&ButtonEvents::default(), [2, 0]);
        assert_eq!(screen.line(), Line::Shift);
        screen.update(&press(Button::Up), [0, 0]);
        assert_eq!(screen.line(), Line::Rotate);
        screen.update(&ButtonEvents::default(), [-3, 0]);
        assert_eq!(screen.line(), Line::Exit, "the list wraps");
    }

    #[test]
    fn the_right_encoder_adjusts_the_highlighted_value() {
        let mut screen = opened_at(Line::StartupApp);
        screen.update(&ButtonEvents::default(), [0, 1]);
        assert_eq!(screen.draft().startup_app, AppId::Scope);

        let mut screen = opened_at(Line::Contrast);
        screen.update(&ButtonEvents::default(), [0, -1]);
        assert_eq!(
            screen.draft().display.contrast,
            DisplaySettings::DEFAULT.contrast - CONTRAST_STEP
        );
        screen.update(&ButtonEvents::default(), [0, 100]);
        assert_eq!(screen.draft().display.contrast, 255, "saturates");

        let mut screen = opened_at(Line::Shift);
        screen.update(&ButtonEvents::default(), [0, -100]);
        assert_eq!(screen.draft().display.column_shift, -MAX_COLUMN_SHIFT);

        let mut screen = opened_at(Line::Rotate);
        screen.update(&ButtonEvents::default(), [0, 1]);
        assert!(screen.draft().display.rotated);

        let mut screen = opened_at(Line::RightEncoder);
        screen.update(&ButtonEvents::default(), [0, -1]);
        assert_eq!(screen.draft().encoder_reversed, [false, true]);
    }

    #[test]
    fn save_keeps_the_draft_and_exit_restores_the_original() {
        let mut screen = opened_at(Line::Rotate);
        screen.update(&ButtonEvents::default(), [0, 1]);
        let mut edited = Settings::DEFAULT;
        edited.display.rotated = true;

        let mut saving = screen;
        saving.update(&ButtonEvents::default(), [1, 0]);
        // Rotate -> Shift; walk to SAVE.
        while saving.line() != Line::Save {
            saving.update(&ButtonEvents::default(), [1, 0]);
        }
        assert_eq!(
            saving.update(&press(Button::RightEncoder), [0, 0]),
            Some(SystemOutcome::Save(edited))
        );

        let mut leaving = screen;
        while leaving.line() != Line::Exit {
            leaving.update(&ButtonEvents::default(), [1, 0]);
        }
        assert_eq!(
            leaving.update(&press(Button::RightEncoder), [0, 0]),
            Some(SystemOutcome::Exit(Settings::DEFAULT))
        );
    }

    #[test]
    fn pressing_on_a_value_line_does_nothing_and_calibrate_asks_for_the_wizard() {
        let mut screen = opened_at(Line::Contrast);
        assert_eq!(screen.update(&press(Button::RightEncoder), [0, 0]), None);
        let mut screen = opened_at(Line::Calibrate);
        assert_eq!(
            screen.update(&press(Button::RightEncoder), [0, 0]),
            Some(SystemOutcome::Calibrate)
        );
    }

    #[test]
    fn safe_mode_drafts_default_screen_and_encoders_but_keeps_the_rest() {
        let mut stored = Settings::DEFAULT;
        stored.startup_app = AppId::Scope;
        stored.display.rotated = true;
        stored.display.contrast = 0x20;
        stored.encoder_reversed = [true, true];

        let mut screen = SystemScreen::new();
        screen.open_safe(stored);
        assert!(screen.is_safe_mode());
        assert_eq!(screen.draft().display, DisplaySettings::DEFAULT);
        assert_eq!(screen.draft().encoder_reversed, [false, false]);
        assert_eq!(screen.draft().startup_app, AppId::Scope);
        assert_eq!(screen.original(), &stored);
    }

    #[test]
    fn the_highlight_always_stays_on_screen() {
        for line in Line::ALL {
            let screen = opened_at(line);
            let first = screen.first_visible();
            assert!(
                (first..first + super::VISIBLE_LINES).contains(&line.index()),
                "{line:?}"
            );
            let mut frame = FrameBuffer::new();
            screen.render(&mut frame);
            assert!(frame.lit_pixels() > 0);
        }
    }
}
