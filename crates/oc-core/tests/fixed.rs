//! The 16.16 `log2` and `exp2` of `oc_core::fixed` against the floating-point
//! ones, which only the host tests may use (journal/D-005, D-032).

use oc_core::fixed::{ONE, div_wide, exp2, log2};
use proptest::prelude::*;

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

#[test]
fn div_wide_is_the_u64_division_at_its_edges() {
    let divisors = [
        1,
        2,
        3,
        0xFFFF,
        0x1_0000,
        0x1_0001,
        0x7FFF_FFFF,
        0x8000_0000,
        0x8000_0001,
        u32::MAX - 1,
        u32::MAX,
    ];
    for divisor in divisors {
        let wide = u64::from(divisor);
        let numerators = [
            0,
            1,
            wide - 1,
            wide,
            1 << 32,
            1 << 48,
            (wide << 32) - 1,
            wide << 32,
            ((wide - 1) << 32) | 0xFFFF_FFFF,
            u64::MAX,
        ];
        for numerator in numerators {
            assert_eq!(
                div_wide(numerator, divisor),
                numerator / wide,
                "{numerator} / {divisor}"
            );
        }
    }
}

proptest! {
    #[test]
    fn div_wide_is_the_u64_division(numerator: u64, divisor in 1..=u32::MAX) {
        prop_assert_eq!(div_wide(numerator, divisor), numerator / u64::from(divisor));
    }

    #[test]
    fn div_wide_is_the_u64_division_with_a_32_bit_quotient(
        quotient: u32,
        divisor in 1..=u32::MAX,
        remainder: u32,
    ) {
        let wide = u64::from(divisor);
        let numerator = u64::from(quotient) * wide + u64::from(remainder) % wide;
        prop_assert_eq!(div_wide(numerator, divisor), u64::from(quotient));
    }
}
