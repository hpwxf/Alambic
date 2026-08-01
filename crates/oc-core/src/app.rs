//! The I/O diagnostic applet.
//!
//! This is the whole application logic of the first milestone. It is
//! deliberately trivial musically: its job is to make every input and every
//! output observable, so that a freshly flashed module can be validated
//! end to end from the front panel alone.
//!
//! Panel mapping:
//!
//! | Control              | Effect                                    |
//! |----------------------|-------------------------------------------|
//! | left encoder, turn   | select the channel driven by the offset   |
//! | left encoder, press  | reset the trigger counters                |
//! | right encoder, turn  | change the offset by 100 mV per detent    |
//! | right encoder, press | set the offset back to 0 V                |
//! | up / down            | next / previous output mode               |

use core::fmt::Write as _;

use embedded_graphics::Drawable as _;
use embedded_graphics::mono_font::MonoTextStyle;
use embedded_graphics::mono_font::ascii::FONT_5X8;
use embedded_graphics::pixelcolor::BinaryColor;
use embedded_graphics::prelude::Point;
use embedded_graphics::text::{Baseline, Text};

use crate::calibration::{CV_OUT_MAX_MV, CV_OUT_MIN_MV};
use crate::debounce::{Debouncer, Edge, EdgeCounter};
use crate::fmt::{TextBuf, write_volts};
use crate::framebuffer::FrameBuffer;
use crate::platform::{
    BUTTONS, Button, CV_CHANNELS, ControlEvents, CvChannel, MilliVolts, TRIGGER_CHANNELS,
    TriggerChannel,
};
use crate::signal::{DEFAULT_ACTIVITY_THRESHOLD_MV, SignalDetector};

/// Offset change per encoder detent, in millivolts.
pub const OFFSET_STEP_MV: MilliVolts = 100;

/// Period of the ramp output, in microseconds.
pub const RAMP_PERIOD_MICROS: u32 = 2_000_000;

/// Height of one text row, in pixels.
///
/// The font is exactly eight pixels tall and rows are top-aligned, so every row
/// occupies exactly one framebuffer page. That keeps the screen layout aligned
/// with the OLED controller's own addressing and makes a row easy to compare
/// against a reference rendering in tests.
pub const ROW_HEIGHT: i32 = 8;

/// Number of text rows on the screen.
pub const ROWS: i32 = 8;

/// How the CV outputs are driven.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum OutputMode {
    /// The selected channel emits the offset; the others mirror the matching
    /// CV input, which exercises the input and output paths at once.
    #[default]
    Offset,
    /// All four channels emit a slow saw across the whole output range, each
    /// shifted by a quarter period so that the four jacks are distinguishable
    /// on a scope.
    Ramp,
    /// All four channels sit at 0 V; useful to measure the output offset error.
    Zero,
}

impl OutputMode {
    /// All modes, in cycling order.
    pub const ALL: [Self; 3] = [Self::Offset, Self::Ramp, Self::Zero];

    /// Short label shown on screen.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Offset => "OFFS",
            Self::Ramp => "RAMP",
            Self::Zero => "ZERO",
        }
    }

    /// The next mode, wrapping around.
    #[must_use]
    pub const fn next(self) -> Self {
        match self {
            Self::Offset => Self::Ramp,
            Self::Ramp => Self::Zero,
            Self::Zero => Self::Offset,
        }
    }

    /// The previous mode, wrapping around.
    #[must_use]
    pub const fn previous(self) -> Self {
        match self {
            Self::Offset => Self::Zero,
            Self::Ramp => Self::Offset,
            Self::Zero => Self::Ramp,
        }
    }
}

/// Everything the applet observes during one tick.
#[derive(Debug, Clone, Copy)]
pub struct InputSnapshot {
    /// Calibrated level of each CV input.
    pub cv: [MilliVolts; CV_CHANNELS],
    /// Whether the host reports a cable on each CV input.
    pub patched: [bool; CV_CHANNELS],
    /// Raw level of each trigger input.
    pub triggers: [bool; TRIGGER_CHANNELS],
    /// Encoder and button activity.
    pub controls: ControlEvents,
    /// Microseconds since the previous tick.
    pub elapsed_micros: u32,
}

