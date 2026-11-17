//! The calibration wizard (journal/D-026, D-027).
//!
//! The steps are the reference firmware's (Phazerville, `apps/SETTINGS.h`,
//! `OC_calibration.cpp`), so that the procedure is familiar to its users:
//!
//! 1. **Start fresh?** — `YES` starts from the factory calibration, `NO` from
//!    the one in force;
//! 2. **DAC A to D**, a low point (−3 V) and a high point (+5 V) per output:
//!    every output is driven to that level, a multimeter reads the output
//!    being calibrated, the right encoder moves its DAC code until the meter
//!    shows the target. Leaving the high point of an output whose points were
//!    adjusted interpolates its whole table through the two; an output left
//!    alone keeps its table. Starting fresh, the table of A is then copied to
//!    B, C and D, which start close to their own;
//! 3. **ADC zero** — with every CV input unplugged, pressing the right encoder
//!    captures each input's averaged code as its 0 V. Averages restart on
//!    every step, so a reading never mixes two output levels;
//! 4. **ADC gain** — with an output patched into `CV1`, the outputs are driven
//!    to +1 V then +3 V and the right encoder captures `CV1` at each; the
//!    slope through the two readings becomes every input's gain, each keeping
//!    its own zero (one gain for all four, as the reference measures it);
//! 5. **Save values?** — `YES` keeps and stores the calibration, `NO` puts
//!    back what was in force.
//!
//! `up` and `down` go to the previous and next step, the right encoder adjusts
//! or chooses, and pressing it toggles coarse adjustment on a DAC point or
//! captures on an ADC step. The wizard drives the outputs itself; the engine
//! runs with its draft, so a DAC code takes effect on the jack immediately.
//! A capture outside a plausible window around the factory values is refused
//! with a message rather than stored.

use core::fmt::Write as _;

use embedded_graphics::prelude::Point;

use crate::buttons::ButtonEvents;
use crate::calibration::{
    Calibration, CvInputCalibration, CvOutputCalibration, OUTPUT_POINTS, div_round_nearest,
};
use crate::fmt::TextBuf;
use crate::framebuffer::{FrameBuffer, ROW_HEIGHT};
use crate::platform::{Button, CV_CHANNELS, MilliVolts};
use crate::settings::Settings;
use crate::text::{self, TextStyle};

/// Table point of the low DAC step: −3 V, the first octave.
pub const LOW_POINT: usize = 0;

/// Table point of the high DAC step: +5 V, octave 8 as in the reference.
pub const HIGH_POINT: usize = 8;

/// DAC codes per detent, fine: about 1.2 mV.
pub const FINE_STEP: i32 = 8;

/// DAC codes per detent, coarse: about 20 mV.
pub const COARSE_STEP: i32 = 128;

/// Ticks averaged for one ADC reading: a quarter of a second at 1 kHz.
pub const AVERAGE_TICKS: u32 = 256;

/// Ticks ignored after a step change before averaging: the outputs move to
/// the new level one tick late, and the analog stages need a moment to settle.
pub const SETTLE_TICKS: u32 = 32;

/// Largest accepted distance between a captured 0 V code and the factory one.
pub const ZERO_WINDOW: i32 = 200;

/// Largest accepted gain error against the factory slope, in percent.
pub const GAIN_WINDOW_PERCENT: i64 = 10;

/// Levels the gain steps drive the outputs to, as the reference does.
const GAIN_LEVELS_MV: [MilliVolts; 2] = [1_000, 3_000];

/// One step of the wizard.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    /// Start from the factory calibration, or from the one in force.
    StartFresh,
    /// The low point (−3 V) of one output.
    DacLow(usize),
    /// The high point (+5 V) of one output.
    DacHigh(usize),
    /// Every input's 0 V, unplugged.
    AdcZero,
    /// `CV1` at the first gain level.
    AdcLow,
    /// `CV1` at the second gain level.
    AdcHigh,
    /// Keep or discard.
    Finish,
}

/// Number of steps.
const STEPS: usize = 3 + 2 * CV_CHANNELS + 2;

