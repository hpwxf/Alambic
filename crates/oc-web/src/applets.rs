//! The applets in the browser (journal/D-043): the page hands each message
//! of a `ListApps` or a `DescribeApp` answer to an [`AppList`] or an
//! [`AppView`], then reads names, values and editors from it, and asks it
//! for the next request. Finding, parsing and writing a value is
//! `oc-probe`'s own code, the command line's, so the page shows and accepts
//! what `cargo probe` does.

use oc_core::fmt::TextBuf;
use oc_core::modulation::{AssignLine, Source};
use oc_probe::apps::{
    INPUT_FIELDS, PortNames, centered, edit_text, input_text, parse_input_field, parse_value,
    source_text, value_text,
};
use oc_probe::client::{
    AppDescription, AppEntry, InputEntry, InputPortEntry, InstanceEntry, OutputPortEntry, SpecEntry,
};
use oc_protocol::{Group, InputSource, ParamKind, ParamValue, Request, Sample, Target};
use wasm_bindgen::prelude::wasm_bindgen;

use crate::{Message, MessageKind, frame};

/// What an applet message carried, kept beside its text for the views.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Applet {
    /// One applet of a list.
    App(AppEntry),
    /// One parameter or observable of a description.
    Spec(Group, SpecEntry),
    /// One name of the choice before it.
    ChoiceName {
        group: Group,
        index: u8,
        name: String,
    },
    /// One action of a description.
    Action(String),
    /// A parameter's value, read or applied.
    Value(ParamValue),
    /// An observable's value, `None` while unknown.
    Observed(Option<ParamValue>),
    /// The CV input of the parameter at that index (journal/D-045).
    Input(u8, InputEntry),
    /// One port of a description: an output, or an input the applet reads
    /// by itself (journal/D-046).
    Port {
        /// An output rather than an input.
        output: bool,
        /// Its name.
        name: String,
        /// For an input, whether it reads a trigger.
        trigger: bool,
        /// For an output, whether it carries a level another instance can
        /// read (journal/D-048).
        level: bool,
    },
    /// One running instance (journal/D-046).
    Instance(InstanceEntry),
}

/// How the page edits a parameter.
#[wasm_bindgen]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Editor {
    /// A text field: a number, or a note's name.
    Number,
    /// A list of names.
    Choice,
    /// On or off.
    Toggle,
}

/// The applets the module runs, from a `ListApps` answer.
#[wasm_bindgen]
#[derive(Debug, Clone, Default)]
pub struct AppList {
    apps: Vec<AppEntry>,
}

#[wasm_bindgen]
impl AppList {
    /// Nothing listed yet.
    #[wasm_bindgen(constructor)]
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Takes one message of the answer; true once it is complete.
    pub fn absorb(&mut self, message: &Message) -> bool {
        if let Some(Applet::App(app)) = &message.applet {
            self.apps.push(app.clone());
        }
        message.kind == MessageKind::Done
    }

    /// How many applets.
    #[wasm_bindgen(getter)]
    #[must_use]
    pub fn len(&self) -> usize {
        self.apps.len()
    }

    /// Whether none was listed.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.apps.is_empty()
    }

    /// Applet `at`'s index, which names it to the module.
    #[must_use]
    pub fn index(&self, at: usize) -> u8 {
        self.apps.get(at).map_or(u8::MAX, |app| app.index)
    }

    /// Applet `at`'s name.
    #[must_use]
    pub fn name(&self, at: usize) -> String {
        self.apps
            .get(at)
            .map_or_else(String::new, |app| app.name.clone())
    }

    /// Whether applet `at` drives the outputs and the screen.
    #[must_use]
    pub fn running(&self, at: usize) -> bool {
        self.apps.get(at).is_some_and(|app| app.running)
    }
}

/// The index of `NOTHING`, what an empty slot runs (journal/D-046).
// The apps fit a byte: the protocol names them by one.
#[allow(clippy::cast_possible_truncation)]
const NOTHING: u8 = oc_core::apps::AppId::Nothing.index() as u8;

/// Every slot, from a `ListInstances` answer (journal/D-046).
#[wasm_bindgen]
#[derive(Debug, Clone, Default)]
pub struct Instances {
    instances: Vec<InstanceEntry>,
}

#[wasm_bindgen]
impl Instances {
    /// Nothing listed yet.
    #[wasm_bindgen(constructor)]
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// The request that lists them, answered by the messages
    /// [`Self::absorb`] takes.
    #[must_use]
    pub fn list_request() -> Vec<u8> {
        frame(&Request::ListInstances)
    }

    /// Takes one message of the answer; true once it is complete.
    pub fn absorb(&mut self, message: &Message) -> bool {
        if let Some(Applet::Instance(instance)) = &message.applet {
            self.instances.push(instance.clone());
        }
        message.kind == MessageKind::Done
    }

    /// How many run.
    #[wasm_bindgen(getter)]
    #[must_use]
    pub fn len(&self) -> usize {
        self.instances.len()
    }

