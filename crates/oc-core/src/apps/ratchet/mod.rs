//! The Ratchet applet (journal/D-032): each rising edge on `TR1` is copied to
//! output A and starts a burst of triggers on output B.
//!
//! Each burst setting can take an input (not `PASSTHRU`, `SYNC`, `MODE`),
//! assigned in a submenu (journal/D-035, [`Assign`]): the input's signal is
//! scaled, shifted and bounded, then sets the setting (`SET`, shown `CVn=`),
//! is added to the value set on the panel (`ADD`, `CVn+`) or multiplies it
//! (`MUL`, `CVn*`, 5 V leaving it unchanged). The burst's settings are latched at the edge, inputs included, so a burst runs
//! as it started; a new edge restarts it. Triggers are 5 V for 5 ms, or
//! half the interval when the repeats come faster. Times are counted in the
//! ticks' elapsed microseconds: a repeat falls on the first tick at or past its
//! time, so on the module it is late by less than a tick.
//!
//! The rate of `TR1` is shown in beats per minute (one trigger a beat); with
//! `SYNC ON`, the time setting is a fraction of its period (`/4`: a quarter of
//! the time between two triggers) instead of milliseconds, until the rate is
//! lost.
//!
//! `SHAPE` is one signed curve, −3.0 to +3.0, an abstract value rather than
//! a unit: from an exponential curve at −3.0, through a straight line at 0
//! (even), to a logarithmic one at +3.0; below 0 the burst accelerates,
//! above 0 it slows down. The time setting, `FIRST` or `SPAN`, is a duration
//! in milliseconds, or a fraction of `TR1`'s period with `SYNC ON`. Two
//! models place the repeats,
//! to compare in situation (`MODE` on page 2, see [`burst`]): `FIRST`, a first
//! interval then intervals changing by a constant factor up to the first times
//! `2^SHAPE`; `SPAN`, the repeats spread over a span, crowding towards its end
//! or its start. `PROB` is the chance each repeat fires.
//!
//! Panel: the left encoder picks a setting, the right one changes it; its
//! switch opens the setting's input submenu, where the left encoder picks a
//! line, the right one changes it, and its switch or `down` closes it; `up`
//! starts a burst by hand.

pub mod burst;
pub mod tempo;

use core::fmt::Write as _;

use self::burst::{
    Burst, CURVE_TENTHS, Count, MAX_INTERVAL_MICROS, MAX_REPEATS, MAX_SPAN_MICROS,
    MIN_INTERVAL_MICROS, Model, chance_from, count_from, curve_from, follow_time_micros, time_from,
};
use self::tempo::Tempo;
use crate::apps::param::{ParamSpec, ParamValue, Refused, Unit};
use crate::apps::{App, AppId, InputSnapshot, TickContext};
use crate::audio::Params;
use crate::fixed::{self, ONE};
use crate::fmt::TextBuf;
use crate::framebuffer::{FrameBuffer, ROW_HEIGHT, WIDTH_I32};
use crate::gfx::controls::scroll_bar;
use crate::gfx::{self, Ink, Point};
use crate::modulation::editor::{AssignEditor, Param};
use crate::modulation::{Assign, AssignLine};
use crate::platform::{Button, CV_CHANNELS, MilliVolts};
use crate::text::fonts::FONT_5X8;
use crate::text::{self, Alignment, TextStyle};

/// A trigger's level.
pub const TRIGGER_MV: MilliVolts = 5_000;

/// A trigger's length, unless the repeats come faster.
pub const PULSE_MICROS: u32 = 5_000;

/// Settings shown per page: the burst on the first, the options on the
/// second.
const PAGE_ROWS: usize = 4;

/// Pages of settings.
const PAGES: usize = Setting::ALL.len().div_ceil(PAGE_ROWS);

/// Width of the scroll bar left of the settings, which shows the page.
const SCROLL_WIDTH: i32 = 3;

/// Left edge of the settings' rows, right of the scroll bar and a gap.
const ROW_LEFT: i32 = SCROLL_WIDTH + 1;

/// Top of the burst's timeline, under its playhead; it runs to the bottom
/// of the screen.
const TIMELINE_TOP: i32 = 48;

/// A five-column, three-row triangle at `at`, its tip down.
fn arrow_down(frame: &mut FrameBuffer, at: Point) {
    for row in 0..3 {
        // Rows narrow towards the tip.
        let half = 2 - row;
        gfx::hline(frame, at.x + 2 - half, at.y + row, 2 * half + 1, Ink::Lit);
    }
}

/// Right edge of the value column, left of the input column.
const VALUE_RIGHT: i32 = 98;

/// `index` moved by `turns` among `len`, wrapping round.
fn wrapped(index: usize, len: usize, turns: i32) -> usize {
    let len = i32::try_from(len).unwrap_or(1);
    let moved = i32::try_from(index).unwrap_or(0) + turns;
    usize::try_from(moved.rem_euclid(len)).unwrap_or(0)
}

/// Shortest trigger.
const MIN_PULSE_MICROS: u32 = 1_000;

