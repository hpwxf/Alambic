//! The tick loop shared by every backend.
//!
//! [`Engine::tick`] is the single function the firmware, the simulator and the
//! VCV Rack module call. Everything that happens inside it — acquisition,
//! state update, output, rendering — is identical in the three environments,
//! which is what makes a test written against the simulator meaningful for the
//! hardware.

use crate::app::{DiagnosticApp, InputSnapshot, TickContext};
use crate::framebuffer::FrameBuffer;
use crate::platform::{
    AnalogIn, AnalogOut, CV_CHANNELS, Clock, Controls, DigitalIn, Display, MilliVolts,
    TRIGGER_CHANNELS, TriggerChannel,
};
use crate::splash::SplashScreen;

/// Upper bound reported for a tick duration, in microseconds.
///
/// A clock that jumps backwards, or one whose 64-bit microsecond counter
/// wraps, must not turn into an absurd duration on screen.
const MAX_REPORTED_DURATION_MICROS: u32 = 1_000_000;

/// Default number of ticks between two screen redraws.
///
/// One means "redraw on every tick", which is what tests and the simulator
/// want. Benchmarks show that drawing the screen costs roughly a thousand
/// times more than updating the applet state, while an OLED panel cannot show
/// more than about sixty frames per second, so the firmware raises this to
/// spend its 1 kHz budget on the signal path instead.
pub const DEFAULT_RENDER_INTERVAL: u32 = 1;

/// Outcome of one tick.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TickReport {
    /// Clock reading taken at the start of the tick.
    pub timestamp_micros: u64,
    /// Microseconds elapsed since the previous tick, zero on the first one.
    pub elapsed_micros: u32,
    /// Time spent inside this tick, in microseconds.
    pub duration_micros: u32,
    /// Number of ticks executed since the engine was created.
    pub tick_count: u64,
    /// Levels written to the CV outputs during this tick.
    pub cv_out: [MilliVolts; CV_CHANNELS],
    /// Whether the screen was redrawn and presented during this tick.
    pub rendered: bool,
}

/// Drives the diagnostic applet against a platform.
#[derive(Debug)]
pub struct Engine<A, O, D, C, K, S> {
    analog_in: A,
    analog_out: O,
    digital_in: D,
    controls: C,
    clock: K,
    display: S,
    app: DiagnosticApp,
    splash: SplashScreen,
    tick_count: u64,
    previous_micros: Option<u64>,
    last_duration_micros: u32,
    render_interval: u32,
    ticks_since_render: u32,
}