    /// Whether none was listed.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.instances.is_empty()
    }

    /// Instance `at`'s applet index.
    #[must_use]
    pub fn app(&self, at: usize) -> u8 {
        self.instances
            .get(at)
            .map_or(u8::MAX, |instance| instance.app)
    }

    /// Whether instance `at` has the screen and the panel.
    #[must_use]
    pub fn focused(&self, at: usize) -> bool {
        self.instances
            .get(at)
            .is_some_and(|instance| instance.focused)
    }

    /// The output port `port` of instance `at` goes to, 0 for A, -1 for
    /// nowhere.
    #[must_use]
    pub fn output(&self, at: usize, port: usize) -> i32 {
        self.instances
            .get(at)
            .and_then(|instance| instance.outputs.get(port).copied().flatten())
            .map_or(-1, i32::from)
    }

    /// What input port `port` of instance `at` reads, as a code: a jack,
    /// 0 to 3, a port of another instance, or nothing (see [`source_code`]).
    #[must_use]
    pub fn input(&self, at: usize, port: usize) -> u8 {
        self.instances
            .get(at)
            .and_then(|instance| instance.inputs.get(port).copied())
            .map_or(0, source_code)
    }

    /// The code of `OFF`, an input that reads nothing: the first of an
    /// input's choices.
    #[must_use]
    pub fn off_code() -> u8 {
        OFF_CODE
    }

    /// The codes of the ports an input of the instance at `own` (-1: none)
    /// can read besides the jacks, slot by slot: those of the other
    /// instances that carry a level (journal/D-048).
    #[must_use]
    pub fn lane_codes(&self, own: i32) -> Vec<u8> {
        let own = usize::try_from(own).ok();
        self.instances
            .iter()
            .filter(|instance| Some(usize::from(instance.position)) != own)
            .flat_map(|instance| {
                let at = usize::from(instance.position);
                instance
                    .levels
                    .iter()
                    .enumerate()
                    .filter(|&(_, &level)| level)
                    .filter_map(move |(port, _)| InputSource::lane(at, port))
            })
            .map(source_code)
            .collect()
    }

    /// How the routing page names what input `code` reads, for a port of
    /// another instance: its slot and name, `1:BURST`.
    #[must_use]
    pub fn source_label(&self, code: u8) -> String {
        match code_source(code) {
            Some(InputSource::Lane { slot, port }) => {
                source_text(Source::Lane { slot, port }, &PortNames::of(&self.instances))
            }
            _ => String::new(),
        }
    }

    /// The ports of the other instances an input of the instance at `own`
    /// (-1: none) can read, as the module's submenu names them (`2:NOTE`):
    /// every output port that carries a level, slot by slot
    /// (journal/D-048).
    #[must_use]
    pub fn sources(&self, own: i32) -> Vec<String> {
        let own = usize::try_from(own).ok();
        let ports = PortNames::of(&self.instances);
        self.instances
            .iter()
            .enumerate()
            .filter(|&(_, instance)| Some(usize::from(instance.position)) != own)
            .flat_map(|(_, instance)| {
                let at = usize::from(instance.position);
                instance
                    .levels
                    .iter()
                    .enumerate()
                    .filter(|&(_, &level)| level)
                    .filter_map(move |(port, _)| Source::lane(at, port))
            })
            .map(|source| source_text(source, &ports))
            .collect()
    }

    /// Whether the slot `at` is empty: it runs `NOTHING` (journal/D-046).
    #[must_use]
    pub fn is_empty_slot(&self, at: usize) -> bool {
        self.app(at) == NOTHING
    }

    /// Puts a fresh applet `app` in the slot at `position`.
    #[must_use]
    pub fn place_request(app: u8, position: u8) -> Vec<u8> {
        frame(&Request::SelectApp {
            app,
            instance: Some(position),
        })
    }

    /// Empties the slot at `position`: `NOTHING` in it.
    #[must_use]
    pub fn remove_request(position: u8) -> Vec<u8> {
        Self::place_request(NOTHING, position)
    }

    /// Shows the instance at `position` on the module.
    #[must_use]
    pub fn focus_request(position: u8) -> Vec<u8> {
        frame(&Request::Focus { instance: position })
    }

    /// Sends output port `port` of the instance at `position` to `output`,
    /// 0 for A, or nowhere for a negative one.
    #[must_use]
    pub fn route_request(position: u8, port: u8, output: i32) -> Vec<u8> {
        frame(&Request::SetRoute {
            instance: position,
            port,
            output: u8::try_from(output).ok(),
        })
    }

    /// Plugs input port `port` of the instance at `position` into what
    /// `code` names: a jack, 0 to 3, or a port of another instance.
    #[must_use]
    pub fn input_request(position: u8, port: u8, code: u8) -> Vec<u8> {
        frame(&Request::SetInput {
            instance: position,
            port,
            source: code_source(code).unwrap_or(InputSource::Jack(0)),
        })
    }
}

/// The code of `OFF` (see [`source_code`]).
const OFF_CODE: u8 = u8::MAX;

/// How the page names an input's source to the script, in one number: a
/// jack 0 to 3, a port of another instance 4 and up, four per slot,
/// [`OFF_CODE`] for nothing.
fn source_code(source: InputSource) -> u8 {
    match source {
        InputSource::Jack(jack) => jack,
        InputSource::Lane { slot, port } => 4 + slot * 4 + port,
        InputSource::Off => OFF_CODE,
    }
}

/// The source `code` names (see [`source_code`]), `None` past the last.
fn code_source(code: u8) -> Option<InputSource> {
    if code == OFF_CODE {
        return Some(InputSource::Off);
    }
    match code.checked_sub(4) {
        None => Some(InputSource::Jack(code)),
        Some(lane) => InputSource::lane(usize::from(lane / 4), usize::from(lane % 4)),
    }
}

