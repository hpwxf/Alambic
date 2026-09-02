//! Terminal user interface for the simulated module.
//!
//! The UI is a thin shell: it turns keystrokes into
//! [`Event`](crate::scenario::Event)s, asks the [`Simulator`] to run ticks, and
//! draws what it reports. It contains no module behaviour whatsoever.

use std::fmt::Write as _;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use ratatui::Frame;
use ratatui::crossterm::event::{self, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};

use oc_core::apps::AppId;
use oc_core::calibration::{CV_IN_MAX_MV, CV_IN_MIN_MV};
use oc_core::platform::{Button, CV_CHANNELS, CvChannel, MilliVolts, TriggerChannel};

use crate::braille;
use crate::clock::Speed;
use crate::scenario::{Event, Scenario};
use crate::simulator::Simulator;

/// Coarse step applied to a CV input by the arrow keys, in millivolts.
const COARSE_STEP_MV: MilliVolts = 100;

/// Step applied when the shift key is held, in millivolts.
const FINE_STEP_MV: MilliVolts = 1_000;

/// Turbo speed factor.
const TURBO_FACTOR: u32 = 50;

/// Detents applied by a fast (shifted) encoder turn.
const FAST_DETENTS: i8 = 10;

/// Maximum number of ticks run between two redraws, so a turbo run cannot
/// starve the input loop.
const MAX_TICKS_PER_FRAME: u64 = 5_000;

/// Interval between two redraws of the terminal.
const FRAME_INTERVAL: Duration = Duration::from_millis(33);

/// Width of the bar used to visualise a CV input.
const BAR_WIDTH: i32 = 20;

/// A selectable keyboard layout: which physical keys reach the canonical
/// (QWERTY) bindings matched in [`Tui::on_key`].
///
/// A French AZERTY keyboard differs from QWERTY at exactly one relevant key
/// pair, W/Z (top-row-2nd swaps with bottom-row-leftmost) — every other
/// letter used by this UI sits at the same physical spot on both layouts.
/// [`Tui::canonicalize`] encodes only that swap.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum KeyLayout {
    #[default]
    Azerty,
    Qwerty,
}

impl KeyLayout {
    /// The other layout.
    const fn toggled(self) -> Self {
        match self {
            Self::Azerty => Self::Qwerty,
            Self::Qwerty => Self::Azerty,
        }
    }

    /// The name shown in the status bar and the key map.
    const fn label(self) -> &'static str {
        match self {
            Self::Azerty => "AZERTY",
            Self::Qwerty => "QWERTY",
        }
    }
}

/// The key map, permanently shown in the help panel: a header naming the
/// active layout, followed by one row per action with the key(s) to press
/// under that layout (see [`Tui::canonicalize`]) and its meaning, the two
/// aligned in columns.
fn help_lines(layout: KeyLayout) -> Vec<String> {
    let (pulse1, gate1, turn_left_ccw) = match layout {
        KeyLayout::Azerty => ("w", "W", "z"),
        KeyLayout::Qwerty => ("z", "Z", "w"),
    };

    let rows = [
        ("Tab".to_owned(), "focus a CV input".to_owned()),
        (
            "<-/->".to_owned(),
            "CV level +/-100mV (Shift: +/-1V)".to_owned(),
        ),
        ("Home".to_owned(), "CV level to 0V".to_owned()),
        ("p".to_owned(), "toggle the patch cable".to_owned()),
        (format!("{pulse1} x c v"), "pulse triggers 1-4".to_owned()),
        (format!("{gate1} X C V"), "hold triggers 1-4".to_owned()),
        (
            "up/down".to_owned(),
            "cycle the output mode (on release)".to_owned(),
        ),
        ("m".to_owned(), "up+down together: the app menu".to_owned()),
        (
            "Shift+up/down".to_owned(),
            "hold (both held = the app menu)".to_owned(),
        ),
        (
            format!("a,{turn_left_ccw}/e"),
            "press or turn the left encoder (Shift: x10)".to_owned(),
        ),
        (
            "r/t,y".to_owned(),
            "turn or press the right encoder (Shift: x10)".to_owned(),
        ),
        (
            "1/2/3".to_owned(),
            "speed: paused / real-time / 50x".to_owned(),
        ),
        ("Space".to_owned(), "single step while paused".to_owned()),
        ("o/0".to_owned(), "reset the module".to_owned()),
        ("l".to_owned(), "switch AZERTY / QWERTY".to_owned()),
        ("q/Esc".to_owned(), "quit".to_owned()),
    ];

    let key_width = rows
        .iter()
        .map(|(key, _)| key.chars().count())
        .max()
        .unwrap_or(0);

    let mut lines = Vec::with_capacity(rows.len() + 1);
    lines.push(format!("keyboard: {}", layout.label()));
    for (key, description) in &rows {
        lines.push(format!("{key:<key_width$}  {description}"));
    }
    lines
}