/// The settings at start.
const DEFAULT_BURST: Burst = Burst {
    model: Model::First,
    count: Count::Repeats(4),
    time_micros: 100_000,
    curve_tenths: 0,
    chance_percent: 100,
};

/// The longest time a model takes: a first interval, or a span.
const fn longest_time(model: Model) -> u32 {
    match model {
        Model::First => MAX_INTERVAL_MICROS,
        Model::Span => MAX_SPAN_MICROS,
    }
}

/// The settings the panel walks through.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Setting {
    /// How many repeats, or as long as the gate.
    Count,
    /// The first interval (`MODE FIRST`) or the span of the burst
    /// (`MODE SPAN`).
    Time,
    /// The curve, −3.0 to +3.0, from exponential through even (0) to
    /// logarithmic: below accelerating, above slowing down.
    Shape,
    /// The chance each repeat fires.
    Chance,
    /// Whether B also carries the incoming trigger.
    Passthru,
    /// Whether `FIRST` is a fraction of `TR1`'s period.
    Sync,
    /// How the repeats are placed: from a first interval, or over a span.
    Mode,
}

impl Setting {
    /// Every setting, top to bottom.
    pub const ALL: [Self; 7] = [
        Self::Count,
        Self::Time,
        Self::Shape,
        Self::Chance,
        Self::Passthru,
        Self::Sync,
        Self::Mode,
    ];

    /// Where this setting's input is kept, `None` for the options, which
    /// take none.
    const fn input_slot(self) -> Option<usize> {
        match self {
            Self::Count => Some(0),
            Self::Time => Some(1),
            Self::Shape => Some(2),
            Self::Chance => Some(3),
            Self::Passthru | Self::Sync | Self::Mode => None,
        }
    }

    const fn label(self) -> &'static str {
        match self {
            Self::Count => "COUNT",
            Self::Time => "FIRST",
            Self::Shape => "SHAPE",
            Self::Chance => "PROB",
            Self::Passthru => "PASSTHRU",
            Self::Sync => "SYNC",
            Self::Mode => "MODE",
        }
    }
}

/// The fractions of `TR1`'s period `FIRST` can be with `SYNC ON`, shortest
/// last: (multiplier, divisor).
pub const DIVISIONS: [(u32, u32); 15] = [
    (2, 1),
    (3, 2),
    (4, 3),
    (1, 1),
    (3, 4),
    (2, 3),
    (1, 2),
    (1, 3),
    (1, 4),
    (1, 6),
    (1, 8),
    (1, 12),
    (1, 16),
    (1, 24),
    (1, 32),
];
/// [`DIVISIONS`] as the screen writes them.
pub const DIVISION_NAMES: [&str; DIVISIONS.len()] = [
    "X2", "X1.5", "X4/3", "/1", "X3/4", "X2/3", "/2", "/3", "/4", "/6", "/8", "/12", "/16", "/24",
    "/32",
];

/// What `RATCHET` is set by (journal/D-043), in the order of the screen.
/// `GATE` and `DIVISION` are the other faces of `COUNT` and `FIRST`: a count
/// past the last is a gate, and `FIRST` with `SYNC ON` a division.
#[allow(clippy::cast_possible_wrap)]
// Every bound here is in the thousands at most.
pub const PARAM_SPECS: [ParamSpec; 9] = [
    ParamSpec::number("COUNT", (1, MAX_REPEATS as i32), 1, 0, Unit::None).assignable(),
    ParamSpec::toggle("GATE"),
    ParamSpec::number(
        "FIRST",
        (
            (MIN_INTERVAL_MICROS / 1_000) as i32,
            (MAX_SPAN_MICROS / 1_000) as i32,
        ),
        1,
        0,
        Unit::Milliseconds,
    )
    .assignable(),
    ParamSpec::choice("DIVISION", &DIVISION_NAMES),
    // A curve without a unit, as the screen writes it (`-1.5`).
    ParamSpec::number("SHAPE", (-CURVE_TENTHS, CURVE_TENTHS), 1, 1, Unit::None).assignable(),
    ParamSpec::number("PROB", (1, 100), 1, 0, Unit::Percent).assignable(),
    ParamSpec::toggle("PASSTHRU"),
    ParamSpec::toggle("SYNC"),
    ParamSpec::choice("MODE", &["FIRST", "SPAN"]),
];

/// What `RATCHET` can be read for: the rate of `TR1`, the repeats sent by
/// the burst under way.
pub const OBSERVABLES: [ParamSpec; 2] = [
    ParamSpec::number("RATE", (0, 999), 1, 0, Unit::Bpm),
    ParamSpec::number("SENT", (0, MAX_REPEATS as i32), 1, 0, Unit::None),
];

/// What `RATCHET` can be asked to do: a burst by hand, as `up` starts one.
pub const ACTIONS: [&str; 1] = ["BURST"];

/// What `FIRST` is measured in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FirstUnit {
    /// Milliseconds.
    Millis,
    /// A fraction of `TR1`'s period (`SYNC ON`).
    Period,
}

/// The default fraction: a quarter of the period.
const DEFAULT_DIVISION: usize = 8;

