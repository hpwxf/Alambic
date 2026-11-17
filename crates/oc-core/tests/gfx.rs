//! The controls, plots and patterns of `oc_core::gfx` (journal/D-030): each
//! stays inside the box it is given and puts its telling pixels where they
//! belong. Shapes are checked by property, not against a picture, so that a
//! deliberate change of style does not break them.

use embedded_graphics::prelude::Point;
use oc_core::framebuffer::{FrameBuffer, HEIGHT_I32, WIDTH_I32};
use oc_core::gfx::controls::{bipolar_hbar, hbar, header, knob, selector, value_box, vbar};
use oc_core::gfx::patterns::{dots, euclid_ring, keyboard, steps};
use oc_core::gfx::plots::{Area, histogram, min_max, needle_scale, trace};
use oc_core::gfx::trig::polar;
use oc_core::gfx::{Ink, Ratio, polyline};
use oc_core::text::TextStyle;
use oc_core::text::fonts::{FONT_4X6, FONT_5X8};

/// The smallest box holding every lit pixel, as (left, top, right, bottom).
fn lit_box(frame: &FrameBuffer) -> Option<(i32, i32, i32, i32)> {
    let mut found: Option<(i32, i32, i32, i32)> = None;
    for y in 0..HEIGHT_I32 {
        for x in 0..WIDTH_I32 {
            if frame.pixel(x, y) {
                found = Some(match found {
                    None => (x, y, x, y),
                    Some((l, t, r, b)) => (l.min(x), t.min(y), r.max(x), b.max(y)),
                });
            }
        }
    }
    found
}

fn lit_in_column(frame: &FrameBuffer, x: i32) -> u32 {
    (0..HEIGHT_I32).map(|y| u32::from(frame.pixel(x, y))).sum()
}

#[test]
fn ratios_clamp_and_round_down() {
    assert_eq!(Ratio::new(5, 10).of(100), 50);
    assert_eq!(Ratio::new(-3, 10).of(100), 0);
    assert_eq!(Ratio::new(30, 10).of(100), 100);
    assert_eq!(Ratio::new(1, 0).of(100), 0);
}

#[test]
fn a_knob_points_from_seven_thirty_to_four_thirty() {
    let center = Point::new(20, 20);
    for (value, angle) in [(0, 160), (500, 64), (1_000, -32)] {
        let mut frame = FrameBuffer::new();
        knob(&mut frame, center, 6, Ratio::new(value, 1_000), Ink::Lit);
        assert_eq!(lit_box(&frame), Some((14, 14, 26, 26)), "inside its ring");
        let tip = polar(center, 5, angle);
        assert!(frame.pixel(tip.x, tip.y), "value {value}: tip at {tip:?}");
        assert!(frame.pixel(center.x, center.y));
    }
}

#[test]
fn a_selector_puts_its_arrows_before_the_value() {
    let mut frame = FrameBuffer::new();
    let end = selector(
        &mut frame,
        Point::new(10, 8),
        "SIN",
        TextStyle::lit(&FONT_5X8),
    );
    assert_eq!(end, 15 + 15);
    let (left, top, _, bottom) = lit_box(&frame).unwrap();
    assert_eq!(left, 10);
    assert!(top >= 8 && bottom < 16);
    // Up arrow tip, down arrow tip.
    assert!(frame.pixel(11, 9) && frame.pixel(11, 13));
}

#[test]
fn a_value_box_is_lit_around_dark_text() {
    let mut frame = FrameBuffer::new();
    let width = value_box(
        &mut frame,
        Point::new(30, 10),
        "064",
        TextStyle::lit(&FONT_4X6),
    );
    assert_eq!(width, 12 + 2);
    assert_eq!(lit_box(&frame), Some((30, 10, 43, 17)));
    // The border is lit, some pixels inside are dark: the digits.
    assert!(frame.pixel(30, 10) && frame.pixel(43, 17));
    let inside: u32 = (31..43)
        .flat_map(|x| (11..17).map(move |y| (x, y)))
        .map(|(x, y)| u32::from(!frame.pixel(x, y)))
        .sum();
    assert!(inside > 5);
}

#[test]
fn bars_fill_in_proportion() {
    let mut frame = FrameBuffer::new();
    hbar(&mut frame, 0, 0, 52, 6, Ratio::new(1, 2));
    // Inner row: 25 of 50 columns.
    assert_eq!((1..51).filter(|&x| frame.pixel(x, 3)).count(), 25);

    let mut frame = FrameBuffer::new();
    vbar(&mut frame, 0, 0, 6, 22, Ratio::new(1, 4));
    assert_eq!(
        lit_in_column(&frame, 3),
        2 + 5,
        "two edges and a fifth of twenty"
    );
    assert!(
        frame.pixel(3, 20) && !frame.pixel(3, 15),
        "filled from the bottom"
    );

    let mut frame = FrameBuffer::new();
    bipolar_hbar(&mut frame, 0, 10, 41, 5, Ratio::new(-10, 10));
    assert!(
        frame.pixel(5, 12) && !frame.pixel(30, 12),
        "negative fills left"
    );
}

