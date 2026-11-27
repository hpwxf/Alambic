//! The applets as the host reads and writes them (journal/D-043): finding
//! one and its parameters by name, a value as text, and text back into a
//! value of the parameter's kind; a parameter's CV input the same way
//! (journal/D-045).
//!
//! The text of a number is the module's own ([`write_number`]), so a value
//! reads the same in `cargo probe`, in the simulator's status line and in
//! the browser; an input's fields read as the module's submenu writes them
//! ([`Assign::field`]).

use std::fmt::Write as _;

use oc_core::apps::param::{Unit, write_number};
use oc_core::modulation::{AssignLine, CENTER_MIN_SPAN_MV, Source, usable_range};
use oc_core::platform::CvChannel;
use oc_protocol::{Assign, Effect, InputSource, ParamKind, ParamValue, Sample};

use crate::client::{AppDescription, AppEntry, InputEntry, InstanceEntry, SpecEntry};
use crate::measure;

/// A name as the user may type it: case and anything but letters and
/// digits ignored, so `gfx-demo` finds `GFX DEMO`.
fn folded(name: &str) -> String {
    name.chars()
        .filter(char::is_ascii_alphanumeric)
        .map(|c| c.to_ascii_lowercase())
        .collect()
}

/// The applet `query` names: its index, or its name.
#[must_use]
pub fn find_app<'a>(apps: &'a [AppEntry], query: &str) -> Option<&'a AppEntry> {
    if let Ok(index) = query.parse::<u8>() {
        return apps.iter().find(|app| app.index == index);
    }
    let query = folded(query);
    apps.iter().find(|app| folded(&app.name) == query)
}

/// The entry `query` names in `list`: its index, its name, or its label
/// now (`span` finds `RATCHET`'s `FIRST` in `MODE SPAN`).
#[must_use]
pub fn find_spec<'a>(list: &'a [SpecEntry], query: &str) -> Option<&'a SpecEntry> {
    if let Ok(index) = query.parse::<u8>() {
        return list.iter().find(|spec| spec.index == index);
    }
    let query = folded(query);
    list.iter()
        .find(|spec| folded(&spec.name) == query)
        .or_else(|| list.iter().find(|spec| folded(&spec.label) == query))
}

/// The action `query` names: its index, or its name.
#[must_use]
pub fn find_action(actions: &[String], query: &str) -> Option<u8> {
    if let Ok(index) = query.parse::<u8>() {
        return (usize::from(index) < actions.len()).then_some(index);
    }
    let query = folded(query);
    actions
        .iter()
        .position(|name| folded(name) == query)
        .and_then(|index| u8::try_from(index).ok())
}

/// The instance position `query` names when it is written `@N`
/// (journal/D-046); `None` for an applet's name or index.
#[must_use]
pub fn instance_position(query: &str) -> Option<u8> {
    query.strip_prefix('@')?.parse().ok()
}

/// What a port of an instance is: an output, or an input.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PortRef {
    /// Output port `n`.
    Output(u8),
    /// Input port `n`, reading a trigger when `trigger`.
    Input(u8, bool),
}

/// The port `query` names in `description`: an output's name first, then an
/// input's, case and punctuation ignored.
#[must_use]
pub fn find_port(description: &AppDescription, query: &str) -> Option<PortRef> {
    let query = folded(query);
    if let Some(index) = description
        .outputs
        .iter()
        .position(|port| folded(&port.name) == query)
    {
        return u8::try_from(index).ok().map(PortRef::Output);
    }
    let index = description
        .inputs
        .iter()
        .position(|port| folded(&port.name) == query)?;
    let trigger = description.inputs[index].trigger;
    u8::try_from(index)
        .ok()
        .map(|index| PortRef::Input(index, trigger))
}

/// The output `text` names, `a` to `d`, or `None` for `off`.
///
/// # Errors
///
/// Anything else, said in words.
pub fn parse_output(text: &str) -> Result<Option<u8>, String> {
    match folded(text).as_str() {
        "off" | "none" => Ok(None),
        "a" => Ok(Some(0)),
        "b" => Ok(Some(1)),
        "c" => Ok(Some(2)),
        "d" => Ok(Some(3)),
        _ => Err(format!("{text}: an output is A, B, C, D or OFF")),
    }
}

/// What `text` plugs an input port into: a jack of its kind, `cv1` to
/// `cv4` or `tr1` to `tr4` for a trigger, or a port of another instance by
/// its slot and the name `ports` gives it, `1:burst`, or its rank, `1:#2`
/// (journal/D-048).
///
/// # Errors
///
/// A jack of the other kind, or nothing it names.
pub fn parse_input_source(
    text: &str,
    trigger: bool,
    ports: &PortNames,
) -> Result<InputSource, String> {
    if text.contains(':') {
        let (slot, port) = parse_lane(&text.to_ascii_lowercase(), ports)
            .ok_or_else(|| format!("{text}: a slot and a port's name, as 1:burst"))?;
        return InputSource::lane(slot, port).ok_or_else(|| format!("{text}: no such port"));
    }
    parse_jack(text, trigger).map(InputSource::Jack)
}

