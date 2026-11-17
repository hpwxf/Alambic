//! Graphics primitives for the applets' screens (journal/D-030).
//!
//! The bases here are the shapes every screen is built from. The straight
//! ones (horizontal and vertical lines, rectangles, inverted areas, dotted
//! lines) write whole framebuffer bytes through masks, as [`crate::text`] does
//! for glyphs, instead of one pixel at a time; the tests check they light the
//! same pixels as `embedded-graphics`. Lines and circles are drawn here too,
//! rather than through `embedded-graphics`, so that every shape can also
//! *invert* what it covers ([`Ink::Invert`]), the way a highlight is drawn over
//! a lit area.
//!
//! Coordinates are pixels from the top-left corner; anything off the screen is
//! clipped. Sizes are `i32` like `embedded-graphics` coordinates; a size of
//! zero or less draws nothing.

pub mod trig;

use embedded_graphics::prelude::Point;

use crate::framebuffer::{FrameBuffer, HEIGHT_I32, PAGES, WIDTH, WIDTH_I32};

/// What a shape does to the pixels it covers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ink {
    /// Lights them.
    Lit,
    /// Darkens them.
    Dark,
    /// Flips them: lit becomes dark and dark lit (inverse video).
    Invert,
}

impl Ink {
    /// Applies the ink to the pixels set in `mask` of one byte.
    const fn apply(self, byte: &mut u8, mask: u8) {
        match self {
            Self::Lit => *byte |= mask,
            Self::Dark => *byte &= !mask,
            Self::Invert => *byte ^= mask,
        }
    }
}

/// The columns `x..x + width` clipped to the screen, as byte offsets in a page.
fn clip_columns(x: i32, width: i32) -> Option<core::ops::Range<usize>> {
    let start = x.max(0);
    let end = x.saturating_add(width).min(WIDTH_I32);
    let start = usize::try_from(start).ok()?;
    let end = usize::try_from(end).ok()?;
    (start < end).then_some(start..end)
}

/// The rows `y..y + height` clipped to the screen, as `(first, last)`.
fn clip_rows(y: i32, height: i32) -> Option<(i32, i32)> {
    let first = y.max(0);
    let last = y.saturating_add(height).min(HEIGHT_I32) - 1;
    (first <= last).then_some((first, last))
}

/// Fills the rectangle at `x`, `y`, `width` x `height`, a page at a time: one
/// mask per page, applied to every column of the rectangle.
pub fn fill_rect(frame: &mut FrameBuffer, x: i32, y: i32, width: i32, height: i32, ink: Ink) {
    let (Some(columns), Some((first, last))) = (clip_columns(x, width), clip_rows(y, height))
    else {
        return;
    };
    let bytes = frame.as_mut_bytes();
    for page in first / 8..=last / 8 {
        // Rows of this page inside the rectangle, as bits.
        let top = (first - page * 8).max(0);
        let bottom = (last - page * 8).min(7);
        let mask = (0xFF_u8 << top) & (0xFF_u8 >> (7 - bottom));
        let Ok(page) = usize::try_from(page) else {
            continue;
        };
        for byte in &mut bytes[page * WIDTH + columns.start..page * WIDTH + columns.end] {
            ink.apply(byte, mask);
        }
    }
}

/// A horizontal line from `x` over `width` pixels.
pub fn hline(frame: &mut FrameBuffer, x: i32, y: i32, width: i32, ink: Ink) {
    fill_rect(frame, x, y, width, 1, ink);
}

/// A vertical line from `y` over `height` pixels.
pub fn vline(frame: &mut FrameBuffer, x: i32, y: i32, height: i32, ink: Ink) {
    fill_rect(frame, x, y, 1, height, ink);
}

/// The outline of the rectangle at `x`, `y`, `width` x `height`, one pixel
/// wide and inside it, as `embedded-graphics` strokes a rectangle.
pub fn rect(frame: &mut FrameBuffer, x: i32, y: i32, width: i32, height: i32, ink: Ink) {
    if width <= 0 || height <= 0 {
        return;
    }
    hline(frame, x, y, width, ink);
    if height > 1 {
        hline(frame, x, y + height - 1, width, ink);
    }
    // The sides between the two edges, so that no pixel is inked twice (which
    // `Ink::Invert` would undo).
    if height > 2 {
        vline(frame, x, y + 1, height - 2, ink);
        if width > 1 {
            vline(frame, x + width - 1, y + 1, height - 2, ink);
        }
    }
}

/// A dotted horizontal line: one pixel every `step` from `x`, over `width`.
pub fn dotted_hline(frame: &mut FrameBuffer, x: i32, y: i32, width: i32, step: i32, ink: Ink) {
    let step = step.max(1);
    let mut column = x;
    while column < x.saturating_add(width) {
        pixel(frame, column, y, ink);
        column += step;
    }
}

/// A dotted vertical line: one pixel every `step` from `y`, over `height`.
pub fn dotted_vline(frame: &mut FrameBuffer, x: i32, y: i32, height: i32, step: i32, ink: Ink) {
    let step = step.max(1);
    let mut row = y;
    while row < y.saturating_add(height) {
        pixel(frame, x, row, ink);
        row += step;
    }
}

