//! The `SUPERSAW` applet (journal/D-037): seven detuned band-limited
//! sawtooths around one pitch, the bank of [`crate::audio::supersaw`].
//!
//! A and B carry all seven voices, or the left and right halves with
//! `STEREO ON`; C a sawtooth an octave below; D rests at 0 V. The left
//! encoder picks a setting, the right one changes it. `SHAPE` blends each
//! voice from a sawtooth into a pulse, `WIDTH` sets that pulse's width, a
//! square at 50 % (journal/D-037, revisions of 2026-11-19). `CALC` picks the
//! bank's arithmetic, integers or `f32`, to compare the two on the module
//! (journal/D-039). The settings an input can move are on the first page,
//! `STEREO` and `CALC` on the second, a scroll bar left of them showing
//! which.
//!
//! Every setting but `STEREO` and `CALC` can take an input, chosen in the submenu the
//! right encoder's click opens (journal/D-036, [`Assign`]): it sets the
//! setting, adds to the panel's value or multiplies it. `NOTE` takes `CV1`
//! at start, added at one volt per octave; the others take none, so that an
//! unplugged input, which reads a few millivolts of noise on the module,
//! moves nothing (journal/D-037, revision of 2026-11-18).

use core::fmt::Write as _;

use crate::apps::tone::{self, HIGHEST_NOTE, LOWEST_NOTE, note_name};
use crate::apps::{App, AppId, InputSnapshot, TickContext};
use crate::audio::supersaw::{
    Arithmetic, FULL, NARROWEST_WIDTH, OFFSETS, Part, SQUARE_WIDTH, Supersaw, detune_curve,
    voice_gains,
};
use crate::audio::{Channel, MilliHertz, Params};
use crate::fixed::{self, ONE};
use crate::fmt::{TextBuf, write_volts};
use crate::framebuffer::{FrameBuffer, ROW_HEIGHT, WIDTH_I32};
use crate::gfx::controls::scroll_bar;
use crate::gfx::plots::Area;
use crate::gfx::{self, Ink, Point};
use crate::modulation::editor::{AssignEditor, Param};
use crate::modulation::{Assign, AssignLine, Effect, multiplied};
use crate::platform::{Button, CV_CHANNELS, CvChannel, MilliVolts};
use crate::text::fonts::FONT_5X8;
use crate::text::{self, Alignment, TextStyle};

/// The note at start, A2: a supersaw is a bass or a pad more than a lead.
pub const DEFAULT_NOTE: u8 = 45;

/// The detune and the mix at start, half their course.
pub const DEFAULT_AMOUNT: u16 = FULL / 2;

/// What one detent changes the detune or the mix by: one percent.
pub const AMOUNT_STEP: u16 = 10;

/// The highest peak level of one sawtooth.
pub const MAX_LEVEL_MV: MilliVolts = 3_000;

/// The level at start.
pub const DEFAULT_LEVEL_MV: MilliVolts = 1_000;

/// What one detent changes the level by.
pub const LEVEL_STEP_MV: MilliVolts = 100;

/// The signal that sweeps the detune, the mix or the level over its whole
/// course when it follows its input: 0 to 5 V, 20 % per volt.
const COURSE_MV: MilliVolts = 5_000;

/// The note an input that sets `NOTE` gives at 0 V, C4: −3 V reach C1 and
/// +4 V C8.
pub const SET_NOTE: u8 = 60;

/// One setting of the applet, in the order of the screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Setting {
    /// The note, C1 to C8.
    Note,
    /// The spread of the side voices.
    Detune,
    /// The side voices against the centre.
    Mix,
    /// Each voice's waveform, from a sawtooth to a pulse.
    Shape,
    /// The pulse's width, 5 to 50 % of the period.
    Width,
    /// The peak level of one sawtooth.
    Level,
    /// A and B as one mono output twice, or left and right.
    Stereo,
    /// The bank in integers or in `f32` (journal/D-039).
    Calc,
}

impl Setting {
    /// Every setting, top to bottom.
    pub const ALL: [Self; 8] = [
        Self::Note,
        Self::Detune,
        Self::Mix,
        Self::Shape,
        Self::Width,
        Self::Level,
        Self::Stereo,
        Self::Calc,
    ];

    /// The label on screen.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Note => "NOTE",
            Self::Detune => "DETUNE",
            Self::Mix => "MIX",
            Self::Shape => "SHAPE",
            Self::Width => "WIDTH",
            Self::Level => "LEVEL",
            Self::Stereo => "STEREO",
            Self::Calc => "CALC",
        }
    }

    /// Where this setting's input is kept, `None` for `STEREO` and `CALC`,
    /// which take none.
    const fn input_slot(self) -> Option<usize> {
        match self {
            Self::Note => Some(0),
            Self::Detune => Some(1),
            Self::Mix => Some(2),
            Self::Level => Some(3),
            Self::Shape => Some(4),
            Self::Width => Some(5),
            Self::Stereo | Self::Calc => None,
        }
    }
}

