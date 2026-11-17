//! The audio path (journal/D-033, lot 7c): an interrupt at 48 kHz owns the
//! shared SPI bus, writes the DAC each period and sends the screen between
//! two samples; the 1 kHz loop only leaves it what to play and what to show.
//!
//! # One period
//!
//! 1. The previous piece of the screen has had the whole period to go out:
//!    wait for the bus to be idle, release the OLED's chip select.
//! 2. Queue the DAC words of the channels whose code changed (24-bit
//!    transactions on PCS0), computed in the previous period: one sample of
//!    latency, and no computing jitter on the jacks.
//! 3. While they go out, take what the loop left (output parameters,
//!    calibration, a frame, a contrast) and compute the next sample.
//! 4. Wait for the DAC words, then queue the next piece of the screen, the
//!    data / command line set and the OLED's chip select low, in one
//!    transaction on PCS3, which no pad carries.
//!
//! # The loop's side
//!
//! The engine keeps its output and display traits: [`MutedOutputs`] drops
//! its codes, [`MailboxDisplay`] hands its frames over; after each tick the
//! loop calls [`publish`] with [`oc_core::Engine::params`]. Everything the
//! two contexts share sits in [`critical_section::Mutex`]es: the loop's
//! sections are copies of a few hundred bytes (a frame: 1 KiB), during which
//! the interrupt waits.
//!
//! The registration of the handler and the NVIC calls, the only `unsafe`
//! of this path, are in [`crate::audio_irq`].

use core::cell::RefCell;

use cortex_m::peripheral::DWT;
use critical_section::Mutex;

use teensy4_bsp::hal::gpio::Output;
use teensy4_bsp::hal::gpt::{ClockSource, Gpt, Mode, OutputCompareRegister};
use teensy4_bsp::hal::lpspi::{ClockConfigs, Direction, Lpspi, Status, Transaction};

use oc_core::audio::{Params, Voice};
use oc_core::calibration::CvOutputCalibration;
use oc_core::framebuffer::{FrameBuffer, LEN};
use oc_core::platform::{AnalogOut, CV_CHANNELS, CvChannel, Display};
use oc_core::profile::{Stage, Stat};
use oc_drivers::dac8565::ChangedCodes;
use oc_drivers::fifo;
use oc_drivers::ssd130x::{ChunkKind, Controller, FrameStream};

use crate::board::{self, BusClocks};
use crate::delay::CORE_CLOCK_HZ;

/// What the loop leaves for the interrupt, and what the interrupt measured.
struct Mailbox {
    params: Params,
    calibration: [CvOutputCalibration; CV_CHANNELS],
    /// Set when `params` or `calibration` changed since the interrupt took
    /// them.
    fresh: bool,
    frame: [u8; LEN],
    /// Set when `frame` holds a frame the interrupt has not taken.
    frame_ready: bool,
    contrast: Option<u8>,
    /// Runs of the interrupt, wrapping.
    periods: u32,
    stats: Stats,
}

/// The interrupt's own measurements, in core cycles, until the loop takes
/// them into the profile.
#[derive(Clone, Copy)]
struct Stats {
    audio: Stat,
    period: Stat,
    frame: Stat,
}

impl Stats {
    const fn new() -> Self {
        Self {
            audio: Stat::new(),
            period: Stat::new(),
            frame: Stat::new(),
        }
    }
}

static MAILBOX: Mutex<RefCell<Mailbox>> = Mutex::new(RefCell::new(Mailbox {
    params: Params::REST,
    calibration: [CvOutputCalibration::FACTORY; CV_CHANNELS],
    fresh: false,
    frame: [0; LEN],
    frame_ready: false,
    contrast: None,
    periods: 0,
    stats: Stats::new(),
}));

/// What the interrupt owns, from [`start`] on.
static STATE: Mutex<RefCell<Option<AudioState>>> = Mutex::new(RefCell::new(None));

