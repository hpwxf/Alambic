//! Restarting into the Teensy's bootloader from the firmware, so the host
//! can flash it without the PROGRAM button (journal/D-029, D-016).
//!
//! This is what Teensyduino's `_reboot_Teensyduino_` does on a Teensy 4
//! (`teensy4/usb.c` in Paul Stoffregen's `cores`): unless the chip is in
//! secure mode, it executes `bkpt #251`, which the separate bootloader chip
//! watches for and answers by taking over. A locked ("secure") Teensy needs
//! a jump into NXP's ROM instead; that path is not implemented here, and
//! [`is_supported`] says so.
//!
//! The only `unsafe` besides `flash_store.rs` (AGENTS.md): reading a fuse
//! register and the breakpoint instruction, which no crate wraps.

#![allow(unsafe_code)]

use teensy4_bsp::ral;

/// The secure-mode bit of `OCOTP_CFG5` (`HW_OCOTP_CFG5 & 0x02` in
/// Teensyduino).
const SECURE_MODE: u32 = 0x02;

/// Whether [`to_loader`] can restart this chip into its bootloader.
pub(crate) fn is_supported() -> bool {
    // SAFETY: the OCOTP block is only read, here, and reading a shadowed fuse
    // value has no side effect; no other code in the firmware touches it.
    let ocotp = unsafe { ral::ocotp::OCOTP::instance() };
    ral::read_reg!(ral::ocotp, ocotp, CFG5) & SECURE_MODE == 0
}

/// Hands the chip to the bootloader, which waits to be flashed. Call it
/// only when [`is_supported`] is true: on a secure chip the breakpoint has
/// no debugger to answer it and faults.
pub(crate) fn to_loader() -> ! {
    cortex_m::interrupt::disable();
    // SAFETY: the breakpoint stops the core for good; the bootloader chip
    // then resets it. Nothing runs after it, so no state can be left broken.
    unsafe { core::arch::asm!("bkpt #251", options(noreturn)) }
}