/// The jack `text` names among a port's kind: `cv1` to `cv4`, or `tr1` to
/// `tr4` for a trigger, 0 for the first.
///
/// # Errors
///
/// A jack of the other kind, or none at all.
pub fn parse_jack(text: &str, trigger: bool) -> Result<u8, String> {
    let folded = folded(text);
    let (prefix, other) = if trigger { ("tr", "cv") } else { ("cv", "tr") };
    if folded.starts_with(other) {
        return Err(format!("{text}: this input reads a {prefix} jack"));
    }
    folded
        .strip_prefix(prefix)
        .and_then(|number| number.parse::<u8>().ok())
        .filter(|number| (1..=4).contains(number))
        .map(|number| number - 1)
        .ok_or_else(|| format!("{text}: a jack is {prefix}1 to {prefix}4"))
}

/// Where an instance's ports go, one per line: `SUB -> OFF`, `CLOCK <- TR2`,
/// `CLOCK <- 1:BURST` for a port of another instance `ports` names.
#[must_use]
pub fn routing_text(
    description: &AppDescription,
    instance: &InstanceEntry,
    ports: &PortNames,
) -> String {
    const OUTPUTS: [&str; 4] = ["A", "B", "C", "D"];
    let mut text = String::new();
    for (index, port) in description.outputs.iter().enumerate() {
        let jack = instance
            .outputs
            .get(index)
            .copied()
            .flatten()
            .and_then(|jack| OUTPUTS.get(usize::from(jack)))
            .copied()
            .unwrap_or("OFF");
        let _ = writeln!(text, "{} -> {jack}", port.name);
    }
    for (index, port) in description.inputs.iter().enumerate() {
        let source = instance
            .inputs
            .get(index)
            .copied()
            .unwrap_or(InputSource::Jack(0));
        let _ = writeln!(
            text,
            "{} <- {}",
            port.name,
            input_source_text(source, port.trigger, ports)
        );
    }
    text
}

/// What an input port reading `source` shows: its jack (`TR2`, `CV1`), or
/// the slot and name of the port it reads, `1:BURST` (journal/D-048).
#[must_use]
pub fn input_source_text(source: InputSource, trigger: bool, ports: &PortNames) -> String {
    match source {
        InputSource::Jack(jack) => {
            format!(
                "{}{}",
                if trigger { "TR" } else { "CV" },
                u32::from(jack) + 1
            )
        }
        InputSource::Lane { slot, port } => source_text(Source::Lane { slot, port }, ports),
    }
}

/// `value` as the module writes it: `-3.0 OCT`, `50.0%`, `SPAN`, `A4`, `ON`.
#[must_use]
pub fn value_text(spec: &SpecEntry, value: ParamValue) -> String {
    let mut text = String::new();
    match (spec.kind, value) {
        (
            ParamKind::Number {
                min,
                decimals,
                unit,
                ..
            },
            ParamValue::Number(number),
        ) => {
            let _ = write_number(number, decimals, unit, min < 0, &mut text);
        }
        (ParamKind::Choice { .. }, ParamValue::Choice(index)) => {
            text.push_str(
                spec.choices
                    .get(usize::from(index))
                    .map_or("?", String::as_str),
            );
        }
        (_, ParamValue::Toggle(on)) => text.push_str(if on { "ON" } else { "OFF" }),
        _ => text.push('?'),
    }
    text
}

/// `value` as one types it back: as [`value_text`] without a unit, so that
/// [`parse_value`] reads it unchanged (`-3.0`, `A4`, `SPAN`, `ON`).
#[must_use]
pub fn edit_text(spec: &SpecEntry, value: ParamValue) -> String {
    match (spec.kind, value) {
        (
            ParamKind::Number {
                min,
                decimals,
                unit,
                ..
            },
            ParamValue::Number(number),
        ) if unit != Unit::Note => {
            let mut text = String::new();
            let _ = write_number(number, decimals, Unit::None, min < 0, &mut text);
            text
        }
        _ => value_text(spec, value),
    }
}

