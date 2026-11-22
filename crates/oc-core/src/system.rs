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

use crate::gfx::controls::{LIST_LEFT, SCROLL_WIDTH, row_highlight, scroll_window};
use crate::gfx::{Ink, Point};

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

/// Title, at the left of the top row.
const TITLE: &str = "SETUP";

/// Column where values start, so labels and values form two columns.
const VALUE_COLUMN: i32 = 62;

/// Horizontal padding of a line, so text does not touch the highlight edge.
const TEXT_INSET: i32 = LIST_LEFT + 2;

/// Room for the build label: `v`, the version, a space and the identifier
/// (a short hash, `-dirty` included, fits with room to spare).
const BUILD_LABEL_CAPACITY: usize = 32;

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

/// Every label ends left of the value column, with a blank column at least:
/// a longer label is a compile error rather than one running into its value.
#[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
// A label is a handful of ASCII characters, so its length fits an `i32`.
const _: () = {
    let mut index = 0;
    while index < Line::ALL.len() {
        let chars = Line::ALL[index].label().len() as i32;
        assert!(
            TEXT_INSET + chars * FONT_5X8.advance() < VALUE_COLUMN,
            "a setting's label runs into its value"
        );
        index += 1;
    }
};

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

    /// What the title shows at its right: `v<version> <build id>`, or
    /// nothing without an identifier, so that the tests and the golden
    /// screens, which set none, depend neither on the version nor on the
    /// commit (journal/D-025).
    fn build_label(build_id: &str, out: &mut TextBuf<BUILD_LABEL_CAPACITY>) {
        use core::fmt::Write as _;

        if !build_id.is_empty() {
            let _ = write!(out, "v{} {build_id}", env!("CARGO_PKG_VERSION"));
        }
    }

    /// First line shown, so that the highlight stays on screen.
    fn first_visible(&self) -> usize {
        let last_start = Line::ALL.len().saturating_sub(VISIBLE_LINES);
        self.cursor
            .saturating_sub(VISIBLE_LINES - 1)
            .min(last_start)
    }

    /// Draws the title and the visible lines, the highlight in inverse video;
    /// `build_id` is the one the backend set on the engine (short commit
    /// hash, `-dirty` if the tree differed), shown with the version at the
    /// right of the title.
    pub fn render(&self, frame: &mut FrameBuffer, build_id: &str) {
        frame.clear();
        text::draw(frame, TITLE, Point::zero(), TextStyle::lit(&FONT_5X8));
        // Safe mode matters more than the build, and both cannot fit.
        let mut build = TextBuf::<BUILD_LABEL_CAPACITY>::new();
        let right = if self.safe_mode {
            "SAFE MODE"
        } else {
            Self::build_label(build_id, &mut build);
            build.as_str()
        };
        text::draw(
            frame,
            right,
            Point::new(WIDTH_I32 - 1, 0),
            TextStyle::lit(&FONT_5X8).aligned(Alignment::Right),
        );

        let first = self.first_visible();
        let rows = i32::try_from(VISIBLE_LINES).unwrap_or(1);
        scroll_window(
            frame,
            0,
            ROW_HEIGHT,
            SCROLL_WIDTH,
            rows * ROW_HEIGHT,
            first,
            VISIBLE_LINES,
            Line::ALL.len(),
        );
        for (slot, &line) in Line::ALL.iter().skip(first).take(VISIBLE_LINES).enumerate() {
            let row = 1 + i32::try_from(slot).unwrap_or(0);
            let top = row * ROW_HEIGHT;
            let highlighted = line.index() == self.cursor;
            if highlighted {
                row_highlight(frame, top, ROW_HEIGHT, Ink::Lit);
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
    use super::{
        BUILD_LABEL_CAPACITY, CONTRAST_STEP, Line, MAX_COLUMN_SHIFT, SystemOutcome, SystemScreen,
        TITLE, VISIBLE_LINES,
    };
    use crate::apps::AppId;
    use crate::buttons::{ButtonEvents, ButtonReader};
    use crate::fmt::TextBuf;
    use crate::framebuffer::{FrameBuffer, HEIGHT_I32, ROW_HEIGHT, WIDTH_I32};
    use crate::platform::{BUTTONS, Button, ControlEvents, ENCODERS};
    use crate::settings::{DisplaySettings, Settings};
    use crate::text::fonts::FONT_5X8;

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

    fn build_label(build_id: &str) -> TextBuf<BUILD_LABEL_CAPACITY> {
        let mut label = TextBuf::new();
        SystemScreen::build_label(build_id, &mut label);
        label
    }

    #[test]
    fn the_title_shows_the_version_and_the_build_only_with_an_identifier() {
        assert_eq!(build_label("").as_str(), "", "tests see no version");
        assert_eq!(
            build_label("a1b2c3d-dirty").as_str(),
            concat!("v", env!("CARGO_PKG_VERSION"), " a1b2c3d-dirty")
        );

        let screen = opened_at(Line::StartupApp);
        let (mut without, mut with) = (FrameBuffer::new(), FrameBuffer::new());
        screen.render(&mut without, "");
        screen.render(&mut with, "a1b2c3d");
        assert!(with.lit_pixels() > without.lit_pixels());
    }

    #[test]
    fn the_longest_build_label_fits_beside_the_title() {
        let label = build_label("a1b2c3d-dirty");
        assert!(!label.is_full());
        let gap = 1;
        assert!(
            FONT_5X8.width_of(TITLE) + gap + FONT_5X8.width_of(label.as_str()) <= WIDTH_I32,
            "{TITLE} and {label:?} overlap"
        );
    }

    #[test]
    fn the_scroll_bar_shows_the_visible_window() {
        // Under the title, column 0 is off the dotted track: only the thumb
        // lights it.
        let thumb = |line: Line| {
            let mut frame = FrameBuffer::new();
            opened_at(line).render(&mut frame, "");
            let lit = |y: &i32| frame.pixel(0, *y);
            let first = (ROW_HEIGHT..HEIGHT_I32).find(lit).unwrap();
            let end = (first..HEIGHT_I32).find(|y| !lit(y)).unwrap_or(HEIGHT_I32);
            assert!(!(end..HEIGHT_I32).any(|y| lit(&y)), "one solid thumb");
            (first, end)
        };
        let track = (
            ROW_HEIGHT,
            ROW_HEIGHT + ROW_HEIGHT * i32::try_from(VISIBLE_LINES).unwrap(),
        );
        let length = (track.1 - track.0) * i32::try_from(VISIBLE_LINES).unwrap()
            / i32::try_from(Line::ALL.len()).unwrap();

        let top = thumb(Line::StartupApp);
        assert_eq!(
            top,
            (track.0, track.0 + length),
            "the share shown, from the top"
        );
        let bottom = thumb(Line::Exit);
        assert_eq!(bottom.1, track.1, "the last window reaches the bottom");
        assert!(
            (bottom.1 - bottom.0 - length).abs() <= 1,
            "the thumb keeps its length as the window slides"
        );
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
            screen.render(&mut frame, "");
            assert!(frame.lit_pixels() > 0);
        }
    }
}
