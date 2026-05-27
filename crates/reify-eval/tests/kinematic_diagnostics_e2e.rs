//! End-to-end smoke test for the v0.2 kinematic diagnostic wrapper (task 2677).
//!
//! Drives `reify_constraints::solve_loop_closure_with_diagnostics` against
//! joint Maps materialised from a Reify source via the full
//! `parse → compile_with_stdlib → eval` pipeline.  Pins that:
//!   * the wrapper's public re-exports
//!     (`LoopClosureReport`, `solve_loop_closure_with_diagnostics`) are
//!     reachable from a downstream consumer;
//!   * the singularity post-process wired in step-10 surfaces a
//!     `KinematicSingularity` warning when the Newton solver hits a
//!     rank-deficient Jacobian on stdlib-emitted joint Maps.
//!
//! Mirrors the structure of `kinematic_loop_closure_machinery.rs`; the only
//! difference is exercising the diagnostic-emitting wrapper rather than the
//! bare Newton solver.
//!
//! See docs/prds/v0_2/kinematic-constraints.md, §"Singularity, over/under-constraint diagnostics".

use reify_constraints::{
    JointValue, LoopClosureReport, NewtonConfig, NewtonOutcome, StartStrategy,
    solve_loop_closure_with_diagnostics,
};
use reify_test_support::{collect_errors, make_simple_engine, parse_and_compile_with_stdlib};
use reify_core::{DiagnosticCode, Severity, ValueCellId};
use reify_ir::{Value, ValueMap};

/// Six free prismatic-x joints (chain_b) plus a single prismatic-x
/// reference (chain_a) — all on the same +X axis with range 0..1000mm.
/// All six free vars contribute to the same +X linear translation, so
/// the closure-residual Jacobian is rank-1 → `NewtonOutcome::Singular`.
const SOURCE: &str = r#"
structure def Kinematic {
    let joint_a = prismatic(vec3(1, 0, 0), 0mm .. 1000mm)
    let joint_0 = prismatic(vec3(1, 0, 0), 0mm .. 1000mm)
    let joint_1 = prismatic(vec3(1, 0, 0), 0mm .. 1000mm)
    let joint_2 = prismatic(vec3(1, 0, 0), 0mm .. 1000mm)
    let joint_3 = prismatic(vec3(1, 0, 0), 0mm .. 1000mm)
    let joint_4 = prismatic(vec3(1, 0, 0), 0mm .. 1000mm)
    let joint_5 = prismatic(vec3(1, 0, 0), 0mm .. 1000mm)
}
"#;

/// Resolve a binding by name from the eval result.
fn get_value<'a>(values: &'a ValueMap, name: &str) -> &'a Value {
    let id = ValueCellId::new("Kinematic", name);
    values
        .get(&id)
        .unwrap_or_else(|| panic!("Kinematic.{name} not found in eval result"))
}

#[test]
fn kinematic_diagnostics_e2e_reports_singularity_for_rank_one_six_dof_chain() {
    // Compile + eval the joint Maps.
    let compiled = parse_and_compile_with_stdlib(SOURCE);
    let mut engine = make_simple_engine();
    let result = engine.eval(&compiled);
    let eval_errors = collect_errors(&result.diagnostics);
    assert!(
        eval_errors.is_empty(),
        "eval should produce no Error-severity diagnostics, got: {eval_errors:?}"
    );

    let v = &result.values;
    let joint_a = get_value(v, "joint_a").clone();
    let chain_b: Vec<Value> = (0..6)
        .map(|i| get_value(v, &format!("joint_{i}")).clone())
        .collect();

    // Single-loop problem with balanced 6 free DOFs == 6 residual count
    // (no DOF imbalance) but rank-1 Jacobian → solver returns Singular.
    let chain_a = vec![joint_a];
    let vals_a = vec![JointValue::Scalar(0.5)];
    let vals_b_initial = vec![JointValue::Scalar(0.5); 6];
    let free_b: Vec<usize> = (0..6).collect();
    let strategy = StartStrategy::WarmStart(vec![0.0; 6]);
    let cfg = NewtonConfig::default();

    let report: LoopClosureReport = solve_loop_closure_with_diagnostics(
        &chain_a,
        &vals_a,
        &chain_b,
        &vals_b_initial,
        &free_b,
        &strategy,
        &cfg,
    );

    assert!(
        report.is_singular(),
        "expected is_singular=true on rank-deficient Jacobian, got is_singular={} (outcome={:?})",
        report.is_singular(),
        report.outcome,
    );
    assert!(
        matches!(report.outcome, NewtonOutcome::Singular { .. }),
        "expected NewtonOutcome::Singular, got {:?}",
        report.outcome
    );

    // Exactly one diagnostic — KinematicSingularity Warning.  No
    // over/under-constrained bleed-through (free_b.len() == 6).
    assert_eq!(
        report.diagnostics.len(),
        1,
        "expected one singularity diagnostic, got {:?}",
        report.diagnostics
    );
    let d = &report.diagnostics[0];
    assert_eq!(d.severity, Severity::Warning);
    assert_eq!(d.code, Some(DiagnosticCode::KinematicSingularity));
}
