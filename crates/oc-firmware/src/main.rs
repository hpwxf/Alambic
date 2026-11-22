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
//! Diagnostics that need a host go out over the Teensy's native USB device,
//! which shows as two CDC ACM serial ports ([`usb`]): the first carries the
//! text log, the second the probe protocol for `oc-probe` (journal/D-029).
//! Plug the USB cable into the Teensy, flash, then open the first port from
//! the host (115200 is conventional but the CDC ACM path ignores baud). Early
//! boot stages also blink the onboard LED before SPI claims that pad — see
//! [`boot_led`].
//!
//! # Unsafe code
//!
//! `deny(unsafe_code)` holds everywhere but three modules, each with its own
//! `allow` and a `SAFETY:` comment on every block: [`flash_store`] (the boot
//! ROM's flash driver), [`reboot`] (a fuse read and the breakpoint that
//! hands over to the bootloader) and [`audio_irq`] (the audio interrupt's
//! handler and NVIC line). Other register access is confined to
//! `teensy4-bsp` and `imxrt-hal`.
//!
//! # The audio interrupt
//!
//! From the end of the start-up on, the audio path owns the SPI bus: at
//! 47 619 Hz a DMA copy writes the DAC and an interrupt computes the next
//! sample and sends the screen in pieces between two bursts ([`audio`],
//! journal/D-033). The 1 kHz loop below runs the engine
//! and hands it what the outputs carry and what the screen shows.

#![no_std]
#![no_main]
#![deny(unsafe_code)]
#![warn(missing_docs)]

mod audio;
mod audio_irq;
mod board;
mod boot_led;
mod clock;
mod cv_in;
mod delay;
mod flash_store;
mod reboot;
#[cfg(feature = "settings-stress")]
mod stress;
mod usb;
mod usb_log;

use core::cell::RefCell;

use embedded_hal::delay::DelayNs as _;
use embedded_hal::digital::OutputPin as _;

use teensy4_bsp as bsp;
use teensy4_panic as _;

use bsp::board as bsp_board;

use oc_core::Engine;
use oc_core::platform::{CV_CHANNELS, Clock as _, CvChannel};
use oc_core::profile::Stage;
use oc_core::settings::SettingsStore;
use oc_core::toast::SaveOutcome;
use oc_drivers::panel::Panel;
use oc_drivers::shared_bus::SharedBus;
use oc_drivers::ssd130x::Ssd130x;
use oc_drivers::triggers::Triggers;
use oc_protocol::server::{Identity, Server};
use static_cell::{ConstStaticCell, StaticCell};

use crate::clock::SystemClock;
use crate::cv_in::CvInputs;
use crate::delay::CycleDelay;
use crate::flash_store::SettingsFlash;
use crate::usb::Usb;

/// How long to spin-poll USB after bringing the CDC backend up, so a host that
/// is already watching has a chance to finish enumeration before the first
/// log lines are no longer in the ring buffer. One second is ample on macOS
/// and Linux; messages logged later in `main` still go out during the loop.
const USB_ENUMERATION_WAIT_MS: u32 = 1_000;

/// The probe's session. Its queues are a few KiB (≈ 4.5 KiB in all):
/// built in place in a static, as [`ENGINE`] is, rather than on the 16 KiB
/// stack (journal/D-038).
static PROBE: ConstStaticCell<Server> = ConstStaticCell::new(Server::new(Identity {
    firmware: env!("CARGO_PKG_VERSION"),
    build: env!("OC_BUILD_ID"),
    can_reboot: false,
}));

/// The engine as this firmware composes it.
type FirmwareEngine = Engine<
    CvInputs,
    audio::MutedOutputs,
    Triggers<board::TriggerPin>,
    Panel<bsp::hal::gpio::Input>,
    SystemClock,
    audio::MailboxDisplay,
>;

/// The engine: every applet, the framebuffer, the profile (≈ 3.5 KiB). In
/// a static rather than on the 16 KiB stack, which the loop and every
/// interrupt share and whose deepest path `cargo xtask size` watches
/// (journal/D-038).
static ENGINE: StaticCell<FirmwareEngine> = StaticCell::new();

