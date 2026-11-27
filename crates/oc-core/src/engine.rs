//! The tick loop shared by every backend.
//!
//! [`Engine::tick`] is the single function the firmware, the simulator and the
//! VCV Rack module call. Everything that happens inside it — acquisition,
//! state update, output, rendering — is identical in the three environments,
//! which is what makes a test written against the simulator meaningful for the
//! hardware.
//!
//! A tick runs the signal path every time and does at most one thing for the
//! screen: nothing, draw a frame, or send one page of the frame already drawn
//! ([`Display::page_count`]). Drawing and a whole frame together outlast a
//! 1 kHz period on the module, so a paged display gets its frame over the
//! ticks after the drawing, and the next frame waits for the last page
//! (journal/D-023, option C+; `docs/agents/firmware.md`, "The screen").

use crate::apps::sound::{Params, Sound};
use crate::apps::{App, AppHost, AppId, MAX_INSTANCES};
use crate::apps::{InputSnapshot, TickContext};
use crate::audio::{Route, Routed};
use crate::buttons::{ButtonEvents, ButtonReader};
use crate::calibration::Calibration;
use crate::framebuffer::FrameBuffer;
use crate::gfx::controls::instance_marks;
use crate::menu::{Menu, MenuEntry, MenuOutcome};
use crate::platform::{
    AnalogIn, AnalogOut, Button, CV_CHANNELS, Clock, Controls, CvChannel, DigitalIn, Display,
    MilliVolts, TRIGGER_CHANNELS, TriggerChannel,
};
use crate::playback::Patch;
use crate::profile::{Profile, Stage};
use crate::routing::RoutingPage;
use crate::settings::Settings;
use crate::splash::SplashScreen;
use crate::system::{Line, SystemOutcome, SystemScreen};
use crate::toast::{SaveOutcome, Toast};
use crate::wizard::{CalibrationWizard, WizardOutcome};

/// Upper bound reported for a tick duration, in microseconds.
///
/// A clock that jumps backwards, or one whose 64-bit microsecond counter
/// wraps, must not turn into an absurd duration on screen.
const MAX_REPORTED_DURATION_MICROS: u32 = 1_000_000;

/// Default number of ticks between two screen redraws.
///
/// One means "redraw on every tick", which is what tests and the simulator
/// want. Benchmarks show that drawing the screen costs roughly a thousand
/// times more than updating the applet state, while an OLED panel cannot show
/// more than about sixty frames per second, so the firmware raises this to
/// spend its 1 kHz budget on the signal path instead.
pub const DEFAULT_RENDER_INTERVAL: u32 = 1;

/// Ticks a second on every backend; the rate of a voice played once a tick,
/// as the simulator's is.
pub const TICK_RATE_HZ: u32 = 1_000;

/// Outcome of one tick.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TickReport {
    /// Clock reading taken at the start of the tick.
    pub timestamp_micros: u64,
    /// Microseconds elapsed since the previous tick, zero on the first one.
    pub elapsed_micros: u32,
    /// Time spent inside this tick, in microseconds.
    pub duration_micros: u32,
    /// Number of ticks executed since the engine was created.
    pub tick_count: u64,
    /// Levels the CV outputs carried when this tick started, as the backend
    /// played them ([`Engine::write_outputs`], [`Engine::set_played`]): one
    /// tick behind [`Engine::params`] (journal/D-040).
    pub cv_out: [MilliVolts; CV_CHANNELS],
    /// Whether the screen was redrawn during this tick. A one-page display
    /// was also sent the frame; a paged one gets it over the next ticks.
    pub rendered: bool,
}

/// Which screen currently owns the display.
///
/// The boot animation and the app menu both take the panel away from the
/// running applet, but for different reasons and at different costs: the splash
/// screen also holds the outputs at rest, while the menu lets the applet keep
/// running underneath.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Screen {
    /// The boot animation, before any applet runs.
    Splash,
    /// The running applet.
    App,
    /// The app picker.
    Menu,
    /// The routing page of the applet with the focus (journal/D-046).
    Routing,
    /// The system settings; the applet is paused and the outputs rest.
    System,
    /// The calibration wizard; the applet is paused and the wizard drives
    /// the outputs.
    Calibration,
}

/// Drives the applets against a platform.
#[derive(Debug)]
pub struct Engine<A, O, D, C, K, S> {
    analog_in: A,
    analog_out: O,
    digital_in: D,
    controls: C,
    clock: K,
    display: S,
    settings: Settings,
    apps: AppHost,
    splash: SplashScreen,
    /// Short commit hash, `-dirty` if the tree differed, empty until a
    /// backend sets it: drawn by the splash and the settings screens.
    build_id: &'static str,
    menu: Menu,
    routing: RoutingPage,
    system: SystemScreen,
    wizard: CalibrationWizard,
    /// Raw ADC code of each input on the last tick, for the wizard.
    raw_codes: [u16; CV_CHANNELS],
    buttons: ButtonReader,
    /// The buttons as last seen during the splash screen: what is still held
    /// when it ends picks the boot gesture.
    boot_buttons: ButtonEvents,
    /// Contrast last sent to the display, so it is only sent on change.
    applied_contrast: Option<u8>,
    /// Set when the settings screen saved; taken by the backend.
    save_requested: bool,
    /// The message drawn over the screen, such as a save's outcome.
    toast: Option<Toast>,
    /// Where the time of the ticks went (journal/D-029).
    profile: Profile,
    /// Fine counter at the start of the previous tick, for the period.
    previous_fine: Option<u32>,
    /// Levels a host probe holds on the outputs (journal/D-029).
    forced_outputs: [Option<MilliVolts>; CV_CHANNELS],
    /// What the outputs carry since the last tick (journal/D-033, D-046).
    patch: Patch,
    /// Moves on whenever [`Self::patch`] changes.
    /// What the outputs last played, as the backend reported it.
    played: [MilliVolts; CV_CHANNELS],
    screen: Screen,
    tick_count: u64,
    previous_micros: Option<u64>,
    last_duration_micros: u32,
    render_interval: u32,
    ticks_since_render: u32,
    /// The page of the last drawn frame still to send, on a paged display.
    next_page: Option<usize>,
}

