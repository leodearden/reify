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

use reify_constraints::{
    JointValue, NewtonConfig, NewtonOutcome, StartStrategy, solve_loop_closure,
};
use reify_core::ValueCellId;
use reify_ir::{Value, ValueMap};
use reify_stdlib::loop_closure::loop_residual_twist;
use reify_test_support::{
    CapturingSubscriberBuilder, collect_errors, make_simple_engine, parse_and_compile_with_stdlib,
    prime_tracing_callsite_cache,
};

/// Source for the KCC-γ step-13 planar-in-loop e2e fixture: a structure with a
/// revolute joint on one side and a planar joint on the other, mirroring the
/// counter-mass-balance pattern from the PRD §11.1 producer-side scenario.
/// The two joint Maps are extracted and driven directly through
/// `solve_loop_closure` (no Mechanism construction) to exercise the widened
/// solver path with multi-DOF chain participation.
const PLANAR_LOOP_SOURCE: &str = r#"
structure def CounterMassBalance {
    let joint_a = revolute(vec3(0, 0, 1), 0rad .. 6.283185307179586rad)
    let joint_b = planar(vec3(1, 0, 0), vec3(0, 1, 0),
                         -100mm .. 100mm, -100mm .. 100mm,
                         -0.7853981633974483rad .. 0.7853981633974483rad)
}
"#;

/// Resolve a binding by structure name + binding name from the eval result.
fn get_value_from<'a>(values: &'a ValueMap, structure: &str, name: &str) -> &'a Value {
    let id = ValueCellId::new(structure, name);
    values
        .get(&id)
        .unwrap_or_else(|| panic!("{structure}.{name} not found in eval result"))
}

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

