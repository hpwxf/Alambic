//! Driver for the module's 128x64 monochrome OLED.
//!
//! Ornament & Crime panels ship with either an SSD1306 or an SSD1309
//! controller. They speak the same command set; only a handful of
//! initialisation values differ, chiefly the charge-pump and pre-charge
//! settings, because the SSD1309 expects an external supply while the SSD1306
//! usually generates its own. Sending the wrong sequence gives a *blank* screen
//! with no other symptom, which is exactly the kind of failure the diagnostic
//! applet exists to make visible, so the controller is selectable at run time
//! rather than baked in.
//!
//! The framebuffer already uses the controller's own byte order (see
//! [`oc_core::framebuffer`]), so a refresh is one command sequence followed by
//! a single 1024-byte write.

use embedded_hal::delay::DelayNs;
use embedded_hal::digital::OutputPin;
use embedded_hal::spi::SpiBus;

use oc_core::framebuffer::{FrameBuffer, LEN, PAGES, WIDTH};
use oc_core::platform::Display;

/// Commands used by this driver, from the SSD1306 and SSD1309 data sheets.
mod command {
    /// Turn the panel off.
    pub(super) const DISPLAY_OFF: u8 = 0xAE;
    /// Turn the panel on.
    pub(super) const DISPLAY_ON: u8 = 0xAF;
    /// Set the display clock divide ratio and oscillator frequency.
    pub(super) const SET_CLOCK_DIVIDE: u8 = 0xD5;
    /// Set the multiplex ratio, i.e. the number of active rows.
    pub(super) const SET_MULTIPLEX: u8 = 0xA8;
    /// Set the display offset in rows.
    pub(super) const SET_DISPLAY_OFFSET: u8 = 0xD3;
    /// Set the display start line; the low nibble is the line number.
    pub(super) const SET_START_LINE: u8 = 0x40;
    /// Configure the internal charge pump (SSD1306 only).
    pub(super) const SET_CHARGE_PUMP: u8 = 0x8D;
    /// Select the memory addressing mode.
    pub(super) const SET_MEMORY_MODE: u8 = 0x20;
    /// Mirror the columns so that column 127 is the right-hand side.
    pub(super) const SET_SEGMENT_REMAP: u8 = 0xA1;
    /// Scan the rows from bottom to top.
    pub(super) const SET_COM_SCAN_REVERSE: u8 = 0xC8;
    /// Set the pin configuration of the row drivers.
    pub(super) const SET_COM_PINS: u8 = 0xDA;
    /// Set the contrast.
    pub(super) const SET_CONTRAST: u8 = 0x81;
    /// Set the pre-charge period.
    pub(super) const SET_PRECHARGE: u8 = 0xD9;
    /// Set the V-COMH deselect level.
    pub(super) const SET_VCOM_DESELECT: u8 = 0xDB;
    /// Show the framebuffer rather than an all-on test pattern.
    pub(super) const DISPLAY_FROM_RAM: u8 = 0xA4;
    /// Normal, non-inverted pixels.
    pub(super) const DISPLAY_NORMAL: u8 = 0xA6;
    /// Set the column address range.
    pub(super) const SET_COLUMN_ADDRESS: u8 = 0x21;
    /// Set the page address range.
    pub(super) const SET_PAGE_ADDRESS: u8 = 0x22;
}

/// Which OLED controller the panel uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Controller {
    /// SSD1306: generates its display supply with an internal charge pump.
    #[default]
    Ssd1306,
    /// SSD1309: expects an external display supply, so the charge pump must
    /// stay off and the pre-charge timing differs.
    Ssd1309,
}

impl Controller {
    /// Human-readable name, for the boot banner.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Ssd1306 => "SSD1306",
            Self::Ssd1309 => "SSD1309",
        }
    }

    /// Charge-pump argument: enabled on the SSD1306, disabled on the SSD1309.
    const fn charge_pump(self) -> u8 {
        match self {
            Self::Ssd1306 => 0x14,
            Self::Ssd1309 => 0x10,
        }
    }

    /// Pre-charge period. The SSD1309's external supply settles faster.
    const fn precharge(self) -> u8 {
        match self {
            Self::Ssd1306 => 0xF1,
            Self::Ssd1309 => 0x22,
        }
    }
}

