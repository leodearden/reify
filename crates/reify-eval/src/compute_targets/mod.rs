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
