//! The settings flash: four 4 KiB sectors of the Teensy 4.0's own NOR flash,
//! erased and programmed through the i.MX RT1062 boot ROM (journal/D-028).
//!
//! No crate drives the `FlexSPI` NOR flash for writing, so this module calls
//! the ROM's own driver, the route NXP documents (`fsl_romapi`, `MCUXpresso`
//! SDK). Facts it relies on, all checked against NXP's sources and the BSP:
//!
//! * the ROM API tree pointer lives at `0x0020_001C`; its fifth word points
//!   at the `FlexSPI` NOR driver, whose functions are `init`, `program` (one
//!   page), `erase_all`, `erase`, `read` and `clear_cache`, in that order
//!   after a version word (`fsl_romapi.c`); the RT1062 ROM has all of them
//!   (`FSL_ROM_FLEXSPINOR_API_HAS_FEATURE_*` in `fsl_romapi.h`);
//! * every call takes the 512-byte serial NOR configuration block. The one
//!   this image boots with (`teensy4_fcb::SERIAL_NOR_CONFIGURATION_BLOCK`,
//!   W25Q16JV) carries the write-enable, sector-erase (`0x20`), page-program
//!   (`0x02`) and read-status sequences, 256-byte pages and 4 KiB sectors;
//! * addresses are offsets from the `FlexSPI` base, not bus addresses;
//! * nothing may execute from flash while it is busy: `imxrt-rt` runs `.text`
//!   from ITCM and keeps data in DTCM and OCRAM, and every call runs with
//!   interrupts masked. Reads go through the ROM's `read` too, never through
//!   the memory-mapped window, so no cache or AHB buffer can serve stale
//!   bytes after a write; `clear_cache` still follows every change.
//!
//! Erasing a sector blocks for tens of milliseconds. It only happens on an
//! explicit save from the settings screen.

// The one module of this crate that needs `unsafe`: calls into the boot ROM
// through function pointers it publishes. Every block states its invariant.
#![allow(unsafe_code)]

use core::ffi::c_void;
use core::future::{Future, ready};

use embedded_storage_async::nor_flash::{
    ErrorType, NorFlash, NorFlashError, NorFlashErrorKind, ReadNorFlash,
};
use teensy4_fcb::{ConfigurationBlock, SERIAL_NOR_CONFIGURATION_BLOCK};

use oc_core::settings::{FLASH_BYTES, SECTOR_BYTES};

use crate::board::SETTINGS_FLASH_OFFSET;

/// Where the boot ROM publishes the pointer to its API tree.
const ROM_API_TREE: usize = 0x0020_001C;

/// `FlexSPI` instance of the boot flash.
const FLEXSPI_INSTANCE: u32 = 0;

/// `kStatus_Success`.
const STATUS_SUCCESS: i32 = 0;

/// Page size of the W25Q16JV, the unit of one ROM `program` call.
const PAGE_BYTES: usize = 256;

/// One page as the ROM wants it: whole words.
const PAGE_WORDS: usize = PAGE_BYTES / 4;

/// `flexspi_nor_driver_interface_t`, up to the last function used here.
#[repr(C)]
struct NorDriver {
    version: u32,
    init: unsafe extern "C" fn(u32, *mut c_void) -> i32,
    program: unsafe extern "C" fn(u32, *mut c_void, u32, *const u32) -> i32,
    erase_all: unsafe extern "C" fn(u32, *mut c_void) -> i32,
    erase: unsafe extern "C" fn(u32, *mut c_void, u32, u32) -> i32,
    read: unsafe extern "C" fn(u32, *mut c_void, *mut u32, u32, u32) -> i32,
    clear_cache: unsafe extern "C" fn(u32),
}

/// `bootloader_api_entry_t`, up to the `FlexSPI` NOR driver.
#[repr(C)]
struct RomApiTree {
    version: u32,
    copyright: *const u8,
    run_bootloader: *const c_void,
    reserved: *const u32,
    flexspi_nor: *const NorDriver,
}

/// The configuration block, word-aligned as the ROM reads it (the BSP's type
/// is packed).
#[repr(C, align(4))]
struct AlignedConfig(ConfigurationBlock);

const _: () = assert!(
    size_of::<ConfigurationBlock>() == 512,
    "the ROM expects a 512-byte flexspi_nor_config_t"
);
const _: () = assert!(
    SECTOR_BYTES as usize == 4_096 && FLASH_BYTES.is_multiple_of(SECTOR_BYTES),
    "the store's sectors must be the W25Q16JV's 4 KiB sectors"
);

