//! Ornament & Crime firmware for the Teensy 4.0 (NXP i.MX RT1062).
//!
//! This crate is pure wiring. Every behaviour lives in `oc-core` and every
//! peripheral protocol in `oc-drivers`, both of which are `no_std`,
//! `forbid(unsafe_code)` and covered by host tests. What is left here is the
//! part that cannot be tested without hardware: turning board resources into
//! the platform traits `oc_core::Engine` expects, and running the loop.
//!
//! The pinout is centralised in [`board`]; read its documentation before the
//! first flash.
//!
//! # There is deliberately no serial console
//!
//! Every LPUART the Teensy 4.0 exposes on pins 0 to 23 collides with the panel:
//! LPUART2 wants pins 14 and 15, which are the right encoder's switch and its
//! `B` line; LPUART6 wants pins 0 and 1, which are `TR1` and `TR2`; LPUART4
//! wants pins 7 and 8, which are the OLED's reset and chip select. Driving any
//! of those as a transmit line is exactly the electrical hazard the pinout table
//! warns about, so the firmware drives **no** UART.
//!
//! The boot banner therefore belongs on USB, which costs no panel pin. That is
//! not wired up yet; until it is, the screen is the only diagnostic channel,
//! which is precisely what the diagnostic applet is for.
//!
//! # Unsafe code
//!
//! This crate contains **no** `unsafe` blocks of its own, which
//! `deny(unsafe_code)` enforces. Register access is confined to `teensy4-bsp`
//! and `imxrt-hal`. Should that ever change, every block must carry a
//! `# Safety` section stating the invariant it upholds.

#![no_std]
#![no_main]
#![deny(unsafe_code)]
#![warn(missing_docs)]

mod board;
mod clock;
mod cv_in;
mod delay;

use core::cell::RefCell;

use embedded_hal::digital::OutputPin as _;

use teensy4_bsp as bsp;
use teensy4_panic as _;

use bsp::board as bsp_board;

use oc_core::Engine;
use oc_core::platform::{CV_CHANNELS, Clock as _, CvChannel};
use oc_drivers::dac8565::Dac8565;
use oc_drivers::panel::{EncoderPins, Panel};
use oc_drivers::shared_bus::SharedBus;
use oc_drivers::ssd130x::Ssd130x;
use oc_drivers::triggers::Triggers;

use crate::clock::SystemClock;
use crate::cv_in::CvInputs;
use crate::delay::CycleDelay;

