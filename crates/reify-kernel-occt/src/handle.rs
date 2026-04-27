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

use reify_types::{
    ExportError, ExportFormat, GeometryError, GeometryHandle, GeometryHandleId, GeometryKernel,
    GeometryOp, GeometryQuery, Mesh, OpaqueState, QueryError, TessError, Value, WarmStartable,
    debug_assert_query_many_invariant,
};
use tokio::sync::{mpsc, oneshot};

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
    ExtractEdges {
        handle: GeometryHandleId,
        reply: oneshot::Sender<Result<Vec<GeometryHandleId>, QueryError>>,
    },
    ExtractFaces {
        handle: GeometryHandleId,
        reply: oneshot::Sender<Result<Vec<GeometryHandleId>, QueryError>>,
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
        let (reply_tx, reply_rx) = oneshot::channel();
        self.tx
            .blocking_send(OcctRequest::Export {
                handle,
                format,
                reply: reply_tx,
            })
            .map_err(|_| ExportError::IoError("kernel thread died".into()))?;
        let bytes = reply_rx
            .blocking_recv()
            .map_err(|_| ExportError::IoError("kernel thread died".into()))??;
        writer
            .write_all(&bytes)
            .map_err(|e| ExportError::IoError(e.to_string()))
    }

    /// Run a query against a geometry handle on the kernel thread.
    ///
    /// # Panics
    ///
    /// Panics if called from within a tokio async execution context. Use
    /// [`query_async`](Self::query_async) instead.
    pub fn query(&self, query: &GeometryQuery) -> Result<Value, QueryError> {
        self.send_request_blocking(
            |reply| OcctRequest::Query { query: query.clone(), reply },
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
    pub fn query_many(
        &self,
        queries: &[GeometryQuery],
    ) -> Result<Vec<Value>, QueryError> {
        // Empty-batch fast path: skip the actor channel round-trip
        // entirely. The kernel-thread arm would itself produce
        // `Ok(Vec::new())`, so the result is identical.
        if queries.is_empty() {
            return Ok(Vec::new());
        }
        let (reply_tx, reply_rx) = oneshot::channel();
        self.tx
            .blocking_send(OcctRequest::QueryMany {
                queries: queries.to_vec(),
                reply: reply_tx,
            })
            .map_err(|_| QueryError::QueryFailed("kernel thread died".into()))?;
        let reply: Vec<Value> = reply_rx
            .blocking_recv()
            .map_err(|_| QueryError::QueryFailed("kernel thread died".into()))??;
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
            |reply| OcctRequest::Tessellate { handle, tolerance, reply },
            || TessError::TessellationFailed("kernel thread died".into()),
        )?
    }

    /// Extract the unique edges of a shape, storing each as a new handle on
    /// the kernel thread, and return the resulting list of handle ids.
    ///
    /// # Panics
    ///
    /// Panics if called from within a tokio async execution context. Use
    /// [`extract_edges_async`](Self::extract_edges_async) instead.
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

    /// Execute a geometry operation on the kernel thread.
    ///
    /// # Panics
    ///
    /// Panics if called from within a tokio async execution context. Use
    /// [`execute_async`](Self::execute_async) instead.
    pub fn execute(&self, op: &GeometryOp) -> Result<GeometryHandle, GeometryError> {
        self.send_request_blocking(
            |reply| OcctRequest::Execute { op: Box::new(op.clone()), reply },
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
                    OcctRequest::ExtractEdges { handle, reply } => {
                        let result = kernel.extract_edges(handle);
                        let _ = reply.send(result);
                    }
                    OcctRequest::ExtractFaces { handle, reply } => {
                        let result = kernel.extract_faces(handle);
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
    /// map-channel-died-twice boilerplate for synchronous inherent methods.
    ///
    /// Panics if called from within a tokio async execution context; use
    /// [`send_request_async`](Self::send_request_async) instead.
    fn send_request_blocking<Resp, E>(
        &self,
        build_req: impl FnOnce(oneshot::Sender<Resp>) -> OcctRequest,
        chan_died: impl Fn() -> E,
    ) -> Result<Resp, E> {
        let (reply_tx, reply_rx) = oneshot::channel();
        self.tx.blocking_send(build_req(reply_tx)).map_err(|_| chan_died())?;
        reply_rx.blocking_recv().map_err(|_| chan_died())
    }

    /// Compress the build-channel + async-send + await-recv +
    /// map-channel-died-twice boilerplate for async inherent methods.
    ///
    /// Safe to call from within a tokio async execution context.
    async fn send_request_async<Resp, E>(
        &self,
        build_req: impl FnOnce(oneshot::Sender<Resp>) -> OcctRequest,
        chan_died: impl Fn() -> E,
    ) -> Result<Resp, E> {
        let (reply_tx, reply_rx) = oneshot::channel();
        self.tx.send(build_req(reply_tx)).await.map_err(|_| chan_died())?;
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
            |reply| OcctRequest::Execute { op: Box::new(op.clone()), reply },
            || GeometryError::OperationFailed("kernel thread died".into()),
        )
        .await?
    }

    /// Run a query against a geometry handle on the kernel thread (async version).
    ///
    /// Safe to call from within a tokio async execution context.
    pub async fn query_async(&self, query: &GeometryQuery) -> Result<Value, QueryError> {
        self.send_request_async(
            |reply| OcctRequest::Query { query: query.clone(), reply },
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
        let (reply_tx, reply_rx) = oneshot::channel();
        self.tx
            .send(OcctRequest::Export {
                handle,
                format,
                reply: reply_tx,
            })
            .await
            .map_err(|_| ExportError::IoError("kernel thread died".into()))?;
        reply_rx
            .await
            .map_err(|_| ExportError::IoError("kernel thread died".into()))?
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
            |reply| OcctRequest::Tessellate { handle, tolerance, reply },
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

    /// Extract warm-start state from the kernel thread (async version).
    ///
    /// Safe to call from within a tokio async execution context.
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

    fn query(&self, query: &GeometryQuery) -> Result<Value, QueryError> {
        OcctKernelHandle::query(self, query)
    }

    /// Override the trait default with a real channel-routed batched
    /// implementation. Delegates to the inherent `query_many` (which
    /// only needs `&self`).
    fn query_many(&self, queries: &[GeometryQuery]) -> Result<Vec<Value>, QueryError> {
        OcctKernelHandle::query_many(self, queries)
    }

    fn export(
        &self,
        handle: GeometryHandleId,
        format: ExportFormat,
        writer: &mut dyn std::io::Write,
    ) -> Result<(), ExportError> {
        OcctKernelHandle::export(self, handle, format, writer)
    }

    fn tessellate(&self, handle: GeometryHandleId, tolerance: f64) -> Result<Mesh, TessError> {
        OcctKernelHandle::tessellate(self, handle, tolerance)
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
}

#[cfg(all(test, has_occt))]
mod tests {
    use reify_types::{GeometryHandleId, GeometryOp, GeometryQuery, ReprKind, Value};

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
        assert_eq!(result.repr, ReprKind::Solid);
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
            .query(&reify_types::GeometryQuery::Volume(gh.id))
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
        let result = handle.query(&reify_types::GeometryQuery::Volume(GeometryHandleId(999)));
        assert!(result.is_err());
        match result.unwrap_err() {
            reify_types::QueryError::InvalidHandle(id) => {
                assert_eq!(id, GeometryHandleId(999));
            }
            other => panic!("expected InvalidHandle, got {:?}", other),
        }
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
            reify_types::QueryError::InvalidHandle(id) => {
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
            .export(gh.id, reify_types::ExportFormat::Step, &mut buf)
            .unwrap();
        let content = String::from_utf8(buf).unwrap();
        assert!(
            content.contains("ISO-10303-21"),
            "STEP export should contain ISO-10303-21 header"
        );
    }

    #[test]
    fn export_unsupported_format_returns_error() {
        let handle = super::OcctKernelHandle::spawn();
        let op = GeometryOp::Box {
            width: Value::Real(10.0),
            height: Value::Real(20.0),
            depth: Value::Real(30.0),
        };
        let gh = handle.execute(&op).unwrap();
        let mut buf = Vec::new();
        let result = handle.export(gh.id, reify_types::ExportFormat::Stl, &mut buf);
        assert!(result.is_err());
    }

    #[test]
    fn handle_implements_geometry_kernel_trait() {
        use reify_types::GeometryKernel;
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
            distance: Value::Real(1.0),
        };
        let result = handle.execute(&chamfer_op);
        assert!(result.is_ok(), "chamfer should succeed, got: {:?}", result.unwrap_err());
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
            .export(chamfered.id, reify_types::ExportFormat::Step, &mut buf)
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
            reify_types::ExportFormat::Step,
            &mut buf,
        );
        assert!(result.is_err());
        match result.unwrap_err() {
            reify_types::ExportError::InvalidHandle(id) => {
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
            reify_types::TessError::InvalidHandle(id) => {
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
                radius: Value::Real(2.0),
            })
            .unwrap();
        assert_eq!(fillet_h.id, GeometryHandleId(4));

        // Query volume
        let vol = handle
            .query(&reify_types::GeometryQuery::Volume(fillet_h.id))
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
            .export(fillet_h.id, reify_types::ExportFormat::Step, &mut buf)
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
        assert_eq!(result.repr, ReprKind::Solid);
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
            reify_types::QueryError::InvalidHandle(id) => {
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
            .export_async(gh.id, reify_types::ExportFormat::Step)
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
            .export_async(GeometryHandleId(999), reify_types::ExportFormat::Step)
            .await;
        assert!(result.is_err());
        match result.unwrap_err() {
            reify_types::ExportError::InvalidHandle(id) => {
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
            .export_async(fillet_h.id, reify_types::ExportFormat::Step)
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
        use reify_types::WarmStartable;
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
        use reify_types::WarmStartable;
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
        use reify_types::WarmStartable;
        let handle = super::OcctKernelHandle::spawn();
        // No ops executed — warm_state should return None
        let state = handle.warm_state();
        assert!(state.is_none(), "empty kernel should have no warm state");
    }

    #[tokio::test]
    async fn warm_startable_trait_safe_in_async_context() {
        use reify_types::WarmStartable;
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
                            .export(gh.id, reify_types::ExportFormat::Step, &mut buf)
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
}
