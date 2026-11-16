//! The messages the firmware and its host probe exchange on the module's
//! second USB serial port, and how they are framed (journal/D-029).
//!
//! Every message is encoded with `postcard` and framed with COBS: a frame
//! is the encoded bytes, none of them zero, followed by one zero byte. A
//! receiver that joins mid-stream, or loses bytes, resynchronises at the next
//! zero. [`FrameReader`] splits the incoming bytes into frames and
//! [`decode`] turns one into a message; [`encode`] does the reverse.
//!
//! The host speaks first ([`Request`]); the module answers with one
//! [`Reply`] per request, in order, and sends [`Reply::Sample`] on its own
//! while a stream is on. Both sides check [`VERSION`] in the hello exchange
//! and refuse to talk across versions rather than misread each other.
//!
//! The crate is `no_std` and allocation-free, so the firmware and the host
//! tool share the exact same types; replies borrow their text and bytes from
//! the frame they were decoded from.

#![no_std]
#![forbid(unsafe_code)]
#![warn(missing_docs)]

#[cfg(test)]
extern crate std;

pub mod queue;
pub mod server;

use serde::{Deserialize, Serialize};

use oc_core::platform::{CV_CHANNELS, MilliVolts};
pub use oc_core::profile::Profile;

/// Protocol version, raised on any change to a message's encoding.
pub const VERSION: u16 = 1;

/// Longest frame either side sends or accepts, zero byte included. A
/// screen page (128 bytes) and a profile (a few hundred) both fit with room
/// for COBS's one byte in 254.
pub const MAX_FRAME: usize = 512;

/// Pages of a framebuffer, as [`Reply::ScreenPage`] sends them.
pub const SCREEN_PAGES: u8 = 8;

/// What the host asks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Request {
    /// Opens a session: the module answers [`Reply::Hello`].
    Hello {
        /// The host's [`VERSION`].
        protocol: u16,
    },
    /// The engine's profile since the boot splash ended or the last clear.
    GetProfile,
    /// Forgets the profile's durations.
    ClearProfile,
    /// Sends a [`Reply::Sample`] every `every_ticks` ticks; zero stops.
    Stream {
        /// Ticks between two samples.
        every_ticks: u16,
    },
    /// Holds one output at a level, overriding whatever drives it, or
    /// releases it (`None`). Every output is released when the session
    /// closes or the module restarts.
    ForceOutput {
        /// Output index, 0 for A to 3 for D.
        channel: u8,
        /// The level, or `None` to release.
        millivolts: Option<MilliVolts>,
    },
    /// The framebuffer as last presented, as [`SCREEN_PAGES`] replies.
    GetScreen,
    /// Restarts into the Teensy's bootloader, ready to be flashed.
    RebootToLoader,
}

/// What the module answers, or sends on its own.
// No heap to box the profile into, and a reply only lives while it is
// encoded or matched, so the size of the largest variant costs nothing.
#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Reply<'a> {
    /// Answer to [`Request::Hello`].
    Hello {
        /// The module's [`VERSION`].
        protocol: u16,
        /// Firmware version (`CARGO_PKG_VERSION`).
        firmware: &'a str,
        /// Build identifier (journal/D-025).
        build: &'a str,
    },
    /// Answer to [`Request::GetProfile`].
    Profile(Profile),
    /// One page of the screen, in the controller's byte order.
    ScreenPage {
        /// Page index, 0 at the top.
        page: u8,
        /// The page's 128 column bytes, least significant bit at the top.
        bytes: &'a [u8],
    },
    /// The state of the module on one tick, while a stream is on.
    Sample(Sample),
    /// The request was carried out and has nothing else to say.
    Done,
    /// The request was refused.
    Error(Error),
}

/// The state of the module on one tick.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Sample {
    /// The engine's tick count.
    pub tick: u64,
    /// Each CV input, calibrated.
    pub cv_in: [MilliVolts; CV_CHANNELS],
    /// Each CV input's raw ADC code.
    pub cv_codes: [u16; CV_CHANNELS],
    /// Each CV output, as written.
    pub cv_out: [MilliVolts; CV_CHANNELS],
    /// The trigger inputs, bit `n` for trigger `n + 1`, set when high.
    pub triggers: u8,
}

/// Why a request was refused, or a frame dropped.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Error {
    /// The frame did not decode as a request.
    Malformed,
    /// The frame was longer than [`MAX_FRAME`].
    Overflow,
    /// The host speaks another [`VERSION`]; the module's is given.
    Version(u16),
    /// No session: [`Request::Hello`] comes first.
    NoSession,
    /// An output index out of range.
    BadChannel,
    /// This module cannot do it.
    Unsupported,
}

/// Encodes `message` as one frame, zero byte included, into `buffer`.
///
/// # Errors
///
/// [`Error::Overflow`] if the frame does not fit in `buffer`.
pub fn encode<'b, T: Serialize>(message: &T, buffer: &'b mut [u8]) -> Result<&'b [u8], Error> {
    postcard::to_slice_cobs(message, buffer)
        .map(|frame| &*frame)
        .map_err(|_| Error::Overflow)
}

/// Decodes one frame, as [`FrameReader::push`] hands it out, in place; the
/// message may borrow from it.
///
/// # Errors
///
/// [`Error::Malformed`] if the bytes are not a valid frame of a `T`.
pub fn decode<'de, T: Deserialize<'de>>(frame: &'de mut [u8]) -> Result<T, Error> {
    postcard::from_bytes_cobs(frame).map_err(|_| Error::Malformed)
}

/// Splits a byte stream into frames at each zero byte.
#[derive(Debug, Clone)]
pub struct FrameReader {
    buffer: [u8; MAX_FRAME],
    len: usize,
    overflowed: bool,
}

impl Default for FrameReader {
    fn default() -> Self {
        Self::new()
    }
}

impl FrameReader {
    /// Nothing received yet.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            buffer: [0; MAX_FRAME],
            len: 0,
            overflowed: false,
        }
    }

    /// Takes one byte; at the end of a frame, hands out its bytes (zero
    /// excluded), or [`Error::Overflow`] if it was too long and was dropped.
    /// Empty frames (two zeros in a row) are skipped.
    pub fn push(&mut self, byte: u8) -> Option<Result<&mut [u8], Error>> {
        if byte != 0 {
            if self.len < self.buffer.len() {
                self.buffer[self.len] = byte;
                self.len += 1;
            } else {
                self.overflowed = true;
            }
            return None;
        }
        let len = core::mem::take(&mut self.len);
        if core::mem::take(&mut self.overflowed) {
            return Some(Err(Error::Overflow));
        }
        if len == 0 {
            return None;
        }
        Some(Ok(&mut self.buffer[..len]))
    }
}
