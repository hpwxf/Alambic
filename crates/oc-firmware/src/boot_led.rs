//! Onboard LED breadcrumbs for the earliest boot stages.
//!
//! The Teensy 4.0 LED shares its pad with LPSPI4 SCK (see the pinout table in
//! [`crate::board`]). This module therefore **never takes ownership** of the
//! pin token: it only reconfigures the pad as GPIO long enough to flash a
//! stage code, then returns. Once the shared SPI bus is brought up the pad is
//! muxed to SCK and the LED must not be touched again — a later panic still
//! recovers it through `teensy4-panic`'s register-level SOS routine.
//!
//! Stage codes (count the flashes, then a longer gap):
//!
//! | Flashes | Meaning                          |
//! |--------:|----------------------------------|
//! |       1 | `main` reached, USB log starting |
//! |       2 | ADC inputs mapped                |
//! |       3 | Triggers mapped; about to take SPI |

use embedded_hal::delay::DelayNs;
use teensy4_bsp::hal::{gpio, iomuxc};
use teensy4_bsp::pins::common::P13;

use crate::board;

/// On duration of a single flash.
const FLASH_ON_MS: u32 = 60;
/// Off duration between flashes of the same stage.
const FLASH_OFF_MS: u32 = 60;
/// Quiet gap after a stage so the human eye can separate groups.
const STAGE_GAP_MS: u32 = 250;

/// Drive the onboard LED for `flashes` short pulses, then pause.
///
/// `pin` is borrowed, not consumed: the caller keeps the token for the later
/// SPI setup, which will switch the mux away from GPIO.
pub(crate) fn signal(gpio2: &mut gpio::Port, pin: &mut P13, delay: &mut impl DelayNs, flashes: u8) {
    // Route the pad to GPIO2 before poking the data register. SPI init later
    // calls `lpspi::prepare` on the same pad and overrides this alternate.
    iomuxc::gpio::prepare(pin);
    let led = gpio::Output::without_pin(gpio2, board::LED_GPIO2_OFFSET);

    for i in 0..flashes {
        led.set();
        delay.delay_ms(FLASH_ON_MS);
        led.clear();
        if i + 1 < flashes {
            delay.delay_ms(FLASH_OFF_MS);
        }
    }
    delay.delay_ms(STAGE_GAP_MS);
}
