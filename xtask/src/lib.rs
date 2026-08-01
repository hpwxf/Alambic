//! Library surface of the firmware build automation.
//!
//! The CLI binary is the user-facing entry point; this crate exists so that
//! integration tests can exercise the pre-flash image gate without spawning
//! the full command line. Keeping validation pure over bytes also makes the
//! rejection messages straightforward to assert on.

pub mod validate;

/// Compilation target of the firmware binary.
pub const FIRMWARE_TARGET: &str = "thumbv7em-none-eabihf";

/// Cargo package name of the firmware binary.
pub const FIRMWARE_PACKAGE: &str = "oc-firmware";