impl Step {
    /// The step at `index`, in wizard order.
    const fn at(index: usize) -> Self {
        match index {
            0 => Self::StartFresh,
            i if i <= 2 * CV_CHANNELS => {
                let channel = (i - 1) / 2;
                if (i - 1) % 2 == 0 {
                    Self::DacLow(channel)
                } else {
                    Self::DacHigh(channel)
                }
            }
            i if i == 2 * CV_CHANNELS + 1 => Self::AdcZero,
            i if i == 2 * CV_CHANNELS + 2 => Self::AdcLow,
            i if i == 2 * CV_CHANNELS + 3 => Self::AdcHigh,
            _ => Self::Finish,
        }
    }
}

/// Panel letter of an output.
const fn output_letter(channel: usize) -> char {
    match channel {
        0 => 'A',
        1 => 'B',
        2 => 'C',
        _ => 'D',
    }
}

/// What the wizard decided on this tick.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WizardOutcome {
    /// Leave, keeping these settings in force and storing them.
    Save(Settings),
    /// Leave, putting these settings back.
    Discard(Settings),
}

/// Why a capture was refused, shown until the next one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Notice {
    /// Nothing to report.
    None,
    /// The capture was stored.
    Captured,
    /// A 0 V code too far from the factory one: an input is probably patched.
    ZeroOutOfRange,
    /// A gain too far from the factory one: probably nothing patched in `CV1`.
    GainOutOfRange,
    /// The +3 V reading is needed before a gain can be computed, or the reverse.
    GainIncomplete,
}

/// Running average of the raw ADC codes, in blocks of [`AVERAGE_TICKS`].
#[derive(Debug, Clone, Copy)]
struct Averager {
    sums: [u32; CV_CHANNELS],
    count: u32,
    /// Readings still to ignore before the first block.
    settling: u32,
    /// Mean of the last complete block, `None` until one completes.
    means: Option<[u16; CV_CHANNELS]>,
}

impl Averager {
    const fn new() -> Self {
        Self {
            sums: [0; CV_CHANNELS],
            count: 0,
            settling: SETTLE_TICKS,
            means: None,
        }
    }

    fn push(&mut self, codes: [u16; CV_CHANNELS]) {
        if self.settling > 0 {
            self.settling -= 1;
            return;
        }
        for (sum, code) in self.sums.iter_mut().zip(codes) {
            *sum = sum.saturating_add(u32::from(code));
        }
        self.count += 1;
        if self.count >= AVERAGE_TICKS {
            let mut means = [0; CV_CHANNELS];
            for (mean, sum) in means.iter_mut().zip(self.sums) {
                let rounded = (sum + AVERAGE_TICKS / 2) / AVERAGE_TICKS;
                *mean = u16::try_from(rounded).unwrap_or(u16::MAX);
            }
            self.means = Some(means);
            self.sums = [0; CV_CHANNELS];
            self.count = 0;
        }
    }
}

/// The calibration wizard.
#[derive(Debug, Clone, Copy)]
pub struct CalibrationWizard {
    original: Settings,
    draft: Settings,
    step: usize,
    fresh: bool,
    coarse: bool,
    keep: bool,
    /// Outputs whose low or high point was moved, which leaving their high
    /// point re-interpolates.
    adjusted: [bool; CV_CHANNELS],
    readings: [Option<u16>; 2],
    average: Averager,
    notice: Notice,
}

impl Default for CalibrationWizard {
    fn default() -> Self {
        Self::new()
    }
}

