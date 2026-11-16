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
//! # There is deliberately no UART console
//!
//! Every LPUART the Teensy 4.0 exposes on pins 0 to 23 collides with the panel:
//! LPUART2 wants pins 14 and 15, which are the right encoder's switch and its
//! `B` line; LPUART6 wants pins 0 and 1, which are `TR1` and `TR2`; LPUART4
//! wants pins 7 and 8, which are the OLED's reset and chip select. Driving any
//! of those as a transmit line is exactly the electrical hazard the pinout table
//! warns about, so the firmware drives **no** UART.
//!
//! # USB CDC boot log
//!
//! Diagnostics that need a host go out over the Teensy's native USB device as
//! a CDC ACM serial port (`imxrt-log`). Plug the USB cable into the Teensy,
//! flash, then open the virtual COM port from the host (115200 is conventional
//! but the CDC ACM path ignores baud). Early boot stages also blink the
//! onboard LED before SPI claims that pad — see [`boot_led`].
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
mod boot_led;
mod clock;
mod cv_in;
mod delay;

use core::cell::RefCell;

use embedded_hal::delay::DelayNs as _;
use embedded_hal::digital::OutputPin as _;

use teensy4_bsp as bsp;
use teensy4_panic as _;

use bsp::board as bsp_board;

use oc_core::Engine;
use oc_core::platform::{AnalogIn as _, CV_CHANNELS, Clock as _, CvChannel};
use oc_core::signal::Extent;
use oc_drivers::dac8565::Dac8565;
use oc_drivers::panel::Panel;
use oc_drivers::shared_bus::SharedBus;
use oc_drivers::ssd130x::Ssd130x;
use oc_drivers::triggers::Triggers;

use crate::clock::SystemClock;
use crate::cv_in::CvInputs;
use crate::delay::CycleDelay;

/// How long to spin-poll USB after bringing the CDC backend up, so a host that
/// is already watching has a chance to finish enumeration before the first
/// log lines are no longer in the ring buffer. One second is ample on macOS
/// and Linux; messages logged later in `main` still go out during the loop.
const USB_ENUMERATION_WAIT_MS: u32 = 1_000;

/// Heartbeat log period in engine ticks (1 kHz → once per second).
const HEARTBEAT_TICKS: u32 = 1_000;

