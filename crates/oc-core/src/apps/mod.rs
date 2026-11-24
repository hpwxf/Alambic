//! The applets the module can run, the ones running, and which has the
//! focus.
//!
//! Up to [`MAX_INSTANCES`] applets run at once, the same kind more than once
//! if need be, each in a slot of [`AppHost`] with the outputs its route
//! gives it (journal/D-046). Every one is updated on every tick; only the
//! one with the focus reads the panel and draws. A slot holds an
//! [`Instance`], an enum of the applets this image composes, so a slot
//! costs the size of the largest, a few hundred bytes. Putting another app
//! in a slot starts it afresh: the residency of D-021, which kept every
//! applet's state across a glance elsewhere, gave way to running several;
//! saving the patch will bring a state back.
//!
//! This module also carries the contract every applet is written against —
//! the [`App`] trait, [`InputSnapshot`] and [`TickContext`] — so that one
//! applet never has to import another just to describe what it is fed, and
//! nothing outside [`AppHost`] has to name an applet to run it
//! (journal/D-043). Each applet itself lives in its own submodule
//! ([`diagnostic`], [`scope`], [`gfx_demo`], [`ratchet`], [`tone`],
//! [`supersaw`]).
//!
//! An applet's `update` returns what the outputs carry as [`Params`]: the
//! CV applets hold levels, `TONE` asks for tones and `SUPERSAW` for parts of
//! its bank, each through the generator of its kind that [`sound`] composes
//! and a [`Voice`](sound::Voice) renders (journal/D-033, D-044).

pub mod diagnostic;
pub mod gfx_demo;
pub mod param;
pub mod ratchet;
pub mod scope;
pub mod sound;
pub mod supersaw;
pub mod tone;

use core::any::Any;

use serde::{Deserialize, Serialize};

use crate::apps::diagnostic::DiagnosticApp;
use crate::apps::gfx_demo::GfxDemoApp;
use crate::apps::param::{ParamSpec, ParamValue, Refused};
use crate::apps::ratchet::RatchetApp;
use crate::apps::scope::ScopeApp;
use crate::apps::sound::{Params, Sound};
use crate::apps::supersaw::SupersawApp;
use crate::apps::tone::ToneApp;
use crate::audio::{Route, Routed};
use crate::buttons::ButtonEvents;
use crate::framebuffer::FrameBuffer;
use crate::modulation::Assign;
use crate::modulation::slots::slot_of;
use crate::platform::{
    BUTTONS, CV_CHANNELS, ControlEvents, CvChannel, ENCODERS, MilliVolts, TRIGGER_CHANNELS,
};

/// How many applets can run at once, each in a slot of its own that
/// takes some of the outputs (journal/D-046). Two is the practical case
/// measured first, four the limit tried next.
pub const MAX_INSTANCES: usize = 2;

/// One output an applet writes: a lane of its sound, which its route sends
/// to a jack or nowhere (journal/D-046).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Port {
    /// The name its routing page shows.
    pub name: &'static str,
}

/// Four ports named after the jacks they go to by default: an applet that
/// writes every output.
pub const PORTS_A_TO_D: [Port; CV_CHANNELS] = [
    Port { name: "A" },
    Port { name: "B" },
    Port { name: "C" },
    Port { name: "D" },
];

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
    /// The levels its lanes reached the outputs at on this tick, lane by
    /// lane: its own, unless something else holds an output; 0 V for a lane
    /// its route sends nowhere (journal/D-046).
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

    /// The outputs it writes, in the order of the lanes of the sound it
    /// publishes: port `i` is lane `i`, and a lane past the last port is
    /// never heard (journal/D-046). Every output by default.
    fn output_ports(&self) -> &'static [Port] {
        &PORTS_A_TO_D
    }

    /// Which of its ports reach a jack now, told whenever its route
    /// changes: an applet may stop preparing a lane nobody hears
    /// (`SUPERSAW`'s `SUB`). Ignored by default; the lane then plays into
    /// nothing (journal/D-046).
    fn set_routed(&mut self, _routed: [bool; CV_CHANNELS]) {}

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

    /// The input of each assignable parameter, in their order
    /// ([`slot_of`]): what `assign` and `set_assign` on `dyn App` reach
    /// (journal/D-045).
    fn assigns(&self) -> &[Assign] {
        &[]
    }

    /// The same inputs, to change.
    fn assigns_mut(&mut self) -> &mut [Assign] {
        &mut []
    }

    /// The signal that sweeps parameter `index` end to end when it follows
    /// its input (journal/D-036), from which a host fits an input to it as
    /// `CENTER` does; `None` for one that takes no input.
    fn assign_sweep(&self, _index: usize) -> Option<(MilliVolts, MilliVolts)> {
        None
    }
}

