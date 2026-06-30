//! A-posteriori adaptive refinement loop control + budget enforcement.
//!
//! PRD reference: `docs/prds/v0_4/a-posteriori-error-estimation.md`
//! (Task decomposition #2, task 2997).
//!
//! This module implements the v0.4 a-posteriori outer refinement loop —
//! `solve → estimate → mark → refine → re-solve` — with three budget knobs
//! ("any of these stops it"), Dörfler bulk marking (θ = 0.5), and a
//! `>10%`-stall-drop termination rule, plus the `ConvergenceStatus` /
//! `BudgetReason` termination-reason bookkeeping this task OWNS.
//!
//! # Distinct from `progressive`
//!
//! [`crate::progressive`] (v0.3 task #15) is a DIFFERENT refinement scheme — a
//! `mesh_tol`/`cg_tol` pass schedule with yield-proximity auto-refine, carrying
//! its own `TerminationReason`/`AdvanceDecision` vocabulary. This module is the
//! distinct v0.4 a-posteriori Dörfler + Z-Z + budget + stall model with its own
//! [`ConvergenceStatus`]/[`BudgetReason`] vocabulary (mirroring the DSL enum
//! from task 2998). The two termination models are NOT interchangeable.
//!
//! # Kernel-form primitives; eval threading deferred
//!
//! Following the crate convention ([`crate::error_estimator`],
//! [`crate::volume_refine`], [`crate::progressive`]): this module ships
//! plain-`f64` kernel-form primitives. The `reify_ir::Value::Enum` bridge that
//! maps a Rust [`ConvergenceStatus`] into the DSL enum, and running the loop
//! inside reify-eval's elastic-static compute target, are OUT OF SCOPE here
//! (mirroring the `progressive` → engine-integration split). The Rust enums
//! mirror the DSL variant/payload-field names exactly so the future bridge is
//! mechanical.

/// Canonical Dörfler bulk-marking fraction θ = 0.5 (the task default).
///
/// "Mark the smallest set of elements whose summed indicators reach half the
/// global indicator." Pass this to [`mark_dorfler`] / [`run_adaptive_refinement`]
/// unless a caller overrides it.
pub const DORFLER_THETA: f64 = 0.5;

/// Dörfler ("bulk") marking: select the smallest set of elements whose summed
/// indicators reach `theta` × the total indicator.
///
/// # Algorithm
///
/// 1. `total = Σ_e indicators[e]`.
/// 2. Visit elements in order of indicator **descending**, ties broken by
///    **index ascending** (so the marked set is bit-deterministic).
/// 3. Accumulate from the largest, marking each visited element, and stop as
///    soon as the running sum reaches `theta * total`.
/// 4. Return the marked indices sorted **ascending**.
///
/// # Edge cases
///
/// An empty slice and an all-zero indicator vector both return an empty `Vec`:
/// when `total == 0` the threshold is `0`, and the empty set already satisfies
/// `cumulative(0) >= theta * 0`, so no element is marked. A zero-error field
/// (e.g. the Zienkiewicz patch test) therefore triggers no wasted refinement,
/// consistent with [`crate::error_estimator`]'s zero-energy guard.
pub fn mark_dorfler(indicators: &[f64], theta: f64) -> Vec<usize> {
    let total: f64 = indicators.iter().sum();
    let threshold = theta * total;

    // Indices sorted by (indicator desc, index asc) — a total order, so the
    // result is deterministic regardless of sort stability.
    let mut order: Vec<usize> = (0..indicators.len()).collect();
    order.sort_by(|&a, &b| {
        indicators[b]
            .partial_cmp(&indicators[a])
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(a.cmp(&b))
    });

    let mut cumulative = 0.0_f64;
    let mut marked: Vec<usize> = Vec::new();
    for &idx in &order {
        if cumulative >= threshold {
            break;
        }
        cumulative += indicators[idx];
        marked.push(idx);
    }

    marked.sort_unstable();
    marked
}

/// Minimum relative drop in the global error indicator required **between**
/// successive refinement iterations for the loop to keep going. A drop of this
/// fraction or less is treated as insufficient progress (a stall).
pub const STALL_MIN_RELATIVE_DROP: f64 = 0.10;

/// Stall detection: returns `true` when the relative drop in the global error
/// indicator from `prev_global` to `curr_global` is **at most**
/// [`STALL_MIN_RELATIVE_DROP`] (10%) — i.e. the loop stopped making enough
/// progress to justify another refinement.
///
/// # Boundary semantics
///
/// The rule requires a drop of *strictly more than* 10% to continue, so an
/// **exactly** 10% drop counts as stalled. Encoded as
/// `curr_global >= (1 - STALL_MIN_RELATIVE_DROP) * prev_global`, which avoids a
/// division (no `prev == 0` hazard: `prev` is only compared after a
/// non-converged iteration, where `global > target_accuracy > 0`). A grown
/// indicator (`curr > prev`) is likewise stalled.
pub fn is_stalled(prev_global: f64, curr_global: f64) -> bool {
    curr_global >= (1.0 - STALL_MIN_RELATIVE_DROP) * prev_global
}

