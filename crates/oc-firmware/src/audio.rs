//! The audio path (journal/D-033, lots 7c and 7e): the DAC is written by DMA
//! at instants the PIT fixes, an interrupt computes the samples and sends
//! the screen between two DAC bursts; the 1 kHz loop only leaves them what
//! to play and what to show.
//!
//! # One period, 21 PERCLK counts
//!
//! 1. PIT channel 0 times out: the DMA copies one slot of the ring
//!    ([`crate::audio_irq`]) into LPSPI4, four 24-bit words on PCS0, about
//!    5.5 µs on the bus. The instant does not depend on any software.
//! 2. The copy done (the words queued, not yet sent), the DMA raises its
//!    interrupt: it counts the bursts by their echo in the receive FIFO,
//!    which says where the DMA is in the ring; takes what the loop left;
//!    computes the next sample and writes it two slots ahead.
//! 3. It waits for the burst's last echo (the words go out meanwhile), then,
//!    if enough of the period is left, sets D/C, lowers the OLED's chip
//!    select and queues a piece of the screen on PCS3, its echo masked, and
//!    returns without waiting for it.
//! 4. LPSPI4's frame-complete interrupt, when that piece's last bit is out,
//!    releases the OLED's chip select before the next burst.
//!
//! # The loop's side
//!
//! The engine keeps its output and display traits: [`MutedOutputs`] drops
//! its codes, [`MailboxDisplay`] hands its frames over; after each tick the
//! loop's [`Publisher`] hands on [`oc_core::Engine::params`] when its
//! revision moved, and prepares the voice's [`Setup`] there, outside any critical section: the
//! interrupt only copies it (journal/D-040). Everything the
//! contexts share sits in [`critical_section::Mutex`]es: the loop's sections
//! are copies of a few hundred bytes (a frame: 1 KiB). They delay the
//! interrupts a little, never the DAC.
//!
//! The DMA's programming, the ring and the handlers, the only `unsafe` of
//! this path, are in [`crate::audio_irq`].

use core::cell::RefCell;

use cortex_m::peripheral::DWT;
use critical_section::Mutex;

use teensy4_bsp::hal::dma::channel::Channel;
use teensy4_bsp::hal::gpio::Output;
use teensy4_bsp::hal::lpspi::{ClockConfigs, Interrupts, Lpspi, Status, Transaction};
use teensy4_bsp::hal::pit::{self, Pit};
use teensy4_bsp::ral::lpspi::TCR;

use oc_core::apps::sound::{Params, Setup, Voice};
use oc_core::calibration::CvOutputCalibration;
use oc_core::framebuffer::{FrameBuffer, LEN};
use oc_core::platform::{AnalogOut, CV_CHANNELS, CvChannel, Display};
use oc_core::profile::{Stage, Stat};
use oc_drivers::dac8565::channel_word;
use oc_drivers::fifo;
use oc_drivers::ssd130x::{ChunkKind, Controller, FrameStream};

use crate::audio_irq::{SLOT_WORDS, SLOTS};
use crate::board::{self, BusClocks};
use crate::delay::CORE_CLOCK_HZ;

/// The PIT channel that triggers the DMA ([`board::AUDIO_DMA_CHANNEL`]).
const DAC_TIMER: pit::Channel = pit::Channel::Chan0;

/// The command the DMA queues before each DAC word: a 24-bit frame on
/// PCS0, mode 0, most significant bit first, the echo kept (it counts the
/// bursts). `imxrt-hal`'s `Transaction` has no raw value to put in a buffer.
const DAC_TCR: u32 = (23 << TCR::FRAMESZ::offset) | ((board::DAC_PCS as u32) << TCR::PCS::offset);