/// The range of the signal that sweeps `setting` over all its values when
/// it follows its input (journal/D-036), from the signal that gives its
/// least value to the one that gives its most; `None` for `STEREO` and
/// `CALC`.
/// [`crate::modulation::usable_range`] derives from it the ranges of the
/// other effects.
#[must_use]
pub const fn sweep(setting: Setting) -> Option<(MilliVolts, MilliVolts)> {
    match setting {
        // C1 to C8 at one volt per octave, C4 at 0 V.
        Setting::Note => Some((
            (LOWEST_NOTE as MilliVolts - SET_NOTE as MilliVolts) * 1_000 / 12,
            (HIGHEST_NOTE as MilliVolts - SET_NOTE as MilliVolts) * 1_000 / 12,
        )),
        Setting::Detune | Setting::Mix | Setting::Shape | Setting::Width | Setting::Level => {
            Some((0, COURSE_MV))
        }
        Setting::Stereo | Setting::Calc => None,
    }
}

/// The pitch an input makes of `note`: added at one volt per octave, set at
/// one volt per octave from [`SET_NOTE`], or the frequency multiplied (5 V
/// leaving it, 10 V an octave up).
#[must_use]
pub fn frequency_from(note: u8, millivolts: MilliVolts, effect: Effect) -> MilliHertz {
    match effect {
        Effect::Add => tone::frequency(note, millivolts),
        Effect::Set => tone::frequency(SET_NOTE, millivolts),
        Effect::Mul => {
            let product = multiplied(i64::from(tone::frequency(note, 0)), millivolts);
            let (low, high) = tone::FREQUENCY_RANGE;
            MilliHertz::try_from(product.clamp(i64::from(low), i64::from(high))).unwrap_or(low)
        }
    }
}

/// The detune or the mix an input makes of `amount`, in thousandths: 20 % per
/// volt from 0 % at 0 V when it is followed, added, or multiplied.
#[must_use]
pub fn amount_from(amount: u16, millivolts: MilliVolts, effect: Effect) -> u16 {
    let moved = swept(i64::from(amount), i64::from(FULL), millivolts, effect);
    u16::try_from(moved).unwrap_or(0)
}

/// The level an input makes of `level`: 0 to 3 V over 0 to 5 V when it is
/// followed, added at that rate, or multiplied, a VCA under an envelope.
#[must_use]
pub fn level_from(level: MilliVolts, millivolts: MilliVolts, effect: Effect) -> MilliVolts {
    let moved = swept(
        i64::from(level),
        i64::from(MAX_LEVEL_MV),
        millivolts,
        effect,
    );
    MilliVolts::try_from(moved).unwrap_or(0)
}

/// The pulse width an input makes of `width`, in thousandths of the period:
/// its course from 5 to 50 % over 0 to 5 V when it is followed, added at
/// that rate, or multiplied (from the narrowest, 5 V leaving it).
#[must_use]
pub fn width_from(width: u16, millivolts: MilliVolts, effect: Effect) -> u16 {
    let narrowest = i64::from(NARROWEST_WIDTH);
    let course = i64::from(SQUARE_WIDTH) - narrowest;
    let moved = swept(i64::from(width) - narrowest, course, millivolts, effect);
    u16::try_from(moved + narrowest).unwrap_or(SQUARE_WIDTH)
}

/// `value`, on a course from 0 to `full` that [`COURSE_MV`] sweeps, moved by
/// `millivolts` under `effect`, within the course.
fn swept(value: i64, full: i64, millivolts: MilliVolts, effect: Effect) -> i64 {
    let followed = i64::from(millivolts) * full / i64::from(COURSE_MV);
    let moved = match effect {
        Effect::Set => followed,
        Effect::Add => value + followed,
        Effect::Mul => multiplied(value, millivolts),
    };
    moved.clamp(0, full)
}

/// What each setting takes at start: `CV1` added to the note, the V/oct an
/// oscillator is patched by; nothing for the others.
const DEFAULT_ASSIGNS: [Assign; 6] = [
    Assign {
        input: Some(CvChannel::One),
        ..Assign::NONE
    },
    Assign::NONE,
    Assign::NONE,
    Assign::NONE,
    Assign::NONE,
    Assign::NONE,
];

/// Right edge of a setting's value, left of the input's name.
const VALUE_RIGHT: i32 = 98;

/// Settings shown per page: the waveform's on the first, `LEVEL`, `STEREO`
/// and `CALC` on the second.
const PAGE_ROWS: usize = 5;

/// Pages of settings.
const PAGES: usize = Setting::ALL.len().div_ceil(PAGE_ROWS);

/// Width of the scroll bar left of the settings, which shows the page.
const SCROLL_WIDTH: i32 = 3;

/// Left edge of the settings' rows, right of the scroll bar and a gap.
const ROW_LEFT: i32 = SCROLL_WIDTH + 1;

/// Left column of the waveform beside `SHAPE`, between its label and its
/// value.
const SHAPE_ICON_LEFT: i32 = 40;

/// Columns of one period of that waveform: two periods show the jumps.
const SHAPE_ICON_PERIOD: i32 = 10;

/// Top row of the voice spread drawing, a row below the settings.
const SPREAD_TOP: i32 = 6 * ROW_HEIGHT;

/// Height of the tallest voice in the drawing.
const SPREAD_HEIGHT: i32 = 2 * ROW_HEIGHT - 1;

