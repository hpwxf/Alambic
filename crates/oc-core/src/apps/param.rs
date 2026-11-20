//! What an applet shows of its state to anything but its own screen
//! (journal/D-043): the parameters it can be set by, the values it can be
//! read for, the actions it can be asked for.
//!
//! The panel, the host probe, the settings flash and, later, a route from
//! another applet all reach a parameter through [`App::set`](super::App::set),
//! with the bounds the panel uses: a value set from afar behaves as one
//! turned by hand. The screen stays the applet's own; this model describes
//! state, not layout, and an applet may use it to write a label or a value.
//!
//! Everything here is `'static` or `Copy`: descriptors live in the
//! applet's code, and a value is a number, a choice or a switch, so nothing
//! allocates and every value fits a message.

use core::fmt::{self, Write};

use crate::apps::tone::note_name;

/// What a number counts, for the text a host shows of it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Unit {
    /// A plain count or ratio.
    None,
    /// Percent.
    Percent,
    /// Milliseconds.
    Milliseconds,
    /// Volts.
    Volts,
    /// Hertz.
    Hertz,
    /// Octaves.
    Octaves,
    /// Beats per minute.
    Bpm,
    /// A note as a MIDI number, shown by its name (`A4` for 69).
    Note,
}

impl Unit {
    /// The suffix after a number, empty for none and for a note.
    const fn suffix(self) -> &'static str {
        match self {
            Self::None | Self::Note => "",
            Self::Percent => "%",
            Self::Milliseconds => " MS",
            Self::Volts => " V",
            Self::Hertz => " HZ",
            Self::Octaves => " OCT",
            Self::Bpm => " BPM",
        }
    }
}

/// What values a parameter takes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Kind {
    /// An integer from `min` to `max`, read with `decimals` digits after the
    /// point: `SHAPE` at −30 with one decimal reads `-3.0`. `step` is what
    /// one detent changes on the panel, for a host that steps like it.
    Number {
        /// The least value.
        min: i32,
        /// The greatest value.
        max: i32,
        /// One detent's change on the panel.
        step: i32,
        /// Digits after the point when the value is read.
        decimals: u8,
        /// What it counts.
        unit: Unit,
    },
    /// One of these names, by its index.
    Choice(&'static [&'static str]),
    /// On or off.
    Toggle,
}

/// One parameter, or one observable value, as an applet declares it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ParamSpec {
    /// Its name, as the screen labels it.
    pub name: &'static str,
    /// What values it takes.
    pub kind: Kind,
    /// Whether it takes a CV input (journal/D-035, D-036).
    pub assignable: bool,
}

impl ParamSpec {
    /// A number; see [`Kind::Number`].
    #[must_use]
    pub const fn number(
        name: &'static str,
        (min, max): (i32, i32),
        step: i32,
        decimals: u8,
        unit: Unit,
    ) -> Self {
        Self {
            name,
            kind: Kind::Number {
                min,
                max,
                step,
                decimals,
                unit,
            },
            assignable: false,
        }
    }

    /// One of `names`.
    #[must_use]
    pub const fn choice(name: &'static str, names: &'static [&'static str]) -> Self {
        Self {
            name,
            kind: Kind::Choice(names),
            assignable: false,
        }
    }

    /// On or off.
    #[must_use]
    pub const fn toggle(name: &'static str) -> Self {
        Self {
            name,
            kind: Kind::Toggle,
            assignable: false,
        }
    }

    /// The same parameter, taking a CV input.
    #[must_use]
    pub const fn assignable(self) -> Self {
        Self {
            assignable: true,
            ..self
        }
    }

    /// `value` if this parameter can take it at all: of its kind, within its
    /// range. The applet may narrow the range further by another setting
    /// (`RATCHET`'s first interval by its mode), and says what it applied.
    ///
    /// # Errors
    ///
    /// [`Refused::WrongKind`] for a value of another kind,
    /// [`Refused::OutOfRange`] for one outside the range or the names.
    pub const fn check(&self, value: ParamValue) -> Result<ParamValue, Refused> {
        match (self.kind, value) {
            (Kind::Number { min, max, .. }, ParamValue::Number(number)) => {
                if number >= min && number <= max {
                    Ok(value)
                } else {
                    Err(Refused::OutOfRange)
                }
            }
            (Kind::Choice(names), ParamValue::Choice(index)) => {
                if (index as usize) < names.len() {
                    Ok(value)
                } else {
                    Err(Refused::OutOfRange)
                }
            }
            (Kind::Toggle, ParamValue::Toggle(_)) => Ok(value),
            _ => Err(Refused::WrongKind),
        }
    }

    /// Writes `value` as a host shows it: `-3.0`, `50.0%`, `GATE`, `A4`.
    ///
    /// # Errors
    ///
    /// If `out` refuses the text.
    pub fn write(&self, value: ParamValue, out: &mut impl Write) -> fmt::Result {
        match (self.kind, value) {
            (
                Kind::Number {
                    unit: Unit::Note, ..
                },
                ParamValue::Number(number),
            ) => {
                let note = u8::try_from(number).unwrap_or(0);
                out.write_str(note_name(note).as_str())
            }
            (Kind::Number { decimals, unit, .. }, ParamValue::Number(number)) => {
                write_fixed(number, decimals, out)?;
                out.write_str(unit.suffix())
            }
            (Kind::Choice(names), ParamValue::Choice(index)) => {
                out.write_str(names.get(usize::from(index)).copied().unwrap_or("?"))
            }
            (_, ParamValue::Toggle(on)) => out.write_str(if on { "ON" } else { "OFF" }),
            _ => out.write_str("?"),
        }
    }
}

/// `number` with `decimals` digits after the point: 1234 with 3 reads
/// `1.234`, −5 with 1 `-0.5`.
fn write_fixed(number: i32, decimals: u8, out: &mut impl Write) -> fmt::Result {
    if decimals == 0 {
        return write!(out, "{number}");
    }
    let scale = 10_u32.pow(u32::from(decimals));
    let magnitude = number.unsigned_abs();
    let sign = if number < 0 { "-" } else { "" };
    let width = usize::from(decimals);
    write!(
        out,
        "{sign}{}.{:0width$}",
        magnitude / scale,
        magnitude % scale
    )
}

/// A parameter's or an observable's value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ParamValue {
    /// For a [`Kind::Number`].
    Number(i32),
    /// For a [`Kind::Choice`]: the index of the name.
    Choice(u8),
    /// For a [`Kind::Toggle`].
    Toggle(bool),
}

