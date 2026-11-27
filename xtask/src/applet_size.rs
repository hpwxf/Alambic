//! An estimate of each applet's share of `.text`, from the image's symbols
//! (`llvm-nm`): the sizes of the functions whose demangled name lies in the
//! applet's module, `oc_core::apps::<applet>`, or in `oc_dsp::<applet>`.
//!
//! The engine reaches every applet through `&dyn App`, from a function that
//! is never inlined (`Instance::app_mut`): an applet's methods stay
//! functions of their own, named after it, whatever the link-time
//! optimiser does. The applets are found from those names, so that a new
//! one shows without a list to keep.
//!
//! An estimate, not a measure (journal/D-038, which measures an applet's
//! cost by building the image with and without it):
//!
//! * what is inlined into a shared function counts as shared: the per-sample
//!   DSP into the audio interrupt, the applets' constructors into
//!   `Instance::new`;
//! * a shared function inlined into an applet's counts as the applet's;
//! * a generic function counts under the type it is generic over, which may
//!   be no applet's.
//!
//! # `llvm-nm` contract
//!
//! ```text
//! llvm-nm --print-size --demangle --defined-only -t d <elf>
//! ```
//!
//! One symbol a line: its address, its size in decimal, its type letter,
//! then its demangled name, which may hold spaces. Only code counts (`t`,
//! `T`, `w`, `W`).

use std::collections::BTreeMap;
use std::fmt::Write as _;

/// The modules under `oc_core::apps` that are no applet.
const NOT_APPLETS: [&str; 3] = ["param", "sound", "bus"];

/// One function of the image.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Symbol {
    /// Its size, in bytes.
    pub size: u64,
    /// Its demangled name.
    pub name: String,
}

/// The functions of `llvm-nm`'s output (see the module's contract).
#[must_use]
pub fn parse_nm(text: &str) -> Vec<Symbol> {
    text.lines()
        .filter_map(|line| {
            let mut fields = line.trim_start().splitn(4, ' ');
            let _address = fields.next()?;
            let size = fields.next()?.parse().ok()?;
            let kind = fields.next()?;
            let name = fields.next()?.trim();
            matches!(kind, "t" | "T" | "w" | "W").then(|| Symbol {
                size,
                name: name.to_owned(),
            })
        })
        .collect()
}

/// Bytes and functions of one share.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Share {
    /// Their sizes added up.
    pub bytes: u64,
    /// How many functions.
    pub functions: usize,
}

impl Share {
    fn add(&mut self, size: u64) {
        self.bytes += size;
        self.functions += 1;
    }
}

/// The image's code shared out.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Breakdown {
    /// Each applet's, by its module's name.
    pub applets: BTreeMap<String, Share>,
    /// What no applet owns, by the part of the workspace or the world it
    /// comes from.
    pub shared: BTreeMap<&'static str, Share>,
}

impl Breakdown {
    /// Every function's size added up.
    #[must_use]
    pub fn total(&self) -> u64 {
        self.applets
            .values()
            .chain(self.shared.values())
            .map(|share| share.bytes)
            .sum()
    }
}

/// The applet module the name lies in, if any: the first
/// `oc_core::apps::<module>::` or `oc_dsp::<module>::` it holds, the module
/// a lower-case name, not one of [`NOT_APPLETS`].
fn applet_of(name: &str) -> Option<&str> {
    let mut found: Option<(usize, &str)> = None;
    for prefix in ["oc_core::apps::", "oc_dsp::"] {
        let mut from = 0;
        while let Some(at) = name[from..].find(prefix) {
            let start = from + at + prefix.len();
            let module: &str = name[start..]
                .split(|c: char| !(c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_'))
                .next()
                .unwrap_or("");
            let follows_path = name[start + module.len()..].starts_with("::");
            if !module.is_empty() && follows_path && !NOT_APPLETS.contains(&module) {
                if found.is_none_or(|(earliest, _)| start < earliest) {
                    found = Some((start, module));
                }
                break;
            }
            from = start;
        }
    }
    found.map(|(_, module)| module)
}

/// The shared part a function no applet owns comes from.
fn shared_of(name: &str) -> &'static str {
    let parts: [(&str, &[&str]); 4] = [
        ("oc_core, shared", &["oc_core::"]),
        ("oc_dsp, shared", &["oc_dsp::"]),
        ("probe protocol", &["oc_protocol::"]),
        (
            "firmware, board, drivers",
            &[
                "oc_firmware::",
                "oc_realtime::",
                "oc_board::",
                "oc_drivers::",
            ],
        ),
    ];
    parts
        .iter()
        .find(|(_, prefixes)| prefixes.iter().any(|prefix| name.contains(prefix)))
        .map_or("Rust core, USB, interrupts, the rest", |(label, _)| *label)
}

/// Shares `symbols` out: an applet's where its name says so, the rest by
/// where it comes from. A module of `oc_dsp` named after no applet that
/// `oc_core::apps` has is shared.
#[must_use]
pub fn attribute(symbols: &[Symbol]) -> Breakdown {
    let core_applets: Vec<&str> = symbols
        .iter()
        .filter(|symbol| symbol.name.contains("oc_core::apps::"))
        .filter_map(|symbol| applet_of(&symbol.name))
        .collect();
    let mut breakdown = Breakdown::default();
    for symbol in symbols {
        match applet_of(&symbol.name).filter(|module| core_applets.contains(module)) {
            Some(module) => breakdown
                .applets
                .entry(module.to_owned())
                .or_default()
                .add(symbol.size),
            None => breakdown
                .shared
                .entry(shared_of(&symbol.name))
                .or_default()
                .add(symbol.size),
        }
    }
    breakdown
}

/// The breakdown as `cargo xtask size` prints it: the applets, largest
/// first, then the shared parts, each in bytes, KiB and functions.
#[must_use]
pub fn format_breakdown(breakdown: &Breakdown) -> String {
    let mut out = String::from(
        "estimated .text per applet (by symbol; DSP inlined in the audio interrupt and \
         constructors in Instance::new count as shared):\n",
    );
    let mut applets: Vec<_> = breakdown.applets.iter().collect();
    applets.sort_by(|a, b| b.1.bytes.cmp(&a.1.bytes).then(a.0.cmp(b.0)));
    let mut shared: Vec<_> = breakdown.shared.iter().collect();
    shared.sort_by(|a, b| b.1.bytes.cmp(&a.1.bytes).then(a.0.cmp(b.0)));
    let line = |out: &mut String, name: &str, share: &Share| {
        let _ = writeln!(
            out,
            "  {name:<38} {:>7} B  {:>5.1} KiB  {:>4} functions",
            share.bytes,
            kib(share.bytes),
            share.functions
        );
    };
    for (name, share) in applets {
        // As the app menu names it: `gfx_demo` is `GFX DEMO`.
        line(
            &mut out,
            &name.to_ascii_uppercase().replace('_', " "),
            share,
        );
    }
    for (name, share) in shared {
        line(&mut out, &format!("({name})"), share);
    }
    let _ = write!(
        out,
        "  {:<38} {:>7} B  {:>5.1} KiB",
        "total of the functions",
        breakdown.total(),
        kib(breakdown.total())
    );
    out
}

/// `bytes` in KiB, for display.
#[allow(clippy::cast_precision_loss)] // A firmware's size fits an f64's mantissa many times over.
fn kib(bytes: u64) -> f64 {
    bytes as f64 / 1024.0
}
