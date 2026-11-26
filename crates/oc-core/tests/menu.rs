//! Behavioural tests for the app menu, driven through `Engine::tick`.
//!
//! These run the exact code the firmware runs; only the platform backend is
//! replaced by the deterministic mocks from [`oc_core::testing`].

use oc_core::apps::AppId;
use oc_core::apps::diagnostic::DiagnosticApp;
use oc_core::menu::MenuEntry;
use oc_core::platform::{Button, CV_CHANNELS, CvChannel};
use oc_core::testing::{MockEngine, adc_reading, mock_engine};

/// Runs `count` ticks, advancing the virtual clock by one millisecond each
/// time, as the 1 kHz firmware loop does.
fn run_ticks(engine: &mut MockEngine, count: usize) {
    for _ in 0..count {
        engine.clock().advance(1_000);
        engine.tick();
    }
}

/// Holds or releases a button without running a tick.
fn hold(engine: &mut MockEngine, button: Button, down: bool) {
    let (_, _, _, controls, _) = engine.parts_mut();
    controls.hold(button, down);
}

/// Presses a button and releases it, long enough for its action to fire.
fn tap(engine: &mut MockEngine, button: Button) {
    hold(engine, button, true);
    run_ticks(engine, 5);
    hold(engine, button, false);
    run_ticks(engine, 5);
}

/// Turns `encoder` by `detents` and runs a tick.
fn turn(engine: &mut MockEngine, encoder: usize, detents: i8) {
    {
        let (_, _, _, controls, _) = engine.parts_mut();
        controls.turn(encoder, detents);
    }
    run_ticks(engine, 1);
}

/// Holds `up` and `down` together, then releases both: the menu gesture.
///
/// The two presses land forty ticks apart on purpose — that is what a human
/// hand does, and it is exactly the case a naive combo detector gets wrong.
fn chord(engine: &mut MockEngine) {
    hold(engine, Button::Up, true);
    run_ticks(engine, 40);
    hold(engine, Button::Down, true);
    run_ticks(engine, 10);
    hold(engine, Button::Up, false);
    hold(engine, Button::Down, false);
    run_ticks(engine, 10);
}

#[test]
fn the_up_down_chord_opens_the_menu_without_cycling_the_output_mode() {
    let mut engine = mock_engine(0);
    run_ticks(&mut engine, 1);
    let mode_before = engine.app::<DiagnosticApp>().mode();

    chord(&mut engine);

    assert!(
        engine.menu_is_open(),
        "holding up and down opens the app menu"
    );
    assert_eq!(
        engine.app::<DiagnosticApp>().mode(),
        mode_before,
        "neither button may fire its own action on the way into the menu"
    );
    assert_eq!(
        engine.menu_selection(),
        MenuEntry::App(AppId::Diagnostic),
        "the menu opens on the running app"
    );
}

#[test]
fn pressing_up_then_down_one_after_the_other_does_not_open_the_menu() {
    let mut engine = mock_engine(0);
    tap(&mut engine, Button::Up);
    tap(&mut engine, Button::Down);

    assert!(
        !engine.menu_is_open(),
        "pressed one after the other, not together, so the chord must not fire"
    );
}

#[test]
fn the_chord_closes_the_menu_without_changing_app() {
    let mut engine = mock_engine(0);
    chord(&mut engine);
    assert!(engine.menu_is_open());

    chord(&mut engine);

    assert!(!engine.menu_is_open(), "the chord toggles the menu");
    assert_eq!(engine.current_app(), AppId::Diagnostic);
}

#[test]
fn the_menu_launches_the_highlighted_app() {
    let mut engine = mock_engine(0);
    {
        let (analog_in, ..) = engine.parts_mut();
        analog_in.patch(CvChannel::One, 1_500);
        analog_in.patch(CvChannel::Two, 4_000);
    }
    run_ticks(&mut engine, 2);

    chord(&mut engine);
    tap(&mut engine, Button::Down);
    assert_eq!(engine.menu_selection(), MenuEntry::App(AppId::Scope));

    tap(&mut engine, Button::RightEncoder);

    assert!(!engine.menu_is_open(), "launching closes the menu");
    assert_eq!(engine.current_app(), AppId::Scope);

    engine.clock().advance(1_000);
    engine.tick();
    assert_eq!(
        engine.outputs(),
        [adc_reading(1_500); CV_CHANNELS],
        "the scope buffers CV1 to every output, which the diagnostic applet never does"
    );
}

