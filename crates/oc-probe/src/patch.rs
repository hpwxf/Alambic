//! The module's patch in a file (journal/D-050): which applet runs in each
//! slot, where its ports go, its settings, its CV inputs and what it keeps
//! beside them, read through the probe and written back the same way.
//!
//! Everything is keyed by name and written as the probe writes it (`"16"`,
//! `"MAJOR"`, `"1:THRU"`, `"x0.50"`), so that a person can read and edit the
//! file and a setting added to an applet does not shift the others. The
//! calibration and the system settings are never part of it.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::io::{Read, Write};

use oc_core::apps::melody::sequence::{MAX_STEPS, Steps};
use oc_core::modulation::{Assign, AssignLine};
use oc_protocol::{InputSource, Target};
use serde::{Deserialize, Serialize};

use crate::apps::{
    INPUT_FIELDS, PortNames, edit_text, find_app, input_source_text, parse_input_field,
    parse_input_source, parse_output, parse_value, source_text,
};
use crate::client::{AppEntry, Client, InstanceEntry};

/// The version of the file's layout, raised on a change a reader of the
/// last one would misread.
pub const FORMAT: u32 = 1;

/// The applets' slots, as the file numbers them, from 1.
const SLOTS: u8 = 4;

/// The jacks an output port goes to, by their index.
const OUTPUT_NAMES: [&str; 4] = ["A", "B", "C", "D"];

/// The name of the applet whose own state this file writes as steps.
const MELODY: &str = "MELODY";

/// The whole patch.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Patch {
    /// The file's layout, [`FORMAT`].
    pub format: u32,
    /// The slot shown, from 1.
    pub focus: u8,
    /// Each slot that runs an applet; the others are empty.
    #[serde(rename = "slot", default)]
    pub slots: Vec<SlotPatch>,
}

/// One slot and its applet.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SlotPatch {
    /// The slot, from 1.
    pub slot: u8,
    /// The applet's name, as the app menu lists it.
    pub app: String,
    /// Each output port's jack, `A` to `D` or `OFF`.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub outputs: BTreeMap<String, String>,
    /// What each input port reads: a jack, another applet's port, `OFF`.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub inputs: BTreeMap<String, String>,
    /// Each parameter's value, as typed.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub params: BTreeMap<String, String>,
    /// Each assignable parameter's CV input; one left out has none.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub assigns: BTreeMap<String, AssignPatch>,
    /// What the applet keeps beside its parameters, if anything.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub state: Option<StatePatch>,
}

/// A parameter's CV input, its fields as the submenu writes them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AssignPatch {
    /// `--`, `CV1` to `CV4`, or another applet's port, `1:NOTE`.
    pub input: String,
    /// The gain, `x1.00`.
    pub scale: String,
    /// The offset, `+0.00V`.
    pub shift: String,
    /// The lower bound, `-10.0`.
    pub min: String,
    /// The upper bound, `+10.0`.
    pub max: String,
    /// `SET`, `ADD` or `MUL`.
    pub effect: String,
}

/// An applet's own state: `MELODY`'s steps, readable; any other's bytes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum StatePatch {
    /// `MELODY`'s steps: each one's place in the window, play rank and tie
    /// rank, as many as its loop can hold.
    Steps {
        /// Places, 0 to the window's top.
        places: Vec<u16>,
        /// Play ranks, against `DENSITY`.
        plays: Vec<u8>,
        /// Tie ranks, against `LEGATO`.
        ties: Vec<u8>,
    },
    /// Another applet's state, as the module serialised it.
    Bytes {
        /// In hexadecimal.
        hex: String,
    },
}

impl Patch {
    /// The patch as TOML.
    ///
    /// # Errors
    ///
    /// What the TOML writer could not write.
    #[cfg(feature = "cli")]
    pub fn to_toml(&self) -> Result<String, String> {
        toml::to_string(self).map_err(|error| error.to_string())
    }

