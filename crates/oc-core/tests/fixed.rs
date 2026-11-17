//! The 16.16 `log2` and `exp2` of `oc_core::fixed` against the floating-point
//! ones, which only the host tests may use (journal/D-005, D-032).

use oc_core::fixed::{ONE, exp2, log2};

/// A 16.16 value as a float; every value here fits an `i32`, so exactly.
fn float(value: i64) -> f64 {
    f64::from(i32::try_from(value).expect("within i32"))
}

#[test]
fn log2_is_within_a_few_units_of_the_real_one() {
    let one = f64::from(ONE);
    for raw in (1..2_000_000_i64).step_by(997) {
        let expected = (float(raw) / one).log2() * one;
        let ours = float(log2(raw).unwrap());
        assert!(
            (ours - expected).abs() <= 8.0,
            "log2({raw}): {ours} for {expected}"
        );
    }
}

#[test]
fn exp2_is_within_a_thousandth_of_the_real_one() {
    let one = f64::from(ONE);
    for raw in (-8 * i64::from(ONE)..8 * i64::from(ONE)).step_by(1_009) {
        let expected = (float(raw) / one).exp2() * one;
        let ours = float(exp2(raw));
        assert!(
            (ours - expected).abs() <= expected / 1_000.0 + 1.0,
            "exp2({raw}): {ours} for {expected}"
        );
    }
}
