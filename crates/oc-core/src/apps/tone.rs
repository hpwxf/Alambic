//! The `TONE` applet (journal/D-033): one test tone on the four outputs, a
//! different waveform on each, to hear the audio path and tune against.
//!
//! A carries a sine, B a triangle, C a sawtooth, D a square, all at the same
//! pitch. As in `RATCHET` and `SUPERSAW`, the left encoder picks a setting
//! and the right one changes it: `NOTE`, C1 to C8 (A4 = 440 Hz at start),
//! and `LEVEL`, the peak level, 0 to 3 V around 0 V (the outputs go down to
//! −3 V). The right encoder's click opens the setting's input submenu
//! (journal/D-036, [`Assign`]). Neither takes an input at start: an
//! unplugged input reads a few millivolts of noise on the module, and a test
//! tone must stay clean until one is chosen. The sawtooth and the square are
//! not band-limited and alias: that is expected of a test tone.

use core::fmt::Write as _;

use crate::apps::param::{ParamSpec, ParamValue, Refused, Unit};
use crate::apps::sound::Params;
use crate::apps::{App, AppId, InputSnapshot, TickContext};
use crate::audio::MilliHertz;
use crate::audio::basic::{BasicParams, Channel, Tone};
use crate::audio::wave::{ONE, Waveform};
use crate::fixed;
use crate::fmt::{TextBuf, write_volts};
use crate::framebuffer::{FrameBuffer, ROW_HEIGHT, WIDTH_I32};
use crate::gfx::controls::{LIST_LEFT, SCROLL_WIDTH, row_highlight, scroll_track};
use crate::gfx::{self, Ink, Point};
use crate::modulation::editor::Param;
use crate::modulation::slots::Modulations;
use crate::modulation::{Assign, AssignLine, Effect, multiplied};
use crate::platform::{Button, CV_CHANNELS, MilliVolts};
use crate::text::fonts::FONT_5X8;
use crate::text::{self, Alignment, TextStyle};

/// The lowest note, C1, as a MIDI number.
pub const LOWEST_NOTE: u8 = 24;

/// The highest note, C8.
pub const HIGHEST_NOTE: u8 = 108;

/// The note at start, A4.
pub const REFERENCE_NOTE: u8 = 69;

/// The frequency of [`REFERENCE_NOTE`].
pub const REFERENCE_FREQUENCY: MilliHertz = 440_000;

/// The highest peak level: the outputs go down to −3 V.
pub const MAX_AMPLITUDE_MV: MilliVolts = 3_000;

/// The peak level at start.
pub const DEFAULT_AMPLITUDE_MV: MilliVolts = 1_000;

/// What one detent of the right encoder changes the level by.
pub const AMPLITUDE_STEP_MV: MilliVolts = 100;

/// The pitch range, whatever an input adds: below a hertz is not a tone, and
/// above 20 kHz not audible.
pub const FREQUENCY_RANGE: (MilliHertz, MilliHertz) = (1_000, 20_000_000);

/// The note an input that sets the note gives at 0 V, C4: −3 V reach C1 and
/// +4 V C8.
pub const SET_NOTE: u8 = 60;

/// The signal that sweeps the note from C1 to C8 when it follows its input,
/// at one volt per octave, [`SET_NOTE`] at 0 V.
#[allow(clippy::cast_lossless)]
// `From` is not `const`.
pub const NOTE_SWEEP: (MilliVolts, MilliVolts) = (
    (LOWEST_NOTE as MilliVolts - SET_NOTE as MilliVolts) * 1_000 / 12,
    (HIGHEST_NOTE as MilliVolts - SET_NOTE as MilliVolts) * 1_000 / 12,
);

/// The signal that sweeps the level from 0 to 3 V when it follows its
/// input: 0 to 5 V.
pub const LEVEL_SWEEP: (MilliVolts, MilliVolts) = (0, 5_000);

/// The waveform of each output, A to D.
const WAVES: [Waveform; CV_CHANNELS] = Waveform::ALL;

/// Names of the notes of an octave, from C.
const NOTE_NAMES: [&str; 12] = [
    "C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B",
];

/// Columns of the screen given to each output's waveform.
const WAVE_COLUMN: i32 = WIDTH_I32 / 4;

/// Top row of the waveform drawings.
const WAVE_TOP: i32 = 50;

/// Half the height of a waveform drawing.
const WAVE_HALF: i32 = 6;

