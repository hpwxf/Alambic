//! Builds, installs and cleans the VCV Rack 2 plugin on top of the
//! `oc-vcv-ffi` staticlib.
//!
//! This module deliberately does none of the C++ compilation itself: the
//! Rack SDK's own `Makefile` framework (`$(RACK_DIR)/plugin.mk`) already
//! knows how to compile, link, package and install a plugin correctly for
//! the host platform. What is missing without this module is keeping that
//! `Makefile` fed with a freshly built Rust artefact and its C header,
//! which is exactly the point of the plan's requirement that "toute la
//! chaîne reste pilotée par `cargo`": a contributor never has to remember to
//! rebuild `oc-vcv-ffi` or copy its header before calling `make` by hand.
//!
//! `vcv clean` is the exception that does not call `make`: the C++ side
//! sometimes keeps stale object files and a previously linked `plugin.*`
//! around after a failed or partial rebuild, and wiping those plus the host
//! `oc-vcv-ffi` artefacts is more reliable when done directly than through
//! the SDK's `clean` target (which also requires a configured `RACK_DIR`).

use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, bail};

use crate::{Profile, cargo, paths};

/// Cargo package name of the ABI crate linked into the plugin.
const VCV_FFI_PACKAGE: &str = "oc-vcv-ffi";

/// Location of the plugin sources, relative to the workspace root.
const VCV_PLUGIN_DIR: &str = "vcv/OrnamentCrimeAlambic";

/// CLI arguments shared by `vcv build` and `vcv install`.
#[derive(Debug, Clone, clap::Args)]
pub(crate) struct VcvArgs {
    /// Cargo profile used to build the `oc-vcv-ffi` staticlib.
    ///
    /// Defaults to `release`: nothing about the plugin benefits from a debug
    /// build, and a release staticlib is what should ever reach a
    /// distributed `.vcvplugin`.
    #[arg(long, value_enum, default_value_t = Profile::Release)]
    pub(crate) profile: Profile,

    /// Absolute path to an extracted Rack SDK.
    ///
    /// Falls back to the `RACK_DIR` environment variable, matching the
    /// Rack SDK's own convention (see the Plugin Development Tutorial at
    /// <https://vcvrack.com/manual/PluginDevelopmentTutorial>). Neither
    /// being set is a hard error: there is no sensible default location to
    /// guess.
    #[arg(long)]
    pub(crate) rack_dir: Option<PathBuf>,
}

/// Builds the `oc-vcv-ffi` staticlib and header, then the plugin's native
/// binary (`plugin.dylib`/`.so`/`.dll`), without installing it into Rack.
pub(crate) fn build(args: &VcvArgs) -> Result<()> {
    let rack_dir = resolve_rack_dir(args)?;
    let staticlib = build_ffi_and_copy_header(args.profile)?;
    run_make(&rack_dir, &staticlib, "all")?;

    println!(
        "plugin: {}",
        paths::workspace_root()
            .join(VCV_PLUGIN_DIR)
            .join(plugin_binary_name())
            .display()
    );
    Ok(())
}

/// Builds the plugin as [`build`] does, then installs it into the current
/// user's Rack plugin directory via the Rack SDK's own `install` target
/// (`$(RACK_USER_DIR)/plugins-<os>-<cpu>`), which already accounts for the
/// platform-specific location so this module does not have to.
pub(crate) fn install(args: &VcvArgs) -> Result<()> {
    let rack_dir = resolve_rack_dir(args)?;
    let staticlib = build_ffi_and_copy_header(args.profile)?;
    run_make(&rack_dir, &staticlib, "install")?;
    println!("installed OrnamentCrimeAlambic into the Rack user plugin directory");
    Ok(())
}

/// Removes every generated artefact involved in a VCV plugin build so the
/// next `vcv build` starts from a clean tree.
///
/// This deliberately does **not** require `--rack-dir` / `RACK_DIR`: stale
/// C++ objects are exactly what you want to wipe when the SDK path is
/// broken or when a previous build left half-linked leftovers. It covers:
///
/// * the Rack-side directories and binaries (`build/`, `dep/`, `dist/`,
///   `plugin.{dylib,so,dll}`) that `make clean` would remove;
/// * the header copied next to the plugin sources by
///   [`build_ffi_and_copy_header`];
/// * the cbindgen header under `crates/oc-vcv-ffi/include/`;
/// * Cargo's host artefacts for the `oc-vcv-ffi` package itself.
pub(crate) fn clean() -> Result<()> {
    let root = paths::workspace_root();
    let plugin_dir = root.join(VCV_PLUGIN_DIR);

    for name in plugin_clean_dirs() {
        remove_path_if_exists(&plugin_dir.join(name))?;
    }
    for name in plugin_binary_names() {
        remove_path_if_exists(&plugin_dir.join(name))?;
    }
    remove_path_if_exists(&plugin_dir.join("src").join("oc_vcv_ffi.h"))?;
    remove_path_if_exists(&root.join("crates").join(VCV_FFI_PACKAGE).join("include"))?;

    clean_vcv_ffi()?;

    println!("cleaned VCV plugin and `{VCV_FFI_PACKAGE}` host artefacts");
    Ok(())
}

