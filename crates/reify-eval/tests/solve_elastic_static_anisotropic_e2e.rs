//! Anisotropic (orthotropic) end-to-end integration test for
//! `solve_elastic_static_trampoline` (task δ/3780 step-5 RED → step-6 GREEN).
//!
//! Calls `solve_elastic_static_trampoline` **directly** with a hand-crafted
//! `value_inputs` slice that has an `OrthotropicMaterial` StructureInstance at
//! index [0].  The current trampoline's isotropic-only `extract_material` call
//! panics on this input (no `youngs_modulus` field), so the test is RED until
//! step-6 adds anisotropic material classification.
//!
//! ## Assertions (once GREEN)
//!
//! 1. No panic — the trampoline completes successfully.
//! 2. `result` is a `Value::StructureInstance` with type_name `"ElasticResult"`.
//! 3. All six ElasticResult fields are present:
//!    `displacement`, `stress`, `frame` (all `Value::Undef`),
//!    `max_von_mises` (`Value::Scalar[PRESSURE]`, finite & > 0),
//!    `converged` (`Value::Bool(true)`),
//!    `iterations` (`Value::Int(n)` with n ≥ 0).
//! 4. `ComputeOutcome::Completed` is returned (not Err / Skip).
//!
//! ## Material fixture
//!
//! E1=200 GPa (beam axis), E2=E3=10 GPa, G12=G13=G23=4 GPa, nu12=nu13=nu23=0.3.
//! L=0.8 m, b=h=0.1 m (L/h = 8), P = 1000 N tip load.
//! Identity material frame: beam axis = material 1-axis.

use reify_core::DimensionVector;
use reify_eval::{CancellationHandle, ComputeOutcome};
use reify_ir::{PersistentMap, StructureInstanceData, StructureTypeId, Value};

// ── helpers: build Value::StructureInstance payloads ─────────────────────────

/// Build an `OrthotropicMaterial` `Value::StructureInstance` carrying the 9
/// orthotropic constants required by the trampoline classifier (step-6).
///
/// The elastic-modulus fields (e1,e2,e3,g12,g13,g23) are `Value::Scalar` with
/// dimension `PRESSURE`; the Poisson ratio fields (nu12,nu13,nu23) are
/// `Value::Real`.
#[allow(clippy::too_many_arguments)]
fn make_orthotropic_material(
    e1: f64, e2: f64, e3: f64,
    g12: f64, g13: f64, g23: f64,
    nu12: f64, nu13: f64, nu23: f64,
) -> Value {
    let fields: PersistentMap<String, Value> = [
        ("e1".to_string(),   Value::Scalar { si_value: e1,  dimension: DimensionVector::PRESSURE }),
        ("e2".to_string(),   Value::Scalar { si_value: e2,  dimension: DimensionVector::PRESSURE }),
        ("e3".to_string(),   Value::Scalar { si_value: e3,  dimension: DimensionVector::PRESSURE }),
        ("g12".to_string(),  Value::Scalar { si_value: g12, dimension: DimensionVector::PRESSURE }),
        ("g13".to_string(),  Value::Scalar { si_value: g13, dimension: DimensionVector::PRESSURE }),
        ("g23".to_string(),  Value::Scalar { si_value: g23, dimension: DimensionVector::PRESSURE }),
        ("nu12".to_string(), Value::Real(nu12)),
        ("nu13".to_string(), Value::Real(nu13)),
        ("nu23".to_string(), Value::Real(nu23)),
    ]
    .into_iter()
    .collect();
    Value::StructureInstance(Box::new(StructureInstanceData {
        type_id:   StructureTypeId(u32::MAX),
        type_name: "OrthotropicMaterial".to_string(),
        version:   1,
        fields,
    }))
}

/// Build a `Value::Scalar` for a geometry length (SI: metres).
fn make_length_scalar(metres: f64) -> Value {
    Value::Scalar { si_value: metres, dimension: DimensionVector::LENGTH }
}

/// Build a `Value::List` containing one `PointLoad` with the given force (N).
/// The trampoline's `extract_tip_force` reads the `force: Value::Real` field.
fn make_point_load_list(force_n: f64) -> Value {
    let fields: PersistentMap<String, Value> =
        [("force".to_string(), Value::Real(force_n))].into_iter().collect();
    let point_load = Value::StructureInstance(Box::new(StructureInstanceData {
        type_id:   StructureTypeId(u32::MAX),
        type_name: "PointLoad".to_string(),
        version:   1,
        fields,
    }));
    Value::List(vec![point_load])
}

/// Build a `Value::List` with one `FixedSupport` instance (fields are not
/// inspected by the trampoline; presence is sufficient for BC application).
fn make_support_list() -> Value {
    let fields: PersistentMap<String, Value> = [].into_iter().collect();
    let support = Value::StructureInstance(Box::new(StructureInstanceData {
        type_id:   StructureTypeId(u32::MAX),
        type_name: "FixedSupport".to_string(),
        version:   1,
        fields,
    }));
    Value::List(vec![support])
}

/// Build a minimal `ElasticOptions` instance (fields are not inspected;
/// defaults are applied inside the trampoline).
fn make_elastic_options() -> Value {
    let fields: PersistentMap<String, Value> = [].into_iter().collect();
    Value::StructureInstance(Box::new(StructureInstanceData {
        type_id:   StructureTypeId(u32::MAX),
        type_name: "ElasticOptions".to_string(),
        version:   1,
        fields,
    }))
}

// ── step-5 RED test ───────────────────────────────────────────────────────────
//
// RED: the current trampoline's `extract_material` panics on an
// `OrthotropicMaterial` (no `youngs_modulus` field).  Step-6 adds material
// classification so the trampoline dispatches the anisotropic branch.

