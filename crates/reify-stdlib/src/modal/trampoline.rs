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

/// The forcing-independent precompute determinants of a transient-response
/// solve, used to decide whether a cached [`TransientCache`] can be reused for
/// a new call (see the module docs and the `reify-eval` `TransientCache` for
/// what the cache holds).
///
/// The key captures EXACTLY the inputs that determine the per-mode integrator
/// coefficients and the uniform time grid:
/// - `t_start`, `t_end`, `dt` — fix the grid (`uniform_time_grid`).
/// - per-mode `(frequency_hz, damping_ratio)` — fix the Duhamel coefficients
///   or the Newmark routing choice for every mode.
///
/// It deliberately EXCLUDES:
/// - `forcing` — the cheap-varying input in the PRD §7.8 / §9.1 input-shaping
///   loop; excluding it makes a forcing-only change a HIT that re-runs only
///   the projection + ODE recurrence (skipping coefficient derivation),
///   mirroring [`ModalCacheKey`]'s exclusion of the eigensolve knobs.
/// - mode SHAPES — shapes feed only the always-recomputed forcing projection;
///   the cached coefficients depend only on (freq, damping, dt, t_range), so
///   a key match over those fields fully certifies the cache without the
///   O(n_modes · n_nodes) bit-compare that shape inclusion would require.
///
/// Compared via [`matches`](TransientCacheKey::matches) — per-field
/// `f64::to_bits` equality plus a leading mode-count check — NOT via
/// `PartialEq`/`Eq` (the `f64` fields are not `Eq`; bit equality gives
/// deterministic, `-0.0`/`NaN`-correct matching). NOT `Copy` (holds a `Vec`).
#[derive(Clone, Debug)]
pub struct TransientCacheKey {
    /// Start of the time window (SI seconds).
    pub t_start: f64,
    /// End of the time window (SI seconds).
    pub t_end: f64,
    /// Uniform time step Δt (SI seconds).
    pub dt: f64,
    /// Per-mode `(frequency_hz, damping_ratio)` pairs — determines the
    /// integrator selection and Duhamel/Newmark coefficients for each mode.
    pub modes: Vec<(f64, f64)>,
}

impl TransientCacheKey {
    /// Build a key from the forcing-independent precompute determinants.
    ///
    /// `modes` is a `Vec` of `(frequency_hz, damping_ratio)` pairs, one per
    /// mode in the cached `ModalResult`. The order must match the mode list;
    /// a different mode count is a cache MISS even if all shared fields match.
    pub fn new(t_start: f64, t_end: f64, dt: f64, modes: Vec<(f64, f64)>) -> Self {
        Self { t_start, t_end, dt, modes }
    }