/// The interrupt's hardware and its signal state.
struct AudioState {
    spi: Lpspi,
    oled_cs: Output,
    oled_dc: Output,
    timer: Gpt,
    clocks: BusClocks,
    /// The timing in force on the bus.
    current_clock: ClockConfigs,
    stream: FrameStream,
    voice: Voice,
    calibration: [CvOutputCalibration; CV_CHANNELS],
    changed: ChangedCodes,
    /// The codes computed last period, sent this one.
    next_codes: [u16; CV_CHANNELS],
    /// Cycle count at the previous run, for the period.
    last_entry: Option<u32>,
    /// Cycle count when the frame going out was taken.
    frame_started: Option<u32>,
    /// Words queued since the bus was last seen idle, each echoed in the
    /// receive FIFO once its last bit is out.
    in_flight: u16,
}

/// Which input GPT2 counts, and so the audio rate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TimerInput {
    /// The 24 MHz crystal: [`board::AUDIO_RATE_HZ`] exactly.
    Crystal,
    /// PERCLK, 1 MHz, if the crystal input did not start:
    /// [`board::AUDIO_PERCLK_COUNTS`] counts a period.
    Perclk,
}

impl TimerInput {
    /// Samples a second.
    pub(crate) const fn rate_hz(self) -> u32 {
        match self {
            Self::Crystal => board::AUDIO_RATE_HZ,
            Self::Perclk => board::AUDIO_PERCLK_HZ / board::AUDIO_PERCLK_COUNTS,
        }
    }
}

/// Points `timer` at `source`, compares every `counts`, in the order the
/// reference manual gives for a change of clock source (GPT, "Selecting the
/// clock source"): disabled, interrupts off, source, software reset, then
/// the rest. The reset is done again for the source in case it clears it.
fn configure(timer: &mut Gpt, source: ClockSource, counts: u32) {
    timer.disable();
    timer.set_output_interrupt_on_compare(OutputCompareRegister::OCR1, false);
    timer.set_clock_source(source);
    timer.reset();
    timer.set_clock_source(source);
    timer.set_divider_24mhz(1);
    timer.set_divider(1);
    timer.set_mode(Mode::Restart);
    timer.set_reset_on_enable(true);
    timer.set_output_compare_count(OutputCompareRegister::OCR1, counts - 1);
    timer.clear_elapsed(OutputCompareRegister::OCR1);
}

/// Sets GPT2 to the crystal and checks it compares within a period and a
/// bit; otherwise falls back to PERCLK. The timer is left disabled.
fn choose_input(timer: &mut Gpt, delay: &mut impl embedded_hal::delay::DelayNs) -> TimerInput {
    configure(
        timer,
        ClockSource::CrystalOscillator,
        board::AUDIO_CRYSTAL_HZ / board::AUDIO_RATE_HZ,
    );
    timer.enable();
    // A period is 20.8 µs; at PERCLK's 1 MHz the compare would take 500 µs.
    delay.delay_us(25);
    let crystal = timer.is_elapsed(OutputCompareRegister::OCR1);
    timer.disable();
    if crystal {
        configure(
            timer,
            ClockSource::CrystalOscillator,
            board::AUDIO_CRYSTAL_HZ / board::AUDIO_RATE_HZ,
        );
        TimerInput::Crystal
    } else {
        configure(
            timer,
            ClockSource::PeripheralClock,
            board::AUDIO_PERCLK_COUNTS,
        );
        TimerInput::Perclk
    }
}

/// Takes the bus, the OLED's control lines and GPT2 over, and starts the
/// interrupt, at the rate of the input GPT2 could count ([`TimerInput`]).
/// The OLED must be initialised; the outputs rest at 0 V (factory
/// calibration) until the first [`publish`].
pub(crate) fn start(
    spi: Lpspi,
    oled_cs: Output,
    oled_dc: Output,
    mut timer: Gpt,
    clocks: BusClocks,
    controller: Controller,
    delay: &mut impl embedded_hal::delay::DelayNs,
) -> TimerInput {
    let input = choose_input(&mut timer, delay);
    timer.set_output_interrupt_on_compare(OutputCompareRegister::OCR1, true);

    let rest = CvOutputCalibration::FACTORY.to_code(0);
    let state = AudioState {
        spi,
        oled_cs,
        oled_dc,
        timer,
        clocks,
        current_clock: clocks.oled,
        stream: FrameStream::new(controller),
        voice: Voice::new(input.rate_hz()),
        calibration: [CvOutputCalibration::FACTORY; CV_CHANNELS],
        changed: ChangedCodes::new(),
        next_codes: [rest; CV_CHANNELS],
        last_entry: None,
        frame_started: None,
        in_flight: 0,
    };
    critical_section::with(|cs| {
        let mut slot = STATE.borrow_ref_mut(cs);
        let state = slot.insert(state);
        state.oled_cs.set();
        state.timer.enable();
    });
    crate::audio_irq::enable();
    input
}

