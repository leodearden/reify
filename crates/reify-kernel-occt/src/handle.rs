//! Dedicated-thread actor handle for the OCCT geometry kernel.
//!
//! OCCT uses process-global state (memory allocators, shape naming tables,
//! Standard_Failure exception state, STEP writer state), making concurrent
//! access undefined behaviour. `OcctKernelHandle` wraps communication with
//! a dedicated `std::thread` that owns the real `OcctKernel`, using
//! `tokio::sync::mpsc` / `oneshot` channels for request–reply messaging.
//!
//! The kernel thread is a plain `std::thread` (not a tokio task) because OCCT
//! operations are blocking CPU-bound FFI calls that would starve the async
//! runtime.
//!
//! `OcctKernelHandle` is naturally `Send + Sync` (channel senders are) and
//! implements `GeometryKernel`, so it can be used anywhere a boxed kernel
//! is expected.

use crate::{
    BooleanOpHistoryRecords, LocalFeatureOpHistoryRecords, LoftOpHistoryRecords,
    SweepOpHistoryRecords,
};
use reify_ir::{AttributeHistory, ExportError, ExportFormat, ExportOptions, ExportWarning, GeometryError, GeometryHandle, GeometryHandleId, GeometryKernel, GeometryOp, GeometryQuery, Mesh, OpaqueState, QueryError, TessError, Value, WarmStartable, debug_assert_query_many_invariant};
use tokio::sync::{mpsc, oneshot};

/// Reply payload for [`OcctRequest::ExportWithOptions`]: the serialized export
/// bytes paired with any non-fatal export warnings (e.g. an AP242→AP214
/// fallback), or an [`ExportError`]. Factored into a `type` alias to keep the
/// `oneshot::Sender<…>` field below under clippy's `type_complexity` threshold.
type ExportWithOptionsReply = Result<(Vec<u8>, Vec<ExportWarning>), ExportError>;

/// Requests sent from `OcctKernelHandle` to the dedicated kernel thread.
enum OcctRequest {
    Execute {
        op: Box<GeometryOp>,
        reply: oneshot::Sender<Result<GeometryHandle, GeometryError>>,
    },
    Query {
        query: GeometryQuery,
        reply: oneshot::Sender<Result<Value, QueryError>>,
    },
    QueryMany {
        queries: Vec<GeometryQuery>,
        reply: oneshot::Sender<Result<Vec<Value>, QueryError>>,
    },
    Export {
        handle: GeometryHandleId,
        format: ExportFormat,
        reply: oneshot::Sender<Result<Vec<u8>, ExportError>>,
    },
    /// Like `Export`, but threads `ExportOptions` (the STEP schema) and
    /// replies with both the serialized bytes and any non-fatal export
    /// warnings (e.g. an AP242→AP214 fallback). The plain `Export` variant is
    /// left untouched so the heavily-used CLI/GUI export path is unchanged.
    ExportWithOptions {
        handle: GeometryHandleId,
        format: ExportFormat,
        options: ExportOptions,
        reply: oneshot::Sender<ExportWithOptionsReply>,
    },
    Tessellate {
        handle: GeometryHandleId,
        tolerance: f64,
        reply: oneshot::Sender<Result<Mesh, TessError>>,
    },
    WarmState {
        reply: oneshot::Sender<Option<OpaqueState>>,
    },
    WithWarmState {
        state: OpaqueState,
        reply: oneshot::Sender<()>,
    },
    /// Task 5212: evict every resident native shape (+ derived caches) on the
    /// kernel thread while keeping `next_id` monotonic, bounding OCCT native
    /// memory across GUI whole-file reloads. Routed here because `OcctKernel`
    /// is `!Send`. Reply is `()` (fire-and-confirm), mirroring `WithWarmState`.
    Reset {
        reply: oneshot::Sender<()>,
    },
    /// T7: assemble N placed product solids into a TopoDS_Compound.
    MakeCompound {
        handles: Vec<GeometryHandleId>,
        reply: oneshot::Sender<Result<GeometryHandle, GeometryError>>,
    },
    ExtractEdges {
        handle: GeometryHandleId,
        reply: oneshot::Sender<Result<Vec<GeometryHandleId>, QueryError>>,
    },
    ExtractFaces {
        handle: GeometryHandleId,
        reply: oneshot::Sender<Result<Vec<GeometryHandleId>, QueryError>>,
    },
    /// Split a solid with an unbounded plane via `BRepAlgoAPI_Splitter`,
    /// returning the result solid handles. Mirrors `ExtractEdges` / `ExtractFaces`
    /// but returns `GeometryError` (not `QueryError`) to match the
    /// `GeometryKernel::execute_split` trait signature.
    Split {
        op: Box<GeometryOp>,
        reply: oneshot::Sender<Result<Vec<GeometryHandleId>, GeometryError>>,
    },
    ExtractVertices {
        handle: GeometryHandleId,
        reply: oneshot::Sender<Result<Vec<GeometryHandleId>, QueryError>>,
    },
    /// Task 4744 (mesh-morph β): project a point onto the closest location of a
    /// B-rep sub-shape. Routes the inherent `OcctKernel::closest_point_on_shape`
    /// across the actor channel so the morph boundary-node projector can reach
    /// it through `&dyn GeometryKernel` (the `OcctKernelHandle` trait impl).
    ClosestPointOnShape {
        handle: GeometryHandleId,
        px: f64,
        py: f64,
        pz: f64,
        reply: oneshot::Sender<Result<[f64; 3], QueryError>>,
    },
    /// Task 4744 (mesh-morph β): read a B-rep vertex's exact position
    /// (`BRep_Tool::Pnt` direct). Routes the inherent `OcctKernel::vertex_point`
    /// across the actor channel (the `vertex_position` projector capability).
    VertexPoint {
        handle: GeometryHandleId,
        reply: oneshot::Sender<Result<[f64; 3], QueryError>>,
    },
    BooleanFuseWithHistory {
        left: GeometryHandleId,
        right: GeometryHandleId,
        reply: oneshot::Sender<Result<(GeometryHandle, BooleanOpHistoryRecords), GeometryError>>,
    },
    /// Run `BRepAlgoAPI_Cut` (left − right) with history. Part of v0.2
    /// persistent-naming-v2 (task 2656, step-2).
    BooleanCutWithHistory {
        left: GeometryHandleId,
        right: GeometryHandleId,
        reply: oneshot::Sender<Result<(GeometryHandle, BooleanOpHistoryRecords), GeometryError>>,
    },
    /// Run `BRepAlgoAPI_Common` (A ∩ B) with history. Part of v0.2
    /// persistent-naming-v2 (task 2656, step-4).
    BooleanCommonWithHistory {
        left: GeometryHandleId,
        right: GeometryHandleId,
        reply: oneshot::Sender<Result<(GeometryHandle, BooleanOpHistoryRecords), GeometryError>>,
    },
    /// v0.2 persistent-naming-v2 local-feature history: apply
    /// `BRepFilletAPI_MakeFillet` to every edge of `shape` with the given
    /// `radius`, capturing Modified/Generated/Deleted records. Mirrors the
    /// BooleanFuseWithHistory pattern (dedicated request variant rather than
    /// routing through ExecuteWithHistory, which would require AttributeHistory
    /// enum variants in reify-types — out of scope for this FFI-only task).
    FilletWithHistory {
        shape: GeometryHandleId,
        radius: f64,
        reply:
            oneshot::Sender<Result<(GeometryHandle, LocalFeatureOpHistoryRecords), GeometryError>>,
    },
    /// Curated per-edge fillet (task 3205): apply `BRepFilletAPI_MakeFillet` to
    /// ONLY the selected `edges` of `shape` with the given `radius`, capturing
    /// Modified/Generated/Deleted records. Mirrors `FilletWithHistory` but
    /// carries the curated edge subset. An empty `edges` vector is rejected by
    /// the kernel — the all-edges path is `FilletWithHistory` / `fillet_all_edges`.
    FilletEdgesWithHistory {
        shape: GeometryHandleId,
        radius: f64,
        edges: Vec<GeometryHandleId>,
        reply:
            oneshot::Sender<Result<(GeometryHandle, LocalFeatureOpHistoryRecords), GeometryError>>,
    },
    /// v0.2 persistent-naming-v2 local-feature history: apply
    /// `BRepFilletAPI_MakeChamfer` to every edge of `shape` with the given
    /// `distance`, capturing Modified/Generated/Deleted records. Mirrors
    /// FilletWithHistory.
    ChamferWithHistory {
        shape: GeometryHandleId,
        distance: f64,
        reply:
            oneshot::Sender<Result<(GeometryHandle, LocalFeatureOpHistoryRecords), GeometryError>>,
    },
    /// Curated per-edge chamfer (task 4185): apply `BRepFilletAPI_MakeChamfer`
    /// to ONLY the selected `edges` of `shape` with the given `distance`,
    /// capturing Modified/Generated/Deleted records. Mirrors `ChamferWithHistory`
    /// but carries the curated edge subset. An empty `edges` vector is rejected
    /// by the kernel — the all-edges path is `ChamferWithHistory` /
    /// `chamfer_all_edges`.
    ChamferEdgesWithHistory {
        shape: GeometryHandleId,
        distance: f64,
        edges: Vec<GeometryHandleId>,
        reply:
            oneshot::Sender<Result<(GeometryHandle, LocalFeatureOpHistoryRecords), GeometryError>>,
    },
    /// Curated asymmetric per-edge chamfer (task 4185, β): apply
    /// `BRepFilletAPI_MakeChamfer` to ONLY the selected `edges` of `shape` with
    /// DISTINCT setbacks `d1`/`d2` (d1 on the reference face, d2 on the other),
    /// capturing Modified/Generated/Deleted records. An empty `edges` vector
    /// means all edges (the Rust wrapper enumerates them; there is no separate
    /// asymmetric all-edges path).
    ChamferAsymmetricEdgesWithHistory {
        shape: GeometryHandleId,
        d1: f64,
        d2: f64,
        edges: Vec<GeometryHandleId>,
        reply:
            oneshot::Sender<Result<(GeometryHandle, LocalFeatureOpHistoryRecords), GeometryError>>,
    },
    /// v0.2 persistent-naming-v2 sweep history: dispatches per-op to a
    /// kernel-side history-aware primitive (Extrude → `extrude_with_history`,
    /// future variants → analogous), returning `AttributeHistory::None` for
    /// ops without a history-aware variant. The single dispatch site
    /// (`Engine::execute_realization_ops`) routes through this variant.
    ExecuteWithHistory {
        op: Box<GeometryOp>,
        reply: oneshot::Sender<Result<(GeometryHandle, AttributeHistory), GeometryError>>,
    },
    /// Test-fixture: build an `width × height` rect_face profile in the
    /// kernel and return its handle id. Only available when the
    /// `test-fixtures` cargo feature is enabled (mirrors the
    /// `OcctKernel::store_*_for_test` pattern). Used by integration tests
    /// of sweep history primitives that need a planar face profile but
    /// have no source-level face constructor.
    #[cfg(feature = "test-fixtures")]
    MakeRectProfileForTest {
        width: f64,
        height: f64,
        reply: oneshot::Sender<GeometryHandleId>,
    },
    /// Test-fixture: build a `width × height` rect_face profile centered
    /// at `(cx, cy, cz)` and return its handle id. Variant of
    /// `MakeRectProfileForTest` that accepts a non-origin center — used
    /// by the `revolve_with_history` integration test to place the profile
    /// off-axis (an on-axis rect would produce a degenerate revolved solid).
    #[cfg(feature = "test-fixtures")]
    MakeRectProfileAtForTest {
        width: f64,
        height: f64,
        cx: f64,
        cy: f64,
        cz: f64,
        reply: oneshot::Sender<GeometryHandleId>,
    },
    /// Test-fixture: build a triangular face profile in the plane Y=cy
    /// with vertices (x1, cy, z1), (x2, cy, z2), (x3, cy, z3), store it
    /// in the kernel, and return its handle id. Used by the revolve
    /// history regression test (task 2636, step-3) to exercise a
    /// non-rectangular profile with one radial edge and two slanted edges.
    #[cfg(feature = "test-fixtures")]
    MakeTriangleProfileAtForTest {
        x1: f64,
        z1: f64,
        x2: f64,
        z2: f64,
        x3: f64,
        z3: f64,
        cy: f64,
        reply: oneshot::Sender<GeometryHandleId>,
    },
    /// Test-fixture: return the outward unit normal for a face without
    /// going through the JSON-encoded `GeometryQuery::FaceNormal` path.
    /// Mirrors `OcctKernel::face_outward_unit_normal_for_test`; used by
    /// integration tests to avoid hand-rolling serde_json parsing.
    #[cfg(feature = "test-fixtures")]
    FaceOutwardUnitNormalForTest {
        face: GeometryHandleId,
        reply: oneshot::Sender<Result<[f64; 3], QueryError>>,
    },
    /// Sampled max facet-chord deviation of a mesh from the exact BRep of
    /// `handle`, in SI metres (Determinacy β, task 4198).
    ///
    /// Mesh data is carried as plain `Vec<f32>` / `Vec<u32>` (not as `Mesh`)
    /// to keep the message type plain and avoid extra trait bounds.
    /// The kernel-thread handler reconstructs a `Mesh` before calling
    /// [`OcctKernel::measure_mesh_deviation`].
    ///
    /// On success the reply carries `Ok(f64)` (metres); the inherent wrapper
    /// maps Ok→Some, Err→None for honest absence at the trait boundary.
    MeasureMeshDeviation {
        handle: GeometryHandleId,
        vertices: Vec<f32>,
        indices: Vec<u32>,
        reply: oneshot::Sender<Result<f64, QueryError>>,
    },
}

