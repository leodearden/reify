//! Heterogeneous `AsPrintedZones` field end-to-end integration test for
//! `solve_elastic_static_trampoline` (task #4757 step-3 RED → step-4 GREEN).
//!
//! Calls `solve_elastic_static_trampoline` **directly** with:
//! - `value_inputs[0]` = a hand-built `Value::Field { source: AsPrintedZones }`
//!   whose lambda encodes a two-zone (skin-stiff / infill-soft) material field.
//! - `value_inputs[0]` = a homogeneous `OrthotropicMaterial` baseline.
//!
//! ## RED state
//!
//! The current trampoline's `classify_material` panics on any non-StructureInstance
//! material value (`other => panic!`), so both calls fail if the field is passed.
//! Step-4 adds the `MaterialModel::Heterogeneous(DiscreteCellField)` arm that
//! handles `FieldSourceKind::AsPrintedZones`.
//!
//! ## Assertions (once GREEN)
//!
//! - Both calls return `ComputeOutcome::Completed` (no panic, no Err).
//! - Both results are `Value::StructureInstance("ElasticResult")`.
//! - Both have `converged = Bool(true)`.
//! - Both have displacement/stress as `Value::Field { source: Sampled }`.
//! - `max_deflection_magnitude(hetero_disp) ≠ max_deflection_magnitude(homo_disp)`
//!   by a relative margin > 1e-3.  (Softer infill → larger heterogeneous deflection.)
//!
//! ## Fixture
//!
//! L=0.8 m, W=H=0.1 m cantilever; tip force [0,0,-1000 N].
//! Zone field: build_z=[1,0,0] (beam axis), wall_thickness=0.04 m, skin_thickness=0.08 m.
//! With the 6-block mesh, end blocks (x centroid ≈ 0.067 < 0.08) → skin (stiff, E=200 GPa);
//! middle blocks → infill (soft, E=40 GPa).  Homogeneous baseline: E=200 GPa everywhere.
//! Expected direction: heterogeneous deflection > homogeneous (softer infill dominates).

// Shared AsPrintedZones Value-fixture builders (single source of truth for
// the lambda layout, shared with the in-module tests in elastic_static.rs).
mod as_printed_zones_test_fixtures;
use as_printed_zones_test_fixtures::het_as_printed_field;

use reify_core::DimensionVector;
use reify_eval::{CancellationHandle, ComputeOutcome};
use reify_ir::{FieldSourceKind, PersistentMap, StructureInstanceData, StructureTypeId, Value};

// ── local helpers (not shared) ────────────────────────────────────────────────

fn pressure_scalar(pa: f64) -> Value {
    Value::Scalar { si_value: pa, dimension: DimensionVector::PRESSURE }
}

fn make_length_scalar(metres: f64) -> Value {
    Value::Scalar { si_value: metres, dimension: DimensionVector::LENGTH }
}

fn make_point_load_list(force_n: f64) -> Value {
    let fields: PersistentMap<String, Value> =
        [("force".to_string(), Value::Real(force_n))].into_iter().collect();
    Value::List(vec![Value::StructureInstance(Box::new(StructureInstanceData {
        type_id: StructureTypeId(u32::MAX),
        type_name: "PointLoad".to_string(),
        version: 1,
        fields,
    }))])
}

fn make_support_list() -> Value {
    Value::List(vec![Value::StructureInstance(Box::new(StructureInstanceData {
        type_id: StructureTypeId(u32::MAX),
        type_name: "FixedSupport".to_string(),
        version: 1,
        fields: [].into_iter().collect(),
    }))])
}

fn make_elastic_options() -> Value {
    Value::StructureInstance(Box::new(StructureInstanceData {
        type_id: StructureTypeId(u32::MAX),
        type_name: "ElasticOptions".to_string(),
        version: 1,
        fields: [].into_iter().collect(),
    }))
}

/// Extract the displacement data from a `Value::Field { source: Sampled }` lambda.
/// Returns an owned copy to avoid lifetime complications with the nested Arc.
fn extract_displacement_data(disp_field: &Value) -> Vec<f64> {
    match disp_field {
        Value::Field { source: FieldSourceKind::Sampled, lambda, .. } => match lambda.as_ref() {
            Value::SampledField(sf) => sf.data.clone(),
            other => panic!("expected SampledField lambda, got {:?}", other),
        },
        other => panic!("expected sampled Value::Field for displacement, got {:?}", other),
    }
}

