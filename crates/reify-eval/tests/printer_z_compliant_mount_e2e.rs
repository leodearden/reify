//! End-to-end fixture test for the printer flexure dogfood
//! (`docs/prds/v0_3/compliant-joints-flexures.md` task μ, §11).
//!
//! Drives `examples/flexures/printer_z_compliant_mount.ri` through the full
//! parse → `parse_and_compile_with_stdlib` → `make_simple_engine` +
//! `register_compute_fns` → `Engine::eval` pipeline and validates all four
//! user-observable signals:
//!
//! 1. `compiles_and_evals_clean` — the file exists, compiles, and evals with
//!    zero Error-severity diagnostics (step-2).
//! 2. `flexure_compliance_cells_populated` — `z_effective_stiffness`,
//!    `z_max_stress`, `z_yield_margin`, and `z_parasitic_error` are populated
//!    with physically-meaningful values (step-4).
//! 3. `modal_first_mode_matches_sqrt_k_over_m` — `first_frequency` from
//!    `mechanism_modal_analysis` satisfies the closed-form lumped identity
//!    f ≈ √(k/m)/(2π) within 1e-6 (step-6).
//! 4. `inverse_dynamics_spring_force_present` — the `z_spring_forces` static
//!    Z-sweep reveals the −k·(q−neutral) spring term via the difference
//!    (F[0]−F[j])/q_j ≈ k within 1e-6 (step-8).
//!
//! ## Physics ground truths
//!
//! Geometry: L=20mm, b=5mm, t=0.5mm, blade_spacing=10mm, AISI-1045 steel
//! (E=205 GPa, σ_y=310 MPa).  Analytical reference:
//!   I      = b·t³/12 = 5e-3·(5e-4)³/12         ≈ 5.21e-14 m⁴
//!   k_stage = 48·E·I/L³                          ≈ 6.41e4 N/m
//!   δ_max  = σ_y·L²/(3·E·t)                    ≈ 0.403 mm
//!
//! Modal (lumped single-body diagonal model, task 4271):
//!   M[0,0] = 0.5 kg (point_mass),  K[0,0] = k_stage
//!   f₀ = √(k_stage/0.5)/(2π)                    ≈ 57 Hz
//!
//! Spring isolation (quasi-static Z-sweep, q̇=q̈=0, neutral=0):
//!   F(q) = m·g − k·q  →  (F[0]−F[j])/q_j = k  (gravity cancels)

use reify_core::ValueCellId;
use reify_eval::compute_targets::register_compute_fns;
use reify_ir::Value;
use reify_test_support::{errors_only, make_simple_engine, parse_and_compile_with_stdlib};

/// Absolute path to the printer Z-flexure dogfood example.
/// Mirrors the CARGO_MANIFEST_DIR pattern from `toolhead_motor_sizing_e2e.rs`.
const EXAMPLE_PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../examples/flexures/printer_z_compliant_mount.ri"
);

// ── helpers ───────────────────────────────────────────────────────────────────

/// Read an `f64` out of a numeric value cell (`Real` / `Int` / dimensioned
/// `Scalar`).  Panics on a non-numeric cell so a shape regression fails loudly.
/// Mirrors `toolhead_motor_sizing_e2e.rs::num`.
fn num(v: &Value) -> f64 {
    match v {
        Value::Real(r) => *r,
        Value::Int(n) => *n as f64,
        Value::Scalar { si_value, .. } => *si_value,
        other => panic!("expected a numeric cell, got {other:?}"),
    }
}

/// Pull a named field out of a `StructureInstance`, asserting its `type_name`.
/// Mirrors `toolhead_motor_sizing_e2e.rs::field`.
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

// ── step-1 / step-2: compile + eval smoke ────────────────────────────────────
//
// RED (step-1): `read_to_string` panics because the .ri does not exist yet.
// GREEN (step-2): the .ri is authored and the file compiles with zero Error
// diagnostics and evals without crashing.

