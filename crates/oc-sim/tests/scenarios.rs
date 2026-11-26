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

use oc_core::apps::AppId;
use oc_core::apps::diagnostic::DiagnosticApp;
use oc_core::apps::diagnostic::OutputMode;
use oc_core::apps::gfx_demo::GfxDemoApp;
use oc_core::apps::melody::MelodyApp;
use oc_core::apps::ratchet::RatchetApp;
use oc_core::apps::supersaw::SupersawApp;
use oc_core::apps::tone::ToneApp;
use oc_core::calibration::CV_OUT_MIN_MV;
use oc_core::platform::{CvChannel, TriggerChannel};
use oc_core::testing::adc_reading;
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
    // Snapshots are braille whatever the `octant` feature says, so the suite
    // passes under `cargo test --all-features` too (journal/D-013).
    let rendered = format!(
        "{}\n",
        braille::render_with(simulator.frame(), braille::Glyphs::Braille).join("\n")
    );

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
fn a_steady_input_is_not_mistaken_for_a_signal() {
    let simulator = replay("cv_passthrough");
    for channel in CvChannel::ALL {
        assert!(
            !simulator.app::<DiagnosticApp>().is_signal_active(channel),
            "a constant level must not read as an active signal on {channel:?}"
        );
    }
}

#[test]
fn gates_and_one_millisecond_triggers_are_all_counted() {
    let mut simulator = replay("trigger_burst");

    assert_eq!(
        simulator
            .app::<DiagnosticApp>()
            .trigger_count(TriggerChannel::One),
        4,
        "four clean gates, four edges"
    );
    assert_eq!(
        simulator
            .app::<DiagnosticApp>()
            .trigger_count(TriggerChannel::Two),
        3,
        "three single-tick triggers, three edges"
    );
    assert_eq!(
        simulator
            .app::<DiagnosticApp>()
            .trigger_count(TriggerChannel::Three),
        0
    );
    assert!(
        simulator
            .app::<DiagnosticApp>()
            .trigger_state(TriggerChannel::Two)
    );

    assert_screen_matches(&mut simulator, "trigger_burst");
}

#[test]
fn the_encoders_select_a_channel_and_dial_an_offset() {
    let mut simulator = replay("encoder_offset");

    assert_eq!(
        simulator.app::<DiagnosticApp>().selected(),
        2,
        "left encoder moved two steps"
    );
    assert_eq!(
        simulator.app::<DiagnosticApp>().offset(),
        1_500,
        "fifteen detents of 100 mV each"
    );

    let levels = simulator.cv_out();
    assert_eq!(levels[2], 1_500, "the selected channel carries the offset");
    assert_eq!(levels[0], 0, "the others mirror their unpatched inputs");

    assert_screen_matches(&mut simulator, "encoder_offset");
}

