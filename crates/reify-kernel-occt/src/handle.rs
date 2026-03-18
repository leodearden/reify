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
    GeometryOp, GeometryQuery, Mesh, QueryError, TessError, Value,
};
use tokio::sync::{mpsc, oneshot};

/// Requests sent from `OcctKernelHandle` to the dedicated kernel thread.
enum OcctRequest {
    Execute {
        op: GeometryOp,
        reply: oneshot::Sender<Result<GeometryHandle, GeometryError>>,
    },
    Query {
        query: GeometryQuery,
        reply: oneshot::Sender<Result<Value, QueryError>>,
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
}

/// Thread-safe handle to an OCCT kernel running on a dedicated thread.
///
/// All geometry operations are serialized through a channel to the kernel
/// thread. The handle is `Send + Sync` and implements `GeometryKernel`.
pub struct OcctKernelHandle {
    tx: mpsc::Sender<OcctRequest>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl OcctKernelHandle {
    /// Run a query against a geometry handle on the kernel thread.
    pub fn query(&self, query: &GeometryQuery) -> Result<Value, QueryError> {
        let (reply_tx, reply_rx) = oneshot::channel();
        self.tx
            .blocking_send(OcctRequest::Query {
                query: query.clone(),
                reply: reply_tx,
            })
            .map_err(|_| QueryError::QueryFailed("kernel thread died".into()))?;
        reply_rx
            .blocking_recv()
            .map_err(|_| QueryError::QueryFailed("kernel thread died".into()))?
    }

    /// Execute a geometry operation on the kernel thread.
    pub fn execute(&self, op: &GeometryOp) -> Result<GeometryHandle, GeometryError> {
        let (reply_tx, reply_rx) = oneshot::channel();
        self.tx
            .blocking_send(OcctRequest::Execute {
                op: op.clone(),
                reply: reply_tx,
            })
            .map_err(|_| GeometryError::OperationFailed("kernel thread died".into()))?;
        reply_rx
            .blocking_recv()
            .map_err(|_| GeometryError::OperationFailed("kernel thread died".into()))?
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
                    OcctRequest::Export {
                        handle,
                        format,
                        reply,
                    } => {
                        let mut buf = Vec::new();
                        let result = kernel
                            .export(handle, format, &mut buf)
                            .map(|()| buf);
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
                }
            }
            // Channel closed (sender dropped) → exit cleanly.
        });

        Self {
            tx,
            thread: Some(thread),
        }
    }
}

#[cfg(test)]
mod tests {
    use reify_types::{GeometryHandleId, GeometryOp, ReprKind, Value};

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
        let result =
            handle.query(&reify_types::GeometryQuery::Volume(GeometryHandleId(999)));
        assert!(result.is_err());
        match result.unwrap_err() {
            reify_types::QueryError::InvalidHandle(id) => {
                assert_eq!(id, GeometryHandleId(999));
            }
            other => panic!("expected InvalidHandle, got {:?}", other),
        }
    }
}