impl<A, O, D, C, K, S> Engine<A, O, D, C, K, S>
where
    A: AnalogIn,
    O: AnalogOut,
    D: DigitalIn,
    C: Controls,
    K: Clock,
    S: Display,
{
    /// Builds an engine over the given platform implementations.
    pub fn new(
        analog_in: A,
        analog_out: O,
        digital_in: D,
        controls: C,
        clock: K,
        display: S,
    ) -> Self {
        Self {
            analog_in,
            analog_out,
            digital_in,
            controls,
            clock,
            display,
            app: DiagnosticApp::new(),
            splash: SplashScreen::new(),
            tick_count: 0,
            previous_micros: None,
            last_duration_micros: 0,
            render_interval: DEFAULT_RENDER_INTERVAL,
            ticks_since_render: 0,
        }
    }

    /// Redraws the screen once every `ticks` ticks.
    ///
    /// Values below one are treated as one. Changing this affects only when the
    /// screen is refreshed; the signal path runs on every tick regardless.
    pub const fn set_render_interval(&mut self, ticks: u32) {
        self.render_interval = if ticks == 0 { 1 } else { ticks };
        self.ticks_since_render = 0;
    }

    /// Current number of ticks between two redraws.
    #[must_use]
    pub const fn render_interval(&self) -> u32 {
        self.render_interval
    }

    /// Runs one complete cycle: acquire, update, output, render.
    ///
    /// While the boot splash screen has not yet finished tracing its border
    /// around the screen, this instead runs [`Self::tick_splash`]: the
    /// diagnostic applet does not see any input and the outputs stay at
    /// rest, so nothing downstream can react to a signal before the module
    /// has visibly finished starting up.
    pub fn tick(&mut self) -> TickReport {
        let started = self.clock.now_micros();
        let elapsed = self.elapsed_since_previous(started);
        self.previous_micros = Some(started);
        self.tick_count = self.tick_count.wrapping_add(1);

        if !self.splash.is_done() {
            return self.tick_splash(started, elapsed);
        }

        let snapshot = self.acquire(elapsed);
        let outputs = self.app.update(&snapshot);

        for (index, &level) in outputs.iter().enumerate() {
            if let Some(channel) = crate::platform::CvChannel::from_index(index) {
                self.analog_out.write_cv(channel, level);
            }
        }
        self.analog_out.flush();

        self.ticks_since_render += 1;
        let rendered = self.ticks_since_render >= self.render_interval;
        if rendered {
            self.ticks_since_render = 0;
            let context = TickContext {
                tick_count: self.tick_count,
                duration_micros: self.last_duration_micros,
            };
            self.app.render(self.display.frame_mut(), &context);
            self.display.present();
        }

        let duration = clamp_duration(self.clock.now_micros().wrapping_sub(started));
        self.last_duration_micros = duration;

        TickReport {
            timestamp_micros: started,
            elapsed_micros: elapsed,
            duration_micros: duration,
            tick_count: self.tick_count,
            cv_out: outputs,
            rendered,
        }
    }

    /// Runs one tick of the boot animation.
    ///
    /// Control input is drained so it is not replayed in one lump once the
    /// diagnostic applet takes over, but otherwise ignored; the outputs stay
    /// at rest and the animation is rendered on every tick, regardless of
    /// [`Self::render_interval`], so the border traces smoothly.
    fn tick_splash(&mut self, started: u64, elapsed: u32) -> TickReport {
        let _ = self.controls.poll();
        self.splash.advance(elapsed);

        let outputs = [0; CV_CHANNELS];
        for (index, &level) in outputs.iter().enumerate() {
            if let Some(channel) = crate::platform::CvChannel::from_index(index) {
                self.analog_out.write_cv(channel, level);
            }
        }
        self.analog_out.flush();

        self.splash.render(self.display.frame_mut());
        self.display.present();

        let duration = clamp_duration(self.clock.now_micros().wrapping_sub(started));
        self.last_duration_micros = duration;

        TickReport {
            timestamp_micros: started,
            elapsed_micros: elapsed,
            duration_micros: duration,
            tick_count: self.tick_count,
            cv_out: outputs,
            rendered: true,
        }
    }

    /// Restarts the engine as if freshly powered on: the diagnostic applet's
    /// state is discarded and the boot splash screen plays again before
    /// normal execution resumes.
    ///
    /// This is what a host should call for its own "Initialize" action (VCV
    /// Rack's `Module::onReset`, for instance), so the module confirms its
    /// identity on screen the same way after a manual reset as after a power
    /// cycle.
    pub fn reset(&mut self) {
        self.app = DiagnosticApp::new();
        self.splash.reset();
        self.tick_count = 0;
        self.previous_micros = None;
        self.last_duration_micros = 0;
        self.ticks_since_render = 0;
    }

    /// Skips straight past the boot splash screen, as if it had already run
    /// its course.
    ///
    /// Meant for hosts and tests that have no use for the boot animation;
    /// production backends should let it play out instead.
    pub const fn skip_splash(&mut self) {
        self.splash.finish();
    }

    /// Samples every input.
    fn acquire(&mut self, elapsed_micros: u32) -> InputSnapshot {
        let mut snapshot = InputSnapshot {
            elapsed_micros,
            controls: self.controls.poll(),
            ..InputSnapshot::default()
        };

        for channel in crate::platform::CvChannel::ALL {
            let index = channel.index();
            snapshot.cv[index] = self.analog_in.read_cv(channel);
            snapshot.patched[index] = self.analog_in.is_patched(channel);
        }

        for channel in TriggerChannel::ALL {
            snapshot.triggers[channel.index()] = self.digital_in.trigger_state(channel);
        }

        debug_assert_eq!(snapshot.cv.len(), CV_CHANNELS);
        debug_assert_eq!(snapshot.triggers.len(), TRIGGER_CHANNELS);
        snapshot
    }

    /// Time since the previous tick, clamped and wrap-safe.
    fn elapsed_since_previous(&self, now: u64) -> u32 {
        let Some(previous) = self.previous_micros else {
            return 0;
        };
        clamp_duration(now.wrapping_sub(previous))
    }

    /// The applet, for inspection by tests and by the host UIs.
    #[must_use]
    pub const fn app(&self) -> &DiagnosticApp {
        &self.app
    }

    /// The framebuffer as last rendered.
    pub fn frame(&mut self) -> &FrameBuffer {
        self.display.frame_mut()
    }

    /// Number of ticks executed so far.
    #[must_use]
    pub const fn tick_count(&self) -> u64 {
        self.tick_count
    }

    /// The clock, so a host can advance a virtual one between ticks.
    #[must_use]
    pub const fn clock(&self) -> &K {
        &self.clock
    }

    /// Borrows the platform pieces, so a host can drive its own inputs between
    /// ticks.
    pub const fn parts_mut(&mut self) -> (&mut A, &mut O, &mut D, &mut C, &mut S) {
        (
            &mut self.analog_in,
            &mut self.analog_out,
            &mut self.digital_in,
            &mut self.controls,
            &mut self.display,
        )
    }
}

