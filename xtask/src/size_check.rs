//! Layout checklist over `llvm-size` `SysV` output.
//!
//! `cargo xtask size` used to dump `llvm-size -A` and leave a human to notice
//! whether `.stack` still sits below `.vector_table`. That invariant is the
//! project's stand-in for `flip-link` (see `crates/oc-firmware/MEMORY.md`), so
//! it belongs in automation rather than in a doc comment.
//!
//! The checker is deliberately a pure function over captured tool text:
//!
//! * integration tests feed fixtures without spawning LLVM;
//! * the CLI only has to choose stable `llvm-size` flags and print the report.
//!
//! # Portable `llvm-size` contract
//!
//! Invocation used by the CLI:
//!
//! ```text
//! llvm-size --format=sysv --radix=16 <elf>
//! ```
//!
//! Long option names avoid short-flag alias churn. `SysV` format is one section
//! per line (`name size addr`), which is far more stable to parse than the
//! Berkeley summary. Radix 16 keeps addresses aligned with the memory map in
//! `MEMORY.md`; the parser still accepts bare decimal tokens because several
//! LLVM releases print `0` without a `0x` prefix for a zero address or size.

use std::collections::BTreeMap;
use std::fmt::{Display, Formatter};

use crate::validate::{
    DTCM, DTCM_USABLE, FLEXSPI, FLEXSPI_BASE, ITCM, ITCM_USABLE, OCRAM, OCRAM_CAPACITY,
    TEENSY40_FLASH_CAPACITY,
};

/// Sections that must appear in a linked Teensy 4.0 image from `imxrt-rt`.
const REQUIRED_SECTIONS: [&str; 4] = [".boot", ".stack", ".vector_table", ".text"];

/// Sections printed in the selective footprint table (no DWARF noise).
const DISPLAY_SECTIONS: [&str; 9] = [
    ".boot",
    ".stack",
    ".vector_table",
    ".text",
    ".rodata",
    ".data",
    ".bss",
    ".uninit",
    ".heap",
];

/// Default stack reservation produced by `teensy4-bsp` / `imxrt-rt` (16 KiB).
///
/// Soft expected size for the checklist warn path. Hard upper bound is the
/// remaining DTCM on the Teensy 4.0 BSP split: **≤ 320 KiB** total DTCM
/// (exclusive end `0x2008_0000`), shared with `.vector_table`/`.data`/`.bss`.
/// Override via `TEENSY4_STACK_SIZE`.
const DEFAULT_STACK_SIZE: u64 = 16 * 1024;

/// Default `.boot` reservation (FCB + IVT + boot data), 8 KiB.
///
/// Soft expected size. Hard upper bound is the Teensy 4.0 application flash
/// partition: **1984 KiB** (`TEENSY40_FLASH_CAPACITY`), shared with the rest of
/// the loadable image; the `FlexSPI` address window ends at **`0x6800_0000`**.
const DEFAULT_BOOT_SIZE: u64 = 8 * 1024;

/// Minimum useful vector-table payload: initial SP + reset handler.
///
/// Lower bound only (8 bytes). No separate size ceiling is enforced here; the
/// table must still fit in DTCM (**≤ 320 KiB** usable on Teensy 4.0, exclusive
/// end `0x2008_0000`). `imxrt-rt` typically emits on the order of 1 KiB.
const MIN_VECTOR_TABLE_SIZE: u64 = 8;

/// One ELF section row extracted from `SysV` `llvm-size` output.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Section {
    /// Section name, including the leading `.` when present.
    pub name: String,
    /// Section size in bytes.
    pub size: u64,
    /// Section VMA as reported by `llvm-size`.
    pub addr: u64,
}

/// Outcome of a single checklist entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CheckStatus {
    /// Hard requirement satisfied.
    Pass,
    /// Soft finding: show it, do not fail the command.
    Warn,
    /// Hard requirement failed; `cargo xtask size` must exit non-zero.
    Fail,
}

impl CheckStatus {
    /// Tag used in the printed checklist.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Pass => "pass",
            Self::Warn => "warn",
            Self::Fail => "FAIL",
        }
    }
}

/// One row of the layout checklist.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckItem {
    /// Short stable identifier for tests and grepping.
    pub id: &'static str,
    /// Pass / warn / fail.
    pub status: CheckStatus,
    /// Human-readable detail, including addresses when relevant.
    pub detail: String,
}

