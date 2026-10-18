// SPDX-License-Identifier: AGPL-3.0-or-later

//! Trampoline for `fdm::as_printed_material_r_fast` — the R-fast (bottom-rung)
//! producer of a heterogeneous `Field<Point3<Length>, AnisotropicMaterial>`
//! for an FDM-printed body (task δ / 3786).
//!
//! # Why a ComputeNode and not a plain builtin
//!
//! Computing the wall / skin / infill zones needs the body's geometry — its
//! axis-aligned bounding box. Only compute-node trampolines receive
//! [`RealizationReadHandle`]s, so the AABB (from the realization mesh vertices)
//! is reachable only here. R-fast uses the AABB faces as the γ distance-probe
//! surface — exact for box bodies (the δ user-observable signal), an R-fast
//! approximation for general bodies (face-partitioned OCCT probes are a
//! higher-rung concern).
//!
//! # Data flow
//!
//! `value_inputs = [body, FDMProcess, AsPrintedOptions]` (the stdlib fn arg
//! order). The trampoline:
//!   1. derives the body AABB from `realization_inputs[0]`'s mesh vertices,
//!   2. reads the FDMProcess (walls / top_bottom_layers / layer_height /
//!      build_direction / infill_density / infill_pattern / base material) and
//!      AsPrintedOptions (line_width / coupon / orthotropic),
//!   3. runs the β effective-property correlation once per zone — dense
//!      (Wall & Skin, ρ = 1) and infill (ρ = infill_density),
//!   4. builds three `AnisotropicMaterial` values (a transverse-isotropic — or,
//!      opt-in, orthotropic — law plus one shared [`MaterialFrame`] whose
//!      axial / z-axis is the build direction, so build-Z is the weakest axis),
//!      and
//!   5. packs them as a `Value::Field { source: AsPrintedZones, .. }` whose
//!      lambda slot is the 7-element zone payload documented on
//!      [`reify_ir::FieldSourceKind::AsPrintedZones`].
//!
//! Sampling (`reify_expr::sample_field_at`) reconstructs the γ box + params,
//! classifies the query point, and returns the matching precomputed material.
//! The solver-side consumption of the heterogeneous field is task ε's scope;
//! δ produces the value only.

use std::sync::Arc;

use reify_core::{DimensionVector, Type};
use reify_fdm::{
    AxisAlignedBox, BaseElastic, CouponOverride, DEFAULT_TOP_BOTTOM_NORMAL_THRESHOLD, InfillPattern,
    OrthotropicConstants, TransverseIsoConstants, Zone, effective_orthotropic,
    effective_transverse_isotropic, zone_solid_fraction,
};
use reify_ir::{FieldSourceKind, OpaqueState, StructureInstanceData, Value};

use super::{rejected_input_outcome, struct_data, structure};
use crate::arg_acceptance::{
    ArgSpec, FieldAcceptance, FieldRejection, accept_field, density_spec, dimensionless_spec,
    length_spec, pressure_spec,
};
use crate::{CancellationHandle, ComputeOutcome, RealizationReadHandle};

/// `@optimized("fdm::as_printed_material_r_fast")` ComputeNode trampoline.
///
/// Returns [`ComputeOutcome::Completed`] with a
/// `Value::Field { source: AsPrintedZones }`. When the inputs are incomplete or
/// the body realization is unavailable, it degrades honestly to a field whose
/// lambda is [`Value::Undef`] (every sample returns `Undef`) rather than
/// panicking — matching the Undef-for-Field convention of the stdlib surface.
/// A present numeric field the reader refuses — a number of the wrong dimension
/// or a value that is not a number at all — is [`ComputeOutcome::Failed`] with
/// that field's `DimensionedArgRejected` Error (`field_scalar`'s refusal rule).
pub fn as_printed_material_r_fast_trampoline(
    value_inputs: &[Value],
    realization_inputs: &[RealizationReadHandle],
    _options: &Value,
    _prior_warm_state: Option<&OpaqueState>,
    _cancellation: &CancellationHandle,
) -> ComputeOutcome {
    match build_as_printed_field(value_inputs, realization_inputs) {
        Ok(field) => ComputeOutcome::Completed {
            result: field.unwrap_or_else(degraded_field),
            new_warm_state: None,
            cost_per_byte: None,
            diagnostics: Vec::new(),
            structured_detail: vec![],
        },
        Err(rejection) => rejected_input_outcome(rejection.to_error(AS_PRINTED_MATERIAL)),
    }
}

