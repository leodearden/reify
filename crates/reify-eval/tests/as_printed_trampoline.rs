// SPDX-License-Identifier: AGPL-3.0-or-later

//! Trampoline-level integration test for `fdm::as_printed_material_r_fast`
//! (task δ / 3786, step-7 RED → step-8 GREEN).
//!
//! Constructs the trampoline inputs by hand — a [`RealizationReadHandle`]
//! wrapping a small box surface mesh plus
//! `value_inputs = [body, FDMProcess, AsPrintedOptions]` — invokes the δ
//! trampoline directly, and asserts the produced
//! `Value::Field { source: AsPrintedZones }` carries:
//!   * codomain `AnisotropicMaterial`,
//!   * the body's AABB (derived from the realization mesh vertices), and
//!   * DISTINCT wall (dense) vs infill (Gibson-Ashby knocked-down) materials,
//!     with the build (Z / axial) axis the weakest in both.
//!
//! Fails until step-8 adds `as_printed_material_r_fast_trampoline` (the symbol
//! is absent, so the test target does not compile — the RED state).

use std::sync::Arc;

use reify_core::{ContentHash, DimensionVector, RealizationNodeId, Type};
use reify_eval::compute_targets::as_printed_material::as_printed_material_r_fast_trampoline;
use reify_eval::{CancellationHandle, ComputeOutcome, RealizationReadHandle, RealizedContent};
use reify_ir::{FieldSourceKind, Mesh, PersistentMap, StructureInstanceData, StructureTypeId, Value};

/// Registry-free placeholder type id for Rust-constructed StructureInstances
/// (mirrors `reify_eval::dynamics_ops::REGISTRY_FREE_TYPE_ID`).
const REGISTRY_FREE: StructureTypeId = StructureTypeId(u32::MAX);

// 40×40×10 mm box (SI metres); Z is the build axis.
const BOX_MIN: [f64; 3] = [0.0, 0.0, 0.0];
const BOX_MAX: [f64; 3] = [0.040, 0.040, 0.010];

fn structure(type_name: &str, fields: Vec<(&str, Value)>) -> Value {
    let fields: PersistentMap<String, Value> =
        fields.into_iter().map(|(k, v)| (k.to_string(), v)).collect();
    Value::StructureInstance(Box::new(StructureInstanceData {
        type_id: REGISTRY_FREE,
        type_name: type_name.to_string(),
        version: 1,
        fields,
    }))
}

fn scalar(si: f64, dim: DimensionVector) -> Value {
    Value::Scalar {
        si_value: si,
        dimension: dim,
    }
}

fn length(m: f64) -> Value {
    scalar(m, DimensionVector::LENGTH)
}

/// `vec3(x,y,z)` as a `Vector3<Length>` — mirrors the FDMProcess
/// `build_direction` representation produced by the stdlib evaluator.
fn vec3_length(v: [f64; 3]) -> Value {
    Value::Vector(vec![length(v[0]), length(v[1]), length(v[2])])
}

/// A box surface mesh covering `[BOX_MIN, BOX_MAX]`. Only the vertex extent
/// matters to the trampoline (it derives the AABB from the vertices); the
/// triangle indices are irrelevant here, so we ship just the 8 corners.
fn box_mesh() -> Mesh {
    let [x0, y0, z0] = BOX_MIN;
    let [x1, y1, z1] = BOX_MAX;
    let corners = [
        [x0, y0, z0],
        [x1, y0, z0],
        [x1, y1, z0],
        [x0, y1, z0],
        [x0, y0, z1],
        [x1, y0, z1],
        [x1, y1, z1],
        [x0, y1, z1],
    ];
    let vertices: Vec<f32> = corners
        .iter()
        .flat_map(|c| c.iter().map(|&x| x as f32))
        .collect();
    Mesh {
        vertices,
        indices: vec![],
        normals: None,
    }
}

/// An ABS-like base filament (E ≈ 2.0 GPa, ν = 0.35, ρ ≈ 1040 kg/m³).
fn abs_like_material() -> Value {
    structure(
        "ABS_Plastic",
        vec![
            ("youngs_modulus", scalar(2.0e9, DimensionVector::PRESSURE)),
            ("poisson_ratio", Value::Real(0.35)),
            ("density", scalar(1040.0, DimensionVector::MASS_DENSITY)),
        ],
    )
}

/// Default (all-`none`) coupon — no measured-property overrides.
fn coupon_default() -> Value {
    structure(
        "FDMCouponOverride",
        vec![
            ("ex", Value::Option(None)),
            ("ey", Value::Option(None)),
            ("ez", Value::Option(None)),
            ("gxy", Value::Option(None)),
            ("infill_gibson_ashby_c", Value::Option(None)),
            ("infill_gibson_ashby_n", Value::Option(None)),
        ],
    )
}

/// A walled+infilled FDM process: 3 walls, 4 top/bottom layers, 0.2mm layers,
/// 20% gyroid infill, Z build axis, ABS-like base material.
fn fdm_process() -> Value {
    structure(
        "FDMProcess",
        vec![
            ("build_direction", vec3_length([0.0, 0.0, 0.001])),
            ("layer_height", length(0.0002)),
            ("walls", Value::Int(3)),
            ("top_bottom_layers", Value::Int(4)),
            ("infill_density", Value::Real(0.2)),
            (
                "infill_pattern",
                Value::Enum {
                    type_name: "InfillPattern".to_string(),
                    variant: "Gyroid".to_string(),
                },
            ),
            ("material", abs_like_material()),
        ],
    )
}