// ── test ─────────────────────────────────────────────────────────────────────

/// Heterogeneous two-zone trampoline e2e:
///
/// - `value_inputs[0]` = `Value::Field { source: AsPrintedZones }` (two-zone)
/// - `value_inputs[0]` = `Value::StructureInstance("OrthotropicMaterial")` (baseline)
///
/// Both calls must complete without panic; the heterogeneous deflection must differ
/// from the homogeneous deflection by more than 0.1% relative (>1e-3 relative margin).
///
/// Expected direction: softer infill → larger heterogeneous deflection.
///
/// RED: `classify_material` currently panics on any non-StructureInstance material.
/// GREEN (step-4): trampoline classifies `AsPrintedZones` → `MaterialModel::Heterogeneous`.
#[test]
fn heterogeneous_trampoline_e2e_deflection_differs_from_homogeneous() {
    const L: f64 = 0.8;
    const W: f64 = 0.1;
    const H: f64 = 0.1;
    const E_STIFF: f64 = 200e9;
    const E_SOFT: f64 = 40e9;
    const NU: f64 = 0.3;

    // ── heterogeneous field ───────────────────────────────────────────────────
    // build_z=[1,0,0]: x is build direction.  wall_thickness=0.04 (< y/z centroid
    // dist=0.05 → no wall elements); skin_thickness=0.08 (> end-block centroid-to-
    // x-end ≈ 0.067 → end elements are skin, rest infill).
    let hetero_material = het_as_printed_field(
        [0.0, 0.0, 0.0], [L, W, H],
        [1.0, 0.0, 0.0],  // build_z = +x
        1.0, 0.04,        // walls=1, line_width=0.04
        1.0, 0.08,        // layers=1, layer_height=0.08
        E_STIFF, E_SOFT,
    );

    // ── homogeneous baseline: all-stiff OrthotropicMaterial ──────────────────
    let homo_material = Value::StructureInstance(Box::new(StructureInstanceData {
        type_id: StructureTypeId(u32::MAX),
        type_name: "OrthotropicMaterial".to_string(),
        version: 1,
        fields: {
            let g = E_STIFF / (2.0 * (1.0 + NU));
            [
                ("e1".to_string(), pressure_scalar(E_STIFF)),
                ("e2".to_string(), pressure_scalar(E_STIFF)),
                ("e3".to_string(), pressure_scalar(E_STIFF)),
                ("g12".to_string(), pressure_scalar(g)),
                ("g13".to_string(), pressure_scalar(g)),
                ("g23".to_string(), pressure_scalar(g)),
                ("nu12".to_string(), Value::Real(NU)),
                ("nu13".to_string(), Value::Real(NU)),
                ("nu23".to_string(), Value::Real(NU)),
            ].into_iter().collect()
        },
    }));

    // ── common geometry / loads / supports / options ──────────────────────────
    let length  = make_length_scalar(L);
    let width   = make_length_scalar(W);
    let height  = make_length_scalar(H);
    let loads    = make_point_load_list(1000.0);  // 1 kN tip load (-z)
    let supports = make_support_list();
    let options  = make_elastic_options();

    let run_trampoline = |material: Value| {
        let value_inputs = [material, length.clone(), width.clone(), height.clone(),
                            loads.clone(), supports.clone(), options.clone()];
        let cancellation = CancellationHandle::new();
        reify_eval::compute_targets::elastic_static::solve_elastic_static_trampoline(
            &value_inputs, &[], &Value::Undef, None, &cancellation,
        )
    };

    // ── call trampoline for each material ─────────────────────────────────────
    let outcome_hetero = run_trampoline(hetero_material);
    let outcome_homo   = run_trampoline(homo_material);

    // ── assert Completed ──────────────────────────────────────────────────────
    let result_hetero = match outcome_hetero {
        ComputeOutcome::Completed { result, .. } => result,
        other => panic!("heterogeneous: expected Completed, got {:?}", other),
    };
    let result_homo = match outcome_homo {
        ComputeOutcome::Completed { result, .. } => result,
        other => panic!("homogeneous: expected Completed, got {:?}", other),
    };

    // ── assert ElasticResult StructureInstance ────────────────────────────────
    let get_field = |result: &Value, key: &str| {
        match result {
            Value::StructureInstance(d) => {
                assert_eq!(d.type_name, "ElasticResult");
                d.fields.get(key).unwrap_or_else(|| panic!("missing field {key}")).clone()
            }
            other => panic!("expected ElasticResult StructureInstance, got {:?}", other),
        }
    };

    let disp_hetero = get_field(&result_hetero, "displacement");
    let disp_homo   = get_field(&result_homo,   "displacement");

    // ── converged ────────────────────────────────────────────────────────────
    assert_eq!(get_field(&result_hetero, "converged"), Value::Bool(true), "hetero: converged");
    assert_eq!(get_field(&result_homo,   "converged"), Value::Bool(true), "homo: converged");

    // ── extract displacement data and compare max deflection ──────────────────
    let data_hetero = extract_displacement_data(&disp_hetero);
    let data_homo   = extract_displacement_data(&disp_homo);

    let defl_hetero = reify_eval::persistent_cache::max_deflection_magnitude(&data_hetero);
    let defl_homo   = reify_eval::persistent_cache::max_deflection_magnitude(&data_homo);

    assert!(
        defl_hetero.is_finite() && defl_hetero > 0.0,
        "heterogeneous max deflection must be finite and > 0, got {defl_hetero}"
    );
    assert!(
        defl_homo.is_finite() && defl_homo > 0.0,
        "homogeneous max deflection must be finite and > 0, got {defl_homo}"
    );

    // Softer infill → larger heterogeneous deflection (documented direction).
    // Assert directional compliance first: the two-zone beam must deflect MORE than the
    // all-stiff homogeneous baseline (Loewner: softer infill zone raises compliance).
    assert!(
        defl_hetero > defl_homo,
        "heterogeneous deflection ({defl_hetero:.6e}) must be LARGER than homogeneous \
         ({defl_homo:.6e}): softer infill increases compliance"
    );
    // Also require the difference to be non-trivial (> 0.1% relative) to catch
    // near-degenerate zone layouts where classification places nothing in the infill.
    let relative_diff = (defl_hetero - defl_homo) / defl_homo.max(1e-30);
    assert!(
        relative_diff > 1e-3,
        "heterogeneous deflection ({defl_hetero:.6e}) should exceed homogeneous \
         ({defl_homo:.6e}) by > 0.1% relative; got {relative_diff:.2e}"
    );
}

