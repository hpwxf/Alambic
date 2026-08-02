//! Library surface of the firmware build automation.
//!
//! The CLI binary is the user-facing entry point; this crate exists so that
//! integration tests can exercise the pre-flash image gate and the
//! `llvm-size` layout checklist without spawning the full command line.
//! Keeping validation pure over bytes (or captured tool text) makes the
//! rejection messages straightforward to assert on.

pub mod size_check;
pub mod validate;

/// Compilation target of the firmware binary.
pub const FIRMWARE_TARGET: &str = "thumbv7em-none-eabihf";

/// Cargo package name of the firmware binary.
pub const FIRMWARE_PACKAGE: &str = "oc-firmware";
