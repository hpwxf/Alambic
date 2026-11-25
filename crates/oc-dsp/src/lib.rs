//! The signal processing of the Ornament & Crime firmware's applets: what
//! computes samples, and what prepares their coefficients whenever a
//! parameter moves (journal/D-038, D-044, revisions of 2026-11-26).
//!
//! A crate of its own so that the release profile compiles it for speed
//! (`opt-level = 3`) while `oc-core`, the control, menus, text and graphics
//! that run at 1 kHz or less, compiles for size: with fat LTO a function
//! keeps the level of the crate it is generated in, and stable Rust has no
//! per-function level. What runs once a sample is generic or inlined, so it
//! is generated where the firmware instantiates it; what runs once per
//! change, such as [`supersaw::Coefficients::new`], is generated here.
//!
//! It knows no applet, no engine and no platform: millivolts, phases and
//! coefficients in, samples out. `oc-core` composes its blocks into
//! generators (`oc_core::apps::sound`); the applets keep their settings,
//! their screens and their CV inputs.

#![no_std]
#![forbid(unsafe_code)]

pub mod fixed;
pub mod supersaw;

/// A level, in millivolts (`oc_core::platform::MilliVolts`).
pub type MilliVolts = i32;

/// A frequency in thousandths of a hertz.
pub type MilliHertz = u32;

/// The lowest level the outputs reach, in millivolts: what a block clamps
/// its samples to (`oc_core::calibration::CV_OUT_MIN_MV`, checked equal there).
pub const OUT_MIN_MV: MilliVolts = -3_000;

/// The highest level the outputs reach, in millivolts.
pub const OUT_MAX_MV: MilliVolts = 6_000;

/// The phase a sample advances for `frequency` at `rate_hz`, held below half
/// a turn: at or above half the rate, a tone would alias to another pitch.
#[must_use]
pub fn phase_step(frequency: MilliHertz, rate_hz: u32) -> u32 {
    let turn = 1u64 << 32;
    // Divided in 32 bits whenever the rate in millihertz fits (journal/D-040).
    let numerator = u64::from(frequency) << 32;
    let step = match rate_hz.max(1).checked_mul(1_000) {
        Some(millihertz) => fixed::div_wide(numerator, millihertz),
        None => numerator / (u64::from(rate_hz) * 1_000),
    };
    // One step short of half a turn: the highest pitch that is still itself.
    u32::try_from(step.min(turn / 2 - 1)).unwrap_or(u32::MAX)
}