/// Thread-safe handle to an OCCT kernel running on a dedicated thread.
///
/// All geometry operations are serialized through a channel to the kernel
/// thread. The handle is `Send + Sync` and implements `GeometryKernel`.
///
/// # Async safety
///
/// The sync methods (`execute`, `query`, `export`, `tessellate`) use
/// `blocking_send`/`blocking_recv` and **panic if called from an async
/// context**. Use the `_async` variants (`execute_async`, `query_async`,
/// `export_async`, `tessellate_async`) from async code.
///
/// The `WarmStartable` trait methods (`warm_state`, `with_warm_state`) are
/// safe to call from both sync and async contexts — they detect the runtime
/// and use `block_in_place` when needed.
///
/// # Drop behaviour
///
/// When dropped inside an async context, the handle detaches the kernel
/// thread (it exits naturally when its channel closes) but does **not**
/// join it — avoiding blocking a tokio worker thread. For deterministic
/// cleanup from async code, call [`shutdown()`](Self::shutdown) instead.
/// When dropped outside an async context, the thread is joined normally.
pub struct OcctKernelHandle {
    tx: mpsc::Sender<OcctRequest>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl OcctKernelHandle {
    /// Export a geometry handle to the given format, writing bytes to `writer`.
    ///
    /// The kernel thread serializes to a `Vec<u8>` internally, then sends the
    /// bytes back through the channel. The handle writes them to the caller's
    /// writer. This avoids sending the `!Send` `&mut dyn Write` across threads.
    ///
    /// # Panics
    ///
    /// Panics if called from within a tokio async execution context. Use
    /// [`export_async`](Self::export_async) instead.
    pub fn export(
        &self,
        handle: GeometryHandleId,
        format: ExportFormat,
        writer: &mut dyn std::io::Write,
    ) -> Result<(), ExportError> {
        let bytes = self.send_request_blocking(
            |reply| OcctRequest::Export {
                handle,
                format,
                reply,
            },
            || ExportError::IoError("kernel thread died".into()),
        )??;
        writer
            .write_all(&bytes)
            .map_err(|e| ExportError::IoError(e.to_string()))
    }

    /// Export a geometry handle honoring [`ExportOptions`] (the STEP schema),
    /// writing bytes to `writer` and returning any non-fatal export warnings.
    ///
    /// Routes through the new `ExportWithOptions` actor request so the schema
    /// selection (and any AP242→AP214 fallback) happens on the kernel thread
    /// alongside the FFI. The kernel serializes to a `Vec<u8>` internally and
    /// replies with `(bytes, warnings)`; the handle writes the bytes to the
    /// caller's writer and returns the warnings.
    ///
    /// # Panics
    ///
    /// Panics if called from within a tokio async execution context.
    pub fn export_with_options(
        &self,
        handle: GeometryHandleId,
        format: ExportFormat,
        options: &ExportOptions,
        writer: &mut dyn std::io::Write,
    ) -> Result<Vec<ExportWarning>, ExportError> {
        let (bytes, warnings) = self.send_request_blocking(
            |reply| OcctRequest::ExportWithOptions {
                handle,
                format,
                options: *options,
                reply,
            },
            || ExportError::IoError("kernel thread died".into()),
        )??;
        writer
            .write_all(&bytes)
            .map_err(|e| ExportError::IoError(e.to_string()))?;
        Ok(warnings)
    }

    /// Run a query against a geometry handle on the kernel thread.
    ///
    /// # Panics
    ///
    /// Panics if called from within a tokio async execution context. Use
    /// [`query_async`](Self::query_async) instead.
    pub fn query(&self, query: &GeometryQuery) -> Result<Value, QueryError> {
        self.send_request_blocking(
            |reply| OcctRequest::Query {
                query: query.clone(),
                reply,
            },
            || QueryError::QueryFailed("kernel thread died".into()),
        )?
    }

    /// Task 4744 (mesh-morph β): project `(px, py, pz)` onto the closest location
    /// of the B-rep sub-shape `handle`, routed across the actor channel to the
    /// inherent `OcctKernel::closest_point_on_shape`. The `GeometryKernel` trait
    /// override (below) forwards `[f64; 3]` here so the morph boundary-node
    /// projector reaches OCCT's `BRepExtrema_DistShapeShape` through
    /// `&dyn GeometryKernel`.
    pub fn closest_point_on_shape(
        &self,
        handle: GeometryHandleId,
        px: f64,
        py: f64,
        pz: f64,
    ) -> Result<[f64; 3], QueryError> {
        self.send_request_blocking(
            |reply| OcctRequest::ClosestPointOnShape {
                handle,
                px,
                py,
                pz,
                reply,
            },
            || QueryError::QueryFailed("kernel thread died".into()),
        )?
    }

    /// Task 4744 (mesh-morph β): read the exact position of the B-rep vertex
    /// `handle`, routed across the actor channel to the inherent
    /// `OcctKernel::vertex_point` (`BRep_Tool::Pnt` direct). The `vertex_position`
    /// projector capability surfaced through `&dyn GeometryKernel`.
    pub fn vertex_point(&self, handle: GeometryHandleId) -> Result<[f64; 3], QueryError> {
        self.send_request_blocking(
            |reply| OcctRequest::VertexPoint { handle, reply },
            || QueryError::QueryFailed("kernel thread died".into()),
        )?
    }

    /// Run a batch of queries in a single channel round-trip and return
    /// the results in order.
    ///
    /// Sends one `QueryMany` request to the kernel thread; the kernel
    /// thread fail-fast collects per-query results (stopping at the
    /// first `QueryError`) and replies with a `Result<Vec<Value>,
    /// QueryError>`. This collapses the actor-channel send/recv to a
    /// single round-trip, eliminating the N+1 latency that per-element
    /// `query` incurs in tight selector loops.
    ///
    /// As a hot-path optimization, an empty `queries` slice is
    /// short-circuited locally: the channel send/recv is skipped and
    /// `Ok(Vec::new())` is returned immediately. This matters because
    /// selectors built on `extract_edges` / `extract_faces` may produce
    /// an empty handle list for shapes with no sub-shapes of the
    /// requested kind, and forcing those calls through the actor channel
    /// for a guaranteed-empty reply is pure overhead.
    ///
    /// # Panics
    ///
    /// Panics if called from within a tokio async execution context.
    pub fn query_many(&self, queries: &[GeometryQuery]) -> Result<Vec<Value>, QueryError> {
        // Empty-batch fast path: skip the actor channel round-trip
        // entirely. The kernel-thread arm would itself produce
        // `Ok(Vec::new())`, so the result is identical.
        if queries.is_empty() {
            return Ok(Vec::new());
        }
        let reply: Vec<Value> = self.send_request_blocking(
            |reply| OcctRequest::QueryMany {
                queries: queries.to_vec(),
                reply,
            },
            || QueryError::QueryFailed("kernel thread died".into()),
        )??;
        debug_assert_query_many_invariant(queries, &reply);
        Ok(reply)
    }

    /// Tessellate a geometry handle into a mesh on the kernel thread.
    ///
    /// # Panics
    ///
    /// Panics if called from within a tokio async execution context. Use
    /// [`tessellate_async`](Self::tessellate_async) instead.
    pub fn tessellate(&self, handle: GeometryHandleId, tolerance: f64) -> Result<Mesh, TessError> {
        self.send_request_blocking(
            |reply| OcctRequest::Tessellate {
                handle,
                tolerance,
                reply,
            },
            || TessError::TessellationFailed("kernel thread died".into()),
        )?
    }

    /// Compute the sampled max facet-chord deviation of `mesh` from the exact
    /// BRep of `handle`, in SI metres.
    ///
    /// Sends a [`OcctRequest::MeasureMeshDeviation`] to the kernel thread and
    /// blocks until the result arrives. Returns `Some(metres)` on success,
    /// `None` on channel failure or kernel error (honest absence, B3).
    ///
    /// Mirrors [`OcctKernel::measure_mesh_deviation`] across the channel:
    /// clones the mesh vertices/indices into the request, blocks on the reply,
    /// maps `Ok(d)` → `Some(d)`, any `Err` → `None`.
    ///
    /// # Panics
    ///
    /// Panics if called from within a tokio async execution context.
    pub fn measure_mesh_deviation(
        &self,
        handle: GeometryHandleId,
        mesh: &Mesh,
    ) -> Option<f64> {
        let vertices = mesh.vertices.clone();
        let indices = mesh.indices.clone();
        self.send_request_blocking(
            |reply| OcctRequest::MeasureMeshDeviation {
                handle,
                vertices,
                indices,
                reply,
            },
            || QueryError::QueryFailed("kernel thread died".into()),
        )
        .ok() // channel failure → None
        .and_then(|r| r.ok()) // kernel error (InvalidHandle, FFI error) → None
    }

    /// Extract the unique edges of a shape, storing each as a new handle on
    /// the kernel thread, and return the resulting list of handle ids.
    ///
    /// # Panics
    ///
    /// Panics if called from within a tokio async execution context. Use
    /// [`extract_edges_async`](Self::extract_edges_async) instead.
    /// Assemble N placed product solids into a single `TopoDS_Compound` handle
    /// for multi-body STEP export (T7 `make_compound`).
    ///
    /// Sends a `MakeCompound` request to the kernel thread and blocks until
    /// the result arrives.  Source handles remain valid after the call.
    ///
    /// # Panics
    ///
    /// Panics if called from within a tokio async execution context.
    pub fn make_compound(
        &self,
        handles: &[GeometryHandleId],
    ) -> Result<GeometryHandle, GeometryError> {
        let handles = handles.to_vec();
        self.send_request_blocking(
            |reply| OcctRequest::MakeCompound { handles, reply },
            || GeometryError::OperationFailed("kernel thread died".into()),
        )?
    }

    /// Split `op.target` with a cutting plane via `BRepAlgoAPI_Splitter`,
    /// returning the result solid handles. Sends an `OcctRequest::Split` to
    /// the kernel thread and blocks until the result arrives.
    ///
    /// # Panics
    ///
    /// Panics if called from within a tokio async execution context.
    pub fn execute_split(
        &self,
        op: &GeometryOp,
    ) -> Result<Vec<GeometryHandleId>, GeometryError> {
        let op = Box::new(op.clone());
        self.send_request_blocking(
            |reply| OcctRequest::Split { op, reply },
            || GeometryError::OperationFailed("kernel thread died".into()),
        )?
    }

    pub fn extract_edges(
        &self,
        handle: GeometryHandleId,
    ) -> Result<Vec<GeometryHandleId>, QueryError> {
        self.send_request_blocking(
            |reply| OcctRequest::ExtractEdges { handle, reply },
            || QueryError::QueryFailed("kernel thread died".into()),
        )?
    }

    /// Extract the unique faces of a shape, storing each as a new handle on
    /// the kernel thread, and return the resulting list of handle ids.
    ///
    /// # Panics
    ///
    /// Panics if called from within a tokio async execution context. Use
    /// [`extract_faces_async`](Self::extract_faces_async) instead.
    pub fn extract_faces(
        &self,
        handle: GeometryHandleId,
    ) -> Result<Vec<GeometryHandleId>, QueryError> {
        self.send_request_blocking(
            |reply| OcctRequest::ExtractFaces { handle, reply },
            || QueryError::QueryFailed("kernel thread died".into()),
        )?
    }

    /// Extract the unique vertices of a shape, storing each as a new handle on
    /// the kernel thread, and return the resulting list of handle ids.
    ///
    /// # Panics
    ///
    /// Panics if called from within a tokio async execution context. Use
    /// [`extract_vertices_async`](Self::extract_vertices_async) instead.
    pub fn extract_vertices(
        &self,
        handle: GeometryHandleId,
    ) -> Result<Vec<GeometryHandleId>, QueryError> {
        self.send_request_blocking(
            |reply| OcctRequest::ExtractVertices { handle, reply },
            || QueryError::QueryFailed("kernel thread died".into()),
        )?
    }

    /// Fuse `left` and `right` via `BRepAlgoAPI_Fuse` and return the
    /// fused-result handle id alongside the per-parent face/edge history
    /// records (Modified / Generated / Deleted) emitted by the algorithm.
    ///
    /// Mirrors [`OcctKernel::boolean_fuse_with_history`] across the
    /// kernel-thread channel. Result handle is registered with
    /// `BRepKind::Solid`.
    ///
    /// # Panics
    ///
    /// Panics if called from within a tokio async execution context.
    ///
    /// Part of v0.2 persistent-naming-v2 (task 2590, step-14).
    pub fn boolean_fuse_with_history(
        &self,
        left: GeometryHandleId,
        right: GeometryHandleId,
    ) -> Result<(GeometryHandleId, BooleanOpHistoryRecords), GeometryError> {
        let (handle, records) = self.send_request_blocking(
            |reply| OcctRequest::BooleanFuseWithHistory { left, right, reply },
            || GeometryError::OperationFailed("kernel thread died".into()),
        )??;
        Ok((handle.id, records))
    }

    /// Cut `left` by `right` via `BRepAlgoAPI_Cut` (left − right) and return
    /// the cut-result handle id alongside the per-parent face/edge history
    /// records (Modified / Generated / Deleted) emitted by the algorithm.
    ///
    /// Mirrors [`OcctKernel::boolean_cut_with_history`] across the
    /// kernel-thread channel. Result handle is registered with
    /// `BRepKind::Solid`.
    ///
    /// # Panics
    ///
    /// Panics if called from within a tokio async execution context.
    ///
    /// Part of v0.2 persistent-naming-v2 (task 2656, step-2).
    pub fn boolean_cut_with_history(
        &self,
        left: GeometryHandleId,
        right: GeometryHandleId,
    ) -> Result<(GeometryHandleId, BooleanOpHistoryRecords), GeometryError> {
        let (handle, records) = self.send_request_blocking(
            |reply| OcctRequest::BooleanCutWithHistory { left, right, reply },
            || GeometryError::OperationFailed("kernel thread died".into()),
        )??;
        Ok((handle.id, records))
    }

    /// Compute the intersection of `left` and `right` via `BRepAlgoAPI_Common`
    /// (A ∩ B) and return the result handle id alongside the per-parent face/edge
    /// history records (Modified / Generated / Deleted) emitted by the algorithm.
    ///
    /// Mirrors [`OcctKernel::boolean_common_with_history`] across the
    /// kernel-thread channel. Result handle is registered with
    /// `BRepKind::Solid`.
    ///
    /// # Panics
    ///
    /// Panics if called from within a tokio async execution context.
    ///
    /// Part of v0.2 persistent-naming-v2 (task 2656, step-4).
    pub fn boolean_common_with_history(
        &self,
        left: GeometryHandleId,
        right: GeometryHandleId,
    ) -> Result<(GeometryHandleId, BooleanOpHistoryRecords), GeometryError> {
        let (handle, records) = self.send_request_blocking(
            |reply| OcctRequest::BooleanCommonWithHistory { left, right, reply },
            || GeometryError::OperationFailed("kernel thread died".into()),
        )??;
        Ok((handle.id, records))
    }

    /// Apply `BRepFilletAPI_MakeFillet` to every edge of `shape` with the
    /// given `radius`, returning the modified-result handle id alongside the
    /// per-parent face/edge history records (Modified / Generated / Deleted)
    /// emitted by the algorithm.
    ///
    /// Mirrors [`OcctKernel::fillet_with_history`] across the kernel-thread
    /// channel. Result handle is registered with `BRepKind::Solid`.
    ///
    /// # Panics
    ///
    /// Panics if called from within a tokio async execution context.
    ///
    /// Part of v0.2 persistent-naming-v2 (task 2655, step-2).
    pub fn fillet_with_history(
        &self,
        shape: GeometryHandleId,
        radius: f64,
    ) -> Result<(GeometryHandleId, LocalFeatureOpHistoryRecords), GeometryError> {
        let (handle, records) = self.send_request_blocking(
            |reply| OcctRequest::FilletWithHistory {
                shape,
                radius,
                reply,
            },
            || GeometryError::OperationFailed("kernel thread died".into()),
        )??;
        Ok((handle.id, records))
    }

    /// Apply `BRepFilletAPI_MakeFillet` to ONLY the selected `edges` of `shape`
    /// (a curated subset) with the given `radius`, returning the
    /// modified-result handle id alongside the per-parent face/edge history
    /// records (Modified / Generated / Deleted) emitted by the algorithm.
    ///
    /// Mirrors [`OcctKernel::fillet_edges_with_history`] across the
    /// kernel-thread channel. Result handle is registered with
    /// `BRepKind::Solid`. An empty `edges` slice is rejected — the all-edges
    /// path is [`Self::fillet_with_history`].
    ///
    /// # Panics
    ///
    /// Panics if called from within a tokio async execution context.
    ///
    /// Curated edge-selection seam (task 3205).
    pub fn fillet_edges_with_history(
        &self,
        shape: GeometryHandleId,
        radius: f64,
        edges: &[GeometryHandleId],
    ) -> Result<(GeometryHandleId, LocalFeatureOpHistoryRecords), GeometryError> {
        let edges = edges.to_vec();
        let (handle, records) = self.send_request_blocking(
            |reply| OcctRequest::FilletEdgesWithHistory {
                shape,
                radius,
                edges,
                reply,
            },
            || GeometryError::OperationFailed("kernel thread died".into()),
        )??;
        Ok((handle.id, records))
    }

    /// Apply `BRepFilletAPI_MakeChamfer` to every edge of `shape` with the
    /// given `distance`, returning the modified-result handle id alongside the
    /// per-parent face/edge history records (Modified / Generated / Deleted)
    /// emitted by the algorithm.
    ///
    /// Mirrors [`OcctKernel::chamfer_with_history`] across the kernel-thread
    /// channel. Result handle is registered with `BRepKind::Solid`.
    ///
    /// # Panics
    ///
    /// Panics if called from within a tokio async execution context.
    ///
    /// Part of v0.2 persistent-naming-v2 (task 2655, step-6).
    pub fn chamfer_with_history(
        &self,
        shape: GeometryHandleId,
        distance: f64,
    ) -> Result<(GeometryHandleId, LocalFeatureOpHistoryRecords), GeometryError> {
        let (handle, records) = self.send_request_blocking(
            |reply| OcctRequest::ChamferWithHistory {
                shape,
                distance,
                reply,
            },
            || GeometryError::OperationFailed("kernel thread died".into()),
        )??;
        Ok((handle.id, records))
    }

    /// Apply `BRepFilletAPI_MakeChamfer` to ONLY the selected `edges` of `shape`
    /// (a curated subset) with the given `distance`, returning the
    /// modified-result handle id alongside the per-parent face/edge history
    /// records (Modified / Generated / Deleted) emitted by the algorithm.
    ///
    /// Mirrors [`OcctKernel::chamfer_edges_with_history`] across the
    /// kernel-thread channel. Result handle is registered with
    /// `BRepKind::Solid`. An empty `edges` slice is rejected — the all-edges
    /// path is [`Self::chamfer_with_history`].
    ///
    /// # Panics
    ///
    /// Panics if called from within a tokio async execution context.
    ///
    /// Curated edge-selection seam (task 4185).
    pub fn chamfer_edges_with_history(
        &self,
        shape: GeometryHandleId,
        distance: f64,
        edges: &[GeometryHandleId],
    ) -> Result<(GeometryHandleId, LocalFeatureOpHistoryRecords), GeometryError> {
        let edges = edges.to_vec();
        let (handle, records) = self.send_request_blocking(
            |reply| OcctRequest::ChamferEdgesWithHistory {
                shape,
                distance,
                edges,
                reply,
            },
            || GeometryError::OperationFailed("kernel thread died".into()),
        )??;
        Ok((handle.id, records))
    }

    /// Apply `BRepFilletAPI_MakeChamfer` with ASYMMETRIC setbacks (`d1`/`d2`) to
    /// a curated subset of `edges` of `shape`, returning the modified-result
    /// handle id alongside the per-parent face/edge history records
    /// (Modified / Generated / Deleted) emitted by the algorithm.
    ///
    /// Mirrors [`OcctKernel::chamfer_asymmetric_edges_with_history`] across the
    /// kernel-thread channel. Result handle is registered with `BRepKind::Solid`.
    /// An empty `edges` slice means ALL edges (back-compat); both `d1` and `d2`
    /// must be finite positive.
    ///
    /// # Panics
    ///
    /// Panics if called from within a tokio async execution context.
    ///
    /// Curated edge-selection seam (task 4185, β).
    pub fn chamfer_asymmetric_edges_with_history(
        &self,
        shape: GeometryHandleId,
        d1: f64,
        d2: f64,
        edges: &[GeometryHandleId],
    ) -> Result<(GeometryHandleId, LocalFeatureOpHistoryRecords), GeometryError> {
        let edges = edges.to_vec();
        let (handle, records) = self.send_request_blocking(
            |reply| OcctRequest::ChamferAsymmetricEdgesWithHistory {
                shape,
                d1,
                d2,
                edges,
                reply,
            },
            || GeometryError::OperationFailed("kernel thread died".into()),
        )??;
        Ok((handle.id, records))
    }

    /// Execute `op` on the kernel thread, returning the result handle and
    /// any kernel-emitted [`AttributeHistory`] for the op.
    ///
    /// For ops the kernel has a history-aware primitive for (currently
    /// `GeometryOp::Extrude`; revolve in step-10), this returns the
    /// op-specific `AttributeHistory` variant carrying the
    /// `SweepOpHistoryRecords`. For all other ops, this returns
    /// `AttributeHistory::None` and is functionally identical to
    /// [`OcctKernelHandle::execute`].
    ///
    /// Channel-routed to the kernel thread's inherent
    /// `OcctKernel::execute_with_history` dispatcher.
    ///
    /// # Panics
    ///
    /// Panics if called from within a tokio async execution context.
    ///
    /// Part of v0.2 persistent-naming-v2 (task 5a / #2573, step-8).
    pub fn execute_with_history(
        &self,
        op: &GeometryOp,
    ) -> Result<(GeometryHandle, AttributeHistory), GeometryError> {
        self.send_request_blocking(
            |reply| OcctRequest::ExecuteWithHistory {
                op: Box::new(op.clone()),
                reply,
            },
            || GeometryError::OperationFailed("kernel thread died".into()),
        )?
    }

    /// Convenience wrapper around [`execute_with_history`](Self::execute_with_history)
    /// for the [`GeometryOp::Extrude`] case: returns `(handle_id,
    /// SweepOpHistoryRecords)` directly, matching the call shape of
    /// [`boolean_fuse_with_history`](Self::boolean_fuse_with_history).
    ///
    /// # Errors
    ///
    /// Returns `GeometryError::OperationFailed` if the kernel reported
    /// an unexpected `AttributeHistory` variant — e.g. `None`, indicating
    /// the kernel built but failed to populate sweep history. This is a
    /// programming-error guard (the kernel-side dispatcher always returns
    /// `Extrude(_)` for `Extrude` ops); it is exposed as an `Err` rather
    /// than a panic so test code can pin the contract.
    ///
    /// # Panics
    ///
    /// Panics if called from within a tokio async execution context.
    ///
    /// Part of v0.2 persistent-naming-v2 (task 5a / #2573, step-8).
    pub fn extrude_with_history(
        &self,
        profile: GeometryHandleId,
        distance: f64,
    ) -> Result<(GeometryHandleId, SweepOpHistoryRecords), GeometryError> {
        let op = GeometryOp::Extrude {
            profile,
            distance: Value::Real(distance),
        };
        let (handle, history) = self.execute_with_history(&op)?;
        match history {
            AttributeHistory::Extrude(records) => Ok((handle.id, records)),
            other => Err(GeometryError::OperationFailed(format!(
                "extrude_with_history expected AttributeHistory::Extrude, got {other:?}"
            ))),
        }
    }

    /// Convenience wrapper around [`execute_with_history`](Self::execute_with_history)
    /// for the [`GeometryOp::Revolve`] case: returns `(handle_id,
    /// SweepOpHistoryRecords)` directly.
    ///
    /// `axis_origin` and `axis_dir` are 3-element arrays in metres /
    /// (dimensionless direction); `angle_rad` is the revolve angle in
    /// radians. Validation thresholds match
    /// [`OcctKernel::revolve_with_history`] (axis must be finite +
    /// magnitude > AXIS_MAG_SQ_MIN; angle must be finite + magnitude >
    /// ANGLE_ABS_MIN).
    ///
    /// # Errors
    ///
    /// Returns `GeometryError::OperationFailed` if the kernel reported
    /// an unexpected `AttributeHistory` variant — e.g. `None`, indicating
    /// the kernel built but failed to populate sweep history. This is a
    /// programming-error guard; it is exposed as an `Err` rather than a
    /// panic so test code can pin the contract.
    ///
    /// # Panics
    ///
    /// Panics if called from within a tokio async execution context.
    ///
    /// Part of v0.2 persistent-naming-v2 (task 5a / #2573, step-10).
    pub fn revolve_with_history(
        &self,
        profile: GeometryHandleId,
        axis_origin: [f64; 3],
        axis_dir: [f64; 3],
        angle_rad: f64,
    ) -> Result<(GeometryHandleId, SweepOpHistoryRecords), GeometryError> {
        let op = GeometryOp::Revolve {
            profile,
            axis_origin,
            axis_dir,
            angle_rad,
        };
        let (handle, history) = self.execute_with_history(&op)?;
        match history {
            AttributeHistory::Revolve(records) => Ok((handle.id, records)),
            other => Err(GeometryError::OperationFailed(format!(
                "revolve_with_history expected AttributeHistory::Revolve, got {other:?}"
            ))),
        }
    }

    /// Convenience wrapper around [`execute_with_history`](Self::execute_with_history)
    /// for the [`GeometryOp::Sweep`] case: returns `(handle_id,
    /// SweepOpHistoryRecords)` directly.
    ///
    /// Sweep is single-parent — the profile is the operand whose
    /// sub-shapes propagate to the result; the path is the spine along
    /// which the profile is swept. `parent_index` in every record is
    /// `0`. `start_cap_face_indices` carries the FirstShape() face
    /// index (profile-as-placed); `end_cap_face_indices` carries the
    /// LastShape() face index (profile at the spine end).
    ///
    /// # Errors
    ///
    /// Returns `GeometryError::OperationFailed` if the kernel reported
    /// an unexpected `AttributeHistory` variant — e.g. `None`, indicating
    /// the kernel built but failed to populate sweep history. This is a
    /// programming-error guard; it is exposed as an `Err` rather than a
    /// panic so test code can pin the contract.
    ///
    /// # Panics
    ///
    /// Panics if called from within a tokio async execution context.
    ///
    /// Part of v0.2 persistent-naming-v2 (task 5b / #2619, step-4).
    pub fn sweep_with_history(
        &self,
        profile: GeometryHandleId,
        path: GeometryHandleId,
    ) -> Result<(GeometryHandleId, SweepOpHistoryRecords), GeometryError> {
        let op = GeometryOp::Sweep { profile, path };
        let (handle, history) = self.execute_with_history(&op)?;
        match history {
            AttributeHistory::Sweep(records) => Ok((handle.id, records)),
            other => Err(GeometryError::OperationFailed(format!(
                "sweep_with_history expected AttributeHistory::Sweep, got {other:?}"
            ))),
        }
    }

    /// Convenience wrapper around [`execute_with_history`](Self::execute_with_history)
    /// for the [`GeometryOp::Loft`] case: returns `(handle_id,
    /// LoftOpHistoryRecords)` directly.
    ///
    /// Loft is **multi-parent** — each profile section indexed `0..N-1`
    /// is a distinct parent, and the `parent_index` field in every
    /// `face_generated` record denotes the section index. The result
    /// `LoftOpHistoryRecords` carries per-section
    /// `BRepOffsetAPI_ThruSections::GeneratedFace(edge)` correspondences
    /// alongside the FirstShape/LastShape cap-face indices (populated
    /// under the hard-coded `is_solid=true` semantics).
    ///
    /// # Errors
    ///
    /// - `GeometryError::OperationFailed("Loft requires at least 2 profiles")`
    ///   when `profiles.len() < 2`. Mirrors the `GeometryOp::Loft` arm of
    ///   `OcctKernel::execute` and the C++ `make_loft_with_history`
    ///   validation.
    /// - `GeometryError::OperationFailed` if the kernel reported an
    ///   unexpected `AttributeHistory` variant. This is a programming-
    ///   error guard (the dispatcher always returns `Loft(_)` for `Loft`
    ///   ops); exposed as `Err` so test code can pin the contract.
    ///
    /// # Panics
    ///
    /// Panics if called from within a tokio async execution context.
    ///
    /// Part of v0.2 persistent-naming-v2 (task 5b / #2619, step-6).
    pub fn loft_with_history(
        &self,
        profiles: &[GeometryHandleId],
    ) -> Result<(GeometryHandleId, LoftOpHistoryRecords), GeometryError> {
        let op = GeometryOp::Loft {
            profiles: profiles.to_vec(),
        };
        let (handle, history) = self.execute_with_history(&op)?;
        match history {
            AttributeHistory::Loft(records) => Ok((handle.id, records)),
            other => Err(GeometryError::OperationFailed(format!(
                "loft_with_history expected AttributeHistory::Loft, got {other:?}"
            ))),
        }
    }

    /// Test-fixture: build a `width × height` rect_face profile on the
    /// kernel thread and return its handle id (registered with
    /// `BRepKind::Face`). Only compiled when the `test-fixtures` cargo
    /// feature is enabled — mirrors the `OcctKernel::store_*_for_test`
    /// pattern in `lib.rs`.
    ///
    /// Used by `extrude_with_history_integration` /
    /// `revolve_with_history_integration` tests (and the future task-5a
    /// e2e variants) to construct planar profiles without a source-level
    /// face constructor.
    ///
    /// # Panics
    ///
    /// Panics if called from within a tokio async execution context, or
    /// if the kernel thread is dead (test-fixture path; production code
    /// should use `execute_with_history` and handle the channel-died error).
    #[cfg(feature = "test-fixtures")]
    pub fn make_rect_profile_for_test(
        &self,
        width: f64,
        height: f64,
    ) -> Result<GeometryHandleId, GeometryError> {
        self.send_request_blocking(
            |reply| OcctRequest::MakeRectProfileForTest {
                width,
                height,
                reply,
            },
            || GeometryError::OperationFailed("kernel thread died".into()),
        )
    }

    /// Test-fixture: build a `width × height` rect_face profile in the
    /// **XZ plane** centered at `(cx, cy, cz)` on the kernel thread, and
    /// return its handle id (registered with `BRepKind::Face`). Variant of
    /// `make_rect_profile_for_test` purpose-built for the
    /// `revolve_with_history` integration test: the rect is placed in a
    /// plane containing the Z-axis (XZ plane), so revolving about Z
    /// produces a non-degenerate solid. `width` becomes the radial
    /// dimension; `height` becomes the axial dimension. The `(cx, cy, cz)`
    /// translation lets the caller offset the profile clear of the
    /// rotation axis (an on-axis rect would produce a degenerate revolved
    /// solid).
    ///
    /// # Panics
    ///
    /// Panics if called from within a tokio async execution context, or
    /// if the kernel thread is dead (test-fixture path).
    #[cfg(feature = "test-fixtures")]
    pub fn make_rect_profile_at_for_test(
        &self,
        width: f64,
        height: f64,
        cx: f64,
        cy: f64,
        cz: f64,
    ) -> Result<GeometryHandleId, GeometryError> {
        self.send_request_blocking(
            |reply| OcctRequest::MakeRectProfileAtForTest {
                width,
                height,
                cx,
                cy,
                cz,
                reply,
            },
            || GeometryError::OperationFailed("kernel thread died".into()),
        )
    }

    /// Test-fixture: build a triangular face profile in the plane Y=cy
    /// with vertices (x1, cy, z1), (x2, cy, z2), (x3, cy, z3) on the
    /// kernel thread, and return its handle id (registered with
    /// `BRepKind::Face`). Used by the revolve history regression test
    /// (task 2636, step-3) to exercise a non-rectangular profile.
    ///
    /// # Panics
    ///
    /// Panics if called from within a tokio async execution context, or
    /// if the kernel thread is dead (test-fixture path).
    #[cfg(feature = "test-fixtures")]
    #[allow(clippy::too_many_arguments)] // 3 vertex coords + cy is intrinsic to triangle-profile geometry; struct wrapping adds noise.
    pub fn make_triangle_profile_at_for_test(
        &self,
        x1: f64,
        z1: f64,
        x2: f64,
        z2: f64,
        x3: f64,
        z3: f64,
        cy: f64,
    ) -> Result<GeometryHandleId, GeometryError> {
        self.send_request_blocking(
            |reply| OcctRequest::MakeTriangleProfileAtForTest {
                x1,
                z1,
                x2,
                z2,
                x3,
                z3,
                cy,
                reply,
            },
            || GeometryError::OperationFailed("kernel thread died".into()),
        )
    }

    /// Test-fixture: return the outward unit normal `[nx, ny, nz]` for the
    /// face identified by `face`, routed through the actor channel to the
    /// dedicated kernel thread.
    ///
    /// Mirrors [`OcctKernel::face_outward_unit_normal_for_test`] and is
    /// intended for integration tests that previously hand-parsed the
    /// `{"x":.., "y":.., "z":..}` JSON returned by
    /// `GeometryQuery::FaceNormal`. Replacing that ad-hoc serde_json parse
    /// with this typed helper decouples tests from the wire encoding.
    ///
    /// # Errors
    ///
    /// Propagates `QueryError::InvalidHandle` or `QueryError::QueryFailed`
    /// from the underlying kernel call. Returns
    /// `QueryError::QueryFailed("kernel thread died")` if the kernel thread
    /// has exited.
    ///
    /// # Panics
    ///
    /// Panics if called from within a tokio async execution context.
    #[cfg(feature = "test-fixtures")]
    pub fn face_outward_unit_normal_for_test(
        &self,
        face: GeometryHandleId,
    ) -> Result<[f64; 3], QueryError> {
        self.send_request_blocking(
            |reply| OcctRequest::FaceOutwardUnitNormalForTest { face, reply },
            || QueryError::QueryFailed("kernel thread died".into()),
        )?
    }

    /// Execute a geometry operation on the kernel thread.
    ///
    /// # Panics
    ///
    /// Panics if called from within a tokio async execution context. Use
    /// [`execute_async`](Self::execute_async) instead.
    pub fn execute(&self, op: &GeometryOp) -> Result<GeometryHandle, GeometryError> {
        self.send_request_blocking(
            |reply| OcctRequest::Execute {
                op: Box::new(op.clone()),
                reply,
            },
            || GeometryError::OperationFailed("kernel thread died".into()),
        )?
    }

    /// Spawn a new OCCT kernel on a dedicated OS thread and return a handle.
    pub fn spawn() -> Self {
        let (tx, mut rx) = mpsc::channel::<OcctRequest>(32);

        let thread = std::thread::spawn(move || {
            let mut kernel = crate::OcctKernel::new();

            while let Some(request) = rx.blocking_recv() {
                match request {
                    OcctRequest::Execute { op, reply } => {
                        let result = kernel.execute(&op);
                        let _ = reply.send(result);
                    }
                    OcctRequest::Query { query, reply } => {
                        let result = kernel.query(&query);
                        let _ = reply.send(result);
                    }
                    OcctRequest::QueryMany { queries, reply } => {
                        // Fail-fast collect: Result<Vec<_>, _>'s FromIterator
                        // short-circuits on the first Err, so we stop issuing
                        // FFI calls once any query fails.
                        let result: Result<Vec<Value>, QueryError> =
                            queries.iter().map(|q| kernel.query(q)).collect();
                        let _ = reply.send(result);
                    }
                    OcctRequest::Export {
                        handle,
                        format,
                        reply,
                    } => {
                        let mut buf = Vec::new();
                        let result = kernel.export(handle, format, &mut buf).map(|()| buf);
                        let _ = reply.send(result);
                    }
                    OcctRequest::ExportWithOptions {
                        handle,
                        format,
                        options,
                        reply,
                    } => {
                        let mut buf = Vec::new();
                        let result = kernel
                            .export_with_options(handle, format, &options, &mut buf)
                            .map(|warnings| (buf, warnings));
                        let _ = reply.send(result);
                    }
                    OcctRequest::Tessellate {
                        handle,
                        tolerance,
                        reply,
                    } => {
                        let result = kernel.tessellate(handle, tolerance);
                        let _ = reply.send(result);
                    }
                    OcctRequest::WarmState { reply } => {
                        let result = kernel.warm_state();
                        let _ = reply.send(result);
                    }
                    OcctRequest::WithWarmState { state, reply } => {
                        kernel.with_warm_state(state);
                        let _ = reply.send(());
                    }
                    OcctRequest::Reset { reply } => {
                        kernel.reset();
                        let _ = reply.send(());
                    }
                    OcctRequest::MakeCompound { handles, reply } => {
                        let result = kernel.make_compound(&handles);
                        let _ = reply.send(result);
                    }
                    OcctRequest::Split { op, reply } => {
                        let result = kernel.execute_split(&op);
                        let _ = reply.send(result);
                    }
                    OcctRequest::ExtractEdges { handle, reply } => {
                        let result = kernel.extract_edges(handle);
                        let _ = reply.send(result);
                    }
                    OcctRequest::ExtractFaces { handle, reply } => {
                        let result = kernel.extract_faces(handle);
                        let _ = reply.send(result);
                    }
                    OcctRequest::ExtractVertices { handle, reply } => {
                        let result = kernel.extract_vertices(handle);
                        let _ = reply.send(result);
                    }
                    OcctRequest::ClosestPointOnShape {
                        handle,
                        px,
                        py,
                        pz,
                        reply,
                    } => {
                        let result = kernel.closest_point_on_shape(handle, px, py, pz);
                        let _ = reply.send(result);
                    }
                    OcctRequest::VertexPoint { handle, reply } => {
                        let result = kernel.vertex_point(handle);
                        let _ = reply.send(result);
                    }
                    OcctRequest::BooleanFuseWithHistory { left, right, reply } => {
                        let result = kernel.boolean_fuse_with_history(left, right);
                        let _ = reply.send(result);
                    }
                    OcctRequest::BooleanCutWithHistory { left, right, reply } => {
                        let result = kernel.boolean_cut_with_history(left, right);
                        let _ = reply.send(result);
                    }
                    OcctRequest::BooleanCommonWithHistory { left, right, reply } => {
                        let result = kernel.boolean_common_with_history(left, right);
                        let _ = reply.send(result);
                    }
                    OcctRequest::FilletWithHistory {
                        shape,
                        radius,
                        reply,
                    } => {
                        let result = kernel.fillet_with_history(shape, radius);
                        let _ = reply.send(result);
                    }
                    OcctRequest::FilletEdgesWithHistory {
                        shape,
                        radius,
                        edges,
                        reply,
                    } => {
                        let result = kernel.fillet_edges_with_history(shape, radius, &edges);
                        let _ = reply.send(result);
                    }
                    OcctRequest::ChamferWithHistory {
                        shape,
                        distance,
                        reply,
                    } => {
                        let result = kernel.chamfer_with_history(shape, distance);
                        let _ = reply.send(result);
                    }
                    OcctRequest::ChamferEdgesWithHistory {
                        shape,
                        distance,
                        edges,
                        reply,
                    } => {
                        let result = kernel.chamfer_edges_with_history(shape, distance, &edges);
                        let _ = reply.send(result);
                    }
                    OcctRequest::ChamferAsymmetricEdgesWithHistory {
                        shape,
                        d1,
                        d2,
                        edges,
                        reply,
                    } => {
                        let result =
                            kernel.chamfer_asymmetric_edges_with_history(shape, d1, d2, &edges);
                        let _ = reply.send(result);
                    }
                    OcctRequest::ExecuteWithHistory { op, reply } => {
                        // Per-op dispatcher: route Extrude/Revolve to the
                        // history-aware kernel primitives; fall through to
                        // `execute(op)` (returning `AttributeHistory::None`)
                        // for ops without a history-aware variant. Future
                        // task-5b/6/7/8 add Sweep/Loft/primitive/local/boolean
                        // arms here.
                        let result = match op.as_ref() {
                            GeometryOp::Extrude { profile, distance } => {
                                // Mirror the validation in
                                // `OcctKernel::extrude_with_history`: distance
                                // must be a finite, non-zero numeric Value.
                                match distance.as_f64() {
                                    Some(d) => kernel
                                        .extrude_with_history(*profile, d)
                                        .map(|(h, recs)| (h, AttributeHistory::Extrude(recs))),
                                    None => Err(GeometryError::OperationFailed(
                                        "extrude distance must be numeric".into(),
                                    )),
                                }
                            }
                            GeometryOp::Revolve {
                                profile,
                                axis_origin,
                                axis_dir,
                                angle_rad,
                            } => kernel
                                .revolve_with_history(*profile, *axis_origin, *axis_dir, *angle_rad)
                                .map(|(h, recs)| (h, AttributeHistory::Revolve(recs))),
                            // Task 5b (#2619): GeometryOp::Sweep routes to
                            // `OcctKernel::sweep_with_history`, which uses
                            // `BRepOffsetAPI_MakePipe` and the same templated
                            // history-emit helpers as extrude / revolve.
                            GeometryOp::Sweep { profile, path } => kernel
                                .sweep_with_history(*profile, *path)
                                .map(|(h, recs)| (h, AttributeHistory::Sweep(recs))),
                            // Task 5b (#2619): GeometryOp::Loft routes to
                            // `OcctKernel::loft_with_history`, which uses
                            // `BRepOffsetAPI_ThruSections::GeneratedFace(edge)`
                            // for per-section face correspondence (multi-parent).
                            GeometryOp::Loft { profiles } => kernel
                                .loft_with_history(profiles)
                                .map(|(h, recs)| (h, AttributeHistory::Loft(recs))),
                            // Task 8 (#2656): Binary boolean ops route to the
                            // matching history-aware BRepAlgoAPI primitives.
                            // All three share the same parent-index semantics:
                            // parent_index 0 = left, 1 = right.
                            GeometryOp::Union { left, right } => kernel
                                .boolean_fuse_with_history(*left, *right)
                                .map(|(h, recs)| (h, AttributeHistory::Boolean(recs))),
                            GeometryOp::Difference { left, right } => kernel
                                .boolean_cut_with_history(*left, *right)
                                .map(|(h, recs)| (h, AttributeHistory::Boolean(recs))),
                            GeometryOp::Intersection { left, right } => kernel
                                .boolean_common_with_history(*left, *right)
                                .map(|(h, recs)| (h, AttributeHistory::Boolean(recs))),
                            // Task 7b (#2831): Fillet branches on
                            // edges.is_empty() — all-edges vs curated-edge —
                            // mirroring OcctKernel::execute (lib.rs Fillet arm).
                            // Both producers return (GeometryHandle,
                            // LocalFeatureOpHistoryRecords) and map uniformly
                            // to AttributeHistory::LocalFeature.
                            GeometryOp::Fillet {
                                target,
                                edges,
                                radius,
                            } => match radius.as_f64() {
                                Some(r) => (if edges.is_empty() {
                                    kernel.fillet_with_history(*target, r)
                                } else {
                                    kernel.fillet_edges_with_history(*target, r, edges)
                                })
                                .map(|(h, recs)| (h, AttributeHistory::LocalFeature(recs))),
                                None => Err(GeometryError::OperationFailed(
                                    "fillet radius must be numeric".into(),
                                )),
                            },
                            // Task β (#4185): Chamfer branches on
                            // edges.is_empty() — all-edges vs curated-edge —
                            // mirroring OcctKernel::execute (lib.rs Chamfer arm)
                            // and the Fillet arm above. Both producers return
                            // (GeometryHandle, LocalFeatureOpHistoryRecords) and
                            // map uniformly to AttributeHistory::LocalFeature.
                            GeometryOp::Chamfer {
                                target,
                                edges,
                                distance,
                            } => match distance.as_f64() {
                                Some(d) => (if edges.is_empty() {
                                    kernel.chamfer_with_history(*target, d)
                                } else {
                                    kernel.chamfer_edges_with_history(*target, d, edges)
                                })
                                .map(|(h, recs)| (h, AttributeHistory::LocalFeature(recs))),
                                None => Err(GeometryError::OperationFailed(
                                    "chamfer distance must be numeric".into(),
                                )),
                            },
                            // Default arm: no history-aware primitive yet for
                            // this op. Forward to plain `execute` and emit
                            // `AttributeHistory::None`.
                            _ => kernel.execute(&op).map(|h| (h, AttributeHistory::None)),
                        };
                        let _ = reply.send(result);
                    }
                    #[cfg(feature = "test-fixtures")]
                    OcctRequest::MakeRectProfileForTest {
                        width,
                        height,
                        reply,
                    } => {
                        let id = kernel.store_rect_face_for_test(width, height);
                        let _ = reply.send(id);
                    }
                    #[cfg(feature = "test-fixtures")]
                    OcctRequest::MakeRectProfileAtForTest {
                        width,
                        height,
                        cx,
                        cy,
                        cz,
                        reply,
                    } => {
                        let id = kernel.store_rect_face_at_for_test(width, height, cx, cy, cz);
                        let _ = reply.send(id);
                    }
                    #[cfg(feature = "test-fixtures")]
                    OcctRequest::MakeTriangleProfileAtForTest {
                        x1,
                        z1,
                        x2,
                        z2,
                        x3,
                        z3,
                        cy,
                        reply,
                    } => {
                        let id = kernel.store_triangle_face_at_for_test(x1, z1, x2, z2, x3, z3, cy);
                        let _ = reply.send(id);
                    }
                    #[cfg(feature = "test-fixtures")]
                    OcctRequest::FaceOutwardUnitNormalForTest { face, reply } => {
                        let result = kernel.face_outward_unit_normal_for_test(face);
                        let _ = reply.send(result);
                    }
                    OcctRequest::MeasureMeshDeviation {
                        handle,
                        vertices,
                        indices,
                        reply,
                    } => {
                        // Reconstruct a `Mesh` from the cloned vertex/index data.
                        // Normals are not needed by the metric.
                        let mesh = Mesh {
                            vertices,
                            indices,
                            normals: None,
                        };
                        let result = kernel.measure_mesh_deviation(handle, &mesh);
                        let _ = reply.send(result);
                    }
                }
            }
            // Channel closed (sender dropped) → exit cleanly.
        });

        Self {
            tx,
            thread: Some(thread),
        }
    }

    /// Compress the build-channel + blocking-send + blocking-recv +
    /// map-channel-died-once boilerplate for synchronous inherent methods that
    /// return `Result<Resp, E>`.
    ///
    /// `chan_died` is `FnOnce` because the two failure paths (send failure and
    /// recv failure) are mutually exclusive — only one can occur per call.
    ///
    /// Panics if called from within a tokio async execution context; use
    /// [`send_request_async`](Self::send_request_async) instead.
    fn send_request_blocking<Resp, E>(
        &self,
        build_req: impl FnOnce(oneshot::Sender<Resp>) -> OcctRequest,
        chan_died: impl FnOnce() -> E,
    ) -> Result<Resp, E> {
        let (reply_tx, reply_rx) = oneshot::channel();
        if self.tx.blocking_send(build_req(reply_tx)).is_err() {
            return Err(chan_died());
        }
        reply_rx.blocking_recv().map_err(|_| chan_died())
    }

    /// Compress the build-channel + async-send + await-recv +
    /// map-channel-died-once boilerplate for async inherent methods that return
    /// `Result<Resp, E>`.
    ///
    /// `chan_died` is `FnOnce` because the two failure paths (send failure and
    /// recv failure) are mutually exclusive — only one can occur per call.
    ///
    /// Note: `warm_state_async` and `with_warm_state_async` intentionally do
    /// not use this helper — their channel-death semantics differ (`Option`/`()`
    /// with `.ok()?` rather than a typed error), so they inline the pattern
    /// directly. See the TODO on each of those methods.
    ///
    /// Safe to call from within a tokio async execution context.
    async fn send_request_async<Resp, E>(
        &self,
        build_req: impl FnOnce(oneshot::Sender<Resp>) -> OcctRequest,
        chan_died: impl FnOnce() -> E,
    ) -> Result<Resp, E> {
        let (reply_tx, reply_rx) = oneshot::channel();
        if self.tx.send(build_req(reply_tx)).await.is_err() {
            return Err(chan_died());
        }
        reply_rx.await.map_err(|_| chan_died())
    }

    // --- Async companion methods ---
    //
    // Safe to call from within a tokio async execution context (unlike the
    // sync methods which use blocking_send/blocking_recv and will panic).

    /// Execute a geometry operation on the kernel thread (async version).
    ///
    /// Safe to call from within a tokio async execution context.
    pub async fn execute_async(&self, op: &GeometryOp) -> Result<GeometryHandle, GeometryError> {
        self.send_request_async(
            |reply| OcctRequest::Execute {
                op: Box::new(op.clone()),
                reply,
            },
            || GeometryError::OperationFailed("kernel thread died".into()),
        )
        .await?
    }

    /// Run a query against a geometry handle on the kernel thread (async version).
    ///
    /// Safe to call from within a tokio async execution context.
    pub async fn query_async(&self, query: &GeometryQuery) -> Result<Value, QueryError> {
        self.send_request_async(
            |reply| OcctRequest::Query {
                query: query.clone(),
                reply,
            },
            || QueryError::QueryFailed("kernel thread died".into()),
        )
        .await?
    }

    /// Export a geometry handle to the given format (async version).
    ///
    /// Returns the exported bytes directly instead of taking `&mut dyn Write`,
    /// because writer references cannot be held across await points and would
    /// make the future `!Send`.
    ///
    /// Safe to call from within a tokio async execution context.
    pub async fn export_async(
        &self,
        handle: GeometryHandleId,
        format: ExportFormat,
    ) -> Result<Vec<u8>, ExportError> {
        self.send_request_async(
            |reply| OcctRequest::Export {
                handle,
                format,
                reply,
            },
            || ExportError::IoError("kernel thread died".into()),
        )
        .await?
    }

    /// Tessellate a geometry handle into a mesh (async version).
    ///
    /// Safe to call from within a tokio async execution context.
    pub async fn tessellate_async(
        &self,
        handle: GeometryHandleId,
        tolerance: f64,
    ) -> Result<Mesh, TessError> {
        self.send_request_async(
            |reply| OcctRequest::Tessellate {
                handle,
                tolerance,
                reply,
            },
            || TessError::TessellationFailed("kernel thread died".into()),
        )
        .await?
    }

    /// Extract the unique edges of a shape (async version).
    ///
    /// Safe to call from within a tokio async execution context.
    pub async fn extract_edges_async(
        &self,
        handle: GeometryHandleId,
    ) -> Result<Vec<GeometryHandleId>, QueryError> {
        self.send_request_async(
            |reply| OcctRequest::ExtractEdges { handle, reply },
            || QueryError::QueryFailed("kernel thread died".into()),
        )
        .await?
    }

    /// Extract the unique faces of a shape (async version).
    ///
    /// Safe to call from within a tokio async execution context.
    pub async fn extract_faces_async(
        &self,
        handle: GeometryHandleId,
    ) -> Result<Vec<GeometryHandleId>, QueryError> {
        self.send_request_async(
            |reply| OcctRequest::ExtractFaces { handle, reply },
            || QueryError::QueryFailed("kernel thread died".into()),
        )
        .await?
    }

    /// Extract the unique vertices of a shape (async version).
    ///
    /// Safe to call from within a tokio async execution context.
    pub async fn extract_vertices_async(
        &self,
        handle: GeometryHandleId,
    ) -> Result<Vec<GeometryHandleId>, QueryError> {
        self.send_request_async(
            |reply| OcctRequest::ExtractVertices { handle, reply },
            || QueryError::QueryFailed("kernel thread died".into()),
        )
        .await?
    }

    /// Extract warm-start state from the kernel thread (async version).
    ///
    /// Safe to call from within a tokio async execution context.
    ///
    /// # Design note
    ///
    /// This method intentionally does not use [`send_request_async`](Self::send_request_async)
    /// because its channel-death semantics differ: a dead kernel is treated as
    /// "no warm state" (`Option` / `.ok()?`) rather than as a typed error.
    /// Routing through the helper would require a fake error type or a third
    /// helper variant — neither improves clarity over the explicit inline form.
    pub async fn warm_state_async(&self) -> Option<OpaqueState> {
        let (reply_tx, reply_rx) = oneshot::channel();
        self.tx
            .send(OcctRequest::WarmState { reply: reply_tx })
            .await
            .ok()?;
        reply_rx.await.ok()?
    }

    /// Restore warm-start state on the kernel thread (async version).
    ///
    /// Safe to call from within a tokio async execution context.
    ///
    /// # Design note
    ///
    /// This method intentionally does not use [`send_request_async`](Self::send_request_async)
    /// because its channel-death semantics differ: failure is silently ignored
    /// (`()` return, fire-and-forget) rather than mapped to a typed error.
    /// Routing through the helper would require a fake error type or a third
    /// helper variant — neither improves clarity over the explicit inline form.
    pub async fn with_warm_state_async(&self, state: OpaqueState) {
        let (reply_tx, reply_rx) = oneshot::channel();
        if self
            .tx
            .send(OcctRequest::WithWarmState {
                state,
                reply: reply_tx,
            })
            .await
            .is_ok()
        {
            let _ = reply_rx.await;
        }
    }

    /// Reset the kernel thread's `OcctKernel`: evict every resident native
    /// shape and clear the derived caches while keeping `next_id` monotonic
    /// (see `OcctKernel::reset`), bounding OCCT native memory across GUI
    /// whole-file reloads.
    ///
    /// Routed over the actor channel because `OcctKernel` is `!Send` and lives
    /// on the dedicated OS thread. Blocks until the kernel thread confirms;
    /// safe from both sync and async contexts (via `send_recv`). A dead kernel
    /// thread is a silent no-op — there is nothing left to reset. Mirrors the
    /// `with_warm_state` channel wiring (`&self` because the actor channel
    /// sender only needs a shared borrow; the `GeometryKernel::reset` override
    /// takes `&mut self` and delegates here).
    pub fn reset(&self) {
        let (reply_tx, reply_rx) = oneshot::channel::<()>();
        send_recv(&self.tx, OcctRequest::Reset { reply: reply_tx }, reply_rx);
    }

    /// Explicitly shut down the kernel thread from an async context.
    ///
    /// Drops the channel sender (closing the channel so the kernel thread exits
    /// naturally) then joins the kernel thread via `spawn_blocking` to avoid
    /// blocking the async worker.
    ///
    /// This gives async callers a deterministic cleanup path — the kernel
    /// thread has fully exited (and OCCT resources are freed) by the time
    /// this future resolves.
    pub async fn shutdown(mut self) {
        // Close the channel by replacing the sender with a dummy.
        let (dummy_tx, _) = mpsc::channel::<OcctRequest>(1);
        let _ = std::mem::replace(&mut self.tx, dummy_tx);

        if let Some(thread) = self.thread.take() {
            // Join on a blocking thread to avoid blocking the async worker.
            let _ = tokio::task::spawn_blocking(move || thread.join()).await;
        }
        // self.thread is now None, so Drop will be a no-op.
    }
}

/// Send a request and wait for the reply, safely handling both sync and
/// async calling contexts.
///
/// When called from outside a tokio runtime, uses `blocking_send` /
/// `blocking_recv` directly. When called from within an async runtime,
/// dispatches the blocking work to a helper `std::thread` to avoid
/// panicking (tokio's blocking primitives panic inside an async context).
fn send_recv<T: Send + 'static>(
    tx: &mpsc::Sender<OcctRequest>,
    request: OcctRequest,
    reply_rx: oneshot::Receiver<T>,
) -> Option<T> {
    if tokio::runtime::Handle::try_current().is_ok() {
        // Inside an async runtime — cannot use blocking_send/blocking_recv.
        // Clone the sender and move everything to a helper OS thread.
        let tx = tx.clone();
        std::thread::spawn(move || {
            tx.blocking_send(request).ok()?;
            reply_rx.blocking_recv().ok()
        })
        .join()
        .ok()?
    } else {
        tx.blocking_send(request).ok()?;
        reply_rx.blocking_recv().ok()
    }
}

