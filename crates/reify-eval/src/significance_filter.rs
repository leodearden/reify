//! Output significance filter for ComputeNode results.
//!
//! # PRD reference
//!
//! `docs/prds/v0_3/compute-node-infrastructure.md §P3.6`
//!
//! # Opt-in contract
//!
//! Only compute targets explicitly opted in (see [`is_opted_in`]) have their
//! outputs compared with per-purpose length tolerance. Unknown targets return
//! [`FilterOutcome::NotOptedIn`], which the caller (P3.3 task 3382, the
//! freshness-walk hook) treats identically to [`FilterOutcome::Different`]
//! (normal invalidation). Keeping the two variants distinct lets telemetry
//! and integration tests distinguish "filter declined" from "filter ran and
//! found a material difference".
//!
//! Current v1 allowlist: `"solver::elastic_static"` and `"solver::buckling"`.
//!
//! # Per-field policy (v1)
//!
//! The per-purpose length tolerance applies only to the `displacement` field
//! (Length-valued). `stress`, `max_von_mises`, `converged`, and `iterations`
//! use exact equality — no Pressure tolerance class exists today; the
//! conservative over-invalidation posture is correct per task 3385 scope.
//!
//! # Conservative-fallback policy
//!
//! Any departure from the expected ElasticResult Map shape (see
//! `crates/reify-stdlib/src/fea.rs`) or a missing/invalid tolerance returns
//! [`FilterOutcome::Different`] — over-invalidate rather than under-invalidate.
//! This includes mismatched sampling geometry on the `displacement` field:
//! identical data values at physically different grid locations are always a
//! material change (see [`reify_types::SampledField::grid_metadata_eq`]).
//!
//! # NaN handling
//!
//! Non-finite values (`NaN`, `±Inf`) in the `displacement` data yield
//! [`FilterOutcome::Different`] when `prev` and `new` are not bit-equal.
//! If both `prev` and `new` carry identical bit-pattern NaN (e.g. the solver
//! returned NaN twice in a row without change), the bit-equality shortcut fires
//! first and returns [`FilterOutcome::Equivalent`].
//! Effective contract: *NaN propagates as Different only when prev and new are
//! not bit-equal.*
//!
//! # Integration contract
//!
//! `length_tolerance_si: Option<f64>` is resolved by the caller via
//! `Engine::active_tolerance_for(subject_entity_ref)` (task 3382 / P3.3).
//! The filter itself is engine-free (pure function).

/// The three-variant outcome of the output significance filter.
///
/// `NotOptedIn` and `Different` both signal "proceed with normal invalidation"
/// at the call site; the distinction lets observability surfaces
/// (logs, telemetry, integration tests) tell "filter declined" from
/// "filter ran and disagreed".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FilterOutcome {
    /// Previous and new result are within per-purpose tolerance — downstream
    /// output ValueCells need NOT be marked dirty.
    Equivalent,
    /// Previous and new result differ materially (or tolerance is unknown) —
    /// proceed with normal invalidation.
    Different,
    /// Target is not in the opt-in allowlist — filter did not run.
    /// Caller should proceed with normal invalidation.
    NotOptedIn,
}

/// Returns `true` if `target` has opted into significance filtering.
///
/// v1 allowlist: `"solver::elastic_static"` and `"solver::buckling"`.
/// Switching to a registry-based or annotation-driven mechanism is a
/// non-breaking internal refactor — the function signature absorbs the
/// lookup mechanism.
pub fn is_opted_in(target: &str) -> bool {
    matches!(target, "solver::elastic_static" | "solver::buckling")
}

/// Relative tolerance for per-mode eigenvalue comparison in BucklingResult.
///
/// λ (the critical-load multiplier) is dimensionless and O(1e4) for typical
/// columns (P_cr ≈ 42 kN at ref_load 1 N → λ ≈ 4e4).  An absolute metre
/// tolerance is dimensionally inapplicable.
///
/// This constant is chosen:
/// - **Above** eigensolver numerical noise: `BucklingOptions.tol` defaults to
///   1e-8 (Lanczos convergence floor), giving ~100× margin on the low side.
/// - **Below** engineering significance: P1-tet method discretization error is
///   itself ~9% for a smoke column, so ~1e-3 is the engineering threshold.
///   This constant sits ~1000× below that, giving a conservative over-Equivalent
///   posture symmetrical to the elastic over-Different posture.
///
/// The constant is `pub(crate)` and named so it is tunable without breaking
/// tests.  Integration tests assert with wide order-of-magnitude margins
/// (1e-9 << EIGENVALUE_REL_TOL << 1e-2) so the exact value is not pinned.
pub(crate) const EIGENVALUE_REL_TOL: f64 = 1e-6;

/// Denominator floor for the relative eigenvalue comparison.
///
/// Guards the near-zero-λ edge case: when both eigenvalues are near zero,
/// `|a|.max(|b|)` is also near zero, making the relative comparison
/// degenerate.  This floor is chosen well below the physical λ floor for
/// real column-buckling problems (λ > 1 for loads at 1 N) so it activates
/// only for near-zero or sign-negative corner cases, not production values.
const EIGENVALUE_MIN_DENOM: f64 = 1e-12;

/// Key for the displacement field in an ElasticResult Map.
const DISPLACEMENT_KEY: &str = "displacement";

/// Non-displacement field keys compared for exact equality in an ElasticResult Map.
///
/// v1 policy: `stress`, `max_von_mises`, `converged`, and `iterations` are
/// compared bit-exactly — no Pressure tolerance class exists today.
const NON_DISPLACEMENT_KEYS: [&str; 4] = ["stress", "max_von_mises", "converged", "iterations"];

/// Cached `Value::String` BTreeMap lookup key for the displacement field.
///
/// Process-lifetime singleton: allocated on first `significance_filter` call
/// and reused for every subsequent invocation. Once task 3382 (P3.3
/// freshness-walk hook) routes filter calls through per-ComputeNode recompute,
/// this avoids one `String` allocation per recompute.
static DISPLACEMENT_KEY_VALUE: std::sync::LazyLock<reify_ir::Value> =
    std::sync::LazyLock::new(|| reify_ir::Value::String(DISPLACEMENT_KEY.to_string()));

