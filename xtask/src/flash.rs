//! Flash orchestration: validate, confirm, and hand off to `teensy_loader_cli`.
//!
//! Keeping the loader invocation behind this module means every path to the
//! device — interactive, `--yes`, or `--dry-run` — runs the same gate.

use std::io::{self, BufRead, IsTerminal, Write};
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, bail};

use crate::paths::FirmwareArtifact;
use crate::{BuildArgs, cargo, llvm};
use xtask::validate::{self, ImageFacts, Rejection};

/// CLI arguments for `cargo xtask flash`.
#[derive(Debug, Clone, clap::Args)]
pub(crate) struct FlashArgs {
    #[command(flatten)]
    pub(crate) build: BuildArgs,

    /// Skip the interactive confirmation prompt.
    #[arg(long)]
    pub(crate) yes: bool,

    /// Run every check and print the planned loader invocation without flashing.
    #[arg(long)]
    pub(crate) dry_run: bool,
}

/// Builds, validates and (unless `--dry-run`) flashes the firmware.
pub(crate) fn flash(args: &FlashArgs) -> Result<()> {
    let artifact = {
        cargo::build_firmware(args.build.profile, &args.build.features)?;
        FirmwareArtifact::locate(args.build.profile)?
    };

    println!("firmware: {}", artifact.elf.display());

    if let Err(rejection) = validate::validate_target_path(&artifact.elf) {
        print_rejections(std::slice::from_ref(&rejection));
        bail!("pre-flash validation failed");
    }

    let bytes = std::fs::read(&artifact.elf)
        .with_context(|| format!("cannot read {}", artifact.elf.display()))?;

    let facts = match validate::validate_image(&bytes) {
        Ok(facts) => facts,
        Err(rejections) => {
            print_rejections(&rejections);
            bail!("pre-flash validation failed");
        }
    };

    print_facts(&artifact.elf, &facts);

    let hex = artifact.hex_path();
    llvm::objcopy_ihex(&artifact.elf, &hex)?;
    println!("hex: {}", hex.display());

    let hex_bytes =
        std::fs::read(&hex).with_context(|| format!("cannot read {}", hex.display()))?;
    let hex_digest = validate::sha256_digest(&hex_bytes);
    println!(
        "hex size: {} bytes\nhex sha256: {}",
        hex_bytes.len(),
        validate::hex_encode(&hex_digest)
    );

    warn_about_device();

    let loader = teensy_loader_cli_path();
    let loader_display = loader.as_ref().map_or_else(
        || "teensy_loader_cli".to_owned(),
        |p| p.display().to_string(),
    );

    if args.dry_run {
        println!(
            "dry-run: would invoke `{loader_display} --mcu=TEENSY40 -w -v {}`",
            hex.display()
        );
        if loader.is_none() {
            eprintln!(
                "warning: `teensy_loader_cli` was not found on PATH; install it before a real flash \
                 (on macOS: `brew install teensy_loader_cli`)"
            );
        }
        println!("dry-run: skipping confirmation and device upload");
        return Ok(());
    }

    let loader = loader.ok_or_else(|| {
        anyhow::anyhow!(
            "`teensy_loader_cli` not found on PATH. Install it and retry \
             (on macOS: `brew install teensy_loader_cli`)"
        )
    })?;

    confirm_flash(args.yes)?;

    invoke_loader(&loader, &hex)
}

/// Prints every rejection clearly to stderr.
fn print_rejections(rejections: &[Rejection]) {
    eprintln!(
        "pre-flash validation failed with {} problem(s):",
        rejections.len()
    );
    for (index, rejection) in rejections.iter().enumerate() {
        eprintln!("  {}. {rejection}", index + 1);
    }
}

/// Prints image facts and any soft warnings collected during validation.
fn print_facts(elf: &Path, facts: &ImageFacts) {
    for warning in &facts.warnings {
        eprintln!("warning: {warning}");
    }

    println!("image: {}", elf.display());
    println!("loadable size: {} bytes", facts.loadable_size);
    println!("entry: {:#010x}", facts.entry);
    if let Some(sp) = facts.initial_sp {
        println!("initial SP: {sp:#010x}");
    }
    if let Some(reset) = facts.reset_handler {
        println!("reset handler: {reset:#010x}");
    }
    println!("elf sha256: {}", facts.sha256_hex());
}

/// Emits a non-fatal reminder about the `HalfKay` PROGRAM button.
///
/// We deliberately avoid a USB/HID dependency; without one we cannot probe for
/// a Teensy, so the operator is always steered toward the button rather than
/// given a false "device found" signal.
fn warn_about_device() {
    eprintln!(
        "warning: cannot probe for a Teensy without a USB stack; \
         press the PROGRAM button on the board so `HalfKay` is listening before upload"
    );
}

/// Resolves `teensy_loader_cli` on `PATH`, if present.
fn teensy_loader_cli_path() -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path) {
        let candidate = dir.join("teensy_loader_cli");
        if candidate.is_file() {
            return Some(candidate);
        }
        // Windows-style name, in case someone cross-runs the tooling.
        let candidate_exe = dir.join("teensy_loader_cli.exe");
        if candidate_exe.is_file() {
            return Some(candidate_exe);
        }
    }
    None
}

/// Interactive `[y/N]` confirmation, or a hard refuse when stdin is not a TTY.
fn confirm_flash(yes: bool) -> Result<()> {
    if yes {
        return Ok(());
    }

    if !io::stdin().is_terminal() {
        bail!(
            "refusing to flash without confirmation: stdin is not a TTY. \
             Re-run with `--yes` to skip the prompt, or `--dry-run` to validate only"
        );
    }

    print!("Flash this image to the Teensy 4.0? [y/N] ");
    io::stdout()
        .flush()
        .context("cannot flush confirmation prompt")?;

    let mut line = String::new();
    io::stdin()
        .lock()
        .read_line(&mut line)
        .context("cannot read confirmation from stdin")?;

    let answer = line.trim();
    if answer.eq_ignore_ascii_case("y") {
        Ok(())
    } else {
        bail!("flash aborted by user");
    }
}

/// Runs `teensy_loader_cli --mcu=TEENSY40 -w -v <hex>`.
fn invoke_loader(loader: &Path, hex: &Path) -> Result<()> {
    let mut command = Command::new(loader);
    command.arg("--mcu=TEENSY40").arg("-w").arg("-v").arg(hex);
    cargo::run(command)
}
