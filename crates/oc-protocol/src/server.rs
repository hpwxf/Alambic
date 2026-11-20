//! The module's side of the protocol: requests in, replies and samples out,
//! served from the engine (journal/D-029).
//!
//! The backend feeds it the bytes the host sent ([`Server::receive`]), calls
//! [`Server::after_tick`] once a tick for the stream, sends what
//! [`Server::outgoing`] holds, and closes the session when the host goes
//! away ([`Server::close`]). Everything a request changes goes through the
//! engine, so it behaves the same on every backend and is tested here, once.

use oc_core::Engine;
use oc_core::TickReport;
use oc_core::apps::param::{Kind, ParamSpec, Refused};
use oc_core::apps::{App, AppId};
use oc_core::platform::{
    AnalogIn, AnalogOut, CV_CHANNELS, Clock, Controls, CvChannel, DigitalIn, Display,
    TriggerChannel,
};

use crate::queue::ByteQueue;
use crate::{
    Error, FrameReader, Group, MAX_FRAME, ParamKind, Reply, Request, SCREEN_PAGES, Sample, VERSION,
    decode, encode,
};

/// Room for the frames waiting to go out: a full screen capture (eight
/// pages) and some samples behind it.
pub const OUTGOING_BYTES: usize = 4_096;

/// What the module says about itself in [`Reply::Hello`], and what it can do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Identity {
    /// Firmware version.
    pub firmware: &'static str,
    /// Build identifier (journal/D-025).
    pub build: &'static str,
    /// Whether [`Request::RebootToLoader`] can be carried out here.
    pub can_reboot: bool,
}

/// A session with the host probe.
#[derive(Debug, Clone)]
pub struct Server {
    identity: Identity,
    reader: FrameReader,
    outgoing: ByteQueue<OUTGOING_BYTES>,
    session: bool,
    stream_every: u16,
    since_sample: u16,
    reboot_requested: bool,
}

impl Server {
    /// No session yet.
    #[must_use]
    pub const fn new(identity: Identity) -> Self {
        Self {
            identity,
            reader: FrameReader::new(),
            outgoing: ByteQueue::new(),
            session: false,
            stream_every: 0,
            since_sample: 0,
            reboot_requested: false,
        }
    }

    /// Says whether [`Request::RebootToLoader`] can be carried out, once the
    /// backend knows (a server built as a constant cannot ask the chip).
    pub const fn set_can_reboot(&mut self, can_reboot: bool) {
        self.identity.can_reboot = can_reboot;
    }

    /// Whether a host opened a session.
    #[must_use]
    pub const fn in_session(&self) -> bool {
        self.session
    }

    /// The bytes waiting for the host.
    pub const fn outgoing(&mut self) -> &mut ByteQueue<OUTGOING_BYTES> {
        &mut self.outgoing
    }

    /// Whether the host asked to restart into the bootloader; true once.
    pub const fn take_reboot_request(&mut self) -> bool {
        core::mem::replace(&mut self.reboot_requested, false)
    }

    /// Serves every request complete in `bytes`.
    pub fn receive<A, O, D, C, K, S>(&mut self, bytes: &[u8], engine: &mut Engine<A, O, D, C, K, S>)
    where
        A: AnalogIn,
        O: AnalogOut,
        D: DigitalIn,
        C: Controls,
        K: Clock,
        S: Display,
    {
        for &byte in bytes {
            let request = match self.reader.push(byte) {
                None => continue,
                Some(Ok(frame)) => decode::<Request>(frame),
                Some(Err(error)) => Err(error),
            };
            match request {
                Ok(request) => self.serve(request, engine),
                Err(error) => self.send(&Reply::Error(error)),
            }
        }
    }

    /// Sends a sample when the stream asks for one on this tick.
    pub fn after_tick<A, O, D, C, K, S>(
        &mut self,
        report: &TickReport,
        engine: &mut Engine<A, O, D, C, K, S>,
    ) where
        A: AnalogIn,
        O: AnalogOut,
        D: DigitalIn,
        C: Controls,
        K: Clock,
        S: Display,
    {
        if !self.session || self.stream_every == 0 {
            return;
        }
        self.since_sample += 1;
        if self.since_sample < self.stream_every {
            return;
        }
        self.since_sample = 0;
        let sample = sample(report, engine);
        self.send(&Reply::Sample(sample));
    }

    /// Ends the session: the stream stops, every forced output is released
    /// and what was waiting for the host is dropped.
    pub fn close<A, O, D, C, K, S>(&mut self, engine: &mut Engine<A, O, D, C, K, S>)
    where
        A: AnalogIn,
        O: AnalogOut,
        D: DigitalIn,
        C: Controls,
        K: Clock,
        S: Display,
    {
        if self.session {
            for channel in CvChannel::ALL {
                engine.force_output(channel, None);
            }
        }
        self.session = false;
        self.stream_every = 0;
        self.reader = FrameReader::new();
        self.outgoing.clear();
    }

