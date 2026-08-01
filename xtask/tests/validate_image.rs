//! Integration tests for the pre-flash image gate.
//!
//! Each case builds a synthetic byte buffer in memory so the suite does not
//! depend on a firmware build. A separate optional test exercises the real
//! release ELF when it happens to be present.

use std::path::PathBuf;

use xtask::validate::{
    self, FCB_TAG, FLEXSPI_BASE, TEENSY40_FLASH_CAPACITY, hex_encode, sha256_digest, validate_image,
};

/// ARM EABI5 + hard-float, matching `thumbv7em-none-eabihf`.
const EF_ARM_EABI5_HARDFLOAT: u32 = 0x0500_0400;
const EM_ARM: u16 = 0x28;
const EM_X86_64: u16 = 0x3E;
const ET_EXEC: u16 = 2;
const PT_LOAD: u32 = 1;
const SHT_PROGBITS: u32 = 1;
const SHT_STRTAB: u32 = 3;

#[test]
fn valid_minimal_arm_elf_passes() {
    let elf = minimal_valid_arm_elf();
    let facts = validate_image(&elf).expect("valid synthetic ARM ELF should pass");
    assert!(facts.loadable_size >= 4);
    assert_eq!(facts.initial_sp, Some(0x2000_4000));
    assert_eq!(facts.reset_handler, Some(0x0000_0001));
    assert!(
        facts.warnings.is_empty(),
        "unexpected warnings: {:?}",
        facts.warnings
    );
}

#[test]
fn x86_64_elf_is_rejected_with_architecture_message() {
    let mut elf = minimal_valid_arm_elf();
    // e_machine at offset 18.
    elf[18..20].copy_from_slice(&EM_X86_64.to_le_bytes());

    let err = validate_image(&elf).expect_err("x86-64 must be rejected");
    assert!(
        err.iter().any(|r| {
            r.message.contains("architecture")
                || r.message.contains("EM_X86_64")
                || r.message.contains("e_machine")
        }),
        "expected architecture rejection, got {err:?}"
    );
}

#[test]
fn elf64_is_rejected() {
    let mut elf = minimal_valid_arm_elf();
    elf[4] = 2; // ELFCLASS64

    let err = validate_image(&elf).expect_err("64-bit ELF must be rejected");
    assert!(
        err.iter()
            .any(|r| r.message.contains("64-bit") || r.message.contains("ELFCLASS64")),
        "expected 64-bit rejection, got {err:?}"
    );
}

#[test]
fn big_endian_elf_is_rejected() {
    let mut elf = minimal_valid_arm_elf();
    elf[5] = 2; // ELFDATA2MSB

    let err = validate_image(&elf).expect_err("big-endian ELF must be rejected");
    assert!(
        err.iter()
            .any(|r| r.message.contains("big-endian") || r.message.contains("ELFDATA2MSB")),
        "expected big-endian rejection, got {err:?}"
    );
}

#[test]
fn truncated_file_is_rejected_without_panic() {
    let err = validate_image(&[0x7F, b'E', b'L', b'F', 1, 1, 1])
        .expect_err("truncated ELF must be rejected");
    assert!(
        err.iter().any(|r| r.message.contains("too small")),
        "expected size rejection, got {err:?}"
    );
}

#[test]
fn empty_file_is_rejected() {
    let err = validate_image(&[]).expect_err("empty file must be rejected");
    assert!(
        err.iter().any(|r| r.message.contains("empty")),
        "expected empty rejection, got {err:?}"
    );
}

#[test]
fn non_elf_file_is_rejected() {
    let hex_text = b":020000040000FA\n:00000001FF\n";
    let err = validate_image(hex_text).expect_err("Intel HEX text must be rejected");
    assert!(
        err.iter()
            .any(|r| r.message.contains("not an ELF") || r.message.contains("magic")),
        "expected non-ELF rejection, got {err:?}"
    );
}

#[test]
fn image_exceeding_flash_capacity_is_rejected() {
    let oversize = TEENSY40_FLASH_CAPACITY + 1;
    let elf = arm_elf_with_load(
        FLEXSPI_BASE,
        FCB_TAG,
        // Claim a huge p_filesz without actually allocating that many bytes.
        Some(oversize),
        true,
    );

    let err = validate_image(&elf).expect_err("oversize image must be rejected");
    assert!(
        err.iter().any(|r| {
            r.message.contains("1984")
                || r.message.contains("flash partition")
                || r.message.contains(&TEENSY40_FLASH_CAPACITY.to_string())
        }),
        "expected size-limit rejection, got {err:?}"
    );
}