impl<A, O, D, C, K, S> Engine<A, O, D, C, K, S>
where
    A: AnalogIn,
    O: AnalogOut,
    D: DigitalIn,
    C: Controls,
    K: Clock,
    S: Display,
{
    /// Builds an engine over the given platform implementations, running
    /// the start-up applet of the default settings: [`Self::without_apps`]
    /// then [`Self::apply_settings`].
    pub fn new(
        analog_in: A,
        analog_out: O,
        digital_in: D,
        controls: C,
        clock: K,
        display: S,
    ) -> Self {
        let mut engine =
            Self::without_apps(analog_in, analog_out, digital_in, controls, clock, display);
        engine.apply_settings(Settings::DEFAULT);
        engine
    }

    /// Builds an engine whose slots are all empty, for a backend that moves
    /// it into its place before applying its settings
    /// ([`Self::apply_settings`]), whose start-up applet it then runs.
    /// Choosing an applet
    /// builds it on the stack, under every frame still on it: under the
    /// engine's own temporary value (≈ 4.4 KiB on the firmware), that was
    /// the deepest path of the firmware's stack (journal/D-038, revision
    /// of 2026-11-27).
    pub fn without_apps(
        analog_in: A,
        analog_out: O,
        digital_in: D,
        controls: C,
        clock: K,
        display: S,
    ) -> Self {
        let profile = Profile::new(clock.fine_per_micro());
        Self {
            analog_in,
            analog_out,
            digital_in,
            controls,
            clock,
            display,
            settings: Settings::DEFAULT,
            apps: AppHost::EMPTY,
            splash: SplashScreen::new(),
            build_id: "",
            menu: Menu::new(),
            routing: RoutingPage::new(),
            system: SystemScreen::new(),
            wizard: CalibrationWizard::new(),
            raw_codes: [0; CV_CHANNELS],
            buttons: ButtonReader::new(),
            boot_buttons: ButtonEvents::default(),
            applied_contrast: None,
            save_requested: false,
            toast: None,
            profile,
            previous_fine: None,
            forced_outputs: [None; CV_CHANNELS],
            patch: Patch::single(Params::REST, Route::IDENTITY),
            played: [0; CV_CHANNELS],
            screen: Screen::Splash,
            tick_count: 0,
            previous_micros: None,
            last_duration_micros: 0,
            render_interval: DEFAULT_RENDER_INTERVAL,
            ticks_since_render: 0,
            next_page: None,
        }
    }

    /// Redraws the screen once every `ticks` ticks.
    ///
    /// Values below one are treated as one. Changing this affects only when the
    /// screen is refreshed; the signal path runs on every tick regardless.
    pub const fn set_render_interval(&mut self, ticks: u32) {
        self.render_interval = if ticks == 0 { 1 } else { ticks };
        self.ticks_since_render = 0;
    }

    /// Current number of ticks between two redraws.
    #[must_use]
    pub const fn render_interval(&self) -> u32 {
        self.render_interval
    }

    /// Runs one complete cycle: acquire, update, output, render.
    ///
    /// While the boot splash screen has not yet finished tracing its border
    /// around the screen, this instead runs `tick_splash`: the
    /// diagnostic applet does not see any input and the outputs stay at
    /// rest, so nothing downstream can react to a signal before the module
    /// has visibly finished starting up.
    pub fn tick(&mut self) -> TickReport {
        let tick_mark = self.clock.fine_count();
        if let Some(previous) = self.previous_fine {
            self.profile.record_period(tick_mark.wrapping_sub(previous));
        }
        self.previous_fine = Some(tick_mark);
        let started = self.clock.now_micros();
        let elapsed = self.elapsed_since_previous(started);
        if let Some(toast) = &mut self.toast
            && !toast.advance(elapsed)
        {
            self.toast = None;
        }
        self.previous_micros = Some(started);
        self.tick_count = self.tick_count.wrapping_add(1);

        if self.screen == Screen::Splash {
            let report = self.tick_splash(started, elapsed);
            self.record_since(Stage::Tick, tick_mark);
            if self.splash.is_done() {
                self.screen = self.after_boot();
                // The splash redraws on every tick; the profile describes
                // the module running, from here on. The period too: the
                // next one must not span this long splash tick.
                self.profile.clear();
                self.previous_fine = None;
            }
            return report;
        }

        let mark = self.clock.fine_count();
        let mut snapshot = self.acquire(elapsed);
        self.record_since(Stage::Acquire, mark);
        // The settings screen and the wizard read the encoders as wired (see
        // `crate::system`); everything else follows the reversal setting.
        let wired = snapshot.controls.encoder_delta;
        if !self.owns_outputs() {
            self.reverse_encoders(&mut snapshot);
        }
        let panel_was_taken = matches!(self.screen, Screen::Menu | Screen::Routing);
        self.route_panel(&snapshot, wired);
        // The click that closed the menu launched the applet, or opened the
        // settings, and the press that closed the routing page was its: it
        // is not also the applet's, nor is its release a tap.
        if panel_was_taken && !matches!(self.screen, Screen::Menu | Screen::Routing) {
            snapshot.silence_controls();
            self.buttons.swallow();
        }

        // The applet keeps running while the menu is up: time advances, the
        // inputs still reach it and its outputs stay live. Only the panel
        // changes hands, so picking an app cannot also move the applet's own
        // selection behind the menu. The settings screen pauses it and rests
        // the outputs instead: it may drive them itself (calibration).
        if matches!(self.screen, Screen::Menu | Screen::Routing) {
            snapshot.silence_controls();
        }
        let mark = self.clock.fine_count();
        self.update_params(&snapshot);
        self.record_since(Stage::Update, mark);
        let outputs = self.played;

        // A tick either draws the screen or sends one page of it, never both:
        // drawing and a whole frame together outlast a period (journal/D-023,
        // option C+). The next frame waits for the last page.
        self.ticks_since_render = self.ticks_since_render.saturating_add(1);
        let sending = self.next_page.is_some();
        if let Some(page) = self.next_page {
            let mark = self.clock.fine_count();
            self.display.present_page(page);
            self.record_since(Stage::Page, mark);
            self.next_page = Some(page + 1).filter(|&next| next < self.display.page_count());
        }
        let rendered = !sending && self.ticks_since_render >= self.render_interval;
        if rendered {
            self.ticks_since_render = 0;
            let mark = self.clock.fine_count();
            self.render_screen(&outputs);
            self.record_since(Stage::Render, mark);
            self.prepare_frame();
            if self.display.page_count() > 1 {
                self.next_page = Some(0);
            } else {
                self.send_frame();
            }
        }

        let duration = clamp_duration(self.clock.now_micros().wrapping_sub(started));
        self.last_duration_micros = duration;
        self.record_since(Stage::Tick, tick_mark);

        TickReport {
            timestamp_micros: started,
            elapsed_micros: elapsed,
            duration_micros: duration,
            tick_count: self.tick_count,
            cv_out: outputs,
            rendered,
        }
    }

    /// What the outputs carry from this tick on, from the screen in charge,
    /// then the levels a probe holds: written into the engine's patch,
    /// which counts what changed (journal/D-040).
    fn update_params(&mut self, snapshot: &InputSnapshot) {
        let parts = match self.screen {
            Screen::System => alone(Params::REST),
            Screen::Calibration => alone(Params::levels([self.wizard.output_level(); CV_CHANNELS])),
            Screen::App | Screen::Menu | Screen::Routing | Screen::Splash => {
                self.apps.update(snapshot)
            }
        };
        self.patch.set_parts(&parts);
        // The wizard measures the outputs it drives: a forced level would
        // calibrate against the wrong voltage.
        let calibrating = self.screen == Screen::Calibration;
        for (index, forced) in self.forced_outputs.into_iter().enumerate() {
            self.patch.hold(index, forced.filter(|_| !calibrating));
        }
    }

    /// Runs one tick of the boot animation.
    ///
    /// Control input is drained so it is not replayed in one lump once the
    /// diagnostic applet takes over, but otherwise ignored; the outputs stay
    /// at rest and the animation is rendered on every tick, regardless of
    /// [`Self::render_interval`], so the border traces smoothly.
    fn tick_splash(&mut self, started: u64, elapsed: u32) -> TickReport {
        let controls = self.controls.poll();
        self.boot_buttons = self.buttons.update(&controls);
        self.splash.advance(elapsed);

        let outputs = self.played;
        self.rest_patch();

        let mark = self.clock.fine_count();
        self.splash.render(self.display.frame_mut(), self.build_id);
        self.record_since(Stage::Render, mark);
        // Whole, on every tick: the splash keeps the outputs at rest, so its
        // long ticks hold nothing back.
        self.prepare_frame();
        self.send_frame();

        let duration = clamp_duration(self.clock.now_micros().wrapping_sub(started));
        self.last_duration_micros = duration;

        TickReport {
            timestamp_micros: started,
            elapsed_micros: elapsed,
            duration_micros: duration,
            tick_count: self.tick_count,
            cv_out: outputs,
            rendered: true,
        }
    }

    /// Restarts the engine as if freshly powered on: the diagnostic applet's
    /// state is discarded and the boot splash screen plays again before
    /// normal execution resumes.
    ///
    /// This is what a host should call for its own "Initialize" action (VCV
    /// Rack's `Module::onReset`, for instance), so the module confirms its
    /// identity on screen the same way after a manual reset as after a power
    /// cycle.
    pub fn reset(&mut self) {
        self.apps = AppHost::EMPTY;
        self.apps.select(self.settings.startup_app);
        self.splash.reset();
        self.menu.reset();
        self.system = SystemScreen::new();
        self.wizard = CalibrationWizard::new();
        self.buttons = ButtonReader::new();
        self.boot_buttons = ButtonEvents::default();
        self.screen = Screen::Splash;
        self.tick_count = 0;
        self.previous_micros = None;
        self.previous_fine = None;
        self.forced_outputs = [None; CV_CHANNELS];
        self.rest_patch();
        // The backend's voice restarts at rest too (`testing::MockEngine`).
        self.played = [0; CV_CHANNELS];
        self.last_duration_micros = 0;
        self.ticks_since_render = 0;
        self.next_page = None;
    }

    /// Where the time of the ticks went since the boot splash ended or the
    /// last [`Profile::clear`] (journal/D-029).
    #[must_use]
    pub const fn profile(&self) -> &Profile {
        &self.profile
    }

    /// The profile, for a backend to add its own stages
    /// ([`Stage::Sample`], [`Stage::Usb`]), set the
    /// expected period or clear it.
    pub const fn profile_mut(&mut self) -> &mut Profile {
        &mut self.profile
    }

    /// Holds `channel` at `level` from the next tick, whatever drives it, or
    /// releases it (`None`); for a host probe testing the outputs
    /// (journal/D-029). Ignored by the boot splash, which keeps the outputs
    /// at rest, and by the calibration wizard, which must measure the levels
    /// it sets itself. [`Self::reset`] releases every output.
    pub const fn force_output(&mut self, channel: CvChannel, level: Option<MilliVolts>) {
        self.forced_outputs[channel.index()] = level;
    }

    /// What the applet with the focus publishes for its lanes since the
    /// last tick (journal/D-033, D-046). What the outputs carry is
    /// [`Self::patch`].
    #[must_use]
    pub const fn params(&self) -> &Params {
        &self.patch.parts()[self.apps.focused_slot()].params
    }

    /// What the outputs carry since the last tick: each slot's parameters
    /// and route, levels held by the probe included; what a backend's
    /// voices render at their own rate ([`crate::audio::Player`],
    /// journal/D-040, D-046). The engine renders none.
    #[must_use]
    pub const fn patch(&self) -> &Patch {
        &self.patch
    }

    /// The levels the outputs carry, as a backend that plays them itself
    /// played them, for the next tick to report: Rack, or the module's audio
    /// interrupt, which write no output of the engine (journal/D-040).
    pub const fn set_played(&mut self, levels: [MilliVolts; CV_CHANNELS]) {
        self.played = levels;
    }

    /// Every output at rest: the first slot at rest along the identity,
    /// the others empty, nothing held.
    fn rest_patch(&mut self) {
        self.patch.set_parts(&alone(Params::REST));
        for index in 0..CV_CHANNELS {
            self.patch.hold(index, None);
        }
    }

    /// The levels held by [`Self::force_output`].
    #[must_use]
    pub const fn forced_outputs(&self) -> &[Option<MilliVolts>; CV_CHANNELS] {
        &self.forced_outputs
    }

    /// The raw ADC code of each input on the last tick.
    #[must_use]
    pub const fn raw_codes(&self) -> &[u16; CV_CHANNELS] {
        &self.raw_codes
    }

    /// Records the time since `mark`, a fine count read earlier, as one
    /// duration of `stage`.
    fn record_since(&mut self, stage: Stage, mark: u32) {
        let fine = self.clock.fine_count().wrapping_sub(mark);
        self.profile.record(stage, fine);
    }

    /// Sets the build identifier drawn under the version on the splash screen,
    /// and after it in the title of the settings screen (short commit hash,
    /// `-dirty` if the tree differed). Each backend passes the `OC_BUILD_ID`
    /// its build script computed (journal/D-025); it survives
    /// [`Self::reset`].
    pub const fn set_build_id(&mut self, build_id: &'static str) {
        self.build_id = build_id;
    }

    /// The build identifier set by the backend, empty if none.
    #[must_use]
    pub const fn build_id(&self) -> &'static str {
        self.build_id
    }

    /// Skips straight past the boot splash screen, as if it had already run
    /// its course.
    ///
    /// Meant for hosts and tests that have no use for the boot animation;
    /// production backends should let it play out instead.
    pub const fn skip_splash(&mut self) {
        self.splash.finish();
        self.screen = Screen::App;
    }

    /// Opens, closes and drives the app menu from this tick's panel input.
    ///
    /// The chord is handled before navigation, and never both in one tick: the
    /// tick that opens the menu must not also move its highlight or close it
    /// again.
    ///
    /// On the settings screen the chord leaves without keeping the draft, like
    /// `EXIT`, rather than opening the menu over it.
    fn route_panel(&mut self, snapshot: &InputSnapshot, wired: [i8; 2]) {
        if snapshot.buttons.menu_chord() {
            match self.screen {
                Screen::Menu | Screen::Routing => self.screen = Screen::App,
                Screen::System => {
                    self.settings = *self.system.original();
                    self.screen = Screen::App;
                }
                Screen::Calibration => {
                    if let WizardOutcome::Discard(settings) = self.wizard.discard() {
                        self.settings = settings;
                    }
                    self.screen = Screen::App;
                }
                Screen::App | Screen::Splash => {
                    self.menu.open(self.apps.current());
                    self.screen = Screen::Menu;
                }
            }
            return;
        }

        match self.screen {
            Screen::Menu => {
                match self
                    .menu
                    .update(&snapshot.buttons, snapshot.controls.encoder_delta)
                {
                    Some(MenuOutcome::Launch(chosen)) => {
                        self.apps.select(chosen);
                        self.screen = Screen::App;
                    }
                    Some(MenuOutcome::OpenSetup) => {
                        self.system.open(self.settings, Line::StartupApp);
                        self.screen = Screen::System;
                    }
                    None => {}
                }
            }
            Screen::System => self.route_system(&snapshot.buttons, wired),
            Screen::Calibration => self.route_wizard(&snapshot.buttons, wired),
            // A short `up` or `down` moves the focus to the applet left or
            // right, from the last round to the first and back, on the
            // applet's screen as on its routing page; a long `up` opens that
            // page, and closes it again. A long `down` does nothing: kept for
            // another page (journal/D-046, revision of 2026-11-26).
            Screen::App => {
                self.move_focus(&snapshot.buttons);
                if snapshot.buttons.long_pressed(Button::Up) {
                    self.routing.open();
                    self.screen = Screen::Routing;
                }
            }
            Screen::Routing => {
                if snapshot.buttons.long_pressed(Button::Up) {
                    self.screen = Screen::App;
                } else if self.move_focus(&snapshot.buttons) {
                    self.routing.open();
                } else {
                    self.routing
                        .update(snapshot.controls.encoder_delta, &mut self.apps);
                }
            }
            Screen::Splash => {}
        }
    }

    /// Draws the screen on show into the frame: the applet with the focus,
    /// or a page of the engine, with the instances' marks over the applet's
    /// screen, its routing page and the app menu.
    fn render_screen(&mut self, outputs: &[MilliVolts; CV_CHANNELS]) {
        // The applet sees its own lanes: what reached the jack each
        // goes to, nothing for one that goes nowhere (journal/D-046).
        let route = self.apps.route();
        let cv_out = core::array::from_fn(|lane| {
            route
                .output(lane)
                .map_or(0, |output| outputs[output.index()])
        });
        let context = TickContext {
            tick_count: self.tick_count,
            duration_micros: self.last_duration_micros,
            cv_out,
            outputs_forced: self.screen != Screen::Calibration
                && self.forced_outputs.iter().any(Option::is_some),
        };
        match self.screen {
            // The marks stay over the menu, so that it shows which slot
            // the app picked will take (journal/D-021, revision of
            // 2026-11-26).
            Screen::Menu => {
                let frame = self.display.frame_mut();
                self.menu.render(frame);
                instance_marks(frame, self.apps.occupied(), self.apps.focus());
            }
            Screen::Routing => {
                let frame = self.display.frame_mut();
                self.routing.render(frame, &self.apps);
                instance_marks(frame, self.apps.occupied(), self.apps.focus());
            }
            Screen::System => self.system.render(self.display.frame_mut(), self.build_id),
            Screen::Calibration => self.wizard.render(self.display.frame_mut()),
            Screen::App | Screen::Splash => {
                let frame = self.display.frame_mut();
                self.apps.render(frame, &context);
                instance_marks(frame, self.apps.occupied(), self.apps.focus());
            }
        }
    }

    /// Moves the focus one applet left on a short `up`, one right on a short
    /// `down`, wrapping at both ends; whether it moved.
    fn move_focus(&mut self, buttons: &ButtonEvents) -> bool {
        let before = self.apps.focus();
        if buttons.pressed(Button::Up) {
            self.apps.step_focus(false);
        }
        if buttons.pressed(Button::Down) {
            self.apps.step_focus(true);
        }
        self.apps.focus() != before
    }

    /// Drives the settings screen; the engine runs with its draft while open.
    fn route_system(&mut self, buttons: &ButtonEvents, wired: [i8; 2]) {
        match self.system.update(buttons, wired) {
            Some(SystemOutcome::Save(settings)) => {
                self.settings = settings;
                self.save_requested = true;
                self.screen = Screen::App;
            }
            Some(SystemOutcome::Exit(settings)) => {
                self.settings = settings;
                self.screen = Screen::App;
            }
            Some(SystemOutcome::Calibrate) => {
                // Calibrating from the settings keeps their edits in the
                // draft; discarding goes back to what was in force before.
                self.wizard
                    .open(*self.system.original(), *self.system.draft());
                self.settings = *self.wizard.draft();
                self.screen = Screen::Calibration;
            }
            None => self.settings = *self.system.draft(),
        }
    }

    /// Drives the calibration wizard; the engine runs with its draft, so a
    /// DAC code reaches the jack on the same tick.
    fn route_wizard(&mut self, buttons: &ButtonEvents, wired: [i8; 2]) {
        match self.wizard.update(buttons, wired, self.raw_codes) {
            Some(WizardOutcome::Save(settings)) => {
                self.settings = settings;
                self.save_requested = true;
                self.screen = Screen::App;
            }
            Some(WizardOutcome::Discard(settings)) => {
                self.settings = settings;
                self.screen = Screen::App;
            }
            None => self.settings = *self.wizard.draft(),
        }
    }

    /// Whether a screen that drives the outputs itself owns the panel.
    const fn owns_outputs(&self) -> bool {
        matches!(self.screen, Screen::System | Screen::Calibration)
    }

    /// Where the module goes once the splash screen has played: the settings
    /// screen if a boot gesture is still held (journal/D-026), the startup
    /// applet otherwise.
    ///
    /// * `up` + `down`: safe mode, the screen and encoder preferences at their
    ///   defaults until saved;
    /// * the left encoder switch: the calibration wizard.
    fn after_boot(&mut self) -> Screen {
        let held = self.boot_buttons;
        if held.held(Button::Up) && held.held(Button::Down) {
            self.system.open_safe(self.settings);
            self.settings = *self.system.draft();
            Screen::System
        } else if held.held(Button::LeftEncoder) {
            self.wizard.open(self.settings, self.settings);
            Screen::Calibration
        } else {
            Screen::App
        }
    }

    /// Negates the detents of each encoder set to reversed.
    fn reverse_encoders(&self, snapshot: &mut InputSnapshot) {
        for (delta, &reversed) in snapshot
            .controls
            .encoder_delta
            .iter_mut()
            .zip(&self.settings.encoder_reversed)
        {
            if reversed {
                *delta = delta.saturating_neg();
            }
        }
    }

    /// Readies the drawn frame for sending, after the screen preferences: the
    /// contrast when it changed, then the shift and the half turn, applied to
    /// the pixels in the core so every backend and controller shows the same
    /// picture.
    fn prepare_frame(&mut self) {
        let display = self.settings.display;
        if self.applied_contrast != Some(display.contrast) {
            self.display.set_contrast(display.contrast);
            self.applied_contrast = Some(display.contrast);
        }
        let frame = self.display.frame_mut();
        if let Some(toast) = &self.toast {
            toast.draw(frame);
        }
        if display.rotated {
            frame.rotate_half_turn();
        }
        frame.shift_columns(i32::from(display.column_shift));
    }

    /// Sends the whole frame at once.
    fn send_frame(&mut self) {
        let mark = self.clock.fine_count();
        self.display.present();
        self.record_since(Stage::Present, mark);
    }

    /// Samples every input.
    fn acquire(&mut self, elapsed_micros: u32) -> InputSnapshot {
        let mut snapshot = InputSnapshot {
            elapsed_micros,
            controls: self.controls.poll(),
            ..InputSnapshot::default()
        };
        snapshot.buttons = self.buttons.update(&snapshot.controls);

        for channel in crate::platform::CvChannel::ALL {
            let index = channel.index();
            let code = self.analog_in.read_code(channel);
            self.raw_codes[index] = code;
            snapshot.cv[index] = self.settings.calibration.inputs[index].to_millivolts(code);
        }

        for channel in TriggerChannel::ALL {
            snapshot.triggers[channel.index()] = self.digital_in.trigger_state(channel);
        }

        debug_assert_eq!(snapshot.cv.len(), CV_CHANNELS);
        debug_assert_eq!(snapshot.triggers.len(), TRIGGER_CHANNELS);
        snapshot
    }

    /// Writes `levels` on the outputs the engine was given, converted by its
    /// calibration and moved together, and keeps them for the next tick to
    /// report ([`Self::set_played`]): for a backend that plays its voice
    /// into the engine's outputs, the simulator once a tick (journal/D-040).
    pub fn write_outputs(&mut self, levels: &[MilliVolts; CV_CHANNELS]) {
        let mark = self.clock.fine_count();
        for channel in crate::platform::CvChannel::ALL {
            let index = channel.index();
            let code = self.settings.calibration.outputs[index].to_code(levels[index]);
            self.analog_out.write_code(channel, code);
        }
        self.analog_out.flush();
        self.played = *levels;
        self.record_since(Stage::Outputs, mark);
    }

    /// The calibration currently converting every input and output.
    #[must_use]
    pub const fn calibration(&self) -> &Calibration {
        &self.settings.calibration
    }

    /// Replaces the calibration, effective from the next tick. It survives
    /// [`Self::reset`]: it belongs to the module, not to a run.
    pub const fn set_calibration(&mut self, calibration: Calibration) {
        self.settings.calibration = calibration;
    }

    /// The settings in force, as a backend would save them
    /// ([`crate::settings::SettingsStore::save`]).
    #[must_use]
    pub const fn settings(&self) -> &Settings {
        &self.settings
    }

    /// Installs settings loaded at power-up: the calibration takes effect on
    /// the next tick and the startup applet is selected. Like the
    /// calibration, they survive [`Self::reset`], which restarts in the
    /// startup applet.
    ///
    /// Screen and encoder preferences are carried but not yet applied
    /// (step 4, lot 4c).
    pub fn apply_settings(&mut self, settings: Settings) {
        self.settings = settings;
        self.apps.select(settings.startup_app);
    }

    /// Time since the previous tick, clamped and wrap-safe.
    fn elapsed_since_previous(&self, now: u64) -> u32 {
        let Some(previous) = self.previous_micros else {
            return 0;
        };
        clamp_duration(now.wrapping_sub(previous))
    }

    /// Every applet, for inspection by tests and by the host UIs.
    #[must_use]
    pub const fn apps(&self) -> &AppHost {
        &self.apps
    }

    /// The first running applet of type `T`, left to right: its own state,
    /// for tests and the host UIs (journal/D-043, D-046). The engine itself
    /// only ever runs an applet through [`App`].
    ///
    /// # Panics
    ///
    /// If no applet of type `T` runs, which is a mistake in the caller, not
    /// a state the module can reach by itself.
    #[must_use]
    pub fn app<T: App>(&self) -> &T {
        self.apps.find::<T>().expect("an applet of that type runs")
    }

    /// The kind of applet with the focus.
    #[must_use]
    pub fn current_app(&self) -> AppId {
        self.apps.current()
    }

    /// The first running applet of kind `id`, to set its parameters or ask
    /// it for an action as the panel would (journal/D-043): the host
    /// probe's way in; `None` while none runs.
    pub fn app_mut(&mut self, id: AppId) -> Option<&mut dyn App> {
        self.apps.get_mut(id)
    }

    /// Every running applet, to add, remove, focus and route them as the
    /// panel would (journal/D-046).
    pub const fn apps_mut(&mut self) -> &mut AppHost {
        &mut self.apps
    }

    /// Puts `id` in the place of the applet with the focus, as launching it
    /// from the app menu does: an open menu closes on it. The system screen,
    /// the calibration wizard and the boot splash stay up, and leave to it.
    pub fn select_app(&mut self, id: AppId) {
        self.apps.select(id);
        if matches!(self.screen, Screen::Menu) {
            self.screen = Screen::App;
        }
    }

    /// Puts a fresh applet of kind `id` in the slot at `position`, as the
    /// menu does on the one with the focus ([`AppHost::select_at`]);
    /// `false` for no such slot.
    pub fn select_app_at(&mut self, position: usize, id: AppId) -> bool {
        self.apps.select_at(position, id)
    }

    /// Whether the routing page currently owns the screen.
    #[must_use]
    pub const fn routing_is_open(&self) -> bool {
        matches!(self.screen, Screen::Routing)
    }

    /// Whether the app menu currently owns the screen.
    #[must_use]
    pub const fn menu_is_open(&self) -> bool {
        matches!(self.screen, Screen::Menu)
    }

    /// The entry the menu is highlighting, meaningful only while it is open.
    #[must_use]
    pub const fn menu_selection(&self) -> MenuEntry {
        self.menu.selected()
    }

    /// Whether the system settings screen currently owns the panel.
    #[must_use]
    pub const fn system_is_open(&self) -> bool {
        matches!(self.screen, Screen::System)
    }

    /// Whether the calibration wizard currently owns the panel.
    #[must_use]
    pub const fn calibration_is_open(&self) -> bool {
        matches!(self.screen, Screen::Calibration)
    }

    /// The calibration wizard, for inspection by tests and host UIs.
    #[must_use]
    pub const fn wizard(&self) -> &CalibrationWizard {
        &self.wizard
    }

    /// The system settings screen, for inspection by tests and host UIs.
    #[must_use]
    pub const fn system(&self) -> &SystemScreen {
        &self.system
    }

    /// Shows how the save asked by [`Self::take_save_request`] went, over
    /// whatever is on screen, for [`crate::toast::DURATION_MICROS`]: the
    /// backend saved, so only it can tell.
    pub const fn report_save(&mut self, outcome: SaveOutcome) {
        self.toast = Some(Toast::new(outcome));
    }

    /// The message currently drawn over the screen, if any.
    #[must_use]
    pub const fn toast(&self) -> Option<SaveOutcome> {
        match &self.toast {
            Some(toast) => Some(toast.outcome()),
            None => None,
        }
    }

    /// Whether the settings screen asked for the settings to be stored since
    /// the last call; the request is consumed.
    ///
    /// The backend owns the store ([`crate::settings::SettingsStore`]): on
    /// `true` it saves [`Self::settings`]. The save blocks; it only ever
    /// follows an explicit `SAVE`.
    pub const fn take_save_request(&mut self) -> bool {
        let requested = self.save_requested;
        self.save_requested = false;
        requested
    }

    /// The framebuffer as last rendered.
    pub fn frame(&mut self) -> &FrameBuffer {
        self.display.frame_mut()
    }

    /// Number of ticks executed so far.
    #[must_use]
    pub const fn tick_count(&self) -> u64 {
        self.tick_count
    }

    /// The clock, so a host can advance a virtual one between ticks.
    #[must_use]
    pub const fn clock(&self) -> &K {
        &self.clock
    }

    /// Borrows the platform pieces, so a host can drive its own inputs between
    /// ticks.
    pub const fn parts_mut(&mut self) -> (&mut A, &mut O, &mut D, &mut C, &mut S) {
        (
            &mut self.analog_in,
            &mut self.analog_out,
            &mut self.digital_in,
            &mut self.controls,
            &mut self.display,
        )
    }
}

