//! The boot splash screen shown once when the module starts, and again on
//! every reset ("Initialize" in VCV Rack terms).
//!
//! The animation is deliberately simple: the module's name and version,
//! centred on the screen, and a one-pixel-wide border that traces itself
//! clockwise around the screen edge. Normal execution only begins once the
//! border has made its full trip, which is [`Engine::tick`](crate::Engine)'s
//! job to enforce; this module only knows how to advance and draw one frame
//! of the animation.

// Screen dimensions and the elapsed-time counter are all small, non-negative
// values well inside `u32`'s range by construction (the animation cannot run
// longer than `DURATION_MICROS`, and the screen is 128x64), so the casts
// below are proven safe rather than accidental narrowing.
#![allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]

use crate::gfx::Point;

use crate::framebuffer::{FrameBuffer, HEIGHT_I32, WIDTH_I32};
use crate::text::fonts::FONT_5X8;
use crate::text::{self, Alignment, Baseline, TextStyle};

/// Duration of the boot animation, in microseconds.
///
/// One and a half seconds is long enough to read the banner and watch the
/// border trace itself, short enough not to feel like a delay every time the
/// module starts or is reset.
pub const DURATION_MICROS: u32 = 1_500_000;

/// Vertical distance between the banner and the build identifier, in pixels:
/// one 8-pixel text line plus a 2-pixel gap.
const BUILD_ID_GAP: i32 = 10;

/// The boot gestures, recalled at the bottom of the screen as the reference
/// firmware does (`[L] => Calibrate`): hold the left encoder for the
/// calibration, `up` + `down` for the safe-mode settings (journal/D-026).
pub const GESTURE_HINT: &str = "L:CALIB  UP+DN:SAFE";

/// Baseline row of the gesture hint: just inside the bottom border.
const GESTURE_HINT_Y: i32 = HEIGHT_I32 - 11;

/// Number of pixels in the one-pixel-wide border traced around the screen:
/// the perimeter of the `WIDTH_I32 x HEIGHT_I32` rectangle.
const PERIMETER_PIXELS: u32 = (2 * WIDTH_I32 + 2 * HEIGHT_I32 - 4) as u32;

/// The module's name/version banner and a border that traces itself
/// clockwise around the screen edge, shown once on start-up and on every
/// reset.
#[derive(Debug, Clone, Copy, Default)]
pub struct SplashScreen {
    elapsed_micros: u32,
}

impl SplashScreen {
    /// A freshly started animation, at its very first frame.
    #[must_use]
    pub const fn new() -> Self {
        Self { elapsed_micros: 0 }
    }

    /// Restarts the animation from its first frame.
    pub const fn reset(&mut self) {
        self.elapsed_micros = 0;
    }

    /// Jumps straight to the last frame, as if the animation had already run
    /// its course.
    pub const fn finish(&mut self) {
        self.elapsed_micros = DURATION_MICROS;
    }

    /// Advances the animation by `elapsed_micros`.
    pub const fn advance(&mut self, elapsed_micros: u32) {
        self.elapsed_micros = self.elapsed_micros.saturating_add(elapsed_micros);
    }

    /// Whether the border has fully traced around the screen.
    #[must_use]
    pub const fn is_done(&self) -> bool {
        self.elapsed_micros >= DURATION_MICROS
    }

    /// Draws the current animation frame: the banner centred on screen,
    /// `build_id` under it (short commit hash, `-dirty` if the tree
    /// differed), and however much of the border has traced so far. An empty
    /// identifier draws nothing: the backends compute it (journal/D-025), the
    /// core only shows it, so its tests never depend on the commit being
    /// built.
    pub fn render(&self, frame: &mut FrameBuffer, build_id: &str) {
        frame.clear();

        let centred = TextStyle::lit(&FONT_5X8)
            .aligned(Alignment::Center)
            .with_baseline(Baseline::Middle);
        // With a build identifier, the two lines straddle the centre; without
        // one, the banner sits on it as before.
        let banner_y = if build_id.is_empty() {
            HEIGHT_I32 / 2
        } else {
            HEIGHT_I32 / 2 - BUILD_ID_GAP / 2
        };
        text::draw(
            frame,
            crate::BANNER,
            Point::new(WIDTH_I32 / 2, banner_y),
            centred,
        );
        if !build_id.is_empty() {
            text::draw(
                frame,
                build_id,
                Point::new(WIDTH_I32 / 2, banner_y + BUILD_ID_GAP),
                centred,
            );
        }

        text::draw(
            frame,
            GESTURE_HINT,
            Point::new(WIDTH_I32 / 2, GESTURE_HINT_Y),
            TextStyle::lit(&FONT_5X8).aligned(Alignment::Center),
        );

        draw_border(frame, self.lit_border_pixels());
    }