#[test]
fn missing_fcb_is_rejected() {
    let elf = arm_elf_with_load(FLEXSPI_BASE, *b"NOPE", None, true);

    let err = validate_image(&elf).expect_err("missing FCB must be rejected");
    assert!(
        err.iter().any(|r| {
            r.message.contains("FlexSPI Configuration Block") || r.message.contains("FCFB")
        }),
        "expected FCB rejection, got {err:?}"
    );
}

#[test]
fn all_failures_are_reported_at_once() {
    // Wrong architecture and missing FCB — two independent hard failures.
    let mut elf = arm_elf_with_load(FLEXSPI_BASE, *b"XXXX", None, true);
    elf[18..20].copy_from_slice(&EM_X86_64.to_le_bytes());

    let err = validate_image(&elf).expect_err("compound fixture must fail");
    assert!(
        err.len() >= 2,
        "expected at least two rejections, got {}: {err:?}",
        err.len()
    );

    let joined = err
        .iter()
        .map(|r| r.message.as_str())
        .collect::<Vec<_>>()
        .join(" | ");
    assert!(
        joined.contains("architecture")
            || joined.contains("EM_X86_64")
            || joined.contains("e_machine"),
        "missing architecture rejection in {joined}"
    );
    assert!(
        joined.contains("FlexSPI") || joined.contains("FCFB"),
        "missing FCB rejection in {joined}"
    );
}

#[test]
fn sha256_digest_is_stable_for_known_input() {
    // Empty input is a published SHA-256 test vector.
    assert_eq!(
        hex_encode(&sha256_digest(b"")),
        "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
    );

    // Pre-computed with standard SHA-256 over a fixed fixture string.
    assert_eq!(
        hex_encode(&sha256_digest(b"alambic-firmware-gate")),
        "7793af60c743d1b02ca4bdd414251965c7697d6a012a5ff284cef86a3ebb545c"
    );

    // validate_image must report the same digest for a valid image.
    let elf = minimal_valid_arm_elf();
    let facts = validate_image(&elf).expect("valid elf");
    assert_eq!(facts.sha256, sha256_digest(&elf));
    assert_eq!(facts.sha256_hex(), hex_encode(&sha256_digest(&elf)));
}

#[test]
fn real_firmware_elf_passes_when_present() {
    let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    path.pop(); // workspace root
    path.push("target");
    path.push("thumbv7em-none-eabihf");
    path.push("release");
    path.push("oc-firmware");

    if !path.is_file() {
        eprintln!(
            "skipping real-firmware validation: {} not found (build with cargo xtask build)",
            path.display()
        );
        return;
    }

    let bytes = std::fs::read(&path).expect("read real firmware ELF");
    let facts = validate_image(&bytes).unwrap_or_else(|rejections| {
        panic!(
            "real firmware ELF failed validation: {}",
            rejections
                .iter()
                .map(|r| r.message.as_str())
                .collect::<Vec<_>>()
                .join("; ")
        );
    });

    assert!(facts.loadable_size > 0);
    assert!(facts.loadable_size <= TEENSY40_FLASH_CAPACITY);
    assert!(
        facts.initial_sp.is_some() && facts.reset_handler.is_some(),
        "expected vector table on the real image; warnings={:?}",
        facts.warnings
    );

    validate::validate_target_path(&path).expect("real path contains target triple");
}

/// Minimal LE32 ARM `ET_EXEC` with FCB load segment and a `.vector_table` section.
fn minimal_valid_arm_elf() -> Vec<u8> {
    arm_elf_with_load(FLEXSPI_BASE, FCB_TAG, None, true)
}

