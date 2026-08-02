//! Pre-flash validation of Teensy 4.0 firmware images.
//!
//! A corrupt or wrong-architecture image will not permanently brick a
//! Teensy 4.0 — `HalfKay` lives in ROM on a separate chip — but it wastes time
//! and a wrong image can still be electrically unsafe on the attached modular
//! hardware. These checks are the guard rail in front of `teensy_loader_cli`.
//!
//! Official board figures (flash size, CPU, etc.) come from the PJRC Teensy
//! technical specifications:
//! <https://www.pjrc.com/teensy/techspecs.html>
//!
//! The core entry point is deliberately a pure function over bytes so that
//! integration tests can build synthetic ELFs in memory and assert on every
//! rejection without touching the filesystem or a device.

// The ELF header field names (`e_type`, `e_version`, `e_shoff`, `e_shentsize`, ...)
// are the canonical ones from the ELF specification. Renaming them to satisfy the
// similarity heuristic would make this module harder to check against the spec,
// which is the only thing that makes it reviewable.
#![allow(clippy::similar_names)]

use std::fmt::{Display, Formatter, Write as _};
use std::ops::Range;

use sha2::{Digest, Sha256};

/// Application flash partition size on the Teensy 4.0, in bytes.
///
/// Hard upper bound for loadable `PT_LOAD` size: **1984 KiB** (`1984 * 1024`).
/// The Teensy 4.0 exposes 2048 KiB of QSPI; 64 KiB is reserved, leaving this
/// application partition for firmware images.
///
/// Source: [PJRC Teensy technical specifications](https://www.pjrc.com/teensy/techspecs.html)
/// (2 MB flash on Teensy 4.0; application partition is 1984 KiB after the
/// reserved bootloader region).
pub const TEENSY40_FLASH_CAPACITY: u64 = 1984 * 1024;

/// `FlexSPI` base address where the i.MX RT1062 boot ROM expects the image.
pub const FLEXSPI_BASE: u32 = 0x6000_0000;

/// Exclusive end of the `FlexSPI` / external flash window we accept.
///
/// Upper address bound for memory-mapped flash checks: **`0x6800_0000`**
/// (128 MiB aperture from [`FLEXSPI_BASE`]). The usable image size is far
/// smaller — see [`TEENSY40_FLASH_CAPACITY`] (1984 KiB on Teensy 4.0).
pub const FLEXSPI_END: u32 = 0x6800_0000;

/// Little-endian tag marking a `FlexSPI` Configuration Block (`FCFB` as ASCII).
pub const FCB_TAG: [u8; 4] = *b"FCFB";

/// Instruction Tightly Coupled Memory on the i.MX RT1062.
///
/// Address window: `0x0000_0000`..`0x0008_0000` — exclusive end **`0x0008_0000`**,
/// i.e. a **512 KiB** maximum aperture. On the Teensy 4.0 BSP `FlexRAM` split
/// only [`ITCM_USABLE`] is actually backed for `.text`; anything past the
/// configured banks faults even if it still falls inside this window.
pub const ITCM: Range<u32> = 0x0000_0000..0x0008_0000;

/// Usable ITCM on the Teensy 4.0 BSP `FlexRAM` split (6 × 32 KiB banks).
///
/// Hard size budget for `.text`: **192 KiB**. The address aperture
/// ([`ITCM`]) is larger (512 KiB) but unbacked banks fault on access.
pub const ITCM_USABLE: u64 = 192 * 1024;

/// Data Tightly Coupled Memory on the i.MX RT1062.
///
/// Address window: `0x2000_0000`..`0x2008_0000` — exclusive end **`0x2008_0000`**,
/// i.e. a **512 KiB** maximum aperture. On the Teensy 4.0 BSP `FlexRAM` split
/// only [`DTCM_USABLE`] is backed for `.stack`, `.vector_table`, `.data` and
/// `.bss` combined.
pub const DTCM: Range<u32> = 0x2000_0000..0x2008_0000;

/// Usable DTCM on the Teensy 4.0 BSP `FlexRAM` split (10 × 32 KiB banks).
///
/// Hard combined size budget for `.stack` + `.vector_table` + `.data` +
/// `.bss`: **320 KiB**. The address aperture ([`DTCM`]) is larger (512 KiB).
pub const DTCM_USABLE: u64 = 320 * 1024;

