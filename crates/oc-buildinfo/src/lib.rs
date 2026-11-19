//! Computes the build identifier shown under the version on every backend's
//! splash screen (journal/D-025).
//!
//! Used only from the build scripts of the three end crates (`oc-firmware`,
//! `oc-sim`, `oc-vcv-ffi`): each calls [`emit`], which sets the
//! `OC_BUILD_ID` compile-time environment variable to the short commit hash,
//! suffixed `-dirty` when a tracked file differs from that commit. Outside a
//! git checkout the identifier is empty and the splash shows the version
//! alone.
//!
//! The repository is read by [`vergen_gix`] (gitoxide, pure Rust): no `git`
//! binary is needed. vergen only reruns the build script when `HEAD` or the
//! current ref moves, so an edit would leave the identifier clean until the
//! next commit; `watch_sources` (unstaged source edits) and `watch_index`
//! (anything staged) close that gap.
//!
//! `oc-core` deliberately does not use this: it only displays the string it
//! is handed, so its tests and the golden simulator screens never depend on
//! the commit being built.

use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use vergen_gix::{Emitter, Gix};

/// Name of the compile-time environment variable [`emit`] sets.
pub const ENV_VAR: &str = "OC_BUILD_ID";

/// Suffix appended when tracked files differ from the commit.
pub const DIRTY_SUFFIX: &str = "-dirty";

/// What vergen reports in place of a value it could not compute (outside a
/// git checkout, for instance).
const VERGEN_DEFAULT: &str = "VERGEN_IDEMPOTENT_OUTPUT";

/// Sets `OC_BUILD_ID` for the calling crate and tells Cargo when to rerun.
///
/// Call it from `build.rs`; read the value with `env!("OC_BUILD_ID")`.
pub fn emit() {
    // An `OC_BUILD_ID` from the environment (a CI job, a reproducible build)
    // wins over git, so the identifier can be imposed rather than computed.
    println!("cargo:rerun-if-env-changed={ENV_VAR}");
    let id = env::var(ENV_VAR).unwrap_or_else(|_| compute());
    println!("cargo:rustc-env={ENV_VAR}={id}");
}

/// The identifier of the repository around the calling crate, or an empty
/// string if it cannot be read. Also emits the `rerun-if-changed` lines.
fn compute() -> String {
    // Short SHA, and dirty on tracked files only (untracked build artefacts
    // must not count), as `git describe --dirty` does.
    let gix = Gix::builder().sha(true).dirty(false).build();
    let emitted = Emitter::default()
        .quiet()
        .add_instructions(&gix)
        .and_then(|emitter| emitter.emit_and_set());
    if emitted.is_err() {
        return String::new();
    }

    if let Some(root) = workspace_root() {
        watch_sources(&root);
        watch_index(&root);
    }

    let (Ok(sha), Ok(dirty)) = (env::var("VERGEN_GIT_SHA"), env::var("VERGEN_GIT_DIRTY")) else {
        return String::new();
    };
    if sha.is_empty() || sha == VERGEN_DEFAULT {
        return String::new();
    }
    format_id(&sha, dirty == "true")
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

/// The nearest ancestor of the calling crate whose `Cargo.toml` declares a
/// `[workspace]`.
fn workspace_root() -> Option<PathBuf> {
    let manifest_dir = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR")?);
    manifest_dir.ancestors().find_map(|dir| {
        let manifest = fs::read_to_string(dir.join("Cargo.toml")).ok()?;
        manifest
            .lines()
            .any(|line| line.trim() == "[workspace]")
            .then(|| dir.to_path_buf())
    })
}

/// Reruns the build script when a source of the workspace changes, so an
/// edit that is not staged yet still turns the identifier `-dirty`.
///
/// Only paths that hold sources are listed, never a directory a build writes
/// into (`target/`, the plugin's `build/`, or the C header that `cargo xtask
/// vcv` copies next to the C++ sources), or every build would retrigger the
/// next one. Documentation edits are not watched here: they show as `-dirty`
/// from the next `git add` ([`watch_index`]).
fn watch_sources(root: &Path) {
    let mut paths = vec![
        root.join("Cargo.toml"),
        root.join("Cargo.lock"),
        root.join("rust-toolchain.toml"),
        root.join(".cargo/config.toml"),
    ];

    for package in subdirectories(&root.join("crates"))
        .into_iter()
        .chain([root.join("xtask")])
    {
        for entry in ["Cargo.toml", "build.rs", "src", "tests", "benches"] {
            paths.push(package.join(entry));
        }
    }

    let plugin = root.join("vcv/OrnamentCrimeAlambic");
    for entry in ["Makefile", "plugin.json", "res"] {
        paths.push(plugin.join(entry));
    }
    paths.extend(
        files(&plugin.join("src"))
            .into_iter()
            .filter(|file| !file.ends_with("oc_vcv_ffi.h")),
    );

    for path in paths.iter().filter(|path| path.exists()) {
        println!("cargo:rerun-if-changed={}", path.display());
    }
}

/// Reruns the build script when the git index changes, so a staged change
/// (`git add`, including documentation) turns the identifier `-dirty`;
/// vergen itself only watches `HEAD` and the current ref.
///
/// Found without the `git` binary: `.git` is a directory in a plain clone,
/// and a file holding `gitdir: <path>` in a worktree or submodule.
fn watch_index(root: &Path) {
    let dot_git = root.join(".git");
    let git_dir = if dot_git.is_dir() {
        Some(dot_git)
    } else {
        fs::read_to_string(&dot_git).ok().and_then(|text| {
            let path = PathBuf::from(text.trim().strip_prefix("gitdir:")?.trim());
            Some(if path.is_absolute() {
                path
            } else {
                root.join(path)
            })
        })
    };
    if let Some(index) = git_dir
        .map(|dir| dir.join("index"))
        .filter(|index| index.exists())
    {
        println!("cargo:rerun-if-changed={}", index.display());
    }
}

/// The subdirectories of `dir`, or none if it cannot be read.
fn subdirectories(dir: &Path) -> Vec<PathBuf> {
    entries(dir)
        .into_iter()
        .filter(|path| path.is_dir())
        .collect()
}

/// The regular files directly inside `dir`, or none if it cannot be read.
fn files(dir: &Path) -> Vec<PathBuf> {
    entries(dir)
        .into_iter()
        .filter(|path| path.is_file())
        .collect()
}

fn entries(dir: &Path) -> Vec<PathBuf> {
    let Ok(read) = fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut paths: Vec<PathBuf> = read.filter_map(|entry| Some(entry.ok()?.path())).collect();
    paths.sort();
    paths
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