/// The builtin both as-printed rungs (R-fast and R0) implement, named in their
/// rejection diagnostics.
pub(crate) const AS_PRINTED_MATERIAL: &str = "as_printed_material";

/// Build the `AsPrintedZones` material field: `Ok(None)` if any required input
/// is missing or unresolved (caller degrades to [`degraded_field`]), `Err` if a
/// present numeric field is refused.
fn build_as_printed_field(
    value_inputs: &[Value],
    realization_inputs: &[RealizationReadHandle],
) -> Result<Option<Value>, FieldRejection> {
    let (Some(process), Some(options)) = (
        value_inputs.get(1).and_then(struct_data),
        value_inputs.get(2).and_then(struct_data),
    ) else {
        return Ok(None);
    };

    // Every dimension-checked read runs before the realization lookup, so a
    // rejection never depends on whether the body has realized yet.
    let layer_height = field_scalar(process, "layer_height", &length_spec())?;
    let infill_density = field_real(process, "infill_density")?;
    let base = read_base_elastic(process)?;
    let line_width = field_scalar(options, "line_width", &length_spec())?;
    let coupon = read_coupon(options)?;

    let (
        Some(layer_height),
        Some(infill_density),
        Some(base),
        Some(line_width),
        Some(walls),
        Some(top_bottom_layers),
        Some(build_direction),
        Some(aabb),
    ) = (
        layer_height,
        infill_density,
        base,
        line_width,
        field_int(process, "walls"),
        field_int(process, "top_bottom_layers"),
        field_vec3(process, "build_direction"),
        body_aabb(realization_inputs),
    )
    else {
        return Ok(None);
    };
    let walls = walls.max(0) as u32;
    let top_bottom_layers = top_bottom_layers.max(0) as u32;
    let pattern = read_pattern(process);
    let orthotropic = matches!(options.fields.get("orthotropic"), Some(Value::Bool(true)));

    // The build axis is the (weak) axial direction; align the material frame's
    // z-axis with it so build-Z is the weakest axis (PRD C4 invariant). A
    // degenerate build_direction falls back to +Z.
    let build_unit = unit3(build_direction).unwrap_or([0.0, 0.0, 1.0]);
    let cos_threshold = DEFAULT_TOP_BOTTOM_NORMAL_THRESHOLD;
    let frame = material_frame(build_unit);

    // ── Per-zone β correlation: dense (Wall & Skin) vs infill ──────────────────
    let dense_rho = zone_solid_fraction(Zone::Wall, infill_density); // 1.0
    // Guard the β domain (ρ ∈ (0, 1]) so a malformed infill_density cannot trip
    // the correlation's debug_assert in test/debug builds.
    let infill_rho = zone_solid_fraction(Zone::Infill, infill_density).clamp(f64::MIN_POSITIVE, 1.0);

    let (mat_dense, mat_infill) = if orthotropic {
        (
            orthotropic_material(
                effective_orthotropic(base, dense_rho, pattern, &coupon),
                &frame,
            ),
            orthotropic_material(
                effective_orthotropic(base, infill_rho, pattern, &coupon),
                &frame,
            ),
        )
    } else {
        (
            transverse_iso_material(
                effective_transverse_isotropic(base, dense_rho, pattern, &coupon),
                &frame,
            ),
            transverse_iso_material(
                effective_transverse_isotropic(base, infill_rho, pattern, &coupon),
                &frame,
            ),
        )
    };
    // Walls and skins are both dense perimeters / solid layers.
    let mat_wall = mat_dense.clone();
    let mat_skin = mat_dense;

    // ── Pack the AsPrintedZones lambda payload (storage contract on
    //    reify_ir::FieldSourceKind::AsPrintedZones) ──────────────────────────────
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
        super::point3_length(aabb.min),
        super::point3_length(aabb.max),
        params_list,
        Value::Real(cos_threshold),
        mat_wall,
        mat_skin,
        mat_infill,
    ]);
    Ok(Some(as_printed_field_value(lambda)))
}

