//! Behavioural tests for the system settings screen, driven through
//! `Engine::tick` (journal/D-026).

use oc_core::apps::AppId;
use oc_core::apps::diagnostic::DiagnosticApp;
use oc_core::menu::MenuEntry;
use oc_core::platform::{Button, CV_CHANNELS, CvChannel};
use oc_core::settings::{DisplaySettings, Settings};
use oc_core::splash::DURATION_MICROS;
use oc_core::system::Line;
use oc_core::testing::{MockEngine, mock_engine, mock_engine_at_boot};
use oc_core::wizard::Step;

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

    engine.tick();
    assert_eq!(engine.outputs(), [0; CV_CHANNELS], "the applet is paused");

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
    assert_eq!(engine.app::<DiagnosticApp>().selected(), 1);

    let mut settings = Settings::DEFAULT;
    settings.encoder_reversed = [true, false];
    engine.apply_settings(settings);
    turn(&mut engine, 0, 1);
    assert_eq!(
        engine.app::<DiagnosticApp>().selected(),
        0,
        "reversed: one step back"
    );

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
fn holding_the_left_encoder_through_the_splash_opens_the_wizard() {
    let mut engine = mock_engine_at_boot(0);
    boot_holding(&mut engine, &[Button::LeftEncoder]);
    assert!(engine.calibration_is_open());
    assert_eq!(engine.wizard().step(), Step::StartFresh);
}

#[test]
fn the_wizard_drives_the_outputs_and_a_saved_dac_point_reaches_the_jack() {
    let mut engine = mock_engine(0);
    open_setup(&mut engine);
    go_to(&mut engine, Line::Calibrate);
    tap(&mut engine, Button::RightEncoder);
    assert!(engine.calibration_is_open());

    tap(&mut engine, Button::Down);
    assert_eq!(engine.wizard().step(), Step::DacLow(0));
    run_ticks(&mut engine, 1);
    {
        let (_, analog_out, ..) = engine.parts_mut();
        assert_eq!(
            analog_out.level(CvChannel::One),
            -3_000,
            "every output at -3 V"
        );
        assert_eq!(analog_out.level(CvChannel::Four), -3_000);
    }

    // The meter reads low: turn up until it reads -3.000 V.
    let before = engine.settings().calibration.outputs[0].codes[0];
    turn(&mut engine, 1, 4);
    let after = engine.settings().calibration.outputs[0].codes[0];
    assert!(
        after < before,
        "a higher voltage is a lower code on this stage"
    );
    {
        let (_, analog_out, ..) = engine.parts_mut();
        assert_eq!(
            analog_out.codes[0], after,
            "the new code is on the jack at once"
        );
    }

    while engine.wizard().step() != Step::Finish {
        tap(&mut engine, Button::Down);
    }
    tap(&mut engine, Button::RightEncoder);
    assert!(!engine.calibration_is_open());
    assert!(engine.take_save_request());
    assert_eq!(engine.settings().calibration.outputs[0].codes[0], after);
}

