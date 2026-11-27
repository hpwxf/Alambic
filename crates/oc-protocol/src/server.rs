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
use oc_core::apps::InputSource;
use oc_core::apps::param::{Kind, ParamSpec, Refused};
use oc_core::apps::{
    App, AppHost, AppId, InputKind, Instance, MAX_INSTANCES, PortSignal, STATE_MAX,
};
use oc_core::modulation::Source;
use oc_core::platform::{
    AnalogIn, AnalogOut, CV_CHANNELS, Clock, Controls, CvChannel, DigitalIn, Display, MilliVolts,
    TriggerChannel,
};

use crate::queue::ByteQueue;
use crate::{
    Assign, Error, FrameReader, Group, MAX_FRAME, ParamKind, Reply, Request, SCREEN_PAGES,
    STATE_CHUNK, STATE_REPLY_CHUNK, Sample, Target, VERSION, decode, encode,
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
    /// An applet's own state as [`Request::SetState`] pieces bring it
    /// (journal/D-050), and how much of it came.
    state: [u8; STATE_MAX],
    state_len: usize,
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
            state: [0; STATE_MAX],
            state_len: 0,
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
            | Request::SelectApp { .. }
            | Request::GetAssign { .. }
            | Request::SetAssign { .. }
            | Request::ListInstances
            | Request::Focus { .. }
            | Request::SetRoute { .. }
            | Request::SetInput { .. }
            | Request::GetState { .. }
            | Request::SetState { .. }) => match self.serve_app(request, engine) {
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
                let mut fresh = None;
                if let Some(app) = read_target(engine.apps(), app, &mut fresh) {
                    self.describe(app);
                    return None;
                }
                Reply::Error(Error::NoSuchApp)
            }
            Request::GetParam { app, param } => match read_target(engine.apps(), app, &mut None) {
                Some(app) => app
                    .get(usize::from(param))
                    .map_or(Reply::Error(Error::Refused(Refused::NoSuch)), Reply::Value),
                None => Reply::Error(Error::NoSuchApp),
            },
            Request::SetParam { app, param, value } => match write_target(engine.apps_mut(), app) {
                Some(app) => match app.set(usize::from(param), value) {
                    Ok(applied) => Reply::Value(applied),
                    Err(refused) => Reply::Error(Error::Refused(refused)),
                },
                None => Reply::Error(Error::NoSuchApp),
            },
            Request::Observe { app, index } => match read_target(engine.apps(), app, &mut None) {
                Some(app) if usize::from(index) < app.observables().len() => {
                    Reply::Observed(app.observe(usize::from(index)))
                }
                Some(_) => Reply::Error(Error::Refused(Refused::NoSuch)),
                None => Reply::Error(Error::NoSuchApp),
            },
            Request::Act { app, action } => match write_target(engine.apps_mut(), app) {
                Some(app) => match app.act(usize::from(action)) {
                    Ok(()) => Reply::Done,
                    Err(refused) => Reply::Error(Error::Refused(refused)),
                },
                None => Reply::Error(Error::NoSuchApp),
            },
            Request::SelectApp { app, instance } => match (app_id(app), instance) {
                (Some(id), None) => {
                    engine.select_app(id);
                    Reply::Done
                }
                (Some(id), Some(instance)) if engine.select_app_at(usize::from(instance), id) => {
                    Reply::Done
                }
                _ => Reply::Error(Error::NoSuchApp),
            },
            Request::GetState { app, offset } => {
                return self.send_state(engine.apps(), app, offset);
            }
            Request::SetState {
                app,
                offset,
                length,
                bytes,
                last,
            } => self.take_state(
                engine.apps_mut(),
                app,
                offset,
                &bytes[..usize::from(length).min(STATE_CHUNK)],
                last,
            ),
            Request::GetAssign { app, param } => match read_target(engine.apps(), app, &mut None) {
                Some(app) => input_reply(app, param),
                None => Reply::Error(Error::NoSuchApp),
            },
            Request::SetAssign { app, assign, .. }
                if !source_offered(engine.apps(), app, assign) =>
            {
                Reply::Error(Error::Refused(Refused::NoSuch))
            }
            Request::SetAssign { app, param, assign } => {
                match write_target(engine.apps_mut(), app) {
                    Some(app) => match app.set_assign(usize::from(param), assign) {
                        Ok(_) => input_reply(app, param),
                        Err(refused) => Reply::Error(Error::Refused(refused)),
                    },
                    None => Reply::Error(Error::NoSuchApp),
                }
            }
            Request::ListInstances
            | Request::Focus { .. }
            | Request::SetRoute { .. }
            | Request::SetInput { .. } => self.serve_instances(request, engine.apps_mut()),
            _ => Reply::Error(Error::Unsupported),
        };
        Some(reply)
    }

    /// Serves a request about the instances running (journal/D-046).
    fn serve_instances(&mut self, request: Request, apps: &mut AppHost) -> Reply<'static> {
        match request {
            Request::ListInstances => {
                for position in 0..MAX_INSTANCES {
                    if let Some(reply) = instance_reply(&*apps, position) {
                        self.send(&reply);
                    }
                }
                Reply::Done
            }
            Request::Focus { instance } => {
                if apps.set_focus(usize::from(instance)) {
                    Reply::Done
                } else {
                    Reply::Error(Error::NoSuchApp)
                }
            }
            Request::SetRoute {
                instance,
                port,
                output,
            } => {
                let output = match output {
                    Some(output) => match CvChannel::from_index(usize::from(output)) {
                        Some(channel) => Some(channel),
                        None => return Reply::Error(Error::BadChannel),
                    },
                    None => None,
                };
                let position = usize::from(instance);
                match apps.set_route(position, usize::from(port), output) {
                    Ok(_) => instance_reply(&*apps, position).unwrap_or(Reply::Done),
                    Err(Refused::NoSuch) if apps.at(position).is_none() => {
                        Reply::Error(Error::NoSuchApp)
                    }
                    Err(refused) => Reply::Error(Error::Refused(refused)),
                }
            }
            Request::SetInput {
                instance,
                port,
                source,
            } => {
                let position = usize::from(instance);
                match apps.set_input(position, usize::from(port), source) {
                    Ok(_) => instance_reply(&*apps, position).unwrap_or(Reply::Done),
                    Err(_) if apps.at(position).is_none() => Reply::Error(Error::NoSuchApp),
                    Err(refused) => Reply::Error(Error::Refused(refused)),
                }
            }
            _ => Reply::Error(Error::Unsupported),
        }
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
            // An empty slot runs NOTHING: not an applet that runs.
            let running = id != AppId::Nothing && engine.apps().get(id).is_some();
            let mut fresh = None;
            let Some(app) = read_target(engine.apps(), Target::App(count(id.index())), &mut fresh)
            else {
                continue;
            };
            self.send(&Reply::App {
                app: count(id.index()),
                name: app.name(),
                running,
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
            let shown = Shown {
                label: app.param_label(index),
                active: app.param_active(index),
                value: app.get(index),
            };
            self.send_spec(Group::Param, index, spec, shown);
            self.send_input(app, count(index));
        }
        for (index, spec) in app.observables().iter().enumerate() {
            let shown = Shown {
                label: spec.name,
                active: true,
                value: app.observe(index),
            };
            self.send_spec(Group::Observable, index, spec, shown);
        }
        for (index, name) in app.actions().iter().enumerate() {
            self.send(&Reply::Action {
                index: count(index),
                name,
            });
        }
        for (index, port) in app.output_ports().iter().enumerate() {
            self.send(&Reply::Port {
                output: true,
                index: count(index),
                name: port.name,
                trigger: false,
                level: port.signal == PortSignal::Cv,
            });
        }
        for (index, port) in app.input_ports().iter().enumerate() {
            self.send(&Reply::Port {
                output: false,
                index: count(index),
                name: port.name,
                trigger: port.kind == InputKind::Trigger,
                level: false,
            });
        }
        self.send(&Reply::Done);
    }

    /// One [`Reply::Spec`], and the names of a choice after it.
    fn send_spec(&mut self, group: Group, index: usize, spec: &ParamSpec, shown: Shown<'_>) {
        self.send(&Reply::Spec {
            group,
            index: count(index),
            name: spec.name,
            label: shown.label,
            kind: ParamKind::of(spec),
            assignable: spec.assignable,
            active: shown.active,
            value: shown.value,
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

    /// The [`Reply::Assign`] of parameter `param` of `app`, if it takes an
    /// input. Not inlined: a [`Reply`] is the size of its largest variant
    /// (the profile, ≈ 460 bytes), and built in [`Self::describe`] it would
    /// sit on its frame under every reply the description sends, on the
    /// deepest path of the main stack.
    #[inline(never)]
    fn send_input(&mut self, app: &dyn App, param: u8) {
        if let Some((assign, sweep)) = input_of(app, param) {
            self.send(&Reply::Assign {
                index: param,
                assign,
                sweep,
            });
        }
    }

    // Not inlined: its two copies of the framebuffer (2 KiB) would sit on
    // the frame of `Usb::serve` under every other request, `DescribeApp`
    // included, on the deepest path of the main stack (16 KiB).
    #[inline(never)]
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
        let frame = upright.as_bytes();
        for (page, bytes) in
            (0..SCREEN_PAGES).zip(frame.chunks(frame.len() / usize::from(SCREEN_PAGES)))
        {
            self.send(&Reply::ScreenPage { page, bytes });
        }
    }

    /// Queues one reply whole, or drops it if the host is not keeping up.
    /// Sends the piece of `target`'s own state from `offset` (journal/D-050):
    /// `None` once sent, the refusal otherwise.
    fn send_state(
        &mut self,
        apps: &AppHost,
        target: Target,
        offset: u16,
    ) -> Option<Reply<'static>> {
        let mut fresh = None;
        let Some(app) = read_target(apps, target, &mut fresh) else {
            return Some(Reply::Error(Error::NoSuchApp));
        };
        let mut buffer = [0; STATE_MAX];
        let Some(total) = app.save_state(&mut buffer) else {
            return Some(Reply::Error(Error::Refused(Refused::NoSuch)));
        };
        let start = usize::from(offset).min(total);
        let end = (start + STATE_REPLY_CHUNK).min(total);
        self.send(&Reply::State {
            total: u16::try_from(total).unwrap_or(u16::MAX),
            offset: u16::try_from(start).unwrap_or(u16::MAX),
            bytes: &buffer[start..end],
        });
        None
    }

    /// Keeps a piece of an applet's own state, and on the last one reads the
    /// whole back into `target` (journal/D-050). A piece that does not
    /// follow the last one, or overflows [`STATE_MAX`], is refused and
    /// starts nothing.
    fn take_state(
        &mut self,
        apps: &mut AppHost,
        target: Target,
        offset: u16,
        piece: &[u8],
        last: bool,
    ) -> Reply<'static> {
        let start = usize::from(offset);
        if start == 0 {
            self.state_len = 0;
        }
        let end = start + piece.len();
        if start != self.state_len || end > STATE_MAX {
            self.state_len = 0;
            return Reply::Error(Error::Refused(Refused::OutOfRange));
        }
        self.state[start..end].copy_from_slice(piece);
        self.state_len = end;
        if !last {
            return Reply::Done;
        }
        let len = core::mem::take(&mut self.state_len);
        let state = &self.state[..len];
        match write_target(apps, target) {
            Some(app) => match app.load_state(state) {
                Ok(()) => Reply::Done,
                Err(refused) => Reply::Error(Error::Refused(refused)),
            },
            None => Reply::Error(Error::NoSuchApp),
        }
    }

    fn send(&mut self, reply: &Reply<'_>) {
        let mut buffer = [0; MAX_FRAME];
        if let Ok(frame) = encode(reply, &mut buffer) {
            self.outgoing.push(frame);
        }
    }
}