/// Wrap a lambda payload as the `Field<Point3<Length>, AnisotropicMaterial>`
/// produced by this node.
pub(crate) fn as_printed_field_value(lambda: Value) -> Value {
    Value::Field {
        domain_type: Type::point3(Type::length()),
        codomain_type: Type::StructureRef("AnisotropicMaterial".to_string()),
        source: FieldSourceKind::AsPrintedZones,
        lambda: Arc::new(lambda),
    }
}

/// Honest-degradation field: a well-typed `AsPrintedZones` field whose lambda is
/// `Undef`, so every `sample_field_at` falls through to `Undef`.
fn degraded_field() -> Value {
    as_printed_field_value(Value::Undef)
}

// ── Realization → AABB ──────────────────────────────────────────────────────

/// Derive the body's axis-aligned bounding box from `realization_inputs[0]`'s
/// mesh vertices (surface or volume). `None` when no handle, no mesh content, or
/// an empty vertex buffer.
fn body_aabb(realization_inputs: &[RealizationReadHandle]) -> Option<AxisAlignedBox> {
    let handle = realization_inputs.first()?;
    if let Some(mesh) = handle.surface_mesh() {
        aabb_from_vertices(&mesh.vertices)
    } else if let Some(vol) = handle.volume_mesh() {
        aabb_from_vertices(&vol.vertices)
    } else {
        None
    }
}

/// Component-wise min/max over a flat stride-3 `[x, y, z, …]` vertex buffer.
/// `None` if the buffer holds no complete vertex.
fn aabb_from_vertices(vertices: &[f32]) -> Option<AxisAlignedBox> {
    let mut min = [f64::INFINITY; 3];
    let mut max = [f64::NEG_INFINITY; 3];
    let mut any = false;
    for c in vertices.chunks_exact(3) {
        any = true;
        for k in 0..3 {
            let v = c[k] as f64;
            if v < min[k] {
                min[k] = v;
            }
            if v > max[k] {
                max[k] = v;
            }
        }
    }
    any.then_some(AxisAlignedBox { min, max })
}

// ── FDMProcess / AsPrintedOptions field marshalling ─────────────────────────

/// Read a numeric field's SI value through `spec`: `Ok(None)` when it is
/// absent, `none` or unresolved; `Err` naming `<Struct>.<key>` for any present
/// value the spec refuses — a number of the wrong dimension, or not a number at
/// all.
///
/// The one strict field reader: the FEA trampolines read their
/// `ElasticMaterial` fields through it too (`fea_inputs::isotropic_from_data`,
/// `fea_loads::material_density`), so a material field accepts the same values
/// whichever trampoline reads it.
pub(crate) fn field_scalar(
    data: &StructureInstanceData,
    key: &str,
    spec: &ArgSpec,
) -> Result<Option<f64>, FieldRejection> {
    match accept_field(data, key, spec) {
        FieldAcceptance::Accepted(si) => Ok(Some(si)),
        FieldAcceptance::Absent | FieldAcceptance::Undefined => Ok(None),
        FieldAcceptance::Rejected(rejection) => {
            Err(FieldRejection::new(&data.type_name, key, rejection))
        }
    }
}

/// [`field_scalar`] at a dimensionless position (a bare `Real`/`Int` or a
/// dimensionless `Scalar`).
pub(crate) fn field_real(
    data: &StructureInstanceData,
    key: &str,
) -> Result<Option<f64>, FieldRejection> {
    field_scalar(data, key, &dimensionless_spec())
}

/// Read an `Int` field; also accepts a `Real` (truncated).
pub(crate) fn field_int(data: &StructureInstanceData, key: &str) -> Option<i64> {
    match data.fields.get(key) {
        Some(Value::Int(i)) => Some(*i),
        Some(Value::Real(r)) => Some(*r as i64),
        _ => None,
    }
}