    /// A patch read from TOML.
    ///
    /// # Errors
    ///
    /// What does not read as a patch, or a layout this tool does not know.
    #[cfg(feature = "cli")]
    pub fn from_toml(text: &str) -> Result<Self, String> {
        let patch: Self = toml::from_str(text).map_err(|error| error.to_string())?;
        if patch.format != FORMAT {
            return Err(format!(
                "format {} is not this tool's ({FORMAT})",
                patch.format
            ));
        }
        Ok(patch)
    }
}

/// Reads the patch off the module.
///
/// # Errors
///
/// What the module or the port refused.
pub fn save<T: Read + Write>(client: &mut Client<T>) -> crate::client::Result<Patch> {
    let apps = client.apps()?;
    let instances = client.instances()?;
    let ports = PortNames::of(&instances);
    let mut patch = Patch {
        format: FORMAT,
        focus: 1,
        slots: Vec::new(),
    };
    for instance in &instances {
        if instance.focused {
            patch.focus = instance.position + 1;
        }
        let Some(app) = apps.iter().find(|app| app.index == instance.app) else {
            continue;
        };
        if app.name == "NOTHING" {
            continue;
        }
        patch.slots.push(save_slot(client, app, instance, &ports)?);
    }
    Ok(patch)
}

/// One slot of the patch.
fn save_slot<T: Read + Write>(
    client: &mut Client<T>,
    app: &AppEntry,
    instance: &InstanceEntry,
    ports: &PortNames,
) -> crate::client::Result<SlotPatch> {
    let target = Target::Instance(instance.position);
    let description = client.describe(target)?;
    let mut slot = SlotPatch {
        slot: instance.position + 1,
        app: app.name.clone(),
        outputs: BTreeMap::new(),
        inputs: BTreeMap::new(),
        params: BTreeMap::new(),
        assigns: BTreeMap::new(),
        state: None,
    };
    for (index, port) in description.outputs.iter().enumerate() {
        let jack = instance
            .outputs
            .get(index)
            .copied()
            .flatten()
            .and_then(|jack| OUTPUT_NAMES.get(usize::from(jack)))
            .copied()
            .unwrap_or("OFF");
        slot.outputs.insert(port.name.clone(), jack.to_owned());
    }
    for (index, port) in description.inputs.iter().enumerate() {
        let source = instance
            .inputs
            .get(index)
            .copied()
            .unwrap_or(InputSource::Jack(0));
        slot.inputs.insert(
            port.name.clone(),
            input_source_text(source, port.trigger, ports),
        );
    }
    for spec in &description.params {
        if let Some(value) = spec.value {
            slot.params
                .insert(spec.name.clone(), edit_text(spec, value));
        }
        // A parameter without an input is left out: loading gives it none.
        if let Some(input) = spec.input.filter(|input| input.assign != Assign::NONE) {
            slot.assigns
                .insert(spec.name.clone(), assign_patch(&input.assign, ports));
        }
    }
    slot.state = client
        .get_state(target)?
        .map(|bytes| state_patch(&app.name, &bytes));
    Ok(slot)
}

/// An input's fields as the file writes them.
fn assign_patch(assign: &Assign, ports: &PortNames) -> AssignPatch {
    let field = |line| assign.field(line).as_str().to_owned();
    AssignPatch {
        input: assign
            .input
            .map_or_else(|| "--".to_owned(), |source| source_text(source, ports)),
        scale: field(AssignLine::Scale),
        shift: field(AssignLine::Shift),
        min: field(AssignLine::Min),
        max: field(AssignLine::Max),
        effect: field(AssignLine::Effect),
    }
}