/// What an entry looks like now, beside its descriptor.
#[derive(Debug, Clone, Copy)]
struct Shown<'a> {
    label: &'a str,
    active: bool,
    value: Option<oc_core::apps::param::ParamValue>,
}

/// The applet at index `app` in the module's list.
/// The input parameter `param` of `app` takes and the signal that sweeps
/// it, `None` for one that takes none.
fn input_of(app: &dyn App, param: u8) -> Option<(Assign, Option<(MilliVolts, MilliVolts)>)> {
    let index = usize::from(param);
    Some((app.assign(index)?, app.assign_sweep(index)))
}

/// The answer to a request about parameter `param`'s input.
fn input_reply(app: &dyn App, param: u8) -> Reply<'static> {
    match input_of(app, param) {
        Some((assign, sweep)) => Reply::Assign {
            index: param,
            assign,
            sweep,
        },
        None => Reply::Error(Error::Refused(Refused::NoSuch)),
    }
}

/// The applet `target` names: the instance at a position, or the first
/// running instance of a kind or, while none runs, a fresh one put in
/// `fresh`, what it would be once started (journal/D-046). One reference,
/// not a closure per request: a closure would be compiled once per branch.
fn read_target<'a>(
    apps: &'a AppHost,
    target: Target,
    fresh: &'a mut Option<Instance>,
) -> Option<&'a dyn App> {
    match target {
        Target::Instance(position) => apps.at(usize::from(position)),
        Target::App(app) => {
            let id = app_id(app)?;
            match apps.get(id) {
                Some(app) => Some(app),
                None => Some(fresh.insert(Instance::new(id)).app()),
            }
        }
    }
}

