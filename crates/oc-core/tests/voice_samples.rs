//! What the voice plays for each applet, sample for sample (journal/D-044):
//! a checksum of the first samples of every sound the applets make, through
//! parameter changes, so that a change of how the voice is built cannot
//! change one bit of what it renders. The checksums were taken before the
//! generators of D-044, on the voice of D-033 and D-040; `SUPERSAW`'s is its
//! `f32` bank's, retaken when its phases became integers (journal/D-039,
//! F2), on purpose, and kept when the integer bank went.
//!
//! Since D-046 the samples go the whole way a backend plays them: the
//! applet's parameters in the first slot of a patch, along the route of its
//! ports, through the voices of every slot.
//!
//! Switching between two kinds of sound (a tone, then the supersaw) is left
//! out on purpose: D-044 starts a generator afresh when the kind changes,
//! where the voice of D-033 kept the bank's phases across.

use oc_core::apps::diagnostic::DiagnosticApp;
use oc_core::apps::param::ParamValue;
use oc_core::apps::ratchet::RatchetApp;
use oc_core::apps::sound::{Params, Patch, Player};
use oc_core::apps::supersaw::SupersawApp;
use oc_core::apps::tone::ToneApp;
use oc_core::apps::{App, InputSnapshot};
use oc_core::audio::Route;

/// The module's audio rate.
const RATE_HZ: u32 = 47_619;

/// One tick of nothing on the panel.
fn tick() -> InputSnapshot {
    InputSnapshot {
        elapsed_micros: 1_000,
        ..InputSnapshot::default()
    }
}

/// FNV-1a over every output of `samples` samples after each parameter
/// change of `scenes`, played along `route`, the voices kept from one to the
/// next.
fn checksum(scenes: &[Params], route: Route, samples: usize) -> u64 {
    let mut player = Player::new(RATE_HZ);
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for (revision, scene) in (0_u32..).zip(scenes) {
        let patch = Patch::single(*scene, route);
        for _ in 0..samples {
            for output in player.play(revision, &patch) {
                for byte in output.to_le_bytes() {
                    hash ^= u64::from(byte);
                    hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
                }
            }
        }
    }
    hash
}

/// What `app` publishes after each of `changes`, a parameter set then a
/// tick each.
fn scenes(app: &mut dyn App, changes: &[(usize, ParamValue)]) -> Vec<Params> {
    let mut out = vec![app.update(&tick())];
    for &(index, value) in changes {
        app.set(index, value).unwrap();
        out.push(app.update(&tick()));
    }
    out
}

fn checksum_of(app: &mut dyn App, changes: &[(usize, ParamValue)]) -> u64 {
    let route = Route::first(app.output_ports().len());
    let scenes = scenes(app, changes);
    checksum(&scenes, route, 2_048)
}

#[test]
fn tone_plays_the_same_samples() {
    let mut tone = ToneApp::new();
    let sum = checksum_of(
        &mut tone,
        &[
            (0, ParamValue::Number(81)),
            (1, ParamValue::Number(2_500)),
            (0, ParamValue::Number(24)),
        ],
    );
    assert_eq!(sum, 0x5ce7_6b55_68b6_c530, "TONE: {sum:#018x}");
}

/// SUPERSAW's settings: mono then stereo, a square, a narrow pulse, full
/// detune at the top note.
fn supersaw_changes() -> Vec<(usize, ParamValue)> {
    vec![
        (6, ParamValue::Toggle(true)),
        (3, ParamValue::Number(250)),
        (4, ParamValue::Number(200)),
        (3, ParamValue::Number(1_000)),
        (1, ParamValue::Number(1_000)),
        (0, ParamValue::Number(108)),
        (5, ParamValue::Number(3_000)),
    ]
}

#[test]
fn supersaw_plays_the_same_samples() {
    let mut saw = SupersawApp::new();
    let sum = checksum_of(&mut saw, &supersaw_changes());
    assert_eq!(sum, 0x9739_0e1f_5598_da60, "SUPERSAW: {sum:#018x}");
}

#[test]
fn a_cv_applet_plays_the_same_levels() {
    // The diagnostic applet's ramp: a new level every tick.
    let mut diagnostic = DiagnosticApp::new();
    diagnostic.set(0, ParamValue::Choice(1)).unwrap();
    let scenes: Vec<_> = (0..64)
        .map(|_| App::update(&mut diagnostic, &tick()))
        .collect();
    let sum = checksum(&scenes, Route::first(diagnostic.output_ports().len()), 48);
    assert_eq!(sum, 0x4ac1_c6f7_3da4_2325, "DIAGNOSTIC RAMP: {sum:#018x}");

    // RATCHET's triggers: a burst by hand, tick by tick.
    let mut ratchet = RatchetApp::new();
    ratchet.act(0).unwrap();
    let scenes: Vec<_> = (0..600)
        .map(|_| App::update(&mut ratchet, &tick()))
        .collect();
    let sum = checksum(&scenes, Route::first(ratchet.output_ports().len()), 48);
    assert_eq!(sum, 0x88d4_4f77_cee2_7f25, "RATCHET: {sum:#018x}");
}
