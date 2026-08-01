//! The module's 128x64 monochrome framebuffer.
//!
//! The byte layout is the SSD1306/SSD1309 native one: the screen is eight
//! horizontal *pages* of eight rows, and byte `page * WIDTH + x` holds the
//! eight pixels of column `x` within that page, the top-most pixel in bit 0.
//! Choosing the controller's own layout means the firmware pushes the buffer
//! to the panel with a single DMA transfer and no repacking, while the
//! simulator and the VCV Rack module read those exact same bytes.

// Pixel coordinates come from `embedded-graphics` as `i32` and are bounds
// checked against the screen before any cast, so the sign and width lints only
// fire on conversions that have already been proven safe.
#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::cast_sign_loss
)]

use core::convert::Infallible;

use embedded_graphics::Pixel;
use embedded_graphics::draw_target::DrawTarget;
use embedded_graphics::geometry::{OriginDimensions, Size};
use embedded_graphics::pixelcolor::BinaryColor;

/// Screen width in pixels.
pub const WIDTH: usize = 128;

/// Screen height in pixels.
pub const HEIGHT: usize = 64;

/// Number of eight-row pages.
pub const PAGES: usize = HEIGHT / 8;

/// Size of the framebuffer in bytes.
pub const LEN: usize = WIDTH * PAGES;

/// Screen width as `i32`, matching `embedded-graphics` coordinates.
pub const WIDTH_I32: i32 = WIDTH as i32;

/// Screen height as `i32`, matching `embedded-graphics` coordinates.
pub const HEIGHT_I32: i32 = HEIGHT as i32;

/// A 128x64 one-bit-per-pixel framebuffer.
#[derive(Clone, PartialEq, Eq)]
pub struct FrameBuffer {
    data: [u8; LEN],
}

impl FrameBuffer {
    /// An all-dark framebuffer.
    #[must_use]
    pub const fn new() -> Self {
        Self { data: [0; LEN] }
    }

    /// Turns every pixel off.
    pub fn clear(&mut self) {
        self.data = [0; LEN];
    }

    /// Turns every pixel on.
    pub fn fill(&mut self) {
        self.data = [0xFF; LEN];
    }

    /// Sets one pixel. Coordinates outside the screen are ignored.
    pub const fn set_pixel(&mut self, x: i32, y: i32, on: bool) {
        let Some(index) = Self::index_of(x, y) else {
            return;
        };
        let mask = 1u8 << (y as usize % 8);
        if on {
            self.data[index] |= mask;
        } else {
            self.data[index] &= !mask;
        }
    }

    /// Reads one pixel. Coordinates outside the screen read as off.
    #[must_use]
    pub const fn pixel(&self, x: i32, y: i32) -> bool {
        let Some(index) = Self::index_of(x, y) else {
            return false;
        };
        self.data[index] & (1u8 << (y as usize % 8)) != 0
    }

    /// The raw bytes, in controller order.
    #[must_use]
    pub const fn as_bytes(&self) -> &[u8; LEN] {
        &self.data
    }

    /// The raw bytes, for a driver that fills the buffer itself.
    pub const fn as_mut_bytes(&mut self) -> &mut [u8; LEN] {
        &mut self.data
    }

    /// Number of lit pixels; useful in tests and for a cheap content hash.
    #[must_use]
    pub fn lit_pixels(&self) -> u32 {
        self.data.iter().map(|byte| byte.count_ones()).sum()
    }

    /// Byte index of a pixel, or `None` when it falls outside the screen.
    const fn index_of(x: i32, y: i32) -> Option<usize> {
        if x < 0 || y < 0 || x >= WIDTH_I32 || y >= HEIGHT_I32 {
            return None;
        }
        Some((y as usize / 8) * WIDTH + x as usize)
    }
}

impl Default for FrameBuffer {
    fn default() -> Self {
        Self::new()
    }
}

impl core::fmt::Debug for FrameBuffer {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("FrameBuffer")
            .field("width", &WIDTH)
            .field("height", &HEIGHT)
            .field("lit_pixels", &self.lit_pixels())
            .finish()
    }
}

impl OriginDimensions for FrameBuffer {
    fn size(&self) -> Size {
        Size::new(WIDTH as u32, HEIGHT as u32)
    }
}