/// Errors the driver can report.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ssd130xError {
    /// The SPI transfer failed.
    Bus,
    /// A control pin could not be driven.
    Pin,
}

/// A 128x64 OLED on a four-wire SPI bus.
///
/// `CS` is the chip select, `DC` selects data (high) or command (low), and
/// `RST` is the active-low reset.
#[derive(Debug)]
pub struct Ssd130x<SPI, CS, DC, RST> {
    spi: SPI,
    chip_select: CS,
    data_command: DC,
    reset: RST,
    controller: Controller,
    frame: FrameBuffer,
    last_error: Option<Ssd130xError>,
}

impl<SPI, CS, DC, RST> Ssd130x<SPI, CS, DC, RST>
where
    SPI: SpiBus<u8>,
    CS: OutputPin,
    DC: OutputPin,
    RST: OutputPin,
{
    /// Wraps the bus and control pins. Call [`Self::init`] before drawing.
    pub fn new(
        spi: SPI,
        chip_select: CS,
        data_command: DC,
        reset: RST,
        controller: Controller,
    ) -> Self {
        Self {
            spi,
            chip_select,
            data_command,
            reset,
            controller,
            frame: FrameBuffer::new(),
            last_error: None,
        }
    }

    /// Which controller this instance drives.
    pub const fn controller(&self) -> Controller {
        self.controller
    }

    /// The most recent error, if the panel is not being refreshed.
    pub const fn last_error(&self) -> Option<Ssd130xError> {
        self.last_error
    }

    /// Pulses the reset line and sends the initialisation sequence.
    ///
    /// # Errors
    ///
    /// Returns an error if a control pin or the SPI bus fails.
    pub fn init(&mut self, delay: &mut impl DelayNs) -> Result<(), Ssd130xError> {
        self.reset.set_high().map_err(|_| Ssd130xError::Pin)?;
        delay.delay_ms(1);
        self.reset.set_low().map_err(|_| Ssd130xError::Pin)?;
        delay.delay_ms(10);
        self.reset.set_high().map_err(|_| Ssd130xError::Pin)?;
        delay.delay_ms(10);

        let multiplex = u8::try_from(PAGES * 8 - 1).unwrap_or(0x3F);
        self.commands(&[
            command::DISPLAY_OFF,
            command::SET_CLOCK_DIVIDE,
            0x80,
            command::SET_MULTIPLEX,
            multiplex,
            command::SET_DISPLAY_OFFSET,
            0x00,
            command::SET_START_LINE,
            command::SET_CHARGE_PUMP,
            self.controller.charge_pump(),
            // Horizontal addressing: the controller wraps from the end of a
            // page to the start of the next, so the whole framebuffer goes out
            // in one transfer.
            command::SET_MEMORY_MODE,
            0x00,
            command::SET_SEGMENT_REMAP,
            command::SET_COM_SCAN_REVERSE,
            command::SET_COM_PINS,
            0x12,
            command::SET_CONTRAST,
            0xCF,
            command::SET_PRECHARGE,
            self.controller.precharge(),
            command::SET_VCOM_DESELECT,
            0x40,
            command::DISPLAY_FROM_RAM,
            command::DISPLAY_NORMAL,
        ])?;

        self.flush_frame()?;
        self.commands(&[command::DISPLAY_ON])
    }

    /// Sends the framebuffer to the panel.
    ///
    /// # Errors
    ///
    /// Returns an error if a control pin or the SPI bus fails.
    pub fn flush_frame(&mut self) -> Result<(), Ssd130xError> {
        let last_column = u8::try_from(WIDTH - 1).unwrap_or(127);
        let last_page = u8::try_from(PAGES - 1).unwrap_or(7);
        self.commands(&[
            command::SET_COLUMN_ADDRESS,
            0,
            last_column,
            command::SET_PAGE_ADDRESS,
            0,
            last_page,
        ])?;

        self.data_command
            .set_high()
            .map_err(|_| Ssd130xError::Pin)?;

        // The framebuffer is borrowed immutably while the bus is borrowed
        // mutably, so it is moved out and put back rather than copied.
        let frame = core::mem::replace(&mut self.frame, FrameBuffer::new());
        let outcome = self.transfer(frame.as_bytes());
        self.frame = frame;
        outcome
    }

    /// Sends a command sequence.
    fn commands(&mut self, bytes: &[u8]) -> Result<(), Ssd130xError> {
        self.data_command.set_low().map_err(|_| Ssd130xError::Pin)?;
        self.transfer(bytes)
    }

    /// Asserts the chip select, writes `bytes`, and always releases it again.
    fn transfer(&mut self, bytes: &[u8]) -> Result<(), Ssd130xError> {
        self.chip_select.set_low().map_err(|_| Ssd130xError::Pin)?;
        let written = self.spi.write(bytes).map_err(|_| Ssd130xError::Bus);
        let released = self.chip_select.set_high().map_err(|_| Ssd130xError::Pin);

        // Releasing the chip select even after a failure keeps the shared bus
        // usable for the DAC.
        written.and(released)
    }
}

