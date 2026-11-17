//! A demonstration of the graphics primitives (journal/D-030): five pages of
//! fonts, controls, curves, sequencer patterns and meters, animated from the
//! time the ticks report, so the simulator replays them exactly.
//!
//! Panel: the left encoder turns the pages, `up` and `down` move the focus on
//! the knobs page, the right encoder changes what the page offers (the
//! focused knob, the LFO shape, the Euclidean hits), its switch resets it.
//! Each page is also a load to measure: `cargo probe stats` shows what its
//! drawing costs on the module.
//!
//! The outputs carry the simulated signals, so a scope can follow them: A the
//! LFO, B the Euclidean gate, C the first knob, D the tuner's needle.

use core::fmt::Write as _;

use crate::gfx::Point;

use crate::apps::{InputSnapshot, TickContext};
use crate::fmt::TextBuf;
use crate::framebuffer::{FrameBuffer, WIDTH_I32};
use crate::gfx::controls::{
    bipolar_hbar, caption_value, hbar, header, knob, selector, value_box, vbar,
};
use crate::gfx::patterns::{dots, euclid_ring, keyboard, steps};
use crate::gfx::plots::{Area, histogram, min_max, needle_scale};
use crate::gfx::trig::{ONE, TURN, sin};
use crate::gfx::{Ink, Ratio, polyline};
use crate::platform::{Button, CV_CHANNELS, MilliVolts};
use crate::text::fonts::{FONT_4X6, FONT_5X8, FONT_6X10, FONT_7X13, FONT_9X15, FONT_10X20};
use crate::text::{self, Alignment, TextStyle};

/// The pages, in the order the left encoder turns them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Page {
    /// One line in each font.
    Fonts,
    /// Knobs, a selector, a value box, a caption and a bar.
    Knobs,
    /// An LFO, an envelope and a sample.
    Curves,
    /// A Euclidean ring, steps, dots and a keyboard.
    Sequencer,
    /// A tuner, a spectrum and bars.
    Meters,
}

impl Page {
    /// Every page, in order.
    pub const ALL: [Self; 5] = [
        Self::Fonts,
        Self::Knobs,
        Self::Curves,
        Self::Sequencer,
        Self::Meters,
    ];

    /// The title-bar name.
    #[must_use]
    pub const fn title(self) -> &'static str {
        match self {
            Self::Fonts => "GFX/FONTS",
            Self::Knobs => "GFX/KNOBS",
            Self::Curves => "GFX/CURVES",
            Self::Sequencer => "GFX/SEQUENCER",
            Self::Meters => "GFX/METERS",
        }
    }
}

/// The LFO's shapes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Shape {
    /// Sine.
    Sine,
    /// Triangle.
    Triangle,
    /// Square.
    Square,
    /// Rising saw.
    Saw,
}

impl Shape {
    const ALL: [Self; 4] = [Self::Sine, Self::Triangle, Self::Square, Self::Saw];

    const fn name(self) -> &'static str {
        match self {
            Self::Sine => "SIN",
            Self::Triangle => "TRI",
            Self::Square => "SQR",
            Self::Saw => "SAW",
        }
    }

    /// The shape at `angle` (1/256 turn), within `-1000..=1000`.
    fn at(self, angle: i32) -> i32 {
        let angle = angle.rem_euclid(TURN);
        match self {
            Self::Sine => sin(angle) * 1_000 / ONE,
            Self::Triangle => {
                let rising = if angle < TURN / 2 {
                    angle
                } else {
                    TURN - angle
                };
                rising * 4_000 / TURN - 1_000
            }
            Self::Square => {
                if angle < TURN / 2 {
                    1_000
                } else {
                    -1_000
                }
            }
            Self::Saw => angle * 2_000 / TURN - 1_000,
        }
    }
}

/// Knob scale.
const KNOB_MAX: i32 = 1_000;

/// One knob detent.
const KNOB_STEP: i32 = 25;

/// The knobs' labels, which the envelope curve reads in this order.
const KNOB_LABELS: [&str; 4] = ["ATTACK", "DECAY", "SUSTAIN", "RELEASE"];