    fn serve<A, O, D, C, K, S>(&mut self, request: Request, engine: &mut Engine<A, O, D, C, K, S>)
    where
        A: AnalogIn,
        O: AnalogOut,
        D: DigitalIn,
        C: Controls,
        K: Clock,
        S: Display,
    {
        if let Request::Hello { protocol } = request {
            if protocol == VERSION {
                self.session = true;
                self.send(&Reply::Hello {
                    protocol: VERSION,
                    firmware: self.identity.firmware,
                    build: self.identity.build,
                });
            } else {
                self.send(&Reply::Error(Error::Version(VERSION)));
            }
            return;
        }
        if !self.session {
            self.send(&Reply::Error(Error::NoSession));
            return;
        }
        let reply = match request {
            Request::Hello { .. } => return,
            Request::GetProfile => Reply::Profile(engine.profile().clone()),
            Request::ClearProfile => {
                engine.profile_mut().clear();
                Reply::Done
            }
            Request::Stream { every_ticks } => {
                self.stream_every = every_ticks;
                self.since_sample = 0;
                Reply::Done
            }
            Request::ForceOutput {
                channel,
                millivolts,
            } => match CvChannel::ALL.get(usize::from(channel)) {
                Some(&channel) => {
                    engine.force_output(channel, millivolts);
                    Reply::Done
                }
                None => Reply::Error(Error::BadChannel),
            },
            Request::GetScreen => {
                self.send_screen(engine);
                return;
            }
            Request::RebootToLoader => {
                if self.identity.can_reboot {
                    self.reboot_requested = true;
                    Reply::Done
                } else {
                    Reply::Error(Error::Unsupported)
                }
            }
            request @ (Request::ListApps
            | Request::DescribeApp { .. }
            | Request::GetParam { .. }
            | Request::SetParam { .. }
            | Request::Observe { .. }
            | Request::Act { .. }
            | Request::SelectApp { .. }) => match self.serve_app(request, engine) {
                Some(reply) => reply,
                None => return,
            },
        };
        self.send(&reply);
    }

    /// Serves a request about the applets (journal/D-043): its reply, or
    /// `None` when it sent several itself.
    fn serve_app<A, O, D, C, K, S>(
        &mut self,
        request: Request,
        engine: &mut Engine<A, O, D, C, K, S>,
    ) -> Option<Reply<'static>>
    where
        A: AnalogIn,
        O: AnalogOut,
        D: DigitalIn,
        C: Controls,
        K: Clock,
        S: Display,
    {
        let reply = match request {
            Request::ListApps => {
                self.send_apps(engine);
                return None;
            }
            Request::DescribeApp { app } => {
                if let Some(id) = app_id(app) {
                    self.describe(engine.apps().get(id));
                    return None;
                }
                Reply::Error(Error::NoSuchApp)
            }
            Request::GetParam { app, param } => match app_id(app) {
                Some(id) => engine
                    .apps()
                    .get(id)
                    .get(usize::from(param))
                    .map_or(Reply::Error(Error::Refused(Refused::NoSuch)), Reply::Value),
                None => Reply::Error(Error::NoSuchApp),
            },
            Request::SetParam { app, param, value } => match app_id(app) {
                Some(id) => match engine.app_mut(id).set(usize::from(param), value) {
                    Ok(applied) => Reply::Value(applied),
                    Err(refused) => Reply::Error(Error::Refused(refused)),
                },
                None => Reply::Error(Error::NoSuchApp),
            },
            Request::Observe { app, index } => match app_id(app) {
                Some(id) => {
                    let app = engine.apps().get(id);
                    if usize::from(index) < app.observables().len() {
                        Reply::Observed(app.observe(usize::from(index)))
                    } else {
                        Reply::Error(Error::Refused(Refused::NoSuch))
                    }
                }
                None => Reply::Error(Error::NoSuchApp),
            },
            Request::Act { app, action } => match app_id(app) {
                Some(id) => match engine.app_mut(id).act(usize::from(action)) {
                    Ok(()) => Reply::Done,
                    Err(refused) => Reply::Error(Error::Refused(refused)),
                },
                None => Reply::Error(Error::NoSuchApp),
            },
            Request::SelectApp { app } => match app_id(app) {
                Some(id) => {
                    engine.select_app(id);
                    Reply::Done
                }
                None => Reply::Error(Error::NoSuchApp),
            },
            _ => Reply::Error(Error::Unsupported),
        };
        Some(reply)
    }

