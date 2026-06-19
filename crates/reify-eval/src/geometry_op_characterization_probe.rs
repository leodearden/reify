//! Cfg-gated cross-crate test seam for the `compile_geometry_op` characterization
//! harness (task #4673, PRD `docs/prds/geometry-op-dispatch-registry.md` DD-4 / §9 L4).
//!
//! `compile_geometry_op` is `pub(crate)` inside the PRIVATE `mod geometry_ops;`
//! (see `lib.rs`), so the L4 characterization suite in
//! `tests/compile_geometry_op_characterization.rs` — a separate integration-test
//! crate — cannot name it directly. This module exposes a faithful 1:1 delegate
//! gated behind `#[cfg(any(test, feature = "test-instrumentation"))]`, reachable
//! from the integration test via the existing self-dev-dep
//! `reify-eval = { path = ".", features = ["test-instrumentation"] }` (so plain
//! `cargo test -p reify-eval` activates it — no `--features` flag needed).
//!
//! ## Why the seam lives here and NOT in `geometry_ops.rs`
//!
//! The L5 leaf (the Axis-3 behavioral refactor of `compile_geometry_op`) edits
//! `geometry_ops.rs`. Placing this probe in a SEPARATE file (plus one gated
//! `pub mod` line in `lib.rs`) keeps L4 and L5 from ever locking/editing the
//! same file — the file-contention motivation behind the dispatch-registry PRD.
//! This file is OUTSIDE `geometry_ops.rs` → zero collision with L5.
//!
//! ## Production-build impact: zero
//!
//! Cfg'd out under default features, so a normal `cargo build`/`cargo check`
//! never compiles it. Mirrors the established Engine test-accessor idiom
//! (`engine_admin.rs::{with_test_kernels_and_registry, test_terminal_handle}`).

use std::collections::HashMap;

/// Faithful 1:1 delegate to the `pub(crate)`
/// [`crate::geometry_ops::compile_geometry_op`].
///
/// The signature is IDENTICAL to the wrapped function (same argument order,
/// types, and return type) so the characterization harness snapshots the
/// byte-identical output of the production code path. This wrapper adds NO logic
/// of its own — it exists solely to make the `pub(crate)` function reachable
/// from the integration-test crate.
#[cfg(any(test, feature = "test-instrumentation"))]
#[allow(clippy::too_many_arguments)]
pub fn compile_geometry_op_probe(
    op: &reify_compiler::CompiledGeometryOp,
    values: &reify_ir::ValueMap,
    step_handles: &[reify_ir::GeometryHandleId],
    functions: &[reify_ir::CompiledFunction],
    meta_map: &HashMap<String, HashMap<String, String>>,
    named_steps: &HashMap<String, reify_ir::KernelHandle>,
    diagnostics: &mut Vec<reify_core::Diagnostic>,
) -> Result<reify_ir::GeometryOp, String> {
    crate::geometry_ops::compile_geometry_op(
        op,
        values,
        step_handles,
        functions,
        meta_map,
        named_steps,
        diagnostics,
    )
}