/// One applet's parameters, observables and actions, from a `DescribeApp`
/// answer, kept up to date by the values that come back.
#[wasm_bindgen]
#[derive(Debug, Clone)]
pub struct AppView {
    app: u8,
    /// Where its requests go: the first instance of `app`, or one instance
    /// (journal/D-046).
    target: Target,
    description: AppDescription,
    broken: bool,
    /// Which parameters the last refresh found changed.
    moved: Vec<bool>,
    /// The value the last set asked for, to tell whether it was narrowed.
    requested: Option<ParamValue>,
    /// Whether the last value absorbed differs from the one asked for.
    narrowed: bool,
    /// The other instances' port names, by which an input names one
    /// (`1:NOTE`, journal/D-048).
    ports: PortNames,
}

#[wasm_bindgen]
impl AppView {
    /// Applet `app`, nothing described yet.
    #[wasm_bindgen(constructor)]
    #[must_use]
    pub fn new(app: u8) -> Self {
        Self {
            app,
            target: Target::App(app),
            description: AppDescription::default(),
            broken: false,
            moved: Vec::new(),
            requested: None,
            narrowed: false,
            ports: PortNames::default(),
        }
    }

    /// The slot of the instance its requests reach, -1 for the first
    /// instance of its applet, wherever it runs.
    #[wasm_bindgen(getter)]
    #[must_use]
    pub fn position(&self) -> i32 {
        match self.target {
            Target::Instance(position) => i32::from(position),
            Target::App(_) => -1,
        }
    }

    /// The instance at `position`, of applet `app`, nothing described yet:
    /// its requests reach that instance alone (journal/D-046).
    #[must_use]
    pub fn of_instance(position: u8, app: u8) -> Self {
        Self {
            target: Target::Instance(position),
            ..Self::new(app)
        }
    }

    /// Takes the port names of the instances `instances` lists, by which
    /// an input names another instance's port (`1:NOTE`, journal/D-048).
    pub fn take_ports(&mut self, instances: &Instances) {
        self.ports = PortNames::of(&instances.instances);
    }

    /// The output ports it writes, in the order of its lanes.
    #[wasm_bindgen(getter)]
    #[must_use]
    pub fn output_count(&self) -> usize {
        self.description.outputs.len()
    }

    /// Output port `at`'s name.
    #[must_use]
    pub fn output_name(&self, at: usize) -> String {
        self.description
            .outputs
            .get(at)
            .map_or_else(String::new, |port| port.name.clone())
    }

    /// The inputs it reads by itself.
    #[wasm_bindgen(getter)]
    #[must_use]
    pub fn input_port_count(&self) -> usize {
        self.description.inputs.len()
    }

    /// Input port `at`'s name.
    #[must_use]
    pub fn input_port_name(&self, at: usize) -> String {
        self.description
            .inputs
            .get(at)
            .map_or_else(String::new, |port| port.name.clone())
    }

    /// Whether input port `at` reads a trigger rather than a CV.
    #[must_use]
    pub fn input_port_trigger(&self, at: usize) -> bool {
        self.description
            .inputs
            .get(at)
            .is_some_and(|port| port.trigger)
    }

    /// The request that describes it, answered by the messages
    /// [`Self::absorb`] takes.
    #[must_use]
    pub fn describe_request(&self) -> Vec<u8> {
        frame(&Request::DescribeApp { app: self.target })
    }

    /// Takes one message of the description; true once it is complete.
    pub fn absorb(&mut self, message: &Message) -> bool {
        match &message.applet {
            Some(Applet::Spec(group, spec)) => self.description.add_spec(*group, spec.clone()),
            Some(Applet::ChoiceName { group, index, name }) => {
                if self
                    .description
                    .add_choice_name(*group, *index, name.clone())
                    .is_err()
                {
                    self.broken = true;
                }
            }
            Some(Applet::Action(name)) => self.description.actions.push(name.clone()),
            Some(Applet::Input(index, input)) => {
                self.broken |= self.description.add_input(*index, *input).is_err();
            }
            Some(Applet::Port {
                output: true,
                name,
                level,
                ..
            }) => {
                self.description.outputs.push(OutputPortEntry {
                    name: name.clone(),
                    level: *level,
                });
            }
            Some(Applet::Port {
                output: false,
                name,
                trigger,
                ..
            }) => self.description.inputs.push(InputPortEntry {
                name: name.clone(),
                trigger: *trigger,
            }),
            _ => {}
        }
        message.kind == MessageKind::Done
    }

    /// The request that reads every value again, after a set that may have
    /// moved other parameters with it (journal/D-043): answered by the
    /// messages [`Self::absorb_refresh`] takes.
    #[must_use]
    pub fn refresh_request(&self) -> Vec<u8> {
        self.describe_request()
    }

    /// Takes one message of a refresh: the values it carries replace the
    /// ones kept, and [`Self::param_moved`] says which changed; true once it
    /// is complete.
    pub fn absorb_refresh(&mut self, message: &Message) -> bool {
        if self.moved.len() != self.description.params.len() {
            self.moved = vec![false; self.description.params.len()];
        }
        if let Some(Applet::Spec(group, fresh)) = &message.applet {
            let list = match group {
                Group::Param => &mut self.description.params,
                Group::Observable => &mut self.description.observables,
            };
            if let Some(at) = list.iter().position(|spec| spec.index == fresh.index) {
                if *group == Group::Param && list[at].value != fresh.value {
                    self.moved[at] = true;
                }
                list[at].value = fresh.value;
                list[at].label.clone_from(&fresh.label);
                list[at].active = fresh.active;
            }
        }
        if let Some(Applet::Input(index, input)) = &message.applet {
            let params = &mut self.description.params;
            if let Some(spec) = params.iter_mut().find(|spec| spec.index == *index) {
                spec.input = Some(*input);
            }
        }
        message.kind == MessageKind::Done
    }

