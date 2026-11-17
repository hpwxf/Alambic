//! Builds the browser probe and flasher into `dist/web/` (journal/D-031).
//!
//! The page needs no JavaScript tooling: `oc-web` is compiled for
//! `wasm32-unknown-unknown`, the `wasm-bindgen` command line generates the
//! JavaScript glue next to it, and the static files of `web/` are copied
//! alongside. The glue only works with the exact `wasm-bindgen` version the
//! crate was built against, so that version is checked against `Cargo.lock`
//! before anything runs, rather than left to fail inside the browser.

use std::fs;
use std::path::Path;
use std::process::Command;

use anyhow::{Context, Result, bail};

use crate::{cargo, paths};

/// Cargo package name of the WebAssembly core.
const WEB_PACKAGE: &str = "oc-web";

/// Target the core is compiled for (`rust-toolchain.toml` provisions it).
const WEB_TARGET: &str = "wasm32-unknown-unknown";

/// Static files of the page, relative to the workspace root.
const WEB_DIR: &str = "web";

/// Builds the page into `dist/web/`.
pub(crate) fn build() -> Result<()> {
    let root = paths::workspace_root();
    let lock = fs::read_to_string(root.join("Cargo.lock")).context("cannot read Cargo.lock")?;
    let wanted = locked_version(&lock, "wasm-bindgen")
        .context("Cargo.lock has no wasm-bindgen package: is oc-web in the workspace?")?;
    check_bindgen_cli(&wanted)?;

    let mut command = Command::new(cargo::cargo_binary());
    command.current_dir(root).args([
        "build",
        "--package",
        WEB_PACKAGE,
        "--target",
        WEB_TARGET,
        "--release",
    ]);
    cargo::run(command)?;

    let wasm = paths::target_dir()
        .join(WEB_TARGET)
        .join("release")
        .join(format!("{}.wasm", WEB_PACKAGE.replace('-', "_")));
    let out = paths::dist_dir().join("web");
    if out.exists() {
        fs::remove_dir_all(&out).with_context(|| format!("cannot clear {}", out.display()))?;
    }
    fs::create_dir_all(&out).with_context(|| format!("cannot create {}", out.display()))?;

    let mut bindgen = Command::new("wasm-bindgen");
    bindgen
        .arg("--target")
        .arg("web")
        .arg("--no-typescript")
        .arg("--out-dir")
        .arg(&out)
        .arg(&wasm);
    cargo::run(bindgen)?;

    copy_static(&root.join(WEB_DIR), &out)?;
    println!("page: {}", out.display());
    println!(
        "serve it from a secure context, e.g. `python3 -m http.server -d {} 8000`, \
         then open http://localhost:8000 in Chrome or Edge",
        out.display()
    );
    Ok(())
}

/// Fails with an install hint unless `wasm-bindgen --version` is `wanted`.
fn check_bindgen_cli(wanted: &str) -> Result<()> {
    let hint = format!("install it with `cargo install wasm-bindgen-cli --version {wanted}`");
    let output = Command::new("wasm-bindgen")
        .arg("--version")
        .output()
        .with_context(|| format!("`wasm-bindgen` not found on PATH; {hint}"))?;
    let reported = String::from_utf8_lossy(&output.stdout);
    match cli_version(&reported) {
        Some(found) if found == wanted => Ok(()),
        Some(found) => bail!("`wasm-bindgen` is {found}, oc-web is built against {wanted}; {hint}"),
        None => bail!("cannot read the version of `wasm-bindgen` from {reported:?}; {hint}"),
    }
}

/// Copies every file of `from` (not its subdirectories) into `to`.
fn copy_static(from: &Path, to: &Path) -> Result<()> {
    let entries = fs::read_dir(from).with_context(|| format!("cannot read {}", from.display()))?;
    for entry in entries {
        let entry = entry?;
        if entry.file_type()?.is_file() {
            let target = to.join(entry.file_name());
            fs::copy(entry.path(), &target)
                .with_context(|| format!("cannot copy to {}", target.display()))?;
        }
    }
    Ok(())
}

/// The version `Cargo.lock` records for `package`, from the line following
/// its `name = "…"` line.
fn locked_version(lock: &str, package: &str) -> Option<String> {
    let name = format!("name = \"{package}\"");
    let mut lines = lock.lines();
    lines.find(|line| line.trim() == name)?;
    let version = lines.next()?.trim().strip_prefix("version = \"")?;
    Some(version.strip_suffix('"')?.to_owned())
}

/// The version in `wasm-bindgen --version`'s output (`wasm-bindgen 0.2.x`).
fn cli_version(output: &str) -> Option<&str> {
    let mut words = output.split_whitespace();
    (words.next()? == "wasm-bindgen").then_some(())?;
    words.next()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_locked_version_is_the_line_after_the_name() {
        let lock = "[[package]]\nname = \"wasm-bindgen-shared\"\nversion = \"9.9.9\"\n\n\
                    [[package]]\nname = \"wasm-bindgen\"\nversion = \"0.2.129\"\n";
        assert_eq!(
            locked_version(lock, "wasm-bindgen").as_deref(),
            Some("0.2.129")
        );
        assert_eq!(locked_version(lock, "serde"), None);
    }

    #[test]
    fn the_cli_version_is_read_from_its_banner() {
        assert_eq!(cli_version("wasm-bindgen 0.2.129\n"), Some("0.2.129"));
        assert_eq!(cli_version("something else 1.0"), None);
        assert_eq!(cli_version(""), None);
    }
}