/// Pixels from the centre to the widest voice at full detune.
const SPREAD_HALF_WIDTH: i64 = 60;

/// Seven detuned sawtooths.
#[derive(Debug, Clone)]
pub struct SupersawApp {
    selected: usize,
    note: u8,
    detune: u16,
    mix: u16,
    shape: u16,
    width: u16,
    level: MilliVolts,
    stereo: bool,
    arithmetic: Arithmetic,
    /// The input of each setting that takes one (see `Setting::input_slot`).
    assigns: [Assign; 6],
    /// The selected setting's input submenu, `None` while it is closed.
    assigning: Option<AssignEditor>,
    /// The last inputs.
    cv: [MilliVolts; CV_CHANNELS],
}

impl SupersawApp {
    /// A2, half detuned, half mixed, 1 V, mono, in integers; `CV1` on the
    /// note.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            selected: 0,
            note: DEFAULT_NOTE,
            detune: DEFAULT_AMOUNT,
            mix: DEFAULT_AMOUNT,
            shape: 0,
            width: SQUARE_WIDTH,
            level: DEFAULT_LEVEL_MV,
            stereo: false,
            arithmetic: Arithmetic::Fixed,
            assigns: DEFAULT_ASSIGNS,
            assigning: None,
            cv: [0; CV_CHANNELS],
        }
    }

    /// Consumes one snapshot and returns what the outputs carry.
    pub fn update(&mut self, input: &InputSnapshot) -> Params {
        self.follow_panel(input);
        self.cv = input.cv;
        let slot = self.selected().input_slot();
        if let (Some(editor), Some(slot)) = (&mut self.assigning, slot) {
            editor.observe(&self.assigns[slot], &self.cv, input.elapsed_micros);
        }
        self.params()
    }

    /// The submenu follows the panel while it is open; otherwise the left
    /// encoder picks a setting, the right one changes it, and its click
    /// opens the setting's submenu.
    fn follow_panel(&mut self, input: &InputSnapshot) {
        let selected = self.selected();
        if let (Some(editor), Some(slot), Some(sweep)) =
            (&mut self.assigning, selected.input_slot(), sweep(selected))
        {
            if !editor.follow(&mut self.assigns[slot], input, sweep) {
                self.assigning = None;
            }
            return;
        }
        let pick = i32::try_from(self.selected).unwrap_or(0) + i32::from(input.controls.delta(0));
        let last = i32::try_from(Setting::ALL.len() - 1).unwrap_or(0);
        self.selected = usize::try_from(pick.clamp(0, last)).unwrap_or(0);
        if input.buttons.pressed(Button::RightEncoder) && self.selected().input_slot().is_some() {
            self.assigning = Some(AssignEditor::new());
        }
        let turn = input.controls.delta(1);
        if turn != 0 {
            self.change(self.selected(), i32::from(turn));
        }
    }

    /// Turns `setting` by `detents`.
    fn change(&mut self, setting: Setting, detents: i32) {
        match setting {
            Setting::Note => {
                let note = (i32::from(self.note) + detents)
                    .clamp(i32::from(LOWEST_NOTE), i32::from(HIGHEST_NOTE));
                self.note = u8::try_from(note).unwrap_or(DEFAULT_NOTE);
            }
            Setting::Detune => self.detune = step_amount(self.detune, detents),
            Setting::Mix => self.mix = step_amount(self.mix, detents),
            Setting::Shape => self.shape = step_amount(self.shape, detents),
            Setting::Width => {
                let width = i32::from(self.width) + detents * i32::from(AMOUNT_STEP);
                let width = width.clamp(i32::from(NARROWEST_WIDTH), i32::from(SQUARE_WIDTH));
                self.width = u16::try_from(width).unwrap_or(SQUARE_WIDTH);
            }
            Setting::Level => {
                self.level = (self.level + detents * LEVEL_STEP_MV).clamp(0, MAX_LEVEL_MV);
            }
            // Any detent flips it.
            Setting::Stereo => self.stereo = !self.stereo,
            Setting::Calc => {
                self.arithmetic = match self.arithmetic {
                    Arithmetic::Fixed => Arithmetic::Float,
                    Arithmetic::Float => Arithmetic::Fixed,
                };
            }
        }
    }

    /// What the outputs carry.
    #[must_use]
    pub fn params(&self) -> Params {
        let mut params = Params::REST;
        let (a, b) = if self.stereo {
            (Part::Left, Part::Right)
        } else {
            (Part::Mono, Part::Mono)
        };
        params.channels[0] = Channel::Supersaw(a);
        params.channels[1] = Channel::Supersaw(b);
        params.channels[2] = Channel::Supersaw(Part::Sub);
        params.supersaw = Supersaw {
            frequency: self.frequency(),
            detune: self.played_detune(),
            mix: self.played_mix(),
            shape: self.played_shape(),
            width: self.played_width(),
            amplitude: self.played_level(),
            arithmetic: self.arithmetic,
        };
        params
    }

    /// The setting the right encoder changes.
    #[must_use]
    pub const fn selected(&self) -> Setting {
        Setting::ALL[self.selected]
    }

    /// The page of settings on screen, from 0: the selected setting's.
    #[must_use]
    pub const fn page(&self) -> usize {
        self.selected / PAGE_ROWS
    }

    /// The note set on the panel, as a MIDI number.
    #[must_use]
    pub const fn note(&self) -> u8 {
        self.note
    }

    /// The detune set on the panel, in thousandths.
    #[must_use]
    pub const fn detune(&self) -> u16 {
        self.detune
    }

    /// The mix set on the panel, in thousandths.
    #[must_use]
    pub const fn mix(&self) -> u16 {
        self.mix
    }

    /// The shape set on the panel, in thousandths: 0 a sawtooth, all a
    /// square.
    #[must_use]
    pub const fn shape(&self) -> u16 {
        self.shape
    }

    /// The peak level of one sawtooth.
    #[must_use]
    pub const fn level(&self) -> MilliVolts {
        self.level
    }

    /// Whether A and B are left and right.
    #[must_use]
    pub const fn stereo(&self) -> bool {
        self.stereo
    }

    /// The bank's arithmetic.
    #[must_use]
    pub const fn arithmetic(&self) -> Arithmetic {
        self.arithmetic
    }

    /// The input `setting` takes, `None` for `STEREO` and `CALC`.
    #[must_use]
    pub fn assign(&self, setting: Setting) -> Option<&Assign> {
        setting.input_slot().map(|slot| &self.assigns[slot])
    }

    /// The field picked in the selected setting's input submenu, `None`
    /// while it is closed.
    #[must_use]
    pub const fn assigning(&self) -> Option<AssignLine> {
        match &self.assigning {
            Some(editor) => Some(editor.line()),
            None => None,
        }
    }

    /// The shaped signal of `setting`'s input and its effect, `None` without
    /// an input.
    fn reading(&self, setting: Setting) -> Option<(MilliVolts, Effect)> {
        self.assign(setting)
            .and_then(|assign| assign.reading(&self.cv))
    }

    /// The pitch played: the note and its input.
    #[must_use]
    pub fn frequency(&self) -> MilliHertz {
        self.reading(Setting::Note)
            .map_or(tone::frequency(self.note, 0), |(millivolts, effect)| {
                frequency_from(self.note, millivolts, effect)
            })
    }

    /// The detune played: the panel's and its input.
    #[must_use]
    pub fn played_detune(&self) -> u16 {
        self.reading(Setting::Detune)
            .map_or(self.detune, |(millivolts, effect)| {
                amount_from(self.detune, millivolts, effect)
            })
    }

    /// The pulse width set on the panel, in thousandths of the period.
    #[must_use]
    pub const fn width(&self) -> u16 {
        self.width
    }

    /// The pulse width played: the panel's and its input.
    #[must_use]
    pub fn played_width(&self) -> u16 {
        self.reading(Setting::Width)
            .map_or(self.width, |(millivolts, effect)| {
                width_from(self.width, millivolts, effect)
            })
    }

    /// The shape played: the panel's and its input.
    #[must_use]
    pub fn played_shape(&self) -> u16 {
        self.reading(Setting::Shape)
            .map_or(self.shape, |(millivolts, effect)| {
                amount_from(self.shape, millivolts, effect)
            })
    }

    /// The mix played: the panel's and its input.
    #[must_use]
    pub fn played_mix(&self) -> u16 {
        self.reading(Setting::Mix)
            .map_or(self.mix, |(millivolts, effect)| {
                amount_from(self.mix, millivolts, effect)
            })
    }

    /// The level played: the panel's and its input.
    #[must_use]
    pub fn played_level(&self) -> MilliVolts {
        self.reading(Setting::Level)
            .map_or(self.level, |(millivolts, effect)| {
                level_from(self.level, millivolts, effect)
            })
    }

    /// Draws the pitch, the settings and their inputs, the one being
    /// changed, and the seven voices: where each sits in pitch and how loud;
    /// the selected setting's input submenu over them while it is open.
    pub fn render(&self, frame: &mut FrameBuffer, _context: &TickContext) {
        frame.clear();
        let lit = TextStyle::lit(&FONT_5X8);
        let right = lit.aligned(Alignment::Right);
        let last = WIDTH_I32 - 1;
        text::draw(frame, "SUPERSAW", Point::zero(), lit);
        let hertz = write_hertz(self.frequency());
        text::draw(frame, hertz.as_str(), Point::new(last, 0), right);

        let rows = i32::try_from(PAGE_ROWS).unwrap_or(4);
        // Left of the settings, under the title.
        scroll_bar(
            frame,
            0,
            ROW_HEIGHT,
            SCROLL_WIDTH,
            rows * ROW_HEIGHT,
            self.page(),
            PAGES,
        );
        let first = self.page() * PAGE_ROWS;
        let shown = &Setting::ALL[first..(first + PAGE_ROWS).min(Setting::ALL.len())];
        for (row, &setting) in (1..).zip(shown) {
            let top = row * ROW_HEIGHT;
            text::draw(frame, setting.label(), Point::new(ROW_LEFT + 1, top), lit);
            let assign = self.assign(setting);
            // A setting that follows its input has no use for the panel's
            // value: its input stands in its place.
            if !assign.is_some_and(Assign::sets) {
                let value = self.value_text(setting);
                text::draw(frame, value.as_str(), Point::new(VALUE_RIGHT, top), right);
            }
            if let Some(assign) = assign {
                let mut name = TextBuf::<8>::new();
                assign.write(&mut name);
                text::draw(frame, name.as_str(), Point::new(last - 1, top), right);
            }
            if setting == Setting::Shape {
                self.draw_shape(frame, top);
            }
            if setting == self.selected() {
                gfx::fill_rect(
                    frame,
                    ROW_LEFT,
                    top,
                    WIDTH_I32 - ROW_LEFT,
                    ROW_HEIGHT,
                    Ink::Invert,
                );
            }
        }
        self.draw_spread(frame);
        if let (Some(editor), Some(assign)) = (&self.assigning, self.assign(self.selected())) {
            self.render_submenu(frame, editor, assign);
        }
    }

    /// The selected setting's input submenu over the main screen: the
    /// setting, the value played now and where both sit in its course
    /// (journal/D-036).
    fn render_submenu(&self, frame: &mut FrameBuffer, editor: &AssignEditor, assign: &Assign) {
        let setting = self.selected();
        let mut now = TextBuf::<16>::new();
        let (panel, played) = match setting {
            Setting::Note => {
                now = write_hertz(self.frequency());
                (
                    pitch_permille(tone::frequency(self.note, 0)),
                    pitch_permille(self.frequency()),
                )
            }
            Setting::Detune | Setting::Mix | Setting::Shape => {
                let (panel, played) = match setting {
                    Setting::Detune => (self.detune, self.played_detune()),
                    Setting::Mix => (self.mix, self.played_mix()),
                    _ => (self.shape, self.played_shape()),
                };
                write_percent(&mut now, played);
                (i32::from(panel), i32::from(played))
            }
            Setting::Width => {
                let played = self.played_width();
                write_percent(&mut now, played);
                (width_permille(self.width), width_permille(played))
            }
            Setting::Level | Setting::Stereo | Setting::Calc => {
                let played = self.played_level();
                let _ = write_volts(&mut now, played, 1);
                let _ = write!(now, " V");
                (
                    self.level * 1_000 / MAX_LEVEL_MV,
                    played * 1_000 / MAX_LEVEL_MV,
                )
            }
        };
        // A level is never negative: drop its sign.
        let mut shown = TextBuf::<16>::new();
        let _ = write!(shown, "{}", now.as_str().trim_start_matches('+'));
        let param = Param {
            name: setting.label(),
            now: shown.as_str(),
            zero_permille: 0,
            panel_permille: panel,
            now_permille: played,
        };
        editor.render(frame, assign, &self.cv, &param);
    }

    /// The value of `setting` on screen, as the panel sets it: what its input
    /// makes of it shows in the submenu, the header and the voices, as in
    /// `RATCHET`.
    fn value_text(&self, setting: Setting) -> TextBuf<16> {
        let mut line = TextBuf::<16>::new();
        match setting {
            Setting::Note => {
                let _ = write!(line, "{}", note_name(self.note).as_str());
            }
            Setting::Detune => write_percent(&mut line, self.detune),
            Setting::Mix => write_percent(&mut line, self.mix),
            Setting::Shape => write_percent(&mut line, self.shape),
            Setting::Width => write_percent(&mut line, self.width),
            Setting::Level => {
                let _ = write_volts(&mut line, self.level, 1);
                let _ = write!(line, " V");
            }
            Setting::Stereo => {
                let _ = write!(line, "{}", if self.stereo { "ON" } else { "OFF" });
            }
            Setting::Calc => {
                let name = match self.arithmetic {
                    Arithmetic::Fixed => "INT",
                    Arithmetic::Float => "F32",
                };
                let _ = write!(line, "{name}");
            }
        }
        // Only the level has a sign, and a peak is never negative: drop it.
        let mut shown = TextBuf::<16>::new();
        let _ = write!(shown, "{}", line.as_str().trim_start_matches('+'));
        shown
    }

    /// Two periods of a voice's waveform as played, from the sawtooth to the
    /// pulse, in the row from `top`, without its correction, as steps. The
    /// picture is the usual one, `saw(t) - s saw(t + w)`: a rising ramp, and a
    /// pulse whose narrow part stands up, as the outputs play it at a full
    /// shape (the bank's `saw(t) - s saw(t - w)`, inverted, journal/D-037).
    fn draw_shape(&self, frame: &mut FrameBuffer, top: i32) {
        let shape = i32::from(self.played_shape());
        let width = i32::from(self.played_width());
        let full = i32::from(FULL);
        // A rising ramp at `at` thousandths of its period.
        let saw = |at: i32| at % full * 2 - full;
        let samples: [i32; 2 * SHAPE_ICON_PERIOD as usize] = core::array::from_fn(|index| {
            let column = i32::try_from(index).unwrap_or(0);
            // Each column at its middle: a pulse of 5 % still lights one.
            let at = (2 * column + 1) * full / (2 * SHAPE_ICON_PERIOD);
            saw(at) - shape * saw(at + width) / full
        });
        // Scaled on its own extremes, so every shape takes the same rows:
        // sampled, a ramp stops a step short of its peak, a square does not.
        let low = samples.iter().copied().min().unwrap_or(-full);
        let high = samples.iter().copied().max().unwrap_or(full);
        let area = Area {
            x: SHAPE_ICON_LEFT,
            y: top,
            width: 2 * SHAPE_ICON_PERIOD,
            height: ROW_HEIGHT - 1,
            range: (low, high),
        };
        // As steps: each column rises or falls from the last value to its
        // own, so a jump stands upright instead of leaning over two columns.
        let mut last = samples[0];
        let points = (area.x..).zip(samples).flat_map(|(x, value)| {
            let from = core::mem::replace(&mut last, value);
            [
                Point::new(x, area.row(from)),
                Point::new(x, area.row(value)),
            ]
        });
        gfx::polyline(frame, points, Ink::Lit);
    }

    /// One bar per voice, at its offset from the fundamental as played, as
    /// tall as its gain.
    fn draw_spread(&self, frame: &mut FrameBuffer) {
        let spread = detune_curve(self.played_detune());
        let gains = voice_gains(self.played_mix());
        let middle = WIDTH_I32 / 2;
        let base = SPREAD_TOP + SPREAD_HEIGHT;
        gfx::dotted_hline(frame, 0, base, WIDTH_I32, 2, Ink::Lit);
        for (offset, gain) in OFFSETS.iter().zip(gains) {
            // The widest offset, the lowest voice's, reaches the half width
            // at full detune.
            let reach = offset * spread * SPREAD_HALF_WIDTH / (-OFFSETS[0] * (1 << 16));
            let x = middle + i32::try_from(reach).unwrap_or(0);
            let height = i64::from(SPREAD_HEIGHT) * gain / (1 << 15);
            let height = i32::try_from(height).unwrap_or(0).max(1);
            gfx::vline(frame, x, base - height, height, Ink::Lit);
        }
    }
}

