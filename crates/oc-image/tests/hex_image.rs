//! Intel HEX files read into flash contents, and the boot header checked in
//! them.
//!
//! Files are written in memory from flash contents; one optional test reads
//! the real firmware's HEX, when `cargo xtask hex` has left it behind, and
//! compares it with the layout of its ELF.

use std::fmt::Write as _;
use std::path::PathBuf;

use oc_image::boot::{IVT_OFFSET, check_boot_header};
use oc_image::flash::{ERASED, Format, firmware_image, flash_image, hex_image};
use oc_image::hex::read_hex;
use oc_image::validate::{FCB_TAG, FLEXSPI_BASE, RESERVED_FLASH_START, Rejection};

/// Where the boot data sit in [`boot_flash`], as `imxrt-rt` puts them.
const BOOT_DATA_OFFSET: usize = IVT_OFFSET + 0x20;

/// One record, checksum included.
fn record(kind: u8, address: u16, data: &[u8]) -> String {
    let mut bytes = vec![u8::try_from(data.len()).unwrap()];
    bytes.extend(address.to_be_bytes());
    bytes.push(kind);
    bytes.extend(data);
    let sum = bytes.iter().fold(0_u8, |sum, &byte| sum.wrapping_add(byte));
    bytes.push(sum.wrapping_neg());
    let mut line = String::from(":");
    for byte in bytes {
        write!(line, "{byte:02X}").unwrap();
    }
    line + "\n"
}

/// The data records of `data` at bus address `start`, 16 bytes a record,
/// with the extended linear address records they need.
fn data_records(start: u32, data: &[u8]) -> String {
    let mut out = String::new();
    let mut upper = None;
    for (index, chunk) in data.chunks(16).enumerate() {
        let address = start + u32::try_from(index * 16).unwrap();
        let high = u16::try_from(address >> 16).unwrap();
        if upper != Some(high) {
            out += &record(0x04, 0, &high.to_be_bytes());
            upper = Some(high);
        }
        out += &record(0x00, u16::try_from(address & 0xFFFF).unwrap(), chunk);
    }
    out
}

/// A whole file holding `flash` from [`FLEXSPI_BASE`].
fn hex_file(flash: &[u8]) -> String {
    data_records(FLEXSPI_BASE, flash) + &record(0x01, 0, &[])
}

/// Flash contents with a boot header as this firmware's: FCB, IVT pointing
/// at the vector table (`entry`) and at boot data claiming `size` bytes.
fn boot_flash(version: u8, entry: u32, size: u32) -> Vec<u8> {
    let mut flash = vec![ERASED; IVT_OFFSET + 0x1010];
    flash[..4].copy_from_slice(&FCB_TAG);
    let ivt_address = FLEXSPI_BASE + u32::try_from(IVT_OFFSET).unwrap();
    let boot_data = FLEXSPI_BASE + u32::try_from(BOOT_DATA_OFFSET).unwrap();
    let mut ivt = vec![0xD1, 0x00, 0x20, version];
    for word in [entry, 0, 0, boot_data, ivt_address, 0, 0] {
        ivt.extend(word.to_le_bytes());
    }
    flash[IVT_OFFSET..IVT_OFFSET + 32].copy_from_slice(&ivt);
    for (at, word) in [FLEXSPI_BASE, size, 0].into_iter().enumerate() {
        let offset = BOOT_DATA_OFFSET + at * 4;
        flash[offset..offset + 4].copy_from_slice(&word.to_le_bytes());
    }
    // The vector table, in flash: initial stack pointer, reset handler.
    let table = (entry - FLEXSPI_BASE) as usize;
    flash[table..table + 4].copy_from_slice(&0x2000_4000_u32.to_le_bytes());
    flash[table + 4..table + 8].copy_from_slice(&0x6000_1031_u32.to_le_bytes());
    flash
}

fn valid_flash() -> Vec<u8> {
    boot_flash(0x40, 0x6000_2000, 0x2010)
}

fn messages(rejections: &[Rejection]) -> String {
    rejections
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join("\n")
}

fn refused(file: &str) -> String {
    messages(&firmware_image(file.as_bytes()).expect_err("the file is refused"))
}

