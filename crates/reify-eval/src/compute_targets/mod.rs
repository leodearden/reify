//! Compute trampolines for `@optimized` stdlib functions.
//!
//! # Placement rationale (PRD §8 task η deviation)
//!
//! The PRD nominates `reify-stdlib` as the host for compute trampolines. The
//! actual dependency graph rules this out:
//!   `reify-eval → reify-expr → reify-stdlib`
//! Adding `reify-eval` as a normal dep of `reify-stdlib` would close that
//! cycle. `reify-eval` already has `reify-solver-elastic` as a direct dep
//! and owns `ComputeFn`/`ComputeOutcome`/`CancellationHandle`, so it is the
//! natural cycle-free host for trampolines in this slice.
//!
//! The architecturally-clean resolution is to move `ComputeFn`/`ComputeOutcome`/
//! `CancellationHandle`/`RealizationReadHandle` down into `reify-ir` (which has
//! no internal deps) so trampolines can then live in their respective
//! implementation crates (`reify-solver-elastic`, `reify-kernel-gmsh`, etc.).
//! That refactor is out of scope for this slice.

/// Task δ (3786): the `fdm::as_printed_material_r_fast` ComputeNode producing a
/// heterogeneous `Field<Point3<Length>, AnisotropicMaterial>` for an FDM body.
pub mod as_printed_material;
/// Task θ (3790): the `fdm::as_printed_material_r0` ComputeNode — the R0 rung
/// that maps a real sliced toolpath (PrusaSlicer G-code) to the same
/// `AsPrintedZones` field via closed-form Rodríguez/Halpin-Tsai/lumped-cooling
/// physics. Reuses δ's value/field helpers (widened to `pub(crate)`).
pub mod as_printed_material_r0;
/// Task 4092: pure boundary-condition resolution helpers bridging a typed
/// predicate topology selector and the realized tet mesh's per-node
/// `BoundaryAssociation` (`resolve_selector_faces` / `build_face_anchors` /
/// `boundary_node_set`).
pub mod bc_resolve;
pub mod buckling;
/// Task 4654 (R3a): carried-topology bundle for result values — the kernel-free
/// selector-resolvable topology that result values carry so R3b's eval-path
/// resolver can operate against baked data, never OCCT.
pub mod result_topology;
pub mod buckling_multi_case;
pub mod elastic_static;
/// Task η (3789): the `fdm::slice` ComputeNode — invokes PrusaSlicer as a
/// subprocess (never FFI, PRD DD#4), composes a deterministic settings profile,
/// runs it with cooperative SIGTERM→SIGKILL cancellation, and parses the
/// resulting G-code into a `Toolpath` `Value::StructureInstance`. Degrades
/// honestly (degraded Toolpath + Info `FdmSlicerUnavailable`) when no slicer is
/// on `$PATH`.
pub mod fdm_slice;
// Task 2929: FEA diagnostic mapping — FeaFailure → reify_core::Diagnostic.
pub mod fea_diagnostics;
pub mod form_find;
pub mod multi_case;
pub mod shell_solve;
/// Shared Tensegrity input-cracking helpers (node / index-pair / scalar / index
/// validation) reused by the `form_find` and `tensegrity_load` trampolines.
mod tensegrity_crack;
pub mod tensegrity_load;
/// Task η (4418): the `solver::membrane_load` ComputeNode — combined membrane +
/// bar/cable load analysis with a tension-only active set (slack cables + slack
/// patches). PRD `docs/prds/v0_6/tensegrity-membrane.md` §5 / §10 / §11.
pub mod membrane_load;

// ── Shared field-construction helpers ───────────────────────────────────────
//
// Both the elastic-static and buckling trampolines emit displacement and stress
// as `Value::Field{source:Sampled}`.  Centralising the type encoding here:
//   • eliminates verbatim duplication of the `Value::Field { ... }` construction
//   • makes future type-encoding changes (codomain, domain) a single-point edit
//   • keeps each trampoline focused on geometry and resampling logic

use std::sync::Arc;

use reify_core::DimensionVector;
use reify_ir::{FieldSourceKind, SampledField, Value};

/// Flatten per-node 3×3 tensors `[[f64;3];3]` into a stride-9 row-major
/// `Vec<f64>`.
///
/// This is a **generic 3×3 row-major flatten** with no stress-specific logic;
/// the name reflects its first use site but the operation is domain-neutral.
/// It is reused for both the nodal stress tensor (σ, symmetric) and the nodal
/// displacement-gradient tensor (∇u, generally asymmetric).
///
/// Layout per node: `[0][0], [0][1], [0][2], [1][0], [1][1], [1][2], [2][0],
/// [2][1], [2][2]` (i.e. `r` is the outer index, `c` the inner).
/// Shared by the elastic-static and buckling trampolines so the packing
/// convention is defined in exactly one place.
pub(crate) fn flatten_nodal_stress(nodal_stress: &[[[f64; 3]; 3]]) -> Vec<f64> {
    nodal_stress
        .iter()
        .flat_map(|s| {
            [
                s[0][0], s[0][1], s[0][2], s[1][0], s[1][1], s[1][2], s[2][0], s[2][1], s[2][2],
            ]
        })
        .collect()
}

/// Wrap a [`SampledField`] as a displacement `Value::Field`.
///
/// domain: `Point3<Length>`, codomain: `Vector3<Length>` — matches
/// `solver_elastic.ri:326` (PRD §4.2 type contract).
pub(crate) fn sampled_disp_field(sf: SampledField) -> Value {
    Value::Field {
        domain_type: reify_core::Type::point3(reify_core::Type::length()),
        codomain_type: reify_core::Type::vec3(reify_core::Type::length()),
        source: FieldSourceKind::Sampled,
        lambda: Arc::new(Value::SampledField(sf)),
    }
}

/// Wrap a [`SampledField`] as a stress `Value::Field`.
///
/// domain: `Point3<Length>`, codomain: `Tensor<2,3,Pressure>` — matches
/// `solver_elastic.ri:327` (PRD §4.2 type contract).
pub(crate) fn sampled_stress_field(sf: SampledField) -> Value {
    Value::Field {
        domain_type: reify_core::Type::point3(reify_core::Type::length()),
        codomain_type: reify_core::Type::tensor(
            2,
            3,
            reify_core::Type::Scalar {
                dimension: DimensionVector::PRESSURE,
            },
        ),
        source: FieldSourceKind::Sampled,
        lambda: Arc::new(Value::SampledField(sf)),
    }
}

/// Wrap a [`SampledField`] as a divergence `Value::Field`.
///
/// domain: `Point3<Length>`, codomain: `Real` (dimensionless scalar, stride 1)
/// — matches `solver_elastic.ri` `divergence : Field<Point3<Length>, Real>`
/// (PRD differential-field-operators.md task α).
pub(crate) fn sampled_divergence_field(sf: SampledField) -> Value {
    Value::Field {
        domain_type: reify_core::Type::point3(reify_core::Type::length()),
        codomain_type: reify_core::Type::dimensionless_scalar(),
        source: FieldSourceKind::Sampled,
        lambda: Arc::new(Value::SampledField(sf)),
    }
}

