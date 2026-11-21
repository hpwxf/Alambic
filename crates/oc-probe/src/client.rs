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
    Error, FrameReader, Group, MAX_FRAME, ParamKind, ParamValue, Profile, Reply, Request,
    SCREEN_PAGES, Sample, VERSION, decode, encode,
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

/// One applet, as the module lists it (journal/D-043).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppEntry {
    /// Its index, which names it in every other request.
    pub index: u8,
    /// Its name, as the app menu lists it.
    pub name: String,
    /// Whether it drives the outputs and the screen now.
    pub running: bool,
}

/// One parameter or observable of an applet.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpecEntry {
    /// Its index in its list.
    pub index: u8,
    /// Its name, which finds it whatever another setting does to its label.
    pub name: String,
    /// How the screen labels it now.
    pub label: String,
    /// What values it takes.
    pub kind: ParamKind,
    /// A choice's names, in order; empty otherwise.
    pub choices: Vec<String>,
    /// Whether it takes a CV input.
    pub assignable: bool,
    /// Whether it acts now: false for a face of a setting the screen does
    /// not show.
    pub active: bool,
    /// Its value when described; `None` for an observable not known yet.
    pub value: Option<ParamValue>,
}

/// Everything an applet declares, with its values when described.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct AppDescription {
    /// What it is set by.
    pub params: Vec<SpecEntry>,
    /// What it can be read for.
    pub observables: Vec<SpecEntry>,
    /// What it can be asked to do, by index.
    pub actions: Vec<String>,
}

impl AppDescription {
    /// Adds a parameter or an observable, in the order the module sends them.
    pub fn add_spec(&mut self, group: Group, spec: SpecEntry) {
        match group {
            Group::Param => self.params.push(spec),
            Group::Observable => self.observables.push(spec),
        }
    }

    /// Adds a choice's next name to the entry it follows.
    ///
    /// # Errors
    ///
    /// When it does not follow that entry, the order the module keeps.
    pub fn add_choice_name(
        &mut self,
        group: Group,
        index: u8,
        name: String,
    ) -> std::result::Result<(), &'static str> {
        let list = match group {
            Group::Param => &mut self.params,
            Group::Observable => &mut self.observables,
        };
        match list.last_mut() {
            Some(spec) if spec.index == index => {
                spec.choices.push(name);
                Ok(())
            }
            _ => Err("a name after its choice"),
        }
    }
}

/// An answer, copied out of the frame it was decoded from.
enum Answer {
    Hello(ModuleInfo),
    Profile(Box<Profile>),
    ScreenPage {
        page: u8,
        bytes: Vec<u8>,
    },
    Done,
    App(AppEntry),
    Spec(Group, SpecEntry),
    ChoiceName {
        group: Group,
        index: u8,
        name: String,
    },
    Action(String),
    Value(ParamValue),
    Observed(Option<ParamValue>),
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

    /// Every applet the module runs, in its order.
    ///
    /// # Errors
    ///
    /// As [`Self::hello`].
    pub fn apps(&mut self) -> Result<Vec<AppEntry>> {
        self.send(&Request::ListApps)?;
        let mut apps = Vec::new();
        loop {
            match self.next_answer()? {
                Answer::App(app) => apps.push(app),
                Answer::Done => return Ok(apps),
                _ => return Err(ProbeError::Unexpected("an applet")),
            }
        }
    }

    /// What applet `app` declares, with its values now.
    ///
    /// # Errors
    ///
    /// As [`Self::hello`]; no applet at that index.
    pub fn describe(&mut self, app: u8) -> Result<AppDescription> {
        self.send(&Request::DescribeApp { app })?;
        let mut description = AppDescription::default();
        loop {
            match self.next_answer()? {
                Answer::Spec(group, spec) => description.add_spec(group, spec),
                Answer::ChoiceName { group, index, name } => description
                    .add_choice_name(group, index, name)
                    .map_err(ProbeError::Unexpected)?,
                Answer::Action(name) => description.actions.push(name),
                Answer::Done => return Ok(description),
                _ => return Err(ProbeError::Unexpected("a description")),
            }
        }
    }

    /// The value parameter `param` of applet `app` holds.
    ///
    /// # Errors
    ///
    /// As [`Self::describe`]; no such parameter.
    pub fn get_param(&mut self, app: u8, param: u8) -> Result<ParamValue> {
        match self.ask(&Request::GetParam { app, param })? {
            Answer::Value(value) => Ok(value),
            _ => Err(ProbeError::Unexpected("a value")),
        }
    }

    /// Sets parameter `param` of applet `app`, as a detent would: what the
    /// applet applied, which another setting may have narrowed.
    ///
    /// # Errors
    ///
    /// As [`Self::get_param`]; a value of another kind or out of range.
    pub fn set_param(&mut self, app: u8, param: u8, value: ParamValue) -> Result<ParamValue> {
        match self.ask(&Request::SetParam { app, param, value })? {
            Answer::Value(value) => Ok(value),
            _ => Err(ProbeError::Unexpected("a value")),
        }
    }

    /// Observable `index` of applet `app` now, `None` while unknown.
    ///
    /// # Errors
    ///
    /// As [`Self::get_param`].
    pub fn observe(&mut self, app: u8, index: u8) -> Result<Option<ParamValue>> {
        match self.ask(&Request::Observe { app, index })? {
            Answer::Observed(value) => Ok(value),
            _ => Err(ProbeError::Unexpected("an observed value")),
        }
    }

    /// Asks applet `app` for action `action`.
    ///
    /// # Errors
    ///
    /// As [`Self::get_param`].
    pub fn act(&mut self, app: u8, action: u8) -> Result<()> {
        self.done(&Request::Act { app, action })
    }

    /// Hands the outputs and the screen to applet `app`.
    ///
    /// # Errors
    ///
    /// As [`Self::describe`].
    pub fn select_app(&mut self, app: u8) -> Result<()> {
        self.done(&Request::SelectApp { app })
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
            Reply::App {
                app, name, running, ..
            } => Answer::App(AppEntry {
                index: app,
                name: name.to_owned(),
                running,
            }),
            Reply::Spec {
                group,
                index,
                name,
                label,
                kind,
                assignable,
                active,
                value,
            } => Answer::Spec(
                group,
                SpecEntry {
                    index,
                    name: name.to_owned(),
                    label: label.to_owned(),
                    kind,
                    choices: Vec::new(),
                    assignable,
                    active,
                    value,
                },
            ),
            Reply::ChoiceName {
                group, index, name, ..
            } => Answer::ChoiceName {
                group,
                index,
                name: name.to_owned(),
            },
            Reply::Action { name, .. } => Answer::Action(name.to_owned()),
            Reply::Value(value) => Answer::Value(value),
            Reply::Observed(value) => Answer::Observed(value),
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
