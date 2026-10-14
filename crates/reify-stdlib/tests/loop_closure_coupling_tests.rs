//! A coupling on a loop's closing side tracks its parent rather than being
//! iterated as a free variable (#7497). Exercised only through public seams.

use std::f64::consts::PI;

use reify_ir::Value;
use reify_stdlib::eval_builtin;
use reify_stdlib::loop_closure::{
    chain_jacobian_fd, extract_loop_closure_chains, loop_residual_jacobian_by_joint,
    loop_residual_twist,
};
use reify_stdlib::loop_closure_solver::{
    NewtonConfig, NewtonOutcome, StartStrategy, solve_loop_closure,
    solve_loop_closure_with_diagnostics,
};
use reify_stdlib::loop_closure_value::JointValue;

const SOLVE_TOL: f64 = 1e-6;

fn axis(x: f64, y: f64, z: f64) -> Value {
    Value::Vector(vec![Value::Real(x), Value::Real(y), Value::Real(z)])
}

fn closed_range(lower: Value, upper: Value) -> Value {
    Value::Range {
        lower: Some(Box::new(lower)),
        upper: Some(Box::new(upper)),
        lower_inclusive: true,
        upper_inclusive: true,
    }
}

/// Joints are identified structurally, so every prismatic in a fixture gets
/// its own range.
fn prismatic_x(lo_m: f64, hi_m: f64) -> Value {
    eval_builtin(
        "prismatic",
        &[
            axis(1.0, 0.0, 0.0),
            closed_range(Value::length(lo_m), Value::length(hi_m)),
        ],
    )
}

fn revolute_z_0_to_pi() -> Value {
    eval_builtin(
        "revolute",
        &[
            axis(0.0, 0.0, 1.0),
            closed_range(Value::angle(0.0), Value::angle(PI)),
        ],
    )
}

fn couple_by_two(parent: &Value) -> Value {
    eval_builtin("couple", &[parent.clone(), Value::Real(2.0)])
}

fn body(mechanism: Value, solid_name: &str, at: &Value, parent: &Value) -> Value {
    eval_builtin(
        "body",
        &[
            mechanism,
            Value::String(solid_name.to_string()),
            at.clone(),
            parent.clone(),
        ],
    )
}

fn bind_len(joint: &Value, metres: f64) -> Value {
    eval_builtin("bind", &[joint.clone(), Value::length(metres)])
}

fn bind_ang(joint: &Value, radians: f64) -> Value {
    eval_builtin("bind", &[joint.clone(), Value::angle(radians)])
}

fn map_field<'a>(value: &'a Value, key: &str) -> Option<&'a Value> {
    match value {
        Value::Map(m) => m.get(&Value::String(key.to_string())),
        other => panic!("expected a Map, got {other:?}"),
    }
}

fn only_loop_closure(mechanism: &Value) -> &Value {
    match map_field(mechanism, "loop_closures") {
        Some(Value::List(records)) if records.len() == 1 => &records[0],
        other => panic!("expected exactly one loop_closures record, got {other:?}"),
    }
}

fn x_of(snapshot: &Value, id: i64) -> f64 {
    match eval_builtin("transform_of", &[snapshot.clone(), Value::Int(id)]) {
        Value::Transform { translation, .. } => match translation.as_ref() {
            Value::Vector(c) if c.len() == 3 => c[0]
                .as_f64()
                .unwrap_or_else(|| panic!("body {id}: translation x not numeric: {:?}", c[0])),
            other => panic!("body {id}: expected Vector(3) translation, got {other:?}"),
        },
        other => panic!("body {id}: expected Transform, got {other:?}"),
    }
}

fn assert_close(actual: f64, expected: f64, label: &str) {
    assert!(
        (actual - expected).abs() < SOLVE_TOL,
        "{label}: expected {expected}, got {actual}"
    );
}

