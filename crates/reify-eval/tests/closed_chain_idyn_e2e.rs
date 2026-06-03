//! End-to-end fixture test for the CLOSED-chain inverse-dynamics bridge
//! (task 4146; descoped from `docs/prds/v0_3/rigid-body-dynamics.md` §5.3 /
//! RBD-η task 3836).
//!
//! Drives `examples/dynamics/closed_2prismatic_idyn.ri` through the full
//! parse → `parse_and_compile_with_stdlib` → `Engine::build` pipeline and
//! asserts the virtual-work POWER identity for a vertical 2-prismatic CLOSED
//! loop under gravity:
//!
//!     Σ τ_i·q̇_i  =  (m_a + m_b)·v·(a + 9.81)
//!                =  (2 + 3)·0.7·(1.5 + 9.81)  =  39.585 W
//!
//! ── What this validates (transparent scope) ───────────────────────────────────
//! End-to-end Value-level marshalling of a CLOSED mechanism (non-empty
//! `loop_closures`) through `inverse_dynamics(m, traj)`: multi-body RNEA, M
//! assembly, loop detection + chain extraction + constraint-Jacobian assembly +
//! rank reduction, the KKT solve, and a physically-correct gravity-loaded energy
//! rate. Strictly stronger than the pure-Rust step-7 finiteness smoke test
//! (`reify-stdlib/.../dynamics/eval.rs::closed_chain_inverse_dynamics_routing_finite_on_prismatic_loop`,
//! which has q̇=q̈=0 and no gravity work).
//!
//! ── What this does NOT validate (and why) ─────────────────────────────────────
//! For a prismatic-closing loop whose closing joint shares the residual axis,
//! `reduce_constraint_rank` projects out the entire residual row → `m_eff = 0`:
//! there is no LIVE constraint, so the closed path reduces to per-DOF open-chain
//! RNEA (τ = τ_open). The power identity Σ τ_i·q̇_i = τ_open·q̇ = dE/dt is exact
//! by the work-energy theorem and holds for `m_eff = 0` too, but this fixture
//! therefore does NOT exercise the nonzero-constraint machinery (λ, `m_eff ≥ 1`,
//! incidence map, rank reduction to a non-empty A). That machinery is covered by
//! the existing array-level unit tests (steps 3–6 — incl. a synthetic revolute
//! rank-reduction case — in `closed_chain.rs` / `loop_closure.rs` / `rnea.rs`).
//! A live-constraint (`m_eff ≥ 1`) *e2e* requires the deferred kinematic
//! inter-joint-offset feature (docs/prds/v0_6/kinematic-inter-joint-offsets.md),
//! which the current kinematic layer cannot express (esc-4146-280).
//!
//! Kernel-INDEPENDENT: `inverse_dynamics` derives mass from each body's
//! `MassProperties` solid and needs no `GeometryKernel`, so a
//! `MockGeometryKernel` suffices (mirrors `rigid_body_dynamics_e2e.rs`).

use reify_ir::{ExportFormat, Value};
use reify_test_support::{errors_only, parse_and_compile_with_stdlib, MockGeometryKernel};

/// Absolute path to the closed 2-prismatic inverse-dynamics example fixture.
/// Mirrors the CARGO_MANIFEST_DIR pattern from `rigid_body_dynamics_e2e.rs`.
const EXAMPLE_PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../examples/dynamics/closed_2prismatic_idyn.ri"
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

/// `inverse_dynamics(m, traj)` on the vertical 2-prismatic CLOSED loop yields a
/// finite `List<List<JointForce>>` of shape 1×2 whose two prismatic
/// `ScalarForce` magnitudes (τ_a, τ_b) satisfy the virtual-work power identity
/// `Σ τ_i·q̇_i = (m_a+m_b)·v·(a+9.81) = 39.585 W` within 1 µW.
#[test]
fn closed_2prismatic_virtual_work_identity() {
    let source = std::fs::read_to_string(EXAMPLE_PATH).expect(
        "examples/dynamics/closed_2prismatic_idyn.ri should exist (task 4146 fixture)",
    );

    let compiled = parse_and_compile_with_stdlib(&source);
    assert!(
        errors_only(&compiled).is_empty(),
        "closed_2prismatic_idyn.ri should compile with no error-severity diagnostics, got:\n{:#?}",
        errors_only(&compiled)
    );

    // Kernel-independent: inverse_dynamics reads mass from each body's
    // MassProperties solid, so a plain mock kernel is enough.
    let checker = reify_constraints::SimpleConstraintChecker;
    let kernel = MockGeometryKernel::new();
    let mut engine = reify_eval::Engine::new(Box::new(checker), Some(Box::new(kernel)));
    let result = engine.build(&compiled, ExportFormat::Step);

    // Locate the top-level `forces` cell. structure_def = `Closed2PrismaticIdyn`;
    // the closed-chain inverse-dynamics result binds to `forces`.
    let cell = reify_core::ValueCellId::new("Closed2PrismaticIdyn", "forces");
    let per_sample = match result.values.get(&cell) {
        Some(Value::List(s)) => s,
        other => panic!(
            "Closed2PrismaticIdyn.forces must be a List<List<JointForce>>, got {other:?}\n\
             (NOT Undef ⇒ closed routing wired; all diagnostics: {:#?})",
            result.diagnostics
        ),
    };
    assert_eq!(
        per_sample.len(),
        1,
        "one trajectory sample ⇒ one inner force list"
    );

    // Inner List<JointForce>: length = tree-joint count = 2 (j_a, j_b).
    // n_tree = bodies − loop_closures = 3 − 1 = 2 (closing body m_c excluded).
    let forces = match &per_sample[0] {
        Value::List(f) => f,
        other => panic!("sample 0: expected a List<JointForce>, got {other:?}"),
    };
    assert_eq!(
        forces.len(),
        2,
        "two spanning-tree joints (j_a, j_b) ⇒ two JointForce entries"
    );

    // The fixture's per-tree-DOF rates (BODIES order: q̇_a, q̇_b). `forces[i]`
    // is returned in the same bodies order, so forces[i] pairs with q_dot[i].
    let q_dot = [0.7_f64, 0.7_f64];

    // Σ τ_i·q̇_i over the returned (signed) prismatic generalized forces.
    let mut power = 0.0_f64;
    for (i, jf) in forces.iter().enumerate() {
        let value = field(jf, "JointForce", "value");
        // Both joints are prismatic ⇒ ScalarForce { magnitude } (signed f64).
        let mag = num(field(value, "ScalarForce", "magnitude"));
        assert!(
            mag.is_finite(),
            "force[{i}].ScalarForce.magnitude must be finite (⇒ KKT nonsingular), got {mag}"
        );
        power += mag * q_dot[i];
    }

    // Virtual-work power identity: dE/dt = (m_a+m_b)·v·(a+9.81)
    //                                    = 5.0·0.7·11.31 = 39.585 W.
    // Exact to numerical roundoff by the work-energy theorem (b ≡ 0, constraint
    // forces do no work on the supplied velocities) ⇒ 1 µW has orders of margin.
    let expected = 39.585_f64;
    assert!(
        (power - expected).abs() < 1e-6,
        "virtual-work power identity Σ τ_i·q̇_i: expected {expected} W, got {power} W \
         (Δ = {} W). A mismatch indicates a real bridge bug — diagnose, do not retune.",
        power - expected
    );
}