/// An applet's own state as the file writes it: `MELODY`'s steps readable,
/// any other's bytes in hexadecimal.
fn state_patch(app: &str, bytes: &[u8]) -> StatePatch {
    if app == MELODY
        && let Ok(steps) = postcard::from_bytes::<Steps>(bytes)
    {
        return StatePatch::Steps {
            places: steps.places.to_vec(),
            plays: steps.plays.to_vec(),
            ties: steps.ties.to_vec(),
        };
    }
    StatePatch::Bytes {
        hex: bytes.iter().fold(String::new(), |mut hex, byte| {
            let _ = write!(hex, "{byte:02x}");
            hex
        }),
    }
}

/// The bytes the module reads back for `state`.
fn state_bytes(state: &StatePatch) -> Result<Vec<u8>, String> {
    match state {
        StatePatch::Steps {
            places,
            plays,
            ties,
        } => {
            let array = |name: &str, len: usize| {
                if len == MAX_STEPS {
                    Ok(())
                } else {
                    Err(format!("{name}: {MAX_STEPS} steps, not {len}"))
                }
            };
            array("places", places.len())?;
            array("plays", plays.len())?;
            array("ties", ties.len())?;
            let mut steps = Steps {
                places: [0; MAX_STEPS],
                plays: [0; MAX_STEPS],
                ties: [0; MAX_STEPS],
            };
            steps.places.copy_from_slice(places);
            steps.plays.copy_from_slice(plays);
            steps.ties.copy_from_slice(ties);
            postcard::to_allocvec(&steps).map_err(|error| error.to_string())
        }
        StatePatch::Bytes { hex } => (0..hex.len())
            .step_by(2)
            .map(|at| {
                hex.get(at..at + 2)
                    .and_then(|pair| u8::from_str_radix(pair, 16).ok())
                    .ok_or_else(|| format!("state: {hex} is not hexadecimal"))
            })
            .collect(),
    }
}

/// Writes `patch` onto the module, in place of everything that runs: each
/// slot's applet, every output cut then routed, the settings twice round,
/// then the inputs and the CV inputs once every applet is there, the
/// applets' own state, the focus last. What the file names and the module
/// does not know (a setting since removed) is passed over and said in the
/// list returned.
///
/// # Errors
///
/// What the module or the port refused, or the file names no such applet.
pub fn load<T: Read + Write>(
    client: &mut Client<T>,
    patch: &Patch,
) -> crate::client::Result<Vec<String>> {
    let mut skipped = Vec::new();
    let apps = client.apps()?;
    let nothing = find_app(&apps, "NOTHING").map(|app| app.index).ok_or(
        crate::client::ProbeError::Unexpected("NOTHING in the app list"),
    )?;
    // Every slot emptied first, so that the applets below take no output
    // another still holds, then each slot's applet.
    for position in 0..SLOTS {
        client.select_app(nothing, Some(position))?;
    }
    let mut placed = Vec::new();
    for slot in &patch.slots {
        let Some(position) = slot.slot.checked_sub(1).filter(|&at| at < SLOTS) else {
            skipped.push(format!("slot {}: there are slots 1 to {SLOTS}", slot.slot));
            continue;
        };
        let Some(app) = find_app(&apps, &slot.app) else {
            skipped.push(format!("slot {}: no applet {}", slot.slot, slot.app));
            continue;
        };
        client.select_app(app.index, Some(position))?;
        placed.push((position, slot));
    }
    // Every output cut, then routed as the file says: no route is refused
    // for a jack the default placement gave another.
    for &(position, _) in &placed {
        let description = client.describe(Target::Instance(position))?;
        for port in 0..description.outputs.len() {
            client.set_route(position, port_index(port), None)?;
        }
    }
    let ports = PortNames::of(&client.instances()?);
    for &(position, slot) in &placed {
        load_slot(client, position, slot, &ports, &mut skipped)?;
    }
    if let Some(focus) = patch.focus.checked_sub(1).filter(|&at| at < SLOTS) {
        client.focus(focus)?;
    }
    Ok(skipped)
}

