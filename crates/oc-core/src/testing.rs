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
use core::ops::{Deref, DerefMut};

use embedded_storage_async::nor_flash::{
    ErrorType, NorFlash, NorFlashError, NorFlashErrorKind, ReadNorFlash,
};

use crate::calibration::{CvInputCalibration, CvOutputCalibration};
use crate::engine::{Engine, TICK_RATE_HZ, TickReport};
use crate::framebuffer::FrameBuffer;
use crate::platform::{
    AnalogIn, AnalogOut, CV_CHANNELS, Clock, ControlEvents, Controls, CvChannel, DigitalIn,
    Display, MilliVolts, TRIGGER_CHANNELS, TriggerChannel,
};
use crate::playback::Player;
use crate::settings::{FLASH_BYTES, SECTOR_BYTES};

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

    /// The virtual time itself, read without advancing it, so profiling a
    /// tick sees the read costs of [`Self::now_micros`] and changes nothing.
    #[allow(clippy::cast_possible_truncation)] // a wrapping counter, by contract
    fn fine_count(&self) -> u32 {
        self.now.get() as u32
    }

    fn fine_per_micro(&self) -> u32 {
        1
    }
}

/// A screen that keeps the framebuffer and counts presentations.
#[derive(Debug, Clone, Default)]
pub struct MockDisplay {
    frame: FrameBuffer,
    /// Number of times the frame was presented.
    pub presents: u32,
    /// Last contrast the engine set, `None` before the first.
    pub contrast: Option<u8>,
    /// Parts the frame is sent in; one unless a test sets it, like the
    /// module's eight pages.
    pub pages: usize,
    /// Pages sent one at a time, in order, oldest first, up to the capacity.
    pub pages_sent: [Option<usize>; 32],
    /// Entries used in [`Self::pages_sent`].
    pub pages_sent_len: usize,
}

impl MockDisplay {
    /// A dark screen, never presented.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            frame: FrameBuffer::new(),
            presents: 0,
            contrast: None,
            pages: 1,
            pages_sent: [None; 32],
            pages_sent_len: 0,
        }
    }

    /// The pages sent one at a time so far, in order.
    #[must_use]
    pub fn pages_sent(&self) -> &[Option<usize>] {
        &self.pages_sent[..self.pages_sent_len]
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

    fn page_count(&self) -> usize {
        self.pages
    }

    fn present_page(&mut self, page: usize) {
        if self.pages == 1 {
            self.present();
        } else if let Some(slot) = self.pages_sent.get_mut(self.pages_sent_len) {
            *slot = Some(page);
            self.pages_sent_len += 1;
        }
    }

    fn set_contrast(&mut self, contrast: u8) {
        self.contrast = Some(contrast);
    }
}

/// An [`Engine`] wired to the mock platform, with no voice: what a backend
/// that plays the outputs itself wraps, as the simulator does.
pub type BareMockEngine =
    Engine<MockAnalogIn, MockAnalogOut, MockDigitalIn, MockControls, VirtualClock, MockDisplay>;

/// An [`Engine`] wired to the mock platform whose outputs play once a tick,
/// as the simulator plays them: the engine renders no voice (journal/D-040).
///
/// It is the engine for every other purpose: it dereferences to it. Its own
/// [`Self::tick`] and [`Self::reset`] stand in front of the engine's, and
/// what the jacks carry reads from [`Self::outputs`]: a test that reached
/// the engine's own tick would see outputs that never move.
#[derive(Debug)]
pub struct MockEngine {
    engine: BareMockEngine,
    player: Player,
    outputs: [MilliVolts; CV_CHANNELS],
}

impl MockEngine {
    /// Wraps `engine`, its outputs at rest until the first tick plays them.
    #[must_use]
    pub fn new(engine: BareMockEngine) -> Self {
        Self {
            engine,
            player: Player::new(TICK_RATE_HZ),
            outputs: [0; CV_CHANNELS],
        }
    }

    /// Runs one tick, then plays one sample of the outputs and writes it
    /// ([`Engine::write_outputs`]), as the simulator does; the engine's
    /// report, whose [`TickReport::cv_out`] is what played before the tick.
    pub fn tick(&mut self) -> TickReport {
        let report = self.engine.tick();
        self.outputs = self.player.play(self.engine.patch());
        self.engine.write_outputs(&self.outputs);
        report
    }

    /// What the outputs played after the last [`Self::tick`], in
    /// millivolts, before the calibration turns them into codes.
    #[must_use]
    pub const fn outputs(&self) -> [MilliVolts; CV_CHANNELS] {
        self.outputs
    }

    /// Restarts the engine ([`Engine::reset`]) and its voice, as Rack does
    /// on its own reset.
    pub fn reset(&mut self) {
        self.engine.reset();
        self.player = Player::new(TICK_RATE_HZ);
        self.outputs = [0; CV_CHANNELS];
    }
}

impl Deref for MockEngine {
    type Target = BareMockEngine;

    fn deref(&self) -> &Self::Target {
        &self.engine
    }
}

impl DerefMut for MockEngine {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.engine
    }
}

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
    MockEngine::new(bare_mock_engine_at_boot(clock_read_cost_micros))
}

/// [`mock_engine_at_boot`] without its voice, for a backend that plays the
/// outputs itself ([`Engine::write_outputs`]).
#[must_use]
pub fn bare_mock_engine_at_boot(clock_read_cost_micros: u64) -> BareMockEngine {
    Engine::new(
        MockAnalogIn::new(),
        MockAnalogOut::new(),
        MockDigitalIn::new(),
        MockControls::new(),
        VirtualClock::with_read_cost(clock_read_cost_micros),
        MockDisplay::new(),
    )
}