/// What the loop leaves for the interrupt, and what the interrupt measured.
struct Mailbox {
    /// The voice prepared for the engine's last parameters.
    setup: Setup,
    calibration: [CvOutputCalibration; CV_CHANNELS],
    /// Set when `setup` or `calibration` changed since the interrupt took
    /// them.
    fresh: bool,
    frame: [u8; LEN],
    /// Set when `frame` holds a frame the interrupt has not taken.
    frame_ready: bool,
    contrast: Option<u8>,
    /// Runs of the interrupt, wrapping.
    periods: u32,
    /// DAC bursts the interrupt has seen, wrapping.
    bursts: u32,
    /// Screen chunks sent, and OLED releases after them, wrapping.
    chunks: (u32, u32),
    /// Whole frames sent to the screen, wrapping.
    frames: u32,
    stats: Stats,
}

/// The interrupt's own measurements, in core cycles, until the loop takes
/// them into the profile.
#[derive(Clone, Copy)]
struct Stats {
    audio: Stat,
    voice: Stat,
    voice_params: Stat,
    period: Stat,
    frame: Stat,
}

impl Stats {
    const fn new() -> Self {
        Self {
            audio: Stat::new(),
            voice: Stat::new(),
            voice_params: Stat::new(),
            period: Stat::new(),
            frame: Stat::new(),
        }
    }
}

static MAILBOX: Mutex<RefCell<Mailbox>> = Mutex::new(RefCell::new(Mailbox {
    setup: Setup::rest(board::AUDIO_RATE_HZ),
    calibration: [CvOutputCalibration::FACTORY; CV_CHANNELS],
    fresh: false,
    frame: [0; LEN],
    frame_ready: false,
    contrast: None,
    periods: 0,
    bursts: 0,
    chunks: (0, 0),
    frames: 0,
    stats: Stats::new(),
}));

/// What the interrupts own, from [`start`] on.
static STATE: Mutex<RefCell<Option<AudioState>>> = Mutex::new(RefCell::new(None));

/// The interrupts' hardware and their signal state.
struct AudioState {
    spi: Lpspi,
    oled_cs: Output,
    oled_dc: Output,
    pit: Pit,
    dma: Channel,
    clocks: BusClocks,
    /// The timing in force on the bus.
    current_clock: ClockConfigs,
    stream: FrameStream,
    voice: Voice,
    calibration: [CvOutputCalibration; CV_CHANNELS],
    /// The ring slot the DMA reads at its next trigger.
    dma_next: usize,
    /// Whether a piece of the screen is on the bus, the OLED selected.
    chunk_active: bool,
    /// Cycle count at the previous run, for the period.
    last_entry: Option<u32>,
    /// Cycle count when the frame going out was taken.
    frame_started: Option<u32>,
    /// The cycle count of the earliest run seen, carried to the current
    /// period: the closest to the DAC's trigger, which the interrupt follows
    /// by a constant. PERCLK and the core both come from the crystal, so the
    /// period is a whole number of cycles; the PIT itself is not read, every
    /// access to it waits on its 1 MHz domain for microseconds.
    reference: Option<u32>,
    /// DAC bursts whose four echoes came back, wrapping.
    completed: usize,
}

/// Core cycles in an audio period.
const PERIOD_CYCLES: u32 = CORE_CLOCK_HZ / board::AUDIO_TIMER_HZ * board::AUDIO_PERIOD_COUNTS;

/// Core cycles in a microsecond.
const CYCLES_PER_MICRO: u32 = CORE_CLOCK_HZ / 1_000_000;

/// The ring slot for `codes`.
fn slot_words(codes: &[u16; CV_CHANNELS]) -> [u32; SLOT_WORDS] {
    let mut words = [0; SLOT_WORDS];
    for channel in CvChannel::ALL {
        let index = channel.index();
        words[2 * index] = DAC_TCR;
        words[2 * index + 1] = channel_word(channel, codes[index]);
    }
    words
}