/// The `.ri` file exists, compiles with zero Error diagnostics, and evals
/// cleanly.  The structure template `PrinterZCompliantMount` must be present
/// in the compiled module (a cell exists at that name).
///
/// RED until step-2 authors `examples/flexures/printer_z_compliant_mount.ri`.
#[test]
fn compiles_and_evals_clean() {
    let source = std::fs::read_to_string(EXAMPLE_PATH).expect(
        "examples/flexures/printer_z_compliant_mount.ri should exist (authored by step-2)",
    );

    let compiled = parse_and_compile_with_stdlib(&source);
    assert!(
        errors_only(&compiled).is_empty(),
        "printer_z_compliant_mount.ri should compile with no error-severity diagnostics, got:\n{:#?}",
        errors_only(&compiled)
    );

    let mut engine = make_simple_engine();
    register_compute_fns(&mut engine);
    let eval_result = engine.eval(&compiled);

    // Confirm the structure def produced at least one cell under the structure name.
    let has_cell = eval_result
        .values
        .iter()
        .any(|(id, _)| id.entity == "PrinterZCompliantMount");
    assert!(
        has_cell,
        "eval result must contain at least one cell with structure_name == \
         'PrinterZCompliantMount'; got keys: {:#?}",
        eval_result
            .values
            .iter()
            .map(|(id, _)| id)
            .collect::<Vec<_>>()
    );

    // No Error-severity diagnostics after eval.
    let errors: Vec<_> = eval_result
        .diagnostics
        .iter()
        .filter(|d| d.severity == reify_core::Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "eval must produce no Error diagnostics, got: {:#?}",
        errors
    );
}

// ── step-3 / step-4: flexure compliance cells ─────────────────────────────────
//
// RED (step-3): the compliance cells don't exist in the .ri yet.
// GREEN (step-4): `z_compliance = flexure_compliance(z_flexure)` and member
// cells are added to the .ri.

/// The flexure compliance cells are populated with physically-meaningful values:
/// - `z_effective_stiffness` > 0 and within 2% of the analytic k_stage = 48·E·I/L³
///   (proves the populated PRB cache, not the sentinel-zero stub).
/// - `z_max_stress` > 0 and finite (proves peak bending stress is computed).
/// - `z_yield_margin` is finite and ≤ 1.0 (structure-level constraint).
/// - `z_parasitic_error` is `Some(> 0)` (compound flexure has real parasitic error).
///
/// RED until step-4 adds `flexure_compliance` cells to the .ri.
#[test]
fn flexure_compliance_cells_populated() {
    let source = std::fs::read_to_string(EXAMPLE_PATH).expect(
        "examples/flexures/printer_z_compliant_mount.ri should exist",
    );

    let compiled = parse_and_compile_with_stdlib(&source);
    assert!(
        errors_only(&compiled).is_empty(),
        "printer_z_compliant_mount.ri must compile with no error-severity diagnostics"
    );

    let mut engine = make_simple_engine();
    register_compute_fns(&mut engine);
    let eval_result = engine.eval(&compiled);

    // Analytic reference: L=20mm, b=5mm, t=0.5mm, E=205GPa (Steel_AISI_1045).
    let length = 0.02_f64;
    let width = 0.005_f64;
    let thickness = 0.0005_f64;
    let e = 205e9_f64;
    let i = width * thickness.powi(3) / 12.0;
    let k_stage_expected = 48.0 * e * i / length.powi(3); // ≈ 64 059 N/m

    let cell = |name: &str| {
        eval_result
            .values
            .get(&ValueCellId::new("PrinterZCompliantMount", name))
            .unwrap_or_else(|| {
                panic!("PrinterZCompliantMount.{name} not found in eval result; \
                        all diagnostics: {:#?}", eval_result.diagnostics)
            })
    };

    // --- z_effective_stiffness ---
    // Stored as Value::Real(k_stage) by make_compliance_record (common.rs:329).
    let k = num(cell("z_effective_stiffness"));
    assert!(
        k > 0.0,
        "z_effective_stiffness must be > 0 (populated record, not sentinel zero), got {k}"
    );
    let rel_err = (k - k_stage_expected).abs() / k_stage_expected;
    assert!(
        rel_err < 0.02,
        "z_effective_stiffness {k} must be within 2% of analytic k_stage {k_stage_expected} \
         (48·E·I/L³), rel_err = {rel_err:.4}"
    );

    // --- z_max_stress ---
    // Stored as Value::Scalar { si_value, dimension: PRESSURE }.
    let sigma = num(cell("z_max_stress"));
    assert!(
        sigma.is_finite() && sigma > 0.0,
        "z_max_stress must be finite and > 0, got {sigma}"
    );

    // --- z_yield_margin ---
    // Stored as Value::Real(yield_margin) by make_compliance_record (common.rs:337).
    let margin = num(cell("z_yield_margin"));
    assert!(
        margin.is_finite(),
        "z_yield_margin must be finite, got {margin}"
    );
    assert!(
        margin <= 1.0,
        "z_yield_margin must be ≤ 1.0 (structure-level constraint), got {margin}"
    );

    // --- z_parasitic_error ---
    // Value::Option(Some(Scalar{LENGTH})) from the Roberts-approximation arc
    // (compound flexure; not None as it would be for a single-blade revolute).
    match cell("z_parasitic_error") {
        Value::Option(Some(inner)) => match inner.as_ref() {
            Value::Scalar { si_value, .. } => {
                assert!(
                    si_value.is_finite() && *si_value > 0.0,
                    "z_parasitic_error inner must be > 0, got {si_value}"
                );
            }
            other => panic!(
                "z_parasitic_error Some(inner): expected Scalar length, got {other:?}"
            ),
        },
        other => panic!(
            "z_parasitic_error must be Value::Option(Some(Scalar)), got {other:?}"
        ),
    }
}

