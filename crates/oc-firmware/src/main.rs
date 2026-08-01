//! Ornament & Crime firmware for the Teensy 4.0 (NXP i.MX RT1062).
//!
//! At this stage the binary only proves the cross-compilation chain: it boots,
//! emits the version banner on LPUART2 and blinks the on-board LED. All module
//! behaviour lives in [`oc_core`] and will be wired in through the platform
//! traits by the board support code.
//!
//! # Unsafe code
//!
//! This crate cannot be `forbid(unsafe_code)`, because register access and the
//! reset vector require it. Every `unsafe` block must carry a `# Safety`
//! comment and stay confined to the hardware backends.

#![no_std]
#![no_main]
#![warn(missing_docs)]

use embedded_io::Write as _;
use teensy4_bsp as bsp;
use teensy4_panic as _;

use bsp::board;

/// Baud rate of the diagnostic console on LPUART2 (Teensy pins 14 and 15).
const CONSOLE_BAUD: u32 = 115_200;

/// Core clock of the i.MX RT1062 as configured by the board support package.
const CORE_CLOCK_HZ: u32 = 600_000_000;

/// Heartbeat half-period, in core clock cycles.
const HEARTBEAT_CYCLES: u32 = CORE_CLOCK_HZ / 2;

#[bsp::rt::entry]
fn main() -> ! {
    let board::Resources {
        pins,
        mut gpio2,
        lpuart2,
        ..
    } = board::t40(board::instances());

    let led = board::led(&mut gpio2, pins.p13);
    let mut console = board::lpuart(lpuart2, pins.p14, pins.p15, CONSOLE_BAUD);

    // Diagnostic output must never be able to stall the main loop, so write
    // failures are deliberately ignored.
    let _ = writeln!(console, "{}\r", oc_core::BANNER);
    let _ = writeln!(console, "boot ok\r");

    loop {
        led.toggle();
        cortex_m::asm::delay(HEARTBEAT_CYCLES);
    }
}
