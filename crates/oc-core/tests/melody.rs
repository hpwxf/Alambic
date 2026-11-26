//! The Melody applet on the mock engine (journal/D-047): what reaches the
//! outputs for the clocks and resets it receives, and what the panel does to
//! its settings and its steps.

use oc_core::apps::melody::scale::{Chord, Scale, chord};
use oc_core::apps::melody::{GATE_MV, MelodyApp, Setting, UNKNOWN_GATE_MICROS};
use oc_core::apps::{App, AppId};
use oc_core::modulation::Assign;
use oc_core::platform::{Button, CvChannel, MilliVolts, TriggerChannel};
use oc_core::settings::Settings;
use oc_core::testing::{MockEngine, mock_engine_at_boot};

/// An engine running Melody, past the splash.
fn melody() -> MockEngine {
    let mut engine = mock_engine_at_boot(0);
    let mut settings = Settings::DEFAULT;
    settings.startup_app = AppId::Melody;
    engine.apply_settings(settings);
    engine.skip_splash();
    engine
}

fn app(engine: &MockEngine) -> &MelodyApp {
    engine.app::<MelodyApp>()
}

/// One millisecond with `TR1` and `TR2` as given.
fn tick(engine: &mut MockEngine, clock: bool, reset: bool) {
    let triggers = &mut *engine.parts_mut().2;
    triggers.set(TriggerChannel::One, clock);
    triggers.set(TriggerChannel::Two, reset);
    engine.clock().advance(1_000);
    engine.tick();
}

/// A clock of 5 ms, then `rest` ms low.
fn clock(engine: &mut MockEngine, rest: u32) {
    for _ in 0..5 {
        tick(engine, true, false);
    }
    for _ in 0..rest {
        tick(engine, false, false);
    }
}

/// A reset of 5 ms, the clock low.
fn reset(engine: &mut MockEngine) {
    for high in [true, true, true, true, true, false] {
        tick(engine, false, high);
    }
}

/// Turns encoder `index` by `detents`, one tick.
fn turn(engine: &mut MockEngine, index: usize, detents: i8) {
    engine.parts_mut().3.turn(index, detents);
    tick(engine, false, false);
}

/// Presses and releases `button`.
fn press(engine: &mut MockEngine, button: Button) {
    for down in [true, true, true, true, false, false, false, false] {
        engine.parts_mut().3.hold(button, down);
        tick(engine, false, false);
    }
}

/// Turns the panel to `setting`, then it by `turns` detents.
fn set(engine: &mut MockEngine, setting: Setting, turns: i8) {
    while app(engine).selected() != setting {
        turn(engine, 0, 1);
    }
    turn(engine, 1, turns);
}

/// Semitones from C4 at one volt per octave, rounded.
fn millivolts(semitones: i32) -> MilliVolts {
    let product = semitones * 1_000;
    (product + product.signum() * 6) / 12
}

/// What step `step` plays now in the major scale from C4, untransposed.
fn played(engine: &MockEngine, step: usize) -> i32 {
    Scale::Major.quantize(i32::from(app(engine).sequence().note(step)))
}

#[test]
fn melody_keeps_its_place_in_the_list() {
    assert_eq!(AppId::Melody.index(), 7, "persisted: never moves");
    assert_eq!(AppId::Melody.name(), "MELODY");
}

#[test]
fn each_clock_plays_the_next_step_on_a_and_loops() {
    let mut engine = melody();
    assert_eq!(app(&engine).sequence().position(), None, "armed");
    for expected in [0, 1, 2, 3, 4, 5, 6, 7, 0, 1] {
        clock(&mut engine, 20);
        assert_eq!(app(&engine).sequence().position(), Some(expected));
        assert_eq!(
            engine.outputs()[0],
            millivolts(played(&engine, expected)),
            "step {expected}"
        );
    }
}

