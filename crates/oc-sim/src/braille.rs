//! Rendering the module's framebuffer as dense terminal characters.
//!
//! Each cell is a 2x4 pseudo-pixel matrix, so the 128x64 screen fits in
//! 64x16 characters — small enough for any terminal while keeping every pixel
//! individually visible.
//!
//! By default the renderer uses classic Unicode *braille* patterns, which
//! almost every terminal font covers. Enable the `octant` cargo feature of
//! `oc-sim` to switch to denser octant glyphs from the Symbols for Legacy
//! Computing Supplement: same 2x4 resolution, but regularly spaced
//! pseudo-pixels that remove the horizontal banding many fonts leave between
//! braille rows. Octants need a recent font; without it the screen shows
//! replacement characters (`�`).

use oc_core::framebuffer::{FrameBuffer, HEIGHT, WIDTH};

#[cfg(not(feature = "octant"))]
use ratatui::symbols::braille::BRAILLE as GLYPHS;
#[cfg(feature = "octant")]
use ratatui::symbols::pixel::OCTANTS as GLYPHS;

/// Characters per rendered row.
pub const COLUMNS: usize = WIDTH / 2;

/// Number of rendered rows.
pub const LINES: usize = HEIGHT / 4;

/// Renders the framebuffer as [`LINES`] strings of [`COLUMNS`] characters.
#[must_use]
pub fn render(frame: &FrameBuffer) -> Vec<String> {
    (0..LINES).map(|line| render_line(frame, line)).collect()
}

/// Renders a single glyph row of the framebuffer.
#[must_use]
pub fn render_line(frame: &FrameBuffer, line: usize) -> String {
    let mut rendered = String::with_capacity(COLUMNS);
    for column in 0..COLUMNS {
        rendered.push(cell(frame, column, line));
    }
    rendered
}

/// The 2x4 glyph covering one block of pixels.
///
/// Both the braille and octant lookup tables from ratatui are indexed by the
/// same row-major bit pattern:
///
/// ```text
/// | 0 1 |
/// | 2 3 |
/// | 4 5 |
/// | 6 7 |
/// ```
fn cell(frame: &FrameBuffer, column: usize, line: usize) -> char {
    let base_x = i32::try_from(column * 2).unwrap_or(i32::MAX);
    let base_y = i32::try_from(line * 4).unwrap_or(i32::MAX);

    let mut pattern = 0u8;
    for offset_y in 0..4usize {
        for offset_x in 0..2usize {
            let x = base_x + i32::try_from(offset_x).unwrap_or(0);
            let y = base_y + i32::try_from(offset_y).unwrap_or(0);
            if frame.pixel(x, y) {
                pattern |= 1u8 << (offset_x + 2 * offset_y);
            }
        }
    }

    GLYPHS[usize::from(pattern)]
}

#[cfg(test)]
mod tests {
    use oc_core::framebuffer::FrameBuffer;

    use super::{COLUMNS, GLYPHS, LINES, render, render_line};

    #[test]
    fn an_empty_screen_renders_as_blank_glyphs() {
        let frame = FrameBuffer::new();
        let lines = render(&frame);
        assert_eq!(lines.len(), LINES);
        for line in &lines {
            assert_eq!(line.chars().count(), COLUMNS);
            assert!(line.chars().all(|glyph| glyph == GLYPHS[0]));
        }
    }

    #[test]
    fn a_full_screen_renders_as_solid_glyphs() {
        let mut frame = FrameBuffer::new();
        frame.fill();
        for line in render(&frame) {
            assert!(line.chars().all(|glyph| glyph == GLYPHS[0xFF]));
        }
    }

    #[test]
    fn the_top_left_pixel_lights_the_first_bit() {
        let mut frame = FrameBuffer::new();
        frame.set_pixel(0, 0, true);
        let line = render_line(&frame, 0);
        assert_eq!(line.chars().next(), Some(GLYPHS[0x01]));
    }

    #[test]
    fn each_dot_of_a_cell_maps_to_a_distinct_bit() {
        // Row-major: bit = x + 2*y inside the 2x4 cell.
        let expected = [
            ((0, 0), 0x01),
            ((1, 0), 0x02),
            ((0, 1), 0x04),
            ((1, 1), 0x08),
            ((0, 2), 0x10),
            ((1, 2), 0x20),
            ((0, 3), 0x40),
            ((1, 3), 0x80),
        ];
        for ((x, y), bit) in expected {
            let mut frame = FrameBuffer::new();
            frame.set_pixel(x, y, true);
            assert_eq!(
                render_line(&frame, 0).chars().next(),
                Some(GLYPHS[bit]),
                "pixel ({x}, {y})"
            );
        }
    }

    #[test]
    fn pixels_land_in_the_right_cell() {
        let mut frame = FrameBuffer::new();
        frame.set_pixel(127, 63, true);
        let lines = render(&frame);
        assert_eq!(lines[LINES - 1].chars().last(), Some(GLYPHS[0x80]));
        assert!(lines[0].chars().all(|glyph| glyph == GLYPHS[0]));
    }
}
