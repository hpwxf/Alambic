//! The simulated module.
//!
//! [`Simulator`] owns an [`Engine`](oc_core::Engine) wired to the host backend
//! from [`oc_core::testing`]. That backend is deliberately *not* reimplemented
//! here: sharing it with the core's own tests guarantees that what the
//! simulator shows is exactly what the test suite asserts on.
//!
//! What this module adds on top is the parts a human or a scenario needs:
//! momentary presses that survive debouncing, a tick counter, and replay.

use oc_core::TickReport;
use oc_core::app::DiagnosticApp;
use oc_core::framebuffer::FrameBuffer;
use oc_core::platform::{
    BUTTONS, Button, CV_CHANNELS, CvChannel, MilliVolts, TRIGGER_CHANNELS, TriggerChannel,
};
use oc_core::testing::{MockEngine, mock_engine_at_boot};

use crate::clock::TICK_MICROS;
use crate::scenario::{Event, Scenario};

/// How long a momentary press is held, in ticks.
///
/// It must exceed the core's debounce depth, otherwise a keystroke would be
/// filtered out as contact bounce.
pub const PRESS_TICKS: u32 = 6;

/// Microseconds of virtual time the module is assumed to spend per tick.
///
/// The host runs orders of magnitude faster than a Cortex-M7, so charging a
/// fixed, plausible cost keeps the on-screen cycle time meaningful instead of
/// showing the host's numbers.
const TICK_COST_MICROS: u64 = 120;

/// A module running entirely in memory.
#[derive(Debug)]
pub struct Simulator {
    engine: MockEngine,
    tick: u64,
    button_hold: [u32; BUTTONS],
    trigger_hold: [u32; TRIGGER_CHANNELS],
    recording: Option<Scenario>,
    last_report: Option<TickReport>,
}

impl Simulator {
    /// A module with every input at rest, freshly powered on: it boots into
    /// the same splash screen (name, version and a border tracing itself
    /// around the screen) as the firmware and the VCV Rack module, before
    /// [`Self::step`] starts reaching the diagnostic applet.
    #[must_use]
    pub fn new() -> Self {
        Self {
            engine: mock_engine_at_boot(TICK_COST_MICROS),
            tick: 0,
            button_hold: [0; BUTTONS],
            trigger_hold: [0; TRIGGER_CHANNELS],
            recording: None,
            last_report: None,
        }
    }

    /// Skips straight past the boot splash screen, as if it had already run
    /// its course.
    ///
    /// Meant for tests that exercise the applet's steady-state behaviour
    /// directly, without waiting out the animation the way a real module or
    /// an interactive session would; production code should let it play.
    pub fn skip_splash(&mut self) {
        self.engine.skip_splash();
    }

    /// Restarts the module as if freshly powered on: the diagnostic applet's
    /// state is discarded and the boot splash screen plays again before
    /// normal execution resumes.
    ///
    /// This is the simulator's equivalent of a host's "Initialize" action
    /// (VCV Rack's `Module::onReset`, wired to `oc_engine_reset`), so it
    /// exercises exactly the same [`Engine::reset`](oc_core::Engine::reset)
    /// path.
    pub fn reset(&mut self) {
        self.apply(Event::Reset);
    }

    /// Starts recording every applied event into a fresh scenario.
    pub fn start_recording(&mut self) {
        self.recording = Some(Scenario::new(self.tick));
    }

    /// Stops recording and returns what was captured.
    pub fn stop_recording(&mut self) -> Option<Scenario> {
        let mut scenario = self.recording.take()?;
        scenario.ticks = scenario.ticks.max(self.tick);
        Some(scenario)
    }

    /// Whether events are currently being recorded.
    #[must_use]
    pub const fn is_recording(&self) -> bool {
        self.recording.is_some()
    }