impl Display for CheckItem {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        // Single-row fallback; `format_checklist` aligns a full table.
        write!(
            f,
            "{:<6}  {}  {}",
            self.status.label(),
            self.id,
            self.detail
        )
    }
}

/// Full checklist result for one `llvm-size` capture.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LayoutReport {
    /// Ordered checklist rows.
    pub items: Vec<CheckItem>,
}

impl LayoutReport {
    /// Whether any hard check failed.
    #[must_use]
    pub fn failed(&self) -> bool {
        self.items
            .iter()
            .any(|item| item.status == CheckStatus::Fail)
    }

    /// All failing items, in checklist order.
    #[must_use]
    pub fn failures(&self) -> Vec<&CheckItem> {
        self.items
            .iter()
            .filter(|item| item.status == CheckStatus::Fail)
            .collect()
    }
}

/// Parses `SysV` `llvm-size` text into section rows.
///
/// Accepts both `--radix=16` (`0x…` tokens, with bare `0` still allowed) and
/// `--radix=10` decimal output so fixtures and older dumps remain usable.
///
/// # Errors
///
/// Returns a single actionable message when no section rows can be recovered
/// (empty capture, Berkeley format, or a format change that dropped the table).
pub fn parse_sysv(output: &str) -> Result<Vec<Section>, String> {
    let mut sections = Vec::new();

    for raw_line in output.lines() {
        let line = raw_line.trim();
        if line.is_empty() {
            continue;
        }
        // Filename banner: "/path/to/elf  :"
        if line.ends_with(':') {
            continue;
        }

        let fields: Vec<&str> = line.split_whitespace().collect();
        if fields.is_empty() {
            continue;
        }
        let name = fields[0];

        // Column header or Berkeley totals line.
        if name.eq_ignore_ascii_case("section") || name.eq_ignore_ascii_case("total") {
            continue;
        }
        // Berkeley format leads with `text data bss ...` — reject early so the
        // caller gets a clear "wrong format" signal instead of an empty parse.
        if name.eq_ignore_ascii_case("text")
            && fields
                .get(1)
                .is_some_and(|field| field.eq_ignore_ascii_case("data"))
        {
            return Err(
                "llvm-size output looks like Berkeley format; expected SysV \
                 (`--format=sysv`)"
                    .to_owned(),
            );
        }

        // SysV section rows are exactly three logical columns.
        if fields.len() != 3 {
            continue;
        }
        let size_tok = fields[1];
        let addr_tok = fields[2];

        let Some(size) = parse_number(size_tok) else {
            continue;
        };
        let Some(addr) = parse_number(addr_tok) else {
            continue;
        };

        sections.push(Section {
            name: name.to_owned(),
            size,
            addr,
        });
    }

    if sections.is_empty() {
        return Err("no section rows found in llvm-size SysV output; \
             is `llvm-tools` producing `--format=sysv`?"
            .to_owned());
    }

    Ok(sections)
}

/// Runs every layout check that can be decided from section names/addresses.
///
/// All checks run to completion so the operator sees the full checklist in one
/// pass (same reporting style as [`crate::validate::validate_image`]).
#[must_use]
pub fn check_layout(sections: &[Section]) -> LayoutReport {
    let by_name = index_sections(sections);
    let mut items = Vec::new();

    check_required_sections(&by_name, &mut items);
    check_boot(&by_name, &mut items);
    check_stack_and_vector_table(&by_name, &mut items);
    check_text(&by_name, &mut items);
    check_optional_dtcm(&by_name, ".data", &mut items);
    check_optional_dtcm(&by_name, ".bss", &mut items);
    check_data_follows_vector_table(&by_name, &mut items);
    check_optional_ocram(&by_name, ".rodata", &mut items);
    check_optional_ocram(&by_name, ".heap", &mut items);
    check_optional_ocram(&by_name, ".uninit", &mut items);

    LayoutReport { items }
}

/// Renders the selective footprint table (runtime sections only).
#[must_use]
pub fn format_section_table(sections: &[Section]) -> String {
    let mut lines = Vec::with_capacity(DISPLAY_SECTIONS.len() + 2);
    lines.push(format!("{:<18} {:>10} {:>12}", "section", "size", "addr"));

    let by_name = index_sections(sections);
    for name in DISPLAY_SECTIONS {
        if let Some(section) = by_name.get(name) {
            lines.push(format!(
                "{:<18} {:>10} {:>12}",
                section.name,
                format_hex(section.size),
                format_hex(section.addr)
            ));
        }
    }

    lines.join("\n")
}

