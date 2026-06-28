//! Eval-level signal tests for task 4789 (cost-min α): robustness floor
//! (slack_i ≥ m) for Money-dimensioned objectives.
//!
//! These tests exercise the full Engine → DimensionalSolver pipeline via
//! `compile_source_with_stdlib` → `engine.eval`, asserting user-observable
//! signal at the eval boundary.  Solver-level assertions live in
//! `crates/reify-constraints/tests/robustness_floor.rs`.
//!
//! # Tests
//!
//! (a) HEADLINE `money_floor_resolves_off_boundary_and_emits_info` (RED until step-6):
//!     The `CostMinFloor` fixture has `thickness > 1mm` with a Money minimize.
//!     After the floor, `thickness` parks near 1.02mm (≈ 1mm + 2% margin) — strictly
//!     off the boundary — and eval emits one `RobustnessFloorApplied` (Info) diagnostic.
//!     RED on the info-diagnostic assertion until step-6 wires detect_robustness_floor_applied.
//!
//! (b) `non_money_objective_emits_no_floor_diagnostic` (invariant ii at diagnostic level):
//!     The `WeightedObjective` fixture (non-Money Length objective) resolves to the
//!     correct corner and emits NO `RobustnessFloorApplied` diagnostic.
//!
//! (c) `floor_infeasible_surfaces_distinct_diagnostic` (invariant iii end-to-end):
//!     The `CostMinFloorInfeasible` fixture (tight box [10mm, 10.3mm]) is infeasible
//!     under the floor; eval surfaces `RobustnessFloorInfeasible` (Error) and NO bare
//!     `ConstraintUnsatisfiable`.

use reify_constraints::DimensionalSolver;
use reify_core::{DiagnosticCode, ValueCellId};
use reify_eval::Engine;
use reify_ir::Value;
use reify_test_support::{MockConstraintChecker, collect_errors, compile_source_with_stdlib};

fn floor_fixture_source() -> &'static str {
    include_str!("fixtures/cost_min_robustness_floor.ri")
}

fn floor_infeasible_fixture_source() -> &'static str {
    include_str!("fixtures/cost_min_floor_infeasible.ri")
}

fn weighted_fixture_source() -> &'static str {
    include_str!("fixtures/objective_set_weighted.ri")
}

/// (a) HEADLINE: floor holds thickness strictly off the > 1mm boundary and eval emits Info.
///
/// RED until step-6: the resolved value off-boundary is satisfied by steps 1/2, but the
/// `RobustnessFloorApplied` info diagnostic is only emitted after step-6 wires
/// `detect_robustness_floor_applied` into the eval post-pass block.
#[test]
fn money_floor_resolves_off_boundary_and_emits_info() {
    let compiled = compile_source_with_stdlib(floor_fixture_source());

    let errors = collect_errors(&compiled.diagnostics);
    assert!(
        errors.is_empty(),
        "fixture should compile without errors: {:#?}",
        errors
    );

    let thickness_id = ValueCellId::new("CostMinFloor", "thickness");

    let mut engine = Engine::new(Box::new(MockConstraintChecker::new()), None)
        .with_solver(Box::new(DimensionalSolver));

    let result = engine.eval(&compiled);

    let thickness_si = match result.values.get(&thickness_id) {
        Some(Value::Scalar { si_value, .. }) => *si_value,
        other => panic!(
            "expected Scalar for CostMinFloor.thickness, got {:?}",
            other
        ),
    };

    // Floor parks thickness near 1mm + 2% margin ≈ 1.02mm.
    // Must be strictly above 1mm (floor satisfied: > boundary) and below 1.5mm
    // (close to boundary, not wandering to the 10mm default seed).
    assert!(
        thickness_si > 0.001,
        "thickness should be strictly above the 1mm boundary = 0.001 m (floor must hold it off); got {:.6} m",
        thickness_si
    );
    assert!(
        thickness_si < 0.0015,
        "thickness should be near floor (1.02mm ≈ 0.00102 m), not the 10mm default seed; got {:.6} m",
        thickness_si
    );

    // Eval must emit exactly one RobustnessFloorApplied (Info) diagnostic.
    let floor_applied: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.code == Some(DiagnosticCode::RobustnessFloorApplied))
        .collect();
    assert_eq!(
        floor_applied.len(),
        1,
        "expected exactly one RobustnessFloorApplied diagnostic; got {}: {:#?}",
        floor_applied.len(),
        result.diagnostics,
    );
}

