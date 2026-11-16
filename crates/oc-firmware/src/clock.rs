//! Microsecond time source built on a free-running general purpose timer.
//!
//! GPT1 is clocked from the 24 MHz crystal and divided by 24, so one timer
//! count is exactly one microsecond, with no rounding anywhere. The counter is
//! 32 bits and therefore wraps every 71 minutes and 35 seconds; the wrap is
//! detected on each reading and folded into a 64-bit accumulator, which lasts
//! longer than the module will.
//!
//! Profiling (journal/D-029) reads the core's cycle counter instead, the
//! DWT's `CYCCNT`: 600 counts a microsecond, read in a few cycles.

use core::cell::Cell;

use cortex_m::peripheral::{DCB, DWT};

use teensy4_bsp::hal::gpt::{ClockSource, Gpt, Mode};

use oc_core::platform::Clock;

use crate::delay::CORE_CLOCK_HZ;

/// Divider applied to the 24 MHz crystal to obtain a 1 MHz counter.
const DIVIDER_24MHZ: u32 = 24;

/// A monotonic microsecond clock.
///
/// [`Clock::now_micros`] takes `&self`, so the wrap bookkeeping lives behind
/// [`Cell`]s. That is sound here because the firmware is single-threaded and
/// never reads the clock from an interrupt.
pub(crate) struct SystemClock {
    gpt: Gpt,
    last_count: Cell<u32>,
    elapsed: Cell<u64>,
}

impl SystemClock {
    /// Configures a GPT as a free-running microsecond counter and starts it,
    /// and starts the core's cycle counter for the profile.
    pub(crate) fn new(mut gpt: Gpt, dcb: &mut DCB, dwt: &mut DWT) -> Self {
        dcb.enable_trace();
        DWT::unlock();
        dwt.enable_cycle_counter();

        gpt.disable();
        gpt.set_clock_source(ClockSource::HighFrequencyReferenceClock);
        gpt.set_divider_24mhz(DIVIDER_24MHZ);
        gpt.set_mode(Mode::FreeRunning);
        gpt.set_reset_on_enable(true);
        gpt.enable();

        Self {
            gpt,
            last_count: Cell::new(0),
            elapsed: Cell::new(0),
        }
    }
}

impl Clock for SystemClock {
    fn now_micros(&self) -> u64 {
        let count = self.gpt.count();
        // `wrapping_sub` is exactly right at the 32-bit rollover: the
        // difference since the previous reading stays correct as long as
        // readings are less than 71 minutes apart, which a 1 kHz loop
        // guarantees.
        let advance = count.wrapping_sub(self.last_count.get());
        self.last_count.set(count);

        let elapsed = self.elapsed.get().wrapping_add(u64::from(advance));
        self.elapsed.set(elapsed);
        elapsed
    }

    fn fine_count(&self) -> u32 {
        DWT::cycle_count()
    }

    fn fine_per_micro(&self) -> u32 {
        CORE_CLOCK_HZ / 1_000_000
    }
}

impl core::fmt::Debug for SystemClock {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("SystemClock")
            .field("micros", &self.elapsed.get())
            .finish_non_exhaustive()
    }
}
