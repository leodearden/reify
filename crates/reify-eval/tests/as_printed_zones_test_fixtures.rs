//! Shared `AsPrintedZones` Value-fixture builders for heterogeneous FEA tests.
//!
//! Previously, the four builder functions below were duplicated verbatim
//! (~150 lines each) between:
//! - `compute_targets/elastic_static.rs` `#[cfg(test)]` block (`het_*` names)
//! - `tests/solve_elastic_static_heterogeneous_e2e.rs` (`make_*` names)
//!
//! Both sites re-implement the inverse of the production `AsPrintedZones` lambda
//! layout emitted by `as_printed_material.rs`.  A layout change in that producer
//! previously required editing two independent copies; now there is one.
//!
//! ## Consumers
//!
//! **Integration tests** (`tests/*.rs`): include with `mod as_printed_zones_test_fixtures;`
//! at the top of the test file.
//!
//! **In-module unit tests** (`compute_targets/elastic_static.rs` `#[cfg(test)]`):
//! ```ignore
//! #[path = "../../tests/as_printed_zones_test_fixtures.rs"]
//! mod as_printed_zones_test_fixtures;
//! ```
//! The path resolves relative to `elastic_static.rs`'s directory
//! (`crates/reify-eval/src/compute_targets/`): two levels up then into `tests/`.

use std::sync::Arc;

use reify_core::{DimensionVector, ty::Type};
use reify_ir::{FieldSourceKind, PersistentMap, StructureInstanceData, StructureTypeId, Value};

/// Build a `Value::StructureInstance("OrthotropicMaterial")` with 9 fields.
///
/// All stiffness moduli (`e1/e2/e3`, `g12/g13/g23`) are `Value::Scalar` in the
/// PRESSURE dimension (Pa).  Poisson ratios (`nu12/nu13/nu23`) are `Value::Real`.
/// Shear moduli are derived as `E / (2·(1 + ν))` (isotropic alias).
pub fn het_ortho_law(e: f64, nu: f64) -> Value {
    let g = e / (2.0 * (1.0 + nu));
    let fields: PersistentMap<String, Value> = [
        ("e1".to_string(),  Value::Scalar { si_value: e, dimension: DimensionVector::PRESSURE }),
        ("e2".to_string(),  Value::Scalar { si_value: e, dimension: DimensionVector::PRESSURE }),
        ("e3".to_string(),  Value::Scalar { si_value: e, dimension: DimensionVector::PRESSURE }),
        ("g12".to_string(), Value::Scalar { si_value: g, dimension: DimensionVector::PRESSURE }),
        ("g13".to_string(), Value::Scalar { si_value: g, dimension: DimensionVector::PRESSURE }),
        ("g23".to_string(), Value::Scalar { si_value: g, dimension: DimensionVector::PRESSURE }),
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

/// Build a `MaterialFrame` StructureInstance whose z-axis is `build_z`
/// (the weak / build direction).  x and y are an orthonormal complement.
///
/// The `origin` field is set to the zero point.  Axis fields are
/// `Value::Vector` of three LENGTH-dimension `Value::Scalar` entries.
pub fn het_material_frame(build_z: [f64; 3]) -> Value {
    let mag = (build_z[0]*build_z[0] + build_z[1]*build_z[1] + build_z[2]*build_z[2]).sqrt();
    let z = [build_z[0]/mag, build_z[1]/mag, build_z[2]/mag];
    // Pick a reference vector not parallel to z:
    let ref_v = if z[0].abs() < 0.9 { [1.0_f64, 0.0, 0.0] } else { [0.0, 1.0, 0.0] };
    // x = cross(ref_v, z), normalised:
    let x = [
        ref_v[1]*z[2] - ref_v[2]*z[1],
        ref_v[2]*z[0] - ref_v[0]*z[2],
        ref_v[0]*z[1] - ref_v[1]*z[0],
    ];
    let xm = (x[0]*x[0] + x[1]*x[1] + x[2]*x[2]).sqrt();
    let x = [x[0]/xm, x[1]/xm, x[2]/xm];
    // y = cross(z, x):
    let y = [z[1]*x[2]-z[2]*x[1], z[2]*x[0]-z[0]*x[2], z[0]*x[1]-z[1]*x[0]];
    let len_scalar = |v: f64| Value::Scalar { si_value: v, dimension: DimensionVector::LENGTH };
    let vec3  = |v: [f64; 3]| Value::Vector(vec![len_scalar(v[0]), len_scalar(v[1]), len_scalar(v[2])]);
    let point3 = |v: [f64; 3]| Value::Point(vec![len_scalar(v[0]), len_scalar(v[1]), len_scalar(v[2])]);
    let frame_fields: PersistentMap<String, Value> = [
        ("origin".to_string(), point3([0.0; 3])),
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
///
/// Uses an isotropic-alias `OrthotropicMaterial` (`E` in all axes, `nu` Poisson
/// ratio) and a `MaterialFrame` whose z-axis is `build_z` (the weak print axis).
pub fn het_aniso_material(e: f64, nu: f64, build_z: [f64; 3]) -> Value {
    let law = het_ortho_law(e, nu);
    let frame = het_material_frame(build_z);
    let fields: PersistentMap<String, Value> = [
        ("law".to_string(), law),
        ("frame".to_string(), frame),
    ].into_iter().collect();
    Value::StructureInstance(Box::new(StructureInstanceData {
        type_id: StructureTypeId(u32::MAX),
        type_name: "AnisotropicMaterial".to_string(),
        version: 1,
        fields,
    }))
}

/// Build a `Value::Field { source: AsPrintedZones }` for the given AABB and zone params.
///
/// Lambda layout (mirrors `as_printed_material.rs`):
/// `[aabb_min, aabb_max, params, cos_threshold, mat_wall, mat_skin, mat_infill]`
///
/// where `params = [walls, top_bottom_layers, layer_height, line_width, bu_x, bu_y, bu_z]`.
///
/// `mat_wall` and `mat_skin` are set to `e_stiff`; `mat_infill` to `e_soft`.
/// `cos_threshold` is fixed at `0.7` (matches the production default).
#[allow(clippy::too_many_arguments)]
pub fn het_as_printed_field(
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
    let len_scalar = |v: f64| Value::Scalar { si_value: v, dimension: DimensionVector::LENGTH };
    let point3 = |v: [f64; 3]| Value::Point(vec![len_scalar(v[0]), len_scalar(v[1]), len_scalar(v[2])]);
    let mag = (build_z[0]*build_z[0] + build_z[1]*build_z[1] + build_z[2]*build_z[2]).sqrt();
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
    let mat_stiff = het_aniso_material(e_stiff, 0.3, build_z);
    let mat_soft  = het_aniso_material(e_soft,  0.3, build_z);
    let lambda = Value::List(vec![
        point3(aabb_min),
        point3(aabb_max),
        params,
        Value::Real(0.7),       // cos_threshold
        mat_stiff.clone(),      // mat_wall  = stiff
        mat_stiff,              // mat_skin  = stiff
        mat_soft,               // mat_infill = soft
    ]);
    Value::Field {
        domain_type: Type::point3(Type::length()),
        codomain_type: Type::StructureRef("AnisotropicMaterial".to_string()),
        source: FieldSourceKind::AsPrintedZones,
        lambda: Arc::new(lambda),
    }
}