#[test]
fn the_chord_abandons_the_wizard() {
    let mut engine = mock_engine(0);
    open_setup(&mut engine);
    go_to(&mut engine, Line::Calibrate);
    tap(&mut engine, Button::RightEncoder);
    tap(&mut engine, Button::Down);
    turn(&mut engine, 1, 10);
    chord(&mut engine);
    assert!(!engine.calibration_is_open());
    assert_eq!(engine.settings(), &Settings::DEFAULT);
    assert!(!engine.take_save_request());
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

/// One tick with output A patched back into `CV1`, as the gain steps ask.
fn tick_with_loopback(engine: &mut MockEngine) {
    let (analog_in, analog_out, ..) = engine.parts_mut();
    let level = analog_out.level(CvChannel::One);
    analog_in.patch(CvChannel::One, level);
    run_ticks(engine, 1);
}

/// Captures on the current ADC step once a full average has settled.
fn capture_with_loopback(engine: &mut MockEngine) {
    for _ in 0..oc_core::wizard::SETTLE_TICKS + oc_core::wizard::AVERAGE_TICKS + 4 {
        tick_with_loopback(engine);
    }
    hold(engine, Button::RightEncoder, true);
    for _ in 0..5 {
        tick_with_loopback(engine);
    }
    hold(engine, Button::RightEncoder, false);
    for _ in 0..5 {
        tick_with_loopback(engine);
    }
}

#[test]
fn a_complete_wizard_run_with_a_loopback_patch_calibrates_the_inputs() {
    use oc_core::calibration::CvInputCalibration;

    let mut engine = mock_engine(0);
    open_setup(&mut engine);
    go_to(&mut engine, Line::Calibrate);
    tap(&mut engine, Button::RightEncoder);
    while engine.wizard().step() != Step::AdcZero {
        tap(&mut engine, Button::Down);
    }

    // Inputs unplugged: the mock jacks sit at 0 V.
    capture_with_loopback(&mut engine);
    tap(&mut engine, Button::Down);
    assert_eq!(engine.wizard().step(), Step::AdcLow);
    capture_with_loopback(&mut engine);
    tap(&mut engine, Button::Down);
    capture_with_loopback(&mut engine);

    let inputs = engine.wizard().draft().calibration.inputs;
    let factory = CvInputCalibration::FACTORY;
    // CV1 carried the loopback during the zero capture as well (0 V then),
    // so every input reads its factory zero back, to the code.
    for input in inputs {
        assert!(
            (input.zero_code - factory.zero_code).abs() <= 1,
            "{input:?}"
        );
        let error = (i64::from(input.nanovolts_per_code) - i64::from(factory.nanovolts_per_code))
            .abs()
            * 1_000
            / i64::from(factory.nanovolts_per_code).abs();
        assert!(
            error <= 5,
            "gain within 0.5 % of the modelled ADC: {input:?}"
        );
    }

    // The gain was measured, not left at the factory value: the modelled ADC
    // quantises 1 V and 3 V to codes 2320 and 1501, 819 codes apart, where
    // the factory slope assumes 819.2.
    assert_ne!(inputs[0].nanovolts_per_code, factory.nanovolts_per_code);
    assert_eq!(inputs[0].nanovolts_per_code, -2_442_002);

    while engine.wizard().step() != Step::Finish {
        tap(&mut engine, Button::Down);
    }
    tap(&mut engine, Button::RightEncoder);
    assert!(engine.take_save_request());
    assert_eq!(engine.settings().calibration.inputs, inputs);
}

#[test]
fn a_reported_save_shows_a_message_that_goes_away() {
    use oc_core::toast::{DURATION_MICROS, SaveOutcome};

    let mut engine = mock_engine(0);
    open_setup(&mut engine);
    go_to(&mut engine, Line::Save);
    tap(&mut engine, Button::RightEncoder);
    assert!(engine.take_save_request());

    engine.report_save(SaveOutcome::Stored);
    run_ticks(&mut engine, 1);
    assert_eq!(engine.toast(), Some(SaveOutcome::Stored));
    let shown = engine.frame().lit_pixels();

    run_ticks(
        &mut engine,
        usize::try_from(DURATION_MICROS / 1_000).unwrap() + 1,
    );
    assert_eq!(engine.toast(), None, "gone after its duration");
    assert_ne!(engine.frame().lit_pixels(), shown, "and off the screen");
}

#[test]
fn a_forced_output_overrides_the_applet_until_released() {
    let mut engine = mock_engine(0);
    run_ticks(&mut engine, 2);
    engine.tick();
    let free = engine.outputs();

    engine.force_output(CvChannel::Two, Some(2_500));
    engine.tick();
    assert_eq!(engine.outputs()[1], 2_500);
    assert_eq!(engine.parts_mut().1.committed[1], 2_500);
    assert_eq!(engine.outputs()[0], free[0], "other outputs untouched");

    engine.force_output(CvChannel::Two, None);
    engine.tick();
    assert_eq!(engine.outputs(), free);
}

#[test]
fn a_reset_releases_every_forced_output() {
    let mut engine = mock_engine(0);
    engine.force_output(CvChannel::Four, Some(-1_000));
    engine.reset();
    assert_eq!(engine.forced_outputs(), &[None; CV_CHANNELS]);
}

#[test]
fn the_wizard_ignores_forced_outputs() {
    let mut engine = mock_engine_at_boot(0);
    boot_holding(&mut engine, &[Button::LeftEncoder]);
    assert!(engine.calibration_is_open());
    engine.force_output(CvChannel::One, Some(4_000));
    engine.tick();
    assert_eq!(
        engine.outputs(),
        [engine.wizard().output_level(); CV_CHANNELS]
    );
}

#[test]
fn the_screen_shows_a_forced_output_as_the_jack_carries_it() {
    use oc_core::platform::Display as _;

    let mut free = mock_engine(0);
    let mut forced = mock_engine(0);
    forced.force_output(CvChannel::One, Some(1_000));
    // The screen shows what the outputs played on the tick before
    // (journal/D-040): two ticks, the level then its picture.
    for engine in [&mut free, &mut forced] {
        engine.tick();
        engine.tick();
    }
    let free_frame = free.parts_mut().4.frame_mut().clone();
    let forced_frame = forced.parts_mut().4.frame_mut().clone();
    assert_ne!(free_frame, forced_frame, "the OUT row must change");

    // Released, the two screens agree again.
    forced.force_output(CvChannel::One, None);
    for engine in [&mut free, &mut forced] {
        engine.tick();
        engine.tick();
    }
    assert_eq!(
        free.parts_mut().4.frame_mut().clone(),
        forced.parts_mut().4.frame_mut().clone()
    );
}
