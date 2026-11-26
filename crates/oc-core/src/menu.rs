//! The app picker, opened by holding `up` and `down` together; its last entry,
//! `SETUP`, opens the system settings screen (journal/D-026).
//!
//! Two columns, each with its scroll bar: the categories on the left half of
//! the screen, the apps of the highlighted one on the right
//! (journal/D-021, revision of 2026-11-26). The left encoder picks the
//! category, the right one the app in it; `up` and `down` step through every
//! entry, category after category, as the single list did; the right
//! encoder's click confirms.
//!
//! Like [`SplashScreen`](crate::splash::SplashScreen), this is a screen that
//! owns the display without owning the outputs: while it is up the running
//! applet keeps tracking its inputs and driving its jacks, and only the panel
//! and the framebuffer belong to the menu. Nothing here decides *when* the menu
//! opens or closes — that is [`Engine::tick`](crate::Engine)'s job — so this
//! module only knows how to move a highlight and draw two lists.

use crate::gfx::controls::{LIST_LEFT, SCROLL_WIDTH, TITLE_LEFT, scroll_window};
use crate::gfx::{Ink, Point, fill_rect};

use crate::apps::{AppId, Category};
use crate::buttons::ButtonEvents;
use crate::framebuffer::FrameBuffer;
use crate::framebuffer::{ROW_HEIGHT, ROWS, WIDTH_I32};
use crate::platform::{Button, ENCODERS};
use crate::text::fonts::FONT_5X8;
use crate::text::{self, TextStyle};

/// Title shown above the lists.
///
/// A fixed string rather than [`crate::BANNER`]: the banner carries the crate
/// version, and this screen has a golden snapshot that should not churn every
/// time the version is bumped.
const TITLE: &str = "SELECT APP";

/// First screen row of the lists, right under the title; past the screen,
/// a list scrolls (journal/D-046).
const FIRST_ROW: i32 = 1;

/// Width of a column: half the screen.
const COLUMN: i32 = WIDTH_I32 / 2;

/// Left edge of the right column, where its scroll bar starts.
const RIGHT_COLUMN: i32 = COLUMN;

/// Width of the left column's highlight, from [`LIST_LEFT`]: it stops a pixel
/// short of the right column's scroll bar, so the two never touch.
const LEFT_HIGHLIGHT: i32 = COLUMN - LIST_LEFT - 1;

/// Width of the right column's highlight, from its [`LIST_LEFT`] to the
/// right edge of the screen.
const RIGHT_HIGHLIGHT: i32 = COLUMN - LIST_LEFT;

/// Horizontal padding of a list entry from its column's edge, so text does
/// not touch the highlight edge.
const TEXT_INSET: i32 = LIST_LEFT + 2;

/// Label of the entry that opens the system settings.
const SETUP_LABEL: &str = "SETUP";

/// Rows of entries under the title: past that, a list scrolls.
const VISIBLE: usize = (ROWS - FIRST_ROW) as usize;

/// The entries: every app, then `SETUP`.
const ENTRIES: usize = AppId::ALL.len() + 1;

/// How many categories the left column lists.
const CATEGORIES: usize = Category::ALL.len();

/// The entries in the order of the menu: category after category, each
/// listing its apps in the order of [`AppId::ALL`], `SETUP` last of
/// [`Category::System`].
const ORDER: [MenuEntry; ENTRIES] = order();

/// Where each category starts in [`ORDER`]; the last element is the end of
/// the last category.
const STARTS: [usize; CATEGORIES + 1] = starts();

/// One line of the menu.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MenuEntry {
    /// An applet to run in the place of the one with the focus;
    /// [`AppId::Nothing`] empties its slot (journal/D-046).
    App(AppId),
    /// The system settings screen.
    Setup,
}

impl MenuEntry {
    /// The entry of rank `index` among the apps, then `SETUP`: the order the
    /// menu sorts by category.
    const fn declared(index: usize) -> Self {
        match AppId::from_index(index) {
            Some(id) => Self::App(id),
            None => Self::Setup,
        }
    }

