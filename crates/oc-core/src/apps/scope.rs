//! A minimal scope applet: the second app, and the proof that there is a menu.
//!
//! It is deliberately small. Its job is to be unmistakably *not* the diagnostic
//! screen — on the panel and on the jacks alike — so that a single assertion can
//! prove an app switch actually happened:
//!
//! * on screen, a scrolling trace of `CV1` instead of the diagnostic table;
//! * on the jacks, `CV1` copied to all four outputs, an output signature no
//!   [`OutputMode`](crate::apps::diagnostic::OutputMode) can produce (`Offset` mirrors channel
//!   by channel, `Ramp` shifts each channel by a quarter period, `Zero` is flat).
//!
//! It reads no control at all, which is the point: an applet owes the engine
//! `update` and `render`, and nothing else.

use core::fmt::Write as _;

use embedded_graphics::Drawable as _;
use embedded_graphics::mono_font::MonoTextStyle;
use embedded_graphics::mono_font::ascii::FONT_5X8;
use embedded_graphics::pixelcolor::BinaryColor;
use embedded_graphics::prelude::Point;
use embedded_graphics::text::{Baseline, Text};

use crate::apps::{InputSnapshot, TickContext};
use crate::calibration::{CV_OUT_MAX_MV, CV_OUT_MIN_MV};
use crate::fmt::{TextBuf, write_volts};
use crate::framebuffer::{FrameBuffer, WIDTH};
use crate::platform::{CV_CHANNELS, MilliVolts};

/// Columns of history: one per screen pixel column.
pub const TRACE_LEN: usize = WIDTH;

/// Virtual time between two trace samples.
///
/// At this rate the full width of the screen holds a little over one second of
/// history, which is slow enough to read an LFO and fast enough to see a gate.
/// It is measured in the tick's own elapsed time, so a replayed scenario and a
/// real module draw the same trace.
pub const SAMPLE_INTERVAL_MICROS: u32 = 8_000;

/// Topmost pixel row of the plot area, below the title line.
const PLOT_TOP: i32 = 16;

/// Height of the plot area in pixels.
const PLOT_HEIGHT: i32 = 48;

/// A scrolling view of `CV1`, buffered to every output.
#[derive(Debug, Clone)]
pub struct ScopeApp {
    /// Plot row for each recorded column, oldest first once the ring has wrapped.
    trace: [u8; TRACE_LEN],
    write: usize,
    filled: usize,
    accumulator_micros: u32,
    level: MilliVolts,
    outputs: [MilliVolts; CV_CHANNELS],
}

impl ScopeApp {
    /// A scope with an empty trace, sitting at 0 V.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            trace: [0; TRACE_LEN],
            write: 0,
            filled: 0,
            accumulator_micros: 0,
            level: 0,
            outputs: [0; CV_CHANNELS],
        }
    }

    /// Consumes one snapshot and returns the levels the outputs should take.
    pub fn update(&mut self, input: &InputSnapshot) -> [MilliVolts; CV_CHANNELS] {
        self.level = input.cv[0];
        self.outputs = [self.level.clamp(CV_OUT_MIN_MV, CV_OUT_MAX_MV); CV_CHANNELS];

        self.accumulator_micros = self.accumulator_micros.saturating_add(input.elapsed_micros);
        // A stalled tick must not spend the whole budget redrawing history that
        // is about to scroll off anyway, so never push more than a screenful.
        let samples = usize::try_from(self.accumulator_micros / SAMPLE_INTERVAL_MICROS)
            .unwrap_or(TRACE_LEN)
            .min(TRACE_LEN);
        self.accumulator_micros %= SAMPLE_INTERVAL_MICROS;
        for _ in 0..samples {
            self.push(self.level);
        }

        self.outputs
    }

    /// Records one sample at the write head, wrapping around the ring.
    fn push(&mut self, level: MilliVolts) {
        self.trace[self.write] = row_for(level);
        self.write = (self.write + 1) % TRACE_LEN;
        self.filled = (self.filled + 1).min(TRACE_LEN);
    }

    /// Plot row recorded `column` samples after the oldest one still held.
    #[must_use]
    fn column(&self, column: usize) -> Option<u8> {
        if column >= self.filled {
            return None;
        }
        let oldest = if self.filled < TRACE_LEN {
            0
        } else {
            self.write
        };
        Some(self.trace[(oldest + column) % TRACE_LEN])
    }

    /// The level last read on `CV1`.
    #[must_use]
    pub const fn level(&self) -> MilliVolts {
        self.level
    }

    /// The levels currently driven on the outputs.
    #[must_use]
    pub const fn outputs(&self) -> &[MilliVolts; CV_CHANNELS] {
        &self.outputs
    }

    /// Number of samples currently held in the trace.
    #[must_use]
    pub const fn recorded(&self) -> usize {
        self.filled
    }

    /// Draws the title line and the trace.
    pub fn render(&self, frame: &mut FrameBuffer, _context: &TickContext) {
        frame.clear();
        let style = MonoTextStyle::new(&FONT_5X8, BinaryColor::On);
        let mut line = TextBuf::<26>::new();
        let _ = write!(line, "SCOPE CV1 ");
        let _ = write_volts(&mut line, self.level, 3);
        let _ = Text::with_baseline(line.as_str(), Point::zero(), style, Baseline::Top).draw(frame);

        for column in 0..TRACE_LEN {
            let Some(row) = self.column(column) else {
                break;
            };
            let Ok(x) = i32::try_from(column) else {
                break;
            };
            frame.set_pixel(x, PLOT_TOP + i32::from(row), true);
        }
    }
}

