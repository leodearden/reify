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
    let _ = (prev, new, length_tolerance_si);
    todo!("significance_filter body: remaining branches land in steps 6, 8, 10")
}

#[cfg(test)]
mod tests {
    use super::{FilterOutcome, is_opted_in, significance_filter};

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
        let v1 = reify_types::Value::Real(0.0);
        let v2 = reify_types::Value::Real(1.0);
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
}