    /// Number of border pixels lit at the current elapsed time.
    fn lit_border_pixels(self) -> u32 {
        let elapsed = self.elapsed_micros.min(DURATION_MICROS);
        (u64::from(elapsed) * u64::from(PERIMETER_PIXELS) / u64::from(DURATION_MICROS)) as u32
    }
}

/// Lights the first `count` pixels of the one-pixel border, tracing it
/// clockwise from the top-left corner: across the top, down the right side,
/// back across the bottom, then up the left side.
fn draw_border(frame: &mut FrameBuffer, count: u32) {
    let mut remaining = count;

    for x in 0..WIDTH_I32 {
        if remaining == 0 {
            return;
        }
        frame.set_pixel(x, 0, true);
        remaining -= 1;
    }
    for y in 1..HEIGHT_I32 {
        if remaining == 0 {
            return;
        }
        frame.set_pixel(WIDTH_I32 - 1, y, true);
        remaining -= 1;
    }
    for x in (0..WIDTH_I32 - 1).rev() {
        if remaining == 0 {
            return;
        }
        frame.set_pixel(x, HEIGHT_I32 - 1, true);
        remaining -= 1;
    }
    for y in (1..HEIGHT_I32 - 1).rev() {
        if remaining == 0 {
            return;
        }
        frame.set_pixel(0, y, true);
        remaining -= 1;
    }
}

#[cfg(test)]
mod tests {
    use super::{DURATION_MICROS, PERIMETER_PIXELS, SplashScreen};
    use crate::framebuffer::FrameBuffer;

    #[test]
    fn a_fresh_animation_is_not_done() {
        let splash = SplashScreen::new();
        assert!(!splash.is_done());
        assert_eq!(splash.lit_border_pixels(), 0);
    }

    #[test]
    fn the_animation_completes_exactly_at_its_duration() {
        let mut splash = SplashScreen::new();
        splash.advance(DURATION_MICROS - 1);
        assert!(!splash.is_done());

        splash.advance(1);
        assert!(splash.is_done());
        assert_eq!(splash.lit_border_pixels(), PERIMETER_PIXELS);
    }

    #[test]
    fn overshooting_the_duration_does_not_overshoot_the_border() {
        let mut splash = SplashScreen::new();
        splash.advance(DURATION_MICROS * 4);
        assert!(splash.is_done());
        assert_eq!(splash.lit_border_pixels(), PERIMETER_PIXELS);
    }

    #[test]
    fn the_border_grows_monotonically_with_elapsed_time() {
        let mut splash = SplashScreen::new();
        let mut previous = 0;
        for _ in 0..10 {
            splash.advance(DURATION_MICROS / 10);
            let lit = splash.lit_border_pixels();
            assert!(lit >= previous, "the border must never shrink");
            previous = lit;
        }
    }

    #[test]
    fn finish_jumps_straight_to_the_last_frame() {
        let mut splash = SplashScreen::new();
        splash.finish();
        assert!(splash.is_done());
        assert_eq!(splash.lit_border_pixels(), PERIMETER_PIXELS);
    }

    #[test]
    fn reset_returns_to_the_first_frame() {
        let mut splash = SplashScreen::new();
        splash.finish();
        splash.reset();
        assert!(!splash.is_done());
        assert_eq!(splash.lit_border_pixels(), 0);
    }

    #[test]
    fn rendering_draws_the_banner_and_the_border() {
        let mut frame = FrameBuffer::new();
        let mut splash = SplashScreen::new();
        splash.render(&mut frame, "");
        let banner_only = frame.lit_pixels();
        assert!(banner_only > 0, "the banner must be visible immediately");

        splash.finish();
        splash.render(&mut frame, "");
        assert!(
            frame.lit_pixels() > banner_only,
            "a fully traced border must light more pixels than the banner alone"
        );
    }

    #[test]
    fn rendering_is_deterministic() {
        let render = |elapsed| {
            let mut splash = SplashScreen::new();
            splash.advance(elapsed);
            let mut frame = FrameBuffer::new();
            splash.render(&mut frame, "");
            frame
        };
        assert_eq!(
            render(DURATION_MICROS / 3),
            render(DURATION_MICROS / 3),
            "the same elapsed time must always draw the same frame"
        );
    }

    #[test]
    fn the_build_id_adds_a_line() {
        let mut without = FrameBuffer::new();
        let mut with = FrameBuffer::new();
        let splash = SplashScreen::new();
        splash.render(&mut without, "");
        splash.render(&mut with, "a1b2c3d-dirty");
        assert_ne!(without, with, "the identifier must be drawn");
    }
}