impl App for SupersawApp {
    fn id(&self) -> AppId {
        AppId::Supersaw
    }

    fn update(&mut self, input: &InputSnapshot) -> Params {
        SupersawApp::update(self, input)
    }

    fn render(&self, frame: &mut FrameBuffer, context: &TickContext) {
        SupersawApp::render(self, frame, context);
    }
}

impl Default for SupersawApp {
    fn default() -> Self {
        Self::new()
    }
}

/// `amount` moved by `detents` steps, within its course.
fn step_amount(amount: u16, detents: i32) -> u16 {
    let moved = i32::from(amount) + detents * i32::from(AMOUNT_STEP);
    u16::try_from(moved.clamp(0, i32::from(FULL))).unwrap_or(0)
}

/// `110.00 Hz`.
fn write_hertz(frequency: MilliHertz) -> TextBuf<16> {
    let mut line = TextBuf::<16>::new();
    let hertz = frequency.saturating_add(5) / 10;
    let _ = write!(line, "{}.{:02} Hz", hertz / 100, hertz % 100);
    line
}

/// Where `frequency` sits from C1 to C8, in thousandths, an octave a
/// seventh of the course.
fn pitch_permille(frequency: MilliHertz) -> i32 {
    let lowest = i64::from(tone::frequency(LOWEST_NOTE, 0));
    let octaves = fixed::log2(i64::from(frequency) * i64::from(ONE) / lowest).unwrap_or(0);
    let span = i64::from(HIGHEST_NOTE - LOWEST_NOTE) / 12;
    i32::try_from(octaves * 1_000 / (span * i64::from(ONE))).unwrap_or(0)
}

