//! Driver for the Texas Instruments DAC8565, the module's four CV outputs.
//!
//! The DAC8565 is a quad 16-bit converter driven over SPI with 24-bit words:
//! one command byte followed by the 16-bit code.
//!
//! The command byte is `0b0001_0000 | (channel << 1)` for a write that takes
//! effect immediately. That value is not a guess: it is exactly what the
//! reference Ornament & Crime firmware emits (`OC_DAC.h`,
//! `kChannelCommand = { 0b00010000, 0b00010010, 0b00010100, 0b00010110 }`), so
//! it is known to work on the real module and is the default here.
//!
//! [`UpdateMode::Simultaneous`] is offered as an opt-in: it buffers three
//! channels and updates all four on the last word so that the outputs step
//! together. It relies on the data sheet's load-command encoding rather than on
//! observed behaviour, so it must be verified on hardware before being trusted.
//!
//! The driver is generic over any `embedded-hal` 1.0 SPI bus and chip-select
//! pin, which keeps it independent of the pinout and testable on the host.

use embedded_hal::digital::OutputPin;
use embedded_hal::spi::SpiBus;

use oc_core::calibration::CvOutputCalibration;
use oc_core::platform::{AnalogOut, CV_CHANNELS, CvChannel, MilliVolts};

/// Command bits of the 24-bit word, from the DAC8565 data sheet (SLAS515).
///
/// A word is `[ 0 0 LD1 LD0 0 A1 A0 PD ][ D15..D8 ][ D7..D0 ]`: `A1 A0` select
/// the channel and `LD1 LD0` decide what happens to the DAC register.
mod command {
    /// Write the input register only; the output does not move.
    pub(super) const BUFFER: u8 = 0b0000_0000;
    /// Write the input register and update this channel immediately.
    pub(super) const WRITE_AND_UPDATE_ONE: u8 = 0b0001_0000;
    /// Write the input register and update every channel at once.
    pub(super) const WRITE_AND_UPDATE_ALL: u8 = 0b0010_0000;
    /// Bit position of the two channel-select bits.
    pub(super) const CHANNEL_SHIFT: u8 = 1;
}

/// When the DAC outputs actually move.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum UpdateMode {
    /// Each channel moves as its word arrives, spreading the four outputs over
    /// roughly three SPI words. This is what the reference firmware does and is
    /// therefore the trusted default.
    #[default]
    PerChannel,
    /// Three channels are buffered and all four move on the last word. Nicer,
    /// but derived from the data sheet rather than from proven firmware.
    Simultaneous,
}

/// Errors the driver can report.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dac8565Error {
    /// The SPI transfer failed.
    Bus,
    /// The chip-select pin could not be driven.
    ChipSelect,
}

/// The module's four CV outputs.
#[derive(Debug)]
pub struct Dac8565<SPI, CS> {
    spi: SPI,
    chip_select: CS,
    calibration: [CvOutputCalibration; CV_CHANNELS],
    update_mode: UpdateMode,
    staged: [MilliVolts; CV_CHANNELS],
    /// Set when a staged value has not yet been pushed to the hardware.
    dirty: bool,
    /// Last error seen, kept so that a failing bus does not silently do nothing.
    last_error: Option<Dac8565Error>,
}