/// Right edge of a setting's value, left of the input's name, as in
/// `SUPERSAW`.
const VALUE_RIGHT: i32 = 98;

/// Rows under the title the scroll bar's place spans, above the waveforms'
/// labels.
const SETTING_ROWS: i32 = 4;

/// What `TONE` is set by (journal/D-043), one per [`Setting`]: the note and
/// the peak level, in millivolts, read as volts.
#[allow(clippy::cast_possible_wrap)]
// MIDI numbers.
pub const PARAM_SPECS: [ParamSpec; 2] = [
    ParamSpec::number(
        "NOTE",
        (LOWEST_NOTE as i32, HIGHEST_NOTE as i32),
        1,
        0,
        Unit::Note,
    )
    .assignable(),
    ParamSpec::number(
        "LEVEL",
        (0, MAX_AMPLITUDE_MV),
        AMPLITUDE_STEP_MV,
        3,
        Unit::Volts,
    )
    .assignable(),
];

/// What it plays once its inputs moved its settings: the frequency, in
/// millihertz, and the level.
pub const OBSERVABLES: [ParamSpec; 2] = [
    ParamSpec::number("FREQ", (0, 20_000_000), 1, 3, Unit::Hertz),
    ParamSpec::number("LEVEL NOW", (0, MAX_AMPLITUDE_MV), 1, 3, Unit::Volts),
];

/// One setting of the applet, in the order of the screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Setting {
    /// The note, C1 to C8.
    Note,
    /// The peak level.
    Level,
}

impl Setting {
    /// Every setting, top to bottom; each keeps its input at its index.
    pub const ALL: [Self; 2] = [Self::Note, Self::Level];

    /// The label on screen.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Note => "NOTE",
            Self::Level => "LEVEL",
        }
    }

    /// The range of the signal that sweeps this setting over all its values
    /// when it follows its input (journal/D-036).
    #[must_use]
    pub const fn sweep(self) -> (MilliVolts, MilliVolts) {
        match self {
            Self::Note => NOTE_SWEEP,
            Self::Level => LEVEL_SWEEP,
        }
    }

    const fn index(self) -> usize {
        self as usize
    }
}

/// One test tone, four waveforms.
#[derive(Debug, Clone)]
pub struct ToneApp {
    selected: usize,
    note: u8,
    amplitude: MilliVolts,
    /// The input of each setting, by its index in [`Setting::ALL`], and the
    /// submenu.
    inputs: Modulations<2>,
}

