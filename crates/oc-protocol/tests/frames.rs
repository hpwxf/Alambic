//! Messages survive encoding, framing and decoding, and a damaged stream
//! costs one frame, never the session.

use oc_core::profile::Stage;
use oc_protocol::{
    Error, FrameReader, MAX_FRAME, Profile, Reply, Request, Sample, VERSION, decode, encode,
};
use proptest::prelude::*;

/// Feeds `bytes` to `reader` and decodes every frame as a `Request`.
fn requests(reader: &mut FrameReader, bytes: &[u8]) -> Vec<Result<Request, Error>> {
    let mut out = Vec::new();
    for &byte in bytes {
        if let Some(frame) = reader.push(byte) {
            out.push(frame.and_then(decode::<Request>));
        }
    }
    out
}

fn framed(request: &Request) -> Vec<u8> {
    let mut buffer = [0; MAX_FRAME];
    encode(request, &mut buffer).unwrap().to_vec()
}

const EVERY_REQUEST: [Request; 9] = [
    Request::Hello { protocol: VERSION },
    Request::GetProfile,
    Request::ClearProfile,
    Request::Stream { every_ticks: 10 },
    Request::Stream { every_ticks: 0 },
    Request::ForceOutput {
        channel: 3,
        millivolts: Some(-3_000),
    },
    Request::ForceOutput {
        channel: 0,
        millivolts: None,
    },
    Request::GetScreen,
    Request::RebootToLoader,
];

#[test]
fn every_request_round_trips_through_the_reader() {
    let mut reader = FrameReader::new();
    let stream: Vec<u8> = EVERY_REQUEST.iter().flat_map(framed).collect();
    let decoded = requests(&mut reader, &stream);
    let expected: Vec<_> = EVERY_REQUEST.iter().copied().map(Ok).collect();
    assert_eq!(decoded, expected);
}

#[test]
fn a_frame_ends_with_its_only_zero() {
    for request in EVERY_REQUEST {
        let frame = framed(&request);
        assert_eq!(frame.last(), Some(&0));
        assert!(!frame[..frame.len() - 1].contains(&0), "{request:?}");
    }
}

#[test]
fn replies_borrow_from_their_frame() {
    let page = [0xA5_u8; 128];
    let replies = [
        Reply::Hello {
            protocol: VERSION,
            firmware: "0.3.0",
            build: "1a7d0c6-dirty",
        },
        Reply::ScreenPage {
            page: 7,
            bytes: &page,
        },
        Reply::Sample(Sample {
            tick: u64::MAX,
            micros: u64::MAX - 1,
            cv_in: [-3_333, 0, 6_665, 1],
            cv_codes: [0, 2_730, 4_095, 1],
            cv_out: [-3_000, 7_000, 0, 5],
            triggers: 0b1010,
        }),
        Reply::Done,
        Reply::Error(Error::Version(VERSION)),
    ];
    for reply in replies {
        let mut buffer = [0; MAX_FRAME];
        let mut frame = encode(&reply, &mut buffer).unwrap().to_vec();
        frame.pop();
        assert_eq!(decode::<Reply<'_>>(&mut frame), Ok(reply));
    }
}

#[test]
fn the_fullest_profile_fits_in_a_frame() {
    let mut profile = Profile::new(600);
    profile.set_expected_period_micros(u32::MAX);
    for stage in Stage::ALL {
        profile.record(stage, u32::MAX);
        profile.record(stage, u32::MAX);
    }
    for _ in 0..3 {
        profile.record_period(u32::MAX);
        profile.record_period(0);
    }
    let mut buffer = [0; MAX_FRAME];
    let frame = encode(&Reply::Profile(profile.clone()), &mut buffer).unwrap();
    assert!(frame.len() < MAX_FRAME / 2, "{} bytes", frame.len());
    let mut frame = frame.to_vec();
    assert_eq!(decode::<Reply<'_>>(&mut frame), Ok(Reply::Profile(profile)));
}

#[test]
fn garbage_costs_one_frame_then_the_stream_resynchronises() {
    let mut reader = FrameReader::new();
    let mut stream = vec![0x17, 0xFF, 0x03, 0];
    stream.extend(framed(&Request::GetProfile));
    let decoded = requests(&mut reader, &stream);
    assert_eq!(decoded, [Err(Error::Malformed), Ok(Request::GetProfile)]);
}

#[test]
fn an_overlong_frame_is_dropped_whole() {
    let mut reader = FrameReader::new();
    let mut stream = vec![0x42; MAX_FRAME + 10];
    stream.push(0);
    stream.extend(framed(&Request::ClearProfile));
    let decoded = requests(&mut reader, &stream);
    assert_eq!(decoded, [Err(Error::Overflow), Ok(Request::ClearProfile)]);
}

#[test]
fn empty_frames_are_skipped() {
    let mut reader = FrameReader::new();
    let mut stream = vec![0, 0, 0];
    stream.extend(framed(&Request::GetScreen));
    assert_eq!(requests(&mut reader, &stream), [Ok(Request::GetScreen)]);
}

proptest! {
    #[test]
    fn any_forced_output_round_trips(channel: u8, millivolts: Option<i32>) {
        let request = Request::ForceOutput { channel, millivolts };
        let mut reader = FrameReader::new();
        prop_assert_eq!(requests(&mut reader, &framed(&request)), [Ok(request)]);
    }

    #[test]
    fn arbitrary_bytes_never_panic_and_resynchronise(noise in proptest::collection::vec(any::<u8>(), 0..2_000)) {
        let mut reader = FrameReader::new();
        let _ = requests(&mut reader, &noise);
        let mut tail = vec![0];
        tail.extend(framed(&Request::GetProfile));
        let decoded = requests(&mut reader, &tail);
        prop_assert_eq!(decoded.last(), Some(&Ok(Request::GetProfile)));
    }
}
