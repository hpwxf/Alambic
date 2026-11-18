//! The four waveforms of a tone, read from a 32-bit phase: no floating point
//! (journal/D-005, D-033).
//!
//! A phase is a fraction of a turn, `u32::MAX + 1` being one whole period, so
//! it wraps by itself. Every waveform starts at zero and rises, as a sine
//! does, and is scaled by [`ONE`]: `-ONE..=ONE`.

/// Scale of every waveform: its value at the top of the period.
pub const ONE: i32 = 1 << 15;

/// Bits of a phase that index the quarter table; the rest interpolates.
const QUARTER_BITS: u32 = 8;

/// Points in the quarter table, both ends included.
const QUARTER_POINTS: usize = (1 << QUARTER_BITS) + 1;

/// `sin` over a quarter turn in [`QUARTER_POINTS`] steps, scaled by [`ONE`],
/// computed when compiling.
const QUARTER: [i32; QUARTER_POINTS] = quarter_table();

/// One, in the 2.30 fixed point the table is computed in.
const UNIT: i64 = 1 << 30;

/// `pi / 2` in 2.30.
const HALF_PI: i64 = 1_686_629_713;

/// The quarter table, from a Taylor series up to `x^13` in 2.30 fixed point:
/// its error at `pi / 2` is below `1e-9`, far under the table's last place.
#[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
// Every entry is at most `ONE`, so it fits an `i32`; the index stays below
// `QUARTER_POINTS`, so it fits an `i64`.
const fn quarter_table() -> [i32; QUARTER_POINTS] {
    let mut table = [0; QUARTER_POINTS];
    let mut index = 0;
    while index < QUARTER_POINTS {
        let x = HALF_PI * index as i64 / (QUARTER_POINTS as i64 - 1);
        let x2 = x * x / UNIT;
        // Horner: sin x = x (1 - x²/(2·3) (1 - x²/(4·5) (1 - ...))).
        let mut term = UNIT;
        let mut k = 12;
        while k >= 2 {
            term = UNIT - x2 * term / UNIT / (k * (k + 1));
            k -= 2;
        }
        let sine = x * term / UNIT;
        table[index] = ((sine + (1 << 14)) >> 15) as i32;
        index += 1;
    }
    table
}

/// The shape of a tone.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Waveform {
    /// A sine, from the table.
    Sine,
    /// A triangle.
    Triangle,
    /// A rising sawtooth, without band limiting: it aliases.
    Saw,
    /// A square, half the period up, without band limiting: it aliases.
    Square,
}

impl Waveform {
    /// Every waveform, in the order of the outputs of `TONE`.
    pub const ALL: [Self; 4] = [Self::Sine, Self::Triangle, Self::Saw, Self::Square];

    /// Its value at `phase`, in `-ONE..=ONE`.
    #[must_use]
    pub fn at(self, phase: u32) -> i32 {
        match self {
            Self::Sine => sine(phase),
            Self::Triangle => {
                // A quarter ahead, so it starts at zero; folded at the half.
                let shifted = phase.wrapping_add(1 << 30);
                let folded = if shifted & (1 << 31) == 0 {
                    shifted
                } else {
                    !shifted
                };
                // `folded` spans the half turn, `0..2^31`.
                (folded >> 15).cast_signed() - ONE
            }
            Self::Saw => ((phase ^ (1 << 31)) >> 16).cast_signed() - ONE,
            Self::Square => {
                if phase < 1 << 31 {
                    ONE
                } else {
                    -ONE
                }
            }
        }
    }

    /// The name shown on screen.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Sine => "SIN",
            Self::Triangle => "TRI",
            Self::Saw => "SAW",
            Self::Square => "SQR",
        }
    }
}

/// `sin` of a phase, scaled by [`ONE`]: the quarter table, mirrored, and
/// interpolated linearly between its points.
#[must_use]
pub fn sine(phase: u32) -> i32 {
    // The two top bits pick the quarter, the next ones the table step.
    let quadrant = phase >> 30;
    let within = phase & ((1 << 30) - 1);
    let position = if quadrant & 1 == 0 {
        within
    } else {
        (1 << 30) - within
    };
    let fraction_bits = 30 - QUARTER_BITS;
    let index = (position >> fraction_bits) as usize;
    let fraction = i64::from(position & ((1 << fraction_bits) - 1));
    let low = i64::from(QUARTER[index]);
    // `position` reaches the end of the quarter only exactly on it.
    let high = i64::from(QUARTER[(index + 1).min(QUARTER_POINTS - 1)]);
    let half = 1 << (fraction_bits - 1);
    let value = low + (((high - low) * fraction + half) >> fraction_bits);
    #[allow(clippy::cast_possible_truncation)]
    // Between two table entries, so within `-ONE..=ONE`.
    let value = value as i32;
    if quadrant < 2 { value } else { -value }
}

#[cfg(test)]
mod tests {
    use super::{ONE, QUARTER, Waveform, sine};

    const QUARTER_TURN: u32 = 1 << 30;

    #[test]
    fn the_quarter_table_ends_exactly() {
        assert_eq!(QUARTER[0], 0);
        assert_eq!(QUARTER[QUARTER.len() - 1], ONE);
        assert!(QUARTER.windows(2).all(|pair| pair[0] < pair[1]));
    }

    #[test]
    fn the_sine_hits_its_cardinal_points() {
        assert_eq!(sine(0), 0);
        assert_eq!(sine(QUARTER_TURN), ONE);
        assert_eq!(sine(2 * QUARTER_TURN), 0);
        assert_eq!(sine(3 * QUARTER_TURN), -ONE);
    }

    #[test]
    fn every_waveform_starts_at_zero_and_rises() {
        for wave in [Waveform::Sine, Waveform::Triangle, Waveform::Saw] {
            assert_eq!(wave.at(0), 0, "{wave:?}");
            assert!(wave.at(QUARTER_TURN / 2) > 0, "{wave:?}");
        }
        assert_eq!(Waveform::Square.at(0), ONE);
    }

    #[test]
    fn every_waveform_stays_within_one() {
        for wave in Waveform::ALL {
            for step in 0..=1024u32 {
                let value = wave.at(step.wrapping_mul(1 << 22).wrapping_sub(1));
                assert!((-ONE..=ONE).contains(&value), "{wave:?} {step}: {value}");
            }
        }
    }

    #[test]
    fn the_triangle_and_the_saw_peak_where_they_should() {
        // The top is one step short of `ONE`: the fold spans `2^31` values.
        assert_eq!(Waveform::Triangle.at(QUARTER_TURN), ONE - 1);
        assert_eq!(Waveform::Triangle.at(3 * QUARTER_TURN), -ONE);
        assert_eq!(Waveform::Saw.at(2 * QUARTER_TURN), -ONE);
        assert_eq!(Waveform::Saw.at(2 * QUARTER_TURN - (1 << 16)), ONE - 1);
    }
}