/// The interactive simulator session.
#[derive(Debug)]
pub struct Tui {
    simulator: Simulator,
    speed: Speed,
    focus: usize,
    layout: KeyLayout,
    last_advance: Instant,
    status: String,
    recording_path: Option<PathBuf>,
    quit: bool,
}

impl Tui {
    /// A session paused at tick zero.
    #[must_use]
    pub fn new() -> Self {
        Self {
            simulator: Simulator::new(),
            speed: Speed::Realtime,
            focus: 0,
            layout: KeyLayout::default(),
            last_advance: Instant::now(),
            status: String::new(),
            recording_path: None,
            quit: false,
        }
    }

    /// Records every input and writes the scenario to `path` on exit.
    pub fn record_to(&mut self, path: PathBuf) {
        self.simulator.start_recording();
        self.status = format!("recording to {}", path.display());
        self.recording_path = Some(path);
    }

    /// Runs the interface until the user quits.
    ///
    /// # Errors
    ///
    /// Fails if the terminal cannot be driven, or if a recording cannot be
    /// written on exit.
    pub fn run(mut self) -> Result<()> {
        let mut terminal = ratatui::init();
        let outcome = self.event_loop(&mut terminal);
        ratatui::restore();

        outcome?;
        self.save_recording()
    }

    /// The draw-and-poll loop.
    fn event_loop(&mut self, terminal: &mut ratatui::DefaultTerminal) -> Result<()> {
        while !self.quit {
            terminal
                .draw(|frame| self.draw(frame))
                .context("cannot draw the interface")?;

            if event::poll(FRAME_INTERVAL).context("cannot poll the terminal")? {
                if let event::Event::Key(key) = event::read().context("cannot read a key")? {
                    if key.kind == KeyEventKind::Press {
                        self.on_key(key);
                    }
                }
            }

            self.advance();
        }
        Ok(())
    }

    /// Writes the recorded scenario, if any.
    fn save_recording(&mut self) -> Result<()> {
        let (Some(path), Some(scenario)) =
            (self.recording_path.clone(), self.simulator.stop_recording())
        else {
            return Ok(());
        };
        std::fs::write(&path, scenario.to_string())
            .with_context(|| format!("cannot write {}", path.display()))?;
        println!("scenario written to {}", path.display());
        Ok(())
    }

    /// Runs the ticks owed since the previous call.
    fn advance(&mut self) {
        let now = Instant::now();
        let owed = self
            .speed
            .ticks_for(now.saturating_duration_since(self.last_advance));
        if owed == 0 {
            return;
        }
        self.last_advance = now;
        self.simulator.step_many(owed.min(MAX_TICKS_PER_FRAME));
    }