impl Default for InputSnapshot {
    fn default() -> Self {
        Self {
            cv: [0; CV_CHANNELS],
            patched: [false; CV_CHANNELS],
            triggers: [false; TRIGGER_CHANNELS],
            controls: ControlEvents::default(),
            elapsed_micros: 0,
        }
    }
}

/// Timing information the applet displays but does not compute itself.
#[derive(Debug, Clone, Copy, Default)]
pub struct TickContext {
    /// Number of ticks executed since boot.
    pub tick_count: u64,
    /// Duration of the previous tick, in microseconds.
    pub duration_micros: u32,
}

/// The I/O diagnostic applet.
#[derive(Debug, Clone)]
pub struct DiagnosticApp {
    detectors: [SignalDetector; CV_CHANNELS],
    patched: [bool; CV_CHANNELS],
    triggers: [EdgeCounter; TRIGGER_CHANNELS],
    buttons: [Debouncer; BUTTONS],
    selected: usize,
    offset_mv: MilliVolts,
    mode: OutputMode,
    ramp_phase_micros: u32,
    outputs: [MilliVolts; CV_CHANNELS],
}

impl DiagnosticApp {
    /// A freshly started applet: offset mode, channel one selected, 0 V offset.
    #[must_use]
    pub fn new() -> Self {
        Self {
            detectors: [SignalDetector::default(); CV_CHANNELS],
            patched: [false; CV_CHANNELS],
            triggers: [EdgeCounter::default(); TRIGGER_CHANNELS],
            buttons: [Debouncer::default(); BUTTONS],
            selected: 0,
            offset_mv: 0,
            mode: OutputMode::Offset,
            ramp_phase_micros: 0,
            outputs: [0; CV_CHANNELS],
        }
    }

    /// Consumes one snapshot and returns the levels the outputs should take.
    pub fn update(&mut self, input: &InputSnapshot) -> [MilliVolts; CV_CHANNELS] {
        self.patched = input.patched;

        for (detector, &level) in self.detectors.iter_mut().zip(input.cv.iter()) {
            detector.update(level);
        }
        for (counter, &raw) in self.triggers.iter_mut().zip(input.triggers.iter()) {
            counter.update(raw);
        }

        self.apply_controls(input.controls);
        self.advance_ramp(input.elapsed_micros);
        self.outputs = self.compute_outputs(&input.cv);
        self.outputs
    }

    /// Applies encoder movement and button presses.
    fn apply_controls(&mut self, controls: ControlEvents) {
        let selection = i32::from(controls.delta(0));
        if selection != 0 {
            let channels = i32::try_from(CV_CHANNELS).unwrap_or(1);
            let selected = i32::try_from(self.selected).unwrap_or(0);
            self.selected = usize::try_from((selected + selection).rem_euclid(channels))
                .unwrap_or(self.selected);
        }

        let turns = i32::from(controls.delta(1));
        if turns != 0 {
            self.offset_mv =
                (self.offset_mv + turns * OFFSET_STEP_MV).clamp(CV_OUT_MIN_MV, CV_OUT_MAX_MV);
        }

        for (index, debouncer) in self.buttons.iter_mut().enumerate() {
            let Some(button) = Button::from_index(index) else {
                continue;
            };
            if debouncer.update(controls.is_down(button)) != Some(Edge::Rising) {
                continue;
            }
            match button {
                Button::Up => self.mode = self.mode.next(),
                Button::Down => self.mode = self.mode.previous(),
                Button::LeftEncoder => {
                    for counter in &mut self.triggers {
                        counter.reset_count();
                    }
                }
                Button::RightEncoder => self.offset_mv = 0,
            }
        }
    }

    /// Advances the ramp phase, wrapping at the end of the period.
    fn advance_ramp(&mut self, elapsed_micros: u32) {
        self.ramp_phase_micros =
            (self.ramp_phase_micros.wrapping_add(elapsed_micros)) % RAMP_PERIOD_MICROS;
    }

    /// Computes the four output levels for the current mode.
    fn compute_outputs(&self, cv: &[MilliVolts; CV_CHANNELS]) -> [MilliVolts; CV_CHANNELS] {
        let mut outputs = [0; CV_CHANNELS];
        for (index, output) in outputs.iter_mut().enumerate() {
            *output = match self.mode {
                OutputMode::Zero => 0,
                OutputMode::Offset => {
                    if index == self.selected {
                        self.offset_mv
                    } else {
                        cv[index].clamp(CV_OUT_MIN_MV, CV_OUT_MAX_MV)
                    }
                }
                OutputMode::Ramp => self.ramp_level(index),
            };
        }
        outputs
    }

