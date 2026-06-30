//! Integration tests for `reify_solver_elastic::adaptive` — the a-posteriori
//! refinement loop driver.
//!
//! These tests drive [`run_adaptive_refinement`] through a configurable
//! [`AdaptiveProblem`] stub that yields a scripted sequence of
//! [`AdaptiveEstimate`]s and records the marked slices passed to `refine`. No
//! gmsh, no real solve pipeline — the dependency-injection seam lets the loop
//! control be exercised deterministically (the task's "stub indicator +
//! refiner" strategy).

use reify_solver_elastic::{
    AdaptiveEstimate, AdaptiveProblem, BudgetReason, ConvergenceStatus, DORFLER_THETA,
    RefinementBudget, mark_dorfler, run_adaptive_refinement,
};

// ---------------------------------------------------------------------------
// Configurable AdaptiveProblem stub
// ---------------------------------------------------------------------------

/// A scripted [`AdaptiveProblem`]: `solve_and_estimate` returns the next
/// `AdaptiveEstimate` from `estimates` (panicking if the driver over-consumes),
/// and `refine` records the marked slice it was handed. Never errors.
struct StubProblem {
    estimates: Vec<AdaptiveEstimate>,
    next: usize,
    refine_calls: Vec<Vec<usize>>,
}

impl StubProblem {
    fn new(estimates: Vec<AdaptiveEstimate>) -> Self {
        Self {
            estimates,
            next: 0,
            refine_calls: Vec::new(),
        }
    }
}

impl AdaptiveProblem for StubProblem {
    type Error = std::convert::Infallible;

    fn solve_and_estimate(&mut self) -> AdaptiveEstimate {
        let est = self
            .estimates
            .get(self.next)
            .unwrap_or_else(|| {
                panic!(
                    "stub exhausted: solve #{} beyond the {} scripted estimates \
                     (driver looped past the script — a termination-gate bug)",
                    self.next,
                    self.estimates.len(),
                )
            })
            .clone();
        self.next += 1;
        est
    }

    fn refine(&mut self, marked: &[usize]) -> Result<(), Self::Error> {
        self.refine_calls.push(marked.to_vec());
        Ok(())
    }
}

/// Build an `AdaptiveEstimate` with a fixed non-trivial per-element vector
/// (so [`mark_dorfler`] marks a real subset) for the budget-gate scripts.
fn est(global_indicator: f64, n_dofs: usize) -> AdaptiveEstimate {
    AdaptiveEstimate {
        global_indicator,
        per_element: vec![1.0, 2.0, 3.0, 4.0],
        n_dofs,
    }
}

// ---------------------------------------------------------------------------
// step-9: happy path — converge after one refine
// ---------------------------------------------------------------------------

#[test]
fn happy_path_converges_after_one_refine() {
    let iter0_per_element = vec![1.0, 2.0, 3.0, 4.0];
    let mut stub = StubProblem::new(vec![
        // iter 0: above target ⇒ mark + refine.
        AdaptiveEstimate {
            global_indicator: 0.5,
            per_element: iter0_per_element.clone(),
            n_dofs: 100,
        },
        // iter 1: re-solve is at/below target ⇒ Converged.
        AdaptiveEstimate {
            global_indicator: 0.04,
            per_element: vec![0.01, 0.01],
            n_dofs: 200,
        },
    ]);
    let budget = RefinementBudget {
        target_accuracy: 0.05,
        max_refinement_iterations: 5,
        max_dofs: 1_000_000,
    };

    let status =
        run_adaptive_refinement(&mut stub, &budget, DORFLER_THETA).expect("stub never errors");

    match status {
        ConvergenceStatus::Converged { final_indicator } => {
            assert_eq!(
                final_indicator, 0.04,
                "final_indicator is the converged second solve's global indicator"
            );
        }
        other => panic!("expected Converged, got {other:?}"),
    }

    // Exactly one mark + refine ran before the re-solve converged.
    assert_eq!(stub.refine_calls.len(), 1, "exactly one refine occurred");
    assert_eq!(
        stub.refine_calls[0],
        mark_dorfler(&iter0_per_element, DORFLER_THETA),
        "refine targets the Dörfler-marked set of iteration 0",
    );
}

