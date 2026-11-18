//! Driver for the module's 128x64 monochrome OLED.
//!
//! The stock Ornament & Crime panel (including the TLM Audio Teensy 4.0 build)
//! is driven exactly like the reference firmware: an **SH1106-class** controller
//! with 132×64 GDDRAM. The visible 128 columns are centred with a column offset
//! of 2, and each of the eight pages is addressed individually before its 128
//! data bytes go out. Treating it as a plain SSD1306 (horizontal full-frame
//! write, columns 0..127) produces a garbled image — often only the top page
//! looks "alive" — which is the failure mode this driver exists to avoid.
//!
//! Some third-party panels really are SSD1306 or SSD1309. Those speak the same
//! basic command set; only the charge-pump / pre-charge values and the way a
//! frame is pushed differ. The controller is therefore selected when the driver
//! is constructed (the firmware wires it from a Cargo feature).
//!
//! The framebuffer already uses the controller's own byte order (see
//! [`oc_core::framebuffer`]), so no repacking is needed on the way to the bus.
//!
//! [`Ssd130x`] sends over a blocking bus; [`FrameStream`] hands the same bytes
//! out in pieces, for a sender that interleaves them with other traffic, such
//! as the firmware's audio interrupt sharing the bus with the DAC
//! (journal/D-033).

use embedded_hal::delay::DelayNs;
use embedded_hal::digital::OutputPin;
use embedded_hal::spi::SpiBus;

use oc_core::framebuffer::{FrameBuffer, LEN, PAGES, WIDTH};
use oc_core::platform::Display;

/// Commands used by this driver, from the SH1106 / SSD1306 / SSD1309 data sheets.
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
    /// Configure the internal charge pump (SSD1306 / SH1106 panels that need it).
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
    /// Deactivate any scrolling setup left over from a previous firmware.
    pub(super) const DEACTIVATE_SCROLL: u8 = 0x2E;
    /// Show the framebuffer rather than an all-on test pattern.
    pub(super) const DISPLAY_FROM_RAM: u8 = 0xA4;
    /// Normal, non-inverted pixels.
    pub(super) const DISPLAY_NORMAL: u8 = 0xA6;
    /// Set the column address range (SSD1306 / SSD1309 horizontal mode).
    pub(super) const SET_COLUMN_ADDRESS: u8 = 0x21;
    /// Set the page address range (SSD1306 / SSD1309 horizontal mode).
    pub(super) const SET_PAGE_ADDRESS: u8 = 0x22;
    /// Page-mode: set the page start address (OR with the page index 0..7).
    pub(super) const SET_PAGE_START: u8 = 0xB0;
    /// Page-mode: set the upper nibble of the column start address (OR with nibble).
    pub(super) const SET_HIGH_COLUMN: u8 = 0x10;
    /// Page-mode: set the lower nibble of the column start address (OR with nibble).
    pub(super) const SET_LOW_COLUMN: u8 = 0x00;
}

/// Which OLED controller the panel uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Controller {
    /// SH1106-class panel used by stock Ornament & Crime (132×64 GDDRAM,
    /// column offset 2, page-addressed refresh). This is the default.
    #[default]
    Sh1106,
    /// SSD1306: 128×64 GDDRAM, horizontal full-frame refresh, internal charge pump.
    Ssd1306,
    /// SSD1309: like the SSD1306 but expects an external display supply, so the
    /// charge pump must stay off and the pre-charge timing differs.
    Ssd1309,
}