/// Directory names under the plugin tree that a full clean must remove.
///
/// Kept as a function so the unit tests can pin the set without reaching
/// into the clean body.
fn plugin_clean_dirs() -> &'static [&'static str] {
    // Mirrors the Rack SDK's `clean` target (`build`, `dist`) plus `dep`,
    // which holds intermediate dependency stamps and is otherwise left
    // behind by a plain `make clean`.
    &["build", "dep", "dist"]
}

/// Every platform-specific name the Rack SDK may give the linked plugin.
///
/// Cleaning all three keeps a cross-compiled or accidentally-renamed
/// leftover from surviving a host-only clean.
fn plugin_binary_names() -> &'static [&'static str] {
    &["plugin.dylib", "plugin.so", "plugin.dll"]
}

/// Deletes `path` if it exists (file or directory); no-ops when absent.
fn remove_path_if_exists(path: &Path) -> Result<()> {
    if !path.exists() {
        return Ok(());
    }

    let metadata =
        fs::symlink_metadata(path).with_context(|| format!("cannot inspect {}", path.display()))?;
    if metadata.is_dir() {
        fs::remove_dir_all(path)
            .with_context(|| format!("cannot remove directory {}", path.display()))?;
    } else {
        fs::remove_file(path).with_context(|| format!("cannot remove file {}", path.display()))?;
    }
    println!("removed {}", path.display());
    Ok(())
}

/// Runs `cargo clean -p oc-vcv-ffi` for every host profile we care about.
///
/// `cargo clean -p <pkg>` without `--release` only wipes the *dev* profile.
/// `vcv build` defaults to release, so cleaning dev alone leaves the release
/// staticlib and its build-script fingerprint intact: the next build is a
/// no-op, `build.rs` never re-runs, and the cbindgen header we just deleted
/// under `include/` is not regenerated. Cleaning both profiles forces a
/// real rebuild (and header rewrite) on the next `vcv build`/`vcv install`.
fn clean_vcv_ffi() -> Result<()> {
    for extra in vcv_ffi_clean_profile_args() {
        let mut command = Command::new(cargo_binary());
        command
            .current_dir(paths::workspace_root())
            .arg("clean")
            .arg("--package")
            .arg(VCV_FFI_PACKAGE);
        command.args(*extra);
        cargo::run(command)?;
    }
    Ok(())
}

/// Extra `cargo clean` arguments, one entry per host profile to wipe.
///
/// The empty slice is the default (dev) profile; `--release` is required
/// separately because Cargo does not clean release artefacts unless asked.
fn vcv_ffi_clean_profile_args() -> &'static [&'static [&'static str]] {
    &[&[], &["--release"]]
}

/// Resolves the Rack SDK directory from `--rack-dir` or `RACK_DIR`.
fn resolve_rack_dir(args: &VcvArgs) -> Result<PathBuf> {
    if let Some(dir) = &args.rack_dir {
        return Ok(dir.clone());
    }

    if let Some(dir) = env::var_os("RACK_DIR") {
        return Ok(PathBuf::from(dir));
    }

    bail!(
        "no Rack SDK found: pass `--rack-dir <path>` or set the `RACK_DIR` environment variable. \
         Download the SDK matching your platform from https://vcvrack.com/downloads and extract it, \
         then point `RACK_DIR` at the extracted folder"
    );
}

/// Cross-compiles nothing (this is a host build): builds `oc-vcv-ffi` for
/// the host, then copies its generated header next to the plugin sources so
/// `#include "oc_vcv_ffi.h"` in `Alambic.cpp` resolves. Returns the
/// staticlib's absolute path.
fn build_ffi_and_copy_header(profile: Profile) -> Result<PathBuf> {
    build_vcv_ffi(profile)?;

    let header_src = paths::workspace_root()
        .join("crates")
        .join(VCV_FFI_PACKAGE)
        .join("include")
        .join("oc_vcv_ffi.h");
    let header_dst = paths::workspace_root()
        .join(VCV_PLUGIN_DIR)
        .join("src")
        .join("oc_vcv_ffi.h");

    if !header_src.is_file() {
        bail!(
            "expected cbindgen to have generated {} while building `{VCV_FFI_PACKAGE}`",
            header_src.display()
        );
    }
    std::fs::copy(&header_src, &header_dst).with_context(|| {
        format!(
            "cannot copy {} to {}",
            header_src.display(),
            header_dst.display()
        )
    })?;

    let staticlib = paths::target_dir()
        .join(profile.dir_name())
        .join(staticlib_file_name());
    if !staticlib.is_file() {
        bail!(
            "expected `cargo build -p {VCV_FFI_PACKAGE}` to produce {}",
            staticlib.display()
        );
    }
    Ok(staticlib)
}

