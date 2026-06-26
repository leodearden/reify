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

/// Isotropic material with the given Young's modulus (Pa), fixed ν = 0.35,
/// ρ = 1040 kg/m³.  For isotropic elasticity K(αE) = α·K(E) exactly
/// (D-matrix is linear in E for fixed ν), so ONLY youngs_modulus varies
/// between operators when using this fixture — making the cold-start heuristic
/// achievability basis (‖K(αE)·u_E − f‖ = |α−1|·‖f‖) exact.
#[allow(dead_code)]
pub fn isotropic_material(youngs_pa: f64) -> Value {
    structure(
        "ABS_Plastic",
        vec![
            ("youngs_modulus", scalar(youngs_pa, DimensionVector::PRESSURE)),
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

/// A multi-role PrusaSlicer G-code snippet for the R0 (θ / 3790) trampoline:
/// two layers (Z 0.2 / 0.4 mm), each depositing one `External perimeter`
/// (→ wall), one `Top solid infill` (→ skin), and one `Internal infill`
/// (→ infill) bead. Every bead runs along **+X** (0→40 mm) at width 0.45 mm,
/// so the dominant bead direction is `[1, 0, 0]` — distinct from the R-fast
/// frame's arbitrary build-Z complement (`[0, -1, 0]` for a +Z build axis).
///
/// The bead centerlines span X∈[0,40], Y∈[0,20], Z∈[0.2,0.4] mm, so the
/// toolpath-derived AABB (mm→SI) is `[0,0,0.0002] … [0.040,0.020,0.0004]`.
#[allow(dead_code)]
pub fn r0_toolpath_gcode() -> &'static str {
    "\
M83
M104 S210
;LAYER_CHANGE
;Z:0.2
;HEIGHT:0.2
G1 Z0.2 F7200
;TYPE:External perimeter
;WIDTH:0.45
G1 X0 Y0 F9000
G1 X40 Y0 E2.0
;TYPE:Top solid infill
;WIDTH:0.45
G1 X0 Y10 F9000
G1 X40 Y10 E2.0
;TYPE:Internal infill
;WIDTH:0.45
G1 X0 Y20 F9000
G1 X40 Y20 E2.0
;LAYER_CHANGE
;Z:0.4
;HEIGHT:0.2
G1 Z0.4 F7200
;TYPE:External perimeter
;WIDTH:0.45
G1 X0 Y0 F9000
G1 X40 Y0 E2.0
;TYPE:Top solid infill
;WIDTH:0.45
G1 X0 Y10 F9000
G1 X40 Y10 E2.0
;TYPE:Internal infill
;WIDTH:0.45
G1 X0 Y20 F9000
G1 X40 Y20 E2.0
"
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

// ── FEA cantilever fixture constants ─────────────────────────────────────────
//
// Cantilever dimensions: 0.8 × 0.1 × 0.1 m.  Matches the dimensions used by
// `solve_elastic_static_heterogeneous_e2e.rs` so the warm-start tests can
// drive the exact same mesh (FEA mesh is derived from the (L,W,H) scalars
// inside the trampoline — fixed across field swaps, which is the warm-start
// precondition).

/// Cantilever length (SI metres).
#[allow(dead_code)]
pub const FEA_L: f64 = 0.8;

/// Cantilever width (SI metres).
#[allow(dead_code)]
pub const FEA_W: f64 = 0.1;

/// Cantilever height (SI metres).
#[allow(dead_code)]
pub const FEA_H: f64 = 0.1;

/// `ElasticOptions` StructureInstance with the `deterministic` flag set as
/// requested.  The `threads` field is absent (→ stdlib default: host CPU count).
///
/// For bit-stability tests pass `deterministic: true`; for warm-start tests
/// pass `deterministic: false` (the default).
#[allow(dead_code)]
pub fn elastic_options(deterministic: bool) -> Value {
    let fields: PersistentMap<String, Value> = [("deterministic".to_string(), Value::Bool(deterministic))]
        .into_iter()
        .collect();
    Value::StructureInstance(Box::new(StructureInstanceData {
        type_id: REGISTRY_FREE,
        type_name: "ElasticOptions".to_string(),
        version: 1,
        fields,
    }))
}

/// A single `PointLoad { force: Real(force_n) }` inside a `Value::List`, as
/// expected by `solve_elastic_static_trampoline`'s `value_inputs[4]`.
#[allow(dead_code)]
pub fn point_load_list(force_n: f64) -> Value {
    let fields: PersistentMap<String, Value> =
        [("force".to_string(), Value::Real(force_n))].into_iter().collect();
    Value::List(vec![Value::StructureInstance(Box::new(StructureInstanceData {
        type_id: REGISTRY_FREE,
        type_name: "PointLoad".to_string(),
        version: 1,
        fields,
    }))])
}

/// A single `FixedSupport {}` inside a `Value::List`, as expected by
/// `solve_elastic_static_trampoline`'s `value_inputs[5]`.
#[allow(dead_code)]
pub fn support_list() -> Value {
    Value::List(vec![Value::StructureInstance(Box::new(StructureInstanceData {
        type_id: REGISTRY_FREE,
        type_name: "FixedSupport".to_string(),
        version: 1,
        fields: [].into_iter().collect(),
    }))])
}
