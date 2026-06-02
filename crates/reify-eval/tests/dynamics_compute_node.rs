//! Engine round-trip integration test for the RBD-ι `inverse_dynamics`
//! ComputeNode trampoline (task 3838; PRD `docs/prds/v0_3/rigid-body-dynamics.md`
//! §6/§7.7, GR-002 `docs/prds/v0_3/compute-node-contract.md` §4).
//!
//! Drives an inline single-pendulum mechanism + a zero-displacement `ramp_profile`
//! trajectory (one motionless rest sample at θ = −30°) through
//! `compute_targets::register_compute_fns` → `Engine::eval`, asserting the two
//! user-observable signals of this task:
//!
//!   (a) the `forces` result cell holds `[[ScalarTorque ≈ 0.4905 N·m]]` — the
//!       static-gravity ground truth `τ = m·g·L·sin(30°) = 1·9.81·0.1·0.5`,
//!       reused verbatim from `rigid_body_dynamics_e2e.rs` and the pure-Rust core
//!       `dynamics/rnea.rs::single_pendulum_static_gravity_torque`; and
//!   (b) a `ComputeNode` whose `target == "dynamics::inverse_dynamics"` was
//!       inserted into the evaluation graph (the trampoline path, not body
//!       inlining), and the output VC settled to `Freshness::Final`.
//!
//! step-13 RED: until step-14 adds `@optimized("dynamics::inverse_dynamics")` on
//! `pub fn inverse_dynamics` (dynamics.ri) AND registers the trampoline in
//! `register_compute_fns`, the engine body-inlines `inverse_dynamics_lower` and
//! inserts NO ComputeNode — so assertion (b) fails. Assertion (a) already passes
//! via that inline fallback (the result is bit-identical either way). step-14
//! flips (b) to GREEN.

use reify_eval::cache::NodeId;
use reify_eval::compute_targets::register_compute_fns;
use reify_test_support::{errors_only, make_simple_engine, parse_and_compile_with_stdlib};
use reify_core::ValueCellId;
use reify_ir::{Freshness, Value};

/// Static single-pendulum ground truth: `τ = m·g·L·sin(30°) = 1·9.81·0.1·0.5`.
const STATIC_TORQUE: f64 = 0.4905;

/// A single revolute pendulum — a 1 kg point mass 100 mm below an axis-+y joint —
/// driven by a *zero-displacement* `ramp_profile` at θ = −30°. A from == to ramp
/// emits a single rest sample (q̇ = q̈ = 0), so `inverse_dynamics` returns one
/// inner per-joint force list whose `ScalarTorque` holds the static pendulum at
/// 0.4905 N·m. Reuses the exact mechanism surface of `examples/dynamics/pendulum_idyn.ri`.
const FIXTURE: &str = r#"
structure InverseDynamicsComputeNode {
    let mp = MassProperties(mass: 1kg, com: point3(0mm, 0mm, -100mm), inertia: [[0.0, 0.0, 0.0], [0.0, 0.0, 0.0], [0.0, 0.0, 0.0]], origin: 0.0)
    let axis_y = vec3(0, 1, 0)
    let joint = revolute(axis_y, -180deg .. 180deg)
    let m0 = mechanism()
    let m = body(m0, mp, joint)
    // Zero-displacement ramp at θ = −30° = −π/6 rad. ramp_profile's joint/from/to
    // params are Real (the η placeholder convention; cf. dynamics.ri) — angle
    // literals (Scalar[rad]) and the Int joint handle don't match, and the SI
    // radian value is what cell_f64 reads anyway. The trajectory's stored joint is
    // unused by inverse_dynamics, so a 0.0 joint placeholder suffices.
    let traj = ramp_profile(0.0, -0.5235987755982988, -0.5235987755982988, 1.0)
    let forces = inverse_dynamics(m, traj)
}
"#;

