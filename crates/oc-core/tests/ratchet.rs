//! The Ratchet applet on the mock engine (journal/D-032): what reaches the
//! outputs, tick by tick, for the edges `TR1` receives.

use oc_core::apps::AppId;
use oc_core::apps::ratchet::RatchetApp;
use oc_core::apps::ratchet::burst::{Count, Model};
use oc_core::apps::ratchet::{Setting, TRIGGER_MV, sweep};
use oc_core::modulation::{AssignLine, Effect};
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
        engine.tick();
        let outputs = engine.outputs();
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
    while engine.app::<RatchetApp>().selected() != setting {
        engine.parts_mut().3.turn(0, 1);
        engine.clock().advance(1_000);
        engine.tick();
    }
    engine.parts_mut().3.turn(1, turns);
    engine.clock().advance(1_000);
    engine.tick();
}

/// Presses and releases `button`.
fn press(engine: &mut MockEngine, button: Button) {
    for down in [true, true, true, true, false, false, false, false] {
        engine.parts_mut().3.hold(button, down);
        engine.clock().advance(1_000);
        engine.tick();
    }
}

/// Clicks the right encoder: the switch opens and closes the input submenu.
fn click(engine: &mut MockEngine) {
    press(engine, Button::RightEncoder);
}

/// Turns encoder `index` by `detents`, one tick.
fn turn(engine: &mut MockEngine, index: usize, detents: i8) {
    engine.parts_mut().3.turn(index, detents);
    engine.clock().advance(1_000);
    engine.tick();
}

/// In the open submenu, picks `line` and turns it by `detents`.
fn turn_line(engine: &mut MockEngine, line: AssignLine, detents: i8) {
    while engine.app::<RatchetApp>().assigning() != Some(line) {
        turn(engine, 0, 1);
    }
    turn(engine, 1, detents);
}

/// Assigns input `cv` (1 to 4) to `setting` with `effect`, the signal
/// otherwise as it comes, through the submenu.
fn assign(engine: &mut MockEngine, setting: Setting, cv: i8, effect: Effect) {
    set(engine, setting, 0);
    click(engine);
    assert_eq!(
        engine.app::<RatchetApp>().assigning(),
        Some(AssignLine::Input)
    );
    turn_line(engine, AssignLine::Input, cv);
    // From ADD, the default: SET one detent down, MUL one up.
    let effect_turns = match effect {
        Effect::Set => -1,
        Effect::Add => 0,
        Effect::Mul => 1,
    };
    turn_line(engine, AssignLine::Effect, effect_turns);
    click(engine);
    assert_eq!(engine.app::<RatchetApp>().assigning(), None);
}

/// Makes the intervals constant: a curve of 0, the default.
fn constant(engine: &mut MockEngine) {
    assert_eq!(engine.app::<RatchetApp>().settings().curve_tenths, 0);
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
    assert_eq!(engine.app::<RatchetApp>().settings().curve_tenths, -10);
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
        engine.tick();
        if engine.outputs()[0] == TRIGGER_MV {
            high += 1;
        }
    }
    assert_eq!(high, 5);
}

#[test]
fn b_can_leave_out_the_incoming_trigger() {
    let mut engine = ratchet();
    set(&mut engine, Setting::Passthru, 1);
    assert!(!engine.app::<RatchetApp>().passthru());
    let (_, b) = run(&mut engine, 1_000, pulse(10));
    assert_eq!(b.first(), Some(&110));
    assert_eq!(b.len(), 4);
}