impl WarmStartable for OcctKernelHandle {
    fn warm_state(&self) -> Option<OpaqueState> {
        let (reply_tx, reply_rx) = oneshot::channel();
        send_recv(
            &self.tx,
            OcctRequest::WarmState { reply: reply_tx },
            reply_rx,
        )?
    }

    fn with_warm_state(&mut self, state: OpaqueState) {
        let (reply_tx, reply_rx) = oneshot::channel::<()>();
        send_recv(
            &self.tx,
            OcctRequest::WithWarmState {
                state,
                reply: reply_tx,
            },
            reply_rx,
        );
    }
}

impl Drop for OcctKernelHandle {
    fn drop(&mut self) {
        if let Some(thread) = self.thread.take() {
            // Replace tx with a dummy sender, dropping the original. This closes
            // the channel, causing the kernel thread's recv loop to exit.
            let (dummy_tx, _) = mpsc::channel::<OcctRequest>(1);
            let _ = std::mem::replace(&mut self.tx, dummy_tx);

            // Detect whether we're inside an async execution context.
            if tokio::runtime::Handle::try_current().is_ok() {
                // Inside async context: do NOT call thread.join() — it would
                // block the tokio worker thread. The kernel thread will exit
                // naturally when its recv loop sees the closed channel.
                // OCCT resources are freed when the thread exits (just
                // asynchronously). For deterministic cleanup, use shutdown().
                //
                // FORWARD-LOOKING HAZARD (task 5212, harmless today): skipping
                // thread.join() here is safe ONLY because the OcctKernelHandle
                // session is never dropped-and-rebuilt inside the tokio runtime
                // today. If an 'open file' / session-swap flow is ever wired to
                // recreate this handle inside the async runtime, the old kernel
                // thread could still be draining while a new one spawns — two
                // OS threads mutating OCCT's process-global state concurrently
                // (UB). Such a flow must route teardown through shutdown()
                // (which joins via spawn_blocking) rather than relying on Drop.
            } else {
                // Outside async context: safe to block on join for
                // deterministic cleanup.
                let _ = thread.join();
            }
        }
    }
}