/// Converts a raw microsecond difference into a plausible duration.
fn clamp_duration(raw: u64) -> u32 {
    u32::try_from(raw).map_or(MAX_REPORTED_DURATION_MICROS, |value| {
        value.min(MAX_REPORTED_DURATION_MICROS)
    })
}

#[cfg(test)]
mod tests {
    use super::{MAX_REPORTED_DURATION_MICROS, clamp_duration};
    use crate::app::OutputMode;
    use crate::platform::{CV_CHANNELS, CvChannel};
    use crate::splash;
    use crate::testing::mock_engine_at_boot;

    #[test]
    fn durations_are_clamped_to_something_believable() {
        assert_eq!(clamp_duration(0), 0);
        assert_eq!(clamp_duration(1_000), 1_000);
        assert_eq!(
            clamp_duration(u64::MAX),
            MAX_REPORTED_DURATION_MICROS,
            "a backwards clock must not report a nonsense duration"
        );
        assert_eq!(
            clamp_duration(5_000_000),
            MAX_REPORTED_DURATION_MICROS,
            "a stalled tick is reported as the ceiling, not as-is"
        );
    }

    #[test]
    fn a_freshly_built_engine_boots_into_the_splash_screen() {
        let mut engine = mock_engine_at_boot(0);
        {
            let (analog_in, ..) = engine.parts_mut();
            analog_in.patch(CvChannel::Two, 4_000);
        }

        let report = engine.tick();
        assert_eq!(
            report.cv_out, [0; CV_CHANNELS],
            "outputs stay at rest during the boot animation"
        );
        assert!(report.rendered);
        assert!(
            engine.frame().lit_pixels() > 0,
            "the banner must be visible on the very first frame"
        );
    }

    #[test]
    fn normal_execution_starts_once_the_splash_screen_is_done() {
        let mut engine = mock_engine_at_boot(0);
        {
            let (analog_in, ..) = engine.parts_mut();
            analog_in.patch(CvChannel::Two, 4_000);
        }

        // Run comfortably past the end of the boot animation.
        let ticks = splash::DURATION_MICROS / 1_000 + 2;
        for _ in 0..ticks {
            engine.clock().advance(1_000);
            engine.tick();
        }

        engine.clock().advance(1_000);
        let report = engine.tick();

        assert_eq!(
            report.cv_out[1], 4_000,
            "once booted, inputs must reach the matching output again"
        );
        assert_eq!(engine.app().mode(), OutputMode::Offset);
    }

    #[test]
    fn resetting_the_engine_replays_the_splash_screen() {
        let mut engine = mock_engine_at_boot(0);
        let ticks = splash::DURATION_MICROS / 1_000 + 2;
        for _ in 0..ticks {
            engine.clock().advance(1_000);
            engine.tick();
        }
        assert_eq!(engine.tick_count(), u64::from(ticks));

        engine.reset();
        assert_eq!(engine.tick_count(), 0);

        let report = engine.tick();
        assert_eq!(
            report.cv_out, [0; CV_CHANNELS],
            "a reset engine boots again before resuming normal execution"
        );
    }

    #[test]
    fn skip_splash_bypasses_the_boot_animation() {
        let mut engine = mock_engine_at_boot(0);
        engine.skip_splash();
        {
            let (analog_in, ..) = engine.parts_mut();
            analog_in.patch(CvChannel::Two, 1_500);
        }

        let report = engine.tick();
        assert_eq!(report.cv_out[1], 1_500);
    }
}
