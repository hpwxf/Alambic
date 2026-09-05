//! Performance guard rails for the shared core.
//!
//! The firmware runs [`Engine::tick`](oc_core::Engine::tick) at 1 kHz on a
//! 600 MHz Cortex-M7, which is a budget of one millisecond. These benchmarks
//! measure the host, so the absolute numbers are not the target; what matters
//! is catching a regression that makes the tick or the screen rendering
//! dramatically more expensive.

use criterion::{Criterion, criterion_group, criterion_main};
use std::hint::black_box;

use oc_core::apps::AppId;
use oc_core::apps::diagnostic::DiagnosticApp;
use oc_core::apps::{InputSnapshot, TickContext};
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
        patched: [true, true, false, true],
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
        };
        bencher.iter(|| {
            app.render(&mut frame, &context);
            black_box(frame.lit_pixels())
        });
    });

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
    let input = CvInputCalibration::NOMINAL;
    let output = CvOutputCalibration::NOMINAL;

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

criterion_group!(benches, engine_tick, applet, menu, conversions);
criterion_main!(benches);
