//! The applets' parameter model (journal/D-043): what every applet
//! declares holds together, and a value set through the model behaves as one
//! turned on the panel.

use oc_core::apps::param::{Kind, ParamValue, Refused, write_summary};
use oc_core::apps::ratchet::{RatchetApp, Setting};
use oc_core::apps::supersaw::SupersawApp;
use oc_core::apps::tone::ToneApp;
use oc_core::apps::{App, AppHost, AppId, InputSnapshot};

/// One tick's snapshot turning the left encoder by `left` and the right one
/// by `right` detents.
fn turning(left: i8, right: i8) -> InputSnapshot {
    let mut input = InputSnapshot {
        elapsed_micros: 1_000,
        ..InputSnapshot::default()
    };
    input.controls.encoder_delta = [left, right];
    input
}

/// Index of the parameter named `name` in `app`.
fn index_of(app: &dyn App, name: &str) -> usize {
    app.param_specs()
        .iter()
        .position(|spec| spec.name == name)
        .unwrap_or_else(|| panic!("{} has no {name}", app.name()))
}

#[test]
fn every_parameter_reads_a_value_of_its_kind_and_takes_it_back() {
    let mut host = AppHost::new();
    for id in AppId::ALL {
        let app = host.get_mut(id);
        let specs = app.param_specs();
        for (index, spec) in specs.iter().enumerate() {
            let value = app
                .get(index)
                .unwrap_or_else(|| panic!("{} {}", app.name(), spec.name));
            assert_eq!(spec.check(value), Ok(value), "{} {}", app.name(), spec.name);
            assert_eq!(
                app.set(index, value),
                Ok(value),
                "{} {}: setting a value read leaves it",
                app.name(),
                spec.name
            );
        }
        assert_eq!(app.get(specs.len()), None, "{}", app.name());
        assert_eq!(
            app.set(specs.len(), ParamValue::Toggle(true)),
            Err(Refused::NoSuch),
            "{}",
            app.name()
        );
        assert_eq!(app.observe(app.observables().len()), None);
        assert_eq!(app.act(app.actions().len()), Err(Refused::NoSuch));
    }
}

#[test]
fn names_are_unique_within_an_applet() {
    let host = AppHost::new();
    for id in AppId::ALL {
        let app = host.get(id);
        let names: Vec<_> = app
            .param_specs()
            .iter()
            .chain(app.observables())
            .map(|spec| spec.name)
            .collect();
        for (index, name) in names.iter().enumerate() {
            assert!(
                !names[index + 1..].contains(name),
                "{} declares {name} twice",
                app.name()
            );
        }
    }
}

#[test]
fn a_value_outside_its_range_or_of_another_kind_is_refused() {
    let mut ratchet = RatchetApp::new();
    let prob = index_of(&ratchet, "PROB");
    assert_eq!(
        ratchet.set(prob, ParamValue::Number(0)),
        Err(Refused::OutOfRange)
    );
    assert_eq!(
        ratchet.set(prob, ParamValue::Toggle(true)),
        Err(Refused::WrongKind)
    );
    assert_eq!(
        ratchet.get(prob),
        Some(ParamValue::Number(100)),
        "a refused value changes nothing"
    );
}

#[test]
fn a_value_set_through_the_model_is_the_one_the_panel_reads() {
    let mut ratchet = RatchetApp::new();
    let prob = index_of(&ratchet, "PROB");
    ratchet.set(prob, ParamValue::Number(60)).unwrap();
    assert_eq!(ratchet.settings().chance_percent, 60);

    // Three detents down from there, on the panel.
    while ratchet.selected() != Setting::Chance {
        RatchetApp::update(&mut ratchet, &turning(1, 0));
    }
    RatchetApp::update(&mut ratchet, &turning(0, -3));
    assert_eq!(ratchet.get(prob), Some(ParamValue::Number(57)));
}

