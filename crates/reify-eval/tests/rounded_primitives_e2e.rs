//! End-to-end OCCT numeric-signal tests for `rounded_box` / `rounded_rect`
//! (task #5201) — the boolean-union-compose convenience geometry constructors
//! pinned structurally by `crates/reify-compiler/tests/rounded_primitives_tests.rs`.
//!
//! Compiles the exact boolean-compose lowering through the full
//! source → parse → compile → Engine (real OCCT) pipeline and asserts:
//!
//! - (a) `volume(rounded_box(480mm,260mm,80mm,25mm))` ≈ `[w·d − (4−π)·r²]·h` —
//!   EXACT (not approximate) because the compose corners are exact
//!   quarter-cylinders and OCCT's analytic-volume integration clears relative
//!   1e-6 on quadric B-reps; asserted at relative 1e-6.
//! - (b) that volume is strictly less than `volume(box(480mm,260mm,80mm))`
//!   (the sharp box) by ≈ `(4−π)·r²·h` ≈ 4.29e-5 m³ — four orders of
//!   magnitude above the epsilon used for the inequality, so the check is
//!   robust.
//! - (c) `volume(extrude(rounded_rect(480mm,260mm,25mm), 80mm))` matches
//!   `volume(rounded_box(480mm,260mm,80mm,25mm))` within relative 1e-6 —
//!   cross-checking the 2D coplanar-union extrudability of `rounded_rect`
//!   (the documented main risk of that lowering).
//! - (d) `bounding_box(rounded_box(...))` is symmetric about z and spans
//!   ±40mm (height 80mm, centred — same anchor as `box`).
//!
//! **Volume/bbox source of truth.** The engine-level `volume(...)` /
//! `bounding_box(...)` DSL cells fold via a `named_steps`-driven post-process
//! that is proven for Primitive/Transform results
//! (`geometry_query_kernel_dispatch.rs`) but may leave a
//! `Boolean{Union}`/`Extrude` result's cell at `Undef` under the current
//! unified-DAG path — the same caveat `fillet_curated_edges_e2e.rs` documents
//! for Modify results. This test therefore wraps the real OCCT kernel in a
//! `RecordingKernel` proxy that caches BOTH the volume and the bounding-box
//! z-span for every successfully-executed op immediately after `execute`,
//! bypassing `result.values` entirely; the last op recorded for a
//! single-realization build IS that build's realization root (the compiler
//! always emits it last — pinned by `rounded_primitives_tests.rs`), so no
//! disambiguation is needed. The DSL volume/bbox cells are still included in
//! each fixture as the designer-observable signal and to prove they at least
//! compile and build without error.
//!
//! Self-skips (non-failing) when `reify_kernel_occt::OCCT_AVAILABLE` is false.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use reify_constraints::SimpleConstraintChecker;
use reify_core::Severity;
use reify_eval::{BuildScheduler, Engine};
use reify_ir::{
    AttributeHistory, ExportError, ExportFormat, ExportOptions, ExportWarning, GeometryError,
    GeometryHandle, GeometryHandleId, GeometryKernel, GeometryOp, GeometryQuery,
    KernelAttributeHook, Mesh, QueryError, SampledField, TessError, Value,
};
use reify_test_support::{compile_source, errors_only};

// Litter-tray dimensions — the task's observable-signal example
// (`examples/litter_tray.ri`).
const W: f64 = 0.480; // 480mm
const D: f64 = 0.260; // 260mm
const H: f64 = 0.080; // 80mm
const R: f64 = 0.025; // 25mm corner radius

/// Parse z from the JSON-encoded bounding box returned by
/// `GeometryQuery::BoundingBox` (format:
/// `{"xmin":<f>,"ymin":<f>,"zmin":<f>,"xmax":<f>,"ymax":<f>,"zmax":<f>}`).
/// Mirrors `centered_primitives_e2e.rs::parse_bbox_z`.
fn parse_bbox_z(s: &str) -> (f64, f64) {
    let mut zmin = f64::NAN;
    let mut zmax = f64::NAN;
    let trimmed = s.trim_start_matches('{').trim_end_matches('}');
    for pair in trimmed.split(',') {
        let mut parts = pair.splitn(2, ':');
        let key = parts.next().unwrap().trim().trim_matches('"');
        let val: f64 = parts.next().unwrap().trim().parse().unwrap();
        match key {
            "zmin" => zmin = val,
            "zmax" => zmax = val,
            _ => {}
        }
    }
    (zmin, zmax)
}

