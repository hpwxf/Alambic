//! **The single source of truth for the Ornament & Crime pinout.**
//!
//! # Read this before the first flash
//!
//! A wrong firmware cannot brick a Teensy 4.0: the `HalfKay` bootloader lives in
//! ROM on a separate chip and the PROGRAM button always restores it. The real
//! residual risk is *electrical*: configuring as an **output** a pin that the
//! module drives as an input can damage either side. Every pin direction in
//! this module is therefore listed explicitly in the table below, and this file
//! is the only place in the firmware allowed to name a pin: [`split`] consumes
//! the BSP's pin set and hands the rest of the firmware typed groups named by
//! *signal*, whose `claim` methods configure each pad in the direction the
//! table states. `xtask/tests/pin_rule.rs` fails the build if another firmware
//! source names a pin (journal/D-007).
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
//! | `SPI MISO`    |         12 | input     | claimed by LPSPI4 only because its pin set needs one; nothing on the bus drives it (the DAC8565 and the OLED in 4-wire SPI have no data output) |
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
//! # What has been checked on a real module
//!
//! Verified on the TLM build while bringing the firmware up: the OLED rows
//! (SH1106, pins 6 to 8 and the shared bus), the boot LED, and the encoder and
//! button rows (pins 4, 5 and 14 to 16, 21 to 23, which needed the pad pull-up
//! of [`DIGITAL_INPUT_CONFIG`]).
//!
//! Still derived from source, not measured — treat as provisional until
//! `TESTING.md` Level 9 has been run:
//!
//! * the CV input rows (17 to 20), the trigger rows (0 to 3) and the DAC rows
//!   (9 and 10);
//! * the OLED controller on other builds. SH1106 is the default; SSD1306 and
//!   SSD1309 are opt-in features.
//!
//! The ADC and DAC calibration is not board data any more: the conversion,
//! the factory calibration (which expresses that both analog stages invert)
//! and its measurement on the module live in `oc_core::calibration`
//! (journal/D-027).

use oc_core::calibration::{CV_IN_MAX_MV, CV_IN_MIN_MV, CV_OUT_MAX_MV, CV_OUT_MIN_MV};
use oc_core::platform::{BUTTONS, CV_CHANNELS, ENCODERS, TRIGGER_CHANNELS};
use oc_drivers::panel::EncoderPins;
use oc_drivers::ssd130x::Controller;
use oc_drivers::triggers::Polarity;
use teensy4_bsp::board::LpspiPins;
use teensy4_bsp::hal::adc::{Adc, AnalogInput};
use teensy4_bsp::hal::gpio::{Input, Output, Port};
use teensy4_bsp::hal::iomuxc::{self, Config, Hysteresis, PullKeeper};
use teensy4_bsp::pins::imxrt_iomuxc::Iomuxc;
use teensy4_bsp::pins::t40::{
    P0, P1, P2, P3, P4, P5, P6, P7, P8, P9, P10, P11, P12, P13, P14, P15, P16, P17, P18, P19, P20,
    P21, P22, P23, Pins,
};

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

/// The pad shared by the onboard LED and SPI SCK.
///
/// Boot breadcrumbs borrow it as a GPIO; the SPI bus then consumes it.
pub(crate) type LedSckPin = P13;

/// Every pin the module uses, grouped by signal.
///
/// Built once by [`split`]. Fields are groups, not pins: the only way to turn
/// a group into a usable peripheral is its `claim` method below, which applies
/// the direction and pad configuration from the table above.
pub(crate) struct BoardPins {
    /// Onboard LED, later SPI SCK.
    pub(crate) led_sck: LedSckPin,
    /// The four analog CV inputs.
    pub(crate) cv: CvPins,
    /// The four trigger inputs.
    pub(crate) triggers: TriggerPins,
    /// SPI data lines (the clock is [`BoardPins::led_sck`]).
    pub(crate) spi: SpiPins,
    /// DAC8565 chip select and reset.
    pub(crate) dac: DacPins,
    /// OLED chip select, data/command and reset.
    pub(crate) oled: OledPins,
    /// Encoders and buttons.
    pub(crate) panel: PanelPins,
}

