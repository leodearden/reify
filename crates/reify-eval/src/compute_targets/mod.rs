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
pub mod buckling;
pub mod buckling_multi_case;
pub mod elastic_static;
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