impl Default for ScopeApp {
    fn default() -> Self {
        Self::new()
    }
}

/// Plot row for a level: the top of the plot is [`CV_OUT_MAX_MV`], the bottom
/// [`CV_OUT_MIN_MV`], and anything outside that range is pinned to the edge.
fn row_for(level: MilliVolts) -> u8 {
    let clamped = level.clamp(CV_OUT_MIN_MV, CV_OUT_MAX_MV);
    let span = i64::from(CV_OUT_MAX_MV - CV_OUT_MIN_MV);
    let from_top = i64::from(CV_OUT_MAX_MV - clamped) * i64::from(PLOT_HEIGHT - 1) / span;
    u8::try_from(from_top).unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::{PLOT_HEIGHT, SAMPLE_INTERVAL_MICROS, ScopeApp, TRACE_LEN, row_for};
    use crate::apps::{InputSnapshot, TickContext};
    use crate::calibration::{CV_OUT_MAX_MV, CV_OUT_MIN_MV};
    use crate::framebuffer::FrameBuffer;
    use crate::platform::{CV_CHANNELS, MilliVolts};

    fn snapshot(level: MilliVolts, elapsed_micros: u32) -> InputSnapshot {
        let mut input = InputSnapshot {
            elapsed_micros,
            ..InputSnapshot::default()
        };
        input.cv[0] = level;
        input
    }

    #[test]
    fn every_output_carries_the_first_input() {
        let mut scope = ScopeApp::new();
        let outputs = scope.update(&snapshot(2_500, 1_000));
        assert_eq!(outputs, [2_500; CV_CHANNELS]);
        assert_eq!(scope.level(), 2_500);
        assert_eq!(scope.outputs(), &outputs);
    }

    #[test]
    fn outputs_are_clamped_to_what_the_hardware_can_produce() {
        let mut scope = ScopeApp::new();
        assert_eq!(scope.update(&snapshot(99_000, 1_000)), [CV_OUT_MAX_MV; 4]);
        assert_eq!(scope.update(&snapshot(-99_000, 1_000)), [CV_OUT_MIN_MV; 4]);
    }

    #[test]
    fn a_sample_is_recorded_once_the_interval_has_elapsed() {
        let mut scope = ScopeApp::new();
        scope.update(&snapshot(0, SAMPLE_INTERVAL_MICROS - 1));
        assert_eq!(scope.recorded(), 0, "not enough time has passed yet");
        scope.update(&snapshot(0, 1));
        assert_eq!(scope.recorded(), 1);
    }

    #[test]
    fn the_trace_wraps_without_panicking_and_keeps_the_newest_sample_last() {
        let mut scope = ScopeApp::new();
        for step in 0..(TRACE_LEN * 3) {
            let level = if step % 2 == 0 {
                CV_OUT_MIN_MV
            } else {
                CV_OUT_MAX_MV
            };
            scope.update(&snapshot(level, SAMPLE_INTERVAL_MICROS));
        }
        assert_eq!(
            scope.recorded(),
            TRACE_LEN,
            "the ring saturates, it does not grow"
        );
        assert_eq!(
            scope.column(TRACE_LEN - 1),
            Some(row_for(CV_OUT_MAX_MV)),
            "the last column holds the most recent sample"
        );
        assert_eq!(scope.column(TRACE_LEN), None);
    }

    #[test]
    fn a_stalled_tick_pushes_at_most_one_screenful() {
        let mut scope = ScopeApp::new();
        scope.update(&snapshot(0, SAMPLE_INTERVAL_MICROS * 4_000));
        assert_eq!(scope.recorded(), TRACE_LEN);
    }

    #[test]
    fn the_plot_row_spans_the_output_range_top_down() {
        assert_eq!(
            row_for(CV_OUT_MAX_MV),
            0,
            "the highest level sits at the top"
        );
        assert_eq!(
            i32::from(row_for(CV_OUT_MIN_MV)),
            PLOT_HEIGHT - 1,
            "the lowest level sits at the bottom of the plot"
        );
        assert_eq!(row_for(99_000), row_for(CV_OUT_MAX_MV));
        assert_eq!(row_for(-99_000), row_for(CV_OUT_MIN_MV));
    }

    #[test]
    fn rendering_draws_the_title_and_the_trace() {
        let mut scope = ScopeApp::new();
        let mut frame = FrameBuffer::new();
        scope.render(&mut frame, &frame_context());
        let title_only = frame.lit_pixels();
        assert!(title_only > 0, "the title must be visible immediately");

        for _ in 0..TRACE_LEN {
            scope.update(&snapshot(1_000, SAMPLE_INTERVAL_MICROS));
        }
        scope.render(&mut frame, &frame_context());
        assert!(
            frame.lit_pixels() > title_only,
            "a filled trace must light more pixels than the title alone"
        );
    }

    /// The scope ignores the tick context; this keeps the tests readable.
    const fn frame_context() -> TickContext {
        TickContext {
            tick_count: 0,
            duration_micros: 0,
        }
    }
}
