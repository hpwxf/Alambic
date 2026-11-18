//! The voice against floating point, and `TONE` on the mock engine
//! (journal/D-033).

use std::f64::consts::TAU;

use oc_core::apps::AppId;
use oc_core::apps::tone::{REFERENCE_NOTE, frequency};
use oc_core::audio::wave::{ONE, Waveform, sine};
use oc_core::audio::{Channel, Params, Tone, Voice};
use oc_core::platform::CvChannel;
use oc_core::settings::Settings;
use oc_core::testing::{MockEngine, mock_engine_at_boot};

#[test]
fn the_sine_is_within_a_step_of_the_true_one() {
    let mut worst = 0.0f64;
    for step in 0..65_536u32 {
        let phase = step << 16 | step;
        let exact = (f64::from(phase) / 4_294_967_296.0 * TAU).sin() * f64::from(ONE);
        worst = worst.max((f64::from(sine(phase)) - exact).abs());
    }
    // A step of `ONE` is 30 µV at 1 V peak: far below a millivolt.
    assert!(worst <= 1.0, "worst error {worst} steps");
}

#[test]
fn the_pitch_is_within_a_tenth_of_a_cent_or_a_millihertz() {
    for note in 24..=108u8 {
        for cv in [-3_000, -1_234, 0, 777, 2_500] {
            let exact =
                440.0 * 2f64.powf((f64::from(note) - 69.0) / 12.0 + f64::from(cv) / 1_000.0);
            let got = f64::from(frequency(note, cv)) / 1_000.0;
            if !(1.0..=20_000.0).contains(&exact) {
                continue;
            }
            let cents = 1_200.0 * (got / exact).log2();
            // Below 20 Hz, a millihertz is the last place, not the table.
            let within = cents.abs() < 0.1 || (got - exact).abs() <= 0.001;
            assert!(within, "note {note} cv {cv}: {cents} cents");
        }
    }
}

#[test]
fn a_voice_plays_its_frequency_over_a_second() {
    // Counts the rising zero crossings of a 440 Hz sine at 48 kHz.
    let mut voice = Voice::new(48_000);
    let mut params = Params::REST;
    params.channels[0] = Channel::Tone(Tone {
        wave: Waveform::Sine,
        frequency: 440_000,
        amplitude: 1_000,
    });
    voice.set_params(&params);
    let mut crossings = 0;
    let mut previous = -1;
    for _ in 0..48_000 {
        let sample = voice.next_sample()[0];
        if previous < 0 && sample >= 0 {
            crossings += 1;
        }
        previous = sample;
    }
    assert!((439..=441).contains(&crossings), "{crossings}");
}

/// An engine running `TONE`, past the splash.
fn tone() -> MockEngine {
    let mut engine = mock_engine_at_boot(0);
    let mut settings = Settings::DEFAULT;
    settings.startup_app = AppId::Tone;
    engine.apply_settings(settings);
    engine.skip_splash();
    engine
}

#[test]
fn the_engine_publishes_the_tones_and_samples_them_once_a_tick() {
    let mut engine = tone();
    engine.clock().advance(1_000);
    let first = engine.tick().cv_out;
    assert_eq!(
        first[..3],
        [0, 0, 0],
        "sine, triangle and saw start at zero"
    );
    assert_eq!(first[3], 1_000, "the square starts high");
    let Channel::Tone(played) = engine.params().channels[0] else {
        panic!("A carries a tone");
    };
    assert_eq!(played.frequency, frequency(REFERENCE_NOTE, 0));
    engine.clock().advance(1_000);
    assert_ne!(engine.tick().cv_out[0], 0, "the sine moves on");
}

#[test]
fn a_probe_holds_an_output_over_the_tone() {
    let mut engine = tone();
    engine.force_output(CvChannel::Two, Some(1_234));
    engine.clock().advance(1_000);
    let report = engine.tick();
    assert_eq!(report.cv_out[1], 1_234);
    assert_eq!(engine.params().channels[1], Channel::Level(1_234));
    assert!(matches!(engine.params().channels[0], Channel::Tone(_)));
}

#[test]
fn every_cv_applet_publishes_held_levels() {
    let mut engine = tone();
    engine.clock().advance(1_000);
    engine.tick();
    for app in [AppId::Diagnostic, AppId::Scope, AppId::Gfx, AppId::Ratchet] {
        let mut settings = *engine.settings();
        settings.startup_app = app;
        engine.apply_settings(settings);
        engine.clock().advance(1_000);
        let report = engine.tick();
        assert_eq!(
            *engine.params(),
            Params::levels(report.cv_out),
            "{app:?} holds what it writes"
        );
    }
}
