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
use reify_constraints::SimpleConstraintChecker;
use reify_core::ValueCellId;
use reify_eval::{BuildScheduler, Engine};
use reify_ir::{
    ExportError, ExportFormat, GeometryError, GeometryHandle, GeometryHandleId, GeometryKernel,
    GeometryOp, GeometryQuery, Mesh, QueryError, TessError, Value,
};
use reify_test_support::{MockGeometryKernel, compile_source};

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
// Helper — create a fresh engine with a RecordingKernel wired under `scheduler`.
// Grabs both Arc recorders BEFORE moving `kernel` into the engine so the test
// can inspect them after the build.
// ─────────────────────────────────────────────────────────────────────────────

fn engine_with_recording_kernel(
    scheduler: BuildScheduler,
) -> (
    Engine,
    Arc<Mutex<Vec<GeometryHandleId>>>,
    Arc<Mutex<Vec<Vec<GeometryHandleId>>>>,
) {
    let kernel = RecordingKernel::new();
    let exported = kernel.exported_handles_ref();
    let compounds = kernel.compound_members_ref();
    let mut engine =
        Engine::new(Box::new(SimpleConstraintChecker), Some(Box::new(kernel) as Box<dyn GeometryKernel>));
    engine.set_build_scheduler(scheduler);
    (engine, exported, compounds)
}

// ─────────────────────────────────────────────────────────────────────────────
// step-1 (RED): build_snapshot multi-entity positional export.
//
// build_snapshot currently exports `*step_handles.last()` (the last realization
// handle) without calling make_compound.  For a module with ≥2 product structures
// (`MULTI_ENTITY_EXPORT_SRC`), this is WRONG — the exported handle is the second
// box body, not a compound of both.  `build()` correctly calls make_compound then
// exports the compound; build_snapshot must do the same.
//
// RED until step-2: the RecordingKernel shows build_snapshot does NOT call
// make_compound, so `compound_members.len()` is 1 after both calls (only build()
// contributed the compound), not 2 as required.
// ─────────────────────────────────────────────────────────────────────────────