/// The range of the signal that sweeps `setting` over all its values when
/// it follows its input (journal/D-035), from the signal that gives its
/// least value to the one that gives its most; `None` for a setting that
/// takes no input. [`crate::modulation::usable_range`] derives from it the ranges of the other
/// effects.
#[must_use]
pub fn sweep(setting: Setting, model: Model) -> Option<(MilliVolts, MilliVolts)> {
    // How many times `longer` halves down to `shorter`, as millivolts: a
    // volt halves the time.
    let halvings = |longer: u32, shorter: u32| {
        fixed::log2(i64::from(longer) * i64::from(ONE) / i64::from(shorter.max(1)))
            .map_or(0, |halvings| {
                MilliVolts::try_from(halvings * 1_000 / i64::from(ONE)).unwrap_or(0)
            })
    };
    let range = match setting {
        // Four repeats per volt, one at 0 V.
        Setting::Count => (0, (i32::from(MAX_REPEATS) - 1) * 1_000 / 4),
        // A volt more halves the time, which follows from its 0 V value.
        Setting::Time => {
            let follow = follow_time_micros(model);
            (
                -halvings(longest_time(model), follow),
                halvings(follow, MIN_INTERVAL_MICROS),
            )
        }
        // A tenth per 100 mV.
        Setting::Shape => (-CURVE_TENTHS * 100, CURVE_TENTHS * 100),
        // 20 % per volt.
        Setting::Chance => (0, 5_000),
        Setting::Passthru | Setting::Sync | Setting::Mode => return None,
    };
    Some(range)
}

/// A burst under way.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Running {
    /// Its settings, latched at the edge.
    burst: Burst,
    /// Repeats already sent.
    sent: u32,
    /// Time since the last trigger of the burst.
    since_micros: u32,
    /// The triggers B carried so far: bit 0 the incoming one, bit `n` the
    /// `n`th repeat.
    fired: u64,
}

impl Running {
    /// Time since the burst started.
    fn elapsed_micros(&self) -> u64 {
        (1..=self.sent)
            .map(|index| u64::from(self.burst.interval_micros(index)))
            .sum::<u64>()
            + u64::from(self.since_micros)
    }
}

/// Retriggers `TR1` onto A and bursts it onto B.
#[derive(Debug, Clone)]
pub struct RatchetApp {
    settings: Burst,
    passthru: bool,
    /// What `FIRST` is measured in (`SYNC`).
    first_unit: FirstUnit,
    /// The fraction, an index in [`DIVISIONS`].
    division: usize,
    /// The rate of `TR1`.
    tempo: Tempo,
    /// The state of the generator that rolls each repeat's chance.
    seed: u32,
    /// The input of each setting that takes one (see `Setting::input_slot`).
    assigns: [Assign; 4],
    selected: usize,
    /// The selected setting's input submenu, `None` while it is closed.
    assigning: Option<AssignEditor>,
    gate: bool,
    running: Option<Running>,
    pulse_a_micros: u32,
    pulse_b_micros: u32,
    cv: [MilliVolts; CV_CHANNELS],
    outputs: [MilliVolts; CV_CHANNELS],
}

