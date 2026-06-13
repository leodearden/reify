//! Integration harness + gates for the unified build-DAG geometry-path
//! executors (task 4358 ε).
//!
//! δ (task 4357) landed `run_unified_pass` as a PURE planner and wired
//! `Engine::build()` to forward its diagnostics under
//! [`BuildScheduler::UnifiedDag`] (proven byte-preserving on acyclic modules by
//! `tests/unified_dag_cycle_contract.rs`). ε wires the schedule onto three
//! geometry-path executors (realization / selector-query / constraint), retires
//! the frozen pre-geometry `constraint_results` ("C7"), and lands the
//! auto-constraint guard decline — all behind the same scheduler flag.
//!
//! This file mirrors the `build_under` pattern from
//! `tests/unified_dag_cycle_contract.rs`, but the ε tests assert on geometry
//! ops, constraint verdicts, and diagnostics, so the shared helpers return the
//! FULL [`BuildResult`] (not just projected diagnostic triples). The scheduler
//! is selected through the deterministic `Engine::set_build_scheduler` test seam
//! (a `#[cfg(any(test, feature = "test-instrumentation"))]` setter reached via
//! the self-dev-dep with `test-instrumentation` enabled — see
//! `crates/reify-eval/Cargo.toml`), so these tests stay parallel-safe and
//! independent of the `unified-dag` cargo feature.
//!
//! The mock kernel's `with_query_result` / bbox / volume builders let a
//! geometry-backed constraint reach a DEFINITE verdict without OCCT; the
//! OCCT-dependent headline e2e tests (verdict-FLIP / volume-≠-all-fillet) are
//! owned by η, not ε.

// The shared `build_*` helpers below are consumed incrementally as the ε steps
// land their RED integration tests (steps 5/7/9/11). Until every helper has a
// caller, an unused helper would trip `-D warnings`; this scaffolding allow is
// intentional and is the prerequisite (`pre-1`) deliverable.
#![allow(dead_code)]

use reify_constraints::SimpleConstraintChecker;
use reify_eval::{BuildResult, BuildScheduler, Engine};
use reify_ir::{ExportFormat, GeometryKernel};
use reify_test_support::{MockGeometryKernel, compile_source};

/// Compile `source`, build it on a FRESH engine under the given `scheduler`
/// with the supplied `kernel`, and return the full [`BuildResult`]
/// (`values`, `constraint_results`, `geometry_output`, `diagnostics`).
///
/// A fresh engine per call guarantees the cold-start `eval()` path runs (which
/// populates `eval_state.trace_map` that `run_unified_pass` consumes); a second
/// build on the same engine would hit the `eval_cached` path.
///
/// The `kernel` is taken by `Box<dyn GeometryKernel>` so callers can pass a
/// `MockGeometryKernel` pre-seeded with `with_query_result` / `with_bbox_result`
/// / `with_volume_result` replies (the ε constraint tests) OR the real
/// eval-test kernel.
pub fn build_with_kernel(
    source: &str,
    scheduler: BuildScheduler,
    kernel: Box<dyn GeometryKernel>,
) -> BuildResult {
    let compiled = compile_source(source);
    let mut engine = Engine::new(Box::new(SimpleConstraintChecker), Some(kernel));
    engine.set_build_scheduler(scheduler);
    engine.build(&compiled, ExportFormat::Step)
}

/// Convenience over [`build_with_kernel`] using a default (unseeded)
/// [`MockGeometryKernel`] — for tests that only inspect recorded geometry ops
/// or diagnostics and need no canned query replies.
pub fn build_under(source: &str, scheduler: BuildScheduler) -> BuildResult {
    build_with_kernel(source, scheduler, Box::new(MockGeometryKernel::new()))
}