impl GeometryKernel for OcctKernelHandle {
    fn execute(&mut self, op: &GeometryOp) -> Result<GeometryHandle, GeometryError> {
        // Delegate to inherent method (which only needs &self).
        OcctKernelHandle::execute(self, op)
    }

    /// Override the trait no-op default: `OcctKernel` owns a growing table of
    /// native B-rep shapes that must be freed on whole-file reload. Delegates
    /// to the channel-routed inherent `OcctKernelHandle::reset` (which only
    /// needs `&self`).
    fn reset(&mut self) {
        OcctKernelHandle::reset(self)
    }

    fn query(&self, query: &GeometryQuery) -> Result<Value, QueryError> {
        OcctKernelHandle::query(self, query)
    }

    /// Override the trait default with a real channel-routed batched
    /// implementation. Delegates to the inherent `query_many` (which
    /// only needs `&self`).
    fn query_many(&self, queries: &[GeometryQuery]) -> Result<Vec<Value>, QueryError> {
        OcctKernelHandle::query_many(self, queries)
    }

    /// Task 4744 (mesh-morph β): override the honest-absence trait default with
    /// the real channel-routed projection. Forwards `[f64; 3]` to the inherent
    /// `OcctKernelHandle::closest_point_on_shape(handle, px, py, pz)` (which
    /// routes to OCCT's BRepExtrema). This is the cycle-free projection seam:
    /// `reify-mesh-morph` reaches it through `&dyn GeometryKernel` without
    /// naming `OcctKernel`.
    fn closest_point_on_shape(
        &self,
        handle: GeometryHandleId,
        point: [f64; 3],
    ) -> Result<[f64; 3], QueryError> {
        OcctKernelHandle::closest_point_on_shape(self, handle, point[0], point[1], point[2])
    }