/// Where `width` sits on its course from 5 to 50 %, in thousandths.
fn width_permille(width: u16) -> i32 {
    let narrowest = i32::from(NARROWEST_WIDTH);
    (i32::from(width) - narrowest) * 1_000 / (i32::from(SQUARE_WIDTH) - narrowest)
}

/// An amount in thousandths as a whole percentage, `50%`.
fn write_percent<const N: usize>(line: &mut TextBuf<N>, amount: u16) {
    let _ = write!(line, "{}%", (amount + 5) / 10);
}

#[cfg(test)]
mod tests {
    use super::{
        AMOUNT_STEP, DEFAULT_AMOUNT, DEFAULT_LEVEL_MV, DEFAULT_NOTE, HIGHEST_NOTE, LOWEST_NOTE,
        MAX_LEVEL_MV, SET_NOTE, SHAPE_ICON_LEFT, SHAPE_ICON_PERIOD, Setting, SupersawApp,
        VALUE_RIGHT, amount_from, frequency_from, level_from, pitch_permille, sweep, width_from,
    };
    use crate::apps::TickContext;
    use crate::apps::{InputSnapshot, tone};
    use crate::audio::Channel;
    use crate::audio::supersaw::{Arithmetic, FULL, NARROWEST_WIDTH, Part, SQUARE_WIDTH};
    use crate::framebuffer::{FrameBuffer, ROW_HEIGHT, WIDTH_I32};
    use crate::modulation::Effect;

