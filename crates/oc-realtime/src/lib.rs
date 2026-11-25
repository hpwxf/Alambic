//! The real-time path (journal/D-033, lots 7c and 7e): what the interrupts
//! own and what the 1 kHz loop hands them. A crate of its own, compiled for
//! speed, so that the firmware around it compiles for size: the per-sample
//! path (`Voices`, the generators) is instantiated here (journal/D-038,
//! revision of 2026-11-26). The DAC is written by DMA at
//! instants the PIT fixes, an interrupt computes the samples and sends the
//! screen between two DAC bursts; the loop only leaves them what to play
//! and what to show.
//!
//! # One period, 21 PERCLK counts
//!
//! 1. PIT channel 0 times out: the DMA copies one slot of the ring
//!    (`irq`) into LPSPI4, four 24-bit words on PCS0, about 5.5 µs on the
//!    bus. The instant does not depend on any software.
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
//! # The interrupts' side
//!
//! What the two handlers own is one `SharedState`: the DAC's side
//! (`dac`: the trigger, the DMA, the voices, steps 1 and 2) and the
//! screen's (`screen`: the OLED's lines and the frame going out, steps 3
//! and 4), with what both depend on, here: the bus they take turns on
//! (`Bus`, LPSPI4 and the timing in force) and where the period stands
//! (`Timing`, on the cycle counter). Each side reaches the bus only through
//! the borrow `SharedState` hands it.
//!
//! # The loop's side
//!
//! The mailbox has a half for each side. The engine keeps its output and
//! display traits: [`MutedOutputs`] stands for outputs it never writes,
//! [`MailboxDisplay`] hands its frames over; after each tick the loop's
//! [`Publisher`] hands on [`oc_core::Engine::patch`] when its revision
//! moved, and prepares there, outside any critical section, the setups of
//! the slots whose parameters changed: the interrupt only copies them
//! (journal/D-040, D-046). Everything the contexts share sits in
//! [`critical_section::Mutex`]es: the loop's sections are copies of a few
//! hundred bytes (a frame: 1 KiB). They delay the interrupts a little,
//! never the DAC.
//!
//! The DMA's programming, the ring and the handlers, the only `unsafe` of
//! this path, are in `irq`.

#![no_std]
#![deny(unsafe_code)]

mod dac;
mod irq;
mod screen;

use core::cell::RefCell;

use cortex_m::peripheral::DWT;
use critical_section::Mutex;

use teensy4_bsp::hal::dma::channel::Channel;
use teensy4_bsp::hal::gpio::Output;
use teensy4_bsp::hal::lpspi::{ClockConfigs, Lpspi};
use teensy4_bsp::hal::pit::Pit;

use oc_core::platform::CV_CHANNELS;
use oc_core::profile::{Stage, Stat};
use oc_drivers::ssd130x::Controller;

use self::dac::{Audio, DacMail};
pub use self::dac::{MutedOutputs, Publisher, played, resume_outputs};
pub use self::screen::MailboxDisplay;
use self::screen::{Screen, ScreenMail};
use oc_board::{self as board, BusClocks, CORE_CLOCK_HZ};

/// What the loop leaves for the interrupts, and what they measured: a half
/// for each side, and what the period itself counts.
struct Mailbox {
    dac: DacMail,
    screen: ScreenMail,
    /// Runs of the interrupt, wrapping.
    periods: u32,
    /// One run of the interrupt, in core cycles.
    run: Stat,
    /// The time between two runs, in core cycles.
    period: Stat,
}

static MAILBOX: Mutex<RefCell<Mailbox>> = Mutex::new(RefCell::new(Mailbox {
    dac: DacMail::new(),
    screen: ScreenMail::new(),
    periods: 0,
    run: Stat::new(),
    period: Stat::new(),
}));

/// What the interrupts own, from [`start`] on.
static STATE: Mutex<RefCell<Option<SharedState>>> = Mutex::new(RefCell::new(None));