// ---------------------------------------------------------------------------
// step-11: one test per BudgetReason — non-overlapping stub scripts that each
// isolate exactly one termination trigger.
// ---------------------------------------------------------------------------

#[test]
fn max_iterations_fires_after_iter_cap() {
    // Strictly improving (each drop > 10% ⇒ never stalls), target unreachable,
    // dofs never near the cap ⇒ only the iteration cap can stop the loop.
    let mut stub = StubProblem::new(vec![est(0.5, 100), est(0.4, 100), est(0.3, 100)]);
    let budget = RefinementBudget {
        target_accuracy: 0.001,
        max_refinement_iterations: 2,
        max_dofs: 1_000_000_000,
    };

    let status = run_adaptive_refinement(&mut stub, &budget, DORFLER_THETA).unwrap();

    assert_eq!(
        status,
        ConvergenceStatus::NotConverged {
            reason: BudgetReason::MaxIterations
        },
    );
    // Two refines (iter 0 and 1) ran before the cap fired at iter 2.
    assert_eq!(stub.refine_calls.len(), 2, "two refines before the iter cap");
}

#[test]
fn max_dofs_fires_when_dofs_reach_cap() {
    // Improving + non-stalling, target unreachable, iter cap huge ⇒ the dof
    // ceiling is the only gate that can fire (n_dofs 2000 >= 1000 at iter 2).
    let mut stub = StubProblem::new(vec![est(0.5, 100), est(0.4, 500), est(0.3, 2000)]);
    let budget = RefinementBudget {
        target_accuracy: 0.001,
        max_refinement_iterations: 100,
        max_dofs: 1000,
    };

    let status = run_adaptive_refinement(&mut stub, &budget, DORFLER_THETA).unwrap();

    assert_eq!(
        status,
        ConvergenceStatus::NotConverged {
            reason: BudgetReason::MaxDofs
        },
    );
}

#[test]
fn stalled_fires_on_insufficient_drop() {
    // 0.5 → 0.48 is a 4% drop (<= 10%) ⇒ stall on the second solve; caps are
    // far away so only the stall gate can fire.
    let mut stub = StubProblem::new(vec![est(0.5, 100), est(0.48, 100)]);
    let budget = RefinementBudget {
        target_accuracy: 0.001,
        max_refinement_iterations: 100,
        max_dofs: 1_000_000,
    };

    let status = run_adaptive_refinement(&mut stub, &budget, DORFLER_THETA).unwrap();

    assert_eq!(
        status,
        ConvergenceStatus::NotConverged {
            reason: BudgetReason::Stalled
        },
    );
    // One refine ran (iter 0); the stall is detected at iter 1 before re-marking.
    assert_eq!(stub.refine_calls.len(), 1, "one refine before the stall");
}

#[test]
fn target_reached_wins_over_simultaneous_caps() {
    // Precedence: at iter 0 the target is already met (0.04 <= 0.05) AND both
    // caps are "hit" (max_refinement_iterations 0; n_dofs 100 >= max_dofs 50).
    // Target must win ⇒ Converged, never NotConverged.
    let mut stub = StubProblem::new(vec![est(0.04, 100)]);
    let budget = RefinementBudget {
        target_accuracy: 0.05,
        max_refinement_iterations: 0,
        max_dofs: 50,
    };

    let status = run_adaptive_refinement(&mut stub, &budget, DORFLER_THETA).unwrap();

    assert_eq!(
        status,
        ConvergenceStatus::Converged {
            final_indicator: 0.04
        },
    );
    assert_eq!(stub.refine_calls.len(), 0, "converged immediately, no refine");
}
