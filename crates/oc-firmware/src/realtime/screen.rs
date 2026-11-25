//! The screen's side of the real-time path (journal/D-033): the frame the
//! engine draws, handed over whole and sent to the OLED piece by piece
//! between two DAC bursts.

use teensy4_bsp::hal::gpio::Output;
use teensy4_bsp::hal::lpspi::{Interrupts, Status, Transaction};

use oc_core::framebuffer::{FrameBuffer, LEN};
use oc_core::platform::Display;
use oc_core::profile::Stat;
use oc_drivers::fifo;
use oc_drivers::ssd130x::{ChunkKind, Controller, FrameStream};

use super::{Bus, MAILBOX};
use crate::board;

/// The screen's half of the mailbox: the frame and contrast the loop leaves,
/// what the interrupt sent of them.
pub(super) struct ScreenMail {
    frame: [u8; LEN],
    /// Set when `frame` holds a frame the interrupt has not taken.
    frame_ready: bool,
    contrast: Option<u8>,
    /// Screen chunks sent, and OLED releases after them, wrapping.
    pub(super) chunks: (u32, u32),
    /// Whole frames sent to the screen, wrapping.
    pub(super) frames: u32,
    /// A frame's transfer, in core cycles.
    pub(super) frame_time: Stat,
}

impl ScreenMail {
    /// Nothing to show yet.
    pub(super) const fn new() -> Self {
        Self {
            frame: [0; LEN],
            frame_ready: false,
            contrast: None,
            chunks: (0, 0),
            frames: 0,
            frame_time: Stat::new(),
        }
    }
}

/// The screen's side: the OLED's control lines and the frame going out,
/// piece by piece between two bursts.
pub(super) struct Screen {
    oled_cs: Output,
    oled_dc: Output,
    stream: FrameStream,
    /// Whether a piece of the screen is on the bus, the OLED selected.
    pub(super) chunk_active: bool,
    /// Cycle count when the frame going out was taken.
    frame_started: Option<u32>,
}

impl Screen {
    /// The OLED's lines, its chip select released, nothing going out.
    pub(super) fn new(oled_cs: Output, oled_dc: Output, controller: Controller) -> Self {
        oled_cs.set();
        Self {
            oled_cs,
            oled_dc,
            stream: FrameStream::new(controller),
            chunk_active: false,
            frame_started: None,
        }
    }

    /// Sets the contrast the loop asked for, and, between two frames, counts
    /// the one sent and takes the next the loop left, at `entry`.
    pub(super) fn take_frame(&mut self, mail: &mut ScreenMail, entry: u32) {
        if let Some(contrast) = mail.contrast.take() {
            self.stream.set_contrast(contrast);
        }
        if self.stream.is_idle() && !self.chunk_active {
            if let Some(started) = self.frame_started.take() {
                mail.frame_time.record(entry.wrapping_sub(started));
                mail.frames = mail.frames.wrapping_add(1);
            }
            if mail.frame_ready {
                self.stream.start(&mail.frame);
                mail.frame_ready = false;
                self.frame_started = Some(entry);
            }
        }
    }

    /// Queues the next piece of the screen on `bus`, if any, and arms the
    /// frame-complete interrupt that releases the OLED; whether it did.
    pub(super) fn send_chunk(&mut self, bus: &mut Bus) -> bool {
        let Some(chunk) = self.stream.next_chunk(board::OLED_CHUNK_BYTES) else {
            return false;
        };
        bus.use_clock(bus.clocks.oled);
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
        bus.spi.clear_status(Status::FRAME_COMPLETE);
        bus.spi.set_interrupts(Interrupts::FRAME_COMPLETE);
        bus.spi.enqueue_transaction(transaction);
        for word in fifo::words(chunk.bytes) {
            bus.spi.enqueue_data(word);
        }
        self.chunk_active = true;
        true
    }

    /// Releases the OLED once its piece is out, and gives `bus` the DAC's
    /// timing back before the next burst; whether a piece was out.
    pub(super) fn release(&mut self, bus: &mut Bus) -> bool {
        bus.spi.clear_status(Status::FRAME_COMPLETE);
        bus.spi.set_interrupts(Interrupts::empty());
        if !self.chunk_active {
            return false;
        }
        // The chip select goes through GPIO2 and the DAC's words through
        // LPSPI4, two buses that do not order their writes: the release is
        // waited for (journal/D-033, lot 7c on the module).
        self.oled_cs.set();
        settle(&self.oled_cs, true);
        self.chunk_active = false;
        bus.use_clock(bus.clocks.dac);
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

/// The engine's display on the module: the frame it draws is copied for the
/// interrupt to send, the latest one winning.
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
            let mail = &mut MAILBOX.borrow_ref_mut(cs).screen;
            mail.frame = *self.frame.as_bytes();
            mail.frame_ready = true;
        });
    }

    fn set_contrast(&mut self, contrast: u8) {
        critical_section::with(|cs| MAILBOX.borrow_ref_mut(cs).screen.contrast = Some(contrast));
    }
}

impl core::fmt::Debug for MailboxDisplay {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("MailboxDisplay").finish_non_exhaustive()
    }
}
