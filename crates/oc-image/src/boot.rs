//! The boot header at the start of the flash, as the i.MX RT1062 boot ROM
//! reads it.
//!
//! An ELF is checked through its own structure (`validate`); a HEX file only
//! carries addresses and bytes, so what can still be checked is the contract
//! with the boot ROM, which every Teensy 4.0 firmware honours: the `FlexSPI`
//! Configuration Block at the very start, the Image Vector Table 4 KiB in,
//! and the boot data the table points to. This firmware's `imxrt-rt` and
//! Teensyduino (`teensy4/bootdata.c`) lay them out the same way
//! (journal/D-031).

use crate::validate::{
    FCB_TAG, FLEXSPI_BASE, Rejection, TEENSY40_FLASH_CAPACITY, address_plausible, read_u32_le,
};

/// Offset of the Image Vector Table in the flash.
pub const IVT_OFFSET: usize = 0x1000;

/// The bus address of the Image Vector Table.
const IVT_ADDRESS: u32 = FLEXSPI_BASE + 0x1000;

/// Bytes of the Image Vector Table.
const IVT_LEN: usize = 32;

/// First byte of an Image Vector Table header.
const IVT_TAG: u8 = 0xD1;

/// Boot data: start, size, plugin flag.
const BOOT_DATA_LEN: usize = 12;

/// What the boot header says about the image.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BootHeader {
    /// Where the boot ROM starts the image: the vector table for
    /// `imxrt-rt`, the reset handler for Teensyduino.
    pub entry: u32,
    /// Bytes of flash the boot data claims for the image.
    pub image_size: u32,
}

/// Checks the boot header of `flash`, the flash contents from its first
/// byte, and returns what it says.
///
/// # Errors
///
/// Every problem found: a missing or invalid `FlexSPI` Configuration Block,
/// Image Vector Table or boot data. The boot ROM would not start such an
/// image.
pub fn check_boot_header(flash: &[u8]) -> Result<BootHeader, Vec<Rejection>> {
    let mut rejections = Vec::new();
    if flash.get(..FCB_TAG.len()) != Some(&FCB_TAG[..]) {
        rejections.push(Rejection::new(format!(
            "`FlexSPI` Configuration Block missing at {FLEXSPI_BASE:#010x}: the image does not start with the tag FCFB"
        )));
    }
    let Some(ivt) = flash.get(IVT_OFFSET..IVT_OFFSET + IVT_LEN) else {
        rejections.push(Rejection::new(format!(
            "the image ends before its Image Vector Table at {IVT_ADDRESS:#010x}"
        )));
        return Err(rejections);
    };

    let length = u16::from_be_bytes([ivt[1], ivt[2]]);
    let version = ivt[3];
    if ivt[0] != IVT_TAG || usize::from(length) != IVT_LEN || version & 0xF0 != 0x40 {
        rejections.push(Rejection::new(format!(
            "no Image Vector Table at {IVT_ADDRESS:#010x}: expected tag d1, length 0x0020 and version 4x, found {:02x} {length:04x} {version:02x}",
            ivt[0]
        )));
        return Err(rejections);
    }

    let entry = read_u32_le(ivt, 4);
    let boot_data = read_u32_le(ivt, 16);
    let own = read_u32_le(ivt, 20);
    if own != IVT_ADDRESS {
        rejections.push(Rejection::new(format!(
            "the Image Vector Table says it lives at {own:#010x}, not at {IVT_ADDRESS:#010x}"
        )));
    }
    if !address_plausible(entry & !1) {
        rejections.push(Rejection::new(format!(
            "the image's entry point {entry:#010x} is outside every memory of the i.MX RT1062"
        )));
    }

    let image_size = match boot_data_at(flash, boot_data) {
        Ok((start, size)) => {
            if start != FLEXSPI_BASE {
                rejections.push(Rejection::new(format!(
                    "the boot data says the image starts at {start:#010x}, not at {FLEXSPI_BASE:#010x}"
                )));
            }
            if u64::from(size) > TEENSY40_FLASH_CAPACITY {
                rejections.push(Rejection::new(format!(
                    "the boot data claims {size} bytes, more than the {TEENSY40_FLASH_CAPACITY} bytes of application flash"
                )));
            }
            size
        }
        Err(rejection) => {
            rejections.push(rejection);
            0
        }
    };

    if rejections.is_empty() {
        Ok(BootHeader { entry, image_size })
    } else {
        Err(rejections)
    }
}

/// The start and size the boot data at bus address `address` hold.
fn boot_data_at(flash: &[u8], address: u32) -> Result<(u32, u32), Rejection> {
    let data = address
        .checked_sub(FLEXSPI_BASE)
        .map(|offset| offset as usize)
        .and_then(|offset| flash.get(offset..offset.checked_add(BOOT_DATA_LEN)?));
    let Some(data) = data else {
        return Err(Rejection::new(format!(
            "the Image Vector Table points to boot data at {address:#010x}, outside the image"
        )));
    };
    Ok((read_u32_le(data, 0), read_u32_le(data, 4)))
}
