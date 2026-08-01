//! C ABI over [`oc_core`], linked into the VCV Rack 2 module.
//!
//! Every exported function is defensive: null pointers and out-of-range
//! indices are no-ops, and no panic is ever allowed to unwind across the
//! boundary into C++.

/// Version of the core engine exposed through the ABI, as a NUL-terminated
/// C string.
#[must_use]
pub fn core_banner() -> &'static str {
    oc_core::BANNER
}