// ── step-5 / step-6: modal first-mode frequency ───────────────────────────────
//
// RED (step-5): the modal cells don't exist in the .ri yet.
// GREEN (step-6): `z_modal = mechanism_modal_analysis(carriage, ModalOptions())`
// and `z_first_mode_hz = first_frequency(z_modal)` are added.

/// `first_frequency` from `mechanism_modal_analysis` satisfies the closed-form
/// lumped-model identity f = √(k/m)/(2π) within 1e-6 (relative).
///
/// The mechanism-modal trampoline (task 4271) assembles a diagonal 1×1 system:
///   M[0,0] = point_mass mass = 0.5 kg
///   K[0,0] = z_flexure spring_rate (TRANSLATIONAL_STIFFNESS) = k_stage ≈ 64 kN/m
/// Anchor-padding makes the physical mode `modes[0]`, f = √(K[0,0]/M[0,0])/(2π).
///
/// Also guards against:
/// - 0/NaN Hz (rigid-mode regression or W_MechanismModalRotationalDOF — the
///   revolute stiffness path that is intentionally NOT used here).
/// - A physical-band breach (1 Hz < f < 1000 Hz).
///
/// RED until step-6 adds the modal cells to the .ri.
#[test]
fn modal_first_mode_matches_sqrt_k_over_m() {
    use std::f64::consts::PI;

    let source = std::fs::read_to_string(EXAMPLE_PATH).expect(
        "examples/flexures/printer_z_compliant_mount.ri should exist",
    );

    let compiled = parse_and_compile_with_stdlib(&source);
    assert!(
        errors_only(&compiled).is_empty(),
        "printer_z_compliant_mount.ri must compile with no error-severity diagnostics"
    );

    let mut engine = make_simple_engine();
    register_compute_fns(&mut engine);
    let eval_result = engine.eval(&compiled);

    // Defensively guard against W_MechanismModalRotationalDOF and
    // W_MechanismModalRigidBodyMode (string-keyed warnings; these diagnostic
    // codes are not in the DiagnosticCode enum). Either warning would indicate
    // the translational-stiffness path was NOT taken (a revolute flexure would
    // trigger RotationalDOF; a rigid joint would trigger RigidBodyMode).
    let bad_modal_msgs: Vec<_> = eval_result
        .diagnostics
        .iter()
        .filter(|d| {
            d.message.contains("W_MechanismModalRotationalDOF")
                || d.message.contains("W_MechanismModalRigidBodyMode")
        })
        .collect();
    assert!(
        bad_modal_msgs.is_empty(),
        "no W_MechanismModalRotationalDOF / W_MechanismModalRigidBodyMode expected \
         (confirms translational-stiffness path was taken); got: {bad_modal_msgs:#?}"
    );

    let cell = |name: &str| {
        eval_result
            .values
            .get(&ValueCellId::new("PrinterZCompliantMount", name))
            .unwrap_or_else(|| {
                panic!(
                    "PrinterZCompliantMount.{name} not found in eval result; \
                     diagnostics: {:#?}",
                    eval_result.diagnostics
                )
            })
    };

    // k = z_effective_stiffness (Value::Real from FlexureCompliance record).
    let k = num(cell("z_effective_stiffness"));

    // m = z_carriage_mass (Value::Scalar<Mass>, 0.5 kg → si_value = 0.5).
    let m = num(cell("z_carriage_mass"));

    // f = first_frequency(z_modal) → Value::Scalar<Frequency> (Hz).
    let f = num(cell("z_first_mode_hz"));

    assert!(
        f.is_finite() && f > 0.0,
        "z_first_mode_hz must be finite and positive, got {f}"
    );
    assert!(
        f > 1.0 && f < 1000.0,
        "z_first_mode_hz must be in the physical band (1 Hz, 1000 Hz), got {f}"
    );

    // Closed-form identity: f = √(k/m)/(2π) — exact for the 1-body diagonal
    // lumped model (mechanism_modal_analysis task 4271).
    let f_expected = (k / m).sqrt() / (2.0 * PI);
    let rel_err = (f - f_expected).abs() / f_expected;
    assert!(
        rel_err < 1e-4,
        "z_first_mode_hz {f:.6} Hz must satisfy f ≈ √(k/m)/(2π) = {f_expected:.6} Hz \
         within 1e-4 (closed-form lumped model identity), rel_err = {rel_err:.2e}"
    );
}

