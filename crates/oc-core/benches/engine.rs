//! Performance guard rails for the shared core.
//!
//! The firmware runs [`Engine::tick`](oc_core::Engine::tick) at 1 kHz on a
//! 600 MHz Cortex-M7, which is a budget of one millisecond. These benchmarks
//! measure the host, so the absolute numbers are not the target; what matters
//! is catching a regression that makes the tick or the screen rendering
//! dramatically more expensive. Text and shape drawing, against
//! `embedded-graphics`, have their own bench: `benches/graphics.rs`.

use criterion::{Criterion, criterion_group, criterion_main};
use std::hint::black_box;

use oc_core::apps::AppId;
use oc_core::apps::diagnostic::DiagnosticApp;
use oc_core::apps::ratchet::RatchetApp;
use oc_core::apps::tone::ToneApp;
use oc_core::apps::{InputSnapshot, TickContext};
use oc_core::audio::supersaw::{Arithmetic, FULL, Part, Supersaw};
use oc_core::audio::{Channel, Params, Voice};
use oc_core::buttons::ButtonReader;
use oc_core::calibration::{CvInputCalibration, CvOutputCalibration};
use oc_core::framebuffer::FrameBuffer;
use oc_core::menu::Menu;
use oc_core::platform::{Button, ControlEvents, CvChannel, TriggerChannel};
use oc_core::testing::mock_engine;

/// A snapshot with all four inputs and all four triggers busy.
fn busy_snapshot() -> InputSnapshot {
    let controls = ControlEvents {
        encoder_delta: [1, -1],
        button_down: [false, false, true, false],
    };
    InputSnapshot {
        cv: [-2_500, 0, 1_234, 4_800],
        triggers: [true, false, true, false],
        buttons: ButtonReader::new().update(&controls),
        controls,
        elapsed_micros: 1_000,
    }
}

fn engine_tick(criterion: &mut Criterion) {
    let mut group = criterion.benchmark_group("engine");

    group.bench_function("tick_idle", |bencher| {
        let mut engine = mock_engine(0);
        bencher.iter(|| {
            engine.clock().advance(1_000);
            black_box(engine.tick())
        });
    });

    group.bench_function("tick_busy", |bencher| {
        let mut engine = mock_engine(0);
        {
            let (analog_in, _, digital_in, ..) = engine.parts_mut();
            analog_in.patch(CvChannel::One, -2_500);
            analog_in.patch(CvChannel::Two, 1_234);
            analog_in.patch(CvChannel::Four, 4_800);
            digital_in.set(TriggerChannel::One, true);
            digital_in.set(TriggerChannel::Three, true);
        }
        bencher.iter(|| {
            {
                let (_, _, _, controls, _) = engine.parts_mut();
                controls.turn(1, 1);
                controls.hold(Button::Up, true);
            }
            engine.clock().advance(1_000);
            black_box(engine.tick())
        });
    });

    group.finish();
}

fn applet(criterion: &mut Criterion) {
    let mut group = criterion.benchmark_group("applet");

    group.bench_function("update", |bencher| {
        let mut app = DiagnosticApp::new();
        let snapshot = busy_snapshot();
        bencher.iter(|| black_box(app.update(black_box(&snapshot))));
    });

    group.bench_function("render", |bencher| {
        let mut app = DiagnosticApp::new();
        app.update(&busy_snapshot());
        let mut frame = FrameBuffer::new();
        let context = TickContext {
            tick_count: 123_456,
            duration_micros: 987,
            cv_out: *app.outputs(),
            outputs_forced: false,
        };
        bencher.iter(|| {
            app.render(&mut frame, &context);
            black_box(frame.lit_pixels())
        });
    });

    group.finish();
}

/// The panel driving a Ratchet tick by tick, its buttons debounced.
struct RatchetPanel {
    app: RatchetApp,
    reader: ButtonReader,
}

impl RatchetPanel {
    /// One tick: the right encoder's switch down or up, the right encoder
    /// turned, `CV1` at `cv`.
    fn step(&mut self, switch: bool, turns: i8, cv: i32) {
        let controls = ControlEvents {
            encoder_delta: [0, turns],
            button_down: [false, switch, false, false],
        };
        let snapshot = InputSnapshot {
            cv: [cv, 0, 0, 0],
            triggers: [false; 4],
            buttons: self.reader.update(&controls),
            controls,
            elapsed_micros: 1_000,
        };
        self.app.update(&snapshot);
    }

    fn click(&mut self) {
        for down in [true, true, true, true, false, false, false, false] {
            self.step(down, 0, 0);
        }
    }
}