/// Converts a raw microsecond difference into a plausible duration.
fn clamp_duration(raw: u64) -> u32 {
    u32::try_from(raw).map_or(MAX_REPORTED_DURATION_MICROS, |value| {
        value.min(MAX_REPORTED_DURATION_MICROS)
    })
}

/// `params` in the first slot along the identity, the others empty: what
/// an engine screen plays.
fn alone(params: Params) -> [Routed<Sound>; MAX_INSTANCES] {
    let mut parts = [Routed::OFF; MAX_INSTANCES];
    parts[0] = Routed {
        params,
        route: Route::IDENTITY,
    };
    parts
}

#[cfg(test)]
mod tests {
    use super::{MAX_REPORTED_DURATION_MICROS, TICK_RATE_HZ, clamp_duration};
    use crate::apps::diagnostic::{DiagnosticApp, OutputMode};
    use crate::platform::{CV_CHANNELS, CvChannel};
    use crate::playback::Player;
    use crate::splash;
    use crate::testing::{adc_reading, bare_mock_engine_at_boot, mock_engine_at_boot};

    #[test]
    fn durations_are_clamped_to_something_believable() {
        assert_eq!(clamp_duration(0), 0);
        assert_eq!(clamp_duration(1_000), 1_000);
        assert_eq!(
            clamp_duration(u64::MAX),
            MAX_REPORTED_DURATION_MICROS,
            "a backwards clock must not report a nonsense duration"
        );
        assert_eq!(
            clamp_duration(5_000_000),
            MAX_REPORTED_DURATION_MICROS,
            "a stalled tick is reported as the ceiling, not as-is"
        );
    }

