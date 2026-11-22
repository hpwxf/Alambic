//! The host's client against the module's real server, joined by an
//! in-memory port over a mock engine: the whole protocol, both sides, with
//! no hardware.

use std::io::{self, Read, Write};

use oc_core::platform::{CvChannel, Display as _};
use oc_core::profile::Stage;
use oc_core::testing::{MockEngine, mock_engine};
use oc_probe::client::{Client, ProbeError};
use oc_probe::render;
use oc_protocol::Error;
use oc_protocol::server::{Identity, Server};

/// The module end of the cable: what the host writes goes to the server,
/// what the server queues comes back; the engine ticks while the host waits.
struct Loopback {
    server: Server,
    engine: MockEngine,
}

impl Loopback {
    fn new(can_reboot: bool) -> Self {
        Self {
            server: Server::new(Identity {
                firmware: "0.3.0",
                build: "abc1234",
                can_reboot,
            }),
            engine: mock_engine(0),
        }
    }
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
    let mut client = Client::new(Loopback::new(true));
    let info = client.hello().unwrap();
    assert_eq!(
        (info.firmware.as_str(), info.build.as_str()),
        ("0.3.0", "abc1234")
    );
    client
}

#[test]
fn the_profile_comes_back_and_prints() {
    let mut client = session();
    // Waiting for a sample lets the engine tick.
    client.stream(3).unwrap();
    let _ = client.next_sample().unwrap();
    client.stream(0).unwrap();
    let profile = client.profile().unwrap();
    assert!(profile.stage(Stage::Tick).count() >= 3);
    let table = render::profile_table(&profile);
    assert!(table.contains("period"), "{table}");
    // The columns line up whatever the stage's name: every row's note starts
    // where the header's does. Section titles carry no numbers.
    let header = table.lines().next().unwrap();
    let notes = header.find("  what").unwrap();
    let rows: Vec<&str> = table
        .lines()
        .take_while(|line| !line.is_empty())
        .filter(|line| line.starts_with("  "))
        .collect();
    assert!(rows.len() >= 2, "{table}");
    assert!(
        rows.iter()
            .all(|row| row.get(notes..notes + 2) == Some("  ") && row.as_bytes()[notes - 1] != b' '),
        "{table}"
    );
    client.clear_profile().unwrap();
}

#[test]
fn a_stream_gives_samples_until_stopped() {
    let mut client = session();
    client.stream(5).unwrap();
    let first = client.next_sample().unwrap();
    let second = client.next_sample().unwrap();
    assert_eq!(second.tick - first.tick, 5);
    client.stream(0).unwrap();
    // Answers still come through, whatever samples were in flight.
    client.clear_profile().unwrap();
}

#[test]
fn a_forced_output_shows_in_the_samples() {
    let mut client = session();
    client.force_output(CvChannel::Three, Some(-2_000)).unwrap();
    client.stream(1).unwrap();
    let _ = client.next_sample().unwrap();
    let sample = client.next_sample().unwrap();
    assert_eq!(sample.cv_out[2], -2_000);
    assert!(render::sample_line(&sample).contains("-2000"));
}

#[test]
fn the_screen_comes_back_whole() {
    let mut client = session();
    let screen = client.screen().unwrap();
    let text = render::screen_text(&screen);
    assert_eq!(text.lines().count(), 32);
    let mut png = Vec::new();
    render::screen_png(&screen, 2, &mut png).unwrap();
    assert_eq!(&png[1..4], b"PNG");
}

#[test]
fn the_screen_matches_the_module_frame() {
    let mut client = session();
    let screen = client.screen().unwrap();
    let mut engine = client.into_inner().engine;
    assert_eq!(&screen, engine.parts_mut().4.frame_mut().as_bytes());
}

#[test]
fn refusals_are_errors() {
    let mut client = Client::new(Loopback::new(false));
    assert!(matches!(
        client.profile(),
        Err(ProbeError::Refused(Error::NoSession))
    ));
    client.hello().unwrap();
    assert!(matches!(
        client.reboot_to_loader(),
        Err(ProbeError::Refused(Error::Unsupported))
    ));
    // The session goes on after a refusal.
    client.force_output(CvChannel::One, Some(1)).unwrap();
}