/// A Ratchet with `CV1` on `COUNT`, its input submenu open when `submenu`,
/// `CV1` swinging so that both bars have a range to draw.
fn ratchet_app(submenu: bool) -> RatchetApp {
    let mut panel = RatchetPanel {
        app: RatchetApp::new(),
        reader: ButtonReader::new(),
    };
    panel.click();
    panel.step(false, 1, 0);
    for cv in [-3_000, 4_500, 1_200] {
        panel.step(false, 0, cv);
    }
    if !submenu {
        panel.click();
    }
    assert_eq!(panel.app.assigning().is_some(), submenu);
    panel.app
}

fn ratchet(criterion: &mut Criterion) {
    let mut group = criterion.benchmark_group("ratchet");
    // The submenu is drawn over the main screen: its cost adds to it, to
    // compare with `applet/render`, whose module cost is measured.
    for (name, submenu) in [("render_main", false), ("render_submenu", true)] {
        group.bench_function(name, |bencher| {
            let app = ratchet_app(submenu);
            let mut frame = FrameBuffer::new();
            let context = TickContext {
                tick_count: 0,
                duration_micros: 0,
                cv_out: [0; 4],
                outputs_forced: false,
            };
            bencher.iter(|| {
                app.render(&mut frame, &context);
                black_box(frame.lit_pixels())
            });
        });
    }
    group.finish();
}

fn menu(criterion: &mut Criterion) {
    let mut group = criterion.benchmark_group("menu");

    // The menu takes the panel over from the applet, so it is drawn instead of
    // the applet's screen, never on top of it: what matters is that it stays in
    // the same order of magnitude as `applet/render`.
    group.bench_function("render", |bencher| {
        let mut menu = Menu::new();
        menu.open(AppId::Scope);
        let mut frame = FrameBuffer::new();
        bencher.iter(|| {
            menu.render(&mut frame);
            black_box(frame.lit_pixels())
        });
    });

    group.finish();
}

fn conversions(criterion: &mut Criterion) {
    let mut group = criterion.benchmark_group("conversions");
    let input = CvInputCalibration::FACTORY;
    let output = CvOutputCalibration::FACTORY;

    group.bench_function("adc_code_to_millivolts", |bencher| {
        bencher.iter(|| {
            let mut total = 0i64;
            for code in (0u16..4_096).step_by(16) {
                total += i64::from(input.to_millivolts(black_box(code)));
            }
            black_box(total)
        });
    });

    group.bench_function("millivolts_to_dac_code", |bencher| {
        bencher.iter(|| {
            let mut total = 0u32;
            for millivolts in (-3_000..6_000).step_by(37) {
                total += u32::from(output.to_code(black_box(millivolts)));
            }
            black_box(total)
        });
    });

    group.finish();
}

/// One sample of the four outputs, as the audio interrupt will compute it
/// 48 000 times a second (journal/D-033): `TONE`'s four waveforms, then each
/// sample to a DAC code.
fn voice(criterion: &mut Criterion) {
    let mut group = criterion.benchmark_group("voice");
    let mut voice = Voice::new(48_000);
    voice.set_params(&ToneApp::new().params());
    let output = CvOutputCalibration::FACTORY;

    group.bench_function("tone_sample_to_codes", |bencher| {
        bencher.iter(|| {
            let samples = voice.next_sample();
            black_box(samples.map(|level| output.to_code(level)))
        });
    });

    // The seven sawtooths, their sums and high-passes and the sub-octave
    // (journal/D-037), every part read.
    let mut bank = Voice::new(48_000);
    let mut params = Params::REST;
    params.channels = [
        Channel::Supersaw(Part::Left),
        Channel::Supersaw(Part::Right),
        Channel::Supersaw(Part::Sub),
        Channel::Supersaw(Part::Mono),
    ];
    params.supersaw = Supersaw {
        frequency: 110_000,
        detune: FULL / 2,
        mix: FULL / 2,
        shape: 0,
        amplitude: 1_000,
        arithmetic: Arithmetic::Fixed,
    };
    bank.set_params(&params);
    group.bench_function("supersaw_sample_to_codes", |bencher| {
        bencher.iter(|| {
            let samples = bank.next_sample();
            black_box(samples.map(|level| output.to_code(level)))
        });
    });

    // The same as squares: one more corrected sawtooth per voice.
    params.supersaw.shape = FULL;
    bank.set_params(&params);
    group.bench_function("supersaw_square_sample_to_codes", |bencher| {
        bencher.iter(|| {
            let samples = bank.next_sample();
            black_box(samples.map(|level| output.to_code(level)))
        });
    });
    params.supersaw.shape = 0;

    // The same in `f32` (journal/D-039).
    params.supersaw.arithmetic = Arithmetic::Float;
    bank.set_params(&params);
    group.bench_function("supersaw_f32_sample_to_codes", |bencher| {
        bencher.iter(|| {
            let samples = bank.next_sample();
            black_box(samples.map(|level| output.to_code(level)))
        });
    });

    group.finish();
}

criterion_group!(
    benches,
    engine_tick,
    applet,
    ratchet,
    menu,
    conversions,
    voice
);
criterion_main!(benches);