    /// Its label.
    const fn label(self) -> &'static str {
        match self {
            Self::App(id) => id.name(),
            Self::Setup => SETUP_LABEL,
        }
    }

    /// The category it is listed under.
    #[must_use]
    pub const fn category(self) -> Category {
        match self {
            Self::App(id) => id.category(),
            Self::Setup => Category::System,
        }
    }
}

/// Builds [`ORDER`]: a stable sort of the declared entries by category.
const fn order() -> [MenuEntry; ENTRIES] {
    let mut sorted = [MenuEntry::Setup; ENTRIES];
    let mut filled = 0;
    let mut category = 0;
    while category < CATEGORIES {
        let mut index = 0;
        while index < ENTRIES {
            let entry = MenuEntry::declared(index);
            if entry.category().index() == category {
                sorted[filled] = entry;
                filled += 1;
            }
            index += 1;
        }
        category += 1;
    }
    sorted
}

/// Builds [`STARTS`] from [`ORDER`].
const fn starts() -> [usize; CATEGORIES + 1] {
    let mut starts = [ENTRIES; CATEGORIES + 1];
    let mut category = 0;
    let mut index = 0;
    while category < CATEGORIES {
        starts[category] = index;
        while index < ENTRIES && ORDER[index].category().index() == category {
            index += 1;
        }
        category += 1;
    }
    starts
}

/// Moves `index` by `steps` among `count` places, wrapping in both
/// directions.
fn wrap(index: usize, steps: i32, count: usize) -> usize {
    let count = i32::try_from(count).unwrap_or(1).max(1);
    let index_i32 = i32::try_from(index).unwrap_or(0);
    usize::try_from((index_i32 + steps).rem_euclid(count)).unwrap_or(index)
}

/// First item shown of a list of `total`, so that `selected` stays on
/// screen.
const fn first_visible(selected: usize, total: usize) -> usize {
    let last_start = total.saturating_sub(VISIBLE);
    let first = selected.saturating_sub(VISIBLE - 1);
    if first < last_start {
        first
    } else {
        last_start
    }
}

/// Draws one column from `x`: its scroll bar, the items in view, the one at
/// `selected` in inverse video over a band `highlight` pixels wide.
fn draw_column(
    frame: &mut FrameBuffer,
    x: i32,
    highlight: i32,
    total: usize,
    selected: usize,
    label: impl Fn(usize) -> &'static str,
) {
    let first = first_visible(selected, total);
    let rows = i32::try_from(VISIBLE).unwrap_or(1);
    scroll_window(
        frame,
        x,
        FIRST_ROW * ROW_HEIGHT,
        SCROLL_WIDTH,
        rows * ROW_HEIGHT,
        first,
        VISIBLE,
        total,
    );
    for (slot, index) in (first..total).take(VISIBLE).enumerate() {
        let top = (FIRST_ROW + i32::try_from(slot).unwrap_or(0)) * ROW_HEIGHT;
        let style = if index == selected {
            fill_rect(frame, x + LIST_LEFT, top, highlight, ROW_HEIGHT, Ink::Lit);
            TextStyle::dark(&FONT_5X8)
        } else {
            TextStyle::lit(&FONT_5X8)
        };
        text::draw(frame, label(index), Point::new(x + TEXT_INSET, top), style);
    }
}

/// What the menu decided on this tick.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MenuOutcome {
    /// The highlighted app was confirmed: it takes the place of the one with
    /// the focus.
    Launch(AppId),
    /// `SETUP` was confirmed: open the system settings.
    OpenSetup,
}

/// The app picker.
#[derive(Debug, Clone, Copy, Default)]
pub struct Menu {
    /// The highlighted entry, as its place in [`ORDER`]; the highlighted
    /// category is its own.
    selected: usize,
}

impl Menu {
    /// A menu with the first app of the first category highlighted.
    #[must_use]
    pub const fn new() -> Self {
        Self { selected: 0 }
    }

    /// Returns the menu to its initial state.
    pub const fn reset(&mut self) {
        *self = Self::new();
    }

