//! The flash contents laid out from an ELF's loadable segments.
//!
//! Synthetic images are built in memory, as in `xtask/tests/validate_image.rs`;
//! one optional test compares the real firmware's layout with the HEX file
//! `llvm-objcopy` made from it, when both happen to be present.

use std::path::PathBuf;

use oc_image::flash::{ERASED, Format, flash_image};
use oc_image::validate::{FCB_TAG, FLEXSPI_BASE, RESERVED_FLASH_START};

const PT_LOAD: u32 = 1;
const ELF_HEADER: usize = 52;
const PROGRAM_HEADER: usize = 32;

/// One `PT_LOAD` segment: its physical address and its bytes.
struct Load {
    paddr: u32,
    data: Vec<u8>,
}

fn load(paddr: u32, data: &[u8]) -> Load {
    Load {
        paddr,
        data: data.to_vec(),
    }
}

/// A minimal ARM executable holding `loads`, with no section headers (the
/// gate only warns that it cannot find the vector table).
fn elf(loads: &[Load]) -> Vec<u8> {
    let phoff = ELF_HEADER;
    let mut offset = phoff + loads.len() * PROGRAM_HEADER;
    let mut bytes = vec![0; offset];
    bytes[..7].copy_from_slice(&[0x7F, b'E', b'L', b'F', 1, 1, 1]);
    put16(&mut bytes, 16, 2); // ET_EXEC
    put16(&mut bytes, 18, 0x28); // EM_ARM
    put32(&mut bytes, 20, 1);
    put32(&mut bytes, 24, 1); // e_entry
    put32(&mut bytes, 28, u32::try_from(phoff).unwrap());
    put32(&mut bytes, 36, 0x0500_0400); // EABI5, hard float
    put16(&mut bytes, 40, 52);
    put16(&mut bytes, 42, 32);
    put16(&mut bytes, 44, u16::try_from(loads.len()).unwrap());
    put16(&mut bytes, 46, 40);

    for (index, segment) in loads.iter().enumerate() {
        let header = phoff + index * PROGRAM_HEADER;
        let size = u32::try_from(segment.data.len()).unwrap();
        put32(&mut bytes, header, PT_LOAD);
        put32(&mut bytes, header + 4, u32::try_from(offset).unwrap());
        put32(&mut bytes, header + 8, segment.paddr);
        put32(&mut bytes, header + 12, segment.paddr);
        put32(&mut bytes, header + 16, size);
        put32(&mut bytes, header + 20, size);
        bytes.extend(&segment.data);
        offset += segment.data.len();
    }
    bytes
}

fn put16(bytes: &mut [u8], at: usize, value: u16) {
    bytes[at..at + 2].copy_from_slice(&value.to_le_bytes());
}

fn put32(bytes: &mut [u8], at: usize, value: u32) {
    bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
}

fn fcb() -> Vec<u8> {
    let mut block = FCB_TAG.to_vec();
    block.extend([1, 2, 3, 4]);
    block
}

#[test]
fn segments_land_at_their_load_address_with_erased_gaps() {
    let image = flash_image(&elf(&[
        load(FLEXSPI_BASE + 0x100, &[0xAB; 3]),
        load(FLEXSPI_BASE, &fcb()),
    ]))
    .expect("a valid image");
    let bytes = image.bytes();
    assert_eq!(bytes.len(), 0x103);
    assert_eq!(&bytes[..8], fcb().as_slice());
    assert!(bytes[8..0x100].iter().all(|&byte| byte == ERASED));
    assert_eq!(&bytes[0x100..], &[0xAB; 3]);
    assert_eq!(image.format, Format::Elf);
}

#[test]
fn a_segment_without_bytes_writes_nothing() {
    let image = flash_image(&elf(&[load(FLEXSPI_BASE, &fcb()), load(0x2000_0000, &[])]))
        .expect("a .bss-like segment is fine");
    assert_eq!(image.bytes(), fcb().as_slice());
}

#[test]
fn a_segment_loading_outside_the_flash_is_refused() {
    let error = flash_image(&elf(&[
        load(FLEXSPI_BASE, &fcb()),
        load(0x2000_0000, &[1, 2, 3]),
    ]))
    .expect_err("RAM-only bytes would never be written");
    assert!(
        error
            .iter()
            .any(|rejection| rejection.message.contains("outside the application flash")),
        "{error:?}"
    );
}

#[test]
fn overlapping_segments_are_refused() {
    let error = flash_image(&elf(&[
        load(FLEXSPI_BASE, &fcb()),
        load(FLEXSPI_BASE + 4, &[0; 4]),
    ]))
    .expect_err("two segments writing the same bytes");
    assert!(
        error
            .iter()
            .any(|rejection| rejection.message.contains("overlap")),
        "{error:?}"
    );
}

#[test]
fn the_gate_still_runs_first() {
    let error = flash_image(&elf(&[load(RESERVED_FLASH_START - 2, &[0; 4])]))
        .expect_err("no FCB, and the settings overwritten");
    assert!(
        error
            .iter()
            .any(|rejection| rejection.message.contains("reserved flash")),
        "{error:?}"
    );
}

/// The real firmware laid out here byte for byte as `llvm-objcopy -O ihex`
/// lays it out, when `cargo xtask hex` has left both files behind.
#[test]
fn real_firmware_matches_its_hex_when_present() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let elf = root.join("target/thumbv7em-none-eabihf/release/oc-firmware");
    let hex = root.join("dist/oc-firmware.hex");
    let (Ok(elf_bytes), Ok(hex_text)) = (std::fs::read(&elf), std::fs::read_to_string(&hex)) else {
        eprintln!("skipped: no firmware ELF and HEX (run `cargo xtask hex`)");
        return;
    };
    let elf_age = std::fs::metadata(&elf).and_then(|meta| meta.modified());
    let hex_age = std::fs::metadata(&hex).and_then(|meta| meta.modified());
    if let (Ok(elf_age), Ok(hex_age)) = (elf_age, hex_age)
        && hex_age < elf_age
    {
        eprintln!("skipped: the HEX is older than the ELF (run `cargo xtask hex`)");
        return;
    }

    let image = flash_image(&elf_bytes).expect("the real firmware passes the gate");
    let mut from_hex = vec![ERASED; image.bytes().len()];
    let mut written = 0_usize;
    let mut base = 0_u32;
    for line in hex_text.lines() {
        let record = decode_hex_record(line);
        let (count, address, kind) = (
            record[0],
            u16::from_be_bytes([record[1], record[2]]),
            record[3],
        );
        let data = &record[4..4 + usize::from(count)];
        match kind {
            0 => {
                let at = (base + u32::from(address) - FLEXSPI_BASE) as usize;
                from_hex[at..at + data.len()].copy_from_slice(data);
                written = written.max(at + data.len());
            }
            4 => base = u32::from(u16::from_be_bytes([data[0], data[1]])) << 16,
            _ => {}
        }
    }
    assert_eq!(written, image.bytes().len(), "same extent as the HEX");
    assert!(from_hex == image.bytes(), "same bytes as the HEX");
}

fn decode_hex_record(line: &str) -> Vec<u8> {
    let digits = line.trim().trim_start_matches(':');
    (0..digits.len())
        .step_by(2)
        .map(|at| u8::from_str_radix(&digits[at..at + 2], 16).expect("hex digits"))
        .collect()
}