/// Cached `Value::String` BTreeMap lookup keys for the four non-displacement
/// fields, in the same iteration order as [`NON_DISPLACEMENT_KEYS`].
///
/// See [`DISPLACEMENT_KEY_VALUE`] for the amortization rationale; this static
/// caches the array of four keys (one allocation event per process instead of
/// four per call).
static NON_DISPLACEMENT_KEY_VALUES: std::sync::LazyLock<[reify_ir::Value; 4]> =
    std::sync::LazyLock::new(|| {
        NON_DISPLACEMENT_KEYS.map(|k| reify_ir::Value::String(k.to_string()))
    });

/// Compare two [`reify_ir::Value::GeometryHandle`] values for cache-key significance.
///
/// Returns [`FilterOutcome::Equivalent`] when the two handles represent the
/// same realized geometry from the caller's caching perspective — i.e. when
/// **both** their `realization_ref` (entity + index) and `upstream_values_hash`
/// are identical.  Returns [`FilterOutcome::Different`] in all other cases,
/// including the conservative fallback when either input is not a
/// `Value::GeometryHandle`.
///
/// # Kernel-handle exclusion (load-bearing)
///
/// `kernel_handle` is intentionally **excluded** from the comparison.  It is an
/// ephemeral session-scoped id: re-realizing the same geometry in a new Engine
/// session assigns a fresh handle while the semantic identity of the geometry is
/// unchanged.  Excluding it means a cached downstream computation is NOT
/// invalidated when the geometry is re-realized to a different handle with
/// identical parameters — the correct behaviour per PRD §1 / §5 and the
/// GHR-β design decision recorded in `crates/reify-ir/src/value.rs`.
///
/// # Conservative-fallback policy
///
/// If either `old` or `new` is not a `Value::GeometryHandle`, the function
/// returns `Different`.  This mirrors the general over-invalidate-rather-than-
/// under-invalidate posture used by [`significance_filter`].
///
/// See also [`significance_filter`] for the tolerance-bearing ElasticResult
/// path; geometry handles use exact equality (no tolerance class), which is
/// why this is a standalone function rather than an arm of that one.
///
/// # Wiring status
///
/// Currently **not called by any production code** — wiring into the
/// compute-node caching path is deferred to GHR-ζ, where geometry persistence
/// and the active-kernel selection land alongside the call site.  Kept
/// crate-private until then to prevent premature API surface drift.
#[allow(dead_code)] // wiring deferred to GHR-ζ; used in tests only for now
pub(crate) fn geometry_handle_significance(
    old: &reify_ir::Value,
    new: &reify_ir::Value,
) -> FilterOutcome {
    match (old, new) {
        (
            reify_ir::Value::GeometryHandle {
                realization_ref: rr_old,
                upstream_values_hash: h_old,
                ..
            },
            reify_ir::Value::GeometryHandle {
                realization_ref: rr_new,
                upstream_values_hash: h_new,
                ..
            },
        ) => {
            if rr_old == rr_new && h_old == h_new {
                FilterOutcome::Equivalent
            } else {
                FilterOutcome::Different
            }
        }
        // Conservative fallback: non-GeometryHandle inputs → Different.
        _ => FilterOutcome::Different,
    }
}

