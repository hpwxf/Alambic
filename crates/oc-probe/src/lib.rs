//! `oc-probe`: talks to a running Ornament & Crime module over the second
//! of its two USB serial ports (journal/D-029).
//!
//! [`client`] speaks the protocol over any byte stream, [`port`] finds and
//! opens the module's probe port, [`render`] turns what comes back into
//! text or an image. The binary (`src/main.rs`) is the command line on top.

pub mod client;
pub mod port;
pub mod render;
