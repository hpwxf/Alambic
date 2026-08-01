//! Access to the LLVM binary utilities shipped with the `llvm-tools` component.
//!
//! Using the toolchain-provided tools rather than a system `objcopy` keeps the
//! build reproducible and avoids the classic "wrong binutils for this target"
//! failure mode.

use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, bail};

use crate::{cargo, paths};

/// Prints the section footprint of `elf` using `llvm-size`.
pub(crate) fn report_size(elf: &Path) -> Result<()> {
    let mut command = Command::new(tool("llvm-size")?);
    command.arg("-A").arg("-d").arg(elf);
    cargo::run(command)
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