#[test]
fn a_valid_file_reads_back_its_bytes_with_reduced_checks() {
    let flash = valid_flash();
    let file = hex_file(&flash);
    let image = firmware_image(file.as_bytes()).expect("a valid file");
    assert_eq!(image.format, Format::Hex);
    assert_eq!(image.bytes(), flash.as_slice());
    assert_eq!(image.entry, 0x6000_2000);
    assert!(image.warnings[0].contains("reduced checks"));
    assert_eq!(
        image.sha256_hex().len(),
        64,
        "the digest is the file's, rendered in hex"
    );
}

#[test]
fn gaps_stay_erased_and_crlf_lines_are_read() {
    let mut flash = valid_flash();
    flash.resize(0x3000, ERASED);
    let mut file = data_records(FLEXSPI_BASE, &flash[..0x2010]);
    file += &data_records(FLEXSPI_BASE + 0x2FF0, &[0x5A; 0x10]);
    file += &record(0x01, 0, &[]);
    let image = hex_image(file.replace('\n', "\r\n").as_bytes()).expect("a valid file");
    assert!(
        image.bytes()[0x2010..0x2FF0]
            .iter()
            .all(|&byte| byte == ERASED)
    );
    assert_eq!(&image.bytes()[0x2FF0..], &[0x5A; 0x10]);
}

#[test]
fn a_segment_address_record_moves_the_base() {
    // Record 02 gives the base in 16-byte paragraphs: 0x1000 puts the data
    // at 0x0001_0000, outside the flash.
    let file = record(0x02, 0, &[0x10, 0x00]) + &record(0x00, 0, &[1]) + &record(0x01, 0, &[]);
    assert!(refused(&file).contains("0x00010000, outside the application flash"));
}

#[test]
fn a_truncated_file_is_refused() {
    let file = data_records(FLEXSPI_BASE, &valid_flash());
    assert!(refused(&file).contains("no end record"));
}

#[test]
fn a_damaged_record_is_refused_with_its_line() {
    let mut file = hex_file(&valid_flash());
    // The second line is the first data record; change one data digit.
    let start = file.find('\n').unwrap() + 1 + 9;
    file.replace_range(
        start..=start,
        if &file[start..=start] == "0" {
            "1"
        } else {
            "0"
        },
    );
    let message = refused(&file);
    assert!(message.contains("line 2: bad checksum"), "{message}");
}

#[test]
fn lines_that_are_not_records_are_refused_and_counted() {
    let mut file = hex_file(&valid_flash());
    for _ in 0..15 {
        file.insert_str(0, "hello\n");
    }
    let message = messages(&read_hex(file.as_bytes()).unwrap_err());
    assert!(
        message.contains("line 1: not an Intel HEX record"),
        "{message}"
    );
    assert!(message.contains("and 5 more problems"), "{message}");
}

#[test]
fn data_outside_the_application_flash_is_refused() {
    let flash = valid_flash();
    let ram = data_records(0x2000_0000, &[0; 4]);
    let reserved = data_records(RESERVED_FLASH_START - 2, &[0; 4]);
    let file = data_records(FLEXSPI_BASE, &flash) + &ram + &reserved + &record(0x01, 0, &[]);
    let message = refused(&file);
    assert!(
        message.contains("4 bytes at 0x20000000, outside"),
        "{message}"
    );
    assert!(
        message.contains("4 bytes at 0x601efffe, outside"),
        "{message}"
    );
}

#[test]
fn an_address_written_twice_is_refused() {
    let flash = valid_flash();
    let file = data_records(FLEXSPI_BASE, &flash)
        + &data_records(FLEXSPI_BASE + 0x10, &[0; 4])
        + &record(0x01, 0, &[]);
    assert!(refused(&file).contains("0x60000010 is written twice"));
}

#[test]
fn a_record_after_the_end_is_refused() {
    let file = hex_file(&valid_flash()) + &record(0x00, 0, &[1]);
    assert!(refused(&file).contains("after the end record"));
}

#[test]
fn a_file_of_neither_kind_is_refused() {
    let message = messages(&firmware_image(b"PK\x03\x04").unwrap_err());
    assert!(message.contains("neither a firmware ELF nor an Intel HEX file"));
    assert!(
        read_hex(record(0x01, 0, &[]).as_bytes()).is_err(),
        "no data"
    );
}

