//! The Ratchet applet (journal/D-032): each rising edge on `TR1` is copied to
//! output A and starts a burst of triggers on output B.
//!
//! Each burst setting can take an input (not `PASSTHRU`, `SYNC`, `MODE`): `CVn=` follows it (the
//! input sets the setting), `CVn+` adds it to the value set on the panel. The
//! burst's settings are latched at the edge, inputs included, so a burst runs
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
//! `SHAPE` is one signed curve, −3.0 to +3.0 octaves: 0 even, below 0 the
//! burst accelerates, above 0 it slows down. Two models place the repeats,
//! to compare in situation (`MODE` on page 2, see [`burst`]): `FIRST`, a first
//! interval then intervals changing by a constant factor up to the first times
//! `2^SHAPE`; `SPAN`, the repeats spread over a span, crowding towards its end
//! or its start. `PROB` is the chance each repeat fires.
//!
//! Panel: the left encoder picks a setting, the right one changes it; its
//! switch toggles what it changes, the setting's value or its input (`--`,
//! `CV1=`..`CV4=`, `CV1+`..`CV4+`); `up` starts a burst by hand.

pub mod burst;
pub mod tempo;

use core::fmt::Write as _;

use self::burst::{
    Burst, CURVE_TENTHS, Count, MAX_INTERVAL_MICROS, MAX_REPEATS, MAX_SPAN_MICROS,
    MIN_INTERVAL_MICROS, Model, chance_from, count_from, curve_from, time_from,
};
use self::tempo::Tempo;
use crate::apps::{InputSnapshot, TickContext};
use crate::fmt::TextBuf;
use crate::framebuffer::{FrameBuffer, ROW_HEIGHT, WIDTH_I32};
use crate::gfx::{self, Ink, Point};
use crate::platform::{Button, CV_CHANNELS, CvChannel, MilliVolts};
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

/// Left edge of the input column (`CV1+`).
const SOURCE_LEFT: i32 = 104;

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
    /// The curve, −3.0 to +3.0 octaves: 0 even, below accelerating, above
    /// slowing down.
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

/// Where a setting takes an input from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    /// None: the panel's value.
    Panel,
    /// Follows this input: the input sets the setting.
    Follow(CvChannel),
    /// Adds this input to the panel's value.
    Add(CvChannel),
}

impl Source {
    /// Every source, in the order the right encoder turns them.
    pub const ALL: [Self; 9] = [
        Self::Panel,
        Self::Follow(CvChannel::One),
        Self::Follow(CvChannel::Two),
        Self::Follow(CvChannel::Three),
        Self::Follow(CvChannel::Four),
        Self::Add(CvChannel::One),
        Self::Add(CvChannel::Two),
        Self::Add(CvChannel::Three),
        Self::Add(CvChannel::Four),
    ];

    /// The input's reading and whether it is followed, `None` for the panel.
    fn reading(self, cv: &[MilliVolts; CV_CHANNELS]) -> Option<(MilliVolts, bool)> {
        match self {
            Self::Panel => None,
            Self::Follow(channel) => Some((cv[channel.index()], true)),
            Self::Add(channel) => Some((cv[channel.index()], false)),
        }
    }

    /// How the screen names it.
    fn write(self, out: &mut TextBuf<8>) {
        let _ = match self {
            Self::Panel => write!(out, "--"),
            Self::Follow(channel) => write!(out, "CV{}=", channel.index() + 1),
            Self::Add(channel) => write!(out, "CV{}+", channel.index() + 1),
        };
    }
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
    sources: [Source; 4],
    selected: usize,
    /// Whether the right encoder changes the selected setting's input rather
    /// than its value.
    editing_source: bool,
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
            sources: [Source::Panel; 4],
            seed: 0x2545_F491,
            selected: 0,
            editing_source: false,
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
    pub fn source(&self, setting: Setting) -> Option<Source> {
        setting.input_slot().map(|slot| self.sources[slot])
    }

    /// The page of settings on screen, from 0: the selected setting's.
    #[must_use]
    pub const fn page(&self) -> usize {
        self.selected / PAGE_ROWS
    }

