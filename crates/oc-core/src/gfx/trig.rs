//! Integer trigonometry for drawing: no floating point in `oc-core`
//! (journal/D-005, D-030).
//!
//! Angles are in 1/256 of a turn, counter-clockwise from three o'clock as in
//! mathematics; screen rows grow downwards, which [`polar`] accounts for.

use super::Point;

/// One full turn.
pub const TURN: i32 = 256;

/// `sin` over a quarter turn, `0..=64`, scaled by 16384.
const QUARTER: [i32; 65] = [
    0, 402, 804, 1205, 1606, 2006, 2404, 2801, 3196, 3590, 3981, 4370, 4756, 5139, 5520, 5897,
    6270, 6639, 7005, 7366, 7723, 8076, 8423, 8765, 9102, 9434, 9760, 10080, 10394, 10702, 11003,
    11297, 11585, 11866, 12140, 12406, 12665, 12916, 13160, 13395, 13623, 13842, 14053, 14256,
    14449, 14635, 14811, 14978, 15137, 15286, 15426, 15557, 15679, 15791, 15893, 15986, 16069,
    16143, 16207, 16261, 16305, 16340, 16364, 16379, 16384,
];

/// Scale of [`sin`] and [`cos`]: their value for a quarter turn.
pub const ONE: i32 = 16384;

/// `sin(angle)` scaled by [`ONE`], `angle` in 1/256 turn.
#[must_use]
pub fn sin(angle: i32) -> i32 {
    let angle = angle.rem_euclid(TURN);
    let quarter = TURN / 4;
    let index = |offset: i32| usize::try_from(offset).unwrap_or(0);
    match angle / quarter {
        0 => QUARTER[index(angle)],
        1 => QUARTER[index(2 * quarter - angle)],
        2 => -QUARTER[index(angle - 2 * quarter)],
        _ => -QUARTER[index(TURN - angle)],
    }
}

/// `cos(angle)` scaled by [`ONE`], `angle` in 1/256 turn.
#[must_use]
pub fn cos(angle: i32) -> i32 {
    sin(angle + TURN / 4)
}

/// The point at `radius` from `center` in direction `angle` (1/256 turn,
/// counter-clockwise from three o'clock), rounded to the nearest pixel.
#[must_use]
pub fn polar(center: Point, radius: i32, angle: i32) -> Point {
    let scaled = |value: i32| {
        let product = radius * value;
        // Round half away from zero.
        (product + product.signum() * ONE / 2) / ONE
    };
    Point::new(center.x + scaled(cos(angle)), center.y - scaled(sin(angle)))
}

#[cfg(test)]
mod tests {
    use super::{ONE, Point, TURN, cos, polar, sin};

    #[test]
    fn the_cardinal_angles_are_exact() {
        assert_eq!([sin(0), sin(64), sin(128), sin(192)], [0, ONE, 0, -ONE]);
        assert_eq!([cos(0), cos(64), cos(128), cos(192)], [ONE, 0, -ONE, 0]);
        assert_eq!(sin(TURN + 10), sin(10));
        assert_eq!(sin(-10), -sin(10));
    }

    #[test]
    fn polar_points_go_the_screen_way() {
        let center = Point::new(10, 10);
        assert_eq!(polar(center, 5, 0), Point::new(15, 10));
        assert_eq!(polar(center, 5, 64), Point::new(10, 5), "up is a lower row");
        assert_eq!(polar(center, 5, 128), Point::new(5, 10));
        assert_eq!(polar(center, 5, 192), Point::new(10, 15));
    }
}