#[test]
fn a_gate_burst_runs_while_the_gate_is_up() {
    let mut engine = ratchet();
    set(&mut engine, Setting::Count, 40);
    assert_eq!(engine.app::<RatchetApp>().settings().count, Count::Gate);
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
    // COUNT takes CV1 added, FIRST follows CV2.
    assign(&mut engine, Setting::Count, 1, Effect::Add);
    assign(&mut engine, Setting::Time, 2, Effect::Set);
    let count = *engine.app::<RatchetApp>().assign(Setting::Count).unwrap();
    assert_eq!(
        (count.input, count.effect),
        (Some(CvChannel::One), Effect::Add)
    );
    let time = *engine.app::<RatchetApp>().assign(Setting::Time).unwrap();
    assert_eq!(
        (time.input, time.effect),
        (Some(CvChannel::Two), Effect::Set)
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
    assert_eq!(engine.app::<RatchetApp>().assigning(), None);
    assert_eq!(engine.app::<RatchetApp>().assign(Setting::Passthru), None);
}

#[test]
fn an_input_can_multiply_a_setting() {
    let mut engine = ratchet();
    constant(&mut engine);
    assign(&mut engine, Setting::Count, 1, Effect::Mul);
    // 2.5 V halves the four repeats.
    engine.parts_mut().0.patch(CvChannel::One, 2_500);
    let (_, b) = run(&mut engine, 1_000, pulse(10));
    assert_eq!(b.len(), 1 + 2, "{b:?}");
}

#[test]
fn the_signal_is_scaled_shifted_and_bounded_before_its_effect() {
    let mut engine = ratchet();
    constant(&mut engine);
    set(&mut engine, Setting::Count, 0);
    click(&mut engine);
    turn_line(&mut engine, AssignLine::Input, 1);
    // SCALE 50 %, SHIFT +0.50 V: 1 V reads 1.0 V, four repeats added.
    turn_line(&mut engine, AssignLine::Scale, -10);
    turn_line(&mut engine, AssignLine::Shift, 10);
    let shaped = *engine.app::<RatchetApp>().assign(Setting::Count).unwrap();
    assert_eq!((shaped.scale_percent, shaped.shift_mv), (50, 500));
    assert_eq!(shaped.shape(1_000), 1_000);
    assert_eq!(shaped.shape(-1_000), 0, "the gain inverts nothing");
    // MAX +0.5 V: four repeats plus two at most.
    turn_line(&mut engine, AssignLine::Max, -95);
    assert_eq!(
        engine
            .app::<RatchetApp>()
            .assign(Setting::Count)
            .unwrap()
            .max_mv,
        500
    );
    // MIN never passes MAX.
    turn_line(&mut engine, AssignLine::Min, 127);
    assert_eq!(
        engine
            .app::<RatchetApp>()
            .assign(Setting::Count)
            .unwrap()
            .min_mv,
        500
    );
    turn_line(&mut engine, AssignLine::Min, -100);
    press(&mut engine, Button::Down);
    assert_eq!(
        engine.app::<RatchetApp>().assigning(),
        None,
        "down closes it"
    );
    engine.parts_mut().0.patch(CvChannel::One, 4_000);
    let (_, b) = run(&mut engine, 1_000, pulse(10));
    assert_eq!(b.len(), 1 + 4 + 2, "{b:?}");
}

#[test]
fn the_submenu_reads_its_input_and_the_range_seen() {
    let mut engine = ratchet();
    set(&mut engine, Setting::Shape, 0);
    click(&mut engine);
    assert_eq!(engine.app::<RatchetApp>().observed(), None, "no input yet");
    turn_line(&mut engine, AssignLine::Input, 3);
    for millivolts in [1_000, -2_000, 3_000, 500] {
        engine.parts_mut().0.patch(CvChannel::Three, millivolts);
        engine.clock().advance(1_000);
        engine.tick();
    }
    let (low, high) = engine.app::<RatchetApp>().observed().unwrap();
    assert!((-2_010..=-1_990).contains(&low) && (2_990..=3_010).contains(&high));
    // A new input starts the range again.
    turn_line(&mut engine, AssignLine::Input, 1);
    let (low, high) = engine.app::<RatchetApp>().observed().unwrap();
    assert!(low > -100 && high < 100, "{low} {high}");
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
    assert_eq!(engine.app::<RatchetApp>().settings().time_micros, 2_000);
    // The shape, -3.0 to +3.0 in tenths.
    set(&mut engine, Setting::Shape, -100);
    assert_eq!(engine.app::<RatchetApp>().settings().curve_tenths, -30);
    set(&mut engine, Setting::Shape, 100);
    assert_eq!(engine.app::<RatchetApp>().settings().curve_tenths, 30);
    // The chance, 1 to 100 %.
    set(&mut engine, Setting::Chance, -120);
    assert_eq!(engine.app::<RatchetApp>().settings().chance_percent, 1);
    set(&mut engine, Setting::Chance, 120);
    assert_eq!(engine.app::<RatchetApp>().settings().chance_percent, 100);
    set(&mut engine, Setting::Count, -10);
    assert_eq!(
        engine.app::<RatchetApp>().settings().count,
        Count::Repeats(1)
    );
}

#[test]
fn the_rate_of_tr1_is_measured() {
    let mut engine = ratchet();
    assert_eq!(engine.app::<RatchetApp>().bpm(), None);
    // A trigger every 500 ms: 120 BPM.
    run(&mut engine, 2_600, |tick| tick % 500 == 10);
    assert_eq!(engine.app::<RatchetApp>().bpm(), Some(120));
    // Silent for more than two seconds: unknown again.
    run(&mut engine, 2_100, |_| false);
    assert_eq!(engine.app::<RatchetApp>().bpm(), None);
}

#[test]
fn with_sync_on_first_is_a_fraction_of_the_period() {
    let mut engine = ratchet();
    constant(&mut engine);
    set(&mut engine, Setting::Sync, 1);
    assert_eq!(engine.app::<RatchetApp>().sync(), Some((1, 4)));
    // Triggers 800 ms apart: from the second one on, a quarter is 200 ms.
    let (_, b) = run(&mut engine, 1_700, |tick| tick == 10 || tick == 810);
    let second: Vec<u32> = b.iter().copied().filter(|&tick| tick >= 810).collect();
    assert_eq!(second, [810, 1_010, 1_210, 1_410, 1_610]);
    // The first burst, with no rate yet, used FIRST's milliseconds.
    assert_eq!(b[1] - b[0], 100);
    set(&mut engine, Setting::Time, -1);
    assert_eq!(engine.app::<RatchetApp>().sync(), Some((1, 3)));
    // The fractions above a third, longest first: x2, x1.5, x4/3, x1,
    // x3/4, x2/3, then /2 and shorter.
    set(&mut engine, Setting::Time, -2);
    assert_eq!(engine.app::<RatchetApp>().sync(), Some((2, 3)));
    set(&mut engine, Setting::Time, -1);
    assert_eq!(engine.app::<RatchetApp>().sync(), Some((3, 4)));
    set(&mut engine, Setting::Time, -2);
    assert_eq!(engine.app::<RatchetApp>().sync(), Some((4, 3)));
    set(&mut engine, Setting::Time, -1);
    assert_eq!(engine.app::<RatchetApp>().sync(), Some((3, 2)));
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
        assert_eq!(engine.app::<RatchetApp>().page(), page, "{setting:?}");
    }
}