#[test]
fn only_the_right_encoder_press_launches_the_highlighted_app() {
    let mut engine = mock_engine(0);
    chord(&mut engine);
    tap(&mut engine, Button::Down);
    tap(&mut engine, Button::LeftEncoder);
    assert!(engine.menu_is_open(), "the left click confirms nothing");
    assert_eq!(engine.current_app(), AppId::Diagnostic);

    tap(&mut engine, Button::RightEncoder);
    assert_eq!(engine.current_app(), AppId::Scope);
}

#[test]
fn the_encoders_pick_the_category_then_the_app_in_it() {
    let mut engine = mock_engine(0);
    chord(&mut engine);
    turn(&mut engine, 0, 2);
    assert_eq!(engine.menu_selection(), MenuEntry::App(AppId::Tone));
    turn(&mut engine, 1, 1);
    assert_eq!(engine.menu_selection(), MenuEntry::App(AppId::Supersaw));
    turn(&mut engine, 1, 1);
    assert_eq!(
        engine.menu_selection(),
        MenuEntry::App(AppId::Tone),
        "the right encoder stays in SOUND"
    );
    turn(&mut engine, 1, -1);
    tap(&mut engine, Button::RightEncoder);
    assert_eq!(engine.current_app(), AppId::Supersaw);
}

#[test]
fn reopening_the_menu_highlights_the_running_app() {
    let mut engine = mock_engine(0);
    chord(&mut engine);
    tap(&mut engine, Button::Down);
    tap(&mut engine, Button::RightEncoder);
    assert_eq!(engine.current_app(), AppId::Scope);

    chord(&mut engine);
    assert_eq!(engine.menu_selection(), MenuEntry::App(AppId::Scope));
}

#[test]
fn the_running_app_keeps_driving_the_outputs_while_the_menu_is_open() {
    let mut engine = mock_engine(0);
    {
        let (analog_in, ..) = engine.parts_mut();
        analog_in.patch(CvChannel::Two, 4_000);
    }
    chord(&mut engine);
    assert!(engine.menu_is_open());

    let flushes_before = {
        let (_, analog_out, ..) = engine.parts_mut();
        analog_out.flushes
    };

    engine.clock().advance(1_000);
    engine.tick();

    assert_eq!(
        engine.outputs()[1],
        adc_reading(4_000),
        "the applet underneath keeps mirroring its inputs"
    );
    let (_, analog_out, ..) = engine.parts_mut();
    assert_eq!(
        analog_out.flushes,
        flushes_before + 1,
        "the DAC is still written on every tick"
    );
}

#[test]
fn menu_navigation_does_not_reach_the_running_app() {
    let mut engine = mock_engine(0);
    run_ticks(&mut engine, 1);
    let selected_before = engine.app::<DiagnosticApp>().selected();
    let offset_before = engine.app::<DiagnosticApp>().offset();

    chord(&mut engine);
    {
        let (_, _, _, controls, _) = engine.parts_mut();
        controls.turn(0, 1);
        controls.turn(1, 5);
    }
    run_ticks(&mut engine, 2);
    chord(&mut engine);

    assert!(!engine.menu_is_open());
    assert_eq!(
        engine.app::<DiagnosticApp>().selected(),
        selected_before,
        "the detents that moved the highlight must not also move the applet's channel"
    );
    assert_eq!(
        engine.app::<DiagnosticApp>().offset(),
        offset_before,
        "nor its offset"
    );
}

