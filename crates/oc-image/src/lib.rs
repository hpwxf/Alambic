//! Firmware images for the Teensy 4.0: the pre-flash gate and what it lets
//! through.
//!
//! [`validate`] is the guard rail every path to the device runs first
//! (journal/D-016); [`flash`] turns an image that passed it into the bytes
//! the flash must hold, which is what a loader writes. Both are pure
//! functions over bytes with no operating-system dependency, so `cargo xtask
//! flash` and the browser flasher (`oc-web`, journal/D-031) share them.

pub mod flash;
pub mod validate;
