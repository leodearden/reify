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
    /// Task 5212: evict every resident native shape (+ derived caches) on the
    /// kernel thread while keeping `next_id` monotonic, bounding OCCT native
    /// memory across GUI whole-file reloads. Routed here because `OcctKernel`
    /// is `!Send`. Reply is `()` (fire-and-confirm).
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
    /// the kernel — the all-edges path is `FilletWithHistory`.
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
    /// by the kernel — the all-edges path is `ChamferWithHistory`.
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
    /// Extrude `profile` by `distance` (SI metres) with sweep history.
    ExtrudeWithHistory {
        profile: GeometryHandleId,
        distance: f64,
        reply: oneshot::Sender<Result<(GeometryHandle, SweepOpHistoryRecords), GeometryError>>,
    },
    /// Revolve `profile` about an axis with sweep history.
    RevolveWithHistory {
        profile: GeometryHandleId,
        axis_origin: [f64; 3],
        axis_dir: [f64; 3],
        angle_rad: f64,
        reply: oneshot::Sender<Result<(GeometryHandle, SweepOpHistoryRecords), GeometryError>>,
    },
    /// Sweep `profile` along `path` with sweep history.
    SweepWithHistory {
        profile: GeometryHandleId,
        path: GeometryHandleId,
        reply: oneshot::Sender<Result<(GeometryHandle, SweepOpHistoryRecords), GeometryError>>,
    },
    /// Loft through `profiles` with per-section history.
    LoftWithHistory {
        profiles: Vec<GeometryHandleId>,
        reply: oneshot::Sender<Result<(GeometryHandle, LoftOpHistoryRecords), GeometryError>>,
    },
    /// Execute `op` with attribute history: forwards to
    /// `OcctKernel::execute_with_history`, the single op-to-history lowering.
    /// The single dispatch site (`Engine::execute_realization_ops`) routes
    /// through this variant.
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
/// the trait no-op, answered on the calling thread without touching the
/// channel, so they are safe from both sync and async contexts.
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
    /// kernel-thread channel, including how that method classifies the
    /// result handle's `BRepKind` (not a fixed `Solid`: a multi-body or
    /// empty result is `Compound`).
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
    /// kernel-thread channel, including how that method classifies the
    /// result handle's `BRepKind` (not a fixed `Solid`: a multi-body or
    /// empty result is `Compound`).
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
    /// kernel-thread channel, including how that method classifies the
    /// result handle's `BRepKind` (not a fixed `Solid`: a multi-body or
    /// empty result is `Compound`).
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
    /// channel.
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
    /// kernel-thread channel. An empty `edges` slice is rejected — the
    /// all-edges path is [`Self::fillet_with_history`].
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
    /// channel.
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
    /// kernel-thread channel. An empty `edges` slice is rejected — the
    /// all-edges path is [`Self::chamfer_with_history`].
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
    /// kernel-thread channel.
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
    /// Forwards to `OcctKernel::execute_with_history`, the single
    /// op-to-history lowering: a history op returns its op-specific
    /// `AttributeHistory` variant, and every other op returns
    /// `AttributeHistory::None`, functionally identical to
    /// [`OcctKernelHandle::execute`].
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

    /// Extrude `profile` by `distance` (SI metres) via
    /// `OcctKernel::extrude_with_history` on the kernel thread, returning
    /// `(handle_id, SweepOpHistoryRecords)` in the call shape of
    /// [`boolean_fuse_with_history`](Self::boolean_fuse_with_history).
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
        let (handle, records) = self.send_request_blocking(
            |reply| OcctRequest::ExtrudeWithHistory {
                profile,
                distance,
                reply,
            },
            || GeometryError::OperationFailed("kernel thread died".into()),
        )??;
        Ok((handle.id, records))
    }

    /// Revolve `profile` via `OcctKernel::revolve_with_history` on the kernel
    /// thread, returning `(handle_id, SweepOpHistoryRecords)`.
    ///
    /// `axis_origin` and `axis_dir` are 3-element arrays in metres /
    /// (dimensionless direction); `angle_rad` is the revolve angle in
    /// radians. Validation thresholds match
    /// [`OcctKernel::revolve_with_history`] (axis must be finite +
    /// magnitude > AXIS_MAG_SQ_MIN; angle must be finite + magnitude >
    /// ANGLE_ABS_MIN).
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
        let (handle, records) = self.send_request_blocking(
            |reply| OcctRequest::RevolveWithHistory {
                profile,
                axis_origin,
                axis_dir,
                angle_rad,
                reply,
            },
            || GeometryError::OperationFailed("kernel thread died".into()),
        )??;
        Ok((handle.id, records))
    }

    /// Sweep `profile` along `path` via `OcctKernel::sweep_with_history` on
    /// the kernel thread, returning `(handle_id, SweepOpHistoryRecords)`.
    ///
    /// Sweep is single-parent — the profile is the operand whose
    /// sub-shapes propagate to the result; the path is the spine along
    /// which the profile is swept. `parent_index` in every record is
    /// `0`. `start_cap_face_indices` carries the FirstShape() face
    /// index (profile-as-placed); `end_cap_face_indices` carries the
    /// LastShape() face index (profile at the spine end).
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
        let (handle, records) = self.send_request_blocking(
            |reply| OcctRequest::SweepWithHistory {
                profile,
                path,
                reply,
            },
            || GeometryError::OperationFailed("kernel thread died".into()),
        )??;
        Ok((handle.id, records))
    }

    /// Loft through `profiles` via `OcctKernel::loft_with_history` on the
    /// kernel thread, returning `(handle_id, LoftOpHistoryRecords)`.
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
    /// `GeometryError::OperationFailed("Loft requires at least 2 profiles")`
    /// when `profiles.len() < 2`. Mirrors the `GeometryOp::Loft` arm of
    /// `OcctKernel::execute` and the C++ `make_loft_with_history`
    /// validation.
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
        let (handle, records) = self.send_request_blocking(
            |reply| OcctRequest::LoftWithHistory {
                profiles: profiles.to_vec(),
                reply,
            },
            || GeometryError::OperationFailed("kernel thread died".into()),
        )??;
        Ok((handle.id, records))
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
                    OcctRequest::ExtrudeWithHistory {
                        profile,
                        distance,
                        reply,
                    } => {
                        let _ = reply.send(kernel.extrude_with_history(profile, distance));
                    }
                    OcctRequest::RevolveWithHistory {
                        profile,
                        axis_origin,
                        axis_dir,
                        angle_rad,
                        reply,
                    } => {
                        let result =
                            kernel.revolve_with_history(profile, axis_origin, axis_dir, angle_rad);
                        let _ = reply.send(result);
                    }
                    OcctRequest::SweepWithHistory {
                        profile,
                        path,
                        reply,
                    } => {
                        let _ = reply.send(kernel.sweep_with_history(profile, path));
                    }
                    OcctRequest::LoftWithHistory { profiles, reply } => {
                        let _ = reply.send(kernel.loft_with_history(&profiles));
                    }
                    OcctRequest::ExecuteWithHistory { op, reply } => {
                        let _ = reply.send(kernel.execute_with_history(&op));
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

    /// Reset the kernel thread's `OcctKernel`: evict every resident native
    /// shape and clear the derived caches while keeping `next_id` monotonic
    /// (see `OcctKernel::reset`), bounding OCCT native memory across GUI
    /// whole-file reloads.
    ///
    /// Routed over the actor channel because `OcctKernel` is `!Send` and lives
    /// on the dedicated OS thread. Blocks until the kernel thread confirms;
    /// safe from both sync and async contexts (via `send_recv`). A dead kernel
    /// thread is a silent no-op — there is nothing left to reset. Takes `&self`
    /// because the actor channel sender only needs a shared borrow; the
    /// `GeometryKernel::reset` override takes `&mut self` and delegates here.
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

/// The kernel thread's `OcctKernel` is stateless across warm starts, so the
/// handle answers directly with no channel round-trip.
impl WarmStartable for OcctKernelHandle {
    fn warm_state(&self) -> Option<OpaqueState> {
        None
    }

    fn with_warm_state(&mut self, _state: OpaqueState) {}
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
    /// that surfaces kernel-emitted [`AttributeHistory`] from
    /// `OcctKernel::execute_with_history`. Delegates to the inherent
    /// `execute_with_history` (which only needs `&self`).
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