    /// Applies one keystroke.
    fn on_key(&mut self, key: KeyEvent) {
        if key.code == KeyCode::Char('l') {
            self.toggle_layout();
            return;
        }

        let shift = key.modifiers.contains(KeyModifiers::SHIFT);
        let step = if shift { FINE_STEP_MV } else { COARSE_STEP_MV };

        match self.canonicalize(key.code) {
            KeyCode::Char('q') | KeyCode::Esc => self.quit = true,

            KeyCode::Tab => {
                self.focus = (self.focus + 1) % CV_CHANNELS;
                self.status = format!("CV {} selected", self.focus + 1);
            }
            KeyCode::Left => self.nudge_focused(-step),
            KeyCode::Right => self.nudge_focused(step),
            KeyCode::Home => self.set_focused(0),
            KeyCode::Char('p') => self.toggle_patch(),

            KeyCode::Char('z') => self.pulse(TriggerChannel::One),
            KeyCode::Char('x') => self.pulse(TriggerChannel::Two),
            KeyCode::Char('c') => self.pulse(TriggerChannel::Three),
            KeyCode::Char('v') => self.pulse(TriggerChannel::Four),
            KeyCode::Char('Z') => self.toggle_gate(TriggerChannel::One),
            KeyCode::Char('X') => self.toggle_gate(TriggerChannel::Two),
            KeyCode::Char('C') => self.toggle_gate(TriggerChannel::Three),
            KeyCode::Char('V') => self.toggle_gate(TriggerChannel::Four),

            // A momentary press auto-releases after a few ticks (see
            // `Simulator::press`), which is normally too short a window for
            // two separate keystrokes to ever overlap. Shift+Up/Down instead
            // holds the button until pressed again, the same way Shift+ZXCV
            // holds a trigger — that is how the up+down chord that opens the
            // app menu is formed by hand. `m` presses both in one tick, which
            // is the same gesture in a single keystroke.
            KeyCode::Up if shift => self.toggle_button_hold(Button::Up),
            KeyCode::Down if shift => self.toggle_button_hold(Button::Down),
            KeyCode::Up => self.press(Button::Up),
            KeyCode::Down => self.press(Button::Down),
            KeyCode::Char('m') => self.chord(),

            // Row 1 (top letter row), six keys in a row, skipping only quit
            // (`q`) and patch (`p`): press-turn-turn for the left encoder,
            // turn-turn-press for the right one.
            KeyCode::Char('a') => self.press(Button::LeftEncoder),
            KeyCode::Char('w') => self.turn(0, -1),
            KeyCode::Char('W') => self.turn(0, -FAST_DETENTS),
            KeyCode::Char('e') => self.turn(0, 1),
            KeyCode::Char('E') => self.turn(0, FAST_DETENTS),
            KeyCode::Char('r') => self.turn(1, -1),
            KeyCode::Char('R') => self.turn(1, -FAST_DETENTS),
            KeyCode::Char('t') => self.turn(1, 1),
            KeyCode::Char('T') => self.turn(1, FAST_DETENTS),
            KeyCode::Char('y') => self.press(Button::RightEncoder),

            KeyCode::Char('1') => self.set_speed(Speed::Paused),
            KeyCode::Char('2') => self.set_speed(Speed::Realtime),
            KeyCode::Char('3') => self.set_speed(Speed::Turbo(TURBO_FACTOR)),
            KeyCode::Char(' ') => {
                self.simulator.step();
                self.status = format!("stepped to tick {}", self.simulator.tick_count());
            }

            KeyCode::Char('o' | '0') => self.reset(),

            _ => {}
        }
    }

    /// Rewrites the handful of keys that sit on different physical keys
    /// under AZERTY than under QWERTY into their QWERTY-canonical
    /// [`KeyCode`], the one matched in [`Tui::on_key`]. A no-op under
    /// [`KeyLayout::Qwerty`].
    ///
    /// AZERTY differs from QWERTY at exactly one relevant key pair: A/Q and
    /// W/Z swap position (top-row-leftmost ↔ home-row-leftmost, and
    /// top-row-2nd ↔ bottom-row-leftmost, respectively). `q`/`a` are left
    /// untouched — `q` is quit on both layouts, reached natively, and `a`
    /// is unused — so only `w`/`z` need rewriting.
    fn canonicalize(&self, code: KeyCode) -> KeyCode {
        if self.layout == KeyLayout::Qwerty {
            return code;
        }
        match code {
            KeyCode::Char('w') => KeyCode::Char('z'),
            KeyCode::Char('z') => KeyCode::Char('w'),
            KeyCode::Char('W') => KeyCode::Char('Z'),
            KeyCode::Char('Z') => KeyCode::Char('W'),
            other => other,
        }
    }

    /// Switches to the other keyboard layout.
    fn toggle_layout(&mut self) {
        self.layout = self.layout.toggled();
        self.status = format!("keyboard: {} (l to switch)", self.layout.label());
    }

    /// Restarts the module as if freshly powered on: the applet's state is
    /// discarded and the boot splash screen plays again.
    fn reset(&mut self) {
        self.simulator.reset();
        "module reset: booting again".clone_into(&mut self.status);
    }

