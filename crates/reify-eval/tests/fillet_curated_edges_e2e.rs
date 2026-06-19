//! Unified-only OCCT acceptance e2e for curated fillet (η; re-homed from 3205 per PRD D6).
//!
//! Under `BuildScheduler::UnifiedDag` with a real OCCT kernel, a curated
//! `fillet(b, edges_at_height(b, 7.5mm, 0.1mm), 2mm)` must:
//!
//! - Dispatch with **4 curated edges** recorded in the kernel-op stream.
//! - Produce a non-Undef Solid with a **finite** `Scalar<Volume>`.
//! - Produce a volume **distinct** from both the original box and an all-edges fillet
//!   (absolute ε = 1e-10 m³ = 0.1 mm³; real differences are tens of mm³).
//!
//! **Geometry:** `box(w, h, d)` is origin-centered. For `box(20mm, 10mm, 15mm)` the
//! top face is at z = +7.5 mm (= depth/2), so `edges_at_height(b, 7.5mm, 0.1mm)`
//! selects exactly the 4 horizontal top edges. The task's literal `15mm` height on a
//! 15mm-deep box would be above the box top (z = 15mm > 7.5mm) and select 0 edges.
//!
//! **Volume fallback:** The engine-level `volume(f)` DSL cell may return `Undef` for a
//! Modify result under the current unified-DAG path (the engine's geometry-query
//! dispatch relies on `named_steps` being populated in a specific way for Modify ops).
//! The `RecordingKernel` therefore caches volumes directly — immediately after each
//! successful `execute`/`execute_with_history` call — bypassing the engine's dispatch
//! path entirely. This is the documented fallback (plan design decision 3).
//!
//! **Gate:** `#[cfg_attr(not(feature = "unified-dag"), ignore)]` — curated fillet
//! dispatch and volume-distinctness are unified-only; these assertions fail on the legacy
//! default. Run with:
//! `cargo test -p reify-eval --features unified-dag fillet_curated_edges_3205_e2e`

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use reify_constraints::SimpleConstraintChecker;
use reify_eval::{BuildScheduler, Engine};
use reify_ir::{
    AttributeHistory, ExportError, ExportFormat, ExportOptions, ExportWarning, GeometryError,
    GeometryHandle, GeometryHandleId, GeometryKernel, GeometryOp, GeometryQuery,
    KernelAttributeHook, Mesh, QueryError, SampledField, TessError, Value,
};
use reify_test_support::{compile_source, errors_only};

