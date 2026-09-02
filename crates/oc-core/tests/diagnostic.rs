//! Behavioural tests for the diagnostic applet, driven through `Engine::tick`.
//!
//! These run the exact code the firmware runs; only the platform backend is
//! replaced by the deterministic mocks from [`oc_core::testing`].

use embedded_graphics::Drawable as _;
use embedded_graphics::mono_font::MonoTextStyle;
use embedded_graphics::mono_font::ascii::FONT_5X8;
use embedded_graphics::pixelcolor::BinaryColor;
use embedded_graphics::prelude::Point;
use embedded_graphics::text::{Baseline, Text};

use oc_core::app::{OFFSET_STEP_MV, OutputMode, ROW_HEIGHT};
use oc_core::calibration::{CV_OUT_MAX_MV, CV_OUT_MIN_MV};
use oc_core::framebuffer::{FrameBuffer, WIDTH};
use oc_core::platform::{Button, CvChannel, TriggerChannel};
use oc_core::testing::{MockEngine, mock_engine};

/// Runs `count` ticks, advancing the virtual clock by one millisecond each
/// time, as the 1 kHz firmware loop does.
fn run_ticks(engine: &mut MockEngine, count: usize) {
    for _ in 0..count {
        engine.clock().advance(1_000);
        engine.tick();
    }
}

/// Presses a button and releases it, running the engine long enough for the
/// action to fire.
///
/// `up` and `down` act on release (see `oc_core::buttons`), so holding one is
/// not enough to make anything happen.
fn tap(engine: &mut MockEngine, button: Button) {
    {
        let (_, _, _, controls, _) = engine.parts_mut();
        controls.hold(button, true);
    }
    run_ticks(engine, 5);
    {
        let (_, _, _, controls, _) = engine.parts_mut();
        controls.hold(button, false);
    }
    run_ticks(engine, 5);
}

/// Renders `text` top-aligned on `row` into an otherwise empty framebuffer.
fn reference_row(text: &str, row: i32) -> FrameBuffer {
    let mut frame = FrameBuffer::new();
    let style = MonoTextStyle::new(&FONT_5X8, BinaryColor::On);
    Text::with_baseline(text, Point::new(0, row * ROW_HEIGHT), style, Baseline::Top)
        .draw(&mut frame)
        .expect("drawing into a framebuffer is infallible");
    frame
}

/// Asserts that one screen row of `frame` matches a reference rendering of
/// `text`.
fn assert_row_reads(frame: &FrameBuffer, row: usize, text: &str) {
    let expected = reference_row(text, i32::try_from(row).unwrap());
    let page = row * WIDTH..(row + 1) * WIDTH;
    assert_eq!(
        &frame.as_bytes()[page.clone()],
        &expected.as_bytes()[page],
        "row {row} should read {text:?}"
    );
}

#[test]
fn a_fresh_engine_starts_in_offset_mode_at_zero_volts() {
    let mut engine = mock_engine(0);
    let report = engine.tick();

    assert_eq!(report.tick_count, 1);
    assert_eq!(
        report.elapsed_micros, 0,
        "the first tick has no predecessor"
    );
    assert_eq!(report.cv_out, [0; 4]);
    assert_eq!(engine.diagnostic().mode(), OutputMode::Offset);
    assert_eq!(engine.diagnostic().selected(), 0);
}

#[test]
fn known_input_levels_are_mirrored_on_the_matching_outputs() {
    let mut engine = mock_engine(0);
    {
        let (analog_in, ..) = engine.parts_mut();
        analog_in.patch(CvChannel::One, 0);
        analog_in.patch(CvChannel::Two, 5_000);
        analog_in.patch(CvChannel::Three, -5_000);
        analog_in.patch(CvChannel::Four, 1_234);
    }

    let report = engine.tick();

    // Channel one is selected, so it carries the offset (0 V) rather than its
    // input. The others mirror their input, clamped to the output range: the
    // inputs reach -5 V but the outputs only go down to -3 V.
    assert_eq!(report.cv_out[0], 0);
    assert_eq!(report.cv_out[1], 5_000, "+5 V is inside the output range");
    assert_eq!(
        report.cv_out[2], CV_OUT_MIN_MV,
        "-5 V must clamp to the output floor rather than wrap"
    );
    assert_eq!(report.cv_out[3], 1_234);
}

