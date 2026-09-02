//! Replayable scenario tests.
//!
//! Each `tests/scenarios/*.scn` file is a committed, hand-editable input
//! sequence. Behavioural expectations are asserted here, and the resulting
//! screen is compared against a golden `*.screen` snapshot so that an
//! unintended change in the rendering is caught.
//!
//! Regenerate the snapshots after an intentional change with:
//!
//! ```sh
//! UPDATE_SCREENS=1 cargo test -p oc-sim --test scenarios
//! ```

use std::path::{Path, PathBuf};

use oc_core::app::OutputMode;
use oc_core::apps::AppId;
use oc_core::calibration::CV_OUT_MIN_MV;
use oc_core::platform::{CvChannel, TriggerChannel};
use oc_sim::braille;
use oc_sim::scenario::Scenario;
use oc_sim::simulator::Simulator;

/// Directory holding the scenario files and their snapshots.
fn scenario_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("scenarios")
}

/// Loads and replays a scenario, returning the simulator afterwards.
fn replay(name: &str) -> Simulator {
    let path = scenario_dir().join(format!("{name}.scn"));
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("cannot read {}: {error}", path.display()));
    let scenario: Scenario = text
        .parse()
        .unwrap_or_else(|error| panic!("cannot parse {}: {error:#}", path.display()));

    let mut simulator = Simulator::new();
    // These fixtures predate the boot splash screen and exercise the
    // applet's steady-state behaviour directly, so start past it.
    simulator.skip_splash();
    simulator.replay(&scenario);
    assert_eq!(
        simulator.tick_count(),
        scenario.ticks,
        "{name} must run its declared number of ticks"
    );
    simulator
}

/// Compares the module's screen against the golden snapshot for `name`.
fn assert_screen_matches(simulator: &mut Simulator, name: &str) {
    let path = scenario_dir().join(format!("{name}.screen"));
    let rendered = format!("{}\n", braille::render(simulator.frame()).join("\n"));

    if std::env::var_os("UPDATE_SCREENS").is_some() {
        std::fs::write(&path, &rendered)
            .unwrap_or_else(|error| panic!("cannot write {}: {error}", path.display()));
        return;
    }

    let expected = std::fs::read_to_string(&path).unwrap_or_else(|error| {
        panic!(
            "cannot read {}: {error}\n\
             run `UPDATE_SCREENS=1 cargo test -p oc-sim --test scenarios` to create it",
            path.display()
        )
    });

    assert_eq!(
        rendered, expected,
        "the screen for {name} changed; if that is intentional, regenerate the \
         snapshot with UPDATE_SCREENS=1"
    );
}

#[test]
fn cv_inputs_reach_their_matching_outputs() {
    let mut simulator = replay("cv_passthrough");

    let levels = simulator.cv_out();
    // Channel one is selected, so it emits the offset, which is still 0 V.
    assert_eq!(levels[0], 0);
    assert_eq!(levels[1], -2_500);
    // The output range reaches +6 V, so +5 V passes through untouched, while
    // -5 V is below the -3 V floor and clamps.
    assert_eq!(levels[2], 5_000);
    assert_eq!(levels[3], CV_OUT_MIN_MV, "-5 V clamps to the output floor");

    assert_screen_matches(&mut simulator, "cv_passthrough");
}

#[test]
fn cable_presence_is_reported_per_channel() {
    let mut simulator = replay("cv_passthrough");
    assert!(simulator.is_patched(CvChannel::One));
    assert!(simulator.is_patched(CvChannel::Three));
    assert!(
        !simulator.is_patched(CvChannel::Four),
        "channel four is driven but not declared patched"
    );
}

#[test]
fn a_steady_input_is_not_mistaken_for_a_signal() {
    let simulator = replay("cv_passthrough");
    for channel in CvChannel::ALL {
        assert!(
            !simulator.diagnostic().is_signal_active(channel),
            "a constant level must not read as an active signal on {channel:?}"
        );
    }
}

#[test]
fn clean_gates_are_counted_and_bounce_is_not() {
    let mut simulator = replay("trigger_burst");

    assert_eq!(
        simulator.diagnostic().trigger_count(TriggerChannel::One),
        4,
        "four clean gates, four edges"
    );
    assert_eq!(
        simulator.diagnostic().trigger_count(TriggerChannel::Two),
        1,
        "a bouncing contact must count once"
    );
    assert_eq!(
        simulator.diagnostic().trigger_count(TriggerChannel::Three),
        0
    );
    assert!(simulator.diagnostic().trigger_state(TriggerChannel::Two));

    assert_screen_matches(&mut simulator, "trigger_burst");
}

#[test]
fn the_encoders_select_a_channel_and_dial_an_offset() {
    let mut simulator = replay("encoder_offset");

    assert_eq!(
        simulator.diagnostic().selected(),
        2,
        "left encoder moved two steps"
    );
    assert_eq!(
        simulator.diagnostic().offset(),
        1_500,
        "fifteen detents of 100 mV each"
    );

    let levels = simulator.cv_out();
    assert_eq!(levels[2], 1_500, "the selected channel carries the offset");
    assert_eq!(levels[0], 0, "the others mirror their unpatched inputs");

    assert_screen_matches(&mut simulator, "encoder_offset");
}

#[test]
fn the_up_button_walks_through_the_output_modes() {
    let mut simulator = replay("mode_cycle");

    assert_eq!(
        simulator.diagnostic().mode(),
        OutputMode::Zero,
        "two presses from OFFS land on ZERO"
    );
    assert_eq!(
        simulator.cv_out(),
        [0; 4],
        "ZERO mode overrides the patched input"
    );

    assert_screen_matches(&mut simulator, "mode_cycle");
}

#[test]
fn the_app_menu_switches_to_the_scope() {
    let mut simulator = replay("app_menu");

    assert!(
        !simulator.menu_is_open(),
        "launching an app closes the menu"
    );
    assert_eq!(simulator.current_app(), AppId::Scope);
    assert_eq!(
        simulator.diagnostic().mode(),
        OutputMode::Offset,
        "neither up nor down fired its own action on the way into the menu"
    );
    assert_eq!(
        simulator.cv_out(),
        [1_500; 4],
        "the scope buffers CV1 to every output, which no output mode can do"
    );

    assert_screen_matches(&mut simulator, "app_menu");
}

#[test]
fn every_scenario_replays_identically_twice() {
    for name in [
        "cv_passthrough",
        "trigger_burst",
        "encoder_offset",
        "mode_cycle",
        "app_menu",
    ] {
        let mut first = replay(name);
        let mut second = replay(name);
        assert_eq!(
            first.frame().clone(),
            second.frame().clone(),
            "{name} is not deterministic"
        );
        assert_eq!(
            first.cv_out(),
            second.cv_out(),
            "{name} is not deterministic"
        );
    }
}

#[test]
fn every_scenario_file_has_a_test_and_a_snapshot() {
    let known = [
        "cv_passthrough",
        "trigger_burst",
        "encoder_offset",
        "mode_cycle",
        "app_menu",
    ];

    let mut found: Vec<String> = std::fs::read_dir(scenario_dir())
        .expect("the scenario directory must exist")
        .filter_map(|entry| {
            let path = entry.ok()?.path();
            (path.extension()? == "scn").then(|| path.file_stem()?.to_str().map(str::to_owned))?
        })
        .collect();
    found.sort();

    let mut expected: Vec<String> = known.iter().map(|&name| name.to_owned()).collect();
    expected.sort();

    assert_eq!(
        found, expected,
        "a scenario file was added or removed without updating this test"
    );
}
