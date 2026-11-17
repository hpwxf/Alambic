//! The write protocol of `HalfKay`, the Teensy 4.0's bootloader, as the
//! reports a loader sends (journal/D-031).
//!
//! `HalfKay` shows up as a HID device ([`VENDOR_ID`]:[`PRODUCT_ID`]) and is
//! programmed with output reports, one per 1 KiB block of flash: a 64-byte
//! header carrying the block's address, then the block. Writing the first
//! block erases the chip, which takes long; the device refuses reports while
//! it is busy, so a loader retries each one until its [`Report::timeout_ms`]
//! runs out. A last report with address [`BOOT_ADDRESS`] starts the new image.
//! This is what `teensy_loader_cli --mcu=TEENSY40` does
//! (`teensy_loader_cli.c`, PJRC), reproduced here without its transport so a
//! browser (`WebHID`) and the tests can share it.
//!
//! The crate only cuts reports; sending them, and waiting, is the
//! transport's business.

#![no_std]
#![forbid(unsafe_code)]
#![warn(missing_docs)]

#[cfg(test)]
extern crate std;

/// PJRC's USB vendor identifier.
pub const VENDOR_ID: u16 = 0x16C0;

/// `HalfKay`'s product identifier, the same on every Teensy.
pub const PRODUCT_ID: u16 = 0x0478;

/// Bytes of flash one report writes.
pub const BLOCK_LEN: usize = 1024;

/// Bytes before the block: the address, then zeros.
pub const HEADER_LEN: usize = 64;

/// Length of every report, the boot request included.
pub const REPORT_LEN: usize = HEADER_LEN + BLOCK_LEN;

/// Flash `HalfKay` accepts on a Teensy 4.0: everything below the reserved
/// top 64 KiB (`teensy_loader_cli`'s `code_size` for `TEENSY40`).
pub const CODE_SIZE: usize = 1984 * 1024;

/// The address that asks `HalfKay` to start the image instead of writing.
pub const BOOT_ADDRESS: u32 = 0x00FF_FFFF;

/// How long the first write may be refused while the chip erases, in
/// milliseconds (`teensy_loader_cli`'s figure).
pub const FIRST_BLOCK_TIMEOUT_MS: u32 = 5_000;

/// How long any later report may be refused, in milliseconds.
pub const BLOCK_TIMEOUT_MS: u32 = 500;

/// Why an image cannot be cut into reports.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Error {
    /// Nothing to write.
    Empty,
    /// The image is longer than [`CODE_SIZE`]; its length is given.
    TooLarge(usize),
}

/// What a report asks for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// Write one block; `first` erases the chip beforehand.
    Write {
        /// Whether this is the first block, the one that erases.
        first: bool,
    },
    /// Start the image just written.
    Boot,
}

/// One report, ready to send as HID output report 0 (`HalfKay` uses no
/// report identifiers).
#[derive(Clone, PartialEq, Eq)]
pub struct Report {
    kind: Kind,
    address: u32,
    bytes: [u8; REPORT_LEN],
}

impl core::fmt::Debug for Report {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        // The payload would bury the two fields that matter.
        f.debug_struct("Report")
            .field("kind", &self.kind)
            .field("address", &self.address)
            .finish_non_exhaustive()
    }
}

impl Report {
    fn new(kind: Kind, address: u32, block: &[u8]) -> Self {
        let mut bytes = [0; REPORT_LEN];
        bytes[..3].copy_from_slice(&address.to_le_bytes()[..3]);
        bytes[HEADER_LEN..HEADER_LEN + block.len()].copy_from_slice(block);
        Self {
            kind,
            address,
            bytes,
        }
    }

    /// What the report asks for.
    #[must_use]
    pub const fn kind(&self) -> Kind {
        self.kind
    }

    /// The flash offset written, or [`BOOT_ADDRESS`].
    #[must_use]
    pub const fn address(&self) -> u32 {
        self.address
    }

    /// The bytes to send.
    #[must_use]
    pub const fn bytes(&self) -> &[u8; REPORT_LEN] {
        &self.bytes
    }

    /// How long to keep retrying while the device refuses this report.
    #[must_use]
    pub const fn timeout_ms(&self) -> u32 {
        match self.kind {
            Kind::Write { first: true } => FIRST_BLOCK_TIMEOUT_MS,
            Kind::Write { first: false } | Kind::Boot => BLOCK_TIMEOUT_MS,
        }
    }
}

/// The reports that write `image` and start it, in order.
///
/// `image` is the flash contents from its first byte (`0x6000_0000` on the
/// bus): `HalfKay` takes addresses relative to the start of the flash. Every
/// block up to the image's end is written, gaps included: writing erased
/// bytes over erased flash changes nothing, and it spares the loader from
/// having to know which bytes the image really covers. The last block is
/// padded with erased bytes (`0xFF`).
///
/// # Errors
///
/// [`Error::Empty`] or [`Error::TooLarge`].
pub fn reports(image: &[u8]) -> Result<Reports<'_>, Error> {
    if image.is_empty() {
        return Err(Error::Empty);
    }
    if image.len() > CODE_SIZE {
        return Err(Error::TooLarge(image.len()));
    }
    Ok(Reports {
        image,
        next: 0,
        booted: false,
    })
}