/// Builds a synthetic ELF32 LE ARM executable.
///
/// `claimed_filesz` overrides the program-header `p_filesz` without growing the
/// file — used to exercise the flash-capacity check cheaply.
fn arm_elf_with_load(
    paddr: u32,
    load_prefix: [u8; 4],
    claimed_filesz: Option<u64>,
    with_vector_table: bool,
) -> Vec<u8> {
    // Layout:
    //  [0, 52)            ELF header
    //  [52, 84)           one PT_LOAD program header
    //  [84, 84+N)         load segment bytes (at least 4)
    //  optional:
    //  [..]               .vector_table payload (8 bytes)
    //  [..]               .shstrtab
    //  [..]               3 section headers: null, .vector_table, .shstrtab
    let load_content_len = 16usize;
    let phoff = 52usize;
    let load_off = 84usize;
    let mut body_end = load_off + load_content_len;

    let mut vector_off = 0usize;
    let mut shstr_off = 0usize;
    let mut shoff = 0usize;
    let shnum: u16;
    let shstrndx: u16;

    if with_vector_table {
        vector_off = align_up(body_end, 4);
        let vector_end = vector_off + 8;
        shstr_off = vector_end;
        // shstrtab: "\0.vector_table\0.shstrtab\0"
        let shstr = b"\0.vector_table\0.shstrtab\0";
        let shstr_end = shstr_off + shstr.len();
        shoff = align_up(shstr_end, 4);
        shnum = 3;
        shstrndx = 2;
        body_end = shoff + 3 * 40;
        let _ = shstr;
    } else {
        shnum = 0;
        shstrndx = 0;
    }

    let mut elf = vec![0u8; body_end];

    // e_ident
    elf[0..4].copy_from_slice(&[0x7F, b'E', b'L', b'F']);
    elf[4] = 1; // ELFCLASS32
    elf[5] = 1; // ELFDATA2LSB
    elf[6] = 1; // EV_CURRENT
    // rest of ident zeroed

    write_u16(&mut elf, 16, ET_EXEC);
    write_u16(&mut elf, 18, EM_ARM);
    write_u32(&mut elf, 20, 1); // e_version
    write_u32(&mut elf, 24, 0x0000_0001); // e_entry (Thumb)
    write_u32(&mut elf, 28, u32::try_from(phoff).expect("phoff"));
    write_u32(
        &mut elf,
        32,
        if with_vector_table {
            u32::try_from(shoff).expect("shoff")
        } else {
            0
        },
    );
    write_u32(&mut elf, 36, EF_ARM_EABI5_HARDFLOAT);
    write_u16(&mut elf, 40, 52); // e_ehsize
    write_u16(&mut elf, 42, 32); // e_phentsize
    write_u16(&mut elf, 44, 1); // e_phnum
    write_u16(&mut elf, 46, 40); // e_shentsize
    write_u16(&mut elf, 48, shnum);
    write_u16(&mut elf, 50, shstrndx);

    // Program header
    let filesz = claimed_filesz.map_or(u64::try_from(load_content_len).expect("len"), |v| v);
    let filesz_u32 = u32::try_from(filesz.min(u64::from(u32::MAX))).expect("filesz");
    write_u32(&mut elf, phoff, PT_LOAD);
    write_u32(&mut elf, phoff + 4, u32::try_from(load_off).expect("off"));
    write_u32(&mut elf, phoff + 8, paddr); // p_vaddr
    write_u32(&mut elf, phoff + 12, paddr); // p_paddr
    write_u32(&mut elf, phoff + 16, filesz_u32);
    write_u32(&mut elf, phoff + 20, filesz_u32);
    write_u32(&mut elf, phoff + 24, 5); // PF_R|PF_X
    write_u32(&mut elf, phoff + 28, 4); // p_align

    // Load contents: FCB tag (or stand-in) at the start.
    elf[load_off..load_off + 4].copy_from_slice(&load_prefix);

    if with_vector_table {
        // SP in DTCM, reset in ITCM (Thumb bit set).
        write_u32(&mut elf, vector_off, 0x2000_4000);
        write_u32(&mut elf, vector_off + 4, 0x0000_0001);

        let shstr = b"\0.vector_table\0.shstrtab\0";
        elf[shstr_off..shstr_off + shstr.len()].copy_from_slice(shstr);

        // section 0: NULL
        // already zeroed

        // section 1: .vector_table
        let vt_name = 1u32; // offset of ".vector_table" in shstrtab
        write_u32(&mut elf, shoff + 40, vt_name);
        write_u32(&mut elf, shoff + 40 + 4, SHT_PROGBITS);
        write_u32(&mut elf, shoff + 40 + 8, 0); // flags
        write_u32(&mut elf, shoff + 40 + 12, 0x2000_4000); // sh_addr
        write_u32(
            &mut elf,
            shoff + 40 + 16,
            u32::try_from(vector_off).expect("voff"),
        );
        write_u32(&mut elf, shoff + 40 + 20, 8); // sh_size

        // section 2: .shstrtab
        let shstr_name = 15u32; // offset of ".shstrtab"
        debug_assert_eq!(&shstr[shstr_name as usize..], b".shstrtab\0");
        write_u32(&mut elf, shoff + 80, shstr_name);
        write_u32(&mut elf, shoff + 80 + 4, SHT_STRTAB);
        write_u32(
            &mut elf,
            shoff + 80 + 16,
            u32::try_from(shstr_off).expect("shstr"),
        );
        write_u32(
            &mut elf,
            shoff + 80 + 20,
            u32::try_from(shstr.len()).expect("shstr len"),
        );
    }

    elf
}

fn align_up(value: usize, align: usize) -> usize {
    (value + align - 1) & !(align - 1)
}

fn write_u16(buf: &mut [u8], off: usize, value: u16) {
    buf[off..off + 2].copy_from_slice(&value.to_le_bytes());
}

fn write_u32(buf: &mut [u8], off: usize, value: u32) {
    buf[off..off + 4].copy_from_slice(&value.to_le_bytes());
}
