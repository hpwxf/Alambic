//! A busy-wait delay driven by the core clock.
//!
//! `imxrt-hal` 0.6 does not ship a `DelayNs` implementation, and the only place
//! the firmware needs one is the OLED reset pulse at boot — a few milliseconds,
//! once. A cycle-counting spin is therefore the right tool: no timer is
//! consumed, no interrupt is involved, and the panel drivers stay expressed in
//! terms of the standard `embedded-hal` trait.
//!
//! This must not be used inside the 1 kHz loop; the loop paces itself against
//! the microsecond clock instead.

use embedded_hal::delay::DelayNs;

/// Core clock of the i.MX RT1062 as configured by the board support package.
pub(crate) const CORE_CLOCK_HZ: u32 = 600_000_000;

/// Core clock cycles per microsecond.
const CYCLES_PER_MICRO: u32 = CORE_CLOCK_HZ / 1_000_000;

/// A delay that spins for a number of core clock cycles.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct CycleDelay;

impl DelayNs for CycleDelay {
    fn delay_ns(&mut self, ns: u32) {
        // Round up so that a requested delay is never shorter than asked, which
        // is what a reset pulse specification means.
        let micros = ns.div_ceil(1_000);
        self.delay_us(micros);
    }

    fn delay_us(&mut self, us: u32) {
        // Split the wait so that the cycle count cannot overflow `u32`: at
        // 600 MHz a single call can only cover about seven seconds.
        let mut remaining = us;
        while remaining > 0 {
            let chunk = remaining.min(1_000);
            remaining -= chunk;
            cortex_m::asm::delay(chunk.saturating_mul(CYCLES_PER_MICRO));
        }
    }
}