/// The reports of one image, from [`reports`].
#[derive(Debug, Clone)]
pub struct Reports<'a> {
    image: &'a [u8],
    next: usize,
    booted: bool,
}

impl Reports<'_> {
    /// Reports in all, the boot request included: for a progress bar.
    #[must_use]
    pub const fn total(&self) -> usize {
        self.image.len().div_ceil(BLOCK_LEN) + 1
    }
}

impl Iterator for Reports<'_> {
    type Item = Report;

    fn next(&mut self) -> Option<Report> {
        if self.next < self.image.len() {
            let start = self.next;
            let end = (start + BLOCK_LEN).min(self.image.len());
            self.next = start + BLOCK_LEN;
            let mut block = [0xFF; BLOCK_LEN];
            block[..end - start].copy_from_slice(&self.image[start..end]);
            // `reports` bounded the image to `CODE_SIZE`, far below 2^24.
            let address = u32::try_from(start).unwrap_or(BOOT_ADDRESS);
            let kind = Kind::Write { first: start == 0 };
            return Some(Report::new(kind, address, &block));
        }
        if self.booted {
            return None;
        }
        self.booted = true;
        Some(Report::new(Kind::Boot, BOOT_ADDRESS, &[]))
    }
}

#[cfg(test)]
mod tests {
    use std::vec;

    use super::*;

    #[test]
    fn an_empty_or_oversize_image_is_refused() {
        assert_eq!(reports(&[]).err(), Some(Error::Empty));
        let big = vec![0; CODE_SIZE + 1];
        assert_eq!(reports(&big).err(), Some(Error::TooLarge(CODE_SIZE + 1)));
    }

    #[test]
    fn each_block_carries_its_address_then_its_bytes() {
        let mut image = [0; 2 * BLOCK_LEN + 10];
        for (index, byte) in image.iter_mut().enumerate() {
            *byte = u8::try_from(index % 251).unwrap();
        }
        let mut reports = reports(&image).unwrap();
        assert_eq!(reports.total(), 4);

        let first = reports.next().unwrap();
        assert_eq!(first.kind(), Kind::Write { first: true });
        assert_eq!(first.timeout_ms(), FIRST_BLOCK_TIMEOUT_MS);
        assert_eq!(&first.bytes()[..HEADER_LEN], &[0; HEADER_LEN]);
        assert_eq!(&first.bytes()[HEADER_LEN..], &image[..BLOCK_LEN]);

        let second = reports.next().unwrap();
        assert_eq!(second.kind(), Kind::Write { first: false });
        assert_eq!(second.timeout_ms(), BLOCK_TIMEOUT_MS);
        assert_eq!(second.address(), 0x400);
        assert_eq!(&second.bytes()[..4], &[0x00, 0x04, 0x00, 0x00]);
        assert!(second.bytes()[3..HEADER_LEN].iter().all(|&byte| byte == 0));
        assert_eq!(
            &second.bytes()[HEADER_LEN..],
            &image[BLOCK_LEN..2 * BLOCK_LEN]
        );
    }

    #[test]
    fn the_last_block_is_padded_with_erased_bytes() {
        let image = [0x12; BLOCK_LEN + 3];
        let last = reports(&image).unwrap().nth(1).unwrap();
        assert_eq!(last.address(), 0x400);
        assert_eq!(&last.bytes()[HEADER_LEN..HEADER_LEN + 3], &[0x12; 3]);
        assert!(
            last.bytes()[HEADER_LEN + 3..]
                .iter()
                .all(|&byte| byte == 0xFF)
        );
    }

    #[test]
    fn a_high_address_is_little_endian_on_three_bytes() {
        // A block near the top of the accepted flash.
        let image = vec![0; CODE_SIZE];
        let last = reports(&image)
            .unwrap()
            .nth(CODE_SIZE / BLOCK_LEN - 1)
            .unwrap();
        assert_eq!(last.address(), 0x001E_FC00);
        assert_eq!(&last.bytes()[..4], &[0x00, 0xFC, 0x1E, 0x00]);
    }

    #[test]
    fn the_reports_end_with_one_boot_request() {
        let image = [0; 10];
        let all: [Option<Report>; 3] = {
            let mut reports = reports(&image).unwrap();
            [reports.next(), reports.next(), reports.next()]
        };
        let boot = all[1].as_ref().unwrap();
        assert_eq!(boot.kind(), Kind::Boot);
        assert_eq!(boot.address(), BOOT_ADDRESS);
        assert_eq!(&boot.bytes()[..3], &[0xFF; 3]);
        assert!(boot.bytes()[3..].iter().all(|&byte| byte == 0));
        assert!(all[2].is_none());
    }
}
