//! Text laid straight into the framebuffer's page layout, in any of the
//! monospace fonts of [`fonts`] (journal/D-009).
//!
//! `embedded-graphics` stores a font row by row, one bit per pixel, and sets
//! every glyph pixel on its own when it draws: about forty steps for one 5x8
//! character. The framebuffer is stored column by column, eight rows to a
//! byte, so a [`ColumnFont`] holds each glyph as columns of
//! `ceil(height / 8)` bytes and [`draw`] lays a column a byte at a time. On the
//! module that cut the diagnostic screen's drawing from about 646 to 88 us
//! (indicative, one module).
//!
//! Nothing here ties an applet to a font, a size or a grid: the font is part
//! of the [`TextStyle`], the position is any pixel, on or off the page grid (a
//! column off the grid is laid as two shifted halves). The pixels are the
//! ones `embedded-graphics` draws, alignment, baseline, line breaks and the
//! replacement glyph included; `tests/fonts.rs` checks that for every font,
//! glyph and placement. `embedded-graphics` is only a test dependency: the
//! reference these fonts and rules are checked against.

pub mod fonts;

use crate::framebuffer::FrameBuffer;
use crate::gfx::{Ink, Point};

/// Which end of a line its position marks, as in `embedded-graphics`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Alignment {
    /// The first column.
    Left,
    /// The middle column, rounded left.
    Center,
    /// The last column.
    Right,
}

/// Which row of the glyphs a position marks, as in `embedded-graphics`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Baseline {
    /// The top row.
    Top,
    /// The bottom row.
    Bottom,
    /// The middle row, rounded up.
    Middle,
    /// The row letters stand on.
    Alphabetic,
}

/// A monospace font as glyph columns: glyph `g`, column `c`, band `b` (rows
/// `8b` to `8b + 7`, the top one in bit 0) is byte
/// `(g * width + c) * bands + b` of the table.
///
/// The tables in [`fonts`] are extracted from `embedded-graphics` fonts; the
/// glyphs cover one contiguous range of characters, as its ASCII fonts do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ColumnFont {
    width: i32,
    height: i32,
    spacing: i32,
    baseline: i32,
    /// Bytes per column.
    bands: usize,
    /// Bytes per glyph.
    glyph_bytes: usize,
    glyphs: usize,
    first: u32,
    replacement: usize,
    columns: &'static [u8],
}

impl ColumnFont {
    /// A font of `size` = (width, height) glyphs, `spacing` columns apart,
    /// whose alphabetic baseline is row `baseline`; `range` = (first,
    /// replacement): `columns` holds the glyphs from `first` on, and
    /// `replacement` is drawn for any character outside them.
    ///
    /// # Panics
    ///
    /// If `columns` does not hold whole glyphs or `replacement` is outside
    /// them; at compile time for the constants of [`fonts`].
    // `From` is not callable in a `const fn`; every cast widens a `u8`, or a
    // character code below the table's few hundred bytes.
    #[allow(clippy::cast_lossless, clippy::cast_possible_truncation)]
    #[must_use]
    pub const fn new(
        size: (u8, u8),
        spacing: u8,
        baseline: u8,
        range: (char, char),
        columns: &'static [u8],
    ) -> Self {
        assert!(size.0 > 0 && size.1 > 0, "a glyph has pixels");
        let bands = (size.1 as usize).div_ceil(8);
        let glyph_bytes = size.0 as usize * bands;
        assert!(
            columns.len().is_multiple_of(glyph_bytes),
            "whole glyphs only"
        );
        let glyphs = columns.len() / glyph_bytes;
        let first = range.0 as u32;
        let replacement = (range.1 as u32).wrapping_sub(first) as usize;
        assert!(
            replacement < glyphs,
            "the replacement glyph is in the table"
        );
        Self {
            width: size.0 as i32,
            height: size.1 as i32,
            spacing: spacing as i32,
            baseline: baseline as i32,
            bands,
            glyph_bytes,
            glyphs,
            first,
            replacement,
            columns,
        }
    }

    /// Glyph width in pixels.
    #[must_use]
    pub const fn width(&self) -> i32 {
        self.width
    }

    /// Glyph height in pixels, and the distance between two lines.
    #[must_use]
    pub const fn height(&self) -> i32 {
        self.height
    }

    /// Distance from one glyph to the next, in pixels: usable in a `const`,
    /// where a layout checks at compile time that its labels fit.
    #[must_use]
    pub const fn advance(&self) -> i32 {
        self.width + self.spacing
    }

