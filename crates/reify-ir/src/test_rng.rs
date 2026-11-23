//! A fixed-seed generator for reproducible sampled tests, without a dependency.

/// splitmix64 from a caller-chosen seed.
pub(crate) struct SplitMix64(pub(crate) u64);

impl SplitMix64 {
    pub(crate) fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// Uniform in [-1, 1).
    pub(crate) fn symmetric(&mut self) -> f64 {
        let unit_interval = (self.next_u64() >> 11) as f64 / (1u64 << 53) as f64;
        2.0 * unit_interval - 1.0
    }
}
