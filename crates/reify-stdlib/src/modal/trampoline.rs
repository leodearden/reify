//! Pure, dependency-free cache-key half of the modal-analysis trampoline
//! (task κ; `docs/grants/GR-002/compute-node-contract.md`).
//!
//! Mirrors the `free_vibration` pure-helper split: the expensive, faer-matrix-
//! holding `(K, M)` warm-state cache (and its `OpaqueState` wiring) lives in the
//! `reify-eval` modal trampoline (`modal_ops.rs`), because `reify-stdlib` does
//! not depend on `faer` / `reify-solver-elastic`. This module holds only the
//! pure [`ModalCacheKey`] that cache is keyed on — `f64`/`u8` primitives, no
//! new deps.
//!
//! The key captures EXACTLY the `(K, M)`-determining inputs: the beam geometry
//! (`length`, `width`, `height`), the isotropic material (`youngs_modulus`,
//! `poisson_ratio`, `density`), and the `element_order` discriminant (P1 vs P2 —
//! task 4066 made the assembled `K`/`M` and even the node count order-dependent,
//! so a P1 assembly must never be reused for a P2 request). It deliberately
//! EXCLUDES everything that affects only the cheap downstream free-DOF
//! projection + eigensolve — `n_modes`, `tol`, `sigma`, `max_iters`,
//! `boundary_conditions`, `damping`, `reference_direction` — so two calls that
//! differ only in those still HIT the cached assembly (the PRD amortization
//! goal, e.g. sweeping `n_modes`).
//!
//! `element_order` is a plain `u8` (e.g. P1 = 0, P2 = 1) because this crate
//! cannot name `reify-solver-elastic::ElementOrder`; `reify-eval` maps the enum
//! to the `u8` discriminant at the call site (the same `extract_element_order`
//! result that selects the mesh, so the key and the assembly can never disagree).
//!
//! Comparison is per-`f64`-field [`f64::to_bits`] equality plus `==` on the
//! `u8` — collision-free and deterministic, with `-0.0`/`NaN` resolved by their
//! bit patterns rather than IEEE `==` (so [`ModalCacheKey`] is intentionally
//! `Copy`/`Debug` but NOT `PartialEq`/`Eq`).

/// The `(K, M)`-determining inputs of a modal assembly, used to decide whether a
/// cached [`ModalAssembly`](../../../reify_eval/modal_ops/index.html) can be
/// reused for a new modal request (see the module docs for what is and is not
/// part of the key).
///
/// Compared via [`matches`](ModalCacheKey::matches) — per-field `f64::to_bits`
/// equality plus a `u8` `element_order` discriminant — NOT via `PartialEq`
/// (the `f64` fields are not `Eq`, and bit equality gives deterministic,
/// `-0.0`/`NaN`-correct matching).
#[derive(Clone, Copy, Debug)]
pub struct ModalCacheKey {
    /// Beam length (SI metres, X / beam axis).
    pub length: f64,
    /// Beam width (SI metres, Y).
    pub width: f64,
    /// Beam height (SI metres, Z / bending axis).
    pub height: f64,
    /// Young's modulus E (Pa).
    pub youngs_modulus: f64,
    /// Poisson's ratio ν (dimensionless).
    pub poisson_ratio: f64,
    /// Mass density ρ (kg/m³) — drives the consistent mass matrix M.
    pub density: f64,
    /// Element-order discriminant (e.g. P1 = 0, P2 = 1). Mapped from
    /// `reify-solver-elastic::ElementOrder` by the `reify-eval` caller, which
    /// owns the enum; this dependency-free crate only stores the `u8`.
    pub element_order: u8,
}

impl ModalCacheKey {
    /// Build a key from the `(K, M)`-determining inputs.
    ///
    /// `element_order` is the caller-mapped `u8` discriminant of
    /// `reify-solver-elastic::ElementOrder` (P1 = 0, P2 = 1); see the module docs.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        length: f64,
        width: f64,
        height: f64,
        youngs_modulus: f64,
        poisson_ratio: f64,
        density: f64,
        element_order: u8,
    ) -> Self {
        Self {
            length,
            width,
            height,
            youngs_modulus,
            poisson_ratio,
            density,
            element_order,
        }
    }

    /// `true` iff every `(K, M)`-determining input bit-matches `other`.
    ///
    /// Each `f64` field is compared by [`f64::to_bits`] (so `-0.0` ≠ `+0.0` and
    /// two identical `NaN` bit patterns DO match — collision-free and
    /// deterministic, unlike IEEE `==`); `element_order` by `u8` equality. A
    /// `true` result means a cached assembly built for `other` may be reused for
    /// `self` (a cache HIT).
    pub fn matches(&self, other: &ModalCacheKey) -> bool {
        self.length.to_bits() == other.length.to_bits()
            && self.width.to_bits() == other.width.to_bits()
            && self.height.to_bits() == other.height.to_bits()
            && self.youngs_modulus.to_bits() == other.youngs_modulus.to_bits()
            && self.poisson_ratio.to_bits() == other.poisson_ratio.to_bits()
            && self.density.to_bits() == other.density.to_bits()
            && self.element_order == other.element_order
    }
}

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