/// `text` as a value of `spec`'s kind: a number with up to its decimals
/// (`-1.5`), a note's name or number (`A4`, `69`), a choice's name or index,
/// `on` or `off`. The range is the module's to check.
///
/// # Errors
///
/// What `text` should have looked like.
pub fn parse_value(spec: &SpecEntry, text: &str) -> Result<ParamValue, String> {
    let text = text.trim();
    match spec.kind {
        ParamKind::Number {
            unit: Unit::Note, ..
        } => text
            .parse::<i32>()
            .ok()
            .or_else(|| note_number(text))
            .map(ParamValue::Number)
            .ok_or_else(|| format!("{}: a note such as A4, or a MIDI number", spec.name)),
        ParamKind::Number { decimals, .. } => fixed(text, decimals)
            .map(ParamValue::Number)
            .ok_or_else(|| format!("{}: a number with at most {decimals} decimal(s)", spec.name)),
        ParamKind::Choice { .. } => {
            let query = folded(text);
            spec.choices
                .iter()
                .position(|name| folded(name) == query)
                .or_else(|| text.parse::<usize>().ok())
                .and_then(|index| u8::try_from(index).ok())
                .map(ParamValue::Choice)
                .ok_or_else(|| format!("{}: one of {}", spec.name, spec.choices.join(", ")))
        }
        ParamKind::Toggle => match text.to_ascii_lowercase().as_str() {
            "on" | "true" | "1" | "yes" => Ok(ParamValue::Toggle(true)),
            "off" | "false" | "0" | "no" => Ok(ParamValue::Toggle(false)),
            _ => Err(format!("{}: on or off", spec.name)),
        },
    }
}

/// `text`, a decimal number, as an integer with `decimals` digits after the
/// point: `-1.5` with one decimal is −15; `None` with more digits than that.
fn fixed(text: &str, decimals: u8) -> Option<i32> {
    let (negative, digits) = match text.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, text.strip_prefix('+').unwrap_or(text)),
    };
    let (whole, fraction) = digits.split_once('.').unwrap_or((digits, ""));
    let decimals = usize::from(decimals);
    let all_digits = |part: &str| part.chars().all(|c| c.is_ascii_digit());
    if whole.is_empty() && fraction.is_empty()
        || fraction.len() > decimals
        || !all_digits(whole)
        || !all_digits(fraction)
    {
        return None;
    }
    let padded = format!("{whole}{fraction:0<decimals$}");
    let magnitude = padded.parse::<i32>().ok()?;
    Some(if negative { -magnitude } else { magnitude })
}

/// A note's name (`C4`, `F#2`, `A-1`) as its MIDI number.
fn note_number(text: &str) -> Option<i32> {
    let upper = text.to_ascii_uppercase();
    let mut chars = upper.chars();
    let semitone = match chars.next()? {
        'C' => 0,
        'D' => 2,
        'E' => 4,
        'F' => 5,
        'G' => 7,
        'A' => 9,
        'B' => 11,
        _ => return None,
    };
    let rest = chars.as_str();
    let (sharp, octave) = match rest.strip_prefix('#') {
        Some(octave) => (1, octave),
        None => (0, rest),
    };
    let octave = octave.parse::<i32>().ok()?;
    Some((octave + 1) * 12 + semitone + sharp)
}

/// The output ports' names of every slot, as the instances list them: what
/// names a port of another instance as an input, `1:NOTE` (journal/D-048).
/// Empty, a port reads by its rank instead, `1:#1`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PortNames {
    slots: Vec<[String; 4]>,
}

impl PortNames {
    /// The names `instances` give their ports, by their positions.
    #[must_use]
    pub fn of(instances: &[InstanceEntry]) -> Self {
        let mut slots: Vec<[String; 4]> = Vec::new();
        for instance in instances {
            let at = usize::from(instance.position);
            if slots.len() <= at {
                slots.resize_with(at + 1, Default::default);
            }
            slots[at].clone_from(&instance.names);
        }
        Self { slots }
    }

    /// The name of port `port` of slot `slot`, if known.
    #[must_use]
    pub fn name(&self, slot: usize, port: usize) -> Option<&str> {
        let name = self.slots.get(slot)?.get(port)?;
        (!name.is_empty()).then_some(name.as_str())
    }

    /// The port of slot `slot` named `name`, case ignored, if known.
    fn port(&self, slot: usize, name: &str) -> Option<usize> {
        self.slots
            .get(slot)?
            .iter()
            .position(|port| !port.is_empty() && port.eq_ignore_ascii_case(name))
    }
}

/// How a host names `source`: `CV1`, or the slot from 1 and the port's name,
/// `1:NOTE`, as the module's submenu does; `1:#1` by the port's rank when
/// `ports` does not know its name.
#[must_use]
pub fn source_text(source: Source, ports: &PortNames) -> String {
    match source {
        Source::Jack(channel) => format!("CV{}", channel.index() + 1),
        Source::Lane { slot, port } => {
            let slot = usize::from(slot);
            let port = usize::from(port);
            match ports.name(slot, port) {
                Some(name) => format!("{}:{name}", slot + 1),
                None => format!("{}:#{}", slot + 1, port + 1),
            }
        }
    }
}

