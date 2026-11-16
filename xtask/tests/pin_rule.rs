//! Enforces the firmware's pin-naming rule (journal/D-007).
//!
//! `crates/oc-firmware/src/board.rs` is the only firmware source allowed to
//! name a Teensy pin, because a wrong pin direction is the one way this
//! firmware could damage hardware. The rule used to live in documentation
//! only and drifted; this test makes it a build failure instead. It runs on
//! the host, so a plain `cargo test` covers the cross-compiled crate's
//! sources too.
//!
//! A pin is named in code as a `teensy4-bsp` token: `P13` (the pad type) or
//! `p13` (the field of the BSP's pin set). Comments are ignored; prose about
//! wiring may mention pin numbers.

use std::fs;
use std::path::{Path, PathBuf};

/// The single file allowed to name a pin.
const BOARD: &str = "board.rs";

fn firmware_sources() -> Vec<PathBuf> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../crates/oc-firmware/src");
    let mut sources = Vec::new();
    let mut pending = vec![root];
    while let Some(dir) = pending.pop() {
        let entries = fs::read_dir(&dir)
            .unwrap_or_else(|error| panic!("cannot read {}: {error}", dir.display()));
        for entry in entries {
            let path = entry.expect("readable directory entry").path();
            if path.is_dir() {
                pending.push(path);
            } else if path.extension().is_some_and(|ext| ext == "rs") {
                sources.push(path);
            }
        }
    }
    sources.sort();
    sources
}

/// Whether `token` is a BSP pin name such as `P13` or `p13`.
fn is_pin_token(token: &str) -> bool {
    let mut chars = token.chars();
    matches!(chars.next(), Some('p' | 'P'))
        && token.len() > 1
        && chars.all(|character| character.is_ascii_digit())
}

/// Pin tokens in the code (not the comments) of one line.
fn pin_tokens(line: &str) -> Vec<String> {
    let code = line.split("//").next().unwrap_or_default();
    code.split(|character: char| !(character.is_ascii_alphanumeric() || character == '_'))
        .filter(|token| is_pin_token(token))
        .map(str::to_owned)
        .collect()
}

#[test]
fn the_token_matcher_recognises_pin_names_only() {
    assert_eq!(pin_tokens("let x = pins.p13;"), ["p13"]);
    assert_eq!(pin_tokens("use teensy4_bsp::pins::common::P0;"), ["P0"]);
    assert!(pin_tokens("// pin 13 is the LED, P13 in the BSP").is_empty());
    assert!(pin_tokens("let pins = board::split(pins); let p = 1; u32;").is_empty());
    assert!(pin_tokens("Pullup22k ResolutionBits::Res12").is_empty());
}

#[test]
fn only_board_rs_names_a_teensy_pin() {
    let sources = firmware_sources();
    assert!(
        sources.iter().any(|path| path.ends_with(BOARD)),
        "{BOARD} not found: the firmware source layout changed"
    );

    let mut violations = Vec::new();
    for path in sources.iter().filter(|path| !path.ends_with(BOARD)) {
        let text = fs::read_to_string(path)
            .unwrap_or_else(|error| panic!("cannot read {}: {error}", path.display()));
        for (number, line) in text.lines().enumerate() {
            for token in pin_tokens(line) {
                violations.push(format!("{}:{}: `{token}`", path.display(), number + 1));
            }
        }
    }

    assert!(
        violations.is_empty(),
        "only crates/oc-firmware/src/{BOARD} may name a Teensy pin \
         (journal/D-007); add a signal group there instead:\n{}",
        violations.join("\n")
    );
}
