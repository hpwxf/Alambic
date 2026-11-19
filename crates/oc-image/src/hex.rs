//! Intel HEX files, read into the flash contents they describe.
//!
//! The HEX is what `cargo xtask hex` and CI publish and what every Teensy
//! tool reads, but it only carries addresses and bytes: the reader checks
//! the file itself (syntax, checksums, an end record, every byte inside the
//! application flash, none written twice), and [`crate::boot`] then checks
//! the boot header the bytes hold (journal/D-031).

use crate::flash::ERASED;
use crate::validate::{FLEXSPI_BASE, RESERVED_FLASH_START, Rejection};

/// Rejections listed before the rest are only counted: a file of the wrong
/// kind would otherwise give one per line.
const LISTED_REJECTIONS: usize = 10;

/// Whether `file` looks like an Intel HEX file: its first character, past
/// any blank, is the record mark.
#[must_use]
pub fn is_hex(file: &[u8]) -> bool {
    file.iter().find(|byte| !byte.is_ascii_whitespace()) == Some(&b':')
}

/// Reads `file` into flash contents, byte `n` going at `FLEXSPI_BASE + n`,
/// gaps left erased.
///
/// # Errors
///
/// A line that is not a valid record (syntax, length, checksum, unknown
/// type), data outside the application flash or written twice, data after
/// the end record, a missing end record, or no data at all; at most
/// `LISTED_REJECTIONS` are listed, the rest counted.
pub fn read_hex(file: &[u8]) -> Result<Vec<u8>, Vec<Rejection>> {
    let mut reader = Reader::default();
    let text = String::from_utf8_lossy(file);
    for (index, line) in text.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        reader.record(index + 1, line);
    }
    reader.finish()
}

/// What has been read so far.
#[derive(Default)]
struct Reader {
    /// Added to each data record's 16-bit address (records 02 and 04).
    base: u32,
    /// The flash contents, as long as the highest byte written.
    bytes: Vec<u8>,
    /// Which bytes of `bytes` a record wrote.
    written: Vec<bool>,
    /// The line of the end record, once seen.
    ended: Option<usize>,
    rejections: Vec<Rejection>,
    /// Rejections past [`LISTED_REJECTIONS`].
    unlisted: usize,
}

impl Reader {
    fn reject(&mut self, line: usize, message: impl std::fmt::Display) {
        if self.rejections.len() < LISTED_REJECTIONS {
            self.rejections
                .push(Rejection::new(format!("line {line}: {message}")));
        } else {
            self.unlisted += 1;
        }
    }

    fn record(&mut self, line: usize, text: &str) {
        if let Some(end) = self.ended {
            self.reject(
                line,
                format_args!("a record after the end record of line {end}"),
            );
            return;
        }
        let record = match decode(text) {
            Ok(record) => record,
            Err(message) => {
                self.reject(line, message);
                return;
            }
        };
        let address = u16::from_be_bytes([record[1], record[2]]);
        let data = &record[4..record.len() - 1];
        match (record[3], data.len()) {
            (0x00, _) => self.data(line, self.base + u32::from(address), data),
            (0x01, 0) => self.ended = Some(line),
            (0x02, 2) => self.base = u32::from(u16::from_be_bytes([data[0], data[1]])) << 4,
            (0x04, 2) => self.base = u32::from(u16::from_be_bytes([data[0], data[1]])) << 16,
            // Start addresses: the boot ROM takes the entry from the boot
            // header instead.
            (0x03 | 0x05, 4) => {}
            (kind @ 0x01..=0x05, len) => {
                self.reject(
                    line,
                    format_args!("record type {kind:02x} with {len} data bytes"),
                );
            }
            (kind, _) => self.reject(line, format_args!("unknown record type {kind:02x}")),
        }
    }

    fn data(&mut self, line: usize, start: u32, data: &[u8]) {
        let end = u64::from(start) + data.len() as u64;
        if start < FLEXSPI_BASE || end > u64::from(RESERVED_FLASH_START) {
            self.reject(
                line,
                format_args!(
                    "{} bytes at {start:#010x}, outside the application flash \
                     {FLEXSPI_BASE:#010x}..{RESERVED_FLASH_START:#010x}",
                    data.len()
                ),
            );
            return;
        }
        let offset = (start - FLEXSPI_BASE) as usize;
        let end = offset + data.len();
        if self.bytes.len() < end {
            self.bytes.resize(end, ERASED);
            self.written.resize(end, false);
        }
        if self.written[offset..end].iter().any(|&seen| seen) {
            self.reject(line, format_args!("{start:#010x} is written twice"));
            return;
        }
        self.bytes[offset..end].copy_from_slice(data);
        self.written[offset..end].fill(true);
    }

    fn finish(mut self) -> Result<Vec<u8>, Vec<Rejection>> {
        if self.ended.is_none() {
            self.rejections.push(Rejection::new(
                "no end record: the file is incomplete (an interrupted download?)",
            ));
        }
        if self.bytes.is_empty() && self.rejections.is_empty() {
            self.rejections
                .push(Rejection::new("the file holds no data"));
        }
        if self.unlisted > 0 {
            self.rejections.push(Rejection::new(format!(
                "and {} more problems",
                self.unlisted
            )));
        }
        if self.rejections.is_empty() {
            Ok(self.bytes)
        } else {
            Err(self.rejections)
        }
    }
}

/// The bytes of one record (count, address, type, data, checksum), checked.
fn decode(text: &str) -> Result<Vec<u8>, String> {
    let Some(digits) = text.strip_prefix(':') else {
        return Err("not an Intel HEX record (no leading `:`)".to_owned());
    };
    if !digits.is_ascii() {
        return Err("not hex digits".to_owned());
    }
    if digits.len() % 2 != 0 {
        return Err("an odd number of hex digits".to_owned());
    }
    let bytes = (0..digits.len())
        .step_by(2)
        .map(|at| u8::from_str_radix(&digits[at..at + 2], 16))
        .collect::<Result<Vec<u8>, _>>()
        .map_err(|_| "not hex digits".to_owned())?;
    if bytes.len() < 5 || bytes.len() != usize::from(bytes[0]) + 5 {
        return Err("the length does not match the byte count".to_owned());
    }
    if bytes.iter().fold(0_u8, |sum, &byte| sum.wrapping_add(byte)) != 0 {
        return Err("bad checksum".to_owned());
    }
    Ok(bytes)
}
