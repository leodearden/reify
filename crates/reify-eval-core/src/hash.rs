//! The core's 128-bit hash: xxh3, under a name that does not collide with
//! `std::hash::Hash`.

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