    #[test]
    fn a_freshly_built_engine_boots_into_the_splash_screen() {
        let mut engine = mock_engine_at_boot(0);
        {
            let (analog_in, ..) = engine.parts_mut();
            analog_in.patch(CvChannel::Two, 4_000);
        }

        let report = engine.tick();
        assert_eq!(
            engine.outputs(),
            [0; CV_CHANNELS],
            "outputs stay at rest during the boot animation"
        );
        assert!(report.rendered);
        assert!(
            engine.frame().lit_pixels() > 0,
            "the banner must be visible on the very first frame"
        );
    }

    #[test]
    fn normal_execution_starts_once_the_splash_screen_is_done() {
        let mut engine = mock_engine_at_boot(0);
        {
            let (analog_in, ..) = engine.parts_mut();
            analog_in.patch(CvChannel::Two, 4_000);
        }

        // Run comfortably past the end of the boot animation.
        let ticks = splash::DURATION_MICROS / 1_000 + 2;
        for _ in 0..ticks {
            engine.clock().advance(1_000);
            engine.tick();
        }

        engine.clock().advance(1_000);
        engine.tick();

        assert_eq!(
            engine.outputs()[1],
            adc_reading(4_000),
            "once booted, inputs must reach the matching output again"
        );
        assert_eq!(engine.app::<DiagnosticApp>().mode(), OutputMode::Offset);
    }