/// build_snapshot must assemble the same compound as build() for a multi-entity
/// module.  RED until step-2 fixes the `*step_handles.last()` export bug.
#[test]
fn build_snapshot_multi_entity_export_uses_compound() {
    let compiled = compile_source(MULTI_ENTITY_EXPORT_SRC);
    let (mut engine, exported, compounds) =
        engine_with_recording_kernel(BuildScheduler::UnifiedDag);

    // Cold build — populates eval_state and realization cache.
    // Recorder should show exactly ONE make_compound call (the two-member assembly).
    engine.build(&compiled, ExportFormat::Step);

    let build_compound_count = compounds.lock().unwrap().len();
    assert_eq!(
        build_compound_count, 1,
        "build() must call make_compound once for a 2-entity module; got {} calls",
        build_compound_count,
    );
    let build_members = compounds.lock().unwrap()[0].clone();
    assert_eq!(
        build_members.len(),
        2,
        "build() compound must have 2 members (one per product structure); got {:?}",
        build_members,
    );

    // Warm build_snapshot — must drive the same export path as build().
    // RED assertion: build_snapshot must call make_compound a SECOND time
    // (one additional compound for the snapshot export).
    engine.build_snapshot(&compiled, ExportFormat::Step);

    let snap_compound_count = compounds.lock().unwrap().len();
    assert_eq!(
        snap_compound_count, 2,
        "build_snapshot must call make_compound for a multi-entity module \
         (currently exports `*step_handles.last()` without compound — RED until step-2); \
         compound call count after build+snapshot: {}",
        snap_compound_count,
    );

    // Cross-check: the snapshot compound must have the same MEMBER COUNT as build()'s.
    // NOTE: Handle IDs differ between build() and build_snapshot() (each run allocates fresh
    // handles from the MockGeometryKernel's incrementing counter), so we compare COUNT not
    // exact IDs. The structural property is: build_snapshot assembles a compound of the same
    // arity as build().
    let compounds_locked = compounds.lock().unwrap();
    let snap_members = &compounds_locked[1];
    assert_eq!(
        snap_members.len(), build_members.len(),
        "build_snapshot compound must have the same member count as build(); \
         build_len={}, snapshot_len={}",
        build_members.len(), snap_members.len(),
    );

    // Structural check: the exported handle from build_snapshot must be the compound
    // (NOT one of the member bodies).  The compound handle is created AFTER the member bodies
    // by make_compound, so its ID is strictly greater than both member IDs.
    let exported_locked = exported.lock().unwrap();
    assert_eq!(
        exported_locked.len(),
        2,
        "expected 2 export calls total (one from build(), one from build_snapshot()); \
         got {:?}",
        exported_locked.as_slice(),
    );
    let snap_exported_id = exported_locked[1];
    assert!(
        !snap_members.contains(&snap_exported_id),
        "build_snapshot must export the COMPOUND handle (not a member body); \
         exported={snap_exported_id:?}, compound_members={snap_members:?}",
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// step-3 (RED) / step-4 (GREEN): eval_cached warm Resolution back-prop.
//
// eval_cached's `SolveResult::Solved { .. }` arm (engine_eval.rs) was an
// intentional no-op until θ step-4.  After a cold eval() where the solver
// resolves `x == 10mm`, a subsequent eval_cached() must back-prop:
//   - write x → (0.01 m SI, Determined)
//   - re-evaluate the downstream let y = x + 5mm → (0.015 m SI, Determined)
//
// WARM_AUTO_CONST_LET_SRC uses `Length` (not `Real`) so that DimensionalSolver's
// bounded search space (1e-6, 10.0) converges within FEASIBILITY_THRESHOLD=1e-12.
// `Real` (dimensionless) uses (-1e6, 1e6) default bounds, leaving Nelder-Mead
// ~2e-8 from the target — above the 1e-12 threshold → Infeasible return.
//
// GREEN after step-4: the Solved arm is implemented; both cold eval and
// eval_cached now resolve x = 10mm = 0.01 m (SI).
// ─────────────────────────────────────────────────────────────────────────────

/// eval_cached must back-prop SolveResult::Solved into values/snapshot.
/// GREEN after step-4 implements the Solved arm in engine_eval.rs.
#[test]
fn eval_cached_warm_auto_plus_const_let_back_props() {
    let (engine, result) =
        warm_eval_cached_with_solver(WARM_AUTO_CONST_LET_SRC, BuildScheduler::UnifiedDag);

    let values = &result.eval_result.values;

    // x must be resolved to 10mm = 0.01 m (SI) as Determined.
    // Uses Value::Scalar since DimensionalSolver writes solved Length params
    // back as Scalar { si_value, dimension: LENGTH }.
    let x_id = ValueCellId::new("WarmAutoConstLet", "x");
    let x_val = values
        .get(&x_id)
        .unwrap_or_else(|| panic!("x must be in the values map after eval_cached; map has {} entries", values.len()));
    assert!(
        matches!(x_val, Value::Scalar { si_value, .. } if (*si_value - 0.01).abs() < 1e-9),
        "eval_cached back-prop: WarmAutoConstLet.x must resolve to 0.01 m (10mm, Determined); got {:?}",
        x_val,
    );

    // y must be re-evaluated to 15mm = 0.015 m (= x + 5mm = 10mm + 5mm).
    let y_id = ValueCellId::new("WarmAutoConstLet", "y");
    let y_val = values
        .get(&y_id)
        .unwrap_or_else(|| panic!("y must be in the values map after eval_cached; map has {} entries", values.len()));
    assert!(
        matches!(y_val, Value::Scalar { si_value, .. } if (*si_value - 0.015).abs() < 1e-9),
        "eval_cached back-prop: WarmAutoConstLet.y must resolve to 0.015 m (15mm = x + 5mm); got {:?}",
        y_val,
    );

    // Snapshot must also record x as (0.01 m, Determined).
    let snap = engine
        .snapshot()
        .expect("snapshot must be set after eval_cached()");
    let (snap_x, x_det) = snap.values.get(&x_id).unwrap_or_else(|| {
        panic!("x must be in snapshot after eval_cached; keys: {:?}", snap.values.iter().map(|(k,_)| format!("{k}")).collect::<Vec<_>>())
    });
    assert!(
        matches!(snap_x, Value::Scalar { si_value, .. } if (*si_value - 0.01).abs() < 1e-9),
        "snapshot.x must be 0.01 m (10mm) after back-prop; got {:?}", snap_x,
    );
    assert_eq!(
        *x_det, reify_ir::DeterminacyState::Determined,
        "snapshot.x must be Determined after back-prop; got {:?}", x_det,
    );
}
