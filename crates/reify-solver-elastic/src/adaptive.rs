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

// ---------------------------------------------------------------------------
// Termination-reason data model (mirrors solver_elastic.ri exactly).
// ---------------------------------------------------------------------------

/// Why an a-posteriori adaptive refinement loop stopped before reaching its
/// accuracy target.
///
/// Mirrors the DSL `enum BudgetReason` in
/// `crates/reify-compiler/stdlib/solver_elastic.ri` — the variant set and the
/// canonical order `[TargetMissed, MaxIterations, MaxDofs, Stalled]` match
/// exactly so the future `reify_ir::Value::Enum` bridge is mechanical.
#[derive(Debug, Clone, PartialEq)]
pub enum BudgetReason {
    /// Budget exhausted with the error estimate still above `target_accuracy`.
    TargetMissed,
    /// Hit the `max_refinement_iterations` cap.
    MaxIterations,
    /// The next refinement would exceed the `max_dofs` cap.
    MaxDofs,
    /// The global error indicator stopped improving iteration-over-iteration.
    Stalled,
}

/// The a-posteriori solve's confidence signal.
///
/// Mirrors the DSL `enum ConvergenceStatus` (a data-carrying enum, DCE) in
/// `solver_elastic.ri`: the variant names and the `final_indicator` payload
/// field name match exactly so the future eval `Value::Enum` bridge is a 1:1
/// mapping with no renaming.
#[derive(Debug, Clone, PartialEq)]
pub enum ConvergenceStatus {
    /// The solve reached its accuracy target; `final_indicator` carries the
    /// final global relative energy-norm error (dimensionless).
    Converged { final_indicator: f64 },
    /// The adaptive loop stopped before hitting the target; `reason` explains
    /// why (budget cap or stall). Per the PRD this is a warning + downgraded
    /// confidence, never a hard error.
    NotConverged { reason: BudgetReason },
}

/// The three budget knobs that bound the adaptive loop ("any of these stops
/// it"), mirroring `ElasticOptions` in `solver_elastic.ri`.
#[derive(Debug, Clone, PartialEq)]
pub struct RefinementBudget {
    /// Relative energy-norm error target; the loop converges once the global
    /// indicator is `<=` this value. (`ElasticOptions.target_accuracy`.)
    pub target_accuracy: f64,
    /// Upper bound on refinement iterations. `0` is legitimate (one solve, no
    /// refinement). (`ElasticOptions.max_refinement_iterations`.)
    pub max_refinement_iterations: usize,
    /// Degrees-of-freedom budget cap. (`ElasticOptions.max_dofs`.)
    pub max_dofs: usize,
}

/// One iteration's solve-and-estimate output.
///
/// Mirrors [`crate::error_estimator::ZzIndicator`]: `global_indicator` is the
/// `global_relative_energy_error` (compared against `target_accuracy` and used
/// for stall detection) and `per_element` feeds [`mark_dorfler`]. `n_dofs` is
/// the current mesh's degree-of-freedom count for the `max_dofs` budget gate.
#[derive(Debug, Clone, PartialEq)]
pub struct AdaptiveEstimate {
    /// Global relative energy-norm error of the current solve.
    pub global_indicator: f64,
    /// Per-element error indicator η_e (element order), the Dörfler input.
    pub per_element: Vec<f64>,
    /// Degrees of freedom of the current mesh.
    pub n_dofs: usize,
}

/// Dependency-injection seam for the adaptive refinement loop.
///
/// [`run_adaptive_refinement`] drives an implementor through
/// `solve → estimate → mark → refine → re-solve` without knowing how a solve
/// or a refine is performed. The real implementation wires a CG solve +
/// [`crate::error_estimator::compute_zz_indicator`] into `solve_and_estimate`
/// and [`refine_marked_elements`] into `refine`; the test suite supplies
/// deterministic synthetic stubs. This decoupling is what lets the loop
/// control be exercised with the task's "stub indicator + refiner" strategy,
/// independent of the heavy solve pipeline.
pub trait AdaptiveProblem {
    /// Error type returned by [`refine`](AdaptiveProblem::refine) (e.g.
    /// [`crate::volume_refine::RefineError`], or `Infallible` for stubs).
    type Error;

    /// Solve on the current mesh and return the a-posteriori estimate.
    fn solve_and_estimate(&mut self) -> AdaptiveEstimate;

