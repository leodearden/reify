//! Stub types for when OCCT libraries are not available at build time.
//!
//! These provide the same public API surface as the real OcctKernel and
//! OcctKernelHandle, but all operations return errors. This allows
//! downstream crates to compile and fail gracefully at runtime.

use reify_types::{
    ExportError, ExportFormat, GeometryError, GeometryHandle, GeometryHandleId, GeometryKernel,
    GeometryOp, GeometryQuery, Mesh, OpaqueState, QueryError, TessError, Value, WarmStartable,
};

/// Stub topology cache build counts (OCCT not available).
#[derive(Debug, PartialEq, Eq)]
pub struct TopologyCacheBuildCounts {
    pub face_map_builds: u32,
    pub edge_map_builds: u32,
    pub edge_face_map_builds: u32,
}

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

    /// Returns [`GeometryError::InvalidReference`] for every handle.
    ///
    /// The stub registers no shapes, so every handle is unknown by definition.
    /// This matches the real impl's documented contract (see `lib.rs`
    /// `topology_cache_build_counts`), which also returns `InvalidReference`
    /// for unknown handles via `get_shape`. Returning the same error variant
    /// keeps callers that pattern-match on `InvalidReference` compatible
    /// across `has_occt` and `!has_occt` builds without special-casing.
    pub fn topology_cache_build_counts(
        &self,
        handle: GeometryHandleId,
    ) -> Result<TopologyCacheBuildCounts, GeometryError> {
        Err(GeometryError::InvalidReference(handle))
    }

    /// Stub topology-extraction selector — always errors because OCCT is
    /// unavailable. Mirrors the real `OcctKernel::extract_edges` signature
    /// so call sites compile under both `has_occt` and `!has_occt`.
    pub fn extract_edges(
        &mut self,
        _handle: GeometryHandleId,
    ) -> Result<Vec<GeometryHandleId>, QueryError> {
        Err(QueryError::QueryFailed(NOT_AVAILABLE.into()))
    }

    /// Stub topology-extraction selector — always errors because OCCT is
    /// unavailable. Mirrors the real `OcctKernel::extract_faces` signature
    /// so call sites compile under both `has_occt` and `!has_occt`.
    pub fn extract_faces(
        &mut self,
        _handle: GeometryHandleId,
    ) -> Result<Vec<GeometryHandleId>, QueryError> {
        Err(QueryError::QueryFailed(NOT_AVAILABLE.into()))
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

    fn tessellate(&self, handle: GeometryHandleId, tolerance: f64) -> Result<Mesh, TessError> {
        OcctKernelHandle::tessellate(self, handle, tolerance)
    }

    /// Override the trait default to surface the OCCT-unavailable message
    /// (matches the inherent stub `OcctKernel::extract_edges`).
    fn extract_edges(
        &mut self,
        _handle: GeometryHandleId,
    ) -> Result<Vec<GeometryHandleId>, QueryError> {
        Err(QueryError::QueryFailed(NOT_AVAILABLE.into()))
    }

    /// Override the trait default to surface the OCCT-unavailable message
    /// (matches the inherent stub `OcctKernel::extract_faces`).
    fn extract_faces(
        &mut self,
        _handle: GeometryHandleId,
    ) -> Result<Vec<GeometryHandleId>, QueryError> {
        Err(QueryError::QueryFailed(NOT_AVAILABLE.into()))
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
        ExportFormat, GeometryError, GeometryHandleId, GeometryKernel, GeometryOp, Value,
        WarmStartable,
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
        let handle = OcctKernelHandle::spawn();
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

    #[test]
    fn stub_kernel_topology_cache_build_counts_returns_invalid_reference() {
        let kernel = OcctKernel::new();
        let bad_id = GeometryHandleId(42);
        match kernel.topology_cache_build_counts(bad_id) {
            Err(GeometryError::InvalidReference(id)) => {
                assert_eq!(id, bad_id, "InvalidReference should carry the bad handle id");
            }
            Ok(c) => panic!(
                "expected Err(InvalidReference) for unknown handle, got Ok({:?})",
                c
            ),
            Err(other) => panic!(
                "expected Err(InvalidReference) for unknown handle, got Err({:?})",
                other
            ),
        }
    }

    /// Helper: assert a string mentions "OCCT" or "not available", matching
    /// the stub crate's `NOT_AVAILABLE` constant verbatim.
    fn assert_stub_message(msg: &str) {
        assert!(
            msg.contains("OCCT") || msg.contains("not available"),
            "stub error message should mention OCCT or 'not available', got: {msg}"
        );
    }

    #[test]
    fn stub_kernel_extract_edges_returns_error() {
        let mut kernel = OcctKernel::new();
        let result = kernel.extract_edges(GeometryHandleId(1));
        let err = result.expect_err("stub extract_edges should error");
        assert_stub_message(&format!("{err:?}"));
    }

    #[test]
    fn stub_kernel_extract_faces_returns_error() {
        let mut kernel = OcctKernel::new();
        let result = kernel.extract_faces(GeometryHandleId(1));
        let err = result.expect_err("stub extract_faces should error");
        assert_stub_message(&format!("{err:?}"));
    }

    #[test]
    fn stub_kernel_query_edge_length_returns_error() {
        let kernel = OcctKernel::new();
        let result =
            kernel.query(&reify_types::GeometryQuery::EdgeLength(GeometryHandleId(1)));
        let err = result.expect_err("stub query EdgeLength should error");
        assert_stub_message(&format!("{err:?}"));
    }

    #[test]
    fn stub_kernel_query_face_normal_returns_error() {
        let kernel = OcctKernel::new();
        let result =
            kernel.query(&reify_types::GeometryQuery::FaceNormal(GeometryHandleId(1)));
        let err = result.expect_err("stub query FaceNormal should error");
        assert_stub_message(&format!("{err:?}"));
    }

    #[test]
    fn stub_kernel_query_edge_tangent_returns_error() {
        let kernel = OcctKernel::new();
        let result =
            kernel.query(&reify_types::GeometryQuery::EdgeTangent(GeometryHandleId(1)));
        let err = result.expect_err("stub query EdgeTangent should error");
        assert_stub_message(&format!("{err:?}"));
    }
}