/// `assign` as the submenu writes its fields, on one line: `CV2 x0.50
/// +0.25V -1.0..+5.0 ADD`; a port of another instance alone, `1:NOTE` as
/// `ports` names it, for it is assigned as it comes (journal/D-048); `--`
/// without an input.
#[must_use]
pub fn input_text(assign: &Assign, ports: &PortNames) -> String {
    let Some(source) = assign.input else {
        return "--".to_owned();
    };
    if assign.assigns() {
        return source_text(source, ports);
    }
    let field = |line| assign.field(line);
    format!(
        "{} {} {} {}..{} {}",
        source_text(source, ports),
        field(AssignLine::Scale).as_str(),
        field(AssignLine::Shift).as_str(),
        field(AssignLine::Min).as_str(),
        field(AssignLine::Max).as_str(),
        field(AssignLine::Effect).as_str(),
    )
}

/// The fields of an input a host can set, by the name the submenu gives
/// them.
pub const INPUT_FIELDS: [&str; 6] = ["input", "scale", "shift", "min", "max", "effect"];

/// `assign` with `field` (one of [`INPUT_FIELDS`]) read from `text`: the
/// input `cv1` to `cv4`, a port of another instance by its slot and the
/// name `ports` gives it (`1:note`) or its rank (`1:#1`, journal/D-048), or
/// `none`; the scale as a factor (`0.5`, `-1`); the
/// shift and the bounds in volts (`-2.5`); the effect `set`, `add` or
/// `mul`. Whether the module takes it is the module's to say.
///
/// # Errors
///
/// The field's name and what it takes, when `text` is not that.
pub fn parse_input_field(
    mut assign: Assign,
    field: &str,
    text: &str,
    ports: &PortNames,
) -> Result<Assign, String> {
    let lower = text.to_ascii_lowercase();
    // As the submenu writes them, `+0.25V` and `x0.50` read back.
    let volts =
        || fixed(lower.trim_end_matches('v'), 3).ok_or_else(|| format!("{field}: volts, as -2.5"));
    let field_name = field.to_ascii_lowercase();
    if assign.assigns() && field_name != "input" && INPUT_FIELDS.contains(&field_name.as_str()) {
        return Err(format!(
            "{field}: a port of another instance is assigned as it comes, \
             with nothing to shape; pick a jack to set it"
        ));
    }
    match field_name.as_str() {
        "input" => {
            assign.input = match lower.as_str() {
                "none" | "--" | "" => None,
                source => Some(parse_source(source, ports).ok_or_else(|| {
                    format!("{field}: cv1 to cv4, a slot and a port's name (1:note), or none")
                })?),
            };
        }
        "scale" => {
            let factor = lower.trim_start_matches('x');
            assign.scale_percent =
                fixed(factor, 2).ok_or_else(|| format!("{field}: a factor, as 0.5 or -1"))?;
        }
        "shift" => assign.shift_mv = volts()?,
        "min" => assign.min_mv = volts()?,
        "max" => assign.max_mv = volts()?,
        "effect" => {
            assign.effect = match lower.as_str() {
                "set" => Effect::Set,
                "add" => Effect::Add,
                "mul" => Effect::Mul,
                _ => return Err(format!("{field}: set, add or mul")),
            };
        }
        _ => {
            return Err(format!(
                "no field {field}; there are: {}",
                INPUT_FIELDS.join(", ")
            ));
        }
    }
    Ok(assign)
}

/// The input `text` names, in lower case: `cv1` to `cv4`, or the slot from
/// 1 of another instance and its port's name, `1:note`, or rank, `1:#1`.
fn parse_source(text: &str, ports: &PortNames) -> Option<Source> {
    if let Some(number) = text.strip_prefix("cv") {
        let index = number.parse::<usize>().ok()?.checked_sub(1)?;
        return CvChannel::ALL.get(index).copied().map(Source::Jack);
    }
    let (slot, port) = parse_lane(text, ports)?;
    Source::lane(slot, port)
}

/// The slot and port `text` names, in lower case: the slot from 1, then the
/// port's name `ports` gives it, `1:note`, or its rank, `1:#1`.
fn parse_lane(text: &str, ports: &PortNames) -> Option<(usize, usize)> {
    let (slot, port) = text.split_once(':')?;
    let slot = slot.parse::<usize>().ok()?.checked_sub(1)?;
    let port = match port.strip_prefix('#') {
        Some(rank) => rank.parse::<usize>().ok()?.checked_sub(1)?,
        None => ports.port(slot, port)?,
    };
    Some((slot, port))
}

