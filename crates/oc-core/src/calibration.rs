//! Conversions between raw converter codes and millivolts.
//!
//! Every backend hands the core raw converter codes and receives raw codes
//! back; the conversion lives here, once, so that a calibration measured on
//! the module applies the same way everywhere (journal/D-027). The model is
//! the reference firmware's (Phazerville, `OC_calibration.{h,cpp}`,
//! `OC_ADC.h`, `OC_DAC.h`):
//!
//! * a CV input is affine, with its own zero code and a *signed* slope;
//! * a CV output is a table of one DAC code per octave from −3 V to +7 V,
//!   interpolated linearly in between.
//!
//! Both analog stages of the module **invert**: the reference computes
//! `offset - raw` for inputs and writes `MAX_VALUE - value` to the DAC. Here
//! that is data, not code: a negative input slope and a decreasing output
//! table. The simulator and VCV Rack model the module's converters with the
//! same factory calibration, so their codes behave like the hardware's.
//!
//! Slopes are stored with sub-code resolution and all arithmetic is integer,
//! clamped and overflow-free: a converter can never make the core panic.

// Every narrowing cast below is immediately preceded by a clamp into the
// destination range, and `const fn` cannot use `TryFrom`, so the fallible
// conversion lints have nothing to offer here beyond noise at each call site.
// The only `usize` casts are table indices below `OUTPUT_POINTS` (11), which
// cannot wrap.
#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_possible_wrap
)]

use serde::{Deserialize, Serialize};

use crate::platform::{CV_CHANNELS, MilliVolts};

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

/// Bound on any converted level, in millivolts: the Eurorack supply rails.
///
/// A calibration can be wrong, but no reading may claim more than the module
/// could physically see.
pub const CV_LIMIT_MV: MilliVolts = 12_000;

/// Lowest level the CV inputs read with the factory calibration (ADC code
/// 4095), in millivolts.
pub const CV_IN_MIN_MV: MilliVolts = CvInputCalibration::FACTORY.to_millivolts(ADC_CODE_MAX as u16);

/// Highest level the CV inputs read with the factory calibration (ADC code
/// 0), in millivolts.
pub const CV_IN_MAX_MV: MilliVolts = CvInputCalibration::FACTORY.to_millivolts(0);

/// Lowest level the CV outputs produce, in millivolts.
pub const CV_OUT_MIN_MV: MilliVolts = -3_000;

/// Highest level the CV outputs produce, in millivolts.
///
/// The reference table reaches +7 V, but the output stage saturates a little
/// above +6 V (the factory table's last octave is squeezed into the top
/// codes), so requests stop here.
pub const CV_OUT_MAX_MV: MilliVolts = 6_000;

/// Affine calibration of one CV input.
///
/// `millivolts = (code - zero_code) * nanovolts_per_code / 1_000_000`
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct CvInputCalibration {
    /// ADC code that reads as 0 V.
    pub zero_code: i32,
    /// Signed nanovolts per ADC code; negative when the front-end inverts.
    pub nanovolts_per_code: i32,
}

impl CvInputCalibration {
    /// Factory calibration of an Ornament & Crime CV input.
    ///
    /// Zero volts sits at two thirds of the ADC range, not at mid-scale
    /// (`_ADC_OFFSET`, "ADC offset @2.2V", `OC_calibration.h`), and the
    /// default pitch scale (`kDefaultPitchCVScale = SEMITONES << 7`,
    /// `OC_ADC.h`) spreads 10 V over the 4096 codes: 2.44 mV per code. The
    /// slope is negative because the front end inverts.
    pub const FACTORY: Self = Self {
        // `(uint16_t)(pow(2, 12) * 0.6666667f)`, truncated as the reference does.
        zero_code: (ADC_CODES as i64 * 6_666_667 / 10_000_000) as i32,
        nanovolts_per_code: -((10_000 * 1_000_000 / ADC_CODES as i64) as i32),
    };

    /// Converts an ADC code to millivolts, bounded by [`CV_LIMIT_MV`].
    #[must_use]
    pub const fn to_millivolts(self, code: u16) -> MilliVolts {
        let code = clamp_i32(code as i32, 0, ADC_CODE_MAX);
        let nanovolts = (code - self.zero_code) as i64 * self.nanovolts_per_code as i64;
        let millivolts = div_round_nearest(nanovolts, 1_000_000);
        clamp_i64(millivolts, -CV_LIMIT_MV as i64, CV_LIMIT_MV as i64) as MilliVolts
    }