/// What the settings flash can report.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FlashError {
    /// The access falls outside the four settings sectors.
    OutOfBounds,
    /// The access is not aligned to the operation's granularity.
    NotAligned,
    /// The ROM published no `FlexSPI` NOR driver.
    NoDriver,
    /// A ROM call returned this status.
    Rom(i32),
}

impl NorFlashError for FlashError {
    fn kind(&self) -> NorFlashErrorKind {
        match self {
            Self::OutOfBounds => NorFlashErrorKind::OutOfBounds,
            Self::NotAligned => NorFlashErrorKind::NotAligned,
            Self::NoDriver | Self::Rom(_) => NorFlashErrorKind::Other,
        }
    }
}

/// The four settings sectors, seen as a [`NorFlash`] of
/// [`FLASH_BYTES`] starting at offset 0.
pub(crate) struct SettingsFlash {
    driver: &'static NorDriver,
    config: AlignedConfig,
    page: [u32; PAGE_WORDS],
}

impl core::fmt::Debug for SettingsFlash {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("SettingsFlash")
            .field("rom_driver_version", &self.driver.version)
            .finish_non_exhaustive()
    }
}

impl SettingsFlash {
    /// Finds the ROM's NOR driver and initialises it with this image's
    /// configuration block.
    ///
    /// # Errors
    ///
    /// Fails if the ROM publishes no driver or refuses the configuration.
    pub(crate) fn new() -> Result<Self, FlashError> {
        // SAFETY: `ROM_API_TREE` is the documented, always-mapped ROM location
        // of the API tree pointer on the i.MX RT1062 (`g_bootloaderTree`,
        // `fsl_romapi.c`); reading one aligned word from ROM has no side
        // effect.
        let tree = unsafe { core::ptr::read_volatile(ROM_API_TREE as *const *const RomApiTree) };
        // SAFETY: a non-null tree points into ROM, valid for the life of the
        // program; the field read is the documented driver pointer, which
        // `as_ref` checks for null before it is used.
        let driver = unsafe { tree.as_ref().and_then(|tree| tree.flexspi_nor.as_ref()) }
            .ok_or(FlashError::NoDriver)?;
        let mut flash = Self {
            driver,
            config: AlignedConfig(SERIAL_NOR_CONFIGURATION_BLOCK),
            page: [0; PAGE_WORDS],
        };
        let init = flash.driver.init;
        // SAFETY: `init` is the ROM driver's own entry point; it gets the
        // instance of the boot flash and a valid, aligned, exclusively
        // borrowed configuration block (see `rom_call`).
        flash.rom_call(|config| unsafe { init(FLEXSPI_INSTANCE, config) })?;
        Ok(flash)
    }

    /// Runs one ROM call with interrupts masked, then clears the `FlexSPI`
    /// read cache so nothing stale survives a change.
    ///
    /// The closure receives the configuration block; it is borrowed
    /// exclusively for the call, which is what the ROM's `*mut` asks for.
    fn rom_call(&mut self, call: impl FnOnce(*mut c_void) -> i32) -> Result<(), FlashError> {
        let config = (&raw mut self.config).cast::<c_void>();
        let clear_cache = self.driver.clear_cache;
        let status = cortex_m::interrupt::free(|_| {
            let status = call(config);
            // SAFETY: the ROM's own cache maintenance for the instance in use;
            // it takes no pointer.
            unsafe { clear_cache(FLEXSPI_INSTANCE) };
            status
        });
        if status == STATUS_SUCCESS {
            Ok(())
        } else {
            Err(FlashError::Rom(status))
        }
    }

    /// The flash offset of a store address, if `length` bytes from it stay
    /// inside the settings sectors.
    fn locate(offset: u32, length: usize) -> Result<u32, FlashError> {
        let length = u32::try_from(length).map_err(|_| FlashError::OutOfBounds)?;
        let end = offset.checked_add(length).ok_or(FlashError::OutOfBounds)?;
        if end > FLASH_BYTES {
            return Err(FlashError::OutOfBounds);
        }
        Ok(SETTINGS_FLASH_OFFSET + offset)
    }