    /// Applies one input change, recording it if recording is on.
    pub fn apply(&mut self, event: Event) {
        if let Some(scenario) = &mut self.recording {
            scenario.push(self.tick, event);
        }

        if matches!(event, Event::Reset) {
            self.engine.reset();
            // A power cycle leaves no momentary press half-completed.
            self.button_hold = [0; BUTTONS];
            self.trigger_hold = [0; TRIGGER_CHANNELS];
            return;
        }

        let (analog_in, _, digital_in, controls, _) = self.engine.parts_mut();
        match event {
            Event::Cv {
                channel,
                millivolts,
            } => {
                if let Some(channel) = CvChannel::from_index(channel) {
                    analog_in.levels[channel.index()] = millivolts;
                }
            }
            Event::Patch { channel, patched } => {
                if let Some(channel) = CvChannel::from_index(channel) {
                    analog_in.patched[channel.index()] = patched;
                }
            }
            Event::Trigger { channel, high } => {
                if let Some(channel) = TriggerChannel::from_index(channel) {
                    digital_in.set(channel, high);
                }
            }
            Event::Encoder { index, detents } => controls.turn(index, detents),
            Event::Button { button, down } => controls.hold(button, down),
            Event::Reset => unreachable!("handled above, before borrowing the engine's parts"),
        }
    }

    /// Presses a button for [`PRESS_TICKS`] ticks, then releases it.
    pub fn press(&mut self, button: Button) {
        self.apply(Event::Button { button, down: true });
        self.button_hold[button.index()] = PRESS_TICKS;
    }

    /// Raises a trigger for [`PRESS_TICKS`] ticks, then lowers it.
    pub fn pulse(&mut self, channel: TriggerChannel) {
        self.apply(Event::Trigger {
            channel: channel.index(),
            high: true,
        });
        self.trigger_hold[channel.index()] = PRESS_TICKS;
    }

    /// Runs one tick.
    pub fn step(&mut self) -> TickReport {
        self.engine.clock().advance(TICK_MICROS);
        let report = self.engine.tick();
        self.tick = self.tick.wrapping_add(1);
        self.last_report = Some(report);
        self.expire_holds();
        report
    }

    /// Runs `count` ticks.
    pub fn step_many(&mut self, count: u64) {
        for _ in 0..count {
            self.step();
        }
    }

    /// Releases momentary presses whose hold has run out.
    fn expire_holds(&mut self) {
        for index in 0..BUTTONS {
            if self.button_hold[index] == 0 {
                continue;
            }
            self.button_hold[index] -= 1;
            if self.button_hold[index] == 0 {
                if let Some(button) = Button::from_index(index) {
                    self.apply(Event::Button {
                        button,
                        down: false,
                    });
                }
            }
        }

        for index in 0..TRIGGER_CHANNELS {
            if self.trigger_hold[index] == 0 {
                continue;
            }
            self.trigger_hold[index] -= 1;
            if self.trigger_hold[index] == 0 {
                self.apply(Event::Trigger {
                    channel: index,
                    high: false,
                });
            }
        }
    }

    /// Replays a scenario from the current state and returns the last report.
    ///
    /// Momentary holds are not used here: a scenario states every level change
    /// explicitly, which is what makes a replay reproducible.
    pub fn replay(&mut self, scenario: &Scenario) -> Option<TickReport> {
        let start = self.tick;
        for offset in 0..scenario.ticks {
            for event in scenario.events_at(offset).collect::<Vec<_>>() {
                self.apply(event);
            }
            self.step();
        }
        debug_assert_eq!(self.tick, start + scenario.ticks);
        self.last_report
    }

    /// Number of ticks run so far.
    #[must_use]
    pub const fn tick_count(&self) -> u64 {
        self.tick
    }

    /// Report of the most recent tick, if any.
    #[must_use]
    pub const fn last_report(&self) -> Option<&TickReport> {
        self.last_report.as_ref()
    }