    /// Converts millivolts back to the ADC code that would produce them.
    ///
    /// The simulator and the VCV Rack module use this to model the module's
    /// converter, so that their inputs flow through the exact same conversion
    /// path as the hardware's.
    #[must_use]
    pub const fn to_code(self, millivolts: MilliVolts) -> u16 {
        if self.nanovolts_per_code == 0 {
            return 0;
        }
        let nanovolts = millivolts as i64 * 1_000_000;
        let offset = div_round_nearest(nanovolts, self.nanovolts_per_code as i64);
        clamp_i64(self.zero_code as i64 + offset, 0, ADC_CODE_MAX as i64) as u16
    }

    /// Size of one ADC step in millivolts, rounded, never zero.
    #[must_use]
    pub const fn step_millivolts(self) -> MilliVolts {
        let step = div_round_nearest(self.nanovolts_per_code.unsigned_abs() as i64, 1_000_000);
        if step == 0 { 1 } else { step as MilliVolts }
    }
}

/// Number of points in an output calibration table: one per octave from
/// [`CvOutputCalibration::LOWEST_MV`] to +7 V (`OCTAVES + 1` in the
/// reference).
pub const OUTPUT_POINTS: usize = 11;

/// Calibration of one CV output: the DAC code for each whole volt.
///
/// Point `n` is the code that produces `LOWEST_MV + n` volts; levels in
/// between are interpolated linearly, as `DAC::PitchToScaledDAC` does in the
/// reference. On the module the table *decreases*, because the output stage
/// inverts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct CvOutputCalibration {
    /// DAC code of each point, lowest voltage first.
    pub codes: [u16; OUTPUT_POINTS],
}

impl CvOutputCalibration {
    /// Level of the first point, in millivolts (`kOctaveZero = 3` octaves
    /// below 0 V in the reference).
    pub const LOWEST_MV: MilliVolts = -3_000;

    /// Millivolts between two points.
    pub const POINT_SPACING_MV: MilliVolts = 1_000;

    /// Index of the 0 V point.
    pub const ZERO_POINT: usize = 3;

    /// Factory calibration of an Ornament & Crime CV output.
    ///
    /// The reference's default table (`kCalibrationDefaults`, `0, 6553, …,
    /// 65535`), shifted by its `DAC_OFFSET` of 4890 on every point but the
    /// last, then inverted as its DAC driver does (`kMaxValue - data`): 10 V
    /// over the full DAC range, 0 V at code 40984.
    pub const FACTORY: Self = {
        const REFERENCE: [i32; OUTPUT_POINTS] = [
            0, 6_553, 13_107, 19_661, 26_214, 32_768, 39_321, 45_875, 52_428, 58_981, 65_535,
        ];
        const DAC_OFFSET: i32 = 4_890;
        let mut codes = [0; OUTPUT_POINTS];
        let mut index = 0;
        while index < OUTPUT_POINTS {
            let offset = if index + 1 < OUTPUT_POINTS {
                DAC_OFFSET
            } else {
                0
            };
            let logical = clamp_i32(REFERENCE[index] + offset, 0, DAC_CODE_MAX);
            codes[index] = (DAC_CODE_MAX - logical) as u16;
            index += 1;
        }
        Self { codes }
    };

    /// A straight-line table: `zero_code` at 0 V, `millicodes_per_volt`
    /// thousandths of a code per volt (negative for an inverting stage),
    /// clamped to the DAC range.
    #[must_use]
    pub const fn linear(zero_code: i32, millicodes_per_volt: i32) -> Self {
        let mut codes = [0; OUTPUT_POINTS];
        let mut index = 0;
        while index < OUTPUT_POINTS {
            let volts = index as i64 - Self::ZERO_POINT as i64;
            let code =
                zero_code as i64 + div_round_nearest(volts * millicodes_per_volt as i64, 1_000);
            codes[index] = clamp_i64(code, 0, DAC_CODE_MAX as i64) as u16;
            index += 1;
        }
        Self { codes }
    }

    /// Converts millivolts to a DAC code, clamped to
    /// [`CV_OUT_MIN_MV`]..=[`CV_OUT_MAX_MV`].
    #[must_use]
    pub const fn to_code(self, millivolts: MilliVolts) -> u16 {
        let millivolts = clamp_i32(millivolts, CV_OUT_MIN_MV, CV_OUT_MAX_MV);
        let position = millivolts - Self::LOWEST_MV;
        let mut index = (position / Self::POINT_SPACING_MV) as usize;
        if index + 1 >= OUTPUT_POINTS {
            index = OUTPUT_POINTS - 2;
        }
        let fraction = position - index as i32 * Self::POINT_SPACING_MV;
        let low = self.codes[index] as i64;
        let high = self.codes[index + 1] as i64;
        let code = low
            + div_round_nearest(
                (high - low) * fraction as i64,
                Self::POINT_SPACING_MV as i64,
            );
        clamp_i64(code, 0, DAC_CODE_MAX as i64) as u16
    }