/// Wrap a [`SampledField`] as a displacement-gradient `Value::Field`.
///
/// domain: `Point3<Length>`, codomain: `Tensor<2,3,Real>` (dimensionless,
/// stride 9) — matches `solver_elastic.ri`
/// `gradient : Field<Point3<Length>, Tensor<2, 3, Real>>`.
/// Layout per node: `(∇u)[r][c] = ∂u_r/∂x_c`, row-major (r*3+c).
/// Dimensionless via dim_quotient_type (Length/Length), PRD task β D6.
pub(crate) fn sampled_gradient_field(sf: SampledField) -> Value {
    Value::Field {
        domain_type: reify_core::Type::point3(reify_core::Type::length()),
        codomain_type: reify_core::Type::tensor(2, 3, reify_core::Type::dimensionless_scalar()),
        source: FieldSourceKind::Sampled,
        lambda: Arc::new(Value::SampledField(sf)),
    }
}

/// Wrap a [`SampledField`] as a curl `Value::Field`.
///
/// domain: `Point3<Length>`, codomain: `Vector3<Real>` (dimensionless,
/// stride 3) — matches `solver_elastic.ri`
/// `curl : Field<Point3<Length>, Vector3<Real>>`.
/// Components: `∇×u = [∂u_z/∂y−∂u_y/∂z, ∂u_x/∂z−∂u_z/∂x, ∂u_y/∂x−∂u_x/∂y]`
/// (twice the infinitesimal rotation vector, PRD task β).
/// Dimensionless via dim_quotient_type (Length/Length), PRD task β D6.
pub(crate) fn sampled_curl_field(sf: SampledField) -> Value {
    Value::Field {
        domain_type: reify_core::Type::point3(reify_core::Type::length()),
        codomain_type: reify_core::Type::vec3(reify_core::Type::dimensionless_scalar()),
        source: FieldSourceKind::Sampled,
        lambda: Arc::new(Value::SampledField(sf)),
    }
}

/// Wrap a [`SampledField`] as a rotation `Value::Field`.
///
/// domain: `Point3<Length>`, codomain: `Vector3<Angle>` (stride 3) — matches
/// `solver_elastic.ri` `rotation : Field<Point3<Length>, Vector3<Angle>>`.
/// The payload is the infinitesimal rotation vector ω = ∇×u / 2, the axial
/// vector of the antisymmetric part of ∇u.
///
/// ## This is the designated crossing (ruling #6164)
///
/// Structurally this is a byte-for-byte clone of [`sampled_curl_field`] with
/// exactly ONE difference: `vec3(angle())` instead of
/// `vec3(dimensionless_scalar())` in the codomain slot. That single difference
/// is the entire ruling. The derivative algebra stays quotient-pure — ∇×u is
/// Length/Length and therefore genuinely dimensionless, so [`sampled_curl_field`]
/// deliberately keeps its dimensionless codomain and `result.curl` stays
/// type-identical to `curl(result.displacement)`. The radian is introduced only
/// here, by a named channel that ASSERTS an arc measure.
///
/// ## Why declaring the codomain is sufficient
///
/// The ANGLE tag on the DECLARED codomain is what makes the runtime emit
/// angle-dimensioned components, with no runtime change anywhere:
/// `reify-expr`'s `sample_at_point` takes its stride>1 branch and extracts
/// `component_type` from `Type::Vector { quantity }`, then its `wrap_result`
/// helper emits `Value::Scalar { dimension }` for any non-dimensionless
/// codomain. Nothing else has to know about rotation.
pub(crate) fn sampled_rotation_field(sf: SampledField) -> Value {
    Value::Field {
        domain_type: reify_core::Type::point3(reify_core::Type::length()),
        codomain_type: reify_core::Type::vec3(reify_core::Type::angle()),
        source: FieldSourceKind::Sampled,
        lambda: Arc::new(Value::SampledField(sf)),
    }
}

/// Derive the `rotation` [`SampledField`] from the `curl` one: ω = ∇×u / 2.
///
/// Halves every `data` entry onto a `"rotation"` field on curl's grid (see
/// [`sampled_field_on_grid_of`]), so the rotation channel shares the curl
/// channel's Regular3D grid exactly — no extra BVH resample pass, and
/// bit-identical node coordinates.
///
/// Halving is bit-exact: IEEE-754 division by 2.0 only decrements the exponent,
/// so it is exact for every normal operand (subnormal underflow is unreachable
/// at physical strain magnitudes). Callers may therefore pin `rotation == curl/2`
/// at 0 ULP.
///
/// ## Why rotation is DERIVED at wrap time and never stored
///
/// `crates/reify-compute-contract/src/elastic_result.rs` carries a FROZEN binary
/// wire header with `curl_len: u64` at a fixed byte offset, guarded by that
/// module's byte-exact golden header test (which pins `curl_len` as literal
/// hex). Adding a `rotation_len` slab would break that header, invalidate every
/// persisted cache entry, and force a format version bump — all to carry data
/// that is a pure ×½ of a slab already on the wire. Deriving instead means
/// existing cache entries gain a correct `.rotation` for free.
pub(crate) fn rotation_sf_from_curl(curl_sf: &SampledField) -> SampledField {
    sampled_field_on_grid_of(
        curl_sf,
        "rotation",
        curl_sf.data.iter().map(|c| c / 2.0).collect(),
    )
}

/// Derive the `shear_angles` [`SampledField`] from the stride-9 row-major
/// `gradient` one: per node, the Voigt-order ENGINEERING shears
/// (γ_yz, γ_zx, γ_xy) with γ_ij = g_ij + g_ji = 2·ε_ij.
///
/// A linear projection of each node's ∇u, so it lands on gradient's grid (see
/// [`sampled_field_on_grid_of`]) with no extra BVH pass. Derived at wrap time
/// and persisted nowhere, for the same frozen-wire-header reason as
/// [`rotation_sf_from_curl`].
///
/// `None` when `grad_sf.data` is not stride-9: the slab may come from a decoded
/// cache record that nothing upstream stride-checks, so the caller decides
/// whether a malformed slab is a construction bug or an absent channel.
pub(crate) fn shear_angles_sf_from_gradient(grad_sf: &SampledField) -> Option<SampledField> {
    if !grad_sf.data.len().is_multiple_of(9) {
        return None;
    }
    let shears = grad_sf
        .data
        .chunks_exact(9)
        .flat_map(|g| [g[5] + g[7], g[6] + g[2], g[1] + g[3]])
        .collect();
    Some(sampled_field_on_grid_of(grad_sf, "shear_angles", shears))
}

/// A new `data` payload named `name` on `source`'s grid: every grid-metadata
/// field is carried through verbatim, without cloning `source.data`.
///
/// The one constructor behind every wrap-time derived channel. The exhaustive
/// struct literal makes a new [`SampledField`] field a compile error here.
fn sampled_field_on_grid_of(source: &SampledField, name: &str, data: Vec<f64>) -> SampledField {
    SampledField {
        name: name.to_string(),
        kind: source.kind,
        bounds_min: source.bounds_min.clone(),
        bounds_max: source.bounds_max.clone(),
        spacing: source.spacing.clone(),
        axis_grids: source.axis_grids.clone(),
        interpolation: source.interpolation,
        data,
        oob_emitted: std::sync::atomic::AtomicBool::new(false),
    }
}

