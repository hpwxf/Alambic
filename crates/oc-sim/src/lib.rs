//! Native simulator for the Ornament & Crime firmware.
//!
//! The simulator runs the real [`oc_core`] engine against an in-memory
//! platform, so the module's behaviour can be driven, replayed and asserted on
//! without any hardware. It is the everyday development harness.
//!
//! * [`simulator`] — the module itself, wrapping `Engine::tick`;
//! * [`scenario`] — a text format for recording and replaying inputs;
//! * [`clock`] — the paused, real-time and turbo speed policies;
//! * [`braille`] — the 128x64 screen rendered as terminal glyphs;
//! * [`tui`] — the interactive terminal interface.
//!
//! # Example
//!
//! ```
//! use oc_sim::scenario::{Event, Scenario};
//! use oc_sim::simulator::Simulator;
//!
//! let scenario: Scenario = "ticks 10\n0 cv 2 -1500\n".parse().unwrap();
//! let mut simulator = Simulator::new();
//! simulator.skip_splash(); // start past the boot splash screen
//! simulator.replay(&scenario);
//!
//! // Channel two mirrors its input on the matching output.
//! assert_eq!(simulator.cv_out()[1], -1_500);
//! ```

/// Dense 2x4 terminal glyphs for the module screen (braille by default).
pub mod braille;
pub mod clock;
pub mod scenario;
pub mod simulator;
pub mod tui;

pub use scenario::{Event, Scenario};
pub use simulator::Simulator;

/// Identification string of the simulator, including the core version it runs.
#[must_use]
pub fn banner() -> &'static str {
    oc_core::BANNER
}
