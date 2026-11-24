//! Several applets at once, through the engine and the voices a backend
//! plays (journal/D-046): each on the jacks its route gives it, the same
//! kind twice with settings of its own, a removed one leaving its jacks at
//! 0 V.

use oc_core::apps::AppId;
use oc_core::apps::ratchet::TRIGGER_MV;
use oc_core::buttons::LONG_PRESS_TICKS;
use oc_core::platform::{Button, CvChannel, TriggerChannel};
use oc_core::testing::{MockEngine, adc_reading, mock_engine};

/// One tick of a millisecond.
fn tick(engine: &mut MockEngine) {
    engine.clock().advance(1_000);
    engine.tick();
}

/// `RATCHET` on A and B, then `SCOPE` beside it, which only C and D are left
/// for; `CV1` at 1.5 V.
fn ratchet_and_scope() -> MockEngine {
    let mut engine = mock_engine(0);
    engine.select_app(AppId::Ratchet);
    assert_eq!(engine.apps_mut().add(AppId::Scope), Some(1));
    engine.parts_mut().0.patch(CvChannel::One, 1_500);
    tick(&mut engine);
    engine
}

#[test]
fn two_applets_play_on_the_jacks_their_routes_give_them() {
    let mut engine = ratchet_and_scope();
    let level = adc_reading(1_500);
    assert_eq!(engine.outputs(), [0, 0, level, level], "SCOPE on C and D");

    engine
        .app_mut(AppId::Ratchet)
        .unwrap()
        .act(0)
        .expect("RATCHET bursts by hand");
    let mut burst = false;
    for _ in 0..3 {
        tick(&mut engine);
        burst |= engine.outputs()[1] == TRIGGER_MV;
        assert_eq!(engine.outputs()[2..], [level, level], "SCOPE plays on");
    }
    assert!(burst, "RATCHET runs without the focus: its burst reaches B");
    assert_eq!(
        engine.current_app(),
        AppId::Scope,
        "the new one has the focus"
    );
}

#[test]
fn a_removed_applet_leaves_its_jacks_at_0_v() {
    let mut engine = ratchet_and_scope();
    assert!(engine.apps_mut().remove(), "SCOPE, which has the focus");
    tick(&mut engine);
    assert_eq!(engine.outputs()[2..], [0, 0]);
    assert_eq!(
        (engine.apps().len(), engine.current_app()),
        (1, AppId::Ratchet)
    );
}

#[test]
fn the_same_applet_twice_keeps_two_sets_of_settings() {
    let mut engine = mock_engine(0);
    engine.select_app(AppId::Ratchet);
    engine.apps_mut().add(AppId::Ratchet);
    let shape = engine
        .apps()
        .focused()
        .param_specs()
        .iter()
        .position(|spec| spec.name == "SHAPE")
        .unwrap();
    let value = engine.apps().focused().get(shape).unwrap();
    let other = engine.apps().focused().param_specs()[shape]
        .check(oc_core::apps::param::ParamValue::Number(-15))
        .unwrap();
    assert_ne!(value, other);
    engine.apps_mut().focused_mut().set(shape, other).unwrap();
    tick(&mut engine);
    let read = |position: usize| -> Option<_> { engine.apps().at(position)?.get(shape) };
    assert_eq!((read(0), read(1)), (Some(value), Some(other)));
}

/// Holds `button` for `ticks`, then lets it go for a few more.
fn hold(engine: &mut MockEngine, button: Button, ticks: usize) {
    engine.parts_mut().3.hold(button, true);
    for _ in 0..ticks {
        tick(engine);
    }
    engine.parts_mut().3.hold(button, false);
    for _ in 0..10 {
        tick(engine);
    }
}

#[test]
fn up_and_down_move_the_focus_without_wrapping() {
    let mut engine = ratchet_and_scope();
    assert_eq!(engine.apps().focus(), 1);
    hold(&mut engine, Button::Up, 10);
    assert_eq!(engine.current_app(), AppId::Ratchet, "up: to the left");
    hold(&mut engine, Button::Up, 10);
    assert_eq!(engine.apps().focus(), 0, "the left end stays");
    hold(&mut engine, Button::Down, 10);
    hold(&mut engine, Button::Down, 10);
    assert_eq!(engine.apps().focus(), 1, "down: to the right, then stays");
    let long = usize::try_from(LONG_PRESS_TICKS).unwrap() + 10;
    hold(&mut engine, Button::Up, long);
    assert_eq!(engine.apps().focus(), 1, "a long press is not a move");
}

#[test]
fn the_screen_marks_the_instances_left_of_the_title() {
    let mut engine = mock_engine(0);
    engine.select_app(AppId::Ratchet);
    for _ in 0..40 {
        tick(&mut engine);
    }
    assert!(
        !engine.frame().pixel(0, 0),
        "one applet alone: no mark, the title's place is empty"
    );
    engine.apps_mut().add(AppId::Scope);
    for _ in 0..40 {
        tick(&mut engine);
    }
    let frame = engine.frame();
    // SCOPE, second, has the focus: its mark two pixels wide.
    assert!(frame.pixel(0, 0) && !frame.pixel(1, 0), "RATCHET");
    assert!(frame.pixel(0, 2) && frame.pixel(1, 2), "SCOPE, focused");
    assert!(!frame.pixel(0, 4), "two marks for two instances");
}

#[test]
fn a_long_press_opens_the_routing_page_where_a_port_goes_off() {
    let mut engine = mock_engine(0);
    engine.select_app(AppId::Supersaw);
    let long = usize::try_from(LONG_PRESS_TICKS).unwrap() + 10;
    hold(&mut engine, Button::Down, long);
    assert!(engine.routing_is_open());
    // SUB is the third row: two detents of the left encoder, one of the
    // right turns it from C to D, the next to OFF.
    engine.parts_mut().3.turn(0, 2);
    tick(&mut engine);
    engine.parts_mut().3.turn(1, 2);
    tick(&mut engine);
    assert_eq!(engine.apps().route().output(2), None, "SUB off");
    assert!(
        engine.params().as_supersaw().unwrap().parts[2].is_none(),
        "and no longer prepared"
    );
    hold(&mut engine, Button::Up, long);
    assert!(!engine.routing_is_open(), "a long press closes it");
    assert_eq!(engine.apps().len(), 1);
}

#[test]
fn an_instance_reads_the_trigger_its_map_plugs_in() {
    let mut engine = mock_engine(0);
    engine.select_app(AppId::Ratchet);
    engine.apps_mut().add(AppId::Ratchet);
    // The second one's CLOCK from TR2; its A on C.
    engine.apps_mut().set_input(1, 0, 1).unwrap();
    let mut copies = [false; 4];
    for step in 0..40 {
        let high = (5..10).contains(&step);
        engine.parts_mut().2.set(TriggerChannel::Two, high);
        tick(&mut engine);
        for (seen, level) in copies.iter_mut().zip(engine.outputs()) {
            *seen |= level == TRIGGER_MV;
        }
    }
    assert_eq!(
        copies,
        [false, false, true, true],
        "TR2 reaches the second RATCHET alone: its A copies it, its B bursts"
    );
}