/// Takes the bus, the OLED's control lines, the PIT and a DMA channel over,
/// and starts the audio path at [`board::AUDIO_RATE_HZ`]. The OLED must be
/// initialised; the outputs rest at 0 V (factory calibration) until the
/// first [`Publisher::publish`].
#[allow(clippy::too_many_arguments)]
// The hardware the path takes over, each once, at start-up.
pub(crate) fn start(
    mut spi: Lpspi,
    oled_cs: Output,
    oled_dc: Output,
    mut pit: Pit,
    mut dma: Channel,
    clocks: BusClocks,
    controller: Controller,
) {
    let rest = slot_words(&[CvOutputCalibration::FACTORY.to_code(0); CV_CHANNELS]);
    for slot in 0..SLOTS {
        crate::audio_irq::write_slot(slot, &rest);
    }
    pit.disable(DAC_TIMER);
    pit.set_load_timer_value(DAC_TIMER, board::AUDIO_PERIOD_COUNTS);
    pit.clear_elapsed(DAC_TIMER);
    spi.clear_fifos();
    spi.disabled(|spi| spi.set_clock_configs(clocks.dac));
    oled_cs.set();
    crate::audio_irq::start_dma(&mut dma, spi.tdr().cast::<u32>());

    let state = AudioState {
        spi,
        oled_cs,
        oled_dc,
        pit,
        dma,
        clocks,
        current_clock: clocks.dac,
        stream: FrameStream::new(controller),
        voice: Voice::new(board::AUDIO_RATE_HZ),
        calibration: [CvOutputCalibration::FACTORY; CV_CHANNELS],
        dma_next: 0,
        chunk_active: false,
        last_entry: None,
        frame_started: None,
        reference: None,
        completed: 0,
    };
    critical_section::with(|cs| {
        let mut slot = STATE.borrow_ref_mut(cs);
        slot.insert(state).pit.enable(DAC_TIMER);
    });
    crate::audio_irq::enable();
}

/// What the audio path has done since start-up, each count wrapping.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct Totals {
    /// Runs of the interrupt.
    pub(crate) periods: u32,
    /// DAC bursts the DMA sent, counted by their echo.
    pub(crate) bursts: u32,
    /// Screen chunks sent.
    pub(crate) chunks: u32,
    /// OLED releases after a chunk.
    pub(crate) releases: u32,
    /// Whole frames sent to the screen.
    pub(crate) frames: u32,
}

impl Totals {
    /// What happened between `earlier` and `self`.
    pub(crate) const fn since(self, earlier: Self) -> Self {
        Self {
            periods: self.periods.wrapping_sub(earlier.periods),
            bursts: self.bursts.wrapping_sub(earlier.bursts),
            chunks: self.chunks.wrapping_sub(earlier.chunks),
            releases: self.releases.wrapping_sub(earlier.releases),
            frames: self.frames.wrapping_sub(earlier.frames),
        }
    }
}

/// The audio path's counts so far.
pub(crate) fn totals() -> Totals {
    critical_section::with(|cs| {
        let mail = MAILBOX.borrow_ref(cs);
        Totals {
            periods: mail.periods,
            bursts: mail.bursts,
            chunks: mail.chunks.0,
            releases: mail.chunks.1,
            frames: mail.frames,
        }
    })
}

/// What [`self_check`] saw of the audio path.
#[derive(Debug)]
pub(crate) struct Health {
    /// How long the check lasted, on the cycle counter, in microseconds.
    pub(crate) micros: u32,
    /// What happened during it.
    pub(crate) counts: Totals,
    /// The interrupt's duration, mean and longest, in microseconds.
    pub(crate) audio_micros: (u32, u32),
    /// The time between two runs, shortest and longest, in microseconds.
    pub(crate) period_micros: (u32, u32),
    /// Whether the period's NVIC line is enabled, and whether it is pending.
    pub(crate) line: (bool, bool),
    /// The DMA channel's error flag and the controller's error status
    /// register, should it fail to read the ring or write LPSPI4.
    pub(crate) dma_error: (bool, u32),
}

impl Health {
    /// `count` events during the check, as a rate a second.
    pub(crate) const fn per_second(&self, count: u32) -> u64 {
        if self.micros == 0 {
            return 0;
        }
        count as u64 * 1_000_000 / self.micros as u64
    }
}

