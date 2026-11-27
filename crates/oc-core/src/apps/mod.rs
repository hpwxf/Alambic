//! The applets the module can run, the ones running, and which has the
//! focus.
//!
//! [`MAX_INSTANCES`] applets always run, the same kind more than once if
//! need be, each in a slot of [`AppHost`] with the outputs its route gives
//! it; an empty slot runs [`nothing`] (journal/D-046). Every one is updated on every tick; only the
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
//! [`supersaw`], [`melody`]).
//!
//! An applet's `update` returns what the outputs carry as [`Params`]: the
//! CV applets hold levels, `TONE` asks for tones and `SUPERSAW` for parts of
//! its bank, each through the generator of its kind that [`sound`] composes
//! and a backend renders ([`crate::playback`]) (journal/D-033, D-044).

pub mod bus;
pub mod diagnostic;
pub mod gfx_demo;
pub mod melody;
pub mod nothing;
pub mod param;
pub mod ratchet;
pub mod scope;
pub mod sound;
pub mod supersaw;
pub mod tone;

use core::any::Any;

use serde::{Deserialize, Serialize};

pub use crate::apps::bus::Bus;
use crate::apps::diagnostic::DiagnosticApp;
use crate::apps::gfx_demo::GfxDemoApp;
use crate::apps::melody::MelodyApp;
use crate::apps::nothing::NothingApp;
use crate::apps::param::{ParamSpec, ParamValue, Refused};
use crate::apps::ratchet::RatchetApp;
use crate::apps::scope::ScopeApp;
use crate::apps::sound::{Params, Sound};
use crate::apps::supersaw::SupersawApp;
use crate::apps::tone::ToneApp;
use crate::audio::basic::Channel;
use crate::audio::{Route, Routed};
use crate::buttons::ButtonEvents;
use crate::framebuffer::FrameBuffer;
use crate::modulation::Assign;
use crate::modulation::slots::slot_of;
use crate::platform::{
    BUTTONS, CV_CHANNELS, ControlEvents, CvChannel, ENCODERS, MilliVolts, TRIGGER_CHANNELS,
};

/// How many slots there are, each running an applet, [`AppId::Nothing`]
/// when empty, that takes some of the outputs (journal/D-046): four, one
/// per output at most.
pub const MAX_INSTANCES: usize = 4;

/// What an output port carries (journal/D-048).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PortSignal {
    /// A level, held from one tick to the next: a pitch, a gate, a
    /// modulation. Another instance's assignment can read it.
    Cv,
    /// A sound, rendered at the audio rate: nothing for a control to read.
    Audio,
}

/// One output an applet writes: a lane of its sound, which its route sends
/// to a jack or nowhere (journal/D-046).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Port {
    /// The name its routing page shows.
    pub name: &'static str,
    /// What it carries.
    pub signal: PortSignal,
}

impl Port {
    /// A port named `name` that carries a level.
    #[must_use]
    pub const fn cv(name: &'static str) -> Self {
        Self {
            name,
            signal: PortSignal::Cv,
        }
    }

    /// A port named `name` that carries a sound.
    #[must_use]
    pub const fn audio(name: &'static str) -> Self {
        Self {
            name,
            signal: PortSignal::Audio,
        }
    }
}

/// Four ports named after the jacks they go to by default, each a level: an
/// applet that writes every output.
pub const PORTS_A_TO_D: [Port; CV_CHANNELS] =
    [Port::cv("A"), Port::cv("B"), Port::cv("C"), Port::cv("D")];

/// What kind of jack an input port reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputKind {
    /// A CV input, `CV1` to `CV4`.
    Cv,
    /// A trigger input, `TR1` to `TR4`.
    Trigger,
}

/// One input an applet reads by itself (not through a CV assignment): a
/// lane of its [`InputSnapshot`], which its input map plugs into a jack of
/// that kind (journal/D-046).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InputPort {
    /// The name its routing page shows.
    pub name: &'static str,
    /// The jacks it can read.
    pub kind: InputKind,
    /// The lane it reads: `cv[lane]` or `triggers[lane]`.
    pub lane: usize,
}