impl<SPI, CS> Dac8565<SPI, CS>
where
    SPI: SpiBus<u8>,
    CS: OutputPin,
{
    /// Wraps an SPI bus and chip-select pin, using one calibration per channel.
    pub fn new(spi: SPI, chip_select: CS, calibration: [CvOutputCalibration; CV_CHANNELS]) -> Self {
        Self {
            spi,
            chip_select,
            calibration,
            update_mode: UpdateMode::PerChannel,
            staged: [0; CV_CHANNELS],
            dirty: true,
            last_error: None,
        }
    }

    /// Chooses when the outputs move; see [`UpdateMode`].
    pub const fn set_update_mode(&mut self, mode: UpdateMode) {
        self.update_mode = mode;
        self.dirty = true;
    }

    /// Wraps an SPI bus using the nominal calibration on every channel.
    pub fn with_nominal_calibration(spi: SPI, chip_select: CS) -> Self {
        Self::new(
            spi,
            chip_select,
            [CvOutputCalibration::NOMINAL; CV_CHANNELS],
        )
    }

    /// Replaces the calibration of one channel.
    pub fn set_calibration(&mut self, channel: CvChannel, calibration: CvOutputCalibration) {
        self.calibration[channel.index()] = calibration;
        self.dirty = true;
    }

    /// The most recent error, if the outputs are not being updated.
    pub const fn last_error(&self) -> Option<Dac8565Error> {
        self.last_error
    }

    /// Pushes the staged values to the converter.
    ///
    /// # Errors
    ///
    /// Returns an error if the chip select or the SPI transfer fails.
    pub fn commit(&mut self) -> Result<(), Dac8565Error> {
        for channel in CvChannel::ALL {
            let index = channel.index();
            let code = self.calibration[index].to_code(self.staged[index]);
            let last = index + 1 == CV_CHANNELS;
            let mode = match (self.update_mode, last) {
                (UpdateMode::PerChannel, _) => command::WRITE_AND_UPDATE_ONE,
                (UpdateMode::Simultaneous, false) => command::BUFFER,
                (UpdateMode::Simultaneous, true) => command::WRITE_AND_UPDATE_ALL,
            };
            self.write_word(mode, channel, code)?;
        }
        self.dirty = false;
        Ok(())
    }

    /// Sends one 24-bit command word.
    fn write_word(&mut self, mode: u8, channel: CvChannel, code: u16) -> Result<(), Dac8565Error> {
        let selector = u8::try_from(channel.index()).unwrap_or(0) << command::CHANNEL_SHIFT;
        let word = [
            mode | selector,
            u8::try_from(code >> 8).unwrap_or(0),
            u8::try_from(code & 0xFF).unwrap_or(0),
        ];

        self.chip_select
            .set_low()
            .map_err(|_| Dac8565Error::ChipSelect)?;
        // `SpiBus::write` may return before the shift register is empty; hold
        // CS until `flush` confirms the 24-bit word has fully left the bus.
        let transfer = self
            .spi
            .write(&word)
            .and_then(|()| self.spi.flush())
            .map_err(|_| Dac8565Error::Bus);
        let release = self
            .chip_select
            .set_high()
            .map_err(|_| Dac8565Error::ChipSelect);

        // The chip select is always released, even when the transfer failed:
        // leaving it asserted would corrupt the next word on the shared bus.
        transfer.and(release)
    }
}

impl<SPI, CS> AnalogOut for Dac8565<SPI, CS>
where
    SPI: SpiBus<u8>,
    CS: OutputPin,
{
    fn write_cv(&mut self, channel: CvChannel, value: MilliVolts) {
        let index = channel.index();
        if self.staged[index] != value {
            self.staged[index] = value;
            self.dirty = true;
        }
    }

    fn flush(&mut self) {
        if !self.dirty {
            return;
        }
        // The trait cannot report failures: record them instead, so the
        // firmware can surface a dead bus rather than silently doing nothing.
        self.last_error = self.commit().err();
    }
}

#[cfg(test)]
mod tests {
    use std::vec::Vec;

    use embedded_hal::digital::{ErrorType as PinErrorType, OutputPin};
    use embedded_hal::spi::{ErrorType as SpiErrorType, SpiBus};

    use oc_core::calibration::{CV_OUT_MAX_MV, CV_OUT_MIN_MV, CvOutputCalibration};
    use oc_core::platform::{AnalogOut, CV_CHANNELS, CvChannel};

    use super::{Dac8565, Dac8565Error, UpdateMode};

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

    /// A chip select that counts its transitions.
    #[derive(Debug, Default)]
    struct CountingPin {
        lows: u32,
        highs: u32,
    }

    impl PinErrorType for CountingPin {
        type Error = core::convert::Infallible;
    }

    impl OutputPin for CountingPin {
        fn set_low(&mut self) -> Result<(), Self::Error> {
            self.lows += 1;
            Ok(())
        }

        fn set_high(&mut self) -> Result<(), Self::Error> {
            self.highs += 1;
            Ok(())
        }
    }

    fn dac() -> Dac8565<RecordingBus, CountingPin> {
        Dac8565::with_nominal_calibration(RecordingBus::default(), CountingPin::default())
    }

    #[test]
    fn a_flush_writes_one_three_byte_word_per_channel() {
        let mut dac = dac();
        dac.flush();
        dac.commit().unwrap();

        let words = u32::try_from(CV_CHANNELS * 2).unwrap();
        assert_eq!(dac.spi.written.len(), 3 * CV_CHANNELS * 2);
        assert_eq!(dac.chip_select.lows, words);
        assert_eq!(
            dac.chip_select.highs, words,
            "the chip select must always be released"
        );
    }

