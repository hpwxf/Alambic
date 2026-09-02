//! The app picker, opened by holding `up` and `down` together.
//!
//! Like [`SplashScreen`](crate::splash::SplashScreen), this is a screen that
//! owns the display without owning the outputs: while it is up the running
//! applet keeps tracking its inputs and driving its jacks, and only the panel
//! and the framebuffer belong to the menu. Nothing here decides *when* the menu
//! opens or closes — that is [`Engine::tick`](crate::Engine)'s job — so this
//! module only knows how to move a highlight and draw a list.

use embedded_graphics::Drawable as _;
use embedded_graphics::mono_font::MonoTextStyle;
use embedded_graphics::mono_font::ascii::FONT_5X8;
use embedded_graphics::pixelcolor::BinaryColor;
use embedded_graphics::prelude::{Point, Primitive as _, Size};
use embedded_graphics::primitives::{PrimitiveStyle, Rectangle};
use embedded_graphics::text::{Baseline, Text};

use crate::app::{ROW_HEIGHT, ROWS};
use crate::apps::AppId;
use crate::buttons::ButtonEvents;
use crate::framebuffer::{FrameBuffer, WIDTH_I32};
use crate::platform::Button;

/// Title shown above the list.
///
/// A fixed string rather than [`crate::BANNER`]: the banner carries the crate
/// version, and this screen has a golden snapshot that should not churn every
/// time the version is bumped.
const TITLE: &str = "SELECT APP";

/// First screen row of the list, leaving the title and a blank line above it.
const FIRST_ROW: i32 = 2;

/// Horizontal padding of a list entry, so text does not touch the highlight edge.
const TEXT_INSET: i32 = 2;

/// The list must fit on screen; adding an app past that is a compile error.
const _: () = assert!(
    AppId::ALL.len() <= (ROWS - FIRST_ROW) as usize,
    "the app list no longer fits on the screen"
);

/// What the menu decided on this tick.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MenuOutcome {
    /// The highlighted app was confirmed and should start running.
    Launch(AppId),
}

/// The app picker.
#[derive(Debug, Clone, Copy, Default)]
pub struct Menu {
    selected: usize,
}

impl Menu {
    /// A menu with the first app highlighted.
    #[must_use]
    pub const fn new() -> Self {
        Self { selected: 0 }
    }

    /// Returns the menu to its initial state.
    pub const fn reset(&mut self) {
        self.selected = 0;
    }

    /// Opens the menu with the running app highlighted.
    pub const fn open(&mut self, current: AppId) {
        self.selected = current.index();
    }

    /// The app currently highlighted.
    #[must_use]
    pub const fn selected(&self) -> AppId {
        match AppId::from_index(self.selected) {
            Some(id) => id,
            // Unreachable while `selected` is only ever set through `move_by`
            // and `open`, both of which keep it in range; falling back beats
            // panicking in a 1 kHz interrupt.
            None => AppId::Diagnostic,
        }
    }

    /// Moves the highlight, wrapping in both directions.
    pub fn move_by(&mut self, steps: i32) {
        if steps == 0 {
            return;
        }
        let count = i32::try_from(AppId::ALL.len()).unwrap_or(1);
        let selected = i32::try_from(self.selected).unwrap_or(0);
        self.selected =
            usize::try_from((selected + steps).rem_euclid(count)).unwrap_or(self.selected);
    }

    /// Feeds one tick of panel input and reports the app to launch, if confirmed.
    ///
    /// `up` and `down` move the highlight, and so does the left encoder;
    /// clockwise moves *down* the list, matching the direction that encoder
    /// already selects channels in. Pressing either encoder launches.
    pub fn update(&mut self, buttons: &ButtonEvents, encoder_delta: i8) -> Option<MenuOutcome> {
        let mut steps = i32::from(encoder_delta);
        if buttons.pressed(Button::Down) {
            steps += 1;
        }
        if buttons.pressed(Button::Up) {
            steps -= 1;
        }
        self.move_by(steps);

        if buttons.pressed(Button::LeftEncoder) || buttons.pressed(Button::RightEncoder) {
            return Some(MenuOutcome::Launch(self.selected()));
        }
        None
    }