// Once for every applet rather than in each one's table: what an applet
// gives of its inputs is the slice and the specs, the rest is common.
impl dyn App {
    /// The input parameter `index` takes, `None` for one that takes none.
    #[must_use]
    pub fn assign(&self, index: usize) -> Option<Assign> {
        let slot = slot_of(self.param_specs(), index)?;
        self.assigns().get(slot).copied()
    }

    /// Gives parameter `index` the input `assign`, as the submenu could
    /// have: what it applied.
    ///
    /// # Errors
    ///
    /// [`Refused::NoSuch`] for a parameter that takes no input,
    /// [`Refused::OutOfRange`] for a value the submenu stops short of
    /// ([`Assign::check`]).
    pub fn set_assign(&mut self, index: usize, assign: Assign) -> Result<Assign, Refused> {
        let slot = slot_of(self.param_specs(), index).ok_or(Refused::NoSuch)?;
        let assign = assign.check()?;
        *self.assigns_mut().get_mut(slot).ok_or(Refused::NoSuch)? = assign;
        Ok(assign)
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

/// One applet in a slot, of whichever kind this image composes: what the
/// pool stores, the size of the largest (journal/D-046).
// No heap to box the larger applets into: the size of the largest is the
// cost of a slot, a few hundred bytes.
#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone)]
pub enum Instance {
    /// See [`AppId::Diagnostic`].
    Diagnostic(DiagnosticApp),
    /// See [`AppId::Scope`].
    Scope(ScopeApp),
    /// See [`AppId::Gfx`].
    Gfx(GfxDemoApp),
    /// See [`AppId::Ratchet`].
    Ratchet(RatchetApp),
    /// See [`AppId::Tone`].
    Tone(ToneApp),
    /// See [`AppId::Supersaw`].
    Supersaw(SupersawApp),
}

impl Instance {
    /// A fresh applet of kind `id`, as it starts.
    // One copy for every caller: inlined, each would build all six applets.
    #[inline(never)]
    #[must_use]
    pub fn new(id: AppId) -> Self {
        match id {
            AppId::Diagnostic => Self::Diagnostic(DiagnosticApp::new()),
            AppId::Scope => Self::Scope(ScopeApp::new()),
            AppId::Gfx => Self::Gfx(GfxDemoApp::new()),
            AppId::Ratchet => Self::Ratchet(RatchetApp::new()),
            AppId::Tone => Self::Tone(ToneApp::new()),
            AppId::Supersaw => Self::Supersaw(SupersawApp::new()),
        }
    }

    /// The applet, through the trait the engine runs it by.
    // One copy: inlined in a loop over the slots, the six arms repeat.
    #[inline(never)]
    #[must_use]
    pub fn app(&self) -> &dyn App {
        match self {
            Self::Diagnostic(app) => app,
            Self::Scope(app) => app,
            Self::Gfx(app) => app,
            Self::Ratchet(app) => app,
            Self::Tone(app) => app,
            Self::Supersaw(app) => app,
        }
    }

    /// The applet, to change it.
    #[inline(never)]
    pub fn app_mut(&mut self) -> &mut dyn App {
        match self {
            Self::Diagnostic(app) => app,
            Self::Scope(app) => app,
            Self::Gfx(app) => app,
            Self::Ratchet(app) => app,
            Self::Tone(app) => app,
            Self::Supersaw(app) => app,
        }
    }
}

/// An instance and where its lanes go.
#[derive(Debug, Clone)]
struct Slot {
    instance: Instance,
    route: Route,
}

impl Slot {
    /// `instance` along `route`, which it is told.
    fn new(instance: Instance, route: Route) -> Self {
        let mut slot = Self { instance, route };
        slot.tell_route();
        slot
    }

    /// Tells the applet which of its ports its route sends somewhere.
    fn tell_route(&mut self) {
        let route = self.route;
        let routed = core::array::from_fn(|lane| route.output(lane).is_some());
        self.instance.app_mut().set_routed(routed);
    }
}

/// The applets running, one per slot, up to [`MAX_INSTANCES`], the same
/// kind more than once if need be, and the one the panel and the screen
/// belong to (journal/D-046).
///
/// Every instance runs on every tick and keeps its outputs; only the one
/// with the focus reads the panel and draws. An instance's place on the
/// panel, left to right, is its *position*; where it is stored, its
/// *slot*, never moves while it runs, so its voice keeps its state when
/// another is added or removed beside it.
#[derive(Debug, Clone)]
pub struct AppHost {
    slots: [Option<Slot>; MAX_INSTANCES],
    /// Slots in panel order; the first `count` are in use.
    order: [usize; MAX_INSTANCES],
    count: usize,
    /// Position of the instance with the focus.
    focus: usize,
}

impl AppHost {
    const EMPTY: Option<Slot> = None;

    /// The diagnostic applet alone, every output its own.
    #[must_use]
    pub fn new() -> Self {
        let mut host = Self {
            slots: [Self::EMPTY; MAX_INSTANCES],
            order: [0; MAX_INSTANCES],
            count: 0,
            focus: 0,
        };
        host.add(AppId::Diagnostic);
        host
    }

    /// The kind of applet with the focus.
    #[must_use]
    pub fn current(&self) -> AppId {
        self.focused().id()
    }

    /// How many instances run.
    #[must_use]
    pub const fn len(&self) -> usize {
        self.count
    }

    /// Whether none runs, which never lasts: there is always one.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.count == 0
    }

    /// The position, left to right, of the instance with the focus.
    #[must_use]
    pub const fn focus(&self) -> usize {
        self.focus
    }

    /// Gives the panel and the screen to the instance at `position`;
    /// `false`, and nothing changes, past the last.
    pub const fn set_focus(&mut self, position: usize) -> bool {
        if position < self.count {
            self.focus = position;
            true
        } else {
            false
        }
    }

    /// The slot of the instance at `position`.
    const fn slot_at(&self, position: usize) -> Option<usize> {
        if position < self.count {
            Some(self.order[position])
        } else {
            None
        }
    }

    /// The slot of the instance with the focus: where its lanes sit in the
    /// patch.
    #[must_use]
    pub const fn focused_slot(&self) -> usize {
        self.order[self.focus]
    }

    /// The instance at `position`, if any.
    #[must_use]
    pub fn at(&self, position: usize) -> Option<&dyn App> {
        let slot = self.slot_at(position)?;
        self.slots[slot].as_ref().map(|slot| slot.instance.app())
    }

    /// The instance at `position`, to change it.
    pub fn at_mut(&mut self, position: usize) -> Option<&mut dyn App> {
        let slot = self.slot_at(position)?;
        self.slots[slot]
            .as_mut()
            .map(|slot| slot.instance.app_mut())
    }

    /// The route of the instance at `position`, if any.
    #[must_use]
    pub fn route_at(&self, position: usize) -> Option<Route> {
        let slot = self.slot_at(position)?;
        self.slots[slot].as_ref().map(|slot| slot.route)
    }

    /// The instance with the focus.
    ///
    /// # Panics
    ///
    /// Never: there is always one.
    #[must_use]
    pub fn focused(&self) -> &dyn App {
        self.at(self.focus)
            .expect("an instance always has the focus")
    }

    /// The instance with the focus, to change it.
    ///
    /// # Panics
    ///
    /// Never: there is always one.
    pub fn focused_mut(&mut self) -> &mut dyn App {
        self.at_mut(self.focus)
            .expect("an instance always has the focus")
    }

    /// Where the lanes of the instance with the focus go.
    #[must_use]
    pub fn route(&self) -> Route {
        self.route_at(self.focus).unwrap_or(Route::OFF)
    }

    /// The first instance of kind `id`, left to right, if one runs.
    #[must_use]
    pub fn get(&self, id: AppId) -> Option<&dyn App> {
        (0..self.count)
            .filter_map(|position| self.at(position))
            .find(|app| app.id() == id)
    }

    /// The first instance of kind `id`, to change it.
    pub fn get_mut(&mut self, id: AppId) -> Option<&mut dyn App> {
        let position = (0..self.count)
            .find(|&position| self.at(position).is_some_and(|app| app.id() == id))?;
        self.at_mut(position)
    }

    /// The first instance of type `T`, left to right, if one runs.
    ///
    /// For tests and the simulator, which look at an applet's own state; the
    /// engine never needs the concrete type.
    #[must_use]
    pub fn find<T: App>(&self) -> Option<&T> {
        (0..self.count).find_map(|position| {
            let app: &dyn Any = self.at(position)?;
            app.downcast_ref::<T>()
        })
    }

    /// Puts a fresh applet of kind `id` in the place of the one with the
    /// focus, unless it already is of that kind: idempotent. The new one
    /// takes the outputs the old one held, port by port, then the free
    /// ones.
    pub fn select(&mut self, id: AppId) {
        if self.current() == id {
            return;
        }
        let slot = self.focused_slot();
        self.slots[slot] = None;
        let instance = Instance::new(id);
        let route = self.free_route(instance.app().output_ports().len());
        self.slots[slot] = Some(Slot::new(instance, route));
    }

    /// Starts a fresh applet of kind `id` right of the one with the focus,
    /// and gives it the focus: its position, or `None` when every slot is
    /// taken. Its ports take the free outputs in order; one left without
    /// goes nowhere until routed.
    pub fn add(&mut self, id: AppId) -> Option<usize> {
        let slot = self.slots.iter().position(Option::is_none)?;
        let instance = Instance::new(id);
        let route = self.free_route(instance.app().output_ports().len());
        self.slots[slot] = Some(Slot::new(instance, route));
        let position = if self.count == 0 { 0 } else { self.focus + 1 };
        let mut at = self.count;
        while at > position {
            self.order[at] = self.order[at - 1];
            at -= 1;
        }
        self.order[position] = slot;
        self.count += 1;
        self.focus = position;
        Some(position)
    }

    /// Stops the instance with the focus, freeing its outputs, and gives
    /// the focus to its left neighbour (its right one at the left end);
    /// `false`, and nothing changes, for the last instance.
    pub fn remove(&mut self) -> bool {
        if self.count <= 1 {
            return false;
        }
        let slot = self.focused_slot();
        self.slots[slot] = None;
        for at in self.focus..self.count - 1 {
            self.order[at] = self.order[at + 1];
        }
        self.count -= 1;
        self.focus = self.focus.saturating_sub(1);
        true
    }

    /// Sends port `port` of the instance at `position` to `output`, or
    /// nowhere: the route it now has.
    ///
    /// # Errors
    ///
    /// [`Refused::NoSuch`] for no such instance or port,
    /// [`Refused::Taken`] for an output another port already reaches, of
    /// this instance or another: outputs are never shared.
    pub fn set_route(
        &mut self,
        position: usize,
        port: usize,
        output: Option<CvChannel>,
    ) -> Result<Route, Refused> {
        let slot = self.slot_at(position).ok_or(Refused::NoSuch)?;
        let ports = self.at(position).map_or(0, |app| app.output_ports().len());
        if port >= ports {
            return Err(Refused::NoSuch);
        }
        if let Some(output) = output
            && self
                .holder(output)
                .is_some_and(|holder| holder != (slot, port))
        {
            return Err(Refused::Taken);
        }
        let held = self.slots[slot].as_mut().ok_or(Refused::NoSuch)?;
        held.route.set(port, output);
        held.tell_route();
        Ok(held.route)
    }

    /// The slot and port that reach `output`, if any.
    fn holder(&self, output: CvChannel) -> Option<(usize, usize)> {
        for (slot, held) in self.slots.iter().enumerate() {
            if let Some(held) = held {
                for lane in 0..CV_CHANNELS {
                    if held.route.output(lane) == Some(output) {
                        return Some((slot, lane));
                    }
                }
            }
        }
        None
    }

    /// A route for `ports` ports over the outputs no instance reaches, in
    /// order: every port to the jack of its rank while one applet runs.
    fn free_route(&self, ports: usize) -> Route {
        let mut taken = [false; CV_CHANNELS];
        for held in self.slots.iter().flatten() {
            for lane in 0..CV_CHANNELS {
                if let Some(output) = held.route.output(lane) {
                    taken[output.index()] = true;
                }
            }
        }
        let mut route = Route::OFF;
        let mut port = 0;
        for output in CvChannel::ALL {
            if port < ports && !taken[output.index()] {
                route.set(port, Some(output));
                port += 1;
            }
        }
        route
    }

    /// Runs one tick of every instance: what each slot's lanes carry and
    /// where they go. Only the one with the focus sees the panel.
    pub fn update(&mut self, input: &InputSnapshot) -> [Routed<Sound>; MAX_INSTANCES] {
        let mut quiet = *input;
        quiet.silence_controls();
        let focused = self.focused_slot();
        let mut parts = [Routed::OFF; MAX_INSTANCES];
        for (index, (slot, part)) in self.slots.iter_mut().zip(&mut parts).enumerate() {
            if let Some(slot) = slot {
                let input = if index == focused { input } else { &quiet };
                *part = Routed {
                    params: slot.instance.app_mut().update(input),
                    route: slot.route,
                };
            }
        }
        parts
    }

    /// Draws the screen of the instance with the focus.
    pub fn render(&self, frame: &mut FrameBuffer, context: &TickContext) {
        self.focused().render(frame, context);
    }
}

impl Default for AppHost {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::diagnostic::DiagnosticApp;
    use super::param::Refused;
    use super::scope::ScopeApp;
    use super::{App, AppHost, AppId, InputSnapshot, Instance};
    use crate::apps::sound::Params;
    use crate::audio::Route;
    use crate::audio::basic::Channel;
    use crate::platform::{CV_CHANNELS, CvChannel, MilliVolts};

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
    fn every_instance_is_the_applet_that_names_it() {
        for id in AppId::ALL {
            let instance = Instance::new(id);
            assert_eq!(instance.app().id(), id);
            assert_eq!(instance.app().name(), id.name());
        }
    }

