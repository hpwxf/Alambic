//! What the tool prints: the profile as a table, the screen as text or as
//! a PNG image (with the `cli` feature).

use std::fmt::Write as _;
#[cfg(feature = "cli")]
use std::io;

use oc_core::framebuffer::{FrameBuffer, HEIGHT, LEN, WIDTH};
use oc_core::profile::{JITTER_BUCKETS, Profile, Stage, Stat};
use oc_protocol::Sample;

/// `fine` counts as microseconds, two decimals: the voice's stages take
/// a fraction of a microsecond (journal/D-039).
fn micros(profile: &Profile, fine: u32) -> String {
    let nanos = profile.nanos(fine);
    format!("{}.{:02}", nanos / 1_000, nanos % 1_000 / 10)
}

/// One row of the table: a stage, or the period between two ticks, which
/// the profile keeps apart from the stages.
#[derive(Clone, Copy)]
enum Row {
    Stage(Stage),
    Period,
}

impl Row {
    fn name(self) -> &'static str {
        match self {
            Self::Stage(stage) => stage.name(),
            Self::Period => "period",
        }
    }
}

/// The table's sections, each row with a short note: the reader of
/// `cargo probe stats` should not need the source to know what a row times
/// or which other row it sits inside.
const GROUPS: [(&str, &[(Row, &str)]); 4] = [
    (
        "loop, one pass per tick",
        &[
            (Row::Period, "start to start of two passes"),
            (
                Row::Stage(Stage::Sample),
                "ADC conversions, triggers, panel, before the tick",
            ),
            (
                Row::Stage(Stage::Tick),
                "Engine::tick, whole: the next section, render, present, page",
            ),
            (
                Row::Stage(Stage::Publish),
                "outputs' parameters handed to the audio interrupt",
            ),
            (Row::Stage(Stage::Usb), "USB poll and probe requests"),
        ],
    ),
    (
        "inside the tick",
        &[
            (
                Row::Stage(Stage::Acquire),
                "panel and inputs read into the engine",
            ),
            (
                Row::Stage(Stage::Update),
                "applet, wizard or menu deciding the outputs",
            ),
            (
                Row::Stage(Stage::Outputs),
                "outputs converted and written (host backends)",
            ),
        ],
    ),
    (
        "screen",
        &[
            (
                Row::Stage(Stage::Render),
                "drawing a frame, in the tick, on the ticks that redraw",
            ),
            (
                Row::Stage(Stage::Present),
                "handing over a whole frame, in the tick",
            ),
            (
                Row::Stage(Stage::Page),
                "sending one page per tick (paged display)",
            ),
            (
                Row::Stage(Stage::Frame),
                "a frame's transfer to the OLED, piece by piece (wall time)",
            ),
        ],
    ),
    (
        "audio interrupt",
        &[
            (
                Row::Stage(Stage::AudioPeriod),
                "start to start of two runs: the sample rate",
            ),
            (
                Row::Stage(Stage::Audio),
                "one run: a sample to the DAC, a piece of the screen",
            ),
            (
                Row::Stage(Stage::Voice),
                "rendering one sample of the voice, inside audio",
            ),
            (
                Row::Stage(Stage::VoiceParams),
                "taking new parameters, inside audio",
            ),
        ],
    ),
];

/// Width of the name column: the longest row's name (`audio_period`) plus
/// the indent under a section title, so that every row's numbers line up
/// under the header.
fn name_width() -> usize {
    GROUPS
        .iter()
        .flat_map(|(_, rows)| rows.iter().map(|(row, _)| row.name().len()))
        .max()
        .unwrap_or(0)
        + INDENT.len()
}

/// What sets a row apart from its section's title.
const INDENT: &str = "  ";

fn stat_row(out: &mut String, name: &str, profile: &Profile, stat: &Stat, note: &str) {
    let width = name_width();
    let cell =
        |value: Option<u32>| value.map_or_else(|| "-".to_owned(), |fine| micros(profile, fine));
    let name = format!("{INDENT}{name}");
    let _ = writeln!(
        out,
        "{name:<width$} {:>9} {:>9} {:>9} {:>9}  {note}",
        stat.count(),
        cell(stat.min()),
        cell(stat.mean()),
        cell(stat.max())
    );
}