impl Controller {
    /// Human-readable name, for the boot banner.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Sh1106 => "SH1106",
            Self::Ssd1306 => "SSD1306",
            Self::Ssd1309 => "SSD1309",
        }
    }

    /// Charge-pump argument: on for SH1106/SSD1306, off for SSD1309.
    const fn charge_pump(self) -> u8 {
        match self {
            Self::Sh1106 | Self::Ssd1306 => 0x14,
            Self::Ssd1309 => 0x10,
        }
    }

    /// Pre-charge period. The SSD1309's external supply settles faster.
    const fn precharge(self) -> u8 {
        match self {
            Self::Sh1106 | Self::Ssd1306 => 0xF1,
            Self::Ssd1309 => 0x22,
        }
    }

    /// Whether frames are pushed page-by-page with a column offset (SH1106).
    const fn page_addressed(self) -> bool {
        matches!(self, Self::Sh1106)
    }

    /// Column offset into the controller GDDRAM for the left-most visible pixel.
    ///
    /// SH1106 RAM is 132 columns wide; the reference firmware centres the 128
    /// visible columns at offset 2. SSD1306/SSD1309 RAM is exactly 128 wide.
    const fn column_offset(self) -> u8 {
        match self {
            Self::Sh1106 => 2,
            Self::Ssd1306 | Self::Ssd1309 => 0,
        }
    }
}

/// The longest page-addressing command sequence ([`page_commands`]).
const MAX_PAGE_COMMANDS: usize = 6;

/// The commands that point the controller at `page` before its 128 data
/// bytes, and how many of the six bytes they use.
///
/// SH1106: the column offset, then the page (`SH1106_128x64_Driver::SendPage`
/// in the reference firmware). SSD1306 / SSD1309: the column and page window
/// of that page alone.
fn page_commands(controller: Controller, page: usize) -> ([u8; MAX_PAGE_COMMANDS], usize) {
    let page = u8::try_from(page).unwrap_or(0);
    if controller.page_addressed() {
        let offset = controller.column_offset();
        (
            [
                command::SET_HIGH_COLUMN | (offset >> 4),
                command::SET_LOW_COLUMN | (offset & 0x0F),
                command::SET_PAGE_START | page,
                0,
                0,
                0,
            ],
            3,
        )
    } else {
        let last_column = u8::try_from(WIDTH - 1).unwrap_or(127);
        (
            [
                command::SET_COLUMN_ADDRESS,
                0,
                last_column,
                command::SET_PAGE_ADDRESS,
                page,
                page,
            ],
            6,
        )
    }
}

/// What a piece of a frame is to the controller, which the data / command
/// line tells apart.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChunkKind {
    /// Commands: the line low.
    Command,
    /// Pixels: the line high.
    Data,
}

/// One piece of a frame: commands or pixels, never both.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Chunk<'a> {
    /// What the bytes are.
    pub kind: ChunkKind,
    /// The bytes, in the order they go out.
    pub bytes: &'a [u8],
}

/// Where a [`FrameStream`] is in its frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Next {
    /// Nothing to send.
    Idle,
    /// The addressing commands of this page.
    Commands(usize),
    /// The pixels of this page, from this column.
    Data(usize, usize),
}

/// A frame sent piece by piece: the bytes [`Ssd130x`] would send, page by
/// page, handed out in chunks of at most a given size, so that a sender can
/// fit each between other transfers on the bus.
///
/// Commands always come whole (at most six bytes); a contrast change waits
/// for the boundary between two pages.
#[derive(Debug, Clone)]
pub struct FrameStream {
    controller: Controller,
    frame: [u8; LEN],
    next: Next,
    contrast: Option<u8>,
    commands: [u8; MAX_PAGE_COMMANDS],
}

impl FrameStream {
    /// Nothing to send yet, for a panel with `controller`.
    #[must_use]
    pub const fn new(controller: Controller) -> Self {
        Self {
            controller,
            frame: [0; LEN],
            next: Next::Idle,
            contrast: None,
            commands: [0; MAX_PAGE_COMMANDS],
        }
    }

    /// Whether the frame and any contrast change have gone out.
    #[must_use]
    pub const fn is_idle(&self) -> bool {
        matches!(self.next, Next::Idle) && self.contrast.is_none()
    }

    /// Starts sending `frame` from its first page, dropping what is left of
    /// the previous one.
    pub const fn start(&mut self, frame: &[u8; LEN]) {
        self.frame = *frame;
        self.next = Next::Commands(0);
    }

    /// Sends a contrast change at the next boundary between two pages.
    pub const fn set_contrast(&mut self, contrast: u8) {
        self.contrast = Some(contrast);
    }

