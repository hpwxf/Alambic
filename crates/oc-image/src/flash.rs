//! The flash contents a validated image describes.
//!
//! `cargo xtask flash` hands `teensy_loader_cli` a HEX file that `llvm-objcopy`
//! derives from the ELF's loadable segments; the browser flasher has neither
//! tool, so it takes the same segments from the ELF itself. Keeping the ELF as
//! the input means the whole gate of [`validate_image`] still runs before a
//! single byte is written (journal/D-031). A HEX file is accepted too, with
//! the reduced checks [`crate::hex`] and [`crate::boot`] can make.

use crate::boot::check_boot_header;
use crate::hex::{is_hex, read_hex};
use crate::validate::{
    ELF32_PHDR_LEN, FLEXSPI_BASE, PT_LOAD, RESERVED_FLASH_START, Rejection, hex_encode,
    read_u16_le, read_u32_le, sha256_digest, validate_image,
};

/// What erased flash reads as. Gaps between segments keep this value, as
/// they do when a HEX file leaves them out.
pub const ERASED: u8 = 0xFF;

/// The kind of file an image came from, which says how far it was checked.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    /// An ELF, through the whole gate of [`validate_image`].
    Elf,
    /// An Intel HEX file: its records and the boot header only, since it
    /// does not say what it was built for.
    Hex,
}

/// The flash a loader writes, from [`FLEXSPI_BASE`] up to the end of the
/// last loadable byte, with what checking the file found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FlashImage {
    /// The kind of file it came from.
    pub format: Format,
    /// SHA-256 of the file, as `cargo xtask flash` prints it for that kind.
    pub sha256: [u8; 32],
    /// Where the image starts: the ELF's entry point, or the boot header's.
    pub entry: u32,
    /// Soft findings that did not stop the image.
    pub warnings: Vec<String>,
    bytes: Vec<u8>,
}

impl FlashImage {
    /// The flash contents; byte `n` goes at `FLEXSPI_BASE + n`.
    #[must_use]
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    /// Lower-case hex rendering of [`Self::sha256`].
    #[must_use]
    pub fn sha256_hex(&self) -> String {
        hex_encode(&self.sha256)
    }
}

/// Recognises `file` as an ELF or an Intel HEX file and lays it out, after
/// the checks that kind allows.
///
/// # Errors
///
/// Every rejection those checks find, or a single one for a file of
/// neither kind.
pub fn firmware_image(file: &[u8]) -> Result<FlashImage, Vec<Rejection>> {
    if file.starts_with(b"\x7fELF") {
        flash_image(file)
    } else if is_hex(file) {
        hex_image(file)
    } else {
        Err(vec![Rejection::new(
            "neither a firmware ELF nor an Intel HEX file",
        )])
    }
}

/// Reads an Intel HEX file and checks the boot header it holds.
///
/// # Errors
///
/// What [`read_hex`] or [`check_boot_header`] reject.
pub fn hex_image(file: &[u8]) -> Result<FlashImage, Vec<Rejection>> {
    let bytes = read_hex(file)?;
    let header = check_boot_header(&bytes)?;
    Ok(FlashImage {
        format: Format::Hex,
        sha256: sha256_digest(file),
        entry: header.entry,
        warnings: vec![
            "reduced checks: a HEX file does not say what it was built for; \
             only its records and its boot header were checked"
                .to_owned(),
        ],
        bytes,
    })
}

/// Validates `elf` and lays its loadable segments out as flash contents.
///
/// Segments are placed at their physical (load) address, which is where the
/// boot ROM and the start-up code read them from: code and initialised data
/// that run from ITCM or DTCM are copied there from flash at reset.
///
/// # Errors
///
/// Every rejection [`validate_image`] finds; then, for an image that passes
/// it, a segment whose bytes lie past the end of the file, one that loads
/// outside the application flash (nothing would write it), or two that
/// overlap.
pub fn flash_image(elf: &[u8]) -> Result<FlashImage, Vec<Rejection>> {
    let facts = validate_image(elf)?;
    let segments = loadable_segments(elf)?;

    let len = segments
        .iter()
        .map(|segment| segment.start + segment.data.len())
        .max()
        .unwrap_or(0);
    let mut bytes = vec![ERASED; len];
    for segment in &segments {
        bytes[segment.start..segment.start + segment.data.len()].copy_from_slice(segment.data);
    }
    Ok(FlashImage {
        format: Format::Elf,
        sha256: facts.sha256,
        entry: facts.entry,
        warnings: facts.warnings,
        bytes,
    })
}

/// One segment's bytes and where they go, as an offset into the flash.
struct Segment<'a> {
    index: u16,
    start: usize,
    data: &'a [u8],
}

/// The non-empty `PT_LOAD` segments of an image [`validate_image`] accepted,
/// sorted by address and checked for overlaps.
fn loadable_segments(elf: &[u8]) -> Result<Vec<Segment<'_>>, Vec<Rejection>> {
    // The gate checked the header framing and that every program header lies
    // within the file, so these reads cannot go out of bounds.
    let phoff = read_u32_le(elf, 28) as usize;
    let phnum = read_u16_le(elf, 44);
    let mut segments = Vec::new();
    let mut rejections = Vec::new();

    for index in 0..phnum {
        let header = phoff + usize::from(index) * ELF32_PHDR_LEN;
        if read_u32_le(elf, header) != PT_LOAD {
            continue;
        }
        let offset = read_u32_le(elf, header + 4) as usize;
        let paddr = read_u32_le(elf, header + 12);
        let filesz = read_u32_le(elf, header + 16) as usize;
        if filesz == 0 {
            // `.bss`, `.stack` and the like: nothing to write.
            continue;
        }
        let Some(data) = offset
            .checked_add(filesz)
            .and_then(|end| elf.get(offset..end))
        else {
            rejections.push(Rejection::new(format!(
                "PT_LOAD segment {index} ({filesz} bytes at file offset {offset}) extends past the end of the file ({} bytes)",
                elf.len()
            )));
            continue;
        };
        if !(FLEXSPI_BASE..RESERVED_FLASH_START).contains(&paddr) {
            rejections.push(Rejection::new(format!(
                "PT_LOAD segment {index} loads at {paddr:#010x}, outside the application flash \
                 {FLEXSPI_BASE:#010x}..{RESERVED_FLASH_START:#010x}: no loader would write it"
            )));
            continue;
        }
        segments.push(Segment {
            index,
            start: (paddr - FLEXSPI_BASE) as usize,
            data,
        });
    }

    segments.sort_by_key(|segment| segment.start);
    for pair in segments.windows(2) {
        let (first, second) = (&pair[0], &pair[1]);
        if second.start < first.start + first.data.len() {
            rejections.push(Rejection::new(format!(
                "PT_LOAD segments {} and {} overlap in flash at {:#010x}",
                first.index,
                second.index,
                FLEXSPI_BASE as usize + second.start
            )));
        }
    }

    if rejections.is_empty() {
        Ok(segments)
    } else {
        Err(rejections)
    }
}
