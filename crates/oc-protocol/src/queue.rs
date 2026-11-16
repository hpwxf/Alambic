//! Bytes waiting for a USB endpoint: the log lines of the first port, the
//! frames of the second.
//!
//! A message is queued whole or not at all ([`ByteQueue::push`]), so a full
//! queue costs whole lines or frames, never a torn one that the reader would
//! have to resynchronise from.

/// A fixed-size first-in first-out queue of bytes.
#[derive(Debug, Clone)]
pub struct ByteQueue<const N: usize> {
    bytes: [u8; N],
    start: usize,
    len: usize,
    dropped: u32,
}

impl<const N: usize> Default for ByteQueue<N> {
    fn default() -> Self {
        Self::new()
    }
}

impl<const N: usize> ByteQueue<N> {
    /// An empty queue.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            bytes: [0; N],
            start: 0,
            len: 0,
            dropped: 0,
        }
    }

    /// Bytes waiting.
    #[must_use]
    pub const fn len(&self) -> usize {
        self.len
    }

    /// Whether nothing is waiting.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Room left, in bytes.
    #[must_use]
    pub const fn free(&self) -> usize {
        N - self.len
    }

    /// Messages refused because the queue was full, since it was made.
    #[must_use]
    pub const fn dropped(&self) -> u32 {
        self.dropped
    }

    /// Queues `message` whole, or refuses it and counts it as dropped.
    pub fn push(&mut self, message: &[u8]) -> bool {
        if message.len() > self.free() {
            self.dropped = self.dropped.saturating_add(1);
            return false;
        }
        for &byte in message {
            self.bytes[(self.start + self.len) % N] = byte;
            self.len += 1;
        }
        true
    }

    /// The oldest waiting bytes that lie in one piece, at most `max`; hand
    /// how many were actually sent to [`Self::consume`].
    #[must_use]
    pub fn peek(&self, max: usize) -> &[u8] {
        let contiguous = self.len.min(N - self.start).min(max);
        &self.bytes[self.start..self.start + contiguous]
    }

    /// Forgets the `count` oldest bytes (at most what is waiting).
    pub const fn consume(&mut self, count: usize) {
        let count = if count < self.len { count } else { self.len };
        self.start = (self.start + count) % N;
        self.len -= count;
    }

    /// Forgets everything waiting.
    pub const fn clear(&mut self) {
        self.start = 0;
        self.len = 0;
    }
}

#[cfg(test)]
mod tests {
    use std::vec::Vec;

    use super::ByteQueue;

    #[test]
    fn bytes_come_out_in_order_across_the_wrap() {
        let mut queue = ByteQueue::<8>::new();
        assert!(queue.push(b"abcdef"));
        queue.consume(4);
        assert!(queue.push(b"ghij"));
        let mut out = Vec::new();
        while !queue.is_empty() {
            let piece = queue.peek(3).to_vec();
            queue.consume(piece.len());
            out.extend(piece);
        }
        assert_eq!(out, b"efghij");
    }

    #[test]
    fn a_message_that_does_not_fit_is_dropped_whole() {
        let mut queue = ByteQueue::<8>::new();
        assert!(queue.push(b"12345"));
        assert!(!queue.push(b"6789"));
        assert_eq!((queue.len(), queue.dropped()), (5, 1));
        assert_eq!(queue.peek(64), b"12345");
    }

    #[test]
    fn consuming_more_than_is_waiting_empties_the_queue() {
        let mut queue = ByteQueue::<4>::new();
        queue.push(b"ab");
        queue.consume(10);
        assert!(queue.is_empty());
        assert_eq!(queue.free(), 4);
    }
}