/// Knob values at start: a short attack, a medium decay and release.
const KNOB_START: [i32; 4] = [150, 400, 600, 500];

/// Period of the LFO.
const LFO_PERIOD_MICROS: u32 = 2_000_000;

/// Time per sequencer step.
const STEP_MICROS: u32 = 125_000;

/// Steps of the Euclidean ring.
const RING_STEPS: usize = 16;

/// Names of the twelve semitones.
const NOTE_NAMES: [&str; 12] = [
    "C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B",
];

/// Gate level on output B.
const GATE_MV: MilliVolts = 5_000;

/// Scale of output A (the LFO) and D (the needle).
const LFO_MV: MilliVolts = 3_000;

/// The demonstration applet.
#[derive(Debug, Clone)]
pub struct GfxDemoApp {
    page: usize,
    focus: usize,
    knobs: [i32; 4],
    shape: usize,
    hits: usize,
    clock_micros: u32,
    outputs: [MilliVolts; CV_CHANNELS],
}

impl GfxDemoApp {
    /// The fonts page, knobs at their starting values.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            page: 0,
            focus: 0,
            knobs: KNOB_START,
            shape: 0,
            hits: 5,
            clock_micros: 0,
            outputs: [0; CV_CHANNELS],
        }
    }

    /// The page on screen.
    #[must_use]
    pub const fn page(&self) -> Page {
        Page::ALL[self.page]
    }

    /// The knobs' values, `0..=1000`.
    #[must_use]
    pub const fn knobs(&self) -> [i32; 4] {
        self.knobs
    }

    /// Advances the animations, follows the panel, and sets the outputs.
    pub fn update(&mut self, input: &InputSnapshot) -> [MilliVolts; CV_CHANNELS] {
        self.clock_micros = self.clock_micros.wrapping_add(input.elapsed_micros);
        let pages = i32::try_from(Page::ALL.len()).unwrap_or(1);
        let page = i32::try_from(self.page).unwrap_or(0) + i32::from(input.controls.delta(0));
        self.page = usize::try_from(page.rem_euclid(pages)).unwrap_or(0);

        let turns = i32::from(input.controls.delta(1));
        match self.page() {
            Page::Knobs | Page::Fonts => {
                if input.buttons.pressed(Button::Down) {
                    self.focus = (self.focus + 1) % self.knobs.len();
                }
                if input.buttons.pressed(Button::Up) {
                    self.focus = (self.focus + self.knobs.len() - 1) % self.knobs.len();
                }
                let knob = &mut self.knobs[self.focus];
                *knob = (*knob + turns * KNOB_STEP).clamp(0, KNOB_MAX);
                if input.buttons.pressed(Button::RightEncoder) {
                    *knob = KNOB_START[self.focus];
                }
            }
            Page::Curves => {
                let shapes = i32::try_from(Shape::ALL.len()).unwrap_or(1);
                let shape = i32::try_from(self.shape).unwrap_or(0) + turns;
                self.shape = usize::try_from(shape.rem_euclid(shapes)).unwrap_or(0);
            }
            Page::Sequencer | Page::Meters => {
                let hits = i32::try_from(self.hits).unwrap_or(0) + turns;
                self.hits = usize::try_from(hits.clamp(0, 16)).unwrap_or(0);
            }
        }

        self.outputs = [
            self.lfo(0) * LFO_MV / 1_000,
            if self.pattern()[self.step()] {
                GATE_MV
            } else {
                0
            },
            self.knobs[0] * 5,
            self.needle() * LFO_MV / 1_000,
        ];
        self.outputs
    }

    /// The LFO at `offset` (1/256 turn) from its current phase, `-1000..=1000`.
    fn lfo(&self, offset: i32) -> i32 {
        let phase = (self.clock_micros % LFO_PERIOD_MICROS) / (LFO_PERIOD_MICROS / 256);
        let phase = i32::try_from(phase).unwrap_or(0);
        Shape::ALL[self.shape].at(phase + offset)
    }

    /// The current sequencer step.
    fn step(&self) -> usize {
        usize::try_from(self.clock_micros / STEP_MICROS).unwrap_or(0) % RING_STEPS
    }

    /// `hits` spread as evenly as possible over the ring (Bresenham's way to
    /// a Euclidean rhythm, first step a hit).
    fn pattern(&self) -> [bool; RING_STEPS] {
        let mut pattern = [false; RING_STEPS];
        for (index, step) in pattern.iter_mut().enumerate() {
            *step = (index * self.hits) % RING_STEPS < self.hits;
        }
        pattern
    }

    /// The tuner's needle, wobbling around the middle, `-1000..=1000`.
    fn needle(&self) -> i32 {
        let angle = i32::try_from((self.clock_micros / 7_000) % 256).unwrap_or(0);
        sin(angle) * 450 / ONE + sin(angle * 3) * 150 / ONE
    }

    /// Draws the page on screen.
    pub fn render(&self, frame: &mut FrameBuffer, _context: &TickContext) {
        frame.clear();
        let small = TextStyle::lit(&FONT_5X8);
        let top = header(
            frame,
            self.page().title(),
            self.page,
            Page::ALL.len(),
            small,
        );
        match self.page() {
            Page::Fonts => Self::render_fonts(frame, top),
            Page::Knobs => self.render_knobs(frame, top),
            Page::Curves => self.render_curves(frame, top),
            Page::Sequencer => self.render_sequencer(frame, top),
            Page::Meters => self.render_meters(frame, top),
        }
    }

    fn render_fonts(frame: &mut FrameBuffer, top: i32) {
        let lines = [
            (&FONT_4X6, "4X6  THE QUICK FOX 0123"),
            (&FONT_5X8, "5X8  QUICK FOX 0123"),
            (&FONT_6X10, "6X10 FOX 012"),
            (&FONT_7X13, "7X13 FOX"),
            (&FONT_9X15, "9X15"),
        ];
        let mut y = top;
        for (font, line) in lines {
            text::draw(frame, line, Point::new(0, y), TextStyle::lit(font));
            y += font.height() + 1;
        }
        let big = TextStyle::lit(&FONT_10X20).aligned(Alignment::Right);
        text::draw(frame, "A4", Point::new(WIDTH_I32 - 1, 43), big);
    }

    fn render_knobs(&self, frame: &mut FrameBuffer, top: i32) {
        let small = TextStyle::lit(&FONT_5X8);
        for (index, (&value, label)) in self.knobs.iter().zip(KNOB_LABELS).enumerate() {
            let column = if index % 2 == 0 { 6 } else { 70 };
            let row = if index < 2 { top + 7 } else { top + 20 };
            let center = Point::new(column, row);
            knob(frame, center, 5, Ratio::new(value, KNOB_MAX), Ink::Lit);
            let at = Point::new(column + 8, row - 4);
            if index == self.focus {
                value_box(frame, Point::new(at.x - 1, at.y - 1), label, small);
            } else {
                text::draw(frame, label, at, small);
            }
        }
        let row = top + 29;
        selector(
            frame,
            Point::new(0, row + 3),
            Shape::ALL[self.shape].name(),
            small,
        );
        let mut number = TextBuf::<8>::new();
        let _ = write!(number, "{:03}", self.knobs[self.focus] / 10);
        value_box(
            frame,
            Point::new(36, row + 3),
            number.as_str(),
            TextStyle::lit(&FONT_4X6),
        );
        let mut volts = TextBuf::<8>::new();
        let _ = write!(
            volts,
            "+{}.{}",
            self.knobs[0] / 200,
            (self.knobs[0] / 20) % 10
        );
        caption_value(
            frame,
            Point::new(64, row),
            "SCALE",
            volts.as_str(),
            TextStyle::lit(&FONT_4X6),
            small,
        );
        hbar(
            frame,
            0,
            57,
            WIDTH_I32,
            7,
            Ratio::new(self.knobs[self.focus], KNOB_MAX),
        );
    }

    fn render_curves(&self, frame: &mut FrameBuffer, top: i32) {
        // The LFO, scrolling: column x shows the phase x/128 of a period ahead.
        let lfo = Area {
            x: 0,
            y: top,
            width: WIDTH_I32 - 24,
            height: 22,
            range: (-1_000, 1_000),
        };
        polyline(
            frame,
            (0..lfo.width).map(|x| Point::new(x, lfo.row(self.lfo(x * 2)))),
            Ink::Lit,
        );
        selector(
            frame,
            Point::new(WIDTH_I32 - 22, top + 7),
            Shape::ALL[self.shape].name(),
            TextStyle::lit(&FONT_4X6),
        );

        // The envelope the knobs describe: attack, decay to the sustain
        // level, a hold, release.
        let envelope = Area {
            x: 0,
            y: top + 26,
            width: 62,
            height: 27,
            range: (0, KNOB_MAX),
        };
        let [attack, decay, _, release] = self.knobs.map(|value| 2 + value * 14 / KNOB_MAX);
        let bottom = envelope.row(0);
        let peak = envelope.row(KNOB_MAX);
        let level = envelope.row(self.knobs[2]);
        let points = [
            Point::new(0, bottom),
            Point::new(attack, peak),
            Point::new(attack + decay, level),
            Point::new(attack + decay + 14, level),
            Point::new(attack + decay + 14 + release, bottom),
        ];
        polyline(frame, points, Ink::Lit);

        // A decaying noise burst, as a sample editor shows a hit.
        let sample = Area {
            x: 66,
            y: top + 26,
            width: 62,
            height: 27,
            range: (-1_000, 1_000),
        };
        let mut columns = [(0, 0); 62];
        let mut seed: u32 = 0x1234_5678;
        for (index, column) in (0_i32..).zip(columns.iter_mut()) {
            seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            let amplitude = 1_000 * (62 - index) * (62 - index) / (62 * 62);
            let spread = i32::try_from(seed >> 22).unwrap_or(0) % 1_000;
            let low = -amplitude * spread / 1_000;
            *column = (low, amplitude * (1_000 - spread / 2) / 1_000);
        }
        min_max(frame, sample, &columns);
    }

    fn render_sequencer(&self, frame: &mut FrameBuffer, top: i32) {
        let pattern = self.pattern();
        let step = self.step();
        euclid_ring(frame, Point::new(21, top + 26), 16, 2, &pattern, Some(step));
        let mut label = TextBuf::<12>::new();
        let _ = write!(label, "E({},{RING_STEPS})", self.hits);
        text::draw(
            frame,
            label.as_str(),
            Point::new(46, top),
            TextStyle::lit(&FONT_5X8),
        );

        // A shift register's last eight bits, as a Turing machine shows them.
        let mut register: u32 = 0xB5_u32.rotate_left(u32::try_from(step).unwrap_or(0));
        let mut bits = [false; 8];
        for bit in &mut bits {
            *bit = register & 1 == 1;
            register >>= 1;
        }
        steps(frame, Point::new(48, top + 12), 6, 2, &bits, Some(step % 8));
        dots(frame, Point::new(46, top + 23), 2, 2, &pattern[..8]);

        let note = u8::try_from(step % 12).unwrap_or(0);
        keyboard(frame, Point::new(46, top + 31), 12, Some(note));
        let name = NOTE_NAMES[usize::from(note)];
        text::draw(
            frame,
            name,
            Point::new(80, top + 32),
            TextStyle::lit(&FONT_6X10),
        );
    }

    fn render_meters(&self, frame: &mut FrameBuffer, top: i32) {
        let needle = self.needle();
        text::draw(frame, "A4", Point::new(0, top), TextStyle::lit(&FONT_10X20));
        let mut cents = TextBuf::<16>::new();
        let sign = if needle < 0 { '-' } else { '+' };
        let _ = write!(
            cents,
            "{sign}{}.{} CENT",
            needle.abs() / 20,
            (needle.abs() / 2) % 10
        );
        text::draw(
            frame,
            cents.as_str(),
            Point::new(26, top + 2),
            TextStyle::lit(&FONT_5X8),
        );
        text::draw(
            frame,
            "440.0 HZ",
            Point::new(26, top + 11),
            TextStyle::lit(&FONT_4X6),
        );
        needle_scale(
            frame,
            0,
            top + 21,
            WIDTH_I32,
            10,
            11,
            Ratio::new(needle + 1_000, 2_000),
        );

        let spectrum = Area {
            x: 0,
            y: 48,
            width: 80,
            height: 16,
            range: (0, 1_000),
        };
        let mut bands = [0; 16];
        for (index, band) in (0_i32..).zip(bands.iter_mut()) {
            let wobble = sin(i32::try_from(self.clock_micros / 9_000).unwrap_or(0) + index * 23);
            *band = (900 - index * 50 + wobble * 200 / ONE).clamp(0, 1_000);
        }
        histogram(frame, spectrum, &bands, 4, 1);
        vbar(frame, 84, 48, 6, 16, Ratio::new(self.lfo(0) + 1_000, 2_000));
        bipolar_hbar(frame, 94, 52, 34, 7, Ratio::new(needle, 1_000));
    }
}

