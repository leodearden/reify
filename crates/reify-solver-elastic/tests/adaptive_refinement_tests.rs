//! Integration tests for `reify_solver_elastic::adaptive` — the a-posteriori
//! refinement loop driver.
//!
//! These tests drive [`run_adaptive_refinement`] through a configurable
//! [`AdaptiveProblem`] stub that yields a scripted sequence of
//! [`AdaptiveEstimate`]s and records the marked slices passed to `refine`. No
//! gmsh, no real solve pipeline — the dependency-injection seam lets the loop
//! control be exercised deterministically (the task's "stub indicator +
//! refiner" strategy).

use reify_ir::{ElementOrderTag, Mesh, VolumeConnectivity, VolumeMesh};
use reify_kernel_gmsh::MeshingOptions;
use reify_solver_elastic::volume_refine::RefineError;
use reify_solver_elastic::{
    AdaptiveEstimate, AdaptiveProblem, BudgetReason, ConvergenceStatus, DORFLER_THETA,
    RefinementBudget, mark_dorfler, refine_marked_elements, run_adaptive_refinement,
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

    fn solve_and_estimate(&mut self) -> Result<AdaptiveEstimate, Self::Error> {
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
        Ok(est)
    }

    fn refine(&mut self, marked: &[usize]) -> Result<(), Self::Error> {
        self.refine_calls.push(marked.to_vec());
        Ok(())
    }
}