    fn turn(left: i8, right: i8) -> InputSnapshot {
        let mut input = InputSnapshot::default();
        input.controls.encoder_delta = [left, right];
        input
    }

    #[test]
    fn it_starts_mono_on_a2() {
        let mut app = SupersawApp::new();
        let params = app.update(&InputSnapshot::default());
        assert_eq!(app.note(), DEFAULT_NOTE);
        assert_eq!(params.supersaw.frequency, 110_000);
        assert_eq!(
            params.channels,
            [
                Channel::Supersaw(Part::Mono),
                Channel::Supersaw(Part::Mono),
                Channel::Supersaw(Part::Sub),
                Channel::Level(0),
            ]
        );
    }

    #[test]
    fn the_left_encoder_picks_and_the_right_one_changes() {
        let mut app = SupersawApp::new();
        app.update(&turn(1, 3));
        assert_eq!(app.selected(), Setting::Detune);
        assert_eq!(app.detune(), DEFAULT_AMOUNT + 3 * AMOUNT_STEP);
        app.update(&turn(5, 1));
        assert_eq!(app.selected(), Setting::Stereo);
        let params = app.update(&InputSnapshot::default());
        assert!(app.stereo());
        assert_eq!(params.channels[0], Channel::Supersaw(Part::Left));
        assert_eq!(params.channels[1], Channel::Supersaw(Part::Right));
        app.update(&turn(-10, 0));
        assert_eq!(app.selected(), Setting::Note);
    }