/// What [`self_check`] saw of the timer and the interrupt.
#[derive(Debug)]
pub(crate) struct Health {
    /// Runs of the interrupt during the check.
    pub(crate) periods: u32,
    /// The timer's counter at the start and the end of the check.
    pub(crate) counts: (u32, u32),
    /// Whether the compare flag is set.
    pub(crate) compare_flag: bool,
    /// Whether the NVIC line is enabled, and whether it is pending.
    pub(crate) line: (bool, bool),
}

/// Watches the interrupt for `micros` microseconds, for the boot log: a
/// silent audio path shows on USB rather than as a black screen.
pub(crate) fn self_check(delay: &mut impl embedded_hal::delay::DelayNs, micros: u32) -> Health {
    let count = || {
        critical_section::with(|cs| {
            STATE.borrow_ref(cs).as_ref().map_or((0, false), |state| {
                (
                    state.timer.count(),
                    state.timer.is_elapsed(OutputCompareRegister::OCR1),
                )
            })
        })
    };
    let start = periods();
    let (first, _) = count();
    delay.delay_us(micros);
    let (last, compare_flag) = count();
    Health {
        periods: periods().wrapping_sub(start),
        counts: (first, last),
        compare_flag,
        line: crate::audio_irq::line_state(),
    }
}

/// Stops the interrupt, the outputs left where they are: before handing the
/// chip to the bootloader.
pub(crate) fn stop() {
    crate::audio_irq::disable();
}

/// Hands the interrupt what the outputs carry and how to convert it, from
/// the next period on; copies only when something changed.
pub(crate) fn publish(params: &Params, calibration: &[CvOutputCalibration; CV_CHANNELS]) {
    critical_section::with(|cs| {
        let mut mail = MAILBOX.borrow_ref_mut(cs);
        if mail.params != *params || mail.calibration != *calibration {
            mail.params = *params;
            mail.calibration = *calibration;
            mail.fresh = true;
        }
    });
}

/// Puts every output at 0 V and waits until the DAC has it: before a flash
/// operation, during which the boot ROM masks the interrupt (journal/D-033).
/// The next [`publish`] brings the parameters back.
pub(crate) fn rest_outputs(calibration: &[CvOutputCalibration; CV_CHANNELS]) {
    publish(&Params::REST, calibration);
    let running = critical_section::with(|cs| STATE.borrow_ref(cs).is_some());
    if !running {
        return;
    }
    // Taken in one period, computed then, sent in the next: three periods
    // leave one to spare.
    let start = periods();
    while periods().wrapping_sub(start) < 3 {
        cortex_m::asm::nop();
    }
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
    profile.merge(Stage::AudioPeriod, &stats.period);
    profile.merge(Stage::Frame, &stats.frame);
}

