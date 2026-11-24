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
//! | up / down            | next / previous output mode, on release   |
//!
//! `up` and `down` act on release rather than on press because holding both
//! together is the global gesture that opens the app menu; see
//! [`crate::buttons`] for why that has to be arbitrated before an applet sees
//! anything. Every button press, either encoder's included, is counted (see
//! [`DiagnosticApp::button_press_count`]), the same way trigger edges are.
//!
//! Each CV row shows the derived presence of the input (`P` present, `.`
//! absent, [`crate::presence`]), whether it is moving (`~`), the trigger gate
//! and its edge count.

use core::fmt::Write as _;

use crate::gfx::Point;

use crate::apps::param::{ParamSpec, ParamValue, Refused, Unit};
use crate::apps::sound::Params;
use crate::apps::{App, AppId, InputSnapshot, TickContext};
use crate::buttons::ButtonEvents;
use crate::calibration::{CV_OUT_MAX_MV, CV_OUT_MIN_MV};
use crate::debounce::{EdgeCounter, TRIGGER_STABLE_SAMPLES};
use crate::fmt::{TextBuf, write_volts};
use crate::framebuffer::{FrameBuffer, ROW_HEIGHT};
use crate::platform::{
    BUTTONS, Button, CV_CHANNELS, ControlEvents, CvChannel, MilliVolts, TRIGGER_CHANNELS,
    TriggerChannel,
};
use crate::presence::{CvPresence, Presence};
use crate::signal::DEFAULT_ACTIVITY_THRESHOLD_MV;
use crate::text::fonts::FONT_5X8;
use crate::text::{self, TextStyle};

/// Offset change per encoder detent, in millivolts.
pub const OFFSET_STEP_MV: MilliVolts = 100;

/// Period of the ramp output, in microseconds.
pub const RAMP_PERIOD_MICROS: u32 = 2_000_000;

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

/// What the diagnostic applet is set by (journal/D-043): the output mode,
/// the offset in millivolts, read as volts, and the channel it drives.
#[allow(clippy::cast_possible_wrap, clippy::cast_possible_truncation)]
// Four channels.
pub const PARAM_SPECS: [ParamSpec; 3] = [
    ParamSpec::choice("MODE", &["OFFS", "RAMP", "ZERO"]),
    ParamSpec::number(
        "OFFSET",
        (CV_OUT_MIN_MV, CV_OUT_MAX_MV),
        OFFSET_STEP_MV,
        3,
        Unit::Volts,
    ),
    ParamSpec::number("CHANNEL", (1, CV_CHANNELS as i32), 1, 0, Unit::None),
];

/// What it can be read for: the rising edges counted on each trigger input.
pub const OBSERVABLES: [ParamSpec; TRIGGER_CHANNELS] = [
    ParamSpec::number("TR1", (0, i32::MAX), 1, 0, Unit::None),
    ParamSpec::number("TR2", (0, i32::MAX), 1, 0, Unit::None),
    ParamSpec::number("TR3", (0, i32::MAX), 1, 0, Unit::None),
    ParamSpec::number("TR4", (0, i32::MAX), 1, 0, Unit::None),
];

/// What it can be asked to do: zero the trigger counters, as the left
/// encoder's switch does.
pub const ACTIONS: [&str; 1] = ["RESET"];

