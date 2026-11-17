//! Text in the module's 5x8 font, laid straight into the framebuffer's page
//! layout (journal/D-009).
//!
//! The font is `embedded-graphics`' `FONT_5X8`, whose glyphs are stored row by
//! row, one bit per pixel: drawing through `embedded-graphics` sets every
//! glyph pixel on its own, about forty iterations per character. The
//! framebuffer is stored column by column, eight rows to a byte, and the font
//! is exactly eight rows tall, so each glyph column is one byte: [`GLYPHS`]
//! holds the font in that form and [`draw`] lays a character as five bytes.
//! On the host that draws the diagnostic screen's text some forty times
//! faster (`cargo bench -p oc-core -- text`).
//!
//! The pixels are the same as `embedded-graphics` draws, alignment, baseline
//! and the replacement glyph included: the tests compare the two for every
//! glyph, both colours, on and off the page grid and across the edges.

use embedded_graphics::pixelcolor::BinaryColor;
use embedded_graphics::prelude::Point;
pub use embedded_graphics::text::{Alignment, Baseline};

use crate::framebuffer::FrameBuffer;

/// Width of a glyph, and the advance from one character to the next: the font
/// has no spacing between characters, its glyphs carry their own.
pub const GLYPH_WIDTH: i32 = 5;

/// Height of a glyph, and of a line.
pub const GLYPH_HEIGHT: i32 = 8;

/// The first character the font draws; the table runs from here to `DEL`.
const FIRST: u32 = 0x20;

/// The glyph drawn for a character outside the font, as `embedded-graphics`
/// does: `?`.
const REPLACEMENT: usize = (b'?' - b' ') as usize;

/// How a text is placed and inked.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TextStyle {
    /// `On` lights the glyph pixels, `Off` darkens them (text over a lit
    /// area); the pixels between strokes are left as they are.
    pub color: BinaryColor,
    /// Which end of the text the position marks.
    pub alignment: Alignment,
    /// Which row of the glyphs the position marks.
    pub baseline: Baseline,
}

impl TextStyle {
    /// Lit text whose position is its top-left corner.
    pub const LIT: Self = Self {
        color: BinaryColor::On,
        alignment: Alignment::Left,
        baseline: Baseline::Top,
    };

    /// Dark text, for a lit background, otherwise as [`Self::LIT`].
    pub const DARK: Self = Self {
        color: BinaryColor::Off,
        ..Self::LIT
    };

    /// The same style with another alignment.
    #[must_use]
    pub const fn aligned(self, alignment: Alignment) -> Self {
        Self { alignment, ..self }
    }

    /// The same style with another baseline.
    #[must_use]
    pub const fn with_baseline(self, baseline: Baseline) -> Self {
        Self { baseline, ..self }
    }
}

/// Width of `line` in pixels.
#[must_use]
pub fn width(line: &str) -> i32 {
    i32::try_from(line.chars().count())
        .unwrap_or(i32::MAX)
        .saturating_mul(GLYPH_WIDTH)
}

/// Draws `text` at `position`, one line per `\n`, eight rows apart.
pub fn draw(frame: &mut FrameBuffer, text: &str, position: Point, style: TextStyle) {
    let on = style.color.is_on();
    let mut top = position.y - baseline_offset(style.baseline);
    for line in text.split('\n') {
        // Where `embedded-graphics` puts the left edge: the position marks
        // the last pixel for a right alignment, the middle one (rounded
        // left) when centred. It measures the line before dropping a `\r`
        // ending, so the same is done here.
        let last = width(line) - 1;
        let line = line.strip_suffix('\r').unwrap_or(line);
        let mut x = match style.alignment {
            Alignment::Left => position.x,
            Alignment::Center => position.x - last / 2,
            Alignment::Right => position.x - last,
        };
        for character in line.chars() {
            for &column in glyph(character) {
                frame.paint_column(x, top, column, on);
                x += 1;
            }
        }
        top += GLYPH_HEIGHT;
    }
}

