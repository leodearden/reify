//! RED tests for mode-shape data surfacing in the BucklingResult Value
//! (task ι/3458, PRD §13 step-1/2).
//!
//! Calls `solve_buckling_trampoline` directly with a tiny mesh geometry
//! (nz=1, n_nodes=162) to verify that after step-2:
//!
//! - `BucklingResult.base_node_positions` is a `Value::List<Real>` of
//!   length 3·n_nodes (flat xyz, all finite).
//! - `BucklingResult.modes[i].mode_shape` is a `Value::Map` with key
//!   `"displaced_positions"` → `Value::List<Real>` of length 3·n_nodes
//!   (all finite), NOT `Value::Undef`.
//!
//! # Tiny-mesh rationale
//!
//! Geometry: lx=ly=10mm, lz=1mm.
//! Trampoline formula: cross_elem_size = min(lx,ly)/(nx/2) = 0.01/4 = 0.0025 m.
//! nz = round(0.001/0.0025) = round(0.4) = 0 → max(1) = 1.
//! n_nodes = nx1·ny1·nz1 = 9·9·2 = 162.
//! DOFs = 3·162 = 486 — fast CG + eigensolve even in debug mode.
//!
//! # Step-1 RED status
//!
//! Both tests FAIL at step-1 because `mode_shape` is still `Value::Undef`
//! and `base_node_positions` does not yet exist in the BucklingResult.
//! GREEN after step-2 populates the displaced-position maps.

use reify_core::DimensionVector;
use reify_eval::{CancellationHandle, ComputeOutcome, RealizationReadHandle};
use reify_ir::{OpaqueState, PersistentMap, StructureInstanceData, StructureTypeId, Value};

// ── helpers ───────────────────────────────────────────────────────────────────

fn make_steel_material() -> Value {
    let fields: PersistentMap<String, Value> = [
        (
            "youngs_modulus".to_string(),
            Value::Scalar {
                si_value: 205.0e9,
                dimension: DimensionVector::PRESSURE,
            },
        ),
        ("poisson_ratio".to_string(), Value::Real(0.3)),
    ]
    .into_iter()
    .collect();
    Value::StructureInstance(Box::new(StructureInstanceData {
        type_id: StructureTypeId(u32::MAX),
        type_name: "ElasticMaterial".to_string(),
        version: 1,
        fields,
    }))
}

fn make_scalar_length(si_value: f64) -> Value {
    Value::Scalar {
        si_value,
        dimension: DimensionVector::LENGTH,
    }
}

fn make_buckling_options(n_modes: i64, tol: f64, max_iters: i64) -> Value {
    let fields: PersistentMap<String, Value> = [
        ("n_modes".to_string(), Value::Int(n_modes)),
        ("tol".to_string(), Value::Real(tol)),
        ("max_iters".to_string(), Value::Int(max_iters)),
    ]
    .into_iter()
    .collect();
    Value::StructureInstance(Box::new(StructureInstanceData {
        type_id: StructureTypeId(u32::MAX),
        type_name: "BucklingOptions".to_string(),
        version: 1,
        fields,
    }))
}

fn extract_field(result: &Value, field: &str) -> Option<Value> {
    match result {
        Value::StructureInstance(data) => data.fields.get(&field.to_string()).cloned(),
        Value::Map(m) => m.get(&Value::String(field.to_string())).cloned(),
        _ => None,
    }
}

/// Run the trampoline on a tiny mesh (lz=1mm, lx=ly=10mm → nz=1, n_nodes=162).
///
/// The empty loads list triggers the 1.0 N sentinel default in the trampoline.
fn run_tiny_trampoline(n_modes: i64) -> ComputeOutcome {
    let no_realization: &[RealizationReadHandle] = &[];
    let no_warm_state: Option<&OpaqueState> = None;

    let value_inputs = vec![
        make_steel_material(),
        make_scalar_length(0.001), // length (lz) = 1 mm  (compression axis)
        make_scalar_length(0.01),  // width  (lx) = 10 mm
        make_scalar_length(0.01),  // height (ly) = 10 mm
        Value::List(vec![]),       // loads  — empty → default 1.0 N sentinel
        Value::List(vec![]),       // supports — unused in task-ε/ι slice
        make_buckling_options(n_modes, 1e-4, 500),
    ];

    reify_eval::compute_targets::buckling::solve_buckling_trampoline(
        &value_inputs,
        no_realization,
        &Value::Undef,
        no_warm_state,
        &CancellationHandle::new(),
    )
}