    /// One [`Reply::App`] per applet, then [`Reply::Done`].
    fn send_apps<A, O, D, C, K, S>(&mut self, engine: &Engine<A, O, D, C, K, S>)
    where
        A: AnalogIn,
        O: AnalogOut,
        D: DigitalIn,
        C: Controls,
        K: Clock,
        S: Display,
    {
        for id in AppId::ALL {
            let app = engine.apps().get(id);
            self.send(&Reply::App {
                app: count(id.index()),
                name: app.name(),
                running: engine.current_app() == id,
                params: count(app.param_specs().len()),
                observables: count(app.observables().len()),
                actions: count(app.actions().len()),
            });
        }
        self.send(&Reply::Done);
    }

    /// Everything `app` declares, with the values it holds now, then
    /// [`Reply::Done`].
    fn describe(&mut self, app: &dyn App) {
        for (index, spec) in app.param_specs().iter().enumerate() {
            self.send_spec(
                Group::Param,
                index,
                spec,
                app.param_label(index),
                app.get(index),
            );
        }
        for (index, spec) in app.observables().iter().enumerate() {
            self.send_spec(
                Group::Observable,
                index,
                spec,
                spec.name,
                app.observe(index),
            );
        }
        for (index, name) in app.actions().iter().enumerate() {
            self.send(&Reply::Action {
                index: count(index),
                name,
            });
        }
        self.send(&Reply::Done);
    }

    /// One [`Reply::Spec`], and the names of a choice after it.
    fn send_spec(
        &mut self,
        group: Group,
        index: usize,
        spec: &ParamSpec,
        label: &str,
        value: Option<oc_core::apps::param::ParamValue>,
    ) {
        self.send(&Reply::Spec {
            group,
            index: count(index),
            name: spec.name,
            label,
            kind: ParamKind::of(spec),
            assignable: spec.assignable,
            value,
        });
        if let Kind::Choice(names) = spec.kind {
            for (choice, name) in names.iter().enumerate() {
                self.send(&Reply::ChoiceName {
                    group,
                    index: count(index),
                    choice: count(choice),
                    name,
                });
            }
        }
    }

    fn send_screen<A, O, D, C, K, S>(&mut self, engine: &mut Engine<A, O, D, C, K, S>)
    where
        A: AnalogIn,
        O: AnalogOut,
        D: DigitalIn,
        C: Controls,
        K: Clock,
        S: Display,
    {
        // The frame was sent turned and shifted for the panel's mounting;
        // undone here, so the capture reads as the module's face does.
        let screen = engine.settings().display;
        let (.., display) = engine.parts_mut();
        let mut upright = display.frame_mut().clone();
        upright.shift_columns(-i32::from(screen.column_shift));
        if screen.rotated {
            upright.rotate_half_turn();
        }
        let frame = *upright.as_bytes();
        for (page, bytes) in
            (0..SCREEN_PAGES).zip(frame.chunks(frame.len() / usize::from(SCREEN_PAGES)))
        {
            self.send(&Reply::ScreenPage { page, bytes });
        }
    }

    /// Queues one reply whole, or drops it if the host is not keeping up.
    fn send(&mut self, reply: &Reply<'_>) {
        let mut buffer = [0; MAX_FRAME];
        if let Ok(frame) = encode(reply, &mut buffer) {
            self.outgoing.push(frame);
        }
    }
}

/// The applet at index `app` in the module's list.
fn app_id(app: u8) -> Option<AppId> {
    AppId::from_index(usize::from(app))
}

/// A list's length or index as it travels; no applet lists anywhere near
/// 255 of anything.
fn count(value: usize) -> u8 {
    u8::try_from(value).unwrap_or(u8::MAX)
}

/// The module's state on the tick `report` describes.
fn sample<A, O, D, C, K, S>(report: &TickReport, engine: &mut Engine<A, O, D, C, K, S>) -> Sample
where
    A: AnalogIn,
    O: AnalogOut,
    D: DigitalIn,
    C: Controls,
    K: Clock,
    S: Display,
{
    let codes = *engine.raw_codes();
    let inputs = engine.calibration().inputs;
    let mut cv_in = [0; CV_CHANNELS];
    for (index, millivolts) in cv_in.iter_mut().enumerate() {
        *millivolts = inputs[index].to_millivolts(codes[index]);
    }
    let (_, _, digital_in, ..) = engine.parts_mut();
    let triggers = TriggerChannel::ALL
        .into_iter()
        .enumerate()
        .filter(|&(_, channel)| digital_in.trigger_state(channel))
        .fold(0_u8, |bits, (index, _)| bits | (1 << index));
    Sample {
        tick: report.tick_count,
        micros: report.timestamp_micros,
        cv_in,
        cv_codes: codes,
        cv_out: report.cv_out,
        triggers,
    }
}
