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
//!
//! This module also carries the contract every applet is written against —
//! [`InputSnapshot`] and [`TickContext`] — so that one applet never has to
//! import another just to describe what it is fed. Each applet itself lives in
//! its own submodule ([`diagnostic`], [`scope`], [`gfx_demo`], [`ratchet`],
//! [`tone`]).
//!
//! An applet's `update` returns what the outputs carry as
//! [`Params`](crate::audio::Params): the CV applets hold levels, `TONE`
//! asks for tones that a [`Voice`](crate::audio::Voice) renders
//! (journal/D-033).

pub mod diagnostic;
pub mod gfx_demo;
pub mod ratchet;
pub mod scope;
pub mod tone;

use serde::{Deserialize, Serialize};

use crate::apps::diagnostic::DiagnosticApp;
use crate::apps::gfx_demo::GfxDemoApp;
use crate::apps::ratchet::RatchetApp;
use crate::apps::scope::ScopeApp;
use crate::apps::tone::ToneApp;
use crate::audio::Params;
use crate::buttons::ButtonEvents;
use crate::framebuffer::FrameBuffer;
use crate::platform::{
    BUTTONS, CV_CHANNELS, ControlEvents, ENCODERS, MilliVolts, TRIGGER_CHANNELS,
};

/// Everything the applet observes during one tick.
#[derive(Debug, Clone, Copy)]
pub struct InputSnapshot {
    /// Calibrated level of each CV input.
    pub cv: [MilliVolts; CV_CHANNELS],
    /// Raw level of each trigger input.
    pub triggers: [bool; TRIGGER_CHANNELS],
    /// Raw encoder and button activity.
    pub controls: ControlEvents,
    /// Button actions, debounced and arbitrated against the menu chord.
    pub buttons: ButtonEvents,
    /// Microseconds since the previous tick.
    pub elapsed_micros: u32,
}

impl InputSnapshot {
    /// Drops every control, keeping the signal inputs and the elapsed time.
    ///
    /// The engine calls this while the app menu owns the front panel, so the
    /// running applet keeps tracking its inputs and driving its outputs without
    /// also reacting to the keys and detents aimed at the menu.
    pub const fn silence_controls(&mut self) {
        self.controls = ControlEvents {
            encoder_delta: [0; ENCODERS],
            button_down: [false; BUTTONS],
        };
        self.buttons.silence();
    }
}

impl Default for InputSnapshot {
    fn default() -> Self {
        Self {
            cv: [0; CV_CHANNELS],
            triggers: [false; TRIGGER_CHANNELS],
            controls: ControlEvents::default(),
            buttons: ButtonEvents::default(),
            elapsed_micros: 0,
        }
    }
}

/// What the applet displays but does not decide itself.
#[derive(Debug, Clone, Copy, Default)]
pub struct TickContext {
    /// Number of ticks executed since boot.
    pub tick_count: u64,
    /// Duration of the previous tick, in microseconds.
    pub duration_micros: u32,
    /// The levels actually written to the outputs on this tick: the
    /// applet's own, unless something else holds an output.
    pub cv_out: [MilliVolts; CV_CHANNELS],
    /// Whether a host probe holds any output (journal/D-029), so the screen
    /// can say its levels are not the applet's.
    pub outputs_forced: bool,
}

/// One of the applets the module can run.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default, Serialize, Deserialize,
)]
pub enum AppId {
    /// The I/O diagnostic screen the module boots into.
    #[default]
    Diagnostic,
    /// A scrolling view of `CV1`, buffered to every output.
    Scope,
    /// The graphics primitives on show (journal/D-030).
    Gfx,
    /// Rising edges on `TR1` copied to A and burst onto B (journal/D-032).
    /// Each new applet comes last, so a startup applet saved before it
    /// existed still reads back.
    Ratchet,
    /// A test tone in four waveforms on A to D (journal/D-033).
    Tone,
}

impl AppId {
    /// Every app, in the order the menu lists them.
    pub const ALL: [Self; 5] = [
        Self::Diagnostic,
        Self::Scope,
        Self::Gfx,
        Self::Ratchet,
        Self::Tone,
    ];

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
            Self::Gfx => "GFX DEMO",
            Self::Ratchet => "RATCHET",
            Self::Tone => "TONE",
        }
    }
}

/// Every applet, one instance each, plus the one currently running.
#[derive(Debug, Clone)]
pub struct AppHost {
    diagnostic: DiagnosticApp,
    scope: ScopeApp,
    gfx: GfxDemoApp,
    ratchet: RatchetApp,
    tone: ToneApp,
    current: AppId,
}

impl AppHost {
    /// Freshly started applets, with the diagnostic screen running.
    #[must_use]
    pub fn new() -> Self {
        Self {
            diagnostic: DiagnosticApp::new(),
            scope: ScopeApp::new(),
            gfx: GfxDemoApp::new(),
            ratchet: RatchetApp::new(),
            tone: ToneApp::new(),
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

    /// Runs one tick of the current applet: what the outputs carry.
    pub fn update(&mut self, input: &InputSnapshot) -> Params {
        match self.current {
            AppId::Diagnostic => self.diagnostic.update(input).into(),
            AppId::Scope => self.scope.update(input).into(),
            AppId::Gfx => self.gfx.update(input).into(),
            AppId::Ratchet => self.ratchet.update(input).into(),
            AppId::Tone => self.tone.update(input),
        }
    }

    /// Draws the current applet's screen.
    pub fn render(&self, frame: &mut FrameBuffer, context: &TickContext) {
        match self.current {
            AppId::Diagnostic => self.diagnostic.render(frame, context),
            AppId::Scope => self.scope.render(frame, context),
            AppId::Gfx => self.gfx.render(frame, context),
            AppId::Ratchet => self.ratchet.render(frame, context),
            AppId::Tone => self.tone.render(frame, context),
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

    /// The graphics demonstration, running or not.
    #[must_use]
    pub const fn gfx(&self) -> &GfxDemoApp {
        &self.gfx
    }

    /// The Ratchet applet, running or not.
    #[must_use]
    pub const fn ratchet(&self) -> &RatchetApp {
        &self.ratchet
    }

    /// The `TONE` applet, running or not.
    #[must_use]
    pub const fn tone(&self) -> &ToneApp {
        &self.tone
    }
}

impl Default for AppHost {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::InputSnapshot;
    use super::{AppHost, AppId};
    use crate::audio::{Channel, Params};
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
            assert_ne!(id.name(), "");
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
            diagnostic.channels[1],
            Channel::Level(2_000),
            "the diagnostic applet mirrors channel by channel"
        );

        host.select(AppId::Scope);
        let scope = host.update(&snapshot(1_000, 2_000));
        assert_eq!(
            scope,
            Params::levels([1_000; CV_CHANNELS]),
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
