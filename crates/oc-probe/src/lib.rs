//! `oc-probe`: talks to a running Ornament & Crime module over the second
//! of its two USB serial ports (journal/D-029).
//!
//! [`client`] speaks the protocol over any byte stream, [`port`] finds and
//! opens the module's probe port, [`render`] turns what comes back into
//! text or an image. The binary (`src/main.rs`) is the command line on top.
//! [`port`], the PNG output and the binary need the `cli` feature (on by
//! default); the rest has no operating-system dependency and also serves the
//! browser tool (`oc-web`, journal/D-031).

pub mod client;
pub mod measure;
#[cfg(feature = "cli")]
pub mod port;
pub mod render;

/// The module's USB identifiers (the pid.codes test pair its firmware uses).
pub const VID: u16 = 0x5824;
/// See [`VID`].
pub const PID: u16 = 0x27dd;