/// Watches the audio path for `micros` microseconds, timed on the cycle
/// counter (a busy delay runs short on the M7), for the boot log: a silent
/// DMA or interrupt shows on USB rather than as a black screen, and the
/// rates and durations give a baseline to compare a later log with.
pub(crate) fn self_check(micros: u32) -> Health {
    critical_section::with(|cs| MAILBOX.borrow_ref_mut(cs).stats = Stats::new());
    let before = totals();
    let start = DWT::cycle_count();
    while DWT::cycle_count().wrapping_sub(start) < micros.saturating_mul(CYCLES_PER_MICRO) {
        core::hint::spin_loop();
    }
    let elapsed = DWT::cycle_count().wrapping_sub(start) / CYCLES_PER_MICRO;
    let counts = totals().since(before);
    let stats = critical_section::with(|cs| MAILBOX.borrow_ref(cs).stats);
    let in_micros = |cycles: Option<u32>| cycles.map_or(0, |cycles| cycles / CYCLES_PER_MICRO);
    let dma_error = critical_section::with(|cs| {
        STATE.borrow_ref(cs).as_ref().map_or((false, 0), |state| {
            (state.dma.is_error(), state.dma.error_status().raw())
        })
    });
    Health {
        micros: elapsed,
        counts,
        audio_micros: (in_micros(stats.audio.mean()), in_micros(stats.audio.max())),
        period_micros: (in_micros(stats.period.min()), in_micros(stats.period.max())),
        line: crate::audio_irq::line_state(),
        dma_error,
    }
}

/// Stops the DMA and the interrupts, the outputs left where they are:
/// before handing the chip to the bootloader.
pub(crate) fn stop() {
    crate::audio_irq::disable();
    critical_section::with(|cs| {
        if let Some(state) = STATE.borrow_ref_mut(cs).as_mut() {
            state.dma.disable();
        }
    });
}

/// What the loop last handed the interrupt, so that a tick where nothing
/// changed costs a comparison of a revision and of the calibration, outside
/// any critical section: comparing the whole [`Params`] under one cost
/// 1.5 µs a tick. Owned by the loop, the only writer of the mailbox.
#[derive(Debug)]
pub(crate) struct Publisher {
    /// [`oc_core::Engine::params_revision`] of the parameters last prepared;
    /// `None` when the mailbox holds something else (the outputs at rest).
    revision: Option<u32>,
    calibration: Option<[CvOutputCalibration; CV_CHANNELS]>,
}

impl Publisher {
    /// Nothing handed yet: the first [`Self::publish`] copies everything.
    pub(crate) const fn new() -> Self {
        Self {
            revision: None,
            calibration: None,
        }
    }

    /// Hands the interrupt what the outputs carry and how to convert it,
    /// from the next period on; copies only when something changed. New
    /// parameters are prepared here, outside the critical section, so the
    /// interrupt never waits for their divisions (journal/D-040).
    pub(crate) fn publish(
        &mut self,
        revision: u32,
        params: &Params,
        calibration: &[CvOutputCalibration; CV_CHANNELS],
    ) {
        let new_params = self.revision != Some(revision);
        let new_calibration = self.calibration.as_ref() != Some(calibration);
        if !new_params && !new_calibration {
            return;
        }
        let setup = new_params.then(|| Setup::new(params, board::AUDIO_RATE_HZ));
        hand_over(setup.as_ref(), calibration);
        self.revision = Some(revision);
        self.calibration = Some(*calibration);
    }

