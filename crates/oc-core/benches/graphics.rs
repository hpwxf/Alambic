//! Text and shape drawing, `oc_core::text` and `oc_core::gfx` against
//! `embedded-graphics`, the reference they light the same pixels as
//! (journal/D-009, D-030); and the pages of the graphics demonstration.
//!
//! Each comparison is a group of two, `embedded_graphics` and `oc_core`, drawing
//! the same thing into the same framebuffer, so the report reads side by side:
//!
//! ```sh
//! cargo bench -p oc-core --bench graphics
//! ```
//!
//! Host numbers: the ratio is what matters, the module's own costs come from
//! its profile (`cargo probe stats`).

use criterion::{Criterion, criterion_group, criterion_main};
use embedded_graphics::Drawable as _;
use embedded_graphics::mono_font::{MonoFont, MonoTextStyle, ascii};
use embedded_graphics::pixelcolor::BinaryColor;
use embedded_graphics::prelude::{Point as EgPoint, Primitive as _, Size};
use embedded_graphics::primitives::{Circle, Line, PrimitiveStyle, Rectangle};
use embedded_graphics::text::{Baseline, Text};
use std::hint::black_box;

#[path = "../tests/common/mod.rs"]
mod common;

use common::Reference;
use oc_core::apps::gfx_demo::GfxDemoApp;
use oc_core::apps::{InputSnapshot, TickContext};
use oc_core::framebuffer::FrameBuffer;
use oc_core::gfx::{self, Ink, Point};
use oc_core::platform::ControlEvents;
use oc_core::text::{self, ColumnFont, TextStyle, fonts};

/// A screenful of text, as the diagnostic applet draws it.
const SCREEN_TEXT: [&str; 8] = [
    "OC-RUST 0.3.0  987us",
    ">1 -2.500V P~H    3",
    " 2 +0.000V .-l    0",
    " 3 +1.234V P~H   12",
    " 4 +4.800V P-l    0",
    "MODE OFFSET OFS +0.000V",
    "OUT -2.5 +0.0 +1.2 +4.8",
    "TICKS 123456",
];

/// Benchmarks `name` as a group of two: `embedded_graphics` then `oc_core`,
/// each drawing into a cleared framebuffer.
fn compare(
    criterion: &mut Criterion,
    name: &str,
    mut reference: impl FnMut(&mut FrameBuffer),
    mut ours: impl FnMut(&mut FrameBuffer),
) {
    let mut group = criterion.benchmark_group(name);
    group.bench_function("embedded_graphics", |bencher| {
        let mut frame = FrameBuffer::new();
        bencher.iter(|| {
            frame.clear();
            reference(&mut frame);
            black_box(frame.as_bytes()[0])
        });
    });
    group.bench_function("oc_core", |bencher| {
        let mut frame = FrameBuffer::new();
        bencher.iter(|| {
            frame.clear();
            ours(&mut frame);
            black_box(frame.as_bytes()[0])
        });
    });
    group.finish();
}

/// Draws `lines` from `top`, one per font height, with `embedded-graphics`.
fn reference_lines(frame: &mut FrameBuffer, font: &MonoFont<'_>, lines: &[&str], top: i32) {
    let style = MonoTextStyle::new(font, BinaryColor::On);
    let height = i32::try_from(font.character_size.height).unwrap_or(8);
    for (row, line) in (0..).zip(lines) {
        let at = EgPoint::new(0, top + row * height);
        let _ = Text::with_baseline(black_box(line), at, style, Baseline::Top)
            .draw(&mut Reference(frame));
    }
}

/// The same with `oc_core::text`.
fn our_lines(frame: &mut FrameBuffer, font: &'static ColumnFont, lines: &[&str], top: i32) {
    for (row, line) in (0..).zip(lines) {
        let at = Point::new(0, top + row * font.height());
        text::draw(frame, black_box(line), at, TextStyle::lit(font));
    }
}