/// Rows between the top of a glyph and the row a baseline names.
const fn baseline_offset(baseline: Baseline) -> i32 {
    match baseline {
        Baseline::Top => 0,
        Baseline::Bottom => GLYPH_HEIGHT - 1,
        Baseline::Middle => (GLYPH_HEIGHT - 1) / 2,
        // `FONT_5X8`'s own baseline.
        Baseline::Alphabetic => 6,
    }
}

/// The columns of the glyph that draws `character`.
fn glyph(character: char) -> &'static [u8; 5] {
    let index = u32::from(character)
        .checked_sub(FIRST)
        .and_then(|offset| usize::try_from(offset).ok())
        .filter(|&offset| offset < GLYPHS.len())
        .unwrap_or(REPLACEMENT);
    &GLYPHS[index]
}

/// `FONT_5X8`, from space to `DEL`, as five columns per glyph, the top row in
/// bit 0; extracted from `embedded-graphics` and checked against it by the
/// tests.
pub const GLYPHS: [[u8; 5]; 96] = [
    [0x00, 0x00, 0x00, 0x00, 0x00], // ' '
    [0x00, 0x00, 0x5E, 0x00, 0x00], // '!'
    [0x00, 0x0E, 0x00, 0x0E, 0x00], // '"'
    [0x14, 0x7F, 0x14, 0x7F, 0x14], // '#'
    [0x04, 0x2A, 0x7F, 0x2A, 0x10], // '$'
    [0x00, 0x16, 0x08, 0x34, 0x00], // '%'
    [0x36, 0x49, 0x36, 0x40, 0x00], // '&'
    [0x00, 0x00, 0x0E, 0x00, 0x00], // '\''
    [0x00, 0x3C, 0x42, 0x00, 0x00], // '('
    [0x00, 0x42, 0x3C, 0x00, 0x00], // ')'
    [0x54, 0x38, 0x38, 0x54, 0x00], // '*'
    [0x10, 0x10, 0x7C, 0x10, 0x10], // '+'
    [0x00, 0x80, 0x60, 0x20, 0x00], // ','
    [0x10, 0x10, 0x10, 0x10, 0x00], // '-'
    [0x00, 0x40, 0xE0, 0x40, 0x00], // '.'
    [0x60, 0x10, 0x08, 0x06, 0x00], // '/'
    [0x00, 0x3C, 0x42, 0x3C, 0x00], // '0'
    [0x00, 0x44, 0x7E, 0x40, 0x00], // '1'
    [0x64, 0x52, 0x52, 0x4C, 0x00], // '2'
    [0x22, 0x4A, 0x4E, 0x32, 0x00], // '3'
    [0x18, 0x14, 0x7E, 0x10, 0x00], // '4'
    [0x2E, 0x4A, 0x4A, 0x32, 0x00], // '5'
    [0x3C, 0x4A, 0x4A, 0x30, 0x00], // '6'
    [0x02, 0x62, 0x1A, 0x06, 0x00], // '7'
    [0x34, 0x4A, 0x4A, 0x34, 0x00], // '8'
    [0x0C, 0x52, 0x52, 0x3C, 0x00], // '9'
    [0x00, 0x6C, 0x6C, 0x00, 0x00], // ':'
    [0x00, 0x80, 0x6C, 0x2C, 0x00], // ';'
    [0x00, 0x18, 0x24, 0x42, 0x00], // '<'
    [0x28, 0x28, 0x28, 0x28, 0x00], // '='
    [0x00, 0x42, 0x24, 0x18, 0x00], // '>'
    [0x00, 0x04, 0x52, 0x0C, 0x00], // '?'
    [0x3C, 0x42, 0x99, 0xA5, 0x1E], // '@'
    [0x7C, 0x12, 0x12, 0x7C, 0x00], // 'A'
    [0x7E, 0x4A, 0x4A, 0x34, 0x00], // 'B'
    [0x3C, 0x42, 0x42, 0x24, 0x00], // 'C'
    [0x7E, 0x42, 0x42, 0x3C, 0x00], // 'D'
    [0x7E, 0x4A, 0x4A, 0x42, 0x00], // 'E'
    [0x7E, 0x0A, 0x0A, 0x02, 0x00], // 'F'
    [0x3C, 0x42, 0x52, 0x34, 0x00], // 'G'
    [0x7E, 0x08, 0x08, 0x7E, 0x00], // 'H'
    [0x00, 0x42, 0x7E, 0x42, 0x00], // 'I'
    [0x20, 0x42, 0x3E, 0x02, 0x00], // 'J'
    [0x7E, 0x08, 0x34, 0x42, 0x00], // 'K'
    [0x7E, 0x40, 0x40, 0x40, 0x00], // 'L'
    [0x7E, 0x0C, 0x0C, 0x7E, 0x00], // 'M'
    [0x7E, 0x0C, 0x38, 0x7E, 0x00], // 'N'
    [0x3C, 0x42, 0x42, 0x3C, 0x00], // 'O'
    [0x7E, 0x12, 0x12, 0x0C, 0x00], // 'P'
    [0x3C, 0x52, 0x62, 0xBC, 0x00], // 'Q'
    [0x7E, 0x12, 0x12, 0x6C, 0x00], // 'R'
    [0x24, 0x4A, 0x52, 0x24, 0x00], // 'S'
    [0x00, 0x02, 0x7E, 0x02, 0x00], // 'T'
    [0x3E, 0x40, 0x40, 0x3E, 0x00], // 'U'
    [0x1E, 0x60, 0x60, 0x1E, 0x00], // 'V'
    [0x7E, 0x30, 0x30, 0x7E, 0x00], // 'W'
    [0x66, 0x18, 0x18, 0x66, 0x00], // 'X'
    [0x06, 0x08, 0x70, 0x08, 0x06], // 'Y'
    [0x62, 0x52, 0x4A, 0x46, 0x00], // 'Z'
    [0x00, 0x7E, 0x42, 0x42, 0x00], // '['
    [0x06, 0x08, 0x10, 0x60, 0x00], // '\\'
    [0x00, 0x42, 0x42, 0x7E, 0x00], // ']'
    [0x00, 0x04, 0x02, 0x04, 0x00], // '^'
    [0x80, 0x80, 0x80, 0x80, 0x00], // '_'
    [0x00, 0x02, 0x04, 0x00, 0x00], // '`'
    [0x30, 0x48, 0x48, 0x78, 0x00], // 'a'
    [0x7E, 0x48, 0x48, 0x30, 0x00], // 'b'
    [0x00, 0x30, 0x48, 0x48, 0x00], // 'c'
    [0x30, 0x48, 0x48, 0x7E, 0x00], // 'd'
    [0x30, 0x68, 0x58, 0x10, 0x00], // 'e'
    [0x10, 0x7C, 0x12, 0x04, 0x00], // 'f'
    [0x10, 0xA8, 0xA8, 0x70, 0x00], // 'g'
    [0x7E, 0x08, 0x08, 0x70, 0x00], // 'h'
    [0x00, 0x48, 0x7A, 0x40, 0x00], // 'i'
    [0x00, 0x40, 0x80, 0x7A, 0x00], // 'j'
    [0x7E, 0x10, 0x10, 0x68, 0x00], // 'k'
    [0x00, 0x42, 0x7E, 0x40, 0x00], // 'l'
    [0x78, 0x08, 0x70, 0x08, 0x70], // 'm'
    [0x78, 0x08, 0x08, 0x70, 0x00], // 'n'
    [0x30, 0x48, 0x48, 0x30, 0x00], // 'o'
    [0xF8, 0x28, 0x28, 0x10, 0x00], // 'p'
    [0x10, 0x28, 0x28, 0xF8, 0x00], // 'q'
    [0x78, 0x10, 0x08, 0x10, 0x00], // 'r'
    [0x00, 0x50, 0x58, 0x28, 0x00], // 's'
    [0x08, 0x3E, 0x48, 0x20, 0x00], // 't'
    [0x38, 0x40, 0x40, 0x78, 0x00], // 'u'
    [0x00, 0x38, 0x40, 0x38, 0x00], // 'v'
    [0x38, 0x40, 0x30, 0x40, 0x38], // 'w'
    [0x48, 0x30, 0x30, 0x48, 0x00], // 'x'
    [0x58, 0xA0, 0xA0, 0x78, 0x00], // 'y'
    [0x48, 0x68, 0x58, 0x48, 0x00], // 'z'
    [0x08, 0x2A, 0x55, 0x41, 0x00], // '{'
    [0x00, 0x00, 0x7E, 0x00, 0x00], // '|'
    [0x41, 0x55, 0x2A, 0x08, 0x00], // '}'
    [0x04, 0x02, 0x04, 0x02, 0x00], // '~'
    [0x00, 0x04, 0x52, 0x0C, 0x00], // DEL (drawn as a question mark)
];