    /// Refine the mesh, targeting the Dörfler-`marked` elements.
    fn refine(&mut self, marked: &[usize]) -> Result<(), Self::Error>;
}

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

/// Canonical h/2 size hints for a Dörfler-marked element set.
///
/// Returns one target characteristic size per element (in element order): each
/// `marked` element has its current size halved (h → h/2, the canonical
/// uniform refinement of a marked element), every unmarked element keeps its
/// current size. The returned `Vec` therefore feeds directly into
/// [`crate::volume_refine::refine_with_size_field`] (one hint per element).
///
/// `marked` is expected to hold distinct in-range indices (as produced by
/// [`mark_dorfler`]); each listed index halves the corresponding entry.
pub fn dorfler_size_hints(marked: &[usize], current_sizes: &[f64]) -> Vec<f64> {
    let mut sizes = current_sizes.to_vec();
    for &idx in marked {
        sizes[idx] *= 0.5;
    }
    sizes
}

/// Drive the a-posteriori adaptive refinement loop over an injected
/// [`AdaptiveProblem`]:
/// `solve → estimate → target-check → mark → refine → re-solve`.
///
/// Each iteration solves on the current mesh, and if the global indicator has
/// reached `budget.target_accuracy` returns
/// [`ConvergenceStatus::Converged`]. Otherwise it Dörfler-marks the per-element
/// indicators (with fraction `theta`) and refines, then re-solves.
///
/// The budget/stall termination gates are layered in by step-12; this initial
/// form only handles the converging path.
pub fn run_adaptive_refinement<P: AdaptiveProblem>(
    problem: &mut P,
    budget: &RefinementBudget,
    theta: f64,
) -> Result<ConvergenceStatus, P::Error> {
    loop {
        let est = problem.solve_and_estimate();
        if est.global_indicator <= budget.target_accuracy {
            return Ok(ConvergenceStatus::Converged {
                final_indicator: est.global_indicator,
            });
        }
        let marked = mark_dorfler(&est.per_element, theta);
        problem.refine(&marked)?;
    }
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

    // -----------------------------------------------------------------------
    // step-7: ConvergenceStatus / BudgetReason data-model contract.
    //
    // These mirror the DSL enums in solver_elastic.ri exactly (variant names +
    // the `final_indicator` payload field) so the future eval Value::Enum
    // bridge is a mechanical 1:1 mapping. A rename here is a deliberate ABI
    // change, surfaced by these pins.
    // -----------------------------------------------------------------------

    #[test]
    fn convergence_status_converged_binds_final_indicator() {
        let status = ConvergenceStatus::Converged { final_indicator: 0.04 };
        match status {
            ConvergenceStatus::Converged { final_indicator } => {
                assert_eq!(final_indicator, 0.04, "Converged binds final_indicator");
            }
            ConvergenceStatus::NotConverged { .. } => panic!("expected Converged"),
        }
    }

    #[test]
    fn convergence_status_notconverged_carries_budget_reason() {
        let status = ConvergenceStatus::NotConverged {
            reason: BudgetReason::Stalled,
        };
        match status {
            ConvergenceStatus::NotConverged { reason } => {
                assert_eq!(reason, BudgetReason::Stalled, "NotConverged carries reason");
            }
            ConvergenceStatus::Converged { .. } => panic!("expected NotConverged"),
        }
    }

    #[test]
    fn budget_reason_has_all_four_variants() {
        // Construct each variant so a removed/renamed variant trips compilation.
        let variants = [
            BudgetReason::TargetMissed,
            BudgetReason::MaxIterations,
            BudgetReason::MaxDofs,
            BudgetReason::Stalled,
        ];
        // PartialEq + distinctness: a variant equals only itself.
        assert_eq!(variants[0], BudgetReason::TargetMissed);
        assert_ne!(variants[0], variants[1]);
        // Debug is non-empty for every variant.
        for v in &variants {
            assert!(!format!("{v:?}").is_empty(), "Debug must be non-empty");
        }
    }

    #[test]
    fn convergence_status_derives_clone_partialeq_debug() {
        let a = ConvergenceStatus::Converged { final_indicator: 0.04 };
        let b = a.clone();
        assert_eq!(a, b, "Clone + PartialEq round-trip on two equal values");
        assert!(!format!("{a:?}").is_empty(), "Debug must be non-empty");
    }
}