    #[test]
    fn the_waveform_is_on_the_first_page_level_stereo_and_calc_on_the_second() {
        let mut app = SupersawApp::new();
        assert_eq!(app.page(), 0);
        let expected = [
            (Setting::Detune, 0),
            (Setting::Mix, 0),
            (Setting::Shape, 0),
            (Setting::Width, 0),
            (Setting::Level, 1),
            (Setting::Stereo, 1),
            (Setting::Calc, 1),
        ];
        for (setting, page) in expected {
            app.update(&turn(1, 0));
            assert_eq!((app.selected(), app.page()), (setting, page));
        }
    }

    #[test]
    fn width_starts_square_and_stays_within_its_course() {
        let mut app = SupersawApp::new();
        assert_eq!(app.params().supersaw.width, SQUARE_WIDTH);
        app.update(&turn(4, -20));
        assert_eq!(app.selected(), Setting::Width);
        assert_eq!(app.width(), 300, "a percent a detent");
        app.update(&turn(0, -100));
        assert_eq!(app.width(), NARROWEST_WIDTH);
        app.update(&turn(0, 100));
        assert_eq!(app.width(), SQUARE_WIDTH);
        assert!(app.assign(Setting::Width).is_some());
        assert_eq!(sweep(Setting::Width), Some((0, 5_000)));
    }

    #[test]
    fn the_width_follows_adds_or_multiplies_over_its_course() {
        assert_eq!(width_from(SQUARE_WIDTH, 0, Effect::Set), NARROWEST_WIDTH);
        assert_eq!(width_from(SQUARE_WIDTH, 5_000, Effect::Set), SQUARE_WIDTH);
        assert_eq!(width_from(100, 1_000, Effect::Add), 190, "9 % a volt");
        assert_eq!(width_from(400, 5_000, Effect::Add), SQUARE_WIDTH);
        assert_eq!(width_from(SQUARE_WIDTH, 2_500, Effect::Mul), 275);
        assert_eq!(
            width_from(SQUARE_WIDTH, -1_000, Effect::Mul),
            NARROWEST_WIDTH
        );
    }

    #[test]
    fn the_narrowest_pulse_still_shows() {
        // A pulse of 5 % is half a column of the picture: it lights one.
        let mut app = SupersawApp::new();
        app.shape = FULL;
        app.width = NARROWEST_WIDTH;
        let mut frame = FrameBuffer::new();
        app.render(&mut frame, &TickContext::default());
        let top = 4 * ROW_HEIGHT;
        let columns = SHAPE_ICON_LEFT..SHAPE_ICON_LEFT + 2 * SHAPE_ICON_PERIOD;
        // On the top row, only the pulses: one or two columns each.
        let high = columns.filter(|&x| frame.pixel(x, top)).count();
        assert!((2..=4).contains(&high), "{high} columns at the top");
    }

    #[test]
    fn shape_starts_as_a_sawtooth_and_takes_an_input() {
        let mut app = SupersawApp::new();
        assert_eq!(app.params().supersaw.shape, 0);
        app.update(&turn(3, 25));
        assert_eq!(app.selected(), Setting::Shape);
        assert_eq!(app.shape(), 25 * AMOUNT_STEP);
        assert_eq!(app.params().supersaw.shape, 250);
        assert_eq!(sweep(Setting::Shape), Some((0, 5_000)));
        assert!(app.assign(Setting::Shape).is_some());
        // Beyond the square, it stays a square.
        app.update(&turn(0, 100));
        assert_eq!(app.shape(), FULL);
    }

    #[test]
    fn calc_switches_the_bank_and_takes_no_input() {
        let mut app = SupersawApp::new();
        assert_eq!(app.params().supersaw.arithmetic, Arithmetic::Fixed);
        app.update(&turn(10, 1));
        assert_eq!(app.selected(), Setting::Calc);
        assert_eq!(app.params().supersaw.arithmetic, Arithmetic::Float);
        assert!(app.assign(Setting::Calc).is_none());
        assert_eq!(sweep(Setting::Calc), None);
        app.update(&turn(0, -1));
        assert_eq!(app.arithmetic(), Arithmetic::Fixed);
    }

