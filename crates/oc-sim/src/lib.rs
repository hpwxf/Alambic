//! Native (host) backend for [`oc_core`].
//!
//! Implements the platform traits against plain in-memory state and a
//! deterministic virtual clock, so that the exact firmware behaviour can be
//! exercised, replayed and asserted on without any hardware.

/// Identification string of the simulator, including the core version it runs.
#[must_use]
pub fn banner() -> &'static str {
    oc_core::BANNER
}