    /// The applet's observable state.
    #[must_use]
    pub const fn app(&self) -> &DiagnosticApp {
        self.engine.app()
    }

    /// The module's screen.
    pub fn frame(&mut self) -> &FrameBuffer {
        self.engine.frame()
    }

    /// Level currently applied to a CV input.
    #[must_use]
    pub fn cv_in(&mut self, channel: CvChannel) -> MilliVolts {
        let (analog_in, ..) = self.engine.parts_mut();
        analog_in.levels[channel.index()]
    }

    /// Whether a cable is reported on a CV input.
    #[must_use]
    pub fn is_patched(&mut self, channel: CvChannel) -> bool {
        let (analog_in, ..) = self.engine.parts_mut();
        analog_in.patched[channel.index()]
    }

    /// Raw level currently applied to a trigger input.
    #[must_use]
    pub fn trigger_in(&mut self, channel: TriggerChannel) -> bool {
        let (_, _, digital_in, ..) = self.engine.parts_mut();
        digital_in.triggers[channel.index()]
    }

    /// Whether a button is currently held down.
    #[must_use]
    pub fn button_held(&mut self, button: Button) -> bool {
        let (_, _, _, controls, _) = self.engine.parts_mut();
        controls.pending.button_down[button.index()]
    }

    /// Levels currently driven on the CV outputs.
    #[must_use]
    pub fn cv_out(&mut self) -> [MilliVolts; CV_CHANNELS] {
        let (_, analog_out, ..) = self.engine.parts_mut();
        analog_out.committed
    }
}

impl Default for Simulator {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use oc_core::platform::{Button, CvChannel, TriggerChannel};

    use super::{PRESS_TICKS, Simulator};
    use crate::scenario::{Event, Scenario};

    #[test]
    fn a_pulse_is_counted_as_one_trigger_event() {
        let mut simulator = Simulator::new();
        simulator.skip_splash();
        simulator.pulse(TriggerChannel::One);
        simulator.step_many(u64::from(PRESS_TICKS) + 4);

        assert_eq!(simulator.app().trigger_count(TriggerChannel::One), 1);
        assert!(
            !simulator.trigger_in(TriggerChannel::One),
            "the pulse must be released again"
        );
    }

    #[test]
    fn two_pulses_are_counted_separately() {
        let mut simulator = Simulator::new();
        simulator.skip_splash();
        for _ in 0..2 {
            simulator.pulse(TriggerChannel::Three);
            simulator.step_many(u64::from(PRESS_TICKS) + 4);
        }
        assert_eq!(simulator.app().trigger_count(TriggerChannel::Three), 2);
    }

    #[test]
    fn a_press_survives_debouncing_and_is_released() {
        let mut simulator = Simulator::new();
        simulator.skip_splash();
        simulator.apply(Event::Encoder {
            index: 1,
            detents: 5,
        });
        simulator.step();
        assert_ne!(simulator.app().offset(), 0);

        simulator.press(Button::RightEncoder);
        simulator.step_many(u64::from(PRESS_TICKS) + 2);
        assert_eq!(
            simulator.app().offset(),
            0,
            "the press must have registered"
        );
    }

    #[test]
    fn a_cv_level_reaches_the_matching_output() {
        let mut simulator = Simulator::new();
        simulator.skip_splash();
        simulator.apply(Event::Cv {
            channel: 1,
            millivolts: -1_500,
        });
        simulator.step();

        assert_eq!(simulator.cv_in(CvChannel::Two), -1_500);
        assert_eq!(simulator.cv_out()[1], -1_500);
    }

    #[test]
    fn out_of_range_indices_are_ignored() {
        let mut simulator = Simulator::new();
        simulator.skip_splash();
        simulator.apply(Event::Cv {
            channel: 99,
            millivolts: 1_000,
        });
        simulator.apply(Event::Trigger {
            channel: 99,
            high: true,
        });
        simulator.apply(Event::Encoder {
            index: 99,
            detents: 5,
        });
        simulator.step();

        assert_eq!(simulator.cv_out(), [0; 4]);
        assert_eq!(simulator.app().offset(), 0);
    }