/// Builds `oc-vcv-ffi` for the host (no `--target`: this staticlib links
/// into a plugin that runs on the developer's own machine, unlike the
/// Teensy firmware).
fn build_vcv_ffi(profile: Profile) -> Result<()> {
    let mut command = Command::new(cargo_binary());
    command
        .current_dir(paths::workspace_root())
        .arg("build")
        .arg("--package")
        .arg(VCV_FFI_PACKAGE);

    if let Some(flag) = profile.cargo_flag() {
        command.arg(flag);
    }

    cargo::run(command)
}

/// The `cargo` executable currently driving the build.
fn cargo_binary() -> String {
    env::var("CARGO").unwrap_or_else(|_| "cargo".to_owned())
}

/// File name Cargo gives the `oc-vcv-ffi` staticlib on this host platform.
///
/// Only Unix-like naming (`lib*.a`) is implemented; the project's own CI and
/// documented workflow only cover Linux and macOS (see `TESTING.md`), and a
/// Windows toolchain names this differently (`oc_vcv_ffi.lib` under MSVC).
fn staticlib_file_name() -> String {
    format!("lib{}.a", VCV_FFI_PACKAGE.replace('-', "_"))
}

/// File name Rack's own `plugin.mk` gives the built plugin, per platform.
fn plugin_binary_name() -> &'static str {
    if cfg!(target_os = "macos") {
        "plugin.dylib"
    } else if cfg!(target_os = "windows") {
        "plugin.dll"
    } else {
        "plugin.so"
    }
}

/// Runs `make <target>` inside the plugin directory, pointing it at
/// `rack_dir` and the freshly built staticlib.
///
/// `OC_VCV_FFI_LIB` overrides the `Makefile`'s own default (a relative guess
/// from a plain `make` invocation), so this works regardless of
/// `CARGO_TARGET_DIR` or the chosen profile.
fn run_make(rack_dir: &Path, staticlib: &Path, target: &str) -> Result<()> {
    if which("make").is_none() {
        bail!(
            "`make` was not found on PATH. Install platform build tools \
             (on macOS: Xcode Command Line Tools via `xcode-select --install`; \
             on Linux: your distribution's `build-essential`/`base-devel` package)"
        );
    }

    if !rack_dir.is_dir() {
        bail!(
            "RACK_DIR does not point at a directory: {}",
            rack_dir.display()
        );
    }

    let mut command = Command::new("make");
    command
        .current_dir(paths::workspace_root().join(VCV_PLUGIN_DIR))
        .arg(target)
        .env("RACK_DIR", rack_dir)
        .env("OC_VCV_FFI_LIB", staticlib);

    cargo::run(command)
}

/// Resolves `binary` on `PATH`, if present.
fn which(binary: &str) -> Option<PathBuf> {
    let path = env::var_os("PATH")?;
    env::split_paths(&path)
        .map(|dir| dir.join(binary))
        .find(|candidate| candidate.is_file())
}

#[cfg(test)]
mod tests {
    use super::{
        VcvArgs, plugin_binary_names, plugin_clean_dirs, resolve_rack_dir, staticlib_file_name,
        vcv_ffi_clean_profile_args,
    };
    use crate::Profile;

    #[test]
    fn staticlib_file_name_uses_underscores_not_hyphens() {
        // Cargo always underscores a package's hyphenated name when naming
        // the artefact it produces; a stray hyphen here would silently make
        // every lookup miss.
        assert_eq!(staticlib_file_name(), "liboc_vcv_ffi.a");
    }

    #[test]
    fn explicit_rack_dir_wins_over_the_environment() {
        let args = VcvArgs {
            profile: Profile::Release,
            rack_dir: Some("/explicit/path".into()),
        };
        let resolved = resolve_rack_dir(&args).expect("an explicit path always resolves");
        assert_eq!(resolved, std::path::Path::new("/explicit/path"));
    }

    #[test]
    fn clean_removes_every_rack_side_directory_make_would() {
        // `make clean` drops `build` and `dist`; we also drop `dep`, which
        // the SDK leaves alone. Losing any of these from the set would let
        // a stale intermediate survive `vcv clean`.
        assert_eq!(plugin_clean_dirs(), &["build", "dep", "dist"]);
    }

    #[test]
    fn clean_covers_every_host_plugin_binary_name() {
        // A macOS-only clean would leave a Linux CI leftover (or vice
        // versa) if someone checked a binary in by mistake; pin the full
        // set so a platform-gated trim is a deliberate edit.
        assert_eq!(
            plugin_binary_names(),
            &["plugin.dylib", "plugin.so", "plugin.dll"]
        );
    }

    #[test]
    fn clean_wipes_both_dev_and_release_ffi_artefacts() {
        // `cargo clean -p` defaults to dev only; without an explicit
        // `--release` pass the release staticlib (and its build.rs
        // fingerprint) survives, so the next `vcv build` never regenerates
        // the cbindgen header we deleted under `include/`.
        assert_eq!(vcv_ffi_clean_profile_args(), &[&[][..], &["--release"][..]]);
    }
}