    /// Task 4744 (mesh-morph β): override the honest-absence trait default with
    /// the real channel-routed vertex-position read. Delegates to the inherent
    /// `OcctKernelHandle::vertex_point` (`BRep_Tool::Pnt` direct).
    fn vertex_point(&self, handle: GeometryHandleId) -> Result<[f64; 3], QueryError> {
        OcctKernelHandle::vertex_point(self, handle)
    }

    fn export(
        &self,
        handle: GeometryHandleId,
        format: ExportFormat,
        writer: &mut dyn std::io::Write,
    ) -> Result<(), ExportError> {
        OcctKernelHandle::export(self, handle, format, writer)
    }

    /// Override the trait default with a real channel-routed implementation
    /// so OCCT actually selects the STEP schema. Delegates to the inherent
    /// `export_with_options` (which only needs `&self`).
    fn export_with_options(
        &self,
        handle: GeometryHandleId,
        format: ExportFormat,
        options: &ExportOptions,
        writer: &mut dyn std::io::Write,
    ) -> Result<Vec<ExportWarning>, ExportError> {
        OcctKernelHandle::export_with_options(self, handle, format, options, writer)
    }

    fn tessellate(&self, handle: GeometryHandleId, tolerance: f64) -> Result<Mesh, TessError> {
        OcctKernelHandle::tessellate(self, handle, tolerance)
    }

    /// Override the trait default with a real channel-routed implementation
    /// (T7 `make_compound`). Delegates to the inherent `make_compound`.
    fn make_compound(
        &mut self,
        handles: &[GeometryHandleId],
    ) -> Result<GeometryHandle, GeometryError> {
        OcctKernelHandle::make_compound(self, handles)
    }

    /// Override the trait default with a real channel-routed implementation.
    /// Delegates to the inherent `execute_split` (which only needs `&self`).
    fn execute_split(
        &mut self,
        op: &GeometryOp,
    ) -> Result<Vec<GeometryHandleId>, GeometryError> {
        OcctKernelHandle::execute_split(self, op)
    }

    /// Override the trait default with a real channel-routed implementation.
    /// Delegates to the inherent `extract_edges` (which only needs `&self`).
    fn extract_edges(
        &mut self,
        handle: GeometryHandleId,
    ) -> Result<Vec<GeometryHandleId>, QueryError> {
        OcctKernelHandle::extract_edges(self, handle)
    }

    /// Override the trait default with a real channel-routed implementation.
    /// Delegates to the inherent `extract_faces` (which only needs `&self`).
    fn extract_faces(
        &mut self,
        handle: GeometryHandleId,
    ) -> Result<Vec<GeometryHandleId>, QueryError> {
        OcctKernelHandle::extract_faces(self, handle)
    }

    /// Override the trait default with a real channel-routed implementation.
    /// Delegates to the inherent `extract_vertices` (which only needs `&self`).
    fn extract_vertices(
        &mut self,
        handle: GeometryHandleId,
    ) -> Result<Vec<GeometryHandleId>, QueryError> {
        OcctKernelHandle::extract_vertices(self, handle)
    }

    /// Override the trait default with a real channel-routed implementation
    /// that surfaces kernel-emitted [`AttributeHistory`] for ops with
    /// history-aware primitives (currently `GeometryOp::Extrude`; revolve
    /// in step-10). Delegates to the inherent `execute_with_history`
    /// (which only needs `&self`).
    fn execute_with_history(
        &mut self,
        op: &GeometryOp,
    ) -> Result<(GeometryHandle, AttributeHistory), GeometryError> {
        OcctKernelHandle::execute_with_history(self, op)
    }

    /// Override the default-absent trait method with a real channel-routed
    /// implementation. Delegates to the inherent
    /// [`measure_mesh_deviation`](OcctKernelHandle::measure_mesh_deviation)
    /// (which only needs `&self`).
    ///
    /// Returns `Some(metres)` on success; `None` on channel failure or invalid
    /// handle (honest absence, B3 — mirrors the default for non-OCCT kernels).
    fn measure_mesh_deviation(
        &self,
        handle: GeometryHandleId,
        mesh: &Mesh,
    ) -> Option<f64> {
        OcctKernelHandle::measure_mesh_deviation(self, handle, mesh)
    }
}

#[cfg(all(test, has_occt))]
mod tests {
    use reify_ir::{BRepKind, GeometryHandleId, GeometryOp, GeometryQuery, Value};

    /// Compile-time assertion: OcctKernelHandle must be Send + Sync.
    const _: fn() = || {
        fn must_be_send_sync<T: Send + Sync>() {}
        must_be_send_sync::<super::OcctKernelHandle>();
    };

    #[test]
    fn spawn_returns_handle_without_panic() {
        let handle = super::OcctKernelHandle::spawn();
        // Just verifying spawn() returns successfully without panic.
        drop(handle);
    }

    #[test]
    fn execute_creates_box_and_returns_handle() {
        let handle = super::OcctKernelHandle::spawn();
        let op = GeometryOp::Box {
            width: Value::Real(10.0),
            height: Value::Real(20.0),
            depth: Value::Real(30.0),
        };
        let result = handle.execute(&op).unwrap();
        assert_eq!(result.id, GeometryHandleId(1));
        assert_eq!(result.repr, Some(BRepKind::Solid));
    }

    #[test]
    fn query_volume_returns_correct_value() {
        let handle = super::OcctKernelHandle::spawn();
        let op = GeometryOp::Box {
            width: Value::Real(10.0),
            height: Value::Real(20.0),
            depth: Value::Real(30.0),
        };
        let gh = handle.execute(&op).unwrap();
        let result = handle
            .query(&reify_ir::GeometryQuery::Volume(gh.id))
            .unwrap();
        match result {
            Value::Real(v) => {
                // 10 * 20 * 30 = 6000
                assert!((v - 6000.0).abs() < 1.0, "expected ~6000, got {v}");
            }
            other => panic!("expected Value::Real, got {:?}", other),
        }
    }

    #[test]
    fn query_invalid_handle_returns_error() {
        let handle = super::OcctKernelHandle::spawn();
        let result = handle.query(&reify_ir::GeometryQuery::Volume(GeometryHandleId(999)));
        assert!(result.is_err());
        match result.unwrap_err() {
            reify_ir::QueryError::InvalidHandle(id) => {
                assert_eq!(id, GeometryHandleId(999));
            }
            other => panic!("expected InvalidHandle, got {:?}", other),
        }
    }

    /// Channel-routed `reset()` on `OcctKernelHandle`: evicts the underlying
    /// shape on the dedicated kernel thread (old handle → error via the #5211
    /// `get_shape` guard, never a panic or stale value) and a subsequent
    /// execute mints a strictly-greater (monotonic) id. Also exercises the
    /// polymorphic trait path (`&mut dyn GeometryKernel`) so the engine's
    /// `Box<dyn GeometryKernel>` reset call is covered.
    #[test]
    fn reset_evicts_shape_and_keeps_next_id_monotonic() {
        let mut handle = super::OcctKernelHandle::spawn();
        let box_h = handle
            .execute(&GeometryOp::Box {
                width: Value::Real(10.0),
                height: Value::Real(20.0),
                depth: Value::Real(30.0),
            })
            .unwrap();
        assert!(
            handle.query(&GeometryQuery::Volume(box_h.id)).is_ok(),
            "box should be queryable before reset"
        );

        // Reset over the actor channel — evicts the resident shape.
        handle.reset();

        // The old handle is now absent from the shape table, so `get_shape`
        // surfaces InvalidHandle (a clean error, not a crash or a stale read).
        let after = handle.query(&GeometryQuery::Volume(box_h.id));
        assert!(
            matches!(after, Err(reify_ir::QueryError::InvalidHandle(_))),
            "old handle must surface as InvalidHandle after reset (shape \
             evicted from the table), got {after:?}"
        );

        // A fresh execute gets a strictly-greater id — reset kept next_id
        // monotonic on the kernel thread, so ids are never reused.
        let box_h2 = handle
            .execute(&GeometryOp::Box {
                width: Value::Real(1.0),
                height: Value::Real(1.0),
                depth: Value::Real(1.0),
            })
            .unwrap();
        assert!(
            box_h2.id.0 > box_h.id.0,
            "next_id must stay monotonic across channel-routed reset: {} !> {}",
            box_h2.id.0,
            box_h.id.0
        );

        // Polymorphic path: reset() through &mut dyn GeometryKernel (the shape
        // the engine's Box<dyn GeometryKernel> reset call takes) must evict too.
        let dyn_kernel: &mut dyn reify_ir::GeometryKernel = &mut handle;
        dyn_kernel.reset();
        let after2 = handle.query(&GeometryQuery::Volume(box_h2.id));
        assert!(
            matches!(after2, Err(reify_ir::QueryError::InvalidHandle(_))),
            "polymorphic reset() through &mut dyn GeometryKernel must also \
             evict the shape, got {after2:?}"
        );
    }

    #[test]
    fn query_many_returns_ordered_values_for_heterogeneous_batch() {
        let handle = super::OcctKernelHandle::spawn();
        let op = GeometryOp::Box {
            width: Value::Real(10.0),
            height: Value::Real(20.0),
            depth: Value::Real(30.0),
        };
        let gh = handle.execute(&op).unwrap();
        let result = handle
            .query_many(&[
                GeometryQuery::Volume(gh.id),
                GeometryQuery::SurfaceArea(gh.id),
            ])
            .expect("query_many should succeed for valid handles");
        assert_eq!(result.len(), 2, "expected one Value per query");
        match (&result[0], &result[1]) {
            (Value::Real(vol), Value::Real(area)) => {
                // 10 * 20 * 30 = 6000
                assert!(
                    (vol - 6000.0).abs() < 1.0,
                    "expected volume ~6000, got {vol}"
                );
                // 2 * (10*20 + 10*30 + 20*30) = 2200
                assert!(
                    (area - 2200.0).abs() < 1.0,
                    "expected surface area ~2200, got {area}"
                );
            }
            other => panic!("expected two Value::Real, got {:?}", other),
        }
    }

    #[test]
    fn query_many_short_circuits_on_first_invalid_handle() {
        let handle = super::OcctKernelHandle::spawn();
        let op = GeometryOp::Box {
            width: Value::Real(10.0),
            height: Value::Real(20.0),
            depth: Value::Real(30.0),
        };
        let gh = handle.execute(&op).unwrap();
        let result = handle.query_many(&[
            GeometryQuery::Volume(GeometryHandleId(999)),
            GeometryQuery::SurfaceArea(gh.id),
        ]);
        assert!(result.is_err(), "query_many must propagate the bad handle");
        match result.unwrap_err() {
            reify_ir::QueryError::InvalidHandle(id) => {
                assert_eq!(id, GeometryHandleId(999));
            }
            other => panic!("expected InvalidHandle, got {:?}", other),
        }
    }

    #[test]
    fn query_many_empty_batch_returns_ok_empty_vec() {
        // The empty-batch fast path should return Ok(Vec::new()) without
        // routing through the actor channel. Observable behaviour is the
        // empty Ok result; the channel skip is documented in the doc-comment.
        let handle = super::OcctKernelHandle::spawn();
        let result = handle
            .query_many(&[])
            .expect("empty query_many should succeed");
        assert!(
            result.is_empty(),
            "empty batch should return empty Vec, got {:?}",
            result
        );
    }

    #[test]
    fn export_step_contains_iso_header() {
        let handle = super::OcctKernelHandle::spawn();
        let op = GeometryOp::Box {
            width: Value::Real(10.0),
            height: Value::Real(20.0),
            depth: Value::Real(30.0),
        };
        let gh = handle.execute(&op).unwrap();
        let mut buf = Vec::new();
        handle
            .export(gh.id, reify_ir::ExportFormat::Step, &mut buf)
            .unwrap();
        let content = String::from_utf8(buf).unwrap();
        assert!(
            content.contains("ISO-10303-21"),
            "STEP export should contain ISO-10303-21 header"
        );
    }

    /// STEP export must keep its unit DECLARATION and its coordinate PAYLOAD
    /// in agreement: reify model space is SI metres, the exported file declares
    /// `SI_UNIT(.MILLI.,.METRE.)`, so the coordinates it carries must be
    /// millimetres.
    ///
    /// The two halves DISAGREE today, and that asymmetry IS the defect. The
    /// declaration half already passes (OCCT writes the millimetre `SI_UNIT`
    /// by default); the payload half fails, because reify's SI-metre
    /// coordinates reach OCCT under a unit regime whose scale factor is 1.0
    /// and are emitted verbatim. A 30 mm cube is written as 0.030 and read by
    /// any STEP consumer as 30 µm — a 1000× shrink.
    ///
    /// The payload assertion is on the per-axis AABB extent parsed out of the
    /// `CARTESIAN_POINT` entities rather than on formatted coordinate strings,
    /// so it is invariant under everything that legitimately varies: OCCT's
    /// float formatting, entity ordering, box centring, and the
    /// AP203/AP214/AP242 schema selection the sibling tests flip. The 1e-6 mm
    /// bound is derived, not tuned: one exactly-representable ×1000 f64
    /// multiply (≤1 ulp ≈ 3.6e-15 mm at 30) plus OCCT's ≥12-significant-digit
    /// decimal round-trip (≤5e-11 mm) — about four orders of margin, and still
    /// four orders tighter than the 0.030-vs-30.0 gap it guards.
    #[test]
    fn export_step_declares_millimetres_and_scales_metre_coordinates() {
        // A 30 mm cube expressed in reify's SI-metre model space.
        let handle = super::OcctKernelHandle::spawn();
        let op = GeometryOp::Box {
            width: Value::Real(0.030),
            height: Value::Real(0.030),
            depth: Value::Real(0.030),
        };
        let gh = handle.execute(&op).unwrap();
        let mut buf = Vec::new();
        handle
            .export(gh.id, reify_ir::ExportFormat::Step, &mut buf)
            .unwrap();
        let content = String::from_utf8(buf).unwrap();

        // STEP wraps long lines, so `contains` (and any parse) on the raw text
        // is flaky — drop every ASCII whitespace byte first.
        let stripped: String = content
            .chars()
            .filter(|c| !c.is_ascii_whitespace())
            .collect();

        // (1) DECLARATION half — passes today.
        assert!(
            stripped.contains("SI_UNIT(.MILLI.,.METRE.)"),
            "STEP export should declare millimetres via SI_UNIT(.MILLI.,.METRE.)"
        );

        // (2) PAYLOAD half — every CARTESIAN_POINT coordinate triple, folded
        // into a per-axis AABB by the shared `cartesian_point_aabb` helper.
        let (min, max, n_points) = cartesian_point_aabb(&stripped);
        assert!(
            n_points > 0,
            "expected at least one 3D CARTESIAN_POINT in the STEP export, found none"
        );

        for (axis, name) in ["x", "y", "z"].into_iter().enumerate() {
            let extent = max[axis] - min[axis];
            assert!(
                (extent - 30.0).abs() < 1e-6,
                "a 30 mm cube (0.030 m in reify model space) should span 30.0 mm on {name} in a \
                 millimetre-declared STEP file, but the CARTESIAN_POINT AABB extent is {extent} \
                 (min {}, max {}) — declaration and payload disagree",
                min[axis],
                max[axis]
            );
        }
    }