/// One slot's routes, settings, inputs and state.
fn load_slot<T: Read + Write>(
    client: &mut Client<T>,
    position: u8,
    slot: &SlotPatch,
    ports: &PortNames,
    skipped: &mut Vec<String>,
) -> crate::client::Result<()> {
    let target = Target::Instance(position);
    let description = client.describe(target)?;
    let mut skip = |what: String| skipped.push(format!("slot {}: {what}", slot.slot));
    for (name, jack) in &slot.outputs {
        let Some(port) = description
            .outputs
            .iter()
            .position(|port| &port.name == name)
        else {
            skip(format!("no output {name}"));
            continue;
        };
        match parse_output(jack) {
            Ok(output) => {
                if let Err(error) = client.set_route(position, port_index(port), output) {
                    skip(format!("{name} -> {jack}: {error}"));
                }
            }
            Err(error) => skip(error),
        }
    }
    for (name, text) in &slot.inputs {
        let Some(port) = description
            .inputs
            .iter()
            .position(|port| &port.name == name)
        else {
            skip(format!("no input {name}"));
            continue;
        };
        let trigger = description.inputs[port].trigger;
        match parse_input_source(text, trigger, ports) {
            Ok(source) => {
                if let Err(error) = client.set_input(position, port_index(port), source) {
                    skip(format!("{name} <- {text}: {error}"));
                }
            }
            Err(error) => skip(error),
        }
    }
    // Twice round: a setting may bound another set before it.
    for _ in 0..2 {
        for spec in &description.params {
            let Some(text) = slot.params.get(&spec.name) else {
                continue;
            };
            match parse_value(spec, text) {
                Ok(value) => {
                    let _ = client.set_param(target, spec.index, value);
                }
                Err(error) => skip(error),
            }
        }
    }
    for name in slot.params.keys() {
        if !description.params.iter().any(|spec| &spec.name == name) {
            skip(format!("no setting {name}"));
        }
    }
    // An assignable parameter the file leaves out takes no input, whatever
    // its applet gives it at start (`SUPERSAW`'s `NOTE` on `CV1`).
    for spec in &description.params {
        if spec.assignable
            && !slot.assigns.contains_key(&spec.name)
            && spec.input.is_some_and(|input| input.assign != Assign::NONE)
            && let Err(error) = client.set_assign(target, spec.index, Assign::NONE)
        {
            skip(format!("{}'s input: {error}", spec.name));
        }
    }
    for (name, fields) in &slot.assigns {
        let Some(spec) = description.params.iter().find(|spec| &spec.name == name) else {
            skip(format!("no setting {name} for an input"));
            continue;
        };
        match assign_from(fields, ports) {
            Ok(assign) => {
                if let Err(error) = client.set_assign(target, spec.index, assign) {
                    skip(format!("{name}'s input: {error}"));
                }
            }
            Err(error) => skip(format!("{name}'s input: {error}")),
        }
    }
    if let Some(state) = &slot.state {
        match state_bytes(state) {
            Ok(bytes) => {
                if let Err(error) = client.set_state(target, &bytes) {
                    skip(format!("state: {error}"));
                }
            }
            Err(error) => skip(error),
        }
    }
    Ok(())
}

/// A CV input from its fields: the shaping first, then the input, since a
/// port of another applet takes no shaping (journal/D-048).
fn assign_from(fields: &AssignPatch, ports: &PortNames) -> Result<Assign, String> {
    let mut assign = Assign::NONE;
    let values = [
        &fields.scale,
        &fields.shift,
        &fields.min,
        &fields.max,
        &fields.effect,
    ];
    // `INPUT_FIELDS` is input, then these five in this order.
    for (field, text) in INPUT_FIELDS[1..].iter().zip(values) {
        assign = parse_input_field(assign, field, text, ports)?;
    }
    parse_input_field(assign, "input", &fields.input, ports)
}

/// A port's index as the protocol takes it.
fn port_index(port: usize) -> u8 {
    u8::try_from(port).unwrap_or(u8::MAX)
}