#[test]
fn a_teensyduino_boot_header_is_accepted() {
    // Teensyduino writes version 0x43 and points the entry at the reset
    // handler, Thumb bit set (`teensy4/bootdata.c`).
    let mut flash = boot_flash(0x43, 0x6000_2000, 0x2010);
    flash[IVT_OFFSET + 4..IVT_OFFSET + 8].copy_from_slice(&0x6000_1235_u32.to_le_bytes());
    let header = check_boot_header(&flash).expect("a Teensyduino header");
    assert_eq!(header.entry, 0x6000_1235);
    assert_eq!(header.image_size, 0x2010);
}

#[test]
fn a_missing_fcb_or_ivt_is_refused() {
    let mut flash = valid_flash();
    flash[0] = 0;
    let message = messages(&check_boot_header(&flash).unwrap_err());
    assert!(
        message.contains("`FlexSPI` Configuration Block missing"),
        "{message}"
    );

    let mut flash = valid_flash();
    flash[IVT_OFFSET] = 0;
    let message = messages(&check_boot_header(&flash).unwrap_err());
    assert!(
        message.contains("no Image Vector Table at 0x60001000"),
        "{message}"
    );

    let message = messages(&check_boot_header(&valid_flash()[..0x800]).unwrap_err());
    assert!(
        message.contains("ends before its Image Vector Table"),
        "{message}"
    );
}

#[test]
fn an_inconsistent_ivt_is_refused() {
    let mut flash = valid_flash();
    flash[IVT_OFFSET + 20..IVT_OFFSET + 24].copy_from_slice(&0x6000_2000_u32.to_le_bytes());
    flash[IVT_OFFSET + 4..IVT_OFFSET + 8].copy_from_slice(&0x9000_0000_u32.to_le_bytes());
    let message = messages(&check_boot_header(&flash).unwrap_err());
    assert!(message.contains("says it lives at 0x60002000"), "{message}");
    assert!(
        message.contains("entry point 0x90000000 is outside"),
        "{message}"
    );
}

#[test]
fn boot_data_out_of_place_or_too_large_are_refused() {
    let mut flash = valid_flash();
    flash[BOOT_DATA_OFFSET..BOOT_DATA_OFFSET + 4].copy_from_slice(&0x7000_0000_u32.to_le_bytes());
    flash[BOOT_DATA_OFFSET + 4..BOOT_DATA_OFFSET + 8]
        .copy_from_slice(&(3 * 1024 * 1024_u32).to_le_bytes());
    let message = messages(&check_boot_header(&flash).unwrap_err());
    assert!(message.contains("starts at 0x70000000"), "{message}");
    assert!(message.contains("claims 3145728 bytes"), "{message}");

    let mut flash = valid_flash();
    flash[IVT_OFFSET + 16..IVT_OFFSET + 20].copy_from_slice(&0x2000_0000_u32.to_le_bytes());
    let message = messages(&check_boot_header(&flash).unwrap_err());
    assert!(
        message.contains("boot data at 0x20000000, outside the image"),
        "{message}"
    );
}

/// The real firmware's HEX reads into the same flash contents as its ELF,
/// and passes the boot header check, when `cargo xtask hex` has left both
/// files behind.
#[test]
fn real_firmware_hex_reads_as_its_elf_when_present() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let elf = root.join("target/thumbv7em-none-eabihf/release/oc-firmware");
    let hex = root.join("dist/oc-firmware.hex");
    let (Ok(elf_bytes), Ok(hex_bytes)) = (std::fs::read(&elf), std::fs::read(&hex)) else {
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

    let from_elf = flash_image(&elf_bytes).expect("the real ELF passes the gate");
    let from_hex = firmware_image(&hex_bytes).expect("the real HEX passes its checks");
    assert_eq!(from_hex.format, Format::Hex);
    assert_eq!(from_hex.bytes(), from_elf.bytes());
    // The entries differ (the ELF's is the reset handler, the boot
    // header's the vector table naming it); the header is there either way.
    check_boot_header(from_elf.bytes()).expect("the ELF's flash holds a boot header too");
}