impl RatchetApp {
    /// The default settings, nothing running.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            settings: DEFAULT_BURST,
            passthru: true,
            first_unit: FirstUnit::Millis,
            division: DEFAULT_DIVISION,
            tempo: Tempo::new(),
            assigns: [Assign::NONE; 4],
            seed: 0x2545_F491,
            selected: 0,
            assigning: None,
            gate: false,
            running: None,
            pulse_a_micros: 0,
            pulse_b_micros: 0,
            cv: [0; CV_CHANNELS],
            outputs: [0; CV_CHANNELS],
        }
    }

    /// The burst settings as the panel sets them.
    #[must_use]
    pub const fn settings(&self) -> &Burst {
        &self.settings
    }

    /// Whether B carries the incoming trigger too.
    #[must_use]
    pub const fn passthru(&self) -> bool {
        self.passthru
    }

    /// The setting the right encoder changes.
    #[must_use]
    pub const fn selected(&self) -> Setting {
        Setting::ALL[self.selected]
    }

    /// The rate of `TR1` in beats per minute, `None` when unknown.
    #[must_use]
    pub fn bpm(&self) -> Option<u32> {
        self.tempo.bpm()
    }

    /// Whether `FIRST` is a fraction of `TR1`'s period, and which.
    #[must_use]
    pub const fn sync(&self) -> Option<(u32, u32)> {
        match self.first_unit {
            FirstUnit::Period => Some(DIVISIONS[self.division]),
            FirstUnit::Millis => None,
        }
    }

    /// The input `setting` takes, `None` for one that takes none.
    #[must_use]
    pub fn assign(&self, setting: Setting) -> Option<&Assign> {
        setting.input_slot().map(|slot| &self.assigns[slot])
    }

    /// The page of settings on screen, from 0: the selected setting's.
    #[must_use]
    pub const fn page(&self) -> usize {
        self.selected / PAGE_ROWS
    }

    /// The field picked in the selected setting's input submenu, `None`
    /// while it is closed.
    #[must_use]
    pub const fn assigning(&self) -> Option<AssignLine> {
        match &self.assigning {
            Some(editor) => Some(editor.line()),
            None => None,
        }
    }

    /// The lowest and highest reading of the submenu's input lately (see
    /// [`AssignEditor::range`]), `None` while it is closed or before the
    /// first.
    #[must_use]
    pub fn observed(&self) -> Option<(MilliVolts, MilliVolts)> {
        self.assigning.as_ref().and_then(AssignEditor::range)
    }

    /// Repeats sent by the burst under way, `None` when none runs.
    #[must_use]
    pub fn repeats_sent(&self) -> Option<u32> {
        self.running.map(|running| running.sent)
    }

    /// Follows the panel and `TR1`, runs the burst, sets the outputs.
    pub fn update(&mut self, input: &InputSnapshot) -> [MilliVolts; CV_CHANNELS] {
        self.follow_panel(input);
        let elapsed = input.elapsed_micros;
        self.pulse_a_micros = self.pulse_a_micros.saturating_sub(elapsed);
        self.pulse_b_micros = self.pulse_b_micros.saturating_sub(elapsed);
        self.cv = input.cv;
        self.observe(elapsed);

        let gate = input.triggers[0];
        let rising = gate && !self.gate;
        self.gate = gate;
        self.tempo.advance(elapsed);
        if rising {
            self.tempo.edge();
        }
        self.advance(elapsed, gate);
        if rising || input.buttons.pressed(Button::Up) {
            self.start();
        }

        self.outputs = [
            if self.pulse_a_micros > 0 {
                TRIGGER_MV
            } else {
                0
            },
            if self.pulse_b_micros > 0 {
                TRIGGER_MV
            } else {
                0
            },
            0,
            0,
        ];
        self.outputs
    }

    /// The settings of a burst started now: the panel's, with each setting's
    /// input applied as it reads.
    fn latched(&self) -> Burst {
        self.with_inputs(self.synced())
    }

    /// The panel's settings, the time a fraction of `TR1`'s period with
    /// `SYNC ON` and the rate known, before the inputs.
    fn synced(&self) -> Burst {
        let mut burst = self.settings;
        if let (Some((multiplier, divisor)), Some(period)) =
            (self.sync(), self.tempo.period_micros())
        {
            let time = u64::from(period) * u64::from(multiplier) / u64::from(divisor);
            let bounded = time.clamp(
                u64::from(MIN_INTERVAL_MICROS),
                u64::from(longest_time(burst.model)),
            );
            burst.time_micros = u32::try_from(bounded).unwrap_or(MAX_INTERVAL_MICROS);
        }
        burst
    }

    /// `burst` with each setting's input applied as it reads.
    fn with_inputs(&self, mut burst: Burst) -> Burst {
        let reading = |setting: Setting| {
            self.assign(setting)
                .and_then(|assign| assign.reading(&self.cv))
        };
        if let Some((millivolts, effect)) = reading(Setting::Count) {
            burst.count = count_from(burst.count, millivolts, effect);
        }
        if let Some((millivolts, effect)) = reading(Setting::Time) {
            burst.time_micros = time_from(burst.model, burst.time_micros, millivolts, effect);
        }
        if let Some((millivolts, effect)) = reading(Setting::Shape) {
            burst.curve_tenths = curve_from(burst.curve_tenths, millivolts, effect);
        }
        if let Some((millivolts, effect)) = reading(Setting::Chance) {
            burst.chance_percent = chance_from(burst.chance_percent, millivolts, effect);
        }
        burst
    }

    /// Starts a burst: the trigger on A, on B too if set, the first repeat
    /// one interval later.
    fn start(&mut self) {
        let burst = self.latched();
        self.pulse_a_micros = PULSE_MICROS;
        if self.passthru {
            self.pulse_b_micros = pulse_for(burst.interval_micros(1));
        }
        self.running = Some(Running {
            burst,
            sent: 0,
            since_micros: 0,
            fired: u64::from(self.passthru),
        });
    }

    /// Sends the repeats whose time has come; ends the burst when it has sent
    /// them all, or when its gate fell.
    fn advance(&mut self, elapsed: u32, gate: bool) {
        let Some(mut running) = self.running else {
            return;
        };
        running.since_micros = running.since_micros.saturating_add(elapsed);
        loop {
            if running.sent >= running.burst.max_repeats()
                || (running.burst.count == Count::Gate && !gate)
            {
                self.running = None;
                return;
            }
            let interval = running.burst.interval_micros(running.sent + 1);
            if running.since_micros < interval {
                break;
            }
            running.since_micros -= interval;
            running.sent += 1;
            let next = running.burst.interval_micros(running.sent + 1);
            // A repeat that does not fire leaves its time to pass all the same.
            if self.roll(running.burst.chance_percent) {
                self.pulse_b_micros = pulse_for(interval.min(next));
                running.fired |= 1_u64.checked_shl(running.sent).unwrap_or(0);
            }
        }
        self.running = Some(running);
    }

    fn follow_panel(&mut self, input: &InputSnapshot) {
        let selected = self.selected();
        if let (Some(editor), Some(slot), Some(sweep)) = (
            &mut self.assigning,
            selected.input_slot(),
            sweep(selected, self.settings.model),
        ) {
            if !editor.follow(&mut self.assigns[slot], input, sweep) {
                self.assigning = None;
            }
            return;
        }
        let picks = i32::from(input.controls.delta(0));
        if picks != 0 {
            self.selected = wrapped(self.selected, Setting::ALL.len(), picks);
        }
        if input.buttons.pressed(Button::RightEncoder) && self.selected().input_slot().is_some() {
            self.assigning = Some(AssignEditor::new());
        }
        let turns = i32::from(input.controls.delta(1));
        if turns != 0 {
            self.change(self.selected(), turns);
        }
    }

    /// Adds the reading of the submenu's input to its range seen, `elapsed`
    /// after the last.
    fn observe(&mut self, elapsed: u32) {
        let slot = self.selected().input_slot();
        if let (Some(editor), Some(slot)) = (&mut self.assigning, slot) {
            editor.observe(&self.assigns[slot], &self.cv, elapsed);
        }
    }

    /// Turns `setting` by `turns` detents.
    fn change(&mut self, setting: Setting, turns: i32) {
        let burst = &mut self.settings;
        match setting {
            Setting::Count => {
                burst.count = match burst.count {
                    Count::Gate if turns < 0 => Count::Repeats(MAX_REPEATS),
                    Count::Gate => Count::Gate,
                    Count::Repeats(count) => {
                        let next = i32::from(count) + turns;
                        if next > i32::from(MAX_REPEATS) {
                            Count::Gate
                        } else {
                            Count::Repeats(u8::try_from(next.max(1)).unwrap_or(1))
                        }
                    }
                };
            }
            Setting::Time if self.first_unit == FirstUnit::Period => {
                let last = i32::try_from(DIVISIONS.len()).unwrap_or(1) - 1;
                let next = (i32::try_from(self.division).unwrap_or(0) + turns).clamp(0, last);
                self.division = usize::try_from(next).unwrap_or(DEFAULT_DIVISION);
            }
            Setting::Time => {
                // A sixteenth of the time per detent, at least a
                // millisecond, kept in whole milliseconds.
                let millis = burst.time_micros / 1_000;
                let step = (millis / 16).max(1);
                let step = i64::from(step) * i64::from(turns);
                let next = (i64::from(millis) + step) * 1_000;
                let bounded = next.clamp(
                    i64::from(MIN_INTERVAL_MICROS),
                    i64::from(longest_time(burst.model)),
                );
                burst.time_micros = u32::try_from(bounded).unwrap_or(MIN_INTERVAL_MICROS);
            }
            Setting::Shape => {
                burst.curve_tenths =
                    (burst.curve_tenths + turns).clamp(-CURVE_TENTHS, CURVE_TENTHS);
            }
            Setting::Chance => {
                let next = i64::from(burst.chance_percent) + i64::from(turns);
                burst.chance_percent = u32::try_from(next.clamp(1, 100)).unwrap_or(100);
            }
            Setting::Passthru => self.passthru = !self.passthru,
            Setting::Sync => {
                self.first_unit = match self.first_unit {
                    FirstUnit::Millis => FirstUnit::Period,
                    FirstUnit::Period => FirstUnit::Millis,
                };
            }
            Setting::Mode => {
                let model = match burst.model {
                    Model::First => Model::Span,
                    Model::Span => Model::First,
                };
                self.set_model(model);
            }
        }
    }

    /// Places the repeats by `model`, its time kept within the model's own
    /// range.
    const fn set_model(&mut self, model: Model) {
        let burst = &mut self.settings;
        burst.model = model;
        let longest = longest_time(model);
        if burst.time_micros > longest {
            burst.time_micros = longest;
        }
    }

    /// Whether a repeat with `chance` percent fires: a xorshift generator,
    /// seeded the same at every start, so a run replays exactly.
    const fn roll(&mut self, chance: u32) -> bool {
        if chance >= 100 {
            return true;
        }
        let mut x = self.seed;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.seed = x;
        x % 100 < chance
    }

    /// Draws the settings, a timeline of the burst they make now, and the
    /// state of `TR1`, A and B.
    pub fn render(&self, frame: &mut FrameBuffer, _context: &TickContext) {
        frame.clear();
        self.render_main(frame);
        if let (Some(editor), Some(assign)) = (&self.assigning, self.assign(self.selected())) {
            self.render_submenu(frame, editor, assign);
        }
    }

    /// The main screen: the title bar, the page of settings, the timeline.
    fn render_main(&self, frame: &mut FrameBuffer) {
        let lit = TextStyle::lit(&FONT_5X8);
        text::draw(frame, "RATCHET", Point::zero(), lit);
        let mut rate = TextBuf::<12>::new();
        let _ = match self.tempo.bpm() {
            Some(bpm) => write!(rate, "{bpm:>3} BPM"),
            None => write!(rate, "--- BPM"),
        };
        text::draw(frame, rate.as_str(), Point::new(42, 0), lit);
        self.render_lights(frame);

        let rows = i32::try_from(PAGE_ROWS).unwrap_or(4);
        // Left of the settings, under the title bar.
        scroll_bar(
            frame,
            0,
            ROW_HEIGHT,
            SCROLL_WIDTH,
            rows * ROW_HEIGHT,
            self.page(),
            PAGES,
        );
        let first = self.page() * PAGE_ROWS;
        let shown = &Setting::ALL[first..(first + PAGE_ROWS).min(Setting::ALL.len())];
        for (row, &setting) in (1..).zip(shown) {
            let top = row * ROW_HEIGHT;
            let highlighted = setting == self.selected();
            if highlighted {
                gfx::fill_rect(
                    frame,
                    ROW_LEFT,
                    top,
                    WIDTH_I32 - ROW_LEFT,
                    ROW_HEIGHT,
                    Ink::Lit,
                );
            }
            let style = if highlighted {
                TextStyle::dark(&FONT_5X8)
            } else {
                lit
            };
            text::draw(
                frame,
                self.label(setting),
                Point::new(ROW_LEFT + 2, top),
                style,
            );
            // A setting that follows its input has no use for the panel's
            // value: its input stands in its place.
            if !self.assign(setting).is_some_and(Assign::sets) {
                let mut value = TextBuf::<16>::new();
                self.write_value(setting, &self.settings, self.sync(), &mut value);
                text::draw(
                    frame,
                    value.as_str(),
                    Point::new(VALUE_RIGHT, top),
                    style.aligned(Alignment::Right),
                );
            }
            if let Some(assign) = self.assign(setting) {
                let mut name = TextBuf::<8>::new();
                assign.write(&mut name);
                text::draw(
                    frame,
                    name.as_str(),
                    Point::new(WIDTH_I32 - 3, top),
                    style.aligned(Alignment::Right),
                );
            }
        }

        self.render_timeline(frame, TIMELINE_TOP);
    }

    /// How the screen names `setting`: the time is a span in `MODE SPAN`.
    fn label(&self, setting: Setting) -> &'static str {
        match (setting, self.settings.model) {
            (Setting::Time, Model::Span) => "SPAN",
            _ => setting.label(),
        }
    }

    /// The lights of `TR1`, A and B at the right of the title bar.
    fn render_lights(&self, frame: &mut FrameBuffer) {
        let lights = [
            ("1", self.gate),
            ("A", self.pulse_a_micros > 0),
            ("B", self.pulse_b_micros > 0),
        ];
        for (index, (name, on)) in (0..).zip(lights) {
            let x = WIDTH_I32 - 30 + index * 10;
            if on {
                gfx::fill_rect(frame, x - 1, 0, 7, ROW_HEIGHT, Ink::Lit);
                text::draw(frame, name, Point::new(x, 0), TextStyle::dark(&FONT_5X8));
            } else {
                text::draw(frame, name, Point::new(x, 0), TextStyle::lit(&FONT_5X8));
            }
        }
    }

    /// The selected setting's input submenu over the main screen: the
    /// setting, the value a burst would take now and where both sit in its
    /// range (journal/D-036).
    fn render_submenu(&self, frame: &mut FrameBuffer, editor: &AssignEditor, assign: &Assign) {
        let setting = self.selected();
        let latched = self.latched();
        let mut now = TextBuf::<16>::new();
        self.write_value(setting, &latched, None, &mut now);
        let param = Param {
            name: self.label(setting),
            now: now.as_str(),
            // A bipolar setting is marked at its middle, the others at
            // their start.
            zero_permille: if setting == Setting::Shape { 500 } else { 0 },
            panel_permille: position(setting, &self.synced()),
            now_permille: position(setting, &latched),
        };
        editor.render(frame, assign, &self.cv, &param);
    }

    /// The value of `setting` in `settings`, as the screen shows it; the
    /// time as a fraction of `TR1`'s period when `sync` gives one.
    fn write_value(
        &self,
        setting: Setting,
        settings: &Burst,
        sync: Option<(u32, u32)>,
        out: &mut TextBuf<16>,
    ) {
        let _ = match setting {
            Setting::Count => match settings.count {
                Count::Repeats(count) => write!(out, "{count}"),
                Count::Gate => write!(out, "GATE"),
            },
            Setting::Time => match sync {
                Some((1, divisor)) => write!(out, "/{divisor}"),
                Some((multiplier, 1)) => write!(out, "X{multiplier}"),
                Some((3, 2)) => write!(out, "X1.5"),
                Some((multiplier, divisor)) => write!(out, "X{multiplier}/{divisor}"),
                None => write!(out, "{} MS", settings.time_micros / 1_000),
            },
            Setting::Shape => {
                let sign = match settings.curve_tenths {
                    0 => "",
                    curve if curve < 0 => "-",
                    _ => "+",
                };
                let tenths = settings.curve_tenths.unsigned_abs();
                write!(out, "{sign}{}.{}", tenths / 10, tenths % 10)
            }
            Setting::Chance => write!(out, "{}%", settings.chance_percent),
            Setting::Passthru => write!(out, "{}", if self.passthru { "ON" } else { "OFF" }),
            Setting::Sync => write!(
                out,
                "{}",
                if self.first_unit == FirstUnit::Period {
                    "ON"
                } else {
                    "OFF"
                }
            ),
            Setting::Mode => write!(
                out,
                "{}",
                match settings.model {
                    Model::First => "FIRST",
                    Model::Span => "SPAN",
                }
            ),
        };
    }

    /// The triggers of the burst on B, as marks on a line (eight repeats for
    /// a gate burst), from `top` to the bottom of the screen: the burst under
    /// way, or else the one a trigger would start now.
    ///
    /// Under way, a playhead above the line follows the burst: the triggers
    /// B carried are drawn bold, the repeats `PROB` dropped as a dot.
    ///
    /// With `TR1`'s rate known, a dotted line marks where the next trigger is
    /// due, one period after the start: the repeats past it would be cut by
    /// the restart, and are drawn short. The line is scaled to the longer of
    /// the burst and the period, so the mark stays on screen.
    fn render_timeline(&self, frame: &mut FrameBuffer, top: i32) {
        let burst = self
            .running
            .map_or_else(|| self.latched(), |running| running.burst);
        let repeats = burst.max_repeats().min(if burst.count == Count::Gate {
            8
        } else {
            u32::MAX
        });
        let total: u64 = (1..=repeats)
            .map(|index| u64::from(burst.interval_micros(index)))
            .sum();
        let next_trigger = self.tempo.period_micros().map(u64::from);
        let span = total.max(next_trigger.unwrap_or(0)).max(1);
        let baseline = crate::framebuffer::HEIGHT_I32 - 1;
        let height = baseline - top;
        gfx::hline(frame, 0, baseline, WIDTH_I32, Ink::Lit);
        let width = u64::from((WIDTH_I32 - 1).unsigned_abs());
        let x_of = |at: u64| i32::try_from(at.min(span) * width / span).unwrap_or(0);
        let mark = |frame: &mut FrameBuffer, at: u64, index: u32, tall: bool| {
            let x = x_of(at);
            let length = if tall { height } else { height / 3 };
            let Some(running) = self.running.filter(|running| index <= running.sent) else {
                gfx::vline(frame, x, baseline - length, length, Ink::Lit);
                return;
            };
            if running.fired & 1_u64.checked_shl(index).unwrap_or(0) != 0 {
                gfx::fill_rect(frame, x - 1, baseline - length, 3, length, Ink::Lit);
            } else {
                gfx::pixel(frame, x, baseline - 1, Ink::Lit);
            }
        };
        mark(frame, 0, 0, self.passthru);
        let mut at: u64 = 0;
        for index in 1..=repeats {
            at += u64::from(burst.interval_micros(index));
            let cut = next_trigger.is_some_and(|due| at >= due);
            mark(frame, at, index, !cut);
        }
        if let Some(due) = next_trigger {
            gfx::dotted_vline(frame, x_of(due), top, height, 2, Ink::Lit);
        }
        if let Some(running) = self.running {
            let x = x_of(running.elapsed_micros());
            arrow_down(frame, Point::new(x - 2, top - 4));
        }
    }
}

