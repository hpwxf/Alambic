//! The applets reached through the probe (journal/D-043): listed,
//! described, set and asked through the model the panel uses.

use oc_core::apps::AppId;
use oc_core::apps::ratchet::RatchetApp;
use oc_core::testing::{MockEngine, mock_engine};
use oc_protocol::server::{Identity, Server};
use oc_protocol::{
    Error, FrameReader, Group, MAX_FRAME, ParamKind, ParamValue, Refused, Reply, Request, VERSION,
    decode, encode,
};

const IDENTITY: Identity = Identity {
    firmware: "0.6.0",
    build: "abc1234",
    can_reboot: false,
};

const RATCHET: u8 = AppId::Ratchet as u8;

/// A session open on a fresh engine.
fn opened() -> (Server, MockEngine) {
    let mut server = Server::new(IDENTITY);
    let mut engine = mock_engine(0);
    replies(
        &mut server,
        &mut engine,
        &Request::Hello { protocol: VERSION },
    );
    (server, engine)
}

/// Every frame the server queued for `request`, still encoded.
fn replies(server: &mut Server, engine: &mut MockEngine, request: &Request) -> Vec<Vec<u8>> {
    let mut buffer = [0; MAX_FRAME];
    let frame = encode(request, &mut buffer).unwrap();
    server.receive(frame, engine);
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

/// The one reply `request` gets, decoded.
fn ask(server: &mut Server, engine: &mut MockEngine, request: &Request) -> Reply<'static> {
    let mut frames = replies(server, engine, request);
    assert_eq!(frames.len(), 1, "{request:?} gets one reply");
    let frame: &'static mut [u8] = frames.remove(0).leak();
    decode(frame).unwrap()
}

#[test]
fn every_applet_is_listed_with_what_it_declares() {
    let (mut server, mut engine) = opened();
    let mut frames = replies(&mut server, &mut engine, &Request::ListApps);
    assert_eq!(frames.len(), AppId::ALL.len() + 1);
    for (id, frame) in AppId::ALL.into_iter().zip(&mut frames) {
        let app = engine.apps().get(id);
        assert_eq!(
            decode::<Reply<'_>>(frame),
            Ok(Reply::App {
                app: u8::try_from(id.index()).unwrap(),
                name: id.name(),
                running: id == AppId::Diagnostic,
                params: u8::try_from(app.param_specs().len()).unwrap(),
                observables: u8::try_from(app.observables().len()).unwrap(),
                actions: u8::try_from(app.actions().len()).unwrap(),
            })
        );
    }
    assert_eq!(
        decode::<Reply<'_>>(frames.last_mut().unwrap()),
        Ok(Reply::Done)
    );
}

#[test]
fn an_applet_is_described_with_its_values_and_choice_names() {
    let (mut server, mut engine) = opened();
    let mut frames = replies(
        &mut server,
        &mut engine,
        &Request::DescribeApp { app: RATCHET },
    );
    let decoded: Vec<Reply<'_>> = frames
        .iter_mut()
        .map(|frame| decode(frame).unwrap())
        .collect();
    assert_eq!(
        decoded[0],
        Reply::Spec {
            group: Group::Param,
            index: 0,
            name: "COUNT",
            label: "COUNT",
            kind: ParamKind::Number {
                min: 1,
                max: 32,
                step: 1,
                decimals: 0,
                unit: oc_core::apps::param::Unit::None,
            },
            assignable: true,
            active: true,
            value: Some(ParamValue::Number(4)),
        }
    );
    let mode_names: Vec<_> = decoded
        .iter()
        .filter_map(|reply| match reply {
            Reply::ChoiceName {
                group: Group::Param,
                index: 8,
                name,
                ..
            } => Some(*name),
            _ => None,
        })
        .collect();
    assert_eq!(mode_names, ["FIRST", "SPAN"]);
    assert!(decoded.contains(&Reply::Spec {
        group: Group::Observable,
        index: 0,
        name: "RATE",
        label: "RATE",
        kind: ParamKind::Number {
            min: 0,
            max: 999,
            step: 1,
            decimals: 0,
            unit: oc_core::apps::param::Unit::Bpm,
        },
        assignable: false,
        active: true,
        value: None,
    }));
    assert!(decoded.contains(&Reply::Action {
        index: 0,
        name: "BURST"
    }));
    assert_eq!(decoded.last(), Some(&Reply::Done));
}

#[test]
fn a_parameter_set_from_afar_is_the_applets() {
    let (mut server, mut engine) = opened();
    let prob = 5;
    assert_eq!(
        ask(
            &mut server,
            &mut engine,
            &Request::SetParam {
                app: RATCHET,
                param: prob,
                value: ParamValue::Number(40),
            }
        ),
        Reply::Value(ParamValue::Number(40))
    );
    assert_eq!(engine.app::<RatchetApp>().settings().chance_percent, 40);
    assert_eq!(
        ask(
            &mut server,
            &mut engine,
            &Request::GetParam {
                app: RATCHET,
                param: prob
            }
        ),
        Reply::Value(ParamValue::Number(40))
    );
    assert_eq!(
        ask(
            &mut server,
            &mut engine,
            &Request::SetParam {
                app: RATCHET,
                param: 2,
                value: ParamValue::Number(5_000),
            }
        ),
        Reply::Value(ParamValue::Number(2_000)),
        "FIRST is narrowed by MODE FIRST and says so"
    );
}