    /// The next piece, at most `max` pixel bytes (at least one), or `None`
    /// once everything went out.
    pub fn next_chunk(&mut self, max: usize) -> Option<Chunk<'_>> {
        if matches!(self.next, Next::Idle | Next::Commands(_))
            && let Some(contrast) = self.contrast.take()
        {
            self.commands[..2].copy_from_slice(&[command::SET_CONTRAST, contrast]);
            return Some(Chunk {
                kind: ChunkKind::Command,
                bytes: &self.commands[..2],
            });
        }
        match self.next {
            Next::Idle => None,
            Next::Commands(page) => {
                let (commands, len) = page_commands(self.controller, page);
                self.commands = commands;
                self.next = Next::Data(page, 0);
                Some(Chunk {
                    kind: ChunkKind::Command,
                    bytes: &self.commands[..len],
                })
            }
            Next::Data(page, column) => {
                let len = max.max(1).min(WIDTH - column);
                self.next = if column + len < WIDTH {
                    Next::Data(page, column + len)
                } else if page + 1 < PAGES {
                    Next::Commands(page + 1)
                } else {
                    Next::Idle
                };
                let start = page * WIDTH + column;
                Some(Chunk {
                    kind: ChunkKind::Data,
                    bytes: &self.frame[start..start + len],
                })
            }
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

    /// Gives the bus and the pins back, for a sender that takes the panel
    /// over once it is initialised ([`FrameStream`]).
    pub fn release(self) -> (SPI, CS, DC, RST) {
        (self.spi, self.chip_select, self.data_command, self.reset)
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
        // Memory mode 0x00 (horizontal) matches the reference O&C init sequence
        // even on SH1106; the SH1106 refresh path still re-sets page/column per
        // page, which is what actually lands the pixels correctly.
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
            command::DEACTIVATE_SCROLL,
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
        if self.controller.page_addressed() {
            self.flush_frame_paged()
        } else {
            self.flush_frame_horizontal()
        }
    }

    /// SSD1306 / SSD1309: set the full window once, then stream all 1024 bytes.
    fn flush_frame_horizontal(&mut self) -> Result<(), Ssd130xError> {
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

    /// SH1106: address each page, starting at the panel's column offset.
    ///
    /// Matches `SH1106_128x64_Driver::SendPage` in the reference firmware: three
    /// page-mode commands, then 128 data bytes, repeated for every page.
    fn flush_frame_paged(&mut self) -> Result<(), Ssd130xError> {
        (0..PAGES).try_for_each(|page| self.flush_page(page))
    }

    /// Sends one page (eight pixel rows) of the framebuffer: on its own, a
    /// transfer short enough to fit between two deadlines of a faster loop,
    /// and the unit the firmware's profile times (journal/D-023, D-029).
    /// Pages past the last are ignored.
    ///
    /// # Errors
    ///
    /// Returns an error if a control pin or the SPI bus fails.
    pub fn flush_page(&mut self, page: usize) -> Result<(), Ssd130xError> {
        if page >= PAGES {
            return Ok(());
        }
        let (commands, len) = page_commands(self.controller, page);
        self.commands(&commands[..len])?;
        self.data_command
            .set_high()
            .map_err(|_| Ssd130xError::Pin)?;
        // The page is copied out because the bus is borrowed mutably while
        // it is sent; 128 bytes cost less than moving the whole frame.
        let start = page * WIDTH;
        let mut bytes = [0; WIDTH];
        bytes.copy_from_slice(&self.frame.as_bytes()[start..start + WIDTH]);
        self.transfer(&bytes)
    }

    /// Sends a command sequence.
    fn commands(&mut self, bytes: &[u8]) -> Result<(), Ssd130xError> {
        self.data_command.set_low().map_err(|_| Ssd130xError::Pin)?;
        self.transfer(bytes)
    }

    /// Largest SPI write the i.MX RT LPSPI `SpiBus` path accepts in one frame.
    ///
    /// `imxrt-hal` builds one hardware transaction whose bit length is
    /// `8 * len`, and rejects anything above 4096 bits (512 bytes). The OLED
    /// framebuffer is 1024 bytes, so a full horizontal refresh must be split.
    /// Page-mode transfers are only 128 bytes and never hit the cap. Controllers
    /// that accept larger frames are unharmed: chunks are just consecutive
    /// writes under the same software chip-select window.
    const MAX_SPI_CHUNK: usize = 512;

    /// Asserts the chip select, writes `bytes` (chunked if needed), waits for
    /// the shift register to drain, and always releases the select again.
    fn transfer(&mut self, bytes: &[u8]) -> Result<(), Ssd130xError> {
        self.chip_select.set_low().map_err(|_| Ssd130xError::Pin)?;

        // `SpiBus::write` may return once the FIFO is filled; the CS pin must
        // stay asserted until `flush` confirms the last bit has left the bus.
        // Chunking keeps each frame inside the LPSPI 4096-bit limit.
        let written = (|| {
            for chunk in bytes.chunks(Self::MAX_SPI_CHUNK) {
                self.spi.write(chunk).map_err(|_| Ssd130xError::Bus)?;
                self.spi.flush().map_err(|_| Ssd130xError::Bus)?;
            }
            Ok(())
        })();

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

    fn set_contrast(&mut self, contrast: u8) {
        self.last_error = self.commands(&[command::SET_CONTRAST, contrast]).err();
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

    use oc_core::framebuffer::{LEN, PAGES, WIDTH};
    use oc_core::platform::Display;

    use super::{Controller, Ssd130x, Ssd130xError, command};

    /// An SPI bus that records every byte written.
    #[derive(Debug, Default)]
    struct RecordingBus {
        written: Vec<u8>,
        /// Number of successful `write` calls (used to check chunking).
        writes: u32,
        /// Number of `flush` calls.
        flushes: u32,
        /// When set, reject any single write larger than this many bytes —
        /// mirrors the i.MX RT LPSPI frame-size ceiling.
        max_write: Option<usize>,
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
            if self.max_write.is_some_and(|max| words.len() > max) {
                return Err(embedded_hal::spi::ErrorKind::Other);
            }
            self.writes += 1;
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
            self.flushes += 1;
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
    fn a_frame_page_by_page_sends_the_same_bytes_on_a_sh1106() {
        let mut whole = panel(Controller::Sh1106);
        let mut paged = panel(Controller::Sh1106);
        for panel in [&mut whole, &mut paged] {
            panel.frame_mut().as_mut_bytes()[300] = 0x5A;
        }
        whole.flush_frame().unwrap();
        for page in 0..PAGES {
            paged.flush_page(page).unwrap();
        }
        assert_eq!(whole.spi.written, paged.spi.written);
    }

    #[test]
    fn one_page_on_a_ssd1306_opens_a_one_page_window() {
        let mut panel = panel(Controller::Ssd1306);
        panel.frame_mut().as_mut_bytes()[3 * WIDTH] = 0x81;
        panel.flush_page(3).unwrap();
        assert_eq!(
            panel.spi.written[..6],
            [
                command::SET_COLUMN_ADDRESS,
                0,
                127,
                command::SET_PAGE_ADDRESS,
                3,
                3
            ]
        );
        assert_eq!(panel.spi.written.len(), 6 + WIDTH);
        assert_eq!(panel.spi.written[6], 0x81);
    }

    #[test]
    fn a_page_past_the_last_sends_nothing() {
        let mut panel = panel(Controller::Sh1106);
        panel.flush_page(PAGES).unwrap();
        assert_eq!(panel.spi.written, Vec::<u8>::new());
    }

    #[test]
    fn initialisation_ends_with_the_panel_switched_on() {
        let mut panel = panel(Controller::Sh1106);
        let mut delay = RecordingDelay::default();
        panel.init(&mut delay).unwrap();

        assert_eq!(panel.spi.written.first(), Some(&command::DISPLAY_OFF));
        assert_eq!(panel.spi.written.last(), Some(&command::DISPLAY_ON));
    }

    #[test]
    fn initialisation_pulses_the_reset_line() {
        let mut panel = panel(Controller::Sh1106);
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
    fn the_ssd_controllers_differ_only_where_expected() {
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
    fn sh1106_enables_the_charge_pump_like_the_reference_firmware() {
        let mut delay = RecordingDelay::default();
        let mut panel = panel(Controller::Sh1106);
        panel.init(&mut delay).unwrap();

        let position = panel
            .spi
            .written
            .iter()
            .position(|&byte| byte == command::SET_CHARGE_PUMP)
            .expect("the sequence must configure the charge pump");
        assert_eq!(panel.spi.written[position + 1], 0x14);
    }

    #[test]
    fn a_horizontal_refresh_sends_the_whole_framebuffer_once() {
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
    fn a_horizontal_refresh_addresses_the_full_screen_before_the_data() {
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
    fn sh1106_refresh_addresses_each_page_with_column_offset_two() {
        // Matches SH1106_data_start_seq in the reference firmware:
        //   0x10, 0x02, 0xB0|page  then 128 data bytes, per page.
        let mut panel = panel(Controller::Sh1106);
        for (i, byte) in panel.frame_mut().as_mut_bytes().iter_mut().enumerate() {
            *byte = u8::try_from(i & 0xFF).unwrap();
        }
        panel.present();

        assert_eq!(panel.last_error(), None);

        let mut cursor = 0;
        for page in 0..PAGES {
            let header = &panel.spi.written[cursor..cursor + 3];
            assert_eq!(
                header,
                [
                    command::SET_HIGH_COLUMN,
                    command::SET_LOW_COLUMN | 0x02,
                    command::SET_PAGE_START | u8::try_from(page).unwrap(),
                ]
                .as_slice(),
                "page {page} must be addressed at column offset 2"
            );
            cursor += 3;
            let data = &panel.spi.written[cursor..cursor + WIDTH];
            let start = page * WIDTH;
            let expected: Vec<u8> = (start..start + WIDTH)
                .map(|i| u8::try_from(i & 0xFF).unwrap())
                .collect();
            assert_eq!(data, expected.as_slice(), "page {page} payload");
            cursor += WIDTH;
        }
        assert_eq!(cursor, panel.spi.written.len(), "no trailing bus traffic");
    }

    #[test]
    fn sh1106_refresh_never_uses_ssd1306_window_commands() {
        let mut panel = panel(Controller::Sh1106);
        panel.present();

        assert!(
            !panel.spi.written.contains(&command::SET_COLUMN_ADDRESS),
            "SH1106 must not be programmed with SSD1306 column-range commands"
        );
        assert!(
            !panel.spi.written.contains(&command::SET_PAGE_ADDRESS),
            "SH1106 must not be programmed with SSD1306 page-range commands"
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
    fn sh1106_toggles_dc_once_per_page() {
        let mut panel = panel(Controller::Sh1106);
        panel.present();

        // Each page: DC low (3 address cmds) then DC high (128 data bytes).
        let expected: Vec<bool> = (0..PAGES).flat_map(|_| [false, true]).collect();
        assert_eq!(panel.data_command.levels, expected);
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
    fn a_full_framebuffer_is_chunked_under_the_lpspi_frame_limit() {
        // The real LPSPI rejects any single write larger than 512 bytes. A
        // horizontal refresh is 1024 bytes, so the driver must split it and
        // flush each chunk before releasing CS — otherwise init fails with
        // `Bus` on hardware even though the wiring is fine.
        let mut panel = panel(Controller::Ssd1306);
        panel.spi.max_write = Some(512);
        panel.frame_mut().fill();
        panel.present();

        assert_eq!(panel.last_error(), None);
        let frame_bytes = &panel.spi.written[panel.spi.written.len() - LEN..];
        assert_eq!(frame_bytes.len(), LEN);
        assert!(
            frame_bytes.iter().all(|&b| b == 0xFF),
            "the filled framebuffer must still reach the bus intact after chunking"
        );
        // Address header (1 write) + two 512-byte pixel chunks.
        assert!(
            panel.spi.writes >= 3,
            "a 1024-byte frame must be split under a 512-byte cap"
        );
        assert_eq!(
            panel.spi.flushes, panel.spi.writes,
            "each chunk must be flushed before CS may rise"
        );
        // Address commands and pixel data each get their own CS window; the
        // important property is that CS ends released after the chunked write.
        assert_eq!(
            panel.chip_select.levels.last(),
            Some(&true),
            "chip select must end released after the chunked framebuffer write"
        );
        assert!(
            panel
                .chip_select
                .levels
                .windows(2)
                .any(|w| w == [false, true]),
            "each transfer still brackets its bytes with a CS low→high pair"
        );
    }

    #[test]
    fn the_contrast_is_one_command_and_its_value() {
        let mut panel = panel(Controller::Sh1106);
        panel.spi.written.clear();
        panel.set_contrast(0x42);
        assert_eq!(panel.spi.written, [command::SET_CONTRAST, 0x42]);
        assert_eq!(
            panel.data_command.levels.last(),
            Some(&false),
            "sent as a command"
        );
    }

    #[test]
    fn controller_names_are_reported_for_the_boot_banner() {
        assert_eq!(Controller::Sh1106.name(), "SH1106");
        assert_eq!(Controller::Ssd1306.name(), "SSD1306");
        assert_eq!(Controller::Ssd1309.name(), "SSD1309");
        assert_eq!(panel(Controller::Sh1106).controller(), Controller::Sh1106);
    }

    /// Sends every chunk of `stream` at most `max` pixel bytes long, the
    /// commands and the pixels apart.
    fn drain(stream: &mut super::FrameStream, max: usize) -> Vec<(super::ChunkKind, Vec<u8>)> {
        let mut chunks = Vec::new();
        while let Some(chunk) = stream.next_chunk(max) {
            chunks.push((chunk.kind, chunk.bytes.to_vec()));
        }
        chunks
    }

    #[test]
    fn a_streamed_frame_is_the_bytes_of_its_pages() {
        use super::{ChunkKind, FrameStream};
        for controller in [Controller::Sh1106, Controller::Ssd1306] {
            let mut panel = panel(controller);
            for x in 0..128 {
                for y in 0..64 {
                    panel.frame_mut().set_pixel(x, y, (x * 7 + y * 3) % 5 == 0);
                }
            }
            let frame = *panel.frame_mut().as_bytes();
            for page in 0..PAGES {
                panel.flush_page(page).unwrap();
            }

            let mut stream = FrameStream::new(controller);
            assert!(stream.is_idle());
            stream.start(&frame);
            let chunks = drain(&mut stream, 24);
            assert!(stream.is_idle());
            let sent: Vec<u8> = chunks.iter().flat_map(|(_, bytes)| bytes.clone()).collect();
            assert_eq!(sent, panel.spi.written, "{controller:?}");
            for (kind, bytes) in &chunks {
                match kind {
                    ChunkKind::Command => assert!(bytes.len() <= 6),
                    ChunkKind::Data => assert!(bytes.len() <= 24),
                }
            }
            let commands = chunks
                .iter()
                .filter(|(kind, _)| *kind == ChunkKind::Command)
                .count();
            assert_eq!(commands, PAGES, "one command chunk per page");
        }
    }

    #[test]
    fn a_contrast_change_waits_for_a_page_boundary() {
        use super::{ChunkKind, FrameStream};
        let mut stream = FrameStream::new(Controller::Sh1106);
        stream.set_contrast(0x42);
        assert!(!stream.is_idle());
        assert_eq!(
            drain(&mut stream, 64),
            [(ChunkKind::Command, std::vec![command::SET_CONTRAST, 0x42])],
            "sent alone when no frame is going out"
        );

        stream.start(&[0; LEN]);
        let first = stream.next_chunk(64).unwrap().bytes.to_vec();
        let _ = stream.next_chunk(64);
        stream.set_contrast(7);
        let middle = stream.next_chunk(64).unwrap();
        assert_eq!(middle.kind, ChunkKind::Data, "not inside a page");
        let after = stream.next_chunk(64).unwrap();
        assert_eq!(after.bytes, [command::SET_CONTRAST, 7]);
        assert_eq!(stream.next_chunk(64).unwrap().bytes[..2], first[..2]);
    }

    #[test]
    fn a_released_panel_gives_its_parts_back() {
        let (spi, ..) = panel(Controller::Sh1106).release();
        assert_eq!(spi.written, [] as [u8; 0]);
    }
}