/// Renders the checklist block printed after the section table.
///
/// Columns are fixed-width so status / check id / detail line up for scanning:
///
/// ```text
/// status  check                       detail
/// pass    stack-below-vector-table    .stack [...] is strictly below ...
/// ```
#[must_use]
pub fn format_checklist(report: &LayoutReport) -> String {
    const STATUS_WIDTH: usize = 6; // fits "status" and "pass"/"warn"/"FAIL"
    let id_width = report
        .items
        .iter()
        .map(|item| item.id.len())
        .max()
        .unwrap_or(0)
        .max("check".len());

    let mut lines = Vec::with_capacity(report.items.len() + 3);
    lines.push("layout checklist:".to_owned());
    lines.push(format!(
        "{:<status_w$}  {:<id_w$}  {}",
        "status",
        "check",
        "detail",
        status_w = STATUS_WIDTH,
        id_w = id_width,
    ));
    for item in &report.items {
        lines.push(format!(
            "{:<status_w$}  {:<id_w$}  {}",
            item.status.label(),
            item.id,
            item.detail,
            status_w = STATUS_WIDTH,
            id_w = id_width,
        ));
    }
    if report.failed() {
        lines.push("result: FAIL (see FAIL rows above)".to_owned());
    } else {
        lines.push("result: ok".to_owned());
    }
    lines.join("\n")
}

fn index_sections(sections: &[Section]) -> BTreeMap<&str, &Section> {
    let mut map = BTreeMap::new();
    for section in sections {
        // First occurrence wins; duplicate names would already be odd in an ELF.
        map.entry(section.name.as_str()).or_insert(section);
    }
    map
}

fn check_required_sections(by_name: &BTreeMap<&str, &Section>, items: &mut Vec<CheckItem>) {
    let missing: Vec<&str> = REQUIRED_SECTIONS
        .iter()
        .copied()
        .filter(|name| !by_name.contains_key(name))
        .collect();

    if missing.is_empty() {
        items.push(CheckItem {
            id: "required-sections",
            status: CheckStatus::Pass,
            detail: format!("present: {}", REQUIRED_SECTIONS.join(", ")),
        });
    } else {
        items.push(CheckItem {
            id: "required-sections",
            status: CheckStatus::Fail,
            detail: format!(
                "missing {}; need {}",
                missing.join(", "),
                REQUIRED_SECTIONS.join(", ")
            ),
        });
    }
}

/// `.boot` must sit at the `FlexSPI` base; size is compared to the 8 KiB default.
///
/// Address upper bound of the flash window: **`0x6800_0000`**. Usable image
/// budget on Teensy 4.0: **1984 KiB** application partition.
fn check_boot(by_name: &BTreeMap<&str, &Section>, items: &mut Vec<CheckItem>) {
    let Some(boot) = by_name.get(".boot").copied() else {
        items.push(CheckItem {
            id: "boot-flexspi",
            status: CheckStatus::Fail,
            detail: "section .boot missing; cannot verify FlexSPI placement".to_owned(),
        });
        return;
    };

    let flash_budget = format_bytes_kib(TEENSY40_FLASH_CAPACITY);
    let flexspi = format_region(FLEXSPI);
    if boot.addr == u64::from(FLEXSPI_BASE) {
        items.push(CheckItem {
            id: "boot-flexspi",
            status: CheckStatus::Pass,
            detail: format!(
                ".boot @ {} (FlexSPI base), size {}; window {flexspi}, app flash ≤ {flash_budget}",
                format_hex(boot.addr),
                format_hex(boot.size),
            ),
        });
    } else {
        items.push(CheckItem {
            id: "boot-flexspi",
            status: CheckStatus::Fail,
            detail: format!(
                ".boot @ {}, expected {} (FlexSPI base); window {flexspi}, app flash ≤ {flash_budget}",
                format_hex(boot.addr),
                format_hex(u64::from(FLEXSPI_BASE)),
            ),
        });
    }

    if boot.size == 0 {
        items.push(CheckItem {
            id: "boot-size",
            status: CheckStatus::Fail,
            detail: ".boot size is 0; FCB/IVT must occupy the boot section".to_owned(),
        });
    } else if boot.size == DEFAULT_BOOT_SIZE {
        items.push(CheckItem {
            id: "boot-size",
            status: CheckStatus::Pass,
            detail: format!(
                ".boot size {} (default 8 KiB; app flash partition ≤ {flash_budget})",
                format_hex(boot.size)
            ),
        });
    } else {
        items.push(CheckItem {
            id: "boot-size",
            status: CheckStatus::Warn,
            detail: format!(
                ".boot size {} (default is {}; app flash partition ≤ {flash_budget}; confirm imxrt-rt boot layout)",
                format_hex(boot.size),
                format_hex(DEFAULT_BOOT_SIZE)
            ),
        });
    }
}

