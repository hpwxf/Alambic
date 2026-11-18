//! The `SUPERSAW` applet (journal/D-037): seven detuned band-limited
//! sawtooths around one pitch, the bank of [`crate::audio::supersaw`].
//!
//! A and B carry all seven voices, or the left and right halves with
//! `STEREO ON`; C a sawtooth an octave below; D rests at 0 V. The left
//! encoder picks a setting, the right one changes it. `CV1` adds to the note
//! at one volt per octave, `CV2` to the detune and `CV3` to the mix, 20 % per
//! volt.

use core::fmt::Write as _;

use crate::apps::tone::{self, HIGHEST_NOTE, LOWEST_NOTE, note_name};
use crate::apps::{InputSnapshot, TickContext};
use crate::audio::supersaw::{CENTRE, FULL, OFFSETS, Part, Supersaw, detune_curve, mix_gains};
use crate::audio::{Channel, MilliHertz, Params};
use crate::fmt::{TextBuf, write_volts};
use crate::framebuffer::{FrameBuffer, ROW_HEIGHT, WIDTH_I32};
use crate::gfx::{self, Ink, Point};
use crate::platform::MilliVolts;
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

/// Millivolts of `CV2` or `CV3` per thousandth of their setting: 20 % per
/// volt.
const MV_PER_THOUSANDTH: MilliVolts = 5;

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
}

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
    /// The last inputs, `CV1` to `CV3`.
    inputs: [MilliVolts; 3],
}

impl SupersawApp {
    /// A2, half detuned, half mixed, 1 V, mono.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            selected: 0,
            note: DEFAULT_NOTE,
            detune: DEFAULT_AMOUNT,
            mix: DEFAULT_AMOUNT,
            level: DEFAULT_LEVEL_MV,
            stereo: false,
            inputs: [0; 3],
        }
    }

    /// Consumes one snapshot and returns what the outputs carry.
    pub fn update(&mut self, input: &InputSnapshot) -> Params {
        let pick = i32::try_from(self.selected).unwrap_or(0) + i32::from(input.controls.delta(0));
        let last = i32::try_from(Setting::ALL.len() - 1).unwrap_or(0);
        self.selected = usize::try_from(pick.clamp(0, last)).unwrap_or(0);
        let turn = input.controls.delta(1);
        if turn != 0 {
            self.change(Setting::ALL[self.selected], i32::from(turn));
        }
        self.inputs = [input.cv[0], input.cv[1], input.cv[2]];
        self.params()
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
            amplitude: self.level,
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

    /// The pitch played: the note plus `CV1`.
    #[must_use]
    pub fn frequency(&self) -> MilliHertz {
        tone::frequency(self.note, self.inputs[0])
    }

    /// The detune played: the panel's plus `CV2`.
    #[must_use]
    pub fn played_detune(&self) -> u16 {
        with_input(self.detune, self.inputs[1])
    }

    /// The mix played: the panel's plus `CV3`.
    #[must_use]
    pub fn played_mix(&self) -> u16 {
        with_input(self.mix, self.inputs[2])
    }

    /// Draws the pitch, the settings, the one being changed, and the seven
    /// voices: where each sits in pitch and how loud.
    pub fn render(&self, frame: &mut FrameBuffer, _context: &TickContext) {
        frame.clear();
        let lit = TextStyle::lit(&FONT_5X8);
        let right = lit.aligned(Alignment::Right);
        let last = WIDTH_I32 - 1;
        text::draw(frame, "SUPERSAW", Point::zero(), lit);
        let mut line = TextBuf::<16>::new();
        let hertz = self.frequency().saturating_add(5) / 10;
        let _ = write!(line, "{}.{:02} Hz", hertz / 100, hertz % 100);
        text::draw(frame, line.as_str(), Point::new(last, 0), right);

        for (row, setting) in (1..).zip(Setting::ALL) {
            let top = row * ROW_HEIGHT;
            text::draw(frame, setting.label(), Point::new(1, top), lit);
            let value = self.value_text(setting);
            text::draw(frame, value.as_str(), Point::new(last - 1, top), right);
            if setting == self.selected() {
                gfx::fill_rect(frame, 0, top, WIDTH_I32, ROW_HEIGHT, Ink::Invert);
            }
        }
        self.draw_spread(frame);
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

/// The panel's `amount` raised by `millivolts`, within its course.
fn with_input(amount: u16, millivolts: MilliVolts) -> u16 {
    let moved = i32::from(amount) + millivolts / MV_PER_THOUSANDTH;
    u16::try_from(moved.clamp(0, i32::from(FULL))).unwrap_or(0)
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
    use super::{AMOUNT_STEP, DEFAULT_AMOUNT, DEFAULT_NOTE, Setting, SupersawApp, with_input};
    use crate::apps::InputSnapshot;
    use crate::audio::Channel;
    use crate::audio::supersaw::{FULL, Part};

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
    fn the_inputs_add_to_the_panel_within_the_course() {
        assert_eq!(with_input(500, 1_000), 700, "20 % a volt");
        assert_eq!(with_input(500, -5_000), 0);
        assert_eq!(with_input(900, 5_000), FULL);
        let mut app = SupersawApp::new();
        let input = InputSnapshot {
            cv: [1_000, 500, -500, 0],
            ..InputSnapshot::default()
        };
        let params = app.update(&input);
        assert_eq!(params.supersaw.frequency, 220_000, "one volt, one octave");
        assert_eq!(params.supersaw.detune, 600);
        assert_eq!(params.supersaw.mix, 400);
        assert_eq!(app.detune(), DEFAULT_AMOUNT, "the panel keeps its value");
    }
}