    #[test]
    fn resetting_the_engine_replays_the_splash_screen() {
        let mut engine = mock_engine_at_boot(0);
        let ticks = splash::DURATION_MICROS / 1_000 + 2;
        for _ in 0..ticks {
            engine.clock().advance(1_000);
            engine.tick();
        }
        assert_eq!(engine.tick_count(), u64::from(ticks));

        engine.reset();
        assert_eq!(engine.tick_count(), 0);

        engine.tick();
        assert_eq!(
            engine.outputs(),
            [0; CV_CHANNELS],
            "a reset engine boots again before resuming normal execution"
        );
    }

    #[test]
    fn the_build_id_survives_a_reset() {
        let mut engine = mock_engine_at_boot(0);
        assert_eq!(engine.build_id(), "");
        engine.set_build_id("a1b2c3d-dirty");
        engine.reset();
        assert_eq!(engine.build_id(), "a1b2c3d-dirty");
    }

    #[test]
    fn applied_settings_drive_the_conversion_and_the_startup_applet() {
        use crate::apps::AppId;
        use crate::calibration::CvOutputCalibration;
        use crate::settings::Settings;

        let mut engine = mock_engine_at_boot(0);
        let mut settings = Settings::DEFAULT;
        settings.startup_app = AppId::Scope;
        // Every output 1000 codes below the factory table (the top point is
        // already 0).
        let mut shifted = CvOutputCalibration::FACTORY;
        for code in &mut shifted.codes {
            *code = code.saturating_sub(1_000);
        }
        settings.calibration.outputs = [shifted; CV_CHANNELS];
        engine.apply_settings(settings);
        engine.skip_splash();
        assert_eq!(engine.current_app(), AppId::Scope);

        engine.tick();
        let (_, analog_out, ..) = engine.parts_mut();
        assert_eq!(analog_out.codes, [shifted.to_code(0); CV_CHANNELS]);

        // A reset is a restart of the same module: same settings, same app.
        engine.reset();
        assert_eq!(engine.current_app(), AppId::Scope);
        assert_eq!(engine.settings(), &settings);
    }