    /// Opens the menu with `current`, the app with the focus, highlighted in
    /// its category.
    pub const fn open(&mut self, current: AppId) {
        let mut index = 0;
        while index < ENTRIES {
            if let MenuEntry::App(id) = ORDER[index]
                && id.index() == current.index()
            {
                self.selected = index;
                return;
            }
            index += 1;
        }
    }

    /// The entry currently highlighted.
    #[must_use]
    pub const fn selected(&self) -> MenuEntry {
        ORDER[self.selected]
    }

    /// The category currently highlighted: that of the entry.
    #[must_use]
    pub const fn category(&self) -> Category {
        self.selected().category()
    }

    /// Moves the highlight through every entry, category after category,
    /// wrapping in both directions: what `up` and `down` do.
    pub fn move_by(&mut self, steps: i32) {
        self.selected = wrap(self.selected, steps, ENTRIES);
    }

    /// Moves to another category, wrapping, and highlights its first entry:
    /// what the left encoder does.
    pub fn move_category(&mut self, steps: i32) {
        if steps == 0 {
            return;
        }
        let category = wrap(self.category().index(), steps, CATEGORIES);
        self.selected = STARTS[category];
    }

    /// Moves the highlight within the category, wrapping at its ends: what
    /// the right encoder does.
    pub fn move_in_category(&mut self, steps: i32) {
        let category = self.category().index();
        let start = STARTS[category];
        let count = STARTS[category + 1] - start;
        self.selected = start + wrap(self.selected - start, steps, count);
    }

    /// Feeds one tick of panel input and reports what was confirmed, if
    /// anything.
    ///
    /// The left encoder picks the category and the right one the app in it,
    /// each column under its own encoder; `up` and `down` step through every
    /// entry. Clockwise moves *down* a list. Pressing the right encoder
    /// confirms, after the detents of the same tick.
    pub fn update(
        &mut self,
        buttons: &ButtonEvents,
        encoder_delta: [i8; ENCODERS],
    ) -> Option<MenuOutcome> {
        let [left, right] = encoder_delta;
        self.move_category(i32::from(left));
        self.move_in_category(i32::from(right));
        let mut steps = 0;
        if buttons.pressed(Button::Down) {
            steps += 1;
        }
        if buttons.pressed(Button::Up) {
            steps -= 1;
        }
        self.move_by(steps);

        if buttons.pressed(Button::RightEncoder) {
            return match self.selected() {
                MenuEntry::App(id) => Some(MenuOutcome::Launch(id)),
                MenuEntry::Setup => Some(MenuOutcome::OpenSetup),
            };
        }
        None
    }

    /// Draws the title, the categories on the left half and the entries of
    /// the highlighted one on the right, each list with its scroll bar and
    /// its highlight in inverse video.
    pub fn render(&self, frame: &mut FrameBuffer) {
        frame.clear();
        // Right of the column where the engine marks the slots, as on an
        // applet's screen: the mark of the one with the focus says which the
        // app picked replaces.
        text::draw(
            frame,
            TITLE,
            Point::new(TITLE_LEFT, 0),
            TextStyle::lit(&FONT_5X8),
        );

        let category = self.category().index();
        draw_column(frame, 0, LEFT_HIGHLIGHT, CATEGORIES, category, |index| {
            Category::ALL[index].name()
        });
        let start = STARTS[category];
        draw_column(
            frame,
            RIGHT_COLUMN,
            RIGHT_HIGHLIGHT,
            STARTS[category + 1] - start,
            self.selected - start,
            |index| ORDER[start + index].label(),
        );
    }
}

/// Every category lists something, so that the right column is never empty
/// and [`Menu::move_in_category`] always has an entry to land on.
const _: () = {
    let mut category = 0;
    while category < CATEGORIES {
        assert!(
            STARTS[category] < STARTS[category + 1],
            "every category lists an entry"
        );
        category += 1;
    }
    assert!(STARTS[CATEGORIES] == ENTRIES, "every entry is listed");
};