/// Default consumer options: 0.4mm line width, no coupon, transverse-isotropic.
fn as_printed_options() -> Value {
    structure(
        "AsPrintedOptions",
        vec![
            ("line_width", length(0.0004)),
            ("coupon", coupon_default()),
            ("orthotropic", Value::Bool(false)),
            ("target_fidelity", Value::Int(0)),
        ],
    )
}

fn body_handle() -> RealizationReadHandle {
    RealizationReadHandle::new(
        RealizationNodeId::new("body", 0),
        ContentHash(1),
        Some(RealizedContent::SurfaceMesh(Arc::new(box_mesh()))),
    )
}

/// Read a `law` constant (e.g. `e_in_plane`) from an `AnisotropicMaterial`
/// `Value` — its nested `law` StructureInstance's dimensioned scalar field.
fn law_constant(aniso: &Value, key: &str) -> f64 {
    let Value::StructureInstance(data) = aniso else {
        panic!("expected AnisotropicMaterial StructureInstance, got {aniso:?}");
    };
    assert_eq!(
        data.type_name, "AnisotropicMaterial",
        "zone material must be an AnisotropicMaterial"
    );
    let law = data.fields.get("law").expect("AnisotropicMaterial.law");
    let Value::StructureInstance(law) = law else {
        panic!("expected law StructureInstance, got {law:?}");
    };
    match law.fields.get(key) {
        Some(Value::Scalar { si_value, .. }) => *si_value,
        other => panic!("expected law.{key} to be a Scalar, got {other:?}"),
    }
}

fn point_components(v: &Value) -> [f64; 3] {
    match v {
        Value::Point(c) | Value::Vector(c) if c.len() == 3 => [
            c[0].as_f64().unwrap(),
            c[1].as_f64().unwrap(),
            c[2].as_f64().unwrap(),
        ],
        other => panic!("expected 3-component Point/Vector, got {other:?}"),
    }
}

#[test]
fn as_printed_trampoline_produces_as_printed_zones_field_with_distinct_zone_materials() {
    let value_inputs = [Value::Undef, fdm_process(), as_printed_options()];
    let outcome = as_printed_material_r_fast_trampoline(
        &value_inputs,
        &[body_handle()],
        &Value::Undef,
        None,
        &CancellationHandle::new(),
    );

    let result = match outcome {
        ComputeOutcome::Completed { result, .. } => result,
        other => panic!("expected ComputeOutcome::Completed, got {other:?}"),
    };

    // (1) The result is a Field<_, AnisotropicMaterial> sourced AsPrintedZones.
    let (codomain_type, source, lambda) = match &result {
        Value::Field {
            codomain_type,
            source,
            lambda,
            ..
        } => (codomain_type, source, lambda.clone()),
        other => panic!("expected Value::Field, got {other:?}"),
    };
    assert!(
        matches!(source, FieldSourceKind::AsPrintedZones),
        "field source must be AsPrintedZones, got {source:?}"
    );
    assert_eq!(
        *codomain_type,
        Type::StructureRef("AnisotropicMaterial".to_string()),
        "field codomain must be AnisotropicMaterial"
    );

    // (2) The lambda slot is the 7-element zone payload.
    let items = match lambda.as_ref() {
        Value::List(items) => items,
        other => panic!("expected lambda Value::List, got {other:?}"),
    };
    assert_eq!(
        items.len(),
        7,
        "AsPrintedZones lambda must be a 7-element list"
    );

    // (3) The stored AABB matches the box extents (derived from the mesh).
    let min = point_components(&items[0]);
    let max = point_components(&items[1]);
    for k in 0..3 {
        assert!(
            (min[k] - BOX_MIN[k]).abs() < 1e-9,
            "aabb_min[{k}] {} != {}",
            min[k],
            BOX_MIN[k]
        );
        assert!(
            (max[k] - BOX_MAX[k]).abs() < 1e-9,
            "aabb_max[{k}] {} != {}",
            max[k],
            BOX_MAX[k]
        );
    }

    // (4) Wall (dense) vs infill (Gibson-Ashby knocked-down) materials are
    // DISTINCT — the non-constant-field signal — and the build (Z / axial) axis
    // is the weakest in BOTH (PRD C4 invariant).
    let mat_wall = &items[4]; // Zone::Wall slot
    let mat_infill = &items[6]; // Zone::Infill slot

    let wall_e = law_constant(mat_wall, "e_in_plane");
    let infill_e = law_constant(mat_infill, "e_in_plane");
    assert!(
        wall_e > infill_e * 2.0,
        "wall in-plane modulus {wall_e} must greatly exceed infill {infill_e}"
    );

    for (name, mat) in [("wall", mat_wall), ("infill", mat_infill)] {
        let e_in_plane = law_constant(mat, "e_in_plane");
        let e_axial = law_constant(mat, "e_axial");
        assert!(
            e_axial < e_in_plane,
            "{name}: build-Z (axial {e_axial}) must be weaker than in-plane ({e_in_plane})"
        );
    }
}
