//! The DAC's side of the real-time path (journal/D-033, D-040): the PIT
//! that times each burst, the DMA that copies the ring to the bus, the
//! voices the interrupt fills the ring with, and what the loop hands them.

use teensy4_bsp::hal::dma::channel::Channel;
use teensy4_bsp::hal::pit::{self, Pit};
use teensy4_bsp::ral::lpspi::TCR;

use cortex_m::peripheral::DWT;

use oc_core::calibration::CvOutputCalibration;
use oc_core::platform::{AnalogOut, CV_CHANNELS, CvChannel, MilliVolts};
use oc_core::playback::{Changes, Follower, Patch, Voices};
use oc_core::profile::Stat;
use oc_drivers::dac8565::channel_word;

use super::irq::{SLOT_WORDS, SLOTS};
use super::{Bus, MAILBOX, STATE, periods};
use crate::board;

/// The PIT channel that triggers the DMA ([`board::AUDIO_DMA_CHANNEL`]).
const DAC_TIMER: pit::Channel = pit::Channel::Chan0;

/// The command the DMA queues before each DAC word: a 24-bit frame on
/// PCS0, mode 0, most significant bit first, the echo kept (it counts the
/// bursts). `imxrt-hal`'s `Transaction` has no raw value to put in a buffer.
const DAC_TCR: u32 = (23 << TCR::FRAMESZ::offset) | ((board::DAC_PCS as u32) << TCR::PCS::offset);

/// The DAC's half of the mailbox: what the loop leaves for the samples,
/// what the interrupt played and measured of them.
pub(super) struct DacMail {
    /// What the engine's patch changed since the interrupt last took it:
    /// the setups prepared for the slots that moved, every route and held
    /// level.
    changes: Changes,
    calibration: [CvOutputCalibration; CV_CHANNELS],
    /// Set when `changes` or `calibration` changed since the interrupt took
    /// them.
    fresh: bool,
    /// The levels of the last sample the interrupt computed, for the engine
    /// to report (`oc_core::Engine::set_played`).
    played: [MilliVolts; CV_CHANNELS],
    /// DAC bursts the interrupt has seen, wrapping.
    pub(super) bursts: u32,
    /// Rendering one sample, in core cycles.
    pub(super) voice: Stat,
    /// Taking new parameters, in core cycles.
    pub(super) voice_params: Stat,
}

impl DacMail {
    /// Nothing to play: every output at rest, the factory calibration.
    pub(super) const fn new() -> Self {
        Self {
            changes: Changes::SILENCE,
            calibration: [CvOutputCalibration::FACTORY; CV_CHANNELS],
            fresh: false,
            played: [0; CV_CHANNELS],
            bursts: 0,
            voice: Stat::new(),
            voice_params: Stat::new(),
        }
    }
}

/// The DAC's side: the trigger, the DMA that copies the ring, the voices
/// that fill it.
pub(super) struct Audio {
    pit: Pit,
    pub(super) dma: Channel,
    voices: Voices,
    calibration: [CvOutputCalibration; CV_CHANNELS],
    /// The ring slot the DMA reads at its next trigger.
    dma_next: usize,
    /// DAC bursts whose four echoes came back, wrapping.
    completed: usize,
}

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

impl Audio {
    /// Fills the ring with 0 V (factory calibration), sets the PIT's period
    /// and programs the DMA to `tdr`, LPSPI4's transmit data register; the
    /// trigger waits for [`Self::enable_trigger`].
    pub(super) fn start(mut pit: Pit, mut dma: Channel, tdr: *const u32) -> Self {
        let rest = slot_words(&[CvOutputCalibration::FACTORY.to_code(0); CV_CHANNELS]);
        for slot in 0..SLOTS {
            super::irq::write_slot(slot, &rest);
        }
        pit.disable(DAC_TIMER);
        pit.set_load_timer_value(DAC_TIMER, board::AUDIO_PERIOD_COUNTS);
        pit.clear_elapsed(DAC_TIMER);
        super::irq::start_dma(&mut dma, tdr);
        Self {
            pit,
            dma,
            voices: Voices::new(board::AUDIO_RATE_HZ),
            calibration: [CvOutputCalibration::FACTORY; CV_CHANNELS],
            dma_next: 0,
            completed: 0,
        }
    }

    /// Lets the PIT trigger the DMA, one burst a period from now on.
    pub(super) fn enable_trigger(&mut self) {
        self.pit.enable(DAC_TIMER);
    }

    /// Counts the bursts whose echo is complete, four words each, and finds
    /// the slot the DMA reads next; the one just queued, if its echo is not
    /// complete, is in flight. The DMA has read every slot of both.
    pub(super) fn count_bursts(&mut self, bus: &mut Bus, mail: &mut DacMail) {
        let received = usize::from(bus.spi.fifo_status().rxcount);
        let bursts = received / CV_CHANNELS;
        for _ in 0..bursts * CV_CHANNELS {
            let _ = bus.spi.read_data();
        }
        self.completed = self.completed.wrapping_add(bursts);
        mail.bursts = mail.bursts.wrapping_add(u32::try_from(bursts).unwrap_or(0));
        let in_flight = usize::from(!bus.burst_out());
        self.dma_next = (self.completed + in_flight) % SLOTS;
    }

