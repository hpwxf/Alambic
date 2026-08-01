//! Rendering the module's framebuffer as braille characters.
//!
//! A braille cell carries a 2x4 dot matrix, so the 128x64 screen fits in
//! 64x16 characters — small enough for any terminal while keeping every pixel
//! individually visible.

use oc_core::framebuffer::{FrameBuffer, HEIGHT, WIDTH};

/// First code point of the braille patterns block.
const BRAILLE_BASE: u32 = 0x2800;

/// Bit weight of each dot, indexed by `[column][row]` inside the 2x4 cell.
///
/// The Unicode block numbers the dots 1-2-3-7 down the left column and
/// 4-5-6-8 down the right one, which is why the last row is not contiguous.
const DOT_WEIGHT: [[u8; 4]; 2] = [[0x01, 0x02, 0x04, 0x40], [0x08, 0x10, 0x20, 0x80]];

/// Characters per rendered row.
pub const COLUMNS: usize = WIDTH / 2;

/// Number of rendered rows.
pub const LINES: usize = HEIGHT / 4;

/// Renders the framebuffer as [`LINES`] strings of [`COLUMNS`] characters.
#[must_use]
pub fn render(frame: &FrameBuffer) -> Vec<String> {
    (0..LINES).map(|line| render_line(frame, line)).collect()
}

/// Renders a single braille row of the framebuffer.
#[must_use]
pub fn render_line(frame: &FrameBuffer, line: usize) -> String {
    let mut rendered = String::with_capacity(COLUMNS);
    for column in 0..COLUMNS {
        rendered.push(cell(frame, column, line));
    }
    rendered
}

/// The braille character covering one 2x4 block of pixels.
fn cell(frame: &FrameBuffer, column: usize, line: usize) -> char {
    let base_x = i32::try_from(column * 2).unwrap_or(i32::MAX);
    let base_y = i32::try_from(line * 4).unwrap_or(i32::MAX);

    let mut dots = 0u8;
    for (offset_x, weights) in DOT_WEIGHT.iter().enumerate() {
        for (offset_y, weight) in weights.iter().enumerate() {
            let x = base_x + i32::try_from(offset_x).unwrap_or(0);
            let y = base_y + i32::try_from(offset_y).unwrap_or(0);
            if frame.pixel(x, y) {
                dots |= weight;
            }
        }
    }

    char::from_u32(BRAILLE_BASE + u32::from(dots)).unwrap_or('?')
}

#[cfg(test)]
mod tests {
    use oc_core::framebuffer::FrameBuffer;

    use super::{COLUMNS, LINES, render, render_line};

    #[test]
    fn an_empty_screen_renders_as_blank_braille() {
        let frame = FrameBuffer::new();
        let lines = render(&frame);
        assert_eq!(lines.len(), LINES);
        for line in &lines {
            assert_eq!(line.chars().count(), COLUMNS);
            assert!(line.chars().all(|glyph| glyph == '\u{2800}'));
        }
    }

    #[test]
    fn a_full_screen_renders_as_solid_braille() {
        let mut frame = FrameBuffer::new();
        frame.fill();
        for line in render(&frame) {
            assert!(line.chars().all(|glyph| glyph == '\u{28FF}'));
        }
    }

    #[test]
    fn the_top_left_pixel_lights_the_first_dot() {
        let mut frame = FrameBuffer::new();
        frame.set_pixel(0, 0, true);
        let line = render_line(&frame, 0);
        assert_eq!(line.chars().next(), Some('\u{2801}'));
    }

    #[test]
    fn each_dot_of_a_cell_maps_to_a_distinct_bit() {
        let expected = [
            ((0, 0), '\u{2801}'),
            ((0, 1), '\u{2802}'),
            ((0, 2), '\u{2804}'),
            ((0, 3), '\u{2840}'),
            ((1, 0), '\u{2808}'),
            ((1, 1), '\u{2810}'),
            ((1, 2), '\u{2820}'),
            ((1, 3), '\u{2880}'),
        ];
        for ((x, y), glyph) in expected {
            let mut frame = FrameBuffer::new();
            frame.set_pixel(x, y, true);
            assert_eq!(
                render_line(&frame, 0).chars().next(),
                Some(glyph),
                "pixel ({x}, {y})"
            );
        }
    }

    #[test]
    fn pixels_land_in_the_right_cell() {
        let mut frame = FrameBuffer::new();
        frame.set_pixel(127, 63, true);
        let lines = render(&frame);
        assert_eq!(lines[LINES - 1].chars().last(), Some('\u{2880}'));
        assert!(lines[0].chars().all(|glyph| glyph == '\u{2800}'));
    }
}
