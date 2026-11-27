//! An input that reads another instance's port (journal/D-048): `SUPERSAW`
//! playing the notes of `MELODY` without a cable, assigned in the submenu
//! as a CV jack would be, and the ports that are not offered.

use oc_core::apps::AppId;
use oc_core::apps::melody::{MelodyApp, Setting as MelodySetting};
use oc_core::apps::supersaw::{Setting, SupersawApp};
use oc_core::apps::tone::frequency_from;
use oc_core::modulation::{AssignLine, Effect, Source};
use oc_core::platform::{Button, CvChannel, TriggerChannel};
use oc_core::testing::{MockEngine, mock_engine};

/// One millisecond, `TR1` as given.
fn tick(engine: &mut MockEngine, clock: bool) {
    engine.parts_mut().2.set(TriggerChannel::One, clock);
    engine.clock().advance(1_000);
    engine.tick();
}

/// A clock of 5 ms, then 45 ms low.
fn clock(engine: &mut MockEngine) {
    for high in (0..50).map(|ms| ms < 5) {
        tick(engine, high);
    }
}

/// Turns encoder `index` by `detents`, one tick.
fn turn(engine: &mut MockEngine, index: usize, detents: i8) {
    engine.parts_mut().3.turn(index, detents);
    tick(engine, false);
}

/// Clicks the right encoder: it opens and closes the input submenu.
fn click(engine: &mut MockEngine) {
    for down in [true, true, true, true, false, false, false, false] {
        engine.parts_mut().3.hold(Button::RightEncoder, down);
        tick(engine, false);
    }
}

/// `MELODY` in the first slot, on A to D; `SUPERSAW` in the second, with
/// the focus and no jack left.
fn melody_and_supersaw() -> MockEngine {
    let mut engine = mock_engine(0);
    engine.select_app(AppId::Melody);
    assert!(engine.select_app_at(1, AppId::Supersaw));
    assert!(engine.apps_mut().set_focus(1));
    tick(&mut engine, false);
    engine
}

/// In the focused applet's open submenu, picks `INPUT` and turns it by
/// `detents`.
fn turn_input(engine: &mut MockEngine, detents: i8) {
    while engine.app::<SupersawApp>().assigning() != Some(AssignLine::Input) {
        turn(engine, 0, 1);
    }
    turn(engine, 1, detents);
}

#[test]
fn supersaw_plays_the_notes_of_melody_through_its_note_input() {
    let mut engine = melody_and_supersaw();
    // NOTE, the first setting, takes CV1 at start: four detents further,
    // past CV2 to CV4, the first port of the instance in slot 1.
    click(&mut engine);
    turn_input(&mut engine, 4);
    click(&mut engine);
    let assign = *engine.app::<SupersawApp>().assign(Setting::Note).unwrap();
    assert_eq!(assign.input, Some(Source::Lane { slot: 0, port: 0 }));

    let mut heard = Vec::new();
    for _ in 0..8 {
        clock(&mut engine);
        let note = engine.app::<MelodyApp>().outputs()[0];
        let supersaw = engine.app::<SupersawApp>();
        assert_eq!(
            supersaw.frequency(),
            frequency_from(supersaw.note(), note, Effect::Set),
            "MELODY's pitch itself: assigned, C4 at 0 V, to the millivolt"
        );
        heard.push(supersaw.frequency());
    }
    heard.dedup();
    assert!(heard.len() > 1, "the pitch follows the steps: {heard:?}");
}

#[test]
fn a_port_routed_nowhere_still_feeds_an_input_one_tick_late() {
    let mut engine = melody_and_supersaw();
    engine.apps_mut().set_route(0, 0, None).unwrap();
    let note = engine
        .apps()
        .focused()
        .param_specs()
        .iter()
        .position(|spec| spec.name == "NOTE")
        .unwrap();
    let mut assign = engine.apps().focused().assign(note).unwrap();
    assign.input = Source::lane(0, 0);
    engine
        .apps_mut()
        .focused_mut()
        .set_assign(note, assign)
        .unwrap();

    // The clock's tick: MELODY publishes a step SUPERSAW reads on the next.
    for _ in 0..5 {
        clock(&mut engine);
    }
    let played = |engine: &MockEngine| engine.app::<SupersawApp>().frequency();
    let level = engine.app::<MelodyApp>().outputs()[0];
    let before = played(&engine);
    tick(&mut engine, true);
    let after = engine.app::<MelodyApp>().outputs()[0];
    assert_ne!(level, after, "a step taken");
    assert_eq!(
        played(&engine),
        before,
        "the step's tick: last tick's level"
    );
    tick(&mut engine, true);
    let supersaw = engine.app::<SupersawApp>();
    assert_eq!(
        played(&engine),
        frequency_from(supersaw.note(), after, Effect::Set),
        "one tick later, the step's level, though NOTE reaches no jack"
    );
}

#[test]
fn an_applet_is_never_offered_its_own_ports_nor_an_audio_one() {
    let mut engine = melody_and_supersaw();
    // MELODY's TRANSPOSE: past CV4 there is only SUPERSAW, whose ports
    // carry sound, and MELODY's own.
    assert!(engine.apps_mut().set_focus(0));
    while engine.app::<MelodyApp>().selected() != MelodySetting::Transpose {
        turn(&mut engine, 0, 1);
    }
    click(&mut engine);
    turn(&mut engine, 1, 20);
    assert_eq!(
        engine
            .app::<MelodyApp>()
            .assign(MelodySetting::Transpose)
            .unwrap()
            .input,
        Some(Source::Jack(CvChannel::Four)),
        "stops at CV4"
    );

    // What the submenu offers, as a host sees it.
    let bus = engine.apps().bus(Some(0));
    assert!(!bus.offers(0, 0), "its own");
    assert!(!bus.offers(1, 0), "SUPERSAW's L/MONO");
    assert!(engine.apps().bus(Some(1)).offers(0, 3), "MELODY's GATE");
}
