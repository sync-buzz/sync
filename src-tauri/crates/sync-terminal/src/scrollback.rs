//! The tail of what a terminal has said, addressed by offset.
//!
//! A pure structure with no process behind it, which is the point: everything
//! about *what a reader is owed* can be decided at a desk, and the only thing
//! left for a test with a shell in it is whether the bytes arrive at all.

use std::collections::VecDeque;

use serde::Serialize;

/// What a reader asking from an offset is given back.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Tail {
    /// The offset these bytes start at.
    ///
    /// Greater than the offset that was asked for when the ring has already
    /// dropped what sat between them. **That difference is the whole reason
    /// this member exists**: a reader that has fallen too far behind is owed
    /// the news that it has, and a terminal stream with bytes missing out of
    /// the middle of it is a corrupted screen with nothing to say why.
    pub from: u64,
    /// The offset to ask from next.
    pub to: u64,
    /// The bytes themselves, escape sequences and all.
    pub bytes: Vec<u8>,
}

impl Tail {
    /// Whether anything was dropped between what was asked for and what came
    /// back.
    #[must_use]
    pub fn is_gapped(&self, asked_from: u64) -> bool {
        self.from > asked_from
    }
}

/// A bounded tail of bytes, and the offset each of them arrived at.
///
/// **Bytes rather than lines.** A pty emits escape sequences, and a sequence
/// does not end at a newline — splitting the stream on one cuts a colour or a
/// cursor move in half and puts its remains on a row of their own.
///
/// **Bounded, because nothing else bounds it.** A command that prints for ever
/// is an ordinary thing to run, and a person who runs one and goes to lunch
/// should come back to a window rather than to a machine that has been swapping
/// for an hour.
#[derive(Debug)]
pub struct Scrollback {
    bytes: VecDeque<u8>,
    /// The offset of the byte at the front of the deque.
    start: u64,
    capacity: usize,
}

impl Scrollback {
    #[must_use]
    pub fn new(capacity: usize) -> Self {
        Self {
            bytes: VecDeque::new(),
            start: 0,
            capacity,
        }
    }

    /// The offset just past the last byte held.
    #[must_use]
    pub fn end(&self) -> u64 {
        self.start + self.bytes.len() as u64
    }

    /// Take in what the process has said, dropping from the front to stay
    /// within capacity.
    pub fn push(&mut self, chunk: &[u8]) -> u64 {
        // A chunk larger than the ring keeps only its own tail. Pushing it byte
        // by byte and trimming after each would be the same answer arrived at
        // slowly.
        let chunk = if chunk.len() > self.capacity {
            let skipped = chunk.len() - self.capacity;
            self.start += skipped as u64;
            &chunk[skipped..]
        } else {
            chunk
        };

        self.bytes.extend(chunk.iter().copied());

        let over = self.bytes.len().saturating_sub(self.capacity);
        if over > 0 {
            self.bytes.drain(..over);
            self.start += over as u64;
        }
        self.end()
    }

    /// Everything held from `offset` onwards.
    ///
    /// An offset past the end answers empty rather than refusing: a reader that
    /// has kept up asks for exactly that on every wake-up, and it is not a
    /// mistake.
    #[must_use]
    pub fn since(&self, offset: u64) -> Tail {
        let from = offset.max(self.start);
        let skip = usize::try_from(from - self.start).unwrap_or(usize::MAX);
        let bytes: Vec<u8> = self.bytes.iter().skip(skip).copied().collect();
        Tail {
            from,
            to: from + bytes.len() as u64,
            bytes,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_back_what_was_written() {
        let mut ring = Scrollback::new(64);
        ring.push(b"hello ");
        ring.push(b"world");
        let tail = ring.since(0);
        assert_eq!(tail.bytes, b"hello world");
        assert_eq!(tail.from, 0);
        assert_eq!(tail.to, 11);
        assert!(!tail.is_gapped(0));
    }

    #[test]
    fn a_reader_that_kept_up_is_given_only_what_is_new() {
        let mut ring = Scrollback::new(64);
        ring.push(b"first");
        let seen = ring.since(0).to;
        ring.push(b"second");
        let tail = ring.since(seen);
        assert_eq!(tail.bytes, b"second");
        assert!(!tail.is_gapped(seen));
    }

    #[test]
    fn an_offset_past_the_end_is_answered_empty() {
        let ring = Scrollback::new(64);
        let tail = ring.since(99);
        assert!(tail.bytes.is_empty());
        assert_eq!(tail.to, tail.from);
    }

    #[test]
    fn falling_behind_the_ring_is_reported_rather_than_papered_over() {
        let mut ring = Scrollback::new(8);
        ring.push(b"aaaaaaaa");
        ring.push(b"bbbbbbbb");
        let tail = ring.since(0);
        assert_eq!(tail.bytes, b"bbbbbbbb");
        assert_eq!(
            tail.from, 8,
            "the first eight bytes are gone and the offset says so"
        );
        assert!(tail.is_gapped(0));
    }

    #[test]
    fn a_chunk_larger_than_the_ring_keeps_its_tail_and_counts_the_rest() {
        let mut ring = Scrollback::new(4);
        ring.push(b"0123456789");
        let tail = ring.since(0);
        assert_eq!(tail.bytes, b"6789");
        assert_eq!(tail.from, 6);
        assert_eq!(
            ring.end(),
            10,
            "every byte that passed through is counted, held or not"
        );
    }

    #[test]
    fn offsets_keep_counting_across_a_drop() {
        let mut ring = Scrollback::new(4);
        ring.push(b"abcd");
        ring.push(b"efgh");
        assert_eq!(ring.end(), 8);
        let tail = ring.since(6);
        assert_eq!(tail.bytes, b"gh");
        assert_eq!(
            tail.from, 6,
            "an offset the ring still holds is answered exactly"
        );
    }
}