    /// Takes what the loop left, then computes the next sample and writes it
    /// two slots ahead of the DMA: the one after its next read.
    pub(super) fn write_next(&mut self, mail: &mut DacMail) {
        // Timed apart from the rest (journal/D-039): within the wait for
        // the burst's echo, `audio` cannot see them. The parameters come
        // prepared: taking them is a copy (journal/D-040).
        if mail.fresh {
            let start = DWT::cycle_count();
            self.voices.apply(&mail.changes);
            mail.changes.applied();
            mail.voice_params
                .record(DWT::cycle_count().wrapping_sub(start));
            self.calibration = mail.calibration;
            mail.fresh = false;
        }
        let start = DWT::cycle_count();
        let samples = self.voices.next_sample();
        mail.voice.record(DWT::cycle_count().wrapping_sub(start));
        mail.played = samples;
        let mut codes = [0; CV_CHANNELS];
        for channel in CvChannel::ALL {
            let index = channel.index();
            codes[index] = self.calibration[index].to_code(samples[index]);
        }
        super::irq::write_slot(self.dma_next + 1, &slot_words(&codes));
    }
}

/// What the loop last handed the interrupt, so that a tick where nothing
/// changed costs a comparison of a revision and of the calibration, outside
/// any critical section: comparing the whole parameters under one cost
/// 1.5 µs a tick. Owned by the loop, the only writer of the mailbox.
#[derive(Debug)]
pub(crate) struct Publisher {
    /// The patch last prepared, by its revisions; forgotten when the
    /// mailbox holds something else (the outputs at rest).
    follower: Follower,
    calibration: Option<[CvOutputCalibration; CV_CHANNELS]>,
}

impl Publisher {
    /// Nothing handed yet: the first [`Self::publish`] copies everything.
    pub(crate) const fn new() -> Self {
        Self {
            follower: Follower::new(),
            calibration: None,
        }
    }

    /// Hands the interrupt what the outputs carry and how to convert it,
    /// from the next period on; copies only when something changed. New
    /// parameters are prepared here, outside the critical section, so the
    /// interrupt never waits for their divisions (journal/D-040).
    pub(crate) fn publish(
        &mut self,
        patch: &Patch,
        calibration: &[CvOutputCalibration; CV_CHANNELS],
    ) {
        let changes = self.follower.follow(patch, board::AUDIO_RATE_HZ);
        let new_calibration = self.calibration.as_ref() != Some(calibration);
        if changes.is_none() && !new_calibration {
            return;
        }
        hand_over(changes.as_ref(), calibration);
        self.calibration = Some(*calibration);
    }

    /// Puts every output at 0 V, waits until the DAC has it, then pauses the
    /// DMA: before a flash operation, during which the boot ROM masks the
    /// interrupts (journal/D-033), and the bursts would pile up in the FIFO.
    /// [`resume_outputs`] restarts it; the next [`Self::publish`] brings the
    /// parameters back.
    pub(crate) fn rest_outputs(&mut self, calibration: &[CvOutputCalibration; CV_CHANNELS]) {
        hand_over(Some(&Changes::SILENCE), calibration);
        self.follower.forget();
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
                state.audio.dma.disable();
            }
        });
    }
}

/// Puts `changes`, when there are some, and `calibration` in the mailbox
/// for the interrupt to take at its next period; changes it has not taken
/// yet are merged, the newer setup of a slot replacing the older.
fn hand_over(changes: Option<&Changes>, calibration: &[CvOutputCalibration; CV_CHANNELS]) {
    critical_section::with(|cs| {
        let mail = &mut MAILBOX.borrow_ref_mut(cs).dac;
        if let Some(changes) = changes {
            mail.changes.merge(changes);
        }
        mail.calibration = *calibration;
        mail.fresh = true;
    });
}

/// The levels of the last sample the interrupt computed: what the outputs
/// carry, for the engine to report instead of rendering them a second time
/// (journal/D-040).
pub(crate) fn played() -> [MilliVolts; CV_CHANNELS] {
    critical_section::with(|cs| MAILBOX.borrow_ref(cs).dac.played)
}

/// Restarts the DMA paused by [`Publisher::rest_outputs`], where it stopped
/// in the ring.
pub(crate) fn resume_outputs() {
    critical_section::with(|cs| {
        if let Some(state) = STATE.borrow_ref_mut(cs).as_mut() {
            super::irq::resume_dma(&state.audio.dma);
        }
    });
}

/// The engine's outputs on the module, never written: the audio interrupt
/// writes the DAC from [`Publisher::publish`], and the loop never calls
/// [`oc_core::Engine::write_outputs`] (journal/D-040).
#[derive(Debug)]
pub(crate) struct MutedOutputs;

impl AnalogOut for MutedOutputs {
    fn write_code(&mut self, _channel: CvChannel, _code: u16) {}

    fn flush(&mut self) {}
}
