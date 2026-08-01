//! Build and packaging automation for the Ornament & Crime firmware.
//!
//! `xtask` is the single supported entry point for everything that a bare
//! `cargo` invocation cannot express: cross-compiling the firmware for
//! `thumbv7em-none-eabihf`, reporting its memory footprint, producing the
//! Intel HEX image consumed by the Teensy loader, and flashing it under the
//! pre-flight checks in [`xtask::validate`].
//!
//! Run it through the workspace alias: `cargo xtask <command>`.

mod cargo;
mod flash;
mod llvm;
mod paths;

use anyhow::Result;
use clap::{Parser, Subcommand, ValueEnum};

use crate::flash::FlashArgs;
use crate::paths::FirmwareArtifact;

/// Compilation target of the firmware binary.
pub(crate) const FIRMWARE_TARGET: &str = xtask::FIRMWARE_TARGET;

/// Cargo package name of the firmware binary.
pub(crate) const FIRMWARE_PACKAGE: &str = xtask::FIRMWARE_PACKAGE;

#[derive(Debug, Parser)]
#[command(
    name = "xtask",
    about = "Build, inspect, package and flash the Ornament & Crime firmware",
    version
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Cross-compile the firmware for the Teensy 4.0.
    Build(BuildArgs),
    /// Report the flash and RAM footprint of the firmware.
    Size(BuildArgs),
    /// Produce the Intel HEX image under `dist/`.
    Hex(BuildArgs),
    /// Validate and flash the firmware to a Teensy 4.0.
    Flash(FlashArgs),
}

#[derive(Debug, Clone, clap::Args)]
pub(crate) struct BuildArgs {
    /// Cargo profile to build with.
    #[arg(long, value_enum, default_value_t = Profile::Release)]
    profile: Profile,
}

/// Cargo profile used for firmware builds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub(crate) enum Profile {
    /// Optimised build; the only profile that should ever reach hardware.
    Release,
    /// Unoptimised build, useful with a debug probe.
    Debug,
}

impl Profile {
    /// Directory name Cargo uses for this profile inside the target directory.
    pub(crate) fn dir_name(self) -> &'static str {
        match self {
            Profile::Release => "release",
            Profile::Debug => "debug",
        }
    }

    /// Extra Cargo flag selecting this profile, if any.
    pub(crate) fn cargo_flag(self) -> Option<&'static str> {
        match self {
            Profile::Release => Some("--release"),
            Profile::Debug => None,
        }
    }
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Command::Build(args) => {
            let artifact = build_firmware(&args)?;
            println!("firmware: {}", artifact.elf.display());
        }
        Command::Size(args) => {
            let artifact = build_firmware(&args)?;
            llvm::report_size(&artifact.elf)?;
        }
        Command::Hex(args) => {
            let artifact = build_firmware(&args)?;
            let hex = artifact.hex_path();
            llvm::objcopy_ihex(&artifact.elf, &hex)?;
            println!("hex: {}", hex.display());
        }
        Command::Flash(args) => flash::flash(&args)?,
    }

    Ok(())
}

/// Cross-compiles the firmware and returns the resulting artifact paths.
fn build_firmware(args: &BuildArgs) -> Result<FirmwareArtifact> {
    cargo::build_firmware(args.profile)?;
    FirmwareArtifact::locate(args.profile)
}