impl<SPI, CS, DC, RST> Display for Ssd130x<SPI, CS, DC, RST>
where
    SPI: SpiBus<u8>,
    CS: OutputPin,
    DC: OutputPin,
    RST: OutputPin,
{
    fn frame_mut(&mut self) -> &mut FrameBuffer {
        &mut self.frame
    }

    fn present(&mut self) {
        // The trait cannot report failures; record them so the firmware can
        // report a dead panel instead of appearing to work.
        self.last_error = self.flush_frame().err();
    }
}

/// Compile-time reminder that a refresh is a single full-buffer transfer.
const _: () = assert!(LEN == WIDTH * PAGES);

#[cfg(test)]
mod tests {
    use std::vec::Vec;

    use embedded_hal::delay::DelayNs;
    use embedded_hal::digital::{ErrorType as PinErrorType, OutputPin};
    use embedded_hal::spi::{ErrorType as SpiErrorType, SpiBus};

    use oc_core::framebuffer::LEN;
    use oc_core::platform::Display;

    use super::{Controller, Ssd130x, Ssd130xError, command};

    /// An SPI bus that records every byte written.
    #[derive(Debug, Default)]
    struct RecordingBus {
        written: Vec<u8>,
        fail: bool,
    }

    impl SpiErrorType for RecordingBus {
        type Error = embedded_hal::spi::ErrorKind;
    }

    impl SpiBus<u8> for RecordingBus {
        fn read(&mut self, _words: &mut [u8]) -> Result<(), Self::Error> {
            Ok(())
        }