    /// Whether parameter `at` changed in the last refresh; cleared by
    /// [`Self::settle`].
    #[must_use]
    pub fn param_moved(&self, at: usize) -> bool {
        self.moved.get(at).copied().unwrap_or(false)
    }

    /// Forgets which parameters the last refresh found changed, before the
    /// next set.
    pub fn settle(&mut self) {
        self.moved.clear();
    }

    /// Whether the description came out of order, and cannot be trusted.
    #[wasm_bindgen(getter)]
    #[must_use]
    pub fn broken(&self) -> bool {
        self.broken
    }

    /// How many parameters.
    #[wasm_bindgen(getter)]
    #[must_use]
    pub fn param_count(&self) -> usize {
        self.description.params.len()
    }

    /// Parameter `at`'s name, which finds it whatever its label.
    #[must_use]
    pub fn param_name(&self, at: usize) -> String {
        param(&self.description, at).map_or_else(String::new, |spec| spec.name.clone())
    }

    /// How the module's screen labels parameter `at` now (`SPAN` for
    /// `RATCHET`'s `FIRST` in `MODE SPAN`); a refresh keeps it current.
    #[must_use]
    pub fn param_label(&self, at: usize) -> String {
        param(&self.description, at).map_or_else(String::new, |spec| spec.label.clone())
    }

    /// Parameter `at`'s value as the module writes it (`2.0 MS`, `SPAN`).
    #[must_use]
    pub fn param_text(&self, at: usize) -> String {
        param(&self.description, at).map_or_else(String::new, shown)
    }

    /// Parameter `at`'s value as one types it back (`-3.0`).
    #[must_use]
    pub fn param_edit_text(&self, at: usize) -> String {
        param(&self.description, at)
            .and_then(|spec| spec.value.map(|value| edit_text(spec, value)))
            .unwrap_or_default()
    }

    /// The values parameter `at` takes, as text (`1..32`, `FIRST|SPAN`).
    #[must_use]
    pub fn param_range(&self, at: usize) -> String {
        param(&self.description, at).map_or_else(String::new, range)
    }

    /// How the page edits parameter `at`.
    #[must_use]
    pub fn param_editor(&self, at: usize) -> Editor {
        match param(&self.description, at).map(|spec| spec.kind) {
            Some(ParamKind::Choice { .. }) => Editor::Choice,
            Some(ParamKind::Toggle) => Editor::Toggle,
            _ => Editor::Number,
        }
    }

    /// A choice's names, in order; empty for another parameter.
    #[must_use]
    pub fn param_choices(&self, at: usize) -> Vec<String> {
        param(&self.description, at).map_or_else(Vec::new, |spec| spec.choices.clone())
    }

    /// Whether parameter `at` acts now; the page greys a face of a setting
    /// the screen does not show. A refresh keeps it current.
    #[must_use]
    pub fn param_active(&self, at: usize) -> bool {
        param(&self.description, at).is_some_and(|spec| spec.active)
    }

    /// Whether parameter `at` takes a CV input.
    #[must_use]
    pub fn param_assignable(&self, at: usize) -> bool {
        param(&self.description, at).is_some_and(|spec| spec.assignable)
    }

    /// The CV input of parameter `at` as the module's main screen marks it
    /// (`+CV2`), `CV` while it takes none yet, empty for a parameter that
    /// takes none: what opens its input panel.
    #[must_use]
    pub fn input_mark(&self, at: usize) -> String {
        let Some(spec) = param(&self.description, at).filter(|spec| spec.assignable) else {
            return String::new();
        };
        let mut mark = TextBuf::<8>::new();
        if let Some(input) = &spec.input {
            let port_name = match input.assign.input {
                Some(Source::Lane { slot, port }) => {
                    self.ports.name(usize::from(slot), usize::from(port))
                }
                _ => None,
            };
            input.assign.write(&mut mark, port_name);
        }
        if mark.as_str().is_empty() {
            "CV".to_owned()
        } else {
            mark.as_str().to_owned()
        }
    }

    /// The CV input of parameter `at` on one line, as `cargo probe assign`
    /// prints it; empty for a parameter that takes none.
    #[must_use]
    pub fn input_text(&self, at: usize) -> String {
        input(&self.description, at)
            .map_or_else(String::new, |input| input_text(&input.assign, &self.ports))
    }

    /// The fields of an input panel, the same for every assignable
    /// parameter of every applet (journal/D-045): `input`, `scale`,
    /// `shift`, `min`, `max`, `effect`.
    #[must_use]
    pub fn input_fields() -> Vec<String> {
        INPUT_FIELDS.iter().map(|&field| field.to_owned()).collect()
    }

    /// Field `field` of parameter `at`'s input as the module's submenu
    /// writes it (`x0.50`, `+0.25V`, `ADD`), which reads back as typed.
    #[must_use]
    pub fn input_field_text(&self, at: usize, field: &str) -> String {
        let Some((input, line)) = input(&self.description, at).zip(input_line(field)) else {
            return String::new();
        };
        match (line, input.assign.input) {
            (AssignLine::Input, Some(source)) => source_text(source, &self.ports),
            _ => input.assign.field(line).as_str().to_owned(),
        }
    }