/// One pixel.
pub fn pixel(frame: &mut FrameBuffer, x: i32, y: i32, ink: Ink) {
    let (Ok(column), Ok(row)) = (usize::try_from(x), usize::try_from(y)) else {
        return;
    };
    if column >= WIDTH || row >= PAGES * 8 {
        return;
    }
    let index = (row / 8) * WIDTH + column;
    let mask = 1_u8 << (row % 8);
    ink.apply(&mut frame.as_mut_bytes()[index], mask);
}

/// A straight line between two points, both included (Bresenham).
pub fn line(frame: &mut FrameBuffer, from: Point, to: Point, ink: Ink) {
    let dx = (to.x - from.x).abs();
    let dy = -(to.y - from.y).abs();
    let step_x = if from.x < to.x { 1 } else { -1 };
    let step_y = if from.y < to.y { 1 } else { -1 };
    let mut error = dx + dy;
    let mut at = from;
    loop {
        pixel(frame, at.x, at.y, ink);
        if at == to {
            return;
        }
        let doubled = 2 * error;
        if doubled >= dy {
            error += dy;
            at.x += step_x;
        }
        if doubled <= dx {
            error += dx;
            at.y += step_y;
        }
    }
}

/// The outline of the circle of `radius` around `center` (midpoint
/// algorithm): every pixel once, so [`Ink::Invert`] flips the ring cleanly.
pub fn circle(frame: &mut FrameBuffer, center: Point, radius: i32, ink: Ink) {
    if radius < 0 {
        return;
    }
    if radius == 0 {
        pixel(frame, center.x, center.y, ink);
        return;
    }
    let (mut x, mut y) = (radius, 0);
    let mut error = 1 - radius;
    while x >= y {
        // The eight octants; on the diagonals and axes some coincide, and
        // each distinct point is inked once.
        let points = [
            (x, y),
            (y, x),
            (-y, x),
            (-x, y),
            (-x, -y),
            (-y, -x),
            (y, -x),
            (x, -y),
        ];
        for (index, &(px, py)) in points.iter().enumerate() {
            if !points[..index].contains(&(px, py)) {
                pixel(frame, center.x + px, center.y + py, ink);
            }
        }
        y += 1;
        if error < 0 {
            error += 2 * y + 1;
        } else {
            x -= 1;
            error += 2 * (y - x) + 1;
        }
    }
}

/// The disc of `radius` around `center`, as horizontal spans.
pub fn fill_circle(frame: &mut FrameBuffer, center: Point, radius: i32, ink: Ink) {
    if radius < 0 {
        return;
    }
    for dy in -radius..=radius {
        // Half the span of row `dy`: the widest `dx` inside the circle, with
        // the same rounding as the outline's midpoint test.
        let mut dx = 0;
        while (dx + 1) * (dx + 1) + dy * dy <= radius * radius + radius {
            dx += 1;
        }
        hline(frame, center.x - dx, center.y + dy, 2 * dx + 1, ink);
    }
}

#[cfg(test)]
mod tests {
    use embedded_graphics::Drawable as _;
    use embedded_graphics::pixelcolor::BinaryColor;
    use embedded_graphics::prelude::{Point, Primitive as _, Size};
    use embedded_graphics::primitives::{PrimitiveStyle, Rectangle};

    use super::{Ink, circle, dotted_hline, fill_circle, fill_rect, hline, line, rect, vline};
    use crate::framebuffer::{FrameBuffer, HEIGHT_I32, WIDTH_I32};

    /// A busy background, so that darkening and inverting show.
    fn background() -> FrameBuffer {
        let mut frame = FrameBuffer::new();
        let mut seed = 0x5A_u8;
        for byte in frame.as_mut_bytes() {
            seed = seed.wrapping_mul(37).wrapping_add(11);
            *byte = seed;
        }
        frame
    }

    /// `embedded-graphics`' rectangle, filled or stroked.
    fn reference_rect(
        x: i32,
        y: i32,
        width: i32,
        height: i32,
        color: BinaryColor,
        filled: bool,
    ) -> FrameBuffer {
        let mut frame = background();
        let size = Size::new(width.max(0).unsigned_abs(), height.max(0).unsigned_abs());
        let style = if filled {
            PrimitiveStyle::with_fill(color)
        } else {
            PrimitiveStyle::with_stroke(color, 1)
        };
        let _ = Rectangle::new(Point::new(x, y), size)
            .into_styled(style)
            .draw(&mut frame);
        frame
    }

    /// Rectangles across page boundaries, on the edges and partly off screen.
    const RECTANGLES: [(i32, i32, i32, i32); 9] = [
        (0, 0, 128, 64),
        (3, 5, 10, 1),
        (3, 5, 1, 10),
        (10, 6, 20, 12),
        (-4, -3, 9, 9),
        (120, 60, 20, 20),
        (40, 8, 30, 8),
        (64, 31, 2, 2),
        (5, 5, 0, 7),
    ];

