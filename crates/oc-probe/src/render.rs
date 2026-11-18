//! What the tool prints: the profile as a table, the screen as text or as
//! a PNG image (with the `cli` feature).

use std::fmt::Write as _;
#[cfg(feature = "cli")]
use std::io;

use oc_core::framebuffer::{FrameBuffer, HEIGHT, LEN, WIDTH};
use oc_core::profile::{JITTER_BUCKETS, Profile, Stage, Stat};
use oc_protocol::Sample;

/// `fine` counts as microseconds, one decimal.
fn micros(profile: &Profile, fine: u32) -> String {
    let nanos = profile.nanos(fine);
    format!("{}.{}", nanos / 1_000, nanos % 1_000 / 100)
}

/// Width of the stage column: the longest stage's name (`audio_period`), so
/// that every row's numbers line up under the header.
fn name_width() -> usize {
    Stage::ALL
        .iter()
        .map(|stage| stage.name().len())
        .chain(["period".len()])
        .max()
        .unwrap_or(0)
}

fn stat_row(out: &mut String, name: &str, profile: &Profile, stat: &Stat) {
    let width = name_width();
    let cell =
        |value: Option<u32>| value.map_or_else(|| "-".to_owned(), |fine| micros(profile, fine));
    let _ = writeln!(
        out,
        "{name:<width$} {:>9} {:>9} {:>9} {:>9}",
        stat.count(),
        cell(stat.min()),
        cell(stat.mean()),
        cell(stat.max())
    );
}

/// The profile as a table: one row per stage that ran, the period, then
/// the jitter histogram.
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
        "{:<width$} {:>9} {:>9} {:>9} {:>9}",
        "us", "count", "min", "mean", "max"
    );
    for stage in Stage::ALL {
        let stat = profile.stage(stage);
        if stat.count() > 0 {
            stat_row(&mut out, stage.name(), profile, stat);
        }
    }
    stat_row(&mut out, "period", profile, profile.period());

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
