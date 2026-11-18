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
    // The columns line up whatever the stage's name: every row of the table
    // as long as its header.
    let rows: Vec<&str> = table.lines().take_while(|line| !line.is_empty()).collect();
    assert!(rows.iter().all(|row| row.len() == rows[0].len()), "{table}");
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
