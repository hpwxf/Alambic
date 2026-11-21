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
//! the [`App`] trait, [`InputSnapshot`] and [`TickContext`] — so that one
//! applet never has to import another just to describe what it is fed, and
//! nothing outside [`AppHost`] has to name an applet to run it
//! (journal/D-043). Each applet itself lives in its own submodule
//! ([`diagnostic`], [`scope`], [`gfx_demo`], [`ratchet`], [`tone`],
//! [`supersaw`]).
//!
//! An applet's `update` returns what the outputs carry as
//! [`Params`]: the CV applets hold levels, `TONE`
//! asks for tones and `SUPERSAW` for parts of a supersaw bank that a
//! [`Voice`](crate::audio::Voice) renders (journal/D-033, D-037).

pub mod diagnostic;
pub mod gfx_demo;
pub mod param;
pub mod ratchet;
pub mod scope;
pub mod supersaw;
pub mod tone;

use core::any::Any;

use serde::{Deserialize, Serialize};

use crate::apps::diagnostic::DiagnosticApp;
use crate::apps::gfx_demo::GfxDemoApp;
use crate::apps::param::{ParamSpec, ParamValue, Refused};
use crate::apps::ratchet::RatchetApp;
use crate::apps::scope::ScopeApp;
use crate::apps::supersaw::SupersawApp;
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

/// What every applet is to the engine (journal/D-043).
///
/// The engine runs one applet at a time and reaches it only through this
/// trait, so a new applet changes [`AppHost`] and [`AppId`], the list of what
/// this image composes, and nothing else in the core. The screen is the
/// applet's own: on 128 x 64 pixels each one lays itself out by hand.
///
/// `Any` is a supertrait so that a test or the simulator can look at the
/// concrete applet behind a `&dyn App` ([`AppHost::find`]); the engine itself
/// never does.
pub trait App: Any {
    /// The applet's place in the image's list, which is also its persisted
    /// key: [`AppId`]'s index never changes for a given applet.
    fn id(&self) -> AppId;

    /// Name shown in the app menu.
    fn name(&self) -> &'static str {
        self.id().name()
    }

    /// Runs one tick: what the outputs carry.
    fn update(&mut self, input: &InputSnapshot) -> Params;

    /// Draws the applet's screen.
    fn render(&self, frame: &mut FrameBuffer, context: &TickContext);

    /// The parameters it is set by, in a fixed order: an index into this
    /// list names one in [`get`](Self::get) and [`set`](Self::set)
    /// ([`param`]).
    fn param_specs(&self) -> &'static [ParamSpec] {
        &[]
    }

    /// How the screen labels parameter `index` now, which another setting
    /// may change (`RATCHET`'s `FIRST` reads `SPAN` in `MODE SPAN`); its
    /// [`ParamSpec::name`] stays the one to find it by.
    fn param_label(&self, index: usize) -> &'static str {
        self.param_specs().get(index).map_or("", |spec| spec.name)
    }

    /// Whether parameter `index` acts now. One face of a setting with two
    /// is inactive while the other shows on the screen (`RATCHET`'s
    /// `DIVISION` with `SYNC OFF`, its `FIRST` with `SYNC ON`, its `COUNT`
    /// in `GATE`): it keeps its value and can still be set, ready for when
    /// the other setting brings it back.
    fn param_active(&self, _index: usize) -> bool {
        true
    }

    /// The value of parameter `index` as the panel set it, before any CV
    /// input moves it; `None` past the last.
    fn get(&self, _index: usize) -> Option<ParamValue> {
        None
    }

    /// Sets parameter `index` as a detent would, within the same bounds:
    /// what it applied, which another setting may have narrowed.
    ///
    /// It may move other parameters too, as the detent would: the other face
    /// of the same setting (`RATCHET`'s `GATE` off puts `COUNT` at its
    /// last), or a bound that depends on it (`MODE` narrowing `FIRST`). A
    /// host that shows them reads them back after a set.
    ///
    /// # Errors
    ///
    /// Why the value was not taken: no such parameter, a value of another
    /// kind, or one outside its range ([`ParamSpec::check`]).
    fn set(&mut self, _index: usize, _value: ParamValue) -> Result<ParamValue, Refused> {
        Err(Refused::NoSuch)
    }

    /// The values it can be read for but not set: what it plays once its
    /// inputs moved its parameters, what it measured, what it counted.
    fn observables(&self) -> &'static [ParamSpec] {
        &[]
    }

    /// Observable `index` now; `None` past the last, or while unknown (a
    /// tempo before its second edge).
    fn observe(&self, _index: usize) -> Option<ParamValue> {
        None
    }

    /// What it can be asked to do, by name.
    fn actions(&self) -> &'static [&'static str] {
        &[]
    }

    /// Does action `index`.
    ///
    /// # Errors
    ///
    /// [`Refused::NoSuch`] past the last action.
    fn act(&mut self, _index: usize) -> Result<(), Refused> {
        Err(Refused::NoSuch)
    }
}