    #[test]
    fn replaying_the_same_scenario_twice_gives_the_same_screen() {
        let text = "\
ticks 40
0 cv 1 2500
0 patch 1 on
5 trigger 2 high
9 trigger 2 low
12 encoder 2 +4
20 button up down
26 button up up
";
        let scenario: Scenario = text.parse().unwrap();

        let run = || {
            let mut simulator = Simulator::new();
            simulator.skip_splash();
            simulator.replay(&scenario);
            (simulator.frame().clone(), simulator.cv_out())
        };

        assert_eq!(run(), run());
    }

    #[test]
    fn recording_captures_what_was_applied() {
        let mut simulator = Simulator::new();
        simulator.skip_splash();
        simulator.start_recording();
        assert!(simulator.is_recording());

        simulator.apply(Event::Cv {
            channel: 0,
            millivolts: 1_000,
        });
        simulator.step_many(3);
        simulator.pulse(TriggerChannel::One);
        simulator.step_many(u64::from(PRESS_TICKS) + 1);

        let recorded = simulator.stop_recording().expect("recording was started");
        assert!(!simulator.is_recording());
        assert!(recorded.ticks >= simulator.tick_count());
        assert!(
            recorded.events.iter().any(|&(_, event)| matches!(
                event,
                Event::Trigger {
                    channel: 0,
                    high: true
                }
            )),
            "the pulse must appear in the recording: {recorded}"
        );
        assert!(
            recorded.events.iter().any(|&(_, event)| matches!(
                event,
                Event::Trigger {
                    channel: 0,
                    high: false
                }
            )),
            "and so must its release: {recorded}"
        );
    }

    #[test]
    fn a_recording_replays_to_the_same_state() {
        let mut original = Simulator::new();
        original.skip_splash();
        original.start_recording();
        original.apply(Event::Cv {
            channel: 2,
            millivolts: -800,
        });
        original.step_many(5);
        original.press(Button::Up);
        original.step_many(20);
        let scenario = original.stop_recording().unwrap();

        let mut replayed = Simulator::new();
        replayed.skip_splash();
        replayed.replay(&scenario);

        assert_eq!(replayed.cv_out(), original.cv_out());
        assert_eq!(replayed.app().mode(), original.app().mode());
    }

    #[test]
    fn the_reported_cycle_time_is_plausible() {
        let mut simulator = Simulator::new();
        simulator.skip_splash();
        let report = simulator.step();
        assert!(
            report.duration_micros > 0 && report.duration_micros < 1_000,
            "a simulated tick must fit in the 1 kHz budget, got {}",
            report.duration_micros
        );
    }

    #[test]
    fn a_fresh_simulator_boots_into_the_splash_screen() {
        let mut simulator = Simulator::new();
        simulator.apply(Event::Cv {
            channel: 1,
            millivolts: 4_000,
        });
        simulator.step();

        assert_eq!(
            simulator.cv_out(),
            [0; 4],
            "outputs stay at rest during the boot animation"
        );
        assert!(
            simulator.frame().lit_pixels() > 0,
            "the banner must be visible on the very first frame"
        );
    }

    #[test]
    fn resetting_replays_the_splash_screen_and_clears_the_applet() {
        let mut simulator = Simulator::new();
        simulator.skip_splash();
        simulator.apply(Event::Encoder {
            index: 1,
            detents: 5,
        });
        simulator.step();
        assert_ne!(simulator.app().offset(), 0);

        simulator.reset();
        assert_eq!(
            simulator.app().offset(),
            0,
            "reset restores the default applet state"
        );

        let report = simulator.step();
        assert_eq!(
            report.cv_out, [0; 4],
            "a reset module boots again before resuming normal execution"
        );
    }
}
