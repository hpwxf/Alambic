//! Behavioural tests for the system settings screen, driven through
//! `Engine::tick` (journal/D-026).

use oc_core::apps::AppId;
use oc_core::menu::MenuEntry;
use oc_core::platform::{Button, CV_CHANNELS, CvChannel};
use oc_core::settings::{DisplaySettings, Settings};
use oc_core::splash::DURATION_MICROS;
use oc_core::system::Line;
use oc_core::testing::{MockEngine, mock_engine, mock_engine_at_boot};

/// Runs `count` ticks of one millisecond each.
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

/// Turns an encoder by `detents` and lets the tick consume it.
fn turn(engine: &mut MockEngine, encoder: usize, detents: i8) {
    {
        let (_, _, _, controls, _) = engine.parts_mut();
        controls.turn(encoder, detents);
    }
    run_ticks(engine, 1);
}

/// The `up` + `down` gesture.
fn chord(engine: &mut MockEngine) {
    hold(engine, Button::Up, true);
    hold(engine, Button::Down, true);
    run_ticks(engine, 10);
    hold(engine, Button::Up, false);
    hold(engine, Button::Down, false);
    run_ticks(engine, 10);
}

/// Opens the settings from the app menu: chord, highlight `SETUP` (the last
/// entry, one step up from the first), confirm.
fn open_setup(engine: &mut MockEngine) {
    chord(engine);
    tap(engine, Button::Up);
    assert_eq!(engine.menu_selection(), MenuEntry::Setup);
    tap(engine, Button::RightEncoder);
    assert!(engine.system_is_open());
}

/// Moves the highlight to `line` with the left encoder.
fn go_to(engine: &mut MockEngine, line: Line) {
    while engine.system().line() != line {
        turn(engine, 0, 1);
    }
}

/// Runs a fresh engine through its splash screen with `buttons` held all
/// along, then releases them.
fn boot_holding(engine: &mut MockEngine, buttons: &[Button]) {
    for &button in buttons {
        hold(engine, button, true);
    }
    let ticks = usize::try_from(DURATION_MICROS / 1_000).unwrap() + 2;
    run_ticks(engine, ticks);
    for &button in buttons {
        hold(engine, button, false);
    }
    run_ticks(engine, 10);
}

#[test]
fn setup_rests_the_outputs_and_the_chord_leaves_without_keeping_edits() {
    let mut engine = mock_engine(0);
    {
        let (analog_in, ..) = engine.parts_mut();
        analog_in.patch(CvChannel::Two, 3_000);
    }
    open_setup(&mut engine);
    go_to(&mut engine, Line::Rotate);
    turn(&mut engine, 1, 1);
    assert!(
        engine.settings().display.rotated,
        "the draft is in force while the screen is open"
    );

    let report = engine.tick();
    assert_eq!(report.cv_out, [0; CV_CHANNELS], "the applet is paused");

    chord(&mut engine);
    assert!(!engine.system_is_open());
    assert_eq!(engine.settings(), &Settings::DEFAULT, "nothing was kept");
    assert!(!engine.take_save_request());
}

#[test]
fn save_keeps_the_edit_asks_the_backend_to_store_it_and_resumes_the_applet() {
    let mut engine = mock_engine(0);
    open_setup(&mut engine);
    go_to(&mut engine, Line::StartupApp);
    turn(&mut engine, 1, 1);
    go_to(&mut engine, Line::Save);
    tap(&mut engine, Button::RightEncoder);

    assert!(!engine.system_is_open());
    assert_eq!(engine.settings().startup_app, AppId::Scope);
    assert!(engine.take_save_request(), "SAVE asks for a store");
    assert!(!engine.take_save_request(), "once");
}

#[test]
fn the_screen_preferences_apply_to_the_picture_and_the_panel() {
    // Two identical modules, one set to rotate: the diagnostic screen shows
    // the tick count, so only frames of the same tick can be compared.
    let mut upright = mock_engine(0);
    let mut turned = mock_engine(0);
    let mut settings = Settings::DEFAULT;
    settings.display.rotated = true;
    settings.display.contrast = 0x40;
    turned.apply_settings(settings);
    run_ticks(&mut upright, 3);
    run_ticks(&mut turned, 3);

    let mut expected = upright.frame().clone();
    expected.rotate_half_turn();
    assert_eq!(turned.frame(), &expected, "the frame is sent turned");
    let (_, _, _, _, display) = turned.parts_mut();
    assert_eq!(display.contrast, Some(0x40));
}

#[test]
fn a_reversed_encoder_turns_the_other_way_outside_the_settings() {
    let mut engine = mock_engine(0);
    turn(&mut engine, 0, 1);
    assert_eq!(engine.diagnostic().selected(), 1);

    let mut settings = Settings::DEFAULT;
    settings.encoder_reversed = [true, false];
    engine.apply_settings(settings);
    turn(&mut engine, 0, 1);
    assert_eq!(engine.diagnostic().selected(), 0, "reversed: one step back");

    // The settings screen reads the encoders as wired.
    open_setup(&mut engine);
    turn(&mut engine, 0, 1);
    assert_eq!(engine.system().line(), Line::Contrast);
}

#[test]
fn holding_up_and_down_through_the_splash_opens_safe_mode() {
    let mut engine = mock_engine_at_boot(0);
    let mut stored = Settings::DEFAULT;
    stored.display.rotated = true;
    stored.encoder_reversed = [true, true];
    engine.apply_settings(stored);

    boot_holding(&mut engine, &[Button::Up, Button::Down]);
    assert!(engine.system_is_open());
    assert!(engine.system().is_safe_mode());
    assert_eq!(engine.settings().display, DisplaySettings::DEFAULT);
    assert!(
        !engine.menu_is_open(),
        "the chord during the splash is not a menu"
    );

    // Leaving without saving restores what was stored.
    chord(&mut engine);
    assert_eq!(engine.settings(), &stored);
}

#[test]
fn holding_the_left_encoder_through_the_splash_opens_calibration() {
    let mut engine = mock_engine_at_boot(0);
    boot_holding(&mut engine, &[Button::LeftEncoder]);
    assert!(engine.system_is_open());
    assert!(!engine.system().is_safe_mode());
    assert_eq!(engine.system().line(), Line::Calibrate);
}

#[test]
fn a_plain_boot_goes_to_the_startup_applet() {
    let mut engine = mock_engine_at_boot(0);
    let mut stored = Settings::DEFAULT;
    stored.startup_app = AppId::Scope;
    engine.apply_settings(stored);
    boot_holding(&mut engine, &[]);
    assert!(!engine.system_is_open());
    assert_eq!(engine.current_app(), AppId::Scope);
}