#[cfg(test)]
mod tests {
    use super::*;

    // -----------------------------------------------------------------------
    // step-1: mark_dorfler — Dörfler bulk marking
    // -----------------------------------------------------------------------

    #[test]
    fn mark_dorfler_half_marks_largest_until_half_total() {
        // total = 10, threshold = 0.5 * 10 = 5.0.
        // Accumulate descending: 4 (<5), 4+3 = 7 (>=5) ⇒ mark {3, 2}.
        let marked = mark_dorfler(&[1.0, 2.0, 3.0, 4.0], 0.5);
        assert_eq!(marked, vec![2, 3], "θ=0.5 marks the two largest, sorted asc");
    }

    #[test]
    fn mark_dorfler_theta_one_marks_all() {
        // threshold = 10.0; must accumulate every element to reach it.
        let marked = mark_dorfler(&[1.0, 2.0, 3.0, 4.0], 1.0);
        assert_eq!(marked, vec![0, 1, 2, 3], "θ=1.0 marks all indices");
    }

    #[test]
    fn mark_dorfler_small_theta_marks_only_top() {
        // threshold = 0.3 * 10 = 3.0; the single largest (4.0) already clears it.
        let marked = mark_dorfler(&[1.0, 2.0, 3.0, 4.0], 0.3);
        assert_eq!(marked, vec![3], "θ=0.3 marks only the largest element");
    }

    #[test]
    fn mark_dorfler_tie_break_is_index_ascending() {
        // total = 5, threshold = 2.5. Two equal 2.0 leaders: desc-by-value then
        // index-asc visits index 0 then 1. 2.0 (<2.5), 2.0+2.0 = 4.0 (>=2.5).
        let marked = mark_dorfler(&[2.0, 2.0, 1.0], 0.5);
        assert_eq!(marked, vec![0, 1], "ties break by index ascending");
    }

    #[test]
    fn mark_dorfler_empty_slice_marks_nothing() {
        let marked = mark_dorfler(&[], 0.5);
        assert!(marked.is_empty(), "empty input ⇒ empty marked set");
    }

    #[test]
    fn mark_dorfler_all_zero_marks_nothing() {
        // total = 0; the empty set satisfies cumulative(0) >= θ*0 = 0 ⇒ no
        // wasted refinement on a zero-error field.
        let marked = mark_dorfler(&[0.0, 0.0, 0.0], 0.5);
        assert!(marked.is_empty(), "all-zero indicators ⇒ empty marked set");
    }

    // -----------------------------------------------------------------------
    // step-3: is_stalled — >10%-drop-required termination
    // -----------------------------------------------------------------------

    #[test]
    fn is_stalled_exactly_ten_percent_drop_is_stalled() {
        // A 10% drop is NOT more than 10%, so it is insufficient ⇒ stalled.
        assert!(is_stalled(1.0, 0.9), "exactly 10% drop counts as stalled");
    }

    #[test]
    fn is_stalled_eleven_percent_drop_continues() {
        // 11% > 10% ⇒ enough progress to keep refining.
        assert!(!is_stalled(1.0, 0.89), "11% drop is not stalled");
    }

    #[test]
    fn is_stalled_fifty_percent_drop_continues() {
        assert!(!is_stalled(1.0, 0.5), "50% drop is healthy progress");
    }

    #[test]
    fn is_stalled_no_drop_is_stalled() {
        assert!(is_stalled(1.0, 1.0), "no improvement ⇒ stalled");
    }

    #[test]
    fn is_stalled_indicator_grew_is_stalled() {
        assert!(is_stalled(1.0, 1.2), "indicator grew ⇒ stalled");
    }

    // -----------------------------------------------------------------------
    // step-5: dorfler_size_hints — canonical h/2 refinement
    // -----------------------------------------------------------------------

    #[test]
    fn dorfler_size_hints_halves_single_marked_element() {
        let hints = dorfler_size_hints(&[1], &[1.0, 1.0, 1.0]);
        assert_eq!(hints, vec![1.0, 0.5, 1.0], "only the marked element halves");
    }

    #[test]
    fn dorfler_size_hints_empty_marked_keeps_all_sizes() {
        let hints = dorfler_size_hints(&[], &[1.0, 1.0, 1.0]);
        assert_eq!(hints, vec![1.0, 1.0, 1.0], "no marks ⇒ sizes unchanged");
    }

    #[test]
    fn dorfler_size_hints_all_marked_halves_all() {
        let hints = dorfler_size_hints(&[0, 1, 2], &[1.0, 1.0, 1.0]);
        assert_eq!(hints, vec![0.5, 0.5, 0.5], "every element halves");
    }

    #[test]
    fn dorfler_size_hints_respects_nonuniform_current_sizes() {
        let hints = dorfler_size_hints(&[0], &[0.8, 0.4]);
        assert_eq!(hints, vec![0.4, 0.4], "marked 0.8 → 0.4; unmarked 0.4 kept");
    }
}
