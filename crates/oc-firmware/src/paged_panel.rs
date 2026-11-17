//! The OLED driver as the engine's [`Display`], sent one page at a time: a
//! whole frame costs about as much as a tick, so the engine sends one page
//! per tick after the one that drew it (journal/D-023, option C+) and times
//! each as [`oc_core::profile::Stage::Page`].

use embedded_hal::digital::OutputPin;
use embedded_hal::spi::SpiBus;

use oc_core::framebuffer::{FrameBuffer, PAGES};
use oc_core::platform::Display;
use oc_drivers::ssd130x::Ssd130x;

/// The panel, seen page by page.
pub(crate) struct PagedPanel<SPI, CS, DC, RST> {
    panel: Ssd130x<SPI, CS, DC, RST>,
}

impl<SPI, CS, DC, RST> PagedPanel<SPI, CS, DC, RST> {
    /// Wraps an initialised panel.
    pub(crate) const fn new(panel: Ssd130x<SPI, CS, DC, RST>) -> Self {
        Self { panel }
    }
}

impl<SPI, CS, DC, RST> Display for PagedPanel<SPI, CS, DC, RST>
where
    SPI: SpiBus<u8>,
    CS: OutputPin,
    DC: OutputPin,
    RST: OutputPin,
{
    fn frame_mut(&mut self) -> &mut FrameBuffer {
        self.panel.frame_mut()
    }

    /// The whole frame, for the boot splash.
    fn present(&mut self) {
        for page in 0..PAGES {
            self.present_page(page);
        }
    }

    fn page_count(&self) -> usize {
        PAGES
    }

    fn present_page(&mut self, page: usize) {
        // A failing page leaves the module running: a dead panel must not
        // stop it, and the next frame retries anyway.
        let _ = self.panel.flush_page(page);
    }

    fn set_contrast(&mut self, contrast: u8) {
        self.panel.set_contrast(contrast);
    }
}

impl<SPI, CS, DC, RST> core::fmt::Debug for PagedPanel<SPI, CS, DC, RST> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("PagedPanel").finish_non_exhaustive()
    }
}