impl Default for GfxDemoApp {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::{GfxDemoApp, KNOB_START, Page, Shape};
    use crate::apps::{InputSnapshot, TickContext};
    use crate::buttons::ButtonReader;
    use crate::framebuffer::FrameBuffer;
    use crate::platform::{BUTTONS, Button, ControlEvents};

    fn turn(left: i8, right: i8) -> InputSnapshot {
        InputSnapshot {
            controls: ControlEvents {
                encoder_delta: [left, right],
                button_down: [false; BUTTONS],
            },
            elapsed_micros: 1_000,
            ..InputSnapshot::default()
        }
    }

    #[test]
    fn the_left_encoder_turns_the_pages_round() {
        let mut app = GfxDemoApp::new();
        assert_eq!(app.page(), Page::Fonts);
        app.update(&turn(1, 0));
        assert_eq!(app.page(), Page::Knobs);
        app.update(&turn(-2, 0));
        assert_eq!(app.page(), Page::Meters, "wraps backwards");
    }

    #[test]
    fn the_right_encoder_turns_the_focused_knob_and_its_switch_resets_it() {
        let mut app = GfxDemoApp::new();
        app.update(&turn(1, 4));
        assert_eq!(app.knobs()[0], KNOB_START[0] + 100);

        let mut reader = ButtonReader::new();
        let mut press = |button: Button, app: &mut GfxDemoApp| {
            for down in [true, true, true, true, false, false, false, false] {
                let mut controls = ControlEvents::default();
                controls.button_down[button.index()] = down;
                let input = InputSnapshot {
                    buttons: reader.update(&controls),
                    controls,
                    ..InputSnapshot::default()
                };
                app.update(&input);
            }
        };
        press(Button::Down, &mut app);
        app.update(&turn(0, -2));
        assert_eq!(app.knobs()[1], KNOB_START[1] - 50);
        press(Button::RightEncoder, &mut app);
        assert_eq!(app.knobs()[1], KNOB_START[1]);
    }

    #[test]
    fn every_shape_spans_its_range() {
        for shape in Shape::ALL {
            let low = (0..256)
                .map(|angle| shape.at(angle))
                .min()
                .unwrap_or_default();
            let high = (0..256)
                .map(|angle| shape.at(angle))
                .max()
                .unwrap_or_default();
            assert!(low <= -980 && high >= 980, "{shape:?}: {low}..{high}");
        }
    }

    #[test]
    fn every_page_draws_something() {
        let mut app = GfxDemoApp::new();
        for _ in Page::ALL {
            let mut frame = FrameBuffer::new();
            app.render(&mut frame, &TickContext::default());
            assert!(frame.lit_pixels() > 200, "{:?}", app.page());
            app.update(&turn(1, 0));
        }
    }

    #[test]
    fn the_gate_follows_the_euclidean_pattern() {
        let mut app = GfxDemoApp::new();
        // Step zero is always a hit.
        assert_eq!(app.update(&turn(0, 0))[1], super::GATE_MV);
    }
}
