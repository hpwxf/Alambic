//! Conversions between raw converter codes and millivolts.
//!
//! Both directions are affine with a *signed* slope, because the Ornament &
//! Crime analog front-end and output stage may invert. Keeping the sign in the
//! calibration data means the same code serves an inverting board, a
//! non-inverting board and the simulator.
//!
//! Slopes are stored with sub-code resolution (nanovolts per ADC code,
//! thousandths of a DAC code per volt) so that truncating the slope to an
//! integer stays well under one converter step across the whole range. All
//! arithmetic is integer, clamped and overflow-free: a converter can never make
//! the core panic.

// Every narrowing cast below is immediately preceded by a clamp into the
// destination range, and `const fn` cannot use `TryFrom`, so the fallible
// conversion lints have nothing to offer here beyond noise at each call site.
#![allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]

use crate::platform::MilliVolts;

/// Resolution of the CV input converter (i.MX RT ADC configured for 12 bits).
pub const ADC_BITS: u32 = 12;

/// Number of distinct ADC codes.
pub const ADC_CODES: i32 = 1 << ADC_BITS;

/// Highest ADC code.
pub const ADC_CODE_MAX: i32 = ADC_CODES - 1;

/// Resolution of the DAC8565.
pub const DAC_BITS: u32 = 16;

/// Number of distinct DAC codes.
pub const DAC_CODES: i32 = 1 << DAC_BITS;

/// Highest DAC code.
pub const DAC_CODE_MAX: i32 = DAC_CODES - 1;

/// Lowest level the CV inputs accept, in millivolts.
pub const CV_IN_MIN_MV: MilliVolts = -5_000;

/// Highest level the CV inputs accept, in millivolts.
pub const CV_IN_MAX_MV: MilliVolts = 5_000;

/// Lowest level the CV outputs can produce, in millivolts.
pub const CV_OUT_MIN_MV: MilliVolts = -3_000;

/// Highest level the CV outputs can produce, in millivolts.
pub const CV_OUT_MAX_MV: MilliVolts = 6_000;

/// Affine calibration of one CV input.
///
/// `millivolts = (code - zero_code) * nanovolts_per_code / 1_000_000`
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CvInputCalibration {
    /// ADC code that reads as 0 V.
    pub zero_code: i32,
    /// Signed nanovolts per ADC code; negative when the front-end inverts.
    pub nanovolts_per_code: i32,
}

impl CvInputCalibration {
    /// Nominal calibration: [`CV_IN_MIN_MV`]..=[`CV_IN_MAX_MV`] spread over the
    /// full ADC range, non-inverting, zero at mid-scale.
    ///
    /// The real module's slope sign and offset are board properties, installed
    /// by `oc-firmware`'s board module once measured. This default exists so
    /// that the simulator, the tests and the VCV Rack module all agree on one
    /// intuitive mapping.
    pub const NOMINAL: Self = Self {
        zero_code: ADC_CODES / 2,
        nanovolts_per_code: ((CV_IN_MAX_MV - CV_IN_MIN_MV) as i64 * 1_000_000 / ADC_CODES as i64)
            as i32,
    };

    /// Converts an ADC code to millivolts, clamped to the input range.
    #[must_use]
    pub const fn to_millivolts(self, code: u16) -> MilliVolts {
        let code = clamp_i32(code as i32, 0, ADC_CODE_MAX);
        let nanovolts = (code - self.zero_code) as i64 * self.nanovolts_per_code as i64;
        let millivolts = div_round_nearest(nanovolts, 1_000_000);
        clamp_i64(millivolts, CV_IN_MIN_MV as i64, CV_IN_MAX_MV as i64) as MilliVolts
    }

    /// Converts millivolts back to the ADC code that would produce them.
    ///
    /// The simulator and the VCV Rack module use this so that their inputs flow
    /// through the exact same conversion path as the hardware's.
    #[must_use]
    pub const fn to_code(self, millivolts: MilliVolts) -> u16 {
        if self.nanovolts_per_code == 0 {
            return 0;
        }
        let nanovolts = millivolts as i64 * 1_000_000;
        let offset = div_round_nearest(nanovolts, self.nanovolts_per_code as i64);
        clamp_i64(self.zero_code as i64 + offset, 0, ADC_CODE_MAX as i64) as u16
    }

    /// Size of one ADC step in millivolts, rounded up, never zero.
    #[must_use]
    pub const fn step_millivolts(self) -> MilliVolts {
        let step = div_round_nearest(self.nanovolts_per_code.unsigned_abs() as i64, 1_000_000);
        if step == 0 { 1 } else { step as MilliVolts }
    }
}