    /// Puts every output at 0 V, waits until the DAC has it, then pauses the
    /// DMA: before a flash operation, during which the boot ROM masks the
    /// interrupts (journal/D-033), and the bursts would pile up in the FIFO.
    /// [`resume_outputs`] restarts it; the next [`Self::publish`] brings the
    /// parameters back.
    pub(crate) fn rest_outputs(&mut self, calibration: &[CvOutputCalibration; CV_CHANNELS]) {
        hand_over(Some(&Setup::rest(board::AUDIO_RATE_HZ)), calibration);
        self.revision = None;
        self.calibration = Some(*calibration);
        let running = critical_section::with(|cs| STATE.borrow_ref(cs).is_some());
        if !running {
            return;
        }
        // Taken in one period, written two slots ahead, sent two later: five
        // periods leave one to spare.
        let start = periods();
        while periods().wrapping_sub(start) < 5 {
            cortex_m::asm::nop();
        }
        critical_section::with(|cs| {
            if let Some(state) = STATE.borrow_ref_mut(cs).as_mut() {
                state.dma.disable();
            }
        });
    }
}

/// Puts `setup`, when there is one, and `calibration` in the mailbox for the
/// interrupt to take at its next period.
fn hand_over(setup: Option<&Setup>, calibration: &[CvOutputCalibration; CV_CHANNELS]) {
    critical_section::with(|cs| {
        let mut mail = MAILBOX.borrow_ref_mut(cs);
        if let Some(setup) = setup {
            mail.setup = *setup;
        }
        mail.calibration = *calibration;
        mail.fresh = true;
    });
}

/// Restarts the DMA paused by [`rest_outputs`], where it stopped in the
/// ring.
pub(crate) fn resume_outputs() {
    critical_section::with(|cs| {
        if let Some(state) = STATE.borrow_ref_mut(cs).as_mut() {
            crate::audio_irq::resume_dma(&state.dma);
        }
    });
}

/// Runs of the interrupt so far, wrapping.
fn periods() -> u32 {
    critical_section::with(|cs| MAILBOX.borrow_ref(cs).periods)
}

/// Moves what the interrupt measured into `profile`.
pub(crate) fn drain_stats(profile: &mut oc_core::profile::Profile) {
    let stats = critical_section::with(|cs| {
        let mut mail = MAILBOX.borrow_ref_mut(cs);
        core::mem::replace(&mut mail.stats, Stats::new())
    });
    profile.merge(Stage::Audio, &stats.audio);
    profile.merge(Stage::Voice, &stats.voice);
    profile.merge(Stage::VoiceParams, &stats.voice_params);
    profile.merge(Stage::AudioPeriod, &stats.period);
    profile.merge(Stage::Frame, &stats.frame);
}

/// One run of the period interrupt; called by its handler.
pub(crate) fn on_period() {
    let entry = DWT::cycle_count();
    critical_section::with(|cs| {
        let mut state = STATE.borrow_ref_mut(cs);
        let Some(state) = state.as_mut() else {
            return;
        };
        let mut mail = MAILBOX.borrow_ref_mut(cs);
        state.run(&mut mail, entry);
    });
}

/// A piece of the screen is out: release the OLED; called by LPSPI4's
/// handler.
pub(crate) fn on_frame_complete() {
    critical_section::with(|cs| {
        if let Some(state) = STATE.borrow_ref_mut(cs).as_mut()
            && state.release_oled()
        {
            let mut mail = MAILBOX.borrow_ref_mut(cs);
            mail.chunks.1 = mail.chunks.1.wrapping_add(1);
        }
    });
}