/// Wrap a [`SampledField`] as a `shear_angles` `Value::Field`.
///
/// domain: `Point3<Length>`, codomain: `Vector3<Angle>` (stride 3) — matches
/// `solver_elastic.ri` `shear_angles : Field<Point3<Length>, Vector3<Angle>>`.
/// The payload is (γ_yz, γ_zx, γ_xy) from [`shear_angles_sf_from_gradient`].
///
/// A named crossing like [`sampled_rotation_field`] (whose doc explains why the
/// declared codomain alone suffices), while [`sampled_gradient_field`] keeps its
/// `Tensor<2,3,Real>` codomain: a tensor carries one quantity slot, so angle
/// readings are extracted by named channels (INV-AD-3).
pub(crate) fn sampled_shear_angles_field(sf: SampledField) -> Value {
    Value::Field {
        domain_type: reify_core::Type::point3(reify_core::Type::length()),
        codomain_type: reify_core::Type::vec3(reify_core::Type::angle()),
        source: FieldSourceKind::Sampled,
        lambda: Arc::new(Value::SampledField(sf)),
    }
}

/// Assert `derived` sits bit-identically on `source`'s grid: every
/// grid-metadata field is equal; only `name` and `data` may differ.
///
/// The SINGLE enumeration of the grid-metadata field list for the derived
/// channels (`rotation`, `shear_angles`): every path test and wrapper unit test
/// calls through here, and the exhaustive destructure of `source` makes a new
/// [`SampledField`] field a compile error until it is classified below.
///
/// Carried-through grid metadata is also what proves a channel was DERIVED from
/// its source rather than independently resampled (an independent resample
/// would need a 6th `resample_multi_nodal_to_grid` entry, which does not exist).
#[cfg(test)]
pub(crate) fn assert_same_grid(derived: &SampledField, source: &SampledField, path: &str) {
    let SampledField {
        name: _,
        kind,
        bounds_min,
        bounds_max,
        spacing,
        axis_grids,
        interpolation,
        data: _,
        oob_emitted: _,
    } = source;
    assert_eq!(&derived.kind, kind, "{path}: grid kind");
    assert_eq!(&derived.bounds_min, bounds_min, "{path}: bounds_min");
    assert_eq!(&derived.bounds_max, bounds_max, "{path}: bounds_max");
    assert_eq!(&derived.spacing, spacing, "{path}: spacing");
    assert_eq!(&derived.axis_grids, axis_grids, "{path}: axis_grids");
    assert_eq!(
        &derived.interpolation, interpolation,
        "{path}: interpolation"
    );
}

/// Assert `rot` is exactly what [`rotation_sf_from_curl`] must produce from
/// `curl`: the same slab halved at 0 ULP, renamed, on the bit-identical grid.
///
/// 0 ULP is a numeric-premise claim, not laziness: IEEE-754 division by 2.0
/// only decrements the exponent, so it is exact for every normal operand
/// (subnormal underflow is unreachable at physical strain magnitudes).
#[cfg(test)]
pub(crate) fn assert_rotation_is_half_of(rot: &SampledField, curl: &SampledField, path: &str) {
    assert_eq!(
        rot.data.len(),
        curl.data.len(),
        "{path}: rotation must have the same node/stride count as curl"
    );
    let expected: Vec<f64> = curl.data.iter().map(|c| c / 2.0).collect();
    assert_eq!(
        rot.data, expected,
        "{path}: rotation data must be curl data halved element-wise, bit-exactly (0 ULP)"
    );
    assert_eq!(rot.name, "rotation", "{path}: derived field is renamed");
    assert_same_grid(rot, curl, path);
}

/// Assert `shear` is exactly what [`shear_angles_sf_from_gradient`] must
/// produce from `grad`: per node, the Voigt engineering shears
/// (γ_yz, γ_zx, γ_xy) with γ_ij = g_ij + g_ji of the row-major stride-9
/// gradient, at 0 ULP, renamed, on the bit-identical grid.
///
/// The oracle is written from the Voigt (i, j) index pairs, not the flattened
/// offsets the production derive uses, so it is independent evidence of the
/// Voigt ordering. 0 ULP holds because each expectation is the same single IEEE
/// addition of the same two operands (addition is commutative bitwise).
#[cfg(test)]
pub(crate) fn assert_shear_angles_project_gradient(
    shear: &SampledField,
    grad: &SampledField,
    path: &str,
) {
    const VOIGT_PAIRS: [(usize, usize); 3] = [(1, 2), (2, 0), (0, 1)];
    assert_eq!(
        grad.data.len() % 9,
        0,
        "{path}: gradient data must be stride-9 (one row-major 3×3 per node)"
    );
    assert_eq!(
        shear.data.len(),
        3 * (grad.data.len() / 9),
        "{path}: shear_angles must carry one stride-3 vector per gradient node"
    );
    let expected: Vec<f64> = grad
        .data
        .chunks_exact(9)
        .flat_map(|g| VOIGT_PAIRS.map(|(i, j)| g[3 * i + j] + g[3 * j + i]))
        .collect();
    assert_eq!(
        shear.data, expected,
        "{path}: shear_angles must be (γ_yz, γ_zx, γ_xy) = (g12+g21, g20+g02, g01+g10) \
         of the gradient, bit-exactly (0 ULP)"
    );
    assert_eq!(
        shear.name, "shear_angles",
        "{path}: derived field is renamed"
    );
    assert_same_grid(shear, grad, path);
}

/// Wrap a [`SampledField`] as an error-indicator `Value::Field`.
///
/// domain: `Point3<Length>`, codomain: `Pressure` (Pa, dimensioned scalar,
/// stride 1) — matches `solver_elastic.ri`
/// `error_indicator : Option<Field<Point3<Length>, Pressure>>` (task 4910).
/// Mirrors [`sampled_divergence_field`], but the codomain is a
/// PRESSURE-dimensioned scalar rather than a dimensionless one: the wrapped
/// data is the per-node Frobenius norm of the ZZ stress-error tensor
/// (`ZzIndicator::per_element_stress_error`, resampled to nodal/grid), a
/// Pa-valued quantity — distinct from the dimensionless energy-norm `eta_e`
/// that drives Dörfler marking.
pub(crate) fn sampled_error_indicator_field(sf: SampledField) -> Value {
    Value::Field {
        domain_type: reify_core::Type::point3(reify_core::Type::length()),
        codomain_type: reify_core::Type::Scalar {
            dimension: DimensionVector::PRESSURE,
        },
        source: FieldSourceKind::Sampled,
        lambda: Arc::new(Value::SampledField(sf)),
    }
}

// ── Scalar / point / list builders (form-find result encoding) ──────────────
//
// The form-find trampoline emits its result as plain dimensioned `Value::Scalar`
// coordinates and forces wrapped in `Value::Point` / `Value::List`.  Centralising
// these builders here — rather than hand-rolling the `Value::Scalar { .. }`
// literal and the map-collect idiom inside the trampoline — keeps the
// dimension/encoding choice a single-point edit, the same rationale as the field
// helpers above.

/// A dimensioned quantity `Value::Scalar` (SI value + dimension). The single
/// definition site for the `Value::Scalar { .. }` encoding used by the builders
/// below.
fn scalar(si_value: f64, dimension: DimensionVector) -> Value {
    Value::Scalar {
        si_value,
        dimension,
    }
}

/// A Length-dimensioned coordinate Scalar (SI metres).
pub(crate) fn length(m: f64) -> Value {
    scalar(m, DimensionVector::LENGTH)
}