    /// Draws the title and the app list, with the highlight in inverse video.
    pub fn render(&self, frame: &mut FrameBuffer) {
        frame.clear();
        let lit = MonoTextStyle::new(&FONT_5X8, BinaryColor::On);
        let dark = MonoTextStyle::new(&FONT_5X8, BinaryColor::Off);
        let _ = Text::with_baseline(TITLE, Point::zero(), lit, Baseline::Top).draw(frame);

        for (index, id) in AppId::ALL.into_iter().enumerate() {
            let row = FIRST_ROW + i32::try_from(index).unwrap_or(0);
            let top_left = Point::new(0, row * ROW_HEIGHT);
            let highlighted = index == self.selected;
            if highlighted {
                let band = Rectangle::new(
                    top_left,
                    Size::new(
                        u32::try_from(WIDTH_I32).unwrap_or(0),
                        u32::try_from(ROW_HEIGHT).unwrap_or(0),
                    ),
                );
                let _ = band
                    .into_styled(PrimitiveStyle::with_fill(BinaryColor::On))
                    .draw(frame);
            }
            let style = if highlighted { dark } else { lit };
            let text_at = Point::new(TEXT_INSET, top_left.y);
            let _ = Text::with_baseline(id.name(), text_at, style, Baseline::Top).draw(frame);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{FIRST_ROW, Menu, MenuOutcome};
    use crate::app::ROW_HEIGHT;
    use crate::apps::AppId;
    use crate::buttons::{ButtonEvents, ButtonReader};
    use crate::framebuffer::{FrameBuffer, WIDTH};
    use crate::platform::{BUTTONS, Button, ControlEvents, ENCODERS};

    /// Button events with `button` firing on this tick.
    ///
    /// The encoder switches fire while held and `up`/`down` only on release, so
    /// this drives a whole press through [`ButtonReader`] and hands back the
    /// tick on which the action actually landed.
    fn press(button: Button) -> ButtonEvents {
        let mut reader = ButtonReader::new();
        let mut controls = ControlEvents {
            encoder_delta: [0; ENCODERS],
            button_down: [false; BUTTONS],
        };

        controls.button_down[button.index()] = true;
        for _ in 0..8 {
            let events = reader.update(&controls);
            if events.pressed(button) {
                return events;
            }
        }

        controls.button_down[button.index()] = false;
        for _ in 0..8 {
            let events = reader.update(&controls);
            if events.pressed(button) {
                return events;
            }
        }
        panic!("{button:?} never fired");
    }

    #[test]
    fn opening_highlights_the_running_app() {
        let mut menu = Menu::new();
        menu.open(AppId::Scope);
        assert_eq!(menu.selected(), AppId::Scope);
        menu.reset();
        assert_eq!(menu.selected(), AppId::Diagnostic);
    }

    #[test]
    fn the_highlight_wraps_in_both_directions() {
        let mut menu = Menu::new();
        let count = i32::try_from(AppId::ALL.len()).unwrap();

        menu.move_by(-1);
        assert_eq!(menu.selected(), AppId::ALL[AppId::ALL.len() - 1]);
        menu.move_by(1);
        assert_eq!(menu.selected(), AppId::Diagnostic);
        menu.move_by(count * 3);
        assert_eq!(menu.selected(), AppId::Diagnostic, "a full turn comes home");
    }

    #[test]
    fn up_and_down_move_the_highlight() {
        let mut menu = Menu::new();
        assert_eq!(menu.update(&press(Button::Down), 0), None);
        assert_eq!(menu.selected(), AppId::Scope);
        assert_eq!(menu.update(&press(Button::Up), 0), None);
        assert_eq!(menu.selected(), AppId::Diagnostic);
    }

    #[test]
    fn the_left_encoder_moves_the_highlight_clockwise_down_the_list() {
        let mut menu = Menu::new();
        assert_eq!(menu.update(&ButtonEvents::default(), 1), None);
        assert_eq!(menu.selected(), AppId::Scope);
    }

    #[test]
    fn either_encoder_press_launches_the_highlighted_app() {
        for button in [Button::LeftEncoder, Button::RightEncoder] {
            let mut menu = Menu::new();
            menu.open(AppId::Scope);
            assert_eq!(
                menu.update(&press(button), 0),
                Some(MenuOutcome::Launch(AppId::Scope)),
                "{button:?} must confirm the highlighted app"
            );
        }
    }

    #[test]
    fn moving_and_confirming_in_the_same_tick_launches_what_is_now_highlighted() {
        let mut menu = Menu::new();
        let events = press(Button::LeftEncoder);
        assert_eq!(
            menu.update(&events, 1),
            Some(MenuOutcome::Launch(AppId::Scope)),
            "the detent is applied before the press is read"
        );
    }

    #[test]
    fn the_highlighted_row_is_drawn_in_inverse_video() {
        let mut frame = FrameBuffer::new();
        let menu = Menu::new();
        menu.render(&mut frame);

        let row = usize::try_from(FIRST_ROW).unwrap();
        let page = &frame.as_bytes()[row * WIDTH..(row + 1) * WIDTH];
        let band: u8 = 0xFF;
        assert_eq!(
            page[WIDTH - 1],
            band,
            "the highlight runs the full width of the screen"
        );
        assert!(
            page.iter().any(|&byte| byte != band),
            "the app name is punched out of the band in inverse video"
        );
    }

    #[test]
    fn every_app_is_listed_on_its_own_row() {
        let mut frame = FrameBuffer::new();
        Menu::new().render(&mut frame);
        for index in 0..AppId::ALL.len() {
            let row = usize::try_from(FIRST_ROW).unwrap() + index;
            let page = &frame.as_bytes()[row * WIDTH..(row + 1) * WIDTH];
            assert!(
                page.iter().any(|&byte| byte != 0),
                "row {row} must show an app name"
            );
        }
        assert_eq!(
            ROW_HEIGHT, 8,
            "the list assumes one row per framebuffer page"
        );
    }
}