/// On-chip RAM window used for rodata / heap on this firmware.
///
/// Address window: `0x2020_0000`..`0x2028_0000` — exclusive end **`0x2028_0000`**.
/// Size equals [`OCRAM_CAPACITY`] (**512 KiB** dedicated, not taken from
/// `FlexRAM`). Upper bound for `.rodata`, `.uninit` and `.heap`.
pub const OCRAM: Range<u32> = 0x2020_0000..0x2028_0000;

/// Dedicated OCRAM capacity on the Teensy 4.0 / RT1062.
///
/// Hard size budget for OCRAM residents: **512 KiB** (matches the
/// [`OCRAM`] address window end-start).
pub const OCRAM_CAPACITY: u64 = 512 * 1024;

/// `FlexSPI` memory-mapped flash window (alias of [`FLEXSPI_BASE`]..[`FLEXSPI_END`]).
///
/// Exclusive end **`0x6800_0000`**; image size must still stay within
/// [`TEENSY40_FLASH_CAPACITY`] (1984 KiB on Teensy 4.0).
pub const FLEXSPI: Range<u32> = FLEXSPI_BASE..FLEXSPI_END;

const ELF_MAGIC: [u8; 4] = [0x7F, b'E', b'L', b'F'];
const ELFCLASS32: u8 = 1;
const ELFDATA2LSB: u8 = 1;
const EV_CURRENT: u8 = 1;
const ET_EXEC: u16 = 2;
const EM_ARM: u16 = 0x28;
const PT_LOAD: u32 = 1;

/// Minimum bytes needed to read an ELF32 header.
const ELF32_HEADER_LEN: usize = 52;
const ELF32_PHDR_LEN: usize = 32;
const ELF32_SHDR_LEN: usize = 40;

/// ARM EABI version 5 encoded in `e_flags` (bits 24–31).
const EF_ARM_EABI_VER5: u32 = 0x0500_0000;
/// Hard-float ABI bit in ARM `e_flags`.
const EF_ARM_ABI_FLOAT_HARD: u32 = 0x0000_0400;

/// A single reason an image must not be flashed.
///
/// Rejections are collected rather than short-circuited so the operator sees
/// every problem in one pass instead of playing whack-a-mole.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rejection {
    /// Human-readable, actionable explanation of the failure.
    pub message: String,
}

