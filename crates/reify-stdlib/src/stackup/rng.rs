//! Vendored deterministic PRNG + distribution samplers (T4 — PRD §3.3)
//!
//! This module provides a self-contained, platform-independent deterministic
//! PRNG ([`Xoshiro256StarStar`]) seeded via SplitMix64, plus per-distribution
//! samplers for Normal, Uniform-symmetric, and Triangular-symmetric distributions.
//!
//! No external `rand`/`rand_xoshiro` crates are used (PRD §3.3 invariant).

#![allow(dead_code)]

// ---------------------------------------------------------------------------
// Xoshiro256** PRNG
// ---------------------------------------------------------------------------

/// Deterministic, platform-independent pseudo-random number generator.
///
/// Uses the xoshiro256\*\* algorithm (Vigna 2018, CC0 public domain) seeded
/// via four successive SplitMix64 steps from a single `u64`.  All state
/// transitions are pure integer arithmetic (xor / shift / rotate / wrapping
/// multiply / wrapping add) — **bit-identical streams on every IEEE-754
/// platform**.
///
/// # Contract
///
/// - `from_seed(s)` ⟹ deterministic: two instances from the same seed
///   produce the same sequence.
/// - The sampling-order contract for Monte-Carlo stack-up (T5) is:
///   *contributor-index ascending, then draw-index ascending* — T4 exposes
///   primitives only; T5 enforces the nesting order when calling them.
///
/// # References
///
/// - <https://prng.di.unimi.it/xoshiro256starstar.c>  (CC0)
/// - <https://prng.di.unimi.it/splitmix64.c>          (CC0)
pub(super) struct Xoshiro256StarStar {
    /// Four-word xoshiro256** state.
    s: [u64; 4],
    /// Cached spare Normal-sampler output from Box–Muller pairing.
    spare: Option<f64>,
}

impl Xoshiro256StarStar {
    /// Construct a new generator from a single `u64` seed.
    ///
    /// The seed is expanded to four 64-bit state words using four successive
    /// SplitMix64 steps (Vigna's canonical seeding pattern).
    pub(super) fn from_seed(seed: u64) -> Self {
        let mut sm = seed;
        let s = [
            splitmix64_step(&mut sm),
            splitmix64_step(&mut sm),
            splitmix64_step(&mut sm),
            splitmix64_step(&mut sm),
        ];
        Xoshiro256StarStar { s, spare: None }
    }

    /// Return a `f64` in the half-open interval `[0, 1)`.
    ///
    /// Recipe: right-shift the raw `u64` by 11 bits to obtain a 53-bit
    /// integer, then multiply by `2^-53 = 0x1.0p-53` (IEEE-754 hex literal).
    ///
    /// This is the standard portable-deterministic recipe:
    /// - The 11-bit right-shift fits the integer exactly into the 53-bit
    ///   IEEE-754 double mantissa.
    /// - Multiplying by an exact power of two is always correctly-rounded ⇒
    ///   **same output on every IEEE-754 platform**.
    /// - Result is in `[0, 1)` because the integer is in `[0, 2^53)`.
    ///
    /// No platform-dependent transcendentals or rounding modes are involved.
    pub(super) fn next_uniform_f64(&mut self) -> f64 {
        // 0x3CA0000000000000 == 2^-53 as a raw IEEE-754 bit pattern.
        (self.next_u64() >> 11) as f64 * f64::from_bits(0x3CA0_0000_0000_0000)
    }