/// Where `setting` sits in its range in `burst`, in thousandths: the count
/// from one repeat to `GATE`, the time on a logarithmic scale (an input
/// halves it per volt), the curve from −3.0 to +3.0, the chance from 0 %.
fn position(setting: Setting, burst: &Burst) -> i32 {
    let permille = match setting {
        Setting::Count => match burst.count {
            Count::Repeats(count) => (i64::from(count) - 1) * 1_000 / i64::from(MAX_REPEATS),
            Count::Gate => 1_000,
        },
        Setting::Time => {
            let halvings = |micros: u32| {
                fixed::log2(i64::from(micros) * i64::from(ONE) / i64::from(MIN_INTERVAL_MICROS))
                    .unwrap_or(0)
            };
            let full = halvings(longest_time(burst.model)).max(1);
            halvings(burst.time_micros) * 1_000 / full
        }
        Setting::Shape => {
            (i64::from(burst.curve_tenths) + i64::from(CURVE_TENTHS)) * 1_000
                / (2 * i64::from(CURVE_TENTHS))
        }
        Setting::Chance => i64::from(burst.chance_percent) * 10,
        Setting::Passthru | Setting::Sync | Setting::Mode => 0,
    };
    i32::try_from(permille.clamp(0, 1_000)).unwrap_or(0)
}

