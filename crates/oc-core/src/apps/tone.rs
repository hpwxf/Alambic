//! The `TONE` applet (journal/D-033): one test tone on the four outputs, a
//! different waveform on each, to hear the audio path and tune against.
//!
//! A carries a sine, B a triangle, C a sawtooth, D a square, all at the same
//! pitch. The left encoder sets the note, C1 to C8 (A4 = 440 Hz at start);
//! the right one the peak level, 0 to 3 V around 0 V (the outputs go down to
//! −3 V); `CV1` adds to the note at one volt per octave. The sawtooth and the
//! square are not band-limited and alias: that is expected of a test tone.

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
use crate::gfx::{self, Ink, Point};
use crate::platform::{CV_CHANNELS, MilliVolts};
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

/// The pitch range, whatever `CV1` adds: below a hertz is not a tone, and
/// above 20 kHz not audible.
pub const FREQUENCY_RANGE: (MilliHertz, MilliHertz) = (1_000, 20_000_000);

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

/// What `TONE` is set by (journal/D-043): the note and the peak level, in
/// millivolts, read as volts.
#[allow(clippy::cast_possible_wrap)]
// MIDI numbers.
pub const PARAM_SPECS: [ParamSpec; 2] = [
    ParamSpec::number(
        "NOTE",
        (LOWEST_NOTE as i32, HIGHEST_NOTE as i32),
        1,
        0,
        Unit::Note,
    ),
    ParamSpec::number(
        "LEVEL",
        (0, MAX_AMPLITUDE_MV),
        AMPLITUDE_STEP_MV,
        3,
        Unit::Volts,
    ),
];

/// What it plays once `CV1` moved the note: the frequency, in millihertz.
pub const OBSERVABLES: [ParamSpec; 1] = [ParamSpec::number(
    "FREQ",
    (0, 20_000_000),
    1,
    3,
    Unit::Hertz,
)];

/// One test tone, four waveforms.
#[derive(Debug, Clone)]
pub struct ToneApp {
    note: u8,
    amplitude: MilliVolts,
    pitch_cv: MilliVolts,
    frequency: MilliHertz,
}

impl ToneApp {
    /// A4 at 1 V peak.
    #[must_use]
    pub fn new() -> Self {
        Self {
            note: REFERENCE_NOTE,
            amplitude: DEFAULT_AMPLITUDE_MV,
            pitch_cv: 0,
            frequency: frequency(REFERENCE_NOTE, 0),
        }
    }

    /// Consumes one snapshot and returns what the outputs carry.
    pub fn update(&mut self, input: &InputSnapshot) -> Params {
        let note = i32::from(self.note) + i32::from(input.controls.delta(0));
        let note = note.clamp(i32::from(LOWEST_NOTE), i32::from(HIGHEST_NOTE));
        self.note = u8::try_from(note).unwrap_or(REFERENCE_NOTE);
        self.amplitude = (self.amplitude + i32::from(input.controls.delta(1)) * AMPLITUDE_STEP_MV)
            .clamp(0, MAX_AMPLITUDE_MV);
        self.pitch_cv = input.cv[0];
        self.frequency = frequency(self.note, self.pitch_cv);
        self.params()
    }

    /// What the outputs carry: the tone in each waveform.
    #[must_use]
    pub fn params(&self) -> Params {
        let mut sound = BasicParams::REST;
        for (channel, wave) in sound.channels.iter_mut().zip(WAVES) {
            *channel = Channel::Tone(Tone {
                wave,
                frequency: self.frequency,
                amplitude: self.amplitude,
            });
        }
        Params::basic(sound)
    }

    /// The note set on the panel, as a MIDI number.
    #[must_use]
    pub const fn note(&self) -> u8 {
        self.note
    }

    /// The peak level.
    #[must_use]
    pub const fn amplitude(&self) -> MilliVolts {
        self.amplitude
    }

    /// The pitch played: the note plus `CV1`.
    #[must_use]
    pub const fn frequency(&self) -> MilliHertz {
        self.frequency
    }