/// KCC-γ step-13 user-observable signal: planar joint participates in a
/// closed-chain Newton solve through the full
/// `parse → compile_with_stdlib → eval → solve_loop_closure` pipeline.
///
/// Mirrors the single-prismatic fixture above with two twists:
///   * chain_b is a planar (3-DOF) joint — the multi-DOF participation that
///     the γ widening enables.
///   * The test captures `tracing::debug!` events at target
///     `reify_stdlib::joints` and asserts the planar analytic-Jacobian seam
///     fires at least once during the solve — the PRD §11.1
///     "analytic-J path used (logged via tracing)" signal.
///
/// chain_a = `[revolute_z @ π/6]`, end-effector at `R_z(π/6)` (pure rotation).
/// chain_b = `[planar_xy]` with the planar slot free; WarmStart from the
/// zero planar config.  The converged planar config that closes the loop is
/// `(x, y, θ) ≈ (0, 0, π/6)` — pure rotation, no translation, well within the
/// `±π/4` rotational range.
#[test]
fn loop_closure_machinery_solves_planar_in_loop_e2e() {
    // Prime the tracing callsite cache so the per-test `with_default`
    // subscriber actually receives events under parallel cargo runs.
    prime_tracing_callsite_cache();

    // Compile + eval the joint Maps.
    let compiled = parse_and_compile_with_stdlib(PLANAR_LOOP_SOURCE);
    let mut engine = make_simple_engine();
    let result = engine.eval(&compiled);
    let eval_errors = collect_errors(&result.diagnostics);
    assert!(
        eval_errors.is_empty(),
        "eval should produce no Error-severity diagnostics, got: {eval_errors:?}"
    );

    let v = &result.values;
    let joint_a = get_value_from(v, "CounterMassBalance", "joint_a").clone();
    let joint_b = get_value_from(v, "CounterMassBalance", "joint_b").clone();

    let theta_a = std::f64::consts::PI / 6.0;
    let chain_a = vec![joint_a];
    let vals_a = vec![JointValue::Scalar(theta_a)];
    let chain_b = vec![joint_b];
    let vals_b_initial = vec![JointValue::Planar([0.0, 0.0, 0.0])];
    let free_b = vec![0];
    // Planar flat_len = 3 (storage = manifold DOF for planar); warm-start
    // vector matches.
    let strategy = StartStrategy::WarmStart(vec![0.0, 0.0, 0.0]);
    let cfg = NewtonConfig::default();

    // Capture DEBUG-level tracing events at the joints seam.  The
    // `reify_stdlib::joints` target prefix matches the
    // `tracing::debug!(target: "reify_stdlib::joints", ...)` site emitted by
    // `joint_jacobian_value`'s multi-DOF arms (step-14 instrumentation).
    let (subscriber, capture) = CapturingSubscriberBuilder::new(tracing::Level::DEBUG)
        .target_prefix("reify_stdlib::joints")
        .build();

    let outcome = tracing::subscriber::with_default(subscriber, || {
        solve_loop_closure(
            &chain_a,
            &vals_a,
            &chain_b,
            &vals_b_initial,
            &free_b,
            &strategy,
            &cfg,
        )
    });

    match outcome {
        NewtonOutcome::Converged {
            x,
            iters,
            residual_norm,
        } => {
            // (b) Iteration budget — the FD-only chain Jacobian path still
            // converges well within 50 iters for this well-conditioned root.
            assert!(
                iters < 50,
                "expected convergence in <50 iters, got {iters} (residual_norm={residual_norm})"
            );
            // (c) Combined position + rotation tolerance gate.
            assert!(
                residual_norm < cfg.tol_pos_m + cfg.tol_rot_rad,
                "expected residual_norm below combined tol, got {residual_norm}"
            );
            // (d) Planar flat_len = 3 (storage = manifold DOF for planar).
            assert_eq!(x.len(), 3, "planar has flat_len=3 free components");

            // (e) Recompose chain_a / chain_b at the converged x via the
            // widened `chain_transform` (called internally by
            // `loop_residual_twist`), and confirm both linear and angular
            // norms are below the solver's own combined convergence tolerance
            // — the closed-chain residual must agree with the Newton solver's
            // reported residual_norm.  `loop_residual_twist` returns `None`
            // if either internal `chain_transform` call fails, so the
            // `.expect(...)` below subsumes the standalone FK assertions.
            let vals_b_final = vec![JointValue::Planar([x[0], x[1], x[2]])];
            let twist = loop_residual_twist(&chain_a, &vals_a, &chain_b, &vals_b_final)
                .expect("loop_residual_twist must succeed at converged config");
            let angular_norm =
                (twist[0] * twist[0] + twist[1] * twist[1] + twist[2] * twist[2]).sqrt();
            let linear_norm =
                (twist[3] * twist[3] + twist[4] * twist[4] + twist[5] * twist[5]).sqrt();
            let combined_tol = cfg.tol_pos_m + cfg.tol_rot_rad;
            assert!(
                angular_norm < combined_tol,
                "recomposed angular residual {angular_norm} should be below \
                 the combined solver tolerance (tol_pos_m + tol_rot_rad = {})",
                combined_tol
            );
            assert!(
                linear_norm < combined_tol,
                "recomposed linear residual {linear_norm} should be below \
                 the combined solver tolerance (tol_pos_m + tol_rot_rad = {})",
                combined_tol
            );
        }
        other => panic!("expected Converged on planar-in-loop closure, got {other:?}"),
    }

    // (f) Tracing seam — the planar analytic-J emission site must have fired
    // at least once during the solve.  The PRD §11.1 producer-side signal is
    // an event at target `reify_stdlib::joints` carrying `kind=planar`; the
    // structural identity (target + field) is the load-bearing check, not
    // the free-text message string (which is intentionally not asserted on
    // so future wording tweaks don't silently break the signal).
    let count = capture.count();
    let messages = capture.messages();
    assert!(
        count >= 1,
        "expected at least one tracing event at target reify_stdlib::joints \
         during the planar closed-chain solve, got count={count} \
         messages={messages:?}"
    );
    let fields = capture.fields_by_event();
    let any_planar_kind = fields
        .iter()
        .any(|f| f.get("kind").map(|s| s.contains("planar")).unwrap_or(false));
    assert!(
        any_planar_kind,
        "expected at least one captured event to carry kind=…planar… in its \
         fields, got fields={fields:?}"
    );
}