/// The level from which a trigger input that reads a port of another
/// instance is high: 1 V, below any gate or trigger an applet writes (5 V),
/// about where the module's input transistors switch (journal/D-048).
pub const INTERNAL_TRIGGER_MV: MilliVolts = 1_000;

/// What an input port reads (journal/D-046, D-048): a jack of its kind, by
/// its index (0 for `CV1` or `TR1`), output port `port` of the instance in
/// slot `slot`, which must carry a level and belong to another instance, or
/// nothing: `OFF`, so that a port the patch does not use (a `RESET`) leaves
/// every jack to the others.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum InputSource {
    /// A jack of the port's kind, from 0.
    Jack(u8),
    /// An output port of another instance, by its slot and rank, from 0.
    Lane {
        /// The instance's slot, from 0.
        slot: u8,
        /// The port's rank among the instance's output ports.
        port: u8,
    },
    /// Nothing: a CV input at 0 V, a trigger input low.
    Off,
}

impl InputSource {
    /// Jack `jack`, `None` past the last.
    #[must_use]
    pub fn jack(jack: usize) -> Option<Self> {
        (jack < CV_CHANNELS)
            .then(|| u8::try_from(jack).ok().map(Self::Jack))
            .flatten()
    }

    /// Port `port` of the instance in slot `slot`, `None` past the last slot
    /// or lane.
    #[must_use]
    pub fn lane(slot: usize, port: usize) -> Option<Self> {
        (slot < MAX_INSTANCES && port < CV_CHANNELS).then(|| Self::Lane {
            slot: u8::try_from(slot).unwrap_or(0),
            port: u8::try_from(port).unwrap_or(0),
        })
    }

    /// The level it carries on `input`: a CV jack's, or what the port
    /// published on the last tick, 0 V for one not offered.
    fn level(self, input: &InputSnapshot) -> MilliVolts {
        match self {
            Self::Jack(jack) => input.cv.get(usize::from(jack)).copied().unwrap_or(0),
            Self::Lane { slot, port } => input.bus.level(usize::from(slot), usize::from(port)),
            Self::Off => 0,
        }
    }

    /// Whether it is high on `input`: a trigger jack's state, or a port's
    /// level from [`INTERNAL_TRIGGER_MV`].
    fn high(self, input: &InputSnapshot) -> bool {
        match self {
            Self::Jack(jack) => input
                .triggers
                .get(usize::from(jack))
                .copied()
                .unwrap_or(false),
            Self::Lane { .. } => self.level(input) >= INTERNAL_TRIGGER_MV,
            Self::Off => false,
        }
    }
}

/// What each lane of an instance's [`InputSnapshot`] reads: a jack, every
/// lane its own by default (journal/D-046), or a port of another instance
/// (journal/D-048).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InputRoute {
    cv: [InputSource; CV_CHANNELS],
    triggers: [InputSource; TRIGGER_CHANNELS],
}

/// Every lane on the jack of its rank.
const OWN_JACKS: [InputSource; CV_CHANNELS] = [
    InputSource::Jack(0),
    InputSource::Jack(1),
    InputSource::Jack(2),
    InputSource::Jack(3),
];

impl InputRoute {
    /// Every lane reads the jack of its rank.
    pub const IDENTITY: Self = Self {
        cv: OWN_JACKS,
        triggers: OWN_JACKS,
    };

    /// What lane `lane` of `kind` reads.
    #[must_use]
    pub fn source(&self, kind: InputKind, lane: usize) -> Option<InputSource> {
        match kind {
            InputKind::Cv => self.cv.get(lane).copied(),
            InputKind::Trigger => self.triggers.get(lane).copied(),
        }
    }

    /// The jack lane `lane` of `kind` reads, as its index; `None` for a
    /// port of another instance, or nothing.
    #[must_use]
    pub fn jack(&self, kind: InputKind, lane: usize) -> Option<usize> {
        match self.source(kind, lane)? {
            InputSource::Jack(jack) => Some(usize::from(jack)),
            InputSource::Lane { .. } | InputSource::Off => None,
        }
    }

