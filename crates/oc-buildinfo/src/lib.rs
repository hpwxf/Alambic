//! Computes the build identifier shown under the version on every backend's
//! splash screen (journal/D-025).
//!
//! Used only from the build scripts of the three end crates (`oc-firmware`,
//! `oc-sim`, `oc-vcv-ffi`): each calls [`emit`], which sets the
//! `OC_BUILD_ID` compile-time environment variable to the short commit hash,
//! suffixed `-dirty` when a tracked file differs from that commit. Outside a
//! git checkout, or without `git` on `PATH`, the identifier is empty and the
//! splash shows the version alone.
//!
//! `oc-core` deliberately does not use this: it only displays the string it
//! is handed, so its tests and the golden simulator screens never depend on
//! the commit being built.

use std::path::{Path, PathBuf};
use std::process::Command;

/// Name of the compile-time environment variable [`emit`] sets.
pub const ENV_VAR: &str = "OC_BUILD_ID";

/// Suffix appended when tracked files differ from the commit.
pub const DIRTY_SUFFIX: &str = "-dirty";

/// Sets `OC_BUILD_ID` for the calling crate and tells Cargo when to rerun.
///
/// Call it from `build.rs`; read the value with `env!("OC_BUILD_ID")`.
pub fn emit() {
    // `OC_BUILD_ID` set by the caller (a CI job, a reproducible build) wins
    // over git, so the identifier can be imposed rather than computed.
    println!("cargo:rerun-if-env-changed={ENV_VAR}");
    let id = std::env::var(ENV_VAR).unwrap_or_else(|_| compute());
    println!("cargo:rustc-env={ENV_VAR}={id}");
}

/// The identifier of the working tree around the calling crate, or an empty
/// string outside a git checkout. Also emits the `rerun-if-changed` lines.
fn compute() -> String {
    let Some(git_dir) = git(&["rev-parse", "--absolute-git-dir"]).map(PathBuf::from) else {
        return String::new();
    };
    watch_git(&git_dir);
    watch_tracked_files();

    let Some(hash) = git(&["rev-parse", "--short", "HEAD"]) else {
        return String::new();
    };
    let dirty = git(&["status", "--porcelain", "--untracked-files=no"])
        .is_some_and(|status| !status.is_empty());
    format_id(&hash, dirty)
}

/// `hash`, or `hash-dirty`.
#[must_use]
pub fn format_id(hash: &str, dirty: bool) -> String {
    if dirty {
        format!("{hash}{DIRTY_SUFFIX}")
    } else {
        hash.to_owned()
    }
}

/// Reruns the build script when HEAD moves, the index changes or a ref is
/// updated.
fn watch_git(git_dir: &Path) {
    for name in ["HEAD", "index", "packed-refs", "refs"] {
        let path = git_dir.join(name);
        if path.exists() {
            println!("cargo:rerun-if-changed={}", path.display());
        }
    }
}

/// Reruns the build script when any tracked file changes, so an edit that is
/// not staged yet still turns the identifier `-dirty`. Untracked build
/// artefacts (`target/`, the plugin's `build/`) are not listed, so building
/// does not retrigger itself.
fn watch_tracked_files() {
    let (Some(top), Some(files)) = (
        git(&["rev-parse", "--show-toplevel"]),
        git(&["ls-files", "-z"]),
    ) else {
        return;
    };
    let top = PathBuf::from(top);
    for file in files.split('\0').filter(|file| !file.is_empty()) {
        println!("cargo:rerun-if-changed={}", top.join(file).display());
    }
}

/// Runs `git` with `arguments` and returns its trimmed standard output, or
/// `None` if git is missing or fails.
fn git(arguments: &[&str]) -> Option<String> {
    let output = Command::new("git")
        .args(arguments)
        // Without this, `git status` opportunistically rewrites `.git/index`,
        // which would make the next build rerun this script forever.
        .env("GIT_OPTIONAL_LOCKS", "0")
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8(output.stdout).ok()?;
    Some(text.trim().to_owned())
}

#[cfg(test)]
mod tests {
    use super::{DIRTY_SUFFIX, format_id};

    #[test]
    fn a_clean_tree_shows_the_hash_alone() {
        assert_eq!(format_id("a1b2c3d", false), "a1b2c3d");
    }

    #[test]
    fn a_dirty_tree_is_flagged() {
        assert_eq!(format_id("a1b2c3d", true), format!("a1b2c3d{DIRTY_SUFFIX}"));
    }
}