    /// Width of `line` in pixels, as `embedded-graphics` measures it.
    #[must_use]
    pub fn width_of(&self, line: &str) -> i32 {
        let count = i32::try_from(line.chars().count()).unwrap_or(i32::MAX);
        count
            .saturating_mul(self.advance())
            .saturating_sub(self.spacing)
            .max(0)
    }

    /// The columns of the glyph that draws `character`.
    fn glyph(&self, character: char) -> &'static [u8] {
        let index = u32::from(character)
            .checked_sub(self.first)
            .and_then(|offset| usize::try_from(offset).ok())
            .filter(|&offset| offset < self.glyphs)
            .unwrap_or(self.replacement);
        let start = index * self.glyph_bytes;
        &self.columns[start..start + self.glyph_bytes]
    }

    /// Rows between the top of a glyph and the row a baseline names.
    const fn baseline_offset(&self, baseline: Baseline) -> i32 {
        match baseline {
            Baseline::Top => 0,
            Baseline::Bottom => self.height - 1,
            Baseline::Middle => (self.height - 1) / 2,
            Baseline::Alphabetic => self.baseline,
        }
    }
}

/// How a text is drawn: font, ink, and what its position marks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TextStyle {
    /// The font.
    pub font: &'static ColumnFont,
    /// What the glyph pixels get: lit, darkened (text over a lit area) or
    /// inverted; the pixels between strokes are left as they are.
    pub color: Ink,
    /// Which end of the text the position marks.
    pub alignment: Alignment,
    /// Which row of the glyphs the position marks.
    pub baseline: Baseline,
}

impl TextStyle {
    /// Lit text in `font`, whose position is its top-left corner.
    #[must_use]
    pub const fn lit(font: &'static ColumnFont) -> Self {
        Self {
            font,
            color: Ink::Lit,
            alignment: Alignment::Left,
            baseline: Baseline::Top,
        }
    }

    /// Dark text in `font`, for a lit background, otherwise as [`Self::lit`].
    #[must_use]
    pub const fn dark(font: &'static ColumnFont) -> Self {
        Self {
            color: Ink::Dark,
            ..Self::lit(font)
        }
    }

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

/// Draws `text` at `position`, one line per `\n`, a font height apart.
pub fn draw(frame: &mut FrameBuffer, text: &str, position: Point, style: TextStyle) {
    let font = style.font;
    let bands = font.bands;
    let ink = style.color;
    let mut top = position.y - font.baseline_offset(style.baseline);
    for line in text.split('\n') {
        // Where `embedded-graphics` puts the left edge: the position marks
        // the last pixel for a right alignment, the middle one (rounded
        // left) when centred. It measures the line before dropping a `\r`
        // ending, so the same is done here.
        let last = font.width_of(line) - 1;
        let line = line.strip_suffix('\r').unwrap_or(line);
        let mut x = match style.alignment {
            Alignment::Left => position.x,
            Alignment::Center => position.x - last / 2,
            Alignment::Right => position.x - last,
        };
        for character in line.chars() {
            let glyph = font.glyph(character);
            if bands == 1 {
                // Fonts up to eight rows: one byte per column.
                for &bits in glyph {
                    frame.paint_column(x, top, bits, ink);
                    x += 1;
                }
            } else {
                for column in glyph.chunks_exact(bands) {
                    let mut band_top = top;
                    for &bits in column {
                        frame.paint_column(x, band_top, bits, ink);
                        band_top += 8;
                    }
                    x += 1;
                }
            }
            x += font.spacing;
        }
        top += font.height;
    }
}

#[cfg(test)]
mod tests {
    use super::fonts::{FONT_5X8, FONT_10X20};

    #[test]
    fn widths_count_characters_not_bytes() {
        assert_eq!(FONT_5X8.width_of(""), 0);
        assert_eq!(FONT_5X8.width_of("SETUP"), 25);
        assert_eq!(FONT_5X8.width_of("\u{e9}t\u{e9}"), 15);
        assert_eq!(FONT_10X20.width_of("ab"), 20);
    }

    #[test]
    fn a_tall_font_takes_several_bytes_per_column() {
        assert_eq!(FONT_5X8.bands, 1);
        assert_eq!(FONT_10X20.bands, 3);
        assert_eq!(FONT_10X20.glyph('A').len(), 30);
    }
}