/// A 3-component `Value::Point` of Length-dimensioned coordinate Scalars.
pub(crate) fn point3_length(p: [f64; 3]) -> Value {
    Value::Point(vec![length(p[0]), length(p[1]), length(p[2])])
}

/// A Velocity-dimensioned Scalar (SI metres per second).
pub(crate) fn velocity(m_per_s: f64) -> Value {
    scalar(m_per_s, DimensionVector::VELOCITY)
}

/// A Temperature-dimensioned Scalar (SI kelvin — never °C: `degC` is an affine
/// unit, so a Temperature Scalar's `si_value` is always absolute).
pub(crate) fn temperature(kelvin: f64) -> Value {
    scalar(kelvin, DimensionVector::TEMPERATURE)
}

/// A 3-component `Value::Vector` of Length-dimensioned Scalars.
///
/// The displacement-field analogue of [`point3_length`]: a displacement is a
/// vector (a delta), not a position, so it lowers to `Value::Vector` rather than
/// `Value::Point`. Used by the tensegrity-load trampoline for its per-node
/// deflection output.
pub(crate) fn vec3_length(v: [f64; 3]) -> Value {
    Value::Vector(vec![length(v[0]), length(v[1]), length(v[2])])
}

/// One `dimension`-typed `Value::Scalar` per SI value, in input order.
pub(crate) fn scalar_list(values: &[f64], dimension: DimensionVector) -> Vec<Value> {
    values.iter().map(|&v| scalar(v, dimension)).collect()
}

// ── Native-unit constructors (printer-native → DSL-visible projection) ──────
//
// A parser layer stays in the units its source is written in, for lossless
// fidelity; the DSL-visible projection of that payload is SI and dimensioned.
// These constructors ARE that projection, so each conversion factor is written
// once and every call site reads as the unit it is handed.

/// Millimetres → SI metres. Same name and value as the other mm→SI boundaries
/// in the workspace (`as_printed_material_r0.rs`, `reify-fdm/src/r0.rs`,
/// `reify-stdlib`'s `trajectory::gcode_import`), so grepping `MM_TO_M`
/// enumerates all of them — and all of them convert, so no unconverted
/// G-code→DSL `Value` seam is left (surveyed under task #6301).
const MM_TO_M: f64 = 1.0e-3;

/// G-code feedrate mm·min⁻¹ → SI m·s⁻¹, as the DIVISOR (1e3 millimetres per
/// metre × 60 seconds per minute) rather than a rounded reciprocal:
/// `1800.0 / 60_000.0` is exactly 0.03 where `1800.0 * (1.0 / 60_000.0)` is
/// 0.030000000000000002. Pinned by `fdm_slice.rs`'s
/// `speed_conversion_divides_rather_than_multiplying_a_reciprocal`.
const MM_PER_MIN_PER_M_PER_S: f64 = 60_000.0;

/// °C → K. Not a free choice: this is the offset the language itself declares
/// for `degC` (`crates/reify-compiler/stdlib/units.ri`, `pub unit degC :
/// Temperature = 1 offset 273.15`), so the affine conversion stays traceable to
/// that declaration rather than reading as a magic number.
const DEG_C_TO_K_OFFSET: f64 = 273.15;

/// A Length Scalar from a native-millimetre measurement.
pub(crate) fn length_mm(mm: f64) -> Value {
    length(mm * MM_TO_M)
}

/// A `Point3<Length>` from native-millimetre coordinates — the shape
/// `resolve_point3_length_arg` requires of a point passed to a geometry builtin.
pub(crate) fn point3_length_mm(p: [f64; 3]) -> Value {
    point3_length([p[0] * MM_TO_M, p[1] * MM_TO_M, p[2] * MM_TO_M])
}

/// A Velocity Scalar from a G-code feedrate in mm·min⁻¹.
pub(crate) fn velocity_mm_per_min(mm_per_min: f64) -> Value {
    velocity(mm_per_min / MM_PER_MIN_PER_M_PER_S)
}

/// A Temperature Scalar from °C (absolute kelvin out — `degC` is affine).
pub(crate) fn temperature_deg_c(deg_c: f64) -> Value {
    temperature(deg_c + DEG_C_TO_K_OFFSET)
}