    /// The values field `field` takes when it is a list (`--`, `CV1`… for
    /// the input; `SET`, `ADD`, `MUL` for the effect), empty when it is
    /// typed.
    #[must_use]
    pub fn input_choices(field: &str) -> Vec<String> {
        let names: &[&str] = match field {
            "input" => &["--", "CV1", "CV2", "CV3", "CV4"],
            "effect" => &["SET", "ADD", "MUL"],
            _ => &[],
        };
        names.iter().map(|&name| name.to_owned()).collect()
    }

    /// The request that sets field `field` of parameter `at`'s input to
    /// what `text` says, the other fields kept, as `cargo probe assign`
    /// reads it.
    ///
    /// # Errors
    ///
    /// What the text should have looked like, or a parameter without an
    /// input.
    pub fn assign_request(&self, at: usize, field: &str, text: &str) -> Result<Vec<u8>, String> {
        let spec = param(&self.description, at).ok_or("no such parameter")?;
        let input = spec.input.ok_or("it takes no CV input")?;
        let assign = parse_input_field(input.assign, field, text, &self.ports)?;
        Ok(frame(&Request::SetAssign {
            app: self.target,
            param: spec.index,
            assign,
        }))
    }

    /// The request that sets parameter `at`'s input to carry what it read
    /// over `readings` onto the range the parameter uses, as the submenu's
    /// `CENTER` and `cargo probe assign --center` do: the same
    /// [`centered`], from the stream (journal/D-045).
    ///
    /// # Errors
    ///
    /// A parameter without an input, or readings `CENTER` cannot work from:
    /// none, or a signal that hardly moves.
    pub fn center_request(&self, at: usize, readings: &Readings) -> Result<Vec<u8>, String> {
        let spec = param(&self.description, at).ok_or("no such parameter")?;
        let input = spec.input.as_ref().ok_or("it takes no CV input")?;
        let assign = centered(input, &readings.samples)?;
        Ok(frame(&Request::SetAssign {
            app: self.target,
            param: spec.index,
            assign,
        }))
    }

    /// Takes the input the module answered for parameter `at` and keeps
    /// it; its text, or an empty one if the message carried none.
    pub fn absorb_input(&mut self, at: usize, message: &Message) -> String {
        let Some(Applet::Input(_, input)) = message.applet else {
            return String::new();
        };
        let Some(spec) = self.description.params.get_mut(at) else {
            return String::new();
        };
        spec.input = Some(input);
        input_text(&input.assign, &self.ports)
    }

    /// The request that sets parameter `at` to what `text` says, as
    /// `cargo probe set` reads it.
    ///
    /// # Errors
    ///
    /// What the text should have looked like.
    pub fn set_request(&mut self, at: usize, text: &str) -> Result<Vec<u8>, String> {
        let spec = param(&self.description, at).ok_or("no such parameter")?;
        let value = parse_value(spec, text)?;
        self.requested = Some(value);
        Ok(frame(&Request::SetParam {
            app: self.target,
            param: spec.index,
            value,
        }))
    }

    /// Takes the value the module answered for parameter `at` and keeps it;
    /// its text, or an empty one if the message carried none.
    pub fn absorb_param(&mut self, at: usize, message: &Message) -> String {
        let Some(Applet::Value(value)) = message.applet else {
            return String::new();
        };
        let Some(spec) = self.description.params.get_mut(at) else {
            return String::new();
        };
        spec.value = Some(value);
        self.narrowed = self.requested.take().is_some_and(|asked| asked != value);
        value_text(spec, value)
    }

    /// Whether the module applied another value than the last set asked
    /// for: another setting narrowed it.
    #[wasm_bindgen(getter)]
    #[must_use]
    pub fn narrowed(&self) -> bool {
        self.narrowed
    }

    /// How many observables.
    #[wasm_bindgen(getter)]
    #[must_use]
    pub fn observable_count(&self) -> usize {
        self.description.observables.len()
    }

    /// Observable `at`'s name.
    #[must_use]
    pub fn observable_name(&self, at: usize) -> String {
        observable(&self.description, at).map_or_else(String::new, |spec| spec.name.clone())
    }

    /// Observable `at`'s value as the module writes it; `--` while unknown.
    #[must_use]
    pub fn observable_text(&self, at: usize) -> String {
        observable(&self.description, at).map_or_else(String::new, shown)
    }

    /// The request that reads observable `at` now.
    #[must_use]
    pub fn observe_request(&self, at: usize) -> Vec<u8> {
        let index = observable(&self.description, at).map_or(u8::MAX, |spec| spec.index);
        frame(&Request::Observe {
            app: self.target,
            index,
        })
    }

    /// Takes the value the module answered for observable `at` and keeps
    /// it; its text.
    pub fn absorb_observed(&mut self, at: usize, message: &Message) -> String {
        let Some(Applet::Observed(value)) = message.applet else {
            return String::new();
        };
        let Some(spec) = self.description.observables.get_mut(at) else {
            return String::new();
        };
        spec.value = value;
        shown(spec)
    }

    /// The actions' names, in order.
    #[wasm_bindgen(getter)]
    #[must_use]
    pub fn actions(&self) -> Vec<String> {
        self.description.actions.clone()
    }