// Wiring is one linear sequence of pin moves into concrete driver types;
// factoring it out only invents type parameters and does not shrink the
// real complexity, so the length lint is waived for this entry point alone.
#[allow(clippy::too_many_lines)]
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
        usb,
        ..
    } = bsp_board::t40(bsp_board::instances());
    // From here on, pins are only reachable by signal (journal/D-007).
    let mut pins = board::split(pins);

    let mut delay = CycleDelay;

    // 1 flash: we reached `main`. The LED pad is still free for GPIO here.
    boot_led::signal(&mut gpio2, &mut pins.led_sck, &mut delay, 1);

    // USB CDC logger. Interrupts stay off: the 1 kHz loop polls the backend.
    // A failure here means the logger was already installed, which cannot
    // happen in this single-threaded firmware.
    let mut usb_log = imxrt_log::log::usbd(usb, imxrt_log::Interrupts::Disabled)
        .unwrap_or_else(|_| panic!("USB log already initialised"));

    log::info!(
        "oc-firmware {} ({}) starting (oled={})",
        env!("CARGO_PKG_VERSION"),
        env!("OC_BUILD_ID"),
        board::OLED_CONTROLLER.name()
    );
    // Drain / keep the device alive while the host enumerates.
    for _ in 0..USB_ENUMERATION_WAIT_MS {
        usb_log.poll();
        delay.delay_ms(1);
    }
    log::info!("usb cdc up");
    usb_log.poll();

    // ---- CV inputs: analog pads, never driven --------------------------------
    let cv_pads = pins.cv.claim(&adc1);
    let cv_inputs = CvInputs::new(adc1, cv_pads, board::CV_INPUT_CALIBRATION);
    log::info!("adc ok");
    usb_log.poll();
    boot_led::signal(&mut gpio2, &mut pins.led_sck, &mut delay, 2);

    // ---- trigger inputs -----------------------------------------------------
    // Pulled up: the front-end buffers are open-collector / active-low against
    // the MCU pad.
    let triggers = Triggers::new(
        pins.triggers.claim(&mut gpio1, &mut gpio4),
        board::TRIGGER_POLARITY,
    );
    log::info!("triggers ok");
    usb_log.poll();
    // Last LED use: after this, the LED pad becomes SPI SCK.
    boot_led::signal(&mut gpio2, &mut pins.led_sck, &mut delay, 3);

    // ---- shared SPI bus -----------------------------------------------------
    // The DAC and the OLED share data and clock and are told apart by their
    // chip selects, which the drivers own; hence a shared bus rather than two
    // `SpiDevice`s.
    let bus = RefCell::new(bsp_board::lpspi(
        lpspi4,
        pins.spi.with_clock(pins.led_sck),
        board::SPI_CLOCK_HZ,
    ));
    log::info!("spi ok");
    usb_log.poll();

    let dac_lines = pins.dac.claim(&mut gpio2);
    let dac = Dac8565::new(
        SharedBus::new(&bus),
        dac_lines.cs,
        board::CV_OUTPUT_CALIBRATIONS,
    );
    let oled_lines = pins.oled.claim(&mut gpio2);
    let mut display = Ssd130x::new(
        SharedBus::new(&bus),
        oled_lines.cs,
        oled_lines.dc,
        oled_lines.reset,
        board::OLED_CONTROLLER,
    );

    // The DAC's reset line is released before the first write. Leaving it
    // asserted would hold the outputs at zero scale, which on an inverting
    // output stage means +6 V on every jack.
    let mut dac_reset = dac_lines.reset;
    let _ = dac_reset.set_high();
    log::info!("dac reset released");
    usb_log.poll();

    // A dead panel must not stop the module: the CV path is what matters.
    // The result is logged so a blank screen is no longer silent on USB.
    match display.init(&mut delay) {
        Ok(()) => log::info!("oled init ok ({})", board::OLED_CONTROLLER.name()),
        Err(e) => log::error!("oled init failed: {e:?}"),
    }
    usb_log.poll();

    // ---- panel controls -----------------------------------------------------
    // Encoders and buttons short to ground; `claim` applies the pull-up without
    // which neither detents nor presses ever reach the engine.
    let panel_lines = pins.panel.claim(&mut gpio1, &mut gpio4);
    let panel = Panel::new(
        panel_lines.encoders,
        panel_lines.buttons,
        board::BUTTON_POLARITY,
    );
    log::info!("panel ok; entering 1 kHz loop");
    usb_log.poll();

    let mut engine = Engine::new(
        cv_inputs,
        dac,
        triggers,
        panel,
        SystemClock::new(gpt1),
        display,
    );
    engine.set_render_interval(board::RENDER_INTERVAL_TICKS);
    engine.set_build_id(env!("OC_BUILD_ID"));

    // Per-CV-input extent over the current heartbeat second, in calibrated
    // millivolts and in raw ADC codes: logged once a second so the noise and
    // offset of an input can be measured as numbers over USB (TESTING.md 9.2).
    let mut cv_millivolts = [Extent::new(); CV_CHANNELS];
    let mut cv_codes = [Extent::new(); CV_CHANNELS];

    let mut ticks: u32 = 0;
    loop {
        {
            let (cv_inputs, _, triggers, panel, _) = engine.parts_mut();
            cv_inputs.sample();
            triggers.sample();
            panel.sample();
        }
        let report = engine.tick();
        {
            let (cv_inputs, ..) = engine.parts_mut();
            for channel in CvChannel::ALL {
                let index = channel.index();
                cv_millivolts[index].update(cv_inputs.read_cv(channel));
                cv_codes[index].update(i32::from(cv_inputs.raw(channel)));
            }
        }

        // Keep the CDC pipe alive. Cheap when the host is absent: the backend
        // either has nothing queued or drops into a full ring buffer.
        usb_log.poll();

        // Occasional heartbeat so a quiet serial monitor still proves the
        // loop is running even when the OLED is blank.
        ticks = ticks.wrapping_add(1);
        if ticks.is_multiple_of(HEARTBEAT_TICKS) {
            log::info!("tick={ticks} last_us={}", report.elapsed_micros);
            usb_log.poll();
            for channel in CvChannel::ALL {
                let index = channel.index();
                if let (Some((low_mv, high_mv)), Some((low_code, high_code))) =
                    (cv_millivolts[index].range(), cv_codes[index].range())
                {
                    log::info!(
                        "cv{} mv={low_mv}..{high_mv} (pp {}) code={low_code}..{high_code}",
                        index + 1,
                        // Saturating: overflow checks are on in release
                        // (journal/D-018), and a log line must never freeze
                        // the module.
                        high_mv.saturating_sub(low_mv)
                    );
                }
                cv_millivolts[index].reset();
                cv_codes[index].reset();
            }
            usb_log.poll();
        }

        // Cooperative pacing against the microsecond clock: spin until the next
        // period boundary. There is no scheduler and no interrupt in the signal
        // path, which keeps the timing easy to reason about; an overrunning tick
        // simply starts the next one immediately rather than accumulating debt.
        // A 64-bit microsecond counter takes 584 000 years to wrap, so a plain
        // comparison is safe here; the clock folds the 32-bit hardware rollover
        // away for us.
        let deadline = report.timestamp_micros + u64::from(board::TICK_PERIOD_MICROS);
        while engine.clock().now_micros() < deadline {
            // Poll USB inside the idle spin so bulk log traffic still drains
            // when a tick finishes early.
            usb_log.poll();
            cortex_m::asm::nop();
        }
    }
}

/// Compile-time reminder that the engine drives exactly four channels.
const _: () = assert!(CV_CHANNELS == 4 && CvChannel::ALL.len() == CV_CHANNELS);
