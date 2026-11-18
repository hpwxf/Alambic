//! The `SUPERSAW` applet (journal/D-037): seven detuned band-limited
//! sawtooths around one pitch, the bank of [`crate::audio::supersaw`].
//!
//! A and B carry all seven voices, or the left and right halves with
//! `STEREO ON`; C a sawtooth an octave below; D rests at 0 V. The left
//! encoder picks a setting, the right one changes it.
//!
//! Every setting but `STEREO` can take an input, chosen in the submenu the
//! right encoder's click opens (journal/D-036, [`Assign`]): it sets the
//! setting, adds to the panel's value or multiplies it. `NOTE` takes `CV1`
//! at start, added at one volt per octave; the others take none, so that an
//! unplugged input, which reads a few millivolts of noise on the module,
//! moves nothing (journal/D-037, revision of 2026-11-18).

use core::fmt::Write as _;

use crate::apps::tone::{self, HIGHEST_NOTE, LOWEST_NOTE, note_name};
use crate::apps::{InputSnapshot, TickContext};
use crate::audio::supersaw::{CENTRE, FULL, OFFSETS, Part, Supersaw, detune_curve, mix_gains};
use crate::audio::{Channel, MilliHertz, Params};
use crate::fixed::{self, ONE};
use crate::fmt::{TextBuf, write_volts};
use crate::framebuffer::{FrameBuffer, ROW_HEIGHT, WIDTH_I32};
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
    /// The peak level of one sawtooth.
    Level,
    /// A and B as one mono output twice, or left and right.
    Stereo,
}

impl Setting {
    /// Every setting, top to bottom.
    pub const ALL: [Self; 5] = [
        Self::Note,
        Self::Detune,
        Self::Mix,
        Self::Level,
        Self::Stereo,
    ];

    /// The label on screen.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Note => "NOTE",
            Self::Detune => "DETUNE",
            Self::Mix => "MIX",
            Self::Level => "LEVEL",
            Self::Stereo => "STEREO",
        }
    }

    /// Where this setting's input is kept, `None` for `STEREO`, which takes
    /// none.
    const fn input_slot(self) -> Option<usize> {
        match self {
            Self::Note => Some(0),
            Self::Detune => Some(1),
            Self::Mix => Some(2),
            Self::Level => Some(3),
            Self::Stereo => None,
        }
    }
}

/// The range of the signal that sweeps `setting` over all its values when
/// it follows its input (journal/D-036), from the signal that gives its
/// least value to the one that gives its most; `None` for `STEREO`.
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
        Setting::Detune | Setting::Mix | Setting::Level => Some((0, COURSE_MV)),
        Setting::Stereo => None,
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
const DEFAULT_ASSIGNS: [Assign; 4] = [
    Assign {
        input: Some(CvChannel::One),
        ..Assign::NONE
    },
    Assign::NONE,
    Assign::NONE,
    Assign::NONE,
];

/// Right edge of a setting's value, left of the input's name.
const VALUE_RIGHT: i32 = 98;

/// Top row of the voice spread drawing.
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
    level: MilliVolts,
    stereo: bool,
    /// The input of each setting that takes one (see `Setting::input_slot`).
    assigns: [Assign; 4],
    /// The selected setting's input submenu, `None` while it is closed.
    assigning: Option<AssignEditor>,
    /// The last inputs.
    cv: [MilliVolts; CV_CHANNELS],
}