#[test]
fn ratchets_first_interval_keeps_within_its_mode() {
    let mut ratchet = RatchetApp::new();
    let first = index_of(&ratchet, "FIRST");
    let mode = index_of(&ratchet, "MODE");
    assert_eq!(
        ratchet.set(first, ParamValue::Number(5_000)),
        Ok(ParamValue::Number(2_000)),
        "a first interval is two seconds at most"
    );
    ratchet.set(mode, ParamValue::Choice(1)).unwrap();
    assert_eq!(
        ratchet.set(first, ParamValue::Number(5_000)),
        Ok(ParamValue::Number(5_000)),
        "a span goes to eight"
    );
    ratchet.set(mode, ParamValue::Choice(0)).unwrap();
    assert_eq!(
        ratchet.get(first),
        Some(ParamValue::Number(2_000)),
        "back to FIRST, the time comes back within its range"
    );
}

#[test]
fn ratchets_gate_is_the_count_past_the_last() {
    let mut ratchet = RatchetApp::new();
    let count = index_of(&ratchet, "COUNT");
    let gate = index_of(&ratchet, "GATE");
    ratchet.set(gate, ParamValue::Toggle(true)).unwrap();
    assert_eq!(ratchet.get(count), Some(ParamValue::Number(32)));
    ratchet.set(gate, ParamValue::Toggle(false)).unwrap();
    assert_eq!(
        ratchet.get(count),
        Some(ParamValue::Number(32)),
        "out of the gate at the last count, as a detent down"
    );
    ratchet.set(count, ParamValue::Number(4)).unwrap();
    assert_eq!(ratchet.get(gate), Some(ParamValue::Toggle(false)));
}

#[test]
fn ratchets_burst_action_starts_a_burst_as_up_does() {
    let mut ratchet = RatchetApp::new();
    let sent = ratchet
        .observables()
        .iter()
        .position(|spec| spec.name == "SENT")
        .unwrap();
    assert_eq!(ratchet.observe(sent), None, "nothing runs at start");
    assert_eq!(ratchet.actions(), ["BURST"]);
    ratchet.act(0).unwrap();
    assert_eq!(ratchet.observe(sent), Some(ParamValue::Number(0)));
    assert_eq!(ratchet.repeats_sent(), Some(0));
}

#[test]
fn supersaws_note_moves_the_pitch_it_plays() {
    let mut saw = SupersawApp::new();
    let note = index_of(&saw, "NOTE");
    let freq = saw
        .observables()
        .iter()
        .position(|spec| spec.name == "FREQ")
        .unwrap();
    saw.set(note, ParamValue::Number(69)).unwrap();
    assert_eq!(saw.observe(freq), Some(ParamValue::Number(440_000)));
    assert!(matches!(saw.param_specs()[note].kind, Kind::Number { .. }));
}

#[test]
fn tones_note_is_heard_at_once() {
    let mut tone = ToneApp::new();
    tone.set(0, ParamValue::Number(81)).unwrap();
    assert_eq!(tone.observe(0), Some(ParamValue::Number(880_000)));
}

#[test]
fn the_summary_names_every_parameter_and_what_is_known() {
    let mut line = String::new();
    write_summary(&RatchetApp::new(), &mut line).unwrap();
    assert_eq!(
        line,
        "COUNT 4   GATE OFF   FIRST 100 MS   SHAPE 0.0   PROB 100%   PASSTHRU ON   SYNC OFF   \
         MODE FIRST",
        "no rate before two edges, no burst under way: nothing more to say"
    );
}

#[test]
fn ratchets_inactive_faces_follow_sync_and_gate() {
    let mut ratchet = RatchetApp::new();
    let [count, gate, first, division, sync] =
        ["COUNT", "GATE", "FIRST", "DIVISION", "SYNC"].map(|name| index_of(&ratchet, name));
    assert!(ratchet.param_active(first) && !ratchet.param_active(division));
    ratchet.set(sync, ParamValue::Toggle(true)).unwrap();
    assert!(!ratchet.param_active(first) && ratchet.param_active(division));
    assert!(ratchet.param_active(count));
    ratchet.set(gate, ParamValue::Toggle(true)).unwrap();
    assert!(!ratchet.param_active(count) && ratchet.param_active(gate));
    // An inactive face still takes a value, ready for when it comes back.
    assert_eq!(
        ratchet.set(first, ParamValue::Number(250)),
        Ok(ParamValue::Number(250))
    );
}