    /// Converts a DAC code back to the level it produces, clamped to
    /// [`CV_OUT_MIN_MV`]..=[`CV_OUT_MAX_MV`].
    ///
    /// The simulator and the VCV Rack module use this to model the module's
    /// output stage. A code outside the table reads as the nearer end.
    #[must_use]
    pub const fn to_millivolts(self, code: u16) -> MilliVolts {
        let code = code as i64;
        let mut index = 0;
        while index + 1 < OUTPUT_POINTS {
            let low = self.codes[index] as i64;
            let high = self.codes[index + 1] as i64;
            let inside = (low <= code && code <= high) || (high <= code && code <= low);
            if inside && low != high {
                let base = Self::LOWEST_MV as i64 + index as i64 * Self::POINT_SPACING_MV as i64;
                let offset =
                    div_round_nearest((code - low) * Self::POINT_SPACING_MV as i64, high - low);
                return clamp_i64(base + offset, CV_OUT_MIN_MV as i64, CV_OUT_MAX_MV as i64)
                    as MilliVolts;
            }
            index += 1;
        }
        let first = self.codes[0] as i64;
        let last = self.codes[OUTPUT_POINTS - 1] as i64;
        if (code - first).abs() <= (code - last).abs() {
            CV_OUT_MIN_MV
        } else {
            CV_OUT_MAX_MV
        }
    }

    /// Largest error, in millivolts, of one DAC step anywhere in the usable
    /// range; never zero.
    #[must_use]
    pub const fn step_millivolts(self) -> MilliVolts {
        let mut worst = 1;
        let mut index = 0;
        while index + 1 < OUTPUT_POINTS {
            let span = (self.codes[index + 1] as i32 - self.codes[index] as i32).abs();
            let step = if span == 0 {
                Self::POINT_SPACING_MV
            } else {
                (Self::POINT_SPACING_MV + span - 1) / span
            };
            if step > worst {
                worst = step;
            }
            index += 1;
        }
        worst
    }
}

/// The calibration of the whole module: every input and every output.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Calibration {
    /// One calibration per CV input, `CV1..CV4`.
    pub inputs: [CvInputCalibration; CV_CHANNELS],
    /// One calibration per CV output, `A..D`.
    pub outputs: [CvOutputCalibration; CV_CHANNELS],
}

impl Calibration {
    /// The reference firmware's defaults on every channel: what every backend
    /// starts from, and what the module falls back to until it is calibrated.
    pub const FACTORY: Self = Self {
        inputs: [CvInputCalibration::FACTORY; CV_CHANNELS],
        outputs: [CvOutputCalibration::FACTORY; CV_CHANNELS],
    };
}

impl Default for Calibration {
    fn default() -> Self {
        Self::FACTORY
    }
}

/// Compile-time checks on the factory data: a slip here would reach every
/// backend, so it must not build.
const _: () = {
    assert!(
        CvInputCalibration::FACTORY.nanovolts_per_code < 0,
        "the O&C analog front end inverts; the input slope must be negative"
    );
    assert!(
        CvInputCalibration::FACTORY.zero_code == 2_730,
        "the reference's 0 V input code is (uint16_t)(4096 * 0.6666667f)"
    );
    assert!(
        CV_IN_MIN_MV < 0 && CV_IN_MAX_MV > 0,
        "0 V must be inside the input range"
    );
    let codes = CvOutputCalibration::FACTORY.codes;
    let mut index = 0;
    while index + 1 < OUTPUT_POINTS {
        assert!(
            codes[index] > codes[index + 1],
            "the O&C output stage inverts; the output table must decrease"
        );
        index += 1;
    }
    assert!(
        codes[CvOutputCalibration::ZERO_POINT] == 40_984,
        "the reference's 0 V output code, inverted"
    );
};

