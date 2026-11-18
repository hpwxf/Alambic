//! The Ratchet applet on the mock engine (journal/D-032): what reaches the
//! outputs, tick by tick, for the edges `TR1` receives.

use oc_core::apps::AppId;
use oc_core::apps::ratchet::burst::{Count, Model};
use oc_core::apps::ratchet::{Setting, Source, TRIGGER_MV};
use oc_core::platform::{Button, CvChannel, TriggerChannel};
use oc_core::settings::Settings;
use oc_core::testing::{MockEngine, mock_engine_at_boot};

/// An engine running Ratchet, past the splash.
fn ratchet() -> MockEngine {
    let mut engine = mock_engine_at_boot(0);
    let mut settings = Settings::DEFAULT;
    settings.startup_app = AppId::Ratchet;
    engine.apply_settings(settings);
    engine.skip_splash();
    engine
}

/// Runs `ticks` one-millisecond ticks, `TR1` set by `gate(tick)`; returns the
/// ticks at which A and B rose.
fn run(engine: &mut MockEngine, ticks: u32, gate: impl Fn(u32) -> bool) -> (Vec<u32>, Vec<u32>) {
    let (mut a_edges, mut b_edges) = (Vec::new(), Vec::new());
    let (mut a_was, mut b_was) = (false, false);
    for tick in 0..ticks {
        engine.parts_mut().2.set(TriggerChannel::One, gate(tick));
        engine.clock().advance(1_000);
        let outputs = engine.tick().cv_out;
        let (a, b) = (outputs[0] == TRIGGER_MV, outputs[1] == TRIGGER_MV);
        if a && !a_was {
            a_edges.push(tick);
        }
        if b && !b_was {
            b_edges.push(tick);
        }
        (a_was, b_was) = (a, b);
    }
    (a_edges, b_edges)
}

/// Turns the panel to `setting` and turns it by `turns` detents.
fn set(engine: &mut MockEngine, setting: Setting, turns: i8) {
    while engine.apps().ratchet().selected() != setting {
        engine.parts_mut().3.turn(0, 1);
        engine.clock().advance(1_000);
        engine.tick();
    }
    engine.parts_mut().3.turn(1, turns);
    engine.clock().advance(1_000);
    engine.tick();
}

/// Clicks the right encoder: the switch toggles value / input.
fn click(engine: &mut MockEngine) {
    for down in [true, true, true, true, false, false, false, false] {
        engine.parts_mut().3.hold(Button::RightEncoder, down);
        engine.clock().advance(1_000);
        engine.tick();
    }
}

/// Sets `setting`'s input `detents` steps along `Source::ALL`, from the panel.
fn source(engine: &mut MockEngine, setting: Setting, detents: i8) {
    set(engine, setting, 0);
    click(engine);
    assert!(engine.apps().ratchet().editing_source());
    engine.parts_mut().3.turn(1, detents);
    engine.clock().advance(1_000);
    engine.tick();
    click(engine);
}

/// Makes the intervals constant: a curve of 0, the default.
fn constant(engine: &mut MockEngine) {
    assert_eq!(engine.apps().ratchet().settings().curve_tenths, 0);
}

/// A short trigger at `at`.
fn pulse(at: u32) -> impl Fn(u32) -> bool {
    move |tick| tick == at
}

#[test]
fn a_trigger_is_copied_to_a_and_bursts_on_b() {
    let mut engine = ratchet();
    // Defaults: mode FIRST, 4 repeats, 100 ms, shape 0 (even).
    let (a, b) = run(&mut engine, 1_000, pulse(10));
    assert_eq!(a, [10]);
    assert_eq!(b, [10, 110, 210, 310, 410], "the trigger and four repeats");
    // Shape -1.0, accelerating: from 100 ms down to 50 ms, geometrically.
    set(&mut engine, Setting::Shape, -10);
    assert_eq!(engine.apps().ratchet().settings().curve_tenths, -10);
    let (_, b) = run(&mut engine, 1_000, pulse(10));
    let gaps: Vec<u32> = b.windows(2).map(|pair| pair[1] - pair[0]).collect();
    assert_eq!(gaps, [100, 80, 63, 50]);
    // Shape +1.0, slowing down: from 100 ms up to 200 ms.
    set(&mut engine, Setting::Shape, 20);
    let (_, b) = run(&mut engine, 1_000, pulse(10));
    let gaps: Vec<u32> = b.windows(2).map(|pair| pair[1] - pair[0]).collect();
    assert_eq!(gaps, [100, 126, 159, 200]);
}

