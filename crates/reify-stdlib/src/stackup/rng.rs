//! Vendored deterministic PRNG + distribution samplers (T4 — PRD §3.3)
//!
//! This module provides a self-contained, platform-independent deterministic
//! PRNG ([`Xoshiro256StarStar`]) seeded via SplitMix64, plus per-distribution
//! samplers for Normal, Uniform-symmetric, and Triangular-symmetric distributions.
//!
//! No external `rand`/`rand_xoshiro` crates are used (PRD §3.3 invariant).

#![allow(dead_code)]

// ---------------------------------------------------------------------------
// SplitMix64 seeder
// ---------------------------------------------------------------------------

/// One step of the SplitMix64 generator (Vigna, CC0 public domain).
///
/// Advances `*state` by one step and returns the mixed output value.  Used
/// exclusively to convert a single `u64` seed into 4 independent `u64` words
/// for `Xoshiro256StarStar::from_seed`.
///
/// Reference: <https://prng.di.unimi.it/splitmix64.c>
fn splitmix64_step(state: &mut u64) -> u64 {
    *state = state.wrapping_add(0x9E3779B97F4A7C15);
    let mut z = *state;
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58476D1CE4E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D049BB133111EB);
    z ^ (z >> 31)
}

#[cfg(test)]
mod tests {
    use super::*;

    // ---- SplitMix64 stepper golden-value tests ----

    /// Verify the first 4 outputs of `splitmix64_step` from seed 0 match
    /// Vigna's published reference values.
    ///
    /// Reference: https://prng.di.unimi.it/splitmix64.c (CC0)
    /// Starting state = 0, 4 successive steps produce:
    ///   0xE220A8397B1DCDAF, 0x6E789E6AA1B965F4,
    ///   0x06C45D188009454F, 0xF88BB8A8724C81EC
    #[test]
    fn splitmix64_step_matches_reference_first_4() {
        let mut state: u64 = 0;
        assert_eq!(splitmix64_step(&mut state), 0xE220A8397B1DCDAF_u64);
        assert_eq!(splitmix64_step(&mut state), 0x6E789E6AA1B965F4_u64);
        assert_eq!(splitmix64_step(&mut state), 0x06C45D188009454F_u64);
        assert_eq!(splitmix64_step(&mut state), 0xF88BB8A8724C81EC_u64);
    }

    /// Verify that two separately-seeded streams from the same seed
    /// produce the same first 4 outputs (determinism / same-seed contract).
    #[test]
    fn splitmix64_step_deterministic_same_seed() {
        let seed = 0xDEAD_BEEF_CAFE_1234_u64;
        let mut s1 = seed;
        let mut s2 = seed;
        for _ in 0..4 {
            assert_eq!(splitmix64_step(&mut s1), splitmix64_step(&mut s2));
        }
    }
}