    /// Whether the right encoder changes the selected setting's input.
    #[must_use]
    pub const fn editing_source(&self) -> bool {
        self.editing_source
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
        let reading = |setting: Setting| {
            self.source(setting)
                .and_then(|source| source.reading(&self.cv))
        };
        if let Some((millivolts, follows)) = reading(Setting::Count) {
            burst.count = count_from(burst.count, millivolts, follows);
        }
        if let Some((millivolts, follows)) = reading(Setting::Time) {
            burst.time_micros = time_from(burst.model, burst.time_micros, millivolts, follows);
        }
        if let Some((millivolts, follows)) = reading(Setting::Shape) {
            burst.curve_tenths = curve_from(burst.curve_tenths, millivolts, follows);
        }
        if let Some((millivolts, follows)) = reading(Setting::Chance) {
            burst.chance_percent = chance_from(burst.chance_percent, millivolts, follows);
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
        let settings = i32::try_from(Setting::ALL.len()).unwrap_or(1);
        let picks = i32::from(input.controls.delta(0));
        if picks != 0 {
            let picked = i32::try_from(self.selected).unwrap_or(0) + picks;
            self.selected = usize::try_from(picked.rem_euclid(settings)).unwrap_or(0);
            // A newly picked setting is changed by value first.
            self.editing_source = false;
        }
        if input.buttons.pressed(Button::RightEncoder) {
            self.editing_source = !self.editing_source && self.selected().input_slot().is_some();
        }

        let turns = i32::from(input.controls.delta(1));
        if turns != 0 {
            match (self.editing_source, self.selected().input_slot()) {
                (true, Some(slot)) => self.turn_source(slot, turns),
                _ => self.change(self.selected(), turns),
            }
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
                burst.model = match burst.model {
                    Model::First => Model::Span,
                    Model::Span => Model::First,
                };
                // Each model keeps its time within its own range.
                burst.time_micros = burst.time_micros.min(longest_time(burst.model));
            }
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

    /// Turns the input in `slot` through [`Source::ALL`], stopping at both
    /// ends.
    fn turn_source(&mut self, slot: usize, turns: i32) {
        let last = i32::try_from(Source::ALL.len()).unwrap_or(1) - 1;
        let index = Source::ALL
            .iter()
            .position(|&source| source == self.sources[slot])
            .unwrap_or(0);
        let next = (i32::try_from(index).unwrap_or(0) + turns).clamp(0, last);
        self.sources[slot] = Source::ALL[usize::try_from(next).unwrap_or(0)];
    }

    /// Draws the settings, a timeline of the burst they make now, and the
    /// state of `TR1`, A and B.
    pub fn render(&self, frame: &mut FrameBuffer, _context: &TickContext) {
        frame.clear();
        let lit = TextStyle::lit(&FONT_5X8);
        text::draw(frame, "RATCHET", Point::zero(), lit);
        let mut rate = TextBuf::<12>::new();
        let _ = match self.tempo.bpm() {
            Some(bpm) => write!(rate, "{bpm:>3} BPM"),
            None => write!(rate, "--- BPM"),
        };
        text::draw(frame, rate.as_str(), Point::new(42, 0), lit);
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
                text::draw(frame, name, Point::new(x, 0), lit);
            }
        }

        let rows = i32::try_from(PAGE_ROWS).unwrap_or(4);
        self.render_scroll_bar(frame, rows * ROW_HEIGHT);
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
            let label = match (setting, self.settings.model) {
                (Setting::Time, Model::Span) => "SPAN",
                _ => setting.label(),
            };
            text::draw(frame, label, Point::new(ROW_LEFT + 2, top), style);
            let mut value = TextBuf::<16>::new();
            self.write_value(setting, &mut value);
            text::draw(
                frame,
                value.as_str(),
                Point::new(VALUE_RIGHT, top),
                style.aligned(Alignment::Right),
            );
            self.render_source(frame, setting, top, highlighted);
        }

        self.render_timeline(frame, TIMELINE_TOP);
    }

    /// The scroll bar left of the settings, `height` pixels under the title
    /// bar: a dotted track, and a thumb over the share of it the page on
    /// screen takes.
    fn render_scroll_bar(&self, frame: &mut FrameBuffer, height: i32) {
        let pages = i32::try_from(PAGES).unwrap_or(1);
        let page = i32::try_from(self.page()).unwrap_or(0);
        gfx::dotted_vline(frame, SCROLL_WIDTH / 2, ROW_HEIGHT, height, 2, Ink::Lit);
        let top = ROW_HEIGHT + page * height / pages;
        let bottom = ROW_HEIGHT + (page + 1) * height / pages;
        gfx::fill_rect(frame, 0, top, SCROLL_WIDTH, bottom - top, Ink::Lit);
    }

    /// The input column of `setting`'s row: its input when it has one, `--`
    /// while it is being chosen, inverted against the row then.
    fn render_source(
        &self,
        frame: &mut FrameBuffer,
        setting: Setting,
        top: i32,
        highlighted: bool,
    ) {
        let Some(source) = self.source(setting) else {
            return;
        };
        let choosing = highlighted && self.editing_source;
        if source == Source::Panel && !choosing {
            return;
        }
        let mut name = TextBuf::<8>::new();
        source.write(&mut name);
        let right = WIDTH_I32 - 3;
        let style = if choosing {
            // A dark box in the lit row: what the right encoder turns now.
            gfx::fill_rect(
                frame,
                SOURCE_LEFT - 1,
                top,
                right - SOURCE_LEFT + 3,
                ROW_HEIGHT,
                Ink::Dark,
            );
            TextStyle::lit(&FONT_5X8)
        } else if highlighted {
            TextStyle::dark(&FONT_5X8)
        } else {
            TextStyle::lit(&FONT_5X8)
        };
        text::draw(
            frame,
            name.as_str(),
            Point::new(right, top),
            style.aligned(Alignment::Right),
        );
    }

    /// The value of `setting`, as the screen shows it.
    fn write_value(&self, setting: Setting, out: &mut TextBuf<16>) {
        let settings = &self.settings;
        let _ = match setting {
            Setting::Count => match settings.count {
                Count::Repeats(count) => write!(out, "{count}"),
                Count::Gate => write!(out, "GATE"),
            },
            Setting::Time => match self.sync() {
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
