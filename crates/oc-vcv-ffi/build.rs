//! Generates `include/oc_vcv_ffi.h` from the ABI in `src/lib.rs`.
//!
//! Running this on every build (rather than as a separate, easy-to-forget
//! step) is what keeps the C header honest: `vcv/OrnamentCrimeRust` includes
//! it directly, so a signature that drifts from the header would otherwise
//! only be caught by the C++ compiler, far from the Rust change that caused
//! it.

use std::env;
use std::path::PathBuf;

fn main() {
    println!("cargo:rerun-if-changed=src/lib.rs");
    println!("cargo:rerun-if-changed=cbindgen.toml");

    let crate_dir = env::var("CARGO_MANIFEST_DIR").expect("Cargo always sets CARGO_MANIFEST_DIR");
    let header_path = PathBuf::from(&crate_dir)
        .join("include")
        .join("oc_vcv_ffi.h");

    std::fs::create_dir_all(
        header_path
            .parent()
            .expect("include/oc_vcv_ffi.h always has a parent directory"),
    )
    .expect("cannot create crates/oc-vcv-ffi/include/");

    let config = cbindgen::Config::from_root_or_default(&crate_dir);

    // A failure here means the exported ABI no longer matches what cbindgen
    // can express in C; that is a build-breaking mistake, not something to
    // silently degrade past, since the whole point of generating the header
    // is to keep it truthful.
    let bindings = cbindgen::Builder::new()
        .with_crate(&crate_dir)
        .with_config(config)
        .generate()
        .expect("cbindgen could not parse the ABI exported by src/lib.rs");

    let _up_to_date = bindings.write_to_file(&header_path);
}
