//! End-to-end fixture test for the RBD-η inverse-dynamics surface
//! (`docs/prds/v0_3/rigid-body-dynamics.md` §5 / task RBD-η, Phase 4).
//!
//! Drives `examples/dynamics/pendulum_idyn.ri` through the full
//! parse → `parse_and_compile_with_stdlib` → `Engine::build` pipeline and
//! asserts the actuator torque holding a static single pendulum is
//! `m·g·L·sin(30°) = 1·9.81·0.1·0.5 = 0.4905 N·m` within 1 µN·m.
//!
//! This is the task's sole stated user-observable signal. The numeric target
//! is the same one validated at the pure-Rust core by
//! `reify-stdlib/src/dynamics/rnea.rs::single_pendulum_static_gravity_torque`
//! (mass=1, com=[0,0,−0.1], inertia=0, revolute +y, θ=−30° ⇒ 0.4905, <1e-6);
//! here it is reproduced through the entire `.ri` surface
//! (MassProperties-valued body solid → mechanism/body/revolute → snapshot/bind
//! → `inverse_dynamics_at_snapshot`).
//!
//! Kernel-INDEPENDENT: `inverse_dynamics` derives mass from the body's
//! MassProperties solid and needs no `GeometryKernel`, so a `MockGeometryKernel`
//! suffices.
//!
//! Step-11 RED: fails because `examples/dynamics/pendulum_idyn.ri` does not yet
//! exist (the `read_to_string` panics). Step-12 authors the fixture → GREEN.

use reify_ir::{ExportFormat, Value};
use reify_test_support::{MockGeometryKernel, errors_only, parse_and_compile_with_stdlib};

/// Absolute path to the pendulum inverse-dynamics example fixture.
/// Mirrors the CARGO_MANIFEST_DIR pattern from
/// `crates/reify-eval/tests/cost_aggregation_eval.rs:24–27`.
const EXAMPLE_PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../examples/dynamics/pendulum_idyn.ri"
);

/// Read an `f64` out of a numeric value cell (`Real` / `Int` / dimensioned
/// `Scalar`). Panics on a non-numeric cell so a shape regression fails loudly.
fn num(v: &Value) -> f64 {
    match v {
        Value::Real(r) => *r,
        Value::Int(n) => *n as f64,
        Value::Scalar { si_value, .. } => *si_value,
        other => panic!("expected a numeric cell, got {other:?}"),
    }
}

/// Pull a named field out of a `StructureInstance`, asserting its `type_name`.
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

/// `inverse_dynamics_at_snapshot` on the static single pendulum yields a
/// length-1 `List<JointForce>` whose revolute `ScalarTorque` magnitude equals
/// the gravity-holding torque `m·g·L·sin(30°) = 0.4905 N·m` within 1 µN·m.
#[test]
fn pendulum_idyn_static_gravity_torque_is_0_4905() {
    let source = std::fs::read_to_string(EXAMPLE_PATH)
        .expect("examples/dynamics/pendulum_idyn.ri should exist (authored by step-12)");

    let compiled = parse_and_compile_with_stdlib(&source);
    assert!(
        errors_only(&compiled).is_empty(),
        "pendulum_idyn.ri should compile with no error-severity diagnostics, got:\n{:#?}",
        errors_only(&compiled)
    );

    // Kernel-independent: inverse_dynamics reads mass from the body's
    // MassProperties solid, so a plain mock kernel is enough.
    let checker = reify_constraints::SimpleConstraintChecker;
    let kernel = MockGeometryKernel::new();
    let mut engine = reify_eval::Engine::new(Box::new(checker), Some(Box::new(kernel)));
    let result = engine.build(&compiled, ExportFormat::Step);

    // Locate the top-level `forces` cell. The structure_def is named
    // `PendulumIdyn`; the inverse-dynamics result binds to `forces`.
    let cell = reify_core::ValueCellId::new("PendulumIdyn", "forces");
    let forces = match result.values.get(&cell) {
        Some(Value::List(f)) => f,
        other => panic!(
            "PendulumIdyn.forces must be a List<JointForce>, got {other:?}\n\
             (all diagnostics: {:#?})",
            result.diagnostics
        ),
    };
    assert_eq!(forces.len(), 1, "one revolute joint ⇒ one JointForce");

    // revolute ⇒ JointForce { value: ScalarTorque { magnitude } }.
    let value = field(&forces[0], "JointForce", "value");
    let torque = num(field(value, "ScalarTorque", "magnitude"));

    let expected = 0.4905_f64; // m·g·L·sin(30°) = 1·9.81·0.1·0.5
    assert!(
        (torque - expected).abs() < 1e-6,
        "expected {expected} N·m holding the static pendulum, got {torque}"
    );
}