/// The I/O diagnostic applet.
#[derive(Debug, Clone)]
pub struct DiagnosticApp {
    inputs: [CvPresence; CV_CHANNELS],
    triggers: [EdgeCounter; TRIGGER_CHANNELS],
    press_counts: [u32; BUTTONS],
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
            inputs: [CvPresence::new(); CV_CHANNELS],
            triggers: [EdgeCounter::new(TRIGGER_STABLE_SAMPLES); TRIGGER_CHANNELS],
            press_counts: [0; BUTTONS],
            selected: 0,
            offset_mv: 0,
            mode: OutputMode::Offset,
            ramp_phase_micros: 0,
            outputs: [0; CV_CHANNELS],
        }
    }

    /// Consumes one snapshot and returns the levels the outputs should take.
    pub fn update(&mut self, input: &InputSnapshot) -> [MilliVolts; CV_CHANNELS] {
        for (presence, &level) in self.inputs.iter_mut().zip(input.cv.iter()) {
            presence.update(level, input.elapsed_micros);
        }
        for (counter, &raw) in self.triggers.iter_mut().zip(input.triggers.iter()) {
            counter.update(raw);
        }

        self.apply_controls(input.controls, &input.buttons);
        self.advance_ramp(input.elapsed_micros);
        self.outputs = self.compute_outputs(&input.cv);
        self.outputs
    }

    /// Applies encoder movement and button presses.
    fn apply_controls(&mut self, controls: ControlEvents, buttons: &ButtonEvents) {
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

        for button in Button::ALL {
            if !buttons.pressed(button) {
                continue;
            }
            self.press_counts[button.index()] = self.press_counts[button.index()].saturating_add(1);
            if button == Button::RightEncoder {
                self.offset_mv = 0;
            }
        }
        // `up` and `down` are the panel's (journal/D-046), counted above
        // all the same: the left switch cycles the mode on a tap and zeroes
        // the counters on a long press.
        if buttons.tapped(Button::LeftEncoder) {
            self.mode = self.mode.next();
        }
        if buttons.long_pressed(Button::LeftEncoder) {
            self.reset_counts();
        }
    }

    /// Zeroes every trigger counter.
    fn reset_counts(&mut self) {
        for counter in &mut self.triggers {
            counter.reset_count();
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

    /// Number of times one button (including either encoder) has been
    /// pressed since boot.
    #[must_use]
    pub fn button_press_count(&self, button: Button) -> u32 {
        self.press_counts[button.index()]
    }

    /// Debounced level of one trigger input.
    #[must_use]
    pub fn trigger_state(&self, channel: TriggerChannel) -> bool {
        self.triggers[channel.index()].state()
    }

    /// Whether an input is moving enough to count as carrying a signal.
    #[must_use]
    pub fn is_signal_active(&self, channel: CvChannel) -> bool {
        self.inputs[channel.index()]
            .signal()
            .is_active(DEFAULT_ACTIVITY_THRESHOLD_MV)
    }

    /// Whether an input carries something, as derived from its signal
    /// (journal/D-024); shown as `P` / `.` on screen.
    #[must_use]
    pub fn presence(&self, channel: CvChannel) -> Presence {
        self.inputs[channel.index()].presence()
    }

    /// Draws the diagnostic screen.
    pub fn render(&self, frame: &mut FrameBuffer, context: &TickContext) {
        frame.clear();
        let mut line = TextBuf::<26>::new();

        let mut draw = |line: &TextBuf<26>, row: i32| {
            let top_left = Point::new(0, row * ROW_HEIGHT);
            text::draw(frame, line.as_str(), top_left, TextStyle::lit(&FONT_5X8));
        };

        let _ = write!(line, "{}{:>5}us", crate::BANNER, context.duration_micros);
        draw(&line, 0);

        for channel in CvChannel::ALL {
            let index = channel.index();
            let input = &self.inputs[index];
            let detector = input.signal();
            line.clear();
            let marker = if index == self.selected { '>' } else { ' ' };
            let _ = write!(line, "{marker}{} ", index + 1);
            let _ = write_volts(&mut line, detector.level(), 3);
            let present = if input.presence().is_present() {
                'P'
            } else {
                '.'
            };
            let active = if detector.is_active(DEFAULT_ACTIVITY_THRESHOLD_MV) {
                '~'
            } else {
                '-'
            };
            let trigger = &self.triggers[index];
            let gate = if trigger.state() { 'H' } else { 'l' };
            let _ = write!(
                line,
                " {present}{active}{gate}{:>5}",
                trigger.rising_count()
            );
            draw(&line, 1 + i32::try_from(index).unwrap_or(0));
        }

        line.clear();
        let _ = write!(line, "MODE {} OFS ", self.mode.label());
        let _ = write_volts(&mut line, self.offset_mv, 3);
        draw(&line, 5);

        line.clear();
        // What the jacks carry; starred when a probe holds an output.
        let label = if context.outputs_forced {
            "OUT*"
        } else {
            "OUT"
        };
        let _ = write!(line, "{label}");
        for level in &context.cv_out {
            let _ = write!(line, " ");
            let _ = write_volts(&mut line, *level, 1);
        }
        draw(&line, 6);

        line.clear();
        let _ = write!(line, "TICKS {}", context.tick_count);
        draw(&line, 7);
    }
}

impl App for DiagnosticApp {
    fn id(&self) -> AppId {
        AppId::Diagnostic
    }

    fn param_specs(&self) -> &'static [ParamSpec] {
        &PARAM_SPECS
    }

    fn get(&self, index: usize) -> Option<ParamValue> {
        let value = match index {
            0 => ParamValue::Choice(match self.mode {
                OutputMode::Offset => 0,
                OutputMode::Ramp => 1,
                OutputMode::Zero => 2,
            }),
            1 => ParamValue::Number(self.offset_mv),
            2 => ParamValue::Number(i32::try_from(self.selected).unwrap_or(0) + 1),
            _ => return None,
        };
        Some(value)
    }

    fn set(&mut self, index: usize, value: ParamValue) -> Result<ParamValue, Refused> {
        let spec = PARAM_SPECS.get(index).ok_or(Refused::NoSuch)?;
        match (index, spec.check(value)?) {
            (0, ParamValue::Choice(mode)) => {
                self.mode = OutputMode::ALL[usize::from(mode)];
            }
            (1, ParamValue::Number(offset)) => self.offset_mv = offset,
            (2, ParamValue::Number(channel)) => {
                self.selected = usize::try_from(channel - 1).unwrap_or(0);
            }
            _ => {}
        }
        self.get(index).ok_or(Refused::NoSuch)
    }

    fn observables(&self) -> &'static [ParamSpec] {
        &OBSERVABLES
    }

    fn observe(&self, index: usize) -> Option<ParamValue> {
        let channel = TriggerChannel::from_index(index)?;
        let count = self.trigger_count(channel);
        Some(ParamValue::Number(i32::try_from(count).unwrap_or(i32::MAX)))
    }

    fn actions(&self) -> &'static [&'static str] {
        &ACTIONS
    }

    fn act(&mut self, index: usize) -> Result<(), Refused> {
        if index == 0 {
            self.reset_counts();
            Ok(())
        } else {
            Err(Refused::NoSuch)
        }
    }

    fn update(&mut self, input: &InputSnapshot) -> Params {
        DiagnosticApp::update(self, input).into()
    }

    fn render(&self, frame: &mut FrameBuffer, context: &TickContext) {
        DiagnosticApp::render(self, frame, context);
    }
}

impl Default for DiagnosticApp {
    fn default() -> Self {
        Self::new()
    }
}