/// What the interrupts own: the DAC's side and the screen's, and what both
/// share, the bus they take turns on and the period's timing.
struct SharedState {
    bus: Bus,
    timing: Timing,
    audio: Audio,
    screen: Screen,
}

/// LPSPI4, which the DAC's bursts and the screen's pieces share, each at its
/// own timing.
struct Bus {
    spi: Lpspi,
    clocks: BusClocks,
    /// The timing in force on the bus.
    current_clock: ClockConfigs,
}

/// Where the interrupt stands in the period, on the cycle counter.
struct Timing {
    /// Cycle count at the previous run, for the period.
    last_entry: Option<u32>,
    /// The cycle count of the earliest run seen, carried to the current
    /// period: the closest to the DAC's trigger, which the interrupt follows
    /// by a constant. PERCLK and the core both come from the crystal, so the
    /// period is a whole number of cycles; the PIT itself is not read, every
    /// access to it waits on its 1 MHz domain for microseconds.
    reference: Option<u32>,
}

/// Core cycles in an audio period.
const PERIOD_CYCLES: u32 = CORE_CLOCK_HZ / board::AUDIO_TIMER_HZ * board::AUDIO_PERIOD_COUNTS;

/// Core cycles in a microsecond.
const CYCLES_PER_MICRO: u32 = CORE_CLOCK_HZ / 1_000_000;

/// Takes the bus, the OLED's control lines, the PIT and a DMA channel over,
/// and starts the real-time path at [`board::AUDIO_RATE_HZ`]. The OLED must
/// be initialised; the outputs rest at 0 V (factory calibration) until the
/// first [`Publisher::publish`]. Not inlined: the state it builds (≈ 2 KiB)
/// would stay in the start-up's frame, the deepest path of the stack
/// (journal/D-038).
#[allow(clippy::too_many_arguments)]
// The hardware the path takes over, each once, at start-up.
#[inline(never)]
pub fn start(
    mut spi: Lpspi,
    oled_cs: Output,
    oled_dc: Output,
    pit: Pit,
    dma: Channel,
    clocks: BusClocks,
    controller: Controller,
) {
    spi.clear_fifos();
    spi.disabled(|spi| spi.set_clock_configs(clocks.dac));
    let screen = Screen::new(oled_cs, oled_dc, controller);
    let audio = Audio::start(pit, dma, spi.tdr().cast::<u32>());
    let state = SharedState {
        bus: Bus {
            spi,
            clocks,
            current_clock: clocks.dac,
        },
        timing: Timing {
            last_entry: None,
            reference: None,
        },
        audio,
        screen,
    };
    critical_section::with(|cs| {
        let mut slot = STATE.borrow_ref_mut(cs);
        slot.insert(state).audio.enable_trigger();
    });
    irq::enable();
}

/// What the real-time path has done since start-up, each count wrapping.
#[derive(Debug, Clone, Copy, Default)]
pub struct Totals {
    /// Runs of the interrupt.
    pub periods: u32,
    /// DAC bursts the DMA sent, counted by their echo.
    pub bursts: u32,
    /// Screen chunks sent.
    pub chunks: u32,
    /// OLED releases after a chunk.
    pub releases: u32,
    /// Whole frames sent to the screen.
    pub frames: u32,
}

impl Totals {
    /// What happened between `earlier` and `self`.
    #[must_use]
    pub const fn since(self, earlier: Self) -> Self {
        Self {
            periods: self.periods.wrapping_sub(earlier.periods),
            bursts: self.bursts.wrapping_sub(earlier.bursts),
            chunks: self.chunks.wrapping_sub(earlier.chunks),
            releases: self.releases.wrapping_sub(earlier.releases),
            frames: self.frames.wrapping_sub(earlier.frames),
        }
    }
}