#[test]
fn a_reset_arms_the_first_step_for_the_next_clock() {
    let mut engine = melody();
    for _ in 0..5 {
        clock(&mut engine, 20);
    }
    let playing = engine.outputs();
    reset(&mut engine);
    assert_eq!(app(&engine).sequence().position(), None, "armed, not moved");
    assert_eq!(
        engine.outputs()[..3],
        playing[..3],
        "the note held until the clock"
    );
    clock(&mut engine, 20);
    assert_eq!(app(&engine).sequence().position(), Some(0));

    // Both on the same tick: the first step.
    clock(&mut engine, 20);
    tick(&mut engine, true, true);
    assert_eq!(app(&engine).sequence().position(), Some(0));
}

#[test]
fn b_and_c_carry_the_chord_stacked_in_the_scale() {
    let mut engine = melody();
    clock(&mut engine, 20);
    let note = played(&engine, 0);
    let tones = chord(Scale::Major, note, Chord::Triad, 0);
    assert_eq!(
        engine.outputs()[1..3],
        tones.map(millivolts),
        "the third and the fifth"
    );
    set(&mut engine, Setting::Chord, 1);
    let tones = chord(Scale::Major, note, Chord::Seventh, 0);
    assert_eq!(engine.outputs()[1..3], tones.map(millivolts), "at once");
}

#[test]
fn d_carries_a_gate_of_the_clocks_period() {
    let mut engine = melody();
    let high = |engine: &MockEngine| engine.outputs()[3] == GATE_MV;
    // Before the rate is known: the default length, closed well before the
    // next clock.
    clock(&mut engine, 95);
    assert!(!high(&engine), "closed again after 10 ms");
    const { assert!(UNKNOWN_GATE_MICROS < 95_000) };
    // 100 ms a clock now, a gate of 50 %.
    let mut ticks = 0;
    for _ in 0..5 {
        tick(&mut engine, true, false);
        ticks += u32::from(high(&engine));
    }
    while high(&engine) {
        tick(&mut engine, false, false);
        ticks += u32::from(high(&engine));
    }
    assert!(
        (50..=51).contains(&ticks),
        "half the period, within a tick: {ticks}"
    );
}

#[test]
fn the_routing_page_sends_the_gate_elsewhere_or_nowhere() {
    let mut engine = melody();
    let ports: Vec<&str> = app(&engine)
        .output_ports()
        .iter()
        .map(|port| port.name)
        .collect();
    assert_eq!(
        ports,
        ["NOTE", "CH1", "CH2", "GATE"],
        "no setting picks D's"
    );
    // The gate nowhere: D free, at 0 V while the clock runs.
    engine.apps_mut().set_route(0, 3, None).unwrap();
    tick(&mut engine, true, false);
    assert_eq!(engine.outputs()[3], 0);
    // CH2 nowhere, the gate on C.
    engine.apps_mut().set_route(0, 2, None).unwrap();
    engine
        .apps_mut()
        .set_route(0, 3, Some(CvChannel::Three))
        .unwrap();
    for _ in 0..30 {
        tick(&mut engine, false, false);
    }
    clock(&mut engine, 0);
    assert_eq!(engine.outputs()[2], GATE_MV, "the gate on C");
}

#[test]
fn no_chance_loops_the_same_notes() {
    let mut engine = melody();
    let first = *app(&engine).sequence().notes();
    for _ in 0..24 {
        clock(&mut engine, 5);
    }
    assert_eq!(*app(&engine).sequence().notes(), first);
    set(&mut engine, Setting::Chance, 100);
    for _ in 0..8 {
        clock(&mut engine, 5);
    }
    assert_ne!(*app(&engine).sequence().notes(), first, "all drawn anew");
}

#[test]
fn reroll_draws_the_whole_sequence_anew() {
    let mut engine = melody();
    let first = *app(&engine).sequence().notes();
    set(&mut engine, Setting::Reroll, 0);
    press(&mut engine, Button::RightEncoder);
    assert_ne!(*app(&engine).sequence().notes(), first);
    assert!(!app(&engine).assigning(), "an action, not a submenu");
}