/// The running applet `target` names, to change it: a write never reaches
/// a fresh one.
fn write_target(apps: &mut AppHost, target: Target) -> Option<&mut dyn App> {
    match target {
        Target::Instance(position) => apps.at_mut(usize::from(position)),
        Target::App(app) => apps.get_mut(app_id(app)?),
    }
}

/// Whether the instance `target` names may read `assign`'s input: a jack,
/// or a port of another instance that carries a level (journal/D-048). A
/// target that runs nowhere is the write's to refuse.
fn source_offered(apps: &AppHost, target: Target, assign: Assign) -> bool {
    let Some(Source::Lane { slot, port }) = assign.input else {
        return true;
    };
    let reader = match target {
        Target::Instance(position) => Some(usize::from(position)),
        Target::App(app) => app_id(app).and_then(|id| apps.position_of(id)),
    };
    reader.is_none_or(|reader| {
        apps.bus(Some(reader))
            .offers(usize::from(slot), usize::from(port))
    })
}

/// What the instance at `position` runs and where its ports go.
fn instance_reply(apps: &AppHost, position: usize) -> Option<Reply<'static>> {
    let app = apps.at(position)?;
    let route = apps.route_at(position)?;
    let inputs = apps.inputs_at(position)?;
    let ports = app.input_ports();
    let outputs = app.output_ports();
    Some(Reply::Instance {
        position: count(position),
        app: count(app.id().index()),
        focused: apps.focus() == position,
        outputs: core::array::from_fn(|lane| route.output(lane).map(|jack| count(jack.index()))),
        inputs: core::array::from_fn(|port| {
            ports
                .get(port)
                .and_then(|spec| inputs.source(spec.kind, spec.lane))
                .unwrap_or(InputSource::Jack(0))
        }),
        levels: core::array::from_fn(|port| {
            outputs
                .get(port)
                .is_some_and(|port| port.signal == PortSignal::Cv)
        }),
        names: core::array::from_fn(|port| outputs.get(port).map_or("", |port| port.name)),
    })
}

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
