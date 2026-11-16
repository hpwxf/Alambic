//! A stress run of the settings flash, built only with the `settings-stress`
//! feature and never in the firmware a module normally runs (`TESTING.md`
//! 9.5c).
//!
//! At power-up, before the first tick, it saves the settings [`SAVES`] times
//! in a row, alternating between the stored settings and the same with the
//! contrast one step off, and reads each save back. That crosses many sector
//! boundaries, so it exercises on the real flash what the host tests only
//! show on a simulated one: sectors filling, being reclaimed and erased. The
//! last save is the stored settings again, so a completed run changes
//! nothing.
//!
//! Cutting the power during the run, which writes continuously for a few
//! seconds, almost certainly lands inside a write or an erase: the next
//! start must load either value, never fail.
//!
//! Progress shows on screen; the result is drawn at the end, repeated on
//! every USB heartbeat line, and shown as the save confirmation.

use core::fmt::Write as _;

use embedded_graphics::Drawable as _;
use embedded_graphics::mono_font::MonoTextStyle;
use embedded_graphics::mono_font::ascii::FONT_5X8;
use embedded_graphics::pixelcolor::BinaryColor;
use embedded_graphics::prelude::Point;
use embedded_graphics::text::{Baseline, Text};

use oc_core::fmt::TextBuf;
use oc_core::platform::Display;
use oc_core::settings::{Settings, SettingsStore};

use crate::flash_store::SettingsFlash;

/// Saves in one run: about twenty times what one 4 KiB sector holds, so the
/// four sectors are each reclaimed several times. Even, so the last save is
/// the original settings.
pub(crate) const SAVES: u32 = 300;

const _: () = assert!(
    SAVES.is_multiple_of(2),
    "the run must end on the original settings"
);

/// How the run ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Report {
    /// Saves that were written and read back identical.
    pub(crate) passed: u32,
    /// Sectors erased during the run.
    pub(crate) erases: u32,
    /// Whether every save passed.
    pub(crate) ok: bool,
}

impl Report {
    /// One line for the log and the screen.
    pub(crate) fn line(&self) -> TextBuf<40> {
        let mut line = TextBuf::new();
        let verdict = if self.ok { "OK" } else { "FAILED" };
        let _ = write!(
            line,
            "STRESS {verdict} {}/{SAVES} {} ERASES",
            self.passed, self.erases
        );
        line
    }
}

/// Runs the stress test on `store`, starting from `original`, the settings
/// it holds; `poll` keeps the USB log alive.
pub(crate) fn run(
    store: &mut SettingsStore<SettingsFlash>,
    original: Settings,
    display: &mut impl Display,
    poll: &mut impl FnMut(),
) -> Report {
    let mut tweaked = original;
    tweaked.display.contrast ^= 1;
    let erases_before = store.flash().erase_count();
    let mut report = Report {
        passed: 0,
        erases: 0,
        ok: true,
    };

    for index in 1..=SAVES {
        let expected = if index.is_multiple_of(2) {
            original
        } else {
            tweaked
        };
        let verdict = store
            .save(&expected)
            .and_then(|()| store.load())
            .map(|loaded| loaded == Some(expected));
        if verdict != Ok(true) {
            log::warn!("stress: save {index} failed: {verdict:?}");
            report.ok = false;
            break;
        }
        report.passed = index;
        if index.is_multiple_of(10) {
            report.erases = store.flash().erase_count() - erases_before;
            show(display, &report.line());
            log::info!("{}", report.line().as_str());
            poll();
        }
    }
    report.erases = store.flash().erase_count() - erases_before;
    show(display, &report.line());
    log::info!("{}", report.line().as_str());
    poll();
    report
}

/// Draws one line on an otherwise blank screen.
fn show(display: &mut impl Display, line: &TextBuf<40>) {
    let frame = display.frame_mut();
    frame.clear();
    let style = MonoTextStyle::new(&FONT_5X8, BinaryColor::On);
    let _ = Text::with_baseline(line.as_str(), Point::new(0, 28), style, Baseline::Top).draw(frame);
    display.present();
}
