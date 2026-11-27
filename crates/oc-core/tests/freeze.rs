//! The freeze (journal/D-049): a long press of the right encoder lets the
//! panel edit a copy of the applet shown while the applet plays on; the
//! release of the next long press publishes every edit at once.

use oc_core::apps::AppId;
use oc_core::apps::melody::MelodyApp;
use oc_core::apps::supersaw::{Setting, SupersawApp};
use oc_core::buttons::LONG_PRESS_TICKS;
use oc_core::modulation::AssignLine;
use oc_core::platform::{Button, Display as _, TriggerChannel};
use oc_core::testing::{MockEngine, mock_engine};

/// One millisecond, `TR1` as given.
fn tick(engine: &mut MockEngine, clock: bool) {
    engine.parts_mut().2.set(TriggerChannel::One, clock);
    engine.clock().advance(1_000);
    engine.tick();
}

/// Turns encoder `index` by `detents`, one tick.
fn turn(engine: &mut MockEngine, index: usize, detents: i8) {
    engine.parts_mut().3.turn(index, detents);
    tick(engine, false);
}

/// Holds `button` for `ticks`, then lets it go.
fn hold(engine: &mut MockEngine, button: Button, ticks: u32) {
    engine.parts_mut().3.hold(button, true);
    for _ in 0..ticks {
        tick(engine, false);
    }
    engine.parts_mut().3.hold(button, false);
    for _ in 0..4 {
        tick(engine, false);
    }
}

/// A long press of the right encoder, held past its long action.
fn long_right(engine: &mut MockEngine) {
    hold(engine, Button::RightEncoder, LONG_PRESS_TICKS + 10);
}

/// A short press of `button`.
fn click(engine: &mut MockEngine, button: Button) {
    hold(engine, button, 4);
}

/// `SUPERSAW` alone, its screen drawn on every tick.
fn supersaw() -> MockEngine {
    let mut engine = mock_engine(0);
    engine.select_app(AppId::Supersaw);
    engine.set_render_interval(1);
    tick(&mut engine, false);
    engine
}

/// Whether the title bar is in inverse video: lit across, but for the
/// text's dark pixels.
fn title_inverted(engine: &mut MockEngine) -> bool {
    let frame = engine.parts_mut().4.frame_mut().clone();
    let lit = (0..128).filter(|&x| frame.pixel(x, 7)).count();
    lit > 100
}

#[test]
fn a_frozen_applet_plays_on_until_the_edits_are_published() {
    let mut engine = supersaw();
    let detune = engine.app::<SupersawApp>().detune();
    long_right(&mut engine);
    assert_eq!(
        engine.apps().frozen(),
        Some(0),
        "frozen once the long press fires"
    );
    assert!(title_inverted(&mut engine), "the title bar says so");

    // DETUNE, two detents up on the panel: the applet does not hear it.
    turn(&mut engine, 0, 1);
    turn(&mut engine, 1, 2);
    assert_eq!(engine.app::<SupersawApp>().detune(), detune, "unpublished");
    assert_eq!(
        engine.app::<SupersawApp>().selected(),
        Setting::Note,
        "the applet's own panel state stays too"
    );

    // The next long press publishes on its release.
    engine.parts_mut().3.hold(Button::RightEncoder, true);
    for _ in 0..LONG_PRESS_TICKS + 10 {
        tick(&mut engine, false);
    }
    assert_eq!(
        engine.app::<SupersawApp>().detune(),
        detune,
        "not before the release"
    );
    engine.parts_mut().3.hold(Button::RightEncoder, false);
    for _ in 0..4 {
        tick(&mut engine, false);
    }
    assert_eq!(engine.apps().frozen(), None);
    assert_ne!(engine.app::<SupersawApp>().detune(), detune, "published");
    assert_eq!(
        engine.app::<SupersawApp>().selected(),
        Setting::Detune,
        "the setting picked carries over"
    );
    assert!(!title_inverted(&mut engine));
}

#[test]
fn a_short_right_click_acts_on_its_release_and_a_long_one_does_not() {
    let mut engine = supersaw();
    engine.parts_mut().3.hold(Button::RightEncoder, true);
    for _ in 0..10 {
        tick(&mut engine, false);
    }
    assert_eq!(
        engine.app::<SupersawApp>().assigning(),
        None,
        "nothing while held: it may become a long press"
    );
    engine.parts_mut().3.hold(Button::RightEncoder, false);
    for _ in 0..4 {
        tick(&mut engine, false);
    }
    assert_eq!(
        engine.app::<SupersawApp>().assigning(),
        Some(AssignLine::Input),
        "NOTE's input submenu, on the release"
    );
    click(&mut engine, Button::RightEncoder);
    long_right(&mut engine);
    assert_eq!(
        engine.app::<SupersawApp>().assigning(),
        None,
        "a long press opens no submenu"
    );
}

#[test]
fn the_panel_stays_on_the_frozen_applet() {
    let mut engine = supersaw();
    assert!(engine.select_app_at(1, AppId::Ratchet));
    long_right(&mut engine);
    click(&mut engine, Button::Down);
    assert_eq!(engine.apps().focus(), 0, "no other applet while frozen");
    hold(&mut engine, Button::Up, LONG_PRESS_TICKS + 10);
    assert!(!engine.routing_is_open(), "no routing page either");
    long_right(&mut engine);
    click(&mut engine, Button::Down);
    assert_eq!(engine.apps().focus(), 1, "free again once published");
}

#[test]
fn melody_keeps_stepping_while_its_steps_are_edited_then_takes_them() {
    let mut engine = mock_engine(0);
    engine.select_app(AppId::Melody);
    tick(&mut engine, false);
    let clock = |engine: &mut MockEngine| {
        for high in (0..50).map(|ms| ms < 5) {
            tick(engine, high);
        }
    };
    clock(&mut engine);
    long_right(&mut engine);
    assert_eq!(engine.apps().frozen(), Some(0));

    // Into the step editor on the copy: the long press there draws a note,
    // it does not publish.
    click(&mut engine, Button::LeftEncoder);
    let place = |engine: &MockEngine, step| engine.app::<MelodyApp>().sequence().place(step);
    let before: Vec<u16> = (0..8).map(|step| place(&engine, step)).collect();
    turn(&mut engine, 1, 5);
    long_right(&mut engine);
    assert_eq!(engine.apps().frozen(), Some(0), "the editor's long press");
    click(&mut engine, Button::LeftEncoder);

    // The applet plays on, its own steps unchanged.
    let position = engine.app::<MelodyApp>().sequence().position();
    clock(&mut engine);
    assert_ne!(engine.app::<MelodyApp>().sequence().position(), position);
    let during: Vec<u16> = (0..8).map(|step| place(&engine, step)).collect();
    assert_eq!(during, before, "unpublished");

    long_right(&mut engine);
    assert_eq!(engine.apps().frozen(), None);
    let after: Vec<u16> = (0..8).map(|step| place(&engine, step)).collect();
    assert_ne!(after, before, "the edited steps, published");
}