    /// Fold every 3D `CARTESIAN_POINT` in a whitespace-stripped STEP file
    /// into a per-axis AABB, returning `(min, max, n_points)`.
    ///
    /// Entity body shape: `CARTESIAN_POINT('',(x,y,z))` once whitespace is
    /// stripped, so the coordinate list is the first parenthesised group after
    /// the entity's opening paren. Non-3D bodies (and unparsable ones) are
    /// skipped rather than failing, so a file mixing 2D and 3D points still
    /// yields a usable 3D box.
    ///
    /// Asserting on this box rather than on formatted coordinate strings keeps
    /// the callers invariant under everything that legitimately varies: OCCT's
    /// float formatting, entity ordering, shape centring, and the
    /// AP203/AP214/AP242 schema selection the sibling tests flip.
    fn cartesian_point_aabb(stripped: &str) -> ([f64; 3], [f64; 3], usize) {
        let mut min = [f64::INFINITY; 3];
        let mut max = [f64::NEG_INFINITY; 3];
        let mut n_points = 0usize;
        for tail in stripped.split("CARTESIAN_POINT(").skip(1) {
            let Some(open) = tail.find('(') else { continue };
            let Some(close) = tail[open + 1..].find(')') else {
                continue;
            };
            let coords: Vec<f64> = tail[open + 1..open + 1 + close]
                .split(',')
                .filter_map(|s| s.parse::<f64>().ok())
                .collect();
            if coords.len() != 3 {
                // Not a 3D point (or an unparsable body) — ignore it.
                continue;
            }
            n_points += 1;
            for axis in 0..3 {
                min[axis] = min[axis].min(coords[axis]);
                max[axis] = max[axis].max(coords[axis]);
            }
        }
        (min, max, n_points)
    }

    /// One `GLOBAL_UNIT_ASSIGNED_CONTEXT` entity, with its unit-reference list
    /// already resolved against the file's instance table.
    struct UnitContext {
        /// The context instance's own `#N` id, for failure messages.
        id: String,
        /// The whole instance record, for failure messages.
        record: String,
        /// EVERY unit instance this context references, resolved against the
        /// instance table, as `(#N, whole record)` pairs — the FULL list, not
        /// just the plane-angle subset. The full list is what the interesting
        /// failure needs: when a context reaches no plane-angle unit at all,
        /// the diagnostic question is "then what DID it reach?", and a
        /// pre-filtered subset answers that with an empty vector.
        units: Vec<(String, String)>,
    }

    impl UnitContext {
        /// Does this context reach a `PLANE_ANGLE_UNIT()` declaration?
        fn reaches_plane_angle_unit(&self) -> bool {
            self.units
                .iter()
                .any(|(_, body)| body.contains("PLANE_ANGLE_UNIT()"))
        }

        /// One line per resolved unit reference, for the failure message that
        /// fires when `reaches_plane_angle_unit` is false.
        fn resolved_units_summary(&self) -> String {
            if self.units.is_empty() {
                return "    (none — the context's reference list resolved to no \
                        instance in the file)"
                    .to_owned();
            }
            self.units
                .iter()
                .map(|(id, body)| format!("    {id} -> {body}"))
                .collect::<Vec<_>>()
                .join("\n")
        }
    }

    /// The result of walking a STEP file's plane-angle unit declarations.
    struct PlaneAngleAudit {
        /// EVERY instance record in the file that declares a
        /// `PLANE_ANGLE_UNIT()`, whether or not a context references it.
        records: Vec<String>,
        /// One entry per `GLOBAL_UNIT_ASSIGNED_CONTEXT`, in file order.
        contexts: Vec<UnitContext>,
    }

    /// Audit a STEP file's PLANE-ANGLE unit declarations BY ASSOCIATION.
    ///
    /// Takes ALREADY-STRIPPED text, like its sibling `cartesian_point_aabb` —
    /// the two helpers share one input convention so a caller cannot get them
    /// out of step, and each caller strips the file exactly once. STEP wraps
    /// long lines at ~72 chars, so every `contains`/parse here must run over
    /// whitespace-stripped text; that is the same idiom the sibling
    /// `export_step_declares_millimetres_and_scales_metre_coordinates` uses.
    /// Part-21 instances terminate at `;`, so splitting the stripped text on
    /// `;` yields one string per entity instance, which is what makes a
    /// per-record ("EVERY declaration is radians") assertion possible rather
    /// than a file-wide substring grep.
    ///
    /// Beyond that, this RESOLVES each `GLOBAL_UNIT_ASSIGNED_CONTEXT`'s unit
    /// reference list (`GLOBAL_UNIT_ASSIGNED_CONTEXT((#13,#17,#18))`) against
    /// the `#N =` instance table, so a caller can ask the sound question — does
    /// THIS context reference a radian plane-angle unit? — rather than the
    /// count proxy `n_plane_angle_unit == n_global_unit_assigned_context`.
    /// The proxy was measured-true against system OCCT 7.8 (each context gets
    /// its own unit entities) but STEP explicitly permits several contexts to
    /// share one unit instance, so an OCCT bump that deduped unit entities
    /// would have failed the proxy on a perfectly correct file. Walking the
    /// association is the form task #6344's runtime guard also plans to use.
    fn plane_angle_unit_audit(stripped: &str) -> PlaneAngleAudit {
        // Instance table: `#N` -> whole record (`#N=(...)`), for every Part-21
        // instance in the file. HEADER-section records carry no `#N =` and are
        // skipped by the `starts_with('#')` filter.
        let mut instances: std::collections::HashMap<String, String> =
            std::collections::HashMap::new();
        for rec in stripped.split(';') {
            if !rec.starts_with('#') {
                continue;
            }
            let Some(eq) = rec.find('=') else { continue };
            let id = rec[..eq].to_owned();
            if id[1..].chars().all(|c| c.is_ascii_digit()) && id.len() > 1 {
                instances.insert(id, rec.to_owned());
            }
        }

        let records: Vec<String> = stripped
            .split(';')
            .filter(|rec| rec.contains("PLANE_ANGLE_UNIT()"))
            .map(str::to_owned)
            .collect();

        let mut contexts: Vec<UnitContext> = Vec::new();
        for rec in stripped.split(';') {
            let Some(marker) = rec.find("GLOBAL_UNIT_ASSIGNED_CONTEXT(") else {
                continue;
            };
            let id = rec
                .find('=')
                .map_or_else(String::new, |eq| rec[..eq].to_owned());
            // The list is a flat `(#a,#b,#c)` — no nesting — so the first `)`
            // after the opening paren closes it.
            let tail = &rec[marker + "GLOBAL_UNIT_ASSIGNED_CONTEXT(".len()..];
            let refs = tail
                .strip_prefix('(')
                .and_then(|inner| inner.find(')').map(|close| &inner[..close]))
                .unwrap_or("");
            // Resolve the WHOLE reference list; `UnitContext` filters for the
            // plane-angle subset on demand and keeps the rest for diagnostics.
            let units = refs
                .split(',')
                .map(str::trim)
                .filter(|r| r.starts_with('#'))
                .filter_map(|r| instances.get(r).map(|body| (r.to_owned(), body.clone())))
                .collect();
            contexts.push(UnitContext {
                id,
                record: rec.to_owned(),
                units,
            });
        }

        PlaneAngleAudit { records, contexts }
    }

    /// The three arms that BOTH STEP angular pins share, applied to whichever
    /// write path `write_path` names (used only to make a failure say which of
    /// the two exports produced the file).
    ///
    /// Extracted so a wording fix, or an added arm — rejecting `.GRAD.`, say —
    /// lands in ONE place. Applied to only one of the two pins, such a change
    /// would silently leave the other write path's coverage behind, which is
    /// exactly the drift these pins exist to prevent.
    ///
    /// What deliberately stays at the call sites, because it is genuinely
    /// per-fixture: the BRep pin's `contexts.len() >= 2` multi-context guard
    /// and its `CONICAL_SURFACE` witness, and the wireframe pin's
    /// `TRIMMED_CURVE` / `PARAMETER_VALUE` / AABB payload checks. This helper
    /// asserts only the floor both share — at least ONE context exists — so
    /// arm (c)'s loop cannot pass vacuously over an empty vector.
    fn assert_plane_angle_units_are_si_radians(audit: &PlaneAngleAudit, write_path: &str) {
        // (a) EVERY plane-angle declaration is the SI radian.
        assert!(
            !audit.records.is_empty(),
            "{write_path} STEP export declared no PLANE_ANGLE_UNIT at all — the angular boundary \
             convention INV-AD-4 requires is missing from the file entirely"
        );
        for rec in &audit.records {
            assert!(
                rec.contains("SI_UNIT($,.RADIAN.)"),
                "every PLANE_ANGLE_UNIT record must declare SI radians as `SI_UNIT($,.RADIAN.)` \
                 (`$` is the null SI prefix; the `*` in the sibling `NAMED_UNIT(*)` is the \
                 redeclared marker, NOT a prefix), but this {write_path} record does not: {rec}"
            );

            // (b) No degree/grad CONVERSION_BASED_UNIT chain around a
            // PLANE-ANGLE unit — the shape a flipped angle unit must take.
            // Scoped to the plane-angle records deliberately: a file-wide count
            // would also see the LENGTH regime's chain the moment
            // `write.step.unit` moves off millimetres (OCCT spells inch/foot
            // that way), reddening an ANGLE pin for a LENGTH-side change.
            assert!(
                !rec.contains("CONVERSION_BASED_UNIT"),
                "a PLANE_ANGLE_UNIT must not be wrapped in a CONVERSION_BASED_UNIT chain (that is \
                 how a degree or grad plane-angle unit would be spelled), but this {write_path} \
                 record is: {rec}"
            );
        }

        // (c) ASSOCIATION — every unit context must actually REACH a
        // plane-angle unit. A context that declares none fails here even
        // though every record the file does declare is radians. Resolving the
        // reference list (rather than comparing counts) keeps this green if a
        // future OCCT shares one radian unit instance across all contexts,
        // which STEP permits.
        assert!(
            !audit.contexts.is_empty(),
            "expected at least one GLOBAL_UNIT_ASSIGNED_CONTEXT in the {write_path} export, found \
             none — the file carries no unit context to declare anything in, so the per-context \
             assertion below would pass vacuously"
        );
        for ctx in &audit.contexts {
            assert!(
                ctx.reaches_plane_angle_unit(),
                "{write_path} unit context {} references no PLANE_ANGLE_UNIT — it crosses the \
                 boundary with no declared angular convention. Context record: {}\nThe units it \
                 DID reach:\n{}",
                ctx.id,
                ctx.record,
                ctx.resolved_units_summary()
            );
        }
    }

    /// STEP export must declare SI RADIANS for plane angles in **every** unit
    /// context the file carries — the boundary declaration INV-AD-4 requires
    /// (`docs/prds/v0_6/angle-dimension-completion.md`, §9 B7; task #6184).
    ///
    /// REGRESSION PIN, GREEN ON ARRIVAL — exactly the posture task 6186's
    /// `export_unit_regime_e2e.rs` documents for the length regime. reify's
    /// angle declaration is correct *by construction*:
    /// `STEPConstruct_UnitContext::Init`, the sole builder of the write-side
    /// unit context, emits `SI_UNIT($,.RADIAN.)` unconditionally, with no
    /// preceding branch and no data dependence on any argument. There is no
    /// write-side angle knob to get wrong. So this pin does not prove a fix
    /// landed — it guards a correct-today property against an OCCT bump or a
    /// future writer-option change. **If it ever fails, the defect is real:
    /// debug it, do not relax it.**
    ///
    /// Why the universal quantifier rather than `content.contains(".RADIAN.")`:
    /// STEP scopes units *per representation_context*, and OCCT uses that
    /// freedom for every compound. Measured on this branch against the linked
    /// system OCCT 7.8, the two-cone union below emits **three**
    /// `GLOBAL_UNIT_ASSIGNED_CONTEXT` entities, each carrying its **own**
    /// `PLANE_ANGLE_UNIT` (`CONICAL_SURFACE` count 2). A substring grep is
    /// therefore satisfied by any one of N contexts and stays green through a
    /// *partial* flip, and it cannot detect a context that declares no
    /// plane-angle unit at all.
    ///
    /// The second hole is closed by ASSOCIATION, not by a count: the helper
    /// resolves each context's unit reference list and this test asserts every
    /// context reaches a radian plane-angle unit. An earlier draft compared
    /// `n_plane_angle_unit == n_global_unit_assigned_context`, which was
    /// measured-true here but is not a property STEP guarantees — several
    /// contexts may legally share ONE unit instance, so an OCCT bump that
    /// deduped unit entities would have reddened this pin on a correct file
    /// and pointed the reader at the export rather than at the proxy.
    ///
    /// Token detail a naive pin gets wrong: the emitted form is
    /// `SI_UNIT($,.RADIAN.)` — `$` is the *null SI prefix*. The `*` in the
    /// sibling `NAMED_UNIT(*)` is the *redeclared* marker, so a pin written
    /// for `SI_UNIT(*,.RADIAN.)` fails immediately.
    ///
    /// No runtime `OCCT_AVAILABLE` skip: inside a `#[cfg(all(test, has_occt))]`
    /// module such a check is tautological (`lib.rs`'s module docs, #6343), and
    /// build-time OCCT presence is gated before any compile by the OCCT arm of
    /// `scripts/check-manifold-deps.sh`.
    #[test]
    fn export_step_declares_si_radians_in_every_unit_context() {
        // Two disjoint 30 mm cones, unioned — a compound, which is what makes
        // OCCT emit more than one representation context.
        let handle = super::OcctKernelHandle::spawn();
        let cone = GeometryOp::Cone {
            bottom_radius: Value::Real(0.015),
            top_radius: Value::Real(0.0),
            height: Value::Real(0.030),
        };
        let left = handle.execute(&cone).unwrap();
        let right_untranslated = handle.execute(&cone).unwrap();
        let right = handle
            .execute(&GeometryOp::Translate {
                target: right_untranslated.id,
                dx: 0.100,
                dy: 0.0,
                dz: 0.0,
            })
            .unwrap();
        let union = handle
            .execute(&GeometryOp::Union {
                left: left.id,
                right: right.id,
            })
            .unwrap();

        let mut buf = Vec::new();
        handle
            .export(union.id, reify_ir::ExportFormat::Step, &mut buf)
            .unwrap();
        let content = String::from_utf8(buf).unwrap();
        let stripped: String = content
            .chars()
            .filter(|c| !c.is_ascii_whitespace())
            .collect();

        let audit = plane_angle_unit_audit(&stripped);

        // Arms (a) declaration-is-radians, (b) no CONVERSION_BASED_UNIT chain,
        // and (c) every context REACHES a plane-angle unit — shared verbatim
        // with the wireframe pin below.
        assert_plane_angle_units_are_si_radians(&audit, "BRep");

        // Per-fixture on top of (c): this pin is the one that exercises the
        // MULTI-context case, so a single-context file means the fixture has
        // degraded even though the shared arms passed.
        assert!(
            audit.contexts.len() >= 2,
            "this pin exists to exercise the MULTI-context case, but the file carries only {} \
             GLOBAL_UNIT_ASSIGNED_CONTEXT entity/entities — the two-cone union fixture no longer \
             produces a compound, so the pin has silently degraded into the single-context case",
            audit.contexts.len()
        );

        // (d) Branch-reached witness: the BRep angle path (GeomToStep_Make-
        // ConicalSurface writes a semi-angle) was actually exercised, so the
        // pin cannot degrade into testing an angle-free file.
        assert!(
            stripped.contains("CONICAL_SURFACE"),
            "expected the cone fixture to emit a CONICAL_SURFACE (whose semi-angle is the BRep \
             angular payload this pin covers); without it the file carries no angle at all and \
             the assertions above are vacuous"
        );
    }