#[test]
fn picking_another_app_starts_it_afresh_in_the_same_place() {
    let mut engine = mock_engine(0);
    {
        let (_, _, _, controls, _) = engine.parts_mut();
        controls.turn(1, 10);
    }
    run_ticks(&mut engine, 1);
    assert_ne!(engine.app::<DiagnosticApp>().offset(), 0);

    chord(&mut engine);
    tap(&mut engine, Button::Down);
    tap(&mut engine, Button::RightEncoder);
    run_ticks(&mut engine, 20);
    assert_eq!(
        (engine.apps().focus(), engine.current_app()),
        (0, AppId::Scope),
        "in the place of the one left"
    );

    chord(&mut engine);
    tap(&mut engine, Button::Up);
    tap(&mut engine, Button::RightEncoder);

    assert_eq!(engine.current_app(), AppId::Diagnostic);
    assert_eq!(
        engine.app::<DiagnosticApp>().offset(),
        0,
        "a new instance, not the one left (journal/D-046)"
    );
}

#[test]
fn the_menu_screen_replaces_the_applet_screen() {
    let mut engine = mock_engine(0);
    run_ticks(&mut engine, 1);
    let applet_screen = engine.frame().clone();

    chord(&mut engine);
    let menu_screen = engine.frame().clone();
    assert_ne!(
        menu_screen, applet_screen,
        "the menu must take the panel over from the applet"
    );

    chord(&mut engine);
    run_ticks(&mut engine, 1);
    assert_ne!(
        *engine.frame(),
        menu_screen,
        "closing the menu hands the panel back"
    );
}

#[test]
fn a_reset_leaves_the_menu_and_returns_to_the_diagnostic_applet() {
    let mut engine = mock_engine(0);
    chord(&mut engine);
    tap(&mut engine, Button::Down);
    tap(&mut engine, Button::RightEncoder);
    assert_eq!(engine.current_app(), AppId::Scope);

    chord(&mut engine);
    engine.reset();

    assert!(!engine.menu_is_open());
    assert_eq!(engine.current_app(), AppId::Diagnostic);
}

#[test]
fn the_menu_fills_an_empty_slot_and_empties_one_with_nothing() {
    let mut engine = mock_engine(0);
    // The second slot, empty: NOTHING's screen, which the menu fills.
    tap(&mut engine, Button::Down);
    assert_eq!(
        (engine.apps().focus(), engine.current_app()),
        (1, AppId::Nothing)
    );
    chord(&mut engine);
    assert_eq!(engine.menu_selection(), MenuEntry::App(AppId::Nothing));
    tap(&mut engine, Button::Up);
    tap(&mut engine, Button::RightEncoder);
    assert!(!engine.menu_is_open());
    assert_eq!(engine.current_app(), AppId::Supersaw, "in the second slot");
    assert_eq!(
        engine.apps().at(0).map(oc_core::apps::App::id),
        Some(AppId::Diagnostic)
    );

    // NOTHING, last of the apps, one up from SETUP, empties it again.
    chord(&mut engine);
    tap(&mut engine, Button::Down);
    assert_eq!(engine.menu_selection(), MenuEntry::App(AppId::Nothing));
    tap(&mut engine, Button::RightEncoder);
    assert_eq!(
        engine.apps().occupied(),
        [true, false, false, false],
        "the slot empty, its outputs free"
    );
}

#[test]
fn the_menu_keeps_the_slot_marks_left_of_its_title() {
    let mut engine = mock_engine(0);
    // The second slot, empty, takes the focus; the first runs DIAGNOSTIC.
    tap(&mut engine, Button::Down);
    chord(&mut engine);
    assert!(engine.menu_is_open());
    run_ticks(&mut engine, 40);

    let frame = engine.frame();
    assert!(
        !frame.pixel(0, 0) && frame.pixel(1, 0) && !frame.pixel(2, 0),
        "DIAGNOSTIC, running in the first slot, a pixel on the track"
    );
    assert!(
        frame.pixel(0, 2) && frame.pixel(2, 2),
        "the slot the app picked will take, across"
    );
    assert!((0..8).all(|y| !frame.pixel(3, y)), "a gap before the title");
}
