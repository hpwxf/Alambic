//! Keeps the VCV Rack plugin version in step with the workspace version.
//!
//! Rack 2 requires a plugin version whose major number is 2, so the plugin
//! cannot simply reuse the firmware's `0.x.y`. The rule is
//! `2.<minor>.<patch>` of the workspace version: firmware `0.2.0` ships as
//! plugin `2.2.0`. Bumping one without the other fails here.

use std::fs;
use std::path::Path;

/// The `"version"` value of a `plugin.json`, found without a JSON parser:
/// the manifest is small, hand-written, and has exactly one such key.
fn manifest_version(manifest: &str) -> Option<&str> {
    let after_key = manifest.split("\"version\"").nth(1)?;
    let after_colon = after_key.split_once(':')?.1;
    let start = after_colon.find('"')? + 1;
    let rest = &after_colon[start..];
    Some(&rest[..rest.find('"')?])
}

#[test]
fn the_manifest_reader_finds_the_version() {
    assert_eq!(
        manifest_version("{\n  \"slug\": \"X\",\n  \"version\": \"2.4.1\",\n}"),
        Some("2.4.1")
    );
    assert_eq!(manifest_version("{}"), None);
}

#[test]
fn the_rack_plugin_version_follows_the_workspace_version() {
    // xtask inherits `version.workspace = true`, so this is the workspace's.
    let workspace = env!("CARGO_PKG_VERSION");
    let (_, minor_patch) = workspace
        .split_once('.')
        .expect("a semantic version has a major part");
    let expected = format!("2.{minor_patch}");

    let path =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../vcv/OrnamentCrimeAlambic/plugin.json");
    let manifest = fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("cannot read {}: {error}", path.display()));
    let actual = manifest_version(&manifest).expect("plugin.json has a \"version\" key");

    assert_eq!(
        actual, expected,
        "plugin.json version must be 2.<minor>.<patch> of the workspace version {workspace}"
    );
}
