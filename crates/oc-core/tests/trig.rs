//! The integer sine of `oc_core::gfx::trig` against the floating-point one,
//! which only the host tests may use (journal/D-005, D-030).

use oc_core::gfx::trig::{ONE, TURN, cos, sin};

#[test]
fn the_integer_sine_is_within_one_step_of_the_real_one() {
    for angle in -TURN..2 * TURN {
        let radians = f64::from(angle) * std::f64::consts::TAU / f64::from(TURN);
        for (ours, real) in [(sin(angle), radians.sin()), (cos(angle), radians.cos())] {
            let expected = real * f64::from(ONE);
            assert!(
                (f64::from(ours) - expected).abs() <= 1.0,
                "angle {angle}: {ours} for {expected}"
            );
        }
    }
}