    fn read_now(&mut self, offset: u32, bytes: &mut [u8]) -> Result<(), FlashError> {
        let mut address = Self::locate(offset, bytes.len())?;
        let read = self.driver.read;
        for chunk in bytes.chunks_mut(PAGE_BYTES) {
            let length = u32::try_from(chunk.len()).map_err(|_| FlashError::OutOfBounds)?;
            let destination = self.page.as_mut_ptr();
            // SAFETY: `read` is the ROM driver's entry point; `destination` is
            // the word-aligned page buffer, valid for `PAGE_BYTES` and so for
            // `length` (at most one chunk), and `address..address + length`
            // lies in the settings sectors (`locate`).
            self.rom_call(|config| unsafe {
                read(FLEXSPI_INSTANCE, config, destination, address, length)
            })?;
            for (byte, word_byte) in chunk
                .iter_mut()
                .zip(self.page.iter().flat_map(|word| word.to_le_bytes()))
            {
                *byte = word_byte;
            }
            address += length;
        }
        Ok(())
    }

    fn erase_now(&mut self, from: u32, to: u32) -> Result<(), FlashError> {
        if !from.is_multiple_of(SECTOR_BYTES) || !to.is_multiple_of(SECTOR_BYTES) || from > to {
            return Err(FlashError::NotAligned);
        }
        let length = usize::try_from(to - from).map_err(|_| FlashError::OutOfBounds)?;
        let start = Self::locate(from, length)?;
        let erase = self.driver.erase;
        let mut sector = start;
        while sector < start + (to - from) {
            // SAFETY: `erase` is the ROM driver's entry point; `sector` is a
            // sector-aligned offset inside the settings sectors (`locate`),
            // never the image nor the restore program's sector.
            self.rom_call(|config| unsafe {
                erase(FLEXSPI_INSTANCE, config, sector, SECTOR_BYTES)
            })?;
            sector += SECTOR_BYTES;
        }
        Ok(())
    }

    /// Programs `bytes` at `offset`, one page at a time. The rest of each
    /// page is sent as `0xFF`, which leaves NOR cells as they are, so earlier
    /// data in the same page survives.
    fn write_now(&mut self, offset: u32, bytes: &[u8]) -> Result<(), FlashError> {
        let word = u32::try_from(<Self as NorFlash>::WRITE_SIZE).unwrap_or(4);
        if !offset.is_multiple_of(word)
            || !bytes.len().is_multiple_of(<Self as NorFlash>::WRITE_SIZE)
        {
            return Err(FlashError::NotAligned);
        }
        let address = Self::locate(offset, bytes.len())?;
        let program = self.driver.program;
        let page_bytes = u32::try_from(PAGE_BYTES).unwrap_or(256);
        let mut written = 0usize;
        while written < bytes.len() {
            let at = address + u32::try_from(written).map_err(|_| FlashError::OutOfBounds)?;
            let page_start = at - at % page_bytes;
            let within = usize::try_from(at - page_start).unwrap_or(0);
            let count = (PAGE_BYTES - within).min(bytes.len() - written);

            let mut page = [0xFF_u8; PAGE_BYTES];
            page[within..within + count].copy_from_slice(&bytes[written..written + count]);
            for (word, quad) in self.page.iter_mut().zip(page.as_chunks::<4>().0) {
                *word = u32::from_le_bytes(*quad);
            }
            let source = self.page.as_ptr();
            // SAFETY: `program` is the ROM driver's entry point; `page_start`
            // is a page-aligned offset inside the settings sectors (`locate`
            // bounded the whole write), and `source` is the word-aligned page
            // buffer of exactly one page, which the ROM only reads.
            self.rom_call(|config| unsafe {
                program(FLEXSPI_INSTANCE, config, page_start, source)
            })?;
            written += count;
        }
        Ok(())
    }
}

impl ErrorType for SettingsFlash {
    type Error = FlashError;
}

impl ReadNorFlash for SettingsFlash {
    const READ_SIZE: usize = 4;

    fn read(
        &mut self,
        offset: u32,
        bytes: &mut [u8],
    ) -> impl Future<Output = Result<(), Self::Error>> {
        ready(self.read_now(offset, bytes))
    }

    fn capacity(&self) -> usize {
        FLASH_BYTES as usize
    }
}

impl NorFlash for SettingsFlash {
    const WRITE_SIZE: usize = 4;
    const ERASE_SIZE: usize = SECTOR_BYTES as usize;

    fn erase(&mut self, from: u32, to: u32) -> impl Future<Output = Result<(), Self::Error>> {
        ready(self.erase_now(from, to))
    }

    fn write(
        &mut self,
        offset: u32,
        bytes: &[u8],
    ) -> impl Future<Output = Result<(), Self::Error>> {
        ready(self.write_now(offset, bytes))
    }
}