/// One of the applets the module can run.
///
/// The list of what this image composes, in the order the menu lists them.
/// The index of each variant is its persisted key (the startup applet is
/// saved by it, journal/D-028), so a new applet always comes last and none
/// is ever reordered.
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
    /// Seven detuned sawtooths on A and B, a sub-octave on C
    /// (journal/D-037).
    Supersaw,
}

impl AppId {
    /// Every app, in the order the menu lists them.
    pub const ALL: [Self; 6] = [
        Self::Diagnostic,
        Self::Scope,
        Self::Gfx,
        Self::Ratchet,
        Self::Tone,
        Self::Supersaw,
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
            Self::Supersaw => "SUPERSAW",
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
    supersaw: SupersawApp,
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
            supersaw: SupersawApp::new(),
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

    /// The applet filling slot `id`.
    #[must_use]
    pub fn get(&self, id: AppId) -> &dyn App {
        match id {
            AppId::Diagnostic => &self.diagnostic,
            AppId::Scope => &self.scope,
            AppId::Gfx => &self.gfx,
            AppId::Ratchet => &self.ratchet,
            AppId::Tone => &self.tone,
            AppId::Supersaw => &self.supersaw,
        }
    }

    /// The applet filling slot `id`, to change it.
    pub fn get_mut(&mut self, id: AppId) -> &mut dyn App {
        match id {
            AppId::Diagnostic => &mut self.diagnostic,
            AppId::Scope => &mut self.scope,
            AppId::Gfx => &mut self.gfx,
            AppId::Ratchet => &mut self.ratchet,
            AppId::Tone => &mut self.tone,
            AppId::Supersaw => &mut self.supersaw,
        }
    }

    /// The applet of type `T`, running or not, if this image composes one.
    ///
    /// For tests and the simulator, which look at an applet's own state; the
    /// engine never needs the concrete type.
    #[must_use]
    pub fn find<T: App>(&self) -> Option<&T> {
        AppId::ALL.into_iter().find_map(|id| {
            let app: &dyn Any = self.get(id);
            app.downcast_ref::<T>()
        })
    }

    /// Runs one tick of the current applet: what the outputs carry.
    pub fn update(&mut self, input: &InputSnapshot) -> Params {
        self.get_mut(self.current).update(input)
    }

    /// Draws the current applet's screen.
    pub fn render(&self, frame: &mut FrameBuffer, context: &TickContext) {
        self.get(self.current).render(frame, context);
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
    use super::diagnostic::DiagnosticApp;
    use super::scope::ScopeApp;
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
    fn every_slot_holds_the_applet_that_names_it() {
        let host = AppHost::new();
        for id in AppId::ALL {
            assert_eq!(host.get(id).id(), id);
            assert_eq!(host.get(id).name(), id.name());
        }
        assert!(host.find::<ScopeApp>().is_some());
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
        let before = *host.find::<DiagnosticApp>().unwrap().outputs();

        host.select(AppId::Scope);
        host.select(AppId::Scope);
        host.update(&snapshot(1_000, 0));

        assert_eq!(
            *host.find::<DiagnosticApp>().unwrap().outputs(),
            before,
            "a frozen applet keeps the state it was left with"
        );
        assert_eq!(host.current(), AppId::Scope);
    }

    #[test]
    fn switching_away_and_back_preserves_the_other_app() {
        let mut host = AppHost::new();
        host.update(&snapshot(0, 3_000));
        let mode = host.find::<DiagnosticApp>().unwrap().mode();
        let selected = host.find::<DiagnosticApp>().unwrap().selected();

        host.select(AppId::Scope);
        host.update(&snapshot(500, 0));
        host.select(AppId::Diagnostic);

        assert_eq!(host.find::<DiagnosticApp>().unwrap().mode(), mode);
        assert_eq!(host.find::<DiagnosticApp>().unwrap().selected(), selected);
        assert_eq!(
            host.find::<ScopeApp>().unwrap().level(),
            500,
            "the scope kept its own state too"
        );
    }
}