/// The profile as a table: the stages that ran, by section, each with a
/// note, then the jitter histogram.
#[must_use]
pub fn profile_table(profile: &Profile) -> String {
    let mut out = String::new();
    if !profile.is_enabled() {
        out.push_str("this module has no fine counter: nothing is timed\n");
        return out;
    }
    let width = name_width();
    let _ = writeln!(
        out,
        "{:<width$} {:>9} {:>9} {:>9} {:>9}  what",
        "us", "count", "min", "mean", "max"
    );
    for (title, rows) in GROUPS {
        let ran: Vec<(&Stat, Row, &str)> = rows
            .iter()
            .map(|&(row, note)| {
                let stat = match row {
                    Row::Stage(stage) => profile.stage(stage),
                    Row::Period => profile.period(),
                };
                (stat, row, note)
            })
            .filter(|(stat, row, _)| stat.count() > 0 || matches!(row, Row::Period))
            .collect();
        if ran.is_empty() {
            continue;
        }
        let _ = writeln!(out, "{title}");
        for (stat, row, note) in ran {
            stat_row(&mut out, row.name(), profile, stat, note);
        }
    }

    let expected = profile.expected_period_micros();
    if expected > 0 {
        let _ = writeln!(out, "\njitter against {expected} us:");
        let total: u64 = profile.jitter().iter().map(|&count| u64::from(count)).sum();
        for (bucket, &count) in profile.jitter().iter().enumerate() {
            if count == 0 {
                continue;
            }
            let range = match bucket {
                0 => "< 1 us".to_owned(),
                last if last == JITTER_BUCKETS - 1 => format!(">= {} us", 1_u64 << (last - 1)),
                _ => format!("{}..{} us", 1_u64 << (bucket - 1), 1_u64 << bucket),
            };
            let share = u64::from(count) * 1_000 / total.max(1);
            let _ = writeln!(
                out,
                "  {range:>14} {count:>9}  {}.{}%",
                share / 10,
                share % 10
            );
        }
    }
    out
}

/// One sample as a line.
#[must_use]
pub fn sample_line(sample: &Sample) -> String {
    // Trigger 1 first, as on the panel.
    let triggers: String = (0..4)
        .map(|index| {
            if sample.triggers & (1 << index) == 0 {
                '.'
            } else {
                char::from(b'1' + index)
            }
        })
        .collect();
    format!(
        "tick {:>9}  in mV {:>6?}  codes {:>4?}  out mV {:>6?}  triggers {triggers}",
        sample.tick, sample.cv_in, sample.cv_codes, sample.cv_out
    )
}

fn frame_of(screen: &[u8; LEN]) -> FrameBuffer {
    let mut frame = FrameBuffer::new();
    frame.as_mut_bytes().copy_from_slice(screen);
    frame
}

/// The screen as text, two pixel rows per line with half blocks.
#[must_use]
pub fn screen_text(screen: &[u8; LEN]) -> String {
    let frame = frame_of(screen);
    let mut out = String::new();
    for y in (0..HEIGHT).step_by(2) {
        for x in 0..WIDTH {
            let (Ok(x), Ok(top), Ok(bottom)) =
                (i32::try_from(x), i32::try_from(y), i32::try_from(y + 1))
            else {
                continue;
            };
            out.push(match (frame.pixel(x, top), frame.pixel(x, bottom)) {
                (true, true) => '█',
                (true, false) => '▀',
                (false, true) => '▄',
                (false, false) => ' ',
            });
        }
        out.push('\n');
    }
    out
}

/// The screen as 8-bit grey levels, row after row, each pixel `scale` times
/// wider and taller, lit pixels white (`0xFF`) on black; with its width and
/// height. The PNG output and the browser tool's canvas both draw from it.
#[must_use]
pub fn screen_grey(screen: &[u8; LEN], scale: u32) -> (u32, u32, Vec<u8>) {
    let frame = frame_of(screen);
    let scale = scale.max(1);
    let width = u32::try_from(WIDTH).unwrap_or(128) * scale;
    let height = u32::try_from(HEIGHT).unwrap_or(64) * scale;
    let mut pixels = Vec::with_capacity((width * height) as usize);
    for y in 0..height {
        for x in 0..width {
            let lit = match (i32::try_from(x / scale), i32::try_from(y / scale)) {
                (Ok(x), Ok(y)) => frame.pixel(x, y),
                _ => false,
            };
            pixels.push(if lit { 0xFF } else { 0x00 });
        }
    }
    (width, height, pixels)
}

/// The screen as a PNG, each pixel `scale` times wider and taller, lit
/// pixels white on black.
///
/// # Errors
///
/// When `out` fails.
#[cfg(feature = "cli")]
pub fn screen_png(screen: &[u8; LEN], scale: u32, out: impl io::Write) -> io::Result<()> {
    let (width, height, pixels) = screen_grey(screen, scale);
    let mut encoder = png::Encoder::new(out, width, height);
    encoder.set_color(png::ColorType::Grayscale);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header().map_err(io::Error::other)?;
    writer.write_image_data(&pixels).map_err(io::Error::other)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_stage_sits_in_exactly_one_section() {
        for stage in Stage::ALL {
            let seen = GROUPS
                .iter()
                .flat_map(|(_, rows)| rows.iter())
                .filter(|(row, _)| matches!(row, Row::Stage(listed) if *listed == stage))
                .count();
            assert_eq!(seen, 1, "{}", stage.name());
        }
    }
}
