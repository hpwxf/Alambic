//! Base-2 logarithm and exponential in 16.16 fixed point, for the curves and
//! scales applets need without floating point in `oc-core` (journal/D-005,
//! D-032).
//!
//! A value `v` in 16.16 stands for `v / 65536`. Both functions interpolate
//! linearly in a 65-entry table over one octave; the error stays within a few
//! units of the last place (checked against `f64` in `tests/fixed.rs`), far
//! below what a millisecond interval or a millivolt can show.

/// One, in 16.16.
pub const ONE: i32 = 1 << 16;

/// `log2(1 + i / 64)` in 16.16, for `i` in `0..=64`.
const LOG2: [i32; 65] = [
    0, 1466, 2909, 4331, 5732, 7112, 8473, 9814, 11136, 12440, 13727, 14996, 16248, 17484, 18704,
    19909, 21098, 22272, 23433, 24579, 25711, 26830, 27936, 29029, 30109, 31178, 32234, 33279,
    34312, 35334, 36346, 37346, 38336, 39316, 40286, 41246, 42196, 43137, 44068, 44990, 45904,
    46809, 47705, 48593, 49472, 50344, 51207, 52063, 52911, 53751, 54584, 55410, 56229, 57040,
    57845, 58643, 59434, 60219, 60997, 61769, 62534, 63294, 64047, 64794, 65536,
];

/// `2^(i / 64)` in 16.16, for `i` in `0..=64`.
const EXP2: [i64; 65] = [
    65536, 66250, 66971, 67700, 68438, 69183, 69936, 70698, 71468, 72246, 73032, 73828, 74632,
    75444, 76266, 77096, 77936, 78785, 79642, 80510, 81386, 82273, 83169, 84074, 84990, 85915,
    86851, 87796, 88752, 89719, 90696, 91684, 92682, 93691, 94711, 95743, 96785, 97839, 98905,
    99982, 101_070, 102_171, 103_283, 104_408, 105_545, 106_694, 107_856, 109_031, 110_218,
    111_418, 112_631, 113_858, 115_098, 116_351, 117_618, 118_899, 120_194, 121_502, 122_825,
    124_163, 125_515, 126_882, 128_263, 129_660, 131_072,
];

/// Interpolates `table` at `position`, a 16.16 fraction of the 64 steps.
fn interpolate<T: Copy + Into<i64>>(table: &[T; 65], position: u32) -> i64 {
    // `position` in 0..65536 covers the 64 steps: 1024 per step.
    let index = usize::try_from(position >> 10).unwrap_or(0).min(63);
    let within = i64::from(position & 0x3FF);
    let low = table[index].into();
    let high = table[index + 1].into();
    low + (high - low) * within / 1024
}

/// `log2(x)` in 16.16, `x` in 16.16; `None` for zero or less.
#[must_use]
pub fn log2(x: i64) -> Option<i64> {
    if x <= 0 {
        return None;
    }
    // x = 2^e * m, m in [1, 2) as 16.16.
    let exponent = 63 - i64::from(x.leading_zeros()) - 16;
    let mantissa = if exponent >= 0 {
        x >> exponent
    } else {
        x << -exponent
    };
    let fraction = u32::try_from(mantissa - i64::from(ONE)).unwrap_or(0);
    Some(exponent * i64::from(ONE) + interpolate(&LOG2, fraction))
}

/// `2^y` in 16.16, `y` in 16.16; saturates instead of overflowing, and rounds
/// what falls below the last place to zero.
#[must_use]
pub fn exp2(y: i64) -> i64 {
    let whole = y.div_euclid(i64::from(ONE));
    let fraction = u32::try_from(y.rem_euclid(i64::from(ONE))).unwrap_or(0);
    let mantissa = interpolate(&EXP2, fraction);
    match whole {
        // 2^31 in 16.16 is past any value an applet uses.
        31.. => i64::MAX >> 1,
        0..=30 => mantissa << whole,
        -48..=-1 => mantissa >> -whole,
        _ => 0,
    }
}

#[cfg(test)]
mod tests {
    use super::{ONE, exp2, log2};

    #[test]
    fn powers_of_two_are_exact() {
        let one = i64::from(ONE);
        assert_eq!(log2(one), Some(0));
        assert_eq!(log2(2 * one), Some(one));
        assert_eq!(log2(one / 4), Some(-2 * one));
        assert_eq!(exp2(0), one);
        assert_eq!(exp2(3 * one), 8 * one);
        assert_eq!(exp2(-one), one / 2);
        assert_eq!(log2(0), None);
    }

    #[test]
    fn exp2_undoes_log2() {
        let one = i64::from(ONE);
        for x in [one / 3, one, 3 * one / 2, 5 * one, 1000 * one] {
            let back = exp2(log2(x).unwrap());
            assert!((back - x).abs() * 1000 <= x, "{x} came back as {back}");
        }
    }
}
