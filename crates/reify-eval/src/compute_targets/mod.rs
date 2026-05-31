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

pub mod buckling;
pub mod elastic_static;
pub mod form_find;

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

/// Flatten per-node stress tensors `[[f64;3];3]` into a stride-9 row-major
/// `Vec<f64>`.
///
/// Layout per node: `σ_xx, σ_xy, σ_xz, σ_yx, σ_yy, σ_yz, σ_zx, σ_zy, σ_zz`.
/// Shared by the elastic-static and buckling trampolines so the packing
/// convention is defined in exactly one place.
pub(crate) fn flatten_nodal_stress(nodal_stress: &[[[f64; 3]; 3]]) -> Vec<f64> {
    nodal_stress
        .iter()
        .flat_map(|s| {
            [
                s[0][0], s[0][1], s[0][2],
                s[1][0], s[1][1], s[1][2],
                s[2][0], s[2][1], s[2][2],
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
        domain_type:   reify_core::Type::point3(reify_core::Type::length()),
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
        domain_type:   reify_core::Type::point3(reify_core::Type::length()),
        codomain_type: reify_core::Type::tensor(2, 3, reify_core::Type::Scalar {
            dimension: DimensionVector::PRESSURE,
        }),
        source: FieldSourceKind::Sampled,
        lambda: Arc::new(Value::SampledField(sf)),
    }
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
}
