//! The core's 128-bit hash: xxh3, under a name that does not collide with
//! `std::hash::Hash`, and the one encoder through which the core combines
//! anything into one (PRD D4: a host supplies leaf contents only).

use xxhash_rust::xxh3::xxh3_128;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Hash128(u128);

impl Hash128 {
    pub fn of_bytes(bytes: &[u8]) -> Self {
        Self(xxh3_128(bytes))
    }

    pub const fn from_u128(bits: u128) -> Self {
        Self(bits)
    }

    pub const fn as_u128(self) -> u128 {
        self.0
    }
}

/// Fields written in order and hashed together. Each field is marked with
/// its sort and text is prefixed with its length, so two different
/// sequences of fields never give the same bytes; a sequence is written as
/// its count, then its items.
pub(crate) struct Encoder {
    bytes: Vec<u8>,
}

impl Encoder {
    /// An encoder whose first field is `tag`, naming what it encodes.
    pub fn new(tag: &str) -> Self {
        let mut encoder = Self { bytes: Vec::new() };
        encoder.tag(tag);
        encoder
    }

    /// Which of several sorts the next fields belong to.
    pub fn tag(&mut self, tag: &str) -> &mut Self {
        self.bytes.push(b't');
        self.length_prefixed(tag.as_bytes())
    }

    pub fn text(&mut self, text: &str) -> &mut Self {
        self.bytes.push(b's');
        self.length_prefixed(text.as_bytes())
    }

    pub fn number(&mut self, number: u64) -> &mut Self {
        self.bytes.push(b'n');
        self.bytes.extend_from_slice(&number.to_le_bytes());
        self
    }

    pub fn hash(&mut self, hash: Hash128) -> &mut Self {
        self.bytes.push(b'h');
        self.bytes.extend_from_slice(&hash.0.to_le_bytes());
        self
    }

    /// The count of `items`, then each written by `write`.
    pub fn sequence<T>(
        &mut self,
        items: impl ExactSizeIterator<Item = T>,
        mut write: impl FnMut(&mut Self, T),
    ) -> &mut Self {
        self.number(items.len() as u64);
        for item in items {
            write(self, item);
        }
        self
    }

    pub fn finish(&self) -> Hash128 {
        Hash128::of_bytes(&self.bytes)
    }

    fn length_prefixed(&mut self, bytes: &[u8]) -> &mut Self {
        self.bytes
            .extend_from_slice(&(bytes.len() as u64).to_le_bytes());
        self.bytes.extend_from_slice(bytes);
        self
    }
}