#[test]
fn a_trigger_lasts_five_milliseconds() {
    let mut engine = ratchet();
    let mut high = 0;
    engine.parts_mut().2.set(TriggerChannel::One, true);
    for _ in 0..20 {
        engine.clock().advance(1_000);
        if engine.tick().cv_out[0] == TRIGGER_MV {
            high += 1;
        }
    }
    assert_eq!(high, 5);
}

#[test]
fn b_can_leave_out_the_incoming_trigger() {
    let mut engine = ratchet();
    set(&mut engine, Setting::Passthru, 1);
    assert!(!engine.apps().ratchet().passthru());
    let (_, b) = run(&mut engine, 1_000, pulse(10));
    assert_eq!(b.first(), Some(&110));
    assert_eq!(b.len(), 4);
}

#[test]
fn a_gate_burst_runs_while_the_gate_is_up() {
    let mut engine = ratchet();
    set(&mut engine, Setting::Count, 40);
    assert_eq!(engine.apps().ratchet().settings().count, Count::Gate);
    constant(&mut engine);
    let (_, b) = run(&mut engine, 1_000, |tick| (10..460).contains(&tick));
    assert_eq!(b, [10, 110, 210, 310, 410], "nothing after the gate falls");
}

#[test]
fn a_new_edge_restarts_the_burst() {
    let mut engine = ratchet();
    constant(&mut engine);
    let (a, b) = run(&mut engine, 1_000, |tick| tick == 10 || tick == 160);
    assert_eq!(a, [10, 160]);
    assert_eq!(b, [10, 110, 160, 260, 360, 460, 560]);
}

#[test]
fn an_input_adds_to_a_setting_or_sets_it() {
    let mut engine = ratchet();
    constant(&mut engine);
    // COUNT takes CV1 added (5 steps: --, CV1= .. CV4=, CV1+), FIRST
    // follows CV2 (2 steps).
    source(&mut engine, Setting::Count, 5);
    source(&mut engine, Setting::Time, 2);
    assert_eq!(
        engine.apps().ratchet().source(Setting::Count),
        Some(Source::Add(CvChannel::One))
    );
    assert_eq!(
        engine.apps().ratchet().source(Setting::Time),
        Some(Source::Follow(CvChannel::Two))
    );
    {
        let (analog_in, ..) = engine.parts_mut();
        analog_in.patch(CvChannel::One, 1_000);
        analog_in.patch(CvChannel::Two, 1_000);
    }
    let (_, b) = run(&mut engine, 2_000, pulse(10));
    assert_eq!(
        b.len(),
        1 + 8,
        "a volt on CV1 adds four repeats to four: {b:?}"
    );
    assert_eq!(
        b[1] - b[0],
        50,
        "followed, a volt on CV2 sets 100 ms halved"
    );
}

#[test]
fn an_input_only_counts_once_chosen_and_b_init_takes_none() {
    let mut engine = ratchet();
    engine.parts_mut().0.patch(CvChannel::One, 5_000);
    let (_, b) = run(&mut engine, 1_000, pulse(10));
    assert_eq!(b.len(), 5, "no input chosen: the panel's count");
    set(&mut engine, Setting::Passthru, 0);
    click(&mut engine);
    assert!(!engine.apps().ratchet().editing_source());
    assert_eq!(engine.apps().ratchet().source(Setting::Passthru), None);
}

#[test]
fn up_starts_a_burst_by_hand() {
    let mut engine = ratchet();
    engine.parts_mut().3.hold(Button::Up, true);
    for _ in 0..10 {
        engine.clock().advance(1_000);
        engine.tick();
    }
    engine.parts_mut().3.hold(Button::Up, false);
    let (a, b) = run(&mut engine, 1_000, |_| false);
    assert_eq!(a.len(), 1);
    assert_eq!(b.len(), 5);
}