/// Affine calibration of one CV output.
///
/// `code = zero_code + millivolts * millicodes_per_volt / 1_000_000`
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CvOutputCalibration {
    /// DAC code that produces 0 V.
    pub zero_code: i32,
    /// Signed thousandths of a DAC code per volt; negative when the output
    /// stage inverts.
    pub millicodes_per_volt: i32,
}

impl CvOutputCalibration {
    /// Thousandths of a code per volt for the nominal, non-inverting mapping of
    /// [`CV_OUT_MIN_MV`]..=[`CV_OUT_MAX_MV`] onto the full DAC range.
    const NOMINAL_SLOPE: i32 =
        (DAC_CODE_MAX as i64 * 1_000_000 / (CV_OUT_MAX_MV - CV_OUT_MIN_MV) as i64) as i32;

    /// Nominal calibration; see [`CvInputCalibration::NOMINAL`] for why a
    /// nominal default exists at all.
    pub const NOMINAL: Self = Self {
        // Rounded, not truncated: truncating here would leave the top of the
        // range one converter code short of full scale.
        zero_code: div_round_nearest(
            -CV_OUT_MIN_MV as i64 * Self::NOMINAL_SLOPE as i64,
            1_000_000,
        ) as i32,
        millicodes_per_volt: Self::NOMINAL_SLOPE,
    };

    /// Converts millivolts to a DAC code, clamped to the converter range.
    #[must_use]
    pub const fn to_code(self, millivolts: MilliVolts) -> u16 {
        let millivolts = clamp_i32(millivolts, CV_OUT_MIN_MV, CV_OUT_MAX_MV);
        let scaled = millivolts as i64 * self.millicodes_per_volt as i64;
        let offset = div_round_nearest(scaled, 1_000_000);
        clamp_i64(self.zero_code as i64 + offset, 0, DAC_CODE_MAX as i64) as u16
    }

    /// Converts a DAC code back to the level it produces.
    #[must_use]
    pub const fn to_millivolts(self, code: u16) -> MilliVolts {
        if self.millicodes_per_volt == 0 {
            return 0;
        }
        let offset = (code as i32 - self.zero_code) as i64 * 1_000_000;
        let millivolts = div_round_nearest(offset, self.millicodes_per_volt as i64);
        clamp_i64(millivolts, CV_OUT_MIN_MV as i64, CV_OUT_MAX_MV as i64) as MilliVolts
    }

    /// Size of one DAC step in millivolts, rounded up, never zero.
    #[must_use]
    pub const fn step_millivolts(self) -> MilliVolts {
        let slope = self.millicodes_per_volt.unsigned_abs() as i64;
        if slope == 0 {
            return CV_OUT_MAX_MV;
        }
        // `slope` is thousandths of a code per volt, so one code spans
        // 1_000_000 / slope millivolts.
        let step = 1_000_000 / slope;
        if step == 0 { 1 } else { step as MilliVolts }
    }
}

/// Divides rounding to nearest, ties away from zero.
const fn div_round_nearest(numerator: i64, denominator: i64) -> i64 {
    debug_assert!(denominator != 0);
    let half = denominator.abs() / 2;
    if (numerator < 0) == (denominator < 0) {
        (numerator + half * denominator.signum()) / denominator
    } else {
        (numerator - half * denominator.signum()) / denominator
    }
}

/// `i32` clamp usable in `const fn`.
const fn clamp_i32(value: i32, low: i32, high: i32) -> i32 {
    if value < low {
        low
    } else if value > high {
        high
    } else {
        value
    }
}

/// `i64` clamp usable in `const fn`.
const fn clamp_i64(value: i64, low: i64, high: i64) -> i64 {
    if value < low {
        low
    } else if value > high {
        high
    } else {
        value
    }
}

#[cfg(test)]
mod tests {
    use super::{
        ADC_CODE_MAX, CV_IN_MAX_MV, CV_IN_MIN_MV, CV_OUT_MAX_MV, CV_OUT_MIN_MV, CvInputCalibration,
        CvOutputCalibration, DAC_CODE_MAX,
    };

    const CV_IN: CvInputCalibration = CvInputCalibration::NOMINAL;
    const CV_OUT: CvOutputCalibration = CvOutputCalibration::NOMINAL;

    #[test]
    fn input_midscale_is_zero_volts() {
        assert_eq!(CV_IN.to_millivolts(2048), 0);
    }

