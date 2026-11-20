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
//! The applets are reached through the same model the panel uses
//! (journal/D-043): [`Request::ListApps`] and [`Request::DescribeApp`] say
//! what each applet is set by, can be read for and can be asked to do, and
//! [`Request::SetParam`] goes through the bounds a detent goes through.
//! An applet is named by its index in the module's list, a parameter by its
//! index in the applet's.
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

use oc_core::apps::param::{Kind, ParamSpec, Unit};
pub use oc_core::apps::param::{ParamValue, Refused};
use oc_core::platform::{CV_CHANNELS, MilliVolts};
pub use oc_core::profile::Profile;

/// Protocol version, raised on any change to a message's encoding.
pub const VERSION: u16 = 4;

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
    /// The framebuffer as last presented, as [`SCREEN_PAGES`] replies, with
    /// the screen's rotation and shift undone: as it reads on the module.
    GetScreen,
    /// Restarts into the Teensy's bootloader, ready to be flashed.
    RebootToLoader,
    /// Every applet the module runs: one [`Reply::App`] each, then
    /// [`Reply::Done`].
    ListApps,
    /// What applet `app` is set by, can be read for and can be asked to do:
    /// one [`Reply::Spec`] per parameter then per observable, a
    /// [`Reply::ChoiceName`] after each choice for each of its names, one
    /// [`Reply::Action`] per action, then [`Reply::Done`].
    DescribeApp {
        /// The applet's index.
        app: u8,
    },
    /// The value parameter `param` of applet `app` holds:
    /// [`Reply::Value`].
    GetParam {
        /// The applet's index.
        app: u8,
        /// The parameter's index.
        param: u8,
    },
    /// Sets parameter `param` of applet `app` as a detent would, within the
    /// same bounds: [`Reply::Value`] with what it applied. Other parameters
    /// may move with it (`App::set`); a host reads them back with
    /// [`Request::DescribeApp`].
    SetParam {
        /// The applet's index.
        app: u8,
        /// The parameter's index.
        param: u8,
        /// The value asked for.
        value: ParamValue,
    },
    /// Observable `index` of applet `app` now: [`Reply::Observed`].
    Observe {
        /// The applet's index.
        app: u8,
        /// The observable's index.
        index: u8,
    },
    /// Asks applet `app` for action `action`: [`Reply::Done`].
    Act {
        /// The applet's index.
        app: u8,
        /// The action's index.
        action: u8,
    },
    /// Hands the outputs and the screen to applet `app`, as the app menu
    /// does: [`Reply::Done`].
    SelectApp {
        /// The applet's index.
        app: u8,
    },
}

/// Which of an applet's lists a [`Reply::Spec`] or a [`Reply::ChoiceName`]
/// belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Group {
    /// Its parameters.
    Param,
    /// Its observables.
    Observable,
}

/// What values a parameter takes, as it travels: a choice's names follow
/// in [`Reply::ChoiceName`]s, so a reply borrows only one string.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ParamKind {
    /// An integer from `min` to `max`, read with `decimals` digits after the
    /// point.
    Number {
        /// The least value.
        min: i32,
        /// The greatest value.
        max: i32,
        /// One detent's change on the panel.
        step: i32,
        /// Digits after the point when the value is read.
        decimals: u8,
        /// What it counts.
        unit: Unit,
    },
    /// One of `count` names.
    Choice {
        /// How many names.
        count: u8,
    },
    /// On or off.
    Toggle,
}

impl ParamKind {
    /// How `spec`'s values travel.
    #[must_use]
    pub fn of(spec: &ParamSpec) -> Self {
        match spec.kind {
            Kind::Number {
                min,
                max,
                step,
                decimals,
                unit,
            } => Self::Number {
                min,
                max,
                step,
                decimals,
                unit,
            },
            Kind::Choice(names) => Self::Choice {
                count: u8::try_from(names.len()).unwrap_or(u8::MAX),
            },
            Kind::Toggle => Self::Toggle,
        }
    }
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
    /// One applet, answering [`Request::ListApps`].
    App {
        /// Its index.
        app: u8,
        /// Its name, as the app menu lists it.
        name: &'a str,
        /// Whether it drives the outputs and the screen now.
        running: bool,
        /// How many parameters it has.
        params: u8,
        /// How many observables.
        observables: u8,
        /// How many actions.
        actions: u8,
    },
    /// One parameter or observable, answering [`Request::DescribeApp`].
    Spec {
        /// Its list.
        group: Group,
        /// Its index in that list.
        index: u8,
        /// Its name, as the screen labels it.
        name: &'a str,
        /// What values it takes.
        kind: ParamKind,
        /// Whether it takes a CV input.
        assignable: bool,
        /// Its value now; `None` for an observable not known yet.
        value: Option<ParamValue>,
    },
    /// One name of a choice, after its [`Reply::Spec`].
    ChoiceName {
        /// The choice's list.
        group: Group,
        /// The choice's index in that list.
        index: u8,
        /// The name's index among the choice's.
        choice: u8,
        /// The name.
        name: &'a str,
    },
    /// One action, answering [`Request::DescribeApp`].
    Action {
        /// Its index.
        index: u8,
        /// Its name.
        name: &'a str,
    },
    /// A parameter's value, answering [`Request::GetParam`] or
    /// [`Request::SetParam`].
    Value(ParamValue),
    /// An observable's value, `None` while unknown, answering
    /// [`Request::Observe`].
    Observed(Option<ParamValue>),
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
    /// The module's clock at the start of that tick, microseconds: ticks
    /// are not all the same length (a tick that redraws the screen may
    /// overrun), so durations are measured on this, not on tick counts.
    pub micros: u64,
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
    /// No applet at that index.
    NoSuchApp,
    /// The applet did not take the value or the action.
    Refused(Refused),
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