/// The realization root's op kind, cached volume, cached bbox z-span, and
/// STEP-export length for a single-realization build — captured directly
/// from the `RecordingKernel`, independent of whether the engine's
/// `result.values` cell-fold resolved.
struct RootSignal {
    last_op: GeometryOp,
    volume: f64,
    bbox_z: Option<(f64, f64)>,
    geometry_output_len: usize,
}

/// Compile `source` (a single `structure def S { .. }` with ONE target
/// realization), build it through a real-OCCT `Engine` wrapped in a
/// `RecordingKernel`, and return the last successfully-executed op's kind
/// plus its cached volume/bbox.
///
/// Each call spins up a FRESH `RecordingKernel` + `Engine`, so "last recorded
/// op" is unambiguous: the compiler always emits a realization's root as its
/// final op, so the last entry in the recording IS that build's target
/// geometry — no cross-realization disambiguation is needed.
fn build_and_capture_root(source: &str) -> RootSignal {
    let compiled = compile_source(source);
    assert!(
        errors_only(&compiled).is_empty(),
        "fixture should compile with no error-severity diagnostics, got:\n{:#?}",
        errors_only(&compiled)
    );

    let recording_kernel =
        RecordingKernel::new(Box::new(reify_kernel_occt::OcctKernelHandle::spawn()));
    let ops_ref = recording_kernel.ops_ref();
    let op_handles_ref = recording_kernel.op_handles_ref();
    let volumes_ref = recording_kernel.volumes_ref();
    let bboxes_ref = recording_kernel.bboxes_ref();

    let mut engine = Engine::new(
        Box::new(SimpleConstraintChecker),
        Some(Box::new(recording_kernel)),
    );
    engine.set_build_scheduler(BuildScheduler::UnifiedDag);

    let result = engine.build(&compiled, ExportFormat::Step);
    let build_errors: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        build_errors.is_empty(),
        "unexpected build errors: {:#?}",
        build_errors
    );

    let ops = ops_ref.lock().unwrap();
    let op_handles = op_handles_ref.lock().unwrap();
    let volumes = volumes_ref.lock().unwrap();
    let bboxes = bboxes_ref.lock().unwrap();

    let last_op = ops
        .last()
        .unwrap_or_else(|| panic!("RecordingKernel captured no ops for source:\n{source}"))
        .clone();
    let last_handle = *op_handles
        .last()
        .expect("op_handles parallels ops; non-empty since ops is non-empty");
    let volume = *volumes.get(&last_handle).unwrap_or_else(|| {
        panic!(
            "no cached volume for the realization root's handle ({last_handle:?}); \
             last op: {last_op:?}; all cached volumes: {volumes:?}"
        )
    });
    let bbox_z = bboxes.get(&last_handle).copied();
    let geometry_output_len = result.geometry_output.as_ref().map_or(0, |s| s.len());

    RootSignal {
        last_op,
        volume,
        bbox_z,
        geometry_output_len,
    }
}

// ─── (a)+(b)+(d): rounded_box volume/bbox ──────────────────────────────────

