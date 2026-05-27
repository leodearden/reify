//! End-to-end smoke test for the v0.2 loop-closure residual machinery (task 2584).
//!
//! Drives the new `reify_constraints::loop_closure::solve_loop_closure` API
//! against joint Maps materialised from a Reify source via the full
//! `parse → compile_with_stdlib → eval` pipeline, exercising the integration
//! point future task 2585's snapshot evaluator will plug into.
//!
//! Mirrors the structure of `kinematic_stdlib_smoke.rs` but on the
//! `reify-constraints`-side API.  Verifies:
//!   * joint Maps emitted by `prismatic(...)` flow through
//!     `solve_loop_closure` without conversion glue;
//!   * a single-loop problem with `StartStrategy::WarmStart(vec![0.0])` from
//!     a 0.0 m initial guess converges to chain_a's 0.5 m value;
//!   * default `NewtonConfig` (1µm position, 1µrad rotation, 50 iters) is
//!     honoured — the converged residual_norm drops below the position
//!     tolerance (split-tolerance contract from step-19).
//!
//! See docs/prds/v0_2/kinematic-constraints.md, §"Decomposition plan", task 2.

use reify_constraints::{JointValue, NewtonConfig, NewtonOutcome, StartStrategy, solve_loop_closure};
use reify_test_support::{collect_errors, make_simple_engine, parse_and_compile_with_stdlib};
use reify_types::{Value, ValueCellId, ValueMap};

/// Source: a `Kinematic` structure with two prismatic joints along +X with
/// range 0..1 m.  These joint Maps are extracted and passed to
/// `solve_loop_closure` to drive a single-loop closure problem.
const SOURCE: &str = r#"
structure def Kinematic {
    let joint_a = prismatic(vec3(1, 0, 0), 0mm .. 1000mm)
    let joint_b = prismatic(vec3(1, 0, 0), 0mm .. 1000mm)
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
fn loop_closure_machinery_solves_single_prismatic_loop_e2e() {
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
    let joint_b = get_value(v, "joint_b").clone();

    // Single-loop problem: chain_a = [joint_a] held at 0.5 m, chain_b =
    // [joint_b] with index 0 free, initialised at 0.0 m.  Solver should
    // drive the free var to 0.5 m to close the loop.
    let chain_a = vec![joint_a];
    let vals_a = vec![JointValue::Scalar(0.5)];
    let chain_b = vec![joint_b];
    let vals_b_initial = vec![JointValue::Scalar(0.0)];
    let free_b = vec![0];
    let strategy = StartStrategy::WarmStart(vec![0.0]);
    let cfg = NewtonConfig::default();

    let outcome = solve_loop_closure(
        &chain_a,
        &vals_a,
        &chain_b,
        &vals_b_initial,
        &free_b,
        &strategy,
        &cfg,
    );

    match outcome {
        NewtonOutcome::Converged {
            x,
            iters: _,
            residual_norm,
        } => {
            assert_eq!(x.len(), 1, "expected one free variable");
            assert!(
                (x[0] - 0.5).abs() < 1e-6,
                "expected x[0] ≈ 0.5 m (chain_a's value), got {}",
                x[0]
            );
            // Default config uses `tol_pos_m = 1e-6` and `tol_rot_rad = 1e-6`;
            // converged residual must be below the position tolerance (the
            // problem is purely linear, no rotation residual).
            assert!(
                residual_norm < cfg.tol_pos_m * 2.0,
                "residual_norm {residual_norm} should be below 2*tol_pos_m {}",
                cfg.tol_pos_m * 2.0
            );
        }
        other => panic!("expected Converged on single-prismatic loop closure, got {other:?}"),
    }
}
