// SPDX-License-Identifier: AGPL-3.0-or-later

//! Trampoline for `fdm::as_printed_material_r0` — the **R0 fidelity rung** of
//! the FDM as-printed material field (task θ / 3790), the closed-form-physics
//! sibling of the δ R-fast rung ([`super::as_printed_material`]).
//!
//! # Why an R0 rung
//!
//! Where R-fast derives per-zone effective properties from the stdlib
//! `FDMProcess` alone (Gibson-Ashby infill knockdown + a fixed 0.67 build-Z
//! ratio) and reads the body AABB from the realization mesh, R0 maps a **real
//! sliced toolpath** (PrusaSlicer G-code, parsed via
//! [`reify_fdm::parse_prusaslicer_gcode`]) to per-zone *orthotropic* constants
//! using closed-form physics ([`reify_fdm::r0_region_materials`]): Rodríguez
//! 2003 orthotropy + Halpin-Tsai fibre (inert by default) + a lumped-cooling
//! build-Z knockdown. The field AABB and line width come from the toolpath bead
//! geometry — this crate owns the mm→SI conversion.
//!
//! # Data flow
//!
//! `value_inputs = [gcode: String, FDMProcess, AsPrintedOptions]`. **No**
//! realization handles are consumed — the geometry comes from the toolpath. The
//! trampoline:
//!   1. parses the G-code string into a [`reify_fdm::Toolpath`] (a parse error
//!      degrades honestly to an `Undef`-lambda field),
//!   2. reads the FDMProcess (build_direction / infill_density / walls /
//!      top_bottom_layers / layer_height / base material),
//!   3. runs [`reify_fdm::r0_region_materials`] for per-zone
//!      [`reify_fdm::R0Region`] orthotropic constants + dominant bead directions,
//!   4. derives the field AABB + line width from the toolpath bead centerlines
//!      (mm→SI),
//!   5. builds three `AnisotropicMaterial { law: OrthotropicMaterial, frame }`
//!      values whose frame z-axis is the build direction (build-Z weakest) and
//!      x-axis is the zone's dominant bead direction, and
//!   6. packs them as the same 7-element `Value::Field { source: AsPrintedZones }`
//!      payload R-fast emits — so the existing `reify_expr::sample_as_printed_zones`
//!      sampler serves the R0 field with zero reify-ir / reify-expr change (only
//!      the per-zone constants + frames differ from R-fast).
//!
//! Registered as `fdm::as_printed_material_r0` (the R0 rung) alongside the
//! R-fast `fdm::as_printed_material_r_fast` — both coexist; the graph-level
//! progressive selection between them is the integration gate ι's concern, not
//! θ's.

use reify_fdm::{
    DEFAULT_TOP_BOTTOM_NORMAL_THRESHOLD, R0Options, R0Region, Toolpath, parse_prusaslicer_gcode,
    r0_region_materials,
};
use reify_ir::{OpaqueState, Value};

use super::as_printed_material::{
    as_printed_field_value, field_int, field_real, field_scalar, field_vec3, orthotropic_material,
    read_base_elastic, struct_data, structure,
};
use crate::{CancellationHandle, ComputeOutcome, RealizationReadHandle};

/// Native G-code millimetres → SI metres (θ owns the mm→SI conversion).
const MM_TO_M: f64 = 1.0e-3;

/// `@optimized("fdm::as_printed_material_r0")` ComputeNode trampoline.
///
/// Always returns [`ComputeOutcome::Completed`] with a `Value::Field { source:
/// AsPrintedZones }`. Malformed G-code or a missing / malformed FDMProcess
/// degrades honestly to a `Value::Undef`-lambda field (every sample returns
/// `Undef`) rather than panicking — mirroring the R-fast `degraded_field`.
pub fn as_printed_material_r0_trampoline(
    value_inputs: &[Value],
    _realization_inputs: &[RealizationReadHandle],
    _options: &Value,
    _prior_warm_state: Option<&OpaqueState>,
    _cancellation: &CancellationHandle,
) -> ComputeOutcome {
    let field = build_r0_field(value_inputs).unwrap_or_else(degraded_field);
    ComputeOutcome::Completed {
        result: field,
        new_warm_state: None,
        cost_per_byte: None,
        diagnostics: Vec::new(),
    }
}