impl ToneApp {
    /// A4 at 1 V peak, no input.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            selected: 0,
            note: REFERENCE_NOTE,
            amplitude: DEFAULT_AMPLITUDE_MV,
            inputs: Modulations::new([Assign::NONE; 2]),
        }
    }

    /// Consumes one snapshot and returns what the outputs carry.
    pub fn update(&mut self, input: &InputSnapshot) -> Params {
        self.follow_panel(input);
        self.inputs.read(&input.cv, input.elapsed_micros);
        self.params()
    }

    /// The submenu follows the panel while it is open; otherwise the left
    /// encoder picks a setting, the right one changes it, and its click
    /// opens the setting's submenu.
    fn follow_panel(&mut self, input: &InputSnapshot) {
        if self.inputs.follow(input, Some(self.selected().sweep())) {
            return;
        }
        let pick = i32::try_from(self.selected).unwrap_or(0) + i32::from(input.controls.delta(0));
        let last = i32::try_from(Setting::ALL.len() - 1).unwrap_or(0);
        self.selected = usize::try_from(pick.clamp(0, last)).unwrap_or(0);
        if input.buttons.pressed(Button::RightEncoder) {
            self.inputs.open(self.selected);
        }
        let turn = i32::from(input.controls.delta(1));
        match self.selected() {
            Setting::Note => {
                let note = (i32::from(self.note) + turn)
                    .clamp(i32::from(LOWEST_NOTE), i32::from(HIGHEST_NOTE));
                self.note = u8::try_from(note).unwrap_or(REFERENCE_NOTE);
            }
            Setting::Level => {
                self.amplitude =
                    (self.amplitude + turn * AMPLITUDE_STEP_MV).clamp(0, MAX_AMPLITUDE_MV);
            }
        }
    }

    /// What the outputs carry: the tone in each waveform.
    #[must_use]
    pub fn params(&self) -> Params {
        let (frequency, amplitude) = (self.frequency(), self.played_amplitude());
        let mut sound = BasicParams::REST;
        for (channel, wave) in sound.channels.iter_mut().zip(WAVES) {
            *channel = Channel::Tone(Tone {
                wave,
                frequency,
                amplitude,
            });
        }
        Params::basic(sound)
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

    /// The peak level set on the panel.
    #[must_use]
    pub const fn amplitude(&self) -> MilliVolts {
        self.amplitude
    }

    /// The input `setting` takes.
    #[must_use]
    pub const fn assign(&self, setting: Setting) -> &Assign {
        self.inputs.assign(setting.index())
    }

    /// The field picked in the selected setting's input submenu, `None`
    /// while it is closed.
    #[must_use]
    pub const fn assigning(&self) -> Option<AssignLine> {
        self.inputs.line()
    }

    /// The shaped signal of `setting`'s input and its effect, `None` without
    /// an input.
    fn reading(&self, setting: Setting) -> Option<(MilliVolts, Effect)> {
        self.inputs.reading(setting.index())
    }

    /// The pitch played: the note and its input.
    #[must_use]
    pub fn frequency(&self) -> MilliHertz {
        self.reading(Setting::Note)
            .map_or(frequency(self.note, 0), |(millivolts, effect)| {
                frequency_from(self.note, millivolts, effect)
            })
    }

    /// The peak level played: the panel's and its input.
    #[must_use]
    pub fn played_amplitude(&self) -> MilliVolts {
        self.reading(Setting::Level)
            .map_or(self.amplitude, |(millivolts, effect)| {
                level_from(self.amplitude, millivolts, effect)
            })
    }

    /// Draws the pitch played in the title row, the settings and their
    /// inputs, the one being changed, and each output's waveform; the
    /// selected setting's input submenu over them while it is open.
    pub fn render(&self, frame: &mut FrameBuffer, _context: &TickContext) {
        frame.clear();
        let lit = TextStyle::lit(&FONT_5X8);
        let right = lit.aligned(Alignment::Right);
        let last = WIDTH_I32 - 1;
        text::draw(frame, "TONE", Point::zero(), lit);
        let hertz = write_hertz(self.frequency());
        text::draw(frame, hertz.as_str(), Point::new(last, 0), right);

        // One page: the scroll bar's place, empty, lines the settings up
        // with those of the applets that scroll.
        scroll_track(
            frame,
            0,
            ROW_HEIGHT,
            SCROLL_WIDTH,
            SETTING_ROWS * ROW_HEIGHT,
        );
        for (row, setting) in (1..).zip(Setting::ALL) {
            let top = row * ROW_HEIGHT;
            text::draw(frame, setting.label(), Point::new(LIST_LEFT + 1, top), lit);
            let assign = self.assign(setting);
            // A setting that follows its input has no use for the panel's
            // value: its input stands in its place.
            if !assign.sets() {
                let value = self.value_text(setting);
                text::draw(frame, value.as_str(), Point::new(VALUE_RIGHT, top), right);
            }
            let mut name = TextBuf::<8>::new();
            assign.write(&mut name);
            text::draw(frame, name.as_str(), Point::new(last - 1, top), right);
            if setting == self.selected() {
                row_highlight(frame, top, ROW_HEIGHT, Ink::Invert);
            }
        }

        for (column, (output, wave)) in (0..).zip(["A", "B", "C", "D"].into_iter().zip(WAVES)) {
            let left = column * WAVE_COLUMN;
            let mut label = TextBuf::<8>::new();
            let _ = write!(label, "{output} {}", wave.name());
            text::draw(
                frame,
                label.as_str(),
                Point::new(left + 1, 5 * ROW_HEIGHT),
                lit,
            );
            draw_wave(frame, wave, left + 2, WAVE_COLUMN - 5);
        }
        if self.inputs.is_open() {
            self.render_submenu(frame);
        }
    }

    /// The selected setting's input submenu over the main screen: the
    /// setting, the value played now and where both sit in its course
    /// (journal/D-036).
    fn render_submenu(&self, frame: &mut FrameBuffer) {
        let setting = self.selected();
        let (now, panel, played) = match setting {
            Setting::Note => (
                write_hertz(self.frequency()),
                pitch_permille(frequency(self.note, 0)),
                pitch_permille(self.frequency()),
            ),
            Setting::Level => {
                let played = self.played_amplitude();
                (
                    level_text(played),
                    self.amplitude * 1_000 / MAX_AMPLITUDE_MV,
                    played * 1_000 / MAX_AMPLITUDE_MV,
                )
            }
        };
        let param = Param {
            name: setting.label(),
            now: now.as_str(),
            zero_permille: 0,
            panel_permille: panel,
            now_permille: played,
        };
        self.inputs.render(frame, &param);
    }

    /// The value of `setting` on screen, as the panel sets it: what its input
    /// makes of it shows in the title row and the submenu, as in `SUPERSAW`.
    fn value_text(&self, setting: Setting) -> TextBuf<16> {
        match setting {
            Setting::Note => {
                let mut line = TextBuf::<16>::new();
                let _ = write!(line, "{}", note_name(self.note).as_str());
                line
            }
            Setting::Level => level_text(self.amplitude),
        }
    }
}