/// Register all compute trampolines shipped in this slice.
///
/// Must be called once at engine startup — typically in the same initialisation
/// block that builds the engine (see `examples/fea_cantilever_smoke.ri` usage).
///
/// Panics if any target is registered twice (duplicate registrations indicate
/// a double-call or a test-isolation bug).
pub fn register_compute_fns(engine: &mut crate::Engine) {
    engine.register_compute_fn(
        "solver::elastic_static",
        elastic_static::solve_elastic_static_trampoline as crate::ComputeFn,
    );
    // Producer-half hook (task 4091): mark FEA as demanding a *tet VolumeMesh*
    // realization (not a surface Mesh) so that once a geometry argument is wired
    // to solve_elastic_static downstream (2930 / P2=4092), the static demand pass
    // projects the body's realization as a VolumeMesh into the node's
    // realization_inputs — which solve_elastic_static_trampoline already consumes
    // via realized_solver_mesh. No production effect on the current FEA signature
    // (no geometry arg → the demand override never fires).
    engine.register_volume_mesh_demand("solver::elastic_static");
    // Producer-half hook (task 4092, P2): mark FEA as demanding a *boundary*-
    // attributed VolumeMesh, so that once a geometry argument is wired to
    // solve_elastic_static (4370 / Bmig), the realization edge routes the body
    // surface through the gmsh attributed producer and threads a
    // BoundaryAssociation onto the realized mesh — which the trampoline's
    // face-selector BC path (loads_supports_to_bc_node_sets) consumes. Boundary
    // demand implies VolumeMesh demand, so this is additive over the 4091 hook.
    // No production effect on the current FEA signature (no geometry arg → the
    // demand override never fires).
    engine.register_volume_mesh_boundary_demand("solver::elastic_static");
    engine.register_compute_fn(
        "solver::buckling",
        buckling::solve_buckling_trampoline as crate::ComputeFn,
    );
    engine.register_compute_fn(
        "solver::form_find",
        form_find::solve_form_find_trampoline as crate::ComputeFn,
    );
    engine.register_compute_fn(
        "solver::form_find_free",
        form_find::solve_form_find_free_trampoline as crate::ComputeFn,
    );
    // Tensegrity T3b (task 3798): load analysis with a tension-only active set.
    // PRD §11 Q2 decision — a DEDICATED target (disjoint input/result shapes +
    // active-set wrapper), not an extension of solver::elastic_static.
    engine.register_compute_fn(
        "solver::tensegrity_load",
        tensegrity_load::solve_tensegrity_load_trampoline as crate::ComputeFn,
    );
    // Tensegrity-membrane η (task 4418, layer M2): combined membrane + bar/cable
    // load analysis with a tension-only active set (slack cables + slack patches).
    engine.register_compute_fn(
        "solver::membrane_load",
        membrane_load::solve_membrane_load_trampoline as crate::ComputeFn,
    );
    engine.register_compute_fn(
        "solver::multi_case",
        multi_case::solve_multi_case_trampoline as crate::ComputeFn,
    );
    // Producer-half hook (task 4870): mark multi-case FEA as demanding a *tet
    // VolumeMesh* realization, mirroring the solver::elastic_static hook above.
    // Once the `body : Solid` overload of solve_load_cases (fea_multi_case.ri) is
    // called, the static demand pass projects the body's realization as a
    // VolumeMesh into the node's realization_inputs — which the multi_case
    // trampoline forwards UNCHANGED to every per-case elastic sub-solve, so all
    // cases sharing the body share ONE realized mesh. No boundary demand here:
    // face-selector BC attribution (task 4092) is out of scope for 4870.
    engine.register_volume_mesh_demand("solver::multi_case");
    engine.register_compute_fn(
        "solver::buckling_multi_case",
        buckling_multi_case::solve_buckling_multi_case_trampoline as crate::ComputeFn,
    );
    // FDM δ (task 3786): the R-fast as-printed material-field producer. Derives
    // the body AABB from its realization mesh, classifies wall/skin/infill zones
    // (γ), runs the β effective-property correlation per zone, and emits a
    // `Value::Field{source: AsPrintedZones}` of `AnisotropicMaterial`.
    engine.register_compute_fn(
        "fdm::as_printed_material_r_fast",
        as_printed_material::as_printed_material_r_fast_trampoline as crate::ComputeFn,
    );
    // FDM θ (task 3790): the R0 as-printed material-field producer — the
    // closed-form-physics rung alongside R-fast. Parses a real sliced toolpath
    // (PrusaSlicer G-code) and maps it to per-zone *orthotropic* constants
    // (Rodríguez 2003 + Halpin-Tsai fibre + lumped-cooling build-Z knockdown),
    // emitting the same `Value::Field{source: AsPrintedZones}` of
    // `AnisotropicMaterial`. Both rungs coexist; the progressive R-fast→R0
    // selection + warm-start is the integration gate ι's concern.
    engine.register_compute_fn(
        "fdm::as_printed_material_r0",
        as_printed_material_r0::as_printed_material_r0_trampoline as crate::ComputeFn,
    );
    // FDM η (task 3789): the `fdm::slice` ComputeNode — invokes PrusaSlicer as a
    // subprocess (never FFI, PRD DD#4), composes a deterministic settings
    // profile, runs it with cooperative SIGTERM→SIGKILL cancellation, and parses
    // the produced G-code into a `Toolpath` `Value::StructureInstance`. Degrades
    // honestly (empty Toolpath + Info `FdmSlicerUnavailable`) when no slicer is
    // on `$PATH`.
    engine.register_compute_fn(
        "fdm::slice",
        fdm_slice::fdm_slice_trampoline as crate::ComputeFn,
    );
    // The modal trampoline lives in `crate::modal_ops` (not `compute_targets`):
    // it shares the FEA-eigensolve machinery with the modal core solver and its
    // unit tests, which co-locate there. Mirrors the buckling/elastic placement
    // rationale at the top of this module.
    engine.register_compute_fn(
        "modal::free_vibration",
        crate::modal_ops::solve_modal_analysis_trampoline as crate::ComputeFn,
    );
    // The transient-response trampolines (task ι) also live in `crate::modal_ops`,
    // alongside the free-vibration trampoline whose Φ serialization they consume.
    engine.register_compute_fn(
        "modal::transient_response",
        crate::modal_ops::solve_transient_response_trampoline as crate::ComputeFn,
    );
    engine.register_compute_fn(
        "modal::displacement_at",
        crate::modal_ops::displacement_at_trampoline as crate::ComputeFn,
    );
    // The mechanism-modal trampoline (κ-modal-bridge, task #4271) lives in
    // `crate::modal_ops` alongside the free-vibration and transient trampolines:
    // it reuses `solve_eigen_dense` + `eigenvalue_to_frequency_hz` (the same
    // generalized-eigensolve primitives) and the `degenerate_modal_result` /
    // `placeholder_part` helpers co-located there.
    engine.register_compute_fn(
        "modal::mechanism_modal",
        crate::modal_ops::solve_mechanism_modal_trampoline as crate::ComputeFn,
    );
    // The inverse-dynamics trajectory trampoline (RBD-ι, task 3838) lives in
    // `crate::dynamics_ops` (not `compute_targets`): it co-locates with the
    // body_mass_props Value-marshalling + warm-state cache there, and the
    // reify-eval ← reify-stdlib dep direction forbids the pure cache-key half
    // (`reify_stdlib::dynamics::trampoline`) from holding the ComputeOutcome /
    // CancellationHandle types. Mirrors the modal placement above.
    engine.register_compute_fn(
        "dynamics::inverse_dynamics",
        crate::dynamics_ops::solve_inverse_dynamics_trampoline as crate::ComputeFn,
    );
    // The trajectory forward-sim and input-shape trampolines (task π, 3876) live
    // in `crate::trajectory_ops`: they co-locate with `worst_case_residual_fraction`
    // and the `SimulateTrajectoryCacheKey`/`InputShapeCacheKey` warm-state caches
    // there, mirroring the modal/dynamics placement rationale above.
    engine.register_compute_fn(
        "trajectory::simulate",
        crate::trajectory_ops::simulate_trajectory_trampoline as crate::ComputeFn,
    );
    engine.register_compute_fn(
        "trajectory::input_shape",
        crate::trajectory_ops::input_shape_trampoline as crate::ComputeFn,
    );
}

/// How a caller supplies — or explicitly declines — the mesh-morph producer to
/// [`Engine::register_production_compute_fns`][crate::Engine::register_production_compute_fns].
///
/// A closed two-variant enum with **no** `Default` and **no** `Option<fn>`:
/// there is no silent "forgot" state. Every caller must make an explicit,
/// reviewed choice — supply the producer fn, or state the reason it is
/// structurally unavailable. That is what lets the drift guard (task A5) prove
/// the canonical bundler is used everywhere, and it forecloses the GUI-drift bug
/// class the PRD (Contract C1) targets: an `Option`-shaped or `Default`-carrying
/// API lets a forgetful caller pass `None` (or rely on `Default`) and compile
/// clean while the morph producer is silently never installed.
pub enum MorphRegistration {
    /// The caller (a normal build, with the optional `reify-mesh-morph` dep on
    /// the graph — CLI / GUI) supplies the concrete `fn(&mut Engine)` that
    /// installs the producer; in production this is
    /// `reify_mesh_morph::register_morph_producer`. Passing the fn *pointer*
    /// rather than naming the type keeps reify-eval from ever naming
    /// `reify-mesh-morph`, avoiding the reify-mesh-morph → reify-eval →
    /// reify-mesh-morph dependency cycle.
    Enabled(fn(&mut crate::Engine)),
    /// The caller structurally cannot link `reify-mesh-morph` in this build — e.g.
    /// reify-eval's own `test_runner`, for which reify-mesh-morph is dev-dep-only
    /// (task 4744) — so no producer is installed. The compute trampolines are
    /// still registered; `reason` documents the omission for the debug log.
    Unavailable {
        /// Human-readable reason the morph producer is not installed (logged at
        /// debug level; must be non-empty).
        reason: &'static str,
    },
}

