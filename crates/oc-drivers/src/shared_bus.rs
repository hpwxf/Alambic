//! Sharing one SPI bus between the DAC and the OLED.
//!
//! Both peripherals hang off the same three wires and are distinguished only by
//! their chip selects, so the drivers cannot each own the bus. `embedded-hal`
//! answers this with `SpiDevice`, but that trait bundles chip-select management,
//! and these drivers must drive their own: the OLED needs its data/command line
//! toggled *inside* a selected window, and the DAC's word framing is its own.
//!
//! [`SharedBus`] is therefore the minimal thing that works: a handle that
//! borrows the bus for the duration of a single transfer. Because the firmware
//! is single-threaded and cooperative, and a transfer never yields, the borrow
//! can never actually be contended; a contended borrow would be a bug, so it is
//! reported rather than ignored.
//!
//! A handle can also carry a *selection*, run on the bus before each of its
//! operations: what one peripheral needs of the controller and the other must
//! not inherit. On the module the DAC's chip select is the controller's own
//! (PCS0), which it asserts on every transaction, so the OLED's transactions
//! must pick a chip select no pad carries; each may want its own clock too
//! (journal/D-033). [`ControllerChipSelect`] stands for a chip select the
//! controller drives, for a driver that expects a pin.

use core::cell::RefCell;

use core::convert::Infallible;

use embedded_hal::digital::{ErrorType as PinErrorType, OutputPin};
use embedded_hal::spi::{ErrorKind, ErrorType, SpiBus};

/// A handle onto a bus shared with other peripherals.
#[derive(Debug)]
pub struct SharedBus<'bus, SPI> {
    bus: &'bus RefCell<SPI>,
    /// Run on the bus before each operation of this handle.
    select: Option<fn(&mut SPI)>,
}

impl<'bus, SPI> SharedBus<'bus, SPI> {
    /// Creates another handle onto `bus`, taking the bus as it is.
    pub const fn new(bus: &'bus RefCell<SPI>) -> Self {
        Self { bus, select: None }
    }

    /// Creates another handle onto `bus` that runs `select` on it before each
    /// of its operations: the chip select, clock or mode its peripheral needs.
    pub const fn with_select(bus: &'bus RefCell<SPI>, select: fn(&mut SPI)) -> Self {
        Self {
            bus,
            select: Some(select),
        }
    }
}

impl<SPI> Clone for SharedBus<'_, SPI> {
    fn clone(&self) -> Self {
        Self {
            bus: self.bus,
            select: self.select,
        }
    }
}

/// A chip select driven by the SPI controller itself, for a driver that
/// expects a pin: setting it does nothing, the controller frames each
/// transaction on its own line.
#[derive(Debug, Clone, Copy, Default)]
pub struct ControllerChipSelect;

impl PinErrorType for ControllerChipSelect {
    type Error = Infallible;
}

impl OutputPin for ControllerChipSelect {
    fn set_low(&mut self) -> Result<(), Self::Error> {
        Ok(())
    }

    fn set_high(&mut self) -> Result<(), Self::Error> {
        Ok(())
    }
}

impl<SPI> ErrorType for SharedBus<'_, SPI>
where
    SPI: SpiBus<u8>,
{
    /// Errors are flattened to [`ErrorKind`] so that the handle's type does not
    /// depend on the concrete bus.
    type Error = ErrorKind;
}

impl<SPI> SpiBus<u8> for SharedBus<'_, SPI>
where
    SPI: SpiBus<u8>,
{
    fn read(&mut self, words: &mut [u8]) -> Result<(), Self::Error> {
        self.with_bus(|bus| bus.read(words))
    }

    fn write(&mut self, words: &[u8]) -> Result<(), Self::Error> {
        self.with_bus(|bus| bus.write(words))
    }

    fn transfer(&mut self, read: &mut [u8], write: &[u8]) -> Result<(), Self::Error> {
        self.with_bus(|bus| bus.transfer(read, write))
    }

    fn transfer_in_place(&mut self, words: &mut [u8]) -> Result<(), Self::Error> {
        self.with_bus(|bus| bus.transfer_in_place(words))
    }

    fn flush(&mut self) -> Result<(), Self::Error> {
        self.with_bus(SpiBus::flush)
    }
}

