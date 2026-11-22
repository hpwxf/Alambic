//! A blocking SPI bus whose `flush` waits for the last bit.
//!
//! `imxrt-hal`'s `Lpspi::flush` returns once the controller is not busy and
//! its transmit FIFO is empty. Its own comment says why that can be early: the
//! busy flag may not be set yet when the FIFO has just emptied into the shift
//! register. A driver that releases its chip select after `flush` then cuts
//! the last word short; for a single-byte command, the whole command is lost.
//! The OLED's `DISPLAY_ON` is such a command, sent last at initialisation:
//! lost, the panel stays dark although every frame reaches it.
//!
//! [`FramedSpi`] waits, after the HAL's `flush`, for the frame-complete flag
//! of the transaction it started (each write is one frame), as the audio
//! interrupt already does for its screen chunks (journal/D-033).

use embedded_hal::spi::{ErrorType, SpiBus};
use teensy4_bsp::hal::lpspi::{Lpspi, Status};

/// LPSPI4 for the blocking drivers, its `flush` waiting for the last bit.
pub(crate) struct FramedSpi {
    spi: Lpspi,
    /// Whether a transaction was started since the last `flush`: without one,
    /// the frame-complete flag would never come.
    pending: bool,
}

impl FramedSpi {
    /// Wraps `spi`.
    pub(crate) const fn new(spi: Lpspi) -> Self {
        Self {
            spi,
            pending: false,
        }
    }

    /// The bus itself, for the audio interrupt.
    pub(crate) fn into_inner(self) -> Lpspi {
        self.spi
    }

    /// The bus, for its configuration.
    pub(crate) fn inner_mut(&mut self) -> &mut Lpspi {
        &mut self.spi
    }

    /// Clears the frame-complete flag before a transaction that sends `len`
    /// words, so that `flush` waits for this one.
    fn start(&mut self, len: usize) {
        if len > 0 {
            self.spi.clear_status(Status::FRAME_COMPLETE);
            self.pending = true;
        }
    }
}

impl ErrorType for FramedSpi {
    type Error = <Lpspi as ErrorType>::Error;
}

impl SpiBus<u8> for FramedSpi {
    fn read(&mut self, words: &mut [u8]) -> Result<(), Self::Error> {
        self.start(words.len());
        SpiBus::<u8>::read(&mut self.spi, words)
    }

    fn write(&mut self, words: &[u8]) -> Result<(), Self::Error> {
        self.start(words.len());
        SpiBus::<u8>::write(&mut self.spi, words)
    }

    fn transfer(&mut self, read: &mut [u8], write: &[u8]) -> Result<(), Self::Error> {
        self.start(read.len().max(write.len()));
        SpiBus::<u8>::transfer(&mut self.spi, read, write)
    }

    fn transfer_in_place(&mut self, words: &mut [u8]) -> Result<(), Self::Error> {
        self.start(words.len());
        SpiBus::<u8>::transfer_in_place(&mut self.spi, words)
    }

    fn flush(&mut self) -> Result<(), Self::Error> {
        SpiBus::<u8>::flush(&mut self.spi)?;
        if self.pending {
            while !self.spi.status().intersects(Status::FRAME_COMPLETE) {
                core::hint::spin_loop();
            }
            self.pending = false;
        }
        Ok(())
    }
}