#[cfg(test)]
mod tests {
    use embedded_graphics::Drawable;
    use embedded_graphics::mono_font::MonoTextStyle;
    use embedded_graphics::mono_font::ascii::FONT_5X8;
    use embedded_graphics::prelude::Point;
    use embedded_graphics::text::{Alignment, Baseline, Text, TextStyleBuilder};

    use super::{GLYPHS, TextStyle, draw, width};
    use crate::framebuffer::FrameBuffer;

    /// A busy background, so that darkening and leaving pixels alone show.
    fn background() -> FrameBuffer {
        let mut frame = FrameBuffer::new();
        let mut seed = 0x5A_u8;
        for byte in frame.as_mut_bytes() {
            seed = seed.wrapping_mul(37).wrapping_add(11);
            *byte = seed;
        }
        frame
    }

    /// What `embedded-graphics` draws for the same call.
    fn reference(text: &str, position: Point, style: TextStyle) -> FrameBuffer {
        let mut frame = background();
        let character = MonoTextStyle::new(&FONT_5X8, style.color);
        let layout = TextStyleBuilder::new()
            .alignment(style.alignment)
            .baseline(style.baseline)
            .build();
        let _ = Text::with_text_style(text, position, character, layout).draw(&mut frame);
        frame
    }

    fn ours(text: &str, position: Point, style: TextStyle) -> FrameBuffer {
        let mut frame = background();
        draw(&mut frame, text, position, style);
        frame
    }

