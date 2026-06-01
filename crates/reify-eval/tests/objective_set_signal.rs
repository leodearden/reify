//! γ leaf-signal tests (PRD §8, task 3997): user-observable signal that the
//! weighted ObjectiveSet is wired through the full Engine→DimensionalSolver pipeline.
//!
//! Signal (1) — WEIGHTED B1/B4:
//!   `minimize 0.7 * mass - 0.3 * stiffness` (a single `minimize` declaration
//!   with a combined expression — 1-term WeightedSum, PRD §5) drives the solver
//!   to the closed-form weighted optimum: mass→lower bound, stiffness→upper bound.
//!   Basis: a linear objective over box bounds attains its minimum at a vertex.
//!
//! Signal (2) — I2 GUARD:
//!   Single-term ObjectiveSet == old single objective. Covered by the migrated
//!   single-objective assertions in resolution.rs and solver_integration.rs
//!   (asserted resolved values left UNCHANGED in steps 3-5). No separate test here.
//!
//! RED until step-8: `fixtures/objective_set_weighted.ri` does not yet exist,
//! so `weighted_fixture_source()` (which uses `include_str!`) causes a compile error.

use reify_constraints::DimensionalSolver;
use reify_core::ValueCellId;
use reify_eval::Engine;
use reify_ir::Value;
use reify_test_support::{MockConstraintChecker, collect_errors, compile_source_with_stdlib};

fn weighted_fixture_source() -> &'static str {
    include_str!("fixtures/objective_set_weighted.ri")
}

/// [B1/B4] WEIGHTED: `minimize 0.7 * mass - 0.3 * stiffness` (1-term WeightedSum,
/// combined expr) resolves to the closed-form weighted optimum via DimensionalSolver.
///
/// Fixture: `WeightedObjective` has two `Scalar = auto` params `mass` and
/// `stiffness`, each bounded in [1 mm, 50 mm] by inequality constraints.
/// Objective: `minimize 0.7 * mass - 0.3 * stiffness` — a linear combined
/// expression, so the minimum is at the box corner:
///   mass → 1 mm (lower bound), stiffness → 50 mm (upper bound).
///
/// Expected (first-principles): mass < 5 mm, stiffness > 46 mm.
/// Tolerance: ±4 mm (Nelder-Mead convergence on a 49 mm range).
#[test]
fn weighted_objective_resolves_to_corner() {
    let compiled = compile_source_with_stdlib(weighted_fixture_source());

    let errors = collect_errors(&compiled.diagnostics);
    assert!(
        errors.is_empty(),
        "fixture should compile without errors: {:#?}",
        errors
    );

    let mass_id = ValueCellId::new("WeightedObjective", "mass");
    let stiffness_id = ValueCellId::new("WeightedObjective", "stiffness");

    let mut engine =
        Engine::new(Box::new(MockConstraintChecker::new()), None).with_solver(Box::new(DimensionalSolver));

    let result = engine.eval(&compiled);

    assert!(
        result.diagnostics.is_empty(),
        "unexpected diagnostics from eval: {:#?}",
        result.diagnostics
    );

    let mass_si = match result.values.get(&mass_id) {
        Some(Value::Scalar { si_value, .. }) => *si_value,
        other => panic!("expected Scalar for WeightedObjective.mass, got {:?}", other),
    };
    let stiffness_si = match result.values.get(&stiffness_id) {
        Some(Value::Scalar { si_value, .. }) => *si_value,
        other => panic!(
            "expected Scalar for WeightedObjective.stiffness, got {:?}",
            other
        ),
    };

    // Linear objective over box bounds: optimum is at the corner vertex.
    //   mass      → lower bound 1 mm (0.001 m): coefficient +0.7 → minimise mass
    //   stiffness → upper bound 50 mm (0.050 m): coefficient −0.3 → maximise stiffness
    // Allow ±4 mm (0.004 m) Nelder-Mead tolerance on the 49 mm range.
    assert!(
        mass_si < 0.005,
        "mass should be near lower bound 1 mm (expect < 5 mm = 0.005 m), got {:.6} m",
        mass_si
    );
    assert!(
        stiffness_si > 0.046,
        "stiffness should be near upper bound 50 mm (expect > 46 mm = 0.046 m), got {:.6} m",
        stiffness_si
    );
}