#[test]
fn outputs_are_only_visible_after_a_flush() {
    let mut engine = mock_engine(0);
    {
        let (analog_in, ..) = engine.parts_mut();
        analog_in.patch(CvChannel::Four, 2_000);
    }
    engine.tick();

    let (_, analog_out, ..) = engine.parts_mut();
    assert_eq!(
        analog_out.flushes, 1,
        "one flush per tick, not one per channel"
    );
    assert_eq!(analog_out.level(CvChannel::Four), 2_000);
}

#[test]
fn a_clean_trigger_edge_is_counted_exactly_once() {
    let mut engine = mock_engine(0);
    {
        let (_, _, digital_in, ..) = engine.parts_mut();
        digital_in.set(TriggerChannel::One, true);
    }
    run_ticks(&mut engine, 10);

    assert_eq!(engine.diagnostic().trigger_count(TriggerChannel::One), 1);
    assert!(engine.diagnostic().trigger_state(TriggerChannel::One));
    assert_eq!(engine.diagnostic().trigger_count(TriggerChannel::Two), 0);
}

#[test]
fn a_bouncing_trigger_edge_is_still_counted_once() {
    let mut engine = mock_engine(0);
    // A noisy gate: the level flaps for a few milliseconds before settling.
    for level in [true, false, true, false, true, true, true, true, true, true] {
        {
            let (_, _, digital_in, ..) = engine.parts_mut();
            digital_in.set(TriggerChannel::One, level);
        }
        engine.clock().advance(1_000);
        engine.tick();
    }

    assert_eq!(
        engine.diagnostic().trigger_count(TriggerChannel::One),
        1,
        "debouncing must collapse the bounce into a single event"
    );
}

#[test]
fn pressing_the_left_encoder_resets_the_trigger_counters() {
    let mut engine = mock_engine(0);
    for _ in 0..3 {
        for level in [true, true, true, true, false, false, false, false] {
            let (_, _, digital_in, ..) = engine.parts_mut();
            digital_in.set(TriggerChannel::Two, level);
            engine.clock().advance(1_000);
            engine.tick();
        }
    }
    assert_eq!(engine.diagnostic().trigger_count(TriggerChannel::Two), 3);

    {
        let (_, _, _, controls, _) = engine.parts_mut();
        controls.hold(Button::LeftEncoder, true);
    }
    run_ticks(&mut engine, 5);

    assert_eq!(engine.diagnostic().trigger_count(TriggerChannel::Two), 0);
}

#[test]
fn turning_the_right_encoder_moves_the_offset_by_one_step_per_detent() {
    let mut engine = mock_engine(0);
    {
        let (_, _, _, controls, _) = engine.parts_mut();
        controls.turn(1, 3);
    }
    engine.tick();

    assert_eq!(engine.diagnostic().offset(), 3 * OFFSET_STEP_MV);
    assert_eq!(engine.diagnostic().outputs()[0], 3 * OFFSET_STEP_MV);
}

#[test]
fn the_offset_saturates_at_the_output_limits() {
    let mut engine = mock_engine(0);
    for _ in 0..100 {
        {
            let (_, _, _, controls, _) = engine.parts_mut();
            controls.turn(1, 100);
        }
        engine.tick();
    }
    assert_eq!(engine.diagnostic().offset(), CV_OUT_MAX_MV);

    for _ in 0..200 {
        {
            let (_, _, _, controls, _) = engine.parts_mut();
            controls.turn(1, -100);
        }
        engine.tick();
    }
    assert_eq!(engine.diagnostic().offset(), CV_OUT_MIN_MV);
}

