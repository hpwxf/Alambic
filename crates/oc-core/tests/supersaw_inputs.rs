//! The `SUPERSAW` applet's inputs on the mock engine (journal/D-037, revision
//! of 2026-11-18): each setting but `STEREO` takes the input its submenu
//! assigns, and an input nobody assigned moves nothing.

use oc_core::apps::AppId;
use oc_core::apps::supersaw::{DEFAULT_AMOUNT, DEFAULT_LEVEL_MV, Setting};
use oc_core::audio::supersaw::FULL;
use oc_core::modulation::{AssignLine, Effect};
use oc_core::platform::{Button, CvChannel};
use oc_core::settings::Settings;
use oc_core::testing::{MockEngine, adc_reading, mock_engine_at_boot};

/// An engine running `SUPERSAW`, past the splash.
fn supersaw() -> MockEngine {
    let mut engine = mock_engine_at_boot(0);
    let mut settings = Settings::DEFAULT;
    settings.startup_app = AppId::Supersaw;
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

/// Picks `setting` with the left encoder.
fn select(engine: &mut MockEngine, setting: Setting) {
    let position = |setting| Setting::ALL.iter().position(|&s| s == setting);
    while engine.apps().supersaw().selected() != setting {
        let towards = if position(engine.apps().supersaw().selected()) < position(setting) {
            1
        } else {
            -1
        };
        turn(engine, 0, towards);
    }
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
    while engine.apps().supersaw().assigning() != Some(line) {
        turn(engine, 0, 1);
    }
    turn(engine, 1, detents);
}

/// Assigns input `cv` (0 for none, 1 to 4) to `setting` with `effect`, from
/// what it takes now, through the submenu.
fn assign(engine: &mut MockEngine, setting: Setting, cv: i8, effect: Effect) {
    select(engine, setting);
    click(engine);
    assert_eq!(
        engine.apps().supersaw().assigning(),
        Some(AssignLine::Input)
    );
    // Down to none, then up to the input wanted.
    turn_line(engine, AssignLine::Input, -4);
    turn_line(engine, AssignLine::Input, cv);
    // From ADD, the default: SET one detent down, MUL one up.
    let effect_turns = match effect {
        Effect::Set => -1,
        Effect::Add => 0,
        Effect::Mul => 1,
    };
    turn_line(engine, AssignLine::Effect, effect_turns);
    click(engine);
    assert_eq!(engine.apps().supersaw().assigning(), None);
}

/// A few millivolts either side of 0 V, as an unplugged input reads on the
/// module.
const NOISE_MV: [i32; 6] = [7, -12, -22, 12, -17, 2];

#[test]
fn an_input_nobody_assigned_moves_nothing() {
    let mut engine = supersaw();
    select(&mut engine, Setting::Detune);
    turn(&mut engine, 1, 60);
    for millivolts in NOISE_MV {
        for channel in [CvChannel::Two, CvChannel::Three, CvChannel::Four] {
            engine.parts_mut().0.patch(channel, millivolts);
        }
        step(&mut engine);
        let played = engine.params().supersaw;
        assert_eq!(played.detune, FULL, "{millivolts} mV");
        assert_eq!(played.mix, DEFAULT_AMOUNT, "{millivolts} mV");
        assert_eq!(played.amplitude, DEFAULT_LEVEL_MV, "{millivolts} mV");
    }
}

#[test]
fn the_note_takes_cv1_at_one_volt_per_octave_until_it_is_removed() {
    let mut engine = supersaw();
    engine.parts_mut().0.patch(CvChannel::One, 1_000);
    step(&mut engine);
    let octave_up = engine.params().supersaw.frequency;
    assign(&mut engine, Setting::Note, 0, Effect::Add);
    assert!(
        engine
            .apps()
            .supersaw()
            .assign(Setting::Note)
            .unwrap()
            .input
            .is_none()
    );
    step(&mut engine);
    let alone = engine.params().supersaw.frequency;
    assert_eq!(alone, 110_000, "A2, CV1 ignored");
    // The input as the converter reads it, a few hertz from 220.
    assert!(octave_up.abs_diff(220_000) < 1_000, "{octave_up}");
}

#[test]
fn an_input_assigned_to_the_detune_adds_twenty_percent_a_volt() {
    let mut engine = supersaw();
    assign(&mut engine, Setting::Detune, 2, Effect::Add);
    engine.parts_mut().0.patch(CvChannel::Two, 1_000);
    step(&mut engine);
    let reading = adc_reading(1_000);
    let expected = i32::from(DEFAULT_AMOUNT) + reading / 5;
    assert_eq!(i32::from(engine.params().supersaw.detune), expected);
    assert_eq!(
        engine.apps().supersaw().detune(),
        DEFAULT_AMOUNT,
        "the panel keeps its value"
    );
}

#[test]
fn an_envelope_multiplying_the_level_makes_a_vca() {
    let mut engine = supersaw();
    assign(&mut engine, Setting::Level, 4, Effect::Mul);
    // The inputs read up to about 6.7 V: an envelope peaks at 5 V.
    for (envelope, level) in [
        (0, 0),
        (2_500, DEFAULT_LEVEL_MV / 2),
        (5_000, DEFAULT_LEVEL_MV),
    ] {
        engine.parts_mut().0.patch(CvChannel::Four, envelope);
        step(&mut engine);
        let reading = adc_reading(envelope);
        let expected = (i64::from(DEFAULT_LEVEL_MV) * i64::from(reading) + 2_500) / 5_000;
        let played = engine.params().supersaw.amplitude;
        assert_eq!(i64::from(played), expected.clamp(0, 3_000), "{envelope} mV");
        assert!(played.abs_diff(level) < 10, "{envelope} mV: {played}");
    }
}

#[test]
fn stereo_takes_no_input() {
    let mut engine = supersaw();
    select(&mut engine, Setting::Stereo);
    click(&mut engine);
    assert_eq!(engine.apps().supersaw().assigning(), None);
    assert!(engine.apps().supersaw().assign(Setting::Stereo).is_none());
}