    /// Plugs lane `lane` of `kind` into `source`; `false`, and nothing
    /// changes, for a lane, a jack or a port past the last. Whether the
    /// port may be read is the caller's to check.
    pub fn set(&mut self, kind: InputKind, lane: usize, source: InputSource) -> bool {
        let valid = match source {
            InputSource::Jack(jack) => usize::from(jack) < CV_CHANNELS,
            InputSource::Lane { slot, port } => {
                InputSource::lane(usize::from(slot), usize::from(port)).is_some()
            }
            InputSource::Off => true,
        };
        let lanes = match kind {
            InputKind::Cv => &mut self.cv,
            InputKind::Trigger => &mut self.triggers,
        };
        match lanes.get_mut(lane) {
            Some(plugged) if valid => {
                *plugged = source;
                true
            }
            _ => false,
        }
    }

    /// `input` as an instance with this map sees it, its bus already the
    /// instance's.
    #[must_use]
    pub fn apply(&self, input: &InputSnapshot) -> InputSnapshot {
        let mut seen = *input;
        seen.cv = self.cv.map(|source| source.level(input));
        seen.triggers = self.triggers.map(|source| source.high(input));
        seen
    }
}

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
    /// What the other instances published on the last tick, for an
    /// assignment that reads one of their ports (journal/D-048).
    pub bus: Bus,
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
            bus: Bus::EMPTY,
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
    /// never heard (journal/D-046). Every output by default, each a level.
    /// A [`PortSignal::Cv`] port publishes a level whether or not its route
    /// sends it anywhere: another instance may read it (journal/D-048).
    fn output_ports(&self) -> &'static [Port] {
        &PORTS_A_TO_D
    }

    /// The inputs it reads by itself, which its routing page can plug into
    /// other jacks: none by default. An applet whose inputs go through CV
    /// assignments declares none, for an assignment names the jack it reads.
    fn input_ports(&self) -> &'static [InputPort] {
        &[]
    }

    /// Which of its ports reach a jack now, told whenever its route
    /// changes: an applet may stop preparing an audio lane nobody hears
    /// (`SUPERSAW`'s `SUB`), never a level another instance may read. Ignored by default; the lane then plays into
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
    /// An empty slot: no port, no setting (journal/D-046, revision of
    /// 2026-11-25). Last in the menu, which sorts by category, and puts it
    /// in a slot to empty it.
    Nothing,
    /// A looping melody of random but controllable notes on A, a chord of
    /// two tones on B and C, a gate on D (journal/D-047). After [`Self::Nothing`]: an index
    /// once persisted never changes.
    Melody,
}

impl AppId {
    /// Every app, in the order the menu lists them.
    pub const ALL: [Self; 8] = [
        Self::Diagnostic,
        Self::Scope,
        Self::Gfx,
        Self::Ratchet,
        Self::Tone,
        Self::Supersaw,
        Self::Nothing,
        Self::Melody,
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
            Self::Nothing => "NOTHING",
            Self::Melody => "MELODY",
        }
    }

    /// The column of the app menu it is listed under.
    #[must_use]
    pub const fn category(self) -> Category {
        match self {
            Self::Diagnostic | Self::Scope | Self::Gfx => Category::Tools,
            Self::Ratchet => Category::Triggers,
            Self::Tone | Self::Supersaw => Category::Sound,
            Self::Melody => Category::Sequence,
            Self::Nothing => Category::System,
        }
    }
}

/// A group of the app menu, which lists the categories on the left and the
/// apps of the highlighted one on the right.
///
/// A property of the app, not of the menu, so that an app added to the image
/// says where it goes where it is declared.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Category {
    /// What helps look at the module and its signals: the boot applet, so
    /// listed first, which keeps the menu's order that of [`AppId::ALL`].
    Tools,
    /// Applets that read and write triggers.
    Triggers,
    /// Oscillators.
    Sound,
    /// Applets that play notes in turn on a clock.
    Sequence,
    /// What is not an applet that makes or reads a signal: an empty slot and
    /// the system settings.
    System,
}

impl Category {
    /// Every category, in the order the menu lists them.
    pub const ALL: [Self; 5] = [
        Self::Tools,
        Self::Triggers,
        Self::Sound,
        Self::Sequence,
        Self::System,
    ];

    /// Zero-based index of this category.
    #[must_use]
    pub const fn index(self) -> usize {
        self as usize
    }