/// Compare a compute node's previous and new result with per-purpose tolerance.
///
/// # Arguments
///
/// - `target`: the compute target string (e.g. `"solver::elastic_static"`).
///   See [`is_opted_in`] for the opt-in mechanism.
/// - `prev`: the previously-cached result value.
/// - `new`: the newly-computed result value.
/// - `length_tolerance_si`: SI-metre tolerance from
///   `Engine::active_tolerance_for(subject_entity_ref)` (task 3382 / P3.3).
///   `None` triggers the conservative `Different` fallback.
///
/// # Return value
///
/// | Outcome      | Meaning |
/// |--------------|---------|
/// | `Equivalent` | Delta within tolerance — MAY skip marking output ValueCells dirty |
/// | `Different`  | Material change / unknown tolerance — MUST mark dirty |
/// | `NotOptedIn` | Target not in allowlist — MUST mark dirty |
// G-allow: task #3427 — P3.3 freshness-walk hook is the designed consumer; fn fully built+tested, caller wiring (freshness-walk invoking significance_filter via Engine::active_tolerance_for) deferred to #3427 (pending: "Significance filter integrated into freshness walk at output-VC boundary")
pub fn significance_filter(
    target: &str,
    prev: &reify_ir::Value,
    new: &reify_ir::Value,
    length_tolerance_si: Option<f64>,
) -> FilterOutcome {
    // Opt-in guard: unknown targets never reach comparison logic.
    if !is_opted_in(target) {
        return FilterOutcome::NotOptedIn;
    }

    // Bit-equality shortcut: identical Values are Equivalent without consulting
    // the Map shape or tolerance. This is the steady-state-cheap path.
    // Note: bit-identical NaN values in displacement.data return Equivalent here
    // (SampledField::PartialEq uses to_bits(), so same-bit NaN == same-bit NaN).
    // The effective NaN contract is: "NaN propagates as Different only when prev
    // and new are not bit-equal" — see the element-wise loop below. Relies on
    // Value::PartialEq (value.rs:1573).
    // Exercises: significance_filter_returns_equivalent_for_bit_equal_results
    //            significance_filter_does_not_false_positive_on_bit_equal_with_zero_tolerance
    if prev == new {
        return FilterOutcome::Equivalent;
    }

    // Missing/invalid tolerance guard: conservative fallback.
    // Collapses None and any malformed tolerance (NaN/Inf/negative) into
    // Different via the same gate as tolerance_scope.rs:151.
    // Exercises: significance_filter_returns_different_when_tolerance_missing
    let tol_si = match length_tolerance_si {
        Some(t) if crate::tolerance_gate::is_valid_tolerance_si(t) => t,
        _ => return FilterOutcome::Different,
    };

    // Buckling dispatch: StructureInstance-shaped result — different path from
    // the elastic Value::Map path below. Placed after the shared prologue so the
    // opt-in / bit-equality / tolerance guards are reused unchanged.
    // Exercises: all buckling_significance integration tests (task θ #3457)
    if target == "solver::buckling" {
        return buckling_result_significance(prev, new, tol_si);
    }

    // Map-shape guard: non-Map inputs are malformed — conservative fallback.
    // Exercises: significance_filter_returns_different_for_malformed_shapes (a)
    let (prev_map, new_map) = match (prev, new) {
        (reify_ir::Value::Map(p), reify_ir::Value::Map(n)) => (p, n),
        _ => return FilterOutcome::Different,
    };

    // BTreeMap key Values are allocated exactly once per process via the
    // module-level LazyLock statics DISPLACEMENT_KEY_VALUE and
    // NON_DISPLACEMENT_KEY_VALUES. BTreeMap::get<Q> requires K: Borrow<Q>;
    // since Value: Borrow<Value> (blanket impl), &Value is a valid argument.
    // Deref coercion: &*LazyLock<T> → &T; &[T; N] coerces to &[T] for iteration.

    // Non-displacement keys: require exact Value equality.
    // Any mismatch (including a key present in one map but absent in the other)
    // returns Different. v1 policy: no Pressure tolerance class exists today.
    // Exercises: significance_filter_returns_different_for_non_displacement_field_changes
    for key_val in &*NON_DISPLACEMENT_KEY_VALUES {
        if prev_map.get(key_val) != new_map.get(key_val) {
            return FilterOutcome::Different;
        }
    }

    // Displacement extraction: missing key is malformed — conservative fallback.
    // Exercises: significance_filter_returns_different_for_malformed_shapes (b)
    let (prev_disp, new_disp) = match (
        prev_map.get(&*DISPLACEMENT_KEY_VALUE),
        new_map.get(&*DISPLACEMENT_KEY_VALUE),
    ) {
        (Some(p), Some(n)) => (p, n),
        _ => return FilterOutcome::Different,
    };

    // Displacement must be a Sampled Field wrapping a SampledField payload.
    // Any other variant (Analytical, Real, missing lambda, …) is malformed.
    // Exercises: significance_filter_returns_different_for_malformed_shapes (c), (d)
    use reify_ir::FieldSourceKind;
    let (prev_sf, new_sf) = match (prev_disp, new_disp) {
        (
            reify_ir::Value::Field {
                source: FieldSourceKind::Sampled,
                lambda: prev_lambda,
                ..
            },
            reify_ir::Value::Field {
                source: FieldSourceKind::Sampled,
                lambda: new_lambda,
                ..
            },
        ) => match (prev_lambda.as_ref(), new_lambda.as_ref()) {
            (reify_ir::Value::SampledField(p), reify_ir::Value::SampledField(n)) => (p, n),
            _ => return FilterOutcome::Different,
        },
        _ => return FilterOutcome::Different,
    };

    // Data-length guard: mismatched DOF counts are malformed.
    // Exercises: significance_filter_returns_different_for_malformed_shapes (e)
    if prev_sf.data.len() != new_sf.data.len() {
        return FilterOutcome::Different;
    }

    // Grid-metadata guard: mismatched sampling geometry is always Different —
    // identical data values at different spatial coordinates are semantically
    // different results. Compares kind, name, bounds, spacing, axis_grids, and
    // interpolation (everything except data and oob_emitted).
    // Exercises: significance_filter_returns_different_for_shifted_grid_metadata
    if !prev_sf.grid_metadata_eq(new_sf) {
        return FilterOutcome::Different;
    }

    // Element-wise absolute-delta comparison with per-purpose length tolerance.
    // Non-finite values (NaN, ±Inf) in EITHER operand yield Different when prev
    // and new are not bit-equal (the bit-equality shortcut above handles the
    // bit-identical NaN case). Effective contract: NaN propagates as Different
    // only when prev != new. NaN comparisons are always false in IEEE 754, so we
    // guard with is_finite() before the subtraction.
    // Strict-greater-than: delta == tol_si is Equivalent (not Different).
    // step-12 confirmed: (p - n).abs() > tol_si covers over-tolerance with strict-gt.
    // Exercises: significance_filter_returns_different_for_nan_in_displacement
    //            significance_filter_returns_different_for_over_tolerance_displacement_delta
    //            significance_filter_returns_equivalent_for_sub_tolerance_displacement_delta
    for (&p, &n) in prev_sf.data.iter().zip(new_sf.data.iter()) {
        if !p.is_finite() || !n.is_finite() || (p - n).abs() > tol_si {
            return FilterOutcome::Different;
        }
    }

    FilterOutcome::Equivalent
}

// ── BucklingResult significance helper ───────────────────────────────────────

