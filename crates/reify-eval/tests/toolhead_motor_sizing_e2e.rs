//! End-to-end fixture test for the RBD-κ printer-toolhead motor-sizing dogfood
//! (`docs/prds/v0_3/rigid-body-dynamics.md` task κ, Phase 5).
//!
//! Drives `examples/dynamics/toolhead_motor_sizing.ri` through the full
//! parse → `parse_and_compile_with_stdlib` → `make_simple_engine` +
//! `register_compute_fns` → `Engine::eval` pipeline and asserts the peak
//! motor force driving a horizontal prismatic X-axis carriage is
//! `M·max_accel = 0.5 kg · 5.0 m/s² = 2.5 N` within 1 µN.
//!
//! ## Physics ground truth
//!
//! For a single horizontal +x prismatic DOF, RNEA gives τ = M·a EXACTLY:
//! gravity `[0, 0, −9.81]` enters the base spatial acceleration as `linear-z`
//! and the +x motion subspace `S = [0,0,0,1,0,0]` projects ZERO gravity onto
//! the x-DOF.  The `ramp_profile` holds |a| = `max_accel` throughout the
//! constant-acceleration triangular phases, so the peak |force| is
//! `M·max_accel` exactly (roundoff ~1e-13 ≪ 1e-6 tolerance).
//!
//! ## Gating model
//!
//! step-1 RED: fails because `examples/dynamics/toolhead_motor_sizing.ri` does
//! not yet exist — `read_to_string` panics with "should exist".
//! step-2 authors the fixture → GREEN.
//!
//! Both sibling dogfood fixtures shipped a dedicated numeric e2e gate:
//! pendulum (η) → `rigid_body_dynamics_e2e.rs`, modal gantry (μ) →
//! `modal_analysis_e2e.rs::e2e_printer_gantry_prints_five_modes`.  This test
//! follows the same pattern: the `forces` cell is exposed in the .ri fixture
//! and reduced to a peak here, because JointForce.value is the marker trait
//! `JointForceValue` (no `.magnitude` member at compile time) and there is no
//! list-aggregate `max` or `report()` builtin in .ri.

use reify_core::ValueCellId;
use reify_eval::compute_targets::register_compute_fns;
use reify_ir::Value;
use reify_test_support::{errors_only, make_simple_engine, parse_and_compile_with_stdlib};

/// Absolute path to the toolhead motor-sizing example fixture.
/// Mirrors the CARGO_MANIFEST_DIR pattern from
/// `crates/reify-eval/tests/rigid_body_dynamics_e2e.rs:30-33`.
const EXAMPLE_PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../examples/dynamics/toolhead_motor_sizing.ri"
);

/// Read an `f64` out of a numeric value cell (`Real` / `Int` / dimensioned
/// `Scalar`).  Panics on a non-numeric cell so a shape regression fails loudly.
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

/// `inverse_dynamics` on a horizontal prismatic X-axis carriage (M = 0.5 kg,
/// max_accel = 5.0 m/s²) yields a `List<List<JointForce>>` whose peak
/// `ScalarForce` magnitude equals `M·max_accel = 2.5 N` within 1 µN.
///
/// The mechanism is driven by `ramp_profile(0.0, 0.0, 0.5, 5.0)` (bare-Real SI
/// args: joint=0.0 placeholder, from=0 m, to=0.5 m, max_accel=5 m/s²).  For a
/// non-zero range the ramp emits multiple samples (≥ 3: accel, cruise, decel
/// phases), so the outer `List` has at least 3 entries.  Each inner list has
/// exactly 1 element (one prismatic joint → one JointForce).
#[test]
fn toolhead_motor_sizing_peak_force_is_2_5_n() {
    let source = std::fs::read_to_string(EXAMPLE_PATH)
        .expect("examples/dynamics/toolhead_motor_sizing.ri should exist (authored by step-2)");

    let compiled = parse_and_compile_with_stdlib(&source);
    assert!(
        errors_only(&compiled).is_empty(),
        "toolhead_motor_sizing.ri should compile with no error-severity diagnostics, got:\n{:#?}",
        errors_only(&compiled)
    );

    let mut engine = make_simple_engine();
    register_compute_fns(&mut engine);
    let eval_result = engine.eval(&compiled);

    // Locate the top-level `forces` cell.  The structure_def is named
    // `ToolheadMotorSizing`; the inverse-dynamics result binds to `forces`.
    let cell = ValueCellId::new("ToolheadMotorSizing", "forces");
    let per_sample = match eval_result.values.get(&cell) {
        Some(Value::List(s)) => s,
        other => panic!(
            "ToolheadMotorSizing.forces must be a List<List<JointForce>>, got {other:?}\n\
             (all diagnostics: {:#?})",
            eval_result.diagnostics
        ),
    };

    // A non-zero ramp_profile (from=0 m, to=0.5 m) emits multiple samples —
    // at minimum the accel, (possibly cruise), and decel phases (≥ 3).
    assert!(
        per_sample.len() >= 3,
        "ramp_profile 0→0.5 m should emit at least 3 samples, got {}",
        per_sample.len()
    );

    // For each sample: one prismatic joint → one JointForce with ScalarForce.
    let mut peak = 0.0_f64;
    for (i, sample) in per_sample.iter().enumerate() {
        let forces = match sample {
            Value::List(f) => f,
            other => panic!("sample {i} must be a List<JointForce>, got {other:?}"),
        };
        assert_eq!(
            forces.len(),
            1,
            "sample {i}: one prismatic joint ⇒ one JointForce, got {}",
            forces.len()
        );

        // prismatic ⇒ JointForce { value: ScalarForce { magnitude } }
        let value = field(&forces[0], "JointForce", "value");
        let magnitude = num(field(value, "ScalarForce", "magnitude"));
        peak = peak.max(magnitude.abs());
    }

    // M·max_accel = 0.5 kg · 5.0 m/s² = 2.5 N.
    // Gravity projects zero onto the horizontal +x DOF, so this is exact up to
    // RNEA floating-point roundoff (~1e-13 ≪ 1e-6 tolerance).
    let expected = 2.5_f64;
    assert!(
        (peak - expected).abs() < 1e-6,
        "expected peak force {expected} N (M·max_accel), got {peak} N"
    );
}