impl Rejection {
    /// Builds a rejection from a formatted message.
    #[must_use]
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl Display for Rejection {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

/// Facts extracted from an ELF that passed every hard check.
///
/// Warnings are soft findings that should be shown to the operator but do not
/// block flashing — for example when the vector table cannot be located
/// reliably and the reset-vector check had to be skipped.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImageFacts {
    /// Sum of `p_filesz` across all `PT_LOAD` segments.
    pub loadable_size: u64,
    /// ELF entry point (`e_entry`).
    pub entry: u32,
    /// Initial stack pointer from the vector table, when located.
    pub initial_sp: Option<u32>,
    /// Reset handler address from the vector table, when located.
    pub reset_handler: Option<u32>,
    /// SHA-256 digest of the raw input bytes.
    pub sha256: [u8; 32],
    /// ARM `e_flags` value, retained for diagnostics.
    pub e_flags: u32,
    /// Soft findings that did not fail the image.
    pub warnings: Vec<String>,
}

impl ImageFacts {
    /// Lower-case hex rendering of the SHA-256 digest.
    #[must_use]
    pub fn sha256_hex(&self) -> String {
        hex_encode(&self.sha256)
    }
}

/// Validates a firmware image and returns either extracted facts or every
/// rejection found.
///
/// All hard checks run to completion so multiple independent problems are
/// reported together. Soft findings are returned as [`ImageFacts::warnings`]
/// on the success path.
///
/// # Errors
///
/// Returns `Err` with one rejection per failed hard check. The list is never
/// empty when `Err` is returned.
pub fn validate_image(bytes: &[u8]) -> Result<ImageFacts, Vec<Rejection>> {
    let sha256 = Sha256::digest(bytes);
    let sha256: [u8; 32] = sha256.into();
    let mut rejections = Vec::new();
    let mut warnings = Vec::new();

    if matches!(check_framing(bytes, &mut rejections), Framing::Unusable) {
        return Err(rejections);
    }

    let ident = Ident::read(bytes);
    check_ident(ident, &mut rejections);

    let header = Header::read(bytes);
    check_header(ident, &header, &mut rejections);
    check_arm_abi(ident, &header, &mut warnings);

    let load = walk_program_headers(bytes, ident, &header, &mut rejections);
    check_loadable_coverage(ident, &header, &load, &mut rejections);

    let vectors = check_vector_table(bytes, ident, &header, &mut rejections, &mut warnings);

    if rejections.is_empty() {
        Ok(ImageFacts {
            loadable_size: load.loadable_size,
            entry: header.entry,
            initial_sp: vectors.initial_sp,
            reset_handler: vectors.reset_handler,
            sha256,
            e_flags: header.flags,
            warnings,
        })
    } else {
        Err(rejections)
    }
}

/// Whether enough of the file survived the framing checks to keep parsing.
enum Framing {
    /// A complete ELF32 header with correct magic is present.
    Parsable,
    /// The input cannot carry an ELF header; parsing further would be noise.
    Unusable,
}

/// Rejects inputs that cannot possibly be an ELF32 image before any parsing.
///
/// Every later helper indexes into the header at fixed offsets, so this is the
/// gate that makes those reads infallible: nothing downstream has to repeat a
/// bounds check. It also gives the operator a precise diagnosis (empty file,
/// truncated file, wrong file type) instead of a generic parse error.
fn check_framing(bytes: &[u8], rejections: &mut Vec<Rejection>) -> Framing {
    if bytes.is_empty() {
        rejections.push(Rejection::new(
            "image is empty; expected an ELF32 little-endian ARM executable",
        ));
        return Framing::Unusable;
    }

    if bytes.len() < ELF32_HEADER_LEN {
        rejections.push(Rejection::new(format!(
            "image is only {} byte{}, too small to contain an ELF32 header (need at least {ELF32_HEADER_LEN})",
            bytes.len(),
            if bytes.len() == 1 { "" } else { "s" },
        )));
        // Still attempt the magic check below when enough bytes exist for it.
        if bytes.len() < 4 {
            return Framing::Unusable;
        }
    }

    if bytes.len() >= 4 && bytes[0..4] != ELF_MAGIC {
        rejections.push(Rejection::new(format!(
            "not an ELF file: missing magic 0x7F 'E' 'L' 'F' (found {:02x} {:02x} {:02x} {:02x})",
            bytes[0], bytes[1], bytes[2], bytes[3]
        )));
        // Without a plausible ELF header the remaining structural checks are
        // noise; still return everything collected so far.
        return Framing::Unusable;
    }

    // From here we need a full ELF32 header.
    if bytes.len() < ELF32_HEADER_LEN {
        return Framing::Unusable;
    }

    Framing::Parsable
}

/// The `e_ident` bytes that decide how the rest of the header must be read.
///
/// The fields drop the spec's `ei_` prefix, which the struct name already
/// carries; each doc comment names the field it stands for so the module stays
/// checkable against the ELF specification.
#[derive(Clone, Copy)]
struct Ident {
    /// `EI_CLASS`: 1 for ELF32, 2 for ELF64.
    class: u8,
    /// `EI_DATA`: 1 for little-endian, 2 for big-endian.
    data: u8,
    /// `EI_VERSION`: always 1 in practice.
    version: u8,
}

impl Ident {
    /// Reads `e_ident` from a buffer already known to hold a full header.
    fn read(bytes: &[u8]) -> Self {
        Self {
            class: bytes[4],
            data: bytes[5],
            version: bytes[6],
        }
    }