/// How long the boot check of the audio path watches it, in microseconds:
/// a few thousand periods, short enough not to delay the start-up.
const AUDIO_CHECK_MICROS: u32 = 50_000;

/// The tick, after the boot, at which the log reports what the audio path
/// did since: the splash is over and frames go out by then.
const AUDIO_REPORT_TICK: u64 = 3_000;

/// How long the USB keeps being served before a restart into the
/// bootloader, so the host sees the reply and the last log lines.
const RESTART_DRAIN_MS: u32 = 50;

// Wiring is one linear sequence of pin moves into concrete driver types;
// factoring it out only invents type parameters and does not shrink the
// real complexity, so the length lint is waived for this function alone.
#[allow(clippy::too_many_lines)]
// Not inlined: the start-up's temporaries (the board's resources, the USB
// device and the settings store as they are built, buffers of the OLED and
// the audio path, about 4 KiB) live on its frame and are gone before the
// loop, rather than staying under every call it makes (journal/D-038).
#[inline(never)]
fn boot() -> Running {
    let bsp_board::Resources {
        pins,
        mut gpio1,
        mut gpio2,
        mut gpio4,
        lpspi4,
        adc1,
        gpt1,
        pit,
        mut dma,
        usb: usb_instances,
        ..
    } = bsp_board::t40(bsp_board::instances());
    // From here on, pins are only reachable by signal (journal/D-007).
    let mut pins = board::split(pins);

    let mut delay = CycleDelay;
    // The core peripherals: only the debug block and the cycle counter are
    // used, for the profile (journal/D-029). Nothing else takes them.
    let mut cortex =
        cortex_m::Peripherals::take().unwrap_or_else(|| panic!("core peripherals already taken"));
    // The cycle counter runs from here: the audio interrupt times itself and
    // its periods on it from its first run (`SystemClock` starts it again).
    cortex.DCB.enable_trace();
    cortex_m::peripheral::DWT::unlock();
    cortex.DWT.enable_cycle_counter();

    // 1 flash: we reached `main`. The LED pad is still free for GPIO here.
    boot_led::signal(&mut gpio2, &mut pins.led_sck, &mut delay, 1);

    // USB: the log port and the probe port. Interrupts stay off: the 1 kHz
    // loop polls the device.
    usb_log::init();
    let mut usb = Usb::new(usb_instances);

    log::info!(
        "oc-firmware {} ({}) starting (oled={})",
        env!("CARGO_PKG_VERSION"),
        env!("OC_BUILD_ID"),
        board::OLED_CONTROLLER.name()
    );
    // Drain / keep the device alive while the host enumerates.
    for _ in 0..USB_ENUMERATION_WAIT_MS {
        usb.poll();
        delay.delay_ms(1);
    }
    log::info!("usb cdc up");
    usb.poll();

    // ---- CV inputs: analog pads, never driven --------------------------------
    let cv_pads = pins.cv.claim(&adc1);
    let cv_inputs = CvInputs::new(adc1, cv_pads);
    log::info!("adc ok");
    usb.poll();
    boot_led::signal(&mut gpio2, &mut pins.led_sck, &mut delay, 2);

    // ---- trigger inputs -----------------------------------------------------
    // Pulled up: the front-end buffers are open-collector / active-low against
    // the MCU pad.
    let triggers = Triggers::new(
        pins.triggers.claim(&mut gpio1, &mut gpio4),
        board::TRIGGER_POLARITY,
    );
    log::info!("triggers ok");
    usb.poll();
    // Last LED use: after this, the LED pad becomes SPI SCK.
    boot_led::signal(&mut gpio2, &mut pins.led_sck, &mut delay, 3);

    // ---- shared SPI bus -----------------------------------------------------
    // The DAC and the OLED share data and clock and are told apart by their
    // chip selects: the DAC's is the controller's PCS0, the OLED's a GPIO.
    // The OLED is initialised over a blocking handle first; then the audio
    // interrupt takes the bus over (journal/D-033).
    let bus = RefCell::new(bsp_board::lpspi(
        lpspi4,
        pins.spi.with_clock(pins.led_sck),
        board::OLED_SPI_HZ,
    ));
    log::info!("spi ok");
    usb.poll();

    let dac_lines = pins.dac.claim(&mut gpio2);
    let oled_lines = pins.oled.claim(&mut gpio2);
    let mut display = Ssd130x::new(
        SharedBus::with_select(&bus, board::select_oled),
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
    usb.poll();

    // A dead panel must not stop the module: the CV path is what matters.
    // The result is logged so a blank screen is no longer silent on USB.
    match display.init(&mut delay) {
        Ok(()) => log::info!("oled init ok ({})", board::OLED_CONTROLLER.name()),
        Err(e) => log::error!("oled init failed: {e:?}"),
    }
    usb.poll();

    // ---- audio interrupt: the bus, the DAC and the screen from here on ----
    // The reset line stays held here, high, for good.
    let (_, oled_cs, oled_dc, _oled_reset) = display.release();
    let mut spi = bus.into_inner();
    let clocks = board::bus_clocks(&mut spi);
    let dac_dma = dma[board::AUDIO_DMA_CHANNEL]
        .take()
        .unwrap_or_else(|| panic!("DMA channel {} already taken", board::AUDIO_DMA_CHANNEL));
    audio::start(
        spi,
        oled_cs,
        oled_dc,
        pit,
        dac_dma,
        clocks,
        board::OLED_CONTROLLER,
    );
    // Expected: interrupts and bursts at the audio rate, no chunk yet (the
    // engine has drawn nothing); anything else says which part stops.
    let health = audio::self_check(AUDIO_CHECK_MICROS);
    log::info!(
        "audio at {} Hz, DAC by DMA, {} screen bytes a chunk; check over {} us: \
         {} interrupts ({}/s), {} DAC bursts ({}/s), {} chunks, {} releases; \
         interrupt {} us mean, {} us max; period {} to {} us; \
         line enabled {} pending {}; DMA error {} (status {:#010x})",
        board::AUDIO_RATE_HZ,
        board::OLED_CHUNK_BYTES,
        health.micros,
        health.counts.periods,
        health.per_second(health.counts.periods),
        health.counts.bursts,
        health.per_second(health.counts.bursts),
        health.counts.chunks,
        health.counts.releases,
        health.audio_micros.0,
        health.audio_micros.1,
        health.period_micros.0,
        health.period_micros.1,
        health.line.0,
        health.line.1,
        health.dma_error.0,
        health.dma_error.1
    );
    usb.poll();

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
    usb.poll();

    let engine = engine_in_static(
        cv_inputs,
        triggers,
        panel,
        SystemClock::new(gpt1, &mut cortex.DCB, &mut cortex.DWT),
    );
    engine.set_render_interval(board::RENDER_INTERVAL_TICKS);
    engine
        .profile_mut()
        .set_expected_period_micros(board::TICK_PERIOD_MICROS);
    engine.set_build_id(env!("OC_BUILD_ID"));

    // ---- settings: calibration and preferences, from the flash -------------
    // Loaded before the first tick, so the splash already uses the stored
    // screen preferences and the outputs the stored calibration. A module
    // whose flash cannot be used still starts, on the defaults.
    let settings_store = match SettingsFlash::new() {
        Ok(flash) => {
            let mut store = SettingsStore::new(flash);
            let (settings, outcome) = store.load_or_default();
            log::info!("settings: {outcome:?}");
            engine.apply_settings(settings);
            Some(store)
        }
        Err(error) => {
            log::warn!("settings flash unavailable ({error:?}): defaults, nothing saved");
            None
        }
    };
    usb.poll();

    #[cfg(feature = "settings-stress")]
    let mut settings_store = settings_store;
    #[cfg(feature = "settings-stress")]
    if let Some(store) = settings_store.as_mut() {
        let original = *engine.settings();
        let (_, _, _, _, display) = engine.parts_mut();
        let report = stress::run(store, original, display, &mut || usb.poll());
        // Leave the result on screen long enough to read before the splash.
        for _ in 0..3_000 {
            delay.delay_ms(1);
            usb.poll();
        }
        engine.report_save(if report.ok {
            SaveOutcome::Stored
        } else {
            SaveOutcome::Failed
        });
    }

    // The log stops here until an event: say where the live data is
    // (journal/D-029).
    log::info!(
        "boot done; this log now shows events only, live data: cargo probe (stats, stream, inputs)"
    );
    Running {
        engine,
        usb,
        settings_store,
    }
}

/// What the loop keeps of the start-up.
struct Running {
    engine: &'static mut FirmwareEngine,
    usb: Usb,
    /// `None` when the flash cannot hold the settings.
    settings_store: Option<SettingsStore<SettingsFlash>>,
}

/// Wires the module, then runs the loop: [`boot`] and [`run`] one after
/// the other, so that the start-up's frame is gone before the loop's
/// (journal/D-038); what the loop keeps stays here, written by `boot`.
#[bsp::rt::entry]
fn main() -> ! {
    let mut running = boot();
    run(&mut running)
}

/// Runs the 1 kHz loop for good. Not inlined: its frame is the loop's
/// alone.
#[inline(never)]
fn run(running: &mut Running) -> ! {
    let Running {
        engine,
        usb,
        settings_store,
    } = running;
    let probe = PROBE.take();
    // The audio path's counts as the loop starts, for the report a few
    // seconds in.
    let boot_audio = audio::totals();
    let mut publisher = audio::Publisher::new();
    probe.set_can_reboot(reboot::is_supported());

    // The start of the current period, sampling included: pacing from the
    // tick's own start left the sampling outside it and stretched every
    // period by its cost.
    let mut deadline = engine.clock().now_micros();
    // The longest USB poll seen in the idle spin, in microseconds: no poll
    // starts closer than that to a deadline. Learnt at run time rather than
    // assumed, since it depends on the host and the traffic.
    let mut longest_poll_micros: u64 = 0;
    loop {
        let mark = engine.clock().fine_count();
        {
            let (cv_inputs, _, triggers, panel, _) = engine.parts_mut();
            cv_inputs.sample();
            triggers.sample_capturing();
            panel.sample();
        }
        record_since(engine, Stage::Sample, mark);
        // The interrupt plays the outputs; the engine renders no voice and
        // reports what it played (journal/D-040).
        engine.set_played(audio::played());
        let report = engine.tick();
        let mark = engine.clock().fine_count();
        publisher.publish(
            engine.params_revision(),
            engine.params(),
            &engine.calibration().outputs,
        );
        record_since(engine, Stage::Publish, mark);
        if report.tick_count == AUDIO_REPORT_TICK {
            let since = audio::totals().since(boot_audio);
            log::info!(
                "audio after {} ticks: {} interrupts, {} DAC bursts, {} chunks, {} releases, \
                 {} frames",
                AUDIO_REPORT_TICK,
                since.periods,
                since.bursts,
                since.chunks,
                since.releases,
                since.frames
            );
        }
        audio::drain_stats(engine.profile_mut());
        probe.after_tick(&report, engine);
        if engine.take_save_request() {
            // Blocks for an erase now and then (tens of milliseconds); only
            // ever after an explicit SAVE on the settings screen. The boot
            // ROM masks the audio interrupt meanwhile: the outputs rest
            // rather than freeze on a sample (journal/D-033).
            publisher.rest_outputs(&engine.calibration().outputs);
            let saved = settings_store
                .as_mut()
                .map(|store| store.save(engine.settings()));
            let outcome = match saved {
                Some(Ok(())) => {
                    let erases = settings_store
                        .as_mut()
                        .map_or(0, |store| store.flash().erase_count());
                    log::info!("settings saved ({erases} sector erases since power-up)");
                    SaveOutcome::Stored
                }
                Some(Err(error)) => {
                    log::warn!("settings NOT saved: {error:?}");
                    SaveOutcome::Failed
                }
                None => {
                    log::warn!("settings NOT saved: no flash store");
                    SaveOutcome::SessionOnly
                }
            };
            audio::resume_outputs();
            engine.report_save(outcome);
            usb.poll();
        }
        // Keep the CDC pipe alive. Cheap when the host is absent: the backend
        // either has nothing queued or drops into a full ring buffer.
        let mark = engine.clock().fine_count();
        usb.poll();
        usb.serve(probe, engine);
        record_since(engine, Stage::Usb, mark);
        if probe.take_reboot_request() || usb.take_reboot_request() {
            restart_into_loader(usb, probe, engine);
        }

        // Cooperative pacing against the microsecond clock, on a fixed
        // schedule: each period ends one period after the previous deadline,
        // not after the moment the spin noticed it, so the spin's own
        // lateness never adds up. There is
        // no scheduler and no interrupt in the signal path, which keeps the
        // timing easy to reason about; an overrunning tick re-anchors the
        // schedule and the next one starts at once, rather than accumulating
        // debt. A 64-bit microsecond counter takes 584 000 years to wrap, so
        // a plain comparison is safe here; the clock folds the 32-bit
        // hardware rollover away for us.
        deadline += u64::from(board::TICK_PERIOD_MICROS);
        let now = engine.clock().now_micros();
        if now >= deadline {
            deadline = now;
        } else {
            // Poll USB inside the idle spin so bulk log traffic still drains
            // when a tick finishes early, but only while the longest poll so
            // far still fits before the deadline (one more microsecond for
            // the clock's resolution).
            loop {
                let before = engine.clock().now_micros();
                if before + longest_poll_micros + 1 >= deadline {
                    break;
                }
                usb.poll();
                usb.serve(probe, engine);
                let took = engine.clock().now_micros() - before;
                longest_poll_micros = longest_poll_micros.max(took);
            }
            while engine.clock().now_micros() < deadline {
                cortex_m::asm::nop();
            }
        }
    }
}

/// Lets the last replies and log lines reach the host, then hands the chip
/// to the bootloader; on a chip that cannot do it, only says so.
fn restart_into_loader<A, O, D, C, K, S>(
    usb: &mut Usb,
    probe: &mut Server,
    engine: &mut Engine<A, O, D, C, K, S>,
) where
    A: oc_core::platform::AnalogIn,
    O: oc_core::platform::AnalogOut,
    D: oc_core::platform::DigitalIn,
    C: oc_core::platform::Controls,
    K: oc_core::platform::Clock,
    S: oc_core::platform::Display,
{
    if !reboot::is_supported() {
        log::warn!("restart into the bootloader refused: secure chip, press PROGRAM");
        return;
    }
    log::info!("restarting into the bootloader");
    let mut delay = CycleDelay;
    for _ in 0..RESTART_DRAIN_MS {
        usb.poll();
        usb.serve(probe, engine);
        delay.delay_ms(1);
    }
    audio::stop();
    reboot::to_loader();
}

/// Records the fine counts since `mark` as one duration of `stage`.
fn record_since<A, O, D, C, S>(
    engine: &mut Engine<A, O, D, C, SystemClock, S>,
    stage: Stage,
    mark: u32,
) where
    A: oc_core::platform::AnalogIn,
    O: oc_core::platform::AnalogOut,
    D: oc_core::platform::DigitalIn,
    C: oc_core::platform::Controls,
    S: oc_core::platform::Display,
{
    let fine = engine.clock().fine_count().wrapping_sub(mark);
    engine.profile_mut().record(stage, fine);
}

/// Builds the engine in [`ENGINE`], once. Not inlined: built in [`boot`],
/// the value would add its 3.5 KiB to the start-up's frame, the deepest
/// path of the stack.
#[inline(never)]
fn engine_in_static(
    cv_inputs: CvInputs,
    triggers: Triggers<board::TriggerPin>,
    panel: Panel<bsp::hal::gpio::Input>,
    clock: SystemClock,
) -> &'static mut FirmwareEngine {
    ENGINE.init_with(|| {
        Engine::new(
            cv_inputs,
            audio::MutedOutputs,
            triggers,
            panel,
            clock,
            audio::MailboxDisplay::new(),
        )
    })
}

/// Compile-time reminder that the engine drives exactly four channels.
const _: () = assert!(CV_CHANNELS == 4 && CvChannel::ALL.len() == CV_CHANNELS);