    #[test]
    fn a_fresh_host_runs_the_diagnostic_applet_alone() {
        let host = AppHost::new();
        assert_eq!(host.current(), AppId::Diagnostic);
        assert_eq!((host.len(), host.focus()), (1, 0));
        assert_eq!(host.route(), Route::IDENTITY, "every output its own");
        assert!(host.get(AppId::Scope).is_none(), "only what runs");
    }

    #[test]
    fn the_running_app_decides_what_the_outputs_carry() {
        let mut host = AppHost::new();
        let diagnostic = host.update(&snapshot(1_000, 2_000))[0].params;
        assert_eq!(
            diagnostic.as_basic().unwrap().channels[1],
            Channel::Level(2_000),
            "the diagnostic applet mirrors channel by channel"
        );

        host.select(AppId::Scope);
        let scope = host.update(&snapshot(1_000, 2_000))[0].params;
        assert_eq!(
            scope,
            Params::levels([1_000; CV_CHANNELS]),
            "the scope buffers CV1 to every output, which no output mode can do"
        );
    }

    #[test]
    fn selecting_the_same_kind_keeps_it_another_starts_afresh() {
        let mut host = AppHost::new();
        host.select(AppId::Scope);
        host.update(&snapshot(1_000, 0));
        host.select(AppId::Scope);
        assert_eq!(
            host.find::<ScopeApp>().unwrap().level(),
            1_000,
            "idempotent"
        );

        host.select(AppId::Diagnostic);
        host.select(AppId::Scope);
        assert_eq!(
            host.find::<ScopeApp>().unwrap().level(),
            0,
            "a new instance, not the one left (journal/D-046)"
        );
        assert!(host.find::<DiagnosticApp>().is_none());
    }

