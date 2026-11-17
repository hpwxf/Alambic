//! Bytes as the 32-bit words an SPI controller's transmit FIFO shifts out,
//! for a sender that fills the FIFO itself instead of going through a
//! blocking bus (the firmware's audio interrupt, journal/D-033).
//!
//! A frame of `8 × n` bits goes out most significant bit first, four bytes a
//! word, the first byte in the top one; the last word holds what is left,
//! right-aligned. That is the i.MX RT LPSPI's rule, and exactly how
//! `imxrt-hal` packs a byte slice for its own blocking writes, which is what
//! the OLED and the DAC were brought up with.

/// The words of `bytes`, in order.
pub fn words(bytes: &[u8]) -> impl Iterator<Item = u32> + '_ {
    bytes.chunks(4).map(|chunk| {
        chunk
            .iter()
            .fold(0u32, |word, &byte| (word << 8) | u32::from(byte))
    })
}

/// How many words [`words`] gives for `len` bytes.
#[must_use]
pub const fn word_count(len: usize) -> usize {
    len.div_ceil(4)
}

#[cfg(test)]
mod tests {
    use std::vec::Vec;

    use super::{word_count, words};

    #[test]
    fn whole_words_put_the_first_byte_on_top() {
        let packed: Vec<u32> = words(&[0x12, 0x34, 0x56, 0x78, 0x9A, 0xBC, 0xDE, 0xF0]).collect();
        assert_eq!(packed, [0x1234_5678, 0x9ABC_DEF0]);
    }

    #[test]
    fn the_last_word_is_right_aligned() {
        let packed: Vec<u32> = words(&[0xB0, 0x10, 0x02]).collect();
        assert_eq!(packed, [0x00B0_1002], "a 24-bit command");
        let packed: Vec<u32> = words(&[1, 2, 3, 4, 5, 6]).collect();
        assert_eq!(packed, [0x0102_0304, 0x0000_0506]);
        assert_eq!(word_count(6), 2);
        assert_eq!(word_count(24), 6);
        assert_eq!(word_count(0), 0);
    }
}