fn check_stack_and_vector_table(by_name: &BTreeMap<&str, &Section>, items: &mut Vec<CheckItem>) {
    let stack = by_name.get(".stack").copied();
    let vector_table = by_name.get(".vector_table").copied();
    check_stack(stack, items);
    check_vector_table(vector_table, items);
    check_stack_below_vector_table(stack, vector_table, items);
}

/// `.stack` must start at the DTCM base and lie entirely inside DTCM.
///
/// Teensy 4.0 DTCM upper bound: exclusive end **`0x2008_0000`** (512 KiB
/// aperture; **320 KiB** backed by the BSP `FlexRAM` split).
fn check_stack(stack: Option<&Section>, items: &mut Vec<CheckItem>) {
    let Some(stack) = stack else {
        items.push(CheckItem {
            id: "stack-dtcm-base",
            status: CheckStatus::Fail,
            detail: "section .stack missing; stack-overflow fault path cannot be verified"
                .to_owned(),
        });
        return;
    };

    let dtcm = format_region(DTCM);
    let dtcm_budget = format_bytes_kib(DTCM_USABLE);
    if stack.addr == u64::from(DTCM.start) {
        items.push(CheckItem {
            id: "stack-dtcm-base",
            status: CheckStatus::Pass,
            detail: format!(
                ".stack @ {} (DTCM base {}), size {}; usable ≤ {dtcm_budget} shared",
                format_hex(stack.addr),
                dtcm,
                format_hex(stack.size),
            ),
        });
    } else {
        items.push(CheckItem {
            id: "stack-dtcm-base",
            status: CheckStatus::Fail,
            detail: format!(
                ".stack @ {}, expected {} (DTCM base {}); overflow would not leave DTCM cleanly; usable ≤ {dtcm_budget}",
                format_hex(stack.addr),
                format_hex(u64::from(DTCM.start)),
                dtcm,
            ),
        });
    }

    if stack.size == 0 {
        items.push(CheckItem {
            id: "stack-size",
            status: CheckStatus::Fail,
            detail: ".stack size is 0".to_owned(),
        });
    } else if stack.size == DEFAULT_STACK_SIZE {
        items.push(CheckItem {
            id: "stack-size",
            status: CheckStatus::Pass,
            detail: format!(
                ".stack size {} (default 16 KiB; DTCM usable ≤ {dtcm_budget} shared)",
                format_hex(stack.size)
            ),
        });
    } else {
        items.push(CheckItem {
            id: "stack-size",
            status: CheckStatus::Warn,
            detail: format!(
                ".stack size {} (default is {}; DTCM usable ≤ {dtcm_budget} shared; TEENSY4_STACK_SIZE override?)",
                format_hex(stack.size),
                format_hex(DEFAULT_STACK_SIZE)
            ),
        });
    }

    let stack_end = stack.addr.saturating_add(stack.size);
    if region_contains(DTCM, stack.addr, stack.size) {
        items.push(CheckItem {
            id: "stack-in-dtcm",
            status: CheckStatus::Pass,
            detail: format!(
                ".stack [{}, {}) inside DTCM {dtcm}; usable ≤ {dtcm_budget} shared",
                format_hex(stack.addr),
                format_hex(stack_end),
            ),
        });
    } else {
        items.push(CheckItem {
            id: "stack-in-dtcm",
            status: CheckStatus::Fail,
            detail: format!(
                ".stack [{}, {}) escapes DTCM {dtcm}; usable ≤ {dtcm_budget}",
                format_hex(stack.addr),
                format_hex(stack_end),
            ),
        });
    }
}