    /// Whether the image claims the 32-bit little-endian encoding we can parse.
    ///
    /// Several later checks are conditional on this: interpreting a big-endian
    /// or 64-bit header with LE32 reads would invent addresses and sizes, and
    /// rejecting an image for a fabricated reason is worse than staying silent.
    fn is_le32(self) -> bool {
        self.class == ELFCLASS32 && self.data == ELFDATA2LSB
    }
}

/// Checks the identification bytes that decide the whole decoding strategy.
///
/// These three failures are the ones an operator hits when the wrong artifact
/// is picked up — a host binary, a cross-build for another endianness — so each
/// names the offending value explicitly rather than saying "malformed ELF".
fn check_ident(ident: Ident, rejections: &mut Vec<Rejection>) {
    if ident.class != ELFCLASS32 {
        let label = match ident.class {
            2 => "64-bit (ELFCLASS64)".to_owned(),
            0 => "invalid (ELFCLASSNONE)".to_owned(),
            other => format!("unrecognised (EI_CLASS={other})"),
        };
        rejections.push(Rejection::new(format!(
            "ELF is not 32-bit: expected EI_CLASS=1 (ELFCLASS32), found {label}"
        )));
    }

    if ident.data != ELFDATA2LSB {
        let label = match ident.data {
            2 => "big-endian (ELFDATA2MSB)".to_owned(),
            0 => "invalid (ELFDATANONE)".to_owned(),
            other => format!("unrecognised (EI_DATA={other})"),
        };
        rejections.push(Rejection::new(format!(
            "ELF is not little-endian: expected EI_DATA=1 (ELFDATA2LSB), found {label}"
        )));
    }

    if ident.version != EV_CURRENT {
        rejections.push(Rejection::new(format!(
            "unsupported ELF version in ident: expected {EV_CURRENT}, found {}",
            ident.version
        )));
    }
}

/// The multi-byte ELF32 header fields, decoded little-endian.
///
/// As with [`Ident`], the spec's `e_` prefix is dropped from the field names
/// and restated in each doc comment, so the mapping back to the specification
/// survives without every field reading the same.
struct Header {
    /// `e_type`: object file kind, must be `ET_EXEC` for a linked image.
    kind: u16,
    /// `e_machine`: target architecture.
    machine: u16,
    /// `e_version`: object file version.
    version: u32,
    /// `e_entry`: entry point address.
    entry: u32,
    /// `e_phoff`: file offset of the program header table.
    phoff: u32,
    /// `e_shoff`: file offset of the section header table.
    shoff: u32,
    /// `e_flags`: processor-specific flags; ARM ABI information lives here.
    flags: u32,
    /// `e_ehsize`: size of this header.
    ehsize: u16,
    /// `e_phentsize`: size of one program header entry.
    phentsize: u16,
    /// `e_phnum`: number of program header entries.
    phnum: u16,
    /// `e_shentsize`: size of one section header entry.
    shentsize: u16,
    /// `e_shnum`: number of section header entries.
    shnum: u16,
    /// `e_shstrndx`: index of the section name string table.
    shstrndx: u16,
}

impl Header {
    /// Decodes the header fields from a buffer that passed [`check_framing`].
    ///
    /// Multi-byte fields are only meaningful for a 32-bit little-endian image.
    /// When class/data are wrong we still try LE32 reads so a deliberately
    /// broken synthetic fixture can surface architecture errors alongside the
    /// class/endian failures, but we do not pretend the values are
    /// authoritative — [`Ident::is_le32`] gates everything that depends on them.
    fn read(bytes: &[u8]) -> Self {
        Self {
            kind: read_u16_le(bytes, 16),
            machine: read_u16_le(bytes, 18),
            version: read_u32_le(bytes, 20),
            entry: read_u32_le(bytes, 24),
            phoff: read_u32_le(bytes, 28),
            shoff: read_u32_le(bytes, 32),
            flags: read_u32_le(bytes, 36),
            ehsize: read_u16_le(bytes, 40),
            phentsize: read_u16_le(bytes, 42),
            phnum: read_u16_le(bytes, 44),
            shentsize: read_u16_le(bytes, 46),
            shnum: read_u16_le(bytes, 48),
            shstrndx: read_u16_le(bytes, 50),
        }
    }
}

/// Checks the header fields that decide whether this image belongs on a Teensy.
///
/// The architecture check is the one that matters most in practice: flashing a
/// host-built binary would upload megabytes of nonsense to the board, so the
/// message names the expected machine and spells out what was found instead.
fn check_header(ident: Ident, header: &Header, rejections: &mut Vec<Rejection>) {
    if header.version != 1 {
        rejections.push(Rejection::new(format!(
            "unsupported ELF e_version: expected 1, found {}",
            header.version
        )));
    }

    if header.kind != ET_EXEC {
        rejections.push(Rejection::new(format!(
            "ELF type is not an executable: expected ET_EXEC (2), found {}",
            header.kind
        )));
    }

    if header.machine != EM_ARM {
        rejections.push(Rejection::new(format!(
            "wrong architecture: expected EM_ARM (0x28) for the Teensy 4.0 (i.MX RT1062), found e_machine=0x{:x} ({})",
            header.machine,
            machine_name(header.machine)
        )));
    }

    if ident.class == ELFCLASS32 && header.ehsize as usize != ELF32_HEADER_LEN {
        rejections.push(Rejection::new(format!(
            "unexpected ELF header size: expected {ELF32_HEADER_LEN}, found {}",
            header.ehsize
        )));
    }
}

/// Corroborates the target triple from the ARM-specific `e_flags`.
///
/// This is the byte-level counterpart of [`validate_target_path`]: a soft-float
/// or pre-EABI5 image would run on the wrong ABI. Linkers are inconsistent
/// about populating these bits, so a mismatch is reported as a warning rather
/// than blocking a build that is otherwise sound.
fn check_arm_abi(ident: Ident, header: &Header, warnings: &mut Vec<String>) {
    // Target-triple-ish ABI check: thumbv7em-none-eabihf is hard-float EABI5.
    if header.machine != EM_ARM || ident.class != ELFCLASS32 {
        return;
    }

    let eabi = header.flags & 0xFF00_0000;
    if eabi != 0 && eabi != EF_ARM_EABI_VER5 {
        warnings.push(format!(
            "ARM EABI version in e_flags is 0x{eabi:x}, expected EABI5 (0x{EF_ARM_EABI_VER5:x}); continuing anyway"
        ));
    }
    if header.flags & EF_ARM_ABI_FLOAT_HARD == 0 {
        warnings.push(
            "ARM e_flags lack EF_ARM_ABI_FLOAT_HARD; image may not match thumbv7em-none-eabihf"
                .to_owned(),
        );
    }
}

/// What the program header walk learnt about the loadable image.
struct LoadSurvey {
    /// Sum of `p_filesz` across every `PT_LOAD` segment seen.
    loadable_size: u64,
    /// Whether a valid FCB tag was found at [`FLEXSPI_BASE`].
    fcb_ok: bool,
    /// Whether any `PT_LOAD` segment was seen at all.
    saw_load: bool,
}

/// Walks the program header table, accounting loadable bytes and the FCB.
///
/// Both facts come from the same table, so they are gathered in one pass: the
/// boot ROM only ever reads what the `PT_LOAD` segments describe, which is why
/// the size budget is measured here rather than from the file length or the
/// section headers.
fn walk_program_headers(
    bytes: &[u8],
    ident: Ident,
    header: &Header,
    rejections: &mut Vec<Rejection>,
) -> LoadSurvey {
    let mut survey = LoadSurvey {
        loadable_size: 0,
        fcb_ok: false,
        saw_load: false,
    };

    if !ident.is_le32() {
        return survey;
    }

    if header.phentsize as usize != ELF32_PHDR_LEN || header.phnum == 0 {
        // Headers claim LE32 but the program header table is unusable.
        if header.phnum > 0 {
            rejections.push(Rejection::new(format!(
                "cannot parse program headers: e_phentsize={} (expected {ELF32_PHDR_LEN}), e_phoff={}, e_phnum={}",
                header.phentsize, header.phoff, header.phnum
            )));
        }
        return survey;
    }

    let phoff = header.phoff as usize;
    for index in 0..header.phnum {
        let Some(offset) = phoff.checked_add(usize::from(index).saturating_mul(ELF32_PHDR_LEN))
        else {
            rejections.push(Rejection::new(format!(
                "program header {index} offset overflows"
            )));
            continue;
        };
        let Some(end) = offset.checked_add(ELF32_PHDR_LEN) else {
            rejections.push(Rejection::new(format!(
                "program header {index} extends past addressable memory"
            )));
            continue;
        };
        if end > bytes.len() {
            rejections.push(Rejection::new(format!(
                "program header {index} at offset {offset} extends past end of file ({} bytes)",
                bytes.len()
            )));
            continue;
        }

        let p_type = read_u32_le(bytes, offset);
        let p_offset = read_u32_le(bytes, offset + 4) as usize;
        let p_paddr = read_u32_le(bytes, offset + 12);
        let p_filesz = read_u32_le(bytes, offset + 16);

        if p_type != PT_LOAD {
            continue;
        }
        survey.saw_load = true;
        survey.loadable_size = survey.loadable_size.saturating_add(u64::from(p_filesz));

        if p_paddr == FLEXSPI_BASE {
            check_fcb_segment(bytes, p_offset, p_filesz, &mut survey.fcb_ok, rejections);
        }
    }

    survey
}

/// Inspects the segment mapped at [`FLEXSPI_BASE`] for the FCB tag.
///
/// The i.MX RT1062 boot ROM reads the `FlexSPI` Configuration Block from the
/// very start of external flash to learn how to talk to the chip; without it
/// the board comes up dead even though the rest of the image is perfect, and
/// the only cure is a PROGRAM-button reflash.
fn check_fcb_segment(
    bytes: &[u8],
    p_offset: usize,
    p_filesz: u32,
    fcb_ok: &mut bool,
    rejections: &mut Vec<Rejection>,
) {
    if p_filesz < 4 {
        rejections.push(Rejection::new(format!(
            "`FlexSPI` Configuration Block segment at physical {FLEXSPI_BASE:#010x} is only {p_filesz} byte{}; need at least 4 for the FCB tag",
            if p_filesz == 1 { "" } else { "s" }
        )));
        return;
    }

    let Some(tag_end) = p_offset.checked_add(4) else {
        rejections.push(Rejection::new(
            "`FlexSPI` Configuration Block segment file offset overflows",
        ));
        return;
    };

    if tag_end > bytes.len() {
        rejections.push(Rejection::new(format!(
            "`FlexSPI` Configuration Block segment at physical {FLEXSPI_BASE:#010x} points past end of file"
        )));
    } else if bytes[p_offset..tag_end] == FCB_TAG {
        *fcb_ok = true;
    } else {
        rejections.push(Rejection::new(format!(
            "`FlexSPI` Configuration Block missing or invalid at physical {FLEXSPI_BASE:#010x}: expected tag FCFB (0x42464346 LE), found {:02x} {:02x} {:02x} {:02x}",
            bytes[p_offset],
            bytes[p_offset + 1],
            bytes[p_offset + 2],
            bytes[p_offset + 3],
        )));
    }
}

/// Draws the conclusions that need the whole program header table.
///
/// Nothing here can be decided while walking segments one by one: the flash
/// budget is a total, and the absence of an FCB is only knowable once every
/// segment has been seen.
fn check_loadable_coverage(
    ident: Ident,
    header: &Header,
    load: &LoadSurvey,
    rejections: &mut Vec<Rejection>,
) {
    if ident.is_le32() && !load.saw_load && rejections.is_empty() {
        // Only complain about missing loads when the header was otherwise sane
        // enough that we would have seen them.
        if header.phnum == 0 {
            rejections.push(Rejection::new(
                "ELF has no program headers; cannot measure loadable size or locate the `FlexSPI` Configuration Block",
            ));
        }
    }

    if load.loadable_size > TEENSY40_FLASH_CAPACITY {
        rejections.push(Rejection::new(format!(
            "loadable image size {} bytes exceeds the Teensy 4.0 application flash partition of {TEENSY40_FLASH_CAPACITY} bytes (1984 KiB)",
            load.loadable_size
        )));
    }

    if ident.is_le32() && load.saw_load && !load.fcb_ok {
        // Avoid a duplicate message when we already rejected a bad tag or a
        // truncated FCB segment at the expected address.
        let already = rejections
            .iter()
            .any(|r| r.message.contains("`FlexSPI` Configuration Block"));
        if !already {
            rejections.push(Rejection::new(format!(
                "`FlexSPI` Configuration Block not found: need a PT_LOAD segment at physical address {FLEXSPI_BASE:#010x} whose first four bytes are the FCB tag FCFB"
            )));
        }
    }
}

/// The vector table entries recovered from the image, when it could be found.
struct VectorFacts {
    /// Initial stack pointer, the first word of the table.
    initial_sp: Option<u32>,
    /// Reset handler address, the second word of the table.
    reset_handler: Option<u32>,
}

/// Checks that the initial stack pointer and reset handler address are real.
///
/// A vector table pointing outside every memory region on the part is the
/// signature of a mislinked image, which would fault immediately on boot.
/// Failing to *locate* the table is a different matter and only warns: this
/// firmware keeps its table in DTCM, and guessing wrong must never cost the
/// operator a false rejection.
fn check_vector_table(
    bytes: &[u8],
    ident: Ident,
    header: &Header,
    rejections: &mut Vec<Rejection>,
    warnings: &mut Vec<String>,
) -> VectorFacts {
    let mut facts = VectorFacts {
        initial_sp: None,
        reset_handler: None,
    };

    if !ident.is_le32() {
        return facts;
    }

    match locate_vector_table(
        bytes,
        header.shoff,
        header.shentsize,
        header.shnum,
        header.shstrndx,
    ) {
        VectorTableLookup::Found { sp, reset } => {
            facts.initial_sp = Some(sp);
            facts.reset_handler = Some(reset);
            if !address_plausible(sp) {
                rejections.push(Rejection::new(format!(
                    "initial stack pointer {sp:#010x} is outside valid i.MX RT1062 regions \
                     (ITCM {ITCM_START:#010x}..{ITCM_END:#010x}, \
                     DTCM {DTCM_START:#010x}..{DTCM_END:#010x}, \
                     OCRAM {OCRAM_START:#010x}..{OCRAM_END:#010x}, \
                     `FlexSPI` {FLEXSPI_BASE:#010x}..{FLEXSPI_END:#010x})",
                )));
            }
            let reset_addr = reset & !1;
            if !address_plausible(reset_addr) {
                rejections.push(Rejection::new(format!(
                    "reset handler {reset:#010x} is outside valid i.MX RT1062 regions \
                     (ITCM {ITCM_START:#010x}..{ITCM_END:#010x}, \
                     DTCM {DTCM_START:#010x}..{DTCM_END:#010x}, \
                     OCRAM {OCRAM_START:#010x}..{OCRAM_END:#010x}, \
                     `FlexSPI` {FLEXSPI_BASE:#010x}..{FLEXSPI_END:#010x})",
                )));
            }
        }
        VectorTableLookup::Missing(reason) => {
            warnings.push(format!(
                "reset vector check skipped: {reason}. \
                 This is a warning, not a hard failure, to avoid false rejections when the vector table cannot be located reliably"
            ));
        }
    }

    facts
}

/// Verifies that an artifact path was produced for the firmware target triple.
///
/// Path checking lives outside [`validate_image`] because it is a property of
/// the build layout, not of the bytes themselves.
///
/// # Errors
///
/// Returns a rejection when the canonical firmware target directory name is
/// not present in the path.
pub fn validate_target_path(path: &std::path::Path) -> Result<(), Rejection> {
    const TARGET: &str = "thumbv7em-none-eabihf";
    let mut matched = false;
    for component in path.components() {
        if component.as_os_str() == TARGET {
            matched = true;
            break;
        }
    }
    if matched {
        Ok(())
    } else {
        Err(Rejection::new(format!(
            "artifact path does not contain the firmware target directory `{TARGET}`: {}",
            path.display()
        )))
    }
}

/// Computes the SHA-256 digest of `bytes`.
///
/// Exposed separately so callers hashing a HEX file after conversion share the
/// same formatting helper as [`validate_image`].
#[must_use]
pub fn sha256_digest(bytes: &[u8]) -> [u8; 32] {
    Sha256::digest(bytes).into()
}

/// Lower-case hex encoding of a byte slice.
#[must_use]
pub fn hex_encode(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        let _ = write!(out, "{byte:02x}");
    }
    out
}