#[cfg(test)]
mod tests {
    use super::{
        COLUMN, FIRST_ROW, LEFT_HIGHLIGHT, LIST_LEFT, Menu, MenuEntry, MenuOutcome, ORDER,
        RIGHT_COLUMN,
    };
    use crate::apps::{AppId, Category};
    use crate::buttons::{ButtonEvents, ButtonReader};
    use crate::framebuffer::{FrameBuffer, ROW_HEIGHT, WIDTH_I32};
    use crate::platform::{BUTTONS, Button, ControlEvents, ENCODERS};

    /// No detent on either encoder.
    const STILL: [i8; ENCODERS] = [0; ENCODERS];

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

    /// Whether any pixel of the band `top..top + ROW_HEIGHT` is lit between
    /// columns `left` and `right` (excluded).
    fn lit(frame: &FrameBuffer, left: i32, right: i32, top: i32) -> bool {
        (left..right).any(|x| (top..top + ROW_HEIGHT).any(|y| frame.pixel(x, y)))
    }

    #[test]
    fn the_order_is_category_after_category() {
        assert!(
            ORDER
                .windows(2)
                .all(|pair| pair[0].category().index() <= pair[1].category().index())
        );
        assert_eq!(ORDER.last(), Some(&MenuEntry::Setup), "SETUP is last");
        assert_eq!(
            ORDER[0],
            MenuEntry::App(AppId::Diagnostic),
            "the boot app first"
        );
    }

    #[test]
    fn opening_highlights_the_running_app_in_its_category() {
        let mut menu = Menu::new();
        menu.open(AppId::Supersaw);
        assert_eq!(menu.selected(), MenuEntry::App(AppId::Supersaw));
        assert_eq!(menu.category(), Category::Sound);
        menu.reset();
        assert_eq!(menu.selected(), MenuEntry::App(AppId::Diagnostic));
        assert_eq!(menu.category(), Category::Tools);
    }

    #[test]
    fn up_and_down_cross_from_one_category_to_the_next() {
        let mut menu = Menu::new();
        menu.open(AppId::Gfx);
        assert_eq!(menu.update(&press(Button::Down), STILL), None);
        assert_eq!(menu.selected(), MenuEntry::App(AppId::Ratchet));
        assert_eq!(menu.category(), Category::Triggers);
        assert_eq!(menu.update(&press(Button::Up), STILL), None);
        assert_eq!(menu.selected(), MenuEntry::App(AppId::Gfx));
    }