    #[test]
    fn a_new_instance_takes_the_free_outputs_and_the_focus() {
        let mut host = AppHost::new();
        host.select(AppId::Ratchet);
        assert_eq!(host.route(), Route::first(2), "A and B");
        assert_eq!(host.add(AppId::Ratchet), Some(1));
        assert_eq!((host.len(), host.focus()), (2, 1));
        let mut second = Route::OFF;
        second.set(0, Some(CvChannel::Three));
        second.set(1, Some(CvChannel::Four));
        assert_eq!(host.route(), second, "C and D");
        assert_eq!(host.add(AppId::Tone), None, "every slot taken");
    }

    #[test]
    fn every_instance_runs_and_only_the_focused_one_sees_the_panel() {
        let mut host = AppHost::new();
        host.select(AppId::Scope);
        host.add(AppId::Diagnostic);
        let mut input = snapshot(1_500, 0);
        input.controls.encoder_delta = [3, 0];
        let parts = host.update(&input);
        assert_eq!(
            parts[0].params,
            Params::levels([1_500; CV_CHANNELS]),
            "the scope runs without the focus"
        );
        assert!(
            parts[1].route.is_off(),
            "the scope holds every output: none left for the second"
        );
        assert!(host.set_focus(0));
        assert_eq!(host.current(), AppId::Scope);
        assert!(!host.set_focus(2));
    }

