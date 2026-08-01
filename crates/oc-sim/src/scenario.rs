//! Recording and replaying input scenarios.
//!
//! A scenario is a plain text file: one event per line, prefixed by the tick at
//! which it applies. Being text means a failing run can be committed as a
//! regression test, edited by hand, and reviewed in a diff.
//!
//! ```text
//! # oc-sim scenario v1
//! ticks 50
//! 0  cv 1 2500
//! 0  patch 1 on
//! 10 trigger 2 high
//! 12 trigger 2 low
//! 20 encoder 2 +3
//! 25 button up down
//! 28 button up up
//! ```

use std::fmt::Write as _;
use std::str::FromStr;

use anyhow::{Context, Result, anyhow, bail};

use oc_core::platform::{BUTTONS, Button, CV_CHANNELS, ENCODERS, TRIGGER_CHANNELS};

/// Header written at the top of every recorded scenario.
const HEADER: &str = "# oc-sim scenario v1";

/// One input change applied to the simulated module.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Event {
    /// Set the level of a CV input, in millivolts.
    Cv {
        /// Zero-based channel index.
        channel: usize,
        /// Level in millivolts.
        millivolts: i32,
    },
    /// Report a cable as present or absent on a CV input.
    Patch {
        /// Zero-based channel index.
        channel: usize,
        /// Whether a cable is present.
        patched: bool,
    },
    /// Set the level of a trigger input.
    Trigger {
        /// Zero-based channel index.
        channel: usize,
        /// Whether the gate is high.
        high: bool,
    },
    /// Turn an encoder by a number of detents.
    Encoder {
        /// Zero-based encoder index.
        index: usize,
        /// Detents travelled, positive clockwise.
        detents: i8,
    },
    /// Hold or release a button.
    Button {
        /// Which button.
        button: Button,
        /// Whether it is held down.
        down: bool,
    },
}

/// Name used for a button in the text format.
const fn button_name(button: Button) -> &'static str {
    match button {
        Button::LeftEncoder => "left",
        Button::RightEncoder => "right",
        Button::Up => "up",
        Button::Down => "down",
    }
}

/// Parses a button name.
fn parse_button(name: &str) -> Result<Button> {
    Button::ALL
        .into_iter()
        .find(|&button| button_name(button) == name)
        .ok_or_else(|| anyhow!("unknown button {name:?}"))
}

/// Parses a `1`-based channel index against an upper bound.
fn parse_channel(token: &str, count: usize) -> Result<usize> {
    let index: usize = token
        .parse()
        .with_context(|| format!("{token:?} is not a channel number"))?;
    if index == 0 || index > count {
        bail!("channel {index} is outside 1..={count}");
    }
    Ok(index - 1)
}

/// Parses `on`/`off`, `high`/`low` or `down`/`up`.
fn parse_state(token: &str) -> Result<bool> {
    match token {
        "on" | "high" | "down" | "true" | "1" => Ok(true),
        "off" | "low" | "up" | "false" | "0" => Ok(false),
        other => bail!("{other:?} is not a state"),
    }
}

impl Event {
    /// Renders the event in the text format, without its tick prefix.
    fn write_to(self, out: &mut String) {
        match self {
            Self::Cv {
                channel,
                millivolts,
            } => {
                let _ = write!(out, "cv {} {millivolts}", channel + 1);
            }
            Self::Patch { channel, patched } => {
                let state = if patched { "on" } else { "off" };
                let _ = write!(out, "patch {} {state}", channel + 1);
            }
            Self::Trigger { channel, high } => {
                let state = if high { "high" } else { "low" };
                let _ = write!(out, "trigger {} {state}", channel + 1);
            }
            Self::Encoder { index, detents } => {
                let _ = write!(out, "encoder {} {detents:+}", index + 1);
            }
            Self::Button { button, down } => {
                let state = if down { "down" } else { "up" };
                let _ = write!(out, "button {} {state}", button_name(button));
            }
        }
    }

    /// Parses one event from its whitespace-separated tokens.
    fn parse(tokens: &[&str]) -> Result<Self> {
        let (kind, arguments) = tokens.split_first().ok_or_else(|| anyhow!("empty event"))?;

        let event = match (*kind, arguments) {
            ("cv", [channel, millivolts]) => Self::Cv {
                channel: parse_channel(channel, CV_CHANNELS)?,
                millivolts: millivolts
                    .parse()
                    .with_context(|| format!("{millivolts:?} is not a level in millivolts"))?,
            },
            ("patch", [channel, state]) => Self::Patch {
                channel: parse_channel(channel, CV_CHANNELS)?,
                patched: parse_state(state)?,
            },
            ("trigger", [channel, state]) => Self::Trigger {
                channel: parse_channel(channel, TRIGGER_CHANNELS)?,
                high: parse_state(state)?,
            },
            ("encoder", [index, detents]) => Self::Encoder {
                index: parse_channel(index, ENCODERS)?,
                detents: detents
                    .parse()
                    .with_context(|| format!("{detents:?} is not a detent count"))?,
            },
            ("button", [name, state]) => Self::Button {
                button: parse_button(name)?,
                down: parse_state(state)?,
            },
            (kind, arguments) => {
                bail!(
                    "unknown event {kind:?} with {} argument(s)",
                    arguments.len()
                );
            }
        };
        Ok(event)
    }
}

/// A reproducible sequence of inputs.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Scenario {
    /// Events, sorted by the tick at which they apply.
    pub events: Vec<(u64, Event)>,
    /// Total number of ticks to run.
    pub ticks: u64,
}

