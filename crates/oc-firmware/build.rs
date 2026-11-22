//! Sets `OC_BUILD_ID`, the build identifier shown under the version on the
//! splash screen (journal/D-025), and generates the linker script `t4link.x`.
//!
//! The runtime used to be configured by `teensy4-bsp`'s `rt` feature, whose
//! build script fixes the `FlexRAM` bank split. Configuring it here keeps the
//! same layout but makes the split ours: giving ITCM more banks when code runs
//! short is a change to `FLEXRAM_BANKS` (journal/D-038, option B), checked by
//! `cargo xtask size`, which reads the split back from the image.

use imxrt_rt::{Family, FlexRamBanks, Memory, RuntimeBuilder};

/// Application flash of the Teensy 4.0: 2 MiB less the 64 KiB reserved at the
/// top (settings, EEPROM emulation, restore program; see `MEMORY.md`).
const FLASH_SIZE: usize = 1984 * 1024;

/// The 16 banks of 32 KiB: 192 KiB of ITCM for code, 320 KiB of DTCM for the
/// stack, the vectors, `.data` and `.bss` (the DMA ring among them, which must
/// stay in DTCM, journal/D-033). The dedicated OCRAM is not part of it.
const FLEXRAM_BANKS: FlexRamBanks = FlexRamBanks {
    ocram: 0,
    itcm: 6,
    dtcm: 10,
};

fn main() {
    oc_buildinfo::emit();

    // `.text` in ITCM is load-bearing, not only fast: the settings are written
    // to flash through the boot ROM while the firmware runs, which is safe only
    // because no code executes from flash meanwhile (journal/D-028).
    RuntimeBuilder::from_flexspi(Family::Imxrt1060, FLASH_SIZE)
        .flexram_banks(FLEXRAM_BANKS)
        .text(Memory::Itcm)
        .rodata(Memory::Ocram)
        .data(Memory::Dtcm)
        .bss(Memory::Dtcm)
        .vectors(Memory::Dtcm)
        .stack(Memory::Dtcm)
        .stack_size(16 * 1024)
        .stack_size_env_override("TEENSY4_STACK_SIZE")
        .heap(Memory::Ocram)
        .heap_size(16 * 1024)
        .heap_size_env_override("TEENSY4_HEAP_SIZE")
        .uninit(Memory::Ocram)
        .linker_script_name("t4link.x")
        .build()
        .expect("cannot generate the linker script t4link.x");
}