    /// `true` iff every forcing-independent precompute determinant bit-matches
    /// `other`.
    ///
    /// Checks `modes.len()` equality first (a mode-count change is always a
    /// MISS), then per-field [`f64::to_bits`] equality for `t_start`, `t_end`,
    /// `dt`, and each mode's `(frequency_hz, damping_ratio)` pair. Bit equality
    /// is collision-free and deterministic: `-0.0` ≠ `+0.0` (bits differ) and
    /// two identical `NaN` bit patterns DO match (unlike IEEE `==`). A `true`
    /// result certifies that the cached grid + integrators are valid for the new
    /// call — only the forcing projection and ODE recurrence need to re-run.
    pub fn matches(&self, other: &TransientCacheKey) -> bool {
        if self.modes.len() != other.modes.len() {
            return false;
        }
        self.t_start.to_bits() == other.t_start.to_bits()
            && self.t_end.to_bits() == other.t_end.to_bits()
            && self.dt.to_bits() == other.dt.to_bits()
            && self
                .modes
                .iter()
                .zip(other.modes.iter())
                .all(|((f1, z1), (f2, z2))| {
                    f1.to_bits() == f2.to_bits() && z1.to_bits() == z2.to_bits()
                })
    }
}

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
    use super::{ModalCacheKey, TransientCacheKey};

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

    // ── TransientCacheKey tests ──────────────────────────────────────────────

    /// Baseline (t_start, t_end, dt, modes) for a two-mode transient setup.
    fn transient_baseline() -> TransientCacheKey {
        TransientCacheKey::new(
            0.0,   // t_start (s)
            0.1,   // t_end (s)
            0.005, // dt (s)
            vec![(40.0, 0.01), (250.0, 0.02)],  // (frequency_hz, damping_ratio)
        )
    }

    /// (a) Two keys built from identical (t_start, t_end, dt, per-mode pairs)
    /// `matches()` — the cache-HIT condition.
    #[test]
    fn transient_key_matches_identical_inputs() {
        assert!(transient_baseline().matches(&transient_baseline()));
    }

    /// (b) A different `dt` must NOT match.
    #[test]
    fn transient_key_differs_on_dt() {
        let other = TransientCacheKey::new(0.0, 0.1, 0.010, vec![(40.0, 0.01), (250.0, 0.02)]);
        assert!(!transient_baseline().matches(&other));
        assert!(!other.matches(&transient_baseline()));
    }

    /// (c) A different `t_start` must NOT match; a different `t_end` must NOT match.
    #[test]
    fn transient_key_differs_on_t_start_and_t_end() {
        let diff_start = TransientCacheKey::new(0.001, 0.1, 0.005, vec![(40.0, 0.01), (250.0, 0.02)]);
        assert!(!transient_baseline().matches(&diff_start), "different t_start must MISS");

        let diff_end = TransientCacheKey::new(0.0, 0.2, 0.005, vec![(40.0, 0.01), (250.0, 0.02)]);
        assert!(!transient_baseline().matches(&diff_end), "different t_end must MISS");
    }

    /// (d) A different mode `frequency_hz` must NOT match;
    /// a different `damping_ratio` must NOT match.
    #[test]
    fn transient_key_differs_on_mode_fields() {
        let diff_freq = TransientCacheKey::new(0.0, 0.1, 0.005, vec![(45.0, 0.01), (250.0, 0.02)]);
        assert!(!transient_baseline().matches(&diff_freq), "different frequency_hz must MISS");

        let diff_zeta = TransientCacheKey::new(0.0, 0.1, 0.005, vec![(40.0, 0.02), (250.0, 0.02)]);
        assert!(!transient_baseline().matches(&diff_zeta), "different damping_ratio must MISS");
    }

    /// (e) A different mode COUNT must NOT match.
    #[test]
    fn transient_key_differs_on_mode_count() {
        let three_modes = TransientCacheKey::new(
            0.0, 0.1, 0.005, vec![(40.0, 0.01), (250.0, 0.02), (600.0, 0.03)],
        );
        assert!(!transient_baseline().matches(&three_modes), "different mode count must MISS");

        let one_mode = TransientCacheKey::new(0.0, 0.1, 0.005, vec![(40.0, 0.01)]);
        assert!(!transient_baseline().matches(&one_mode), "different mode count must MISS");
    }

    /// (f) Bit-equality semantics: two keys with a NaN in the same field match
    /// (identical NaN bit patterns), while -0.0 vs +0.0 in `dt` do NOT match.
    #[test]
    fn transient_key_uses_bit_equality() {
        // Identical NaN bits in the same field → match.
        let nan_a = TransientCacheKey::new(f64::NAN, 0.1, 0.005, vec![(40.0, 0.01)]);
        let nan_b = TransientCacheKey::new(f64::NAN, 0.1, 0.005, vec![(40.0, 0.01)]);
        assert!(nan_a.matches(&nan_b), "identical NaN bits must match");

        // -0.0 vs +0.0 in dt → equal under `==`, distinct bits → must NOT match.
        let neg_zero_dt = TransientCacheKey::new(0.0, 0.1, -0.0_f64, vec![(40.0, 0.01)]);
        let pos_zero_dt = TransientCacheKey::new(0.0, 0.1, 0.0_f64, vec![(40.0, 0.01)]);
        assert!(!neg_zero_dt.matches(&pos_zero_dt), "-0.0 and +0.0 in dt differ by bits");
    }
}