#[test]
#[cfg_attr(not(feature = "unified-dag"), ignore)]
fn fillet_curated_edges_3205_e2e() {
    if !reify_kernel_occt::OCCT_AVAILABLE {
        eprintln!("skipping fillet_curated_edges_3205_e2e: OCCT not available");
        return;
    }

    // box(20mm, 10mm, 15mm) is origin-centered → top face at z = +7.5mm.
    // edges_at_height(b, 7.5mm, 0.1mm) selects exactly the 4 horizontal top edges.
    let source = r#"pub structure S {
    let b = box(20mm, 10mm, 15mm)
    let e = edges_at_height(b, 7.5mm, 0.1mm)
    let f = fillet(b, e, 2mm)
    let fall = fillet(b, 2mm)
    let v_box = volume(b)
    let v_cur = volume(f)
    let v_all = volume(fall)
}"#;

    let compiled = compile_source(source);
    assert!(
        errors_only(&compiled).is_empty(),
        "fixture should compile with no errors, got: {:#?}",
        errors_only(&compiled)
    );

    // Wrap the real OCCT kernel in the transparent RecordingKernel proxy.
    // Capture the shared Arcs BEFORE moving the kernel into the engine.
    let recording_kernel =
        RecordingKernel::new(Box::new(reify_kernel_occt::OcctKernelHandle::spawn()));
    let ops_ref = recording_kernel.ops_ref();
    let op_handles_ref = recording_kernel.op_handles_ref();
    let volumes_ref = recording_kernel.volumes_ref();

    let mut engine = Engine::new(
        Box::new(SimpleConstraintChecker),
        Some(Box::new(recording_kernel)),
    );
    engine.set_build_scheduler(BuildScheduler::UnifiedDag);

    let result = engine.build(&compiled, ExportFormat::Step);

    // ── Assertion (b): exactly one Fillet op with 4 curated edges ──
    // Checked FIRST so a failed fillet (0 or wrong edge count) gives a clear
    // error rather than a confusing "volume = Undef".
    let ops = ops_ref.lock().unwrap();
    let op_handles = op_handles_ref.lock().unwrap();
    let volumes = volumes_ref.lock().unwrap();

    let curated_fillet_indices: Vec<usize> = ops
        .iter()
        .enumerate()
        .filter_map(|(i, op)| {
            if matches!(op, GeometryOp::Fillet { edges, .. } if edges.len() == 4) {
                Some(i)
            } else {
                None
            }
        })
        .collect();
    assert_eq!(
        curated_fillet_indices.len(),
        1,
        "expected exactly one Fillet op with 4 curated edges; all Fillet ops recorded: {:?}; \
         diagnostics: {:#?}",
        ops.iter()
            .filter(|op| matches!(op, GeometryOp::Fillet { .. }))
            .collect::<Vec<_>>(),
        result.diagnostics,
    );

    let curated_fillet_handle = op_handles[curated_fillet_indices[0]];

    // ── Assertion (a): v_cur — curated fillet has a finite, positive volume ──
    // Volume is obtained from the RecordingKernel's direct-query cache, which
    // bypasses the engine's `result.values` dispatch path (the fallback documented
    // in the plan — `volume(f)` on a Modify result may stay Undef in the engine's
    // geometry-query post-process for the current unified-DAG implementation).
    let v_cur_si = *volumes.get(&curated_fillet_handle).unwrap_or_else(|| {
        panic!(
            "curated fillet volume must be a finite, positive m³ value cached by RecordingKernel; \
             all cached volumes: {:?}; diagnostics: {:#?}",
            volumes, result.diagnostics
        )
    });
    assert!(
        v_cur_si.is_finite() && v_cur_si > 0.0,
        "curated fillet volume must be finite and positive, got {v_cur_si}"
    );

    // ── Assertion (c): volume distinctness ──
    // v_cur ≠ v_box: curated fillet removed material from the box.
    // v_cur ≠ v_all: curated (4 edges) fillet removed less than all-edges fillet.
    // Non-equality threshold: 1e-10 m³ = 0.1 mm³ — far below real differences
    // (~tens of mm³) and far above float noise.
    let box_idx = ops
        .iter()
        .position(|op| matches!(op, GeometryOp::Box { .. }))
        .expect("Box op must be recorded by RecordingKernel");
    let v_box_si = *volumes
        .get(&op_handles[box_idx])
        .expect("box volume must be cached by RecordingKernel");

    // Distinguish the all-edges fillet from the curated one by edge count: the curated
    // fillet has exactly 4 edges (verified above); the back-compat 2-arg `fillet(b, 2mm)`
    // lowers to `edges.is_empty()` today, but if lowering ever changes to enumerate all
    // edges explicitly the count would still be != 4.  Matching `edges.len() != 4` is
    // therefore more stable than matching `edges.is_empty()` (which would silently miss
    // a re-encoded all-edges op and cause an opaque `.expect()` panic).
    let all_fillet_idx = ops
        .iter()
        .position(|op| matches!(op, GeometryOp::Fillet { edges, .. } if edges.len() != 4))
        .expect(
            "all-edges Fillet op (edges.len() != 4; today this is edges.is_empty() from the \
             back-compat 'fillet(b, r)' lowering — if lowering changes to enumerate edges \
             explicitly this check still holds) must be recorded",
        );
    let v_all_si = *volumes
        .get(&op_handles[all_fillet_idx])
        .expect("all-edges fillet volume must be cached by RecordingKernel");

    const EPSILON: f64 = 1e-10; // 0.1 mm³ in m³
    // Physical invariants (directional) — these subsume the original abs-difference
    // distinctness checks and also catch regressions that *increase* volume or that
    // swap the curated/all-edges results:
    //
    //   v_cur < v_box  — curated fillet removes material from the box.
    //   v_all < v_cur  — all-edges fillet removes MORE material than the 4-edge curated one.
    assert!(
        v_cur_si < v_box_si - EPSILON,
        "curated fillet must remove material from box (v_cur < v_box - ε): \
         v_cur={v_cur_si:.15e}, v_box={v_box_si:.15e}"
    );
    assert!(
        v_all_si < v_cur_si - EPSILON,
        "all-edges fillet must remove more material than curated (v_all < v_cur - ε): \
         v_all={v_all_si:.15e}, v_cur={v_cur_si:.15e}"
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// RecordingKernel: transparent GeometryKernel proxy that captures executed ops,
// their result handles, and — via immediate post-execute kernel query — their
// volumes. The volume cache is the fallback for when `result.values["v_cur"]`
// returns Undef for Modify-op results under the current unified-DAG path.
// ─────────────────────────────────────────────────────────────────────────────

/// A transparent [`GeometryKernel`] proxy that records every SUCCESSFUL
/// [`GeometryOp`] dispatched through [`execute`](GeometryKernel::execute) and
/// [`execute_with_history`](GeometryKernel::execute_with_history), forwarding
/// ALL calls to the inner kernel unchanged.
///
/// **Ops are recorded only on success** (after the delegate call returns `Ok`).
/// Failed ops (kernel error) are NOT recorded — this keeps `ops` and `op_handles`
/// in sync with the realizations that actually produced a valid handle.
///
/// **Volume caching:** immediately after a successful execute, the proxy queries
/// `Volume(handle.id())` on the inner kernel and caches the result. This lets the
/// test use real OCCT volumes for ALL realized geometry (box, curated fillet,
/// all-edges fillet) without going through the engine's `result.values` post-process,
/// which may leave Modify-op volume cells at Undef.
///
/// The curated `Fillet` op may dispatch through either path under the unified-DAG
/// executor (cf. `topology_attribute_local_features_e2e.rs` routing fillet through
/// `execute_with_history`), so **both** paths record + cache.
///
/// Clone the shared [`Arc`]s via [`ops_ref`](Self::ops_ref) /
/// [`op_handles_ref`](Self::op_handles_ref) / [`volumes_ref`](Self::volumes_ref)
/// **before** moving `self` into `Box<dyn GeometryKernel>` to retain visibility
/// after the move.
struct RecordingKernel {
    inner: Box<dyn GeometryKernel>,
    /// Successfully-executed ops (parallel to `op_handles`).
    ops: Arc<Mutex<Vec<GeometryOp>>>,
    /// Handle ID for each successfully-executed op (parallel to `ops`).
    op_handles: Arc<Mutex<Vec<GeometryHandleId>>>,
    /// Cached volumes (m³) for each handle that had a successful, finite, positive
    /// `GeometryQuery::Volume` response from the inner kernel.
    volumes: Arc<Mutex<HashMap<GeometryHandleId, f64>>>,
}

impl RecordingKernel {
    /// Wrap `inner` in a recording proxy with empty op log, handle list, and volume cache.
    fn new(inner: Box<dyn GeometryKernel>) -> Self {
        Self {
            inner,
            ops: Arc::new(Mutex::new(Vec::new())),
            op_handles: Arc::new(Mutex::new(Vec::new())),
            volumes: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Clone the shared op-log [`Arc`]. Must be called **before** moving `self`.
    fn ops_ref(&self) -> Arc<Mutex<Vec<GeometryOp>>> {
        Arc::clone(&self.ops)
    }

    /// Clone the shared handle-list [`Arc`] (parallel to `ops`). Must be called
    /// **before** moving `self`.
    fn op_handles_ref(&self) -> Arc<Mutex<Vec<GeometryHandleId>>> {
        Arc::clone(&self.op_handles)
    }

    /// Clone the shared volume-cache [`Arc`]. Must be called **before** moving `self`.
    fn volumes_ref(&self) -> Arc<Mutex<HashMap<GeometryHandleId, f64>>> {
        Arc::clone(&self.volumes)
    }

    /// Record a successful op + its handle, and cache its volume from the inner kernel.
    ///
    /// Called only after `Ok(handle)` — failed ops are NOT recorded.
    fn record_success(&mut self, op: &GeometryOp, handle_id: GeometryHandleId) {
        self.ops.lock().unwrap().push(op.clone());
        self.op_handles.lock().unwrap().push(handle_id);
        // Query the volume immediately while we hold the kernel. The OCCT kernel
        // returns `Value::Real(v)` (m³) for Volume queries (see geometry.rs kernel
        // reply contract). Non-volume-queryable shapes (e.g. Sdf, Voxel) or errors
        // are silently skipped — the volume cache entry is simply absent.
        if let Ok(Value::Real(v)) = self.inner.query(&GeometryQuery::Volume(handle_id))
            && v.is_finite() && v > 0.0
        {
            self.volumes.lock().unwrap().insert(handle_id, v);
        }
    }
}

impl GeometryKernel for RecordingKernel {
    fn execute(&mut self, op: &GeometryOp) -> Result<GeometryHandle, GeometryError> {
        let result = self.inner.execute(op)?;
        self.record_success(op, result.id);
        Ok(result)
    }

    fn execute_with_history(
        &mut self,
        op: &GeometryOp,
    ) -> Result<(GeometryHandle, AttributeHistory), GeometryError> {
        let (handle, history) = self.inner.execute_with_history(op)?;
        self.record_success(op, handle.id);
        Ok((handle, history))
    }

    fn query(&self, query: &GeometryQuery) -> Result<Value, QueryError> {
        self.inner.query(query)
    }

    fn query_many(&self, queries: &[GeometryQuery]) -> Result<Vec<Value>, QueryError> {
        self.inner.query_many(queries)
    }

    fn export(
        &self,
        handle: GeometryHandleId,
        format: ExportFormat,
        writer: &mut dyn std::io::Write,
    ) -> Result<(), ExportError> {
        self.inner.export(handle, format, writer)
    }

    fn export_with_options(
        &self,
        handle: GeometryHandleId,
        format: ExportFormat,
        options: &ExportOptions,
        writer: &mut dyn std::io::Write,
    ) -> Result<Vec<ExportWarning>, ExportError> {
        self.inner.export_with_options(handle, format, options, writer)
    }

    fn tessellate(&self, handle: GeometryHandleId, tolerance: f64) -> Result<Mesh, TessError> {
        self.inner.tessellate(handle, tolerance)
    }

    fn extract_edges(
        &mut self,
        handle: GeometryHandleId,
    ) -> Result<Vec<GeometryHandleId>, QueryError> {
        self.inner.extract_edges(handle)
    }

    fn extract_faces(
        &mut self,
        handle: GeometryHandleId,
    ) -> Result<Vec<GeometryHandleId>, QueryError> {
        self.inner.extract_faces(handle)
    }

    fn extract_vertices(
        &mut self,
        handle: GeometryHandleId,
    ) -> Result<Vec<GeometryHandleId>, QueryError> {
        self.inner.extract_vertices(handle)
    }

    fn densify_grid_to_sampled(
        &mut self,
        handle: GeometryHandleId,
    ) -> Result<SampledField, QueryError> {
        self.inner.densify_grid_to_sampled(handle)
    }

    fn execute_split(
        &mut self,
        op: &GeometryOp,
    ) -> Result<Vec<GeometryHandleId>, GeometryError> {
        self.inner.execute_split(op)
    }

    fn make_compound(
        &mut self,
        handles: &[GeometryHandleId],
    ) -> Result<GeometryHandle, GeometryError> {
        self.inner.make_compound(handles)
    }

    fn ingest_mesh(&mut self, mesh: &Mesh) -> Result<GeometryHandle, GeometryError> {
        self.inner.ingest_mesh(mesh)
    }

    fn attribute_hook(&self) -> Option<&dyn KernelAttributeHook> {
        self.inner.attribute_hook()
    }

    fn measure_mesh_deviation(
        &self,
        handle: GeometryHandleId,
        mesh: &Mesh,
    ) -> Option<f64> {
        self.inner.measure_mesh_deviation(handle, mesh)
    }
}