impl App for ToneApp {
    fn id(&self) -> AppId {
        AppId::Tone
    }

    fn param_specs(&self) -> &'static [ParamSpec] {
        &PARAM_SPECS
    }

    fn get(&self, index: usize) -> Option<ParamValue> {
        match Setting::ALL.get(index)? {
            Setting::Note => Some(ParamValue::Number(i32::from(self.note))),
            Setting::Level => Some(ParamValue::Number(self.amplitude)),
        }
    }

    fn set(&mut self, index: usize, value: ParamValue) -> Result<ParamValue, Refused> {
        let spec = PARAM_SPECS.get(index).ok_or(Refused::NoSuch)?;
        match (Setting::ALL[index], spec.check(value)?) {
            (Setting::Note, ParamValue::Number(note)) => {
                self.note = u8::try_from(note).unwrap_or(REFERENCE_NOTE);
            }
            (Setting::Level, ParamValue::Number(level)) => self.amplitude = level,
            _ => {}
        }
        self.get(index).ok_or(Refused::NoSuch)
    }

    fn observables(&self) -> &'static [ParamSpec] {
        &OBSERVABLES
    }

    fn observe(&self, index: usize) -> Option<ParamValue> {
        let number = match index {
            0 => i32::try_from(self.frequency()).unwrap_or(i32::MAX),
            1 => self.played_amplitude(),
            _ => return None,
        };
        Some(ParamValue::Number(number))
    }

    fn assigns(&self) -> &[Assign] {
        self.inputs.as_slice()
    }

    fn assigns_mut(&mut self) -> &mut [Assign] {
        self.inputs.as_mut_slice()
    }

    fn assign_sweep(&self, index: usize) -> Option<(MilliVolts, MilliVolts)> {
        Setting::ALL.get(index).map(|setting| setting.sweep())
    }

    fn update(&mut self, input: &InputSnapshot) -> Params {
        ToneApp::update(self, input)
    }

    fn render(&self, frame: &mut FrameBuffer, context: &TickContext) {
        ToneApp::render(self, frame, context);
    }
}

impl Default for ToneApp {
    fn default() -> Self {
        Self::new()
    }
}

/// One period of `wave` across `width` columns from `left`.
fn draw_wave(frame: &mut FrameBuffer, wave: Waveform, left: i32, width: i32) {
    let middle = WAVE_TOP + WAVE_HALF;
    let point = |x: i32| {
        // `x` spans the period: the phase is `x / width` of a turn.
        let phase = (u64::from(x.unsigned_abs()) << 32) / u64::from(width.unsigned_abs().max(1));
        let phase = u32::try_from(phase).unwrap_or(u32::MAX);
        let y = middle - wave.at(phase) * WAVE_HALF / ONE;
        Point::new(left + x, y)
    };
    gfx::polyline(frame, (0..width).map(point), Ink::Lit);
}

/// The frequency of MIDI `note` raised by `cv` at one volt per octave, held
/// within [`FREQUENCY_RANGE`].
#[must_use]
pub fn frequency(note: u8, cv: MilliVolts) -> MilliHertz {
    let one = i64::from(fixed::ONE);
    let semitones = i64::from(note) - i64::from(REFERENCE_NOTE);
    // Octaves above A4, in 16.16; twenty either way is past the range.
    let octaves = (semitones * one / 12 + i64::from(cv) * one / 1_000).clamp(-20 * one, 20 * one);
    // The fraction of an octave through the table, the whole octaves as a
    // shift: a low note keeps every bit of its mantissa.
    let whole = octaves.div_euclid(one);
    let scaled = i64::from(REFERENCE_FREQUENCY) * fixed::exp2(octaves.rem_euclid(one));
    let frequency = if whole >= 0 {
        (scaled << whole) / one
    } else {
        (scaled >> -whole) / one
    };
    let (low, high) = FREQUENCY_RANGE;
    u32::try_from(frequency.clamp(i64::from(low), i64::from(high))).unwrap_or(high)
}

