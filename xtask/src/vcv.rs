//! Builds and installs the VCV Rack 2 plugin on top of the `oc-vcv-ffi`
//! staticlib.
//!
//! This module deliberately does none of the C++ compilation itself: the
//! Rack SDK's own `Makefile` framework (`$(RACK_DIR)/plugin.mk`) already
//! knows how to compile, link, package and install a plugin correctly for
//! the host platform. What is missing without this module is keeping that
//! `Makefile` fed with a freshly built Rust artefact and its C header,
//! which is exactly the point of the plan's requirement that "toute la
//! chaîne reste pilotée par `cargo`": a contributor never has to remember to
//! rebuild `oc-vcv-ffi` or copy its header before calling `make` by hand.

use std::env;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, bail};

use crate::{Profile, cargo, paths};

/// Cargo package name of the ABI crate linked into the plugin.
const VCV_FFI_PACKAGE: &str = "oc-vcv-ffi";

/// Location of the plugin sources, relative to the workspace root.
const VCV_PLUGIN_DIR: &str = "vcv/OrnamentCrimeRust";

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
    println!("installed OrnamentCrimeRust into the Rack user plugin directory");
    Ok(())
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
/// `#include "oc_vcv_ffi.h"` in `Diagnostic.cpp` resolves. Returns the
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
    use super::{VcvArgs, resolve_rack_dir, staticlib_file_name};
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
}