/// `input` with the scale and shift that carry what its input read over
/// `samples` onto the range its parameter can use, as the submenu's
/// `CENTER` does with the readings it saw: the same
/// [`Assign::center`], from the host's readings (journal/D-045).
///
/// # Errors
///
/// Without an input, a sweep, samples, or a signal that moves
/// [`CENTER_MIN_SPAN_MV`] at least; for a port of another instance, which
/// the host's samples do not carry (the submenu's `CENTER` fits it).
pub fn centered(input: &InputEntry, samples: &[Sample]) -> Result<Assign, String> {
    let mut assign = input.assign;
    let source = assign.input.ok_or("no input to fit: set one first")?;
    let Source::Jack(channel) = source else {
        return Err("the host samples the jacks only: use CENTER on the module".to_owned());
    };
    let sweep = input.sweep.ok_or("the module gives no range for it")?;
    let spread = measure::input_spread(samples).ok_or("no sample came")?;
    let seen = &spread[channel.index()];
    if assign.center(
        (seen.min_mv, seen.max_mv),
        usable_range(sweep, assign.effect),
    ) {
        Ok(assign)
    } else {
        Err(format!(
            "CV{} moved {} mV, less than the {CENTER_MIN_SPAN_MV} mV CENTER needs",
            channel.index() + 1,
            seen.max_mv - seen.min_mv
        ))
    }
}

/// The parameters whose value differs from `before` to `after`, but for
/// `except` (the one just set), as `NAME value` with their new value: what
/// a set moved besides itself (journal/D-043).
#[must_use]
pub fn moved(before: &AppDescription, after: &AppDescription, except: u8) -> Vec<String> {
    after
        .params
        .iter()
        .filter(|spec| spec.index != except)
        .filter(|spec| {
            before
                .params
                .iter()
                .find(|old| old.index == spec.index)
                .is_none_or(|old| old.value != spec.value)
        })
        .filter_map(|spec| {
            spec.value
                .map(|value| format!("{} {}", spec.label, value_text(spec, value)))
        })
        .collect()
}

/// One applet's parameters, observables and actions, a line each: the
/// index, the name, the value, the CV input of a parameter that takes one
/// (journal/D-045), then the range. The input's column has the width of the
/// longest, so that inputs read down one column whatever the ranges.
#[must_use]
pub fn description_text(app: &AppEntry, description: &AppDescription, ports: &PortNames) -> String {
    let mut out = String::new();
    let _ = writeln!(
        out,
        "{} {}{}",
        app.index,
        app.name,
        if app.running { " (running)" } else { "" }
    );
    let width = description
        .params
        .iter()
        .chain(&description.observables)
        .map(|spec| shown_name(spec).len())
        .max()
        .unwrap_or(0);
    let input_width = description
        .params
        .iter()
        .map(|spec| input_column(spec, ports).chars().count())
        .max()
        .unwrap_or(0);
    for (title, list) in [
        ("parameters", &description.params),
        ("observables", &description.observables),
    ] {
        if list.is_empty() {
            continue;
        }
        let _ = writeln!(out, "{title}:");
        for spec in list {
            let value = spec
                .value
                .map_or_else(|| "--".to_owned(), |value| value_text(spec, value));
            let mut line = format!(
                "  {:>2} {:<width$}  {value:<12}",
                spec.index,
                shown_name(spec)
            );
            if input_width > 0 {
                let _ = write!(line, " {:<input_width$} ", input_column(spec, ports));
            }
            let _ = write!(
                line,
                " {}{}",
                range_text(spec),
                if spec.active { "" } else { "  (inactive)" }
            );
            let _ = writeln!(out, "{}", line.trim_end());
        }
    }
    if !description.actions.is_empty() {
        let _ = writeln!(out, "actions: {}", description.actions.join(", "));
    }
    if !description.outputs.is_empty() {
        let names: Vec<String> = description
            .outputs
            .iter()
            .map(|port| {
                let kind = if port.level { "" } else { " (audio)" };
                format!("{}{kind}", port.name)
            })
            .collect();
        let _ = writeln!(out, "outputs: {}", names.join(", "));
    }
    if !description.inputs.is_empty() {
        let names: Vec<_> = description
            .inputs
            .iter()
            .map(|port| format!("{} ({})", port.name, if port.trigger { "TR" } else { "CV" }))
            .collect();
        let _ = writeln!(out, "inputs: {}", names.join(", "));
    }
    out
}

/// What the input column shows of `spec`: `CV` and the input as the
/// submenu writes it, `CV ?` for an input not described, nothing for a
/// parameter or an observable that takes none.
fn input_column(spec: &SpecEntry, ports: &PortNames) -> String {
    match (spec.assignable, &spec.input) {
        (true, Some(input)) => format!("CV {}", input_text(&input.assign, ports)),
        (true, None) => "CV ?".to_owned(),
        (false, _) => String::new(),
    }
}

/// An entry's label, with its name after it when they differ
/// (`SPAN (FIRST)`), so that both ways to find it show.
fn shown_name(spec: &SpecEntry) -> String {
    if spec.label == spec.name || spec.label.is_empty() {
        spec.name.clone()
    } else {
        format!("{} ({})", spec.label, spec.name)
    }
}