/// Read a 3-component `Vector` / `Point` / `Direction` field.
pub(crate) fn field_vec3(data: &StructureInstanceData, key: &str) -> Option<[f64; 3]> {
    match data.fields.get(key) {
        Some(Value::Vector(c) | Value::Point(c)) if c.len() == 3 => {
            Some([c[0].as_f64()?, c[1].as_f64()?, c[2].as_f64()?])
        }
        Some(Value::Direction { x, y, z }) => Some([*x, *y, *z]),
        _ => None,
    }
}

/// Map the FDMProcess `infill_pattern` enum to the β [`InfillPattern`].
/// Unknown / absent → `Gyroid` (the near-isotropic structural default).
fn read_pattern(process: &StructureInstanceData) -> InfillPattern {
    match process.fields.get("infill_pattern") {
        Some(Value::Enum { variant, .. }) => match variant.as_str() {
            "Cubic" => InfillPattern::Cubic,
            "Grid" => InfillPattern::Grid,
            "Triangular" => InfillPattern::Triangular,
            "Honeycomb" => InfillPattern::Honeycomb,
            _ => InfillPattern::Gyroid,
        },
        _ => InfillPattern::Gyroid,
    }
}

/// Read the base filament `ElasticMaterial` into a β [`BaseElastic`]
/// (`Ok(None)` when it or one of its fields is missing).
pub(crate) fn read_base_elastic(
    process: &StructureInstanceData,
) -> Result<Option<BaseElastic>, FieldRejection> {
    let Some(material) = process.fields.get("material").and_then(struct_data) else {
        return Ok(None);
    };
    let youngs_modulus = field_scalar(material, "youngs_modulus", &pressure_spec())?;
    let poisson_ratio = field_real(material, "poisson_ratio")?;
    let density = field_scalar(material, "density", &density_spec())?;
    Ok(match (youngs_modulus, poisson_ratio, density) {
        (Some(youngs_modulus), Some(poisson_ratio), Some(density)) => Some(BaseElastic {
            youngs_modulus,
            poisson_ratio,
            density,
        }),
        _ => None,
    })
}

/// Read the optional `FDMCouponOverride` into a β [`CouponOverride`]. An absent
/// or non-struct coupon, and each `none` field, is no override.
fn read_coupon(options: &StructureInstanceData) -> Result<CouponOverride, FieldRejection> {
    let Some(coupon) = options.fields.get("coupon").and_then(struct_data) else {
        return Ok(CouponOverride::default());
    };
    let modulus = |key| field_scalar(coupon, key, &pressure_spec());
    Ok(CouponOverride {
        ex: modulus("ex")?,
        ey: modulus("ey")?,
        ez: modulus("ez")?,
        gxy: modulus("gxy")?,
        infill_c: field_real(coupon, "infill_gibson_ashby_c")?,
        infill_n: field_real(coupon, "infill_gibson_ashby_n")?,
    })
}

// ── AnisotropicMaterial value construction ──────────────────────────────────

/// Build an `AnisotropicMaterial { law: TransverseIsotropicMaterial, frame }`
/// value from β transverse-isotropic constants.
fn transverse_iso_material(c: TransverseIsoConstants, frame: &Value) -> Value {
    let law = structure(
        "TransverseIsotropicMaterial",
        vec![
            ("e_in_plane", pressure(c.e_in_plane)),
            ("e_axial", pressure(c.e_axial)),
            ("nu_in_plane", Value::Real(c.nu_in_plane)),
            ("nu_axial", Value::Real(c.nu_axial)),
            ("g_axial", pressure(c.g_axial)),
            ("density", mass_density(c.density)),
            ("e_in_plane_provenance", empty_provenance()),
            ("e_axial_provenance", empty_provenance()),
            ("nu_in_plane_provenance", empty_provenance()),
            ("nu_axial_provenance", empty_provenance()),
            ("g_axial_provenance", empty_provenance()),
            ("density_provenance", empty_provenance()),
        ],
    );
    anisotropic_material(law, frame)
}