/// Size of a [`MemoryFlash`], in bytes: exactly what the settings store uses.
pub const MEMORY_FLASH_BYTES: usize = FLASH_BYTES as usize;

/// A NOR flash held in memory, sized for the settings store.
///
/// It behaves like the real part where it matters: erasing sets whole
/// sectors to `0xFF`, and a write can only clear bits, never set them, so a
/// store that forgot to erase fails here as it would on the module. The
/// simulator mirrors its bytes to a file, and the tests restart a store over
/// the same bytes to model a power cycle.
#[derive(Clone)]
pub struct MemoryFlash {
    bytes: [u8; MEMORY_FLASH_BYTES],
    erases: u32,
}

impl core::fmt::Debug for MemoryFlash {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("MemoryFlash")
            .field("erases", &self.erases)
            .finish_non_exhaustive()
    }
}

/// What a [`MemoryFlash`] refuses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MemoryFlashError {
    /// The access falls outside the flash.
    OutOfBounds,
    /// The access is not aligned to the operation's granularity.
    NotAligned,
    /// The write would need to set a bit that is not erased.
    NotErased,
}

impl NorFlashError for MemoryFlashError {
    fn kind(&self) -> NorFlashErrorKind {
        match self {
            Self::OutOfBounds => NorFlashErrorKind::OutOfBounds,
            Self::NotAligned => NorFlashErrorKind::NotAligned,
            Self::NotErased => NorFlashErrorKind::Other,
        }
    }
}

impl MemoryFlash {
    /// A blank flash: every byte erased.
    #[must_use]
    pub const fn erased() -> Self {
        Self {
            bytes: [0xFF; MEMORY_FLASH_BYTES],
            erases: 0,
        }
    }

    /// A flash holding a previously captured image.
    #[must_use]
    pub const fn from_image(bytes: [u8; MEMORY_FLASH_BYTES]) -> Self {
        Self { bytes, erases: 0 }
    }

    /// The flash contents, to save or to restart a store over.
    #[must_use]
    pub const fn as_bytes(&self) -> &[u8; MEMORY_FLASH_BYTES] {
        &self.bytes
    }

    /// Number of sector erases performed, a measure of wear.
    #[must_use]
    pub const fn erase_count(&self) -> u32 {
        self.erases
    }

    /// The byte range of an access, if it lies inside the flash.
    fn span(offset: u32, length: usize) -> Result<core::ops::Range<usize>, MemoryFlashError> {
        let start = usize::try_from(offset).map_err(|_| MemoryFlashError::OutOfBounds)?;
        let end = start
            .checked_add(length)
            .filter(|&end| end <= MEMORY_FLASH_BYTES)
            .ok_or(MemoryFlashError::OutOfBounds)?;
        Ok(start..end)
    }
}

impl Default for MemoryFlash {
    fn default() -> Self {
        Self::erased()
    }
}

impl ErrorType for MemoryFlash {
    type Error = MemoryFlashError;
}

/// The flash operations themselves, synchronous: a [`MemoryFlash`] never has
/// to wait, so its trait methods return futures that are already complete.
impl MemoryFlash {
    fn read_now(&self, offset: u32, bytes: &mut [u8]) -> Result<(), MemoryFlashError> {
        let span = Self::span(offset, bytes.len())?;
        bytes.copy_from_slice(&self.bytes[span]);
        Ok(())
    }

    fn erase_now(&mut self, from: u32, to: u32) -> Result<(), MemoryFlashError> {
        if !from.is_multiple_of(SECTOR_BYTES) || !to.is_multiple_of(SECTOR_BYTES) || from > to {
            return Err(MemoryFlashError::NotAligned);
        }
        let length = usize::try_from(to - from).map_err(|_| MemoryFlashError::OutOfBounds)?;
        let span = Self::span(from, length)?;
        self.bytes[span].fill(0xFF);
        self.erases = self.erases.saturating_add((to - from) / SECTOR_BYTES);
        Ok(())
    }

    fn write_now(&mut self, offset: u32, bytes: &[u8]) -> Result<(), MemoryFlashError> {
        let aligned = usize::try_from(offset).is_ok_and(|offset| {
            offset.is_multiple_of(<Self as NorFlash>::WRITE_SIZE)
                && bytes.len().is_multiple_of(<Self as NorFlash>::WRITE_SIZE)
        });
        if !aligned {
            return Err(MemoryFlashError::NotAligned);
        }
        let span = Self::span(offset, bytes.len())?;
        let target = &mut self.bytes[span];
        if target.iter().zip(bytes).any(|(&old, &new)| new & !old != 0) {
            return Err(MemoryFlashError::NotErased);
        }
        for (old, &new) in target.iter_mut().zip(bytes) {
            *old &= new;
        }
        Ok(())
    }
}

impl ReadNorFlash for MemoryFlash {
    const READ_SIZE: usize = 1;

    fn read(
        &mut self,
        offset: u32,
        bytes: &mut [u8],
    ) -> impl Future<Output = Result<(), Self::Error>> {
        core::future::ready(self.read_now(offset, bytes))
    }

    fn capacity(&self) -> usize {
        MEMORY_FLASH_BYTES
    }
}

impl NorFlash for MemoryFlash {
    const WRITE_SIZE: usize = 4;
    const ERASE_SIZE: usize = SECTOR_BYTES as usize;

    fn erase(&mut self, from: u32, to: u32) -> impl Future<Output = Result<(), Self::Error>> {
        core::future::ready(self.erase_now(from, to))
    }

    fn write(
        &mut self,
        offset: u32,
        bytes: &[u8],
    ) -> impl Future<Output = Result<(), Self::Error>> {
        core::future::ready(self.write_now(offset, bytes))
    }
}
