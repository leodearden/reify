//! Pure, dependency-free cache-key half of the modal-analysis trampoline.
//!
//! The faer-matrix-holding `(K, M)` warm-state cache lives in the `reify-eval`
//! modal trampoline (`modal_ops.rs`); this module holds only the pure
//! `ModalCacheKey` it keys that cache on. (Type lands in task κ step-2.)

#[cfg(test)]
mod tests {
    use super::ModalCacheKey;

    // Baseline (K,M)-determining inputs for a steel beam at P1 order:
    // length/width/height (m); E (Pa), ν, density (kg/m³); element_order (P1=0).
    const L: f64 = 0.02;
    const W: f64 = 0.05;
    const H: f64 = 0.1;
    const E: f64 = 205e9;
    const NU: f64 = 0.29;
    const RHO: f64 = 7850.0;
    const P1: u8 = 0;
    const P2: u8 = 1;

    fn baseline() -> ModalCacheKey {
        ModalCacheKey::new(L, W, H, E, NU, RHO, P1)
    }

    /// (a) Two keys built from identical (geometry, material, element_order)
    /// match — the cache-HIT condition.
    #[test]
    fn matches_identical_inputs() {
        assert!(baseline().matches(&baseline()));
    }

    /// (b) A different `length` (geometry change → different K/M) must NOT match.
    #[test]
    fn differs_on_length() {
        let other = ModalCacheKey::new(L + 1e-6, W, H, E, NU, RHO, P1);
        assert!(!baseline().matches(&other));
        // matches() is symmetric (per-field bit equality).
        assert!(!other.matches(&baseline()));
    }

    /// (c) A different `density` (changes the consistent mass M) must NOT match.
    #[test]
    fn differs_on_density() {
        let other = ModalCacheKey::new(L, W, H, E, NU, RHO * 1.001, P1);
        assert!(!baseline().matches(&other));
    }

    /// (d) Same geometry+material but DIFFERENT element_order (P1 vs P2) must NOT
    /// match — task 4066's P2 path assembles a distinct (K,M)/n_nodes, so a
    /// P1-assembled cache must never be served for a P2 request.
    #[test]
    fn differs_on_element_order() {
        let p1 = ModalCacheKey::new(L, W, H, E, NU, RHO, P1);
        let p2 = ModalCacheKey::new(L, W, H, E, NU, RHO, P2);
        assert!(!p1.matches(&p2));
        assert!(!p2.matches(&p1));
    }

    /// (e) Equality is per-field `f64::to_bits`, NOT `==`: two keys carrying a
    /// NaN in the same field match (NaN != NaN under `==`, but the bit patterns
    /// are equal), while -0.0 and +0.0 — equal under `==` — do NOT match (their
    /// bits differ). This pins the bit-exact, deterministic comparison.
    #[test]
    fn uses_bit_equality_not_partial_eq() {
        // Identical NaN bits in the same field → match (an `==`-based key fails).
        let nan_a = ModalCacheKey::new(L, W, H, E, NU, f64::NAN, P1);
        let nan_b = ModalCacheKey::new(L, W, H, E, NU, f64::NAN, P1);
        assert!(nan_a.matches(&nan_b), "identical NaN bits must match");

        // -0.0 vs +0.0 length → equal under `==`, distinct bits → must NOT match.
        let neg_zero = ModalCacheKey::new(-0.0, W, H, E, NU, RHO, P1);
        let pos_zero = ModalCacheKey::new(0.0, W, H, E, NU, RHO, P1);
        assert!(!neg_zero.matches(&pos_zero), "-0.0 and +0.0 differ by bits");
    }
}