    /// A booted engine whose display sends its frame in `pages` parts.
    fn paged_engine(pages: usize, render_interval: u32) -> crate::testing::MockEngine {
        let mut engine = mock_engine_at_boot(0);
        engine.parts_mut().4.pages = pages;
        engine.skip_splash();
        engine.set_render_interval(render_interval);
        engine
    }

    #[test]
    fn a_paged_display_gets_one_page_per_tick_after_the_drawing() {
        let mut engine = paged_engine(8, 20);
        let rendered: [bool; 40] = core::array::from_fn(|_| engine.tick().rendered);
        // Drawn on the 20th tick and every 20 after; the pages follow it.
        let mut drawn = (0..40).filter(|&tick| rendered[tick]);
        assert_eq!(
            (drawn.next(), drawn.next(), drawn.next()),
            (Some(19), Some(39), None)
        );
        let display = &engine.parts_mut().4;
        assert_eq!(
            display.pages_sent(),
            [0, 1, 2, 3, 4, 5, 6, 7].map(Some),
            "eight pages, in order, one per tick"
        );
        assert_eq!(display.presents, 0, "never the whole frame at once");
    }

    #[test]
    fn no_frame_is_drawn_while_the_last_one_is_being_sent() {
        // Asked to redraw on every tick, a paged display still needs eight
        // ticks per frame: drawing over half-sent pixels would tear it.
        let mut engine = paged_engine(4, 1);
        let rendered: [bool; 10] = core::array::from_fn(|_| engine.tick().rendered);
        assert_eq!(
            rendered,
            [
                true, false, false, false, false, true, false, false, false, false
            ]
        );
        assert_eq!(
            engine.parts_mut().4.pages_sent(),
            [0, 1, 2, 3, 0, 1, 2, 3].map(Some)
        );
    }