    /// Level of the ramp on one channel, phase-shifted by a quarter period per
    /// channel.
    fn ramp_level(&self, index: usize) -> MilliVolts {
        let shift = RAMP_PERIOD_MICROS / 4;
        let index = u32::try_from(index).unwrap_or(0);
        let phase = (self.ramp_phase_micros + index.wrapping_mul(shift)) % RAMP_PERIOD_MICROS;
        let span = i64::from(CV_OUT_MAX_MV - CV_OUT_MIN_MV);
        let offset = i64::from(phase) * span / i64::from(RAMP_PERIOD_MICROS);
        MilliVolts::try_from(i64::from(CV_OUT_MIN_MV) + offset).unwrap_or(CV_OUT_MIN_MV)
    }

    /// The levels currently driven on the outputs.
    #[must_use]
    pub const fn outputs(&self) -> &[MilliVolts; CV_CHANNELS] {
        &self.outputs
    }

    /// Current output mode.
    #[must_use]
    pub const fn mode(&self) -> OutputMode {
        self.mode
    }

    /// Channel currently driven by the offset.
    #[must_use]
    pub const fn selected(&self) -> usize {
        self.selected
    }

    /// Current offset level.
    #[must_use]
    pub const fn offset(&self) -> MilliVolts {
        self.offset_mv
    }

    /// Rising edges counted on one trigger input.
    #[must_use]
    pub fn trigger_count(&self, channel: TriggerChannel) -> u32 {
        self.triggers[channel.index()].rising_count()
    }

    /// Debounced level of one trigger input.
    #[must_use]
    pub fn trigger_state(&self, channel: TriggerChannel) -> bool {
        self.triggers[channel.index()].state()
    }

    /// Whether an input is moving enough to count as carrying a signal.
    #[must_use]
    pub fn is_signal_active(&self, channel: CvChannel) -> bool {
        self.detectors[channel.index()].is_active(DEFAULT_ACTIVITY_THRESHOLD_MV)
    }

    /// Draws the diagnostic screen.
    pub fn render(&self, frame: &mut FrameBuffer, context: &TickContext) {
        frame.clear();
        let style = MonoTextStyle::new(&FONT_5X8, BinaryColor::On);
        let mut line = TextBuf::<26>::new();

        let mut draw = |line: &TextBuf<26>, row: i32| {
            let top_left = Point::new(0, row * ROW_HEIGHT);
            let _ = Text::with_baseline(line.as_str(), top_left, style, Baseline::Top).draw(frame);
        };

        let _ = write!(line, "{} {:>5}us", crate::BANNER, context.duration_micros);
        draw(&line, 0);

        for channel in CvChannel::ALL {
            let index = channel.index();
            let detector = &self.detectors[index];
            line.clear();
            let marker = if index == self.selected { '>' } else { ' ' };
            let _ = write!(line, "{marker}{} ", index + 1);
            let _ = write_volts(&mut line, detector.level(), 3);
            let patched = if self.patched[index] { 'P' } else { '.' };
            let active = if detector.is_active(DEFAULT_ACTIVITY_THRESHOLD_MV) {
                '~'
            } else {
                '-'
            };
            let trigger = &self.triggers[index];
            let gate = if trigger.state() { 'H' } else { 'l' };
            let _ = write!(
                line,
                " {patched}{active}{gate}{:>5}",
                trigger.rising_count()
            );
            draw(&line, 1 + i32::try_from(index).unwrap_or(0));
        }

        line.clear();
        let _ = write!(line, "MODE {} OFS ", self.mode.label());
        let _ = write_volts(&mut line, self.offset_mv, 3);
        draw(&line, 5);

        line.clear();
        let _ = write!(line, "OUT");
        for level in &self.outputs {
            let _ = write!(line, " ");
            let _ = write_volts(&mut line, *level, 1);
        }
        draw(&line, 6);

        line.clear();
        let _ = write!(line, "TICKS {}", context.tick_count);
        draw(&line, 7);
    }
}

impl Default for DiagnosticApp {
    fn default() -> Self {
        Self::new()
    }
}