/// The name of MIDI `note`, such as `A4` or `C#3`.
#[must_use]
pub fn note_name(note: u8) -> TextBuf<4> {
    let mut name = TextBuf::<4>::new();
    let octave = i32::from(note / 12) - 1;
    let _ = write!(name, "{}{octave}", NOTE_NAMES[usize::from(note % 12)]);
    name
}

/// The pitch an input makes of `note`: added at one volt per octave, set at
/// one volt per octave from [`SET_NOTE`], or the frequency multiplied (5 V
/// leaving it, 10 V an octave up).
#[must_use]
pub fn frequency_from(note: u8, millivolts: MilliVolts, effect: Effect) -> MilliHertz {
    match effect {
        Effect::Add => frequency(note, millivolts),
        Effect::Set => frequency(SET_NOTE, millivolts),
        Effect::Mul => {
            let product = multiplied(i64::from(frequency(note, 0)), millivolts);
            let (low, high) = FREQUENCY_RANGE;
            MilliHertz::try_from(product.clamp(i64::from(low), i64::from(high))).unwrap_or(low)
        }
    }
}

/// The peak level an input makes of `level`: 0 to 3 V over
/// [`LEVEL_SWEEP`] when it is followed, added at that rate, or multiplied,
/// a VCA under an envelope.
#[must_use]
pub fn level_from(level: MilliVolts, millivolts: MilliVolts, effect: Effect) -> MilliVolts {
    let full = i64::from(MAX_AMPLITUDE_MV);
    let followed = i64::from(millivolts) * full / i64::from(LEVEL_SWEEP.1);
    let moved = match effect {
        Effect::Set => followed,
        Effect::Add => i64::from(level) + followed,
        Effect::Mul => multiplied(i64::from(level), millivolts),
    };
    MilliVolts::try_from(moved.clamp(0, full)).unwrap_or(0)
}

/// `110.00 Hz`.
pub(crate) fn write_hertz(frequency: MilliHertz) -> TextBuf<16> {
    let mut line = TextBuf::<16>::new();
    let hertz = frequency.saturating_add(5) / 10;
    let _ = write!(line, "{}.{:02} Hz", hertz / 100, hertz % 100);
    line
}

/// Where `frequency` sits from C1 to C8, in thousandths, an octave a
/// seventh of the course.
pub(crate) fn pitch_permille(frequency: MilliHertz) -> i32 {
    let lowest = i64::from(self::frequency(LOWEST_NOTE, 0));
    let octaves = fixed::log2(i64::from(frequency) * i64::from(fixed::ONE) / lowest).unwrap_or(0);
    let span = i64::from(HIGHEST_NOTE - LOWEST_NOTE) / 12;
    i32::try_from(octaves * 1_000 / (span * i64::from(fixed::ONE))).unwrap_or(0)
}

/// A peak level as volts with one decimal, `1.5 V`: never negative, so
/// without a sign.
fn level_text(level: MilliVolts) -> TextBuf<16> {
    let mut line = TextBuf::<16>::new();
    let _ = write_volts(&mut line, level, 1);
    let _ = write!(line, " V");
    let mut shown = TextBuf::<16>::new();
    let _ = write!(shown, "{}", line.as_str().trim_start_matches('+'));
    shown
}

#[cfg(test)]
mod tests {
    use super::{
        AMPLITUDE_STEP_MV, DEFAULT_AMPLITUDE_MV, HIGHEST_NOTE, LIST_LEFT, LOWEST_NOTE,
        MAX_AMPLITUDE_MV, NOTE_SWEEP, SET_NOTE, Setting, ToneApp, frequency, frequency_from,
        level_from, note_name,
    };
    use crate::apps::{InputSnapshot, TickContext};
    use crate::audio::basic::{Channel, Tone};
    use crate::audio::wave::Waveform;
    use crate::framebuffer::{FrameBuffer, ROW_HEIGHT};
    use crate::modulation::Effect;

    fn turn(left: i8, right: i8) -> InputSnapshot {
        let mut input = InputSnapshot::default();
        input.controls.encoder_delta = [left, right];
        input
    }

