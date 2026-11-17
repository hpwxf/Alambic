//! The glyph-column fonts of `oc_core::text`, against the `embedded-graphics`
//! fonts they are extracted from (journal/D-009).
//!
//! `src/text/fonts.rs` is generated from those fonts by this test; after
//! adding a font to [`SOURCES`] or upgrading `embedded-graphics`, regenerate
//! it, then review the diff:
//!
//! ```sh
//! UPDATE_FONTS=1 cargo test -p oc-core --test fonts
//! ```

use std::fmt::Write as _;
use std::path::Path;

mod common;

use common::{Reference, background};
use embedded_graphics::Drawable as _;
use embedded_graphics::mono_font::{MonoFont, MonoTextStyle, ascii};
use embedded_graphics::pixelcolor::BinaryColor;
use embedded_graphics::text::{Text, TextStyleBuilder};
use oc_core::framebuffer::{FrameBuffer, WIDTH};
use oc_core::gfx::Point;
use oc_core::text::{Alignment, Baseline, ColumnFont, TextStyle, draw, fonts};

/// The fonts `src/text/fonts.rs` provides, by the name they keep there.
const SOURCES: [(&str, &MonoFont<'static>); 6] = [
    ("FONT_4X6", &ascii::FONT_4X6),
    ("FONT_5X8", &ascii::FONT_5X8),
    ("FONT_6X10", &ascii::FONT_6X10),
    ("FONT_7X13", &ascii::FONT_7X13),
    ("FONT_9X15", &ascii::FONT_9X15),
    ("FONT_10X20", &ascii::FONT_10X20),
];

/// The characters every ASCII font of `embedded-graphics` maps.
const FIRST: u8 = 0x20;
const LAST: u8 = 0x7F;

/// One glyph's columns, as `embedded-graphics` draws it at the origin.
fn glyph_columns(font: &MonoFont<'static>, code: u8) -> Vec<u8> {
    let mut frame = FrameBuffer::new();
    let style = MonoTextStyle::new(font, BinaryColor::On);
    let mut buffer = [0; 4];
    let text = char::from(code).encode_utf8(&mut buffer);
    let _ = Text::with_baseline(
        text,
        embedded_graphics::prelude::Point::zero(),
        style,
        embedded_graphics::text::Baseline::Top,
    )
    .draw(&mut Reference(&mut frame));
    let width = font.character_size.width as usize;
    let bands = (font.character_size.height as usize).div_ceil(8);
    let bytes = frame.as_bytes();
    let mut columns = Vec::new();
    for column in 0..width {
        for band in 0..bands {
            columns.push(bytes[band * WIDTH + column]);
        }
    }
    columns
}

/// How a character is named in the table's comments.
fn label(code: u8) -> String {
    match code {
        0x7F => "DEL".to_owned(),
        b'\'' | b'\\' => format!("'\\{}'", char::from(code)),
        _ => format!("'{}'", char::from(code)),
    }
}

/// The source of `src/text/fonts.rs`.
fn generate() -> String {
    let mut out = String::from(
        "//! The fonts [`super::draw`] draws, as glyph columns extracted from the\n\
         //! `embedded-graphics` fonts of the same name (journal/D-009).\n\
         //!\n\
         //! Generated: `UPDATE_FONTS=1 cargo test -p oc-core --test fonts` rewrites\n\
         //! this file from `tests/fonts.rs`; never edit it by hand. A font no code\n\
         //! uses adds nothing to the firmware image.\n\
         \n\
         use super::ColumnFont;\n",
    );
    for (name, font) in SOURCES {
        let size = font.character_size;
        let _ = write!(
            out,
            "\n/// `embedded-graphics`' `{name}`: {} x {} pixels.\n\
             #[rustfmt::skip]\n\
             pub const {name}: ColumnFont = ColumnFont::new(({}, {}), {}, {}, (' ', '?'), &[\n",
            size.width, size.height, size.width, size.height, font.character_spacing, font.baseline,
        );
        for code in FIRST..=LAST {
            let bytes: Vec<String> = glyph_columns(font, code)
                .iter()
                .map(|byte| format!("0x{byte:02X}"))
                .collect();
            let _ = writeln!(out, "    {}, // {}", bytes.join(", "), label(code));
        }
        out.push_str("]);\n");
    }
    let _ = write!(
        out,
        "\n/// Every font above, by name, smallest first.\n\
         pub const ALL: [(&str, &ColumnFont); {}] = [\n",
        SOURCES.len()
    );
    for (name, _) in SOURCES {
        let _ = writeln!(out, "    (\"{name}\", &{name}),");
    }
    out.push_str("];\n");
    out
}

#[test]
fn the_font_tables_are_generated_from_embedded_graphics() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/text/fonts.rs");
    let generated = generate();
    if std::env::var_os("UPDATE_FONTS").is_some() {
        std::fs::write(&path, &generated).expect("write src/text/fonts.rs");
        return;
    }
    let committed = std::fs::read_to_string(&path).expect("read src/text/fonts.rs");
    assert!(
        committed == generated,
        "src/text/fonts.rs is stale: run `UPDATE_FONTS=1 cargo test -p oc-core --test fonts`"
    );
}