    #[test]
    fn filled_rectangles_light_and_darken_what_embedded_graphics_does() {
        for (x, y, width, height) in RECTANGLES {
            for (ink, color) in [(Ink::Lit, BinaryColor::On), (Ink::Dark, BinaryColor::Off)] {
                let mut ours = background();
                fill_rect(&mut ours, x, y, width, height, ink);
                assert!(
                    ours == reference_rect(x, y, width, height, color, true),
                    "fill {x},{y} {width}x{height} {ink:?}"
                );
            }
        }
    }

    #[test]
    fn outlines_light_what_embedded_graphics_strokes() {
        for (x, y, width, height) in RECTANGLES {
            for (ink, color) in [(Ink::Lit, BinaryColor::On), (Ink::Dark, BinaryColor::Off)] {
                let mut ours = background();
                rect(&mut ours, x, y, width, height, ink);
                assert!(
                    ours == reference_rect(x, y, width, height, color, false),
                    "outline {x},{y} {width}x{height} {ink:?}"
                );
            }
        }
    }

    #[test]
    fn inverting_flips_exactly_the_covered_pixels() {
        for (x, y, width, height) in RECTANGLES {
            let mut frame = background();
            fill_rect(&mut frame, x, y, width, height, Ink::Invert);
            for row in 0..HEIGHT_I32 {
                for column in 0..WIDTH_I32 {
                    let inside = column >= x && column < x + width && row >= y && row < y + height;
                    assert_eq!(
                        frame.pixel(column, row),
                        background().pixel(column, row) ^ inside,
                        "{column},{row} for {x},{y} {width}x{height}"
                    );
                }
            }
            // Twice is nothing, outlines included (no pixel inked twice).
            fill_rect(&mut frame, x, y, width, height, Ink::Invert);
            rect(&mut frame, x, y, width, height, Ink::Invert);
            rect(&mut frame, x, y, width, height, Ink::Invert);
            assert_eq!(frame, background());
        }
    }

    #[test]
    fn straight_lines_are_one_pixel_rectangles() {
        let mut ours = FrameBuffer::new();
        hline(&mut ours, 2, 9, 7, Ink::Lit);
        vline(&mut ours, 30, 4, 20, Ink::Lit);
        let mut reference = FrameBuffer::new();
        fill_rect(&mut reference, 2, 9, 7, 1, Ink::Lit);
        fill_rect(&mut reference, 30, 4, 1, 20, Ink::Lit);
        assert_eq!(ours, reference);
        assert_eq!(ours.lit_pixels(), 27);
    }

    #[test]
    fn a_dotted_line_lights_one_pixel_per_step() {
        let mut frame = FrameBuffer::new();
        dotted_hline(&mut frame, 0, 3, 10, 3, Ink::Lit);
        assert_eq!(frame.lit_pixels(), 4);
        assert!(frame.pixel(9, 3) && !frame.pixel(8, 3));
    }

    #[test]
    fn a_line_joins_its_ends_one_pixel_per_step() {
        for (from, to) in [
            (Point::new(0, 0), Point::new(127, 63)),
            (Point::new(10, 50), Point::new(14, 2)),
            (Point::new(5, 5), Point::new(5, 5)),
            (Point::new(100, 20), Point::new(20, 22)),
        ] {
            let mut frame = FrameBuffer::new();
            line(&mut frame, from, to, Ink::Lit);
            assert!(frame.pixel(from.x, from.y) && frame.pixel(to.x, to.y));
            let steps = (to.x - from.x).abs().max((to.y - from.y).abs());
            assert_eq!(i64::from(frame.lit_pixels()), i64::from(steps) + 1);
        }
    }

    #[test]
    fn a_circle_is_symmetric_and_at_its_radius() {
        for radius in 0..12 {
            let center = Point::new(64, 32);
            let mut frame = FrameBuffer::new();
            circle(&mut frame, center, radius, Ink::Lit);
            for dy in -radius..=radius {
                for dx in -radius..=radius {
                    let lit = frame.pixel(center.x + dx, center.y + dy);
                    assert_eq!(lit, frame.pixel(center.x - dx, center.y + dy));
                    assert_eq!(lit, frame.pixel(center.x + dy, center.y + dx));
                    if lit {
                        let distance = dx * dx + dy * dy;
                        assert!(
                            (distance - radius * radius).abs() <= radius + 1,
                            "r={radius} {dx},{dy}"
                        );
                    }
                }
            }
            // Inverting twice restores: every point was inked once.
            circle(&mut frame, center, radius, Ink::Invert);
            assert_eq!(frame.lit_pixels(), 0, "radius {radius}");
        }
    }

    #[test]
    fn a_disc_covers_its_circle() {
        for radius in 0..12 {
            let center = Point::new(20, 40);
            let mut outline = FrameBuffer::new();
            circle(&mut outline, center, radius, Ink::Lit);
            let mut disc = FrameBuffer::new();
            fill_circle(&mut disc, center, radius, Ink::Lit);
            for row in 0..HEIGHT_I32 {
                for column in 0..WIDTH_I32 {
                    if outline.pixel(column, row) {
                        assert!(disc.pixel(column, row), "r={radius} {column},{row}");
                    }
                }
            }
        }
    }
}