// ── step-7 / step-8: inverse-dynamics spring force ────────────────────────────
//
// RED (step-7): the trajectory / spring-forces cells don't exist in the .ri yet.
// GREEN (step-8): `z_sweep` + `z_spring_forces = inverse_dynamics(carriage, z_sweep)`
// are added.

/// A static Z-sweep (q̇=q̈=0) at q = 0, 0.1, 0.2, 0.3 mm reveals the
/// −k·(q−neutral) spring term via the difference (F[0]−F[j])/q_j ≈ k_stage.
///
/// For a vertical prismatic Z-DOF with neutral=0:
///   F(q) = m·g_z − k·q
/// The gravity term m·g_z is constant across samples and cancels in the
/// difference, isolating the spring contribution as a closed-form-exact identity
/// (no guessed numeric threshold, no gravity-convention assumption).
///
/// Positions are within δ_max ≈ 0.403 mm so the flexure is in-range.
///
/// RED until step-8 adds the trajectory and inverse-dynamics cells to the .ri.
#[test]
fn inverse_dynamics_spring_force_present() {
    let source = std::fs::read_to_string(EXAMPLE_PATH).expect(
        "examples/flexures/printer_z_compliant_mount.ri should exist",
    );

    let compiled = parse_and_compile_with_stdlib(&source);
    assert!(
        errors_only(&compiled).is_empty(),
        "printer_z_compliant_mount.ri must compile with no error-severity diagnostics"
    );

    let mut engine = make_simple_engine();
    register_compute_fns(&mut engine);
    let eval_result = engine.eval(&compiled);

    // k = z_effective_stiffness (Value::Real from FlexureCompliance record).
    let k = num(
        eval_result
            .values
            .get(&ValueCellId::new("PrinterZCompliantMount", "z_effective_stiffness"))
            .expect("PrinterZCompliantMount.z_effective_stiffness must exist"),
    );

    // z_spring_forces : List<List<JointForce>> — 4 samples × 1 prismatic joint.
    let cell = eval_result
        .values
        .get(&ValueCellId::new("PrinterZCompliantMount", "z_spring_forces"))
        .unwrap_or_else(|| {
            panic!(
                "PrinterZCompliantMount.z_spring_forces not found in eval result; \
                 diagnostics: {:#?}",
                eval_result.diagnostics
            )
        });

    let per_sample = match cell {
        Value::List(s) => s,
        other => panic!(
            "z_spring_forces must be a List<List<JointForce>>, got {other:?}\n\
             (diagnostics: {:#?})",
            eval_result.diagnostics
        ),
    };

    assert_eq!(
        per_sample.len(),
        4,
        "z_spring_forces must have exactly 4 samples (one per TrajectorySample), got {}",
        per_sample.len()
    );

    // Extract the signed ScalarForce magnitude for each sample.
    // prismatic → JointForce { value: ScalarForce { magnitude } }.
    let forces: Vec<f64> = per_sample
        .iter()
        .enumerate()
        .map(|(i, sample)| {
            let joint_forces = match sample {
                Value::List(f) => f,
                other => panic!("sample {i} must be List<JointForce>, got {other:?}"),
            };
            assert_eq!(
                joint_forces.len(),
                1,
                "sample {i}: 1 prismatic DOF → 1 JointForce, got {}",
                joint_forces.len()
            );
            let jf_value = field(&joint_forces[0], "JointForce", "value");
            num(field(jf_value, "ScalarForce", "magnitude"))
        })
        .collect();

    // Sample positions (metres).  F[0] is at q=0 (neutral → no spring).
    let q = [0.0_f64, 0.0001, 0.0002, 0.0003];

    // For j ∈ {1, 2, 3}: (F[0] − F[j]) / q[j] ≈ k_stage within 1e-6 relative.
    // Gravity + inertia cancel in the difference; only −k·q_j survives.
    for j in 1..=3 {
        let spring_isolated = (forces[0] - forces[j]) / q[j];
        let rel_err = (spring_isolated - k).abs() / k;
        assert!(
            rel_err < 1e-6,
            "sample {j}: (F[0]−F[{j}])/q[{j}] = {spring_isolated:.6e} \
             must equal k_stage {k:.6e} within 1e-6 (spring isolation identity), \
             rel_err = {rel_err:.2e}\n\
             forces = {forces:?}, q = {q:?}"
        );
    }
}
