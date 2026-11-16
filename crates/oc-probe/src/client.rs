//! The host's side of a probe session, over any byte stream: the module's
//! second serial port in use, an in-memory loopback in the tests.
//!
//! Requests are answered in order; samples arrive on their own while a
//! stream is on, so waiting for an answer sets the samples it meets aside
//! for [`Client::next_sample`].

use std::collections::VecDeque;
use std::fmt;
use std::io::{self, Read, Write};

use oc_core::framebuffer::LEN;
use oc_core::platform::{CvChannel, MilliVolts};
use oc_protocol::{
    Error, FrameReader, MAX_FRAME, Profile, Reply, Request, SCREEN_PAGES, Sample, VERSION, decode,
    encode,
};

/// What went wrong talking to the module.
#[derive(Debug)]
pub enum ProbeError {
    /// The port failed.
    Io(io::Error),
    /// Nothing came back in time.
    Timeout,
    /// The module refused the request.
    Refused(Error),
    /// The module answered something else than the request calls for.
    Unexpected(&'static str),
}

impl fmt::Display for ProbeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(f, "serial port: {error}"),
            Self::Timeout => f.write_str("no answer from the module"),
            Self::Refused(Error::Version(module)) => write!(
                f,
                "the module speaks protocol {module}, this tool {VERSION}: flash a matching firmware"
            ),
            Self::Refused(error) => write!(f, "the module refused: {error:?}"),
            Self::Unexpected(wanted) => write!(f, "unexpected answer, wanted {wanted}"),
        }
    }
}

impl std::error::Error for ProbeError {}

impl From<io::Error> for ProbeError {
    fn from(error: io::Error) -> Self {
        if matches!(
            error.kind(),
            io::ErrorKind::TimedOut | io::ErrorKind::WouldBlock
        ) {
            Self::Timeout
        } else {
            Self::Io(error)
        }
    }
}

/// Short for this module's results.
pub type Result<T> = std::result::Result<T, ProbeError>;

/// Who answered the hello.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModuleInfo {
    /// Firmware version.
    pub firmware: String,
    /// Build identifier.
    pub build: String,
}

/// An answer, copied out of the frame it was decoded from.
enum Answer {
    Hello(ModuleInfo),
    Profile(Box<Profile>),
    ScreenPage { page: u8, bytes: Vec<u8> },
    Done,
}

/// A session with one module.
#[derive(Debug)]
pub struct Client<T> {
    port: T,
    reader: FrameReader,
    received: VecDeque<u8>,
    samples: VecDeque<Sample>,
}

impl<T: Read + Write> Client<T> {
    /// Wraps an open port; nothing is sent until [`Self::hello`].
    pub fn new(port: T) -> Self {
        Self {
            port,
            reader: FrameReader::new(),
            received: VecDeque::new(),
            samples: VecDeque::new(),
        }
    }

    /// Gives the port back, ending the session from this side.
    pub fn into_inner(self) -> T {
        self.port
    }

    /// Opens the session and checks both sides speak the same protocol.
    ///
    /// # Errors
    ///
    /// A port failure, no answer, or another protocol version.
    pub fn hello(&mut self) -> Result<ModuleInfo> {
        match self.ask(&Request::Hello { protocol: VERSION })? {
            Answer::Hello(info) => Ok(info),
            _ => Err(ProbeError::Unexpected("hello")),
        }
    }

    /// The engine's profile.
    ///
    /// # Errors
    ///
    /// As [`Self::hello`].
    pub fn profile(&mut self) -> Result<Profile> {
        match self.ask(&Request::GetProfile)? {
            Answer::Profile(profile) => Ok(*profile),
            _ => Err(ProbeError::Unexpected("profile")),
        }
    }

    /// Forgets the profile's durations.
    ///
    /// # Errors
    ///
    /// As [`Self::hello`].
    pub fn clear_profile(&mut self) -> Result<()> {
        self.done(&Request::ClearProfile)
    }

    /// Asks for a sample every `every_ticks` ticks; zero stops.
    ///
    /// # Errors
    ///
    /// As [`Self::hello`].
    pub fn stream(&mut self, every_ticks: u16) -> Result<()> {
        self.done(&Request::Stream { every_ticks })?;
        if every_ticks == 0 {
            self.samples.clear();
        }
        Ok(())
    }