/// Build the R0 `AsPrintedZones` material field, or `None` if any required input
/// is missing / malformed (caller degrades to [`degraded_field`]).
fn build_r0_field(value_inputs: &[Value]) -> Option<Value> {
    // value_inputs = [gcode String, FDMProcess, AsPrintedOptions].
    let gcode = match value_inputs.first()? {
        Value::String(s) => s,
        _ => return None,
    };
    // A hard toolpath parse error degrades to the Undef-lambda field.
    let toolpath = parse_prusaslicer_gcode(gcode).ok()?;
    let process = struct_data(value_inputs.get(1)?)?;

    // ── FDMProcess → BaseElastic + R0Options ───────────────────────────────────
    let walls = field_int(process, "walls")?.max(0) as u32;
    let top_bottom_layers = field_int(process, "top_bottom_layers")?.max(0) as u32;
    let layer_height = field_scalar(process, "layer_height")?;
    let build_direction = field_vec3(process, "build_direction")?;
    let infill_density = field_real(process, "infill_density")?;
    let base = read_base_elastic(process)?;

    // The build axis is the frame z-axis / weakest axis; degenerate → +Z.
    let build_unit = unit3(build_direction).unwrap_or([0.0, 0.0, 1.0]);

    let opts = R0Options {
        // Guard the β/R0 domain (ρ ∈ (0, 1]); the inert default fibre keeps the
        // Halpin-Tsai factor at exactly 1.0 (no fibre fields in FDMProcess yet).
        infill_density: infill_density.clamp(f64::MIN_POSITIVE, 1.0),
        fibre: None,
        build_direction: build_unit,
        ..R0Options::default()
    };
    let regions = r0_region_materials(&toolpath, base, &opts);

    // ── Toolpath geometry → field AABB + line_width (mm→SI) ─────────────────────
    let (aabb_min, aabb_max) = toolpath_aabb(&toolpath)?;
    // line_width derives from the measured wall bead width (already SI metres).
    let line_width = regions.wall.mean_width_m;

    // ── Per-zone orthotropic AnisotropicMaterial values (bead-direction frame) ──
    let mat_wall = r0_zone_material(&regions.wall, build_unit);
    let mat_skin = r0_zone_material(&regions.skin, build_unit);
    let mat_infill = r0_zone_material(&regions.infill, build_unit);

    // ── Pack the AsPrintedZones lambda (same 7-element contract as R-fast) ───────
    // params = [walls, top_bottom_layers, layer_height, line_width, bx, by, bz].
    let params_list = Value::List(vec![
        Value::Real(walls as f64),
        Value::Real(top_bottom_layers as f64),
        Value::Real(layer_height),
        Value::Real(line_width),
        Value::Real(build_unit[0]),
        Value::Real(build_unit[1]),
        Value::Real(build_unit[2]),
    ]);
    let lambda = Value::List(vec![
        super::point3_length(aabb_min),
        super::point3_length(aabb_max),
        params_list,
        Value::Real(DEFAULT_TOP_BOTTOM_NORMAL_THRESHOLD),
        mat_wall,
        mat_skin,
        mat_infill,
    ]);
    Some(as_printed_field_value(lambda))
}

/// Honest-degradation field: a well-typed `AsPrintedZones` field whose lambda is
/// `Undef`, so every `sample_field_at` falls through to `Undef` (mirrors the
/// R-fast `degraded_field`).
fn degraded_field() -> Value {
    as_printed_field_value(Value::Undef)
}

/// Build one zone's `AnisotropicMaterial { law: OrthotropicMaterial, frame }`
/// from its R0 constants + a frame whose z-axis is the build direction and
/// x-axis is the zone's dominant bead direction.
fn r0_zone_material(region: &R0Region, build_unit: [f64; 3]) -> Value {
    let frame = r0_material_frame(build_unit, region.bead_direction);
    orthotropic_material(region.constants, &frame)
}

/// The R0 material frame (Contract C3): z-axis = build direction (build-Z
/// weakest), x-axis = the zone's dominant bead-centerline direction projected
/// into the plane ⊥ z, y = z × x. A bead direction parallel to z (degenerate)
/// falls back to an arbitrary in-plane complement.
fn r0_material_frame(build_z: [f64; 3], bead_dir: [f64; 3]) -> Value {
    let z = unit3(build_z).unwrap_or([0.0, 0.0, 1.0]);
    // Project the bead direction into the plane perpendicular to z.
    let d = bead_dir[0] * z[0] + bead_dir[1] * z[1] + bead_dir[2] * z[2];
    let x_perp = [
        bead_dir[0] - d * z[0],
        bead_dir[1] - d * z[1],
        bead_dir[2] - d * z[2],
    ];
    let x = unit3(x_perp).unwrap_or_else(|| complement_x(z));
    let y = cross(z, x);
    structure(
        "MaterialFrame",
        vec![
            ("origin", super::point3_length([0.0, 0.0, 0.0])),
            ("x_axis", super::vec3_length(x)),
            ("y_axis", super::vec3_length(y)),
            ("z_axis", super::vec3_length(z)),
        ],
    )
}

/// The toolpath bead-centerline AABB in SI metres (native mm × 1e-3), or `None`
/// if the toolpath has no beads (no geometry to define the field domain — the
/// caller then degrades honestly).
fn toolpath_aabb(toolpath: &Toolpath) -> Option<([f64; 3], [f64; 3])> {
    let mut min = [f64::INFINITY; 3];
    let mut max = [f64::NEG_INFINITY; 3];
    let mut any = false;
    for bead in &toolpath.beads {
        for p in &bead.centerline {
            any = true;
            for k in 0..3 {
                if p[k] < min[k] {
                    min[k] = p[k];
                }
                if p[k] > max[k] {
                    max[k] = p[k];
                }
            }
        }
    }
    any.then(|| {
        (
            [min[0] * MM_TO_M, min[1] * MM_TO_M, min[2] * MM_TO_M],
            [max[0] * MM_TO_M, max[1] * MM_TO_M, max[2] * MM_TO_M],
        )
    })
}

// ── small geometry helpers (frame math) ─────────────────────────────────────

/// Normalize a 3-vector to unit length; `None` if degenerate / non-finite.
fn unit3(v: [f64; 3]) -> Option<[f64; 3]> {
    let mag = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    (mag.is_finite() && mag > 1e-12).then(|| [v[0] / mag, v[1] / mag, v[2] / mag])
}

fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

/// An in-plane unit x-axis for the degenerate case (bead direction parallel to
/// z): cross a reference axis not parallel to z with z.
fn complement_x(z: [f64; 3]) -> [f64; 3] {
    let reference = if z[0].abs() < 0.9 {
        [1.0, 0.0, 0.0]
    } else {
        [0.0, 1.0, 0.0]
    };
    unit3(cross(reference, z)).unwrap_or([1.0, 0.0, 0.0])
}