    #[test]
    fn an_output_is_never_shared() {
        let mut host = AppHost::new();
        host.select(AppId::Ratchet);
        host.add(AppId::Ratchet);
        assert_eq!(
            host.set_route(1, 0, Some(CvChannel::One)),
            Err(Refused::Taken),
            "A belongs to the first"
        );
        assert_eq!(
            host.set_route(1, 2, None),
            Err(Refused::NoSuch),
            "two ports"
        );
        assert!(host.set_route(0, 1, None).is_ok(), "the first frees B");
        let route = host.set_route(1, 0, Some(CvChannel::Two)).unwrap();
        assert_eq!(route.output(0), Some(CvChannel::Two));
        assert_eq!(
            host.set_route(1, 0, Some(CvChannel::Two)),
            Ok(route),
            "a port may keep its own output"
        );
    }

    #[test]
    fn removing_frees_the_outputs_and_moves_the_focus_left() {
        let mut host = AppHost::new();
        host.select(AppId::Ratchet);
        assert!(!host.remove(), "the last instance stays");
        host.add(AppId::Tone);
        assert_eq!(host.route().output(0), Some(CvChannel::Three));
        assert!(host.remove());
        assert_eq!(
            (host.len(), host.focus(), host.current()),
            (1, 0, AppId::Ratchet)
        );
        host.add(AppId::Tone);
        assert_eq!(
            host.route().output(0),
            Some(CvChannel::Three),
            "C and D again: A and B stay RATCHET's"
        );
    }