/// Why an applet did not take a value or an action.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Refused {
    /// No parameter or action at that index.
    NoSuch,
    /// A value of another kind than the parameter's.
    WrongKind,
    /// A value outside the parameter's range or names.
    OutOfRange,
}

/// Writes every parameter of `app`, then every observable it knows, as
/// `NAME value` separated by three spaces: one line that says what the
/// applet is doing, whichever it is.
///
/// # Errors
///
/// If `out` refuses the text.
pub fn write_summary(app: &dyn super::App, out: &mut impl Write) -> fmt::Result {
    let mut first = true;
    let mut entry = |spec: &ParamSpec, value: Option<ParamValue>| -> fmt::Result {
        let Some(value) = value else {
            return Ok(());
        };
        if !first {
            out.write_str("   ")?;
        }
        first = false;
        write!(out, "{} ", spec.name)?;
        spec.write(value, out)
    };
    for (index, spec) in app.param_specs().iter().enumerate() {
        entry(spec, app.get(index))?;
    }
    for (index, spec) in app.observables().iter().enumerate() {
        entry(spec, app.observe(index))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{Kind, ParamSpec, ParamValue, Refused, Unit};
    use crate::fmt::TextBuf;

    const SHAPE: ParamSpec = ParamSpec::number("SHAPE", (-30, 30), 1, 1, Unit::Octaves);
    const MODE: ParamSpec = ParamSpec::choice("MODE", &["FIRST", "SPAN"]);

    fn text(spec: &ParamSpec, value: ParamValue) -> TextBuf<24> {
        let mut out = TextBuf::<24>::new();
        spec.write(value, &mut out).unwrap();
        out
    }

    #[test]
    fn a_value_is_checked_against_its_kind_and_range() {
        assert_eq!(
            SHAPE.check(ParamValue::Number(-30)),
            Ok(ParamValue::Number(-30))
        );
        assert_eq!(
            SHAPE.check(ParamValue::Number(31)),
            Err(Refused::OutOfRange)
        );
        assert_eq!(
            SHAPE.check(ParamValue::Toggle(true)),
            Err(Refused::WrongKind)
        );
        assert_eq!(MODE.check(ParamValue::Choice(1)), Ok(ParamValue::Choice(1)));
        assert_eq!(MODE.check(ParamValue::Choice(2)), Err(Refused::OutOfRange));
    }

    #[test]
    fn a_value_reads_with_its_decimals_unit_or_name() {
        assert_eq!(text(&SHAPE, ParamValue::Number(-5)).as_str(), "-0.5 OCT");
        assert_eq!(text(&SHAPE, ParamValue::Number(25)).as_str(), "2.5 OCT");
        assert_eq!(text(&MODE, ParamValue::Choice(1)).as_str(), "SPAN");
        let level = ParamSpec::number("LEVEL", (0, 3_000), 100, 3, Unit::Volts);
        assert_eq!(text(&level, ParamValue::Number(1_000)).as_str(), "1.000 V");
        let note = ParamSpec::number("NOTE", (24, 108), 1, 0, Unit::Note);
        assert_eq!(text(&note, ParamValue::Number(69)).as_str(), "A4");
        let on = ParamSpec::toggle("STEREO");
        assert_eq!(text(&on, ParamValue::Toggle(true)).as_str(), "ON");
        assert!(matches!(on.kind, Kind::Toggle));
    }
}
