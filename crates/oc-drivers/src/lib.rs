//! Peripheral drivers for the Ornament & Crime module.
//!
//! These drivers talk to the DAC8565 and to the OLED controller through
//! `embedded-hal` 1.0 traits only. They therefore know nothing about the
//! i.MX RT registers or about the module's pinout, which has two consequences
//! worth the extra crate:
//!
//! * they contain no `unsafe` code, so the whole crate is
//!   `forbid(unsafe_code)` like [`oc_core`];
//! * they are exercised on the host against recording mock buses, so the exact
//!   bytes that will reach the hardware are asserted in tests.
//!
//! Only the wiring of these drivers to real peripherals lives in
//! `oc-firmware`, which is the one crate that cannot be tested on the host.

#![no_std]
#![forbid(unsafe_code)]
#![warn(missing_docs)]

// The unit tests build recording mocks that are far more readable with `Vec`
// than with fixed-size buffers.
#[cfg(test)]
extern crate std;

pub mod dac8565;
pub mod panel;
pub mod shared_bus;
pub mod ssd130x;
pub mod triggers;

pub use dac8565::{Dac8565, Dac8565Error, UpdateMode};
pub use panel::{EncoderPins, Panel};
pub use shared_bus::SharedBus;
pub use ssd130x::{Controller, Ssd130x, Ssd130xError};
pub use triggers::{Polarity, Triggers};
