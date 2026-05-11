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
//! Current v1 allowlist: `"solver::elastic_static"` only.
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
//!
//! # Integration contract
//!
//! `length_tolerance_si: Option<f64>` is resolved by the caller via
//! `Engine::active_tolerance_for(subject_entity_ref)` (task 3382, P3.3).
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
/// v1 allowlist: only `"solver::elastic_static"` opts in. Switching to a
/// registry-based or annotation-driven mechanism is a non-breaking internal
/// refactor — the function signature absorbs the lookup mechanism.
pub fn is_opted_in(target: &str) -> bool {
    matches!(target, "solver::elastic_static")
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
pub fn significance_filter(
    target: &str,
    prev: &reify_types::Value,
    new: &reify_types::Value,
    length_tolerance_si: Option<f64>,
) -> FilterOutcome {
    // Opt-in guard: unknown targets never reach comparison logic.
    if !is_opted_in(target) {
        return FilterOutcome::NotOptedIn;
    }

    // Bit-equality shortcut: identical Values are Equivalent without consulting
    // the Map shape or tolerance. This is the steady-state-cheap path and the
    // ONLY path that can declare Equivalent without consulting length_tolerance_si.
    // Relies on Value::PartialEq (value.rs:1573).
    // Exercises: significance_filter_returns_equivalent_for_bit_equal_results
    //            significance_filter_does_not_false_positive_on_bit_equal_with_zero_tolerance
    if prev == new {
        return FilterOutcome::Equivalent;
    }

    let _ = length_tolerance_si;
    todo!("significance_filter body: tolerance guard + Map extraction land in steps 8, 10")
}

#[cfg(test)]
mod tests {
    use super::{FilterOutcome, is_opted_in, significance_filter};
    use reify_types::{FieldSourceKind, InterpolationKind, SampledField, SampledGridKind, Type, Value};
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
            domain_type: Type::Real,
            codomain_type: Type::Real,
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
        map.insert(Value::String("max_von_mises".to_string()), Value::Real(max_vm));
        map.insert(Value::String("converged".to_string()), Value::Bool(converged));
        map.insert(Value::String("iterations".to_string()), Value::Int(iters as i64));
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
        assert_ne!(v1, v2, "test fixture: v1 and v2 must be distinct (not bit-equal)");
        assert_eq!(
            significance_filter("solver::elastic_static", &v1, &v2, None),
            FilterOutcome::Different,
            "None tolerance must produce Different (conservative fallback)",
        );
    }
}
