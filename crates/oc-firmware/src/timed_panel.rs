//! The OLED driver, sending its frame one page at a time and timing each
//! page on the core's cycle counter (journal/D-029): the cost the faster
//! loop of D-023 must fit between two deadlines.
//!
//! The engine only sees a [`Display`]; the loop collects the page times
//! after each tick ([`TimedPanel::take_page_times`]) into the profile's
//! [`oc_core::profile::Stage::Page`].

use cortex_m::peripheral::DWT;
use embedded_hal::digital::OutputPin;
use embedded_hal::spi::SpiBus;

use oc_core::framebuffer::{FrameBuffer, PAGES};
use oc_core::platform::Display;
use oc_drivers::ssd130x::Ssd130x;

/// The panel, and the time of each page of the last frame sent.
pub(crate) struct TimedPanel<SPI, CS, DC, RST> {
    panel: Ssd130x<SPI, CS, DC, RST>,
    page_cycles: [u32; PAGES],
    fresh: bool,
}

impl<SPI, CS, DC, RST> TimedPanel<SPI, CS, DC, RST>
where
    SPI: SpiBus<u8>,
    CS: OutputPin,
    DC: OutputPin,
    RST: OutputPin,
{
    /// Wraps an initialised panel.
    pub(crate) const fn new(panel: Ssd130x<SPI, CS, DC, RST>) -> Self {
        Self {
            panel,
            page_cycles: [0; PAGES],
            fresh: false,
        }
    }

    /// The cycles each page of the last frame took, once per frame sent.
    pub(crate) fn take_page_times(&mut self) -> Option<[u32; PAGES]> {
        core::mem::take(&mut self.fresh).then_some(self.page_cycles)
    }
}

impl<SPI, CS, DC, RST> Display for TimedPanel<SPI, CS, DC, RST>
where
    SPI: SpiBus<u8>,
    CS: OutputPin,
    DC: OutputPin,
    RST: OutputPin,
{
    fn frame_mut(&mut self) -> &mut FrameBuffer {
        self.panel.frame_mut()
    }

    fn present(&mut self) {
        for (page, cycles) in self.page_cycles.iter_mut().enumerate() {
            let start = DWT::cycle_count();
            // A failing page leaves the others to try: a dead panel must not
            // stop the module, and the next frame retries anyway.
            let _ = self.panel.flush_page(page);
            *cycles = DWT::cycle_count().wrapping_sub(start);
        }
        self.fresh = true;
    }

    fn set_contrast(&mut self, contrast: u8) {
        self.panel.set_contrast(contrast);
    }
}

impl<SPI, CS, DC, RST> core::fmt::Debug for TimedPanel<SPI, CS, DC, RST> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("TimedPanel")
            .field("page_cycles", &self.page_cycles)
            .finish_non_exhaustive()
    }
}