    fn assert_same(text: &str, position: Point, style: TextStyle) {
        assert!(
            ours(text, position, style) == reference(text, position, style),
            "{text:?} at {position:?} in {style:?} differs from embedded-graphics"
        );
    }

    #[test]
    fn every_glyph_matches_embedded_graphics_on_and_off_the_page_grid() {
        let mut buffer = [0; 4];
        for code in 0x20..=0x7F_u8 {
            let text = char::from(code).encode_utf8(&mut buffer);
            for style in [TextStyle::LIT, TextStyle::DARK] {
                for position in [
                    Point::new(0, 0),
                    Point::new(61, 27),
                    Point::new(-2, 59),
                    Point::new(125, -3),
                ] {
                    assert_same(text, position, style);
                }
            }
        }
    }

    #[test]
    fn alignment_and_baseline_place_text_as_embedded_graphics_does() {
        let alignments = [Alignment::Left, Alignment::Center, Alignment::Right];
        let baselines = [
            Baseline::Top,
            Baseline::Middle,
            Baseline::Bottom,
            Baseline::Alphabetic,
        ];
        for text in ["", "A", "SAFE MODE", "OC-RUST 0.3.0", "two\nlines\r\n"] {
            for alignment in alignments {
                for baseline in baselines {
                    for position in [Point::new(64, 32), Point::new(127, 0), Point::new(3, 63)] {
                        let style = TextStyle::LIT.aligned(alignment).with_baseline(baseline);
                        assert_same(text, position, style);
                    }
                }
            }
        }
    }

    #[test]
    fn a_character_outside_the_font_draws_the_replacement_glyph() {
        for text in ["\u{e9}", "\u{1f600}", "\t", "a\u{0}b"] {
            assert_same(text, Point::new(10, 8), TextStyle::LIT);
        }
    }

    #[test]
    fn widths_count_characters_not_bytes() {
        assert_eq!(width(""), 0);
        assert_eq!(width("SETUP"), 25);
        assert_eq!(width("\u{e9}t\u{e9}"), 15);
        assert_eq!(GLYPHS.len(), 0x80 - 0x20);
    }
}
