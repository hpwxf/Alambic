//! The module's side of a session, served from a mock engine.

use oc_core::platform::{CV_CHANNELS, CvChannel, Display as _, TriggerChannel};
use oc_core::profile::Stage;
use oc_core::testing::{MockEngine, mock_engine};
use oc_protocol::server::{Identity, Server};
use oc_protocol::{
    Error, FrameReader, MAX_FRAME, Reply, Request, SCREEN_PAGES, VERSION, decode, encode,
};

const IDENTITY: Identity = Identity {
    firmware: "0.3.0",
    build: "abc1234",
    can_reboot: true,
};

fn send(server: &mut Server, engine: &mut MockEngine, requests: &[Request]) {
    for request in requests {
        let mut buffer = [0; MAX_FRAME];
        let frame = encode(request, &mut buffer).unwrap();
        server.receive(frame, engine);
    }
}

/// Every frame the server queued, still encoded (zero excluded).
fn frames(server: &mut Server) -> Vec<Vec<u8>> {
    let mut reader = FrameReader::new();
    let mut out = Vec::new();
    let queue = server.outgoing();
    while !queue.is_empty() {
        let piece = queue.peek(64).to_vec();
        queue.consume(piece.len());
        for byte in piece {
            if let Some(frame) = reader.push(byte) {
                out.push(frame.unwrap().to_vec());
            }
        }
    }
    out
}

fn opened() -> (Server, MockEngine) {
    let mut server = Server::new(IDENTITY);
    let mut engine = mock_engine(0);
    send(
        &mut server,
        &mut engine,
        &[Request::Hello { protocol: VERSION }],
    );
    let mut replies = frames(&mut server);
    assert_eq!(
        decode::<Reply<'_>>(&mut replies[0]),
        Ok(Reply::Hello {
            protocol: VERSION,
            firmware: "0.3.0",
            build: "abc1234"
        })
    );
    (server, engine)
}

#[test]
fn nothing_is_served_before_the_hello() {
    let mut server = Server::new(IDENTITY);
    let mut engine = mock_engine(0);
    send(&mut server, &mut engine, &[Request::GetProfile]);
    let mut replies = frames(&mut server);
    assert_eq!(
        decode::<Reply<'_>>(&mut replies[0]),
        Ok(Reply::Error(Error::NoSession))
    );
}

#[test]
fn another_version_is_refused() {
    let mut server = Server::new(IDENTITY);
    let mut engine = mock_engine(0);
    send(
        &mut server,
        &mut engine,
        &[Request::Hello {
            protocol: VERSION + 1,
        }],
    );
    let mut replies = frames(&mut server);
    assert_eq!(
        decode::<Reply<'_>>(&mut replies[0]),
        Ok(Reply::Error(Error::Version(VERSION)))
    );
    assert!(!server.in_session());
}

#[test]
fn the_profile_comes_from_the_engine_and_can_be_cleared() {
    let (mut server, mut engine) = opened();
    engine.tick();
    send(
        &mut server,
        &mut engine,
        &[
            Request::GetProfile,
            Request::ClearProfile,
            Request::GetProfile,
        ],
    );
    let mut replies = frames(&mut server);
    let Ok(Reply::Profile(before)) = decode::<Reply<'_>>(&mut replies[0]) else {
        panic!("no profile");
    };
    assert_eq!(before.stage(Stage::Tick).count(), 1);
    assert_eq!(decode::<Reply<'_>>(&mut replies[1]), Ok(Reply::Done));
    let Ok(Reply::Profile(after)) = decode::<Reply<'_>>(&mut replies[2]) else {
        panic!("no profile");
    };
    assert_eq!(after.stage(Stage::Tick).count(), 0);
}

