//! Invocation of nested `cargo` builds.

use std::process::Command;

use anyhow::{Context, Result, bail};

use crate::{FIRMWARE_PACKAGE, FIRMWARE_TARGET, Profile, paths};

/// Cross-compiles the firmware package for the Teensy 4.0 target.
///
/// `features` is forwarded as Cargo's `--features` (comma-joined), so callers
/// can select `oc-firmware` flags such as `ssd1306` / `ssd1309` without leaving
/// the xtask gate.
pub(crate) fn build_firmware(profile: Profile, features: &[String]) -> Result<()> {
    let mut command = Command::new(cargo_binary());
    command
        .current_dir(paths::workspace_root())
        .args(firmware_build_args(profile, features));

    run(command)
}

/// Arguments passed to a nested `cargo` for an `oc-firmware` cross-build.
///
/// Kept pure so the feature-forwarding shape can be unit-tested without
/// spawning Cargo.
pub(crate) fn firmware_build_args(profile: Profile, features: &[String]) -> Vec<String> {
    let mut args = vec![
        "build".to_owned(),
        "--package".to_owned(),
        FIRMWARE_PACKAGE.to_owned(),
        "--target".to_owned(),
        FIRMWARE_TARGET.to_owned(),
    ];

    if let Some(flag) = profile.cargo_flag() {
        args.push(flag.to_owned());
    }

    if !features.is_empty() {
        args.push("--features".to_owned());
        args.push(features.join(","));
    }

    args
}

/// The `cargo` executable currently driving the build.
fn cargo_binary() -> String {
    std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_owned())
}

/// Runs `command`, failing if it cannot be spawned or exits non-zero.
pub(crate) fn run(mut command: Command) -> Result<()> {
    let rendered = render(&command);
    let status = command
        .status()
        .with_context(|| format!("cannot run `{rendered}`"))?;

    if !status.success() {
        bail!("`{rendered}` failed with {status}");
    }

    Ok(())
}

/// Renders a command for diagnostics, without attempting shell quoting.
fn render(command: &Command) -> String {
    let mut rendered = command.get_program().to_string_lossy().into_owned();
    for arg in command.get_args() {
        rendered.push(' ');
        rendered.push_str(&arg.to_string_lossy());
    }
    rendered
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn release_build_without_features_matches_cargo_defaults() {
        assert_eq!(
            firmware_build_args(Profile::Release, &[]),
            [
                "build",
                "--package",
                FIRMWARE_PACKAGE,
                "--target",
                FIRMWARE_TARGET,
                "--release",
            ]
        );
    }

    #[test]
    fn debug_build_omits_release_flag() {
        assert_eq!(
            firmware_build_args(Profile::Debug, &[]),
            [
                "build",
                "--package",
                FIRMWARE_PACKAGE,
                "--target",
                FIRMWARE_TARGET,
            ]
        );
    }

    #[test]
    fn features_are_forwarded_comma_joined() {
        let features = ["ssd1306".to_owned(), "extra".to_owned()];
        assert_eq!(
            firmware_build_args(Profile::Release, &features),
            [
                "build",
                "--package",
                FIRMWARE_PACKAGE,
                "--target",
                FIRMWARE_TARGET,
                "--release",
                "--features",
                "ssd1306,extra",
            ]
        );
    }

    #[test]
    fn single_oled_feature_is_forwarded() {
        let features = ["ssd1309".to_owned()];
        let args = firmware_build_args(Profile::Release, &features);
        assert!(args.windows(2).any(|w| w == ["--features", "ssd1309"]));
    }
}