impl Scenario {
    /// An empty scenario of `ticks` ticks.
    #[must_use]
    pub const fn new(ticks: u64) -> Self {
        Self {
            events: Vec::new(),
            ticks,
        }
    }

    /// Appends an event, keeping the list sorted by tick.
    pub fn push(&mut self, tick: u64, event: Event) {
        let position = self.events.partition_point(|&(at, _)| at <= tick);
        self.events.insert(position, (tick, event));
        self.ticks = self.ticks.max(tick + 1);
    }

    /// Events that apply exactly at `tick`.
    pub fn events_at(&self, tick: u64) -> impl Iterator<Item = Event> + '_ {
        self.events
            .iter()
            .filter(move |&&(at, _)| at == tick)
            .map(|&(_, event)| event)
    }
}

impl std::fmt::Display for Scenario {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "{HEADER}")?;
        writeln!(f, "ticks {}", self.ticks)?;
        for &(tick, event) in &self.events {
            let mut rendered = String::new();
            event.write_to(&mut rendered);
            writeln!(f, "{tick} {rendered}")?;
        }
        Ok(())
    }
}

impl FromStr for Scenario {
    type Err = anyhow::Error;

    fn from_str(text: &str) -> Result<Self> {
        let mut scenario = Self::default();

        for (number, raw) in text.lines().enumerate() {
            let line = raw.split('#').next().unwrap_or("").trim();
            if line.is_empty() {
                continue;
            }

            parse_line(&mut scenario, line)
                .with_context(|| format!("line {}: {raw:?}", number + 1))?;
        }

        Ok(scenario)
    }
}

/// Parses one non-empty, comment-free line into `scenario`.
fn parse_line(scenario: &mut Scenario, line: &str) -> Result<()> {
    let tokens: Vec<&str> = line.split_whitespace().collect();

    if tokens[0] == "ticks" {
        let [_, count] = tokens[..] else {
            bail!("`ticks` takes exactly one argument");
        };
        scenario.ticks = count
            .parse()
            .with_context(|| format!("{count:?} is not a tick count"))?;
        return Ok(());
    }

    let tick: u64 = tokens[0]
        .parse()
        .with_context(|| format!("{:?} is not a tick number", tokens[0]))?;
    scenario.push(tick, Event::parse(&tokens[1..])?);
    Ok(())
}

/// Compile-time reminder that the text format covers every button.
const _: () = assert!(BUTTONS == 4);

#[cfg(test)]
mod tests {
    use oc_core::platform::Button;

    use super::{Event, Scenario};

    #[test]
    fn a_scenario_survives_a_text_round_trip() {
        let mut scenario = Scenario::new(0);
        scenario.push(
            0,
            Event::Cv {
                channel: 0,
                millivolts: 2_500,
            },
        );
        scenario.push(
            0,
            Event::Patch {
                channel: 0,
                patched: true,
            },
        );
        scenario.push(
            10,
            Event::Trigger {
                channel: 1,
                high: true,
            },
        );
        scenario.push(
            20,
            Event::Encoder {
                index: 1,
                detents: -3,
            },
        );
        scenario.push(
            25,
            Event::Button {
                button: Button::Up,
                down: true,
            },
        );

        let text = scenario.to_string();
        let parsed: Scenario = text.parse().expect("the rendered scenario must parse");
        assert_eq!(parsed, scenario, "round trip through:\n{text}");
    }

    #[test]
    fn comments_and_blank_lines_are_ignored() {
        let text = "# a comment\n\nticks 5\n1 cv 2 -1000 # trailing comment\n";
        let scenario: Scenario = text.parse().unwrap();
        assert_eq!(scenario.ticks, 5);
        assert_eq!(
            scenario.events,
            vec![(
                1,
                Event::Cv {
                    channel: 1,
                    millivolts: -1_000
                }
            )]
        );
    }

    #[test]
    fn events_are_kept_in_tick_order() {
        let scenario: Scenario = "5 cv 1 100\n1 cv 1 200\n3 cv 1 300\n".parse().unwrap();
        let ticks: Vec<u64> = scenario.events.iter().map(|&(tick, _)| tick).collect();
        assert_eq!(ticks, vec![1, 3, 5]);
        assert_eq!(scenario.ticks, 6, "the run must cover the last event");
    }

    #[test]
    fn events_can_be_selected_by_tick() {
        let scenario: Scenario = "2 cv 1 100\n2 patch 1 on\n3 cv 1 300\n".parse().unwrap();
        assert_eq!(scenario.events_at(2).count(), 2);
        assert_eq!(scenario.events_at(7).count(), 0);
    }

    #[test]
    fn an_out_of_range_channel_is_rejected() {
        let error = "0 cv 9 100".parse::<Scenario>().unwrap_err();
        let message = format!("{error:#}");
        assert!(message.contains("channel 9"), "{message}");
        assert!(message.contains("line 1"), "{message}");
    }

    #[test]
    fn a_zero_channel_is_rejected() {
        assert!("0 cv 0 100".parse::<Scenario>().is_err());
    }

    #[test]
    fn an_unknown_event_is_rejected() {
        let error = "0 wiggle 1 2".parse::<Scenario>().unwrap_err();
        assert!(format!("{error:#}").contains("wiggle"));
    }

    #[test]
    fn a_malformed_state_is_rejected() {
        assert!("0 patch 1 maybe".parse::<Scenario>().is_err());
    }

    #[test]
    fn a_missing_argument_is_rejected() {
        assert!("0 cv 1".parse::<Scenario>().is_err());
    }

    #[test]
    fn an_unknown_button_is_rejected() {
        assert!("0 button middle down".parse::<Scenario>().is_err());
    }
}
