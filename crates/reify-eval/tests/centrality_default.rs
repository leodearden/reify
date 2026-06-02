//! η LEAF integration test (PRD §10.7/§18 row 1, task 4013):
//! user-observable signal that the Chebyshev-centre default objective fires
//! on the full Engine→DimensionalSolver pipeline.
//!
//! Signal (B6):
//!   A scope with `Scalar = auto` param, two-sided inequality constraints,
//!   and NO `minimize`/`maximize` declaration resolves to the analytic midpoint
//!   (5mm for [2mm, 8mm]) — not a boundary point.
//!
//! Signal (I5 provenance hook):
//!   `engine.centrality_synthesized_scopes()` reports the scope name when centrality
//!   was synthesised, and omits it for scopes with an explicit user objective.
//!
//! RED until step-6: `Engine::centrality_synthesized_scopes()` does not exist.

use reify_constraints::DimensionalSolver;
use reify_core::ValueCellId;
use reify_eval::Engine;
use reify_ir::Value;
use reify_test_support::{MockConstraintChecker, collect_errors, compile_source_with_stdlib};

/// Combined source: CentredBar (no objective → centrality fires) +
/// ExplicitMinimize (explicit `maximize` → centrality does NOT fire).
fn combined_source() -> String {
    let centred = include_str!("fixtures/centrality_two_sided_bound.ri");
    // Control structure: has an explicit objective declaration → no centrality synthesis.
    //
    // Design follows the objective_set_weighted.ri pattern (task 3997): the constraint is
    // placed on the side OPPOSITE to the optimizer's direction so the optimizer drives
    // AWAY from the constraint boundary:
    //   constraint y >= 1mm  (lower bound)
    //   maximize y           → drives y toward the default upper bound (~10 m)
    //
    // This keeps initially_feasible=true (initial y ≈ 10 mm satisfies y >= 1 mm) and
    // avoids boundary-overshoot at the constraint, so the solver returns Solved cleanly.
    let explicit = r#"
structure ExplicitMinimize {
    param y: Scalar = auto

    constraint y >= 1mm

    maximize y
}
"#;
    format!("{centred}\n{explicit}")
}

/// [B6] x ∈ [2mm, 8mm], no objective → solver places x ≈ 5mm (Chebyshev centre).
/// [I5] engine.centrality_synthesized_scopes() contains "CentredBar" but not
///      "ExplicitMinimize" (which has an explicit user objective — `maximize y`).
#[test]
fn centrality_resolves_to_midpoint_and_records_scope() {
    let source = combined_source();
    let compiled = compile_source_with_stdlib(&source);

    let errors = collect_errors(&compiled.diagnostics);
    assert!(
        errors.is_empty(),
        "fixture should compile without errors: {:#?}",
        errors
    );

    // Verify the CentredBar template has no objective (fixture sanity check).
    let centred_template = compiled
        .templates
        .iter()
        .find(|t| t.name == "CentredBar")
        .expect("CentredBar template must be in compiled module");
    assert!(
        centred_template.objective.is_none(),
        "CentredBar must have no objective — it is the centrality-synthesis fixture"
    );

    // Verify ExplicitMinimize has an objective (control sanity check).
    let explicit_template = compiled
        .templates
        .iter()
        .find(|t| t.name == "ExplicitMinimize")
        .expect("ExplicitMinimize template must be in compiled module");
    assert!(
        explicit_template.objective.is_some(),
        "ExplicitMinimize must have an objective (the `maximize y` declaration)"
    );

    let x_id = ValueCellId::new("CentredBar", "x");

    let mut engine = Engine::new(Box::new(MockConstraintChecker::new()), None)
        .with_solver(Box::new(DimensionalSolver));

    let result = engine.eval(&compiled);

    assert!(
        result.diagnostics.is_empty(),
        "unexpected diagnostics from eval: {:#?}",
        result.diagnostics
    );

    // [B6] x must be ≈ 5mm (Chebyshev centre of [2mm, 8mm]), within 1e-4 m.
    let x_si = match result.values.get(&x_id) {
        Some(Value::Scalar { si_value, .. }) => *si_value,
        other => panic!("expected Scalar for CentredBar.x, got {:?}", other),
    };
    assert!(
        (x_si - 0.005).abs() < 1e-4,
        "centrality should place x ≈ 5mm (0.005 m); got {:.6} m (delta {:.2e} m)",
        x_si,
        (x_si - 0.005).abs()
    );
    assert!(
        x_si > 0.002 && x_si < 0.008,
        "x must be strictly interior to [2mm, 8mm]; got {:.6} m",
        x_si
    );

    // [I5] Scope that received synthetic centrality must be recorded.
    let synth = engine.centrality_synthesized_scopes();
    assert!(
        synth.contains("CentredBar"),
        "engine must record CentredBar in centrality_synthesized_scopes; got {:?}",
        synth
    );

    // [I5 control] Scope with explicit minimize must NOT be in the set.
    assert!(
        !synth.contains("ExplicitMinimize"),
        "ExplicitMinimize has a user objective and must not appear in \
         centrality_synthesized_scopes; got {:?}",
        synth
    );
}