    /// The request that does action `at`.
    #[must_use]
    pub fn act_request(&self, at: usize) -> Vec<u8> {
        frame(&Request::Act {
            app: self.target,
            action: u8::try_from(at).unwrap_or(u8::MAX),
        })
    }

    /// The request that puts this applet in the slot shown on the module.
    #[must_use]
    pub fn select_request(&self) -> Vec<u8> {
        frame(&Request::SelectApp {
            app: self.app,
            instance: None,
        })
    }
}

/// What the stream carried while an input panel's `CENTER` listened: the
/// samples over so many seconds of the module's clock, kept as
/// `cargo probe assign --center` keeps them.
#[wasm_bindgen]
#[derive(Debug, Clone, Default)]
pub struct Readings {
    micros: u64,
    samples: Vec<Sample>,
}

#[wasm_bindgen]
impl Readings {
    /// Nothing read yet, to listen for `seconds` of the module's clock.
    #[wasm_bindgen(constructor)]
    #[must_use]
    pub fn new(seconds: u32) -> Self {
        Self {
            micros: u64::from(seconds) * 1_000_000,
            samples: Vec::new(),
        }
    }

    /// Takes one message of the stream; true once the samples span the
    /// time asked for, measured on the module's clock as `cargo probe`
    /// measures it. The sample that reaches the end is not kept.
    pub fn absorb(&mut self, message: &Message) -> bool {
        let Some(sample) = message.sample else {
            return false;
        };
        if let Some(first) = self.samples.first()
            && sample.micros.saturating_sub(first.micros) >= self.micros
        {
            return true;
        }
        self.samples.push(sample);
        false
    }

    /// How many samples were kept.
    #[wasm_bindgen(getter)]
    #[must_use]
    pub fn len(&self) -> usize {
        self.samples.len()
    }

    /// Whether none was.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.samples.is_empty()
    }
}

fn param(description: &AppDescription, at: usize) -> Option<&SpecEntry> {
    description.params.get(at)
}

fn input(description: &AppDescription, at: usize) -> Option<&InputEntry> {
    param(description, at).and_then(|spec| spec.input.as_ref())
}

/// The submenu's field a panel field names.
fn input_line(field: &str) -> Option<AssignLine> {
    Some(match field {
        "input" => AssignLine::Input,
        "scale" => AssignLine::Scale,
        "shift" => AssignLine::Shift,
        "min" => AssignLine::Min,
        "max" => AssignLine::Max,
        "effect" => AssignLine::Effect,
        _ => return None,
    })
}

fn observable(description: &AppDescription, at: usize) -> Option<&SpecEntry> {
    description.observables.get(at)
}

/// An entry's value as text, `--` while unknown.
fn shown(spec: &SpecEntry) -> String {
    spec.value
        .map_or_else(|| "--".to_owned(), |value| value_text(spec, value))
}

