// SPDX-License-Identifier: AGPL-3.0-or-later

//! Shared FDM as-printed fixture builders for `reify-eval` integration tests.
//!
//! Each public item carries `#[allow(dead_code)]` because this module recompiles
//! into every `mod common;` test binary; binaries that don't use any as-printed
//! helper would otherwise trip the `dead_code` lint (mirrors `alloc_counter.rs`).

use reify_core::DimensionVector;
use reify_ir::{Mesh, PersistentMap, StructureInstanceData, StructureTypeId, Value};

/// Registry-free placeholder type id for Rust-constructed StructureInstances
/// (mirrors `reify_eval::dynamics_ops::REGISTRY_FREE_TYPE_ID`).
#[allow(dead_code)]
pub const REGISTRY_FREE: StructureTypeId = StructureTypeId(u32::MAX);

// 40×40×10 mm box (SI metres); Z is the build axis.
#[allow(dead_code)]
pub const BOX_MIN: [f64; 3] = [0.0, 0.0, 0.0];
#[allow(dead_code)]
pub const BOX_MAX: [f64; 3] = [0.040, 0.040, 0.010];

#[allow(dead_code)]
pub fn structure(type_name: &str, fields: Vec<(&str, Value)>) -> Value {
    let fields: PersistentMap<String, Value> =
        fields.into_iter().map(|(k, v)| (k.to_string(), v)).collect();
    Value::StructureInstance(Box::new(StructureInstanceData {
        type_id: REGISTRY_FREE,
        type_name: type_name.to_string(),
        version: 1,
        fields,
    }))
}

#[allow(dead_code)]
pub fn scalar(si: f64, dim: DimensionVector) -> Value {
    Value::Scalar {
        si_value: si,
        dimension: dim,
    }
}

#[allow(dead_code)]
pub fn length(m: f64) -> Value {
    scalar(m, DimensionVector::LENGTH)
}

/// `vec3(x,y,z)` as a `Vector3<Length>` — mirrors the FDMProcess
/// `build_direction` representation produced by the stdlib evaluator.
#[allow(dead_code)]
pub fn vec3_length(v: [f64; 3]) -> Value {
    Value::Vector(vec![length(v[0]), length(v[1]), length(v[2])])
}

/// A box surface mesh covering `[BOX_MIN, BOX_MAX]`. Only the vertex extent
/// matters to the trampoline (it derives the AABB from the vertices); the
/// triangle indices are irrelevant here, so we ship just the 8 corners.
#[allow(dead_code)]
pub fn box_mesh() -> Mesh {
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
#[allow(dead_code)]
pub fn abs_like_material() -> Value {
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
#[allow(dead_code)]
pub fn coupon_default() -> Value {
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
#[allow(dead_code)]
pub fn fdm_process() -> Value {
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
#[allow(dead_code)]
pub fn as_printed_options() -> Value {
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
