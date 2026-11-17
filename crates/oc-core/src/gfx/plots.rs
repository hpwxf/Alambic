//! Plots: signals and measurements drawn as curves and scales (journal/D-030).
//!
//! Values are mapped into a box (`x`, `y`, `width`, `height`): a value of
//! `range.0` lands on the bottom row, `range.1` on the top one, anything
//! outside is clamped to the box.

use embedded_graphics::prelude::Point;

use super::{Ink, Ratio, fill_rect, hline, polyline, vline};
use crate::framebuffer::FrameBuffer;

/// A plotting area and the values its bottom and top rows stand for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Area {
    /// Left column.
    pub x: i32,
    /// Top row.
    pub y: i32,
    /// Columns.
    pub width: i32,
    /// Rows.
    pub height: i32,
    /// The values of the bottom and the top row.
    pub range: (i32, i32),
}

impl Area {
    /// The row of `value`, clamped to the area.
    #[must_use]
    pub fn row(&self, value: i32) -> i32 {
        let (low, high) = self.range;
        if high <= low || self.height <= 0 {
            return self.y + self.height - 1;
        }
        let rows = self.height - 1;
        let offset = Ratio::new(value - low, high - low).of(rows);
        self.y + rows - offset
    }
}

/// A trace: one sample per column from the area's left edge, consecutive
/// samples joined, as a scope draws a signal.
pub fn trace(frame: &mut FrameBuffer, area: Area, samples: &[i32]) {
    let points = (area.x..area.x + area.width)
        .zip(samples)
        .map(|(x, &value)| Point::new(x, area.row(value)));
    polyline(frame, points, Ink::Lit);
}

/// A waveform as a sample editor shows it: per column, a vertical line from
/// the lowest to the highest value that column covers.
pub fn min_max(frame: &mut FrameBuffer, area: Area, columns: &[(i32, i32)]) {
    for (x, &(low, high)) in (area.x..area.x + area.width).zip(columns) {
        let top = area.row(high.max(low));
        let bottom = area.row(low.min(high));
        vline(frame, x, top, bottom - top + 1, Ink::Lit);
    }
}

/// Bars rising from the bottom of the area, `bar` columns wide and `gap`
/// apart, as a spectrum.
pub fn histogram(frame: &mut FrameBuffer, area: Area, values: &[i32], bar: i32, gap: i32) {
    let bottom = area.y + area.height - 1;
    let mut x = area.x;
    for &value in values {
        if x + bar > area.x + area.width {
            break;
        }
        let top = area.row(value);
        fill_rect(frame, x, top, bar, bottom - top + 1, Ink::Lit);
        x += bar + gap;
    }
}

/// A scale: a baseline along the bottom of the box, `ticks` evenly spaced
/// marks (the middle one taller), and a needle at `value` across the height,
/// as a tuner shows how far a note is.
pub fn needle_scale(
    frame: &mut FrameBuffer,
    x: i32,
    y: i32,
    width: i32,
    height: i32,
    ticks: i32,
    value: Ratio,
) {
    let bottom = y + height - 1;
    hline(frame, x, bottom, width, Ink::Lit);
    if ticks > 1 {
        for tick in 0..ticks {
            let column = x + Ratio::new(tick, ticks - 1).of(width - 1);
            let tall = if tick * 2 == ticks - 1 {
                height / 2
            } else {
                height / 4
            };
            vline(frame, column, bottom - tall, tall, Ink::Lit);
        }
    }
    let needle = x + value.of(width - 1);
    fill_rect(frame, needle - 1, y, 3, height - 1, Ink::Lit);
}
