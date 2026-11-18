//! Robustness tests for the C ABI.
//!
//! These exercise exactly the inputs a C++ plugin could plausibly send by
//! mistake — null pointers, out-of-range indices, a tick before any input is
//! configured — and assert the crate's own promise: none of it panics, and
//! every getter falls back to a harmless, documented default.

use oc_core::framebuffer::LEN as FRAMEBUFFER_LEN;
use oc_core::platform::{BUTTONS, CV_CHANNELS, ENCODERS, TRIGGER_CHANNELS};
use oc_core::splash::DURATION_MICROS;
use oc_core::testing::adc_reading;
use oc_vcv_ffi::{
    OcEngine, oc_engine_audio, oc_engine_button, oc_engine_buttons, oc_engine_cv_channels,
    oc_engine_cv_out, oc_engine_encoder, oc_engine_encoders, oc_engine_framebuffer,
    oc_engine_framebuffer_len, oc_engine_free, oc_engine_new, oc_engine_set_cv_in,
    oc_engine_set_trigger, oc_engine_settings_load, oc_engine_settings_max_len,
    oc_engine_settings_save, oc_engine_tick, oc_engine_trigger_channels,
};

/// Ticks `engine` comfortably past the end of the boot splash screen, so a
/// test can exercise steady-state applet behaviour on the very next tick
/// instead of waiting out the real animation.
fn skip_boot(engine: *mut OcEngine) {
    let ticks = DURATION_MICROS / 1_000 + 2;
    for step in 0..u64::from(ticks) {
        // SAFETY: `engine` is live for the duration of the calling test.
        unsafe { oc_engine_tick(engine, (step + 1) * 1_000) };
    }
}

/// Every function that takes an `engine` pointer must survive a null one.
///
/// A C++ plugin that fails to check `oc_engine_new`'s return value (which the
/// ABI documents as possible, however unlikely) must not crash the host on
/// its very next call.
#[test]
fn null_engine_pointers_are_safe_everywhere() {
    let null_mut = std::ptr::null_mut();
    let null_const = std::ptr::null();

    // SAFETY: every function's contract explicitly allows a null pointer.
    unsafe {
        oc_engine_set_cv_in(null_mut, 0, 2_500);
        oc_engine_set_trigger(null_mut, 0, true);
        oc_engine_encoder(null_mut, 0, 1, true);
        oc_engine_button(null_mut, 0, true);
        oc_engine_tick(null_mut, 1_000);
        oc_engine_audio(null_mut, 48_000);
        oc_engine_free(null_mut);

        assert_eq!(oc_engine_cv_out(null_const, 0), 0);
        assert!(oc_engine_framebuffer(null_const).is_null());
    }
}

/// Out-of-range channel and index arguments are ignored rather than
/// clamped, wrapped, or panicking, on every setter that takes one.
#[test]
fn out_of_range_indices_are_ignored_not_clamped() {
    let engine = oc_engine_new();
    assert!(!engine.is_null());

    // SAFETY: `engine` is live for the duration of this test and never
    // shared across threads.
    unsafe {
        oc_engine_set_cv_in(engine, u8::MAX, 2_500);
        oc_engine_set_trigger(engine, u8::MAX, true);
        oc_engine_encoder(engine, u8::MAX, 1, true);
        oc_engine_button(engine, u8::MAX, true);
        oc_engine_tick(engine, 1_000);

        // None of the out-of-range writes above should have landed anywhere
        // a valid index could read back.
        for channel in 0..u8::try_from(CV_CHANNELS).expect("CV_CHANNELS fits a u8") {
            assert_eq!(oc_engine_cv_out(engine, channel), 0);
        }
        assert_eq!(oc_engine_cv_out(engine, u8::MAX), 0);

        oc_engine_free(engine);
    }
}

/// A tick against a never-configured engine must produce a defined, inert
/// result rather than reading uninitialised state or panicking.
#[test]
fn ticking_a_freshly_created_engine_is_harmless() {
    let engine = oc_engine_new();
    assert!(!engine.is_null());

    // SAFETY: `engine` is live for the duration of this test.
    unsafe {
        oc_engine_tick(engine, 0);
        for channel in 0..u8::try_from(CV_CHANNELS).expect("CV_CHANNELS fits a u8") {
            assert_eq!(oc_engine_cv_out(engine, channel), 0);
        }

        let frame = oc_engine_framebuffer(engine);
        assert!(!frame.is_null());
        // Reading every advertised byte must not fault, even though nothing
        // was configured before the tick above.
        let bytes = std::slice::from_raw_parts(frame, oc_engine_framebuffer_len());
        assert_eq!(bytes.len(), FRAMEBUFFER_LEN);

        oc_engine_free(engine);
    }
}