    /// Name shown in the left column of the app menu.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Tools => "TOOLS",
            Self::Triggers => "TRIGGERS",
            Self::Sound => "SOUND",
            Self::Sequence => "SEQUENCE",
            Self::System => "SYSTEM",
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
    /// See [`AppId::Nothing`].
    Nothing(NothingApp),
    /// See [`AppId::Melody`].
    Melody(MelodyApp),
}

impl Instance {
    /// A fresh applet of kind `id`, as it starts.
    // One copy for every caller: inlined, each would build all six applets.
    //
    // Each arm builds its applet as a temporary, then moves it into the
    // instance, and the instance into the caller's: a `const fn` called here
    // is an ordinary call, which Rust does not promise to build in place.
    // Those copies take stack, on the firmware's deepest path (an applet
    // chosen by the probe, from the loop): 1,240 bytes in this frame, about
    // the temporaries of every applet side by side, `MELODY`'s 432 the
    // largest. That is below the critical level (`cargo xtask size`, about
    // 6 KiB of the 16 KiB stack left); should it not be, a
    // `const { .. }` arm, or a constant then drawn through a reference for
    // what runs at start, took 600 bytes off when tried
    // (docs/plan/avancement/etape-14.md, journal/D-038).
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
            AppId::Nothing => Self::Nothing(NothingApp::new()),
            AppId::Melody => Self::Melody(MelodyApp::new()),
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
            Self::Nothing(app) => app,
            Self::Melody(app) => app,
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
            Self::Nothing(app) => app,
            Self::Melody(app) => app,
        }
    }
}

/// An instance and where its lanes go.
#[derive(Debug, Clone)]
struct Slot {
    instance: Instance,
    route: Route,
    inputs: InputRoute,
}

impl Slot {
    /// An empty slot: `NOTHING`, going nowhere. A constant, so that a pool
    /// of them is laid out in place rather than built a slot at a time on
    /// the stack (journal/D-038).
    const EMPTY: Self = Self {
        instance: Instance::Nothing(NothingApp::new()),
        route: Route::OFF,
        inputs: InputRoute::IDENTITY,
    };

