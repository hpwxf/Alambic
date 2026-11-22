//! The audio path's `unsafe` (journal/D-033, lot 7e), kept apart as
//! AGENTS.md asks: the ring the DMA reads the DAC's words from, the DMA
//! channel's programming, the two interrupt handlers and their NVIC lines.
//! What the interrupts do is in [`crate::audio`].
//!
//! Both lines keep their reset priority, 0, the highest, so neither preempts
//! the other: no other interrupt is enabled on the module (USB is polled),
//! and nothing but the loop's short critical sections and the boot ROM's
//! flash calls can delay them.

#![allow(unsafe_code)]

use core::cell::UnsafeCell;

use cortex_m::peripheral::NVIC;
use imxrt_rt::interrupt;
use teensy4_bsp::hal::dma::channel::{Channel, Configuration};
use teensy4_bsp::interrupt;
use teensy4_bsp::ral;

/// The DMAMUX channel enable bit (`CHCFG[ENBL]`, bit 31), which the RAL's
/// field list for this chip leaves out.
const ENBL_MASK: u32 = 1 << 31;

/// Words of one ring slot: four times a `TCR` command and a DAC word, in the
/// order the DMA writes them to `TCR` and `TDR`.
pub(crate) const SLOT_WORDS: usize = 8;

/// Slots in the ring: a few periods of latency at most are ever used.
pub(crate) const SLOTS: usize = 8;

/// The DAC's words, one slot a period, read by the DMA and written by the
/// audio interrupt a few slots ahead of it. In DTCM, which the DMA reaches
/// and the data cache does not cover; aligned to its size, so the DMA's
/// address modulo cycles through it.
#[repr(C, align(256))]
struct Ring(UnsafeCell<[u32; SLOTS * SLOT_WORDS]>);

// SAFETY: the CPU side is only the audio interrupt (and `start` before the
// DMA runs), one context; the DMA reads whole words.
unsafe impl Sync for Ring {}

static RING: Ring = Ring(UnsafeCell::new([0; SLOTS * SLOT_WORDS]));

const _: () = assert!(
    SLOTS * SLOT_WORDS * 4 == 256,
    "the ring's alignment and the DMA's source modulo assume 256 bytes"
);

/// The source modulo for the ring: its size is 2^8 bytes.
const RING_MODULO: u8 = 8;

/// Writes `slot` of the ring. The caller writes slots the DMA will not read
/// before the next interrupt ([`crate::audio`] keeps two ahead).
pub(crate) fn write_slot(slot: usize, words: &[u32; SLOT_WORDS]) {
    let base = (slot % SLOTS) * SLOT_WORDS;
    for (offset, &word) in words.iter().enumerate() {
        // SAFETY: the index is within the ring; the only CPU writer is the
        // audio interrupt (one context); the DMA reads another slot, and
        // word-sized volatile stores are what it reads.
        unsafe {
            RING.0
                .get()
                .cast::<u32>()
                .add(base + offset)
                .write_volatile(word);
        }
    }
}

/// Programs `channel` to copy one slot of the ring to `TCR` and `TDR` of
/// LPSPI4 at each trigger of the PIT channel of the same number, cycling
/// through the ring forever, to raise its interrupt after each copy, and
/// enables it. `tdr` is LPSPI4's transmit data register; `TCR` is the word
/// before it.
pub(crate) fn start_dma(channel: &mut Channel, tdr: *const u32) {
    // A slot, 32 bytes: the cast cannot truncate.
    #[allow(clippy::cast_possible_truncation)]
    const SLOT_BYTES: u32 = (SLOT_WORDS * 4) as u32;
    let tcr = tdr.wrapping_sub(1);
    channel.reset();
    // SAFETY: the source is the static ring, read word by word for one slot
    // a trigger, its address kept inside the ring by the modulo (the ring
    // is aligned to its 256 bytes); the destination alternates between
    // `TCR` and `TDR`, the 8-byte window `DMOD = 3` keeps it to (0x60 and
    // 0x64 in LPSPI4, aligned), two registers that only queue into the
    // transmit FIFO. One trigger is a whole major loop, so its end raises
    // the interrupt. Nothing else uses this channel.
    unsafe {
        channel.set_source_address(RING.0.get().cast::<u32>().cast_const());
        channel.set_source_offset(4);
        channel.set_source_attributes::<u32>(RING_MODULO);
        channel.set_source_last_address_adjustment(0);
        channel.set_destination_address(tcr);
        channel.set_destination_offset(4);
        channel.set_destination_attributes::<u32>(3);
        channel.set_destination_last_address_adjustment(0);
        channel.set_minor_loop_bytes(SLOT_BYTES);
        channel.set_transfer_iterations(1);
    }
    channel.set_disable_on_completion(false);
    channel.set_interrupt_on_completion(true);
    // Always requesting, gated by the PIT channel of the same number: one
    // minor loop a timeout. `imxrt-dma` sets the trigger only with a source
    // slot (the RT1060's DMAMUX has no always-on slot, the first lot 7e
    // firmware saw no burst with slot 54) and the always-on bit only
    // without the trigger, so the register is written here.
    channel.set_channel_configuration(Configuration::Off);
    // SAFETY: a store to this channel's own DMAMUX configuration register,
    // which nothing else writes; the instance only aliases the one the
    // DMA channels were built from.
    unsafe {
        let dmamux = ral::dmamux::DMAMUX::instance();
        ral::write_reg!(
            ral::dmamux,
            dmamux,
            CHCFG[channel.channel()],
            ENBL_MASK | ral::dmamux::CHCFG::TRIG::mask | ral::dmamux::CHCFG::A_ON::mask
        );
    }
    // SAFETY: the transfer is fully described above and only touches the
    // static ring and LPSPI4's queueing registers.
    unsafe { channel.enable() };
}

/// Lets `channel`, programmed by [`start_dma`] and paused since, take its
/// triggers again, where it stopped in the ring.
pub(crate) fn resume_dma(channel: &Channel) {
    // SAFETY: the transfer is still the one `start_dma` described, over the
    // static ring and LPSPI4's queueing registers.
    unsafe { channel.enable() };
}

/// DMA channel 0 (and 16, unused): a DAC burst is queued, one audio
/// period.
#[interrupt]
fn DMA0_DMA16() {
    crate::audio::on_period();
}

/// LPSPI4: a screen chunk's frame is complete.
#[interrupt]
fn LPSPI4() {
    crate::audio::on_frame_complete();
}

/// Lets both interrupts through; [`crate::audio::start`] has put their
/// state in place.
pub(crate) fn enable() {
    // SAFETY: unmasking can break a critical section built on masking these
    // lines; the firmware's critical sections mask every interrupt instead
    // (`critical-section-single-core`, PRIMASK), and the handlers only take
    // state through them.
    unsafe {
        NVIC::unmask(interrupt::DMA0_DMA16);
        NVIC::unmask(interrupt::LPSPI4);
    }
}

/// Whether the period's line is enabled, and whether it is pending.
pub(crate) fn line_state() -> (bool, bool) {
    (
        NVIC::is_enabled(interrupt::DMA0_DMA16),
        NVIC::is_pending(interrupt::DMA0_DMA16),
    )
}

/// Blocks both interrupts again.
pub(crate) fn disable() {
    NVIC::mask(interrupt::DMA0_DMA16);
    NVIC::mask(interrupt::LPSPI4);
}
