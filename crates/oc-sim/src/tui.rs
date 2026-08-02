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

/// Maximum number of ticks run between two redraws, so a turbo run cannot
/// starve the input loop.
const MAX_TICKS_PER_FRAME: u64 = 5_000;

/// Interval between two redraws of the terminal.
const FRAME_INTERVAL: Duration = Duration::from_millis(33);

/// Width of the bar used to visualise a CV input.
const BAR_WIDTH: i32 = 20;

/// The key map, shown in the status bar on demand.
const KEY_MAP: &str = concat!(
    "TAB focus  <-/-> level  p patch  zxcv pulse  ZXCV gate  ",
    "[ ] , . encoders  ENTER/b press  up/down mode  1/2/3 speed  SPACE step  ",
    "r reset  q quit"
);

/// The interactive simulator session.
#[derive(Debug)]
pub struct Tui {
    simulator: Simulator,
    speed: Speed,
    focus: usize,
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
            last_advance: Instant::now(),
            status: "press ? for the key map".to_owned(),
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
        let shift = key.modifiers.contains(KeyModifiers::SHIFT);
        let step = if shift { FINE_STEP_MV } else { COARSE_STEP_MV };

        match key.code {
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

            KeyCode::Char('[') => self.turn(0, -1),
            KeyCode::Char(']') => self.turn(0, 1),
            KeyCode::Char(',') => self.turn(1, -1),
            KeyCode::Char('.') => self.turn(1, 1),
            KeyCode::Char('<') => self.turn(1, -10),
            KeyCode::Char('>') => self.turn(1, 10),

            KeyCode::Enter => self.press(Button::LeftEncoder),
            KeyCode::Char('b') => self.press(Button::RightEncoder),
            KeyCode::Up => self.press(Button::Up),
            KeyCode::Down => self.press(Button::Down),

            KeyCode::Char('1') => self.set_speed(Speed::Paused),
            KeyCode::Char('2') => self.set_speed(Speed::Realtime),
            KeyCode::Char('3') => self.set_speed(Speed::Turbo(TURBO_FACTOR)),
            KeyCode::Char(' ') => {
                self.simulator.step();
                self.status = format!("stepped to tick {}", self.simulator.tick_count());
            }

            KeyCode::Char('r') => self.reset(),

            KeyCode::Char('?') => KEY_MAP.clone_into(&mut self.status),
            _ => {}
        }
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

    /// Draws the whole interface.
    fn draw(&mut self, frame: &mut Frame<'_>) {
        let [main, status] =
            Layout::vertical([Constraint::Min(0), Constraint::Length(3)]).areas(frame.area());
        let [inputs, module] =
            Layout::horizontal([Constraint::Length(34), Constraint::Min(0)]).areas(main);
        let [screen_row, outputs] = Layout::vertical([
            Constraint::Length(u16::try_from(braille::LINES).unwrap_or(16) + 2),
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
            .map(|channel| self.simulator.app().trigger_count(channel).to_string())
            .collect();
        lines.push(Line::raw(format!("  edges    {}", counts.join(" "))));

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

        let lines = vec![
            Line::raw(format!("  {}", rendered.join("  "))),
            Line::raw(format!(
                "  mode {}   offset {}mV   channel {}",
                self.simulator.app().mode().label(),
                self.simulator.app().offset(),
                self.simulator.app().selected() + 1
            )),
            Line::raw(format!("  tick {ticks}   cycle {duration}us")),
        ];

        frame.render_widget(
            Paragraph::new(lines).block(Block::default().borders(Borders::ALL).title(" outputs ")),
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
        .map(|channel| simulator.app().trigger_count(channel))
        .collect();

    let mut report = String::new();
    let _ = writeln!(report, "ticks   {}", simulator.tick_count());
    let _ = writeln!(report, "mode    {}", simulator.app().mode().label());
    let _ = writeln!(report, "offset  {}mV", simulator.app().offset());
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

    use oc_core::calibration::{CV_IN_MAX_MV, CV_IN_MIN_MV};

    use super::{Tui, bar};
    use crate::braille;

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