/// Compare two `BucklingResult` [`reify_ir::Value::StructureInstance`] values
/// for output significance.
///
/// Called from [`significance_filter`] after the shared prologue (opt-in guard,
/// bit-equality shortcut, valid-tolerance gate).
///
/// # Field comparison policy
///
/// | Field | Policy |
/// |-------|--------|
/// | `converged` | Exact `Bool` equality |
/// | `iterations` | Exact `Int` equality |
/// | `modes` count | Equal length required |
/// | per-mode `eigenvalue` | Relative tolerance [`EIGENVALUE_REL_TOL`] |
/// | per-mode `mode_shape displaced_positions` | Absolute `tol_si` element-wise |
/// | `pre_stress` | Structural presence/type check only |
///
/// # Conservative-Different contract
///
/// Returns [`FilterOutcome::Different`] on **every** shape departure:
/// non-`StructureInstance` input, wrong `type_name`, missing fields, modes
/// count mismatch, non-`StructureInstance` mode entry, missing `eigenvalue`,
/// NaN/Inf eigenvalue, mode_shape structural errors.
///
/// The conservative posture mirrors the elastic filter — over-invalidate rather
/// than under-invalidate.
fn buckling_result_significance(
    prev: &reify_ir::Value,
    new: &reify_ir::Value,
    tol_si: f64,
) -> FilterOutcome {
    // Both must be StructureInstance with type_name "BucklingResult".
    let (prev_d, new_d) = match (prev, new) {
        (
            reify_ir::Value::StructureInstance(p),
            reify_ir::Value::StructureInstance(n),
        ) => (p, n),
        _ => return FilterOutcome::Different,
    };
    if prev_d.type_name != "BucklingResult" || new_d.type_name != "BucklingResult" {
        return FilterOutcome::Different;
    }

    // converged: exact Bool equality.
    match (
        prev_d.fields.get("converged"),
        new_d.fields.get("converged"),
    ) {
        (Some(reify_ir::Value::Bool(p)), Some(reify_ir::Value::Bool(n))) if p == n => {}
        _ => return FilterOutcome::Different,
    }

    // iterations: exact Int equality.
    match (
        prev_d.fields.get("iterations"),
        new_d.fields.get("iterations"),
    ) {
        (Some(reify_ir::Value::Int(p)), Some(reify_ir::Value::Int(n))) if p == n => {}
        _ => return FilterOutcome::Different,
    }

    // modes: both must be Value::List of equal length.
    let (prev_modes, new_modes) = match (
        prev_d.fields.get("modes"),
        new_d.fields.get("modes"),
    ) {
        (Some(reify_ir::Value::List(p)), Some(reify_ir::Value::List(n))) => (p, n),
        _ => return FilterOutcome::Different,
    };
    if prev_modes.len() != new_modes.len() {
        return FilterOutcome::Different;
    }

    // Per-mode: eigenvalue (relative) + mode_shape displaced_positions (absolute tol_si).
    for (p_mode, n_mode) in prev_modes.iter().zip(new_modes.iter()) {
        // Mode entries must be StructureInstances.
        let (pm, nm) = match (p_mode, n_mode) {
            (
                reify_ir::Value::StructureInstance(p),
                reify_ir::Value::StructureInstance(n),
            ) => (p, n),
            _ => return FilterOutcome::Different,
        };

        // Eigenvalue: relative tolerance comparison.
        let (p_ev, n_ev) = match (pm.fields.get("eigenvalue"), nm.fields.get("eigenvalue")) {
            (Some(reify_ir::Value::Real(p)), Some(reify_ir::Value::Real(n))) => (*p, *n),
            _ => return FilterOutcome::Different,
        };
        let denom = p_ev.abs().max(n_ev.abs()).max(EIGENVALUE_MIN_DENOM);
        if !p_ev.is_finite() || !n_ev.is_finite() || (p_ev - n_ev).abs() > EIGENVALUE_REL_TOL * denom {
            return FilterOutcome::Different;
        }

        // Mode_shape displaced_positions: absolute tol_si element-wise.
        // Conservative Different on any structural departure.
        // Exercises: step-6 (buckling_significance.rs mode_shape tests)
        let (p_pos, n_pos) = match (pm.fields.get("mode_shape"), nm.fields.get("mode_shape")) {
            (Some(reify_ir::Value::Map(p)), Some(reify_ir::Value::Map(n))) => {
                let key = reify_ir::Value::String("displaced_positions".to_string());
                match (p.get(&key), n.get(&key)) {
                    (Some(reify_ir::Value::List(pl)), Some(reify_ir::Value::List(nl))) => (pl, nl),
                    _ => return FilterOutcome::Different,
                }
            }
            _ => return FilterOutcome::Different,
        };
        if p_pos.len() != n_pos.len() {
            return FilterOutcome::Different;
        }
        for (pv, nv) in p_pos.iter().zip(n_pos.iter()) {
            let (p, n) = match (pv, nv) {
                (reify_ir::Value::Real(p), reify_ir::Value::Real(n)) => (*p, *n),
                _ => return FilterOutcome::Different,
            };
            // Non-finite values → Different. Strict-greater-than mirrors the
            // elastic displacement comparison (`> tol_si`, not `>= tol_si`).
            if !p.is_finite() || !n.is_finite() || (p - n).abs() > tol_si {
                return FilterOutcome::Different;
            }
        }
    }

    // pre_stress: structural presence/type check only.
    // Deep significance is subsumed transitively by the eigenvalue (λ is a
    // functional of pre_stress via K_g), so a structural guard suffices in v1.
    // Exercises: step-8 (buckling_significance.rs pre_stress tests)
    match (
        prev_d.fields.get("pre_stress"),
        new_d.fields.get("pre_stress"),
    ) {
        (Some(reify_ir::Value::StructureInstance(_)), Some(reify_ir::Value::StructureInstance(_))) => {}
        _ => return FilterOutcome::Different,
    }

    FilterOutcome::Equivalent
}

#[cfg(test)]
mod tests {
    use super::{FilterOutcome, is_opted_in, significance_filter};

    // ── geometry_handle_significance tests (step-1 RED) ──────────────────────
    mod geometry_handle {
        use super::super::{FilterOutcome, geometry_handle_significance};
        use reify_core::identity::RealizationNodeId;
        use reify_ir::{GeometryHandleId, Value};

        /// Build a `Value::GeometryHandle` with the given entity, realization
        /// index, upstream_values_hash, and kernel_handle id.
        fn gh(entity: &str, index: u32, hash: [u8; 32], kernel_id: u64) -> Value {
            Value::GeometryHandle {
                realization_ref: RealizationNodeId::new(entity, index),
                upstream_values_hash: hash,
                kernel_handle: GeometryHandleId(kernel_id),
            }
        }