    /// Changes the run speed.
    fn set_speed(&mut self, speed: Speed) {
        self.speed = speed;
        self.last_advance = Instant::now();
        self.status = match speed {
            Speed::Paused => "paused: SPACE steps one tick".to_owned(),
            Speed::Realtime => "running at 1 kHz".to_owned(),
            Speed::Turbo(factor) => format!("running at {factor}x"),
        };
    }

    /// Moves the focused CV input by `delta`, clamped to the input range.
    fn nudge_focused(&mut self, delta: MilliVolts) {
        let Some(channel) = CvChannel::from_index(self.focus) else {
            return;
        };
        let level = (self.simulator.cv_in(channel) + delta).clamp(CV_IN_MIN_MV, CV_IN_MAX_MV);
        self.set_focused(level);
    }

    /// Sets the focused CV input to an absolute level.
    fn set_focused(&mut self, millivolts: MilliVolts) {
        self.simulator.apply(Event::Cv {
            channel: self.focus,
            millivolts,
        });
    }

    /// Toggles the cable on the focused CV input.
    fn toggle_patch(&mut self) {
        let Some(channel) = CvChannel::from_index(self.focus) else {
            return;
        };
        let patched = !self.simulator.is_patched(channel);
        self.simulator.apply(Event::Patch {
            channel: self.focus,
            patched,
        });
        self.status = format!(
            "CV {} {}",
            self.focus + 1,
            if patched { "patched" } else { "unpatched" }
        );
    }

    /// Fires a momentary pulse on a trigger input.
    fn pulse(&mut self, channel: TriggerChannel) {
        self.simulator.pulse(channel);
        self.status = format!("TR {} pulsed", channel.index() + 1);
    }

    /// Toggles a trigger input between held high and held low.
    fn toggle_gate(&mut self, channel: TriggerChannel) {
        let high = !self.simulator.trigger_in(channel);
        self.simulator.apply(Event::Trigger {
            channel: channel.index(),
            high,
        });
        self.status = format!(
            "TR {} held {}",
            channel.index() + 1,
            if high { "high" } else { "low" }
        );
    }

    /// Turns an encoder.
    fn turn(&mut self, index: usize, detents: i8) {
        self.simulator.apply(Event::Encoder { index, detents });
    }

    /// Presses a button momentarily.
    fn press(&mut self, button: Button) {
        self.simulator.press(button);
    }

    /// Presses up and down in the same tick: the app-menu chord.
    fn chord(&mut self) {
        self.simulator.chord();
        "up+down: app menu".clone_into(&mut self.status);
    }

    /// Toggles a button between held down and released.
    fn toggle_button_hold(&mut self, button: Button) {
        let down = !self.simulator.button_held(button);
        self.simulator.apply(Event::Button { button, down });
        self.status = format!("{button:?} held {}", if down { "down" } else { "released" });
    }

    /// Draws the whole interface.
    fn draw(&mut self, frame: &mut Frame<'_>) {
        let [main, status] =
            Layout::vertical([Constraint::Min(0), Constraint::Length(3)]).areas(frame.area());
        let [inputs, module] =
            Layout::horizontal([Constraint::Length(34), Constraint::Min(0)]).areas(main);
        let [screen_row, outputs, help] = Layout::vertical([
            Constraint::Length(u16::try_from(braille::LINES).unwrap_or(16) + 2),
            Constraint::Length(5),
            Constraint::Min(0),
        ])
        .areas(module);
        // The module screen is a fixed 128x64 OLED (64x16 braille/octant glyphs):
        // it never grows past that, however much room the terminal offers.
        let [screen, _] = Layout::horizontal([
            Constraint::Length(u16::try_from(braille::COLUMNS).unwrap_or(64) + 2),
            Constraint::Min(0),
        ])
        .areas(screen_row);

        self.draw_inputs(frame, inputs);
        self.draw_screen(frame, screen);
        self.draw_outputs(frame, outputs);
        self.draw_help(frame, help);
        self.draw_status(frame, status);
    }