#[test]
fn mode_span_spreads_the_repeats_over_the_step() {
    let mut engine = ratchet();
    set(&mut engine, Setting::Mode, 1);
    assert_eq!(engine.app::<RatchetApp>().settings().model, Model::Span);
    assert_eq!(engine.app::<RatchetApp>().page(), 1, "MODE is on page 2");
    set(&mut engine, Setting::Sync, 1);
    // SPAN x1 of an 800 ms period: four even repeats at fifths of it.
    set(&mut engine, Setting::Time, -5);
    assert_eq!(engine.app::<RatchetApp>().sync(), Some((1, 1)));
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
    assert_eq!(engine.app::<RatchetApp>().settings().chance_percent, 50);
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

#[test]
fn the_submenu_is_a_frame_over_the_main_screen() {
    use oc_core::platform::Display as _;
    let mut engine = ratchet();
    set(&mut engine, Setting::Count, 0);
    click(&mut engine);
    engine.parts_mut().2.set(TriggerChannel::One, true);
    engine.clock().advance(1_000);
    engine.tick();
    let frame = engine.parts_mut().4.frame_mut().clone();
    // No activity on top: the title bar's lights are covered.
    assert!((0..128).all(|x| !frame.pixel(x, 0)));
    // The frame's sides and bottom, and the edge broken by the title.
    assert!(frame.pixel(0, 30) && frame.pixel(127, 30) && frame.pixel(64, 53));
    assert!(frame.pixel(64, 3) && !frame.pixel(5, 3));
    // Under it, the bottom of the timeline: its baseline.
    assert!(frame.pixel(64, 63) && !frame.pixel(64, 54));
}

#[test]
fn the_bars_show_the_signal_and_the_setting_it_moves() {
    use oc_core::platform::Display as _;
    let mut engine = ratchet();
    assign(&mut engine, Setting::Count, 1, Effect::Add);
    engine.parts_mut().0.patch(CvChannel::One, 5_000);
    set(&mut engine, Setting::Count, 0);
    click(&mut engine);
    for millivolts in [1_000, 5_000] {
        engine.parts_mut().0.patch(CvChannel::One, millivolts);
        engine.clock().advance(1_000);
        engine.tick();
    }
    let frame = engine.parts_mut().4.frame_mut().clone();
    // The signal bar, x 26 to 123 for -10 V to +10 V: 5 V near x 99, the
    // reading a stroke taller than the bar, the signal (unscaled) a band from
    // 0 V (x 74); the readings seen, 1 V to 5 V, a line along the top.
    let stroke: Vec<i32> = (26..124).filter(|&x| frame.pixel(x, 32)).collect();
    assert!(
        stroke.len() == 1 && (97..=99).contains(&stroke[0]),
        "{stroke:?}"
    );
    assert!(frame.pixel(stroke[0], 40));
    assert!(frame.pixel(76, 35) && frame.pixel(95, 37) && !frame.pixel(72, 35));
    assert!(frame.pixel(85, 33) && !frame.pixel(76, 33));
    // The setting's bar, x 26 to 123 for 1 to 33 (GATE) repeats: the
    // panel's 4 a tick at x 35, 4 + 4 x 5 V (23 or 24 as the converter
    // reads) a block near x 94, the stretch between a band.
    assert!(frame.pixel(35, 43) && frame.pixel(35, 51));
    let block: Vec<i32> = (36..123).filter(|&x| frame.pixel(x, 44)).collect();
    assert!(
        block.len() == 3 && (90..=97).contains(&block[1]),
        "{block:?}"
    );
    assert!(frame.pixel(60, 46) && !frame.pixel(60, 44));
    // Halved by SCALE, the signal ends near 2.5 V (x 86), the reading stays.
    turn_line(&mut engine, AssignLine::Scale, -10);
    let frame = engine.parts_mut().4.frame_mut().clone();
    assert!(frame.pixel(80, 35) && !frame.pixel(92, 35));
    assert!(frame.pixel(stroke[0], 32) && frame.pixel(stroke[0], 40));
}

#[test]
fn an_input_that_sets_a_setting_stands_in_for_its_value() {
    use oc_core::platform::Display as _;
    let mut engine = ratchet();
    let value_area = |frame: &oc_core::framebuffer::FrameBuffer| {
        (60..98).any(|x| (17..23).any(|y| frame.pixel(x, y)))
    };
    // COUNT's row on the main screen, highlighted: its value shows dark on
    // the lit row, so look at the next row, FIRST.
    set(&mut engine, Setting::Count, 0);
    let frame = engine.parts_mut().4.frame_mut().clone();
    assert!(value_area(&frame), "100 MS");
    assign(&mut engine, Setting::Time, 2, Effect::Set);
    set(&mut engine, Setting::Count, 0);
    let frame = engine.parts_mut().4.frame_mut().clone();
    assert!(!value_area(&frame), "no panel value beside =CV2");
    // Its input in its place.
    assert!((101..125).any(|x| (17..23).any(|y| frame.pixel(x, y))));
}

#[test]
fn each_setting_is_swept_end_to_end_by_its_own_range() {
    // Followed: COUNT 4 per volt over 1 to 32, SHAPE a volt per 1.0, PROB
    // 20 % per volt.
    assert_eq!(sweep(Setting::Count, Model::First), Some((0, 7_750)));
    assert_eq!(sweep(Setting::Shape, Model::First), Some((-3_000, 3_000)));
    assert_eq!(sweep(Setting::Chance, Model::First), Some((0, 5_000)));
    // The time is 100 ms at 0 V and halves per volt: about 4.3 V of
    // halvings to 2 s, 5.6 V to 2 ms; SPAN from 400 ms, up to 8 s.
    let (slow, fast) = sweep(Setting::Time, Model::First).unwrap();
    assert!((-4_330..=-4_310).contains(&slow) && (5_630..=5_650).contains(&fast));
    let (slow, fast) = sweep(Setting::Time, Model::Span).unwrap();
    assert!((-4_330..=-4_310).contains(&slow) && (7_630..=7_650).contains(&fast));
    // The options take no input.
    assert_eq!(sweep(Setting::Mode, Model::First), None);
}

#[test]
fn center_is_a_click_in_the_submenu() {
    let mut engine = ratchet();
    set(&mut engine, Setting::Count, 0);
    click(&mut engine);
    turn_line(&mut engine, AssignLine::Input, 1);
    // Within the converter's reach (about -3.3 V to +6.7 V).
    for millivolts in [-3_000, 4_900, 0] {
        engine.parts_mut().0.patch(CvChannel::One, millivolts);
        engine.clock().advance(1_000);
        engine.tick();
    }
    turn_line(&mut engine, AssignLine::Center, 0);
    click(&mut engine);
    let ratchet = engine.app::<RatchetApp>();
    assert_eq!(ratchet.assigning(), Some(AssignLine::Center), "still open");
    // The readings seen, once shaped, sit centred on 0 V and span about the
    // usable +-3.875 V, to the steps' rounding.
    let assign = ratchet.assign(Setting::Count).unwrap();
    let (low, high) = ratchet.observed().unwrap();
    let (low, high) = (assign.shape(low), assign.shape(high));
    assert!((low + high).abs() <= 100, "{low} {high}");
    assert!((7_400..=8_100).contains(&(high - low)), "{low} {high}");
    press(&mut engine, Button::Down);
    assert_eq!(engine.app::<RatchetApp>().assigning(), None);
}

#[test]
fn center_follows_a_signal_that_changes_on_the_same_input() {
    let mut engine = ratchet();
    set(&mut engine, Setting::Count, 0);
    click(&mut engine);
    turn_line(&mut engine, AssignLine::Input, 1);
    turn_line(&mut engine, AssignLine::Center, 0);
    // A wave between -3 V and +3 V for a second, then a square between -1 V
    // and +1 V on the same input: the first one leaves the range within
    // twice the window.
    let swing = |engine: &mut MockEngine, millivolts: i32, ticks: u32| {
        for tick in 0..ticks {
            let level = if tick % 200 < 100 {
                millivolts
            } else {
                -millivolts
            };
            engine.parts_mut().0.patch(CvChannel::One, level);
            engine.clock().advance(1_000);
            engine.tick();
        }
    };
    swing(&mut engine, 3_000, 1_000);
    click(&mut engine);
    let wide = engine
        .app::<RatchetApp>()
        .assign(Setting::Count)
        .unwrap()
        .scale_percent;
    swing(&mut engine, 1_000, 4_100);
    let (low, high) = engine.app::<RatchetApp>().observed().unwrap();
    assert!(low > -1_100 && high < 1_100, "{low} {high}");
    click(&mut engine);
    let narrow = engine
        .app::<RatchetApp>()
        .assign(Setting::Count)
        .unwrap()
        .scale_percent;
    // 6 V, then 2 V, onto 7.75 V: x1.30, then x2.00 (the most).
    assert_eq!((wide, narrow), (130, 200));
}