/// The snapshot closed cleanly: no singular flag and exactly one free value.
fn only_free_value(snapshot: &Value) -> f64 {
    let free_values = map_field(snapshot, "free_values");
    assert!(
        map_field(snapshot, "is_singular").is_none(),
        "snapshot must not be singular; free_values = {free_values:?}"
    );
    let loops = match free_values {
        Some(Value::List(loops)) if loops.len() == 1 => loops,
        other => panic!("expected one loop's free_values, got {other:?}"),
    };
    match &loops[0] {
        Value::List(values) if values.len() == 1 => match values[0] {
            Value::Real(v) => v,
            ref other => panic!("free value must be a Real, got {other:?}"),
        },
        other => panic!("expected exactly one free value, got {other:?}"),
    }
}

/// Case 1, the coupling's parent is bound: `2·jP + jB = jA` along X.
///
/// Body ids: 0 sA@jA, 1 sP@jP, 2 sC@jC, 3 sB@jB (on jC), 4 sX@jX (on jA),
/// 5 sD@jX (on jB) — the closing edge, `path_a = [world, jA, jX]`,
/// `path_b = [world, jC, jB]`.
struct BoundParentFixture {
    mechanism: Value,
    bindings: Vec<Value>,
    j_c: Value,
    j_b: Value,
}

fn bound_parent_mechanism() -> BoundParentFixture {
    let world = eval_builtin("world", &[]);
    let j_a = prismatic_x(0.0, 1.0);
    let j_p = prismatic_x(0.0, 0.5);
    let j_c = couple_by_two(&j_p);
    let j_b = prismatic_x(0.0, 2.0);
    let j_x = revolute_z_0_to_pi();

    let mut m = eval_builtin("mechanism", &[]);
    m = body(m, "sA", &j_a, &world);
    m = body(m, "sP", &j_p, &world);
    m = body(m, "sC", &j_c, &world);
    m = body(m, "sB", &j_b, &j_c);
    m = body(m, "sX", &j_x, &j_a);
    m = body(m, "sD", &j_x, &j_b);

    BoundParentFixture {
        mechanism: m,
        bindings: vec![
            bind_len(&j_a, 0.5),
            bind_len(&j_p, 0.1),
            bind_ang(&j_x, 0.0),
        ],
        j_c,
        j_b,
    }
}

#[test]
fn extraction_derives_a_closing_side_coupling_from_its_bound_parent() {
    let fixture = bound_parent_mechanism();
    let (_, _, chain_b, vals_b_initial, free_b) =
        extract_loop_closure_chains(only_loop_closure(&fixture.mechanism), &fixture.bindings)
            .expect("the bound-parent loop must extract");

    assert_eq!(chain_b, vec![fixture.j_c.clone(), fixture.j_b.clone()]);
    match vals_b_initial[0] {
        JointValue::Scalar(v) => assert!(
            (v - 0.1).abs() < 1e-12,
            "the coupling slot carries its parent's bound value, got {v}"
        ),
        ref other => panic!("coupling slot must be a Scalar, got {other:?}"),
    }
    assert_eq!(
        free_b,
        vec![1],
        "only the driving prismatic jB is free; the coupling contributes no variable"
    );
}

#[test]
fn snapshot_honours_coupling_ratio_when_parent_is_bound() {
    let fixture = bound_parent_mechanism();
    let snapshot = eval_builtin(
        "snapshot",
        &[fixture.mechanism, Value::List(fixture.bindings)],
    );

    assert_close(only_free_value(&snapshot), 0.3, "solved jB");
    assert_close(x_of(&snapshot, 2), 0.2, "sC at 2·jP");
    assert_close(x_of(&snapshot, 3), 0.5, "sB at 2·jP + jB");
    assert_close(x_of(&snapshot, 4), 0.5, "sX at jA");
    assert_close(x_of(&snapshot, 5), 0.5, "closing body sD meets sX");
}