    /// The WIREFRAME half of the STEP angular boundary — a cone-only fixture
    /// covers one of the two angle-bearing write paths, and this is the other
    /// (task #6184; `docs/prds/v0_6/angle-dimension-completion.md`, INV-AD-4).
    ///
    /// REGRESSION PIN, GREEN ON ARRIVAL, same posture as the BRep pin above
    /// and as 6186's `export_unit_regime_e2e.rs`: the declaration is correct by
    /// construction (`STEPConstruct_UnitContext::Init` emits
    /// `SI_UNIT($,.RADIAN.)` unconditionally). **If it ever fails, the defect
    /// is real: debug it, do not relax it.**
    ///
    /// Why this pin lives in the kernel crate rather than beside 6186's
    /// reify-eval `.ri` e2e: `STEPOutput.subject : Solid`
    /// (`crates/reify-compiler/stdlib/io.ri`) cannot carry a free curve, so the
    /// DSL route physically cannot reach the wireframe branch. The kernel API
    /// can — `OcctKernel::export`'s STEP arm hands `get_shape(handle)` straight
    /// to `ffi::export_step` with no solid restriction, so an `Arc` wire handle
    /// exports as a `GEOMETRIC_CURVE_SET`.
    ///
    /// The DECLARATION-vs-PAYLOAD half is what proves the trim parameter space
    /// really is radians. Measured on this branch against the linked system
    /// OCCT 7.8, the fixture emits
    /// `TRIMMED_CURVE('',#17,(#22,PARAMETER_VALUE(0.)),(#23,PARAMETER_VALUE(1.)),.T.,.PARAMETER.)`
    /// — the trim bounds are the arc's `start_angle`/`end_angle` verbatim, and
    /// `.PARAMETER.` says the parameter (not the redundant cartesian point) is
    /// the master representation. The arc's `CARTESIAN_POINT` AABB then has
    /// y-extent `20·sin(1)` = 16.829419696157930 mm.
    ///
    /// BOUND DERIVATION (do not retune) — the same basis 6186 documented: one
    /// exactly-representable ×1000 f64 multiply (≤1 ulp ≈ 3.6e-15 mm at 20)
    /// plus OCCT's ≥12-significant-digit decimal round-trip (≤2e-11 mm at 20),
    /// roughly five orders of margin inside the 1e-6 mm bound. And the bound is
    /// ~7 orders TIGHTER than the defect it guards: reading the same trim
    /// parameter as degrees puts the endpoint at `20·sin(1°)` = 0.349 mm, a
    /// 16.48 mm gap.
    ///
    /// The x-extent is deliberately NOT asserted: it is 20.0 under BOTH
    /// interpretations (the θ=0 endpoint and the circle centre both sit on it),
    /// so it cannot discriminate. Do not "strengthen" this test by adding it.
    ///
    /// Independently cross-checked at chartering against the redundant
    /// cartesian trim point (r·cos 1, r·sin 1) to 12 digits.
    #[test]
    fn export_step_declares_si_radians_for_wireframe_curve_parameters() {
        // A free 20 mm-radius arc swept through 1 RADIAN, expressed in reify's
        // SI-metre / SI-radian model space.
        let handle = super::OcctKernelHandle::spawn();
        let arc = handle
            .execute(&GeometryOp::Arc {
                center: [0.0, 0.0, 0.0],
                radius: 0.020,
                start_angle: 0.0,
                end_angle: 1.0,
                axis: [0.0, 0.0, 1.0],
            })
            .unwrap();

        let mut buf = Vec::new();
        handle
            .export(arc.id, reify_ir::ExportFormat::Step, &mut buf)
            .unwrap();
        let content = String::from_utf8(buf).unwrap();
        let stripped: String = content
            .chars()
            .filter(|c| !c.is_ascii_whitespace())
            .collect();

        let audit = plane_angle_unit_audit(&stripped);

        // Arms (a)/(b)/(c), the SAME universal quantifier the BRep pin applies,
        // so neither write path can drift from the other. No `contexts.len()`
        // floor beyond the helper's `>= 1` here: a single-context wireframe
        // file is legitimate, and the multi-context case is the BRep pin's job.
        assert_plane_angle_units_are_si_radians(&audit, "wireframe");

        // (d) DECLARATION-vs-PAYLOAD agreement. Locate the TRIMMED_CURVE
        // instance; a missing one would make everything below vacuous, so this
        // panics with the whole file rather than passing silently.
        let trimmed = stripped
            .split(';')
            .find(|rec| rec.contains("TRIMMED_CURVE("))
            .unwrap_or_else(|| {
                panic!(
                    "expected the arc fixture to emit a TRIMMED_CURVE (the wireframe angular \
                     payload this pin covers); without it the assertions above are vacuous. \
                     Stripped file:\n{stripped}"
                )
            });
        assert!(
            trimmed.contains(".PARAMETER."),
            "the trimmed curve's master representation must be `.PARAMETER.` (the trim bounds ARE \
             the angular parameters, not merely redundant cartesian points), but the record is: \
             {trimmed}"
        );

        // The record carries both trim bounds — PARAMETER_VALUE(0.) and
        // PARAMETER_VALUE(1.) — i.e. the arc's start_angle/end_angle verbatim,
        // in radians.
        let mut params: Vec<f64> = Vec::new();
        for tail in trimmed.split("PARAMETER_VALUE(").skip(1) {
            let Some(close) = tail.find(')') else {
                continue;
            };
            if let Ok(v) = tail[..close].parse::<f64>() {
                params.push(v);
            }
        }
        assert_eq!(
            params.len(),
            2,
            "expected two PARAMETER_VALUE trim bounds on the trimmed curve, parsed {params:?} \
             from: {trimmed}"
        );
        let hi = params.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
        let lo = params.iter().cloned().fold(f64::INFINITY, f64::min);
        assert!(
            (lo - 0.0).abs() < 1e-12 && (hi - 1.0).abs() < 1e-12,
            "the arc was built with start_angle 0.0 rad and end_angle 1.0 rad, so its STEP trim \
             bounds must be 0.0 and 1.0 (parameter space is radians, unscaled); parsed \
             {params:?} instead — a degree rescale would put the upper bound at 57.29578"
        );

        // The payload half: 20·sin(1) mm on y. Under a degree misreading the
        // same arc would span 20·sin(1°) = 0.349 mm.
        let (min, max, n_points) = cartesian_point_aabb(&stripped);
        assert!(
            n_points > 0,
            "expected at least one 3D CARTESIAN_POINT in the wireframe STEP export, found none"
        );
        let expected_y = 20.0 * (1.0_f64).sin();
        let y_extent = max[1] - min[1];
        assert!(
            (y_extent - expected_y).abs() < 1e-6,
            "a 20 mm-radius arc swept through 1 RADIAN should span 20·sin(1) = {expected_y} mm on \
             y, but the CARTESIAN_POINT AABB y-extent is {y_extent} (min {}, max {}) — \
             declaration and payload disagree. 20·sin(1°) = {} is what a degree misreading of the \
             trim parameter would give.",
            min[1],
            max[1],
            20.0 * (1.0_f64).to_radians().sin()
        );
    }

    /// Real-OCCT schema selection through the new `export_with_options`.
    ///
    /// Asserts the declared STEP schema reaches the OCCT writer and is
    /// observable in the written FILE_SCHEMA. The assertions use the actual
    /// OCCT EXPRESS schema identifiers (verified against linked OCCT 7.9.3):
    /// AP203 → `CONFIG_CONTROL_DESIGN`, AP214 → `AUTOMOTIVE_DESIGN`,
    /// AP242 → a name containing `AP242`. The literal token "AP203" is never
    /// written by OCCT, so we assert the EXPRESS schema name instead.
    ///
    /// All three exports run in one process, so they share the process-global
    /// `write.step.schema` static — this is exactly the case the per-call
    /// `Interface_Static::SetCVal` must make deterministic.
    #[test]
    fn export_with_options_selects_step_schema() {
        use reify_ir::{ExportFormat, ExportOptions, StepSchema};
        let handle = super::OcctKernelHandle::spawn();
        let op = GeometryOp::Box {
            width: Value::Real(10.0),
            height: Value::Real(20.0),
            depth: Value::Real(30.0),
        };
        let gh = handle.execute(&op).unwrap();

        // (a) AP203 → CONFIG_CONTROL_DESIGN, never AUTOMOTIVE_DESIGN.
        let mut buf_203 = Vec::new();
        let w_203 = handle
            .export_with_options(
                gh.id,
                ExportFormat::Step,
                &ExportOptions {
                    step_schema: StepSchema::Ap203,
                    ..ExportOptions::default()
                },
                &mut buf_203,
            )
            .unwrap();
        let content_203 = String::from_utf8(buf_203).unwrap();
        assert!(
            content_203.contains("CONFIG_CONTROL_DESIGN"),
            "AP203 export must write the CONFIG_CONTROL_DESIGN EXPRESS schema"
        );
        assert!(
            !content_203.contains("AUTOMOTIVE_DESIGN"),
            "AP203 export must NOT write the AP214 AUTOMOTIVE_DESIGN schema"
        );
        assert!(w_203.is_empty(), "AP203 export raises no warnings");

        // (b) default (AP214) → AUTOMOTIVE_DESIGN.
        let mut buf_def = Vec::new();
        let w_def = handle
            .export_with_options(
                gh.id,
                ExportFormat::Step,
                &ExportOptions::default(),
                &mut buf_def,
            )
            .unwrap();
        let content_def = String::from_utf8(buf_def).unwrap();
        assert!(
            content_def.contains("AUTOMOTIVE_DESIGN"),
            "default (AP214) export must write the AUTOMOTIVE_DESIGN schema"
        );
        assert!(w_def.is_empty());

        // (c) The schema really changed: AP203 and AP214 bytes differ.
        assert_ne!(
            content_203, content_def,
            "AP203 and AP214 exports must differ in their FILE_SCHEMA"
        );

        // (d) AP242 → schema name contains "AP242". OCCT 7.9.3 supports
        // AP242DIS, so the happy path succeeds with no fallback warning.
        let mut buf_242 = Vec::new();
        let w_242 = handle
            .export_with_options(
                gh.id,
                ExportFormat::Step,
                &ExportOptions {
                    step_schema: StepSchema::Ap242,
                    ..ExportOptions::default()
                },
                &mut buf_242,
            )
            .unwrap();
        let content_242 = String::from_utf8(buf_242).unwrap();
        assert!(
            content_242.contains("AP242"),
            "AP242 export must write a schema name containing AP242"
        );
        assert!(
            w_242.is_empty(),
            "OCCT 7.9.3 supports AP242DIS — AP242 happy path raises no warning"
        );
    }

    /// The plain `export(Step)` path is unchanged by the options plumbing: it
    /// still writes the ISO-10303-21 header and defaults to the AP214
    /// AUTOMOTIVE_DESIGN schema (the per-call SetCVal on the default path
    /// keeps this deterministic even after a prior AP203 export in-process).
    #[test]
    fn plain_export_step_still_writes_default_ap214_schema() {
        let handle = super::OcctKernelHandle::spawn();
        let op = GeometryOp::Box {
            width: Value::Real(10.0),
            height: Value::Real(20.0),
            depth: Value::Real(30.0),
        };
        let gh = handle.execute(&op).unwrap();
        let mut buf = Vec::new();
        handle
            .export(gh.id, reify_ir::ExportFormat::Step, &mut buf)
            .unwrap();
        let content = String::from_utf8(buf).unwrap();
        assert!(
            content.contains("ISO-10303-21"),
            "plain export must still write the ISO-10303-21 header"
        );
        assert!(
            content.contains("AUTOMOTIVE_DESIGN"),
            "plain export must default to the AP214 AUTOMOTIVE_DESIGN schema"
        );
    }

    #[test]
    fn export_unsupported_format_returns_error() {
        // Stl is now wired; use Obj (explicitly unsupported) to pin the
        // error-path contract for an unsupported format.
        let handle = super::OcctKernelHandle::spawn();
        let op = GeometryOp::Box {
            width: Value::Real(10.0),
            height: Value::Real(20.0),
            depth: Value::Real(30.0),
        };
        let gh = handle.execute(&op).unwrap();
        let mut buf = Vec::new();
        let result = handle.export(gh.id, reify_ir::ExportFormat::Obj, &mut buf);
        assert!(result.is_err());
    }

    #[test]
    fn handle_implements_geometry_kernel_trait() {
        use reify_ir::GeometryKernel;
        let mut handle = super::OcctKernelHandle::spawn();
        // Use it through the trait interface as Box<dyn GeometryKernel>
        let kernel: &mut dyn GeometryKernel = &mut handle;
        let op = GeometryOp::Box {
            width: Value::Real(5.0),
            height: Value::Real(5.0),
            depth: Value::Real(5.0),
        };
        let gh = kernel.execute(&op).unwrap();
        assert_eq!(gh.id, GeometryHandleId(1));
    }

    #[test]
    fn tessellate_returns_valid_mesh() {
        let handle = super::OcctKernelHandle::spawn();
        let op = GeometryOp::Box {
            width: Value::Real(10.0),
            height: Value::Real(20.0),
            depth: Value::Real(30.0),
        };
        let gh = handle.execute(&op).unwrap();
        let mesh = handle.tessellate(gh.id, 0.1).unwrap();
        assert!(!mesh.vertices.is_empty(), "mesh should have vertices");
        assert!(!mesh.indices.is_empty(), "mesh should have indices");
        assert_eq!(
            mesh.indices.len() % 3,
            0,
            "indices should be divisible by 3 (triangles)"
        );
        assert!(mesh.normals.is_some(), "mesh should have normals");
    }

    #[test]
    fn chamfer_all_edges_through_channel() {
        let handle = super::OcctKernelHandle::spawn();
        let box_op = GeometryOp::Box {
            width: Value::Real(10.0),
            height: Value::Real(10.0),
            depth: Value::Real(10.0),
        };
        let gh = handle.execute(&box_op).unwrap();
        let chamfer_op = GeometryOp::Chamfer {
            target: gh.id,
            edges: vec![],
            distance: Value::Real(1.0),
        };
        let result = handle.execute(&chamfer_op);
        assert!(
            result.is_ok(),
            "chamfer should succeed, got: {:?}",
            result.unwrap_err()
        );
        let chamfered = result.unwrap();
        assert!(
            chamfered.id.0 > 0,
            "chamfered shape should have a valid handle id, got {:?}",
            chamfered.id
        );
        // Verify the resulting shape is exportable and topologically valid by
        // exporting to STEP and checking the ISO-10303-21 header is present.
        let mut buf = Vec::new();
        handle
            .export(chamfered.id, reify_ir::ExportFormat::Step, &mut buf)
            .expect("chamfered shape should be exportable to STEP");
        let content = String::from_utf8(buf).expect("STEP output should be valid UTF-8");
        assert!(
            content.contains("ISO-10303-21"),
            "chamfered STEP export should contain ISO-10303-21 header"
        );
    }

    #[test]
    fn export_invalid_handle_returns_error() {
        let handle = super::OcctKernelHandle::spawn();
        let mut buf = Vec::new();
        let result = handle.export(
            GeometryHandleId(999),
            reify_ir::ExportFormat::Step,
            &mut buf,
        );
        assert!(result.is_err());
        match result.unwrap_err() {
            reify_ir::ExportError::InvalidHandle(id) => {
                assert_eq!(id, GeometryHandleId(999));
            }
            other => panic!("expected InvalidHandle, got {:?}", other),
        }
    }

    #[test]
    fn tessellate_invalid_handle_returns_error() {
        let handle = super::OcctKernelHandle::spawn();
        let result = handle.tessellate(GeometryHandleId(999), 0.1);
        assert!(result.is_err());
        match result.unwrap_err() {
            reify_ir::TessError::InvalidHandle(id) => {
                assert_eq!(id, GeometryHandleId(999));
            }
            other => panic!("expected InvalidHandle, got {:?}", other),
        }
    }

    #[test]
    fn drop_handle_exits_thread_cleanly() {
        let handle = super::OcctKernelHandle::spawn();
        // Execute an op to ensure kernel thread is alive and working
        let op = GeometryOp::Box {
            width: Value::Real(1.0),
            height: Value::Real(1.0),
            depth: Value::Real(1.0),
        };
        handle.execute(&op).unwrap();
        // Drop should not panic — thread exits cleanly
        drop(handle);
    }

    #[test]
    fn multiple_sequential_handles() {
        for _ in 0..3 {
            let handle = super::OcctKernelHandle::spawn();
            let op = GeometryOp::Box {
                width: Value::Real(5.0),
                height: Value::Real(5.0),
                depth: Value::Real(5.0),
            };
            let gh = handle.execute(&op).unwrap();
            // Each handle starts with its own id counter
            assert_eq!(gh.id, GeometryHandleId(1));
            drop(handle);
        }
    }

    #[test]
    fn multi_operation_sequence() {
        let handle = super::OcctKernelHandle::spawn();

        // Create box
        let box_h = handle
            .execute(&GeometryOp::Box {
                width: Value::Real(100.0),
                height: Value::Real(60.0),
                depth: Value::Real(10.0),
            })
            .unwrap();
        assert_eq!(box_h.id, GeometryHandleId(1));

        // Create cylinder
        let cyl_h = handle
            .execute(&GeometryOp::Cylinder {
                radius: Value::Real(5.0),
                height: Value::Real(20.0),
            })
            .unwrap();
        assert_eq!(cyl_h.id, GeometryHandleId(2));

        // Boolean union
        let union_h = handle
            .execute(&GeometryOp::Union {
                left: box_h.id,
                right: cyl_h.id,
            })
            .unwrap();
        assert_eq!(union_h.id, GeometryHandleId(3));

        // Fillet
        let fillet_h = handle
            .execute(&GeometryOp::Fillet {
                target: union_h.id,
                edges: vec![],
                radius: Value::Real(2.0),
            })
            .unwrap();
        assert_eq!(fillet_h.id, GeometryHandleId(4));

        // Query volume
        let vol = handle
            .query(&reify_ir::GeometryQuery::Volume(fillet_h.id))
            .unwrap();
        match vol {
            Value::Real(v) => assert!(v > 0.0, "volume should be positive, got {v}"),
            other => panic!("expected Value::Real, got {:?}", other),
        }

        // Tessellate
        let mesh = handle.tessellate(fillet_h.id, 0.1).unwrap();
        assert!(!mesh.vertices.is_empty());
        assert!(!mesh.indices.is_empty());

        // Export STEP
        let mut buf = Vec::new();
        handle
            .export(fillet_h.id, reify_ir::ExportFormat::Step, &mut buf)
            .unwrap();
        let content = String::from_utf8(buf).unwrap();
        assert!(content.contains("ISO-10303-21"));
    }

    // --- Async companion method tests (step-21) ---

    #[tokio::test]
    async fn execute_async_creates_box() {
        let handle = super::OcctKernelHandle::spawn();
        let op = GeometryOp::Box {
            width: Value::Real(10.0),
            height: Value::Real(20.0),
            depth: Value::Real(30.0),
        };
        let result = handle.execute_async(&op).await.unwrap();
        assert_eq!(result.id, GeometryHandleId(1));
        assert_eq!(result.repr, Some(BRepKind::Solid));
    }