/// The real-time path's counts so far.
#[must_use]
pub fn totals() -> Totals {
    critical_section::with(|cs| {
        let mail = MAILBOX.borrow_ref(cs);
        Totals {
            periods: mail.periods,
            bursts: mail.dac.bursts,
            chunks: mail.screen.chunks.0,
            releases: mail.screen.chunks.1,
            frames: mail.screen.frames,
        }
    })
}

/// What [`self_check`] saw of the real-time path.
#[derive(Debug)]
pub struct Health {
    /// How long the check lasted, on the cycle counter, in microseconds.
    pub micros: u32,
    /// What happened during it.
    pub counts: Totals,
    /// The interrupt's duration, mean and longest, in microseconds.
    pub audio_micros: (u32, u32),
    /// The time between two runs, shortest and longest, in microseconds.
    pub period_micros: (u32, u32),
    /// Whether the period's NVIC line is enabled, and whether it is pending.
    pub line: (bool, bool),
    /// The DMA channel's error flag and the controller's error status
    /// register, should it fail to read the ring or write LPSPI4.
    pub dma_error: (bool, u32),
}

impl Health {
    /// `count` events during the check, as a rate a second.
    #[must_use]
    pub const fn per_second(&self, count: u32) -> u64 {
        if self.micros == 0 {
            return 0;
        }
        count as u64 * 1_000_000 / self.micros as u64
    }
}

/// Watches the real-time path for `micros` microseconds, timed on the cycle
/// counter (a busy delay runs short on the M7), for the boot log: a silent
/// DMA or interrupt shows on USB rather than as a black screen, and the
/// rates and durations give a baseline to compare a later log with.
#[must_use]
pub fn self_check(micros: u32) -> Health {
    critical_section::with(|cs| {
        let mut mail = MAILBOX.borrow_ref_mut(cs);
        mail.run = Stat::new();
        mail.period = Stat::new();
        mail.dac.voice = Stat::new();
        mail.dac.voice_params = Stat::new();
        mail.screen.frame_time = Stat::new();
    });
    let before = totals();
    let start = DWT::cycle_count();
    while DWT::cycle_count().wrapping_sub(start) < micros.saturating_mul(CYCLES_PER_MICRO) {
        core::hint::spin_loop();
    }
    let elapsed = DWT::cycle_count().wrapping_sub(start) / CYCLES_PER_MICRO;
    let counts = totals().since(before);
    let (run, period) = critical_section::with(|cs| {
        let mail = MAILBOX.borrow_ref(cs);
        (mail.run, mail.period)
    });
    let in_micros = |cycles: Option<u32>| cycles.map_or(0, |cycles| cycles / CYCLES_PER_MICRO);
    let dma_error = critical_section::with(|cs| {
        STATE.borrow_ref(cs).as_ref().map_or((false, 0), |state| {
            (
                state.audio.dma.is_error(),
                state.audio.dma.error_status().raw(),
            )
        })
    });
    Health {
        micros: elapsed,
        counts,
        audio_micros: (in_micros(run.mean()), in_micros(run.max())),
        period_micros: (in_micros(period.min()), in_micros(period.max())),
        line: irq::line_state(),
        dma_error,
    }
}

/// Stops the DMA and the interrupts, the outputs left where they are:
/// before handing the chip to the bootloader.
pub fn stop() {
    irq::disable();
    critical_section::with(|cs| {
        if let Some(state) = STATE.borrow_ref_mut(cs).as_mut() {
            state.audio.dma.disable();
        }
    });
}

/// Runs of the interrupt so far, wrapping.
fn periods() -> u32 {
    critical_section::with(|cs| MAILBOX.borrow_ref(cs).periods)
}

