//! Property-based tests for the unit conversions.
//!
//! Round-tripping millivolts through a converter code and back is the one
//! invariant the whole signal path depends on, so it is checked over the entire
//! input domain rather than at a handful of hand-picked points.

use proptest::prelude::*;

use oc_core::calibration::{
    ADC_CODE_MAX, CV_IN_MAX_MV, CV_IN_MIN_MV, CV_LIMIT_MV, CV_OUT_MAX_MV, CV_OUT_MIN_MV,
    CvInputCalibration, CvOutputCalibration, DAC_CODE_MAX,
};
use oc_core::debounce::EdgeCounter;
use oc_core::encoder::QuadratureDecoder;
use oc_core::framebuffer::{FrameBuffer, HEIGHT_I32, WIDTH_I32};

/// The factory input calibration, and a non-inverting counterpart.
fn input_calibrations() -> [CvInputCalibration; 2] {
    let factory = CvInputCalibration::FACTORY;
    [
        factory,
        CvInputCalibration {
            zero_code: 4_095 - factory.zero_code,
            nanovolts_per_code: -factory.nanovolts_per_code,
        },
    ]
}

/// The factory output table, and a non-inverting straight line.
fn output_calibrations() -> [CvOutputCalibration; 2] {
    [
        CvOutputCalibration::FACTORY,
        CvOutputCalibration::linear(24_551, 6_553_500),
    ]
}

proptest! {
    #[test]
    fn input_round_trip_stays_within_one_step(millivolts in CV_IN_MIN_MV..=CV_IN_MAX_MV) {
        for calibration in input_calibrations() {
            let code = calibration.to_code(millivolts);
            let recovered = calibration.to_millivolts(code);
            let tolerance = calibration.step_millivolts();
            prop_assert!(
                (recovered - millivolts).abs() <= tolerance,
                "{millivolts} mV -> {code} -> {recovered} mV exceeds {tolerance} mV"
            );
        }
    }

    #[test]
    fn output_round_trip_stays_within_one_step(millivolts in CV_OUT_MIN_MV..=CV_OUT_MAX_MV) {
        for calibration in output_calibrations() {
            let code = calibration.to_code(millivolts);
            let recovered = calibration.to_millivolts(code);
            let tolerance = calibration.step_millivolts();
            prop_assert!(
                (recovered - millivolts).abs() <= tolerance,
                "{millivolts} mV -> {code} -> {recovered} mV exceeds {tolerance} mV"
            );
        }
    }

    #[test]
    fn any_adc_code_yields_a_level_inside_the_factory_range(code in 0u16..=u16::MAX) {
        let level = CvInputCalibration::FACTORY.to_millivolts(code);
        prop_assert!((CV_IN_MIN_MV..=CV_IN_MAX_MV).contains(&level));
    }

    #[test]
    fn any_input_calibration_stays_within_the_rails(
        code: u16,
        zero_code in -10_000..10_000i32,
        nanovolts_per_code: i32,
    ) {
        let calibration = CvInputCalibration { zero_code, nanovolts_per_code };
        prop_assert!(calibration.to_millivolts(code).abs() <= CV_LIMIT_MV);
    }

    #[test]
    fn any_requested_level_yields_a_code_inside_the_converter_range(millivolts: i32) {
        for calibration in output_calibrations() {
            let code = i32::from(calibration.to_code(millivolts));
            prop_assert!((0..=DAC_CODE_MAX).contains(&code));
        }
        for calibration in input_calibrations() {
            let code = i32::from(calibration.to_code(millivolts));
            prop_assert!((0..=ADC_CODE_MAX).contains(&code));
        }
    }

    #[test]
    fn any_dac_code_reads_back_inside_the_output_range(code: u16) {
        for calibration in output_calibrations() {
            let level = calibration.to_millivolts(code);
            prop_assert!((CV_OUT_MIN_MV..=CV_OUT_MAX_MV).contains(&level));
        }
    }

    #[test]
    fn the_conversions_are_monotonic(
        low in CV_OUT_MIN_MV..CV_OUT_MAX_MV,
        span in 1..=1_000i32,
    ) {
        let high = (low + span).min(CV_OUT_MAX_MV);
        // Both factory stages invert: a higher level is a lower code.
        let input = CvInputCalibration::FACTORY;
        prop_assert!(input.to_code(low) >= input.to_code(high));
        let output = CvOutputCalibration::FACTORY;
        prop_assert!(output.to_code(low) >= output.to_code(high));
    }

    #[test]
    fn debouncing_never_counts_more_edges_than_there_are_samples(
        samples in proptest::collection::vec(any::<bool>(), 0..200),
    ) {
        let mut counter = EdgeCounter::default();
        for &sample in &samples {
            counter.update(sample);
        }
        let bound = u32::try_from(samples.len()).unwrap();
        prop_assert!(counter.rising_count() <= bound);
    }

    #[test]
    fn a_quadrature_decoder_never_drifts_on_random_input(
        samples in proptest::collection::vec((any::<bool>(), any::<bool>()), 0..500),
    ) {
        let mut decoder = QuadratureDecoder::new();
        let mut travelled = 0i32;
        for &(a, b) in &samples {
            travelled += i32::from(decoder.update(a, b));
        }
        // Every reported detent needs four valid transitions, so the total can
        // never exceed a quarter of the samples in either direction.
        let bound = i32::try_from(samples.len()).unwrap() / 4 + 1;
        prop_assert!(travelled.abs() <= bound, "drifted by {travelled}");
    }

    #[test]
    fn framebuffer_pixels_are_independent(
        x in 0..WIDTH_I32,
        y in 0..HEIGHT_I32,
    ) {
        let mut frame = FrameBuffer::new();
        frame.set_pixel(x, y, true);
        prop_assert!(frame.pixel(x, y));
        prop_assert_eq!(frame.lit_pixels(), 1, "setting one pixel must not light others");
        frame.set_pixel(x, y, false);
        prop_assert_eq!(frame.lit_pixels(), 0);
    }
}
