//! Invocation of nested `cargo` builds.

use std::process::Command;

use anyhow::{Context, Result, bail};

use crate::{FIRMWARE_PACKAGE, FIRMWARE_TARGET, Profile, paths};

/// Cross-compiles the firmware package for the Teensy 4.0 target.
pub(crate) fn build_firmware(profile: Profile) -> Result<()> {
    let mut command = Command::new(cargo_binary());
    command
        .current_dir(paths::workspace_root())
        .arg("build")
        .arg("--package")
        .arg(FIRMWARE_PACKAGE)
        .arg("--target")
        .arg(FIRMWARE_TARGET);

    if let Some(flag) = profile.cargo_flag() {
        command.arg(flag);
    }

    run(command)
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
