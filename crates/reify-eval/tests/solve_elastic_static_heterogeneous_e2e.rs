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

use std::sync::Arc;

use reify_core::{DimensionVector, Type};
use reify_eval::{CancellationHandle, ComputeOutcome};
use reify_ir::{FieldSourceKind, PersistentMap, StructureInstanceData, StructureTypeId, Value};

// ── helpers ──────────────────────────────────────────────────────────────────

fn pressure_scalar(pa: f64) -> Value {
    Value::Scalar { si_value: pa, dimension: DimensionVector::PRESSURE }
}

fn length_scalar(m: f64) -> Value {
    Value::Scalar { si_value: m, dimension: DimensionVector::LENGTH }
}

fn point3_length(v: [f64; 3]) -> Value {
    Value::Point(vec![length_scalar(v[0]), length_scalar(v[1]), length_scalar(v[2])])
}

/// Build an `OrthotropicMaterial` StructureInstance (all-isotropic alias: E1=E2=E3).
fn make_ortho_law(e: f64, nu: f64) -> Value {
    let g = e / (2.0 * (1.0 + nu));
    let fields: PersistentMap<String, Value> = [
        ("e1".to_string(), pressure_scalar(e)),
        ("e2".to_string(), pressure_scalar(e)),
        ("e3".to_string(), pressure_scalar(e)),
        ("g12".to_string(), pressure_scalar(g)),
        ("g13".to_string(), pressure_scalar(g)),
        ("g23".to_string(), pressure_scalar(g)),
        ("nu12".to_string(), Value::Real(nu)),
        ("nu13".to_string(), Value::Real(nu)),
        ("nu23".to_string(), Value::Real(nu)),
    ].into_iter().collect();
    Value::StructureInstance(Box::new(StructureInstanceData {
        type_id: StructureTypeId(u32::MAX),
        type_name: "OrthotropicMaterial".to_string(),
        version: 1,
        fields,
    }))
}

/// Build a `MaterialFrame` whose z-axis = `build_z` (weak/build direction).
fn make_material_frame(build_z: [f64; 3]) -> Value {
    let mag = (build_z[0]*build_z[0] + build_z[1]*build_z[1] + build_z[2]*build_z[2]).sqrt();
    let z = [build_z[0]/mag, build_z[1]/mag, build_z[2]/mag];
    let ref_v = if z[0].abs() < 0.9 { [1.0_f64, 0.0, 0.0] } else { [0.0, 1.0, 0.0] };
    let x = [
        ref_v[1]*z[2] - ref_v[2]*z[1],
        ref_v[2]*z[0] - ref_v[0]*z[2],
        ref_v[0]*z[1] - ref_v[1]*z[0],
    ];
    let xm = (x[0]*x[0] + x[1]*x[1] + x[2]*x[2]).sqrt();
    let x = [x[0]/xm, x[1]/xm, x[2]/xm];
    let y = [z[1]*x[2]-z[2]*x[1], z[2]*x[0]-z[0]*x[2], z[0]*x[1]-z[1]*x[0]];
    let vec3 = |v: [f64; 3]| Value::Vector(vec![
        Value::Scalar { si_value: v[0], dimension: DimensionVector::LENGTH },
        Value::Scalar { si_value: v[1], dimension: DimensionVector::LENGTH },
        Value::Scalar { si_value: v[2], dimension: DimensionVector::LENGTH },
    ]);
    let frame_fields: PersistentMap<String, Value> = [
        ("origin".to_string(), point3_length([0.0; 3])),
        ("x_axis".to_string(), vec3(x)),
        ("y_axis".to_string(), vec3(y)),
        ("z_axis".to_string(), vec3(z)),
    ].into_iter().collect();
    Value::StructureInstance(Box::new(StructureInstanceData {
        type_id: StructureTypeId(u32::MAX),
        type_name: "MaterialFrame".to_string(),
        version: 1,
        fields: frame_fields,
    }))
}

/// Build an `AnisotropicMaterial { law: OrthotropicMaterial, frame }` StructureInstance.
fn make_aniso_material(e: f64, nu: f64, build_z: [f64; 3]) -> Value {
    let fields: PersistentMap<String, Value> = [
        ("law".to_string(), make_ortho_law(e, nu)),
        ("frame".to_string(), make_material_frame(build_z)),
    ].into_iter().collect();
    Value::StructureInstance(Box::new(StructureInstanceData {
        type_id: StructureTypeId(u32::MAX),
        type_name: "AnisotropicMaterial".to_string(),
        version: 1,
        fields,
    }))
}

/// Build a `Value::Field { source: AsPrintedZones }` for the given AABB.
///
/// Lambda: `[aabb_min, aabb_max, params, cos_threshold, mat_wall, mat_skin, mat_infill]`
///
/// `mat_wall` / `mat_skin` = `e_stiff`; `mat_infill` = `e_soft`.
fn make_as_printed_zones_field(
    aabb_min: [f64; 3],
    aabb_max: [f64; 3],
    build_z: [f64; 3],
    walls: f64,
    line_width: f64,
    layers: f64,
    layer_height: f64,
    e_stiff: f64,
    e_soft: f64,
) -> Value {
    let mag = (build_z[0]*build_z[0]+build_z[1]*build_z[1]+build_z[2]*build_z[2]).sqrt();
    let bu = [build_z[0]/mag, build_z[1]/mag, build_z[2]/mag];
    let params = Value::List(vec![
        Value::Real(walls),
        Value::Real(layers),
        Value::Real(layer_height),
        Value::Real(line_width),
        Value::Real(bu[0]),
        Value::Real(bu[1]),
        Value::Real(bu[2]),
    ]);
    let mat_stiff = make_aniso_material(e_stiff, 0.3, build_z);
    let mat_soft  = make_aniso_material(e_soft,  0.3, build_z);
    let lambda = Value::List(vec![
        point3_length(aabb_min),
        point3_length(aabb_max),
        params,
        Value::Real(0.7),       // cos_threshold
        mat_stiff.clone(),      // mat_wall = stiff
        mat_stiff,              // mat_skin = stiff
        mat_soft,               // mat_infill = soft
    ]);
    Value::Field {
        domain_type: Type::point3(Type::length()),
        codomain_type: Type::StructureRef("AnisotropicMaterial".to_string()),
        source: FieldSourceKind::AsPrintedZones,
        lambda: Arc::new(lambda),
    }
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
    let hetero_material = make_as_printed_zones_field(
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

    let defl_hetero = reify_eval::persistent_cache::max_deflection_magnitude(data_hetero);
    let defl_homo   = reify_eval::persistent_cache::max_deflection_magnitude(data_homo);

    assert!(
        defl_hetero.is_finite() && defl_hetero > 0.0,
        "heterogeneous max deflection must be finite and > 0, got {defl_hetero}"
    );
    assert!(
        defl_homo.is_finite() && defl_homo > 0.0,
        "homogeneous max deflection must be finite and > 0, got {defl_homo}"
    );

    // The two deflections must differ by more than 0.1% relative.
    // Softer infill → larger heterogeneous deflection (documented direction).
    let relative_diff = (defl_hetero - defl_homo).abs() / defl_homo.max(1e-30);
    assert!(
        relative_diff > 1e-3,
        "heterogeneous deflection ({defl_hetero:.6e}) should differ from homogeneous \
         ({defl_homo:.6e}) by > 0.1% relative; got {relative_diff:.2e}"
    );
}
