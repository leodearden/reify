use std::fmt;
use xxhash_rust::xxh3::xxh3_128;

/// 128-bit content hash for incremental change detection.
/// Backed by XXH3-128 for speed and quality.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct ContentHash(pub u128);

impl ContentHash {
    /// Hash arbitrary bytes.
    pub fn of(data: &[u8]) -> Self {
        Self(xxh3_128(data))
    }

    /// Hash a string.
    pub fn of_str(s: &str) -> Self {
        Self::of(s.as_bytes())
    }

    /// Hash a `u64` via its little-endian byte representation.
    ///
    /// **Forward-looking stability.** ContentHash is currently in-memory only.
    /// If a persistent incremental-compile cache is introduced, this little-endian
    /// encoding becomes part of its wire format — any change to byte order or width
    /// must accompany a cache-format version bump to avoid silent collisions with
    /// hashes computed by prior builds.
    pub fn of_u64(n: u64) -> Self {
        Self::of(&n.to_le_bytes())
    }

    /// Combine two hashes (order-dependent).
    pub fn combine(self, other: ContentHash) -> ContentHash {
        let mut buf = [0u8; 32];
        buf[..16].copy_from_slice(&self.0.to_le_bytes());
        buf[16..].copy_from_slice(&other.0.to_le_bytes());
        ContentHash::of(&buf)
    }

    /// Combine a sequence of hashes.
    pub fn combine_all(hashes: impl IntoIterator<Item = ContentHash>) -> ContentHash {
        let mut acc = ContentHash(0);
        for h in hashes {
            acc = acc.combine(h);
        }
        acc
    }
}

impl fmt::Debug for ContentHash {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "ContentHash({:032x})", self.0)
    }
}

impl fmt::Display for ContentHash {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:032x}", self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deterministic() {
        let a = ContentHash::of_str("hello");
        let b = ContentHash::of_str("hello");
        assert_eq!(a, b);
    }

    #[test]
    fn different_inputs_differ() {
        let a = ContentHash::of_str("hello");
        let b = ContentHash::of_str("world");
        assert_ne!(a, b);
    }

    #[test]
    fn combine_is_order_dependent() {
        let a = ContentHash::of_str("a");
        let b = ContentHash::of_str("b");
        assert_ne!(a.combine(b), b.combine(a));
    }

    #[test]
    fn whitespace_change_changes_hash() {
        let a = ContentHash::of_str("param width: Scalar = 80mm");
        let b = ContentHash::of_str("param width:  Scalar = 80mm");
        assert_ne!(a, b);
    }

    #[test]
    fn of_u64_is_deterministic() {
        assert_eq!(ContentHash::of_u64(42), ContentHash::of_u64(42));
    }

    #[test]
    fn of_u64_differs_for_different_values() {
        assert_ne!(ContentHash::of_u64(0), ContentHash::of_u64(1));
        assert_ne!(ContentHash::of_u64(1), ContentHash::of_u64(u64::MAX));
    }
}
