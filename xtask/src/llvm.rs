//! Access to the LLVM binary utilities shipped with the `llvm-tools` component.
//!
//! Using the toolchain-provided tools rather than a system `objcopy` keeps the
//! build reproducible and avoids the classic "wrong binutils for this target"
//! failure mode.

use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, bail};

use crate::{cargo, paths};
use xtask::size_check;

/// Stable `llvm-size` flags: long option names, `SysV` table, hex radix.
///
/// Short aliases (`-A`, `-x`) exist today but have moved before; the long form
/// is what `xtask::size_check` is written against.
const LLVM_SIZE_ARGS: [&str; 2] = ["--format=sysv", "--radix=16"];

/// Prints a selective section table and runs the layout checklist on `elf`.
pub(crate) fn report_size(elf: &Path) -> Result<()> {
    let output = Command::new(tool("llvm-size")?)
        .args(LLVM_SIZE_ARGS)
        .arg(elf)
        .output()
        .with_context(|| format!("cannot run llvm-size on {}", elf.display()))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!("llvm-size failed with {}: {}", output.status, stderr.trim());
    }

    let stdout = String::from_utf8(output.stdout).context("llvm-size wrote non-UTF-8 output")?;
    if !output.stderr.is_empty() {
        eprint!("{}", String::from_utf8_lossy(&output.stderr));
    }

    println!("{}:", elf.display());
    let sections = size_check::parse_sysv(&stdout).map_err(|message| anyhow::anyhow!(message))?;
    println!("{}", size_check::format_section_table(&sections));
    println!();
    // Flash loadable size is validated by `cargo xtask flash --dry-run`; this
    // command is about section placement, not the naive llvm-size Total line.
    println!(
        "note: debug sections omitted; flash loadable size is reported by `cargo xtask flash --dry-run`"
    );
    println!();

    let report = size_check::check_layout(&sections);
    println!("{}", size_check::format_checklist(&report));

    if report.failed() {
        let failed = report
            .failures()
            .iter()
            .map(|item| item.id)
            .collect::<Vec<_>>()
            .join(", ");
        bail!("firmware layout checklist failed ({failed})");
    }

    Ok(())
}

/// Converts `elf` into an Intel HEX image at `destination`.
pub(crate) fn objcopy_ihex(elf: &Path, destination: &Path) -> Result<()> {
    paths::ensure_dist_dir()?;

    let mut command = Command::new(tool("llvm-objcopy")?);
    command
        .arg("--output-target")
        .arg("ihex")
        .arg(elf)
        .arg(destination);
    cargo::run(command)
}

/// Locates an LLVM tool inside the active Rust sysroot.
fn tool(name: &str) -> Result<PathBuf> {
    let sysroot = sysroot()?;
    let bin_dirs = std::fs::read_dir(sysroot.join("lib/rustlib"))
        .context("cannot enumerate the Rust sysroot; is `llvm-tools` installed?")?;

    for entry in bin_dirs {
        let candidate = entry?.path().join("bin").join(name);
        if candidate.is_file() {
            return Ok(candidate);
        }
    }

    bail!("`{name}` not found in the Rust sysroot; run `rustup component add llvm-tools`")
}

/// Absolute path of the sysroot of the active toolchain.
fn sysroot() -> Result<PathBuf> {
    let output = Command::new("rustc")
        .arg("--print")
        .arg("sysroot")
        .output()
        .context("cannot run `rustc --print sysroot`")?;

    if !output.status.success() {
        bail!("`rustc --print sysroot` failed with {}", output.status);
    }

    let path = String::from_utf8(output.stdout)
        .context("`rustc --print sysroot` returned non-UTF-8 output")?;
    Ok(PathBuf::from(path.trim()))
}
