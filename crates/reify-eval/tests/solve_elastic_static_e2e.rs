//! End-to-end integration tests for `fn solve_elastic_static` @optimized →
//! ComputeNode → trampoline pipeline (PRD §8 task η,
//! docs/prds/v0_3/compute-node-contract.md).
//!
//! Steps:
//!   step-3/4  — API surface pin + module skeleton
//!   step-5/6  — ComputeNode-insertion assertion + smoke .ri
//!   step-7/8  — cantilever stress magnitude assertion + real FEA impl
//!   step-9/10 — cache-hit assertion + doc comments

use reify_eval::{CancellationHandle, ComputeFn, ComputeOutcome, RealizationReadHandle};
use reify_test_support::{make_simple_engine, parse_and_compile_with_stdlib};
use reify_core::{Severity, ValueCellId};
use reify_ir::{OpaqueState, Value};

// ── step-3: RED — API surface pin ────────────────────────────────────────────
//
// Compile-time test: coerce
//   `reify_eval::compute_targets::elastic_static::solve_elastic_static_trampoline`
// to `ComputeFn` to pin the cross-crate signature. No runtime assertion —
// compile success is the signal. Expected to fail until step-4 creates the
// `compute_targets` module.

#[allow(dead_code)]
fn _seam_pin() {
    let _f: ComputeFn =
        reify_eval::compute_targets::elastic_static::solve_elastic_static_trampoline;
}

/// Step-3: `register_compute_fns` installs the trampoline under the correct key.
///
/// Constructs `make_simple_engine()`, calls
/// `reify_eval::compute_targets::register_compute_fns(&mut engine)`, asserts
/// `engine.compute_dispatch("solver::elastic_static").is_some()`.
///
/// Expected to fail until step-4 creates the `compute_targets` module.
#[test]
fn register_compute_fns_installs_solver_elastic_static() {
    let mut engine = make_simple_engine();
    reify_eval::compute_targets::register_compute_fns(&mut engine);
    assert!(
        engine.compute_dispatch("solver::elastic_static").is_some(),
        "register_compute_fns must install a trampoline under 'solver::elastic_static'"
    );
}