/// The coupling's parent jQ is unbound and on neither path, so nothing in the
/// solve moves it. The coupling slot therefore stays at jQ's range midpoint,
/// the same value snapshot's `value_for` renders jC with. It is not a free
/// variable that could absorb the loop's error.
///
/// Body ids: 0 sA@jA, 1 sC@jC, 2 sX@jX (on jA), 3 sD@jX (on jC) — the closing
/// edge, `path_a = [world, jA, jX]`, `path_b = [world, jC]`.
#[test]
fn extraction_holds_a_coupling_of_an_off_path_parent_at_the_parents_midpoint() {
    let world = eval_builtin("world", &[]);
    let j_a = prismatic_x(0.0, 1.0);
    let j_q = prismatic_x(0.0, 0.8);
    let j_c = couple_by_two(&j_q);
    let j_x = revolute_z_0_to_pi();

    let mut m = eval_builtin("mechanism", &[]);
    m = body(m, "sA", &j_a, &world);
    m = body(m, "sC", &j_c, &world);
    m = body(m, "sX", &j_x, &j_a);
    m = body(m, "sD", &j_x, &j_c);
    let bindings = vec![bind_len(&j_a, 0.5), bind_ang(&j_x, 0.0)];

    let (_, _, chain_b, vals_b_initial, free_b) =
        extract_loop_closure_chains(only_loop_closure(&m), &bindings)
            .expect("the off-path-parent loop must extract");

    assert_eq!(chain_b, vec![j_c]);
    match vals_b_initial[0] {
        JointValue::Scalar(v) => assert!(
            (v - 0.4).abs() < 1e-12,
            "the coupling slot carries jQ's range midpoint 0.4, got {v}"
        ),
        ref other => panic!("coupling slot must be a Scalar, got {other:?}"),
    }
    assert!(
        free_b.is_empty(),
        "a coupling gets no free variable even when its parent is off both paths, \
         got {free_b:?}"
    );
}

/// A closing side whose second slot is the non-driving joint under test.
struct NonDrivingFreeSlot {
    chain_a: Vec<Value>,
    vals_a: Vec<JointValue>,
    chain_b: Vec<Value>,
    vals_b_initial: Vec<JointValue>,
}

/// Builds the non-driving joint from the driver in the first slot.
type NonDrivingFromDriver = fn(&Value) -> Value;

fn non_driving_in_second_slot(non_driving: impl FnOnce(&Value) -> Value) -> NonDrivingFreeSlot {
    let j_p = prismatic_x(0.0, 1.0);
    let second = non_driving(&j_p);
    NonDrivingFreeSlot {
        chain_a: vec![prismatic_x(0.0, 2.0)],
        vals_a: vec![JointValue::Scalar(0.6)],
        chain_b: vec![j_p, second],
        vals_b_initial: vec![JointValue::Scalar(0.5), JointValue::Scalar(0.5)],
    }
}

fn assert_non_driving_rejection(outcome: &NewtonOutcome, label: &str) {
    match outcome {
        NewtonOutcome::InvalidInput { reason } => assert!(
            reason.contains("non-driving") && reason.contains("index 1"),
            "{label}: the reason must name the non-driving free_b index 1, got {reason:?}"
        ),
        other => panic!("{label}: expected InvalidInput, got {other:?}"),
    }
}

#[test]
fn solve_loop_closure_rejects_a_non_driving_joint_in_free_b() {
    let cases: [(&str, NonDrivingFromDriver); 2] = [
        ("coupling", couple_by_two),
        ("fixed", |_| eval_builtin("fixed", &[])),
    ];
    for (label, non_driving) in cases {
        let input = non_driving_in_second_slot(non_driving);
        let outcome = solve_loop_closure(
            &input.chain_a,
            &input.vals_a,
            &input.chain_b,
            &input.vals_b_initial,
            &[0, 1],
            &StartStrategy::Midpoint,
            &NewtonConfig::default(),
        );
        assert_non_driving_rejection(&outcome, label);
    }
}