        /// (1) Equal realization_ref + equal upstream_values_hash → Equivalent.
        #[test]
        fn equal_rr_and_hash_yields_equivalent() {
            let a = gh("Widget", 0, [0xAAu8; 32], 1);
            let b = gh("Widget", 0, [0xAAu8; 32], 1);
            assert_eq!(
                geometry_handle_significance(&a, &b),
                FilterOutcome::Equivalent,
                "same rr + same hash must be Equivalent",
            );
        }

        /// (2) Equal realization_ref + DIFFERENT upstream_values_hash → Different.
        #[test]
        fn different_hash_yields_different() {
            let a = gh("Widget", 0, [0xAAu8; 32], 1);
            let b = gh("Widget", 0, [0xBBu8; 32], 1);
            assert_eq!(
                geometry_handle_significance(&a, &b),
                FilterOutcome::Different,
                "same rr but different hash must be Different",
            );
        }

        /// (3a) DIFFERENT realization_ref entity + equal hash → Different.
        #[test]
        fn different_entity_yields_different() {
            let a = gh("Widget", 0, [0xAAu8; 32], 1);
            let b = gh("Gadget", 0, [0xAAu8; 32], 1);
            assert_eq!(
                geometry_handle_significance(&a, &b),
                FilterOutcome::Different,
                "different entity in rr must be Different",
            );
        }

        /// (3b) DIFFERENT realization_ref index + equal hash → Different.
        #[test]
        fn different_index_yields_different() {
            let a = gh("Widget", 0, [0xAAu8; 32], 1);
            let b = gh("Widget", 1, [0xAAu8; 32], 1);
            assert_eq!(
                geometry_handle_significance(&a, &b),
                FilterOutcome::Different,
                "different realization index in rr must be Different",
            );
        }

        /// (4) Equal rr + equal hash but DIFFERENT kernel_handle → Equivalent.
        /// kernel_handle is intentionally excluded from significance comparison
        /// (re-realization to a new handle for semantically-identical geometry
        /// must NOT invalidate downstream per GHR-β §DD / PRD §1).
        #[test]
        fn different_kernel_handle_yields_equivalent() {
            let a = gh("Widget", 0, [0xAAu8; 32], 1);
            let b = gh("Widget", 0, [0xAAu8; 32], 999);
            assert_eq!(
                geometry_handle_significance(&a, &b),
                FilterOutcome::Equivalent,
                "kernel_handle difference must NOT cause Different (excluded from comparison)",
            );
        }

        /// (5) Non-GeometryHandle input on either or both sides → Different
        /// (conservative fallback).
        #[test]
        fn non_geometry_handle_input_yields_different() {
            let gh_val = gh("Widget", 0, [0xAAu8; 32], 1);
            let other = Value::Undef;
            assert_eq!(
                geometry_handle_significance(&other, &gh_val),
                FilterOutcome::Different,
                "non-GH old must yield Different",
            );
            assert_eq!(
                geometry_handle_significance(&gh_val, &other),
                FilterOutcome::Different,
                "non-GH new must yield Different",
            );
            assert_eq!(
                geometry_handle_significance(&other, &other),
                FilterOutcome::Different,
                "both non-GH must yield Different",
            );
        }
    }
    use reify_core::Type;
    use reify_ir::{FieldSourceKind, InterpolationKind, SampledField, SampledGridKind, Value};
    use std::collections::BTreeMap;
    use std::sync::Arc;
    use std::sync::atomic::AtomicBool;

    // ── Test helper: ElasticResult-shaped Value::Map ──────────────────────────
    //
    // Matches the stdlib shape documented in crates/reify-stdlib/src/fea.rs:
    //   "displacement" → Value::Field { source: Sampled, lambda: Value::SampledField }
    //   "stress"       → Value::Field { source: Sampled, lambda: Value::SampledField }
    //   "max_von_mises"→ Value::Real
    //   "converged"    → Value::Bool
    //   "iterations"   → Value::Int

    fn make_sampled_field(name: &str, data: &[f64]) -> Value {
        Value::Field {
            domain_type: Type::dimensionless_scalar(),
            codomain_type: Type::dimensionless_scalar(),
            source: FieldSourceKind::Sampled,
            lambda: Arc::new(Value::SampledField(SampledField {
                name: name.to_string(),
                kind: SampledGridKind::Regular1D,
                bounds_min: vec![0.0],
                bounds_max: vec![1.0],
                spacing: vec![0.5],
                axis_grids: vec![(0..data.len()).map(|i| i as f64).collect()],
                interpolation: InterpolationKind::Linear,
                data: data.to_vec(),
                oob_emitted: AtomicBool::new(false),
            })),
        }
    }

    /// Build a synthesised ElasticResult-shaped Value::Map for use in
    /// significance_filter unit tests. Matches the stdlib fea.rs output shape.
    fn make_elastic_result_value(
        displacement_data: &[f64],
        stress_data: &[f64],
        max_vm: f64,
        converged: bool,
        iters: u32,
    ) -> Value {
        let mut map = BTreeMap::new();
        map.insert(
            Value::String("displacement".to_string()),
            make_sampled_field("displacement", displacement_data),
        );
        map.insert(
            Value::String("stress".to_string()),
            make_sampled_field("stress", stress_data),
        );
        map.insert(
            Value::String("max_von_mises".to_string()),
            Value::Real(max_vm),
        );
        map.insert(
            Value::String("converged".to_string()),
            Value::Bool(converged),
        );
        map.insert(
            Value::String("iterations".to_string()),
            Value::Int(iters as i64),
        );
        Value::Map(map)
    }

    // ── Step-1: is_opted_in allowlist tests ──────────────────────────────────

    #[test]
    fn is_opted_in_returns_true_for_elastic_static() {
        assert!(
            is_opted_in("solver::elastic_static"),
            "\"solver::elastic_static\" must be in the v1 opt-in allowlist"
        );
    }

    #[test]
    fn is_opted_in_returns_false_for_modal_and_arbitrary() {
        assert!(
            !is_opted_in("solver::modal"),
            "\"solver::modal\" must NOT be in the opt-in allowlist"
        );
        assert!(
            !is_opted_in("foo::bar"),
            "arbitrary strings must NOT be in the opt-in allowlist"
        );
    }