#[test]
fn pressing_the_right_encoder_returns_the_offset_to_zero() {
    let mut engine = mock_engine(0);
    {
        let (_, _, _, controls, _) = engine.parts_mut();
        controls.turn(1, 10);
    }
    engine.tick();
    assert_ne!(engine.diagnostic().offset(), 0);

    {
        let (_, _, _, controls, _) = engine.parts_mut();
        controls.hold(Button::RightEncoder, true);
    }
    run_ticks(&mut engine, 5);
    assert_eq!(engine.diagnostic().offset(), 0);
}

#[test]
fn each_encoder_press_is_counted_independently_like_a_trigger_edge() {
    let mut engine = mock_engine(0);
    assert_eq!(
        engine.diagnostic().button_press_count(Button::LeftEncoder),
        0
    );
    assert_eq!(
        engine.diagnostic().button_press_count(Button::RightEncoder),
        0
    );

    for _ in 0..2 {
        {
            let (_, _, _, controls, _) = engine.parts_mut();
            controls.hold(Button::LeftEncoder, true);
        }
        run_ticks(&mut engine, 5);
        {
            let (_, _, _, controls, _) = engine.parts_mut();
            controls.hold(Button::LeftEncoder, false);
        }
        run_ticks(&mut engine, 5);
    }
    {
        let (_, _, _, controls, _) = engine.parts_mut();
        controls.hold(Button::RightEncoder, true);
    }
    run_ticks(&mut engine, 5);

    assert_eq!(
        engine.diagnostic().button_press_count(Button::LeftEncoder),
        2
    );
    assert_eq!(
        engine.diagnostic().button_press_count(Button::RightEncoder),
        1
    );
}

#[test]
fn pressing_both_encoders_at_once_fires_both_actions_in_the_same_tick() {
    let mut engine = mock_engine(0);
    for level in [true, true, true, true, false, false, false, false] {
        let (_, _, digital_in, ..) = engine.parts_mut();
        digital_in.set(TriggerChannel::One, level);
        engine.clock().advance(1_000);
        engine.tick();
    }
    {
        let (_, _, _, controls, _) = engine.parts_mut();
        controls.turn(1, 10);
    }
    engine.tick();
    assert_eq!(engine.diagnostic().trigger_count(TriggerChannel::One), 1);
    assert_ne!(engine.diagnostic().offset(), 0);

    {
        let (_, _, _, controls, _) = engine.parts_mut();
        controls.hold(Button::LeftEncoder, true);
        controls.hold(Button::RightEncoder, true);
    }
    run_ticks(&mut engine, 5);

    assert_eq!(
        engine.diagnostic().trigger_count(TriggerChannel::One),
        0,
        "left encoder still resets the trigger counters when held together with the right"
    );
    assert_eq!(
        engine.diagnostic().offset(),
        0,
        "right encoder still zeroes the offset when held together with the left"
    );
}

#[test]
fn turning_the_left_encoder_selects_a_channel_and_wraps_around() {
    let mut engine = mock_engine(0);
    {
        let (_, _, _, controls, _) = engine.parts_mut();
        controls.turn(0, 2);
    }
    engine.tick();
    assert_eq!(engine.diagnostic().selected(), 2);

    {
        let (_, _, _, controls, _) = engine.parts_mut();
        controls.turn(0, 3);
    }
    engine.tick();
    assert_eq!(
        engine.diagnostic().selected(),
        1,
        "selection wraps, never panics"
    );

    {
        let (_, _, _, controls, _) = engine.parts_mut();
        controls.turn(0, -3);
    }
    engine.tick();
    assert_eq!(engine.diagnostic().selected(), 2, "and wraps downwards too");
}

