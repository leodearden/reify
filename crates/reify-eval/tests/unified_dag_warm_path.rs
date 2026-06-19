//! θ (task 4361) warm-path test binary.
//!
//! Tests the four θ gaps:
//!   1. `build_snapshot` multi-entity positional export fix (RecordingKernel-observable).
//!   2. `build_snapshot` + `tessellate_from_values` driver routing through `run_unified_pass`.
//!   3. `eval_cached` warm `SolveResult::Solved` back-prop (`let y = auto_x + N`).
//!   4. Concurrent path re-verify + serialization invariant.
//!
//! The shared ζ harness (`common/differential.rs`) is `#[path]`-included so
//! this binary reuses all corpus helpers with zero edits to existing shared files.
//! `RecordingKernel` is defined test-locally (NOT in mocks.rs) to keep the
//! blast radius minimal, following the design decision in the plan.
#![allow(dead_code, unused_imports)]

#[path = "common/differential.rs"]
mod differential;

use std::sync::{Arc, Mutex};

use differential::{
    MULTI_ENTITY_EXPORT_SRC, WARM_AUTO_CONST_LET_SRC, build_with_kernel, fresh_engine_with_solver,
    warm_eval_cached_with_solver,
};
use reify_eval::BuildScheduler;
use reify_ir::{
    ExportError, ExportFormat, GeometryError, GeometryHandle, GeometryHandleId, GeometryKernel,
    GeometryOp, GeometryQuery, Mesh, QueryError, TessError, Value,
};
use reify_test_support::MockGeometryKernel;

// ─────────────────────────────────────────────────────────────────────────────
// RecordingKernel — a test-local wrapper around MockGeometryKernel that records
// which handles are passed to export() and make_compound().
//
// Pattern mirrors MockGeometryKernel's `tessellate_tolerances` recorder:
// grab Arc handles BEFORE moving the kernel into the engine, then inspect
// them after the build call returns.
//
// NOT in mocks.rs: keeps the shared crate blast-radius zero (plan design
// decision: "observe the build_snapshot export fix with a test-local
// RecordingKernel wrapper").
// ─────────────────────────────────────────────────────────────────────────────

/// A geometry kernel that wraps [`MockGeometryKernel`] and records:
/// - every `GeometryHandleId` passed to `export()` in `exported_handles`
/// - every member list `&[GeometryHandleId]` passed to `make_compound()` in
///   `compound_members`
///
/// All other operations delegate to the inner mock unchanged.  Grab the
/// `Arc<Mutex<>>` recorders via `exported_handles_ref()` /
/// `compound_members_ref()` BEFORE moving this kernel into an `Engine`.
pub struct RecordingKernel {
    inner: MockGeometryKernel,
    /// Handles passed to `export()`, in invocation order.
    exported_handles: Arc<Mutex<Vec<GeometryHandleId>>>,
    /// Member lists passed to `make_compound()`, in invocation order.
    compound_members: Arc<Mutex<Vec<Vec<GeometryHandleId>>>>,
}

impl RecordingKernel {
    pub fn new() -> Self {
        Self {
            inner: MockGeometryKernel::new(),
            exported_handles: Arc::new(Mutex::new(Vec::new())),
            compound_members: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// Returns an [`Arc`] to the exported-handles recorder.  Grab this BEFORE
    /// moving `self` into the engine so the test can inspect it after the build.
    pub fn exported_handles_ref(&self) -> Arc<Mutex<Vec<GeometryHandleId>>> {
        Arc::clone(&self.exported_handles)
    }

    /// Returns an [`Arc`] to the compound-members recorder.  Grab this BEFORE
    /// moving `self` into the engine so the test can inspect it after the build.
    pub fn compound_members_ref(&self) -> Arc<Mutex<Vec<Vec<GeometryHandleId>>>> {
        Arc::clone(&self.compound_members)
    }
}

impl GeometryKernel for RecordingKernel {
    fn execute(&mut self, op: &GeometryOp) -> Result<GeometryHandle, GeometryError> {
        self.inner.execute(op)
    }

    fn query(&self, query: &GeometryQuery) -> Result<Value, QueryError> {
        self.inner.query(query)
    }

    fn export(
        &self,
        handle: GeometryHandleId,
        format: ExportFormat,
        writer: &mut dyn std::io::Write,
    ) -> Result<(), ExportError> {
        self.exported_handles.lock().unwrap().push(handle);
        self.inner.export(handle, format, writer)
    }

    fn tessellate(&self, handle: GeometryHandleId, tolerance: f64) -> Result<Mesh, TessError> {
        self.inner.tessellate(handle, tolerance)
    }

    fn make_compound(
        &mut self,
        handles: &[GeometryHandleId],
    ) -> Result<GeometryHandle, GeometryError> {
        self.compound_members
            .lock()
            .unwrap()
            .push(handles.to_vec());
        self.inner.make_compound(handles)
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests are added in subsequent steps (step-1 through step-7).
// This file compiles as the prereq-1 scaffolding, keeping all items alive
// via the `#![allow(dead_code)]` in common/differential.rs.
//
// IMPORTANT: the imports above (MULTI_ENTITY_EXPORT_SRC, WARM_AUTO_CONST_LET_SRC,
// build_with_kernel, fresh_engine_with_solver, warm_eval_cached_with_solver,
// RecordingKernel) are referenced by the step tests; they are live from
// prereq-1 onward.
// ─────────────────────────────────────────────────────────────────────────────