    #[test]
    fn up_and_down_wrap_around_the_whole_menu() {
        let mut menu = Menu::new();
        let count = i32::try_from(ORDER.len()).unwrap();

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
    fn the_left_encoder_picks_the_category_and_its_first_entry() {
        let mut menu = Menu::new();
        menu.open(AppId::Scope);
        assert_eq!(menu.update(&ButtonEvents::default(), [1, 0]), None);
        assert_eq!(menu.category(), Category::Triggers);
        assert_eq!(menu.selected(), MenuEntry::App(AppId::Ratchet));

        menu.update(&ButtonEvents::default(), [2, 0]);
        assert_eq!(menu.category(), Category::System);
        assert_eq!(menu.selected(), MenuEntry::App(AppId::Nothing));

        menu.update(&ButtonEvents::default(), [1, 0]);
        assert_eq!(menu.category(), Category::Tools, "it wraps");
        menu.update(&ButtonEvents::default(), [-1, 0]);
        assert_eq!(menu.category(), Category::System, "both ways");
    }

    #[test]
    fn the_right_encoder_moves_within_the_category_and_wraps_there() {
        let mut menu = Menu::new();
        menu.update(&ButtonEvents::default(), [0, 1]);
        assert_eq!(menu.selected(), MenuEntry::App(AppId::Scope));
        menu.update(&ButtonEvents::default(), [0, 2]);
        assert_eq!(
            menu.selected(),
            MenuEntry::App(AppId::Diagnostic),
            "past GFX DEMO, back to the top of TOOLS, not on to RATCHET"
        );
        menu.update(&ButtonEvents::default(), [0, -1]);
        assert_eq!(menu.selected(), MenuEntry::App(AppId::Gfx));
    }

    #[test]
    fn setup_is_in_system_after_nothing() {
        let mut menu = Menu::new();
        menu.update(&ButtonEvents::default(), [-1, 1]);
        assert_eq!(menu.category(), Category::System);
        assert_eq!(menu.selected(), MenuEntry::Setup);
        assert_eq!(
            menu.update(&press(Button::RightEncoder), STILL),
            Some(MenuOutcome::OpenSetup)
        );
    }

    #[test]
    fn the_right_encoder_press_launches_the_highlighted_app() {
        let mut menu = Menu::new();
        menu.open(AppId::Scope);
        assert_eq!(
            menu.update(&press(Button::RightEncoder), STILL),
            Some(MenuOutcome::Launch(AppId::Scope))
        );
    }

    #[test]
    fn the_left_encoder_press_launches_nothing() {
        let mut menu = Menu::new();
        menu.open(AppId::Scope);
        assert_eq!(menu.update(&press(Button::LeftEncoder), STILL), None);
        assert_eq!(menu.selected(), MenuEntry::App(AppId::Scope));
    }

    #[test]
    fn moving_and_confirming_in_the_same_tick_launches_what_is_now_highlighted() {
        let mut menu = Menu::new();
        let events = press(Button::RightEncoder);
        assert_eq!(
            menu.update(&events, [1, 1]),
            Some(MenuOutcome::Launch(AppId::Ratchet)),
            "the detents are applied before the press is read: TRIGGERS, then \
             its only entry"
        );
    }

    #[test]
    fn nothing_is_the_last_app_before_setup() {
        let mut menu = Menu::new();
        menu.move_by(-2);
        assert_eq!(menu.selected(), MenuEntry::App(AppId::Nothing));
        assert_eq!(
            menu.update(&press(Button::RightEncoder), STILL),
            Some(MenuOutcome::Launch(AppId::Nothing)),
            "it empties the slot shown"
        );
    }

    #[test]
    fn each_column_has_its_scroll_bar_and_its_highlight() {
        let mut frame = FrameBuffer::new();
        Menu::new().render(&mut frame);
        let top = FIRST_ROW * ROW_HEIGHT;

        for x in [0, RIGHT_COLUMN] {
            assert!(frame.pixel(x + 1, top), "a scroll bar at {x}");
            assert!(!frame.pixel(x + LIST_LEFT - 1, top), "then a gap");
            assert!(frame.pixel(x + LIST_LEFT, top), "then the highlight");
        }
        assert!(
            frame.pixel(LIST_LEFT + LEFT_HIGHLIGHT - 1, top),
            "the category's band runs to the middle"
        );
        assert!(
            !frame.pixel(COLUMN - 1, top),
            "and stops short of the right column's scroll bar"
        );
        assert!(
            frame.pixel(WIDTH_I32 - 1, top),
            "the app's band runs to the right edge"
        );
    }

    #[test]
    fn the_right_column_lists_the_apps_of_the_highlighted_category() {
        let mut menu = Menu::new();
        let mut frame = FrameBuffer::new();
        menu.render(&mut frame);
        let row = |index: i32| (FIRST_ROW + index) * ROW_HEIGHT;
        let text_left = RIGHT_COLUMN + LIST_LEFT + 2;

        for index in 0..3 {
            assert!(
                lit(&frame, text_left, WIDTH_I32, row(index)),
                "TOOLS lists three apps"
            );
        }
        assert!(!lit(&frame, text_left, WIDTH_I32, row(3)));
        for index in 0..4 {
            assert!(
                lit(&frame, LIST_LEFT + 2, COLUMN - 1, row(index)),
                "four categories"
            );
        }

        menu.move_category(1);
        menu.render(&mut frame);
        assert!(
            !lit(&frame, text_left, WIDTH_I32, row(1)),
            "TRIGGERS lists one app"
        );
    }
}
