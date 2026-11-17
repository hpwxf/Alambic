//! The flash contents a validated image describes.
//!
//! `cargo xtask flash` hands `teensy_loader_cli` a HEX file that `llvm-objcopy`
//! derives from the ELF's loadable segments; the browser flasher has neither
//! tool, so it takes the same segments from the ELF itself. Keeping the ELF as
//! the input means the whole gate of [`validate_image`] still runs before a
//! single byte is written (journal/D-031).

use crate::validate::{
    ELF32_PHDR_LEN, FLEXSPI_BASE, ImageFacts, PT_LOAD, RESERVED_FLASH_START, Rejection,
    read_u16_le, read_u32_le, validate_image,
};

/// What erased flash reads as. Gaps between segments keep this value, as
/// they do when a HEX file leaves them out.
pub const ERASED: u8 = 0xFF;

/// The flash a loader writes, from [`FLEXSPI_BASE`] up to the end of the
/// last loadable byte, with the facts of the image it came from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FlashImage {
    /// What validating the ELF found, its digest included.
    pub facts: ImageFacts,
    bytes: Vec<u8>,
}

impl FlashImage {
    /// The flash contents; byte `n` goes at `FLEXSPI_BASE + n`.
    #[must_use]
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }
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
    Ok(FlashImage { facts, bytes })
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