/// `.vector_table` must reside in DTCM and hold at least SP + reset.
///
/// Teensy 4.0 DTCM upper bound: exclusive end **`0x2008_0000`** (**≤ 320 KiB**
/// usable with the BSP bank split).
fn check_vector_table(vector_table: Option<&Section>, items: &mut Vec<CheckItem>) {
    let Some(vt) = vector_table else {
        items.push(CheckItem {
            id: "vector-table-dtcm",
            status: CheckStatus::Fail,
            detail: "section .vector_table missing".to_owned(),
        });
        return;
    };

    let dtcm = format_region(DTCM);
    let dtcm_budget = format_bytes_kib(DTCM_USABLE);
    if region_contains(DTCM, vt.addr, vt.size.max(1)) {
        items.push(CheckItem {
            id: "vector-table-dtcm",
            status: CheckStatus::Pass,
            detail: format!(
                ".vector_table @ {}, size {} (DTCM {dtcm}; usable ≤ {dtcm_budget} shared)",
                format_hex(vt.addr),
                format_hex(vt.size),
            ),
        });
    } else {
        items.push(CheckItem {
            id: "vector-table-dtcm",
            status: CheckStatus::Fail,
            detail: format!(
                ".vector_table @ {} is outside DTCM {dtcm}; usable ≤ {dtcm_budget}",
                format_hex(vt.addr),
            ),
        });
    }

    if vt.size < MIN_VECTOR_TABLE_SIZE {
        items.push(CheckItem {
            id: "vector-table-size",
            status: CheckStatus::Fail,
            detail: format!(
                ".vector_table size {} is smaller than {MIN_VECTOR_TABLE_SIZE} bytes (SP + reset); DTCM usable ≤ {dtcm_budget}",
                format_hex(vt.size)
            ),
        });
    } else {
        items.push(CheckItem {
            id: "vector-table-size",
            status: CheckStatus::Pass,
            detail: format!(
                ".vector_table size {} (≥ {MIN_VECTOR_TABLE_SIZE} bytes; DTCM usable ≤ {dtcm_budget} shared)",
                format_hex(vt.size)
            ),
        });
    }
}

/// Critical anti-overflow invariant: stack below the vector table, no overlap.
///
/// This is why the project does not need `flip-link`.
fn check_stack_below_vector_table(
    stack: Option<&Section>,
    vector_table: Option<&Section>,
    items: &mut Vec<CheckItem>,
) {
    match (stack, vector_table) {
        (Some(stack), Some(vt)) => {
            let stack_end = stack.addr.saturating_add(stack.size);
            if stack.addr < vt.addr && stack_end <= vt.addr {
                items.push(CheckItem {
                    id: "stack-below-vector-table",
                    status: CheckStatus::Pass,
                    detail: format!(
                        ".stack [{}, {}) is strictly below .vector_table @ {}",
                        format_hex(stack.addr),
                        format_hex(stack_end),
                        format_hex(vt.addr)
                    ),
                });
            } else if stack.addr < vt.addr {
                items.push(CheckItem {
                    id: "stack-below-vector-table",
                    status: CheckStatus::Fail,
                    detail: format!(
                        ".stack ends at {} which overlaps .vector_table @ {}",
                        format_hex(stack_end),
                        format_hex(vt.addr)
                    ),
                });
            } else {
                items.push(CheckItem {
                    id: "stack-below-vector-table",
                    status: CheckStatus::Fail,
                    detail: format!(
                        ".stack @ {} is not below .vector_table @ {} \
                         (overflow would corrupt the vector table / statics)",
                        format_hex(stack.addr),
                        format_hex(vt.addr)
                    ),
                });
            }
        }
        _ => items.push(CheckItem {
            id: "stack-below-vector-table",
            status: CheckStatus::Fail,
            detail: "cannot compare .stack and .vector_table; one or both missing".to_owned(),
        }),
    }
}

/// `.text` must be non-empty and lie entirely inside ITCM.
///
/// Teensy 4.0 ITCM upper bound: exclusive end **`0x0008_0000`** (512 KiB
/// aperture; **192 KiB** backed by the BSP `FlexRAM` split for code).
fn check_text(by_name: &BTreeMap<&str, &Section>, items: &mut Vec<CheckItem>) {
    let Some(text) = by_name.get(".text").copied() else {
        items.push(CheckItem {
            id: "text-itcm",
            status: CheckStatus::Fail,
            detail: "section .text missing".to_owned(),
        });
        return;
    };

    let itcm = format_region(ITCM);
    let itcm_budget = format_bytes_kib(ITCM_USABLE);
    if text.size == 0 {
        items.push(CheckItem {
            id: "text-itcm",
            status: CheckStatus::Fail,
            detail: format!(".text size is 0 (ITCM {itcm}; usable ≤ {itcm_budget})"),
        });
        return;
    }

    let text_end = text.addr.saturating_add(text.size);
    if region_contains(ITCM, text.addr, text.size) {
        items.push(CheckItem {
            id: "text-itcm",
            status: CheckStatus::Pass,
            detail: format!(
                ".text @ {}, size {} / ≤ {itcm_budget} usable (ITCM {itcm})",
                format_hex(text.addr),
                format_hex(text.size),
            ),
        });
    } else {
        items.push(CheckItem {
            id: "text-itcm",
            status: CheckStatus::Fail,
            detail: format!(
                ".text [{}, {}) escapes ITCM {itcm}; usable ≤ {itcm_budget}",
                format_hex(text.addr),
                format_hex(text_end),
            ),
        });
    }
}

