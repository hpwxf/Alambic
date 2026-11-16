//! In-memory platform implementations for tests, benchmarks and host tools.
//!
//! These are deliberately part of the public API rather than hidden behind
//! `#[cfg(test)]`: the simulator, the benchmarks and the integration tests all
//! need the same fixtures, and a backend author needs a reference
//! implementation of the platform traits to compare against.
//!
//! Everything here is deterministic. Given the same sequence of inputs, the
//! resulting CV outputs and framebuffer are bit-for-bit identical.

use core::cell::Cell;

use crate::calibration::{CvInputCalibration, CvOutputCalibration};
use crate::engine::Engine;
use crate::framebuffer::FrameBuffer;
use crate::platform::{
    AnalogIn, AnalogOut, CV_CHANNELS, Clock, ControlEvents, Controls, CvChannel, DigitalIn,
    Display, MilliVolts, TRIGGER_CHANNELS, TriggerChannel,
};

/// CV inputs backed by a plain array of levels.
///
/// The levels are what the jacks see; [`AnalogIn::read_code`] turns them into
/// the codes the module's ADC would return with the factory calibration, so
/// the engine's conversion runs exactly as on the hardware, quantisation
/// included.
#[derive(Debug, Clone, Copy, Default)]
pub struct MockAnalogIn {
    /// Level at each input jack.
    pub levels: [MilliVolts; CV_CHANNELS],
}

impl MockAnalogIn {
    /// All channels at 0 V, as unpatched jacks read.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            levels: [0; CV_CHANNELS],
        }
    }

    /// Drives one channel to a level, as a patched cable would.
    pub const fn patch(&mut self, channel: CvChannel, millivolts: MilliVolts) {
        self.levels[channel.index()] = millivolts;
    }
}

/// The level the engine reads for `millivolts` at an input jack: the factory
/// ADC quantises it to one of its 4096 codes (about 2.44 mV apart).
///
/// Tests that patch an input and check where the level ends up compare
/// against this rather than against the exact value they patched.
#[must_use]
pub const fn adc_reading(millivolts: MilliVolts) -> MilliVolts {
    let factory = CvInputCalibration::FACTORY;
    factory.to_millivolts(factory.to_code(millivolts))
}

impl AnalogIn for MockAnalogIn {
    fn read_code(&mut self, channel: CvChannel) -> u16 {
        CvInputCalibration::FACTORY.to_code(self.levels[channel.index()])
    }
}

/// CV outputs that record what was written to them.
///
/// The engine writes DAC codes; the mock reads them back through the factory
/// calibration, as the module's output stage would turn them into volts.
#[derive(Debug, Clone, Copy)]
pub struct MockAnalogOut {
    staged: [u16; CV_CHANNELS],
    /// DAC codes as of the last flush.
    pub codes: [u16; CV_CHANNELS],
    /// Levels as of the last flush; this is what the jacks would show.
    pub committed: [MilliVolts; CV_CHANNELS],
    /// Number of flushes performed.
    pub flushes: u32,
}

impl MockAnalogOut {
    /// All channels at 0 V, never flushed.
    #[must_use]
    pub const fn new() -> Self {
        let rest = CvOutputCalibration::FACTORY.to_code(0);
        Self {
            staged: [rest; CV_CHANNELS],
            codes: [rest; CV_CHANNELS],
            committed: [0; CV_CHANNELS],
            flushes: 0,
        }
    }

    /// Committed level of one channel.
    #[must_use]
    pub const fn level(&self, channel: CvChannel) -> MilliVolts {
        self.committed[channel.index()]
    }
}

impl Default for MockAnalogOut {
    fn default() -> Self {
        Self::new()
    }
}

impl AnalogOut for MockAnalogOut {
    fn write_code(&mut self, channel: CvChannel, code: u16) {
        self.staged[channel.index()] = code;
    }

    fn flush(&mut self) {
        self.codes = self.staged;
        for (level, &code) in self.committed.iter_mut().zip(&self.codes) {
            *level = CvOutputCalibration::FACTORY.to_millivolts(code);
        }
        self.flushes = self.flushes.saturating_add(1);
    }
}

/// Trigger inputs backed by a plain array.
#[derive(Debug, Clone, Copy, Default)]
pub struct MockDigitalIn {
    /// Raw level of each trigger input.
    pub triggers: [bool; TRIGGER_CHANNELS],
}

impl MockDigitalIn {
    /// All triggers low.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            triggers: [false; TRIGGER_CHANNELS],
        }
    }

    /// Sets one trigger's level.
    pub const fn set(&mut self, channel: TriggerChannel, high: bool) {
        self.triggers[channel.index()] = high;
    }
}

impl DigitalIn for MockDigitalIn {
    fn trigger_state(&self, channel: TriggerChannel) -> bool {
        self.triggers[channel.index()]
    }
}