    #[test]
    fn a_one_page_display_is_sent_the_frame_it_was_drawn() {
        let mut engine = paged_engine(1, 3);
        let rendered: [bool; 6] = core::array::from_fn(|_| engine.tick().rendered);
        assert_eq!(rendered, [false, false, true, false, false, true]);
        let display = &engine.parts_mut().4;
        assert_eq!(display.presents, 2);
        assert_eq!(display.pages_sent(), &[] as &[Option<usize>]);
    }

    #[test]
    fn the_splash_sends_whole_frames_even_to_a_paged_display() {
        let mut engine = mock_engine_at_boot(0);
        engine.parts_mut().4.pages = 8;
        engine.tick();
        engine.tick();
        let display = &engine.parts_mut().4;
        assert_eq!(display.presents, 2);
        assert_eq!(display.pages_sent(), &[] as &[Option<usize>]);
    }

    #[test]
    fn skip_splash_bypasses_the_boot_animation() {
        let mut engine = mock_engine_at_boot(0);
        engine.skip_splash();
        {
            let (analog_in, ..) = engine.parts_mut();
            analog_in.patch(CvChannel::Two, 1_500);
        }

        engine.tick();
        assert_eq!(engine.outputs()[1], adc_reading(1_500));
    }

    #[test]
    fn the_patch_revision_moves_only_when_the_params_change() {
        let mut engine = mock_engine_at_boot(0);
        engine.skip_splash();
        engine.parts_mut().0.patch(CvChannel::Two, 1_500);
        engine.tick();
        let first = engine.patch().revision();
        engine.tick();
        assert_eq!(engine.patch().revision(), first, "same input, same params");

        engine.parts_mut().0.patch(CvChannel::Two, -800);
        engine.tick();
        assert_ne!(
            engine.patch().revision(),
            first,
            "a new level is new params"
        );
    }

    #[test]
    fn the_engine_reports_what_the_outputs_played() {
        let mut engine = bare_mock_engine_at_boot(0);
        engine.skip_splash();
        engine.parts_mut().0.patch(CvChannel::Two, 1_500);
        assert_eq!(engine.tick().cv_out, [0; CV_CHANNELS], "nothing played yet");
        assert_eq!(engine.parts_mut().1.flushes, 0, "the tick writes no output");

        let mut player = Player::new(TICK_RATE_HZ);
        let levels = player.play(engine.patch());
        assert_eq!(levels[1], adc_reading(1_500), "one sample of the params");
        engine.write_outputs(&levels);
        assert_eq!(engine.parts_mut().1.level(CvChannel::Two), levels[1]);
        assert_eq!(engine.tick().cv_out, levels, "reported on the next tick");

        engine.set_played([1, 2, 3, 4]);
        assert_eq!(engine.tick().cv_out, [1, 2, 3, 4], "or what a backend gave");
    }
}