    /// Draws the CV, trigger and control state the user is driving.
    fn draw_inputs(&mut self, frame: &mut Frame<'_>, area: Rect) {
        let mut lines = Vec::new();

        for channel in CvChannel::ALL {
            let level = self.simulator.cv_in(channel);
            let patched = self.simulator.is_patched(channel);
            let focused = channel.index() == self.focus;
            let style = if focused {
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default()
            };
            lines.push(Line::styled(
                format!(
                    "{} CV{} {:>6}mV {}",
                    if focused { '>' } else { ' ' },
                    channel.index() + 1,
                    level,
                    if patched { "[cable]" } else { "       " }
                ),
                style,
            ));
            lines.push(Line::styled(format!("   {}", bar(level)), style));
        }

        lines.push(Line::raw(""));
        let gates: String = TriggerChannel::ALL
            .into_iter()
            .map(|channel| {
                if self.simulator.trigger_in(channel) {
                    '#'
                } else {
                    '.'
                }
            })
            .collect();
        lines.push(Line::raw(format!("  TR 1234  {gates}")));

        let counts: Vec<String> = TriggerChannel::ALL
            .into_iter()
            .map(|channel| {
                self.simulator
                    .diagnostic()
                    .trigger_count(channel)
                    .to_string()
            })
            .collect();
        lines.push(Line::raw(format!("  edges    {}", counts.join(" "))));

        lines.push(Line::raw(format!(
            "  presses  L{} R{} up{} dn{}",
            self.simulator
                .diagnostic()
                .button_press_count(Button::LeftEncoder),
            self.simulator
                .diagnostic()
                .button_press_count(Button::RightEncoder),
            self.simulator.diagnostic().button_press_count(Button::Up),
            self.simulator.diagnostic().button_press_count(Button::Down),
        )));

        frame.render_widget(
            Paragraph::new(lines).block(Block::default().borders(Borders::ALL).title(" inputs ")),
            area,
        );
    }

    /// Draws the module's OLED as terminal glyphs (braille, or octants with `--features octant`).
    fn draw_screen(&mut self, frame: &mut Frame<'_>, area: Rect) {
        let lines: Vec<Line<'_>> = braille::render(self.simulator.frame())
            .into_iter()
            .map(Line::raw)
            .collect();
        frame.render_widget(
            Paragraph::new(lines).block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(" module screen "),
            ),
            area,
        );
    }

    /// Draws the CV outputs and the timing report.
    fn draw_outputs(&mut self, frame: &mut Frame<'_>, area: Rect) {
        let levels = self.simulator.cv_out();
        let rendered: Vec<String> = levels
            .iter()
            .zip('A'..='D')
            .map(|(level, name)| format!("{name}:{level:>6}mV"))
            .collect();

        let (duration, ticks) = self
            .simulator
            .last_report()
            .map_or((0, 0), |report| (report.duration_micros, report.tick_count));

        // The second line describes whichever applet is running, so it stays
        // meaningful once the app menu has handed the panel to another one.
        let detail = match self.simulator.current_app() {
            AppId::Diagnostic => format!(
                "mode {}   offset {}mV   channel {}",
                self.simulator.diagnostic().mode().label(),
                self.simulator.diagnostic().offset(),
                self.simulator.diagnostic().selected() + 1
            ),
            AppId::Scope => format!("cv1 {}mV", self.simulator.scope().level()),
        };
        let menu = if self.simulator.menu_is_open() {
            "   [MENU]"
        } else {
            ""
        };

        let lines = vec![
            Line::raw(format!("  {}", rendered.join("  "))),
            Line::raw(format!("  {}{menu}", self.simulator.current_app().name())),
            Line::raw(format!("  {detail}")),
            Line::raw(format!("  tick {ticks}   cycle {duration}us")),
        ];

        frame.render_widget(
            Paragraph::new(lines).block(Block::default().borders(Borders::ALL).title(" outputs ")),
            area,
        );
    }

    /// Draws the permanent key map for the active keyboard layout.
    fn draw_help(&self, frame: &mut Frame<'_>, area: Rect) {
        let lines: Vec<Line<'_>> = help_lines(self.layout)
            .into_iter()
            .map(|line| Line::raw(format!(" {line}")))
            .collect();
        frame.render_widget(
            Paragraph::new(lines).block(Block::default().borders(Borders::ALL).title(" help ")),
            area,
        );
    }

    /// Draws the status bar.
    fn draw_status(&self, frame: &mut Frame<'_>, area: Rect) {
        let recording = if self.simulator.is_recording() {
            " REC "
        } else {
            ""
        };
        let line = Line::from(vec![
            Span::styled(
                format!(" {} ", self.speed.label()),
                Style::default().fg(Color::Black).bg(Color::Cyan),
            ),
            Span::styled(
                recording.to_owned(),
                Style::default().fg(Color::Black).bg(Color::Red),
            ),
            Span::raw(format!(" {}", self.status)),
        ]);
        frame.render_widget(
            Paragraph::new(line).block(Block::default().borders(Borders::ALL)),
            area,
        );
    }
}