#[test]
fn a_forced_output_reaches_the_engine_and_a_bad_channel_is_refused() {
    let (mut server, mut engine) = opened();
    send(
        &mut server,
        &mut engine,
        &[
            Request::ForceOutput {
                channel: 2,
                millivolts: Some(1_000),
            },
            Request::ForceOutput {
                channel: 4,
                millivolts: Some(1_000),
            },
        ],
    );
    let mut replies = frames(&mut server);
    assert_eq!(decode::<Reply<'_>>(&mut replies[0]), Ok(Reply::Done));
    assert_eq!(
        decode::<Reply<'_>>(&mut replies[1]),
        Ok(Reply::Error(Error::BadChannel))
    );
    assert_eq!(engine.forced_outputs()[2], Some(1_000));
}

#[test]
fn closing_the_session_releases_the_outputs_and_stops_the_stream() {
    let (mut server, mut engine) = opened();
    send(
        &mut server,
        &mut engine,
        &[
            Request::ForceOutput {
                channel: 0,
                millivolts: Some(-500),
            },
            Request::Stream { every_ticks: 1 },
        ],
    );
    server.close(&mut engine);
    assert_eq!(engine.forced_outputs(), &[None; CV_CHANNELS]);
    let report = engine.tick();
    server.after_tick(&report, &mut engine);
    assert_eq!(frames(&mut server), Vec::<Vec<u8>>::new());
}

#[test]
fn the_stream_samples_every_nth_tick() {
    let (mut server, mut engine) = opened();
    send(
        &mut server,
        &mut engine,
        &[Request::Stream { every_ticks: 3 }],
    );
    let _ = frames(&mut server);
    engine.parts_mut().2.set(TriggerChannel::Two, true);
    engine.force_output(CvChannel::One, Some(1_234));
    for _ in 0..9 {
        let report = engine.tick();
        server.after_tick(&report, &mut engine);
    }
    let mut samples = frames(&mut server);
    assert_eq!(samples.len(), 3);
    let Ok(Reply::Sample(sample)) = decode::<Reply<'_>>(&mut samples[2]) else {
        panic!("no sample");
    };
    assert_eq!(sample.tick, 9);
    assert_eq!(sample.cv_out[0], 1_234);
    assert_eq!(sample.triggers, 0b0010);
}

#[test]
fn the_screen_comes_as_eight_pages_of_the_framebuffer() {
    let (mut server, mut engine) = opened();
    engine.tick();
    let expected = *engine.parts_mut().4.frame_mut().as_bytes();
    send(&mut server, &mut engine, &[Request::GetScreen]);
    let replies = frames(&mut server);
    assert_eq!(replies.len(), usize::from(SCREEN_PAGES));
    let mut screen = Vec::new();
    for (index, mut frame) in replies.into_iter().enumerate() {
        let Ok(Reply::ScreenPage { page, bytes }) = decode::<Reply<'_>>(&mut frame) else {
            panic!("no page");
        };
        assert_eq!(usize::from(page), index);
        screen.extend_from_slice(bytes);
    }
    assert_eq!(screen, expected);
}

#[test]
fn a_reboot_is_requested_once_or_refused_where_it_cannot_be_done() {
    let (mut server, mut engine) = opened();
    send(&mut server, &mut engine, &[Request::RebootToLoader]);
    assert!(server.take_reboot_request());
    assert!(!server.take_reboot_request());

    let mut server = Server::new(Identity {
        can_reboot: false,
        ..IDENTITY
    });
    send(
        &mut server,
        &mut engine,
        &[
            Request::Hello { protocol: VERSION },
            Request::RebootToLoader,
        ],
    );
    let mut replies = frames(&mut server);
    assert_eq!(
        decode::<Reply<'_>>(&mut replies[1]),
        Ok(Reply::Error(Error::Unsupported))
    );
    assert!(!server.take_reboot_request());
}

#[test]
fn a_garbled_frame_gets_an_error_and_the_session_goes_on() {
    let (mut server, mut engine) = opened();
    server.receive(&[0x55, 0x01, 0], &mut engine);
    send(&mut server, &mut engine, &[Request::ClearProfile]);
    let mut replies = frames(&mut server);
    assert_eq!(
        decode::<Reply<'_>>(&mut replies[0]),
        Ok(Reply::Error(Error::Malformed))
    );
    assert_eq!(decode::<Reply<'_>>(&mut replies[1]), Ok(Reply::Done));
}

#[test]
fn the_screen_is_captured_upright_whatever_the_mounting() {
    use oc_core::settings::Settings;

    let (mut server, mut engine) = opened();
    let mut settings = Settings::DEFAULT;
    settings.display.rotated = true;
    engine.apply_settings(settings);
    engine.tick();
    // The frame as sent is turned; what the module shows is it turned back.
    let mut upright = engine.parts_mut().4.frame_mut().clone();
    upright.rotate_half_turn();
    let upright = *upright.as_bytes();
    assert_ne!(
        *engine.parts_mut().4.frame_mut().as_bytes(),
        upright,
        "sent turned"
    );

    send(&mut server, &mut engine, &[Request::GetScreen]);
    let mut screen = Vec::new();
    for mut frame in frames(&mut server) {
        let Ok(Reply::ScreenPage { bytes, .. }) = decode::<Reply<'_>>(&mut frame) else {
            panic!("no page");
        };
        screen.extend_from_slice(bytes);
    }
    assert_eq!(screen, upright);
}
