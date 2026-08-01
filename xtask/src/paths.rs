//! Workspace path resolution for build artifacts.

use std::ffi::OsStr;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};

use crate::{FIRMWARE_PACKAGE, FIRMWARE_TARGET, Profile};

/// Paths of a successfully built firmware image.
#[derive(Debug, Clone)]
pub(crate) struct FirmwareArtifact {
    /// The linked ELF file produced by Cargo.
    pub(crate) elf: PathBuf,
}

impl FirmwareArtifact {
    /// Resolves the ELF produced for `profile`, failing if it is missing.
    pub(crate) fn locate(profile: Profile) -> Result<Self> {
        let elf = target_dir()
            .join(FIRMWARE_TARGET)
            .join(profile.dir_name())
            .join(FIRMWARE_PACKAGE);

        if !elf.is_file() {
            bail!("firmware ELF not found at {}", elf.display());
        }

        Ok(Self { elf })
    }

    /// Destination of the Intel HEX image for this artifact.
    pub(crate) fn hex_path(&self) -> PathBuf {
        let stem = self
            .elf
            .file_stem()
            .unwrap_or_else(|| OsStr::new(FIRMWARE_PACKAGE));
        dist_dir().join(Path::new(stem).with_extension("hex"))
    }
}

/// Root of the Cargo workspace, derived from this crate's manifest location.
pub(crate) fn workspace_root() -> &'static Path {
    // `xtask/Cargo.toml` lives exactly one level below the workspace root.
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("xtask manifest directory always has a parent")
}

/// Cargo target directory, honouring `CARGO_TARGET_DIR`.
pub(crate) fn target_dir() -> PathBuf {
    std::env::var_os("CARGO_TARGET_DIR")
        .map_or_else(|| workspace_root().join("target"), PathBuf::from)
}

/// Directory holding distributable artifacts such as the HEX image.
pub(crate) fn dist_dir() -> PathBuf {
    workspace_root().join("dist")
}

/// Creates `dist/` if needed and returns it.
pub(crate) fn ensure_dist_dir() -> Result<PathBuf> {
    let dir = dist_dir();
    std::fs::create_dir_all(&dir).with_context(|| format!("cannot create {}", dir.display()))?;
    Ok(dir)
}