    #[test]
    fn input_endpoints_span_the_declared_range() {
        assert_eq!(CV_IN.to_millivolts(0), CV_IN_MIN_MV);
        let top = CV_IN.to_millivolts(u16::try_from(ADC_CODE_MAX).unwrap());
        assert!(
            (CV_IN_MAX_MV - top) <= CV_IN.step_millivolts(),
            "top code reads {top} mV, more than one step below {CV_IN_MAX_MV} mV"
        );
    }

    #[test]
    fn input_saturates_instead_of_wrapping() {
        assert_eq!(CV_IN.to_millivolts(u16::MAX), CV_IN.to_millivolts(4095));
        assert!(CV_IN.to_millivolts(u16::MAX) <= CV_IN_MAX_MV);
        assert_eq!(
            CV_IN.to_code(i32::MAX),
            u16::try_from(ADC_CODE_MAX).unwrap()
        );
        assert_eq!(CV_IN.to_code(i32::MIN), 0);
    }

    #[test]
    fn known_input_levels_survive_a_round_trip() {
        let tolerance = CV_IN.step_millivolts();
        for expected in [-5_000, -3_000, -1_000, 0, 1_000, 2_500, 5_000] {
            let code = CV_IN.to_code(expected);
            let actual = CV_IN.to_millivolts(code);
            assert!(
                (actual - expected).abs() <= tolerance,
                "{expected} mV -> code {code} -> {actual} mV, tolerance {tolerance}"
            );
        }
    }

    #[test]
    fn inverting_input_stage_is_supported() {
        let inverting = CvInputCalibration {
            zero_code: CV_IN.zero_code,
            nanovolts_per_code: -CV_IN.nanovolts_per_code,
        };
        assert!(inverting.to_millivolts(0) > inverting.to_millivolts(4095));
        assert_eq!(inverting.to_millivolts(inverting.to_code(0)), 0);
    }

    #[test]
    fn output_zero_volts_maps_to_the_zero_code() {
        assert_eq!(CV_OUT.to_code(0), u16::try_from(CV_OUT.zero_code).unwrap());
        assert_eq!(CV_OUT.to_millivolts(CV_OUT.to_code(0)), 0);
    }

    #[test]
    fn output_endpoints_reach_the_converter_limits() {
        assert_eq!(CV_OUT.to_code(CV_OUT_MIN_MV), 0);
        assert_eq!(
            CV_OUT.to_code(CV_OUT_MAX_MV),
            u16::try_from(DAC_CODE_MAX).unwrap()
        );
    }

    #[test]
    fn output_clamps_out_of_range_requests() {
        let max = u16::try_from(DAC_CODE_MAX).unwrap();
        assert_eq!(CV_OUT.to_code(CV_OUT_MAX_MV + 10_000), max);
        assert_eq!(CV_OUT.to_code(CV_OUT_MIN_MV - 10_000), 0);
        assert_eq!(CV_OUT.to_code(i32::MAX), max);
        assert_eq!(CV_OUT.to_code(i32::MIN), 0);
    }

    #[test]
    fn inverting_output_stage_is_supported() {
        let inverting = CvOutputCalibration {
            zero_code: DAC_CODE_MAX - CV_OUT.zero_code,
            millicodes_per_volt: -CV_OUT.millicodes_per_volt,
        };
        assert!(inverting.to_code(CV_OUT_MAX_MV) < inverting.to_code(CV_OUT_MIN_MV));
        assert_eq!(inverting.to_millivolts(inverting.to_code(1_500)), 1_500);
    }

    #[test]
    fn one_dac_step_is_well_under_a_millivolt_of_error() {
        assert_eq!(CV_OUT.step_millivolts(), 1);
        assert!(CV_IN.step_millivolts() <= 3);
    }

    #[test]
    fn degenerate_calibration_does_not_divide_by_zero() {
        let flat_in = CvInputCalibration {
            zero_code: 0,
            nanovolts_per_code: 0,
        };
        assert_eq!(flat_in.to_code(1_000), 0);
        assert_eq!(flat_in.to_millivolts(1_000), 0);
        assert_eq!(flat_in.step_millivolts(), 1);

        let flat_out = CvOutputCalibration {
            zero_code: 0,
            millicodes_per_volt: 0,
        };
        assert_eq!(flat_out.to_millivolts(1_000), 0);
        assert_eq!(flat_out.step_millivolts(), CV_OUT_MAX_MV);
    }
}