/// Build an `AdaptiveEstimate` with a fixed non-trivial per-element vector
/// (so [`mark_dorfler`] marks a real subset) for the budget-gate scripts.
fn est(relative_error: f64, n_dofs: usize) -> AdaptiveEstimate {
    AdaptiveEstimate {
        relative_error,
        per_element: vec![1.0, 2.0, 3.0, 4.0],
        n_dofs,
        qoi: None,
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
            relative_error: 0.5,
            per_element: iter0_per_element.clone(),
            n_dofs: 100,
            qoi: None,
        },
        // iter 1: re-solve is at/below target ⇒ Converged.
        AdaptiveEstimate {
            relative_error: 0.04,
            per_element: vec![0.01, 0.01],
            n_dofs: 200,
            qoi: None,
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
    // 0.5 → 0.48 is a 4% drop (<= 10%) ⇒ stall on the second solve. Caps are
    // far away, so budget remains: the case stall exists for — stopping early.
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

/// At iter 1 the stall gate and the iteration cap are live SIMULTANEOUSLY; the
/// cap wins. Stall is an early-stop rule, and here no further refine was
/// possible anyway, so the cap is the cause of termination. See
/// `docs/prds/v0_4/a-posteriori-error-estimation.md`, Resolved decisions →
/// Budget knobs amendment (task 7449).
#[test]
fn the_iteration_cap_outranks_a_simultaneous_stall() {
    // 0.5 → 0.48 is a 4% drop, so at iter 1 the stall gate is live
    // (0.48 >= 0.9 * 0.5 = 0.45) — and so is the iteration cap (iter 1 >= 1).
    // Deliberately NOT the exact-10% boundary: those float semantics are pinned
    // orthogonally by `is_stalled_exactly_ten_percent_drop_is_stalled`, and
    // mixing the two would couple a precedence claim to an FP-representation one.
    let mut stub = StubProblem::new(vec![est(0.5, 100), est(0.48, 100)]);
    let budget = RefinementBudget {
        target_accuracy: 0.001,
        max_refinement_iterations: 1, // <- the cap is ALSO hit at iter 1
        max_dofs: 1_000_000_000,
    };

    let status = run_adaptive_refinement(&mut stub, &budget, DORFLER_THETA).unwrap();

    assert_eq!(
        status,
        ConvergenceStatus::NotConverged {
            reason: BudgetReason::MaxIterations
        },
        "the iteration cap must outrank a stall on the same iteration — a \
         budget whose single refine happens not to clear the 10% drop still \
         reports `MaxIterations`, never `Stalled`"
    );
    assert_eq!(
        stub.refine_calls.len(),
        1,
        "one refine at iter 0, then the iter-1 gates fire before any re-marking"
    );
}

/// At iter 1 the stall gate and the dof ceiling are live SIMULTANEOUSLY; the
/// ceiling wins, for the same reason as the iteration cap above. The iteration
/// cap is far away so only the dof ceiling and the stall compete.
#[test]
fn the_dof_ceiling_outranks_a_simultaneous_stall() {
    // 0.5 → 0.48 is a 4% drop (stall gate live at iter 1), and the re-solve's
    // n_dofs 2000 >= max_dofs 1000 (dof ceiling live at iter 1).
    let mut stub = StubProblem::new(vec![est(0.5, 100), est(0.48, 2000)]);
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
        "the dof ceiling must outrank a stall on the same iteration"
    );
    assert_eq!(
        stub.refine_calls.len(),
        1,
        "one refine at iter 0, then the iter-1 gates fire before any re-marking"
    );
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

// ---------------------------------------------------------------------------
// task 7449: linear Dörfler accumulation is a deliberate choice.
// ---------------------------------------------------------------------------

/// SplitMix64: a tiny deterministic generator, so the property sweep below
/// needs no `rand` dev-dependency and has no flake surface.
struct SplitMix64(u64);

impl SplitMix64 {
    fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// Uniform-enough draw from `0..n` (modulo bias is irrelevant here).
    fn below(&mut self, n: u64) -> u64 {
        self.next_u64() % n
    }
}

fn squares_of(weights: &[f64]) -> Vec<f64> {
    weights.iter().map(|w| w * w).collect()
}

/// For one `(weights, theta)`: the linear set meets the squared bulk criterion
/// at the same θ, and contains the minimal squared set.
fn assert_linear_set_meets_squared_bulk_criterion(weights: &[f64], theta: f64) {
    let linear = mark_dorfler(weights, theta);
    let squares = squares_of(weights);
    let minimal_squared = mark_dorfler(&squares, theta);

    let marked_squares: f64 = linear.iter().map(|&i| squares[i]).sum();
    let total_squares: f64 = squares.iter().sum();
    assert!(
        marked_squares >= theta * total_squares,
        "linear Dörfler set misses the squared bulk criterion: \
         Σ_M w² = {marked_squares} < θ·Σ w² = {} \
         (w = {weights:?}, θ = {theta}, linear = {linear:?}, \
         minimal squared = {minimal_squared:?})",
        theta * total_squares,
    );
    assert!(
        minimal_squared.iter().all(|i| linear.contains(i)),
        "linear Dörfler set does not contain the minimal squared set \
         (w = {weights:?}, θ = {theta}, linear = {linear:?}, \
         minimal squared = {minimal_squared:?})",
    );
}

/// `mark_dorfler` accumulates weights linearly (Σ_M w ≥ θ Σ w). For any
/// non-negative weights that set also meets the textbook squared bulk
/// criterion Σ_M w² ≥ θ Σ w² at the SAME θ, and contains the minimal set that
/// does. The PRD's convergence rationale needs only that bulk criterion, so
/// linear marking keeps it while over-marking relative to the minimal set. See
/// `docs/prds/v0_4/a-posteriori-error-estimation.md`, Resolved decisions →
/// Refinement marking amendment (task 7449).
///
/// Integer weights in `0..=1000`, at most 40 of them, and dyadic θ keep every
/// sum and threshold exact in f64, so the inequality needs no tolerance.
#[test]
fn the_linear_dorfler_set_meets_the_squared_bulk_criterion_and_contains_the_minimal_squared_set() {
    const THETAS: [f64; 6] = [0.125, 0.25, DORFLER_THETA, 0.75, 0.875, 1.0];

    // Non-vacuous: on a skewed vector the two forms really differ.
    let skewed = vec![3.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0];
    assert_eq!(mark_dorfler(&skewed, DORFLER_THETA), vec![0, 1, 2]);
    assert_eq!(mark_dorfler(&squares_of(&skewed), DORFLER_THETA), vec![0]);

    let mut cases: Vec<Vec<f64>> = vec![
        skewed,
        vec![5.0; 10],
        vec![0.0, 7.0, 0.0, 3.0, 0.0, 0.0, 1.0],
        vec![0.0, 0.0, 0.0],
    ];
    let mut rng = SplitMix64(0x7449);
    for _ in 0..500 {
        let len = 1 + rng.below(40) as usize;
        // Half the vectors heavy-tailed (many small values, ties and zeros).
        let heavy_tailed = rng.below(2) == 0;
        let weights = (0..len)
            .map(|_| {
                let w = rng.below(1001);
                let w = if heavy_tailed { w >> rng.below(11) } else { w };
                w as f64
            })
            .collect();
        cases.push(weights);
    }

    for weights in &cases {
        for theta in THETAS {
            assert_linear_set_meets_squared_bulk_criterion(weights, theta);
        }
    }
}

// ---------------------------------------------------------------------------
// step-13: refine_marked_elements — build-agnostic length-guard validation.
//
// The size-hint length guard runs BEFORE any gmsh remesh, so this test passes
// identically in `has_gmsh` and stub builds (no `GMSH_AVAILABLE` runtime guard
// needed). The gmsh remesh path itself is covered transitively by
// `tests/volume_refine_tests.rs`.
// ---------------------------------------------------------------------------

/// Minimal two-tet (P1) bipyramid: 5 vertices, 2 tetrahedra ⇒ element count 2.
/// Mirrors the in-module fixture in `volume_refine.rs`.
fn two_tet_bipyramid() -> VolumeMesh {
    VolumeMesh {
        vertices: vec![
            0.0_f32, 0.0, 0.0, // 0
            1.0, 0.0, 0.0, // 1
            0.0, 1.0, 0.0, // 2
            0.0, 0.0, 1.0, // 3
            0.0, 0.0, -1.0, // 4
        ],
        connectivity: VolumeConnectivity::Tet {
            indices: vec![
                0, 1, 2, 3, // tet A
                0, 1, 2, 4, // tet B
            ],
            order: ElementOrderTag::P1,
        },
        normals: None,
        boundary: None,
    }
}

/// Minimal placeholder surface. Never inspected: the length guard returns
/// before any gmsh work touches the surface.
fn dummy_surface() -> Mesh {
    Mesh {
        vertices: vec![0.0_f32; 9],
        indices: vec![0, 1, 2],
        normals: None,
    }
}

/// `current_sizes` of the wrong length must trip `SizeHintsLengthMismatch`
/// before any gmsh remesh is attempted (so the test is build-agnostic).
#[test]
fn refine_marked_elements_rejects_wrong_length_current_sizes() {
    let surface = dummy_surface();
    let vm = two_tet_bipyramid(); // 2 elements
    let marked = [0usize]; // Dörfler-marked tet A
    let current_sizes = vec![1.0_f64]; // len 1 ≠ 2 elements ⇒ mismatch
    let opts = MeshingOptions::default();

    let result = refine_marked_elements(&surface, &vm, &marked, &current_sizes, &opts);

    assert!(
        matches!(
            result,
            Err(RefineError::SizeHintsLengthMismatch { got: 1, expected: 2 })
        ),
        "expected SizeHintsLengthMismatch {{got: 1, expected: 2}} from the \
         pre-gmsh length guard, got: {result:?}",
    );
}

/// An out-of-range `marked` index (independent of `current_sizes` length) must
/// trip `MarkedIndexOutOfRange` before any gmsh remesh — guarding the unchecked
/// `current_sizes[idx]` indexing inside `dorfler_size_hints` (also build-agnostic).
#[test]
fn refine_marked_elements_rejects_out_of_range_marked_index() {
    let surface = dummy_surface();
    let vm = two_tet_bipyramid(); // 2 elements
    let marked = [2usize]; // index 2 >= 2 elements ⇒ out of range
    let current_sizes = vec![1.0_f64, 1.0]; // correct length (2)
    let opts = MeshingOptions::default();

    let result = refine_marked_elements(&surface, &vm, &marked, &current_sizes, &opts);

    assert!(
        matches!(
            result,
            Err(RefineError::MarkedIndexOutOfRange {
                index: 2,
                element_count: 2
            })
        ),
        "expected MarkedIndexOutOfRange {{index: 2, element_count: 2}} from the \
         pre-gmsh bounds guard, got: {result:?}",
    );
}

// ---------------------------------------------------------------------------
// task 3000 / step-5: crate-root surface pin for the per-probe target_accuracy
// contract and the lazy-refinement timing contract. Fails to COMPILE until
// lib.rs re-exports these symbols from `adaptive` (step-6).
// ---------------------------------------------------------------------------

#[test]
fn probe_target_accuracy_and_refine_trigger_are_reexported_at_crate_root() {
    use reify_solver_elastic::{
        FAR_FROM_BOUNDARY_TARGET_ACCURACY, NEAR_BOUNDARY_TARGET_ACCURACY, RefineTrigger,
        probe_target_accuracy, should_run_refinement,
    };

    assert_eq!(probe_target_accuracy(true), NEAR_BOUNDARY_TARGET_ACCURACY);
    assert_eq!(probe_target_accuracy(true), 0.01);
    assert_eq!(probe_target_accuracy(false), FAR_FROM_BOUNDARY_TARGET_ACCURACY);
    assert_eq!(probe_target_accuracy(false), 0.10);

    assert!(should_run_refinement(RefineTrigger::ExplicitRequest));
    assert!(!should_run_refinement(RefineTrigger::ParameterSlide));
}