/// (b) Invariant (ii) at the diagnostic level: a non-Money Length objective
/// emits no RobustnessFloorApplied diagnostic and resolves to the correct corner.
#[test]
fn non_money_objective_emits_no_floor_diagnostic() {
    let compiled = compile_source_with_stdlib(weighted_fixture_source());

    let errors = collect_errors(&compiled.diagnostics);
    assert!(
        errors.is_empty(),
        "fixture should compile without errors: {:#?}",
        errors
    );

    let mass_id = ValueCellId::new("WeightedObjective", "mass");
    let stiffness_id = ValueCellId::new("WeightedObjective", "stiffness");

    let mut engine = Engine::new(Box::new(MockConstraintChecker::new()), None)
        .with_solver(Box::new(DimensionalSolver));

    let result = engine.eval(&compiled);

    // No floor diagnostic for non-Money objectives (invariant ii).
    let floor_diags: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.code == Some(DiagnosticCode::RobustnessFloorApplied))
        .collect();
    assert!(
        floor_diags.is_empty(),
        "non-Money objective must emit no RobustnessFloorApplied; got: {:#?}",
        floor_diags,
    );

    // Resolved values must still be at the linear-objective corner (same as before floor).
    let mass_si = match result.values.get(&mass_id) {
        Some(Value::Scalar { si_value, .. }) => *si_value,
        other => panic!(
            "expected Scalar for WeightedObjective.mass, got {:?}",
            other
        ),
    };
    let stiffness_si = match result.values.get(&stiffness_id) {
        Some(Value::Scalar { si_value, .. }) => *si_value,
        other => panic!(
            "expected Scalar for WeightedObjective.stiffness, got {:?}",
            other
        ),
    };

    assert!(
        mass_si < 0.005,
        "mass should be near default lower bound ~1 µm (< 5 mm = 0.005 m), got {:.6} m",
        mass_si
    );
    assert!(
        stiffness_si > 0.046,
        "stiffness should be near default upper bound ~10 m (> 46 mm = 0.046 m), got {:.6} m",
        stiffness_si
    );
}

/// (c) Invariant (iii) end-to-end: floor-infeasible problem surfaces
/// RobustnessFloorInfeasible (Error) and no bare ConstraintUnsatisfiable.
///
/// The tight box [10mm, 10.3mm] is non-empty un-floored but the 2% margin
/// pushes the required region to [10.2mm, ~10.094mm] which is empty.
#[test]
fn floor_infeasible_surfaces_distinct_diagnostic() {
    let compiled = compile_source_with_stdlib(floor_infeasible_fixture_source());

    let errors = collect_errors(&compiled.diagnostics);
    assert!(
        errors.is_empty(),
        "fixture should compile without errors: {:#?}",
        errors
    );

    let mut engine = Engine::new(Box::new(MockConstraintChecker::new()), None)
        .with_solver(Box::new(DimensionalSolver));

    let result = engine.eval(&compiled);

    // Must contain RobustnessFloorInfeasible (Error).
    let floor_inf: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.code == Some(DiagnosticCode::RobustnessFloorInfeasible))
        .collect();
    assert!(
        !floor_inf.is_empty(),
        "expected at least one RobustnessFloorInfeasible diagnostic; got none. All diagnostics: {:#?}",
        result.diagnostics,
    );

    // Must NOT contain a bare ConstraintUnsatisfiable (invariant iii: distinct code).
    let bare_unsat: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.code == Some(DiagnosticCode::ConstraintUnsatisfiable))
        .collect();
    assert!(
        bare_unsat.is_empty(),
        "floor-infeasible must NOT emit bare ConstraintUnsatisfiable; got: {:#?}",
        bare_unsat,
    );
}