// Named copies of range bounds for interpolation in error messages (range
// Debug formatting is not as readable as explicit hex endpoints).
const ITCM_START: u32 = ITCM.start;
const ITCM_END: u32 = ITCM.end;
const DTCM_START: u32 = DTCM.start;
const DTCM_END: u32 = DTCM.end;
const OCRAM_START: u32 = OCRAM.start;
const OCRAM_END: u32 = OCRAM.end;

fn address_plausible(addr: u32) -> bool {
    ITCM.contains(&addr) || DTCM.contains(&addr) || OCRAM.contains(&addr) || FLEXSPI.contains(&addr)
}

fn machine_name(machine: u16) -> &'static str {
    match machine {
        0x3E => "EM_X86_64",
        0x03 => "EM_386",
        0xB7 => "EM_AARCH64",
        0xF3 => "EM_RISCV",
        0x28 => "EM_ARM",
        _ => "unknown",
    }
}

fn read_u16_le(bytes: &[u8], offset: usize) -> u16 {
    u16::from_le_bytes([bytes[offset], bytes[offset + 1]])
}

fn read_u32_le(bytes: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes([
        bytes[offset],
        bytes[offset + 1],
        bytes[offset + 2],
        bytes[offset + 3],
    ])
}

enum VectorTableLookup {
    Found { sp: u32, reset: u32 },
    Missing(String),
}