impl DrawTarget for FrameBuffer {
    type Color = BinaryColor;
    /// Drawing can never fail: out-of-bounds pixels are dropped.
    type Error = Infallible;

    fn draw_iter<I>(&mut self, pixels: I) -> Result<(), Self::Error>
    where
        I: IntoIterator<Item = Pixel<Self::Color>>,
    {
        for Pixel(point, color) in pixels {
            self.set_pixel(point.x, point.y, color.is_on());
        }
        Ok(())
    }

    fn clear(&mut self, color: Self::Color) -> Result<(), Self::Error> {
        if color.is_on() {
            FrameBuffer::fill(self);
        } else {
            FrameBuffer::clear(self);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{FrameBuffer, HEIGHT, HEIGHT_I32, LEN, WIDTH, WIDTH_I32};

    use embedded_graphics::mono_font::MonoTextStyle;
    use embedded_graphics::mono_font::ascii::FONT_6X10;
    use embedded_graphics::pixelcolor::BinaryColor;
    use embedded_graphics::prelude::*;
    use embedded_graphics::primitives::{PrimitiveStyle, Rectangle};
    use embedded_graphics::text::Text;

    #[test]
    fn a_new_buffer_is_dark() {
        let frame = FrameBuffer::new();
        assert_eq!(frame.lit_pixels(), 0);
        assert_eq!(frame.as_bytes().len(), LEN);
    }

    #[test]
    fn pixels_round_trip() {
        let mut frame = FrameBuffer::new();
        for (x, y) in [(0, 0), (127, 63), (64, 8), (1, 7)] {
            frame.set_pixel(x, y, true);
            assert!(frame.pixel(x, y), "pixel ({x}, {y}) should be on");
        }
        assert_eq!(frame.lit_pixels(), 4);

        frame.set_pixel(0, 0, false);
        assert!(!frame.pixel(0, 0));
        assert_eq!(frame.lit_pixels(), 3);
    }

    #[test]
    fn the_layout_matches_the_oled_controller() {
        let mut frame = FrameBuffer::new();
        // Pixel (5, 9) lives in page 1, column 5, bit 1.
        frame.set_pixel(5, 9, true);
        assert_eq!(frame.as_bytes()[WIDTH + 5], 0b0000_0010);
    }

    #[test]
    fn out_of_bounds_access_is_a_no_op() {
        let mut frame = FrameBuffer::new();
        for (x, y) in [(-1, 0), (0, -1), (WIDTH_I32, 0), (0, HEIGHT_I32)] {
            frame.set_pixel(x, y, true);
            assert!(!frame.pixel(x, y));
        }
        assert_eq!(frame.lit_pixels(), 0);
    }

    #[test]
    fn filling_and_clearing_touch_every_pixel() {
        let mut frame = FrameBuffer::new();
        frame.fill();
        assert_eq!(frame.lit_pixels() as usize, WIDTH * HEIGHT);
        frame.clear();
        assert_eq!(frame.lit_pixels(), 0);
    }

    #[test]
    fn embedded_graphics_primitives_draw_into_the_buffer() {
        let mut frame = FrameBuffer::new();
        Rectangle::new(Point::new(2, 2), Size::new(10, 4))
            .into_styled(PrimitiveStyle::with_fill(BinaryColor::On))
            .draw(&mut frame)
            .unwrap();
        assert_eq!(frame.lit_pixels(), 40);
        assert!(frame.pixel(2, 2));
        assert!(!frame.pixel(12, 2));
    }

    #[test]
    fn text_rendering_is_deterministic() {
        let render = || {
            let mut frame = FrameBuffer::new();
            let style = MonoTextStyle::new(&FONT_6X10, BinaryColor::On);
            Text::new("O&C", Point::new(0, 8), style)
                .draw(&mut frame)
                .unwrap();
            frame
        };
        let first = render();
        let second = render();
        assert_eq!(first.as_bytes(), second.as_bytes());
        assert!(first.lit_pixels() > 0);
    }

    #[test]
    fn clearing_through_the_draw_target_matches_the_inherent_method() {
        let mut a = FrameBuffer::new();
        a.fill();
        DrawTarget::clear(&mut a, BinaryColor::Off).unwrap();

        let b = FrameBuffer::new();
        assert_eq!(a, b);
    }
}