impl AudioState {
    /// One period: see the module documentation.
    fn run(&mut self, mail: &mut Mailbox, entry: u32) {
        self.dma.clear_interrupt();
        self.dma.clear_complete();
        let half_period = PERIOD_CYCLES / 2;
        if let Some(previous) = self.last_entry {
            let since = entry.wrapping_sub(previous);
            // Not a new burst: the flag of the last one, seen again.
            if since < half_period {
                return;
            }
            mail.stats.period.record(since);
        }
        self.last_entry = Some(entry);
        self.follow(entry);
        mail.periods = mail.periods.wrapping_add(1);

        // The bursts whose echo is complete, four words each; the one just
        // queued, if its echo is not, is in flight. The DMA has read every
        // slot of both.
        let received = usize::from(self.spi.fifo_status().rxcount);
        let bursts = received / CV_CHANNELS;
        for _ in 0..bursts * CV_CHANNELS {
            let _ = self.spi.read_data();
        }
        self.completed = self.completed.wrapping_add(bursts);
        mail.bursts = mail.bursts.wrapping_add(u32::try_from(bursts).unwrap_or(0));
        let in_flight = usize::from(!self.burst_out());
        self.dma_next = (self.completed + in_flight) % SLOTS;

        // The next sample, two slots ahead of the DMA: the one after its
        // next read.
        // Timed apart from the rest (journal/D-039): within the wait for
        // the burst's echo, `audio` cannot see them. The parameters come
        // prepared: taking them is a copy (journal/D-040).
        if mail.fresh {
            let start = DWT::cycle_count();
            self.voice.apply(&mail.setup);
            mail.stats
                .voice_params
                .record(DWT::cycle_count().wrapping_sub(start));
            self.calibration = mail.calibration;
            mail.fresh = false;
        }
        let start = DWT::cycle_count();
        let samples = self.voice.next_sample();
        mail.stats
            .voice
            .record(DWT::cycle_count().wrapping_sub(start));
        let mut codes = [0; CV_CHANNELS];
        for channel in CvChannel::ALL {
            let index = channel.index();
            codes[index] = self.calibration[index].to_code(samples[index]);
        }
        crate::audio_irq::write_slot(self.dma_next + 1, &slot_words(&codes));

        // The frame to send, the contrast to set.
        if let Some(contrast) = mail.contrast.take() {
            self.stream.set_contrast(contrast);
        }
        if self.stream.is_idle() && !self.chunk_active {
            if let Some(started) = self.frame_started.take() {
                mail.stats.frame.record(entry.wrapping_sub(started));
                mail.frames = mail.frames.wrapping_add(1);
            }
            if mail.frame_ready {
                self.stream.start(&mail.frame);
                mail.frame_ready = false;
                self.frame_started = Some(entry);
            }
        }

        // The burst out, a piece of the screen if the period has room for
        // it and for the release before the next burst. The wait is bounded:
        // a burst that never echoes must not hold the interrupt.
        let deadline = entry.wrapping_add(2 * board::DAC_BURST_MICROS * CYCLES_PER_MICRO);
        while !self.burst_out() && deadline.wrapping_sub(DWT::cycle_count()).cast_signed() > 0 {
            core::hint::spin_loop();
        }
        let left = board::AUDIO_PERIOD_COUNTS.saturating_sub(self.since_trigger_micros());
        let fits = left >= board::OLED_CHUNK_MICROS + board::OLED_RELEASE_MICROS;
        if !self.chunk_active && self.burst_out() && fits && self.send_chunk() {
            mail.chunks.0 = mail.chunks.0.wrapping_add(1);
        }

        mail.stats
            .audio
            .record(DWT::cycle_count().wrapping_sub(entry));
    }

    /// Whether every queued DAC word has been echoed: nothing left to send
    /// and the echoes a whole number of bursts.
    fn burst_out(&self) -> bool {
        let fifo = self.spi.fifo_status();
        fifo.txcount == 0 && usize::from(fifo.rxcount).is_multiple_of(CV_CHANNELS)
    }

    /// Keeps [`Self::reference`] on the earliest run's phase, carried to the
    /// period of `entry`.
    fn follow(&mut self, entry: u32) {
        let reference = self.reference.map_or(entry, |reference| {
            let late = entry.wrapping_sub(reference) % PERIOD_CYCLES;
            if late > PERIOD_CYCLES / 2 {
                // Earlier than the reference: the new earliest.
                entry
            } else {
                entry.wrapping_sub(late)
            }
        });
        self.reference = Some(reference);
    }

    /// Microseconds since this period's trigger, as far as the earliest run
    /// tells it (a little after the trigger: the DMA's copy and the
    /// interrupt's entry).
    fn since_trigger_micros(&self) -> u32 {
        let now = DWT::cycle_count();
        now.wrapping_sub(self.reference.unwrap_or(now)) / CYCLES_PER_MICRO
    }

