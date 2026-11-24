//! The app picker, opened by holding `up` and `down` together; its last entry,
//! `SETUP`, opens the system settings screen (journal/D-026).
//!
//! Like [`SplashScreen`](crate::splash::SplashScreen), this is a screen that
//! owns the display without owning the outputs: while it is up the running
//! applet keeps tracking its inputs and driving its jacks, and only the panel
//! and the framebuffer belong to the menu. Nothing here decides *when* the menu
//! opens or closes — that is [`Engine::tick`](crate::Engine)'s job — so this
//! module only knows how to move a highlight and draw a list.

use crate::gfx::controls::{LIST_LEFT, SCROLL_WIDTH, row_highlight, scroll_track, scroll_window};
use crate::gfx::{Ink, Point};

use crate::apps::AppId;
use crate::buttons::ButtonEvents;
use crate::framebuffer::FrameBuffer;
use crate::framebuffer::{ROW_HEIGHT, ROWS};
use crate::platform::Button;
use crate::text::fonts::FONT_5X8;
use crate::text::{self, TextStyle};

/// Title shown above the list.
///
/// A fixed string rather than [`crate::BANNER`]: the banner carries the crate
/// version, and this screen has a golden snapshot that should not churn every
/// time the version is bumped.
const TITLE: &str = "SELECT APP";

/// First screen row of the list, right under the title; past the screen,
/// the list scrolls (journal/D-046).
const FIRST_ROW: i32 = 1;

/// Horizontal padding of a list entry, so text does not touch the highlight edge.
const TEXT_INSET: i32 = LIST_LEFT + 2;

/// Label of the entry that opens the system settings.
const SETUP_LABEL: &str = "SETUP";

/// Label of the entry that adds an applet, and title of the list it opens.
const ADD_LABEL: &str = "ADD APP";

/// Label of the entry that stops the applet with the focus.
const REMOVE_LABEL: &str = "REMOVE APP";

/// Rows of entries under the title: past that, the list scrolls.
const VISIBLE: usize = (ROWS - FIRST_ROW) as usize;

/// Most entries: every app, `ADD APP`, `REMOVE APP`, then `SETUP`.
const MOST_ENTRIES: usize = AppId::ALL.len() + 3;

/// One line of the menu.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MenuEntry {
    /// An applet to run in the place of the one with the focus, or, while
    /// adding, beside it.
    App(AppId),
    /// Adds an applet right of the one with the focus: the list of apps
    /// again, to pick which (journal/D-046).
    Add,
    /// Stops the applet with the focus.
    Remove,
    /// The system settings screen.
    Setup,
}

impl MenuEntry {
    /// Its label.
    const fn label(self) -> &'static str {
        match self {
            Self::App(id) => id.name(),
            Self::Add => ADD_LABEL,
            Self::Remove => REMOVE_LABEL,
            Self::Setup => SETUP_LABEL,
        }
    }
}

/// What the menu decided on this tick.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MenuOutcome {
    /// The highlighted app was confirmed: it takes the place of the one with
    /// the focus.
    Launch(AppId),
    /// An app was picked from `ADD APP`: it starts right of the one with the
    /// focus.
    Add(AppId),
    /// `REMOVE APP` was confirmed: the one with the focus stops.
    Remove,
    /// `SETUP` was confirmed: open the system settings.
    OpenSetup,
}

/// The app picker.
#[derive(Debug, Clone, Copy, Default)]
pub struct Menu {
    selected: usize,
    /// Whether another applet can start: a slot is free.
    can_add: bool,
    /// Whether the applet with the focus can stop: it is not alone.
    can_remove: bool,
    /// Whether `ADD APP` was confirmed: the list is the apps to add.
    adding: bool,
}