#[test]
fn a_left_tap_walks_through_the_output_modes() {
    let mut simulator = replay("mode_cycle");

    assert_eq!(
        simulator.app::<DiagnosticApp>().mode(),
        OutputMode::Zero,
        "two taps from OFFS land on ZERO"
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
    // SCOPE took DIAGNOSTIC's place (journal/D-046): that up and down fire
    // nothing on the way into the menu is `tests/menu.rs`'s to check.
    assert_eq!(simulator.current_app(), AppId::Scope);
    assert_eq!(
        simulator.cv_out(),
        [adc_reading(1_500); 4],
        "the scope buffers CV1 to every output, which no output mode can do"
    );

    assert_screen_matches(&mut simulator, "app_menu");
}

#[test]
fn the_app_menu_lists_the_apps_right_of_the_scroll_track() {
    let mut simulator = replay("app_menu_open");

    assert!(simulator.menu_is_open(), "nothing was launched");
    assert_screen_matches(&mut simulator, "app_menu_open");
}

#[test]
fn melody_plays_its_sixth_step_of_sixteen() {
    let mut simulator = replay("melody");
    assert_eq!(simulator.current_app(), AppId::Melody);
    let melody = simulator.app::<MelodyApp>();
    assert_eq!(melody.sequence().length(), 16);
    assert_eq!(melody.sequence().position(), Some(5));
    assert_screen_matches(&mut simulator, "melody");
}

#[test]
fn melody_edits_a_step_on_the_screen() {
    let mut simulator = replay("melody_edit");
    let melody = simulator.app::<MelodyApp>();
    assert_eq!(melody.editing(), Some(4), "the fifth step");
    assert_eq!(melody.sequence().position(), Some(2), "the clock's own");
    assert_screen_matches(&mut simulator, "melody_edit");
}

#[test]
fn ratchet_bursts_a_trigger_onto_b() {
    let mut simulator = replay("ratchet");
    assert_eq!(simulator.current_app(), AppId::Ratchet);
    // 200 ms after the trigger: the initial trigger and two repeats sent
    // (100 ms, then about 84 ms), the burst still running.
    assert_eq!(simulator.app::<RatchetApp>().repeats_sent(), Some(2));
    assert_screen_matches(&mut simulator, "ratchet");
}

#[test]
fn ratchet_marks_where_the_next_trigger_is_due() {
    let mut simulator = replay("ratchet_tempo");
    assert_eq!(simulator.app::<RatchetApp>().bpm(), Some(242));
    assert_screen_matches(&mut simulator, "ratchet_tempo");
}

#[test]
fn ratchet_shows_the_input_submenu() {
    use oc_core::apps::ratchet::Setting;
    use oc_core::modulation::AssignLine;
    use oc_core::platform::CvChannel;
    let mut simulator = replay("ratchet_inputs");
    let ratchet = simulator.app::<RatchetApp>();
    assert_eq!(ratchet.assigning(), Some(AssignLine::Scale));
    let assign = ratchet.assign(Setting::Count).unwrap();
    assert_eq!(
        (assign.input, assign.scale_percent),
        (Some(CvChannel::Two), 50)
    );
    assert_screen_matches(&mut simulator, "ratchet_inputs");
}

#[test]
fn a_long_up_opens_the_routing_page_where_sub_goes_off() {
    let mut simulator = replay("routing");
    assert!(simulator.routing_is_open());
    assert_eq!(simulator.current_app(), AppId::Supersaw);
    assert!(
        simulator
            .app::<SupersawApp>()
            .params()
            .as_supersaw()
            .unwrap()
            .parts[2]
            .is_none(),
        "SUB routed nowhere: not prepared"
    );
    assert_screen_matches(&mut simulator, "routing");
}

#[test]
fn supersaw_plays_its_settings_and_cv2_once_assigned() {
    let mut simulator = replay("supersaw");
    assert_eq!(simulator.current_app(), AppId::Supersaw);
    let supersaw = simulator.app::<SupersawApp>();
    assert_eq!(supersaw.note(), 57, "A3");
    assert_eq!(supersaw.frequency(), 220_000);
    assert_eq!(supersaw.detune(), 600);
    assert_eq!(supersaw.played_detune(), 800, "CV2 adds 20 % a volt");
    assert_screen_matches(&mut simulator, "supersaw");
}

#[test]
fn supersaw_shows_the_input_submenu() {
    use oc_core::apps::supersaw::Setting;
    use oc_core::modulation::AssignLine;
    use oc_core::platform::CvChannel;
    let mut simulator = replay("supersaw_inputs");
    let supersaw = simulator.app::<SupersawApp>();
    assert_eq!(supersaw.assigning(), Some(AssignLine::Input));
    let assign = supersaw.assign(Setting::Detune).unwrap();
    assert_eq!(assign.input, Some(CvChannel::Two));
    assert_eq!(supersaw.played_detune(), 800);
    assert_screen_matches(&mut simulator, "supersaw_inputs");
}

#[test]
fn supersaw_shows_level_and_stereo_on_its_second_page() {
    use oc_core::apps::supersaw::Setting;
    let mut simulator = replay("supersaw_page2");
    let supersaw = simulator.app::<SupersawApp>();
    assert_eq!((supersaw.selected(), supersaw.page()), (Setting::Stereo, 1));
    assert!(supersaw.stereo());
    assert_screen_matches(&mut simulator, "supersaw_page2");
}

#[test]
fn tone_plays_its_note_raised_by_the_cv1_it_was_given() {
    use oc_core::apps::tone::Setting;
    use oc_core::platform::CvChannel;
    let simulator = replay("tone");
    assert_eq!(simulator.current_app(), AppId::Tone);
    let tone = simulator.app::<ToneApp>();
    assert_eq!(tone.note(), 72, "C5");
    assert_eq!(tone.amplitude(), 1_500);
    assert_eq!(tone.selected(), Setting::Note);
    assert_eq!(tone.assigning(), None);
    assert_eq!(tone.assign(Setting::Note).input, Some(CvChannel::One));
    // Half an octave above C5 (523.25 Hz): 739.99 Hz, CV1 as the module's
    // converter reads 500 mV.
    assert!(
        tone.frequency().abs_diff(739_989) < 1_000,
        "{}",
        tone.frequency()
    );
    let mut simulator = simulator;
    assert_screen_matches(&mut simulator, "tone");
}

#[test]
fn tone_shows_the_input_submenu_of_its_level() {
    use oc_core::apps::tone::Setting;
    use oc_core::modulation::AssignLine;
    use oc_core::platform::CvChannel;
    let mut simulator = replay("tone_inputs");
    let tone = simulator.app::<ToneApp>();
    assert_eq!(tone.selected(), Setting::Level);
    assert_eq!(tone.assigning(), Some(AssignLine::Input));
    assert_eq!(tone.assign(Setting::Level).input, Some(CvChannel::Two));
    assert!(
        tone.played_amplitude().abs_diff(1_600) <= 5,
        "{}",
        tone.played_amplitude()
    );
    assert_screen_matches(&mut simulator, "tone_inputs");
}

#[test]
fn the_graphics_demo_shows_each_of_its_pages() {
    use oc_core::apps::gfx_demo::Page;
    for (name, page) in [
        ("gfx_fonts", Page::Fonts),
        ("gfx_knobs", Page::Knobs),
        ("gfx_curves", Page::Curves),
        ("gfx_sequencer", Page::Sequencer),
        ("gfx_meters", Page::Meters),
    ] {
        let mut simulator = replay(name);
        assert_eq!(simulator.current_app(), AppId::Gfx, "{name}");
        assert_eq!(simulator.app::<GfxDemoApp>().page(), page, "{name}");
        assert_screen_matches(&mut simulator, name);
    }
}

#[test]
fn every_scenario_replays_identically_twice() {
    for name in [
        "cv_passthrough",
        "trigger_burst",
        "encoder_offset",
        "mode_cycle",
        "app_menu",
        "app_menu_open",
        "gfx_curves",
        "gfx_meters",
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
fn the_setup_screen_previews_a_rotation_before_any_save() {
    let mut simulator = replay("setup_menu");
    assert!(!simulator.menu_is_open());
    assert!(
        simulator.system_is_open(),
        "SETUP opened the settings screen"
    );
    assert!(
        simulator.settings().display.rotated,
        "the draft is in force"
    );
    assert_eq!(
        simulator.cv_out(),
        [0; 4],
        "the applet is paused, outputs at rest"
    );
    assert_screen_matches(&mut simulator, "setup_menu");
}

#[test]
fn the_wizard_drives_every_output_to_the_point_being_trimmed() {
    let mut simulator = replay("calibration_wizard");
    assert!(simulator.calibration_is_open());
    let levels = simulator.cv_out();
    assert_eq!(levels[1..], [-3_000; 3], "every output at the -3 V point");
    // Three detents up on A's code: the simulated jack, a factory converter,
    // now reads a few millivolts higher, as a meter would.
    assert!(levels[0] > -3_000 && levels[0] < -2_990, "{levels:?}");
    assert_screen_matches(&mut simulator, "calibration_wizard");
}

#[test]
fn every_scenario_file_has_a_test_and_a_snapshot() {
    let known = [
        "cv_passthrough",
        "trigger_burst",
        "encoder_offset",
        "mode_cycle",
        "app_menu",
        "app_menu_open",
        "setup_menu",
        "calibration_wizard",
        "gfx_fonts",
        "gfx_knobs",
        "gfx_curves",
        "gfx_sequencer",
        "gfx_meters",
        "ratchet",
        "ratchet_inputs",
        "ratchet_tempo",
        "tone",
        "tone_inputs",
        "supersaw",
        "supersaw_inputs",
        "supersaw_page2",
        "routing",
        "melody",
        "melody_edit",
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