impl App for RatchetApp {
    fn id(&self) -> AppId {
        AppId::Ratchet
    }

    fn update(&mut self, input: &InputSnapshot) -> Params {
        RatchetApp::update(self, input).into()
    }

    fn render(&self, frame: &mut FrameBuffer, context: &TickContext) {
        RatchetApp::render(self, frame, context);
    }

    fn param_specs(&self) -> &'static [ParamSpec] {
        &PARAM_SPECS
    }

    fn param_active(&self, index: usize) -> bool {
        let synced = self.first_unit == FirstUnit::Period;
        match index {
            // A count past the last is the gate, which its own row shows.
            0 => self.settings.count != Count::Gate,
            // The time in milliseconds, or as a division of TR1's period.
            2 => !synced,
            3 => synced,
            _ => true,
        }
    }

    fn param_label(&self, index: usize) -> &'static str {
        match index {
            // The time is a span in `MODE SPAN`, as the screen says.
            2 => self.label(Setting::Time),
            _ => PARAM_SPECS.get(index).map_or("", |spec| spec.name),
        }
    }

    fn get(&self, index: usize) -> Option<ParamValue> {
        let burst = &self.settings;
        let value = match index {
            0 => ParamValue::Number(match burst.count {
                Count::Repeats(count) => i32::from(count),
                Count::Gate => i32::from(MAX_REPEATS),
            }),
            1 => ParamValue::Toggle(burst.count == Count::Gate),
            2 => ParamValue::Number(i32::try_from(burst.time_micros / 1_000).unwrap_or(0)),
            3 => ParamValue::Choice(u8::try_from(self.division).unwrap_or(0)),
            4 => ParamValue::Number(burst.curve_tenths),
            5 => ParamValue::Number(i32::try_from(burst.chance_percent).unwrap_or(100)),
            6 => ParamValue::Toggle(self.passthru),
            7 => ParamValue::Toggle(self.first_unit == FirstUnit::Period),
            8 => ParamValue::Choice(match burst.model {
                Model::First => 0,
                Model::Span => 1,
            }),
            _ => return None,
        };
        Some(value)
    }

    fn set(&mut self, index: usize, value: ParamValue) -> Result<ParamValue, Refused> {
        let spec = PARAM_SPECS.get(index).ok_or(Refused::NoSuch)?;
        let value = spec.check(value)?;
        let burst = &mut self.settings;
        match (index, value) {
            (0, ParamValue::Number(count)) => {
                burst.count = Count::Repeats(u8::try_from(count).unwrap_or(1));
            }
            (1, ParamValue::Toggle(true)) => burst.count = Count::Gate,
            // Out of the gate, as a detent down leaves it.
            (1, ParamValue::Toggle(false)) if burst.count == Count::Gate => {
                burst.count = Count::Repeats(MAX_REPEATS);
            }
            (2, ParamValue::Number(millis)) => {
                let micros = u32::try_from(millis).unwrap_or(0) * 1_000;
                burst.time_micros = micros.min(longest_time(burst.model));
            }
            (3, ParamValue::Choice(division)) => self.division = usize::from(division),
            (4, ParamValue::Number(curve)) => burst.curve_tenths = curve,
            (5, ParamValue::Number(chance)) => {
                burst.chance_percent = u32::try_from(chance).unwrap_or(100);
            }
            (6, ParamValue::Toggle(on)) => self.passthru = on,
            (7, ParamValue::Toggle(on)) => {
                self.first_unit = if on {
                    FirstUnit::Period
                } else {
                    FirstUnit::Millis
                };
            }
            (8, ParamValue::Choice(model)) => {
                self.set_model(if model == 0 {
                    Model::First
                } else {
                    Model::Span
                });
            }
            _ => {}
        }
        self.get(index).ok_or(Refused::NoSuch)
    }

    fn observables(&self) -> &'static [ParamSpec] {
        &OBSERVABLES
    }

    fn observe(&self, index: usize) -> Option<ParamValue> {
        let number = match index {
            0 => self.tempo.bpm()?,
            1 => self.repeats_sent()?,
            _ => return None,
        };
        Some(ParamValue::Number(
            i32::try_from(number).unwrap_or(i32::MAX),
        ))
    }

    fn actions(&self) -> &'static [&'static str] {
        &ACTIONS
    }

    fn act(&mut self, index: usize) -> Result<(), Refused> {
        if index == 0 {
            self.start();
            Ok(())
        } else {
            Err(Refused::NoSuch)
        }
    }
}

impl Default for RatchetApp {
    fn default() -> Self {
        Self::new()
    }
}

/// A trigger's length before a repeat `interval` away: 5 ms, or half the
/// interval when that is shorter, never under a millisecond.
const fn pulse_for(interval: u32) -> u32 {
    let half = interval / 2;
    let pulse = if half < PULSE_MICROS {
        half
    } else {
        PULSE_MICROS
    };
    if pulse < MIN_PULSE_MICROS {
        MIN_PULSE_MICROS
    } else {
        pulse
    }
}

#[cfg(test)]
mod tests {
    use super::{DEFAULT_BURST, DIVISION_NAMES, DIVISIONS, RatchetApp, Setting};
    use crate::fmt::TextBuf;

    #[test]
    fn the_division_names_are_the_screens() {
        let app = RatchetApp::new();
        for (division, name) in DIVISIONS.into_iter().zip(DIVISION_NAMES) {
            let mut shown = TextBuf::<16>::new();
            app.write_value(Setting::Time, &DEFAULT_BURST, Some(division), &mut shown);
            assert_eq!(shown.as_str(), name, "{division:?}");
        }
    }
}
