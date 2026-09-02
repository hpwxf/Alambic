//! Behavioural tests for the boot splash screen, driven through
//! `Engine::tick` exactly like `tests/diagnostic.rs`.
//!
//! [`mock_engine_at_boot`] is used here instead of [`mock_engine`], since the
//! latter starts past the animation on purpose (see its own documentation).

use oc_core::app::OutputMode;
use oc_core::platform::CvChannel;
use oc_core::splash::DURATION_MICROS;
use oc_core::testing::{MockEngine, mock_engine_at_boot};

/// Number of one-millisecond ticks that comfortably run past the end of the
/// animation.
const TICKS_PAST_BOOT: u32 = DURATION_MICROS / 1_000 + 2;

/// Runs `count` one-millisecond ticks, as the firmware's 1 kHz loop does.
fn run_ticks(engine: &mut MockEngine, count: u32) {
    for _ in 0..count {
        engine.clock().advance(1_000);
        engine.tick();
    }
}

#[test]
fn a_fresh_engine_shows_the_banner_and_holds_the_outputs_at_rest() {
    let mut engine = mock_engine_at_boot(0);
    {
        let (analog_in, ..) = engine.parts_mut();
        analog_in.patch(CvChannel::One, 2_500);
    }

    let report = engine.tick();
    assert_eq!(report.cv_out, [0; 4], "the outputs must stay at rest");
    assert!(report.rendered);

    let frame = engine.frame().clone();
    assert!(
        frame.lit_pixels() > 0,
        "the banner is drawn on the very first frame"
    );
    assert!(
        frame
            .as_bytes()
            .iter()
            .zip(oc_core::framebuffer::FrameBuffer::new().as_bytes())
            .any(|(a, b)| a != b),
        "the splash frame must differ from a blank screen"
    );
}

#[test]
fn the_border_grows_across_several_frames() {
    let mut engine = mock_engine_at_boot(0);
    engine.tick();
    let first = engine.frame().lit_pixels();

    run_ticks(&mut engine, TICKS_PAST_BOOT / 2);
    let mid = engine.frame().lit_pixels();

    assert!(
        mid > first,
        "the border should have grown by the middle of the animation ({mid} <= {first})"
    );
}

#[test]
fn inputs_are_ignored_until_the_animation_completes() {
    let mut engine = mock_engine_at_boot(0);
    {
        let (analog_in, ..) = engine.parts_mut();
        analog_in.patch(CvChannel::Two, 5_000);
    }

    run_ticks(&mut engine, TICKS_PAST_BOOT - 1);
    assert_eq!(
        engine.diagnostic().outputs(),
        &[0; 4],
        "the applet must not have processed anything yet"
    );

    run_ticks(&mut engine, 1);
    engine.clock().advance(1_000);
    let report = engine.tick();

    assert_eq!(
        report.cv_out[1], 5_000,
        "normal execution begins once the border has fully traced"
    );
    assert_eq!(engine.diagnostic().mode(), OutputMode::Offset);
}

#[test]
fn control_input_during_the_animation_is_not_replayed_afterwards() {
    let mut engine = mock_engine_at_boot(0);
    {
        let (_, _, _, controls, _) = engine.parts_mut();
        controls.turn(1, 50);
    }

    run_ticks(&mut engine, TICKS_PAST_BOOT);

    assert_eq!(
        engine.diagnostic().offset(),
        0,
        "an encoder turn during the boot animation must not carry over"
    );
}

#[test]
fn resetting_a_booted_engine_plays_the_animation_again() {
    let mut engine = mock_engine_at_boot(0);
    run_ticks(&mut engine, TICKS_PAST_BOOT);
    {
        let (_, _, _, controls, _) = engine.parts_mut();
        controls.turn(1, 5);
    }
    engine.tick();
    assert_ne!(engine.diagnostic().offset(), 0);

    engine.reset();
    assert_eq!(
        engine.diagnostic().offset(),
        0,
        "reset restores the default applet state"
    );

    let report = engine.tick();
    assert_eq!(
        report.cv_out, [0; 4],
        "a reset engine boots again before resuming normal execution"
    );
}

#[test]
fn skipping_the_splash_screen_starts_normal_execution_immediately() {
    let mut engine = mock_engine_at_boot(0);
    engine.skip_splash();
    {
        let (analog_in, ..) = engine.parts_mut();
        analog_in.patch(CvChannel::Three, -1_000);
    }

    let report = engine.tick();
    assert_eq!(report.cv_out[2], -1_000);
}

#[test]
fn the_animation_is_bit_for_bit_reproducible() {
    let run = || {
        let mut engine = mock_engine_at_boot(0);
        run_ticks(&mut engine, TICKS_PAST_BOOT / 2);
        engine.frame().clone()
    };

    assert_eq!(
        run(),
        run(),
        "the same elapsed time must draw the same frame"
    );
}