    /// Queues the next piece of the screen, if any, and arms the
    /// frame-complete interrupt that releases the OLED; whether it did.
    fn send_chunk(&mut self) -> bool {
        let Some(chunk) = self.stream.next_chunk(board::OLED_CHUNK_BYTES) else {
            return false;
        };
        if self.current_clock != self.clocks.oled {
            let oled = self.clocks.oled;
            self.spi.disabled(|spi| spi.set_clock_configs(oled));
            self.current_clock = oled;
        }
        match chunk.kind {
            ChunkKind::Command => self.oled_dc.clear(),
            ChunkKind::Data => self.oled_dc.set(),
        }
        self.oled_cs.clear();
        settle(&self.oled_cs, false);
        let bits = u16::try_from(chunk.bytes.len() * 8).unwrap_or(8);
        let mut transaction = Transaction::new(bits).unwrap_or_else(|_| unreachable!());
        transaction.set_pcs(board::OLED_PCS);
        transaction.set_receive_data_mask(true);
        self.spi.clear_status(Status::FRAME_COMPLETE);
        self.spi.set_interrupts(Interrupts::FRAME_COMPLETE);
        self.spi.enqueue_transaction(transaction);
        for word in fifo::words(chunk.bytes) {
            self.spi.enqueue_data(word);
        }
        self.chunk_active = true;
        true
    }

    /// Releases the OLED once its piece is out, and gives the bus the DAC's
    /// timing back before the next burst; whether a piece was out.
    fn release_oled(&mut self) -> bool {
        self.spi.clear_status(Status::FRAME_COMPLETE);
        self.spi.set_interrupts(Interrupts::empty());
        if !self.chunk_active {
            return false;
        }
        // The chip select goes through GPIO2 and the DAC's words through
        // LPSPI4, two buses that do not order their writes: the release is
        // waited for (journal/D-033, lot 7c on the module).
        self.oled_cs.set();
        settle(&self.oled_cs, true);
        self.chunk_active = false;
        if self.current_clock != self.clocks.dac {
            let dac = self.clocks.dac;
            self.spi.disabled(|spi| spi.set_clock_configs(dac));
            self.current_clock = dac;
        }
        true
    }
}

/// Waits until a write to `pin` has reached it: the barrier drains the core's
/// write buffer, the read-back waits for the GPIO itself.
fn settle(pin: &Output, high: bool) {
    cortex_m::asm::dsb();
    while pin.is_set() != high {
        core::hint::spin_loop();
    }
}

/// The engine's outputs on the module: the audio interrupt writes the DAC
/// from [`Publisher::publish`], so the codes the engine computes once a tick are
/// dropped.
#[derive(Debug)]
pub(crate) struct MutedOutputs;

impl AnalogOut for MutedOutputs {
    fn write_code(&mut self, _channel: CvChannel, _code: u16) {}

    fn flush(&mut self) {}
}

/// The engine's display on the module: the frame it draws is copied for the
/// audio interrupt to send, the latest one winning.
pub(crate) struct MailboxDisplay {
    frame: FrameBuffer,
}

impl MailboxDisplay {
    /// A blank frame.
    pub(crate) const fn new() -> Self {
        Self {
            frame: FrameBuffer::new(),
        }
    }
}

impl Display for MailboxDisplay {
    fn frame_mut(&mut self) -> &mut FrameBuffer {
        &mut self.frame
    }

    fn present(&mut self) {
        critical_section::with(|cs| {
            let mut mail = MAILBOX.borrow_ref_mut(cs);
            mail.frame = *self.frame.as_bytes();
            mail.frame_ready = true;
        });
    }

    fn set_contrast(&mut self, contrast: u8) {
        critical_section::with(|cs| MAILBOX.borrow_ref_mut(cs).contrast = Some(contrast));
    }
}

impl core::fmt::Debug for MailboxDisplay {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("MailboxDisplay").finish_non_exhaustive()
    }
}
