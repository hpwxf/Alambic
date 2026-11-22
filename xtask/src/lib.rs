//! Library surface of the firmware build automation.
//!
//! The CLI binary is the user-facing entry point; this crate exists so that
//! integration tests can exercise the pre-flash image gate, the
//! `llvm-size` layout checklist and the stack estimate over `llvm-objdump`
//! text without spawning the full command line.
//! Keeping validation pure over bytes (or captured tool text) makes the
//! rejection messages straightforward to assert on.

pub mod size_check;
pub mod stack_check;

// The gate moved to `oc-image` so the browser flasher runs the very same
// checks (journal/D-031); `xtask::validate` stays its path for the CLI and
// the tests.
pub use oc_image::validate;

/// Compilation target of the firmware binary.
pub const FIRMWARE_TARGET: &str = "thumbv7em-none-eabihf";

/// Cargo package name of the firmware binary.
pub const FIRMWARE_PACKAGE: &str = "oc-firmware";