// ── tests ─────────────────────────────────────────────────────────────────────

/// Expected n_nodes for the tiny geometry.
///
/// Derivation: nx1=9, ny1=9, nz1=2 → 9·9·2 = 162.
const TINY_N_NODES: usize = 9 * 9 * 2;

/// `BucklingResult.base_node_positions` must be a flat-xyz `Value::List<Real>`
/// of length 3·n_nodes with all-finite entries.
///
/// **RED at step-1**: the field does not yet exist in the BucklingResult.
/// **GREEN after step-2**.
#[test]
fn base_node_positions_is_a_flat_xyz_real_list() {
    let result = match run_tiny_trampoline(1) {
        ComputeOutcome::Completed { result, .. } => result,
        other => panic!("expected ComputeOutcome::Completed, got: {:?}", other),
    };

    let base = extract_field(&result, "base_node_positions").unwrap_or_else(|| {
        panic!(
            "BucklingResult must have a 'base_node_positions' field — \
             field is missing (step-2 not yet implemented)"
        )
    });

    let items = match base {
        Value::List(v) => v,
        other => panic!("base_node_positions must be Value::List, got: {:?}", other),
    };
    assert_eq!(
        items.len(),
        3 * TINY_N_NODES,
        "base_node_positions length must be 3·n_nodes = {}",
        3 * TINY_N_NODES
    );
    for (i, item) in items.iter().enumerate() {
        match item {
            Value::Real(r) => assert!(
                r.is_finite(),
                "base_node_positions[{i}] = {r} is not finite"
            ),
            other => panic!("base_node_positions[{i}] must be Value::Real, got: {other:?}"),
        }
    }
}

/// `BucklingResult.modes[i].mode_shape` must be a `Value::Map` containing
/// key `"displaced_positions"` → `Value::List<Real>` of length 3·n_nodes,
/// all-finite entries.  Must NOT be `Value::Undef`.
///
/// **RED at step-1**: `mode_shape` is still `Value::Undef`.
/// **GREEN after step-2**.
#[test]
fn mode_shape_is_a_displaced_positions_map() {
    let result = match run_tiny_trampoline(1) {
        ComputeOutcome::Completed { result, .. } => result,
        other => panic!("expected ComputeOutcome::Completed, got: {:?}", other),
    };

    let modes_val = extract_field(&result, "modes").expect("result.modes not found");
    let modes_list = match modes_val {
        Value::List(v) => v,
        other => panic!("result.modes must be Value::List, got: {:?}", other),
    };
    assert!(!modes_list.is_empty(), "modes list must be non-empty");

    for (i, mode_val) in modes_list.iter().enumerate() {
        let mode_data = match mode_val {
            Value::StructureInstance(d) => d,
            other => panic!("modes[{i}] must be StructureInstance, got: {other:?}"),
        };

        let mode_shape = mode_data
            .fields
            .get(&"mode_shape".to_string())
            .cloned()
            .unwrap_or_else(|| panic!("modes[{i}] has no 'mode_shape' field"));

        // This assertion fails at step-1: mode_shape is Value::Undef.
        let disp_val = match &mode_shape {
            Value::Map(m) => m
                .get(&Value::String("displaced_positions".to_string()))
                .cloned()
                .unwrap_or_else(|| {
                    panic!(
                        "modes[{i}].mode_shape Map is missing 'displaced_positions' key; \
                         present keys: {:?}",
                        m.keys().collect::<Vec<_>>()
                    )
                }),
            Value::Undef => panic!(
                "modes[{i}].mode_shape must NOT be Value::Undef; \
                 expected Value::Map with 'displaced_positions' key \
                 (step-2 not yet implemented)"
            ),
            other => panic!("modes[{i}].mode_shape must be Value::Map, got: {other:?}"),
        };

        let positions = match disp_val {
            Value::List(v) => v,
            other => panic!(
                "modes[{i}].mode_shape['displaced_positions'] must be Value::List, got: {other:?}"
            ),
        };
        assert_eq!(
            positions.len(),
            3 * TINY_N_NODES,
            "modes[{i}] displaced_positions length must be 3·n_nodes = {}",
            3 * TINY_N_NODES
        );
        for (k, pos) in positions.iter().enumerate() {
            match pos {
                Value::Real(r) => assert!(
                    r.is_finite(),
                    "modes[{i}].displaced_positions[{k}] = {r} is not finite"
                ),
                other => panic!(
                    "modes[{i}].displaced_positions[{k}] must be Value::Real, got: {other:?}"
                ),
            }
        }
    }
}