/// The values an entry takes, as text.
fn range(spec: &SpecEntry) -> String {
    match spec.kind {
        ParamKind::Number { min, max, .. } => format!(
            "{} .. {}",
            value_text(spec, ParamValue::Number(min)),
            value_text(spec, ParamValue::Number(max))
        ),
        ParamKind::Choice { .. } => spec.choices.join(" | "),
        ParamKind::Toggle => "ON | OFF".to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use oc_core::apps::ratchet::RatchetApp;
    use oc_core::apps::{AppId, MAX_INSTANCES};
    use oc_core::testing::{MockEngine, mock_engine};
    use oc_protocol::server::{Identity, Server};
    use oc_protocol::{Request, VERSION};

    use super::{AppList, AppView, Editor, Readings};
    use crate::{Message, ProbeReader, frame};

    /// A module end: the server over a mock engine, and the page's reader.
    struct Module {
        server: Server,
        engine: MockEngine,
        reader: ProbeReader,
    }

    impl Module {
        fn new() -> Self {
            let mut module = Self {
                server: Server::new(Identity {
                    firmware: "0.6.0",
                    build: "abc1234",
                    can_reboot: false,
                }),
                engine: mock_engine(0),
                reader: ProbeReader::new(),
            };
            module.send(&frame(&Request::Hello { protocol: VERSION }));
            module
        }

        /// A module running `app`: an applet is set only while it runs
        /// (journal/D-046).
        fn running(app: AppId) -> Self {
            let mut module = Self::new();
            module.engine.select_app(app);
            module
        }

        /// What the page receives for `bytes`.
        fn send(&mut self, bytes: &[u8]) -> Vec<Message> {
            self.server.receive(bytes, &mut self.engine);
            let queue = self.server.outgoing();
            let waiting = queue.peek(usize::MAX).to_vec();
            queue.consume(waiting.len());
            self.reader.push(&waiting)
        }

        /// What the page receives over one tick of a millisecond, `CV2` at
        /// `millivolts`: a sample when a stream asks for one.
        fn tick(&mut self, millivolts: i32) -> Vec<Message> {
            use oc_core::platform::CvChannel;

            self.engine.parts_mut().0.patch(CvChannel::Two, millivolts);
            self.engine.clock().advance(1_000);
            let report = self.engine.tick();
            self.server.after_tick(&report, &mut self.engine);
            let queue = self.server.outgoing();
            let waiting = queue.peek(usize::MAX).to_vec();
            queue.consume(waiting.len());
            self.reader.push(&waiting)
        }
    }

    #[test]
    fn the_list_names_every_applet_and_the_running_one() {
        let mut module = Module::new();
        let mut list = AppList::new();
        let messages = module.send(&frame(&Request::ListApps));
        let done = messages.iter().map(|message| list.absorb(message)).last();
        assert_eq!(done, Some(true));
        assert_eq!(list.len(), AppId::ALL.len());
        assert_eq!(list.name(3), "RATCHET");
        assert!(list.running(0) && !list.running(3));
    }

    #[test]
    fn a_view_shows_edits_and_sets_as_the_command_line_does() {
        let mut module = Module::running(AppId::Ratchet);
        let ratchet = u8::try_from(AppId::Ratchet.index()).unwrap();
        let mut view = AppView::new(ratchet);
        for message in module.send(&view.describe_request()) {
            if view.absorb(&message) {
                break;
            }
        }
        assert!(!view.broken());
        assert_eq!(view.param_name(4), "SHAPE");
        assert_eq!(view.param_text(4), "0.0");
        assert_eq!(view.param_edit_text(4), "0.0");
        assert_eq!(view.param_range(4), "-3.0 .. +3.0");
        assert_eq!(view.param_editor(8), Editor::Choice);
        assert_eq!(view.param_choices(8), ["FIRST", "SPAN"]);
        assert!(view.param_assignable(0) && !view.param_assignable(1));
        assert_eq!(view.actions(), ["BURST"]);
        assert_eq!(view.param_label(2), "FIRST");
        assert!(
            view.param_active(2) && !view.param_active(3),
            "SYNC OFF: FIRST acts"
        );

        // SYNC ON: DIVISION acts instead, and a refresh says so.
        module.send(&view.set_request(7, "on").unwrap());
        for message in module.send(&view.refresh_request()) {
            if view.absorb_refresh(&message) {
                break;
            }
        }
        assert!(!view.param_active(2) && view.param_active(3));
        module.send(&view.set_request(7, "off").unwrap());

        // MODE SPAN: the time reads SPAN on the screen, and so on the page.
        module.send(&view.set_request(8, "span").unwrap());
        for message in module.send(&view.refresh_request()) {
            if view.absorb_refresh(&message) {
                break;
            }
        }
        assert_eq!(
            (view.param_name(2).as_str(), view.param_label(2).as_str()),
            ("FIRST", "SPAN")
        );
        module.send(&view.set_request(8, "first").unwrap());

        // GATE off puts COUNT at its last: a refresh shows it, as the module does.
        let count = 0;
        let gate = 1;
        view.settle();
        module.send(&view.set_request(gate, "on").unwrap());
        module.send(&view.set_request(gate, "off").unwrap());
        for message in module.send(&view.refresh_request()) {
            if view.absorb_refresh(&message) {
                break;
            }
        }
        assert_eq!(view.param_text(count), "32");
        assert!(view.param_moved(count), "COUNT moved with GATE");
        assert!(!view.param_moved(4), "SHAPE did not");

        let request = view.set_request(4, "-1.5").unwrap();
        let answer = module.send(&request);
        assert_eq!(view.absorb_param(4, &answer[0]), "-1.5");
        assert!(!view.narrowed());
        assert_eq!(
            module.engine.app::<RatchetApp>().settings().curve_tenths,
            -15
        );
        let answer = module.send(&view.set_request(4, "1.5").unwrap());
        assert_eq!(view.absorb_param(4, &answer[0]), "+1.5");
        assert!(!view.narrowed(), "1.5 typed, +1.5 shown: the same value");
        let answer = module.send(&view.set_request(2, "5000").unwrap());
        view.absorb_param(2, &answer[0]);
        assert!(
            view.narrowed(),
            "FIRST is two seconds at most in MODE FIRST"
        );
        assert!(view.set_request(4, "steep").is_err());

        let rate = module.send(&view.observe_request(0));
        assert_eq!(view.absorb_observed(0, &rate[0]), "--");
        module.send(&view.select_request());
        assert_eq!(module.engine.current_app(), AppId::Ratchet);
    }

    #[test]
    fn every_input_panel_shows_and_sets_its_fields_as_the_command_line_does() {
        use oc_core::apps::supersaw::{Setting, SupersawApp};
        use oc_core::platform::CvChannel;
        use oc_protocol::Effect;

        let mut module = Module::running(AppId::Supersaw);
        let supersaw = u8::try_from(AppId::Supersaw.index()).unwrap();
        let mut view = AppView::new(supersaw);
        for message in module.send(&view.describe_request()) {
            if view.absorb(&message) {
                break;
            }
        }
        assert!(!view.broken());
        assert_eq!(view.param_name(0), "NOTE");
        assert_eq!(view.input_mark(0), "+CV1", "CV1 on NOTE at start");
        assert_eq!(view.input_mark(1), "CV", "DETUNE takes one, none yet");
        let stereo = view.param_count() - 1;
        assert_eq!(view.input_mark(stereo), "", "STEREO takes none");
        assert!(view.assign_request(stereo, "input", "cv2").is_err());

        // Every field reads back as the panel shows it.
        let fields = AppView::input_fields();
        assert_eq!(fields, ["input", "scale", "shift", "min", "max", "effect"]);
        for field in &fields {
            let shown = view.input_field_text(0, field);
            let request = view.assign_request(0, field, &shown).unwrap();
            let [message] = module.send(&request).try_into().unwrap();
            assert_eq!(
                view.absorb_input(0, &message),
                "CV1 x1.00 +0.00V -10.0..+10.0 ADD",
                "{field} {shown}"
            );
        }
        assert_eq!(AppView::input_choices("effect"), ["SET", "ADD", "MUL"]);
        assert_eq!(AppView::input_choices("scale"), Vec::<String>::new());

        for (field, text) in [("input", "CV3"), ("effect", "MUL"), ("shift", "+0.25V")] {
            let request = view.assign_request(1, field, text).unwrap();
            let [message] = module.send(&request).try_into().unwrap();
            view.absorb_input(1, &message);
        }
        assert_eq!(view.input_mark(1), "*CV3");
        assert_eq!(view.input_text(1), "CV3 x1.00 +0.25V -10.0..+10.0 MUL");
        let detune = *module
            .engine
            .app::<SupersawApp>()
            .assign(Setting::Detune)
            .unwrap();
        assert_eq!(
            (detune.input, detune.effect, detune.shift_mv),
            (Some(CvChannel::Three.into()), Effect::Mul, 250)
        );
        assert!(
            view.assign_request(1, "scale", "x2.5").is_ok(),
            "the module refuses it, not the page"
        );
        assert!(view.assign_request(1, "gain", "1").is_err());
    }

    #[test]
    fn center_fits_an_input_as_the_command_line_does() {
        use oc_core::apps::supersaw::{Setting, SupersawApp};
        use oc_probe::apps::centered;

        /// A second of the stream, `CV2` at `level(tick)`.
        fn listen(module: &mut Module, level: impl Fn(i32) -> i32) -> Readings {
            module.send(&frame(&Request::Stream { every_ticks: 1 }));
            let mut readings = Readings::new(1);
            let mut full = false;
            for step in 0.. {
                for message in module.tick(level(step)) {
                    full |= readings.absorb(&message);
                }
                if full {
                    break;
                }
            }
            module.send(&frame(&Request::Stream { every_ticks: 0 }));
            readings
        }

        let mut module = Module::running(AppId::Supersaw);
        let mut view = AppView::new(u8::try_from(AppId::Supersaw.index()).unwrap());
        for message in module.send(&view.describe_request()) {
            if view.absorb(&message) {
                break;
            }
        }
        let detune = 1;
        for (field, text) in [("input", "CV2"), ("effect", "SET")] {
            let request = view.assign_request(detune, field, text).unwrap();
            let [message] = module.send(&request).try_into().unwrap();
            view.absorb_input(detune, &message);
        }

        // A triangle from 1 V to 3 V: the fit is the command line's.
        let readings = listen(&mut module, |step| 1_000 + (step % 200 - 100).abs() * 20);
        let before = view.description.params[detune].input.unwrap();
        let fitted = centered(&before, &readings.samples).unwrap();
        assert_ne!(fitted, before.assign);
        let request = view.center_request(detune, &readings).unwrap();
        let [message] = module.send(&request).try_into().unwrap();
        view.absorb_input(detune, &message);
        let applied = module.engine.app::<SupersawApp>().assign(Setting::Detune);
        assert_eq!(applied, Some(&fitted));

        // A still input is refused, as on the module.
        let still = listen(&mut module, |_| 2_000);
        let refused = view.center_request(detune, &still).unwrap_err();
        assert!(refused.contains("moved 0 mV"), "{refused}");
    }

    #[test]
    fn instances_are_listed_described_and_routed_as_the_command_line_does() {
        use super::Instances;
        use crate::MessageKind;

        let mut module = Module::running(AppId::Ratchet);
        let supersaw = u8::try_from(AppId::Supersaw.index()).unwrap();
        let [placed] = module
            .send(&Instances::place_request(supersaw, 1))
            .try_into()
            .unwrap();
        assert_eq!(placed.kind, MessageKind::Done);
        module.send(&Instances::focus_request(1));

        let mut instances = Instances::new();
        let done = module
            .send(&Instances::list_request())
            .iter()
            .map(|message| instances.absorb(message))
            .last();
        assert_eq!((done, instances.len()), (Some(true), MAX_INSTANCES));
        assert!(instances.is_empty_slot(3) && !instances.is_empty_slot(1));
        assert_eq!(instances.app(1), supersaw);
        assert!(instances.focused(1) && !instances.focused(0));
        assert_eq!((instances.output(1, 0), instances.output(1, 2)), (2, -1));

        let mut view = AppView::of_instance(1, supersaw);
        for message in module.send(&view.describe_request()) {
            if view.absorb(&message) {
                break;
            }
        }
        assert_eq!(view.output_count(), 3);
        assert_eq!(view.output_name(2), "SUB");
        let mut ratchet = AppView::of_instance(0, u8::try_from(AppId::Ratchet.index()).unwrap());
        for message in module.send(&ratchet.describe_request()) {
            if ratchet.absorb(&message) {
                break;
            }
        }
        assert_eq!(
            (
                ratchet.input_port_count(),
                ratchet.input_port_name(0).as_str()
            ),
            (1, "CLOCK")
        );
        assert!(ratchet.input_port_trigger(0));

        // RATCHET frees B; SUPERSAW's SUB takes it.
        module.send(&Instances::route_request(0, 1, -1));
        let [routed] = module
            .send(&Instances::route_request(1, 2, 1))
            .try_into()
            .unwrap();
        let mut after = Instances::new();
        after.absorb(&routed);
        assert_eq!(after.output(0, 2), 1, "SUB on B");
    }
}