/// One run of the interrupt; called by the handler in [`crate::audio_irq`].
pub(crate) fn on_interrupt() {
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

impl AudioState {
    /// One period: see the module documentation.
    fn run(&mut self, mail: &mut Mailbox, entry: u32) {
        self.timer.clear_elapsed(OutputCompareRegister::OCR1);
        if let Some(previous) = self.last_entry {
            mail.stats.period.record(entry.wrapping_sub(previous));
        }
        self.last_entry = Some(entry);
        mail.periods = mail.periods.wrapping_add(1);

        // 1. The previous piece of the screen. The chip select goes through
        // GPIO2 and the words through LPSPI4, two buses that do not order
        // their writes: the release is waited for, or the OLED would take
        // the start of the DAC's words (journal/D-033, lot 7c on the module).
        self.wait_idle();
        self.oled_cs.set();
        settle(&self.oled_cs, true);

        // 2. The DAC words of last period's sample.
        let words = self.changed.words(&self.next_codes);
        if !words.as_slice().is_empty() {
            self.set_clock(self.clocks.dac);
            let mut transaction = Transaction::new(24).unwrap_or_else(|_| unreachable!());
            transaction.set_pcs(board::DAC_PCS);
            self.in_flight = u16::try_from(words.as_slice().len()).unwrap_or(0);
            for &word in words.as_slice() {
                self.spi.enqueue_transaction(transaction);
                self.spi.enqueue_data(word);
            }
        }

        // 3. The mailbox and the next sample, while they go out.
        if mail.fresh {
            self.voice.set_params(&mail.params);
            self.calibration = mail.calibration;
            mail.fresh = false;
        }
        if let Some(contrast) = mail.contrast.take() {
            self.stream.set_contrast(contrast);
        }
        if self.stream.is_idle() {
            if let Some(started) = self.frame_started.take() {
                mail.stats.frame.record(entry.wrapping_sub(started));
            }
            if mail.frame_ready {
                self.stream.start(&mail.frame);
                mail.frame_ready = false;
                self.frame_started = Some(entry);
            }
        }
        let samples = self.voice.next_sample();
        for channel in CvChannel::ALL {
            let index = channel.index();
            self.next_codes[index] = self.calibration[index].to_code(samples[index]);
        }

        // 4. The next piece of the screen, once the DAC words are out.
        self.wait_idle();
        self.send_chunk();

        // The compare flag sits in the timer's clock domain: on PERCLK's
        // 1 MHz its clearing takes a few microseconds to land, and a short
        // run that returned before would run again at once. Waited for
        // within half a period, past which a set flag is the next period's.
        let half_period = CORE_CLOCK_HZ / self.voice.rate_hz() / 2;
        while self.timer.is_elapsed(OutputCompareRegister::OCR1)
            && DWT::cycle_count().wrapping_sub(entry) < half_period
        {
            core::hint::spin_loop();
        }

        mail.stats
            .audio
            .record(DWT::cycle_count().wrapping_sub(entry));
    }

    /// Queues the next piece of the screen, if any.
    fn send_chunk(&mut self) {
        let Some(chunk) = self.stream.next_chunk(board::OLED_CHUNK_BYTES) else {
            return;
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
        self.in_flight = u16::try_from(fifo::word_count(chunk.bytes.len())).unwrap_or(0);
        self.spi.enqueue_transaction(transaction);
        for word in fifo::words(chunk.bytes) {
            self.spi.enqueue_data(word);
        }
    }

    /// Waits until the words queued last have left the bus: each is echoed
    /// in the receive FIFO once its last bit has been clocked, which is how
    /// `imxrt-hal`'s blocking writes end too. The busy flag with an empty
    /// transmit FIFO, and the transfer complete flag, answered early: the
    /// last DAC word was still going out when the OLED was selected (`TONE`,
    /// lot 7c on the module).
    fn wait_idle(&mut self) {
        if self.in_flight > 0 {
            while self.spi.fifo_status().rxcount < self.in_flight {
                core::hint::spin_loop();
            }
            self.in_flight = 0;
        }
        self.spi.clear_fifo(Direction::Rx);
        let errors = Status::TRANSMIT_ERROR | Status::RECEIVE_ERROR;
        if self.spi.status().intersects(errors) {
            self.spi.clear_status(errors);
        }
    }

    /// Puts `clock` in force; the bus is idle here.
    fn set_clock(&mut self, clock: ClockConfigs) {
        if self.current_clock != clock {
            self.spi.disabled(|spi| spi.set_clock_configs(clock));
            self.current_clock = clock;
        }
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
/// from [`publish`], so the codes the engine computes once a tick are
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
