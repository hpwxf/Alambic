//! **The single source of truth for the Ornament & Crime pinout.**
//!
//! # Read this before the first flash
//!
//! A wrong firmware cannot brick a Teensy 4.0: the `HalfKay` bootloader lives in
//! ROM on a separate chip and the PROGRAM button always restores it. The real
//! residual risk is *electrical*: configuring as an **output** a pin that the
//! module drives as an input can damage either side. Every pin direction in
//! this module is therefore listed explicitly in the table below, and this file
//! is the only place in the firmware allowed to name a pin.
//!
//! # Provenance
//!
//! The assignments come from the reference Ornament & Crime firmware, which
//! keeps them in `OC_gpio.cpp` (the "old hardware", default-orientation branch)
//! and `OC_gpio.h`. Teensy 4.0 is pin-compatible with the Teensy 3.2 that O&C
//! was designed around for pins 0 to 23, so the classic map applies unchanged.
//!
//! | Signal        | Teensy pin | Direction | Notes                                    |
//! |---------------|-----------:|-----------|------------------------------------------|
//! | `TR1`         |          0 | input     | pull-up, **active low** (inverting buffer) |
//! | `TR2`         |          1 | input     | pull-up, active low                      |
//! | `TR3`         |          2 | input     | pull-up, active low                      |
//! | `TR4`         |          3 | input     | pull-up, active low                      |
//! | button `down` |          4 | input     | pull-up, active low                      |
//! | button `up`   |          5 | input     | pull-up, active low                      |
//! | `OLED_DC`     |          6 | output    | low = command, high = data               |
//! | `OLED_RST`    |          7 | output    | active low                               |
//! | `OLED_CS`     |          8 | output    | active low                               |
//! | `DAC_RST`     |          9 | output    | active low                               |
//! | `DAC_CS`      |         10 | output    | active low                               |
//! | `SPI MOSI`    |         11 | output    | shared by the DAC and the OLED           |
//! | `SPI SCK`     |         13 | output    | shared; also the on-board LED            |
//! | right enc btn |         14 | input     | pull-up, active low                      |
//! | right enc `B` |         15 | input     | pull-up                                  |
//! | right enc `A` |         16 | input     | pull-up                                  |
//! | `CV4`         |         17 | **input** | analog, A3 — never drive this pin         |
//! | `CV2`         |         18 | **input** | analog, A4 — never drive this pin         |
//! | `CV1`         |         19 | **input** | analog, A5 — never drive this pin         |
//! | `CV3`         |         20 | **input** | analog, A6 — never drive this pin         |
//! | left enc `B`  |         21 | input     | pull-up                                  |
//! | left enc `A`  |         22 | input     | pull-up                                  |
//! | left enc btn  |         23 | input     | pull-up, active low                      |
//!
//! Note the CV order: `CV1..CV4` are **not** in pin order. That is how the
//! panel is wired, and it is the classic source of a "the inputs are shuffled"
//! bug.
//!
//! # Unverified against real hardware
//!
//! Everything below is derived from source, not measured. Until the diagnostic
//! applet has been run on a real module, treat as provisional:
//!
//! * the ADC and DAC calibration slopes and offsets. Both stages **invert**
//!   (the reference firmware computes `offset - raw` for inputs and
//!   `MAX_VALUE - value` for outputs), which is expressed here as a negative
//!   slope, but the exact gain is a per-unit calibration.
//! * which OLED controller the panel carries. Stock O&C is SH1106; SSD1306/SSD1309 are opt-in.

use oc_core::calibration::{
    ADC_CODES, CV_IN_MAX_MV, CV_IN_MIN_MV, CV_OUT_MAX_MV, CV_OUT_MIN_MV, CvInputCalibration,
    CvOutputCalibration, DAC_CODE_MAX,
};
use oc_core::platform::CV_CHANNELS;
use oc_drivers::ssd130x::Controller;
use oc_drivers::triggers::Polarity;
use teensy4_bsp::hal::iomuxc::{Config, Hysteresis, PullKeeper};

/// SPI clock for the DAC8565 and the OLED, in hertz.
///
/// The DAC8565 accepts up to 50 MHz and the OLED controllers up to about 10 MHz,
/// and the two share the bus, so the slower part sets the rate.
pub(crate) const SPI_CLOCK_HZ: u32 = 8_000_000;

/// Nominal period of the main loop, in microseconds.
pub(crate) const TICK_PERIOD_MICROS: u32 = 1_000;

/// Redraw the screen once every this many ticks, i.e. about 50 Hz.
///
/// Rendering costs three orders of magnitude more than the signal path (see the
/// benchmarks in `oc-core`), and no OLED shows more than about 60 frames per
/// second, so the redraw rate is decoupled from the control loop.
pub(crate) const RENDER_INTERVAL_TICKS: u32 = 20;

/// Which OLED controller the panel carries.
///
/// Stock Ornament & Crime (Phazerville / TLM `µO_C`) uses an SH1106-class panel.
/// Override with the `ssd1306` or `ssd1309` Cargo features for third-party
/// builds. Getting the controller wrong yields a blank or garbled screen
/// without a panic.
pub(crate) const OLED_CONTROLLER: Controller = if cfg!(feature = "ssd1309") {
    Controller::Ssd1309
} else if cfg!(feature = "ssd1306") {
    Controller::Ssd1306
} else {
    Controller::Sh1106
};