impl crate::Engine {
    /// The canonical compute-trampoline registration bundler required by
    /// INV-FEA-1 — the single place that registers the full production set of
    /// compute trampolines on an [`Engine`][crate::Engine].
    ///
    /// Replaces the three independently hand-rolled bundlers (CLI
    /// `register_compute_trampolines`, `test_runner::build_test_engine`, and the
    /// GUI engine setup); tasks A2/A3/A4 migrate those call sites to this one
    /// method and A5 adds the drift guard that keeps them migrated. The bundle
    /// is, in order: [`register_compute_fns`] (FEA / buckling / modal / form-find
    /// / multi-case / dynamics / trajectory), then
    /// [`register_shell_extract_compute_fns`][crate::register_shell_extract_compute_fns]
    /// (shell mid-surface extraction), then the mesh-morph producer per `morph`.
    ///
    /// # Panics
    ///
    /// Panics if called twice on the same engine — the second call re-runs
    /// [`register_compute_fns`], which re-registers `solver::elastic_static`
    /// unconditionally and trips the duplicate-target guard in
    /// [`register_compute_fn`][crate::Engine::register_compute_fn]. This inherits
    /// the same single-install discipline the individual registrars enforce; the
    /// bundler adds no new guard.
    pub fn register_production_compute_fns(&mut self, morph: MorphRegistration) {
        register_compute_fns(self);
        crate::register_shell_extract_compute_fns(self);
        match morph {
            MorphRegistration::Enabled(f) => f(self),
            MorphRegistration::Unavailable { reason } => {
                debug_assert!(!reason.is_empty(), "Unavailable reason must be non-empty");
                // The debug_assert above is compiled out in release, so guard the
                // log too: never emit a reasonless line if an empty `reason` slips
                // through in a release build. A fallback, not a panic — this is a
                // debug-only log path and every real caller passes a &'static str
                // literal, so this branch is defense-in-depth, never hot.
                let reason = if reason.is_empty() { "(unspecified)" } else { reason };
                tracing::debug!(reason, "mesh-morph producer not registered on this Engine");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::Engine;
    use reify_test_support::mocks::MockConstraintChecker;

    /// step-5 RED (task 4910): `sampled_error_indicator_field` wraps a
    /// [`reify_ir::SampledField`] as a `Value::Field` with domain
    /// `Point3<Length>` and codomain `Pressure` (Pa) — the type contract for
    /// `ElasticResult.error_indicator : Option<Field<Point3<Length>, Pressure>>`
    /// in `solver_elastic.ri`. Mirrors [`super::sampled_divergence_field`] but
    /// with a dimensioned (not dimensionless) scalar codomain.
    ///
    /// RED: `sampled_error_indicator_field` does not exist yet.
    #[test]
    fn sampled_error_indicator_field_wraps_pressure_scalar_field() {
        use reify_core::DimensionVector;
        use reify_ir::{FieldSourceKind, InterpolationKind, SampledField, SampledGridKind, Value};
        use std::sync::atomic::AtomicBool;

        let sf = SampledField {
            name: "error_indicator".to_string(),
            kind: SampledGridKind::Regular1D,
            bounds_min: vec![0.0],
            bounds_max: vec![1.0],
            spacing: vec![1.0],
            axis_grids: vec![vec![0.0, 1.0]],
            interpolation: InterpolationKind::Linear,
            data: vec![10.0, 20.0],
            oob_emitted: AtomicBool::new(false),
        };

        let value = super::sampled_error_indicator_field(sf);

        match value {
            Value::Field {
                domain_type,
                codomain_type,
                source,
                ..
            } => {
                assert_eq!(
                    domain_type,
                    reify_core::Type::point3(reify_core::Type::length()),
                    "error_indicator field domain must be Point3<Length>"
                );
                assert_eq!(
                    codomain_type,
                    reify_core::Type::Scalar {
                        dimension: DimensionVector::PRESSURE,
                    },
                    "error_indicator field codomain must be a Pressure-dimensioned scalar (Pa)"
                );
                assert_eq!(
                    source,
                    FieldSourceKind::Sampled,
                    "error_indicator field must be source Sampled"
                );
            }
            other => panic!("expected Value::Field, got {other:?}"),
        }
    }

    /// step-11 RED (task 4091): `register_compute_fns` registers the producer-side
    /// VolumeMesh demand for `solver::elastic_static`, so that once a geometry
    /// argument is wired to FEA (downstream — 2930 / P2=4092) its realization is
    /// demanded as a tet `VolumeMesh` (not a surface `Mesh`). A sibling trampoline
    /// target (`solver::buckling`) must stay non-demanding — no spurious demand.
    ///
    /// Mirrors the engine_admin.rs `register_volume_mesh_demand` registry unit
    /// test, asserting through the `demands_volume_mesh` reader.
    ///
    /// RED: `register_compute_fns` does not yet call `register_volume_mesh_demand`,
    /// so `demands_volume_mesh("solver::elastic_static")` is `false` (step-12 wires it).
    #[test]
    fn register_compute_fns_marks_elastic_static_volume_mesh_demand() {
        let mut engine = Engine::new(Box::new(MockConstraintChecker::new()), None);
        super::register_compute_fns(&mut engine);

        assert!(
            engine.demands_volume_mesh("solver::elastic_static"),
            "register_compute_fns must register the solver::elastic_static VolumeMesh \
             demand (task 4091 producer-half hook)"
        );
        // Control: a sibling solver target must NOT be VolumeMesh-demanding.
        assert!(
            !engine.demands_volume_mesh("solver::buckling"),
            "solver::buckling must stay non-VolumeMesh-demanding (no spurious demand)"
        );
        // Task 4092: elastic_static is ALSO boundary-demanding (FEA face-selector
        // BC producer-half hook); a sibling solver target must not be.
        assert!(
            engine.demands_boundary("solver::elastic_static"),
            "register_compute_fns must register the solver::elastic_static boundary \
             demand (task 4092 producer-half hook)"
        );
        assert!(
            !engine.demands_boundary("solver::buckling"),
            "solver::buckling must stay non-boundary-demanding (no spurious demand)"
        );
    }

    /// step-5 RED (task 4870): `register_compute_fns` must ALSO register the
    /// producer-side VolumeMesh demand for `solver::multi_case`, so that once a
    /// `body : Solid` argument is wired to `solve_load_cases` its realization is
    /// demanded as a tet `VolumeMesh` (mirroring the `solver::elastic_static`
    /// hook). Every case sharing the body then shares ONE realized mesh.
    ///
    /// Unlike `solver::elastic_static`, multi_case gets VolumeMesh demand ONLY —
    /// NOT boundary demand: face-selector BC attribution (task 4092) is out of
    /// scope for 4870, and boundary demand would spuriously route the body
    /// surface through the attributed gmsh producer.
    ///
    /// RED: `register_compute_fns` does not yet call
    /// `register_volume_mesh_demand("solver::multi_case")`, so
    /// `demands_volume_mesh("solver::multi_case")` is `false` (step-6a wires it).
    #[test]
    fn register_compute_fns_marks_multi_case_volume_mesh_demand() {
        let mut engine = Engine::new(Box::new(MockConstraintChecker::new()), None);
        super::register_compute_fns(&mut engine);

        assert!(
            engine.demands_volume_mesh("solver::multi_case"),
            "register_compute_fns must register the solver::multi_case VolumeMesh \
             demand (task 4870 — body : Solid overload of solve_load_cases)"
        );
        // multi_case must NOT be boundary-demanding: face-selector BC attribution
        // (task 4092) is out of scope for 4870.
        assert!(
            !engine.demands_boundary("solver::multi_case"),
            "solver::multi_case must stay non-boundary-demanding (task 4092 \
             face-selector BCs are out of scope for 4870)"
        );
    }

    // ── register_production_compute_fns / MorphRegistration (task 5072) ──────
    //
    // These three tests pin the canonical compute-trampoline bundler
    // `Engine::register_production_compute_fns(MorphRegistration)` that INV-FEA-1
    // requires (replacing the three hand-rolled bundlers migrated in A2/A3/A4).
    // Observables: the existing `Engine::morph_producer()` accessor (did the
    // `Enabled(f)` arm run f?) + `Engine::compute_dispatch(target)` (did the
    // shared bundle body run?). No reify-mesh-morph link is needed — a trivial
    // in-crate `NoopTestProducer` stands in for the production producer fn.

    /// A trivial in-crate [`crate::MorphProducer`] so the `Enabled(f)` arm has a
    /// concrete producer to install without linking reify-mesh-morph (dev-dep-
    /// only per task 4744). It never actually morphs — `Ineligible` is the
    /// common, expected edit class — which is irrelevant here: the test only
    /// observes that a producer became *installed*.
    struct NoopTestProducer;

    impl crate::MorphProducer for NoopTestProducer {
        fn try_morph(&self, _ctx: crate::MorphRequest<'_>) -> crate::MorphResult {
            crate::MorphResult::Ineligible("test".into())
        }
    }

    /// A non-capturing free `fn(&mut Engine)` — a valid value for the
    /// `MorphRegistration::Enabled` variant, standing in for production's
    /// `reify_mesh_morph::register_morph_producer`.
    fn install_test_producer(e: &mut Engine) {
        e.register_morph_producer(Box::new(NoopTestProducer));
    }

    /// RED (task 5072): the `Enabled(f)` arm must invoke `f` (installing the
    /// morph producer) AND run the shared bundle body (registering the compute
    /// trampolines). Asserts both observables: `morph_producer().is_some()`
    /// (f ran) and `compute_dispatch("solver::elastic_static").is_some()`
    /// (the `register_compute_fns` half ran).
    ///
    /// RED: `MorphRegistration` and `register_production_compute_fns` do not
    /// exist yet — this `mod tests` fails to compile until step-2 defines them.
    #[test]
    fn register_production_compute_fns_enabled_invokes_f() {
        let mut engine = Engine::new(Box::new(MockConstraintChecker::new()), None);
        engine.register_production_compute_fns(super::MorphRegistration::Enabled(
            install_test_producer,
        ));

        assert!(
            engine.morph_producer().is_some(),
            "Enabled(f) must invoke f, installing the morph producer"
        );
        assert!(
            engine.compute_dispatch("solver::elastic_static").is_some(),
            "register_production_compute_fns must run the register_compute_fns bundle half"
        );
    }

    /// RED (task 5072): the `Unavailable { reason }` arm must NOT install a morph
    /// producer (the `Enabled` arm is not taken) yet must STILL run the shared
    /// bundle body — both `register_compute_fns` and
    /// `register_shell_extract_compute_fns`. This is the `test_runner` shape
    /// (reify-mesh-morph is dev-dep-only, so it cannot supply a producer fn).
    ///
    /// RED: `MorphRegistration` and `register_production_compute_fns` do not
    /// exist yet.
    #[test]
    fn register_production_compute_fns_unavailable_does_not_install_morph() {
        let mut engine = Engine::new(Box::new(MockConstraintChecker::new()), None);
        engine.register_production_compute_fns(super::MorphRegistration::Unavailable {
            reason: "test-runner: reify-mesh-morph is dev-dep-only",
        });

        assert!(
            engine.morph_producer().is_none(),
            "Unavailable must not install a morph producer (Enabled arm not taken)"
        );
        assert!(
            engine.compute_dispatch("solver::elastic_static").is_some(),
            "Unavailable must still run the register_compute_fns bundle half"
        );
        assert!(
            engine.compute_dispatch("shell-extract::extract").is_some(),
            "Unavailable must still run the register_shell_extract_compute_fns bundle half"
        );
    }

    /// RED (task 5072): calling the bundler twice on one engine must panic — the
    /// inherited duplicate-registration guard. The 2nd call re-runs
    /// `register_compute_fns`, which re-registers `solver::elastic_static`
    /// unconditionally, tripping `Engine::register_compute_fn`'s `Entry::Occupied`
    /// panic (`"register_compute_fn: duplicate target ..."`). No new guard is
    /// written by this task — the contract is structural.
    ///
    /// RED: `register_production_compute_fns` does not exist yet.
    #[test]
    #[should_panic(expected = "duplicate target")]
    fn register_production_compute_fns_twice_panics() {
        let mut engine = Engine::new(Box::new(MockConstraintChecker::new()), None);
        engine.register_production_compute_fns(super::MorphRegistration::Unavailable {
            reason: "test",
        });
        engine.register_production_compute_fns(super::MorphRegistration::Unavailable {
            reason: "test",
        });
    }
    /// step-3 RED (ruling #6164): the `rotation` derivative channel is the
    /// DESIGNATED CROSSING where the radian enters the elastic-result algebra.
    /// This test pins the whole mechanism by which the `rad` tag reaches the
    /// runtime: `sampled_rotation_field` DECLARES a `Vector3<Angle>` codomain,
    /// and `reify-expr`'s `sample_at_point` / `wrap_result`
    /// reads that declared codomain to decide what `Value::Scalar { dimension }`
    /// to emit per component. Declaring the codomain is therefore sufficient —
    /// no runtime change is needed anywhere.
    ///
    /// Drives the exact production composition
    /// `sampled_rotation_field(rotation_sf_from_curl(&curl_sf))`, and asserts:
    ///
    ///   - domain `Point3<Length>`, codomain `Vector3<Angle>` (NOT
    ///     `dimensionless_scalar` — THIS assertion is what encodes the ruling;
    ///     the sibling divergence/gradient/curl wrappers all declare
    ///     dimensionless codomains, and `curl` must stay that way);
    ///   - `source == FieldSourceKind::Sampled`;
    ///   - `data` is the curl input halved element-wise, BIT-EXACTLY.
    ///
    /// Bit-exactness is asserted at 0 ULP with `assert_eq!` on f64 rather than
    /// with a tolerance, and that is a deliberate numeric-premise claim, not
    /// laziness: IEEE-754 division by 2.0 only decrements the exponent, so it
    /// is exact for every normal operand (subnormal underflow is unreachable at
    /// physical strain magnitudes). The fixture values are 3.0 / 5.0 / 7.0 —
    /// deliberately NOT powers of two — so a halving bug cannot hide behind a
    /// coincidental exact result.
    ///
    /// The second half asserts every grid-metadata field survives the derive
    /// unchanged (only `data` and `name` may differ), which is what lets the
    /// rotation channel share the curl channel's Regular3D grid with no extra
    /// BVH resample pass.
    ///
    /// RED: neither `sampled_rotation_field` nor `rotation_sf_from_curl`
    /// exists yet, so this does not compile until step-4.
    #[test]
    fn sampled_rotation_field_declares_angle_codomain_and_halves_curl() {
        use reify_ir::{FieldSourceKind, InterpolationKind, SampledField, SampledGridKind, Value};
        use std::sync::atomic::AtomicBool;

        // stride-3 (one vector per node), 2 nodes on a Regular1D grid.
        let curl_sf = SampledField {
            name: "curl".to_string(),
            kind: SampledGridKind::Regular1D,
            bounds_min: vec![0.0],
            bounds_max: vec![1.0],
            spacing: vec![1.0],
            axis_grids: vec![vec![0.0, 1.0]],
            interpolation: InterpolationKind::Linear,
            data: vec![3.0, 5.0, 7.0, -3.0, -5.0, -7.0],
            oob_emitted: AtomicBool::new(false),
        };

        let rot_sf = super::rotation_sf_from_curl(&curl_sf);
        let value = super::sampled_rotation_field(rot_sf);

        let Value::Field {
            domain_type,
            codomain_type,
            source,
            lambda,
        } = value
        else {
            panic!("expected Value::Field")
        };

        assert_eq!(
            domain_type,
            reify_core::Type::point3(reify_core::Type::length()),
            "rotation field domain must be Point3<Length>"
        );
        assert_eq!(
            codomain_type,
            reify_core::Type::vec3(reify_core::Type::angle()),
            "rotation field codomain must be Vector3<Angle> — this is ruling \
             #6164's designated crossing, NOT vec3(dimensionless_scalar()) like \
             the sibling divergence/gradient/curl wrappers"
        );
        assert_eq!(
            source,
            FieldSourceKind::Sampled,
            "rotation field must be source Sampled"
        );

        let Value::SampledField(out) = lambda.as_ref() else {
            panic!("expected lambda to be a Value::SampledField, got {lambda:?}")
        };

        // Bit-exact halving at 0 ULP, rename, and verbatim grid metadata.
        // Do NOT soften this to a tolerance.
        super::assert_rotation_is_half_of(out, &curl_sf, "wrapper");
        assert_eq!(
            out.data,
            vec![1.5, 2.5, 3.5, -1.5, -2.5, -3.5],
            "sanity: literal expected halves of the non-power-of-two fixture"
        );
    }

    /// Task #6183 σ: the `shear_angles` channel on a hand-computable PATCH
    /// fixture. A linear displacement u = A·x has
    /// ∇u ≡ A at every node, so the Voigt engineering shears are known in closed
    /// form: (γ_yz, γ_zx, γ_xy) = (A12+A21, A20+A02, A01+A10).
    ///
    /// A's off-diagonals are distinct, non-symmetric degree literals, so an
    /// index swap or an antisymmetric (rotation) leak both fail; its nonzero
    /// diagonal proves the normal strains do not leak.
    ///
    /// Drives the exact production composition, pins the declared
    /// `Vector3<Angle>` codomain, then samples through the PRODUCTION sampler
    /// and compares each ANGLE component against the degree expectation.
    #[test]
    fn shear_angles_patch_fixture_samples_deg_derived_engineering_shears() {
        use reify_core::{DimensionVector, Type};
        use reify_ir::{
            FieldSourceKind, InterpolationKind, SampledField, SampledGridKind, Value, ValueMap,
        };
        use std::sync::atomic::AtomicBool;

        let deg = f64::to_radians;
        #[rustfmt::skip]
        let a: [f64; 9] = [
            1e-3,     deg(0.7), deg(0.6),
            deg(0.8), -3e-4,    deg(0.2),
            deg(0.4), deg(0.3), -3e-4,
        ];
        // Regular3D on [0,1]^3, 2 nodes per axis: ∇u ≡ A at all 8 nodes.
        let grad_sf = SampledField {
            name: "gradient".to_string(),
            kind: SampledGridKind::Regular3D,
            bounds_min: vec![0.0; 3],
            bounds_max: vec![1.0; 3],
            spacing: vec![1.0; 3],
            axis_grids: vec![vec![0.0, 1.0]; 3],
            interpolation: InterpolationKind::Linear,
            data: a.repeat(8),
            oob_emitted: AtomicBool::new(false),
        };

        let shear_sf = super::shear_angles_sf_from_gradient(&grad_sf)
            .expect("patch fixture gradient is stride-9");
        let value = super::sampled_shear_angles_field(shear_sf);
        let Value::Field {
            domain_type,
            codomain_type,
            source,
            lambda,
        } = value
        else {
            panic!("expected Value::Field")
        };
        assert_eq!(domain_type, Type::point3(Type::length()));
        assert_eq!(
            codomain_type,
            Type::vec3(Type::angle()),
            "shear_angles codomain must be Vector3<Angle> (a named crossing), \
             NOT vec3(dimensionless_scalar())"
        );
        assert_eq!(source, FieldSourceKind::Sampled);
        let Value::SampledField(out) = lambda.as_ref() else {
            panic!("expected lambda to be a Value::SampledField, got {lambda:?}")
        };
        super::assert_shear_angles_project_gradient(out, &grad_sf, "wrapper");

        let length = |m: f64| Value::Scalar {
            si_value: m,
            dimension: DimensionVector::LENGTH,
        };
        let probe = Value::Point(vec![length(0.25), length(0.5), length(0.75)]);
        let empty = ValueMap::new();
        let sampled = reify_expr::sampled::sample_at_point(
            out,
            &probe,
            &codomain_type,
            &reify_expr::EvalContext::simple(&empty),
        );
        let Value::Vector(components) = &sampled else {
            panic!("expected a sampled Value::Vector, got {sampled:?}")
        };
        // Tolerance basis: trilinear interpolation reproduces constant nodal
        // data by partition of unity (≤~8 ULP), plus ≤2 ULP for the rad→deg
        // round trip — orders of magnitude inside 1e-12 relative.
        let expected_deg = [0.5, 1.0, 1.5];
        assert_eq!(components.len(), expected_deg.len(), "Voigt shear arity");
        for (i, (component, want)) in components.iter().zip(expected_deg).enumerate() {
            let Value::Scalar {
                si_value,
                dimension,
            } = component
            else {
                panic!("component {i} must be an ANGLE-dimensioned Scalar, got {component:?}")
            };
            assert_eq!(
                *dimension,
                DimensionVector::ANGLE,
                "component {i} dimension"
            );
            let got = si_value.to_degrees();
            assert!(
                (got - want).abs() <= 1e-12 * want,
                "component {i}: expected {want}°, got {got}°"
            );
        }
    }

    /// A gradient slab that is not stride-9 yields `None` rather than a
    /// mis-strided projection or a panic: a decoded cache record can carry one,
    /// and nothing upstream of the derive checks its stride.
    #[test]
    fn shear_angles_derive_rejects_non_stride_9_gradient() {
        use reify_ir::{InterpolationKind, SampledField, SampledGridKind};
        use std::sync::atomic::AtomicBool;

        let malformed = |len: usize| SampledField {
            name: "gradient".to_string(),
            kind: SampledGridKind::Regular3D,
            bounds_min: vec![0.0; 3],
            bounds_max: vec![1.0; 3],
            spacing: vec![1.0; 3],
            axis_grids: vec![vec![0.0, 1.0]; 3],
            interpolation: InterpolationKind::Linear,
            data: (0..len).map(|i| i as f64).collect(),
            oob_emitted: AtomicBool::new(false),
        };
        for len in [1, 8, 10, 8 * 9 - 1, 8 * 3] {
            assert!(
                super::shear_angles_sf_from_gradient(&malformed(len)).is_none(),
                "a {len}-value gradient slab is not stride-9 and must yield None"
            );
        }
    }
}