/// Moves what the interrupts measured into `profile`.
pub fn drain_stats(profile: &mut oc_core::profile::Profile) {
    let (run, period, voice, voice_params, frame) = critical_section::with(|cs| {
        let mut mail = MAILBOX.borrow_ref_mut(cs);
        let mail = &mut *mail;
        (
            core::mem::replace(&mut mail.run, Stat::new()),
            core::mem::replace(&mut mail.period, Stat::new()),
            core::mem::replace(&mut mail.dac.voice, Stat::new()),
            core::mem::replace(&mut mail.dac.voice_params, Stat::new()),
            core::mem::replace(&mut mail.screen.frame_time, Stat::new()),
        )
    });
    profile.merge(Stage::Audio, &run);
    profile.merge(Stage::Voice, &voice);
    profile.merge(Stage::VoiceParams, &voice_params);
    profile.merge(Stage::AudioPeriod, &period);
    profile.merge(Stage::Frame, &frame);
}

/// One run of the period interrupt; called by its handler.
fn on_period() {
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
fn on_frame_complete() {
    critical_section::with(|cs| {
        if let Some(state) = STATE.borrow_ref_mut(cs).as_mut()
            && state.screen.release(&mut state.bus)
        {
            let mail = &mut MAILBOX.borrow_ref_mut(cs).screen;
            mail.chunks.1 = mail.chunks.1.wrapping_add(1);
        }
    });
}

impl SharedState {
    /// One period: see the module documentation.
    fn run(&mut self, mail: &mut Mailbox, entry: u32) {
        self.audio.dma.clear_interrupt();
        self.audio.dma.clear_complete();
        if !self.timing.enter(entry, &mut mail.period) {
            return;
        }
        mail.periods = mail.periods.wrapping_add(1);
        self.audio.count_bursts(&mut self.bus, &mut mail.dac);
        self.audio.write_next(&mut mail.dac);
        self.screen.take_frame(&mut mail.screen, entry);

        // The burst out, a piece of the screen if the period has room for
        // it and for the release before the next burst. The wait is bounded:
        // a burst that never echoes must not hold the interrupt.
        let deadline = entry.wrapping_add(2 * board::DAC_BURST_MICROS * CYCLES_PER_MICRO);
        while !self.bus.burst_out() && deadline.wrapping_sub(DWT::cycle_count()).cast_signed() > 0 {
            core::hint::spin_loop();
        }
        let left = board::AUDIO_PERIOD_COUNTS.saturating_sub(self.timing.since_trigger_micros());
        let fits = left >= board::OLED_CHUNK_MICROS + board::OLED_RELEASE_MICROS;
        if !self.screen.chunk_active
            && self.bus.burst_out()
            && fits
            && self.screen.send_chunk(&mut self.bus)
        {
            mail.screen.chunks.0 = mail.screen.chunks.0.wrapping_add(1);
        }

        mail.run.record(DWT::cycle_count().wrapping_sub(entry));
    }
}

impl Bus {
    /// Whether every queued DAC word has been echoed: nothing left to send
    /// and the echoes a whole number of bursts.
    fn burst_out(&self) -> bool {
        let fifo = self.spi.fifo_status();
        fifo.txcount == 0 && usize::from(fifo.rxcount).is_multiple_of(CV_CHANNELS)
    }

    /// Puts `configs` in force, unless it already is; the bus is disabled
    /// meanwhile.
    fn use_clock(&mut self, configs: ClockConfigs) {
        if self.current_clock != configs {
            self.spi.disabled(|spi| spi.set_clock_configs(configs));
            self.current_clock = configs;
        }
    }
}

impl Timing {
    /// Takes a run at `entry` into the period, its length into `period`;
    /// `false` for a run within half a period of the last: not a new burst,
    /// the flag of the last one seen again.
    fn enter(&mut self, entry: u32, period: &mut Stat) -> bool {
        if let Some(previous) = self.last_entry {
            let since = entry.wrapping_sub(previous);
            if since < PERIOD_CYCLES / 2 {
                return false;
            }
            period.record(since);
        }
        self.last_entry = Some(entry);
        self.follow(entry);
        true
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
}
