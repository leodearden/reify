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
/// Fixture: `WeightedObjective` has two `Scalar = auto` params. Constraint design
/// intentionally avoids placing the optimizer against a boundary it must hit:
///   mass    — upper bound only (mass < 50 mm): minimized → drives toward ~1 µm.
///   stiffness — lower bound only (stiffness > 1 mm): maximized → drives toward ~10 m.
///
/// Closed-form optimum (linear objective, unconstrained in the optimizing direction):
///   mass      → DimensionalSolver default lower bound (~1 µm)
///   stiffness → DimensionalSolver default upper bound (~10 m)
///
/// Expected (first-principles): mass < 5 mm, stiffness > 46 mm.
#[test]
fn weighted_objective_resolves_to_corner() {
    let compiled = compile_source_with_stdlib(weighted_fixture_source());

    let errors = collect_errors(&compiled.diagnostics);
    assert!(
        errors.is_empty(),
        "fixture should compile without errors: {:#?}",
        errors
    );

    // Debug: verify the template has the expected structure
    let wo_template = compiled
        .templates
        .iter()
        .find(|t| t.name == "WeightedObjective")
        .expect("WeightedObjective template must be in compiled module");
    assert!(
        wo_template.objective.is_some(),
        "WeightedObjective template must have an objective after compiling 'minimize' decl; \
         templates found: {:?}",
        compiled.templates.iter().map(|t| &t.name).collect::<Vec<_>>()
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

    // Linear objective, unconstrained in optimizing direction → optimum at default bound.
    //   mass      → minimised, no lower constraint → drives toward ~1 µm (< 5 mm)
    //   stiffness → maximised, no upper constraint → drives toward ~10 m (> 46 mm)
    assert!(
        mass_si < 0.005,
        "mass should be near default lower bound ~1 µm (expect < 5 mm = 0.005 m), got {:.6} m",
        mass_si
    );
    assert!(
        stiffness_si > 0.046,
        "stiffness should be near default upper bound ~10 m (expect > 46 mm = 0.046 m), got {:.6} m",
        stiffness_si
    );
}