    // ── Step-3: significance_filter opt-in guard (non-opted-in target) ───────

    /// Pins that an unknown target returns NotOptedIn BEFORE any comparison logic
    /// runs — even when prev/new differ, the filter declines rather than comparing.
    #[test]
    fn significance_filter_returns_not_opted_in_for_unknown_target() {
        let v1 = Value::Real(0.0);
        let v2 = Value::Real(1.0);
        assert_eq!(
            significance_filter("solver::modal", &v1, &v2, Some(1e-6)),
            FilterOutcome::NotOptedIn,
            "\"solver::modal\" is not opted in — filter must return NotOptedIn",
        );
        // Arbitrary string also returns NotOptedIn.
        assert_eq!(
            significance_filter("foo::bar", &v1, &v2, Some(1e-6)),
            FilterOutcome::NotOptedIn,
            "\"foo::bar\" is not opted in — filter must return NotOptedIn",
        );
    }

    // ── Step-5: bit-equality shortcut ────────────────────────────────────────

    /// Bit-equal Values → Equivalent, regardless of Map shape or tolerance.
    /// Pins the shortcut: identical Values short-circuit before any tolerance
    /// branch or Map extraction.
    #[test]
    fn significance_filter_returns_equivalent_for_bit_equal_results() {
        let val = make_elastic_result_value(&[0.0, 0.001], &[0.0, 0.001], 1e8, true, 5);
        assert_eq!(
            significance_filter("solver::elastic_static", &val, &val.clone(), Some(1e-6)),
            FilterOutcome::Equivalent,
            "bit-equal Values must short-circuit to Equivalent before Map extraction",
        );
    }

    // ── Step-9: ε-equivalent displacement delta test ─────────────────────────

    /// Displacement delta within tolerance → Equivalent.
    /// prev.displacement.data = [0.0, 0.001]; new adds 1e-12 to each sample.
    /// tolerance = 1e-6 > 1e-12 → delta is sub-threshold → Equivalent.
    /// Other fields (stress, max_von_mises, converged, iterations) are bit-equal.
    #[test]
    fn significance_filter_returns_equivalent_for_sub_tolerance_displacement_delta() {
        let v1 = make_elastic_result_value(&[0.0, 0.001], &[0.0, 0.001], 1e8, true, 5);
        let v2 =
            make_elastic_result_value(&[0.0 + 1e-12, 0.001 + 1e-12], &[0.0, 0.001], 1e8, true, 5);
        assert_ne!(
            v1, v2,
            "test fixture: v1 and v2 must be distinct (not bit-equal)"
        );
        assert_eq!(
            significance_filter("solver::elastic_static", &v1, &v2, Some(1e-6)),
            FilterOutcome::Equivalent,
            "displacement delta 1e-12 < tolerance 1e-6 must yield Equivalent",
        );
    }

    // ── Step-11: over-tolerance displacement delta ────────────────────────────

    /// Large displacement delta (1.0 m) >>> tolerance (1e-6 m) → Different.
    /// Also pins the strict-greater-than semantics: a delta EQUAL to tol_si
    /// is NOT Different (the comparison is `> tol_si`, not `>= tol_si`).
    #[test]
    fn significance_filter_returns_different_for_over_tolerance_displacement_delta() {
        let v1 = make_elastic_result_value(&[0.0, 0.001], &[0.0, 0.001], 1e8, true, 5);

        // Clear over-threshold: delta = 1.0 m >> tol_si = 1e-6 m.
        let v2 = make_elastic_result_value(&[0.0 + 1.0, 0.001 + 1.0], &[0.0, 0.001], 1e8, true, 5);
        assert_eq!(
            significance_filter("solver::elastic_static", &v1, &v2, Some(1e-6)),
            FilterOutcome::Different,
            "displacement delta 1.0 m >> tol 1e-6 m must yield Different",
        );

        // Boundary: delta == tol_si → Equivalent (strict > semantics).
        let tol = 1e-6_f64;
        let v3 = make_elastic_result_value(&[0.0, 0.001], &[0.0, 0.001], 1e8, true, 5);
        let v4 = make_elastic_result_value(&[0.0 + tol, 0.001], &[0.0, 0.001], 1e8, true, 5);
        assert_eq!(
            significance_filter("solver::elastic_static", &v3, &v4, Some(tol)),
            FilterOutcome::Equivalent,
            "displacement delta == tol_si must be Equivalent (strict-gt: delta > tol is false)",
        );
    }

    // ── Step-13: NaN / ±Inf in displacement always signals Different ──────────

