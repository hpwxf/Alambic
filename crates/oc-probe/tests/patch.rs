//! The patch file (journal/D-050) against the module's real server over a
//! mock engine: saved, written as TOML, read back, the module changed, the
//! file loaded, and the module saved again as it was.

use std::io::{self, Read, Write};

use oc_core::testing::{MockEngine, mock_engine};
use oc_probe::apps::{PortNames, find_app, parse_input_field};
use oc_probe::client::Client;
use oc_probe::patch::{self, Patch, StatePatch};
use oc_protocol::server::{Identity, Server};
use oc_protocol::{Assign, InputSource, Target};

/// The module end of the cable, as in `tests/client.rs`.
struct Loopback {
    server: Server,
    engine: MockEngine,
}

impl Write for Loopback {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.server.receive(bytes, &mut self.engine);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl Read for Loopback {
    fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
        for _ in 0..1_000 {
            let waiting = self.server.outgoing().peek(out.len());
            if !waiting.is_empty() {
                let count = waiting.len();
                out[..count].copy_from_slice(waiting);
                self.server.outgoing().consume(count);
                return Ok(count);
            }
            self.engine.clock().advance(1_000);
            let report = self.engine.tick();
            self.server.after_tick(&report, &mut self.engine);
        }
        Err(io::ErrorKind::TimedOut.into())
    }
}

fn session() -> Client<Loopback> {
    let mut client = Client::new(Loopback {
        server: Server::new(Identity {
            firmware: "0.7.0",
            build: "abc1234",
            can_reboot: false,
        }),
        engine: mock_engine(0),
    });
    client.hello().unwrap();
    client
}

/// The index of applet `name`.
fn app(client: &mut Client<Loopback>, name: &str) -> u8 {
    let apps = client.apps().unwrap();
    find_app(&apps, name).unwrap().index
}

/// The index of parameter `name` of the instance at `position`.
fn param(client: &mut Client<Loopback>, position: u8, name: &str) -> u8 {
    let description = client.describe(Target::Instance(position)).unwrap();
    description
        .params
        .iter()
        .find(|spec| spec.name == name)
        .unwrap()
        .index
}

/// MELODY, RATCHET and SUPERSAW, set away from their defaults, routed into
/// one another.
fn patched() -> Client<Loopback> {
    let mut client = session();
    let (melody, ratchet, supersaw) = (
        app(&mut client, "melody"),
        app(&mut client, "ratchet"),
        app(&mut client, "supersaw"),
    );
    client.select_app(melody, Some(0)).unwrap();
    client.select_app(ratchet, Some(1)).unwrap();
    client.select_app(supersaw, Some(2)).unwrap();
    let length = param(&mut client, 0, "LENGTH");
    client
        .set_param(
            Target::Instance(0),
            length,
            oc_protocol::ParamValue::Number(12),
        )
        .unwrap();
    let count = param(&mut client, 1, "COUNT");
    client
        .set_param(
            Target::Instance(1),
            count,
            oc_protocol::ParamValue::Number(3),
        )
        .unwrap();
    // MELODY's CLOCK on RATCHET's THRU, its RESET on nothing; CH2 cut.
    client
        .set_input(0, 0, InputSource::Lane { slot: 1, port: 0 })
        .unwrap();
    client.set_input(0, 1, InputSource::Off).unwrap();
    client.set_route(0, 2, None).unwrap();
    // SUPERSAW's NOTE assigned MELODY's NOTE.
    let ports = PortNames::of(&client.instances().unwrap());
    let note = param(&mut client, 2, "NOTE");
    let assign = parse_input_field(Assign::NONE, "input", "1:note", &ports).unwrap();
    client
        .set_assign(Target::Instance(2), note, assign)
        .unwrap();
    client.focus(2).unwrap();
    client
}

#[test]
fn a_patch_saved_reads_back_as_toml() {
    let mut client = patched();
    let saved = patch::save(&mut client).unwrap();
    assert_eq!(saved.focus, 3);
    assert_eq!(
        saved
            .slots
            .iter()
            .map(|slot| (slot.slot, slot.app.as_str()))
            .collect::<Vec<_>>(),
        [(1, "MELODY"), (2, "RATCHET"), (3, "SUPERSAW")]
    );
    let melody = &saved.slots[0];
    assert_eq!(melody.params["LENGTH"], "12");
    assert_eq!(melody.inputs["CLOCK"], "2:THRU");
    assert_eq!(melody.inputs["RESET"], "OFF");
    assert_eq!(melody.outputs["CH2"], "OFF");
    assert!(
        matches!(&melody.state, Some(StatePatch::Steps { places, .. }) if places.len() == 32),
        "MELODY's steps, readable"
    );
    assert_eq!(saved.slots[2].assigns["NOTE"].input, "1:NOTE");

    let text = saved.to_toml().unwrap();
    assert!(text.contains("[[slot]]") && text.contains("app = \"MELODY\""));
    assert_eq!(Patch::from_toml(&text).unwrap(), saved);
}

#[test]
fn a_patch_loaded_brings_the_module_back_as_it_was() {
    let mut client = patched();
    let saved = patch::save(&mut client).unwrap();

    // Everything changed: other applets, MELODY's steps redrawn.
    let tone = app(&mut client, "tone");
    let nothing = app(&mut client, "nothing");
    client.act(Target::Instance(0), 0).unwrap();
    client.select_app(tone, Some(1)).unwrap();
    client.select_app(nothing, Some(2)).unwrap();
    client.focus(0).unwrap();
    assert_ne!(patch::save(&mut client).unwrap(), saved);

    let skipped = patch::load(&mut client, &saved).unwrap();
    assert!(skipped.is_empty(), "{skipped:?}");
    assert_eq!(patch::save(&mut client).unwrap(), saved);
}

#[test]
fn what_the_module_does_not_know_is_passed_over() {
    let mut client = patched();
    let mut saved = patch::save(&mut client).unwrap();
    saved.slots[0]
        .params
        .insert("WOBBLE".to_owned(), "3".to_owned());
    saved.slots[1].app = "THEREMIN".to_owned();
    let skipped = patch::load(&mut client, &saved).unwrap();
    assert!(
        skipped.iter().any(|line| line.contains("WOBBLE")),
        "{skipped:?}"
    );
    assert!(
        skipped.iter().any(|line| line.contains("THEREMIN")),
        "{skipped:?}"
    );
    assert_eq!(
        patch::save(&mut client).unwrap().slots[0].params["LENGTH"],
        "12",
        "the rest loaded"
    );
}

#[test]
fn a_file_of_another_layout_is_refused() {
    assert!(Patch::from_toml("format = 2\nfocus = 1\n").is_err());
}

#[test]
#[ignore = "prints an example file"]
fn print_an_example() {
    let mut client = patched();
    println!("{}", patch::save(&mut client).unwrap().to_toml().unwrap());
}