impl Menu {
    /// A menu with the first app highlighted.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            selected: 0,
            can_add: false,
            can_remove: false,
            adding: false,
        }
    }

    /// Returns the menu to its initial state.
    pub const fn reset(&mut self) {
        *self = Self::new();
    }

    /// Opens the menu with `current`, the app with the focus, highlighted;
    /// `ADD APP` listed when `can_add`, `REMOVE APP` when `can_remove`.
    pub const fn open(&mut self, current: AppId, can_add: bool, can_remove: bool) {
        self.selected = current.index();
        self.can_add = can_add;
        self.can_remove = can_remove;
        self.adding = false;
    }

    /// Whether it lists the apps to add rather than the menu.
    #[must_use]
    pub const fn is_adding(&self) -> bool {
        self.adding
    }

    /// How many entries it lists now.
    const fn len(&self) -> usize {
        if self.adding {
            AppId::ALL.len()
        } else {
            AppId::ALL.len() + self.can_add as usize + self.can_remove as usize + 1
        }
    }

    /// The entry at `index`: the apps, then what applies of `ADD APP` and
    /// `REMOVE APP`, then `SETUP`; while adding, the apps alone.
    const fn at(&self, index: usize) -> MenuEntry {
        if let Some(id) = AppId::from_index(index) {
            return MenuEntry::App(id);
        }
        let mut extra = index - AppId::ALL.len();
        if self.can_add {
            if extra == 0 {
                return MenuEntry::Add;
            }
            extra -= 1;
        }
        if self.can_remove && extra == 0 {
            return MenuEntry::Remove;
        }
        MenuEntry::Setup
    }

    /// The entry currently highlighted.
    #[must_use]
    pub const fn selected(&self) -> MenuEntry {
        self.at(self.selected)
    }

    /// Moves the highlight, wrapping in both directions.
    pub fn move_by(&mut self, steps: i32) {
        if steps == 0 {
            return;
        }
        let count = i32::try_from(self.len()).unwrap_or(1);
        let selected = i32::try_from(self.selected).unwrap_or(0);
        self.selected =
            usize::try_from((selected + steps).rem_euclid(count)).unwrap_or(self.selected);
    }

    /// Feeds one tick of panel input and reports what was confirmed, if
    /// anything.
    ///
    /// `up` and `down` move the highlight, and so does the left encoder;
    /// clockwise moves *down* the list, matching the direction that encoder
    /// already selects channels in. Pressing either encoder confirms;
    /// confirming `ADD APP` lists the apps to add, with the highlight where
    /// it was.
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
            return match (self.adding, self.selected()) {
                (true, MenuEntry::App(id)) => Some(MenuOutcome::Add(id)),
                (_, MenuEntry::App(id)) => Some(MenuOutcome::Launch(id)),
                (_, MenuEntry::Add) => {
                    self.adding = true;
                    self.selected = 0;
                    None
                }
                (_, MenuEntry::Remove) => Some(MenuOutcome::Remove),
                (_, MenuEntry::Setup) => Some(MenuOutcome::OpenSetup),
            };
        }
        None
    }

    /// First entry shown, so that the highlight stays on screen.
    const fn first_visible(&self) -> usize {
        let last_start = self.len().saturating_sub(VISIBLE);
        let first = self.selected.saturating_sub(VISIBLE - 1);
        if first < last_start {
            first
        } else {
            last_start
        }
    }

    /// Draws the title and the entries in view, with the highlight in
    /// inverse video and the scroll bar at the left.
    pub fn render(&self, frame: &mut FrameBuffer) {
        frame.clear();
        let title = if self.adding { ADD_LABEL } else { TITLE };
        text::draw(frame, title, Point::zero(), TextStyle::lit(&FONT_5X8));

        let first = self.first_visible();
        let rows = i32::try_from(VISIBLE).unwrap_or(1);
        if self.len() > VISIBLE {
            scroll_window(
                frame,
                0,
                FIRST_ROW * ROW_HEIGHT,
                SCROLL_WIDTH,
                rows * ROW_HEIGHT,
                first,
                VISIBLE,
                self.len(),
            );
        } else {
            // One page: the scroll bar's place, empty, lines the entries up
            // with the screens that scroll.
            let shown = i32::try_from(self.len()).unwrap_or(1);
            scroll_track(
                frame,
                0,
                FIRST_ROW * ROW_HEIGHT,
                SCROLL_WIDTH,
                shown * ROW_HEIGHT,
            );
        }
        for (slot, index) in (first..self.len()).take(VISIBLE).enumerate() {
            let row = FIRST_ROW + i32::try_from(slot).unwrap_or(0);
            let top = row * ROW_HEIGHT;
            let highlighted = index == self.selected;
            if highlighted {
                row_highlight(frame, top, ROW_HEIGHT, Ink::Lit);
            }
            let style = if highlighted {
                TextStyle::dark(&FONT_5X8)
            } else {
                TextStyle::lit(&FONT_5X8)
            };
            text::draw(
                frame,
                self.at(index).label(),
                Point::new(TEXT_INSET, top),
                style,
            );
        }
    }
}

const _: () = assert!(MOST_ENTRIES > VISIBLE, "the menu scrolls once it has more");

