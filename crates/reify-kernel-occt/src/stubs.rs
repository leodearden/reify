//! Stub types for when OCCT libraries are not available at build time.
//!
//! These provide the same public API surface as the real OcctKernel and
//! OcctKernelHandle, but all operations return errors. This allows
//! downstream crates to compile and fail gracefully at runtime.

use reify_types::{
    ExportError, ExportFormat, GeometryError, GeometryHandle, GeometryHandleId, GeometryKernel,
    GeometryOp, GeometryQuery, Mesh, OpaqueState, QueryError, TessError, Value, WarmStartable,
};

const NOT_AVAILABLE: &str = "OCCT libraries not available at build time";

/// Stub OpenCASCADE kernel — all operations return errors.
pub struct OcctKernel {
    _private: (),
}

impl OcctKernel {
    pub fn new() -> Self {
        Self { _private: () }
    }

    pub fn execute(&mut self, _op: &GeometryOp) -> Result<GeometryHandle, GeometryError> {
        Err(GeometryError::OperationFailed(NOT_AVAILABLE.into()))
    }

    pub fn query(&self, _query: &GeometryQuery) -> Result<Value, QueryError> {
        Err(QueryError::QueryFailed(NOT_AVAILABLE.into()))
    }

    pub fn export(
        &self,
        _handle: GeometryHandleId,
        _format: ExportFormat,
        _writer: &mut dyn std::io::Write,
    ) -> Result<(), ExportError> {
        Err(ExportError::FormatError(NOT_AVAILABLE.into()))
    }

    pub fn tessellate(
        &self,
        _handle: GeometryHandleId,
        _tolerance: f64,
    ) -> Result<Mesh, TessError> {
        Err(TessError::TessellationFailed(NOT_AVAILABLE.into()))
    }
}

impl Default for OcctKernel {
    fn default() -> Self {
        Self::new()
    }
}

impl WarmStartable for OcctKernel {
    fn warm_state(&self) -> Option<OpaqueState> {
        None
    }

    fn with_warm_state(&mut self, _state: OpaqueState) {
        // No-op: OCCT not available, silently ignore per trait contract.
    }
}

/// Stub thread-safe handle — implements GeometryKernel with error returns.
pub struct OcctKernelHandle {
    _private: (),
}

// Safety: stub contains no mutable state, is trivially Send + Sync.
unsafe impl Send for OcctKernelHandle {}
unsafe impl Sync for OcctKernelHandle {}

impl OcctKernelHandle {
    /// Create a stub handle (no thread is spawned).
    pub fn spawn() -> Self {
        Self { _private: () }
    }

    pub fn execute(&self, _op: &GeometryOp) -> Result<GeometryHandle, GeometryError> {
        Err(GeometryError::OperationFailed(NOT_AVAILABLE.into()))
    }

    pub fn query(&self, _query: &GeometryQuery) -> Result<Value, QueryError> {
        Err(QueryError::QueryFailed(NOT_AVAILABLE.into()))
    }

    pub fn export(
        &self,
        _handle: GeometryHandleId,
        _format: ExportFormat,
        _writer: &mut dyn std::io::Write,
    ) -> Result<(), ExportError> {
        Err(ExportError::IoError(NOT_AVAILABLE.into()))
    }

    pub fn tessellate(
        &self,
        _handle: GeometryHandleId,
        _tolerance: f64,
    ) -> Result<Mesh, TessError> {
        Err(TessError::TessellationFailed(NOT_AVAILABLE.into()))
    }

    /// No-op shutdown (no thread to join).
    pub async fn shutdown(self) {}
}

impl WarmStartable for OcctKernelHandle {
    fn warm_state(&self) -> Option<OpaqueState> {
        None
    }

    fn with_warm_state(&mut self, _state: OpaqueState) {
        // No-op: OCCT not available.
    }
}

impl GeometryKernel for OcctKernelHandle {
    fn execute(&mut self, op: &GeometryOp) -> Result<GeometryHandle, GeometryError> {
        OcctKernelHandle::execute(self, op)
    }

    fn query(&self, query: &GeometryQuery) -> Result<Value, QueryError> {
        OcctKernelHandle::query(self, query)
    }

    fn export(
        &self,
        handle: GeometryHandleId,
        format: ExportFormat,
        writer: &mut dyn std::io::Write,
    ) -> Result<(), ExportError> {
        OcctKernelHandle::export(self, handle, format, writer)
    }

    fn tessellate(
        &self,
        handle: GeometryHandleId,
        tolerance: f64,
    ) -> Result<Mesh, TessError> {
        OcctKernelHandle::tessellate(self, handle, tolerance)
    }
}

impl Drop for OcctKernelHandle {
    fn drop(&mut self) {
        // No-op: no thread to join.
    }
}

#[cfg(all(test, not(has_occt)))]
mod tests {
    use super::*;
    use reify_types::{
        ExportFormat, GeometryHandleId, GeometryKernel, GeometryOp, Value, WarmStartable,
    };

    #[test]
    fn stub_kernel_new_succeeds() {
        let _kernel = OcctKernel::new();
    }

    #[test]
    fn stub_kernel_execute_returns_error() {
        let mut kernel = OcctKernel::new();
        let result = kernel.execute(&GeometryOp::Box {
            width: Value::Real(10.0),
            height: Value::Real(10.0),
            depth: Value::Real(10.0),
        });
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            format!("{err:?}").contains("OCCT"),
            "error should mention OCCT: {err:?}"
        );
    }

    #[test]
    fn stub_kernel_query_returns_error() {
        let kernel = OcctKernel::new();
        let result = kernel.query(&reify_types::GeometryQuery::Volume(GeometryHandleId(1)));
        assert!(result.is_err());
    }

    #[test]
    fn stub_kernel_export_returns_error() {
        let kernel = OcctKernel::new();
        let mut buf = Vec::new();
        let result = kernel.export(GeometryHandleId(1), ExportFormat::Step, &mut buf);
        assert!(result.is_err());
    }

    #[test]
    fn stub_kernel_tessellate_returns_error() {
        let kernel = OcctKernel::new();
        let result = kernel.tessellate(GeometryHandleId(1), 0.1);
        assert!(result.is_err());
    }

    #[test]
    fn stub_kernel_warm_state_returns_none() {
        let kernel = OcctKernel::new();
        assert!(kernel.warm_state().is_none());
    }

    #[test]
    fn stub_handle_spawn_succeeds() {
        let _handle = OcctKernelHandle::spawn();
    }

    #[test]
    fn stub_handle_execute_returns_error() {
        let mut handle = OcctKernelHandle::spawn();
        let result = handle.execute(&GeometryOp::Box {
            width: Value::Real(10.0),
            height: Value::Real(10.0),
            depth: Value::Real(10.0),
        });
        assert!(result.is_err());
    }

    #[test]
    fn stub_handle_is_geometry_kernel() {
        let handle = OcctKernelHandle::spawn();
        // Verify it can be used as Box<dyn GeometryKernel>
        let _boxed: Box<dyn GeometryKernel> = Box::new(handle);
    }
}