    /// Return the next `u64` from the stream and advance the state.
    ///
    /// Output function: `rotl(s[1] * 5, 7) * 9`  (xoshiro256** variant).
    pub(super) fn next_u64(&mut self) -> u64 {
        let result = self.s[1]
            .wrapping_mul(5)
            .rotate_left(7)
            .wrapping_mul(9);

        // xoshiro256** state advance (Vigna's reference):
        let t = self.s[1] << 17;
        self.s[2] ^= self.s[0];
        self.s[3] ^= self.s[1];
        self.s[1] ^= self.s[2];
        self.s[0] ^= self.s[3];
        self.s[2] ^= t;
        self.s[3] = self.s[3].rotate_left(45);

        result
    }
}

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

    // Golden first-16 u64 outputs of xoshiro256** seeded from 0x9E3779B97F4A7C15
    // via SplitMix64 (4 successive steps), derived from Vigna's reference C:
    //   https://prng.di.unimi.it/xoshiro256starstar.c  (CC0)
    //   https://prng.di.unimi.it/splitmix64.c          (CC0)
    const GOLDEN_SEED: u64 = 0x9E3779B97F4A7C15;
    const GOLDEN_FIRST_16: [u64; 16] = [
        0x422EA740D0977210_u64,
        0xE062B061B42E2928_u64,
        0x5A071FC5930841B6_u64,
        0x01334EF8ED3CC2BD_u64,
        0xE45CBD6A2D9E96DB_u64,
        0x3BC1FE841A5F292F_u64,
        0x60001D95EBBBD8E6_u64,
        0xA0AEE00B5B303762_u64,
        0x9E23C8D7514CF750_u64,
        0xFC79B675A1A76A3C_u64,
        0xD430797EB1952242_u64,
        0x5D8C1E38C042F56D_u64,
        0x62192F394C129095_u64,
        0xB66848E210A0F50D_u64,
        0x2D1D2EB24EDABA45_u64,
        0x794532BCAC68202C_u64,
    ];

    // Golden first-8 f64 outputs from next_uniform_f64(), derived bit-exactly
    // from GOLDEN_FIRST_16 via: (u >> 11) as f64 * 2^-53
    const GOLDEN_F64_FIRST_8: [f64; 8] = [
        0.2585243733634266_f64,
        0.8765058744940509_f64,
        0.35167120526878737_f64,
        0.004689155362245678_f64,
        0.8920400985931514_f64,
        0.23342886662646534_f64,
        0.375001763440863_f64,
        0.627668383381377_f64,
    ];

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

    // ---- next_uniform_f64 tests ----

    /// Every draw from next_uniform_f64 must lie in [0, 1).
    #[test]
    fn next_uniform_f64_is_in_unit_interval_half_open() {
        let mut rng = Xoshiro256StarStar::from_seed(0xABCD_EF01_2345_6789);
        for i in 0..10_000 {
            let f = rng.next_uniform_f64();
            assert!(f >= 0.0 && f < 1.0,
                "draw {i}: {f} out of [0, 1)");
        }
    }

    /// The first 8 draws from seed GOLDEN_SEED must match the bit-exact
    /// golden vector derived from (GOLDEN_FIRST_16[i] >> 11) * 2^-53.
    #[test]
    fn next_uniform_f64_first_8_bit_exact_at_seed() {
        let mut rng = Xoshiro256StarStar::from_seed(GOLDEN_SEED);
        for (i, &expected) in GOLDEN_F64_FIRST_8.iter().enumerate() {
            let got = rng.next_uniform_f64();
            assert_eq!(got.to_bits(), expected.to_bits(),
                "draw {i}: got {got}, expected {expected}");
        }
    }

    /// Two instances from the same seed produce the same 100 uniform draws.
    #[test]
    fn next_uniform_f64_deterministic_same_seed() {
        let mut rng1 = Xoshiro256StarStar::from_seed(0x1234_5678_9ABC_DEF0);
        let mut rng2 = Xoshiro256StarStar::from_seed(0x1234_5678_9ABC_DEF0);
        for i in 0..100 {
            let v1 = rng1.next_uniform_f64();
            let v2 = rng2.next_uniform_f64();
            assert_eq!(v1.to_bits(), v2.to_bits(), "diverged at draw {i}");
        }
    }

    // ---- Xoshiro256** golden + same-seed tests ----

    /// Verify the first 16 `next_u64()` outputs from seed `GOLDEN_SEED` match
    /// the hard-coded golden vector derived from Vigna's reference C code.
    #[test]
    fn xoshiro256ss_first_16_u64_matches_reference_at_seed() {
        let mut rng = Xoshiro256StarStar::from_seed(GOLDEN_SEED);
        for (i, &expected) in GOLDEN_FIRST_16.iter().enumerate() {
            let got = rng.next_u64();
            assert_eq!(got, expected, "mismatch at index {i}: got 0x{got:016X}, expected 0x{expected:016X}");
        }
    }

    /// Verify two independently-constructed `Xoshiro256StarStar` instances with
    /// the same seed produce bit-identical `next_u64()` streams (16 draws).
    #[test]
    fn xoshiro256ss_same_seed_streams_are_bit_identical() {
        let seed = GOLDEN_SEED;
        let mut rng1 = Xoshiro256StarStar::from_seed(seed);
        let mut rng2 = Xoshiro256StarStar::from_seed(seed);
        for i in 0..16 {
            let v1 = rng1.next_u64();
            let v2 = rng2.next_u64();
            assert_eq!(v1, v2, "diverged at draw {i}");
        }
    }
}