/// Optional DTCM residents (`.data`, `.bss`) must not escape DTCM.
///
/// Teensy 4.0 DTCM upper bound: exclusive end **`0x2008_0000`** (**≤ 320 KiB**
/// usable with the BSP bank split, shared with stack and vector table).
fn check_optional_dtcm(
    by_name: &BTreeMap<&str, &Section>,
    name: &'static str,
    items: &mut Vec<CheckItem>,
) {
    let id = match name {
        ".data" => "data-dtcm",
        ".bss" => "bss-dtcm",
        _ => "dtcm-section",
    };

    let dtcm = format_region(DTCM);
    let dtcm_budget = format_bytes_kib(DTCM_USABLE);
    let Some(section) = by_name.get(name).copied() else {
        items.push(CheckItem {
            id,
            status: CheckStatus::Warn,
            detail: format!(
                "section {name} absent (ok if the image has no {name} payload; DTCM {dtcm}, usable ≤ {dtcm_budget})"
            ),
        });
        return;
    };

    // Zero-sized sections may still carry a plausible address.
    let span = section.size.max(1);
    if region_contains(DTCM, section.addr, span)
        || (section.size == 0 && in_range(DTCM, section.addr))
    {
        items.push(CheckItem {
            id,
            status: CheckStatus::Pass,
            detail: format!(
                "{name} @ {}, size {} (DTCM {dtcm}; usable ≤ {dtcm_budget} shared)",
                format_hex(section.addr),
                format_hex(section.size),
            ),
        });
    } else {
        items.push(CheckItem {
            id,
            status: CheckStatus::Fail,
            detail: format!(
                "{name} @ {} is outside DTCM {dtcm}; usable ≤ {dtcm_budget}",
                format_hex(section.addr),
            ),
        });
    }
}

fn check_data_follows_vector_table(by_name: &BTreeMap<&str, &Section>, items: &mut Vec<CheckItem>) {
    let (Some(vt), Some(data)) = (
        by_name.get(".vector_table").copied(),
        by_name.get(".data").copied(),
    ) else {
        // Covered by the individual optional/required checks.
        return;
    };

    let vt_end = vt.addr.saturating_add(vt.size);
    if data.addr >= vt_end {
        items.push(CheckItem {
            id: "data-above-vector-table",
            status: CheckStatus::Pass,
            detail: format!(
                ".data @ {} is at/after .vector_table end {}",
                format_hex(data.addr),
                format_hex(vt_end)
            ),
        });
    } else {
        items.push(CheckItem {
            id: "data-above-vector-table",
            status: CheckStatus::Fail,
            detail: format!(
                ".data @ {} overlaps .vector_table [{}, {})",
                format_hex(data.addr),
                format_hex(vt.addr),
                format_hex(vt_end)
            ),
        });
    }
}

/// Optional OCRAM residents (`.rodata`, `.heap`, `.uninit`) must stay in OCRAM.
///
/// Teensy 4.0 OCRAM upper bound: exclusive end **`0x2028_0000`** (**512 KiB**
/// dedicated on-chip RAM). Default `.heap` reservation from `imxrt-rt` is 16 KiB.
fn check_optional_ocram(
    by_name: &BTreeMap<&str, &Section>,
    name: &'static str,
    items: &mut Vec<CheckItem>,
) {
    let id = match name {
        ".rodata" => "rodata-ocram",
        ".heap" => "heap-ocram",
        ".uninit" => "uninit-ocram",
        _ => "ocram-section",
    };

    let ocram = format_region(OCRAM);
    let ocram_budget = format_bytes_kib(OCRAM_CAPACITY);
    let Some(section) = by_name.get(name).copied() else {
        // `.uninit` is frequently empty/absent; stay quiet unless present.
        if name == ".uninit" {
            return;
        }
        items.push(CheckItem {
            id,
            status: CheckStatus::Warn,
            detail: format!("section {name} absent (OCRAM {ocram}, ≤ {ocram_budget})"),
        });
        return;
    };

    let span = section.size.max(1);
    let ok = if section.size == 0 {
        in_range(OCRAM, section.addr) || section.addr == 0
    } else {
        region_contains(OCRAM, section.addr, span)
    };

    if ok {
        items.push(CheckItem {
            id,
            status: CheckStatus::Pass,
            detail: format!(
                "{name} @ {}, size {} / ≤ {ocram_budget} (OCRAM {ocram})",
                format_hex(section.addr),
                format_hex(section.size),
            ),
        });
    } else if section.size == 0 && section.addr == 0 {
        // Empty section with a null address is an ELF placeholder — ignore.
        items.push(CheckItem {
            id,
            status: CheckStatus::Pass,
            detail: format!("{name} empty (OCRAM {ocram}, ≤ {ocram_budget})"),
        });
    } else {
        items.push(CheckItem {
            id,
            status: CheckStatus::Fail,
            detail: format!(
                "{name} @ {} is outside OCRAM {ocram}; capacity ≤ {ocram_budget}",
                format_hex(section.addr),
            ),
        });
    }
}

