//! The four CV inputs, read through ADC1.
//!
//! All four Ornament & Crime CV pads (Teensy pins 19, 18, 20 and 17) are wired
//! to `GPIO_AD_B1_*` pads, which `imxrt-iomuxc` exposes on both ADC1 and ADC2.
//! Using a single bank keeps the four readings in a predictable order at the
//! cost of serialising them, which is affordable: four blocking 12-bit
//! conversions cost a few microseconds out of the 1000 the tick has.
//!
//! The channel numbers are resolved from the pad *types*, never hand-written,
//! which removes the classic "the inputs are shuffled" failure mode.

use teensy4_bsp::hal::adc::{Adc, AnalogInput, AveragingCount, ConversionSpeed, ResolutionBits};

use oc_core::calibration::CvInputCalibration;
use oc_core::platform::{AnalogIn, CV_CHANNELS, CvChannel, MilliVolts};

/// The four CV inputs.
pub(crate) struct CvInputs {
    adc: Adc,
    inputs: [AnalogInput; CV_CHANNELS],
    calibration: [CvInputCalibration; CV_CHANNELS],
    raw: [u16; CV_CHANNELS],
}

impl CvInputs {
    /// Wraps an ADC bank and the four analog inputs, in `CV1..CV4` order.
    ///
    /// The bank is configured for 12-bit conversions with four hardware
    /// averages: the extra averaging costs a little time but visibly steadies
    /// the reading, and the module's own noise floor is well above one code.
    pub(crate) fn new(
        mut adc: Adc,
        inputs: [AnalogInput; CV_CHANNELS],
        calibration: CvInputCalibration,
    ) -> Self {
        adc.set_resolution(ResolutionBits::Res12);
        adc.set_averaging(AveragingCount::Avg4);
        adc.set_conversion_speed(ConversionSpeed::Medium);

        Self {
            adc,
            inputs,
            calibration: [calibration; CV_CHANNELS],
            raw: [0; CV_CHANNELS],
        }
    }

    /// Converts all four channels, so the tick sees one coherent snapshot.
    pub(crate) fn sample(&mut self) {
        for (index, input) in self.inputs.iter_mut().enumerate() {
            self.raw[index] = self.adc.read_blocking(input);
        }
    }
}

impl AnalogIn for CvInputs {
    fn read_cv(&mut self, channel: CvChannel) -> MilliVolts {
        let index = channel.index();
        self.calibration[index].to_millivolts(self.raw[index])
    }

    fn is_patched(&self, _channel: CvChannel) -> bool {
        // The module's jacks have no cable-detection switch, so the honest
        // answer is "always connected"; the applet relies on
        // `oc_core::signal::SignalDetector` to tell a live input from a dead
        // one.
        true
    }
}

impl core::fmt::Debug for CvInputs {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("CvInputs")
            .field("raw", &self.raw)
            .finish_non_exhaustive()
    }
}