/// Splits the BSP's pin set into the module's signal groups.
///
/// Pins the module does not use stay inside `pins` and are dropped, so nothing
/// else can claim them by accident.
pub(crate) fn split(pins: Pins) -> BoardPins {
    BoardPins {
        led_sck: pins.p13,
        cv: CvPins {
            cv1: pins.p19,
            cv2: pins.p18,
            cv3: pins.p20,
            cv4: pins.p17,
        },
        triggers: TriggerPins {
            tr1: pins.p0,
            tr2: pins.p1,
            tr3: pins.p2,
            tr4: pins.p3,
        },
        spi: SpiPins {
            sdo: pins.p11,
            sdi: pins.p12,
        },
        dac: DacPins {
            cs: pins.p10,
            reset: pins.p9,
        },
        oled: OledPins {
            cs: pins.p8,
            dc: pins.p6,
            reset: pins.p7,
        },
        panel: PanelPins {
            left_a: pins.p22,
            left_b: pins.p21,
            right_a: pins.p16,
            right_b: pins.p15,
            left_switch: pins.p23,
            right_switch: pins.p14,
            up: pins.p5,
            down: pins.p4,
        },
    }
}

/// Enables the internal pull-up (and hysteresis) required by every panel
/// digital input, then returns the pad so it can be claimed as a GPIO.
///
/// `Port::input` only muxes the pad to GPIO; the electrical configuration is
/// a separate step. Forgetting it leaves buttons and encoders floating.
fn with_pullup<P: Iomuxc>(mut pin: P) -> P {
    iomuxc::configure(&mut pin, DIGITAL_INPUT_CONFIG);
    pin
}

/// The CV input pads, `CV1..CV4`. **Inputs only, never driven.**
pub(crate) struct CvPins {
    cv1: P19,
    cv2: P18,
    cv3: P20,
    cv4: P17,
}

impl CvPins {
    /// Claims the four pads as ADC inputs, in `CV1..CV4` order.
    ///
    /// The ADC channel of each pad is resolved from its type, so the mapping
    /// cannot silently drift from [`CV_ADC_CHANNELS`].
    pub(crate) fn claim(self, adc: &Adc) -> [AnalogInput; CV_CHANNELS] {
        let (Ok(cv1), Ok(cv2), Ok(cv3), Ok(cv4)) = (
            adc.input::<_, 1>(self.cv1),
            adc.input::<_, 1>(self.cv2),
            adc.input::<_, 1>(self.cv3),
            adc.input::<_, 1>(self.cv4),
        ) else {
            panic!("pins 19, 18, 20 and 17 are all ADC1 pads")
        };
        [cv1, cv2, cv3, cv4]
    }
}

/// The trigger input pads, `TR1..TR4`.
pub(crate) struct TriggerPins {
    tr1: P0,
    tr2: P1,
    tr3: P2,
    tr4: P3,
}

impl TriggerPins {
    /// Claims the four pads as pulled-up GPIO inputs, in `TR1..TR4` order.
    ///
    /// `Port::input` fails only when a pin does not belong to the port, so each
    /// `expect` documents a fact of the table above.
    pub(crate) fn claim(self, gpio1: &mut Port, gpio4: &mut Port) -> [Input; TRIGGER_CHANNELS] {
        [
            gpio1
                .input(with_pullup(self.tr1))
                .expect("P0 is a GPIO1 pin"),
            gpio1
                .input(with_pullup(self.tr2))
                .expect("P1 is a GPIO1 pin"),
            gpio4
                .input(with_pullup(self.tr3))
                .expect("P2 is a GPIO4 pin"),
            gpio4
                .input(with_pullup(self.tr4))
                .expect("P3 is a GPIO4 pin"),
        ]
    }
}

/// The SPI data pads.
pub(crate) struct SpiPins {
    sdo: P11,
    sdi: P12,
}

impl SpiPins {
    /// The LPSPI4 pin set, taking the LED pad as the clock: from here on the
    /// LED must not be touched again.
    pub(crate) fn with_clock(self, sck: LedSckPin) -> LpspiPins<P11, P12, P13> {
        LpspiPins {
            sdo: self.sdo,
            sdi: self.sdi,
            sck,
        }
    }
}

/// The DAC8565 control pads.
pub(crate) struct DacPins {
    cs: P10,
    reset: P9,
}

/// The DAC8565 control lines, as GPIO outputs.
pub(crate) struct DacLines {
    /// Chip select, active low. A plain GPIO rather than the LPSPI hardware PCS,
    /// so that the DAC's 24-bit framing stays in software.
    pub(crate) cs: Output,
    /// Reset, active low.
    pub(crate) reset: Output,
}