    #[test]
    fn the_default_command_bytes_match_the_reference_firmware() {
        let mut dac = dac();
        dac.commit().unwrap();

        let commands: Vec<u8> = dac.spi.written.chunks(3).map(|word| word[0]).collect();
        assert_eq!(
            commands,
            [0b0001_0000, 0b0001_0010, 0b0001_0100, 0b0001_0110],
            "these are the exact bytes the reference O&C firmware emits"
        );
    }

    #[test]
    fn simultaneous_mode_buffers_all_but_the_last_channel() {
        let mut dac = dac();
        dac.set_update_mode(UpdateMode::Simultaneous);
        dac.commit().unwrap();

        let words: Vec<&[u8]> = dac.spi.written.chunks(3).collect();
        assert_eq!(words.len(), CV_CHANNELS);
        for word in &words[..CV_CHANNELS - 1] {
            assert_eq!(word[0] & 0b0011_0000, 0, "channels A to C must be buffered");
        }
        assert_eq!(
            words[CV_CHANNELS - 1][0] & 0b0011_0000,
            0b0010_0000,
            "the last word must update every channel at once"
        );
    }

    #[test]
    fn the_channel_selector_lands_in_the_right_bits() {
        let mut dac = dac();
        dac.commit().unwrap();

        for (index, word) in dac.spi.written.chunks(3).enumerate() {
            let selector = (word[0] >> 1) & 0b11;
            assert_eq!(usize::from(selector), index, "channel {index} selector");
        }
    }

    #[test]
    fn levels_are_converted_through_the_calibration() {
        let mut dac = dac();
        dac.write_cv(CvChannel::One, CV_OUT_MAX_MV);
        dac.write_cv(CvChannel::Two, CV_OUT_MIN_MV);
        dac.flush();

        let words: Vec<&[u8]> = dac.spi.written.chunks(3).collect();
        assert_eq!(
            [words[0][1], words[0][2]],
            [0xFF, 0xFF],
            "+6 V is full scale"
        );
        assert_eq!(
            [words[1][1], words[1][2]],
            [0x00, 0x00],
            "-3 V is zero scale"
        );
    }

    #[test]
    fn out_of_range_levels_saturate_rather_than_wrap() {
        let mut dac = dac();
        dac.write_cv(CvChannel::One, i32::MAX);
        dac.write_cv(CvChannel::Two, i32::MIN);
        dac.flush();

        let words: Vec<&[u8]> = dac.spi.written.chunks(3).collect();
        assert_eq!([words[0][1], words[0][2]], [0xFF, 0xFF]);
        assert_eq!([words[1][1], words[1][2]], [0x00, 0x00]);
    }

    #[test]
    fn an_unchanged_value_does_not_retrigger_a_transfer() {
        let mut dac = dac();
        dac.write_cv(CvChannel::One, 1_000);
        dac.flush();
        let after_first = dac.spi.written.len();

        dac.write_cv(CvChannel::One, 1_000);
        dac.flush();
        assert_eq!(
            dac.spi.written.len(),
            after_first,
            "writing the same level again must not cost an SPI transaction"
        );
    }

    #[test]
    fn a_failing_bus_is_recorded_rather_than_ignored() {
        let mut dac = dac();
        dac.spi.fail = true;
        dac.write_cv(CvChannel::One, 500);
        dac.flush();

        assert_eq!(dac.last_error(), Some(Dac8565Error::Bus));
        assert_eq!(
            dac.chip_select.highs, 1,
            "the chip select is released even when the transfer fails"
        );
    }

    #[test]
    fn a_custom_calibration_changes_the_emitted_code() {
        let mut dac = dac();
        let nominal = CvOutputCalibration::NOMINAL;
        dac.set_calibration(
            CvChannel::One,
            CvOutputCalibration {
                zero_code: 65_535 - nominal.zero_code,
                millicodes_per_volt: -nominal.millicodes_per_volt,
            },
        );
        dac.write_cv(CvChannel::One, CV_OUT_MAX_MV);
        dac.flush();

        let word = &dac.spi.written[..3];
        assert_eq!(
            [word[1], word[2]],
            [0x00, 0x00],
            "an inverting output stage must send the opposite code"
        );
    }
}