/// True when `[addr, addr + size)` lies inside `region` (exclusive end = max).
///
/// Callers pass the Teensy 4.0 windows from [`crate::validate`]: ITCM/DTCM end
/// at `0x…8_0000` (512 KiB aperture each), OCRAM at `0x2028_0000` (512 KiB),
/// with tighter usable `FlexRAM` budgets noted on those constants.
fn region_contains(region: std::ops::Range<u32>, addr: u64, size: u64) -> bool {
    let Some(end) = addr.checked_add(size) else {
        return false;
    };
    let start_ok = addr >= u64::from(region.start);
    let end_ok = end <= u64::from(region.end);
    start_ok && end_ok
}

fn in_range(region: std::ops::Range<u32>, addr: u64) -> bool {
    addr >= u64::from(region.start) && addr < u64::from(region.end)
}

fn parse_number(token: &str) -> Option<u64> {
    let token = token.trim();
    if let Some(hex) = token
        .strip_prefix("0x")
        .or_else(|| token.strip_prefix("0X"))
    {
        return u64::from_str_radix(hex, 16).ok();
    }
    // Bare tokens: llvm-size often prints a plain `0` even under `--radix=16`.
    token.parse::<u64>().ok()
}

fn format_hex(value: u64) -> String {
    format!("0x{value:x}")
}

/// Formats a half-open address window as `[start, end)`.
fn format_region(region: std::ops::Range<u32>) -> String {
    format!(
        "[{}, {})",
        format_hex(u64::from(region.start)),
        format_hex(u64::from(region.end))
    )
}

