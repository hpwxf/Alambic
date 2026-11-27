//! Several applets at once, through the engine and the voices a backend
//! plays (journal/D-046): each on the jacks its route gives it, the same
//! kind twice with settings of its own, a removed one leaving its jacks at
//! 0 V.

use oc_core::apps::ratchet::TRIGGER_MV;
use oc_core::apps::{AppId, MAX_INSTANCES};
use oc_core::buttons::LONG_PRESS_TICKS;
use oc_core::platform::{Button, CvChannel, TriggerChannel};
use oc_core::testing::{MockEngine, adc_reading, mock_engine};

/// One tick of a millisecond.
fn tick(engine: &mut MockEngine) {
    engine.clock().advance(1_000);
    engine.tick();
}

/// `RATCHET` on A and B in the first slot, then `SCOPE` in the second,
/// which only C and D are left for, with the focus; `CV1` at 1.5 V.
fn ratchet_and_scope() -> MockEngine {
    let mut engine = mock_engine(0);
    engine.select_app(AppId::Ratchet);
    assert!(engine.select_app_at(1, AppId::Scope));
    assert!(engine.apps_mut().set_focus(1));
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
    engine.select_app(AppId::Nothing);
    tick(&mut engine);
    assert_eq!(engine.outputs()[2..], [0, 0]);
    assert_eq!(
        (engine.apps().occupied(), engine.current_app()),
        ([true, false, false, false], AppId::Nothing),
        "NOTHING in SCOPE's slot, which keeps the focus"
    );
}

#[test]
fn the_same_applet_twice_keeps_two_sets_of_settings() {
    let mut engine = mock_engine(0);
    engine.select_app(AppId::Ratchet);
    engine.select_app_at(1, AppId::Ratchet);
    engine.apps_mut().set_focus(1);
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
fn up_and_down_move_the_focus_through_every_slot_round_the_ends() {
    let mut engine = ratchet_and_scope();
    assert_eq!(engine.apps().focus(), 1);
    hold(&mut engine, Button::Up, 10);
    assert_eq!(engine.current_app(), AppId::Ratchet, "up: to the left");
    hold(&mut engine, Button::Up, 10);
    assert_eq!(
        (engine.apps().focus(), engine.current_app()),
        (MAX_INSTANCES - 1, AppId::Nothing),
        "up from the first: the last, empty slots too"
    );
    hold(&mut engine, Button::Down, 10);
    assert_eq!(engine.apps().focus(), 0, "down from the last: the first");
    hold(&mut engine, Button::Down, 10);
    assert_eq!(engine.apps().focus(), 1, "down: to the right");
    let long = usize::try_from(LONG_PRESS_TICKS).unwrap() + 10;
    hold(&mut engine, Button::Up, long);
    assert_eq!(engine.apps().focus(), 1, "a long press is not a move");
}

#[test]
fn up_and_down_move_the_focus_on_the_routing_page_too() {
    let mut engine = ratchet_and_scope();
    let long = usize::try_from(LONG_PRESS_TICKS).unwrap() + 10;
    hold(&mut engine, Button::Up, long);
    assert!(engine.routing_is_open());
    hold(&mut engine, Button::Up, 10);
    assert!(engine.routing_is_open(), "a short press leaves it open");
    assert_eq!(engine.current_app(), AppId::Ratchet, "on RATCHET's routes");
    let frame = engine.frame();
    assert!(
        frame.pixel(0, 0) && frame.pixel(2, 0) && !frame.pixel(0, 2) && frame.pixel(1, 2),
        "the marks on the page, RATCHET's across"
    );
    hold(&mut engine, Button::Up, 10);
    assert_eq!(engine.current_app(), AppId::Nothing, "round the left end");
}

#[test]
fn the_screen_marks_the_slots_left_of_the_title() {
    let mut engine = mock_engine(0);
    engine.select_app(AppId::Ratchet);
    for _ in 0..40 {
        tick(&mut engine);
    }
    let frame = engine.frame();
    assert!(
        frame.pixel(0, 0) && frame.pixel(2, 0),
        "RATCHET, focused, across"
    );
    assert!(
        (2..8).all(|y| !frame.pixel(1, y)),
        "the empty slots unmarked"
    );
    engine.select_app_at(1, AppId::Scope);
    for _ in 0..40 {
        tick(&mut engine);
    }
    let frame = engine.frame();
    assert!(
        !frame.pixel(0, 2) && frame.pixel(1, 2) && !frame.pixel(2, 2),
        "SCOPE, running beside, a pixel on the track"
    );
    assert!(!frame.pixel(1, 4), "the third slot still empty");
}

#[test]
fn a_long_up_opens_the_routing_page_where_a_port_goes_off() {
    let mut engine = mock_engine(0);
    engine.select_app(AppId::Supersaw);
    let long = usize::try_from(LONG_PRESS_TICKS).unwrap() + 10;
    hold(&mut engine, Button::Down, long);
    assert!(
        !engine.routing_is_open(),
        "a long down is kept for later (journal/D-046, revision of 2026-11-26)"
    );
    assert_eq!(engine.apps().focus(), 0, "nor a move");
    hold(&mut engine, Button::Up, long);
    assert!(engine.routing_is_open());
    // SUB is the third row: two detents of the left encoder, one of the
    // right down turns it from C to OFF, past B and A, SUPERSAW's own.
    engine.parts_mut().3.turn(0, 2);
    tick(&mut engine);
    engine.parts_mut().3.turn(1, -1);
    tick(&mut engine);
    assert_eq!(engine.apps().route().output(2), None, "SUB off");
    assert!(
        engine.params().as_supersaw().unwrap().parts[2].is_none(),
        "and no longer prepared"
    );
    hold(&mut engine, Button::Up, long);
    assert!(!engine.routing_is_open(), "a long press closes it");
    assert_eq!(engine.apps().occupied(), [true, false, false, false]);
}

#[test]
fn an_instance_reads_the_trigger_its_map_plugs_in() {
    let mut engine = mock_engine(0);
    engine.select_app(AppId::Ratchet);
    engine.select_app_at(1, AppId::Ratchet);
    // The second one's CLOCK from TR2; its A on C.
    engine
        .apps_mut()
        .set_input(1, 0, oc_core::apps::InputSource::Jack(1))
        .unwrap();
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

#[test]
fn an_engine_built_without_apps_runs_the_start_app_its_settings_give() {
    use oc_core::engine::Engine;
    use oc_core::settings::Settings;
    use oc_core::testing::{
        MockAnalogIn, MockAnalogOut, MockControls, MockDigitalIn, MockDisplay, VirtualClock,
    };
    let empty = || {
        Engine::without_apps(
            MockAnalogIn::new(),
            MockAnalogOut::new(),
            MockDigitalIn::new(),
            MockControls::new(),
            VirtualClock::with_read_cost(0),
            MockDisplay::new(),
        )
    };
    let mut engine = empty();
    assert_eq!(engine.apps().occupied(), [false; MAX_INSTANCES], "none yet");
    assert_eq!(engine.apps().current(), AppId::Nothing);
    let mut settings = Settings::DEFAULT;
    settings.startup_app = AppId::Melody;
    engine.apply_settings(settings);
    assert_eq!(engine.apps().current(), AppId::Melody, "START APP");
    // The defaults: what a new engine runs.
    let mut engine = empty();
    engine.apply_settings(Settings::DEFAULT);
    let fresh = oc_core::testing::bare_mock_engine_at_boot(0);
    assert_eq!(engine.apps().current(), fresh.apps().current());
    assert_eq!(engine.apps().route(), fresh.apps().route());
}
