//! Allocation-free text formatting for the screen.
//!
//! The core has no allocator, so every string shown on the OLED is built in a
//! fixed-size stack buffer. Overflow truncates instead of panicking: a clipped
//! label is a cosmetic problem, a panic on the module is not.

use core::fmt::{self, Write};

use crate::platform::MilliVolts;

/// A fixed-capacity, truncating string buffer.
#[derive(Debug, Clone, Copy)]
pub struct TextBuf<const N: usize> {
    bytes: [u8; N],
    len: usize,
}

impl<const N: usize> TextBuf<N> {
    /// An empty buffer.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            bytes: [0; N],
            len: 0,
        }
    }

    /// The text written so far.
    #[must_use]
    pub fn as_str(&self) -> &str {
        // Only whole `str` slices are ever appended, so the content is valid
        // UTF-8 by construction.
        core::str::from_utf8(&self.bytes[..self.len]).unwrap_or("")
    }

    /// Empties the buffer, keeping its capacity.
    pub const fn clear(&mut self) {
        self.len = 0;
    }

    /// Whether the last write had to be truncated.
    #[must_use]
    pub const fn is_full(&self) -> bool {
        self.len == N
    }
}

impl<const N: usize> Default for TextBuf<N> {
    fn default() -> Self {
        Self::new()
    }
}

impl<const N: usize> Write for TextBuf<N> {
    fn write_str(&mut self, text: &str) -> fmt::Result {
        for chunk in text.chars() {
            let mut encoded = [0u8; 4];
            let encoded = chunk.encode_utf8(&mut encoded).as_bytes();
            if self.len + encoded.len() > N {
                // Truncate silently; reporting an error here would make every
                // call site handle a failure that cannot be recovered from.
                return Ok(());
            }
            self.bytes[self.len..self.len + encoded.len()].copy_from_slice(encoded);
            self.len += encoded.len();
        }
        Ok(())
    }
}

/// Writes a level as signed volts, for example `+1.234` or `-0.500`.
///
/// `decimals` is clamped to `1..=3`; three decimals is exactly millivolt
/// resolution. The value is rounded to nearest, and the sign is always shown so
/// that columns line up on screen.
///
/// # Errors
///
/// Propagates the failure of the underlying writer.
pub fn write_volts(out: &mut impl Write, millivolts: MilliVolts, decimals: u32) -> fmt::Result {
    let decimals = decimals.clamp(1, 3);
    let sign = if millivolts < 0 { '-' } else { '+' };

    let divisor = 10u32.pow(3 - decimals);
    let scaled = (millivolts.unsigned_abs() + divisor / 2) / divisor;
    let unit = 10u32.pow(decimals);

    let whole = scaled / unit;
    let fraction = scaled % unit;
    let width = decimals as usize;
    write!(out, "{sign}{whole}.{fraction:0width$}")
}

#[cfg(test)]
mod tests {
    use core::fmt::Write;

    use super::{TextBuf, write_volts};

    fn volts(millivolts: i32, decimals: u32) -> TextBuf<16> {
        let mut buf = TextBuf::<16>::new();
        write_volts(&mut buf, millivolts, decimals).unwrap();
        buf
    }

    #[test]
    fn zero_is_positive_and_padded() {
        assert_eq!(volts(0, 3).as_str(), "+0.000");
        assert_eq!(volts(0, 1).as_str(), "+0.0");
    }

    #[test]
    fn signs_are_always_shown() {
        assert_eq!(volts(1_234, 3).as_str(), "+1.234");
        assert_eq!(volts(-1_234, 3).as_str(), "-1.234");
    }

    #[test]
    fn fewer_decimals_round_to_nearest() {
        assert_eq!(volts(1_250, 1).as_str(), "+1.3");
        assert_eq!(volts(-1_249, 1).as_str(), "-1.2");
        assert_eq!(volts(1_999, 2).as_str(), "+2.00");
    }

    #[test]
    fn the_decimal_count_is_clamped_to_a_sane_range() {
        assert_eq!(volts(1_500, 0).as_str(), volts(1_500, 1).as_str());
        assert_eq!(volts(1_500, 9).as_str(), volts(1_500, 3).as_str());
    }

    #[test]
    fn extreme_levels_do_not_panic() {
        assert!(volts(i32::MAX, 3).as_str().starts_with('+'));
        assert!(volts(i32::MIN, 3).as_str().starts_with('-'));
    }

    #[test]
    fn writing_past_capacity_truncates_instead_of_panicking() {
        let mut buf = TextBuf::<4>::new();
        write!(buf, "abcdefgh").unwrap();
        assert_eq!(buf.as_str(), "abcd");
        assert!(buf.is_full());
    }

    #[test]
    fn truncation_never_splits_a_character() {
        let mut buf = TextBuf::<3>::new();
        write!(buf, "aé").unwrap();
        // `é` is two bytes and does not fit after `a`, so it is dropped whole.
        assert_eq!(buf.as_str(), "aé");
        let mut tight = TextBuf::<2>::new();
        write!(tight, "aé").unwrap();
        assert_eq!(tight.as_str(), "a");
    }

    #[test]
    fn clearing_reuses_the_buffer() {
        let mut buf = TextBuf::<8>::new();
        write!(buf, "first").unwrap();
        buf.clear();
        write!(buf, "second").unwrap();
        assert_eq!(buf.as_str(), "second");
    }
}