impl CalibrationWizard {
    /// A closed wizard.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            original: Settings::DEFAULT,
            draft: Settings::DEFAULT,
            step: 0,
            fresh: false,
            coarse: false,
            keep: true,
            adjusted: [false; CV_CHANNELS],
            readings: [None; 2],
            average: Averager::new(),
            notice: Notice::None,
        }
    }

    /// Opens on `draft`, the settings to calibrate; `original` is what
    /// discarding goes back to.
    pub const fn open(&mut self, original: Settings, draft: Settings) {
        *self = Self::new();
        self.original = original;
        self.draft = draft;
    }

    /// Abandons the wizard: what leaving with the `up` + `down` chord means.
    #[must_use]
    pub const fn discard(&self) -> WizardOutcome {
        WizardOutcome::Discard(self.original)
    }

    /// The settings being edited, which the engine runs with while open.
    #[must_use]
    pub const fn draft(&self) -> &Settings {
        &self.draft
    }

    /// The current step.
    #[must_use]
    pub const fn step(&self) -> Step {
        Step::at(self.step)
    }

    /// The level every output is driven to on this step.
    #[must_use]
    pub fn output_level(&self) -> MilliVolts {
        let point = match self.step() {
            Step::DacLow(_) => LOW_POINT,
            Step::DacHigh(_) => HIGH_POINT,
            Step::AdcLow => return GAIN_LEVELS_MV[0],
            Step::AdcHigh => return GAIN_LEVELS_MV[1],
            Step::StartFresh | Step::AdcZero | Step::Finish => return 0,
        };
        let point = i32::try_from(point).unwrap_or(0);
        CvOutputCalibration::LOWEST_MV + point * CvOutputCalibration::POINT_SPACING_MV
    }

    /// Feeds one tick: the button actions, the encoder detents as wired (left,
    /// right) and the raw ADC code of every input.
    pub fn update(
        &mut self,
        buttons: &ButtonEvents,
        deltas: [i8; 2],
        codes: [u16; CV_CHANNELS],
    ) -> Option<WizardOutcome> {
        self.average.push(codes);
        let detents = i32::from(deltas[1]);
        let press = buttons.pressed(Button::RightEncoder);

        match self.step() {
            Step::StartFresh => {
                if detents % 2 != 0 {
                    self.fresh = !self.fresh;
                }
            }
            Step::DacLow(channel) => self.adjust_point(channel, LOW_POINT, detents, press),
            Step::DacHigh(channel) => self.adjust_point(channel, HIGH_POINT, detents, press),
            Step::AdcZero if press => self.capture_zero(),
            Step::AdcLow if press => self.capture_gain(0),
            Step::AdcHigh if press => self.capture_gain(1),
            Step::Finish => {
                if detents % 2 != 0 {
                    self.keep = !self.keep;
                }
                if press {
                    return Some(if self.keep {
                        WizardOutcome::Save(self.draft)
                    } else {
                        WizardOutcome::Discard(self.original)
                    });
                }
            }
            Step::AdcZero | Step::AdcLow | Step::AdcHigh => {}
        }

        if buttons.pressed(Button::Down) {
            self.go_to(self.step + 1);
        } else if buttons.pressed(Button::Up) {
            self.go_to(self.step.saturating_sub(1));
        }
        None
    }

    /// Moves to another step, running what leaving the current one implies.
    fn go_to(&mut self, next: usize) {
        let next = next.min(STEPS - 1);
        if next == self.step {
            return;
        }
        match self.step() {
            Step::StartFresh if self.fresh => self.draft.calibration = Calibration::FACTORY,
            Step::DacHigh(channel) => {
                if self.adjusted[channel] {
                    self.interpolate(channel);
                }
                if self.fresh && channel == 0 {
                    let table = self.draft.calibration.outputs[0];
                    for other in &mut self.draft.calibration.outputs[1..] {
                        *other = table;
                    }
                }
            }
            _ => {}
        }
        self.step = next;
        self.coarse = false;
        self.notice = Notice::None;
        self.average = Averager::new();
    }

    /// Moves one DAC point by `detents`, in volts: clockwise raises the
    /// output, whichever way the table runs.
    fn adjust_point(&mut self, channel: usize, point: usize, detents: i32, press: bool) {
        if press {
            self.coarse = !self.coarse;
        }
        if detents == 0 {
            return;
        }
        let table = &mut self.draft.calibration.outputs[channel].codes;
        let rising = table[OUTPUT_POINTS - 1] >= table[0];
        let step = if self.coarse { COARSE_STEP } else { FINE_STEP };
        let signed = if rising { step } else { -step };
        let moved = (i32::from(table[point]) + detents * signed).clamp(0, i32::from(u16::MAX));
        table[point] = u16::try_from(moved).unwrap_or(table[point]);
        self.adjusted[channel] = true;
    }

    /// Redraws one output's table as the straight line through its low and
    /// high points, as the reference's `InterpolateChannel` does.
    fn interpolate(&mut self, channel: usize) {
        let table = &mut self.draft.calibration.outputs[channel].codes;
        let low = i64::from(table[LOW_POINT]);
        let high = i64::from(table[HIGH_POINT]);
        let span = i64::try_from(HIGH_POINT - LOW_POINT).unwrap_or(1);
        for (index, code) in table.iter_mut().enumerate() {
            let offset = i64::try_from(index).unwrap_or(0) - i64::try_from(LOW_POINT).unwrap_or(0);
            let value = low + div_round_nearest((high - low) * offset, span);
            *code = u16::try_from(value.clamp(0, i64::from(u16::MAX))).unwrap_or(0);
        }
    }

    /// Stores every input's averaged code as its 0 V, if all are plausible.
    fn capture_zero(&mut self) {
        let Some(means) = self.average.means else {
            return;
        };
        let factory = CvInputCalibration::FACTORY.zero_code;
        if means
            .iter()
            .any(|&mean| (i32::from(mean) - factory).abs() > ZERO_WINDOW)
        {
            self.notice = Notice::ZeroOutOfRange;
            return;
        }
        for (input, mean) in self.draft.calibration.inputs.iter_mut().zip(means) {
            input.zero_code = i32::from(mean);
        }
        self.notice = Notice::Captured;
    }

    /// Stores `CV1`'s averaged code at gain level `which`, and the gain once
    /// both are known.
    fn capture_gain(&mut self, which: usize) {
        let Some(means) = self.average.means else {
            return;
        };
        self.readings[which] = Some(means[0]);
        let (Some(low), Some(high)) = (self.readings[0], self.readings[1]) else {
            self.notice = Notice::GainIncomplete;
            return;
        };
        let codes = i64::from(high) - i64::from(low);
        if codes == 0 {
            self.notice = Notice::GainOutOfRange;
            return;
        }
        let millivolts = i64::from(GAIN_LEVELS_MV[1] - GAIN_LEVELS_MV[0]);
        let slope = div_round_nearest(millivolts * 1_000_000, codes);
        let factory = i64::from(CvInputCalibration::FACTORY.nanovolts_per_code);
        let error = (slope - factory).abs() * 100;
        let Ok(slope) = i32::try_from(slope) else {
            self.notice = Notice::GainOutOfRange;
            return;
        };
        if error > factory.abs() * GAIN_WINDOW_PERCENT {
            self.notice = Notice::GainOutOfRange;
            return;
        }
        for input in &mut self.draft.calibration.inputs {
            input.nanovolts_per_code = slope;
        }
        self.notice = Notice::Captured;
    }

    /// Draws the current step: title, instructions, values, controls.
    pub fn render(&self, frame: &mut FrameBuffer) {
        frame.clear();
        let mut lines: [TextBuf<24>; 8] = [TextBuf::new(); 8];
        let _ = write!(lines[0], "CALIBRATE {}/{STEPS}", self.step + 1);
        let calibration = &self.draft.calibration;
        match self.step() {
            Step::StartFresh => {
                let _ = lines[2].write_str("START FRESH?");
                let _ = lines[3].write_str(if self.fresh { "> YES" } else { "> NO" });
                let _ = lines[5].write_str("YES: FACTORY VALUES");
            }
            Step::DacLow(channel) | Step::DacHigh(channel) => {
                let point = if matches!(self.step(), Step::DacLow(_)) {
                    LOW_POINT
                } else {
                    HIGH_POINT
                };
                let letter = output_letter(channel);
                let _ = write!(lines[2], "OUT {letter}: SET ");
                let _ = crate::fmt::write_volts(&mut lines[2], self.output_level(), 3);
                let _ = write!(lines[3], "METER ON OUT {letter}");
                let _ = write!(
                    lines[4],
                    "CODE {} {}",
                    calibration.outputs[channel].codes[point],
                    if self.coarse { "COARSE" } else { "FINE" }
                );
                let _ = lines[5].write_str("R:ADJUST  PUSH:STEP");
            }
            Step::AdcZero => {
                let _ = lines[1].write_str("UNPLUG ALL CV IN");
                self.render_codes(&mut lines);
                let _ = lines[6].write_str("PUSH R: CAPTURE");
            }
            Step::AdcLow | Step::AdcHigh => {
                let which = usize::from(matches!(self.step(), Step::AdcHigh));
                let _ = lines[1].write_str("PATCH AN OUT TO CV1");
                let _ = lines[2].write_str("OUTS AT ");
                let _ = crate::fmt::write_volts(&mut lines[2], GAIN_LEVELS_MV[which], 3);
                let live = self.average.means.map_or(0, |means| means[0]);
                let _ = write!(lines[3], "CV1 {live}");
                if let Some(reading) = self.readings[which] {
                    let _ = write!(lines[4], "TAKEN {reading}");
                }
                let _ = lines[6].write_str("PUSH R: CAPTURE");
            }
            Step::Finish => {
                let _ = lines[2].write_str("SAVE VALUES?");
                let _ = lines[3].write_str(if self.keep { "> YES" } else { "> NO" });
                let _ = lines[5].write_str("PUSH R: CONFIRM");
            }
        }
        let _ = lines[7].write_str(match self.notice {
            Notice::None => "UP:PREV  DOWN:NEXT",
            Notice::Captured => "CAPTURED",
            Notice::ZeroOutOfRange => "REFUSED: UNPLUG CV IN",
            Notice::GainOutOfRange => "REFUSED: CHECK PATCH",
            Notice::GainIncomplete => "NOW THE OTHER LEVEL",
        });

        for (row, line) in lines.iter().enumerate() {
            let y = i32::try_from(row).unwrap_or(0) * ROW_HEIGHT;
            text::draw(frame, line.as_str(), Point::new(0, y), TextStyle::LIT);
        }
    }

    /// The live averaged code and the stored 0 V of every input, one per row.
    fn render_codes(&self, lines: &mut [TextBuf<24>; 8]) {
        let means = self.average.means.unwrap_or([0; CV_CHANNELS]);
        let rows = lines[2..].iter_mut();
        for (channel, ((row, mean), input)) in rows
            .zip(means)
            .zip(&self.draft.calibration.inputs)
            .enumerate()
        {
            let _ = write!(row, "CV{} {mean:4} ZERO {}", channel + 1, input.zero_code);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        AVERAGE_TICKS, CalibrationWizard, FINE_STEP, HIGH_POINT, LOW_POINT, SETTLE_TICKS, Step,
        WizardOutcome,
    };
    use crate::buttons::{ButtonEvents, ButtonReader};
    use crate::calibration::{Calibration, CvInputCalibration, CvOutputCalibration};
    use crate::framebuffer::FrameBuffer;
    use crate::platform::{BUTTONS, Button, CV_CHANNELS, ControlEvents, ENCODERS};
    use crate::settings::Settings;

    /// Button events with `button` firing on this tick.
    fn press(button: Button) -> ButtonEvents {
        let mut reader = ButtonReader::new();
        let mut controls = ControlEvents {
            encoder_delta: [0; ENCODERS],
            button_down: [false; BUTTONS],
        };
        for down in [true, false] {
            controls.button_down[button.index()] = down;
            for _ in 0..8 {
                let events = reader.update(&controls);
                if events.pressed(button) {
                    return events;
                }
            }
        }
        panic!("{button:?} never fired");
    }

    /// Feeds `codes` long enough for one complete average.
    fn settle(wizard: &mut CalibrationWizard, codes: [u16; CV_CHANNELS]) {
        for _ in 0..SETTLE_TICKS + AVERAGE_TICKS {
            assert_eq!(wizard.update(&ButtonEvents::default(), [0, 0], codes), None);
        }
    }

    fn next(wizard: &mut CalibrationWizard) {
        wizard.update(&press(Button::Down), [0, 0], [2_730; CV_CHANNELS]);
    }

    fn opened() -> CalibrationWizard {
        let mut wizard = CalibrationWizard::new();
        wizard.open(Settings::DEFAULT, Settings::DEFAULT);
        wizard
    }

    #[test]
    fn the_steps_follow_the_reference_order() {
        let mut wizard = opened();
        let mut seen = [Step::Finish; 20];
        for step in &mut seen {
            *step = wizard.step();
            next(&mut wizard);
        }
        assert_eq!(
            &seen[..4],
            &[
                Step::StartFresh,
                Step::DacLow(0),
                Step::DacHigh(0),
                Step::DacLow(1)
            ]
        );
        assert_eq!(
            seen[9..13],
            [Step::AdcZero, Step::AdcLow, Step::AdcHigh, Step::Finish]
        );
        assert_eq!(seen[19], Step::Finish, "the last step stays");
    }

    #[test]
    fn dac_points_drive_the_outputs_and_move_towards_higher_volts_clockwise() {
        let mut wizard = opened();
        next(&mut wizard);
        assert_eq!(wizard.step(), Step::DacLow(0));
        assert_eq!(wizard.output_level(), -3_000);
        let before = wizard.draft().calibration.outputs[0].codes[LOW_POINT];
        wizard.update(&ButtonEvents::default(), [0, 1], [0; CV_CHANNELS]);
        let after = wizard.draft().calibration.outputs[0].codes[LOW_POINT];
        // The factory table falls: a higher voltage is a lower code.
        assert_eq!(i32::from(before) - i32::from(after), FINE_STEP);
        next(&mut wizard);
        assert_eq!(wizard.output_level(), 5_000);
    }

    #[test]
    fn leaving_the_high_point_interpolates_the_table() {
        let mut wizard = opened();
        next(&mut wizard);
        next(&mut wizard);
        assert_eq!(wizard.step(), Step::DacHigh(0));
        wizard.update(&ButtonEvents::default(), [0, -10], [0; CV_CHANNELS]);
        next(&mut wizard);
        let table = wizard.draft().calibration.outputs[0].codes;
        let low = i32::from(table[LOW_POINT]);
        let high = i32::from(table[HIGH_POINT]);
        for (index, &code) in table.iter().enumerate().take(HIGH_POINT + 1) {
            let expected = low + (high - low) * i32::try_from(index).unwrap() / 8;
            assert!((i32::from(code) - expected).abs() <= 1, "point {index}");
        }
    }

    #[test]
    fn walking_through_the_steps_changes_nothing() {
        let mut wizard = opened();
        while wizard.step() != Step::Finish {
            next(&mut wizard);
        }
        assert_eq!(wizard.draft(), &Settings::DEFAULT);
    }

    #[test]
    fn starting_fresh_copies_output_a_to_the_others() {
        let mut stored = Settings::DEFAULT;
        stored.calibration.outputs[2] = CvOutputCalibration::linear(30_000, -6_000_000);
        let mut wizard = CalibrationWizard::new();
        wizard.open(stored, stored);
        wizard.update(&ButtonEvents::default(), [0, 1], [0; CV_CHANNELS]);
        next(&mut wizard);
        assert_eq!(wizard.draft().calibration, Calibration::FACTORY);
        next(&mut wizard);
        wizard.update(&ButtonEvents::default(), [0, 3], [0; CV_CHANNELS]);
        next(&mut wizard);
        let outputs = wizard.draft().calibration.outputs;
        assert!(outputs.iter().all(|table| *table == outputs[0]));
    }

    #[test]
    fn zeros_are_captured_averaged_and_refused_when_implausible() {
        let mut wizard = opened();
        while wizard.step() != Step::AdcZero {
            next(&mut wizard);
        }
        settle(&mut wizard, [2_735, 2_728, 2_731, 2_740]);
        wizard.update(
            &press(Button::RightEncoder),
            [0, 0],
            [2_735, 2_728, 2_731, 2_740],
        );
        let zeros = wizard
            .draft()
            .calibration
            .inputs
            .map(|input| input.zero_code);
        assert_eq!(zeros, [2_735, 2_728, 2_731, 2_740]);

        settle(&mut wizard, [2_735, 2_728, 1_000, 2_740]);
        wizard.update(
            &press(Button::RightEncoder),
            [0, 0],
            [2_735, 2_728, 1_000, 2_740],
        );
        assert_eq!(
            wizard.draft().calibration.inputs[2].zero_code,
            2_731,
            "a patched input must not become a zero"
        );
    }

    #[test]
    fn the_gain_comes_from_two_readings_on_cv1_and_applies_to_every_input() {
        let mut wizard = opened();
        while wizard.step() != Step::AdcLow {
            next(&mut wizard);
        }
        assert_eq!(wizard.output_level(), 1_000);
        // A slope 2 % steeper than the factory one.
        let factory = CvInputCalibration::FACTORY;
        let one_volt = 2_730 - 401;
        let three_volts = 2_730 - 1_204;
        settle(&mut wizard, [one_volt; CV_CHANNELS]);
        wizard.update(
            &press(Button::RightEncoder),
            [0, 0],
            [one_volt; CV_CHANNELS],
        );
        assert_eq!(
            wizard.draft().calibration.inputs[0].nanovolts_per_code,
            factory.nanovolts_per_code,
            "one reading is not a gain yet"
        );
        next(&mut wizard);
        assert_eq!(wizard.output_level(), 3_000);
        settle(&mut wizard, [three_volts; CV_CHANNELS]);
        wizard.update(
            &press(Button::RightEncoder),
            [0, 0],
            [three_volts; CV_CHANNELS],
        );
        let slope = wizard.draft().calibration.inputs[3].nanovolts_per_code;
        assert_eq!(slope, -2_490_660, "2000 mV over -803 codes");

        // Nothing patched: CV1 does not move between the levels.
        let mut wizard = opened();
        while wizard.step() != Step::AdcLow {
            next(&mut wizard);
        }
        settle(&mut wizard, [2_730; CV_CHANNELS]);
        wizard.update(&press(Button::RightEncoder), [0, 0], [2_730; CV_CHANNELS]);
        next(&mut wizard);
        settle(&mut wizard, [2_731; CV_CHANNELS]);
        wizard.update(&press(Button::RightEncoder), [0, 0], [2_731; CV_CHANNELS]);
        assert_eq!(
            wizard.draft().calibration.inputs[0].nanovolts_per_code,
            factory.nanovolts_per_code
        );
    }

    #[test]
    fn finishing_saves_or_discards() {
        let mut stored = Settings::DEFAULT;
        stored.display.rotated = true;
        let mut wizard = CalibrationWizard::new();
        wizard.open(stored, stored);
        next(&mut wizard);
        wizard.update(&ButtonEvents::default(), [0, 2], [0; CV_CHANNELS]);
        while wizard.step() != Step::Finish {
            next(&mut wizard);
        }
        let edited = *wizard.draft();
        assert_ne!(
            edited.calibration, stored.calibration,
            "output A was adjusted"
        );
        let mut keeping = wizard;
        assert_eq!(
            keeping.update(&press(Button::RightEncoder), [0, 0], [0; CV_CHANNELS]),
            Some(WizardOutcome::Save(edited))
        );
        wizard.update(&ButtonEvents::default(), [0, 1], [0; CV_CHANNELS]);
        assert_eq!(
            wizard.update(&press(Button::RightEncoder), [0, 0], [0; CV_CHANNELS]),
            Some(WizardOutcome::Discard(stored))
        );
    }

    #[test]
    fn every_step_draws_something() {
        let mut wizard = opened();
        let mut frame = FrameBuffer::new();
        for _ in 0..13 {
            wizard.render(&mut frame);
            assert!(frame.lit_pixels() > 0, "{:?}", wizard.step());
            next(&mut wizard);
        }
    }
}