/// Human-readable size for Teensy budgets (always whole KiB in this crate).
fn format_bytes_kib(bytes: u64) -> String {
    if bytes % 1024 == 0 {
        format!("{} KiB", bytes / 1024)
    } else {
        format!("{bytes} bytes")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const REALISTIC_SYSV_HEX: &str = "\
target/thumbv7em-none-eabihf/release/oc-firmware  :
section               size         addr
.boot                0x2000   0x60000000
.stack               0x4000   0x20000000
.vector_table         0x2b8   0x20004000
.text                0x6964            0
.rodata               0x4c4   0x20200000
.data                 0x188   0x200042b8
.bss                    0x4   0x20004440
.uninit                   0   0x202004c4
.heap                0x4000   0x202004c4
.debug_info         0x49931            0
Total               0xc0ffc
";

    #[test]
    fn parses_sysv_hex_including_bare_zero_addresses() {
        let sections = parse_sysv(REALISTIC_SYSV_HEX).expect("parse");
        assert!(sections.iter().any(|s| s.name == ".text" && s.addr == 0));
        assert!(
            sections
                .iter()
                .any(|s| s.name == ".boot" && s.addr == 0x6000_0000 && s.size == 0x2000)
        );
        assert!(sections.iter().any(|s| s.name == ".debug_info"));
    }

    #[test]
    fn parses_sysv_decimal() {
        let output = "\
elf :
section size addr
.boot 8192 1610612736
.stack 16384 536870912
.vector_table 696 536887296
.text 100 0
";
        let sections = parse_sysv(output).expect("parse decimal");
        let boot = sections.iter().find(|s| s.name == ".boot").unwrap();
        assert_eq!(boot.addr, 0x6000_0000);
        assert_eq!(boot.size, 8192);
    }

    #[test]
    fn rejects_berkeley_format() {
        let berkeley = "\
   text\t   data\t    bss\t    dec\t    hex\tfilename
  53472\t    392\t  16388\t  70252\t  1126c\telf
";
        let err = parse_sysv(berkeley).expect_err("berkeley");
        assert!(err.contains("Berkeley"), "{err}");
    }

    #[test]
    fn realistic_layout_passes() {
        let sections = parse_sysv(REALISTIC_SYSV_HEX).unwrap();
        let report = check_layout(&sections);
        assert!(
            !report.failed(),
            "failures: {:?}",
            report
                .failures()
                .iter()
                .map(|i| format!("{}: {}", i.id, i.detail))
                .collect::<Vec<_>>()
        );
        assert!(
            report
                .items
                .iter()
                .any(|i| i.id == "stack-below-vector-table" && i.status == CheckStatus::Pass)
        );
    }

    #[test]
    fn stack_above_vector_table_fails() {
        let output = "\
section size addr
.boot 0x2000 0x60000000
.stack 0x4000 0x20005000
.vector_table 0x2b8 0x20004000
.text 0x100 0
";
        let report = check_layout(&parse_sysv(output).unwrap());
        assert!(report.failed());
        let item = report
            .items
            .iter()
            .find(|i| i.id == "stack-below-vector-table")
            .unwrap();
        assert_eq!(item.status, CheckStatus::Fail);
    }

    #[test]
    fn missing_required_section_fails() {
        let output = "\
section size addr
.boot 0x2000 0x60000000
.stack 0x4000 0x20000000
.text 0x100 0
";
        let report = check_layout(&parse_sysv(output).unwrap());
        assert!(report.failed());
        assert!(
            report
                .items
                .iter()
                .any(|i| i.id == "required-sections" && i.status == CheckStatus::Fail)
        );
    }

    #[test]
    fn selective_table_skips_debug_sections() {
        let sections = parse_sysv(REALISTIC_SYSV_HEX).unwrap();
        let table = format_section_table(&sections);
        assert!(table.contains(".boot"));
        assert!(table.contains(".stack"));
        assert!(!table.contains(".debug_info"));
        assert!(!table.contains("Total"));
    }

    #[test]
    fn checklist_surfaces_teensy40_limits() {
        let sections = parse_sysv(REALISTIC_SYSV_HEX).unwrap();
        let report = check_layout(&sections);
        let text = format_checklist(&report);

        // Address windows (exclusive ends).
        assert!(text.contains("[0x0, 0x80000)"), "{text}");
        assert!(text.contains("[0x20000000, 0x20080000)"), "{text}");
        assert!(text.contains("[0x20200000, 0x20280000)"), "{text}");
        assert!(text.contains("[0x60000000, 0x68000000)"), "{text}");

        // Usable / capacity budgets printed next to the measured values.
        assert!(text.contains("192 KiB"), "{text}");
        assert!(text.contains("320 KiB"), "{text}");
        assert!(text.contains("512 KiB"), "{text}");
        assert!(text.contains("1984 KiB"), "{text}");

        // Size-vs-budget phrasing for the tightest code budget.
        assert!(text.contains("size 0x6964 / ≤ 192 KiB usable"), "{text}");
    }

    #[test]
    fn checklist_is_columnar() {
        let sections = parse_sysv(REALISTIC_SYSV_HEX).unwrap();
        let report = check_layout(&sections);
        let text = format_checklist(&report);
        let mut lines = text.lines();

        assert_eq!(lines.next(), Some("layout checklist:"));
        let header = lines.next().expect("header row");
        assert!(
            header.starts_with("status") && header.contains("check") && header.contains("detail"),
            "{header}"
        );

        let check_col = header.find("check").expect("check column");
        let detail_col = header.find("detail").expect("detail column");

        let mut saw_body = false;
        for line in lines {
            if line.starts_with("result:") {
                break;
            }
            saw_body = true;
            assert!(line.len() > detail_col, "{line}");

            let status = line.split_whitespace().next().unwrap();
            assert!(
                matches!(status, "pass" | "warn" | "FAIL"),
                "unexpected status in {line}"
            );
            // `check` / `detail` columns start at the same indices as the header.
            assert!(
                !line[check_col..].starts_with(' '),
                "check column misaligned: {line}"
            );
            assert!(
                !line[detail_col..].starts_with(' '),
                "detail column misaligned: {line}"
            );
        }
        assert!(saw_body, "{text}");
    }
}