/// Six-element `value_inputs` — no `options` argument — exercises the
/// `value_inputs.get(6).unwrap_or(&Value::Undef)` fallback added in task ε/#4757.
///
/// The 6-param `.ri` overload `solve_elastic_static(material: Field<…>, …)` omits
/// `options: ElasticOptions`, so the trampoline receives only 6 elements.  Both
/// `extract_shell_route_params` and `extract_execution_params` must return stdlib
/// defaults for `Value::Undef`; the solve must complete without panic.
///
/// This is a **regression guard**: a revert to `value_inputs[6]` (index OOB → panic)
/// would be caught here before any field-path test in CI notices.
#[test]
fn six_element_value_inputs_field_overload_routes_to_defaults_and_completes() {
    const L: f64 = 0.8;
    const W: f64 = 0.1;
    const H: f64 = 0.1;
    const E_STIFF: f64 = 200e9;
    const E_SOFT: f64 = 40e9;

    let material = het_as_printed_field(
        [0.0, 0.0, 0.0], [L, W, H],
        [1.0, 0.0, 0.0],  // build_z = +x
        1.0, 0.04,
        1.0, 0.08,
        E_STIFF, E_SOFT,
    );

    // 6 elements — no options element at index 6 (mirrors the 6-param .ri overload).
    let value_inputs: &[Value] = &[
        material,
        make_length_scalar(L),
        make_length_scalar(W),
        make_length_scalar(H),
        make_point_load_list(1000.0),
        make_support_list(),
    ];
    let cancellation = CancellationHandle::new();
    let outcome = reify_eval::compute_targets::elastic_static::solve_elastic_static_trampoline(
        value_inputs, &[], &Value::Undef, None, &cancellation,
    );
    match outcome {
        ComputeOutcome::Completed { .. } => {}
        other => panic!(
            "6-element value_inputs (no options) expected Completed, got {:?}", other
        ),
    }
}
