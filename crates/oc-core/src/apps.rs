//! The applets the module can run, and which one currently owns the outputs.
//!
//! Every applet is resident: [`AppHost`] holds one instance of each and a cursor
//! saying which is running. Storing only the running one — an enum of applet
//! variants — would throw away the other's state on every switch, so an offset
//! dialled into the diagnostic applet would vanish the moment you glanced at
//! another app and came back. A handful of bytes of static state buys that
//! back, and keeps every per-applet accessor infallible.
//!
//! Only the running applet is updated. The 1 kHz budget therefore does not grow
//! with the number of apps, at the cost of a frozen applet's
//! [`SignalDetector`](crate::signal::SignalDetector) history being stale when it
//! comes back — deliberate, and cheaper than the alternative.

use crate::app::{DiagnosticApp, InputSnapshot, TickContext};
use crate::framebuffer::FrameBuffer;
use crate::platform::{CV_CHANNELS, MilliVolts};
use crate::scope::ScopeApp;

/// One of the applets the module can run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub enum AppId {
    /// The I/O diagnostic screen the module boots into.
    #[default]
    Diagnostic,
    /// A scrolling view of `CV1`, buffered to every output.
    Scope,
}

impl AppId {
    /// Every app, in the order the menu lists them.
    pub const ALL: [Self; 2] = [Self::Diagnostic, Self::Scope];

    /// Zero-based index of this app.
    #[must_use]
    pub const fn index(self) -> usize {
        self as usize
    }

    /// The app with the given index, or `None` when out of range.
    #[must_use]
    pub const fn from_index(index: usize) -> Option<Self> {
        if index < Self::ALL.len() {
            Some(Self::ALL[index])
        } else {
            None
        }
    }

    /// Name shown in the app menu.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Diagnostic => "DIAGNOSTIC",
            Self::Scope => "SCOPE",
        }
    }
}

/// Every applet, one instance each, plus the one currently running.
#[derive(Debug, Clone)]
pub struct AppHost {
    diagnostic: DiagnosticApp,
    scope: ScopeApp,
    current: AppId,
}

impl AppHost {
    /// Freshly started applets, with the diagnostic screen running.
    #[must_use]
    pub fn new() -> Self {
        Self {
            diagnostic: DiagnosticApp::new(),
            scope: ScopeApp::new(),
            current: AppId::default(),
        }
    }

    /// The applet currently driving the outputs and the screen.
    #[must_use]
    pub const fn current(&self) -> AppId {
        self.current
    }

    /// Hands the outputs and the screen to another applet.
    ///
    /// Idempotent, and it resets nothing: an applet picked up again is exactly
    /// where it was left.
    pub const fn select(&mut self, id: AppId) {
        self.current = id;
    }

    /// Runs one tick of the current applet.
    pub fn update(&mut self, input: &InputSnapshot) -> [MilliVolts; CV_CHANNELS] {
        match self.current {
            AppId::Diagnostic => self.diagnostic.update(input),
            AppId::Scope => self.scope.update(input),
        }
    }

    /// Draws the current applet's screen.
    pub fn render(&self, frame: &mut FrameBuffer, context: &TickContext) {
        match self.current {
            AppId::Diagnostic => self.diagnostic.render(frame, context),
            AppId::Scope => self.scope.render(frame, context),
        }
    }

    /// The diagnostic applet, running or not.
    #[must_use]
    pub const fn diagnostic(&self) -> &DiagnosticApp {
        &self.diagnostic
    }

    /// The scope applet, running or not.
    #[must_use]
    pub const fn scope(&self) -> &ScopeApp {
        &self.scope
    }
}

impl Default for AppHost {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::{AppHost, AppId};
    use crate::app::InputSnapshot;
    use crate::platform::{CV_CHANNELS, MilliVolts};

    fn snapshot(cv1: MilliVolts, cv2: MilliVolts) -> InputSnapshot {
        let mut input = InputSnapshot {
            elapsed_micros: 1_000,
            ..InputSnapshot::default()
        };
        input.cv[0] = cv1;
        input.cv[1] = cv2;
        input
    }

    #[test]
    fn app_indices_are_dense_and_ordered() {
        for (expected, id) in AppId::ALL.into_iter().enumerate() {
            assert_eq!(id.index(), expected);
            assert_eq!(AppId::from_index(expected), Some(id));
        }
        assert_eq!(AppId::from_index(AppId::ALL.len()), None);
    }

    #[test]
    fn every_app_has_a_name_that_fits_the_screen() {
        for id in AppId::ALL {
            assert!(!id.name().is_empty());
            assert!(
                id.name().len() <= 24,
                "{} would overflow a menu row",
                id.name()
            );
        }
    }

    #[test]
    fn a_fresh_host_runs_the_diagnostic_applet() {
        let host = AppHost::new();
        assert_eq!(host.current(), AppId::Diagnostic);
    }

    #[test]
    fn the_running_app_decides_what_the_outputs_carry() {
        let mut host = AppHost::new();
        let diagnostic = host.update(&snapshot(1_000, 2_000));
        assert_eq!(
            diagnostic[1], 2_000,
            "the diagnostic applet mirrors channel by channel"
        );

        host.select(AppId::Scope);
        let scope = host.update(&snapshot(1_000, 2_000));
        assert_eq!(
            scope, [1_000; CV_CHANNELS],
            "the scope buffers CV1 to every output, which no output mode can do"
        );
    }

    #[test]
    fn selecting_is_idempotent_and_resets_nothing() {
        let mut host = AppHost::new();
        host.update(&snapshot(0, 4_000));
        let before = *host.diagnostic().outputs();

        host.select(AppId::Scope);
        host.select(AppId::Scope);
        host.update(&snapshot(1_000, 0));

        assert_eq!(
            *host.diagnostic().outputs(),
            before,
            "a frozen applet keeps the state it was left with"
        );
        assert_eq!(host.current(), AppId::Scope);
    }

    #[test]
    fn switching_away_and_back_preserves_the_other_app() {
        let mut host = AppHost::new();
        host.update(&snapshot(0, 3_000));
        let mode = host.diagnostic().mode();
        let selected = host.diagnostic().selected();

        host.select(AppId::Scope);
        host.update(&snapshot(500, 0));
        host.select(AppId::Diagnostic);

        assert_eq!(host.diagnostic().mode(), mode);
        assert_eq!(host.diagnostic().selected(), selected);
        assert_eq!(
            host.scope().level(),
            500,
            "the scope kept its own state too"
        );
    }
}