impl Default for Tui {
    fn default() -> Self {
        Self::new()
    }
}

/// Renders a level as a centre-zero horizontal bar.
fn bar(millivolts: MilliVolts) -> String {
    let span = CV_IN_MAX_MV - CV_IN_MIN_MV;
    let clamped = millivolts.clamp(CV_IN_MIN_MV, CV_IN_MAX_MV);
    let filled = ((clamped - CV_IN_MIN_MV) * BAR_WIDTH / span).clamp(0, BAR_WIDTH);

    let mut rendered = String::with_capacity(usize::try_from(BAR_WIDTH).unwrap_or(0) + 2);
    rendered.push('[');
    for position in 0..BAR_WIDTH {
        rendered.push(match position.cmp(&filled) {
            std::cmp::Ordering::Less => '=',
            std::cmp::Ordering::Equal => '|',
            std::cmp::Ordering::Greater if position == BAR_WIDTH / 2 => '+',
            std::cmp::Ordering::Greater => ' ',
        });
    }
    rendered.push(']');
    rendered
}

/// Replays a scenario without opening a terminal, and reports the result.
///
/// # Errors
///
/// Fails if the scenario file cannot be read or parsed.
pub fn replay_headless(path: &std::path::Path) -> Result<String> {
    let text =
        std::fs::read_to_string(path).with_context(|| format!("cannot read {}", path.display()))?;
    let scenario: Scenario = text
        .parse()
        .with_context(|| format!("cannot parse {}", path.display()))?;

    let mut simulator = Simulator::new();
    simulator.replay(&scenario);

    let counts: Vec<u32> = TriggerChannel::ALL
        .into_iter()
        .map(|channel| simulator.diagnostic().trigger_count(channel))
        .collect();

    let mut report = String::new();
    let _ = writeln!(report, "ticks   {}", simulator.tick_count());
    let _ = writeln!(report, "app     {}", simulator.current_app().name());
    let _ = writeln!(
        report,
        "menu    {}",
        if simulator.menu_is_open() {
            "open"
        } else {
            "closed"
        }
    );
    let _ = writeln!(report, "mode    {}", simulator.diagnostic().mode().label());
    let _ = writeln!(report, "offset  {}mV", simulator.diagnostic().offset());
    let _ = writeln!(
        report,
        "presses L{} R{} up{} dn{}",
        simulator
            .diagnostic()
            .button_press_count(Button::LeftEncoder),
        simulator
            .diagnostic()
            .button_press_count(Button::RightEncoder),
        simulator.diagnostic().button_press_count(Button::Up),
        simulator.diagnostic().button_press_count(Button::Down),
    );
    let _ = writeln!(report, "scope   {}mV", simulator.scope().level());
    let _ = writeln!(report, "cv out  {:?}", simulator.cv_out());
    let _ = writeln!(report, "edges   {counts:?}");
    report.push_str("screen\n");
    for line in braille::render(simulator.frame()) {
        let _ = writeln!(report, "  {line}");
    }
    Ok(report)
}

#[cfg(test)]
mod tests {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    use oc_core::apps::AppId;
    use oc_core::calibration::{CV_IN_MAX_MV, CV_IN_MIN_MV};
    use oc_core::platform::{Button, TriggerChannel};

    use super::{KeyLayout, Tui, bar};
    use crate::braille;
    use crate::simulator::{PRESS_TICKS, SETTLE_TICKS};