    #[tokio::test]
    async fn query_async_volume() {
        let handle = super::OcctKernelHandle::spawn();
        let op = GeometryOp::Box {
            width: Value::Real(10.0),
            height: Value::Real(20.0),
            depth: Value::Real(30.0),
        };
        let gh = handle.execute_async(&op).await.unwrap();
        let result = handle
            .query_async(&GeometryQuery::Volume(gh.id))
            .await
            .unwrap();
        match result {
            Value::Real(v) => {
                assert!((v - 6000.0).abs() < 1.0, "expected ~6000, got {v}");
            }
            other => panic!("expected Value::Real, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn query_async_invalid_handle() {
        let handle = super::OcctKernelHandle::spawn();
        let result = handle
            .query_async(&GeometryQuery::Volume(GeometryHandleId(999)))
            .await;
        assert!(result.is_err());
        match result.unwrap_err() {
            reify_ir::QueryError::InvalidHandle(id) => {
                assert_eq!(id, GeometryHandleId(999));
            }
            other => panic!("expected InvalidHandle, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn export_async_returns_step_bytes() {
        let handle = super::OcctKernelHandle::spawn();
        let op = GeometryOp::Box {
            width: Value::Real(10.0),
            height: Value::Real(20.0),
            depth: Value::Real(30.0),
        };
        let gh = handle.execute_async(&op).await.unwrap();
        let bytes = handle
            .export_async(gh.id, reify_ir::ExportFormat::Step)
            .await
            .unwrap();
        let content = String::from_utf8(bytes).unwrap();
        assert!(
            content.contains("ISO-10303-21"),
            "STEP export should contain ISO-10303-21 header"
        );
    }

    #[tokio::test]
    async fn export_async_invalid_handle() {
        let handle = super::OcctKernelHandle::spawn();
        let result = handle
            .export_async(GeometryHandleId(999), reify_ir::ExportFormat::Step)
            .await;
        assert!(result.is_err());
        match result.unwrap_err() {
            reify_ir::ExportError::InvalidHandle(id) => {
                assert_eq!(id, GeometryHandleId(999));
            }
            other => panic!("expected InvalidHandle, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn tessellate_async_returns_valid_mesh() {
        let handle = super::OcctKernelHandle::spawn();
        let op = GeometryOp::Box {
            width: Value::Real(10.0),
            height: Value::Real(20.0),
            depth: Value::Real(30.0),
        };
        let gh = handle.execute_async(&op).await.unwrap();
        let mesh = handle.tessellate_async(gh.id, 0.1).await.unwrap();
        assert!(!mesh.vertices.is_empty(), "mesh should have vertices");
        assert!(!mesh.indices.is_empty(), "mesh should have indices");
        assert_eq!(
            mesh.indices.len() % 3,
            0,
            "indices should be divisible by 3 (triangles)"
        );
        assert!(mesh.normals.is_some(), "mesh should have normals");
    }

    #[tokio::test]
    async fn async_multi_op_sequence() {
        let handle = super::OcctKernelHandle::spawn();

        // Create box
        let box_h = handle
            .execute_async(&GeometryOp::Box {
                width: Value::Real(100.0),
                height: Value::Real(60.0),
                depth: Value::Real(10.0),
            })
            .await
            .unwrap();
        assert_eq!(box_h.id, GeometryHandleId(1));

        // Create cylinder
        let cyl_h = handle
            .execute_async(&GeometryOp::Cylinder {
                radius: Value::Real(5.0),
                height: Value::Real(20.0),
            })
            .await
            .unwrap();
        assert_eq!(cyl_h.id, GeometryHandleId(2));

        // Boolean union
        let union_h = handle
            .execute_async(&GeometryOp::Union {
                left: box_h.id,
                right: cyl_h.id,
            })
            .await
            .unwrap();
        assert_eq!(union_h.id, GeometryHandleId(3));

        // Fillet
        let fillet_h = handle
            .execute_async(&GeometryOp::Fillet {
                target: union_h.id,
                edges: vec![],
                radius: Value::Real(2.0),
            })
            .await
            .unwrap();
        assert_eq!(fillet_h.id, GeometryHandleId(4));

        // Query volume via async
        let vol = handle
            .query_async(&GeometryQuery::Volume(fillet_h.id))
            .await
            .unwrap();
        match vol {
            Value::Real(v) => assert!(v > 0.0, "volume should be positive, got {v}"),
            other => panic!("expected Value::Real, got {:?}", other),
        }

        // Tessellate via async
        let mesh = handle.tessellate_async(fillet_h.id, 0.1).await.unwrap();
        assert!(!mesh.vertices.is_empty());
        assert!(!mesh.indices.is_empty());

        // Export STEP via async (returns Vec<u8>)
        let bytes = handle
            .export_async(fillet_h.id, reify_ir::ExportFormat::Step)
            .await
            .unwrap();
        let content = String::from_utf8(bytes).unwrap();
        assert!(content.contains("ISO-10303-21"));
    }

    // --- Async-safe Drop and shutdown tests (step-23) ---

    #[tokio::test]
    async fn drop_in_async_context_does_not_block() {
        // Dropping OcctKernelHandle inside an async context must not block
        // the tokio worker thread (i.e., must not call thread.join()).
        let handle = super::OcctKernelHandle::spawn();
        let op = GeometryOp::Box {
            width: Value::Real(5.0),
            height: Value::Real(5.0),
            depth: Value::Real(5.0),
        };
        handle.execute_async(&op).await.unwrap();
        // Drop inside async context — must complete without blocking
        drop(handle);
    }

    #[tokio::test]
    async fn shutdown_completes_cleanly() {
        let handle = super::OcctKernelHandle::spawn();
        let op = GeometryOp::Box {
            width: Value::Real(5.0),
            height: Value::Real(5.0),
            depth: Value::Real(5.0),
        };
        handle.execute_async(&op).await.unwrap();
        // Explicit async shutdown — should complete cleanly
        handle.shutdown().await;
        // After shutdown, spawning a new handle should work (kernel thread exited)
        let handle2 = super::OcctKernelHandle::spawn();
        let result = handle2.execute_async(&op).await.unwrap();
        assert_eq!(result.id, GeometryHandleId(1)); // fresh kernel, fresh ids
    }

    // --- Warm-start tests ---

    #[test]
    fn handle_warm_state_returns_some_after_op() {
        use reify_ir::WarmStartable;
        let handle = super::OcctKernelHandle::spawn();
        let op = GeometryOp::Box {
            width: Value::Real(10.0),
            height: Value::Real(20.0),
            depth: Value::Real(30.0),
        };
        handle.execute(&op).unwrap();
        let state = handle.warm_state();
        assert!(state.is_some(), "handle with shapes should have warm state");
        assert!(
            state.unwrap().estimated_size_bytes() > 0,
            "estimated size should be positive"
        );
    }

    #[test]
    fn cross_handle_warm_start_transfer() {
        use reify_ir::WarmStartable;
        // Handle A: create box
        let handle_a = super::OcctKernelHandle::spawn();
        let op = GeometryOp::Box {
            width: Value::Real(10.0),
            height: Value::Real(20.0),
            depth: Value::Real(30.0),
        };
        handle_a.execute(&op).unwrap();

        // Extract warm state from handle A
        let state = handle_a.warm_state().expect("should have warm state");

        // Handle B: restore warm state
        let mut handle_b = super::OcctKernelHandle::spawn();
        handle_b.with_warm_state(state);

        // Query volume on handle B using handle ID 1
        let vol = handle_b
            .query(&GeometryQuery::Volume(GeometryHandleId(1)))
            .unwrap();
        match vol {
            Value::Real(v) => {
                assert!((v - 6000.0).abs() < 1.0, "expected volume ~6000, got {v}");
            }
            other => panic!("expected Value::Real, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn async_warm_start_roundtrip() {
        let handle_a = super::OcctKernelHandle::spawn();
        let op = GeometryOp::Box {
            width: Value::Real(10.0),
            height: Value::Real(20.0),
            depth: Value::Real(30.0),
        };
        handle_a.execute_async(&op).await.unwrap();

        // Extract warm state via async
        let state = handle_a
            .warm_state_async()
            .await
            .expect("should have warm state");

        // Restore on new handle via async
        let handle_b = super::OcctKernelHandle::spawn();
        handle_b.with_warm_state_async(state).await;

        // Query volume via async
        let vol = handle_b
            .query_async(&GeometryQuery::Volume(GeometryHandleId(1)))
            .await
            .unwrap();
        match vol {
            Value::Real(v) => {
                assert!((v - 6000.0).abs() < 1.0, "expected volume ~6000, got {v}");
            }
            other => panic!("expected Value::Real, got {:?}", other),
        }
    }

    #[test]
    fn handle_warm_state_none_on_empty_kernel() {
        use reify_ir::WarmStartable;
        let handle = super::OcctKernelHandle::spawn();
        // No ops executed — warm_state should return None
        let state = handle.warm_state();
        assert!(state.is_none(), "empty kernel should have no warm state");
    }

    #[tokio::test]
    async fn warm_startable_trait_safe_in_async_context() {
        use reify_ir::WarmStartable;
        // Calling the sync WarmStartable trait methods from an async context
        // must not panic (previously used blocking_send/blocking_recv which
        // panicked inside tokio runtime).
        let handle_a = super::OcctKernelHandle::spawn();
        let op = GeometryOp::Box {
            width: Value::Real(10.0),
            height: Value::Real(20.0),
            depth: Value::Real(30.0),
        };
        handle_a.execute_async(&op).await.unwrap();

        // Call sync warm_state() from async context — must not panic
        let state = handle_a.warm_state().expect("should have warm state");

        // Call sync with_warm_state() from async context — must not panic
        let mut handle_b = super::OcctKernelHandle::spawn();
        handle_b.with_warm_state(state);

        // Verify restored state works
        let vol = handle_b
            .query_async(&GeometryQuery::Volume(GeometryHandleId(1)))
            .await
            .unwrap();
        match vol {
            Value::Real(v) => {
                assert!((v - 6000.0).abs() < 1.0, "expected volume ~6000, got {v}");
            }
            other => panic!("expected Value::Real, got {:?}", other),
        }
    }

    #[test]
    fn concurrent_export_step_from_multiple_handles() {
        // Regression test: spawn N OcctKernelHandle instances, each on its own
        // dedicated thread, create a box on each, and export to STEP concurrently.
        // This reliably triggers the OCCT global STEP writer state race condition
        // when the C++ export_step() function is not guarded by a mutex.
        const N: usize = 4;
        std::thread::scope(|s| {
            let threads: Vec<_> = (0..N)
                .map(|_| {
                    s.spawn(|| {
                        let handle = super::OcctKernelHandle::spawn();
                        let op = GeometryOp::Box {
                            width: Value::Real(10.0),
                            height: Value::Real(20.0),
                            depth: Value::Real(30.0),
                        };
                        let gh = handle.execute(&op).unwrap();
                        let mut buf = Vec::new();
                        handle
                            .export(gh.id, reify_ir::ExportFormat::Step, &mut buf)
                            .expect("STEP export should succeed under concurrent access");
                        let content = String::from_utf8(buf).unwrap();
                        assert!(
                            content.contains("ISO-10303-21"),
                            "STEP export should contain ISO-10303-21 header"
                        );
                    })
                })
                .collect();
            for t in threads {
                t.join().unwrap();
            }
        });
    }

    #[test]
    fn kernel_thread_responsive_after_errors() {
        let handle = super::OcctKernelHandle::spawn();

        // 1. Create a valid box (id=1) — should succeed
        let box_h = handle
            .execute(&GeometryOp::Box {
                width: Value::Real(10.0),
                height: Value::Real(20.0),
                depth: Value::Real(30.0),
            })
            .unwrap();
        assert_eq!(box_h.id, GeometryHandleId(1));

        // 2. Union with invalid handles — should return Err(InvalidReference)
        let union_result = handle.execute(&GeometryOp::Union {
            left: GeometryHandleId(999),
            right: GeometryHandleId(998),
        });
        assert!(union_result.is_err());

        // 3. Box with zero width — should return Err(OperationFailed) from validation
        let zero_result = handle.execute(&GeometryOp::Box {
            width: Value::Real(0.0),
            height: Value::Real(10.0),
            depth: Value::Real(10.0),
        });
        assert!(zero_result.is_err());

        // 4. Query volume on invalid handle — should return Err
        let query_result = handle.query(&GeometryQuery::Volume(GeometryHandleId(999)));
        assert!(query_result.is_err());

        // 5. Create another valid box — proves kernel thread is still alive
        let box2_h = handle
            .execute(&GeometryOp::Box {
                width: Value::Real(10.0),
                height: Value::Real(20.0),
                depth: Value::Real(30.0),
            })
            .unwrap();

        // 6. Query volume of the new box — should return correct value
        let vol = handle.query(&GeometryQuery::Volume(box2_h.id)).unwrap();
        match vol {
            Value::Real(v) => {
                assert!((v - 6000.0).abs() < 1.0, "expected ~6000, got {v}");
            }
            other => panic!("expected Value::Real, got {:?}", other),
        }
    }

    #[test]
    fn sync_drop_still_joins_thread() {
        // Sync (non-async) Drop should preserve existing join behavior
        let handle = super::OcctKernelHandle::spawn();
        let op = GeometryOp::Box {
            width: Value::Real(5.0),
            height: Value::Real(5.0),
            depth: Value::Real(5.0),
        };
        handle.execute(&op).unwrap();
        // Drop outside async context — should join the thread
        drop(handle);
        // No panic means success
    }

    // --- Topology extraction through the handle channel (step-21) ---

    /// Helper: build a box on a fresh handle and return both.
    fn handle_with_box(w: f64, h: f64, d: f64) -> (super::OcctKernelHandle, GeometryHandleId) {
        let handle = super::OcctKernelHandle::spawn();
        let gh = handle
            .execute(&GeometryOp::Box {
                width: Value::Real(w),
                height: Value::Real(h),
                depth: Value::Real(d),
            })
            .expect("Box execute should succeed");
        (handle, gh.id)
    }

    /// Assert all ids in `ids` are pairwise distinct, none are
    /// `GeometryHandleId::INVALID`, and none equal `excluded`.
    fn assert_distinct_valid(ids: &[GeometryHandleId], excluded: GeometryHandleId) {
        for id in ids {
            assert_ne!(*id, GeometryHandleId::INVALID, "id should not be INVALID");
            assert_ne!(*id, excluded, "extracted id should not equal source handle");
        }
        let mut seen = std::collections::HashSet::new();
        for id in ids {
            assert!(seen.insert(*id), "duplicate id in extracted vec: {id:?}");
        }
    }

    #[test]
    fn extract_edges_through_handle_channel_returns_twelve_handles() {
        let (handle, box_id) = handle_with_box(10.0, 20.0, 30.0);
        let edges = handle
            .extract_edges(box_id)
            .expect("extract_edges through channel should succeed");
        assert_eq!(
            edges.len(),
            12,
            "a box should have 12 edges, got {}",
            edges.len()
        );
        assert_distinct_valid(&edges, box_id);
    }

    #[tokio::test]
    async fn extract_edges_async_through_handle_channel_returns_twelve_handles() {
        let handle = super::OcctKernelHandle::spawn();
        let gh = handle
            .execute_async(&GeometryOp::Box {
                width: Value::Real(10.0),
                height: Value::Real(20.0),
                depth: Value::Real(30.0),
            })
            .await
            .unwrap();
        let edges = handle
            .extract_edges_async(gh.id)
            .await
            .expect("extract_edges_async through channel should succeed");
        assert_eq!(
            edges.len(),
            12,
            "a box should have 12 edges, got {}",
            edges.len()
        );
        assert_distinct_valid(&edges, gh.id);
    }

    #[test]
    fn extract_faces_through_handle_channel_returns_six_handles() {
        let (handle, box_id) = handle_with_box(10.0, 20.0, 30.0);
        let faces = handle
            .extract_faces(box_id)
            .expect("extract_faces through channel should succeed");
        assert_eq!(
            faces.len(),
            6,
            "a box should have 6 faces, got {}",
            faces.len()
        );
        assert_distinct_valid(&faces, box_id);
    }

    #[tokio::test]
    async fn extract_faces_async_through_handle_channel_returns_six_handles() {
        let handle = super::OcctKernelHandle::spawn();
        let gh = handle
            .execute_async(&GeometryOp::Box {
                width: Value::Real(10.0),
                height: Value::Real(20.0),
                depth: Value::Real(30.0),
            })
            .await
            .unwrap();
        let faces = handle
            .extract_faces_async(gh.id)
            .await
            .expect("extract_faces_async through channel should succeed");
        assert_eq!(
            faces.len(),
            6,
            "a box should have 6 faces, got {}",
            faces.len()
        );
        assert_distinct_valid(&faces, gh.id);
    }

    #[test]
    fn extract_vertices_through_handle_channel_returns_eight_handles() {
        let (handle, box_id) = handle_with_box(10.0, 20.0, 30.0);
        let vertices = handle
            .extract_vertices(box_id)
            .expect("extract_vertices through channel should succeed");
        assert_eq!(
            vertices.len(),
            8,
            "a box should have 8 vertices, got {}",
            vertices.len()
        );
        assert_distinct_valid(&vertices, box_id);
    }

    #[tokio::test]
    async fn extract_vertices_async_through_handle_channel_returns_eight_handles() {
        let handle = super::OcctKernelHandle::spawn();
        let gh = handle
            .execute_async(&GeometryOp::Box {
                width: Value::Real(10.0),
                height: Value::Real(20.0),
                depth: Value::Real(30.0),
            })
            .await
            .unwrap();
        let vertices = handle
            .extract_vertices_async(gh.id)
            .await
            .expect("extract_vertices_async through channel should succeed");
        assert_eq!(
            vertices.len(),
            8,
            "a box should have 8 vertices, got {}",
            vertices.len()
        );
        assert_distinct_valid(&vertices, gh.id);
    }
}