/// What values `spec` takes, as text: `1..32`, `FIRST|SPAN`, `on|off`.
fn range_text(spec: &SpecEntry) -> String {
    match spec.kind {
        ParamKind::Number { min, max, .. } => format!(
            "{}..{}",
            value_text(spec, ParamValue::Number(min)),
            value_text(spec, ParamValue::Number(max))
        ),
        ParamKind::Choice { .. } => spec.choices.join("|"),
        ParamKind::Toggle => "on|off".to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use oc_core::apps::param::Unit;
    use oc_protocol::{ParamKind, ParamValue};

    use super::{find_spec, fixed, note_number, parse_value, value_text};
    use crate::client::{AppDescription, SpecEntry};

    fn spec(name: &str, kind: ParamKind, choices: &[&str]) -> SpecEntry {
        SpecEntry {
            index: 0,
            name: name.to_owned(),
            label: name.to_owned(),
            kind,
            choices: choices.iter().map(|&name| name.to_owned()).collect(),
            assignable: false,
            active: true,
            value: None,
            input: None,
        }
    }

    const fn number(decimals: u8, unit: Unit) -> ParamKind {
        ParamKind::Number {
            min: -100,
            max: 100,
            step: 1,
            decimals,
            unit,
        }
    }

    #[test]
    fn a_decimal_number_reads_back_as_its_integer() {
        assert_eq!(fixed("-1.5", 1), Some(-15));
        assert_eq!(fixed("2", 3), Some(2_000));
        assert_eq!(fixed("+0.25", 3), Some(250));
        assert_eq!(fixed("1.25", 1), None, "more decimals than the parameter");
        assert_eq!(fixed("1e3", 0), None);
        assert_eq!(fixed("", 0), None);
    }

    #[test]
    fn a_note_reads_by_its_name_or_its_number() {
        assert_eq!(note_number("A4"), Some(69));
        assert_eq!(note_number("c#3"), Some(49));
        assert_eq!(note_number("C-1"), Some(0));
        let note = spec("NOTE", number(0, Unit::Note), &[]);
        assert_eq!(parse_value(&note, "A4"), Ok(ParamValue::Number(69)));
        assert_eq!(parse_value(&note, "45"), Ok(ParamValue::Number(45)));
        assert_eq!(value_text(&note, ParamValue::Number(45)), "A2");
    }

    #[test]
    fn a_choice_or_a_toggle_reads_by_name() {
        let mode = spec("MODE", ParamKind::Choice { count: 2 }, &["FIRST", "SPAN"]);
        assert_eq!(parse_value(&mode, "span"), Ok(ParamValue::Choice(1)));
        assert_eq!(parse_value(&mode, "0"), Ok(ParamValue::Choice(0)));
        assert!(parse_value(&mode, "middle").is_err());
        let sync = spec("SYNC", ParamKind::Toggle, &[]);
        assert_eq!(parse_value(&sync, "On"), Ok(ParamValue::Toggle(true)));
        assert_eq!(value_text(&sync, ParamValue::Toggle(false)), "OFF");
    }

    #[test]
    fn a_value_edited_reads_back_unchanged() {
        let shape = spec("TRANSPOSE", number(1, Unit::Octaves), &[]);
        let note = spec("NOTE", number(0, Unit::Note), &[]);
        let mode = spec("MODE", ParamKind::Choice { count: 2 }, &["FIRST", "SPAN"]);
        for (entry, value) in [
            (&shape, ParamValue::Number(-25)),
            (&note, ParamValue::Number(69)),
            (&mode, ParamValue::Choice(1)),
        ] {
            let text = super::edit_text(entry, value);
            assert_eq!(parse_value(entry, &text), Ok(value), "{text}");
        }
        assert_eq!(super::edit_text(&shape, ParamValue::Number(-25)), "-2.5");
    }

    #[test]
    fn a_value_reads_as_the_module_writes_it() {
        let shape = spec("TRANSPOSE", number(1, Unit::Octaves), &[]);
        assert_eq!(value_text(&shape, ParamValue::Number(-30)), "-3.0 OCT");
        assert_eq!(parse_value(&shape, "-3.0"), Ok(ParamValue::Number(-30)));
    }

    #[test]
    fn what_a_set_moved_besides_itself_is_named() {
        let mut count = spec(
            "COUNT",
            ParamKind::Number {
                min: 1,
                max: 32,
                step: 1,
                decimals: 0,
                unit: Unit::None,
            },
            &[],
        );
        count.value = Some(ParamValue::Number(4));
        let mut gate = spec("GATE", ParamKind::Toggle, &[]);
        gate.index = 1;
        gate.value = Some(ParamValue::Toggle(true));
        let before = AppDescription {
            params: vec![count.clone(), gate.clone()],
            ..AppDescription::default()
        };
        count.value = Some(ParamValue::Number(32));
        gate.value = Some(ParamValue::Toggle(false));
        let after = AppDescription {
            params: vec![count, gate],
            ..AppDescription::default()
        };
        assert_eq!(super::moved(&before, &after, 1), ["COUNT 32"]);
        assert_eq!(super::moved(&after, &after, 1), Vec::<String>::new());
    }

    #[test]
    fn an_input_is_a_jack_or_a_port_of_another_instance_by_name() {
        use oc_core::modulation::{Assign, Source};
        use oc_core::platform::CvChannel;

        use super::{PortNames, input_text, parse_input_field};
        use crate::client::InstanceEntry;

        let melody = InstanceEntry {
            position: 1,
            app: 7,
            focused: false,
            outputs: [None; 4],
            inputs: [oc_protocol::InputSource::Jack(0); 4],
            levels: [true; 4],
            names: ["NOTE", "CH1", "CH2", "GATE"].map(str::to_owned),
        };
        let ports = PortNames::of(&[melody]);
        let parse = |text| parse_input_field(Assign::NONE, "input", text, &ports).map(|a| a.input);
        assert_eq!(parse("CV3"), Ok(Some(Source::Jack(CvChannel::Three))));
        assert_eq!(parse("2:gate"), Ok(Some(Source::Lane { slot: 1, port: 3 })));
        assert_eq!(parse("2:CH1"), Ok(Some(Source::Lane { slot: 1, port: 1 })));
        assert_eq!(parse("1:#2"), Ok(Some(Source::Lane { slot: 0, port: 1 })));
        assert_eq!(parse("none"), Ok(None));
        for wrong in ["cv5", "2:sub", "1:note", "5:#1", "2:#5", "0:#1", "2", "1a"] {
            assert!(parse(wrong).is_err(), "{wrong}");
        }
        let assign = parse_input_field(Assign::NONE, "input", "2:note", &ports).unwrap();
        assert_eq!(
            input_text(&assign, &ports),
            "2:NOTE",
            "assigned: nothing else"
        );
        assert_eq!(
            input_text(&assign, &PortNames::default()),
            "2:#1",
            "by its rank, names unknown"
        );
        assert!(
            parse_input_field(assign, "scale", "0.5", &ports).is_err(),
            "nothing to shape"
        );
        assert!(parse_input_field(assign, "input", "cv1", &ports).is_ok());
    }

    #[test]
    fn an_input_port_reads_a_jack_or_a_port_of_another_instance() {
        use oc_protocol::InputSource;

        use super::{PortNames, input_source_text, parse_input_source};
        use crate::client::InstanceEntry;

        let ratchet = InstanceEntry {
            position: 0,
            app: 3,
            focused: false,
            outputs: [None; 4],
            inputs: [InputSource::Jack(0); 4],
            levels: [true, true, false, false],
            names: ["THRU", "BURST", "", ""].map(str::to_owned),
        };
        let ports = PortNames::of(&[ratchet]);
        let parse = |text| parse_input_source(text, true, &ports);
        assert_eq!(parse("tr2"), Ok(InputSource::Jack(1)));
        assert_eq!(parse("1:burst"), Ok(InputSource::Lane { slot: 0, port: 1 }));
        assert_eq!(parse("1:#1"), Ok(InputSource::Lane { slot: 0, port: 0 }));
        for wrong in ["cv1", "1:sub", "5:#1", "1"] {
            assert!(parse(wrong).is_err(), "{wrong}");
        }
        assert_eq!(
            input_source_text(InputSource::Lane { slot: 0, port: 1 }, true, &ports),
            "1:BURST"
        );
        assert_eq!(input_source_text(InputSource::Jack(3), true, &ports), "TR4");
    }

    #[test]
    fn a_name_is_found_whatever_its_case_and_punctuation() {
        let mut first = spec("FIRST", number(0, Unit::Milliseconds), &[]);
        first.label = "SPAN".to_owned();
        let list = [spec("PASSTHRU", ParamKind::Toggle, &[]), first];
        assert_eq!(
            find_spec(&list, "span").map(|spec| spec.name.as_str()),
            Some("FIRST")
        );
        assert_eq!(
            find_spec(&list, "first").map(|spec| spec.label.as_str()),
            Some("SPAN")
        );
        assert!(find_spec(&list, "passthru").is_some());
        assert!(find_spec(&list, "0").is_some());
        assert!(find_spec(&list, "pass").is_none());
    }

    #[test]
    fn centring_carries_the_signal_read_onto_the_parameters_sweep() {
        use oc_core::platform::CvChannel;
        use oc_protocol::{Assign, Effect, Sample};

        use super::centered;
        use crate::client::InputEntry;

        let input = InputEntry {
            assign: Assign {
                input: Some(CvChannel::Two.into()),
                effect: Effect::Set,
                ..Assign::NONE
            },
            sweep: Some((0, 4_000)),
        };
        // CV2 swings from 1 V to 3 V; the other inputs do not matter.
        let samples: Vec<Sample> = (0..=20)
            .map(|step| Sample {
                cv_in: [5_000, 1_000 + step * 100, -3_000, 0],
                ..Sample::default()
            })
            .collect();
        let assign = centered(&input, &samples).unwrap();
        assert_eq!(
            assign.shape(1_000),
            0,
            "the lowest reading at the sweep's start"
        );
        assert_eq!(assign.shape(3_000), 4_000, "the highest at its end");

        let still = vec![Sample::default(); 5];
        assert!(centered(&input, &still).unwrap_err().contains("moved 0 mV"));
        let unplugged = InputEntry {
            assign: Assign::NONE,
            ..input
        };
        assert!(centered(&unplugged, &samples).is_err());
        assert!(centered(&input, &[]).is_err());
    }

    #[test]
    fn inputs_and_ranges_each_read_down_one_column() {
        use oc_protocol::Assign;

        use super::description_text;
        use crate::client::{AppEntry, InputEntry};

        let mut long = spec(
            "LONG",
            ParamKind::Choice { count: 2 },
            &["A-LONG-NAME", "B"],
        );
        long.index = 0;
        let mut cv = spec("CV", number(0, Unit::None), &[]);
        cv.index = 1;
        cv.assignable = true;
        cv.input = Some(InputEntry {
            assign: Assign::NONE,
            sweep: None,
        });
        let mut set = cv.clone();
        set.index = 2;
        set.input = Some(InputEntry {
            assign: Assign {
                input: Some(oc_core::platform::CvChannel::Two.into()),
                ..Assign::NONE
            },
            sweep: None,
        });
        let description = AppDescription {
            params: vec![long, cv, set],
            ..AppDescription::default()
        };
        let app = AppEntry {
            index: 0,
            name: "TEST".to_owned(),
            running: false,
        };
        let text = description_text(&app, &description, &super::PortNames::default());
        let rows: Vec<&str> = text.lines().skip(2).collect();
        let column = |row: &str, needle: &str| {
            row.find(needle)
                .unwrap_or_else(|| panic!("{needle} in {row}"))
        };
        assert_eq!(
            column(rows[1], "CV --"),
            column(rows[2], "CV CV2"),
            "{text}"
        );
        assert_eq!(
            column(rows[0], "A-LONG-NAME"),
            column(rows[1], "-100"),
            "{text}"
        );
        assert!(!rows[0].contains("CV"), "LONG takes no input: {text}");
    }

    #[test]
    fn ports_jacks_and_instances_read_from_text() {
        use super::{
            PortRef, find_port, instance_position, parse_jack, parse_output, routing_text,
        };
        use crate::client::{InputPortEntry, InstanceEntry, OutputPortEntry};

        assert_eq!(instance_position("@1"), Some(1));
        assert_eq!(instance_position("1"), None, "an applet's index");
        assert_eq!(parse_output("off"), Ok(None));
        assert_eq!(parse_output("D"), Ok(Some(3)));
        assert!(parse_output("e").is_err());
        assert_eq!(parse_jack("tr2", true), Ok(1));
        assert_eq!(parse_jack("CV4", false), Ok(3));
        assert!(parse_jack("cv1", true).is_err(), "a trigger input");
        assert!(parse_jack("tr5", true).is_err());

        let description = AppDescription {
            outputs: ["A", "B"]
                .map(|name| OutputPortEntry {
                    name: name.to_owned(),
                    level: true,
                })
                .to_vec(),
            inputs: vec![InputPortEntry {
                name: "CLOCK".to_owned(),
                trigger: true,
            }],
            ..AppDescription::default()
        };
        assert_eq!(find_port(&description, "b"), Some(PortRef::Output(1)));
        assert_eq!(
            find_port(&description, "clock"),
            Some(PortRef::Input(0, true))
        );
        assert_eq!(find_port(&description, "sub"), None);
        let instance = InstanceEntry {
            position: 1,
            app: 3,
            focused: true,
            outputs: [Some(3), None, None, None],
            inputs: [
                oc_protocol::InputSource::Jack(1),
                oc_protocol::InputSource::Jack(0),
                oc_protocol::InputSource::Jack(0),
                oc_protocol::InputSource::Jack(0),
            ],
            levels: [true, true, false, false],
            names: ["A", "B", "", ""].map(str::to_owned),
        };
        assert_eq!(
            routing_text(&description, &instance, &super::PortNames::default()),
            "A -> D\nB -> OFF\nCLOCK <- TR2\n"
        );
    }
}