    /// A plain, unmodified key press (no shift/ctrl/alt).
    fn key(c: char) -> KeyEvent {
        KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE)
    }

    fn press_and_settle(tui: &mut Tui, c: char) {
        tui.on_key(key(c));
        tui.simulator.step_many(u64::from(PRESS_TICKS) + 4);
    }

    #[test]
    fn the_default_layout_is_azerty() {
        let tui = Tui::new();
        assert_eq!(tui.layout, KeyLayout::Azerty);
    }

    #[test]
    fn under_azerty_w_pulses_trigger_one() {
        let mut tui = Tui::new();
        tui.simulator.skip_splash();

        press_and_settle(&mut tui, 'w');
        assert_eq!(
            tui.simulator
                .diagnostic()
                .trigger_count(TriggerChannel::One),
            1,
            "w is trigger 1's canonical key on AZERTY"
        );
    }

    #[test]
    fn a_plain_up_then_down_press_do_not_overlap_and_the_menu_stays_shut() {
        let mut tui = Tui::new();
        tui.simulator.skip_splash();

        tui.on_key(KeyEvent::new(KeyCode::Up, KeyModifiers::NONE));
        tui.simulator.step_many(u64::from(SETTLE_TICKS)); // Up fully auto-releases
        tui.on_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
        tui.simulator.step_many(u64::from(SETTLE_TICKS));

        assert!(
            !tui.simulator.menu_is_open(),
            "the two presses never overlapped, so the chord must not fire"
        );
    }

    #[test]
    fn shift_up_then_shift_down_holds_both_and_opens_the_app_menu() {
        let mut tui = Tui::new();
        tui.simulator.skip_splash();

        tui.on_key(KeyEvent::new(KeyCode::Up, KeyModifiers::SHIFT));
        tui.simulator.step_many(2);
        assert!(tui.simulator.button_held(Button::Up));

        tui.on_key(KeyEvent::new(KeyCode::Down, KeyModifiers::SHIFT));
        tui.simulator.step_many(5);

        assert!(
            tui.simulator.menu_is_open(),
            "holding both via Shift+Up/Shift+Down reliably overlaps and opens the menu"
        );

        tui.on_key(KeyEvent::new(KeyCode::Up, KeyModifiers::SHIFT));
        tui.on_key(KeyEvent::new(KeyCode::Down, KeyModifiers::SHIFT));
        assert!(!tui.simulator.button_held(Button::Up));
        assert!(!tui.simulator.button_held(Button::Down));
    }

    #[test]
    fn m_opens_and_closes_the_app_menu() {
        let mut tui = Tui::new();
        tui.simulator.skip_splash();

        press_and_settle(&mut tui, 'm');
        assert!(
            tui.simulator.menu_is_open(),
            "m forms the chord in one keystroke"
        );

        press_and_settle(&mut tui, 'm');
        assert!(!tui.simulator.menu_is_open(), "the chord toggles the menu");
    }

    #[test]
    fn the_menu_launches_the_scope_from_the_keyboard() {
        let mut tui = Tui::new();
        tui.simulator.skip_splash();

        press_and_settle(&mut tui, 'm');
        tui.on_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
        tui.simulator.step_many(u64::from(SETTLE_TICKS));
        press_and_settle(&mut tui, 'a');

        assert!(!tui.simulator.menu_is_open());
        assert_eq!(tui.simulator.current_app(), AppId::Scope);
    }

    #[test]
    fn a_presses_the_left_encoder_and_resets_trigger_counters_on_either_layout() {
        let mut tui = Tui::new();
        tui.simulator.skip_splash();
        press_and_settle(&mut tui, 'w');
        assert_eq!(
            tui.simulator
                .diagnostic()
                .trigger_count(TriggerChannel::One),
            1
        );

        press_and_settle(&mut tui, 'a');
        assert_eq!(
            tui.simulator
                .diagnostic()
                .trigger_count(TriggerChannel::One),
            0,
            "a presses the left encoder on both layouts, which resets the trigger counters"
        );
    }

    #[test]
    fn q_always_quits_regardless_of_layout() {
        let mut tui = Tui::new();
        assert!(!tui.quit);
        tui.on_key(key('q'));
        assert!(tui.quit, "q quits under AZERTY");

        let mut tui = Tui::new();
        tui.on_key(key('l'));
        assert!(!tui.quit);
        tui.on_key(key('q'));
        assert!(tui.quit, "q quits under QWERTY too");
    }

    #[test]
    fn toggling_the_layout_swaps_which_key_pulses_trigger_one() {
        let mut tui = Tui::new();
        tui.simulator.skip_splash();
        assert_eq!(tui.layout, KeyLayout::Azerty);

        tui.on_key(key('l'));
        assert_eq!(tui.layout, KeyLayout::Qwerty);

        press_and_settle(&mut tui, 'z');
        assert_eq!(
            tui.simulator
                .diagnostic()
                .trigger_count(TriggerChannel::One),
            1,
            "z is trigger 1's canonical key, reached natively under QWERTY"
        );

        press_and_settle(&mut tui, 'w');
        assert_eq!(
            tui.simulator
                .diagnostic()
                .trigger_count(TriggerChannel::One),
            1,
            "w turns the left encoder under QWERTY (native), not trigger 1 nor its reset"
        );
    }

    /// Renders `tui` into a terminal of the given size and returns the
    /// width, in columns, of the module screen panel's border on its first
    /// row: from where it starts (right of the fixed-width inputs column)
    /// to its top-right corner (`'┐'`).
    fn rendered_screen_panel_width(tui: &mut Tui, terminal_width: u16) -> u16 {
        let mut terminal = Terminal::new(TestBackend::new(terminal_width, 40)).unwrap();
        terminal.draw(|frame| tui.draw(frame)).unwrap();

        let buffer = terminal.backend().buffer();
        let inputs_width = 34;
        let corner = (inputs_width..terminal_width)
            .find(|&x| buffer.cell((x, 0)).is_some_and(|cell| cell.symbol() == "┐"))
            .expect("the module screen panel must have a top-right corner");
        corner + 1 - inputs_width
    }

    #[test]
    fn the_module_screen_panel_keeps_its_128x64_size_however_wide_the_terminal_is() {
        let expected_width = u16::try_from(braille::COLUMNS).unwrap_or(64) + 2;

        for terminal_width in [100, 160, 220] {
            let mut tui = Tui::new();
            tui.simulator.skip_splash();
            assert_eq!(
                rendered_screen_panel_width(&mut tui, terminal_width),
                expected_width,
                "terminal width {terminal_width}"
            );
        }
    }

    /// All the text currently in the rendered buffer, concatenated without
    /// separators (good enough for a substring check, not for layout).
    fn rendered_text(tui: &mut Tui, terminal_width: u16, terminal_height: u16) -> String {
        let mut terminal =
            Terminal::new(TestBackend::new(terminal_width, terminal_height)).unwrap();
        terminal.draw(|frame| tui.draw(frame)).unwrap();
        terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(ratatui::buffer::Cell::symbol)
            .collect()
    }

    #[test]
    fn the_help_panel_is_always_visible_and_reflects_the_active_layout() {
        let mut tui = Tui::new();
        tui.simulator.skip_splash();

        let rendered = rendered_text(&mut tui, 120, 50);
        assert!(rendered.contains("help"), "the help panel has a title");
        assert!(
            rendered.contains("keyboard: AZERTY"),
            "the help panel shows the active layout without needing to press '?'"
        );
        assert!(
            rendered.contains("quit"),
            "the panel is tall enough to show the whole key map, key and meaning aligned"
        );

        tui.on_key(key('l'));
        let rendered = rendered_text(&mut tui, 120, 50);
        assert!(
            rendered.contains("keyboard: QWERTY"),
            "the help panel updates when the layout is toggled"
        );
    }

    #[test]
    fn the_bar_is_empty_at_the_bottom_of_the_range() {
        let rendered = bar(CV_IN_MIN_MV);
        assert!(rendered.starts_with("[|"), "{rendered}");
    }

    #[test]
    fn the_bar_is_full_at_the_top_of_the_range() {
        let rendered = bar(CV_IN_MAX_MV);
        assert!(rendered.ends_with("=]"), "{rendered}");
    }

    #[test]
    fn the_bar_marks_the_centre_when_below_zero() {
        let rendered = bar(CV_IN_MIN_MV / 2);
        assert!(rendered.contains('+'), "{rendered}");
    }

    #[test]
    fn out_of_range_levels_do_not_panic() {
        assert_eq!(bar(i32::MAX).len(), bar(CV_IN_MAX_MV).len());
        assert_eq!(bar(i32::MIN).len(), bar(CV_IN_MIN_MV).len());
    }
}