/// `rounded_box(480mm,260mm,80mm,25mm)` realizes (via the boolean-union
/// compose pinned by `rounded_primitives_tests.rs`) to:
///   - a volume matching the closed form `[w·d − (4−π)·r²]·h` (EXACT, since
///     the compose corners are exact quarter-cylinders);
///   - a volume strictly less than the sharp `box(480mm,260mm,80mm)` by
///     ≈(4−π)·r²·h ≈ 4.29e-5 m³;
///   - a bounding box symmetric about z, spanning ±40mm (height 80mm,
///     centred — same anchor as `box`).
#[test]
fn rounded_box_volume_matches_formula_and_less_than_sharp() {
    if !reify_kernel_occt::OCCT_AVAILABLE {
        eprintln!(
            "skipping rounded_box_volume_matches_formula_and_less_than_sharp: OCCT not available"
        );
        return;
    }

    let rb = build_and_capture_root(
        r#"structure def S {
    let rb = rounded_box(480mm, 260mm, 80mm, 25mm)
    let v_rb = volume(rb)
    let bb_rb = bounding_box(rb)
}"#,
    );
    assert!(
        matches!(rb.last_op, GeometryOp::Union { .. }),
        "rounded_box's realization root should be the final Boolean(Union), got {:?}",
        rb.last_op
    );

    let sharp = build_and_capture_root(
        r#"structure def S {
    let sharp = box(480mm, 260mm, 80mm)
    let v_sharp = volume(sharp)
}"#,
    );
    assert!(
        matches!(sharp.last_op, GeometryOp::Box { .. }),
        "the sharp box's realization root should be the Box primitive, got {:?}",
        sharp.last_op
    );

    // (a) closed form: w·d·h − (4−π)·r²·h — EXACT (quarter-cylinder corners).
    let expected_rb = (W * D - (4.0 - std::f64::consts::PI) * R * R) * H;
    let rel_err_rb = (rb.volume - expected_rb).abs() / expected_rb;
    assert!(
        rel_err_rb < 1e-6,
        "rounded_box volume should be {expected_rb:.10e} m³ ([w·d−(4−π)r²]·h), \
         got {:.10e} (rel_err={rel_err_rb:.3e})",
        rb.volume
    );

    // Sanity: the sharp box volume is the plain w·d·h product.
    let expected_sharp = W * D * H;
    let rel_err_sharp = (sharp.volume - expected_sharp).abs() / expected_sharp;
    assert!(
        rel_err_sharp < 1e-6,
        "sharp box volume should be {expected_sharp:.10e} m³ (w·d·h), \
         got {:.10e} (rel_err={rel_err_sharp:.3e})",
        sharp.volume
    );

    // (b) rounded strictly < sharp; Δ ≈ (4−π)·r²·h ≈ 4.29e-5 m³ — four
    // orders of magnitude above this epsilon, so the inequality is robust.
    const EPSILON: f64 = 1e-8;
    assert!(
        rb.volume < sharp.volume - EPSILON,
        "rounded_box volume must be strictly less than the sharp box \
         (v_rb < v_sharp − ε): v_rb={:.10e}, v_sharp={:.10e}",
        rb.volume,
        sharp.volume
    );
    let expected_delta = (4.0 - std::f64::consts::PI) * R * R * H;
    let actual_delta = sharp.volume - rb.volume;
    let rel_err_delta = (actual_delta - expected_delta).abs() / expected_delta;
    assert!(
        rel_err_delta < 1e-6,
        "sharp−rounded volume delta should be {expected_delta:.10e} m³ ((4−π)r²h), \
         got {:.10e} (rel_err={rel_err_delta:.3e})",
        actual_delta
    );

    // (d) bbox(rb) is symmetric about z and spans ±40mm (height 80mm,
    // centred). OCCT's BRepBndLib padding tolerance mirrors
    // `centered_primitives_e2e.rs`.
    let (zmin, zmax) = rb
        .bbox_z
        .expect("bounding_box(rb) should have cached a bbox for the Union root");
    let tol = 1e-6_f64;
    let expected_half = H / 2.0;
    assert!(
        (zmin - (-expected_half)).abs() < tol,
        "rounded_box bbox zmin should be ≈ -{expected_half:.4} m, got {zmin}"
    );
    assert!(
        (zmax - expected_half).abs() < tol,
        "rounded_box bbox zmax should be ≈ +{expected_half:.4} m, got {zmax}"
    );

    // Extra signal: the build actually produced non-empty STEP output.
    assert!(
        rb.geometry_output_len > 0,
        "rounded_box build should produce non-empty STEP geometry output"
    );
}

// ─── (c): rounded_rect → extrude cross-check ───────────────────────────────