        fn write(&mut self, words: &[u8]) -> Result<(), Self::Error> {
            if self.fail {
                return Err(embedded_hal::spi::ErrorKind::Other);
            }
            self.written.extend_from_slice(words);
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

    /// A pin that records the sequence of levels it was driven to.
    #[derive(Debug, Default)]
    struct RecordingPin {
        levels: Vec<bool>,
    }

    impl PinErrorType for RecordingPin {
        type Error = core::convert::Infallible;
    }

    impl OutputPin for RecordingPin {
        fn set_low(&mut self) -> Result<(), Self::Error> {
            self.levels.push(false);
            Ok(())
        }

        fn set_high(&mut self) -> Result<(), Self::Error> {
            self.levels.push(true);
            Ok(())
        }
    }

    /// A delay that records how long it was asked to wait.
    #[derive(Debug, Default)]
    struct RecordingDelay {
        waited_ns: u64,
    }

    impl DelayNs for RecordingDelay {
        fn delay_ns(&mut self, ns: u32) {
            self.waited_ns += u64::from(ns);
        }
    }

    type TestPanel = Ssd130x<RecordingBus, RecordingPin, RecordingPin, RecordingPin>;

    fn panel(controller: Controller) -> TestPanel {
        Ssd130x::new(
            RecordingBus::default(),
            RecordingPin::default(),
            RecordingPin::default(),
            RecordingPin::default(),
            controller,
        )
    }

    #[test]
    fn initialisation_ends_with_the_panel_switched_on() {
        let mut panel = panel(Controller::Ssd1306);
        let mut delay = RecordingDelay::default();
        panel.init(&mut delay).unwrap();

        assert_eq!(panel.spi.written.first(), Some(&command::DISPLAY_OFF));
        assert_eq!(panel.spi.written.last(), Some(&command::DISPLAY_ON));
    }

    #[test]
    fn initialisation_pulses_the_reset_line() {
        let mut panel = panel(Controller::Ssd1306);
        let mut delay = RecordingDelay::default();
        panel.init(&mut delay).unwrap();

        assert_eq!(
            panel.reset.levels,
            [true, false, true],
            "the reset must be pulsed low, not merely held high"
        );
        assert!(delay.waited_ns > 0, "the controller needs settling time");
    }

    #[test]
    fn the_two_controllers_differ_only_where_expected() {
        let mut delay = RecordingDelay::default();
        let mut ssd1306 = panel(Controller::Ssd1306);
        let mut ssd1309 = panel(Controller::Ssd1309);
        ssd1306.init(&mut delay).unwrap();
        ssd1309.init(&mut delay).unwrap();

        let differences = ssd1306
            .spi
            .written
            .iter()
            .zip(ssd1309.spi.written.iter())
            .filter(|(a, b)| a != b)
            .count();
        assert_eq!(
            differences, 2,
            "only the charge pump and pre-charge arguments should differ"
        );
    }

    #[test]
    fn the_charge_pump_is_off_on_the_ssd1309() {
        let mut delay = RecordingDelay::default();
        let mut ssd1309 = panel(Controller::Ssd1309);
        ssd1309.init(&mut delay).unwrap();

        let position = ssd1309
            .spi
            .written
            .iter()
            .position(|&byte| byte == command::SET_CHARGE_PUMP)
            .expect("the sequence must configure the charge pump");
        assert_eq!(
            ssd1309.spi.written[position + 1],
            0x10,
            "an externally supplied panel must not enable the internal pump"
        );
    }

    #[test]
    fn a_refresh_sends_the_whole_framebuffer_once() {
        let mut panel = panel(Controller::Ssd1306);
        panel.frame_mut().fill();
        panel.present();

        assert_eq!(panel.last_error(), None);
        let data = &panel.spi.written[panel.spi.written.len() - LEN..];
        assert_eq!(data.len(), LEN);
        assert!(
            data.iter().all(|&byte| byte == 0xFF),
            "a filled framebuffer must reach the panel unchanged"
        );
    }

    #[test]
    fn a_refresh_addresses_the_full_screen_before_the_data() {
        let mut panel = panel(Controller::Ssd1306);
        panel.present();

        let header = &panel.spi.written[..6];
        assert_eq!(
            header,
            [
                command::SET_COLUMN_ADDRESS,
                0,
                127,
                command::SET_PAGE_ADDRESS,
                0,
                7
            ]
            .as_slice()
        );
    }

    #[test]
    fn data_and_command_are_distinguished_by_the_dc_pin() {
        let mut panel = panel(Controller::Ssd1306);
        panel.present();

        assert_eq!(
            panel.data_command.levels,
            [false, true],
            "the address commands go out low, the pixel data high"
        );
    }

    #[test]
    fn a_failing_bus_is_recorded_and_the_chip_select_released() {
        let mut panel = panel(Controller::Ssd1306);
        panel.spi.fail = true;
        panel.present();

        assert_eq!(panel.last_error(), Some(Ssd130xError::Bus));
        assert_eq!(
            panel.chip_select.levels.last(),
            Some(&true),
            "the chip select must end released even after a failure"
        );
    }

    #[test]
    fn controller_names_are_reported_for_the_boot_banner() {
        assert_eq!(Controller::Ssd1306.name(), "SSD1306");
        assert_eq!(Controller::Ssd1309.name(), "SSD1309");
        assert_eq!(panel(Controller::Ssd1309).controller(), Controller::Ssd1309);
    }
}