/// Read an `f64` out of a numeric value cell (`Real` / `Int` / dimensioned
/// `Scalar`); panics on a non-numeric cell so a shape regression fails loudly.
/// Mirrors `rigid_body_dynamics_e2e.rs::num`.
fn num(v: &Value) -> f64 {
    match v {
        Value::Real(r) => *r,
        Value::Int(n) => *n as f64,
        Value::Scalar { si_value, .. } => *si_value,
        other => panic!("expected a numeric cell, got {other:?}"),
    }
}

/// Pull a named field out of a `StructureInstance`, asserting its `type_name`.
/// Mirrors `rigid_body_dynamics_e2e.rs::field`.
fn field<'a>(v: &'a Value, type_name: &str, member: &str) -> &'a Value {
    match v {
        Value::StructureInstance(data) => {
            assert_eq!(
                data.type_name, type_name,
                "expected a {type_name} instance, got type_name {}",
                data.type_name
            );
            data.fields
                .get(member)
                .unwrap_or_else(|| panic!("{type_name} missing field `{member}`"))
        }
        other => panic!("expected a {type_name} StructureInstance, got {other:?}"),
    }
}

/// End-to-end: a registered `@optimized("dynamics::inverse_dynamics")` call lowers
/// to a ComputeNode and the observable `forces` cell holds the static-pendulum
/// torque. RED at step-13 on assertion (b) (no annotation/registration yet).
#[test]
fn inverse_dynamics_lowers_to_compute_node_and_holds_static_torque() {
    let compiled = parse_and_compile_with_stdlib(FIXTURE);
    assert!(
        errors_only(&compiled).is_empty(),
        "fixture must compile with no error-severity diagnostics, got:\n{:#?}",
        errors_only(&compiled)
    );

    let mut engine = make_simple_engine();
    register_compute_fns(&mut engine);

    let eval_result = engine.eval(&compiled);

    // ── (a) the result cell holds [[ScalarTorque ≈ 0.4905]] ──────────────────────
    let cell = ValueCellId::new("InverseDynamicsComputeNode", "forces");
    let per_sample = match eval_result.values.get(&cell) {
        Some(Value::List(s)) => s,
        other => panic!(
            "InverseDynamicsComputeNode.forces must be a List<List<JointForce>>, got {other:?}\n\
             (all diagnostics: {:#?})",
            eval_result.diagnostics
        ),
    };
    assert_eq!(
        per_sample.len(),
        1,
        "a zero-displacement ramp_profile emits exactly one rest sample"
    );
    let forces = match &per_sample[0] {
        Value::List(f) => f,
        other => panic!("sample 0 must be a List<JointForce>, got {other:?}"),
    };
    assert_eq!(forces.len(), 1, "one revolute joint ⇒ one JointForce");
    let value = field(&forces[0], "JointForce", "value");
    let torque = num(field(value, "ScalarTorque", "magnitude"));
    assert!(
        (torque - STATIC_TORQUE).abs() < 1e-6,
        "expected {STATIC_TORQUE} N·m holding the static pendulum, got {torque}"
    );

    // ── (b) a ComputeNode with target "dynamics::inverse_dynamics" was inserted ──
    let snapshot = engine
        .eval_state()
        .expect("eval_state must be Some after eval()")
        .snapshot
        .clone();
    let compute_node = snapshot
        .graph
        .compute_nodes
        .iter()
        .find(|(_, data)| data.target == "dynamics::inverse_dynamics");
    assert!(
        compute_node.is_some(),
        "expected a ComputeNode with target==\"dynamics::inverse_dynamics\" in the graph \
         (the trampoline path, not body inlining), found compute nodes: {:?}",
        snapshot
            .graph
            .compute_nodes
            .iter()
            .map(|(_, d)| &d.target)
            .collect::<Vec<_>>()
    );

    // ── (b cont.) the output VC settled to Freshness::Final ──────────────────────
    let node = NodeId::Value(cell.clone());
    assert!(
        matches!(engine.freshness(&node), Freshness::Final),
        "the inverse_dynamics output VC must be Freshness::Final after eval, got {:?}",
        engine.freshness(&node)
    );
}