/// Locates the interrupt vector table from section headers when present.
///
/// This firmware relocates the vector table into DTCM (`.vector_table` at
/// `0x2000_4000`); a classic "vectors at the image base" check would false-reject
/// it. Preferring the section name keeps the check honest about what the
/// linker actually produced.
fn locate_vector_table(
    bytes: &[u8],
    e_shoff: u32,
    e_shentsize: u16,
    e_shnum: u16,
    e_shstrndx: u16,
) -> VectorTableLookup {
    if e_shnum == 0 || e_shoff == 0 {
        return VectorTableLookup::Missing(
            "ELF has no section headers, so `.vector_table` cannot be found".to_owned(),
        );
    }
    if e_shentsize as usize != ELF32_SHDR_LEN {
        return VectorTableLookup::Missing(format!(
            "unsupported section header size {e_shentsize} (expected {ELF32_SHDR_LEN})"
        ));
    }
    if usize::from(e_shstrndx) >= usize::from(e_shnum) {
        return VectorTableLookup::Missing(format!(
            "e_shstrndx {e_shstrndx} is out of range for e_shnum {e_shnum}"
        ));
    }

    let shoff = e_shoff as usize;
    let Some(shstr_hdr_off) =
        shoff.checked_add(usize::from(e_shstrndx).saturating_mul(ELF32_SHDR_LEN))
    else {
        return VectorTableLookup::Missing(
            "section header string table offset overflows".to_owned(),
        );
    };
    let Some(shstr_hdr_end) = shstr_hdr_off.checked_add(ELF32_SHDR_LEN) else {
        return VectorTableLookup::Missing(
            "section header string table header extends past addressable memory".to_owned(),
        );
    };
    if shstr_hdr_end > bytes.len() {
        return VectorTableLookup::Missing(
            "section header string table header extends past end of file".to_owned(),
        );
    }

    let str_off = read_u32_le(bytes, shstr_hdr_off + 16) as usize;
    let str_size = read_u32_le(bytes, shstr_hdr_off + 20) as usize;
    let Some(str_end) = str_off.checked_add(str_size) else {
        return VectorTableLookup::Missing("section name string table extent overflows".to_owned());
    };
    if str_end > bytes.len() {
        return VectorTableLookup::Missing(
            "section name string table extends past end of file".to_owned(),
        );
    }
    let strtab = &bytes[str_off..str_end];

    for index in 0..e_shnum {
        let Some(hdr_off) = shoff.checked_add(usize::from(index).saturating_mul(ELF32_SHDR_LEN))
        else {
            continue;
        };
        let Some(hdr_end) = hdr_off.checked_add(ELF32_SHDR_LEN) else {
            continue;
        };
        if hdr_end > bytes.len() {
            continue;
        }

        let name_off = read_u32_le(bytes, hdr_off) as usize;
        let Some(name) = read_c_string(strtab, name_off) else {
            continue;
        };
        if name != ".vector_table" {
            continue;
        }

        let sh_offset = read_u32_le(bytes, hdr_off + 16) as usize;
        let sh_size = read_u32_le(bytes, hdr_off + 20) as usize;
        if sh_size < 8 {
            return VectorTableLookup::Missing(format!(
                "`.vector_table` section is only {sh_size} byte{}, need at least 8 for SP and reset",
                if sh_size == 1 { "" } else { "s" }
            ));
        }
        let Some(payload_end) = sh_offset.checked_add(8) else {
            return VectorTableLookup::Missing(
                "`.vector_table` section offset overflows".to_owned(),
            );
        };
        if payload_end > bytes.len() {
            return VectorTableLookup::Missing(
                "`.vector_table` section extends past end of file".to_owned(),
            );
        }

        let sp = read_u32_le(bytes, sh_offset);
        let reset = read_u32_le(bytes, sh_offset + 4);
        return VectorTableLookup::Found { sp, reset };
    }

    VectorTableLookup::Missing(
        "no section named `.vector_table` (this firmware keeps the table in DTCM, not at the image base)".to_owned(),
    )
}

fn read_c_string(haystack: &[u8], offset: usize) -> Option<&str> {
    if offset >= haystack.len() {
        return None;
    }
    let tail = &haystack[offset..];
    let end = tail.iter().position(|&b| b == 0).unwrap_or(tail.len());
    std::str::from_utf8(&tail[..end]).ok()
}