#[test]
fn the_panel_walks_and_bounds_the_settings() {
    let mut engine = ratchet();
    set(&mut engine, Setting::Time, -100);
    assert_eq!(engine.apps().ratchet().settings().time_micros, 2_000);
    // The shape, -3.0 to +3.0 in tenths of an octave.
    set(&mut engine, Setting::Shape, -100);
    assert_eq!(engine.apps().ratchet().settings().curve_tenths, -30);
    set(&mut engine, Setting::Shape, 100);
    assert_eq!(engine.apps().ratchet().settings().curve_tenths, 30);
    // The chance, 1 to 100 %.
    set(&mut engine, Setting::Chance, -120);
    assert_eq!(engine.apps().ratchet().settings().chance_percent, 1);
    set(&mut engine, Setting::Chance, 120);
    assert_eq!(engine.apps().ratchet().settings().chance_percent, 100);
    set(&mut engine, Setting::Count, -10);
    assert_eq!(engine.apps().ratchet().settings().count, Count::Repeats(1));
}

#[test]
fn the_rate_of_tr1_is_measured() {
    let mut engine = ratchet();
    assert_eq!(engine.apps().ratchet().bpm(), None);
    // A trigger every 500 ms: 120 BPM.
    run(&mut engine, 2_600, |tick| tick % 500 == 10);
    assert_eq!(engine.apps().ratchet().bpm(), Some(120));
    // Silent for more than two seconds: unknown again.
    run(&mut engine, 2_100, |_| false);
    assert_eq!(engine.apps().ratchet().bpm(), None);
}

#[test]
fn with_sync_on_first_is_a_fraction_of_the_period() {
    let mut engine = ratchet();
    constant(&mut engine);
    set(&mut engine, Setting::Sync, 1);
    assert_eq!(engine.apps().ratchet().sync(), Some((1, 4)));
    // Triggers 800 ms apart: from the second one on, a quarter is 200 ms.
    let (_, b) = run(&mut engine, 1_700, |tick| tick == 10 || tick == 810);
    let second: Vec<u32> = b.iter().copied().filter(|&tick| tick >= 810).collect();
    assert_eq!(second, [810, 1_010, 1_210, 1_410, 1_610]);
    // The first burst, with no rate yet, used FIRST's milliseconds.
    assert_eq!(b[1] - b[0], 100);
    set(&mut engine, Setting::Time, -1);
    assert_eq!(engine.apps().ratchet().sync(), Some((1, 3)));
    // The fractions above a third, longest first: x2, x1.5, x4/3, x1,
    // x3/4, x2/3, then /2 and shorter.
    set(&mut engine, Setting::Time, -2);
    assert_eq!(engine.apps().ratchet().sync(), Some((2, 3)));
    set(&mut engine, Setting::Time, -1);
    assert_eq!(engine.apps().ratchet().sync(), Some((3, 4)));
    set(&mut engine, Setting::Time, -2);
    assert_eq!(engine.apps().ratchet().sync(), Some((4, 3)));
    set(&mut engine, Setting::Time, -1);
    assert_eq!(engine.apps().ratchet().sync(), Some((3, 2)));
}

#[test]
fn the_burst_settings_are_on_page_one_and_the_options_on_page_two() {
    let mut engine = ratchet();
    for (setting, page) in [
        (Setting::Count, 0),
        (Setting::Shape, 0),
        (Setting::Chance, 0),
        (Setting::Passthru, 1),
        (Setting::Sync, 1),
        (Setting::Mode, 1),
    ] {
        set(&mut engine, setting, 0);
        assert_eq!(engine.apps().ratchet().page(), page, "{setting:?}");
    }
}

#[test]
fn mode_span_spreads_the_repeats_over_the_step() {
    let mut engine = ratchet();
    set(&mut engine, Setting::Mode, 1);
    assert_eq!(engine.apps().ratchet().settings().model, Model::Span);
    assert_eq!(engine.apps().ratchet().page(), 1, "MODE is on page 2");
    set(&mut engine, Setting::Sync, 1);
    // SPAN x1 of an 800 ms period: four even repeats at fifths of it.
    set(&mut engine, Setting::Time, -5);
    assert_eq!(engine.apps().ratchet().sync(), Some((1, 1)));
    let (_, b) = run(&mut engine, 1_700, |tick| tick == 10 || tick == 810);
    let second: Vec<u32> = b.iter().copied().filter(|&tick| tick >= 810).collect();
    assert_eq!(second, [810, 970, 1_130, 1_290, 1_450]);
    // Accelerating: the repeats crowd towards the next step, within it.
    set(&mut engine, Setting::Shape, -20);
    let (_, b) = run(&mut engine, 1_700, |tick| tick == 10 || tick == 810);
    let second: Vec<u32> = b.iter().copied().filter(|&tick| tick >= 810).collect();
    let gaps: Vec<u32> = second.windows(2).map(|pair| pair[1] - pair[0]).collect();
    assert!(gaps.windows(2).all(|pair| pair[1] < pair[0]), "{gaps:?}");
    assert!(
        *second.last().unwrap() < 1_610,
        "within the step: {second:?}"
    );
}