/// `extrude(rounded_rect(480mm,260mm,25mm), 80mm)` must match
/// `rounded_box(480mm,260mm,80mm,25mm)`'s volume within relative 1e-6 —
/// validating 2D coplanar-union extrudability, the documented main risk of
/// the `rounded_rect` lowering.
#[test]
fn rounded_rect_extrude_volume_matches_rounded_box() {
    if !reify_kernel_occt::OCCT_AVAILABLE {
        eprintln!("skipping rounded_rect_extrude_volume_matches_rounded_box: OCCT not available");
        return;
    }

    let rb = build_and_capture_root(
        r#"structure def S {
    let rb = rounded_box(480mm, 260mm, 80mm, 25mm)
    let v_rb = volume(rb)
}"#,
    );

    let ext = build_and_capture_root(
        r#"structure def S {
    let rr = rounded_rect(480mm, 260mm, 25mm)
    let ext = extrude(rr, 80mm)
    let v_ext = volume(ext)
}"#,
    );
    assert!(
        matches!(ext.last_op, GeometryOp::Extrude { .. }),
        "extrude(rounded_rect(...), ...)'s realization root should be the Extrude op, got {:?}",
        ext.last_op
    );
    assert!(
        ext.geometry_output_len > 0,
        "extrude(rounded_rect(...)) build should produce non-empty STEP geometry output \
         (2D coplanar-union extrudability signal)"
    );

    let rel_err = (ext.volume - rb.volume).abs() / rb.volume;
    assert!(
        rel_err < 1e-6,
        "extrude(rounded_rect(...), 80mm) volume should match rounded_box(...) volume: \
         v_ext={:.10e}, v_rb={:.10e} (rel_err={rel_err:.3e})",
        ext.volume,
        rb.volume
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// RecordingKernel: transparent GeometryKernel proxy — adapted from
// `fillet_curated_edges_e2e.rs`'s RecordingKernel, extended to ALSO cache
// each successful op's bounding-box z-span (not just volume), since this
// test needs both signals and neither relies on the engine's `result.values`
// cell-fold (see module docs).
// ─────────────────────────────────────────────────────────────────────────────

/// A transparent [`GeometryKernel`] proxy that records every SUCCESSFUL
/// [`GeometryOp`] dispatched through [`execute`](GeometryKernel::execute) and
/// [`execute_with_history`](GeometryKernel::execute_with_history), forwarding
/// ALL calls to the inner kernel unchanged, and caches each success's volume
/// and bbox z-span queried directly against the inner kernel.
struct RecordingKernel {
    inner: Box<dyn GeometryKernel>,
    /// Successfully-executed ops (parallel to `op_handles`).
    ops: Arc<Mutex<Vec<GeometryOp>>>,
    /// Handle ID for each successfully-executed op (parallel to `ops`).
    op_handles: Arc<Mutex<Vec<GeometryHandleId>>>,
    /// Cached volumes (m³) for each handle that had a successful, finite,
    /// positive `GeometryQuery::Volume` response from the inner kernel.
    volumes: Arc<Mutex<HashMap<GeometryHandleId, f64>>>,
    /// Cached bounding-box (zmin, zmax) (SI metres) for each handle that had
    /// a successful, finite `GeometryQuery::BoundingBox` response.
    bboxes: Arc<Mutex<HashMap<GeometryHandleId, (f64, f64)>>>,
}

impl RecordingKernel {
    /// Wrap `inner` in a recording proxy with empty op log, handle list, and
    /// volume/bbox caches.
    fn new(inner: Box<dyn GeometryKernel>) -> Self {
        Self {
            inner,
            ops: Arc::new(Mutex::new(Vec::new())),
            op_handles: Arc::new(Mutex::new(Vec::new())),
            volumes: Arc::new(Mutex::new(HashMap::new())),
            bboxes: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Clone the shared op-log [`Arc`]. Must be called **before** moving `self`.
    fn ops_ref(&self) -> Arc<Mutex<Vec<GeometryOp>>> {
        Arc::clone(&self.ops)
    }

    /// Clone the shared handle-list [`Arc`] (parallel to `ops`). Must be
    /// called **before** moving `self`.
    fn op_handles_ref(&self) -> Arc<Mutex<Vec<GeometryHandleId>>> {
        Arc::clone(&self.op_handles)
    }

    /// Clone the shared volume-cache [`Arc`]. Must be called **before**
    /// moving `self`.
    fn volumes_ref(&self) -> Arc<Mutex<HashMap<GeometryHandleId, f64>>> {
        Arc::clone(&self.volumes)
    }

    /// Clone the shared bbox-cache [`Arc`]. Must be called **before** moving
    /// `self`.
    fn bboxes_ref(&self) -> Arc<Mutex<HashMap<GeometryHandleId, (f64, f64)>>> {
        Arc::clone(&self.bboxes)
    }

    /// Record a successful op + its handle, and cache its volume and bbox
    /// z-span from the inner kernel.
    ///
    /// Called only after `Ok(handle)` — failed ops are NOT recorded.
    /// Non-volume-queryable / non-bbox-queryable shapes or query errors are
    /// silently skipped — the corresponding cache entry is simply absent.
    fn record_success(&mut self, op: &GeometryOp, handle_id: GeometryHandleId) {
        self.ops.lock().unwrap().push(op.clone());
        self.op_handles.lock().unwrap().push(handle_id);

        if let Ok(Value::Real(v)) = self.inner.query(&GeometryQuery::Volume(handle_id))
            && v.is_finite()
            && v > 0.0
        {
            self.volumes.lock().unwrap().insert(handle_id, v);
        }
        if let Ok(Value::String(s)) = self.inner.query(&GeometryQuery::BoundingBox(handle_id)) {
            let (zmin, zmax) = parse_bbox_z(&s);
            if zmin.is_finite() && zmax.is_finite() {
                self.bboxes.lock().unwrap().insert(handle_id, (zmin, zmax));
            }
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
        self.inner
            .export_with_options(handle, format, options, writer)
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

    fn execute_split(&mut self, op: &GeometryOp) -> Result<Vec<GeometryHandleId>, GeometryError> {
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

    fn measure_mesh_deviation(&self, handle: GeometryHandleId, mesh: &Mesh) -> Option<f64> {
        self.inner.measure_mesh_deviation(handle, mesh)
    }
}