/// Trigger inputs are buffered through inverting transistors and pulled up, so
/// a gate at the jack reads as a low pin.
pub(crate) const TRIGGER_POLARITY: Polarity = Polarity::ActiveLow;

/// Buttons and encoder switches short to ground against an internal pull-up.
pub(crate) const BUTTON_POLARITY: Polarity = Polarity::ActiveLow;

/// Pad configuration for every panel digital input (triggers, buttons, encoder
/// lines and encoder switches).
///
/// The panel shorts these pins to ground and relies on the MCU's internal
/// pull-up. `Port::input` only selects the GPIO alternate; without this pad
/// config the pins float, quadrature decoding never settles, and every
/// active-low button looks stuck or dead. Hysteresis keeps mechanical contacts
/// from chattering at the pad.
pub(crate) const DIGITAL_INPUT_CONFIG: Config = Config::zero()
    .set_pull_keeper(Some(PullKeeper::Pullup22k))
    .set_hysteresis(Hysteresis::Enabled);

/// Bit offset of the onboard LED within GPIO2.
///
/// Teensy pin 13 is `GPIO_B0_03` = `GPIO2_IO03`, which is also LPSPI4 SCK.
/// Boot breadcrumbs may drive this pad as GPIO **before** the SPI bus claims
/// it; afterwards only a panic handler (register-level SOS) may touch it.
pub(crate) const LED_GPIO2_OFFSET: u32 = 3;

/// Provisional CV input calibration.
///
/// The analog front end inverts, hence the negative slope: a rising voltage at
/// the jack lowers the ADC code. Zero volts sits at mid-scale until a real
/// calibration is measured.
pub(crate) const CV_INPUT_CALIBRATION: CvInputCalibration = CvInputCalibration {
    zero_code: ADC_CODES / 2,
    nanovolts_per_code: -CvInputCalibration::NOMINAL.nanovolts_per_code,
};

/// Provisional CV output calibration.
///
/// The output stage inverts as well: the reference firmware writes
/// `MAX_VALUE - value`, so code zero produces the *highest* voltage.
pub(crate) const CV_OUTPUT_CALIBRATION: CvOutputCalibration = CvOutputCalibration {
    zero_code: DAC_CODE_MAX - CvOutputCalibration::NOMINAL.zero_code,
    millicodes_per_volt: -CvOutputCalibration::NOMINAL.millicodes_per_volt,
};

/// One calibration per output channel, ready for the DAC driver.
pub(crate) const CV_OUTPUT_CALIBRATIONS: [CvOutputCalibration; CV_CHANNELS] =
    [CV_OUTPUT_CALIBRATION; CV_CHANNELS];

/// ADC channel of each CV input, in `CV1..CV4` order.
///
/// The firmware never uses these numbers to address the converter: they are
/// resolved from the pad types by `imxrt-iomuxc`. They are recorded here so that
/// the table above can be checked against the Teensy documentation by eye, and
/// asserted below so that the two cannot silently disagree.
pub(crate) const CV_ADC_CHANNELS: [u32; CV_CHANNELS] = [5, 6, 15, 11];

/// Compile-time sanity checks on the calibration data.
///
/// These are cheap and catch a copy-and-paste slip in the constants above
/// before it reaches a module.
const _: () = {
    assert!(
        CV_INPUT_CALIBRATION.nanovolts_per_code < 0,
        "the O&C analog front end inverts; the input slope must be negative"
    );
    assert!(
        CV_OUTPUT_CALIBRATION.millicodes_per_volt < 0,
        "the O&C output stage inverts; the output slope must be negative"
    );
    assert!(
        CV_OUTPUT_CALIBRATION.zero_code > 0 && CV_OUTPUT_CALIBRATION.zero_code < DAC_CODE_MAX,
        "0 V must land strictly inside the DAC range"
    );
    assert!(
        CV_INPUT_CALIBRATION.to_millivolts(0) > CV_INPUT_CALIBRATION.to_millivolts(4095),
        "an inverting front end must read code 0 as the highest voltage"
    );
    assert!(
        CV_OUTPUT_CALIBRATION.to_code(CV_OUT_MAX_MV) < CV_OUTPUT_CALIBRATION.to_code(CV_OUT_MIN_MV),
        "an inverting output stage must map the highest voltage to the lowest code"
    );
    assert!(
        RENDER_INTERVAL_TICKS > 0,
        "the screen must be redrawn eventually"
    );
    // The four CV pads are all on `GPIO_AD_B1_*`, whose ADC channels are 5, 6,
    // 15 and 11 for pins 19, 18, 20 and 17 respectively.
    assert!(
        CV_ADC_CHANNELS[0] == 5 && CV_ADC_CHANNELS[1] == 6,
        "CV1 is pin 19 (channel 5) and CV2 is pin 18 (channel 6)"
    );
    assert!(
        CV_ADC_CHANNELS[2] == 15 && CV_ADC_CHANNELS[3] == 11,
        "CV3 is pin 20 (channel 15) and CV4 is pin 17 (channel 11)"
    );
    assert!(
        CV_IN_MIN_MV < 0 && CV_IN_MAX_MV > 0,
        "the CV inputs are bipolar"
    );
    assert!(
        CV_OUT_MIN_MV < 0 && CV_OUT_MAX_MV > 0,
        "the CV outputs are bipolar"
    );
};