#[test]
fn the_up_and_down_buttons_cycle_the_output_mode() {
    let mut engine = mock_engine(0);
    assert_eq!(engine.diagnostic().mode(), OutputMode::Offset);

    for expected in [OutputMode::Ramp, OutputMode::Zero, OutputMode::Offset] {
        tap(&mut engine, Button::Up);
        assert_eq!(engine.diagnostic().mode(), expected);
    }

    tap(&mut engine, Button::Down);
    assert_eq!(engine.diagnostic().mode(), OutputMode::Zero);
}

#[test]
fn zero_mode_pins_every_output_regardless_of_the_inputs() {
    let mut engine = mock_engine(0);
    {
        let (analog_in, _, _, controls, _) = engine.parts_mut();
        analog_in.patch(CvChannel::Two, 4_000);
        controls.turn(1, 10);
    }
    tap(&mut engine, Button::Down);

    assert_eq!(engine.diagnostic().mode(), OutputMode::Zero);
    assert_eq!(engine.diagnostic().outputs(), &[0; 4]);
}

#[test]
fn the_ramp_sweeps_the_whole_output_range_and_stays_in_bounds() {
    let mut engine = mock_engine(0);
    tap(&mut engine, Button::Up);
    assert_eq!(engine.diagnostic().mode(), OutputMode::Ramp);

    let mut lowest = CV_OUT_MAX_MV;
    let mut highest = CV_OUT_MIN_MV;
    // A little over one full 2 s period at 1 kHz.
    for _ in 0..2_200 {
        engine.clock().advance(1_000);
        let report = engine.tick();
        for level in report.cv_out {
            assert!(
                (CV_OUT_MIN_MV..=CV_OUT_MAX_MV).contains(&level),
                "the ramp left the output range at {level} mV"
            );
            lowest = lowest.min(level);
            highest = highest.max(level);
        }
    }

    assert_eq!(lowest, CV_OUT_MIN_MV);
    assert!(
        highest > CV_OUT_MAX_MV - 20,
        "the ramp should reach the top"
    );
}

#[test]
fn the_four_ramp_channels_are_a_quarter_period_apart() {
    let mut engine = mock_engine(0);
    tap(&mut engine, Button::Up);

    engine.clock().advance(1_000);
    let report = engine.tick();
    let span = CV_OUT_MAX_MV - CV_OUT_MIN_MV;
    for index in 0..3 {
        let step = report.cv_out[index + 1] - report.cv_out[index];
        let expected = span / 4;
        let normalised = step.rem_euclid(span);
        assert!(
            (normalised - expected).abs() <= 10,
            "channels {index} and {} are {step} mV apart, expected {expected}",
            index + 1
        );
    }
}

#[test]
fn unpatching_an_input_does_not_disturb_the_outputs() {
    let mut engine = mock_engine(0);
    {
        let (analog_in, ..) = engine.parts_mut();
        analog_in.patch(CvChannel::Three, 1_500);
    }
    run_ticks(&mut engine, 4);
    let before = *engine.diagnostic().outputs();

    {
        let (analog_in, ..) = engine.parts_mut();
        analog_in.unpatch(CvChannel::Three);
    }
    run_ticks(&mut engine, 1);

    assert_eq!(
        *engine.diagnostic().outputs(),
        before,
        "cable presence is informational; it must not glitch the outputs"
    );
}

#[test]
fn a_static_input_reads_as_inactive_and_a_moving_one_as_active() {
    let mut engine = mock_engine(0);
    {
        let (analog_in, ..) = engine.parts_mut();
        analog_in.patch(CvChannel::One, 2_000);
    }
    // Two full detection windows of a perfectly steady level.
    run_ticks(&mut engine, 600);
    assert!(!engine.diagnostic().is_signal_active(CvChannel::One));

    for step in 0..600 {
        {
            let (analog_in, ..) = engine.parts_mut();
            analog_in.patch(CvChannel::One, if step % 2 == 0 { -1_000 } else { 1_000 });
        }
        engine.clock().advance(1_000);
        engine.tick();
    }
    assert!(engine.diagnostic().is_signal_active(CvChannel::One));
}