/// Build an `AnisotropicMaterial { law: OrthotropicMaterial, frame }` value from
/// β orthotropic constants (the opt-in known-unidirectional-raster path).
pub(crate) fn orthotropic_material(c: OrthotropicConstants, frame: &Value) -> Value {
    let law = structure(
        "OrthotropicMaterial",
        vec![
            ("e1", pressure(c.e1)),
            ("e2", pressure(c.e2)),
            ("e3", pressure(c.e3)),
            ("g12", pressure(c.g12)),
            ("g13", pressure(c.g13)),
            ("g23", pressure(c.g23)),
            ("nu12", Value::Real(c.nu12)),
            ("nu13", Value::Real(c.nu13)),
            ("nu23", Value::Real(c.nu23)),
            ("density", mass_density(c.density)),
            ("e1_provenance", empty_provenance()),
            ("e2_provenance", empty_provenance()),
            ("e3_provenance", empty_provenance()),
            ("g12_provenance", empty_provenance()),
            ("g13_provenance", empty_provenance()),
            ("g23_provenance", empty_provenance()),
            ("nu12_provenance", empty_provenance()),
            ("nu13_provenance", empty_provenance()),
            ("nu23_provenance", empty_provenance()),
            ("density_provenance", empty_provenance()),
        ],
    );
    anisotropic_material(law, frame)
}

fn anisotropic_material(law: Value, frame: &Value) -> Value {
    structure(
        "AnisotropicMaterial",
        vec![("law", law), ("frame", frame.clone())],
    )
}

/// One shared [`MaterialFrame`] whose z-axis is the (weak) build axis; the
/// in-plane x/y axes are an arbitrary orthonormal complement (the
/// transverse-isotropic isotropy plane makes their in-plane orientation
/// observationally irrelevant).
fn material_frame(build_z: [f64; 3]) -> Value {
    let (x_axis, y_axis) = orthonormal_complement(build_z);
    structure(
        "MaterialFrame",
        vec![
            ("origin", super::point3_length([0.0, 0.0, 0.0])),
            ("x_axis", vec3_dimensionless(x_axis)),
            ("y_axis", vec3_dimensionless(y_axis)),
            ("z_axis", vec3_dimensionless(build_z)),
        ],
    )
}

/// An empty-citation `MaterialPropertyProvenance` (structurally present, content
/// blank) — the δ correlations are Rust-side; the user-facing citation surface
/// is the stdlib `FDMCorrelationDefaults`.
fn empty_provenance() -> Value {
    structure(
        "MaterialPropertyProvenance",
        vec![
            ("source", Value::String(String::new())),
            ("reference", Value::String(String::new())),
            ("notes", Value::String(String::new())),
        ],
    )
}

// ── small value/geometry helpers ────────────────────────────────────────────

fn pressure(si: f64) -> Value {
    Value::Scalar {
        si_value: si,
        dimension: DimensionVector::PRESSURE,
    }
}

fn mass_density(si: f64) -> Value {
    Value::Scalar {
        si_value: si,
        dimension: DimensionVector::MASS_DENSITY,
    }
}

/// A `Vector3<Dimensionless>` — the representation `MaterialFrame`'s three
/// axes declare (task 5848) and what the stdlib `vec3(0, 0, 1)` axis literals
/// evaluate to. Components are `Value::Real`, the house spelling for a
/// dimensionless quantity (Invariant V), matching `default_frame3()`'s axes.
pub(crate) fn vec3_dimensionless(v: [f64; 3]) -> Value {
    Value::Vector(vec![
        Value::Real(v[0]),
        Value::Real(v[1]),
        Value::Real(v[2]),
    ])
}

/// Normalize a 3-vector to unit length; `None` if degenerate/non-finite.
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

/// Two unit vectors forming a right-handed orthonormal basis with the unit
/// `z` axis. Picks a reference axis not parallel to `z`, then crosses.
fn orthonormal_complement(z: [f64; 3]) -> ([f64; 3], [f64; 3]) {
    let reference = if z[0].abs() < 0.9 {
        [1.0, 0.0, 0.0]
    } else {
        [0.0, 1.0, 0.0]
    };
    let x = unit3(cross(reference, z)).unwrap_or([1.0, 0.0, 0.0]);
    let y = cross(z, x);
    (x, y)
}

// ── Rung → compute-key bridge (task ι / 3791, step-3 RED → step-4 GREEN) ────