/// Controls whose pending events are set by the test.
///
/// Encoder deltas are consumed by [`Controls::poll`], mirroring real hardware
/// where movement is reported once; button levels persist until changed.
#[derive(Debug, Clone, Copy, Default)]
pub struct MockControls {
    /// Events returned by the next poll.
    pub pending: ControlEvents,
}

impl MockControls {
    /// No movement, no button held.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            pending: ControlEvents {
                encoder_delta: [0; crate::platform::ENCODERS],
                button_down: [false; crate::platform::BUTTONS],
            },
        }
    }

    /// Queues encoder movement, accumulating with anything not yet polled.
    pub const fn turn(&mut self, encoder: usize, detents: i8) {
        if encoder < self.pending.encoder_delta.len() {
            self.pending.encoder_delta[encoder] =
                self.pending.encoder_delta[encoder].saturating_add(detents);
        }
    }

    /// Holds or releases a button.
    pub const fn hold(&mut self, button: crate::platform::Button, down: bool) {
        self.pending.button_down[button.index()] = down;
    }
}

impl Controls for MockControls {
    fn poll(&mut self) -> ControlEvents {
        let events = self.pending;
        self.pending.encoder_delta = [0; crate::platform::ENCODERS];
        events
    }
}

/// A deterministic microsecond clock.
///
/// `read_cost_micros` models the time the module itself takes: every reading
/// advances the clock by that much, so a tick that reads the clock twice
/// reports a non-zero duration without any real time passing.
#[derive(Debug, Clone, Default)]
pub struct VirtualClock {
    now: Cell<u64>,
    read_cost_micros: u64,
}

impl VirtualClock {
    /// A clock starting at zero that does not advance on its own.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            now: Cell::new(0),
            read_cost_micros: 0,
        }
    }

    /// A clock that advances by `micros` on every reading.
    #[must_use]
    pub const fn with_read_cost(micros: u64) -> Self {
        Self {
            now: Cell::new(0),
            read_cost_micros: micros,
        }
    }

    /// Moves the clock forward.
    pub fn advance(&self, micros: u64) {
        self.now.set(self.now.get().wrapping_add(micros));
    }

    /// Forces the clock to an absolute value, to exercise wrap-around.
    pub fn set(&self, micros: u64) {
        self.now.set(micros);
    }
}

impl Clock for VirtualClock {
    fn now_micros(&self) -> u64 {
        let now = self.now.get();
        self.now.set(now.wrapping_add(self.read_cost_micros));
        now
    }
}

/// A screen that keeps the framebuffer and counts presentations.
#[derive(Debug, Clone, Default)]
pub struct MockDisplay {
    frame: FrameBuffer,
    /// Number of times the frame was presented.
    pub presents: u32,
}

impl MockDisplay {
    /// A dark screen, never presented.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            frame: FrameBuffer::new(),
            presents: 0,
        }
    }

    /// The framebuffer as last drawn.
    #[must_use]
    pub const fn frame(&self) -> &FrameBuffer {
        &self.frame
    }
}

impl Display for MockDisplay {
    fn frame_mut(&mut self) -> &mut FrameBuffer {
        &mut self.frame
    }

    fn present(&mut self) {
        self.presents = self.presents.saturating_add(1);
    }
}

/// An [`Engine`] wired to the mock platform.
pub type MockEngine =
    Engine<MockAnalogIn, MockAnalogOut, MockDigitalIn, MockControls, VirtualClock, MockDisplay>;

/// Builds an engine over the mock platform, past its boot splash screen,
/// with a clock that charges `cost` microseconds per reading.
///
/// A real [`Engine::new`] always starts with the boot animation, which is
/// what [`mock_engine_at_boot`] gives a test that actually wants to exercise
/// it. Every other test in this workspace is about the diagnostic applet's
/// steady-state behaviour, not about the few seconds of start-up before it,
/// so this convenience calls [`Engine::skip_splash`] before handing the
/// engine back.
#[must_use]
pub fn mock_engine(clock_read_cost_micros: u64) -> MockEngine {
    let mut engine = mock_engine_at_boot(clock_read_cost_micros);
    engine.skip_splash();
    engine
}

/// Builds a genuinely fresh engine over the mock platform, boot splash
/// screen included, with a clock that charges `cost` microseconds per
/// reading.
#[must_use]
pub fn mock_engine_at_boot(clock_read_cost_micros: u64) -> MockEngine {
    Engine::new(
        MockAnalogIn::new(),
        MockAnalogOut::new(),
        MockDigitalIn::new(),
        MockControls::new(),
        VirtualClock::with_read_cost(clock_read_cost_micros),
        MockDisplay::new(),
    )
}