    /// NaN or ±Inf in a displacement sample always yields Different (when prev
    /// and new are not bit-equal). Pins the task spec contract:
    /// "NaN propagates as Different only when prev and new are not bit-equal."
    /// f64::is_finite() uniformly covers NaN, +Inf, and -Inf.
    #[test]
    fn significance_filter_returns_different_for_nan_in_displacement() {
        let v_prev = make_elastic_result_value(&[0.0, 0.0], &[0.0, 0.0], 1e8, true, 5);
        for bad in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            let v_new = make_elastic_result_value(&[0.0, bad], &[0.0, 0.0], 1e8, true, 5);
            assert_eq!(
                significance_filter("solver::elastic_static", &v_prev, &v_new, Some(1e-6)),
                FilterOutcome::Different,
                "non-finite displacement value {bad} must yield Different",
            );
        }
    }

    // ── Step-7: missing-tolerance conservative fallback ───────────────────────

    /// When length_tolerance_si is None, the filter returns Different
    /// (conservative: over-invalidate rather than under-invalidate).
    /// Pins the contract that no per-purpose tolerance → Different regardless
    /// of whether the actual displacement delta would be within any bound.
    #[test]
    fn significance_filter_returns_different_when_tolerance_missing() {
        let v1 = make_elastic_result_value(&[0.0, 0.001], &[0.0, 0.001], 1e8, true, 5);
        // 1 ULP difference in one displacement sample (not bit-equal).
        let disp2 = vec![0.0_f64, f64::from_bits(0.001_f64.to_bits() + 1)];
        let v2 = make_elastic_result_value(&disp2, &[0.0, 0.001], 1e8, true, 5);
        assert_ne!(
            v1, v2,
            "test fixture: v1 and v2 must be distinct (not bit-equal)"
        );
        assert_eq!(
            significance_filter("solver::elastic_static", &v1, &v2, None),
            FilterOutcome::Different,
            "None tolerance must produce Different (conservative fallback)",
        );
    }

    // ── Step-15: non-displacement field changes always yield Different ─────────

    /// Any change to a non-displacement field (stress, max_von_mises, converged,
    /// iterations) must return Different, regardless of displacement delta.
    /// v1 per-field policy: no Pressure tolerance class — exact equality only.
    #[test]
    fn significance_filter_returns_different_for_non_displacement_field_changes() {
        let baseline = make_elastic_result_value(&[0.0, 0.001], &[0.0, 0.001], 1e8, true, 5);

        // stress: different stress data (displacement identical).
        let stress_changed = make_elastic_result_value(&[0.0, 0.001], &[0.0, 1.0], 1e8, true, 5);
        assert_eq!(
            significance_filter(
                "solver::elastic_static",
                &baseline,
                &stress_changed,
                Some(1e-6)
            ),
            FilterOutcome::Different,
            "stress field change must yield Different",
        );

        // max_von_mises: change by 1 ULP.
        let mvm_ulp = f64::from_bits(1e8_f64.to_bits() + 1);
        let mvm_changed = make_elastic_result_value(&[0.0, 0.001], &[0.0, 0.001], mvm_ulp, true, 5);
        assert_eq!(
            significance_filter(
                "solver::elastic_static",
                &baseline,
                &mvm_changed,
                Some(1e-6)
            ),
            FilterOutcome::Different,
            "max_von_mises ULP change must yield Different",
        );

        // converged: true vs false.
        let conv_changed = make_elastic_result_value(&[0.0, 0.001], &[0.0, 0.001], 1e8, false, 5);
        assert_eq!(
            significance_filter(
                "solver::elastic_static",
                &baseline,
                &conv_changed,
                Some(1e-6)
            ),
            FilterOutcome::Different,
            "converged flip (true→false) must yield Different",
        );

        // iterations: 5 vs 6.
        let iters_changed = make_elastic_result_value(&[0.0, 0.001], &[0.0, 0.001], 1e8, true, 6);
        assert_eq!(
            significance_filter(
                "solver::elastic_static",
                &baseline,
                &iters_changed,
                Some(1e-6)
            ),
            FilterOutcome::Different,
            "iterations change (5→6) must yield Different",
        );
    }

    // ── Step-17: malformed Value shapes always yield Different ────────────────

    /// All departures from the documented ElasticResult Map shape must return
    /// Different — never Equivalent. Conservative-fallback policy.
    #[test]
    fn significance_filter_returns_different_for_malformed_shapes() {
        let valid = make_elastic_result_value(&[0.0, 0.001], &[0.0, 0.001], 1e8, true, 5);

        // (a) prev is not a Map at all.
        let not_map = Value::Real(0.0);
        assert_eq!(
            significance_filter("solver::elastic_static", &not_map, &valid, Some(1e-6)),
            FilterOutcome::Different,
            "(a) non-Map prev must yield Different",
        );

        // (b) new is a Map missing the "displacement" key.
        let mut no_disp = BTreeMap::new();
        no_disp.insert(
            Value::String("stress".to_string()),
            make_sampled_field("stress", &[0.0, 0.001]),
        );
        no_disp.insert(Value::String("max_von_mises".to_string()), Value::Real(1e8));
        no_disp.insert(Value::String("converged".to_string()), Value::Bool(true));
        no_disp.insert(Value::String("iterations".to_string()), Value::Int(5));
        let new_no_disp = Value::Map(no_disp);
        assert_eq!(
            significance_filter("solver::elastic_static", &valid, &new_no_disp, Some(1e-6)),
            FilterOutcome::Different,
            "(b) Map missing displacement key must yield Different",
        );

        // (c) new's displacement value is Value::Real (not a Field).
        let mut wrong_disp = BTreeMap::new();
        wrong_disp.insert(Value::String("displacement".to_string()), Value::Real(0.0));
        wrong_disp.insert(
            Value::String("stress".to_string()),
            make_sampled_field("stress", &[0.0, 0.001]),
        );
        wrong_disp.insert(Value::String("max_von_mises".to_string()), Value::Real(1e8));
        wrong_disp.insert(Value::String("converged".to_string()), Value::Bool(true));
        wrong_disp.insert(Value::String("iterations".to_string()), Value::Int(5));
        let new_wrong_disp = Value::Map(wrong_disp);
        assert_eq!(
            significance_filter(
                "solver::elastic_static",
                &valid,
                &new_wrong_disp,
                Some(1e-6)
            ),
            FilterOutcome::Different,
            "(c) displacement = Real (not Field) must yield Different",
        );

        // (d) new's displacement Field has source: Analytical (not Sampled).
        let analytical_field = Value::Field {
            domain_type: reify_core::ty::Type::dimensionless_scalar(),
            codomain_type: reify_core::ty::Type::dimensionless_scalar(),
            source: FieldSourceKind::Analytical,
            lambda: Arc::new(Value::Undef),
        };
        let mut analytical_map = BTreeMap::new();
        analytical_map.insert(Value::String("displacement".to_string()), analytical_field);
        analytical_map.insert(
            Value::String("stress".to_string()),
            make_sampled_field("stress", &[0.0, 0.001]),
        );
        analytical_map.insert(Value::String("max_von_mises".to_string()), Value::Real(1e8));
        analytical_map.insert(Value::String("converged".to_string()), Value::Bool(true));
        analytical_map.insert(Value::String("iterations".to_string()), Value::Int(5));
        let new_analytical = Value::Map(analytical_map);
        assert_eq!(
            significance_filter(
                "solver::elastic_static",
                &valid,
                &new_analytical,
                Some(1e-6)
            ),
            FilterOutcome::Different,
            "(d) displacement Field with Analytical source must yield Different",
        );

        // (e) displacement data vectors have mismatched lengths (3 vs 4).
        let v_len3 = make_elastic_result_value(&[0.0, 0.001, 0.002], &[0.0, 0.001], 1e8, true, 5);
        let v_len4 =
            make_elastic_result_value(&[0.0, 0.001, 0.002, 0.003], &[0.0, 0.001], 1e8, true, 5);
        assert_eq!(
            significance_filter("solver::elastic_static", &v_len3, &v_len4, Some(1e-6)),
            FilterOutcome::Different,
            "(e) mismatched displacement data lengths must yield Different",
        );
    }

    // ── Step-19: bit-equality shortcut precedes ALL other guards ─────────────

    /// bit-equal values must return Equivalent even when tolerance is 0.0 or
    /// None — the shortcut runs immediately after the opt-in guard, before the
    /// tolerance guard. Pins the documented invariant:
    /// "bit-equality declares Equivalent regardless of whether tolerance is supplied."
    #[test]
    fn significance_filter_does_not_false_positive_on_bit_equal_with_zero_tolerance() {
        let v = make_elastic_result_value(&[0.0, 0.001], &[0.0, 0.001], 1e8, true, 5);

        // Some(0.0) tolerance — shortcut must run before the tolerance gate.
        assert_eq!(
            significance_filter("solver::elastic_static", &v, &v.clone(), Some(0.0)),
            FilterOutcome::Equivalent,
            "bit-equal values must be Equivalent even when tolerance is Some(0.0)",
        );

        // None tolerance — shortcut must run before the tolerance guard.
        assert_eq!(
            significance_filter("solver::elastic_static", &v, &v.clone(), None),
            FilterOutcome::Equivalent,
            "bit-equal values must be Equivalent even when tolerance is None",
        );
    }

    // ── Amendment: grid-metadata inequality always yields Different ──────────

    /// If two displacement SampledFields have identical data values within
    /// tol_si but different axis_grids/bounds, the filter must return Different.
    /// Identical data at different physical locations is a semantically material
    /// change — same conservative posture as the missing-tolerance fallback.
    ///
    /// Pins the `SampledField::grid_metadata_eq` check added in the amendment
    /// pass: the filter compares grid geometry (kind, name, bounds, spacing,
    /// axis_grids, interpolation) before the element-wise data comparison, so
    /// a shifted grid cannot sneak through as Equivalent.
    #[test]
    fn significance_filter_returns_different_for_shifted_grid_metadata() {
        // Build a displacement field with a spatially-shifted grid.
        fn make_shifted_disp(data: &[f64], grid_offset: f64) -> Value {
            let n = data.len();
            Value::Field {
                domain_type: Type::dimensionless_scalar(),
                codomain_type: Type::dimensionless_scalar(),
                source: FieldSourceKind::Sampled,
                lambda: Arc::new(Value::SampledField(SampledField {
                    name: "displacement".to_string(),
                    kind: SampledGridKind::Regular1D,
                    bounds_min: vec![grid_offset],
                    bounds_max: vec![grid_offset + 1.0],
                    spacing: vec![if n > 1 { 1.0 / (n as f64 - 1.0) } else { 1.0 }],
                    axis_grids: vec![(0..n).map(|i| grid_offset + i as f64).collect()],
                    interpolation: InterpolationKind::Linear,
                    data: data.to_vec(),
                    oob_emitted: AtomicBool::new(false),
                })),
            }
        }

        // prev: displacement data [0.0, 1e-12], grid at offset 0.0
        // new:  displacement data [0.0, 2e-12], grid at offset 1.0
        // data delta (1e-12) is well below tol_si (1e-6), so without the
        // grid-metadata check the filter would incorrectly declare Equivalent.
        let mut map_prev = BTreeMap::new();
        map_prev.insert(
            Value::String("displacement".to_string()),
            make_shifted_disp(&[0.0, 1e-12], 0.0),
        );
        map_prev.insert(
            Value::String("stress".to_string()),
            make_sampled_field("stress", &[0.0, 0.001]),
        );
        map_prev.insert(Value::String("max_von_mises".to_string()), Value::Real(1e8));
        map_prev.insert(Value::String("converged".to_string()), Value::Bool(true));
        map_prev.insert(Value::String("iterations".to_string()), Value::Int(5));
        let v_prev = Value::Map(map_prev);

        let mut map_new = BTreeMap::new();
        map_new.insert(
            Value::String("displacement".to_string()),
            // Same data length, sub-tolerance data delta, but axis_grids/bounds shifted.
            make_shifted_disp(&[0.0, 2e-12], 1.0),
        );
        map_new.insert(
            Value::String("stress".to_string()),
            make_sampled_field("stress", &[0.0, 0.001]),
        );
        map_new.insert(Value::String("max_von_mises".to_string()), Value::Real(1e8));
        map_new.insert(Value::String("converged".to_string()), Value::Bool(true));
        map_new.insert(Value::String("iterations".to_string()), Value::Int(5));
        let v_new = Value::Map(map_new);

        // Sanity: confirm data delta is sub-tolerance (only grid metadata differs).
        assert!(
            (1e-12_f64 - 2e-12_f64).abs() < 1e-6,
            "fixture: data delta must be sub-tolerance so only grid metadata triggers Different"
        );
        assert_ne!(v_prev, v_new, "fixture: values must be distinct");

        assert_eq!(
            significance_filter("solver::elastic_static", &v_prev, &v_new, Some(1e-6)),
            FilterOutcome::Different,
            "different axis_grids/bounds must yield Different even when data delta < tol_si",
        );
    }
}