/// Two engines created back to back must not share state: an ABI bug that
/// aliased a global instead of allocating per-call would only show up with
/// more than one live engine.
#[test]
fn two_engines_are_independent() {
    let first = oc_engine_new();
    let second = oc_engine_new();
    assert!(!first.is_null());
    assert!(!second.is_null());
    assert_ne!(first, second, "two allocations must not coincide");

    // Channel 1, not 0: in the applet's default `Offset` mode the selected
    // channel (0 by default) emits the dialled offset rather than mirroring
    // its input, so only the non-selected channels pass their CV straight
    // through (see `OutputMode::Offset` in `oc_core::app`).
    skip_boot(first);
    skip_boot(second);

    // SAFETY: both pointers are live for the duration of this test.
    unsafe {
        oc_engine_set_cv_in(first, 1, 4_000);
        oc_engine_tick(first, u64::from(DURATION_MICROS) * 2);
        oc_engine_tick(second, u64::from(DURATION_MICROS) * 2);

        // The level crosses the modelled 12-bit ADC on the way.
        assert_eq!(oc_engine_cv_out(first, 1), adc_reading(4_000));
        assert_eq!(oc_engine_cv_out(second, 1), 0);

        oc_engine_free(first);
        oc_engine_free(second);
    }
}

/// The channel-count getters exist so a plugin never hard-codes the panel
/// layout; they must agree with `oc-core`'s own constants.
#[test]
fn channel_count_getters_match_oc_core() {
    assert_eq!(usize::from(oc_engine_cv_channels()), CV_CHANNELS);
    assert_eq!(usize::from(oc_engine_trigger_channels()), TRIGGER_CHANNELS);
    assert_eq!(usize::from(oc_engine_encoders()), ENCODERS);
    assert_eq!(usize::from(oc_engine_buttons()), BUTTONS);
    assert_eq!(oc_engine_framebuffer_len(), FRAMEBUFFER_LEN);
}

/// What a patch saves, a fresh instance must read back; and a damaged patch
/// must be refused rather than half-applied.
#[test]
fn settings_round_trip_through_the_patch_bytes() {
    let source = oc_engine_new();
    let target = oc_engine_new();
    let mut buffer = vec![0u8; oc_engine_settings_max_len()];

    // SAFETY: both engines are live and `buffer` is as long as stated.
    unsafe {
        let len = oc_engine_settings_save(source, buffer.as_mut_ptr(), buffer.len());
        assert!(len > 1, "the defaults still encode to something");
        assert!(oc_engine_settings_load(target, buffer.as_ptr(), len));

        assert_eq!(
            oc_engine_settings_save(source, buffer.as_mut_ptr(), 1),
            0,
            "a buffer that is too short is refused"
        );
        assert!(!oc_engine_settings_load(target, buffer.as_ptr(), 1));
        assert!(!oc_engine_settings_load(target, std::ptr::null(), 8));
        assert!(!oc_engine_settings_load(
            std::ptr::null_mut(),
            buffer.as_ptr(),
            len
        ));
        assert_eq!(
            oc_engine_settings_save(std::ptr::null(), buffer.as_mut_ptr(), buffer.len()),
            0
        );

        oc_engine_free(source);
        oc_engine_free(target);
    }
}

/// A held level reads back from the audio call as the tick wrote it, at any
/// rate, a zero one included.
#[test]
fn the_audio_call_keeps_held_levels() {
    let engine = oc_engine_new();
    skip_boot(engine);
    // SAFETY: `engine` is live until freed below.
    unsafe {
        oc_engine_set_cv_in(engine, 0, 2_000);
        oc_engine_tick(engine, 1_000_000);
        let ticked: Vec<i32> = (0..4)
            .map(|channel| oc_engine_cv_out(engine, channel))
            .collect();
        for rate in [44_100, 96_000, 0] {
            oc_engine_audio(engine, rate);
            let sampled: Vec<i32> = (0..4)
                .map(|channel| oc_engine_cv_out(engine, channel))
                .collect();
            assert_eq!(sampled, ticked, "at {rate} Hz");
        }
        oc_engine_free(engine);
    }
}

/// `TONE` plays at the rate the audio call gives, between two ticks.
#[test]
fn the_audio_call_plays_a_tone_between_ticks() {
    let mut settings = oc_core::settings::Settings::DEFAULT;
    settings.startup_app = oc_core::apps::AppId::Tone;
    let mut bytes = vec![0u8; oc_engine_settings_max_len()];
    let len = oc_core::settings::encode(&settings, &mut bytes).expect("the buffer is long enough");

    let engine = oc_engine_new();
    // SAFETY: `engine` is live until freed below; `bytes` holds `len` bytes.
    unsafe {
        assert!(oc_engine_settings_load(engine, bytes.as_ptr(), len));
        skip_boot(engine);
        // A 440 Hz sine at 48 kHz: the first samples rise from zero.
        let samples: Vec<i32> = (0..4)
            .map(|_| {
                oc_engine_audio(engine, 48_000);
                oc_engine_cv_out(engine, 0)
            })
            .collect();
        assert!(
            samples.windows(2).all(|pair| pair[0] < pair[1]),
            "{samples:?}"
        );
        oc_engine_free(engine);
    }
}