impl<SPI> SharedBus<'_, SPI>
where
    SPI: SpiBus<u8>,
{
    /// Runs one operation with exclusive access to the bus.
    fn with_bus<T>(
        &self,
        operation: impl FnOnce(&mut SPI) -> Result<T, SPI::Error>,
    ) -> Result<T, ErrorKind> {
        use embedded_hal::spi::Error as _;

        let Ok(mut bus) = self.bus.try_borrow_mut() else {
            // Reachable only if a driver called back into the bus from inside a
            // transfer, which the cooperative loop makes impossible.
            return Err(ErrorKind::Other);
        };
        if let Some(select) = self.select {
            select(&mut bus);
        }
        operation(&mut bus).map_err(|error| error.kind())
    }
}

#[cfg(test)]
mod tests {
    use std::vec::Vec;

    use core::cell::RefCell;

    use embedded_hal::spi::{ErrorType, SpiBus};

    use super::SharedBus;

    /// A bus that records every byte written, and which device it was
    /// selected for when it was.
    #[derive(Debug, Default)]
    struct RecordingBus {
        written: Vec<u8>,
        device: u8,
        writes: Vec<(u8, Vec<u8>)>,
    }

    impl ErrorType for RecordingBus {
        type Error = embedded_hal::spi::ErrorKind;
    }

    impl SpiBus<u8> for RecordingBus {
        fn read(&mut self, _words: &mut [u8]) -> Result<(), Self::Error> {
            Ok(())
        }

        fn write(&mut self, words: &[u8]) -> Result<(), Self::Error> {
            self.written.extend_from_slice(words);
            self.writes.push((self.device, words.to_vec()));
            Ok(())
        }

        fn transfer(&mut self, _read: &mut [u8], _write: &[u8]) -> Result<(), Self::Error> {
            Ok(())
        }

        fn transfer_in_place(&mut self, _words: &mut [u8]) -> Result<(), Self::Error> {
            Ok(())
        }

        fn flush(&mut self) -> Result<(), Self::Error> {
            Ok(())
        }
    }

    #[test]
    fn two_handles_write_to_the_same_bus_in_order() {
        let bus = RefCell::new(RecordingBus::default());
        let mut first = SharedBus::new(&bus);
        let mut second = SharedBus::new(&bus);

        first.write(&[1, 2]).unwrap();
        second.write(&[3]).unwrap();
        first.write(&[4]).unwrap();

        assert_eq!(bus.borrow().written, [1, 2, 3, 4]);
    }

    #[test]
    fn a_handle_can_be_cloned() {
        let bus = RefCell::new(RecordingBus::default());
        let first = SharedBus::new(&bus);
        let mut second = first.clone();
        second.write(&[7]).unwrap();
        assert_eq!(bus.borrow().written, [7]);
    }

    #[test]
    fn a_contended_borrow_is_reported_rather_than_panicking() {
        let bus = RefCell::new(RecordingBus::default());
        let mut handle = SharedBus::new(&bus);

        let held = bus.borrow_mut();
        assert!(
            handle.write(&[1]).is_err(),
            "re-entering the bus must fail cleanly, not panic"
        );
        drop(held);

        assert!(handle.write(&[1]).is_ok(), "and must recover afterwards");
    }

    #[test]
    fn each_handle_selects_its_device_before_each_operation() {
        let bus = RefCell::new(RecordingBus::default());
        let mut dac = SharedBus::with_select(&bus, |bus| bus.device = 1);
        let mut oled = SharedBus::with_select(&bus, |bus| bus.device = 2);
        let mut plain = SharedBus::new(&bus);

        dac.write(&[1]).unwrap();
        oled.write(&[2]).unwrap();
        dac.write(&[3]).unwrap();
        // A handle without a selection takes the bus as the last one left it.
        plain.write(&[4]).unwrap();

        let devices: Vec<u8> = bus
            .borrow()
            .writes
            .iter()
            .map(|(device, _)| *device)
            .collect();
        assert_eq!(devices, [1, 2, 1, 1]);
    }

    #[test]
    fn a_cloned_handle_keeps_its_selection() {
        let bus = RefCell::new(RecordingBus::default());
        let mut copy = SharedBus::with_select(&bus, |bus| bus.device = 5).clone();
        copy.write(&[0]).unwrap();
        assert_eq!(bus.borrow().writes[0].0, 5);
    }

    #[test]
    fn flushing_reaches_the_underlying_bus() {
        let bus = RefCell::new(RecordingBus::default());
        let mut handle = SharedBus::new(&bus);
        assert!(handle.flush().is_ok());
    }
}