    /// Draws the note, the pitch, the level, `CV1` and each output's waveform.
    pub fn render(&self, frame: &mut FrameBuffer, _context: &TickContext) {
        frame.clear();
        let lit = TextStyle::lit(&FONT_5X8);
        let right = lit.aligned(Alignment::Right);
        let last = WIDTH_I32 - 1;
        text::draw(frame, "TONE", Point::zero(), lit);

        let mut line = TextBuf::<16>::new();
        let _ = write!(line, "NOTE {}", note_name(self.note).as_str());
        text::draw(frame, line.as_str(), Point::new(0, ROW_HEIGHT), lit);
        let mut line = TextBuf::<16>::new();
        let hertz = self.frequency.saturating_add(5) / 10;
        let _ = write!(line, "{}.{:02} Hz", hertz / 100, hertz % 100);
        text::draw(frame, line.as_str(), Point::new(last, ROW_HEIGHT), right);

        text::draw(frame, "LEVEL", Point::new(0, 2 * ROW_HEIGHT), lit);
        let mut line = TextBuf::<16>::new();
        let _ = write_volts(&mut line, self.amplitude, 1);
        let _ = write!(line, " V");
        // The level is a peak, never negative: no sign.
        let shown = line.as_str().trim_start_matches('+');
        text::draw(frame, shown, Point::new(last, 2 * ROW_HEIGHT), right);

        text::draw(frame, "CV1", Point::new(0, 3 * ROW_HEIGHT), lit);
        let mut line = TextBuf::<16>::new();
        let _ = write_volts(&mut line, self.pitch_cv, 3);
        let _ = write!(line, " V");
        text::draw(
            frame,
            line.as_str(),
            Point::new(last, 3 * ROW_HEIGHT),
            right,
        );

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
        match index {
            0 => Some(ParamValue::Number(i32::from(self.note))),
            1 => Some(ParamValue::Number(self.amplitude)),
            _ => None,
        }
    }

    fn set(&mut self, index: usize, value: ParamValue) -> Result<ParamValue, Refused> {
        let spec = PARAM_SPECS.get(index).ok_or(Refused::NoSuch)?;
        match (index, spec.check(value)?) {
            (0, ParamValue::Number(note)) => {
                self.note = u8::try_from(note).unwrap_or(REFERENCE_NOTE);
                // The tone follows at once, as the panel's next tick would.
                self.frequency = frequency(self.note, self.pitch_cv);
            }
            (1, ParamValue::Number(level)) => self.amplitude = level,
            _ => {}
        }
        self.get(index).ok_or(Refused::NoSuch)
    }

    fn observables(&self) -> &'static [ParamSpec] {
        &OBSERVABLES
    }

    fn observe(&self, index: usize) -> Option<ParamValue> {
        (index == 0).then(|| ParamValue::Number(i32::try_from(self.frequency).unwrap_or(i32::MAX)))
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

#[cfg(test)]
mod tests {
    use super::{
        AMPLITUDE_STEP_MV, DEFAULT_AMPLITUDE_MV, HIGHEST_NOTE, LOWEST_NOTE, MAX_AMPLITUDE_MV,
        ToneApp, frequency, note_name,
    };
    use crate::apps::InputSnapshot;
    use crate::audio::basic::{Channel, Tone};
    use crate::audio::wave::Waveform;

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
    fn the_encoders_set_the_note_and_the_level_within_bounds() {
        let mut tone = ToneApp::new();
        tone.update(&turn(3, 2));
        assert_eq!(tone.note(), 72);
        assert_eq!(
            tone.amplitude(),
            DEFAULT_AMPLITUDE_MV + 2 * AMPLITUDE_STEP_MV
        );
        for _ in 0..20 {
            tone.update(&turn(-100, 100));
        }
        assert_eq!(tone.note(), LOWEST_NOTE);
        assert_eq!(tone.amplitude(), MAX_AMPLITUDE_MV);
        for _ in 0..20 {
            tone.update(&turn(100, -100));
        }
        assert_eq!(tone.note(), HIGHEST_NOTE);
        assert_eq!(tone.amplitude(), 0);
    }

    #[test]
    fn cv1_transposes_at_one_volt_per_octave() {
        let mut tone = ToneApp::new();
        let mut input = InputSnapshot::default();
        input.cv[0] = 500;
        tone.update(&input);
        // Half an octave above 440 Hz: 622.25 Hz.
        assert!(
            tone.frequency().abs_diff(622_254) <= 30,
            "{}",
            tone.frequency()
        );
    }
}