/// Divides rounding to nearest, ties away from zero.
pub(crate) const fn div_round_nearest(numerator: i64, denominator: i64) -> i64 {
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
        ADC_CODE_MAX, CV_IN_MAX_MV, CV_IN_MIN_MV, CV_LIMIT_MV, CV_OUT_MAX_MV, CV_OUT_MIN_MV,
        CvInputCalibration, CvOutputCalibration, DAC_CODE_MAX, OUTPUT_POINTS,
    };

    const CV_IN: CvInputCalibration = CvInputCalibration::FACTORY;
    const CV_OUT: CvOutputCalibration = CvOutputCalibration::FACTORY;

    #[test]
    fn input_zero_volts_is_at_two_thirds_of_the_range() {
        assert_eq!(CV_IN.to_millivolts(2_730), 0);
        assert_eq!(CV_IN.to_code(0), 2_730);
    }

    #[test]
    fn input_factory_range_matches_the_reference() {
        // 10 V over 4096 codes, 0 V at code 2730: about -3.33 V to +6.67 V.
        assert_eq!(CV_IN_MIN_MV, -3_333);
        assert_eq!(CV_IN_MAX_MV, 6_665);
        assert!(CV_IN.to_millivolts(0) > CV_IN.to_millivolts(4_095));
    }

    #[test]
    fn input_saturates_instead_of_wrapping() {
        assert_eq!(CV_IN.to_millivolts(u16::MAX), CV_IN.to_millivolts(4095));
        assert_eq!(CV_IN.to_code(i32::MAX), 0);
        assert_eq!(
            CV_IN.to_code(i32::MIN),
            u16::try_from(ADC_CODE_MAX).unwrap()
        );
    }

    #[test]
    fn a_wild_input_calibration_stays_within_the_rails() {
        let wild = CvInputCalibration {
            zero_code: 0,
            nanovolts_per_code: i32::MAX,
        };
        assert_eq!(wild.to_millivolts(4_095), CV_LIMIT_MV);
    }

    #[test]
    fn known_input_levels_survive_a_round_trip() {
        let tolerance = CV_IN.step_millivolts();
        for expected in [-3_000, -1_000, 0, 1_000, 2_500, 5_000, 6_500] {
            let code = CV_IN.to_code(expected);
            let actual = CV_IN.to_millivolts(code);
            assert!(
                (actual - expected).abs() <= tolerance,
                "{expected} mV -> code {code} -> {actual} mV, tolerance {tolerance}"
            );
        }
    }

    #[test]
    fn output_factory_table_matches_the_reference() {
        assert_eq!(
            CV_OUT.codes,
            [
                60_645, 54_092, 47_538, 40_984, 34_431, 27_877, 21_324, 14_770, 8_217, 1_664, 0
            ]
        );
        assert_eq!(CV_OUT.to_code(0), 40_984);
        assert_eq!(CV_OUT.to_millivolts(40_984), 0);
    }

    #[test]
    fn output_interpolates_between_points() {
        // Halfway between the 0 V and +1 V points.
        let expected = i32::midpoint(40_984, 34_431);
        assert!((i32::from(CV_OUT.to_code(500)) - expected).abs() <= 1);
    }

    #[test]
    fn output_clamps_out_of_range_requests() {
        assert_eq!(
            CV_OUT.to_code(CV_OUT_MAX_MV + 10_000),
            CV_OUT.to_code(CV_OUT_MAX_MV)
        );
        assert_eq!(CV_OUT.to_code(CV_OUT_MIN_MV - 10_000), CV_OUT.codes[0]);
        assert_eq!(CV_OUT.to_code(i32::MAX), CV_OUT.to_code(CV_OUT_MAX_MV));
        assert_eq!(CV_OUT.to_code(i32::MIN), CV_OUT.codes[0]);
    }

    #[test]
    fn output_codes_beyond_the_table_read_as_its_ends() {
        assert_eq!(CV_OUT.to_millivolts(u16::MAX), CV_OUT_MIN_MV);
        assert_eq!(CV_OUT.to_millivolts(0), CV_OUT_MAX_MV);
    }

    #[test]
    fn a_linear_table_supports_both_polarities() {
        let rising = CvOutputCalibration::linear(20_000, 6_553_500);
        let falling = CvOutputCalibration::linear(40_000, -6_553_500);
        assert!(rising.to_code(CV_OUT_MAX_MV) > rising.to_code(CV_OUT_MIN_MV));
        assert!(falling.to_code(CV_OUT_MAX_MV) < falling.to_code(CV_OUT_MIN_MV));
        for calibration in [rising, falling] {
            assert_eq!(calibration.to_millivolts(calibration.to_code(1_500)), 1_500);
        }
    }

    #[test]
    fn one_dac_step_is_well_under_a_millivolt_of_error() {
        assert_eq!(CV_OUT.step_millivolts(), 1);
        assert!(CV_IN.step_millivolts() <= 3);
    }

    #[test]
    fn degenerate_calibrations_do_not_divide_by_zero() {
        let flat_in = CvInputCalibration {
            zero_code: 0,
            nanovolts_per_code: 0,
        };
        assert_eq!(flat_in.to_code(1_000), 0);
        assert_eq!(flat_in.to_millivolts(1_000), 0);
        assert_eq!(flat_in.step_millivolts(), 1);

        let flat_out = CvOutputCalibration {
            codes: [1_234; OUTPUT_POINTS],
        };
        assert_eq!(flat_out.to_code(2_000), 1_234);
        assert_eq!(flat_out.to_millivolts(1_234), CV_OUT_MIN_MV);
        assert_eq!(flat_out.step_millivolts(), 1_000);
        assert!(i32::from(flat_out.to_code(0)) <= DAC_CODE_MAX);
    }
}
