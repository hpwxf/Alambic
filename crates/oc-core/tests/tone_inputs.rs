//! The `TONE` applet's inputs on the mock engine: no input moves the tone at
//! start, and each setting takes the input its submenu assigns, as in
//! `SUPERSAW`.

use oc_core::apps::AppId;
use oc_core::apps::tone::{DEFAULT_AMPLITUDE_MV, REFERENCE_NOTE, Setting, ToneApp};
use oc_core::modulation::{AssignLine, Effect, Source};
use oc_core::platform::{Button, CvChannel};
use oc_core::settings::Settings;
use oc_core::testing::{MockEngine, adc_reading, mock_engine_at_boot};

/// An engine running `TONE`, past the splash.
fn tone() -> MockEngine {
    let mut engine = mock_engine_at_boot(0);
    let mut settings = Settings::DEFAULT;
    settings.startup_app = AppId::Tone;
    engine.apply_settings(settings);
    engine.skip_splash();
    engine
}

/// One tick.
fn step(engine: &mut MockEngine) {
    engine.clock().advance(1_000);
    engine.tick();
}

/// Turns encoder `index` by `detents`, one tick.
fn turn(engine: &mut MockEngine, index: usize, detents: i8) {
    engine.parts_mut().3.turn(index, detents);
    step(engine);
}

/// Clicks the right encoder: the switch opens and closes the input submenu.
fn click(engine: &mut MockEngine) {
    for down in [true, true, true, true, false, false, false, false] {
        engine.parts_mut().3.hold(Button::RightEncoder, down);
        step(engine);
    }
}

/// In the open submenu, picks `line` and turns it by `detents`.
fn turn_line(engine: &mut MockEngine, line: AssignLine, detents: i8) {
    while engine.app::<ToneApp>().assigning() != Some(line) {
        turn(engine, 0, 1);
    }
    turn(engine, 1, detents);
}

/// Sets every input to `millivolts`, then ticks so that the applet reads it.
fn hold_inputs(engine: &mut MockEngine, millivolts: [i32; 4]) {
    let channels = [
        CvChannel::One,
        CvChannel::Two,
        CvChannel::Three,
        CvChannel::Four,
    ];
    for (channel, value) in channels.into_iter().zip(millivolts) {
        engine.parts_mut().0.patch(channel, value);
    }
    step(engine);
}

#[test]
fn no_input_moves_the_tone_at_start() {
    let mut engine = tone();
    hold_inputs(&mut engine, [8, -12, 500, 2_000]);
    let tone = engine.app::<ToneApp>();
    assert_eq!(tone.note(), REFERENCE_NOTE);
    assert_eq!(tone.frequency(), 440_000, "CV1's noise moves nothing");
    assert_eq!(tone.played_amplitude(), DEFAULT_AMPLITUDE_MV);
}

#[test]
fn the_note_takes_cv1_through_its_submenu() {
    let mut engine = tone();
    click(&mut engine);
    assert_eq!(engine.app::<ToneApp>().assigning(), Some(AssignLine::Input));
    turn_line(&mut engine, AssignLine::Input, 1);
    assert_eq!(
        engine.app::<ToneApp>().note(),
        REFERENCE_NOTE,
        "the submenu has the encoders"
    );
    click(&mut engine);
    assert_eq!(engine.app::<ToneApp>().assigning(), None);
    let tone = engine.app::<ToneApp>();
    assert_eq!(
        tone.assign(Setting::Note).input,
        Some(Source::Jack(CvChannel::One))
    );
    assert_eq!(tone.assign(Setting::Note).effect, Effect::Add);

    hold_inputs(&mut engine, [500, 0, 0, 0]);
    let expected = oc_core::apps::tone::frequency(REFERENCE_NOTE, adc_reading(500));
    assert_eq!(engine.app::<ToneApp>().frequency(), expected);
}

#[test]
fn the_level_takes_an_input_as_a_vca() {
    let mut engine = tone();
    turn(&mut engine, 0, 1);
    assert_eq!(engine.app::<ToneApp>().selected(), Setting::Level);
    click(&mut engine);
    turn_line(&mut engine, AssignLine::Input, 2);
    // From ADD, the default, MUL one detent up.
    turn_line(&mut engine, AssignLine::Effect, 1);
    click(&mut engine);
    let assign = *engine.app::<ToneApp>().assign(Setting::Level);
    assert_eq!(
        (assign.input, assign.effect),
        (Some(Source::Jack(CvChannel::Two)), Effect::Mul)
    );

    hold_inputs(&mut engine, [0, 2_500, 0, 0]);
    let tone = engine.app::<ToneApp>();
    let played = tone.played_amplitude();
    assert!(
        played.abs_diff(DEFAULT_AMPLITUDE_MV / 2) <= 5,
        "half the level at 2.5 V: {played}"
    );
    assert_eq!(tone.amplitude(), DEFAULT_AMPLITUDE_MV, "the panel's stays");
}
