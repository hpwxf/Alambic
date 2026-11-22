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
use oc_core::modulation::{AssignLine, CENTER_MIN_SPAN_MV, usable_range};
use oc_core::platform::CvChannel;
use oc_protocol::{Assign, Effect, ParamKind, ParamValue, Sample};

use crate::client::{AppDescription, AppEntry, InputEntry, SpecEntry};
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

/// `assign` as the submenu writes its fields, on one line: `CV2 x0.50
/// +0.25V -1.0..+5.0 ADD`; `--` without an input.
#[must_use]
pub fn input_text(assign: &Assign) -> String {
    if assign.input.is_none() {
        return "--".to_owned();
    }
    let field = |line| assign.field(line);
    format!(
        "{} {} {} {}..{} {}",
        field(AssignLine::Input).as_str(),
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
/// input `cv1` to `cv4` or `none`; the scale as a factor (`0.5`, `-1`); the
/// shift and the bounds in volts (`-2.5`); the effect `set`, `add` or
/// `mul`. Whether the module takes it is the module's to say.
///
/// # Errors
///
/// The field's name and what it takes, when `text` is not that.
pub fn parse_input_field(mut assign: Assign, field: &str, text: &str) -> Result<Assign, String> {
    let lower = text.to_ascii_lowercase();
    // As the submenu writes them, `+0.25V` and `x0.50` read back.
    let volts =
        || fixed(lower.trim_end_matches('v'), 3).ok_or_else(|| format!("{field}: volts, as -2.5"));
    match field.to_ascii_lowercase().as_str() {
        "input" => {
            assign.input = match lower.trim_start_matches("cv") {
                "none" | "--" | "" => None,
                number => {
                    let index = number.parse::<usize>().ok().and_then(|n| n.checked_sub(1));
                    Some(
                        index
                            .and_then(|index| CvChannel::ALL.get(index).copied())
                            .ok_or_else(|| format!("{field}: cv1 to cv4, or none"))?,
                    )
                }
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

/// `input` with the scale and shift that carry what its input read over
/// `samples` onto the range its parameter can use, as the submenu's
/// `CENTER` does with the readings it saw: the same
/// [`Assign::center`], from the host's readings (journal/D-045).
///
/// # Errors
///
/// Without an input, a sweep, samples, or a signal that moves
/// [`CENTER_MIN_SPAN_MV`] at least.
pub fn centered(input: &InputEntry, samples: &[Sample]) -> Result<Assign, String> {
    let mut assign = input.assign;
    let channel = assign.input.ok_or("no input to fit: set one first")?;
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
pub fn description_text(app: &AppEntry, description: &AppDescription) -> String {
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
        .map(|spec| input_column(spec).chars().count())
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
                let _ = write!(line, " {:<input_width$} ", input_column(spec));
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
    out
}

/// What the input column shows of `spec`: `CV` and the input as the
/// submenu writes it, `CV ?` for an input not described, nothing for a
/// parameter or an observable that takes none.
fn input_column(spec: &SpecEntry) -> String {
    match (spec.assignable, &spec.input) {
        (true, Some(input)) => format!("CV {}", input_text(&input.assign)),
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
                input: Some(CvChannel::Two),
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
                input: Some(oc_core::platform::CvChannel::Two),
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
        let text = description_text(&app, &description);
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
}