/// The `embedded-graphics` font a table was extracted from.
fn source(name: &str) -> &'static MonoFont<'static> {
    SOURCES
        .iter()
        .find(|(source, _)| *source == name)
        .map_or_else(|| panic!("{name} has no source font"), |(_, font)| *font)
}

/// Draws `text` both ways and compares the frames.
fn assert_same(name: &str, font: &'static ColumnFont, text: &str, at: Point, style: TextStyle) {
    let style = TextStyle { font, ..style };
    let mut ours = background();
    draw(&mut ours, text, at, style);

    let mut reference = background();
    let character = MonoTextStyle::new(source(name), common::color(style.color));
    let layout = TextStyleBuilder::new()
        .alignment(common::alignment(style.alignment))
        .baseline(common::baseline(style.baseline))
        .build();
    let _ = Text::with_text_style(text, common::point(at), character, layout)
        .draw(&mut Reference(&mut reference));

    assert!(
        ours == reference,
        "{name}: {text:?} at {at:?} ({:?}, {:?}, {:?}) differs from embedded-graphics",
        style.color,
        style.alignment,
        style.baseline
    );
}

#[test]
fn every_glyph_of_every_font_matches_on_and_off_the_page_grid() {
    let mut buffer = [0; 4];
    for (name, font) in fonts::ALL {
        for code in FIRST..=LAST {
            let text = char::from(code).encode_utf8(&mut buffer);
            for style in [TextStyle::lit(font), TextStyle::dark(font)] {
                for at in [
                    Point::new(0, 0),
                    Point::new(61, 27),
                    Point::new(-2, 52),
                    Point::new(122, -3),
                ] {
                    assert_same(name, font, text, at, style);
                }
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
    for (name, font) in fonts::ALL {
        for text in ["", "A", "SAFE MODE", "two\nlines\r\n"] {
            for alignment in alignments {
                for baseline in baselines {
                    for at in [Point::new(64, 32), Point::new(127, 0), Point::new(3, 63)] {
                        let style = TextStyle::lit(font)
                            .aligned(alignment)
                            .with_baseline(baseline);
                        assert_same(name, font, text, at, style);
                    }
                }
            }
        }
    }
}

#[test]
fn a_character_outside_a_font_draws_its_replacement_glyph() {
    for (name, font) in fonts::ALL {
        for text in ["\u{e9}", "\u{1f600}", "\t", "a\u{0}b"] {
            assert_same(name, font, text, Point::new(10, 8), TextStyle::lit(font));
        }
    }
}

#[test]
fn every_font_is_generated() {
    assert_eq!(fonts::ALL.len(), SOURCES.len());
}