#[test]
fn a_header_marks_the_current_page_and_ends_with_a_line() {
    let mut frame = FrameBuffer::new();
    let free = header(&mut frame, "GFX/KNOBS", 1, 4, TextStyle::lit(&FONT_5X8));
    assert_eq!(free, 11);
    assert_eq!((0..WIDTH_I32).filter(|&x| frame.pixel(x, 9)).count(), 128);
    // Four dots from the right edge, the second filled: its centre is lit,
    // the others' are not.
    let centres: Vec<bool> = [107, 113, 119, 125]
        .iter()
        .map(|&x| frame.pixel(x, 4))
        .collect();
    assert_eq!(centres, [false, true, false, false]);
}

#[test]
fn an_area_maps_its_range_onto_its_rows() {
    let area = Area {
        x: 0,
        y: 10,
        width: 50,
        height: 21,
        range: (-100, 100),
    };
    assert_eq!(area.row(-100), 30);
    assert_eq!(area.row(100), 10);
    assert_eq!(area.row(0), 20);
    assert_eq!(area.row(1_000), 10, "clamped");
}

#[test]
fn a_trace_has_a_pixel_in_every_column_and_stays_in_its_area() {
    let area = Area {
        x: 4,
        y: 20,
        width: 60,
        height: 30,
        range: (-50, 50),
    };
    let samples: Vec<i32> = (0..60).map(|i| ((i * 37) % 101) - 50).collect();
    let mut frame = FrameBuffer::new();
    trace(&mut frame, area, &samples);
    for x in 4..64 {
        assert!(lit_in_column(&frame, x) > 0, "column {x}");
    }
    let (left, top, right, bottom) = lit_box(&frame).unwrap();
    assert!(left >= 4 && right < 64 && top >= 20 && bottom < 50);
}

#[test]
fn a_polyline_inks_shared_points_once() {
    let points = [
        Point::new(0, 0),
        Point::new(10, 5),
        Point::new(20, 0),
        Point::new(30, 9),
    ];
    let mut frame = FrameBuffer::new();
    polyline(&mut frame, points, Ink::Lit);
    polyline(&mut frame, points, Ink::Invert);
    assert_eq!(frame.lit_pixels(), 0);
}

#[test]
fn min_max_columns_span_their_values() {
    let area = Area {
        x: 0,
        y: 0,
        width: 3,
        height: 11,
        range: (0, 10),
    };
    let mut frame = FrameBuffer::new();
    min_max(&mut frame, area, &[(0, 10), (4, 6), (7, 3)]);
    assert_eq!(
        [
            lit_in_column(&frame, 0),
            lit_in_column(&frame, 1),
            lit_in_column(&frame, 2)
        ],
        [11, 3, 5]
    );
}

#[test]
fn a_histogram_stops_at_the_area_edge() {
    let area = Area {
        x: 0,
        y: 0,
        width: 10,
        height: 10,
        range: (0, 9),
    };
    let mut frame = FrameBuffer::new();
    histogram(&mut frame, area, &[9, 4, 0, 9], 2, 1);
    assert_eq!(lit_in_column(&frame, 0), 10);
    assert_eq!(lit_in_column(&frame, 3), 5);
    assert_eq!(lit_in_column(&frame, 6), 1);
    assert_eq!(lit_in_column(&frame, 9), 0, "the fourth bar does not fit");
}

#[test]
fn a_needle_stands_where_its_value_is() {
    let mut frame = FrameBuffer::new();
    needle_scale(&mut frame, 0, 40, 101, 12, 11, Ratio::new(75, 100));
    assert!(frame.pixel(75, 40) && frame.pixel(75, 50));
    assert!(frame.pixel(50, 46), "the middle tick is taller");
    assert_eq!(lit_box(&frame).map(|b| (b.1, b.3)), Some((40, 51)));
}

#[test]
fn steps_and_dots_show_set_and_unset_alike() {
    let mut frame = FrameBuffer::new();
    steps(
        &mut frame,
        Point::new(2, 2),
        6,
        3,
        &[true, false, true],
        Some(1),
    );
    assert!(frame.pixel(4, 4), "a set step is filled");
    assert!(
        !frame.pixel(13, 4) && frame.pixel(11, 2),
        "an unset one is hollow"
    );
    assert!(frame.pixel(9, 0), "the current one is ringed");

    let mut frame = FrameBuffer::new();
    dots(&mut frame, Point::new(0, 0), 2, 1, &[true, false]);
    assert!(frame.pixel(2, 2) && !frame.pixel(8, 2));
}

#[test]
fn a_euclid_ring_starts_at_the_top_and_goes_clockwise() {
    let center = Point::new(64, 32);
    let mut frame = FrameBuffer::new();
    euclid_ring(
        &mut frame,
        center,
        20,
        2,
        &[true, true, false, false],
        Some(0),
    );
    assert!(frame.pixel(64, 12), "first step at the top, filled");
    assert!(frame.pixel(84, 32), "second at three o'clock, filled");
    assert!(
        !frame.pixel(64, 52) && frame.pixel(64, 50),
        "third at the bottom, hollow"
    );
    assert!(frame.pixel(64, 8), "the current step is ringed");
}

#[test]
fn a_keyboard_inverts_its_note() {
    let mut plain = FrameBuffer::new();
    let width = keyboard(&mut plain, Point::new(0, 0), 10, None);
    assert_eq!(width, 29);
    assert_eq!(lit_box(&plain), Some((0, 0, 28, 9)));
    let mut with_e = FrameBuffer::new();
    keyboard(&mut with_e, Point::new(0, 0), 10, Some(4));
    // E is the third white key: its lower half lights up.
    assert!(!plain.pixel(10, 8) && with_e.pixel(10, 8));
}