    #[test]
    fn an_instance_keeps_its_slot_when_another_comes_left_of_it() {
        let mut host = AppHost::new();
        host.select(AppId::Ratchet);
        host.add(AppId::Tone);
        assert!(host.set_focus(0));
        host.remove();
        host.add(AppId::Scope);
        // TONE stays in slot 1, where its voice is; SCOPE took slot 0.
        assert_eq!(host.at(0).map(App::id), Some(AppId::Tone));
        assert_eq!(host.at(1).map(App::id), Some(AppId::Scope));
        assert_eq!(host.focused_slot(), 0);
    }

    #[test]
    fn a_port_routed_nowhere_is_not_prepared() {
        let mut host = AppHost::new();
        host.select(AppId::Supersaw);
        let parts = |host: &mut AppHost| {
            host.update(&snapshot(0, 0))[0]
                .params
                .as_supersaw()
                .unwrap()
                .parts
        };
        assert!(parts(&mut host)[2].is_some(), "SUB on C");
        host.set_route(0, 2, None).unwrap();
        assert!(parts(&mut host)[2].is_none(), "SUB off: not prepared");
        host.add(AppId::Ratchet);
        assert_eq!(
            (host.route().output(0), host.route().output(1)),
            (Some(CvChannel::Three), Some(CvChannel::Four)),
            "RATCHET takes the jacks SUPERSAW left"
        );
    }
}