#[test]
fn a_wrapping_microsecond_counter_does_not_produce_absurd_timings() {
    let mut engine = mock_engine(0);
    engine.clock().set(u64::MAX - 500);
    engine.tick();

    engine.clock().advance(1_000);
    let report = engine.tick();

    assert_eq!(
        report.elapsed_micros, 1_000,
        "wrapping arithmetic must survive the rollover"
    );
}

#[test]
fn the_tick_duration_is_measured_and_reported() {
    // The clock charges 40 us per reading, and a tick reads it twice.
    let mut engine = mock_engine(40);
    let first = engine.tick();
    assert_eq!(first.duration_micros, 40);

    let second = engine.tick();
    assert_eq!(second.duration_micros, 40);
    assert_eq!(
        second.elapsed_micros, 80,
        "one tick costs two clock readings"
    );
}

#[test]
fn every_tick_presents_exactly_one_frame() {
    let mut engine = mock_engine(0);
    assert_eq!(engine.render_interval(), 1);
    run_ticks(&mut engine, 7);

    let (_, _, _, _, display) = engine.parts_mut();
    assert_eq!(display.presents, 7);
}

#[test]
fn the_screen_can_be_redrawn_less_often_than_the_signal_path_runs() {
    let mut engine = mock_engine(0);
    engine.set_render_interval(20);

    {
        let (analog_in, ..) = engine.parts_mut();
        analog_in.patch(CvChannel::Two, 1_500);
    }

    let mut rendered = 0;
    for _ in 0..100 {
        engine.clock().advance(1_000);
        let report = engine.tick();
        rendered += u32::from(report.rendered);
        // Whatever the redraw rate, the outputs are refreshed every tick.
        assert_eq!(report.cv_out[1], 1_500);
    }

    assert_eq!(rendered, 5);
    let (_, analog_out, _, _, display) = engine.parts_mut();
    assert_eq!(
        display.presents, 5,
        "the panel is only written when redrawn"
    );
    assert_eq!(analog_out.flushes, 100, "the DAC is written on every tick");
}

#[test]
fn a_zero_render_interval_is_treated_as_every_tick() {
    let mut engine = mock_engine(0);
    engine.set_render_interval(0);
    assert_eq!(engine.render_interval(), 1);
    assert!(engine.tick().rendered);
}

#[test]
fn the_screen_shows_the_banner_and_the_measured_levels() {
    let mut engine = mock_engine(0);
    {
        let (analog_in, ..) = engine.parts_mut();
        analog_in.patch(CvChannel::One, 1_234);
    }
    engine.clock().advance(1_000);
    engine.tick();

    let frame = engine.frame().clone();
    assert_row_reads(&frame, 0, &format!("{}    0us", oc_core::BANNER));
    assert_row_reads(&frame, 1, ">1 +1.234 P-l    0");
    // Channel two: unpatched, no signal detected, gate low, no edges counted.
    assert_row_reads(&frame, 2, " 2 +0.000 .-l    0");
    assert_row_reads(&frame, 5, "MODE OFFS OFS +0.000");
    assert_row_reads(&frame, 6, "OUT +0.0 +0.0 +0.0 +0.0");
    assert_row_reads(&frame, 7, "TICKS 1");
}

#[test]
fn rendering_is_bit_for_bit_reproducible() {
    let run = || {
        let mut engine = mock_engine(0);
        {
            let (analog_in, _, digital_in, controls, _) = engine.parts_mut();
            analog_in.patch(CvChannel::Two, -2_500);
            digital_in.set(TriggerChannel::Three, true);
            controls.turn(1, 4);
        }
        run_ticks(&mut engine, 25);
        engine.frame().clone()
    };

    assert_eq!(run(), run(), "the same inputs must produce the same screen");
}