    /// `instance` along `route`, which it is told, every input its own.
    fn new(instance: Instance, route: Route) -> Self {
        let mut slot = Self {
            instance,
            route,
            inputs: InputRoute::IDENTITY,
        };
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

/// The applets running, one in each of the [`MAX_INSTANCES`] slots, the
/// same kind more than once if need be, and the one the panel and the
/// screen belong to (journal/D-046).
///
/// Every slot always holds an applet: one nobody uses holds
/// [`AppId::Nothing`], which has no port and takes no output (revision of
/// 2026-11-25). A slot's place on the panel, left to right, is its
/// *position*, the same as where it is stored: replacing the applet in one
/// leaves the others where they are, their voices keeping their state.
/// Every instance runs on every tick and keeps its outputs; only the one
/// with the focus reads the panel and draws.
#[derive(Debug, Clone)]
pub struct AppHost {
    slots: [Slot; MAX_INSTANCES],
    /// Position of the instance with the focus.
    focus: usize,
    /// The level each lane of each slot published on the last tick, for
    /// the others to read (journal/D-048).
    levels: [[MilliVolts; CV_CHANNELS]; MAX_INSTANCES],
}

impl AppHost {
    /// Every slot empty, the focus on the first: a constant, laid out in
    /// place, for an engine built where no applet may be chosen yet
    /// ([`crate::engine::Engine::without_apps`]).
    pub const EMPTY: Self = Self {
        slots: [Slot::EMPTY; MAX_INSTANCES],
        focus: 0,
        levels: [[0; CV_CHANNELS]; MAX_INSTANCES],
    };

    /// The diagnostic applet in the first slot, every output its own, the
    /// others empty.
    #[must_use]
    pub fn new() -> Self {
        let mut host = Self::EMPTY;
        host.select(AppId::Diagnostic);
        host
    }

    /// The kind of applet with the focus.
    #[must_use]
    pub fn current(&self) -> AppId {
        self.focused().id()
    }

    /// Whether each slot runs an applet, left to right, rather than
    /// [`AppId::Nothing`].
    #[must_use]
    pub fn occupied(&self) -> [bool; MAX_INSTANCES] {
        core::array::from_fn(|position| self.slots[position].instance.app().id() != AppId::Nothing)
    }

    /// The position, left to right, of the instance with the focus.
    #[must_use]
    pub const fn focus(&self) -> usize {
        self.focus
    }

    /// Gives the panel and the screen to the instance at `position`;
    /// `false`, and nothing changes, past the last slot.
    pub const fn set_focus(&mut self, position: usize) -> bool {
        if position < MAX_INSTANCES {
            self.focus = position;
            true
        } else {
            false
        }
    }

    /// Moves the focus to the next slot, `forward` to the right, from the
    /// last round to the first and back; empty slots too.
    pub const fn step_focus(&mut self, forward: bool) {
        self.focus = if forward {
            (self.focus + 1) % MAX_INSTANCES
        } else {
            (self.focus + MAX_INSTANCES - 1) % MAX_INSTANCES
        };
    }

    /// The slot of the instance with the focus: where its lanes sit in the
    /// patch, its position.
    #[must_use]
    pub const fn focused_slot(&self) -> usize {
        self.focus
    }

    /// The instance at `position`, if there is such a slot.
    #[must_use]
    pub fn at(&self, position: usize) -> Option<&dyn App> {
        self.slots.get(position).map(|slot| slot.instance.app())
    }

    /// The instance at `position`, to change it.
    pub fn at_mut(&mut self, position: usize) -> Option<&mut dyn App> {
        self.slots
            .get_mut(position)
            .map(|slot| slot.instance.app_mut())
    }

    /// The route of the instance at `position`, if there is such a slot.
    #[must_use]
    pub fn route_at(&self, position: usize) -> Option<Route> {
        self.slots.get(position).map(|slot| slot.route)
    }

    /// The instance with the focus.
    #[must_use]
    pub fn focused(&self) -> &dyn App {
        self.slots[self.focus].instance.app()
    }

    /// The instance with the focus, to change it.
    pub fn focused_mut(&mut self) -> &mut dyn App {
        self.slots[self.focus].instance.app_mut()
    }

    /// Where the lanes of the instance with the focus go.
    #[must_use]
    pub fn route(&self) -> Route {
        self.slots[self.focus].route
    }

    /// The first instance of kind `id`, left to right, if one runs.
    #[must_use]
    pub fn get(&self, id: AppId) -> Option<&dyn App> {
        self.slots
            .iter()
            .map(|slot| slot.instance.app())
            .find(|app| app.id() == id)
    }

    /// The position of the first instance of kind `id`, left to right, if
    /// one runs.
    #[must_use]
    pub fn position_of(&self, id: AppId) -> Option<usize> {
        self.slots
            .iter()
            .position(|slot| slot.instance.app().id() == id)
    }

    /// What the instance at `reader` (`None`: no instance) reads of the
    /// others' ports: the levels they published on the last tick
    /// (journal/D-048).
    #[must_use]
    pub fn bus(&self, reader: Option<usize>) -> Bus {
        Bus::new(self.levels, self.ports(), reader)
    }

    /// The output ports of every slot's instance.
    fn ports(&self) -> [&'static [Port]; MAX_INSTANCES] {
        core::array::from_fn(|index| self.slots[index].instance.app().output_ports())
    }

    /// The first instance of kind `id`, to change it.
    pub fn get_mut(&mut self, id: AppId) -> Option<&mut dyn App> {
        self.slots
            .iter_mut()
            .map(|slot| slot.instance.app_mut())
            .find(|app| app.id() == id)
    }

    /// The first instance of type `T`, left to right, if one runs.
    ///
    /// For tests and the simulator, which look at an applet's own state; the
    /// engine never needs the concrete type.
    #[must_use]
    pub fn find<T: App>(&self) -> Option<&T> {
        self.slots.iter().find_map(|slot| {
            let app: &dyn Any = slot.instance.app();
            app.downcast_ref::<T>()
        })
    }

    /// Puts a fresh applet of kind `id` in the place of the one with the
    /// focus ([`Self::select_at`]).
    pub fn select(&mut self, id: AppId) {
        self.select_at(self.focus, id);
    }

    /// Puts a fresh applet of kind `id` in the slot at `position`, unless it
    /// already is of that kind: idempotent. The old one's outputs are freed
    /// first; the new one's ports take the free outputs in order, one left
    /// without going nowhere until routed. [`AppId::Nothing`] empties the
    /// slot. `false`, and nothing changes, past the last slot.
    pub fn select_at(&mut self, position: usize, id: AppId) -> bool {
        let Some(slot) = self.slots.get(position) else {
            return false;
        };
        if slot.instance.app().id() == id {
            return true;
        }
        self.slots[position].route = Route::OFF;
        let instance = Instance::new(id);
        let route = self.free_route(instance.app().output_ports().len());
        self.slots[position] = Slot::new(instance, route);
        self.levels[position] = [0; CV_CHANNELS];
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
        let slot = position;
        let ports = self
            .at(position)
            .ok_or(Refused::NoSuch)?
            .output_ports()
            .len();
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
        let held = &mut self.slots[slot];
        held.route.set(port, output);
        held.tell_route();
        Ok(held.route)
    }

    /// The input map of the instance at `position`, if any.
    #[must_use]
    pub fn inputs_at(&self, position: usize) -> Option<InputRoute> {
        self.slots.get(position).map(|slot| slot.inputs)
    }

    /// Plugs input port `port` of the instance at `position` into `source`,
    /// a jack of its kind or another instance's port that carries a level
    /// (journal/D-048): the map it now has. Jacks are shared: any instance
    /// may read any jack, and any port.
    ///
    /// # Errors
    ///
    /// [`Refused::NoSuch`] for no such instance, port or jack, or a port it
    /// may not read: its own, one that carries sound, one past the last.
    pub fn set_input(
        &mut self,
        position: usize,
        port: usize,
        source: InputSource,
    ) -> Result<InputRoute, Refused> {
        if let InputSource::Lane { slot, port } = source
            && !self
                .bus(Some(position))
                .offers(usize::from(slot), usize::from(port))
        {
            return Err(Refused::NoSuch);
        }
        let held = self.slots.get_mut(position).ok_or(Refused::NoSuch)?;
        let port = *held
            .instance
            .app()
            .input_ports()
            .get(port)
            .ok_or(Refused::NoSuch)?;
        if !held.inputs.set(port.kind, port.lane, source) {
            return Err(Refused::NoSuch);
        }
        Ok(held.inputs)
    }

    /// The slot and port that reach `output`, if any.
    fn holder(&self, output: CvChannel) -> Option<(usize, usize)> {
        for (slot, held) in self.slots.iter().enumerate() {
            for lane in 0..CV_CHANNELS {
                if held.route.output(lane) == Some(output) {
                    return Some((slot, lane));
                }
            }
        }
        None
    }

    /// A route for `ports` ports over the outputs no instance reaches, in
    /// order: every port to the jack of its rank while one applet runs.
    fn free_route(&self, ports: usize) -> Route {
        let mut taken = [false; CV_CHANNELS];
        for held in &self.slots {
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
    /// where they go. Only the one with the focus sees the panel. Each sees
    /// the levels every slot published on the last tick (journal/D-048).
    pub fn update(&mut self, input: &InputSnapshot) -> [Routed<Sound>; MAX_INSTANCES] {
        let mut quiet = *input;
        quiet.silence_controls();
        let focused = self.focused_slot();
        let outputs = self.ports();
        let last = self.levels;
        let mut parts = [Routed::OFF; MAX_INSTANCES];
        for (index, ((slot, part), levels)) in self
            .slots
            .iter_mut()
            .zip(&mut parts)
            .zip(&mut self.levels)
            .enumerate()
        {
            let input = if index == focused { input } else { &quiet };
            let mut input = *input;
            input.bus = Bus::new(last, outputs, Some(index));
            if slot.inputs != InputRoute::IDENTITY {
                input = slot.inputs.apply(&input);
            }
            let params = slot.instance.app_mut().update(&input);
            *levels = published(&params, outputs[index]);
            *part = Routed {
                params,
                route: slot.route,
            };
        }
        parts
    }

    /// Draws the screen of the instance with the focus.
    pub fn render(&self, frame: &mut FrameBuffer, context: &TickContext) {
        self.focused().render(frame, context);
    }
}

/// The level each lane of `params` carries on its [`PortSignal::Cv`] port
/// among `ports`, 0 V for the others: what the other instances read of it.
fn published(params: &Params, ports: &[Port]) -> [MilliVolts; CV_CHANNELS] {
    let Some(basic) = params.as_basic() else {
        return [0; CV_CHANNELS];
    };
    core::array::from_fn(|lane| match (ports.get(lane), basic.channels[lane]) {
        (Some(port), Channel::Level(level)) if port.signal == PortSignal::Cv => level,
        _ => 0,
    })
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
    use super::{App, AppHost, AppId, InputSnapshot, Instance, MAX_INSTANCES};
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
        assert_eq!(host.focus(), 0);
        let mut occupied = [false; MAX_INSTANCES];
        occupied[0] = true;
        assert_eq!(host.occupied(), occupied, "the other slots empty");
        assert_eq!(host.route(), Route::IDENTITY, "every output its own");
        for position in 1..MAX_INSTANCES {
            assert_eq!(host.at(position).map(App::id), Some(AppId::Nothing));
            assert!(host.route_at(position).unwrap().is_off());
        }
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
    fn an_app_put_in_an_empty_slot_takes_the_free_outputs() {
        let mut host = AppHost::new();
        host.select(AppId::Ratchet);
        assert_eq!(host.route(), Route::first(2), "A and B");
        assert!(host.select_at(1, AppId::Ratchet));
        let mut second = Route::OFF;
        second.set(0, Some(CvChannel::Three));
        second.set(1, Some(CvChannel::Four));
        assert_eq!(host.route_at(1), Some(second), "C and D");
        assert_eq!(host.focus(), 0, "the focus stays where it was");
        for position in 2..MAX_INSTANCES {
            assert!(host.select_at(position, AppId::Tone));
            assert!(host.route_at(position).unwrap().is_off(), "no output left");
        }
        assert!(!host.select_at(MAX_INSTANCES, AppId::Tone), "no such slot");
    }

    #[test]
    fn every_instance_runs_and_only_the_focused_one_sees_the_panel() {
        let mut host = AppHost::new();
        host.select(AppId::Scope);
        host.select_at(1, AppId::Diagnostic);
        assert!(host.set_focus(1));
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
        assert!(!host.set_focus(MAX_INSTANCES));
    }

    #[test]
    fn the_focus_steps_through_every_slot_round_the_ends() {
        let mut host = AppHost::new();
        host.step_focus(false);
        assert_eq!(
            host.focus(),
            MAX_INSTANCES - 1,
            "left of the first: the last"
        );
        assert_eq!(host.current(), AppId::Nothing, "empty slots too");
        host.step_focus(true);
        assert_eq!(host.focus(), 0);
    }

    #[test]
    fn an_output_is_never_shared() {
        let mut host = AppHost::new();
        host.select(AppId::Ratchet);
        host.select_at(1, AppId::Ratchet);
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
        assert_eq!(
            host.set_route(2, 0, None),
            Err(Refused::NoSuch),
            "NOTHING has no port"
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
    fn nothing_frees_the_outputs_and_leaves_the_others_in_place() {
        let mut host = AppHost::new();
        host.select(AppId::Ratchet);
        host.select_at(1, AppId::Tone);
        assert_eq!(host.route_at(1).unwrap().output(0), Some(CvChannel::Three));
        assert!(host.select_at(0, AppId::Nothing));
        assert!(host.route().is_off(), "an empty slot holds no output");
        assert_eq!(
            host.at(1).map(App::id),
            Some(AppId::Tone),
            "TONE stays in its slot"
        );
        host.select(AppId::Ratchet);
        assert_eq!(
            host.route(),
            Route::first(2),
            "A and B again: C and D stay TONE's"
        );
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
        host.select_at(1, AppId::Ratchet);
        let route = host.route_at(1).unwrap();
        assert_eq!(
            (route.output(0), route.output(1)),
            (Some(CvChannel::Three), Some(CvChannel::Four)),
            "RATCHET takes the jacks SUPERSAW left"
        );
    }
}