#[test]
fn solve_loop_closure_with_diagnostics_rejects_a_non_driving_joint_in_free_b() {
    let input = non_driving_in_second_slot(couple_by_two);
    let report = solve_loop_closure_with_diagnostics(
        &input.chain_a,
        &input.vals_a,
        &input.chain_b,
        &input.vals_b_initial,
        &[0, 1],
        &StartStrategy::Midpoint,
        &NewtonConfig::default(),
    );
    assert_non_driving_rejection(&report.outcome, "coupling via diagnostics");
}

/// The single FD column of `chain` w.r.t. slot 0, in `[ω; v]` twist order.
fn only_jacobian_column(chain: &[Value]) -> [f64; 6] {
    let values = [JointValue::Scalar(0.2), JointValue::Scalar(0.2)];
    let columns =
        chain_jacobian_fd(chain, &values, &[0], 1e-6).expect("the chain Jacobian must evaluate");
    assert_eq!(columns.len(), 1, "one Scalar free joint gives one column");
    columns[0]
}

fn assert_column_close(actual: [f64; 6], expected: [f64; 6], label: &str) {
    assert!(
        (0..6).all(|k| (actual[k] - expected[k]).abs() < SOLVE_TOL),
        "{label}: expected {expected:?}, got {actual:?}"
    );
}

#[test]
fn chain_jacobian_fd_moves_a_coupling_with_its_free_parent() {
    let j_p = prismatic_x(0.0, 1.0);
    let j_c = couple_by_two(&j_p);
    assert_column_close(
        only_jacobian_column(&[j_p, j_c]),
        [0.0, 0.0, 0.0, 3.0, 0.0, 0.0],
        "jP's own travel plus twice it through the coupled slot",
    );
}

#[test]
fn chain_jacobian_fd_leaves_a_coupling_of_an_absent_parent_alone() {
    let j_b = prismatic_x(0.0, 1.0);
    let j_q = prismatic_x(0.0, 2.0);
    let j_c = couple_by_two(&j_q);
    assert_column_close(
        only_jacobian_column(&[j_b, j_c]),
        [0.0, 0.0, 0.0, 1.0, 0.0, 0.0],
        "a coupling of a joint outside the chain does not follow jB",
    );
}

/// The single `loop_residual_jacobian_by_joint` column for `target`, with
/// every slot of both chains at 0.2.
fn only_residual_column(chain_a: &[Value], chain_b: &[Value], target: &Value) -> [f64; 6] {
    let vals_a = vec![JointValue::Scalar(0.2); chain_a.len()];
    let vals_b = vec![JointValue::Scalar(0.2); chain_b.len()];
    let columns = loop_residual_jacobian_by_joint(
        chain_a,
        &vals_a,
        chain_b,
        &vals_b,
        std::slice::from_ref(target),
        1e-6,
    )
    .expect("the loop Jacobian must evaluate");
    assert_eq!(columns.len(), 1, "one Scalar target gives one column");
    columns[0]
}

#[test]
fn loop_residual_jacobian_by_joint_moves_a_coupling_with_its_target() {
    let j_p = prismatic_x(0.0, 1.0);
    let j_c = couple_by_two(&j_p);
    assert_column_close(
        only_residual_column(&[prismatic_x(0.0, 2.0)], &[j_p.clone(), j_c], &j_p),
        [0.0, 0.0, 0.0, 3.0, 0.0, 0.0],
        "jP's own travel plus twice it through the coupled slot",
    );
}

#[test]
fn loop_residual_jacobian_by_joint_moves_a_coupling_on_the_other_chain() {
    let j_p = prismatic_x(0.0, 1.0);
    let j_c = couple_by_two(&j_p);
    assert_column_close(
        only_residual_column(&[j_c], &[j_p.clone()], &j_p),
        [0.0, 0.0, 0.0, -1.0, 0.0, 0.0],
        "jP on the closing side minus twice it through the reference-side coupling",
    );
}

