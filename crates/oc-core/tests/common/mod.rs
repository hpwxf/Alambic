//! Shared by the tests and the benchmarks: `embedded-graphics` drawing into an
//! `oc_core` framebuffer, the reference the text and shapes are checked
//! against. `oc-core` itself does not depend on `embedded-graphics`
//! (journal/D-030).

#![allow(dead_code, unreachable_pub)]

use std::convert::Infallible;

use embedded_graphics::Pixel;
use embedded_graphics::draw_target::DrawTarget;
use embedded_graphics::geometry::{OriginDimensions, Size};
use embedded_graphics::pixelcolor::BinaryColor;
use oc_core::framebuffer::{FrameBuffer, HEIGHT_I32, WIDTH_I32};
use oc_core::gfx::Ink;
use oc_core::text;

/// A framebuffer `embedded-graphics` can draw into.
#[derive(Debug)]
pub struct Reference<'frame>(pub &'frame mut FrameBuffer);

impl OriginDimensions for Reference<'_> {
    fn size(&self) -> Size {
        Size::new(WIDTH_I32.unsigned_abs(), HEIGHT_I32.unsigned_abs())
    }
}

impl DrawTarget for Reference<'_> {
    type Color = BinaryColor;
    type Error = Infallible;

    fn draw_iter<I>(&mut self, pixels: I) -> Result<(), Self::Error>
    where
        I: IntoIterator<Item = Pixel<Self::Color>>,
    {
        for Pixel(point, color) in pixels {
            self.0.set_pixel(point.x, point.y, color.is_on());
        }
        Ok(())
    }
}

/// The `embedded-graphics` point for an `oc_core` one.
pub fn point(at: oc_core::gfx::Point) -> embedded_graphics::prelude::Point {
    embedded_graphics::prelude::Point::new(at.x, at.y)
}

/// The `embedded-graphics` colour for a lit or dark ink.
pub fn color(ink: Ink) -> BinaryColor {
    match ink {
        Ink::Lit => BinaryColor::On,
        Ink::Dark => BinaryColor::Off,
        Ink::Invert => panic!("embedded-graphics cannot invert"),
    }
}

/// The `embedded-graphics` alignment for an `oc_core` one.
pub fn alignment(alignment: text::Alignment) -> embedded_graphics::text::Alignment {
    match alignment {
        text::Alignment::Left => embedded_graphics::text::Alignment::Left,
        text::Alignment::Center => embedded_graphics::text::Alignment::Center,
        text::Alignment::Right => embedded_graphics::text::Alignment::Right,
    }
}

/// The `embedded-graphics` baseline for an `oc_core` one.
pub fn baseline(baseline: text::Baseline) -> embedded_graphics::text::Baseline {
    match baseline {
        text::Baseline::Top => embedded_graphics::text::Baseline::Top,
        text::Baseline::Bottom => embedded_graphics::text::Baseline::Bottom,
        text::Baseline::Middle => embedded_graphics::text::Baseline::Middle,
        text::Baseline::Alphabetic => embedded_graphics::text::Baseline::Alphabetic,
    }
}

/// A busy background, so that darkening, inverting and leaving pixels alone
/// all show.
pub fn background() -> FrameBuffer {
    let mut frame = FrameBuffer::new();
    let mut seed = 0x5A_u8;
    for byte in frame.as_mut_bytes() {
        seed = seed.wrapping_mul(37).wrapping_add(11);
        *byte = seed;
    }
    frame
}