fn text_drawing(criterion: &mut Criterion) {
    // The diagnostic screen: eight lines on the page grid.
    compare(
        criterion,
        "text_screen_5x8",
        |frame| reference_lines(frame, &ascii::FONT_5X8, &SCREEN_TEXT, 0),
        |frame| our_lines(frame, &fonts::FONT_5X8, &SCREEN_TEXT, 0),
    );
    // The same three rows lower: every glyph column straddles two pages.
    compare(
        criterion,
        "text_off_grid_5x8",
        |frame| reference_lines(frame, &ascii::FONT_5X8, &SCREEN_TEXT[..7], 3),
        |frame| our_lines(frame, &fonts::FONT_5X8, &SCREEN_TEXT[..7], 3),
    );
    // A tall font: three bytes per glyph column.
    let big = ["A4 440HZ", "C#3 -12"];
    compare(
        criterion,
        "text_10x20",
        |frame| reference_lines(frame, &ascii::FONT_10X20, &big, 0),
        |frame| our_lines(frame, &fonts::FONT_10X20, &big, 0),
    );
}

fn shapes(criterion: &mut Criterion) {
    let lit = PrimitiveStyle::with_fill(BinaryColor::On);
    let stroke = PrimitiveStyle::with_stroke(BinaryColor::On, 1);

    compare(
        criterion,
        "fill_rect_100x40",
        |frame| {
            let area = Rectangle::new(EgPoint::new(black_box(3), 5), Size::new(100, 40));
            let _ = area.into_styled(lit).draw(&mut Reference(frame));
        },
        |frame| gfx::fill_rect(frame, black_box(3), 5, 100, 40, Ink::Lit),
    );
    compare(
        criterion,
        "rect_100x40",
        |frame| {
            let area = Rectangle::new(EgPoint::new(black_box(3), 5), Size::new(100, 40));
            let _ = area.into_styled(stroke).draw(&mut Reference(frame));
        },
        |frame| gfx::rect(frame, black_box(3), 5, 100, 40, Ink::Lit),
    );

    // A fan of lines from one corner, as a polyline's segments.
    let ends: Vec<(i32, i32)> = (0..16).map(|step| (127, step * 4)).collect();
    compare(
        criterion,
        "lines_fan",
        |frame| {
            for &(x, y) in &ends {
                let line = Line::new(EgPoint::new(0, 0), EgPoint::new(black_box(x), y));
                let _ = line.into_styled(stroke).draw(&mut Reference(frame));
            }
        },
        |frame| {
            for &(x, y) in &ends {
                gfx::line(
                    frame,
                    Point::new(0, 0),
                    Point::new(black_box(x), y),
                    Ink::Lit,
                );
            }
        },
    );

    // A knob's ring and a filled dot, radius 10.
    compare(
        criterion,
        "circle_r10",
        |frame| {
            let ring = Circle::with_center(EgPoint::new(black_box(64), 32), 21);
            let _ = ring.into_styled(stroke).draw(&mut Reference(frame));
        },
        |frame| gfx::circle(frame, Point::new(black_box(64), 32), 10, Ink::Lit),
    );
    compare(
        criterion,
        "fill_circle_r10",
        |frame| {
            let disc = Circle::with_center(EgPoint::new(black_box(64), 32), 21);
            let _ = disc.into_styled(lit).draw(&mut Reference(frame));
        },
        |frame| gfx::fill_circle(frame, Point::new(black_box(64), 32), 10, Ink::Lit),
    );
}

/// Each page of the graphics demonstration, as drawn on a redraw tick.
fn demo_pages(criterion: &mut Criterion) {
    let mut group = criterion.benchmark_group("gfx_demo");
    let mut app = GfxDemoApp::new();
    let next_page = InputSnapshot {
        controls: ControlEvents {
            encoder_delta: [1, 0],
            button_down: [false; 4],
        },
        elapsed_micros: 1_000,
        ..InputSnapshot::default()
    };
    for _ in 0..5 {
        let name = format!("{:?}", app.page()).to_lowercase();
        let page = app.clone();
        group.bench_function(name, |bencher| {
            let mut frame = FrameBuffer::new();
            bencher.iter(|| {
                page.render(&mut frame, &TickContext::default());
                black_box(frame.as_bytes()[0])
            });
        });
        app.update(&next_page);
    }
    group.finish();
}

criterion_group!(benches, text_drawing, shapes, demo_pages);
criterion_main!(benches);