#[test]
fn an_applet_is_found_described_and_set_by_name() {
    use oc_core::apps::ratchet::RatchetApp;
    use oc_probe::apps;
    use oc_protocol::ParamValue;

    let mut client = session();
    let list = client.apps().unwrap();
    assert_eq!(list.len(), oc_core::apps::AppId::ALL.len());
    assert!(list[0].running, "the diagnostic applet runs at start");
    let ratchet = apps::find_app(&list, "ratchet").unwrap().clone();

    let description = client.describe(ratchet.index).unwrap();
    let mode = apps::find_spec(&description.params, "mode").unwrap();
    assert_eq!(mode.choices, ["FIRST", "SPAN"]);
    let text = apps::description_text(&ratchet, &description);
    assert!(text.contains("COUNT"), "{text}");
    assert!(text.contains("actions: BURST"), "{text}");

    let shape = apps::find_spec(&description.params, "shape")
        .unwrap()
        .clone();
    let value = apps::parse_value(&shape, "-1.5").unwrap();
    assert_eq!(
        client.set_param(ratchet.index, shape.index, value).unwrap(),
        ParamValue::Number(-15)
    );
    assert_eq!(
        client
            .into_inner()
            .engine
            .app::<RatchetApp>()
            .settings()
            .curve_tenths,
        -15
    );
}

#[test]
fn an_input_is_described_and_set_by_its_fields() {
    use oc_core::apps::supersaw::{Setting, SupersawApp};
    use oc_probe::apps;

    let mut client = session();
    let list = client.apps().unwrap();
    let supersaw = apps::find_app(&list, "supersaw").unwrap().clone();
    let description = client.describe(supersaw.index).unwrap();
    let detune = apps::find_spec(&description.params, "detune").unwrap();
    let input = detune.input.expect("DETUNE takes an input");
    assert_eq!(apps::input_text(&input.assign), "--");
    let note = apps::find_spec(&description.params, "note").unwrap();
    let text = apps::input_text(&note.input.unwrap().assign);
    assert_eq!(
        text, "CV1 x1.00 +0.00V -10.0..+10.0 ADD",
        "CV1 on NOTE at start"
    );
    assert!(
        apps::description_text(&supersaw, &description).contains("CV CV1 x1.00"),
        "the description shows it"
    );

    let mut assign = input.assign;
    for (field, text) in [
        ("input", "cv2"),
        ("effect", "mul"),
        ("scale", "0.5"),
        ("shift", "-0.25"),
        ("min", "0"),
        ("max", "5"),
    ] {
        assign = apps::parse_input_field(assign, field, text).unwrap();
    }
    let applied = client
        .set_assign(supersaw.index, detune.index, assign)
        .unwrap();
    assert_eq!(
        apps::input_text(&applied.assign),
        "CV2 x0.50 -0.25V +0.0..+5.0 MUL"
    );
    assert_eq!(
        client
            .get_assign(supersaw.index, detune.index)
            .unwrap()
            .assign,
        assign
    );
    assert!(apps::parse_input_field(assign, "input", "cv5").is_err());
    assert!(apps::parse_input_field(assign, "gain", "1").is_err());
    assert!(matches!(
        client.set_assign(
            supersaw.index,
            detune.index,
            apps::parse_input_field(assign, "scale", "3").unwrap()
        ),
        Err(ProbeError::Refused(Error::Refused(
            oc_protocol::Refused::OutOfRange
        )))
    ));
    assert_eq!(
        client
            .into_inner()
            .engine
            .app::<SupersawApp>()
            .assign(Setting::Detune),
        Some(&assign)
    );
}

#[test]
fn an_applet_selected_from_afar_runs() {
    let mut client = session();
    let list = client.apps().unwrap();
    let tone = oc_probe::apps::find_app(&list, "tone").unwrap().index;
    client.select_app(tone).unwrap();
    let list = client.apps().unwrap();
    assert!(list[usize::from(tone)].running);
    assert!(matches!(
        client.describe(99),
        Err(ProbeError::Refused(Error::NoSuchApp))
    ));
}
