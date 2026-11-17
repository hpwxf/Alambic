//! The engine's own profile (journal/D-029), on the virtual clock: its fine
//! counter is the virtual time, read without advancing it.

use oc_core::profile::Stage;
use oc_core::testing::{mock_engine, mock_engine_at_boot};

#[test]
fn every_tick_times_its_stages_and_the_period() {
    let mut engine = mock_engine(0);
    engine.set_render_interval(4);
    engine.profile_mut().set_expected_period_micros(1_000);
    for _ in 0..8 {
        engine.tick();
        engine.clock().advance(1_000);
    }

    let profile = engine.profile();
    for stage in [Stage::Tick, Stage::Acquire, Stage::Update, Stage::Outputs] {
        assert_eq!(profile.stage(stage).count(), 8, "{}", stage.name());
    }
    for stage in [Stage::Render, Stage::Present] {
        assert_eq!(profile.stage(stage).count(), 2, "{}", stage.name());
    }
    for stage in [Stage::Page, Stage::Sample, Stage::Usb] {
        assert_eq!(
            profile.stage(stage).count(),
            0,
            "{} is the backend's",
            stage.name()
        );
    }
    assert_eq!(profile.period().count(), 7);
    assert_eq!(profile.period().min(), Some(1_000));
    assert_eq!(profile.jitter()[0], 7, "every period on time");
}

#[test]
fn a_tick_lasts_as_long_as_its_clock_reads_cost() {
    // Two microsecond readings per tick (start, end), five microseconds each.
    let mut engine = mock_engine(5);
    engine.tick();
    assert_eq!(engine.profile().stage(Stage::Tick).max(), Some(10));
}

#[test]
fn a_late_tick_shows_in_the_jitter() {
    let mut engine = mock_engine(0);
    engine.profile_mut().set_expected_period_micros(1_000);
    for period in [1_000, 1_000, 1_300] {
        engine.tick();
        engine.clock().advance(period);
    }
    engine.tick();
    let profile = engine.profile();
    assert_eq!(profile.period().max(), Some(1_300));
    assert_eq!(profile.jitter()[9], 1, "300 us late: [256, 512)");
}

#[test]
fn the_profile_starts_when_the_splash_ends() {
    let mut engine = mock_engine_at_boot(0);
    while engine.profile().stage(Stage::Tick).count() < 3 {
        engine.tick();
        engine.clock().advance(10_000);
    }
    for _ in 0..200 {
        engine.tick();
        engine.clock().advance(10_000);
    }
    // 1.5 s of splash at 10 ms per tick is 150 ticks, then the profile is
    // cleared: what is left are the ticks after it.
    let ticks = engine.profile().stage(Stage::Tick).count();
    assert!(
        ticks < 60,
        "{ticks} ticks profiled: the splash was not dropped"
    );
}

#[test]
fn the_first_period_after_the_splash_is_not_a_splash_tick() {
    // The splash sends whole frames: its ticks run long. Once it ends and the
    // profile is cleared, no period may still span one of them.
    let mut engine = mock_engine_at_boot(0);
    engine.profile_mut().set_expected_period_micros(1_000);
    let mut splash_ticks = 0;
    loop {
        engine.tick();
        engine.clock().advance(2_000);
        splash_ticks += 1;
        if splash_ticks > 1 && engine.profile().stage(Stage::Tick).count() == 0 {
            break;
        }
    }
    for _ in 0..10 {
        engine.tick();
        engine.clock().advance(1_000);
    }
    let period = engine.profile().period();
    assert_eq!(
        period.max(),
        Some(1_000),
        "a splash tick leaked into the profile"
    );
    assert_eq!(period.count(), 9);
}
