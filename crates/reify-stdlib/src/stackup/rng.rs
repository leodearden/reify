//! Vendored deterministic PRNG + distribution samplers (T4 — PRD §3.3)

#![allow(dead_code)]

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