    #[test]
    fn a4_is_440_hz_and_octaves_double() {
        assert_eq!(frequency(69, 0), 440_000);
        assert_eq!(frequency(81, 0), 880_000);
        assert_eq!(frequency(57, 0), 220_000);
        assert_eq!(frequency(69, 1_000), 880_000, "one volt, one octave");
        assert_eq!(frequency(69, -2_000), 110_000);
    }

    #[test]
    fn notes_are_named_with_their_octave() {
        assert_eq!(note_name(69).as_str(), "A4");
        assert_eq!(note_name(LOWEST_NOTE).as_str(), "C1");
        assert_eq!(note_name(61).as_str(), "C#4");
        assert_eq!(note_name(HIGHEST_NOTE).as_str(), "C8");
    }

    #[test]
    fn each_output_carries_its_waveform_at_the_same_pitch() {
        let mut tone = ToneApp::new();
        let params = tone.update(&InputSnapshot::default());
        let channels = params.as_basic().unwrap().channels;
        for (channel, wave) in channels.into_iter().zip(Waveform::ALL) {
            assert_eq!(
                channel,
                Channel::Tone(Tone {
                    wave,
                    frequency: 440_000,
                    amplitude: DEFAULT_AMPLITUDE_MV,
                })
            );
        }
    }

    #[test]
    fn the_left_encoder_picks_and_the_right_one_changes_within_bounds() {
        let mut tone = ToneApp::new();
        tone.update(&turn(0, 3));
        assert_eq!((tone.selected(), tone.note()), (Setting::Note, 72));
        tone.update(&turn(1, 2));
        assert_eq!(tone.selected(), Setting::Level);
        assert_eq!(tone.note(), 72, "the note stays once LEVEL is picked");
        assert_eq!(
            tone.amplitude(),
            DEFAULT_AMPLITUDE_MV + 2 * AMPLITUDE_STEP_MV
        );
        tone.update(&turn(5, 100));
        assert_eq!(tone.selected(), Setting::Level, "no page past LEVEL");
        assert_eq!(tone.amplitude(), MAX_AMPLITUDE_MV);
        tone.update(&turn(0, -100));
        assert_eq!(tone.amplitude(), 0);
        tone.update(&turn(-5, -100));
        assert_eq!(tone.note(), LOWEST_NOTE);
        tone.update(&turn(0, 100));
        assert_eq!(tone.note(), HIGHEST_NOTE);
    }

    #[test]
    fn no_input_moves_the_tone_at_start() {
        // An unplugged input reads a few millivolts of noise on the module:
        // a test tone must not wobble with it.
        let mut tone = ToneApp::new();
        let input = InputSnapshot {
            cv: [7, -12, 500, 2_000],
            ..InputSnapshot::default()
        };
        tone.update(&input);
        assert_eq!(tone.frequency(), 440_000);
        assert_eq!(tone.played_amplitude(), DEFAULT_AMPLITUDE_MV);
        for setting in Setting::ALL {
            assert_eq!(tone.assign(setting).input, None);
        }
    }

    #[test]
    fn the_inputs_follow_add_or_multiply() {
        assert_eq!(frequency_from(69, 1_000, Effect::Add), 880_000);
        assert_eq!(frequency_from(69, 0, Effect::Set), frequency(SET_NOTE, 0));
        assert_eq!(frequency_from(69, 10_000, Effect::Mul), 880_000);
        assert_eq!(NOTE_SWEEP, (-3_000, 4_000), "C1 to C8");
        assert_eq!(level_from(0, 5_000, Effect::Set), MAX_AMPLITUDE_MV);
        assert_eq!(level_from(1_000, 1_000, Effect::Add), 1_600);
        assert_eq!(level_from(1_000, -1_000, Effect::Mul), 0);
    }

    #[test]
    fn the_selected_row_is_highlighted() {
        let lit_in_row = |tone: &ToneApp, row: i32| {
            let mut frame = FrameBuffer::new();
            tone.render(&mut frame, &TickContext::default());
            (LIST_LEFT..LIST_LEFT + 4)
                .filter(|&x| frame.pixel(x, row * ROW_HEIGHT))
                .count()
        };
        let mut tone = ToneApp::new();
        assert_eq!(lit_in_row(&tone, 1), 4, "NOTE in inverse video");
        assert_eq!(lit_in_row(&tone, 2), 0);
        tone.update(&turn(1, 0));
        assert_eq!(lit_in_row(&tone, 1), 0);
        assert_eq!(lit_in_row(&tone, 2), 4, "LEVEL in inverse video");
    }
}
