//! Hardware-independent core of the Ornament & Crime Rust firmware.
//!
//! This crate contains **all** behaviour of the module: signal conversion,
//! control decoding, application state and screen rendering. It knows nothing
//! about registers, operating systems or allocation, and is driven entirely
//! through the platform traits defined in the `platform` module.
//!
//! Three backends implement those traits and share this exact code:
//!
//! * `oc-firmware` — the Teensy 4.0 binary;
//! * `oc-sim` — a native simulator with a terminal user interface;
//! * `oc-vcv-ffi` — a static library exposing a C ABI to a VCV Rack 2 module.
//!
//! Keeping the behaviour here is what makes the simulator a meaningful proxy
//! for the hardware: only register access differs between the backends.

#![no_std]
#![forbid(unsafe_code)]
#![warn(missing_docs)]

/// Human-readable firmware identification, emitted on boot and shown on screen.
pub const BANNER: &str = concat!("O&C Rust v", env!("CARGO_PKG_VERSION"));

#[cfg(test)]
mod tests {
    use super::BANNER;

    #[test]
    fn banner_is_identifiable() {
        assert!(BANNER.starts_with("O&C Rust v"));
    }
}