impl SupersawApp {
    /// A2, half detuned, half mixed, 1 V, mono; `CV1` on the note.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            selected: 0,
            note: DEFAULT_NOTE,
            detune: DEFAULT_AMOUNT,
            mix: DEFAULT_AMOUNT,
            level: DEFAULT_LEVEL_MV,
            stereo: false,
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
            Setting::Level => {
                self.level = (self.level + detents * LEVEL_STEP_MV).clamp(0, MAX_LEVEL_MV);
            }
            // Any detent flips it.
            Setting::Stereo => self.stereo = !self.stereo,
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
            amplitude: self.played_level(),
        };
        params
    }

    /// The setting the right encoder changes.
    #[must_use]
    pub const fn selected(&self) -> Setting {
        Setting::ALL[self.selected]
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

    /// The input `setting` takes, `None` for `STEREO`.
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

        for (row, setting) in (1..).zip(Setting::ALL) {
            let top = row * ROW_HEIGHT;
            text::draw(frame, setting.label(), Point::new(1, top), lit);
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
            if setting == self.selected() {
                gfx::fill_rect(frame, 0, top, WIDTH_I32, ROW_HEIGHT, Ink::Invert);
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
            Setting::Detune | Setting::Mix => {
                let (panel, played) = if setting == Setting::Detune {
                    (self.detune, self.played_detune())
                } else {
                    (self.mix, self.played_mix())
                };
                let _ = write!(now, "{}%", (played + 5) / 10);
                (i32::from(panel), i32::from(played))
            }
            Setting::Level | Setting::Stereo => {
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

    /// The value of `setting` on screen; the detune and the mix show what
    /// their input makes of them when it moves them.
    fn value_text(&self, setting: Setting) -> TextBuf<16> {
        let mut line = TextBuf::<16>::new();
        match setting {
            Setting::Note => {
                let _ = write!(line, "{}", note_name(self.note).as_str());
            }
            Setting::Detune => write_amount(&mut line, self.detune, self.played_detune()),
            Setting::Mix => write_amount(&mut line, self.mix, self.played_mix()),
            Setting::Level => {
                let _ = write_volts(&mut line, self.level, 1);
                let _ = write!(line, " V");
            }
            Setting::Stereo => {
                let _ = write!(line, "{}", if self.stereo { "ON" } else { "OFF" });
            }
        }
        // Only the level has a sign, and a peak is never negative: drop it.
        let mut shown = TextBuf::<16>::new();
        let _ = write!(shown, "{}", line.as_str().trim_start_matches('+'));
        shown
    }

    /// One bar per voice, at its offset from the fundamental as played, as
    /// tall as its gain.
    fn draw_spread(&self, frame: &mut FrameBuffer) {
        let spread = detune_curve(self.played_detune());
        let (centre, side) = mix_gains(self.played_mix());
        let middle = WIDTH_I32 / 2;
        let base = SPREAD_TOP + SPREAD_HEIGHT;
        gfx::dotted_hline(frame, 0, base, WIDTH_I32, 2, Ink::Lit);
        for (index, offset) in OFFSETS.iter().enumerate() {
            // The widest offset, the lowest voice's, reaches the half width
            // at full detune.
            let reach = offset * spread * SPREAD_HALF_WIDTH / (-OFFSETS[0] * (1 << 16));
            let x = middle + i32::try_from(reach).unwrap_or(0);
            let gain = if index == CENTRE { centre } else { side };
            let height = i64::from(SPREAD_HEIGHT) * gain / (1 << 15);
            let height = i32::try_from(height).unwrap_or(0).max(1);
            gfx::vline(frame, x, base - height, height, Ink::Lit);
        }
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

/// `50%`, or `50>62%` when the input moves it.
fn write_amount(line: &mut TextBuf<16>, panel: u16, played: u16) {
    let percent = |amount: u16| (amount + 5) / 10;
    if percent(panel) == percent(played) {
        let _ = write!(line, "{}%", percent(panel));
    } else {
        let _ = write!(line, "{}>{}%", percent(panel), percent(played));
    }
}

#[cfg(test)]
mod tests {
    use super::{
        AMOUNT_STEP, DEFAULT_AMOUNT, DEFAULT_LEVEL_MV, DEFAULT_NOTE, HIGHEST_NOTE, LOWEST_NOTE,
        MAX_LEVEL_MV, SET_NOTE, Setting, SupersawApp, amount_from, frequency_from, level_from,
        pitch_permille, sweep,
    };
    use crate::apps::{InputSnapshot, tone};
    use crate::audio::Channel;
    use crate::audio::supersaw::{FULL, Part};
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
        app.update(&turn(10, 1));
        assert_eq!(app.selected(), Setting::Stereo);
        let params = app.update(&InputSnapshot::default());
        assert!(app.stereo());
        assert_eq!(params.channels[0], Channel::Supersaw(Part::Left));
        assert_eq!(params.channels[1], Channel::Supersaw(Part::Right));
        app.update(&turn(-10, 0));
        assert_eq!(app.selected(), Setting::Note);
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
}