#[test]
fn the_left_switch_edits_the_steps_on_the_screen() {
    let mut engine = melody();
    press(&mut engine, Button::LeftEncoder);
    assert_eq!(app(&engine).editing(), Some(0));
    turn(&mut engine, 0, 2);
    assert_eq!(app(&engine).editing(), Some(2), "the left encoder: a step");
    let before = i32::from(app(&engine).sequence().note(2));
    turn(&mut engine, 1, 1);
    let after = i32::from(app(&engine).sequence().note(2));
    assert_eq!(
        after,
        Scale::Major.step(before, 1),
        "the right encoder: the next note of the scale"
    );
    turn(&mut engine, 0, -3);
    assert_eq!(app(&engine).editing(), Some(7), "round the loop");
    assert_eq!(
        app(&engine).selected(),
        Setting::Length,
        "settings untouched"
    );
    press(&mut engine, Button::LeftEncoder);
    assert_eq!(app(&engine).editing(), None);
}

#[test]
fn the_editor_frames_the_first_and_the_last_step_on_the_screen() {
    let mut engine = melody();
    press(&mut engine, Button::LeftEncoder);
    for _ in 0..30 {
        tick(&mut engine, false, false);
    }
    // The first step's frame: its left edge on the first column.
    let left_edge = (41..63).all(|y| engine.frame().pixel(0, y));
    assert!(left_edge, "the first step's frame starts on the screen");
    turn(&mut engine, 0, -1);
    for _ in 0..30 {
        tick(&mut engine, false, false);
    }
    assert_eq!(app(&engine).editing(), Some(7));
    let right_edge = (41..63).all(|y| engine.frame().pixel(127, y));
    assert!(right_edge, "the last step's frame ends on the screen");
}

#[test]
fn the_length_changes_the_loop_at_once() {
    let mut engine = melody();
    set(&mut engine, Setting::Length, -5);
    assert_eq!(app(&engine).sequence().length(), 3);
    for expected in [0, 1, 2, 0] {
        clock(&mut engine, 5);
        assert_eq!(app(&engine).sequence().position(), Some(expected));
    }
}

#[test]
fn an_input_given_to_transpose_moves_it_a_volt_an_octave() {
    let mut engine = melody();
    clock(&mut engine, 5);
    let untransposed = engine.outputs()[0];
    engine.parts_mut().0.patch(CvChannel::One, 1_000);
    for _ in 0..8 {
        clock(&mut engine, 5);
    }
    assert_eq!(
        engine.outputs()[0],
        untransposed,
        "no input at start: CV1 moves nothing"
    );
    assert!(
        app(&engine)
            .assign(Setting::Transpose)
            .unwrap()
            .input
            .is_none()
    );
    let transpose = Setting::ALL
        .iter()
        .position(|&setting| setting == Setting::Transpose)
        .unwrap();
    engine
        .apps_mut()
        .at_mut(0)
        .unwrap()
        .set_assign(
            transpose,
            Assign {
                input: Some(CvChannel::One),
                ..Assign::NONE
            },
        )
        .unwrap();
    for _ in 0..8 {
        clock(&mut engine, 5);
    }
    assert_eq!(app(&engine).sequence().position(), Some(0));
    assert_eq!(
        engine.outputs()[0],
        untransposed + 1_000,
        "an octave up, still in the scale"
    );
}

#[test]
fn a_scale_quantises_without_changing_what_the_steps_hold() {
    let mut engine = melody();
    let notes = *app(&engine).sequence().notes();
    set(&mut engine, Setting::Scale, 8);
    assert_eq!(app(&engine).settings().scale, Scale::PentaMajor);
    clock(&mut engine, 5);
    let note = Scale::PentaMajor.quantize(i32::from(notes[0]));
    assert_eq!(engine.outputs()[0], millivolts(note));
    set(&mut engine, Setting::Scale, -8);
    assert_eq!(*app(&engine).sequence().notes(), notes);
}