    #[test]
    fn only_the_note_takes_an_input_at_start() {
        let mut app = SupersawApp::new();
        let input = InputSnapshot {
            cv: [1_000, 500, -500, 2_000],
            ..InputSnapshot::default()
        };
        let params = app.update(&input);
        assert_eq!(params.supersaw.frequency, 220_000, "one volt, one octave");
        assert_eq!(params.supersaw.detune, DEFAULT_AMOUNT);
        assert_eq!(params.supersaw.mix, DEFAULT_AMOUNT);
        assert_eq!(params.supersaw.amplitude, DEFAULT_LEVEL_MV);
        assert!(app.assign(Setting::Stereo).is_none());
    }

    #[test]
    fn the_amounts_and_the_level_follow_add_or_multiply() {
        assert_eq!(amount_from(500, 1_000, Effect::Add), 700, "20 % a volt");
        assert_eq!(amount_from(500, -5_000, Effect::Add), 0);
        assert_eq!(amount_from(900, 5_000, Effect::Add), FULL);
        assert_eq!(amount_from(900, 2_500, Effect::Set), 500);
        assert_eq!(amount_from(400, 2_500, Effect::Mul), 200);
        assert_eq!(level_from(0, 5_000, Effect::Set), MAX_LEVEL_MV);
        assert_eq!(level_from(1_000, 2_500, Effect::Mul), 500, "a VCA");
        assert_eq!(level_from(1_000, -1_000, Effect::Mul), 0);
        assert_eq!(level_from(1_000, 5_000, Effect::Add), MAX_LEVEL_MV);
    }

    #[test]
    fn the_note_adds_sets_from_c4_or_multiplies_the_frequency() {
        assert_eq!(frequency_from(DEFAULT_NOTE, 1_000, Effect::Add), 220_000);
        assert_eq!(
            frequency_from(DEFAULT_NOTE, 0, Effect::Set),
            tone::frequency(SET_NOTE, 0)
        );
        assert_eq!(frequency_from(DEFAULT_NOTE, 5_000, Effect::Mul), 110_000);
        assert_eq!(frequency_from(DEFAULT_NOTE, 10_000, Effect::Mul), 220_000);
        assert_eq!(
            frequency_from(DEFAULT_NOTE, 0, Effect::Mul),
            tone::FREQUENCY_RANGE.0
        );
        assert_eq!(sweep(Setting::Note), Some((-3_000, 4_000)), "C1 to C8");
        assert_eq!(sweep(Setting::Stereo), None);
    }

    #[test]
    fn the_pitch_sits_on_its_course_in_octaves() {
        assert_eq!(pitch_permille(tone::frequency(LOWEST_NOTE, 0)), 0);
        assert_eq!(pitch_permille(tone::frequency(HIGHEST_NOTE, 0)), 1_000);
        let middle = pitch_permille(tone::frequency(LOWEST_NOTE + 42, 0));
        assert!((499..=501).contains(&middle), "{middle}");
    }

    /// The screen with `SHAPE` selected at `shape`.
    fn shape_screen(shape: u16) -> FrameBuffer {
        let mut app = SupersawApp::new();
        app.update(&turn(3, 0));
        app.shape = shape;
        let mut frame = FrameBuffer::new();
        app.render(&mut frame, &TickContext::default());
        frame
    }

    #[test]
    fn shape_shows_its_waveform() {
        let (saw, square) = (shape_screen(0), shape_screen(FULL));
        let top = 4 * ROW_HEIGHT;
        let icon = |x: i32, y: i32| {
            (SHAPE_ICON_LEFT..SHAPE_ICON_LEFT + 2 * SHAPE_ICON_PERIOD).contains(&x)
                && y < top + ROW_HEIGHT - 1
        };
        let mut changed = 0;
        for y in top..top + ROW_HEIGHT {
            for x in 0..WIDTH_I32 {
                if icon(x, y) {
                    changed += usize::from(saw.pixel(x, y) != square.pixel(x, y));
                } else if x < VALUE_RIGHT - 30 {
                    // The label and the gap: nothing else moves.
                    assert_eq!(saw.pixel(x, y), square.pixel(x, y), "at {x}, {y}");
                }
            }
        }
        assert!(changed > 0, "the waveform must change with the shape");
    }

    #[test]
    fn every_shape_takes_the_same_rows() {
        let rows = |shape: u16| {
            let mut app = SupersawApp::new();
            app.shape = shape;
            let mut frame = FrameBuffer::new();
            app.render(&mut frame, &TickContext::default());
            let top = 4 * ROW_HEIGHT;
            let lit = |y: i32| {
                (SHAPE_ICON_LEFT..SHAPE_ICON_LEFT + 2 * SHAPE_ICON_PERIOD)
                    .any(|x| frame.pixel(x, y))
            };
            let used: [bool; 8] =
                core::array::from_fn(|row| lit(top + i32::try_from(row).unwrap_or(0)));
            used
        };
        let saw = rows(0);
        for shape in [1, 250, 500, 750, 999, FULL] {
            assert_eq!(rows(shape), saw, "shape {shape}");
        }
    }
}