#[test]
fn loop_residual_jacobian_by_joint_reaches_a_target_carried_only_by_its_coupling() {
    let j_q = prismatic_x(0.0, 1.0);
    assert_column_close(
        only_residual_column(&[prismatic_x(0.0, 2.0)], &[couple_by_two(&j_q)], &j_q),
        [0.0, 0.0, 0.0, 2.0, 0.0, 0.0],
        "jQ is on neither chain but moves the residual through its coupling",
    );
}

/// Case 2, the coupling's parent is itself free on the closing side:
/// `jP + 2·jP = jA` along X.
///
/// Body ids: 0 sA@jA, 1 sP@jP, 2 sC@jC (on its own parent jP), 3 sX@jX (on
/// jA), 4 sD@jX (on jC) — the closing edge, `path_a = [world, jA, jX]`,
/// `path_b = [world, jP, jC]`. jP's range midpoint (0.25) is off the closure,
/// so Newton has to move.
struct FreeParentFixture {
    mechanism: Value,
    bindings: Vec<Value>,
}

fn free_parent_mechanism() -> FreeParentFixture {
    let world = eval_builtin("world", &[]);
    let j_a = prismatic_x(0.0, 1.0);
    let j_p = prismatic_x(0.0, 0.5);
    let j_c = couple_by_two(&j_p);
    let j_x = revolute_z_0_to_pi();

    let mut m = eval_builtin("mechanism", &[]);
    m = body(m, "sA", &j_a, &world);
    m = body(m, "sP", &j_p, &world);
    m = body(m, "sC", &j_c, &j_p);
    m = body(m, "sX", &j_x, &j_a);
    m = body(m, "sD", &j_x, &j_c);

    FreeParentFixture {
        mechanism: m,
        bindings: vec![bind_len(&j_a, 0.6), bind_ang(&j_x, 0.0)],
    }
}

#[test]
fn solve_loop_closure_keeps_a_coupling_in_step_with_its_free_parent() {
    let fixture = free_parent_mechanism();
    let (chain_a, vals_a, chain_b, vals_b_initial, free_b) =
        extract_loop_closure_chains(only_loop_closure(&fixture.mechanism), &fixture.bindings)
            .expect("the free-parent loop must extract");
    assert_eq!(free_b, vec![0], "jP is free; its coupling jC is not");

    let outcome = solve_loop_closure(
        &chain_a,
        &vals_a,
        &chain_b,
        &vals_b_initial,
        &free_b,
        &StartStrategy::Midpoint,
        &NewtonConfig::default(),
    );
    let x = match outcome {
        NewtonOutcome::Converged { x, .. } => x,
        other => panic!("expected Converged, got {other:?}"),
    };
    assert_eq!(x.len(), 1, "one free variable");
    assert_close(x[0], 0.2, "solved jP");

    let realised = [JointValue::Scalar(x[0]), JointValue::Scalar(x[0])];
    let residual = loop_residual_twist(&chain_a, &vals_a, &chain_b, &realised)
        .expect("the residual must evaluate at the solved configuration");
    assert!(
        residual.iter().all(|r| r.abs() < SOLVE_TOL),
        "the loop must close with jC following jP, residual {residual:?}"
    );
}

#[test]
fn snapshot_honours_coupling_ratio_when_parent_is_free() {
    let fixture = free_parent_mechanism();
    let snapshot = eval_builtin(
        "snapshot",
        &[fixture.mechanism, Value::List(fixture.bindings)],
    );

    assert_close(only_free_value(&snapshot), 0.2, "solved jP");
    assert_close(x_of(&snapshot, 1), 0.2, "sP at jP");
    assert_close(x_of(&snapshot, 2), 0.6, "sC at jP + 2·jP");
    assert_close(x_of(&snapshot, 3), 0.6, "sX at jA");
    assert_close(x_of(&snapshot, 4), 0.6, "closing body sD meets sX");
}