/// Orthotropic cantilever trampoline e2e: OrthotropicMaterial value_inputs →
/// solve_elastic_static_trampoline → ElasticResult StructureInstance.
///
/// Fixture: E1=200 GPa (beam axis), E2=E3=10 GPa, G12=G13=G23=4 GPa,
/// nu12=nu13=nu23=0.3, L=0.8 m, b=h=0.1 m, P=1000 N.
///
/// Assertions:
/// - `ComputeOutcome::Completed` (not panic, not Err).
/// - Result is `Value::StructureInstance(type_name = "ElasticResult")`.
/// - `converged` == `Bool(true)`.
/// - `max_von_mises` is `Scalar[PRESSURE]`, finite and > 0.
/// - `iterations` is `Int(n)` with n ≥ 0.
/// - `displacement`, `stress`, `frame` are all `Value::Undef`.
#[test]
fn orthotropic_trampoline_e2e_returns_elastic_result() {
    // ── material ──────────────────────────────────────────────────────────────
    let material = make_orthotropic_material(
        200e9_f64, // e1: 200 GPa (beam axis)
        10e9_f64,  // e2: 10 GPa
        10e9_f64,  // e3: 10 GPa
        4e9_f64,   // g12: 4 GPa
        4e9_f64,   // g13: 4 GPa
        4e9_f64,   // g23: 4 GPa
        0.3_f64,   // nu12
        0.3_f64,   // nu13
        0.3_f64,   // nu23
    );

    // ── geometry (L=0.8 m, b=h=0.1 m → L/h = 8) ─────────────────────────────
    let length  = make_length_scalar(0.8);
    let width   = make_length_scalar(0.1);
    let height  = make_length_scalar(0.1);

    // ── loads and supports ────────────────────────────────────────────────────
    let loads    = make_point_load_list(1000.0);  // 1 kN tip load
    let supports = make_support_list();
    let options  = make_elastic_options();

    let value_inputs = [material, length, width, height, loads, supports, options];

    // ── call trampoline ───────────────────────────────────────────────────────
    //
    // RED: current trampoline calls extract_material which panics because
    // OrthotropicMaterial has no `youngs_modulus` field.
    // GREEN (step-6): trampoline classifies by type_name and dispatches
    // MaterialModel::Anisotropic.
    let cancellation = CancellationHandle::new();
    let outcome = reify_eval::compute_targets::elastic_static::solve_elastic_static_trampoline(
        &value_inputs,
        &[],         // no realization inputs
        &Value::Undef,
        None,        // no prior warm state
        &cancellation,
    );

    // ── assert ComputeOutcome::Completed ─────────────────────────────────────
    let result = match outcome {
        ComputeOutcome::Completed { result, .. } => result,
        other => panic!(
            "expected ComputeOutcome::Completed, got: {:?}",
            other
        ),
    };

    // ── assert ElasticResult StructureInstance ────────────────────────────────
    let data = match &result {
        Value::StructureInstance(d) => d,
        other => panic!(
            "expected result to be Value::StructureInstance, got: {:?}",
            other
        ),
    };
    assert_eq!(
        data.type_name, "ElasticResult",
        "expected type_name == \"ElasticResult\", got: {:?}",
        data.type_name
    );

    // Helper to fetch a named field.
    let get = |key: &str| -> &Value {
        data.fields.get(&key.to_string())
            .unwrap_or_else(|| panic!("ElasticResult missing field {:?}", key))
    };

    // ── displacement / stress → Sampled Field (task 4084/α) ─────────────────
    // α populates displacement + stress as Regular3D Sampled Value::Field;
    // the old Undef assertions are voided (same rationale as removing
    // tet_trampoline_stress_is_undef in solve_elastic_static_e2e.rs step-5).
    assert!(
        matches!(get("displacement"), Value::Field { .. }),
        "displacement must be a Value::Field after task 4084/α, got: {:?}",
        get("displacement")
    );
    assert!(
        matches!(get("stress"), Value::Field { .. }),
        "stress must be a Value::Field after task 4084/α, got: {:?}",
        get("stress")
    );
    // ── frame → Undef (tet convention, unchanged) ─────────────────────────────
    assert_eq!(
        get("frame"), &Value::Undef,
        "frame must remain Undef (tet/solid: no per-element local frame)"
    );

    // ── converged → Bool(true) ────────────────────────────────────────────────
    assert_eq!(
        get("converged"), &Value::Bool(true),
        "expected converged == Bool(true)"
    );

    // ── iterations → Int(n ≥ 0) ──────────────────────────────────────────────
    match get("iterations") {
        Value::Int(n) => assert!(
            *n >= 0,
            "expected iterations ≥ 0, got: {}",
            n
        ),
        other => panic!("expected iterations to be Value::Int, got: {:?}", other),
    }

    // ── max_von_mises → Scalar[PRESSURE], finite, > 0 ────────────────────────
    match get("max_von_mises") {
        Value::Scalar { si_value, dimension } => {
            assert_eq!(
                *dimension,
                DimensionVector::PRESSURE,
                "expected max_von_mises dimension == PRESSURE, got: {:?}",
                dimension
            );
            assert!(
                si_value.is_finite(),
                "expected max_von_mises to be finite, got: {}",
                si_value
            );
            assert!(
                *si_value > 0.0,
                "expected max_von_mises > 0, got: {}",
                si_value
            );
        }
        other => panic!(
            "expected max_von_mises to be Value::Scalar, got: {:?}",
            other
        ),
    }
}