/// Map a [`reify_fdm::Rung`] to the name of the registered compute-target
/// that produces the corresponding `AsPrintedZones` material field.
///
/// The returned string is the exact target string passed to
/// [`crate::compute_targets::register_compute_fns`] for the matching
/// trampoline:
/// - `Rung::RFast` → `"fdm::as_printed_material_r_fast"` (δ / 3786)
/// - `Rung::R0`    → `"fdm::as_printed_material_r0"` (θ / 3790)
///
/// This is the eval-side bridge from the abstract rung (domain knowledge in
/// `reify-fdm`) to the concrete registered producer (registry in `reify-eval`).
pub fn rung_compute_key(rung: reify_fdm::Rung) -> &'static str {
    match rung {
        reify_fdm::Rung::RFast => "fdm::as_printed_material_r_fast",
        reify_fdm::Rung::R0 => "fdm::as_printed_material_r0",
    }
}

#[cfg(test)]
mod iota_tests {
    use super::rung_compute_key;
    use reify_fdm::Rung;

    /// step-3 RED: `rung_compute_key` maps each rung to the exact registered
    /// target string. Fails to compile until step-4 adds `rung_compute_key`.
    #[test]
    fn rung_compute_key_r_fast_matches_registered_name() {
        assert_eq!(
            rung_compute_key(Rung::RFast),
            "fdm::as_printed_material_r_fast",
            "RFast rung must map to the δ registered target name"
        );
    }

    #[test]
    fn rung_compute_key_r0_matches_registered_name() {
        assert_eq!(
            rung_compute_key(Rung::R0),
            "fdm::as_printed_material_r0",
            "R0 rung must map to the θ registered target name"
        );
    }

    /// step-3 RED: both rung keys returned by `rung_compute_key` must be
    /// registered in `register_compute_fns`.
    #[test]
    fn rung_keys_are_registered_in_compute_dispatch() {
        use reify_test_support::mocks::MockConstraintChecker;
        let mut engine = crate::Engine::new(Box::new(MockConstraintChecker::new()), None);
        crate::compute_targets::register_compute_fns(&mut engine);
        for rung in [Rung::RFast, Rung::R0] {
            let key = rung_compute_key(rung);
            assert!(
                engine.compute_dispatch(key).is_some(),
                "rung {rung:?} key {key:?} must be registered in compute dispatch"
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use reify_ir::StructureTypeId;

    use super::*;

    /// Task 5848 retypes `FDMProcess.build_direction` from `Vec3<Length>` to a
    /// dimensionless 3-vector, moving its default from `vec3(0mm, 0mm, 1mm)` to
    /// `vec3(0, 0, 1)`. That is behaviour-preserving for the knockdown path
    /// because [`field_vec3`] reads components through `as_f64()` — blind to
    /// the dimension AND to the `Value` variant — and [`unit3`] normalises
    /// immediately, so `[0, 0, 0.001]` and `[0, 0, 1]` both reduce to the same
    /// `build_unit`.
    #[test]
    fn build_unit_is_identical_for_the_length_and_dimensionless_spellings() {
        let process = |build_direction: Value| StructureInstanceData {
            type_id: StructureTypeId(u32::MAX),
            type_name: "FDMProcess".to_string(),
            version: 1,
            fields: [("build_direction".to_string(), build_direction)]
                .into_iter()
                .collect(),
        };
        let millimetres = Value::Vector(vec![
            Value::Scalar { si_value: 0.0, dimension: DimensionVector::LENGTH },
            Value::Scalar { si_value: 0.0, dimension: DimensionVector::LENGTH },
            Value::Scalar { si_value: 1e-3, dimension: DimensionVector::LENGTH },
        ]);
        let dimensionless = vec3_dimensionless([0.0, 0.0, 1.0]);

        let build_unit = |v: Value| {
            unit3(field_vec3(&process(v), "build_direction").expect("build_direction must read"))
                .expect("a non-degenerate build direction must normalise")
        };
        assert_eq!(build_unit(millimetres), build_unit(dimensionless));
        assert_eq!(build_unit(vec3_dimensionless([0.0, 0.0, 1.0])), [0.0, 0.0, 1.0]);
    }
}