#[cfg(test)]
mod tests {
    use super::{FIRST_ROW, LIST_LEFT, Menu, MenuEntry, MenuOutcome};
    use crate::apps::AppId;
    use crate::buttons::{ButtonEvents, ButtonReader};
    use crate::framebuffer::{FrameBuffer, WIDTH};
    use crate::framebuffer::{ROW_HEIGHT, ROWS};
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
        menu.open(AppId::Scope, false, false);
        assert_eq!(menu.selected(), MenuEntry::App(AppId::Scope));
        menu.reset();
        assert_eq!(menu.selected(), MenuEntry::App(AppId::Diagnostic));
    }

    #[test]
    fn the_highlight_wraps_in_both_directions() {
        let mut menu = Menu::new();
        let count = i32::try_from(AppId::ALL.len() + 1).unwrap();

        menu.move_by(-1);
        assert_eq!(menu.selected(), MenuEntry::Setup, "SETUP is last");
        menu.move_by(1);
        assert_eq!(menu.selected(), MenuEntry::App(AppId::Diagnostic));
        menu.move_by(count * 3);
        assert_eq!(
            menu.selected(),
            MenuEntry::App(AppId::Diagnostic),
            "a full turn comes home"
        );
    }

    #[test]
    fn up_and_down_move_the_highlight() {
        let mut menu = Menu::new();
        assert_eq!(menu.update(&press(Button::Down), 0), None);
        assert_eq!(menu.selected(), MenuEntry::App(AppId::Scope));
        assert_eq!(menu.update(&press(Button::Up), 0), None);
        assert_eq!(menu.selected(), MenuEntry::App(AppId::Diagnostic));
    }

    #[test]
    fn the_left_encoder_moves_the_highlight_clockwise_down_the_list() {
        let mut menu = Menu::new();
        assert_eq!(menu.update(&ButtonEvents::default(), 1), None);
        assert_eq!(menu.selected(), MenuEntry::App(AppId::Scope));
    }

    #[test]
    fn confirming_setup_asks_for_the_settings_screen() {
        let mut menu = Menu::new();
        menu.move_by(-1);
        assert_eq!(
            menu.update(&press(Button::RightEncoder), 0),
            Some(MenuOutcome::OpenSetup)
        );
    }

    #[test]
    fn either_encoder_press_launches_the_highlighted_app() {
        for button in [Button::LeftEncoder, Button::RightEncoder] {
            let mut menu = Menu::new();
            menu.open(AppId::Scope, false, false);
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
        let row_left = usize::try_from(LIST_LEFT).unwrap();
        assert_eq!(
            page[WIDTH - 1],
            band,
            "the highlight runs to the right edge of the screen"
        );
        assert!(
            page[row_left..].iter().any(|&byte| byte != band),
            "the app name is punched out of the band in inverse video"
        );
        assert_ne!(
            page[0], band,
            "the scroll bar's place stays outside the highlight"
        );
    }

    #[test]
    fn the_empty_scroll_track_stays_left_of_the_highlight() {
        let mut frame = FrameBuffer::new();
        Menu::new().render(&mut frame);

        assert!(frame.pixel(1, ROW_HEIGHT));
        assert!(!frame.pixel(0, ROW_HEIGHT));
        assert!(frame.pixel(LIST_LEFT, ROW_HEIGHT));
    }

    #[test]
    fn every_app_is_listed_on_its_own_row() {
        let mut frame = FrameBuffer::new();
        Menu::new().render(&mut frame);
        for index in 0..=AppId::ALL.len() {
            let row = usize::try_from(FIRST_ROW).unwrap() + index;
            let page = &frame.as_bytes()[row * WIDTH..(row + 1) * WIDTH];
            assert!(
                page.iter().any(|&byte| byte != 0),
                "row {row} must show an entry"
            );
        }
        assert_eq!(
            ROW_HEIGHT, 8,
            "the list assumes one row per framebuffer page"
        );
    }

    #[test]
    fn add_and_remove_sit_between_the_apps_and_setup_when_they_apply() {
        let mut menu = Menu::new();
        menu.open(AppId::Diagnostic, true, true);
        menu.move_by(-3);
        assert_eq!(menu.selected(), MenuEntry::Add);
        menu.move_by(1);
        assert_eq!(menu.selected(), MenuEntry::Remove);
        menu.move_by(1);
        assert_eq!(menu.selected(), MenuEntry::Setup);
        menu.open(AppId::Diagnostic, false, true);
        menu.move_by(-2);
        assert_eq!(menu.selected(), MenuEntry::Remove, "nothing to add");
        menu.open(AppId::Diagnostic, false, false);
        menu.move_by(-1);
        assert_eq!(menu.selected(), MenuEntry::Setup, "one app, no room");
    }

    #[test]
    fn add_lists_the_apps_to_start_beside() {
        let mut menu = Menu::new();
        menu.open(AppId::Diagnostic, true, false);
        menu.move_by(-2);
        assert_eq!(menu.update(&press(Button::RightEncoder), 0), None);
        assert!(menu.is_adding());
        assert_eq!(menu.selected(), MenuEntry::App(AppId::Diagnostic));
        menu.move_by(-1);
        assert_eq!(
            menu.selected(),
            MenuEntry::App(AppId::Supersaw),
            "the apps alone, wrapping"
        );
        assert_eq!(
            menu.update(&press(Button::LeftEncoder), 0),
            Some(MenuOutcome::Add(AppId::Supersaw))
        );
        menu.open(AppId::Tone, true, true);
        assert!(!menu.is_adding(), "opening again starts from the menu");
    }

    #[test]
    fn a_long_list_scrolls_with_the_highlight() {
        let mut menu = Menu::new();
        menu.open(AppId::Diagnostic, true, true);
        menu.move_by(-1);
        assert_eq!(menu.selected(), MenuEntry::Setup);
        let mut frame = FrameBuffer::new();
        menu.render(&mut frame);
        let last = usize::try_from(ROWS - 1).unwrap();
        let page = &frame.as_bytes()[last * WIDTH..(last + 1) * WIDTH];
        assert!(
            page.iter().any(|&byte| byte != 0),
            "SETUP, highlighted, on the last row"
        );
        assert_eq!(menu.first_visible(), 2, "nine entries, seven rows");
    }
}