#[bsp::rt::entry]
fn main() -> ! {
    let bsp_board::Resources {
        pins,
        mut gpio1,
        mut gpio2,
        mut gpio4,
        lpspi4,
        adc1,
        gpt1,
        ..
    } = bsp_board::t40(bsp_board::instances());

    // ---- CV inputs ----------------------------------------------------------
    // Pins 19, 18, 20 and 17 are analog inputs and are never driven. The ADC
    // channel of each pad is resolved from its type, so the mapping cannot
    // silently drift.
    let (Ok(cv1), Ok(cv2), Ok(cv3), Ok(cv4)) = (
        adc1.input::<_, 1>(pins.p19),
        adc1.input::<_, 1>(pins.p18),
        adc1.input::<_, 1>(pins.p20),
        adc1.input::<_, 1>(pins.p17),
    ) else {
        panic!("pins 19, 18, 20 and 17 are all ADC1 pads")
    };
    let cv_inputs = CvInputs::new(adc1, [cv1, cv2, cv3, cv4], board::CV_INPUT_CALIBRATION);

    // ---- trigger inputs: pins 0 and 1 on GPIO1, 2 and 3 on GPIO4 ------------
    // `Port::input` is fallible only when a pin does not belong to the port, so
    // each `expect` documents a fact the pinout table already states.
    let triggers = Triggers::new(
        [
            gpio1.input(pins.p0).expect("P0 is a GPIO1 pin"),
            gpio1.input(pins.p1).expect("P1 is a GPIO1 pin"),
            gpio4.input(pins.p2).expect("P2 is a GPIO4 pin"),
            gpio4.input(pins.p3).expect("P3 is a GPIO4 pin"),
        ],
        board::TRIGGER_POLARITY,
    );

    // ---- shared SPI bus -----------------------------------------------------
    // The DAC and the OLED share pins 11 and 13 and are told apart by their
    // chip selects, which the drivers own; hence a shared bus rather than two
    // `SpiDevice`s. Pin 10 is used as a plain GPIO chip select, not as the
    // LPSPI hardware PCS, so that the DAC's 24-bit framing stays in software.
    let bus = RefCell::new(bsp_board::lpspi(
        lpspi4,
        bsp_board::LpspiPins {
            sdo: pins.p11,
            sdi: pins.p12,
            sck: pins.p13,
        },
        board::SPI_CLOCK_HZ,
    ));

    let dac = Dac8565::new(
        SharedBus::new(&bus),
        gpio2.output(pins.p10).expect("P10 is a GPIO2 pin"),
        board::CV_OUTPUT_CALIBRATIONS,
    );
    let mut display = Ssd130x::new(
        SharedBus::new(&bus),
        gpio2.output(pins.p8).expect("P8 is a GPIO2 pin"),
        gpio2.output(pins.p6).expect("P6 is a GPIO2 pin"),
        gpio2.output(pins.p7).expect("P7 is a GPIO2 pin"),
        board::OLED_CONTROLLER,
    );

    // The DAC's reset line is released before the first write. Leaving it
    // asserted would hold the outputs at zero scale, which on an inverting
    // output stage means +6 V on every jack.
    let mut dac_reset = gpio2.output(pins.p9).expect("P9 is a GPIO2 pin");
    let _ = dac_reset.set_high();

    // A dead panel must not stop the module: the CV path is what matters, and
    // the failure stays visible through `Ssd130x::last_error`.
    let _ = display.init(&mut CycleDelay);

    // ---- panel controls -----------------------------------------------------
    let panel = Panel::new(
        [
            EncoderPins {
                line_a: gpio1.input(pins.p22).expect("P22 is a GPIO1 pin"),
                line_b: gpio1.input(pins.p21).expect("P21 is a GPIO1 pin"),
            },
            EncoderPins {
                line_a: gpio1.input(pins.p16).expect("P16 is a GPIO1 pin"),
                line_b: gpio1.input(pins.p15).expect("P15 is a GPIO1 pin"),
            },
        ],
        [
            gpio1.input(pins.p23).expect("P23 is a GPIO1 pin"),
            gpio1.input(pins.p14).expect("P14 is a GPIO1 pin"),
            gpio4.input(pins.p5).expect("P5 is a GPIO4 pin"),
            gpio4.input(pins.p4).expect("P4 is a GPIO4 pin"),
        ],
        board::BUTTON_POLARITY,
    );

    let mut engine = Engine::new(
        cv_inputs,
        dac,
        triggers,
        panel,
        SystemClock::new(gpt1),
        display,
    );
    engine.set_render_interval(board::RENDER_INTERVAL_TICKS);

    loop {
        {
            let (cv_inputs, _, triggers, panel, _) = engine.parts_mut();
            cv_inputs.sample();
            triggers.sample();
            panel.sample();
        }
        let report = engine.tick();

        // Cooperative pacing against the microsecond clock: spin until the next
        // period boundary. There is no scheduler and no interrupt in the signal
        // path, which keeps the timing easy to reason about; an overrunning tick
        // simply starts the next one immediately rather than accumulating debt.
        // A 64-bit microsecond counter takes 584 000 years to wrap, so a plain
        // comparison is safe here; the clock folds the 32-bit hardware rollover
        // away for us.
        let deadline = report.timestamp_micros + u64::from(board::TICK_PERIOD_MICROS);
        while engine.clock().now_micros() < deadline {
            cortex_m::asm::nop();
        }
    }
}

/// Compile-time reminder that the engine drives exactly four channels.
const _: () = assert!(CV_CHANNELS == 4 && CvChannel::ALL.len() == CV_CHANNELS);