    /// The next sample of the stream.
    ///
    /// # Errors
    ///
    /// A port failure, or no sample in time.
    pub fn next_sample(&mut self) -> Result<Sample> {
        loop {
            if let Some(sample) = self.samples.pop_front() {
                return Ok(sample);
            }
            // An answer here belongs to no request: dropped.
            let _ = self.next_message()?;
        }
    }

    /// Holds `channel` at `millivolts`, or releases it (`None`), for as long
    /// as the session lasts.
    ///
    /// # Errors
    ///
    /// As [`Self::hello`].
    pub fn force_output(
        &mut self,
        channel: CvChannel,
        millivolts: Option<MilliVolts>,
    ) -> Result<()> {
        let channel = u8::try_from(channel.index()).unwrap_or(u8::MAX);
        self.done(&Request::ForceOutput {
            channel,
            millivolts,
        })
    }

    /// The screen, as it reads on the module, in the controller's byte order.
    ///
    /// # Errors
    ///
    /// As [`Self::hello`], or a page missing.
    pub fn screen(&mut self) -> Result<[u8; LEN]> {
        self.send(&Request::GetScreen)?;
        let mut screen = [0; LEN];
        let page_bytes = LEN / usize::from(SCREEN_PAGES);
        for expected in 0..SCREEN_PAGES {
            match self.next_answer()? {
                Answer::ScreenPage { page, bytes }
                    if page == expected && bytes.len() == page_bytes =>
                {
                    let start = usize::from(page) * page_bytes;
                    screen[start..start + page_bytes].copy_from_slice(&bytes);
                }
                _ => return Err(ProbeError::Unexpected("the next screen page")),
            }
        }
        Ok(screen)
    }

    /// Restarts the module into its bootloader. The port goes away with it.
    ///
    /// # Errors
    ///
    /// As [`Self::hello`]; a secure chip refuses.
    pub fn reboot_to_loader(&mut self) -> Result<()> {
        self.done(&Request::RebootToLoader)
    }

    fn done(&mut self, request: &Request) -> Result<()> {
        match self.ask(request)? {
            Answer::Done => Ok(()),
            _ => Err(ProbeError::Unexpected("done")),
        }
    }

    fn ask(&mut self, request: &Request) -> Result<Answer> {
        self.send(request)?;
        self.next_answer()
    }

    fn send(&mut self, request: &Request) -> Result<()> {
        let mut buffer = [0; MAX_FRAME];
        let frame = encode(request, &mut buffer).map_err(ProbeError::Refused)?;
        self.port.write_all(frame)?;
        self.port.flush()?;
        Ok(())
    }

    /// The next answer that is not a sample; samples met on the way are
    /// set aside. A refusal is returned as an error.
    fn next_answer(&mut self) -> Result<Answer> {
        loop {
            if let Some(answer) = self.next_message()? {
                return Ok(answer);
            }
        }
    }

    /// Reads one message: an answer, or `None` for a sample, set aside.
    fn next_message(&mut self) -> Result<Option<Answer>> {
        let mut frame = self.next_frame()?;
        let reply = decode::<Reply<'_>>(&mut frame)
            .map_err(|_| ProbeError::Unexpected("a frame that decodes"))?;
        let answer = match reply {
            Reply::Sample(sample) => {
                self.samples.push_back(sample);
                if self.samples.len() > 10_000 {
                    self.samples.pop_front();
                }
                return Ok(None);
            }
            Reply::Hello {
                firmware, build, ..
            } => Answer::Hello(ModuleInfo {
                firmware: firmware.to_owned(),
                build: build.to_owned(),
            }),
            Reply::Profile(profile) => Answer::Profile(Box::new(profile)),
            Reply::ScreenPage { page, bytes } => Answer::ScreenPage {
                page,
                bytes: bytes.to_vec(),
            },
            Reply::Done => Answer::Done,
            Reply::Error(error) => return Err(ProbeError::Refused(error)),
        };
        Ok(Some(answer))
    }

    /// The next complete frame, read from the port as needed.
    fn next_frame(&mut self) -> Result<Vec<u8>> {
        loop {
            while let Some(byte) = self.received.pop_front() {
                match self.reader.push(byte) {
                    None => {}
                    Some(Ok(frame)) => return Ok(frame.to_vec()),
                    Some(Err(error)) => return Err(ProbeError::Refused(error)),
                }
            }
            let mut chunk = [0; 512];
            let count = self.port.read(&mut chunk)?;
            if count == 0 {
                return Err(ProbeError::Timeout);
            }
            self.received.extend(&chunk[..count]);
        }
    }
}