#[test]
fn each_repeat_fires_with_its_chance() {
    let mut engine = ratchet();
    // 100 %: every repeat, every time.
    let (_, b) = run(&mut engine, 5_000, |tick| tick % 500 == 10);
    assert_eq!(b.len(), 10 * 5);
    // 50 %: about half the repeats over many bursts, the incoming trigger
    // always passed through; the same each run, the rolls being seeded.
    set(&mut engine, Setting::Chance, -50);
    assert_eq!(engine.apps().ratchet().settings().chance_percent, 50);
    let (a, b) = run(&mut engine, 20_000, |tick| tick % 500 == 10);
    assert_eq!(a.len(), 40);
    let repeats = b.len() - 40;
    assert!(
        (60..=100).contains(&repeats),
        "{repeats} of 160 repeats fired"
    );
}

#[test]
fn a_repeat_that_does_not_fire_keeps_the_others_in_place() {
    let mut engine = ratchet();
    set(&mut engine, Setting::Chance, -70);
    let (_, b) = run(&mut engine, 30_000, |tick| tick % 1_000 == 10);
    // Every edge on B is a whole number of 100 ms after its burst's start.
    for tick in b {
        assert_eq!((tick % 1_000 - 10) % 100, 0, "{tick} is off the grid");
    }
}

#[test]
fn a_scroll_bar_left_of_the_settings_shows_the_page() {
    use oc_core::platform::Display as _;
    let mut engine = ratchet();
    engine.tick();
    // Page 1 of 2: the thumb over the top half of the rows (8..40).
    let frame = engine.parts_mut().4.frame_mut().clone();
    assert!(frame.pixel(0, 8) && frame.pixel(2, 23) && !frame.pixel(0, 24) && !frame.pixel(2, 39));
    set(&mut engine, Setting::Mode, 0);
    let frame = engine.parts_mut().4.frame_mut().clone();
    assert!(!frame.pixel(0, 8) && !frame.pixel(2, 23) && frame.pixel(0, 24) && frame.pixel(2, 39));
    // Nothing left of the rows in the title bar's place of the old cursor.
    assert!((80..90).all(|x| (0..8).all(|y| !frame.pixel(x, y))));
}

#[test]
fn the_timeline_follows_the_burst_under_way() {
    use oc_core::platform::Display as _;
    let mut engine = ratchet();
    // One trigger, then 240 ms of the default burst (4 x 100 ms, scaled to
    // 400 ms over 127 columns): two repeats sent, the playhead at x 76.
    run(&mut engine, 250, |tick| tick == 10);
    let frame = engine.parts_mut().4.frame_mut().clone();
    // The incoming trigger and the repeats sent are bold, the others thin.
    for x in [0, 1, 30, 31, 32, 62, 63, 64] {
        assert!(frame.pixel(x, 55), "bold at {x}");
    }
    assert!(frame.pixel(95, 55) && !frame.pixel(94, 55) && !frame.pixel(96, 55));
    // The playhead's tip, alone on its row.
    let tips: Vec<i32> = (0..128).filter(|&x| frame.pixel(x, 46)).collect();
    assert_eq!(tips, [75]);
    // Once the burst is over, the timeline shows the next one, all thin.
    run(&mut engine, 300, |_| false);
    let frame = engine.parts_mut().4.frame_mut().clone();
    assert!(frame.pixel(31, 55) && !frame.pixel(30, 55) && !frame.pixel(32, 55));
    assert!((0..128).all(|x| !frame.pixel(x, 46)));
}