#[test]
fn refusals_say_why() {
    let (mut server, mut engine) = opened();
    assert_eq!(
        ask(&mut server, &mut engine, &Request::DescribeApp { app: 99 }),
        Reply::Error(Error::NoSuchApp)
    );
    assert_eq!(
        ask(
            &mut server,
            &mut engine,
            &Request::SetParam {
                app: RATCHET,
                param: 5,
                value: ParamValue::Number(0),
            }
        ),
        Reply::Error(Error::Refused(Refused::OutOfRange))
    );
    assert_eq!(
        ask(
            &mut server,
            &mut engine,
            &Request::GetParam {
                app: RATCHET,
                param: 99
            }
        ),
        Reply::Error(Error::Refused(Refused::NoSuch))
    );
    assert_eq!(
        ask(
            &mut server,
            &mut engine,
            &Request::Observe {
                app: RATCHET,
                index: 99
            }
        ),
        Reply::Error(Error::Refused(Refused::NoSuch))
    );
}

#[test]
fn an_action_and_a_selection_act_as_the_panel_does() {
    let (mut server, mut engine) = opened();
    assert_eq!(
        ask(
            &mut server,
            &mut engine,
            &Request::SelectApp { app: RATCHET }
        ),
        Reply::Done
    );
    assert_eq!(engine.current_app(), AppId::Ratchet);
    assert_eq!(
        ask(
            &mut server,
            &mut engine,
            &Request::Act {
                app: RATCHET,
                action: 0
            }
        ),
        Reply::Done
    );
    assert_eq!(
        ask(
            &mut server,
            &mut engine,
            &Request::Observe {
                app: RATCHET,
                index: 1
            }
        ),
        Reply::Observed(Some(ParamValue::Number(0))),
        "a burst runs, none of its repeats sent yet"
    );
}

#[test]
fn a_label_follows_the_setting_that_changes_it() {
    let (mut server, mut engine) = opened();
    let label_of_first = |server: &mut Server, engine: &mut MockEngine| {
        let mut frames = replies(server, engine, &Request::DescribeApp { app: RATCHET });
        frames
            .iter_mut()
            .find_map(|frame| match decode::<Reply<'_>>(frame).unwrap() {
                Reply::Spec {
                    group: Group::Param,
                    index: 2,
                    name,
                    label,
                    ..
                } => Some((name.to_owned(), label.to_owned())),
                _ => None,
            })
            .unwrap()
    };
    assert_eq!(
        label_of_first(&mut server, &mut engine),
        ("FIRST".to_owned(), "FIRST".to_owned())
    );
    ask(
        &mut server,
        &mut engine,
        &Request::SetParam {
            app: RATCHET,
            param: 8,
            value: ParamValue::Choice(1),
        },
    );
    assert_eq!(
        label_of_first(&mut server, &mut engine),
        ("FIRST".to_owned(), "SPAN".to_owned()),
        "MODE SPAN: the screen says SPAN, the name stays"
    );
}

#[test]
fn an_input_is_read_and_set_from_afar_by_its_parameters_index() {
    use oc_core::apps::ratchet::Setting;
    use oc_core::platform::CvChannel;
    use oc_protocol::{Assign, Effect};

    let (mut server, mut engine) = opened();
    let count = 0;
    let Reply::Assign {
        index,
        assign,
        sweep,
    } = ask(
        &mut server,
        &mut engine,
        &Request::GetAssign {
            app: RATCHET,
            param: count,
        },
    )
    else {
        panic!("COUNT takes an input");
    };
    assert_eq!((index, assign), (count, Assign::NONE));
    assert!(sweep.is_some(), "COUNT says what sweeps it");

    let wanted = Assign {
        input: Some(CvChannel::Three),
        effect: Effect::Set,
        scale_percent: 50,
        ..Assign::NONE
    };
    assert!(matches!(
        ask(
            &mut server,
            &mut engine,
            &Request::SetAssign {
                app: RATCHET,
                param: count,
                assign: wanted,
            }
        ),
        Reply::Assign { assign, .. } if assign == wanted
    ));
    assert_eq!(
        engine.app::<RatchetApp>().assign(Setting::Count),
        Some(&wanted)
    );

    let gate = 1;
    for request in [
        Request::GetAssign {
            app: RATCHET,
            param: gate,
        },
        Request::SetAssign {
            app: RATCHET,
            param: gate,
            assign: wanted,
        },
    ] {
        assert_eq!(
            ask(&mut server, &mut engine, &request),
            Reply::Error(Error::Refused(Refused::NoSuch)),
            "GATE takes no input: {request:?}"
        );
    }
    assert_eq!(
        ask(
            &mut server,
            &mut engine,
            &Request::SetAssign {
                app: RATCHET,
                param: count,
                assign: Assign {
                    min_mv: 1_000,
                    max_mv: 0,
                    ..wanted
                },
            }
        ),
        Reply::Error(Error::Refused(Refused::OutOfRange))
    );
}

#[test]
fn a_description_gives_each_input_after_its_parameter() {
    let (mut server, mut engine) = opened();
    let frames = replies(
        &mut server,
        &mut engine,
        &Request::DescribeApp { app: RATCHET },
    );
    let mut last_param = None;
    let mut inputs = Vec::new();
    for mut frame in frames {
        match decode::<Reply<'_>>(&mut frame).unwrap() {
            Reply::Spec {
                group: Group::Param,
                index,
                assignable,
                ..
            } => last_param = Some((index, assignable)),
            Reply::Assign { index, .. } => {
                assert_eq!(last_param, Some((index, true)), "after its own spec");
                inputs.push(index);
            }
            _ => {}
        }
    }
    assert_eq!(inputs, [0, 2, 4, 5], "COUNT, FIRST, SHAPE, PROB");
}
