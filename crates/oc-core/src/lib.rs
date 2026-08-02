//! Hardware-independent core of the Ornament & Crime Rust firmware.
//!
//! This crate contains **all** behaviour of the module: signal conversion,
//! control decoding, application state and screen rendering. It knows nothing
//! about registers, operating systems or allocation, and is driven entirely
//! through the traits in [`platform`].
//!
//! Three backends implement those traits and share this exact code:
//!
//! * `oc-firmware` — the Teensy 4.0 binary;
//! * `oc-sim` — a native simulator with a terminal user interface;
//! * `oc-vcv-ffi` — a static library exposing a C ABI to a VCV Rack 2 module.
//!
//! Keeping the behaviour here is what makes the simulator a meaningful proxy
//! for the hardware: only register access differs between the backends.
//!
//! # Example
//!
//! ```
//! use oc_core::platform::{CvChannel, TriggerChannel};
//! use oc_core::testing::mock_engine;
//!
//! let mut engine = mock_engine(50);
//! let (analog_in, _, digital_in, _, _) = engine.parts_mut();
//! analog_in.patch(CvChannel::One, 2_500);
//! digital_in.set(TriggerChannel::One, true);
//!
//! let report = engine.tick();
//! assert_eq!(report.tick_count, 1);
//! ```

#![no_std]
#![forbid(unsafe_code)]
#![warn(missing_docs)]

pub mod app;
pub mod calibration;
pub mod debounce;
pub mod encoder;
pub mod engine;
pub mod fmt;
pub mod framebuffer;
pub mod platform;
pub mod signal;
pub mod splash;
pub mod testing;

pub use app::{DiagnosticApp, InputSnapshot, OutputMode, TickContext};
pub use engine::{Engine, TickReport};
pub use framebuffer::FrameBuffer;
pub use platform::MilliVolts;
pub use splash::SplashScreen;

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