impl DacPins {
    /// Claims both pads as GPIO outputs.
    pub(crate) fn claim(self, gpio2: &mut Port) -> DacLines {
        DacLines {
            cs: gpio2.output(self.cs).expect("P10 is a GPIO2 pin"),
            reset: gpio2.output(self.reset).expect("P9 is a GPIO2 pin"),
        }
    }
}

/// The OLED control pads.
pub(crate) struct OledPins {
    cs: P8,
    dc: P6,
    reset: P7,
}

/// The OLED control lines, as GPIO outputs.
pub(crate) struct OledLines {
    /// Chip select, active low.
    pub(crate) cs: Output,
    /// Data / command select: low = command, high = data.
    pub(crate) dc: Output,
    /// Reset, active low.
    pub(crate) reset: Output,
}

impl OledPins {
    /// Claims the three pads as GPIO outputs.
    pub(crate) fn claim(self, gpio2: &mut Port) -> OledLines {
        OledLines {
            cs: gpio2.output(self.cs).expect("P8 is a GPIO2 pin"),
            dc: gpio2.output(self.dc).expect("P6 is a GPIO2 pin"),
            reset: gpio2.output(self.reset).expect("P7 is a GPIO2 pin"),
        }
    }
}

/// The encoder and button pads.
pub(crate) struct PanelPins {
    left_a: P22,
    left_b: P21,
    right_a: P16,
    right_b: P15,
    left_switch: P23,
    right_switch: P14,
    up: P5,
    down: P4,
}

/// The panel inputs, ready for `oc_drivers::panel::Panel::new`.
pub(crate) struct PanelLines {
    /// Left then right encoder quadrature lines.
    pub(crate) encoders: [EncoderPins<Input>; ENCODERS],
    /// Buttons in `oc_core::platform::Button` order: left switch, right
    /// switch, `up`, `down`.
    pub(crate) buttons: [Input; BUTTONS],
}

impl PanelPins {
    /// Claims every panel pad as a pulled-up GPIO input.
    pub(crate) fn claim(self, gpio1: &mut Port, gpio4: &mut Port) -> PanelLines {
        PanelLines {
            encoders: [
                EncoderPins {
                    line_a: gpio1
                        .input(with_pullup(self.left_a))
                        .expect("P22 is a GPIO1 pin"),
                    line_b: gpio1
                        .input(with_pullup(self.left_b))
                        .expect("P21 is a GPIO1 pin"),
                },
                EncoderPins {
                    line_a: gpio1
                        .input(with_pullup(self.right_a))
                        .expect("P16 is a GPIO1 pin"),
                    line_b: gpio1
                        .input(with_pullup(self.right_b))
                        .expect("P15 is a GPIO1 pin"),
                },
            ],
            buttons: [
                gpio1
                    .input(with_pullup(self.left_switch))
                    .expect("P23 is a GPIO1 pin"),
                gpio1
                    .input(with_pullup(self.right_switch))
                    .expect("P14 is a GPIO1 pin"),
                gpio4
                    .input(with_pullup(self.up))
                    .expect("P5 is a GPIO4 pin"),
                gpio4
                    .input(with_pullup(self.down))
                    .expect("P4 is a GPIO4 pin"),
            ],
        }
    }
}

/// Bit offset of the onboard LED within GPIO2.
///
/// Teensy pin 13 is `GPIO_B0_03` = `GPIO2_IO03`, which is also LPSPI4 SCK.
/// Boot breadcrumbs may drive this pad as GPIO **before** the SPI bus claims
/// it; afterwards only a panic handler (register-level SOS) may touch it.
pub(crate) const LED_GPIO2_OFFSET: u32 = 3;

/// ADC channel of each CV input, in `CV1..CV4` order.
///
/// The firmware never uses these numbers to address the converter: they are
/// resolved from the pad types by `imxrt-iomuxc`. They are recorded here so that
/// the table above can be checked against the Teensy documentation by eye, and
/// asserted below so that the two cannot silently disagree.
pub(crate) const CV_ADC_CHANNELS: [u32; CV_CHANNELS] = [5, 6, 15, 11];

/// Compile-time sanity checks on the board data.
///
/// These are cheap and catch a copy-and-paste slip in the constants above
/// before it reaches a module.
const _: () = {
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
